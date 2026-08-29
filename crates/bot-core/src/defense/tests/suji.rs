use super::common::*;
use crate::action::LegalAction;
use crate::context::GameContext;
use crate::defense::*;
use bot_logic::TileId;

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
    // 1m 河 → 4m は片スジなので is_suji_for は false、4m 河 → 1m/7m は完全スジ。
    let context = single_reacher_discards_context(vec![tile(0)]);
    assert!(!is_suji_for(tile(12).tile_type(), 1, &context));

    let context = single_reacher_discards_context(vec![tile(24)]);
    assert!(!is_suji_for(tile(12).tile_type(), 1, &context));

    let context = single_reacher_discards_context(vec![tile(0), tile(24)]);
    assert!(is_suji_for(tile(12).tile_type(), 1, &context));

    let context = single_reacher_discards_context(vec![tile(12)]);
    assert!(is_suji_for(tile(0).tile_type(), 1, &context));
    assert!(is_suji_for(tile(24).tile_type(), 1, &context));
}

#[test]
fn is_suji_for_detects_two_five_eight() {
    // 5m が河にあれば 2m と 8m はスジ。5m は片スジなので false。
    let context = single_reacher_discards_context(vec![tile(16)]);
    assert!(is_suji_for(tile(4).tile_type(), 1, &context));
    assert!(is_suji_for(tile(28).tile_type(), 1, &context));
    assert!(!is_suji_for(tile(16).tile_type(), 1, &context));
}

#[test]
fn is_suji_for_detects_three_six_nine() {
    // 6m が河にあれば 3m と 9m はスジ。6m は片スジなので false。
    let context = single_reacher_discards_context(vec![tile(20)]);
    assert!(is_suji_for(tile(8).tile_type(), 1, &context));
    assert!(is_suji_for(tile(32).tile_type(), 1, &context));
    assert!(!is_suji_for(tile(20).tile_type(), 1, &context));
}

// player1 だけがリーチしている状況で、その河だけを差し替える helper。
fn single_reacher_discards_context(discards: Vec<TileId>) -> GameContext {
    table_state_context(
        Some(0),
        None,
        [vec![], discards, vec![], vec![]],
        [false, true, false, false],
    )
}

// 単独リーチ者の河から、対象牌のスジ安全度を求める。
fn single_reacher_suji_rank(target: u8, discards: Vec<TileId>) -> Option<SujiSafetyRank> {
    let context = single_reacher_discards_context(discards);
    suji_safety_rank_for_all_reached(tile(target).tile_type(), &context)
}

#[test]
fn suji_safety_rank_for_four_distinguishes_half_and_full_suji() {
    // 4p は 1p-4p と 4p-7p の2本。1p(36) / 7p(60) の有無で NoSuji / HalfSuji / Suji。
    assert_eq!(
        single_reacher_suji_rank(48, vec![]),
        Some(SujiSafetyRank::NoSuji)
    );
    assert_eq!(
        single_reacher_suji_rank(48, vec![tile(36)]),
        Some(SujiSafetyRank::HalfSuji)
    );
    assert_eq!(
        single_reacher_suji_rank(48, vec![tile(60)]),
        Some(SujiSafetyRank::HalfSuji)
    );
    assert_eq!(
        single_reacher_suji_rank(48, vec![tile(36), tile(60)]),
        Some(SujiSafetyRank::Suji)
    );
}

#[test]
fn suji_safety_rank_for_five_distinguishes_half_and_full_suji() {
    // 5p は 2p(40) と 8p(64) の2本。
    assert_eq!(
        single_reacher_suji_rank(52, vec![]),
        Some(SujiSafetyRank::NoSuji)
    );
    assert_eq!(
        single_reacher_suji_rank(52, vec![tile(40)]),
        Some(SujiSafetyRank::HalfSuji)
    );
    assert_eq!(
        single_reacher_suji_rank(52, vec![tile(64)]),
        Some(SujiSafetyRank::HalfSuji)
    );
    assert_eq!(
        single_reacher_suji_rank(52, vec![tile(40), tile(64)]),
        Some(SujiSafetyRank::Suji)
    );
}

#[test]
fn suji_safety_rank_for_six_distinguishes_half_and_full_suji() {
    // 6p は 3p(44) と 9p(68) の2本。
    assert_eq!(
        single_reacher_suji_rank(56, vec![]),
        Some(SujiSafetyRank::NoSuji)
    );
    assert_eq!(
        single_reacher_suji_rank(56, vec![tile(44)]),
        Some(SujiSafetyRank::HalfSuji)
    );
    assert_eq!(
        single_reacher_suji_rank(56, vec![tile(68)]),
        Some(SujiSafetyRank::HalfSuji)
    );
    assert_eq!(
        single_reacher_suji_rank(56, vec![tile(44), tile(68)]),
        Some(SujiSafetyRank::Suji)
    );
}

#[test]
fn suji_safety_rank_for_terminal_side_is_never_half_suji() {
    // 1/2/3 と 7/8/9 はスジが1本だけ。対応牌があれば Suji、無ければ NoSuji。
    for (target, partner) in [
        (36u8, 48u8),
        (40, 52),
        (44, 56),
        (60, 48),
        (64, 52),
        (68, 56),
    ] {
        assert_eq!(
            single_reacher_suji_rank(target, vec![tile(partner)]),
            Some(SujiSafetyRank::Suji)
        );
        assert_eq!(
            single_reacher_suji_rank(target, vec![]),
            Some(SujiSafetyRank::NoSuji)
        );
    }
}

#[test]
fn suji_safety_rank_for_honor_is_none() {
    // 字牌はスジ評価対象外。player 単位でも全リーチ者基準でも None。
    let context = single_reacher_discards_context(vec![tile(12)]);
    assert_eq!(
        suji_safety_rank_for(tile(108).tile_type(), 1, &context),
        None
    );
    assert_eq!(
        suji_safety_rank_for_all_reached(tile(108).tile_type(), &context),
        None
    );
}

#[test]
fn suji_safety_rank_for_out_of_range_player_is_no_suji() {
    // 河を取得できない player は推測せず NoSuji。安全側へは倒さない。
    let context = single_reacher_discards_context(vec![tile(12)]);
    assert_eq!(
        suji_safety_rank_for(tile(0).tile_type(), 4, &context),
        Some(SujiSafetyRank::NoSuji)
    );
}

// 二人リーチで、player1 の河と player2 の河を個別に与える helper。
fn two_reachers_context(first: Vec<TileId>, second: Vec<TileId>) -> GameContext {
    table_state_context(
        Some(0),
        None,
        [vec![], first, second, vec![]],
        [false, true, true, false],
    )
}

#[test]
fn suji_safety_rank_for_all_reached_takes_most_dangerous_rank() {
    // 4p を対象に、二人のリーチ者の rank の最小値を採る。
    let four_pin = tile(48).tile_type();

    // 両者とも 1p/7p 持ち → Suji。
    let context = two_reachers_context(vec![tile(36), tile(60)], vec![tile(37), tile(61)]);
    assert_eq!(
        suji_safety_rank_for_all_reached(four_pin, &context),
        Some(SujiSafetyRank::Suji)
    );

    // player1 は Suji、player2 は 1p だけで HalfSuji → 全体は HalfSuji。
    let context = two_reachers_context(vec![tile(36), tile(60)], vec![tile(37)]);
    assert_eq!(
        suji_safety_rank_for(four_pin, 1, &context),
        Some(SujiSafetyRank::Suji)
    );
    assert_eq!(
        suji_safety_rank_for(four_pin, 2, &context),
        Some(SujiSafetyRank::HalfSuji)
    );
    assert_eq!(
        suji_safety_rank_for_all_reached(four_pin, &context),
        Some(SujiSafetyRank::HalfSuji)
    );

    // player1 は Suji、player2 は根拠なしで NoSuji → 全体は NoSuji。
    let context = two_reachers_context(vec![tile(36), tile(60)], vec![]);
    assert_eq!(
        suji_safety_rank_for_all_reached(four_pin, &context),
        Some(SujiSafetyRank::NoSuji)
    );
}

#[test]
fn genbutsu_in_own_river_is_excluded_from_reached_suji_aggregation() {
    let nine_man = tile_type("9m");
    let context = two_reachers_context(vec![discarded("9m")], vec![discarded("6m")]);

    assert!(is_discarded_by_player(nine_man, 1, &context));
    assert!(is_genbutsu_for(nine_man, 1, &context));
    assert_eq!(
        suji_safety_rank_for(nine_man, 1, &context),
        Some(SujiSafetyRank::NoSuji)
    );
    assert!(!is_genbutsu_for(nine_man, 2, &context));
    assert_eq!(
        suji_safety_rank_for(nine_man, 2, &context),
        Some(SujiSafetyRank::Suji)
    );
    assert_eq!(
        suji_safety_rank_for_all_reached(nine_man, &context),
        Some(SujiSafetyRank::Suji)
    );
    assert_eq!(
        suited_safety_rank_for_all_reached(nine_man, &context),
        Some(SuitedSafetyRank::Suji)
    );

    let action = LegalAction::Dahai { tile: held("9m") };
    let candidate = DefenseCandidateDiagnostic::for_dahai_action(&context, &action, false).unwrap();
    assert_eq!(
        candidate.suji_safety_rank_for_all_reached,
        Some(SujiSafetyRank::Suji)
    );
    assert_eq!(candidate.suited_safety_rank, Some(SuitedSafetyRank::Suji));

    let diagnostic = DefenseFallbackDiagnostic::from_selection(
        &context,
        &action,
        DefenseFallbackKind::SuitedSafety(SuitedSafetyRank::Suji),
    );
    assert_eq!(
        diagnostic.selected_suji_safety_rank_for_all_reached,
        Some(SujiSafetyRank::Suji)
    );
    assert_eq!(
        diagnostic.selected_suited_safety_rank,
        Some(SuitedSafetyRank::Suji)
    );
}

#[test]
fn post_reach_genbutsu_is_excluded_from_reached_suji_aggregation() {
    let nine_man = tile_type("9m");
    let context = post_reach_context(
        Some(0),
        [vec![], vec![], vec![discarded("6m")], vec![]],
        [false, true, true, false],
        [vec![], vec![nine_man], vec![], vec![]],
    );

    assert!(!is_discarded_by_player(nine_man, 1, &context));
    assert!(context.is_post_reach_passed(nine_man, 1));
    assert!(is_genbutsu_for(nine_man, 1, &context));
    assert_eq!(
        suji_safety_rank_for(nine_man, 1, &context),
        Some(SujiSafetyRank::NoSuji)
    );
    assert_eq!(
        suji_safety_rank_for_all_reached(nine_man, &context),
        Some(SujiSafetyRank::Suji)
    );
    assert_eq!(
        suited_safety_rank_for_all_reached(nine_man, &context),
        Some(SuitedSafetyRank::Suji)
    );
}

#[test]
fn non_genbutsu_no_suji_reacher_still_makes_the_aggregate_unsafe() {
    let nine_man = tile_type("9m");
    let context = two_reachers_context(vec![discarded("9m")], vec![]);

    assert!(is_genbutsu_for(nine_man, 1, &context));
    assert!(!is_genbutsu_for(nine_man, 2, &context));
    assert_eq!(
        suji_safety_rank_for_all_reached(nine_man, &context),
        Some(SujiSafetyRank::NoSuji)
    );
    assert_eq!(
        suited_safety_rank_for_all_reached(nine_man, &context),
        Some(SuitedSafetyRank::NoSafety)
    );
}

#[test]
fn all_reachers_genbutsu_keeps_genbutsu_as_the_fallback_source() {
    let nine_man = tile_type("9m");
    let context = post_reach_context(
        Some(0),
        [vec![], vec![discarded("9m")], vec![], vec![]],
        [false, true, true, false],
        [vec![], vec![], vec![nine_man], vec![]],
    );
    let actions = vec![LegalAction::Dahai { tile: held("9m") }];

    assert!(is_genbutsu_for_all_reached(nine_man, &context));
    assert_eq!(
        suji_safety_rank_for_all_reached(nine_man, &context),
        Some(SujiSafetyRank::NoSuji)
    );
    assert_eq!(
        suited_safety_rank_for_all_reached(nine_man, &context),
        Some(SuitedSafetyRank::NoSafety)
    );
    assert_eq!(
        select_defense_fallback_action_with_kind(&context, &actions),
        Some((&actions[0], DefenseFallbackKind::Genbutsu))
    );
}

#[test]
fn suji_safety_rank_for_any_reached_takes_safest_rank() {
    // any 基準は最大値。片方が Suji なら全体も Suji。
    let four_pin = tile(48).tile_type();
    let context = two_reachers_context(vec![tile(36), tile(60)], vec![]);
    assert_eq!(
        suji_safety_rank_for_any_reached(four_pin, &context),
        Some(SujiSafetyRank::Suji)
    );

    // 片スジと無スジなら HalfSuji。
    let context = two_reachers_context(vec![tile(36)], vec![]);
    assert_eq!(
        suji_safety_rank_for_any_reached(four_pin, &context),
        Some(SujiSafetyRank::HalfSuji)
    );
}

#[test]
fn suji_safety_rank_no_suji_without_reachers() {
    // リーチ者がいなければ、河に根拠があっても安全牌として扱わない。
    let discards = [vec![], vec![tile(36), tile(60)], vec![], vec![]];
    let context = table_state_context(Some(0), None, discards, [false; 4]);
    assert_eq!(
        suji_safety_rank_for_all_reached(tile(48).tile_type(), &context),
        Some(SujiSafetyRank::NoSuji)
    );
    assert_eq!(
        suji_safety_rank_for_any_reached(tile(48).tile_type(), &context),
        Some(SujiSafetyRank::NoSuji)
    );
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
fn is_suji_for_all_reached_false_without_reachers() {
    // 河に 4m があっても、リーチ者がいなければ false。
    let discards = [vec![], vec![tile(12)], vec![], vec![]];
    let context = table_state_context(Some(0), None, discards, [false; 4]);
    assert!(!is_suji_for_all_reached(tile(0).tile_type(), &context));
}

#[test]
fn is_suji_for_all_reached_single_reacher_classifies_number_tiles() {
    // 単独リーチ者(1)の河に 4m。1m と 7m はスジ、5m は無スジ。
    let discards = [vec![], vec![tile(12)], vec![], vec![]];
    let context = table_state_context(Some(0), None, discards, [false, true, false, false]);
    assert!(is_suji_for_all_reached(tile(0).tile_type(), &context));
    assert!(is_suji_for_all_reached(tile(24).tile_type(), &context));
    assert!(!is_suji_for_all_reached(tile(16).tile_type(), &context));
}

#[test]
fn is_suji_for_all_reached_matches_any_for_single_reacher() {
    // 単独リーチでは any 判定と all 判定が一致する。
    let discards = [vec![], vec![tile(12)], vec![], vec![]];
    let context = table_state_context(Some(0), None, discards, [false, true, false, false]);
    for value in [0u8, 16, 24] {
        let tile_type = tile(value).tile_type();
        assert_eq!(
            is_suji_for_any_reached(tile_type, &context),
            is_suji_for_all_reached(tile_type, &context)
        );
    }
}

#[test]
fn is_suji_for_all_reached_true_when_all_reachers_have_suji() {
    // 二人のリーチ者の河にそれぞれ 4m。1m は全員にスジ。
    let discards = [vec![], vec![tile(12)], vec![tile(13)], vec![]];
    let context = table_state_context(Some(0), None, discards, [false, true, true, false]);
    assert!(is_suji_for_all_reached(tile(0).tile_type(), &context));
}

#[test]
fn is_suji_for_all_reached_false_when_only_one_reacher_has_suji() {
    // 一人目の河にだけ 4m。any は true でも all は false(主要な回帰テスト)。
    let discards = [vec![], vec![tile(12)], vec![], vec![]];
    let context = table_state_context(Some(0), None, discards, [false, true, true, false]);
    assert!(is_suji_for_any_reached(tile(0).tile_type(), &context));
    assert!(!is_suji_for_all_reached(tile(0).tile_type(), &context));
}

#[test]
fn is_suji_for_all_reached_ignores_own_reach() {
    // 自分(0)の河にだけ 4m。自分のリーチは対象外で、他家リーチ者(1)の河には根拠なし。
    let discards = [vec![tile(12)], vec![], vec![], vec![]];
    let context = table_state_context(Some(0), None, discards, [true, true, false, false]);
    assert!(!is_suji_for_all_reached(tile(0).tile_type(), &context));
}

#[test]
fn is_suji_for_all_reached_without_player_id_targets_all_reached() {
    // player_id なしはリーチフラグが立っている全席を対象にする。
    let discards = [vec![tile(12)], vec![], vec![], vec![]];
    let context = table_state_context(None, None, discards, [true, false, false, false]);
    assert!(is_suji_for_all_reached(tile(0).tile_type(), &context));
}

#[test]
fn is_suji_for_all_reached_false_for_honor() {
    // 字牌はスジ判定対象外なので false。
    let discards = [vec![], vec![tile(12)], vec![], vec![]];
    let context = table_state_context(Some(0), None, discards, [false, true, false, false]);
    assert!(!is_suji_for_all_reached(tile(108).tile_type(), &context));
}

#[test]
fn suji_safety_rank_for_all_reached_none_for_honor() {
    let context = table_state_context(
        Some(0),
        None,
        Default::default(),
        [false, true, false, false],
    );
    assert_eq!(
        suji_safety_rank_for_all_reached(tile(108).tile_type(), &context),
        None
    );
}

#[test]
fn suji_safety_rank_for_all_reached_no_suji_when_only_one_reacher_has_suji() {
    // 二人リーチで一人だけにスジ。all 基準では NoSuji / NoSafety。
    let discards = [vec![], vec![tile(12)], vec![], vec![]];
    let context = table_state_context(Some(0), None, discards, [false, true, true, false]);
    assert_eq!(
        suji_safety_rank_for_all_reached(tile(0).tile_type(), &context),
        Some(SujiSafetyRank::NoSuji)
    );
    assert_eq!(
        suited_safety_rank_for_all_reached(tile(0).tile_type(), &context),
        Some(SuitedSafetyRank::NoSafety)
    );
}

#[test]
fn suji_safety_rank_for_all_reached_suji_when_all_reachers_have_suji() {
    // 二人リーチで全員にスジ。all 基準でも Suji。
    let discards = [vec![], vec![tile(12)], vec![tile(13)], vec![]];
    let context = table_state_context(Some(0), None, discards, [false, true, true, false]);
    assert_eq!(
        suji_safety_rank_for_all_reached(tile(0).tile_type(), &context),
        Some(SujiSafetyRank::Suji)
    );
    assert_eq!(
        suited_safety_rank_for_all_reached(tile(0).tile_type(), &context),
        Some(SuitedSafetyRank::Suji)
    );
}

#[test]
fn suji_dahai_actions_by_safety_uses_all_reached_basis() {
    // 合法 Dahai を 1m, 2m の順で渡す。2m は全員スジ、1m は一人だけスジ。
    let context = all_reached_partial_suji_context(vec![]);
    let actions = vec![
        LegalAction::Dahai { tile: tile(0) },
        LegalAction::Dahai { tile: tile(4) },
    ];
    let ranked = suji_dahai_actions_by_safety(&actions, &context);
    assert_eq!(
        ranked,
        vec![
            (&LegalAction::Dahai { tile: tile(4) }, SujiSafetyRank::Suji),
            (
                &LegalAction::Dahai { tile: tile(0) },
                SujiSafetyRank::NoSuji
            ),
        ]
    );
}
