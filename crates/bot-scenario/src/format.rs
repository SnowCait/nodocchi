use bot_core::{
    AgentActionSource, DefenseCandidateDiagnostic, DefenseDecisionDiagnostic, GameContext,
    LegalAction, PushPullDecision, PushPullInputs, ShantenDecisionDiagnostic,
};
use bot_logic::{DiscardCandidateDiagnostic, DiscardDecisionDiagnostic, DiscardEvaluation, TileId};

use crate::scenario::Scenario;

const NONE: &str = "none";
const ABSENT: &str = "-";

pub fn format_diagnostic(
    scenario: &Scenario,
    diagnostic: &ShantenDecisionDiagnostic,
    verbose: bool,
) -> String {
    let mut sections = vec![
        format_scenario(scenario, verbose),
        format_final_decision(diagnostic),
        format_normal_discard(diagnostic),
    ];

    if let Some(section) =
        format_normal_discard_candidates(diagnostic.normal_discard.as_ref(), verbose)
    {
        sections.push(section);
    }

    sections.push(format_push_pull(
        diagnostic.push_pull_inputs.as_ref(),
        diagnostic.push_pull_decision.as_ref(),
    ));
    sections.push(format_defense(diagnostic.defense.as_ref()));

    if let Some(section) = format_defense_candidates(diagnostic.defense.as_ref()) {
        sections.push(section);
    }

    sections.join("\n\n")
}

fn format_scenario(scenario: &Scenario, verbose: bool) -> String {
    let context = &scenario.context;
    let mut lines = vec!["Scenario".to_string()];

    lines.push(format!("  hand: {}", format_tiles(context.hand_tiles())));
    lines.push(format!(
        "  draw: {}",
        context
            .drawn_tile()
            .map(|tile| tile.to_mjai_string())
            .unwrap_or_else(|| "None".to_string())
    ));
    lines.push(format!(
        "  dora indicators: {}",
        format_tiles(context.dora_indicators())
    ));
    lines.push(format!(
        "  round wind: {}",
        format_wind(context.round_wind())
    ));
    lines.push(format!("  seat wind: {}", format_wind(context.seat_wind())));
    lines.push(format!("  player id: {}", format_seat(context.player_id())));
    lines.push(format!("  oya: {}", format_seat(context.oya())));
    lines.push(format!("  reached players: {}", format_reached(context)));

    for (player, discards) in context.discards().iter().enumerate() {
        if !discards.is_empty() {
            lines.push(format!("  discards[{player}]: {}", format_tiles(discards)));
        }
    }

    if verbose {
        lines.push(format!(
            "  visible tiles: {}",
            format_tiles(context.visible_tiles())
        ));
    } else {
        lines.push(format!(
            "  visible tiles: {} tiles",
            context.visible_tiles().len()
        ));
    }

    lines.push(format!(
        "  legal actions: {}",
        format_actions(&scenario.legal_actions)
    ));

    lines.join("\n")
}

fn format_final_decision(diagnostic: &ShantenDecisionDiagnostic) -> String {
    let mut lines = vec!["Final decision".to_string()];
    lines.push(format!(
        "  action: {}",
        action_label(&diagnostic.selected_action)
    ));
    lines.push(format!(
        "  source: {}",
        source_label(diagnostic.selected_source)
    ));
    if let Some(kind) = diagnostic.defense_fallback_kind() {
        lines.push(format!("  defense kind: {kind:?}"));
    }
    lines.join("\n")
}

fn format_normal_discard(diagnostic: &ShantenDecisionDiagnostic) -> String {
    let mut lines = vec!["Normal discard".to_string()];

    let Some(normal_discard) = diagnostic.normal_discard.as_ref() else {
        lines.push("  not evaluated".to_string());
        return lines.join("\n");
    };

    lines.push("  evaluated".to_string());
    match diagnostic.normal_discard_action.as_ref() {
        Some(action) => lines.push(format!("  selected action: {}", action_label(action))),
        None => lines.push(format!("  selected action: {NONE}")),
    }
    lines.push(format!("  candidates: {}", normal_discard.candidates.len()));

    lines.join("\n")
}

fn format_normal_discard_candidates(
    normal_discard: Option<&DiscardDecisionDiagnostic>,
    verbose: bool,
) -> Option<String> {
    let normal_discard = normal_discard?;
    if normal_discard.candidates.is_empty() {
        return None;
    }

    let mut blocks = vec!["Normal discard candidates".to_string()];
    for candidate in &normal_discard.candidates {
        blocks.push(format_normal_discard_candidate(candidate, verbose));
    }
    Some(blocks.join("\n\n"))
}

fn format_normal_discard_candidate(
    candidate: &DiscardCandidateDiagnostic,
    verbose: bool,
) -> String {
    let evaluation = &candidate.evaluation;
    let mut lines = vec![discard_label(evaluation)];

    lines.push(format!("  selected: {}", yes_no(candidate.selected)));
    lines.push(format!(
        "  shanten: {}",
        evaluation.min_shanten_after_discard()
    ));
    lines.push(format!(
        "  acceptance: {} / {} types",
        evaluation.acceptance_total_remaining(),
        evaluation.acceptance_type_count()
    ));
    lines.push(format!(
        "  iishanten shape: {:?}",
        evaluation.standard_iishanten_shape_after_discard
    ));
    lines.push(format!("  shape penalty: {}", evaluation.shape_penalty));
    lines.push(format!(
        "  floating tile value: {}",
        evaluation.floating_tile_value
    ));
    lines.push(format!(
        "  isolated: {}",
        yes_no(evaluation.discards_isolated_tile)
    ));
    lines.push(format!(
        "  discarded dora: {}",
        evaluation.discarded_dora_count
    ));
    lines.push(format!(
        "  discarded value honor: {}",
        evaluation.discarded_value_honor_count
    ));
    lines.push(format!(
        "  red five: {}",
        yes_no(evaluation.discards_red_five)
    ));

    if !candidate.selected {
        lines.push(format!("  lost by: {:?}", candidate.comparison_reason));
    }

    if verbose {
        lines.push(format!(
            "  standard shanten: {}",
            evaluation.shanten_after_discard.standard
        ));
        lines.push(format!(
            "  chiitoitsu shanten: {}",
            evaluation.shanten_after_discard.chiitoitsu
        ));
        lines.push(format!(
            "  kokushi shanten: {}",
            evaluation.shanten_after_discard.kokushi
        ));
        lines.push("  acceptance tiles:".to_string());
        if evaluation.acceptance_after_discard.tiles.is_empty() {
            lines.push(format!("    {NONE}"));
        }
        for tile in &evaluation.acceptance_after_discard.tiles {
            lines.push(format!(
                "    {}: {} remaining, shanten after draw {}",
                tile.tile.to_mjai_string(),
                tile.remaining,
                tile.shanten_after_draw.min()
            ));
        }
        lines.push(format!(
            "  shape breakdown: {:?}",
            candidate.shape_breakdown
        ));
        lines.push(format!("  pair context: {:?}", candidate.pair_context));
        lines.push(format!("  block context: {:?}", candidate.block_context));
        lines.push(format!(
            "  floating tile value breakdown: {:?}",
            candidate.floating_tile_value_breakdown
        ));
    }

    lines.join("\n")
}

fn format_push_pull(
    inputs: Option<&PushPullInputs>,
    decision: Option<&PushPullDecision>,
) -> String {
    let mut lines = vec!["Push/Pull".to_string()];

    if inputs.is_none() && decision.is_none() {
        lines.push("  not evaluated".to_string());
        return lines.join("\n");
    }

    if let Some(decision) = decision {
        lines.push(format!("  mode: {:?}", decision.mode));
        lines.push(format!("  reason: {:?}", decision.reason));
    }

    if let Some(inputs) = inputs {
        lines.push(format!(
            "  opponent reach count: {}",
            inputs.opponent_reach_count
        ));
        lines.push(format!("  dealer reacher: {}", inputs.dealer_reacher));
        lines.push(format!("  self dealer: {}", inputs.self_dealer));

        match inputs.offense.as_ref() {
            None => lines.push(format!("  offense: {NONE}")),
            Some(offense) => {
                lines.push("  offense".to_string());
                lines.push(format!(
                    "    min shanten after discard: {}",
                    offense.min_shanten_after_discard
                ));
                lines.push(format!(
                    "    acceptance: {} / {} types",
                    offense.acceptance_total_remaining, offense.acceptance_type_count
                ));
                lines.push(format!(
                    "    standard iishanten shape: {:?}",
                    offense.standard_iishanten_shape_after_discard
                ));
                lines.push(format!(
                    "    dora after discard: {}",
                    offense.dora_count_after_discard
                ));
                lines.push(format!(
                    "    red dora after discard: {}",
                    offense.red_dora_count_after_discard
                ));
                lines.push(format!(
                    "    value honor han proxy after discard: {}",
                    offense.value_honor_han_proxy_after_discard
                ));
            }
        }
    }

    lines.join("\n")
}

fn format_defense(defense: Option<&DefenseDecisionDiagnostic>) -> String {
    let mut lines = vec!["Defense".to_string()];

    let Some(defense) = defense else {
        lines.push("  not evaluated".to_string());
        return lines.join("\n");
    };

    lines.push("  evaluated".to_string());

    let Some(selected) = defense.selected.as_ref() else {
        lines.push(format!("  selected: {NONE}"));
        return lines.join("\n");
    };

    lines.push(format!("  selected action: {}", selected.selected_action));
    lines.push(format!("  selected kind: {:?}", selected.selected_kind));
    lines.push(format!(
        "  opponent reach count: {}",
        selected.opponent_reach_count
    ));
    lines.push(format!(
        "  genbutsu: {}",
        selected.selected_genbutsu_for_all
    ));
    lines.push(format!(
        "  honor safety: {}",
        optional(selected.selected_honor_safety_rank)
    ));
    lines.push(format!("  wall: {}", optional(selected.selected_wall_rank)));
    lines.push(format!(
        "  suji: {}",
        optional(selected.selected_suji_for_all_reached)
    ));
    lines.push(format!(
        "  suited safety: {}",
        optional(selected.selected_suited_safety_rank)
    ));

    lines.join("\n")
}

fn format_defense_candidates(defense: Option<&DefenseDecisionDiagnostic>) -> Option<String> {
    let defense = defense?;
    if defense.candidates.is_empty() {
        return None;
    }

    let mut blocks = vec!["Defense candidates".to_string()];
    for candidate in &defense.candidates {
        blocks.push(format_defense_candidate(candidate));
    }
    Some(blocks.join("\n\n"))
}

fn format_defense_candidate(candidate: &DefenseCandidateDiagnostic) -> String {
    let mut lines = vec![action_label(&candidate.action)];

    lines.push(format!("  selected: {}", yes_no(candidate.selected)));
    lines.push(format!("  genbutsu: {}", candidate.genbutsu_for_all));
    lines.push(format!(
        "  honor safety: {}",
        optional(candidate.honor_safety_rank)
    ));
    lines.push(format!("  wall: {}", optional(candidate.wall_rank)));
    lines.push(format!(
        "  suji: {}",
        optional(candidate.suji_for_all_reached)
    ));
    lines.push(format!(
        "  suited safety: {}",
        optional(candidate.suited_safety_rank)
    ));

    lines.join("\n")
}

fn discard_label(evaluation: &DiscardEvaluation) -> String {
    let mut label = evaluation.discard.to_mjai_string();
    if evaluation.discards_red_five {
        label.push('r');
    }
    label
}

pub fn action_label(action: &LegalAction) -> String {
    match action {
        LegalAction::Dahai { tile } => tile.to_mjai_string(),
        LegalAction::Reach => "Reach".to_string(),
        LegalAction::Hora => "Hora".to_string(),
        LegalAction::Ryukyoku => "Ryukyoku".to_string(),
        LegalAction::None => "None".to_string(),
        other => format!("{other:?}"),
    }
}

fn source_label(source: AgentActionSource) -> String {
    match source {
        AgentActionSource::DefenseFallback(_) => "DefenseFallback".to_string(),
        other => format!("{other:?}"),
    }
}

fn format_actions(actions: &[LegalAction]) -> String {
    if actions.is_empty() {
        return NONE.to_string();
    }
    actions
        .iter()
        .map(action_label)
        .collect::<Vec<_>>()
        .join(" ")
}

fn format_tiles(tiles: &[TileId]) -> String {
    if tiles.is_empty() {
        return NONE.to_string();
    }
    tiles
        .iter()
        .map(|tile| tile.to_mjai_string())
        .collect::<Vec<_>>()
        .join(" ")
}

fn format_wind(wind: Option<bot_logic::TileType>) -> String {
    wind.map(|wind| wind.to_mjai_string())
        .unwrap_or_else(|| "None".to_string())
}

fn format_seat(seat: Option<u8>) -> String {
    seat.map(|seat| seat.to_string())
        .unwrap_or_else(|| "None".to_string())
}

fn format_reached(context: &GameContext) -> String {
    let reached: Vec<String> = context
        .reached()
        .iter()
        .enumerate()
        .filter(|(_, reached)| **reached)
        .map(|(player, _)| player.to_string())
        .collect();

    if reached.is_empty() {
        NONE.to_string()
    } else {
        reached.join(", ")
    }
}

fn optional<T: std::fmt::Debug>(value: Option<T>) -> String {
    value
        .map(|value| format!("{value:?}"))
        .unwrap_or_else(|| ABSENT.to_string())
}

fn yes_no(value: bool) -> &'static str {
    if value { "yes" } else { "no" }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scenario::ScenarioSpec;
    use bot_core::{Agent, ShantenAgent};

    fn scenario_from_json(json: &str) -> Scenario {
        let spec: ScenarioSpec = serde_json::from_str(json).unwrap();
        Scenario::resolve(&spec).unwrap()
    }

    fn diagnose(scenario: &Scenario) -> ShantenDecisionDiagnostic {
        ShantenAgent::diagnose(&scenario.context, &scenario.legal_actions)
    }

    fn rendered(json: &str, verbose: bool) -> (Scenario, ShantenDecisionDiagnostic, String) {
        let scenario = scenario_from_json(json);
        let diagnostic = diagnose(&scenario);
        let output = format_diagnostic(&scenario, &diagnostic, verbose);
        (scenario, diagnostic, output)
    }

    fn section(output: &str, header: &str) -> String {
        output
            .split("\n\n")
            .find(|section| section.starts_with(header))
            .unwrap_or_else(|| panic!("missing section {header} in:\n{output}"))
            .to_string()
    }

    fn is_section_header(block: &str) -> bool {
        matches!(
            block.lines().next().unwrap_or_default(),
            "Scenario"
                | "Final decision"
                | "Normal discard"
                | "Normal discard candidates"
                | "Push/Pull"
                | "Defense"
                | "Defense candidates"
        )
    }

    fn candidate_block(output: &str, header: &str, tile: &str) -> String {
        let mut blocks = output
            .split("\n\n")
            .skip_while(|block| !block.starts_with(header));
        blocks.next();
        blocks
            .take_while(|block| !is_section_header(block))
            .find(|block| block.lines().next() == Some(tile))
            .unwrap_or_else(|| panic!("missing candidate {tile} in:\n{output}"))
            .to_string()
    }

    const NORMAL_SCENARIO: &str = r#"{
        "hand": "234m455p789s1123z",
        "draw": "N",
        "dora_indicators": "3p",
        "round_wind": "E",
        "seat_wind": "S",
        "player_id": 0,
        "oya": 3
    }"#;

    const DEFENSE_SCENARIO: &str = r#"{
        "hand": "19m19p1478s23467z",
        "draw": "4p",
        "dora_indicators": "3p",
        "round_wind": "E",
        "player_id": 0,
        "oya": 1,
        "reached": [false, true, false, false],
        "discards": ["", "1m 4m 7p E", "", ""]
    }"#;

    #[test]
    fn scenario_section_lists_inputs() {
        let (_, _, output) = rendered(NORMAL_SCENARIO, false);
        let scenario = section(&output, "Scenario");
        assert!(
            scenario.contains("  hand: 2m 3m 4m 4p 5p 5p 7s 8s 9s E E S W"),
            "{scenario}"
        );
        assert!(scenario.contains("  draw: N"), "{scenario}");
        assert!(scenario.contains("  dora indicators: 3p"), "{scenario}");
        assert!(scenario.contains("  round wind: E"), "{scenario}");
        assert!(scenario.contains("  seat wind: S"), "{scenario}");
        assert!(scenario.contains("  player id: 0"), "{scenario}");
        assert!(scenario.contains("  oya: 3"), "{scenario}");
    }

    #[test]
    fn scenario_section_marks_missing_inputs() {
        let (_, _, output) = rendered(r#"{"hand": "123m456p789s11z"}"#, false);
        let scenario = section(&output, "Scenario");
        assert!(scenario.contains("  draw: None"), "{scenario}");
        assert!(scenario.contains("  dora indicators: none"), "{scenario}");
        assert!(scenario.contains("  round wind: None"), "{scenario}");
        assert!(scenario.contains("  seat wind: None"), "{scenario}");
        assert!(scenario.contains("  player id: None"), "{scenario}");
        assert!(scenario.contains("  oya: None"), "{scenario}");
        assert!(scenario.contains("  reached players: none"), "{scenario}");
    }

    #[test]
    fn final_decision_comes_from_the_diagnostic() {
        let (_, diagnostic, output) = rendered(NORMAL_SCENARIO, false);
        let final_decision = section(&output, "Final decision");
        assert!(
            final_decision.contains(&format!(
                "  action: {}",
                action_label(&diagnostic.selected_action)
            )),
            "{final_decision}"
        );
        assert!(
            final_decision.contains(&format!(
                "  source: {}",
                source_label(diagnostic.selected_source)
            )),
            "{final_decision}"
        );
    }

    #[test]
    fn final_action_matches_agent_act() {
        for json in [NORMAL_SCENARIO, DEFENSE_SCENARIO] {
            let scenario = scenario_from_json(json);
            let diagnostic = diagnose(&scenario);
            let mut agent = ShantenAgent;
            let acted = agent.act(&scenario.context, &scenario.legal_actions);
            assert_eq!(diagnostic.selected_action, acted);

            let output = format_diagnostic(&scenario, &diagnostic, false);
            let final_decision = section(&output, "Final decision");
            assert!(
                final_decision.contains(&format!("  action: {}", action_label(&acted))),
                "{final_decision}"
            );
        }
    }

    #[test]
    fn normal_discard_section_shows_every_candidate() {
        let (_, diagnostic, output) = rendered(NORMAL_SCENARIO, false);
        let normal_discard = diagnostic.normal_discard.as_ref().unwrap();
        assert!(!normal_discard.candidates.is_empty());

        let candidates = section(&output, "Normal discard candidates");
        for candidate in &normal_discard.candidates {
            assert!(
                output.contains(&format!(
                    "\n{}\n  selected:",
                    discard_label(&candidate.evaluation)
                )),
                "missing candidate {} in:\n{candidates}",
                discard_label(&candidate.evaluation)
            );
        }
    }

    #[test]
    fn normal_discard_candidate_shows_default_fields() {
        let (_, diagnostic, output) = rendered(NORMAL_SCENARIO, false);
        let selected = diagnostic
            .normal_discard
            .as_ref()
            .unwrap()
            .candidates
            .iter()
            .find(|candidate| candidate.selected)
            .unwrap();
        let block = candidate_block(
            &output,
            "Normal discard candidates",
            &discard_label(&selected.evaluation),
        );

        assert!(block.contains("  selected: yes"), "{block}");
        assert!(
            block.contains(&format!(
                "  shanten: {}",
                selected.evaluation.min_shanten_after_discard()
            )),
            "{block}"
        );
        assert!(
            block.contains(&format!(
                "  acceptance: {} / {} types",
                selected.evaluation.acceptance_total_remaining(),
                selected.evaluation.acceptance_type_count()
            )),
            "{block}"
        );
        assert!(block.contains("  iishanten shape:"), "{block}");
        assert!(block.contains("  shape penalty:"), "{block}");
        assert!(block.contains("  floating tile value:"), "{block}");
        assert!(block.contains("  isolated:"), "{block}");
        assert!(block.contains("  discarded dora:"), "{block}");
        assert!(block.contains("  discarded value honor:"), "{block}");
        assert!(block.contains("  red five:"), "{block}");
        assert!(!block.contains("  lost by:"), "{block}");
    }

    #[test]
    fn normal_discard_candidate_shows_comparison_reason() {
        let (_, diagnostic, output) = rendered(NORMAL_SCENARIO, false);
        let losers: Vec<&DiscardCandidateDiagnostic> = diagnostic
            .normal_discard
            .as_ref()
            .unwrap()
            .candidates
            .iter()
            .filter(|candidate| !candidate.selected)
            .collect();
        assert!(!losers.is_empty());

        for candidate in losers {
            let block = candidate_block(
                &output,
                "Normal discard candidates",
                &discard_label(&candidate.evaluation),
            );
            assert!(block.contains("  selected: no"), "{block}");
            assert!(
                block.contains(&format!("  lost by: {:?}", candidate.comparison_reason)),
                "{block}"
            );
        }
    }

    #[test]
    fn verbose_adds_candidate_details() {
        let (_, _, default_output) = rendered(NORMAL_SCENARIO, false);
        let (_, _, verbose_output) = rendered(NORMAL_SCENARIO, true);

        assert!(!default_output.contains("standard shanten:"));
        assert!(!default_output.contains("acceptance tiles:"));
        assert!(!default_output.contains("shape breakdown:"));

        assert!(verbose_output.contains("  standard shanten:"));
        assert!(verbose_output.contains("  chiitoitsu shanten:"));
        assert!(verbose_output.contains("  kokushi shanten:"));
        assert!(verbose_output.contains("  acceptance tiles:"));
        assert!(verbose_output.contains("  shape breakdown:"));
        assert!(verbose_output.contains("  pair context:"));
        assert!(verbose_output.contains("  block context:"));
        assert!(verbose_output.contains("  floating tile value breakdown:"));
        assert!(verbose_output.len() > default_output.len());
    }

    #[test]
    fn push_pull_section_matches_diagnostic() {
        let (_, diagnostic, output) = rendered(DEFENSE_SCENARIO, false);
        let decision = diagnostic.push_pull_decision.unwrap();
        let inputs = diagnostic.push_pull_inputs.unwrap();

        let push_pull = section(&output, "Push/Pull");
        assert!(
            push_pull.contains(&format!("  mode: {:?}", decision.mode)),
            "{push_pull}"
        );
        assert!(
            push_pull.contains(&format!("  reason: {:?}", decision.reason)),
            "{push_pull}"
        );
        assert!(
            push_pull.contains(&format!(
                "  opponent reach count: {}",
                inputs.opponent_reach_count
            )),
            "{push_pull}"
        );
        assert!(
            push_pull.contains(&format!("  dealer reacher: {}", inputs.dealer_reacher)),
            "{push_pull}"
        );
        assert!(
            push_pull.contains(&format!("  self dealer: {}", inputs.self_dealer)),
            "{push_pull}"
        );

        if let Some(offense) = inputs.offense {
            assert!(
                push_pull.contains(&format!(
                    "    min shanten after discard: {}",
                    offense.min_shanten_after_discard
                )),
                "{push_pull}"
            );
            assert!(
                push_pull.contains(&format!(
                    "    acceptance: {} / {} types",
                    offense.acceptance_total_remaining, offense.acceptance_type_count
                )),
                "{push_pull}"
            );
            assert!(
                push_pull.contains(&format!(
                    "    standard iishanten shape: {:?}",
                    offense.standard_iishanten_shape_after_discard
                )),
                "{push_pull}"
            );
        }
    }

    #[test]
    fn defense_section_matches_diagnostic() {
        let (_, diagnostic, output) = rendered(DEFENSE_SCENARIO, false);
        let defense = diagnostic.defense.as_ref().unwrap();
        let selected = defense.selected.as_ref().unwrap();

        let section = section(&output, "Defense\n");
        assert!(section.contains("  evaluated"), "{section}");
        assert!(
            section.contains(&format!("  selected action: {}", selected.selected_action)),
            "{section}"
        );
        assert!(
            section.contains(&format!("  selected kind: {:?}", selected.selected_kind)),
            "{section}"
        );
    }

    #[test]
    fn defense_candidates_match_diagnostic() {
        let (_, diagnostic, output) = rendered(DEFENSE_SCENARIO, false);
        let defense = diagnostic.defense.as_ref().unwrap();
        assert!(!defense.candidates.is_empty());

        for candidate in &defense.candidates {
            let block = candidate_block(
                &output,
                "Defense candidates",
                &action_label(&candidate.action),
            );
            assert!(
                block.contains(&format!("  selected: {}", yes_no(candidate.selected))),
                "{block}"
            );
            assert!(
                block.contains(&format!("  genbutsu: {}", candidate.genbutsu_for_all)),
                "{block}"
            );
            assert!(
                block.contains(&format!(
                    "  honor safety: {}",
                    optional(candidate.honor_safety_rank)
                )),
                "{block}"
            );
            assert!(
                block.contains(&format!("  wall: {}", optional(candidate.wall_rank))),
                "{block}"
            );
            assert!(
                block.contains(&format!(
                    "  suji: {}",
                    optional(candidate.suji_for_all_reached)
                )),
                "{block}"
            );
            assert!(
                block.contains(&format!(
                    "  suited safety: {}",
                    optional(candidate.suited_safety_rank)
                )),
                "{block}"
            );
        }
    }

    #[test]
    fn defense_fallback_shows_defense_kind() {
        let (_, diagnostic, output) = rendered(DEFENSE_SCENARIO, false);
        let Some(kind) = diagnostic.defense_fallback_kind() else {
            panic!("defense scenario must end in a defense fallback:\n{output}");
        };
        let final_decision = section(&output, "Final decision");
        assert!(
            final_decision.contains("  source: DefenseFallback"),
            "{final_decision}"
        );
        assert!(
            final_decision.contains(&format!("  defense kind: {kind:?}")),
            "{final_decision}"
        );
    }

    #[test]
    fn hora_scenario_reports_unevaluated_sections() {
        let (_, diagnostic, output) = rendered(
            r#"{"hand": "123m456p789s11z", "draw": "9s", "allow_hora": true}"#,
            false,
        );
        assert_eq!(diagnostic.selected_action, LegalAction::Hora);
        assert!(diagnostic.normal_discard.is_none());
        assert!(diagnostic.push_pull_decision.is_none());
        assert!(diagnostic.defense.is_none());

        assert!(
            output.contains("Final decision\n  action: Hora\n  source: Hora"),
            "{output}"
        );
        assert!(
            output.contains("Normal discard\n  not evaluated"),
            "{output}"
        );
        assert!(output.contains("Push/Pull\n  not evaluated"), "{output}");
        assert!(output.contains("Defense\n  not evaluated"), "{output}");
    }

    #[test]
    fn evaluated_defense_without_selection_is_distinguished() {
        let (_, diagnostic, output) = rendered(
            r#"{"hand": "19m19p19s1234z", "draw": "5z", "legal_dahai": ""}"#,
            false,
        );
        assert!(diagnostic.defense.is_some());
        assert!(diagnostic.defense.as_ref().unwrap().selected.is_none());
        assert!(
            output.contains("Defense\n  evaluated\n  selected: none"),
            "{output}"
        );
    }

    #[test]
    fn red_five_is_kept_in_output() {
        let (_, diagnostic, output) = rendered(
            r#"{"hand": "234m0m5m789s112z", "draw": "N", "dora_indicators": "3p"}"#,
            false,
        );
        let candidates = &diagnostic.normal_discard.as_ref().unwrap().candidates;
        let five_man: Vec<&DiscardCandidateDiagnostic> = candidates
            .iter()
            .filter(|candidate| candidate.evaluation.discard.to_mjai_string() == "5m")
            .collect();
        assert_eq!(five_man.len(), 1);

        let scenario = scenario_from_json(
            r#"{"hand": "234m0m5m789s112z", "draw": "N", "dora_indicators": "3p"}"#,
        );
        let dahai: Vec<String> = scenario.legal_actions.iter().map(action_label).collect();
        assert!(dahai.contains(&"5m".to_string()), "{dahai:?}");
        assert!(dahai.contains(&"5mr".to_string()), "{dahai:?}");
        assert!(output.contains("5mr"), "{output}");
    }

    #[test]
    fn sections_are_separated_by_blank_lines() {
        let (_, _, output) = rendered(NORMAL_SCENARIO, false);
        assert!(output.starts_with("Scenario\n"));
        assert!(output.contains("\n\nFinal decision\n"));
        assert!(output.contains("\n\nNormal discard\n"));
        assert!(output.contains("\n\nPush/Pull\n"));
        assert!(output.contains("\n\nDefense\n"));
        assert!(!output.ends_with('\n'));
    }
}
