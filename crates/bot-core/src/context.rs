use bot_logic::{TileId, TileType};

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GameContext {
    drawn_tile: Option<TileId>,
    hand_tiles: Vec<TileId>,
    dora_indicators: Vec<TileId>,
    round_wind: Option<TileType>,
    seat_wind: Option<TileType>,
    visible_tiles: Vec<TileId>,
}

impl GameContext {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_drawn_tile(drawn_tile: TileId) -> Self {
        Self {
            drawn_tile: Some(drawn_tile),
            ..Self::default()
        }
    }

    pub fn with_hand_tiles(hand_tiles: Vec<TileId>) -> Self {
        Self {
            hand_tiles,
            ..Self::default()
        }
    }

    pub fn from_parts(drawn_tile: Option<TileId>, hand_tiles: Vec<TileId>) -> Self {
        Self {
            drawn_tile,
            hand_tiles,
            ..Self::default()
        }
    }

    pub fn from_parts_with_dora(
        drawn_tile: Option<TileId>,
        hand_tiles: Vec<TileId>,
        dora_indicators: Vec<TileId>,
    ) -> Self {
        Self {
            drawn_tile,
            hand_tiles,
            dora_indicators,
            ..Self::default()
        }
    }

    pub fn from_parts_with_context(
        drawn_tile: Option<TileId>,
        hand_tiles: Vec<TileId>,
        dora_indicators: Vec<TileId>,
        round_wind: Option<TileType>,
        seat_wind: Option<TileType>,
    ) -> Self {
        Self::from_parts_with_visible_tiles(
            drawn_tile,
            hand_tiles,
            dora_indicators,
            round_wind,
            seat_wind,
            Vec::new(),
        )
    }

    pub fn from_parts_with_visible_tiles(
        drawn_tile: Option<TileId>,
        hand_tiles: Vec<TileId>,
        dora_indicators: Vec<TileId>,
        round_wind: Option<TileType>,
        seat_wind: Option<TileType>,
        visible_tiles: Vec<TileId>,
    ) -> Self {
        Self {
            drawn_tile,
            hand_tiles,
            dora_indicators,
            round_wind,
            seat_wind,
            visible_tiles,
        }
    }

    pub fn drawn_tile(&self) -> Option<TileId> {
        self.drawn_tile
    }

    pub fn hand_tiles(&self) -> &[TileId] {
        &self.hand_tiles
    }

    pub fn dora_indicators(&self) -> &[TileId] {
        &self.dora_indicators
    }

    pub fn round_wind(&self) -> Option<TileType> {
        self.round_wind
    }

    pub fn seat_wind(&self) -> Option<TileType> {
        self.seat_wind
    }

    pub fn visible_tiles(&self) -> &[TileId] {
        &self.visible_tiles
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tile(value: u8) -> TileId {
        TileId::new(value).unwrap()
    }

    fn wind(value: u8) -> TileType {
        TileType::new(value).unwrap()
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
    fn default_has_no_dora_indicators() {
        assert!(GameContext::default().dora_indicators().is_empty());
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
    fn with_drawn_tile_has_no_dora_indicators() {
        assert!(
            GameContext::with_drawn_tile(tile(16))
                .dora_indicators()
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
    fn with_hand_tiles_has_no_dora_indicators() {
        assert!(
            GameContext::with_hand_tiles(vec![tile(0)])
                .dora_indicators()
                .is_empty()
        );
    }

    #[test]
    fn from_parts_holds_both() {
        let hand_tiles = vec![tile(0), tile(56)];
        let context = GameContext::from_parts(Some(tile(16)), hand_tiles.clone());
        assert_eq!(context.drawn_tile(), Some(tile(16)));
        assert_eq!(context.hand_tiles(), hand_tiles.as_slice());
    }

    #[test]
    fn from_parts_has_no_dora_indicators() {
        let context = GameContext::from_parts(Some(tile(16)), vec![tile(0)]);
        assert!(context.dora_indicators().is_empty());
    }

    #[test]
    fn from_parts_with_dora_holds_all() {
        let hand_tiles = vec![tile(0), tile(56)];
        let dora_indicators = vec![tile(4), tile(20)];
        let context = GameContext::from_parts_with_dora(
            Some(tile(16)),
            hand_tiles.clone(),
            dora_indicators.clone(),
        );
        assert_eq!(context.drawn_tile(), Some(tile(16)));
        assert_eq!(context.hand_tiles(), hand_tiles.as_slice());
        assert_eq!(context.dora_indicators(), dora_indicators.as_slice());
    }

    #[test]
    fn hand_tiles_returns_slice() {
        let context = GameContext::with_hand_tiles(vec![tile(0)]);
        let slice: &[TileId] = context.hand_tiles();
        assert_eq!(slice, &[tile(0)]);
    }

    #[test]
    fn dora_indicators_returns_slice() {
        let context = GameContext::from_parts_with_dora(None, vec![], vec![tile(4)]);
        let slice: &[TileId] = context.dora_indicators();
        assert_eq!(slice, &[tile(4)]);
    }

    #[test]
    fn default_has_no_winds() {
        let context = GameContext::default();
        assert_eq!(context.round_wind(), None);
        assert_eq!(context.seat_wind(), None);
    }

    #[test]
    fn existing_constructors_have_no_winds() {
        for context in [
            GameContext::new(),
            GameContext::with_drawn_tile(tile(16)),
            GameContext::with_hand_tiles(vec![tile(0)]),
            GameContext::from_parts(Some(tile(16)), vec![tile(0)]),
            GameContext::from_parts_with_dora(Some(tile(16)), vec![tile(0)], vec![tile(4)]),
        ] {
            assert_eq!(context.round_wind(), None);
            assert_eq!(context.seat_wind(), None);
        }
    }

    #[test]
    fn from_parts_with_context_holds_winds() {
        let context = GameContext::from_parts_with_context(
            Some(tile(16)),
            vec![tile(0)],
            vec![tile(4)],
            Some(wind(27)),
            Some(wind(28)),
        );
        assert_eq!(context.round_wind(), Some(wind(27)));
        assert_eq!(context.seat_wind(), Some(wind(28)));
    }

    #[test]
    fn from_parts_with_context_keeps_other_parts() {
        let hand_tiles = vec![tile(0), tile(56)];
        let dora_indicators = vec![tile(4)];
        let context = GameContext::from_parts_with_context(
            Some(tile(16)),
            hand_tiles.clone(),
            dora_indicators.clone(),
            None,
            None,
        );
        assert_eq!(context.drawn_tile(), Some(tile(16)));
        assert_eq!(context.hand_tiles(), hand_tiles.as_slice());
        assert_eq!(context.dora_indicators(), dora_indicators.as_slice());
        assert_eq!(context.round_wind(), None);
        assert_eq!(context.seat_wind(), None);
    }

    #[test]
    fn from_parts_with_context_none_winds_equals_from_parts_with_dora() {
        let hand_tiles = vec![tile(0), tile(56)];
        let dora_indicators = vec![tile(4)];
        assert_eq!(
            GameContext::from_parts_with_context(
                Some(tile(16)),
                hand_tiles.clone(),
                dora_indicators.clone(),
                None,
                None,
            ),
            GameContext::from_parts_with_dora(Some(tile(16)), hand_tiles, dora_indicators)
        );
    }

    #[test]
    fn default_has_no_visible_tiles() {
        assert!(GameContext::default().visible_tiles().is_empty());
    }

    #[test]
    fn new_has_no_visible_tiles() {
        assert!(GameContext::new().visible_tiles().is_empty());
    }

    #[test]
    fn existing_constructors_have_no_visible_tiles() {
        for context in [
            GameContext::new(),
            GameContext::with_drawn_tile(tile(16)),
            GameContext::with_hand_tiles(vec![tile(0)]),
            GameContext::from_parts(Some(tile(16)), vec![tile(0)]),
            GameContext::from_parts_with_dora(Some(tile(16)), vec![tile(0)], vec![tile(4)]),
            GameContext::from_parts_with_context(
                Some(tile(16)),
                vec![tile(0)],
                vec![tile(4)],
                Some(wind(27)),
                Some(wind(28)),
            ),
        ] {
            assert!(context.visible_tiles().is_empty());
        }
    }

    #[test]
    fn from_parts_with_visible_tiles_holds_visible_tiles() {
        let visible_tiles = vec![tile(0), tile(16), tile(16)];
        let context = GameContext::from_parts_with_visible_tiles(
            Some(tile(16)),
            vec![tile(0)],
            vec![tile(4)],
            None,
            None,
            visible_tiles.clone(),
        );
        assert_eq!(context.visible_tiles(), visible_tiles.as_slice());
    }

    #[test]
    fn from_parts_with_visible_tiles_keeps_other_parts() {
        let hand_tiles = vec![tile(0), tile(56)];
        let dora_indicators = vec![tile(4)];
        let visible_tiles = vec![tile(0), tile(56), tile(4)];
        let context = GameContext::from_parts_with_visible_tiles(
            Some(tile(16)),
            hand_tiles.clone(),
            dora_indicators.clone(),
            Some(wind(27)),
            Some(wind(28)),
            visible_tiles.clone(),
        );
        assert_eq!(context.drawn_tile(), Some(tile(16)));
        assert_eq!(context.hand_tiles(), hand_tiles.as_slice());
        assert_eq!(context.dora_indicators(), dora_indicators.as_slice());
        assert_eq!(context.round_wind(), Some(wind(27)));
        assert_eq!(context.seat_wind(), Some(wind(28)));
        assert_eq!(context.visible_tiles(), visible_tiles.as_slice());
    }

    #[test]
    fn from_parts_with_context_has_no_visible_tiles() {
        let context = GameContext::from_parts_with_context(
            Some(tile(16)),
            vec![tile(0)],
            vec![tile(4)],
            Some(wind(27)),
            Some(wind(28)),
        );
        assert!(context.visible_tiles().is_empty());
    }

    #[test]
    fn from_parts_with_visible_tiles_empty_equals_from_parts_with_context() {
        let hand_tiles = vec![tile(0), tile(56)];
        let dora_indicators = vec![tile(4)];
        assert_eq!(
            GameContext::from_parts_with_visible_tiles(
                Some(tile(16)),
                hand_tiles.clone(),
                dora_indicators.clone(),
                Some(wind(27)),
                Some(wind(28)),
                Vec::new(),
            ),
            GameContext::from_parts_with_context(
                Some(tile(16)),
                hand_tiles,
                dora_indicators,
                Some(wind(27)),
                Some(wind(28)),
            )
        );
    }

    #[test]
    fn visible_tiles_returns_slice() {
        let context = GameContext::from_parts_with_visible_tiles(
            None,
            vec![],
            vec![],
            None,
            None,
            vec![tile(0)],
        );
        let slice: &[TileId] = context.visible_tiles();
        assert_eq!(slice, &[tile(0)]);
    }
}
