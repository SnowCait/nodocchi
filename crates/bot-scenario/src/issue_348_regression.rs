//! Issue #348 の実戦代表局面を固定する回帰テスト。
//!
//! `scenarios/issue_348_pon_5s_before_call.json` は Issue #348 に記載した Call 前の state そのもの
//! (revision `e60513f` で `Pon 5s` を `EligibleTwoShantenSelfTsumo` で採用した局面)、
//! `scenarios/issue_348_pon_5s_after_pon.json` は同じ局面で Pon した直後の state そのもの。
//!
//! Call / Pass 比較では Pon が成立するが、鳴き後の選択打牌 W を既存 Push/Pull へ通すと
//! `Fold / IishantenAgainstHighOpenHand` になるので、Pon 自体を採用しない。Pon 直後の state でも
//! production は同じ Push/Pull で降りる。

use bot_analysis::{Scenario, ScenarioSpec};
use bot_core::{
    Agent, AgentActionSource, CallDecisionReason, CallIishantenComparison, CallKind, LegalAction,
    OpenHandDefenseCategory, PushPullDecision, PushPullMode, PushPullReason, ShantenAgent,
};
use bot_logic::TileType;

const BEFORE_CALL: &str = include_str!("../scenarios/issue_348_pon_5s_before_call.json");
const AFTER_PON: &str = include_str!("../scenarios/issue_348_pon_5s_after_pon.json");

const POST_CALL_FOLD: PushPullDecision = PushPullDecision {
    mode: PushPullMode::Fold,
    reason: PushPullReason::IishantenAgainstHighOpenHand,
};

fn resolve(json: &str) -> Scenario {
    let spec: ScenarioSpec = serde_json::from_str(json).expect("scenario spec");
    Scenario::resolve(&spec).expect("scenario")
}

fn tile_type(mjai: &str) -> TileType {
    TileType::from_mjai_type_str(mjai).unwrap()
}

fn dahai_type(action: &LegalAction) -> Option<TileType> {
    match action {
        LegalAction::Dahai { tile } => Some(tile.tile_type()),
        _ => None,
    }
}

#[test]
fn the_legal_actions_are_the_pon_the_chi_and_none_of_the_issue() {
    let scenario = resolve(BEFORE_CALL);
    let five_sou = tile_type("5s");

    let [pon, chi, none] = scenario.legal_actions.as_slice() else {
        panic!("{:?}", scenario.legal_actions);
    };
    assert!(matches!(
        pon,
        LegalAction::Pon { tile, consumed }
            if tile.tile_type() == five_sou
                && consumed.iter().map(|tile| (tile.tile_type(), tile.is_red())).collect::<Vec<_>>()
                    == [(five_sou, true), (five_sou, false)]
    ));
    assert!(matches!(
        chi,
        LegalAction::Chi { tile, consumed }
            if tile.tile_type() == five_sou
                && consumed.iter().map(|tile| tile.tile_type()).collect::<Vec<_>>()
                    == [tile_type("3s"), tile_type("4s")]
    ));
    assert_eq!(none, &LegalAction::None);
    assert_eq!(scenario.context.reaction_source_player(), Some(3));
}

#[test]
fn the_pon_5s_that_folds_right_after_the_call_is_not_taken() {
    let scenario = resolve(BEFORE_CALL);
    let mut agent = ShantenAgent;
    let action = agent.act(&scenario.context, &scenario.legal_actions);
    let diagnostic = ShantenAgent::diagnose(&scenario.context, &scenario.legal_actions);

    assert_eq!(action, LegalAction::None);
    assert_eq!(diagnostic.selected_action, action);

    let call = diagnostic.call.as_ref().expect("call diagnostic");
    assert_eq!(call.selected, None);
    assert_eq!(call.reason, CallDecisionReason::PostCallNotPush);

    let pon = call
        .candidates
        .iter()
        .find(|candidate| candidate.kind == CallKind::Pon)
        .expect("Pon 5s");
    assert!(!pon.eligible);
    assert_eq!(pon.reason, CallDecisionReason::PostCallNotPush);
    assert_eq!(pon.current_shanten, Some(2));
    assert_eq!(pon.post_call_shanten(), Some(1));

    // Call / Pass 比較の結果は変えていない。
    let comparison = pon.two_shanten_self_tsumo.expect("2向聴 Call / Pass 比較");
    assert_eq!(comparison.comparison, CallIishantenComparison::CallHigher);
    assert_eq!(
        pon.call_pass_eligible_reason(),
        Some(CallDecisionReason::EligibleTwoShantenSelfTsumo)
    );

    assert_eq!(
        pon.post_call_discard
            .as_ref()
            .map(|evaluation| evaluation.discard),
        Some(tile_type("W"))
    );
    assert_eq!(pon.post_call_push_pull, Some(POST_CALL_FOLD));
}

#[test]
fn the_decision_right_after_the_pon_folds_to_the_open_hand_defense() {
    let scenario = resolve(AFTER_PON);
    let mut agent = ShantenAgent;
    let action = agent.act(&scenario.context, &scenario.legal_actions);
    let diagnostic = ShantenAgent::diagnose(&scenario.context, &scenario.legal_actions);

    assert_eq!(diagnostic.selected_action, action);
    assert_eq!(dahai_type(&action), Some(tile_type("2s")));
    assert_eq!(
        diagnostic.selected_source,
        AgentActionSource::OpenHandDefenseFallback(OpenHandDefenseCategory::ExactRonRisk)
    );
    assert_eq!(diagnostic.push_pull_decision, Some(POST_CALL_FOLD));
    assert_eq!(
        diagnostic
            .normal_discard_action
            .as_ref()
            .and_then(dahai_type),
        Some(tile_type("W"))
    );
    let offense = diagnostic
        .push_pull_inputs
        .and_then(|inputs| inputs.offense)
        .expect("通常打牌の offense");
    assert_eq!(offense.min_shanten_after_discard, 1);
}

#[test]
fn the_post_call_push_pull_before_the_call_matches_the_decision_after_the_pon() {
    let before = resolve(BEFORE_CALL);
    let call = ShantenAgent::diagnose(&before.context, &before.legal_actions)
        .call
        .expect("call diagnostic");
    let pon = call
        .candidates
        .iter()
        .find(|candidate| candidate.kind == CallKind::Pon)
        .expect("Pon 5s");

    let after = resolve(AFTER_PON);
    let diagnostic = ShantenAgent::diagnose(&after.context, &after.legal_actions);

    assert_eq!(pon.post_call_push_pull, diagnostic.push_pull_decision);
    assert_eq!(
        pon.post_call_discard
            .as_ref()
            .map(|evaluation| evaluation.discard),
        diagnostic
            .normal_discard_action
            .as_ref()
            .and_then(dahai_type)
    );
    // 鳴き後 Push/Pull が読んだ1向聴の ExpectedSelfTsumoValue も、Pon 後の通常打牌選択と同じ。
    assert_eq!(
        pon.two_shanten_self_tsumo
            .and_then(|comparison| comparison.call_expected_self_tsumo_value),
        diagnostic
            .push_pull_inputs
            .and_then(|inputs| inputs.offense)
            .and_then(|offense| offense.iishanten_expected_self_tsumo_value())
    );
}
