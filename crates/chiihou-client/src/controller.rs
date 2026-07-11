use nostr_sdk::PublicKey;

use crate::lifecycle::ChiihouLifecycleNotification;
use crate::match_state::{ChiihouMatchState, ChiihouTableSnapshot, ChiihouTableStateError};
use crate::table_notification::ChiihouTableNotification;

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
    auto_next: bool,
    next_sent_for_current_kyoku: bool,
}

impl ChiihouLifecycleController {
    pub(crate) fn new(ai_pubkey: PublicKey, auto_next: bool) -> Self {
        Self {
            ai_pubkey,
            match_state: ChiihouMatchState::new(),
            auto_next,
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

    pub(crate) fn table_snapshot(&self) -> ChiihouTableSnapshot {
        self.match_state.table_snapshot(&self.ai_pubkey)
    }

    pub(crate) fn apply_table_notification(
        &mut self,
        notification: &ChiihouTableNotification,
    ) -> Result<(), ChiihouTableStateError> {
        self.match_state
            .apply_table_notification(&self.ai_pubkey, notification)
    }

    fn kyoku_end_effect(&mut self) -> ChiihouLifecycleEffect {
        if !self.auto_next || self.next_sent_for_current_kyoku {
            return ChiihouLifecycleEffect::None;
        }
        self.next_sent_for_current_kyoku = true;
        ChiihouLifecycleEffect::PublishNext
    }
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

    fn controller_in_kyoku(ai_index: u64, auto_next: bool) -> ChiihouLifecycleController {
        let mut controller = ChiihouLifecycleController::new(player_pubkey(ai_index), auto_next);
        controller.apply(&gamestart(players(1..=4)));
        controller.apply(&kyokustart());
        controller
    }

    #[test]
    fn gamestart_holds_players_in_match_state() {
        let mut controller = ChiihouLifecycleController::new(player_pubkey(1), false);
        let effect = controller.apply(&gamestart(players(1..=4)));
        assert_eq!(effect, ChiihouLifecycleEffect::None);
        assert_eq!(controller.match_state.players(), players(1..=4));
    }

    #[test]
    fn kyokuend_with_auto_next_disabled_is_none() {
        let mut controller = controller_in_kyoku(1, false);
        assert_eq!(
            controller.apply(&ChiihouLifecycleNotification::KyokuEnd),
            ChiihouLifecycleEffect::None
        );
    }

    #[test]
    fn repeated_kyokuend_with_auto_next_disabled_is_always_none() {
        let mut controller = controller_in_kyoku(1, false);
        for _ in 0..3 {
            assert_eq!(
                controller.apply(&ChiihouLifecycleNotification::KyokuEnd),
                ChiihouLifecycleEffect::None
            );
        }
    }

    #[test]
    fn kyokuend_with_auto_next_disabled_updates_phase() {
        let mut controller = controller_in_kyoku(1, false);
        controller.apply(&ChiihouLifecycleNotification::KyokuEnd);
        assert_eq!(
            controller.match_state.phase(),
            ChiihouMatchPhase::WaitingNext
        );
    }

    #[test]
    fn kyokuend_with_auto_next_enabled_publishes_next() {
        let mut controller = controller_in_kyoku(1, true);
        assert_eq!(
            controller.apply(&ChiihouLifecycleNotification::KyokuEnd),
            ChiihouLifecycleEffect::PublishNext
        );
    }

    #[test]
    fn all_auto_next_clients_publish_next_regardless_of_pubkey_order() {
        for ai_index in 1..=4 {
            let mut controller = controller_in_kyoku(ai_index, true);
            assert_eq!(
                controller.apply(&ChiihouLifecycleNotification::KyokuEnd),
                ChiihouLifecycleEffect::PublishNext,
                "ai_index: {ai_index}"
            );
        }
    }

    #[test]
    fn only_auto_next_enabled_clients_publish_next() {
        let expectations = [
            (1, true, ChiihouLifecycleEffect::PublishNext),
            (2, false, ChiihouLifecycleEffect::None),
            (3, false, ChiihouLifecycleEffect::None),
            (4, true, ChiihouLifecycleEffect::PublishNext),
        ];
        for (ai_index, auto_next, expected) in expectations {
            let mut controller = controller_in_kyoku(ai_index, auto_next);
            assert_eq!(
                controller.apply(&ChiihouLifecycleNotification::KyokuEnd),
                expected,
                "ai_index: {ai_index}"
            );
        }
    }

    #[test]
    fn kyokuend_without_gamestart_publishes_next_when_auto_next_enabled() {
        let mut controller = ChiihouLifecycleController::new(player_pubkey(1), true);
        assert_eq!(
            controller.apply(&ChiihouLifecycleNotification::KyokuEnd),
            ChiihouLifecycleEffect::PublishNext
        );
    }

    #[test]
    fn kyokuend_with_three_players_publishes_next_when_auto_next_enabled() {
        let mut controller = ChiihouLifecycleController::new(player_pubkey(1), true);
        controller.apply(&gamestart(players(1..=3)));
        controller.apply(&kyokustart());
        assert_eq!(
            controller.apply(&ChiihouLifecycleNotification::KyokuEnd),
            ChiihouLifecycleEffect::PublishNext
        );
    }

    #[test]
    fn kyokuend_with_ai_outside_players_publishes_next_when_auto_next_enabled() {
        let outsider = player_pubkey(9);
        let listed = players(1..=4);
        assert!(!listed.contains(&outsider));
        let mut controller = ChiihouLifecycleController::new(outsider, true);
        controller.apply(&gamestart(listed));
        controller.apply(&kyokustart());
        assert_eq!(
            controller.apply(&ChiihouLifecycleNotification::KyokuEnd),
            ChiihouLifecycleEffect::PublishNext
        );
    }

    #[test]
    fn second_kyokuend_in_same_kyoku_does_not_publish_next() {
        let mut controller = controller_in_kyoku(1, true);
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
        let mut controller = controller_in_kyoku(1, true);
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
    fn gameend_requests_end_game_when_auto_next_disabled() {
        let mut controller = controller_in_kyoku(1, false);
        assert_eq!(
            controller.apply(&gameend()),
            ChiihouLifecycleEffect::EndGame
        );
    }

    #[test]
    fn gameend_requests_end_game_when_auto_next_enabled() {
        let mut controller = controller_in_kyoku(1, true);
        assert_eq!(
            controller.apply(&gameend()),
            ChiihouLifecycleEffect::EndGame
        );
    }

    #[test]
    fn gameend_does_not_publish_next() {
        let mut controller = controller_in_kyoku(1, true);
        assert_ne!(
            controller.apply(&gameend()),
            ChiihouLifecycleEffect::PublishNext
        );
    }

    #[test]
    fn phase_after_gameend_is_ended() {
        let mut controller = controller_in_kyoku(1, false);
        controller.apply(&gameend());
        assert_eq!(controller.match_state.phase(), ChiihouMatchPhase::Ended);
    }
}
