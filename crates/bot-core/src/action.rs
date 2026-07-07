use bot_logic::TileId;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LegalAction {
    Dahai { tile: TileId },
    Reach,
    Hora,
    Ryukyoku,
    None,
}
