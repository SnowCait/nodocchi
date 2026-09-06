use crate::action::{LegalAction, prefer_black_five_for_action};
use crate::agent::Agent;
use crate::call_decision::{CallDecisionDiagnostic, evaluate_call_decision};
use crate::combined_defense::CombinedDefenseCategory;
use crate::context::GameContext;
use crate::decision_timing::{DecisionPhase, DecisionPhaseTimer, TimedAgentAction};
use crate::defense::{DefenseFallbackKind, log_defense_fallback_evaluation};
use crate::discard_selection::{
    DiscardActionSelection, select_discard_action_with_diagnostic,
    select_discard_action_with_evaluation_instrumented,
};
use crate::fold_defense::{FoldDefenseKind, evaluate_fold_defense, evaluate_reach_defense};
use crate::open_hand_defense::OpenHandDefenseCategory;
use crate::push_pull::{
    PushPullDecision, PushPullInputs, PushPullMode, decide_push_pull, log_push_pull_decision,
    push_pull_inputs_from_threat_facts,
};
use crate::reach_decision::{ReachDecision, ReachDecisionDiagnostic, decide_reach};
use crate::ryukyoku_decision::{RyukyokuDecisionDiagnostic, evaluate_ryukyoku_decision};
use crate::shanten_diagnostic::{
    DecisionDiagnostics, DiagnosticOptions, ShantenDecisionDiagnostic, diagnose_shanten_decision,
    diagnose_shanten_decision_with_options,
};
use crate::threat::player_threat_facts_from_context;

const AGENT_DECISION_LOG_TARGET: &str = "bot_core::agent_decision";

/// 最終 action がどの経路で選ばれたかを表す診断。プロトコル非依存。
///
/// `ShantenAgent::act()` が実際に通った経路そのものであり、診断用の別判断ロジックではない。
///
/// 防御 fallback はリーチ者向けの [`Self::DefenseFallback`]、非リーチ副露相手向けの
/// [`Self::OpenHandDefenseFallback`]、両者が同時にいる複合 threat 向けの
/// [`Self::CombinedThreatDefenseFallback`] を別の経路として区別する。リーチ者向けの現物
/// ([`DefenseFallbackKind::Genbutsu`])、全 OpenHand target へのロン安全
/// ([`OpenHandDefenseCategory::SafeAgainstAllTargets`])、全 threat へのロン安全
/// ([`CombinedDefenseCategory::SafeAgainstAllThreats`]) は根拠が違うため、同じ種別へ押し込まない。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentActionSource {
    Hora,
    Ryukyoku,
    Call,
    Reach,
    NormalDiscard,
    DefenseFallback(DefenseFallbackKind),
    OpenHandDefenseFallback(OpenHandDefenseCategory),
    CombinedThreatDefenseFallback(CombinedDefenseCategory),
    LegalDahaiFallback,
    None,
}

impl AgentActionSource {
    // 防御 kind を分離して扱えるよう、source ラベルは kind を含めない固定名にする。
    fn label(&self) -> &'static str {
        match self {
            AgentActionSource::Hora => "Hora",
            AgentActionSource::Ryukyoku => "Ryukyoku",
            AgentActionSource::Call => "Call",
            AgentActionSource::Reach => "Reach",
            AgentActionSource::NormalDiscard => "NormalDiscard",
            AgentActionSource::DefenseFallback(_) => "DefenseFallback",
            AgentActionSource::OpenHandDefenseFallback(_) => "OpenHandDefenseFallback",
            AgentActionSource::CombinedThreatDefenseFallback(_) => "CombinedThreatDefenseFallback",
            AgentActionSource::LegalDahaiFallback => "LegalDahaiFallback",
            AgentActionSource::None => "None",
        }
    }

    /// リーチ者向けの防御 fallback 経路で選ばれた場合のその種別。他の経路では `None`。
    pub fn defense_kind(&self) -> Option<DefenseFallbackKind> {
        match self {
            AgentActionSource::DefenseFallback(kind) => Some(*kind),
            _ => None,
        }
    }

    /// 非リーチ副露相手向けの防御 fallback 経路で選ばれた場合のその大分類。
    /// 他の経路では `None`。
    pub fn open_hand_defense_category(&self) -> Option<OpenHandDefenseCategory> {
        match self {
            AgentActionSource::OpenHandDefenseFallback(category) => Some(*category),
            _ => None,
        }
    }

    /// 複合 threat 向けの防御 fallback 経路で選ばれた場合のその大分類。他の経路では `None`。
    pub fn combined_defense_category(&self) -> Option<CombinedDefenseCategory> {
        match self {
            AgentActionSource::CombinedThreatDefenseFallback(category) => Some(*category),
            _ => None,
        }
    }
}

/// `ShantenAgent` が下した最終判断と、その選択経路・ログ用文脈をまとめた内部表現。
///
/// ログや diagnostics assembly のために判断ロジックを再実行しないよう、action 選択の過程で
/// 得た情報を保持する。
/// `push_pull` / `push_pull_inputs` / `normal_discard` は Hora / Ryukyoku / 鳴きの早期 return では
/// `None`。`call` は合法な Chi / Pon が1件も無い局面では `None`。`reach` はリーチを検討する Push
/// mode 以外では `None`。`ryukyoku` は `LegalAction::Ryukyoku` が合法だった局面だけ `Some` で、
/// Hora で早期終了した場合は検討自体を行わないので `None`。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AgentDecision {
    pub(crate) action: LegalAction,
    pub(crate) source: AgentActionSource,
    pub(crate) push_pull_inputs: Option<PushPullInputs>,
    pub(crate) push_pull: Option<PushPullDecision>,
    pub(crate) normal_discard: Option<LegalAction>,
    pub(crate) reach: Option<ReachDecisionDiagnostic>,
    pub(crate) call: Option<CallDecisionDiagnostic>,
    pub(crate) ryukyoku: Option<RyukyokuDecisionDiagnostic>,
}

#[derive(Debug, Default)]
pub struct ShantenAgent;

impl ShantenAgent {
    /// `act()` と同じ判断を行い、その過程を構造化診断として返す。
    ///
    /// 判断経路は `act()` と共通の内部 helper を通るため、
    /// `diagnose(...).selected_action == ShantenAgent::act(...)` が常に成り立つ。診断用の別判断
    /// ロジックは持たない。契約の詳細は [`ShantenDecisionDiagnostic`] を参照。
    ///
    /// 解析専用の追加情報(候補ごとの形の内訳、全合法 Dahai の防御候補評価など)はこの経路
    /// でのみ構築する。通常の `act()` では計算しない。
    pub fn diagnose(
        context: &GameContext,
        legal_actions: &[LegalAction],
    ) -> ShantenDecisionDiagnostic {
        diagnose_shanten_decision(context, legal_actions)
    }

    /// 追加診断を指定して `act()` と同じ判断を行い、その過程を構造化診断として返す。
    ///
    /// `options` は解析専用の追加情報を構築するかどうかだけを決め、選択結果には影響しない。
    /// `diagnose_with_options(...).selected_action == ShantenAgent::act(...)` は `options` に
    /// かかわらず常に成り立つ。
    pub fn diagnose_with_options(
        context: &GameContext,
        legal_actions: &[LegalAction],
        options: DiagnosticOptions,
    ) -> ShantenDecisionDiagnostic {
        diagnose_shanten_decision_with_options(context, legal_actions, options)
    }

    /// `act()` と同じ判断を1回だけ行い、phase ごとの実測時間を併せて返す。
    ///
    /// 判断経路は `act()` と共通で、計測のために判断を再実行しない。計測を有効にしても
    /// 選択結果は変わらず、`act_with_phase_timing(...).action == ShantenAgent::act(...)` が
    /// 常に成り立つ。
    pub fn act_with_phase_timing(
        &mut self,
        ctx: &GameContext,
        legal_actions: &[LegalAction],
    ) -> TimedAgentAction {
        let mut timing = DecisionPhaseTimer::started();
        let decision = self.decide_instrumented(
            ctx,
            legal_actions,
            &mut DecisionDiagnostics::disabled(),
            &mut timing,
        );
        let two_shanten_self_tsumo_candidates = timing.take_two_shanten_self_tsumo_candidates();
        let phases = timing.finish();
        log_agent_decision(&decision);
        TimedAgentAction {
            action: decision.action,
            phases,
            two_shanten_self_tsumo_candidates,
        }
    }

    // 最終 action と選択経路を1回で決める内部 helper。act() はこの結果を返し、
    // 共通箇所で agent decision ログを1件だけ出す。
    pub(crate) fn decide(&self, ctx: &GameContext, legal_actions: &[LegalAction]) -> AgentDecision {
        self.decide_with_diagnostics(ctx, legal_actions, &mut DecisionDiagnostics::disabled())
    }

    pub(crate) fn decide_with_diagnostics(
        &self,
        ctx: &GameContext,
        legal_actions: &[LegalAction],
        diagnostics: &mut DecisionDiagnostics,
    ) -> AgentDecision {
        self.decide_instrumented(
            ctx,
            legal_actions,
            diagnostics,
            &mut DecisionPhaseTimer::disabled(),
        )
    }

    // 判断経路の本体。act() と構造化診断はこの1本を共有し、diagnostics が有効な場合だけ
    // 解析専用の追加情報を収集する。追加情報の収集は action 選択に影響しない。
    //
    // `timing` は無効な場合に何もしない optional な計測で、判断の順序も内容も変えない。
    fn decide_instrumented(
        &self,
        ctx: &GameContext,
        legal_actions: &[LegalAction],
        diagnostics: &mut DecisionDiagnostics,
        timing: &mut DecisionPhaseTimer,
    ) -> AgentDecision {
        if let Some(action) = legal_actions
            .iter()
            .find(|a| matches!(a, LegalAction::Hora))
        {
            return AgentDecision {
                action: action.clone(),
                source: AgentActionSource::Hora,
                push_pull_inputs: None,
                push_pull: None,
                normal_discard: None,
                reach: None,
                call: None,
                ryukyoku: None,
            };
        }

        // 九種九牌。合法性は入力側が source of truth なので成立条件は再判定せず、宣言するか
        // 続行するかだけを決める。続行する場合は Ryukyoku を合法手から取り除かず、そのまま
        // 既存の判断へ進む。
        let legal_ryukyoku = legal_actions
            .iter()
            .find(|a| matches!(a, LegalAction::Ryukyoku));
        let ryukyoku = legal_ryukyoku.map(|_| evaluate_ryukyoku_decision(ctx));

        if let Some(action) = legal_ryukyoku
            && ryukyoku.is_some_and(|decision| decision.should_declare())
        {
            return AgentDecision {
                action: action.clone(),
                source: AgentActionSource::Ryukyoku,
                push_pull_inputs: None,
                push_pull: None,
                normal_discard: None,
                reach: None,
                call: None,
                ryukyoku,
            };
        }

        // 鳴き。和了・流局より後、通常打牌 / 押し引き / 防御より前に検討する。
        let call = evaluate_call_decision(ctx, legal_actions, diagnostics.is_enabled());
        if let Some(action) = call.as_ref().and_then(|call| call.selected.clone()) {
            return AgentDecision {
                action,
                source: AgentActionSource::Call,
                push_pull_inputs: None,
                push_pull: None,
                normal_discard: None,
                reach: None,
                call,
                ryukyoku,
            };
        }

        // 通常打牌の evaluation と action を一度だけ取得し、その evaluation を
        // 押し引き入力にも共有して二重計算を避ける。
        //
        // 脅威 facts もここで一度だけ構築し、押し引き入力へそのまま渡す。meld ごとの Vec を
        // 作らない軽量 facts なので、通常 act() で allocation は増えない。構造化診断は
        // この facts を再利用して full diagnostic を組み立てる。
        timing.enter(DecisionPhase::NormalDiscard);
        let discard_selection = self.select_normal_discard(ctx, legal_actions, diagnostics, timing);
        timing.enter(DecisionPhase::PostDiscard);

        let inputs = push_pull_inputs_from_threat_facts(
            ctx,
            player_threat_facts_from_context(ctx),
            discard_selection.evaluation.as_ref(),
            discard_selection.iishanten_forward_metrics,
            discard_selection.tenpai_wait.as_ref(),
            discard_selection.tenpai_offense_value,
            legal_actions,
        );
        let push_pull = decide_push_pull(&inputs);
        log_push_pull_decision(&push_pull, &inputs, discard_selection.action.as_ref());

        let normal_discard = discard_selection.action.clone();
        let mut reach = None;

        if let Some((action, source)) = self.select_action_for_push_pull_mode(
            push_pull.mode,
            ctx,
            legal_actions,
            &inputs,
            &discard_selection,
            &mut reach,
            diagnostics,
        ) {
            return AgentDecision {
                action,
                source,
                push_pull_inputs: Some(inputs),
                push_pull: Some(push_pull),
                normal_discard,
                reach,
                call,
                ryukyoku,
            };
        }

        if let Some(first_dahai) = legal_actions
            .iter()
            .find(|a| matches!(a, LegalAction::Dahai { .. }))
        {
            // 最初の合法 Dahai の牌種を選んだうえで、その牌種内では黒牌を優先する。
            // 別牌種との順番は変えないため、先頭牌種はそのまま維持される。
            let action = prefer_black_five_for_action(legal_actions, first_dahai);
            return AgentDecision {
                action: action.clone(),
                source: AgentActionSource::LegalDahaiFallback,
                push_pull_inputs: Some(inputs),
                push_pull: Some(push_pull),
                normal_discard,
                reach,
                call,
                ryukyoku,
            };
        }

        let action = legal_actions
            .iter()
            .find(|a| matches!(a, LegalAction::None))
            .cloned()
            .unwrap_or(LegalAction::None);
        AgentDecision {
            action,
            source: AgentActionSource::None,
            push_pull_inputs: Some(inputs),
            push_pull: Some(push_pull),
            normal_discard,
            reach,
            call,
            ryukyoku,
        }
    }

    // 通常打牌選択。production selection は診断の有無にかかわらず、既存 forward_metrics の
    // 2手先枝評価を利用する。通常 act() では詳細な LookaheadDiagnostic は構築しない。
    // Reach timing は恒常フリテンまたは限定した非フリテン悪形の gate 後に、selected candidate
    // 1件だけ既存 lookahead evaluator を利用する。診断が有効な場合だけ、表示用の全候補の構造化診断と
    // LookaheadDiagnostic 等を追加で構築する。
    fn select_normal_discard(
        &self,
        ctx: &GameContext,
        legal_actions: &[LegalAction],
        diagnostics: &mut DecisionDiagnostics,
        timing: &mut DecisionPhaseTimer,
    ) -> DiscardActionSelection {
        if !diagnostics.is_enabled() {
            // 内訳の計測も act() と同じ選択を1回通すだけで、選択のために評価を再実行しない。
            let mut normal_discard_timing = timing.normal_discard_timer();
            let selection = select_discard_action_with_evaluation_instrumented(
                ctx,
                legal_actions,
                &mut normal_discard_timing,
            );
            timing.record_two_shanten_self_tsumo_candidates(
                normal_discard_timing.take_two_shanten_self_tsumo_candidates(),
            );
            timing.record_normal_discard_phases(normal_discard_timing.finish());
            return selection;
        }

        let selection = select_discard_action_with_diagnostic(
            ctx,
            legal_actions,
            diagnostics.lookahead_scope(),
        );
        diagnostics.collect_normal_discard(selection)
    }

    // 押し引きモードに応じた action 選択。候補は必要になった時点でのみ計算する。
    // 選ばれた action とともに、その選択経路を表す source を返す。
    //
    // - Push:    Reach → 通常打牌 → 防御 fallback
    // - Neutral: 通常打牌 → 防御 fallback(Reach は検討しない)
    // - Fold:    防御 fallback → 通常打牌(Reach は検討しない)
    //
    // 現在の押し引き policy は Neutral を返さないが、action 順序としては維持している。
    // Push の順序は threat の種類で変えない。安全牌を通常打牌より優先するのは Fold の場合だけ。
    //
    // リーチ判断と通常打牌・押し引きは同じ `discard_selection` を参照する。リーチのために打牌を
    // 選び直したり、待ちを別経路で計算し直したりしない。検討した場合の判断内訳は `reach` へ
    // 書き込み、検討しなかった Neutral / Fold では `None` のままにする。
    #[allow(clippy::too_many_arguments)]
    fn select_action_for_push_pull_mode(
        &self,
        mode: PushPullMode,
        ctx: &GameContext,
        legal_actions: &[LegalAction],
        inputs: &PushPullInputs,
        discard_selection: &DiscardActionSelection,
        reach: &mut Option<ReachDecisionDiagnostic>,
        diagnostics: &mut DecisionDiagnostics,
    ) -> Option<(LegalAction, AgentActionSource)> {
        let normal_discard = discard_selection.action.as_ref();
        match mode {
            PushPullMode::Push => {
                let ReachDecision { diagnostic, hands } = decide_reach(
                    ctx,
                    legal_actions,
                    discard_selection,
                    &inputs.open_hand_threats,
                );
                let decision = reach.insert(diagnostic);
                // 統合診断は判断が終わった後の観測値だけを集める。診断が無効な act() 経路では
                // 何も構築せず、リーチ判断が組み立てた完成手もそのまま捨てる。
                diagnostics.collect_reach_damaten_comparison(
                    ctx,
                    legal_actions,
                    decision,
                    discard_selection,
                    hands,
                    &inputs.open_hand_threats,
                );

                if let Some(action) = decision.selected.clone() {
                    return Some((action, AgentActionSource::Reach));
                }
                if let Some(action) = normal_discard {
                    return Some((action.clone(), AgentActionSource::NormalDiscard));
                }
                self.select_defense_fallback(ctx, legal_actions, diagnostics)
            }
            PushPullMode::Neutral => {
                if let Some(action) = normal_discard {
                    return Some((action.clone(), AgentActionSource::NormalDiscard));
                }
                self.select_defense_fallback(ctx, legal_actions, diagnostics)
            }
            PushPullMode::Fold => {
                let evaluation =
                    evaluate_fold_defense(ctx, legal_actions, inputs, diagnostics.is_enabled());
                diagnostics.collect_fold_defense(ctx, legal_actions, inputs, &evaluation);
                if let Some(selection) = evaluation.selected() {
                    let source = match selection.kind {
                        FoldDefenseKind::Reach(kind) => AgentActionSource::DefenseFallback(kind),
                        FoldDefenseKind::OpenHand(category) => {
                            AgentActionSource::OpenHandDefenseFallback(category)
                        }
                        FoldDefenseKind::Combined(category) => {
                            AgentActionSource::CombinedThreatDefenseFallback(category)
                        }
                    };
                    return Some((selection.action.clone(), source));
                }
                normal_discard
                    .cloned()
                    .map(|action| (action, AgentActionSource::NormalDiscard))
            }
        }
    }

    // 防御 fallback を採用する場合に、その理由を診断ログへ出しつつ action と種別を返す。
    //
    // 構造化診断が有効な場合だけ、現物で早期決着しても exact candidate evidence を追加収集する。
    // tracing は production evaluation が既に持つ evidence だけを使い、ログのために exact model を
    // 起動しない。候補評価の収集は選択結果に影響せず、選択とログ・診断は同じ evaluation を共有する。
    fn select_defense_fallback(
        &self,
        ctx: &GameContext,
        legal_actions: &[LegalAction],
        diagnostics: &mut DecisionDiagnostics,
    ) -> Option<(LegalAction, AgentActionSource)> {
        let evaluation = evaluate_reach_defense(ctx, legal_actions, diagnostics.is_enabled());
        let selected = evaluation.selected;

        log_defense_fallback_evaluation(ctx, &evaluation, legal_actions);

        diagnostics.collect_defense(ctx, legal_actions, &evaluation);

        let (action, kind) = selected?;
        Some((action.clone(), AgentActionSource::DefenseFallback(kind)))
    }
}

// action を agent decision ログ用のコンパクトな文字列へ変換する。
fn agent_action_label(action: &LegalAction) -> String {
    match action {
        LegalAction::Dahai { tile } => tile.to_mjai_string(),
        LegalAction::Pon { tile, .. } => format!("Pon {}", tile.to_mjai_string()),
        LegalAction::Reach => "Reach".to_string(),
        LegalAction::Hora => "Hora".to_string(),
        LegalAction::Ryukyoku => "Ryukyoku".to_string(),
        LegalAction::None => "None".to_string(),
        other => format!("{other:?}"),
    }
}

/// 意思決定1回につき最終 action と選択経路の DEBUG イベントを1件出す opt-in ログ。
///
/// `RUST_LOG=bot_core::agent_decision=debug` で有効化する。debug が無効な通常時は
/// ログ用の文字列変換などを一切行わない。
pub(crate) fn log_agent_decision(decision: &AgentDecision) {
    if !tracing::enabled!(target: AGENT_DECISION_LOG_TARGET, tracing::Level::DEBUG) {
        return;
    }

    let selected_action = agent_action_label(&decision.action);
    let normal_discard = decision
        .normal_discard
        .as_ref()
        .map(agent_action_label)
        .unwrap_or_else(|| "None".to_string());
    let push_pull_mode = match &decision.push_pull {
        Some(decision) => format!("{:?}", decision.mode),
        None => "None".to_string(),
    };
    let push_pull_reason = match &decision.push_pull {
        Some(decision) => format!("{:?}", decision.reason),
        None => "None".to_string(),
    };
    let defense_kind = match decision.source.defense_kind() {
        Some(kind) => format!("{kind:?}"),
        None => "None".to_string(),
    };
    let open_hand_defense_category = match decision.source.open_hand_defense_category() {
        Some(category) => format!("{category:?}"),
        None => "None".to_string(),
    };
    let combined_defense_category = match decision.source.combined_defense_category() {
        Some(category) => format!("{category:?}"),
        None => "None".to_string(),
    };
    let call_reason = match &decision.call {
        Some(call) => format!("{:?}", call.reason),
        None => "None".to_string(),
    };
    let ryukyoku_verdict = match &decision.ryukyoku {
        Some(ryukyoku) => format!("{:?}", ryukyoku.verdict),
        None => "None".to_string(),
    };
    let reach_reason = match &decision.reach {
        Some(reach) => format!("{:?}", reach.reason),
        None => "None".to_string(),
    };
    let damaten_verdict = match decision
        .reach
        .as_ref()
        .and_then(|reach| reach.damaten_verdict())
    {
        Some(verdict) => format!("{verdict:?}"),
        None => "None".to_string(),
    };
    // base policy がリーチを選んだのに action が Dahai になった局面を、log だけで判別できる
    // ようにする。base の reason はそのまま別 field に残す。
    let timing = decision.reach.as_ref().and_then(|reach| reach.timing);
    let reach_timing = match timing {
        Some(timing) => format!("{:?}", timing.decision),
        None => "None".to_string(),
    };
    let reach_timing_reason = match timing {
        Some(timing) => format!("{:?}", timing.reason),
        None => "None".to_string(),
    };

    tracing::debug!(
        target: AGENT_DECISION_LOG_TARGET,
        selected_action = %selected_action,
        selected_source = decision.source.label(),
        push_pull_mode = %push_pull_mode,
        push_pull_reason = %push_pull_reason,
        normal_discard = %normal_discard,
        reach_reason = %reach_reason,
        reach_timing = %reach_timing,
        reach_timing_reason = %reach_timing_reason,
        damaten_verdict = %damaten_verdict,
        defense_kind = %defense_kind,
        open_hand_defense_category = %open_hand_defense_category,
        combined_defense_category = %combined_defense_category,
        call_reason = %call_reason,
        ryukyoku_verdict = %ryukyoku_verdict,
        "agent decision",
    );
}

impl Agent for ShantenAgent {
    fn act(&mut self, ctx: &GameContext, legal_actions: &[LegalAction]) -> LegalAction {
        let decision = self.decide(ctx, legal_actions);
        log_agent_decision(&decision);
        decision.action
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::call_decision::{
        CALL_MIN_LIVE_WAIT_REMAINING, CALL_TENPAI_SHANTEN, CallCandidateDiagnostic,
        CallDecisionReason, CallKind, CallWaitYaku,
    };
    use crate::combined_defense::{
        ThreatDefenseTarget, combined_threat_defense_targets_from_context,
        select_combined_threat_defense_fallback_action_with_kind,
    };
    use crate::decision_timing::NormalDiscardPhaseDurations;
    use crate::defense::{
        DefenseDecisionDiagnostic, HonorSafetyRank, OpponentHonorValue, SuitedSafetyRank,
        honor_safety_rank, is_genbutsu_for_all_reached, opponent_honor_value_for_reached,
        select_defense_fallback_action, suited_safety_rank_for_all_reached,
    };
    use crate::discard_selection::{
        select_best_normal_discard_evaluation, select_discard_action,
        select_discard_action_with_evaluation,
    };
    use crate::push_pull::{
        PushPullReason, push_pull_inputs_from_context,
        push_pull_inputs_from_context_with_evaluation,
    };
    use crate::reach_policy::ReachDecisionReason;
    use crate::ryukyoku_decision::RyukyokuVerdict;
    use crate::ryukyoku_decision::tests::{
        CHIITOITSU_THREE_HAND, CHIITOITSU_TWO_HAND, KOKUSHI_FOUR_HAND, KOKUSHI_THREE_HAND,
        STANDARD_THREE_HAND, STANDARD_TWO_HAND, context_from_hand,
    };
    use crate::shanten_test_support::{
        OPPONENT_MELD_DRAW, OPPONENT_MELD_HAND, TENPAI_DRAWN, dahai, fold_actions,
        fold_under_reach_context, opponent_meld_actions, opponent_reach_context,
        opponent_reach_context_with_visible, pon_meld, suited_reach_context,
        suited_reach_context_with_reached, tenpai_actions, tenpai_context, tenpai_dahai_actions,
        tenpai_under_reach_context, tile, unavailable_reach_meld, weak_tenpai_actions,
        weak_tenpai_under_reach_context, weak_tenpai_under_reach_context_with,
    };
    use bot_logic::{
        DiscardComparisonReason, DiscardEvaluation, FixedMeldCount, PermanentFuriten, TileCounts,
        TileId, TileType, calculate_shanten, chiitoitsu_shanten, kokushi_shanten, standard_shanten,
    };
    use std::time::Duration;

    #[derive(Debug)]
    struct DefenseTraceSubscriber;

    impl tracing::Subscriber for DefenseTraceSubscriber {
        fn enabled(&self, metadata: &tracing::Metadata<'_>) -> bool {
            metadata.target() == "bot_core::defense"
        }

        fn new_span(&self, _span: &tracing::span::Attributes<'_>) -> tracing::span::Id {
            tracing::span::Id::from_u64(1)
        }

        fn record(&self, _span: &tracing::span::Id, _values: &tracing::span::Record<'_>) {}

        fn record_follows_from(&self, _span: &tracing::span::Id, _follows: &tracing::span::Id) {}

        fn event(&self, _event: &tracing::Event<'_>) {}

        fn enter(&self, _span: &tracing::span::Id) {}

        fn exit(&self, _span: &tracing::span::Id) {}
    }

    fn with_defense_trace<T>(f: impl FnOnce() -> T) -> T {
        tracing::subscriber::with_default(DefenseTraceSubscriber, f)
    }

    fn phase_timing_contexts() -> Vec<(GameContext, Vec<LegalAction>)> {
        vec![
            (
                GameContext::with_drawn_tile(tile(0)),
                vec![dahai(0), LegalAction::Hora],
            ),
            (
                GameContext::with_drawn_tile(tile(0)),
                vec![dahai(0), LegalAction::Ryukyoku],
            ),
            (tenpai_context(&[]), tenpai_actions()),
            (fold_under_reach_context(), fold_actions()),
        ]
    }

    #[test]
    fn phase_timing_does_not_change_the_selected_action() {
        for (ctx, actions) in phase_timing_contexts() {
            let mut timed = ShantenAgent;
            let mut untimed = ShantenAgent;
            assert_eq!(
                timed.act_with_phase_timing(&ctx, &actions).action,
                untimed.act(&ctx, &actions),
                "{actions:?}"
            );
        }
    }

    #[test]
    fn an_early_return_keeps_the_phases_it_never_reached_at_zero() {
        let mut agent = ShantenAgent;
        let ctx = GameContext::with_drawn_tile(tile(0));

        let hora = agent.act_with_phase_timing(&ctx, &[dahai(0), LegalAction::Hora]);
        assert_eq!(hora.action, LegalAction::Hora);
        assert_eq!(hora.phases.normal_discard, Duration::ZERO);
        assert_eq!(hora.phases.post_discard, Duration::ZERO);
        assert_eq!(hora.phases.total(), hora.phases.early);

        let ryukyoku = agent.act_with_phase_timing(&ctx, &[dahai(0), LegalAction::Ryukyoku]);
        assert_eq!(ryukyoku.action, LegalAction::Ryukyoku);
        assert_eq!(ryukyoku.phases.normal_discard, Duration::ZERO);
        assert_eq!(ryukyoku.phases.post_discard, Duration::ZERO);
        assert_eq!(ryukyoku.phases.total(), ryukyoku.phases.early);
    }

    #[test]
    fn an_early_return_keeps_the_normal_discard_subphases_at_zero() {
        let mut agent = ShantenAgent;
        let ctx = GameContext::with_drawn_tile(tile(0));

        let hora = agent.act_with_phase_timing(&ctx, &[dahai(0), LegalAction::Hora]);
        assert_eq!(hora.action, LegalAction::Hora);
        assert_eq!(
            hora.phases.normal_discard_phases,
            NormalDiscardPhaseDurations::default()
        );
        assert_eq!(hora.two_shanten_self_tsumo_candidates().len(), 0);

        let ryukyoku = agent.act_with_phase_timing(&ctx, &[dahai(0), LegalAction::Ryukyoku]);
        assert_eq!(ryukyoku.action, LegalAction::Ryukyoku);
        assert_eq!(
            ryukyoku.phases.normal_discard_phases,
            NormalDiscardPhaseDurations::default()
        );
        assert_eq!(ryukyoku.two_shanten_self_tsumo_candidates().len(), 0);
    }

    #[test]
    fn the_normal_discard_breakdown_measures_a_single_selection() {
        let production = include_str!("shanten.rs")
            .split("#[cfg(test)]")
            .next()
            .unwrap();
        let select_normal_discard = production
            .split("fn select_normal_discard(")
            .nth(1)
            .unwrap()
            .split("fn select_action_for_push_pull_mode(")
            .next()
            .unwrap();

        assert_eq!(
            select_normal_discard
                .matches("select_discard_action_with_evaluation_instrumented(")
                .count(),
            1,
            "{select_normal_discard}"
        );
        assert_eq!(
            select_normal_discard
                .matches("select_discard_action_with_diagnostic(")
                .count(),
            1,
            "{select_normal_discard}"
        );
    }

    #[test]
    fn phase_timing_runs_the_decision_only_once() {
        let production = include_str!("shanten.rs")
            .split("#[cfg(test)]")
            .next()
            .unwrap();
        let timed_entry_point = production
            .split("pub fn act_with_phase_timing(")
            .nth(1)
            .unwrap()
            .split("pub(crate) fn decide(")
            .next()
            .unwrap();

        assert_eq!(
            timed_entry_point.matches("decide_instrumented(").count(),
            1,
            "{timed_entry_point}"
        );
        assert_eq!(
            production.matches("self.select_normal_discard(").count(),
            1,
            "{production}"
        );
    }

    #[test]
    fn picks_hora_first() {
        let mut agent = ShantenAgent;
        let ctx = GameContext::with_drawn_tile(tile(0));
        let actions = vec![dahai(0), LegalAction::Hora];
        assert_eq!(agent.act(&ctx, &actions), LegalAction::Hora);
    }

    #[test]
    fn prefers_ryukyoku_over_dahai() {
        let mut agent = ShantenAgent;
        let ctx = GameContext::with_drawn_tile(tile(0));
        let actions = vec![dahai(0), LegalAction::Ryukyoku];
        assert_eq!(agent.act(&ctx, &actions), LegalAction::Ryukyoku);
    }

    #[test]
    fn picks_dahai_by_discard_evaluation() {
        let mut agent = ShantenAgent;
        let hand_values = [0, 4, 8, 12, 17, 20, 24, 28, 32, 36, 40, 44, 89];
        let ctx = GameContext::from_parts(
            Some(tile(116)),
            hand_values.iter().map(|&value| tile(value)).collect(),
        );
        let actions: Vec<LegalAction> = hand_values
            .iter()
            .map(|&value| dahai(value))
            .chain([dahai(116)])
            .collect();

        let expected = select_discard_action(&ctx, &actions).unwrap();

        assert_eq!(agent.act(&ctx, &actions), expected);
    }

    #[test]
    fn prefers_hora_over_reach() {
        let mut agent = ShantenAgent;
        let ctx = GameContext::with_drawn_tile(tile(0));
        let actions = vec![LegalAction::Reach, LegalAction::Hora];
        assert_eq!(agent.act(&ctx, &actions), LegalAction::Hora);
    }

    #[test]
    fn prefers_ryukyoku_over_reach() {
        let mut agent = ShantenAgent;
        let ctx = GameContext::with_drawn_tile(tile(0));
        let actions = vec![LegalAction::Reach, LegalAction::Ryukyoku];
        assert_eq!(agent.act(&ctx, &actions), LegalAction::Ryukyoku);
    }

    #[test]
    fn prefers_reach_over_evaluated_dahai() {
        let mut agent = ShantenAgent;
        let ctx = tenpai_context(&[]);
        let actions = tenpai_actions();
        assert!(select_discard_action(&ctx, &actions).is_some());
        assert_eq!(agent.act(&ctx, &actions), LegalAction::Reach);
    }

    #[test]
    fn reach_is_policy_choice_not_fallback() {
        // 通常打牌を選べる局面でも、選んだ打牌後の待ちが十分ならリーチを選ぶ。
        let mut agent = ShantenAgent;
        let ctx = tenpai_context(&[]);
        let actions = tenpai_actions();

        let normal = select_discard_action(&ctx, &actions).expect("通常打牌を選べる");
        assert_eq!(agent.act(&ctx, &actions), LegalAction::Reach);
        assert_ne!(agent.act(&ctx, &actions), normal);
    }

    #[test]
    fn picks_dahai_when_reach_absent() {
        let mut agent = ShantenAgent;
        let ctx = GameContext::with_drawn_tile(tile(0));
        let actions = vec![dahai(0)];
        assert_eq!(agent.act(&ctx, &actions), dahai(0));
    }

    #[test]
    fn falls_back_to_first_dahai_without_reach() {
        let mut agent = ShantenAgent;
        let ctx = GameContext::default();
        let actions = vec![dahai(4), dahai(0)];
        assert_eq!(agent.act(&ctx, &actions), dahai(4));
    }

    #[test]
    fn picks_none_when_no_dahai() {
        let mut agent = ShantenAgent;
        let ctx = GameContext::default();
        let actions = vec![LegalAction::None];
        assert_eq!(agent.act(&ctx, &actions), LegalAction::None);
    }

    // Chi と各種カン。カンは鳴き判断の対象外なので、積極的に選ばない。
    fn chi_and_kan_actions() -> Vec<LegalAction> {
        vec![
            LegalAction::Chi {
                tile: tile(17),
                consumed: vec![tile(12), tile(20)],
            },
            LegalAction::Daiminkan {
                tile: tile(104),
                consumed: vec![tile(105), tile(106), tile(107)],
            },
            LegalAction::Ankan {
                consumed: vec![tile(72), tile(73), tile(74), tile(75)],
            },
            LegalAction::Kakan {
                tile: tile(124),
                consumed: vec![tile(125), tile(126), tile(127)],
            },
        ]
    }

    #[test]
    fn does_not_actively_claim_chi_or_kans() {
        let mut agent = ShantenAgent;
        let ctx = GameContext::default();
        let actions: Vec<LegalAction> = chi_and_kan_actions()
            .into_iter()
            .chain([LegalAction::None])
            .collect();
        assert_eq!(agent.act(&ctx, &actions), LegalAction::None);
    }

    #[test]
    fn does_not_claim_pon_outside_the_limited_conditions() {
        let mut agent = ShantenAgent;
        let ctx = GameContext::default();
        let actions: Vec<LegalAction> = chi_and_kan_actions()
            .into_iter()
            .chain([
                LegalAction::Pon {
                    tile: tile(108),
                    consumed: vec![tile(109), tile(110)],
                },
                LegalAction::None,
            ])
            .collect();
        assert_eq!(agent.act(&ctx, &actions), LegalAction::None);
    }

    #[test]
    fn falls_back_to_none_for_empty_actions() {
        let mut agent = ShantenAgent;
        let ctx = GameContext::default();
        assert_eq!(agent.act(&ctx, &[]), LegalAction::None);
    }

    #[test]
    fn uses_visible_tiles_for_discard_evaluation() {
        let mut agent = ShantenAgent;
        let hand_values = [0, 4, 8, 12, 17, 20, 24, 28, 32, 48, 53, 56, 36];
        let hand: Vec<_> = hand_values.iter().map(|&value| tile(value)).collect();
        let mut visible = hand.clone();
        visible.extend([68, 69, 70, 71].iter().map(|&value| tile(value)));
        let ctx = GameContext::from_parts_with_visible_tiles(
            Some(tile(68)),
            hand,
            vec![],
            None,
            None,
            visible,
        );
        let actions: Vec<LegalAction> = hand_values
            .iter()
            .map(|&value| dahai(value))
            .chain([dahai(68)])
            .collect();

        let selected = agent.act(&ctx, &actions);
        let LegalAction::Dahai { tile } = selected else {
            panic!("expected dahai");
        };
        assert_eq!(tile.tile_type().to_mjai_string(), "9p");
    }

    fn reach_diagnostic(ctx: &GameContext, actions: &[LegalAction]) -> ReachDecisionDiagnostic {
        ShantenAgent::diagnose(ctx, actions)
            .reach
            .expect("リーチを検討している")
    }

    // 114477m 114477p + 1s + ツモ E。どちらの孤立牌を切っても七対子単騎テンパイになり、
    // 生き枚数が同じなので待ち牌の品質で打牌が決まる。
    const CHIITOITSU_TANKI_HAND: [u8; 13] = [0, 1, 12, 13, 24, 25, 36, 37, 48, 49, 60, 61, 72];
    const CHIITOITSU_TANKI_DRAWN: u8 = 108;
    const CHIITOITSU_TANKI_DISCARD: u8 = 72;

    fn chiitoitsu_tanki_context() -> GameContext {
        let hand: Vec<_> = CHIITOITSU_TANKI_HAND
            .iter()
            .map(|&value| tile(value))
            .collect();
        GameContext::from_parts(Some(tile(CHIITOITSU_TANKI_DRAWN)), hand)
    }

    fn chiitoitsu_tanki_dahai_actions() -> Vec<LegalAction> {
        CHIITOITSU_TANKI_HAND
            .iter()
            .map(|&value| dahai(value))
            .chain([dahai(CHIITOITSU_TANKI_DRAWN)])
            .collect()
    }

    #[test]
    fn chiitoitsu_tanki_selection_is_shared_by_act_and_diagnose() {
        // 品質の高い E 単騎に取るため 1s を切る。診断は production comparator の理由を載せる。
        let ctx = chiitoitsu_tanki_context();
        let actions = chiitoitsu_tanki_dahai_actions();
        let mut agent = ShantenAgent;
        let acted = agent.act(&ctx, &actions);
        let diagnostic = ShantenAgent::diagnose(&ctx, &actions);
        let with_lookahead =
            ShantenAgent::diagnose_with_options(&ctx, &actions, DiagnosticOptions::WITH_LOOKAHEAD);

        assert_eq!(acted, dahai(CHIITOITSU_TANKI_DISCARD));
        assert_eq!(diagnostic.selected_action, acted);
        assert_eq!(with_lookahead.selected_action, acted);
        assert_eq!(diagnostic.selected_source, AgentActionSource::NormalDiscard);

        let normal_discard = diagnostic
            .normal_discard
            .as_ref()
            .expect("通常打牌を評価している");
        let loser = normal_discard
            .candidates
            .iter()
            .find(|candidate| {
                candidate.evaluation.discard == tile(CHIITOITSU_TANKI_DRAWN).tile_type()
            })
            .expect("打 E も候補になる");
        assert!(loser.selected_is_strictly_better_than_candidate);
        assert_eq!(
            loser.comparison_reason,
            DiscardComparisonReason::ChiitoitsuWaitQuality
        );
    }

    #[test]
    fn follows_discard_selection_for_same_tile_type() {
        let mut agent = ShantenAgent;
        let ctx = GameContext::from_parts(Some(tile(16)), vec![tile(17)]);
        let actions = vec![dahai(17), dahai(16)];

        let expected = select_discard_action(&ctx, &actions).unwrap();

        assert_eq!(agent.act(&ctx, &actions), expected);
    }

    #[test]
    fn prefers_hora_over_genbutsu_fallback() {
        let mut agent = ShantenAgent;
        let ctx = opponent_reach_context(Some(0), &[]);
        let actions = vec![dahai(16), LegalAction::Hora];
        assert_eq!(agent.act(&ctx, &actions), LegalAction::Hora);
    }

    #[test]
    fn prefers_ryukyoku_over_genbutsu_fallback() {
        let mut agent = ShantenAgent;
        let ctx = opponent_reach_context(Some(0), &[]);
        let actions = vec![dahai(16), LegalAction::Ryukyoku];
        assert_eq!(agent.act(&ctx, &actions), LegalAction::Ryukyoku);
    }

    #[test]
    fn prefers_genbutsu_fallback_over_reach() {
        let mut agent = ShantenAgent;
        let ctx = opponent_reach_context(Some(0), &[]);
        let actions = vec![LegalAction::Reach, dahai(0), dahai(16)];
        assert_eq!(agent.act(&ctx, &actions), dahai(16));
    }

    #[test]
    fn fold_iishanten_prefers_genbutsu_fallback_over_normal_discard() {
        let mut agent = ShantenAgent;
        // 単独の子リーチに対する一向聴。受け入れが広くても押さず、共通現物 16(5m) を
        // 通常打牌より優先する。
        let hand_values = [0, 4, 8, 12, 13, 20, 24, 28, 32, 36, 40, 44, 89];
        let ctx = opponent_reach_context(Some(116), &hand_values);
        let actions: Vec<LegalAction> = hand_values
            .iter()
            .map(|&value| dahai(value))
            .chain([dahai(116), dahai(16)])
            .collect();
        let decision = decide_push_pull(&push_pull_inputs_from_context(&ctx, &actions));
        assert_eq!(decision.mode, PushPullMode::Fold);
        assert_eq!(decision.reason, PushPullReason::IishantenAgainstReach);

        let normal = select_discard_action(&ctx, &actions).unwrap();
        assert_eq!(agent.act(&ctx, &actions), dahai(16));
        assert_ne!(agent.act(&ctx, &actions), normal);
    }

    #[test]
    fn fold_without_common_genbutsu_falls_through_to_normal_discard() {
        let mut agent = ShantenAgent;
        // 他家リーチ中でも合法 Dahai に共通現物が無い Fold 局面。Reach は抑制し通常打牌へ進む。
        let ctx = opponent_reach_context(Some(0), &[]);
        let actions = vec![LegalAction::Reach, dahai(0), dahai(56)];
        assert_eq!(
            decide_push_pull(&push_pull_inputs_from_context(&ctx, &actions)).mode,
            PushPullMode::Fold
        );
        assert_eq!(agent.act(&ctx, &actions), dahai(0));
    }

    #[test]
    fn keeps_normal_behavior_without_opponent_reach() {
        let mut agent = ShantenAgent;
        // 他家リーチが無ければ、河の 16(5m) と同じ現物相当の 17(5m) が合法でも Reach を選ぶ。
        let ctx = tenpai_under_reach_context(None, [false; 4]);
        let actions = vec![LegalAction::Reach, dahai(TENPAI_DRAWN), dahai(17)];
        assert_eq!(agent.act(&ctx, &actions), LegalAction::Reach);
    }

    #[test]
    fn does_not_claim_melds_even_under_opponent_reach() {
        let mut agent = ShantenAgent;
        // 他家リーチ中でも副露・カンは積極選択しない。共通現物も無い局面。
        let ctx = opponent_reach_context(None, &[]);
        let actions = vec![
            LegalAction::Pon {
                tile: tile(108),
                consumed: vec![tile(109), tile(110)],
            },
            LegalAction::Ankan {
                consumed: vec![tile(72), tile(73), tile(74), tile(75)],
            },
            LegalAction::None,
        ];
        assert_eq!(agent.act(&ctx, &actions), LegalAction::None);
    }

    #[test]
    fn prefers_genbutsu_fallback_over_none() {
        let mut agent = ShantenAgent;
        let ctx = opponent_reach_context(Some(0), &[]);
        let actions = vec![dahai(16), LegalAction::None];
        assert_eq!(agent.act(&ctx, &actions), dahai(16));
    }

    #[test]
    fn prefers_genbutsu_fallback_over_honor_safety_fallback() {
        let mut agent = ShantenAgent;
        // 共通現物 16(5m) と字牌 108(東) が両方合法でも、現物を優先する。
        let ctx = opponent_reach_context(Some(0), &[]);
        let actions = vec![dahai(108), dahai(16)];
        assert_eq!(agent.act(&ctx, &actions), dahai(16));
    }

    #[test]
    fn picks_safest_honor_dahai_when_no_common_genbutsu() {
        let mut agent = ShantenAgent;
        // 共通現物なし。東は2枚見え、南は0枚見え。より安全な東を切る。
        let ctx = opponent_reach_context_with_visible(Some(112), &[], &[108, 109]);
        let actions = vec![dahai(112), dahai(108)];
        assert_eq!(agent.act(&ctx, &actions), dahai(108));
    }

    #[test]
    fn prefers_honor_safety_fallback_over_reach() {
        let mut agent = ShantenAgent;
        // 共通現物なし。数牌と字牌が合法なら Reach より字牌を切る。
        let ctx = opponent_reach_context(Some(0), &[]);
        let actions = vec![LegalAction::Reach, dahai(0), dahai(108)];
        assert_eq!(agent.act(&ctx, &actions), dahai(108));
    }

    #[test]
    fn prefers_honor_safety_fallback_over_discard_evaluation() {
        let mut agent = ShantenAgent;
        // 通常評価では別牌が選ばれ得る手牌だが、共通現物がなければ字牌 108(東) を優先する。
        let hand_values = [0, 4, 8, 12, 13, 20, 24, 28, 32, 36, 40, 44, 89];
        let ctx = opponent_reach_context(Some(108), &hand_values);
        let actions: Vec<LegalAction> = hand_values
            .iter()
            .map(|&value| dahai(value))
            .chain([dahai(108)])
            .collect();
        assert_eq!(agent.act(&ctx, &actions), dahai(108));
    }

    #[test]
    fn fold_without_common_genbutsu_or_honor_falls_through_to_normal_discard() {
        let mut agent = ShantenAgent;
        // 共通現物も字牌 Dahai もなく、数牌も全て NoSafety の Fold 局面。
        // リーチ者の河は 16(5m) のみで、0(1m) / 56(6p) は無スジ・壁なしの NoSafety。
        // Reach を抑制し、防御牌が無いので通常打牌へ進む。
        let ctx = opponent_reach_context(Some(0), &[]);
        let actions = vec![LegalAction::Reach, dahai(0), dahai(56)];
        assert_eq!(
            decide_push_pull(&push_pull_inputs_from_context(&ctx, &actions)).mode,
            PushPullMode::Fold
        );
        assert_eq!(agent.act(&ctx, &actions), dahai(0));
    }

    #[test]
    fn does_not_use_honor_safety_fallback_without_opponent_reach() {
        let mut agent = ShantenAgent;
        // 他家リーチが無ければ、字牌(116 = 北)が合法でも従来の Reach を選ぶ。
        let ctx = tenpai_context(&[]);
        let actions = vec![LegalAction::Reach, dahai(TENPAI_DRAWN)];
        assert_eq!(agent.act(&ctx, &actions), LegalAction::Reach);
    }

    #[test]
    fn honor_safety_fallback_ignores_number_dahai() {
        let mut agent = ShantenAgent;
        // 数牌のみで字牌がなければ字牌 fallback は発動しない。Fold だが安全牌が無いので通常打牌へ進む。
        let ctx = opponent_reach_context(Some(0), &[]);
        let actions = vec![LegalAction::Reach, dahai(0), dahai(56)];
        assert_eq!(
            decide_push_pull(&push_pull_inputs_from_context(&ctx, &actions)).mode,
            PushPullMode::Fold
        );
        assert_eq!(agent.act(&ctx, &actions), dahai(0));
    }

    #[test]
    fn honor_safety_fallback_ignores_non_dahai_honor_actions() {
        let mut agent = ShantenAgent;
        // 字牌の Pon はあっても字牌 Dahai が無ければ fallback は発動しない。
        let ctx = opponent_reach_context(Some(0), &[]);
        let actions = vec![
            LegalAction::Pon {
                tile: tile(108),
                consumed: vec![tile(109), tile(110)],
            },
            LegalAction::None,
        ];
        assert_eq!(agent.act(&ctx, &actions), LegalAction::None);
    }

    #[test]
    fn honor_safety_fallback_preserves_order_within_same_rank() {
        let mut agent = ShantenAgent;
        // 東も南も0枚見えで同安全度なら legal_actions の元順序を保つ。
        let ctx = opponent_reach_context(Some(0), &[]);
        let actions = vec![dahai(112), dahai(108)];
        assert_eq!(agent.act(&ctx, &actions), dahai(112));
    }

    fn multiple_reach_wind_context(oya: u8, drawn_tile: Option<u8>) -> GameContext {
        let discards = [vec![], vec![tile(16)], vec![], vec![]];
        let mut melds: [Vec<_>; 4] = Default::default();
        melds[3] = vec![unavailable_reach_meld()];
        GameContext::from_parts_with_melds(
            drawn_tile.map(tile),
            vec![],
            vec![],
            TileType::new(27),
            None,
            Vec::new(),
            Some(0),
            Some(oya),
            discards,
            [false, true, false, true],
            melds,
        )
    }

    #[test]
    fn honor_safety_fallback_breaks_same_rank_ties_by_opponent_honor_value() {
        let mut agent = ShantenAgent;
        // player 1 / 3 の複数リーチで legacy path を通す。oya=3 では両者にとって N は
        // GuestWind、C は SingleValueHonor のままなので、同じ HonorSafetyRank を N が制する。
        let ctx = multiple_reach_wind_context(3, Some(0));
        assert_eq!(
            honor_safety_rank(tile(120).tile_type(), &ctx),
            Some(HonorSafetyRank::NoVisible)
        );
        assert_eq!(
            honor_safety_rank(tile(132).tile_type(), &ctx),
            Some(HonorSafetyRank::NoVisible)
        );
        assert_eq!(
            opponent_honor_value_for_reached(tile(120).tile_type(), &ctx),
            Some(OpponentHonorValue::GuestWind)
        );
        assert_eq!(
            opponent_honor_value_for_reached(tile(132).tile_type(), &ctx),
            Some(OpponentHonorValue::SingleValueHonor)
        );
        assert!(!is_genbutsu_for_all_reached(tile(120).tile_type(), &ctx));
        assert!(!is_genbutsu_for_all_reached(tile(132).tile_type(), &ctx));
        assert_eq!(agent.act(&ctx, &[dahai(132), dahai(120)]), dahai(120));
        assert_eq!(agent.act(&ctx, &[dahai(120), dahai(132)]), dahai(120));

        // oya=1 では E が player 1 の DoubleWind、C が SingleValueHonor、N が両者の
        // GuestWind。player 3 を加えても intended ordering は変わらない。
        let ctx = multiple_reach_wind_context(1, Some(0));
        assert_eq!(
            opponent_honor_value_for_reached(tile(108).tile_type(), &ctx),
            Some(OpponentHonorValue::DoubleWind)
        );
        assert_eq!(
            opponent_honor_value_for_reached(tile(132).tile_type(), &ctx),
            Some(OpponentHonorValue::SingleValueHonor)
        );
        assert_eq!(
            opponent_honor_value_for_reached(tile(120).tile_type(), &ctx),
            Some(OpponentHonorValue::GuestWind)
        );
        assert_eq!(agent.act(&ctx, &[dahai(108), dahai(132)]), dahai(132));
        assert_eq!(agent.act(&ctx, &[dahai(108), dahai(120)]), dahai(120));
    }

    #[test]
    fn honor_safety_fallback_keeps_visible_count_over_opponent_honor_value() {
        let mut agent = ShantenAgent;
        let ctx = GameContext::from_parts_with_table_state(
            Some(tile(0)),
            vec![],
            vec![],
            TileType::new(27),
            None,
            vec![tile(132), tile(133), tile(134)],
            Some(0),
            Some(3),
            [vec![], vec![tile(16)], vec![], vec![]],
            [false, true, false, false],
        );
        assert_eq!(agent.act(&ctx, &[dahai(120), dahai(135)]), dahai(135));
    }

    #[test]
    fn prefers_genbutsu_fallback_over_suited_safety_fallback() {
        let mut agent = ShantenAgent;
        // 共通現物 16(5m) と NoChance 数牌 4(2m) が両方合法でも、現物を優先する。
        let ctx = suited_reach_context(Some(0), &[], &[4, 5, 6, 7], &[16]);
        let actions = vec![dahai(4), dahai(16)];
        assert_eq!(agent.act(&ctx, &actions), dahai(16));
    }

    #[test]
    fn prefers_honor_safety_fallback_over_suited_safety_fallback() {
        let mut agent = ShantenAgent;
        // 共通現物なし。字牌 108(東) と NoChance 数牌 4(2m) が合法なら字牌を優先する。
        let ctx = suited_reach_context(Some(0), &[], &[4, 5, 6, 7], &[]);
        let actions = vec![dahai(108), dahai(4)];
        assert_eq!(agent.act(&ctx, &actions), dahai(108));
    }

    #[test]
    fn picks_no_chance_suited_dahai_when_no_genbutsu_or_honor() {
        let mut agent = ShantenAgent;
        // 共通現物も字牌もなし。4m を4枚見えにして経路 [3m,4m] を Blocked にし 2m を NoChance。
        // 無スジ 0(1m) より NoChance 4(2m) を選ぶ。
        let ctx = suited_reach_context(Some(0), &[], &[12, 13, 14, 15], &[]);
        let actions = vec![dahai(0), dahai(4)];
        assert_eq!(agent.act(&ctx, &actions), dahai(4));
    }

    #[test]
    fn picks_one_chance_suited_dahai_when_no_genbutsu_or_honor() {
        let mut agent = ShantenAgent;
        // 複数リーチの legacy path。共通現物も字牌もなく、4m を3枚見えにして経路
        // [3m,4m] を OneChance にする。無スジ 1m より OneChance 2m を選ぶ。
        let ctx = suited_reach_context_with_reached(
            Some(0),
            &[],
            &[12, 13, 14],
            &[],
            [false, true, true, false],
        );
        let actions = vec![dahai(0), dahai(4)];
        assert_eq!(
            suited_safety_rank_for_all_reached(tile(0).tile_type(), &ctx),
            Some(SuitedSafetyRank::NoSafety)
        );
        assert_eq!(
            suited_safety_rank_for_all_reached(tile(4).tile_type(), &ctx),
            Some(SuitedSafetyRank::OneChance)
        );
        assert!(actions.iter().all(|action| {
            let LegalAction::Dahai { tile } = action else {
                return false;
            };
            !is_genbutsu_for_all_reached(tile.tile_type(), &ctx)
        }));
        assert_eq!(agent.act(&ctx, &actions), dahai(4));
    }

    #[test]
    fn picks_suji_suited_dahai_when_no_genbutsu_or_honor() {
        let mut agent = ShantenAgent;
        // 共通現物も字牌もなし。リーチ者の河に 12(4m) があり 0(1m) はスジ。無スジ 16(5m) より選ぶ。
        let ctx = suited_reach_context(Some(0), &[], &[], &[12]);
        let actions = vec![dahai(16), dahai(0)];
        assert_eq!(agent.act(&ctx, &actions), dahai(0));
    }

    #[test]
    fn suited_safety_fallback_follows_safety_order() {
        let mut agent = ShantenAgent;
        // 経路壁で安全度を作る。1p は 2p 4枚で NoChance、9p は 8p 3枚で OneChance、
        // 1s は両リーチ者の 4s 河でスジ(Suji)、5s は無スジ・壁なし(NoSafety)。複数リーチの
        // legacy path で最も安全な NoChance を選ぶ。
        let mut melds: [Vec<_>; 4] = Default::default();
        melds[2] = vec![unavailable_reach_meld()];
        let ctx = GameContext::from_parts_with_melds(
            Some(tile(0)),
            vec![],
            vec![],
            None,
            None,
            [40, 41, 42, 43, 64, 65, 66].map(tile).to_vec(),
            Some(0),
            None,
            [vec![], vec![tile(84)], vec![tile(85)], vec![]],
            [false, true, true, false],
            melds,
        );
        let actions = vec![dahai(88), dahai(72), dahai(68), dahai(36)];
        assert_eq!(
            suited_safety_rank_for_all_reached(tile(36).tile_type(), &ctx),
            Some(SuitedSafetyRank::NoChance)
        );
        assert_eq!(
            suited_safety_rank_for_all_reached(tile(68).tile_type(), &ctx),
            Some(SuitedSafetyRank::OneChance)
        );
        assert_eq!(
            suited_safety_rank_for_all_reached(tile(72).tile_type(), &ctx),
            Some(SuitedSafetyRank::Suji)
        );
        assert_eq!(
            suited_safety_rank_for_all_reached(tile(88).tile_type(), &ctx),
            Some(SuitedSafetyRank::NoSafety)
        );
        assert!(actions.iter().all(|action| {
            let LegalAction::Dahai { tile } = action else {
                return false;
            };
            !is_genbutsu_for_all_reached(tile.tile_type(), &ctx)
        }));
        assert_eq!(agent.act(&ctx, &actions), dahai(36));
    }

    #[test]
    fn fold_without_safe_suited_falls_through_to_normal_discard() {
        let mut agent = ShantenAgent;
        // Fold 局面で共通現物も字牌もなく数牌が全て NoSafety なら、防御 fallback は無い。
        // Reach は抑制し、防御牌がないことを理由に失敗させず通常打牌へ進む。
        let ctx = suited_reach_context(Some(0), &[], &[], &[]);
        let actions = vec![LegalAction::Reach, dahai(0), dahai(4)];
        assert_eq!(
            decide_push_pull(&push_pull_inputs_from_context(&ctx, &actions)).mode,
            PushPullMode::Fold
        );
        assert_eq!(agent.act(&ctx, &actions), dahai(0));
    }

    #[test]
    fn does_not_use_suited_safety_fallback_without_opponent_reach() {
        let mut agent = ShantenAgent;
        // 他家リーチが無ければ、河に 16(5m) があり 4(2m) がスジ相当でも従来の Reach を選ぶ。
        let ctx = tenpai_under_reach_context(None, [false; 4]);
        let actions = vec![LegalAction::Reach, dahai(TENPAI_DRAWN), dahai(4)];
        assert_eq!(agent.act(&ctx, &actions), LegalAction::Reach);
    }

    #[test]
    fn prefers_suited_safety_fallback_over_reach() {
        let mut agent = ShantenAgent;
        // 共通現物も字牌もなし。4m を4枚見えにして 2m を NoChance にすると Reach より優先する。
        let ctx = suited_reach_context(Some(0), &[], &[12, 13, 14, 15], &[]);
        let actions = vec![LegalAction::Reach, dahai(0), dahai(4)];
        assert_eq!(agent.act(&ctx, &actions), dahai(4));
    }

    #[test]
    fn push_prefers_normal_discard_over_the_defense_fallback() {
        let mut agent = ShantenAgent;
        // 強いテンパイで単独の子リーチに対しては Push。Reach が合法でなければ通常打牌へ進み、
        // 現物 17(5m) より通常打牌を優先する。
        let ctx = tenpai_under_reach_context(None, [false, true, false, false]);
        let actions = tenpai_dahai_actions();
        assert_eq!(
            decide_push_pull(&push_pull_inputs_from_context(&ctx, &actions)).mode,
            PushPullMode::Push
        );
        let normal = select_discard_action(&ctx, &actions).unwrap();
        assert_ne!(normal, dahai(17));
        assert_eq!(agent.act(&ctx, &actions), normal);
    }

    #[test]
    fn suited_safety_fallback_ignores_non_dahai_actions() {
        let mut agent = ShantenAgent;
        // 数牌の Pon はあっても数牌 Dahai が無ければ数牌防御 fallback は発動しない。
        let ctx = suited_reach_context(Some(0), &[], &[4, 5, 6, 7], &[]);
        let actions = vec![
            LegalAction::Pon {
                tile: tile(4),
                consumed: vec![tile(5), tile(6)],
            },
            LegalAction::None,
        ];
        assert_eq!(agent.act(&ctx, &actions), LegalAction::None);
    }

    #[test]
    fn push_tenpai_against_single_non_dealer_reaches() {
        let mut agent = ShantenAgent;
        // 単独の子リーチに対するテンパイ。decide_push_pull は Push。
        // Reach が合法でリーチ判断も Eligible なら、現物があっても Reach を選ぶ。
        let ctx = tenpai_under_reach_context(None, [false, true, false, false]);
        let actions = tenpai_actions();
        assert_eq!(
            decide_push_pull(&push_pull_inputs_from_context(&ctx, &actions)).mode,
            PushPullMode::Push
        );
        assert_eq!(
            reach_diagnostic(&ctx, &actions).reason,
            ReachDecisionReason::Eligible
        );
        assert_eq!(agent.act(&ctx, &actions), LegalAction::Reach);
    }

    #[test]
    fn push_strong_tenpai_against_dealer_reach_reaches() {
        let mut agent = ShantenAgent;
        // 親リーチでも強いテンパイなら押す。Push の順序どおり Reach を最優先する。
        let ctx = tenpai_under_reach_context(Some(1), [false, true, false, false]);
        let inputs = push_pull_inputs_from_context(&ctx, &tenpai_actions());
        assert!(inputs.dealer_reacher);
        let decision = decide_push_pull(&inputs);
        assert_eq!(decision.mode, PushPullMode::Push);
        assert_eq!(decision.reason, PushPullReason::StrongTenpaiAgainstReach);

        assert_eq!(agent.act(&ctx, &tenpai_actions()), LegalAction::Reach);
    }

    #[test]
    fn push_strong_tenpai_against_multiple_reach_reaches() {
        let mut agent = ShantenAgent;
        // 複数リーチでも強いテンパイなら押す。
        let ctx = tenpai_under_reach_context(None, [false, true, true, false]);
        let inputs = push_pull_inputs_from_context(&ctx, &tenpai_actions());
        assert_eq!(inputs.opponent_reach_count, 2);
        let decision = decide_push_pull(&inputs);
        assert_eq!(decision.mode, PushPullMode::Push);
        assert_eq!(decision.reason, PushPullReason::StrongTenpaiAgainstReach);

        assert_eq!(agent.act(&ctx, &tenpai_actions()), LegalAction::Reach);
    }

    #[test]
    fn fold_weak_tenpai_against_a_reach_prefers_the_defense_fallback() {
        let mut agent = ShantenAgent;
        // 待ち枚数が足りないテンパイは押さない。Reach が合法でも抑制し、現物を優先する。
        let ctx = weak_tenpai_under_reach_context();
        let mut actions = vec![LegalAction::Reach];
        actions.extend(weak_tenpai_actions());
        let inputs = push_pull_inputs_from_context(&ctx, &actions);
        let wait = inputs
            .offense
            .and_then(|offense| offense.tenpai_wait_after_discard)
            .expect("テンパイの待ち facts がある");
        assert_eq!(wait.permanent_furiten, PermanentFuriten::No);
        assert!(wait.tsumo_remaining < 6);

        let decision = decide_push_pull(&inputs);
        assert_eq!(decision.mode, PushPullMode::Fold);
        assert_eq!(decision.reason, PushPullReason::WeakTenpaiAgainstReach);

        let normal = select_discard_action(&ctx, &actions).unwrap();
        let defense = select_defense_fallback_action(&ctx, &actions)
            .cloned()
            .unwrap();
        let selected = agent.act(&ctx, &actions);
        assert_eq!(selected, defense);
        assert_ne!(selected, normal);
        assert_ne!(selected, LegalAction::Reach);
    }

    #[test]
    fn fold_weak_tenpai_against_a_dealer_or_multiple_reach_folds() {
        // 親リーチ・複数リーチでも弱いテンパイは同じく降りる。
        for reached in [[false, true, false, false], [false, true, true, false]] {
            let ctx = weak_tenpai_under_reach_context_with(Some(1), reached);
            let mut actions = vec![LegalAction::Reach];
            actions.extend(weak_tenpai_actions());
            let decision = decide_push_pull(&push_pull_inputs_from_context(&ctx, &actions));
            assert_eq!(decision.mode, PushPullMode::Fold);
            assert_eq!(decision.reason, PushPullReason::WeakTenpaiAgainstReach);
        }
    }

    #[test]
    fn fold_prefers_defense_fallback_over_normal_discard() {
        let mut agent = ShantenAgent;
        // 二向聴以上で他家リーチを受ける Fold 局面。防御 fallback(現物 5s)と通常打牌が異なり、
        // Fold では防御 fallback を通常打牌より優先する。
        let ctx = fold_under_reach_context();
        let actions = fold_actions();
        assert_eq!(
            decide_push_pull(&push_pull_inputs_from_context(&ctx, &actions)).mode,
            PushPullMode::Fold
        );
        let normal = select_discard_action(&ctx, &actions).unwrap();
        let defense = select_defense_fallback_action(&ctx, &actions)
            .cloned()
            .unwrap();
        assert_ne!(normal, defense);
        assert_eq!(agent.act(&ctx, &actions), defense);
        assert_eq!(defense, dahai(89));
    }

    #[test]
    fn agent_push_pull_uses_legal_candidate_evaluation_not_illegal_global_best() {
        // 全体最善(手牌の東を切ってテンパイ = shanten 0)は非合法で、合法なのはツモ切り 3p だけ。
        // Agent は合法候補側の evaluation / 押し引き判断を使う。
        let hand_values = [0, 4, 8, 12, 17, 20, 24, 28, 32, 36, 40, 89, 108];
        let drawn = 44; // 3p ツモ
        let discards = [vec![], vec![tile(16)], vec![], vec![]];
        let ctx = GameContext::from_parts_with_table_state(
            Some(tile(drawn)),
            hand_values.iter().map(|&value| tile(value)).collect(),
            vec![],
            None,
            None,
            Vec::new(),
            Some(0),
            None,
            discards,
            [false, true, false, false],
        );
        let tiles: Vec<_> = ctx
            .hand_tiles()
            .iter()
            .copied()
            .chain(ctx.drawn_tile())
            .collect();

        // 非合法な全体最善候補はテンパイだが、単騎の待ちは 3 枚しかないので弱いテンパイ。
        let global_best = select_best_normal_discard_evaluation(&ctx, &tiles, &[]).unwrap();
        assert_eq!(global_best.min_shanten_after_discard(), 0);
        // 合法なのはツモ切り 3p だけ。
        let actions = vec![dahai(drawn)];
        let illegal_inputs =
            push_pull_inputs_from_context_with_evaluation(&ctx, Some(&global_best), &actions);
        let illegal_wait = illegal_inputs
            .offense
            .and_then(|offense| offense.tenpai_wait_after_discard)
            .expect("テンパイの待ち facts がある");
        assert_eq!(illegal_wait.tsumo_remaining, 3);
        let illegal_reason = decide_push_pull(&illegal_inputs).reason;
        assert_eq!(illegal_reason, PushPullReason::WeakTenpaiAgainstReach);

        // Agent が使う offense は合法候補の評価に一致する。
        let selection = select_discard_action_with_evaluation(&ctx, &actions);
        let legal_evaluation = selection.evaluation.clone().unwrap();
        assert_eq!(legal_evaluation.min_shanten_after_discard(), 1);
        let legal_inputs = push_pull_inputs_from_context_with_evaluation(
            &ctx,
            selection.evaluation.as_ref(),
            &actions,
        );
        let legal_reason = decide_push_pull(&legal_inputs).reason;

        assert_ne!(illegal_reason, legal_reason);
        assert_eq!(legal_reason, PushPullReason::IishantenAgainstReach);
        assert_eq!(
            legal_inputs.offense.unwrap().min_shanten_after_discard,
            legal_evaluation.min_shanten_after_discard()
        );
        assert_eq!(
            legal_inputs.offense.unwrap().tenpai_wait_after_discard,
            None
        );

        // Agent は合法候補(ツモ切り 3p)を切る。
        let mut agent = ShantenAgent;
        assert_eq!(agent.act(&ctx, &actions), dahai(drawn));
    }

    // ---- decide() の選択経路(AgentActionSource)テスト ----
    // tracing の出力文字列に依存せず、pure な診断構造(AgentDecision)を検証する。

    #[test]
    fn decide_reports_hora_source() {
        let agent = ShantenAgent;
        let ctx = opponent_reach_context(Some(0), &[]);
        let actions = vec![dahai(16), LegalAction::Hora];
        let decision = agent.decide(&ctx, &actions);
        assert_eq!(decision.action, LegalAction::Hora);
        assert_eq!(decision.source, AgentActionSource::Hora);
        assert_eq!(decision.push_pull, None);
        assert_eq!(decision.normal_discard, None);
        assert_eq!(decision.source.defense_kind(), None);
    }

    #[test]
    fn decide_reports_ryukyoku_source() {
        let agent = ShantenAgent;
        let ctx = opponent_reach_context(Some(0), &[]);
        let actions = vec![dahai(16), LegalAction::Ryukyoku];
        let decision = agent.decide(&ctx, &actions);
        assert_eq!(decision.action, LegalAction::Ryukyoku);
        assert_eq!(decision.source, AgentActionSource::Ryukyoku);
        assert_eq!(decision.push_pull, None);
        assert_eq!(decision.normal_discard, None);
    }

    #[test]
    fn decide_reports_reach_source_on_push() {
        let agent = ShantenAgent;
        // 他家リーチなし → Push。選んだ打牌後のテンパイが十分な待ちを持つなら Reach を選ぶ。
        let ctx = tenpai_context(&[]);
        let actions = tenpai_actions();
        let decision = agent.decide(&ctx, &actions);
        assert_eq!(decision.action, LegalAction::Reach);
        assert_eq!(decision.source, AgentActionSource::Reach);
        assert_eq!(decision.push_pull.map(|d| d.mode), Some(PushPullMode::Push));
        assert_eq!(
            decision.reach.map(|reach| reach.reason),
            Some(ReachDecisionReason::Eligible)
        );
    }

    #[test]
    fn decide_reports_normal_discard_source_on_push() {
        let agent = ShantenAgent;
        // 他家リーチなし → Push。Reach が無ければ通常打牌。
        let hand_values = [0, 4, 8, 12, 17, 20, 24, 28, 32, 36, 40, 44, 89];
        let ctx = GameContext::from_parts(
            Some(tile(116)),
            hand_values.iter().map(|&value| tile(value)).collect(),
        );
        let actions: Vec<LegalAction> = hand_values
            .iter()
            .map(|&value| dahai(value))
            .chain([dahai(116)])
            .collect();
        let normal = select_discard_action(&ctx, &actions).unwrap();
        let decision = agent.decide(&ctx, &actions);
        assert_eq!(decision.source, AgentActionSource::NormalDiscard);
        assert_eq!(decision.action, normal);
        assert_eq!(decision.normal_discard, Some(normal));
    }

    #[test]
    fn the_neutral_ordering_prefers_the_normal_discard_over_the_defense_fallback() {
        // 現在の押し引き policy は Neutral を返さないが、action 順序としては維持している。
        // 通常打牌 → 防御 fallback で、Reach は検討しない。
        let ctx = fold_under_reach_context();
        let mut actions = vec![LegalAction::Reach];
        actions.extend(fold_actions());
        let selection = select_discard_action_with_evaluation(&ctx, &actions);
        let normal = selection.action.clone().expect("通常打牌を選べる");
        let inputs = push_pull_inputs_from_context(&ctx, &actions);
        let mut reach = None;
        let mut diagnostics = DecisionDiagnostics::disabled();

        let (action, source) = ShantenAgent
            .select_action_for_push_pull_mode(
                PushPullMode::Neutral,
                &ctx,
                &actions,
                &inputs,
                &selection,
                &mut reach,
                &mut diagnostics,
            )
            .expect("action を選べる");

        assert_eq!(action, normal);
        assert_eq!(source, AgentActionSource::NormalDiscard);
        assert_eq!(reach, None);
    }

    #[test]
    fn decide_reports_defense_fallback_genbutsu_source_on_fold() {
        let agent = ShantenAgent;
        // 二向聴以上の Fold 局面。防御 fallback(現物 5s)を採用し、通常打牌とは異なる。
        let ctx = fold_under_reach_context();
        let actions = fold_actions();
        let normal = select_discard_action(&ctx, &actions).unwrap();
        let defense = select_defense_fallback_action(&ctx, &actions)
            .cloned()
            .unwrap();
        let decision = agent.decide(&ctx, &actions);
        assert_eq!(
            decision.source,
            AgentActionSource::DefenseFallback(DefenseFallbackKind::Genbutsu)
        );
        assert_eq!(decision.action, defense);
        assert_eq!(decision.normal_discard, Some(normal.clone()));
        assert_ne!(decision.normal_discard, Some(decision.action.clone()));
        assert_ne!(normal, defense);
        assert_eq!(decision.push_pull.map(|d| d.mode), Some(PushPullMode::Fold));
        assert_eq!(
            decision.source.defense_kind(),
            Some(DefenseFallbackKind::Genbutsu)
        );
    }

    #[test]
    fn defense_trace_does_not_collect_exact_evidence_for_genbutsu() {
        let agent = ShantenAgent;
        let ctx = fold_under_reach_context();
        let actions = fold_actions();
        let diagnostics = DecisionDiagnostics::disabled();

        let without_trace = evaluate_reach_defense(&ctx, &actions, diagnostics.is_enabled());
        assert_eq!(
            without_trace.selected.map(|(_, kind)| kind),
            Some(DefenseFallbackKind::Genbutsu)
        );
        assert_eq!(without_trace.ron_risk_vectors, None);

        let with_trace = with_defense_trace(|| {
            assert!(tracing::enabled!(
                target: "bot_core::defense",
                tracing::Level::TRACE
            ));
            evaluate_reach_defense(&ctx, &actions, diagnostics.is_enabled())
        });
        assert_eq!(
            with_trace.selected.map(|(_, kind)| kind),
            Some(DefenseFallbackKind::Genbutsu)
        );
        assert_eq!(with_trace.ron_risk_vectors, None);

        let untraced_decision = agent.decide(&ctx, &actions);
        let traced_decision = with_defense_trace(|| agent.decide(&ctx, &actions));
        assert_eq!(traced_decision.action, untraced_decision.action);
        assert_eq!(traced_decision.source, untraced_decision.source);
    }

    #[test]
    fn defense_trace_reuses_exact_evidence_when_selection_requires_it() {
        let agent = ShantenAgent;
        // 共通現物なし。production selector 自体が exact R/T を比較して 2m を選ぶ。
        let ctx = suited_reach_context(Some(0), &[], &[12, 13, 14, 15], &[]);
        let actions = vec![dahai(0), dahai(4)];
        let diagnostics = DecisionDiagnostics::disabled();

        let evaluation =
            with_defense_trace(|| evaluate_reach_defense(&ctx, &actions, diagnostics.is_enabled()));
        assert_eq!(
            evaluation.selected.map(|(_, kind)| kind),
            Some(DefenseFallbackKind::ExactRonRisk)
        );
        assert!(evaluation.ron_risk_vectors.is_some());

        // logging と構造化診断が受け取るのは production evaluation が保持する同じ vector。
        let diagnostic = DefenseDecisionDiagnostic::from_evaluation(&ctx, &actions, &evaluation);
        assert!(
            diagnostic
                .selected
                .as_ref()
                .unwrap()
                .selected_player_ron_risk_evidence
                .is_some()
        );

        let untraced_decision = agent.decide(&ctx, &actions);
        let traced_decision = with_defense_trace(|| agent.decide(&ctx, &actions));
        assert_eq!(traced_decision.action, untraced_decision.action);
        assert_eq!(traced_decision.source, untraced_decision.source);
    }

    // 回帰構造: normal_discard(通常打牌)と最終 selected_action(防御 fallback)が
    // Fold 時に異なり得ることを、SuitedSafety 経路で確認する。
    // 実牌姿での防御は河・visible の正確な再現が必要なため、pure な選択経路として構築する。
    #[test]
    fn decide_reports_exact_ron_risk_defense_source_on_single_reach_fold() {
        let agent = ShantenAgent;
        // 共通現物なしの単独リーチ。ツモ 1m(0) だけが手牌評価対象なので通常打牌は 1m、
        // exact defense fallback は R の小さい 2m。
        let ctx = suited_reach_context(Some(0), &[], &[12, 13, 14, 15], &[]);
        let actions = vec![dahai(0), dahai(4)];
        assert_eq!(
            decide_push_pull(&push_pull_inputs_from_context(&ctx, &actions)).mode,
            PushPullMode::Fold
        );
        let decision = agent.decide(&ctx, &actions);
        assert_eq!(decision.action, dahai(4));
        assert_eq!(
            decision.source,
            AgentActionSource::DefenseFallback(DefenseFallbackKind::ExactRonRisk)
        );
        assert_eq!(decision.normal_discard, Some(dahai(0)));
        assert_ne!(decision.normal_discard, Some(decision.action.clone()));
    }

    #[test]
    fn decide_falls_through_to_normal_discard_when_fold_has_no_defense() {
        let agent = ShantenAgent;
        // Fold だが共通現物・字牌・数牌 safety がいずれも無い局面。通常打牌へ進む。
        let ctx =
            suited_reach_context_with_reached(Some(0), &[], &[], &[], [false, true, true, false]);
        let actions = vec![LegalAction::Reach, dahai(0), dahai(4)];
        assert_eq!(
            decide_push_pull(&push_pull_inputs_from_context(&ctx, &actions)).mode,
            PushPullMode::Fold
        );
        let normal = select_discard_action(&ctx, &actions).unwrap();
        let decision = agent.decide(&ctx, &actions);
        assert_eq!(decision.source, AgentActionSource::NormalDiscard);
        assert_eq!(decision.action, normal);
        assert_eq!(decision.normal_discard, Some(normal));
    }

    #[test]
    fn legal_dahai_fallback_prefers_black_five() {
        // 手牌評価が作れず(手牌なし)、他家リーチも無い局面。通常打牌も防御 fallback も None で
        // LegalDahaiFallback へ落ちる。合法 Dahai [赤5m, 黒5m] なら黒5m を返す。
        let agent = ShantenAgent;
        let ctx = GameContext::default();
        let actions = vec![dahai(16), dahai(17)];
        let decision = agent.decide(&ctx, &actions);
        assert_eq!(decision.action, dahai(17));
        assert_eq!(decision.source, AgentActionSource::LegalDahaiFallback);
    }

    #[test]
    fn legal_dahai_fallback_prefers_black_five_when_reversed() {
        let agent = ShantenAgent;
        let ctx = GameContext::default();
        let actions = vec![dahai(17), dahai(16)];
        let decision = agent.decide(&ctx, &actions);
        assert_eq!(decision.action, dahai(17));
        assert_eq!(decision.source, AgentActionSource::LegalDahaiFallback);
    }

    #[test]
    fn legal_dahai_fallback_keeps_red_five_when_only_red() {
        let agent = ShantenAgent;
        let ctx = GameContext::default();
        let actions = vec![dahai(16)];
        let decision = agent.decide(&ctx, &actions);
        assert_eq!(decision.action, dahai(16));
        assert_eq!(decision.source, AgentActionSource::LegalDahaiFallback);
    }

    #[test]
    fn legal_dahai_fallback_keeps_leading_tile_type() {
        // 合法 Dahai [1p, 赤5m, 黒5m] では先頭牌種 1p を維持する。黒5優先で 5m を前へ出さない。
        let agent = ShantenAgent;
        let ctx = GameContext::default();
        let actions = vec![dahai(36), dahai(16), dahai(17)];
        let decision = agent.decide(&ctx, &actions);
        assert_eq!(decision.action, dahai(36));
        assert_eq!(decision.source, AgentActionSource::LegalDahaiFallback);
    }

    #[test]
    fn decide_reports_none_source_for_empty_actions() {
        let agent = ShantenAgent;
        let ctx = GameContext::default();
        let decision = agent.decide(&ctx, &[]);
        assert_eq!(decision.action, LegalAction::None);
        assert_eq!(decision.source, AgentActionSource::None);
    }

    fn diagnose_matching_act(
        ctx: &GameContext,
        actions: &[LegalAction],
    ) -> ShantenDecisionDiagnostic {
        let mut agent = ShantenAgent;
        let expected = agent.act(ctx, actions);
        let diagnostic = ShantenAgent::diagnose(ctx, actions);
        assert_eq!(diagnostic.selected_action, expected);
        assert_eq!(
            diagnostic.selected_source,
            agent.decide(ctx, actions).source
        );
        diagnostic
    }

    // ---- 鳴き (AgentActionSource::Call) テスト ----

    // 他家(player 1)が捨てた牌への Chi / Pon reaction 局面を組み立てる。
    // 既定は東場東家・リーチ者なし・副露なし・ツモ牌なし・履歴依存フリテンなしで、検証したい
    // 条件だけ差し替える。
    #[derive(Debug, Clone)]
    struct CallReaction {
        kind: CallKind,
        hand: Vec<u8>,
        target: u8,
        consumed: Vec<u8>,
        round_wind: Option<u8>,
        seat_wind: Option<u8>,
        reached: [bool; 4],
        extra_visible: Vec<u8>,
        own_discards: Vec<u8>,
        own_melds: Vec<crate::meld::Meld>,
        opponent_melds: Vec<crate::meld::Meld>,
        drawn_tile: Option<u8>,
        player_id: Option<u8>,
        history_furiten: bot_logic::HistoryFuritenFacts,
    }

    impl CallReaction {
        fn pon(hand: &[u8], target: u8, consumed: &[u8]) -> Self {
            Self::new(CallKind::Pon, hand, target, consumed)
        }

        fn chi(hand: &[u8], target: u8, consumed: &[u8]) -> Self {
            Self::new(CallKind::Chi, hand, target, consumed)
        }

        fn new(kind: CallKind, hand: &[u8], target: u8, consumed: &[u8]) -> Self {
            Self {
                kind,
                hand: hand.to_vec(),
                target,
                consumed: consumed.to_vec(),
                round_wind: Some(27),
                seat_wind: Some(27),
                reached: [false; 4],
                extra_visible: Vec::new(),
                own_discards: Vec::new(),
                own_melds: Vec::new(),
                opponent_melds: Vec::new(),
                drawn_tile: None,
                player_id: Some(0),
                // 実際の client が局開始で確定させる値。unknown を既定にすると全ての鳴きが
                // ロン可否不明で落ちるため、既定は既知の非フリテンにする。
                history_furiten: known_history_furiten(),
            }
        }

        fn with_winds(mut self, round_wind: Option<u8>, seat_wind: Option<u8>) -> Self {
            self.round_wind = round_wind;
            self.seat_wind = seat_wind;
            self
        }

        fn with_reached(mut self, reached: [bool; 4]) -> Self {
            self.reached = reached;
            self
        }

        fn with_extra_visible(mut self, extra_visible: &[u8]) -> Self {
            self.extra_visible = extra_visible.to_vec();
            self
        }

        fn with_own_discards(mut self, own_discards: &[u8]) -> Self {
            self.own_discards = own_discards.to_vec();
            self
        }

        fn with_own_melds(mut self, own_melds: Vec<crate::meld::Meld>) -> Self {
            self.own_melds = own_melds;
            self
        }

        fn with_opponent_melds(mut self, opponent_melds: Vec<crate::meld::Meld>) -> Self {
            self.opponent_melds = opponent_melds;
            self
        }

        fn with_drawn_tile(mut self, drawn_tile: u8) -> Self {
            self.drawn_tile = Some(drawn_tile);
            self
        }

        fn without_player_id(mut self) -> Self {
            self.player_id = None;
            self
        }

        fn with_consumed(mut self, consumed: &[u8]) -> Self {
            self.consumed = consumed.to_vec();
            self
        }

        fn with_history_furiten(mut self, history_furiten: bot_logic::HistoryFuritenFacts) -> Self {
            self.history_furiten = history_furiten;
            self
        }

        fn context(&self) -> GameContext {
            let hand: Vec<TileId> = self.hand.iter().map(|&value| tile(value)).collect();
            let own_discards: Vec<TileId> =
                self.own_discards.iter().map(|&value| tile(value)).collect();
            let discards = [
                own_discards.clone(),
                vec![tile(self.target)],
                vec![],
                vec![],
            ];

            let mut visible = hand.clone();
            visible.extend(self.drawn_tile.map(tile));
            visible.push(tile(self.target));
            visible.extend(own_discards);
            visible.extend(self.own_melds.iter().flat_map(|meld| meld.tiles().to_vec()));
            visible.extend(
                self.opponent_melds
                    .iter()
                    .flat_map(|meld| meld.tiles().to_vec()),
            );
            visible.extend(self.extra_visible.iter().map(|&value| tile(value)));

            let mut melds: [Vec<crate::meld::Meld>; 4] = Default::default();
            melds[0] = self.own_melds.clone();
            melds[1] = self.opponent_melds.clone();

            GameContext::from_parts_with_melds(
                self.drawn_tile.map(tile),
                hand,
                vec![],
                self.round_wind.and_then(TileType::new),
                self.seat_wind.and_then(TileType::new),
                visible,
                self.player_id,
                Some(0),
                discards,
                self.reached,
                melds,
            )
            .with_history_furiten_facts(self.history_furiten)
        }

        fn call(&self) -> LegalAction {
            let called_tile = tile(self.target);
            let consumed: Vec<TileId> = self.consumed.iter().map(|&value| tile(value)).collect();
            match self.kind {
                CallKind::Chi => LegalAction::Chi {
                    tile: called_tile,
                    consumed,
                },
                CallKind::Pon => LegalAction::Pon {
                    tile: called_tile,
                    consumed,
                },
            }
        }

        fn actions(&self) -> Vec<LegalAction> {
            vec![self.call(), LegalAction::None]
        }

        // consumed を除いた鳴き後 concealed hand の物理牌一覧。
        fn post_call_tiles(&self) -> Vec<TileId> {
            let mut remaining: Vec<TileId> = self.hand.iter().map(|&value| tile(value)).collect();
            for consumed in &self.consumed {
                let position = remaining
                    .iter()
                    .position(|held| *held == tile(*consumed))
                    .expect("consumed は手牌にある");
                remaining.remove(position);
            }
            remaining
        }
    }

    // 実際の client が局開始で確定させる履歴依存フリテン。
    fn known_history_furiten() -> bot_logic::HistoryFuritenFacts {
        bot_logic::HistoryFuritenFacts {
            same_turn: Some(false),
            riichi_missed_win: Some(false),
        }
    }

    fn honor_pon_meld(first: u8) -> crate::meld::Meld {
        crate::meld::Meld::new(
            crate::meld::MeldKind::Pon,
            vec![tile(first), tile(first + 1), tile(first + 2)],
            Some(tile(first)),
        )
    }

    fn high_open_hand_melds() -> Vec<crate::meld::Meld> {
        [108, 112, 116].map(honor_pon_meld).to_vec()
    }

    // 123456m 55p 78s N PP。P(白) の対子を持つ一向聴で、PP を Pon して N を切るとテンパイ。
    const PON_HAND: [u8; 13] = [0, 4, 8, 12, 17, 20, 53, 54, 96, 100, 120, 124, 125];
    const PON_TARGET: u8 = 126;
    const PON_CONSUMED: [u8; 2] = [124, 125];

    fn dragon_pon_reaction() -> CallReaction {
        CallReaction::pon(&PON_HAND, PON_TARGET, &PON_CONSUMED)
    }

    // 診断の候補は1件で、その理由と最終 action が期待どおりであることを確認する。
    fn assert_single_call_candidate(
        reaction: &CallReaction,
        expected_action: &LegalAction,
        expected_reason: CallDecisionReason,
    ) -> CallCandidateDiagnostic {
        let ctx = reaction.context();
        let actions = reaction.actions();
        let diagnostic = diagnose_matching_act(&ctx, &actions);

        assert_eq!(&diagnostic.selected_action, expected_action);

        let call = diagnostic.call.as_ref().unwrap();
        assert_eq!(call.reason, expected_reason);
        assert_eq!(call.candidates.len(), 1);

        let candidate = call.candidates[0].clone();
        assert_eq!(candidate.reason, expected_reason);
        assert_eq!(
            candidate.eligible,
            matches!(
                expected_reason,
                CallDecisionReason::EligibleTenpai | CallDecisionReason::EligibleIishantenSelfTsumo
            )
        );
        assert_eq!(candidate.selected, candidate.eligible);
        assert_eq!(
            call.selected.as_ref(),
            candidate.eligible.then_some(&candidate.action)
        );
        candidate
    }

    fn assert_call_is_declined(reaction: &CallReaction, expected_reason: CallDecisionReason) {
        let candidate = assert_single_call_candidate(reaction, &LegalAction::None, expected_reason);
        assert!(!candidate.eligible);
    }

    #[test]
    fn pons_value_honor_pair_that_reaches_a_live_tenpai() {
        let reaction = dragon_pon_reaction();
        let candidate = assert_single_call_candidate(
            &reaction,
            &reaction.call(),
            CallDecisionReason::EligibleTenpai,
        );

        assert_eq!(candidate.kind, CallKind::Pon);
        assert_eq!(candidate.current_shanten, Some(1));
        assert_eq!(
            candidate.current_fixed_meld_count.map(FixedMeldCount::get),
            Some(0)
        );
        assert_eq!(
            candidate
                .post_call_fixed_meld_count
                .map(FixedMeldCount::get),
            Some(1)
        );
        assert_eq!(candidate.post_call_shanten(), Some(0));
        assert_eq!(candidate.post_call_acceptance_total_remaining(), Some(8));
        assert_eq!(candidate.post_call_acceptance_type_count(), Some(2));
        assert_eq!(candidate.live_wait_remaining(), Some(8));
        assert_eq!(candidate.can_ron(), Some(true));
        assert_eq!(candidate.live_waits_have_yaku(), Some(true));

        let evaluation = candidate.post_call_discard.as_ref().unwrap();
        assert_eq!(evaluation.discard.to_mjai_string(), "N");
        let acceptance: Vec<(String, u8)> = evaluation
            .acceptance_after_discard
            .tiles
            .iter()
            .map(|entry| (entry.tile.to_mjai_string(), entry.remaining))
            .collect();
        assert_eq!(
            acceptance,
            vec![("6s".to_string(), 4), ("9s".to_string(), 4)]
        );

        // 役の有無は和了牌の物理牌ごとに既存 HandValue で確認する。
        let wait_yaku: Vec<(String, u8, CallWaitYaku)> = candidate
            .post_call_wait_yaku
            .as_ref()
            .unwrap()
            .iter()
            .map(|wait| {
                (
                    wait.winning_tile.to_mjai_string(),
                    wait.remaining,
                    wait.yaku,
                )
            })
            .collect();
        assert_eq!(
            wait_yaku,
            vec![
                ("6s".to_string(), 4, CallWaitYaku::Present),
                ("9s".to_string(), 4, CallWaitYaku::Present),
            ]
        );
    }

    #[test]
    fn declines_a_call_when_its_post_call_tenpai_folds_against_a_high_open_hand() {
        // Call 単体では白 Pon → 北切りの役あり8枚待ちテンパイだが、3副露の相手に対しては
        // 既存 Push/Pull policy が弱いテンパイとして Fold にするため、Pon 自体を採用しない。
        let reaction = dragon_pon_reaction().with_opponent_melds(high_open_hand_melds());
        let candidate = assert_single_call_candidate(
            &reaction,
            &LegalAction::None,
            CallDecisionReason::PostCallNotPush,
        );

        assert_eq!(candidate.post_call_shanten(), Some(CALL_TENPAI_SHANTEN));
        assert_eq!(candidate.live_wait_remaining(), Some(8));
        assert_eq!(candidate.can_ron(), Some(true));
        assert_eq!(candidate.live_waits_have_yaku(), Some(true));
        assert_eq!(
            candidate.post_call_push_pull,
            Some(PushPullDecision {
                mode: PushPullMode::Fold,
                reason: PushPullReason::WeakTenpaiAgainstHighOpenHand,
            })
        );
    }

    #[test]
    fn call_source_is_reported_for_the_selected_call() {
        let reaction = dragon_pon_reaction();
        let ctx = reaction.context();
        let actions = reaction.actions();

        let mut agent = ShantenAgent;
        assert_eq!(agent.act(&ctx, &actions), reaction.call());

        let decision = ShantenAgent.decide(&ctx, &actions);
        assert_eq!(decision.source, AgentActionSource::Call);
        // 鳴き後候補の Push/Pull は call 診断内に保持する。現在局面の通常打牌・Push/Pull・防御は
        // 鳴きの採用後には進まないため、AgentDecision の各フィールドには持たない。
        assert_eq!(decision.push_pull, None);
        assert_eq!(decision.push_pull_inputs, None);
        assert_eq!(decision.normal_discard, None);

        let diagnostic = ShantenAgent::diagnose(&ctx, &actions);
        assert_eq!(diagnostic.selected_source, AgentActionSource::Call);
        assert_eq!(diagnostic.normal_discard, None);
        assert_eq!(diagnostic.push_pull_decision, None);
        assert_eq!(diagnostic.defense, None);
    }

    #[test]
    fn post_call_evaluation_matches_the_shared_discard_helper() {
        // 診断が持つ鳴き後の打牌評価は、本番の打牌評価 helper の結果そのものである。
        let reaction = dragon_pon_reaction();
        let ctx = reaction.context();
        let candidate = assert_single_call_candidate(
            &reaction,
            &reaction.call(),
            CallDecisionReason::EligibleTenpai,
        );

        let post_call_tiles: Vec<TileId> = PON_HAND
            .iter()
            .filter(|value| !PON_CONSUMED.contains(value))
            .map(|&value| tile(value))
            .collect();
        let expected =
            crate::discard_selection::select_best_one_step_discard_evaluation_with_fixed_meld_count(
                &ctx,
                &post_call_tiles,
                FixedMeldCount::new(1).unwrap(),
                candidate.post_call_forbidden_discards.as_deref().unwrap(),
            );

        assert_eq!(candidate.post_call_discard, expected);
        assert!(expected.is_some());
    }

    #[test]
    fn pons_round_wind_pair() {
        // 東場・南家。場風の東は既存 HandValue で役になる。
        let reaction = CallReaction::pon(
            &[0, 4, 8, 12, 17, 20, 53, 54, 96, 100, 120, 108, 109],
            110,
            &[108, 109],
        )
        .with_winds(Some(27), Some(28));

        let candidate = assert_single_call_candidate(
            &reaction,
            &reaction.call(),
            CallDecisionReason::EligibleTenpai,
        );
        assert_eq!(candidate.current_shanten, Some(1));
        assert_eq!(candidate.post_call_shanten(), Some(0));
        assert_eq!(candidate.post_call_acceptance_total_remaining(), Some(8));
        assert_eq!(candidate.live_waits_have_yaku(), Some(true));
    }

    #[test]
    fn does_not_pon_guest_wind_pair_without_any_yaku() {
        // 東場・南家の西は場風でも自風でもないので、鳴いても全ての待ちで役なしになる。
        let reaction = CallReaction::pon(
            &[0, 4, 8, 12, 17, 20, 53, 54, 96, 100, 120, 116, 117],
            118,
            &[116, 117],
        )
        .with_winds(Some(27), Some(28));

        let candidate = assert_single_call_candidate(
            &reaction,
            &LegalAction::None,
            CallDecisionReason::YakuMissing,
        );
        assert_eq!(candidate.post_call_shanten(), Some(0));
        assert_eq!(candidate.live_wait_remaining(), Some(8));
        assert_eq!(candidate.can_ron(), Some(true));
        assert_eq!(candidate.live_waits_have_yaku(), Some(false));
    }

    #[test]
    fn pons_a_pair_that_is_not_a_value_honor_when_every_live_wait_has_a_yaku() {
        // 234m 567m 456s 33p 2s 8s。3p は役牌ではないが、Pon 後は全ての待ちで断么九が付く。
        let reaction = CallReaction::pon(
            &[4, 8, 12, 17, 20, 24, 84, 89, 92, 44, 45, 76, 100],
            46,
            &[44, 45],
        );

        let candidate = assert_single_call_candidate(
            &reaction,
            &reaction.call(),
            CallDecisionReason::EligibleTenpai,
        );
        assert_eq!(candidate.kind, CallKind::Pon);
        assert_eq!(candidate.current_shanten, Some(1));
        assert_eq!(candidate.post_call_shanten(), Some(0));
        assert_eq!(candidate.live_wait_remaining(), Some(3));
        assert_eq!(candidate.live_waits_have_yaku(), Some(true));
    }

    // 1m 1s2s3s 3s4s 6s7s 99s PPP。5s を Chi して 1m を切ると混一色のテンパイになる一向聴。
    const CHI_HAND: [u8; 13] = [0, 72, 76, 80, 81, 84, 92, 96, 104, 105, 124, 125, 126];
    const CHI_TARGET: u8 = 89;
    const CHI_CONSUMED_LOWER: [u8; 2] = [80, 84];
    const CHI_CONSUMED_UPPER: [u8; 2] = [92, 96];

    fn honitsu_chi_reaction() -> CallReaction {
        CallReaction::chi(&CHI_HAND, CHI_TARGET, &CHI_CONSUMED_LOWER)
    }

    #[test]
    fn chis_a_sequence_that_reaches_a_live_tenpai_with_a_yaku() {
        let reaction = honitsu_chi_reaction();
        let candidate = assert_single_call_candidate(
            &reaction,
            &reaction.call(),
            CallDecisionReason::EligibleTenpai,
        );

        assert_eq!(candidate.kind, CallKind::Chi);
        assert_eq!(candidate.current_shanten, Some(1));
        assert_eq!(
            candidate
                .post_call_fixed_meld_count
                .map(FixedMeldCount::get),
            Some(1)
        );
        assert_eq!(candidate.post_call_shanten(), Some(0));
        assert_eq!(
            candidate
                .post_call_discard
                .as_ref()
                .unwrap()
                .discard
                .to_mjai_string(),
            "1m"
        );
        assert_eq!(candidate.live_wait_remaining(), Some(7));
        assert_eq!(candidate.can_ron(), Some(true));
        assert_eq!(candidate.live_waits_have_yaku(), Some(true));
    }

    #[test]
    fn keeps_a_call_when_its_post_call_tenpai_pushes_against_a_high_open_hand() {
        let reaction = honitsu_chi_reaction().with_opponent_melds(high_open_hand_melds());
        let candidate = assert_single_call_candidate(
            &reaction,
            &reaction.call(),
            CallDecisionReason::EligibleTenpai,
        );

        assert_eq!(candidate.post_call_shanten(), Some(CALL_TENPAI_SHANTEN));
        assert_eq!(candidate.can_ron(), Some(true));
        assert_eq!(candidate.live_waits_have_yaku(), Some(true));
        assert_eq!(
            candidate.post_call_push_pull,
            Some(PushPullDecision {
                mode: PushPullMode::Push,
                reason: PushPullReason::StrongTenpaiAgainstHighOpenHand,
            })
        );
    }

    #[test]
    fn chi_evaluates_the_red_and_black_five_as_separate_variants() {
        // 5s 待ちは赤5と黒5で残枚数が分かれる。牌種単位へ潰して1回だけ評価しない。
        let reaction = honitsu_chi_reaction();
        let candidate = assert_single_call_candidate(
            &reaction,
            &reaction.call(),
            CallDecisionReason::EligibleTenpai,
        );

        let wait_yaku: Vec<(String, u8, CallWaitYaku)> = candidate
            .post_call_wait_yaku
            .as_ref()
            .unwrap()
            .iter()
            .map(|wait| {
                (
                    wait.winning_tile.to_mjai_string(),
                    wait.remaining,
                    wait.yaku,
                )
            })
            .collect();
        assert_eq!(
            wait_yaku,
            vec![
                ("5sr".to_string(), 1, CallWaitYaku::Present),
                ("5s".to_string(), 2, CallWaitYaku::Present),
                ("8s".to_string(), 4, CallWaitYaku::Present),
            ]
        );
        // 物理牌ごとの残枚数の合計は既存受け入れの残枚数と一致する。
        assert_eq!(candidate.live_wait_remaining(), Some(7));
    }

    #[test]
    fn does_not_call_with_a_partial_yaku_wait() {
        // 234m 567m 9m 55p 23s + 3p Pon。4s は断么九だが 1s は役なしなので鳴かない。
        let reaction = CallReaction::pon(
            &[4, 8, 12, 17, 20, 24, 32, 53, 54, 44, 45, 76, 80],
            46,
            &[44, 45],
        );

        let candidate = assert_single_call_candidate(
            &reaction,
            &LegalAction::None,
            CallDecisionReason::YakuMissing,
        );
        assert_eq!(candidate.post_call_shanten(), Some(0));
        assert_eq!(candidate.live_wait_remaining(), Some(8));

        let wait_yaku: Vec<(String, u8, CallWaitYaku)> = candidate
            .post_call_wait_yaku
            .as_ref()
            .unwrap()
            .iter()
            .map(|wait| {
                (
                    wait.winning_tile.to_mjai_string(),
                    wait.remaining,
                    wait.yaku,
                )
            })
            .collect();
        assert_eq!(
            wait_yaku,
            vec![
                ("1s".to_string(), 4, CallWaitYaku::Absent),
                ("4s".to_string(), 4, CallWaitYaku::Present),
            ]
        );
    }

    #[test]
    fn a_dead_wait_without_a_yaku_does_not_block_the_call() {
        // 同じ形でも 1s が場に4枚見えていれば、役なしの 1s は現在和了できないので邪魔しない。
        let reaction = CallReaction::pon(
            &[4, 8, 12, 17, 20, 24, 32, 53, 54, 44, 45, 76, 80],
            46,
            &[44, 45],
        )
        .with_extra_visible(&[72, 73, 74, 75]);

        let candidate = assert_single_call_candidate(
            &reaction,
            &reaction.call(),
            CallDecisionReason::EligibleTenpai,
        );
        assert_eq!(candidate.live_wait_remaining(), Some(4));

        let wait_yaku: Vec<(String, u8, CallWaitYaku)> = candidate
            .post_call_wait_yaku
            .as_ref()
            .unwrap()
            .iter()
            .map(|wait| {
                (
                    wait.winning_tile.to_mjai_string(),
                    wait.remaining,
                    wait.yaku,
                )
            })
            .collect();
        assert_eq!(
            wait_yaku,
            vec![("4s".to_string(), 4, CallWaitYaku::Present)]
        );
    }

    #[test]
    fn calls_at_the_live_wait_threshold() {
        // 生きた待ちが threshold ちょうどの3枚なら、他の条件を満たす限り鳴く。
        let reaction = dragon_pon_reaction().with_extra_visible(&[92, 93, 94, 95, 104]);

        let candidate = assert_single_call_candidate(
            &reaction,
            &reaction.call(),
            CallDecisionReason::EligibleTenpai,
        );
        assert_eq!(
            candidate.live_wait_remaining(),
            Some(CALL_MIN_LIVE_WAIT_REMAINING)
        );
    }

    #[test]
    fn does_not_call_below_the_live_wait_threshold() {
        for (extra_visible, expected_remaining) in [
            (vec![92, 93, 94, 95, 104, 105], 2u8),
            (vec![92, 93, 94, 95, 104, 105, 106], 1),
        ] {
            let reaction = dragon_pon_reaction().with_extra_visible(&extra_visible);

            let candidate = assert_single_call_candidate(
                &reaction,
                &LegalAction::None,
                CallDecisionReason::TooFewLiveWaits,
            );
            assert_eq!(
                candidate.live_wait_remaining(),
                Some(expected_remaining),
                "{extra_visible:?}"
            );
            // 役の判定まで進まない。
            assert_eq!(candidate.post_call_wait_yaku, None, "{extra_visible:?}");
        }
    }

    #[test]
    fn does_not_call_when_the_post_call_tenpai_is_furiten() {
        // 待ちの 6s が自分の河にある恒常フリテン。ロンできない鳴きは選ばない。
        let reaction = dragon_pon_reaction().with_own_discards(&[92]);

        let candidate = assert_single_call_candidate(
            &reaction,
            &LegalAction::None,
            CallDecisionReason::CannotRon,
        );
        assert_eq!(candidate.can_ron(), Some(false));
        assert_eq!(candidate.post_call_wait_yaku, None);
    }

    #[test]
    fn does_not_call_when_the_ron_availability_is_unknown() {
        // 履歴依存フリテンが不明な局面では、非フリテンだと推測せず鳴かない。
        let reaction =
            dragon_pon_reaction().with_history_furiten(bot_logic::HistoryFuritenFacts::default());

        let candidate = assert_single_call_candidate(
            &reaction,
            &LegalAction::None,
            CallDecisionReason::CannotRon,
        );
        assert_eq!(candidate.can_ron(), None);
    }

    #[test]
    fn does_not_call_when_the_hand_value_is_unknown() {
        // 場風・自風が不明だと既存 HandValue で役を確定できない。役ありだと推測しない。
        let reaction = dragon_pon_reaction().with_winds(None, None);

        let candidate = assert_single_call_candidate(
            &reaction,
            &LegalAction::None,
            CallDecisionReason::HandValueUnknown,
        );
        assert_eq!(candidate.can_ron(), Some(true));
        assert_eq!(candidate.live_waits_have_yaku(), Some(false));
        let wait_yaku = candidate.post_call_wait_yaku.as_ref().unwrap();
        assert!(
            wait_yaku
                .iter()
                .all(|wait| wait.yaku == CallWaitYaku::Unknown),
            "{wait_yaku:?}"
        );
    }

    #[test]
    fn does_not_call_from_two_shanten() {
        // 123456m 55p 1s 9s N PP。役牌の対子でも2向聴からは鳴かない。
        let reaction = CallReaction::pon(
            &[0, 4, 8, 12, 17, 20, 53, 54, 72, 104, 120, 124, 125],
            PON_TARGET,
            &PON_CONSUMED,
        );

        let candidate = assert_single_call_candidate(
            &reaction,
            &LegalAction::None,
            CallDecisionReason::CurrentShantenNotOne,
        );
        assert_eq!(candidate.current_shanten, Some(2));
        // production では鳴かないが、diagnostics では鳴き後1向聴を観測する。
        assert_eq!(candidate.post_call_shanten(), Some(1));
        assert!(candidate.two_shanten_self_tsumo.is_some());
    }

    #[test]
    fn does_not_call_while_already_tenpai() {
        // 123456789m 55p PP。テンパイ維持の比較は今回の対象外。
        let reaction = CallReaction::pon(
            &[0, 4, 8, 12, 17, 20, 24, 28, 32, 53, 54, 124, 125],
            PON_TARGET,
            &PON_CONSUMED,
        );

        let candidate = assert_single_call_candidate(
            &reaction,
            &LegalAction::None,
            CallDecisionReason::CurrentShantenNotOne,
        );
        assert_eq!(candidate.current_shanten, Some(0));
    }

    #[test]
    fn does_not_call_when_the_best_post_call_discard_is_still_one_shanten() {
        // 12345678m 1p 45p PP。Pon してもテンパイにならない一向聴。
        let reaction = CallReaction::pon(
            &[0, 4, 8, 12, 17, 20, 24, 28, 36, 48, 53, 124, 125],
            PON_TARGET,
            &PON_CONSUMED,
        );

        let candidate = assert_single_call_candidate(
            &reaction,
            &LegalAction::None,
            CallDecisionReason::ReactionSourceUnknown,
        );
        assert_eq!(candidate.current_shanten, Some(1));
        assert_eq!(candidate.post_call_shanten(), Some(1));
        assert_eq!(candidate.post_call_wait, None);
    }

    #[test]
    fn does_not_call_without_live_acceptance() {
        // 6s / 9s が場に4枚ずつ見えている枯れ待ち。形の上ではテンパイでも鳴かない。
        let reaction =
            dragon_pon_reaction().with_extra_visible(&[92, 93, 94, 95, 104, 105, 106, 107]);

        let candidate = assert_single_call_candidate(
            &reaction,
            &LegalAction::None,
            CallDecisionReason::NoLiveAcceptance,
        );
        assert_eq!(candidate.post_call_shanten(), Some(0));
        assert_eq!(candidate.post_call_acceptance_total_remaining(), Some(0));
        assert_eq!(candidate.live_wait_remaining(), Some(0));
    }

    #[test]
    fn does_not_call_under_opponent_reach() {
        for reaction in [
            dragon_pon_reaction().with_reached([false, true, false, false]),
            honitsu_chi_reaction().with_reached([false, false, false, true]),
        ] {
            let candidate = assert_single_call_candidate(
                &reaction,
                &LegalAction::None,
                CallDecisionReason::OpponentReached,
            );
            // リーチ者がいる時点で以降の条件は評価しない。
            assert_eq!(candidate.current_shanten, None);
            assert_eq!(candidate.post_call_discard, None);
        }
    }

    #[test]
    fn evaluates_a_pon_from_a_triplet_through_the_shared_path() {
        // 123456m 55p 1s 9s PPP。対子限定の gating は無いので、暗刻からの Pon も同じ path で
        // 評価する。ここでは Pon 後にテンパイにならないので鳴かない。
        let reaction = CallReaction::pon(
            &[0, 4, 8, 12, 17, 20, 53, 54, 72, 104, 124, 125, 126],
            127,
            &PON_CONSUMED,
        );

        let candidate = assert_single_call_candidate(
            &reaction,
            &LegalAction::None,
            CallDecisionReason::ReactionSourceUnknown,
        );
        assert_eq!(candidate.current_shanten, Some(1));
        assert_eq!(candidate.post_call_shanten(), Some(1));
    }

    #[test]
    fn does_not_call_without_a_known_fixed_meld_count() {
        // player_id が無く自分の副露数が確定できない局面。0副露だろうと推測しない。
        let reaction = dragon_pon_reaction().without_player_id();
        assert_eq!(reaction.context().own_fixed_meld_count(), None);

        let candidate = assert_single_call_candidate(
            &reaction,
            &LegalAction::None,
            CallDecisionReason::FixedMeldCountUnknown,
        );
        assert_eq!(candidate.post_call_fixed_meld_count, None);
    }

    #[test]
    fn calls_with_an_existing_meld() {
        // 東 Pon 済みの 123m 55p 78s N PP。副露済み面子数を1増やして評価する。
        let reaction = CallReaction::pon(
            &[0, 4, 8, 53, 54, 96, 100, 120, 124, 125],
            PON_TARGET,
            &PON_CONSUMED,
        )
        .with_own_melds(vec![pon_meld()]);

        let candidate = assert_single_call_candidate(
            &reaction,
            &reaction.call(),
            CallDecisionReason::EligibleTenpai,
        );
        assert_eq!(
            candidate.current_fixed_meld_count.map(FixedMeldCount::get),
            Some(1)
        );
        assert_eq!(
            candidate
                .post_call_fixed_meld_count
                .map(FixedMeldCount::get),
            Some(2)
        );
        assert_eq!(candidate.current_shanten, Some(1));
        assert_eq!(candidate.post_call_shanten(), Some(0));
        assert_eq!(candidate.post_call_acceptance_total_remaining(), Some(8));
        assert_eq!(
            candidate
                .post_call_discard
                .as_ref()
                .unwrap()
                .discard
                .to_mjai_string(),
            "N"
        );
    }

    #[test]
    fn does_not_call_when_the_post_call_fixed_meld_count_would_overflow() {
        // 副露済み4組からの鳴きは成立しない。silent clamp せず理由として報告する。
        let melds: Vec<crate::meld::Meld> = [108, 112, 116, 120]
            .iter()
            .map(|&first| honor_pon_meld(first))
            .collect();
        let reaction =
            CallReaction::pon(&[124, 125], PON_TARGET, &PON_CONSUMED).with_own_melds(melds);
        assert_eq!(
            reaction
                .context()
                .own_fixed_meld_count()
                .map(FixedMeldCount::get),
            Some(4)
        );

        let candidate = assert_single_call_candidate(
            &reaction,
            &LegalAction::None,
            CallDecisionReason::FixedMeldCountOverflow,
        );
        assert_eq!(
            candidate.current_fixed_meld_count.map(FixedMeldCount::get),
            Some(4)
        );
        assert_eq!(candidate.post_call_fixed_meld_count, None);
    }

    #[test]
    fn does_not_call_in_a_reaction_context_that_has_a_drawn_tile() {
        // reaction 局面に drawn_tile がある不整合な context では、14枚扱いで判断しない。
        let reaction = dragon_pon_reaction().with_drawn_tile(132);

        assert_call_is_declined(&reaction, CallDecisionReason::UnexpectedDrawnTile);
    }

    #[test]
    fn does_not_pon_with_inconsistent_consumed_tiles() {
        for consumed in [
            // 手牌に無い物理牌
            vec![124, 127],
            // 同じ物理牌の重複
            vec![124, 124],
            // 枚数不足
            vec![124],
            // 枚数過多
            vec![124, 125, 126],
            // 刻子にならない牌種
            vec![124, 120],
        ] {
            let reaction = dragon_pon_reaction().with_consumed(&consumed);
            let candidate = assert_single_call_candidate(
                &reaction,
                &LegalAction::None,
                CallDecisionReason::InvalidConsumed,
            );
            assert_eq!(candidate.current_shanten, None, "{consumed:?}");
        }
    }

    #[test]
    fn does_not_chi_with_inconsistent_consumed_tiles() {
        for consumed in [
            // 手牌に無い物理牌
            vec![80, 87],
            // 順子にならない牌種
            vec![80, 81],
            // 枚数不足
            vec![80],
        ] {
            let reaction = honitsu_chi_reaction().with_consumed(&consumed);
            let candidate = assert_single_call_candidate(
                &reaction,
                &LegalAction::None,
                CallDecisionReason::InvalidConsumed,
            );
            assert_eq!(candidate.current_shanten, None, "{consumed:?}");
        }
    }

    #[test]
    fn prefers_hora_over_an_eligible_call() {
        let mut agent = ShantenAgent;
        let reaction = dragon_pon_reaction();
        let ctx = reaction.context();

        for actions in [
            vec![LegalAction::Hora, reaction.call(), LegalAction::None],
            vec![reaction.call(), LegalAction::Hora, LegalAction::None],
        ] {
            assert_eq!(agent.act(&ctx, &actions), LegalAction::Hora);
            let diagnostic = ShantenAgent::diagnose(&ctx, &actions);
            assert_eq!(diagnostic.selected_source, AgentActionSource::Hora);
            // Hora で早期終了するので鳴きは検討していない。
            assert_eq!(diagnostic.call, None);
        }
    }

    #[test]
    fn prefers_ryukyoku_over_an_eligible_call() {
        let mut agent = ShantenAgent;
        let reaction = dragon_pon_reaction();
        let ctx = reaction.context();
        let actions = vec![reaction.call(), LegalAction::Ryukyoku, LegalAction::None];

        assert_eq!(agent.act(&ctx, &actions), LegalAction::Ryukyoku);
        let diagnostic = ShantenAgent::diagnose(&ctx, &actions);
        assert_eq!(diagnostic.selected_source, AgentActionSource::Ryukyoku);
        assert_eq!(diagnostic.call, None);
    }

    #[test]
    fn does_not_claim_kans_in_an_eligible_pon_context() {
        // Pon が成立する局面でも、Daiminkan / Ankan / Kakan は今回の対象外なので選ばない。
        let mut agent = ShantenAgent;
        let ctx = dragon_pon_reaction().context();
        let actions: Vec<LegalAction> = chi_and_kan_actions()
            .into_iter()
            .filter(|action| !matches!(action, LegalAction::Chi { .. }))
            .chain([LegalAction::None])
            .collect();

        assert_eq!(agent.act(&ctx, &actions), LegalAction::None);
        assert_eq!(ShantenAgent::diagnose(&ctx, &actions).call, None);
    }

    #[test]
    fn call_diagnostic_is_absent_without_a_legal_chi_or_pon() {
        let ctx = dragon_pon_reaction().context();
        let actions = vec![LegalAction::None];
        assert_eq!(ShantenAgent::diagnose(&ctx, &actions).call, None);
    }

    #[test]
    fn each_candidate_is_evaluated_independently() {
        // 合法な鳴きが複数ある場合は、成立しない候補も理由付きで残したうえで成立候補を採用する。
        let reaction = dragon_pon_reaction();
        let ctx = reaction.context();
        let declined = LegalAction::Pon {
            tile: tile(PON_TARGET),
            consumed: vec![tile(124), tile(120)],
        };

        for actions in [
            vec![declined.clone(), reaction.call(), LegalAction::None],
            vec![reaction.call(), declined.clone(), LegalAction::None],
        ] {
            let diagnostic = diagnose_matching_act(&ctx, &actions);
            assert_eq!(diagnostic.selected_action, reaction.call());

            let call = diagnostic.call.as_ref().unwrap();
            assert_eq!(call.reason, CallDecisionReason::EligibleTenpai);
            assert_eq!(call.candidates.len(), 2);
            assert_eq!(call.selected.as_ref(), Some(&reaction.call()));

            let declined_candidate = call
                .candidates
                .iter()
                .find(|candidate| candidate.action == declined)
                .unwrap();
            assert_eq!(
                declined_candidate.reason,
                CallDecisionReason::InvalidConsumed
            );
            assert!(!declined_candidate.selected);
            assert_eq!(
                call.candidates
                    .iter()
                    .filter(|candidate| candidate.selected)
                    .count(),
                1
            );
        }
    }

    #[test]
    fn several_eligible_calls_are_ranked_by_the_shared_discard_evaluation() {
        // どちらの Chi も成立するが、鳴き後の受け入れが広い下側の Chi を選ぶ。列挙順は結果を
        // 変えない。
        let lower = honitsu_chi_reaction();
        let upper = CallReaction::chi(&CHI_HAND, CHI_TARGET, &CHI_CONSUMED_UPPER);
        let ctx = lower.context();

        for actions in [
            vec![lower.call(), upper.call(), LegalAction::None],
            vec![upper.call(), lower.call(), LegalAction::None],
        ] {
            let diagnostic = diagnose_matching_act(&ctx, &actions);
            assert_eq!(diagnostic.selected_action, lower.call());
            assert_eq!(diagnostic.selected_source, AgentActionSource::Call);

            let call = diagnostic.call.as_ref().unwrap();
            assert_eq!(call.candidates.len(), 2);
            assert!(call.candidates.iter().all(|candidate| candidate.eligible));

            let remaining: Vec<Option<u8>> = call
                .candidates
                .iter()
                .map(CallCandidateDiagnostic::live_wait_remaining)
                .collect();
            assert_eq!(
                remaining.iter().filter(|value| **value == Some(7)).count(),
                1
            );
            assert_eq!(
                remaining.iter().filter(|value| **value == Some(6)).count(),
                1
            );
        }
    }

    #[test]
    fn equally_ranked_eligible_calls_keep_the_action_order() {
        // 完全に同値な候補では合法 action の列挙順を安定した tie-break として維持する。
        let reaction = CallReaction::pon(
            &[0, 4, 8, 12, 17, 20, 53, 54, 96, 100, 120, 124, 125],
            126,
            &PON_CONSUMED,
        );
        let ctx = reaction.context();
        let first = reaction.call();
        let second = LegalAction::Pon {
            tile: tile(126),
            consumed: vec![tile(125), tile(124)],
        };
        let actions = vec![first.clone(), second, LegalAction::None];

        let diagnostic = diagnose_matching_act(&ctx, &actions);
        assert_eq!(diagnostic.selected_action, first);

        let call = diagnostic.call.as_ref().unwrap();
        assert!(call.candidates.iter().all(|candidate| candidate.eligible));
        assert!(call.candidates[0].selected);
        assert!(!call.candidates[1].selected);
    }

    // ---- 喰い替え (鳴き後の合法打牌) テスト ----

    // 喰い替え禁止を適用しない場合の鳴き後 best discard。制約が効いていることを示す前提として、
    // 本番と同じ helper へ空の禁止牌を渡して求める。
    fn unconstrained_post_call_discard(reaction: &CallReaction) -> DiscardEvaluation {
        crate::discard_selection::select_best_one_step_discard_evaluation_with_fixed_meld_count(
            &reaction.context(),
            &reaction.post_call_tiles(),
            FixedMeldCount::new(1).unwrap(),
            &[],
        )
        .expect("制約なしの鳴き後 best discard")
    }

    // 喰い替え禁止牌が制約なしの best discard であること (前提) と、本番がそれを選ばないことを
    // 確認する。戻り値は本番が実際に選んだ打牌の牌種。
    fn assert_forbidden_discard_is_not_selected(
        reaction: &CallReaction,
        expected_forbidden: &[&str],
        unconstrained_best: &str,
        expected_reason: CallDecisionReason,
    ) -> TileType {
        assert!(expected_forbidden.contains(&unconstrained_best));

        let candidate = assert_single_call_candidate(reaction, &LegalAction::None, expected_reason);

        let forbidden: Vec<String> = candidate
            .post_call_forbidden_discards
            .as_ref()
            .unwrap()
            .iter()
            .map(|tile| tile.to_mjai_string())
            .collect();
        assert_eq!(forbidden, expected_forbidden);

        // 制約が無ければ禁止牌が選ばれる局面であることを固定する。
        let unconstrained = unconstrained_post_call_discard(reaction);
        assert_eq!(unconstrained.discard.to_mjai_string(), unconstrained_best);

        let selected = candidate.post_call_discard.as_ref().unwrap().discard;
        assert!(
            !expected_forbidden.contains(&selected.to_mjai_string().as_str()),
            "{selected:?}"
        );
        selected
    }

    #[test]
    fn a_pon_does_not_discard_the_called_tile_type() {
        // 123456m 55p 1s 9s PPP。Pon 後に残る P は、制約が無ければ best discard になるが、
        // 鳴いた牌と同じ牌種なので切れない。
        let reaction = CallReaction::pon(
            &[0, 4, 8, 12, 17, 20, 53, 54, 72, 104, 124, 125, 126],
            127,
            &PON_CONSUMED,
        );

        let selected = assert_forbidden_discard_is_not_selected(
            &reaction,
            &["P"],
            "P",
            CallDecisionReason::ReactionSourceUnknown,
        );
        assert_eq!(selected.to_mjai_string(), "1s");
    }

    #[test]
    fn a_chi_does_not_discard_the_called_tile_type() {
        // 1m 2m 3m 5m 8m 456p 789p 55s。2m3m で 1m を Chi した後、手牌に残る 1m は切れない。
        let reaction = CallReaction::chi(
            &[0, 4, 8, 17, 28, 48, 53, 56, 60, 64, 68, 89, 90],
            1,
            &[4, 8],
        );

        let selected = assert_forbidden_discard_is_not_selected(
            &reaction,
            &["1m", "4m"],
            "1m",
            CallDecisionReason::ReactionSourceUnknown,
        );
        assert_eq!(selected.to_mjai_string(), "8m");
    }

    #[test]
    fn a_chi_does_not_discard_the_forbidden_flank_tile() {
        // 123m 456m 55p 2s3s4s 2p 8p。3s4s で 5s を Chi すると 345s になり、2s を切ると
        // 元から持っていた 234s への喰い替えになる。
        let reaction = CallReaction::chi(
            &[0, 4, 8, 12, 17, 20, 53, 54, 76, 80, 84, 40, 64],
            89,
            &[80, 84],
        );

        let selected = assert_forbidden_discard_is_not_selected(
            &reaction,
            &["5s", "2s"],
            "2s",
            CallDecisionReason::ReactionSourceUnknown,
        );
        assert_eq!(selected.to_mjai_string(), "2p");
    }

    #[test]
    fn a_forbidden_discard_never_makes_the_call_eligible() {
        // 喰い替え禁止牌が制約なしの best discard になる局面では、残った合法候補でテンパイに
        // ならなければ鳴かない。禁止牌を切ればテンパイする、を理由に鳴くことはない。
        for (label, reaction) in [
            (
                "pon",
                CallReaction::pon(
                    &[0, 4, 8, 12, 17, 20, 53, 54, 72, 104, 124, 125, 126],
                    127,
                    &PON_CONSUMED,
                ),
            ),
            (
                "chi",
                CallReaction::chi(
                    &[0, 4, 8, 12, 17, 20, 53, 54, 76, 80, 84, 40, 64],
                    89,
                    &[80, 84],
                ),
            ),
        ] {
            let ctx = reaction.context();
            let actions = reaction.actions();
            let diagnostic = diagnose_matching_act(&ctx, &actions);

            assert_eq!(diagnostic.selected_action, LegalAction::None, "{label}");
            let call = diagnostic.call.as_ref().unwrap();
            assert_eq!(call.selected, None, "{label}");
            assert_eq!(
                call.reason,
                CallDecisionReason::ReactionSourceUnknown,
                "{label}"
            );

            // 制約なしの best discard もテンパイにならないため、禁止で結論が変わったのではなく
            // 合法な候補が元からテンパイに届いていない。
            let candidate = &call.candidates[0];
            assert_eq!(candidate.post_call_shanten(), Some(1), "{label}");
            assert_eq!(
                unconstrained_post_call_discard(&reaction).min_shanten_after_discard(),
                1,
                "{label}"
            );
        }
    }

    #[test]
    fn a_forbidden_discard_does_not_block_a_call_that_reaches_tenpai_otherwise() {
        // 既存の混一色 Chi。3s4s で 5s を鳴くので 5s と 2s は切れないが、2s を残したまま
        // 1m を切ってテンパイになるため鳴く。
        let reaction = honitsu_chi_reaction();
        let candidate = assert_single_call_candidate(
            &reaction,
            &reaction.call(),
            CallDecisionReason::EligibleTenpai,
        );

        let forbidden: Vec<String> = candidate
            .post_call_forbidden_discards
            .as_ref()
            .unwrap()
            .iter()
            .map(|tile| tile.to_mjai_string())
            .collect();
        assert_eq!(forbidden, ["5s", "2s"]);

        // 禁止牌 2s は鳴き後の手牌に残っているが、選ばれる打牌は 1m でテンパイになる。
        assert!(
            reaction
                .post_call_tiles()
                .iter()
                .any(|tile| tile.tile_type().to_mjai_string() == "2s")
        );
        assert_eq!(
            candidate
                .post_call_discard
                .as_ref()
                .unwrap()
                .discard
                .to_mjai_string(),
            "1m"
        );
        assert_eq!(candidate.post_call_shanten(), Some(0));
        // 喰い替え禁止は打牌の制約であって、待ちには影響しない。5s は引き続き待ち。
        assert_eq!(candidate.live_wait_remaining(), Some(7));
    }

    #[test]
    fn a_forbidden_five_excludes_both_the_red_and_the_black_five() {
        // 123m 456m 789p 9p + 赤5s。制約が無ければ 5s を切ってテンパイだが、5s を Pon した
        // 直後は赤5s も黒5s も切れない。
        let reaction = CallReaction::pon(
            &[0, 4, 8, 12, 17, 20, 60, 64, 68, 69, 88, 89, 90],
            91,
            &[89, 90],
        );
        let ctx = reaction.context();
        let post_call_tiles = reaction.post_call_tiles();
        let fixed_meld_count = FixedMeldCount::new(1).unwrap();

        let forbidden = crate::kuikae::forbidden_discards_after_call(&crate::meld::Meld::new(
            crate::meld::MeldKind::Pon,
            vec![tile(91), tile(89), tile(90)],
            Some(tile(91)),
        ));
        assert_eq!(
            forbidden
                .iter()
                .map(|tile| tile.to_mjai_string())
                .collect::<Vec<_>>(),
            ["5s"]
        );

        // 制約が無ければ、手牌に残った赤5s を切ってテンパイする。
        let unconstrained =
            crate::discard_selection::select_best_one_step_discard_evaluation_with_fixed_meld_count(
                &ctx,
                &post_call_tiles,
                fixed_meld_count,
                &[],
            )
            .unwrap();
        assert_eq!(unconstrained.discard.to_mjai_string(), "5s");
        assert!(unconstrained.discards_red_five);
        assert_eq!(unconstrained.min_shanten_after_discard(), 0);

        // 黒5s を鳴いても、牌種単位の禁止なので赤5s も候補から外れる。
        let constrained =
            crate::discard_selection::select_best_one_step_discard_evaluation_with_fixed_meld_count(
                &ctx,
                &post_call_tiles,
                fixed_meld_count,
                &forbidden,
            )
            .unwrap();
        assert_ne!(constrained.discard.to_mjai_string(), "5s");
    }

    #[test]
    fn agent_action_source_labels_are_stable() {
        assert_eq!(AgentActionSource::Hora.label(), "Hora");
        assert_eq!(AgentActionSource::Ryukyoku.label(), "Ryukyoku");
        assert_eq!(AgentActionSource::Call.label(), "Call");
        assert_eq!(AgentActionSource::Reach.label(), "Reach");
        assert_eq!(AgentActionSource::NormalDiscard.label(), "NormalDiscard");
        assert_eq!(
            AgentActionSource::DefenseFallback(DefenseFallbackKind::Genbutsu).label(),
            "DefenseFallback"
        );
        assert_eq!(
            AgentActionSource::OpenHandDefenseFallback(
                OpenHandDefenseCategory::SafeAgainstAllTargets
            )
            .label(),
            "OpenHandDefenseFallback"
        );
        assert_eq!(
            AgentActionSource::LegalDahaiFallback.label(),
            "LegalDahaiFallback"
        );
        assert_eq!(AgentActionSource::None.label(), "None");
    }

    #[test]
    fn agent_action_source_separates_the_two_defense_paths() {
        let riichi = AgentActionSource::DefenseFallback(DefenseFallbackKind::Genbutsu);
        let open_hand = AgentActionSource::OpenHandDefenseFallback(
            OpenHandDefenseCategory::SafeAgainstAllTargets,
        );

        assert_eq!(riichi.defense_kind(), Some(DefenseFallbackKind::Genbutsu));
        assert_eq!(riichi.open_hand_defense_category(), None);
        assert_eq!(open_hand.defense_kind(), None);
        assert_eq!(
            open_hand.open_hand_defense_category(),
            Some(OpenHandDefenseCategory::SafeAgainstAllTargets)
        );
    }

    // ---- High OpenHandThreat に対する action 選択 ----

    // 弱い一向聴 (受け入れ 7 枚 / 2 種類) になる自分の手牌。123m 456m 789m 1p 3p 5p 7p + ツモ 北。
    const OPEN_HAND_FOLD_HAND: [u8; 13] = [0, 4, 8, 12, 17, 20, 24, 28, 32, 36, 44, 53, 60];
    const OPEN_HAND_FOLD_DRAWN: u8 = 120;
    // 受け入れ牌をほぼ見え牌にして、弱い一向聴に固定するための見え牌。
    const OPEN_HAND_FOLD_DEAD: [u8; 20] = [
        37, 38, 39, 40, 41, 42, 43, 45, 46, 47, 52, 54, 55, 56, 57, 58, 59, 61, 62, 63,
    ];

    // 強い一向聴 (受け入れ 8 枚 / 2 種類) になる自分の手牌。
    const OPEN_HAND_IISHANTEN_HAND: [u8; 13] = [0, 4, 8, 12, 16, 20, 28, 29, 36, 40, 48, 52, 60];
    // テンパイになる自分の手牌。
    const OPEN_HAND_TENPAI_HAND: [u8; 13] = [0, 4, 8, 12, 16, 20, 28, 29, 36, 40, 44, 56, 60];

    // ドラも役牌も含まない Chi。open meld 数を作るためだけに使う。
    fn plain_chi() -> crate::meld::Meld {
        crate::meld::Meld::new(
            crate::meld::MeldKind::Chi,
            vec![tile(72), tile(76), tile(80)],
            Some(tile(72)),
        )
    }

    // 自分は player 0、親は player 2。`melded` の席が3副露で High OpenHandThreat になる。
    fn open_hand_context(
        hand_values: &[u8],
        drawn: Option<u8>,
        melded: usize,
        discards: [&[u8]; 4],
        reached: [bool; 4],
        extra_visible: &[u8],
    ) -> GameContext {
        open_hand_context_with_meld_count(
            hand_values,
            drawn,
            melded,
            3,
            discards,
            reached,
            extra_visible,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn open_hand_context_with_meld_count(
        hand_values: &[u8],
        drawn: Option<u8>,
        melded: usize,
        open_meld_count: usize,
        discards: [&[u8]; 4],
        reached: [bool; 4],
        extra_visible: &[u8],
    ) -> GameContext {
        let mut melds: [Vec<crate::meld::Meld>; 4] = Default::default();
        melds[melded] = (0..open_meld_count).map(|_| plain_chi()).collect();

        let mut visible: Vec<TileId> = hand_values.iter().map(|&value| tile(value)).collect();
        visible.extend(drawn.map(tile));
        visible.extend(extra_visible.iter().map(|&value| tile(value)));
        for discard in discards {
            visible.extend(discard.iter().map(|&value| tile(value)));
        }

        GameContext::from_parts_with_melds(
            drawn.map(tile),
            hand_values.iter().map(|&value| tile(value)).collect(),
            vec![],
            None,
            None,
            visible,
            Some(0),
            Some(2),
            std::array::from_fn(|player| {
                discards[player].iter().map(|&value| tile(value)).collect()
            }),
            reached,
            melds,
        )
    }

    // 弱い一向聴 + player 1 が High の副露相手。
    fn open_hand_fold_context(opponent_discards: &[u8], extra_visible: &[u8]) -> GameContext {
        let mut visible = OPEN_HAND_FOLD_DEAD.to_vec();
        visible.extend_from_slice(extra_visible);

        open_hand_context(
            &OPEN_HAND_FOLD_HAND,
            Some(OPEN_HAND_FOLD_DRAWN),
            1,
            [&[], opponent_discards, &[], &[]],
            [false; 4],
            &visible,
        )
    }

    fn open_hand_fold_actions() -> Vec<LegalAction> {
        OPEN_HAND_FOLD_HAND
            .iter()
            .map(|&value| dahai(value))
            .chain([dahai(OPEN_HAND_FOLD_DRAWN)])
            .collect()
    }

    #[test]
    fn fold_against_a_high_open_hand_prefers_a_tile_in_every_targets_river() {
        // player 1 の河に 9m があるので、通常打牌より本人の河の安全牌を優先する。
        let ctx = open_hand_fold_context(&[33], &[]);
        let actions = open_hand_fold_actions();
        let diagnostic = diagnose_matching_act(&ctx, &actions);

        let decision = diagnostic
            .push_pull_decision
            .expect("押し引きを判定している");
        assert_eq!(decision.mode, PushPullMode::Fold);
        assert_eq!(
            decision.reason,
            PushPullReason::IishantenAgainstHighOpenHand
        );

        assert_eq!(diagnostic.selected_action, dahai(32));
        assert_eq!(
            diagnostic.selected_source,
            AgentActionSource::OpenHandDefenseFallback(
                OpenHandDefenseCategory::SafeAgainstAllTargets
            )
        );
        assert_ne!(
            diagnostic.selected_action,
            diagnostic.normal_discard_action.clone().unwrap()
        );

        // リーチ者向けの防御 fallback は検討自体が起きない。
        assert!(diagnostic.defense.is_none());
        assert_eq!(diagnostic.defense_fallback_kind(), None);

        // 診断は production selector の結果をそのまま写す。
        let selection = diagnostic
            .open_hand_defense
            .selected
            .as_ref()
            .expect("OpenHand 防御 fallback を採用している");
        assert_eq!(selection.selected_action, dahai(32));
        assert_eq!(
            selection.selected_category,
            OpenHandDefenseCategory::SafeAgainstAllTargets
        );
        assert_eq!(
            diagnostic
                .open_hand_defense
                .candidates
                .iter()
                .filter(|candidate| candidate.selected)
                .map(|candidate| candidate.action.clone())
                .collect::<Vec<LegalAction>>(),
            vec![dahai(32)]
        );
    }

    #[test]
    fn fold_against_a_high_open_hand_uses_the_honor_safety_without_a_river_safe_tile() {
        // 本人の河に通る牌が無ければ字牌 safety。手牌の字牌は北だけ。
        let ctx = open_hand_fold_context(&[], &[]);
        let actions = open_hand_fold_actions();
        let diagnostic = diagnose_matching_act(&ctx, &actions);

        assert_eq!(
            diagnostic.push_pull_decision.map(|decision| decision.mode),
            Some(PushPullMode::Fold)
        );
        assert_eq!(diagnostic.selected_action, dahai(OPEN_HAND_FOLD_DRAWN));
        assert_eq!(
            diagnostic.selected_source,
            AgentActionSource::OpenHandDefenseFallback(OpenHandDefenseCategory::HonorSafety(
                HonorSafetyRank::OneVisible
            ))
        );
    }

    #[test]
    fn fold_against_a_high_open_hand_uses_the_suited_safety_without_honors() {
        // 8m が4枚見えているので 9m は NoChance。無スジの 1m より優先する。
        let ctx = open_hand_fold_context(&[], &[29, 30, 31]);
        let actions = vec![dahai(0), dahai(32)];
        let diagnostic = diagnose_matching_act(&ctx, &actions);

        assert_eq!(
            diagnostic.push_pull_decision.map(|decision| decision.mode),
            Some(PushPullMode::Fold)
        );
        assert_eq!(diagnostic.selected_action, dahai(32));
        assert_eq!(
            diagnostic.selected_source,
            AgentActionSource::OpenHandDefenseFallback(OpenHandDefenseCategory::SuitedSafety(
                SuitedSafetyRank::NoChance
            ))
        );
    }

    #[test]
    fn fold_against_a_high_open_hand_falls_back_to_the_normal_discard() {
        // 安全牌候補が1件も無い場合だけ通常打牌に戻る。
        let ctx = open_hand_fold_context(&[], &[]);
        let actions = vec![dahai(0), dahai(4)];
        let diagnostic = diagnose_matching_act(&ctx, &actions);

        assert_eq!(
            diagnostic.push_pull_decision.map(|decision| decision.mode),
            Some(PushPullMode::Fold)
        );
        assert_eq!(diagnostic.selected_source, AgentActionSource::NormalDiscard);
        assert_eq!(
            diagnostic.selected_action,
            diagnostic.normal_discard_action.clone().unwrap()
        );
        assert_eq!(diagnostic.open_hand_defense.selected, None);
        assert!(
            diagnostic
                .open_hand_defense
                .candidates
                .iter()
                .all(|candidate| !candidate.selected)
        );
    }

    #[test]
    fn fold_against_a_high_open_hand_with_a_strong_iishanten_prefers_the_defense_fallback() {
        // 強い一向聴でも High の副露相手には降りる。player 1 の河に 1m があるので、通常打牌より
        // 本人の河の安全牌を優先する。
        let ctx = open_hand_context(
            &OPEN_HAND_IISHANTEN_HAND,
            Some(OPEN_HAND_FOLD_DRAWN),
            1,
            [&[], &[1], &[], &[]],
            [false; 4],
            &[],
        );
        let actions: Vec<LegalAction> = OPEN_HAND_IISHANTEN_HAND
            .iter()
            .map(|&value| dahai(value))
            .chain([dahai(OPEN_HAND_FOLD_DRAWN)])
            .collect();
        let diagnostic = diagnose_matching_act(&ctx, &actions);

        let decision = diagnostic
            .push_pull_decision
            .expect("押し引きを判定している");
        let offense = diagnostic
            .push_pull_inputs
            .expect("押し引き入力がある")
            .offense
            .expect("offense がある");
        assert_eq!(offense.min_shanten_after_discard, 1);
        assert!(offense.acceptance_total_remaining >= 8);
        assert!(offense.acceptance_type_count >= 2);

        assert_eq!(decision.mode, PushPullMode::Fold);
        assert_eq!(
            decision.reason,
            PushPullReason::IishantenAgainstHighOpenHand
        );

        assert!(diagnostic.open_hand_defense.has_target());
        assert_eq!(diagnostic.selected_action, dahai(0));
        assert_eq!(
            diagnostic.selected_source,
            AgentActionSource::OpenHandDefenseFallback(
                OpenHandDefenseCategory::SafeAgainstAllTargets
            )
        );
        assert_ne!(
            diagnostic.selected_action,
            diagnostic.normal_discard_action.clone().unwrap()
        );
    }

    #[test]
    fn push_against_a_high_open_hand_keeps_the_reach_priority() {
        // テンパイは Push。Reach → 通常打牌 の既存順序を変えない。
        let ctx = open_hand_context(
            &OPEN_HAND_TENPAI_HAND,
            Some(OPEN_HAND_FOLD_DRAWN),
            1,
            [&[], &[33], &[], &[]],
            [false; 4],
            &[],
        );
        let normal_actions: Vec<LegalAction> = OPEN_HAND_TENPAI_HAND
            .iter()
            .map(|&value| dahai(value))
            .chain([dahai(OPEN_HAND_FOLD_DRAWN)])
            .collect();
        let mut reach_actions = vec![LegalAction::Reach];
        reach_actions.extend(normal_actions.clone());

        let with_reach = diagnose_matching_act(&ctx, &reach_actions);
        let decision = with_reach
            .push_pull_decision
            .expect("押し引きを判定している");
        assert_eq!(decision.mode, PushPullMode::Push);
        assert_eq!(
            decision.reason,
            PushPullReason::StrongTenpaiAgainstHighOpenHand
        );
        assert_eq!(with_reach.selected_action, LegalAction::Reach);
        assert_eq!(with_reach.selected_source, AgentActionSource::Reach);
        assert_eq!(with_reach.open_hand_defense.selected, None);

        let without_reach = diagnose_matching_act(&ctx, &normal_actions);
        assert_eq!(
            without_reach.selected_source,
            AgentActionSource::NormalDiscard
        );
        assert_eq!(
            without_reach.selected_action,
            without_reach.normal_discard_action.clone().unwrap()
        );
    }

    fn request_407_context_and_actions() -> (GameContext, Vec<LegalAction>) {
        // Capture request_id=407 相当。player 3 の 14 枚は
        // 23566m 222p 123s 0s67s。player 1 は2副露かつ河9枚で High になり、
        // 通常打牌 selector が選ぶ 5m はその河にある。
        let hand = [4, 8, 17, 20, 21, 40, 41, 72, 76, 80, 88, 92, 96];
        let drawn = 42;
        let opponent_discards = [16, 0, 1, 12, 24, 28, 32, 36, 44];
        let opponent_melds = vec![
            crate::meld::Meld::new(
                crate::meld::MeldKind::Chi,
                vec![tile(48), tile(52), tile(56)],
                Some(tile(48)),
            ),
            crate::meld::Meld::new(
                crate::meld::MeldKind::Chi,
                vec![tile(97), tile(100), tile(104)],
                Some(tile(97)),
            ),
        ];
        let mut melds: [Vec<crate::meld::Meld>; 4] = Default::default();
        melds[1] = opponent_melds.clone();
        let discards: [Vec<TileId>; 4] = [
            vec![],
            opponent_discards.iter().map(|&value| tile(value)).collect(),
            vec![],
            vec![],
        ];
        let dora_indicator = tile(64);
        let mut visible: Vec<TileId> = hand.iter().map(|&value| tile(value)).collect();
        visible.push(tile(drawn));
        visible.push(dora_indicator);
        visible.extend(discards.iter().flatten().copied());
        visible.extend(opponent_melds.iter().flat_map(|meld| meld.tiles().to_vec()));
        let ctx = GameContext::from_parts_with_melds(
            Some(tile(drawn)),
            hand.iter().map(|&value| tile(value)).collect(),
            vec![dora_indicator],
            TileType::new(27),
            TileType::new(28),
            visible,
            Some(3),
            Some(2),
            discards,
            [false; 4],
            melds,
        )
        .with_history_furiten_facts(bot_logic::HistoryFuritenFacts {
            same_turn: Some(false),
            riichi_missed_win: Some(false),
        });
        let actions: Vec<LegalAction> = hand
            .iter()
            .chain([&drawn])
            .map(|&value| dahai(value))
            .chain([LegalAction::Reach])
            .collect();

        (ctx, actions)
    }

    #[test]
    fn request_407_safe_tenpai_discard_pushes_against_a_high_open_hand() {
        use crate::offense_value::TenpaiOffenseMode;
        use crate::open_hand_threat::{OpenHandThreatLevel, OpenHandThreatReason};

        let (ctx, actions) = request_407_context_and_actions();

        let diagnostic = diagnose_matching_act(&ctx, &actions);
        assert_eq!(diagnostic.normal_discard_action, Some(dahai(17)));

        let selected = diagnostic
            .normal_discard
            .as_ref()
            .and_then(|normal| normal.selected.as_ref())
            .expect("通常打牌評価が選ばれている");
        assert_eq!(selected.discard, TileType::new(4).unwrap());
        assert_eq!(selected.min_shanten_after_discard(), 0);
        assert_eq!(selected.acceptance_total_remaining(), 5);
        assert_eq!(selected.acceptance_type_count(), 2);

        let inputs = diagnostic.push_pull_inputs.expect("押し引き入力がある");
        assert_eq!(inputs.player_threats[1].open_meld_count, 2);
        assert_eq!(inputs.player_threats[1].discard_count, 9);
        assert_eq!(
            inputs.open_hand_threats[1].level(),
            Some(OpenHandThreatLevel::High)
        );
        assert_eq!(
            inputs.open_hand_threats[1].reason(),
            Some(OpenHandThreatReason::TwoOrMoreOpenMeldsFromNineDiscards)
        );
        assert!(inputs.selected_normal_discard_hard_safe_for_all_high_open_hand_targets);

        let offense = inputs.offense.expect("offense がある");
        let wait = offense
            .tenpai_wait_after_discard
            .expect("選択打牌後の待ちがある");
        assert_eq!(wait.tsumo_remaining, 5);
        assert_eq!(wait.tsumo_type_count, 2);
        assert_eq!(wait.permanent_furiten, bot_logic::PermanentFuriten::No);
        assert_eq!(wait.can_ron, Some(true));
        let reach_wait = diagnostic
            .reach
            .as_ref()
            .and_then(|reach| reach.tenpai_wait.as_ref())
            .expect("リーチ判断にも選択済み待ちが共有される");
        assert_eq!(
            reach_wait.live_waits,
            vec![TileType::new(0).unwrap(), TileType::new(3).unwrap()]
        );
        let tenpai_value = offense
            .tenpai_offense_value_after_discard
            .expect("現在聴牌 offense value がある");
        assert_eq!(tenpai_value.mode, TenpaiOffenseMode::Reach);
        assert_eq!(tenpai_value.value.weighted_total(), Some(13_000));
        assert!(tenpai_value.value.weighted_total().unwrap() < 15_600);

        assert_eq!(
            diagnostic.push_pull_decision,
            Some(crate::push_pull::PushPullDecision {
                mode: PushPullMode::Push,
                reason: PushPullReason::SafeTenpaiAgainstHighOpenHand,
            })
        );
        assert_eq!(diagnostic.selected_action, LegalAction::Reach);
        assert_eq!(diagnostic.selected_source, AgentActionSource::Reach);
        assert_eq!(
            diagnostic
                .reach
                .as_ref()
                .and_then(|reach| reach.selected_discard.clone()),
            Some(dahai(17))
        );
        assert_eq!(diagnostic.open_hand_defense.selected, None);
    }

    #[test]
    fn public_push_pull_inputs_do_not_use_an_illegal_global_best_for_hard_safe() {
        let (ctx, actions) = request_407_context_and_actions();

        // public API は offense の既存 global-best semantics を維持するが、追加した hard-safe
        // fact には非合法な global best を使わない。5m を合法候補から外し、実際に選べる通常打牌を
        // hard-safe でない 2m だけに制限する。
        let restricted_actions = vec![dahai(4), LegalAction::Reach];
        let tiles: Vec<TileId> = ctx
            .hand_tiles()
            .iter()
            .copied()
            .chain(ctx.drawn_tile())
            .collect();
        let global_best = select_best_normal_discard_evaluation(&ctx, &tiles, &restricted_actions)
            .expect("全手牌候補の global best がある");
        assert_eq!(global_best.discard, TileType::new(4).unwrap());
        assert!(
            !restricted_actions.iter().any(
                |action| matches!(action, LegalAction::Dahai { tile } if tile.tile_type() == global_best.discard)
            )
        );

        let legal_selection = select_discard_action_with_evaluation(&ctx, &restricted_actions);
        assert_eq!(legal_selection.action, Some(dahai(4)));
        assert_eq!(
            legal_selection
                .evaluation
                .as_ref()
                .map(|evaluation| evaluation.discard),
            TileType::new(1)
        );
        let legal_inputs = push_pull_inputs_from_context_with_evaluation(
            &ctx,
            legal_selection.evaluation.as_ref(),
            &restricted_actions,
        );
        assert!(!legal_inputs.selected_normal_discard_hard_safe_for_all_high_open_hand_targets);

        let restricted_public = push_pull_inputs_from_context(&ctx, &restricted_actions);
        assert_eq!(
            restricted_public
                .offense
                .expect("global-best offense は既存どおり保持する")
                .min_shanten_after_discard,
            0
        );
        assert!(
            !restricted_public.selected_normal_discard_hard_safe_for_all_high_open_hand_targets
        );
        assert_eq!(
            decide_push_pull(&restricted_public),
            crate::push_pull::PushPullDecision {
                mode: PushPullMode::Fold,
                reason: PushPullReason::WeakTenpaiAgainstHighOpenHand,
            }
        );

        // 同じ public 入口でも global best の 5m が合法なら、all-target hard-safe fact は true。
        let unrestricted_public = push_pull_inputs_from_context(&ctx, &actions);
        assert!(
            unrestricted_public.selected_normal_discard_hard_safe_for_all_high_open_hand_targets
        );
        assert_eq!(
            decide_push_pull(&unrestricted_public).reason,
            PushPullReason::SafeTenpaiAgainstHighOpenHand
        );
    }

    #[test]
    fn weak_tenpai_push_against_a_late_one_meld_high_matches_every_entry_point() {
        let late_discards = [96, 97, 98, 99, 100, 101, 102, 103, 104, 105, 106, 107];
        let ctx = open_hand_context_with_meld_count(
            &OPPONENT_MELD_HAND,
            Some(OPPONENT_MELD_DRAW),
            1,
            1,
            [&[], &late_discards, &[], &[]],
            [false; 4],
            &[],
        );
        let actions = opponent_meld_actions();
        let mut agent = ShantenAgent;
        let acted = agent.act(&ctx, &actions);
        let diagnostic = ShantenAgent::diagnose(&ctx, &actions);
        let with_lookahead =
            ShantenAgent::diagnose_with_options(&ctx, &actions, DiagnosticOptions::WITH_LOOKAHEAD);

        let inputs = diagnostic.push_pull_inputs.expect("押し引き入力がある");
        let offense = inputs.offense.expect("offense がある");
        assert_eq!(inputs.player_threats[1].open_meld_count, 1);
        assert_eq!(inputs.player_threats[1].discard_count, 12);
        assert!(inputs.has_only_late_one_meld_high_open_hand_threats());
        assert_eq!(offense.min_shanten_after_discard, 0);
        assert!(
            offense
                .tenpai_wait_after_discard
                .is_some_and(|wait| wait.tsumo_remaining < 6)
        );
        assert_eq!(
            diagnostic.push_pull_decision,
            Some(crate::push_pull::PushPullDecision {
                mode: PushPullMode::Push,
                reason: PushPullReason::TenpaiAgainstLateOneMeldHighOpenHand,
            })
        );
        assert_eq!(diagnostic.selected_action, acted);
        assert_eq!(with_lookahead.selected_action, acted);
        assert_eq!(with_lookahead.selected_source, diagnostic.selected_source);
        assert_eq!(diagnostic.selected_source, AgentActionSource::NormalDiscard);
        assert_eq!(diagnostic.open_hand_defense.selected, None);
    }

    // ---- RiichiThreat + High OpenHandThreat の複合 threat に対する action 選択 ----

    // 弱い一向聴 + player 1 がリーチ + player 2 が High の副露相手。
    fn combined_threat_fold_context(
        riichi_discards: &[u8],
        open_hand_discards: &[u8],
    ) -> GameContext {
        open_hand_context(
            &OPEN_HAND_FOLD_HAND,
            Some(OPEN_HAND_FOLD_DRAWN),
            2,
            [&[], riichi_discards, open_hand_discards, &[]],
            [false, true, false, false],
            &OPEN_HAND_FOLD_DEAD,
        )
    }

    #[test]
    fn fold_against_a_combined_threat_prefers_a_tile_safe_against_all_threats() {
        // 9m はリーチ者の現物でも副露相手の河にもある。両方に通る牌を最優先する。
        let ctx = combined_threat_fold_context(&[33], &[33]);
        let actions = open_hand_fold_actions();
        let diagnostic = diagnose_matching_act(&ctx, &actions);

        let decision = diagnostic
            .push_pull_decision
            .expect("押し引きを判定している");
        assert_eq!(decision.mode, PushPullMode::Fold);
        assert_eq!(
            decision.reason,
            PushPullReason::IishantenAgainstCombinedThreat
        );

        assert_eq!(diagnostic.selected_action, dahai(32));
        assert_eq!(
            diagnostic.selected_source,
            AgentActionSource::CombinedThreatDefenseFallback(
                CombinedDefenseCategory::SafeAgainstAllThreats
            )
        );
        assert_ne!(
            diagnostic.selected_action,
            diagnostic.normal_discard_action.clone().unwrap()
        );

        // 既存のリーチ者向け / OpenHand 向け防御 fallback には切り替えない。
        assert!(diagnostic.defense.is_none());
        assert_eq!(diagnostic.defense_fallback_kind(), None);
        assert_eq!(diagnostic.open_hand_defense.selected, None);
        assert_eq!(diagnostic.open_hand_defense_category(), None);

        // 診断は production selector の結果をそのまま写す。
        let selection = diagnostic
            .combined_defense
            .selected
            .as_ref()
            .expect("複合 threat の防御 fallback を採用している");
        assert_eq!(selection.selected_action, dahai(32));
        assert_eq!(
            selection.selected_category,
            CombinedDefenseCategory::SafeAgainstAllThreats
        );
        assert_eq!(
            diagnostic
                .combined_defense
                .candidates
                .iter()
                .filter(|candidate| candidate.selected)
                .map(|candidate| candidate.action.clone())
                .collect::<Vec<LegalAction>>(),
            vec![dahai(32)]
        );
    }

    #[test]
    fn a_tile_safe_for_only_the_riichi_target_is_not_the_first_category() {
        // 9m はリーチ者の現物だが副露相手の河には無いので、字牌 safety が選ばれる。
        let ctx = combined_threat_fold_context(&[33], &[]);
        let actions = open_hand_fold_actions();
        let diagnostic = diagnose_matching_act(&ctx, &actions);

        assert_eq!(
            diagnostic.push_pull_decision.map(|decision| decision.mode),
            Some(PushPullMode::Fold)
        );
        assert_eq!(diagnostic.selected_action, dahai(OPEN_HAND_FOLD_DRAWN));
        assert_eq!(
            diagnostic.selected_source,
            AgentActionSource::CombinedThreatDefenseFallback(CombinedDefenseCategory::HonorSafety(
                HonorSafetyRank::OneVisible
            ))
        );
    }

    #[test]
    fn the_combined_defense_targets_both_threats() {
        let ctx = combined_threat_fold_context(&[33], &[33]);
        let actions = open_hand_fold_actions();
        let diagnostic = diagnose_matching_act(&ctx, &actions);

        assert_eq!(
            diagnostic.combined_defense.targets,
            vec![
                ThreatDefenseTarget::riichi(1),
                ThreatDefenseTarget::high_open_hand(2),
            ]
        );
        // OpenHand 診断の target は High の相手だけで、既存 semantics のまま。
        assert_eq!(diagnostic.open_hand_defense.targets, vec![2]);
    }

    #[test]
    fn fold_against_a_combined_threat_falls_back_to_the_normal_discard() {
        // 安全牌候補が1件も無い場合だけ通常打牌に戻る。
        let ctx = combined_threat_fold_context(&[], &[]);
        let actions = vec![dahai(0), dahai(4)];
        let diagnostic = diagnose_matching_act(&ctx, &actions);

        assert_eq!(
            diagnostic.push_pull_decision.map(|decision| decision.mode),
            Some(PushPullMode::Fold)
        );
        assert_eq!(diagnostic.selected_source, AgentActionSource::NormalDiscard);
        assert_eq!(
            diagnostic.selected_action,
            diagnostic.normal_discard_action.clone().unwrap()
        );
        assert_eq!(diagnostic.combined_defense.selected, None);
        assert!(
            diagnostic
                .combined_defense
                .candidates
                .iter()
                .all(|candidate| !candidate.selected)
        );
    }

    #[test]
    fn push_strong_tenpai_against_a_combined_threat_reaches() {
        // 複合 threat でも強いテンパイなら押す。Push の順序どおり Reach を最優先する。
        let ctx = open_hand_context(
            &OPEN_HAND_TENPAI_HAND,
            Some(OPEN_HAND_FOLD_DRAWN),
            2,
            [&[], &[33], &[33], &[]],
            [false, true, false, false],
            &[],
        );
        let mut actions = vec![LegalAction::Reach];
        actions.extend(
            OPEN_HAND_TENPAI_HAND
                .iter()
                .map(|&value| dahai(value))
                .chain([dahai(OPEN_HAND_FOLD_DRAWN)]),
        );
        let diagnostic = diagnose_matching_act(&ctx, &actions);

        let decision = diagnostic
            .push_pull_decision
            .expect("押し引きを判定している");
        assert_eq!(decision.mode, PushPullMode::Push);
        assert_eq!(
            decision.reason,
            PushPullReason::StrongTenpaiAgainstCombinedThreat
        );

        assert!(diagnostic.combined_defense.has_target());
        assert_eq!(diagnostic.selected_action, LegalAction::Reach);
        assert_eq!(diagnostic.selected_source, AgentActionSource::Reach);
        assert_eq!(diagnostic.combined_defense.selected, None);
    }

    #[test]
    fn the_combined_defense_selection_matches_act_and_every_diagnose_entry_point() {
        // act() / diagnose() / 追加診断つき diagnose() の選択結果は必ず一致する。
        let ctx = combined_threat_fold_context(&[33], &[33]);
        let actions = open_hand_fold_actions();
        let mut agent = ShantenAgent;
        let acted = agent.act(&ctx, &actions);
        let diagnostic = ShantenAgent::diagnose(&ctx, &actions);
        let with_lookahead =
            ShantenAgent::diagnose_with_options(&ctx, &actions, DiagnosticOptions::WITH_LOOKAHEAD);

        assert_eq!(diagnostic.selected_action, acted);
        assert_eq!(with_lookahead.selected_action, acted);
        assert_eq!(with_lookahead.selected_source, diagnostic.selected_source);
        assert_eq!(
            with_lookahead.combined_defense, diagnostic.combined_defense,
            "追加診断は選択結果を変えない"
        );

        // 診断は production selector を再実行せず、その結果を写す。
        let targets = combined_threat_defense_targets_from_context(&ctx);
        let selected =
            select_combined_threat_defense_fallback_action_with_kind(&ctx, &actions, &targets);
        let (action, category) = selected.expect("防御 fallback を選べる");
        assert_eq!(acted, *action);
        assert_eq!(diagnostic.combined_defense_category(), Some(category));
    }

    // ---- 九種九牌の宣言 / 続行 ----

    // 自摸後14枚の手牌から、全ての牌を切る合法 Dahai を作る。
    fn ryukyoku_dahai_actions(ctx: &GameContext) -> Vec<LegalAction> {
        ctx.hand_tiles()
            .iter()
            .copied()
            .chain(ctx.drawn_tile())
            .map(|tile| LegalAction::Dahai { tile })
            .collect()
    }

    fn ryukyoku_actions(ctx: &GameContext) -> Vec<LegalAction> {
        let mut actions = ryukyoku_dahai_actions(ctx);
        actions.push(LegalAction::Ryukyoku);
        actions
    }

    fn ryukyoku_diagnostic(
        ctx: &GameContext,
        actions: &[LegalAction],
    ) -> RyukyokuDecisionDiagnostic {
        ShantenAgent::diagnose(ctx, actions)
            .ryukyoku
            .expect("九種九牌を検討している")
    }

    #[test]
    fn prefers_hora_over_ryukyoku() {
        let mut agent = ShantenAgent;
        let ctx = context_from_hand(&KOKUSHI_FOUR_HAND);
        let mut actions = ryukyoku_actions(&ctx);
        actions.push(LegalAction::Hora);

        assert_eq!(agent.act(&ctx, &actions), LegalAction::Hora);

        let diagnostic = diagnose_matching_act(&ctx, &actions);
        assert_eq!(diagnostic.selected_source, AgentActionSource::Hora);
        // Hora で早期終了するので九種九牌は検討していない。
        assert_eq!(diagnostic.ryukyoku, None);
    }

    #[test]
    fn keeps_the_existing_selection_without_a_legal_ryukyoku() {
        let mut agent = ShantenAgent;
        let ctx = context_from_hand(&KOKUSHI_FOUR_HAND);
        let actions = ryukyoku_dahai_actions(&ctx);

        assert_eq!(
            agent.act(&ctx, &actions),
            select_discard_action(&ctx, &actions).expect("通常打牌を選べる")
        );

        let diagnostic = diagnose_matching_act(&ctx, &actions);
        assert_eq!(diagnostic.selected_source, AgentActionSource::NormalDiscard);
        assert_eq!(diagnostic.ryukyoku, None);
    }

    #[test]
    fn declares_ryukyoku_when_every_shanten_is_too_far() {
        let mut agent = ShantenAgent;
        for hand in [
            &KOKUSHI_FOUR_HAND,
            &STANDARD_THREE_HAND,
            &CHIITOITSU_THREE_HAND,
        ] {
            let ctx = context_from_hand(hand);
            let actions = ryukyoku_actions(&ctx);

            assert_eq!(agent.act(&ctx, &actions), LegalAction::Ryukyoku, "{hand:?}");

            let diagnostic = diagnose_matching_act(&ctx, &actions);
            assert_eq!(
                diagnostic.selected_source,
                AgentActionSource::Ryukyoku,
                "{hand:?}"
            );
            assert_eq!(
                ryukyoku_diagnostic(&ctx, &actions).verdict,
                RyukyokuVerdict::Declare,
                "{hand:?}"
            );
        }
    }

    #[test]
    fn continues_past_ryukyoku_when_any_shanten_is_close_enough() {
        let mut agent = ShantenAgent;
        for hand in [
            &STANDARD_TWO_HAND,
            &CHIITOITSU_TWO_HAND,
            &KOKUSHI_THREE_HAND,
        ] {
            let ctx = context_from_hand(hand);
            let actions = ryukyoku_actions(&ctx);
            let acted = agent.act(&ctx, &actions);

            assert_ne!(acted, LegalAction::Ryukyoku, "{hand:?}");
            assert_eq!(
                ryukyoku_diagnostic(&ctx, &actions).verdict,
                RyukyokuVerdict::Continue,
                "{hand:?}"
            );

            // 続行後の打牌選択は、Ryukyoku が合法手に無い場合とまったく同じ。
            let without_ryukyoku = ryukyoku_dahai_actions(&ctx);
            assert_eq!(acted, agent.act(&ctx, &without_ryukyoku), "{hand:?}");

            let diagnostic = diagnose_matching_act(&ctx, &actions);
            assert_eq!(
                diagnostic.selected_source,
                ShantenAgent::diagnose(&ctx, &without_ryukyoku).selected_source,
                "{hand:?}"
            );
        }
    }

    #[test]
    fn the_ryukyoku_diagnostic_reports_the_existing_shanten() {
        let ctx = context_from_hand(&KOKUSHI_THREE_HAND);
        let actions = ryukyoku_actions(&ctx);
        let ryukyoku = ryukyoku_diagnostic(&ctx, &actions);

        let counts =
            TileCounts::from_tiles(ctx.hand_tiles().iter().copied().chain(ctx.drawn_tile()));
        assert_eq!(ryukyoku.shanten, Some(calculate_shanten(&counts)));
        assert_eq!(ryukyoku.standard_shanten(), Some(standard_shanten(&counts)));
        assert_eq!(
            ryukyoku.chiitoitsu_shanten(),
            Some(chiitoitsu_shanten(&counts))
        );
        assert_eq!(ryukyoku.kokushi_shanten(), Some(kokushi_shanten(&counts)));
    }

    #[test]
    fn keeps_ryukyoku_when_the_current_hand_cannot_be_evaluated() {
        let mut agent = ShantenAgent;
        // 自摸牌が分からない context では自摸後手牌を復元できない。向聴数を推測して続行せず、
        // 従来どおり Ryukyoku を選ぶ。
        let evaluable = context_from_hand(&KOKUSHI_THREE_HAND);
        let ctx = GameContext::from_parts(None, evaluable.hand_tiles().to_vec());
        let actions = ryukyoku_actions(&evaluable);

        assert_eq!(agent.act(&ctx, &actions), LegalAction::Ryukyoku);

        let ryukyoku = ryukyoku_diagnostic(&ctx, &actions);
        assert_eq!(ryukyoku.verdict, RyukyokuVerdict::Declare);
        assert_eq!(ryukyoku.shanten, None);
        assert_eq!(ryukyoku.standard_shanten(), None);
        assert_eq!(ryukyoku.chiitoitsu_shanten(), None);
        assert_eq!(ryukyoku.kokushi_shanten(), None);
    }
}
