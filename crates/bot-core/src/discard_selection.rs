use crate::action::LegalAction;
use crate::context::GameContext;
use bot_logic::{TileCounts, select_best_discard};

pub fn select_discard_action(
    context: &GameContext,
    legal_actions: &[LegalAction],
) -> Option<LegalAction> {
    let counts = TileCounts::from_tiles(
        context
            .hand_tiles()
            .iter()
            .copied()
            .chain(context.drawn_tile()),
    );

    let selected_type = select_best_discard(&counts)?.discard;

    legal_actions
        .iter()
        .find(|action| {
            matches!(action, LegalAction::Dahai { tile } if tile.tile_type() == selected_type)
        })
        .cloned()
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
    fn returns_none_for_empty_legal_actions() {
        let context = GameContext::from_parts(Some(tile(0)), vec![tile(4)]);
        assert_eq!(select_discard_action(&context, &[]), None);
    }

    #[test]
    fn returns_none_without_dahai_action() {
        let context = GameContext::from_parts(Some(tile(0)), vec![tile(1)]);
        let actions = vec![LegalAction::Reach, LegalAction::None];
        assert_eq!(select_discard_action(&context, &actions), None);
    }

    #[test]
    fn returns_none_without_context_tiles() {
        let context = GameContext::default();
        let actions = vec![dahai(0)];
        assert_eq!(select_discard_action(&context, &actions), None);
    }

    #[test]
    fn returns_dahai_matching_best_discard() {
        let hand_values = [0, 4, 8, 12, 17, 20, 24, 28, 32, 36, 40, 44, 89];
        let context = GameContext::from_parts(
            Some(tile(116)),
            hand_values.iter().map(|&value| tile(value)).collect(),
        );
        let actions: Vec<LegalAction> = hand_values
            .iter()
            .map(|&value| dahai(value))
            .chain([dahai(116)])
            .collect();

        let selected_action = select_discard_action(&context, &actions).unwrap();

        let counts = TileCounts::from_tiles(
            context
                .hand_tiles()
                .iter()
                .copied()
                .chain(context.drawn_tile()),
        );
        let selected_type = select_best_discard(&counts).unwrap().discard;

        assert!(matches!(
            selected_action,
            LegalAction::Dahai { tile } if tile.tile_type() == selected_type
        ));
    }

    #[test]
    fn evaluates_drawn_tile() {
        let context = GameContext::with_drawn_tile(tile(0));
        let actions = vec![dahai(0)];
        assert_eq!(select_discard_action(&context, &actions), Some(dahai(0)));
    }

    #[test]
    fn evaluates_hand_tiles() {
        let context = GameContext::with_hand_tiles(vec![tile(0), tile(4), tile(8)]);
        let actions = vec![dahai(0), dahai(4), dahai(8)];
        assert!(matches!(
            select_discard_action(&context, &actions),
            Some(LegalAction::Dahai { .. })
        ));
    }

    #[test]
    fn returns_first_dahai_of_same_tile_type() {
        let context = GameContext::from_parts(Some(tile(16)), vec![tile(17)]);
        let actions = vec![dahai(17), dahai(16)];
        assert_eq!(select_discard_action(&context, &actions), Some(dahai(17)));
    }

    #[test]
    fn returns_none_when_selected_type_has_no_dahai() {
        let context = GameContext::with_hand_tiles(vec![tile(0)]);
        let actions = vec![dahai(4)];
        assert_eq!(select_discard_action(&context, &actions), None);
    }

    #[test]
    fn does_not_select_non_dahai_actions() {
        let context = GameContext::with_drawn_tile(tile(0));
        let actions = vec![
            LegalAction::Hora,
            LegalAction::Reach,
            LegalAction::Ryukyoku,
            LegalAction::None,
            dahai(0),
        ];
        assert_eq!(select_discard_action(&context, &actions), Some(dahai(0)));
    }
}
