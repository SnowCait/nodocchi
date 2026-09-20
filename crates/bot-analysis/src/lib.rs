//! platform 非依存の局面解析基盤。
//!
//! 牌文字列の parse、physical tile の割り当て、scenario の validation、`GameContext` と
//! `LegalAction` の構築を担い、`ScenarioSpec` → [`Scenario::resolve()`] → [`Scenario`] を
//! 局面構築の source of truth とする。CLI 引数・file I/O・capture 再生・出力整形のような
//! platform 固有の責務は持たない。

mod error;
mod input;
mod scenario;
mod tiles;

pub use crate::error::ScenarioBuildError;
pub use crate::input::{LogicalTile, TileInputError, parse_tiles};
pub use crate::scenario::{
    DoubleRiichiSpec, HistoryFuritenSpec, MeldKindSpec, MeldSpec, PonActionSpec,
    RiichiSituationSpec, Scenario, ScenarioSpec, parse_seat_wind,
};
pub use crate::tiles::TileAllocationError;
