//! 恒常フリテンが確定した現在聴牌 cohort について、継続 timing を観測するための診断層。
//!
//! ```text
//! current-tenpai cohort = AllPermanentFuriten
//! + candidate の既存 base offense mode = Reach
//!   → reach now / defer → forced Reach
//! ```
//!
//! 既存 [`crate::tenpai_continuation`] の1候補評価をそのまま呼ぶだけで、この層は探索も打点
//! 計算も待ち計算も持たない。向聴・受け入れ・次打牌・待ち・打点・役判定はすべて既存基盤の
//! 結論そのままで、継続 horizon も既存の「1ツモ → 1打牌 → 次の聴牌」から延ばさない。
//!
//! # 対象を AllPermanentFuriten だけにする理由
//!
//! cohort の分類は打牌選択が使うものと同じ
//! [`classify_current_tenpai_furiten_cohort`] が source of truth で、この層で判定し直さない。
//!
//! ```text
//! AllNonFuriten       → 従来どおり Ron offense weighted total の軸。対象外
//! MixedKnown          → Yes 側にロン機会が無く No 側にはある。ロン確率を持たないので対象外
//! Unknown             → 恒常フリテンを推測しない。対象外
//! AllPermanentFuriten → 全候補がロンできないので self-tsumo だけで揃う。対象
//! ```
//!
//! `PermanentFuriten::Yes` と `No` が混ざる cohort では、`No` 側にだけロン機会がある。両者を
//! self-tsumo だけで比べると `No` 側のロンを 0 として扱うことになるため、ロン確率を持たない
//! この層では比較しない。
//!
//! base offense mode が Damaten の候補 (`HighValueDamaten` / `NamedYakumanDamaten` など既存
//! policy がダマを選んだ候補) も対象にしない。この診断が比べるのは「base policy が選んだ
//! リーチを今宣言するか1巡 defer するか」であって、リーチとダマの選択そのものではない。
//!
//! # 観測値であること
//!
//! [`ReachTimingDiagnostic`] は既存 [`decide_permanent_furiten_reach_timing`] を診断のためだけ
//! に適用した結果で、[`bot_logic::DiscardComparisonReason`] にも selection metric にも
//! 接続していない。production の打牌選択・リーチ判断・リーチ timing はこの層を参照せず、
//! 診断の有無で選択結果は変わらない。確定しない値は 0 点にせず `None` のままにする。
//!
//! # 構築する経路
//!
//! 1候補につき既存2手先評価を1回走らせる重い経路なので、診断を要求した経路だけで構築する。
//! 通常の `act()` / 打牌選択が全候補分を構築することはなく、既存 current-tenpai comparator の
//! 計算量も変わらない。

use bot_logic::{
    CurrentTenpaiFuritenCohort, CurrentTenpaiMetrics, DiscardEvaluation, TileType,
    classify_current_tenpai_furiten_cohort,
};

use crate::context::GameContext;
use crate::offense_value::TenpaiOffenseMode;
use crate::reach_policy::{
    ReachTimingDecision, ReachTimingDiagnostic, decide_permanent_furiten_reach_timing,
};
use crate::tenpai_continuation::{
    TenpaiSelfTsumoComparison, tenpai_candidate_self_tsumo_comparison,
};

// テンパイの向聴数。
const TENPAI_SHANTEN: i8 = 0;

/// 恒常フリテン確定 cohort の現在聴牌候補について観測した継続 timing。
///
/// `candidates` は対象になった候補だけを、既存打牌評価と同じ順序で並べる。対象が1件も無い
/// 局面では空になる。打牌選択にもリーチ判断にも使わない解析専用の情報。
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CurrentTenpaiContinuationDiagnostic {
    pub candidates: Vec<CurrentTenpaiContinuationCandidate>,
}

impl CurrentTenpaiContinuationDiagnostic {
    pub fn candidate(&self, discard: TileType) -> Option<&CurrentTenpaiContinuationCandidate> {
        self.candidates
            .iter()
            .find(|candidate| candidate.discard == discard)
    }
}

/// 現在聴牌候補1件分の継続 timing 観測。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CurrentTenpaiContinuationCandidate {
    /// 現在の打牌候補の牌種。この打牌後が現在聴牌になる。
    pub discard: TileType,
    /// 既存 [`crate::tenpai_continuation`] が求めた self-tsumo 比較そのもの。
    pub self_tsumo: TenpaiSelfTsumoComparison,
    /// 既存 timing policy を診断のためだけに適用した結果。selection には接続しない。
    pub timing: ReachTimingDiagnostic,
}

impl CurrentTenpaiContinuationCandidate {
    /// 今すぐリーチして手変わりせず、残り自摸機会全体でツモ和了する期待支払い。
    pub fn reach_now(&self) -> Option<u64> {
        self.self_tsumo.reach_now
    }

    /// 1巡 defer し、次のテンパイで合法ならリーチする期待ツモ支払い。
    pub fn defer_forced_reach(&self) -> Option<u64> {
        self.self_tsumo.defer_forced_reach()
    }

    /// 既存 timing policy を適用した場合の self-tsumo value。
    ///
    /// [`ReachTimingDecision::ReachNow`] なら `reach now`、
    /// [`ReachTimingDecision::DeferReach`] なら `defer → forced Reach` の値そのもの。比較不能
    /// で base policy のリーチを維持した場合は 0 点にせず `None`。
    pub fn timing_self_tsumo_value(&self) -> Option<u64> {
        match self.timing.decision {
            ReachTimingDecision::ReachNow => self.reach_now(),
            ReachTimingDecision::DeferReach => self.defer_forced_reach(),
        }
    }
}

/// 継続 timing を観測するための材料。
///
/// `evaluations` / `metrics` / `offense_modes` は打牌選択が既に構築・使用した値そのもので、
/// 同じ候補集合から作った同じ順序のものを渡す。
pub(crate) struct CurrentTenpaiContinuationInputs<'a> {
    pub context: &'a GameContext,
    pub evaluations: &'a [DiscardEvaluation],
    /// 打牌選択が cohort 単位の軸解決へ渡すものと同じ現在聴牌 metric。
    pub metrics: &'a [CurrentTenpaiMetrics],
    /// 候補ごとの既存 base offense mode。現在聴牌の評価対象外は `None`。
    pub offense_modes: &'a [Option<TenpaiOffenseMode>],
    /// 現在局面の合法手に [`LegalAction::Reach`](crate::action::LegalAction::Reach) があるか。
    pub reach_legal: bool,
}

/// 対象候補について `reach now` と `defer → forced Reach` を観測する。
///
/// 対象は AllPermanentFuriten cohort かつ base offense mode が
/// [`TenpaiOffenseMode::Reach`] の現在聴牌候補だけ。それ以外の候補では継続比較そのものを
/// 構築しない。
pub(crate) fn diagnose_current_tenpai_continuation(
    inputs: &CurrentTenpaiContinuationInputs,
) -> CurrentTenpaiContinuationDiagnostic {
    CurrentTenpaiContinuationDiagnostic {
        candidates: continuation_targets(inputs.evaluations, inputs.metrics, inputs.offense_modes)
            .filter_map(|index| {
                let evaluation = &inputs.evaluations[index];
                let self_tsumo = tenpai_candidate_self_tsumo_comparison(
                    inputs.context,
                    evaluation,
                    inputs.reach_legal,
                )?;
                Some(CurrentTenpaiContinuationCandidate {
                    discard: evaluation.discard,
                    timing: decide_permanent_furiten_reach_timing(
                        self_tsumo.reach_now,
                        self_tsumo.defer_forced_reach(),
                    ),
                    self_tsumo,
                })
            })
            .collect(),
    }
}

// 継続比較を構築する候補の index。
//
// cohort の分類は打牌選択と同じ pure classification をそのまま使い、候補の組ごとに判定を
// 変えない。base offense mode も既存 policy が決めた値そのもので、ここでリーチ・ダマを
// 決め直さない。
fn continuation_targets<'a>(
    evaluations: &'a [DiscardEvaluation],
    metrics: &'a [CurrentTenpaiMetrics],
    offense_modes: &'a [Option<TenpaiOffenseMode>],
) -> impl Iterator<Item = usize> + 'a {
    (0..evaluations.len()).filter(move |&index| {
        evaluations[index].min_shanten_after_discard() == TENPAI_SHANTEN
            && classify_current_tenpai_furiten_cohort(evaluations, metrics, index)
                == CurrentTenpaiFuritenCohort::AllPermanentFuriten
            && offense_modes.get(index).copied().flatten() == Some(TenpaiOffenseMode::Reach)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::sync::LazyLock;

    use bot_logic::{HistoryFuritenFacts, TileId};

    use crate::action::LegalAction;
    use crate::context::TableStateFacts;
    use crate::discard_selection::{
        DiscardActionSelectionWithDiagnostic, LookaheadDiagnosticScope,
        select_discard_action_with_diagnostic, select_discard_action_with_evaluation,
    };
    use crate::reach_policy::ReachTimingReason;

    // 234m 678m 789p 99s 34s + 6s。打 6s で 2s / 5s 待ち、打 3s で 4s6s の 5s 待ちになり、
    // どちらも現在聴牌の候補になる。打 4s は 3s / 6s が浮いて1向聴。
    const HAND: [&str; 13] = [
        "2m", "3m", "4m", "6m", "7m", "8m", "7p", "8p", "9p", "9s", "9s", "3s", "4s",
    ];
    const DRAW: &str = "6s";

    // 山の残枚数。4人で分けて自分の残り自摸機会になる。
    const REMAINING_TILES: u32 = 70;
    // リーチ宣言の条件を満たす持ち点。
    const REACH_SCORE: i32 = 25_000;

    struct CaseSpec<'a> {
        /// 自分の河。待ち牌を含めると恒常フリテンになる。
        own_river: &'a [&'a str],
        /// 合法手にリーチを含めるか。含めなければ base offense mode はダマになる。
        legal_reach: bool,
        /// 山の残枚数。`None` では self-tsumo 確率模型の材料が揃わない。
        remaining_tiles: Option<u32>,
        scope: LookaheadDiagnosticScope,
    }

    impl Default for CaseSpec<'_> {
        fn default() -> Self {
            Self {
                own_river: &["5s"],
                legal_reach: true,
                remaining_tiles: Some(REMAINING_TILES),
                scope: LookaheadDiagnosticScope::Lookahead,
            }
        }
    }

    struct CaseContext {
        context: GameContext,
        actions: Vec<LegalAction>,
    }

    // 同じ牌種の物理牌を取り違えないよう、1枚ずつ払い出す。
    struct TileIdSource {
        used: [bool; TileId::COUNT],
    }

    impl TileIdSource {
        fn new() -> Self {
            Self {
                used: [false; TileId::COUNT],
            }
        }

        fn tiles(&mut self, strings: &[&str]) -> Vec<TileId> {
            strings.iter().map(|s| self.tile(s)).collect()
        }

        fn tile(&mut self, s: &str) -> TileId {
            let id = TileId::copies(tile(s))
                .find(|id| !id.is_red() && !self.used[id.index()])
                .expect("同じ物理牌を使い回していない");
            self.used[id.index()] = true;
            id
        }
    }

    fn tile(s: &str) -> TileType {
        TileType::from_mjai_type_str(s).expect("牌種として読める")
    }

    impl CaseSpec<'_> {
        fn build(&self) -> DiscardActionSelectionWithDiagnostic {
            let case = self.context();
            select_discard_action_with_diagnostic(&case.context, &case.actions, self.scope)
        }

        fn context(&self) -> CaseContext {
            let mut source = TileIdSource::new();
            let hand_tiles = source.tiles(&HAND);
            let drawn_tile = source.tile(DRAW);
            let own_river = source.tiles(self.own_river);

            let visible: Vec<TileId> = hand_tiles
                .iter()
                .chain([&drawn_tile])
                .chain(own_river.iter())
                .copied()
                .collect();
            let actions: Vec<LegalAction> = hand_tiles
                .iter()
                .chain([&drawn_tile])
                .map(|&tile| LegalAction::Dahai { tile })
                .chain(self.legal_reach.then_some(LegalAction::Reach))
                .collect();

            let mut discards: [Vec<TileId>; 4] = Default::default();
            discards[0] = own_river;

            let context = GameContext::from_parts_with_melds(
                Some(drawn_tile),
                hand_tiles,
                Vec::new(),
                Some(tile("E")),
                Some(tile("S")),
                visible,
                Some(0),
                Some(3),
                discards,
                [false; 4],
                Default::default(),
            )
            .with_table_state_facts(TableStateFacts {
                remaining_tiles: self.remaining_tiles,
                scores: Some([REACH_SCORE; 4]),
                ..Default::default()
            })
            .with_history_furiten_facts(HistoryFuritenFacts {
                same_turn: Some(false),
                riichi_missed_win: Some(false),
            });

            CaseContext { context, actions }
        }
    }

    // 2手先探索は重いので、同じ局面を使う複数のテストで構築結果を共有する。
    static FURITEN_CASE: LazyLock<DiscardActionSelectionWithDiagnostic> =
        LazyLock::new(|| CaseSpec::default().build());

    fn continuation(
        selection: &DiscardActionSelectionWithDiagnostic,
    ) -> &CurrentTenpaiContinuationDiagnostic {
        selection
            .current_tenpai_continuation
            .as_ref()
            .expect("診断を要求した経路では構築されている")
    }

    #[test]
    fn an_all_permanent_furiten_reach_cohort_observes_both_current_tenpai_candidates() {
        // 打 6s (2s / 5s 待ち) と打 3s (5s 待ち) はどちらも 5s が自分の河にあって恒常フリテン。
        // base policy はどちらもリーチを選ぶので、両方の継続比較を観測できる。
        let selection = &*FURITEN_CASE;
        let candidates = &continuation(selection).candidates;

        assert_eq!(candidates.len(), 2, "{candidates:?}");
        for discard in ["6s", "3s"] {
            let candidate = continuation(selection)
                .candidate(tile(discard))
                .unwrap_or_else(|| panic!("打 {discard} を観測している"));

            assert!(candidate.reach_now().is_some(), "{candidate:?}");
            assert!(candidate.defer_forced_reach().is_some(), "{candidate:?}");
            assert_eq!(
                candidate.timing.reason,
                ReachTimingReason::PermanentFuritenSelfTsumo
            );
            // 既存 timing policy を適用した場合の値は、その決定側の self-tsumo value そのもの。
            let expected = match candidate.timing.decision {
                ReachTimingDecision::ReachNow => candidate.reach_now(),
                ReachTimingDecision::DeferReach => candidate.defer_forced_reach(),
            };
            assert_eq!(candidate.timing_self_tsumo_value(), expected);
        }
    }

    #[test]
    fn the_selected_candidate_and_the_runner_up_are_both_observed() {
        // 選択済み候補だけでなく、比較で負けた候補の reach now / defer も観測できる。
        let selection = &*FURITEN_CASE;
        let selected = selection
            .selection
            .evaluation
            .as_ref()
            .expect("現在聴牌候補が選ばれている");
        let runner_up = continuation(selection)
            .candidates
            .iter()
            .find(|candidate| candidate.discard != selected.discard)
            .expect("比較で負けた候補も観測している");

        assert!(
            continuation(selection)
                .candidate(selected.discard)
                .is_some()
        );
        assert!(runner_up.reach_now().is_some(), "{runner_up:?}");
        assert!(runner_up.defer_forced_reach().is_some(), "{runner_up:?}");
    }

    #[test]
    fn diagnostics_do_not_change_the_selected_discard() {
        let case = CaseSpec::default().context();
        let without = select_discard_action_with_evaluation(&case.context, &case.actions);

        assert_eq!(FURITEN_CASE.selection, without);
        assert!(!continuation(&FURITEN_CASE).candidates.is_empty());
    }

    #[test]
    fn a_mixed_cohort_is_not_evaluated() {
        // 打 6s (2s / 5s 待ち) だけが恒常フリテンで、打 3s (5s 待ち) はロンできる。No 側の
        // ロン機会を self-tsumo だけで比べないため、継続比較そのものを構築しない。
        let selection = CaseSpec {
            own_river: &["2s"],
            ..CaseSpec::default()
        }
        .build();

        assert_eq!(continuation(&selection).candidates, Vec::new());
    }

    #[test]
    fn a_base_damaten_candidate_is_not_evaluated() {
        // 合法手にリーチが無ければ base offense mode はダマ。リーチ timing の比較対象にしない。
        let selection = CaseSpec {
            legal_reach: false,
            ..CaseSpec::default()
        }
        .build();

        assert_eq!(continuation(&selection).candidates, Vec::new());
    }

    #[test]
    fn an_unresolvable_comparison_is_not_treated_as_zero() {
        // 山の残枚数が分からないと self-tsumo 確率模型の材料が揃わない。対象からは外さず、
        // 比較不能を 0 点と混同しない。
        let selection = CaseSpec {
            remaining_tiles: None,
            ..CaseSpec::default()
        }
        .build();
        let candidates = &continuation(&selection).candidates;

        assert_eq!(candidates.len(), 2, "{candidates:?}");
        for candidate in candidates {
            assert_eq!(candidate.reach_now(), None);
            assert_eq!(candidate.defer_forced_reach(), None);
            assert_eq!(candidate.timing_self_tsumo_value(), None);
            assert_eq!(
                candidate.timing.reason,
                ReachTimingReason::SelfTsumoComparisonUnknown
            );
        }
    }

    #[test]
    fn the_continuation_is_not_built_without_the_diagnostic_scope() {
        let selection = CaseSpec {
            scope: LookaheadDiagnosticScope::None,
            ..CaseSpec::default()
        }
        .build();

        assert_eq!(selection.current_tenpai_continuation, None);
    }
}
