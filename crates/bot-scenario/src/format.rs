use bot_core::{
    AgentActionSource, CombinedDefenseCandidateDiagnostic, CombinedDefenseDiagnostic,
    DefenseCandidateDiagnostic, DefenseDecisionDiagnostic, DefenseFallbackKind, GameContext,
    LegalAction, Meld, MeldKind, MeldKindCounts, MeldThreatDiagnostic,
    OpenHandDefenseCandidateDiagnostic, OpenHandDefenseDiagnostic, OpenHandThreatAssessment,
    PlayerThreatDiagnostic, PonCandidateDiagnostic, PonDecisionDiagnostic, PushPullDecision,
    PushPullInputs, ReachDecisionDiagnostic, ShantenAgent, ShantenDecisionDiagnostic,
    ThreatDefenseTarget,
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
        format_table_state(&scenario.context),
        format_history_furiten(diagnostic),
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

    sections.push(format_open_hand_defense(&diagnostic.open_hand_defense));
    sections.push(format_combined_defense(&diagnostic.combined_defense));
    sections.push(format_player_threats(diagnostic));
    sections.push(format_summary(scenario, diagnostic));

    sections.join("\n\n")
}

fn format_history_furiten(diagnostic: &ShantenDecisionDiagnostic) -> String {
    fn value(value: Option<bool>) -> &'static str {
        match value {
            Some(true) => "true",
            Some(false) => "false",
            None => UNKNOWN,
        }
    }

    format!(
        "History furiten\n  same turn: {}\n  riichi missed win: {}",
        value(diagnostic.history_furiten.same_turn),
        value(diagnostic.history_furiten.riichi_missed_win)
    )
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

    if let Some(temporary_passed) = context.temporary_passed_tiles() {
        for (player, passed) in temporary_passed.iter().enumerate() {
            if !passed.is_empty() {
                lines.push(format!(
                    "  temporary passed[{player}]: {}",
                    format_tile_types(passed)
                ));
            }
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

// 現時点では AI policy の入力ではなく、局面調査のための観測事実として表示するだけ。
fn format_table_state(context: &GameContext) -> String {
    let table_state = context.table_state();
    [
        "Table state".to_string(),
        format!(
            "  remaining tiles: {}",
            format_optional_count(table_state.remaining_tiles)
        ),
        format!("  honba: {}", format_optional_count(table_state.honba)),
        format!(
            "  kyotaku: {}",
            format_optional_points(table_state.kyotaku_points)
        ),
        format!("  scores: {}", format_scores(table_state.scores)),
        format!("  kyoku: {}", format_optional_count(table_state.kyoku)),
    ]
    .join("\n")
}

fn format_optional_count<T: std::fmt::Display>(value: Option<T>) -> String {
    value.map_or_else(|| UNKNOWN.to_string(), |value| value.to_string())
}

fn format_optional_points(points: Option<u32>) -> String {
    points.map_or_else(|| UNKNOWN.to_string(), |points| format!("{points} points"))
}

fn format_scores(scores: Option<[i32; 4]>) -> String {
    let Some(scores) = scores else {
        return UNKNOWN.to_string();
    };
    scores
        .iter()
        .map(i32::to_string)
        .collect::<Vec<_>>()
        .join(" / ")
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
    if let Some(category) = diagnostic.open_hand_defense_category() {
        lines.push(format!("  open hand defense category: {category:?}"));
    }
    if let Some(category) = diagnostic.combined_defense_category() {
        lines.push(format!("  combined defense category: {category:?}"));
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
    lines.push(format!(
        "  weighted next acceptance: {}",
        format_tenpai_wait(candidate.next_acceptance)
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
                // 押し引きが強いテンパイを判定するときに見る事実。テンパイにならない打牌では出ない。
                match offense.tenpai_wait_after_discard.as_ref() {
                    None => lines.push(format!("    tenpai wait after discard: {NONE}")),
                    Some(wait) => {
                        lines.push("    tenpai wait after discard".to_string());
                        lines.push(format!(
                            "      live wait: {} remaining / {} types",
                            wait.tsumo_remaining, wait.tsumo_type_count
                        ));
                        lines.push(format!(
                            "      permanent furiten: {}",
                            permanent_furiten_label(wait.permanent_furiten)
                        ));
                        lines.push(format!(
                            "      ron: {}",
                            format_optional_yes_no(wait.can_ron)
                        ));
                    }
                }
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

// High OpenHandThreat 相手に対する防御 safety。診断が持つ pure helper の結果をそのまま出し、
// 表示用に安全度を計算し直さない。target がいない局面は候補を出さず、target なしと分かる表示に
// する。`selected` は production selector が選んだ結果そのもので、表示側で選び直さない。
fn format_open_hand_defense(open_hand_defense: &OpenHandDefenseDiagnostic) -> String {
    let mut header = vec![
        "OpenHand defense".to_string(),
        format!("  targets: {}", format_targets(&open_hand_defense.targets)),
    ];
    match open_hand_defense.selected.as_ref() {
        Some(selected) => {
            header.push(format!(
                "  selected action: {}",
                action_label(&selected.selected_action)
            ));
            header.push(format!(
                "  selected category: {:?}",
                selected.selected_category
            ));
        }
        None => header.push(format!("  selected: {NONE}")),
    }

    let mut blocks = vec![header.join("\n")];
    for candidate in &open_hand_defense.candidates {
        blocks.push(format_open_hand_defense_candidate(candidate));
    }
    blocks.join("\n\n")
}

fn format_targets(targets: &[usize]) -> String {
    if targets.is_empty() {
        return NONE.to_string();
    }
    targets
        .iter()
        .map(usize::to_string)
        .collect::<Vec<_>>()
        .join(", ")
}

fn format_open_hand_defense_candidate(candidate: &OpenHandDefenseCandidateDiagnostic) -> String {
    let mut lines = vec![action_label(&candidate.action)];

    lines.push(format!("  selected: {}", yes_no(candidate.selected)));
    lines.push(format!(
        "  discarded by all targets: {}",
        yes_no(candidate.discarded_by_all_targets)
    ));
    lines.push(format!(
        "  ron safe for all targets: {}",
        yes_no(candidate.ron_safe_for_all_targets)
    ));
    for target in &candidate.targets {
        lines.push(format!(
            "  discarded by target[{}]: {}",
            target.player,
            yes_no(target.discarded_by_target)
        ));
        lines.push(format!(
            "  ron safe[{}]: {}",
            target.player,
            yes_no(target.ron_safe)
        ));
    }
    lines.push(format!(
        "  honor safety: {}",
        optional(candidate.honor_safety_rank)
    ));
    lines.push(format!(
        "  opponent honor value: {}",
        optional(candidate.opponent_honor_value)
    ));
    lines.push(format!("  wall: {}", optional(candidate.wall_rank)));
    for target in &candidate.targets {
        lines.push(format!(
            "  suji safety[{}]: {}",
            target.player,
            optional(target.suji_safety_rank)
        ));
    }
    lines.push(format!(
        "  suji safety: {}",
        optional(candidate.suji_safety_rank)
    ));
    lines.push(format!(
        "  suited safety: {}",
        optional(candidate.suited_safety_rank)
    ));
    lines.push(format!("  category: {}", optional(candidate.category)));

    lines.join("\n")
}

// リーチ者と High OpenHandThreat の相手が同時にいる複合 threat 局面の防御 safety。診断が持つ
// pure helper の結果をそのまま出し、表示用に安全度を計算し直さない。複合 threat でない局面は
// target を持たないので候補も出さない。`selected` は production selector が選んだ結果そのもの。
fn format_combined_defense(combined_defense: &CombinedDefenseDiagnostic) -> String {
    let mut header = vec![
        "Combined defense".to_string(),
        format!(
            "  targets: {}",
            format_threat_targets(&combined_defense.targets)
        ),
    ];
    match combined_defense.selected.as_ref() {
        Some(selected) => {
            header.push(format!(
                "  selected action: {}",
                action_label(&selected.selected_action)
            ));
            header.push(format!(
                "  selected category: {:?}",
                selected.selected_category
            ));
        }
        None => header.push(format!("  selected: {NONE}")),
    }

    let mut blocks = vec![header.join("\n")];
    for candidate in &combined_defense.candidates {
        blocks.push(format_combined_defense_candidate(candidate));
    }
    blocks.join("\n\n")
}

// target は席だけでなく種類も出す。リーチ者と High の副露相手ではロン安全の根拠が違うため。
fn format_threat_targets(targets: &[ThreatDefenseTarget]) -> String {
    if targets.is_empty() {
        return NONE.to_string();
    }
    targets
        .iter()
        .map(|target| format!("{}({:?})", target.player, target.kind))
        .collect::<Vec<_>>()
        .join(", ")
}

fn format_combined_defense_candidate(candidate: &CombinedDefenseCandidateDiagnostic) -> String {
    let mut lines = vec![action_label(&candidate.action)];

    lines.push(format!("  selected: {}", yes_no(candidate.selected)));
    lines.push(format!(
        "  safe against all threats: {}",
        yes_no(candidate.safe_against_all_threats)
    ));
    for target in &candidate.targets {
        lines.push(format!(
            "  ron safe[{} {:?}]: {}",
            target.player(),
            target.kind(),
            yes_no(target.ron_safe)
        ));
    }
    lines.push(format!(
        "  honor safety: {}",
        optional(candidate.honor_safety_rank)
    ));
    lines.push(format!(
        "  opponent honor value: {}",
        optional(candidate.opponent_honor_value)
    ));
    lines.push(format!("  wall: {}", optional(candidate.wall_rank)));
    for target in &candidate.targets {
        lines.push(format!(
            "  suji safety[{}]: {}",
            target.player(),
            optional(target.suji_safety_rank)
        ));
    }
    lines.push(format!(
        "  suji safety: {}",
        optional(candidate.suji_safety_rank)
    ));
    lines.push(format!(
        "  suited safety: {}",
        optional(candidate.suited_safety_rank)
    ));
    lines.push(format!("  category: {}", optional(candidate.category)));

    lines.join("\n")
}

// player ごとの脅威診断。診断が持つ観測事実をそのまま出し、表示用に副露やドラを解析し直さない。
// 危険度の判断は含まず、押し引きにもまだ反映していない。
fn format_player_threats(diagnostic: &ShantenDecisionDiagnostic) -> String {
    let mut blocks = vec!["Player threats".to_string()];
    for threat in &diagnostic.player_threats {
        blocks.push(format_player_threat(threat));
    }
    blocks.join("\n\n")
}

fn format_player_threat(threat: &PlayerThreatDiagnostic) -> String {
    let facts = threat.facts;
    let mut lines = vec![format!("player {}", facts.player)];

    lines.push(format!(
        "  opponent: {}",
        format_optional_yes_no(facts.is_opponent())
    ));
    lines.push(format!("  reached: {}", yes_no(facts.reached)));
    lines.push(format!(
        "  dealer: {}",
        format_optional_yes_no(facts.is_dealer)
    ));
    lines.push(format!("  seat wind: {}", format_wind(facts.seat_wind)));
    lines.push(format!("  discards: {}", facts.discard_count));
    lines.push(format!("  melds: {}", facts.meld_count));
    lines.push(format!("  open melds: {}", facts.open_meld_count));
    lines.push(format!("  kans: {}", facts.kan_count));
    lines.push(format!(
        "  meld kinds: {}",
        format_meld_kind_counts(facts.meld_kinds)
    ));
    lines.push(format!("  meld dora: {}", facts.meld_dora_count));
    lines.push(format!("  meld red dora: {}", facts.meld_red_dora_count));
    lines.push(format!("  open meld dora: {}", facts.open_meld_dora_count));
    lines.push(format!(
        "  open meld red dora: {}",
        facts.open_meld_red_dora_count
    ));
    lines.push(format!(
        "  open confirmed value honor: {}",
        facts.open_value_honor_melds.confirmed
    ));
    lines.push(format!(
        "  open visible han proxy: {}",
        facts.open_visible_han_proxy()
    ));
    lines.push(format!(
        "  open hand threat: {}",
        format_open_hand_threat(threat.open_hand_threat)
    ));
    lines.push(format!(
        "  open hand threat reason: {}",
        optional(threat.open_hand_threat.reason())
    ));

    for (index, meld) in threat.melds.iter().enumerate() {
        lines.extend(format_meld_threat(index + 1, meld));
    }

    lines.join("\n")
}

// 対象外の席を「危険度なし」と読ませないため、level と対象外を別の表記にする。
fn format_open_hand_threat(assessment: OpenHandThreatAssessment) -> String {
    match assessment {
        OpenHandThreatAssessment::Classified(decision) => format!("{:?}", decision.level),
        OpenHandThreatAssessment::NotApplicable(exclusion) => {
            format!("not applicable ({exclusion:?})")
        }
    }
}

fn format_meld_threat(number: usize, meld: &MeldThreatDiagnostic) -> Vec<String> {
    let facts = meld.facts;
    let mut lines = vec![format!(
        "  meld {number}: {:?} {}",
        facts.kind,
        format_tiles(&meld.tiles)
    )];

    lines.push(format!("    open: {}", yes_no(facts.is_open)));
    lines.push(format!("    kan: {}", yes_no(facts.is_kan)));
    lines.push(format!("    dora: {}", facts.dora_count));
    lines.push(format!("    red dora: {}", facts.red_dora_count));

    // 役牌になり得ない Chi・数牌の刻子槓子では役牌の行自体を出さない。
    if let Some(value_honor) = facts.value_honor {
        lines.push(format!("    dragon: {}", yes_no(value_honor.is_dragon)));
        lines.push(format!(
            "    round wind: {}",
            format_optional_yes_no(value_honor.is_round_wind)
        ));
        lines.push(format!(
            "    seat wind: {}",
            format_optional_yes_no(value_honor.is_seat_wind)
        ));
    }

    lines
}

fn format_meld_kind_counts(counts: MeldKindCounts) -> String {
    let labels: Vec<String> = [
        MeldKind::Chi,
        MeldKind::Pon,
        MeldKind::Daiminkan,
        MeldKind::Ankan,
        MeldKind::Kakan,
    ]
    .into_iter()
    .filter(|&kind| counts.get(kind) > 0)
    .map(|kind| format!("{kind:?} {}", counts.get(kind)))
    .collect();

    if labels.is_empty() {
        return NONE.to_string();
    }
    labels.join(", ")
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
    if let Some(category) = diagnostic.open_hand_defense_category() {
        lines.push(format!("  selected detail: {category:?}"));
    }
    if let Some(category) = diagnostic.combined_defense_category() {
        lines.push(format!("  selected detail: {category:?}"));
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
    if let Some(category) = runner_up.open_hand_defense_category() {
        lines.push(format!("  runner-up detail: {category:?}"));
    }
    if let Some(category) = runner_up.combined_defense_category() {
        lines.push(format!("  runner-up detail: {category:?}"));
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
        AgentActionSource::OpenHandDefenseFallback(_) => "OpenHandDefenseFallback".to_string(),
        AgentActionSource::CombinedThreatDefenseFallback(_) => {
            "CombinedThreatDefenseFallback".to_string()
        }
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
                | "Table state"
                | "History furiten"
                | "Final decision"
                | "Pon"
                | "Normal discard"
                | "Normal discard candidates"
                | "Lookahead"
                | "Push/Pull"
                | "Reach"
                | "Defense"
                | "Defense candidates"
                | "OpenHand defense"
                | "Combined defense"
                | "Player threats"
                | "Summary"
        )
    }

    fn sections(output: &str) -> Vec<&str> {
        output.split("\n\n").collect()
    }

    #[test]
    fn formats_known_and_unknown_history_furiten() {
        let (_, _, known) = rendered(
            r#"{"hand":"123m","history_furiten":{"same_turn":true,"riichi_missed_win":false}}"#,
            false,
        );
        let known = section(&known, "History furiten");
        assert!(known.contains("  same turn: true"));
        assert!(known.contains("  riichi missed win: false"));

        let (_, _, unknown) = rendered(r#"{"hand":"123m"}"#, false);
        let unknown = section(&unknown, "History furiten");
        assert!(unknown.contains("  same turn: unknown"));
        assert!(unknown.contains("  riichi missed win: unknown"));
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

    // 単独の子リーチに対するテンパイ。押し引きの強いテンパイ判定を表示で確認するための局面。
    const TENPAI_UNDER_REACH_SCENARIO: &str = r#"{
        "hand": "234m 567m 88m 345p 67p",
        "draw": "N",
        "player_id": 0,
        "oya": 2,
        "reached": [false, true, false, false],
        "discards": ["1s", "9m", "", ""]
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

    const TABLE_STATE_SCENARIO: &str = r#"{
        "hand": "234m455p789s1123z",
        "draw": "N",
        "remaining_tiles": 42,
        "honba": 1,
        "kyotaku_points": 0,
        "scores": [25000, 24000, 26000, 25000],
        "kyoku": 2
    }"#;

    #[test]
    fn table_state_section_lists_every_fact() {
        let (_, _, output) = rendered(TABLE_STATE_SCENARIO, false);
        assert_eq!(
            section(&output, "Table state"),
            "Table state\n  \
             remaining tiles: 42\n  \
             honba: 1\n  \
             kyotaku: 0 points\n  \
             scores: 25000 / 24000 / 26000 / 25000\n  \
             kyoku: 2"
        );
    }

    #[test]
    fn table_state_section_marks_unknown_facts() {
        let (_, _, output) = rendered(NORMAL_SCENARIO, false);
        assert_eq!(
            section(&output, "Table state"),
            "Table state\n  \
             remaining tiles: unknown\n  \
             honba: unknown\n  \
             kyotaku: unknown\n  \
             scores: unknown\n  \
             kyoku: unknown"
        );
    }

    #[test]
    fn table_state_section_distinguishes_a_known_zero_from_unknown() {
        let (_, _, output) = rendered(
            r#"{"hand": "234m455p789s1123z", "draw": "N", "remaining_tiles": 0, "honba": 0}"#,
            false,
        );
        let table_state = section(&output, "Table state");
        assert!(
            table_state.contains("  remaining tiles: 0"),
            "{table_state}"
        );
        assert!(table_state.contains("  honba: 0"), "{table_state}");
        assert!(table_state.contains("  kyotaku: unknown"), "{table_state}");
        assert!(table_state.contains("  scores: unknown"), "{table_state}");
    }

    #[test]
    fn table_state_does_not_change_the_selected_action() {
        let (_, plain, plain_output) = rendered(NORMAL_SCENARIO, false);
        let (_, with_table_state, table_state_output) = rendered(
            r#"{
                "hand": "234m455p789s1123z",
                "draw": "N",
                "dora_indicators": "3p",
                "round_wind": "E",
                "seat_wind": "S",
                "player_id": 0,
                "oya": 3,
                "remaining_tiles": 4,
                "honba": 5,
                "kyotaku_points": 3000,
                "scores": [12300, 28700, 40100, 18900],
                "kyoku": 4
            }"#,
            false,
        );

        assert_eq!(
            with_table_state.selected_action, plain.selected_action,
            "{table_state_output}"
        );
        assert_eq!(with_table_state.selected_source, plain.selected_source);
        assert_eq!(
            with_table_state.push_pull_decision,
            plain.push_pull_decision
        );
        assert_eq!(with_table_state.reach, plain.reach);
        assert_eq!(
            section(&table_state_output, "Final decision"),
            section(&plain_output, "Final decision")
        );
        assert_eq!(
            section(&table_state_output, "Summary"),
            section(&plain_output, "Summary")
        );
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
    const RYANSHANTEN_NEXT_ACCEPTANCE_SCENARIO: &str =
        include_str!("../scenarios/ryanshanten_next_acceptance.json");

    #[test]
    fn ryanshanten_scenario_reports_weighted_next_acceptance_selection() {
        let (_, diagnostic, output) = rendered(RYANSHANTEN_NEXT_ACCEPTANCE_SCENARIO, true);
        let candidates = &diagnostic.normal_discard.as_ref().unwrap().candidates;
        assert_eq!(candidates.len(), 9, "all discard tile types must be legal");
        let selected = candidates
            .iter()
            .find(|candidate| candidate.selected)
            .unwrap();
        let runner_up = candidates
            .iter()
            .find(|candidate| candidate.evaluation.discard.to_mjai_string() == "6s")
            .unwrap();
        assert_eq!(selected.evaluation.discard.to_mjai_string(), "7p");
        assert_eq!(runner_up.evaluation.discard.to_mjai_string(), "6s");
        assert_eq!(
            runner_up.comparison_reason,
            DiscardComparisonReason::WeightedNextAcceptanceRemaining
        );
        assert!(
            selected.next_acceptance.unwrap().weighted_remaining
                > runner_up.next_acceptance.unwrap().weighted_remaining
        );
        assert!(
            output.contains("weighted next acceptance: 428 remaining / 138 types"),
            "{output}"
        );
        assert!(
            output.contains("runner-up lost by: WeightedNextAcceptanceRemaining"),
            "{output}"
        );
    }

    #[test]
    fn ryanshanten_scenario_keeps_act_and_diagnostics_consistent() {
        let scenario = scenario_from_json(RYANSHANTEN_NEXT_ACCEPTANCE_SCENARIO);
        let mut agent = ShantenAgent;
        let acted = agent.act(&scenario.context, &scenario.legal_actions);
        let diagnosed = diagnose(&scenario);
        let with_lookahead = ShantenAgent::diagnose_with_options(
            &scenario.context,
            &scenario.legal_actions,
            DiagnosticOptions::WITH_LOOKAHEAD,
        );

        assert_eq!(scenario.legal_actions.len(), 9);
        assert_eq!(acted, diagnosed.selected_action);
        assert_eq!(acted, with_lookahead.selected_action);
        assert_eq!(
            acted,
            LegalAction::Dahai {
                tile: TileId::new(60).unwrap()
            }
        );
    }

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
    fn push_pull_section_shows_the_tenpai_wait_after_discard() {
        // 押し引きが強いテンパイを判定するときに見る事実をそのまま出す。
        let (_, diagnostic, output) = rendered(TENPAI_UNDER_REACH_SCENARIO, false);
        let wait = diagnostic
            .push_pull_inputs
            .unwrap()
            .offense
            .unwrap()
            .tenpai_wait_after_discard
            .expect("テンパイの待ち facts がある");
        let push_pull = section(&output, "Push/Pull");

        assert!(
            push_pull.contains("    tenpai wait after discard\n"),
            "{push_pull}"
        );
        assert!(
            push_pull.contains(&format!(
                "      live wait: {} remaining / {} types",
                wait.tsumo_remaining, wait.tsumo_type_count
            )),
            "{push_pull}"
        );
        assert!(
            push_pull.contains(&format!(
                "      permanent furiten: {}",
                permanent_furiten_label(wait.permanent_furiten)
            )),
            "{push_pull}"
        );
        assert!(
            push_pull.contains(&format!(
                "      ron: {}",
                format_optional_yes_no(wait.can_ron)
            )),
            "{push_pull}"
        );
    }

    #[test]
    fn push_pull_section_shows_no_tenpai_wait_without_a_tenpai() {
        let (_, diagnostic, output) = rendered(LOW_VALUE_IISHANTEN_SCENARIO, false);
        let offense = diagnostic.push_pull_inputs.unwrap().offense.unwrap();
        assert_eq!(offense.min_shanten_after_discard, 1);
        assert_eq!(offense.tenpai_wait_after_discard, None);

        let push_pull = section(&output, "Push/Pull");
        assert!(
            push_pull.contains("    tenpai wait after discard: none"),
            "{push_pull}"
        );
    }

    #[test]
    fn low_value_iishanten_folds_under_single_non_dealer_reach() {
        let (_, _, output) = rendered(LOW_VALUE_IISHANTEN_SCENARIO, false);
        let push_pull = section(&output, "Push/Pull");

        assert!(push_pull.contains("  mode: Fold"), "{push_pull}");
        assert!(
            push_pull.contains("  reason: IishantenAgainstReach"),
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
    fn high_value_iishanten_also_folds_under_single_non_dealer_reach() {
        // 簡易打点 proxy は診断に出るだけで、押し引きには影響しない。
        let (_, _, output) = rendered(HIGH_VALUE_IISHANTEN_SCENARIO, false);
        let push_pull = section(&output, "Push/Pull");

        assert!(push_pull.contains("  mode: Fold"), "{push_pull}");
        assert!(
            push_pull.contains("  reason: IishantenAgainstReach"),
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
            "Player threats",
            "Summary",
        ] {
            assert_eq!(section(&without, header), section(&with, header));
        }
    }

    const OPPONENT_THREAT_SCENARIO: &str = include_str!("../scenarios/opponent_threat.json");

    // "Player threats" 直後の player ブロックを取り出す。
    fn player_threat_block(output: &str, player: usize) -> String {
        let mut blocks = output
            .split("\n\n")
            .skip_while(|block| !block.starts_with("Player threats"));
        blocks.next();
        blocks
            .take_while(|block| !is_section_header(block))
            .find(|block| block.lines().next() == Some(&format!("player {player}")))
            .unwrap_or_else(|| panic!("missing player {player} in:\n{output}"))
            .to_string()
    }

    #[test]
    fn player_threats_section_shows_every_seat() {
        let (_, diagnostic, output) = rendered(OPPONENT_THREAT_SCENARIO, false);
        assert_eq!(diagnostic.player_threats.len(), 4);

        for player in 0..4 {
            let block = player_threat_block(&output, player);
            assert!(block.contains("  melds: "), "{block}");
        }
    }

    #[test]
    fn player_threats_section_shows_the_open_hand_facts() {
        let (_, _, output) = rendered(OPPONENT_THREAT_SCENARIO, false);
        assert_eq!(
            player_threat_block(&output, 1),
            "player 1\n  \
             opponent: yes\n  \
             reached: no\n  \
             dealer: no\n  \
             seat wind: S\n  \
             discards: 0\n  \
             melds: 2\n  \
             open melds: 2\n  \
             kans: 0\n  \
             meld kinds: Chi 1, Pon 1\n  \
             meld dora: 2\n  \
             meld red dora: 1\n  \
             open meld dora: 2\n  \
             open meld red dora: 1\n  \
             open confirmed value honor: 1\n  \
             open visible han proxy: 3\n  \
             open hand threat: High\n  \
             open hand threat reason: TwoOrMoreWithVisibleHan\n  \
             meld 1: Pon P P P\n    \
             open: yes\n    \
             kan: no\n    \
             dora: 0\n    \
             red dora: 0\n    \
             dragon: yes\n    \
             round wind: no\n    \
             seat wind: no\n  \
             meld 2: Chi 4m 5mr 6m\n    \
             open: yes\n    \
             kan: no\n    \
             dora: 2\n    \
             red dora: 1"
        );
    }

    #[test]
    fn player_threats_section_keeps_ankan_out_of_the_open_meld_count() {
        let (_, _, output) = rendered(OPPONENT_THREAT_SCENARIO, false);
        let block = player_threat_block(&output, 2);

        assert!(block.contains("  melds: 1"), "{block}");
        assert!(block.contains("  open melds: 0"), "{block}");
        assert!(block.contains("  kans: 1"), "{block}");
        assert!(block.contains("  meld kinds: Ankan 1"), "{block}");
        // 暗槓の自風は fixed meld 全体では役牌でも、open meld 限定では数えない。
        assert!(block.contains("  open meld dora: 0"), "{block}");
        assert!(block.contains("  open meld red dora: 0"), "{block}");
        assert!(block.contains("  open confirmed value honor: 0"), "{block}");
        assert!(block.contains("  open hand threat: None"), "{block}");
        assert!(
            block.contains("  open hand threat reason: NoOpenMeld"),
            "{block}"
        );
        assert!(
            block.contains("  meld 1: Ankan W W W W\n    open: no\n    kan: yes"),
            "{block}"
        );
        // 自風の暗槓なので、場風ではなく自風として診断されている。
        assert!(
            block.contains("    round wind: no\n    seat wind: yes"),
            "{block}"
        );
    }

    #[test]
    fn player_threats_section_shows_the_reached_player() {
        let (_, _, output) = rendered(OPPONENT_THREAT_SCENARIO, false);
        let block = player_threat_block(&output, 3);

        assert!(block.contains("  reached: yes"), "{block}");
        assert!(block.contains("  melds: 0"), "{block}");
        // リーチ者の threat は既存のリーチ情報が source of truth なので OpenHandThreat の対象外。
        assert!(
            block.contains("  open hand threat: not applicable (Reached)"),
            "{block}"
        );
        assert!(block.contains("  open hand threat reason: -"), "{block}");
    }

    #[test]
    fn player_threats_section_shows_the_self_seat() {
        let (_, _, output) = rendered(OPPONENT_THREAT_SCENARIO, false);
        let block = player_threat_block(&output, 0);

        assert!(block.contains("  opponent: no"), "{block}");
        assert!(block.contains("  dealer: yes"), "{block}");
        assert!(block.contains("  discards: 2"), "{block}");
        assert!(
            block.contains("  open hand threat: not applicable (SelfSeat)"),
            "{block}"
        );
    }

    #[test]
    fn player_threats_section_shows_the_late_round_open_hand_threat() {
        // 1副露でも河が12枚に達した非リーチ相手は暫定 heuristic で High になる。
        let (_, diagnostic, output) = rendered(
            r#"{
                "hand": "234m 567m 88m 345p 67p",
                "draw": "N",
                "player_id": 0,
                "oya": 0,
                "discards": ["1s", "E E E E S S S S W W W W", "", ""],
                "melds": [[], [{"kind": "chi", "tiles": "1s 2s 3s", "called_tile": "1s"}], [], []]
            }"#,
            false,
        );
        let block = player_threat_block(&output, 1);

        assert_eq!(diagnostic.player_threats[1].facts.discard_count, 12);
        assert!(block.contains("  discards: 12"), "{block}");
        assert!(block.contains("  open melds: 1"), "{block}");
        assert!(block.contains("  open hand threat: High"), "{block}");
        assert!(
            block.contains("  open hand threat reason: OpenMeldFromTwelveDiscards"),
            "{block}"
        );
        // High の副露相手がいても、待ちが広い非フリテンのテンパイなら押す。
        assert!(
            section(&output, "Push/Pull").contains(
                "  mode: Push\n  reason: StrongTenpaiAgainstHighOpenHand\n  opponent reach count: 0"
            ),
            "{output}"
        );
    }

    const OPEN_HAND_DEFENSE_SCENARIO: &str = include_str!("../scenarios/open_hand_defense.json");

    #[test]
    fn open_hand_defense_section_lists_the_high_targets_and_their_safety() {
        let (_, diagnostic, output) = rendered(OPEN_HAND_DEFENSE_SCENARIO, false);
        let open_hand_defense = section(&output, "OpenHand defense");

        assert_eq!(diagnostic.open_hand_defense.targets, vec![1, 3]);
        assert!(open_hand_defense.contains("  targets: 1, 3"), "{output}");

        // 本人の河にある牌が第一分類。post_reach_passed は使わない。
        let five_man = candidate_block(&output, "OpenHand defense", "5m");
        assert!(
            five_man.contains("  discarded by all targets: yes"),
            "{five_man}"
        );
        assert!(
            five_man.contains("  discarded by target[1]: yes"),
            "{five_man}"
        );
        assert!(
            five_man.contains("  discarded by target[3]: yes"),
            "{five_man}"
        );
        assert!(
            five_man.contains("  category: DiscardedByAllTargets"),
            "{five_man}"
        );

        // 字牌は既存 Defense と同じ見え枚数の safety と役牌価値を出す。
        let east = candidate_block(&output, "OpenHand defense", "E");
        assert!(east.contains("  honor safety: OneVisible"), "{east}");
        assert!(
            east.contains("  opponent honor value: DoubleWind"),
            "{east}"
        );
        assert!(
            east.contains("  category: HonorSafety(OneVisible)"),
            "{east}"
        );

        // 数牌は target ごとのスジと、その集約 (最も危険な rank) の両方を出す。
        let three_sou = candidate_block(&output, "OpenHand defense", "3s");
        assert!(three_sou.contains("  suji safety[1]: Suji"), "{three_sou}");
        assert!(
            three_sou.contains("  suji safety[3]: NoSuji"),
            "{three_sou}"
        );
        assert!(three_sou.contains("  suji safety: NoSuji"), "{three_sou}");
        assert!(
            three_sou.contains("  suited safety: NoSafety"),
            "{three_sou}"
        );

        // 壁は見え牌由来で、スジより優先する。
        let nine_man = candidate_block(&output, "OpenHand defense", "9m");
        assert!(nine_man.contains("  wall: NoChance"), "{nine_man}");
        assert!(nine_man.contains("  suited safety: NoChance"), "{nine_man}");
    }

    #[test]
    fn open_hand_defense_section_reports_no_target_without_a_high_threat() {
        // Present の副露相手しかいない局面は「target なし」と分かる表示にする。
        let (_, diagnostic, output) = rendered(
            r#"{
                "hand": "234m 567m 88m 345p 67p",
                "draw": "N",
                "player_id": 0,
                "oya": 0,
                "discards": ["1s", "", "", ""],
                "melds": [[], [{"kind": "chi", "tiles": "1s 2s 3s", "called_tile": "1s"}], [], []]
            }"#,
            false,
        );

        assert!(!diagnostic.open_hand_defense.has_target());
        assert_eq!(
            section(&output, "OpenHand defense"),
            "OpenHand defense\n  targets: none\n  selected: none"
        );
    }

    #[test]
    fn open_hand_defense_section_is_the_production_diagnostic() {
        // 表示用に safety を計算し直さず、診断が持つ値をそのまま出す。
        let (_, diagnostic, output) = rendered(OPEN_HAND_DEFENSE_SCENARIO, false);

        for candidate in &diagnostic.open_hand_defense.candidates {
            let block = candidate_block(
                &output,
                "OpenHand defense",
                &candidate.tile.to_mjai_string(),
            );
            assert!(
                block.contains(&format!(
                    "  suited safety: {}",
                    optional(candidate.suited_safety_rank)
                )),
                "{block}"
            );
            assert!(
                block.contains(&format!("  category: {}", optional(candidate.category))),
                "{block}"
            );
        }
    }

    const COMBINED_THREAT_DEFENSE_SCENARIO: &str =
        include_str!("../scenarios/combined_threat_defense.json");

    #[test]
    fn combined_defense_section_lists_both_threat_kinds_and_their_safety() {
        let (_, diagnostic, output) = rendered(COMBINED_THREAT_DEFENSE_SCENARIO, false);
        let combined_defense = section(&output, "Combined defense");

        assert_eq!(
            diagnostic.combined_defense.targets,
            vec![
                bot_core::ThreatDefenseTarget::riichi(1),
                bot_core::ThreatDefenseTarget::high_open_hand(3),
            ]
        );
        // target は席だけでなく種類も出す。ロン安全の根拠が種類で変わるため。
        assert!(
            combined_defense.contains("  targets: 1(Riichi), 3(HighOpenHand)"),
            "{output}"
        );
        assert!(
            combined_defense.contains("  selected action: 5m"),
            "{output}"
        );
        assert!(
            combined_defense.contains("  selected category: SafeAgainstAllThreats"),
            "{output}"
        );

        // 全 target にロンされない牌が第一分類。
        let five_man = candidate_block(&output, "Combined defense", "5m");
        assert!(five_man.contains("  selected: yes"), "{five_man}");
        assert!(
            five_man.contains("  safe against all threats: yes"),
            "{five_man}"
        );
        assert!(five_man.contains("  ron safe[1 Riichi]: yes"), "{five_man}");
        assert!(
            five_man.contains("  ron safe[3 HighOpenHand]: yes"),
            "{five_man}"
        );
        assert!(
            five_man.contains("  category: SafeAgainstAllThreats"),
            "{five_man}"
        );

        // post_reach_passed はリーチ者にだけ効く。副露相手には安全根拠にしない。
        let nine_man = candidate_block(&output, "Combined defense", "9m");
        assert!(nine_man.contains("  ron safe[1 Riichi]: yes"), "{nine_man}");
        assert!(
            nine_man.contains("  ron safe[3 HighOpenHand]: no"),
            "{nine_man}"
        );
        assert!(
            nine_man.contains("  safe against all threats: no"),
            "{nine_man}"
        );

        // 字牌は既存 Defense と同じ見え枚数の safety と、ロン可能な target の役牌価値を出す。
        let east = candidate_block(&output, "Combined defense", "E");
        assert!(east.contains("  honor safety: OneVisible"), "{east}");
        assert!(
            east.contains("  opponent honor value: DoubleWind"),
            "{east}"
        );

        // 数牌は target ごとのスジと、その集約 (最も危険な rank) の両方を出す。
        let three_sou = candidate_block(&output, "Combined defense", "3s");
        assert!(three_sou.contains("  suji safety[1]: Suji"), "{three_sou}");
        assert!(
            three_sou.contains("  suji safety[3]: NoSuji"),
            "{three_sou}"
        );
        assert!(three_sou.contains("  suji safety: NoSuji"), "{three_sou}");
        assert!(
            three_sou.contains("  suited safety: NoSafety"),
            "{three_sou}"
        );
    }

    #[test]
    fn combined_defense_section_reports_no_target_without_a_combined_threat() {
        // リーチ者だけの局面は複合 threat ではないので「target なし」と分かる表示にする。
        let (_, diagnostic, output) = rendered(POST_REACH_GENBUTSU_SCENARIO, false);

        assert!(!diagnostic.combined_defense.has_target());
        assert_eq!(
            section(&output, "Combined defense"),
            "Combined defense\n  targets: none\n  selected: none"
        );
    }

    #[test]
    fn combined_defense_section_is_the_production_diagnostic() {
        // 表示用に safety を計算し直さず、診断が持つ値をそのまま出す。
        let (_, diagnostic, output) = rendered(COMBINED_THREAT_DEFENSE_SCENARIO, false);

        for candidate in &diagnostic.combined_defense.candidates {
            let block = candidate_block(
                &output,
                "Combined defense",
                &candidate.tile.to_mjai_string(),
            );
            assert!(
                block.contains(&format!(
                    "  suited safety: {}",
                    optional(candidate.suited_safety_rank)
                )),
                "{block}"
            );
            assert!(
                block.contains(&format!("  category: {}", optional(candidate.category))),
                "{block}"
            );
        }
    }

    #[test]
    fn the_combined_defense_fallback_is_reported_as_its_own_source() {
        // 最終 action の経路は既存の DefenseFallback / OpenHandDefenseFallback と区別できる。
        let (_, diagnostic, output) = rendered(COMBINED_THREAT_DEFENSE_SCENARIO, false);
        let final_decision = section(&output, "Final decision");
        let summary = section(&output, "Summary");

        assert_eq!(
            diagnostic.selected_source,
            AgentActionSource::CombinedThreatDefenseFallback(
                bot_core::CombinedDefenseCategory::SafeAgainstAllThreats
            )
        );
        assert!(
            final_decision.contains("  source: CombinedThreatDefenseFallback"),
            "{output}"
        );
        assert!(
            final_decision.contains("  combined defense category: SafeAgainstAllThreats"),
            "{output}"
        );
        assert!(!final_decision.contains("  defense kind:"), "{output}");
        assert!(
            !final_decision.contains("  open hand defense category:"),
            "{output}"
        );
        assert!(
            summary.contains("  source: CombinedThreatDefenseFallback"),
            "{output}"
        );
        assert!(
            summary.contains("  selected detail: SafeAgainstAllThreats"),
            "{output}"
        );
    }

    #[test]
    fn player_threats_open_hand_threat_is_the_production_classification() {
        // 表示用に分類し直さず、診断が持つ classification をそのまま出す。
        let (_, diagnostic, output) = rendered(OPPONENT_THREAT_SCENARIO, false);

        for (player, threat) in diagnostic.player_threats.iter().enumerate() {
            assert_eq!(
                threat.open_hand_threat,
                bot_core::classify_open_hand_threat(threat.facts),
                "player {player}"
            );

            let block = player_threat_block(&output, player);
            let expected = match threat.open_hand_threat.level() {
                Some(level) => format!("  open hand threat: {level:?}"),
                None => "  open hand threat: not applicable".to_string(),
            };
            assert!(block.contains(&expected), "{block}");
        }
    }

    #[test]
    fn player_threats_section_does_not_guess_the_self_seat_without_player_id() {
        let (_, _, output) = rendered(
            r#"{"hand": "234m455p789s1123z", "draw": "N", "melds": [[], [{"kind": "chi", "tiles": "1m 2m 3m", "called_tile": "1m"}], [], []], "discards": ["1m", "", "", ""]}"#,
            false,
        );

        for player in 0..4 {
            let block = player_threat_block(&output, player);
            assert!(block.contains("  opponent: unknown"), "{block}");
            assert!(block.contains("  dealer: unknown"), "{block}");
            assert!(block.contains("  seat wind: None"), "{block}");
            // 席が不明な相手を他家と推測して分類しない。危険度なしにも確定させない。
            assert!(
                block.contains("  open hand threat: not applicable (UnknownSeat)"),
                "{block}"
            );
        }
        assert!(
            player_threat_block(&output, 1).contains("  melds: 1"),
            "{output}"
        );
    }

    #[test]
    fn player_threats_are_the_production_diagnostic_values() {
        // 表示専用に副露やドラを解析し直さず、診断が持つ値をそのまま出す。
        let (_, diagnostic, output) = rendered(OPPONENT_THREAT_SCENARIO, false);
        let facts = diagnostic.player_threats[1].facts;
        let block = player_threat_block(&output, 1);

        assert!(
            block.contains(&format!("  melds: {}", facts.meld_count)),
            "{block}"
        );
        assert!(
            block.contains(&format!("  open melds: {}", facts.open_meld_count)),
            "{block}"
        );
        assert!(
            block.contains(&format!("  meld dora: {}", facts.meld_dora_count)),
            "{block}"
        );
        assert!(
            block.contains(&format!("  meld red dora: {}", facts.meld_red_dora_count)),
            "{block}"
        );
    }

    #[test]
    fn player_threats_section_uses_the_push_pull_threat_facts() {
        // 表示・押し引き・診断が同じ軽量 facts を共有していることを固定する。
        let (_, diagnostic, output) = rendered(OPPONENT_THREAT_SCENARIO, false);
        let inputs = diagnostic.push_pull_inputs.expect("押し引き入力がある");

        for player in 0..4 {
            assert_eq!(
                inputs.player_threats[player], diagnostic.player_threats[player].facts,
                "player {player}"
            );
        }

        let facts = inputs.player_threats[1];
        let block = player_threat_block(&output, 1);
        assert!(
            block.contains(&format!("  open melds: {}", facts.open_meld_count)),
            "{block}"
        );
        assert!(
            block.contains(&format!("  meld dora: {}", facts.meld_dora_count)),
            "{block}"
        );
        // 白ポンは役牌と確定し、暗槓した自風も確定した役牌として数える。
        assert_eq!(facts.value_honor_melds.dragon, 1);
        assert_eq!(facts.value_honor_melds.confirmed, 1);
        assert_eq!(
            inputs.player_threats[2].value_honor_melds.seat_wind, 1,
            "{output}"
        );
    }

    #[test]
    fn opponent_melds_do_not_change_the_push_pull_section() {
        // Present の副露相手だけの局面では、押し引きは従来どおり NoThreat → Push。
        let (_, diagnostic, output) = rendered(
            r#"{
                "hand": "234m455p789s1123z",
                "draw": "N",
                "player_id": 0,
                "oya": 0,
                "melds": [[], [{"kind": "pon", "tiles": "P P P", "called_tile": "P"}], [], []],
                "discards": ["P", "", "", ""]
            }"#,
            false,
        );

        assert_eq!(diagnostic.player_threats[1].facts.open_meld_count, 1);
        assert!(
            section(&output, "Push/Pull")
                .contains("  mode: Push\n  reason: NoThreat\n  opponent reach count: 0"),
            "{output}"
        );
    }
}
