use super::common::*;
use crate::action::LegalAction;
use crate::context::GameContext;
use crate::defense::*;
use bot_logic::TileId;

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
    // 1m は 4m 河でスジ。経路 [2m,3m] を 2m 4枚で Blocked にすると NoChance。壁が最優先。
    let discards = [vec![], vec![tile(12)], vec![], vec![]];
    let context = suited_context(
        vec![tile(4), tile(5), tile(6), tile(7)],
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
    // 1m は 4m 河でスジ。経路 [2m,3m] を 2m 3枚で OneChance にすると OneChance が Suji より優先。
    let discards = [vec![], vec![tile(12)], vec![], vec![]];
    let context = suited_context(
        vec![tile(4), tile(5), tile(6)],
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
    // 経路壁で安全度を作る。1p は 2p 4枚で NoChance、9p は 8p 3枚で OneChance、
    // 1s は 4s 河でスジ(Suji)、5s は無スジ・壁なし(NoSafety)。順序が入れ替わっても安全度順に並ぶ。
    let discards = [vec![], vec![tile(84)], vec![], vec![]];
    let context = suited_context(
        vec![
            tile(40),
            tile(41),
            tile(42),
            tile(43),
            tile(64),
            tile(65),
            tile(66),
        ],
        discards,
        [false, true, false, false],
    );
    let actions = vec![
        LegalAction::Dahai { tile: tile(88) },
        LegalAction::Dahai { tile: tile(72) },
        LegalAction::Dahai { tile: tile(68) },
        LegalAction::Dahai { tile: tile(36) },
    ];
    let ranked = suited_dahai_actions_by_safety(&actions, &context);
    assert_eq!(
        ranked,
        vec![
            (
                &LegalAction::Dahai { tile: tile(36) },
                SuitedSafetyRank::NoChance
            ),
            (
                &LegalAction::Dahai { tile: tile(68) },
                SuitedSafetyRank::OneChance
            ),
            (
                &LegalAction::Dahai { tile: tile(72) },
                SuitedSafetyRank::Suji
            ),
            (
                &LegalAction::Dahai { tile: tile(88) },
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
    // 他家リーチがいなければ、1m が 2m 4枚で NoChance でも選ばない。
    let context = suited_context(
        vec![tile(4), tile(5), tile(6), tile(7)],
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
    // 4m 河でスジ判定。4m を4枚見えにして 2m を NoChance、1m はスジ(Suji)。最も安全な 2m を選ぶ。
    let discards = [vec![], vec![tile(12)], vec![], vec![]];
    let context = suited_context(
        vec![tile(12), tile(13), tile(14), tile(15)],
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

// 5m を NoChance にする visible。経路 [3m,4m] は 4m(12-15)4枚で Blocked、
// 経路 [6m,7m] は 6m(20-23)4枚で Blocked。5m 自身は含めない。
fn five_man_no_chance_visible() -> Vec<TileId> {
    vec![
        tile(12),
        tile(13),
        tile(14),
        tile(15),
        tile(20),
        tile(21),
        tile(22),
        tile(23),
    ]
}

// 上記 visible で他家(player 1)リーチ中の context。
fn five_man_no_chance_visible_context() -> GameContext {
    suited_context(
        five_man_no_chance_visible(),
        Default::default(),
        [false, true, false, false],
    )
}

#[test]
fn select_suited_safety_fallback_action_prefers_black_five() {
    // 5m が NoChance。合法 Dahai [赤5m, 黒5m] なら黒5m を選ぶ。安全度 rank は不変。
    let context = five_man_no_chance_visible_context();
    let actions = vec![
        LegalAction::Dahai { tile: tile(16) },
        LegalAction::Dahai { tile: tile(17) },
    ];
    assert_eq!(
        select_suited_safety_fallback_action(&actions, &context),
        Some(&LegalAction::Dahai { tile: tile(17) })
    );
    assert_eq!(
        suited_safety_rank_for_all_reached(tile(17).tile_type(), &context),
        Some(SuitedSafetyRank::NoChance)
    );
}

#[test]
fn select_suited_safety_fallback_action_prefers_black_five_when_reversed() {
    // 合法 Dahai の順序が [黒5m, 赤5m] でも黒5m を選ぶ。
    let context = five_man_no_chance_visible_context();
    let actions = vec![
        LegalAction::Dahai { tile: tile(17) },
        LegalAction::Dahai { tile: tile(16) },
    ];
    assert_eq!(
        select_suited_safety_fallback_action(&actions, &context),
        Some(&LegalAction::Dahai { tile: tile(17) })
    );
}

#[test]
fn select_suited_safety_fallback_action_keeps_red_five_when_only_red() {
    // 赤5m しか合法でなければ赤5m を維持する。
    let context = five_man_no_chance_visible_context();
    let actions = vec![LegalAction::Dahai { tile: tile(16) }];
    assert_eq!(
        select_suited_safety_fallback_action(&actions, &context),
        Some(&LegalAction::Dahai { tile: tile(16) })
    );
}

#[test]
fn select_suited_safety_fallback_action_keeps_leading_tile_type_over_black_five() {
    // 1p も NoChance にして、合法順 [1p, 赤5m, 黒5m] で 1p と 5m が同じ rank。
    // 先頭牌種 1p を維持し、黒5優先で 5m を前へ出さない。
    // 1p の唯一の経路 [2p,3p] を 2p(40-43)4枚で Blocked にする。
    let mut visible = five_man_no_chance_visible();
    visible.extend([tile(40), tile(41), tile(42), tile(43)]);
    let context = suited_context(visible, Default::default(), [false, true, false, false]);
    let actions = vec![
        LegalAction::Dahai { tile: tile(36) },
        LegalAction::Dahai { tile: tile(16) },
        LegalAction::Dahai { tile: tile(17) },
    ];
    assert_eq!(
        suited_safety_rank_for_all_reached(tile(36).tile_type(), &context),
        Some(SuitedSafetyRank::NoChance)
    );
    assert_eq!(
        select_suited_safety_fallback_action(&actions, &context),
        Some(&LegalAction::Dahai { tile: tile(36) })
    );
}

#[test]
fn suited_safety_rank_for_all_reached_none_for_honor() {
    let context = table_state_context(
        Some(0),
        None,
        Default::default(),
        [false, true, false, false],
    );
    assert_eq!(
        suited_safety_rank_for_all_reached(tile(108).tile_type(), &context),
        None
    );
}

#[test]
fn suited_safety_rank_for_all_reached_keeps_wall_priority_over_suji() {
    // 二人リーチで一人だけにスジの 1m でも、壁評価はスジより優先される。
    let discards = [vec![], vec![tile(12)], vec![], vec![]];
    // 2m を4枚見えにして経路 [2m,3m] を Blocked -> 1m は NoChance。
    let context = suited_context(
        vec![tile(4), tile(5), tile(6), tile(7)],
        discards.clone(),
        [false, true, true, false],
    );
    assert_eq!(
        suited_safety_rank_for_all_reached(tile(0).tile_type(), &context),
        Some(SuitedSafetyRank::NoChance)
    );
    // 2m を3枚見え -> 1m は OneChance。
    let context = suited_context(
        vec![tile(4), tile(5), tile(6)],
        discards,
        [false, true, true, false],
    );
    assert_eq!(
        suited_safety_rank_for_all_reached(tile(0).tile_type(), &context),
        Some(SuitedSafetyRank::OneChance)
    );
}

#[test]
fn suited_dahai_actions_by_safety_uses_all_reached_basis() {
    let context = all_reached_partial_suji_context(vec![]);
    let actions = vec![
        LegalAction::Dahai { tile: tile(0) },
        LegalAction::Dahai { tile: tile(4) },
    ];
    let ranked = suited_dahai_actions_by_safety(&actions, &context);
    assert_eq!(
        ranked,
        vec![
            (
                &LegalAction::Dahai { tile: tile(4) },
                SuitedSafetyRank::Suji
            ),
            (
                &LegalAction::Dahai { tile: tile(0) },
                SuitedSafetyRank::NoSafety
            ),
        ]
    );
}

#[test]
fn select_suited_safety_fallback_action_prefers_all_reached_suji() {
    // 一人だけスジの 1m と全員スジの 2m がある場合、全員スジの 2m を選ぶ。
    let context = all_reached_partial_suji_context(vec![]);
    let actions = vec![
        LegalAction::Dahai { tile: tile(0) },
        LegalAction::Dahai { tile: tile(4) },
    ];
    assert_eq!(
        select_suited_safety_fallback_action(&actions, &context),
        Some(&LegalAction::Dahai { tile: tile(4) })
    );
}

#[test]
fn select_suited_safety_fallback_action_none_when_only_partial_suji() {
    // 一人だけスジの牌しかなく壁もない場合は None。
    let context = all_reached_partial_suji_context(vec![]);
    let actions = vec![LegalAction::Dahai { tile: tile(0) }];
    assert_eq!(
        select_suited_safety_fallback_action(&actions, &context),
        None
    );
}

#[test]
fn suited_safety_rank_reflects_half_suji() {
    // 4p は片スジで HalfSuji、7s は完全スジで Suji。壁はどちらも NoWall。
    let context = half_suji_regression_context();
    let four_pin = tile(48).tile_type();
    let seven_sou = tile(96).tile_type();

    assert_eq!(wall_rank(four_pin, &context), WallRank::NoWall);
    assert_eq!(wall_rank(seven_sou, &context), WallRank::NoWall);
    assert_eq!(
        suji_safety_rank_for_all_reached(four_pin, &context),
        Some(SujiSafetyRank::HalfSuji)
    );
    assert_eq!(
        suji_safety_rank_for_all_reached(seven_sou, &context),
        Some(SujiSafetyRank::Suji)
    );
    assert_eq!(
        suited_safety_rank_for_all_reached(four_pin, &context),
        Some(SuitedSafetyRank::HalfSuji)
    );
    assert_eq!(
        suited_safety_rank_for_all_reached(seven_sou, &context),
        Some(SuitedSafetyRank::Suji)
    );
}

#[test]
fn suited_safety_rank_orders_half_suji_between_suji_and_no_safety() {
    assert!(SuitedSafetyRank::Suji > SuitedSafetyRank::HalfSuji);
    assert!(SuitedSafetyRank::HalfSuji > SuitedSafetyRank::NoSafety);
    assert!(SuitedSafetyRank::OneChance > SuitedSafetyRank::Suji);
    assert!(SujiSafetyRank::Suji > SujiSafetyRank::HalfSuji);
    assert!(SujiSafetyRank::HalfSuji > SujiSafetyRank::NoSuji);
}

#[test]
fn suited_safety_rank_keeps_wall_priority_over_half_suji() {
    // 片スジの 4p でも、経路 [2p,3p] と [5p,6p] を塞げば壁評価が優先される。
    let visible = vec![
        tile(40),
        tile(41),
        tile(42),
        tile(43),
        tile(52),
        tile(53),
        tile(54),
        tile(55),
    ];
    let context = suited_context(
        visible,
        [vec![], vec![tile(36)], vec![], vec![]],
        [false, true, false, false],
    );
    assert_eq!(
        suji_safety_rank_for_all_reached(tile(48).tile_type(), &context),
        Some(SujiSafetyRank::HalfSuji)
    );
    assert_eq!(
        suited_safety_rank_for_all_reached(tile(48).tile_type(), &context),
        Some(SuitedSafetyRank::NoChance)
    );
}

#[test]
fn suited_dahai_actions_by_safety_orders_full_suji_over_half_suji() {
    let context = half_suji_regression_context();
    let actions = vec![
        LegalAction::Dahai { tile: tile(48) },
        LegalAction::Dahai { tile: tile(96) },
    ];
    let ranked = suited_dahai_actions_by_safety(&actions, &context);
    assert_eq!(
        ranked,
        vec![
            (
                &LegalAction::Dahai { tile: tile(96) },
                SuitedSafetyRank::Suji
            ),
            (
                &LegalAction::Dahai { tile: tile(48) },
                SuitedSafetyRank::HalfSuji
            ),
        ]
    );
}

#[test]
fn half_suji_regression_prefers_full_suji_regardless_of_action_order() {
    // 複数リーチの legacy path では、合法 action の順序に関係なく完全スジの 7s を片スジの
    // 4p より優先する。
    let context = multiple_reach_half_suji_regression_context();
    let expected = Some((
        &LegalAction::Dahai { tile: tile(96) },
        DefenseFallbackKind::SuitedSafety(SuitedSafetyRank::Suji),
    ));

    let actions = vec![
        LegalAction::Dahai { tile: tile(48) },
        LegalAction::Dahai { tile: tile(96) },
    ];
    assert_eq!(
        select_defense_fallback_action_with_kind(&context, &actions),
        expected
    );

    let actions = vec![
        LegalAction::Dahai { tile: tile(96) },
        LegalAction::Dahai { tile: tile(48) },
    ];
    assert_eq!(
        select_defense_fallback_action_with_kind(&context, &actions),
        expected
    );
}

#[test]
fn select_suited_safety_fallback_action_takes_half_suji_over_no_safety() {
    // 片スジは無スジより安全なので、他に候補が無ければ片スジを選ぶ。
    let context = half_suji_regression_context();
    let actions = vec![
        LegalAction::Dahai { tile: tile(0) },
        LegalAction::Dahai { tile: tile(48) },
    ];
    assert_eq!(
        suited_safety_rank_for_all_reached(tile(0).tile_type(), &context),
        Some(SuitedSafetyRank::NoSafety)
    );
    assert_eq!(
        select_suited_safety_fallback_action(&actions, &context),
        Some(&LegalAction::Dahai { tile: tile(48) })
    );
}

#[test]
fn one_visible_double_wind_is_outweighed_by_full_suji() {
    assert!(suited_safety_outweighs_honor(
        HonorSafetyRank::OneVisible,
        Some(OpponentHonorValue::DoubleWind),
        SuitedSafetyRank::Suji,
    ));
}

#[test]
fn cross_category_rule_does_not_demote_clearly_safe_honors() {
    for honor_rank in [
        HonorSafetyRank::TwoVisible,
        HonorSafetyRank::ThreeOrMoreVisible,
    ] {
        assert!(!suited_safety_outweighs_honor(
            honor_rank,
            Some(OpponentHonorValue::DoubleWind),
            SuitedSafetyRank::NoChance,
        ));
    }
    assert!(!suited_safety_outweighs_honor(
        HonorSafetyRank::OneVisible,
        Some(OpponentHonorValue::GuestWind),
        SuitedSafetyRank::NoChance,
    ));
}

// ---- SuitedSafetyEvidence ----

fn suited_evidence(wall_rank: WallRank, suji_rank: SujiSafetyRank) -> SuitedSafetyEvidence {
    SuitedSafetyEvidence {
        wall_rank,
        suji_rank,
    }
}

// 壁とスジの全組み合わせ。evidence が失わずに保持すべき9通り。
fn all_suited_evidence_combinations() -> Vec<SuitedSafetyEvidence> {
    [WallRank::NoWall, WallRank::OneChance, WallRank::NoChance]
        .into_iter()
        .flat_map(|wall_rank| {
            [
                SujiSafetyRank::NoSuji,
                SujiSafetyRank::HalfSuji,
                SujiSafetyRank::Suji,
            ]
            .into_iter()
            .map(move |suji_rank| suited_evidence(wall_rank, suji_rank))
        })
        .collect()
}

#[test]
fn suited_safety_evidence_distinguishes_every_wall_and_suji_combination() {
    let combinations = all_suited_evidence_combinations();
    assert_eq!(combinations.len(), 9);
    for (left_index, left) in combinations.iter().enumerate() {
        for (right_index, right) in combinations.iter().enumerate() {
            assert_eq!(left == right, left_index == right_index);
        }
    }
}

#[test]
fn suited_safety_evidence_legacy_rank_matches_the_existing_mapping() {
    for suji_rank in [
        SujiSafetyRank::NoSuji,
        SujiSafetyRank::HalfSuji,
        SujiSafetyRank::Suji,
    ] {
        assert_eq!(
            suited_evidence(WallRank::NoChance, suji_rank).legacy_rank(),
            SuitedSafetyRank::NoChance
        );
        assert_eq!(
            suited_evidence(WallRank::OneChance, suji_rank).legacy_rank(),
            SuitedSafetyRank::OneChance
        );
    }
    for (suji_rank, legacy_rank) in [
        (SujiSafetyRank::Suji, SuitedSafetyRank::Suji),
        (SujiSafetyRank::HalfSuji, SuitedSafetyRank::HalfSuji),
        (SujiSafetyRank::NoSuji, SuitedSafetyRank::NoSafety),
    ] {
        assert_eq!(
            suited_evidence(WallRank::NoWall, suji_rank).legacy_rank(),
            legacy_rank
        );
    }
}

#[test]
fn suited_safety_evidence_none_for_honor() {
    let context = suited_context(vec![], Default::default(), [false, true, false, false]);
    let honor = tile(108).tile_type();
    assert_eq!(
        suited_safety_evidence_for_players(honor, &[1], &context),
        None
    );
    assert_eq!(
        suited_safety_evidence_for_all_reached(honor, &context),
        None
    );
    assert_eq!(
        suited_safety_evidence_for_any_reached(honor, &context),
        None
    );
}

#[test]
fn suited_safety_evidence_keeps_the_suji_under_a_one_chance_wall() {
    // 1m は 4m 河でスジ。経路 [2m,3m] を 2m 3枚で OneChance にしてもスジ根拠は残る。
    let context = suited_context(
        vec![tile(4), tile(5), tile(6)],
        [vec![], vec![tile(12)], vec![], vec![]],
        [false, true, false, false],
    );
    let one_man = tile(0).tile_type();
    let evidence =
        suited_safety_evidence_for_all_reached(one_man, &context).expect("数牌の evidence");

    assert_eq!(
        evidence,
        suited_evidence(WallRank::OneChance, SujiSafetyRank::Suji)
    );
    assert_eq!(evidence.legacy_rank(), SuitedSafetyRank::OneChance);
    assert_eq!(
        suited_safety_rank_for_all_reached(one_man, &context),
        Some(SuitedSafetyRank::OneChance)
    );
}

#[test]
fn suited_safety_evidence_keeps_the_suji_under_a_no_chance_wall() {
    // 1m は 4m 河でスジ。経路 [2m,3m] を 2m 4枚で NoChance にしてもスジ根拠は残る。
    let context = suited_context(
        vec![tile(4), tile(5), tile(6), tile(7)],
        [vec![], vec![tile(12)], vec![], vec![]],
        [false, true, false, false],
    );
    let one_man = tile(0).tile_type();
    let evidence =
        suited_safety_evidence_for_all_reached(one_man, &context).expect("数牌の evidence");

    assert_eq!(
        evidence,
        suited_evidence(WallRank::NoChance, SujiSafetyRank::Suji)
    );
    assert_eq!(evidence.legacy_rank(), SuitedSafetyRank::NoChance);
    assert_eq!(
        suited_safety_rank_for_all_reached(one_man, &context),
        Some(SuitedSafetyRank::NoChance)
    );
}

#[test]
fn suited_safety_evidence_keeps_the_half_suji_under_a_one_chance_wall() {
    // 5m は 2m 河だけで片スジ。経路 [3m,4m] を 3m 3枚、[6m,7m] を 6m 4枚で塞ぐと OneChance。
    let visible = vec![
        tile(8),
        tile(9),
        tile(10),
        tile(20),
        tile(21),
        tile(22),
        tile(23),
    ];
    let context = suited_context(
        visible,
        [vec![], vec![tile(4)], vec![], vec![]],
        [false, true, false, false],
    );
    let five_man = tile(16).tile_type();
    let evidence =
        suited_safety_evidence_for_all_reached(five_man, &context).expect("数牌の evidence");

    assert_eq!(
        evidence,
        suited_evidence(WallRank::OneChance, SujiSafetyRank::HalfSuji)
    );
    assert_eq!(evidence.legacy_rank(), SuitedSafetyRank::OneChance);
}

#[test]
fn suited_safety_evidence_without_a_wall_carries_the_suji_rank() {
    // 壁が無い局面では legacy rank がスジ評価そのままの写像になる。
    let context = half_suji_regression_context();
    for (tile_id, suji_rank, legacy_rank) in [
        (96, SujiSafetyRank::Suji, SuitedSafetyRank::Suji),
        (48, SujiSafetyRank::HalfSuji, SuitedSafetyRank::HalfSuji),
        (0, SujiSafetyRank::NoSuji, SuitedSafetyRank::NoSafety),
    ] {
        let tile_type = tile(tile_id).tile_type();
        let evidence =
            suited_safety_evidence_for_all_reached(tile_type, &context).expect("数牌の evidence");

        assert_eq!(evidence, suited_evidence(WallRank::NoWall, suji_rank));
        assert_eq!(evidence.legacy_rank(), legacy_rank);
        assert_eq!(
            suited_safety_rank_for_all_reached(tile_type, &context),
            Some(legacy_rank)
        );
    }
}

#[test]
fn suited_safety_evidence_with_only_a_wall_keeps_no_suji() {
    // リーチ者の河が空なのでスジ根拠は無い。壁だけで legacy rank が決まる。
    for (visible, wall_rank, legacy_rank) in [
        (
            vec![tile(4), tile(5), tile(6)],
            WallRank::OneChance,
            SuitedSafetyRank::OneChance,
        ),
        (
            vec![tile(4), tile(5), tile(6), tile(7)],
            WallRank::NoChance,
            SuitedSafetyRank::NoChance,
        ),
    ] {
        let context = suited_context(visible, Default::default(), [false, true, false, false]);
        let one_man = tile(0).tile_type();
        let evidence =
            suited_safety_evidence_for_all_reached(one_man, &context).expect("数牌の evidence");

        assert_eq!(evidence, suited_evidence(wall_rank, SujiSafetyRank::NoSuji));
        assert_eq!(evidence.legacy_rank(), legacy_rank);
    }
}

#[test]
fn suited_safety_evidence_for_all_reached_excludes_genbutsu_reachers() {
    // player1 の河に 1m があるので集約対象から外れ、player2 の 4m だけでスジが成立する。
    let context = suited_context(
        vec![],
        [vec![], vec![tile(0)], vec![tile(12)], vec![]],
        [false, true, true, false],
    );
    let one_man = tile(0).tile_type();

    assert_eq!(
        suited_safety_evidence_for_all_reached(one_man, &context),
        Some(suited_evidence(WallRank::NoWall, SujiSafetyRank::Suji))
    );
    assert_eq!(
        suited_safety_evidence_for_players(one_man, &[1, 2], &context),
        Some(suited_evidence(WallRank::NoWall, SujiSafetyRank::NoSuji))
    );
}

#[test]
fn suited_safety_evidence_for_any_reached_keeps_the_safest_suji_aggregation() {
    // player1 にだけスジ。any_reached は最も安全な評価、all_reached は最も危険な評価。
    let context = suited_context(
        vec![],
        [vec![], vec![tile(12)], vec![], vec![]],
        [false, true, true, false],
    );
    let one_man = tile(0).tile_type();

    assert_eq!(
        suited_safety_evidence_for_any_reached(one_man, &context),
        Some(suited_evidence(WallRank::NoWall, SujiSafetyRank::Suji))
    );
    assert_eq!(
        suited_safety_evidence_for_all_reached(one_man, &context),
        Some(suited_evidence(WallRank::NoWall, SujiSafetyRank::NoSuji))
    );
}

#[test]
fn suited_safety_evidence_for_players_treats_an_empty_set_as_no_suji() {
    let context = suited_context(
        vec![tile(4), tile(5), tile(6)],
        [vec![], vec![tile(12)], vec![], vec![]],
        [false, true, false, false],
    );

    assert_eq!(
        suited_safety_evidence_for_players(tile(0).tile_type(), &[], &context),
        Some(suited_evidence(WallRank::OneChance, SujiSafetyRank::NoSuji))
    );
}

#[test]
fn suited_safety_evidence_does_not_change_the_production_ordering() {
    // 1m は NoWall + Suji、9s は OneChance + NoSuji。evidence を導入しても legacy ranking では
    // 壁のある 9s が先。composite comparator を入れるまで production の順位は変えない。
    let context = suited_context(
        vec![tile(96), tile(97), tile(98)],
        [vec![], vec![tile(12)], vec![], vec![]],
        [false, true, false, false],
    );
    let one_man = tile(0).tile_type();
    let nine_sou = tile(104).tile_type();

    assert_eq!(
        suited_safety_evidence_for_all_reached(one_man, &context),
        Some(suited_evidence(WallRank::NoWall, SujiSafetyRank::Suji))
    );
    assert_eq!(
        suited_safety_evidence_for_all_reached(nine_sou, &context),
        Some(suited_evidence(WallRank::OneChance, SujiSafetyRank::NoSuji))
    );

    let actions = vec![
        LegalAction::Dahai { tile: tile(0) },
        LegalAction::Dahai { tile: tile(104) },
    ];
    assert_eq!(
        suited_dahai_actions_by_safety(&actions, &context),
        vec![
            (
                &LegalAction::Dahai { tile: tile(104) },
                SuitedSafetyRank::OneChance
            ),
            (
                &LegalAction::Dahai { tile: tile(0) },
                SuitedSafetyRank::Suji
            ),
        ]
    );
    assert_eq!(
        select_suited_safety_fallback_action(&actions, &context),
        Some(&LegalAction::Dahai { tile: tile(104) })
    );
}
