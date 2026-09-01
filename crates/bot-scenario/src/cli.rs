use thiserror::Error;

use crate::scenario::{HistoryFuritenSpec, ScenarioSpec};

pub const USAGE: &str = "usage:
  bot-scenario --hand <TILES> [--draw <TILE>] [--dora-indicator <TILES>] [--round-wind <WIND>]
               [--seat-wind <WIND>] [--player-id <0..3>] [--oya <0..3>]
               [--extra-visible-tiles <TILES>] [--no-history-furiten] [--allow-hora]
               [--allow-ryukyoku] [--lookahead] [--verbose] [--summary-only]
  bot-scenario <SCENARIO_JSON> [--lookahead] [--verbose] [--summary-only]
  bot-scenario --riichilab-capture <CAPTURE_JSONL> [--request-id <ID>] [--lookahead] [--verbose]
               [--summary-only]
  bot-scenario --benchmark-riichilab-capture <CAPTURE_JSONL>... [--benchmark-json <PATH>]

  --dora is a backward-compatible alias of --dora-indicator
  --extra-visible-tiles adds visible tiles that no other option expresses
  inline --hand defaults to round wind E, player 0, dealer 1, and no history furiten;
  explicit inline options override these defaults
  --no-history-furiten explicitly declares both same-turn and post-riichi missed-win furiten false
  --summary-only prints the Summary section only, and cannot be combined with
  --lookahead or --verbose
  --benchmark-riichilab-capture replays every captured request_action and measures the
  production agent decision only; it takes all following capture paths and cannot be
  combined with the other scenario or diagnostic options";

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

    #[error("--riichilab-capture cannot be combined with {0}")]
    ConflictingCaptureInput(String),

    #[error("--request-id requires --riichilab-capture")]
    RequestIdWithoutCapture,

    #[error("--request-id must be a number, but is {0:?}")]
    InvalidRequestId(String),

    #[error("{option} must be a number, but is {value:?}")]
    InvalidSeatValue { option: String, value: String },

    #[error("--dora-indicator cannot be combined with its alias --dora")]
    ConflictingDoraIndicator,

    #[error("--summary-only cannot be combined with {0}")]
    ConflictingSummaryOnly(String),

    #[error("--benchmark-riichilab-capture cannot be combined with {0}")]
    ConflictingBenchmarkInput(String),

    #[error("--benchmark-json requires --benchmark-riichilab-capture")]
    BenchmarkJsonWithoutBenchmark,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScenarioSource {
    Json(String),
    Inline(Box<ScenarioSpec>),
    RiichilabCapture {
        path: String,
        request_id: Option<u64>,
    },
    RiichilabCaptureBenchmark(CaptureBenchmarkSpec),
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CaptureBenchmarkSpec {
    pub paths: Vec<String>,
    pub json_path: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CliArgs {
    pub source: ScenarioSource,
    pub verbose: bool,
    /// 2手先診断を構築して表示するかどうか。既存の打牌診断より重い探索なので既定では行わない。
    pub lookahead: bool,
    /// Summary だけを表示するかどうか。判断は同じで、表示する section だけが変わる。
    pub summary_only: bool,
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
        let mut summary_only = false;
        let mut capture: Option<String> = None;
        let mut request_id: Option<u64> = None;
        let mut benchmark_captures: Vec<String> = Vec::new();
        let mut benchmark_json: Option<String> = None;
        let mut dora_indicator = false;
        let mut dora_alias = false;

        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--hand" => hand = Some(value_of(&mut args, "--hand")?),
                "--draw" => {
                    spec.draw = Some(value_of(&mut args, "--draw")?);
                    inline_options = true;
                }
                "--dora-indicator" => {
                    if dora_alias {
                        return Err(CliError::ConflictingDoraIndicator);
                    }
                    dora_indicator = true;
                    spec.dora_indicators = Some(value_of(&mut args, "--dora-indicator")?);
                    inline_options = true;
                }
                "--dora" => {
                    if dora_indicator {
                        return Err(CliError::ConflictingDoraIndicator);
                    }
                    dora_alias = true;
                    spec.dora_indicators = Some(value_of(&mut args, "--dora")?);
                    inline_options = true;
                }
                "--extra-visible-tiles" => {
                    spec.extra_visible_tiles = Some(value_of(&mut args, "--extra-visible-tiles")?);
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
                "--player-id" => {
                    spec.player_id = Some(seat_value_of(&mut args, "--player-id")?);
                    inline_options = true;
                }
                "--oya" => {
                    spec.oya = Some(seat_value_of(&mut args, "--oya")?);
                    inline_options = true;
                }
                "--no-history-furiten" => {
                    spec.history_furiten = Some(HistoryFuritenSpec {
                        same_turn: Some(false),
                        riichi_missed_win: Some(false),
                    });
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
                "--riichilab-capture" => {
                    capture = Some(value_of(&mut args, "--riichilab-capture")?);
                }
                "--benchmark-riichilab-capture" => {
                    benchmark_captures.push(value_of(&mut args, "--benchmark-riichilab-capture")?);
                }
                "--benchmark-json" => {
                    benchmark_json = Some(value_of(&mut args, "--benchmark-json")?);
                }
                "--request-id" => {
                    let value = value_of(&mut args, "--request-id")?;
                    request_id = Some(
                        value
                            .parse::<u64>()
                            .map_err(|_| CliError::InvalidRequestId(value))?,
                    );
                }
                "--lookahead" => lookahead = true,
                "--verbose" => verbose = true,
                "--summary-only" => summary_only = true,
                other if other.starts_with('-') => {
                    return Err(CliError::UnknownOption(other.to_string()));
                }
                other if !benchmark_captures.is_empty() => {
                    benchmark_captures.push(other.to_string());
                }
                other => match path {
                    Some(_) => return Err(CliError::MultipleScenarioFiles(other.to_string())),
                    None => path = Some(other.to_string()),
                },
            }
        }

        if !benchmark_captures.is_empty() {
            let conflict = if capture.is_some() {
                Some("--riichilab-capture".to_string())
            } else if let Some(path) = path.as_deref() {
                Some(format!("{path:?}"))
            } else if hand.is_some() {
                Some("--hand".to_string())
            } else if inline_options {
                Some("scenario options".to_string())
            } else if request_id.is_some() {
                Some("--request-id".to_string())
            } else if lookahead {
                Some("--lookahead".to_string())
            } else if verbose {
                Some("--verbose".to_string())
            } else if summary_only {
                Some("--summary-only".to_string())
            } else {
                None
            };
            if let Some(conflict) = conflict {
                return Err(CliError::ConflictingBenchmarkInput(conflict));
            }

            return Ok(Self {
                source: ScenarioSource::RiichilabCaptureBenchmark(CaptureBenchmarkSpec {
                    paths: benchmark_captures,
                    json_path: benchmark_json,
                }),
                verbose: false,
                lookahead: false,
                summary_only: false,
            });
        }

        if benchmark_json.is_some() {
            return Err(CliError::BenchmarkJsonWithoutBenchmark);
        }

        if summary_only {
            if lookahead {
                return Err(CliError::ConflictingSummaryOnly("--lookahead".to_string()));
            }
            if verbose {
                return Err(CliError::ConflictingSummaryOnly("--verbose".to_string()));
            }
        }

        let source = match (capture, path, hand) {
            (Some(_), Some(path), _) => return Err(CliError::ConflictingCaptureInput(path)),
            (Some(_), None, Some(_)) => {
                return Err(CliError::ConflictingCaptureInput("--hand".to_string()));
            }
            (Some(_), None, None) if inline_options => {
                return Err(CliError::ConflictingCaptureInput(
                    "scenario options".to_string(),
                ));
            }
            (Some(capture), None, None) => ScenarioSource::RiichilabCapture {
                path: capture,
                request_id,
            },
            _ if request_id.is_some() => return Err(CliError::RequestIdWithoutCapture),
            (None, Some(path), None) if !inline_options => ScenarioSource::Json(path),
            (None, Some(path), _) => return Err(CliError::ConflictingInput(path)),
            (None, None, Some(hand)) => {
                spec.hand = hand;
                apply_inline_baseline(&mut spec);
                ScenarioSource::Inline(Box::new(spec))
            }
            (None, None, None) => return Err(CliError::MissingHand),
        };

        Ok(Self {
            source,
            verbose,
            lookahead,
            summary_only,
        })
    }
}

// 簡易「何切る」用の deterministic baseline。ScenarioSpec 一般の default にはせず、inline
// `--hand` source の構築時だけ未指定 field を補う。明示 CLI option は `get_or_insert*` により
// 必ず優先される。
fn apply_inline_baseline(spec: &mut ScenarioSpec) {
    spec.round_wind.get_or_insert_with(|| "E".to_string());
    if spec.seat_wind.is_none() {
        spec.player_id.get_or_insert(0);
        spec.oya.get_or_insert(1);
    } else if spec.player_id.is_none() && spec.oya.is_none() {
        // 明示 seat wind と baseline identity からの導出値を競合させない。自分の河を特定する
        // player_id だけは補い、oya は明示 seat wind を source of truth にするため unknown に保つ。
        spec.player_id = Some(0);
    }
    spec.history_furiten.get_or_insert(HistoryFuritenSpec {
        same_turn: Some(false),
        riichi_missed_win: Some(false),
    });
}

fn value_of<I>(args: &mut I, option: &str) -> Result<String, CliError>
where
    I: Iterator<Item = String>,
{
    args.next()
        .ok_or_else(|| CliError::MissingValue(option.to_string()))
}

// CLI は数値化だけを担当する。0..=3 の範囲検証と seat wind の整合性検証は、JSON scenario と
// 同じ `Scenario::resolve()` の canonical path に任せる。
fn seat_value_of<I>(args: &mut I, option: &str) -> Result<u8, CliError>
where
    I: Iterator<Item = String>,
{
    let value = value_of(args, option)?;
    value.parse::<u8>().map_err(|_| CliError::InvalidSeatValue {
        option: option.to_string(),
        value,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scenario::Scenario;
    use bot_core::ShantenAgent;

    fn parse(args: &[&str]) -> Result<CliArgs, CliError> {
        CliArgs::parse(args.iter().map(|arg| arg.to_string()))
    }

    fn inline_spec(args: &[&str]) -> ScenarioSpec {
        match parse(args).unwrap().source {
            ScenarioSource::Inline(spec) => *spec,
            other => panic!("expected an inline scenario, got {other:?}"),
        }
    }

    fn inline_scenario(args: &[&str]) -> Scenario {
        Scenario::resolve(&inline_spec(args)).unwrap()
    }

    fn acceptance_remaining(scenario: &Scenario, discard: &str) -> u8 {
        ShantenAgent::diagnose(&scenario.context, &scenario.legal_actions)
            .normal_discard
            .expect("normal discard evaluated")
            .candidates
            .iter()
            .find(|candidate| candidate.evaluation.discard.to_mjai_string() == discard)
            .expect("discard candidate")
            .evaluation
            .acceptance_total_remaining()
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
            "--dora-indicator",
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
    fn inline_hand_applies_the_deterministic_baseline() {
        let spec = inline_spec(&["--hand", "123m"]);
        assert_eq!(spec.round_wind, Some("E".to_string()));
        assert_eq!(spec.player_id, Some(0));
        assert_eq!(spec.oya, Some(1));
        assert_eq!(
            spec.history_furiten,
            Some(HistoryFuritenSpec {
                same_turn: Some(false),
                riichi_missed_win: Some(false),
            })
        );

        let scenario = Scenario::resolve(&spec).unwrap();
        assert_eq!(scenario.context.round_wind().unwrap().to_mjai_string(), "E");
        assert_eq!(scenario.context.player_id(), Some(0));
        assert_eq!(scenario.context.oya(), Some(1));
        assert_eq!(scenario.context.seat_wind().unwrap().to_mjai_string(), "N");
        assert_eq!(scenario.context.history_furiten().same_turn, Some(false));
        assert_eq!(
            scenario.context.history_furiten().riichi_missed_win,
            Some(false)
        );
    }

    #[test]
    fn parses_extra_visible_tiles() {
        let spec = inline_spec(&["--hand", "123m", "--extra-visible-tiles", "11p 44p"]);
        assert_eq!(spec.extra_visible_tiles, Some("11p 44p".to_string()));

        let scenario = Scenario::resolve(&spec).unwrap();
        let visible: Vec<String> = scenario
            .context
            .visible_tiles()
            .iter()
            .map(|tile| tile.to_mjai_string())
            .collect();
        assert_eq!(visible, ["1m", "2m", "3m", "1p", "1p", "4p", "4p"]);
        assert_eq!(scenario.context.hand_tiles().len(), 3);
    }

    #[test]
    fn extra_visible_tiles_reduce_the_acceptance_remaining() {
        let hand = "34599m235p345567s";
        let baseline = inline_scenario(&["--hand", hand]);
        let with_extra = inline_scenario(&["--hand", hand, "--extra-visible-tiles", "11p 44p"]);

        assert_eq!(acceptance_remaining(&baseline, "3m"), 15);
        assert_eq!(acceptance_remaining(&with_extra, "3m"), 11);
    }

    #[test]
    fn rejects_extra_visible_tiles_with_other_input_modes() {
        assert_eq!(
            parse(&["scenario.json", "--extra-visible-tiles", "444p"]),
            Err(CliError::ConflictingInput("scenario.json".to_string()))
        );
        assert_eq!(
            parse(&[
                "--riichilab-capture",
                "capture.jsonl",
                "--extra-visible-tiles",
                "444p"
            ]),
            Err(CliError::ConflictingCaptureInput(
                "scenario options".to_string()
            ))
        );
        assert_eq!(
            parse(&[
                "--benchmark-riichilab-capture",
                "capture.jsonl",
                "--extra-visible-tiles",
                "444p"
            ]),
            Err(CliError::ConflictingBenchmarkInput(
                "scenario options".to_string()
            ))
        );
    }

    #[test]
    fn rejects_extra_visible_tiles_without_a_value() {
        assert_eq!(
            parse(&["--hand", "123m", "--extra-visible-tiles"]),
            Err(CliError::MissingValue("--extra-visible-tiles".to_string()))
        );
    }

    #[test]
    fn explicit_inline_options_override_the_baseline() {
        let spec = inline_spec(&[
            "--hand",
            "123m",
            "--round-wind",
            "S",
            "--player-id",
            "2",
            "--oya",
            "3",
        ]);
        assert_eq!(spec.round_wind, Some("S".to_string()));
        assert_eq!(spec.player_id, Some(2));
        assert_eq!(spec.oya, Some(3));

        let scenario = Scenario::resolve(&spec).unwrap();
        assert_eq!(scenario.context.round_wind().unwrap().to_mjai_string(), "S");
        assert_eq!(scenario.context.player_id(), Some(2));
        assert_eq!(scenario.context.oya(), Some(3));
        assert_eq!(scenario.context.seat_wind().unwrap().to_mjai_string(), "N");
    }

    #[test]
    fn explicit_seat_wind_is_not_overridden_by_the_identity_baseline() {
        let spec = inline_spec(&["--hand", "123m", "--seat-wind", "E"]);
        assert_eq!(spec.seat_wind, Some("E".to_string()));
        assert_eq!(spec.player_id, Some(0));
        assert_eq!(spec.oya, None);

        let scenario = Scenario::resolve(&spec).unwrap();
        assert_eq!(scenario.context.seat_wind().unwrap().to_mjai_string(), "E");
        assert_eq!(scenario.context.player_id(), Some(0));
        assert_eq!(scenario.context.oya(), None);
    }

    #[test]
    fn parses_player_id_boundaries() {
        for value in [0, 3] {
            let value = value.to_string();
            let spec = inline_spec(&["--hand", "123m", "--player-id", &value]);
            assert_eq!(spec.player_id, value.parse().ok());
            Scenario::resolve(&spec).unwrap();
        }
    }

    #[test]
    fn parses_each_oya_seat() {
        for value in 0..=3 {
            let value = value.to_string();
            let spec = inline_spec(&["--hand", "123m", "--oya", &value]);
            assert_eq!(spec.oya, value.parse().ok());
            Scenario::resolve(&spec).unwrap();
        }
    }

    #[test]
    fn rejects_non_numeric_seats_as_cli_usage_errors() {
        for option in ["--player-id", "--oya"] {
            assert_eq!(
                parse(&["--hand", "123m", option, "foo"]),
                Err(CliError::InvalidSeatValue {
                    option: option.to_string(),
                    value: "foo".to_string(),
                })
            );
        }
    }

    #[test]
    fn leaves_numeric_seat_range_validation_to_scenario_resolution() {
        for (option, field) in [("--player-id", "player_id"), ("--oya", "oya")] {
            let spec = inline_spec(&["--hand", "123m", option, "4"]);
            assert_eq!(
                Scenario::resolve(&spec),
                Err(crate::error::ScenarioError::SeatOutOfRange {
                    field: field.to_string(),
                    value: 4,
                })
            );
        }
    }

    #[test]
    fn no_history_furiten_is_an_explicit_shorthand_for_the_inline_baseline() {
        let unspecified = inline_spec(&["--hand", "123m"]);
        assert_eq!(
            unspecified.history_furiten,
            Some(HistoryFuritenSpec {
                same_turn: Some(false),
                riichi_missed_win: Some(false),
            })
        );
        let unspecified_facts = Scenario::resolve(&unspecified)
            .unwrap()
            .context
            .history_furiten();
        assert_eq!(unspecified_facts.same_turn, Some(false));
        assert_eq!(unspecified_facts.riichi_missed_win, Some(false));

        let specified = inline_spec(&["--hand", "123m", "--no-history-furiten"]);
        assert_eq!(
            specified.history_furiten,
            Some(HistoryFuritenSpec {
                same_turn: Some(false),
                riichi_missed_win: Some(false),
            })
        );
        let specified_facts = Scenario::resolve(&specified)
            .unwrap()
            .context
            .history_furiten();
        assert_eq!(specified_facts.same_turn, Some(false));
        assert_eq!(specified_facts.riichi_missed_win, Some(false));
        assert_eq!(specified.history_furiten, unspecified.history_furiten);
    }

    #[test]
    fn dora_is_a_backward_compatible_alias_of_dora_indicator() {
        let alias = inline_spec(&["--hand", "123m", "--dora", "3p E"]);
        assert_eq!(alias.dora_indicators, Some("3p E".to_string()));
        assert_eq!(
            alias,
            inline_spec(&["--hand", "123m", "--dora-indicator", "3p E"])
        );
    }

    #[test]
    fn rejects_dora_indicator_with_its_alias() {
        assert_eq!(
            parse(&["--hand", "123m", "--dora-indicator", "3p", "--dora", "E"]),
            Err(CliError::ConflictingDoraIndicator)
        );
        assert_eq!(
            parse(&["--hand", "123m", "--dora", "3p", "--dora-indicator", "E"]),
            Err(CliError::ConflictingDoraIndicator)
        );
    }

    #[test]
    fn parses_summary_only_flag() {
        assert!(!parse(&["--hand", "123m"]).unwrap().summary_only);
        assert!(
            parse(&["--hand", "123m", "--summary-only"])
                .unwrap()
                .summary_only
        );
        assert!(
            parse(&["scenario.json", "--summary-only"])
                .unwrap()
                .summary_only
        );
    }

    #[test]
    fn rejects_summary_only_with_lookahead_or_verbose() {
        assert_eq!(
            parse(&["--hand", "123m", "--summary-only", "--lookahead"]),
            Err(CliError::ConflictingSummaryOnly("--lookahead".to_string()))
        );
        assert_eq!(
            parse(&["--hand", "123m", "--lookahead", "--summary-only"]),
            Err(CliError::ConflictingSummaryOnly("--lookahead".to_string()))
        );
        assert_eq!(
            parse(&["--hand", "123m", "--summary-only", "--verbose"]),
            Err(CliError::ConflictingSummaryOnly("--verbose".to_string()))
        );
    }

    #[test]
    fn parses_allow_flags() {
        let spec = inline_spec(&["--hand", "123m", "--allow-hora", "--allow-ryukyoku"]);
        assert!(spec.allow_hora);
        assert!(spec.allow_ryukyoku);
    }

    #[test]
    fn allow_flags_default_to_disabled() {
        let spec = inline_spec(&["--hand", "123m"]);
        assert!(!spec.allow_hora);
        assert!(!spec.allow_ryukyoku);
        assert_eq!(spec.draw, None);
        assert_eq!(spec.dora_indicators, None);
    }

    #[test]
    fn rejects_the_removed_allow_reach_option() {
        assert_eq!(
            parse(&["--hand", "123m", "--allow-reach"]),
            Err(CliError::UnknownOption("--allow-reach".to_string()))
        );
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
    fn parses_riichilab_capture() {
        let args = parse(&["--riichilab-capture", "logs/ranked-capture.jsonl"]).unwrap();
        assert_eq!(
            args.source,
            ScenarioSource::RiichilabCapture {
                path: "logs/ranked-capture.jsonl".to_string(),
                request_id: None,
            }
        );
        assert!(!args.verbose);
        assert!(!args.lookahead);
    }

    #[test]
    fn parses_riichilab_capture_with_request_id_and_flags() {
        let args = parse(&[
            "--riichilab-capture",
            "logs/ranked-capture.jsonl",
            "--request-id",
            "425",
            "--lookahead",
            "--verbose",
        ])
        .unwrap();
        assert_eq!(
            args.source,
            ScenarioSource::RiichilabCapture {
                path: "logs/ranked-capture.jsonl".to_string(),
                request_id: Some(425),
            }
        );
        assert!(args.verbose);
        assert!(args.lookahead);
    }

    #[test]
    fn rejects_riichilab_capture_with_other_scenario_input() {
        assert_eq!(
            parse(&["--riichilab-capture", "capture.jsonl", "--hand", "123m"]),
            Err(CliError::ConflictingCaptureInput("--hand".to_string()))
        );
        assert_eq!(
            parse(&["--riichilab-capture", "capture.jsonl", "scenario.json"]),
            Err(CliError::ConflictingCaptureInput(
                "scenario.json".to_string()
            ))
        );
        assert_eq!(
            parse(&["--riichilab-capture", "capture.jsonl", "--draw", "N"]),
            Err(CliError::ConflictingCaptureInput(
                "scenario options".to_string()
            ))
        );
    }

    #[test]
    fn rejects_request_id_without_capture() {
        assert_eq!(
            parse(&["scenario.json", "--request-id", "1"]),
            Err(CliError::RequestIdWithoutCapture)
        );
        assert_eq!(
            parse(&["--hand", "123m", "--request-id", "1"]),
            Err(CliError::RequestIdWithoutCapture)
        );
    }

    #[test]
    fn rejects_invalid_request_id() {
        assert_eq!(
            parse(&["--riichilab-capture", "capture.jsonl", "--request-id", "x"]),
            Err(CliError::InvalidRequestId("x".to_string()))
        );
        assert_eq!(
            parse(&["--riichilab-capture", "capture.jsonl", "--request-id"]),
            Err(CliError::MissingValue("--request-id".to_string()))
        );
        assert_eq!(
            parse(&["--riichilab-capture"]),
            Err(CliError::MissingValue("--riichilab-capture".to_string()))
        );
    }

    fn benchmark_spec(args: &[&str]) -> CaptureBenchmarkSpec {
        match parse(args).unwrap().source {
            ScenarioSource::RiichilabCaptureBenchmark(spec) => spec,
            other => panic!("expected a capture benchmark, got {other:?}"),
        }
    }

    #[test]
    fn parses_a_benchmark_capture() {
        let args = parse(&["--benchmark-riichilab-capture", "logs/game-001.jsonl"]).unwrap();

        assert_eq!(
            args.source,
            ScenarioSource::RiichilabCaptureBenchmark(CaptureBenchmarkSpec {
                paths: vec!["logs/game-001.jsonl".to_string()],
                json_path: None,
            })
        );
        assert!(!args.verbose);
        assert!(!args.lookahead);
        assert!(!args.summary_only);
    }

    #[test]
    fn parses_glob_expanded_benchmark_captures() {
        let spec = benchmark_spec(&[
            "--benchmark-riichilab-capture",
            "logs/game-001.jsonl",
            "logs/game-002.jsonl",
            "logs/game-003.jsonl",
        ]);

        assert_eq!(
            spec.paths,
            vec![
                "logs/game-001.jsonl".to_string(),
                "logs/game-002.jsonl".to_string(),
                "logs/game-003.jsonl".to_string(),
            ]
        );
    }

    #[test]
    fn parses_repeated_benchmark_capture_options() {
        let spec = benchmark_spec(&[
            "--benchmark-riichilab-capture",
            "logs/game-001.jsonl",
            "--benchmark-riichilab-capture",
            "logs/game-002.jsonl",
        ]);

        assert_eq!(
            spec.paths,
            vec![
                "logs/game-001.jsonl".to_string(),
                "logs/game-002.jsonl".to_string(),
            ]
        );
    }

    #[test]
    fn parses_benchmark_json_output() {
        let spec = benchmark_spec(&[
            "--benchmark-riichilab-capture",
            "logs/game-001.jsonl",
            "--benchmark-json",
            "logs/benchmark.json",
        ]);

        assert_eq!(spec.paths, vec!["logs/game-001.jsonl".to_string()]);
        assert_eq!(spec.json_path, Some("logs/benchmark.json".to_string()));
    }

    #[test]
    fn rejects_benchmark_capture_with_other_scenario_or_diagnostic_options() {
        for (args, conflict) in [
            (
                vec![
                    "--benchmark-riichilab-capture",
                    "capture.jsonl",
                    "--riichilab-capture",
                    "capture.jsonl",
                ],
                "--riichilab-capture",
            ),
            (
                vec![
                    "--benchmark-riichilab-capture",
                    "capture.jsonl",
                    "--hand",
                    "123m",
                ],
                "--hand",
            ),
            (
                vec![
                    "--benchmark-riichilab-capture",
                    "capture.jsonl",
                    "--draw",
                    "N",
                ],
                "scenario options",
            ),
            (
                vec![
                    "--benchmark-riichilab-capture",
                    "capture.jsonl",
                    "--request-id",
                    "425",
                ],
                "--request-id",
            ),
            (
                vec![
                    "--benchmark-riichilab-capture",
                    "capture.jsonl",
                    "--lookahead",
                ],
                "--lookahead",
            ),
            (
                vec![
                    "--benchmark-riichilab-capture",
                    "capture.jsonl",
                    "--verbose",
                ],
                "--verbose",
            ),
            (
                vec![
                    "--benchmark-riichilab-capture",
                    "capture.jsonl",
                    "--summary-only",
                ],
                "--summary-only",
            ),
        ] {
            assert_eq!(
                parse(&args),
                Err(CliError::ConflictingBenchmarkInput(conflict.to_string())),
                "{args:?}"
            );
        }
    }

    #[test]
    fn rejects_a_scenario_file_before_the_benchmark_capture() {
        assert_eq!(
            parse(&[
                "scenario.json",
                "--benchmark-riichilab-capture",
                "capture.jsonl"
            ]),
            Err(CliError::ConflictingBenchmarkInput(
                "\"scenario.json\"".to_string()
            ))
        );
    }

    #[test]
    fn rejects_benchmark_json_without_a_benchmark_capture() {
        assert_eq!(
            parse(&["--hand", "123m", "--benchmark-json", "benchmark.json"]),
            Err(CliError::BenchmarkJsonWithoutBenchmark)
        );
    }

    #[test]
    fn rejects_missing_benchmark_option_values() {
        assert_eq!(
            parse(&["--benchmark-riichilab-capture"]),
            Err(CliError::MissingValue(
                "--benchmark-riichilab-capture".to_string()
            ))
        );
        assert_eq!(
            parse(&[
                "--benchmark-riichilab-capture",
                "capture.jsonl",
                "--benchmark-json"
            ]),
            Err(CliError::MissingValue("--benchmark-json".to_string()))
        );
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
            parse(&["--hand", "123m", "--dora-indicator"]),
            Err(CliError::MissingValue("--dora-indicator".to_string()))
        );
        assert_eq!(
            parse(&["--hand", "123m", "--dora"]),
            Err(CliError::MissingValue("--dora".to_string()))
        );
        for option in ["--player-id", "--oya"] {
            assert_eq!(
                parse(&["--hand", "123m", option]),
                Err(CliError::MissingValue(option.to_string()))
            );
        }
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
