//! 3向聴 Progress self-tsumo 評価の1向聴 continuation scope A/B 比較の表示。
//!
//! A (current) は1向聴到達後も Progress + SameShanten を追う現行 production、B
//! (progress-only) は Progress だけを追う実験。値の意味が違うため、どちらの方式の値かを
//! 必ず添えて表示する。production の打牌選択はこの経路を通らない。

use std::time::Duration;

use bot_core::{
    ThreeShantenContinuationComparison, ThreeShantenContinuationDecision,
    ThreeShantenContinuationProfile, ThreeShantenContinuationScope,
    compare_three_shanten_continuation_scopes, profile_three_shanten_continuation_scope,
};
use bot_logic::{ProgressMemoStats, SELF_TSUMO_VALUE_SCALE, ThreeShantenSearchStats};

use crate::benchmark::LatencyStatistics;
use crate::cli::CaptureComparisonSpec;
use crate::error::ScenarioError;
use crate::format::action_label;
use crate::replay::load_captured_scenarios;
use crate::scenario::Scenario;

/// 1局面の A/B 比較。値と探索規模は profile、打牌は production 比較そのものから取る。
pub fn format_scenario_comparison(scenario: &Scenario) -> String {
    let context = &scenario.context;
    let actions = scenario.legal_actions.as_slice();
    // 方式ごとの評価は bot-core 側でそれぞれ新しい thread へ入るため、どちらも同じ cold な
    // thread-local memo から始まる。ここでの呼び出し順は結果を変えない。
    let current = profile_three_shanten_continuation_scope(
        context,
        actions,
        ThreeShantenContinuationScope::Current,
    );
    let progress_only = profile_three_shanten_continuation_scope(
        context,
        actions,
        ThreeShantenContinuationScope::ProgressOnly,
    );
    let comparison = compare_three_shanten_continuation_scopes(context, actions);

    let mut lines = vec![
        "Three-shanten continuation scope comparison".to_string(),
        "  A current: 3->2 Progress, 2->1 Progress, 1->0 Progress + SameShanten".to_string(),
        "  B progress-only: 3->2 Progress, 2->1 Progress, 1->0 Progress only".to_string(),
        "  terminal scoring, acceptance, comparator, probability and horizon are shared"
            .to_string(),
        "  each scope is measured on its own fresh thread, so neither warms the other".to_string(),
        "  production discard selection is unchanged and always uses A".to_string(),
        String::new(),
    ];
    lines.extend(format_profile(&current));
    lines.push(String::new());
    lines.extend(format_profile(&progress_only));
    lines.push(String::new());
    lines.extend(format_profile_delta(&current, &progress_only));
    lines.push(String::new());
    lines.extend(format_decision(&comparison));
    lines.join("\n")
}

fn format_profile(profile: &ThreeShantenContinuationProfile) -> Vec<String> {
    let mut lines = vec![
        format!("{} values", scope_label(profile.scope)),
        format!("  evaluated candidates: {}", profile.candidates.len()),
        format!("  total elapsed: {}", format_duration(profile.total)),
        format!(
            "  memo hits / misses: two-shanten {} / {}, iishanten {} / {}, next-discard {} / {}",
            profile.memo.two_shanten_hits,
            profile.memo.two_shanten_misses,
            profile.memo.iishanten_hits,
            profile.memo.iishanten_misses,
            profile.memo.next_discard_hits,
            profile.memo.next_discard_misses,
        ),
    ];
    for (discard, value, elapsed) in &profile.candidates {
        lines.push(format!(
            "  {}: {}, elapsed: {}",
            discard.to_mjai_string(),
            format_value(*value),
            format_duration(*elapsed),
        ));
    }
    lines
}

// 表示する数え上げ1件。label と、その値を取り出す関数の組。
type SearchCounter = (&'static str, fn(&ThreeShantenSearchStats) -> u64);
type MemoCounter = (&'static str, fn(&ProgressMemoStats) -> u64);

fn format_profile_delta(
    current: &ThreeShantenContinuationProfile,
    progress_only: &ThreeShantenContinuationProfile,
) -> Vec<String> {
    let search: [SearchCounter; 11] = [
        ("3->2 draw variants", |stats| stats.three_to_two_variants),
        ("2->1 draw variants", |stats| stats.two_to_one_variants),
        ("1-shanten Progress branches", |stats| {
            stats.iishanten_progress_variants
        }),
        ("1-shanten SameShanten branches", |stats| {
            stats.iishanten_same_shanten_variants
        }),
        ("SameShanten downstream branches", |stats| {
            stats.iishanten_downstream_variants
        }),
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
    let memo: [MemoCounter; 2] = [
        ("next_discard calls", |memo| {
            memo.next_discard_hits + memo.next_discard_misses
        }),
        ("next_discard misses", |memo| memo.next_discard_misses),
    ];

    let mut lines = vec![
        "Search size A -> B".to_string(),
        format!(
            "  total elapsed: {} -> {} ({})",
            format_duration(current.total),
            format_duration(progress_only.total),
            format_speedup(current.total, progress_only.total),
        ),
    ];
    for (label, value) in search {
        let a = value(&current.search);
        let b = value(&progress_only.search);
        lines.push(format!("  {label}: {a} -> {b} ({})", format_ratio(a, b)));
    }
    for (label, value) in memo {
        let a = value(&current.memo);
        let b = value(&progress_only.memo);
        lines.push(format!("  {label}: {a} -> {b} ({})", format_ratio(a, b)));
    }
    lines
}

fn format_decision(comparison: &ThreeShantenContinuationComparison) -> Vec<String> {
    let mut lines = vec![
        "Selected discard".to_string(),
        format!("  three-shanten axis fired: {}", comparison.fired()),
    ];
    for decision in [&comparison.current, &comparison.progress_only] {
        lines.push(format!(
            "  {}: {} (discard selection {}, three-shanten phase {})",
            scope_label(decision.scope),
            decision
                .selected
                .as_ref()
                .map(action_label)
                .unwrap_or_else(|| "none".to_string()),
            format_duration(decision.elapsed),
            format_duration(decision.three_shanten_elapsed()),
        ));
    }
    lines.push(format!(
        "  same discard: {}",
        comparison.selects_the_same_discard()
    ));
    lines
}

/// capture 全 request の A/B 比較。
pub fn run_capture_comparison(spec: &CaptureComparisonSpec) -> Result<String, ScenarioError> {
    let mut requests = Vec::new();
    for path in &spec.paths {
        for captured in load_captured_scenarios(path)? {
            let comparison = compare_three_shanten_continuation_scopes(
                &captured.scenario.context,
                &captured.scenario.legal_actions,
            );
            if !comparison.fired() {
                continue;
            }
            requests.push(ComparedRequest {
                capture: captured.path.clone(),
                request_id: captured.request_id,
                comparison,
            });
        }
    }
    Ok(format_capture_comparison(&spec.paths, &requests))
}

struct ComparedRequest {
    capture: String,
    request_id: u64,
    comparison: ThreeShantenContinuationComparison,
}

fn format_capture_comparison(paths: &[String], requests: &[ComparedRequest]) -> String {
    let current: Vec<_> = requests
        .iter()
        .map(|request| request.comparison.current.elapsed)
        .collect();
    let progress_only: Vec<_> = requests
        .iter()
        .map(|request| request.comparison.progress_only.elapsed)
        .collect();
    let current_phase: Vec<_> = requests
        .iter()
        .map(|request| request.comparison.current.three_shanten_elapsed())
        .collect();
    let progress_only_phase: Vec<_> = requests
        .iter()
        .map(|request| request.comparison.progress_only.three_shanten_elapsed())
        .collect();

    let mut lines = vec![
        "Three-shanten continuation scope A/B over captures".to_string(),
        format!("  captures: {}", paths.len()),
        "  A current: 1-shanten Progress + SameShanten (production)".to_string(),
        "  B progress-only: 1-shanten Progress only (experimental)".to_string(),
        "  each scope is measured on its own fresh thread, so neither warms the other".to_string(),
        format!(
            "  requests with the three-shanten axis fired: {}",
            requests.len()
        ),
        String::new(),
    ];
    lines.extend(format_statistics(
        "Discard selection elapsed",
        &current,
        &progress_only,
    ));
    lines.push(String::new());
    lines.extend(format_statistics(
        "Three-shanten phase elapsed",
        &current_phase,
        &progress_only_phase,
    ));
    lines.push(String::new());
    lines.extend(format_selection_difference(requests));
    lines.push(String::new());
    lines.push("Per request".to_string());
    for request in requests {
        lines.push(format!(
            "  {}  request_id={}  A={} B={} ({})  three_shanten A={} B={}  A_selected={} B_selected={}{}",
            request.capture,
            request.request_id,
            format_duration(request.comparison.current.elapsed),
            format_duration(request.comparison.progress_only.elapsed),
            format_speedup(
                request.comparison.current.elapsed,
                request.comparison.progress_only.elapsed
            ),
            format_duration(request.comparison.current.three_shanten_elapsed()),
            format_duration(request.comparison.progress_only.three_shanten_elapsed()),
            selected_label(&request.comparison.current),
            selected_label(&request.comparison.progress_only),
            if request.comparison.selects_the_same_discard() {
                ""
            } else {
                "  DIFFERENT"
            },
        ));
    }
    lines.join("\n")
}

fn format_selection_difference(requests: &[ComparedRequest]) -> Vec<String> {
    let same = requests
        .iter()
        .filter(|request| request.comparison.selects_the_same_discard())
        .count();
    let different = requests.len() - same;
    let mut lines = vec![
        "Selection difference".to_string(),
        format!("  same: {same}"),
        format!("  different: {different}"),
        format!(
            "  agreement: {}",
            format_ratio_percent(same, requests.len())
        ),
    ];
    for request in requests
        .iter()
        .filter(|request| !request.comparison.selects_the_same_discard())
    {
        lines.push(format!(
            "  {}  request_id={}  current={} progress-only={}",
            request.capture,
            request.request_id,
            selected_label(&request.comparison.current),
            selected_label(&request.comparison.progress_only),
        ));
        lines.push(format!(
            "    A top: {}",
            format_ranked(&request.comparison.current)
        ));
        lines.push(format!(
            "    B top: {}",
            format_ranked(&request.comparison.progress_only)
        ));
    }
    lines
}

// 上位3候補だけを出し、差がある局面の理由を1行で読めるようにする。
const RANKED_CANDIDATE_COUNT: usize = 3;

fn format_ranked(decision: &ThreeShantenContinuationDecision) -> String {
    decision
        .ranked_candidates()
        .into_iter()
        .take(RANKED_CANDIDATE_COUNT)
        .map(|(discard, value)| format!("{}={}", discard.to_mjai_string(), format_value(value)))
        .collect::<Vec<_>>()
        .join(" ")
}

fn selected_label(decision: &ThreeShantenContinuationDecision) -> String {
    decision
        .selected
        .as_ref()
        .map(action_label)
        .unwrap_or_else(|| "none".to_string())
}

fn format_statistics(label: &str, current: &[Duration], progress_only: &[Duration]) -> Vec<String> {
    let a = LatencyStatistics::from_durations(current);
    let b = LatencyStatistics::from_durations(progress_only);
    vec![
        label.to_string(),
        format!("  requests: {}", a.requests),
        format!(
            "  total: A={} B={} ({})",
            format_duration(a.total),
            format_duration(b.total),
            format_speedup(a.total, b.total)
        ),
        format!(
            "  min: A={} B={}",
            format_duration(minimum(current)),
            format_duration(minimum(progress_only))
        ),
        format!(
            "  p50: A={} B={} ({})",
            format_duration(a.p50),
            format_duration(b.p50),
            format_speedup(a.p50, b.p50)
        ),
        format!(
            "  p90: A={} B={} ({})",
            format_duration(a.p90),
            format_duration(b.p90),
            format_speedup(a.p90, b.p90)
        ),
        format!(
            "  max: A={} B={} ({})",
            format_duration(a.max),
            format_duration(b.max),
            format_speedup(a.max, b.max)
        ),
    ]
}

fn minimum(durations: &[Duration]) -> Duration {
    durations.iter().copied().min().unwrap_or(Duration::ZERO)
}

fn scope_label(scope: ThreeShantenContinuationScope) -> &'static str {
    match scope {
        ThreeShantenContinuationScope::Current => "A current",
        ThreeShantenContinuationScope::ProgressOnly => "B progress-only",
    }
}

fn format_duration(duration: Duration) -> String {
    format!("{:.3} ms", duration.as_secs_f64() * 1_000.0)
}

// A / B の比。B が 0 の場合は比を作れないので出さない。
fn format_speedup(current: Duration, progress_only: Duration) -> String {
    if progress_only.is_zero() {
        return "speedup n/a".to_string();
    }
    format!(
        "speedup {:.3}x",
        current.as_secs_f64() / progress_only.as_secs_f64()
    )
}

fn format_ratio(current: u64, progress_only: u64) -> String {
    if current == 0 {
        return "n/a".to_string();
    }
    format!("{:.1}%", progress_only as f64 / current as f64 * 100.0)
}

fn format_ratio_percent(part: usize, total: usize) -> String {
    if total == 0 {
        return "n/a".to_string();
    }
    format!("{:.1}%", part as f64 / total as f64 * 100.0)
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scenario::Scenario;
    use riichilab_client::capture::{self, CaptureDirection};
    use riichilab_client::observation::fixture_base64_with_dora;
    use tempfile::TempDir;

    // 3向聴軸が発火する軽い局面。3479m478p237s + 南南西 で、ツモ切り候補は東。
    const THREE_SHANTEN_HAND: [u8; 13] = [8, 12, 24, 32, 48, 60, 64, 76, 80, 96, 112, 113, 116];
    const THREE_SHANTEN_DRAWN: u8 = 108;

    // CLI と同じ inline baseline で組み立てる。
    fn scenario() -> Scenario {
        let args = ["--hand", "3479m478p237s1223z", "--dora-indicator", "1p"];
        let parsed = crate::cli::CliArgs::parse(args.iter().map(|arg| arg.to_string()))
            .expect("option を読める");
        match parsed.source {
            crate::cli::ScenarioSource::Inline(spec) => {
                Scenario::resolve(&spec).expect("局面を組み立てられる")
            }
            other => panic!("inline scenario ではない: {other:?}"),
        }
    }

    fn capture_line(hand: &[u8], drawn: u8, request_id: u64) -> String {
        let observation = fixture_base64_with_dora(0, Some(drawn), hand.to_vec(), vec![36]);
        let possible: Vec<_> = hand
            .iter()
            .copied()
            .chain([drawn])
            .map(|id| {
                let pai = bot_logic::TileId::new(id).unwrap().to_mjai_string();
                format!(r#"{{"type":"dahai","pai":"{pai}","tsumogiri":false}}"#)
            })
            .collect();
        let line = capture::record_line(
            CaptureDirection::Server,
            &format!(
                r#"{{"type":"request_action","request_id":{request_id},"actor":0,"possible_actions":[{}],"observation":"{observation}"}}"#,
                possible.join(",")
            ),
        )
        .expect("capture 行を作れる");
        format!("{line}\n")
    }

    #[test]
    fn the_scenario_comparison_reports_both_scopes_and_the_search_size() {
        let output = format_scenario_comparison(&scenario());

        assert!(output.contains("A current values"), "{output}");
        assert!(output.contains("B progress-only values"), "{output}");
        assert!(
            output.contains("three-shanten axis fired: true"),
            "{output}"
        );
        assert!(output.contains("same discard: true"), "{output}");
        // SameShanten 枝だけが消え、3->2 と 2->1 の枝数は変わらない。
        assert!(
            output.contains("1-shanten SameShanten branches:"),
            "{output}"
        );
        let same_shanten = output
            .lines()
            .find(|line| {
                line.trim_start()
                    .starts_with("1-shanten SameShanten branches:")
            })
            .expect("枝数の行がある");
        assert!(same_shanten.ends_with("-> 0 (0.0%)"), "{same_shanten}");
        let three_to_two = output
            .lines()
            .find(|line| line.trim_start().starts_with("3->2 draw variants:"))
            .expect("枝数の行がある");
        assert!(three_to_two.ends_with("(100.0%)"), "{three_to_two}");
    }

    #[test]
    fn the_capture_comparison_reports_the_fired_requests() {
        let directory = TempDir::new().expect("一時 directory を作れる");
        let path = directory.path().join("capture.jsonl");
        std::fs::write(
            &path,
            capture_line(&THREE_SHANTEN_HAND, THREE_SHANTEN_DRAWN, 42),
        )
        .expect("capture を書ける");
        let spec = CaptureComparisonSpec {
            paths: vec![path.to_string_lossy().into_owned()],
        };

        let output = run_capture_comparison(&spec).expect("比較できる");

        assert!(
            output.contains("requests with the three-shanten axis fired: 1"),
            "{output}"
        );
        assert!(output.contains("Discard selection elapsed"), "{output}");
        assert!(output.contains("Three-shanten phase elapsed"), "{output}");
        assert!(output.contains("agreement: 100.0%"), "{output}");
        assert!(output.contains("request_id=42"), "{output}");
        assert!(!output.contains("DIFFERENT"), "{output}");
    }

    // 3向聴軸が発火しない1向聴の request。
    const IISHANTEN_HAND: [u8; 13] = [0, 4, 8, 12, 17, 20, 53, 54, 96, 100, 120, 124, 125];
    const IISHANTEN_DRAWN: u8 = 59;

    #[test]
    fn the_capture_comparison_skips_requests_without_the_three_shanten_axis() {
        let directory = TempDir::new().expect("一時 directory を作れる");
        let path = directory.path().join("iishanten.jsonl");
        std::fs::write(&path, capture_line(&IISHANTEN_HAND, IISHANTEN_DRAWN, 1))
            .expect("capture を書ける");
        let spec = CaptureComparisonSpec {
            paths: vec![path.to_string_lossy().into_owned()],
        };

        let output = run_capture_comparison(&spec).expect("比較できる");

        assert!(
            output.contains("requests with the three-shanten axis fired: 0"),
            "{output}"
        );
        assert!(output.contains("agreement: n/a"), "{output}");
    }
}
