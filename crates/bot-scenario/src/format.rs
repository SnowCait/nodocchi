use bot_core::{
    AgentActionSource, DefenseCandidateDiagnostic, DefenseDecisionDiagnostic, DefenseFallbackKind,
    GameContext, LegalAction, Meld, PushPullDecision, PushPullInputs, ShantenAgent,
    ShantenDecisionDiagnostic,
};
use bot_logic::{
    DiscardCandidateDiagnostic, DiscardComparisonReason, DiscardDecisionDiagnostic,
    DiscardEvaluation, EffectiveShanten, FixedMeldCount, Shanten, TileId,
};

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

    sections.push(format_summary(scenario, diagnostic));

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

    for (player, melds) in context.melds().iter().enumerate() {
        if !melds.is_empty() {
            lines.push(format!("  melds[{player}]: {}", format_melds(melds)));
        }
    }
    lines.push(format!(
        "  own fixed meld count: {}",
        format_fixed_meld_count(context.own_fixed_meld_count())
    ));

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
            evaluation.shanten_after_discard.standard()
        ));
        lines.push(format!(
            "  chiitoitsu shanten: {}",
            format_concealed_only_shanten(evaluation.shanten_after_discard, |shanten| shanten
                .chiitoitsu)
        ));
        lines.push(format!(
            "  kokushi shanten: {}",
            format_concealed_only_shanten(evaluation.shanten_after_discard, |shanten| shanten
                .kokushi)
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
                lines.push(format!(
                    "    simple value proxy after discard: {}",
                    offense.simple_value_proxy_after_discard()
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
    lines.push(format!(
        "  opponent honor value: {}",
        optional(selected.selected_opponent_honor_value)
    ));
    lines.push(format!("  wall: {}", optional(selected.selected_wall_rank)));
    lines.push(format!(
        "  suji: {}",
        optional(selected.selected_suji_for_all_reached)
    ));
    lines.push(format!(
        "  suji safety: {}",
        optional(selected.selected_suji_safety_rank_for_all_reached)
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
    lines.push(format!(
        "  opponent honor value: {}",
        optional(candidate.opponent_honor_value)
    ));
    lines.push(format!("  wall: {}", optional(candidate.wall_rank)));
    lines.push(format!(
        "  suji: {}",
        optional(candidate.suji_for_all_reached)
    ));
    lines.push(format!(
        "  suji safety: {}",
        optional(candidate.suji_safety_rank_for_all_reached)
    ));
    lines.push(format!(
        "  suited safety: {}",
        optional(candidate.suited_safety_rank)
    ));

    lines.join("\n")
}

fn format_summary(scenario: &Scenario, diagnostic: &ShantenDecisionDiagnostic) -> String {
    let mut lines = vec!["Summary".to_string()];

    lines.push(format!(
        "  selected: {}",
        action_label(&diagnostic.selected_action)
    ));
    lines.push(format!(
        "  source: {}",
        source_label(diagnostic.selected_source)
    ));
    if let Some(kind) = diagnostic.defense_fallback_kind() {
        lines.push(format!("  selected detail: {kind:?}"));
    }
    if let Some(value) = honor_safety_opponent_honor_value(diagnostic) {
        lines.push(format!("  selected opponent honor value: {value}"));
    }

    let Some(runner_up) = diagnose_runner_up(scenario, diagnostic) else {
        lines.push(format!("  runner-up: {ABSENT}"));
        return lines.join("\n");
    };

    lines.push(format!(
        "  runner-up: {}",
        action_label(&runner_up.selected_action)
    ));
    lines.push(format!(
        "  runner-up source: {}",
        source_label(runner_up.selected_source)
    ));
    if let Some(kind) = runner_up.defense_fallback_kind() {
        lines.push(format!("  runner-up detail: {kind:?}"));
    }
    if let Some(value) = honor_safety_opponent_honor_value(&runner_up) {
        lines.push(format!("  runner-up opponent honor value: {value}"));
    }
    if let Some(reason) = runner_up_comparison_reason(diagnostic, &runner_up) {
        lines.push(format!("  runner-up lost by: {reason:?}"));
    }

    lines.join("\n")
}

fn honor_safety_opponent_honor_value(diagnostic: &ShantenDecisionDiagnostic) -> Option<String> {
    if !matches!(
        diagnostic.defense_fallback_kind(),
        Some(DefenseFallbackKind::HonorSafety(_))
    ) {
        return None;
    }
    let selected = diagnostic.defense.as_ref()?.selected.as_ref()?;
    Some(optional(selected.selected_opponent_honor_value))
}

fn diagnose_runner_up(
    scenario: &Scenario,
    diagnostic: &ShantenDecisionDiagnostic,
) -> Option<ShantenDecisionDiagnostic> {
    if diagnostic.selected_action == LegalAction::None {
        return None;
    }

    let runner_up_actions =
        legal_actions_without_selected(&scenario.legal_actions, &diagnostic.selected_action);
    if runner_up_actions.is_empty() {
        return None;
    }

    let runner_up = ShantenAgent::diagnose(&scenario.context, &runner_up_actions);
    if runner_up.selected_action == LegalAction::None {
        return None;
    }

    Some(runner_up)
}

fn legal_actions_without_selected(
    legal_actions: &[LegalAction],
    selected: &LegalAction,
) -> Vec<LegalAction> {
    let mut excluded = false;
    legal_actions
        .iter()
        .filter(|action| {
            if !excluded && *action == selected {
                excluded = true;
                return false;
            }
            true
        })
        .cloned()
        .collect()
}

fn runner_up_comparison_reason(
    diagnostic: &ShantenDecisionDiagnostic,
    runner_up: &ShantenDecisionDiagnostic,
) -> Option<DiscardComparisonReason> {
    if diagnostic.selected_source != AgentActionSource::NormalDiscard
        || runner_up.selected_source != AgentActionSource::NormalDiscard
    {
        return None;
    }

    let LegalAction::Dahai { tile } = &runner_up.selected_action else {
        return None;
    };

    diagnostic
        .normal_discard
        .as_ref()?
        .candidates
        .iter()
        .find(|candidate| {
            candidate.evaluation.discard == tile.tile_type()
                && candidate.evaluation.discards_red_five == tile.is_red()
        })
        .map(|candidate| candidate.comparison_reason)
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
        LegalAction::Pon { tile, consumed } => format!(
            "Pon {} <- {}",
            tile.to_mjai_string(),
            format_tiles(consumed)
        ),
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

fn format_melds(melds: &[Meld]) -> String {
    if melds.is_empty() {
        return NONE.to_string();
    }
    melds.iter().map(format_meld).collect::<Vec<_>>().join(", ")
}

fn format_meld(meld: &Meld) -> String {
    let mut label = format!("{:?} {}", meld.kind(), format_tiles(meld.tiles()));
    if let Some(called_tile) = meld.called_tile() {
        label.push_str(&format!(" (called {})", called_tile.to_mjai_string()));
    }
    label
}

// 七対子・国士のように門前でしか意味を持たない向聴数の表示。副露済み面子がある場合は
// これらを完成形候補にできないため、適当な sentinel を表示せず ABSENT にする。
fn format_concealed_only_shanten(
    shanten: EffectiveShanten,
    select: impl Fn(Shanten) -> i8,
) -> String {
    shanten
        .concealed()
        .map(|shanten| select(shanten).to_string())
        .unwrap_or_else(|| ABSENT.to_string())
}

fn format_fixed_meld_count(fixed_meld_count: Option<FixedMeldCount>) -> String {
    fixed_meld_count
        .map(|count| count.get().to_string())
        .unwrap_or_else(|| "None".to_string())
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
    use bot_core::Agent;

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
                | "Summary"
        )
    }

    fn sections(output: &str) -> Vec<&str> {
        output.split("\n\n").collect()
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

    // 単独の子リーチに対する子の Weak 一向聴。打点だけが違う対照ケース。
    const LOW_VALUE_IISHANTEN_SCENARIO: &str = r#"{
        "hand": "123456789m23p5s1z",
        "draw": "C",
        "player_id": 0,
        "oya": 2,
        "reached": [false, true, false, false],
        "extra_visible_tiles": "111444p5s",
        "legal_dahai": "C"
    }"#;

    const HIGH_VALUE_IISHANTEN_SCENARIO: &str = r#"{
        "hand": "123406789m23p0s1z",
        "draw": "C",
        "dora_indicators": "4m4s",
        "player_id": 0,
        "oya": 2,
        "reached": [false, true, false, false],
        "extra_visible_tiles": "111444p5s",
        "legal_dahai": "C"
    }"#;

    const HALF_SUJI_SCENARIO: &str = r#"{
        "hand": "444p147m258p123s7s",
        "draw": "9m",
        "player_id": 0,
        "oya": 3,
        "reached": [false, true, false, false],
        "discards": ["", "1p 4s", "", ""],
        "legal_dahai": "4p 7s"
    }"#;

    #[test]
    fn defense_shows_pure_suji_safety_rank() {
        // 4p は 1p だけ河にある片スジ、7s は 4s でスジ。bool の suji とは別に rank を表示する。
        let (_, _, output) = rendered(HALF_SUJI_SCENARIO, false);

        let four_pin = candidate_block(&output, "Defense candidates", "4p");
        assert!(four_pin.contains("  suji: false"), "{four_pin}");
        assert!(four_pin.contains("  suji safety: HalfSuji"), "{four_pin}");
        assert!(four_pin.contains("  suited safety: HalfSuji"), "{four_pin}");

        let seven_sou = candidate_block(&output, "Defense candidates", "7s");
        assert!(seven_sou.contains("  suji: true"), "{seven_sou}");
        assert!(seven_sou.contains("  suji safety: Suji"), "{seven_sou}");

        let defense = section(&output, "Defense\n");
        assert!(defense.contains("  selected action: 7s"), "{defense}");
        assert!(defense.contains("  suji safety: Suji"), "{defense}");
    }

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

    const OWN_PON_SCENARIO: &str = r#"{
        "hand": "234m455p789s",
        "draw": "N",
        "player_id": 0,
        "oya": 1,
        "discards": ["", "E", "", ""],
        "melds": [
            [{"kind": "pon", "tiles": "E E E", "called_tile": "E"}],
            [],
            [],
            []
        ]
    }"#;

    #[test]
    fn scenario_section_lists_melds_and_own_fixed_meld_count() {
        let (_, _, output) = rendered(OWN_PON_SCENARIO, false);
        let scenario = section(&output, "Scenario");
        assert!(
            scenario.contains("  melds[0]: Pon E E E (called E)"),
            "{scenario}"
        );
        assert!(scenario.contains("  own fixed meld count: 1"), "{scenario}");
    }

    #[test]
    fn scenario_section_shows_ankan_melds() {
        let (_, _, output) = rendered(
            r#"{
                "hand": "234m455p789s",
                "player_id": 0,
                "melds": [[{"kind": "ankan", "tiles": "1111z"}], [], [], []]
            }"#,
            false,
        );
        let scenario = section(&output, "Scenario");
        assert!(scenario.contains("  melds[0]: Ankan E E E E"), "{scenario}");
        assert!(!scenario.contains("(called"), "{scenario}");
        assert!(scenario.contains("  own fixed meld count: 1"), "{scenario}");
    }

    const PON_REACTION_SCENARIO: &str = include_str!("../scenarios/pon_reaction.json");

    #[test]
    fn scenario_section_shows_pon_legal_action() {
        let (_, _, output) = rendered(PON_REACTION_SCENARIO, false);
        let scenario = section(&output, "Scenario");
        assert!(
            scenario.contains("  legal actions: Pon P <- P P None"),
            "{scenario}"
        );
        assert!(scenario.contains("  own fixed meld count: 0"), "{scenario}");
        assert!(!scenario.contains("  melds["), "{scenario}");
    }

    #[test]
    fn pon_reaction_baseline_selects_none() {
        let (scenario, diagnostic, output) = rendered(PON_REACTION_SCENARIO, false);
        assert_eq!(scenario.legal_actions.len(), 2);
        assert!(matches!(scenario.legal_actions[0], LegalAction::Pon { .. }));
        assert_eq!(scenario.legal_actions[1], LegalAction::None);

        let mut agent = ShantenAgent;
        assert_eq!(
            agent.act(&scenario.context, &scenario.legal_actions),
            LegalAction::None
        );
        assert_eq!(diagnostic.selected_action, LegalAction::None);
        assert_eq!(diagnostic.selected_source, AgentActionSource::None);

        assert!(
            output.contains("Final decision\n  action: None\n  source: None"),
            "{output}"
        );
        let summary = section(&output, "Summary");
        assert_eq!(
            summary,
            "Summary\n  selected: None\n  source: None\n  runner-up: -"
        );
    }

    #[test]
    fn scenario_section_omits_meld_lines_without_melds() {
        let (_, _, output) = rendered(NORMAL_SCENARIO, false);
        let scenario = section(&output, "Scenario");
        assert!(!scenario.contains("  melds["), "{scenario}");
        assert!(scenario.contains("  own fixed meld count: 0"), "{scenario}");
    }

    #[test]
    fn scenario_section_shows_no_fixed_meld_count_without_player_id() {
        let (_, _, output) = rendered(r#"{"hand": "123m456p789s11z"}"#, false);
        let scenario = section(&output, "Scenario");
        assert!(
            scenario.contains("  own fixed meld count: None"),
            "{scenario}"
        );
    }

    // 白ポン1組 + 123456m 78p 55s + ツモ N。N を切ると副露込みの通常形テンパイ (待ち 6p / 9p)。
    const ONE_MELD_TENPAI_SCENARIO: &str = r#"{
        "hand": "123456m78p55s",
        "draw": "N",
        "player_id": 0,
        "oya": 1,
        "discards": ["", "P", "", ""],
        "melds": [
            [{"kind": "pon", "tiles": "P P P", "called_tile": "P"}],
            [],
            [],
            []
        ]
    }"#;

    #[test]
    fn normal_discard_candidates_use_the_fixed_meld_aware_evaluation() {
        let (_, _, output) = rendered(ONE_MELD_TENPAI_SCENARIO, false);
        let scenario = section(&output, "Scenario");
        assert!(scenario.contains("  own fixed meld count: 1"), "{scenario}");

        let north = candidate_block(&output, "Normal discard candidates", "N");
        assert!(north.contains("  selected: yes"), "{north}");
        assert!(north.contains("  shanten: 0"), "{north}");
        assert!(north.contains("  acceptance: 8 / 2 types"), "{north}");
    }

    #[test]
    fn fixed_meld_aware_summary_selects_the_tenpai_discard() {
        let (_, _, output) = rendered(ONE_MELD_TENPAI_SCENARIO, false);
        let summary = section(&output, "Summary");
        assert!(summary.contains("  selected: N"), "{summary}");
        assert!(summary.contains("  source: NormalDiscard"), "{summary}");
        assert!(
            summary.contains("  runner-up lost by: Shanten"),
            "{summary}"
        );

        let push_pull = section(&output, "Push/Pull");
        assert!(
            push_pull.contains("    min shanten after discard: 0"),
            "{push_pull}"
        );
        assert!(
            push_pull.contains("    acceptance: 8 / 2 types"),
            "{push_pull}"
        );
    }

    #[test]
    fn verbose_candidate_shows_no_chiitoitsu_or_kokushi_with_fixed_melds() {
        let (_, _, output) = rendered(ONE_MELD_TENPAI_SCENARIO, true);
        let north = candidate_block(&output, "Normal discard candidates", "N");

        assert!(north.contains("  standard shanten: 0"), "{north}");
        assert!(north.contains("  chiitoitsu shanten: -"), "{north}");
        assert!(north.contains("  kokushi shanten: -"), "{north}");
        assert!(
            north.contains("    6p: 4 remaining, shanten after draw -1"),
            "{north}"
        );
        assert!(
            north.contains("    9p: 4 remaining, shanten after draw -1"),
            "{north}"
        );
    }

    #[test]
    fn verbose_candidate_keeps_chiitoitsu_and_kokushi_without_melds() {
        let (_, _, output) = rendered(NORMAL_SCENARIO, true);
        let north = candidate_block(&output, "Normal discard candidates", "N");

        assert!(north.contains("  standard shanten: 2"), "{north}");
        assert!(north.contains("  chiitoitsu shanten: 4"), "{north}");
        assert!(north.contains("  kokushi shanten: 8"), "{north}");
    }

    #[test]
    fn concealed_scenario_selection_is_unchanged() {
        // 副露が無い既存 scenario の selected / runner-up は fixed meld 対応後も変わらない。
        let (_, _, output) = rendered(NORMAL_SCENARIO, false);
        let summary = section(&output, "Summary");
        assert!(summary.contains("  selected: W"), "{summary}");
        assert!(summary.contains("  runner-up: N"), "{summary}");
        assert!(
            summary.contains("  runner-up lost by: StableOrder"),
            "{summary}"
        );
    }

    #[test]
    fn diagnostic_own_fixed_meld_count_matches_the_context() {
        let (scenario, diagnostic, _) = rendered(OWN_PON_SCENARIO, false);
        assert_eq!(
            diagnostic.own_fixed_meld_count,
            scenario.context.own_fixed_meld_count()
        );
        assert_eq!(
            diagnostic.own_fixed_meld_count.map(FixedMeldCount::get),
            Some(1)
        );
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
    fn push_pull_section_shows_simple_value_proxy() {
        let (_, diagnostic, output) = rendered(DEFENSE_SCENARIO, false);
        let offense = diagnostic.push_pull_inputs.unwrap().offense.unwrap();

        let push_pull = section(&output, "Push/Pull");
        assert!(
            push_pull.contains(&format!(
                "    simple value proxy after discard: {}",
                offense.simple_value_proxy_after_discard()
            )),
            "{push_pull}"
        );
    }

    #[test]
    fn low_value_iishanten_folds_under_single_non_dealer_reach() {
        let (_, _, output) = rendered(LOW_VALUE_IISHANTEN_SCENARIO, false);
        let push_pull = section(&output, "Push/Pull");

        assert!(push_pull.contains("  mode: Fold"), "{push_pull}");
        assert!(
            push_pull.contains("  reason: IishantenUnderHighPressure"),
            "{push_pull}"
        );
        assert!(
            push_pull.contains("    min shanten after discard: 1"),
            "{push_pull}"
        );
        assert!(
            push_pull.contains("    simple value proxy after discard: 0"),
            "{push_pull}"
        );
    }

    #[test]
    fn high_value_iishanten_is_neutral_under_single_non_dealer_reach() {
        let (_, _, output) = rendered(HIGH_VALUE_IISHANTEN_SCENARIO, false);
        let push_pull = section(&output, "Push/Pull");

        assert!(push_pull.contains("  mode: Neutral"), "{push_pull}");
        assert!(
            push_pull.contains("  reason: HighValueIishantenAgainstSingleNonDealer"),
            "{push_pull}"
        );
        assert!(
            push_pull.contains("    min shanten after discard: 1"),
            "{push_pull}"
        );
        assert!(
            push_pull.contains("    simple value proxy after discard: 4"),
            "{push_pull}"
        );
    }

    #[test]
    fn value_only_differs_between_low_and_high_value_iishanten_scenarios() {
        // 打牌後の牌種構造・向聴数・受け入れが同じで、打点 proxy だけが違うことを確認する。
        let (_, low, _) = rendered(LOW_VALUE_IISHANTEN_SCENARIO, false);
        let (_, high, _) = rendered(HIGH_VALUE_IISHANTEN_SCENARIO, false);
        let low = low.push_pull_inputs.unwrap().offense.unwrap();
        let high = high.push_pull_inputs.unwrap().offense.unwrap();

        assert_eq!(
            low.min_shanten_after_discard,
            high.min_shanten_after_discard
        );
        assert_eq!(
            low.acceptance_total_remaining,
            high.acceptance_total_remaining
        );
        assert_eq!(low.acceptance_type_count, high.acceptance_type_count);
        assert_eq!(
            low.standard_iishanten_shape_after_discard,
            high.standard_iishanten_shape_after_discard
        );
        assert_eq!(low.simple_value_proxy_after_discard(), 0);
        assert_eq!(high.simple_value_proxy_after_discard(), 4);
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
                block.contains(&format!(
                    "  opponent honor value: {}",
                    optional(candidate.opponent_honor_value)
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
                    "  suji safety: {}",
                    optional(candidate.suji_safety_rank_for_all_reached)
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

    const RED_FIVE_SCENARIO: &str = r#"{
        "hand": "0m5m234m789p123s11z",
        "legal_dahai": "5m 5mr"
    }"#;

    const SINGLE_ACTION_SCENARIO: &str = r#"{
        "hand": "234m455p789s1123z",
        "draw": "N",
        "legal_dahai": "N"
    }"#;

    const REACH_SCENARIO: &str = r#"{
        "hand": "123456789m123p1s",
        "draw": "9s",
        "allow_reach": true
    }"#;

    fn without_selected_action(
        legal_actions: &[LegalAction],
        selected: &LegalAction,
    ) -> Vec<LegalAction> {
        let mut remaining = legal_actions.to_vec();
        let index = remaining
            .iter()
            .position(|action| action == selected)
            .unwrap_or_else(|| panic!("selected action must be legal"));
        remaining.remove(index);
        remaining
    }

    fn expected_runner_up(
        scenario: &Scenario,
        selected: &LegalAction,
    ) -> ShantenDecisionDiagnostic {
        let remaining = without_selected_action(&scenario.legal_actions, selected);
        ShantenAgent::diagnose(&scenario.context, &remaining)
    }

    #[test]
    fn summary_is_the_last_section() {
        for json in [
            NORMAL_SCENARIO,
            DEFENSE_SCENARIO,
            HALF_SUJI_SCENARIO,
            REACH_SCENARIO,
        ] {
            for verbose in [false, true] {
                let (_, _, output) = rendered(json, verbose);
                let sections = sections(&output);
                let last = sections.last().unwrap();
                assert!(last.starts_with("Summary\n"), "{output}");
                assert_eq!(
                    sections
                        .iter()
                        .filter(|section| section.starts_with("Summary"))
                        .count(),
                    1,
                    "{output}"
                );
                assert!(output.ends_with(last), "{output}");
            }
        }
    }

    #[test]
    fn summary_shows_normal_discard_runner_up() {
        let (scenario, diagnostic, output) = rendered(NORMAL_SCENARIO, false);
        assert_eq!(diagnostic.selected_source, AgentActionSource::NormalDiscard);

        let runner_up = expected_runner_up(&scenario, &diagnostic.selected_action);
        assert_eq!(
            runner_up.selected_source,
            AgentActionSource::NormalDiscard,
            "{output}"
        );
        assert_ne!(runner_up.selected_action, diagnostic.selected_action);

        let summary = section(&output, "Summary");
        assert!(
            summary.contains(&format!(
                "  selected: {}",
                action_label(&diagnostic.selected_action)
            )),
            "{summary}"
        );
        assert!(summary.contains("  source: NormalDiscard"), "{summary}");
        assert!(
            summary.contains(&format!(
                "  runner-up: {}",
                action_label(&runner_up.selected_action)
            )),
            "{summary}"
        );
        assert!(
            summary.contains("  runner-up source: NormalDiscard"),
            "{summary}"
        );

        let LegalAction::Dahai { tile } = &runner_up.selected_action else {
            panic!("expected a dahai runner-up:\n{summary}");
        };
        let candidate = diagnostic
            .normal_discard
            .as_ref()
            .unwrap()
            .candidates
            .iter()
            .find(|candidate| candidate.evaluation.discard == tile.tile_type())
            .unwrap_or_else(|| panic!("missing runner-up candidate:\n{output}"));
        assert!(
            summary.contains(&format!(
                "  runner-up lost by: {:?}",
                candidate.comparison_reason
            )),
            "{summary}"
        );
    }

    #[test]
    fn summary_shows_defense_fallback_details() {
        let (scenario, diagnostic, output) = rendered(HALF_SUJI_SCENARIO, false);
        let runner_up = expected_runner_up(&scenario, &diagnostic.selected_action);

        let summary = section(&output, "Summary");
        assert!(summary.contains("  selected: 7s"), "{summary}");
        assert!(summary.contains("  source: DefenseFallback"), "{summary}");
        assert!(
            summary.contains("  selected detail: SuitedSafety(Suji)"),
            "{summary}"
        );
        assert!(summary.contains("  runner-up: 4p"), "{summary}");
        assert!(
            summary.contains("  runner-up source: DefenseFallback"),
            "{summary}"
        );
        assert!(
            summary.contains("  runner-up detail: SuitedSafety(HalfSuji)"),
            "{summary}"
        );
        assert!(!summary.contains("  runner-up lost by:"), "{summary}");

        assert_eq!(
            summary,
            format!(
                "Summary\n  selected: {}\n  source: {}\n  selected detail: {:?}\n  runner-up: {}\n  runner-up source: {}\n  runner-up detail: {:?}",
                action_label(&diagnostic.selected_action),
                source_label(diagnostic.selected_source),
                diagnostic.defense_fallback_kind().unwrap(),
                action_label(&runner_up.selected_action),
                source_label(runner_up.selected_source),
                runner_up.defense_fallback_kind().unwrap(),
            )
        );
    }

    const HONOR_GUEST_VS_VALUE_SCENARIO: &str = r#"{
        "hand": "19m19p1478s23467z",
        "draw": "4p",
        "round_wind": "E",
        "player_id": 0,
        "oya": 3,
        "reached": [false, true, false, false],
        "discards": ["", "1m 4m 7p", "", ""],
        "legal_dahai": "C N"
    }"#;

    const HONOR_VALUE_VS_DOUBLE_SCENARIO: &str = r#"{
        "hand": "19m19p1478s13467z",
        "draw": "4p",
        "round_wind": "E",
        "player_id": 0,
        "oya": 1,
        "reached": [false, true, false, false],
        "discards": ["", "1m 4m 7p", "", ""],
        "legal_dahai": "E C"
    }"#;

    #[test]
    fn defense_candidates_show_opponent_honor_value() {
        let (_, _, output) = rendered(HONOR_VALUE_VS_DOUBLE_SCENARIO, false);

        let east = candidate_block(&output, "Defense candidates", "E");
        assert!(east.contains("  honor safety: OneVisible"), "{east}");
        assert!(
            east.contains("  opponent honor value: DoubleWind"),
            "{east}"
        );

        let chun = candidate_block(&output, "Defense candidates", "C");
        assert!(chun.contains("  honor safety: OneVisible"), "{chun}");
        assert!(
            chun.contains("  opponent honor value: SingleValueHonor"),
            "{chun}"
        );
        assert!(chun.contains("  selected: yes"), "{chun}");
    }

    #[test]
    fn summary_shows_opponent_honor_value_for_guest_wind_over_value_honor() {
        let (_, _, output) = rendered(HONOR_GUEST_VS_VALUE_SCENARIO, false);
        assert_eq!(
            section(&output, "Summary"),
            "Summary\n  \
             selected: N\n  \
             source: DefenseFallback\n  \
             selected detail: HonorSafety(OneVisible)\n  \
             selected opponent honor value: GuestWind\n  \
             runner-up: C\n  \
             runner-up source: DefenseFallback\n  \
             runner-up detail: HonorSafety(OneVisible)\n  \
             runner-up opponent honor value: SingleValueHonor"
        );
    }

    #[test]
    fn summary_shows_opponent_honor_value_for_value_honor_over_double_wind() {
        let (_, _, output) = rendered(HONOR_VALUE_VS_DOUBLE_SCENARIO, false);
        assert_eq!(
            section(&output, "Summary"),
            "Summary\n  \
             selected: C\n  \
             source: DefenseFallback\n  \
             selected detail: HonorSafety(OneVisible)\n  \
             selected opponent honor value: SingleValueHonor\n  \
             runner-up: E\n  \
             runner-up source: DefenseFallback\n  \
             runner-up detail: HonorSafety(OneVisible)\n  \
             runner-up opponent honor value: DoubleWind"
        );
    }

    #[test]
    fn honor_safety_selection_does_not_depend_on_legal_action_order() {
        for (json, expected) in [
            (HONOR_GUEST_VS_VALUE_SCENARIO, "N"),
            (HONOR_VALUE_VS_DOUBLE_SCENARIO, "C"),
        ] {
            let scenario = scenario_from_json(json);
            let forward = diagnose(&scenario);
            assert_eq!(action_label(&forward.selected_action), expected);

            let mut reversed_actions = scenario.legal_actions.clone();
            reversed_actions.reverse();
            let reversed = ShantenAgent::diagnose(&scenario.context, &reversed_actions);
            assert_eq!(action_label(&reversed.selected_action), expected);
        }
    }

    #[test]
    fn summary_omits_opponent_honor_value_outside_honor_safety() {
        for json in [HALF_SUJI_SCENARIO, NORMAL_SCENARIO] {
            let (_, _, output) = rendered(json, false);
            let summary = section(&output, "Summary");
            assert!(!summary.contains("opponent honor value"), "{summary}");
        }
    }

    #[test]
    fn excluding_selected_keeps_remaining_action_order() {
        let scenario = scenario_from_json(
            r#"{
                "hand": "234m455p789s1123z",
                "draw": "N",
                "legal_dahai": "4p 7s 9s"
            }"#,
        );
        let selected = scenario.legal_actions[1].clone();
        let remaining = legal_actions_without_selected(&scenario.legal_actions, &selected);
        assert_eq!(
            remaining.iter().map(action_label).collect::<Vec<_>>(),
            ["4p", "9s"]
        );
    }

    #[test]
    fn excluding_selected_keeps_the_other_five() {
        let (scenario, diagnostic, output) = rendered(RED_FIVE_SCENARIO, false);
        assert_eq!(
            scenario
                .legal_actions
                .iter()
                .map(action_label)
                .collect::<Vec<_>>(),
            ["5m", "5mr"]
        );
        assert_eq!(action_label(&diagnostic.selected_action), "5m");

        let remaining =
            legal_actions_without_selected(&scenario.legal_actions, &diagnostic.selected_action);
        assert_eq!(
            remaining.iter().map(action_label).collect::<Vec<_>>(),
            ["5mr"]
        );

        let summary = section(&output, "Summary");
        assert!(summary.contains("  selected: 5m\n"), "{summary}");
        assert!(summary.contains("  runner-up: 5mr"), "{summary}");
    }

    #[test]
    fn summary_marks_a_missing_runner_up() {
        let (scenario, _, output) = rendered(SINGLE_ACTION_SCENARIO, false);
        assert_eq!(scenario.legal_actions.len(), 1);

        let summary = section(&output, "Summary");
        assert!(summary.contains("  selected: N"), "{summary}");
        assert!(summary.contains("  runner-up: -"), "{summary}");
        assert!(!summary.contains("  runner-up source:"), "{summary}");
        assert!(!summary.contains("  runner-up detail:"), "{summary}");
        assert!(!summary.contains("  runner-up lost by:"), "{summary}");
    }

    #[test]
    fn summary_runner_up_source_can_differ_from_selected_source() {
        let (scenario, diagnostic, output) = rendered(REACH_SCENARIO, false);
        assert_eq!(diagnostic.selected_action, LegalAction::Reach);
        assert_eq!(diagnostic.selected_source, AgentActionSource::Reach);

        let runner_up = expected_runner_up(&scenario, &diagnostic.selected_action);
        assert_eq!(runner_up.selected_source, AgentActionSource::NormalDiscard);
        assert!(matches!(
            runner_up.selected_action,
            LegalAction::Dahai { .. }
        ));

        let summary = section(&output, "Summary");
        assert!(summary.contains("  selected: Reach"), "{summary}");
        assert!(summary.contains("  source: Reach"), "{summary}");
        assert!(
            summary.contains(&format!(
                "  runner-up: {}",
                action_label(&runner_up.selected_action)
            )),
            "{summary}"
        );
        assert!(
            summary.contains("  runner-up source: NormalDiscard"),
            "{summary}"
        );
        assert!(!summary.contains("  runner-up lost by:"), "{summary}");
    }

    #[test]
    fn summary_does_not_change_the_final_decision() {
        for json in [
            NORMAL_SCENARIO,
            DEFENSE_SCENARIO,
            HALF_SUJI_SCENARIO,
            REACH_SCENARIO,
        ] {
            let scenario = scenario_from_json(json);
            let diagnostic = diagnose(&scenario);
            let output = format_diagnostic(&scenario, &diagnostic, false);

            let mut agent = ShantenAgent;
            assert_eq!(
                diagnostic.selected_action,
                agent.act(&scenario.context, &scenario.legal_actions)
            );
            assert_eq!(diagnose(&scenario), diagnostic);

            let final_decision = section(&output, "Final decision");
            let summary = section(&output, "Summary");
            let action = action_label(&diagnostic.selected_action);
            let source = source_label(diagnostic.selected_source);
            assert!(
                final_decision.contains(&format!("  action: {action}\n  source: {source}")),
                "{final_decision}"
            );
            assert!(
                summary.contains(&format!("  selected: {action}\n  source: {source}")),
                "{summary}"
            );
        }
    }
}
