use super::common::*;
use crate::action::LegalAction;
use crate::context::GameContext;
use crate::defense::*;
use bot_logic::TileId;

#[test]
fn is_genbutsu_for_detects_discarded_tile_type() {
    let discards = [vec![tile(0)], vec![tile(16)], vec![], vec![]];
    let context = table_state_context(Some(3), None, discards, [false; 4]);
    let one_man = tile(0).tile_type();
    assert!(is_genbutsu_for(one_man, 0, &context));
    assert!(!is_genbutsu_for(one_man, 1, &context));
}

#[test]
fn is_discarded_by_player_only_looks_at_that_players_river() {
    let discards = [vec![tile(0)], vec![tile(16)], vec![], vec![]];
    let context = table_state_context(Some(3), None, discards, [false; 4]);
    let one_man = tile(0).tile_type();

    assert!(is_discarded_by_player(one_man, 0, &context));
    // 他家が切っただけの牌は、その player 自身の河ではない。
    assert!(!is_discarded_by_player(one_man, 1, &context));
    assert!(!is_discarded_by_player(one_man, 2, &context));
}

#[test]
fn is_discarded_by_player_treats_red_five_as_the_same_type() {
    // 河に通常5m(tile 17)、判定対象が赤5m相当(tile 16)。
    let discards = [vec![], vec![tile(17)], vec![], vec![]];
    let context = table_state_context(Some(0), None, discards, [false; 4]);

    assert!(is_discarded_by_player(tile(16).tile_type(), 1, &context));
}

#[test]
fn is_discarded_by_player_out_of_range_player_is_false() {
    let context = GameContext::default();
    assert!(!is_discarded_by_player(tile(0).tile_type(), 4, &context));
}

#[test]
fn is_discarded_by_all_players_needs_every_player_and_a_non_empty_set() {
    let discards = [vec![], vec![tile(16)], vec![tile(17)], vec![]];
    let context = table_state_context(Some(0), None, discards, [false; 4]);
    let five_man = tile(16).tile_type();

    assert!(is_discarded_by_all_players(five_man, &[1, 2], &context));
    assert!(!is_discarded_by_all_players(five_man, &[1, 3], &context));
    // 対象がいないことを「全員に通る」と扱わない。
    assert!(!is_discarded_by_all_players(five_man, &[], &context));
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
fn reach_declaration_tile_stays_genbutsu_for_the_reacher() {
    let discards = [vec![], vec![discarded("3p")], vec![], vec![]];
    let context = post_reach_context(
        Some(0),
        discards,
        [false, true, false, false],
        Default::default(),
    );
    assert!(is_genbutsu_for(tile_type("3p"), 1, &context));
    assert!(is_genbutsu_for_all_reached(tile_type("3p"), &context));
}

#[test]
fn tile_passed_after_single_reach_is_genbutsu_for_the_reacher() {
    let discards = [vec![], vec![discarded("3p")], vec![discarded("4s")], vec![]];
    let context = post_reach_context(
        Some(0),
        discards,
        [false, true, false, false],
        [vec![], vec![tile_type("4s")], vec![], vec![]],
    );
    assert!(is_genbutsu_for(tile_type("4s"), 1, &context));
    assert!(is_genbutsu_for_all_reached(tile_type("4s"), &context));
}

#[test]
fn tile_discarded_before_reach_is_not_genbutsu_for_the_reacher() {
    let discards = [vec![], vec![discarded("3p")], vec![discarded("4s")], vec![]];
    let context = post_reach_context(
        Some(0),
        discards,
        [false, true, false, false],
        Default::default(),
    );
    assert!(!is_genbutsu_for(tile_type("4s"), 1, &context));
    assert!(!is_genbutsu_for_all_reached(tile_type("4s"), &context));
}

#[test]
fn declaration_tile_of_a_later_reach_is_genbutsu_for_both_reachers() {
    let discards = [vec![], vec![discarded("3p")], vec![discarded("4s")], vec![]];
    let context = post_reach_context(
        Some(0),
        discards,
        [false, true, true, false],
        [vec![], vec![tile_type("4s")], vec![], vec![]],
    );
    assert!(is_genbutsu_for(tile_type("4s"), 1, &context));
    assert!(is_genbutsu_for(tile_type("4s"), 2, &context));
    assert!(is_genbutsu_for_all_reached(tile_type("4s"), &context));
}

#[test]
fn genbutsu_fallback_selects_the_tile_passed_after_reach() {
    let discards = [vec![], vec![discarded("3p")], vec![discarded("4s")], vec![]];
    let context = post_reach_context(
        Some(0),
        discards,
        [false, true, true, false],
        [vec![], vec![tile_type("4s")], vec![], vec![]],
    );
    let four_sou = LegalAction::Dahai { tile: held("4s") };
    let actions = vec![LegalAction::Dahai { tile: tile(0) }, four_sou.clone()];

    assert_eq!(
        select_defense_fallback_action_with_kind(&context, &actions),
        Some((&four_sou, DefenseFallbackKind::Genbutsu))
    );
}

#[test]
fn is_genbutsu_for_is_the_players_own_river_or_a_post_reach_passed_tile() {
    // 現物の2つの根拠を分けても、既存 semantics (OR) は変わらない。
    let discards = [vec![], vec![discarded("3p")], vec![discarded("4s")], vec![]];
    let context = post_reach_context(
        Some(0),
        discards,
        [false, true, false, false],
        [vec![], vec![tile_type("4s")], vec![], vec![]],
    );

    // 本人の河の牌。post_reach_passed には無い。
    assert!(is_discarded_by_player(tile_type("3p"), 1, &context));
    assert!(!context.is_post_reach_passed(tile_type("3p"), 1));
    assert!(is_genbutsu_for(tile_type("3p"), 1, &context));

    // リーチ後に通った牌。本人の河には無い。
    assert!(!is_discarded_by_player(tile_type("4s"), 1, &context));
    assert!(context.is_post_reach_passed(tile_type("4s"), 1));
    assert!(is_genbutsu_for(tile_type("4s"), 1, &context));

    // どちらの根拠も無い牌。
    assert!(!is_discarded_by_player(tile_type("6p"), 1, &context));
    assert!(!context.is_post_reach_passed(tile_type("6p"), 1));
    assert!(!is_genbutsu_for(tile_type("6p"), 1, &context));
}

#[test]
fn post_reach_passed_tiles_are_tracked_per_player() {
    let discards = [
        vec![],
        vec![discarded("3p")],
        vec![discarded("6p")],
        vec![discarded("7s")],
    ];
    let context = post_reach_context(
        Some(0),
        discards,
        [false, true, true, false],
        [vec![], vec![tile_type("7s")], vec![], vec![]],
    );
    assert!(is_genbutsu_for(tile_type("7s"), 1, &context));
    assert!(!is_genbutsu_for(tile_type("7s"), 2, &context));
    assert!(!is_genbutsu_for_all_reached(tile_type("7s"), &context));
}

#[test]
fn tile_passed_after_three_reaches_is_genbutsu_for_all_of_them() {
    let context = post_reach_context(
        Some(0),
        Default::default(),
        [false, true, true, true],
        [
            vec![],
            vec![tile_type("4s")],
            vec![tile_type("4s")],
            vec![tile_type("4s")],
        ],
    );
    assert_eq!(context.reached_opponents(), vec![1, 2, 3]);
    assert!(is_genbutsu_for_all_reached(tile_type("4s"), &context));
}

#[test]
fn post_reach_passed_black_five_covers_the_red_five() {
    let context = post_reach_context(
        Some(0),
        Default::default(),
        [false, true, false, false],
        [vec![], vec![tile_type("5s")], vec![], vec![]],
    );
    let red_five_sou = TileId::new(88).unwrap();
    assert!(red_five_sou.is_red());
    assert_eq!(red_five_sou.tile_type(), tile_type("5s"));
    assert!(is_genbutsu_for(red_five_sou.tile_type(), 1, &context));
    assert!(is_genbutsu_for_all_reached(
        red_five_sou.tile_type(),
        &context
    ));
}

#[test]
fn own_post_reach_passed_tiles_do_not_make_a_tile_genbutsu_for_all_reached() {
    let context = post_reach_context(
        Some(0),
        Default::default(),
        [true, true, false, false],
        [vec![tile_type("4s")], vec![], vec![], vec![]],
    );
    assert!(!is_genbutsu_for_all_reached(tile_type("4s"), &context));
}

#[test]
fn post_reach_passed_tiles_of_out_of_range_player_is_none() {
    let context = GameContext::default();
    assert_eq!(context.post_reach_passed_tiles_of(4), None);
    assert!(!is_genbutsu_for(tile_type("4s"), 4, &context));
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

#[test]
fn select_genbutsu_fallback_action_prefers_black_five() {
    // 河に5m系があり現物。合法 Dahai [赤5m, 黒5m] なら黒5m を選ぶ。
    let discards = [vec![], vec![tile(17)], vec![], vec![]];
    let context = table_state_context(Some(0), None, discards, [false, true, false, false]);
    let actions = vec![
        LegalAction::Dahai { tile: tile(16) },
        LegalAction::Dahai { tile: tile(17) },
    ];
    assert_eq!(
        select_genbutsu_fallback_action(&context, &actions),
        Some(&LegalAction::Dahai { tile: tile(17) })
    );
}

#[test]
fn select_genbutsu_fallback_action_prefers_black_five_when_reversed() {
    // 合法 Dahai の順序が [黒5m, 赤5m] でも黒5m を選ぶ。
    let discards = [vec![], vec![tile(17)], vec![], vec![]];
    let context = table_state_context(Some(0), None, discards, [false, true, false, false]);
    let actions = vec![
        LegalAction::Dahai { tile: tile(17) },
        LegalAction::Dahai { tile: tile(16) },
    ];
    assert_eq!(
        select_genbutsu_fallback_action(&context, &actions),
        Some(&LegalAction::Dahai { tile: tile(17) })
    );
}

#[test]
fn select_genbutsu_fallback_action_keeps_red_five_when_only_red() {
    // 赤5m しか合法でなければ赤5m を維持する。
    let discards = [vec![], vec![tile(17)], vec![], vec![]];
    let context = table_state_context(Some(0), None, discards, [false, true, false, false]);
    let actions = vec![LegalAction::Dahai { tile: tile(16) }];
    assert_eq!(
        select_genbutsu_fallback_action(&context, &actions),
        Some(&LegalAction::Dahai { tile: tile(16) })
    );
}

#[test]
fn select_genbutsu_fallback_action_keeps_leading_tile_type_over_black_five() {
    // 合法順 [1p, 赤5m, 黒5m] で 1p と 5m系がともに現物。先頭牌種 1p を維持する。
    let discards = [vec![], vec![tile(17), tile(36)], vec![], vec![]];
    let context = table_state_context(Some(0), None, discards, [false, true, false, false]);
    let actions = vec![
        LegalAction::Dahai { tile: tile(36) },
        LegalAction::Dahai { tile: tile(16) },
        LegalAction::Dahai { tile: tile(17) },
    ];
    assert_eq!(
        select_genbutsu_fallback_action(&context, &actions),
        Some(&LegalAction::Dahai { tile: tile(36) })
    );
}

#[test]
fn select_genbutsu_fallback_action_normalizes_black_five_when_type_leads() {
    // 合法順 [赤5m, 1p, 黒5m] で 1p と 5m系がともに現物。先頭牌種 5m のまま黒5m へ正規化する。
    let discards = [vec![], vec![tile(17), tile(36)], vec![], vec![]];
    let context = table_state_context(Some(0), None, discards, [false, true, false, false]);
    let actions = vec![
        LegalAction::Dahai { tile: tile(16) },
        LegalAction::Dahai { tile: tile(36) },
        LegalAction::Dahai { tile: tile(17) },
    ];
    assert_eq!(
        select_genbutsu_fallback_action(&context, &actions),
        Some(&LegalAction::Dahai { tile: tile(17) })
    );
}
