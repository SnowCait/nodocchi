use super::*;

use bot_logic::{AcceptanceTile, EffectiveShanten, TileType};

use crate::context::TableStateFacts;
use crate::discard_selection::select_best_normal_discard_evaluation;
use crate::meld::{Meld, MeldKind};
use crate::offense_value::OffenseValue;
use crate::reach_policy::REACH_MIN_REMAINING_TILES;
use crate::shanten_test_support::{
    ANKAN_ACCEPTANCE_REGRESSING_CONSUMED, ANKAN_ACCEPTANCE_REGRESSING_DRAWN,
    ANKAN_ACCEPTANCE_REGRESSING_HAND, ANKAN_FREE_CONSUMED, ANKAN_FREE_DRAWN, ANKAN_FREE_HAND,
    ANKAN_IISHANTEN_CONSUMED, ANKAN_IISHANTEN_DRAWN, ANKAN_IISHANTEN_HAND, ANKAN_REACH_CONSUMED,
    ANKAN_REACH_DRAWN, ANKAN_REACH_HAND, ANKAN_REGRESSING_CONSUMED, ANKAN_REGRESSING_DRAWN,
    ANKAN_REGRESSING_HAND, KAKAN_FREE_CONSUMED, KAKAN_FREE_DRAWN, KAKAN_FREE_HAND,
    KAKAN_IISHANTEN_DRAWN, KAKAN_IISHANTEN_HAND, KAKAN_SHANTEN_REGRESSING_ADDED,
    KAKAN_SHANTEN_REGRESSING_CONSUMED, KAKAN_SHANTEN_REGRESSING_DRAWN,
    KAKAN_SHANTEN_REGRESSING_HAND, KAKAN_VALUE_REGRESSING_ADDED, KAKAN_VALUE_REGRESSING_CONSUMED,
    KAKAN_VALUE_REGRESSING_DRAWN, KAKAN_VALUE_REGRESSING_HAND, ankan_action, ankan_context,
    ankan_dahai_actions, dahai, east_pon_meld, kakan_action, kakan_context,
    rivers_with_tile_for_all_opponents, three_man_pon_meld, tile, two_man_pon_meld,
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

fn iishanten_actions() -> Vec<LegalAction> {
    ankan_dahai_actions(&ANKAN_IISHANTEN_HAND, ANKAN_IISHANTEN_DRAWN)
        .into_iter()
        .chain([ankan_action(&ANKAN_IISHANTEN_CONSUMED)])
        .collect()
}

// 打点比較だけを見るための13枚相当 state。
fn hand(
    shanten: i8,
    acceptance_remaining: u8,
    offense: Option<TenpaiOffenseValue>,
) -> KanHandDiagnostic {
    KanHandDiagnostic {
        shanten,
        acceptance_remaining,
        acceptance_type_count: 1,
        offense,
    }
}

fn offense(mode: TenpaiOffenseMode, weighted_total: u64) -> Option<TenpaiOffenseValue> {
    Some(TenpaiOffenseValue {
        mode,
        value: OffenseValue::Known {
            weighted_total,
            total_remaining: 4,
        },
    })
}

fn unknown_offense(mode: TenpaiOffenseMode) -> Option<TenpaiOffenseValue> {
    Some(TenpaiOffenseValue {
        mode,
        value: OffenseValue::Unknown,
    })
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
fn ankan_is_selected_when_shanten_acceptance_and_value_do_not_regress() {
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
    // 打点も既存の攻撃打点で比較し、暗槓後が下回っていない。暗槓で増える符は既存 scoring が
    // 暗槓込みで求めるが、満貫で頭打ちになる手では合計が変わらないこともある。
    assert_eq!(
        candidate.baseline.and_then(|hand| hand.offense_mode()),
        candidate.post_kan.and_then(|hand| hand.offense_mode())
    );
    assert!(
        candidate
            .baseline
            .and_then(|hand| hand.weighted_total())
            .is_some()
    );
    assert!(
        candidate
            .weighted_total_delta()
            .is_some_and(|delta| delta >= 0),
        "candidate: {candidate:?}"
    );
}

// テンパイ以外は既存の攻撃打点で暗槓前後を比較できないので、速度が悪化しなくても暗槓しない。
#[test]
fn ankan_is_rejected_when_the_value_is_not_evaluable() {
    let ctx = context(
        &ANKAN_IISHANTEN_HAND,
        ANKAN_IISHANTEN_DRAWN,
        [false; 4],
        Default::default(),
    );
    let actions = iishanten_actions();

    let decision = decide(&ctx, &actions, PushPullMode::Push).expect("暗槓候補");
    assert_eq!(decision.selected, None);
    assert_eq!(decision.reason, KanDecisionReason::ValueNotEvaluable);

    let candidate = decision.candidates.first().expect("候補");
    // 速度は悪化していないが、どちらの side もテンパイではないので打点を確定できない。
    assert_eq!(candidate.shanten_delta(), Some(0));
    assert!(
        candidate
            .acceptance_remaining_delta()
            .is_some_and(|delta| delta >= 0)
    );
    assert_eq!(candidate.baseline.and_then(|hand| hand.offense), None);
    assert_eq!(candidate.post_kan.and_then(|hand| hand.offense), None);
}

// 暗槓で向聴が進む場合、向聴段階が違う受け入れを直接比べて reject しない。向聴が進んだ分の
// 価値も既存 primitive では確定できないので、改善だけを根拠に暗槓もしない。
#[test]
fn an_improved_shanten_is_not_rejected_by_comparing_acceptance() {
    let ctx = context(
        &ANKAN_FREE_HAND,
        ANKAN_FREE_DRAWN,
        [false; 4],
        Default::default(),
    );
    let actions = ankan_free_actions();

    // 暗槓しない側が「1向聴・受け入れ8枚2種」、暗槓後が「テンパイ・待ち3枚1種」になる比較。
    // 枚数だけ見れば 3 < 8 だが、向聴段階が違うので受け入れの劣化ではない。
    let mut iishanten = baseline(&ctx, &actions).expect("通常打牌評価");
    iishanten.shanten_after_discard = EffectiveShanten::Melded { standard: 1 };
    iishanten.acceptance_after_discard.current = EffectiveShanten::Melded { standard: 1 };
    iishanten
        .acceptance_after_discard
        .tiles
        .push(AcceptanceTile {
            tile: TileType::new(17).unwrap(),
            remaining: 5,
            shanten_after_draw: EffectiveShanten::Melded { standard: 0 },
        });

    let decision = evaluate_kan_decision(&ctx, &actions, PushPullMode::Push, Some(&iishanten))
        .expect("暗槓候補");
    let candidate = decision.candidates.first().expect("候補");
    assert_eq!(candidate.baseline.map(|hand| hand.shanten), Some(1));
    assert_eq!(candidate.post_kan.map(|hand| hand.shanten), Some(0));
    assert_eq!(
        candidate.baseline.map(|hand| hand.acceptance_remaining),
        Some(8)
    );
    assert_eq!(
        candidate.post_kan.map(|hand| hand.acceptance_remaining),
        Some(3)
    );

    // 受け入れ枚数は減っているが、受け入れの劣化としては扱わない。
    assert!(
        candidate
            .acceptance_remaining_delta()
            .is_some_and(|delta| delta < 0)
    );
    assert_ne!(decision.reason, KanDecisionReason::AcceptanceRegresses);
    assert_eq!(
        decision.reason,
        KanDecisionReason::ShantenImprovedNotComparable
    );
    // 向聴が進んだことだけを理由に暗槓もしない。
    assert_eq!(decision.selected, None);
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

// 同じ向聴段階では受け入れの劣化を比較する。暗槓で面子の使い分けが消えて待ちが狭まる局面。
#[test]
fn ankan_is_rejected_when_acceptance_regresses_at_the_same_shanten() {
    let ctx = context(
        &ANKAN_ACCEPTANCE_REGRESSING_HAND,
        ANKAN_ACCEPTANCE_REGRESSING_DRAWN,
        [false; 4],
        Default::default(),
    );
    let actions: Vec<LegalAction> = ankan_dahai_actions(
        &ANKAN_ACCEPTANCE_REGRESSING_HAND,
        ANKAN_ACCEPTANCE_REGRESSING_DRAWN,
    )
    .into_iter()
    .chain([ankan_action(&ANKAN_ACCEPTANCE_REGRESSING_CONSUMED)])
    .collect();

    let decision = decide(&ctx, &actions, PushPullMode::Push).expect("暗槓候補");
    assert_eq!(decision.selected, None);
    assert_eq!(decision.reason, KanDecisionReason::AcceptanceRegresses);

    let candidate = decision.candidates.first().expect("候補");
    // 向聴段階は同じテンパイなので、受け入れをそのまま比較できる。
    assert_eq!(candidate.shanten_delta(), Some(0));
    assert_eq!(candidate.baseline.map(|hand| hand.shanten), Some(0));
    assert!(
        candidate
            .acceptance_remaining_delta()
            .is_some_and(|delta| delta < 0)
    );
    assert!(
        candidate
            .acceptance_type_delta()
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

// 自席を特定できない局面では、リーチ済みだともしていないとも推測せずどちらの policy へも
// 進まない。
// ---- 暗槓後の山の残りツモ可能枚数とリーチ合法性 ----

// リーチ前の安手テンパイ局面。Reach を合法手に含めるので、暗槓しない側の攻撃モードは Reach。
fn reach_mode_context(remaining_tiles: Option<u32>) -> GameContext {
    context(
        &ANKAN_REACH_HAND,
        ANKAN_REACH_DRAWN,
        [false; 4],
        Default::default(),
    )
    .with_table_state_facts(TableStateFacts {
        remaining_tiles,
        ..Default::default()
    })
}

fn reach_mode_actions() -> Vec<LegalAction> {
    ankan_dahai_actions(&ANKAN_REACH_HAND, ANKAN_REACH_DRAWN)
        .into_iter()
        .chain([LegalAction::Reach, ankan_action(&ANKAN_REACH_CONSUMED)])
        .collect()
}

// 暗槓後は嶺上牌を1枚引くので、その時点の残りツモ可能枚数は現在より1枚少ない。
#[test]
fn post_kan_remaining_tiles_reflects_the_replacement_draw() {
    let remaining = |tiles: Option<u32>| post_kan_remaining_tiles(&reach_mode_context(tiles));

    assert_eq!(
        remaining(Some(REACH_MIN_REMAINING_TILES + 1)),
        Some(REACH_MIN_REMAINING_TILES)
    );
    assert_eq!(
        remaining(Some(REACH_MIN_REMAINING_TILES)),
        Some(REACH_MIN_REMAINING_TILES - 1)
    );
    // 残り0枚からは引けない。unknown へ倒すとリーチ可能側になるので、0のまま残す。
    assert_eq!(remaining(Some(0)), Some(0));
    // 現在の枚数が分からない局面では推測しない。
    assert_eq!(remaining(None), None);
}

// 暗槓後のリーチ合法性は共有条件をそのまま通す。remaining tiles だけが境界を決める。
#[test]
fn post_kan_reach_legality_follows_the_shared_condition() {
    let legal = |tiles: Option<u32>| {
        let ctx = reach_mode_context(tiles);
        future_reach_legal(&ctx, Some(true), post_kan_remaining_tiles(&ctx))
    };

    assert!(legal(Some(REACH_MIN_REMAINING_TILES + 1)));
    assert!(!legal(Some(REACH_MIN_REMAINING_TILES)));
    assert!(!legal(Some(0)));
    // unknown は共有条件の unknown 規則のまま。
    assert!(legal(None));
}

// 残りちょうど REACH_MIN_REMAINING_TILES 枚では、暗槓後にリーチできない。暗槓しない側を Reach
// 手、暗槓後をダマ手として別尺度で比べることになるので、打点を比較せず暗槓しない。
#[test]
fn an_ankan_at_the_reach_wall_boundary_is_not_compared_as_a_reach_hand() {
    let ctx = reach_mode_context(Some(REACH_MIN_REMAINING_TILES));
    let actions = reach_mode_actions();

    let decision = decide(&ctx, &actions, PushPullMode::Push).expect("暗槓候補");
    assert_eq!(decision.selected, None);
    assert_eq!(decision.reason, KanDecisionReason::ValueNotEvaluable);

    let candidate = decision.candidates.first().expect("候補");
    assert_eq!(
        candidate.baseline.and_then(|hand| hand.offense_mode()),
        Some(TenpaiOffenseMode::Reach)
    );
    assert_eq!(
        candidate.post_kan.and_then(|hand| hand.offense_mode()),
        Some(TenpaiOffenseMode::Damaten)
    );
    // 速度は悪化していない。落ちたのは攻撃モードが食い違うため。
    assert_eq!(candidate.shanten_delta(), Some(0));
    assert_eq!(candidate.acceptance_remaining_delta(), Some(0));
}

// 1枚多ければ暗槓後も remaining tiles 条件を満たすので、remaining tiles を理由にモードが
// 食い違うことはない。
#[test]
fn an_ankan_one_tile_above_the_reach_wall_boundary_keeps_the_reach_mode() {
    let ctx = reach_mode_context(Some(REACH_MIN_REMAINING_TILES + 1));
    let actions = reach_mode_actions();

    let decision = decide(&ctx, &actions, PushPullMode::Push).expect("暗槓候補");
    let candidate = decision.candidates.first().expect("候補");
    assert_eq!(
        candidate.baseline.and_then(|hand| hand.offense_mode()),
        Some(TenpaiOffenseMode::Reach)
    );
    assert_eq!(
        candidate.post_kan.and_then(|hand| hand.offense_mode()),
        Some(TenpaiOffenseMode::Reach)
    );
    assert_ne!(decision.reason, KanDecisionReason::ValueNotEvaluable);
    assert_eq!(decision.selected, Some(ankan_action(&ANKAN_REACH_CONSUMED)));
}

// 山の残枚数が分からない局面では具体値を推測せず、既存の unknown semantics のままにする。
#[test]
fn an_unknown_wall_keeps_the_existing_unknown_semantics() {
    let ctx = reach_mode_context(None);
    let actions = reach_mode_actions();

    let decision = decide(&ctx, &actions, PushPullMode::Push).expect("暗槓候補");
    let candidate = decision.candidates.first().expect("候補");
    assert_eq!(
        candidate.baseline.and_then(|hand| hand.offense_mode()),
        Some(TenpaiOffenseMode::Reach)
    );
    assert_eq!(
        candidate.post_kan.and_then(|hand| hand.offense_mode()),
        Some(TenpaiOffenseMode::Reach)
    );
    assert_eq!(decision.selected, Some(ankan_action(&ANKAN_REACH_CONSUMED)));
}

// ---- 自己リーチ後の暫定 policy ----

// 自己リーチ後は、server が合法とした暗槓をそのまま採用する。
#[test]
fn a_legal_ankan_after_own_reach_is_selected() {
    let ctx = context(
        &ANKAN_FREE_HAND,
        ANKAN_FREE_DRAWN,
        [true, false, false, false],
        Default::default(),
    );
    let actions = vec![dahai(ANKAN_FREE_DRAWN), ankan_action(&ANKAN_FREE_CONSUMED)];

    let decision = decide(&ctx, &actions, PushPullMode::Push).expect("暗槓候補");
    assert_eq!(decision.selected, Some(ankan_action(&ANKAN_FREE_CONSUMED)));
    assert_eq!(
        decision.reason,
        KanDecisionReason::EligibleAnkanAfterOwnReach
    );

    let candidate = decision.candidates.first().expect("候補");
    assert!(candidate.selected);
    assert!(candidate.eligible);
    // structural validation だけを通す。向聴・受け入れ・打点の比較は行わないので埋めない。
    assert_eq!(candidate.current_fixed_meld_count.map(|c| c.get()), Some(0));
    assert_eq!(
        candidate.post_kan_fixed_meld_count.map(|c| c.get()),
        Some(1)
    );
    assert_eq!(candidate.baseline_discard, None);
    assert_eq!(candidate.baseline, None);
    assert_eq!(candidate.post_kan, None);
}

// 他家リーチも押し引きの Fold も、自己リーチ後の暗槓を落とす理由にしない。
#[test]
fn an_ankan_after_own_reach_ignores_the_opponent_reach_and_the_push_pull_verdict() {
    let ctx = context(
        &ANKAN_FREE_HAND,
        ANKAN_FREE_DRAWN,
        [true, true, false, false],
        Default::default(),
    );
    let actions = vec![dahai(ANKAN_FREE_DRAWN), ankan_action(&ANKAN_FREE_CONSUMED)];
    assert!(ctx.any_opponent_reached());

    for mode in [
        PushPullMode::Push,
        PushPullMode::Neutral,
        PushPullMode::Fold,
    ] {
        let decision = decide(&ctx, &actions, mode).expect("暗槓候補");
        assert_eq!(
            decision.selected,
            Some(ankan_action(&ANKAN_FREE_CONSUMED)),
            "mode: {mode:?}"
        );
        assert_eq!(
            decision.reason,
            KanDecisionReason::EligibleAnkanAfterOwnReach,
            "mode: {mode:?}"
        );
    }
}

// 打点を比較できない手牌でも、自己リーチ後は暗槓する。打点比較は自己リーチ前だけの条件。
#[test]
fn an_ankan_after_own_reach_does_not_require_a_comparable_offense_value() {
    let ctx = context(
        &ANKAN_IISHANTEN_HAND,
        ANKAN_IISHANTEN_DRAWN,
        [true, false, false, false],
        Default::default(),
    );
    let actions = vec![
        dahai(ANKAN_IISHANTEN_DRAWN),
        ankan_action(&ANKAN_IISHANTEN_CONSUMED),
    ];

    // 同じ手牌を自己リーチ前として評価すると、テンパイでないので打点を比較できない。
    let before_reach = context(
        &ANKAN_IISHANTEN_HAND,
        ANKAN_IISHANTEN_DRAWN,
        [false; 4],
        Default::default(),
    );
    assert_eq!(
        decide(&before_reach, &actions, PushPullMode::Push)
            .expect("暗槓候補")
            .reason,
        KanDecisionReason::ValueNotEvaluable
    );

    let decision = decide(&ctx, &actions, PushPullMode::Push).expect("暗槓候補");
    assert_eq!(
        decision.selected,
        Some(ankan_action(&ANKAN_IISHANTEN_CONSUMED))
    );
    assert_eq!(
        decision.reason,
        KanDecisionReason::EligibleAnkanAfterOwnReach
    );
}

// 自己リーチ後でも structural validation は通す。合法手として渡された暗槓を手牌から
// 組み立てられない場合は選ばない。
#[test]
fn an_ankan_after_own_reach_still_needs_a_valid_meld() {
    let ctx = context(
        &ANKAN_FREE_HAND,
        ANKAN_FREE_DRAWN,
        [true, false, false, false],
        Default::default(),
    );

    // 手牌に無い 南 (112..115) の暗槓。
    let not_held = vec![ankan_action(&[112, 113, 114, 115]), LegalAction::None];
    assert_eq!(
        decide(&ctx, &not_held, PushPullMode::Push)
            .expect("暗槓候補")
            .reason,
        KanDecisionReason::InvalidConsumed
    );

    // カンの形にならない3枚。
    let malformed = vec![ankan_action(&[108, 109, 110]), LegalAction::None];
    assert_eq!(
        decide(&ctx, &malformed, PushPullMode::Push)
            .expect("暗槓候補")
            .reason,
        KanDecisionReason::InvalidConsumed
    );
}

// 自己リーチ後は加槓しない。大明槓は production へ接続していない。
#[test]
fn kakan_and_daiminkan_are_not_taken_after_own_reach() {
    let ctx = context(
        &ANKAN_FREE_HAND,
        ANKAN_FREE_DRAWN,
        [true, false, false, false],
        Default::default(),
    );
    let actions = vec![kakan(), daiminkan(), LegalAction::None];

    let decision = decide(&ctx, &actions, PushPullMode::Push).expect("カン候補");
    assert_eq!(decision.selected, None);
    assert_eq!(
        decision
            .candidates
            .iter()
            .map(|candidate| candidate.reason)
            .collect::<Vec<_>>(),
        vec![
            KanDecisionReason::KakanAfterOwnReach,
            KanDecisionReason::DaiminkanNotConnected,
        ]
    );
}

// 自己リーチ後でも、成立した候補が2件以上あれば合法 action の列挙順で選ばない。
//
// RiichiLab (riichienv-core) の legal action 生成はリーチ後の暗槓をツモ牌の牌種1件だけに
// 限定するので、実戦でこの局面は出ない。それでも順序依存を残さないため規則として固定する。
#[test]
fn several_eligible_ankan_after_own_reach_are_not_broken_by_the_legal_action_order() {
    let ctx = context(
        &ANKAN_FREE_HAND,
        ANKAN_FREE_DRAWN,
        [true, false, false, false],
        Default::default(),
    );
    let actions = vec![
        dahai(ANKAN_FREE_DRAWN),
        ankan_action(&ANKAN_FREE_CONSUMED),
        ankan_action(&[111, 110, 109, 108]),
    ];

    let decision = decide(&ctx, &actions, PushPullMode::Push).expect("暗槓候補");
    assert_eq!(decision.candidates.len(), 2);
    assert!(
        decision
            .candidates
            .iter()
            .all(|candidate| candidate.eligible)
    );
    assert_eq!(decision.selected, None);
    assert_eq!(
        decision.reason,
        KanDecisionReason::MultipleEligibleCandidates
    );
}

// 自己リーチ後の暫定 policy は山の残枚数を見ない。境界の枚数でも結論は変わらない。
#[test]
fn an_ankan_after_own_reach_ignores_the_reach_wall_boundary() {
    let ctx = context(
        &ANKAN_FREE_HAND,
        ANKAN_FREE_DRAWN,
        [true, false, false, false],
        Default::default(),
    )
    .with_table_state_facts(TableStateFacts {
        remaining_tiles: Some(REACH_MIN_REMAINING_TILES),
        ..Default::default()
    });
    let actions = vec![dahai(ANKAN_FREE_DRAWN), ankan_action(&ANKAN_FREE_CONSUMED)];

    let decision = decide(&ctx, &actions, PushPullMode::Push).expect("暗槓候補");
    assert_eq!(decision.selected, Some(ankan_action(&ANKAN_FREE_CONSUMED)));
    assert_eq!(
        decision.reason,
        KanDecisionReason::EligibleAnkanAfterOwnReach
    );
    // 打点の比較そのものを行わないので、両 side の値は埋めない。
    let candidate = decision.candidates.first().expect("候補");
    assert_eq!(candidate.baseline, None);
    assert_eq!(candidate.post_kan, None);
}

#[test]
fn ankan_is_rejected_when_own_reach_is_unknown() {
    let ctx = ankan_context(
        &ANKAN_FREE_HAND,
        ANKAN_FREE_DRAWN,
        [false; 4],
        Default::default(),
        None,
    );
    let actions = ankan_free_actions();
    assert_eq!(ctx.own_reached(), None);

    let decision = decide(&ctx, &actions, PushPullMode::Push).expect("暗槓候補");
    assert_eq!(decision.selected, None);
    assert_eq!(decision.reason, KanDecisionReason::OwnReachUnknown);

    // 自己リーチ後の暫定 policy へ進まないので、structural validation の値も埋めない。
    let candidate = decision.candidates.first().expect("候補");
    assert_eq!(candidate.current_fixed_meld_count, None);
    assert_eq!(candidate.post_kan_fixed_meld_count, None);
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

// 対応する Pon が無い加槓は、合法手として渡されても採用しない。
#[test]
fn kakan_without_a_matching_pon_is_rejected() {
    let ctx = context(
        &ANKAN_FREE_HAND,
        ANKAN_FREE_DRAWN,
        [false; 4],
        Default::default(),
    );
    let actions = vec![kakan(), LegalAction::None];

    let decision = decide(&ctx, &actions, PushPullMode::Push).expect("カン候補");
    assert_eq!(decision.selected, None);
    assert_eq!(decision.reason, KanDecisionReason::KakanWithoutMatchingPon);
    assert_eq!(decision.candidates[0].kind, KanKind::Kakan);
    assert!(KanKind::Kakan.is_production_connected());
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
fn only_daiminkan_stays_unconnected() {
    assert!(KanKind::Ankan.is_production_connected());
    assert!(KanKind::Kakan.is_production_connected());
    assert!(!KanKind::Daiminkan.is_production_connected());
    assert_eq!(KanKind::Ankan.meld_kind(), MeldKind::Ankan);
    assert_eq!(KanKind::Kakan.meld_kind(), MeldKind::Kakan);
    assert_eq!(KanKind::Daiminkan.meld_kind(), MeldKind::Daiminkan);
}

// 打点比較そのものの semantics。カン前後で同じ尺度の値を確定できない組み合わせはすべて
// 評価不能にし、速度非劣化だけでカンへ倒さない。暗槓と加槓で同じ規則を共有する。
#[test]
fn the_offense_value_comparison_requires_the_same_scale_on_both_sides() {
    let reach = TenpaiOffenseMode::Reach;
    let damaten = TenpaiOffenseMode::Damaten;

    // 打点が上がる / 変わらない場合だけ成立する。
    assert_eq!(
        compare_offense_value(
            hand(0, 4, offense(damaten, 8000)),
            hand(0, 4, offense(damaten, 12000))
        ),
        Ok(())
    );
    assert_eq!(
        compare_offense_value(
            hand(0, 4, offense(damaten, 8000)),
            hand(0, 4, offense(damaten, 8000))
        ),
        Ok(())
    );
    assert_eq!(
        compare_offense_value(
            hand(0, 4, offense(damaten, 12000)),
            hand(0, 4, offense(damaten, 8000))
        ),
        Err(KanDecisionReason::ValueRegresses)
    );

    // リーチ手とダマ手の打点は別 baseline の値なので比較しない。
    assert_eq!(
        compare_offense_value(
            hand(0, 4, offense(reach, 8000)),
            hand(0, 4, offense(damaten, 12000))
        ),
        Err(KanDecisionReason::ValueNotEvaluable)
    );
    // 攻撃モードを確定できない場合も比較しない。
    assert_eq!(
        compare_offense_value(
            hand(0, 4, offense(TenpaiOffenseMode::Unknown, 8000)),
            hand(0, 4, offense(TenpaiOffenseMode::Unknown, 12000))
        ),
        Err(KanDecisionReason::ValueNotEvaluable)
    );
    // 役なし・ロン不可・点数計算の入力不足で打点を確定できない side があれば比較しない。
    assert_eq!(
        compare_offense_value(
            hand(0, 4, unknown_offense(damaten)),
            hand(0, 4, offense(damaten, 12000))
        ),
        Err(KanDecisionReason::ValueNotEvaluable)
    );
    assert_eq!(
        compare_offense_value(
            hand(0, 4, offense(damaten, 8000)),
            hand(0, 4, unknown_offense(damaten))
        ),
        Err(KanDecisionReason::ValueNotEvaluable)
    );
    // テンパイでない side は打点そのものを持たない。
    assert_eq!(
        compare_offense_value(hand(1, 8, None), hand(1, 8, offense(damaten, 12000))),
        Err(KanDecisionReason::ValueNotEvaluable)
    );
    assert_eq!(
        compare_offense_value(hand(1, 8, offense(damaten, 8000)), hand(1, 8, None)),
        Err(KanDecisionReason::ValueNotEvaluable)
    );
}

// 成立した候補が2件以上ある場合は、合法 action の列挙順を tie-break にせずどれも選ばない。
#[test]
fn several_eligible_ankan_candidates_are_not_broken_by_the_legal_action_order() {
    let ctx = context(
        &ANKAN_FREE_HAND,
        ANKAN_FREE_DRAWN,
        [false; 4],
        Default::default(),
    );
    // 同じ4枚を別の並びで消費する2件。どちらも同じ暗槓後 state になるので両方成立する。
    let first = ankan_action(&ANKAN_FREE_CONSUMED);
    let second = ankan_action(&[111, 110, 109, 108]);
    let actions: Vec<LegalAction> = ankan_dahai_actions(&ANKAN_FREE_HAND, ANKAN_FREE_DRAWN)
        .into_iter()
        .chain([first.clone(), second.clone()])
        .collect();

    let decision = decide(&ctx, &actions, PushPullMode::Push).expect("暗槓候補");
    assert_eq!(
        decision.candidates.len(),
        2,
        "candidates: {:?}",
        decision.candidates
    );
    assert!(
        decision
            .candidates
            .iter()
            .all(|candidate| candidate.eligible)
    );
    // 先頭の候補を採らない。
    assert_eq!(decision.selected, None);
    assert!(
        decision
            .candidates
            .iter()
            .all(|candidate| !candidate.selected)
    );
    assert_eq!(
        decision.reason,
        KanDecisionReason::MultipleEligibleCandidates
    );
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
        KanDecisionReason::KakanWithoutMatchingPon
    );
    assert_eq!(
        decision.candidates[1].reason,
        KanDecisionReason::DaiminkanNotConnected
    );
    assert_eq!(
        decision.candidates[2].reason,
        KanDecisionReason::EligibleAnkanNoRegression
    );
    // 成立したのは列挙順の最後の候補。採用は eligible かどうかで決め、合法 action の順序では
    // 決めない。
    assert_eq!(decision.selected, Some(ankan_action(&ANKAN_FREE_CONSUMED)));
    assert!(decision.candidates[2].selected);
    assert_eq!(
        decision.reason,
        KanDecisionReason::EligibleAnkanNoRegression
    );
}

#[test]
fn eligible_reason_is_only_the_kan_verdict() {
    assert!(KanDecisionReason::EligibleAnkanNoRegression.is_eligible());
    assert!(KanDecisionReason::EligibleAnkanAfterOwnReach.is_eligible());
    assert!(KanDecisionReason::EligibleKakanNoRegression.is_eligible());
    for reason in [
        KanDecisionReason::DaiminkanNotConnected,
        KanDecisionReason::KakanAfterOwnReach,
        KanDecisionReason::KakanWithoutMatchingPon,
        KanDecisionReason::InvalidKakanShape,
        KanDecisionReason::KakanChankanNotHardSafe,
        KanDecisionReason::OwnReachUnknown,
        KanDecisionReason::OpponentReached,
        KanDecisionReason::NotPush,
        KanDecisionReason::NotAfterOwnDraw,
        KanDecisionReason::FixedMeldCountUnknown,
        KanDecisionReason::FixedMeldCountOverflow,
        KanDecisionReason::InvalidConsumed,
        KanDecisionReason::NormalDiscardUnavailable,
        KanDecisionReason::ShantenRegresses,
        KanDecisionReason::ShantenImprovedNotComparable,
        KanDecisionReason::AcceptanceRegresses,
        KanDecisionReason::ValueNotEvaluable,
        KanDecisionReason::ValueRegresses,
        KanDecisionReason::MultipleEligibleCandidates,
    ] {
        assert!(!reason.is_eligible(), "reason: {reason:?}");
    }
}

// ---- 加槓 (Kakan) ----

fn kakan_free_context(discards: [Vec<u8>; 4]) -> GameContext {
    kakan_context(
        &KAKAN_FREE_HAND,
        KAKAN_FREE_DRAWN,
        [false; 4],
        [vec![east_pon_meld()], vec![], vec![], vec![]],
        Some(0),
        discards,
    )
}

// 加槓牌が全他家の河にある局面。搶槓 hard-safe を満たす。
fn kakan_free_hard_safe_context() -> GameContext {
    kakan_free_context(rivers_with_tile_for_all_opponents([108, 109, 110]))
}

fn kakan_free_actions() -> Vec<LegalAction> {
    kakan_dahai_actions(&KAKAN_FREE_HAND, KAKAN_FREE_DRAWN)
        .into_iter()
        .chain([kakan_action(KAKAN_FREE_DRAWN, &KAKAN_FREE_CONSUMED)])
        .collect()
}

fn kakan_dahai_actions(hand: &[u8], drawn: u8) -> Vec<LegalAction> {
    hand.iter()
        .chain(std::iter::once(&drawn))
        .map(|&value| dahai(value))
        .collect()
}

// structural validation だけを単体で通すための候補。
fn structure_candidate() -> KanCandidateDiagnostic {
    new_kan_candidate(&LegalAction::None, KanKind::Kakan, None, &[])
}

// ---- Pon → Kakan の post-state ----

// 加槓は固定面子の追加ではなく置換なので、元の Pon は消えて同じ位置を Kakan が埋める。
// 副露済み面子数は前後で変わらない。
#[test]
fn a_kakan_replaces_the_matching_pon_in_place() {
    let other_pon = Meld::new(MeldKind::Pon, tiles(&[112, 113, 114]), Some(tile(112)));
    let ctx = kakan_context(
        &KAKAN_FREE_HAND,
        KAKAN_FREE_DRAWN,
        [false; 4],
        [
            vec![other_pon.clone(), east_pon_meld()],
            vec![],
            vec![],
            vec![],
        ],
        Some(0),
        Default::default(),
    );
    let mut candidate = structure_candidate();

    let (added, melds, concealed, fixed_meld_count) = validate_kakan_structure(
        &ctx,
        Some(tile(KAKAN_FREE_DRAWN)),
        &tiles(&KAKAN_FREE_CONSUMED),
        &mut candidate,
    )
    .expect("加槓の形が成り立つ");

    assert_eq!(added, tile(KAKAN_FREE_DRAWN));
    // 元の Pon は消え、同じ位置が Kakan になる。無関係な副露は動かない。
    assert_eq!(melds.len(), 2);
    assert_eq!(melds[0], other_pon);
    assert_eq!(melds[1].kind(), MeldKind::Kakan);
    // Kakan の4枚は元 Pon の3枚 + 追加牌1枚。
    assert_eq!(
        melds[1].tiles(),
        tiles(&[108, 109, 110, KAKAN_FREE_DRAWN]).as_slice()
    );
    assert!(!melds.iter().any(|meld| *meld == east_pon_meld()));

    // 副露済み面子数は前後で変わらない。
    assert_eq!(fixed_meld_count.get(), 2);
    assert_eq!(
        candidate.current_fixed_meld_count.map(|count| count.get()),
        Some(2)
    );
    assert_eq!(
        candidate.post_kan_fixed_meld_count.map(|count| count.get()),
        Some(2)
    );
    assert_eq!(candidate.matching_pon, Some(east_pon_meld()));
    assert_eq!(concealed.len(), KAKAN_FREE_HAND.len());
}

// 加槓の called_tile は元 Pon で鳴いた牌のままにする。LegalAction::Kakan の tile は追加する
// 4枚目であって、Kakan 面子の called_tile ではない。
#[test]
fn a_kakan_keeps_the_called_tile_of_the_original_pon() {
    let ctx = kakan_free_hard_safe_context();
    let mut candidate = structure_candidate();

    let (_, melds, _, _) = validate_kakan_structure(
        &ctx,
        Some(tile(KAKAN_FREE_DRAWN)),
        &tiles(&KAKAN_FREE_CONSUMED),
        &mut candidate,
    )
    .expect("加槓の形が成り立つ");

    assert_eq!(melds[0].called_tile(), east_pon_meld().called_tile());
    assert_eq!(melds[0].called_tile(), Some(tile(108)));
    assert_ne!(melds[0].called_tile(), Some(tile(KAKAN_FREE_DRAWN)));
}

// concealed hand からは追加牌1枚だけを取り除く。
#[test]
fn a_kakan_removes_only_the_added_tile_from_the_concealed_hand() {
    let ctx = kakan_free_hard_safe_context();
    let mut candidate = structure_candidate();

    let (_, _, concealed, _) = validate_kakan_structure(
        &ctx,
        Some(tile(KAKAN_FREE_DRAWN)),
        &tiles(&KAKAN_FREE_CONSUMED),
        &mut candidate,
    )
    .expect("加槓の形が成り立つ");

    // 追加牌はツモ牌なので、手牌13枚相当はそのまま残る。
    assert_eq!(concealed, tiles(&KAKAN_FREE_HAND));
    assert!(!concealed.contains(&tile(KAKAN_FREE_DRAWN)));
}

// 追加牌を取り除くときは赤5と黒5を区別する。牌種だけで一致させると赤ドラを取り違える。
#[test]
fn a_kakan_distinguishes_the_red_five_from_the_black_five() {
    // 5p の Pon (52 は赤5)。手牌に黒5p (54) と赤ではない 5p (55) が残っている状態は作れない
    // ので、Pon を黒3枚にして赤5p (52) と黒5p (55) を手牌へ置く。
    let pon = Meld::new(MeldKind::Pon, tiles(&[53, 54, 55]), Some(tile(53)));
    let hand = [52u8, 0, 4, 8, 12, 17, 20, 24, 28, 32];
    let ctx = kakan_context(
        &hand,
        36,
        [false; 4],
        [vec![pon], vec![], vec![], vec![]],
        Some(0),
        Default::default(),
    );

    // 赤5p を加える指定では赤5p だけが手牌から消える。
    let mut candidate = structure_candidate();
    let (added, melds, concealed, _) =
        validate_kakan_structure(&ctx, Some(tile(52)), &tiles(&[53, 54, 55]), &mut candidate)
            .expect("赤5の加槓");
    assert!(added.is_red());
    assert!(melds[0].tiles().contains(&tile(52)));
    assert!(!concealed.contains(&tile(52)));

    // 黒5p を加える指定は、手牌に黒5p が無いので形が成り立たない。赤5p で代用しない。
    let mut candidate = structure_candidate();
    assert_eq!(
        validate_kakan_structure(&ctx, Some(tile(56)), &tiles(&[53, 54, 55]), &mut candidate),
        Err(KanDecisionReason::InvalidKakanShape)
    );
}

// RiichiLab の mjai → TileId 変換は黒牌の物理 copy ID を復元できないので、consumed の物理牌が
// 既存 Pon と完全一致することを条件にしない。牌種 semantics だけを見る。
#[test]
fn a_kakan_does_not_require_the_consumed_physical_tile_ids_to_match_the_pon() {
    let ctx = kakan_free_hard_safe_context();
    let mut candidate = structure_candidate();

    // 東の代表 ID (108) を3枚並べた consumed。既存 Pon の物理牌 (108, 109, 110) とは一致しない。
    let (_, melds, _, _) = validate_kakan_structure(
        &ctx,
        Some(tile(KAKAN_FREE_DRAWN)),
        &tiles(&[108, 108, 108]),
        &mut candidate,
    )
    .expect("牌種が揃っていれば加槓の形として扱う");

    // 置換後の Kakan が持つのは既存 Pon の物理牌で、consumed の表現をそのまま採らない。
    assert_eq!(
        melds[0].tiles(),
        tiles(&[108, 109, 110, KAKAN_FREE_DRAWN]).as_slice()
    );
}

// 対応する Pon が無い加槓は、合法手として渡されても組み立てない。
#[test]
fn a_kakan_without_a_matching_pon_is_not_built() {
    let ctx = kakan_context(
        &KAKAN_FREE_HAND,
        KAKAN_FREE_DRAWN,
        [false; 4],
        // 東の Pon ではなく南の Pon を持つ。
        [
            vec![Meld::new(
                MeldKind::Pon,
                tiles(&[112, 113, 114]),
                Some(tile(112)),
            )],
            vec![],
            vec![],
            vec![],
        ],
        Some(0),
        Default::default(),
    );
    let mut candidate = structure_candidate();

    assert_eq!(
        validate_kakan_structure(
            &ctx,
            Some(tile(KAKAN_FREE_DRAWN)),
            &tiles(&KAKAN_FREE_CONSUMED),
            &mut candidate
        ),
        Err(KanDecisionReason::KakanWithoutMatchingPon)
    );
    assert_eq!(candidate.matching_pon, None);
}

// 暗槓を Kakan として渡すような、形が成り立たない指定は組み立てない。
#[test]
fn a_malformed_kakan_is_not_built() {
    let ctx = kakan_free_hard_safe_context();

    // consumed が3枚でない。
    let mut candidate = structure_candidate();
    assert_eq!(
        validate_kakan_structure(
            &ctx,
            Some(tile(KAKAN_FREE_DRAWN)),
            &tiles(&[108, 109]),
            &mut candidate
        ),
        Err(KanDecisionReason::InvalidKakanShape)
    );

    // consumed の牌種が揃っていない。
    let mut candidate = structure_candidate();
    assert_eq!(
        validate_kakan_structure(
            &ctx,
            Some(tile(KAKAN_FREE_DRAWN)),
            &tiles(&[108, 109, 112]),
            &mut candidate
        ),
        Err(KanDecisionReason::InvalidKakanShape)
    );

    // 追加牌が手牌にもツモ牌にも無い。4枚目の東を持たない局面で東の加槓を渡す。
    let without_added_tile = kakan_context(
        &KAKAN_FREE_HAND,
        40,
        [false; 4],
        [vec![east_pon_meld()], vec![], vec![], vec![]],
        Some(0),
        Default::default(),
    );
    let mut candidate = structure_candidate();
    assert_eq!(
        validate_kakan_structure(
            &without_added_tile,
            Some(tile(KAKAN_FREE_DRAWN)),
            &tiles(&KAKAN_FREE_CONSUMED),
            &mut candidate
        ),
        Err(KanDecisionReason::InvalidKakanShape)
    );
    // 元 Pon は特定できているので、落ちたのは追加牌を手牌から取り除けないためである。
    assert_eq!(candidate.matching_pon, Some(east_pon_meld()));
}

// ---- 搶槓 hard-safe ----

// 加槓牌が全3家それぞれ自身の河にある場合だけ、搶槓されないと確定して加槓できる。
#[test]
fn a_kakan_is_selected_when_every_opponent_discarded_the_added_tile() {
    let ctx = kakan_free_hard_safe_context();
    let actions = kakan_free_actions();

    let decision = decide(&ctx, &actions, PushPullMode::Push).expect("加槓候補");
    assert_eq!(
        decision.selected,
        Some(kakan_action(KAKAN_FREE_DRAWN, &KAKAN_FREE_CONSUMED))
    );
    assert_eq!(
        decision.reason,
        KanDecisionReason::EligibleKakanNoRegression
    );

    let candidate = decision.candidates.first().expect("候補");
    assert!(candidate.selected);
    assert!(candidate.eligible);
    assert_eq!(candidate.kind, KanKind::Kakan);
    // tile は追加する4枚目。
    assert_eq!(candidate.tile, TileType::from_mjai_type_str("E").ok());
    assert_eq!(candidate.matching_pon, Some(east_pon_meld()));
    // 加槓は Pon の置換なので副露済み面子数は変わらない。
    assert_eq!(
        candidate.current_fixed_meld_count.map(|count| count.get()),
        Some(1)
    );
    assert_eq!(
        candidate.post_kan_fixed_meld_count.map(|count| count.get()),
        Some(1)
    );

    let chankan = candidate.chankan.as_ref().expect("搶槓判定");
    assert!(chankan.hard_safe);
    assert_eq!(
        chankan
            .opponents
            .iter()
            .map(|opponent| (opponent.player, opponent.discarded))
            .collect::<Vec<_>>(),
        vec![(1, true), (2, true), (3, true)]
    );

    // 速度は変わらず、打点は明槓ぶんの符で下がらない。
    assert_eq!(candidate.shanten_delta(), Some(0));
    assert_eq!(candidate.acceptance_remaining_delta(), Some(0));
    assert_eq!(candidate.acceptance_type_delta(), Some(0));
    assert!(
        candidate
            .weighted_total_delta()
            .is_some_and(|delta| delta >= 0),
        "candidate: {candidate:?}"
    );
}

// 1人でも自身の河に加槓牌が無ければ搶槓ロン不能を確定できないので加槓しない。
#[test]
fn a_kakan_is_rejected_when_one_opponent_has_not_discarded_the_added_tile() {
    for missing in 1..=3 {
        let mut discards = rivers_with_tile_for_all_opponents([108, 109, 110]);
        discards[missing] = vec![];
        let ctx = kakan_free_context(discards);
        let actions = kakan_free_actions();

        let decision = decide(&ctx, &actions, PushPullMode::Push).expect("加槓候補");
        assert_eq!(decision.selected, None, "missing: {missing}");
        assert_eq!(
            decision.reason,
            KanDecisionReason::KakanChankanNotHardSafe,
            "missing: {missing}"
        );

        let candidate = decision.candidates.first().expect("候補");
        let chankan = candidate.chankan.as_ref().expect("搶槓判定");
        assert!(!chankan.hard_safe);
        assert_eq!(
            chankan
                .opponents
                .iter()
                .find(|opponent| opponent.player == missing)
                .map(|opponent| opponent.discarded),
            Some(false)
        );
        // hard-safe で落ちた候補は、以降の判断材料を推測で埋めない。
        assert_eq!(candidate.baseline, None);
        assert_eq!(candidate.post_kan, None);
    }
}

// 一時通過牌は「今はロンできない」だけで、搶槓で新しく役が付く手を排除できない。hard-safe の
// 根拠に流用しない。
#[test]
fn temporary_passed_tiles_alone_are_not_chankan_hard_safe() {
    let east = TileType::from_mjai_type_str("E").expect("東");
    let ctx = kakan_free_context(Default::default()).with_temporary_passed_tiles(Some([
        vec![],
        vec![east],
        vec![east],
        vec![east],
    ]));
    let actions = kakan_free_actions();

    assert!(ctx.is_temporary_passed(east, 1));
    let decision = decide(&ctx, &actions, PushPullMode::Push).expect("加槓候補");
    assert_eq!(decision.selected, None);
    assert_eq!(decision.reason, KanDecisionReason::KakanChankanNotHardSafe);
}

// 同一手牌中の通過牌も観測事実であって hard fact ではないので、hard-safe の根拠にしない。
#[test]
fn same_hand_passed_tiles_alone_are_not_chankan_hard_safe() {
    let east = TileType::from_mjai_type_str("E").expect("東");
    let ctx = kakan_free_context(Default::default()).with_same_hand_passed_tiles(Some([
        vec![],
        vec![east],
        vec![east],
        vec![east],
    ]));
    let actions = kakan_free_actions();

    assert!(ctx.is_same_hand_passed(east, 1));
    let decision = decide(&ctx, &actions, PushPullMode::Push).expect("加槓候補");
    assert_eq!(decision.selected, None);
    assert_eq!(decision.reason, KanDecisionReason::KakanChankanNotHardSafe);
}

// 通常打牌なら誰にもロンされ得ない牌でも、自身の河という hard fact が無ければ加槓しない。
//
// この局面の東は Pon の3枚と手牌の1枚で4枚すべてが自分のもとにあり、他家は1枚も持てないので
// 通常 Dahai の exact ron 評価は誰に対しても0になる。それでも v1 は通常打牌用の評価を搶槓へ
// 流用せず、河の hard fact だけを根拠にする。
#[test]
fn a_tile_no_opponent_can_hold_is_not_chankan_hard_safe_without_the_river_fact() {
    let ctx = kakan_free_context(Default::default());
    let actions = kakan_free_actions();

    // 東4枚は Pon 3枚 + 手牌1枚で、他家の手に入り得ない。
    let east = TileType::from_mjai_type_str("E").expect("東");
    assert_eq!(
        east_pon_meld()
            .tiles()
            .iter()
            .chain(std::iter::once(&tile(KAKAN_FREE_DRAWN)))
            .filter(|tile| tile.tile_type() == east)
            .count(),
        4
    );
    assert!(!ctx.any_opponent_reached());
    assert!((1..=3).all(|player| ctx.melds_of(player).is_some_and(|melds| melds.is_empty())));

    let decision = decide(&ctx, &actions, PushPullMode::Push).expect("加槓候補");
    assert_eq!(decision.selected, None);
    assert_eq!(decision.reason, KanDecisionReason::KakanChankanNotHardSafe);
}

// 他家リーチ中は加槓しない。新しい槓ドラ・一発消去・搶槓 risk・嶺上牌を同じ尺度で比べられない。
#[test]
fn a_kakan_is_rejected_under_opponent_reach() {
    let ctx = kakan_context(
        &KAKAN_FREE_HAND,
        KAKAN_FREE_DRAWN,
        [false, true, false, false],
        [vec![east_pon_meld()], vec![], vec![], vec![]],
        Some(0),
        rivers_with_tile_for_all_opponents([108, 109, 110]),
    );
    let actions = kakan_free_actions();

    let decision = decide(&ctx, &actions, PushPullMode::Push).expect("加槓候補");
    assert_eq!(decision.selected, None);
    assert_eq!(decision.reason, KanDecisionReason::OpponentReached);

    // 搶槓 hard-safe を満たす局面でも、他家リーチが先に落とす。
    let candidate = decision.candidates.first().expect("候補");
    assert_eq!(candidate.chankan, None);
    assert_eq!(candidate.matching_pon, None);
}

// Push 以外では加槓しない。搶槓 hard-safe とは別 gate で、「加槓牌が安全だから Fold でも
// 加槓する」という拡張は行わない。
#[test]
fn a_kakan_is_rejected_outside_push() {
    let ctx = kakan_free_hard_safe_context();
    let actions = kakan_free_actions();

    for mode in [PushPullMode::Neutral, PushPullMode::Fold] {
        let decision = decide(&ctx, &actions, mode).expect("加槓候補");
        assert_eq!(decision.selected, None, "mode: {mode:?}");
        assert_eq!(
            decision.reason,
            KanDecisionReason::NotPush,
            "mode: {mode:?}"
        );
    }
}

// 自己リーチ後は加槓しない。server / context が矛盾していても自己リーチ状態を推測しない。
#[test]
fn a_kakan_is_rejected_after_own_reach() {
    let ctx = kakan_context(
        &KAKAN_FREE_HAND,
        KAKAN_FREE_DRAWN,
        [true, false, false, false],
        [vec![east_pon_meld()], vec![], vec![], vec![]],
        Some(0),
        rivers_with_tile_for_all_opponents([108, 109, 110]),
    );
    let actions = kakan_free_actions();

    let decision = decide(&ctx, &actions, PushPullMode::Push).expect("加槓候補");
    assert_eq!(decision.selected, None);
    assert_eq!(decision.reason, KanDecisionReason::KakanAfterOwnReach);
}

// 自席を特定できない局面では、リーチ済みかどうかを推測せず加槓しない。
#[test]
fn a_kakan_is_rejected_when_own_reach_is_unknown() {
    let ctx = kakan_context(
        &KAKAN_FREE_HAND,
        KAKAN_FREE_DRAWN,
        [false; 4],
        [vec![east_pon_meld()], vec![], vec![], vec![]],
        None,
        rivers_with_tile_for_all_opponents([108, 109, 110]),
    );
    let actions = kakan_free_actions();

    assert_eq!(ctx.own_reached(), None);
    let decision = decide(&ctx, &actions, PushPullMode::Push).expect("加槓候補");
    assert_eq!(decision.selected, None);
    assert_eq!(decision.reason, KanDecisionReason::OwnReachUnknown);
}

// ---- 速度と打点の比較 ----

// 4枚目を加槓すると搭子が崩れる局面では、搶槓 hard-safe でも加槓しない。
#[test]
fn a_kakan_is_rejected_when_shanten_regresses() {
    let ctx = kakan_context(
        &KAKAN_SHANTEN_REGRESSING_HAND,
        KAKAN_SHANTEN_REGRESSING_DRAWN,
        [false; 4],
        [vec![two_man_pon_meld()], vec![], vec![], vec![]],
        Some(0),
        rivers_with_tile_for_all_opponents([4, 5, 6]),
    );
    let actions: Vec<LegalAction> = kakan_dahai_actions(
        &KAKAN_SHANTEN_REGRESSING_HAND,
        KAKAN_SHANTEN_REGRESSING_DRAWN,
    )
    .into_iter()
    .chain([kakan_action(
        KAKAN_SHANTEN_REGRESSING_ADDED,
        &KAKAN_SHANTEN_REGRESSING_CONSUMED,
    )])
    .collect();

    let decision = decide(&ctx, &actions, PushPullMode::Push).expect("加槓候補");
    assert_eq!(decision.selected, None);
    assert_eq!(decision.reason, KanDecisionReason::ShantenRegresses);

    let candidate = decision.candidates.first().expect("候補");
    assert!(
        candidate
            .chankan
            .as_ref()
            .is_some_and(|chankan| chankan.hard_safe)
    );
    assert_eq!(candidate.baseline.map(|hand| hand.shanten), Some(0));
    assert_eq!(candidate.post_kan.map(|hand| hand.shanten), Some(1));
}

// 向聴が進む加槓も今回は採用しない。向聴段階が違う受け入れ・打点を同じ尺度で比べないため。
#[test]
fn an_improved_shanten_does_not_make_the_kakan_eligible() {
    let ctx = kakan_free_hard_safe_context();
    let actions = kakan_free_actions();

    // 加槓しない側を「1向聴・受け入れ8枚2種」にした比較。加槓後はテンパイのままになる。
    let mut iishanten = baseline(&ctx, &actions).expect("通常打牌評価");
    iishanten.shanten_after_discard = EffectiveShanten::Melded { standard: 1 };
    iishanten.acceptance_after_discard.current = EffectiveShanten::Melded { standard: 1 };
    iishanten
        .acceptance_after_discard
        .tiles
        .push(AcceptanceTile {
            tile: TileType::new(17).unwrap(),
            remaining: 5,
            shanten_after_draw: EffectiveShanten::Melded { standard: 0 },
        });

    let decision = evaluate_kan_decision(&ctx, &actions, PushPullMode::Push, Some(&iishanten))
        .expect("加槓候補");
    assert_eq!(decision.selected, None);
    assert_eq!(
        decision.reason,
        KanDecisionReason::ShantenImprovedNotComparable
    );

    let candidate = decision.candidates.first().expect("候補");
    assert!(candidate.shanten_delta().is_some_and(|delta| delta < 0));
    // 受け入れ枚数は減っているが、向聴段階が違うので受け入れの劣化としては扱わない。
    assert!(
        candidate
            .acceptance_remaining_delta()
            .is_some_and(|delta| delta < 0)
    );
    assert_ne!(decision.reason, KanDecisionReason::AcceptanceRegresses);
}

// 向聴が同じで受け入れだけが減る加槓は採用しない。
#[test]
fn a_kakan_is_rejected_when_acceptance_regresses_at_the_same_shanten() {
    let ctx = kakan_free_hard_safe_context();
    let actions = kakan_free_actions();

    // 加槓しない側の待ちを広げた比較。向聴段階は同じテンパイのまま。
    let mut wider = baseline(&ctx, &actions).expect("通常打牌評価");
    wider.acceptance_after_discard.tiles.push(AcceptanceTile {
        tile: TileType::new(17).unwrap(),
        remaining: 4,
        shanten_after_draw: EffectiveShanten::Melded { standard: -1 },
    });

    let decision =
        evaluate_kan_decision(&ctx, &actions, PushPullMode::Push, Some(&wider)).expect("加槓候補");
    assert_eq!(decision.selected, None);
    assert_eq!(decision.reason, KanDecisionReason::AcceptanceRegresses);

    let candidate = decision.candidates.first().expect("候補");
    assert_eq!(candidate.shanten_delta(), Some(0));
    assert!(
        candidate
            .acceptance_remaining_delta()
            .is_some_and(|delta| delta < 0)
    );
}

// テンパイでない局面は既存の攻撃打点で加槓前後を比較できないので、速度が悪化しなくても
// 加槓しない。
#[test]
fn a_kakan_is_rejected_when_the_value_is_not_evaluable() {
    let ctx = kakan_context(
        &KAKAN_IISHANTEN_HAND,
        KAKAN_IISHANTEN_DRAWN,
        [false; 4],
        [vec![east_pon_meld()], vec![], vec![], vec![]],
        Some(0),
        rivers_with_tile_for_all_opponents([108, 109, 110]),
    );
    let actions: Vec<LegalAction> =
        kakan_dahai_actions(&KAKAN_IISHANTEN_HAND, KAKAN_IISHANTEN_DRAWN)
            .into_iter()
            .chain([kakan_action(KAKAN_IISHANTEN_DRAWN, &KAKAN_FREE_CONSUMED)])
            .collect();

    let decision = decide(&ctx, &actions, PushPullMode::Push).expect("加槓候補");
    assert_eq!(decision.selected, None);
    assert_eq!(decision.reason, KanDecisionReason::ValueNotEvaluable);

    let candidate = decision.candidates.first().expect("候補");
    assert_eq!(candidate.shanten_delta(), Some(0));
    assert_eq!(candidate.baseline.and_then(|hand| hand.offense), None);
    assert_eq!(candidate.post_kan.and_then(|hand| hand.offense), None);
}

// 速度が変わらなくても、加槓で三色が消えて打点が下がる局面では加槓しない。
#[test]
fn a_kakan_is_rejected_when_the_offense_value_regresses() {
    let ctx = kakan_context(
        &KAKAN_VALUE_REGRESSING_HAND,
        KAKAN_VALUE_REGRESSING_DRAWN,
        [false; 4],
        [vec![three_man_pon_meld()], vec![], vec![], vec![]],
        Some(0),
        rivers_with_tile_for_all_opponents([8, 9, 10]),
    );
    let actions: Vec<LegalAction> =
        kakan_dahai_actions(&KAKAN_VALUE_REGRESSING_HAND, KAKAN_VALUE_REGRESSING_DRAWN)
            .into_iter()
            .chain([kakan_action(
                KAKAN_VALUE_REGRESSING_ADDED,
                &KAKAN_VALUE_REGRESSING_CONSUMED,
            )])
            .collect();

    let decision = decide(&ctx, &actions, PushPullMode::Push).expect("加槓候補");
    assert_eq!(decision.selected, None);
    assert_eq!(decision.reason, KanDecisionReason::ValueRegresses);

    let candidate = decision.candidates.first().expect("候補");
    // 速度は同じで、打点だけが下がる。
    assert_eq!(candidate.shanten_delta(), Some(0));
    assert_eq!(candidate.acceptance_remaining_delta(), Some(0));
    assert_eq!(candidate.acceptance_type_delta(), Some(0));
    assert_eq!(
        candidate.baseline.and_then(|hand| hand.offense_mode()),
        candidate.post_kan.and_then(|hand| hand.offense_mode())
    );
    assert!(
        candidate
            .weighted_total_delta()
            .is_some_and(|delta| delta < 0),
        "candidate: {candidate:?}"
    );
}

// 成立したカン候補が2件以上ある場合は、種別にかかわらず合法 action の列挙順を tie-break に
// せずどれも選ばない。
#[test]
fn several_eligible_kakan_candidates_are_not_broken_by_the_legal_action_order() {
    let ctx = kakan_free_hard_safe_context();
    // 同じ加槓を consumed の並び順だけ変えて2件渡す。どちらも同じ加槓後 state になる。
    let first = kakan_action(KAKAN_FREE_DRAWN, &KAKAN_FREE_CONSUMED);
    let second = kakan_action(KAKAN_FREE_DRAWN, &[110, 109, 108]);
    let actions: Vec<LegalAction> = kakan_dahai_actions(&KAKAN_FREE_HAND, KAKAN_FREE_DRAWN)
        .into_iter()
        .chain([first, second])
        .collect();

    let decision = decide(&ctx, &actions, PushPullMode::Push).expect("加槓候補");
    assert_eq!(decision.candidates.len(), 2);
    assert!(
        decision
            .candidates
            .iter()
            .all(|candidate| candidate.eligible)
    );
    // 先頭の候補を採らない。
    assert_eq!(decision.selected, None);
    assert_eq!(
        decision.reason,
        KanDecisionReason::MultipleEligibleCandidates
    );
}

// 加槓を production へ接続しても、暗槓の既存 policy は変わらない。
#[test]
fn connecting_the_kakan_keeps_the_existing_ankan_policy() {
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
}
