use crate::action::LegalAction;
use crate::context::GameContext;

pub trait Agent {
    fn act(&mut self, ctx: &GameContext, legal_actions: &[LegalAction]) -> LegalAction;
}

#[cfg(test)]
mod tests {
    use super::*;

    struct FirstActionAgent;

    impl Agent for FirstActionAgent {
        fn act(&mut self, _ctx: &GameContext, legal_actions: &[LegalAction]) -> LegalAction {
            legal_actions.first().cloned().unwrap_or(LegalAction::None)
        }
    }

    #[test]
    fn agent_picks_from_legal_actions() {
        let mut agent = FirstActionAgent;
        let ctx = GameContext::default();
        let actions = vec![LegalAction::Hora, LegalAction::None];
        assert_eq!(agent.act(&ctx, &actions), LegalAction::Hora);
        assert_eq!(agent.act(&ctx, &[]), LegalAction::None);
    }

    #[test]
    fn dahai_action_holds_tile_id() {
        let tile = bot_logic::TileId::new(16).unwrap();
        let action = LegalAction::Dahai { tile };
        assert_eq!(action, LegalAction::Dahai { tile });
    }
}
