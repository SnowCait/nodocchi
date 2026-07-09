use crate::action::LegalAction;
use crate::context::GameContext;
use bot_logic::TileType;

// discards は防御・現物判定用、visible_tiles は枚数補正用なので用途を分ける。
pub fn is_genbutsu_for(tile: TileType, player: usize, context: &GameContext) -> bool {
    context
        .discards_of(player)
        .is_some_and(|discards| discards.iter().any(|t| t.tile_type() == tile))
}

// 全リーチ者に共通する現物か判定する。リーチ者がいなければ false。
pub fn is_genbutsu_for_all_reached(tile: TileType, context: &GameContext) -> bool {
    let reached = context.reached_opponents();
    if reached.is_empty() {
        return false;
    }
    reached
        .iter()
        .all(|&player| is_genbutsu_for(tile, player, context))
}

// 合法 Dahai の中から全リーチ者に共通する現物候補を、元の順序を保ったまま抽出する。
pub fn genbutsu_dahai_actions_for_all_reached<'a>(
    legal_actions: &'a [LegalAction],
    context: &GameContext,
) -> Vec<&'a LegalAction> {
    legal_actions
        .iter()
        .filter(|action| match action {
            LegalAction::Dahai { tile } => is_genbutsu_for_all_reached(tile.tile_type(), context),
            _ => false,
        })
        .collect()
}

// 他家リーチ中に、合法 Dahai の中から全リーチ者に共通する現物を fallback として選ぶ。
// 他家リーチがない、または共通現物がなければ None。合法 action からのみ選ぶ。
pub fn select_genbutsu_fallback_action<'a>(
    context: &GameContext,
    legal_actions: &'a [LegalAction],
) -> Option<&'a LegalAction> {
    if !context.any_opponent_reached() {
        return None;
    }
    genbutsu_dahai_actions_for_all_reached(legal_actions, context)
        .into_iter()
        .next()
}

// visible_tiles 中で同じ TileType の枚数を数える。赤5も通常5と同じ TileType として数える。
pub fn visible_count_of(tile: TileType, context: &GameContext) -> u8 {
    context
        .visible_tiles()
        .iter()
        .filter(|visible| visible.tile_type() == tile)
        .count() as u8
}

// 字牌の見え枚数に基づく安全度。見えているほど当たりにくいので安全度が高い。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum HonorSafetyRank {
    NoVisible,
    OneVisible,
    TwoVisible,
    ThreeOrMoreVisible,
}

// 字牌の安全度を見え枚数から求める。字牌でなければ None。
pub fn honor_safety_rank(tile: TileType, context: &GameContext) -> Option<HonorSafetyRank> {
    if !tile.is_honor() {
        return None;
    }
    let rank = match visible_count_of(tile, context) {
        0 => HonorSafetyRank::NoVisible,
        1 => HonorSafetyRank::OneVisible,
        2 => HonorSafetyRank::TwoVisible,
        _ => HonorSafetyRank::ThreeOrMoreVisible,
    };
    Some(rank)
}

// 合法 Dahai のうち字牌のみを安全度の高い順に並べる。同安全度は元の順序を保つ。
pub fn honor_dahai_actions_by_safety<'a>(
    legal_actions: &'a [LegalAction],
    context: &GameContext,
) -> Vec<(&'a LegalAction, HonorSafetyRank)> {
    let mut ranked: Vec<(&'a LegalAction, HonorSafetyRank)> = legal_actions
        .iter()
        .filter_map(|action| match action {
            LegalAction::Dahai { tile } => {
                honor_safety_rank(tile.tile_type(), context).map(|rank| (action, rank))
            }
            _ => None,
        })
        .collect();
    ranked.sort_by(|a, b| b.1.cmp(&a.1));
    ranked
}

// 最も安全度の高い字牌 Dahai を fallback として選ぶ。候補がなければ None。
pub fn select_honor_safety_fallback_action<'a>(
    legal_actions: &'a [LegalAction],
    context: &GameContext,
) -> Option<&'a LegalAction> {
    honor_dahai_actions_by_safety(legal_actions, context)
        .into_iter()
        .next()
        .map(|(action, _)| action)
}

#[cfg(test)]
mod tests {
    use super::*;
    use bot_logic::TileId;

    fn tile(value: u8) -> TileId {
        TileId::new(value).unwrap()
    }

    fn table_state_context(
        player_id: Option<u8>,
        oya: Option<u8>,
        discards: [Vec<TileId>; 4],
        reached: [bool; 4],
    ) -> GameContext {
        GameContext::from_parts_with_table_state(
            None,
            vec![],
            vec![],
            None,
            None,
            Vec::new(),
            player_id,
            oya,
            discards,
            reached,
        )
    }

    #[test]
    fn is_genbutsu_for_detects_discarded_tile_type() {
        let discards = [vec![tile(0)], vec![tile(16)], vec![], vec![]];
        let context = table_state_context(Some(3), None, discards, [false; 4]);
        let one_man = tile(0).tile_type();
        assert!(is_genbutsu_for(one_man, 0, &context));
        assert!(!is_genbutsu_for(one_man, 1, &context));
    }

    #[test]
    fn is_genbutsu_for_out_of_range_player_is_false() {
        let context = GameContext::default();
        assert!(!is_genbutsu_for(tile(0).tile_type(), 4, &context));
    }

    #[test]
    fn is_genbutsu_for_all_reached_false_without_reachers() {
        let discards = [
            vec![tile(16)],
            vec![tile(16)],
            vec![tile(16)],
            vec![tile(16)],
        ];
        let context = table_state_context(Some(0), None, discards, [false; 4]);
        assert!(!is_genbutsu_for_all_reached(tile(16).tile_type(), &context));
    }

    #[test]
    fn is_genbutsu_for_all_reached_single_reacher_hit() {
        let discards = [vec![], vec![tile(16)], vec![], vec![]];
        let context = table_state_context(Some(0), None, discards, [false, true, false, false]);
        assert!(is_genbutsu_for_all_reached(tile(16).tile_type(), &context));
    }

    #[test]
    fn is_genbutsu_for_all_reached_single_reacher_miss() {
        let discards = [vec![], vec![tile(0)], vec![], vec![]];
        let context = table_state_context(Some(0), None, discards, [false, true, false, false]);
        assert!(!is_genbutsu_for_all_reached(tile(16).tile_type(), &context));
    }

    #[test]
    fn is_genbutsu_for_all_reached_multiple_reachers_all_hit() {
        let discards = [vec![], vec![tile(16)], vec![tile(17)], vec![]];
        let context = table_state_context(Some(0), None, discards, [false, true, true, false]);
        assert!(is_genbutsu_for_all_reached(tile(16).tile_type(), &context));
    }

    #[test]
    fn is_genbutsu_for_all_reached_multiple_reachers_partial_miss() {
        let discards = [vec![], vec![tile(16)], vec![], vec![]];
        let context = table_state_context(Some(0), None, discards, [false, true, true, false]);
        assert!(!is_genbutsu_for_all_reached(tile(16).tile_type(), &context));
    }

    #[test]
    fn is_genbutsu_for_all_reached_ignores_own_reach() {
        // 自分(0)の河にはあるが自分のリーチは対象外、他家リーチ者の河には無い。
        let discards = [vec![tile(16)], vec![], vec![], vec![]];
        let context = table_state_context(Some(0), None, discards, [true, true, false, false]);
        assert!(!is_genbutsu_for_all_reached(tile(16).tile_type(), &context));
    }

    #[test]
    fn is_genbutsu_for_all_reached_without_player_id_targets_all_reached() {
        let discards = [vec![tile(16)], vec![], vec![], vec![]];
        let context = table_state_context(None, None, discards, [true, false, false, false]);
        assert!(is_genbutsu_for_all_reached(tile(16).tile_type(), &context));
    }

    #[test]
    fn is_genbutsu_for_all_reached_treats_red_five_as_same_type() {
        // 河に通常5m(tile 17)、判定対象が赤5m相当(tile 16)。同じ TileType として現物扱い。
        let discards = [vec![], vec![tile(17)], vec![], vec![]];
        let context = table_state_context(Some(0), None, discards, [false, true, false, false]);
        assert!(is_genbutsu_for_all_reached(tile(16).tile_type(), &context));
    }

    #[test]
    fn genbutsu_dahai_actions_for_all_reached_empty_without_reachers() {
        let discards = [vec![tile(16)], vec![tile(16)], vec![], vec![]];
        let context = table_state_context(Some(0), None, discards, [false; 4]);
        let actions = vec![LegalAction::Dahai { tile: tile(16) }];
        assert!(genbutsu_dahai_actions_for_all_reached(&actions, &context).is_empty());
    }

    #[test]
    fn genbutsu_dahai_actions_for_all_reached_filters_to_common_genbutsu() {
        let discards = [vec![], vec![tile(16)], vec![tile(0)], vec![]];
        let context = table_state_context(Some(0), None, discards, [false, true, true, false]);
        let actions = vec![
            LegalAction::Dahai { tile: tile(16) },
            LegalAction::Dahai { tile: tile(0) },
            LegalAction::Dahai { tile: tile(56) },
        ];
        let filtered = genbutsu_dahai_actions_for_all_reached(&actions, &context);
        assert_eq!(filtered.len(), 0);
    }

    #[test]
    fn genbutsu_dahai_actions_for_all_reached_returns_matching_dahai() {
        let discards = [vec![], vec![tile(16), tile(0)], vec![], vec![]];
        let context = table_state_context(Some(0), None, discards, [false, true, false, false]);
        let actions = vec![
            LegalAction::Dahai { tile: tile(16) },
            LegalAction::Dahai { tile: tile(0) },
            LegalAction::Dahai { tile: tile(56) },
        ];
        let filtered = genbutsu_dahai_actions_for_all_reached(&actions, &context);
        assert_eq!(
            filtered,
            vec![
                &LegalAction::Dahai { tile: tile(16) },
                &LegalAction::Dahai { tile: tile(0) },
            ]
        );
    }

    #[test]
    fn genbutsu_dahai_actions_for_all_reached_excludes_non_dahai() {
        let discards = [vec![], vec![tile(16)], vec![], vec![]];
        let context = table_state_context(Some(0), None, discards, [false, true, false, false]);
        let actions = vec![
            LegalAction::Reach,
            LegalAction::Hora,
            LegalAction::Ryukyoku,
            LegalAction::None,
            LegalAction::Pon {
                tile: tile(16),
                consumed: vec![tile(17), tile(18)],
            },
            LegalAction::Ankan {
                consumed: vec![tile(16), tile(17), tile(18), tile(19)],
            },
            LegalAction::Dahai { tile: tile(16) },
        ];
        let filtered = genbutsu_dahai_actions_for_all_reached(&actions, &context);
        assert_eq!(filtered, vec![&LegalAction::Dahai { tile: tile(16) }]);
    }

    #[test]
    fn genbutsu_dahai_actions_for_all_reached_preserves_order() {
        let discards = [vec![], vec![tile(0), tile(16), tile(56)], vec![], vec![]];
        let context = table_state_context(Some(0), None, discards, [false, true, false, false]);
        let actions = vec![
            LegalAction::Dahai { tile: tile(56) },
            LegalAction::Dahai { tile: tile(0) },
            LegalAction::Dahai { tile: tile(16) },
        ];
        let filtered = genbutsu_dahai_actions_for_all_reached(&actions, &context);
        assert_eq!(
            filtered,
            vec![
                &LegalAction::Dahai { tile: tile(56) },
                &LegalAction::Dahai { tile: tile(0) },
                &LegalAction::Dahai { tile: tile(16) },
            ]
        );
    }

    #[test]
    fn genbutsu_dahai_actions_for_all_reached_matches_red_five() {
        // 河に通常5m(tile 17)、Dahai が赤5m相当(tile 16)でも同じ TileType の現物として抽出。
        let discards = [vec![], vec![tile(17)], vec![], vec![]];
        let context = table_state_context(Some(0), None, discards, [false, true, false, false]);
        let actions = vec![LegalAction::Dahai { tile: tile(16) }];
        let filtered = genbutsu_dahai_actions_for_all_reached(&actions, &context);
        assert_eq!(filtered, vec![&LegalAction::Dahai { tile: tile(16) }]);
    }

    #[test]
    fn select_genbutsu_fallback_action_none_without_opponent_reach() {
        let discards = [vec![], vec![tile(16)], vec![], vec![]];
        let context = table_state_context(Some(0), None, discards, [false; 4]);
        let actions = vec![LegalAction::Dahai { tile: tile(16) }];
        assert_eq!(select_genbutsu_fallback_action(&context, &actions), None);
    }

    #[test]
    fn select_genbutsu_fallback_action_ignores_only_own_reach() {
        // 自分(0)だけがリーチしている場合は他家リーチ扱いにしない。
        let discards = [vec![tile(16)], vec![], vec![], vec![]];
        let context = table_state_context(Some(0), None, discards, [true, false, false, false]);
        let actions = vec![LegalAction::Dahai { tile: tile(16) }];
        assert_eq!(select_genbutsu_fallback_action(&context, &actions), None);
    }

    #[test]
    fn select_genbutsu_fallback_action_returns_common_genbutsu() {
        let discards = [vec![], vec![tile(16)], vec![], vec![]];
        let context = table_state_context(Some(0), None, discards, [false, true, false, false]);
        let actions = vec![
            LegalAction::Dahai { tile: tile(0) },
            LegalAction::Dahai { tile: tile(16) },
        ];
        assert_eq!(
            select_genbutsu_fallback_action(&context, &actions),
            Some(&LegalAction::Dahai { tile: tile(16) })
        );
    }

    #[test]
    fn select_genbutsu_fallback_action_none_when_no_common_genbutsu() {
        let discards = [vec![], vec![tile(16)], vec![], vec![]];
        let context = table_state_context(Some(0), None, discards, [false, true, false, false]);
        let actions = vec![
            LegalAction::Dahai { tile: tile(0) },
            LegalAction::Dahai { tile: tile(56) },
        ];
        assert_eq!(select_genbutsu_fallback_action(&context, &actions), None);
    }

    #[test]
    fn select_genbutsu_fallback_action_returns_first_in_legal_order() {
        let discards = [vec![], vec![tile(0), tile(16)], vec![], vec![]];
        let context = table_state_context(Some(0), None, discards, [false, true, false, false]);
        let actions = vec![
            LegalAction::Dahai { tile: tile(56) },
            LegalAction::Dahai { tile: tile(16) },
            LegalAction::Dahai { tile: tile(0) },
        ];
        assert_eq!(
            select_genbutsu_fallback_action(&context, &actions),
            Some(&LegalAction::Dahai { tile: tile(16) })
        );
    }

    #[test]
    fn select_genbutsu_fallback_action_never_returns_non_dahai() {
        // 現物になり得るのは Dahai のみ。Reach/Hora/Ryukyoku/None/副露・カンは返さない。
        let discards = [vec![], vec![tile(16)], vec![], vec![]];
        let context = table_state_context(Some(0), None, discards, [false, true, false, false]);
        let actions = vec![
            LegalAction::Reach,
            LegalAction::Hora,
            LegalAction::Ryukyoku,
            LegalAction::None,
            LegalAction::Pon {
                tile: tile(16),
                consumed: vec![tile(17), tile(18)],
            },
            LegalAction::Ankan {
                consumed: vec![tile(16), tile(17), tile(18), tile(19)],
            },
        ];
        assert_eq!(select_genbutsu_fallback_action(&context, &actions), None);
    }

    #[test]
    fn select_genbutsu_fallback_action_matches_red_five_dahai() {
        // 河に通常5m(tile 17)、Dahai が赤5m相当(tile 16)でも同じ TileType の現物として選ぶ。
        let discards = [vec![], vec![tile(17)], vec![], vec![]];
        let context = table_state_context(Some(0), None, discards, [false, true, false, false]);
        let actions = vec![LegalAction::Dahai { tile: tile(16) }];
        assert_eq!(
            select_genbutsu_fallback_action(&context, &actions),
            Some(&LegalAction::Dahai { tile: tile(16) })
        );
    }

    fn visible_context(visible_tiles: Vec<TileId>) -> GameContext {
        GameContext::from_parts_with_visible_tiles(None, vec![], vec![], None, None, visible_tiles)
    }

    #[test]
    fn visible_count_of_counts_duplicate_tile_types() {
        // 東(tile 108-110)を3枚、白(tile 124)を1枚見えている状態。
        let context = visible_context(vec![tile(108), tile(109), tile(110), tile(124)]);
        assert_eq!(visible_count_of(tile(108).tile_type(), &context), 3);
        assert_eq!(visible_count_of(tile(124).tile_type(), &context), 1);
        assert_eq!(visible_count_of(tile(0).tile_type(), &context), 0);
    }

    #[test]
    fn visible_count_of_treats_red_five_as_same_type() {
        // 赤5m(tile 16)と通常5m(tile 17)は同じ TileType として数える。
        let context = visible_context(vec![tile(16), tile(17)]);
        assert_eq!(visible_count_of(tile(16).tile_type(), &context), 2);
    }

    #[test]
    fn honor_safety_rank_none_for_number_tiles() {
        let context = visible_context(vec![]);
        assert_eq!(honor_safety_rank(tile(0).tile_type(), &context), None);
        assert_eq!(honor_safety_rank(tile(16).tile_type(), &context), None);
        assert_eq!(honor_safety_rank(tile(104).tile_type(), &context), None);
    }

    #[test]
    fn honor_safety_rank_classifies_visible_count() {
        // 東を0/1/2/3枚見えているそれぞれのケース。
        let east = tile(108).tile_type();
        assert_eq!(
            honor_safety_rank(east, &visible_context(vec![])),
            Some(HonorSafetyRank::NoVisible)
        );
        assert_eq!(
            honor_safety_rank(east, &visible_context(vec![tile(108)])),
            Some(HonorSafetyRank::OneVisible)
        );
        assert_eq!(
            honor_safety_rank(east, &visible_context(vec![tile(108), tile(109)])),
            Some(HonorSafetyRank::TwoVisible)
        );
        assert_eq!(
            honor_safety_rank(
                east,
                &visible_context(vec![tile(108), tile(109), tile(110)])
            ),
            Some(HonorSafetyRank::ThreeOrMoreVisible)
        );
    }

    #[test]
    fn honor_dahai_actions_by_safety_excludes_non_dahai() {
        let context = visible_context(vec![tile(108)]);
        let actions = vec![
            LegalAction::Reach,
            LegalAction::Pon {
                tile: tile(108),
                consumed: vec![tile(109), tile(110)],
            },
            LegalAction::Dahai { tile: tile(108) },
        ];
        let ranked = honor_dahai_actions_by_safety(&actions, &context);
        assert_eq!(
            ranked,
            vec![(
                &LegalAction::Dahai { tile: tile(108) },
                HonorSafetyRank::OneVisible
            )]
        );
    }

    #[test]
    fn honor_dahai_actions_by_safety_excludes_number_dahai() {
        let context = visible_context(vec![]);
        let actions = vec![
            LegalAction::Dahai { tile: tile(0) },
            LegalAction::Dahai { tile: tile(108) },
        ];
        let ranked = honor_dahai_actions_by_safety(&actions, &context);
        assert_eq!(
            ranked,
            vec![(
                &LegalAction::Dahai { tile: tile(108) },
                HonorSafetyRank::NoVisible
            )]
        );
    }

    #[test]
    fn honor_dahai_actions_by_safety_orders_high_safety_first() {
        // 東は3枚見え、南は1枚見え、白は0枚見え。安全度の高い順に並ぶ。
        let context = visible_context(vec![tile(108), tile(109), tile(110), tile(112)]);
        let actions = vec![
            LegalAction::Dahai { tile: tile(124) },
            LegalAction::Dahai { tile: tile(112) },
            LegalAction::Dahai { tile: tile(108) },
        ];
        let ranked = honor_dahai_actions_by_safety(&actions, &context);
        assert_eq!(
            ranked,
            vec![
                (
                    &LegalAction::Dahai { tile: tile(108) },
                    HonorSafetyRank::ThreeOrMoreVisible
                ),
                (
                    &LegalAction::Dahai { tile: tile(112) },
                    HonorSafetyRank::OneVisible
                ),
                (
                    &LegalAction::Dahai { tile: tile(124) },
                    HonorSafetyRank::NoVisible
                ),
            ]
        );
    }

    #[test]
    fn honor_dahai_actions_by_safety_preserves_order_within_same_rank() {
        // すべて0枚見えの字牌 Dahai は元の順序を保つ。
        let context = visible_context(vec![]);
        let actions = vec![
            LegalAction::Dahai { tile: tile(124) },
            LegalAction::Dahai { tile: tile(108) },
            LegalAction::Dahai { tile: tile(120) },
        ];
        let ranked = honor_dahai_actions_by_safety(&actions, &context);
        assert_eq!(
            ranked,
            vec![
                (
                    &LegalAction::Dahai { tile: tile(124) },
                    HonorSafetyRank::NoVisible
                ),
                (
                    &LegalAction::Dahai { tile: tile(108) },
                    HonorSafetyRank::NoVisible
                ),
                (
                    &LegalAction::Dahai { tile: tile(120) },
                    HonorSafetyRank::NoVisible
                ),
            ]
        );
    }

    #[test]
    fn select_honor_safety_fallback_action_returns_safest_honor_dahai() {
        // 東は2枚見え、南は0枚見え。より安全な東を選ぶ。
        let context = visible_context(vec![tile(108), tile(109)]);
        let actions = vec![
            LegalAction::Dahai { tile: tile(112) },
            LegalAction::Dahai { tile: tile(108) },
        ];
        assert_eq!(
            select_honor_safety_fallback_action(&actions, &context),
            Some(&LegalAction::Dahai { tile: tile(108) })
        );
    }

    #[test]
    fn select_honor_safety_fallback_action_none_without_honor_dahai() {
        let context = visible_context(vec![]);
        let actions = vec![LegalAction::Dahai { tile: tile(0) }, LegalAction::Reach];
        assert_eq!(
            select_honor_safety_fallback_action(&actions, &context),
            None
        );
    }
}
