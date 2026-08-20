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

use bot_core::{DiagnosticOptions, ShantenAgent};

use crate::cli::{CliArgs, ScenarioSource, USAGE};
use crate::error::ScenarioError;
use crate::format::{format_diagnostic, format_summary};
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
    };

    let options = if args.lookahead {
        DiagnosticOptions::WITH_LOOKAHEAD
    } else {
        DiagnosticOptions::NONE
    };
    let diagnostic =
        ShantenAgent::diagnose_with_options(&scenario.context, &scenario.legal_actions, options);

    let output = if args.summary_only {
        format_summary(&scenario, &diagnostic)
    } else {
        format_diagnostic(&scenario, &diagnostic, args.verbose)
    };
    Ok(match header {
        Some(header) => format!("{header}\n\n{output}"),
        None => output,
    })
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

    #[test]
    fn runs_a_simple_cli_scenario() {
        let output = run_args(&["--hand", "234m455p789s1123z", "--draw", "N"]).unwrap();
        assert!(output.starts_with("Scenario\n"), "{output}");
        assert!(output.contains("\n\nFinal decision\n"), "{output}");
        assert!(output.contains("\n\nNormal discard candidates"), "{output}");
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
        let line = format!(
            r#"{{"type":"request_action","request_id":425,"possible_actions":[{{"type":"dahai","pai":"1m","tsumogiri":false}},{{"type":"dahai","pai":"6p","tsumogiri":true}}],"observation":"{observation}"}}"#
        );
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
        let line = format!(
            r#"{{"type":"request_action","request_id":425,"possible_actions":[{{"type":"dahai","pai":"1m","tsumogiri":false}},{{"type":"dahai","pai":"6p","tsumogiri":true}}],"observation":"{observation}"}}"#
        );
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
