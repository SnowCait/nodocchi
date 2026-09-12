//! 1向聴の手変わり深度を、production の打牌 comparator を通した最終選択として比較する。
//!
//! A は production へ追加深度を接続する前の旧設定 (legacy shallow depth) で、手変わりは1回まで
//! (`Progress` と `SameShanten -> Progress`)。B は現在の production depth で、
//! `SameShanten -> SameShanten -> Progress` をもう1段だけ許し、追加深度に必要な exact
//! same-state memo を有効にする。段数は2回で閉じていて、任意深度の再帰へは一般化しない。
//!
//! [`crate::iishanten_continuation_depth_comparison`] が全1向聴候補の
//! ExpectedSelfTsumoValue を単独 ranking として並べるのに対し、この module は
//! `Shanten → IsolatedTile → IsolatedHonor → ExpectedSelfTsumoValue` の既存 comparator を
//! そのまま通した最終打牌を比べる。production は全候補を深く評価せず、pre-acceptance 軸まで
//! 同順位の cohort ([`bot_logic::forward_target_mask`]) だけを深く探索するため、深度を上げた
//! ときの実際の selection cost はこちらでしか分からない。
//!
//! 観測は既存の production 経路を1回通すだけで、候補の絞り込み・unknown の軸解決・比較順・
//! 安定順序・最終選択はどれも既存 helper をそのまま使う。この module は comparator を複製せず、
//! 特定の牌や役に固有の処理も持たない。
//!
//! # 計測条件
//!
//! 方式ごとに run を2本に分ける。時間は production selection に無い観測コストを一切含まない
//! run から取る。
//!
//! - 観測 run ([`IishantenSelectionDepthDecision::observation`]): 探索規模の計上と phase timer
//!   を有効にする。cohort・ExpectedSelfTsumoValue・search / memo / phase の stats はこの run
//!   のもの。
//! - 計測 run ([`IishantenSelectionDepthDecision::timing`]): 探索規模の計上も phase timer も
//!   持たない。`elapsed` はこの run のもの。観測 run の後に走らせるので、実際の対局と同じく
//!   process が暖まった状態の値になる。
//!
//! 2本の run は同じ入力に対する同じ純粋な探索なので、選んだ打牌も cohort も値も一致する
//! ([`IishantenSelectionDepthDecision::runs_agree`])。
//!
//! B は追加深度と一緒に探索内の同一 state memo
//! ([`bot_logic::LookaheadInputs::with_search_state_memo`]) も有効にするため、A → B の elapsed
//! 差は深度だけの差ではない。同じ memo 条件へ揃えた純粋な深度比較は
//! [`crate::iishanten_continuation_depth_comparison`] が全候補評価として持っているので、ここでは
//! 重複して持たない。
//!
//! 向聴・受け入れ・一向聴形の memo は thread ごとに持つため、同じ thread で続けて評価すると後
//! から走った run が暖まった memo を使ってしまう。方式ごとの計測 run と観測 run はそれぞれ新しい
//! thread で行い、どの run も同じ cold な thread-local から始める。探索する枝も評価値も選択も、
//! 計測 thread の違いでは変わらない。
//!
//! どちらの方式も深い候補評価は逐次で行う。この module が比べるのは深度だけで、production が
//! 使う候補単位の並列評価は [`crate::iishanten_selection_parallel_comparison`] が同じ B depth
//! の中だけで比べる。並列評価は値を変えないので、B の値も選択も production のものと一致する。

use std::time::Duration;

use bot_logic::{DiscardComparisonReason, SearchStateMemoStats, ThreeShantenSearchStats, TileType};

use crate::action::LegalAction;
use crate::context::GameContext;
use crate::decision_timing::{
    IishantenForwardCandidateDuration, NormalDiscardPhaseDurations, NormalDiscardPhaseTimer,
};
use crate::discard_selection::{
    IishantenContinuationSelection, IishantenContinuationSettings,
    select_discard_action_with_iishanten_continuation_settings,
};

/// 比較する2方式。段数が違えば経路確率も違うため、どちらの深度で選んだ打牌かを必ず添える。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IishantenSelectionDepth {
    /// A: production 接続前の旧設定。手変わりは1回まで、探索内 memo も旧 production の判断の
    /// まま。比較 baseline としてだけ残る。
    LegacyShallow,
    /// B: 現行 production の深度。手変わりを2回まで許し、追加深度に必要な exact same-state
    /// memo を有効にする。
    Production,
}

impl IishantenSelectionDepth {
    pub const BOTH: [Self; 2] = [Self::LegacyShallow, Self::Production];

    pub fn label(self) -> &'static str {
        match self {
            Self::LegacyShallow => "A legacy shallow depth (same-shanten once, pre-production)",
            Self::Production => "B production depth (same-shanten twice + exact same-state memo)",
        }
    }

    // 探索そのものの設定。差し替えるのは深度と、それに必要な memo だけで、候補の絞り込みも
    // 比較順も最終選択も production と同じ経路をそのまま通る。計測 run と観測 run はこの同じ
    // 設定から作るので、探索する枝は2本の run で同じになる。
    //
    // B は production の深度と memo をそのまま持つが、深い候補評価は逐次で行う。並列評価は値を
    // 変えないので、選択も候補の値も production と一致する。
    pub(crate) fn continuation(self) -> IishantenContinuationSettings {
        match self {
            Self::LegacyShallow => IishantenContinuationSettings::LEGACY_SHALLOW,
            Self::Production => IishantenContinuationSettings::PRODUCTION_SEQUENTIAL,
        }
    }

    // 計測 run。探索規模の計上も phase timer も持たない。
    fn timing_settings(self) -> IishantenContinuationSettings {
        IishantenContinuationSettings {
            search_stats: false,
            ..self.continuation()
        }
    }

    // 観測 run。探索規模を計上する。
    fn observation_settings(self) -> IishantenContinuationSettings {
        IishantenContinuationSettings {
            search_stats: true,
            ..self.continuation()
        }
    }
}

/// 打牌候補1件について、production selection が実際に使った値と比較結果。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IishantenSelectionDepthCandidate {
    pub discard: TileType,
    pub shanten_after_discard: i8,
    pub selected: bool,
    /// pre-acceptance cohort に残り、深い前方評価の対象になった候補か。
    pub deep_evaluated: bool,
    /// 深い前方評価が実際に確定させた ExpectedSelfTsumoValue。cohort 単位の軸解決を通す前の値。
    pub evaluated_expected_self_tsumo_value: Option<u64>,
    /// 選択が比較に使った ExpectedSelfTsumoValue。cohort に unknown があって軸ごと落ちた場合も
    /// `None` になる。
    pub expected_self_tsumo_value: Option<u64>,
    /// 選ばれた候補がこの候補を上回った理由。選ばれた候補自身は
    /// [`DiscardComparisonReason::StableOrder`]。
    pub comparison_reason: DiscardComparisonReason,
    pub selected_is_strictly_better: bool,
}

/// 1局面を1方式で1回選択した run。計測 run と観測 run は同じ型で、instrumentation の有無だけが
/// 違う。
#[derive(Debug, Clone)]
pub struct IishantenSelectionDepthRun {
    pub selected: Option<LegalAction>,
    /// 全合法候補。順序は既存 selection の候補順そのもの。
    pub candidates: Vec<IishantenSelectionDepthCandidate>,
    /// 診断の構築を含まない、打牌選択1回の実測時間。
    pub elapsed: Duration,
    /// phase 別の内訳。phase timer を持たない計測 run では 0 のまま。
    pub phases: NormalDiscardPhaseDurations,
    /// 深い前方評価を実際に行った1向聴候補ごとの実測。phase timer を持たない計測 run と、
    /// 最善向聴数が1向聴でない局面では空。並行に評価した run では、候補の `elapsed` の合計が
    /// `phases.forward_metrics` の壁時計を超える。
    pub iishanten_forward_candidates: Vec<IishantenForwardCandidateDuration>,
    /// 探索規模。計上しない計測 run では 0 のまま。
    pub search: ThreeShantenSearchStats,
    /// 探索内の同一 state memo の利用数。memo を持たない方式では 0 のまま。
    pub memo: SearchStateMemoStats,
    /// 深い候補評価に実際に使った thread 数。逐次評価では 1。
    pub forward_workers: usize,
}

impl IishantenSelectionDepthRun {
    pub fn selected_discard(&self) -> Option<TileType> {
        match self.selected {
            Some(LegalAction::Dahai { tile }) => Some(tile.tile_type()),
            _ => None,
        }
    }

    pub fn candidate(&self, discard: TileType) -> Option<&IishantenSelectionDepthCandidate> {
        self.candidates
            .iter()
            .find(|candidate| candidate.discard == discard)
    }

    /// 深い前方評価の対象になった候補。production の候補絞り込みが残した cohort そのもの。
    pub fn deep_evaluated_candidates(&self) -> Vec<TileType> {
        self.candidates
            .iter()
            .filter(|candidate| candidate.deep_evaluated)
            .map(|candidate| candidate.discard)
            .collect()
    }

    /// ExpectedSelfTsumoValue を確定できた候補数。
    pub fn evaluated_expected_self_tsumo_value_count(&self) -> usize {
        self.candidates
            .iter()
            .filter(|candidate| candidate.evaluated_expected_self_tsumo_value.is_some())
            .count()
    }

    /// 選択が実際に ExpectedSelfTsumoValue を比較に使えた候補数。軸ごと落ちた cohort は数えない。
    pub fn compared_expected_self_tsumo_value_count(&self) -> usize {
        self.candidates
            .iter()
            .filter(|candidate| candidate.expected_self_tsumo_value.is_some())
            .count()
    }

    /// 選ばれた候補の ExpectedSelfTsumoValue。
    pub fn selected_expected_self_tsumo_value(&self) -> Option<u64> {
        self.candidates
            .iter()
            .find(|candidate| candidate.selected)
            .and_then(|candidate| candidate.evaluated_expected_self_tsumo_value)
    }

    /// 選ばれた候補が他候補を上回った理由。候補ごとに違い得るので候補単位で持つ。
    pub fn comparison_reasons(&self) -> Vec<(TileType, DiscardComparisonReason)> {
        self.candidates
            .iter()
            .filter(|candidate| !candidate.selected)
            .map(|candidate| (candidate.discard, candidate.comparison_reason))
            .collect()
    }
}

/// 1局面を1方式で選択した結果。時間は計測 run から、探索の内訳は観測 run から取る。
#[derive(Debug, Clone)]
pub struct IishantenSelectionDepthDecision {
    pub depth: IishantenSelectionDepth,
    /// instrumentation を持たない計測 run。`elapsed` はこの run のもので、production selection
    /// が実際に払うコストだけを含む。
    pub timing: IishantenSelectionDepthRun,
    /// 探索規模の計上と phase timer を有効にした観測 run。cohort・値・search / memo / phase の
    /// stats はこの run のもの。
    pub observation: IishantenSelectionDepthRun,
}

impl IishantenSelectionDepthDecision {
    /// instrumentation を含まない打牌選択1回の実測時間。
    pub fn elapsed(&self) -> Duration {
        self.timing.elapsed
    }

    pub fn selected(&self) -> Option<&LegalAction> {
        self.timing.selected.as_ref()
    }

    pub fn selected_discard(&self) -> Option<TileType> {
        self.timing.selected_discard()
    }

    /// 計測 run と観測 run が同じ選択・同じ cohort・同じ候補の値になったか。
    ///
    /// 2本の run は同じ入力に対する同じ純粋な探索なので必ず一致する。instrumentation が探索も
    /// 選択も変えていないことの確認として持つ。
    pub fn runs_agree(&self) -> bool {
        self.timing.selected == self.observation.selected
            && self.timing.candidates == self.observation.candidates
    }
}

// 計測を新しい thread で行う。向聴・受け入れ・一向聴形の memo は thread-local なので、thread を
// 分ければ先に走った run が後の run の memo を暖めることがない。探索する枝も評価値も選択も、この
// thread の違いでは変わらない。
pub(crate) fn measured_on_a_fresh_thread<T: Send>(measure: impl FnOnce() -> T + Send) -> T {
    std::thread::scope(|scope| {
        scope
            .spawn(measure)
            .join()
            .expect("計測 thread は panic しない")
    })
}

/// 指定した深度で production selection を計測 run と観測 run の2回行う。
///
/// 深度と、それに必要な memo 以外は production の打牌選択と同じ経路を1回ずつ通る。計測 run は
/// 探索規模の計上も phase timer も持たないので、その `elapsed` には production selection に無い
/// 観測コストが入らない。観測 run はそれらを有効にして cohort・値・stats を取る。
///
/// どちらの run も [`measured_on_a_fresh_thread`] の中で行うため、先に走った run が後の run の
/// thread-local memo を暖めることはない。探索する枝も評価値も選択も run の順に依らない。
///
/// 観測 run を先に走らせる。thread-local memo は fresh thread なのでどちらの run も cold から
/// 始まるが、process 全体の暖まり (allocator・code page・CPU) は run をまたいで残る。実際の
/// 対局では process が暖まった状態で1手ずつ選ぶので、その条件で計った方の値を `elapsed` に
/// 使う。
pub fn decide_with_iishanten_selection_depth(
    context: &GameContext,
    legal_actions: &[LegalAction],
    depth: IishantenSelectionDepth,
) -> IishantenSelectionDepthDecision {
    let observation = measured_on_a_fresh_thread(|| {
        run_on_the_measuring_thread(
            context,
            legal_actions,
            depth.observation_settings(),
            NormalDiscardPhaseTimer::started(),
        )
    });
    let timing = measured_on_a_fresh_thread(|| {
        run_on_the_measuring_thread(
            context,
            legal_actions,
            depth.timing_settings(),
            NormalDiscardPhaseTimer::disabled(),
        )
    });
    IishantenSelectionDepthDecision {
        depth,
        timing,
        observation,
    }
}

pub(crate) fn run_on_the_measuring_thread(
    context: &GameContext,
    legal_actions: &[LegalAction],
    continuation: IishantenContinuationSettings,
    timing: NormalDiscardPhaseTimer,
) -> IishantenSelectionDepthRun {
    let observed = select_discard_action_with_iishanten_continuation_settings(
        context,
        legal_actions,
        continuation,
        timing,
    );
    IishantenSelectionDepthRun {
        selected: observed.selection.action.clone(),
        candidates: candidates_from_observation(&observed),
        elapsed: observed.elapsed,
        phases: observed.phases,
        iishanten_forward_candidates: observed.iishanten_forward_candidates.clone(),
        search: observed.search,
        memo: observed.memo,
        forward_workers: observed.forward_workers,
    }
}

// 候補の値も比較理由も選択が使ったものそのままで、表示のために比較をやり直さない。構築は選択が
// 終わってからなので、run の実測時間には入らない。
pub(crate) fn candidates_from_observation(
    observed: &IishantenContinuationSelection,
) -> Vec<IishantenSelectionDepthCandidate> {
    observed
        .diagnostic
        .candidates
        .iter()
        .enumerate()
        .map(|(index, candidate)| IishantenSelectionDepthCandidate {
            discard: candidate.evaluation.discard,
            shanten_after_discard: candidate.evaluation.min_shanten_after_discard(),
            selected: candidate.selected,
            deep_evaluated: observed
                .forward_targets
                .get(index)
                .copied()
                .unwrap_or_default(),
            evaluated_expected_self_tsumo_value: observed
                .forward
                .get(index)
                .and_then(|metrics| metrics.expected_self_tsumo_value),
            expected_self_tsumo_value: candidate.expected_self_tsumo_value,
            comparison_reason: candidate.comparison_reason,
            selected_is_strictly_better: candidate.selected_is_strictly_better_than_candidate,
        })
        .collect()
}

/// 同じ局面を A / B で1回ずつ選択した比較結果。
#[derive(Debug, Clone)]
pub struct IishantenSelectionDepthComparison {
    /// A: production 接続前の旧設定。比較 baseline。
    pub legacy: IishantenSelectionDepthDecision,
    /// B: 現行 production の深度。
    pub production: IishantenSelectionDepthDecision,
}

impl IishantenSelectionDepthComparison {
    /// A / B が同じ打牌を選んだか。
    pub fn selects_the_same_discard(&self) -> bool {
        self.legacy.selected() == self.production.selected()
    }

    /// 深い前方評価の対象になった候補が A / B で同じか。候補の絞り込みは深度に依らないため、
    /// 同じ局面では必ず一致する。
    pub fn shares_the_deep_evaluated_candidates(&self) -> bool {
        self.legacy.observation.deep_evaluated_candidates()
            == self.production.observation.deep_evaluated_candidates()
    }

    /// A / B のどちらも計測 run と観測 run で同じ選択になったか。
    pub fn runs_agree(&self) -> bool {
        self.legacy.runs_agree() && self.production.runs_agree()
    }

    /// B / A の比。A が 0 の場合は比を作れない。
    pub fn slowdown(&self) -> Option<f64> {
        let legacy = self.legacy.elapsed().as_secs_f64();
        (legacy > 0.0).then(|| self.production.elapsed().as_secs_f64() / legacy)
    }
}

/// 同じ局面について A / B の打牌選択を1回ずつ行う。
///
/// 評価順は A → B の固定だが、どちらも自分専用の thread で計るため、先に走った方式が後の方式の
/// thread-local memo を暖めることはない。値も選択も評価順に依らない。
pub fn compare_iishanten_selection_depths(
    context: &GameContext,
    legal_actions: &[LegalAction],
) -> IishantenSelectionDepthComparison {
    IishantenSelectionDepthComparison {
        legacy: decide_with_iishanten_selection_depth(
            context,
            legal_actions,
            IishantenSelectionDepth::LegacyShallow,
        ),
        production: decide_with_iishanten_selection_depth(
            context,
            legal_actions,
            IishantenSelectionDepth::Production,
        ),
    }
}

/// 調査対象の1向聴局面。深度 A/B と候補並列の診断はどちらも同じ fixture を使う。
#[cfg(test)]
pub(crate) mod test_support {
    use super::*;
    use crate::context::{GameContext, TableStateFacts};
    use crate::shanten_test_support::{dahai, tile};
    use bot_logic::{HistoryFuritenFacts, TileId};

    // 1向聴局面 34567899m5799p34s。ドラ表示 3m / 場風 E / 自風 N / player 0 / oya 1 /
    // remaining 66 / 履歴フリテンなしで、bot-scenario の inline baseline と同じ facts になる。
    // 赤5を持たない物理牌を選ぶ。
    const HAND: [u8; 14] = [8, 12, 17, 20, 24, 28, 32, 33, 53, 60, 68, 69, 80, 84];
    const DORA_INDICATOR: u8 = 9;

    pub(crate) fn iishanten_context() -> (GameContext, Vec<LegalAction>) {
        let hand: Vec<TileId> = HAND.iter().map(|&value| tile(value)).collect();
        let dora_indicator = tile(DORA_INDICATOR);
        let visible: Vec<_> = hand.iter().copied().chain([dora_indicator]).collect();
        let context = GameContext::from_parts_with_table_state(
            None,
            hand,
            vec![dora_indicator],
            TileType::new(27),
            TileType::new(30),
            visible,
            Some(0),
            Some(1),
            Default::default(),
            [false; 4],
        )
        .with_table_state_facts(TableStateFacts {
            remaining_tiles: Some(66),
            ..Default::default()
        })
        .with_history_furiten_facts(HistoryFuritenFacts {
            same_turn: Some(false),
            riichi_missed_win: Some(false),
        });
        // production の打牌選択と同じく全合法打牌を候補にする。深く評価される候補は既存の
        // 候補絞り込みが決める。
        let actions = HAND.iter().map(|&value| dahai(value)).collect();
        (context, actions)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::iishanten_selection_depth_comparison::test_support::iishanten_context;
    use crate::shanten_test_support::tile;

    #[test]
    fn the_production_comparator_narrows_the_fixture_to_its_pre_acceptance_cohort() {
        // 候補の絞り込みは深度に依らない既存 comparator の pre-acceptance 軸そのもの。2向聴に
        // 落ちる候補は深い前方評価の対象にならず、1向聴を維持する候補だけが cohort に残る。
        let (context, actions) = iishanten_context();
        let comparison = compare_iishanten_selection_depths(&context, &actions);

        let cohort = ["3m", "6m", "9m", "5p", "7p", "9p", "3s", "4s"];
        for decision in [&comparison.legacy, &comparison.production] {
            // 計測 run と観測 run のどちらも同じ cohort を残す。
            for run in [&decision.timing, &decision.observation] {
                let deep: Vec<_> = run
                    .deep_evaluated_candidates()
                    .iter()
                    .map(|discard| discard.to_mjai_string())
                    .collect();
                assert_eq!(deep, cohort, "{}", decision.depth.label());
                assert_eq!(run.evaluated_expected_self_tsumo_value_count(), 8);
                // cohort の全候補で値が確定したので、軸は解決後も残る。
                assert_eq!(run.compared_expected_self_tsumo_value_count(), 8);
                for candidate in &run.candidates {
                    assert_eq!(
                        candidate.deep_evaluated,
                        candidate.shanten_after_discard == 1,
                        "{}",
                        candidate.discard.to_mjai_string(),
                    );
                }
            }
        }
        assert!(comparison.shares_the_deep_evaluated_candidates());
    }

    #[test]
    fn the_extra_depth_changes_the_selected_discard_of_the_fixture() {
        // 既存 comparator を通しても、A は 9p を選び B は 5p を選ぶ。どちらも
        // ExpectedSelfTsumoValue 軸で決着し、軸単独の値は既知の A/B と一致する。
        let (context, actions) = iishanten_context();
        let comparison = compare_iishanten_selection_depths(&context, &actions);

        let five_pin = tile(53).tile_type();
        let nine_pin = tile(68).tile_type();
        assert_eq!(comparison.legacy.selected_discard(), Some(nine_pin), "A");
        assert_eq!(
            comparison.production.selected_discard(),
            Some(five_pin),
            "B"
        );
        assert!(!comparison.selects_the_same_discard());

        let value = |decision: &IishantenSelectionDepthDecision, discard| {
            decision
                .observation
                .candidate(discard)
                .expect("候補がある")
                .evaluated_expected_self_tsumo_value
        };
        assert_eq!(value(&comparison.legacy, five_pin), Some(697_451_162));
        assert_eq!(value(&comparison.legacy, nine_pin), Some(697_475_278));
        assert_eq!(value(&comparison.production, five_pin), Some(1_031_805_837));
        assert_eq!(value(&comparison.production, nine_pin), Some(989_272_961));

        assert_eq!(
            comparison
                .legacy
                .observation
                .selected_expected_self_tsumo_value(),
            Some(697_475_278),
        );
        assert_eq!(
            comparison
                .production
                .observation
                .selected_expected_self_tsumo_value(),
            Some(1_031_805_837),
        );

        // 選ばれた候補が cohort の他候補を上回った理由は、どちらの深度でも同じ軸。
        for decision in [&comparison.legacy, &comparison.production] {
            for (discard, reason) in decision.observation.comparison_reasons() {
                let candidate = decision.observation.candidate(discard).expect("候補がある");
                let expected = if candidate.deep_evaluated {
                    DiscardComparisonReason::ExpectedSelfTsumoValue
                } else {
                    DiscardComparisonReason::Shanten
                };
                assert_eq!(reason, expected, "{}", discard.to_mjai_string());
                assert!(candidate.selected_is_strictly_better);
            }
        }
    }

    #[test]
    fn the_extra_depth_searches_more_and_only_it_uses_the_exact_memo() {
        // B は A の枝を残したまま手変わり2回の経路を足すので探索規模は増える。exact memo は B
        // だけの設定で、旧設定の A は memo を持たない。
        let (context, actions) = iishanten_context();
        let comparison = compare_iishanten_selection_depths(&context, &actions);

        let legacy = &comparison.legacy.observation;
        let production = &comparison.production.observation;
        assert!(production.search.draw_variants > legacy.search.draw_variants);
        assert!(production.search.terminal_scorings > legacy.search.terminal_scorings);

        assert_eq!(legacy.memo.next_discard_hits, 0);
        assert_eq!(legacy.memo.next_discard_misses, 0);
        assert!(production.memo.next_discard_hits > 0);
        assert!(production.memo.same_shanten_next_discard_hits > 0);

        // 計測 run は探索規模を計上しない。memo の利用数は memo 自体が持つ値なので、memo を
        // 有効にした方式では計測 run にも残る。
        for decision in [&comparison.legacy, &comparison.production] {
            assert_eq!(
                decision.timing.search,
                ThreeShantenSearchStats::default(),
                "{}",
                decision.depth.label(),
            );
            assert_eq!(
                decision.timing.phases,
                NormalDiscardPhaseDurations::default(),
                "{}",
                decision.depth.label(),
            );
            assert!(decision.observation.phases.forward_metrics > Duration::ZERO);
        }
    }

    #[test]
    fn the_timing_run_and_the_observation_run_select_the_same_discard() {
        // instrumentation は探索も選択も変えない。時間を計測 run から、内訳を観測 run から
        // 取っても、両者が指す選択は同じものになる。
        let (context, actions) = iishanten_context();
        let comparison = compare_iishanten_selection_depths(&context, &actions);

        assert!(comparison.runs_agree());
        for decision in [&comparison.legacy, &comparison.production] {
            let label = decision.depth.label();
            assert!(decision.runs_agree(), "{label}");
            assert_eq!(
                decision.timing.selected, decision.observation.selected,
                "{label}",
            );
            assert_eq!(
                decision.timing.deep_evaluated_candidates(),
                decision.observation.deep_evaluated_candidates(),
                "{label}",
            );
            assert_eq!(
                decision.timing.selected_expected_self_tsumo_value(),
                decision.observation.selected_expected_self_tsumo_value(),
                "{label}",
            );
        }
    }

    #[test]
    fn the_production_depth_selects_what_the_production_discard_selection_selects() {
        // B は現行 production selection そのもの。production は同じ深度を候補単位の並列評価で
        // 通すが、並列評価は値を変えないので、逐次で通した B と選択も候補ごとの値も比較理由も
        // bit-exact に一致する。
        let (context, actions) = iishanten_context();
        let sequential = decide_with_iishanten_selection_depth(
            &context,
            &actions,
            IishantenSelectionDepth::Production,
        );
        let production = crate::discard_selection::select_discard_action(&context, &actions);
        assert_eq!(sequential.timing.selected, production);
        assert_eq!(sequential.observation.selected, production);

        let parallel = measured_on_a_fresh_thread(|| {
            run_on_the_measuring_thread(
                &context,
                &actions,
                crate::discard_selection::production_iishanten_continuation_settings(),
                NormalDiscardPhaseTimer::disabled(),
            )
        });
        assert_eq!(parallel.selected, production);
        assert_eq!(parallel.candidates, sequential.timing.candidates);

        // 実 worker 数は深く評価する候補数も runtime の並列度も超えない。
        let cohort = sequential.observation.deep_evaluated_candidates().len();
        assert!(parallel.forward_workers >= 1);
        assert!(
            parallel.forward_workers <= cohort,
            "{}",
            parallel.forward_workers
        );
        assert!(
            parallel.forward_workers <= crate::discard_selection::available_parallelism(),
            "{}",
            parallel.forward_workers,
        );

        // 対象 fixture の production 打牌は 5p で、cohort の値は B のもの。
        let five_pin = tile(53).tile_type();
        let nine_pin = tile(68).tile_type();
        assert_eq!(parallel.selected_discard(), Some(five_pin));
        let value = |discard| {
            parallel
                .candidate(discard)
                .expect("候補がある")
                .evaluated_expected_self_tsumo_value
        };
        assert_eq!(value(five_pin), Some(1_031_805_837));
        assert_eq!(value(nine_pin), Some(989_272_961));
    }
}
