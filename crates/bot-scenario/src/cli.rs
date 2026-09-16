use std::iter::Peekable;

use bot_core::seat_wind_for_player;
use bot_logic::{TileType, TwoShantenSelfTsumoScope};
use thiserror::Error;

use crate::scenario::{HistoryFuritenSpec, ScenarioSpec, parse_seat_wind};

pub const USAGE: &str = "usage:
  bot-scenario --hand <TILES> [--draw <TILE>] [--dora-indicator <TILES>] [--round-wind <WIND>]
               [--seat-wind <WIND>] [--player-id <0..3>] [--oya <0..3>]
               [--discards-shimocha <TILES>] [--discards-toimen <TILES>]
               [--discards-kamicha <TILES>] [--riichi-shimocha [INDEX]]
               [--riichi-toimen [INDEX]] [--riichi-kamicha [INDEX]]
               [--extra-visible-tiles <TILES>] [--remaining-tiles <COUNT>]
               [--no-history-furiten] [--allow-hora] [--force-fold]
               [--allow-ryukyoku] [--lookahead] [--two-shanten-self-tsumo] [--verbose]
               [--two-shanten-self-tsumo-cost <SCOPE>]
               [--two-shanten-progress-self-tsumo-cost <SCOPE>] [--summary-only]
  bot-scenario <SCENARIO_JSON> [--lookahead] [--two-shanten-self-tsumo] [--verbose]
               [--two-shanten-self-tsumo-cost <SCOPE>] [--force-fold]
               [--two-shanten-progress-self-tsumo-cost <SCOPE>] [--summary-only]
  bot-scenario --riichilab-capture <CAPTURE_JSONL> [--request-id <ID>] [--lookahead]
               [--two-shanten-self-tsumo] [--verbose] [--force-fold] [--summary-only]
  bot-scenario --hand <TILES> [scenario options] --three-shanten-progress-self-tsumo
  bot-scenario <SCENARIO_JSON> --three-shanten-progress-self-tsumo
  bot-scenario --riichilab-capture <CAPTURE_JSONL> [--request-id <ID>]
               --three-shanten-progress-self-tsumo
  bot-scenario --benchmark-riichilab-capture <CAPTURE_JSONL>... [--benchmark-json <PATH>]
  bot-scenario --hand <TILES> [scenario options] --three-shanten-continuation-comparison
  bot-scenario <SCENARIO_JSON> --three-shanten-continuation-comparison
  bot-scenario --riichilab-capture <CAPTURE_JSONL> [--request-id <ID>]
               --three-shanten-continuation-comparison
  bot-scenario --compare-three-shanten-continuation <CAPTURE_JSONL>...
  bot-scenario --hand <TILES> [scenario options] --iishanten-continuation-depth-comparison
  bot-scenario <SCENARIO_JSON> --iishanten-continuation-depth-comparison
  bot-scenario --riichilab-capture <CAPTURE_JSONL> [--request-id <ID>]
               --iishanten-continuation-depth-comparison
  bot-scenario --hand <TILES> [scenario options] --iishanten-selection-depth-comparison
  bot-scenario <SCENARIO_JSON> --iishanten-selection-depth-comparison
  bot-scenario --riichilab-capture <CAPTURE_JSONL> [--request-id <ID>]
               --iishanten-selection-depth-comparison
  bot-scenario --hand <TILES> [scenario options] --iishanten-selection-parallel-comparison
  bot-scenario <SCENARIO_JSON> --iishanten-selection-parallel-comparison
  bot-scenario --riichilab-capture <CAPTURE_JSONL> [--request-id <ID>]
               --iishanten-selection-parallel-comparison
  bot-scenario --hand <TILES> [scenario options] --two-shanten-full-parallel-comparison
  bot-scenario <SCENARIO_JSON> --two-shanten-full-parallel-comparison
  bot-scenario --riichilab-capture <CAPTURE_JSONL> [--request-id <ID>]
               --two-shanten-full-parallel-comparison

  --dora is a backward-compatible alias of --dora-indicator
  --discards-shimocha, --discards-toimen and --discards-kamicha set the whole river of that
  relative seat, with the same semantics as the JSON scenario discards field
  --riichi-shimocha, --riichi-toimen and --riichi-kamicha mark that relative seat as reached
  and take the optional 1-based INDEX of the reach declaration tile in its river, where 1 is
  the first discard; without INDEX the seat stays reached with an unknown declaration tile
  relative seats resolve from player_id: shimocha is (player_id + 1) % 4, toimen is
  (player_id + 2) % 4 and kamicha is (player_id + 3) % 4
  a scenario that uses any of them is not a first turn, so the inline remaining-tiles
  baseline is not applied and the wall count stays unknown unless --remaining-tiles says
  otherwise; it is never derived from the given rivers
  --extra-visible-tiles adds visible tiles that no other option expresses
  --remaining-tiles overrides the inline initial live wall count derived from player and
  dealer, or explicit seat wind, plus draw state
  inline --hand defaults to round wind E, player 0, dealer 1, and no history furiten;
  explicit inline options override these defaults
  --no-history-furiten explicitly declares both same-turn and post-riichi missed-win furiten false
  --three-shanten-progress-self-tsumo evaluates all three-shanten candidates with the same
  evaluator the production discard comparison uses and reports values and elapsed time;
  cannot be combined with other diagnostic options
  --two-shanten-self-tsumo adds the expected self-tsumo value of the two-shanten discard
  candidates; it implies --lookahead and searches deeper than the standard lookahead
  --two-shanten-self-tsumo-cost measures that same search instead of rendering it, with
  <SCOPE> all for every two-shanten candidate and forward-targets for the production
  comparison cohort only; it cannot be combined with any other diagnostic option, so that
  no deeper search warms the shanten and acceptance memos before the measurement
  --two-shanten-progress-self-tsumo-cost measures only the first Progress branch through
  the existing progress helper, with the same <SCOPE> and isolation as the full cost option
  --force-fold reports the best defensive discard assuming a fold, independently of the
  normal push/pull decision; it evaluates the existing fold defense directly instead of
  running the normal discard selection, it never changes what the production bot decides,
  and it is unavailable when there is no clear threat or no legal Dahai to choose from
  --summary-only prints the Summary section only, and cannot be combined with
  --lookahead, --two-shanten-self-tsumo or --verbose
  --benchmark-riichilab-capture replays every captured request_action and measures the
  production agent decision only; it takes all following capture paths and cannot be
  combined with the other scenario or diagnostic options
  --three-shanten-continuation-comparison evaluates the three-shanten candidates twice, once
  with the production one-shanten continuation (Progress + SameShanten) and once with a
  Progress-only one-shanten continuation, and reports both sets of values, the search size
  and the discard each one selects; production discard selection is unchanged and cannot be
  combined with other diagnostic options
  --iishanten-continuation-depth-comparison evaluates every one-shanten candidate twice,
  once with the legacy shallow continuation (Progress and SameShanten -> Progress) and once
  with the production depth that allows one extra hand-change step (SameShanten ->
  SameShanten -> Progress), and reports both sets of values, the ranking, the search size
  and the elapsed time; it ranks the axis alone instead of running the production
  comparator and it cannot be combined with other diagnostic options
  --iishanten-selection-depth-comparison runs the production discard selection with the
  legacy shallow depth (A) and with the production depth (B, one extra hand-change step
  plus the exact same-state memo), and reports the discard each one selects, the candidates
  the existing gating evaluates deeply, the comparison reasons, the search size and the
  elapsed time; each depth runs twice, a timing run with no instrumentation the elapsed
  time comes from and an observation run with the search-size counters and the phase timer
  the cohort, values and stats come from; unlike
  --iishanten-continuation-depth-comparison it measures the whole comparator instead of
  ranking the axis alone, so the A -> B elapsed difference is not the depth alone;
  production selects with B, A stays only as the comparison baseline, both depths here
  evaluate the deep candidates sequentially, and it cannot be combined with other
  diagnostic options
  --iishanten-selection-parallel-comparison runs that same production depth four times,
  once sequentially (S) and once for each candidate-level parallel mode (P2 up to 2
  workers, P4 up to 4 workers and PA up to available_parallelism), and reports the elapsed
  time, the speedup, the search size and the memo hit and miss counts of each one; the
  parallel modes split only the deeply evaluated candidates across threads and write every
  result back to its candidate index, so the cohort, the values, the axis resolution, the
  comparison reasons and the selected discard stay bit-exact; PA is the configuration
  production itself uses, S, P2 and P4 stay as the comparison baselines, and it cannot be
  combined with other diagnostic options
  --two-shanten-full-parallel-comparison runs the production discard selection on a
  two-shanten position twice, once with the dora-gated provisional top-2 evaluated in order
  (S) and once with those same two candidates evaluated on up to min(2,
  available_parallelism) workers (P2, the current production configuration), and reports the
  elapsed time, the speedup, the search size and the memo hit and miss counts of each one;
  only the execution of those two Full evaluations differs, so the Progress cohort, the
  top-2 selection, the gate, the comparison reasons, the Full values and the selected
  discard stay bit-exact; each mode runs twice, a timing run with no instrumentation the
  elapsed time comes from and an observation run with the search-size counters and the phase
  timer the values and stats come from, and it cannot be combined with other diagnostic
  options
  --compare-three-shanten-continuation replays every captured request_action, runs the same
  A/B comparison on the requests where the three-shanten axis fires, and reports latency,
  search size and selection differences; it takes all following capture paths and cannot be
  combined with the other scenario or diagnostic options";

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum CliError {
    #[error("--three-shanten-progress-self-tsumo cannot be combined with {0}")]
    ConflictingThreeShantenProgressSelfTsumo(String),
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

    #[error("{option} must be a number, but is {value:?}")]
    InvalidCount { option: String, value: String },

    #[error("--dora-indicator cannot be combined with its alias --dora")]
    ConflictingDoraIndicator,

    #[error("{0} needs a player_id to resolve the relative seat")]
    RelativeSeatWithoutPlayerId(String),

    #[error("--summary-only cannot be combined with {0}")]
    ConflictingSummaryOnly(String),

    #[error("--benchmark-riichilab-capture cannot be combined with {0}")]
    ConflictingBenchmarkInput(String),

    #[error("--benchmark-json requires --benchmark-riichilab-capture")]
    BenchmarkJsonWithoutBenchmark,

    #[error("--three-shanten-continuation-comparison cannot be combined with {0}")]
    ConflictingThreeShantenContinuationComparison(String),
    #[error("--iishanten-continuation-depth-comparison cannot be combined with {0}")]
    ConflictingIishantenContinuationDepthComparison(String),
    #[error("--two-shanten-full-parallel-comparison cannot be combined with {0}")]
    ConflictingTwoShantenFullParallelComparison(String),

    #[error("--iishanten-selection-parallel-comparison cannot be combined with {0}")]
    ConflictingIishantenSelectionParallelComparison(String),

    #[error("--iishanten-selection-depth-comparison cannot be combined with {0}")]
    ConflictingIishantenSelectionDepthComparison(String),

    #[error("--compare-three-shanten-continuation cannot be combined with {0}")]
    ConflictingCaptureComparisonInput(String),

    #[error("--two-shanten-self-tsumo-cost must be all or forward-targets, but is {0:?}")]
    InvalidTwoShantenSelfTsumoCostScope(String),

    #[error("--two-shanten-progress-self-tsumo-cost must be all or forward-targets, but is {0:?}")]
    InvalidTwoShantenProgressSelfTsumoCostScope(String),

    #[error("--two-shanten-self-tsumo-cost cannot be combined with {0}")]
    ConflictingTwoShantenSelfTsumoCost(String),

    #[error("--two-shanten-progress-self-tsumo-cost cannot be combined with {0}")]
    ConflictingTwoShantenProgressSelfTsumoCost(String),

    #[error("--force-fold cannot be combined with {0}")]
    ConflictingForceFold(String),
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
    RiichilabCaptureComparison(CaptureComparisonSpec),
}

/// A/B 比較を行う capture の指定。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CaptureComparisonSpec {
    pub paths: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CaptureBenchmarkSpec {
    pub paths: Vec<String>,
    pub json_path: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CliArgs {
    /// 全3向聴候補の Progress-only 値と時間を表示する専用診断。
    pub three_shanten_progress_self_tsumo: bool,
    /// 1向聴 continuation scope の A/B 比較を表示する専用診断。production 選択は変えない。
    pub three_shanten_continuation_comparison: bool,
    /// 1向聴 ExpectedSelfTsumoValue の手変わり深度 A/B 比較を表示する専用診断。
    /// A は旧設定、B は現行 production depth。
    pub iishanten_continuation_depth_comparison: bool,
    /// 1向聴の手変わり深度 A/B を production comparator を通した最終打牌選択として表示する
    /// 専用診断。A は旧設定、B は現行 production depth。
    pub iishanten_selection_depth_comparison: bool,
    /// production と同じ B depth の中で、深い候補評価の分け方 (S / P2 / P4 / PA) を比べる
    /// 専用診断。PA が現行 production と同じ方式。
    pub iishanten_selection_parallel_comparison: bool,
    /// 2向聴のドラ差 gate を通った provisional 上位2候補の Full 追加評価の分け方 (S / P2) を
    /// 比べる専用診断。P2 が現行 production と同じ方式。
    pub two_shanten_full_parallel_comparison: bool,
    pub source: ScenarioSource,
    pub verbose: bool,
    /// 2手先診断を構築して表示するかどうか。既存の打牌診断より重い探索なので既定では行わない。
    pub lookahead: bool,
    /// 2向聴候補の ExpectedSelfTsumoValue を構築して表示するかどうか。2手先診断よりさらに重い
    /// 探索なので既定では行わない。指定した場合は2手先診断も構築する。
    pub two_shanten_self_tsumo: bool,
    /// 2向聴候補の ExpectedSelfTsumoValue の実行コストを計測する対象の範囲。計測する場合だけ
    /// `Some`。表示するのは実測時間と値だけで、打牌選択も他の診断も変わらない。
    pub two_shanten_self_tsumo_cost: Option<TwoShantenSelfTsumoScope>,
    /// Progress 枝だけの実行コストを計測する対象の範囲。
    pub two_shanten_progress_self_tsumo_cost: Option<TwoShantenSelfTsumoScope>,
    /// Summary だけを表示するかどうか。判断は同じで、表示する section だけが変わる。
    pub summary_only: bool,
    /// 通常の押し引き判断とは無関係に、ベタ降りを仮定した場合の防御打牌を表示するかどうか。
    /// production の判断は変えず、通常打牌の診断も構築しない。
    pub force_fold: bool,
}

impl CliArgs {
    pub fn parse<I>(args: I) -> Result<Self, CliError>
    where
        I: IntoIterator<Item = String>,
    {
        let mut args = args.into_iter().peekable();
        let mut three_shanten_progress_self_tsumo = false;
        let mut three_shanten_continuation_comparison = false;
        let mut iishanten_continuation_depth_comparison = false;
        let mut iishanten_selection_depth_comparison = false;
        let mut iishanten_selection_parallel_comparison = false;
        let mut two_shanten_full_parallel_comparison = false;
        let mut comparison_captures: Vec<String> = Vec::new();
        let mut path: Option<String> = None;
        let mut spec = ScenarioSpec::default();
        let mut hand: Option<String> = None;
        let mut inline_options = false;
        let mut verbose = false;
        let mut lookahead = false;
        let mut two_shanten_self_tsumo = false;
        let mut two_shanten_self_tsumo_cost = None;
        let mut two_shanten_progress_self_tsumo_cost = None;
        let mut summary_only = false;
        let mut force_fold = false;
        let mut capture: Option<String> = None;
        let mut request_id: Option<u64> = None;
        let mut benchmark_captures: Vec<String> = Vec::new();
        let mut benchmark_json: Option<String> = None;
        let mut dora_indicator = false;
        let mut dora_alias = false;
        let mut relative_discards: [Option<String>; RELATIVE_SEAT_COUNT] = Default::default();
        let mut relative_riichi: [Option<Option<u32>>; RELATIVE_SEAT_COUNT] = Default::default();

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
                "--discards-shimocha" => {
                    relative_discards[SHIMOCHA] = Some(value_of(&mut args, "--discards-shimocha")?);
                    inline_options = true;
                }
                "--discards-toimen" => {
                    relative_discards[TOIMEN] = Some(value_of(&mut args, "--discards-toimen")?);
                    inline_options = true;
                }
                "--discards-kamicha" => {
                    relative_discards[KAMICHA] = Some(value_of(&mut args, "--discards-kamicha")?);
                    inline_options = true;
                }
                "--riichi-shimocha" => {
                    relative_riichi[SHIMOCHA] = Some(optional_index_of(&mut args));
                    inline_options = true;
                }
                "--riichi-toimen" => {
                    relative_riichi[TOIMEN] = Some(optional_index_of(&mut args));
                    inline_options = true;
                }
                "--riichi-kamicha" => {
                    relative_riichi[KAMICHA] = Some(optional_index_of(&mut args));
                    inline_options = true;
                }
                "--remaining-tiles" => {
                    spec.remaining_tiles = Some(count_value_of(&mut args, "--remaining-tiles")?);
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
                "--three-shanten-progress-self-tsumo" => three_shanten_progress_self_tsumo = true,
                "--three-shanten-continuation-comparison" => {
                    three_shanten_continuation_comparison = true;
                }
                "--iishanten-continuation-depth-comparison" => {
                    iishanten_continuation_depth_comparison = true;
                }
                "--iishanten-selection-depth-comparison" => {
                    iishanten_selection_depth_comparison = true;
                }
                "--iishanten-selection-parallel-comparison" => {
                    iishanten_selection_parallel_comparison = true;
                }
                "--two-shanten-full-parallel-comparison" => {
                    two_shanten_full_parallel_comparison = true;
                }
                "--compare-three-shanten-continuation" => {
                    comparison_captures
                        .push(value_of(&mut args, "--compare-three-shanten-continuation")?);
                }
                "--two-shanten-self-tsumo" => two_shanten_self_tsumo = true,
                "--two-shanten-self-tsumo-cost" => {
                    let value = value_of(&mut args, "--two-shanten-self-tsumo-cost")?;
                    two_shanten_self_tsumo_cost = Some(two_shanten_self_tsumo_cost_scope(&value)?);
                }
                "--two-shanten-progress-self-tsumo-cost" => {
                    let value = value_of(&mut args, "--two-shanten-progress-self-tsumo-cost")?;
                    two_shanten_progress_self_tsumo_cost =
                        Some(two_shanten_self_tsumo_cost_scope(&value).map_err(|_| {
                            CliError::InvalidTwoShantenProgressSelfTsumoCostScope(value)
                        })?);
                }
                "--verbose" => verbose = true,
                "--summary-only" => summary_only = true,
                "--force-fold" => force_fold = true,
                other if other.starts_with('-') => {
                    return Err(CliError::UnknownOption(other.to_string()));
                }
                other if !benchmark_captures.is_empty() => {
                    benchmark_captures.push(other.to_string());
                }
                other if !comparison_captures.is_empty() => {
                    comparison_captures.push(other.to_string());
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
            } else if two_shanten_self_tsumo {
                Some("--two-shanten-self-tsumo".to_string())
            } else if two_shanten_self_tsumo_cost.is_some() {
                Some("--two-shanten-self-tsumo-cost".to_string())
            } else if two_shanten_progress_self_tsumo_cost.is_some() {
                Some("--two-shanten-progress-self-tsumo-cost".to_string())
            } else if three_shanten_progress_self_tsumo {
                Some("--three-shanten-progress-self-tsumo".to_string())
            } else if three_shanten_continuation_comparison {
                Some("--three-shanten-continuation-comparison".to_string())
            } else if iishanten_continuation_depth_comparison {
                Some("--iishanten-continuation-depth-comparison".to_string())
            } else if iishanten_selection_depth_comparison {
                Some("--iishanten-selection-depth-comparison".to_string())
            } else if iishanten_selection_parallel_comparison {
                Some("--iishanten-selection-parallel-comparison".to_string())
            } else if two_shanten_full_parallel_comparison {
                Some("--two-shanten-full-parallel-comparison".to_string())
            } else if !comparison_captures.is_empty() {
                Some("--compare-three-shanten-continuation".to_string())
            } else if force_fold {
                Some("--force-fold".to_string())
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
                three_shanten_progress_self_tsumo: false,
                three_shanten_continuation_comparison: false,
                iishanten_continuation_depth_comparison: false,
                iishanten_selection_depth_comparison: false,
                iishanten_selection_parallel_comparison: false,
                two_shanten_full_parallel_comparison: false,
                lookahead: false,
                two_shanten_self_tsumo: false,
                two_shanten_self_tsumo_cost: None,
                two_shanten_progress_self_tsumo_cost: None,
                summary_only: false,
                force_fold: false,
            });
        }

        if !comparison_captures.is_empty() {
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
            } else if two_shanten_self_tsumo {
                Some("--two-shanten-self-tsumo".to_string())
            } else if two_shanten_self_tsumo_cost.is_some() {
                Some("--two-shanten-self-tsumo-cost".to_string())
            } else if two_shanten_progress_self_tsumo_cost.is_some() {
                Some("--two-shanten-progress-self-tsumo-cost".to_string())
            } else if three_shanten_progress_self_tsumo {
                Some("--three-shanten-progress-self-tsumo".to_string())
            } else if three_shanten_continuation_comparison {
                Some("--three-shanten-continuation-comparison".to_string())
            } else if iishanten_continuation_depth_comparison {
                Some("--iishanten-continuation-depth-comparison".to_string())
            } else if iishanten_selection_depth_comparison {
                Some("--iishanten-selection-depth-comparison".to_string())
            } else if iishanten_selection_parallel_comparison {
                Some("--iishanten-selection-parallel-comparison".to_string())
            } else if two_shanten_full_parallel_comparison {
                Some("--two-shanten-full-parallel-comparison".to_string())
            } else if benchmark_json.is_some() {
                Some("--benchmark-json".to_string())
            } else if force_fold {
                Some("--force-fold".to_string())
            } else if verbose {
                Some("--verbose".to_string())
            } else if summary_only {
                Some("--summary-only".to_string())
            } else {
                None
            };
            if let Some(conflict) = conflict {
                return Err(CliError::ConflictingCaptureComparisonInput(conflict));
            }

            return Ok(Self {
                source: ScenarioSource::RiichilabCaptureComparison(CaptureComparisonSpec {
                    paths: comparison_captures,
                }),
                verbose: false,
                three_shanten_progress_self_tsumo: false,
                three_shanten_continuation_comparison: false,
                iishanten_continuation_depth_comparison: false,
                iishanten_selection_depth_comparison: false,
                iishanten_selection_parallel_comparison: false,
                two_shanten_full_parallel_comparison: false,
                lookahead: false,
                two_shanten_self_tsumo: false,
                two_shanten_self_tsumo_cost: None,
                two_shanten_progress_self_tsumo_cost: None,
                summary_only: false,
                force_fold: false,
            });
        }

        if benchmark_json.is_some() {
            return Err(CliError::BenchmarkJsonWithoutBenchmark);
        }

        // forced fold は通常打牌の選択も追加診断も走らせないので、それらを要求する option とは
        // 併用しない。比較・計測専用 mode も同じく単独で実行する。
        if force_fold {
            for (enabled, option) in [
                (lookahead, "--lookahead"),
                (two_shanten_self_tsumo, "--two-shanten-self-tsumo"),
                (
                    two_shanten_self_tsumo_cost.is_some(),
                    "--two-shanten-self-tsumo-cost",
                ),
                (
                    two_shanten_progress_self_tsumo_cost.is_some(),
                    "--two-shanten-progress-self-tsumo-cost",
                ),
                (
                    three_shanten_progress_self_tsumo,
                    "--three-shanten-progress-self-tsumo",
                ),
                (
                    three_shanten_continuation_comparison,
                    "--three-shanten-continuation-comparison",
                ),
                (
                    iishanten_continuation_depth_comparison,
                    "--iishanten-continuation-depth-comparison",
                ),
                (
                    iishanten_selection_depth_comparison,
                    "--iishanten-selection-depth-comparison",
                ),
                (
                    iishanten_selection_parallel_comparison,
                    "--iishanten-selection-parallel-comparison",
                ),
                (
                    two_shanten_full_parallel_comparison,
                    "--two-shanten-full-parallel-comparison",
                ),
            ] {
                if enabled {
                    return Err(CliError::ConflictingForceFold(option.to_string()));
                }
            }
        }

        if three_shanten_continuation_comparison {
            for (enabled, option) in [
                (lookahead, "--lookahead"),
                (verbose, "--verbose"),
                (summary_only, "--summary-only"),
                (two_shanten_self_tsumo, "--two-shanten-self-tsumo"),
                (
                    two_shanten_self_tsumo_cost.is_some(),
                    "--two-shanten-self-tsumo-cost",
                ),
                (
                    two_shanten_progress_self_tsumo_cost.is_some(),
                    "--two-shanten-progress-self-tsumo-cost",
                ),
                (
                    three_shanten_progress_self_tsumo,
                    "--three-shanten-progress-self-tsumo",
                ),
            ] {
                if enabled {
                    return Err(CliError::ConflictingThreeShantenContinuationComparison(
                        option.to_string(),
                    ));
                }
            }
        }

        if iishanten_continuation_depth_comparison {
            for (enabled, option) in [
                (lookahead, "--lookahead"),
                (verbose, "--verbose"),
                (summary_only, "--summary-only"),
                (two_shanten_self_tsumo, "--two-shanten-self-tsumo"),
                (
                    two_shanten_self_tsumo_cost.is_some(),
                    "--two-shanten-self-tsumo-cost",
                ),
                (
                    two_shanten_progress_self_tsumo_cost.is_some(),
                    "--two-shanten-progress-self-tsumo-cost",
                ),
                (
                    three_shanten_progress_self_tsumo,
                    "--three-shanten-progress-self-tsumo",
                ),
                (
                    three_shanten_continuation_comparison,
                    "--three-shanten-continuation-comparison",
                ),
                (
                    iishanten_selection_depth_comparison,
                    "--iishanten-selection-depth-comparison",
                ),
                (
                    iishanten_selection_parallel_comparison,
                    "--iishanten-selection-parallel-comparison",
                ),
                (
                    two_shanten_full_parallel_comparison,
                    "--two-shanten-full-parallel-comparison",
                ),
            ] {
                if enabled {
                    return Err(CliError::ConflictingIishantenContinuationDepthComparison(
                        option.to_string(),
                    ));
                }
            }
        }

        // 計測は他の診断を一切走らせない。先行する深い探索は向聴・受け入れの memo を温めるため、
        // 後続の A/B が本来より速く見えてしまう。
        if iishanten_selection_depth_comparison {
            for (enabled, option) in [
                (lookahead, "--lookahead"),
                (verbose, "--verbose"),
                (summary_only, "--summary-only"),
                (two_shanten_self_tsumo, "--two-shanten-self-tsumo"),
                (
                    two_shanten_self_tsumo_cost.is_some(),
                    "--two-shanten-self-tsumo-cost",
                ),
                (
                    two_shanten_progress_self_tsumo_cost.is_some(),
                    "--two-shanten-progress-self-tsumo-cost",
                ),
                (
                    three_shanten_progress_self_tsumo,
                    "--three-shanten-progress-self-tsumo",
                ),
                (
                    three_shanten_continuation_comparison,
                    "--three-shanten-continuation-comparison",
                ),
                (
                    iishanten_continuation_depth_comparison,
                    "--iishanten-continuation-depth-comparison",
                ),
                (
                    iishanten_selection_parallel_comparison,
                    "--iishanten-selection-parallel-comparison",
                ),
                (
                    two_shanten_full_parallel_comparison,
                    "--two-shanten-full-parallel-comparison",
                ),
            ] {
                if enabled {
                    return Err(CliError::ConflictingIishantenSelectionDepthComparison(
                        option.to_string(),
                    ));
                }
            }
        }

        // 候補並列の計測も同じく他の診断を走らせない。先行する深い探索は向聴・受け入れの memo を
        // 温めるため、後続の方式が本来より速く見えてしまう。
        if iishanten_selection_parallel_comparison {
            for (enabled, option) in [
                (lookahead, "--lookahead"),
                (verbose, "--verbose"),
                (summary_only, "--summary-only"),
                (two_shanten_self_tsumo, "--two-shanten-self-tsumo"),
                (
                    two_shanten_self_tsumo_cost.is_some(),
                    "--two-shanten-self-tsumo-cost",
                ),
                (
                    two_shanten_progress_self_tsumo_cost.is_some(),
                    "--two-shanten-progress-self-tsumo-cost",
                ),
                (
                    three_shanten_progress_self_tsumo,
                    "--three-shanten-progress-self-tsumo",
                ),
                (
                    three_shanten_continuation_comparison,
                    "--three-shanten-continuation-comparison",
                ),
                (
                    iishanten_continuation_depth_comparison,
                    "--iishanten-continuation-depth-comparison",
                ),
                (
                    iishanten_selection_depth_comparison,
                    "--iishanten-selection-depth-comparison",
                ),
                (
                    two_shanten_full_parallel_comparison,
                    "--two-shanten-full-parallel-comparison",
                ),
            ] {
                if enabled {
                    return Err(CliError::ConflictingIishantenSelectionParallelComparison(
                        option.to_string(),
                    ));
                }
            }
        }

        // 2向聴 Full の分け方の計測も同じく他の診断を走らせない。先行する深い探索は向聴・
        // 受け入れの memo を温めるため、後続の方式が本来より速く見えてしまう。
        if two_shanten_full_parallel_comparison {
            for (enabled, option) in [
                (lookahead, "--lookahead"),
                (verbose, "--verbose"),
                (summary_only, "--summary-only"),
                (two_shanten_self_tsumo, "--two-shanten-self-tsumo"),
                (
                    two_shanten_self_tsumo_cost.is_some(),
                    "--two-shanten-self-tsumo-cost",
                ),
                (
                    two_shanten_progress_self_tsumo_cost.is_some(),
                    "--two-shanten-progress-self-tsumo-cost",
                ),
                (
                    three_shanten_progress_self_tsumo,
                    "--three-shanten-progress-self-tsumo",
                ),
                (
                    three_shanten_continuation_comparison,
                    "--three-shanten-continuation-comparison",
                ),
                (
                    iishanten_continuation_depth_comparison,
                    "--iishanten-continuation-depth-comparison",
                ),
                (
                    iishanten_selection_depth_comparison,
                    "--iishanten-selection-depth-comparison",
                ),
                (
                    iishanten_selection_parallel_comparison,
                    "--iishanten-selection-parallel-comparison",
                ),
            ] {
                if enabled {
                    return Err(CliError::ConflictingTwoShantenFullParallelComparison(
                        option.to_string(),
                    ));
                }
            }
        }

        if three_shanten_progress_self_tsumo {
            for (enabled, option) in [
                (lookahead, "--lookahead"),
                (verbose, "--verbose"),
                (summary_only, "--summary-only"),
                (two_shanten_self_tsumo, "--two-shanten-self-tsumo"),
                (
                    two_shanten_self_tsumo_cost.is_some(),
                    "--two-shanten-self-tsumo-cost",
                ),
                (
                    two_shanten_progress_self_tsumo_cost.is_some(),
                    "--two-shanten-progress-self-tsumo-cost",
                ),
            ] {
                if enabled {
                    return Err(CliError::ConflictingThreeShantenProgressSelfTsumo(
                        option.to_string(),
                    ));
                }
            }
        }

        // 計測は cost measurement より前に追加の深い探索を走らせない。先行する探索は向聴・
        // 受け入れの memo を温めるため、後続の計測が本来より速く見えてしまう。
        if two_shanten_self_tsumo_cost.is_some() {
            let conflict = if two_shanten_self_tsumo {
                Some("--two-shanten-self-tsumo")
            } else if lookahead {
                Some("--lookahead")
            } else if verbose {
                Some("--verbose")
            } else {
                None
            };
            if let Some(conflict) = conflict {
                return Err(CliError::ConflictingTwoShantenSelfTsumoCost(
                    conflict.to_string(),
                ));
            }
        }

        if two_shanten_progress_self_tsumo_cost.is_some() {
            let conflict = if two_shanten_self_tsumo_cost.is_some() {
                Some("--two-shanten-self-tsumo-cost")
            } else if two_shanten_self_tsumo {
                Some("--two-shanten-self-tsumo")
            } else if lookahead {
                Some("--lookahead")
            } else if verbose {
                Some("--verbose")
            } else {
                None
            };
            if let Some(conflict) = conflict {
                return Err(CliError::ConflictingTwoShantenProgressSelfTsumoCost(
                    conflict.to_string(),
                ));
            }
        }

        if summary_only {
            if lookahead {
                return Err(CliError::ConflictingSummaryOnly("--lookahead".to_string()));
            }
            if two_shanten_self_tsumo {
                return Err(CliError::ConflictingSummaryOnly(
                    "--two-shanten-self-tsumo".to_string(),
                ));
            }
            if two_shanten_self_tsumo_cost.is_some() {
                return Err(CliError::ConflictingSummaryOnly(
                    "--two-shanten-self-tsumo-cost".to_string(),
                ));
            }
            if two_shanten_progress_self_tsumo_cost.is_some() {
                return Err(CliError::ConflictingSummaryOnly(
                    "--two-shanten-progress-self-tsumo-cost".to_string(),
                ));
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
                let relative_seats = relative_discards.iter().any(Option::is_some)
                    || relative_riichi.iter().any(Option::is_some);
                apply_inline_baseline(&mut spec, relative_seats);
                apply_relative_seats(&mut spec, &relative_discards, &relative_riichi)?;
                ScenarioSource::Inline(Box::new(spec))
            }
            (None, None, None) => return Err(CliError::MissingHand),
        };

        Ok(Self {
            source,
            three_shanten_progress_self_tsumo,
            three_shanten_continuation_comparison,
            iishanten_continuation_depth_comparison,
            iishanten_selection_depth_comparison,
            iishanten_selection_parallel_comparison,
            two_shanten_full_parallel_comparison,
            verbose,
            // 2向聴診断は2手先診断の枝をさらに深く追うので、明示指定は2手先診断も含む。
            lookahead: lookahead || two_shanten_self_tsumo,
            two_shanten_self_tsumo,
            two_shanten_self_tsumo_cost,
            two_shanten_progress_self_tsumo_cost,
            summary_only,
            force_fold,
        })
    }
}

const SEAT_COUNT: usize = 4;

// 相対席の option。席差は player_id 基準で shimocha = +1、toimen = +2、kamicha = +3。
const RELATIVE_SEAT_COUNT: usize = 3;
const SHIMOCHA: usize = 0;
const TOIMEN: usize = 1;
const KAMICHA: usize = 2;
const RELATIVE_SEAT_OPTIONS: [(&str, u8); RELATIVE_SEAT_COUNT] =
    [("shimocha", 1), ("toimen", 2), ("kamicha", 3)];

// 相対席の指定を player_id 基準の席 index へ展開する。CLI の INDEX は 1-based のまま
// `ScenarioSpec` へ渡し、河の枚数や `reached` との整合は JSON scenario と同じ
// `Scenario::resolve()` の canonical path で検証する。
fn apply_relative_seats(
    spec: &mut ScenarioSpec,
    discards: &[Option<String>; RELATIVE_SEAT_COUNT],
    riichi: &[Option<Option<u32>>; RELATIVE_SEAT_COUNT],
) -> Result<(), CliError> {
    let Some(first_option) = first_relative_seat_option(discards, riichi) else {
        return Ok(());
    };
    let Some(player_id) = spec.player_id else {
        return Err(CliError::RelativeSeatWithoutPlayerId(first_option));
    };

    let used_discards = discards.iter().any(Option::is_some);
    let used_riichi = riichi.iter().any(Option::is_some);

    let mut spec_discards = spec
        .discards
        .clone()
        .unwrap_or_else(|| vec![String::new(); SEAT_COUNT]);
    let mut spec_reached = spec
        .reached
        .clone()
        .unwrap_or_else(|| vec![false; SEAT_COUNT]);
    let mut spec_reach_discard_indices = spec
        .reach_discard_indices
        .clone()
        .unwrap_or_else(|| vec![None; SEAT_COUNT]);

    for (relative, (_, offset)) in RELATIVE_SEAT_OPTIONS.iter().enumerate() {
        let seat = (usize::from(player_id) + usize::from(*offset)) % SEAT_COUNT;
        if let Some(tiles) = &discards[relative] {
            spec_discards[seat] = tiles.clone();
        }
        if let Some(index) = riichi[relative] {
            spec_reached[seat] = true;
            spec_reach_discard_indices[seat] = index;
        }
    }

    if used_discards {
        spec.discards = Some(spec_discards);
    }
    if used_riichi {
        spec.reached = Some(spec_reached);
        spec.reach_discard_indices = Some(spec_reach_discard_indices);
    }
    Ok(())
}

// 指定された相対席 option のうち最初の1つの名前。どれも指定されていない場合は `None`。
fn first_relative_seat_option(
    discards: &[Option<String>; RELATIVE_SEAT_COUNT],
    riichi: &[Option<Option<u32>>; RELATIVE_SEAT_COUNT],
) -> Option<String> {
    RELATIVE_SEAT_OPTIONS
        .iter()
        .enumerate()
        .find_map(|(seat, (name, _))| {
            if discards[seat].is_some() {
                Some(format!("--discards-{name}"))
            } else if riichi[seat].is_some() {
                Some(format!("--riichi-{name}"))
            } else {
                None
            }
        })
}

// INDEX を省略できる option の値。次の token が符号なし整数の場合だけ INDEX として消費し、
// 別の option や末尾なら省略とみなして `None` を返す。
fn optional_index_of<I>(args: &mut Peekable<I>) -> Option<u32>
where
    I: Iterator<Item = String>,
{
    let index = args.peek()?.parse::<u32>().ok()?;
    args.next();
    Some(index)
}

// 簡易「何切る」用の deterministic baseline。ScenarioSpec 一般の default にはせず、inline
// `--hand` source の構築時だけ未指定 field を補う。明示 CLI option は `get_or_insert*` により
// 必ず優先される。
//
// `relative_seats` は相対席 option (`--discards-*` / `--riichi-*`) を使ったかどうか。使った
// 局面は初巡ではないので、初巡相当の残枚数を既知の事実として渡さない。
fn apply_inline_baseline(spec: &mut ScenarioSpec, relative_seats: bool) {
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
    // 相対席 option を使った局面では初巡 baseline を当てない。簡易 CLI は自分の河などを
    // 指定できず正確な山枚数を復元できないので、河の枚数から推測もせず unknown に保つ。
    if spec.remaining_tiles.is_none() && !relative_seats {
        spec.remaining_tiles = inline_initial_remaining_tiles(spec);
    }
}

// 配牌13枚 x 4人と王牌14枚を除いた70枚から、親から自席までに済んだ初巡のツモと、
// 現在のツモ牌があればその1枚を引く。player + oya が揃わない場合だけ明示自風を使い、
// どちらからも席が確定しない場合は推測しない。
fn inline_initial_remaining_tiles(spec: &ScenarioSpec) -> Option<u32> {
    const PLAYER_COUNT: u8 = 4;
    const INITIAL_LIVE_TILES: u32 = 70;

    let seat_wind = match (spec.player_id, spec.oya) {
        (Some(player_id), Some(oya)) => seat_wind_for_player(usize::from(player_id), oya),
        _ => parse_seat_wind(spec.seat_wind.as_deref()).ok().flatten(),
    }?;
    let turns_after_dealer = (0..PLAYER_COUNT)
        .find(|&seat_index| TileType::wind_from_seat_index(seat_index) == Some(seat_wind))
        .map(u32::from)?;
    let current_draw = u32::from(spec.draw.is_some());
    INITIAL_LIVE_TILES.checked_sub(turns_after_dealer + current_draw)
}

fn two_shanten_self_tsumo_cost_scope(value: &str) -> Result<TwoShantenSelfTsumoScope, CliError> {
    match value {
        "all" => Ok(TwoShantenSelfTsumoScope::AllCandidates),
        "forward-targets" => Ok(TwoShantenSelfTsumoScope::ForwardTargets),
        other => Err(CliError::InvalidTwoShantenSelfTsumoCostScope(
            other.to_string(),
        )),
    }
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

// 山の残枚数も CLI は数値化だけを担当し、局面としての妥当性は JSON scenario と同じ
// `Scenario::resolve()` の canonical path に任せる。
fn count_value_of<I>(args: &mut I, option: &str) -> Result<u32, CliError>
where
    I: Iterator<Item = String>,
{
    let value = value_of(args, option)?;
    value.parse::<u32>().map_err(|_| CliError::InvalidCount {
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

    const RELATIVE_SEAT_HAND: &str = "234m455p789s1123z";

    #[test]
    fn parses_the_relative_seat_discards() {
        let spec = inline_spec(&[
            "--hand",
            RELATIVE_SEAT_HAND,
            "--discards-shimocha",
            "1m 7p",
            "--discards-toimen",
            "4s",
            "--discards-kamicha",
            "E",
        ]);

        assert_eq!(
            spec.discards,
            Some(vec![
                String::new(),
                "1m 7p".to_string(),
                "4s".to_string(),
                "E".to_string(),
            ])
        );
        // 河だけの指定は リーチ状態を変えない。
        assert_eq!(spec.reached, None);
        assert_eq!(spec.reach_discard_indices, None);
    }

    #[test]
    fn relative_seat_discards_become_the_river_of_that_seat() {
        let context = inline_scenario(&[
            "--hand",
            RELATIVE_SEAT_HAND,
            "--discards-shimocha",
            "1m 7p 4s 7p E",
        ])
        .context;

        assert_eq!(
            context
                .discards_of(1)
                .unwrap()
                .iter()
                .map(|tile| tile.to_mjai_string())
                .collect::<Vec<_>>(),
            ["1m", "7p", "4s", "7p", "E"]
        );
        assert!(context.discards_of(0).unwrap().is_empty());
    }

    #[test]
    fn a_riichi_option_without_an_index_leaves_the_declaration_tile_unknown() {
        let context = inline_scenario(&[
            "--hand",
            RELATIVE_SEAT_HAND,
            "--discards-shimocha",
            "1m 7p 4s 7p E",
            "--riichi-shimocha",
        ])
        .context;

        assert_eq!(context.reached(), &[false, true, false, false]);
        assert_eq!(context.reach_discard_indices(), &[None; 4]);
    }

    #[test]
    fn a_riichi_option_without_an_index_works_without_discards() {
        let context = inline_scenario(&["--hand", RELATIVE_SEAT_HAND, "--riichi-toimen"]).context;

        assert_eq!(context.reached(), &[false, false, true, false]);
        assert_eq!(context.reach_discard_indices(), &[None; 4]);
    }

    #[test]
    fn a_one_based_riichi_index_becomes_a_zero_based_context_index() {
        let context = inline_scenario(&[
            "--hand",
            RELATIVE_SEAT_HAND,
            "--discards-shimocha",
            "1m 7p 4s 7p E",
            "--riichi-shimocha",
            "4",
        ])
        .context;

        assert_eq!(context.reached(), &[false, true, false, false]);
        assert_eq!(
            context.reach_discard_indices(),
            &[None, Some(3), None, None]
        );
        assert_eq!(
            context
                .reach_discard_tile_of(1)
                .map(|tile| tile.to_mjai_string()),
            Some("7p".to_string())
        );
    }

    #[test]
    fn relative_seats_follow_the_player_id() {
        for player_id in 0..4u8 {
            let context = inline_scenario(&[
                "--hand",
                RELATIVE_SEAT_HAND,
                "--player-id",
                &player_id.to_string(),
                "--oya",
                "0",
                "--discards-shimocha",
                "1m 7p",
                "--discards-toimen",
                "4s 5s",
                "--discards-kamicha",
                "E S",
                "--riichi-shimocha",
                "2",
                "--riichi-toimen",
                "--riichi-kamicha",
                "1",
            ])
            .context;

            let shimocha = usize::from((player_id + 1) % 4);
            let toimen = usize::from((player_id + 2) % 4);
            let kamicha = usize::from((player_id + 3) % 4);

            assert_eq!(context.reach_discard_index_of(shimocha), Some(1));
            assert_eq!(context.reach_discard_index_of(toimen), None);
            assert_eq!(context.reach_discard_index_of(kamicha), Some(0));
            assert!(context.is_reached(shimocha));
            assert!(context.is_reached(toimen));
            assert!(context.is_reached(kamicha));
            assert!(!context.is_reached(usize::from(player_id)));
            assert!(
                context
                    .discards_of(usize::from(player_id))
                    .unwrap()
                    .is_empty()
            );
            assert_eq!(context.discards_of(shimocha).unwrap().len(), 2);
            assert_eq!(context.discards_of(toimen).unwrap().len(), 2);
            assert_eq!(context.discards_of(kamicha).unwrap().len(), 2);
        }
    }

    #[test]
    fn a_riichi_index_beyond_the_river_is_rejected() {
        let spec = inline_spec(&[
            "--hand",
            RELATIVE_SEAT_HAND,
            "--discards-shimocha",
            "1m 7p",
            "--riichi-shimocha",
            "3",
        ]);

        assert_eq!(
            Scenario::resolve(&spec),
            Err(crate::error::ScenarioError::ReachDiscardIndexOutOfRange {
                player: 1,
                index: 3,
                discard_count: 2,
            })
        );
    }

    #[test]
    fn a_riichi_option_does_not_consume_the_next_option_as_an_index() {
        let spec = inline_spec(&[
            "--hand",
            RELATIVE_SEAT_HAND,
            "--riichi-shimocha",
            "--dora-indicator",
            "3p",
        ]);

        assert_eq!(spec.dora_indicators, Some("3p".to_string()));
        assert_eq!(spec.reached, Some(vec![false, true, false, false]));
        assert_eq!(spec.reach_discard_indices, Some(vec![None; 4]));
    }

    #[test]
    fn a_relative_seat_without_a_resolvable_player_id_is_rejected() {
        // 明示 seat_wind と oya だけを指定すると player_id は unknown のままなので、相対席を
        // 席番号へ展開できない。
        assert_eq!(
            parse(&[
                "--hand",
                RELATIVE_SEAT_HAND,
                "--seat-wind",
                "E",
                "--oya",
                "1",
                "--riichi-shimocha",
            ]),
            Err(CliError::RelativeSeatWithoutPlayerId(
                "--riichi-shimocha".to_string()
            ))
        );
    }

    #[test]
    fn relative_seat_discards_leave_the_remaining_tiles_unknown() {
        // 河を指定した局面は初巡ではないので、初巡相当の残枚数を既知の事実にしない。
        let context = inline_scenario(&[
            "--hand",
            RELATIVE_SEAT_HAND,
            "--discards-shimocha",
            "1m 7p 4s 7p E",
        ])
        .context;

        assert_eq!(context.remaining_tiles(), None);
    }

    #[test]
    fn a_riichi_option_alone_leaves_the_remaining_tiles_unknown() {
        let context = inline_scenario(&["--hand", RELATIVE_SEAT_HAND, "--riichi-shimocha"]).context;

        assert_eq!(context.remaining_tiles(), None);
    }

    #[test]
    fn an_explicit_remaining_tiles_survives_the_relative_seat_options() {
        let context = inline_scenario(&[
            "--hand",
            RELATIVE_SEAT_HAND,
            "--discards-shimocha",
            "1m 7p 4s 7p E",
            "--riichi-shimocha",
            "4",
            "--remaining-tiles",
            "42",
        ])
        .context;

        assert_eq!(context.remaining_tiles(), Some(42));
    }

    #[test]
    fn inline_scenarios_without_relative_seat_options_are_unchanged() {
        let spec = inline_spec(&["--hand", RELATIVE_SEAT_HAND]);

        assert_eq!(spec.discards, None);
        assert_eq!(spec.reached, None);
        assert_eq!(spec.reach_discard_indices, None);
    }

    #[test]
    fn parses_the_iishanten_selection_depth_comparison_option() {
        let args = parse(&[
            "--hand",
            "34567899m5799p34s",
            "--iishanten-selection-depth-comparison",
        ])
        .unwrap();
        assert!(args.iishanten_selection_depth_comparison);
        assert!(!args.iishanten_continuation_depth_comparison);
        assert!(!args.three_shanten_continuation_comparison);
        assert!(!args.lookahead);
    }

    #[test]
    fn the_iishanten_selection_depth_comparison_cannot_be_combined_with_another_diagnostic() {
        for option in [
            "--lookahead",
            "--verbose",
            "--summary-only",
            "--two-shanten-self-tsumo",
            "--three-shanten-progress-self-tsumo",
            "--three-shanten-continuation-comparison",
        ] {
            assert!(
                matches!(
                    parse(&[
                        "--hand",
                        "34567899m5799p34s",
                        option,
                        "--iishanten-selection-depth-comparison",
                    ]),
                    Err(CliError::ConflictingIishantenSelectionDepthComparison(
                        conflicting
                    )) if conflicting == option
                ),
                "{option}",
            );
        }
    }

    #[test]
    fn parses_the_iishanten_selection_parallel_comparison_option() {
        let args = parse(&[
            "--hand",
            "34567899m5799p34s",
            "--iishanten-selection-parallel-comparison",
        ])
        .unwrap();
        assert!(args.iishanten_selection_parallel_comparison);
        assert!(!args.iishanten_selection_depth_comparison);
        assert!(!args.iishanten_continuation_depth_comparison);
        assert!(!args.three_shanten_continuation_comparison);
        assert!(!args.lookahead);
    }

    #[test]
    fn the_iishanten_selection_parallel_comparison_cannot_be_combined_with_another_diagnostic() {
        for option in [
            "--lookahead",
            "--verbose",
            "--summary-only",
            "--two-shanten-self-tsumo",
            "--three-shanten-progress-self-tsumo",
            "--three-shanten-continuation-comparison",
        ] {
            assert!(
                matches!(
                    parse(&[
                        "--hand",
                        "34567899m5799p34s",
                        option,
                        "--iishanten-selection-parallel-comparison",
                    ]),
                    Err(CliError::ConflictingIishantenSelectionParallelComparison(
                        conflicting
                    )) if conflicting == option
                ),
                "{option}",
            );
        }
    }

    #[test]
    fn parses_the_two_shanten_full_parallel_comparison_option() {
        let args = parse(&[
            "--hand",
            "34567899m5799p34s",
            "--two-shanten-full-parallel-comparison",
        ])
        .unwrap();
        assert!(args.two_shanten_full_parallel_comparison);
        assert!(!args.iishanten_selection_parallel_comparison);
        assert!(!args.iishanten_selection_depth_comparison);
        assert!(!args.iishanten_continuation_depth_comparison);
        assert!(!args.three_shanten_continuation_comparison);
        assert!(!args.lookahead);
    }

    #[test]
    fn the_two_shanten_full_parallel_comparison_cannot_be_combined_with_another_diagnostic() {
        for option in [
            "--lookahead",
            "--verbose",
            "--summary-only",
            "--two-shanten-self-tsumo",
            "--three-shanten-progress-self-tsumo",
            "--three-shanten-continuation-comparison",
        ] {
            assert!(
                matches!(
                    parse(&[
                        "--hand",
                        "34567899m5799p34s",
                        option,
                        "--two-shanten-full-parallel-comparison",
                    ]),
                    Err(CliError::ConflictingTwoShantenFullParallelComparison(
                        conflicting
                    )) if conflicting == option
                ),
                "{option}",
            );
        }
    }

    #[test]
    fn the_parallel_comparison_cannot_be_combined_with_the_selection_depth_comparison() {
        // 深度 A/B と候補並列はどちらも B を走らせる。先に走った方が memo を温めないよう、
        // 同時には走らせない。
        assert!(matches!(
            parse(&[
                "--hand",
                "34567899m5799p34s",
                "--iishanten-selection-parallel-comparison",
                "--iishanten-selection-depth-comparison",
            ]),
            Err(CliError::ConflictingIishantenSelectionDepthComparison(
                conflicting
            )) if conflicting == "--iishanten-selection-parallel-comparison"
        ));
    }

    #[test]
    fn the_two_iishanten_depth_comparisons_cannot_be_combined() {
        // 先に走った深い探索が memo を温めないよう、深度診断同士も同時には走らせない。
        assert!(matches!(
            parse(&[
                "--hand",
                "34567899m5799p34s",
                "--iishanten-selection-depth-comparison",
                "--iishanten-continuation-depth-comparison",
            ]),
            Err(CliError::ConflictingIishantenContinuationDepthComparison(
                conflicting
            )) if conflicting == "--iishanten-selection-depth-comparison"
        ));
    }

    #[test]
    fn parses_the_iishanten_continuation_depth_comparison_option() {
        let args = parse(&[
            "--hand",
            "34567899m5799p34s",
            "--iishanten-continuation-depth-comparison",
        ])
        .unwrap();
        assert!(args.iishanten_continuation_depth_comparison);
        assert!(!args.three_shanten_continuation_comparison);
        assert!(!args.lookahead);
    }

    #[test]
    fn the_iishanten_continuation_depth_comparison_cannot_be_combined_with_another_diagnostic() {
        for option in [
            "--lookahead",
            "--verbose",
            "--summary-only",
            "--two-shanten-self-tsumo",
            "--three-shanten-progress-self-tsumo",
            "--three-shanten-continuation-comparison",
        ] {
            assert!(
                matches!(
                    parse(&[
                        "--hand",
                        "34567899m5799p34s",
                        option,
                        "--iishanten-continuation-depth-comparison",
                    ]),
                    Err(CliError::ConflictingIishantenContinuationDepthComparison(
                        conflicting
                    )) if conflicting == option
                ),
                "{option}",
            );
        }
    }

    #[test]
    fn parses_the_three_shanten_continuation_comparison_option() {
        let args = parse(&[
            "--hand",
            "234m455p789s1123z",
            "--three-shanten-continuation-comparison",
        ])
        .unwrap();
        assert!(args.three_shanten_continuation_comparison);
        assert!(!args.three_shanten_progress_self_tsumo);
        assert!(!args.lookahead);
    }

    #[test]
    fn the_three_shanten_continuation_comparison_cannot_be_combined_with_another_diagnostic() {
        for option in [
            "--lookahead",
            "--verbose",
            "--summary-only",
            "--two-shanten-self-tsumo",
            "--three-shanten-progress-self-tsumo",
        ] {
            assert_eq!(
                parse(&[
                    "--hand",
                    "234m455p789s1123z",
                    "--three-shanten-continuation-comparison",
                    option,
                ]),
                Err(CliError::ConflictingThreeShantenContinuationComparison(
                    option.to_string()
                )),
                "{option}"
            );
        }
    }

    #[test]
    fn parses_the_capture_comparison_paths() {
        let args = parse(&[
            "--compare-three-shanten-continuation",
            "first.jsonl",
            "second.jsonl",
        ])
        .unwrap();
        assert_eq!(
            args.source,
            ScenarioSource::RiichilabCaptureComparison(CaptureComparisonSpec {
                paths: vec!["first.jsonl".to_string(), "second.jsonl".to_string()],
            })
        );
        assert!(!args.three_shanten_continuation_comparison);
    }

    #[test]
    fn the_capture_comparison_cannot_be_combined_with_the_scenario_options() {
        assert_eq!(
            parse(&[
                "--compare-three-shanten-continuation",
                "capture.jsonl",
                "--hand",
                "234m455p789s1123z",
            ]),
            Err(CliError::ConflictingCaptureComparisonInput(
                "--hand".to_string()
            ))
        );
        assert_eq!(
            parse(&[
                "--compare-three-shanten-continuation",
                "capture.jsonl",
                "--benchmark-json",
                "out.json",
            ]),
            Err(CliError::ConflictingCaptureComparisonInput(
                "--benchmark-json".to_string()
            ))
        );
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
        assert_eq!(spec.remaining_tiles, Some(67));
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
    fn explicit_remaining_tiles_override_the_inline_baseline() {
        let spec = inline_spec(&[
            "--hand",
            "123m",
            "--draw",
            "9s",
            "--player-id",
            "0",
            "--oya",
            "1",
            "--remaining-tiles",
            "42",
        ]);
        assert_eq!(spec.remaining_tiles, Some(42));
        assert_eq!(
            Scenario::resolve(&spec).unwrap().context.remaining_tiles(),
            Some(42)
        );
    }

    #[test]
    fn inline_remaining_tiles_follow_the_first_turn_order_and_draw_state() {
        for oya in 0..4u8 {
            for player_id in 0..4u8 {
                for has_draw in [false, true] {
                    let player_id_arg = player_id.to_string();
                    let oya_arg = oya.to_string();
                    let mut args = vec![
                        "--hand",
                        "123m",
                        "--player-id",
                        &player_id_arg,
                        "--oya",
                        &oya_arg,
                    ];
                    if has_draw {
                        args.extend(["--draw", "9s"]);
                    }

                    let turns_after_dealer = u32::from((player_id + 4 - oya) % 4);
                    let expected = 70 - turns_after_dealer - u32::from(has_draw);
                    assert_eq!(
                        inline_spec(&args).remaining_tiles,
                        Some(expected),
                        "player {player_id}, dealer {oya}, draw {has_draw}",
                    );
                }
            }
        }

        let baseline = inline_spec(&["--hand", "11258m234789p13s", "--draw", "9s"]);
        assert_eq!(baseline.player_id, Some(0));
        assert_eq!(baseline.oya, Some(1));
        assert_eq!(baseline.remaining_tiles, Some(66));
    }

    #[test]
    fn inline_remaining_tiles_follow_explicit_seat_wind_and_draw_state() {
        for (seat_wind, turns_after_dealer) in [("E", 0), ("S", 1), ("W", 2), ("N", 3)] {
            for has_draw in [false, true] {
                let mut args = vec!["--hand", "123m", "--seat-wind", seat_wind];
                if has_draw {
                    args.extend(["--draw", "9s"]);
                }

                let spec = inline_spec(&args);
                assert_eq!(spec.player_id, Some(0));
                assert_eq!(spec.oya, None);
                let expected = Some(70 - turns_after_dealer - u32::from(has_draw));
                assert_eq!(spec.remaining_tiles, expected);
                assert_eq!(
                    Scenario::resolve(&spec).unwrap().context.remaining_tiles(),
                    expected,
                    "seat wind {seat_wind}, draw {has_draw}",
                );
            }
        }
    }

    #[test]
    fn rejects_remaining_tiles_without_a_number() {
        assert_eq!(
            parse(&["--hand", "123m", "--remaining-tiles", "many"]),
            Err(CliError::InvalidCount {
                option: "--remaining-tiles".to_string(),
                value: "many".to_string(),
            })
        );
        assert_eq!(
            parse(&["--hand", "123m", "--remaining-tiles"]),
            Err(CliError::MissingValue("--remaining-tiles".to_string()))
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
    fn parses_two_shanten_self_tsumo_flag() {
        let default = parse(&["--hand", "123m"]).unwrap();
        assert!(!default.two_shanten_self_tsumo);
        assert!(!default.lookahead);

        // 2向聴診断は2手先診断の枝をさらに深く追うので、明示指定は2手先診断も含む。
        let requested = parse(&["--hand", "123m", "--two-shanten-self-tsumo"]).unwrap();
        assert!(requested.two_shanten_self_tsumo);
        assert!(requested.lookahead);

        assert!(
            parse(&["scenario.json", "--two-shanten-self-tsumo"])
                .unwrap()
                .two_shanten_self_tsumo
        );
        assert_eq!(
            parse(&[
                "--hand",
                "123m",
                "--summary-only",
                "--two-shanten-self-tsumo"
            ]),
            Err(CliError::ConflictingSummaryOnly(
                "--two-shanten-self-tsumo".to_string()
            ))
        );
    }

    #[test]
    fn parses_two_shanten_self_tsumo_cost_scope() {
        // 計測は既定では行わない。範囲は明示指定した値がそのまま入り、他の診断は変わらない。
        let default = parse(&["--hand", "123m"]).unwrap();
        assert_eq!(default.two_shanten_self_tsumo_cost, None);
        assert_eq!(default.two_shanten_progress_self_tsumo_cost, None);

        let all = parse(&["--hand", "123m", "--two-shanten-self-tsumo-cost", "all"]).unwrap();
        assert_eq!(
            all.two_shanten_self_tsumo_cost,
            Some(TwoShantenSelfTsumoScope::AllCandidates)
        );
        assert!(!all.two_shanten_self_tsumo);
        assert!(!all.lookahead);
        assert_eq!(all.two_shanten_progress_self_tsumo_cost, None);

        assert_eq!(
            parse(&[
                "--hand",
                "123m",
                "--two-shanten-self-tsumo-cost",
                "forward-targets"
            ])
            .unwrap()
            .two_shanten_self_tsumo_cost,
            Some(TwoShantenSelfTsumoScope::ForwardTargets)
        );

        assert_eq!(
            parse(&["--hand", "123m", "--two-shanten-self-tsumo-cost", "cohort"]),
            Err(CliError::InvalidTwoShantenSelfTsumoCostScope(
                "cohort".to_string()
            ))
        );
        assert_eq!(
            parse(&["--hand", "123m", "--two-shanten-self-tsumo-cost"]),
            Err(CliError::MissingValue(
                "--two-shanten-self-tsumo-cost".to_string()
            ))
        );

        let progress = parse(&[
            "--hand",
            "123m",
            "--two-shanten-progress-self-tsumo-cost",
            "forward-targets",
        ])
        .unwrap();
        assert_eq!(
            progress.two_shanten_progress_self_tsumo_cost,
            Some(TwoShantenSelfTsumoScope::ForwardTargets)
        );
        assert_eq!(progress.two_shanten_self_tsumo_cost, None);
        assert!(!progress.two_shanten_self_tsumo);
        assert!(!progress.lookahead);
        assert_eq!(
            parse(&[
                "--hand",
                "123m",
                "--two-shanten-progress-self-tsumo-cost",
                "cohort",
            ]),
            Err(CliError::InvalidTwoShantenProgressSelfTsumoCostScope(
                "cohort".to_string()
            ))
        );
    }

    #[test]
    fn the_two_shanten_self_tsumo_cost_cannot_be_combined_with_another_diagnostic() {
        // 計測より前に深い探索を走らせると memo が温まって実測が本来より速く見えるため、他の
        // 診断 option とは同時に指定できない。どちらの範囲でも同じ扱いになる。
        for scope in ["all", "forward-targets"] {
            for option in ["--lookahead", "--verbose", "--two-shanten-self-tsumo"] {
                assert_eq!(
                    parse(&[
                        "--hand",
                        "123m",
                        "--two-shanten-self-tsumo-cost",
                        scope,
                        option
                    ]),
                    Err(CliError::ConflictingTwoShantenSelfTsumoCost(
                        option.to_string()
                    )),
                    "{scope} と {option}"
                );
            }

            assert_eq!(
                parse(&[
                    "--hand",
                    "123m",
                    "--summary-only",
                    "--two-shanten-self-tsumo-cost",
                    scope
                ]),
                Err(CliError::ConflictingSummaryOnly(
                    "--two-shanten-self-tsumo-cost".to_string()
                )),
                "{scope} と --summary-only"
            );

            // 単独指定は引き続き有効で、他の診断は要求しない。
            let args = parse(&["--hand", "123m", "--two-shanten-self-tsumo-cost", scope]).unwrap();
            assert!(args.two_shanten_self_tsumo_cost.is_some());
            assert!(!args.lookahead);
            assert!(!args.two_shanten_self_tsumo);
            assert!(!args.verbose);
            assert!(!args.summary_only);
        }

        assert_eq!(
            parse(&[
                "--hand",
                "123m",
                "--two-shanten-progress-self-tsumo-cost",
                "forward-targets",
                "--two-shanten-self-tsumo-cost",
                "forward-targets",
            ]),
            Err(CliError::ConflictingTwoShantenProgressSelfTsumoCost(
                "--two-shanten-self-tsumo-cost".to_string()
            ))
        );
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

    #[test]
    fn parses_force_fold_for_every_scenario_input() {
        assert!(
            parse(&["--hand", "123m", "--force-fold"])
                .unwrap()
                .force_fold
        );
        assert!(
            parse(&["scenario.json", "--force-fold"])
                .unwrap()
                .force_fold
        );
        assert!(
            parse(&["--riichilab-capture", "capture.jsonl", "--force-fold"])
                .unwrap()
                .force_fold
        );
        assert!(
            parse(&["--hand", "123m", "--force-fold", "--summary-only"])
                .unwrap()
                .force_fold
        );
        assert!(!parse(&["--hand", "123m"]).unwrap().force_fold);
    }

    #[test]
    fn rejects_force_fold_with_the_normal_discard_diagnostics() {
        for option in [
            "--lookahead",
            "--two-shanten-self-tsumo",
            "--three-shanten-progress-self-tsumo",
            "--three-shanten-continuation-comparison",
            "--iishanten-continuation-depth-comparison",
            "--iishanten-selection-depth-comparison",
            "--iishanten-selection-parallel-comparison",
            "--two-shanten-full-parallel-comparison",
        ] {
            assert_eq!(
                parse(&["--hand", "123m", "--force-fold", option]),
                Err(CliError::ConflictingForceFold(option.to_string())),
                "{option}"
            );
        }

        for option in [
            "--two-shanten-self-tsumo-cost",
            "--two-shanten-progress-self-tsumo-cost",
        ] {
            assert_eq!(
                parse(&["--hand", "123m", "--force-fold", option, "all"]),
                Err(CliError::ConflictingForceFold(option.to_string())),
                "{option}"
            );
        }
    }

    #[test]
    fn rejects_force_fold_with_the_capture_only_modes() {
        assert_eq!(
            parse(&[
                "--benchmark-riichilab-capture",
                "capture.jsonl",
                "--force-fold",
            ]),
            Err(CliError::ConflictingBenchmarkInput(
                "--force-fold".to_string()
            ))
        );
        assert_eq!(
            parse(&[
                "--compare-three-shanten-continuation",
                "capture.jsonl",
                "--force-fold",
            ]),
            Err(CliError::ConflictingCaptureComparisonInput(
                "--force-fold".to_string()
            ))
        );
    }
}
