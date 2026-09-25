//! 非テンパイの Chi / Pon を、鳴き後の既存 Push/Pull と整合させる scenario 回帰テスト。
//!
//! `scenarios/two_shanten_pon_post_call_fold.json` は、2向聴から 5s を Pon すると1向聴になり、
//! Call / Pass 比較では Call が上回る (`EligibleTwoShantenSelfTsumo`) 局面。3副露の相手
//! (OpenHandThreat の Danger) がいるため、鳴いた直後の打牌では既存 Push/Pull が
//! `Fold / IishantenAgainstHighOpenHand` になり、防御 fallback (`OpenHandDefenseFallback`) へ移る。
//! 修正前は Pon を採用してから直後に降りていたが、この局面では Pon 自体を採用しない。
//!
//! `scenarios/two_shanten_pon_post_call_fold_after_pon.json` は同じ局面で Pon した直後の打牌
//! 局面で、鳴き判断が見た鳴き後 Push/Pull と、実際に鳴いた後の production 判断が一致することを
//! 固定する。

use bot_analysis::{Scenario, ScenarioSpec};
use bot_core::{
    Agent, AgentActionSource, CallDecisionReason, CallIishantenComparison, LegalAction,
    PushPullDecision, PushPullMode, PushPullReason, ShantenAgent,
};
use bot_logic::TileType;

const PON_POST_CALL_FOLD: &str = include_str!("../scenarios/two_shanten_pon_post_call_fold.json");
const PON_POST_CALL_FOLD_AFTER_PON: &str =
    include_str!("../scenarios/two_shanten_pon_post_call_fold_after_pon.json");

fn spec(json: &str) -> ScenarioSpec {
    serde_json::from_str(json).expect("scenario spec")
}

fn resolve(spec: &ScenarioSpec) -> Scenario {
    Scenario::resolve(spec).expect("scenario")
}

fn tile_type(mjai: &str) -> TileType {
    TileType::from_mjai_type_str(mjai).unwrap()
}

const POST_CALL_FOLD: PushPullDecision = PushPullDecision {
    mode: PushPullMode::Fold,
    reason: PushPullReason::IishantenAgainstHighOpenHand,
};

#[test]
fn a_two_shanten_pon_whose_post_call_push_pull_folds_is_not_taken() {
    let scenario = resolve(&spec(PON_POST_CALL_FOLD));
    let mut agent = ShantenAgent;
    let action = agent.act(&scenario.context, &scenario.legal_actions);
    let diagnostic = ShantenAgent::diagnose(&scenario.context, &scenario.legal_actions);

    assert_eq!(action, LegalAction::None);
    assert_eq!(diagnostic.selected_action, action);

    let call = diagnostic.call.as_ref().expect("call diagnostic");
    assert_eq!(call.selected, None);
    assert_eq!(call.reason, CallDecisionReason::PostCallNotPush);
    let [candidate] = call.candidates.as_slice() else {
        panic!("Pon 5s だけが合法な鳴き");
    };
    assert!(matches!(
        &candidate.action,
        LegalAction::Pon { tile, .. } if tile.tile_type() == tile_type("5s")
    ));
    assert!(!candidate.eligible);
    assert_eq!(candidate.reason, CallDecisionReason::PostCallNotPush);
    assert_eq!(candidate.current_shanten, Some(2));
    assert_eq!(candidate.post_call_shanten(), Some(1));

    // Call / Pass 比較では Call が成立していた。
    let comparison = candidate
        .two_shanten_self_tsumo
        .expect("2向聴 Call / Pass 比較");
    assert_eq!(comparison.comparison, CallIishantenComparison::CallHigher);
    assert!(comparison.call_expected_self_tsumo_value > comparison.pass_expected_self_tsumo_value);
    assert_eq!(
        candidate.call_pass_eligible_reason(),
        Some(CallDecisionReason::EligibleTwoShantenSelfTsumo)
    );

    // 鳴き後の選択打牌を既存 Push/Pull が Fold と判定したので採用しない。
    assert_eq!(
        candidate
            .post_call_discard
            .as_ref()
            .map(|evaluation| evaluation.discard),
        Some(tile_type("7s"))
    );
    assert_eq!(candidate.post_call_push_pull, Some(POST_CALL_FOLD));
}

#[test]
fn the_post_call_push_pull_matches_the_decision_right_after_the_pon() {
    let reaction = resolve(&spec(PON_POST_CALL_FOLD));
    let call = ShantenAgent::diagnose(&reaction.context, &reaction.legal_actions)
        .call
        .expect("call diagnostic");
    let candidate = &call.candidates[0];

    // 修正前に Pon を採用していた場合の、直後の打牌局面。
    let after_pon = resolve(&spec(PON_POST_CALL_FOLD_AFTER_PON));
    let mut agent = ShantenAgent;
    let action = agent.act(&after_pon.context, &after_pon.legal_actions);
    let diagnostic = ShantenAgent::diagnose(&after_pon.context, &after_pon.legal_actions);

    assert_eq!(diagnostic.selected_action, action);
    assert_eq!(diagnostic.push_pull_decision, Some(POST_CALL_FOLD));
    assert!(matches!(
        diagnostic.selected_source,
        AgentActionSource::OpenHandDefenseFallback(_)
    ));
    assert_eq!(
        diagnostic
            .normal_discard_action
            .as_ref()
            .and_then(|action| match action {
                LegalAction::Dahai { tile } => Some(tile.tile_type()),
                _ => None,
            }),
        candidate
            .post_call_discard
            .as_ref()
            .map(|evaluation| evaluation.discard)
    );
    assert_eq!(candidate.post_call_push_pull, diagnostic.push_pull_decision);
}

#[test]
fn the_same_pon_is_taken_when_the_post_call_discard_is_hard_safe() {
    // 鳴き後の選択打牌 7s が3副露の相手に一時通過牌なら、既存 Push/Pull は hard-safe な
    // 1向聴として押すので、Call / Pass 比較の結論どおり Pon する。
    let mut spec = spec(PON_POST_CALL_FOLD);
    spec.temporary_passed = Some(vec![
        String::new(),
        String::new(),
        "7s".to_string(),
        String::new(),
    ]);
    let scenario = resolve(&spec);
    let mut agent = ShantenAgent;
    let action = agent.act(&scenario.context, &scenario.legal_actions);
    let diagnostic = ShantenAgent::diagnose(&scenario.context, &scenario.legal_actions);

    assert!(matches!(action, LegalAction::Pon { .. }));
    assert_eq!(diagnostic.selected_action, action);
    assert_eq!(diagnostic.selected_source, AgentActionSource::Call);
    let candidate = &diagnostic
        .call
        .as_ref()
        .expect("call diagnostic")
        .candidates[0];
    assert_eq!(
        candidate.reason,
        CallDecisionReason::EligibleTwoShantenSelfTsumo
    );
    assert_eq!(
        candidate.post_call_push_pull,
        Some(PushPullDecision {
            mode: PushPullMode::Push,
            reason: PushPullReason::SafeIishantenAgainstHighOpenHand,
        })
    );
}
