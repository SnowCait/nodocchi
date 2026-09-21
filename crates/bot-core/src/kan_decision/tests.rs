use super::*;

use bot_logic::{AcceptanceTile, TileType};

use crate::discard_selection::select_best_normal_discard_evaluation;
use crate::meld::{Meld, MeldKind};
use crate::shanten_test_support::{
    ANKAN_FREE_CONSUMED, ANKAN_FREE_DRAWN, ANKAN_FREE_HAND, ANKAN_REGRESSING_CONSUMED,
    ANKAN_REGRESSING_DRAWN, ANKAN_REGRESSING_HAND, ankan_action, ankan_context,
    ankan_dahai_actions, tile,
};

fn tiles(values: &[u8]) -> Vec<TileId> {
    values.iter().map(|&value| tile(value)).collect()
}

fn context(hand: &[u8], drawn: u8, reached: [bool; 4], melds: [Vec<Meld>; 4]) -> GameContext {
    ankan_context(hand, drawn, reached, melds, Some(0))
}

fn kakan() -> LegalAction {
    LegalAction::Kakan {
        tile: tile(124),
        consumed: tiles(&[125, 126, 127]),
    }
}

fn daiminkan() -> LegalAction {
    LegalAction::Daiminkan {
        tile: tile(104),
        consumed: tiles(&[105, 106, 107]),
    }
}

// production の通常打牌選択が選んだ評価。カン判断へ渡す比較の基準をテストでも同じ経路から作る。
fn baseline(ctx: &GameContext, legal_actions: &[LegalAction]) -> Option<DiscardEvaluation> {
    let tiles: Vec<TileId> = ctx
        .hand_tiles()
        .iter()
        .copied()
        .chain(ctx.drawn_tile())
        .collect();
    select_best_normal_discard_evaluation(ctx, &tiles, legal_actions)
}

fn decide(
    ctx: &GameContext,
    legal_actions: &[LegalAction],
    mode: PushPullMode,
) -> Option<KanDecisionDiagnostic> {
    let normal_discard = baseline(ctx, legal_actions);
    evaluate_kan_decision(ctx, legal_actions, mode, normal_discard.as_ref())
}

// 暗刻テンパイ局面の合法手。Reach は含めず、カン判断そのものを見る。
fn ankan_free_actions() -> Vec<LegalAction> {
    ankan_dahai_actions(&ANKAN_FREE_HAND, ANKAN_FREE_DRAWN)
        .into_iter()
        .chain([ankan_action(&ANKAN_FREE_CONSUMED)])
        .collect()
}

fn regressing_actions() -> Vec<LegalAction> {
    ankan_dahai_actions(&ANKAN_REGRESSING_HAND, ANKAN_REGRESSING_DRAWN)
        .into_iter()
        .chain([ankan_action(&ANKAN_REGRESSING_CONSUMED)])
        .collect()
}

#[test]
fn no_legal_kan_is_not_evaluated() {
    let ctx = context(
        &ANKAN_FREE_HAND,
        ANKAN_FREE_DRAWN,
        [false; 4],
        Default::default(),
    );
    let actions = ankan_dahai_actions(&ANKAN_FREE_HAND, ANKAN_FREE_DRAWN);

    assert_eq!(decide(&ctx, &actions, PushPullMode::Push), None);
}

#[test]
fn ankan_is_selected_when_shanten_and_acceptance_do_not_regress() {
    let ctx = context(
        &ANKAN_FREE_HAND,
        ANKAN_FREE_DRAWN,
        [false; 4],
        Default::default(),
    );
    let actions = ankan_free_actions();

    let decision = decide(&ctx, &actions, PushPullMode::Push).expect("暗槓候補");
    assert_eq!(decision.selected, Some(ankan_action(&ANKAN_FREE_CONSUMED)));
    assert_eq!(
        decision.reason,
        KanDecisionReason::EligibleAnkanNoRegression
    );

    let candidate = decision.candidates.first().expect("候補");
    assert!(candidate.selected);
    assert!(candidate.eligible);
    assert_eq!(candidate.kind, KanKind::Ankan);
    assert_eq!(candidate.tile, TileType::from_mjai_type_str("E").ok());
    assert_eq!(candidate.current_fixed_meld_count.map(|c| c.get()), Some(0));
    assert_eq!(
        candidate.post_kan_fixed_meld_count.map(|c| c.get()),
        Some(1)
    );
    // 向聴も受け入れも悪化していない。
    assert_eq!(candidate.shanten_delta(), Some(0));
    assert_eq!(candidate.acceptance_remaining_delta(), Some(0));
    assert_eq!(candidate.acceptance_type_delta(), Some(0));
}

#[test]
fn ankan_is_rejected_when_shanten_regresses() {
    let ctx = context(
        &ANKAN_REGRESSING_HAND,
        ANKAN_REGRESSING_DRAWN,
        [false; 4],
        Default::default(),
    );
    let actions = regressing_actions();

    let decision = decide(&ctx, &actions, PushPullMode::Push).expect("暗槓候補");
    assert_eq!(decision.selected, None);
    assert_eq!(decision.reason, KanDecisionReason::ShantenRegresses);

    let candidate = decision.candidates.first().expect("候補");
    assert!(!candidate.eligible);
    // 判断に使った既存評価の両側を診断に残す。
    assert_eq!(candidate.baseline.map(|hand| hand.shanten), Some(0));
    assert_eq!(candidate.post_kan.map(|hand| hand.shanten), Some(1));
    assert!(candidate.shanten_delta().is_some_and(|delta| delta > 0));
}

#[test]
fn ankan_is_rejected_when_acceptance_regresses() {
    let ctx = context(
        &ANKAN_FREE_HAND,
        ANKAN_FREE_DRAWN,
        [false; 4],
        Default::default(),
    );
    let actions = ankan_free_actions();

    // 受け入れが1牌種 (9p) 分だけ広い通常打牌を基準にした場合。向聴は変わらないが、暗槓後の
    // 受け入れがその基準より狭くなるので暗槓しない。
    let mut wider = baseline(&ctx, &actions).expect("通常打牌評価");
    let shanten_after_draw = wider.shanten_after_discard;
    wider.acceptance_after_discard.tiles.push(AcceptanceTile {
        tile: TileType::new(17).unwrap(),
        remaining: 4,
        shanten_after_draw,
    });

    let decision =
        evaluate_kan_decision(&ctx, &actions, PushPullMode::Push, Some(&wider)).expect("暗槓候補");
    assert_eq!(decision.selected, None);
    assert_eq!(decision.reason, KanDecisionReason::AcceptanceRegresses);

    let candidate = decision.candidates.first().expect("候補");
    // 向聴は悪化していないが、受け入れが減っている。
    assert_eq!(candidate.shanten_delta(), Some(0));
    assert!(
        candidate
            .acceptance_remaining_delta()
            .is_some_and(|delta| delta < 0)
    );
}

#[test]
fn ankan_is_rejected_under_opponent_reach() {
    let ctx = context(
        &ANKAN_FREE_HAND,
        ANKAN_FREE_DRAWN,
        [false, true, false, false],
        Default::default(),
    );
    let actions = ankan_free_actions();

    let decision = decide(&ctx, &actions, PushPullMode::Push).expect("暗槓候補");
    assert_eq!(decision.selected, None);
    assert_eq!(decision.reason, KanDecisionReason::OpponentReached);
    // 他家リーチで落ちた候補は、以降の判断材料を推測で埋めない。
    let candidate = decision.candidates.first().expect("候補");
    assert_eq!(candidate.current_fixed_meld_count, None);
    assert_eq!(candidate.baseline, None);
    assert_eq!(candidate.post_kan, None);
}

#[test]
fn ankan_is_rejected_outside_push() {
    let ctx = context(
        &ANKAN_FREE_HAND,
        ANKAN_FREE_DRAWN,
        [false; 4],
        Default::default(),
    );
    let actions = ankan_free_actions();

    for mode in [PushPullMode::Neutral, PushPullMode::Fold] {
        let decision = decide(&ctx, &actions, mode).expect("暗槓候補");
        assert_eq!(decision.selected, None, "mode: {mode:?}");
        assert_eq!(
            decision.reason,
            KanDecisionReason::NotPush,
            "mode: {mode:?}"
        );
    }
}

#[test]
fn ankan_is_rejected_without_a_confirmed_own_draw() {
    let ctx = GameContext::from_parts_with_melds(
        None,
        tiles(&ANKAN_FREE_HAND),
        vec![],
        TileType::from_mjai_type_str("E").ok(),
        TileType::from_mjai_type_str("E").ok(),
        tiles(&ANKAN_FREE_HAND),
        Some(0),
        Some(0),
        Default::default(),
        [false; 4],
        Default::default(),
    );
    let actions = vec![ankan_action(&ANKAN_FREE_CONSUMED), LegalAction::None];

    let decision = decide(&ctx, &actions, PushPullMode::Push).expect("暗槓候補");
    assert_eq!(decision.selected, None);
    assert_eq!(decision.reason, KanDecisionReason::NotAfterOwnDraw);
}

#[test]
fn ankan_is_rejected_when_the_fixed_meld_count_is_unknown() {
    let ctx = ankan_context(
        &ANKAN_FREE_HAND,
        ANKAN_FREE_DRAWN,
        [false; 4],
        Default::default(),
        None,
    );
    let actions = ankan_free_actions();

    let decision = decide(&ctx, &actions, PushPullMode::Push).expect("暗槓候補");
    assert_eq!(decision.selected, None);
    assert_eq!(decision.reason, KanDecisionReason::FixedMeldCountUnknown);
}

#[test]
fn ankan_is_rejected_when_the_post_kan_fixed_meld_count_overflows() {
    let pon = Meld::new(MeldKind::Pon, tiles(&[112, 113, 114]), Some(tile(112)));
    let melds = [
        vec![pon.clone(), pon.clone(), pon.clone(), pon],
        vec![],
        vec![],
        vec![],
    ];
    let ctx = context(&ANKAN_FREE_HAND, ANKAN_FREE_DRAWN, [false; 4], melds);
    let actions = vec![ankan_action(&ANKAN_FREE_CONSUMED), LegalAction::None];

    let decision = decide(&ctx, &actions, PushPullMode::Push).expect("暗槓候補");
    assert_eq!(decision.selected, None);
    assert_eq!(decision.reason, KanDecisionReason::FixedMeldCountOverflow);
}

#[test]
fn ankan_is_rejected_when_consumed_is_not_in_the_hand() {
    let ctx = context(
        &ANKAN_FREE_HAND,
        ANKAN_FREE_DRAWN,
        [false; 4],
        Default::default(),
    );
    // 手牌に無い 南 (112..115) の暗槓。
    let actions = vec![ankan_action(&[112, 113, 114, 115]), LegalAction::None];

    let decision = decide(&ctx, &actions, PushPullMode::Push).expect("暗槓候補");
    assert_eq!(decision.selected, None);
    assert_eq!(decision.reason, KanDecisionReason::InvalidConsumed);
}

#[test]
fn ankan_is_rejected_when_consumed_is_not_four_tiles() {
    let ctx = context(
        &ANKAN_FREE_HAND,
        ANKAN_FREE_DRAWN,
        [false; 4],
        Default::default(),
    );
    let actions = vec![ankan_action(&[108, 109, 110]), LegalAction::None];

    let decision = decide(&ctx, &actions, PushPullMode::Push).expect("暗槓候補");
    assert_eq!(decision.reason, KanDecisionReason::InvalidConsumed);
}

#[test]
fn ankan_is_rejected_without_a_normal_discard_baseline() {
    let ctx = context(
        &ANKAN_FREE_HAND,
        ANKAN_FREE_DRAWN,
        [false; 4],
        Default::default(),
    );
    let actions = vec![ankan_action(&ANKAN_FREE_CONSUMED)];

    let decision =
        evaluate_kan_decision(&ctx, &actions, PushPullMode::Push, None).expect("暗槓候補");
    assert_eq!(decision.selected, None);
    assert_eq!(decision.reason, KanDecisionReason::NormalDiscardUnavailable);
}

#[test]
fn kakan_is_not_connected_to_production() {
    let ctx = context(
        &ANKAN_FREE_HAND,
        ANKAN_FREE_DRAWN,
        [false; 4],
        Default::default(),
    );
    let actions = vec![kakan(), LegalAction::None];

    let decision = decide(&ctx, &actions, PushPullMode::Push).expect("カン候補");
    assert_eq!(decision.selected, None);
    assert_eq!(decision.reason, KanDecisionReason::KakanNotConnected);
    assert_eq!(decision.candidates[0].kind, KanKind::Kakan);
    assert!(!KanKind::Kakan.is_production_connected());
}

#[test]
fn daiminkan_is_not_connected_to_production() {
    let ctx = context(
        &ANKAN_FREE_HAND,
        ANKAN_FREE_DRAWN,
        [false; 4],
        Default::default(),
    );
    let actions = vec![daiminkan(), LegalAction::None];

    let decision = decide(&ctx, &actions, PushPullMode::Push).expect("カン候補");
    assert_eq!(decision.selected, None);
    assert_eq!(decision.reason, KanDecisionReason::DaiminkanNotConnected);
    assert_eq!(decision.candidates[0].kind, KanKind::Daiminkan);
    assert!(!KanKind::Daiminkan.is_production_connected());
}

#[test]
fn only_ankan_is_production_connected() {
    assert!(KanKind::Ankan.is_production_connected());
    assert_eq!(KanKind::Ankan.meld_kind(), MeldKind::Ankan);
    assert_eq!(KanKind::Kakan.meld_kind(), MeldKind::Kakan);
    assert_eq!(KanKind::Daiminkan.meld_kind(), MeldKind::Daiminkan);
}

#[test]
fn every_legal_kan_is_evaluated_independently() {
    let ctx = context(
        &ANKAN_FREE_HAND,
        ANKAN_FREE_DRAWN,
        [false; 4],
        Default::default(),
    );
    let actions: Vec<LegalAction> = ankan_dahai_actions(&ANKAN_FREE_HAND, ANKAN_FREE_DRAWN)
        .into_iter()
        .chain([kakan(), daiminkan(), ankan_action(&ANKAN_FREE_CONSUMED)])
        .collect();

    let decision = decide(&ctx, &actions, PushPullMode::Push).expect("カン候補");
    assert_eq!(decision.candidates.len(), 3);
    assert_eq!(
        decision.candidates[0].reason,
        KanDecisionReason::KakanNotConnected
    );
    assert_eq!(
        decision.candidates[1].reason,
        KanDecisionReason::DaiminkanNotConnected
    );
    assert_eq!(
        decision.candidates[2].reason,
        KanDecisionReason::EligibleAnkanNoRegression
    );
    assert_eq!(decision.selected, Some(ankan_action(&ANKAN_FREE_CONSUMED)));
    assert_eq!(
        decision.reason,
        KanDecisionReason::EligibleAnkanNoRegression
    );
}

#[test]
fn eligible_reason_is_only_the_ankan_verdict() {
    assert!(KanDecisionReason::EligibleAnkanNoRegression.is_eligible());
    for reason in [
        KanDecisionReason::KakanNotConnected,
        KanDecisionReason::DaiminkanNotConnected,
        KanDecisionReason::OpponentReached,
        KanDecisionReason::NotPush,
        KanDecisionReason::NotAfterOwnDraw,
        KanDecisionReason::FixedMeldCountUnknown,
        KanDecisionReason::FixedMeldCountOverflow,
        KanDecisionReason::InvalidConsumed,
        KanDecisionReason::NormalDiscardUnavailable,
        KanDecisionReason::ShantenRegresses,
        KanDecisionReason::AcceptanceRegresses,
    ] {
        assert!(!reason.is_eligible(), "reason: {reason:?}");
    }
}
