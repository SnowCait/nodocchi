mod benchmark;
mod cli;
#[cfg(test)]
mod combined_defense;
mod error;
mod format;
mod input;
#[cfg(test)]
mod open_hand_defense;
#[cfg(test)]
mod open_hand_threat;
mod replay;
mod scenario;
mod tiles;

use std::process::ExitCode;

use bot_core::{
    DiagnosticOptions, ShantenAgent, measure_two_shanten_progress_self_tsumo,
    measure_two_shanten_self_tsumo,
};

use crate::benchmark::run_capture_benchmark;
use crate::cli::{CliArgs, ScenarioSource, USAGE};
use crate::error::ScenarioError;
use crate::format::{
    format_diagnostic, format_summary, format_two_shanten_progress_self_tsumo_cost,
    format_two_shanten_self_tsumo_cost,
};
use crate::replay::load_captured_scenario;
use crate::scenario::{Scenario, ScenarioSpec};

fn main() -> ExitCode {
    match run(std::env::args().skip(1)) {
        Ok(output) => {
            println!("{output}");
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("error: {error}");
            if error.is_usage_error() {
                eprintln!("{USAGE}");
            }
            ExitCode::FAILURE
        }
    }
}

fn run<I>(args: I) -> Result<String, ScenarioError>
where
    I: IntoIterator<Item = String>,
{
    let args = CliArgs::parse(args)?;
    let (header, scenario) = match &args.source {
        ScenarioSource::Json(path) => (None, Scenario::resolve(&read_spec(path)?)?),
        ScenarioSource::Inline(spec) => (None, Scenario::resolve(spec)?),
        ScenarioSource::RiichilabCapture { path, request_id } => {
            let captured = load_captured_scenario(path, *request_id)?;
            (Some(captured.header()), captured.scenario)
        }
        ScenarioSource::RiichilabCaptureBenchmark(spec) => return run_capture_benchmark(spec),
    };

    // cost 計測は production selection が同じ2向聴探索を走らせる前に取り、baseline と同じ
    // cold memo 条件を保つ。表示順は従来どおり診断の後にする。
    let two_shanten_self_tsumo_cost = args.two_shanten_self_tsumo_cost.map(|scope| {
        (
            scope,
            measure_two_shanten_self_tsumo(&scenario.context, &scenario.legal_actions, scope),
        )
    });
    let two_shanten_progress_self_tsumo_cost =
        args.two_shanten_progress_self_tsumo_cost.map(|scope| {
            (
                scope,
                measure_two_shanten_progress_self_tsumo(
                    &scenario.context,
                    &scenario.legal_actions,
                    scope,
                ),
            )
        });
    let diagnostic = ShantenAgent::diagnose_with_options(
        &scenario.context,
        &scenario.legal_actions,
        diagnostic_options(&args),
    );

    let output = if args.summary_only {
        format_summary(&scenario, &diagnostic)
    } else {
        format_diagnostic(&scenario, &diagnostic, args.verbose)
    };
    // 計測は他の診断とは別の section として後ろに足すだけで、その手前の判断も表示も変えない。
    let output = match two_shanten_self_tsumo_cost {
        Some((scope, cost)) => {
            let section = format_two_shanten_self_tsumo_cost(
                scope,
                &cost,
                diagnostic.normal_discard_self_tsumo_facts,
            );
            format!("{output}\n\n{section}")
        }
        None => output,
    };
    let output = match two_shanten_progress_self_tsumo_cost {
        Some((scope, cost)) => {
            let section = format_two_shanten_progress_self_tsumo_cost(
                scope,
                &cost,
                diagnostic.normal_discard_self_tsumo_facts,
            );
            format!("{output}\n\n{section}")
        }
        None => output,
    };
    Ok(match header {
        Some(header) => format!("{header}\n\n{output}"),
        None => output,
    })
}

// CLI option から構築する診断の範囲。追加の深い探索は互いに独立で、要求されたものだけを
// 構築する。same-shanten の枝をテンパイまで追う探索は枝の詳細を出す --verbose と組み合わせた
// 場合だけ、2向聴候補の ExpectedSelfTsumoValue は --two-shanten-self-tsumo を指定した場合だけに
// なる。診断の範囲は選択結果を変えない。
fn diagnostic_options(args: &CliArgs) -> DiagnosticOptions {
    DiagnosticOptions {
        lookahead: args.lookahead,
        same_shanten_downstream: args.lookahead && args.verbose,
        two_shanten_self_tsumo: args.two_shanten_self_tsumo,
    }
}

fn read_spec(path: &str) -> Result<ScenarioSpec, ScenarioError> {
    let text = std::fs::read_to_string(path).map_err(|error| ScenarioError::ReadFile {
        path: path.to_string(),
        message: error.to_string(),
    })?;

    serde_json::from_str(&text).map_err(|error| ScenarioError::Json {
        path: path.to_string(),
        message: error.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run_args(args: &[&str]) -> Result<String, ScenarioError> {
        run(args.iter().map(|arg| arg.to_string()))
    }

    fn options_of(args: &[&str]) -> DiagnosticOptions {
        let parsed = CliArgs::parse(args.iter().map(|arg| arg.to_string())).unwrap();
        diagnostic_options(&parsed)
    }

    #[test]
    fn the_diagnostic_scope_options_are_independent() {
        // 追加の深い探索は互いに含まない。--two-shanten-self-tsumo 単独では
        // same-shanten downstream を構築しない。
        assert_eq!(options_of(&["--hand", "123m"]), DiagnosticOptions::NONE);
        assert_eq!(
            options_of(&["--hand", "123m", "--lookahead"]),
            DiagnosticOptions::WITH_LOOKAHEAD
        );
        assert_eq!(
            options_of(&["--hand", "123m", "--lookahead", "--verbose"]),
            DiagnosticOptions::WITH_SAME_SHANTEN_DOWNSTREAM
        );
        assert_eq!(
            options_of(&["--hand", "123m", "--two-shanten-self-tsumo"]),
            DiagnosticOptions::WITH_TWO_SHANTEN_SELF_TSUMO
        );
        assert_eq!(
            options_of(&["--hand", "123m", "--two-shanten-self-tsumo", "--verbose"]),
            DiagnosticOptions::WITH_SAME_SHANTEN_DOWNSTREAM_AND_TWO_SHANTEN_SELF_TSUMO
        );
    }

    #[test]
    fn runs_a_simple_cli_scenario() {
        let output = run_args(&["--hand", "234m455p789s1123z", "--draw", "N"]).unwrap();
        assert!(output.starts_with("Scenario\n"), "{output}");
        assert!(output.contains("\n\nFinal decision\n"), "{output}");
        assert!(output.contains("\n\nNormal discard candidates"), "{output}");
    }

    // 追加オプション無しの何切る CLI でも、打 W のテンパイからリーチが生成されて選ばれる。
    #[test]
    fn a_menzen_tenpai_cli_scenario_selects_reach_without_any_option() {
        let output = run_args(&[
            "--hand",
            "12388m56p234789s3z",
            "--dora-indicator",
            "7s",
            "--summary-only",
        ])
        .unwrap();

        assert!(
            output.starts_with(
                "Summary\n  choice 1: Reach\n  choice 1 discard: W\n  choice 1 source: Reach\n"
            ),
            "{output}"
        );
    }

    #[test]
    fn reported_reach_scenario_shows_three_production_choices() {
        let args = ["--hand", "34599m235p345567s"];
        let full = run_args(&args).unwrap();
        let summary = run_args(&[args.as_slice(), &["--summary-only"]].concat()).unwrap();

        assert!(
            full.contains("Final decision\n  action: Reach\n  discard: 2p\n  source: Reach"),
            "{full}"
        );
        assert!(
            summary.starts_with(
                "Summary\n  choice 1: Reach\n  choice 1 discard: 2p\n  choice 1 source: Reach"
            ),
            "{summary}"
        );
        assert!(
            summary.contains("  choice 2: 2p\n  choice 2 source: NormalDiscard"),
            "{summary}"
        );
        assert!(
            summary.contains("  choice 3: 5p\n  choice 3 source: NormalDiscard"),
            "{summary}"
        );
        assert!(
            summary.contains("  choice 3 lost by: CurrentTenpaiOffenseWeightedTotal"),
            "{summary}"
        );
        assert!(
            full.contains("  current tenpai offense weighted total: 20800"),
            "{full}"
        );
        assert!(
            full.contains("  current tenpai offense weighted total: 16000"),
            "{full}"
        );
        assert!(full.ends_with(&summary), "{full}");
    }

    #[test]
    fn request_407_safe_tenpai_discard_pushes_in_the_summary() {
        let path = format!(
            "{}/scenarios/request_407_safe_tenpai.json",
            env!("CARGO_MANIFEST_DIR")
        );
        let output = run([path, "--summary-only".to_string()]).unwrap();

        assert!(
            output.starts_with(
                "Summary\n  choice 1: Reach\n  choice 1 discard: 5m\n  choice 1 source: Reach"
            ),
            "{output}"
        );
        assert!(output.contains("  push/pull: Push"), "{output}");
        assert!(
            output.contains("  push/pull reason: SafeTenpaiAgainstHighOpenHand"),
            "{output}"
        );
        assert!(
            output.contains("  offense live wait: 5 remaining / 2 types"),
            "{output}"
        );
        assert!(output.contains("  offense furiten: no"), "{output}");
        assert!(
            output.contains("  offense value: Reach 2600 / total: 13000"),
            "{output}"
        );
        assert!(
            output.contains("  strong tenpai requirement: weighted total >= 15600"),
            "{output}"
        );
    }

    #[test]
    fn explicit_inline_baseline_facts_select_the_same_current_tenpai_value() {
        let args = [
            "--hand",
            "34599m235p345567s",
            "--player-id",
            "0",
            "--oya",
            "1",
            "--round-wind",
            "E",
            "--no-history-furiten",
        ];
        let full = run_args(&args).unwrap();
        let summary = run_args(&[args.as_slice(), &["--summary-only"]].concat()).unwrap();

        assert!(
            summary.starts_with(
                "Summary\n  choice 1: Reach\n  choice 1 discard: 2p\n  choice 1 source: Reach"
            ),
            "{summary}"
        );
        assert!(
            summary.contains("  choice 3: 5p\n  choice 3 source: NormalDiscard\n  choice 3 lost by: CurrentTenpaiOffenseWeightedTotal"),
            "{summary}"
        );
        assert!(
            full.contains("  current tenpai offense weighted total: 20800"),
            "{full}"
        );
        assert!(
            full.contains("  current tenpai offense weighted total: 16000"),
            "{full}"
        );
        assert!(full.ends_with(&summary), "{full}");
    }

    #[test]
    fn runs_a_simple_cli_scenario_with_red_five() {
        let output = run_args(&["--hand", "340m455p789s1123z", "--draw", "N"]).unwrap();
        assert!(output.contains("5mr"), "{output}");
    }

    #[test]
    fn dora_indicator_fills_the_scenario_dora_indicators() {
        let output = run_args(&[
            "--hand",
            "234m455p789s1123z",
            "--draw",
            "N",
            "--dora-indicator",
            "3p E",
        ])
        .unwrap();
        assert!(output.contains("  dora indicators: 3p E"), "{output}");

        let alias = run_args(&[
            "--hand",
            "234m455p789s1123z",
            "--draw",
            "N",
            "--dora",
            "3p E",
        ])
        .unwrap();
        assert_eq!(output, alias);
    }

    #[test]
    fn summary_only_prints_the_summary_section_only() {
        let hand = ["--hand", "234m455p789s1123z", "--draw", "N"];
        let full = run_args(&hand).unwrap();
        let summary = run_args(&[hand.as_slice(), &["--summary-only"]].concat()).unwrap();

        assert!(summary.starts_with("Summary\n"), "{summary}");
        for name in [
            "Scenario",
            "Table state",
            "History furiten",
            "Final decision",
            "Normal discard",
            "Push/Pull",
            "Reach",
            "Defense",
            "Player threats",
        ] {
            let header = format!("\n\n{name}\n");
            assert!(!summary.contains(&header), "{name} in {summary}");
            assert!(
                full.contains(&header) || full.starts_with(&format!("{name}\n")),
                "{name} missing from {full}"
            );
        }
        assert!(full.ends_with(&summary), "{full}");
    }

    #[test]
    fn summary_only_keeps_the_capture_header() {
        let observation = riichilab_client::observation::fixture_base64(
            0,
            Some(59),
            vec![0, 4, 8, 12, 17, 20, 53, 54, 96, 100, 120, 124, 125],
        );
        let line = riichilab_client::capture::record_line(
            riichilab_client::CaptureDirection::Server,
            &format!(
                r#"{{"type":"request_action","request_id":425,"possible_actions":[{{"type":"dahai","pai":"1m","tsumogiri":false}},{{"type":"dahai","pai":"6p","tsumogiri":true}}],"observation":"{observation}"}}"#
            ),
        )
        .unwrap();
        let path = std::env::temp_dir().join(format!(
            "bot-scenario-main-summary-only-capture-{}.jsonl",
            std::process::id()
        ));
        std::fs::write(&path, format!("{line}\n")).unwrap();

        let full = run_args(&["--riichilab-capture", path.to_str().unwrap()]).unwrap();
        let summary_only = run_args(&[
            "--riichilab-capture",
            path.to_str().unwrap(),
            "--summary-only",
        ])
        .unwrap();
        std::fs::remove_file(&path).unwrap();

        assert!(
            summary_only.starts_with("RiichiLab capture\n"),
            "{summary_only}"
        );
        assert!(summary_only.contains("  request_id: 425"), "{summary_only}");
        assert!(summary_only.contains("\n\nSummary\n"), "{summary_only}");
        assert!(!summary_only.contains("\n\nScenario\n"), "{summary_only}");
        assert!(
            !summary_only.contains("\n\nPlayer threats\n"),
            "{summary_only}"
        );

        let header = summary_only.split("\n\nSummary\n").next().unwrap();
        assert!(full.starts_with(header), "{full}");
        assert!(
            full.ends_with(summary_only.split_once("\n\n").unwrap().1),
            "{full}"
        );
    }

    #[test]
    fn reports_summary_only_conflicts_as_usage_errors() {
        for args in [
            ["--hand", "123m", "--summary-only", "--lookahead"],
            ["--hand", "123m", "--summary-only", "--verbose"],
        ] {
            let error = run_args(&args).unwrap_err();
            assert!(error.is_usage_error(), "{error:?}");
            assert!(
                error
                    .to_string()
                    .starts_with("--summary-only cannot be combined with"),
                "{error}"
            );
        }
    }

    #[test]
    fn reports_missing_hand_as_usage_error() {
        let error = run_args(&[]).unwrap_err();
        assert!(error.is_usage_error(), "{error:?}");
        assert_eq!(error.to_string(), "--hand is required");
    }

    #[test]
    fn reports_invalid_tiles() {
        let error = run_args(&["--hand", "123x"]).unwrap_err();
        assert!(!error.is_usage_error(), "{error:?}");
        assert!(error.to_string().contains("hand"), "{error}");
        assert!(error.to_string().contains("123x"), "{error}");
    }

    #[test]
    fn reports_missing_scenario_file() {
        let error = run_args(&["missing-scenario.json"]).unwrap_err();
        assert!(
            matches!(&error, ScenarioError::ReadFile { path, .. } if path == "missing-scenario.json"),
            "{error:?}"
        );
    }

    #[test]
    fn reports_invalid_scenario_json() {
        let path = std::env::temp_dir().join("bot-scenario-invalid-json.json");
        std::fs::write(&path, "{ \"hand\": ").unwrap();
        let error = run_args(&[path.to_str().unwrap()]).unwrap_err();
        std::fs::remove_file(&path).unwrap();
        assert!(matches!(&error, ScenarioError::Json { .. }), "{error:?}");
    }

    #[test]
    fn runs_a_json_scenario() {
        let path = std::env::temp_dir().join("bot-scenario-json-scenario.json");
        std::fs::write(
            &path,
            r#"{
                "hand": "234m455p789s1123z",
                "draw": "N",
                "dora_indicators": "3p",
                "round_wind": "E",
                "seat_wind": "N",
                "player_id": 0,
                "oya": 1,
                "reached": [false, true, false, false],
                "discards": ["", "1m 4m 7p E", "", ""]
            }"#,
        )
        .unwrap();
        let output = run_args(&[path.to_str().unwrap()]).unwrap();
        std::fs::remove_file(&path).unwrap();

        assert!(output.contains("  reached players: 1"), "{output}");
        assert!(output.contains("  discards[1]: 1m 4m 7p E"), "{output}");
        assert!(output.contains("\n\nPush/Pull\n"), "{output}");
        assert!(output.contains("\n\nDefense\n"), "{output}");
    }

    #[test]
    fn runs_a_captured_riichilab_request() {
        let observation = riichilab_client::observation::fixture_base64(
            0,
            Some(59),
            vec![0, 4, 8, 12, 17, 20, 53, 54, 96, 100, 120, 124, 125],
        );
        let line = riichilab_client::capture::record_line(
            riichilab_client::CaptureDirection::Server,
            &format!(
                r#"{{"type":"request_action","request_id":425,"possible_actions":[{{"type":"dahai","pai":"1m","tsumogiri":false}},{{"type":"dahai","pai":"6p","tsumogiri":true}}],"observation":"{observation}"}}"#
            ),
        )
        .unwrap();
        let path = std::env::temp_dir().join(format!(
            "bot-scenario-main-capture-{}.jsonl",
            std::process::id()
        ));
        std::fs::write(&path, format!("{line}\n")).unwrap();

        let output = run_args(&["--riichilab-capture", path.to_str().unwrap()]).unwrap();
        let selected = run_args(&[
            "--riichilab-capture",
            path.to_str().unwrap(),
            "--request-id",
            "425",
        ])
        .unwrap();
        std::fs::remove_file(&path).unwrap();

        assert_eq!(output, selected);
        assert!(output.starts_with("RiichiLab capture\n"), "{output}");
        assert!(output.contains("  request_id: 425"), "{output}");
        assert!(output.contains("\n\nScenario\n"), "{output}");
        assert!(output.contains("\n\nPush/Pull\n"), "{output}");
        assert!(output.contains("\n\nPlayer threats\n"), "{output}");
        assert!(output.contains("\n\nSummary\n"), "{output}");
    }

    fn write_benchmark_capture(name: &str, request_ids: &[u64]) -> String {
        let observation = riichilab_client::observation::fixture_base64(
            0,
            Some(128),
            vec![0, 12, 24, 36, 48, 60, 72, 84, 96, 108, 116, 124, 132],
        );
        let text = request_ids
            .iter()
            .map(|request_id| {
                riichilab_client::capture::record_line(
                    riichilab_client::CaptureDirection::Server,
                    &format!(
                        r#"{{"type":"request_action","request_id":{request_id},"possible_actions":[{{"type":"dahai","pai":"1m","tsumogiri":false}},{{"type":"dahai","pai":"F","tsumogiri":true}}],"observation":"{observation}"}}"#
                    ),
                )
                .unwrap()
            })
            .collect::<Vec<_>>()
            .join("\n");
        let path = std::env::temp_dir().join(format!(
            "bot-scenario-main-benchmark-{name}-{}.jsonl",
            std::process::id()
        ));
        std::fs::write(&path, format!("{text}\n")).unwrap();
        path.to_str().unwrap().to_string()
    }

    #[test]
    fn benchmarks_every_request_of_multiple_captures() {
        let first = write_benchmark_capture("first", &[425, 426]);
        let second = write_benchmark_capture("second", &[517]);

        let output = run_args(&[
            "--benchmark-riichilab-capture",
            first.as_str(),
            second.as_str(),
        ])
        .unwrap();
        let _ = std::fs::remove_file(&first);
        let _ = std::fs::remove_file(&second);

        assert!(
            output.starts_with("RiichiLab production latency benchmark\n"),
            "{output}"
        );
        assert!(output.contains("\n  captures: 2\n"), "{output}");
        assert!(output.contains("\n  requests: 3\n"), "{output}");
        assert!(output.contains("\n  p99: "), "{output}");
        assert!(output.contains("\n  > 3 s: "), "{output}");
        assert!(output.contains("\n\nSlowest requests\n"), "{output}");
        for request_id in [425, 426, 517] {
            assert!(
                output.contains(&format!("request_id={request_id}  early=")),
                "{output}"
            );
        }
        assert!(
            output.contains("  normal_discard=") && output.contains("  post_discard="),
            "{output}"
        );
        assert!(
            output.contains(" (base=")
                && output.contains(" forward=")
                && output.contains(" finalize="),
            "{output}"
        );
        assert!(output.contains("  selected="), "{output}");
        assert!(output.contains(&first), "{output}");
        assert!(output.contains(&second), "{output}");

        assert!(!output.contains("Push/Pull"), "{output}");
        assert!(!output.contains("Player threats"), "{output}");
    }

    #[test]
    fn benchmark_writes_the_machine_readable_output() {
        let capture = write_benchmark_capture("json", &[425, 426]);
        let json_path = std::env::temp_dir().join(format!(
            "bot-scenario-main-benchmark-json-{}.json",
            std::process::id()
        ));

        run_args(&[
            "--benchmark-riichilab-capture",
            capture.as_str(),
            "--benchmark-json",
            json_path.to_str().unwrap(),
        ])
        .unwrap();
        let text = std::fs::read_to_string(&json_path).unwrap();
        let _ = std::fs::remove_file(&capture);
        let _ = std::fs::remove_file(&json_path);

        let json: crate::benchmark::BenchmarkJson = serde_json::from_str(&text).unwrap();
        assert_eq!(json.summary.captures, 1);
        assert_eq!(json.summary.requests, 2);
        assert_eq!(
            json.requests
                .iter()
                .map(|request| request.request_id)
                .collect::<Vec<_>>(),
            vec![425, 426]
        );
        assert!(
            json.requests
                .iter()
                .all(|request| request.capture == capture && !request.selected.is_empty())
        );
    }

    #[test]
    fn reports_a_missing_benchmark_capture_file() {
        let error = run_args(&[
            "--benchmark-riichilab-capture",
            "missing-benchmark-capture.jsonl",
        ])
        .unwrap_err();

        assert!(
            matches!(&error, ScenarioError::ReadFile { path, .. } if path == "missing-benchmark-capture.jsonl"),
            "{error:?}"
        );
    }

    #[test]
    fn reports_a_missing_capture_file() {
        let error = run_args(&["--riichilab-capture", "missing-capture.jsonl"]).unwrap_err();
        assert!(
            matches!(&error, ScenarioError::ReadFile { path, .. } if path == "missing-capture.jsonl"),
            "{error:?}"
        );
    }

    #[test]
    fn lookahead_is_opt_in() {
        // 2手先は重い探索なので既定では計算せず表示もしない。小さい手牌で確認する。
        let default = run_args(&["--hand", "12m12p55s", "--draw", "9p"]).unwrap();
        assert!(!default.contains("Lookahead"), "{default}");

        let lookahead = run_args(&["--hand", "12m12p55s", "--draw", "9p", "--lookahead"]).unwrap();
        assert!(lookahead.contains("\n\nLookahead\n"), "{lookahead}");
        assert!(lookahead.contains("draws: "), "{lookahead}");
        assert!(!lookahead.contains("next discard:"), "{lookahead}");
    }

    #[test]
    fn tenpai_continuation_follows_the_lookahead_option() {
        // 現在聴牌のダマ継続は既存の --lookahead と同じ範囲でだけ出す。
        let hand = ["--hand", "123m456m789m123p1z", "--draw", "2z"];
        let default = run_args(&hand).unwrap();
        assert!(!default.contains("Tenpai continuation"), "{default}");

        let summary_only = run_args(&[hand.as_slice(), &["--summary-only"]].concat()).unwrap();
        assert!(
            !summary_only.contains("Tenpai continuation"),
            "{summary_only}"
        );
        assert!(!summary_only.contains("Lookahead"), "{summary_only}");
        // self-tsumo 比較のための点数計算も Summary だけの経路では行わない。
        assert!(
            !summary_only.contains("self-tsumo comparison"),
            "{summary_only}"
        );

        let lookahead = run_args(&[hand.as_slice(), &["--lookahead"]].concat()).unwrap();
        assert!(
            lookahead.contains("\n\nTenpai continuation\n"),
            "{lookahead}"
        );
        assert!(lookahead.contains("    current wait: "), "{lookahead}");
        assert!(
            lookahead.contains("    continuation branches: "),
            "{lookahead}"
        );
        assert!(
            lookahead.contains("    self-tsumo comparison"),
            "{lookahead}"
        );
        assert!(!lookahead.contains("      new wait: "), "{lookahead}");
    }

    #[test]
    fn the_reach_damaten_comparison_stays_out_of_the_summary() {
        // 統合表示は detailed diagnostics の section で、Summary には足さない。
        let hand = ["--hand", "340678m789p34789s", "--remaining-tiles", "70"];
        let default = run_args(&hand).unwrap();
        assert!(
            default.contains("\n\nReach / Damaten comparison\n"),
            "{default}"
        );
        // 2手先探索を要求していない局面では self-tsumo の材料を作らない。
        assert!(default.contains("  self-tsumo: unavailable"), "{default}");
        assert!(default.contains("    reach baseline"), "{default}");

        let summary_only = run_args(&[hand.as_slice(), &["--summary-only"]].concat()).unwrap();
        assert!(
            !summary_only.contains("Reach / Damaten comparison"),
            "{summary_only}"
        );
        assert!(!summary_only.contains("reach baseline"), "{summary_only}");
        assert!(!summary_only.contains("damaten baseline"), "{summary_only}");

        // --lookahead を付けた場合だけ self-tsumo の比較まで並ぶ。
        let lookahead = run_args(&[hand.as_slice(), &["--lookahead"]].concat()).unwrap();
        assert!(
            lookahead.contains("  self-tsumo (expected tsumo payment)"),
            "{lookahead}"
        );
        assert!(lookahead.contains("    reach now: 1460.235"), "{lookahead}");
    }

    #[test]
    fn the_inline_baseline_supplies_remaining_tiles_to_the_self_tsumo_comparison() {
        let output = run_args(&["--hand", "340678m789p34789s", "--lookahead"]).unwrap();

        assert!(output.contains("      defer one draw"), "{output}");
        for label in [
            "      reach now: ",
            "        production policy: ",
            "        forced Reach: ",
            "        forced Damaten: ",
            "        immediate Damaten tsumo: ",
        ] {
            assert!(output.contains(label), "{label}\n{output}");
            assert!(
                !output.contains(&format!("{label}unknown")),
                "{label}\n{output}"
            );
        }
    }

    #[test]
    fn explicit_north_seat_wind_enables_two_shanten_self_tsumo_selection() {
        let output = run_args(&[
            "--hand",
            "11258m234789p13s",
            "--draw",
            "9s",
            "--seat-wind",
            "N",
            "--summary-only",
        ])
        .unwrap();

        assert!(
            output.starts_with("Summary\n  choice 1: 8m\n  choice 1 source: NormalDiscard"),
            "{output}"
        );
        assert!(
            output.contains("choice 2 lost by: TwoShantenExpectedSelfTsumoValue"),
            "{output}"
        );
    }

    #[test]
    fn verbose_lookahead_adds_each_draw() {
        let summary = run_args(&["--hand", "12m12p55s", "--draw", "9p", "--lookahead"]).unwrap();
        let verbose = run_args(&[
            "--hand",
            "12m12p55s",
            "--draw",
            "9p",
            "--lookahead",
            "--verbose",
        ])
        .unwrap();

        assert!(verbose.len() > summary.len());
        assert!(verbose.contains("      next discard: "), "{verbose}");
    }

    #[test]
    fn verbose_output_is_longer() {
        let default = run_args(&["--hand", "234m455p789s1123z", "--draw", "N"]).unwrap();
        let verbose =
            run_args(&["--hand", "234m455p789s1123z", "--draw", "N", "--verbose"]).unwrap();
        assert!(verbose.len() > default.len());
    }
}
