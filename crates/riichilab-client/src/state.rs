use bot_logic::TileType;

const PLAYER_COUNT: usize = 4;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ValidationState {
    seat_id: Option<u8>,
    last_tsumo: Option<String>,
    pending_reach: [bool; PLAYER_COUNT],
    active_reach: [bool; PLAYER_COUNT],
    post_reach_passed_tiles: [Vec<TileType>; PLAYER_COUNT],
}

impl ValidationState {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn seat_id(&self) -> Option<u8> {
        self.seat_id
    }

    pub fn last_tsumo(&self) -> Option<&str> {
        self.last_tsumo.as_deref()
    }

    pub fn is_reach_pending(&self, player: usize) -> bool {
        self.pending_reach.get(player).copied().unwrap_or(false)
    }

    pub fn is_reach_active(&self, player: usize) -> bool {
        self.active_reach.get(player).copied().unwrap_or(false)
    }

    pub fn post_reach_passed_tiles(&self) -> &[Vec<TileType>; PLAYER_COUNT] {
        &self.post_reach_passed_tiles
    }

    pub fn on_start_game(&mut self, id: u8) {
        self.seat_id = Some(id);
        self.last_tsumo = None;
        self.reset_reach_tracking();
    }

    pub fn on_start_kyoku(&mut self) {
        self.reset_reach_tracking();
    }

    pub fn on_tsumo(&mut self, actor: u8, pai: String) {
        if Some(actor) == self.seat_id && pai != "?" {
            self.last_tsumo = Some(pai);
        }
    }

    pub fn on_reach(&mut self, actor: u8) {
        if let Some(pending) = self.pending_reach.get_mut(usize::from(actor)) {
            *pending = true;
        }
    }

    pub fn on_dahai(&mut self, actor: u8, pai: &str) {
        if Some(actor) == self.seat_id {
            self.last_tsumo = None;
        }
        self.record_post_reach_passed_tile(actor, pai);
        self.establish_pending_reach(actor);
    }

    pub fn actor_or_default(&self) -> u8 {
        self.seat_id.unwrap_or(0)
    }

    fn reset_reach_tracking(&mut self) {
        self.pending_reach = [false; PLAYER_COUNT];
        self.active_reach = [false; PLAYER_COUNT];
        self.post_reach_passed_tiles = Default::default();
    }

    fn record_post_reach_passed_tile(&mut self, actor: u8, pai: &str) {
        let Ok(tile) = TileType::from_mjai_type_str(pai) else {
            return;
        };
        for player in 0..PLAYER_COUNT {
            if player == usize::from(actor) || !self.active_reach[player] {
                continue;
            }
            if !self.post_reach_passed_tiles[player].contains(&tile) {
                self.post_reach_passed_tiles[player].push(tile);
            }
        }
    }

    fn establish_pending_reach(&mut self, actor: u8) {
        let player = usize::from(actor);
        if self.is_reach_pending(player) {
            self.pending_reach[player] = false;
            self.active_reach[player] = true;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn initial_state_has_no_seat_and_no_tsumo() {
        let state = ValidationState::new();
        assert_eq!(state.seat_id(), None);
        assert_eq!(state.last_tsumo(), None);
    }

    #[test]
    fn on_start_game_sets_seat_id() {
        let mut state = ValidationState::new();
        state.on_start_game(0);
        assert_eq!(state.seat_id(), Some(0));
        assert_eq!(state.last_tsumo(), None);
    }

    #[test]
    fn on_start_game_clears_last_tsumo() {
        let mut state = ValidationState::new();
        state.on_start_game(1);
        state.on_tsumo(1, "6p".to_string());
        assert_eq!(state.last_tsumo(), Some("6p"));
        state.on_start_game(2);
        assert_eq!(state.seat_id(), Some(2));
        assert_eq!(state.last_tsumo(), None);
    }

    #[test]
    fn own_tsumo_is_recorded() {
        let mut state = ValidationState::new();
        state.on_start_game(0);
        state.on_tsumo(0, "6p".to_string());
        assert_eq!(state.last_tsumo(), Some("6p"));
    }

    #[test]
    fn other_players_tsumo_is_ignored() {
        let mut state = ValidationState::new();
        state.on_start_game(0);
        state.on_tsumo(1, "6p".to_string());
        assert_eq!(state.last_tsumo(), None);
    }

    #[test]
    fn tsumo_before_start_game_is_ignored() {
        let mut state = ValidationState::new();
        state.on_tsumo(0, "6p".to_string());
        assert_eq!(state.last_tsumo(), None);
    }

    #[test]
    fn hidden_tsumo_pai_is_ignored() {
        let mut state = ValidationState::new();
        state.on_start_game(0);
        state.on_tsumo(0, "?".to_string());
        assert_eq!(state.last_tsumo(), None);
    }

    #[test]
    fn own_dahai_clears_last_tsumo() {
        let mut state = ValidationState::new();
        state.on_start_game(0);
        state.on_tsumo(0, "6p".to_string());
        state.on_dahai(0, "6p");
        assert_eq!(state.last_tsumo(), None);
    }

    #[test]
    fn other_players_dahai_does_not_clear_last_tsumo() {
        let mut state = ValidationState::new();
        state.on_start_game(0);
        state.on_tsumo(0, "6p".to_string());
        state.on_dahai(1, "6p");
        assert_eq!(state.last_tsumo(), Some("6p"));
    }

    #[test]
    fn actor_or_default_falls_back_to_zero() {
        let mut state = ValidationState::new();
        assert_eq!(state.actor_or_default(), 0);
        state.on_start_game(3);
        assert_eq!(state.actor_or_default(), 3);
    }

    mod reach_tracking {
        use super::*;

        fn tile(mjai: &str) -> TileType {
            TileType::from_mjai_type_str(mjai).unwrap()
        }

        fn passed(state: &ValidationState, player: usize) -> Vec<String> {
            state.post_reach_passed_tiles()[player]
                .iter()
                .map(|tile| tile.to_mjai_string())
                .collect()
        }

        fn started() -> ValidationState {
            let mut state = ValidationState::new();
            state.on_start_game(0);
            state.on_start_kyoku();
            state
        }

        #[test]
        fn initial_state_has_no_reach_and_no_passed_tiles() {
            let state = ValidationState::new();
            for player in 0..4 {
                assert!(!state.is_reach_pending(player));
                assert!(!state.is_reach_active(player));
                assert!(passed(&state, player).is_empty());
            }
        }

        #[test]
        fn reach_alone_is_pending_and_not_active() {
            let mut state = started();
            state.on_reach(1);
            assert!(state.is_reach_pending(1));
            assert!(!state.is_reach_active(1));
        }

        #[test]
        fn declaration_dahai_makes_the_reach_active() {
            let mut state = started();
            state.on_reach(1);
            state.on_dahai(1, "3p");
            assert!(!state.is_reach_pending(1));
            assert!(state.is_reach_active(1));
        }

        #[test]
        fn declaration_tile_is_not_recorded_as_passed_for_the_declarer() {
            let mut state = started();
            state.on_reach(1);
            state.on_dahai(1, "3p");
            assert!(passed(&state, 1).is_empty());
        }

        #[test]
        fn tile_discarded_after_a_single_reach_is_recorded() {
            let mut state = started();
            state.on_reach(1);
            state.on_dahai(1, "3p");
            state.on_dahai(2, "4s");
            assert_eq!(passed(&state, 1), ["4s"]);
        }

        #[test]
        fn tile_discarded_before_the_reach_is_not_recorded() {
            let mut state = started();
            state.on_dahai(2, "4s");
            state.on_reach(1);
            state.on_dahai(1, "3p");
            assert!(passed(&state, 1).is_empty());
        }

        #[test]
        fn declaration_tile_of_a_later_reach_is_recorded_for_the_earlier_reacher() {
            let mut state = started();
            state.on_reach(1);
            state.on_dahai(1, "3p");
            state.on_reach(2);
            state.on_dahai(2, "4s");
            assert_eq!(passed(&state, 1), ["4s"]);
            assert!(passed(&state, 2).is_empty());
            assert!(state.is_reach_active(1));
            assert!(state.is_reach_active(2));
        }

        #[test]
        fn each_reacher_only_collects_tiles_passed_after_its_own_reach() {
            let mut state = started();
            state.on_reach(1);
            state.on_dahai(1, "3p");
            state.on_dahai(3, "7s");
            state.on_reach(2);
            state.on_dahai(2, "6p");
            state.on_dahai(3, "1m");
            assert_eq!(passed(&state, 1), ["7s", "6p", "1m"]);
            assert_eq!(passed(&state, 2), ["1m"]);
        }

        #[test]
        fn three_reachers_share_a_tile_passed_after_all_of_them() {
            let mut state = started();
            for (actor, declaration) in [(1, "3p"), (2, "6p"), (3, "9m")] {
                state.on_reach(actor);
                state.on_dahai(actor, declaration);
            }
            state.on_dahai(0, "4s");
            for player in [1, 2, 3] {
                assert!(passed(&state, player).contains(&"4s".to_string()));
            }
        }

        #[test]
        fn red_five_is_recorded_as_the_same_tile_type_as_the_black_five() {
            let mut state = started();
            state.on_reach(1);
            state.on_dahai(1, "3p");
            state.on_dahai(2, "5sr");
            assert_eq!(state.post_reach_passed_tiles()[1], [tile("5s")]);
        }

        #[test]
        fn the_same_tile_type_is_recorded_once() {
            let mut state = started();
            state.on_reach(1);
            state.on_dahai(1, "3p");
            state.on_dahai(2, "5s");
            state.on_dahai(3, "5sr");
            assert_eq!(passed(&state, 1), ["5s"]);
        }

        #[test]
        fn hidden_dahai_pai_is_ignored() {
            let mut state = started();
            state.on_reach(1);
            state.on_dahai(1, "3p");
            state.on_dahai(2, "?");
            assert!(passed(&state, 1).is_empty());
        }

        #[test]
        fn start_kyoku_clears_reach_tracking() {
            let mut state = started();
            state.on_reach(1);
            state.on_dahai(1, "3p");
            state.on_dahai(2, "4s");

            state.on_start_kyoku();

            assert!(!state.is_reach_active(1));
            assert!(!state.is_reach_pending(1));
            for player in 0..4 {
                assert!(passed(&state, player).is_empty());
            }
        }

        #[test]
        fn start_kyoku_clears_a_pending_reach_too() {
            let mut state = started();
            state.on_reach(1);
            state.on_start_kyoku();
            assert!(!state.is_reach_pending(1));
        }

        #[test]
        fn start_kyoku_keeps_the_seat_id() {
            let mut state = started();
            state.on_start_kyoku();
            assert_eq!(state.seat_id(), Some(0));
        }

        #[test]
        fn start_game_clears_reach_tracking() {
            let mut state = started();
            state.on_reach(1);
            state.on_dahai(1, "3p");
            state.on_dahai(2, "4s");

            state.on_start_game(1);

            assert!(!state.is_reach_active(1));
            for player in 0..4 {
                assert!(passed(&state, player).is_empty());
            }
        }

        #[test]
        fn out_of_range_actor_is_ignored() {
            let mut state = started();
            state.on_reach(4);
            state.on_dahai(4, "4s");
            for player in 0..4 {
                assert!(!state.is_reach_pending(player));
                assert!(!state.is_reach_active(player));
                assert!(passed(&state, player).is_empty());
            }
        }

        #[test]
        fn own_reach_is_tracked_like_any_other_seat() {
            let mut state = started();
            state.on_reach(0);
            state.on_dahai(0, "3p");
            state.on_dahai(1, "4s");
            assert_eq!(passed(&state, 0), ["4s"]);
        }
    }
}
