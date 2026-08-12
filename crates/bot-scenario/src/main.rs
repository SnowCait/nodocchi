mod cli;
mod error;
mod format;
mod input;
mod scenario;
mod tiles;

use std::process::ExitCode;

use bot_core::{DiagnosticOptions, ShantenAgent};

use crate::cli::{CliArgs, ScenarioSource, USAGE};
use crate::error::ScenarioError;
use crate::format::format_diagnostic;
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
    let spec = match &args.source {
        ScenarioSource::Json(path) => read_spec(path)?,
        ScenarioSource::Inline(spec) => spec.as_ref().clone(),
    };

    let scenario = Scenario::resolve(&spec)?;
    let options = if args.second_ply {
        DiagnosticOptions::WITH_LOOKAHEAD
    } else {
        DiagnosticOptions::NONE
    };
    let diagnostic =
        ShantenAgent::diagnose_with_options(&scenario.context, &scenario.legal_actions, options);

    Ok(format_diagnostic(&scenario, &diagnostic, args.verbose))
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
    fn second_ply_is_opt_in() {
        // 2手先は重い探索なので既定では計算せず表示もしない。小さい手牌で確認する。
        let default = run_args(&["--hand", "12m12p55s", "--draw", "9p"]).unwrap();
        assert!(!default.contains("Second ply"), "{default}");

        let second_ply =
            run_args(&["--hand", "12m12p55s", "--draw", "9p", "--second-ply"]).unwrap();
        assert!(second_ply.contains("\n\nSecond ply\n"), "{second_ply}");
        assert!(second_ply.contains("draws: "), "{second_ply}");
        assert!(!second_ply.contains("next discard:"), "{second_ply}");
    }

    #[test]
    fn verbose_second_ply_adds_each_draw() {
        let summary = run_args(&["--hand", "12m12p55s", "--draw", "9p", "--second-ply"]).unwrap();
        let verbose = run_args(&[
            "--hand",
            "12m12p55s",
            "--draw",
            "9p",
            "--second-ply",
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
