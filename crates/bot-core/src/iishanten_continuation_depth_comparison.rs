//! 1向聴 ExpectedSelfTsumoValue の手変わり深度 A/B 比較。
//!
//! A (same-shanten once) は production へ追加深度を接続する前の1向聴 continuation で、
//! `Progress` と `SameShanten -> Progress` までを追う。B (same-shanten twice) は現在の
//! production depth で、`SameShanten -> SameShanten -> Progress` をもう1段だけ許す。段数は2回で
//! 閉じていて、任意深度の再帰へは一般化しない。
//!
//! 違いは1向聴 state で [`bot_logic::DrawTransition::SameShanten`] を2回まで許すかどうかだけで、
//! ツモ牌の列挙・残枚数・物理牌 variant・ツモ後の最良打牌の比較・テンパイ到達後の terminal
//! scoring・Reach / Damaten・確率・残り自摸機会・unknown 伝播はどちらも同じ primitive を通る。
//!
//! この module は比較のための計測だけを行う。表示する順位は ExpectedSelfTsumoValue 単独の
//! ranking で、production の最終打牌選択ではない。production は
//! `Shanten → IsolatedTile → IsolatedHonor → ExpectedSelfTsumoValue` の順に既存 comparator を
//! 通し、pre-acceptance 軸まで同順位の cohort の中だけでこの値を比べ、その cohort に unknown が
//! 1件でもあれば軸ごと落とす。この module はその comparator を複製しない。
//!
//! # 計測条件
//!
//! 向聴・受け入れ・一向聴形の memo は thread ごとに持つため、同じ thread で A → B と続けて
//! 評価すると後から走った方式が暖まった memo を使ってしまう。方式ごとの実測は必ず新しい thread
//! で行い、どちらの方式も同じ cold な thread-local から始める。探索する枝も評価値も、計測
//! thread の違いでは変わらない。
//!
//! 探索内の同一 state memo ([`bot_logic::LookaheadInputs::with_search_state_memo`]) は A / B の
//! どちらにも同じように有効化する。共有するのは同じ入力なら必ず同じ値になる純関数の結果だけ
//! なので値は変わらず、A → B の差が深度そのものの差だけになる。旧 production の1向聴打牌選択は
//! この memo を有効にしていなかったため、A の実測は旧 production の latency そのものではない。
//! B も全1向聴候補を単独で評価するので、production の cohort 絞り込みを通した実測は
//! [`crate::iishanten_selection_depth_comparison`] と
//! [`crate::iishanten_selection_parallel_comparison`] が持つ。

use std::time::{Duration, Instant};

use bot_logic::{
    DiscardEvaluation, DrawTransition, LookaheadInputs, SameShantenContinuationDepth,
    SearchStateMemoStats, ThreeShantenSearchStats, TileType, diagnose_lookahead_candidate,
    forward_metrics_for_candidate,
};

use crate::action::LegalAction;
use crate::context::GameContext;
use crate::discard_selection::{
    LookaheadDiagnosticScope, legal_discard_evaluations, lookahead_inputs,
};
use crate::prospective_value::ProductionProspectiveValuator;

const IISHANTEN_SHANTEN: i8 = 1;

/// 比較する2方式。段数が違えば経路確率も違うため、どちらの深度で評価した値かを必ず添える。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IishantenContinuationDepth {
    /// A: production 接続前の旧深度。手変わりは1回まで (`SameShanten -> Progress`)。
    Once,
    /// B: 現行 production の深度。手変わりを2回まで
    /// (`SameShanten -> SameShanten -> Progress`)。
    Twice,
}

impl IishantenContinuationDepth {
    pub const BOTH: [Self; 2] = [Self::Once, Self::Twice];

    pub fn label(self) -> &'static str {
        match self {
            Self::Once => "A legacy shallow depth (Progress, SameShanten -> Progress)",
            Self::Twice => "B production depth (+ SameShanten -> SameShanten -> Progress)",
        }
    }

    fn depth(self) -> SameShantenContinuationDepth {
        match self {
            Self::Once => SameShantenContinuationDepth::Once,
            Self::Twice => SameShantenContinuationDepth::Twice,
        }
    }
}

/// 候補1件の ExpectedSelfTsumoValue を、最初のツモ1牌種ごとに分けた内訳。
///
/// 値は集計本体そのもので、全枝分を足すと候補の値と一致する。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FirstDrawContribution {
    pub draw: TileType,
    pub transition: DrawTransition,
    pub remaining: u8,
    pub value: Option<u64>,
}

/// 候補1件分の内訳。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CandidateBreakdown {
    pub discard: TileType,
    pub draws: Vec<FirstDrawContribution>,
}

impl CandidateBreakdown {
    pub fn value(&self, draw: TileType) -> Option<Option<u64>> {
        self.draws
            .iter()
            .find(|contribution| contribution.draw == draw)
            .map(|contribution| contribution.value)
    }
}

/// 全合法1向聴候補を1方式で評価した値・実測時間と、探索規模。
#[derive(Debug, Clone)]
pub struct IishantenContinuationDepthProfile {
    pub depth: IishantenContinuationDepth,
    pub search: ThreeShantenSearchStats,
    /// 同一 state memo の利用数。どちらの深度も同じ memo を有効にして評価する。
    pub memo: SearchStateMemoStats,
    /// unknown は `None` のまま保持する。
    pub candidates: Vec<(TileType, Option<u64>, Duration)>,
    /// 候補ごとの最初のツモ1牌種単位の内訳。計測後に別途構築する。
    pub breakdown: Vec<CandidateBreakdown>,
    /// 入力構築を除く全候補の評価時間。内訳の構築は含まない。
    pub total: Duration,
}

impl IishantenContinuationDepthProfile {
    pub fn breakdown_of(&self, discard: TileType) -> Option<&CandidateBreakdown> {
        self.breakdown
            .iter()
            .find(|breakdown| breakdown.discard == discard)
    }

    pub fn value(&self, discard: TileType) -> Option<Option<u64>> {
        self.candidates
            .iter()
            .find(|(candidate, ..)| *candidate == discard)
            .map(|&(_, value, _)| value)
    }

    /// ExpectedSelfTsumoValue の高い順に並べた候補。unknown は値の順へ混ぜず末尾へ回す。
    ///
    /// これはこの軸単独の ranking で、production の最終打牌選択ではない。production は
    /// `Shanten → IsolatedTile → IsolatedHonor → ExpectedSelfTsumoValue` の順に既存
    /// comparator を通し、pre-acceptance 軸まで同順位の cohort の中だけでこの値を比べ、その
    /// cohort に unknown が1件でもあれば軸ごと落とす。この診断はその絞り込みも軸解決も持たない。
    pub fn ranked(&self) -> Vec<(TileType, Option<u64>)> {
        let mut ranked: Vec<_> = self
            .candidates
            .iter()
            .map(|&(discard, value, _)| (discard, value))
            .collect();
        ranked.sort_by_key(|&(_, value)| (value.is_none(), std::cmp::Reverse(value)));
        ranked
    }

    /// [`Self::ranked`] の1位、つまり ExpectedSelfTsumoValue 単独で最も高い候補。
    ///
    /// production が選ぶ打牌ではない。この診断は production selection を呼ばず、
    /// [`Self::ranked`] が説明する pre-acceptance cohort の絞り込みも unknown の軸解決も
    /// 持たない。
    pub fn top_expected_self_tsumo_value_candidate(&self) -> Option<(TileType, u64)> {
        self.ranked()
            .into_iter()
            .find_map(|(discard, value)| value.map(|value| (discard, value)))
    }
}

// 計測を新しい thread で行う。向聴・受け入れ・一向聴形の memo は thread-local なので、thread を
// 分ければ先に走った方式が後の方式の memo を暖めることがない。探索する枝も評価値もこの thread
// の違いでは変わらない。
fn measured_on_a_fresh_thread<T: Send>(measure: impl FnOnce() -> T + Send) -> T {
    std::thread::scope(|scope| {
        scope
            .spawn(measure)
            .join()
            .expect("計測 thread は panic しない")
    })
}

/// 通常打牌と同じ入力・valuator で全1向聴候補を1方式で評価し、探索規模も計上する。
///
/// production selection は呼ばない。計上の有無で値も枝も変わらない。
pub fn profile_iishanten_continuation_depth(
    context: &GameContext,
    legal_actions: &[LegalAction],
    depth: IishantenContinuationDepth,
) -> IishantenContinuationDepthProfile {
    measured_on_a_fresh_thread(|| profile_on_the_measuring_thread(context, legal_actions, depth))
}

fn profile_on_the_measuring_thread(
    context: &GameContext,
    legal_actions: &[LegalAction],
    depth: IishantenContinuationDepth,
) -> IishantenContinuationDepthProfile {
    let legal = legal_discard_evaluations(context, legal_actions);
    let valuator = ProductionProspectiveValuator::new(context);
    let inputs = lookahead_inputs(
        context,
        &legal.tiles,
        &valuator,
        LookaheadDiagnosticScope::None,
    )
    .with_same_shanten_continuation_depth(depth.depth())
    .with_search_state_memo()
    .with_three_shanten_search_stats();
    let started = Instant::now();
    let targets: Vec<_> = legal
        .evaluations
        .iter()
        .filter(|evaluation| evaluation.min_shanten_after_discard() == IISHANTEN_SHANTEN)
        .collect();
    let candidates: Vec<_> = targets
        .iter()
        .map(|evaluation| candidate_value(&inputs, evaluation))
        .collect();
    // 計測は値だけの経路で終える。内訳はこの後に構築するので、時間にも探索規模にも入らない。
    let total = started.elapsed();
    let search = inputs.three_shanten_search_stats();
    let memo = inputs.search_state_memo_stats();
    let breakdown = targets
        .iter()
        .map(|evaluation| candidate_breakdown(&inputs, evaluation))
        .collect();
    IishantenContinuationDepthProfile {
        depth,
        search,
        memo,
        candidates,
        breakdown,
        total,
    }
}

// 候補1件の内訳。値の集計本体は候補全体の値と共有し、内訳のために別の計算を持たない。
fn candidate_breakdown(
    inputs: &LookaheadInputs,
    evaluation: &DiscardEvaluation,
) -> CandidateBreakdown {
    let candidate = diagnose_lookahead_candidate(inputs, evaluation);
    let facts = inputs.self_tsumo_facts();
    CandidateBreakdown {
        discard: evaluation.discard,
        draws: candidate
            .draws
            .iter()
            .map(|draw| FirstDrawContribution {
                draw: draw.draw,
                transition: draw.transition,
                remaining: draw.remaining,
                value: facts.and_then(|facts| draw.self_tsumo_value(facts)),
            })
            .collect(),
    }
}

fn candidate_value(
    inputs: &LookaheadInputs,
    evaluation: &DiscardEvaluation,
) -> (TileType, Option<u64>, Duration) {
    let started = Instant::now();
    let value = forward_metrics_for_candidate(inputs, evaluation).expected_self_tsumo_value;
    (evaluation.discard, value, started.elapsed())
}

/// 同じ局面を A / B で1回ずつ評価した比較結果。
#[derive(Debug, Clone)]
pub struct IishantenContinuationDepthComparison {
    pub once: IishantenContinuationDepthProfile,
    pub twice: IishantenContinuationDepthProfile,
}

impl IishantenContinuationDepthComparison {
    /// A / B の ExpectedSelfTsumoValue ranking の1位が同じ候補か。値が1つも確定しない場合は
    /// 比較そのものが `None`。
    ///
    /// production が同じ打牌を選ぶかどうかではない。比べているのは
    /// [`IishantenContinuationDepthProfile::top_expected_self_tsumo_value_candidate`] 同士で、
    /// production の既存 comparator は通していない。
    pub fn shares_the_top_expected_self_tsumo_value_candidate(&self) -> Option<bool> {
        Some(
            self.once.top_expected_self_tsumo_value_candidate()?.0
                == self.twice.top_expected_self_tsumo_value_candidate()?.0,
        )
    }
}

/// 同じ局面について A / B の評価を1回ずつ行う。
///
/// 評価順は A → B の固定だが、どちらも自分専用の thread で計るため、先に走った方式が後の方式の
/// thread-local memo を暖めることはない。値は評価順に依らない。
pub fn compare_iishanten_continuation_depths(
    context: &GameContext,
    legal_actions: &[LegalAction],
) -> IishantenContinuationDepthComparison {
    IishantenContinuationDepthComparison {
        once: profile_iishanten_continuation_depth(
            context,
            legal_actions,
            IishantenContinuationDepth::Once,
        ),
        twice: profile_iishanten_continuation_depth(
            context,
            legal_actions,
            IishantenContinuationDepth::Twice,
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context::{GameContext, TableStateFacts};
    use crate::shanten_test_support::{dahai, tile};
    use bot_logic::{HistoryFuritenFacts, TileId, forward_metrics_for_candidate};

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
        // 合法打牌を絞ると評価する候補だけが減り、候補1件あたりの探索も値も変わらない。
        // 追加深度の探索は重いので、比較対象の 5p / 9p だけを残す。
        let actions = [53, 68].iter().map(|&value| dahai(value)).collect();
        (context, actions)
    }

    #[test]
    fn the_once_depth_is_the_production_expected_self_tsumo_value() {
        // A は production の1向聴 continuation そのもの。既存の前方集計基盤が返す値と一致する。
        let (context, actions) = iishanten_context();
        let profile = profile_iishanten_continuation_depth(
            &context,
            &actions,
            IishantenContinuationDepth::Once,
        );
        assert!(profile.candidates.len() > 1);

        let legal = legal_discard_evaluations(&context, &actions);
        let valuator = ProductionProspectiveValuator::new(&context);
        let inputs = lookahead_inputs(
            &context,
            &legal.tiles,
            &valuator,
            LookaheadDiagnosticScope::None,
        );
        for &(discard, value, _) in &profile.candidates {
            let evaluation = legal
                .evaluations
                .iter()
                .find(|evaluation| evaluation.discard == discard)
                .expect("候補は合法打牌");
            assert_eq!(
                value,
                forward_metrics_for_candidate(&inputs, evaluation).expected_self_tsumo_value,
                "{discard:?}",
            );
        }
    }

    #[test]
    fn the_extra_depth_only_adds_value_to_the_same_candidates() {
        // B は A の枝を残したまま手変わり2回の経路を足すので、候補集合は同じで値は下がらない。
        // 内訳も候補全体の値の分解そのもので、どちらの深度でも足すと一致する。
        let (context, actions) = iishanten_context();
        let comparison = compare_iishanten_continuation_depths(&context, &actions);

        let discards = |profile: &IishantenContinuationDepthProfile| {
            profile
                .candidates
                .iter()
                .map(|&(discard, ..)| discard)
                .collect::<Vec<_>>()
        };
        assert_eq!(discards(&comparison.once), discards(&comparison.twice));

        let mut added_anywhere = false;
        for &(discard, once, _) in &comparison.once.candidates {
            let once = once.expect("A の値を確定できる");
            let twice = comparison
                .twice
                .value(discard)
                .expect("同じ候補がある")
                .expect("B の値を確定できる");
            assert!(twice >= once, "{discard:?}: {twice} >= {once}");
            added_anywhere |= twice > once;
        }
        assert!(added_anywhere);

        // 表示する順位は ExpectedSelfTsumoValue 単独の ranking の1位で、production の打牌選択では
        // ない。5p / 9p はどちらも同じ pre-acceptance cohort にあり値も確定しているため、この
        // 局面に限れば深度で1位が入れ替わることがそのまま観測できる。
        let top = |profile: &IishantenContinuationDepthProfile| {
            let (discard, value) = profile
                .top_expected_self_tsumo_value_candidate()
                .expect("値を確定できる候補がある");
            assert_eq!(
                profile.ranked().first().copied(),
                Some((discard, Some(value)))
            );
            (discard, value)
        };
        // A は production の打牌選択が実際に使う値そのもの。
        assert_eq!(top(&comparison.once), (tile(68).tile_type(), 697_475_278));
        assert_eq!(
            top(&comparison.twice),
            (tile(53).tile_type(), 1_031_805_837)
        );
        assert_eq!(
            comparison.shares_the_top_expected_self_tsumo_value_candidate(),
            Some(false),
        );

        assert!(comparison.twice.search.draw_variants > comparison.once.search.draw_variants);
        assert!(
            comparison.twice.search.terminal_scorings > comparison.once.search.terminal_scorings
        );

        for profile in [&comparison.once, &comparison.twice] {
            for &(discard, value, _) in &profile.candidates {
                let breakdown = profile.breakdown_of(discard).expect("内訳がある");
                let total: Option<u64> = breakdown
                    .draws
                    .iter()
                    .try_fold(0u64, |total, contribution| {
                        Some(total + contribution.value?)
                    });
                assert_eq!(total, value, "{discard:?}");
            }
        }
    }
}
