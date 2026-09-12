use std::time::{Duration, Instant};

use bot_core::{
    CallCandidateDuration, CallDecisionDurations, DecisionPhaseDurations,
    ForwardMetricsPhaseDurations, IishantenForwardCandidateDuration, LegalAction,
    NormalDiscardPhaseDurations, ShantenAgent,
};
use bot_logic::TileType;
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
    pub two_shanten_self_tsumo_candidates: Vec<(TileType, Duration)>,
    pub iishanten_forward_candidates: Vec<IishantenForwardCandidateDuration>,
    pub call_candidates: Vec<CallCandidateDuration>,
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
        two_shanten_self_tsumo_candidates: timed.two_shanten_self_tsumo_candidates().collect(),
        iishanten_forward_candidates: timed.iishanten_forward_candidates().to_vec(),
        call_candidates: timed.call_candidates().to_vec(),
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
            "  {}  {}  request_id={}  early={} ({})  normal_discard={} ({})  post_discard={}  selected={}",
            format_duration(measurement.elapsed),
            measurement.capture,
            measurement.request_id,
            format_duration(measurement.phases.early),
            format_call_phases(&measurement.phases.call, &measurement.call_candidates),
            format_duration(measurement.phases.normal_discard),
            format_normal_discard_phases(
                &measurement.phases.normal_discard_phases,
                &measurement.two_shanten_self_tsumo_candidates,
                &measurement.iishanten_forward_candidates,
            ),
            format_duration(measurement.phases.post_discard),
            action_label(&measurement.selected_action),
        ));
    }

    lines.join("\n")
}

fn format_duration(duration: Duration) -> String {
    format!("{:.3} ms", duration.as_secs_f64() * 1_000.0)
}

// 鳴き判断の内訳は同じ request の early に括弧で添える。鳴き候補が無い request では 0 が並ぶ。
fn format_call_phases(
    durations: &CallDecisionDurations,
    candidates: &[CallCandidateDuration],
) -> String {
    format!(
        "call={} call_candidates={} count={} [{}] call_pass={} call_remaining={}",
        format_duration(durations.total),
        format_duration(durations.candidates),
        candidates.len(),
        candidates
            .iter()
            .map(format_call_candidate)
            .collect::<Vec<_>>()
            .join(" "),
        format_duration(durations.pass_iishanten_self_tsumo),
        format_duration(durations.remaining()),
    )
}

// semantic に同一な先行候補の結果を再利用した候補は、実測 0 が「速かった」と読めてしまうので
// reused を明示する。候補そのものは合法 action の列挙順で残す。
fn format_call_candidate(candidate: &CallCandidateDuration) -> String {
    format!(
        "{}={} post_call_discard={}{}",
        call_candidate_label(candidate),
        format_duration(candidate.elapsed),
        format_duration(candidate.post_call_discard_selection),
        if candidate.reused { " reused" } else { "" },
    )
}

fn call_candidate_label(candidate: &CallCandidateDuration) -> String {
    format!(
        "{:?}({}<-{})",
        candidate.kind,
        candidate.tile.to_mjai_string(),
        candidate
            .consumed
            .iter()
            .map(|tile| tile.to_mjai_string())
            .collect::<Vec<_>>()
            .join(","),
    )
}

// normal discard の内訳は同じ request の normal_discard に括弧で添える。phase 別の集計は出さない。
fn format_normal_discard_phases(
    phases: &NormalDiscardPhaseDurations,
    candidates: &[(TileType, Duration)],
    forward_candidates: &[IishantenForwardCandidateDuration],
) -> String {
    format!(
        "base={} forward={} [{}] forward_candidates={} [{}] two_shanten_self_tsumo={} candidates={} [{}] three_shanten_self_tsumo={} finalize={}",
        format_duration(phases.base_evaluation),
        format_duration(phases.forward_metrics),
        format_forward_metrics_phases(&phases.forward_metrics_phases),
        forward_candidates.len(),
        forward_candidates
            .iter()
            .map(format_iishanten_forward_candidate)
            .collect::<Vec<_>>()
            .join(" "),
        format_duration(phases.two_shanten_self_tsumo),
        candidates.len(),
        candidates
            .iter()
            .map(|(discard, elapsed)| format!(
                "{}={}",
                discard.to_mjai_string(),
                format_duration(*elapsed)
            ))
            .collect::<Vec<_>>()
            .join(" "),
        format_duration(phases.three_shanten_self_tsumo),
        format_duration(phases.selection_finalize),
    )
}

// 1向聴の深い前方評価を実際に行った候補は、production の候補順そのままで並べる。候補単位で
// 並行に評価するため、`elapsed` の合計は forward の壁時計を超え得る。
fn format_iishanten_forward_candidate(candidate: &IishantenForwardCandidateDuration) -> String {
    format!(
        "{}={} [lookahead_search={} weighted_aggregation={} self_tsumo_continuation={}]",
        candidate.discard.to_mjai_string(),
        format_duration(candidate.elapsed),
        format_duration(candidate.phases.lookahead_search),
        format_duration(candidate.phases.weighted_aggregation),
        format_duration(candidate.phases.self_tsumo_continuation),
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
    #[serde(default)]
    pub call_ns: u64,
    #[serde(default)]
    pub call_candidates_ns: u64,
    #[serde(default)]
    pub call_pass_iishanten_self_tsumo_ns: u64,
    #[serde(default)]
    pub call_remaining_ns: u64,
    #[serde(default)]
    pub call_candidate_count: usize,
    #[serde(default)]
    pub call_candidates: Vec<BenchmarkCallCandidateJson>,
    pub normal_discard_ns: u64,
    pub normal_discard_base_ns: u64,
    pub normal_discard_forward_ns: u64,
    pub forward_lookahead_search_ns: u64,
    pub forward_weighted_aggregation_ns: u64,
    pub forward_self_tsumo_ns: u64,
    #[serde(default)]
    pub iishanten_forward_candidate_count: usize,
    #[serde(default)]
    pub iishanten_forward_candidates: Vec<BenchmarkIishantenForwardCandidateJson>,
    pub two_shanten_self_tsumo_ns: u64,
    pub two_shanten_self_tsumo_candidate_count: usize,
    pub two_shanten_self_tsumo_candidates: Vec<BenchmarkTwoShantenSelfTsumoCandidateJson>,
    pub three_shanten_self_tsumo_ns: u64,
    pub normal_discard_finalize_ns: u64,
    pub post_discard_ns: u64,
    pub selected: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BenchmarkCallCandidateJson {
    pub kind: String,
    pub tile: String,
    pub consumed: Vec<String>,
    pub elapsed_ns: u64,
    pub post_call_discard_selection_ns: u64,
    #[serde(default)]
    pub reused: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BenchmarkTwoShantenSelfTsumoCandidateJson {
    pub discard: String,
    pub elapsed_ns: u64,
}

/// production が1向聴の深い前方評価を実際に行った候補1件。
///
/// 並ぶのは深い前方評価の対象になった候補だけで、順序は production の候補順そのまま。
/// `elapsed_ns` はその候補を評価していた実時間で、候補単位で並行に評価するため、合計は
/// `normal_discard_forward_ns` の壁時計を超え得る。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BenchmarkIishantenForwardCandidateJson {
    pub discard: String,
    pub elapsed_ns: u64,
    pub lookahead_search_ns: u64,
    pub weighted_aggregation_ns: u64,
    pub self_tsumo_continuation_ns: u64,
    /// 候補1件の評価が使った探索内の同一 state memo の利用数。
    pub search_state_memo_hits: u64,
    pub search_state_memo_misses: u64,
    /// 候補1件の評価が引いた未来テンパイの値 memo の利用数。miss が実際に打点を評価した件数。
    pub tenpai_value_memo_hits: u64,
    pub tenpai_value_memo_misses: u64,
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
                    call_ns: nanos(measurement.phases.call.total),
                    call_candidates_ns: nanos(measurement.phases.call.candidates),
                    call_pass_iishanten_self_tsumo_ns: nanos(
                        measurement.phases.call.pass_iishanten_self_tsumo,
                    ),
                    call_remaining_ns: nanos(measurement.phases.call.remaining()),
                    call_candidate_count: measurement.call_candidates.len(),
                    call_candidates: measurement
                        .call_candidates
                        .iter()
                        .map(|candidate| BenchmarkCallCandidateJson {
                            kind: format!("{:?}", candidate.kind),
                            tile: candidate.tile.to_mjai_string(),
                            consumed: candidate
                                .consumed
                                .iter()
                                .map(|tile| tile.to_mjai_string())
                                .collect(),
                            elapsed_ns: nanos(candidate.elapsed),
                            post_call_discard_selection_ns: nanos(
                                candidate.post_call_discard_selection,
                            ),
                            reused: candidate.reused,
                        })
                        .collect(),
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
                    iishanten_forward_candidate_count: measurement
                        .iishanten_forward_candidates
                        .len(),
                    iishanten_forward_candidates: measurement
                        .iishanten_forward_candidates
                        .iter()
                        .map(iishanten_forward_candidate_json)
                        .collect(),
                    two_shanten_self_tsumo_ns: nanos(
                        measurement
                            .phases
                            .normal_discard_phases
                            .two_shanten_self_tsumo,
                    ),
                    two_shanten_self_tsumo_candidate_count: measurement
                        .two_shanten_self_tsumo_candidates
                        .len(),
                    two_shanten_self_tsumo_candidates: measurement
                        .two_shanten_self_tsumo_candidates
                        .iter()
                        .map(
                            |(discard, elapsed)| BenchmarkTwoShantenSelfTsumoCandidateJson {
                                discard: discard.to_mjai_string(),
                                elapsed_ns: nanos(*elapsed),
                            },
                        )
                        .collect(),
                    three_shanten_self_tsumo_ns: nanos(
                        measurement
                            .phases
                            .normal_discard_phases
                            .three_shanten_self_tsumo,
                    ),
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

// 同一 state memo の利用数は、候補単位では hit / miss の合計だけを持つ。memo の種類別の内訳は
// 既存の探索診断が request 単位で持っているので、候補ごとに並べ直さない。
fn iishanten_forward_candidate_json(
    candidate: &IishantenForwardCandidateDuration,
) -> BenchmarkIishantenForwardCandidateJson {
    let memo = &candidate.search_state_memo;
    BenchmarkIishantenForwardCandidateJson {
        discard: candidate.discard.to_mjai_string(),
        elapsed_ns: nanos(candidate.elapsed),
        lookahead_search_ns: nanos(candidate.phases.lookahead_search),
        weighted_aggregation_ns: nanos(candidate.phases.weighted_aggregation),
        self_tsumo_continuation_ns: nanos(candidate.phases.self_tsumo_continuation),
        search_state_memo_hits: memo.two_shanten_hits
            + memo.iishanten_hits
            + memo.next_discard_hits
            + memo.same_shanten_next_discard_hits,
        search_state_memo_misses: memo.two_shanten_misses
            + memo.iishanten_misses
            + memo.next_discard_misses
            + memo.same_shanten_next_discard_misses,
        tenpai_value_memo_hits: candidate.tenpai_value_memo_hits,
        tenpai_value_memo_misses: candidate.tenpai_value_memo_misses,
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
    use bot_core::{Agent, CallKind};
    use bot_logic::{SearchStateMemoStats, TileId};
    use riichilab_client::observation::{
        fixture_base64, fixture_base64_with_discards, game_context_from_decoded_observation,
    };
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
            call: CallDecisionDurations::default(),
        }
    }

    fn with_call_breakdown(
        mut measurement: RequestMeasurement,
        total: u64,
        pass: u64,
        candidates: &[(CallKind, u8, [u8; 2], u64, u64)],
    ) -> RequestMeasurement {
        measurement.call_candidates = candidates
            .iter()
            .map(
                |(kind, tile, consumed, elapsed, post_call)| CallCandidateDuration {
                    kind: *kind,
                    tile: TileId::new(*tile).unwrap(),
                    consumed: consumed
                        .iter()
                        .map(|value| TileId::new(*value).unwrap())
                        .collect(),
                    elapsed: Duration::from_millis(*elapsed),
                    post_call_discard_selection: Duration::from_millis(*post_call),
                    reused: false,
                },
            )
            .collect();
        measurement.phases.call = CallDecisionDurations {
            total: Duration::from_millis(total),
            candidates: measurement
                .call_candidates
                .iter()
                .map(|candidate| candidate.elapsed)
                .sum(),
            pass_iishanten_self_tsumo: Duration::from_millis(pass),
        };
        measurement
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
        mut measurement: RequestMeasurement,
        total: u64,
        candidates: &[(&str, u64)],
    ) -> RequestMeasurement {
        measurement
            .phases
            .normal_discard_phases
            .two_shanten_self_tsumo = Duration::from_millis(total);
        measurement.two_shanten_self_tsumo_candidates = candidates
            .iter()
            .map(|(discard, elapsed)| {
                (
                    TileType::from_mjai_type_str(discard).unwrap(),
                    Duration::from_millis(*elapsed),
                )
            })
            .collect();
        measurement
    }

    fn with_iishanten_forward_breakdown(
        mut measurement: RequestMeasurement,
        candidates: &[(&str, u64, u64, u64, u64)],
    ) -> RequestMeasurement {
        measurement.iishanten_forward_candidates = candidates
            .iter()
            .map(|(discard, elapsed, search, aggregate, self_tsumo)| {
                IishantenForwardCandidateDuration {
                    discard: TileType::from_mjai_type_str(discard).unwrap(),
                    elapsed: Duration::from_millis(*elapsed),
                    phases: ForwardMetricsPhaseDurations {
                        lookahead_search: Duration::from_millis(*search),
                        weighted_aggregation: Duration::from_millis(*aggregate),
                        self_tsumo_continuation: Duration::from_millis(*self_tsumo),
                    },
                    search_state_memo: SearchStateMemoStats {
                        iishanten_hits: 7,
                        iishanten_misses: 3,
                        ..SearchStateMemoStats::default()
                    },
                    tenpai_value_memo_hits: 11,
                    tenpai_value_memo_misses: 5,
                }
            })
            .collect();
        measurement
    }

    fn with_three_shanten_breakdown(
        mut measurement: RequestMeasurement,
        total: u64,
    ) -> RequestMeasurement {
        measurement
            .phases
            .normal_discard_phases
            .three_shanten_self_tsumo = Duration::from_millis(total);
        measurement
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
            two_shanten_self_tsumo_candidates: Vec::new(),
            iishanten_forward_candidates: Vec::new(),
            call_candidates: Vec::new(),
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
            with_two_shanten_breakdown(
                measurement_with_phases(
                    "game-002.jsonl",
                    2,
                    2470,
                    with_forward_breakdown(
                        phases_with_normal_discard_breakdown(1, 2400, 69, 30, 2000, 20),
                        1950,
                        30,
                        20,
                    ),
                ),
                350,
                &[("5m", 180), ("8m", 160)],
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
            "  2470.000 ms  game-002.jsonl  request_id=2  early=1.000 ms (call=0.000 ms call_candidates=0.000 ms count=0 [] call_pass=0.000 ms call_remaining=0.000 ms)  normal_discard=2400.000 ms (base=30.000 ms forward=2000.000 ms [lookahead_search=1950.000 ms weighted_aggregation=30.000 ms self_tsumo_continuation=20.000 ms] forward_candidates=0 [] two_shanten_self_tsumo=350.000 ms candidates=2 [5m=180.000 ms 8m=160.000 ms] three_shanten_self_tsumo=0.000 ms finalize=20.000 ms)  post_discard=69.000 ms  selected=1m\n  10.000 ms  game-001.jsonl  request_id=1  early=0.000 ms (call=0.000 ms call_candidates=0.000 ms count=0 [] call_pass=0.000 ms call_remaining=0.000 ms)  normal_discard=0.000 ms (base=0.000 ms forward=0.000 ms [lookahead_search=0.000 ms weighted_aggregation=0.000 ms self_tsumo_continuation=0.000 ms] forward_candidates=0 [] two_shanten_self_tsumo=0.000 ms candidates=0 [] three_shanten_self_tsumo=0.000 ms finalize=0.000 ms)  post_discard=0.000 ms  selected=1m"
        );
    }

    // 1向聴から FF を Pon できる reaction request。直前の dahai record が reaction 元を確定させ、
    // production の1向聴 Call / Pass 比較まで進む。
    fn iishanten_pon_request_action_line(request_id: u64) -> String {
        let mut discards: [Vec<u8>; 4] = Default::default();
        discards[1] = vec![112, 113, 114, 115, 116, 117, 118, 119, 120, 130];
        let observation = fixture_base64_with_discards(
            0,
            None,
            vec![4, 8, 12, 17, 20, 24, 56, 64, 76, 84, 108, 128, 129],
            vec![],
            discards,
        );
        server_record_line(&format!(
            r#"{{"type":"request_action","request_id":{request_id},"actor":0,"possible_actions":[{{"type":"pon","pai":"F","consumed":["F","F"]}},{{"type":"none"}}],"observation":"{observation}"}}"#
        ))
    }

    #[test]
    fn a_captured_reaction_request_measures_the_call_subphases() {
        let path = write_capture(
            "iishanten-call",
            &[
                server_record_line(r#"{"type":"dahai","actor":1,"pai":"F"}"#),
                iishanten_pon_request_action_line(425),
            ],
        );
        let run = measure_captures(std::slice::from_ref(&path)).unwrap();
        let _ = std::fs::remove_file(&path);
        let measurement = &run.requests[0];
        let call = measurement.phases.call;

        assert!(call.total > Duration::ZERO);
        assert!(call.candidates > Duration::ZERO);
        assert!(call.pass_iishanten_self_tsumo > Duration::ZERO);
        assert!(call.total <= measurement.phases.early);
        assert_eq!(measurement.call_candidates.len(), 1);

        let candidate = &measurement.call_candidates[0];
        assert_eq!(candidate.kind, CallKind::Pon);
        assert_eq!(candidate.tile.to_mjai_string(), "F");
        assert!(candidate.post_call_discard_selection > Duration::ZERO);
        assert!(candidate.post_call_discard_selection <= candidate.elapsed);
        assert!(format_benchmark(&run).contains("Pon(F<-F,F)="));
    }

    #[test]
    fn report_and_json_show_the_call_subphases_of_a_slow_reaction_request() {
        let run = synthetic_run(vec![with_call_breakdown(
            measurement_with_phases("game-004.jsonl", 279, 1146, phases(1145, 0, 0)),
            1140,
            262,
            &[
                (CallKind::Chi, 8, [4, 12], 847, 840),
                (CallKind::Chi, 8, [4, 12], 846, 839),
                (CallKind::Pon, 20, [21, 22], 25, 20),
            ],
        )]);
        let report = format_benchmark(&run);
        let json = BenchmarkJson::from_run(&run);
        let request = &json.requests[0];

        assert!(
            report.contains("early=1145.000 ms (call=1140.000 ms"),
            "{report}"
        );
        assert!(report.contains("call_candidates=1718.000 ms"), "{report}");
        assert!(report.contains("call_pass=262.000 ms"), "{report}");
        assert!(
            report.contains("Chi(3m<-2m,4m)=847.000 ms post_call_discard=840.000 ms"),
            "{report}"
        );
        assert!(
            report.contains("Pon(6m<-6m,6m)=25.000 ms post_call_discard=20.000 ms"),
            "{report}"
        );

        assert_eq!(request.call_ns, 1_140_000_000);
        assert_eq!(request.call_candidates_ns, 1_718_000_000);
        assert_eq!(request.call_pass_iishanten_self_tsumo_ns, 262_000_000);
        assert_eq!(request.call_candidate_count, 3);
        assert_eq!(
            request.call_candidates[0],
            BenchmarkCallCandidateJson {
                kind: "Chi".to_string(),
                tile: "3m".to_string(),
                consumed: vec!["2m".to_string(), "4m".to_string()],
                elapsed_ns: 847_000_000,
                post_call_discard_selection_ns: 840_000_000,
                reused: false,
            }
        );
        // 同じ表示になる候補も行をまとめず、合法 action の順にそのまま2件並ぶ。
        assert_eq!(
            request.call_candidates[1].tile,
            request.call_candidates[0].tile
        );
        assert_eq!(
            request.call_candidates[1].consumed,
            request.call_candidates[0].consumed
        );

        let text = serde_json::to_string(&json).unwrap();
        assert!(text.contains("\"call_candidates_ns\""), "{text}");
        assert_eq!(serde_json::from_str::<BenchmarkJson>(&text).unwrap(), json);
    }

    #[test]
    fn report_and_json_show_the_iishanten_forward_candidates_of_a_slow_normal_discard() {
        // 候補単位で並行に評価するため、候補の実測の合計は forward の壁時計を超える。合計を
        // 壁時計として見せず、候補の内訳をそのまま並べる。並行評価では計測 thread が phase の
        // 区切りを通らないので、既存の forward subphase は従来どおり 0 のまま。
        let run = synthetic_run(vec![with_iishanten_forward_breakdown(
            measurement_with_phases(
                "game-005.jsonl",
                731,
                2_050,
                phases_with_normal_discard_breakdown(0, 2_040, 10, 30, 1_980, 30),
            ),
            &[("3m", 1_800, 1_700, 60, 40), ("6m", 1_560, 1_450, 60, 50)],
        )]);
        let report = format_benchmark(&run);
        let json = BenchmarkJson::from_run(&run);
        let request = &json.requests[0];

        assert!(report.contains("forward_candidates=2 ["), "{report}");
        assert!(
            report.contains(
                "3m=1800.000 ms [lookahead_search=1700.000 ms weighted_aggregation=60.000 ms \
                 self_tsumo_continuation=40.000 ms]"
            ),
            "{report}"
        );

        assert_eq!(request.iishanten_forward_candidate_count, 2);
        assert_eq!(
            request.iishanten_forward_candidates[0],
            BenchmarkIishantenForwardCandidateJson {
                discard: "3m".to_string(),
                elapsed_ns: 1_800_000_000,
                lookahead_search_ns: 1_700_000_000,
                weighted_aggregation_ns: 60_000_000,
                self_tsumo_continuation_ns: 40_000_000,
                search_state_memo_hits: 7,
                search_state_memo_misses: 3,
                tenpai_value_memo_hits: 11,
                tenpai_value_memo_misses: 5,
            }
        );
        // 候補の実測の合計は forward phase の壁時計を超えたままで、どちらも書き換えない。
        let summed: u64 = request
            .iishanten_forward_candidates
            .iter()
            .map(|candidate| candidate.elapsed_ns)
            .sum();
        assert!(summed > request.normal_discard_forward_ns);

        // 既存 scalar field の semantics は変えない。候補の内訳をここへ足し込まないので、
        // 並行評価した request では従来どおり 0 のまま。
        assert_eq!(request.normal_discard_forward_ns, 1_980_000_000);
        assert_eq!(request.forward_lookahead_search_ns, 0);
        assert_eq!(request.forward_weighted_aggregation_ns, 0);
        assert_eq!(request.forward_self_tsumo_ns, 0);

        let text = serde_json::to_string(&json).unwrap();
        assert!(text.contains("\"iishanten_forward_candidates\""), "{text}");
        assert_eq!(serde_json::from_str::<BenchmarkJson>(&text).unwrap(), json);
    }

    #[test]
    fn the_call_fields_of_an_earlier_benchmark_json_default_to_zero() {
        // 既存 consumer が書いた call field の無い JSON も読めるままにする。
        let text = r#"{
            "summary": {
                "captures": 1, "requests": 1, "total_ns": 1, "mean_ns": 1, "p50_ns": 1,
                "p90_ns": 1, "p95_ns": 1, "p99_ns": 1, "max_ns": 1,
                "over_500ms": 0, "over_1s": 0, "over_2s": 0, "over_3s": 0
            },
            "requests": [{
                "capture": "game-001.jsonl", "request_id": 1, "actor": 0, "elapsed_ns": 1,
                "early_ns": 0, "normal_discard_ns": 0, "normal_discard_base_ns": 0,
                "normal_discard_forward_ns": 0, "forward_lookahead_search_ns": 0,
                "forward_weighted_aggregation_ns": 0, "forward_self_tsumo_ns": 0,
                "two_shanten_self_tsumo_ns": 0, "two_shanten_self_tsumo_candidate_count": 0,
                "two_shanten_self_tsumo_candidates": [], "three_shanten_self_tsumo_ns": 0,
                "normal_discard_finalize_ns": 0, "post_discard_ns": 0, "selected": "1m"
            }]
        }"#;
        let json: BenchmarkJson = serde_json::from_str(text).unwrap();

        assert_eq!(json.requests[0].call_ns, 0);
        assert_eq!(json.requests[0].call_candidate_count, 0);
        assert!(json.requests[0].call_candidates.is_empty());
        assert_eq!(json.requests[0].iishanten_forward_candidate_count, 0);
        assert!(json.requests[0].iishanten_forward_candidates.is_empty());
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
    fn report_and_json_keep_the_three_shanten_self_tsumo_phase() {
        let run = synthetic_run(vec![with_three_shanten_breakdown(
            measurement_with_phases(
                "game-003.jsonl",
                3,
                1900,
                phases_with_normal_discard_breakdown(1, 1880, 19, 30, 50, 20),
            ),
            1780,
        )]);
        let report = format_benchmark(&run);
        let json = BenchmarkJson::from_run(&run);

        assert!(
            report.contains("three_shanten_self_tsumo=1780.000 ms"),
            "{report}"
        );
        assert_eq!(json.requests[0].three_shanten_self_tsumo_ns, 1_780_000_000);
        assert_eq!(json.requests[0].two_shanten_self_tsumo_ns, 0);
        assert_eq!(
            run.requests[0].phases.normal_discard_phases.total(),
            run.requests[0].phases.normal_discard
        );

        let text = serde_json::to_string(&json).unwrap();
        assert!(text.contains("\"three_shanten_self_tsumo_ns\""), "{text}");
        assert_eq!(serde_json::from_str::<BenchmarkJson>(&text).unwrap(), json);
    }

    #[test]
    fn benchmark_json_keeps_the_summary_and_every_request() {
        let run = synthetic_run(vec![
            measurement("game-001.jsonl", 1, 10),
            with_two_shanten_breakdown(
                measurement_with_phases(
                    "game-002.jsonl",
                    2,
                    2470,
                    with_forward_breakdown(
                        phases_with_normal_discard_breakdown(1, 2400, 69, 30, 2000, 20),
                        1950,
                        30,
                        20,
                    ),
                ),
                350,
                &[("5m", 180), ("8m", 160)],
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
                    call_ns: 0,
                    call_candidates_ns: 0,
                    call_pass_iishanten_self_tsumo_ns: 0,
                    call_remaining_ns: 0,
                    call_candidate_count: 0,
                    call_candidates: vec![],
                    normal_discard_ns: 0,
                    normal_discard_base_ns: 0,
                    normal_discard_forward_ns: 0,
                    forward_lookahead_search_ns: 0,
                    forward_weighted_aggregation_ns: 0,
                    forward_self_tsumo_ns: 0,
                    iishanten_forward_candidate_count: 0,
                    iishanten_forward_candidates: vec![],
                    two_shanten_self_tsumo_ns: 0,
                    two_shanten_self_tsumo_candidate_count: 0,
                    two_shanten_self_tsumo_candidates: vec![],
                    three_shanten_self_tsumo_ns: 0,
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
                    call_ns: 0,
                    call_candidates_ns: 0,
                    call_pass_iishanten_self_tsumo_ns: 0,
                    call_remaining_ns: 0,
                    call_candidate_count: 0,
                    call_candidates: vec![],
                    normal_discard_ns: 2_400_000_000,
                    normal_discard_base_ns: 30_000_000,
                    normal_discard_forward_ns: 2_000_000_000,
                    forward_lookahead_search_ns: 1_950_000_000,
                    forward_weighted_aggregation_ns: 30_000_000,
                    forward_self_tsumo_ns: 20_000_000,
                    iishanten_forward_candidate_count: 0,
                    iishanten_forward_candidates: vec![],
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
                    three_shanten_self_tsumo_ns: 0,
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
            run.requests[0].two_shanten_self_tsumo_candidates.len()
        );
        assert_eq!(
            request.two_shanten_self_tsumo_candidates.len(),
            run.requests[0].two_shanten_self_tsumo_candidates.len()
        );
        assert_eq!(
            request.three_shanten_self_tsumo_ns,
            nanos(phases.three_shanten_self_tsumo)
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
        assert!(text.contains("\"three_shanten_self_tsumo_ns\""), "{text}");
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
