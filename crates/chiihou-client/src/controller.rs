use nostr_sdk::PublicKey;

use crate::lifecycle::{CHIIHOU_PLAYER_COUNT, ChiihouLifecycleNotification};
use crate::match_state::ChiihouMatchState;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ChiihouLifecycleEffect {
    None,
    PublishNext,
    EndGame,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ChiihouLifecycleController {
    ai_pubkey: PublicKey,
    match_state: ChiihouMatchState,
    next_sent_for_current_kyoku: bool,
}

impl ChiihouLifecycleController {
    pub(crate) fn new(ai_pubkey: PublicKey) -> Self {
        Self {
            ai_pubkey,
            match_state: ChiihouMatchState::new(),
            next_sent_for_current_kyoku: false,
        }
    }

    pub(crate) fn apply(
        &mut self,
        notification: &ChiihouLifecycleNotification,
    ) -> ChiihouLifecycleEffect {
        self.match_state.apply(notification);
        match notification {
            ChiihouLifecycleNotification::GameStart { .. }
            | ChiihouLifecycleNotification::KyokuStart { .. } => {
                self.next_sent_for_current_kyoku = false;
                ChiihouLifecycleEffect::None
            }
            ChiihouLifecycleNotification::KyokuEnd => self.kyoku_end_effect(),
            ChiihouLifecycleNotification::GameEnd { .. } => ChiihouLifecycleEffect::EndGame,
        }
    }

    fn kyoku_end_effect(&mut self) -> ChiihouLifecycleEffect {
        if self.next_sent_for_current_kyoku {
            return ChiihouLifecycleEffect::None;
        }
        let Some(coordinator) = select_chiihou_next_coordinator(self.match_state.players()) else {
            tracing::warn!(
                player_count = self.match_state.players().len(),
                "cannot select chiihou next coordinator"
            );
            return ChiihouLifecycleEffect::None;
        };
        if coordinator != self.ai_pubkey {
            return ChiihouLifecycleEffect::None;
        }
        self.next_sent_for_current_kyoku = true;
        ChiihouLifecycleEffect::PublishNext
    }
}

pub(crate) fn select_chiihou_next_coordinator(players: &[PublicKey]) -> Option<PublicKey> {
    if players.len() != CHIIHOU_PLAYER_COUNT {
        return None;
    }
    players.iter().min().copied()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lifecycle::{ChiihouPlayerScore, ChiihouWind};
    use crate::match_state::ChiihouMatchPhase;
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

    fn min_player(players: &[PublicKey]) -> PublicKey {
        players
            .iter()
            .min_by_key(|player| player.to_hex())
            .copied()
            .unwrap()
    }

    fn gamestart(players: Vec<PublicKey>) -> ChiihouLifecycleNotification {
        ChiihouLifecycleNotification::GameStart {
            seat: ChiihouWind::East,
            players,
        }
    }

    fn kyokustart() -> ChiihouLifecycleNotification {
        ChiihouLifecycleNotification::KyokuStart {
            round_wind: ChiihouWind::East,
            dealer: player_pubkey(1),
            honba: 0,
            kyotaku_points: 0,
        }
    }

    fn gameend() -> ChiihouLifecycleNotification {
        ChiihouLifecycleNotification::GameEnd {
            scores: (1..=4)
                .map(|index| ChiihouPlayerScore {
                    player: player_pubkey(index),
                    score: 25000,
                })
                .collect(),
        }
    }

    fn coordinator_controller() -> ChiihouLifecycleController {
        let players = players(1..=4);
        let mut controller = ChiihouLifecycleController::new(min_player(&players));
        controller.apply(&gamestart(players));
        controller.apply(&kyokustart());
        controller
    }

    fn non_coordinator_controller() -> ChiihouLifecycleController {
        let players = players(1..=4);
        let non_coordinator = players
            .iter()
            .find(|player| **player != min_player(&players))
            .copied()
            .unwrap();
        let mut controller = ChiihouLifecycleController::new(non_coordinator);
        controller.apply(&gamestart(players));
        controller.apply(&kyokustart());
        controller
    }

    #[test]
    fn selects_exactly_one_coordinator_among_four_players() {
        let players = players(1..=4);
        let coordinator = select_chiihou_next_coordinator(&players).unwrap();
        let selected: Vec<PublicKey> = players
            .iter()
            .filter(|player| select_chiihou_next_coordinator(&players) == Some(**player))
            .copied()
            .collect();
        assert_eq!(selected, vec![coordinator]);
    }

    #[test]
    fn coordinator_does_not_depend_on_player_order() {
        let base = players(1..=4);
        let expected = select_chiihou_next_coordinator(&base);
        let reordered = vec![base[2], base[0], base[3], base[1]];
        assert_eq!(select_chiihou_next_coordinator(&reordered), expected);
    }

    #[test]
    fn coordinator_is_lexicographically_smallest_hex_pubkey() {
        let players = players([7, 3, 9, 5]);
        assert_eq!(
            select_chiihou_next_coordinator(&players),
            Some(min_player(&players))
        );
    }

    #[test]
    fn no_coordinator_for_empty_players() {
        assert_eq!(select_chiihou_next_coordinator(&[]), None);
    }

    #[test]
    fn no_coordinator_for_wrong_player_count() {
        assert_eq!(select_chiihou_next_coordinator(&players(1..=3)), None);
        assert_eq!(select_chiihou_next_coordinator(&players(1..=5)), None);
    }

    #[test]
    fn ai_outside_player_list_is_not_coordinator() {
        let players = players(2..=5);
        let outsider = player_pubkey(1);
        assert!(!players.contains(&outsider));
        assert_ne!(select_chiihou_next_coordinator(&players), Some(outsider));
    }

    #[test]
    fn gamestart_holds_players_in_match_state() {
        let mut controller = ChiihouLifecycleController::new(player_pubkey(1));
        let effect = controller.apply(&gamestart(players(1..=4)));
        assert_eq!(effect, ChiihouLifecycleEffect::None);
        assert_eq!(controller.match_state.players(), players(1..=4));
    }

    #[test]
    fn coordinator_kyokuend_publishes_next() {
        let mut controller = coordinator_controller();
        assert_eq!(
            controller.apply(&ChiihouLifecycleNotification::KyokuEnd),
            ChiihouLifecycleEffect::PublishNext
        );
    }

    #[test]
    fn non_coordinator_kyokuend_does_not_publish_next() {
        let mut controller = non_coordinator_controller();
        assert_eq!(
            controller.apply(&ChiihouLifecycleNotification::KyokuEnd),
            ChiihouLifecycleEffect::None
        );
    }

    #[test]
    fn second_kyokuend_in_same_kyoku_does_not_publish_next() {
        let mut controller = coordinator_controller();
        assert_eq!(
            controller.apply(&ChiihouLifecycleNotification::KyokuEnd),
            ChiihouLifecycleEffect::PublishNext
        );
        assert_eq!(
            controller.apply(&ChiihouLifecycleNotification::KyokuEnd),
            ChiihouLifecycleEffect::None
        );
    }

    #[test]
    fn kyokustart_resets_next_sent_state() {
        let mut controller = coordinator_controller();
        controller.apply(&ChiihouLifecycleNotification::KyokuEnd);
        assert_eq!(
            controller.apply(&kyokustart()),
            ChiihouLifecycleEffect::None
        );
        assert_eq!(
            controller.apply(&ChiihouLifecycleNotification::KyokuEnd),
            ChiihouLifecycleEffect::PublishNext
        );
    }

    #[test]
    fn kyokuend_without_players_does_not_publish_next() {
        let mut controller = ChiihouLifecycleController::new(player_pubkey(1));
        assert_eq!(
            controller.apply(&ChiihouLifecycleNotification::KyokuEnd),
            ChiihouLifecycleEffect::None
        );
    }

    #[test]
    fn gameend_requests_end_game() {
        let mut controller = coordinator_controller();
        assert_eq!(
            controller.apply(&gameend()),
            ChiihouLifecycleEffect::EndGame
        );
    }

    #[test]
    fn gameend_does_not_publish_next() {
        let mut controller = coordinator_controller();
        assert_ne!(
            controller.apply(&gameend()),
            ChiihouLifecycleEffect::PublishNext
        );
    }

    #[test]
    fn phase_after_gameend_is_ended() {
        let mut controller = coordinator_controller();
        controller.apply(&gameend());
        assert_eq!(controller.match_state.phase(), ChiihouMatchPhase::Ended);
    }
}
