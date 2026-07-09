use bot_logic::{TileId, TileType};

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GameContext {
    drawn_tile: Option<TileId>,
    hand_tiles: Vec<TileId>,
    dora_indicators: Vec<TileId>,
    round_wind: Option<TileType>,
    seat_wind: Option<TileType>,
    visible_tiles: Vec<TileId>,
    player_id: Option<u8>,
    oya: Option<u8>,
    discards: [Vec<TileId>; 4],
    reached: [bool; 4],
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
            ..Self::default()
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn from_parts_with_table_state(
        drawn_tile: Option<TileId>,
        hand_tiles: Vec<TileId>,
        dora_indicators: Vec<TileId>,
        round_wind: Option<TileType>,
        seat_wind: Option<TileType>,
        visible_tiles: Vec<TileId>,
        player_id: Option<u8>,
        oya: Option<u8>,
        discards: [Vec<TileId>; 4],
        reached: [bool; 4],
    ) -> Self {
        Self {
            drawn_tile,
            hand_tiles,
            dora_indicators,
            round_wind,
            seat_wind,
            visible_tiles,
            player_id,
            oya,
            discards,
            reached,
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

    pub fn player_id(&self) -> Option<u8> {
        self.player_id
    }

    pub fn oya(&self) -> Option<u8> {
        self.oya
    }

    // discards は防御・現物判定用に player ごとの河として保持する。
    pub fn discards(&self) -> &[Vec<TileId>; 4] {
        &self.discards
    }

    pub fn discards_of(&self, player: usize) -> Option<&[TileId]> {
        self.discards.get(player).map(Vec::as_slice)
    }

    pub fn reached(&self) -> &[bool; 4] {
        &self.reached
    }

    pub fn is_reached(&self, player: usize) -> bool {
        self.reached.get(player).copied().unwrap_or(false)
    }

    pub fn any_opponent_reached(&self) -> bool {
        self.reached
            .iter()
            .enumerate()
            .any(|(player, &reached)| reached && self.player_id != Some(player as u8))
    }
}

// discards は防御・現物判定用、visible_tiles は枚数補正用なので用途を分ける。
pub fn is_genbutsu_for(tile: TileType, player: usize, context: &GameContext) -> bool {
    context
        .discards_of(player)
        .is_some_and(|discards| discards.iter().any(|t| t.tile_type() == tile))
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

    fn table_state_context(
        player_id: Option<u8>,
        oya: Option<u8>,
        discards: [Vec<TileId>; 4],
        reached: [bool; 4],
    ) -> GameContext {
        GameContext::from_parts_with_table_state(
            None,
            vec![],
            vec![],
            None,
            None,
            Vec::new(),
            player_id,
            oya,
            discards,
            reached,
        )
    }

    #[test]
    fn default_has_no_player_id_or_oya() {
        let context = GameContext::default();
        assert_eq!(context.player_id(), None);
        assert_eq!(context.oya(), None);
    }

    #[test]
    fn default_has_empty_discards() {
        let context = GameContext::default();
        assert!(context.discards().iter().all(|d| d.is_empty()));
    }

    #[test]
    fn default_has_no_reached() {
        let context = GameContext::default();
        assert_eq!(context.reached(), &[false; 4]);
    }

    #[test]
    fn existing_constructors_have_empty_table_state() {
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
            GameContext::from_parts_with_visible_tiles(
                Some(tile(16)),
                vec![tile(0)],
                vec![tile(4)],
                Some(wind(27)),
                Some(wind(28)),
                vec![tile(0)],
            ),
        ] {
            assert_eq!(context.player_id(), None);
            assert_eq!(context.oya(), None);
            assert!(context.discards().iter().all(|d| d.is_empty()));
            assert_eq!(context.reached(), &[false; 4]);
        }
    }

    #[test]
    fn from_parts_with_table_state_holds_table_state() {
        let discards = [
            vec![tile(0)],
            vec![tile(16), tile(20)],
            vec![],
            vec![tile(104)],
        ];
        let context = table_state_context(
            Some(1),
            Some(2),
            discards.clone(),
            [false, true, false, true],
        );
        assert_eq!(context.player_id(), Some(1));
        assert_eq!(context.oya(), Some(2));
        assert_eq!(context.discards(), &discards);
        assert_eq!(context.reached(), &[false, true, false, true]);
    }

    #[test]
    fn from_parts_with_table_state_keeps_other_parts() {
        let hand_tiles = vec![tile(0), tile(56)];
        let dora_indicators = vec![tile(4)];
        let visible_tiles = vec![tile(0), tile(56)];
        let context = GameContext::from_parts_with_table_state(
            Some(tile(16)),
            hand_tiles.clone(),
            dora_indicators.clone(),
            Some(wind(27)),
            Some(wind(28)),
            visible_tiles.clone(),
            Some(0),
            Some(0),
            Default::default(),
            [false; 4],
        );
        assert_eq!(context.drawn_tile(), Some(tile(16)));
        assert_eq!(context.hand_tiles(), hand_tiles.as_slice());
        assert_eq!(context.dora_indicators(), dora_indicators.as_slice());
        assert_eq!(context.round_wind(), Some(wind(27)));
        assert_eq!(context.seat_wind(), Some(wind(28)));
        assert_eq!(context.visible_tiles(), visible_tiles.as_slice());
    }

    #[test]
    fn discards_returns_reference() {
        let discards = [vec![tile(0)], vec![], vec![], vec![]];
        let context = table_state_context(None, None, discards.clone(), [false; 4]);
        let reference: &[Vec<TileId>; 4] = context.discards();
        assert_eq!(reference, &discards);
    }

    #[test]
    fn reached_returns_reference() {
        let context =
            table_state_context(None, None, Default::default(), [true, false, false, false]);
        let reference: &[bool; 4] = context.reached();
        assert_eq!(reference, &[true, false, false, false]);
    }

    #[test]
    fn discards_of_returns_player_river() {
        let discards = [vec![tile(0)], vec![tile(16), tile(20)], vec![], vec![]];
        let context = table_state_context(None, None, discards, [false; 4]);
        assert_eq!(context.discards_of(0), Some([tile(0)].as_slice()));
        assert_eq!(
            context.discards_of(1),
            Some([tile(16), tile(20)].as_slice())
        );
        assert_eq!(context.discards_of(2), Some([].as_slice()));
    }

    #[test]
    fn discards_of_out_of_range_returns_none() {
        let context = GameContext::default();
        assert_eq!(context.discards_of(4), None);
        assert_eq!(context.discards_of(usize::MAX), None);
    }

    #[test]
    fn is_reached_reports_per_player() {
        let context =
            table_state_context(None, None, Default::default(), [false, true, false, false]);
        assert!(!context.is_reached(0));
        assert!(context.is_reached(1));
    }

    #[test]
    fn is_reached_out_of_range_returns_false() {
        let context = table_state_context(None, None, Default::default(), [true; 4]);
        assert!(!context.is_reached(4));
    }

    #[test]
    fn any_opponent_reached_detects_other_players() {
        let context = table_state_context(
            Some(0),
            None,
            Default::default(),
            [false, false, true, false],
        );
        assert!(context.any_opponent_reached());
    }

    #[test]
    fn any_opponent_reached_ignores_own_reach() {
        let context = table_state_context(
            Some(0),
            None,
            Default::default(),
            [true, false, false, false],
        );
        assert!(!context.any_opponent_reached());
    }

    #[test]
    fn any_opponent_reached_without_player_id_reports_any_reach() {
        let context =
            table_state_context(None, None, Default::default(), [true, false, false, false]);
        assert!(context.any_opponent_reached());
    }

    #[test]
    fn any_opponent_reached_is_false_when_nobody_reached() {
        let context = table_state_context(Some(0), None, Default::default(), [false; 4]);
        assert!(!context.any_opponent_reached());
    }

    #[test]
    fn is_genbutsu_for_detects_discarded_tile_type() {
        let discards = [vec![tile(0)], vec![tile(16)], vec![], vec![]];
        let context = table_state_context(Some(3), None, discards, [false; 4]);
        let one_man = tile(0).tile_type();
        assert!(is_genbutsu_for(one_man, 0, &context));
        assert!(!is_genbutsu_for(one_man, 1, &context));
    }

    #[test]
    fn is_genbutsu_for_out_of_range_player_is_false() {
        let context = GameContext::default();
        assert!(!is_genbutsu_for(tile(0).tile_type(), 4, &context));
    }
}
