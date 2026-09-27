//! `現在2向聴 → Chi / Pon → 2向聴のまま` の Call / Pass を Progress / Full の2つの scope で観測した
//! 結果を表示する。
//!
//! 観測そのものは [`bot_core::observe_two_shanten_stay_calls`] が行い、ここは単一局面と capture
//! 全体の集計・整形だけを持つ。production の鳴き判断は変わらず、表示のために比較をやり直さない。

use std::collections::BTreeMap;
use std::time::Duration;

use bot_analysis::Scenario;
use bot_core::{
    CallIishantenComparison, CallKind, LegalAction, ProspectiveHanVerdict,
    TwoShantenStayAllowPartialWaits, TwoShantenStayCallAgreement, TwoShantenStayCallCandidate,
    TwoShantenStayCallObservation, TwoShantenStayCallScope, TwoShantenStayCallUnknown,
    TwoShantenStayCallValue, TwoShantenStayPassObservation, TwoShantenStayRequireAllLiveWaits,
    observe_two_shanten_stay_calls,
};
use bot_logic::{SELF_TSUMO_VALUE_SCALE, SearchStateMemoStats, ThreeShantenSearchStats, TileType};

use crate::benchmark::LatencyStatistics;
use crate::cli::CaptureComparisonSpec;
use crate::error::ScenarioError;
use crate::format::action_label;
use crate::replay::load_captured_scenarios;

const SCOPES: [TwoShantenStayCallScope; 2] = [
    TwoShantenStayCallScope::Progress,
    TwoShantenStayCallScope::Full,
];

/// 単一局面の observation。
pub fn format_scenario_observation(scenario: &Scenario) -> String {
    let observation =
        observe_two_shanten_stay_calls(&scenario.context, scenario.legal_actions.as_slice());
    format_observation(&observation)
}

fn format_observation(observation: &TwoShantenStayCallObservation) -> String {
    let mut lines = vec!["Two-shanten stay call observation (2 -> Chi / Pon -> 2)".to_string()];
    lines.extend(definition_lines());
    lines.push(String::new());
    lines.extend(format_production(observation));
    lines.push(String::new());

    if !observation.has_targets() {
        lines.push(
            "No 2 -> 2 Chi / Pon candidate: nothing is observed and no pass or call is evaluated"
                .to_string(),
        );
        return lines.join("\n");
    }

    lines.push("Pass (evaluated once per request and scope, shared by every target)".to_string());
    for scope in SCOPES {
        lines.push(format!(
            "  {}: {}",
            scope_label(scope),
            format_pass(observation.pass(scope))
        ));
    }
    for candidate in &observation.candidates {
        lines.push(String::new());
        lines.extend(format_candidate(observation, candidate));
    }
    lines.join("\n")
}

fn definition_lines() -> Vec<String> {
    vec![
        "  observation only: the production call decision, its reasons and the selected action \
         stay unchanged, and nothing here feeds back into the production policy"
            .to_string(),
        "  targets: current shanten 2, post-call min shanten 2 and reason PostCallNotIishanten, \
         read from the production call decision and its candidate preparation"
            .to_string(),
        "  progress: the first draw Progress branch into the production one-shanten continuation; \
         the call selects its post-call discard with the existing two-shanten Progress comparator"
            .to_string(),
        "  full: the production two-shanten discard selection semantics (Progress cohort, \
         provisional ranking, dora-difference gate, gated top-2 Full evaluation, production \
         comparator and parallelism), not a Full evaluation of every discard; a discard the \
         selection does not evaluate with Full stays unknown"
            .to_string(),
        "  progress and full are compared independently; a tie keeps the pass as the production \
         call / pass policy does"
            .to_string(),
        "  every measured run starts on its own fresh thread with cold memos; the call runs twice \
         (a timing run with no instrumentation the elapsed time comes from, and an observation \
         run the search stats and the terminal yaku come from)"
            .to_string(),
        "  partial-yaku: counterfactual policies on the progress conclusion, never wired into \
         production; terminal yaku folds every live winning-tile variant of every terminal the \
         selected post-call discard's progress value scored (tsumo baseline), and an unknown is \
         never taken as yaku; RequireAllLiveWaits calls only on call > pass with yaku on every \
         live variant, AllowPartialWaits reads the progress conclusion as is (a no-yaku variant \
         already counts as a non-winning draw there)"
            .to_string(),
    ]
}

fn format_production(observation: &TwoShantenStayCallObservation) -> Vec<String> {
    let mut lines = vec!["Production call decision (unchanged)".to_string()];
    let Some(call) = observation.call.as_ref() else {
        lines.push("  no legal Chi / Pon".to_string());
        return lines;
    };
    lines.push(format!(
        "  selected: {}",
        call.selected
            .as_ref()
            .map(call_label)
            .unwrap_or_else(|| "none".to_string())
    ));
    lines.push(format!("  reason: {:?}", call.reason));
    lines.push(format!(
        "  reaction source player: {}",
        observation
            .reaction_source_player
            .map(|player| player.to_string())
            .unwrap_or_else(|| "unknown".to_string())
    ));
    lines.push(format!(
        "  call candidates: {}, 2 -> 2 targets: {}",
        call.candidates.len(),
        observation.candidates.len()
    ));
    for (index, candidate) in call.candidates.iter().enumerate() {
        lines.push(format!(
            "    #{index} {}: {:?}{}",
            call_label(&candidate.action),
            candidate.reason,
            if candidate.stays_two_shanten_after_call() {
                " (2 -> 2 target)"
            } else {
                ""
            },
        ));
    }
    lines
}

fn format_pass(pass: Option<TwoShantenStayPassObservation>) -> String {
    let Some(pass) = pass else {
        return "not evaluated".to_string();
    };
    match pass.elapsed {
        Some(elapsed) => format!(
            "{} (elapsed {})",
            format_value(pass.value),
            format_duration(elapsed)
        ),
        None => format!("{} (not evaluated)", format_value(pass.value)),
    }
}

fn format_candidate(
    observation: &TwoShantenStayCallObservation,
    candidate: &TwoShantenStayCallCandidate,
) -> Vec<String> {
    let mut lines = vec![format!(
        "Candidate #{} {}",
        candidate.candidate_index,
        call_label(&candidate.action)
    )];
    if let Some(source) = candidate.reused_from {
        lines.push(format!(
            "  same post-call state as #{source}: the observation is reused, nothing is evaluated \
             again"
        ));
    }
    lines.push(format!(
        "  forbidden discards: {}, post-call fixed melds: {}, post-call min shanten: {}",
        format_tiles(&candidate.forbidden_discards),
        candidate.post_call_fixed_meld_count.get(),
        candidate.post_call_min_shanten,
    ));

    let progress = &candidate.progress;
    lines.push(format!(
        "  progress: selected {}, call {}, pass {}, {}",
        format_selected(progress.selected.as_ref()),
        format_value(progress.value),
        format_pass_value(observation.pass(TwoShantenStayCallScope::Progress)),
        format_comparison(observation, candidate, TwoShantenStayCallScope::Progress),
    ));
    lines.push(format!(
        "    call elapsed (timing run): {}, timing and observation runs agree: {}",
        format_duration(progress.elapsed),
        progress.runs_agree
    ));
    lines.push(format!(
        "    search (observation run): {}",
        format_search(&progress.search, &progress.memo)
    ));
    lines.push("  partial-yaku (progress, counterfactual)".to_string());
    lines.push(format!(
        "    terminal yaku (observation run): {}",
        format_terminal_yaku(candidate)
    ));
    lines.push(format!(
        "    RequireAllLiveWaits: {}",
        require_all_live_waits_label(candidate.require_all_live_waits())
    ));
    lines.push(format!(
        "    AllowPartialWaits: {}",
        allow_partial_waits_label(candidate.allow_partial_waits())
    ));

    let full = &candidate.full;
    lines.push(format!(
        "  full: selected {}, call {}, pass {}, {}",
        format_selected(full.selected.as_ref()),
        format_value(full.value),
        format_pass_value(observation.pass(TwoShantenStayCallScope::Full)),
        format_comparison(observation, candidate, TwoShantenStayCallScope::Full),
    ));
    lines.push(format!(
        "    progress cohort: {} ({}), full evaluated: {} ({}), full workers: {}",
        full.progress_cohort.len(),
        format_tiles(&full.progress_cohort),
        full.full_evaluated.len(),
        format_tiles(&full.full_evaluated),
        full.full_workers,
    ));
    lines.push(format!(
        "    selection elapsed (timing run): {}, timing and observation runs agree: {}",
        format_duration(full.elapsed),
        full.runs_agree
    ));
    lines.push(format!(
        "    search (observation run): {}",
        format_search(&full.search, &full.memo)
    ));

    lines.push(format!(
        "  selected post-call discard: {}",
        match candidate.selected_discards_match() {
            Some(true) => "same".to_string(),
            Some(false) => "different".to_string(),
            None => "unavailable".to_string(),
        }
    ));
    lines.push(format!(
        "  call / pass conclusion progress vs full: {}",
        agreement_label(candidate.comparison_agreement())
    ));
    lines
}

fn format_terminal_yaku(candidate: &TwoShantenStayCallCandidate) -> String {
    let terminal_yaku = candidate.progress.terminal_yaku;
    let verdict = terminal_yaku_label(terminal_yaku.verdict);
    if candidate.progress.selected.is_none() || terminal_yaku.reproduces_value {
        verdict.to_string()
    } else {
        format!("{verdict} (the yaku run did not reproduce the progress value)")
    }
}

fn format_comparison(
    observation: &TwoShantenStayCallObservation,
    candidate: &TwoShantenStayCallCandidate,
    scope: TwoShantenStayCallScope,
) -> String {
    let comparison = candidate.comparison(scope);
    if comparison != CallIishantenComparison::Unknown {
        return comparison_label(comparison).to_string();
    }
    let mut causes = Vec::new();
    if let TwoShantenStayCallValue::Unknown(cause) = candidate.call_value(scope) {
        causes.push(format!("call {}", unknown_label(cause)));
    }
    if let Some(TwoShantenStayCallValue::Unknown(cause)) =
        observation.pass(scope).map(|pass| pass.value)
    {
        causes.push(format!("pass {}", unknown_label(cause)));
    }
    format!("unknown ({})", causes.join(", "))
}

fn format_pass_value(pass: Option<TwoShantenStayPassObservation>) -> String {
    pass.map(|pass| format_value(pass.value))
        .unwrap_or_else(|| "not evaluated".to_string())
}

fn format_selected(selected: Option<&LegalAction>) -> String {
    selected
        .map(call_label)
        .unwrap_or_else(|| "none".to_string())
}

fn format_search(search: &ThreeShantenSearchStats, memo: &SearchStateMemoStats) -> String {
    format!(
        "2->1 draw variants {}, leaf draw states {}, base evaluation calls {} / misses {}, \
         shanten / acceptance rebuilds {}, same-shanten enumerations {}, terminal scorings {}, \
         same-state memo hits {} / misses {}",
        search.two_to_one_variants,
        search.draw_variants,
        search.base_evaluation_calls,
        search.base_evaluation_misses,
        search.structural_evaluation_misses,
        search.same_shanten_enumerations,
        search.terminal_scorings,
        memo.two_shanten_hits + memo.iishanten_hits + memo.next_discard_hits,
        memo.two_shanten_misses + memo.iishanten_misses + memo.next_discard_misses,
    )
}

/// capture 全 request の observation。
pub fn run_capture_comparison(spec: &CaptureComparisonSpec) -> Result<String, ScenarioError> {
    let mut replayed = 0;
    let mut requests = Vec::new();
    for path in &spec.paths {
        for captured in load_captured_scenarios(path)? {
            replayed += 1;
            let observation = observe_two_shanten_stay_calls(
                &captured.scenario.context,
                &captured.scenario.legal_actions,
            );
            requests.push(ObservedRequest {
                capture: captured.path.clone(),
                request_id: captured.request_id,
                observation,
            });
        }
    }
    Ok(format_capture_comparison(
        spec.paths.len(),
        replayed,
        &requests,
    ))
}

/// capture の request 1件分の observation。
struct ObservedRequest {
    capture: String,
    request_id: u64,
    observation: TwoShantenStayCallObservation,
}

/// Call / Pass の結論ごとの候補数。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct ConclusionCounts {
    call_higher: usize,
    pass_not_lower: usize,
    unknown: usize,
    /// unknown の原因。値を確定できなかった側と原因の組ごとの候補数。
    unknown_causes: BTreeMap<String, usize>,
}

/// capture 全体の集計。request 単位と候補単位を分けて持つ。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct Summary {
    replayed_requests: usize,
    call_requests: usize,
    target_requests: usize,
    candidates: usize,
    chi: usize,
    pon: usize,
    reused: usize,
    progress: ConclusionCounts,
    full: ConclusionCounts,
    same_conclusion: usize,
    flipped_conclusion: usize,
    undetermined_conclusion: usize,
    same_discard: usize,
    different_discard: usize,
    unavailable_discard: usize,
    partial_yaku: PartialYakuCounts,
}

/// Progress が `call > pass` の候補についての、片和了 policy ごとの結論の件数。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct PartialYakuCounts {
    candidates: usize,
    yaku_at_least: usize,
    yaku_below: usize,
    yaku_unknown: usize,
    require_all_call: usize,
    require_all_blocked: usize,
    require_all_unknown: usize,
    allow_partial_call: usize,
}

impl PartialYakuCounts {
    fn count(&mut self, candidate: &TwoShantenStayCallCandidate) {
        if candidate.progress_comparison != CallIishantenComparison::CallHigher {
            return;
        }
        self.candidates += 1;
        match candidate.progress.terminal_yaku.verdict {
            ProspectiveHanVerdict::AtLeast => self.yaku_at_least += 1,
            ProspectiveHanVerdict::Below => self.yaku_below += 1,
            ProspectiveHanVerdict::Unknown => self.yaku_unknown += 1,
        }
        match candidate.require_all_live_waits() {
            TwoShantenStayRequireAllLiveWaits::Call => self.require_all_call += 1,
            TwoShantenStayRequireAllLiveWaits::BlockedByPartialYaku => {
                self.require_all_blocked += 1
            }
            TwoShantenStayRequireAllLiveWaits::Unknown => self.require_all_unknown += 1,
            TwoShantenStayRequireAllLiveWaits::Pass => {}
        }
        if candidate.allow_partial_waits() == TwoShantenStayAllowPartialWaits::Call {
            self.allow_partial_call += 1;
        }
    }
}

impl Summary {
    fn from_requests(replayed_requests: usize, requests: &[ObservedRequest]) -> Self {
        let mut summary = Self {
            replayed_requests,
            ..Self::default()
        };
        for request in requests {
            let observation = &request.observation;
            if observation.call.is_some() {
                summary.call_requests += 1;
            }
            if observation.has_targets() {
                summary.target_requests += 1;
            }
            for candidate in &observation.candidates {
                summary.candidates += 1;
                match candidate.kind {
                    CallKind::Chi => summary.chi += 1,
                    CallKind::Pon => summary.pon += 1,
                }
                if candidate.reused_from.is_some() {
                    summary.reused += 1;
                }
                for scope in SCOPES {
                    let counts = match scope {
                        TwoShantenStayCallScope::Progress => &mut summary.progress,
                        TwoShantenStayCallScope::Full => &mut summary.full,
                    };
                    counts.count(observation, candidate, scope);
                }
                match candidate.comparison_agreement() {
                    TwoShantenStayCallAgreement::Same => summary.same_conclusion += 1,
                    TwoShantenStayCallAgreement::Flipped => summary.flipped_conclusion += 1,
                    TwoShantenStayCallAgreement::Undetermined => {
                        summary.undetermined_conclusion += 1
                    }
                }
                match candidate.selected_discards_match() {
                    Some(true) => summary.same_discard += 1,
                    Some(false) => summary.different_discard += 1,
                    None => summary.unavailable_discard += 1,
                }
                summary.partial_yaku.count(candidate);
            }
        }
        summary
    }
}

impl ConclusionCounts {
    fn count(
        &mut self,
        observation: &TwoShantenStayCallObservation,
        candidate: &TwoShantenStayCallCandidate,
        scope: TwoShantenStayCallScope,
    ) {
        match candidate.comparison(scope) {
            CallIishantenComparison::CallHigher => self.call_higher += 1,
            CallIishantenComparison::PassNotLower => self.pass_not_lower += 1,
            CallIishantenComparison::Unknown => {
                self.unknown += 1;
                if let TwoShantenStayCallValue::Unknown(cause) = candidate.call_value(scope) {
                    *self
                        .unknown_causes
                        .entry(format!("call {}", unknown_label(cause)))
                        .or_default() += 1;
                }
                if let Some(TwoShantenStayCallValue::Unknown(cause)) =
                    observation.pass(scope).map(|pass| pass.value)
                {
                    *self
                        .unknown_causes
                        .entry(format!("pass {}", unknown_label(cause)))
                        .or_default() += 1;
                }
            }
        }
    }
}

// 候補を評価し直さずに複製した候補は latency の標本に入れない。
fn evaluated_candidates(
    requests: &[ObservedRequest],
) -> impl Iterator<Item = (&ObservedRequest, &TwoShantenStayCallCandidate)> {
    requests.iter().flat_map(|request| {
        request
            .observation
            .candidates
            .iter()
            .filter(|candidate| candidate.reused_from.is_none())
            .map(move |candidate| (request, candidate))
    })
}

// 代表 case として表示する件数。
const SLOWEST_CASE_COUNT: usize = 5;
const FLIPPED_CASE_COUNT: usize = 10;

fn format_capture_comparison(
    captures: usize,
    replayed_requests: usize,
    requests: &[ObservedRequest],
) -> String {
    let summary = Summary::from_requests(replayed_requests, requests);
    let mut lines = vec!["Two-shanten stay call observation over captures".to_string()];
    lines.extend(definition_lines());
    lines.extend([
        String::new(),
        "Requests".to_string(),
        format!("  captures: {captures}"),
        format!("  replayed requests: {}", summary.replayed_requests),
        format!(
            "  requests with a legal Chi / Pon: {}",
            summary.call_requests
        ),
        format!(
            "  requests with a 2 -> 2 target: {}",
            summary.target_requests
        ),
        String::new(),
        "Candidates".to_string(),
        format!("  2 -> 2 call candidates: {}", summary.candidates),
        format!("  chi: {}", summary.chi),
        format!("  pon: {}", summary.pon),
        format!(
            "  reusing an earlier candidate's post-call state: {}",
            summary.reused
        ),
        String::new(),
        "Call / Pass conclusion (candidates)".to_string(),
    ]);
    for scope in SCOPES {
        let counts = match scope {
            TwoShantenStayCallScope::Progress => &summary.progress,
            TwoShantenStayCallScope::Full => &summary.full,
        };
        lines.push(format!(
            "  {}: call > pass {}, pass >= call {}, unknown {}",
            scope_label(scope),
            counts.call_higher,
            counts.pass_not_lower,
            counts.unknown
        ));
        if !counts.unknown_causes.is_empty() {
            lines.push(format!(
                "    unknown causes: {}",
                counts
                    .unknown_causes
                    .iter()
                    .map(|(cause, count)| format!("{cause} {count}"))
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }
    }
    lines.extend([
        String::new(),
        "Progress vs full (candidates)".to_string(),
        format!("  same conclusion: {}", summary.same_conclusion),
        format!("  flipped conclusion: {}", summary.flipped_conclusion),
        format!(
            "  undetermined (either scope unknown): {}",
            summary.undetermined_conclusion
        ),
        format!(
            "  selected post-call discard: same {}, different {}, unavailable {}",
            summary.same_discard, summary.different_discard, summary.unavailable_discard
        ),
        String::new(),
    ]);
    lines.extend(format_partial_yaku(&summary.partial_yaku));
    lines.push(String::new());
    lines.extend(format_partial_yaku_differences(requests));
    lines.push(String::new());
    lines.extend(format_latency(requests));
    lines.push(String::new());
    lines.extend(format_slowest_full(requests));
    lines.push(String::new());
    lines.extend(format_flipped(requests));
    lines.push(String::new());
    lines.push("Per request with a 2 -> 2 target".to_string());
    for request in requests
        .iter()
        .filter(|request| request.observation.has_targets())
    {
        lines.push(format!(
            "  {}  request_id={}  production={}  targets={}  progress_pass={}  full_pass={}",
            request.capture,
            request.request_id,
            format_production_summary(&request.observation),
            request.observation.candidates.len(),
            format_pass(request.observation.progress_pass),
            format_pass(request.observation.full_pass),
        ));
    }
    lines.join("\n")
}

fn format_partial_yaku(counts: &PartialYakuCounts) -> Vec<String> {
    vec![
        "Progress call > pass partial-yaku policy (candidates, counterfactual)".to_string(),
        format!("  candidates: {}", counts.candidates),
        "  terminal yaku:".to_string(),
        format!("    all live variants >= 1 han: {}", counts.yaku_at_least),
        format!("    contains no-yaku live variant: {}", counts.yaku_below),
        format!("    unknown: {}", counts.yaku_unknown),
        "  RequireAllLiveWaits:".to_string(),
        format!("    call: {}", counts.require_all_call),
        format!(
            "    blocked by partial yaku: {}",
            counts.require_all_blocked
        ),
        format!("    unknown: {}", counts.require_all_unknown),
        "  AllowPartialWaits:".to_string(),
        format!("    call: {}", counts.allow_partial_call),
    ]
}

// 2つの policy の結論が違う候補はどれも Progress が `call > pass` なので件数は少なく、全件を出す。
fn format_partial_yaku_differences(requests: &[ObservedRequest]) -> Vec<String> {
    let differences: Vec<_> = requests
        .iter()
        .flat_map(|request| {
            request
                .observation
                .candidates
                .iter()
                .filter(|candidate| candidate.partial_yaku_policies_differ())
                .map(move |candidate| (request, candidate))
        })
        .collect();
    let mut lines = vec![format!(
        "RequireAllLiveWaits vs AllowPartialWaits differences ({})",
        differences.len()
    )];
    for (request, candidate) in differences {
        lines.push(format!(
            "  {}  request_id={}  #{} {}{}",
            request.capture,
            request.request_id,
            candidate.candidate_index,
            call_label(&candidate.action),
            candidate
                .reused_from
                .map(|source| format!("  reused_from=#{source}"))
                .unwrap_or_default(),
        ));
        lines.push(format!(
            "    progress: selected {}, call {}, pass {}, {}",
            format_selected(candidate.progress.selected.as_ref()),
            format_value(candidate.progress.value),
            format_pass_value(request.observation.pass(TwoShantenStayCallScope::Progress)),
            comparison_label(candidate.progress_comparison),
        ));
        lines.push(format!(
            "    terminal yaku: {}, RequireAllLiveWaits: {}, AllowPartialWaits: {}",
            format_terminal_yaku(candidate),
            require_all_live_waits_label(candidate.require_all_live_waits()),
            allow_partial_waits_label(candidate.allow_partial_waits()),
        ));
    }
    lines
}

fn format_production_summary(observation: &TwoShantenStayCallObservation) -> String {
    let selected = observation
        .production_selected()
        .map(call_label)
        .unwrap_or_else(|| "none".to_string());
    match observation.production_reason() {
        Some(reason) => format!("{selected} ({reason:?})"),
        None => selected,
    }
}

fn format_latency(requests: &[ObservedRequest]) -> Vec<String> {
    let target_requests: Vec<_> = requests
        .iter()
        .filter(|request| request.observation.has_targets())
        .collect();
    let pass = |scope| -> Vec<Duration> {
        target_requests
            .iter()
            .filter_map(|request| request.observation.pass(scope)?.elapsed)
            .collect()
    };
    let progress_calls: Vec<_> = evaluated_candidates(requests)
        .map(|(_, candidate)| candidate.progress.elapsed)
        .collect();
    let full_calls: Vec<_> = evaluated_candidates(requests)
        .map(|(_, candidate)| candidate.full.elapsed)
        .collect();
    let total = |scope: TwoShantenStayCallScope| -> Vec<Duration> {
        target_requests
            .iter()
            .map(|request| {
                let pass = request
                    .observation
                    .pass(scope)
                    .and_then(|pass| pass.elapsed)
                    .unwrap_or_default();
                request
                    .observation
                    .candidates
                    .iter()
                    .filter(|candidate| candidate.reused_from.is_none())
                    .map(|candidate| match scope {
                        TwoShantenStayCallScope::Progress => candidate.progress.elapsed,
                        TwoShantenStayCallScope::Full => candidate.full.elapsed,
                    })
                    .sum::<Duration>()
                    + pass
            })
            .collect()
    };
    vec![
        "Latency (count / median / p95 / max)".to_string(),
        format_statistics(
            "progress pass (per request)",
            &pass(TwoShantenStayCallScope::Progress),
        ),
        format_statistics(
            "full pass (per request)",
            &pass(TwoShantenStayCallScope::Full),
        ),
        format_statistics("progress call (per evaluated candidate)", &progress_calls),
        format_statistics("full call (per evaluated candidate)", &full_calls),
        format_statistics(
            "progress pass + calls (per request, sequential sum)",
            &total(TwoShantenStayCallScope::Progress),
        ),
        format_statistics(
            "full pass + calls (per request, sequential sum)",
            &total(TwoShantenStayCallScope::Full),
        ),
    ]
}

fn format_statistics(label: &str, durations: &[Duration]) -> String {
    let statistics = LatencyStatistics::from_durations(durations);
    format!(
        "  {label}: {} / {} / {} / {}",
        statistics.requests,
        format_duration(statistics.p50),
        format_duration(statistics.p95),
        format_duration(statistics.max),
    )
}

fn format_slowest_full(requests: &[ObservedRequest]) -> Vec<String> {
    let mut candidates: Vec<_> = evaluated_candidates(requests).collect();
    candidates.sort_by_key(|(_, candidate)| std::cmp::Reverse(candidate.full.elapsed));
    let mut lines = vec!["Slowest full calls (evaluated candidates)".to_string()];
    for (request, candidate) in candidates.into_iter().take(SLOWEST_CASE_COUNT) {
        lines.push(format!(
            "  {}  {}  request_id={}  #{} {}  progress={}  full evaluated={} ({})  selected={}",
            format_duration(candidate.full.elapsed),
            request.capture,
            request.request_id,
            candidate.candidate_index,
            call_label(&candidate.action),
            format_duration(candidate.progress.elapsed),
            candidate.full.full_evaluated.len(),
            format_tiles(&candidate.full.full_evaluated),
            format_selected(candidate.full.selected.as_ref()),
        ));
    }
    lines
}

fn format_flipped(requests: &[ObservedRequest]) -> Vec<String> {
    let flipped: Vec<_> = requests
        .iter()
        .flat_map(|request| {
            request
                .observation
                .candidates
                .iter()
                .filter(|candidate| {
                    candidate.comparison_agreement() == TwoShantenStayCallAgreement::Flipped
                })
                .map(move |candidate| (request, candidate))
        })
        .collect();
    let mut lines = vec![format!(
        "Flipped conclusions (showing up to {FLIPPED_CASE_COUNT} of {})",
        flipped.len()
    )];
    for (request, candidate) in flipped.into_iter().take(FLIPPED_CASE_COUNT) {
        let observation = &request.observation;
        lines.push(format!(
            "  {}  request_id={}  #{} {}",
            request.capture,
            request.request_id,
            candidate.candidate_index,
            call_label(&candidate.action),
        ));
        for scope in SCOPES {
            let selected = match scope {
                TwoShantenStayCallScope::Progress => candidate.progress.selected.as_ref(),
                TwoShantenStayCallScope::Full => candidate.full.selected.as_ref(),
            };
            lines.push(format!(
                "    {}: {} (call {}, pass {}, selected {})",
                scope_label(scope),
                comparison_label(candidate.comparison(scope)),
                format_value(candidate.call_value(scope)),
                format_pass_value(observation.pass(scope)),
                format_selected(selected),
            ));
        }
    }
    lines
}

// Chi も Pon と同じ形で、物理牌 (赤5 / 黒5) をそのまま表示する。
fn call_label(action: &LegalAction) -> String {
    match action {
        LegalAction::Chi { tile, consumed } => format!(
            "Chi {} <- {}",
            tile.to_mjai_string(),
            consumed
                .iter()
                .map(|tile| tile.to_mjai_string())
                .collect::<Vec<_>>()
                .join(" ")
        ),
        other => action_label(other),
    }
}

fn scope_label(scope: TwoShantenStayCallScope) -> &'static str {
    match scope {
        TwoShantenStayCallScope::Progress => "progress",
        TwoShantenStayCallScope::Full => "full",
    }
}

fn comparison_label(comparison: CallIishantenComparison) -> &'static str {
    match comparison {
        CallIishantenComparison::CallHigher => "call > pass",
        CallIishantenComparison::PassNotLower => "pass >= call",
        CallIishantenComparison::Unknown => "unknown",
    }
}

fn agreement_label(agreement: TwoShantenStayCallAgreement) -> &'static str {
    match agreement {
        TwoShantenStayCallAgreement::Same => "same",
        TwoShantenStayCallAgreement::Flipped => "flipped",
        TwoShantenStayCallAgreement::Undetermined => "undetermined (either scope unknown)",
    }
}

fn terminal_yaku_label(verdict: ProspectiveHanVerdict) -> &'static str {
    match verdict {
        ProspectiveHanVerdict::AtLeast => "AtLeast (all live variants >= 1 han)",
        ProspectiveHanVerdict::Below => "Below (contains a no-yaku live variant)",
        ProspectiveHanVerdict::Unknown => "Unknown",
    }
}

fn require_all_live_waits_label(conclusion: TwoShantenStayRequireAllLiveWaits) -> &'static str {
    match conclusion {
        TwoShantenStayRequireAllLiveWaits::Call => "Call",
        TwoShantenStayRequireAllLiveWaits::BlockedByPartialYaku => "Blocked (partial yaku)",
        TwoShantenStayRequireAllLiveWaits::Unknown => "Unknown",
        TwoShantenStayRequireAllLiveWaits::Pass => "Pass",
    }
}

fn allow_partial_waits_label(conclusion: TwoShantenStayAllowPartialWaits) -> &'static str {
    match conclusion {
        TwoShantenStayAllowPartialWaits::Call => "Call",
        TwoShantenStayAllowPartialWaits::Unknown => "Unknown",
        TwoShantenStayAllowPartialWaits::Pass => "Pass",
    }
}

fn unknown_label(cause: TwoShantenStayCallUnknown) -> &'static str {
    match cause {
        TwoShantenStayCallUnknown::ReactionSourceUnknown => "reaction source unknown",
        TwoShantenStayCallUnknown::NoPostCallSelection => "no post-call selection",
        TwoShantenStayCallUnknown::NoCompetingTargets => "no competing targets",
        TwoShantenStayCallUnknown::FullGateNotFired => "full gate not fired",
        TwoShantenStayCallUnknown::SelectedOutsideFullPair => "selected outside the full pair",
        TwoShantenStayCallUnknown::Unresolved => "unresolved",
    }
}

fn format_value(value: TwoShantenStayCallValue) -> String {
    match value {
        TwoShantenStayCallValue::Known(scaled) => format!(
            "{}.{:06}",
            scaled / SELF_TSUMO_VALUE_SCALE,
            scaled % SELF_TSUMO_VALUE_SCALE
        ),
        TwoShantenStayCallValue::Unknown(cause) => format!("unknown ({})", unknown_label(cause)),
    }
}

fn format_tiles(tiles: &[TileType]) -> String {
    if tiles.is_empty() {
        return "none".to_string();
    }
    tiles
        .iter()
        .map(|tile| tile.to_mjai_string())
        .collect::<Vec<_>>()
        .join(" ")
}

fn format_duration(duration: Duration) -> String {
    format!("{:.3} ms", duration.as_secs_f64() * 1_000.0)
}

#[cfg(test)]
mod tests {
    use bot_analysis::ScenarioSpec;
    use bot_core::{
        CallDecisionDiagnostic, CallDecisionReason, TwoShantenStayCallFull,
        TwoShantenStayCallProgress, TwoShantenStayCallTerminalYaku,
    };
    use bot_logic::{FixedMeldCount, TileId};
    use riichilab_client::capture::{self, CaptureDirection};
    use riichilab_client::observation::fixture_base64_with_discards;
    use tempfile::TempDir;

    use super::*;

    const CHI_DORA_GATE: &str =
        include_str!("../scenarios/two_shanten_stay_call_chi_dora_gate.json");
    const PON_CHI: &str = include_str!("../scenarios/two_shanten_stay_call_pon_chi.json");
    const TWO_TO_ONE_AND_TWO: &str = include_str!("../scenarios/issue_348_pon_5s_before_call.json");
    const NO_CALL: &str = include_str!("../scenarios/normal.json");
    const HAKU_PON: &str = include_str!("../scenarios/two_shanten_stay_call_haku_pon.json");
    const TANYAO_PON: &str = include_str!("../scenarios/two_shanten_stay_call_tanyao_pon.json");

    fn resolve(json: &str) -> Scenario {
        let spec: ScenarioSpec = serde_json::from_str(json).expect("scenario spec");
        Scenario::resolve(&spec).expect("scenario")
    }

    fn line_with<'a>(output: &'a str, prefix: &str) -> &'a str {
        output
            .lines()
            .find(|line| line.trim_start().starts_with(prefix))
            .unwrap_or_else(|| panic!("{prefix}: {output}"))
    }

    #[test]
    fn a_scenario_reports_both_scopes_and_a_different_selected_discard() {
        let output = format_scenario_observation(&resolve(CHI_DORA_GATE));

        assert!(
            output.starts_with("Two-shanten stay call observation"),
            "{output}"
        );
        assert!(
            output.contains("  selected: none\n  reason: PostCallNotIishanten"),
            "{output}"
        );
        assert!(
            output.contains("#0 Chi 8m <- 6m 7m: PostCallNotIishanten (2 -> 2 target)"),
            "{output}"
        );
        assert!(output.contains("Candidate #0 Chi 8m <- 6m 7m"), "{output}");
        let progress = line_with(&output, "progress: selected");
        assert!(
            progress.starts_with("  progress: selected F, call "),
            "{progress}"
        );
        assert!(progress.ends_with(", pass >= call"), "{progress}");
        let full = line_with(&output, "full: selected");
        assert!(full.starts_with("  full: selected 5p, call "), "{full}");
        assert!(!full.contains("unknown"), "{full}");
        assert!(full.ends_with(", pass >= call"), "{full}");
        assert!(output.contains("full evaluated: 2 (F 5p)"), "{output}");
        assert!(
            output.contains("  selected post-call discard: different"),
            "{output}"
        );
        assert!(
            output.contains("  call / pass conclusion progress vs full: same"),
            "{output}"
        );
        // 観測専用の出力で、通常の打牌診断は走らせない。
        assert!(!output.contains("Final decision"), "{output}");
    }

    #[test]
    fn a_scenario_reports_an_unknown_full_value_and_the_shared_pass() {
        let output = format_scenario_observation(&resolve(PON_CHI));

        assert!(
            output.contains("call candidates: 2, 2 -> 2 targets: 2"),
            "{output}"
        );
        assert!(output.contains("Candidate #0 Pon 4s <- 4s 4s"), "{output}");
        assert!(output.contains("Candidate #1 Chi 4s <- 3s 5s"), "{output}");
        assert_eq!(
            output.matches("unknown (call full gate not fired)").count(),
            2,
            "{output}"
        );
        assert_eq!(
            output
                .matches("  call / pass conclusion progress vs full: undetermined")
                .count(),
            2,
            "{output}"
        );
        assert_eq!(
            output.matches("  selected post-call discard: same").count(),
            2
        );
        // Pass は request で1回だけ評価し、候補ごとには出さない。
        let pass = output
            .split("Pass (evaluated once per request and scope, shared by every target)\n")
            .nth(1)
            .unwrap()
            .split("\n\n")
            .next()
            .unwrap();
        assert_eq!(pass.lines().count(), 2, "{pass}");
        assert_eq!(output.matches("Pass (evaluated").count(), 1, "{output}");
    }

    fn partial_yaku_lines(output: &str) -> Vec<&str> {
        output
            .split("  partial-yaku (progress, counterfactual)\n")
            .nth(1)
            .unwrap_or_else(|| panic!("{output}"))
            .lines()
            .take(3)
            .collect()
    }

    #[test]
    fn a_yakuhai_pon_is_a_call_under_both_partial_yaku_policies() {
        let output = format_scenario_observation(&resolve(HAKU_PON));

        assert!(
            output.contains("  reason: PostCallNotIishanten"),
            "{output}"
        );
        assert!(
            line_with(&output, "progress: selected").ends_with(", call > pass"),
            "{output}"
        );
        assert_eq!(
            partial_yaku_lines(&output),
            [
                "    terminal yaku (observation run): AtLeast (all live variants >= 1 han)",
                "    RequireAllLiveWaits: Call",
                "    AllowPartialWaits: Call",
            ]
        );
    }

    #[test]
    fn a_tanyao_dependent_pon_is_blocked_only_when_every_live_wait_needs_yaku() {
        let output = format_scenario_observation(&resolve(TANYAO_PON));

        assert!(
            output.contains("  reason: PostCallNotIishanten"),
            "{output}"
        );
        assert!(
            line_with(&output, "progress: selected").ends_with(", call > pass"),
            "{output}"
        );
        assert_eq!(
            partial_yaku_lines(&output),
            [
                "    terminal yaku (observation run): Below (contains a no-yaku live variant)",
                "    RequireAllLiveWaits: Blocked (partial yaku)",
                "    AllowPartialWaits: Call",
            ]
        );
    }

    #[test]
    fn a_request_without_a_two_shanten_stay_call_says_so() {
        let output = format_scenario_observation(&resolve(NO_CALL));

        assert!(output.contains("  no legal Chi / Pon"), "{output}");
        assert!(output.contains("No 2 -> 2 Chi / Pon candidate"), "{output}");
        assert!(!output.contains("Candidate #"), "{output}");
        assert!(!output.contains("Pass (evaluated"), "{output}");
    }

    // Issue #348 の局面は Pon 5s が 2→1、Chi 5s が 2→2。2→1 の候補は観測対象にしない。
    #[test]
    fn only_the_two_shanten_stay_call_of_a_mixed_request_is_observed() {
        let output = format_scenario_observation(&resolve(TWO_TO_ONE_AND_TWO));

        assert!(output.contains("  reason: PostCallNotPush"), "{output}");
        assert!(
            output.contains("call candidates: 2, 2 -> 2 targets: 1"),
            "{output}"
        );
        assert!(
            output.contains("    #0 Pon 5s <- 5sr 5s: PostCallNotPush\n"),
            "{output}"
        );
        assert!(
            output.contains("    #1 Chi 5s <- 3s 4s: PostCallNotIishanten (2 -> 2 target)"),
            "{output}"
        );
        assert!(!output.contains("Candidate #0"), "{output}");
        assert!(output.contains("Candidate #1 Chi 5s <- 3s 4s"), "{output}");
        assert!(
            output.contains("unknown (call no competing targets)"),
            "{output}"
        );
    }

    const KAMICHA_DAHAI_4S: &str = r#"{"type":"dahai","actor":3,"pai":"4s"}"#;
    const KAMICHA_DAHAI_8M: &str = r#"{"type":"dahai","actor":3,"pai":"8m"}"#;
    // 11m 5p6p 88p 3s44s5s 77s8s。ドラ表示 8p。
    const PON_CHI_HAND: [u8; 13] = [0, 1, 53, 56, 64, 65, 80, 84, 85, 89, 96, 97, 100];
    // 11m 567m 55p6p 1s3s 6s7s F。ドラ表示 P。
    const CHI_DORA_GATE_HAND: [u8; 13] = [0, 1, 17, 21, 25, 53, 54, 56, 72, 80, 92, 96, 128];

    fn server_line(event: &str) -> String {
        capture::record_line(CaptureDirection::Server, event).expect("capture 行を作れる")
    }

    fn reaction_request(
        request_id: u64,
        hand: &[u8],
        dora_indicator: u8,
        target: u8,
        possible_actions: &str,
    ) -> String {
        let mut discards: [Vec<u8>; 4] = Default::default();
        discards[3] = vec![target];
        let observation =
            fixture_base64_with_discards(0, None, hand.to_vec(), vec![dora_indicator], discards);
        server_line(&format!(
            r#"{{"type":"request_action","request_id":{request_id},"actor":0,"possible_actions":[{possible_actions},{{"type":"none"}}],"observation":"{observation}"}}"#
        ))
    }

    fn dahai_request(request_id: u64) -> String {
        let hand = [0u8, 4, 8, 12, 17, 20, 53, 54, 96, 100, 120, 124, 125];
        let observation =
            fixture_base64_with_discards(0, Some(59), hand.to_vec(), vec![], Default::default());
        let possible: Vec<_> = hand
            .iter()
            .copied()
            .chain([59])
            .map(|id| {
                let pai = TileId::new(id).unwrap().to_mjai_string();
                format!(r#"{{"type":"dahai","pai":"{pai}","tsumogiri":false}}"#)
            })
            .collect();
        server_line(&format!(
            r#"{{"type":"request_action","request_id":{request_id},"actor":0,"possible_actions":[{}],"observation":"{observation}"}}"#,
            possible.join(",")
        ))
    }

    #[test]
    fn the_capture_comparison_counts_requests_and_candidates_separately() {
        let directory = TempDir::new().expect("一時 directory を作れる");
        let path = directory.path().join("capture.jsonl");
        let lines = [
            server_line(KAMICHA_DAHAI_4S),
            reaction_request(
                11,
                &PON_CHI_HAND,
                66,
                86,
                r#"{"type":"pon","pai":"4s","consumed":["4s","4s"]},{"type":"chi","pai":"4s","consumed":["3s","5s"]}"#,
            ),
            dahai_request(12),
            server_line(KAMICHA_DAHAI_8M),
            reaction_request(
                13,
                &CHI_DORA_GATE_HAND,
                124,
                29,
                r#"{"type":"chi","pai":"8m","consumed":["6m","7m"]}"#,
            ),
        ];
        std::fs::write(&path, format!("{}\n", lines.join("\n"))).expect("capture を書ける");
        let spec = CaptureComparisonSpec {
            paths: vec![path.to_string_lossy().into_owned()],
        };

        let output = run_capture_comparison(&spec).expect("集計できる");

        for expected in [
            "  captures: 1",
            "  replayed requests: 3",
            "  requests with a legal Chi / Pon: 2",
            "  requests with a 2 -> 2 target: 2",
            "  2 -> 2 call candidates: 3",
            "  chi: 2",
            "  pon: 1",
            "  progress: call > pass 0, pass >= call 3, unknown 0",
            "  full: call > pass 0, pass >= call 1, unknown 2",
            "    unknown causes: call full gate not fired 2",
            "  same conclusion: 1",
            "  flipped conclusion: 0",
            "  undetermined (either scope unknown): 2",
            "  selected post-call discard: same 2, different 1, unavailable 0",
            "Progress call > pass partial-yaku policy (candidates, counterfactual)\n  candidates: 0",
            "RequireAllLiveWaits vs AllowPartialWaits differences (0)",
            "Flipped conclusions (showing up to 10 of 0)",
        ] {
            assert!(
                output.contains(&format!("{expected}\n")),
                "{expected}: {output}"
            );
        }
        // latency は request 単位の Pass と、評価した候補単位の Call を分けて数える。
        assert!(
            output.contains("  progress pass (per request): 2 / "),
            "{output}"
        );
        assert!(
            output.contains("  full pass (per request): 2 / "),
            "{output}"
        );
        assert!(
            output.contains("  progress call (per evaluated candidate): 3 / "),
            "{output}"
        );
        assert!(
            output.contains("  full call (per evaluated candidate): 3 / "),
            "{output}"
        );
        assert_eq!(
            output
                .split("Slowest full calls (evaluated candidates)\n")
                .nth(1)
                .unwrap()
                .split("\n\n")
                .next()
                .unwrap()
                .lines()
                .count(),
            3,
            "{output}"
        );
        assert!(
            output.contains("request_id=11  production=none (PostCallNotIishanten)  targets=2")
        );
        assert!(
            output.contains("request_id=13  production=none (PostCallNotIishanten)  targets=1")
        );
        assert!(!output.contains("request_id=12  production"), "{output}");
    }

    fn synthetic_candidate(
        index: usize,
        kind: CallKind,
        progress: (u64, CallIishantenComparison),
        full: (TwoShantenStayCallValue, CallIishantenComparison),
        reused_from: Option<usize>,
    ) -> TwoShantenStayCallCandidate {
        let discard = LegalAction::Dahai {
            tile: TileId::new(0).unwrap(),
        };
        TwoShantenStayCallCandidate {
            candidate_index: index,
            action: LegalAction::Pon {
                tile: TileId::new(2).unwrap(),
                consumed: vec![TileId::new(0).unwrap(), TileId::new(1).unwrap()],
            },
            kind,
            reused_from,
            forbidden_discards: Vec::new(),
            post_call_fixed_meld_count: FixedMeldCount::new(1).unwrap(),
            post_call_min_shanten: 2,
            progress: TwoShantenStayCallProgress {
                selected: Some(discard.clone()),
                value: TwoShantenStayCallValue::Known(progress.0),
                elapsed: Duration::from_millis(10),
                search: ThreeShantenSearchStats::default(),
                memo: SearchStateMemoStats::default(),
                runs_agree: true,
                terminal_yaku: TwoShantenStayCallTerminalYaku {
                    verdict: ProspectiveHanVerdict::AtLeast,
                    reproduces_value: true,
                },
            },
            full: TwoShantenStayCallFull {
                selected: Some(discard),
                value: full.0,
                progress_cohort: Vec::new(),
                full_evaluated: Vec::new(),
                elapsed: Duration::from_millis(100 + index as u64),
                search: ThreeShantenSearchStats::default(),
                memo: SearchStateMemoStats::default(),
                full_workers: 1,
                runs_agree: true,
            },
            progress_comparison: progress.1,
            full_comparison: full.1,
        }
    }

    fn synthetic_request(
        request_id: u64,
        candidates: Vec<TwoShantenStayCallCandidate>,
    ) -> ObservedRequest {
        let pass = TwoShantenStayPassObservation {
            value: TwoShantenStayCallValue::Known(50),
            elapsed: Some(Duration::from_millis(5)),
        };
        ObservedRequest {
            capture: "synthetic.jsonl".to_string(),
            request_id,
            observation: TwoShantenStayCallObservation {
                call: Some(CallDecisionDiagnostic {
                    selected: None,
                    reason: CallDecisionReason::PostCallNotIishanten,
                    candidates: Vec::new(),
                }),
                reaction_source_player: Some(3),
                progress_pass: Some(pass),
                full_pass: Some(pass),
                candidates,
            },
        }
    }

    #[test]
    fn the_summary_separates_flipped_same_and_unknown_conclusions() {
        use CallIishantenComparison::{CallHigher, PassNotLower, Unknown};
        let requests = vec![
            synthetic_request(
                1,
                vec![
                    synthetic_candidate(
                        0,
                        CallKind::Pon,
                        (60, CallHigher),
                        (TwoShantenStayCallValue::Known(40), PassNotLower),
                        None,
                    ),
                    synthetic_candidate(
                        1,
                        CallKind::Pon,
                        (60, CallHigher),
                        (TwoShantenStayCallValue::Known(40), PassNotLower),
                        Some(0),
                    ),
                ],
            ),
            synthetic_request(
                2,
                vec![synthetic_candidate(
                    0,
                    CallKind::Chi,
                    (10, PassNotLower),
                    (
                        TwoShantenStayCallValue::Unknown(
                            TwoShantenStayCallUnknown::NoCompetingTargets,
                        ),
                        Unknown,
                    ),
                    None,
                )],
            ),
        ];

        let summary = Summary::from_requests(4, &requests);

        assert_eq!(summary.replayed_requests, 4);
        assert_eq!(summary.call_requests, 2);
        assert_eq!(summary.target_requests, 2);
        assert_eq!(summary.candidates, 3);
        assert_eq!((summary.chi, summary.pon, summary.reused), (1, 2, 1));
        assert_eq!(
            (
                summary.progress.call_higher,
                summary.progress.pass_not_lower,
                summary.progress.unknown
            ),
            (2, 1, 0)
        );
        assert_eq!(
            (
                summary.full.call_higher,
                summary.full.pass_not_lower,
                summary.full.unknown
            ),
            (0, 2, 1)
        );
        assert_eq!(
            summary.full.unknown_causes,
            BTreeMap::from([("call no competing targets".to_string(), 1)])
        );
        assert_eq!(
            (
                summary.same_conclusion,
                summary.flipped_conclusion,
                summary.undetermined_conclusion
            ),
            (0, 2, 1)
        );

        let output = format_capture_comparison(1, 4, &requests);
        assert!(
            output.contains("Flipped conclusions (showing up to 10 of 2)\n  synthetic.jsonl  request_id=1  #0 Pon 1m <- 1m 1m\n    progress: call > pass (call 0.000060, pass 0.000050, selected 1m)\n    full: pass >= call (call 0.000040, pass 0.000050, selected 1m)\n  synthetic.jsonl  request_id=1  #1 Pon"),
            "{output}"
        );
        // 複製した候補は latency の標本に入れない。
        assert!(
            output.contains("  full call (per evaluated candidate): 2 / "),
            "{output}"
        );
        let slowest = output
            .split("Slowest full calls (evaluated candidates)\n")
            .nth(1)
            .unwrap();
        assert!(
            slowest.starts_with("  100.000 ms  synthetic.jsonl  request_id=1  #0"),
            "{slowest}"
        );
    }

    #[test]
    fn the_partial_yaku_summary_counts_call_higher_candidates_and_lists_every_difference() {
        use CallIishantenComparison::{CallHigher, PassNotLower, Unknown};
        let full = (
            TwoShantenStayCallValue::Unknown(TwoShantenStayCallUnknown::FullGateNotFired),
            Unknown,
        );
        let with_yaku = |mut candidate: TwoShantenStayCallCandidate, verdict| {
            candidate.progress.terminal_yaku.verdict = verdict;
            candidate
        };
        let requests = vec![
            synthetic_request(
                1,
                vec![
                    with_yaku(
                        synthetic_candidate(0, CallKind::Pon, (60, CallHigher), full, None),
                        ProspectiveHanVerdict::AtLeast,
                    ),
                    with_yaku(
                        synthetic_candidate(1, CallKind::Chi, (70, CallHigher), full, None),
                        ProspectiveHanVerdict::Below,
                    ),
                    with_yaku(
                        synthetic_candidate(2, CallKind::Chi, (70, CallHigher), full, Some(1)),
                        ProspectiveHanVerdict::Below,
                    ),
                ],
            ),
            synthetic_request(
                2,
                vec![
                    with_yaku(
                        synthetic_candidate(0, CallKind::Pon, (80, CallHigher), full, None),
                        ProspectiveHanVerdict::Unknown,
                    ),
                    // Pass 以上の候補は役なしの terminal があっても数えない。
                    with_yaku(
                        synthetic_candidate(1, CallKind::Chi, (10, PassNotLower), full, None),
                        ProspectiveHanVerdict::Below,
                    ),
                ],
            ),
        ];

        let summary = Summary::from_requests(2, &requests);

        assert_eq!(summary.candidates, 5);
        assert_eq!(
            summary.partial_yaku,
            PartialYakuCounts {
                candidates: 4,
                yaku_at_least: 1,
                yaku_below: 2,
                yaku_unknown: 1,
                require_all_call: 1,
                require_all_blocked: 2,
                require_all_unknown: 1,
                allow_partial_call: 4,
            }
        );

        let output = format_capture_comparison(1, 2, &requests);
        for expected in [
            "Progress call > pass partial-yaku policy (candidates, counterfactual)\n  candidates: 4\n  terminal yaku:\n    all live variants >= 1 han: 1\n    contains no-yaku live variant: 2\n    unknown: 1\n  RequireAllLiveWaits:\n    call: 1\n    blocked by partial yaku: 2\n    unknown: 1\n  AllowPartialWaits:\n    call: 4\n",
            "RequireAllLiveWaits vs AllowPartialWaits differences (3)\n  synthetic.jsonl  request_id=1  #1 Pon 1m <- 1m 1m\n    progress: selected 1m, call 0.000070, pass 0.000050, call > pass\n    terminal yaku: Below (contains a no-yaku live variant), RequireAllLiveWaits: Blocked (partial yaku), AllowPartialWaits: Call\n  synthetic.jsonl  request_id=1  #2 Pon 1m <- 1m 1m  reused_from=#1\n",
            "  synthetic.jsonl  request_id=2  #0 Pon 1m <- 1m 1m\n    progress: selected 1m, call 0.000080, pass 0.000050, call > pass\n    terminal yaku: Unknown, RequireAllLiveWaits: Unknown, AllowPartialWaits: Call\n\n",
        ] {
            assert!(output.contains(expected), "{expected}\n{output}");
        }
    }
}
