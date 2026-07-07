#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ValidationState {
    seat_id: Option<u8>,
    last_tsumo: Option<String>,
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

    pub fn on_start_game(&mut self, id: u8) {
        self.seat_id = Some(id);
        self.last_tsumo = None;
    }

    pub fn on_tsumo(&mut self, actor: u8, pai: String) {
        if Some(actor) == self.seat_id && pai != "?" {
            self.last_tsumo = Some(pai);
        }
    }

    pub fn on_dahai(&mut self, actor: u8) {
        if Some(actor) == self.seat_id {
            self.last_tsumo = None;
        }
    }

    pub fn actor_or_default(&self) -> u8 {
        self.seat_id.unwrap_or(0)
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
        state.on_dahai(0);
        assert_eq!(state.last_tsumo(), None);
    }

    #[test]
    fn other_players_dahai_does_not_clear_last_tsumo() {
        let mut state = ValidationState::new();
        state.on_start_game(0);
        state.on_tsumo(0, "6p".to_string());
        state.on_dahai(1);
        assert_eq!(state.last_tsumo(), Some("6p"));
    }

    #[test]
    fn actor_or_default_falls_back_to_zero() {
        let mut state = ValidationState::new();
        assert_eq!(state.actor_or_default(), 0);
        state.on_start_game(3);
        assert_eq!(state.actor_or_default(), 3);
    }
}
