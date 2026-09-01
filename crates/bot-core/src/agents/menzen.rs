use crate::action::LegalAction;
use crate::agent::Agent;
use crate::agents::shanten::ShantenAgent;
use crate::context::GameContext;

#[derive(Debug, Default)]
pub struct MenzenAgent {
    inner: ShantenAgent,
}

fn is_menzen_compatible_action(action: &LegalAction) -> bool {
    match action {
        LegalAction::Hora
        | LegalAction::Ryukyoku
        | LegalAction::Reach
        | LegalAction::Dahai { .. }
        | LegalAction::Ankan { .. }
        | LegalAction::None => true,
        LegalAction::Chi { .. }
        | LegalAction::Pon { .. }
        | LegalAction::Daiminkan { .. }
        | LegalAction::Kakan { .. } => false,
    }
}

fn menzen_compatible_actions(legal_actions: &[LegalAction]) -> Vec<LegalAction> {
    legal_actions
        .iter()
        .filter(|action| is_menzen_compatible_action(action))
        .cloned()
        .collect()
}

impl Agent for MenzenAgent {
    fn act(&mut self, ctx: &GameContext, legal_actions: &[LegalAction]) -> LegalAction {
        self.inner
            .act(ctx, &menzen_compatible_actions(legal_actions))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agents::shanten::tests::{
        CallReaction, TENPAI_SCARCE_VISIBLE, dahai, opponent_reach_context, ryukyoku_actions,
        tenpai_actions, tenpai_context, tile,
    };
    use crate::ryukyoku_decision::tests::{
        KOKUSHI_FOUR_HAND, KOKUSHI_THREE_HAND, context_from_hand,
    };

    fn chi() -> LegalAction {
        LegalAction::Chi {
            tile: tile(17),
            consumed: vec![tile(12), tile(20)],
        }
    }

    fn pon() -> LegalAction {
        LegalAction::Pon {
            tile: tile(108),
            consumed: vec![tile(109), tile(110)],
        }
    }

    fn daiminkan() -> LegalAction {
        LegalAction::Daiminkan {
            tile: tile(104),
            consumed: vec![tile(105), tile(106), tile(107)],
        }
    }

    fn ankan() -> LegalAction {
        LegalAction::Ankan {
            consumed: vec![tile(72), tile(73), tile(74), tile(75)],
        }
    }

    fn kakan() -> LegalAction {
        LegalAction::Kakan {
            tile: tile(124),
            consumed: vec![tile(125), tile(126), tile(127)],
        }
    }

    #[test]
    fn menzen_compatible_actions_are_allowed() {
        for action in [
            LegalAction::Hora,
            LegalAction::Ryukyoku,
            LegalAction::Reach,
            dahai(0),
            LegalAction::None,
            ankan(),
        ] {
            assert!(
                is_menzen_compatible_action(&action),
                "expected menzen compatible: {action:?}"
            );
        }
    }

    #[test]
    fn open_meld_actions_are_not_menzen_compatible() {
        for action in [chi(), pon(), daiminkan(), kakan()] {
            assert!(
                !is_menzen_compatible_action(&action),
                "expected not menzen compatible: {action:?}"
            );
        }
    }

    #[test]
    fn filter_drops_chi_before_delegating() {
        let actions = vec![chi(), LegalAction::None];
        assert_eq!(menzen_compatible_actions(&actions), vec![LegalAction::None]);

        let mut agent = MenzenAgent::default();
        let ctx = GameContext::default();
        assert_eq!(agent.act(&ctx, &actions), LegalAction::None);
    }

    #[test]
    fn filter_drops_pon_before_delegating() {
        let actions = vec![pon(), LegalAction::None];
        assert_eq!(menzen_compatible_actions(&actions), vec![LegalAction::None]);

        let mut agent = MenzenAgent::default();
        let ctx = GameContext::default();
        assert_eq!(agent.act(&ctx, &actions), LegalAction::None);
    }

    #[test]
    fn filter_drops_daiminkan_before_delegating() {
        let actions = vec![daiminkan(), LegalAction::None];
        assert_eq!(menzen_compatible_actions(&actions), vec![LegalAction::None]);

        let mut agent = MenzenAgent::default();
        let ctx = GameContext::default();
        assert_eq!(agent.act(&ctx, &actions), LegalAction::None);
    }

    #[test]
    fn filter_drops_kakan_before_delegating() {
        let actions = vec![kakan(), dahai(0)];
        assert_eq!(menzen_compatible_actions(&actions), vec![dahai(0)]);

        let mut agent = MenzenAgent::default();
        let ctx = GameContext::with_drawn_tile(tile(0));
        assert_eq!(agent.act(&ctx, &actions), dahai(0));
    }

    #[test]
    fn filter_keeps_ankan() {
        let actions = vec![ankan(), dahai(0)];
        assert_eq!(menzen_compatible_actions(&actions), actions);
    }

    #[test]
    fn filter_drops_every_open_meld_action_at_once() {
        let actions = vec![
            chi(),
            pon(),
            daiminkan(),
            kakan(),
            ankan(),
            dahai(0),
            LegalAction::Reach,
            LegalAction::Hora,
            LegalAction::Ryukyoku,
            LegalAction::None,
        ];
        assert_eq!(
            menzen_compatible_actions(&actions),
            vec![
                ankan(),
                dahai(0),
                LegalAction::Reach,
                LegalAction::Hora,
                LegalAction::Ryukyoku,
                LegalAction::None,
            ]
        );
    }

    #[test]
    fn keeps_hora_even_when_pon_is_legal() {
        let mut agent = MenzenAgent::default();
        let ctx = GameContext::default();
        let actions = vec![pon(), LegalAction::Hora, LegalAction::None];
        assert_eq!(agent.act(&ctx, &actions), LegalAction::Hora);
    }

    #[test]
    fn keeps_reach_even_when_pon_is_legal() {
        let mut agent = MenzenAgent::default();
        let mut shanten = ShantenAgent;
        let ctx = tenpai_context(&[]);
        let menzen_only = tenpai_actions();
        assert_eq!(shanten.act(&ctx, &menzen_only), LegalAction::Reach);

        let actions: Vec<LegalAction> = std::iter::once(pon()).chain(menzen_only).collect();
        assert_eq!(agent.act(&ctx, &actions), LegalAction::Reach);
    }

    #[test]
    fn matches_shanten_agent_on_normal_discard() {
        let mut agent = MenzenAgent::default();
        let mut shanten = ShantenAgent;
        let ctx = tenpai_context(&TENPAI_SCARCE_VISIBLE);
        let actions = tenpai_actions();

        let expected = shanten.act(&ctx, &actions);
        assert!(matches!(expected, LegalAction::Dahai { .. }));
        assert_eq!(agent.act(&ctx, &actions), expected);
    }

    #[test]
    fn matches_shanten_agent_on_hora() {
        let mut agent = MenzenAgent::default();
        let mut shanten = ShantenAgent;
        let ctx = tenpai_context(&[]);
        let actions: Vec<LegalAction> = tenpai_actions()
            .into_iter()
            .chain([LegalAction::Hora])
            .collect();

        let expected = shanten.act(&ctx, &actions);
        assert_eq!(expected, LegalAction::Hora);
        assert_eq!(agent.act(&ctx, &actions), expected);
    }

    #[test]
    fn matches_shanten_agent_on_reach() {
        let mut agent = MenzenAgent::default();
        let mut shanten = ShantenAgent;
        let ctx = tenpai_context(&[]);
        let actions = tenpai_actions();

        let expected = shanten.act(&ctx, &actions);
        assert_eq!(expected, LegalAction::Reach);
        assert_eq!(agent.act(&ctx, &actions), expected);
    }

    #[test]
    fn matches_shanten_agent_under_opponent_reach() {
        let mut agent = MenzenAgent::default();
        let mut shanten = ShantenAgent;
        let ctx = opponent_reach_context(Some(0), &[]);
        let actions = vec![LegalAction::Reach, dahai(0), dahai(16)];

        let expected = shanten.act(&ctx, &actions);
        assert_eq!(expected, dahai(16));
        assert_eq!(agent.act(&ctx, &actions), expected);
    }

    #[test]
    fn keeps_none_where_shanten_agent_pons_a_value_honor_pair() {
        // 123456m 55p 78s N PP に他家が P を捨てた局面。ShantenAgent は Pon するが、
        // MenzenAgent は Pon を除外するので None を維持する。
        let reaction = CallReaction::pon(
            &[0, 4, 8, 12, 17, 20, 53, 54, 96, 100, 120, 124, 125],
            126,
            &[124, 125],
        );
        let ctx = reaction.context();
        let actions = reaction.actions();

        let mut shanten = ShantenAgent;
        assert_eq!(shanten.act(&ctx, &actions), reaction.call());
        assert_eq!(
            ShantenAgent::diagnose(&ctx, &actions).selected_source,
            crate::AgentActionSource::Call
        );

        let mut agent = MenzenAgent::default();
        assert_eq!(menzen_compatible_actions(&actions), vec![LegalAction::None]);
        assert_eq!(agent.act(&ctx, &actions), LegalAction::None);
    }

    #[test]
    fn falls_back_to_none_for_empty_actions() {
        let mut agent = MenzenAgent::default();
        let ctx = GameContext::default();
        assert_eq!(agent.act(&ctx, &[]), LegalAction::None);
    }

    // 九種九牌の宣言 / 続行 policy は ShantenAgent が持ち、MenzenAgent は委譲するだけで
    // 同じ結論になる。判断 logic を複製しない。
    #[test]
    fn delegates_the_ryukyoku_decision_to_the_shanten_agent() {
        for hand in [&KOKUSHI_FOUR_HAND, &KOKUSHI_THREE_HAND] {
            let ctx = context_from_hand(hand);
            let actions = ryukyoku_actions(&ctx);

            let mut menzen = MenzenAgent::default();
            let mut shanten = ShantenAgent;
            assert_eq!(
                menzen.act(&ctx, &actions),
                shanten.act(&ctx, &actions),
                "{hand:?}"
            );
        }
    }

    #[test]
    fn declares_ryukyoku_only_on_the_far_hand() {
        let mut agent = MenzenAgent::default();

        let far = context_from_hand(&KOKUSHI_FOUR_HAND);
        assert_eq!(
            agent.act(&far, &ryukyoku_actions(&far)),
            LegalAction::Ryukyoku
        );

        let close = context_from_hand(&KOKUSHI_THREE_HAND);
        assert_ne!(
            agent.act(&close, &ryukyoku_actions(&close)),
            LegalAction::Ryukyoku
        );
    }
}
