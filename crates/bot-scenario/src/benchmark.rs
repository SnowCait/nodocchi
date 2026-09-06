use std::time::{Duration, Instant};

use bot_core::{
    DecisionPhaseDurations, ForwardMetricsPhaseDurations, LegalAction, NormalDiscardPhaseDurations,
    ShantenAgent,
};
use serde::{Deserialize, Serialize};

use crate::cli::CaptureBenchmarkSpec;
use crate::error::ScenarioError;
use crate::format::action_label;
use crate::replay::{CapturedScenario, load_captured_scenarios};

const SLOWEST_REQUEST_COUNT: usize = 20;

const OVER_500MS: Duration = Duration::from_millis(500);
const OVER_1S: Duration = Duration::from_secs(1);
const OVER_2S: Duration = Duration::from_secs(2);
const OVER_3S: Duration = Duration::from_secs(3);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RequestMeasurement {
    pub capture: String,
    pub request_id: u64,
    pub actor: Option<u8>,
    pub elapsed: Duration,
    pub phases: DecisionPhaseDurations,
    pub selected_action: LegalAction,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ThresholdCounts {
    pub over_500ms: usize,
    pub over_1s: usize,
    pub over_2s: usize,
    pub over_3s: usize,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct LatencyStatistics {
    pub requests: usize,
    pub total: Duration,
    pub mean: Duration,
    pub p50: Duration,
    pub p90: Duration,
    pub p95: Duration,
    pub p99: Duration,
    pub max: Duration,
    pub thresholds: ThresholdCounts,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BenchmarkRun {
    pub captures: usize,
    pub requests: Vec<RequestMeasurement>,
    pub statistics: LatencyStatistics,
}

pub fn run_capture_benchmark(spec: &CaptureBenchmarkSpec) -> Result<String, ScenarioError> {
    let run = measure_captures(&spec.paths)?;
    if let Some(path) = spec.json_path.as_deref() {
        write_benchmark_json(path, &run)?;
    }
    Ok(format_benchmark(&run))
}

pub fn measure_captures(paths: &[String]) -> Result<BenchmarkRun, ScenarioError> {
    let mut requests = Vec::new();
    for path in paths {
        for captured in load_captured_scenarios(path)? {
            requests.push(measure_request(&captured));
        }
    }

    let durations = requests
        .iter()
        .map(|measurement| measurement.elapsed)
        .collect::<Vec<_>>();
    Ok(BenchmarkRun {
        captures: paths.len(),
        requests,
        statistics: LatencyStatistics::from_durations(&durations),
    })
}

fn measure_request(captured: &CapturedScenario) -> RequestMeasurement {
    let mut agent = ShantenAgent;
    let context = &captured.scenario.context;
    let legal_actions = captured.scenario.legal_actions.as_slice();

    let start = Instant::now();
    let timed = agent.act_with_phase_timing(context, legal_actions);
    let elapsed = start.elapsed();

    RequestMeasurement {
        capture: captured.path.clone(),
        request_id: captured.request_id,
        actor: captured.actor,
        elapsed,
        phases: timed.phases,
        selected_action: timed.action,
    }
}

impl LatencyStatistics {
    pub fn from_durations(durations: &[Duration]) -> Self {
        let mut sorted = durations.to_vec();
        sorted.sort_unstable();

        let requests = sorted.len();
        let total = sorted.iter().sum::<Duration>();
        Self {
            requests,
            total,
            mean: u32::try_from(requests)
                .ok()
                .and_then(|requests| total.checked_div(requests))
                .unwrap_or(Duration::ZERO),
            p50: nearest_rank_percentile(&sorted, 50),
            p90: nearest_rank_percentile(&sorted, 90),
            p95: nearest_rank_percentile(&sorted, 95),
            p99: nearest_rank_percentile(&sorted, 99),
            max: sorted.last().copied().unwrap_or(Duration::ZERO),
            thresholds: ThresholdCounts {
                over_500ms: count_over(&sorted, OVER_500MS),
                over_1s: count_over(&sorted, OVER_1S),
                over_2s: count_over(&sorted, OVER_2S),
                over_3s: count_over(&sorted, OVER_3S),
            },
        }
    }
}

fn nearest_rank_percentile(sorted_ascending: &[Duration], percentile: usize) -> Duration {
    if sorted_ascending.is_empty() {
        return Duration::ZERO;
    }

    let rank = (percentile * sorted_ascending.len()).div_ceil(100).max(1);
    sorted_ascending[rank.min(sorted_ascending.len()) - 1]
}

fn count_over(sorted_ascending: &[Duration], threshold: Duration) -> usize {
    sorted_ascending.len() - sorted_ascending.partition_point(|elapsed| *elapsed <= threshold)
}

pub fn slowest_requests(run: &BenchmarkRun, count: usize) -> Vec<&RequestMeasurement> {
    let mut slowest = run.requests.iter().collect::<Vec<_>>();
    slowest.sort_by(|left, right| {
        right
            .elapsed
            .cmp(&left.elapsed)
            .then_with(|| left.capture.cmp(&right.capture))
            .then_with(|| left.request_id.cmp(&right.request_id))
    });
    slowest.truncate(count);
    slowest
}

pub fn format_benchmark(run: &BenchmarkRun) -> String {
    let statistics = &run.statistics;
    let mut lines = vec![
        "RiichiLab production latency benchmark".to_string(),
        format!("  captures: {}", run.captures),
        format!("  requests: {}", statistics.requests),
        format!("  total: {}", format_duration(statistics.total)),
        format!("  mean: {}", format_duration(statistics.mean)),
        format!("  p50: {}", format_duration(statistics.p50)),
        format!("  p90: {}", format_duration(statistics.p90)),
        format!("  p95: {}", format_duration(statistics.p95)),
        format!("  p99: {}", format_duration(statistics.p99)),
        format!("  max: {}", format_duration(statistics.max)),
        format!("  > 500 ms: {}", statistics.thresholds.over_500ms),
        format!("  > 1 s: {}", statistics.thresholds.over_1s),
        format!("  > 2 s: {}", statistics.thresholds.over_2s),
        format!("  > 3 s: {}", statistics.thresholds.over_3s),
        String::new(),
        "Slowest requests".to_string(),
    ];

    for measurement in slowest_requests(run, SLOWEST_REQUEST_COUNT) {
        lines.push(format!(
            "  {}  {}  request_id={}  early={}  normal_discard={} ({})  post_discard={}  selected={}",
            format_duration(measurement.elapsed),
            measurement.capture,
            measurement.request_id,
            format_duration(measurement.phases.early),
            format_duration(measurement.phases.normal_discard),
            format_normal_discard_phases(&measurement.phases.normal_discard_phases),
            format_duration(measurement.phases.post_discard),
            action_label(&measurement.selected_action),
        ));
    }

    lines.join("\n")
}

fn format_duration(duration: Duration) -> String {
    format!("{:.3} ms", duration.as_secs_f64() * 1_000.0)
}

// normal discard の内訳は同じ request の normal_discard に括弧で添える。phase 別の集計は出さない。
fn format_normal_discard_phases(phases: &NormalDiscardPhaseDurations) -> String {
    format!(
        "base={} forward={} [{}] two_shanten_self_tsumo={} candidates={} [{}] finalize={}",
        format_duration(phases.base_evaluation),
        format_duration(phases.forward_metrics),
        format_forward_metrics_phases(&phases.forward_metrics_phases),
        format_duration(phases.two_shanten_self_tsumo),
        phases.two_shanten_self_tsumo_candidates.len(),
        phases
            .two_shanten_self_tsumo_candidates
            .iter()
            .map(|candidate| format!(
                "{}={}",
                candidate.discard.to_mjai_string(),
                format_duration(candidate.elapsed)
            ))
            .collect::<Vec<_>>()
            .join(" "),
        format_duration(phases.selection_finalize),
    )
}

// forward metrics の内訳は同じ request の forward に角括弧で添える。
fn format_forward_metrics_phases(phases: &ForwardMetricsPhaseDurations) -> String {
    format!(
        "lookahead_search={} weighted_aggregation={} self_tsumo_continuation={}",
        format_duration(phases.lookahead_search),
        format_duration(phases.weighted_aggregation),
        format_duration(phases.self_tsumo_continuation),
    )
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BenchmarkJson {
    pub summary: BenchmarkSummaryJson,
    pub requests: Vec<BenchmarkRequestJson>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct BenchmarkSummaryJson {
    pub captures: usize,
    pub requests: usize,
    pub total_ns: u64,
    pub mean_ns: u64,
    pub p50_ns: u64,
    pub p90_ns: u64,
    pub p95_ns: u64,
    pub p99_ns: u64,
    pub max_ns: u64,
    pub over_500ms: usize,
    pub over_1s: usize,
    pub over_2s: usize,
    pub over_3s: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BenchmarkRequestJson {
    pub capture: String,
    pub request_id: u64,
    pub actor: Option<u8>,
    pub elapsed_ns: u64,
    pub early_ns: u64,
    pub normal_discard_ns: u64,
    pub normal_discard_base_ns: u64,
    pub normal_discard_forward_ns: u64,
    pub forward_lookahead_search_ns: u64,
    pub forward_weighted_aggregation_ns: u64,
    pub forward_self_tsumo_ns: u64,
    pub two_shanten_self_tsumo_ns: u64,
    pub two_shanten_self_tsumo_candidate_count: usize,
    pub two_shanten_self_tsumo_candidates: Vec<BenchmarkTwoShantenSelfTsumoCandidateJson>,
    pub normal_discard_finalize_ns: u64,
    pub post_discard_ns: u64,
    pub selected: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BenchmarkTwoShantenSelfTsumoCandidateJson {
    pub discard: String,
    pub elapsed_ns: u64,
}

impl BenchmarkJson {
    pub fn from_run(run: &BenchmarkRun) -> Self {
        let statistics = &run.statistics;
        Self {
            summary: BenchmarkSummaryJson {
                captures: run.captures,
                requests: statistics.requests,
                total_ns: nanos(statistics.total),
                mean_ns: nanos(statistics.mean),
                p50_ns: nanos(statistics.p50),
                p90_ns: nanos(statistics.p90),
                p95_ns: nanos(statistics.p95),
                p99_ns: nanos(statistics.p99),
                max_ns: nanos(statistics.max),
                over_500ms: statistics.thresholds.over_500ms,
                over_1s: statistics.thresholds.over_1s,
                over_2s: statistics.thresholds.over_2s,
                over_3s: statistics.thresholds.over_3s,
            },
            requests: run
                .requests
                .iter()
                .map(|measurement| BenchmarkRequestJson {
                    capture: measurement.capture.clone(),
                    request_id: measurement.request_id,
                    actor: measurement.actor,
                    elapsed_ns: nanos(measurement.elapsed),
                    early_ns: nanos(measurement.phases.early),
                    normal_discard_ns: nanos(measurement.phases.normal_discard),
                    normal_discard_base_ns: nanos(
                        measurement.phases.normal_discard_phases.base_evaluation,
                    ),
                    normal_discard_forward_ns: nanos(
                        measurement.phases.normal_discard_phases.forward_metrics,
                    ),
                    forward_lookahead_search_ns: nanos(
                        measurement
                            .phases
                            .normal_discard_phases
                            .forward_metrics_phases
                            .lookahead_search,
                    ),
                    forward_weighted_aggregation_ns: nanos(
                        measurement
                            .phases
                            .normal_discard_phases
                            .forward_metrics_phases
                            .weighted_aggregation,
                    ),
                    forward_self_tsumo_ns: nanos(
                        measurement
                            .phases
                            .normal_discard_phases
                            .forward_metrics_phases
                            .self_tsumo_continuation,
                    ),
                    two_shanten_self_tsumo_ns: nanos(
                        measurement
                            .phases
                            .normal_discard_phases
                            .two_shanten_self_tsumo,
                    ),
                    two_shanten_self_tsumo_candidate_count: measurement
                        .phases
                        .normal_discard_phases
                        .two_shanten_self_tsumo_candidates
                        .len(),
                    two_shanten_self_tsumo_candidates: measurement
                        .phases
                        .normal_discard_phases
                        .two_shanten_self_tsumo_candidates
                        .iter()
                        .map(|candidate| BenchmarkTwoShantenSelfTsumoCandidateJson {
                            discard: candidate.discard.to_mjai_string(),
                            elapsed_ns: nanos(candidate.elapsed),
                        })
                        .collect(),
                    normal_discard_finalize_ns: nanos(
                        measurement.phases.normal_discard_phases.selection_finalize,
                    ),
                    post_discard_ns: nanos(measurement.phases.post_discard),
                    selected: action_label(&measurement.selected_action),
                })
                .collect(),
        }
    }
}

fn nanos(duration: Duration) -> u64 {
    u64::try_from(duration.as_nanos()).unwrap_or(u64::MAX)
}

fn write_benchmark_json(path: &str, run: &BenchmarkRun) -> Result<(), ScenarioError> {
    let mut text =
        serde_json::to_string_pretty(&BenchmarkJson::from_run(run)).map_err(|error| {
            ScenarioError::WriteFile {
                path: path.to_string(),
                message: error.to_string(),
            }
        })?;
    text.push('\n');

    std::fs::write(path, text).map_err(|error| ScenarioError::WriteFile {
        path: path.to_string(),
        message: error.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use bot_core::{Agent, TwoShantenSelfTsumoCandidateDuration};
    use bot_logic::TileId;
    use riichilab_client::observation::{fixture_base64, game_context_from_decoded_observation};
    use riichilab_client::{
        CaptureDirection, MjaiPossibleAction, ObservationPayload, possible_actions_to_legal_actions,
    };

    const CAPTURED_HAND: [u8; 13] = [0, 4, 8, 12, 17, 20, 53, 54, 96, 100, 120, 124, 125];

    const CAPTURED_DRAWN_TILE: u8 = 59;

    const CAPTURED_DAHAI: [&str; 12] = [
        "1m", "2m", "3m", "4m", "5m", "6m", "5p", "6p", "7s", "8s", "N", "P",
    ];

    const SHALLOW_HAND: [u8; 13] = [0, 12, 24, 36, 48, 60, 72, 84, 96, 108, 116, 124, 132];

    const SHALLOW_DRAWN_TILE: u8 = 128;

    const SHALLOW_DAHAI: [&str; 2] = ["F", "1m"];

    fn possible_actions_json(dahai: &[&str]) -> String {
        dahai
            .iter()
            .map(|pai| format!(r#"{{"type":"dahai","pai":"{pai}","tsumogiri":false}}"#))
            .collect::<Vec<_>>()
            .join(",")
    }

    fn observation_base64(hand: &[u8], drawn_tile: u8) -> String {
        fixture_base64(0, Some(drawn_tile), hand.to_vec())
    }

    fn server_record_line(event: &str) -> String {
        riichilab_client::capture::record_line(CaptureDirection::Server, event).unwrap()
    }

    fn client_record_line(event: &str) -> String {
        riichilab_client::capture::record_line(CaptureDirection::Client, event).unwrap()
    }

    fn request_action_line(request_id: u64, hand: &[u8], drawn_tile: u8, dahai: &[&str]) -> String {
        server_record_line(&format!(
            r#"{{"type":"request_action","request_id":{request_id},"actor":0,"possible_actions":[{}],"observation":"{}"}}"#,
            possible_actions_json(dahai),
            observation_base64(hand, drawn_tile)
        ))
    }

    fn shallow_request_action_line(request_id: u64) -> String {
        request_action_line(
            request_id,
            &SHALLOW_HAND,
            SHALLOW_DRAWN_TILE,
            &SHALLOW_DAHAI,
        )
    }

    fn captured_request_action_line(request_id: u64) -> String {
        request_action_line(
            request_id,
            &CAPTURED_HAND,
            CAPTURED_DRAWN_TILE,
            &CAPTURED_DAHAI,
        )
    }

    fn temp_path(name: &str, extension: &str) -> String {
        std::env::temp_dir()
            .join(format!(
                "bot-scenario-benchmark-{name}-{}.{extension}",
                std::process::id()
            ))
            .to_str()
            .unwrap()
            .to_string()
    }

    fn write_capture(name: &str, lines: &[String]) -> String {
        let path = temp_path(name, "jsonl");
        let mut text = lines.join("\n");
        text.push('\n');
        std::fs::write(&path, text).unwrap();
        path
    }

    fn write_requests(name: &str, request_ids: &[u64]) -> String {
        let lines = request_ids
            .iter()
            .map(|request_id| shallow_request_action_line(*request_id))
            .collect::<Vec<_>>();
        write_capture(name, &lines)
    }

    fn request_ids(run: &BenchmarkRun) -> Vec<u64> {
        run.requests
            .iter()
            .map(|measurement| measurement.request_id)
            .collect()
    }

    fn durations(millis: &[u64]) -> Vec<Duration> {
        millis.iter().copied().map(Duration::from_millis).collect()
    }

    fn phases(early: u64, normal_discard: u64, post_discard: u64) -> DecisionPhaseDurations {
        DecisionPhaseDurations {
            early: Duration::from_millis(early),
            normal_discard: Duration::from_millis(normal_discard),
            normal_discard_phases: NormalDiscardPhaseDurations::default(),
            post_discard: Duration::from_millis(post_discard),
        }
    }

    fn phases_with_normal_discard_breakdown(
        early: u64,
        normal_discard: u64,
        post_discard: u64,
        base: u64,
        forward: u64,
        finalize: u64,
    ) -> DecisionPhaseDurations {
        DecisionPhaseDurations {
            normal_discard_phases: NormalDiscardPhaseDurations {
                base_evaluation: Duration::from_millis(base),
                forward_metrics: Duration::from_millis(forward),
                selection_finalize: Duration::from_millis(finalize),
                ..NormalDiscardPhaseDurations::default()
            },
            ..phases(early, normal_discard, post_discard)
        }
    }

    fn with_forward_breakdown(
        phases: DecisionPhaseDurations,
        search: u64,
        aggregate: u64,
        self_tsumo: u64,
    ) -> DecisionPhaseDurations {
        DecisionPhaseDurations {
            normal_discard_phases: NormalDiscardPhaseDurations {
                forward_metrics_phases: ForwardMetricsPhaseDurations {
                    lookahead_search: Duration::from_millis(search),
                    weighted_aggregation: Duration::from_millis(aggregate),
                    self_tsumo_continuation: Duration::from_millis(self_tsumo),
                },
                ..phases.normal_discard_phases
            },
            ..phases
        }
    }

    fn with_two_shanten_breakdown(
        phases: DecisionPhaseDurations,
        total: u64,
        candidates: &[(&str, u64)],
    ) -> DecisionPhaseDurations {
        DecisionPhaseDurations {
            normal_discard_phases: NormalDiscardPhaseDurations {
                two_shanten_self_tsumo: Duration::from_millis(total),
                two_shanten_self_tsumo_candidates: candidates
                    .iter()
                    .map(|(discard, elapsed)| TwoShantenSelfTsumoCandidateDuration {
                        discard: bot_logic::TileType::from_mjai_type_str(discard).unwrap(),
                        elapsed: Duration::from_millis(*elapsed),
                    })
                    .collect(),
                ..phases.normal_discard_phases
            },
            ..phases
        }
    }

    fn measurement(capture: &str, request_id: u64, millis: u64) -> RequestMeasurement {
        measurement_with_phases(
            capture,
            request_id,
            millis,
            DecisionPhaseDurations::default(),
        )
    }

    fn measurement_with_phases(
        capture: &str,
        request_id: u64,
        millis: u64,
        phases: DecisionPhaseDurations,
    ) -> RequestMeasurement {
        RequestMeasurement {
            capture: capture.to_string(),
            request_id,
            actor: Some(0),
            elapsed: Duration::from_millis(millis),
            phases,
            selected_action: LegalAction::Dahai {
                tile: TileId::new(0).unwrap(),
            },
        }
    }

    fn synthetic_run(requests: Vec<RequestMeasurement>) -> BenchmarkRun {
        let durations = requests
            .iter()
            .map(|measurement| measurement.elapsed)
            .collect::<Vec<_>>();
        BenchmarkRun {
            captures: 1,
            requests,
            statistics: LatencyStatistics::from_durations(&durations),
        }
    }

    #[test]
    fn measures_every_captured_request_of_a_file() {
        let path = write_requests("batch", &[401, 402, 403]);
        let run = measure_captures(std::slice::from_ref(&path)).unwrap();
        let _ = std::fs::remove_file(&path);

        assert_eq!(run.captures, 1);
        assert_eq!(request_ids(&run), vec![401, 402, 403]);
        assert_eq!(run.statistics.requests, 3);
    }

    #[test]
    fn measures_multiple_captures_in_one_run() {
        let first = write_requests("multiple-first", &[411, 412]);
        let second = write_requests("multiple-second", &[421]);
        let run = measure_captures(&[first.clone(), second.clone()]).unwrap();
        let _ = std::fs::remove_file(&first);
        let _ = std::fs::remove_file(&second);

        assert_eq!(run.captures, 2);
        assert_eq!(request_ids(&run), vec![411, 412, 421]);
        assert_eq!(run.statistics.requests, 3);
    }

    #[test]
    fn measurements_keep_the_capture_path_and_the_request_identity() {
        let first = write_requests("identity-first", &[451]);
        let second = write_requests("identity-second", &[452, 453]);
        let run = measure_captures(&[first.clone(), second.clone()]).unwrap();
        let _ = std::fs::remove_file(&first);
        let _ = std::fs::remove_file(&second);

        let captures = run
            .requests
            .iter()
            .map(|measurement| measurement.capture.as_str())
            .collect::<Vec<_>>();
        assert_eq!(
            captures,
            vec![first.as_str(), second.as_str(), second.as_str()]
        );
        assert_eq!(request_ids(&run), vec![451, 452, 453]);
        assert!(
            run.requests
                .iter()
                .all(|measurement| measurement.actor == Some(0))
        );
    }

    #[test]
    fn measured_selection_is_the_production_agent_decision() {
        let path = write_capture("production", &[captured_request_action_line(431)]);
        let run = measure_captures(std::slice::from_ref(&path)).unwrap();
        let _ = std::fs::remove_file(&path);

        let decoded =
            ObservationPayload::new(observation_base64(&CAPTURED_HAND, CAPTURED_DRAWN_TILE))
                .decode_4p()
                .unwrap();
        let context = game_context_from_decoded_observation(&decoded);
        let possible_actions: Vec<MjaiPossibleAction> =
            serde_json::from_str(&format!("[{}]", possible_actions_json(&CAPTURED_DAHAI))).unwrap();
        let legal_actions = possible_actions_to_legal_actions(&possible_actions);

        let mut agent = ShantenAgent;
        assert_eq!(
            run.requests[0].selected_action,
            agent.act(&context, &legal_actions)
        );
        assert!(
            matches!(run.requests[0].selected_action, LegalAction::Dahai { .. }),
            "{:?}",
            run.requests[0].selected_action
        );
    }

    #[test]
    fn benchmark_does_not_use_the_diagnostic_api() {
        let production = include_str!("benchmark.rs")
            .split("#[cfg(test)]")
            .next()
            .unwrap();

        assert!(!production.contains("diagnose"), "{production}");
        assert!(!production.contains("Diagnostic"), "{production}");
    }

    #[test]
    fn statistics_use_nearest_rank_percentiles() {
        let statistics =
            LatencyStatistics::from_durations(&durations(&(1..=100).collect::<Vec<_>>()));

        assert_eq!(statistics.requests, 100);
        assert_eq!(statistics.total, Duration::from_millis(5050));
        assert_eq!(statistics.mean, Duration::from_micros(50_500));
        assert_eq!(statistics.p50, Duration::from_millis(50));
        assert_eq!(statistics.p90, Duration::from_millis(90));
        assert_eq!(statistics.p95, Duration::from_millis(95));
        assert_eq!(statistics.p99, Duration::from_millis(99));
        assert_eq!(statistics.max, Duration::from_millis(100));
    }

    #[test]
    fn statistics_sort_the_durations_before_summarizing_them() {
        let statistics = LatencyStatistics::from_durations(&durations(&[3000, 10, 700]));

        assert_eq!(statistics.total, Duration::from_millis(3710));
        assert_eq!(statistics.mean, Duration::from_nanos(1_236_666_666));
        assert_eq!(statistics.p50, Duration::from_millis(700));
        assert_eq!(statistics.p90, Duration::from_millis(3000));
        assert_eq!(statistics.p95, Duration::from_millis(3000));
        assert_eq!(statistics.p99, Duration::from_millis(3000));
        assert_eq!(statistics.max, Duration::from_millis(3000));
    }

    #[test]
    fn percentiles_of_a_single_request_are_that_request() {
        let statistics = LatencyStatistics::from_durations(&durations(&[1234]));

        assert_eq!(statistics.p50, Duration::from_millis(1234));
        assert_eq!(statistics.p99, Duration::from_millis(1234));
        assert_eq!(statistics.max, Duration::from_millis(1234));
        assert_eq!(statistics.mean, Duration::from_millis(1234));
    }

    #[test]
    fn statistics_of_no_request_are_zero() {
        let statistics = LatencyStatistics::from_durations(&[]);

        assert_eq!(statistics, LatencyStatistics::default());
        assert_eq!(statistics.requests, 0);
        assert_eq!(statistics.mean, Duration::ZERO);
    }

    #[test]
    fn threshold_counts_are_strictly_over_the_threshold() {
        let statistics = LatencyStatistics::from_durations(&durations(&[
            500, 501, 1000, 1001, 2000, 2001, 3000, 3001,
        ]));

        assert_eq!(
            statistics.thresholds,
            ThresholdCounts {
                over_500ms: 7,
                over_1s: 5,
                over_2s: 3,
                over_3s: 1,
            }
        );
    }

    #[test]
    fn slowest_requests_are_sorted_by_elapsed_descending() {
        let run = synthetic_run(vec![
            measurement("game-001.jsonl", 1, 10),
            measurement("game-002.jsonl", 2, 2470),
            measurement("game-003.jsonl", 3, 700),
        ]);

        let slowest = slowest_requests(&run, 3);
        assert_eq!(
            slowest
                .iter()
                .map(|measurement| measurement.request_id)
                .collect::<Vec<_>>(),
            vec![2, 3, 1]
        );
        assert_eq!(
            slowest_requests(&run, 2)
                .iter()
                .map(|measurement| measurement.elapsed)
                .collect::<Vec<_>>(),
            durations(&[2470, 700])
        );
    }

    #[test]
    fn slowest_requests_of_equal_elapsed_keep_a_deterministic_order() {
        let run = synthetic_run(vec![
            measurement("game-002.jsonl", 9, 100),
            measurement("game-001.jsonl", 8, 100),
            measurement("game-001.jsonl", 7, 100),
        ]);

        assert_eq!(
            slowest_requests(&run, 3)
                .iter()
                .map(|measurement| (measurement.capture.as_str(), measurement.request_id))
                .collect::<Vec<_>>(),
            vec![
                ("game-001.jsonl", 7),
                ("game-001.jsonl", 8),
                ("game-002.jsonl", 9),
            ]
        );
    }

    #[test]
    fn report_shows_the_statistics_and_the_slowest_requests() {
        let run = synthetic_run(vec![
            measurement("game-001.jsonl", 1, 10),
            measurement_with_phases(
                "game-002.jsonl",
                2,
                2470,
                with_two_shanten_breakdown(
                    with_forward_breakdown(
                        phases_with_normal_discard_breakdown(1, 2400, 69, 30, 2000, 20),
                        1950,
                        30,
                        20,
                    ),
                    350,
                    &[("5m", 180), ("8m", 160)],
                ),
            ),
        ]);
        let report = format_benchmark(&run);

        assert!(
            report.starts_with("RiichiLab production latency benchmark\n"),
            "{report}"
        );
        assert!(report.contains("\n  captures: 1\n"), "{report}");
        assert!(report.contains("\n  requests: 2\n"), "{report}");
        assert!(report.contains("\n  total: 2480.000 ms\n"), "{report}");
        assert!(report.contains("\n  mean: 1240.000 ms\n"), "{report}");
        assert!(report.contains("\n  p50: 10.000 ms\n"), "{report}");
        assert!(report.contains("\n  max: 2470.000 ms\n"), "{report}");
        assert!(report.contains("\n  > 500 ms: 1\n"), "{report}");
        assert!(report.contains("\n  > 1 s: 1\n"), "{report}");
        assert!(report.contains("\n  > 2 s: 1\n"), "{report}");
        assert!(report.contains("\n  > 3 s: 0\n"), "{report}");

        let slowest = report.split("\n\nSlowest requests\n").nth(1).unwrap();
        assert_eq!(
            slowest,
            "  2470.000 ms  game-002.jsonl  request_id=2  early=1.000 ms  normal_discard=2400.000 ms (base=30.000 ms forward=2000.000 ms [lookahead_search=1950.000 ms weighted_aggregation=30.000 ms self_tsumo_continuation=20.000 ms] two_shanten_self_tsumo=350.000 ms candidates=2 [5m=180.000 ms 8m=160.000 ms] finalize=20.000 ms)  post_discard=69.000 ms  selected=1m\n  10.000 ms  game-001.jsonl  request_id=1  early=0.000 ms  normal_discard=0.000 ms (base=0.000 ms forward=0.000 ms [lookahead_search=0.000 ms weighted_aggregation=0.000 ms self_tsumo_continuation=0.000 ms] two_shanten_self_tsumo=0.000 ms candidates=0 [] finalize=0.000 ms)  post_discard=0.000 ms  selected=1m"
        );
    }

    #[test]
    fn report_lists_at_most_the_slowest_request_count() {
        let run = synthetic_run(
            (0..SLOWEST_REQUEST_COUNT as u64 + 5)
                .map(|index| measurement("game-001.jsonl", index, index))
                .collect(),
        );
        let report = format_benchmark(&run);
        let slowest = report.split("\n\nSlowest requests\n").nth(1).unwrap();

        assert_eq!(slowest.lines().count(), SLOWEST_REQUEST_COUNT);
    }

    #[test]
    fn benchmark_json_keeps_the_summary_and_every_request() {
        let run = synthetic_run(vec![
            measurement("game-001.jsonl", 1, 10),
            measurement_with_phases(
                "game-002.jsonl",
                2,
                2470,
                with_two_shanten_breakdown(
                    with_forward_breakdown(
                        phases_with_normal_discard_breakdown(1, 2400, 69, 30, 2000, 20),
                        1950,
                        30,
                        20,
                    ),
                    350,
                    &[("5m", 180), ("8m", 160)],
                ),
            ),
        ]);
        let json = BenchmarkJson::from_run(&run);

        assert_eq!(json.summary.captures, 1);
        assert_eq!(json.summary.requests, 2);
        assert_eq!(json.summary.total_ns, 2_480_000_000);
        assert_eq!(json.summary.mean_ns, 1_240_000_000);
        assert_eq!(json.summary.p50_ns, 10_000_000);
        assert_eq!(json.summary.p90_ns, 2_470_000_000);
        assert_eq!(json.summary.p95_ns, 2_470_000_000);
        assert_eq!(json.summary.p99_ns, 2_470_000_000);
        assert_eq!(json.summary.max_ns, 2_470_000_000);
        assert_eq!(json.summary.over_2s, 1);
        assert_eq!(json.summary.over_3s, 0);
        assert_eq!(
            json.requests,
            vec![
                BenchmarkRequestJson {
                    capture: "game-001.jsonl".to_string(),
                    request_id: 1,
                    actor: Some(0),
                    elapsed_ns: 10_000_000,
                    early_ns: 0,
                    normal_discard_ns: 0,
                    normal_discard_base_ns: 0,
                    normal_discard_forward_ns: 0,
                    forward_lookahead_search_ns: 0,
                    forward_weighted_aggregation_ns: 0,
                    forward_self_tsumo_ns: 0,
                    two_shanten_self_tsumo_ns: 0,
                    two_shanten_self_tsumo_candidate_count: 0,
                    two_shanten_self_tsumo_candidates: vec![],
                    normal_discard_finalize_ns: 0,
                    post_discard_ns: 0,
                    selected: "1m".to_string(),
                },
                BenchmarkRequestJson {
                    capture: "game-002.jsonl".to_string(),
                    request_id: 2,
                    actor: Some(0),
                    elapsed_ns: 2_470_000_000,
                    early_ns: 1_000_000,
                    normal_discard_ns: 2_400_000_000,
                    normal_discard_base_ns: 30_000_000,
                    normal_discard_forward_ns: 2_000_000_000,
                    forward_lookahead_search_ns: 1_950_000_000,
                    forward_weighted_aggregation_ns: 30_000_000,
                    forward_self_tsumo_ns: 20_000_000,
                    two_shanten_self_tsumo_ns: 350_000_000,
                    two_shanten_self_tsumo_candidate_count: 2,
                    two_shanten_self_tsumo_candidates: vec![
                        BenchmarkTwoShantenSelfTsumoCandidateJson {
                            discard: "5m".to_string(),
                            elapsed_ns: 180_000_000,
                        },
                        BenchmarkTwoShantenSelfTsumoCandidateJson {
                            discard: "8m".to_string(),
                            elapsed_ns: 160_000_000,
                        },
                    ],
                    normal_discard_finalize_ns: 20_000_000,
                    post_discard_ns: 69_000_000,
                    selected: "1m".to_string(),
                },
            ]
        );

        let text = serde_json::to_string(&json).unwrap();
        assert_eq!(serde_json::from_str::<BenchmarkJson>(&text).unwrap(), json);
    }

    #[test]
    fn an_early_return_request_keeps_the_phases_it_never_reached_at_zero() {
        let observation = fixture_base64(0, Some(CAPTURED_DRAWN_TILE), CAPTURED_HAND.to_vec());
        let path = write_capture(
            "early-return",
            &[server_record_line(&format!(
                r#"{{"type":"request_action","request_id":482,"actor":0,"possible_actions":[{{"type":"hora"}},{{"type":"none"}}],"observation":"{observation}"}}"#
            ))],
        );
        let run = measure_captures(std::slice::from_ref(&path)).unwrap();
        let _ = std::fs::remove_file(&path);

        assert_eq!(run.requests[0].selected_action, LegalAction::Hora);
        assert_eq!(run.requests[0].phases.normal_discard, Duration::ZERO);
        assert_eq!(run.requests[0].phases.post_discard, Duration::ZERO);
    }

    #[test]
    fn the_measured_selection_is_the_same_with_and_without_phase_timing() {
        let decoded =
            ObservationPayload::new(observation_base64(&CAPTURED_HAND, CAPTURED_DRAWN_TILE))
                .decode_4p()
                .unwrap();
        let context = game_context_from_decoded_observation(&decoded);
        let possible_actions: Vec<MjaiPossibleAction> =
            serde_json::from_str(&format!("[{}]", possible_actions_json(&CAPTURED_DAHAI))).unwrap();
        let legal_actions = possible_actions_to_legal_actions(&possible_actions);

        let mut timed_agent = ShantenAgent;
        let mut untimed_agent = ShantenAgent;
        assert_eq!(
            timed_agent
                .act_with_phase_timing(&context, &legal_actions)
                .action,
            untimed_agent.act(&context, &legal_actions)
        );
    }

    #[test]
    fn benchmark_json_keeps_the_phase_timing_of_every_request() {
        let path = write_requests("json-phase-timing", &[483]);
        let run = measure_captures(std::slice::from_ref(&path)).unwrap();
        let json = BenchmarkJson::from_run(&run);
        let json_path = temp_path("json-phase-timing", "json");
        write_benchmark_json(&json_path, &run).unwrap();
        let text = std::fs::read_to_string(&json_path).unwrap();
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(&json_path);

        let measurement = &run.requests[0];
        let request = &json.requests[0];
        assert_eq!(request.request_id, 483);
        assert_eq!(request.early_ns, nanos(measurement.phases.early));
        assert_eq!(
            request.normal_discard_ns,
            nanos(measurement.phases.normal_discard)
        );
        assert_eq!(
            request.post_discard_ns,
            nanos(measurement.phases.post_discard)
        );

        assert!(text.contains("\"early_ns\""), "{text}");
        assert!(text.contains("\"normal_discard_ns\""), "{text}");
        assert!(text.contains("\"post_discard_ns\""), "{text}");
        assert_eq!(serde_json::from_str::<BenchmarkJson>(&text).unwrap(), json);
    }

    #[test]
    fn benchmark_json_keeps_the_normal_discard_subphases_of_every_request() {
        let path = write_requests("json-normal-discard-subphases", &[484]);
        let run = measure_captures(std::slice::from_ref(&path)).unwrap();
        let json = BenchmarkJson::from_run(&run);
        let json_path = temp_path("json-normal-discard-subphases", "json");
        write_benchmark_json(&json_path, &run).unwrap();
        let text = std::fs::read_to_string(&json_path).unwrap();
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(&json_path);

        let phases = &run.requests[0].phases.normal_discard_phases;
        let request = &json.requests[0];
        assert_eq!(request.request_id, 484);
        assert_eq!(
            request.normal_discard_base_ns,
            nanos(phases.base_evaluation)
        );
        assert_eq!(
            request.normal_discard_forward_ns,
            nanos(phases.forward_metrics)
        );
        assert_eq!(
            request.two_shanten_self_tsumo_ns,
            nanos(phases.two_shanten_self_tsumo)
        );
        assert_eq!(
            request.two_shanten_self_tsumo_candidate_count,
            phases.two_shanten_self_tsumo_candidates.len()
        );
        assert_eq!(
            request.two_shanten_self_tsumo_candidates.len(),
            phases.two_shanten_self_tsumo_candidates.len()
        );
        assert_eq!(
            request.normal_discard_finalize_ns,
            nanos(phases.selection_finalize)
        );

        assert!(text.contains("\"normal_discard_base_ns\""), "{text}");
        assert!(text.contains("\"normal_discard_forward_ns\""), "{text}");
        assert!(text.contains("\"two_shanten_self_tsumo_ns\""), "{text}");
        assert!(
            text.contains("\"two_shanten_self_tsumo_candidate_count\""),
            "{text}"
        );
        assert!(
            text.contains("\"two_shanten_self_tsumo_candidates\""),
            "{text}"
        );
        assert!(text.contains("\"normal_discard_finalize_ns\""), "{text}");
        assert_eq!(serde_json::from_str::<BenchmarkJson>(&text).unwrap(), json);
    }

    #[test]
    fn benchmark_json_keeps_the_forward_subphases_of_every_request() {
        let path = write_requests("json-forward-subphases", &[486]);
        let run = measure_captures(std::slice::from_ref(&path)).unwrap();
        let json = BenchmarkJson::from_run(&run);
        let json_path = temp_path("json-forward-subphases", "json");
        write_benchmark_json(&json_path, &run).unwrap();
        let text = std::fs::read_to_string(&json_path).unwrap();
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(&json_path);

        let phases = &run.requests[0]
            .phases
            .normal_discard_phases
            .forward_metrics_phases;
        let request = &json.requests[0];
        assert_eq!(request.request_id, 486);
        assert_eq!(
            request.forward_lookahead_search_ns,
            nanos(phases.lookahead_search)
        );
        assert_eq!(
            request.forward_weighted_aggregation_ns,
            nanos(phases.weighted_aggregation)
        );
        assert_eq!(
            request.forward_self_tsumo_ns,
            nanos(phases.self_tsumo_continuation)
        );

        assert!(text.contains("\"forward_lookahead_search_ns\""), "{text}");
        assert!(
            !text.contains(&["forward_", "candidate_search_ns"].concat()),
            "{text}"
        );
        assert!(
            text.contains("\"forward_weighted_aggregation_ns\""),
            "{text}"
        );
        assert!(text.contains("\"forward_self_tsumo_ns\""), "{text}");
        assert_eq!(serde_json::from_str::<BenchmarkJson>(&text).unwrap(), json);
    }

    #[test]
    fn an_early_return_request_keeps_the_forward_subphases_at_zero() {
        let observation = fixture_base64(0, Some(CAPTURED_DRAWN_TILE), CAPTURED_HAND.to_vec());
        let path = write_capture(
            "early-return-forward-subphases",
            &[server_record_line(&format!(
                r#"{{"type":"request_action","request_id":487,"actor":0,"possible_actions":[{{"type":"hora"}},{{"type":"none"}}],"observation":"{observation}"}}"#
            ))],
        );
        let run = measure_captures(std::slice::from_ref(&path)).unwrap();
        let _ = std::fs::remove_file(&path);

        assert_eq!(run.requests[0].selected_action, LegalAction::Hora);
        assert_eq!(
            run.requests[0]
                .phases
                .normal_discard_phases
                .forward_metrics_phases,
            ForwardMetricsPhaseDurations::default()
        );
    }

    #[test]
    fn an_early_return_request_keeps_the_normal_discard_subphases_at_zero() {
        let observation = fixture_base64(0, Some(CAPTURED_DRAWN_TILE), CAPTURED_HAND.to_vec());
        let path = write_capture(
            "early-return-subphases",
            &[server_record_line(&format!(
                r#"{{"type":"request_action","request_id":485,"actor":0,"possible_actions":[{{"type":"hora"}},{{"type":"none"}}],"observation":"{observation}"}}"#
            ))],
        );
        let run = measure_captures(std::slice::from_ref(&path)).unwrap();
        let _ = std::fs::remove_file(&path);

        assert_eq!(run.requests[0].selected_action, LegalAction::Hora);
        assert_eq!(
            run.requests[0].phases.normal_discard_phases,
            NormalDiscardPhaseDurations::default()
        );
    }

    #[test]
    fn writes_the_machine_readable_output_of_a_capture_benchmark() {
        let path = write_requests("json-output", &[441, 442]);
        let json_path = temp_path("json-output", "json");
        let report = run_capture_benchmark(&CaptureBenchmarkSpec {
            paths: vec![path.clone()],
            json_path: Some(json_path.clone()),
        })
        .unwrap();
        let text = std::fs::read_to_string(&json_path).unwrap();
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(&json_path);

        let json: BenchmarkJson = serde_json::from_str(&text).unwrap();
        assert_eq!(json.summary.captures, 1);
        assert_eq!(json.summary.requests, 2);
        assert_eq!(
            json.requests
                .iter()
                .map(|request| (request.capture.as_str(), request.request_id))
                .collect::<Vec<_>>(),
            vec![(path.as_str(), 441), (path.as_str(), 442)]
        );
        assert!(report.contains("\n  requests: 2\n"), "{report}");
    }

    #[test]
    fn a_capture_benchmark_without_json_output_writes_no_file() {
        let path = write_requests("no-json-output", &[443]);
        let report = run_capture_benchmark(&CaptureBenchmarkSpec {
            paths: vec![path.clone()],
            json_path: None,
        })
        .unwrap();
        let _ = std::fs::remove_file(&path);

        assert!(report.contains("\n  requests: 1\n"), "{report}");
    }

    #[test]
    fn a_malformed_record_fails_the_whole_benchmark() {
        let path = write_capture(
            "malformed",
            &[
                shallow_request_action_line(461),
                r#"{"type":"action_ack","request_id":461,"status":"accepted"}"#.to_string(),
                shallow_request_action_line(462),
            ],
        );
        let error = measure_captures(std::slice::from_ref(&path)).unwrap_err();
        let _ = std::fs::remove_file(&path);

        assert!(
            matches!(&error, ScenarioError::CaptureRecord { line, .. } if *line == 2),
            "{error:?}"
        );
    }

    #[test]
    fn measures_only_the_request_actions_of_a_session_capture() {
        let path = write_capture(
            "session",
            &[
                server_record_line(r#"{"type":"start_kyoku","kyoku":1}"#),
                shallow_request_action_line(463),
                client_record_line(r#"{"type":"dahai","actor":0,"pai":"1m","request_id":463}"#),
                server_record_line(r#"{"type":"action_ack","request_id":463,"status":"accepted"}"#),
                shallow_request_action_line(464),
                server_record_line(r#"{"type":"end_game","scores":[25000,25000,25000,25000]}"#),
            ],
        );
        let run = measure_captures(std::slice::from_ref(&path)).unwrap();
        let _ = std::fs::remove_file(&path);

        assert_eq!(
            run.requests
                .iter()
                .map(|measurement| measurement.request_id)
                .collect::<Vec<_>>(),
            [463, 464]
        );
    }

    #[test]
    fn benchmark_uses_the_replay_context_with_its_reaction_source() {
        let observation = fixture_base64(0, None, CAPTURED_HAND.to_vec());
        let request = server_record_line(&format!(
            r#"{{"type":"request_action","request_id":465,"actor":0,"possible_actions":[{{"type":"none"}}],"observation":"{observation}"}}"#
        ));
        let path = write_capture(
            "reaction-source",
            &[
                server_record_line(r#"{"type":"dahai","actor":2,"pai":"4s"}"#),
                request,
            ],
        );

        let captured = load_captured_scenarios(&path).unwrap();
        let run = measure_captures(std::slice::from_ref(&path)).unwrap();
        let _ = std::fs::remove_file(&path);

        assert_eq!(
            captured[0].scenario.context.reaction_source_player(),
            Some(2)
        );
        assert_eq!(run.requests.len(), 1);
        assert_eq!(run.requests[0].selected_action, LegalAction::None);
    }

    #[test]
    fn an_undecodable_observation_fails_the_whole_benchmark() {
        let line = server_record_line(&format!(
            r#"{{"type":"request_action","request_id":471,"possible_actions":[{}],"observation":"not-base64!!"}}"#,
            possible_actions_json(&SHALLOW_DAHAI)
        ));
        let path = write_capture("undecodable", &[shallow_request_action_line(470), line]);
        let error = measure_captures(std::slice::from_ref(&path)).unwrap_err();
        let _ = std::fs::remove_file(&path);

        assert!(
            matches!(&error, ScenarioError::CaptureObservation { request_id, .. } if *request_id == 471),
            "{error:?}"
        );
    }

    #[test]
    fn an_empty_capture_fails_the_benchmark() {
        let path = write_capture("empty", &[]);
        let error = measure_captures(std::slice::from_ref(&path)).unwrap_err();
        let _ = std::fs::remove_file(&path);

        assert_eq!(error, ScenarioError::EmptyCapture { path });
    }

    #[test]
    fn a_missing_capture_file_fails_the_benchmark() {
        let error = measure_captures(&["missing-benchmark-capture.jsonl".to_string()]).unwrap_err();

        assert!(
            matches!(&error, ScenarioError::ReadFile { path, .. } if path == "missing-benchmark-capture.jsonl"),
            "{error:?}"
        );
    }
}
