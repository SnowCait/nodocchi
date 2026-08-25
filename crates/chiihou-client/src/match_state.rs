use bot_core::MeldKind;
use nostr_sdk::prelude::PublicKey;
use thiserror::Error;

use crate::lifecycle::{
    CHIIHOU_PLAYER_COUNT, ChiihouLifecycleNotification, ChiihouPlayerScore, ChiihouWind,
};
use crate::protocol::{ChiihouPai, ChiihouSuit};
use crate::table_notification::{ChiihouSayAction, ChiihouTableNotification};

const OPEN_KAKAN_TILE_COUNT: usize = 1;
const OPEN_CALL_TILE_COUNT: usize = 3;
const OPEN_KAN_TILE_COUNT: usize = 4;

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
    #[error("invalid open tile count: {0}")]
    InvalidOpenTileCount(usize),
    #[error("open call does not contain the claimable discard")]
    OpenWithoutCalledTile,
    #[error("open tiles do not form a meld: {0:?}")]
    OpenTilesAreNotMeld(Vec<ChiihouPai>),
    #[error("kakan {0} has no matching pon")]
    KakanWithoutPon(ChiihouPai),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChiihouMeld {
    pub kind: MeldKind,
    pub tiles: Vec<ChiihouPai>,
    pub called_tile: Option<ChiihouPai>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ChiihouTableSnapshot {
    pub dora_indicators: Vec<ChiihouPai>,
    pub round_wind: Option<ChiihouWind>,
    pub seat_wind: Option<ChiihouWind>,
    pub player_id: Option<u8>,
    pub oya: Option<u8>,
    pub remaining_tiles: Option<u32>,
    pub honba: Option<u32>,
    pub kyotaku_points: Option<u32>,
    pub discards: [Vec<ChiihouPai>; CHIIHOU_PLAYER_COUNT],
    pub reached: [bool; CHIIHOU_PLAYER_COUNT],
    pub newly_visible_meld_tiles: [Vec<ChiihouPai>; CHIIHOU_PLAYER_COUNT],
    pub melds: [Vec<ChiihouMeld>; CHIIHOU_PLAYER_COUNT],
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ChiihouMatchState {
    players: Vec<PublicKey>,
    seat: Option<ChiihouWind>,
    round_wind: Option<ChiihouWind>,
    dealer: Option<PublicKey>,
    honba: Option<u32>,
    kyotaku_points: Option<u32>,
    phase: ChiihouMatchPhase,
    final_scores: Vec<ChiihouPlayerScore>,
    hand: Vec<ChiihouPai>,
    drawn: Option<ChiihouPai>,
    remaining_tiles: Option<u32>,
    dora_indicators: Vec<ChiihouPai>,
    discards: [Vec<ChiihouPai>; CHIIHOU_PLAYER_COUNT],
    reached: [bool; CHIIHOU_PLAYER_COUNT],
    claimable_discard: Option<ChiihouPai>,
    newly_visible_meld_tiles: [Vec<ChiihouPai>; CHIIHOU_PLAYER_COUNT],
    melds: [Vec<ChiihouMeld>; CHIIHOU_PLAYER_COUNT],
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

    /// 本場 [本]。`kyokustart` を受け取るまでは 0 と断定せず `None`。
    pub fn honba(&self) -> Option<u32> {
        self.honba
    }

    /// 供託 [点]。`kyokustart` を受け取るまでは 0 と断定せず `None`。
    pub fn kyotaku_points(&self) -> Option<u32> {
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

    pub fn claimable_discard(&self) -> Option<ChiihouPai> {
        self.claimable_discard
    }

    pub fn newly_visible_meld_tiles(&self) -> &[Vec<ChiihouPai>; CHIIHOU_PLAYER_COUNT] {
        &self.newly_visible_meld_tiles
    }

    pub fn melds(&self) -> &[Vec<ChiihouMeld>; CHIIHOU_PLAYER_COUNT] {
        &self.melds
    }

    pub fn player_index(&self, player: &PublicKey) -> Option<usize> {
        self.players.iter().position(|p| p == player)
    }

    pub fn table_snapshot(&self, ai_pubkey: &PublicKey) -> ChiihouTableSnapshot {
        let player_id = self
            .player_index(ai_pubkey)
            .and_then(|index| u8::try_from(index).ok());
        let oya = self
            .dealer
            .and_then(|dealer| self.player_index(&dealer))
            .and_then(|index| u8::try_from(index).ok());
        ChiihouTableSnapshot {
            dora_indicators: self.dora_indicators.clone(),
            round_wind: self.round_wind,
            seat_wind: player_id
                .zip(oya)
                .and_then(|(player_id, oya)| seat_wind_from_player_and_dealer(player_id, oya)),
            player_id,
            oya,
            remaining_tiles: self.remaining_tiles,
            honba: self.honba,
            kyotaku_points: self.kyotaku_points,
            discards: self.discards.clone(),
            reached: self.reached,
            newly_visible_meld_tiles: self.newly_visible_meld_tiles.clone(),
            melds: self.melds.clone(),
        }
    }

    pub fn apply(&mut self, notification: &ChiihouLifecycleNotification) {
        match notification {
            ChiihouLifecycleNotification::GameStart { seat, players } => {
                self.players = players.clone();
                self.seat = Some(*seat);
                self.round_wind = None;
                self.dealer = None;
                self.honba = None;
                self.kyotaku_points = None;
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
                self.honba = Some(*honba);
                self.kyotaku_points = Some(*kyotaku_points);
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
                self.claimable_discard = None;
                Ok(())
            }
            ChiihouTableNotification::Sutehai { player, tile } => {
                let Some(index) = self.player_index(player) else {
                    return Err(ChiihouTableStateError::UnknownPlayer);
                };
                self.discards[index].push(*tile);
                self.claimable_discard = Some(*tile);
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
            ChiihouTableNotification::Open { player, tiles } => {
                let Some(index) = self.player_index(player) else {
                    return Err(ChiihouTableStateError::UnknownPlayer);
                };
                let meld_update = self.meld_update_from_open(index, tiles)?;
                let newly_visible = self.newly_visible_tiles_from_open(tiles)?;
                self.apply_meld_update(index, meld_update);
                self.newly_visible_meld_tiles[index].extend(newly_visible);
                self.claimable_discard = None;
                Ok(())
            }
        }
    }

    fn newly_visible_tiles_from_open(
        &self,
        tiles: &[ChiihouPai],
    ) -> Result<Vec<ChiihouPai>, ChiihouTableStateError> {
        let called = self.called_tile_index(tiles);
        match (tiles.len(), called) {
            (OPEN_KAKAN_TILE_COUNT, _) | (OPEN_KAN_TILE_COUNT, None) => Ok(tiles.to_vec()),
            (OPEN_CALL_TILE_COUNT, Some(called)) | (OPEN_KAN_TILE_COUNT, Some(called)) => {
                let mut newly_visible = tiles.to_vec();
                newly_visible.remove(called);
                Ok(newly_visible)
            }
            (OPEN_CALL_TILE_COUNT, None) => Err(ChiihouTableStateError::OpenWithoutCalledTile),
            (count, _) => Err(ChiihouTableStateError::InvalidOpenTileCount(count)),
        }
    }

    fn called_tile_index(&self, tiles: &[ChiihouPai]) -> Option<usize> {
        self.claimable_discard
            .and_then(|discard| tiles.iter().position(|tile| *tile == discard))
    }

    fn meld_update_from_open(
        &self,
        player: usize,
        tiles: &[ChiihouPai],
    ) -> Result<ChiihouMeldUpdate, ChiihouTableStateError> {
        let called = self.called_tile_index(tiles);
        match (tiles.len(), called) {
            (OPEN_KAKAN_TILE_COUNT, _) => {
                let tile = tiles[0];
                let pon = self
                    .pon_index_of(player, tile)
                    .ok_or(ChiihouTableStateError::KakanWithoutPon(tile))?;
                Ok(ChiihouMeldUpdate::UpgradePonToKakan { pon, tile })
            }
            (OPEN_CALL_TILE_COUNT, Some(called)) => Ok(ChiihouMeldUpdate::Append(ChiihouMeld {
                kind: called_meld_kind(tiles)?,
                tiles: tiles.to_vec(),
                called_tile: Some(tiles[called]),
            })),
            (OPEN_KAN_TILE_COUNT, called) => {
                if !is_same_tile(tiles) {
                    return Err(ChiihouTableStateError::OpenTilesAreNotMeld(tiles.to_vec()));
                }
                Ok(ChiihouMeldUpdate::Append(ChiihouMeld {
                    kind: match called {
                        Some(_) => MeldKind::Daiminkan,
                        None => MeldKind::Ankan,
                    },
                    tiles: tiles.to_vec(),
                    called_tile: called.map(|called| tiles[called]),
                }))
            }
            (OPEN_CALL_TILE_COUNT, None) => Err(ChiihouTableStateError::OpenWithoutCalledTile),
            (count, _) => Err(ChiihouTableStateError::InvalidOpenTileCount(count)),
        }
    }

    fn pon_index_of(&self, player: usize, tile: ChiihouPai) -> Option<usize> {
        self.melds.get(player)?.iter().position(|meld| {
            meld.kind == MeldKind::Pon && meld.tiles.first().copied() == Some(tile)
        })
    }

    fn apply_meld_update(&mut self, player: usize, update: ChiihouMeldUpdate) {
        match update {
            ChiihouMeldUpdate::Append(meld) => self.melds[player].push(meld),
            ChiihouMeldUpdate::UpgradePonToKakan { pon, tile } => {
                let meld = &mut self.melds[player][pon];
                meld.kind = MeldKind::Kakan;
                meld.tiles.push(tile);
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
        self.claimable_discard = None;
        self.newly_visible_meld_tiles = Default::default();
        self.melds = Default::default();
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

#[derive(Debug, Clone, PartialEq, Eq)]
enum ChiihouMeldUpdate {
    Append(ChiihouMeld),
    UpgradePonToKakan { pon: usize, tile: ChiihouPai },
}

fn called_meld_kind(tiles: &[ChiihouPai]) -> Result<MeldKind, ChiihouTableStateError> {
    if is_same_tile(tiles) {
        return Ok(MeldKind::Pon);
    }
    if is_sequence(tiles) {
        return Ok(MeldKind::Chi);
    }
    Err(ChiihouTableStateError::OpenTilesAreNotMeld(tiles.to_vec()))
}

fn is_same_tile(tiles: &[ChiihouPai]) -> bool {
    tiles.windows(2).all(|pair| pair[0] == pair[1])
}

fn is_sequence(tiles: &[ChiihouPai]) -> bool {
    let Some(first) = tiles.first() else {
        return false;
    };
    if first.suit() == ChiihouSuit::Zi || tiles.iter().any(|tile| tile.suit() != first.suit()) {
        return false;
    }
    let mut numbers: Vec<u8> = tiles.iter().map(|tile| tile.number()).collect();
    numbers.sort_unstable();
    numbers.windows(2).all(|pair| pair[1] == pair[0] + 1)
}

fn seat_wind_from_player_and_dealer(player_id: u8, oya: u8) -> Option<ChiihouWind> {
    let player_count = CHIIHOU_PLAYER_COUNT as u8;
    if player_id >= player_count || oya >= player_count {
        return None;
    }
    let seat_index = (player_id + player_count - oya) % player_count;
    match seat_index {
        0 => Some(ChiihouWind::East),
        1 => Some(ChiihouWind::South),
        2 => Some(ChiihouWind::West),
        3 => Some(ChiihouWind::North),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nostr_sdk::prelude::Keys;

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
        assert_eq!(state.honba(), None);
        assert_eq!(state.kyotaku_points(), None);
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
        assert_eq!(state.honba(), Some(1));
        assert_eq!(state.kyotaku_points(), Some(2000));
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
        assert_eq!(state.honba(), Some(1));
        assert_eq!(state.kyotaku_points(), Some(1000));
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
        assert_eq!(state.honba(), None);
        assert_eq!(state.kyotaku_points(), None);
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

    fn open(player: PublicKey, items: &[&str]) -> ChiihouTableNotification {
        ChiihouTableNotification::Open {
            player,
            tiles: pais(items),
        }
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

    #[test]
    fn initial_state_has_no_claimable_discard_or_meld_tiles() {
        let state = ChiihouMatchState::new();
        assert_eq!(state.claimable_discard(), None);
        assert!(
            state
                .newly_visible_meld_tiles()
                .iter()
                .all(|tiles| tiles.is_empty())
        );
    }

    #[test]
    fn sutehai_sets_claimable_discard_and_tsumo_clears_it() {
        let mut state = state_in_kyoku();
        state
            .apply_table_notification(&ai_pubkey(), &sutehai(player_pubkey(2), "1z"))
            .unwrap();
        assert_eq!(state.claimable_discard(), Some(pai("1z")));
        state
            .apply_table_notification(&ai_pubkey(), &tsumo(ai_pubkey(), 69, "7z"))
            .unwrap();
        assert_eq!(state.claimable_discard(), None);
    }

    #[test]
    fn pon_keeps_called_tile_in_river_and_adds_two_tiles() {
        let mut state = state_in_kyoku();
        state
            .apply_table_notification(&ai_pubkey(), &sutehai(player_pubkey(2), "1z"))
            .unwrap();
        state
            .apply_table_notification(&ai_pubkey(), &open(player_pubkey(3), &["1z", "1z", "1z"]))
            .unwrap();
        assert_eq!(state.discards()[1], pais(&["1z"]));
        assert_eq!(state.newly_visible_meld_tiles()[2], pais(&["1z", "1z"]));
        assert_eq!(state.claimable_discard(), None);
    }

    #[test]
    fn chi_excludes_only_the_called_tile() {
        let mut state = state_in_kyoku();
        state
            .apply_table_notification(&ai_pubkey(), &sutehai(player_pubkey(2), "3m"))
            .unwrap();
        state
            .apply_table_notification(&ai_pubkey(), &open(player_pubkey(3), &["1m", "2m", "3m"]))
            .unwrap();
        assert_eq!(state.discards()[1], pais(&["3m"]));
        assert_eq!(state.newly_visible_meld_tiles()[2], pais(&["1m", "2m"]));
    }

    #[test]
    fn daiminkan_adds_three_tiles() {
        let mut state = state_in_kyoku();
        state
            .apply_table_notification(&ai_pubkey(), &sutehai(player_pubkey(2), "1z"))
            .unwrap();
        state
            .apply_table_notification(
                &ai_pubkey(),
                &open(player_pubkey(3), &["1z", "1z", "1z", "1z"]),
            )
            .unwrap();
        assert_eq!(state.discards()[1], pais(&["1z"]));
        assert_eq!(state.newly_visible_meld_tiles()[2], pais(&["1z"; 3]));
    }

    #[test]
    fn ankan_after_own_tsumo_adds_four_tiles() {
        let mut state = state_in_kyoku();
        state
            .apply_table_notification(&ai_pubkey(), &sutehai(player_pubkey(2), "9p"))
            .unwrap();
        state
            .apply_table_notification(&ai_pubkey(), &tsumo(ai_pubkey(), 69, "1z"))
            .unwrap();
        assert_eq!(state.claimable_discard(), None);
        state
            .apply_table_notification(&ai_pubkey(), &open(ai_pubkey(), &["1z", "1z", "1z", "1z"]))
            .unwrap();
        assert_eq!(state.discards()[1], pais(&["9p"]));
        assert_eq!(state.newly_visible_meld_tiles()[0], pais(&["1z"; 4]));
    }

    #[test]
    fn ankan_by_other_player_adds_four_tiles_without_observed_tsumo() {
        let mut state = state_in_kyoku();
        state
            .apply_table_notification(&ai_pubkey(), &sutehai(player_pubkey(2), "9p"))
            .unwrap();
        state
            .apply_table_notification(
                &ai_pubkey(),
                &open(player_pubkey(3), &["1z", "1z", "1z", "1z"]),
            )
            .unwrap();
        assert_eq!(state.discards()[1], pais(&["9p"]));
        assert_eq!(state.newly_visible_meld_tiles()[2], pais(&["1z"; 4]));
    }

    #[test]
    fn kakan_adds_the_single_notified_tile() {
        let mut state = ponned_state();
        state
            .apply_table_notification(&ai_pubkey(), &open(player_pubkey(3), &["1z"]))
            .unwrap();
        assert_eq!(
            state.newly_visible_meld_tiles()[2],
            pais(&["1z", "1z", "1z"])
        );
    }

    fn ponned_state() -> ChiihouMatchState {
        let mut state = state_in_kyoku();
        state
            .apply_table_notification(&ai_pubkey(), &sutehai(player_pubkey(2), "1z"))
            .unwrap();
        state
            .apply_table_notification(&ai_pubkey(), &open(player_pubkey(3), &["1z", "1z", "1z"]))
            .unwrap();
        state
    }

    fn meld_kinds(state: &ChiihouMatchState, player: usize) -> Vec<MeldKind> {
        state.melds()[player].iter().map(|meld| meld.kind).collect()
    }

    #[test]
    fn initial_state_has_no_melds() {
        let state = ChiihouMatchState::new();
        assert!(state.melds().iter().all(|melds| melds.is_empty()));
    }

    #[test]
    fn pon_records_a_pon_meld_with_the_called_tile() {
        let state = ponned_state();
        assert_eq!(meld_kinds(&state, 2), [MeldKind::Pon]);
        assert_eq!(state.melds()[2][0].tiles, pais(&["1z", "1z", "1z"]));
        assert_eq!(state.melds()[2][0].called_tile, Some(pai("1z")));
        assert!(state.melds()[2][0].kind.is_open());
        assert!(state.melds()[0].is_empty());
    }

    #[test]
    fn chi_records_a_chi_meld_with_the_called_tile() {
        let mut state = state_in_kyoku();
        state
            .apply_table_notification(&ai_pubkey(), &sutehai(player_pubkey(2), "3m"))
            .unwrap();
        state
            .apply_table_notification(&ai_pubkey(), &open(player_pubkey(3), &["1m", "2m", "3m"]))
            .unwrap();
        assert_eq!(meld_kinds(&state, 2), [MeldKind::Chi]);
        assert_eq!(state.melds()[2][0].tiles, pais(&["1m", "2m", "3m"]));
        assert_eq!(state.melds()[2][0].called_tile, Some(pai("3m")));
    }

    #[test]
    fn daiminkan_records_a_daiminkan_meld() {
        let mut state = state_in_kyoku();
        state
            .apply_table_notification(&ai_pubkey(), &sutehai(player_pubkey(2), "1z"))
            .unwrap();
        state
            .apply_table_notification(
                &ai_pubkey(),
                &open(player_pubkey(3), &["1z", "1z", "1z", "1z"]),
            )
            .unwrap();
        assert_eq!(meld_kinds(&state, 2), [MeldKind::Daiminkan]);
        assert_eq!(state.melds()[2][0].tiles.len(), 4);
        assert_eq!(state.melds()[2][0].called_tile, Some(pai("1z")));
        assert!(state.melds()[2][0].kind.is_open());
    }

    #[test]
    fn ankan_records_an_ankan_meld_without_called_tile() {
        let mut state = state_in_kyoku();
        state
            .apply_table_notification(&ai_pubkey(), &sutehai(player_pubkey(2), "9p"))
            .unwrap();
        state
            .apply_table_notification(&ai_pubkey(), &tsumo(ai_pubkey(), 69, "1z"))
            .unwrap();
        state
            .apply_table_notification(&ai_pubkey(), &open(ai_pubkey(), &["1z", "1z", "1z", "1z"]))
            .unwrap();
        assert_eq!(meld_kinds(&state, 0), [MeldKind::Ankan]);
        assert_eq!(state.melds()[0][0].called_tile, None);
        assert!(!state.melds()[0][0].kind.is_open());
    }

    #[test]
    fn kakan_upgrades_the_existing_pon_instead_of_adding_a_meld() {
        let mut state = ponned_state();
        state
            .apply_table_notification(&ai_pubkey(), &open(player_pubkey(3), &["1z"]))
            .unwrap();
        assert_eq!(meld_kinds(&state, 2), [MeldKind::Kakan]);
        assert_eq!(state.melds()[2][0].tiles, pais(&["1z"; 4]));
        assert_eq!(state.melds()[2][0].called_tile, Some(pai("1z")));
    }

    #[test]
    fn kakan_without_pon_is_error_and_keeps_state() {
        let mut state = state_in_kyoku();
        assert_eq!(
            state.apply_table_notification(&ai_pubkey(), &open(player_pubkey(3), &["1z"])),
            Err(ChiihouTableStateError::KakanWithoutPon(pai("1z")))
        );
        assert!(state.melds()[2].is_empty());
        assert!(state.newly_visible_meld_tiles()[2].is_empty());
    }

    #[test]
    fn kakan_of_another_tile_than_the_pon_is_error() {
        let mut state = ponned_state();
        assert_eq!(
            state.apply_table_notification(&ai_pubkey(), &open(player_pubkey(3), &["2z"])),
            Err(ChiihouTableStateError::KakanWithoutPon(pai("2z")))
        );
        assert_eq!(meld_kinds(&state, 2), [MeldKind::Pon]);
        assert_eq!(state.newly_visible_meld_tiles()[2], pais(&["1z", "1z"]));
    }

    #[test]
    fn kakan_by_another_player_does_not_touch_own_pon() {
        let mut state = ponned_state();
        state
            .apply_table_notification(&ai_pubkey(), &sutehai(player_pubkey(3), "2z"))
            .unwrap();
        state
            .apply_table_notification(&ai_pubkey(), &open(player_pubkey(4), &["2z", "2z", "2z"]))
            .unwrap();
        state
            .apply_table_notification(&ai_pubkey(), &open(player_pubkey(4), &["2z"]))
            .unwrap();
        assert_eq!(meld_kinds(&state, 2), [MeldKind::Pon]);
        assert_eq!(meld_kinds(&state, 3), [MeldKind::Kakan]);
    }

    #[test]
    fn melds_accumulate_per_player() {
        let mut state = ponned_state();
        state
            .apply_table_notification(&ai_pubkey(), &sutehai(player_pubkey(3), "3m"))
            .unwrap();
        state
            .apply_table_notification(&ai_pubkey(), &open(player_pubkey(3), &["1m", "2m", "3m"]))
            .unwrap();
        assert_eq!(meld_kinds(&state, 2), [MeldKind::Pon, MeldKind::Chi]);
        assert!(state.melds()[3].is_empty());
    }

    #[test]
    fn open_tiles_that_do_not_form_a_meld_are_error() {
        let mut state = state_in_kyoku();
        state
            .apply_table_notification(&ai_pubkey(), &sutehai(player_pubkey(2), "1m"))
            .unwrap();
        assert_eq!(
            state.apply_table_notification(
                &ai_pubkey(),
                &open(player_pubkey(3), &["1m", "3m", "5m"])
            ),
            Err(ChiihouTableStateError::OpenTilesAreNotMeld(pais(&[
                "1m", "3m", "5m"
            ])))
        );
        assert!(state.melds()[2].is_empty());
        assert!(state.newly_visible_meld_tiles()[2].is_empty());
    }

    #[test]
    fn open_kan_of_mixed_tiles_is_error() {
        let mut state = state_in_kyoku();
        state
            .apply_table_notification(&ai_pubkey(), &sutehai(player_pubkey(2), "1z"))
            .unwrap();
        assert_eq!(
            state.apply_table_notification(
                &ai_pubkey(),
                &open(player_pubkey(3), &["1z", "1z", "1z", "2z"])
            ),
            Err(ChiihouTableStateError::OpenTilesAreNotMeld(pais(&[
                "1z", "1z", "1z", "2z"
            ])))
        );
        assert!(state.melds()[2].is_empty());
        assert!(state.newly_visible_meld_tiles()[2].is_empty());
    }

    #[test]
    fn open_with_invalid_tile_count_keeps_melds_empty() {
        let mut state = state_in_kyoku();
        state
            .apply_table_notification(&ai_pubkey(), &sutehai(player_pubkey(2), "1z"))
            .unwrap();
        assert_eq!(
            state.apply_table_notification(&ai_pubkey(), &open(player_pubkey(3), &["1z", "1z"])),
            Err(ChiihouTableStateError::InvalidOpenTileCount(2))
        );
        assert!(state.melds()[2].is_empty());
    }

    #[test]
    fn open_from_unknown_player_keeps_melds_empty() {
        let mut state = state_in_kyoku();
        state
            .apply_table_notification(&ai_pubkey(), &sutehai(player_pubkey(2), "1z"))
            .unwrap();
        assert_eq!(
            state.apply_table_notification(
                &ai_pubkey(),
                &open(player_pubkey(9), &["1z", "1z", "1z"])
            ),
            Err(ChiihouTableStateError::UnknownPlayer)
        );
        assert!(state.melds().iter().all(|melds| melds.is_empty()));
    }

    #[test]
    fn repeated_opens_accumulate_per_player() {
        let mut state = state_in_kyoku();
        state
            .apply_table_notification(&ai_pubkey(), &sutehai(player_pubkey(2), "1z"))
            .unwrap();
        state
            .apply_table_notification(&ai_pubkey(), &open(player_pubkey(3), &["1z", "1z", "1z"]))
            .unwrap();
        state
            .apply_table_notification(&ai_pubkey(), &sutehai(player_pubkey(3), "3m"))
            .unwrap();
        state
            .apply_table_notification(&ai_pubkey(), &open(player_pubkey(4), &["1m", "2m", "3m"]))
            .unwrap();
        assert_eq!(state.newly_visible_meld_tiles()[2], pais(&["1z", "1z"]));
        assert_eq!(state.newly_visible_meld_tiles()[3], pais(&["1m", "2m"]));
        assert!(state.newly_visible_meld_tiles()[0].is_empty());
        assert!(state.newly_visible_meld_tiles()[1].is_empty());
    }

    #[test]
    fn open_call_without_the_claimable_discard_is_error() {
        let mut state = state_in_kyoku();
        state
            .apply_table_notification(&ai_pubkey(), &sutehai(player_pubkey(2), "3m"))
            .unwrap();
        assert_eq!(
            state.apply_table_notification(
                &ai_pubkey(),
                &open(player_pubkey(3), &["4m", "5m", "6m"])
            ),
            Err(ChiihouTableStateError::OpenWithoutCalledTile)
        );
        assert!(state.newly_visible_meld_tiles()[2].is_empty());
        assert_eq!(state.claimable_discard(), Some(pai("3m")));
    }

    #[test]
    fn open_with_invalid_tile_count_is_error() {
        let mut state = state_in_kyoku();
        state
            .apply_table_notification(&ai_pubkey(), &sutehai(player_pubkey(2), "1z"))
            .unwrap();
        assert_eq!(
            state.apply_table_notification(&ai_pubkey(), &open(player_pubkey(3), &["1z", "1z"])),
            Err(ChiihouTableStateError::InvalidOpenTileCount(2))
        );
        assert!(state.newly_visible_meld_tiles()[2].is_empty());
    }

    #[test]
    fn open_from_unknown_player_is_error() {
        let mut state = state_in_kyoku();
        state
            .apply_table_notification(&ai_pubkey(), &sutehai(player_pubkey(2), "1z"))
            .unwrap();
        assert_eq!(
            state.apply_table_notification(
                &ai_pubkey(),
                &open(player_pubkey(9), &["1z", "1z", "1z"])
            ),
            Err(ChiihouTableStateError::UnknownPlayer)
        );
        assert!(
            state
                .newly_visible_meld_tiles()
                .iter()
                .all(|tiles| tiles.is_empty())
        );
        assert_eq!(state.claimable_discard(), Some(pai("1z")));
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
            .apply_table_notification(&ai_pubkey(), &sutehai(player_pubkey(3), "1m"))
            .unwrap();
        state
            .apply_table_notification(&ai_pubkey(), &open(player_pubkey(4), &["1m", "2m", "3m"]))
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
        assert_eq!(state.claimable_discard(), None);
        assert!(
            state
                .newly_visible_meld_tiles()
                .iter()
                .all(|tiles| tiles.is_empty())
        );
        assert!(state.melds().iter().all(|melds| melds.is_empty()));
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
        assert_eq!(state.claimable_discard(), None);
        assert!(
            state
                .newly_visible_meld_tiles()
                .iter()
                .all(|tiles| tiles.is_empty())
        );
        assert!(state.melds().iter().all(|melds| melds.is_empty()));
    }

    #[test]
    fn table_snapshot_reflects_table_state() {
        let state = filled_table_state();
        let snapshot = state.table_snapshot(&ai_pubkey());
        assert_eq!(snapshot.dora_indicators, pais(&["5p"]));
        assert_eq!(snapshot.round_wind, Some(ChiihouWind::East));
        assert_eq!(snapshot.seat_wind, Some(ChiihouWind::North));
        assert_eq!(snapshot.player_id, Some(0));
        assert_eq!(snapshot.oya, Some(1));
        assert_eq!(snapshot.remaining_tiles, Some(69));
        assert_eq!(snapshot.discards[1], pais(&["1z"]));
        assert_eq!(snapshot.reached, [false, true, false, false]);
        assert_eq!(snapshot.newly_visible_meld_tiles[3], pais(&["2m", "3m"]));
        assert!(snapshot.newly_visible_meld_tiles[0].is_empty());
        assert_eq!(
            snapshot.melds[3]
                .iter()
                .map(|meld| meld.kind)
                .collect::<Vec<_>>(),
            [MeldKind::Chi]
        );
        assert!(snapshot.melds[0].is_empty());
    }

    #[test]
    fn table_snapshot_meld_tiles_exclude_the_called_tile() {
        let state = ponned_state();
        let snapshot = state.table_snapshot(&ai_pubkey());
        assert_eq!(snapshot.discards[1], pais(&["1z"]));
        assert_eq!(snapshot.newly_visible_meld_tiles[2], pais(&["1z", "1z"]));
        assert_eq!(snapshot.melds[2][0].tiles, pais(&["1z", "1z", "1z"]));
        assert_eq!(snapshot.melds[2][0].called_tile, Some(pai("1z")));
    }

    #[test]
    fn table_snapshot_remaining_tiles_follow_tsumo_and_kyokustart() {
        let mut state = state_in_kyoku();
        assert_eq!(state.table_snapshot(&ai_pubkey()).remaining_tiles, None);
        state
            .apply_table_notification(&ai_pubkey(), &tsumo(ai_pubkey(), 42, "7z"))
            .unwrap();
        assert_eq!(state.table_snapshot(&ai_pubkey()).remaining_tiles, Some(42));
        state.apply(&ChiihouLifecycleNotification::KyokuEnd);
        state.apply(&kyokustart(1, 0));
        assert_eq!(state.table_snapshot(&ai_pubkey()).remaining_tiles, None);
    }

    fn npub_token(index: u64) -> String {
        use nostr_sdk::prelude::ToBech32;
        format!("nostr:{}", player_pubkey(index).to_bech32().unwrap())
    }

    #[test]
    fn protocol_notifications_carry_the_table_state_into_the_game_context() {
        use crate::decision::game_context_from_sutehai_request_with_state;
        use crate::lifecycle::parse_chiihou_lifecycle_notification;
        use crate::table_notification::parse_chiihou_table_notification;

        let mut state = ChiihouMatchState::new();
        let gamestart_content = format!(
            "NOTIFY gamestart 南 {} {} {} {}",
            npub_token(1),
            npub_token(2),
            npub_token(3),
            npub_token(4)
        );
        state.apply(
            &parse_chiihou_lifecycle_notification(&gamestart_content)
                .unwrap()
                .unwrap(),
        );

        let kyokustart_content = format!("NOTIFY kyokustart 東 {} 2 3000", npub_token(2));
        state.apply(
            &parse_chiihou_lifecycle_notification(&kyokustart_content)
                .unwrap()
                .unwrap(),
        );

        let tsumo_content = format!("NOTIFY tsumo {} 42 7z", npub_token(1));
        state
            .apply_table_notification(
                &ai_pubkey(),
                &parse_chiihou_table_notification(&tsumo_content)
                    .unwrap()
                    .unwrap(),
            )
            .unwrap();

        let snapshot = state.table_snapshot(&ai_pubkey());
        assert_eq!(snapshot.remaining_tiles, Some(42));
        assert_eq!(snapshot.honba, Some(2));
        assert_eq!(snapshot.kyotaku_points, Some(3000));

        let context = game_context_from_sutehai_request_with_state(&[], None, &snapshot);
        assert_eq!(context.remaining_tiles(), Some(42));
        assert_eq!(context.honba(), Some(2));
        assert_eq!(context.kyotaku_points(), Some(3000));
        assert_eq!(context.scores(), None);
        assert_eq!(context.kyoku(), None);
    }

    #[test]
    fn table_snapshot_honba_and_kyotaku_points_follow_kyokustart() {
        let mut state = ChiihouMatchState::new();
        state.apply(&gamestart());
        let before = state.table_snapshot(&ai_pubkey());
        assert_eq!(before.honba, None);
        assert_eq!(before.kyotaku_points, None);

        state.apply(&kyokustart(2, 3000));
        let snapshot = state.table_snapshot(&ai_pubkey());
        assert_eq!(snapshot.honba, Some(2));
        assert_eq!(snapshot.kyotaku_points, Some(3000));
    }

    #[test]
    fn table_snapshot_keeps_a_kyokustart_zero_as_a_known_zero() {
        let mut state = ChiihouMatchState::new();
        state.apply(&gamestart());
        state.apply(&kyokustart(0, 0));
        let snapshot = state.table_snapshot(&ai_pubkey());
        assert_eq!(snapshot.honba, Some(0));
        assert_eq!(snapshot.kyotaku_points, Some(0));
    }

    #[test]
    fn table_snapshot_honba_and_kyotaku_points_are_unknown_before_kyokustart() {
        // gamestart 直後は 0 本場 / 供託 0 点と断定できない。
        let mut state = ChiihouMatchState::new();
        state.apply(&gamestart());
        state.apply(&kyokustart(1, 1000));
        state.apply(&ChiihouLifecycleNotification::KyokuEnd);
        state.apply(&gamestart());

        let snapshot = state.table_snapshot(&ai_pubkey());
        assert_eq!(snapshot.honba, None);
        assert_eq!(snapshot.kyotaku_points, None);
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
        assert_eq!(snapshot.seat_wind, None);
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
        let snapshot = state.table_snapshot(&ai_pubkey());
        assert_eq!(snapshot.oya, None);
        assert_eq!(snapshot.seat_wind, None);
    }

    #[test]
    fn seat_wind_for_ai_index_zero_covers_all_dealers() {
        for (oya, expected) in [
            (0, ChiihouWind::East),
            (1, ChiihouWind::North),
            (2, ChiihouWind::West),
            (3, ChiihouWind::South),
        ] {
            assert_eq!(
                seat_wind_from_player_and_dealer(0, oya),
                Some(expected),
                "oya: {oya}"
            );
        }
    }

    #[test]
    fn seat_wind_for_other_player_indexes() {
        assert_eq!(
            seat_wind_from_player_and_dealer(2, 1),
            Some(ChiihouWind::South)
        );
        assert_eq!(
            seat_wind_from_player_and_dealer(3, 3),
            Some(ChiihouWind::East)
        );
    }

    #[test]
    fn seat_wind_rejects_out_of_range_indexes() {
        assert_eq!(seat_wind_from_player_and_dealer(4, 0), None);
        assert_eq!(seat_wind_from_player_and_dealer(0, 4), None);
        assert_eq!(seat_wind_from_player_and_dealer(255, 0), None);
        assert_eq!(seat_wind_from_player_and_dealer(0, 255), None);
    }

    fn kyokustart_with_dealer(dealer_index: u64) -> ChiihouLifecycleNotification {
        ChiihouLifecycleNotification::KyokuStart {
            round_wind: ChiihouWind::East,
            dealer: player_pubkey(dealer_index),
            honba: 0,
            kyotaku_points: 0,
        }
    }

    #[test]
    fn snapshot_seat_wind_follows_initial_dealer() {
        let mut state = ChiihouMatchState::new();
        state.apply(&gamestart());
        assert_eq!(state.table_snapshot(&ai_pubkey()).seat_wind, None);
        state.apply(&kyokustart_with_dealer(1));
        assert_eq!(
            state.table_snapshot(&ai_pubkey()).seat_wind,
            Some(ChiihouWind::East)
        );
    }

    #[test]
    fn snapshot_seat_wind_changes_when_dealer_moves() {
        let mut state = ChiihouMatchState::new();
        state.apply(&gamestart());
        state.apply(&kyokustart_with_dealer(1));
        assert_eq!(
            state.table_snapshot(&ai_pubkey()).seat_wind,
            Some(ChiihouWind::East)
        );
        state.apply(&ChiihouLifecycleNotification::KyokuEnd);
        state.apply(&kyokustart_with_dealer(2));
        assert_eq!(
            state.table_snapshot(&ai_pubkey()).seat_wind,
            Some(ChiihouWind::North)
        );
    }

    #[test]
    fn snapshot_seat_wind_stays_when_dealer_repeats() {
        let mut state = ChiihouMatchState::new();
        state.apply(&gamestart());
        state.apply(&kyokustart_with_dealer(2));
        assert_eq!(
            state.table_snapshot(&ai_pubkey()).seat_wind,
            Some(ChiihouWind::North)
        );
        state.apply(&ChiihouLifecycleNotification::KyokuEnd);
        state.apply(&kyokustart_with_dealer(2));
        assert_eq!(
            state.table_snapshot(&ai_pubkey()).seat_wind,
            Some(ChiihouWind::North)
        );
    }

    #[test]
    fn snapshot_seat_wind_does_not_use_gamestart_seat() {
        let mut state = ChiihouMatchState::new();
        state.apply(&gamestart());
        state.apply(&kyokustart_with_dealer(1));
        assert_eq!(state.seat(), Some(ChiihouWind::South));
        assert_eq!(
            state.table_snapshot(&ai_pubkey()).seat_wind,
            Some(ChiihouWind::East)
        );
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
