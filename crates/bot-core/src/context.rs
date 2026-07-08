use bot_logic::TileId;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GameContext {
    drawn_tile: Option<TileId>,
    hand_tiles: Vec<TileId>,
}

impl GameContext {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_drawn_tile(drawn_tile: TileId) -> Self {
        Self {
            drawn_tile: Some(drawn_tile),
            hand_tiles: Vec::new(),
        }
    }

    pub fn with_hand_tiles(hand_tiles: Vec<TileId>) -> Self {
        Self {
            drawn_tile: None,
            hand_tiles,
        }
    }

    pub fn from_parts(drawn_tile: Option<TileId>, hand_tiles: Vec<TileId>) -> Self {
        Self {
            drawn_tile,
            hand_tiles,
        }
    }

    pub fn drawn_tile(&self) -> Option<TileId> {
        self.drawn_tile
    }

    pub fn hand_tiles(&self) -> &[TileId] {
        &self.hand_tiles
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tile(value: u8) -> TileId {
        TileId::new(value).unwrap()
    }

    #[test]
    fn default_has_no_drawn_tile() {
        assert_eq!(GameContext::default().drawn_tile(), None);
    }

    #[test]
    fn default_has_no_hand_tiles() {
        assert!(GameContext::default().hand_tiles().is_empty());
    }

    #[test]
    fn new_has_no_drawn_tile() {
        assert_eq!(GameContext::new().drawn_tile(), None);
    }

    #[test]
    fn new_has_no_hand_tiles() {
        assert!(GameContext::new().hand_tiles().is_empty());
    }

    #[test]
    fn with_drawn_tile_holds_tile() {
        let tile = tile(16);
        assert_eq!(GameContext::with_drawn_tile(tile).drawn_tile(), Some(tile));
    }

    #[test]
    fn with_drawn_tile_has_no_hand_tiles() {
        assert!(
            GameContext::with_drawn_tile(tile(16))
                .hand_tiles()
                .is_empty()
        );
    }

    #[test]
    fn with_hand_tiles_holds_tiles_without_drawn_tile() {
        let hand_tiles = vec![tile(0), tile(16), tile(56)];
        let context = GameContext::with_hand_tiles(hand_tiles.clone());
        assert_eq!(context.hand_tiles(), hand_tiles.as_slice());
        assert_eq!(context.drawn_tile(), None);
    }

    #[test]
    fn from_parts_holds_both() {
        let hand_tiles = vec![tile(0), tile(56)];
        let context = GameContext::from_parts(Some(tile(16)), hand_tiles.clone());
        assert_eq!(context.drawn_tile(), Some(tile(16)));
        assert_eq!(context.hand_tiles(), hand_tiles.as_slice());
    }

    #[test]
    fn hand_tiles_returns_slice() {
        let context = GameContext::with_hand_tiles(vec![tile(0)]);
        let slice: &[TileId] = context.hand_tiles();
        assert_eq!(slice, &[tile(0)]);
    }
}
