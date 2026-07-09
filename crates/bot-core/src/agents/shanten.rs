use crate::action::LegalAction;
use crate::agent::Agent;
use crate::context::GameContext;
use crate::defense::select_genbutsu_fallback_action;
use crate::discard_selection::select_discard_action;
use bot_logic::{TileCounts, calculate_acceptance_with_visible_tiles};

// 補正後の待ち枚数がこの枚数以上ならリーチする。
const REACH_MIN_REMAINING: u8 = 4;

#[derive(Debug, Default)]
pub struct ShantenAgent;

impl ShantenAgent {
    fn select_reach_action(
        &self,
        ctx: &GameContext,
        legal_actions: &[LegalAction],
    ) -> Option<LegalAction> {
        if !should_reach(ctx) {
            return None;
        }
        legal_actions
            .iter()
            .find(|a| matches!(a, LegalAction::Reach))
            .cloned()
    }
}

// 補正後の待ち枚数が明らかに少ない即リーチだけを抑制する最小判断。
// TODO: 役判定・打点・押し引きを考慮したリーチ判断に置き換える。
fn should_reach(ctx: &GameContext) -> bool {
    let tiles: Vec<_> = ctx
        .hand_tiles()
        .iter()
        .copied()
        .chain(ctx.drawn_tile())
        .collect();

    // 手牌情報がない場合は従来挙動を維持する。
    if tiles.is_empty() {
        return true;
    }

    // visible_tiles がない場合は補正できないため従来挙動を維持する。
    if ctx.visible_tiles().is_empty() {
        return true;
    }

    let counts = TileCounts::from_tiles(tiles.iter().copied());
    let acceptance = calculate_acceptance_with_visible_tiles(&counts, ctx.visible_tiles());

    // テンパイしていないなら即リーチしない。
    if acceptance.current.min() != 0 {
        return false;
    }

    acceptance.total_remaining() >= REACH_MIN_REMAINING
}

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

        // 他家リーチ中に共通現物 Dahai があれば、攻撃的な Reach や通常評価より先に切る。
        if let Some(action) = select_genbutsu_fallback_action(ctx, legal_actions) {
            return action.clone();
        }

        if let Some(action) = self.select_reach_action(ctx, legal_actions) {
            return action;
        }

        if let Some(action) = select_discard_action(ctx, legal_actions) {
            return action;
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
    fn prefers_hora_over_reach() {
        let mut agent = ShantenAgent;
        let ctx = GameContext::with_drawn_tile(tile(0));
        let actions = vec![LegalAction::Reach, LegalAction::Hora];
        assert_eq!(agent.act(&ctx, &actions), LegalAction::Hora);
    }

    #[test]
    fn prefers_ryukyoku_over_reach() {
        let mut agent = ShantenAgent;
        let ctx = GameContext::with_drawn_tile(tile(0));
        let actions = vec![LegalAction::Reach, LegalAction::Ryukyoku];
        assert_eq!(agent.act(&ctx, &actions), LegalAction::Ryukyoku);
    }

    #[test]
    fn picks_reach_when_available() {
        let mut agent = ShantenAgent;
        let ctx = GameContext::default();
        let actions = vec![LegalAction::Reach];
        assert_eq!(agent.act(&ctx, &actions), LegalAction::Reach);
    }

    #[test]
    fn prefers_reach_over_evaluated_dahai() {
        let mut agent = ShantenAgent;
        let ctx = GameContext::with_drawn_tile(tile(0));
        let actions = vec![LegalAction::Reach, dahai(0)];
        assert_eq!(agent.act(&ctx, &actions), LegalAction::Reach);
    }

    #[test]
    fn reach_is_policy_choice_not_fallback() {
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
            .chain([LegalAction::Reach])
            .collect();

        assert!(select_discard_action(&ctx, &actions).is_some());
        assert_eq!(agent.act(&ctx, &actions), LegalAction::Reach);
    }

    #[test]
    fn picks_dahai_when_reach_absent() {
        let mut agent = ShantenAgent;
        let ctx = GameContext::with_drawn_tile(tile(0));
        let actions = vec![dahai(0)];
        assert_eq!(agent.act(&ctx, &actions), dahai(0));
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
    fn does_not_actively_claim_melds_or_kans() {
        let mut agent = ShantenAgent;
        let ctx = GameContext::default();
        let actions = vec![
            LegalAction::Chi {
                tile: tile(17),
                consumed: vec![tile(12), tile(20)],
            },
            LegalAction::Pon {
                tile: tile(108),
                consumed: vec![tile(109), tile(110)],
            },
            LegalAction::Daiminkan {
                tile: tile(104),
                consumed: vec![tile(105), tile(106), tile(107)],
            },
            LegalAction::Ankan {
                consumed: vec![tile(72), tile(73), tile(74), tile(75)],
            },
            LegalAction::Kakan {
                tile: tile(124),
                consumed: vec![tile(125), tile(126), tile(127)],
            },
            LegalAction::None,
        ];
        assert_eq!(agent.act(&ctx, &actions), LegalAction::None);
    }

    #[test]
    fn falls_back_to_none_for_empty_actions() {
        let mut agent = ShantenAgent;
        let ctx = GameContext::default();
        assert_eq!(agent.act(&ctx, &[]), LegalAction::None);
    }

    #[test]
    fn uses_visible_tiles_for_discard_evaluation() {
        let mut agent = ShantenAgent;
        let hand_values = [0, 4, 8, 12, 17, 20, 24, 28, 32, 48, 53, 56, 36];
        let hand: Vec<_> = hand_values.iter().map(|&value| tile(value)).collect();
        let mut visible = hand.clone();
        visible.extend([68, 69, 70, 71].iter().map(|&value| tile(value)));
        let ctx = GameContext::from_parts_with_visible_tiles(
            Some(tile(68)),
            hand,
            vec![],
            None,
            None,
            visible,
        );
        let actions: Vec<LegalAction> = hand_values
            .iter()
            .map(|&value| dahai(value))
            .chain([dahai(68)])
            .collect();

        let selected = agent.act(&ctx, &actions);
        let LegalAction::Dahai { tile } = selected else {
            panic!("expected dahai");
        };
        assert_eq!(tile.tile_type().to_mjai_string(), "9p");
    }

    // 4面子 + 1s + 9s のタンキ含みテンパイ形。捨て牌前提で待ちは {1s, 9s}。
    const TENPAI_HAND: [u8; 13] = [0, 4, 8, 12, 17, 20, 24, 28, 32, 36, 40, 44, 72];
    const TENPAI_DRAWN: u8 = 104;

    fn tenpai_context(extra_visible: &[u8]) -> GameContext {
        let hand: Vec<_> = TENPAI_HAND.iter().map(|&value| tile(value)).collect();
        let mut visible = hand.clone();
        visible.push(tile(TENPAI_DRAWN));
        visible.extend(extra_visible.iter().map(|&value| tile(value)));
        GameContext::from_parts_with_visible_tiles(
            Some(tile(TENPAI_DRAWN)),
            hand,
            vec![],
            None,
            None,
            visible,
        )
    }

    fn tenpai_actions() -> Vec<LegalAction> {
        TENPAI_HAND
            .iter()
            .map(|&value| dahai(value))
            .chain([dahai(TENPAI_DRAWN)])
            .chain([LegalAction::Reach])
            .collect()
    }

    #[test]
    fn reaches_when_visible_waits_are_plentiful() {
        let mut agent = ShantenAgent;
        let ctx = tenpai_context(&[]);
        assert_eq!(agent.act(&ctx, &tenpai_actions()), LegalAction::Reach);
    }

    #[test]
    fn skips_reach_when_visible_waits_are_scarce() {
        let mut agent = ShantenAgent;
        // 1s / 9s をそれぞれ2枚見せて待ち枚数を枯らす。
        let ctx = tenpai_context(&[73, 74, 105, 106]);
        let selected = agent.act(&ctx, &tenpai_actions());
        assert!(matches!(selected, LegalAction::Dahai { .. }));
    }

    #[test]
    fn reaches_when_visible_tiles_empty_even_with_hand() {
        let mut agent = ShantenAgent;
        let hand: Vec<_> = TENPAI_HAND.iter().map(|&value| tile(value)).collect();
        let ctx = GameContext::from_parts(Some(tile(TENPAI_DRAWN)), hand);
        assert_eq!(agent.act(&ctx, &tenpai_actions()), LegalAction::Reach);
    }

    #[test]
    fn reaches_without_hand_information() {
        let mut agent = ShantenAgent;
        let ctx = GameContext::default();
        assert_eq!(agent.act(&ctx, &[LegalAction::Reach]), LegalAction::Reach);
    }

    #[test]
    fn follows_discard_selection_for_same_tile_type() {
        let mut agent = ShantenAgent;
        let ctx = GameContext::from_parts(Some(tile(16)), vec![tile(17)]);
        let actions = vec![dahai(17), dahai(16)];

        let expected = select_discard_action(&ctx, &actions).unwrap();

        assert_eq!(agent.act(&ctx, &actions), expected);
    }

    // 他家(player 1)がリーチしており、その河に 16(5m) がある局面。自分は player 0。
    fn opponent_reach_context(drawn_tile: Option<u8>, hand_values: &[u8]) -> GameContext {
        let discards = [vec![], vec![tile(16)], vec![], vec![]];
        GameContext::from_parts_with_table_state(
            drawn_tile.map(tile),
            hand_values.iter().map(|&value| tile(value)).collect(),
            vec![],
            None,
            None,
            Vec::new(),
            Some(0),
            None,
            discards,
            [false, true, false, false],
        )
    }

    #[test]
    fn prefers_hora_over_genbutsu_fallback() {
        let mut agent = ShantenAgent;
        let ctx = opponent_reach_context(Some(0), &[]);
        let actions = vec![dahai(16), LegalAction::Hora];
        assert_eq!(agent.act(&ctx, &actions), LegalAction::Hora);
    }

    #[test]
    fn prefers_ryukyoku_over_genbutsu_fallback() {
        let mut agent = ShantenAgent;
        let ctx = opponent_reach_context(Some(0), &[]);
        let actions = vec![dahai(16), LegalAction::Ryukyoku];
        assert_eq!(agent.act(&ctx, &actions), LegalAction::Ryukyoku);
    }

    #[test]
    fn prefers_genbutsu_fallback_over_reach() {
        let mut agent = ShantenAgent;
        let ctx = opponent_reach_context(Some(0), &[]);
        let actions = vec![LegalAction::Reach, dahai(0), dahai(16)];
        assert_eq!(agent.act(&ctx, &actions), dahai(16));
    }

    #[test]
    fn prefers_genbutsu_fallback_over_discard_evaluation() {
        let mut agent = ShantenAgent;
        // 通常評価では別牌が選ばれ得る手牌だが、共通現物 16(5m) を優先して切る。
        // 手牌には 5m を含めず、共通現物が dahai(16) のみになるようにする。
        let hand_values = [0, 4, 8, 12, 13, 20, 24, 28, 32, 36, 40, 44, 89];
        let ctx = opponent_reach_context(Some(116), &hand_values);
        let actions: Vec<LegalAction> = hand_values
            .iter()
            .map(|&value| dahai(value))
            .chain([dahai(116), dahai(16)])
            .collect();
        assert_eq!(agent.act(&ctx, &actions), dahai(16));
    }

    #[test]
    fn falls_through_when_no_common_genbutsu_available() {
        let mut agent = ShantenAgent;
        // 他家リーチ中でも合法 Dahai に共通現物が無ければ従来の Reach 判断に進む。
        let ctx = opponent_reach_context(Some(0), &[]);
        let actions = vec![LegalAction::Reach, dahai(0), dahai(56)];
        assert_eq!(agent.act(&ctx, &actions), LegalAction::Reach);
    }

    #[test]
    fn keeps_normal_behavior_without_opponent_reach() {
        let mut agent = ShantenAgent;
        // 他家リーチが無ければ、現物相当の牌があっても従来の Reach を選ぶ。
        let discards = [vec![], vec![tile(16)], vec![], vec![]];
        let ctx = GameContext::from_parts_with_table_state(
            Some(tile(0)),
            vec![],
            vec![],
            None,
            None,
            Vec::new(),
            Some(0),
            None,
            discards,
            [false; 4],
        );
        let actions = vec![LegalAction::Reach, dahai(16)];
        assert_eq!(agent.act(&ctx, &actions), LegalAction::Reach);
    }

    #[test]
    fn does_not_claim_melds_even_under_opponent_reach() {
        let mut agent = ShantenAgent;
        // 他家リーチ中でも副露・カンは積極選択しない。共通現物も無い局面。
        let ctx = opponent_reach_context(None, &[]);
        let actions = vec![
            LegalAction::Pon {
                tile: tile(108),
                consumed: vec![tile(109), tile(110)],
            },
            LegalAction::Ankan {
                consumed: vec![tile(72), tile(73), tile(74), tile(75)],
            },
            LegalAction::None,
        ];
        assert_eq!(agent.act(&ctx, &actions), LegalAction::None);
    }

    #[test]
    fn prefers_genbutsu_fallback_over_none() {
        let mut agent = ShantenAgent;
        let ctx = opponent_reach_context(Some(0), &[]);
        let actions = vec![dahai(16), LegalAction::None];
        assert_eq!(agent.act(&ctx, &actions), dahai(16));
    }
}
