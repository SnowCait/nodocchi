//! `現在2向聴 → Chi / Pon → 打牌 → 2向聴のまま` の Call / Pass を、production policy へ接続せずに
//! 観測する。
//!
//! production の鳴き判断はこの候補を [`CallDecisionReason::PostCallNotIishanten`] で止め、Call /
//! Pass を比較しない。この module はその候補について、同じ Call を2つの scope で独立に評価し、
//! Pass と比べた結論・選ぶ鳴き後打牌・実行コストを並べるだけで、production の判断は一切変えない。
//! production path はこの module を通らず、専用の observation 入口を呼ばない限り追加の探索も
//! 走らない。
//!
//! # 対象候補
//!
//! production と同じ鳴き判断を1回行い、
//!
//! ```text
//! current_shanten = 2
//! post_call_min_shanten = 2
//! reason = PostCallNotIishanten
//! ```
//!
//! になった Chi / Pon 候補だけを対象にする。鳴き後の手牌・副露・1手評価・喰い替え禁止牌・副露数・
//! 最小向聴数は、その判断の candidate preparation が組み立てたものをそのまま使う。物理牌
//! semantics まで同じ鳴き後 state を作る候補 (`Reused`) は production と同じく先行候補の結果を
//! 複製し、評価し直さない。赤5 / 黒5 が違う候補は別の state として別々に評価する。
//!
//! # 2つの scope
//!
//! | scope | Call | Pass |
//! | --- | --- | --- |
//! | Progress | 鳴き後の合法打牌を既存の2向聴 Progress comparator で比べ、選んだ打牌の Progress 値 | 次の自摸を待つ現在2向聴 state の Progress 値 |
//! | Full | 鳴き後の局面を production の2向聴 discard selection へそのまま渡し、選ばれた打牌の Full 値 | 2→1 Call / Pass 比較の Pass 側と同じ Full 値 |
//!
//! Progress は「最初のツモで1向聴へ進む枝 → production の1向聴 continuation」だけの寄与で、2向聴
//! production selection の Progress cohort と同じ尺度になる。Full はそれに最初のツモで2向聴を
//! 維持する枝を1回だけ足した値。
//!
//! Full scope の Call 側は「全打牌候補を Full 評価する」ものではない。production の2向聴
//! selection (Progress cohort → provisional ranking → ドラ差 gate → gated top-2 の Full 追加評価 →
//! production comparator、Full の並列度も production のまま) を1回通すだけなので、gate が
//! 発火しない局面では選ばれた打牌の Full 値は存在せず unknown になる。0 などで補完しない。
//!
//! Progress と Full の値は混ぜない。比較は scope ごとに独立に行い、同値は既存 Call / Pass policy
//! と同じく Pass 側に倒す。
//!
//! # 計測条件
//!
//! Pass は request ごと・scope ごとに1回だけ評価し、全候補で共有する。Call は候補ごと・scope
//! ごとに、instrumentation を持たない計測 run と、探索規模を計上する観測 run の2本を取る。
//! どの run も新しい thread で行うため、向聴・受け入れの thread-local memo は毎回 cold から
//! 始まり、先に走った評価 (対象候補を決める production の鳴き判断を含む) が後の計測を暖めない。
//! 探索内の memo は run ごとに作り直すので、Progress と Full も互いに暖めない。

use std::time::{Duration, Instant};

use bot_logic::{FixedMeldCount, SearchStateMemoStats, ThreeShantenSearchStats, TileType};

use crate::action::{LegalAction, preferred_dahai_action_for_type};
use crate::call_decision::{
    CallDecisionDiagnostic, CallDecisionReason, CallIishantenComparison, CallKind,
    TwoShantenStayCallSource, TwoShantenStayCallState, compare_call_pass_self_tsumo_values,
    evaluate_call_decision_with_two_shanten_stay_calls, pass_two_shanten_expected_self_tsumo_value,
    pass_two_shanten_progress_self_tsumo_value, reaction_draw_distance,
};
use crate::context::GameContext;
use crate::discard_selection::select_two_shanten_progress_post_call_discard_observed;
use crate::iishanten_selection_depth_comparison::measured_on_a_fresh_thread;
use crate::two_shanten_full_parallel_comparison::{
    TwoShantenFullParallelDecision, TwoShantenFullParallelism,
    decide_with_two_shanten_full_parallelism,
};

/// 観測する scope。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TwoShantenStayCallScope {
    /// 最初のツモで1向聴へ進む枝だけの寄与。
    Progress,
    /// production の2向聴 selection semantics の Full 値。
    Full,
}

/// 値を確定できなかった理由。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TwoShantenStayCallUnknown {
    /// 反応元の席が分からず、Pass の horizon を作れない。Pass は評価しない。
    ReactionSourceUnknown,
    /// 鳴き後の局面を作れない、または鳴き後の合法打牌が無い。
    NoPostCallSelection,
    /// production の2向聴 selection が比較対象を1件に絞ったので、Progress も Full も評価しない。
    NoCompetingTargets,
    /// production の2向聴 selection でドラ差 gate が発火せず、Full 追加評価が走らない。
    FullGateNotFired,
    /// production の2向聴 selection で Full 追加評価の対象外になった打牌が選ばれた。
    SelectedOutsideFullPair,
    /// 評価したが、既存 helper が値を確定できなかった。
    Unresolved,
}

/// Call / Pass の片側の値 [[`bot_logic::SELF_TSUMO_VALUE_SCALE`]]。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TwoShantenStayCallValue {
    Known(u64),
    Unknown(TwoShantenStayCallUnknown),
}

impl TwoShantenStayCallValue {
    pub fn known(self) -> Option<u64> {
        match self {
            Self::Known(value) => Some(value),
            Self::Unknown(_) => None,
        }
    }

    fn from_option(value: Option<u64>, unknown: TwoShantenStayCallUnknown) -> Self {
        value.map_or(Self::Unknown(unknown), Self::Known)
    }
}

/// request 内で1回だけ評価し、全候補で共有する Pass 側。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TwoShantenStayPassObservation {
    pub value: TwoShantenStayCallValue,
    /// 評価そのものの実測。反応元不明で評価しなかった場合は `None`。
    pub elapsed: Option<Duration>,
}

/// Progress scope の Call 側。
#[derive(Debug, Clone)]
pub struct TwoShantenStayCallProgress {
    /// 選んだ鳴き後打牌。牌種の選択は Progress comparator、同じ牌種の物理牌は production と同じ
    /// 合法 Dahai の preference で決める。
    pub selected: Option<LegalAction>,
    pub value: TwoShantenStayCallValue,
    /// instrumentation を持たない計測 run の実測。
    pub elapsed: Duration,
    /// 観測 run の探索規模と探索内 memo の利用数。
    pub search: ThreeShantenSearchStats,
    pub memo: SearchStateMemoStats,
    /// 計測 run と観測 run が同じ打牌・同じ値になったか。
    pub runs_agree: bool,
}

/// Full scope の Call 側。値も選択も production の2向聴 selection が実際に使ったもの。
#[derive(Debug, Clone)]
pub struct TwoShantenStayCallFull {
    pub selected: Option<LegalAction>,
    pub value: TwoShantenStayCallValue,
    /// Progress 寄与を評価した ForwardTargets cohort。
    pub progress_cohort: Vec<TileType>,
    /// ドラ差 gate を通って Full 追加評価を行った候補。gate 不発では空。
    pub full_evaluated: Vec<TileType>,
    /// instrumentation を持たない計測 run の実測。鳴き後の局面を作れない場合は 0。
    pub elapsed: Duration,
    pub search: ThreeShantenSearchStats,
    pub memo: SearchStateMemoStats,
    /// Full 追加評価に実際に使った thread 数。
    pub full_workers: usize,
    pub runs_agree: bool,
}

/// Progress と Full の Call / Pass 結論の関係。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TwoShantenStayCallAgreement {
    /// どちらの scope も結論を確定し、同じ結論になった。
    Same,
    /// どちらの scope も結論を確定し、結論が反転した。
    Flipped,
    /// どちらかの scope が unknown。
    Undetermined,
}

/// 対象候補1件の観測。
#[derive(Debug, Clone)]
pub struct TwoShantenStayCallCandidate {
    /// production の [`CallDecisionDiagnostic::candidates`] の index。合法 action の列挙順。
    pub candidate_index: usize,
    pub action: LegalAction,
    pub kind: CallKind,
    /// 同じ鳴き後 state を作る先行候補の index。先行候補の観測をそのまま複製した候補だけ `Some`。
    pub reused_from: Option<usize>,
    pub forbidden_discards: Vec<TileType>,
    pub post_call_fixed_meld_count: FixedMeldCount,
    pub post_call_min_shanten: i8,
    pub progress: TwoShantenStayCallProgress,
    pub full: TwoShantenStayCallFull,
    pub progress_comparison: CallIishantenComparison,
    pub full_comparison: CallIishantenComparison,
}

impl TwoShantenStayCallCandidate {
    /// 2つの scope が同じ鳴き後打牌を選んだか。どちらかが選べなかった場合は `None`。
    pub fn selected_discards_match(&self) -> Option<bool> {
        Some(self.progress.selected.as_ref()? == self.full.selected.as_ref()?)
    }

    pub fn comparison_agreement(&self) -> TwoShantenStayCallAgreement {
        use CallIishantenComparison::Unknown;
        match (self.progress_comparison, self.full_comparison) {
            (Unknown, _) | (_, Unknown) => TwoShantenStayCallAgreement::Undetermined,
            (progress, full) if progress == full => TwoShantenStayCallAgreement::Same,
            _ => TwoShantenStayCallAgreement::Flipped,
        }
    }

    pub fn comparison(&self, scope: TwoShantenStayCallScope) -> CallIishantenComparison {
        match scope {
            TwoShantenStayCallScope::Progress => self.progress_comparison,
            TwoShantenStayCallScope::Full => self.full_comparison,
        }
    }

    pub fn call_value(&self, scope: TwoShantenStayCallScope) -> TwoShantenStayCallValue {
        match scope {
            TwoShantenStayCallScope::Progress => self.progress.value,
            TwoShantenStayCallScope::Full => self.full.value,
        }
    }
}

/// request 1件の観測。
#[derive(Debug, Clone)]
pub struct TwoShantenStayCallObservation {
    /// production の鳴き判断そのもの。observation はこの結論を変えない。合法な Chi / Pon が無い
    /// request では `None`。
    pub call: Option<CallDecisionDiagnostic>,
    pub reaction_source_player: Option<u8>,
    /// 対象候補がある request だけ評価する Pass 側。
    pub progress_pass: Option<TwoShantenStayPassObservation>,
    pub full_pass: Option<TwoShantenStayPassObservation>,
    pub candidates: Vec<TwoShantenStayCallCandidate>,
}

impl TwoShantenStayCallObservation {
    pub fn has_targets(&self) -> bool {
        !self.candidates.is_empty()
    }

    /// production の鳴き判断が採用した鳴き。
    pub fn production_selected(&self) -> Option<&LegalAction> {
        self.call.as_ref().and_then(|call| call.selected.as_ref())
    }

    pub fn production_reason(&self) -> Option<CallDecisionReason> {
        self.call.as_ref().map(|call| call.reason)
    }

    pub fn pass(&self, scope: TwoShantenStayCallScope) -> Option<TwoShantenStayPassObservation> {
        match scope {
            TwoShantenStayCallScope::Progress => self.progress_pass,
            TwoShantenStayCallScope::Full => self.full_pass,
        }
    }
}

/// `現在2向聴 → Chi / Pon → 2向聴のまま` の候補を Progress / Full の2つの scope で観測する。
///
/// production の判断は変えない。対象候補が無い request では Pass も Call も評価しない。
pub fn observe_two_shanten_stay_calls(
    context: &GameContext,
    legal_actions: &[LegalAction],
) -> TwoShantenStayCallObservation {
    observe_with_pass_evaluators(
        context,
        legal_actions,
        pass_two_shanten_progress_self_tsumo_value,
        pass_two_shanten_expected_self_tsumo_value,
    )
}

// Pass の評価器は request ごと・scope ごとに1回だけ呼ぶ。`FnOnce` なので候補ごとに呼び直せない。
fn observe_with_pass_evaluators(
    context: &GameContext,
    legal_actions: &[LegalAction],
    progress_pass: impl FnOnce(&GameContext) -> Option<u64> + Send,
    full_pass: impl FnOnce(&GameContext) -> Option<u64> + Send,
) -> TwoShantenStayCallObservation {
    let reaction_source_player = context.reaction_source_player();
    let Some((call, targets)) =
        evaluate_call_decision_with_two_shanten_stay_calls(context, legal_actions)
    else {
        return TwoShantenStayCallObservation {
            call: None,
            reaction_source_player,
            progress_pass: None,
            full_pass: None,
            candidates: Vec::new(),
        };
    };
    if targets.is_empty() {
        return TwoShantenStayCallObservation {
            call: Some(call),
            reaction_source_player,
            progress_pass: None,
            full_pass: None,
            candidates: Vec::new(),
        };
    }

    let reaction_source_known = reaction_draw_distance(context).is_some();
    let progress_pass = observe_pass(context, reaction_source_known, progress_pass);
    let full_pass = observe_pass(context, reaction_source_known, full_pass);

    let mut candidates: Vec<TwoShantenStayCallCandidate> = Vec::with_capacity(targets.len());
    for target in targets {
        let production = &call.candidates[target.candidate_index];
        let candidate = match &target.source {
            TwoShantenStayCallSource::Reused(source) => {
                let mut candidate = candidates
                    .iter()
                    .find(|candidate| candidate.candidate_index == *source)
                    .expect("先行候補も同じ鳴き後 state の観測対象")
                    .clone();
                candidate.candidate_index = target.candidate_index;
                candidate.action = production.action.clone();
                candidate.reused_from = Some(*source);
                candidate
            }
            TwoShantenStayCallSource::Evaluated(state) => {
                let progress = observe_progress_call(context, state);
                let full = observe_full_call(state);
                let compare = |call: TwoShantenStayCallValue, pass: TwoShantenStayCallValue| {
                    compare_call_pass_self_tsumo_values(
                        reaction_source_known,
                        call.known(),
                        pass.known(),
                        CallDecisionReason::EligibleTwoShantenSelfTsumo,
                    )
                    .0
                };
                TwoShantenStayCallCandidate {
                    candidate_index: target.candidate_index,
                    action: production.action.clone(),
                    kind: production.kind,
                    reused_from: None,
                    forbidden_discards: state.forbidden_discards.clone(),
                    post_call_fixed_meld_count: state.post_call_fixed_meld_count,
                    post_call_min_shanten: state.post_call_min_shanten,
                    progress_comparison: compare(progress.value, progress_pass.value),
                    full_comparison: compare(full.value, full_pass.value),
                    progress,
                    full,
                }
            }
        };
        candidates.push(candidate);
    }

    TwoShantenStayCallObservation {
        call: Some(call),
        reaction_source_player,
        progress_pass: Some(progress_pass),
        full_pass: Some(full_pass),
        candidates,
    }
}

// 反応元の席が分からない局面では production の Call / Pass 比較と同じく Pass を評価しない。
fn observe_pass(
    context: &GameContext,
    reaction_source_known: bool,
    evaluate: impl FnOnce(&GameContext) -> Option<u64> + Send,
) -> TwoShantenStayPassObservation {
    if !reaction_source_known {
        return TwoShantenStayPassObservation {
            value: TwoShantenStayCallValue::Unknown(
                TwoShantenStayCallUnknown::ReactionSourceUnknown,
            ),
            elapsed: None,
        };
    }
    let (value, elapsed) = measured_on_a_fresh_thread(|| {
        let started = Instant::now();
        let value = evaluate(context);
        (value, started.elapsed())
    });
    TwoShantenStayPassObservation {
        value: TwoShantenStayCallValue::from_option(value, TwoShantenStayCallUnknown::Unresolved),
        elapsed: Some(elapsed),
    }
}

// Progress 側の run 1本分。
struct ProgressRun {
    selected: Option<LegalAction>,
    value: TwoShantenStayCallValue,
    elapsed: Duration,
    search: ThreeShantenSearchStats,
    memo: SearchStateMemoStats,
}

fn observe_progress_call(
    context: &GameContext,
    state: &TwoShantenStayCallState,
) -> TwoShantenStayCallProgress {
    let observation = measured_on_a_fresh_thread(|| progress_run(context, state, true));
    let timing = measured_on_a_fresh_thread(|| progress_run(context, state, false));
    TwoShantenStayCallProgress {
        runs_agree: timing.selected == observation.selected && timing.value == observation.value,
        selected: timing.selected,
        value: timing.value,
        elapsed: timing.elapsed,
        search: observation.search,
        memo: observation.memo,
    }
}

fn progress_run(
    context: &GameContext,
    state: &TwoShantenStayCallState,
    search_stats: bool,
) -> ProgressRun {
    let started = Instant::now();
    let observed = select_two_shanten_progress_post_call_discard_observed(
        context,
        &state.post_call_tiles,
        &state.post_call_melds,
        &state.post_call_discards,
        search_stats,
    );
    let elapsed = started.elapsed();
    let (selected, value) = match observed.selection {
        Some(selection) => (
            preferred_dahai_action_for_type(
                &state.post_call_legal_actions,
                selection.evaluation.discard,
            )
            .cloned(),
            TwoShantenStayCallValue::from_option(
                selection.expected_self_tsumo_value,
                TwoShantenStayCallUnknown::Unresolved,
            ),
        ),
        None => (
            None,
            TwoShantenStayCallValue::Unknown(TwoShantenStayCallUnknown::NoPostCallSelection),
        ),
    };
    ProgressRun {
        selected,
        value,
        elapsed,
        search: observed.search,
        memo: observed.memo,
    }
}

// Full 側は production の2向聴 selection を production と同じ並列度で通す。計測 run と観測 run
// の分け方も fresh thread も既存の Full pair 比較と同じ入口を使う。
fn observe_full_call(state: &TwoShantenStayCallState) -> TwoShantenStayCallFull {
    let Some(post_call_context) = state.post_call_context.as_ref() else {
        return TwoShantenStayCallFull {
            selected: None,
            value: TwoShantenStayCallValue::Unknown(TwoShantenStayCallUnknown::NoPostCallSelection),
            progress_cohort: Vec::new(),
            full_evaluated: Vec::new(),
            elapsed: Duration::ZERO,
            search: ThreeShantenSearchStats::default(),
            memo: SearchStateMemoStats::default(),
            full_workers: 1,
            runs_agree: true,
        };
    };
    let decision = decide_with_two_shanten_full_parallelism(
        post_call_context,
        &state.post_call_legal_actions,
        TwoShantenFullParallelism::Parallel,
    );
    full_call_from_decision(&decision)
}

fn full_call_from_decision(decision: &TwoShantenFullParallelDecision) -> TwoShantenStayCallFull {
    let run = &decision.timing;
    let selected = run.selected.clone();
    let value = match (
        run.selected_discard(),
        run.candidates.iter().find(|candidate| candidate.selected),
    ) {
        (Some(discard), Some(candidate)) => match candidate.expected_self_tsumo_value {
            Some(value) => TwoShantenStayCallValue::Known(value),
            None => TwoShantenStayCallValue::Unknown(full_unknown_reason(
                discard,
                run.full_pair,
                !run.progress_evaluated_candidates().is_empty(),
            )),
        },
        _ => TwoShantenStayCallValue::Unknown(TwoShantenStayCallUnknown::NoPostCallSelection),
    };
    TwoShantenStayCallFull {
        selected,
        value,
        progress_cohort: run.progress_evaluated_candidates(),
        full_evaluated: run.full_pair.map(Vec::from).unwrap_or_default(),
        elapsed: decision.elapsed(),
        search: *decision.search(),
        memo: *decision.memo(),
        full_workers: decision.full_workers(),
        runs_agree: decision.runs_agree(),
    }
}

// 選ばれた打牌の Full 値が無い理由。production selection が実際に評価した範囲だけから読む。
fn full_unknown_reason(
    selected: TileType,
    full_pair: Option<[TileType; 2]>,
    progress_evaluated: bool,
) -> TwoShantenStayCallUnknown {
    match full_pair {
        Some(pair) if pair.contains(&selected) => TwoShantenStayCallUnknown::Unresolved,
        Some(_) => TwoShantenStayCallUnknown::SelectedOutsideFullPair,
        None if progress_evaluated => TwoShantenStayCallUnknown::FullGateNotFired,
        None => TwoShantenStayCallUnknown::NoCompetingTargets,
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use bot_logic::{
        HistoryFuritenFacts, TileId, best_two_shanten_progress_discard_among,
        two_shanten_progress_self_tsumo_value_for_candidate,
    };

    use super::*;
    use crate::agent::Agent;
    use crate::agents::ShantenAgent;
    use crate::call_decision::{
        CALL_TWO_SHANTEN_SHANTEN, TwoShantenStayCallTarget, evaluate_call_decision,
    };
    use crate::context::TableStateFacts;
    use crate::decision_timing::CallDecisionTimer;
    use crate::discard_selection::{
        LookaheadDiagnosticScope, lookahead_inputs, select_discard_action_with_evaluation,
        with_production_iishanten_continuation,
    };
    use crate::prospective_value::ProductionProspectiveValuator;

    // 11m 5p6p 88p 3s44s5s 77s8s。上家の 4s を Pon しても 3s5s で Chi しても2向聴のまま。
    const STAY_HAND: [u8; 13] = [0, 1, 53, 56, 64, 65, 80, 84, 85, 89, 96, 97, 100];
    const STAY_TARGET: u8 = 86;
    const STAY_PON_CONSUMED: [u8; 2] = [84, 85];
    const STAY_CHI_CONSUMED: [u8; 2] = [80, 89];
    // ドラ 9p。
    const STAY_DORA_INDICATOR: u8 = 66;

    // 123m 45m 79m 24p 9s EE N。上家の 3p を 2p4p で Chi すると1向聴へ進む。
    const REACH_HAND: [u8; 13] = [0, 4, 8, 12, 17, 24, 32, 40, 48, 104, 108, 109, 120];
    const REACH_TARGET: u8 = 44;
    const REACH_CHI_CONSUMED: [u8; 2] = [40, 48];

    // 11m 567m 55p6p 1s3s 6s7s F、ドラ F。上家の 8m を 6m7m で Chi しても2向聴のままで、鳴き後の
    // Progress 上位2候補 (F / 5p) はドラ枚数が違うので production の Full gate が発火する。
    const GATE_HAND: [u8; 13] = [0, 1, 17, 21, 25, 53, 54, 56, 72, 80, 92, 96, 128];
    const GATE_TARGET: u8 = 29;
    const GATE_CHI_CONSUMED: [u8; 2] = [21, 25];
    const GATE_DORA_INDICATOR: u8 = 124;

    const KAMICHA: u8 = 3;

    fn tile(id: u8) -> TileId {
        TileId::new(id).unwrap()
    }

    fn tiles(ids: &[u8]) -> Vec<TileId> {
        ids.iter().copied().map(tile).collect()
    }

    fn reaction_context(hand: &[u8], target: u8, source: Option<u8>) -> GameContext {
        reaction_context_with_dora(hand, target, source, STAY_DORA_INDICATOR)
    }

    fn reaction_context_with_dora(
        hand: &[u8],
        target: u8,
        source: Option<u8>,
        dora_indicator: u8,
    ) -> GameContext {
        let hand_tiles = tiles(hand);
        let mut visible = hand_tiles.clone();
        visible.push(tile(target));
        visible.push(tile(dora_indicator));
        GameContext::from_parts_with_melds(
            None,
            hand_tiles,
            vec![tile(dora_indicator)],
            TileType::new(27),
            TileType::new(28),
            visible,
            Some(0),
            Some(KAMICHA),
            [vec![], vec![], vec![], vec![tile(target)]],
            [false; 4],
            Default::default(),
        )
        .with_history_furiten_facts(HistoryFuritenFacts {
            same_turn: Some(false),
            riichi_missed_win: Some(false),
        })
        .with_reaction_source_player(source)
        .with_table_state_facts(TableStateFacts {
            remaining_tiles: Some(56),
            ..Default::default()
        })
    }

    fn stay_context() -> GameContext {
        reaction_context(&STAY_HAND, STAY_TARGET, Some(KAMICHA))
    }

    fn stay_actions() -> Vec<LegalAction> {
        vec![
            LegalAction::Pon {
                tile: tile(STAY_TARGET),
                consumed: tiles(&STAY_PON_CONSUMED),
            },
            LegalAction::Chi {
                tile: tile(STAY_TARGET),
                consumed: tiles(&STAY_CHI_CONSUMED),
            },
            LegalAction::None,
        ]
    }

    fn evaluated_state(target: &TwoShantenStayCallTarget) -> &TwoShantenStayCallState {
        match &target.source {
            TwoShantenStayCallSource::Evaluated(state) => state,
            TwoShantenStayCallSource::Reused(_) => panic!("先行候補の複製ではない"),
        }
    }

    fn production_call(ctx: &GameContext, actions: &[LegalAction]) -> CallDecisionDiagnostic {
        evaluate_call_decision(ctx, actions, false, &mut CallDecisionTimer::disabled())
            .expect("合法な Chi / Pon がある")
    }

    #[test]
    fn two_shanten_calls_that_stay_two_shanten_are_the_targets() {
        let ctx = stay_context();
        let (decision, targets) =
            evaluate_call_decision_with_two_shanten_stay_calls(&ctx, &stay_actions()).unwrap();

        assert_eq!(targets.len(), 2);
        for (target, kind) in targets.iter().zip([CallKind::Pon, CallKind::Chi]) {
            let candidate = &decision.candidates[target.candidate_index];
            assert_eq!(candidate.kind, kind);
            assert_eq!(candidate.current_shanten, Some(CALL_TWO_SHANTEN_SHANTEN));
            assert_eq!(
                candidate.post_call_shanten(),
                Some(CALL_TWO_SHANTEN_SHANTEN)
            );
            assert_eq!(candidate.reason, CallDecisionReason::PostCallNotIishanten);
            let state = evaluated_state(target);
            assert_eq!(state.post_call_min_shanten, CALL_TWO_SHANTEN_SHANTEN);
            assert_eq!(state.post_call_fixed_meld_count.get(), 1);
            assert_eq!(
                Some(&state.forbidden_discards),
                candidate.post_call_forbidden_discards.as_ref()
            );
            assert!(state.post_call_context.is_some());
            assert!(
                state
                    .post_call_legal_actions
                    .iter()
                    .all(|action| matches!(action, LegalAction::Dahai { tile }
                        if !state.forbidden_discards.contains(&tile.tile_type())))
            );
        }
        // 判断そのものは act() と同じ入口の結論。
        assert_eq!(decision, production_call(&ctx, &stay_actions()));
    }

    #[test]
    fn a_two_shanten_call_that_reaches_iishanten_is_not_a_target() {
        let ctx = reaction_context(&REACH_HAND, REACH_TARGET, Some(KAMICHA));
        let actions = vec![
            LegalAction::Chi {
                tile: tile(REACH_TARGET),
                consumed: tiles(&REACH_CHI_CONSUMED),
            },
            LegalAction::None,
        ];

        let observation = observe_two_shanten_stay_calls(&ctx, &actions);

        let call = observation.call.as_ref().unwrap();
        assert_eq!(call.candidates[0].current_shanten, Some(2));
        assert_eq!(call.candidates[0].post_call_shanten(), Some(1));
        assert!(!call.candidates[0].stays_two_shanten_after_call());
        assert!(!observation.has_targets());
        assert!(observation.progress_pass.is_none());
        assert!(observation.full_pass.is_none());
    }

    #[test]
    fn a_request_without_a_call_observes_nothing() {
        let ctx = stay_context();
        let evaluations = AtomicUsize::new(0);
        let count = |_: &GameContext| {
            evaluations.fetch_add(1, Ordering::Relaxed);
            None
        };

        let observation = observe_with_pass_evaluators(&ctx, &[LegalAction::None], count, count);

        assert!(observation.call.is_none());
        assert!(!observation.has_targets());
        assert_eq!(evaluations.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn the_pass_is_evaluated_once_per_request_and_scope() {
        let ctx = stay_context();
        let progress = AtomicUsize::new(0);
        let full = AtomicUsize::new(0);

        let observation = observe_with_pass_evaluators(
            &ctx,
            &stay_actions(),
            |ctx| {
                progress.fetch_add(1, Ordering::Relaxed);
                pass_two_shanten_progress_self_tsumo_value(ctx)
            },
            |ctx| {
                full.fetch_add(1, Ordering::Relaxed);
                pass_two_shanten_expected_self_tsumo_value(ctx)
            },
        );

        assert_eq!(observation.candidates.len(), 2);
        assert_eq!(progress.load(Ordering::Relaxed), 1);
        assert_eq!(full.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn the_progress_scope_reuses_the_existing_progress_helpers() {
        let ctx = stay_context();
        let observation = observe_two_shanten_stay_calls(&ctx, &stay_actions());
        let (_, targets) =
            evaluate_call_decision_with_two_shanten_stay_calls(&ctx, &stay_actions()).unwrap();

        let pass = observation.progress_pass.unwrap();
        assert_eq!(
            pass.value.known(),
            pass_two_shanten_progress_self_tsumo_value(&ctx)
        );
        assert!(pass.value.known().is_some());

        for (candidate, target) in observation.candidates.iter().zip(&targets) {
            let state = evaluated_state(target);
            let valuator = ProductionProspectiveValuator::new_with_hand_state(
                &ctx,
                Some(&state.post_call_melds),
            );
            let inputs = with_production_iishanten_continuation(lookahead_inputs(
                &ctx,
                &state.post_call_tiles,
                &valuator,
                LookaheadDiagnosticScope::None,
            ));
            let (index, value) =
                best_two_shanten_progress_discard_among(&inputs, &state.post_call_discards)
                    .unwrap();
            let evaluation = &state.post_call_discards[index];

            assert_eq!(candidate.progress.value.known(), value);
            assert_eq!(
                value,
                two_shanten_progress_self_tsumo_value_for_candidate(&inputs, evaluation)
            );
            assert_eq!(
                candidate.progress.selected.as_ref(),
                preferred_dahai_action_for_type(&state.post_call_legal_actions, evaluation.discard)
            );
            assert!(candidate.progress.runs_agree);
            assert!(candidate.progress.search.two_to_one_variants > 0);
        }
    }

    #[test]
    fn the_full_scope_reuses_the_production_two_shanten_selection_and_the_pass_full() {
        let ctx = stay_context();
        let observation = observe_two_shanten_stay_calls(&ctx, &stay_actions());
        let (_, targets) =
            evaluate_call_decision_with_two_shanten_stay_calls(&ctx, &stay_actions()).unwrap();

        assert_eq!(
            observation.full_pass.unwrap().value.known(),
            pass_two_shanten_expected_self_tsumo_value(&ctx)
        );
        // Full は Progress に SameShanten 枝の寄与を足した値。
        assert!(
            observation.full_pass.unwrap().value.known()
                >= observation.progress_pass.unwrap().value.known()
        );

        for (candidate, target) in observation.candidates.iter().zip(&targets) {
            let state = evaluated_state(target);
            let production = select_discard_action_with_evaluation(
                state.post_call_context.as_ref().unwrap(),
                &state.post_call_legal_actions,
            );
            assert_eq!(candidate.full.selected, production.action);
            assert!(candidate.full.runs_agree);
            // この局面ではドラ差 gate が発火しないので、選ばれた打牌の Full 値は存在しない。
            assert!(candidate.full.full_evaluated.is_empty());
            assert_eq!(
                candidate.full.value,
                TwoShantenStayCallValue::Unknown(TwoShantenStayCallUnknown::FullGateNotFired)
            );
            assert_eq!(candidate.full_comparison, CallIishantenComparison::Unknown);
            assert_eq!(
                candidate.comparison_agreement(),
                TwoShantenStayCallAgreement::Undetermined
            );
        }
    }

    #[test]
    fn the_full_scope_reports_the_value_the_gated_production_selection_established() {
        let ctx =
            reaction_context_with_dora(&GATE_HAND, GATE_TARGET, Some(KAMICHA), GATE_DORA_INDICATOR);
        let actions = vec![
            LegalAction::Chi {
                tile: tile(GATE_TARGET),
                consumed: tiles(&GATE_CHI_CONSUMED),
            },
            LegalAction::None,
        ];

        let observation = observe_two_shanten_stay_calls(&ctx, &actions);
        let (_, targets) =
            evaluate_call_decision_with_two_shanten_stay_calls(&ctx, &actions).unwrap();

        let [candidate] = observation.candidates.as_slice() else {
            panic!("{:?}", observation.candidates);
        };
        let state = evaluated_state(&targets[0]);
        let production = select_discard_action_with_evaluation(
            state.post_call_context.as_ref().unwrap(),
            &state.post_call_legal_actions,
        );
        let selected = |action: Option<&LegalAction>| match action {
            Some(LegalAction::Dahai { tile }) => Some(tile.tile_type().to_mjai_string()),
            _ => None,
        };

        assert_eq!(candidate.full.selected, production.action);
        assert_eq!(candidate.full.full_evaluated.len(), 2);
        assert!(matches!(
            candidate.full.value,
            TwoShantenStayCallValue::Known(_)
        ));
        assert_eq!(
            selected(candidate.progress.selected.as_ref()).as_deref(),
            Some("F")
        );
        assert_eq!(
            selected(candidate.full.selected.as_ref()).as_deref(),
            Some("5p")
        );
        assert_eq!(candidate.selected_discards_match(), Some(false));
        assert_eq!(
            candidate.full_comparison,
            CallIishantenComparison::PassNotLower
        );
        assert_eq!(
            candidate.progress_comparison,
            CallIishantenComparison::PassNotLower
        );
        assert_eq!(
            candidate.comparison_agreement(),
            TwoShantenStayCallAgreement::Same
        );
    }

    #[test]
    fn the_observation_keeps_the_production_call_decision() {
        let ctx = stay_context();
        let actions = stay_actions();
        let before = production_call(&ctx, &actions);
        let act_before = ShantenAgent.act(&ctx, &actions);

        let observation = observe_two_shanten_stay_calls(&ctx, &actions);

        assert_eq!(observation.call.as_ref(), Some(&before));
        for candidate in &observation.candidates {
            assert_eq!(
                before.candidates[candidate.candidate_index].reason,
                CallDecisionReason::PostCallNotIishanten
            );
        }
        assert_eq!(production_call(&ctx, &actions), before);
        assert_eq!(ShantenAgent.act(&ctx, &actions), act_before);
        assert_eq!(act_before, LegalAction::None);
        assert_eq!(observation.production_selected(), None);
    }

    #[test]
    fn an_unknown_reaction_source_keeps_both_comparisons_unknown_without_evaluating_the_pass() {
        let ctx = reaction_context(&STAY_HAND, STAY_TARGET, None);
        let evaluations = AtomicUsize::new(0);
        let count = |_: &GameContext| {
            evaluations.fetch_add(1, Ordering::Relaxed);
            None
        };

        let observation = observe_with_pass_evaluators(&ctx, &stay_actions(), count, count);

        assert!(observation.has_targets());
        assert_eq!(evaluations.load(Ordering::Relaxed), 0);
        let unknown =
            TwoShantenStayCallValue::Unknown(TwoShantenStayCallUnknown::ReactionSourceUnknown);
        for scope in [
            TwoShantenStayCallScope::Progress,
            TwoShantenStayCallScope::Full,
        ] {
            let pass = observation.pass(scope).unwrap();
            assert_eq!(pass.value, unknown);
            assert_eq!(pass.elapsed, None);
        }
        for candidate in &observation.candidates {
            assert_eq!(
                candidate.progress_comparison,
                CallIishantenComparison::Unknown
            );
            assert_eq!(candidate.full_comparison, CallIishantenComparison::Unknown);
        }
    }

    // 各 run は fresh thread の cold memo から始まるので、先に production の深い判断や observation
    // を走らせても、観測 run の探索規模は変わらない。
    #[test]
    fn every_run_starts_from_the_same_cold_memos() {
        let ctx = stay_context();
        let actions = stay_actions();
        let first = observe_two_shanten_stay_calls(&ctx, &actions);

        crate::shanten_diagnostic::diagnose_shanten_decision(&ctx, &actions);
        let second = observe_two_shanten_stay_calls(&ctx, &actions);

        for (first, second) in first.candidates.iter().zip(&second.candidates) {
            assert_eq!(first.progress.search, second.progress.search);
            assert_eq!(first.full.search, second.full.search);
            assert_eq!(
                first.progress.search.structural_evaluation_misses,
                second.progress.search.structural_evaluation_misses
            );
            assert_eq!(
                first.progress.memo.two_shanten_misses,
                second.progress.memo.two_shanten_misses
            );
            assert_eq!(
                first.progress.memo.iishanten_misses,
                second.progress.memo.iishanten_misses
            );
        }
    }
}
