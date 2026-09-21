//! platform 非依存の局面解析基盤。
//!
//! 牌文字列の parse、physical tile の割り当て、scenario の validation、`GameContext` と
//! `LegalAction` の構築を担い、`ScenarioSpec` → [`Scenario::resolve()`] → [`Scenario`] を
//! 手入力 / scenario JSON からの局面構築の source of truth とする。
//!
//! 解析の入口は `GameContext` と合法手で、`Scenario` 型そのものではない。scenario 構築の
//! 結果も、将来 replay から復元する decision point も、同じ [`analyze()`] へ繋がる。
//! [`analyze()`] は production 診断を1回だけ取り、そこから choice 1 / 2 / 3 の
//! 順位付け ([`rank_choices`]) と、九種九牌・押し引き・Reach / Damaten・鳴き・防御を
//! 合わせて [`AnalysisResult`] へ投影する。consumer が見るのはこの薄い構造化結果だけで、
//! 内部診断そのものは公開しない。
//! CLI 引数・file I/O・capture 再生・出力整形のような platform 固有の責務は持たない。

mod analysis_result;
mod analyze;
mod error;
mod input;
mod ranked_choice;
mod scenario;
mod tiles;

pub use crate::analysis_result::{
    AnalysisCall, AnalysisCallCandidate, AnalysisCallReasonSource, AnalysisCallSelfTsumo,
    AnalysisCallSelfTsumoComparison, AnalysisDamaten, AnalysisDamatenWinningTile, AnalysisDefense,
    AnalysisDiscardTile, AnalysisPushPull, AnalysisReach, AnalysisReachDecision,
    AnalysisReachTenpaiWait, AnalysisReachThreatDefense, AnalysisReachVerdict, AnalysisResult,
    AnalysisRyukyoku, AnalysisTenpaiOffense,
};
pub use crate::analyze::analyze;
pub use crate::error::ScenarioBuildError;
pub use crate::input::{LogicalTile, TileInputError, parse_tiles};
pub use crate::ranked_choice::{
    AnalysisOpponentHonorValue, RankedChoice, RankedChoiceComparison, RankedChoiceComparisonValues,
    RankedChoiceHitProbability, rank_choices,
};
pub use crate::scenario::{
    DoubleRiichiSpec, HistoryFuritenSpec, MeldKindSpec, MeldSpec, PonActionSpec,
    RiichiSituationSpec, Scenario, ScenarioSpec, parse_seat_wind,
};
pub use crate::tiles::TileAllocationError;
