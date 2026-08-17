//! リーチ者と High OpenHandThreat の副露相手が同時にいる複合 threat 局面の防御 safety の
//! scenario 回帰テスト。
//!
//! `scenarios/combined_threat_defense.json` は、player 1 がリーチ・player 3 が3副露の High・
//! player 2 が Present という局面で、合法 Dahai ごとに「全 threat へのロン安全」「字牌 safety」
//! 「壁」「スジ」がどう出るかを1つの fixture で見比べるためのもの。corpus 側で safety を計算し
//! 直さず、production の pure helper が返した値をそのまま確認する。
//!
//! この局面は自分が二向聴なので、複合 threat では `Fold` になり、通常打牌より複合 threat 用の
//! 防御 fallback が優先されることも合わせて固定する。

use bot_core::{
    Agent, CombinedDefenseCandidateDiagnostic, CombinedDefenseCategory, DiagnosticOptions,
    HonorSafetyRank, LegalAction, OpenHandDefenseCategory, OpenHandThreatReason,
    OpponentHonorValue, PushPullMode, PushPullReason, ShantenAgent, ShantenDecisionDiagnostic,
    SuitedSafetyRank, SujiSafetyRank, ThreatDefenseTarget, ThreatDefenseTargetKind,
    combined_defense_category, combined_threat_defense_targets_from_context, honor_safety_rank,
    is_discarded_by_player, is_ron_safe_for_target, is_safe_against_all_threats,
    opponent_honor_value_for_combined_threats,
    select_combined_threat_defense_fallback_action_with_kind,
    suited_safety_rank_for_combined_threats, suji_safety_rank_for,
    suji_safety_rank_for_combined_threats, wall_rank,
};
use bot_logic::TileType;

use crate::scenario::{Scenario, ScenarioSpec};

const COMBINED_THREAT_DEFENSE: &str = include_str!("../scenarios/combined_threat_defense.json");
const REQUEST_131_TEMPORARY_PASSED: &str =
    include_str!("../scenarios/request_131_temporary_passed.json");

// リーチしている親。post_reach_passed もこの player の分だけが安全根拠になる。
const RIICHI_TARGET: usize = 1;
// 1副露だけの Present な副露相手。防御 target にしない。
const PRESENT_PLAYER: usize = 2;
// 3副露の High な副露相手。
const OPEN_HAND_TARGET: usize = 3;

fn spec() -> ScenarioSpec {
    serde_json::from_str(COMBINED_THREAT_DEFENSE).expect("scenario spec")
}

fn resolve(spec: &ScenarioSpec) -> Scenario {
    Scenario::resolve(spec).expect("scenario")
}

fn scenario() -> Scenario {
    resolve(&spec())
}

#[test]
fn request_131_selects_nine_man_after_drawing_eight_sou() {
    let spec: ScenarioSpec = serde_json::from_str(REQUEST_131_TEMPORARY_PASSED).unwrap();
    let scenario = resolve(&spec);
    let diagnostic = diagnose(&scenario);
    let nine_man = candidate(&diagnostic, "9m");

    assert_eq!(
        scenario.context.drawn_tile().unwrap().to_mjai_string(),
        "8s"
    );
    assert_eq!(scenario.context.player_id(), Some(1));
    assert_eq!(scenario.context.oya(), Some(0));
    assert_eq!(scenario.legal_actions.len(), 12);
    assert_eq!(
        diagnostic.combined_defense.targets,
        vec![
            ThreatDefenseTarget::riichi(0),
            ThreatDefenseTarget::high_open_hand(3),
        ]
    );
    assert_eq!(
        diagnostic.player_threats[3].open_hand_threat.reason(),
        Some(OpenHandThreatReason::TwoOrMoreOpenMeldsFromNineDiscards)
    );
    assert_eq!(
        scenario
            .context
            .temporary_passed_tiles()
            .map(|players| players
                .iter()
                .map(|tiles| tiles.iter().map(|tile| tile.to_mjai_string()).collect())
                .collect::<Vec<Vec<_>>>()),
        Some(vec![
            vec![],
            vec![],
            vec!["9m".to_string()],
            vec!["9m".to_string()],
        ])
    );
    assert!(is_discarded_by_player(
        tile_type("9m"),
        0,
        &scenario.context
    ));
    assert!(scenario.context.is_temporary_passed(tile_type("9m"), 3));
    assert_eq!(
        nine_man
            .targets
            .iter()
            .map(|target| (target.player(), target.kind(), target.ron_safe))
            .collect::<Vec<_>>(),
        vec![
            (0, ThreatDefenseTargetKind::Riichi, true),
            (3, ThreatDefenseTargetKind::HighOpenHand, true),
        ]
    );
    assert_eq!(
        nine_man.category,
        Some(CombinedDefenseCategory::SafeAgainstAllThreats)
    );
    assert_eq!(
        diagnostic
            .combined_defense
            .selected
            .as_ref()
            .map(|selection| discards(&selection.selected_action)),
        Some(tile_type("9m"))
    );
    assert_eq!(discards(&diagnostic.selected_action), tile_type("9m"));
    assert_eq!(
        diagnostic.combined_defense_category(),
        Some(CombinedDefenseCategory::SafeAgainstAllThreats)
    );
    assert_ne!(
        diagnostic
            .combined_defense
            .selected
            .as_ref()
            .map(|selection| discards(&selection.selected_action)),
        Some(tile_type("1s"))
    );
}

fn diagnose(scenario: &Scenario) -> ShantenDecisionDiagnostic {
    ShantenAgent::diagnose(&scenario.context, &scenario.legal_actions)
}

fn tile_type(mjai: &str) -> TileType {
    TileType::from_mjai_type_str(mjai).unwrap()
}

fn discards(action: &LegalAction) -> TileType {
    match action {
        LegalAction::Dahai { tile } => tile.tile_type(),
        other => panic!("Dahai ではない: {other:?}"),
    }
}

fn candidate(
    diagnostic: &ShantenDecisionDiagnostic,
    mjai: &str,
) -> CombinedDefenseCandidateDiagnostic {
    diagnostic
        .combined_defense
        .candidates
        .iter()
        .find(|candidate| candidate.tile == tile_type(mjai))
        .unwrap_or_else(|| panic!("{mjai} の候補"))
        .clone()
}

#[test]
fn the_riichi_and_the_high_open_hand_are_both_defense_targets() {
    let diagnostic = diagnose(&scenario());

    assert_eq!(
        diagnostic.combined_defense.targets,
        vec![
            ThreatDefenseTarget::riichi(RIICHI_TARGET),
            ThreatDefenseTarget::high_open_hand(OPEN_HAND_TARGET),
        ]
    );
    // Present の副露相手は複合 threat の target にしない。
    assert!(
        !diagnostic
            .combined_defense
            .targets
            .iter()
            .any(|target| target.player == PRESENT_PLAYER)
    );
    // 既存 section の target 集合は変えない。
    assert_eq!(diagnostic.open_hand_defense.targets, vec![OPEN_HAND_TARGET]);
}

#[test]
fn a_tile_in_every_targets_river_is_the_first_category() {
    // 5m は player 1 と player 3 の河にある。どちらにもフリテンでロンされない。
    let diagnostic = diagnose(&scenario());
    let five_man = candidate(&diagnostic, "5m");

    assert!(five_man.safe_against_all_threats);
    assert!(five_man.targets.iter().all(|target| target.ron_safe));
    assert_eq!(
        five_man.category,
        Some(CombinedDefenseCategory::SafeAgainstAllThreats)
    );
    // スジや壁が無くても、全 threat へのロン安全が最優先の分類になる。
    assert_eq!(five_man.suji_safety_rank, Some(SujiSafetyRank::NoSuji));
    assert_eq!(
        five_man.suited_safety_rank,
        Some(SuitedSafetyRank::NoSafety)
    );
}

#[test]
fn a_post_reach_passed_tile_is_ron_safe_only_for_the_riichi_target() {
    // 9m は player 1 のリーチ後に通った牌。player 3 には安全根拠にならない。
    let scenario = scenario();
    let diagnostic = diagnose(&scenario);
    let nine_man = candidate(&diagnostic, "9m");

    assert!(
        scenario
            .context
            .is_post_reach_passed(tile_type("9m"), RIICHI_TARGET)
    );
    assert_eq!(
        nine_man
            .targets
            .iter()
            .map(|target| (target.player(), target.kind(), target.ron_safe))
            .collect::<Vec<(usize, ThreatDefenseTargetKind, bool)>>(),
        vec![
            (RIICHI_TARGET, ThreatDefenseTargetKind::Riichi, true),
            (
                OPEN_HAND_TARGET,
                ThreatDefenseTargetKind::HighOpenHand,
                false
            ),
        ]
    );
    assert!(!nine_man.safe_against_all_threats);
    assert_ne!(
        nine_man.category,
        Some(CombinedDefenseCategory::SafeAgainstAllThreats)
    );
}

#[test]
fn a_post_reach_passed_tile_becomes_safe_with_the_open_hand_targets_river() {
    // player 3 の河に 9m を足すと、両方の根拠が揃って第一分類になる。
    let mut spec = spec();
    spec.discards = Some(vec![
        "C 2p".to_string(),
        "5m 1p 7p 6s".to_string(),
        "9p 1s 4s F".to_string(),
        "5m 1p 7p 2m 9m".to_string(),
    ]);
    let scenario = resolve(&spec);
    let diagnostic = diagnose(&scenario);
    let nine_man = candidate(&diagnostic, "9m");

    assert!(nine_man.targets.iter().all(|target| target.ron_safe));
    assert!(nine_man.safe_against_all_threats);
    assert_eq!(
        nine_man.category,
        Some(CombinedDefenseCategory::SafeAgainstAllThreats)
    );
}

#[test]
fn another_players_discard_is_not_a_safety_source_for_the_open_hand_target() {
    // player 2 が 9m を切っても、player 3 本人の河ではないので安全根拠にしない。
    let mut spec = spec();
    spec.discards = Some(vec![
        "C 2p".to_string(),
        "5m 1p 7p 6s".to_string(),
        "9p 1s 4s F 9m".to_string(),
        "5m 1p 7p 2m".to_string(),
    ]);
    let scenario = resolve(&spec);
    let diagnostic = diagnose(&scenario);
    let nine_man = candidate(&diagnostic, "9m");

    assert!(!nine_man.safe_against_all_threats);
    assert!(
        !nine_man.targets.iter().any(|target| target.kind()
            == ThreatDefenseTargetKind::HighOpenHand
            && target.ron_safe)
    );
}

#[test]
fn a_suji_against_only_the_riichi_target_is_aggregated_as_the_most_dangerous_rank() {
    // 3s は player 1 の河の 6s から片側スジだが、player 3 に対しては無スジ。
    let diagnostic = diagnose(&scenario());
    let three_sou = candidate(&diagnostic, "3s");

    assert_eq!(
        three_sou
            .targets
            .iter()
            .map(|target| (target.player(), target.suji_safety_rank))
            .collect::<Vec<(usize, Option<SujiSafetyRank>)>>(),
        vec![
            (RIICHI_TARGET, Some(SujiSafetyRank::Suji)),
            (OPEN_HAND_TARGET, Some(SujiSafetyRank::NoSuji)),
        ]
    );
    assert_eq!(three_sou.suji_safety_rank, Some(SujiSafetyRank::NoSuji));
    assert_eq!(
        three_sou.suited_safety_rank,
        Some(SuitedSafetyRank::NoSafety)
    );
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
            target.player()
        );
    }
    assert_eq!(four_pin.suji_safety_rank, Some(SujiSafetyRank::Suji));
    assert_eq!(four_pin.suited_safety_rank, Some(SuitedSafetyRank::Suji));
}

#[test]
fn the_most_dangerous_opponent_honor_value_of_the_ron_capable_targets_is_used() {
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
        Some(CombinedDefenseCategory::HonorSafety(
            HonorSafetyRank::OneVisible
        ))
    );
}

#[test]
fn a_ron_safe_target_is_excluded_from_the_aggregated_honor_value() {
    // 東を player 1 の河へ足すと、そのダブ東は集約に残らない。
    let mut spec = spec();
    spec.discards = Some(vec![
        "C 2p".to_string(),
        "5m 1p 7p 6s E".to_string(),
        "9p 1s 4s F".to_string(),
        "5m 1p 7p 2m".to_string(),
    ]);
    let scenario = resolve(&spec);
    let diagnostic = diagnose(&scenario);
    let east = candidate(&diagnostic, "E");

    assert_eq!(
        east.targets
            .iter()
            .map(|target| target.ron_safe)
            .collect::<Vec<bool>>(),
        vec![true, false]
    );
    assert_eq!(
        east.opponent_honor_value,
        Some(OpponentHonorValue::SingleValueHonor)
    );
}

#[test]
fn the_wall_comes_from_the_visible_tiles_and_is_shared_with_the_existing_defense() {
    // 8m が4枚見えているので 9m の順子待ち経路は残らない。壁は target に依らない見え牌由来。
    let scenario = scenario();
    let diagnostic = diagnose(&scenario);
    let nine_man = candidate(&diagnostic, "9m");

    assert_eq!(
        nine_man.wall_rank,
        Some(wall_rank(tile_type("9m"), &scenario.context))
    );
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
    let targets = diagnostic.combined_defense.targets.clone();

    assert_eq!(
        diagnostic
            .combined_defense
            .candidates
            .iter()
            .map(|candidate| candidate.action.clone())
            .collect::<Vec<_>>(),
        scenario.legal_actions
    );

    for candidate in &diagnostic.combined_defense.candidates {
        let tile = candidate.tile;
        let label = tile.to_mjai_string();

        assert_eq!(
            candidate.safe_against_all_threats,
            is_safe_against_all_threats(tile, &targets, context),
            "{label}"
        );
        assert_eq!(
            candidate.honor_safety_rank,
            honor_safety_rank(tile, context),
            "{label}"
        );
        assert_eq!(
            candidate.opponent_honor_value,
            opponent_honor_value_for_combined_threats(tile, &targets, context),
            "{label}"
        );
        assert_eq!(
            candidate.suji_safety_rank,
            suji_safety_rank_for_combined_threats(tile, &targets, context),
            "{label}"
        );
        assert_eq!(
            candidate.suited_safety_rank,
            suited_safety_rank_for_combined_threats(tile, &targets, context),
            "{label}"
        );
        assert_eq!(
            candidate.category,
            combined_defense_category(tile, &targets, context),
            "{label}"
        );
        for target in &candidate.targets {
            assert_eq!(
                target.ron_safe,
                is_ron_safe_for_target(tile, target.target, context),
                "{label} target {}",
                target.player()
            );
            assert_eq!(
                target.suji_safety_rank,
                suji_safety_rank_for(tile, target.player(), context),
                "{label} target {}",
                target.player()
            );
        }
    }
}

#[test]
fn the_combined_threat_drives_the_fold_and_the_selected_action() {
    let scenario = scenario();
    let mut agent = ShantenAgent;
    let acted = agent.act(&scenario.context, &scenario.legal_actions);
    let diagnostic = diagnose(&scenario);
    let with_lookahead = ShantenAgent::diagnose_with_options(
        &scenario.context,
        &scenario.legal_actions,
        DiagnosticOptions::WITH_LOOKAHEAD,
    );

    assert!(diagnostic.combined_defense.has_target());
    let decision = diagnostic
        .push_pull_decision
        .expect("押し引きを判定している");
    assert_eq!(decision.mode, PushPullMode::Fold);
    assert_eq!(
        decision.reason,
        PushPullReason::TwoOrMoreShantenAgainstCombinedThreat
    );

    // 既存のリーチ者向け / OpenHand 向け防御 fallback には切り替えない。
    assert!(diagnostic.defense.is_none());
    assert_eq!(diagnostic.defense_fallback_kind(), None);
    assert_eq!(diagnostic.open_hand_defense.selected, None);
    assert_eq!(diagnostic.open_hand_defense_category(), None);

    assert_eq!(diagnostic.selected_action, acted);
    assert_eq!(with_lookahead.selected_action, acted);
    assert_eq!(discards(&acted), tile_type("5m"));
    assert_eq!(
        diagnostic.combined_defense_category(),
        Some(CombinedDefenseCategory::SafeAgainstAllThreats)
    );
    assert_ne!(diagnostic.normal_discard_action, Some(acted.clone()));

    // 診断は production selector の結果をそのまま写す。
    let selection = diagnostic
        .combined_defense
        .selected
        .as_ref()
        .expect("複合 threat の防御 fallback を採用している");
    assert_eq!(selection.selected_action, acted);
    assert_eq!(
        selection.selected_category,
        CombinedDefenseCategory::SafeAgainstAllThreats
    );
    assert_eq!(
        with_lookahead.combined_defense, diagnostic.combined_defense,
        "追加診断は選択結果を変えない"
    );
    assert_eq!(
        diagnostic
            .combined_defense
            .candidates
            .iter()
            .filter(|candidate| candidate.selected)
            .map(|candidate| candidate.tile)
            .collect::<Vec<TileType>>(),
        vec![tile_type("5m")]
    );
}

#[test]
fn the_selected_fallback_matches_the_production_selector() {
    // 診断は選び直さず、production selector の結果を写す。
    let scenario = scenario();
    let diagnostic = diagnose(&scenario);
    let selected = select_combined_threat_defense_fallback_action_with_kind(
        &scenario.context,
        &scenario.legal_actions,
        &combined_threat_defense_targets_from_context(&scenario.context),
    );

    let (action, category) = selected.expect("防御 fallback を選べる");
    assert_eq!(diagnostic.selected_action, *action);
    assert_eq!(diagnostic.combined_defense_category(), Some(category));
}

#[test]
fn a_fold_without_a_safe_tile_falls_back_to_the_normal_discard() {
    // 安全牌候補が1件も無い場合だけ通常打牌に戻る。合法 Dahai を無スジの数牌だけに絞る。
    let mut spec = spec();
    spec.legal_dahai = Some("3s".to_string());
    let scenario = resolve(&spec);
    let diagnostic = diagnose(&scenario);

    assert!(diagnostic.combined_defense.has_target());
    assert_eq!(
        diagnostic.push_pull_decision.map(|decision| decision.mode),
        Some(PushPullMode::Fold)
    );
    assert_eq!(diagnostic.combined_defense_category(), None);
    assert_eq!(diagnostic.combined_defense.selected, None);
    assert_eq!(
        Some(diagnostic.selected_action.clone()),
        diagnostic.normal_discard_action
    );
}

#[test]
fn a_riichi_without_a_high_open_hand_keeps_the_existing_defense() {
    // player 3 の副露を1つに減らすと Present になり、複合 threat ではなくなる。
    let mut spec = spec();
    let melds = spec.melds.as_mut().expect("melds");
    melds[OPEN_HAND_TARGET].truncate(1);
    let scenario = resolve(&spec);
    let diagnostic = diagnose(&scenario);

    assert!(!diagnostic.combined_defense.has_target());
    assert!(diagnostic.combined_defense.candidates.is_empty());
    assert_eq!(diagnostic.combined_defense_category(), None);
    assert!(diagnostic.open_hand_defense.targets.is_empty());

    // 押し引きも防御も既存のリーチ policy のまま。
    assert_eq!(
        diagnostic
            .push_pull_decision
            .map(|decision| decision.reason),
        Some(PushPullReason::TwoOrMoreShantenAgainstReach)
    );
    assert!(diagnostic.defense.is_some());
    assert!(diagnostic.defense_fallback_kind().is_some());
}

#[test]
fn a_high_open_hand_without_a_riichi_keeps_the_open_hand_defense() {
    // リーチを外すと複合 threat ではなくなり、既存の OpenHand 防御 fallback に戻る。
    let mut spec = spec();
    spec.reached = Some(vec![false; 4]);
    let scenario = resolve(&spec);
    let diagnostic = diagnose(&scenario);

    assert!(!diagnostic.combined_defense.has_target());
    assert_eq!(diagnostic.combined_defense_category(), None);
    assert_eq!(
        diagnostic
            .push_pull_decision
            .map(|decision| decision.reason),
        Some(PushPullReason::TwoOrMoreShantenAgainstHighOpenHand)
    );
    assert_eq!(
        diagnostic.open_hand_defense_category(),
        Some(OpenHandDefenseCategory::SafeAgainstAllTargets)
    );
    assert!(diagnostic.defense.is_none());
}
