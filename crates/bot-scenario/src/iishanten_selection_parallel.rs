//! production の1向聴 depth B を、深く評価する候補ごとに並行評価した場合の wall-clock を表示
//! する。
//!
//! 比べるのは同じ深度 B の中での候補評価の分け方だけで、深度も comparator も値も枝も scoring
//! semantics も変えない。S が同じ B depth の逐次評価、P<N> が候補単位の並行評価。PA が
//! `available_parallelism` を上限にする現在の production と同じ方式。
//!
//! 表示するのは ExpectedSelfTsumoValue 単独の ranking ではなく、既存 comparator を通した最終
//! 打牌。深く評価される候補も、unknown の軸解決も、比較理由も production selection が使った
//! ものそのままで、表示のために比較をやり直さない。
//!
//! 方式ごとに run を2本取る。`elapsed` は instrumentation を持たない計測 run のもので、cohort・
//! 値・search / memo / phase の stats は観測 run のもの。どちらの run も新しい thread で行うため、
//! 先に走った方式が後の方式の thread-local memo を暖めない。
//!
//! 候補を thread へ分けると、候補間で共有していた探索基盤 (base 評価 memo・同一 state memo・
//! thread-local の向聴 / 受け入れ memo) を worker ごとに作り直す。速くなったかだけでなく、その
//! 共有を失って総仕事量がどれだけ増えたかも併せて表示する。
//!
//! production の打牌選択は B depth を PA と同じ方式で通す。この表示はその production 経路を
//! そのまま計測するだけで、並列評価を別実装しない。

use std::time::Duration;

use bot_core::{
    IishantenSelectionDepthCandidate, IishantenSelectionParallelComparison,
    IishantenSelectionParallelDecision, IishantenSelectionParallelRun,
    compare_iishanten_selection_parallelism,
};
use bot_logic::{SELF_TSUMO_VALUE_SCALE, SearchStateMemoStats, ThreeShantenSearchStats, TileType};

use crate::scenario::Scenario;

/// 1局面の S / P 比較。選択も値も実測も decision そのもので、表示のために選択し直さない。
pub fn format_scenario_comparison(scenario: &Scenario) -> String {
    let comparison = compare_iishanten_selection_parallelism(
        &scenario.context,
        scenario.legal_actions.as_slice(),
    );

    let mut lines = vec![
        "Iishanten selection candidate parallelism comparison".to_string(),
        "  every mode runs the same production B depth: same-shanten twice plus the exact \
         same-state memo"
            .to_string(),
        "  every mode runs the production discard selection: the same candidate gating, unknown \
         axis resolution, comparison order, stable order and final selection"
            .to_string(),
        "  only candidates tied through the pre-acceptance axes (Shanten -> IsolatedTile -> \
         IsolatedHonor) are evaluated deeply; the parallel modes split exactly those candidates \
         across threads and write each result back to its candidate index"
            .to_string(),
        "  a candidate's forward metrics depend only on that candidate and the search settings, \
         so the split changes neither the values nor the branches; the workers never exceed the \
         deep evaluated candidates"
            .to_string(),
        "  each mode runs twice, each run on its own fresh thread so that none warms another's \
         thread-local memos: an observation run with the search-size counters and the phase timer \
         enabled, then a timing run with no instrumentation at all"
            .to_string(),
        "  elapsed comes from the timing run only; the cohort, the values and the search / memo / \
         phase stats come from the observation run"
            .to_string(),
        "  the parallel modes give every worker its own search state, so the base evaluation, \
         same-state and thread-local memos are rebuilt per worker: the wall clock drops while the \
         total work grows"
            .to_string(),
        "  production discard selection runs this same B depth with the PA mode: up to \
         available_parallelism workers, capped by the deep evaluated candidates; S, P2 and P4 \
         stay as the comparison baselines"
            .to_string(),
        String::new(),
    ];
    for decision in comparison.decisions() {
        lines.extend(format_decision(&comparison, decision));
        lines.push(String::new());
    }
    lines.extend(format_delta(&comparison));
    lines.join("\n")
}

fn format_decision(
    comparison: &IishantenSelectionParallelComparison,
    decision: &IishantenSelectionParallelDecision,
) -> Vec<String> {
    let observation: &IishantenSelectionParallelRun = &decision.observation;
    let deep = observation.deep_evaluated_candidates();
    let mut lines = vec![
        decision.parallelism.label(),
        format!(
            "  workers requested: {}, used for the deep candidate evaluation: {}",
            decision.parallelism.requested_workers(),
            decision.workers(),
        ),
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
            "  bit-exact with the sequential mode: {}",
            comparison.matches_sequential(decision),
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

fn format_delta(comparison: &IishantenSelectionParallelComparison) -> Vec<String> {
    let sequential = &comparison.sequential;
    let mut lines = vec![
        "Selection S -> P".to_string(),
        format!(
            "  every parallel mode is bit-exact with the sequential mode: {}",
            comparison.every_mode_matches_sequential(),
        ),
        format!("  selected discard: {}", format_selected(sequential)),
        format!(
            "  selected ExpectedSelfTsumoValue: {}",
            format_value(sequential.observation.selected_expected_self_tsumo_value()),
        ),
        String::new(),
        "Latency".to_string(),
    ];
    for decision in comparison.decisions() {
        lines.push(format!(
            "  {}: {} ({} workers, {})",
            mode_name(decision),
            format_duration(decision.elapsed()),
            decision.workers(),
            format_speedup(comparison.speedup(decision)),
        ));
    }

    lines.push(String::new());
    lines.push("Total work (observation runs, S -> mode)".to_string());
    for decision in comparison.parallel.iter() {
        lines.push(format!("  {}", mode_name(decision)));
        for (label, value) in SEARCH_COUNTERS {
            let base = value(&sequential.observation.search);
            let mode = value(&decision.observation.search);
            lines.push(format!(
                "    {label}: {base} -> {mode} ({})",
                format_ratio(base, mode),
            ));
        }
        for (label, value) in MEMO_COUNTERS {
            let base = value(&sequential.observation.memo);
            let mode = value(&decision.observation.memo);
            lines.push(format!(
                "    {label}: {base} -> {mode} ({})",
                format_ratio(base, mode),
            ));
        }
    }
    lines
}

fn mode_name(decision: &IishantenSelectionParallelDecision) -> String {
    decision
        .parallelism
        .label()
        .split_whitespace()
        .next()
        .unwrap_or_default()
        .to_string()
}

fn format_selected(decision: &IishantenSelectionParallelDecision) -> String {
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

// S / この方式の比。この方式が 0 の場合は比を作れないので出さない。
fn format_speedup(speedup: Option<f64>) -> String {
    match speedup {
        Some(speedup) => format!("speedup {speedup:.3}x"),
        None => "speedup n/a".to_string(),
    }
}

fn format_ratio(sequential: u64, mode: u64) -> String {
    if sequential == 0 {
        return "n/a".to_string();
    }
    format!("{:.1}%", mode as f64 / sequential as f64 * 100.0)
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
