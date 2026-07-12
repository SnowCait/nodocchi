use crate::action::LegalAction;
use crate::agent::Agent;
use crate::context::GameContext;
use crate::defense::{log_defense_fallback_decision, select_defense_fallback_action_with_kind};
use crate::discard_selection::select_discard_action_with_evaluation;
use crate::push_pull::{
    PushPullMode, decide_push_pull, log_push_pull_decision,
    push_pull_inputs_from_context_with_evaluation,
};
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

    // 押し引きモードに応じた action 選択。候補は必要になった時点でのみ計算する。
    //
    // - Push:    Reach → 通常打牌 → 防御 fallback
    // - Neutral: 通常打牌 → 防御 fallback(Reach は検討しない)
    // - Fold:    防御 fallback → 通常打牌(Reach は検討しない)
    fn select_action_for_push_pull_mode(
        &self,
        mode: PushPullMode,
        ctx: &GameContext,
        legal_actions: &[LegalAction],
        normal_discard: Option<&LegalAction>,
    ) -> Option<LegalAction> {
        match mode {
            PushPullMode::Push => {
                if let Some(action) = self.select_reach_action(ctx, legal_actions) {
                    return Some(action);
                }
                if let Some(action) = normal_discard {
                    return Some(action.clone());
                }
                self.select_defense_fallback(ctx, legal_actions)
            }
            PushPullMode::Neutral => {
                if let Some(action) = normal_discard {
                    return Some(action.clone());
                }
                self.select_defense_fallback(ctx, legal_actions)
            }
            PushPullMode::Fold => {
                if let Some(action) = self.select_defense_fallback(ctx, legal_actions) {
                    return Some(action);
                }
                normal_discard.cloned()
            }
        }
    }

    // 防御 fallback を採用する場合に、その理由を診断ログへ出しつつ action を返す。
    fn select_defense_fallback(
        &self,
        ctx: &GameContext,
        legal_actions: &[LegalAction],
    ) -> Option<LegalAction> {
        let (action, kind) = select_defense_fallback_action_with_kind(ctx, legal_actions)?;
        log_defense_fallback_decision(ctx, action, kind, legal_actions);
        Some(action.clone())
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

        // 通常打牌の evaluation と action を一度だけ取得し、その evaluation を
        // 押し引き入力にも共有して二重計算を避ける。
        let discard_selection = select_discard_action_with_evaluation(ctx, legal_actions);
        let inputs = push_pull_inputs_from_context_with_evaluation(
            ctx,
            discard_selection.evaluation.as_ref(),
        );
        let decision = decide_push_pull(&inputs);
        log_push_pull_decision(&decision, &inputs, discard_selection.action.as_ref());

        if let Some(action) = self.select_action_for_push_pull_mode(
            decision.mode,
            ctx,
            legal_actions,
            discard_selection.action.as_ref(),
        ) {
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
    use crate::defense::select_defense_fallback_action;
    use crate::discard_selection::{select_best_discard_evaluation, select_discard_action};
    use crate::push_pull::push_pull_inputs_from_context;
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
    fn neutral_prefers_normal_discard_over_genbutsu_fallback() {
        let mut agent = ShantenAgent;
        // 単独の子リーチに対する強い一向聴で Neutral。共通現物 16(5m) があっても
        // Neutral では通常打牌(浮いた 116(北))を防御 fallback より優先する。
        let hand_values = [0, 4, 8, 12, 13, 20, 24, 28, 32, 36, 40, 44, 89];
        let ctx = opponent_reach_context(Some(116), &hand_values);
        assert_eq!(
            decide_push_pull(&push_pull_inputs_from_context(&ctx)).mode,
            PushPullMode::Neutral
        );
        let actions: Vec<LegalAction> = hand_values
            .iter()
            .map(|&value| dahai(value))
            .chain([dahai(116), dahai(16)])
            .collect();
        let normal = select_discard_action(&ctx, &actions).unwrap();
        assert_eq!(agent.act(&ctx, &actions), normal);
        assert_ne!(agent.act(&ctx, &actions), dahai(16));
    }

    #[test]
    fn fold_without_common_genbutsu_falls_through_to_normal_discard() {
        let mut agent = ShantenAgent;
        // 他家リーチ中でも合法 Dahai に共通現物が無い Fold 局面。Reach は抑制し通常打牌へ進む。
        let ctx = opponent_reach_context(Some(0), &[]);
        assert_eq!(
            decide_push_pull(&push_pull_inputs_from_context(&ctx)).mode,
            PushPullMode::Fold
        );
        let actions = vec![LegalAction::Reach, dahai(0), dahai(56)];
        assert_eq!(agent.act(&ctx, &actions), dahai(0));
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

    // 他家(player 1)がリーチしており、その河に 16(5m) がある局面に visible_tiles を加える。
    fn opponent_reach_context_with_visible(
        drawn_tile: Option<u8>,
        hand_values: &[u8],
        visible_values: &[u8],
    ) -> GameContext {
        let discards = [vec![], vec![tile(16)], vec![], vec![]];
        GameContext::from_parts_with_table_state(
            drawn_tile.map(tile),
            hand_values.iter().map(|&value| tile(value)).collect(),
            vec![],
            None,
            None,
            visible_values.iter().map(|&value| tile(value)).collect(),
            Some(0),
            None,
            discards,
            [false, true, false, false],
        )
    }

    #[test]
    fn prefers_genbutsu_fallback_over_honor_safety_fallback() {
        let mut agent = ShantenAgent;
        // 共通現物 16(5m) と字牌 108(東) が両方合法でも、現物を優先する。
        let ctx = opponent_reach_context(Some(0), &[]);
        let actions = vec![dahai(108), dahai(16)];
        assert_eq!(agent.act(&ctx, &actions), dahai(16));
    }

    #[test]
    fn picks_safest_honor_dahai_when_no_common_genbutsu() {
        let mut agent = ShantenAgent;
        // 共通現物なし。東は2枚見え、南は0枚見え。より安全な東を切る。
        let ctx = opponent_reach_context_with_visible(Some(112), &[], &[108, 109]);
        let actions = vec![dahai(112), dahai(108)];
        assert_eq!(agent.act(&ctx, &actions), dahai(108));
    }

    #[test]
    fn prefers_honor_safety_fallback_over_reach() {
        let mut agent = ShantenAgent;
        // 共通現物なし。数牌と字牌が合法なら Reach より字牌を切る。
        let ctx = opponent_reach_context(Some(0), &[]);
        let actions = vec![LegalAction::Reach, dahai(0), dahai(108)];
        assert_eq!(agent.act(&ctx, &actions), dahai(108));
    }

    #[test]
    fn prefers_honor_safety_fallback_over_discard_evaluation() {
        let mut agent = ShantenAgent;
        // 通常評価では別牌が選ばれ得る手牌だが、共通現物がなければ字牌 108(東) を優先する。
        let hand_values = [0, 4, 8, 12, 13, 20, 24, 28, 32, 36, 40, 44, 89];
        let ctx = opponent_reach_context(Some(108), &hand_values);
        let actions: Vec<LegalAction> = hand_values
            .iter()
            .map(|&value| dahai(value))
            .chain([dahai(108)])
            .collect();
        assert_eq!(agent.act(&ctx, &actions), dahai(108));
    }

    #[test]
    fn fold_without_common_genbutsu_or_honor_falls_through_to_normal_discard() {
        let mut agent = ShantenAgent;
        // 共通現物も字牌 Dahai もなく、数牌も全て NoSafety の Fold 局面。
        // リーチ者の河は 16(5m) のみで、0(1m) / 56(6p) は無スジ・壁なしの NoSafety。
        // Reach を抑制し、防御牌が無いので通常打牌へ進む。
        let ctx = opponent_reach_context(Some(0), &[]);
        assert_eq!(
            decide_push_pull(&push_pull_inputs_from_context(&ctx)).mode,
            PushPullMode::Fold
        );
        let actions = vec![LegalAction::Reach, dahai(0), dahai(56)];
        assert_eq!(agent.act(&ctx, &actions), dahai(0));
    }

    #[test]
    fn does_not_use_honor_safety_fallback_without_opponent_reach() {
        let mut agent = ShantenAgent;
        // 他家リーチが無ければ、字牌が合法でも従来の Reach を選ぶ。
        let ctx = GameContext::from_parts_with_table_state(
            Some(tile(0)),
            vec![],
            vec![],
            None,
            None,
            Vec::new(),
            Some(0),
            None,
            [vec![], vec![], vec![], vec![]],
            [false; 4],
        );
        let actions = vec![LegalAction::Reach, dahai(108)];
        assert_eq!(agent.act(&ctx, &actions), LegalAction::Reach);
    }

    #[test]
    fn honor_safety_fallback_ignores_number_dahai() {
        let mut agent = ShantenAgent;
        // 数牌のみで字牌がなければ字牌 fallback は発動しない。Fold だが安全牌が無いので通常打牌へ進む。
        let ctx = opponent_reach_context(Some(0), &[]);
        assert_eq!(
            decide_push_pull(&push_pull_inputs_from_context(&ctx)).mode,
            PushPullMode::Fold
        );
        let actions = vec![LegalAction::Reach, dahai(0), dahai(56)];
        assert_eq!(agent.act(&ctx, &actions), dahai(0));
    }

    #[test]
    fn honor_safety_fallback_ignores_non_dahai_honor_actions() {
        let mut agent = ShantenAgent;
        // 字牌の Pon はあっても字牌 Dahai が無ければ fallback は発動しない。
        let ctx = opponent_reach_context(Some(0), &[]);
        let actions = vec![
            LegalAction::Pon {
                tile: tile(108),
                consumed: vec![tile(109), tile(110)],
            },
            LegalAction::None,
        ];
        assert_eq!(agent.act(&ctx, &actions), LegalAction::None);
    }

    #[test]
    fn honor_safety_fallback_preserves_order_within_same_rank() {
        let mut agent = ShantenAgent;
        // 東も南も0枚見えで同安全度なら legal_actions の元順序を保つ。
        let ctx = opponent_reach_context(Some(0), &[]);
        let actions = vec![dahai(112), dahai(108)];
        assert_eq!(agent.act(&ctx, &actions), dahai(112));
    }

    // 他家(player 1)がリーチしている局面。河・visible・手牌・引き牌を個別に指定する。
    fn suited_reach_context(
        drawn_tile: Option<u8>,
        hand_values: &[u8],
        visible_values: &[u8],
        reacher_discards: &[u8],
    ) -> GameContext {
        let discards = [
            vec![],
            reacher_discards.iter().map(|&value| tile(value)).collect(),
            vec![],
            vec![],
        ];
        GameContext::from_parts_with_table_state(
            drawn_tile.map(tile),
            hand_values.iter().map(|&value| tile(value)).collect(),
            vec![],
            None,
            None,
            visible_values.iter().map(|&value| tile(value)).collect(),
            Some(0),
            None,
            discards,
            [false, true, false, false],
        )
    }

    #[test]
    fn prefers_genbutsu_fallback_over_suited_safety_fallback() {
        let mut agent = ShantenAgent;
        // 共通現物 16(5m) と NoChance 数牌 4(2m) が両方合法でも、現物を優先する。
        let ctx = suited_reach_context(Some(0), &[], &[4, 5, 6, 7], &[16]);
        let actions = vec![dahai(4), dahai(16)];
        assert_eq!(agent.act(&ctx, &actions), dahai(16));
    }

    #[test]
    fn prefers_honor_safety_fallback_over_suited_safety_fallback() {
        let mut agent = ShantenAgent;
        // 共通現物なし。字牌 108(東) と NoChance 数牌 4(2m) が合法なら字牌を優先する。
        let ctx = suited_reach_context(Some(0), &[], &[4, 5, 6, 7], &[]);
        let actions = vec![dahai(108), dahai(4)];
        assert_eq!(agent.act(&ctx, &actions), dahai(108));
    }

    #[test]
    fn picks_no_chance_suited_dahai_when_no_genbutsu_or_honor() {
        let mut agent = ShantenAgent;
        // 共通現物も字牌もなし。4m を4枚見えにして経路 [3m,4m] を Blocked にし 2m を NoChance。
        // 無スジ 0(1m) より NoChance 4(2m) を選ぶ。
        let ctx = suited_reach_context(Some(0), &[], &[12, 13, 14, 15], &[]);
        let actions = vec![dahai(0), dahai(4)];
        assert_eq!(agent.act(&ctx, &actions), dahai(4));
    }

    #[test]
    fn picks_one_chance_suited_dahai_when_no_genbutsu_or_honor() {
        let mut agent = ShantenAgent;
        // 共通現物も字牌もなし。4m を3枚見えにして経路 [3m,4m] を OneChance にし 2m を OneChance。
        // 無スジ 0(1m) より OneChance 4(2m) を選ぶ。
        let ctx = suited_reach_context(Some(0), &[], &[12, 13, 14], &[]);
        let actions = vec![dahai(0), dahai(4)];
        assert_eq!(agent.act(&ctx, &actions), dahai(4));
    }

    #[test]
    fn picks_suji_suited_dahai_when_no_genbutsu_or_honor() {
        let mut agent = ShantenAgent;
        // 共通現物も字牌もなし。リーチ者の河に 12(4m) があり 0(1m) はスジ。無スジ 16(5m) より選ぶ。
        let ctx = suited_reach_context(Some(0), &[], &[], &[12]);
        let actions = vec![dahai(16), dahai(0)];
        assert_eq!(agent.act(&ctx, &actions), dahai(0));
    }

    #[test]
    fn suited_safety_fallback_follows_safety_order() {
        let mut agent = ShantenAgent;
        // 経路壁で安全度を作る。1p は 2p 4枚で NoChance、9p は 8p 3枚で OneChance、
        // 1s は 4s(84) 河でスジ(Suji)、5s は無スジ・壁なし(NoSafety)。最も安全な NoChance を選ぶ。
        let ctx = suited_reach_context(Some(0), &[], &[40, 41, 42, 43, 64, 65, 66], &[84]);
        let actions = vec![dahai(88), dahai(72), dahai(68), dahai(36)];
        assert_eq!(agent.act(&ctx, &actions), dahai(36));
    }

    #[test]
    fn fold_without_safe_suited_falls_through_to_normal_discard() {
        let mut agent = ShantenAgent;
        // Fold 局面で共通現物も字牌もなく数牌が全て NoSafety なら、防御 fallback は無い。
        // Reach は抑制し、防御牌がないことを理由に失敗させず通常打牌へ進む。
        let ctx = suited_reach_context(Some(0), &[], &[], &[]);
        assert_eq!(
            decide_push_pull(&push_pull_inputs_from_context(&ctx)).mode,
            PushPullMode::Fold
        );
        let actions = vec![LegalAction::Reach, dahai(0), dahai(4)];
        assert_eq!(agent.act(&ctx, &actions), dahai(0));
    }

    #[test]
    fn does_not_use_suited_safety_fallback_without_opponent_reach() {
        let mut agent = ShantenAgent;
        // 他家リーチが無ければ、河に 16(5m) があり 4(2m) がスジ相当でも従来の Reach を選ぶ。
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
        let actions = vec![LegalAction::Reach, dahai(4)];
        assert_eq!(agent.act(&ctx, &actions), LegalAction::Reach);
    }

    #[test]
    fn prefers_suited_safety_fallback_over_reach() {
        let mut agent = ShantenAgent;
        // 共通現物も字牌もなし。4m を4枚見えにして 2m を NoChance にすると Reach より優先する。
        let ctx = suited_reach_context(Some(0), &[], &[12, 13, 14, 15], &[]);
        let actions = vec![LegalAction::Reach, dahai(0), dahai(4)];
        assert_eq!(agent.act(&ctx, &actions), dahai(4));
    }

    #[test]
    fn push_prefers_normal_discard_over_suited_safety_fallback() {
        let mut agent = ShantenAgent;
        // テンパイで単独の子リーチに対しては Push。Reach が合法でなければ通常打牌へ進み、
        // 防御 fallback より通常打牌(32(9m))を優先する。
        let hand_values = [0, 4, 8, 12, 13, 20, 24, 28, 32, 36, 40, 44, 89];
        let ctx = suited_reach_context(Some(88), &hand_values, &[4, 5, 6, 7], &[]);
        assert_eq!(
            decide_push_pull(&push_pull_inputs_from_context(&ctx)).mode,
            PushPullMode::Push
        );
        let actions: Vec<LegalAction> = hand_values
            .iter()
            .map(|&value| dahai(value))
            .chain([dahai(88)])
            .collect();
        let normal = select_discard_action(&ctx, &actions).unwrap();
        assert_eq!(agent.act(&ctx, &actions), normal);
        assert_ne!(agent.act(&ctx, &actions), dahai(4));
    }

    #[test]
    fn suited_safety_fallback_ignores_non_dahai_actions() {
        let mut agent = ShantenAgent;
        // 数牌の Pon はあっても数牌 Dahai が無ければ数牌防御 fallback は発動しない。
        let ctx = suited_reach_context(Some(0), &[], &[4, 5, 6, 7], &[]);
        let actions = vec![
            LegalAction::Pon {
                tile: tile(4),
                consumed: vec![tile(5), tile(6)],
            },
            LegalAction::None,
        ];
        assert_eq!(agent.act(&ctx, &actions), LegalAction::None);
    }

    // テンパイ手牌で他家リーチを受ける局面。oya と reached を指定してモードを作り分ける。
    // visible は空にして should_reach を従来挙動(true)に保つ。
    fn tenpai_under_reach_context(oya: Option<u8>, reached: [bool; 4]) -> GameContext {
        let hand: Vec<_> = TENPAI_HAND.iter().map(|&value| tile(value)).collect();
        // リーチ者(player 1)の河に 5m を置き、テンパイ手牌の 5m を現物にする。
        let discards = [vec![], vec![tile(16)], vec![], vec![]];
        GameContext::from_parts_with_table_state(
            Some(tile(TENPAI_DRAWN)),
            hand,
            vec![],
            None,
            None,
            Vec::new(),
            Some(0),
            oya,
            discards,
            reached,
        )
    }

    #[test]
    fn push_tenpai_against_single_non_dealer_reaches() {
        let mut agent = ShantenAgent;
        // 単独の子リーチに対するテンパイ。decide_push_pull は Push。
        // Reach が合法で should_reach() == true なら、現物があっても Reach を選ぶ。
        let ctx = tenpai_under_reach_context(None, [false, true, false, false]);
        assert_eq!(
            decide_push_pull(&push_pull_inputs_from_context(&ctx)).mode,
            PushPullMode::Push
        );
        let actions = tenpai_actions();
        assert!(should_reach(&ctx));
        assert_eq!(agent.act(&ctx, &actions), LegalAction::Reach);
    }

    #[test]
    fn neutral_tenpai_against_dealer_reach_prefers_normal_discard_over_reach() {
        let mut agent = ShantenAgent;
        // 親リーチに対するテンパイ。decide_push_pull は Neutral。
        // Reach が合法でも選ばず、暫定的にダマ相当の通常打牌を優先する。
        let ctx = tenpai_under_reach_context(Some(1), [false, true, false, false]);
        assert_eq!(
            decide_push_pull(&push_pull_inputs_from_context(&ctx)).mode,
            PushPullMode::Neutral
        );
        let actions = tenpai_actions();
        let normal = select_discard_action(&ctx, &actions).unwrap();
        let selected = agent.act(&ctx, &actions);
        assert_eq!(selected, normal);
        assert_ne!(selected, LegalAction::Reach);
    }

    #[test]
    fn neutral_tenpai_against_multiple_reach_prefers_normal_discard_over_reach() {
        let mut agent = ShantenAgent;
        // 複数リーチに対するテンパイ。decide_push_pull は Neutral。Reach は選ばない。
        let ctx = tenpai_under_reach_context(None, [false, true, true, false]);
        assert_eq!(
            decide_push_pull(&push_pull_inputs_from_context(&ctx)).mode,
            PushPullMode::Neutral
        );
        let actions = tenpai_actions();
        let normal = select_discard_action(&ctx, &actions).unwrap();
        let selected = agent.act(&ctx, &actions);
        assert_eq!(selected, normal);
        assert_ne!(selected, LegalAction::Reach);
    }

    // 2向聴以上で他家リーチを受ける Fold 局面。リーチ者の河に 5s を置き手牌の 5s を現物にする。
    const FOLD_HAND: [u8; 13] = [0, 4, 17, 20, 36, 40, 56, 60, 89, 108, 112, 120, 124];
    const FOLD_DRAWN: u8 = 16;

    fn fold_under_reach_context() -> GameContext {
        let hand: Vec<_> = FOLD_HAND.iter().map(|&value| tile(value)).collect();
        let discards = [vec![], vec![tile(89)], vec![], vec![]];
        GameContext::from_parts_with_table_state(
            Some(tile(FOLD_DRAWN)),
            hand,
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

    fn fold_actions() -> Vec<LegalAction> {
        FOLD_HAND
            .iter()
            .map(|&value| dahai(value))
            .chain([dahai(FOLD_DRAWN)])
            .collect()
    }

    #[test]
    fn fold_prefers_defense_fallback_over_normal_discard() {
        let mut agent = ShantenAgent;
        // 二向聴以上で他家リーチを受ける Fold 局面。防御 fallback(現物 5s)と通常打牌が異なり、
        // Fold では防御 fallback を通常打牌より優先する。
        let ctx = fold_under_reach_context();
        assert_eq!(
            decide_push_pull(&push_pull_inputs_from_context(&ctx)).mode,
            PushPullMode::Fold
        );
        let actions = fold_actions();
        let normal = select_discard_action(&ctx, &actions).unwrap();
        let defense = select_defense_fallback_action(&ctx, &actions)
            .cloned()
            .unwrap();
        assert_ne!(normal, defense);
        assert_eq!(agent.act(&ctx, &actions), defense);
        assert_eq!(defense, dahai(89));
    }

    #[test]
    fn agent_push_pull_uses_legal_candidate_evaluation_not_illegal_global_best() {
        // 全体最善(手牌の東を切ってテンパイ = shanten 0)は非合法で、合法なのはツモ切り 3p だけ。
        // 非合法な全体最善を使うと Push、合法なツモ切り(shanten 1)を使うと Neutral になる。
        // Agent は合法候補側の evaluation / mode を使う。
        let hand_values = [0, 4, 8, 12, 17, 20, 24, 28, 32, 36, 40, 89, 108];
        let drawn = 44; // 3p ツモ
        let discards = [vec![], vec![tile(16)], vec![], vec![]];
        let ctx = GameContext::from_parts_with_table_state(
            Some(tile(drawn)),
            hand_values.iter().map(|&value| tile(value)).collect(),
            vec![],
            None,
            None,
            Vec::new(),
            Some(0),
            None,
            discards,
            [false, true, false, false],
        );
        let tiles: Vec<_> = ctx
            .hand_tiles()
            .iter()
            .copied()
            .chain(ctx.drawn_tile())
            .collect();

        // 非合法な全体最善候補の mode は Push。
        let global_best = select_best_discard_evaluation(&ctx, &tiles).unwrap();
        assert_eq!(global_best.min_shanten_after_discard(), 0);
        let illegal_mode = decide_push_pull(&push_pull_inputs_from_context_with_evaluation(
            &ctx,
            Some(&global_best),
        ))
        .mode;
        assert_eq!(illegal_mode, PushPullMode::Push);

        // 合法なのはツモ切り 3p だけ。Agent が使う offense は合法候補の評価に一致する。
        let actions = vec![dahai(drawn)];
        let selection = select_discard_action_with_evaluation(&ctx, &actions);
        let legal_evaluation = selection.evaluation.clone().unwrap();
        assert_eq!(legal_evaluation.min_shanten_after_discard(), 1);
        let legal_inputs =
            push_pull_inputs_from_context_with_evaluation(&ctx, selection.evaluation.as_ref());
        let legal_mode = decide_push_pull(&legal_inputs).mode;

        assert_ne!(illegal_mode, legal_mode);
        assert_eq!(legal_mode, PushPullMode::Neutral);
        assert_eq!(
            legal_inputs.offense.unwrap().min_shanten_after_discard,
            legal_evaluation.min_shanten_after_discard()
        );

        // Agent は合法候補(ツモ切り 3p)を切る。
        let mut agent = ShantenAgent;
        assert_eq!(agent.act(&ctx, &actions), dahai(drawn));
    }
}
