use crate::action::LegalAction;
use crate::agent::Agent;
use crate::context::GameContext;

#[derive(Debug, Default)]
pub struct AlwaysLegalAgent;

impl Agent for AlwaysLegalAgent {
    fn act(&mut self, _ctx: &GameContext, legal_actions: &[LegalAction]) -> LegalAction {
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
        legal_actions.first().cloned().unwrap_or(LegalAction::None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bot_logic::TileId;

    fn dahai() -> LegalAction {
        LegalAction::Dahai {
            tile: TileId::new(0).unwrap(),
        }
    }

    #[test]
    fn prefers_hora_over_everything() {
        let mut agent = AlwaysLegalAgent;
        let ctx = GameContext::default();
        let actions = vec![
            dahai(),
            LegalAction::Reach,
            LegalAction::Ryukyoku,
            LegalAction::Hora,
            LegalAction::None,
        ];
        assert_eq!(agent.act(&ctx, &actions), LegalAction::Hora);
    }

    #[test]
    fn prefers_ryukyoku_over_dahai() {
        let mut agent = AlwaysLegalAgent;
        let ctx = GameContext::default();
        let actions = vec![dahai(), LegalAction::Ryukyoku];
        assert_eq!(agent.act(&ctx, &actions), LegalAction::Ryukyoku);
    }

    #[test]
    fn prefers_reach_over_dahai() {
        let mut agent = AlwaysLegalAgent;
        let ctx = GameContext::default();
        let actions = vec![dahai(), LegalAction::Reach];
        assert_eq!(agent.act(&ctx, &actions), LegalAction::Reach);
    }

    #[test]
    fn picks_dahai_when_present() {
        let mut agent = AlwaysLegalAgent;
        let ctx = GameContext::default();
        let actions = vec![LegalAction::None, dahai()];
        assert_eq!(agent.act(&ctx, &actions), dahai());
    }

    #[test]
    fn returns_none_for_empty_actions() {
        let mut agent = AlwaysLegalAgent;
        let ctx = GameContext::default();
        assert_eq!(agent.act(&ctx, &[]), LegalAction::None);
    }
}
