//! scenario JSON fixture を使った防御 fallback と diagnostic の回帰テスト。
//!
//! 局面構築は bot-analysis の `Scenario::resolve()` に任せ、ここでは production の判断が
//! fixture ごとに変わっていないことだけを確認する。

use bot_analysis::{Scenario, ScenarioSpec};
use bot_core::{
    Agent, DefenseCandidateDiagnostic, DefenseFallbackKind, DiagnosticOptions, HonorSafetyRank,
    LegalAction, OpponentHonorValue, ShantenAgent, SuitedSafetyRank, SujiSafetyRank,
    is_genbutsu_for, is_genbutsu_for_all_reached, select_defense_fallback_action_with_kind,
    suji_safety_rank_for,
};
use bot_logic::{HistoryFuritenFacts, TileId, TileType};

const POST_REACH_GENBUTSU_SCENARIO: &str = include_str!("../scenarios/post_reach_genbutsu.json");
const MULTI_RIICHI_DOUBLE_WIND_SCENARIO: &str =
    include_str!("../scenarios/defense_multi_riichi_double_wind.json");

fn spec_from_json(json: &str) -> ScenarioSpec {
    serde_json::from_str(json).unwrap()
}

fn resolve(spec: &ScenarioSpec) -> Scenario {
    Scenario::resolve(spec).unwrap()
}

fn tile_type(mjai: &str) -> TileType {
    TileType::from_mjai_type_str(mjai).unwrap()
}

#[test]
fn post_reach_genbutsu_scenario_makes_the_passed_tile_genbutsu_for_both_reachers() {
    let spec = spec_from_json(POST_REACH_GENBUTSU_SCENARIO);
    let context = resolve(&spec).context;
    let four_sou = tile_type("4s");

    assert_eq!(context.reached_opponents(), vec![1, 2]);
    assert!(is_genbutsu_for(four_sou, 1, &context));
    assert!(is_genbutsu_for(four_sou, 2, &context));
    assert!(is_genbutsu_for_all_reached(four_sou, &context));
}

#[test]
fn post_reach_genbutsu_scenario_selects_the_passed_tile_as_genbutsu_fallback() {
    let spec = spec_from_json(POST_REACH_GENBUTSU_SCENARIO);
    let scenario = resolve(&spec);
    let selected =
        select_defense_fallback_action_with_kind(&scenario.context, &scenario.legal_actions);

    assert_eq!(
        selected.map(|(action, kind)| (action.clone(), kind)),
        Some((
            LegalAction::Dahai {
                tile: TileId::new(tile_type("4s").raw() * 4).unwrap(),
            },
            DefenseFallbackKind::Genbutsu
        ))
    );
}

#[test]
fn multi_riichi_double_wind_scenario_prefers_suji_and_keeps_diagnostics_consistent() {
    // Player 2's open guest-wind Pon intentionally makes exact reach evaluation unavailable,
    // keeping this scenario focused on the legacy multi-riichi Suji fallback.
    let scenario = resolve(&spec_from_json(MULTI_RIICHI_DOUBLE_WIND_SCENARIO));
    let nine_man = tile_type("9m");
    let south = tile_type("S");

    assert_eq!(scenario.context.reached_opponents(), vec![1, 2]);
    assert!(!is_genbutsu_for(nine_man, 1, &scenario.context));
    assert!(is_genbutsu_for(nine_man, 2, &scenario.context));
    assert_eq!(
        suji_safety_rank_for(nine_man, 1, &scenario.context),
        Some(SujiSafetyRank::Suji)
    );

    let selected =
        select_defense_fallback_action_with_kind(&scenario.context, &scenario.legal_actions)
            .expect("defense fallback");
    assert_eq!(
        selected.1,
        DefenseFallbackKind::SuitedSafety(SuitedSafetyRank::Suji)
    );
    assert!(matches!(selected.0, LegalAction::Dahai { tile } if tile.tile_type() == nine_man));

    let candidates = DefenseCandidateDiagnostic::for_legal_actions(
        &scenario.context,
        &scenario.legal_actions,
        Some(selected.0),
    );
    let south = candidates
        .iter()
        .find(|candidate| candidate.tile == south)
        .unwrap();
    assert_eq!(south.honor_safety_rank, Some(HonorSafetyRank::OneVisible));
    assert_eq!(
        south.opponent_honor_value,
        Some(OpponentHonorValue::DoubleWind)
    );

    let mut agent = ShantenAgent;
    let action = agent.act(&scenario.context, &scenario.legal_actions);
    let diagnostic = ShantenAgent::diagnose(&scenario.context, &scenario.legal_actions);
    let with_lookahead = ShantenAgent::diagnose_with_options(
        &scenario.context,
        &scenario.legal_actions,
        DiagnosticOptions::WITH_LOOKAHEAD,
    );
    assert_eq!(action, diagnostic.selected_action);
    assert_eq!(action, with_lookahead.selected_action);
    assert!(matches!(action, LegalAction::Dahai { tile } if tile.tile_type() == nine_man));
    assert_eq!(
        diagnostic.defense_fallback_kind(),
        Some(DefenseFallbackKind::SuitedSafety(SuitedSafetyRank::Suji))
    );
    assert_eq!(
        with_lookahead.defense_fallback_kind(),
        diagnostic.defense_fallback_kind()
    );
}

#[test]
fn history_furiten_does_not_change_selection_or_diagnostic_consistency() {
    let base = resolve(&spec_from_json(MULTI_RIICHI_DOUBLE_WIND_SCENARIO));
    let context = base
        .context
        .clone()
        .with_history_furiten_facts(HistoryFuritenFacts {
            same_turn: Some(true),
            riichi_missed_win: Some(true),
        });
    let mut agent = ShantenAgent;
    let action = agent.act(&context, &base.legal_actions);
    let diagnostic = ShantenAgent::diagnose(&context, &base.legal_actions);
    let lookahead = ShantenAgent::diagnose_with_options(
        &context,
        &base.legal_actions,
        DiagnosticOptions::WITH_LOOKAHEAD,
    );
    assert_eq!(action, diagnostic.selected_action);
    assert_eq!(action, lookahead.selected_action);
    assert_eq!(diagnostic.history_furiten, context.history_furiten());
    assert_eq!(lookahead.history_furiten, context.history_furiten());
}
