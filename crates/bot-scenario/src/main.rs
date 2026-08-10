mod cli;
mod error;
mod format;
mod input;
mod scenario;
mod tiles;

use std::process::ExitCode;

use bot_core::ShantenAgent;

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
    let diagnostic = ShantenAgent::diagnose(&scenario.context, &scenario.legal_actions);

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
    fn verbose_output_is_longer() {
        let default = run_args(&["--hand", "234m455p789s1123z", "--draw", "N"]).unwrap();
        let verbose =
            run_args(&["--hand", "234m455p789s1123z", "--draw", "N", "--verbose"]).unwrap();
        assert!(verbose.len() > default.len());
    }
}
