use crate::action::LegalAction;
use crate::agent::Agent;
use crate::context::GameContext;

#[derive(Debug, Default)]
pub struct NormalAgent;

impl Agent for NormalAgent {
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

        if let Some(action) = legal_actions
            .iter()
            .find(|a| matches!(a, LegalAction::Reach))
        {
            return action.clone();
        }

        if let Some(drawn_tile) = ctx.drawn_tile()
            && let Some(action) = legal_actions
                .iter()
                .find(|a| matches!(a, LegalAction::Dahai { tile } if *tile == drawn_tile))
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
    fn picks_hora_when_present() {
        let mut agent = NormalAgent;
        let ctx = GameContext::default();
        let actions = vec![LegalAction::None, LegalAction::Hora];
        assert_eq!(agent.act(&ctx, &actions), LegalAction::Hora);
    }

    #[test]
    fn picks_ryukyoku_when_present() {
        let mut agent = NormalAgent;
        let ctx = GameContext::default();
        let actions = vec![LegalAction::None, LegalAction::Ryukyoku];
        assert_eq!(agent.act(&ctx, &actions), LegalAction::Ryukyoku);
    }

    #[test]
    fn picks_reach_when_present() {
        let mut agent = NormalAgent;
        let ctx = GameContext::default();
        let actions = vec![LegalAction::None, LegalAction::Reach];
        assert_eq!(agent.act(&ctx, &actions), LegalAction::Reach);
    }

    #[test]
    fn prefers_hora_over_reach_and_dahai() {
        let mut agent = NormalAgent;
        let ctx = GameContext::with_drawn_tile(tile(16));
        let actions = vec![dahai(16), LegalAction::Reach, LegalAction::Hora];
        assert_eq!(agent.act(&ctx, &actions), LegalAction::Hora);
    }

    #[test]
    fn prefers_reach_over_dahai() {
        let mut agent = NormalAgent;
        let ctx = GameContext::with_drawn_tile(tile(16));
        let actions = vec![dahai(16), LegalAction::Reach];
        assert_eq!(agent.act(&ctx, &actions), LegalAction::Reach);
    }

    #[test]
    fn prefers_matching_dahai_over_other_dahai() {
        let mut agent = NormalAgent;
        let ctx = GameContext::with_drawn_tile(tile(16));
        let actions = vec![dahai(0), dahai(16), dahai(32)];
        assert_eq!(agent.act(&ctx, &actions), dahai(16));
    }

    #[test]
    fn picks_first_dahai_without_drawn_tile() {
        let mut agent = NormalAgent;
        let ctx = GameContext::default();
        let actions = vec![dahai(0), dahai(16)];
        assert_eq!(agent.act(&ctx, &actions), dahai(0));
    }

    #[test]
    fn picks_first_dahai_when_no_matching_dahai() {
        let mut agent = NormalAgent;
        let ctx = GameContext::with_drawn_tile(tile(16));
        let actions = vec![dahai(0), dahai(32)];
        assert_eq!(agent.act(&ctx, &actions), dahai(0));
    }

    #[test]
    fn picks_none_when_no_dahai() {
        let mut agent = NormalAgent;
        let ctx = GameContext::default();
        let actions = vec![LegalAction::None];
        assert_eq!(agent.act(&ctx, &actions), LegalAction::None);
    }

    #[test]
    fn falls_back_to_none_for_empty_actions() {
        let mut agent = NormalAgent;
        let ctx = GameContext::default();
        assert_eq!(agent.act(&ctx, &[]), LegalAction::None);
    }
}
