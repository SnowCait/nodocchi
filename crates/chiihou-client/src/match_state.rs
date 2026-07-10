use nostr_sdk::PublicKey;
use thiserror::Error;

use crate::lifecycle::{
    CHIIHOU_PLAYER_COUNT, ChiihouLifecycleNotification, ChiihouPlayerScore, ChiihouWind,
};
use crate::protocol::ChiihouPai;
use crate::table_notification::{ChiihouSayAction, ChiihouTableNotification};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ChiihouMatchPhase {
    #[default]
    Idle,
    GameStarted,
    InKyoku,
    WaitingNext,
    Ended,
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum ChiihouTableStateError {
    #[error("player is not in the player list")]
    UnknownPlayer,
    #[error("haipai player is not the AI itself")]
    HaipaiForOtherPlayer,
    #[error("tsumo player is not the AI itself")]
    TsumoForOtherPlayer,
    #[error("sutehai tile {0} is not in the held hand")]
    SutehaiTileNotHeld(ChiihouPai),
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ChiihouTableSnapshot {
    pub dora_indicators: Vec<ChiihouPai>,
    pub round_wind: Option<ChiihouWind>,
    pub seat_wind: Option<ChiihouWind>,
    pub player_id: Option<u8>,
    pub oya: Option<u8>,
    pub discards: [Vec<ChiihouPai>; CHIIHOU_PLAYER_COUNT],
    pub reached: [bool; CHIIHOU_PLAYER_COUNT],
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
    hand: Vec<ChiihouPai>,
    drawn: Option<ChiihouPai>,
    remaining_tiles: Option<u32>,
    dora_indicators: Vec<ChiihouPai>,
    discards: [Vec<ChiihouPai>; CHIIHOU_PLAYER_COUNT],
    reached: [bool; CHIIHOU_PLAYER_COUNT],
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

    pub fn hand(&self) -> &[ChiihouPai] {
        &self.hand
    }

    pub fn drawn(&self) -> Option<ChiihouPai> {
        self.drawn
    }

    pub fn remaining_tiles(&self) -> Option<u32> {
        self.remaining_tiles
    }

    pub fn dora_indicators(&self) -> &[ChiihouPai] {
        &self.dora_indicators
    }

    pub fn discards(&self) -> &[Vec<ChiihouPai>; CHIIHOU_PLAYER_COUNT] {
        &self.discards
    }

    pub fn reached(&self) -> &[bool; CHIIHOU_PLAYER_COUNT] {
        &self.reached
    }

    pub fn player_index(&self, player: &PublicKey) -> Option<usize> {
        self.players.iter().position(|p| p == player)
    }

    pub fn table_snapshot(&self, ai_pubkey: &PublicKey) -> ChiihouTableSnapshot {
        ChiihouTableSnapshot {
            dora_indicators: self.dora_indicators.clone(),
            round_wind: self.round_wind,
            seat_wind: self.seat,
            player_id: self
                .player_index(ai_pubkey)
                .and_then(|index| u8::try_from(index).ok()),
            oya: self
                .dealer
                .and_then(|dealer| self.player_index(&dealer))
                .and_then(|index| u8::try_from(index).ok()),
            discards: self.discards.clone(),
            reached: self.reached,
        }
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
                self.reset_table_state();
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
                self.reset_table_state();
                self.phase = ChiihouMatchPhase::InKyoku;
            }
            ChiihouLifecycleNotification::KyokuEnd => {
                self.drawn = None;
                self.phase = ChiihouMatchPhase::WaitingNext;
            }
            ChiihouLifecycleNotification::GameEnd { scores } => {
                self.final_scores = scores.clone();
                self.phase = ChiihouMatchPhase::Ended;
            }
        }
    }

    pub fn apply_table_notification(
        &mut self,
        ai_pubkey: &PublicKey,
        notification: &ChiihouTableNotification,
    ) -> Result<(), ChiihouTableStateError> {
        match notification {
            ChiihouTableNotification::Haipai { player, hand } => {
                if player != ai_pubkey {
                    return Err(ChiihouTableStateError::HaipaiForOtherPlayer);
                }
                self.hand = hand.clone();
                self.drawn = None;
                Ok(())
            }
            ChiihouTableNotification::Dora { indicator } => {
                self.dora_indicators.push(*indicator);
                Ok(())
            }
            ChiihouTableNotification::Tsumo {
                player,
                remaining_tiles,
                tile,
            } => {
                if player != ai_pubkey {
                    return Err(ChiihouTableStateError::TsumoForOtherPlayer);
                }
                self.drawn = Some(*tile);
                self.remaining_tiles = Some(*remaining_tiles);
                Ok(())
            }
            ChiihouTableNotification::Sutehai { player, tile } => {
                let Some(index) = self.player_index(player) else {
                    return Err(ChiihouTableStateError::UnknownPlayer);
                };
                self.discards[index].push(*tile);
                if player == ai_pubkey {
                    self.discard_from_held_hand(*tile)?;
                }
                Ok(())
            }
            ChiihouTableNotification::Say { player, action } => {
                let Some(index) = self.player_index(player) else {
                    return Err(ChiihouTableStateError::UnknownPlayer);
                };
                if *action == ChiihouSayAction::Richi {
                    self.reached[index] = true;
                }
                Ok(())
            }
        }
    }

    fn reset_table_state(&mut self) {
        self.hand.clear();
        self.drawn = None;
        self.remaining_tiles = None;
        self.dora_indicators.clear();
        self.discards = Default::default();
        self.reached = [false; CHIIHOU_PLAYER_COUNT];
    }

    fn discard_from_held_hand(&mut self, tile: ChiihouPai) -> Result<(), ChiihouTableStateError> {
        let mut tiles = self.hand.clone();
        tiles.extend(self.drawn);
        let Some(position) = tiles.iter().position(|held| *held == tile) else {
            return Err(ChiihouTableStateError::SutehaiTileNotHeld(tile));
        };
        tiles.remove(position);
        self.hand = tiles;
        self.drawn = None;
        Ok(())
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

    fn pai(s: &str) -> ChiihouPai {
        s.parse().unwrap()
    }

    fn pais(items: &[&str]) -> Vec<ChiihouPai> {
        items.iter().map(|s| pai(s)).collect()
    }

    fn haipai_hand() -> Vec<ChiihouPai> {
        pais(&[
            "1m", "2m", "3m", "4m", "5m", "6m", "7p", "8p", "9p", "1s", "1s", "2z", "2z",
        ])
    }

    fn ai_pubkey() -> PublicKey {
        player_pubkey(1)
    }

    fn state_in_kyoku() -> ChiihouMatchState {
        let mut state = ChiihouMatchState::new();
        state.apply(&gamestart());
        state.apply(&kyokustart(0, 0));
        state
    }

    fn haipai(player: PublicKey) -> ChiihouTableNotification {
        ChiihouTableNotification::Haipai {
            player,
            hand: haipai_hand(),
        }
    }

    fn dora(s: &str) -> ChiihouTableNotification {
        ChiihouTableNotification::Dora { indicator: pai(s) }
    }

    fn tsumo(player: PublicKey, remaining_tiles: u32, s: &str) -> ChiihouTableNotification {
        ChiihouTableNotification::Tsumo {
            player,
            remaining_tiles,
            tile: pai(s),
        }
    }

    fn sutehai(player: PublicKey, s: &str) -> ChiihouTableNotification {
        ChiihouTableNotification::Sutehai {
            player,
            tile: pai(s),
        }
    }

    fn say(player: PublicKey, action: ChiihouSayAction) -> ChiihouTableNotification {
        ChiihouTableNotification::Say { player, action }
    }

    #[test]
    fn initial_state_has_empty_table_state() {
        let state = ChiihouMatchState::new();
        assert!(state.hand().is_empty());
        assert_eq!(state.drawn(), None);
        assert_eq!(state.remaining_tiles(), None);
        assert!(state.dora_indicators().is_empty());
        assert!(state.discards().iter().all(|river| river.is_empty()));
        assert_eq!(state.reached(), &[false; 4]);
    }

    #[test]
    fn haipai_for_self_sets_hand() {
        let mut state = state_in_kyoku();
        state
            .apply_table_notification(&ai_pubkey(), &haipai(ai_pubkey()))
            .unwrap();
        assert_eq!(state.hand(), haipai_hand());
        assert_eq!(state.drawn(), None);
    }

    #[test]
    fn haipai_for_other_player_is_error() {
        let mut state = state_in_kyoku();
        assert_eq!(
            state.apply_table_notification(&ai_pubkey(), &haipai(player_pubkey(2))),
            Err(ChiihouTableStateError::HaipaiForOtherPlayer)
        );
        assert!(state.hand().is_empty());
    }

    #[test]
    fn dora_indicators_are_appended_in_order() {
        let mut state = state_in_kyoku();
        state
            .apply_table_notification(&ai_pubkey(), &dora("5p"))
            .unwrap();
        state
            .apply_table_notification(&ai_pubkey(), &dora("1z"))
            .unwrap();
        assert_eq!(state.dora_indicators(), pais(&["5p", "1z"]));
    }

    #[test]
    fn same_dora_value_from_another_event_is_appended() {
        let mut state = state_in_kyoku();
        state
            .apply_table_notification(&ai_pubkey(), &dora("5p"))
            .unwrap();
        state
            .apply_table_notification(&ai_pubkey(), &dora("5p"))
            .unwrap();
        assert_eq!(state.dora_indicators(), pais(&["5p", "5p"]));
    }

    #[test]
    fn tsumo_for_self_sets_drawn_and_remaining_tiles() {
        let mut state = state_in_kyoku();
        state
            .apply_table_notification(&ai_pubkey(), &tsumo(ai_pubkey(), 69, "7z"))
            .unwrap();
        assert_eq!(state.drawn(), Some(pai("7z")));
        assert_eq!(state.remaining_tiles(), Some(69));
    }

    #[test]
    fn tsumo_for_other_player_is_error() {
        let mut state = state_in_kyoku();
        assert_eq!(
            state.apply_table_notification(&ai_pubkey(), &tsumo(player_pubkey(2), 69, "7z")),
            Err(ChiihouTableStateError::TsumoForOtherPlayer)
        );
        assert_eq!(state.drawn(), None);
        assert_eq!(state.remaining_tiles(), None);
    }

    #[test]
    fn sutehai_appends_to_target_player_river() {
        let mut state = state_in_kyoku();
        state
            .apply_table_notification(&ai_pubkey(), &sutehai(player_pubkey(2), "7z"))
            .unwrap();
        state
            .apply_table_notification(&ai_pubkey(), &sutehai(player_pubkey(2), "1m"))
            .unwrap();
        assert_eq!(state.discards()[1], pais(&["7z", "1m"]));
        assert!(state.discards()[0].is_empty());
    }

    #[test]
    fn own_sutehai_updates_hand_and_clears_drawn() {
        let mut state = state_in_kyoku();
        state
            .apply_table_notification(&ai_pubkey(), &haipai(ai_pubkey()))
            .unwrap();
        state
            .apply_table_notification(&ai_pubkey(), &tsumo(ai_pubkey(), 69, "7z"))
            .unwrap();
        state
            .apply_table_notification(&ai_pubkey(), &sutehai(ai_pubkey(), "1m"))
            .unwrap();
        let mut expected = haipai_hand();
        expected.remove(0);
        expected.push(pai("7z"));
        assert_eq!(state.hand(), expected);
        assert_eq!(state.drawn(), None);
        assert_eq!(state.discards()[0], pais(&["1m"]));
    }

    #[test]
    fn own_tsumogiri_removes_drawn_tile() {
        let mut state = state_in_kyoku();
        state
            .apply_table_notification(&ai_pubkey(), &haipai(ai_pubkey()))
            .unwrap();
        state
            .apply_table_notification(&ai_pubkey(), &tsumo(ai_pubkey(), 69, "7z"))
            .unwrap();
        state
            .apply_table_notification(&ai_pubkey(), &sutehai(ai_pubkey(), "7z"))
            .unwrap();
        assert_eq!(state.hand(), haipai_hand());
        assert_eq!(state.drawn(), None);
    }

    #[test]
    fn own_sutehai_not_in_held_hand_is_error_but_river_is_updated() {
        let mut state = state_in_kyoku();
        state
            .apply_table_notification(&ai_pubkey(), &haipai(ai_pubkey()))
            .unwrap();
        assert_eq!(
            state.apply_table_notification(&ai_pubkey(), &sutehai(ai_pubkey(), "9s")),
            Err(ChiihouTableStateError::SutehaiTileNotHeld(pai("9s")))
        );
        assert_eq!(state.hand(), haipai_hand());
        assert_eq!(state.discards()[0], pais(&["9s"]));
    }

    #[test]
    fn sutehai_from_unknown_player_is_error() {
        let mut state = state_in_kyoku();
        assert_eq!(
            state.apply_table_notification(&ai_pubkey(), &sutehai(player_pubkey(9), "7z")),
            Err(ChiihouTableStateError::UnknownPlayer)
        );
        assert!(state.discards().iter().all(|river| river.is_empty()));
    }

    #[test]
    fn say_richi_sets_reached_for_target_player() {
        let mut state = state_in_kyoku();
        state
            .apply_table_notification(
                &ai_pubkey(),
                &say(player_pubkey(3), ChiihouSayAction::Richi),
            )
            .unwrap();
        assert_eq!(state.reached(), &[false, false, true, false]);
    }

    #[test]
    fn reapplying_same_richi_keeps_state() {
        let mut state = state_in_kyoku();
        let notification = say(player_pubkey(3), ChiihouSayAction::Richi);
        state
            .apply_table_notification(&ai_pubkey(), &notification)
            .unwrap();
        let after_first = state.clone();
        state
            .apply_table_notification(&ai_pubkey(), &notification)
            .unwrap();
        assert_eq!(state, after_first);
    }

    #[test]
    fn say_other_actions_do_not_change_state() {
        let mut state = state_in_kyoku();
        let before = state.clone();
        for action in [
            ChiihouSayAction::Tsumo,
            ChiihouSayAction::Ron,
            ChiihouSayAction::Pon,
            ChiihouSayAction::Chi,
            ChiihouSayAction::Kan,
            ChiihouSayAction::Tenpai,
            ChiihouSayAction::Noten,
        ] {
            state
                .apply_table_notification(&ai_pubkey(), &say(player_pubkey(2), action))
                .unwrap();
            assert_eq!(state, before, "action: {action:?}");
        }
    }

    #[test]
    fn say_from_unknown_player_is_error() {
        let mut state = state_in_kyoku();
        assert_eq!(
            state.apply_table_notification(
                &ai_pubkey(),
                &say(player_pubkey(9), ChiihouSayAction::Richi)
            ),
            Err(ChiihouTableStateError::UnknownPlayer)
        );
        assert_eq!(state.reached(), &[false; 4]);
    }

    fn filled_table_state() -> ChiihouMatchState {
        let mut state = state_in_kyoku();
        state
            .apply_table_notification(&ai_pubkey(), &haipai(ai_pubkey()))
            .unwrap();
        state
            .apply_table_notification(&ai_pubkey(), &dora("5p"))
            .unwrap();
        state
            .apply_table_notification(&ai_pubkey(), &tsumo(ai_pubkey(), 69, "7z"))
            .unwrap();
        state
            .apply_table_notification(&ai_pubkey(), &sutehai(player_pubkey(2), "1z"))
            .unwrap();
        state
            .apply_table_notification(
                &ai_pubkey(),
                &say(player_pubkey(2), ChiihouSayAction::Richi),
            )
            .unwrap();
        state
    }

    #[test]
    fn kyokustart_resets_table_state() {
        let mut state = filled_table_state();
        state.apply(&ChiihouLifecycleNotification::KyokuEnd);
        state.apply(&kyokustart(1, 1000));
        assert!(state.hand().is_empty());
        assert_eq!(state.drawn(), None);
        assert_eq!(state.remaining_tiles(), None);
        assert!(state.dora_indicators().is_empty());
        assert!(state.discards().iter().all(|river| river.is_empty()));
        assert_eq!(state.reached(), &[false; 4]);
    }

    #[test]
    fn kyokuend_clears_drawn_but_keeps_river_and_dora() {
        let mut state = filled_table_state();
        state.apply(&ChiihouLifecycleNotification::KyokuEnd);
        assert_eq!(state.drawn(), None);
        assert_eq!(state.dora_indicators(), pais(&["5p"]));
        assert_eq!(state.discards()[1], pais(&["1z"]));
    }

    #[test]
    fn gamestart_resets_table_state() {
        let mut state = filled_table_state();
        state.apply(&gamestart());
        assert!(state.hand().is_empty());
        assert_eq!(state.drawn(), None);
        assert_eq!(state.remaining_tiles(), None);
        assert!(state.dora_indicators().is_empty());
        assert!(state.discards().iter().all(|river| river.is_empty()));
        assert_eq!(state.reached(), &[false; 4]);
    }

    #[test]
    fn table_snapshot_reflects_table_state() {
        let state = filled_table_state();
        let snapshot = state.table_snapshot(&ai_pubkey());
        assert_eq!(snapshot.dora_indicators, pais(&["5p"]));
        assert_eq!(snapshot.round_wind, Some(ChiihouWind::East));
        assert_eq!(snapshot.seat_wind, Some(ChiihouWind::South));
        assert_eq!(snapshot.player_id, Some(0));
        assert_eq!(snapshot.oya, Some(1));
        assert_eq!(snapshot.discards[1], pais(&["1z"]));
        assert_eq!(snapshot.reached, [false, true, false, false]);
    }

    #[test]
    fn table_snapshot_without_gamestart_is_empty() {
        let state = ChiihouMatchState::new();
        let snapshot = state.table_snapshot(&ai_pubkey());
        assert_eq!(snapshot, ChiihouTableSnapshot::default());
    }

    #[test]
    fn table_snapshot_for_unknown_ai_has_no_player_id() {
        let state = state_in_kyoku();
        let snapshot = state.table_snapshot(&player_pubkey(9));
        assert_eq!(snapshot.player_id, None);
        assert_eq!(snapshot.oya, Some(1));
    }

    #[test]
    fn table_snapshot_with_unknown_dealer_has_no_oya() {
        let mut state = ChiihouMatchState::new();
        state.apply(&gamestart());
        state.apply(&ChiihouLifecycleNotification::KyokuStart {
            round_wind: ChiihouWind::East,
            dealer: player_pubkey(9),
            honba: 0,
            kyotaku_points: 0,
        });
        assert_eq!(state.table_snapshot(&ai_pubkey()).oya, None);
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
