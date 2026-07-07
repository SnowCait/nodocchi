use bot_logic::TileId;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GameContext {
    drawn_tile: Option<TileId>,
}

impl GameContext {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_drawn_tile(drawn_tile: TileId) -> Self {
        Self {
            drawn_tile: Some(drawn_tile),
        }
    }

    pub fn drawn_tile(&self) -> Option<TileId> {
        self.drawn_tile
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_has_no_drawn_tile() {
        assert_eq!(GameContext::default().drawn_tile(), None);
    }

    #[test]
    fn new_has_no_drawn_tile() {
        assert_eq!(GameContext::new().drawn_tile(), None);
    }

    #[test]
    fn with_drawn_tile_holds_tile() {
        let tile = TileId::new(16).unwrap();
        assert_eq!(GameContext::with_drawn_tile(tile).drawn_tile(), Some(tile));
    }
}
