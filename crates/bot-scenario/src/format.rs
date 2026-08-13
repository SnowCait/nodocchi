use bot_core::{
    AgentActionSource, DefenseCandidateDiagnostic, DefenseDecisionDiagnostic, DefenseFallbackKind,
    GameContext, LegalAction, Meld, PonCandidateDiagnostic, PonDecisionDiagnostic,
    PushPullDecision, PushPullInputs, ReachDecisionDiagnostic, ShantenAgent,
    ShantenDecisionDiagnostic,
};
use bot_logic::{
    DiscardCandidateDiagnostic, DiscardComparisonReason, DiscardDecisionDiagnostic,
    DiscardEvaluation, DiscardFuritenDiagnostic, DiscardLookaheadDiagnostic,
    DrawLookaheadDiagnostic, EffectiveShanten, FixedMeldCount, LookaheadDiagnostic,
    PermanentFuriten, Shanten, TenpaiWaitAvailability, TenpaiWaitMetric, TileId, TileType,
};

use crate::scenario::Scenario;

const NONE: &str = "none";
const ABSENT: &str = "-";
const UNKNOWN: &str = "unknown";

pub fn format_diagnostic(
    scenario: &Scenario,
    diagnostic: &ShantenDecisionDiagnostic,
    verbose: bool,
) -> String {
    let mut sections = vec![
        format_scenario(scenario, verbose),
        format_final_decision(diagnostic),
        format_pon(diagnostic.pon.as_ref(), verbose),
        format_normal_discard(diagnostic),
    ];

    if let Some(section) = format_normal_discard_candidates(
        diagnostic.normal_discard.as_ref(),
        diagnostic.normal_discard_furiten.as_deref(),
        verbose,
    ) {
        sections.push(section);
    }

    if let Some(section) = format_lookahead(diagnostic.normal_discard_lookahead.as_ref(), verbose) {
        sections.push(section);
    }

    sections.push(format_push_pull(
        diagnostic.push_pull_inputs.as_ref(),
        diagnostic.push_pull_decision.as_ref(),
    ));
    sections.push(format_reach(diagnostic.reach.as_ref()));
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

    for (player, passed) in context.post_reach_passed_tiles().iter().enumerate() {
        if !passed.is_empty() {
            lines.push(format!(
                "  post reach passed[{player}]: {}",
                format_tile_types(passed)
            ));
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

fn format_pon(pon: Option<&PonDecisionDiagnostic>, verbose: bool) -> String {
    let mut lines = vec!["Pon".to_string()];

    let Some(pon) = pon else {
        lines.push("  not evaluated".to_string());
        return lines.join("\n");
    };

    lines.push("  evaluated".to_string());
    match pon.selected.as_ref() {
        Some(action) => lines.push(format!("  selected: {}", action_label(action))),
        None => lines.push(format!("  selected: {NONE}")),
    }
    lines.push(format!("  reason: {:?}", pon.reason));
    lines.push(format!("  candidates: {}", pon.candidates.len()));

    for candidate in &pon.candidates {
        lines.extend(format_pon_candidate(candidate, verbose));
    }

    lines.join("\n")
}

fn format_pon_candidate(candidate: &PonCandidateDiagnostic, verbose: bool) -> Vec<String> {
    let mut lines = vec![format!("  {}", action_label(&candidate.action))];

    lines.push(format!("    selected: {}", yes_no(candidate.selected)));
    lines.push(format!("    eligible: {}", yes_no(candidate.eligible)));
    lines.push(format!("    reason: {:?}", candidate.reason));
    lines.push(format!("    target: {}", candidate.target.to_mjai_string()));
    lines.push(format!(
        "    value honor: {}",
        yes_no(candidate.value_honor)
    ));
    lines.push(format!(
        "    current shanten: {}",
        optional(candidate.current_shanten)
    ));
    lines.push(format!(
        "    current fixed meld count: {}",
        format_fixed_meld_count(candidate.current_fixed_meld_count)
    ));
    lines.push(format!(
        "    post-Pon fixed meld count: {}",
        format_fixed_meld_count(candidate.post_pon_fixed_meld_count)
    ));

    let Some(evaluation) = candidate.post_pon_discard.as_ref() else {
        lines.push(format!("    best discard: {ABSENT}"));
        lines.push(format!("    shanten after discard: {ABSENT}"));
        lines.push(format!("    acceptance: {ABSENT}"));
        return lines;
    };

    lines.push(format!("    best discard: {}", discard_label(evaluation)));
    lines.push(format!(
        "    shanten after discard: {}",
        evaluation.min_shanten_after_discard()
    ));
    lines.push(format!(
        "    acceptance: {} / {} types",
        evaluation.acceptance_total_remaining(),
        evaluation.acceptance_type_count()
    ));

    if verbose {
        lines.push("    acceptance tiles:".to_string());
        if evaluation.acceptance_after_discard.tiles.is_empty() {
            lines.push(format!("      {NONE}"));
        }
        for tile in &evaluation.acceptance_after_discard.tiles {
            lines.push(format!(
                "      {}: {} remaining, shanten after draw {}",
                tile.tile.to_mjai_string(),
                tile.remaining,
                tile.shanten_after_draw.min()
            ));
        }
    }

    lines
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
    furiten: Option<&[DiscardFuritenDiagnostic]>,
    verbose: bool,
) -> Option<String> {
    let normal_discard = normal_discard?;
    if normal_discard.candidates.is_empty() {
        return None;
    }

    let mut blocks = vec!["Normal discard candidates".to_string()];
    for candidate in &normal_discard.candidates {
        blocks.push(format_normal_discard_candidate(
            candidate,
            furiten_for_candidate(furiten, candidate),
            verbose,
        ));
    }
    Some(blocks.join("\n\n"))
}

// 打牌候補に対応する恒常フリテン診断。診断専用に判定し直さず、production と同じ pure helper が
// 返した結果をそのまま引く。
fn furiten_for_candidate<'a>(
    furiten: Option<&'a [DiscardFuritenDiagnostic]>,
    candidate: &DiscardCandidateDiagnostic,
) -> Option<&'a DiscardFuritenDiagnostic> {
    furiten?
        .iter()
        .find(|diagnostic| diagnostic.discard == candidate.evaluation.discard)
}

fn format_normal_discard_candidate(
    candidate: &DiscardCandidateDiagnostic,
    furiten: Option<&DiscardFuritenDiagnostic>,
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
        "  weighted tenpai wait: {}",
        format_tenpai_wait(candidate.tenpai_wait)
    ));
    lines.extend(format_permanent_furiten(furiten));
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

// 打牌選択に使った1向聴限定の前方集計値。前方評価を計算していない候補は "-" にして、
// 待ちがすべて死んでいる場合の有効な 0 と区別する。
fn format_tenpai_wait(tenpai_wait: Option<TenpaiWaitMetric>) -> String {
    match tenpai_wait {
        Some(metric) => format!(
            "{} remaining / {} types",
            metric.weighted_remaining, metric.weighted_type_count
        ),
        None => ABSENT.to_string(),
    }
}

// その打牌でテンパイになる場合の恒常フリテン診断。テンパイにならない打牌候補では何も出さない。
//
// 表示専用にフリテンを判定し直さず、production の打牌選択が使ったものと同じ pure helper
// ([`bot_logic::diagnose_discard_furiten`]) の結果をそのまま出す。
fn format_permanent_furiten(furiten: Option<&DiscardFuritenDiagnostic>) -> Vec<String> {
    let Some(tenpai) = furiten.and_then(|furiten| furiten.tenpai.as_ref()) else {
        return Vec::new();
    };

    vec![
        format!(
            "  permanent furiten: {}",
            permanent_furiten_label(tenpai.permanent_furiten())
        ),
        format!("  ron: {}", format_optional_yes_no(tenpai.can_ron())),
        // 構造上のアガリ牌種。残枚数 0 の牌種も含み、恒常フリテン判定に使う。
        format!(
            "  tenpai waits: {}",
            format_tile_types(&tenpai.structural_waits)
        ),
        // 実際に残っているツモ可能牌。見え牌を反映した既存受け入れそのもの。
        format!(
            "  live tenpai waits: {}",
            format_tile_types(&tenpai.live_waits)
        ),
        format!("  discarded waits: {}", format_discarded_waits(tenpai)),
    ]
}

fn permanent_furiten_label(status: PermanentFuriten) -> &'static str {
    match status {
        PermanentFuriten::Yes => "yes",
        PermanentFuriten::No => "no",
        PermanentFuriten::Unknown => UNKNOWN,
    }
}

// 自分の河が特定できない場合は「重複なし」と読めてしまう none ではなく ABSENT を出す。
fn format_discarded_waits(tenpai: &TenpaiWaitAvailability) -> String {
    match tenpai.permanent_furiten() {
        PermanentFuriten::Unknown => ABSENT.to_string(),
        _ => format_tile_types(tenpai.discarded_waits()),
    }
}

// 2手先診断 (lookahead) の表示。通常表示は現在の打牌候補ごとの概要だけにして出力を短く保ち、
// verbose で各受け入れ牌の詳細を出す。この節の値は選択に一切使われない解析専用の情報。
fn format_lookahead(lookahead: Option<&LookaheadDiagnostic>, verbose: bool) -> Option<String> {
    let lookahead = lookahead?;
    if lookahead.candidates.is_empty() {
        return None;
    }

    let mut lines = vec!["Lookahead".to_string()];
    for candidate in &lookahead.candidates {
        lines.extend(format_lookahead_candidate(candidate, verbose));
    }
    Some(lines.join("\n"))
}

fn format_lookahead_candidate(
    candidate: &DiscardLookaheadDiagnostic,
    verbose: bool,
) -> Vec<String> {
    let mut lines = vec![format!("  {}", candidate.discard.to_mjai_string())];

    let total_remaining: u32 = candidate
        .draws
        .iter()
        .map(|draw| u32::from(draw.remaining))
        .sum();
    lines.push(format!(
        "    draws: {} types / {} remaining",
        candidate.draws.len(),
        total_remaining
    ));

    if !verbose {
        return lines;
    }

    if candidate.draws.is_empty() {
        lines.push(format!("    {NONE}"));
    }
    for draw in &candidate.draws {
        lines.extend(format_lookahead_draw(draw));
    }
    lines
}

fn format_lookahead_draw(draw: &DrawLookaheadDiagnostic) -> Vec<String> {
    let mut lines = vec![format!(
        "    draw {}: {} remaining, shanten after draw {}",
        draw.draw.to_mjai_string(),
        draw.remaining,
        draw.shanten_after_draw.min()
    )];

    let Some(next) = draw.next_discard.as_ref() else {
        lines.push(format!("      next discard: {ABSENT}"));
        return lines;
    };

    lines.push(format!(
        "      next discard: {}",
        next.discard.to_mjai_string()
    ));
    lines.push(format!(
        "      next shanten: {}",
        next.min_shanten_after_discard()
    ));
    lines.push(format!(
        "      next acceptance: {} / {} types",
        next.acceptance_total_remaining(),
        next.acceptance_type_count()
    ));
    lines.push(format!(
        "      next iishanten shape: {:?}",
        next.standard_iishanten_shape_after_discard
    ));
    lines
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

// リーチ判断。通常打牌 selection が選んだ打牌と、その打牌後のテンパイの待ち・恒常フリテンを
// そのまま出す。表示専用に待ちやフリテンを求め直さない。
fn format_reach(reach: Option<&ReachDecisionDiagnostic>) -> String {
    let mut lines = vec!["Reach".to_string()];

    let Some(reach) = reach else {
        lines.push("  not evaluated".to_string());
        return lines.join("\n");
    };

    lines.push("  evaluated".to_string());
    lines.push(format!("  decision: {}", yes_no(reach.should_reach())));
    lines.push(format!("  reason: {:?}", reach.reason));
    match reach.selected_discard.as_ref() {
        Some(action) => lines.push(format!("  selected discard: {}", action_label(action))),
        None => lines.push(format!("  selected discard: {NONE}")),
    }
    lines.push(format!(
        "  shanten: {}",
        reach
            .shanten_after_discard
            .map_or_else(|| ABSENT.to_string(), |shanten| shanten.to_string())
    ));

    let Some(tenpai) = reach.tenpai_wait.as_ref() else {
        return lines.join("\n");
    };

    // ツモ和了できる待ち。選ばれた打牌評価の受け入れそのもので、見え牌を反映済み。
    lines.push(format!(
        "  live wait: {} remaining / {} types",
        tenpai.tsumo_remaining, tenpai.tsumo_type_count
    ));
    lines.push(format!(
        "  permanent furiten: {}",
        permanent_furiten_label(tenpai.permanent_furiten())
    ));
    lines.push(format!(
        "  ron: {}",
        format_optional_yes_no(tenpai.can_ron())
    ));
    lines.push(format!(
        "  tenpai waits: {}",
        format_tile_types(&tenpai.structural_waits)
    ));
    lines.push(format!(
        "  live tenpai waits: {}",
        format_tile_types(&tenpai.live_waits)
    ));
    lines.push(format!(
        "  discarded waits: {}",
        format_discarded_waits(tenpai)
    ));

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

fn format_tile_types(tiles: &[TileType]) -> String {
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

// 判断できない場合を「no」と読ませないための三値表示。
fn format_optional_yes_no(value: Option<bool>) -> &'static str {
    value.map_or(UNKNOWN, yes_no)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scenario::ScenarioSpec;
    use bot_core::{Agent, DiagnosticOptions, MenzenAgent};
    use bot_logic::{TileCounts, calculate_acceptance_with_visible_tiles};

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

    // 2手先診断は「打牌候補 × 受け入れ牌 × 次打牌候補」の探索になり重いため、表示の確認には
    // 小さい手牌の局面だけを使う。
    const LOOKAHEAD_SCENARIO: &str = r#"{
        "hand": "12m12p55s",
        "draw": "9p"
    }"#;

    fn rendered_with_lookahead(json: &str, verbose: bool) -> String {
        let scenario = scenario_from_json(json);
        let diagnostic = ShantenAgent::diagnose_with_options(
            &scenario.context,
            &scenario.legal_actions,
            DiagnosticOptions::WITH_LOOKAHEAD,
        );
        format_diagnostic(&scenario, &diagnostic, verbose)
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
                | "Pon"
                | "Normal discard"
                | "Normal discard candidates"
                | "Lookahead"
                | "Push/Pull"
                | "Reach"
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

    const POST_REACH_GENBUTSU_SCENARIO: &str =
        include_str!("../scenarios/post_reach_genbutsu.json");

    #[test]
    fn scenario_section_shows_post_reach_passed_tiles() {
        let (_, _, output) = rendered(POST_REACH_GENBUTSU_SCENARIO, false);
        let scenario = section(&output, "Scenario");
        assert!(
            scenario.contains("  post reach passed[1]: 4s"),
            "{scenario}"
        );
        assert!(!scenario.contains("  post reach passed[0]"), "{scenario}");
        assert!(!scenario.contains("  post reach passed[2]"), "{scenario}");
    }

    #[test]
    fn scenario_section_omits_post_reach_passed_when_empty() {
        let (_, _, output) = rendered(r#"{"hand": "234m455p789s"}"#, false);
        let scenario = section(&output, "Scenario");
        assert!(!scenario.contains("post reach passed"), "{scenario}");
    }

    #[test]
    fn post_reach_genbutsu_scenario_reports_genbutsu_in_the_defense_section() {
        let (_, _, output) = rendered(POST_REACH_GENBUTSU_SCENARIO, false);
        let defense = section(&output, "Defense");
        assert!(defense.contains("  selected action: 4s"), "{defense}");
        assert!(defense.contains("  selected kind: Genbutsu"), "{defense}");
        assert!(defense.contains("  opponent reach count: 2"), "{defense}");
        assert!(defense.contains("  genbutsu: true"), "{defense}");
    }

    #[test]
    fn post_reach_genbutsu_scenario_discards_the_passed_tile() {
        let (_, _, output) = rendered(POST_REACH_GENBUTSU_SCENARIO, false);
        assert!(
            output.contains("Final decision\n  action: 4s\n  source: DefenseFallback"),
            "{output}"
        );
    }

    const IISHANTEN_TENPAI_WAIT_SCENARIO: &str =
        include_str!("../scenarios/iishanten_tenpai_wait.json");

    // 1向聴の2候補だけを合法にした scenario から、selected と runner-up の候補診断を取り出す。
    fn iishanten_tenpai_wait_candidates() -> (
        DiscardCandidateDiagnostic,
        DiscardCandidateDiagnostic,
        String,
    ) {
        let (_, diagnostic, output) = rendered(IISHANTEN_TENPAI_WAIT_SCENARIO, false);
        let candidates = diagnostic
            .normal_discard
            .as_ref()
            .expect("normal discard evaluated")
            .candidates
            .clone();
        assert_eq!(candidates.len(), 2);

        let selected = candidates
            .iter()
            .find(|candidate| candidate.selected)
            .expect("selected candidate")
            .clone();
        let runner_up = candidates
            .iter()
            .find(|candidate| !candidate.selected)
            .expect("runner-up candidate")
            .clone();
        (selected, runner_up, output)
    }

    #[test]
    fn iishanten_tenpai_wait_scenario_prefers_the_wider_tenpai() {
        // 受け入れは runner-up の方が広いが、テンパイ後の待ちが広い候補を選ぶ。
        let (selected, runner_up, output) = iishanten_tenpai_wait_candidates();

        assert_eq!(selected.evaluation.min_shanten_after_discard(), 1);
        assert_eq!(runner_up.evaluation.min_shanten_after_discard(), 1);
        assert!(
            runner_up.evaluation.acceptance_total_remaining()
                > selected.evaluation.acceptance_total_remaining()
        );

        // 修正前の1手比較では、受け入れの多い runner-up が勝っていた。
        let before =
            bot_logic::compare_discard_evaluations(&runner_up.evaluation, &selected.evaluation);
        assert!(before.candidate_is_better);
        assert_eq!(before.reason, DiscardComparisonReason::AcceptanceRemaining);

        // 修正後は weighted tenpai wait remaining で決着する。
        assert_eq!(
            runner_up.comparison_reason,
            DiscardComparisonReason::WeightedTenpaiWaitRemaining
        );
        assert!(
            selected
                .tenpai_wait
                .expect("weighted wait")
                .weighted_remaining
                > runner_up
                    .tenpai_wait
                    .expect("weighted wait")
                    .weighted_remaining
        );
        assert!(
            output.contains("  runner-up lost by: WeightedTenpaiWaitRemaining"),
            "{output}"
        );
    }

    #[test]
    fn iishanten_tenpai_wait_scenario_shows_the_weighted_wait_of_both_candidates() {
        let (selected, runner_up, output) = iishanten_tenpai_wait_candidates();

        for candidate in [&selected, &runner_up] {
            let metric = candidate.tenpai_wait.expect("weighted wait");
            let block = candidate_block(
                &output,
                "Normal discard candidates",
                &candidate.evaluation.discard.to_mjai_string(),
            );
            assert!(
                block.contains(&format!(
                    "  weighted tenpai wait: {} remaining / {} types",
                    metric.weighted_remaining, metric.weighted_type_count
                )),
                "{block}"
            );
        }
    }

    #[test]
    fn weighted_wait_is_absent_for_non_iishanten_candidates() {
        // 1向聴以外では計算しないので、意味の無い 0 ではなく "-" を出す。
        let (_, diagnostic, output) = rendered(LOOKAHEAD_SCENARIO, false);
        let normal_discard = diagnostic
            .normal_discard
            .as_ref()
            .expect("normal discard evaluated");

        assert!(
            normal_discard
                .candidates
                .iter()
                .all(|candidate| candidate.tenpai_wait.is_none())
        );
        assert!(output.contains("  weighted tenpai wait: -"), "{output}");
        assert!(
            !output.contains("  weighted tenpai wait: 0 remaining"),
            "{output}"
        );
    }

    #[test]
    fn iishanten_tenpai_wait_scenario_selects_the_same_action_with_lookahead() {
        // act() / diagnose() / --lookahead 付き診断で selected action が一致する。
        let scenario = scenario_from_json(IISHANTEN_TENPAI_WAIT_SCENARIO);
        let mut agent = ShantenAgent;

        let acted = agent.act(&scenario.context, &scenario.legal_actions);
        let diagnostic = diagnose(&scenario);
        let with_lookahead = ShantenAgent::diagnose_with_options(
            &scenario.context,
            &scenario.legal_actions,
            DiagnosticOptions::WITH_LOOKAHEAD,
        );

        assert_eq!(diagnostic.selected_action, acted);
        assert_eq!(with_lookahead.selected_action, acted);
        assert_eq!(with_lookahead.normal_discard, diagnostic.normal_discard);
    }

    const PERMANENT_FURITEN_SCENARIO: &str = include_str!("../scenarios/permanent_furiten.json");

    // 同じ手牌・同じ待ちで、自分の河だけを変えた対照 scenario。
    const OPPONENT_RIVER_FURITEN_SCENARIO: &str = r#"{
        "hand": "123456789m123p5s",
        "draw": "9s",
        "player_id": 0,
        "oya": 0,
        "discards": ["E", "5s", "", ""],
        "legal_dahai": "9s 1m"
    }"#;

    // player_id が無く自分の河を特定できない scenario。
    const UNKNOWN_PLAYER_FURITEN_SCENARIO: &str = r#"{
        "hand": "123456789m123p5s",
        "draw": "9s",
        "discards": ["5s E", "", "", ""],
        "legal_dahai": "9s 1m"
    }"#;

    #[test]
    fn permanent_furiten_scenario_reports_the_furiten_tenpai() {
        let (_, diagnostic, output) = rendered(PERMANENT_FURITEN_SCENARIO, false);
        let block = candidate_block(&output, "Normal discard candidates", "9s");

        assert!(block.contains("  permanent furiten: yes"), "{block}");
        assert!(block.contains("  ron: no"), "{block}");
        assert!(block.contains("  tenpai waits: 5s"), "{block}");
        assert!(block.contains("  live tenpai waits: 5s"), "{block}");
        assert!(block.contains("  discarded waits: 5s"), "{block}");
        // ツモ側の残枚数・種類数は既存受け入れのままで、フリテンでも 0 に書き換えない。
        assert!(block.contains("  acceptance: 2 / 1 types"), "{block}");

        // 表示は production と同じ pure helper の結果そのもので、表示専用の判定を持たない。
        let furiten = diagnostic
            .normal_discard_furiten
            .as_ref()
            .expect("恒常フリテン診断がある")
            .iter()
            .find(|furiten| furiten.discard.to_mjai_string() == "9s")
            .expect("打 9s の候補がある");
        let tenpai = furiten.tenpai.as_ref().expect("テンパイになる");
        assert_eq!(furiten.permanent_furiten(), Some(PermanentFuriten::Yes));
        assert_eq!(tenpai.can_ron(), Some(false));
        assert_eq!(tenpai.tsumo_remaining, 2);
        assert_eq!(tenpai.tsumo_type_count, 1);
    }

    const PERMANENT_FURITEN_VISIBLE_WAIT_SCENARIO: &str =
        include_str!("../scenarios/permanent_furiten_visible_wait.json");

    #[test]
    fn a_fully_visible_discarded_wait_is_still_reported_as_furiten() {
        // 3面待ちのうち 3s を自分が捨てていて 3s が4枚とも見えている局面。3s は既存受け入れから
        // 消えるが、恒常フリテンは解除されない。
        let (_, diagnostic, output) = rendered(PERMANENT_FURITEN_VISIBLE_WAIT_SCENARIO, false);
        let block = candidate_block(&output, "Normal discard candidates", "1p");

        assert!(block.contains("  permanent furiten: yes"), "{block}");
        assert!(block.contains("  ron: no"), "{block}");
        assert!(block.contains("  tenpai waits: 3s 6s 9s"), "{block}");
        assert!(block.contains("  live tenpai waits: 6s 9s"), "{block}");
        assert!(block.contains("  discarded waits: 3s"), "{block}");
        // ツモ側は見え牌を反映した既存受け入れのまま。
        assert!(block.contains("  acceptance: 6 / 2 types"), "{block}");

        let furiten = diagnostic
            .normal_discard_furiten
            .as_ref()
            .expect("恒常フリテン診断がある")
            .iter()
            .find(|furiten| furiten.discard.to_mjai_string() == "1p")
            .expect("打 1p の候補がある");
        let tenpai = furiten.tenpai.as_ref().expect("テンパイになる");
        assert_eq!(tenpai.structural_waits.len(), 3);
        assert_eq!(tenpai.live_waits.len(), 2);
        assert_eq!(tenpai.tsumo_type_count, 2);
        assert_eq!(tenpai.tsumo_remaining, 6);
    }

    #[test]
    fn candidates_that_do_not_reach_tenpai_show_no_furiten_lines() {
        let (_, _, output) = rendered(PERMANENT_FURITEN_SCENARIO, false);
        let block = candidate_block(&output, "Normal discard candidates", "1m");

        assert!(block.contains("  shanten: 1"), "{block}");
        assert!(!block.contains("permanent furiten"), "{block}");
    }

    #[test]
    fn a_wait_in_the_opponent_river_is_not_reported_as_furiten() {
        let (_, _, output) = rendered(OPPONENT_RIVER_FURITEN_SCENARIO, false);
        let block = candidate_block(&output, "Normal discard candidates", "9s");

        assert!(block.contains("  permanent furiten: no"), "{block}");
        assert!(block.contains("  ron: yes"), "{block}");
        assert!(block.contains("  discarded waits: none"), "{block}");
    }

    #[test]
    fn an_unknown_player_id_is_not_reported_as_non_furiten() {
        let (scenario, _, output) = rendered(UNKNOWN_PLAYER_FURITEN_SCENARIO, false);
        assert_eq!(scenario.context.player_id(), None);

        let block = candidate_block(&output, "Normal discard candidates", "9s");
        assert!(block.contains("  permanent furiten: unknown"), "{block}");
        assert!(block.contains("  ron: unknown"), "{block}");
        assert!(block.contains("  discarded waits: -"), "{block}");
        assert!(!block.contains("  permanent furiten: no"), "{block}");
    }

    #[test]
    fn permanent_furiten_scenario_selects_the_same_action_everywhere() {
        // フリテン診断は事実の表現だけで、act() / diagnose() / --lookahead の選択を変えない。
        let scenario = scenario_from_json(PERMANENT_FURITEN_SCENARIO);
        let mut agent = ShantenAgent;

        let acted = agent.act(&scenario.context, &scenario.legal_actions);
        let diagnostic = diagnose(&scenario);
        let with_lookahead = ShantenAgent::diagnose_with_options(
            &scenario.context,
            &scenario.legal_actions,
            DiagnosticOptions::WITH_LOOKAHEAD,
        );

        assert_eq!(action_label(&acted), "9s");
        assert_eq!(diagnostic.selected_action, acted);
        assert_eq!(with_lookahead.selected_action, acted);
        assert_eq!(
            with_lookahead.normal_discard_furiten,
            diagnostic.normal_discard_furiten
        );
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
    fn pon_reaction_selects_the_value_honor_pon() {
        let (scenario, diagnostic, output) = rendered(PON_REACTION_SCENARIO, false);
        assert_eq!(scenario.legal_actions.len(), 2);
        assert!(matches!(scenario.legal_actions[0], LegalAction::Pon { .. }));
        assert_eq!(scenario.legal_actions[1], LegalAction::None);

        let mut agent = ShantenAgent;
        assert_eq!(
            agent.act(&scenario.context, &scenario.legal_actions),
            scenario.legal_actions[0]
        );
        assert_eq!(diagnostic.selected_action, scenario.legal_actions[0]);
        assert_eq!(diagnostic.selected_source, AgentActionSource::Pon);

        assert!(
            output.contains("Final decision\n  action: Pon P <- P P\n  source: Pon"),
            "{output}"
        );
    }

    #[test]
    fn pon_reaction_section_shows_why_the_pon_is_eligible() {
        let (_, _, output) = rendered(PON_REACTION_SCENARIO, false);
        let pon = section(&output, "Pon\n");

        assert_eq!(
            pon,
            "Pon\n  \
             evaluated\n  \
             selected: Pon P <- P P\n  \
             reason: EligibleTenpai\n  \
             candidates: 1\n  \
             Pon P <- P P\n    \
             selected: yes\n    \
             eligible: yes\n    \
             reason: EligibleTenpai\n    \
             target: P\n    \
             value honor: yes\n    \
             current shanten: 1\n    \
             current fixed meld count: 0\n    \
             post-Pon fixed meld count: 1\n    \
             best discard: N\n    \
             shanten after discard: 0\n    \
             acceptance: 8 / 2 types"
        );
    }

    #[test]
    fn verbose_pon_candidate_lists_the_live_acceptance_tiles() {
        let (_, _, output) = rendered(PON_REACTION_SCENARIO, true);
        let pon = section(&output, "Pon\n");

        assert!(pon.contains("    acceptance tiles:"), "{pon}");
        assert!(
            pon.contains("      6s: 4 remaining, shanten after draw -1"),
            "{pon}"
        );
        assert!(
            pon.contains("      9s: 4 remaining, shanten after draw -1"),
            "{pon}"
        );
    }

    #[test]
    fn pon_reaction_summary_reports_the_pon_source() {
        let (_, _, output) = rendered(PON_REACTION_SCENARIO, false);
        let summary = section(&output, "Summary");
        assert_eq!(
            summary,
            "Summary\n  selected: Pon P <- P P\n  source: Pon\n  runner-up: -"
        );
    }

    #[test]
    fn menzen_agent_keeps_none_on_the_pon_reaction_scenario() {
        let scenario = scenario_from_json(PON_REACTION_SCENARIO);

        let mut menzen = MenzenAgent::default();
        assert_eq!(
            menzen.act(&scenario.context, &scenario.legal_actions),
            LegalAction::None
        );

        let mut shanten = ShantenAgent;
        assert_eq!(
            shanten.act(&scenario.context, &scenario.legal_actions),
            scenario.legal_actions[0]
        );
    }

    #[test]
    fn pon_section_is_not_evaluated_without_a_legal_pon() {
        let (_, diagnostic, output) = rendered(NORMAL_SCENARIO, false);
        assert_eq!(diagnostic.pon, None);
        assert_eq!(section(&output, "Pon\n"), "Pon\n  not evaluated");
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

    // 打 北 で 2p / 5p の両面テンパイになり、待ちは8枚。
    const REACH_SCENARIO: &str = r#"{
        "hand": "123456789m34p55s",
        "draw": "N",
        "allow_reach": true
    }"#;

    // 打 北 で 5s 単騎テンパイになり、待ちは3枚だけ。14枚をそのまま評価すると受け入れは
    // {5s, 北} の6枚に見えるが、実際に選んだ打牌後の待ちは threshold に届かない。
    const REACH_TANKI_WAIT_SCENARIO: &str = include_str!("../scenarios/reach_tanki_wait.json");

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

    // ---- Reach 表示 ----

    #[test]
    fn reach_section_reports_the_selected_discard_and_its_wait() {
        let (_, diagnostic, output) = rendered(REACH_SCENARIO, false);
        let reach = section(&output, "Reach");
        let decision = diagnostic.reach.as_ref().expect("リーチを検討している");

        assert_eq!(diagnostic.selected_action, LegalAction::Reach);
        assert!(reach.contains("  decision: yes"), "{reach}");
        assert!(reach.contains("  reason: Eligible"), "{reach}");
        assert!(reach.contains("  selected discard: N"), "{reach}");
        assert!(reach.contains("  shanten: 0"), "{reach}");
        assert!(
            reach.contains("  live wait: 8 remaining / 2 types"),
            "{reach}"
        );
        assert!(reach.contains("  permanent furiten: unknown"), "{reach}");
        assert!(reach.contains("  ron: unknown"), "{reach}");
        assert!(reach.contains("  tenpai waits: 2p 5p"), "{reach}");
        assert!(reach.contains("  live tenpai waits: 2p 5p"), "{reach}");
        assert!(reach.contains("  discarded waits: -"), "{reach}");

        // 表示は診断が持つ値そのもので、表示専用に待ちを求め直さない。
        assert_eq!(decision.tsumo_remaining(), Some(8));
        assert_eq!(decision.tsumo_type_count(), Some(2));
        assert_eq!(
            decision.selected_discard.as_ref(),
            diagnostic.normal_discard_action.as_ref()
        );
    }

    #[test]
    fn reach_section_reports_the_insufficient_wait_of_the_selected_discard() {
        // 14枚をそのまま評価すると {5s, 北} の6枚に見えるが、実際に切る 北 を決めた後の待ちは
        // 5s の3枚だけ。リーチ判断は選んだ打牌後の待ちで行う。
        let (scenario, diagnostic, output) = rendered(REACH_TANKI_WAIT_SCENARIO, false);
        let reach = section(&output, "Reach");

        // 打牌前の14枚をそのまま評価すると threshold を満たしてしまう局面であることを固定する。
        let counts = TileCounts::from_tiles(
            scenario
                .context
                .hand_tiles()
                .iter()
                .copied()
                .chain(scenario.context.drawn_tile()),
        );
        let whole_hand =
            calculate_acceptance_with_visible_tiles(&counts, scenario.context.visible_tiles());
        assert_eq!(whole_hand.current.min(), 0);
        assert_eq!(whole_hand.total_remaining(), 6);

        assert_eq!(diagnostic.selected_source, AgentActionSource::NormalDiscard);
        assert_eq!(action_label(&diagnostic.selected_action), "N");
        assert!(reach.contains("  decision: no"), "{reach}");
        assert!(reach.contains("  reason: InsufficientLiveWait"), "{reach}");
        assert!(reach.contains("  selected discard: N"), "{reach}");
        assert!(reach.contains("  shanten: 0"), "{reach}");
        assert!(
            reach.contains("  live wait: 3 remaining / 1 types"),
            "{reach}"
        );
        assert!(reach.contains("  tenpai waits: 5s"), "{reach}");
        assert!(reach.contains("  permanent furiten: no"), "{reach}");
        assert!(reach.contains("  ron: yes"), "{reach}");
    }

    #[test]
    fn reach_section_is_not_evaluated_outside_push() {
        // 防御局面 (Fold) ではリーチを検討しない。
        let (_, diagnostic, output) = rendered(DEFENSE_SCENARIO, false);
        assert!(diagnostic.reach.is_none());
        assert_eq!(section(&output, "Reach"), "Reach\n  not evaluated");
    }

    // ---- Lookahead (2手先診断) 表示 ----

    #[test]
    fn lookahead_section_is_absent_without_the_option() {
        let (_, _, output) = rendered(LOOKAHEAD_SCENARIO, true);
        assert!(!output.contains("Lookahead"), "{output}");
    }

    #[test]
    fn lookahead_summarises_every_current_discard_candidate() {
        let output = rendered_with_lookahead(LOOKAHEAD_SCENARIO, false);
        let lookahead = section(&output, "Lookahead");
        let candidates = section(&output, "Normal discard candidates");

        for tile in ["1m", "2m", "1p", "2p", "5s", "9p"] {
            assert!(
                candidates.contains(tile) || output.contains(tile),
                "{output}"
            );
            assert!(lookahead.contains(&format!("\n  {tile}\n")), "{lookahead}");
        }
        assert!(lookahead.contains("draws: "), "{lookahead}");
        assert!(lookahead.contains(" types / "), "{lookahead}");
        // 通常表示では受け入れ牌ごとの詳細を出さない。
        assert!(!lookahead.contains("next discard:"), "{lookahead}");
    }

    #[test]
    fn verbose_lookahead_lists_each_draw() {
        let output = rendered_with_lookahead(LOOKAHEAD_SCENARIO, true);
        let lookahead = section(&output, "Lookahead");

        assert!(lookahead.contains("    draw "), "{lookahead}");
        assert!(
            lookahead.contains("remaining, shanten after draw"),
            "{lookahead}"
        );
        assert!(lookahead.contains("      next discard: "), "{lookahead}");
        assert!(lookahead.contains("      next shanten: "), "{lookahead}");
        assert!(lookahead.contains("      next acceptance: "), "{lookahead}");
        assert!(
            lookahead.contains("      next iishanten shape: "),
            "{lookahead}"
        );
    }

    #[test]
    fn lookahead_does_not_change_the_existing_sections() {
        let (_, _, without) = rendered(LOOKAHEAD_SCENARIO, true);
        let with = rendered_with_lookahead(LOOKAHEAD_SCENARIO, true);

        for header in [
            "Final decision",
            "Normal discard",
            "Push/Pull",
            "Defense",
            "Summary",
        ] {
            assert_eq!(section(&without, header), section(&with, header));
        }
    }
}
