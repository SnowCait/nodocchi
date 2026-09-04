use bot_logic::{HistoryFuritenFacts, TileType};

const PLAYER_COUNT: usize = 4;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ValidationState {
    seat_id: Option<u8>,
    last_tsumo: Option<String>,
    pending_reach: [bool; PLAYER_COUNT],
    active_reach: [bool; PLAYER_COUNT],
    post_reach_passed_tiles: [Vec<TileType>; PLAYER_COUNT],
    temporary_passed_tiles: Option<[Vec<TileType>; PLAYER_COUNT]>,
    same_hand_passed_tiles: Option<[Vec<TileType>; PLAYER_COUNT]>,
    pending_passed_tile: Option<(u8, TileType)>,
    history_furiten: HistoryFuritenFacts,
    own_tsumo_pending_discard: bool,
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

    pub fn temporary_passed_tiles(&self) -> Option<&[Vec<TileType>; PLAYER_COUNT]> {
        self.temporary_passed_tiles.as_ref()
    }

    pub fn same_hand_passed_tiles(&self) -> Option<&[Vec<TileType>; PLAYER_COUNT]> {
        self.same_hand_passed_tiles.as_ref()
    }

    pub fn history_furiten(&self) -> HistoryFuritenFacts {
        self.history_furiten
    }

    /// 現在 response を待っている直前の打牌者。
    ///
    /// `pending_passed_tile` が無い局面では reaction 元を推測せず `None` を返す。
    pub fn reaction_source_player(&self) -> Option<u8> {
        self.pending_passed_tile.map(|(actor, _)| actor)
    }

    pub fn on_start_game(&mut self, id: u8) {
        self.seat_id = Some(id);
        self.last_tsumo = None;
        self.reset_reach_tracking();
        self.history_furiten = HistoryFuritenFacts::default();
        self.own_tsumo_pending_discard = false;
    }

    pub fn on_start_kyoku(&mut self) {
        self.reset_reach_tracking();
        self.start_passed_tracking();
        self.last_tsumo = None;
        self.history_furiten = HistoryFuritenFacts {
            same_turn: Some(false),
            riichi_missed_win: Some(false),
        };
        self.own_tsumo_pending_discard = false;
    }

    pub fn on_tsumo(&mut self, actor: u8, pai: String) {
        self.confirm_pending_passed_tile();
        self.clear_temporary_passed_tiles(actor);
        if Some(actor) == self.seat_id {
            self.own_tsumo_pending_discard = true;
            if pai != "?" {
                self.last_tsumo = Some(pai);
            }
        }
    }

    pub fn on_reach(&mut self, actor: u8) {
        if let Some(pending) = self.pending_reach.get_mut(usize::from(actor)) {
            *pending = true;
        }
    }

    pub fn on_dahai(&mut self, actor: u8, pai: &str) {
        self.on_dahai_with_tsumogiri(actor, pai, None);
    }

    pub fn on_dahai_with_tsumogiri(&mut self, actor: u8, pai: &str, tsumogiri: Option<bool>) {
        if usize::from(actor) >= PLAYER_COUNT {
            return;
        }
        if Some(actor) == self.seat_id {
            if self.own_tsumo_pending_discard {
                self.history_furiten.same_turn = Some(false);
            }
            self.own_tsumo_pending_discard = false;
            self.last_tsumo = None;
        }
        self.confirm_pending_passed_tile();
        if tsumogiri != Some(true) {
            self.clear_same_hand_passed_tiles(actor);
        }
        self.record_post_reach_passed_tile(actor, pai);
        self.establish_pending_reach(actor);
        self.set_pending_passed_tile(actor, pai);
    }

    /// 鳴き・槓による手牌変化。直前のロン機会は見逃されて局が継続したと確定してから、
    /// actor の旧手牌に対する一時安全情報を消す。
    pub fn on_hand_change(&mut self, actor: u8) {
        self.confirm_pending_passed_tile();
        self.clear_temporary_passed_tiles(actor);
        self.clear_same_hand_passed_tiles(actor);
    }

    /// 加槓による手牌変化を反映し、加槓牌を新しいロン機会として pending にする。
    pub fn on_kakan(&mut self, actor: u8, pai: &str) {
        if usize::from(actor) >= PLAYER_COUNT {
            return;
        }
        self.on_hand_change(actor);
        self.set_pending_passed_tile(actor, pai);
    }

    /// 和了イベントでは直前のロン機会が「通った」とは言えないため pending を破棄する。
    pub fn on_hora(&mut self) {
        self.pending_passed_tile = None;
    }

    /// legal Hora を選ばなかった claim decision を履歴依存フリテンとして記録する。
    ///
    /// 自摸直後の Hora はツモ和了なので対象外。新局開始を観測できず state が unknown の場合も、
    /// リーチ状態を false と推測して記録しない。
    pub fn on_action_response(&mut self, ron_legal: bool, chose_hora: bool) {
        if !ron_legal || chose_hora || self.own_tsumo_pending_discard {
            return;
        }
        if self.history_furiten.same_turn.is_none()
            || self.history_furiten.riichi_missed_win.is_none()
        {
            return;
        }
        let Some(player) = self.seat_id.map(usize::from) else {
            return;
        };
        if self.is_reach_active(player) {
            self.history_furiten.same_turn = Some(true);
            self.history_furiten.riichi_missed_win = Some(true);
        } else {
            self.history_furiten.same_turn = Some(true);
        }
    }

    pub fn actor_or_default(&self) -> u8 {
        self.seat_id.unwrap_or(0)
    }

    fn reset_reach_tracking(&mut self) {
        self.pending_reach = [false; PLAYER_COUNT];
        self.active_reach = [false; PLAYER_COUNT];
        self.post_reach_passed_tiles = Default::default();
        self.temporary_passed_tiles = None;
        self.same_hand_passed_tiles = None;
        self.pending_passed_tile = None;
    }

    fn start_passed_tracking(&mut self) {
        self.temporary_passed_tiles = Some(Default::default());
        self.same_hand_passed_tiles = Some(Default::default());
        self.pending_passed_tile = None;
    }

    fn confirm_pending_passed_tile(&mut self) {
        let Some((actor, tile)) = self.pending_passed_tile.take() else {
            return;
        };
        for passed in [
            self.temporary_passed_tiles.as_mut(),
            self.same_hand_passed_tiles.as_mut(),
        ]
        .into_iter()
        .flatten()
        {
            for (player, tiles) in passed.iter_mut().enumerate() {
                if player != usize::from(actor) && !tiles.contains(&tile) {
                    tiles.push(tile);
                }
            }
        }
    }

    fn set_pending_passed_tile(&mut self, actor: u8, pai: &str) {
        self.pending_passed_tile = TileType::from_mjai_type_str(pai)
            .ok()
            .map(|tile| (actor, tile));
    }

    fn clear_temporary_passed_tiles(&mut self, actor: u8) {
        if let Some(tiles) = self
            .temporary_passed_tiles
            .as_mut()
            .and_then(|passed| passed.get_mut(usize::from(actor)))
        {
            tiles.clear();
        }
    }

    fn clear_same_hand_passed_tiles(&mut self, actor: u8) {
        if let Some(tiles) = self
            .same_hand_passed_tiles
            .as_mut()
            .and_then(|passed| passed.get_mut(usize::from(actor)))
        {
            tiles.clear();
        }
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

    mod history_furiten {
        use super::*;

        fn started() -> ValidationState {
            let mut state = ValidationState::new();
            state.on_start_game(0);
            state.on_start_kyoku();
            state
        }

        #[test]
        fn is_unknown_until_a_kyoku_start_is_observed() {
            let state = ValidationState::new();
            assert_eq!(state.history_furiten(), HistoryFuritenFacts::default());
        }

        #[test]
        fn non_riichi_legal_ron_pass_sets_same_turn_only() {
            let mut state = started();
            state.on_action_response(true, false);
            assert_eq!(
                state.history_furiten(),
                HistoryFuritenFacts {
                    same_turn: Some(true),
                    riichi_missed_win: Some(false),
                }
            );
        }

        #[test]
        fn no_legal_ron_or_choosing_ron_does_not_set_furiten() {
            for (ron_legal, chose_hora) in [(false, false), (true, true)] {
                let mut state = started();
                state.on_action_response(ron_legal, chose_hora);
                assert_eq!(
                    state.history_furiten(),
                    HistoryFuritenFacts {
                        same_turn: Some(false),
                        riichi_missed_win: Some(false),
                    }
                );
            }
        }

        #[test]
        fn own_tsumo_then_dahai_clears_same_turn() {
            let mut state = started();
            state.on_action_response(true, false);
            state.on_tsumo(0, "?".to_string());
            assert_eq!(state.history_furiten().same_turn, Some(true));
            state.on_dahai(0, "1m");
            assert_eq!(state.history_furiten().same_turn, Some(false));
        }

        #[test]
        fn own_dahai_without_own_tsumo_does_not_clear_same_turn() {
            let mut state = started();
            state.on_action_response(true, false);
            state.on_dahai(0, "1m");
            assert_eq!(state.history_furiten().same_turn, Some(true));
        }

        #[test]
        fn passing_tsumo_hora_does_not_set_missed_ron() {
            let mut state = started();
            state.on_tsumo(0, "1m".to_string());
            state.on_action_response(true, false);
            assert_eq!(
                state.history_furiten(),
                HistoryFuritenFacts {
                    same_turn: Some(false),
                    riichi_missed_win: Some(false),
                }
            );
        }

        #[test]
        fn riichi_legal_ron_pass_sets_persistent_state() {
            let mut state = started();
            state.on_reach(0);
            state.on_dahai(0, "1m");
            state.on_action_response(true, false);
            assert_eq!(state.history_furiten().same_turn, Some(true));
            assert_eq!(state.history_furiten().riichi_missed_win, Some(true));

            state.on_tsumo(0, "2m".to_string());
            state.on_dahai(0, "2m");
            assert_eq!(state.history_furiten().same_turn, Some(false));
            assert_eq!(state.history_furiten().riichi_missed_win, Some(true));
        }

        #[test]
        fn start_kyoku_resets_both_to_known_false() {
            let mut state = started();
            state.on_action_response(true, false);
            state.on_reach(0);
            state.on_dahai(0, "1m");
            state.on_action_response(true, false);
            state.on_start_kyoku();
            assert_eq!(
                state.history_furiten(),
                HistoryFuritenFacts {
                    same_turn: Some(false),
                    riichi_missed_win: Some(false),
                }
            );
        }
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
        fn out_of_range_actor_dahai_is_not_recorded_for_active_reachers() {
            let mut state = started();
            state.on_tsumo(0, "6p".to_string());
            state.on_reach(1);
            state.on_dahai(1, "3p");
            state.on_reach(2);
            assert!(state.is_reach_active(1));
            assert!(passed(&state, 1).is_empty());

            let before = state.clone();
            state.on_dahai(4, "4s");

            assert!(passed(&state, 1).is_empty());
            assert!(state.is_reach_active(1));
            assert!(state.is_reach_pending(2));
            assert!(!state.is_reach_active(2));
            assert_eq!(state.last_tsumo(), Some("6p"));
            assert_eq!(state, before);
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

    mod temporary_passed_tracking {
        use super::*;

        fn tile(mjai: &str) -> TileType {
            TileType::from_mjai_type_str(mjai).unwrap()
        }

        fn started() -> ValidationState {
            let mut state = ValidationState::new();
            state.on_start_game(1);
            state.on_start_kyoku();
            state
        }

        fn is_passed(state: &ValidationState, player: usize, mjai: &str) -> bool {
            state
                .temporary_passed_tiles()
                .is_some_and(|passed| passed[player].contains(&tile(mjai)))
        }

        #[test]
        fn dahai_is_not_safe_until_a_continuation_event_confirms_it_passed() {
            let mut state = started();
            state.on_dahai(0, "9m");
            assert!((0..4).all(|player| !is_passed(&state, player, "9m")));

            state.on_tsumo(1, "8s".to_string());
            assert!(!is_passed(&state, 0, "9m"));
            assert!(!is_passed(&state, 1, "9m"));
            assert!(is_passed(&state, 2, "9m"));
            assert!(is_passed(&state, 3, "9m"));
        }

        #[test]
        fn kakan_is_confirmed_for_other_players_when_the_hand_continues() {
            let mut state = started();
            state.on_kakan(0, "5m");
            assert!((0..4).all(|player| !is_passed(&state, player, "5m")));

            state.on_tsumo(0, "?".to_string());
            assert!(!is_passed(&state, 0, "5m"));
            assert!((1..4).all(|player| is_passed(&state, player, "5m")));
        }

        #[test]
        fn hora_does_not_confirm_the_kakan_tile_as_passed() {
            let mut state = started();
            state.on_kakan(0, "5m");
            state.on_hora();
            state.on_tsumo(0, "?".to_string());

            assert!((0..4).all(|player| !is_passed(&state, player, "5m")));
        }

        #[test]
        fn target_draw_clears_a_confirmed_kakan_tile() {
            let mut state = started();
            state.on_kakan(0, "5m");
            state.on_tsumo(0, "?".to_string());
            assert!(is_passed(&state, 2, "5m"));

            state.on_tsumo(2, "?".to_string());
            assert!(!is_passed(&state, 2, "5m"));
        }

        #[test]
        fn next_tsumo_clears_only_the_drawing_players_previous_safety() {
            let mut state = started();
            state.on_dahai(0, "9m");
            state.on_tsumo(2, "?".to_string());
            assert!(!is_passed(&state, 2, "9m"));
            assert!(is_passed(&state, 3, "9m"));
        }

        #[test]
        fn next_tsumo_clears_temporary_safety_but_keeps_post_reach_safety() {
            let mut state = started();
            state.on_reach(0);
            state.on_dahai(0, "3p");
            state.on_dahai(1, "9m");
            state.on_tsumo(3, "?".to_string());

            assert!(state.post_reach_passed_tiles()[0].contains(&tile("9m")));
            assert!(!is_passed(&state, 3, "9m"));
            assert!(is_passed(&state, 2, "9m"));
        }

        #[test]
        fn every_meld_kind_clears_the_callers_previous_safety() {
            for actor in 0..4 {
                let mut state = started();
                state.on_dahai((actor + 1) % 4, "9m");
                state.on_hand_change(actor);
                assert!(!is_passed(&state, usize::from(actor), "9m"));
            }
        }

        #[test]
        fn red_and_black_five_are_the_same_temporary_tile_type() {
            let mut state = started();
            state.on_dahai(0, "5sr");
            state.on_tsumo(1, "?".to_string());
            assert!(is_passed(&state, 3, "5s"));
        }

        #[test]
        fn hora_does_not_confirm_the_winning_discard_as_passed() {
            let mut state = started();
            state.on_dahai(0, "9m");
            state.on_hora();
            state.on_tsumo(1, "?".to_string());
            assert!((0..4).all(|player| !is_passed(&state, player, "9m")));
        }

        #[test]
        fn new_round_resets_known_temporary_safety_to_empty() {
            let mut state = started();
            state.on_dahai(0, "9m");
            state.on_tsumo(1, "?".to_string());
            assert!(is_passed(&state, 3, "9m"));

            state.on_start_kyoku();
            let passed = state.temporary_passed_tiles().unwrap();
            assert!(passed.iter().all(Vec::is_empty));
        }

        #[test]
        fn tracking_is_unknown_until_start_kyoku_is_observed() {
            let mut state = ValidationState::new();
            state.on_start_game(0);
            assert_eq!(state.temporary_passed_tiles(), None);
        }
    }

    mod same_hand_passed_tracking {
        use super::*;

        fn tile(mjai: &str) -> TileType {
            TileType::from_mjai_type_str(mjai).unwrap()
        }

        fn started() -> ValidationState {
            let mut state = ValidationState::new();
            state.on_start_game(0);
            state.on_start_kyoku();
            state
        }

        fn is_temporary_passed(state: &ValidationState, player: usize, mjai: &str) -> bool {
            state
                .temporary_passed_tiles()
                .is_some_and(|passed| passed[player].contains(&tile(mjai)))
        }

        fn is_same_hand_passed(state: &ValidationState, player: usize, mjai: &str) -> bool {
            state
                .same_hand_passed_tiles()
                .is_some_and(|passed| passed[player].contains(&tile(mjai)))
        }

        fn confirm_nine_man_against(state: &mut ValidationState, player: u8) {
            state.on_dahai_with_tsumogiri((player + 1) % 4, "9m", Some(false));
            state.on_tsumo(player, "?".to_string());
        }

        #[test]
        fn confirmed_tile_enters_temporary_and_same_hand_history() {
            let mut state = started();
            state.on_dahai_with_tsumogiri(0, "9m", Some(false));
            state.on_tsumo(1, "?".to_string());

            assert!(is_temporary_passed(&state, 2, "9m"));
            assert!(is_same_hand_passed(&state, 2, "9m"));
        }

        #[test]
        fn confirmed_kakan_enters_same_hand_history_for_other_players() {
            let mut state = started();
            state.on_kakan(0, "5m");
            state.on_tsumo(0, "?".to_string());

            assert!(!is_same_hand_passed(&state, 0, "5m"));
            assert!((1..4).all(|player| is_same_hand_passed(&state, player, "5m")));
        }

        #[test]
        fn draw_clears_only_temporary_history_for_the_drawing_player() {
            let mut state = started();
            confirm_nine_man_against(&mut state, 2);

            assert!(!is_temporary_passed(&state, 2, "9m"));
            assert!(is_same_hand_passed(&state, 2, "9m"));
        }

        #[test]
        fn consecutive_tsumogiri_keeps_same_hand_history() {
            let mut state = started();
            confirm_nine_man_against(&mut state, 2);

            for pai in ["1p", "2p"] {
                state.on_dahai_with_tsumogiri(2, pai, Some(true));
                for actor in [3, 0, 1] {
                    state.on_tsumo(actor, "?".to_string());
                    state.on_dahai_with_tsumogiri(actor, "1m", Some(true));
                }
                state.on_tsumo(2, "?".to_string());
            }

            assert!(is_same_hand_passed(&state, 2, "9m"));
        }

        #[test]
        fn tedashi_and_unknown_tsumogiri_clear_same_hand_history() {
            for tsumogiri in [Some(false), None] {
                let mut state = started();
                confirm_nine_man_against(&mut state, 2);

                state.on_dahai_with_tsumogiri(2, "1p", tsumogiri);

                assert!(!is_same_hand_passed(&state, 2, "9m"));
            }
        }

        #[test]
        fn calls_and_kans_clear_same_hand_history() {
            for actor in 0..4 {
                let mut state = started();
                confirm_nine_man_against(&mut state, actor);

                state.on_hand_change(actor);

                assert!(!is_same_hand_passed(&state, usize::from(actor), "9m"));
            }
        }

        #[test]
        fn another_players_hand_change_keeps_same_hand_history() {
            let mut state = started();
            confirm_nine_man_against(&mut state, 2);

            state.on_hand_change(3);
            state.on_dahai_with_tsumogiri(3, "1p", Some(false));

            assert!(is_same_hand_passed(&state, 2, "9m"));
        }

        #[test]
        fn tracking_is_unknown_until_start_kyoku_is_observed() {
            let mut state = ValidationState::new();
            state.on_start_game(0);
            assert_eq!(state.same_hand_passed_tiles(), None);
        }
    }
}
