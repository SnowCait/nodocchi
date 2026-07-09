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

// 簡易スジ安全度。現時点では無スジ / スジの2段階のみ。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum SujiSafetyRank {
    NoSuji,
    Suji,
}

// 指定 player の河から簡易スジ判定する。字牌は対象外。player が範囲外なら false。
// 同じ suit で number が ±3 の牌が河にあればスジ扱い。
pub fn is_suji_for(tile: TileType, player: usize, context: &GameContext) -> bool {
    let (Some(number), Some(suit)) = (tile.number(), tile.suit()) else {
        return false;
    };
    context.discards_of(player).is_some_and(|discards| {
        discards.iter().any(|discarded| {
            let discarded = discarded.tile_type();
            discarded.suit() == Some(suit)
                && discarded.number().is_some_and(|n| n.abs_diff(number) == 3)
        })
    })
}

// いずれかのリーチ者の河からスジ判定する。リーチ者がいなければ false。
pub fn is_suji_for_any_reached(tile: TileType, context: &GameContext) -> bool {
    let reached = context.reached_opponents();
    if reached.is_empty() {
        return false;
    }
    reached
        .iter()
        .any(|&player| is_suji_for(tile, player, context))
}

// リーチ者の河に対する簡易スジ安全度。数牌なら Some、字牌なら None。
pub fn suji_safety_rank_for_any_reached(
    tile: TileType,
    context: &GameContext,
) -> Option<SujiSafetyRank> {
    if tile.is_honor() {
        return None;
    }
    if is_suji_for_any_reached(tile, context) {
        Some(SujiSafetyRank::Suji)
    } else {
        Some(SujiSafetyRank::NoSuji)
    }
}

// 合法 Dahai のうち数牌のみを安全度の高い順(Suji → NoSuji)に並べる。同安全度は元の順序を保つ。
pub fn suji_dahai_actions_by_safety<'a>(
    legal_actions: &'a [LegalAction],
    context: &GameContext,
) -> Vec<(&'a LegalAction, SujiSafetyRank)> {
    let mut ranked: Vec<(&'a LegalAction, SujiSafetyRank)> = legal_actions
        .iter()
        .filter_map(|action| match action {
            LegalAction::Dahai { tile } => {
                suji_safety_rank_for_any_reached(tile.tile_type(), context)
                    .map(|rank| (action, rank))
            }
            _ => None,
        })
        .collect();
    ranked.sort_by(|a, b| b.1.cmp(&a.1));
    ranked
}

// 数牌の見え枚数に基づく壁 / ワンチャンス分類。見えているほど当たり筋が減る。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum WallRank {
    NoWall,
    OneChance,
    NoChance,
}

// 数牌の見え枚数から壁 / ワンチャンスを分類する。字牌は対象外で NoWall。
pub fn wall_rank(tile: TileType, context: &GameContext) -> WallRank {
    if tile.is_honor() {
        return WallRank::NoWall;
    }
    match visible_count_of(tile, context) {
        0..=2 => WallRank::NoWall,
        3 => WallRank::OneChance,
        _ => WallRank::NoChance,
    }
}

// 3枚見えのワンチャンス数牌か判定する。
pub fn is_one_chance(tile: TileType, context: &GameContext) -> bool {
    wall_rank(tile, context) == WallRank::OneChance
}

// 4枚見えのノーチャンス数牌か判定する。
pub fn is_no_chance(tile: TileType, context: &GameContext) -> bool {
    wall_rank(tile, context) == WallRank::NoChance
}

// 数牌のみを TileType::all() の順序で壁分類と共に返す。NoWall も含める。
pub fn wall_tile_types_by_rank(context: &GameContext) -> Vec<(TileType, WallRank)> {
    TileType::all()
        .filter(|tile| !tile.is_honor())
        .map(|tile| (tile, wall_rank(tile, context)))
        .collect()
}

// 数牌の防御 fallback 用の安全度。壁 / スジを統合して分類する。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum SuitedSafetyRank {
    NoSafety,
    Suji,
    OneChance,
    NoChance,
}

// リーチ者の河に対する数牌の安全度を壁 / スジから分類する。字牌は対象外で None。
pub fn suited_safety_rank_for_any_reached(
    tile: TileType,
    context: &GameContext,
) -> Option<SuitedSafetyRank> {
    if tile.is_honor() {
        return None;
    }
    let rank = match wall_rank(tile, context) {
        WallRank::NoChance => SuitedSafetyRank::NoChance,
        WallRank::OneChance => SuitedSafetyRank::OneChance,
        WallRank::NoWall => {
            if is_suji_for_any_reached(tile, context) {
                SuitedSafetyRank::Suji
            } else {
                SuitedSafetyRank::NoSafety
            }
        }
    };
    Some(rank)
}

// 合法 Dahai のうち数牌のみを安全度の高い順(NoChance → OneChance → Suji → NoSafety)に並べる。
// 同安全度は元の順序を保つ。
pub fn suited_dahai_actions_by_safety<'a>(
    legal_actions: &'a [LegalAction],
    context: &GameContext,
) -> Vec<(&'a LegalAction, SuitedSafetyRank)> {
    let mut ranked: Vec<(&'a LegalAction, SuitedSafetyRank)> = legal_actions
        .iter()
        .filter_map(|action| match action {
            LegalAction::Dahai { tile } => {
                suited_safety_rank_for_any_reached(tile.tile_type(), context)
                    .map(|rank| (action, rank))
            }
            _ => None,
        })
        .collect();
    ranked.sort_by(|a, b| b.1.cmp(&a.1));
    ranked
}

// 他家リーチ中に、最も安全度の高い数牌 Dahai を fallback として選ぶ。
// 他家リーチがない、または NoSafety しか候補がなければ None。NoSafety は選ばない。
pub fn select_suited_safety_fallback_action<'a>(
    legal_actions: &'a [LegalAction],
    context: &GameContext,
) -> Option<&'a LegalAction> {
    if !context.any_opponent_reached() {
        return None;
    }
    suited_dahai_actions_by_safety(legal_actions, context)
        .into_iter()
        .find(|(_, rank)| *rank != SuitedSafetyRank::NoSafety)
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

    #[test]
    fn suji_safety_rank_for_any_reached_none_for_honor() {
        // 字牌はスジ判定対象外なので None。
        let context = table_state_context(
            Some(0),
            None,
            Default::default(),
            [false, true, false, false],
        );
        assert_eq!(
            suji_safety_rank_for_any_reached(tile(108).tile_type(), &context),
            None
        );
    }

    #[test]
    fn suji_safety_rank_for_any_reached_classifies_number_tiles() {
        // リーチ者(1)の河に 4m。1m はスジ、5m は無スジ。
        let discards = [vec![], vec![tile(12)], vec![], vec![]];
        let context = table_state_context(Some(0), None, discards, [false, true, false, false]);
        assert_eq!(
            suji_safety_rank_for_any_reached(tile(0).tile_type(), &context),
            Some(SujiSafetyRank::Suji)
        );
        assert_eq!(
            suji_safety_rank_for_any_reached(tile(16).tile_type(), &context),
            Some(SujiSafetyRank::NoSuji)
        );
    }

    #[test]
    fn is_suji_for_out_of_range_player_is_false() {
        let discards = [vec![], vec![tile(12)], vec![], vec![]];
        let context = table_state_context(Some(0), None, discards, [false, true, false, false]);
        assert!(!is_suji_for(tile(0).tile_type(), 4, &context));
    }

    #[test]
    fn is_suji_for_any_reached_false_without_reachers() {
        // 河に 4m があっても、リーチ者がいなければ false。
        let discards = [vec![], vec![tile(12)], vec![], vec![]];
        let context = table_state_context(Some(0), None, discards, [false; 4]);
        assert!(!is_suji_for_any_reached(tile(0).tile_type(), &context));
    }

    #[test]
    fn is_suji_for_detects_plus_minus_three_same_suit() {
        // 4m が河にあれば、同じ suit で ±3 の 1m と 7m はスジ。
        let discards = [vec![], vec![tile(12)], vec![], vec![]];
        let context = table_state_context(Some(0), None, discards, [false, true, false, false]);
        assert!(is_suji_for(tile(0).tile_type(), 1, &context));
        assert!(is_suji_for(tile(24).tile_type(), 1, &context));
    }

    #[test]
    fn is_suji_for_false_for_different_suit() {
        // 4p が河にあっても 1m はスジにならない。
        let discards = [vec![], vec![tile(48)], vec![], vec![]];
        let context = table_state_context(Some(0), None, discards, [false, true, false, false]);
        assert!(!is_suji_for(tile(0).tile_type(), 1, &context));
    }

    #[test]
    fn is_suji_for_detects_one_four_seven() {
        // 1m 河 → 4m スジ、7m 河 → 4m スジ、4m 河 → 1m/7m スジ。
        let context = table_state_context(
            Some(0),
            None,
            [vec![], vec![tile(0)], vec![], vec![]],
            [false, true, false, false],
        );
        assert!(is_suji_for(tile(12).tile_type(), 1, &context));

        let context = table_state_context(
            Some(0),
            None,
            [vec![], vec![tile(24)], vec![], vec![]],
            [false, true, false, false],
        );
        assert!(is_suji_for(tile(12).tile_type(), 1, &context));

        let context = table_state_context(
            Some(0),
            None,
            [vec![], vec![tile(12)], vec![], vec![]],
            [false, true, false, false],
        );
        assert!(is_suji_for(tile(0).tile_type(), 1, &context));
        assert!(is_suji_for(tile(24).tile_type(), 1, &context));
    }

    #[test]
    fn is_suji_for_detects_two_five_eight() {
        // 5m が河にあれば 2m と 8m はスジ。
        let context = table_state_context(
            Some(0),
            None,
            [vec![], vec![tile(16)], vec![], vec![]],
            [false, true, false, false],
        );
        assert!(is_suji_for(tile(4).tile_type(), 1, &context));
        assert!(is_suji_for(tile(28).tile_type(), 1, &context));
    }

    #[test]
    fn is_suji_for_detects_three_six_nine() {
        // 6m が河にあれば 3m と 9m はスジ。
        let context = table_state_context(
            Some(0),
            None,
            [vec![], vec![tile(20)], vec![], vec![]],
            [false, true, false, false],
        );
        assert!(is_suji_for(tile(8).tile_type(), 1, &context));
        assert!(is_suji_for(tile(32).tile_type(), 1, &context));
    }

    #[test]
    fn suji_dahai_actions_by_safety_excludes_non_dahai() {
        let discards = [vec![], vec![tile(12)], vec![], vec![]];
        let context = table_state_context(Some(0), None, discards, [false, true, false, false]);
        let actions = vec![
            LegalAction::Reach,
            LegalAction::Pon {
                tile: tile(0),
                consumed: vec![tile(1), tile(2)],
            },
            LegalAction::Dahai { tile: tile(0) },
        ];
        let ranked = suji_dahai_actions_by_safety(&actions, &context);
        assert_eq!(
            ranked,
            vec![(&LegalAction::Dahai { tile: tile(0) }, SujiSafetyRank::Suji)]
        );
    }

    #[test]
    fn suji_dahai_actions_by_safety_excludes_honor_dahai() {
        let discards = [vec![], vec![tile(12)], vec![], vec![]];
        let context = table_state_context(Some(0), None, discards, [false, true, false, false]);
        let actions = vec![
            LegalAction::Dahai { tile: tile(108) },
            LegalAction::Dahai { tile: tile(0) },
        ];
        let ranked = suji_dahai_actions_by_safety(&actions, &context);
        assert_eq!(
            ranked,
            vec![(&LegalAction::Dahai { tile: tile(0) }, SujiSafetyRank::Suji)]
        );
    }

    #[test]
    fn suji_dahai_actions_by_safety_orders_suji_first() {
        // 4m 河 → 1m はスジ、5m は無スジ。Suji → NoSuji の順。
        let discards = [vec![], vec![tile(12)], vec![], vec![]];
        let context = table_state_context(Some(0), None, discards, [false, true, false, false]);
        let actions = vec![
            LegalAction::Dahai { tile: tile(16) },
            LegalAction::Dahai { tile: tile(0) },
        ];
        let ranked = suji_dahai_actions_by_safety(&actions, &context);
        assert_eq!(
            ranked,
            vec![
                (&LegalAction::Dahai { tile: tile(0) }, SujiSafetyRank::Suji),
                (
                    &LegalAction::Dahai { tile: tile(16) },
                    SujiSafetyRank::NoSuji
                ),
            ]
        );
    }

    #[test]
    fn suji_dahai_actions_by_safety_preserves_order_within_same_rank() {
        // リーチ者はいるが河は空なので全て NoSuji。元の順序を保つ。
        let context = table_state_context(
            Some(0),
            None,
            Default::default(),
            [false, true, false, false],
        );
        let actions = vec![
            LegalAction::Dahai { tile: tile(16) },
            LegalAction::Dahai { tile: tile(0) },
            LegalAction::Dahai { tile: tile(32) },
        ];
        let ranked = suji_dahai_actions_by_safety(&actions, &context);
        assert_eq!(
            ranked,
            vec![
                (
                    &LegalAction::Dahai { tile: tile(16) },
                    SujiSafetyRank::NoSuji
                ),
                (
                    &LegalAction::Dahai { tile: tile(0) },
                    SujiSafetyRank::NoSuji
                ),
                (
                    &LegalAction::Dahai { tile: tile(32) },
                    SujiSafetyRank::NoSuji
                ),
            ]
        );
    }

    #[test]
    fn wall_rank_no_wall_for_zero_one_two_visible() {
        // 1m(tile 0-3)を0/1/2枚見えているそれぞれのケース。
        let one_man = tile(0).tile_type();
        assert_eq!(
            wall_rank(one_man, &visible_context(vec![])),
            WallRank::NoWall
        );
        assert_eq!(
            wall_rank(one_man, &visible_context(vec![tile(0)])),
            WallRank::NoWall
        );
        assert_eq!(
            wall_rank(one_man, &visible_context(vec![tile(0), tile(1)])),
            WallRank::NoWall
        );
    }

    #[test]
    fn wall_rank_one_chance_for_three_visible() {
        let one_man = tile(0).tile_type();
        assert_eq!(
            wall_rank(one_man, &visible_context(vec![tile(0), tile(1), tile(2)])),
            WallRank::OneChance
        );
    }

    #[test]
    fn wall_rank_no_chance_for_four_visible() {
        let one_man = tile(0).tile_type();
        assert_eq!(
            wall_rank(
                one_man,
                &visible_context(vec![tile(0), tile(1), tile(2), tile(3)])
            ),
            WallRank::NoChance
        );
    }

    #[test]
    fn wall_rank_no_chance_for_five_or_more_visible() {
        // 通常あり得ないが5枚以上相当の入力でも NoChance。
        let five_man = tile(17).tile_type();
        assert_eq!(
            wall_rank(
                five_man,
                &visible_context(vec![tile(16), tile(17), tile(18), tile(19), tile(20)])
            ),
            WallRank::NoChance
        );
    }

    #[test]
    fn wall_rank_no_wall_for_honor() {
        // 字牌は3枚見えでも壁対象外なので NoWall。
        let east = tile(108).tile_type();
        assert_eq!(
            wall_rank(
                east,
                &visible_context(vec![tile(108), tile(109), tile(110)])
            ),
            WallRank::NoWall
        );
    }

    #[test]
    fn wall_rank_counts_red_five_as_same_type() {
        // 赤5m(tile 16)と通常5m(tile 17-19)で計4枚。同じ TileType として NoChance。
        let five_man = tile(17).tile_type();
        assert_eq!(
            wall_rank(
                five_man,
                &visible_context(vec![tile(16), tile(17), tile(18), tile(19)])
            ),
            WallRank::NoChance
        );
    }

    #[test]
    fn is_one_chance_true_only_for_three_visible_number() {
        let one_man = tile(0).tile_type();
        assert!(is_one_chance(
            one_man,
            &visible_context(vec![tile(0), tile(1), tile(2)])
        ));
        assert!(!is_one_chance(
            one_man,
            &visible_context(vec![tile(0), tile(1)])
        ));
        assert!(!is_one_chance(
            one_man,
            &visible_context(vec![tile(0), tile(1), tile(2), tile(3)])
        ));
        // 字牌は3枚見えでも false。
        let east = tile(108).tile_type();
        assert!(!is_one_chance(
            east,
            &visible_context(vec![tile(108), tile(109), tile(110)])
        ));
    }

    #[test]
    fn is_no_chance_true_only_for_four_or_more_visible_number() {
        let one_man = tile(0).tile_type();
        assert!(is_no_chance(
            one_man,
            &visible_context(vec![tile(0), tile(1), tile(2), tile(3)])
        ));
        assert!(!is_no_chance(
            one_man,
            &visible_context(vec![tile(0), tile(1), tile(2)])
        ));
        // 字牌は4枚見えでも false。
        let east = tile(108).tile_type();
        assert!(!is_no_chance(
            east,
            &visible_context(vec![tile(108), tile(109), tile(110), tile(111)])
        ));
    }

    #[test]
    fn wall_tile_types_by_rank_excludes_honors() {
        let context = visible_context(vec![]);
        let ranked = wall_tile_types_by_rank(&context);
        assert!(ranked.iter().all(|(tile, _)| !tile.is_honor()));
    }

    #[test]
    fn wall_tile_types_by_rank_returns_number_tiles_in_all_order() {
        let context = visible_context(vec![]);
        let ranked = wall_tile_types_by_rank(&context);
        let expected: Vec<(TileType, WallRank)> = TileType::all()
            .filter(|tile| !tile.is_honor())
            .map(|tile| (tile, WallRank::NoWall))
            .collect();
        assert_eq!(ranked, expected);
        // 数牌は27種。
        assert_eq!(ranked.len(), 27);
    }

    #[test]
    fn wall_tile_types_by_rank_includes_no_wall_entries() {
        // 1m だけ4枚見え、他は NoWall。NoWall も含めて返す。
        let context = visible_context(vec![tile(0), tile(1), tile(2), tile(3)]);
        let ranked = wall_tile_types_by_rank(&context);
        let one_man = tile(0).tile_type();
        assert_eq!(
            ranked
                .iter()
                .find(|(tile, _)| *tile == one_man)
                .map(|(_, rank)| *rank),
            Some(WallRank::NoChance)
        );
        assert!(
            ranked
                .iter()
                .any(|(tile, rank)| *tile != one_man && *rank == WallRank::NoWall)
        );
        assert_eq!(ranked.len(), 27);
    }

    fn suited_context(
        visible_tiles: Vec<TileId>,
        discards: [Vec<TileId>; 4],
        reached: [bool; 4],
    ) -> GameContext {
        GameContext::from_parts_with_table_state(
            None,
            vec![],
            vec![],
            None,
            None,
            visible_tiles,
            Some(0),
            None,
            discards,
            reached,
        )
    }

    #[test]
    fn suited_safety_rank_for_any_reached_none_for_honor() {
        // 字牌は数牌防御対象外なので None。
        let context = suited_context(
            vec![tile(108), tile(109), tile(110), tile(111)],
            Default::default(),
            [false, true, false, false],
        );
        assert_eq!(
            suited_safety_rank_for_any_reached(tile(108).tile_type(), &context),
            None
        );
    }

    #[test]
    fn suited_safety_rank_for_any_reached_no_chance_over_one_chance_and_suji() {
        // 1m は 4m 河でスジかつ4枚見え。NoChance が最優先。
        let discards = [vec![], vec![tile(12)], vec![], vec![]];
        let context = suited_context(
            vec![tile(0), tile(1), tile(2), tile(3)],
            discards,
            [false, true, false, false],
        );
        assert_eq!(
            suited_safety_rank_for_any_reached(tile(0).tile_type(), &context),
            Some(SuitedSafetyRank::NoChance)
        );
    }

    #[test]
    fn suited_safety_rank_for_any_reached_one_chance_over_suji() {
        // 1m は 4m 河でスジかつ3枚見え。OneChance が Suji より優先。
        let discards = [vec![], vec![tile(12)], vec![], vec![]];
        let context = suited_context(
            vec![tile(0), tile(1), tile(2)],
            discards,
            [false, true, false, false],
        );
        assert_eq!(
            suited_safety_rank_for_any_reached(tile(0).tile_type(), &context),
            Some(SuitedSafetyRank::OneChance)
        );
    }

    #[test]
    fn suited_safety_rank_for_any_reached_suji_over_no_safety() {
        // 4m 河で 1m はスジ(Suji)、5m は無スジ(NoSafety)。壁は無し。
        let discards = [vec![], vec![tile(12)], vec![], vec![]];
        let context = suited_context(vec![], discards, [false, true, false, false]);
        assert_eq!(
            suited_safety_rank_for_any_reached(tile(0).tile_type(), &context),
            Some(SuitedSafetyRank::Suji)
        );
        assert_eq!(
            suited_safety_rank_for_any_reached(tile(16).tile_type(), &context),
            Some(SuitedSafetyRank::NoSafety)
        );
    }

    #[test]
    fn suited_dahai_actions_by_safety_excludes_non_dahai() {
        let discards = [vec![], vec![tile(12)], vec![], vec![]];
        let context = suited_context(vec![], discards, [false, true, false, false]);
        let actions = vec![
            LegalAction::Reach,
            LegalAction::Pon {
                tile: tile(0),
                consumed: vec![tile(1), tile(2)],
            },
            LegalAction::Dahai { tile: tile(0) },
        ];
        let ranked = suited_dahai_actions_by_safety(&actions, &context);
        assert_eq!(
            ranked,
            vec![(
                &LegalAction::Dahai { tile: tile(0) },
                SuitedSafetyRank::Suji
            )]
        );
    }

    #[test]
    fn suited_dahai_actions_by_safety_excludes_honor_dahai() {
        let discards = [vec![], vec![tile(12)], vec![], vec![]];
        let context = suited_context(vec![], discards, [false, true, false, false]);
        let actions = vec![
            LegalAction::Dahai { tile: tile(108) },
            LegalAction::Dahai { tile: tile(0) },
        ];
        let ranked = suited_dahai_actions_by_safety(&actions, &context);
        assert_eq!(
            ranked,
            vec![(
                &LegalAction::Dahai { tile: tile(0) },
                SuitedSafetyRank::Suji
            )]
        );
    }

    #[test]
    fn suited_dahai_actions_by_safety_orders_by_safety() {
        // 4m 河でスジ判定。2m は4枚見え(NoChance)、3m は3枚見え(OneChance)、
        // 1m はスジ(Suji)、5m は無スジ(NoSafety)。順序が入れ替わっていても安全度順に並ぶ。
        let discards = [vec![], vec![tile(12)], vec![], vec![]];
        let context = suited_context(
            vec![
                tile(4),
                tile(5),
                tile(6),
                tile(7),
                tile(8),
                tile(9),
                tile(10),
            ],
            discards,
            [false, true, false, false],
        );
        let actions = vec![
            LegalAction::Dahai { tile: tile(16) },
            LegalAction::Dahai { tile: tile(0) },
            LegalAction::Dahai { tile: tile(8) },
            LegalAction::Dahai { tile: tile(4) },
        ];
        let ranked = suited_dahai_actions_by_safety(&actions, &context);
        assert_eq!(
            ranked,
            vec![
                (
                    &LegalAction::Dahai { tile: tile(4) },
                    SuitedSafetyRank::NoChance
                ),
                (
                    &LegalAction::Dahai { tile: tile(8) },
                    SuitedSafetyRank::OneChance
                ),
                (
                    &LegalAction::Dahai { tile: tile(0) },
                    SuitedSafetyRank::Suji
                ),
                (
                    &LegalAction::Dahai { tile: tile(16) },
                    SuitedSafetyRank::NoSafety
                ),
            ]
        );
    }

    #[test]
    fn suited_dahai_actions_by_safety_includes_no_safety_and_preserves_order() {
        // リーチ者はいるが河は空・壁も無しなので全て NoSafety。NoSafety も含み元の順序を保つ。
        let context = suited_context(vec![], Default::default(), [false, true, false, false]);
        let actions = vec![
            LegalAction::Dahai { tile: tile(16) },
            LegalAction::Dahai { tile: tile(0) },
            LegalAction::Dahai { tile: tile(32) },
        ];
        let ranked = suited_dahai_actions_by_safety(&actions, &context);
        assert_eq!(
            ranked,
            vec![
                (
                    &LegalAction::Dahai { tile: tile(16) },
                    SuitedSafetyRank::NoSafety
                ),
                (
                    &LegalAction::Dahai { tile: tile(0) },
                    SuitedSafetyRank::NoSafety
                ),
                (
                    &LegalAction::Dahai { tile: tile(32) },
                    SuitedSafetyRank::NoSafety
                ),
            ]
        );
    }

    #[test]
    fn select_suited_safety_fallback_action_none_without_opponent_reach() {
        // 他家リーチがいなければ NoChance でも選ばない。
        let context = suited_context(
            vec![tile(0), tile(1), tile(2), tile(3)],
            Default::default(),
            [false; 4],
        );
        let actions = vec![LegalAction::Dahai { tile: tile(0) }];
        assert_eq!(
            select_suited_safety_fallback_action(&actions, &context),
            None
        );
    }

    #[test]
    fn select_suited_safety_fallback_action_none_when_only_no_safety() {
        // 全て NoSafety の数牌しかない場合は None。
        let context = suited_context(vec![], Default::default(), [false, true, false, false]);
        let actions = vec![
            LegalAction::Dahai { tile: tile(0) },
            LegalAction::Dahai { tile: tile(16) },
        ];
        assert_eq!(
            select_suited_safety_fallback_action(&actions, &context),
            None
        );
    }

    #[test]
    fn select_suited_safety_fallback_action_returns_safest_dahai() {
        // 2m は4枚見え(NoChance)、1m はスジ(Suji)。最も安全な 2m を選ぶ。
        let discards = [vec![], vec![tile(12)], vec![], vec![]];
        let context = suited_context(
            vec![tile(4), tile(5), tile(6), tile(7)],
            discards,
            [false, true, false, false],
        );
        let actions = vec![
            LegalAction::Dahai { tile: tile(0) },
            LegalAction::Dahai { tile: tile(4) },
        ];
        assert_eq!(
            select_suited_safety_fallback_action(&actions, &context),
            Some(&LegalAction::Dahai { tile: tile(4) })
        );
    }
}
