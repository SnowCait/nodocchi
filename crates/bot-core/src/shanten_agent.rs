use crate::action::LegalAction;
use crate::agent::Agent;
use crate::context::GameContext;
use crate::discard_selection::select_discard_action;

#[derive(Debug, Default)]
pub struct ShantenAgent;

impl Agent for ShantenAgent {
    fn act(&mut self, ctx: &GameContext, legal_actions: &[LegalAction]) -> LegalAction {
        if let Some(action) = legal_actions
            .iter()
            .find(|a| matches!(a, LegalAction::Hora))
        {
            return action.clone();
        }

        if let Some(action) = legal_actions
            .iter()
            .find(|a| matches!(a, LegalAction::Ryukyoku))
        {
            return action.clone();
        }

        if let Some(action) = select_discard_action(ctx, legal_actions) {
            return action;
        }

        if let Some(action) = legal_actions
            .iter()
            .find(|a| matches!(a, LegalAction::Reach))
        {
            return action.clone();
        }

        if let Some(action) = legal_actions
            .iter()
            .find(|a| matches!(a, LegalAction::Dahai { .. }))
        {
            return action.clone();
        }

        if let Some(action) = legal_actions
            .iter()
            .find(|a| matches!(a, LegalAction::None))
        {
            return action.clone();
        }

        LegalAction::None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bot_logic::TileId;

    fn tile(value: u8) -> TileId {
        TileId::new(value).unwrap()
    }

    fn dahai(value: u8) -> LegalAction {
        LegalAction::Dahai { tile: tile(value) }
    }

    #[test]
    fn picks_hora_first() {
        let mut agent = ShantenAgent;
        let ctx = GameContext::with_drawn_tile(tile(0));
        let actions = vec![dahai(0), LegalAction::Hora];
        assert_eq!(agent.act(&ctx, &actions), LegalAction::Hora);
    }

    #[test]
    fn prefers_ryukyoku_over_dahai() {
        let mut agent = ShantenAgent;
        let ctx = GameContext::with_drawn_tile(tile(0));
        let actions = vec![dahai(0), LegalAction::Ryukyoku];
        assert_eq!(agent.act(&ctx, &actions), LegalAction::Ryukyoku);
    }

    #[test]
    fn picks_dahai_by_discard_evaluation() {
        let mut agent = ShantenAgent;
        let hand_values = [0, 4, 8, 12, 17, 20, 24, 28, 32, 36, 40, 44, 89];
        let ctx = GameContext::from_parts(
            Some(tile(116)),
            hand_values.iter().map(|&value| tile(value)).collect(),
        );
        let actions: Vec<LegalAction> = hand_values
            .iter()
            .map(|&value| dahai(value))
            .chain([dahai(116)])
            .collect();

        let expected = select_discard_action(&ctx, &actions).unwrap();

        assert_eq!(agent.act(&ctx, &actions), expected);
    }

    #[test]
    fn prefers_dahai_over_reach_when_evaluable() {
        let mut agent = ShantenAgent;
        let ctx = GameContext::with_drawn_tile(tile(0));
        let actions = vec![LegalAction::Reach, dahai(0)];
        assert_eq!(agent.act(&ctx, &actions), dahai(0));
    }

    #[test]
    fn falls_back_to_reach_when_discard_selection_returns_none() {
        let mut agent = ShantenAgent;
        let ctx = GameContext::default();
        let actions = vec![LegalAction::Reach];
        assert_eq!(agent.act(&ctx, &actions), LegalAction::Reach);
    }

    #[test]
    fn falls_back_to_first_dahai_without_reach() {
        let mut agent = ShantenAgent;
        let ctx = GameContext::default();
        let actions = vec![dahai(4), dahai(0)];
        assert_eq!(agent.act(&ctx, &actions), dahai(4));
    }

    #[test]
    fn picks_none_when_no_dahai() {
        let mut agent = ShantenAgent;
        let ctx = GameContext::default();
        let actions = vec![LegalAction::None];
        assert_eq!(agent.act(&ctx, &actions), LegalAction::None);
    }

    #[test]
    fn falls_back_to_none_for_empty_actions() {
        let mut agent = ShantenAgent;
        let ctx = GameContext::default();
        assert_eq!(agent.act(&ctx, &[]), LegalAction::None);
    }

    #[test]
    fn follows_discard_selection_for_same_tile_type() {
        let mut agent = ShantenAgent;
        let ctx = GameContext::from_parts(Some(tile(16)), vec![tile(17)]);
        let actions = vec![dahai(17), dahai(16)];

        let expected = select_discard_action(&ctx, &actions).unwrap();

        assert_eq!(agent.act(&ctx, &actions), expected);
    }
}
