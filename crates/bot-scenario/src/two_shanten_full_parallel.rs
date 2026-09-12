//! production の2向聴 selection で、ドラ差 gate を通った provisional 上位2候補の Full 追加
//! 評価を2並列にした場合の wall-clock を表示する。
//!
//! 比べるのはこの2候補の実行 orchestration だけで、Progress-first も ForwardTargets cohort も
//! 上位2候補の選び方も Full gate も comparator も値の意味も変えない。S がその2候補を1本の
//! `LookaheadInputs` で順に評価する baseline、P2 が `min(2, available_parallelism)` を上限に
//! 分けて評価し探索基盤を worker ごとに閉じる方式、P2S が同じく分けたうえで base 評価・構造
//! 評価・未来テンパイ値の exact cache を worker 間で共有する現在の production の方式。
//!
//! 表示するのは Full 値単独の ranking ではなく、既存 comparator を通した最終打牌。Progress
//! cohort も gate を通った pair も比較理由も production selection が使ったものそのままで、
//! 表示のために比較をやり直さない。
//!
//! 方式ごとに run を2本取る。`elapsed` は instrumentation を持たない計測 run のもので、値・
//! search / memo / phase の stats は観測 run のもの。どちらの run も新しい thread で行うため、
//! 先に走った方式が後の方式の thread-local memo を暖めない。
//!
//! 2候補を thread へ分けると、Progress 段で暖まっていた探索基盤 (base 評価 memo・構造評価
//! memo・未来テンパイ値 memo・thread-local の向聴 / 受け入れ memo) を worker ごとに作り直す。
//! P2S はそのうち base 評価・構造評価・未来テンパイ値を worker 間で共有して同じ exact input の
//! 再評価を落とす。速くなったかだけでなく、方式ごとに総仕事量がどう動いたかと、共有 cache が
//! 保持した entry 数も併せて表示する。

use std::time::Duration;

use bot_core::{
    TwoShantenFullParallelCandidate, TwoShantenFullParallelComparison,
    TwoShantenFullParallelDecision, TwoShantenFullParallelRun,
    compare_two_shanten_full_parallelism, two_shanten_full_parallelism_is_available,
};
use bot_logic::{SELF_TSUMO_VALUE_SCALE, SearchStateMemoStats, ThreeShantenSearchStats, TileType};

use crate::scenario::Scenario;

/// 1局面の S / P2 比較。選択も値も実測も decision そのもので、表示のために選択し直さない。
pub fn format_scenario_comparison(scenario: &Scenario) -> String {
    let comparison =
        compare_two_shanten_full_parallelism(&scenario.context, scenario.legal_actions.as_slice());

    let mut lines = vec![
        "Two-shanten full pair parallelism comparison".to_string(),
        "  both modes run the production discard selection: the same Progress-first cohort, the \
         same provisional top-2, the same dora-difference gate, the same comparator and the same \
         final selection"
            .to_string(),
        "  only the dora-gated provisional top-2 are evaluated with the full self-tsumo value; \
         the parallel mode splits exactly those two candidates across threads and writes each \
         result back to its pair index"
            .to_string(),
        "  a candidate's full value depends only on that candidate, the search settings and its \
         own progress contribution, so the split changes neither the values nor the branches"
            .to_string(),
        "  each mode runs twice, each run on its own fresh thread so that none warms another's \
         thread-local memos: an observation run with the search-size counters and the phase timer \
         enabled, then a timing run with no instrumentation at all"
            .to_string(),
        "  elapsed comes from the timing run only; the values and the search / memo / phase stats \
         come from the observation run"
            .to_string(),
        "  P2 gives every worker its own search state, so the base evaluation and structural \
         memos warmed by the Progress cohort are rebuilt per worker: the wall clock drops while \
         the total work grows"
            .to_string(),
        "  P2S splits the same pair but shares the exact base evaluation, structural evaluation \
         and future-tenpai value entries across the workers, seeded from what the Progress cohort \
         already evaluated: the same values from fewer evaluations"
            .to_string(),
        "  the shared entries are keyed by the position state itself and never evicted, so which \
         worker fills an entry first changes neither the values nor the selection; only the miss \
         counters move by a few entries between runs"
            .to_string(),
        format!(
            "  production discard selection runs the P2 mode; this runtime splits the pair: {}",
            two_shanten_full_parallelism_is_available(),
        ),
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
    comparison: &TwoShantenFullParallelComparison,
    decision: &TwoShantenFullParallelDecision,
) -> Vec<String> {
    let observation: &TwoShantenFullParallelRun = &decision.observation;
    let progress = observation.progress_evaluated_candidates();
    let full = observation.full_evaluated_candidates();
    let mut lines = vec![
        decision.parallelism.label(),
        format!(
            "  workers requested: {}, used for the full pair evaluation: {}",
            decision.parallelism.requested_workers(),
            decision.full_workers(),
        ),
        format!("  selected discard: {}", format_selected(decision)),
        format!(
            "  progress cohort: {} of {} ({})",
            progress.len(),
            observation.candidates.len(),
            format_tiles(&progress),
        ),
        format!(
            "  full evaluated pair: {} ({})",
            full.len(),
            format_tiles(&full),
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
            comparison.parallel_matches_sequential(),
        ),
        format!(
            "  observation run phases: base evaluation {}, forward metrics {}, two-shanten \
             self-tsumo {}, selection finalize {}",
            format_duration(observation.phases.base_evaluation),
            format_duration(observation.phases.forward_metrics),
            format_duration(observation.phases.two_shanten_self_tsumo),
            format_duration(observation.phases.selection_finalize),
        ),
        // 並行に評価した2候補は同時に走るため、候補別の合計は phase を超え得る。
        format!(
            "  two-shanten self-tsumo candidate timings (observation run, the gated pair appears \
             again after its progress entry): {}",
            format_candidate_timings(&observation.two_shanten_self_tsumo_candidates),
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

// 候補1件。Progress 寄与、gate を通った場合の Full 値、そして選ばれた候補がこの候補を上回った
// 理由まで、選択が使った事実をそのまま並べる。
fn format_candidate(candidate: &TwoShantenFullParallelCandidate) -> String {
    let progress = match candidate.progress_self_tsumo_value {
        Some(value) => format!("progress {}", format_value(Some(value))),
        None => "progress not evaluated".to_string(),
    };
    let full = match candidate.expected_self_tsumo_value {
        Some(value) => format!("full {}", format_value(Some(value))),
        None => "full not evaluated".to_string(),
    };
    let verdict = if candidate.selected {
        "selected".to_string()
    } else {
        format!("lost on {:?}", candidate.comparison_reason)
    };
    format!(
        "{}: {}, {}, {}",
        candidate.discard.to_mjai_string(),
        progress,
        full,
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

fn format_delta(comparison: &TwoShantenFullParallelComparison) -> Vec<String> {
    let sequential = &comparison.sequential;
    let mut lines = vec![
        "Selection".to_string(),
        format!(
            "  every mode is bit-exact with the sequential mode: {}",
            comparison.parallel_matches_sequential(),
        ),
        format!("  selected discard: {}", format_selected(sequential)),
        format!(
            "  full evaluated pair: {}",
            format_tiles(&sequential.observation.full_evaluated_candidates()),
        ),
        String::new(),
        "Latency".to_string(),
    ];
    for decision in comparison.decisions() {
        lines.push(format!(
            "  {}: {} ({} workers)",
            mode_name(decision),
            format_duration(decision.elapsed()),
            decision.full_workers(),
        ));
    }
    lines.push(format!(
        "  S -> P2S: {}",
        format_speedup(comparison.speedup()),
    ));
    lines.push(format!(
        "  P2 -> P2S: {}",
        format_speedup(comparison.shared_speedup()),
    ));

    lines.push(String::new());
    lines.push("Total work (observation runs, S -> P2 -> P2S)".to_string());
    for (label, value) in SEARCH_COUNTERS {
        let base = value(&sequential.observation.search);
        let isolated = value(&comparison.isolated.observation.search);
        let shared = value(&comparison.shared.observation.search);
        lines.push(format!(
            "  {label}: {base} -> {isolated} ({}) -> {shared} ({})",
            format_ratio(base, isolated),
            format_ratio(base, shared),
        ));
    }
    for (label, value) in MEMO_COUNTERS {
        let base = value(&sequential.observation.memo);
        let isolated = value(&comparison.isolated.observation.memo);
        let shared = value(&comparison.shared.observation.memo);
        lines.push(format!(
            "  {label}: {base} -> {isolated} ({}) -> {shared} ({})",
            format_ratio(base, isolated),
            format_ratio(base, shared),
        ));
    }

    lines.push(String::new());
    lines.push("Shared cache footprint (P2S observation run)".to_string());
    let shared = comparison.shared.shared_cache();
    for (label, entries) in [
        ("base evaluations", shared.base_evaluations),
        ("structural evaluations", shared.structural_evaluations),
        (
            "future-tenpai selection values",
            shared.tenpai_selection_values,
        ),
        ("future-tenpai tsumo values", shared.tenpai_tsumo_values),
    ] {
        lines.push(format!("  {label}: {entries} entries"));
    }
    lines
}

fn mode_name(decision: &TwoShantenFullParallelDecision) -> String {
    decision
        .parallelism
        .label()
        .split_whitespace()
        .next()
        .unwrap_or_default()
        .to_string()
}

fn format_selected(decision: &TwoShantenFullParallelDecision) -> String {
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

fn format_candidate_timings(candidates: &[(TileType, Duration)]) -> String {
    if candidates.is_empty() {
        return "none".to_string();
    }
    candidates
        .iter()
        .map(|(discard, elapsed)| {
            format!("{}={}", discard.to_mjai_string(), format_duration(*elapsed))
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn format_duration(duration: Duration) -> String {
    format!("{:.3} ms", duration.as_secs_f64() * 1_000.0)
}

// S / P2 の比。P2 が 0 の場合は比を作れないので出さない。
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
