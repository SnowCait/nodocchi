use thiserror::Error;

use crate::scenario::ScenarioSpec;

pub const USAGE: &str = "usage:
  bot-scenario --hand <TILES> [--draw <TILE>] [--dora <TILES>] [--round-wind <WIND>]
               [--seat-wind <WIND>] [--allow-reach] [--allow-hora] [--allow-ryukyoku]
               [--lookahead] [--verbose]
  bot-scenario <SCENARIO_JSON> [--lookahead] [--verbose]";

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum CliError {
    #[error("unknown option: {0}")]
    UnknownOption(String),

    #[error("{0} requires a value")]
    MissingValue(String),

    #[error("--hand is required")]
    MissingHand,

    #[error("scenario file {0:?} cannot be combined with hand options")]
    ConflictingInput(String),

    #[error("multiple scenario files: {0:?}")]
    MultipleScenarioFiles(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScenarioSource {
    Json(String),
    Inline(Box<ScenarioSpec>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CliArgs {
    pub source: ScenarioSource,
    pub verbose: bool,
    /// 2手先診断を構築して表示するかどうか。既存の打牌診断より重い探索なので既定では行わない。
    pub lookahead: bool,
}

impl CliArgs {
    pub fn parse<I>(args: I) -> Result<Self, CliError>
    where
        I: IntoIterator<Item = String>,
    {
        let mut args = args.into_iter();
        let mut path: Option<String> = None;
        let mut spec = ScenarioSpec::default();
        let mut hand: Option<String> = None;
        let mut inline_options = false;
        let mut verbose = false;
        let mut lookahead = false;

        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--hand" => hand = Some(value_of(&mut args, "--hand")?),
                "--draw" => {
                    spec.draw = Some(value_of(&mut args, "--draw")?);
                    inline_options = true;
                }
                "--dora" => {
                    spec.dora_indicators = Some(value_of(&mut args, "--dora")?);
                    inline_options = true;
                }
                "--round-wind" => {
                    spec.round_wind = Some(value_of(&mut args, "--round-wind")?);
                    inline_options = true;
                }
                "--seat-wind" => {
                    spec.seat_wind = Some(value_of(&mut args, "--seat-wind")?);
                    inline_options = true;
                }
                "--allow-reach" => {
                    spec.allow_reach = true;
                    inline_options = true;
                }
                "--allow-hora" => {
                    spec.allow_hora = true;
                    inline_options = true;
                }
                "--allow-ryukyoku" => {
                    spec.allow_ryukyoku = true;
                    inline_options = true;
                }
                "--lookahead" => lookahead = true,
                "--verbose" => verbose = true,
                other if other.starts_with('-') => {
                    return Err(CliError::UnknownOption(other.to_string()));
                }
                other => match path {
                    Some(_) => return Err(CliError::MultipleScenarioFiles(other.to_string())),
                    None => path = Some(other.to_string()),
                },
            }
        }

        let source = match (path, hand) {
            (Some(path), None) if !inline_options => ScenarioSource::Json(path),
            (Some(path), _) => return Err(CliError::ConflictingInput(path)),
            (None, Some(hand)) => {
                spec.hand = hand;
                ScenarioSource::Inline(Box::new(spec))
            }
            (None, None) => return Err(CliError::MissingHand),
        };

        Ok(Self {
            source,
            verbose,
            lookahead,
        })
    }
}

fn value_of<I>(args: &mut I, option: &str) -> Result<String, CliError>
where
    I: Iterator<Item = String>,
{
    args.next()
        .ok_or_else(|| CliError::MissingValue(option.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scenario::Scenario;

    fn parse(args: &[&str]) -> Result<CliArgs, CliError> {
        CliArgs::parse(args.iter().map(|arg| arg.to_string()))
    }

    fn inline_spec(args: &[&str]) -> ScenarioSpec {
        match parse(args).unwrap().source {
            ScenarioSource::Inline(spec) => *spec,
            other => panic!("expected an inline scenario, got {other:?}"),
        }
    }

    #[test]
    fn parses_hand_and_draw() {
        let spec = inline_spec(&["--hand", "234m455p789s1123z", "--draw", "N"]);
        assert_eq!(spec.hand, "234m455p789s1123z");
        assert_eq!(spec.draw, Some("N".to_string()));
    }

    #[test]
    fn builds_a_scenario_from_hand_and_draw() {
        let spec = inline_spec(&["--hand", "234m455p789s1123z", "--draw", "N"]);
        let scenario = Scenario::resolve(&spec).unwrap();
        assert_eq!(scenario.context.hand_tiles().len(), 13);
        assert_eq!(
            scenario
                .context
                .drawn_tile()
                .map(|tile| tile.to_mjai_string()),
            Some("N".to_string())
        );
        assert_eq!(scenario.legal_actions.len(), 12);
    }

    #[test]
    fn parses_table_options() {
        let spec = inline_spec(&[
            "--hand",
            "123m",
            "--dora",
            "3p E",
            "--round-wind",
            "E",
            "--seat-wind",
            "S",
        ]);
        assert_eq!(spec.dora_indicators, Some("3p E".to_string()));
        assert_eq!(spec.round_wind, Some("E".to_string()));
        assert_eq!(spec.seat_wind, Some("S".to_string()));
    }

    #[test]
    fn parses_allow_flags() {
        let spec = inline_spec(&[
            "--hand",
            "123m",
            "--allow-reach",
            "--allow-hora",
            "--allow-ryukyoku",
        ]);
        assert!(spec.allow_reach);
        assert!(spec.allow_hora);
        assert!(spec.allow_ryukyoku);
    }

    #[test]
    fn allow_flags_default_to_disabled() {
        let spec = inline_spec(&["--hand", "123m"]);
        assert!(!spec.allow_reach);
        assert!(!spec.allow_hora);
        assert!(!spec.allow_ryukyoku);
        assert_eq!(spec.draw, None);
        assert_eq!(spec.dora_indicators, None);
    }

    #[test]
    fn parses_verbose_flag() {
        assert!(!parse(&["--hand", "123m"]).unwrap().verbose);
        assert!(parse(&["--hand", "123m", "--verbose"]).unwrap().verbose);
    }

    #[test]
    fn parses_lookahead_flag() {
        assert!(!parse(&["--hand", "123m"]).unwrap().lookahead);
        assert!(parse(&["--hand", "123m", "--lookahead"]).unwrap().lookahead);
        assert!(parse(&["scenario.json", "--lookahead"]).unwrap().lookahead);
    }

    #[test]
    fn parses_scenario_file() {
        let args = parse(&["scenario.json"]).unwrap();
        assert_eq!(
            args.source,
            ScenarioSource::Json("scenario.json".to_string())
        );
        assert!(!args.verbose);
    }

    #[test]
    fn parses_scenario_file_with_verbose() {
        let args = parse(&["scenario.json", "--verbose"]).unwrap();
        assert_eq!(
            args.source,
            ScenarioSource::Json("scenario.json".to_string())
        );
        assert!(args.verbose);
    }

    #[test]
    fn rejects_missing_hand() {
        assert_eq!(parse(&[]), Err(CliError::MissingHand));
        assert_eq!(parse(&["--draw", "N"]), Err(CliError::MissingHand));
    }

    #[test]
    fn rejects_missing_option_value() {
        assert_eq!(
            parse(&["--hand"]),
            Err(CliError::MissingValue("--hand".to_string()))
        );
        assert_eq!(
            parse(&["--hand", "123m", "--dora"]),
            Err(CliError::MissingValue("--dora".to_string()))
        );
    }

    #[test]
    fn rejects_unknown_option() {
        assert_eq!(
            parse(&["--hand", "123m", "--unknown"]),
            Err(CliError::UnknownOption("--unknown".to_string()))
        );
    }

    #[test]
    fn rejects_scenario_file_with_hand_options() {
        assert_eq!(
            parse(&["scenario.json", "--hand", "123m"]),
            Err(CliError::ConflictingInput("scenario.json".to_string()))
        );
        assert_eq!(
            parse(&["scenario.json", "--draw", "N"]),
            Err(CliError::ConflictingInput("scenario.json".to_string()))
        );
    }

    #[test]
    fn rejects_multiple_scenario_files() {
        assert_eq!(
            parse(&["first.json", "second.json"]),
            Err(CliError::MultipleScenarioFiles("second.json".to_string()))
        );
    }
}
