use super::common::*;
use crate::action::LegalAction;
use crate::defense::*;

#[test]
fn select_defense_fallback_action_with_kind_none_without_opponent_reach() {
    let context = suited_context(
        vec![tile(0), tile(1), tile(2), tile(3)],
        [vec![], vec![tile(16)], vec![], vec![]],
        [false; 4],
    );
    let actions = vec![
        LegalAction::Dahai { tile: tile(16) },
        LegalAction::Dahai { tile: tile(108) },
        LegalAction::Dahai { tile: tile(0) },
    ];
    assert_eq!(
        select_defense_fallback_action_with_kind(&context, &actions),
        None
    );
}

#[test]
fn select_defense_fallback_action_with_kind_returns_genbutsu() {
    let context = suited_context(
        vec![],
        [vec![], vec![tile(16)], vec![], vec![]],
        [false, true, false, false],
    );
    let actions = vec![
        LegalAction::Dahai { tile: tile(0) },
        LegalAction::Dahai { tile: tile(16) },
    ];
    assert_eq!(
        select_defense_fallback_action_with_kind(&context, &actions),
        Some((
            &LegalAction::Dahai { tile: tile(16) },
            DefenseFallbackKind::Genbutsu
        ))
    );
}

#[test]
fn select_defense_fallback_action_with_kind_prefers_genbutsu_over_honor() {
    // 共通現物 16(5m) と字牌 108(東) が両方候補でも Genbutsu を優先する。
    let context = suited_context(
        vec![],
        [vec![], vec![tile(16)], vec![], vec![]],
        [false, true, false, false],
    );
    let actions = vec![
        LegalAction::Dahai { tile: tile(108) },
        LegalAction::Dahai { tile: tile(16) },
    ];
    assert_eq!(
        select_defense_fallback_action_with_kind(&context, &actions),
        Some((
            &LegalAction::Dahai { tile: tile(16) },
            DefenseFallbackKind::Genbutsu
        ))
    );
}

#[test]
fn select_defense_fallback_action_with_kind_returns_honor_safety_with_rank() {
    // 共通現物なし。東は2枚見えなので HonorSafety(TwoVisible)。
    let context = suited_context(
        vec![tile(108), tile(109)],
        Default::default(),
        [false, true, false, false],
    );
    let actions = vec![
        LegalAction::Dahai { tile: tile(112) },
        LegalAction::Dahai { tile: tile(108) },
    ];
    assert_eq!(
        select_defense_fallback_action_with_kind(&context, &actions),
        Some((
            &LegalAction::Dahai { tile: tile(108) },
            DefenseFallbackKind::HonorSafety(HonorSafetyRank::TwoVisible)
        ))
    );
}

#[test]
fn select_defense_fallback_action_with_kind_returns_suited_safety_no_chance() {
    // 共通現物も字牌もなし。4m を4枚見えにして経路 [3m,4m] を Blocked にし 2m を NoChance。
    let context = suited_context(
        vec![tile(12), tile(13), tile(14), tile(15)],
        Default::default(),
        [false, true, false, false],
    );
    let actions = vec![
        LegalAction::Dahai { tile: tile(0) },
        LegalAction::Dahai { tile: tile(4) },
    ];
    assert_eq!(
        select_defense_fallback_action_with_kind(&context, &actions),
        Some((
            &LegalAction::Dahai { tile: tile(4) },
            DefenseFallbackKind::SuitedSafety(SuitedSafetyRank::NoChance)
        ))
    );
}

#[test]
fn select_defense_fallback_action_with_kind_returns_suited_safety_one_chance() {
    // 4m を3枚見えにして経路 [3m,4m] を OneChance にし 2m を OneChance。
    let context = suited_context(
        vec![tile(12), tile(13), tile(14)],
        Default::default(),
        [false, true, false, false],
    );
    let actions = vec![
        LegalAction::Dahai { tile: tile(0) },
        LegalAction::Dahai { tile: tile(4) },
    ];
    assert_eq!(
        select_defense_fallback_action_with_kind(&context, &actions),
        Some((
            &LegalAction::Dahai { tile: tile(4) },
            DefenseFallbackKind::SuitedSafety(SuitedSafetyRank::OneChance)
        ))
    );
}

#[test]
fn select_defense_fallback_action_with_kind_returns_suited_safety_suji() {
    // リーチ者の河に 12(4m)。1m はスジで Suji。
    let context = suited_context(
        vec![],
        [vec![], vec![tile(12)], vec![], vec![]],
        [false, true, false, false],
    );
    let actions = vec![
        LegalAction::Dahai { tile: tile(16) },
        LegalAction::Dahai { tile: tile(0) },
    ];
    assert_eq!(
        select_defense_fallback_action_with_kind(&context, &actions),
        Some((
            &LegalAction::Dahai { tile: tile(0) },
            DefenseFallbackKind::SuitedSafety(SuitedSafetyRank::Suji)
        ))
    );
}

#[test]
fn select_defense_fallback_action_with_kind_none_when_only_no_safety() {
    // 共通現物も字牌もなく、数牌が全て NoSafety なら None。
    let context = suited_context(vec![], Default::default(), [false, true, false, false]);
    let actions = vec![
        LegalAction::Dahai { tile: tile(0) },
        LegalAction::Dahai { tile: tile(16) },
    ];
    assert_eq!(
        select_defense_fallback_action_with_kind(&context, &actions),
        None
    );
}

#[test]
fn select_defense_fallback_action_returns_action_only() {
    let context = suited_context(
        vec![],
        [vec![], vec![tile(16)], vec![], vec![]],
        [false, true, false, false],
    );
    let actions = vec![
        LegalAction::Dahai { tile: tile(0) },
        LegalAction::Dahai { tile: tile(16) },
    ];
    assert_eq!(
        select_defense_fallback_action(&context, &actions),
        Some(&LegalAction::Dahai { tile: tile(16) })
    );
}

#[test]
fn select_defense_fallback_action_matches_with_kind_on_black_five() {
    // 薄い wrapper が with_kind と同じ黒5の action を返す。現物 5m系 [赤5m, 黒5m]。
    let discards = [vec![], vec![tile(17)], vec![], vec![]];
    let context = table_state_context(Some(0), None, discards, [false, true, false, false]);
    let actions = vec![
        LegalAction::Dahai { tile: tile(16) },
        LegalAction::Dahai { tile: tile(17) },
    ];
    let with_kind =
        select_defense_fallback_action_with_kind(&context, &actions).map(|(action, _)| action);
    assert_eq!(with_kind, Some(&LegalAction::Dahai { tile: tile(17) }));
    assert_eq!(
        select_defense_fallback_action(&context, &actions),
        with_kind
    );
}

#[test]
fn select_defense_fallback_action_with_kind_prefers_all_reached_suji() {
    // 共通現物なし・字牌 Dahai なし。一人だけスジと全員スジがあれば全員スジを選ぶ。
    let context = all_reached_partial_suji_context(vec![]);
    let actions = vec![
        LegalAction::Dahai { tile: tile(0) },
        LegalAction::Dahai { tile: tile(4) },
    ];
    assert_eq!(
        select_defense_fallback_action_with_kind(&context, &actions),
        Some((
            &LegalAction::Dahai { tile: tile(4) },
            DefenseFallbackKind::SuitedSafety(SuitedSafetyRank::Suji)
        ))
    );
}

// 実戦問題の最小回帰。合法 Dahai は 6p(56) と 1s(72)。6p は自身が3枚見えているが周辺牌に壁なし、
// リーチ者(1人)の河は 4s(84) のみ。6p は現物でなく無スジ、1s は 4s に対してスジ。
#[test]
fn real_world_regression_prefers_suji_1s_over_self_visible_6p() {
    let six_pin = tile(56).tile_type();
    let one_sou = tile(72).tile_type();
    let context = suited_context(
        vec![tile(56), tile(57), tile(58)],
        [vec![], vec![tile(84)], vec![], vec![]],
        [false, true, false, false],
    );

    // 6p 自身が3枚見えていても、経路 4p/5p/7p/8p に壁がないので NoWall。
    assert_eq!(wall_rank(six_pin, &context), WallRank::NoWall);
    assert_eq!(
        suited_safety_rank_for_all_reached(six_pin, &context),
        Some(SuitedSafetyRank::NoSafety)
    );
    assert_eq!(
        suited_safety_rank_for_all_reached(one_sou, &context),
        Some(SuitedSafetyRank::Suji)
    );

    let actions = vec![
        LegalAction::Dahai { tile: tile(56) },
        LegalAction::Dahai { tile: tile(72) },
    ];
    assert_eq!(
        select_defense_fallback_action_with_kind(&context, &actions),
        Some((
            &LegalAction::Dahai { tile: tile(72) },
            DefenseFallbackKind::SuitedSafety(SuitedSafetyRank::Suji)
        ))
    );
}

// 同じ候補でも、リーチ者の河に 6p があれば 6p を現物として選ぶ。現物優先が壊れていないこと。
#[test]
fn real_world_regression_keeps_genbutsu_6p_when_in_river() {
    let context = suited_context(
        vec![tile(56), tile(57), tile(58)],
        [vec![], vec![tile(59), tile(84)], vec![], vec![]],
        [false, true, false, false],
    );
    let actions = vec![
        LegalAction::Dahai { tile: tile(56) },
        LegalAction::Dahai { tile: tile(72) },
    ];
    assert_eq!(
        select_defense_fallback_action_with_kind(&context, &actions),
        Some((
            &LegalAction::Dahai { tile: tile(56) },
            DefenseFallbackKind::Genbutsu
        ))
    );
}

// ---- 防御 fallback の黒5優先(物理牌正規化)テスト ----

#[test]
fn defense_fallback_genbutsu_prefers_black_five() {
    // リーチ者の河に 5m があり 5m 系は現物。合法 Dahai が [赤5m, 黒5m] でも黒5m を選ぶ。
    let discards = [vec![], vec![tile(17)], vec![], vec![]];
    let context = table_state_context(Some(0), None, discards, [false, true, false, false]);
    let actions = vec![
        LegalAction::Dahai { tile: tile(16) },
        LegalAction::Dahai { tile: tile(17) },
    ];
    assert_eq!(
        select_defense_fallback_action_with_kind(&context, &actions),
        Some((
            &LegalAction::Dahai { tile: tile(17) },
            DefenseFallbackKind::Genbutsu
        ))
    );
}

#[test]
fn defense_fallback_genbutsu_prefers_black_five_when_red_first_reversed() {
    // 合法 action の順序を逆(黒5m→赤5m)にしても黒5m を選ぶ。
    let discards = [vec![], vec![tile(17)], vec![], vec![]];
    let context = table_state_context(Some(0), None, discards, [false, true, false, false]);
    let actions = vec![
        LegalAction::Dahai { tile: tile(17) },
        LegalAction::Dahai { tile: tile(16) },
    ];
    assert_eq!(
        select_defense_fallback_action_with_kind(&context, &actions).map(|(a, _)| a),
        Some(&LegalAction::Dahai { tile: tile(17) })
    );
}

#[test]
fn defense_fallback_genbutsu_keeps_red_five_when_only_red_legal() {
    // 赤5m しか合法でなければ赤5m を選ぶ。
    let discards = [vec![], vec![tile(17)], vec![], vec![]];
    let context = table_state_context(Some(0), None, discards, [false, true, false, false]);
    let actions = vec![LegalAction::Dahai { tile: tile(16) }];
    assert_eq!(
        select_defense_fallback_action_with_kind(&context, &actions),
        Some((
            &LegalAction::Dahai { tile: tile(16) },
            DefenseFallbackKind::Genbutsu
        ))
    );
}

#[test]
fn defense_fallback_suited_safety_prefers_black_five() {
    // 5m を NoChance にする。4m(12-15)を4枚見せて経路 [4m,5m]... ではなく 5m を対象に
    // 経路 [3m,4m] と [6m,7m] を塞ぐ必要がある。ここでは 4m と 6m を各4枚見せる。
    // すると 5m の経路 [3m,4m]=Blocked, [6m,7m]=Blocked で NoChance。
    let visible = vec![
        tile(12),
        tile(13),
        tile(14),
        tile(15),
        tile(20),
        tile(21),
        tile(22),
        tile(23),
    ];
    let context = suited_context(visible, Default::default(), [false, true, false, false]);
    // 5m の合法 Dahai が [赤5m, 黒5m]。NoChance の 5m を選び、物理牌は黒5m。
    let actions = vec![
        LegalAction::Dahai { tile: tile(16) },
        LegalAction::Dahai { tile: tile(17) },
    ];
    assert_eq!(
        select_defense_fallback_action_with_kind(&context, &actions),
        Some((
            &LegalAction::Dahai { tile: tile(17) },
            DefenseFallbackKind::SuitedSafety(SuitedSafetyRank::NoChance)
        ))
    );
}

#[test]
fn defense_fallback_suited_safety_keeps_red_when_only_red_legal() {
    // 同じ NoChance 5m だが合法 Dahai が赤5m だけなら赤5m を選ぶ。安全度は変わらない。
    let visible = vec![
        tile(12),
        tile(13),
        tile(14),
        tile(15),
        tile(20),
        tile(21),
        tile(22),
        tile(23),
    ];
    let context = suited_context(visible, Default::default(), [false, true, false, false]);
    let actions = vec![LegalAction::Dahai { tile: tile(16) }];
    assert_eq!(
        select_defense_fallback_action_with_kind(&context, &actions),
        Some((
            &LegalAction::Dahai { tile: tile(16) },
            DefenseFallbackKind::SuitedSafety(SuitedSafetyRank::NoChance)
        ))
    );
}

#[test]
fn defense_fallback_does_not_change_tile_type_for_black_five() {
    // 同一安全度で [赤5m, 1p, 5m] の順。先頭牌種は 5m。黒5優先で 1p へは変えず黒5m を選ぶ。
    // リーチ者はいるが河・visible が空なので 5m も 1p も NoSafety で同一安全度。
    // NoSafety は防御 fallback の対象外なので、この局面では防御 fallback は None になる。
    // 牌種順維持の確認は現物経路で行う(下記テスト)。
    let discards = [vec![], vec![tile(17), tile(36)], vec![], vec![]];
    let context = table_state_context(Some(0), None, discards, [false, true, false, false]);
    // 5m 系と 1p が現物。合法順 [赤5m, 1p, 黒5m] で先頭現物牌種は 5m。黒5m を選ぶ。
    let actions = vec![
        LegalAction::Dahai { tile: tile(16) },
        LegalAction::Dahai { tile: tile(36) },
        LegalAction::Dahai { tile: tile(17) },
    ];
    assert_eq!(
        select_defense_fallback_action_with_kind(&context, &actions),
        Some((
            &LegalAction::Dahai { tile: tile(17) },
            DefenseFallbackKind::Genbutsu
        ))
    );
}

#[test]
fn defense_fallback_keeps_leading_tile_type_over_black_five() {
    // 合法順 [1p, 赤5m, 黒5m] で 1p と 5m 系がともに現物。先頭現物牌種 1p を維持する。
    // 黒5優先のために 5m を 1p より前へ移動しない。
    let discards = [vec![], vec![tile(17), tile(36)], vec![], vec![]];
    let context = table_state_context(Some(0), None, discards, [false, true, false, false]);
    let actions = vec![
        LegalAction::Dahai { tile: tile(36) },
        LegalAction::Dahai { tile: tile(16) },
        LegalAction::Dahai { tile: tile(17) },
    ];
    assert_eq!(
        select_defense_fallback_action_with_kind(&context, &actions),
        Some((
            &LegalAction::Dahai { tile: tile(36) },
            DefenseFallbackKind::Genbutsu
        ))
    );
}
