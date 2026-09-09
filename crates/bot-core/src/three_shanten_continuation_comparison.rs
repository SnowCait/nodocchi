//! 3向聴 Progress self-tsumo 評価の1向聴 continuation scope A/B 比較。
//!
//! A (current) と B (experimental) の違いは1向聴到達後に [`bot_logic::DrawTransition::SameShanten`]
//! のツモを追うかどうかだけで、3→2 / 2→1 の探索も、ツモ後の最良打牌の比較も、テンパイ到達後の
//! terminal scoring も、確率・残り自摸機会・unknown 伝播も両方式で同じ primitive を共有する。
//!
//! この module は比較のための計測だけを行い、production の打牌選択には接続しない。
//!
//! # 計測条件
//!
//! 向聴・受け入れ・一向聴形の memo は thread ごとに持つため、同じ thread で A → B と続けて
//! 評価すると後から走った方式が暖まった memo を使ってしまう。方式ごとの実測は必ず新しい
//! thread で行い ([`measured_on_a_fresh_thread`])、どちらの方式も同じ cold な thread-local
//! から始める。探索する枝も評価値も選択も、計測 thread の違いでは変わらない。

use std::cmp::Reverse;
use std::time::{Duration, Instant};

use bot_logic::{IishantenContinuationScope, ProgressMemoStats, ThreeShantenSearchStats, TileType};

use crate::action::LegalAction;
use crate::context::GameContext;
use crate::decision_timing::{NormalDiscardPhaseDurations, NormalDiscardPhaseTimer};
use crate::discard_selection::{
    LookaheadDiagnosticScope, legal_discard_evaluations, lookahead_inputs,
    select_discard_action_with_continuation_scope_instrumented, three_shanten_value_for_candidate,
};
use crate::prospective_value::ProductionProspectiveValuator;

/// 比較する2方式。値の意味が違うため、どちらの scope で評価した値かを必ず添える。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThreeShantenContinuationScope {
    /// A: 現行 production。1向聴到達後も Progress + SameShanten を追う。
    Current,
    /// B: 比較実験。1向聴到達後は Progress だけを追う。
    ProgressOnly,
}

impl ThreeShantenContinuationScope {
    pub const BOTH: [Self; 2] = [Self::Current, Self::ProgressOnly];

    pub fn label(self) -> &'static str {
        match self {
            Self::Current => "current (1-shanten: Progress + SameShanten)",
            Self::ProgressOnly => "progress-only (1-shanten: Progress only)",
        }
    }

    fn continuation(self) -> IishantenContinuationScope {
        match self {
            Self::Current => IishantenContinuationScope::ProgressAndSameShanten,
            Self::ProgressOnly => IishantenContinuationScope::ProgressOnly,
        }
    }
}

/// 1局面を1方式で評価した打牌選択の実測。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ThreeShantenContinuationDecision {
    pub scope: ThreeShantenContinuationScope,
    /// 3向聴 Progress 軸を実際に評価したか。
    pub fired: bool,
    pub selected: Option<LegalAction>,
    /// 通常打牌選択1回の実測時間。
    pub elapsed: Duration,
    pub phases: NormalDiscardPhaseDurations,
    /// 3向聴軸を評価した候補の値。発火しなかった局面では空。
    pub candidates: Vec<(TileType, Option<u64>)>,
}

impl ThreeShantenContinuationDecision {
    /// 3向聴軸だけの実測時間。
    pub fn three_shanten_elapsed(&self) -> Duration {
        self.phases.three_shanten_self_tsumo
    }

    pub fn selected_discard(&self) -> Option<TileType> {
        match self.selected {
            Some(LegalAction::Dahai { tile }) => Some(tile.tile_type()),
            _ => None,
        }
    }

    /// 値の高い順に並べた3向聴候補。unknown は値の順へ混ぜず末尾へ回す。
    ///
    /// `Option` の既定の順序では `None` が `Some` より小さく、降順に並べると unknown が先頭へ
    /// 来てしまうため、確定しているかどうかを先に見る。
    pub fn ranked_candidates(&self) -> Vec<(TileType, Option<u64>)> {
        let mut ranked = self.candidates.clone();
        ranked.sort_by_key(|(_, value)| (value.is_none(), Reverse(*value)));
        ranked
    }
}

// 計測を新しい thread で行う。向聴・受け入れ・一向聴形の memo は thread-local なので、
// thread を分ければ先に走った方式が後の方式の memo を暖めることがない。探索する枝も評価値も
// 選択もこの thread の違いでは変わらない。
fn measured_on_a_fresh_thread<T: Send>(measure: impl FnOnce() -> T + Send) -> T {
    std::thread::scope(|scope| {
        scope
            .spawn(measure)
            .join()
            .expect("計測 thread は panic しない")
    })
}

/// 指定した方式で通常打牌選択を1回行い、選ばれた action と実測時間を返す。
///
/// 3向聴軸の evaluator 以外は production の打牌選択と同じ経路を1回ずつ通る。計測は
/// [`measured_on_a_fresh_thread`] の中で行うため、同じ局面を続けて評価しても前の方式の
/// thread-local memo は引き継がない。
pub fn decide_with_three_shanten_continuation_scope(
    context: &GameContext,
    legal_actions: &[LegalAction],
    scope: ThreeShantenContinuationScope,
) -> ThreeShantenContinuationDecision {
    measured_on_a_fresh_thread(|| decide_on_the_measuring_thread(context, legal_actions, scope))
}

fn decide_on_the_measuring_thread(
    context: &GameContext,
    legal_actions: &[LegalAction],
    scope: ThreeShantenContinuationScope,
) -> ThreeShantenContinuationDecision {
    let mut timing = NormalDiscardPhaseTimer::started();
    let started = Instant::now();
    let scoped = select_discard_action_with_continuation_scope_instrumented(
        context,
        legal_actions,
        scope.continuation(),
        &mut timing,
    );
    let elapsed = started.elapsed();
    ThreeShantenContinuationDecision {
        scope,
        fired: !scoped.three_shanten.is_empty(),
        selected: scoped.selection.action,
        elapsed,
        phases: timing.finish(),
        candidates: scoped.three_shanten,
    }
}

/// 同じ局面を A / B の順で評価した比較結果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ThreeShantenContinuationComparison {
    pub current: ThreeShantenContinuationDecision,
    pub progress_only: ThreeShantenContinuationDecision,
}

impl ThreeShantenContinuationComparison {
    /// 3向聴軸がどちらの方式でも発火したか。発火条件は評価器に依らないため必ず一致する。
    pub fn fired(&self) -> bool {
        self.current.fired && self.progress_only.fired
    }

    /// A / B が同じ打牌を選んだか。
    pub fn selects_the_same_discard(&self) -> bool {
        self.current.selected == self.progress_only.selected
    }
}

/// 同じ局面について A / B の打牌選択を1回ずつ行う。
///
/// 評価順は A → B の固定だが、どちらも自分専用の thread で計るため、先に走った方式が後の
/// 方式の thread-local memo を暖めることはない。値も選択も評価順に依らない。
pub fn compare_three_shanten_continuation_scopes(
    context: &GameContext,
    legal_actions: &[LegalAction],
) -> ThreeShantenContinuationComparison {
    ThreeShantenContinuationComparison {
        current: decide_with_three_shanten_continuation_scope(
            context,
            legal_actions,
            ThreeShantenContinuationScope::Current,
        ),
        progress_only: decide_with_three_shanten_continuation_scope(
            context,
            legal_actions,
            ThreeShantenContinuationScope::ProgressOnly,
        ),
    }
}

/// 全合法3向聴候補を1方式で評価した値・実測時間と、探索規模。
#[derive(Debug, Clone)]
pub struct ThreeShantenContinuationProfile {
    pub scope: ThreeShantenContinuationScope,
    pub memo: ProgressMemoStats,
    pub search: ThreeShantenSearchStats,
    /// unknown は `None` のまま保持する。
    pub candidates: Vec<(TileType, Option<u64>, Duration)>,
    /// 入力構築を除く全候補の評価時間。候補間では既存 memo を共有する。
    pub total: Duration,
}

/// 通常打牌と同じ入力・valuator で全3向聴候補を1方式で評価し、探索規模も計上する。
///
/// production selection は呼ばない。計上の有無で値も枝も変わらない。計測は
/// [`measured_on_a_fresh_thread`] の中で行うため、方式ごとに同じ cold な thread-local から
/// 始める。
pub fn profile_three_shanten_continuation_scope(
    context: &GameContext,
    legal_actions: &[LegalAction],
    scope: ThreeShantenContinuationScope,
) -> ThreeShantenContinuationProfile {
    measured_on_a_fresh_thread(|| profile_on_the_measuring_thread(context, legal_actions, scope))
}

fn profile_on_the_measuring_thread(
    context: &GameContext,
    legal_actions: &[LegalAction],
    scope: ThreeShantenContinuationScope,
) -> ThreeShantenContinuationProfile {
    let legal = legal_discard_evaluations(context, legal_actions);
    let valuator = ProductionProspectiveValuator::new(context);
    let inputs = lookahead_inputs(
        context,
        &legal.tiles,
        &valuator,
        LookaheadDiagnosticScope::TWO_SHANTEN_SELF_TSUMO,
    )
    .with_three_shanten_progress_memo()
    .with_three_shanten_search_stats();
    let value_for_candidate = three_shanten_value_for_candidate(scope.continuation());
    let started = Instant::now();
    let candidates = legal
        .evaluations
        .iter()
        .filter(|evaluation| evaluation.min_shanten_after_discard() == 3)
        .map(|evaluation| {
            let started = Instant::now();
            let value = value_for_candidate(&inputs, evaluation);
            (evaluation.discard, value, started.elapsed())
        })
        .collect();
    ThreeShantenContinuationProfile {
        scope,
        memo: inputs.three_shanten_progress_memo_stats(),
        search: inputs.three_shanten_search_stats(),
        candidates,
        total: started.elapsed(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::meld::{Meld, MeldKind};
    use crate::shanten_test_support::tile;
    use bot_logic::TileId;

    // 3向聴軸が発火する軽い局面。2副露 (白ポン + 發ポン) で concealed 8枚。
    // 役牌2つで開いた手でもツモ和了の打点が確定するため、continuation の値が unknown にならない。
    fn open_three_shanten_context() -> (GameContext, Vec<LegalAction>) {
        let hand: Vec<TileId> = [0, 4, 12, 24, 36, 48, 60, 72]
            .iter()
            .map(|&value| tile(value))
            .collect();
        let melds = [
            vec![
                Meld::new(
                    MeldKind::Pon,
                    vec![tile(124), tile(125), tile(126)],
                    Some(tile(124)),
                ),
                Meld::new(
                    MeldKind::Pon,
                    vec![tile(128), tile(129), tile(130)],
                    Some(tile(128)),
                ),
            ],
            Vec::new(),
            Vec::new(),
            Vec::new(),
        ];
        let dora_indicator = tile(132);
        let visible: Vec<_> = hand
            .iter()
            .copied()
            .chain([124, 125, 126, 128, 129, 130].iter().map(|&v| tile(v)))
            .chain([dora_indicator])
            .collect();
        let context = GameContext::from_parts_with_melds(
            None,
            hand.clone(),
            vec![dora_indicator],
            TileType::new(27),
            TileType::new(28),
            visible,
            Some(0),
            Some(1),
            Default::default(),
            [false; 4],
            melds,
        )
        .with_table_state_facts(crate::context::TableStateFacts {
            remaining_tiles: Some(66),
            ..Default::default()
        });
        // 合法打牌を絞ると評価する候補だけが減り、候補1件あたりの探索も値も変わらない。
        let actions = [48, 60, 72]
            .iter()
            .map(|&value| LegalAction::Dahai { tile: tile(value) })
            .collect();
        (context, actions)
    }

    #[test]
    fn the_progress_only_scope_only_drops_the_one_shanten_same_shanten_branches() {
        // 3→2 の枝数は両方式で同じで、1向聴の SameShanten 枝とその先だけが消える。
        let (context, actions) = open_three_shanten_context();
        let current = profile_three_shanten_continuation_scope(
            &context,
            &actions,
            ThreeShantenContinuationScope::Current,
        );
        let progress_only = profile_three_shanten_continuation_scope(
            &context,
            &actions,
            ThreeShantenContinuationScope::ProgressOnly,
        );

        let discards = |profile: &ThreeShantenContinuationProfile| {
            profile
                .candidates
                .iter()
                .map(|(discard, _, _)| *discard)
                .collect::<Vec<_>>()
        };
        assert_eq!(discards(&current), discards(&progress_only));
        assert!(current.candidates.len() > 1);

        for ((discard, full, _), (_, progress, _)) in
            current.candidates.iter().zip(&progress_only.candidates)
        {
            let full = full.expect("現行方式の値を確定できる");
            let progress = progress.expect("Progress-only の値を確定できる");
            assert!(progress < full, "{discard:?}: {progress} < {full}");
        }

        assert_eq!(
            current.search.three_to_two_variants,
            progress_only.search.three_to_two_variants
        );
        assert!(current.search.iishanten_same_shanten_variants > 0);
        assert!(current.search.iishanten_downstream_variants > 0);
        assert_eq!(progress_only.search.iishanten_same_shanten_variants, 0);
        assert_eq!(progress_only.search.iishanten_downstream_variants, 0);
        assert!(progress_only.search.draw_variants < current.search.draw_variants);
        assert!(progress_only.search.terminal_scorings < current.search.terminal_scorings);
        assert!(progress_only.search.base_evaluation_calls < current.search.base_evaluation_calls);

        // 同じ方式を2回評価しても値は変わらない。
        let repeated = profile_three_shanten_continuation_scope(
            &context,
            &actions,
            ThreeShantenContinuationScope::ProgressOnly,
        );
        let values = |profile: &ThreeShantenContinuationProfile| {
            profile
                .candidates
                .iter()
                .map(|(discard, value, _)| (*discard, *value))
                .collect::<Vec<_>>()
        };
        assert_eq!(values(&repeated), values(&progress_only));
    }

    #[test]
    fn the_current_scope_decision_is_the_production_discard() {
        // A の打牌選択は production の打牌選択そのもの。
        let (context, actions) = open_three_shanten_context();
        let comparison = compare_three_shanten_continuation_scopes(&context, &actions);

        assert!(comparison.fired());
        assert_eq!(
            comparison.current.selected,
            crate::discard_selection::select_discard_action(&context, &actions)
        );
        assert_eq!(
            comparison.current.scope,
            ThreeShantenContinuationScope::Current
        );
        assert_eq!(
            comparison.progress_only.scope,
            ThreeShantenContinuationScope::ProgressOnly
        );
        assert!(comparison.current.three_shanten_elapsed() > Duration::ZERO);
        assert!(comparison.progress_only.three_shanten_elapsed() > Duration::ZERO);

        // 3向聴軸の値は方式ごとに違い、候補の並びは同じ。
        let discards = |decision: &ThreeShantenContinuationDecision| {
            decision
                .candidates
                .iter()
                .map(|(discard, _)| *discard)
                .collect::<Vec<_>>()
        };
        assert_eq!(
            discards(&comparison.current),
            discards(&comparison.progress_only)
        );
        assert_ne!(
            comparison.current.candidates,
            comparison.progress_only.candidates
        );
        assert!(comparison.selects_the_same_discard());
        assert!(comparison.current.selected_discard().is_some());
        assert_eq!(
            comparison.current.ranked_candidates().first().map(|c| c.0),
            comparison.current.selected_discard()
        );
    }

    #[test]
    fn each_measurement_runs_on_its_own_fresh_thread() {
        // 方式ごとに新しい thread で計るので、向聴 / 受け入れ / 一向聴形の thread-local memo は
        // 先に走った方式から引き継がない。
        let caller = std::thread::current().id();
        let first = measured_on_a_fresh_thread(|| std::thread::current().id());
        let second = measured_on_a_fresh_thread(|| std::thread::current().id());

        assert_ne!(first, caller);
        assert_ne!(second, caller);
        assert_ne!(first, second);
    }

    #[test]
    fn the_comparison_values_and_selection_do_not_depend_on_the_evaluation_order() {
        // A → B の比較で得た値・打牌は、B → A の順に単独評価したものと一致する。
        let (context, actions) = open_three_shanten_context();
        let forward = compare_three_shanten_continuation_scopes(&context, &actions);
        let progress_only = decide_with_three_shanten_continuation_scope(
            &context,
            &actions,
            ThreeShantenContinuationScope::ProgressOnly,
        );
        let current = decide_with_three_shanten_continuation_scope(
            &context,
            &actions,
            ThreeShantenContinuationScope::Current,
        );

        assert_eq!(forward.current.candidates, current.candidates);
        assert_eq!(forward.current.selected, current.selected);
        assert_eq!(forward.current.fired, current.fired);
        assert_eq!(forward.progress_only.candidates, progress_only.candidates);
        assert_eq!(forward.progress_only.selected, progress_only.selected);
        assert_eq!(forward.progress_only.fired, progress_only.fired);
    }

    #[test]
    fn the_ranking_puts_unknown_candidates_last() {
        let discard = |mjai: &str| TileType::from_mjai_type_str(mjai).expect("牌種として読める");
        let decision = ThreeShantenContinuationDecision {
            scope: ThreeShantenContinuationScope::Current,
            fired: true,
            selected: None,
            elapsed: Duration::ZERO,
            phases: NormalDiscardPhaseDurations::default(),
            candidates: vec![
                (discard("1m"), None),
                (discard("2m"), Some(10)),
                (discard("3m"), None),
                (discard("4m"), Some(30)),
                (discard("5m"), Some(20)),
            ],
        };

        assert_eq!(
            decision.ranked_candidates(),
            vec![
                (discard("4m"), Some(30)),
                (discard("5m"), Some(20)),
                (discard("2m"), Some(10)),
                // unknown は値の順へ混ぜず、元の順序のまま末尾へ回る。
                (discard("1m"), None),
                (discard("3m"), None),
            ]
        );
    }

    #[test]
    fn the_comparison_reports_no_three_shanten_axis_outside_the_cohort() {
        // 3向聴軸が発火しない局面では、どちらの方式も同じ production 打牌になる。
        let context = crate::shanten_test_support::tenpai_context(&[]);
        let actions = crate::shanten_test_support::tenpai_dahai_actions();
        let comparison = compare_three_shanten_continuation_scopes(&context, &actions);

        assert!(!comparison.fired());
        assert!(comparison.current.candidates.is_empty());
        assert!(comparison.progress_only.candidates.is_empty());
        assert!(comparison.selects_the_same_discard());
        assert_eq!(
            comparison.current.selected,
            crate::discard_selection::select_discard_action(&context, &actions)
        );
        assert_eq!(comparison.current.three_shanten_elapsed(), Duration::ZERO);
    }
}
