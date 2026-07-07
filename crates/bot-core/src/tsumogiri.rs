use crate::action::LegalAction;
use crate::agent::Agent;
use crate::context::GameContext;

#[derive(Debug, Default)]
pub struct TsumogiriAgent;

impl Agent for TsumogiriAgent {
    fn act(&mut self, ctx: &GameContext, legal_actions: &[LegalAction]) -> LegalAction {
        if let Some(drawn_tile) = ctx.drawn_tile()
            && let Some(action) = legal_actions
                .iter()
                .find(|a| matches!(a, LegalAction::Dahai { tile } if *tile == drawn_tile))
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
    fn discards_drawn_tile_when_matching_dahai_exists() {
        let mut agent = TsumogiriAgent;
        let ctx = GameContext::with_drawn_tile(tile(16));
        let actions = vec![dahai(0), dahai(16), dahai(32)];
        assert_eq!(agent.act(&ctx, &actions), dahai(16));
    }

    #[test]
    fn prefers_matching_dahai_over_hora() {
        let mut agent = TsumogiriAgent;
        let ctx = GameContext::with_drawn_tile(tile(16));
        let actions = vec![LegalAction::Hora, dahai(16)];
        assert_eq!(agent.act(&ctx, &actions), dahai(16));
    }

    #[test]
    fn prefers_matching_dahai_over_reach() {
        let mut agent = TsumogiriAgent;
        let ctx = GameContext::with_drawn_tile(tile(16));
        let actions = vec![LegalAction::Reach, dahai(16)];
        assert_eq!(agent.act(&ctx, &actions), dahai(16));
    }

    #[test]
    fn does_not_pick_mismatched_dahai() {
        let mut agent = TsumogiriAgent;
        let ctx = GameContext::with_drawn_tile(tile(16));
        let actions = vec![dahai(0), dahai(32)];
        assert_eq!(agent.act(&ctx, &actions), LegalAction::None);
    }

    #[test]
    fn does_not_pick_dahai_without_drawn_tile() {
        let mut agent = TsumogiriAgent;
        let ctx = GameContext::default();
        let actions = vec![dahai(0), dahai(16)];
        assert_eq!(agent.act(&ctx, &actions), LegalAction::None);
    }

    #[test]
    fn picks_none_when_no_matching_dahai() {
        let mut agent = TsumogiriAgent;
        let ctx = GameContext::with_drawn_tile(tile(16));
        let actions = vec![dahai(0), LegalAction::None];
        assert_eq!(agent.act(&ctx, &actions), LegalAction::None);
    }

    #[test]
    fn falls_back_to_none_when_no_matching_dahai_and_no_none() {
        let mut agent = TsumogiriAgent;
        let ctx = GameContext::with_drawn_tile(tile(16));
        let actions = vec![LegalAction::Hora, LegalAction::Reach];
        assert_eq!(agent.act(&ctx, &actions), LegalAction::None);
    }
}
