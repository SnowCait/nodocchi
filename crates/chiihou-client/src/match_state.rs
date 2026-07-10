use nostr_sdk::PublicKey;

use crate::lifecycle::{ChiihouLifecycleNotification, ChiihouPlayerScore, ChiihouWind};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ChiihouMatchPhase {
    #[default]
    Idle,
    GameStarted,
    InKyoku,
    WaitingNext,
    Ended,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ChiihouMatchState {
    players: Vec<PublicKey>,
    seat: Option<ChiihouWind>,
    round_wind: Option<ChiihouWind>,
    dealer: Option<PublicKey>,
    honba: u32,
    kyotaku_points: u32,
    phase: ChiihouMatchPhase,
    final_scores: Vec<ChiihouPlayerScore>,
}

impl ChiihouMatchState {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn players(&self) -> &[PublicKey] {
        &self.players
    }

    pub fn seat(&self) -> Option<ChiihouWind> {
        self.seat
    }

    pub fn round_wind(&self) -> Option<ChiihouWind> {
        self.round_wind
    }

    pub fn dealer(&self) -> Option<PublicKey> {
        self.dealer
    }

    pub fn honba(&self) -> u32 {
        self.honba
    }

    pub fn kyotaku_points(&self) -> u32 {
        self.kyotaku_points
    }

    pub fn phase(&self) -> ChiihouMatchPhase {
        self.phase
    }

    pub fn final_scores(&self) -> &[ChiihouPlayerScore] {
        &self.final_scores
    }

    pub fn apply(&mut self, notification: &ChiihouLifecycleNotification) {
        match notification {
            ChiihouLifecycleNotification::GameStart { seat, players } => {
                self.players = players.clone();
                self.seat = Some(*seat);
                self.round_wind = None;
                self.dealer = None;
                self.honba = 0;
                self.kyotaku_points = 0;
                self.final_scores.clear();
                self.phase = ChiihouMatchPhase::GameStarted;
            }
            ChiihouLifecycleNotification::KyokuStart {
                round_wind,
                dealer,
                honba,
                kyotaku_points,
            } => {
                self.round_wind = Some(*round_wind);
                self.dealer = Some(*dealer);
                self.honba = *honba;
                self.kyotaku_points = *kyotaku_points;
                self.phase = ChiihouMatchPhase::InKyoku;
            }
            ChiihouLifecycleNotification::KyokuEnd => {
                self.phase = ChiihouMatchPhase::WaitingNext;
            }
            ChiihouLifecycleNotification::GameEnd { scores } => {
                self.final_scores = scores.clone();
                self.phase = ChiihouMatchPhase::Ended;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nostr_sdk::Keys;

    // テスト専用の秘密鍵から鍵を導出する。実際の運用で使用してはならない。
    fn test_keys(index: u64) -> Keys {
        Keys::parse(&format!("{index:064x}")).unwrap()
    }

    fn player_pubkey(index: u64) -> PublicKey {
        test_keys(index).public_key()
    }

    fn players(indexes: impl IntoIterator<Item = u64>) -> Vec<PublicKey> {
        indexes.into_iter().map(player_pubkey).collect()
    }

    fn gamestart() -> ChiihouLifecycleNotification {
        ChiihouLifecycleNotification::GameStart {
            seat: ChiihouWind::South,
            players: players(1..=4),
        }
    }

    fn kyokustart(honba: u32, kyotaku_points: u32) -> ChiihouLifecycleNotification {
        ChiihouLifecycleNotification::KyokuStart {
            round_wind: ChiihouWind::East,
            dealer: player_pubkey(2),
            honba,
            kyotaku_points,
        }
    }

    fn gameend() -> ChiihouLifecycleNotification {
        ChiihouLifecycleNotification::GameEnd {
            scores: vec![
                ChiihouPlayerScore {
                    player: player_pubkey(1),
                    score: 45000,
                },
                ChiihouPlayerScore {
                    player: player_pubkey(2),
                    score: 30000,
                },
                ChiihouPlayerScore {
                    player: player_pubkey(3),
                    score: 26000,
                },
                ChiihouPlayerScore {
                    player: player_pubkey(4),
                    score: -1000,
                },
            ],
        }
    }

    #[test]
    fn initial_state_is_idle_and_empty() {
        let state = ChiihouMatchState::new();
        assert_eq!(state.phase(), ChiihouMatchPhase::Idle);
        assert!(state.players().is_empty());
        assert_eq!(state.seat(), None);
        assert_eq!(state.round_wind(), None);
        assert_eq!(state.dealer(), None);
        assert_eq!(state.honba(), 0);
        assert_eq!(state.kyotaku_points(), 0);
        assert!(state.final_scores().is_empty());
    }

    #[test]
    fn gamestart_sets_players_seat_and_phase() {
        let mut state = ChiihouMatchState::new();
        state.apply(&gamestart());
        assert_eq!(state.phase(), ChiihouMatchPhase::GameStarted);
        assert_eq!(state.players(), players(1..=4));
        assert_eq!(state.seat(), Some(ChiihouWind::South));
        assert_eq!(state.round_wind(), None);
        assert_eq!(state.dealer(), None);
    }

    #[test]
    fn kyokustart_sets_kyoku_fields_and_phase() {
        let mut state = ChiihouMatchState::new();
        state.apply(&gamestart());
        state.apply(&kyokustart(1, 2000));
        assert_eq!(state.phase(), ChiihouMatchPhase::InKyoku);
        assert_eq!(state.round_wind(), Some(ChiihouWind::East));
        assert_eq!(state.dealer(), Some(player_pubkey(2)));
        assert_eq!(state.honba(), 1);
        assert_eq!(state.kyotaku_points(), 2000);
        assert_eq!(state.players(), players(1..=4));
        assert_eq!(state.seat(), Some(ChiihouWind::South));
    }

    #[test]
    fn kyokuend_moves_to_waiting_next() {
        let mut state = ChiihouMatchState::new();
        state.apply(&gamestart());
        state.apply(&kyokustart(0, 0));
        state.apply(&ChiihouLifecycleNotification::KyokuEnd);
        assert_eq!(state.phase(), ChiihouMatchPhase::WaitingNext);
        assert_eq!(state.players(), players(1..=4));
        assert_eq!(state.round_wind(), Some(ChiihouWind::East));
        assert_eq!(state.dealer(), Some(player_pubkey(2)));
    }

    #[test]
    fn next_kyokustart_after_kyokuend_reenters_kyoku() {
        let mut state = ChiihouMatchState::new();
        state.apply(&gamestart());
        state.apply(&kyokustart(0, 0));
        state.apply(&ChiihouLifecycleNotification::KyokuEnd);
        state.apply(&kyokustart(1, 1000));
        assert_eq!(state.phase(), ChiihouMatchPhase::InKyoku);
        assert_eq!(state.honba(), 1);
        assert_eq!(state.kyotaku_points(), 1000);
    }

    #[test]
    fn gameend_sets_final_scores_and_phase() {
        let mut state = ChiihouMatchState::new();
        state.apply(&gamestart());
        state.apply(&kyokustart(0, 0));
        state.apply(&ChiihouLifecycleNotification::KyokuEnd);
        state.apply(&gameend());
        assert_eq!(state.phase(), ChiihouMatchPhase::Ended);
        let ChiihouLifecycleNotification::GameEnd { scores } = gameend() else {
            unreachable!();
        };
        assert_eq!(state.final_scores(), scores);
    }

    #[test]
    fn new_gamestart_resets_previous_match() {
        let mut state = ChiihouMatchState::new();
        state.apply(&gamestart());
        state.apply(&kyokustart(2, 3000));
        state.apply(&ChiihouLifecycleNotification::KyokuEnd);
        state.apply(&gameend());
        let next_gamestart = ChiihouLifecycleNotification::GameStart {
            seat: ChiihouWind::West,
            players: players([5, 6, 7, 8]),
        };
        state.apply(&next_gamestart);
        assert_eq!(state.phase(), ChiihouMatchPhase::GameStarted);
        assert_eq!(state.players(), players([5, 6, 7, 8]));
        assert_eq!(state.seat(), Some(ChiihouWind::West));
        assert_eq!(state.round_wind(), None);
        assert_eq!(state.dealer(), None);
        assert_eq!(state.honba(), 0);
        assert_eq!(state.kyotaku_points(), 0);
        assert!(state.final_scores().is_empty());
    }

    #[test]
    fn reapplying_same_notification_keeps_state() {
        let notifications = [
            gamestart(),
            kyokustart(1, 1000),
            ChiihouLifecycleNotification::KyokuEnd,
            gameend(),
        ];
        let mut state = ChiihouMatchState::new();
        for notification in &notifications {
            state.apply(notification);
            let after_first = state.clone();
            state.apply(notification);
            assert_eq!(state, after_first, "notification: {notification:?}");
        }
    }
}
