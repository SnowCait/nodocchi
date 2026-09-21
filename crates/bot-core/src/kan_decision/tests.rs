use super::*;

use bot_logic::{AcceptanceTile, EffectiveShanten, TileType};

use crate::discard_selection::select_best_normal_discard_evaluation;
use crate::meld::{Meld, MeldKind};
use crate::offense_value::OffenseValue;
use crate::shanten_test_support::{
    ANKAN_ACCEPTANCE_REGRESSING_CONSUMED, ANKAN_ACCEPTANCE_REGRESSING_DRAWN,
    ANKAN_ACCEPTANCE_REGRESSING_HAND, ANKAN_FREE_CONSUMED, ANKAN_FREE_DRAWN, ANKAN_FREE_HAND,
    ANKAN_IISHANTEN_CONSUMED, ANKAN_IISHANTEN_DRAWN, ANKAN_IISHANTEN_HAND,
    ANKAN_REGRESSING_CONSUMED, ANKAN_REGRESSING_DRAWN, ANKAN_REGRESSING_HAND, ankan_action,
    ankan_context, ankan_dahai_actions, tile,
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

// 打点比較そのものの semantics。暗槓前後で同じ尺度の値を確定できない組み合わせはすべて
// 評価不能にし、速度非劣化だけで暗槓へ倒さない。
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
        KanDecisionReason::EligibleAnkanNoRegression
    );
    assert_eq!(
        compare_offense_value(
            hand(0, 4, offense(damaten, 8000)),
            hand(0, 4, offense(damaten, 8000))
        ),
        KanDecisionReason::EligibleAnkanNoRegression
    );
    assert_eq!(
        compare_offense_value(
            hand(0, 4, offense(damaten, 12000)),
            hand(0, 4, offense(damaten, 8000))
        ),
        KanDecisionReason::ValueRegresses
    );

    // リーチ手とダマ手の打点は別 baseline の値なので比較しない。
    assert_eq!(
        compare_offense_value(
            hand(0, 4, offense(reach, 8000)),
            hand(0, 4, offense(damaten, 12000))
        ),
        KanDecisionReason::ValueNotEvaluable
    );
    // 攻撃モードを確定できない場合も比較しない。
    assert_eq!(
        compare_offense_value(
            hand(0, 4, offense(TenpaiOffenseMode::Unknown, 8000)),
            hand(0, 4, offense(TenpaiOffenseMode::Unknown, 12000))
        ),
        KanDecisionReason::ValueNotEvaluable
    );
    // 役なし・ロン不可・点数計算の入力不足で打点を確定できない side があれば比較しない。
    assert_eq!(
        compare_offense_value(
            hand(0, 4, unknown_offense(damaten)),
            hand(0, 4, offense(damaten, 12000))
        ),
        KanDecisionReason::ValueNotEvaluable
    );
    assert_eq!(
        compare_offense_value(
            hand(0, 4, offense(damaten, 8000)),
            hand(0, 4, unknown_offense(damaten))
        ),
        KanDecisionReason::ValueNotEvaluable
    );
    // テンパイでない side は打点そのものを持たない。
    assert_eq!(
        compare_offense_value(hand(1, 8, None), hand(1, 8, offense(damaten, 12000))),
        KanDecisionReason::ValueNotEvaluable
    );
    assert_eq!(
        compare_offense_value(hand(1, 8, offense(damaten, 8000)), hand(1, 8, None)),
        KanDecisionReason::ValueNotEvaluable
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
        KanDecisionReason::ShantenImprovedNotComparable,
        KanDecisionReason::AcceptanceRegresses,
        KanDecisionReason::ValueNotEvaluable,
        KanDecisionReason::ValueRegresses,
        KanDecisionReason::MultipleEligibleCandidates,
    ] {
        assert!(!reason.is_eligible(), "reason: {reason:?}");
    }
}
