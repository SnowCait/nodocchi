use super::*;

use crate::agent::Agent;
use crate::agents::ShantenAgent;
use crate::meld::{Meld, MeldKind};
use crate::open_hand_defense::high_open_hand_threat_players;
use crate::push_pull::{PushPullMode, decide_push_pull, push_pull_inputs_from_context};
use crate::shanten_test_support::{
    dahai, fold_actions, fold_under_reach_context, suited_reach_context_with_reached,
    tenpai_actions, tenpai_under_reach_context, tile,
};
use bot_logic::TileId;

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
// 確認する共通 helper。
fn assert_matches_fold_defense(context: &GameContext, actions: &[LegalAction]) {
    let inputs = push_pull_inputs_from_context(context, actions);
    let expected = evaluate_fold_defense(context, actions, &inputs, false);
    let expected = expected.selected().expect("既存 Fold defense が打牌を選ぶ");

    let forced = evaluate_forced_fold(context, actions).expect("forced fold が打牌を選ぶ");

    assert_eq!(&forced.selected_action, expected.action);
    let expected_kind = match expected.kind {
        FoldDefenseKind::Reach(kind) => ForcedFoldDefenseKind::Reach(kind),
        FoldDefenseKind::OpenHand(category) => ForcedFoldDefenseKind::OpenHand(category),
        FoldDefenseKind::Combined(category) => ForcedFoldDefenseKind::Combined(category),
    };
    assert_eq!(forced.defense_kind, expected_kind);
}

#[test]
fn routes_a_reached_opponent_to_reach_defense() {
    let context = fold_under_reach_context();
    let actions = fold_actions();

    let forced = evaluate_forced_fold(&context, &actions).expect("リーチ者向け防御が選ばれる");

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

    let forced = evaluate_forced_fold(&context, &actions).expect("OpenHand 向け防御が選ばれる");

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

    let forced = evaluate_forced_fold(&context, &actions).expect("複合 threat 向け防御が選ばれる");

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
        evaluate_forced_fold(&context, &actions),
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
        evaluate_forced_fold(&context, &actions),
        Err(ForcedFoldUnavailable::NoDefenseSelection)
    );
}

#[test]
fn is_unavailable_when_the_routed_defense_cannot_select_a_tile() {
    let context =
        suited_reach_context_with_reached(Some(0), &[], &[], &[], [false, true, true, false]);
    let actions = vec![LegalAction::Reach, dahai(0), dahai(56)];

    assert_eq!(
        evaluate_forced_fold(&context, &actions),
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

    let forced = evaluate_forced_fold(&context, &actions).expect("forced fold が打牌を選ぶ");

    let after = agent.act(&context, &actions);
    let after_diagnostic = ShantenAgent::diagnose(&context, &actions);

    assert_eq!(before, after);
    assert_eq!(before_diagnostic, after_diagnostic);
    assert_eq!(after_diagnostic.selected_action, after);
    assert_ne!(after_diagnostic.selected_action, forced.selected_action);
}
