use super::*;
use crate::agents::NodocchiAgent;
use crate::defense::reached_opponents_dahai_actions_by_ron_risk;
use crate::discard_selection::legal_discard_evaluations;
use crate::meld::{Meld, MeldKind};
use crate::nodocchi_test_support::{
    SINGLE_REACH_IISHANTEN_DRAWN as DRAWN, SINGLE_REACH_IISHANTEN_HAND as HAND, TENPAI_DRAWN,
    TENPAI_HAND, dahai_actions_for as actions_for, single_reach_iishanten_context,
    single_reach_iishanten_fixture as fixture, single_reach_iishanten_push_context, tenpai_actions,
    tile, unavailable_reach_meld,
};
use crate::push_pull::{
    PushPullMode, PushPullReason, decide_push_pull, push_pull_inputs_from_context,
};
use bot_logic::TileId;

fn tile_type(mjai: &str) -> TileType {
    TileType::from_mjai_type_str(mjai).unwrap()
}

fn single_reach_context(oya: Option<u8>) -> GameContext {
    single_reach_iishanten_context(oya)
}

fn pon(values: [u8; 3]) -> Meld {
    let tiles: Vec<TileId> = values.iter().map(|&value| tile(value)).collect();
    Meld::new(MeldKind::Pon, tiles.clone(), Some(tiles[0]))
}

fn production_ron_risk(
    context: &GameContext,
    actions: &[LegalAction],
) -> Option<IishantenReachRonRisk> {
    NodocchiAgent
        .decide(context, actions)
        .push_pull_inputs
        .and_then(|inputs| inputs.iishanten_reach_ron_risk)
}

// Reach Defense の exact evidence から、1向聴候補の牌種ごとの evidence を取り出す。
fn reach_defense_iishanten_evidence(
    context: &GameContext,
    actions: &[LegalAction],
) -> Vec<(TileType, RonRiskEvidence)> {
    let iishanten = legal_discard_evaluations(context, actions).iishanten_discards();
    let vectors = reached_opponents_dahai_actions_by_ron_risk(context, actions)
        .expect("単独リーチの exact model が利用できる");
    let mut seen = TileTypeSet::new();
    let mut evidence = Vec::new();
    for vector in vectors {
        let LegalAction::Dahai { tile } = vector.action else {
            continue;
        };
        if iishanten.contains(tile.tile_type()) && !seen.contains(tile.tile_type()) {
            seen.insert(tile.tile_type());
            assert_eq!(vector.player_evidence.len(), 1);
            evidence.push((tile.tile_type(), vector.player_evidence[0].evidence));
        }
    }
    evidence
}

fn evidence(ron_capable_weight: u128, tenpai_weight: u128) -> RonRiskEvidence {
    RonRiskEvidence {
        ron_capable_weight,
        tenpai_weight,
    }
}

#[test]
fn single_reach_iishanten_gets_an_exact_summary() {
    let context = single_reach_context(None);
    let actions = actions_for(&HAND, DRAWN);

    let decision = NodocchiAgent.decide(&context, &actions);
    let inputs = decision.push_pull_inputs.expect("通常打牌選択を通る");
    assert!(inputs.is_single_reach_threat());
    assert_eq!(inputs.offense.unwrap().min_shanten_after_discard, 1);

    let ron_risk = inputs
        .iishanten_reach_ron_risk
        .expect("単独リーチ × 1向聴は対象");
    assert_eq!(ron_risk.reacher, 1);
    let Some(LegalAction::Dahai { tile: selected }) = decision.normal_discard else {
        panic!("通常打牌を選んでいる");
    };
    assert_eq!(ron_risk.selected_discard, selected.tile_type());

    let exact = ron_risk.exact.expect("exact model が利用できる");
    assert!(ron_risk.is_exact_available());
    assert!(exact.selected.tenpai_weight > 0);
    assert!(exact.selected.ron_capable_weight <= exact.selected.tenpai_weight);
    // 赤5m と黒5m は1候補にまとめ、1m / 4m / 5m / 5s / W の5牌種になる。
    assert_eq!(exact.candidate_count, 5);
}

#[test]
fn every_candidate_matches_the_reach_defense_exact_evidence() {
    let context = single_reach_context(None);
    let actions = actions_for(&HAND, DRAWN);
    let expected = reach_defense_iishanten_evidence(&context, &actions);
    assert_eq!(
        expected
            .iter()
            .map(|(tile_type, _)| *tile_type)
            .collect::<Vec<_>>(),
        vec![
            tile_type("1m"),
            tile_type("4m"),
            tile_type("5m"),
            tile_type("5s"),
            tile_type("W")
        ]
    );
    // 同じリーチ者の T は全候補で共通。
    assert!(
        expected
            .iter()
            .all(|(_, evidence)| evidence.tenpai_weight == expected[0].1.tenpai_weight)
    );

    let ron_risk = production_ron_risk(&context, &actions).unwrap();
    let exact = ron_risk.exact.unwrap();
    let selected = expected
        .iter()
        .find(|(tile_type, _)| *tile_type == ron_risk.selected_discard)
        .unwrap()
        .1;
    assert_eq!(exact.selected, selected);
    assert_eq!(exact.candidate_count, expected.len());

    // 候補ごとの evidence は production の collector を1向聴候補だけへ絞っても変わらない。
    let iishanten = legal_discard_evaluations(&context, &actions).iishanten_discards();
    let candidates: Vec<_> = actions
        .iter()
        .filter(|action| matches!(action, LegalAction::Dahai { tile } if iishanten.contains(tile.tile_type())))
        .cloned()
        .collect();
    let restricted = dahai_ron_risk_evidence_for_player(1, &context, &candidates).unwrap();
    for (action, evidence) in candidates.iter().zip(restricted) {
        let LegalAction::Dahai { tile } = action else {
            unreachable!();
        };
        let reach_defense = expected
            .iter()
            .find(|(tile_type, _)| *tile_type == tile.tile_type())
            .unwrap()
            .1;
        assert_eq!(
            evidence.evidence,
            reach_defense,
            "{}",
            tile.to_mjai_string()
        );
    }
}

#[test]
fn rank_and_minimum_follow_the_exact_ratio_comparison() {
    let context = single_reach_context(None);
    let actions = actions_for(&HAND, DRAWN);
    let expected = reach_defense_iishanten_evidence(&context, &actions);
    let exact = production_ron_risk(&context, &actions)
        .unwrap()
        .exact
        .unwrap();

    let strictly_safer = expected
        .iter()
        .filter(|(_, evidence)| evidence.compare_ratio(&exact.selected) == Some(Ordering::Less))
        .count();
    assert_eq!(exact.selected_rank, strictly_safer + 1);
    assert!(
        expected.iter().all(|(_, evidence)| {
            evidence.compare_ratio(&exact.minimum) != Some(Ordering::Less)
        })
    );
    assert!(
        expected
            .iter()
            .any(|(_, evidence)| evidence.compare_ratio(&exact.minimum) == Some(Ordering::Equal))
    );
    assert_eq!(
        exact.selected_is_minimum,
        exact.selected.compare_ratio(&exact.minimum) == Some(Ordering::Equal)
    );
    assert_eq!(exact.selected_is_minimum, exact.selected_rank == 1);
}

#[test]
fn ties_share_a_rank_without_the_stable_order_creating_a_difference() {
    let (a, b, c, d) = (
        tile_type("1m"),
        tile_type("2m"),
        tile_type("3m"),
        tile_type("4m"),
    );
    // 2/20 と 1/10 は R が違っても同率。
    let candidates = [
        (a, evidence(3, 10)),
        (b, evidence(2, 20)),
        (c, evidence(1, 10)),
        (d, evidence(5, 10)),
    ];

    let worst = summarize_iishanten_ron_risk(d, &candidates).unwrap();
    assert_eq!(worst.selected_rank, 4);
    assert!(!worst.selected_is_minimum);
    assert_eq!(
        worst.minimum.compare_ratio(&evidence(1, 10)),
        Some(Ordering::Equal)
    );
    assert_eq!(worst.candidate_count, 4);

    let middle = summarize_iishanten_ron_risk(a, &candidates).unwrap();
    assert_eq!(middle.selected_rank, 3);
    assert!(!middle.selected_is_minimum);

    for tied in [b, c] {
        let summary = summarize_iishanten_ron_risk(tied, &candidates).unwrap();
        assert_eq!(summary.selected_rank, 1, "{tied:?}");
        assert!(summary.selected_is_minimum, "{tied:?}");
    }

    let mut reversed = candidates;
    reversed.reverse();
    assert_eq!(
        summarize_iishanten_ron_risk(b, &reversed)
            .unwrap()
            .selected_rank,
        1
    );
    assert_eq!(
        summarize_iishanten_ron_risk(a, &reversed)
            .unwrap()
            .selected_rank,
        3
    );
}

#[test]
fn dealer_reach_uses_the_same_exact_model() {
    let actions = actions_for(&HAND, DRAWN);
    let child = single_reach_context(None);
    let dealer = single_reach_context(Some(1));

    let decision = NodocchiAgent.decide(&dealer, &actions);
    let inputs = decision.push_pull_inputs.unwrap();
    assert!(inputs.dealer_reacher);

    let dealer_risk = inputs.iishanten_reach_ron_risk.unwrap();
    let child_risk = production_ron_risk(&child, &actions).unwrap();
    assert_eq!(dealer_risk.reacher, 1);
    assert_eq!(dealer_risk.exact, child_risk.exact);

    let expected = reach_defense_iishanten_evidence(&dealer, &actions);
    let selected = expected
        .iter()
        .find(|(tile_type, _)| *tile_type == dealer_risk.selected_discard)
        .unwrap()
        .1;
    assert_eq!(dealer_risk.exact.unwrap().selected, selected);
}

#[test]
fn multiple_reaches_are_not_evaluated() {
    let context = fixture(
        &HAND,
        DRAWN,
        None,
        [false, true, true, false],
        Default::default(),
    );
    let actions = actions_for(&HAND, DRAWN);

    let decision = NodocchiAgent.decide(&context, &actions);
    let inputs = decision.push_pull_inputs.unwrap();
    assert_eq!(inputs.opponent_reach_count, 2);
    assert_eq!(inputs.offense.unwrap().min_shanten_after_discard, 1);
    assert_eq!(inputs.iishanten_reach_ron_risk, None);
    assert_eq!(
        decision.push_pull.unwrap().reason,
        PushPullReason::IishantenAgainstReach
    );
}

#[test]
fn combined_threats_are_not_evaluated() {
    let mut melds: [Vec<Meld>; 4] = Default::default();
    melds[2] = vec![pon([120, 121, 122]), pon([96, 97, 98]), pon([56, 57, 58])];
    let context = fixture(&HAND, DRAWN, None, [false, true, false, false], melds);
    let actions = actions_for(&HAND, DRAWN);

    let decision = NodocchiAgent.decide(&context, &actions);
    let inputs = decision.push_pull_inputs.unwrap();
    assert!(inputs.has_combined_threat());
    assert!(!inputs.is_single_reach_threat());
    assert_eq!(inputs.offense.unwrap().min_shanten_after_discard, 1);
    assert_eq!(inputs.iishanten_reach_ron_risk, None);
    assert_eq!(
        decision.push_pull.unwrap().reason,
        PushPullReason::IishantenAgainstCombinedThreat
    );
}

#[test]
fn tenpai_and_two_or_more_shanten_are_not_evaluated() {
    let tenpai = fixture(
        &TENPAI_HAND,
        TENPAI_DRAWN,
        None,
        [false, true, false, false],
        Default::default(),
    );
    let tenpai_decision = NodocchiAgent.decide(&tenpai, &tenpai_actions());
    let tenpai_inputs = tenpai_decision.push_pull_inputs.unwrap();
    assert!(tenpai_inputs.is_single_reach_threat());
    assert_eq!(tenpai_inputs.offense.unwrap().min_shanten_after_discard, 0);
    assert_eq!(tenpai_inputs.iishanten_reach_ron_risk, None);

    // 1m2m3m 5m 7m8m9m 1p3p 4p5p 9p 5s + W。どれを切っても2向聴以上。
    let two_shanten_hand = [0, 4, 8, 17, 24, 28, 32, 36, 44, 48, 52, 68, 89];
    let two_shanten = fixture(
        &two_shanten_hand,
        DRAWN,
        None,
        [false, true, false, false],
        Default::default(),
    );
    let two_shanten_actions = actions_for(&two_shanten_hand, DRAWN);
    let legal = legal_discard_evaluations(&two_shanten, &two_shanten_actions);
    assert_eq!(legal.best_shanten_after_discard(), Some(2));
    assert!(legal.iishanten_discards().is_empty());
    assert_eq!(
        production_ron_risk(&two_shanten, &two_shanten_actions),
        None
    );

    // 通常打牌選択後の入力を直接渡しても、1向聴でなければ exact model を構築しない。
    let inputs = push_pull_inputs_from_context(&two_shanten, &two_shanten_actions);
    assert_eq!(inputs.offense.unwrap().min_shanten_after_discard, 2);
    assert_eq!(
        iishanten_reach_ron_risk(
            &two_shanten,
            &inputs,
            two_shanten_actions.first(),
            legal.iishanten_discards(),
            &two_shanten_actions,
        ),
        None
    );
}

#[test]
fn no_threat_and_open_hand_threat_alone_are_not_evaluated() {
    let actions = actions_for(&HAND, DRAWN);
    let no_threat = fixture(&HAND, DRAWN, None, [false; 4], Default::default());
    assert_eq!(production_ron_risk(&no_threat, &actions), None);

    let mut melds: [Vec<Meld>; 4] = Default::default();
    melds[2] = vec![pon([120, 121, 122]), pon([96, 97, 98]), pon([56, 57, 58])];
    let open_hand = fixture(&HAND, DRAWN, None, [false; 4], melds);
    let inputs = NodocchiAgent
        .decide(&open_hand, &actions)
        .push_pull_inputs
        .unwrap();
    assert!(inputs.has_actionable_open_hand_threat());
    assert_eq!(inputs.offense.unwrap().min_shanten_after_discard, 1);
    assert_eq!(inputs.iishanten_reach_ron_risk, None);
}

#[test]
fn an_unavailable_exact_model_is_not_supplemented() {
    let mut melds: [Vec<Meld>; 4] = Default::default();
    melds[1] = vec![unavailable_reach_meld()];
    let context = fixture(&HAND, DRAWN, None, [false, true, false, false], melds);
    let actions = actions_for(&HAND, DRAWN);
    assert_eq!(
        reached_opponents_dahai_actions_by_ron_risk(&context, &actions),
        None
    );

    let decision = NodocchiAgent.decide(&context, &actions);
    let ron_risk = decision
        .push_pull_inputs
        .unwrap()
        .iishanten_reach_ron_risk
        .expect("対象局面なので unavailable として残る");
    assert_eq!(ron_risk.reacher, 1);
    assert_eq!(ron_risk.exact, None);
    assert!(!ron_risk.is_exact_available());
    assert_eq!(
        decision.push_pull.unwrap().reason,
        PushPullReason::IishantenAgainstReach
    );
}

#[test]
fn inconsistent_evidence_is_unavailable_instead_of_clamped() {
    let (a, b) = (tile_type("1m"), tile_type("2m"));
    assert_eq!(
        summarize_iishanten_ron_risk(a, &[(a, evidence(1, 10)), (b, evidence(11, 10))]),
        None
    );
    assert_eq!(
        summarize_iishanten_ron_risk(a, &[(a, evidence(0, 0)), (b, evidence(1, 10))]),
        None
    );
    // 選択打牌が1向聴候補に無い場合も推測しない。
    assert_eq!(
        summarize_iishanten_ron_risk(b, &[(a, evidence(1, 10))]),
        None
    );
}

#[test]
fn the_summary_does_not_change_the_push_pull_decision() {
    let actions = actions_for(&HAND, DRAWN);
    let cases = [
        (
            single_reach_context(None),
            PushPullReason::IishantenAgainstReach,
        ),
        (
            single_reach_context(Some(1)),
            PushPullReason::IishantenAgainstReach,
        ),
        (
            single_reach_iishanten_push_context(3),
            PushPullReason::ValuableIishantenAgainstReach,
        ),
        (
            single_reach_iishanten_push_context(1),
            PushPullReason::ValuableIishantenAgainstReach,
        ),
    ];
    for (context, reason) in cases {
        let decision = NodocchiAgent.decide(&context, &actions);
        let inputs = decision.push_pull_inputs.unwrap();
        assert!(
            inputs
                .iishanten_reach_ron_risk
                .unwrap()
                .is_exact_available()
        );

        let without = PushPullInputs {
            iishanten_reach_ron_risk: None,
            ..inputs
        };
        assert_eq!(decide_push_pull(&without), decision.push_pull.unwrap());
        assert_eq!(decide_push_pull(&inputs), decide_push_pull(&without));
        assert_eq!(decision.push_pull.unwrap().reason, reason);
        let mode = match reason {
            PushPullReason::ValuableIishantenAgainstReach => PushPullMode::Push,
            _ => PushPullMode::Fold,
        };
        assert_eq!(decision.push_pull.unwrap().mode, mode);
    }
}
