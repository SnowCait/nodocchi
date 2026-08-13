//! 非リーチ副露相手 (High OpenHandThreat) に対する防御 safety の scenario 回帰テスト。
//!
//! `scenarios/open_hand_defense.json` は、High の副露相手が2人・Present の副露相手が1人いる
//! 局面で、合法 Dahai ごとに「本人の河」「字牌 safety」「壁」「スジ」がどう出るかを1つの
//! fixture で見比べるためのもの。corpus 側で safety を計算し直さず、production の pure helper が
//! 返した値をそのまま確認する。
//!
//! この PR では防御 safety を押し引き・selected action へは接続していないため、High の相手が
//! いても `NoOpponentReach` → `Push` のまま通常打牌を選ぶことも合わせて固定する。

use bot_core::{
    Agent, DiagnosticOptions, HonorSafetyRank, OpenHandDefenseCandidateDiagnostic,
    OpenHandDefenseCategory, OpenHandThreatLevel, OpenHandThreatReason, OpponentHonorValue,
    PushPullMode, PushPullReason, ShantenAgent, ShantenDecisionDiagnostic, SuitedSafetyRank,
    SujiSafetyRank, WallRank, honor_safety_rank, is_discarded_by_all_open_hand_threats,
    open_hand_defense_category, opponent_honor_value_for_open_hand_threats,
    suited_safety_rank_for_open_hand_threats, suji_safety_rank_for,
    suji_safety_rank_for_open_hand_threats, wall_rank,
};
use bot_logic::TileType;

use crate::scenario::{Scenario, ScenarioSpec};

const OPEN_HAND_DEFENSE: &str = include_str!("../scenarios/open_hand_defense.json");

// High の副露相手。player 1 は親の役牌入り2副露、player 3 は3副露。
const DEALER_TARGET: usize = 1;
const CHILD_TARGET: usize = 3;
// 1副露だけの Present な副露相手。防御 target にしない。
const PRESENT_PLAYER: usize = 2;

fn spec() -> ScenarioSpec {
    serde_json::from_str(OPEN_HAND_DEFENSE).expect("scenario spec")
}

fn resolve(spec: &ScenarioSpec) -> Scenario {
    Scenario::resolve(spec).expect("scenario")
}

fn scenario() -> Scenario {
    resolve(&spec())
}

fn diagnose(scenario: &Scenario) -> ShantenDecisionDiagnostic {
    ShantenAgent::diagnose(&scenario.context, &scenario.legal_actions)
}

fn tile_type(mjai: &str) -> TileType {
    TileType::from_mjai_type_str(mjai).unwrap()
}

fn candidate(
    diagnostic: &ShantenDecisionDiagnostic,
    mjai: &str,
) -> OpenHandDefenseCandidateDiagnostic {
    diagnostic
        .open_hand_defense
        .candidates
        .iter()
        .find(|candidate| candidate.tile == tile_type(mjai))
        .unwrap_or_else(|| panic!("{mjai} の候補"))
        .clone()
}

#[test]
fn only_the_high_open_hand_threats_are_defense_targets() {
    let diagnostic = diagnose(&scenario());

    assert_eq!(
        diagnostic.open_hand_defense.targets,
        vec![DEALER_TARGET, CHILD_TARGET]
    );
    assert_eq!(
        diagnostic.player_threats[DEALER_TARGET]
            .open_hand_threat
            .reason(),
        Some(OpenHandThreatReason::TwoOrMoreWithValueHonor)
    );
    assert_eq!(
        diagnostic.player_threats[CHILD_TARGET]
            .open_hand_threat
            .reason(),
        Some(OpenHandThreatReason::ThreeOrMoreOpenMelds)
    );
    // 1副露だけの相手は Present なので target にしない。
    assert_eq!(
        diagnostic.player_threats[PRESENT_PLAYER]
            .open_hand_threat
            .level(),
        Some(OpenHandThreatLevel::Present)
    );
}

#[test]
fn a_tile_in_every_targets_river_is_the_first_category() {
    // 5m は player 1 と player 3 の河にある。本人の河はフリテン根拠なのでロンされない。
    let diagnostic = diagnose(&scenario());
    let five_man = candidate(&diagnostic, "5m");

    assert!(five_man.discarded_by_all_targets);
    assert!(
        five_man
            .targets
            .iter()
            .all(|target| target.discarded_by_target)
    );
    assert_eq!(
        five_man.category,
        Some(OpenHandDefenseCategory::DiscardedByAllTargets)
    );
    // スジや壁が無くても、本人の河が最優先の分類になる。
    assert_eq!(five_man.suji_safety_rank, Some(SujiSafetyRank::NoSuji));
    assert_eq!(five_man.wall_rank, Some(WallRank::NoWall));
}

#[test]
fn the_most_dangerous_opponent_honor_value_of_the_targets_is_used() {
    // 場風 東で player 1 は親。東は player 1 にとってダブ東、player 3 にとっては場風だけ。
    let diagnostic = diagnose(&scenario());
    let east = candidate(&diagnostic, "E");

    assert_eq!(east.honor_safety_rank, Some(HonorSafetyRank::OneVisible));
    assert_eq!(
        east.opponent_honor_value,
        Some(OpponentHonorValue::DoubleWind)
    );
    assert_eq!(
        east.category,
        Some(OpenHandDefenseCategory::HonorSafety(
            HonorSafetyRank::OneVisible
        ))
    );
    assert_eq!(east.wall_rank, None);
    assert_eq!(east.suji_safety_rank, None);
}

#[test]
fn a_suji_against_every_target_stays_suji() {
    // 1p と 7p が両方の河にあるので 4p は両側スジ。
    let diagnostic = diagnose(&scenario());
    let four_pin = candidate(&diagnostic, "4p");

    for target in &four_pin.targets {
        assert_eq!(
            target.suji_safety_rank,
            Some(SujiSafetyRank::Suji),
            "target {}",
            target.player
        );
    }
    assert_eq!(four_pin.suji_safety_rank, Some(SujiSafetyRank::Suji));
    assert_eq!(four_pin.suited_safety_rank, Some(SuitedSafetyRank::Suji));
    assert_eq!(
        four_pin.category,
        Some(OpenHandDefenseCategory::SuitedSafety(
            SuitedSafetyRank::Suji
        ))
    );
}

#[test]
fn a_suji_against_only_one_target_is_aggregated_as_the_most_dangerous_rank() {
    // 6s は player 1 の河にしか無いので、3s は player 3 に対して無スジ。
    let diagnostic = diagnose(&scenario());
    let three_sou = candidate(&diagnostic, "3s");

    assert_eq!(
        three_sou
            .targets
            .iter()
            .map(|target| (target.player, target.suji_safety_rank))
            .collect::<Vec<(usize, Option<SujiSafetyRank>)>>(),
        vec![
            (DEALER_TARGET, Some(SujiSafetyRank::Suji)),
            (CHILD_TARGET, Some(SujiSafetyRank::NoSuji)),
        ]
    );
    assert_eq!(three_sou.suji_safety_rank, Some(SujiSafetyRank::NoSuji));
    assert_eq!(
        three_sou.suited_safety_rank,
        Some(SuitedSafetyRank::NoSafety)
    );
}

#[test]
fn the_wall_comes_from_the_visible_tiles_and_beats_the_suji() {
    // 8m が4枚見えているので 9m の順子待ち経路は残らない。壁は target に依らない見え牌由来。
    let scenario = scenario();
    let diagnostic = diagnose(&scenario);
    let nine_man = candidate(&diagnostic, "9m");

    assert_eq!(
        nine_man.wall_rank,
        Some(wall_rank(tile_type("9m"), &scenario.context))
    );
    assert_eq!(nine_man.wall_rank, Some(WallRank::NoChance));
    assert_eq!(nine_man.suji_safety_rank, Some(SujiSafetyRank::NoSuji));
    assert_eq!(
        nine_man.suited_safety_rank,
        Some(SuitedSafetyRank::NoChance)
    );
}

#[test]
fn every_candidate_reports_the_production_safety_helpers() {
    let scenario = scenario();
    let diagnostic = diagnose(&scenario);
    let context = &scenario.context;
    let targets = diagnostic.open_hand_defense.targets.clone();

    assert_eq!(
        diagnostic
            .open_hand_defense
            .candidates
            .iter()
            .map(|candidate| candidate.action.clone())
            .collect::<Vec<_>>(),
        scenario.legal_actions
    );

    for candidate in &diagnostic.open_hand_defense.candidates {
        let tile = candidate.tile;
        let label = tile.to_mjai_string();

        assert_eq!(
            candidate.discarded_by_all_targets,
            is_discarded_by_all_open_hand_threats(tile, &targets, context),
            "{label}"
        );
        assert_eq!(
            candidate.honor_safety_rank,
            honor_safety_rank(tile, context),
            "{label}"
        );
        assert_eq!(
            candidate.opponent_honor_value,
            opponent_honor_value_for_open_hand_threats(tile, &targets, context),
            "{label}"
        );
        assert_eq!(
            candidate.suji_safety_rank,
            suji_safety_rank_for_open_hand_threats(tile, &targets, context),
            "{label}"
        );
        assert_eq!(
            candidate.suited_safety_rank,
            suited_safety_rank_for_open_hand_threats(tile, &targets, context),
            "{label}"
        );
        assert_eq!(
            candidate.category,
            open_hand_defense_category(tile, &targets, context),
            "{label}"
        );
        for target in &candidate.targets {
            assert_eq!(
                target.suji_safety_rank,
                suji_safety_rank_for(tile, target.player, context),
                "{label} target {}",
                target.player
            );
        }
    }
}

#[test]
fn a_target_with_the_tile_in_its_river_is_excluded_from_the_aggregated_suji() {
    // player 3 の河へ 3s を足すと、player 3 はフリテンで 3s をロンできなくなる。その無スジは
    // 集約に持ち込まれず、まだロンされ得る player 1 の両側スジがそのまま全体の評価になる。
    let mut spec = spec();
    spec.discards = Some(vec![
        "C 2p".to_string(),
        "5m 1p 7p 6s".to_string(),
        "9p 1s 4s F".to_string(),
        "5m 1p 7p 2m 3s".to_string(),
    ]);
    let scenario = resolve(&spec);
    let diagnostic = diagnose(&scenario);
    let three_sou = candidate(&diagnostic, "3s");

    assert_eq!(
        three_sou
            .targets
            .iter()
            .map(|target| (
                target.player,
                target.discarded_by_target,
                target.suji_safety_rank
            ))
            .collect::<Vec<(usize, bool, Option<SujiSafetyRank>)>>(),
        vec![
            (DEALER_TARGET, false, Some(SujiSafetyRank::Suji)),
            (CHILD_TARGET, true, Some(SujiSafetyRank::NoSuji)),
        ]
    );
    // 全 target の河にある訳ではないので第一分類にはならないが、集約は Suji のまま。
    assert!(!three_sou.discarded_by_all_targets);
    assert_eq!(three_sou.suji_safety_rank, Some(SujiSafetyRank::Suji));
    assert_eq!(three_sou.suited_safety_rank, Some(SuitedSafetyRank::Suji));
    assert_eq!(
        three_sou.category,
        Some(OpenHandDefenseCategory::SuitedSafety(
            SuitedSafetyRank::Suji
        ))
    );
}

#[test]
fn a_post_reach_passed_tile_is_not_river_safe_for_a_non_reach_target() {
    // post_reach_passed はリーチ者専用の情報で、非リーチ副露相手には流用しない。
    let mut spec = spec();
    spec.post_reach_passed = Some(vec![
        String::new(),
        "3s".to_string(),
        String::new(),
        "3s".to_string(),
    ]);
    let scenario = resolve(&spec);
    let diagnostic = diagnose(&scenario);
    let three_sou = candidate(&diagnostic, "3s");

    for player in [DEALER_TARGET, CHILD_TARGET] {
        assert!(
            scenario
                .context
                .is_post_reach_passed(tile_type("3s"), player)
        );
    }
    assert!(!three_sou.discarded_by_all_targets);
    assert!(
        three_sou
            .targets
            .iter()
            .all(|target| !target.discarded_by_target)
    );
    assert_ne!(
        three_sou.category,
        Some(OpenHandDefenseCategory::DiscardedByAllTargets)
    );
}

#[test]
fn the_high_threats_do_not_change_the_push_pull_or_the_selected_action() {
    let scenario = scenario();
    let mut agent = ShantenAgent;
    let acted = agent.act(&scenario.context, &scenario.legal_actions);
    let diagnostic = diagnose(&scenario);
    let with_lookahead = ShantenAgent::diagnose_with_options(
        &scenario.context,
        &scenario.legal_actions,
        DiagnosticOptions::WITH_LOOKAHEAD,
    );

    assert!(diagnostic.open_hand_defense.has_target());
    let decision = diagnostic
        .push_pull_decision
        .expect("押し引きを判定している");
    assert_eq!(decision.mode, PushPullMode::Push);
    assert_eq!(decision.reason, PushPullReason::NoOpponentReach);

    // 防御 fallback はリーチ局面用なので、この局面では検討自体が起きない。
    assert!(diagnostic.defense.is_none());
    assert_eq!(diagnostic.selected_action, acted);
    assert_eq!(with_lookahead.selected_action, acted);
    assert_eq!(diagnostic.normal_discard_action, Some(acted));
}
