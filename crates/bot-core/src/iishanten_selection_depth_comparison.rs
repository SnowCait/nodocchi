//! 1向聴の手変わり深度 A/B を、production の打牌 comparator を通した最終選択として観測する。
//!
//! A は現行 production の打牌選択そのもので、手変わりは1回まで
//! (`Progress` と `SameShanten -> Progress`)。B は `SameShanten -> SameShanten -> Progress` を
//! もう1段だけ許した診断専用の追加深度で、任意深度の再帰へは一般化しない。
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
//! A は current production configuration、B は proposed depth + exact memo configuration。
//! B は追加深度と一緒に探索内の同一 state memo
//! ([`bot_logic::LookaheadInputs::with_search_state_memo`]) も有効にするため、A → B の elapsed
//! 差は深度だけの差ではない。同じ memo 条件へ揃えた純粋な深度比較は
//! [`crate::iishanten_continuation_depth_comparison`] が全候補評価として持っているので、ここでは
//! 重複して持たない。
//!
//! 向聴・受け入れ・一向聴形の memo は thread ごとに持つため、同じ thread で A → B と続けて
//! 評価すると後から走った方式が暖まった memo を使ってしまう。方式ごとの実測は既存 A/B 計測と
//! 同じく新しい thread で行い、どちらも同じ cold な thread-local から始める。探索する枝も
//! 評価値も選択も、計測 thread の違いでは変わらない。
//!
//! production の打牌選択は A のままで、この module は B を production selection へ接続しない。

use std::time::Duration;

use bot_logic::{
    DiscardComparisonReason, SameShantenContinuationDepth, SearchStateMemoStats,
    ThreeShantenSearchStats, TileType,
};

use crate::action::LegalAction;
use crate::context::GameContext;
use crate::decision_timing::NormalDiscardPhaseDurations;
use crate::discard_selection::{
    IishantenContinuationSelection, IishantenContinuationSettings,
    select_discard_action_with_iishanten_continuation_settings,
};

/// 比較する2方式。段数が違えば経路確率も違うため、どちらの深度で選んだ打牌かを必ず添える。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IishantenSelectionDepth {
    /// A: 現行 production。手変わりは1回まで、探索内 memo も production の判断のまま。
    Production,
    /// B: 診断専用。手変わりを2回まで許し、追加深度に必要な exact memo を有効にする。
    TwiceWithExactMemo,
}

impl IishantenSelectionDepth {
    pub const BOTH: [Self; 2] = [Self::Production, Self::TwiceWithExactMemo];

    pub fn label(self) -> &'static str {
        match self {
            Self::Production => "A production depth (same-shanten once)",
            Self::TwiceWithExactMemo => "B same-shanten twice + exact same-state memo",
        }
    }

    // 差し替えるのは深度と、それに必要な memo / 探索規模の計上だけ。候補の絞り込みも比較順も
    // 最終選択も production と同じ経路をそのまま通る。
    fn settings(self) -> IishantenContinuationSettings {
        match self {
            Self::Production => IishantenContinuationSettings {
                search_stats: true,
                ..IishantenContinuationSettings::PRODUCTION
            },
            Self::TwiceWithExactMemo => IishantenContinuationSettings {
                depth: SameShantenContinuationDepth::Twice,
                search_state_memo: true,
                search_stats: true,
            },
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

/// 1局面を1方式で選択した結果の実測。
#[derive(Debug, Clone)]
pub struct IishantenSelectionDepthDecision {
    pub depth: IishantenSelectionDepth,
    pub selected: Option<LegalAction>,
    /// 全合法候補。順序は既存 selection の候補順そのもの。
    pub candidates: Vec<IishantenSelectionDepthCandidate>,
    /// 診断の構築を含まない、打牌選択1回の実測時間。
    pub elapsed: Duration,
    pub phases: NormalDiscardPhaseDurations,
    pub search: ThreeShantenSearchStats,
    /// 探索内の同一 state memo の利用数。memo を持たない方式では 0 のまま。
    pub memo: SearchStateMemoStats,
}

impl IishantenSelectionDepthDecision {
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

// 計測を新しい thread で行う。向聴・受け入れ・一向聴形の memo は thread-local なので、thread を
// 分ければ先に走った方式が後の方式の memo を暖めることがない。探索する枝も評価値も選択も、この
// thread の違いでは変わらない。
fn measured_on_a_fresh_thread<T: Send>(measure: impl FnOnce() -> T + Send) -> T {
    std::thread::scope(|scope| {
        scope
            .spawn(measure)
            .join()
            .expect("計測 thread は panic しない")
    })
}

/// 指定した深度で production selection を1回行い、選ばれた打牌と実測時間を返す。
///
/// 深度と、それに必要な memo / 探索規模の計上以外は production の打牌選択と同じ経路を1回ずつ
/// 通る。計測は [`measured_on_a_fresh_thread`] の中で行うため、同じ局面を続けて評価しても前の
/// 方式の thread-local memo は引き継がない。
pub fn decide_with_iishanten_selection_depth(
    context: &GameContext,
    legal_actions: &[LegalAction],
    depth: IishantenSelectionDepth,
) -> IishantenSelectionDepthDecision {
    measured_on_a_fresh_thread(|| decide_on_the_measuring_thread(context, legal_actions, depth))
}

fn decide_on_the_measuring_thread(
    context: &GameContext,
    legal_actions: &[LegalAction],
    depth: IishantenSelectionDepth,
) -> IishantenSelectionDepthDecision {
    let observed = select_discard_action_with_iishanten_continuation_settings(
        context,
        legal_actions,
        depth.settings(),
    );
    IishantenSelectionDepthDecision {
        depth,
        selected: observed.selection.action.clone(),
        candidates: candidates_from_observation(&observed),
        elapsed: observed.elapsed,
        phases: observed.phases,
        search: observed.search,
        memo: observed.memo,
    }
}

// 候補の値も比較理由も選択が使ったものそのままで、表示のために比較をやり直さない。
fn candidates_from_observation(
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
    pub production: IishantenSelectionDepthDecision,
    pub twice: IishantenSelectionDepthDecision,
}

impl IishantenSelectionDepthComparison {
    /// A / B が同じ打牌を選んだか。
    pub fn selects_the_same_discard(&self) -> bool {
        self.production.selected == self.twice.selected
    }

    /// 深い前方評価の対象になった候補が A / B で同じか。候補の絞り込みは深度に依らないため、
    /// 同じ局面では必ず一致する。
    pub fn shares_the_deep_evaluated_candidates(&self) -> bool {
        self.production.deep_evaluated_candidates() == self.twice.deep_evaluated_candidates()
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
        production: decide_with_iishanten_selection_depth(
            context,
            legal_actions,
            IishantenSelectionDepth::Production,
        ),
        twice: decide_with_iishanten_selection_depth(
            context,
            legal_actions,
            IishantenSelectionDepth::TwiceWithExactMemo,
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context::{GameContext, TableStateFacts};
    use crate::shanten_test_support::{dahai, tile};
    use bot_logic::{HistoryFuritenFacts, TileId};

    // 調査対象の1向聴局面 34567899m5799p34s。ドラ表示 3m / 場風 E / 自風 N / player 0 / oya 1 /
    // remaining 66 / 履歴フリテンなしで、bot-scenario の inline baseline と同じ facts になる。
    // 赤5を持たない物理牌を選ぶ。
    const HAND: [u8; 14] = [8, 12, 17, 20, 24, 28, 32, 33, 53, 60, 68, 69, 80, 84];
    const DORA_INDICATOR: u8 = 9;

    fn iishanten_context() -> (GameContext, Vec<LegalAction>) {
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

    #[test]
    fn the_production_comparator_narrows_the_fixture_to_its_pre_acceptance_cohort() {
        // 候補の絞り込みは深度に依らない既存 comparator の pre-acceptance 軸そのもの。2向聴に
        // 落ちる候補は深い前方評価の対象にならず、1向聴を維持する候補だけが cohort に残る。
        let (context, actions) = iishanten_context();
        let comparison = compare_iishanten_selection_depths(&context, &actions);

        let cohort = ["3m", "6m", "9m", "5p", "7p", "9p", "3s", "4s"];
        for decision in [&comparison.production, &comparison.twice] {
            let deep: Vec<_> = decision
                .deep_evaluated_candidates()
                .iter()
                .map(|discard| discard.to_mjai_string())
                .collect();
            assert_eq!(deep, cohort, "{}", decision.depth.label());
            assert_eq!(decision.evaluated_expected_self_tsumo_value_count(), 8);
            // cohort の全候補で値が確定したので、軸は解決後も残る。
            assert_eq!(decision.compared_expected_self_tsumo_value_count(), 8);
            for candidate in &decision.candidates {
                assert_eq!(
                    candidate.deep_evaluated,
                    candidate.shanten_after_discard == 1,
                    "{}",
                    candidate.discard.to_mjai_string(),
                );
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
        assert_eq!(
            comparison.production.selected_discard(),
            Some(nine_pin),
            "A",
        );
        assert_eq!(comparison.twice.selected_discard(), Some(five_pin), "B");
        assert!(!comparison.selects_the_same_discard());

        let value = |decision: &IishantenSelectionDepthDecision, discard| {
            decision
                .candidate(discard)
                .expect("候補がある")
                .evaluated_expected_self_tsumo_value
        };
        assert_eq!(value(&comparison.production, five_pin), Some(697_451_162));
        assert_eq!(value(&comparison.production, nine_pin), Some(697_475_278));
        assert_eq!(value(&comparison.twice, five_pin), Some(1_031_805_837));
        assert_eq!(value(&comparison.twice, nine_pin), Some(989_272_961));

        assert_eq!(
            comparison.production.selected_expected_self_tsumo_value(),
            Some(697_475_278),
        );
        assert_eq!(
            comparison.twice.selected_expected_self_tsumo_value(),
            Some(1_031_805_837),
        );

        // 選ばれた候補が cohort の他候補を上回った理由は、どちらの深度でも同じ軸。
        for decision in [&comparison.production, &comparison.twice] {
            for (discard, reason) in decision.comparison_reasons() {
                let candidate = decision.candidate(discard).expect("候補がある");
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
        // だけの設定で、A は production のまま memo を持たない。
        let (context, actions) = iishanten_context();
        let comparison = compare_iishanten_selection_depths(&context, &actions);

        assert!(comparison.twice.search.draw_variants > comparison.production.search.draw_variants);
        assert!(
            comparison.twice.search.terminal_scorings
                > comparison.production.search.terminal_scorings
        );

        assert_eq!(comparison.production.memo.next_discard_hits, 0);
        assert_eq!(comparison.production.memo.next_discard_misses, 0);
        assert!(comparison.twice.memo.next_discard_hits > 0);
        assert!(comparison.twice.memo.same_shanten_next_discard_hits > 0);
    }

    #[test]
    fn the_production_depth_selects_what_the_production_discard_selection_selects() {
        // A は現行 production selection そのもの。既存の打牌選択と同じ action を返す。
        let (context, actions) = iishanten_context();
        let decision = decide_with_iishanten_selection_depth(
            &context,
            &actions,
            IishantenSelectionDepth::Production,
        );
        assert_eq!(
            decision.selected,
            crate::discard_selection::select_discard_action(&context, &actions),
        );
    }
}
