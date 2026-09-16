use super::*;

use crate::agent::Agent;
use crate::agents::ShantenAgent;
use crate::context::TableStateFacts;
use crate::defense::{
    HonorSafetyRank, PlayerRonRiskEvidence, RonRiskEvidence,
    open_hand_targets_dahai_actions_by_ron_risk, select_genbutsu_fallback_action,
};
use crate::meld::{Meld, MeldKind};
use crate::push_pull::{PushPullMode, decide_push_pull, push_pull_inputs_from_context};
use crate::shanten_test_support::{
    dahai, fold_actions, fold_under_reach_context, suited_reach_context_with_reached,
    tenpai_actions, tenpai_under_reach_context, tile,
};
use bot_logic::{TileId, TileType};

const OPEN_HAND_FOLD_HAND: [u8; 13] = [0, 4, 8, 12, 17, 20, 24, 28, 32, 36, 44, 53, 60];
const OPEN_HAND_FOLD_DRAWN: u8 = 120;
const OPEN_HAND_FOLD_DEAD: [u8; 20] = [
    37, 38, 39, 40, 41, 42, 43, 45, 46, 47, 52, 54, 55, 56, 57, 58, 59, 61, 62, 63,
];

fn plain_chi() -> Meld {
    Meld::new(
        MeldKind::Chi,
        vec![tile(72), tile(76), tile(80)],
        Some(tile(72)),
    )
}

fn open_hand_context(reached: [bool; 4]) -> GameContext {
    let mut melds: [Vec<Meld>; 4] = Default::default();
    melds[2] = (0..3).map(|_| plain_chi()).collect();

    let discards: [&[u8]; 4] = [&[], &[33], &[33], &[]];
    let mut visible: Vec<TileId> = OPEN_HAND_FOLD_HAND
        .iter()
        .map(|&value| tile(value))
        .collect();
    visible.push(tile(OPEN_HAND_FOLD_DRAWN));
    visible.extend(OPEN_HAND_FOLD_DEAD.iter().map(|&value| tile(value)));
    for discard in discards {
        visible.extend(discard.iter().map(|&value| tile(value)));
    }

    GameContext::from_parts_with_melds(
        Some(tile(OPEN_HAND_FOLD_DRAWN)),
        OPEN_HAND_FOLD_HAND
            .iter()
            .map(|&value| tile(value))
            .collect(),
        vec![],
        None,
        None,
        visible,
        Some(0),
        Some(2),
        std::array::from_fn(|player| discards[player].iter().map(|&value| tile(value)).collect()),
        reached,
        melds,
    )
}

// 副露もリーチもない局面。open_hand_context と手牌・見え牌は同じで threat だけがない。
fn no_threat_context() -> GameContext {
    let mut visible: Vec<TileId> = OPEN_HAND_FOLD_HAND
        .iter()
        .map(|&value| tile(value))
        .collect();
    visible.push(tile(OPEN_HAND_FOLD_DRAWN));
    visible.extend(OPEN_HAND_FOLD_DEAD.iter().map(|&value| tile(value)));

    GameContext::from_parts_with_melds(
        Some(tile(OPEN_HAND_FOLD_DRAWN)),
        OPEN_HAND_FOLD_HAND
            .iter()
            .map(|&value| tile(value))
            .collect(),
        vec![],
        None,
        None,
        visible,
        Some(0),
        Some(2),
        Default::default(),
        [false; 4],
        Default::default(),
    )
}

fn open_hand_actions() -> Vec<LegalAction> {
    OPEN_HAND_FOLD_HAND
        .iter()
        .map(|&value| dahai(value))
        .chain([dahai(OPEN_HAND_FOLD_DRAWN)])
        .collect()
}

// forced fold の選択が、同じ局面で既存 evaluate_fold_defense() が選ぶものと一致することを
// 確認する共通 helper。rank 1 が同じ選択であることも合わせて確認する。
fn assert_matches_fold_defense(context: &GameContext, actions: &[LegalAction]) {
    let inputs = push_pull_inputs_from_context(context, actions);
    let expected = evaluate_fold_defense(context, actions, &inputs, false);
    let expected = expected.selected().expect("既存 Fold defense が打牌を選ぶ");
    let expected_kind = match expected.kind {
        FoldDefenseKind::Reach(kind) => ForcedFoldDefenseKind::Reach(kind),
        FoldDefenseKind::OpenHand(category) => ForcedFoldDefenseKind::OpenHand(category),
        FoldDefenseKind::Combined(category) => ForcedFoldDefenseKind::Combined(category),
    };

    let forced = detailed(context, actions).expect("forced fold が打牌を選ぶ");

    assert_eq!(&forced.selected_action, expected.action);
    assert_eq!(forced.defense_kind, expected_kind);

    let ranked = forced.ranked_candidates.first().expect("候補が1件以上ある");
    assert_eq!(&ranked.action, expected.action);
    assert_eq!(ranked.rank, 1);
    assert_eq!(ranked.defense_kind, expected_kind);
}

fn detailed(
    context: &GameContext,
    actions: &[LegalAction],
) -> Result<ForcedFoldDiagnostic, ForcedFoldUnavailable> {
    evaluate_forced_fold(context, actions)
}

// 合法 Dahai の牌種を、赤5 / 黒5 を同じ候補として数えた件数。
fn dahai_tile_type_count(actions: &[LegalAction]) -> usize {
    let mut types: Vec<_> = actions
        .iter()
        .filter_map(|action| match action {
            LegalAction::Dahai { tile } => Some(tile.tile_type()),
            _ => None,
        })
        .collect();
    types.sort_by_key(|tile| tile.index());
    types.dedup();
    types.len()
}

#[test]
fn routes_a_reached_opponent_to_reach_defense() {
    let context = fold_under_reach_context();
    let actions = fold_actions();

    let forced = detailed(&context, &actions).expect("リーチ者向け防御が選ばれる");

    assert_eq!(forced.selected_action, dahai(89));
    assert_eq!(
        forced.defense_kind,
        ForcedFoldDefenseKind::Reach(DefenseFallbackKind::Genbutsu)
    );
    assert!(forced.defense.is_some());
    assert!(forced.open_hand_defense.is_none());
    assert!(forced.combined_defense.is_none());
    assert_matches_fold_defense(&context, &actions);
}

#[test]
fn routes_a_high_open_hand_to_open_hand_defense() {
    let context = open_hand_context([false; 4]);
    let actions = open_hand_actions();
    let inputs = push_pull_inputs_from_context(&context, &actions);
    assert_eq!(inputs.opponent_reach_count, 0);
    assert_eq!(
        high_open_hand_threat_players(&inputs.open_hand_threats),
        vec![2]
    );

    let forced = detailed(&context, &actions).expect("OpenHand 向け防御が選ばれる");

    assert_eq!(forced.selected_action, dahai(32));
    assert_eq!(
        forced.defense_kind,
        ForcedFoldDefenseKind::OpenHand(OpenHandDefenseCategory::SafeAgainstAllTargets)
    );
    assert!(forced.open_hand_defense.is_some());
    assert!(forced.defense.is_none());
    assert!(forced.combined_defense.is_none());
    assert_matches_fold_defense(&context, &actions);
}

#[test]
fn routes_combined_threats_to_combined_defense() {
    let context = open_hand_context([false, true, false, false]);
    let actions = open_hand_actions();

    let forced = detailed(&context, &actions).expect("複合 threat 向け防御が選ばれる");

    assert_eq!(forced.selected_action, dahai(32));
    assert_eq!(
        forced.defense_kind,
        ForcedFoldDefenseKind::Combined(CombinedDefenseCategory::SafeAgainstAllThreats)
    );
    assert!(forced.combined_defense.is_some());
    assert!(forced.defense.is_none());
    assert!(forced.open_hand_defense.is_none());
    assert_matches_fold_defense(&context, &actions);
}

#[test]
fn is_unavailable_without_a_clear_threat() {
    // リーチ者も High OpenHandThreat の相手もいない局面。通常打牌を「ベタ降り最善打牌」として
    // 返さない。
    let context = no_threat_context();
    let actions = open_hand_actions();
    let inputs = push_pull_inputs_from_context(&context, &actions);
    assert!(!has_clear_threat(&inputs));

    assert_eq!(
        detailed(&context, &actions),
        Err(ForcedFoldUnavailable::NoClearThreat)
    );
}

#[test]
fn is_unavailable_without_a_defense_selection() {
    // 合法 Dahai がなく防御打牌を選べない局面。通常打牌を forced fold の結果として返さない。
    let context = fold_under_reach_context();
    let actions = vec![LegalAction::Reach];
    let inputs = push_pull_inputs_from_context(&context, &actions);
    assert!(has_clear_threat(&inputs));

    assert_eq!(
        detailed(&context, &actions),
        Err(ForcedFoldUnavailable::NoDefenseSelection)
    );
}

#[test]
fn is_unavailable_when_the_routed_defense_cannot_select_a_tile() {
    let context =
        suited_reach_context_with_reached(Some(0), &[], &[], &[], [false, true, true, false]);
    let actions = vec![LegalAction::Reach, dahai(0), dahai(56)];

    assert_eq!(
        detailed(&context, &actions),
        Err(ForcedFoldUnavailable::NoDefenseSelection)
    );
}

#[test]
fn returns_the_fold_defense_discard_even_when_the_decision_is_push() {
    let context = tenpai_under_reach_context(None, [false, true, false, false]);
    let actions = tenpai_actions();
    let inputs = push_pull_inputs_from_context(&context, &actions);
    assert_eq!(decide_push_pull(&inputs).mode, PushPullMode::Push);

    assert_matches_fold_defense(&context, &actions);
}

#[test]
fn does_not_change_the_production_decision() {
    let context = tenpai_under_reach_context(None, [false, true, false, false]);
    let actions = tenpai_actions();
    let mut agent = ShantenAgent;

    let before = agent.act(&context, &actions);
    let before_diagnostic = ShantenAgent::diagnose(&context, &actions);

    let forced = detailed(&context, &actions).expect("forced fold が打牌を選ぶ");

    let after = agent.act(&context, &actions);
    let after_diagnostic = ShantenAgent::diagnose(&context, &actions);

    assert_eq!(before, after);
    assert_eq!(before_diagnostic, after_diagnostic);
    assert_eq!(after_diagnostic.selected_action, after);
    assert_ne!(after_diagnostic.selected_action, forced.selected_action);
}

#[test]
fn ranks_every_legal_dahai_tile_type_once() {
    for (context, actions) in [
        (fold_under_reach_context(), fold_actions()),
        (open_hand_context([false; 4]), open_hand_actions()),
        (
            open_hand_context([false, true, false, false]),
            open_hand_actions(),
        ),
    ] {
        let forced = detailed(&context, &actions).expect("forced fold が打牌を選ぶ");

        assert_eq!(
            forced.ranked_candidates.len(),
            dahai_tile_type_count(&actions)
        );
        for (index, candidate) in forced.ranked_candidates.iter().enumerate() {
            assert_eq!(candidate.rank, index + 1);
            assert!(matches!(candidate.action, LegalAction::Dahai { .. }));
        }
    }
}

#[test]
fn ranks_candidates_in_the_production_defense_ordering() {
    // ranking は既存 selector の ordering helper そのもので、family ごとに比較する。
    let reach_context = fold_under_reach_context();
    let reach_actions = fold_actions();
    let reach_inputs = push_pull_inputs_from_context(&reach_context, &reach_actions);
    let FoldDefenseEvaluation::Reach(reach) =
        evaluate_fold_defense(&reach_context, &reach_actions, &reach_inputs, true)
    else {
        panic!("リーチ者向け防御へ routing される");
    };
    let expected: Vec<_> = ordered_defense_fallback_candidates(
        &reach_context,
        &reach_actions,
        reach.ron_risk_vectors.as_deref(),
    )
    .into_iter()
    .map(|(action, kind)| (action.clone(), ForcedFoldDefenseKind::Reach(kind)))
    .collect();
    assert_ranking_matches(&reach_context, &reach_actions, &expected);

    let open_hand = open_hand_context([false; 4]);
    let open_hand_dahai = open_hand_actions();
    let open_hand_inputs = push_pull_inputs_from_context(&open_hand, &open_hand_dahai);
    let targets = high_open_hand_threat_players(&open_hand_inputs.open_hand_threats);
    let FoldDefenseEvaluation::OpenHand(evaluation) =
        evaluate_fold_defense(&open_hand, &open_hand_dahai, &open_hand_inputs, true)
    else {
        panic!("OpenHand 向け防御へ routing される");
    };
    let expected: Vec<_> = ordered_open_hand_defense_candidates(
        &open_hand,
        &open_hand_dahai,
        &targets,
        evaluation.ron_risk_vectors.as_deref(),
    )
    .into_iter()
    .map(|(action, category)| (action.clone(), ForcedFoldDefenseKind::OpenHand(category)))
    .collect();
    assert_ranking_matches(&open_hand, &open_hand_dahai, &expected);

    let combined_context = open_hand_context([false, true, false, false]);
    let combined_actions = open_hand_actions();
    let combined_inputs = push_pull_inputs_from_context(&combined_context, &combined_actions);
    let combined_targets = combined_threat_defense_targets(
        &combined_inputs.player_threats,
        &combined_inputs.open_hand_threats,
    );
    let FoldDefenseEvaluation::Combined(combined) =
        evaluate_fold_defense(&combined_context, &combined_actions, &combined_inputs, true)
    else {
        panic!("複合 threat 向け防御へ routing される");
    };
    let expected: Vec<_> = ordered_combined_defense_candidates(
        &combined_context,
        &combined_actions,
        &combined_targets,
        combined.ron_risk_vectors.as_deref(),
    )
    .into_iter()
    .map(|(action, category)| (action.clone(), ForcedFoldDefenseKind::Combined(category)))
    .collect();
    assert_ranking_matches(&combined_context, &combined_actions, &expected);
}

fn assert_ranking_matches(
    context: &GameContext,
    actions: &[LegalAction],
    expected: &[(LegalAction, ForcedFoldDefenseKind)],
) {
    let forced = detailed(context, actions).expect("forced fold が打牌を選ぶ");
    let actual: Vec<_> = forced
        .ranked_candidates
        .iter()
        .map(|candidate| (candidate.action.clone(), candidate.defense_kind))
        .collect();
    assert_eq!(actual, expected);
}

#[test]
fn collects_the_exact_candidate_evidence_after_a_genbutsu() {
    let context = fold_under_reach_context();
    let actions = fold_actions();
    let inputs = push_pull_inputs_from_context(&context, &actions);

    // 共通現物があるので、既存 evaluator は exact model を走らせず即 return できる局面。
    assert!(select_genbutsu_fallback_action(&context, &actions).is_some());
    let FoldDefenseEvaluation::Reach(evaluation) =
        evaluate_fold_defense(&context, &actions, &inputs, false)
    else {
        panic!("リーチ者向け防御へ routing される");
    };
    assert!(evaluation.ron_risk_vectors.is_none());

    // forced fold は常に詳細評価を行うので、現物で決着した後も exact candidate evidence を
    // 収集する。0-risk candidate を全件検出するために必要な評価を省略しない。
    let forced = detailed(&context, &actions).expect("forced fold が打牌を選ぶ");
    let defense = forced.defense.as_ref().expect("防御診断を構築する");
    let selected = defense.selected.as_ref().expect("選択した打牌の診断を持つ");
    assert!(selected.selected_ron_risk_evidence().is_some());
    assert!(!defense.candidates.is_empty());
    assert!(
        forced
            .ranked_candidates
            .iter()
            .all(|candidate| candidate.player_ron_risk_evidence.is_some())
    );
}

#[test]
fn shares_the_candidate_evidence_between_the_ranking_and_the_diagnostics() {
    let context = fold_under_reach_context();
    let actions = fold_actions();
    let forced = detailed(&context, &actions).expect("forced fold が打牌を選ぶ");
    let defense = forced.defense.as_ref().expect("防御診断を構築する");

    // ranking と詳細診断は1回の evaluation が構築した同じ candidate evidence を写す。
    for candidate in &forced.ranked_candidates {
        let LegalAction::Dahai { tile } = &candidate.action else {
            panic!("候補は Dahai だけ")
        };
        let diagnostic = defense
            .candidates
            .iter()
            .find(|diagnostic| diagnostic.tile == tile.tile_type())
            .expect("同じ牌種の候補診断がある");
        assert_eq!(
            candidate.player_ron_risk_evidence,
            diagnostic.player_ron_risk_evidence
        );
    }
}

#[test]
fn does_not_invent_evidence_where_the_exact_model_is_truly_unavailable() {
    // 局面情報が足りず exact model 自体を構築できない局面。診断用の追加収集を試しても
    // unavailable なので、percentage を捏造せず heuristic のまま順位を付ける。
    let context = open_hand_context([false; 4]);
    let actions = open_hand_actions();
    let targets = high_open_hand_threat_players(
        &push_pull_inputs_from_context(&context, &actions).open_hand_threats,
    );
    assert!(
        open_hand_targets_dahai_actions_by_ron_risk(&context, &actions, &targets).is_none(),
        "exact model 自体が利用できない局面"
    );

    let forced = detailed(&context, &actions).expect("forced fold が打牌を選ぶ");
    assert!(
        forced
            .ranked_candidates
            .iter()
            .all(|candidate| candidate.player_ron_risk_evidence.is_none())
    );
    assert!(
        forced
            .ranked_candidates
            .iter()
            .all(|candidate| !candidate.has_exact_zero_ron_risk())
    );
    // hard-safe は exact evidence なしでも確定 fact として 0-risk。
    let selected = forced.ranked_candidates.first().expect("候補がある");
    assert!(selected.is_hard_safe());
    assert!(selected.is_zero_risk());
    // exact が無い候補は既存 heuristic の根拠で並ぶ。
    assert!(
        forced.ranked_candidates[1..]
            .iter()
            .all(|candidate| candidate.heuristic_evidence().is_some())
    );
}

#[test]
fn separates_hard_safe_from_an_exact_zero_ron_risk() {
    let context = fold_under_reach_context();
    let actions = fold_actions();
    let forced = detailed(&context, &actions).expect("forced fold が打牌を選ぶ");

    let genbutsu = forced
        .ranked_candidates
        .iter()
        .find(|candidate| candidate.action == dahai(89))
        .expect("共通現物が候補に含まれる");
    assert!(genbutsu.is_hard_safe());
    assert!(genbutsu.is_zero_risk());
    assert_eq!(
        genbutsu.defense_kind,
        ForcedFoldDefenseKind::Reach(DefenseFallbackKind::Genbutsu)
    );
    assert!(genbutsu.heuristic_evidence().is_none());

    // 現物ではない候補は hard-safe にしない。0-risk かどうかは integer evidence だけで決める。
    for candidate in &forced.ranked_candidates {
        if candidate.is_hard_safe() {
            continue;
        }
        let evidence = candidate
            .ron_risk_evidence()
            .expect("単独リーチの exact evidence を持つ");
        assert_eq!(
            candidate.has_exact_zero_ron_risk(),
            evidence.ron_capable_weight == 0
        );
        assert_eq!(candidate.is_zero_risk(), evidence.ron_capable_weight == 0);
    }
}

#[test]
fn does_not_treat_a_rounded_zero_percent_as_zero_risk() {
    let candidate = ForcedFoldRankedCandidate {
        action: dahai(0),
        rank: 3,
        defense_kind: ForcedFoldDefenseKind::Reach(DefenseFallbackKind::ExactRonRisk),
        player_ron_risk_evidence: Some(vec![PlayerRonRiskEvidence {
            player: 1,
            evidence: RonRiskEvidence {
                ron_capable_weight: 1,
                tenpai_weight: 50_000,
            },
        }]),
    };

    assert!(!candidate.has_exact_zero_ron_risk());
    assert!(!candidate.is_zero_risk());
}

#[test]
fn requires_every_target_to_be_exactly_zero() {
    let partial = ForcedFoldRankedCandidate {
        action: dahai(0),
        rank: 2,
        defense_kind: ForcedFoldDefenseKind::Combined(CombinedDefenseCategory::ExactRonRisk),
        player_ron_risk_evidence: Some(vec![
            PlayerRonRiskEvidence {
                player: 1,
                evidence: RonRiskEvidence {
                    ron_capable_weight: 0,
                    tenpai_weight: 1_498,
                },
            },
            PlayerRonRiskEvidence {
                player: 3,
                evidence: RonRiskEvidence {
                    ron_capable_weight: 41,
                    tenpai_weight: 1_750,
                },
            },
        ]),
    };
    assert!(!partial.has_exact_zero_ron_risk());
    assert!(partial.ron_risk_evidence().is_none());

    let all_zero = ForcedFoldRankedCandidate {
        player_ron_risk_evidence: Some(vec![
            PlayerRonRiskEvidence {
                player: 1,
                evidence: RonRiskEvidence {
                    ron_capable_weight: 0,
                    tenpai_weight: 1_498,
                },
            },
            PlayerRonRiskEvidence {
                player: 3,
                evidence: RonRiskEvidence {
                    ron_capable_weight: 0,
                    tenpai_weight: 1_750,
                },
            },
        ]),
        ..partial.clone()
    };
    assert!(all_zero.has_exact_zero_ron_risk());
    assert!(all_zero.is_zero_risk());

    // worst-first 表示順も production comparator と同じ並べ替えを共有する。
    let worst_first = partial
        .worst_first_player_ron_risk_evidence()
        .expect("exact 比較できる");
    assert_eq!(
        worst_first
            .iter()
            .map(|evidence| evidence.player)
            .collect::<Vec<_>>(),
        vec![3, 1]
    );
}

#[test]
fn keeps_a_three_visible_honor_out_of_zero_risk_without_an_exact_zero() {
    let heuristic = ForcedFoldRankedCandidate {
        action: dahai(120),
        rank: 2,
        defense_kind: ForcedFoldDefenseKind::Reach(DefenseFallbackKind::HonorSafety(
            HonorSafetyRank::ThreeOrMoreVisible,
        )),
        player_ron_risk_evidence: None,
    };

    // exact model が unavailable な3枚見え字牌は heuristic safety のままで、0-risk にしない。
    assert!(!heuristic.is_hard_safe());
    assert!(!heuristic.has_exact_zero_ron_risk());
    assert!(!heuristic.is_zero_risk());
    assert_eq!(
        heuristic.heuristic_evidence(),
        Some(ForcedFoldDefenseKind::Reach(
            DefenseFallbackKind::HonorSafety(HonorSafetyRank::ThreeOrMoreVisible)
        ))
    );

    // exact model が利用できて R > 0 なら通常候補。
    let exact = ForcedFoldRankedCandidate {
        player_ron_risk_evidence: Some(vec![PlayerRonRiskEvidence {
            player: 1,
            evidence: RonRiskEvidence {
                ron_capable_weight: 3,
                tenpai_weight: 3_812,
            },
        }]),
        ..heuristic.clone()
    };
    assert!(!exact.is_zero_risk());
}

// ---- OpenHand / 複合 threat の hard-safe 決着後の candidate evidence ----

const EXACT_OPEN_HAND_TARGET: usize = 2;

fn exact_tile_type(mjai: &str) -> TileType {
    TileType::from_mjai_type_str(mjai).expect("牌種として読める")
}

fn exact_tile_types(mjai: &str) -> Vec<TileType> {
    mjai.split_whitespace().map(exact_tile_type).collect()
}

fn exact_chi(mjai: &str) -> Meld {
    let tiles: Vec<_> = exact_tile_types(mjai)
        .into_iter()
        .map(|tile| TileId::copies(tile).next().expect("物理牌がある"))
        .collect();
    Meld::new(MeldKind::Chi, tiles.clone(), Some(tiles[0]))
}

// 白の Pon。風情報が無くても確定役牌で、High OpenHandThreat の条件を満たす。
fn exact_value_pon() -> Meld {
    let tiles: Vec<_> = TileId::copies(exact_tile_type("P")).take(3).collect();
    Meld::new(MeldKind::Pon, tiles.clone(), Some(tiles[0]))
}

/// exact hidden-hand model が使える High OpenHandThreat 局面。
///
/// target は4副露で隠れ手牌が1枚だけなので、見え牌を絞ると単騎候補を数え切れる。`unseen` は
/// 「target がまだ持ち得る牌種と残り枚数」で、これが exact model の `T` になる。`unseen` に無い
/// 牌種は4枚見えなので `R == 0` だが、target 自身の河ではないので hard-safe ではない。
fn exact_open_hand_context(
    unseen: &[(&str, u8)],
    target_discards: &str,
    same_hand_passed: &str,
) -> GameContext {
    let mut melds: [Vec<Meld>; 4] = Default::default();
    melds[EXACT_OPEN_HAND_TARGET] = vec![
        exact_value_pon(),
        exact_chi("1p 2p 3p"),
        exact_chi("4p 5p 6p"),
        exact_chi("7p 8p 9p"),
    ];

    let mut remaining = [0u8; TileType::COUNT];
    for &(mjai, copies) in unseen {
        remaining[exact_tile_type(mjai).index()] = copies;
    }
    let visible: Vec<TileId> = TileType::all()
        .flat_map(|tile| TileId::copies(tile).take(usize::from(4 - remaining[tile.index()])))
        .collect();

    let mut discards: [Vec<TileId>; 4] = Default::default();
    discards[EXACT_OPEN_HAND_TARGET] = exact_tile_types(target_discards)
        .into_iter()
        .map(|tile| TileId::copies(tile).next().expect("物理牌がある"))
        .collect();

    let mut passed: [Vec<TileType>; 4] = Default::default();
    passed[EXACT_OPEN_HAND_TARGET] = exact_tile_types(same_hand_passed);

    GameContext::from_parts_with_melds(
        None,
        vec![],
        vec![],
        Some(exact_tile_type("E")),
        None,
        visible,
        Some(0),
        Some(1),
        discards,
        [false; 4],
        melds,
    )
    .with_temporary_passed_tiles(Some(Default::default()))
    .with_same_hand_passed_tiles(Some(passed))
    .with_table_state_facts(TableStateFacts {
        remaining_tiles: Some(1),
        ..Default::default()
    })
}

// 同じ局面の production evaluation が OpenHand へ routing され、exact 比較より前の段で決着して
// exact model を走らせていないことを確認する。
fn assert_open_hand_settles_without_exact_vectors(
    context: &GameContext,
    actions: &[LegalAction],
    expected: OpenHandDefenseCategory,
) {
    let inputs = push_pull_inputs_from_context(context, actions);
    let FoldDefenseEvaluation::OpenHand(production) =
        evaluate_fold_defense(context, actions, &inputs, true)
    else {
        panic!("OpenHand 向け防御へ routing される");
    };
    let (_, category) = production.selected.expect("防御打牌を選ぶ");
    assert_eq!(category, expected);
    assert!(production.ron_risk_vectors.is_none());
}

#[test]
fn collects_the_open_hand_candidate_evidence_after_a_hard_safe_selection() {
    // 3m が target の河にあるので hard-safe で決着する。4m は4枚見えで `R == 0` だが hard-safe
    // ではなく、1m / 2m は `R > 0`。
    let context = exact_open_hand_context(&[("1m", 1), ("2m", 2), ("3m", 1)], "3m", "");
    let actions = vec![dahai(8), dahai(12), dahai(0), dahai(4)];

    assert_open_hand_settles_without_exact_vectors(
        &context,
        &actions,
        OpenHandDefenseCategory::SafeAgainstAllTargets,
    );

    let forced = detailed(&context, &actions).expect("forced fold が打牌を選ぶ");

    // selection は変わらない。
    assert_eq!(forced.selected_action, dahai(8));
    assert_eq!(
        forced.defense_kind,
        ForcedFoldDefenseKind::OpenHand(OpenHandDefenseCategory::SafeAgainstAllTargets)
    );

    // hard-safe が先頭のまま、その下の候補は exact evidence で並ぶ。
    assert_eq!(
        forced
            .ranked_candidates
            .iter()
            .map(|candidate| (
                candidate.action.clone(),
                candidate.defense_kind,
                candidate
                    .ron_risk_evidence()
                    .map(|evidence| evidence.ron_capable_weight)
            ))
            .collect::<Vec<_>>(),
        vec![
            (
                dahai(8),
                ForcedFoldDefenseKind::OpenHand(OpenHandDefenseCategory::SafeAgainstAllTargets),
                Some(0),
            ),
            (
                dahai(12),
                ForcedFoldDefenseKind::OpenHand(OpenHandDefenseCategory::ExactRonRisk),
                Some(0),
            ),
            (
                dahai(0),
                ForcedFoldDefenseKind::OpenHand(OpenHandDefenseCategory::ExactRonRisk),
                Some(1),
            ),
            (
                dahai(4),
                ForcedFoldDefenseKind::OpenHand(OpenHandDefenseCategory::ExactRonRisk),
                Some(2),
            ),
        ]
    );

    // hard-safe と exact `R == 0` は別の根拠として残る。
    let hard_safe = &forced.ranked_candidates[0];
    assert!(hard_safe.is_hard_safe());
    assert!(hard_safe.is_zero_risk());
    let exact_zero = &forced.ranked_candidates[1];
    assert!(!exact_zero.is_hard_safe());
    assert!(exact_zero.has_exact_zero_ron_risk());
    assert!(exact_zero.is_zero_risk());
}

#[test]
fn collects_the_open_hand_candidate_evidence_after_a_same_hand_passed_selection() {
    // target の河に該当牌が無いので hard-safe は無く、same-hand passed の 1m で決着する。
    let context = exact_open_hand_context(&[("1m", 1), ("2m", 2), ("3m", 1)], "", "1m");
    let actions = vec![dahai(0), dahai(12), dahai(4)];

    assert_open_hand_settles_without_exact_vectors(
        &context,
        &actions,
        OpenHandDefenseCategory::SameHandPassed,
    );

    let forced = detailed(&context, &actions).expect("forced fold が打牌を選ぶ");

    assert_eq!(forced.selected_action, dahai(0));
    assert_eq!(
        forced.defense_kind,
        ForcedFoldDefenseKind::OpenHand(OpenHandDefenseCategory::SameHandPassed)
    );

    // same-hand passed が先頭のまま、その下は exact evidence で並ぶ。
    assert_eq!(
        forced
            .ranked_candidates
            .iter()
            .map(|candidate| (
                candidate.rank,
                candidate.action.clone(),
                candidate.defense_kind
            ))
            .collect::<Vec<_>>(),
        vec![
            (
                1,
                dahai(0),
                ForcedFoldDefenseKind::OpenHand(OpenHandDefenseCategory::SameHandPassed),
            ),
            (
                2,
                dahai(12),
                ForcedFoldDefenseKind::OpenHand(OpenHandDefenseCategory::ExactRonRisk),
            ),
            (
                3,
                dahai(4),
                ForcedFoldDefenseKind::OpenHand(OpenHandDefenseCategory::ExactRonRisk),
            ),
        ]
    );

    // same-hand passed は hard-safe ではないので、`R > 0` なら 0-risk にしない。
    let same_hand_passed = &forced.ranked_candidates[0];
    assert!(!same_hand_passed.is_hard_safe());
    assert_eq!(
        same_hand_passed
            .ron_risk_evidence()
            .expect("exact evidence を持つ")
            .ron_capable_weight,
        1
    );
    assert!(!same_hand_passed.is_zero_risk());
    // 4m は4枚見えで `R == 0` なので 0-risk。
    assert!(forced.ranked_candidates[1].has_exact_zero_ron_risk());
}

#[test]
fn keeps_a_zero_risk_candidate_below_the_hard_safe_ranks() {
    // 3m / 7m / 8m が target の河にあるので上位3件は hard-safe。4m は4枚見えで `R == 0` だが
    // hard-safe ではないので rank 4 に並ぶ。
    let context = exact_open_hand_context(&[("1m", 1), ("2m", 2), ("3m", 1)], "3m 7m 8m", "");
    let actions = vec![
        dahai(24),
        dahai(28),
        dahai(8),
        dahai(12),
        dahai(0),
        dahai(4),
    ];

    assert_open_hand_settles_without_exact_vectors(
        &context,
        &actions,
        OpenHandDefenseCategory::SafeAgainstAllTargets,
    );

    let forced = detailed(&context, &actions).expect("forced fold が打牌を選ぶ");
    assert_eq!(forced.selected_action, dahai(24));

    // 0-risk candidate の順位を付け替えない。
    let zero_risk: Vec<_> = forced
        .ranked_candidates
        .iter()
        .filter(|candidate| candidate.is_zero_risk())
        .map(|candidate| (candidate.rank, candidate.action.clone()))
        .collect();
    assert_eq!(
        zero_risk,
        vec![
            (1, dahai(24)),
            (2, dahai(28)),
            (3, dahai(8)),
            (4, dahai(12)),
        ]
    );
    assert!(forced.ranked_candidates[3].has_exact_zero_ron_risk());
    assert!(!forced.ranked_candidates[3].is_hard_safe());
    assert!(!forced.ranked_candidates[4].is_zero_risk());
}

#[test]
fn does_not_recollect_the_evidence_when_the_exact_comparison_already_ran() {
    // hard-safe も same-hand passed も無いので production が exact 比較まで進む。診断のために
    // 追加収集せず、selection が構築した vector をそのまま使う。
    let context = exact_open_hand_context(&[("1m", 1), ("2m", 2), ("3m", 1)], "", "");
    let actions = vec![dahai(0), dahai(4), dahai(8)];
    let inputs = push_pull_inputs_from_context(&context, &actions);
    let FoldDefenseEvaluation::OpenHand(production) =
        evaluate_fold_defense(&context, &actions, &inputs, true)
    else {
        panic!("OpenHand 向け防御へ routing される");
    };
    let (_, category) = production.selected.expect("防御打牌を選ぶ");
    assert_eq!(category, OpenHandDefenseCategory::ExactRonRisk);
    let vectors = production
        .ron_risk_vectors
        .as_deref()
        .expect("selection が exact vector を構築する");

    let forced = detailed(&context, &actions).expect("forced fold が打牌を選ぶ");
    for candidate in &forced.ranked_candidates {
        assert_eq!(
            candidate.player_ron_risk_evidence.as_deref(),
            player_ron_risk_evidence_for_action(Some(vectors), &candidate.action)
        );
    }
}
