use super::common::*;
use crate::action::LegalAction;
use crate::context::{GameContext, seat_wind_for_player};
use crate::defense::honor::{RankedHonorCandidate, sort_group_by_opponent_honor_value};
use crate::defense::*;

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
fn seat_wind_for_player_derives_from_oya() {
    assert_eq!(seat_wind_for_player(0, 0), Some(honor(EAST)));
    assert_eq!(seat_wind_for_player(1, 0), Some(honor(SOUTH)));
    assert_eq!(seat_wind_for_player(2, 0), Some(honor(WEST)));
    assert_eq!(seat_wind_for_player(3, 0), Some(honor(NORTH)));
    assert_eq!(seat_wind_for_player(1, 1), Some(honor(EAST)));
    assert_eq!(seat_wind_for_player(0, 3), Some(honor(SOUTH)));
    assert_eq!(seat_wind_for_player(1, 3), Some(honor(WEST)));
}

#[test]
fn seat_wind_for_player_rejects_out_of_range() {
    assert_eq!(seat_wind_for_player(4, 0), None);
    assert_eq!(seat_wind_for_player(0, 4), None);
    assert_eq!(seat_wind_for_player(usize::MAX, 0), None);
    assert_eq!(seat_wind_for_player(0, 255), None);
}

#[test]
fn opponent_honor_value_for_dragons_is_single_value_honor() {
    let context = single_reacher_honor_context(3);
    for dragon in [HAKU, HATSU, CHUN] {
        assert_eq!(
            opponent_honor_value_for(honor(dragon), 1, &context),
            Some(OpponentHonorValue::SingleValueHonor)
        );
    }

    let unknown = honor_value_context(
        None,
        None,
        [false, true, false, false],
        Default::default(),
        vec![],
    );
    assert_eq!(
        opponent_honor_value_for(honor(CHUN), 1, &unknown),
        Some(OpponentHonorValue::SingleValueHonor)
    );
}

#[test]
fn opponent_honor_value_for_round_wind_only_is_single_value_honor() {
    let context = single_reacher_honor_context(0);
    assert_eq!(seat_wind_for_player(1, 0), Some(honor(SOUTH)));
    assert_eq!(
        opponent_honor_value_for(honor(EAST), 1, &context),
        Some(OpponentHonorValue::SingleValueHonor)
    );
}

#[test]
fn opponent_honor_value_for_seat_wind_only_is_single_value_honor() {
    let context = single_reacher_honor_context(0);
    assert_eq!(
        opponent_honor_value_for(honor(SOUTH), 1, &context),
        Some(OpponentHonorValue::SingleValueHonor)
    );
}

#[test]
fn opponent_honor_value_for_guest_wind() {
    let context = single_reacher_honor_context(0);
    assert_eq!(
        opponent_honor_value_for(honor(WEST), 1, &context),
        Some(OpponentHonorValue::GuestWind)
    );
    assert_eq!(
        opponent_honor_value_for(honor(NORTH), 1, &context),
        Some(OpponentHonorValue::GuestWind)
    );
}

#[test]
fn opponent_honor_value_for_double_east() {
    let context = single_reacher_honor_context(1);
    assert_eq!(seat_wind_for_player(1, 1), Some(honor(EAST)));
    assert_eq!(
        opponent_honor_value_for(honor(EAST), 1, &context),
        Some(OpponentHonorValue::DoubleWind)
    );
}

#[test]
fn opponent_honor_value_for_double_south() {
    let context = honor_value_context(
        Some(honor(SOUTH)),
        Some(0),
        [false, true, false, false],
        Default::default(),
        vec![],
    );
    assert_eq!(seat_wind_for_player(1, 0), Some(honor(SOUTH)));
    assert_eq!(
        opponent_honor_value_for(honor(SOUTH), 1, &context),
        Some(OpponentHonorValue::DoubleWind)
    );
}

#[test]
fn opponent_honor_value_for_number_tile_is_none() {
    let context = single_reacher_honor_context(0);
    assert_eq!(
        opponent_honor_value_for(tile(0).tile_type(), 1, &context),
        None
    );
    assert_eq!(
        opponent_honor_value_for(tile(16).tile_type(), 1, &context),
        None
    );
}

#[test]
fn opponent_honor_value_for_wind_without_round_wind_is_unknown() {
    let context = honor_value_context(
        None,
        Some(0),
        [false, true, false, false],
        Default::default(),
        vec![],
    );
    for value in [EAST, SOUTH, WEST, NORTH] {
        assert_eq!(opponent_honor_value_for(honor(value), 1, &context), None);
    }
}

#[test]
fn opponent_honor_value_for_wind_without_oya_is_unknown() {
    let context = honor_value_context(
        Some(honor(EAST)),
        None,
        [false, true, false, false],
        Default::default(),
        vec![],
    );
    for value in [EAST, SOUTH, WEST, NORTH] {
        assert_eq!(opponent_honor_value_for(honor(value), 1, &context), None);
    }
}

#[test]
fn opponent_honor_value_for_out_of_range_player_is_unknown() {
    let context = single_reacher_honor_context(0);
    assert_eq!(opponent_honor_value_for(honor(EAST), 4, &context), None);
}

#[test]
fn opponent_honor_value_for_ignores_own_seat_wind() {
    let context = GameContext::from_parts_with_table_state(
        None,
        vec![],
        vec![],
        Some(honor(EAST)),
        Some(honor(NORTH)),
        Vec::new(),
        Some(0),
        Some(0),
        Default::default(),
        [false, true, false, false],
    );
    assert_eq!(
        opponent_honor_value_for(honor(NORTH), 1, &context),
        Some(OpponentHonorValue::GuestWind)
    );
}

#[test]
fn opponent_honor_value_for_reached_takes_most_dangerous_guest_and_guest() {
    let context = honor_value_context(
        Some(honor(EAST)),
        Some(0),
        [false, true, false, true],
        Default::default(),
        vec![],
    );
    assert_eq!(
        opponent_honor_value_for_reached(honor(WEST), &context),
        Some(OpponentHonorValue::GuestWind)
    );
}

#[test]
fn opponent_honor_value_for_reached_takes_most_dangerous_guest_and_single() {
    let context = honor_value_context(
        Some(honor(EAST)),
        Some(0),
        [false, true, true, false],
        Default::default(),
        vec![],
    );
    assert_eq!(
        opponent_honor_value_for_reached(honor(WEST), &context),
        Some(OpponentHonorValue::SingleValueHonor)
    );
}

#[test]
fn opponent_honor_value_for_reached_takes_most_dangerous_single_and_double() {
    let context = GameContext::from_parts_with_table_state(
        None,
        vec![],
        vec![],
        Some(honor(EAST)),
        None,
        Vec::new(),
        Some(3),
        Some(0),
        Default::default(),
        [true, true, false, false],
    );
    assert_eq!(
        opponent_honor_value_for(honor(EAST), 0, &context),
        Some(OpponentHonorValue::DoubleWind)
    );
    assert_eq!(
        opponent_honor_value_for(honor(EAST), 1, &context),
        Some(OpponentHonorValue::SingleValueHonor)
    );
    assert_eq!(
        opponent_honor_value_for_reached(honor(EAST), &context),
        Some(OpponentHonorValue::DoubleWind)
    );
}

#[test]
fn opponent_honor_value_for_reached_excludes_genbutsu_player() {
    let discards = [vec![], vec![], vec![tile(116)], vec![]];
    let context = honor_value_context(
        Some(honor(EAST)),
        Some(0),
        [false, true, true, false],
        discards,
        vec![],
    );
    assert_eq!(
        opponent_honor_value_for(honor(WEST), 2, &context),
        Some(OpponentHonorValue::SingleValueHonor)
    );
    assert_eq!(
        opponent_honor_value_for_reached(honor(WEST), &context),
        Some(OpponentHonorValue::GuestWind)
    );
}

#[test]
fn opponent_honor_value_for_reached_is_unknown_when_all_reachers_have_genbutsu() {
    let discards = [vec![], vec![tile(116)], vec![], vec![]];
    let context = honor_value_context(
        Some(honor(EAST)),
        Some(0),
        [false, true, false, false],
        discards,
        vec![],
    );
    assert_eq!(
        opponent_honor_value_for_reached(honor(WEST), &context),
        None
    );
}

#[test]
fn opponent_honor_value_for_reached_is_unknown_without_reachers() {
    let context = honor_value_context(
        Some(honor(EAST)),
        Some(0),
        [false; 4],
        Default::default(),
        vec![],
    );
    assert_eq!(
        opponent_honor_value_for_reached(honor(WEST), &context),
        None
    );
    assert_eq!(
        opponent_honor_value_for_reached(honor(CHUN), &context),
        None
    );
}

#[test]
fn opponent_honor_value_for_reached_is_none_for_number_tiles() {
    let context = single_reacher_honor_context(0);
    assert_eq!(
        opponent_honor_value_for_reached(tile(0).tile_type(), &context),
        None
    );
}

#[test]
fn honor_dahai_actions_by_safety_breaks_ties_by_opponent_honor_value() {
    let context = single_reacher_honor_context(3);
    let actions = vec![
        LegalAction::Dahai { tile: tile(132) },
        LegalAction::Dahai { tile: tile(120) },
    ];
    let ranked = honor_dahai_actions_by_safety(&actions, &context);
    assert_eq!(
        ranked,
        vec![
            (
                &LegalAction::Dahai { tile: tile(120) },
                HonorSafetyRank::NoVisible
            ),
            (
                &LegalAction::Dahai { tile: tile(132) },
                HonorSafetyRank::NoVisible
            ),
        ]
    );
}

#[test]
fn select_honor_safety_fallback_action_prefers_guest_wind_over_value_honor() {
    let context = single_reacher_honor_context(3);
    let chun = LegalAction::Dahai { tile: tile(132) };
    let north = LegalAction::Dahai { tile: tile(120) };

    assert_eq!(
        select_honor_safety_fallback_action(&[chun.clone(), north.clone()], &context),
        Some(&north)
    );
    assert_eq!(
        select_honor_safety_fallback_action(&[north.clone(), chun.clone()], &context),
        Some(&north)
    );
}

#[test]
fn select_honor_safety_fallback_action_prefers_value_honor_over_double_wind() {
    let context = single_reacher_honor_context(1);
    let east = LegalAction::Dahai { tile: tile(108) };
    let chun = LegalAction::Dahai { tile: tile(132) };

    assert_eq!(
        select_honor_safety_fallback_action(&[east.clone(), chun.clone()], &context),
        Some(&chun)
    );
    assert_eq!(
        select_honor_safety_fallback_action(&[chun.clone(), east.clone()], &context),
        Some(&chun)
    );
}

#[test]
fn select_honor_safety_fallback_action_prefers_guest_wind_over_double_wind() {
    let context = single_reacher_honor_context(1);
    let north = LegalAction::Dahai { tile: tile(120) };
    let east = LegalAction::Dahai { tile: tile(108) };

    assert_eq!(
        select_honor_safety_fallback_action(&[east.clone(), north.clone()], &context),
        Some(&north)
    );
    assert_eq!(
        select_honor_safety_fallback_action(&[north.clone(), east.clone()], &context),
        Some(&north)
    );
}

#[test]
fn select_honor_safety_fallback_action_uses_partial_genbutsu_aggregation() {
    let discards = [vec![], vec![], vec![tile(116)], vec![]];
    let context = honor_value_context(
        Some(honor(EAST)),
        Some(0),
        [false, true, true, false],
        discards,
        vec![],
    );
    let west = LegalAction::Dahai { tile: tile(116) };
    let chun = LegalAction::Dahai { tile: tile(132) };

    assert_eq!(
        opponent_honor_value_for_reached(honor(WEST), &context),
        Some(OpponentHonorValue::GuestWind)
    );
    assert_eq!(
        select_honor_safety_fallback_action(&[chun.clone(), west.clone()], &context),
        Some(&west)
    );
}

#[test]
fn select_honor_safety_fallback_action_keeps_visible_count_priority() {
    let context = honor_value_context(
        Some(honor(EAST)),
        Some(1),
        [false, true, false, false],
        Default::default(),
        vec![tile(132), tile(133), tile(134), tile(116)],
    );
    let west = LegalAction::Dahai { tile: tile(117) };
    let chun = LegalAction::Dahai { tile: tile(135) };

    assert_eq!(
        honor_safety_rank(honor(CHUN), &context),
        Some(HonorSafetyRank::ThreeOrMoreVisible)
    );
    assert_eq!(
        honor_safety_rank(honor(WEST), &context),
        Some(HonorSafetyRank::OneVisible)
    );
    assert_eq!(
        opponent_honor_value_for_reached(honor(WEST), &context),
        Some(OpponentHonorValue::GuestWind)
    );
    assert_eq!(
        select_honor_safety_fallback_action(&[west.clone(), chun.clone()], &context),
        Some(&chun)
    );
}

#[test]
fn honor_dahai_actions_by_safety_preserves_order_for_equal_value_honors() {
    let context = single_reacher_honor_context(1);
    let actions = vec![
        LegalAction::Dahai { tile: tile(132) },
        LegalAction::Dahai { tile: tile(124) },
    ];
    let ranked: Vec<&LegalAction> = honor_dahai_actions_by_safety(&actions, &context)
        .into_iter()
        .map(|(action, _)| action)
        .collect();
    assert_eq!(ranked, vec![&actions[0], &actions[1]]);
}

#[test]
fn honor_dahai_actions_by_safety_preserves_order_for_equal_guest_winds() {
    let context = single_reacher_honor_context(1);
    let actions = vec![
        LegalAction::Dahai { tile: tile(120) },
        LegalAction::Dahai { tile: tile(116) },
    ];
    let ranked: Vec<&LegalAction> = honor_dahai_actions_by_safety(&actions, &context)
        .into_iter()
        .map(|(action, _)| action)
        .collect();
    assert_eq!(ranked, vec![&actions[0], &actions[1]]);
}

#[test]
fn honor_dahai_actions_by_safety_leaves_unknown_value_to_stable_order() {
    let context = honor_value_context(
        None,
        Some(1),
        [false, true, false, false],
        Default::default(),
        vec![],
    );
    let east = LegalAction::Dahai { tile: tile(108) };
    let chun = LegalAction::Dahai { tile: tile(132) };

    assert_eq!(
        opponent_honor_value_for_reached(honor(EAST), &context),
        None
    );
    assert_eq!(
        select_honor_safety_fallback_action(&[east.clone(), chun.clone()], &context),
        Some(&east)
    );
    assert_eq!(
        select_honor_safety_fallback_action(&[chun.clone(), east.clone()], &context),
        Some(&chun)
    );
}

#[test]
fn honor_dahai_actions_by_safety_does_not_reorder_across_unknown() {
    let discards = [vec![], vec![tile(116)], vec![], vec![]];
    let context = honor_value_context(
        Some(honor(EAST)),
        Some(1),
        [false, true, false, false],
        discards,
        vec![tile(117), tile(132), tile(120)],
    );
    let chun = LegalAction::Dahai { tile: tile(133) };
    let west = LegalAction::Dahai { tile: tile(118) };
    let north = LegalAction::Dahai { tile: tile(121) };
    let actions = vec![chun.clone(), west.clone(), north.clone()];

    for tile_type in [honor(CHUN), honor(WEST), honor(NORTH)] {
        assert_eq!(
            honor_safety_rank(tile_type, &context),
            Some(HonorSafetyRank::OneVisible)
        );
    }
    assert_eq!(
        opponent_honor_value_for_reached(honor(WEST), &context),
        None
    );

    let ranked: Vec<&LegalAction> = honor_dahai_actions_by_safety(&actions, &context)
        .into_iter()
        .map(|(action, _)| action)
        .collect();
    assert_eq!(ranked, vec![&chun, &west, &north]);
}

const GUEST: Option<OpponentHonorValue> = Some(OpponentHonorValue::GuestWind);
const VALUE: Option<OpponentHonorValue> = Some(OpponentHonorValue::SingleValueHonor);
const DOUBLE: Option<OpponentHonorValue> = Some(OpponentHonorValue::DoubleWind);
const UNKNOWN: Option<OpponentHonorValue> = None;

fn sorted_group_order(values: &[Option<OpponentHonorValue>]) -> Vec<usize> {
    let actions: Vec<LegalAction> = (0..values.len())
        .map(|index| LegalAction::Dahai {
            tile: tile(108 + index as u8),
        })
        .collect();
    let mut group: Vec<RankedHonorCandidate<'_>> = actions
        .iter()
        .zip(values)
        .map(|(action, &value)| (action, HonorSafetyRank::NoVisible, value))
        .collect();

    sort_group_by_opponent_honor_value(&mut group);

    group
        .iter()
        .map(|candidate| match candidate.0 {
            LegalAction::Dahai { tile } => (tile.raw() - 108) as usize,
            other => panic!("unexpected action: {other:?}"),
        })
        .collect()
}

#[test]
fn sort_group_keeps_order_when_unknown_is_in_the_middle() {
    assert_eq!(sorted_group_order(&[VALUE, UNKNOWN, GUEST]), vec![0, 1, 2]);
}

#[test]
fn sort_group_sorts_only_the_run_before_a_trailing_unknown() {
    assert_eq!(sorted_group_order(&[VALUE, GUEST, UNKNOWN]), vec![1, 0, 2]);
}

#[test]
fn sort_group_sorts_only_the_run_after_a_leading_unknown() {
    assert_eq!(sorted_group_order(&[UNKNOWN, DOUBLE, GUEST]), vec![0, 2, 1]);
}

#[test]
fn sort_group_sorts_each_run_between_unknowns() {
    assert_eq!(
        sorted_group_order(&[DOUBLE, UNKNOWN, VALUE, GUEST, UNKNOWN]),
        vec![0, 1, 3, 2, 4]
    );
}

#[test]
fn sort_group_sorts_every_candidate_when_all_are_known() {
    assert_eq!(sorted_group_order(&[DOUBLE, VALUE, GUEST]), vec![2, 1, 0]);
    assert_eq!(sorted_group_order(&[GUEST, VALUE, DOUBLE]), vec![0, 1, 2]);
}

#[test]
fn sort_group_keeps_order_when_all_are_unknown() {
    assert_eq!(
        sorted_group_order(&[UNKNOWN, UNKNOWN, UNKNOWN]),
        vec![0, 1, 2]
    );
}

#[test]
fn sort_group_keeps_order_within_equal_values() {
    assert_eq!(sorted_group_order(&[VALUE, GUEST, GUEST]), vec![1, 2, 0]);
}

#[test]
fn honor_dahai_actions_by_safety_without_reachers_keeps_stable_order() {
    let context = honor_value_context(
        Some(honor(EAST)),
        Some(1),
        [false; 4],
        Default::default(),
        vec![],
    );
    let east = LegalAction::Dahai { tile: tile(108) };
    let chun = LegalAction::Dahai { tile: tile(132) };

    assert_eq!(
        select_honor_safety_fallback_action(&[east.clone(), chun.clone()], &context),
        Some(&east)
    );
}
