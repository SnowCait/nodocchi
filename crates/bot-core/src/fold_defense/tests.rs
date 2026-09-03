use super::*;

use crate::context::GameContext;
use crate::meld::{Meld, MeldKind};
use crate::push_pull::{PushPullMode, push_pull_inputs_from_context};
use crate::shanten_test_support::{
    dahai, fold_actions, fold_under_reach_context, suited_reach_context_with_reached, tile,
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

fn open_hand_actions() -> Vec<LegalAction> {
    OPEN_HAND_FOLD_HAND
        .iter()
        .map(|&value| dahai(value))
        .chain([dahai(OPEN_HAND_FOLD_DRAWN)])
        .collect()
}

#[test]
fn routes_a_reached_opponent_to_reach_defense() {
    let context = fold_under_reach_context();
    let actions = fold_actions();
    let inputs = push_pull_inputs_from_context(&context, &actions);
    assert_eq!(inputs.opponent_reach_count, 1);
    assert!(!inputs.has_combined_threat());

    let evaluation = evaluate_fold_defense(&context, &actions, &inputs, false);
    let selected = evaluation.selected().expect("reach defense selects a tile");

    assert_eq!(selected.action, &dahai(89));
    assert_eq!(
        selected.kind,
        FoldDefenseKind::Reach(DefenseFallbackKind::Genbutsu)
    );
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

    let evaluation = evaluate_fold_defense(&context, &actions, &inputs, false);
    let selected = evaluation
        .selected()
        .expect("open-hand defense selects a tile");

    assert_eq!(selected.action, &dahai(32));
    assert_eq!(
        selected.kind,
        FoldDefenseKind::OpenHand(OpenHandDefenseCategory::SafeAgainstAllTargets)
    );
}

#[test]
fn routes_combined_threats_to_combined_defense() {
    let context = open_hand_context([false, true, false, false]);
    let actions = open_hand_actions();
    let inputs = push_pull_inputs_from_context(&context, &actions);
    assert!(inputs.has_combined_threat());

    let evaluation = evaluate_fold_defense(&context, &actions, &inputs, false);
    let selected = evaluation
        .selected()
        .expect("combined defense selects a tile");

    assert_eq!(selected.action, &dahai(32));
    assert_eq!(
        selected.kind,
        FoldDefenseKind::Combined(CombinedDefenseCategory::SafeAgainstAllThreats)
    );
}

#[test]
fn returns_none_when_the_routed_defense_cannot_select_a_tile() {
    let context =
        suited_reach_context_with_reached(Some(0), &[], &[], &[], [false, true, true, false]);
    let actions = vec![LegalAction::Reach, dahai(0), dahai(56)];
    let inputs = push_pull_inputs_from_context(&context, &actions);
    assert_eq!(inputs.opponent_reach_count, 2);
    assert_eq!(
        crate::push_pull::decide_push_pull(&inputs).mode,
        PushPullMode::Fold
    );

    let evaluation = evaluate_fold_defense(&context, &actions, &inputs, false);
    assert!(matches!(evaluation, FoldDefenseEvaluation::Reach(_)));
    assert!(evaluation.selected().is_none());
}
