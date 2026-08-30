use super::common::*;
use super::hidden_hand_states::{ankan, multiple_reached_fixture, pon, reached_fixture};
use crate::action::LegalAction;
use crate::defense::*;
use std::cmp::Ordering;

fn four_ankans() -> Vec<crate::meld::Meld> {
    ["5p", "6p", "7p", "8p"].map(ankan).to_vec()
}

fn multiple_reacher_ankans(include_player_three: bool) -> [Vec<crate::meld::Meld>; 4] {
    let mut melds: [Vec<_>; 4] = Default::default();
    melds[1] = ["5p", "6p", "7p", "8p"].map(ankan).to_vec();
    melds[2] = ["1s", "2s", "3s", "4s"].map(ankan).to_vec();
    if include_player_three {
        melds[3] = ["6s", "7s", "8s", "9s"].map(ankan).to_vec();
    }
    melds
}

fn risk_for(vectors: &[DahaiRonRiskVector<'_>], tile: &str, player: usize) -> RonRiskEvidence {
    vectors
        .iter()
        .find(|candidate| {
            matches!(candidate.action, LegalAction::Dahai { tile: candidate } if candidate.tile_type() == tile_type(tile))
        })
        .and_then(|candidate| {
            candidate
                .player_evidence
                .iter()
                .find(|evidence| evidence.player == player)
        })
        .map(|evidence| evidence.evidence)
        .expect("candidate/player evidence")
}

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
    let context = legacy_suited_context(
        vec![tile(108), tile(109)],
        Default::default(),
        [false, true, true, false],
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
    let context = legacy_suited_context(
        vec![tile(12), tile(13), tile(14), tile(15)],
        Default::default(),
        [false, true, true, false],
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
    let context = legacy_suited_context(
        vec![tile(12), tile(13), tile(14)],
        Default::default(),
        [false, true, true, false],
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
    let context = legacy_suited_context(
        vec![],
        [vec![], vec![tile(12)], vec![tile(13)], vec![]],
        [false, true, true, false],
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
    let context = legacy_suited_context(vec![], Default::default(), [false, true, true, false]);
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
    let context = legacy_suited_context(
        vec![],
        [vec![], vec![tile(12), tile(16)], vec![tile(17)], vec![]],
        [false, true, true, false],
    );
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
    let evidence = single_reach_dahai_actions_by_ron_risk(1, &context, &actions).unwrap();
    assert!(evidence[1].evidence.ron_capable_weight < evidence[0].evidence.ron_capable_weight);
    assert_eq!(
        select_defense_fallback_action_with_kind(&context, &actions),
        Some((
            &LegalAction::Dahai { tile: tile(72) },
            DefenseFallbackKind::ExactRonRisk
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
    let context = legacy_suited_context(visible, Default::default(), [false, true, true, false]);
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
    let context = legacy_suited_context(visible, Default::default(), [false, true, true, false]);
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

#[test]
fn exact_ratio_vectors_compare_worst_first_without_raw_weight_aggregation() {
    let a = [
        PlayerRonRiskEvidence {
            player: 1,
            evidence: RonRiskEvidence {
                ron_capable_weight: 1,
                tenpai_weight: 2,
            },
        },
        PlayerRonRiskEvidence {
            player: 2,
            evidence: RonRiskEvidence {
                ron_capable_weight: 1,
                tenpai_weight: 20,
            },
        },
    ];
    let b = [
        PlayerRonRiskEvidence {
            player: 1,
            evidence: RonRiskEvidence {
                ron_capable_weight: 4,
                tenpai_weight: 10,
            },
        },
        PlayerRonRiskEvidence {
            player: 2,
            evidence: RonRiskEvidence {
                ron_capable_weight: 3,
                tenpai_weight: 10,
            },
        },
    ];

    // A=[1/2, 1/20], B=[4/10, 3/10]。raw R の和ではなく worst ratio で B が安全。
    assert_eq!(
        compare_lexicographic_minimax_ron_risk(&a, &b),
        Some(Ordering::Greater)
    );

    let same_worst_better_second = [
        a[0],
        PlayerRonRiskEvidence {
            player: 2,
            evidence: RonRiskEvidence {
                ron_capable_weight: 1,
                tenpai_weight: 10,
            },
        },
    ];
    let same_worst_worse_second = [
        PlayerRonRiskEvidence {
            player: 1,
            evidence: RonRiskEvidence {
                ron_capable_weight: 2,
                tenpai_weight: 4,
            },
        },
        PlayerRonRiskEvidence {
            player: 2,
            evidence: RonRiskEvidence {
                ron_capable_weight: 2,
                tenpai_weight: 10,
            },
        },
    ];
    assert_eq!(
        compare_lexicographic_minimax_ron_risk(&same_worst_better_second, &same_worst_worse_second,),
        Some(Ordering::Less)
    );

    let unavailable = [PlayerRonRiskEvidence {
        player: 1,
        evidence: RonRiskEvidence {
            ron_capable_weight: 0,
            tenpai_weight: 0,
        },
    }];
    assert_eq!(
        compare_lexicographic_minimax_ron_risk(&unavailable, &unavailable),
        None
    );
}

#[test]
fn two_reaches_exact_minimax_selects_balanced_middle_risk() {
    let context = multiple_reached_fixture(
        &[("1m", 3), ("2m", 2), ("3m", 3)],
        multiple_reacher_ankans(false),
        &[(1, &["3m"]), (2, &["1m"])],
        [false, true, true, false],
    );
    let actions = vec![
        LegalAction::Dahai {
            tile: discarded("1m"),
        },
        LegalAction::Dahai {
            tile: discarded("2m"),
        },
        LegalAction::Dahai {
            tile: discarded("3m"),
        },
    ];
    let vectors = reached_opponents_dahai_actions_by_ron_risk(&context, &actions).unwrap();

    assert_eq!(
        risk_for(&vectors, "1m", 1),
        RonRiskEvidence {
            ron_capable_weight: 3,
            tenpai_weight: 8
        }
    );
    assert_eq!(
        risk_for(&vectors, "1m", 2),
        RonRiskEvidence {
            ron_capable_weight: 0,
            tenpai_weight: 8
        }
    );
    assert_eq!(
        risk_for(&vectors, "2m", 1),
        RonRiskEvidence {
            ron_capable_weight: 2,
            tenpai_weight: 8
        }
    );
    assert_eq!(
        risk_for(&vectors, "2m", 2),
        RonRiskEvidence {
            ron_capable_weight: 2,
            tenpai_weight: 8
        }
    );
    assert_eq!(
        risk_for(&vectors, "3m", 1),
        RonRiskEvidence {
            ron_capable_weight: 0,
            tenpai_weight: 8
        }
    );
    assert_eq!(
        risk_for(&vectors, "3m", 2),
        RonRiskEvidence {
            ron_capable_weight: 3,
            tenpai_weight: 8
        }
    );
    assert_eq!(
        select_defense_fallback_action_with_kind(&context, &actions),
        Some((&actions[1], DefenseFallbackKind::ExactRonRisk))
    );
}

#[test]
fn two_reaches_exact_minimax_uses_second_worst_tie_break() {
    let context = multiple_reached_fixture(
        &[("1m", 3), ("2m", 3)],
        multiple_reacher_ankans(false),
        &[(2, &["1m"])],
        [false, true, true, false],
    );
    let actions = vec![
        LegalAction::Dahai {
            tile: discarded("2m"),
        },
        LegalAction::Dahai {
            tile: discarded("1m"),
        },
    ];
    let vectors = reached_opponents_dahai_actions_by_ron_risk(&context, &actions).unwrap();
    let two_man = &vectors[0].player_evidence;
    let one_man = &vectors[1].player_evidence;

    // 2m=[3/6,3/6], 1m=[3/6,0]。worst は同率で second-worst が小さい 1m を選ぶ。
    assert_eq!(
        compare_lexicographic_minimax_ron_risk(one_man, two_man),
        Some(Ordering::Less)
    );
    assert_eq!(
        select_defense_fallback_action_with_kind(&context, &actions),
        Some((&actions[1], DefenseFallbackKind::ExactRonRisk))
    );
}

#[test]
fn three_reaches_exact_minimax_uses_third_worst_tie_break() {
    let context = multiple_reached_fixture(
        &[("1m", 3), ("2m", 3)],
        multiple_reacher_ankans(true),
        &[(3, &["2m"])],
        [false, true, true, true],
    );
    let actions = vec![
        LegalAction::Dahai {
            tile: discarded("1m"),
        },
        LegalAction::Dahai {
            tile: discarded("2m"),
        },
    ];
    let vectors = reached_opponents_dahai_actions_by_ron_risk(&context, &actions).unwrap();

    // 1m=[3/6,3/6,3/6], 2m=[3/6,3/6,0]。worst / second が同率で third が決める。
    assert_eq!(
        compare_lexicographic_minimax_ron_risk(
            &vectors[1].player_evidence,
            &vectors[0].player_evidence,
        ),
        Some(Ordering::Less)
    );
    assert_eq!(
        select_defense_fallback_action_with_kind(&context, &actions),
        Some((&actions[1], DefenseFallbackKind::ExactRonRisk))
    );
}

#[test]
fn multiple_reach_exact_minimax_respects_pareto_dominance() {
    let context = multiple_reached_fixture(
        &[("1m", 1), ("2m", 2)],
        multiple_reacher_ankans(false),
        &[],
        [false, true, true, false],
    );
    let actions = vec![
        LegalAction::Dahai {
            tile: discarded("2m"),
        },
        LegalAction::Dahai {
            tile: discarded("1m"),
        },
    ];
    let vectors = reached_opponents_dahai_actions_by_ron_risk(&context, &actions).unwrap();
    assert!(
        vectors[1]
            .player_evidence
            .iter()
            .zip(&vectors[0].player_evidence)
            .all(
                |(one_man, two_man)| one_man.evidence.compare_ratio(&two_man.evidence)
                    == Some(Ordering::Less)
            )
    );
    assert_eq!(
        select_defense_fallback_action_with_kind(&context, &actions),
        Some((&actions[1], DefenseFallbackKind::ExactRonRisk))
    );
}

#[test]
fn player_specific_genbutsu_is_zero_risk_but_not_common_genbutsu() {
    let context = multiple_reached_fixture(
        &[("1m", 3), ("2m", 2)],
        multiple_reacher_ankans(false),
        &[(1, &["1m"])],
        [false, true, true, false],
    );
    let actions = vec![
        LegalAction::Dahai {
            tile: discarded("1m"),
        },
        LegalAction::Dahai {
            tile: discarded("2m"),
        },
    ];
    let vectors = reached_opponents_dahai_actions_by_ron_risk(&context, &actions).unwrap();

    assert!(is_genbutsu_for(tile_type("1m"), 1, &context));
    assert!(!is_genbutsu_for(tile_type("1m"), 2, &context));
    assert!(!is_genbutsu_for_all_reached(tile_type("1m"), &context));
    assert_eq!(risk_for(&vectors, "1m", 1).ron_capable_weight, 0);
    assert_eq!(risk_for(&vectors, "1m", 2).ron_capable_weight, 3);
    assert_eq!(
        select_defense_fallback_action_with_kind(&context, &actions)
            .unwrap()
            .1,
        DefenseFallbackKind::ExactRonRisk
    );
}

#[test]
fn multiple_reach_common_genbutsu_stays_highest_priority() {
    let context = multiple_reached_fixture(
        &[("1m", 3), ("2m", 1)],
        multiple_reacher_ankans(false),
        &[(1, &["1m"]), (2, &["1m"])],
        [false, true, true, false],
    );
    let actions = vec![
        LegalAction::Dahai {
            tile: discarded("2m"),
        },
        LegalAction::Dahai {
            tile: discarded("1m"),
        },
    ];
    assert_eq!(
        select_defense_fallback_action_with_kind(&context, &actions),
        Some((&actions[1], DefenseFallbackKind::Genbutsu))
    );
}

#[test]
fn one_unavailable_reacher_falls_back_the_whole_position_to_legacy() {
    let mut melds = multiple_reacher_ankans(false);
    melds[2] = vec![pon("9p")];
    let context = multiple_reached_fixture(
        &[("1m", 1), ("E", 3)],
        melds,
        &[],
        [false, true, true, false],
    );
    let actions = vec![
        LegalAction::Dahai {
            tile: discarded("1m"),
        },
        LegalAction::Dahai {
            tile: discarded("E"),
        },
    ];

    assert_eq!(
        reached_player_dahai_actions_by_ron_risk(1, &context, &actions)
            .unwrap()
            .len(),
        2
    );
    assert_eq!(
        reached_player_dahai_actions_by_ron_risk(2, &context, &actions),
        None
    );
    assert_eq!(
        reached_opponents_dahai_actions_by_ron_risk(&context, &actions),
        None
    );
    assert_eq!(
        select_defense_fallback_action_with_kind(&context, &actions),
        Some((
            &actions[1],
            DefenseFallbackKind::HonorSafety(HonorSafetyRank::OneVisible)
        ))
    );
}

#[test]
fn multiple_reach_exact_vector_tie_keeps_original_action_order() {
    let context = multiple_reached_fixture(
        &[("1m", 1), ("2m", 1)],
        multiple_reacher_ankans(false),
        &[],
        [false, true, true, false],
    );
    let actions = vec![
        LegalAction::Dahai {
            tile: discarded("2m"),
        },
        LegalAction::Dahai {
            tile: discarded("1m"),
        },
    ];
    assert_eq!(
        select_defense_fallback_action_with_kind(&context, &actions),
        Some((&actions[0], DefenseFallbackKind::ExactRonRisk))
    );
}

#[test]
fn multiple_reach_exact_prefers_black_five_within_selected_tile_type() {
    let context = multiple_reached_fixture(
        &[("5m", 1)],
        multiple_reacher_ankans(false),
        &[],
        [false, true, true, false],
    );
    let actions = vec![
        LegalAction::Dahai { tile: tile(16) },
        LegalAction::Dahai { tile: tile(17) },
    ];
    assert_eq!(
        select_defense_fallback_action_with_kind(&context, &actions),
        Some((&actions[1], DefenseFallbackKind::ExactRonRisk))
    );
}

#[test]
fn single_reach_exact_prefers_smaller_weight_with_same_legacy_rank() {
    // 4暗槓なら concealed hand は1枚で、target と同種の1枚だけが Tanki state になる。
    // 1m/2m はどちらも周辺牌が見え切った NoChance だが、remaining が1対2なので exact R は異なる。
    let context = reached_fixture(&[("1m", 1), ("2m", 2)], four_ankans(), &[], &[]);
    let actions = vec![
        LegalAction::Dahai {
            tile: discarded("2m"),
        },
        LegalAction::Dahai {
            tile: discarded("1m"),
        },
    ];
    assert_eq!(
        suited_safety_rank_for_all_reached(tile_type("1m"), &context),
        Some(SuitedSafetyRank::NoChance)
    );
    assert_eq!(
        suited_safety_rank_for_all_reached(tile_type("2m"), &context),
        Some(SuitedSafetyRank::NoChance)
    );

    let evidence = single_reach_dahai_actions_by_ron_risk(1, &context, &actions).unwrap();
    assert_eq!(evidence[0].evidence.ron_capable_weight, 2);
    assert_eq!(evidence[1].evidence.ron_capable_weight, 1);
    assert_eq!(evidence[0].evidence.tenpai_weight, 3);
    assert_eq!(evidence[1].evidence.tenpai_weight, 3);
    assert_eq!(
        select_defense_fallback_action_with_kind(&context, &actions),
        Some((&actions[1], DefenseFallbackKind::ExactRonRisk))
    );
}

#[test]
fn single_reach_exact_wait_structure_changes_production_selection() {
    // concealed 4枚。1m は PP/EE + 23m の Ryanmen states (weight 3 + 1) から、東は
    // PPP + E / 789s + E の Tanki と PP + EE の Shanpon states (weight 2 + 2 + 3) から
    // ロン可能になる。同じ fixed coefficient ではなく、hidden-hand state の構造差が R に出る。
    let context = reached_fixture(
        &[
            ("P", 3),
            ("2m", 1),
            ("3m", 1),
            ("7s", 1),
            ("8s", 1),
            ("9s", 1),
            ("E", 2),
        ],
        ["5p", "6p", "7p"].map(ankan).to_vec(),
        &[],
        &[],
    );
    let actions = vec![
        LegalAction::Dahai {
            tile: discarded("E"),
        },
        LegalAction::Dahai {
            tile: discarded("1m"),
        },
    ];
    let evidence = single_reach_dahai_actions_by_ron_risk(1, &context, &actions).unwrap();
    assert_eq!(evidence[0].evidence.ron_capable_weight, 7);
    assert_eq!(evidence[1].evidence.ron_capable_weight, 4);
    assert_eq!(
        select_defense_fallback_action_with_kind(&context, &actions),
        Some((&actions[1], DefenseFallbackKind::ExactRonRisk))
    );
}

#[test]
fn single_reach_exact_compares_honor_and_suited_in_one_model() {
    // legacy は字牌候補を優先する局面だが、exact model では R(1m)=1 < R(E)=3。
    let context = reached_fixture(&[("1m", 1), ("E", 3)], four_ankans(), &[], &[]);
    let actions = vec![
        LegalAction::Dahai {
            tile: discarded("E"),
        },
        LegalAction::Dahai {
            tile: discarded("1m"),
        },
    ];
    assert_eq!(
        honor_safety_rank(tile_type("E"), &context),
        Some(HonorSafetyRank::OneVisible)
    );
    assert!(!suited_safety_outweighs_honor(
        HonorSafetyRank::OneVisible,
        opponent_honor_value_for_reached(tile_type("E"), &context),
        SuitedSafetyRank::NoChance,
    ));

    let evidence = single_reach_dahai_actions_by_ron_risk(1, &context, &actions).unwrap();
    assert_eq!(evidence[0].evidence.ron_capable_weight, 3);
    assert_eq!(evidence[1].evidence.ron_capable_weight, 1);
    assert_eq!(
        select_defense_fallback_action_with_kind(&context, &actions),
        Some((&actions[1], DefenseFallbackKind::ExactRonRisk))
    );
}

#[test]
fn single_reach_exact_tie_keeps_original_action_order() {
    let context = reached_fixture(&[("1m", 1), ("2m", 1)], four_ankans(), &[], &[]);
    let actions = vec![
        LegalAction::Dahai {
            tile: discarded("2m"),
        },
        LegalAction::Dahai {
            tile: discarded("1m"),
        },
    ];
    assert_eq!(
        select_defense_fallback_action_with_kind(&context, &actions),
        Some((&actions[0], DefenseFallbackKind::ExactRonRisk))
    );
}

#[test]
fn single_reach_exact_prefers_black_five_within_selected_tile_type() {
    let context = reached_fixture(&[("5m", 1)], four_ankans(), &[], &[]);
    let actions = vec![
        LegalAction::Dahai { tile: tile(16) },
        LegalAction::Dahai { tile: tile(17) },
    ];
    assert_eq!(
        select_defense_fallback_action_with_kind(&context, &actions),
        Some((&actions[1], DefenseFallbackKind::ExactRonRisk))
    );
}

#[test]
fn single_reach_zero_denominator_falls_back_to_legacy() {
    let context = reached_fixture(&[], four_ankans(), &[], &[]);
    let actions = vec![
        LegalAction::Dahai {
            tile: discarded("1m"),
        },
        LegalAction::Dahai {
            tile: discarded("E"),
        },
    ];
    assert_eq!(
        single_reach_dahai_actions_by_ron_risk(1, &context, &actions),
        None
    );
    assert_eq!(
        select_defense_fallback_action_with_kind(&context, &actions),
        Some((
            &actions[1],
            DefenseFallbackKind::HonorSafety(HonorSafetyRank::ThreeOrMoreVisible)
        ))
    );
}

#[test]
fn single_reach_unsupported_open_meld_falls_back_to_legacy() {
    let pon_tiles: Vec<_> = bot_logic::TileId::copies(tile_type("9p")).take(3).collect();
    let pon = crate::meld::Meld::new(
        crate::meld::MeldKind::Pon,
        pon_tiles.clone(),
        Some(pon_tiles[0]),
    );
    let context = reached_fixture(&[("1m", 1), ("E", 3)], vec![pon], &[], &[]);
    let actions = vec![
        LegalAction::Dahai {
            tile: discarded("1m"),
        },
        LegalAction::Dahai {
            tile: discarded("E"),
        },
    ];

    assert_eq!(
        single_reach_dahai_actions_by_ron_risk(1, &context, &actions),
        None
    );
    assert_eq!(
        select_defense_fallback_action_with_kind(&context, &actions),
        Some((
            &actions[1],
            DefenseFallbackKind::HonorSafety(HonorSafetyRank::OneVisible)
        ))
    );
}
