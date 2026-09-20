//! platform 非依存の局面解析基盤。
//!
//! 牌文字列の parse、physical tile の割り当て、scenario の validation、`GameContext` と
//! `LegalAction` の構築を担い、`ScenarioSpec` → [`Scenario::resolve()`] → [`Scenario`] を
//! 局面構築の source of truth とする。その局面と production 診断から choice 1 / 2 / 3 の
//! 順位付け ([`rank_choices`]) も行い、表示に必要な値だけを薄い構造化結果として返す。
//! CLI 引数・file I/O・capture 再生・出力整形のような platform 固有の責務は持たない。

mod error;
mod input;
mod ranked_choice;
mod scenario;
mod tiles;

pub use crate::error::ScenarioBuildError;
pub use crate::input::{LogicalTile, TileInputError, parse_tiles};
pub use crate::ranked_choice::{
    RankedChoice, RankedChoiceComparison, RankedChoiceComparisonValues, RankedChoiceHitProbability,
    RankedChoiceHonorValue, rank_choices,
};
pub use crate::scenario::{
    DoubleRiichiSpec, HistoryFuritenSpec, MeldKindSpec, MeldSpec, PonActionSpec,
    RiichiSituationSpec, Scenario, ScenarioSpec, parse_seat_wind,
};
pub use crate::tiles::TileAllocationError;
