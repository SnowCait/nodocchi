//! 1向聴の手変わり深度 A/B を、production の打牌 comparator を通した最終選択として表示する。
//!
//! A は現行 production の打牌選択そのもの (手変わり1回まで)、B は
//! `SameShanten -> SameShanten -> Progress` をもう1段だけ許した診断専用の追加深度。段数が違えば
//! 経路確率も違うため、どちらの深度の値かを必ず添えて表示する。
//!
//! 表示するのは ExpectedSelfTsumoValue 単独の ranking ではなく、既存 comparator を通した最終
//! 打牌。深く評価される候補も、unknown の軸解決も、比較理由も production selection が使った
//! ものそのままで、表示のために比較をやり直さない。
//!
//! 方式ごとに run を2本取る。`elapsed` は instrumentation を持たない計測 run のもので、
//! production selection に無い観測コスト (探索規模の RefCell counter 更新と phase timer) を
//! 含まない。cohort・ExpectedSelfTsumoValue・search / memo / phase の stats は、それらを有効に
//! した観測 run のもの。2本の run は同じ純粋な探索なので選択も cohort も値も一致する。
//!
//! B は追加深度と exact same-state memo を一緒に有効にするため、A -> B の elapsed 差は深度だけ
//! の差ではない。同じ memo 条件へ揃えた純粋な深度比較は
//! `--iishanten-continuation-depth-comparison` が全候補評価として持っている。

use std::time::Duration;

use bot_core::{
    IishantenSelectionDepthCandidate, IishantenSelectionDepthComparison,
    IishantenSelectionDepthDecision, IishantenSelectionDepthRun,
    compare_iishanten_selection_depths,
};
use bot_logic::{SELF_TSUMO_VALUE_SCALE, SearchStateMemoStats, ThreeShantenSearchStats, TileType};

use crate::scenario::Scenario;

/// 1局面の A/B 比較。選択も値も実測も decision そのもので、表示のために選択し直さない。
pub fn format_scenario_comparison(scenario: &Scenario) -> String {
    let comparison =
        compare_iishanten_selection_depths(&scenario.context, scenario.legal_actions.as_slice());

    let mut lines = vec![
        "Iishanten selection depth comparison".to_string(),
        "  A production depth: same-shanten once (Progress, SameShanten -> Progress)".to_string(),
        "  B same-shanten twice: A plus SameShanten -> SameShanten -> Progress, with the exact \
         same-state memo"
            .to_string(),
        "  both depths run the production discard selection: the same candidate gating, unknown \
         axis resolution, comparison order, stable order and final selection"
            .to_string(),
        "  only candidates tied through the pre-acceptance axes (Shanten -> IsolatedTile -> \
         IsolatedHonor) are evaluated deeply"
            .to_string(),
        "  each depth runs twice, each run on its own fresh thread so that none warms another's \
         thread-local memos: an observation run with the search-size counters and the phase timer \
         enabled, then a timing run with no instrumentation at all"
            .to_string(),
        "  elapsed comes from the timing run only, taken after the observation run so that the \
         process is warm the way it is during a match; the cohort, the values and the search / \
         memo / phase stats come from the observation run"
            .to_string(),
        "  B changes the depth and the exact same-state memo together, so A -> B elapsed is not \
         the depth alone; the same-memo depth-only comparison is \
         --iishanten-continuation-depth-comparison"
            .to_string(),
        "  production discard selection uses A; B is a diagnostics-only experiment".to_string(),
        String::new(),
    ];
    lines.extend(format_decision(&comparison.production));
    lines.push(String::new());
    lines.extend(format_decision(&comparison.twice));
    lines.push(String::new());
    lines.extend(format_delta(&comparison));
    lines.join("\n")
}

fn format_decision(decision: &IishantenSelectionDepthDecision) -> Vec<String> {
    let observation: &IishantenSelectionDepthRun = &decision.observation;
    let deep = observation.deep_evaluated_candidates();
    let mut lines = vec![
        decision.depth.label().to_string(),
        format!("  selected discard: {}", format_selected(decision)),
        format!(
            "  selected ExpectedSelfTsumoValue: {}",
            format_value(observation.selected_expected_self_tsumo_value()),
        ),
        format!(
            "  deep evaluation candidates: {} of {} ({})",
            deep.len(),
            observation.candidates.len(),
            format_tiles(&deep),
        ),
        format!(
            "  ExpectedSelfTsumoValue determined: {}, compared by the selection: {}",
            observation.evaluated_expected_self_tsumo_value_count(),
            observation.compared_expected_self_tsumo_value_count(),
        ),
        format!(
            "  total selection elapsed (timing run, no instrumentation): {}",
            format_duration(decision.elapsed()),
        ),
        format!(
            "  observation run elapsed (search-size counters and phase timer enabled): {}",
            format_duration(observation.elapsed),
        ),
        format!(
            "  timing run and observation run agree on the selection: {}",
            decision.runs_agree(),
        ),
        format!(
            "  observation run phases: base evaluation {}, forward metrics {}, selection \
             finalize {}",
            format_duration(observation.phases.base_evaluation),
            format_duration(observation.phases.forward_metrics),
            format_duration(observation.phases.selection_finalize),
        ),
        "  candidates (observation run)".to_string(),
    ];
    for candidate in &observation.candidates {
        lines.push(format!("    {}", format_candidate(candidate)));
    }
    lines.push("  search size (observation run)".to_string());
    for (label, value) in SEARCH_COUNTERS {
        lines.push(format!("    {label}: {}", value(&observation.search)));
    }
    lines.push("  same-state memo (observation run)".to_string());
    for (label, value) in MEMO_COUNTERS {
        lines.push(format!("    {label}: {}", value(&observation.memo)));
    }
    lines
}

// 候補1件。深い評価の対象かどうか、選択が比較に使えた値かどうか、そして選ばれた候補がこの候補を
// 上回った理由まで、選択が使った事実をそのまま並べる。
fn format_candidate(candidate: &IishantenSelectionDepthCandidate) -> String {
    let depth = if candidate.deep_evaluated {
        "deep"
    } else {
        "gated out before the deep evaluation"
    };
    let value = match (
        candidate.evaluated_expected_self_tsumo_value,
        candidate.expected_self_tsumo_value,
    ) {
        (None, _) => "ExpectedSelfTsumoValue not evaluated".to_string(),
        (Some(evaluated), None) => format!(
            "ExpectedSelfTsumoValue {} (axis dropped for its cohort)",
            format_value(Some(evaluated)),
        ),
        (Some(evaluated), Some(_)) => {
            format!("ExpectedSelfTsumoValue {}", format_value(Some(evaluated)))
        }
    };
    let verdict = if candidate.selected {
        "selected".to_string()
    } else {
        format!("lost on {:?}", candidate.comparison_reason)
    };
    format!(
        "{}: {}-shanten after the discard, {}, {}, {}",
        candidate.discard.to_mjai_string(),
        candidate.shanten_after_discard,
        depth,
        value,
        verdict,
    )
}

// 表示する数え上げ1件。label と、その値を取り出す関数の組。
type SearchCounter = (&'static str, fn(&ThreeShantenSearchStats) -> u64);
type MemoCounter = (&'static str, fn(&SearchStateMemoStats) -> u64);

const SEARCH_COUNTERS: [SearchCounter; 6] = [
    ("leaf draw states", |stats| stats.draw_variants),
    ("base evaluation calls", |stats| stats.base_evaluation_calls),
    ("base evaluation misses", |stats| {
        stats.base_evaluation_misses
    }),
    ("shanten / acceptance rebuilds", |stats| {
        stats.structural_evaluation_misses
    }),
    ("same-shanten enumerations", |stats| {
        stats.same_shanten_enumerations
    }),
    ("terminal scorings", |stats| stats.terminal_scorings),
];

const MEMO_COUNTERS: [MemoCounter; 4] = [
    ("next_discard hits", |memo| memo.next_discard_hits),
    ("next_discard misses", |memo| memo.next_discard_misses),
    ("same-shanten next_discard hits", |memo| {
        memo.same_shanten_next_discard_hits
    }),
    ("same-shanten next_discard misses", |memo| {
        memo.same_shanten_next_discard_misses
    }),
];

fn format_delta(comparison: &IishantenSelectionDepthComparison) -> Vec<String> {
    let mut lines = vec![
        "Selection A -> B".to_string(),
        format!(
            "  selected discard: {} -> {}",
            format_selected(&comparison.production),
            format_selected(&comparison.twice),
        ),
        format!("  same discard: {}", comparison.selects_the_same_discard()),
        format!(
            "  deep evaluation candidates: {} -> {} (same cohort: {})",
            comparison
                .production
                .observation
                .deep_evaluated_candidates()
                .len(),
            comparison
                .twice
                .observation
                .deep_evaluated_candidates()
                .len(),
            comparison.shares_the_deep_evaluated_candidates(),
        ),
        format!(
            "  every timing run and observation run agrees: {}",
            comparison.runs_agree(),
        ),
        String::new(),
        "ExpectedSelfTsumoValue A -> B".to_string(),
    ];
    for candidate in &comparison.production.observation.candidates {
        let twice = comparison
            .twice
            .observation
            .candidate(candidate.discard)
            .and_then(|candidate| candidate.evaluated_expected_self_tsumo_value);
        lines.push(format!(
            "  {}: {} -> {}",
            candidate.discard.to_mjai_string(),
            format_value(candidate.evaluated_expected_self_tsumo_value),
            format_value(twice),
        ));
    }

    lines.push(String::new());
    lines.push("Cost A -> B".to_string());
    lines.push(format!(
        "  total selection elapsed (timing runs): {} -> {} ({})",
        format_duration(comparison.production.elapsed()),
        format_duration(comparison.twice.elapsed()),
        format_slowdown(comparison.slowdown()),
    ));
    for (label, value) in SEARCH_COUNTERS {
        let a = value(&comparison.production.observation.search);
        let b = value(&comparison.twice.observation.search);
        lines.push(format!("  {label}: {a} -> {b} ({})", format_ratio(a, b)));
    }
    for (label, value) in MEMO_COUNTERS {
        let a = value(&comparison.production.observation.memo);
        let b = value(&comparison.twice.observation.memo);
        lines.push(format!("  {label}: {a} -> {b} ({})", format_ratio(a, b)));
    }
    lines
}

fn format_selected(decision: &IishantenSelectionDepthDecision) -> String {
    decision
        .selected_discard()
        .map(|discard| discard.to_mjai_string())
        .unwrap_or_else(|| "none".to_string())
}

fn format_tiles(tiles: &[TileType]) -> String {
    if tiles.is_empty() {
        return "none".to_string();
    }
    tiles
        .iter()
        .map(|tile| tile.to_mjai_string())
        .collect::<Vec<_>>()
        .join(", ")
}

fn format_duration(duration: Duration) -> String {
    format!("{:.3} ms", duration.as_secs_f64() * 1_000.0)
}

// A / B の比。A が 0 の場合は比を作れないので出さない。
fn format_slowdown(slowdown: Option<f64>) -> String {
    match slowdown {
        Some(slowdown) => format!("slowdown {slowdown:.3}x"),
        None => "slowdown n/a".to_string(),
    }
}

fn format_ratio(production: u64, twice: u64) -> String {
    if production == 0 {
        return "n/a".to_string();
    }
    format!("{:.1}%", twice as f64 / production as f64 * 100.0)
}

fn format_value(scaled: Option<u64>) -> String {
    let Some(scaled) = scaled else {
        return "unknown".to_string();
    };
    format!(
        "{}.{:06}",
        scaled / SELF_TSUMO_VALUE_SCALE,
        scaled % SELF_TSUMO_VALUE_SCALE
    )
}
