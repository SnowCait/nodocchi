use crate::action::{LegalAction, prefer_black_five_for_action};
use crate::agent::Agent;
use crate::context::GameContext;
use crate::defense::{
    DefenseDecisionDiagnostic, DefenseFallbackKind, log_defense_fallback_decision,
    select_defense_fallback_action_with_kind,
};
use crate::discard_selection::{
    DiscardActionSelection, select_discard_action_with_diagnostic,
    select_discard_action_with_evaluation,
};
use crate::push_pull::{
    PushPullDecision, PushPullInputs, PushPullMode, decide_push_pull, log_push_pull_decision,
    push_pull_inputs_from_context_with_evaluation,
};
use bot_logic::{DiscardDecisionDiagnostic, TileCounts, calculate_acceptance_with_visible_tiles};

const AGENT_DECISION_LOG_TARGET: &str = "bot_core::agent_decision";

// 補正後の待ち枚数がこの枚数以上ならリーチする。
const REACH_MIN_REMAINING: u8 = 4;

/// 最終 action がどの経路で選ばれたかを表す診断。プロトコル非依存。
///
/// `ShantenAgent::act()` が実際に通った経路そのものであり、診断用の別判断ロジックではない。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentActionSource {
    Hora,
    Ryukyoku,
    Reach,
    NormalDiscard,
    DefenseFallback(DefenseFallbackKind),
    LegalDahaiFallback,
    None,
}

impl AgentActionSource {
    // 防御 kind を分離して扱えるよう、source ラベルは kind を含めない固定名にする。
    fn label(&self) -> &'static str {
        match self {
            AgentActionSource::Hora => "Hora",
            AgentActionSource::Ryukyoku => "Ryukyoku",
            AgentActionSource::Reach => "Reach",
            AgentActionSource::NormalDiscard => "NormalDiscard",
            AgentActionSource::DefenseFallback(_) => "DefenseFallback",
            AgentActionSource::LegalDahaiFallback => "LegalDahaiFallback",
            AgentActionSource::None => "None",
        }
    }

    /// 防御 fallback 経路で選ばれた場合のその種別。他の経路では `None`。
    pub fn defense_kind(&self) -> Option<DefenseFallbackKind> {
        match self {
            AgentActionSource::DefenseFallback(kind) => Some(*kind),
            _ => None,
        }
    }
}

/// `ShantenAgent` が下した最終判断と、その選択経路・ログ用文脈をまとめた内部表現。
///
/// ログのためだけに判断ロジックを再実行しないよう、action 選択の過程で得た情報を保持する。
/// `push_pull` / `push_pull_inputs` / `normal_discard` は Hora / Ryukyoku の早期 return では `None`。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AgentDecision {
    action: LegalAction,
    source: AgentActionSource,
    push_pull_inputs: Option<PushPullInputs>,
    push_pull: Option<PushPullDecision>,
    normal_discard: Option<LegalAction>,
}

/// `ShantenAgent` の判断過程を外部の解析ツールから辿るための構造化診断。
///
/// 契約:
///
/// - `selected_action` / `selected_source` は `ShantenAgent::act()` と**同じ selection logic** の
///   結果である。診断専用の別判断ロジックは持たない。
///   常に `selected_action == ShantenAgent::act(context, legal_actions)` が成り立つ。
/// - 追加診断情報(候補ごとの形の内訳、全防御候補評価など)は解析用途であり、action 選択には
///   影響しない。
/// - 実際に実行されなかった判断は `None` で、推測して埋めない。Hora / Ryukyoku で早期終了した
///   場合は `normal_discard` / `normal_discard_action` / `push_pull_inputs` /
///   `push_pull_decision` / `defense` がすべて `None`。
///
/// tracing ログとは独立した pure なデータであり、ログをパースして構築することはない。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShantenDecisionDiagnostic {
    /// 最終的に選んだ action。`ShantenAgent::act()` の結果と一致する。
    pub selected_action: LegalAction,
    /// 最終 action をどの経路で選んだか。
    pub selected_source: AgentActionSource,
    /// 通常打牌評価が選んだ合法 Dahai。最終 action が別経路でも、比較用に保持する。
    pub normal_discard_action: Option<LegalAction>,
    /// 通常打牌評価を行った場合の全合法候補診断。合法 Dahai が無い場合は
    /// `selected == None` かつ `candidates` が空の診断になる。
    pub normal_discard: Option<DiscardDecisionDiagnostic>,
    /// 押し引き判定に使った入力。`push_pull_inputs_from_context_with_evaluation()` の実結果。
    pub push_pull_inputs: Option<PushPullInputs>,
    /// 押し引き判定の結果。`decide_push_pull()` の実結果。
    pub push_pull_decision: Option<PushPullDecision>,
    /// 防御 fallback を検討した場合の診断。採用されなかった場合も候補評価を保持する。
    pub defense: Option<DefenseDecisionDiagnostic>,
}

impl ShantenDecisionDiagnostic {
    /// 最終 action が防御 fallback 由来の場合のその種別。他の経路では `None`。
    pub fn defense_fallback_kind(&self) -> Option<DefenseFallbackKind> {
        self.selected_source.defense_kind()
    }
}

/// `ShantenAgent::act()` と同じ判断を行い、その過程を構造化診断として返す。
///
/// [`ShantenAgent::diagnose`] の別名。契約は [`ShantenDecisionDiagnostic`] を参照。
pub fn diagnose_shanten_decision(
    context: &GameContext,
    legal_actions: &[LegalAction],
) -> ShantenDecisionDiagnostic {
    ShantenAgent::diagnose(context, legal_actions)
}

// 解析専用の追加診断を集める内部収集器。
//
// `enabled == false` の通常 act() 経路では、候補ごとの形の内訳や全防御候補評価といった
// action 選択に不要な情報を一切構築しない。selection logic 自体は enabled にかかわらず共通。
#[derive(Debug, Default)]
struct DecisionDiagnostics {
    enabled: bool,
    normal_discard: Option<DiscardDecisionDiagnostic>,
    defense: Option<DefenseDecisionDiagnostic>,
}

impl DecisionDiagnostics {
    fn disabled() -> Self {
        Self::default()
    }

    fn enabled() -> Self {
        Self {
            enabled: true,
            ..Self::default()
        }
    }
}

#[derive(Debug, Default)]
pub struct ShantenAgent;

impl ShantenAgent {
    fn select_reach_action(
        &self,
        ctx: &GameContext,
        legal_actions: &[LegalAction],
    ) -> Option<LegalAction> {
        if !should_reach(ctx) {
            return None;
        }
        legal_actions
            .iter()
            .find(|a| matches!(a, LegalAction::Reach))
            .cloned()
    }

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
        let mut diagnostics = DecisionDiagnostics::enabled();
        let decision =
            ShantenAgent.decide_with_diagnostics(context, legal_actions, &mut diagnostics);
        log_agent_decision(&decision);

        ShantenDecisionDiagnostic {
            selected_action: decision.action,
            selected_source: decision.source,
            normal_discard_action: decision.normal_discard,
            normal_discard: diagnostics.normal_discard,
            push_pull_inputs: decision.push_pull_inputs,
            push_pull_decision: decision.push_pull,
            defense: diagnostics.defense,
        }
    }

    // 最終 action と選択経路を1回で決める内部 helper。act() はこの結果を返し、
    // 共通箇所で agent decision ログを1件だけ出す。
    pub(crate) fn decide(&self, ctx: &GameContext, legal_actions: &[LegalAction]) -> AgentDecision {
        self.decide_with_diagnostics(ctx, legal_actions, &mut DecisionDiagnostics::disabled())
    }

    // 判断経路の本体。act() と構造化診断はこの1本を共有し、diagnostics が有効な場合だけ
    // 解析専用の追加情報を収集する。追加情報の収集は action 選択に影響しない。
    fn decide_with_diagnostics(
        &self,
        ctx: &GameContext,
        legal_actions: &[LegalAction],
        diagnostics: &mut DecisionDiagnostics,
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
            };
        }

        if let Some(action) = legal_actions
            .iter()
            .find(|a| matches!(a, LegalAction::Ryukyoku))
        {
            return AgentDecision {
                action: action.clone(),
                source: AgentActionSource::Ryukyoku,
                push_pull_inputs: None,
                push_pull: None,
                normal_discard: None,
            };
        }

        // 通常打牌の evaluation と action を一度だけ取得し、その evaluation を
        // 押し引き入力にも共有して二重計算を避ける。
        let discard_selection = self.select_normal_discard(ctx, legal_actions, diagnostics);
        let inputs = push_pull_inputs_from_context_with_evaluation(
            ctx,
            discard_selection.evaluation.as_ref(),
        );
        let push_pull = decide_push_pull(&inputs);
        log_push_pull_decision(&push_pull, &inputs, discard_selection.action.as_ref());

        let normal_discard = discard_selection.action.clone();

        if let Some((action, source)) = self.select_action_for_push_pull_mode(
            push_pull.mode,
            ctx,
            legal_actions,
            discard_selection.action.as_ref(),
            diagnostics,
        ) {
            return AgentDecision {
                action,
                source,
                push_pull_inputs: Some(inputs),
                push_pull: Some(push_pull),
                normal_discard,
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
        }
    }

    // 通常打牌選択。選択結果は診断の有無で変わらず、診断が有効な場合だけ全合法候補の
    // 構造化診断を追加で受け取る。
    fn select_normal_discard(
        &self,
        ctx: &GameContext,
        legal_actions: &[LegalAction],
        diagnostics: &mut DecisionDiagnostics,
    ) -> DiscardActionSelection {
        if !diagnostics.enabled {
            return select_discard_action_with_evaluation(ctx, legal_actions);
        }

        let selection = select_discard_action_with_diagnostic(ctx, legal_actions);
        diagnostics.normal_discard = Some(selection.diagnostic);
        selection.selection
    }

    // 押し引きモードに応じた action 選択。候補は必要になった時点でのみ計算する。
    // 選ばれた action とともに、その選択経路を表す source を返す。
    //
    // - Push:    Reach → 通常打牌 → 防御 fallback
    // - Neutral: 通常打牌 → 防御 fallback(Reach は検討しない)
    // - Fold:    防御 fallback → 通常打牌(Reach は検討しない)
    fn select_action_for_push_pull_mode(
        &self,
        mode: PushPullMode,
        ctx: &GameContext,
        legal_actions: &[LegalAction],
        normal_discard: Option<&LegalAction>,
        diagnostics: &mut DecisionDiagnostics,
    ) -> Option<(LegalAction, AgentActionSource)> {
        match mode {
            PushPullMode::Push => {
                if let Some(action) = self.select_reach_action(ctx, legal_actions) {
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
                if let Some(result) = self.select_defense_fallback(ctx, legal_actions, diagnostics)
                {
                    return Some(result);
                }
                normal_discard
                    .cloned()
                    .map(|action| (action, AgentActionSource::NormalDiscard))
            }
        }
    }

    // 防御 fallback を採用する場合に、その理由を診断ログへ出しつつ action と種別を返す。
    //
    // 診断が有効な場合は、採用の有無にかかわらず検討した候補評価を収集する。候補評価の収集は
    // 選択結果に影響せず、選択は select_defense_fallback_action_with_kind() が source of truth。
    fn select_defense_fallback(
        &self,
        ctx: &GameContext,
        legal_actions: &[LegalAction],
        diagnostics: &mut DecisionDiagnostics,
    ) -> Option<(LegalAction, AgentActionSource)> {
        let selected = select_defense_fallback_action_with_kind(ctx, legal_actions);

        if let Some((action, kind)) = selected {
            log_defense_fallback_decision(ctx, action, kind, legal_actions);
        }

        if diagnostics.enabled {
            diagnostics.defense = Some(DefenseDecisionDiagnostic::from_selection(
                ctx,
                legal_actions,
                selected,
            ));
        }

        let (action, kind) = selected?;
        Some((action.clone(), AgentActionSource::DefenseFallback(kind)))
    }
}

// action を agent decision ログ用のコンパクトな文字列へ変換する。
fn agent_action_label(action: &LegalAction) -> String {
    match action {
        LegalAction::Dahai { tile } => tile.to_mjai_string(),
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
fn log_agent_decision(decision: &AgentDecision) {
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

    tracing::debug!(
        target: AGENT_DECISION_LOG_TARGET,
        selected_action = %selected_action,
        selected_source = decision.source.label(),
        push_pull_mode = %push_pull_mode,
        push_pull_reason = %push_pull_reason,
        normal_discard = %normal_discard,
        defense_kind = %defense_kind,
        "agent decision",
    );
}

// 補正後の待ち枚数が明らかに少ない即リーチだけを抑制する最小判断。
// TODO: 役判定・打点・押し引きを考慮したリーチ判断に置き換える。
fn should_reach(ctx: &GameContext) -> bool {
    let tiles: Vec<_> = ctx
        .hand_tiles()
        .iter()
        .copied()
        .chain(ctx.drawn_tile())
        .collect();

    // 手牌情報がない場合は従来挙動を維持する。
    if tiles.is_empty() {
        return true;
    }

    // visible_tiles がない場合は補正できないため従来挙動を維持する。
    if ctx.visible_tiles().is_empty() {
        return true;
    }

    let counts = TileCounts::from_tiles(tiles.iter().copied());
    let acceptance = calculate_acceptance_with_visible_tiles(&counts, ctx.visible_tiles());

    // テンパイしていないなら即リーチしない。
    if acceptance.current.min() != 0 {
        return false;
    }

    acceptance.total_remaining() >= REACH_MIN_REMAINING
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
    use crate::defense::{HonorSafetyRank, select_defense_fallback_action};
    use crate::discard_selection::{select_best_discard_evaluation, select_discard_action};
    use crate::push_pull::{PushPullReason, push_pull_inputs_from_context};
    use bot_logic::{DiscardComparisonReason, TileId, TileType, compare_discard_evaluations};

    fn tile(value: u8) -> TileId {
        TileId::new(value).unwrap()
    }

    fn dahai(value: u8) -> LegalAction {
        LegalAction::Dahai { tile: tile(value) }
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
    fn picks_reach_when_available() {
        let mut agent = ShantenAgent;
        let ctx = GameContext::default();
        let actions = vec![LegalAction::Reach];
        assert_eq!(agent.act(&ctx, &actions), LegalAction::Reach);
    }

    #[test]
    fn prefers_reach_over_evaluated_dahai() {
        let mut agent = ShantenAgent;
        let ctx = GameContext::with_drawn_tile(tile(0));
        let actions = vec![LegalAction::Reach, dahai(0)];
        assert_eq!(agent.act(&ctx, &actions), LegalAction::Reach);
    }

    #[test]
    fn reach_is_policy_choice_not_fallback() {
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
            .chain([LegalAction::Reach])
            .collect();

        assert!(select_discard_action(&ctx, &actions).is_some());
        assert_eq!(agent.act(&ctx, &actions), LegalAction::Reach);
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

    #[test]
    fn does_not_actively_claim_melds_or_kans() {
        let mut agent = ShantenAgent;
        let ctx = GameContext::default();
        let actions = vec![
            LegalAction::Chi {
                tile: tile(17),
                consumed: vec![tile(12), tile(20)],
            },
            LegalAction::Pon {
                tile: tile(108),
                consumed: vec![tile(109), tile(110)],
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
            LegalAction::None,
        ];
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

    // 4面子 + 1s + 9s のタンキ含みテンパイ形。捨て牌前提で待ちは {1s, 9s}。
    const TENPAI_HAND: [u8; 13] = [0, 4, 8, 12, 17, 20, 24, 28, 32, 36, 40, 44, 72];
    const TENPAI_DRAWN: u8 = 104;

    fn tenpai_context(extra_visible: &[u8]) -> GameContext {
        let hand: Vec<_> = TENPAI_HAND.iter().map(|&value| tile(value)).collect();
        let mut visible = hand.clone();
        visible.push(tile(TENPAI_DRAWN));
        visible.extend(extra_visible.iter().map(|&value| tile(value)));
        GameContext::from_parts_with_visible_tiles(
            Some(tile(TENPAI_DRAWN)),
            hand,
            vec![],
            None,
            None,
            visible,
        )
    }

    fn tenpai_actions() -> Vec<LegalAction> {
        TENPAI_HAND
            .iter()
            .map(|&value| dahai(value))
            .chain([dahai(TENPAI_DRAWN)])
            .chain([LegalAction::Reach])
            .collect()
    }

    #[test]
    fn reaches_when_visible_waits_are_plentiful() {
        let mut agent = ShantenAgent;
        let ctx = tenpai_context(&[]);
        assert_eq!(agent.act(&ctx, &tenpai_actions()), LegalAction::Reach);
    }

    #[test]
    fn skips_reach_when_visible_waits_are_scarce() {
        let mut agent = ShantenAgent;
        // 1s / 9s をそれぞれ2枚見せて待ち枚数を枯らす。
        let ctx = tenpai_context(&[73, 74, 105, 106]);
        let selected = agent.act(&ctx, &tenpai_actions());
        assert!(matches!(selected, LegalAction::Dahai { .. }));
    }

    #[test]
    fn reaches_when_visible_tiles_empty_even_with_hand() {
        let mut agent = ShantenAgent;
        let hand: Vec<_> = TENPAI_HAND.iter().map(|&value| tile(value)).collect();
        let ctx = GameContext::from_parts(Some(tile(TENPAI_DRAWN)), hand);
        assert_eq!(agent.act(&ctx, &tenpai_actions()), LegalAction::Reach);
    }

    #[test]
    fn reaches_without_hand_information() {
        let mut agent = ShantenAgent;
        let ctx = GameContext::default();
        assert_eq!(agent.act(&ctx, &[LegalAction::Reach]), LegalAction::Reach);
    }

    #[test]
    fn follows_discard_selection_for_same_tile_type() {
        let mut agent = ShantenAgent;
        let ctx = GameContext::from_parts(Some(tile(16)), vec![tile(17)]);
        let actions = vec![dahai(17), dahai(16)];

        let expected = select_discard_action(&ctx, &actions).unwrap();

        assert_eq!(agent.act(&ctx, &actions), expected);
    }

    // 他家(player 1)がリーチしており、その河に 16(5m) がある局面。自分は player 0。
    fn opponent_reach_context(drawn_tile: Option<u8>, hand_values: &[u8]) -> GameContext {
        let discards = [vec![], vec![tile(16)], vec![], vec![]];
        GameContext::from_parts_with_table_state(
            drawn_tile.map(tile),
            hand_values.iter().map(|&value| tile(value)).collect(),
            vec![],
            None,
            None,
            Vec::new(),
            Some(0),
            None,
            discards,
            [false, true, false, false],
        )
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
    fn neutral_prefers_normal_discard_over_genbutsu_fallback() {
        let mut agent = ShantenAgent;
        // 単独の子リーチに対する強い一向聴で Neutral。共通現物 16(5m) があっても
        // Neutral では通常打牌(浮いた 116(北))を防御 fallback より優先する。
        let hand_values = [0, 4, 8, 12, 13, 20, 24, 28, 32, 36, 40, 44, 89];
        let ctx = opponent_reach_context(Some(116), &hand_values);
        assert_eq!(
            decide_push_pull(&push_pull_inputs_from_context(&ctx)).mode,
            PushPullMode::Neutral
        );
        let actions: Vec<LegalAction> = hand_values
            .iter()
            .map(|&value| dahai(value))
            .chain([dahai(116), dahai(16)])
            .collect();
        let normal = select_discard_action(&ctx, &actions).unwrap();
        assert_eq!(agent.act(&ctx, &actions), normal);
        assert_ne!(agent.act(&ctx, &actions), dahai(16));
    }

    #[test]
    fn fold_without_common_genbutsu_falls_through_to_normal_discard() {
        let mut agent = ShantenAgent;
        // 他家リーチ中でも合法 Dahai に共通現物が無い Fold 局面。Reach は抑制し通常打牌へ進む。
        let ctx = opponent_reach_context(Some(0), &[]);
        assert_eq!(
            decide_push_pull(&push_pull_inputs_from_context(&ctx)).mode,
            PushPullMode::Fold
        );
        let actions = vec![LegalAction::Reach, dahai(0), dahai(56)];
        assert_eq!(agent.act(&ctx, &actions), dahai(0));
    }

    #[test]
    fn keeps_normal_behavior_without_opponent_reach() {
        let mut agent = ShantenAgent;
        // 他家リーチが無ければ、現物相当の牌があっても従来の Reach を選ぶ。
        let discards = [vec![], vec![tile(16)], vec![], vec![]];
        let ctx = GameContext::from_parts_with_table_state(
            Some(tile(0)),
            vec![],
            vec![],
            None,
            None,
            Vec::new(),
            Some(0),
            None,
            discards,
            [false; 4],
        );
        let actions = vec![LegalAction::Reach, dahai(16)];
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

    // 他家(player 1)がリーチしており、その河に 16(5m) がある局面に visible_tiles を加える。
    fn opponent_reach_context_with_visible(
        drawn_tile: Option<u8>,
        hand_values: &[u8],
        visible_values: &[u8],
    ) -> GameContext {
        let discards = [vec![], vec![tile(16)], vec![], vec![]];
        GameContext::from_parts_with_table_state(
            drawn_tile.map(tile),
            hand_values.iter().map(|&value| tile(value)).collect(),
            vec![],
            None,
            None,
            visible_values.iter().map(|&value| tile(value)).collect(),
            Some(0),
            None,
            discards,
            [false, true, false, false],
        )
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
        assert_eq!(
            decide_push_pull(&push_pull_inputs_from_context(&ctx)).mode,
            PushPullMode::Fold
        );
        let actions = vec![LegalAction::Reach, dahai(0), dahai(56)];
        assert_eq!(agent.act(&ctx, &actions), dahai(0));
    }

    #[test]
    fn does_not_use_honor_safety_fallback_without_opponent_reach() {
        let mut agent = ShantenAgent;
        // 他家リーチが無ければ、字牌が合法でも従来の Reach を選ぶ。
        let ctx = GameContext::from_parts_with_table_state(
            Some(tile(0)),
            vec![],
            vec![],
            None,
            None,
            Vec::new(),
            Some(0),
            None,
            [vec![], vec![], vec![], vec![]],
            [false; 4],
        );
        let actions = vec![LegalAction::Reach, dahai(108)];
        assert_eq!(agent.act(&ctx, &actions), LegalAction::Reach);
    }

    #[test]
    fn honor_safety_fallback_ignores_number_dahai() {
        let mut agent = ShantenAgent;
        // 数牌のみで字牌がなければ字牌 fallback は発動しない。Fold だが安全牌が無いので通常打牌へ進む。
        let ctx = opponent_reach_context(Some(0), &[]);
        assert_eq!(
            decide_push_pull(&push_pull_inputs_from_context(&ctx)).mode,
            PushPullMode::Fold
        );
        let actions = vec![LegalAction::Reach, dahai(0), dahai(56)];
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

    // 東場・自分 player0・player1 リーチ。親を変えて player1 の自風を切り替える。
    fn opponent_reach_wind_context(oya: u8, drawn_tile: Option<u8>) -> GameContext {
        let discards = [vec![], vec![tile(16)], vec![], vec![]];
        GameContext::from_parts_with_table_state(
            drawn_tile.map(tile),
            vec![],
            vec![],
            TileType::new(27),
            None,
            Vec::new(),
            Some(0),
            Some(oya),
            discards,
            [false, true, false, false],
        )
    }

    #[test]
    fn honor_safety_fallback_breaks_same_rank_ties_by_opponent_honor_value() {
        let mut agent = ShantenAgent;
        // oya = player3 なので player1 の自風は西。北は客風、中は役牌。
        let ctx = opponent_reach_wind_context(3, Some(0));
        assert_eq!(agent.act(&ctx, &[dahai(132), dahai(120)]), dahai(120));
        assert_eq!(agent.act(&ctx, &[dahai(120), dahai(132)]), dahai(120));

        // oya = player1 なので player1 の自風は東。東はダブ東で最も危険。
        let ctx = opponent_reach_wind_context(1, Some(0));
        assert_eq!(agent.act(&ctx, &[dahai(108), dahai(132)]), dahai(132));
        assert_eq!(agent.act(&ctx, &[dahai(108), dahai(120)]), dahai(120));
    }

    #[test]
    fn honor_safety_fallback_keeps_visible_count_over_opponent_honor_value() {
        let mut agent = ShantenAgent;
        // 中は3枚見えの役牌、北は0枚見えの客風。見え枚数の安全度を役牌価値で逆転しない。
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

    // 他家(player 1)がリーチしている局面。河・visible・手牌・引き牌を個別に指定する。
    fn suited_reach_context(
        drawn_tile: Option<u8>,
        hand_values: &[u8],
        visible_values: &[u8],
        reacher_discards: &[u8],
    ) -> GameContext {
        let discards = [
            vec![],
            reacher_discards.iter().map(|&value| tile(value)).collect(),
            vec![],
            vec![],
        ];
        GameContext::from_parts_with_table_state(
            drawn_tile.map(tile),
            hand_values.iter().map(|&value| tile(value)).collect(),
            vec![],
            None,
            None,
            visible_values.iter().map(|&value| tile(value)).collect(),
            Some(0),
            None,
            discards,
            [false, true, false, false],
        )
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
        // 共通現物も字牌もなし。4m を3枚見えにして経路 [3m,4m] を OneChance にし 2m を OneChance。
        // 無スジ 0(1m) より OneChance 4(2m) を選ぶ。
        let ctx = suited_reach_context(Some(0), &[], &[12, 13, 14], &[]);
        let actions = vec![dahai(0), dahai(4)];
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
        // 1s は 4s(84) 河でスジ(Suji)、5s は無スジ・壁なし(NoSafety)。最も安全な NoChance を選ぶ。
        let ctx = suited_reach_context(Some(0), &[], &[40, 41, 42, 43, 64, 65, 66], &[84]);
        let actions = vec![dahai(88), dahai(72), dahai(68), dahai(36)];
        assert_eq!(agent.act(&ctx, &actions), dahai(36));
    }

    #[test]
    fn fold_without_safe_suited_falls_through_to_normal_discard() {
        let mut agent = ShantenAgent;
        // Fold 局面で共通現物も字牌もなく数牌が全て NoSafety なら、防御 fallback は無い。
        // Reach は抑制し、防御牌がないことを理由に失敗させず通常打牌へ進む。
        let ctx = suited_reach_context(Some(0), &[], &[], &[]);
        assert_eq!(
            decide_push_pull(&push_pull_inputs_from_context(&ctx)).mode,
            PushPullMode::Fold
        );
        let actions = vec![LegalAction::Reach, dahai(0), dahai(4)];
        assert_eq!(agent.act(&ctx, &actions), dahai(0));
    }

    #[test]
    fn does_not_use_suited_safety_fallback_without_opponent_reach() {
        let mut agent = ShantenAgent;
        // 他家リーチが無ければ、河に 16(5m) があり 4(2m) がスジ相当でも従来の Reach を選ぶ。
        let discards = [vec![], vec![tile(16)], vec![], vec![]];
        let ctx = GameContext::from_parts_with_table_state(
            Some(tile(0)),
            vec![],
            vec![],
            None,
            None,
            Vec::new(),
            Some(0),
            None,
            discards,
            [false; 4],
        );
        let actions = vec![LegalAction::Reach, dahai(4)];
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
    fn push_prefers_normal_discard_over_suited_safety_fallback() {
        let mut agent = ShantenAgent;
        // テンパイで単独の子リーチに対しては Push。Reach が合法でなければ通常打牌へ進み、
        // 防御 fallback より通常打牌(32(9m))を優先する。
        let hand_values = [0, 4, 8, 12, 13, 20, 24, 28, 32, 36, 40, 44, 89];
        let ctx = suited_reach_context(Some(88), &hand_values, &[4, 5, 6, 7], &[]);
        assert_eq!(
            decide_push_pull(&push_pull_inputs_from_context(&ctx)).mode,
            PushPullMode::Push
        );
        let actions: Vec<LegalAction> = hand_values
            .iter()
            .map(|&value| dahai(value))
            .chain([dahai(88)])
            .collect();
        let normal = select_discard_action(&ctx, &actions).unwrap();
        assert_eq!(agent.act(&ctx, &actions), normal);
        assert_ne!(agent.act(&ctx, &actions), dahai(4));
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

    // テンパイ手牌で他家リーチを受ける局面。oya と reached を指定してモードを作り分ける。
    // visible は空にして should_reach を従来挙動(true)に保つ。
    fn tenpai_under_reach_context(oya: Option<u8>, reached: [bool; 4]) -> GameContext {
        let hand: Vec<_> = TENPAI_HAND.iter().map(|&value| tile(value)).collect();
        // リーチ者(player 1)の河に 5m を置き、テンパイ手牌の 5m を現物にする。
        let discards = [vec![], vec![tile(16)], vec![], vec![]];
        GameContext::from_parts_with_table_state(
            Some(tile(TENPAI_DRAWN)),
            hand,
            vec![],
            None,
            None,
            Vec::new(),
            Some(0),
            oya,
            discards,
            reached,
        )
    }

    #[test]
    fn push_tenpai_against_single_non_dealer_reaches() {
        let mut agent = ShantenAgent;
        // 単独の子リーチに対するテンパイ。decide_push_pull は Push。
        // Reach が合法で should_reach() == true なら、現物があっても Reach を選ぶ。
        let ctx = tenpai_under_reach_context(None, [false, true, false, false]);
        assert_eq!(
            decide_push_pull(&push_pull_inputs_from_context(&ctx)).mode,
            PushPullMode::Push
        );
        let actions = tenpai_actions();
        assert!(should_reach(&ctx));
        assert_eq!(agent.act(&ctx, &actions), LegalAction::Reach);
    }

    #[test]
    fn neutral_tenpai_against_dealer_reach_prefers_normal_discard_over_reach() {
        let mut agent = ShantenAgent;
        // 親リーチに対するテンパイ。decide_push_pull は Neutral。
        // Reach が合法でも選ばず、暫定的にダマ相当の通常打牌を優先する。
        let ctx = tenpai_under_reach_context(Some(1), [false, true, false, false]);
        assert_eq!(
            decide_push_pull(&push_pull_inputs_from_context(&ctx)).mode,
            PushPullMode::Neutral
        );
        let actions = tenpai_actions();
        let normal = select_discard_action(&ctx, &actions).unwrap();
        let selected = agent.act(&ctx, &actions);
        assert_eq!(selected, normal);
        assert_ne!(selected, LegalAction::Reach);
    }

    #[test]
    fn neutral_tenpai_against_multiple_reach_prefers_normal_discard_over_reach() {
        let mut agent = ShantenAgent;
        // 複数リーチに対するテンパイ。decide_push_pull は Neutral。Reach は選ばない。
        let ctx = tenpai_under_reach_context(None, [false, true, true, false]);
        assert_eq!(
            decide_push_pull(&push_pull_inputs_from_context(&ctx)).mode,
            PushPullMode::Neutral
        );
        let actions = tenpai_actions();
        let normal = select_discard_action(&ctx, &actions).unwrap();
        let selected = agent.act(&ctx, &actions);
        assert_eq!(selected, normal);
        assert_ne!(selected, LegalAction::Reach);
    }

    // 2向聴以上で他家リーチを受ける Fold 局面。リーチ者の河に 5s を置き手牌の 5s を現物にする。
    const FOLD_HAND: [u8; 13] = [0, 4, 17, 20, 36, 40, 56, 60, 89, 108, 112, 120, 124];
    const FOLD_DRAWN: u8 = 16;

    fn fold_under_reach_context() -> GameContext {
        let hand: Vec<_> = FOLD_HAND.iter().map(|&value| tile(value)).collect();
        let discards = [vec![], vec![tile(89)], vec![], vec![]];
        GameContext::from_parts_with_table_state(
            Some(tile(FOLD_DRAWN)),
            hand,
            vec![],
            None,
            None,
            Vec::new(),
            Some(0),
            None,
            discards,
            [false, true, false, false],
        )
    }

    fn fold_actions() -> Vec<LegalAction> {
        FOLD_HAND
            .iter()
            .map(|&value| dahai(value))
            .chain([dahai(FOLD_DRAWN)])
            .collect()
    }

    #[test]
    fn fold_prefers_defense_fallback_over_normal_discard() {
        let mut agent = ShantenAgent;
        // 二向聴以上で他家リーチを受ける Fold 局面。防御 fallback(現物 5s)と通常打牌が異なり、
        // Fold では防御 fallback を通常打牌より優先する。
        let ctx = fold_under_reach_context();
        assert_eq!(
            decide_push_pull(&push_pull_inputs_from_context(&ctx)).mode,
            PushPullMode::Fold
        );
        let actions = fold_actions();
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
        // 非合法な全体最善を使うと Push、合法なツモ切り(shanten 1)を使うと Neutral になる。
        // Agent は合法候補側の evaluation / mode を使う。
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

        // 非合法な全体最善候補の mode は Push。
        let global_best = select_best_discard_evaluation(&ctx, &tiles).unwrap();
        assert_eq!(global_best.min_shanten_after_discard(), 0);
        let illegal_mode = decide_push_pull(&push_pull_inputs_from_context_with_evaluation(
            &ctx,
            Some(&global_best),
        ))
        .mode;
        assert_eq!(illegal_mode, PushPullMode::Push);

        // 合法なのはツモ切り 3p だけ。Agent が使う offense は合法候補の評価に一致する。
        let actions = vec![dahai(drawn)];
        let selection = select_discard_action_with_evaluation(&ctx, &actions);
        let legal_evaluation = selection.evaluation.clone().unwrap();
        assert_eq!(legal_evaluation.min_shanten_after_discard(), 1);
        let legal_inputs =
            push_pull_inputs_from_context_with_evaluation(&ctx, selection.evaluation.as_ref());
        let legal_mode = decide_push_pull(&legal_inputs).mode;

        assert_ne!(illegal_mode, legal_mode);
        assert_eq!(legal_mode, PushPullMode::Neutral);
        assert_eq!(
            legal_inputs.offense.unwrap().min_shanten_after_discard,
            legal_evaluation.min_shanten_after_discard()
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
        // 他家リーチなし → Push。Reach が合法なら Reach を選ぶ。
        let ctx = GameContext::with_drawn_tile(tile(0));
        let actions = vec![LegalAction::Reach, dahai(0)];
        let decision = agent.decide(&ctx, &actions);
        assert_eq!(decision.action, LegalAction::Reach);
        assert_eq!(decision.source, AgentActionSource::Reach);
        assert_eq!(decision.push_pull.map(|d| d.mode), Some(PushPullMode::Push));
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
    fn decide_reports_normal_discard_source_on_neutral() {
        let agent = ShantenAgent;
        // 単独の子リーチに対する強い一向聴で Neutral。共通現物があっても通常打牌を選ぶ。
        let hand_values = [0, 4, 8, 12, 13, 20, 24, 28, 32, 36, 40, 44, 89];
        let ctx = opponent_reach_context(Some(116), &hand_values);
        assert_eq!(
            decide_push_pull(&push_pull_inputs_from_context(&ctx)).mode,
            PushPullMode::Neutral
        );
        let actions: Vec<LegalAction> = hand_values
            .iter()
            .map(|&value| dahai(value))
            .chain([dahai(116), dahai(16)])
            .collect();
        let normal = select_discard_action(&ctx, &actions).unwrap();
        let decision = agent.decide(&ctx, &actions);
        assert_eq!(decision.source, AgentActionSource::NormalDiscard);
        assert_eq!(decision.action, normal);
        assert_eq!(
            decision.push_pull.map(|d| d.mode),
            Some(PushPullMode::Neutral)
        );
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

    // 回帰構造: normal_discard(通常打牌)と最終 selected_action(防御 fallback)が
    // Fold 時に異なり得ることを、SuitedSafety 経路で確認する。
    // 実牌姿での防御は河・visible の正確な再現が必要なため、pure な選択経路として構築する。
    #[test]
    fn decide_reports_defense_fallback_suited_safety_source_on_fold() {
        use crate::defense::SuitedSafetyRank;

        let agent = ShantenAgent;
        // 共通現物も字牌もなし。4m を4枚見せて 2m を NoChance にする。
        // ツモ 1m(0) だけが手牌評価対象なので通常打牌は 1m、防御 fallback は NoChance の 2m。
        let ctx = suited_reach_context(Some(0), &[], &[12, 13, 14, 15], &[]);
        let actions = vec![dahai(0), dahai(4)];
        assert_eq!(
            decide_push_pull(&push_pull_inputs_from_context(&ctx)).mode,
            PushPullMode::Fold
        );
        let decision = agent.decide(&ctx, &actions);
        assert_eq!(decision.action, dahai(4));
        assert_eq!(
            decision.source,
            AgentActionSource::DefenseFallback(DefenseFallbackKind::SuitedSafety(
                SuitedSafetyRank::NoChance
            ))
        );
        assert_eq!(decision.normal_discard, Some(dahai(0)));
        assert_ne!(decision.normal_discard, Some(decision.action.clone()));
    }

    #[test]
    fn decide_falls_through_to_normal_discard_when_fold_has_no_defense() {
        let agent = ShantenAgent;
        // Fold だが共通現物・字牌・数牌 safety がいずれも無い局面。通常打牌へ進む。
        let ctx = suited_reach_context(Some(0), &[], &[], &[]);
        assert_eq!(
            decide_push_pull(&push_pull_inputs_from_context(&ctx)).mode,
            PushPullMode::Fold
        );
        let actions = vec![LegalAction::Reach, dahai(0), dahai(4)];
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

    // ---- 構造化診断 (ShantenAgent::diagnose) テスト ----
    // 診断は act() と同じ selection logic を通るため、最終 action は常に act() と一致する。

    // 診断の最終 action / source が act() と一致することを確認し、診断を返す共通 helper。
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

    #[test]
    fn diagnose_free_function_matches_associated_function() {
        let ctx = fold_under_reach_context();
        let actions = fold_actions();
        assert_eq!(
            diagnose_shanten_decision(&ctx, &actions),
            ShantenAgent::diagnose(&ctx, &actions)
        );
    }

    #[test]
    fn diagnose_reports_hora_without_other_judgments() {
        let ctx = opponent_reach_context(Some(0), &[]);
        let actions = vec![dahai(16), LegalAction::Hora];
        let diagnostic = diagnose_matching_act(&ctx, &actions);

        assert_eq!(diagnostic.selected_action, LegalAction::Hora);
        assert_eq!(diagnostic.selected_source, AgentActionSource::Hora);
        assert_eq!(diagnostic.normal_discard, None);
        assert_eq!(diagnostic.normal_discard_action, None);
        assert_eq!(diagnostic.push_pull_inputs, None);
        assert_eq!(diagnostic.push_pull_decision, None);
        assert_eq!(diagnostic.defense, None);
        assert_eq!(diagnostic.defense_fallback_kind(), None);
    }

    #[test]
    fn diagnose_reports_ryukyoku_without_other_judgments() {
        let ctx = opponent_reach_context(Some(0), &[]);
        let actions = vec![dahai(16), LegalAction::Ryukyoku];
        let diagnostic = diagnose_matching_act(&ctx, &actions);

        assert_eq!(diagnostic.selected_action, LegalAction::Ryukyoku);
        assert_eq!(diagnostic.selected_source, AgentActionSource::Ryukyoku);
        assert_eq!(diagnostic.normal_discard, None);
        assert_eq!(diagnostic.normal_discard_action, None);
        assert_eq!(diagnostic.push_pull_inputs, None);
        assert_eq!(diagnostic.push_pull_decision, None);
        assert_eq!(diagnostic.defense, None);
    }

    #[test]
    fn diagnose_reports_reach_source() {
        // 待ち枚数が十分なテンパイで Reach が選ばれる局面。
        let ctx = tenpai_context(&[]);
        let actions = tenpai_actions();
        let diagnostic = diagnose_matching_act(&ctx, &actions);

        assert_eq!(diagnostic.selected_action, LegalAction::Reach);
        assert_eq!(diagnostic.selected_source, AgentActionSource::Reach);
        // Reach 経路でも通常打牌評価は実行済みなので、比較用の通常打牌は保持する。
        assert!(diagnostic.normal_discard.is_some());
        assert!(diagnostic.normal_discard_action.is_some());
        assert_eq!(
            diagnostic.push_pull_decision.map(|decision| decision.mode),
            Some(PushPullMode::Push)
        );
        // Reach を採用したので防御 fallback は検討していない。
        assert_eq!(diagnostic.defense, None);
    }

    #[test]
    fn diagnose_reports_normal_discard_source_with_matching_selection() {
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

        let diagnostic = diagnose_matching_act(&ctx, &actions);

        assert_eq!(diagnostic.selected_source, AgentActionSource::NormalDiscard);
        assert_eq!(diagnostic.selected_action, normal);
        assert_eq!(diagnostic.normal_discard_action, Some(normal.clone()));

        // 診断内の selected discard が実 action の牌種と一致する。
        let LegalAction::Dahai {
            tile: selected_tile,
        } = normal
        else {
            panic!("expected dahai");
        };
        let selected = diagnostic
            .normal_discard
            .as_ref()
            .unwrap()
            .selected
            .as_ref();
        assert_eq!(selected.map(|e| e.discard), Some(selected_tile.tile_type()));
        assert_eq!(diagnostic.defense, None);
    }

    #[test]
    fn diagnose_keeps_only_legal_discard_candidates_with_comparison_reasons() {
        // 手牌には他の候補があるが、合法 Dahai は 1m / 5s / 北 の3種だけ。
        let hand_values = [0, 4, 8, 12, 17, 20, 24, 28, 32, 36, 40, 44, 89];
        let ctx = GameContext::from_parts(
            Some(tile(116)),
            hand_values.iter().map(|&value| tile(value)).collect(),
        );
        let actions = vec![dahai(0), dahai(89), dahai(116)];

        let diagnostic = diagnose_matching_act(&ctx, &actions);
        let normal_discard = diagnostic.normal_discard.as_ref().unwrap();

        let legal_types: Vec<_> = [0u8, 89, 116]
            .iter()
            .map(|&value| tile(value).tile_type())
            .collect();
        let candidate_types: Vec<_> = normal_discard
            .candidates
            .iter()
            .map(|candidate| candidate.evaluation.discard)
            .collect();
        assert_eq!(candidate_types, legal_types);

        // 選択された候補は1件で、最終 action と牌種が一致する。
        let selected: Vec<_> = normal_discard
            .candidates
            .iter()
            .filter(|candidate| candidate.selected)
            .collect();
        assert_eq!(selected.len(), 1);
        let LegalAction::Dahai {
            tile: selected_tile,
        } = &diagnostic.selected_action
        else {
            panic!("expected dahai");
        };
        assert_eq!(selected[0].evaluation.discard, selected_tile.tile_type());
        assert_eq!(
            selected[0].comparison_reason,
            DiscardComparisonReason::StableOrder
        );

        // 非選択候補は「何の比較軸で負けたか」を持つ。
        for candidate in normal_discard
            .candidates
            .iter()
            .filter(|candidate| !candidate.selected)
        {
            assert_eq!(
                candidate.selected_is_strictly_better_than_candidate,
                compare_discard_evaluations(
                    normal_discard.selected.as_ref().unwrap(),
                    &candidate.evaluation
                )
                .candidate_is_better
            );
        }
    }

    #[test]
    fn diagnose_matches_act_at_physical_tile_level_for_black_and_red_five() {
        // 赤5m と黒5m が同一牌種として合法。黒5優先を維持し、評価も黒5mの物理牌情報に合わせる。
        let ctx = GameContext::from_parts_with_dora(None, vec![tile(16), tile(17)], vec![tile(12)]);
        let actions = vec![dahai(16), dahai(17)];

        let diagnostic = diagnose_matching_act(&ctx, &actions);
        assert_eq!(diagnostic.selected_action, dahai(17));
        assert_eq!(diagnostic.selected_source, AgentActionSource::NormalDiscard);

        let selected = diagnostic
            .normal_discard
            .as_ref()
            .unwrap()
            .selected
            .as_ref()
            .unwrap();
        assert!(!selected.discards_red_five);
        assert_eq!(selected.discarded_dora_count, 1);
    }

    #[test]
    fn diagnose_matches_act_when_only_red_five_is_legal() {
        let ctx = GameContext::from_parts_with_dora(None, vec![tile(16), tile(17)], vec![tile(12)]);
        let actions = vec![dahai(16)];

        let diagnostic = diagnose_matching_act(&ctx, &actions);
        assert_eq!(diagnostic.selected_action, dahai(16));

        let selected = diagnostic
            .normal_discard
            .as_ref()
            .unwrap()
            .selected
            .as_ref()
            .unwrap();
        assert!(selected.discards_red_five);
        assert_eq!(selected.discarded_dora_count, 2);
    }

    // 押し引き診断が実際の push-pull 結果と一致することを確認する共通 helper。
    fn assert_push_pull_diagnostic(
        ctx: &GameContext,
        actions: &[LegalAction],
        expected_mode: PushPullMode,
    ) -> ShantenDecisionDiagnostic {
        let diagnostic = diagnose_matching_act(ctx, actions);
        let inputs = diagnostic.push_pull_inputs.unwrap();
        let decision = diagnostic.push_pull_decision.unwrap();

        let selection = select_discard_action_with_evaluation(ctx, actions);
        assert_eq!(
            inputs,
            push_pull_inputs_from_context_with_evaluation(ctx, selection.evaluation.as_ref())
        );
        assert_eq!(decision, decide_push_pull(&inputs));
        assert_eq!(decision.mode, expected_mode);
        diagnostic
    }

    #[test]
    fn diagnose_holds_push_inputs_and_decision() {
        // テンパイで単独の子リーチに対する Push。
        let hand_values = [0, 4, 8, 12, 13, 20, 24, 28, 32, 36, 40, 44, 89];
        let ctx = suited_reach_context(Some(88), &hand_values, &[4, 5, 6, 7], &[]);
        let actions: Vec<LegalAction> = hand_values
            .iter()
            .map(|&value| dahai(value))
            .chain([dahai(88)])
            .collect();

        let diagnostic = assert_push_pull_diagnostic(&ctx, &actions, PushPullMode::Push);
        let inputs = diagnostic.push_pull_inputs.unwrap();
        assert_eq!(inputs.opponent_reach_count, 1);
        assert!(!inputs.dealer_reacher);
        assert!(!inputs.self_dealer);
        assert_eq!(
            inputs.offense.unwrap().min_shanten_after_discard,
            diagnostic
                .normal_discard
                .as_ref()
                .unwrap()
                .selected
                .as_ref()
                .unwrap()
                .min_shanten_after_discard()
        );
        assert_eq!(
            diagnostic.push_pull_decision.unwrap().reason,
            PushPullReason::TenpaiAgainstSingleNonDealer
        );
    }

    #[test]
    fn diagnose_holds_neutral_inputs_and_decision() {
        // 単独の子リーチに対する強い一向聴で Neutral。
        let hand_values = [0, 4, 8, 12, 13, 20, 24, 28, 32, 36, 40, 44, 89];
        let ctx = opponent_reach_context(Some(116), &hand_values);
        let actions: Vec<LegalAction> = hand_values
            .iter()
            .map(|&value| dahai(value))
            .chain([dahai(116), dahai(16)])
            .collect();

        let diagnostic = assert_push_pull_diagnostic(&ctx, &actions, PushPullMode::Neutral);
        assert_eq!(diagnostic.selected_source, AgentActionSource::NormalDiscard);
        // Neutral で通常打牌を採用したので防御 fallback は検討していない。
        assert_eq!(diagnostic.defense, None);
    }

    #[test]
    fn diagnose_holds_fold_inputs_and_decision() {
        let ctx = fold_under_reach_context();
        let actions = fold_actions();

        let diagnostic = assert_push_pull_diagnostic(&ctx, &actions, PushPullMode::Fold);
        assert_eq!(
            diagnostic.push_pull_decision.unwrap().reason,
            PushPullReason::TwoOrMoreShanten
        );
    }

    #[test]
    fn diagnose_reports_genbutsu_defense_fallback() {
        let ctx = fold_under_reach_context();
        let actions = fold_actions();
        let normal = select_discard_action(&ctx, &actions).unwrap();

        let diagnostic = diagnose_matching_act(&ctx, &actions);

        assert_eq!(
            diagnostic.selected_source,
            AgentActionSource::DefenseFallback(DefenseFallbackKind::Genbutsu)
        );
        assert_eq!(
            diagnostic.defense_fallback_kind(),
            Some(DefenseFallbackKind::Genbutsu)
        );
        assert_eq!(diagnostic.selected_action, dahai(89));
        // 通常打牌とは異なる action を選んでいる。
        assert_eq!(diagnostic.normal_discard_action, Some(normal.clone()));
        assert_ne!(diagnostic.normal_discard_action, Some(dahai(89)));

        let defense = diagnostic.defense.as_ref().unwrap();
        let selected = defense.selected.as_ref().unwrap();
        assert_eq!(selected.selected_kind, DefenseFallbackKind::Genbutsu);
        assert_eq!(selected.selected_action, "5s".to_string());
        assert!(selected.selected_genbutsu_for_all);

        // 候補診断は合法 Dahai 全件を持ち、選択された候補が最終 action と一致する。
        assert_eq!(defense.candidates.len(), actions.len());
        let selected_candidates: Vec<_> = defense
            .candidates
            .iter()
            .filter(|candidate| candidate.selected)
            .collect();
        assert_eq!(selected_candidates.len(), 1);
        assert_eq!(selected_candidates[0].action, diagnostic.selected_action);
        assert!(selected_candidates[0].genbutsu_for_all);
    }

    #[test]
    fn diagnose_reports_honor_safety_defense_candidates() {
        // 共通現物なし。東は2枚見え、南は0枚見え。より安全な東を切る。
        let ctx = opponent_reach_context_with_visible(Some(112), &[], &[108, 109]);
        let actions = vec![dahai(112), dahai(108)];

        let diagnostic = diagnose_matching_act(&ctx, &actions);

        assert_eq!(diagnostic.selected_action, dahai(108));
        assert_eq!(
            diagnostic.defense_fallback_kind(),
            Some(DefenseFallbackKind::HonorSafety(
                HonorSafetyRank::TwoVisible
            ))
        );

        let defense = diagnostic.defense.as_ref().unwrap();
        assert_eq!(
            defense
                .selected
                .as_ref()
                .unwrap()
                .selected_honor_safety_rank,
            Some(HonorSafetyRank::TwoVisible)
        );

        let south = &defense.candidates[0];
        let east = &defense.candidates[1];
        assert_eq!(south.tile, tile(112).tile_type());
        assert_eq!(south.honor_safety_rank, Some(HonorSafetyRank::NoVisible));
        assert!(!south.genbutsu_for_all);
        assert_eq!(south.wall_rank, None);
        assert_eq!(east.tile, tile(108).tile_type());
        assert_eq!(east.honor_safety_rank, Some(HonorSafetyRank::TwoVisible));
        assert!(east.selected);
    }

    #[test]
    fn diagnose_reports_suited_safety_defense_candidates() {
        use crate::defense::{SuitedSafetyRank, WallRank};

        // 共通現物も字牌もなし。4m を4枚見せて 2m を NoChance にする。
        let ctx = suited_reach_context(Some(0), &[], &[12, 13, 14, 15], &[]);
        let actions = vec![dahai(0), dahai(4)];

        let diagnostic = diagnose_matching_act(&ctx, &actions);

        assert_eq!(diagnostic.selected_action, dahai(4));
        assert_eq!(
            diagnostic.defense_fallback_kind(),
            Some(DefenseFallbackKind::SuitedSafety(
                SuitedSafetyRank::NoChance
            ))
        );

        let defense = diagnostic.defense.as_ref().unwrap();
        let one_man = &defense.candidates[0];
        let two_man = &defense.candidates[1];

        assert_eq!(one_man.tile, tile(0).tile_type());
        assert_eq!(one_man.wall_rank, Some(WallRank::NoWall));
        assert_eq!(one_man.suji_for_all_reached, Some(false));
        assert_eq!(one_man.suited_safety_rank, Some(SuitedSafetyRank::NoSafety));
        assert!(!one_man.selected);

        assert_eq!(two_man.tile, tile(4).tile_type());
        assert_eq!(two_man.wall_rank, Some(WallRank::NoChance));
        assert_eq!(two_man.suji_for_all_reached, Some(false));
        assert_eq!(two_man.suited_safety_rank, Some(SuitedSafetyRank::NoChance));
        assert!(two_man.selected);
    }

    #[test]
    fn diagnose_keeps_defense_candidates_when_fallback_is_not_adopted() {
        // Fold だが防御候補が無い局面。防御を検討した記録として候補評価だけ残る。
        let ctx = suited_reach_context(Some(0), &[], &[], &[]);
        let actions = vec![LegalAction::Reach, dahai(0), dahai(4)];

        let diagnostic = diagnose_matching_act(&ctx, &actions);

        assert_eq!(diagnostic.selected_source, AgentActionSource::NormalDiscard);
        assert_eq!(diagnostic.defense_fallback_kind(), None);

        let defense = diagnostic.defense.as_ref().unwrap();
        assert_eq!(defense.selected, None);
        assert_eq!(defense.selected_kind(), None);
        assert_eq!(defense.candidates.len(), 2);
        assert!(
            defense
                .candidates
                .iter()
                .all(|candidate| !candidate.selected)
        );
    }

    #[test]
    fn diagnose_reports_legal_dahai_fallback_source() {
        // 手牌情報が無く通常打牌も防御 fallback も選べない局面。合法 Dahai へ落ちる。
        let ctx = GameContext::default();
        let actions = vec![dahai(16), dahai(17)];

        let diagnostic = diagnose_matching_act(&ctx, &actions);

        assert_eq!(diagnostic.selected_action, dahai(17));
        assert_eq!(
            diagnostic.selected_source,
            AgentActionSource::LegalDahaiFallback
        );
        assert_eq!(diagnostic.normal_discard_action, None);
        assert!(diagnostic.push_pull_inputs.is_some());
    }

    #[test]
    fn diagnose_reports_none_source_for_empty_actions() {
        let ctx = GameContext::default();
        let diagnostic = diagnose_matching_act(&ctx, &[]);

        assert_eq!(diagnostic.selected_action, LegalAction::None);
        assert_eq!(diagnostic.selected_source, AgentActionSource::None);
        assert_eq!(diagnostic.normal_discard_action, None);
        // 通常打牌評価は実行したが合法候補が無いので、候補は空。
        let normal_discard = diagnostic.normal_discard.as_ref().unwrap();
        assert_eq!(normal_discard.selected, None);
        assert!(normal_discard.candidates.is_empty());
    }

    #[test]
    fn diagnose_reports_none_source_without_dahai_actions() {
        let ctx = GameContext::default();
        let actions = vec![
            LegalAction::Pon {
                tile: tile(108),
                consumed: vec![tile(109), tile(110)],
            },
            LegalAction::None,
        ];
        let diagnostic = diagnose_matching_act(&ctx, &actions);

        assert_eq!(diagnostic.selected_action, LegalAction::None);
        assert_eq!(diagnostic.selected_source, AgentActionSource::None);
    }

    // ---- 診断収集が action 選択へ影響しないことの確認 ----

    #[test]
    fn act_path_does_not_build_analysis_diagnostics() {
        // 通常の act() 経路では、解析専用の追加診断(候補ごとの形の内訳・全防御候補評価)を作らない。
        let ctx = fold_under_reach_context();
        let actions = fold_actions();

        let mut diagnostics = DecisionDiagnostics::disabled();
        let decision = ShantenAgent.decide_with_diagnostics(&ctx, &actions, &mut diagnostics);

        assert_eq!(decision.action, dahai(89));
        assert!(diagnostics.normal_discard.is_none());
        assert!(diagnostics.defense.is_none());
    }

    #[test]
    fn enabling_diagnostics_does_not_change_decision() {
        let cases: Vec<(GameContext, Vec<LegalAction>)> = vec![
            (fold_under_reach_context(), fold_actions()),
            (tenpai_context(&[]), tenpai_actions()),
            (GameContext::default(), vec![dahai(16), dahai(17)]),
            (GameContext::default(), vec![]),
            (
                opponent_reach_context(Some(0), &[]),
                vec![dahai(16), LegalAction::Hora],
            ),
            (
                suited_reach_context(Some(0), &[], &[12, 13, 14, 15], &[]),
                vec![dahai(0), dahai(4)],
            ),
        ];

        for (ctx, actions) in cases {
            let agent = ShantenAgent;
            let production = agent.decide(&ctx, &actions);
            let with_diagnostics =
                agent.decide_with_diagnostics(&ctx, &actions, &mut DecisionDiagnostics::enabled());
            assert_eq!(production, with_diagnostics);
        }
    }

    #[test]
    fn agent_action_source_labels_are_stable() {
        assert_eq!(AgentActionSource::Hora.label(), "Hora");
        assert_eq!(AgentActionSource::Ryukyoku.label(), "Ryukyoku");
        assert_eq!(AgentActionSource::Reach.label(), "Reach");
        assert_eq!(AgentActionSource::NormalDiscard.label(), "NormalDiscard");
        assert_eq!(
            AgentActionSource::DefenseFallback(DefenseFallbackKind::Genbutsu).label(),
            "DefenseFallback"
        );
        assert_eq!(
            AgentActionSource::LegalDahaiFallback.label(),
            "LegalDahaiFallback"
        );
        assert_eq!(AgentActionSource::None.label(), "None");
    }
}
