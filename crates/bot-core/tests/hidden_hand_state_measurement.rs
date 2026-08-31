use std::time::{Duration, Instant};

use bot_core::action::LegalAction;
use bot_core::context::{GameContext, TableStateFacts};
use bot_core::defense::{
    CompressedHiddenHandStateMetrics, CompressedHiddenHandStates,
    CompressedStructuralTenpaiHiddenHandStates, DefenseFallbackKind, HiddenHandStateMetrics,
    PlayerRonRiskEvidence, ReachedHiddenHandStates, RonCapableStateWeight, TenpaiStateWeight,
    compare_lexicographic_minimax_ron_risk, select_defense_fallback_action_with_kind,
};
use bot_core::meld::{Meld, MeldKind};
use bot_core::open_hand_defense::{
    OpenHandDefenseCategory, high_open_hand_threat_players_from_context,
    select_open_hand_defense_fallback_action_with_kind,
};
use bot_logic::{TileId, TileType};

// 自分の手牌。target はここから選ぶ。prototype の target は「今から自分が捨てる物理牌」なので、
// その1枚が visible_tiles に含まれている局面でしか正しい残枚数にならない。
const REPRESENTATIVE_HAND: [&str; 13] = [
    "2m", "3m", "4m", "6m", "7m", "3p", "4p", "5p", "7p", "8p", "2s", "3s", "E",
];

// 34牌種すべてが未知領域に残る representative な局面を組み立てるための物理牌 allocator。
struct TileSource {
    used: [u8; TileType::COUNT],
}

impl TileSource {
    fn new() -> Self {
        Self {
            used: [0; TileType::COUNT],
        }
    }

    fn tiles(&mut self, mjai: &[&str]) -> Vec<TileId> {
        mjai.iter().map(|value| self.tile(value)).collect()
    }

    fn tile(&mut self, mjai: &str) -> TileId {
        let tile_type = tile_type(mjai);
        let copy = &mut self.used[tile_type.index()];
        let id = TileId::new(tile_type.raw() * 4 + *copy).expect("at most four copies");
        *copy += 1;
        id
    }
}

fn tile_type(mjai: &str) -> TileType {
    TileType::from_mjai_type_str(mjai).expect("valid mjai tile")
}

// player 1 がリーチしている中盤の局面。見え牌は自分の手牌・4人の河・ドラ表示牌。
fn representative_context() -> GameContext {
    representative_context_with_reached([false, true, false, false])
}

fn representative_context_with_reached(reached: [bool; 4]) -> GameContext {
    let mut source = TileSource::new();
    let hand = source.tiles(&REPRESENTATIVE_HAND);
    let dora_indicator = source.tile("9s");
    let discards = [
        source.tiles(&["1m", "9m", "N", "1s", "C", "9p"]),
        source.tiles(&["9m", "1p", "W", "F", "8s", "1m"]),
        source.tiles(&["P", "S", "1s", "9s", "2p", "6s"]),
        source.tiles(&["N", "C", "1p", "9p", "7s", "4s"]),
    ];

    let mut visible = hand.clone();
    visible.push(dora_indicator);
    for river in &discards {
        visible.extend(river.iter().copied());
    }

    GameContext::from_parts_with_table_state(
        None,
        hand,
        vec![dora_indicator],
        Some(tile_type("E")),
        Some(tile_type("S")),
        visible,
        Some(0),
        Some(0),
        discards,
        reached,
    )
    .with_post_reach_passed_tiles([vec![], vec![tile_type("4s")], vec![], vec![]])
}

#[test]
#[ignore = "release build 前提の multi-reach production-like fallback 計測用。wall-clock threshold は持たない"]
fn measure_multi_reach_exact_defense_fallback_selection() {
    for reached in [[false, true, true, false], [false, true, true, true]] {
        let context = representative_context_with_reached(reached);
        let actions: Vec<LegalAction> = context
            .hand_tiles()
            .iter()
            .copied()
            .map(|tile| LegalAction::Dahai { tile })
            .collect();
        let mut targets = Vec::new();
        let mut seen = [false; TileType::COUNT];
        for action in &actions {
            let LegalAction::Dahai { tile } = action else {
                continue;
            };
            let target = tile.tile_type();
            if !seen[target.index()] {
                seen[target.index()] = true;
                targets.push(target);
            }
        }

        let players = context.reached_opponents();
        let mut vectors = vec![Vec::with_capacity(players.len()); targets.len()];
        let mut construction_total = Duration::ZERO;
        let mut target_evaluation_total = Duration::ZERO;
        for &player in &players {
            let start = Instant::now();
            let mut states =
                CompressedHiddenHandStates::new(player, &context).expect("reached player");
            let construction = start.elapsed();
            construction_total += construction;

            let start = Instant::now();
            for (vector, &target) in vectors.iter_mut().zip(&targets) {
                vector.push(PlayerRonRiskEvidence {
                    player,
                    evidence: states.ron_risk_evidence(target),
                });
            }
            let target_evaluation = start.elapsed();
            target_evaluation_total += target_evaluation;
            println!(
                "  player {player}: construction={construction:?}, all targets={target_evaluation:?}"
            );
        }

        let start = Instant::now();
        let mut best = 0;
        for candidate in 1..vectors.len() {
            if compare_lexicographic_minimax_ron_risk(&vectors[candidate], &vectors[best])
                == Some(std::cmp::Ordering::Less)
            {
                best = candidate;
            }
        }
        let risk_vector_comparison = start.elapsed();

        let start = Instant::now();
        let selected = select_defense_fallback_action_with_kind(&context, &actions)
            .expect("representative multi-reach fallback");
        let total_selection = start.elapsed();
        assert_eq!(selected.1, DefenseFallbackKind::ExactRonRisk);

        println!(
            "production-like {}-reach exact minimax defense fallback:",
            players.len()
        );
        println!("  legal Dahai actions:          {}", actions.len());
        println!("  unique TileType evaluations:  {}", targets.len());
        println!("  construction total:           {construction_total:?}");
        println!("  all target evaluations total: {target_evaluation_total:?}");
        println!("  risk-vector comparison:       {risk_vector_comparison:?}");
        println!("  total fallback selection:     {total_selection:?}");
        println!(
            "  comparator best target:       {}",
            targets[best].to_mjai_string()
        );
        println!("  production selected:          {:?}", selected.0);
    }
}

// target は自分が今から捨てる牌なので、必ず自分の手牌にある牌種から選ぶ。
fn held_target(mjai: &str) -> TileType {
    assert!(
        REPRESENTATIVE_HAND.contains(&mjai),
        "target {mjai} is not held in the representative hand"
    );
    tile_type(mjai)
}

fn report(weight: RonCapableStateWeight, metrics: HiddenHandStateMetrics, total: Duration) {
    println!("  weight={} states={}", weight.weight, weight.states);
    println!(
        "  generated candidates={} evaluated states={} completion checks={}",
        metrics.generated_candidates, metrics.evaluated_states, metrics.completion_checks
    );
    println!(
        "  candidate generation + dedup: {:?}",
        metrics.candidate_generation
    );
    println!(
        "  unron filtering:              {:?}",
        metrics.unron_filtering
    );
    println!(
        "  target completion:            {:?}",
        metrics.target_completion
    );
    println!("  total:                        {total:?}");
}

#[test]
#[ignore = "release build 前提の計測用。wall-clock threshold は持たない"]
fn measure_one_target_ron_capable_hidden_hand_weight() {
    let context = representative_context();
    let target = held_target("5p");

    let mut states = ReachedHiddenHandStates::new(1, &context).expect("menzen reached player");
    let start = Instant::now();
    let weight = states.ron_capable_state_weight(target);
    let elapsed = start.elapsed();

    println!(
        "1 player / 1 target ({}), unron tiles={}:",
        target.to_mjai_string(),
        states.unron_capable_tiles().len()
    );
    report(weight, states.metrics(), elapsed);
}

#[test]
#[ignore = "release build 前提の計測用。wall-clock threshold は持たない"]
fn measure_multiple_targets_ron_capable_hidden_hand_weight() {
    let context = representative_context();
    let targets: Vec<TileType> = ["6m", "5p", "E"]
        .iter()
        .map(|mjai| held_target(mjai))
        .collect();

    let mut states = ReachedHiddenHandStates::new(1, &context).expect("menzen reached player");
    let start = Instant::now();
    let mut total = RonCapableStateWeight::default();
    for target in &targets {
        let weight = states.ron_capable_state_weight(*target);
        total.weight += weight.weight;
        total.states += weight.states;
        println!(
            "  {} done at {:?}",
            target.to_mjai_string(),
            start.elapsed()
        );
    }
    let elapsed = start.elapsed();

    println!("1 player / {} targets:", targets.len());
    report(total, states.metrics(), elapsed);
    println!("  cached states: {}", states.evaluated_state_count());
}

fn compressed_report(
    tenpai: TenpaiStateWeight,
    metrics: CompressedHiddenHandStateMetrics,
    player_construction: Duration,
    evidence_evaluation: Duration,
) {
    println!("  T={} (tenpai states={})", tenpai.weight, tenpai.states);
    println!(
        "  enumerated group vectors={} retained group classes={}",
        metrics.enumerated_group_vectors, metrics.retained_group_classes
    );
    println!(
        "  collapsed tenpai classes={} tenpai dp transitions={}",
        metrics.collapsed_tenpai_classes, metrics.tenpai_dp_transitions
    );
    println!(
        "  collapsed target classes={} target dp transitions={}",
        metrics.collapsed_target_classes, metrics.dp_transitions
    );
    println!("  block tables:                 {:?}", metrics.block_tables);
    println!(
        "  player group precomputation:  {:?}",
        metrics.precomputation
    );
    println!(
        "  T(p) calculation:             {:?}",
        metrics.tenpai_calculation
    );
    println!(
        "  target evaluation:            {:?}",
        metrics.target_evaluation
    );
    println!("  player construction total:    {player_construction:?}");
    println!("  R/T evidence wall time:        {evidence_evaluation:?}");
}

fn open_hand_ron_enumerator_report(metrics: HiddenHandStateMetrics) {
    assert_eq!(
        metrics.completion_checks,
        metrics.furiten_completion_checks + metrics.target_completion_checks
    );
    assert_eq!(
        metrics.ron_capable_states,
        metrics.guaranteed_yaku_shortcuts
            + metrics.yaku_successful_states
            + metrics.yakuman_successful_states
    );

    println!("  R enumerator:");
    println!("    candidate generation:");
    println!(
        "      generated candidates:     {}",
        metrics.generated_candidates
    );
    println!(
        "      unique target states:     {}",
        metrics.unique_candidates
    );
    println!(
        "      elapsed:                  {:?}",
        metrics.candidate_generation
    );
    println!("    dedup/cache:");
    println!("      hits:                     {}", metrics.cache_hits);
    println!("      misses/evaluated states: {}", metrics.cache_misses);
    println!("      clears:                   {}", metrics.cache_clears);
    println!("      cached states:            {}", metrics.cached_states);
    println!("    furiten filtering:");
    println!(
        "      states checked:           {}",
        metrics.furiten_states_checked
    );
    println!(
        "      states filtered:          {}",
        metrics.furiten_states_filtered
    );
    println!(
        "      completion checks:        {}",
        metrics.furiten_completion_checks
    );
    println!(
        "      elapsed:                  {:?}",
        metrics.unron_filtering
    );
    println!("    target structural completion:");
    println!(
        "      completion checks:        {}",
        metrics.target_completion_checks
    );
    println!(
        "      completed states:         {}",
        metrics.completed_states
    );
    println!(
        "      elapsed:                  {:?}",
        metrics.target_completion
    );
    println!(
        "    guaranteed-yaku shortcuts: {}",
        metrics.guaranteed_yaku_shortcuts
    );
    println!("    yaku evaluation:");
    println!(
        "      evaluations:              {}",
        metrics.yaku_evaluations
    );
    println!(
        "      successful states:        {}",
        metrics.yaku_successful_states
    );
    println!(
        "      elapsed:                  {:?}",
        metrics.yaku_evaluation
    );
    println!("    yakuman evaluation:");
    println!(
        "      evaluations:              {}",
        metrics.yakuman_evaluations
    );
    println!(
        "      successful states:        {}",
        metrics.yakuman_successful_states
    );
    println!(
        "      elapsed:                  {:?}",
        metrics.yakuman_evaluation
    );
    println!("    weight/count result:");
    println!(
        "      ron-capable weight:        {}",
        metrics.ron_capable_weight
    );
    println!(
        "      ron-capable states:        {}",
        metrics.ron_capable_states
    );
    println!(
        "    total R evaluation elapsed: {:?}",
        metrics.total_r_evaluation
    );
}

#[test]
#[ignore = "release build 前提の計測用。wall-clock threshold は持たない"]
fn measure_one_target_compressed_ron_capable_hidden_hand_weight() {
    let context = representative_context();
    let target = held_target("5p");

    let player_start = Instant::now();
    let mut states = CompressedHiddenHandStates::new(1, &context).expect("menzen reached player");
    let player_construction = player_start.elapsed();
    let tenpai = states.tenpai_state_weight();
    let target_start = Instant::now();
    let evidence = states.ron_risk_evidence(target);
    let evidence_evaluation = target_start.elapsed();
    assert_eq!(evidence.tenpai_weight, tenpai.weight);

    println!(
        "compressed player precomputation + T(p), then 1 target R/T evidence ({}), unron tiles={}:",
        target.to_mjai_string(),
        states.unron_capable_tiles().len()
    );
    println!(
        "  {}: R={} / T={}",
        target.to_mjai_string(),
        evidence.ron_capable_weight,
        evidence.tenpai_weight
    );
    compressed_report(
        tenpai,
        states.metrics(),
        player_construction,
        evidence_evaluation,
    );
}

#[test]
#[ignore = "release build 前提の計測用。wall-clock threshold は持たない"]
fn measure_multiple_targets_compressed_ron_capable_hidden_hand_weight() {
    let context = representative_context();
    let targets: Vec<TileType> = ["6m", "5p", "E"]
        .iter()
        .map(|mjai| held_target(mjai))
        .collect();

    let player_start = Instant::now();
    let mut states = CompressedHiddenHandStates::new(1, &context).expect("menzen reached player");
    let player_construction = player_start.elapsed();
    let tenpai = states.tenpai_state_weight();
    let target_start = Instant::now();
    for target in &targets {
        let evidence = states.ron_risk_evidence(*target);
        assert_eq!(evidence.tenpai_weight, tenpai.weight);
        println!(
            "  {}: R={} / T={}, done at {:?}",
            target.to_mjai_string(),
            evidence.ron_capable_weight,
            evidence.tenpai_weight,
            target_start.elapsed()
        );
    }
    let evidence_evaluation = target_start.elapsed();

    println!(
        "compressed player precomputation + T(p), then {} target R/T evidence values:",
        targets.len()
    );
    compressed_report(
        tenpai,
        states.metrics(),
        player_construction,
        evidence_evaluation,
    );
}

#[test]
#[ignore = "release build 前提の production-like fallback 計測用。wall-clock threshold は持たない"]
fn measure_single_reach_exact_defense_fallback_selection() {
    let context = representative_context();
    let actions: Vec<LegalAction> = context
        .hand_tiles()
        .iter()
        .copied()
        .map(|tile| LegalAction::Dahai { tile })
        .collect();

    let construction_start = Instant::now();
    let mut states = CompressedHiddenHandStates::new(1, &context).expect("single reached player");
    let construction = construction_start.elapsed();
    let tenpai = states.tenpai_state_weight();

    let evaluation_start = Instant::now();
    let mut evaluated = [false; TileType::COUNT];
    let mut unique_targets = 0;
    for action in &actions {
        let LegalAction::Dahai { tile } = action else {
            continue;
        };
        let target = tile.tile_type();
        if !evaluated[target.index()] {
            evaluated[target.index()] = true;
            unique_targets += 1;
            let evidence = states.ron_risk_evidence(target);
            assert_eq!(evidence.tenpai_weight, tenpai.weight);
        }
    }
    let all_unique_evaluations = evaluation_start.elapsed();

    let selection_start = Instant::now();
    let selected = select_defense_fallback_action_with_kind(&context, &actions)
        .expect("representative single-reach fallback");
    let total_selection = selection_start.elapsed();
    assert_eq!(selected.1, DefenseFallbackKind::ExactRonRisk);

    println!("production-like single-reach exact defense fallback:");
    println!("  legal Dahai actions:          {}", actions.len());
    println!("  unique TileType evaluations:  {unique_targets}");
    println!("  CompressedHiddenHandStates:   {construction:?}");
    println!("  all unique R/T evaluations:   {all_unique_evaluations:?}");
    println!("  total fallback selection:     {total_selection:?}");
    println!(
        "  selected:                     {:?} ({:?})",
        selected.0, selected.1
    );
    println!("  T={} states={}", tenpai.weight, tenpai.states);
}

fn open_hand_measurement_context(open_meld_count: usize) -> GameContext {
    assert!((1..=3).contains(&open_meld_count));

    let mut source = TileSource::new();
    let hand = source.tiles(&REPRESENTATIVE_HAND);
    let dora_indicator = source.tile("C");
    let discards = [
        source.tiles(&["1m", "5m", "9m", "1p", "6p", "9p", "1s", "N"]),
        source.tiles(&[
            "1m", "5m", "8m", "9m", "1p", "2p", "6p", "9p", "1s", "4s", "5s", "6s",
        ]),
        source.tiles(&["2m", "4m", "7m", "3p", "7p", "2s", "S", "W"]),
        source.tiles(&["3m", "6m", "8m", "4p", "8p", "3s", "F", "C"]),
    ];

    let mut target_melds = Vec::with_capacity(open_meld_count);
    let white = source.tiles(&["P", "P", "P"]);
    target_melds.push(Meld::new(MeldKind::Pon, white.clone(), Some(white[0])));
    if open_meld_count >= 2 {
        let chi = source.tiles(&["4s", "5s", "6s"]);
        target_melds.push(Meld::new(MeldKind::Chi, chi.clone(), Some(chi[0])));
    }
    if open_meld_count >= 3 {
        let chi = source.tiles(&["7s", "8s", "9s"]);
        target_melds.push(Meld::new(MeldKind::Chi, chi.clone(), Some(chi[0])));
    }

    let mut visible = hand.clone();
    visible.push(dora_indicator);
    for river in &discards {
        visible.extend(river.iter().copied());
    }
    for meld in &target_melds {
        visible.extend(meld.tiles().iter().copied());
    }

    let mut melds: [Vec<Meld>; 4] = Default::default();
    melds[1] = target_melds;
    GameContext::from_parts_with_melds(
        None,
        hand,
        vec![dora_indicator],
        Some(tile_type("E")),
        Some(tile_type("S")),
        visible,
        Some(0),
        Some(1),
        discards,
        [false; 4],
        melds,
    )
    .with_temporary_passed_tiles(Some(Default::default()))
    .with_same_hand_passed_tiles(Some(Default::default()))
    .with_table_state_facts(TableStateFacts {
        remaining_tiles: Some(16),
        ..TableStateFacts::default()
    })
}

#[test]
#[ignore = "release build 前提の OpenHand production-like fallback 計測用。wall-clock threshold は持たない"]
fn measure_open_hand_exact_defense_fallback_selection() {
    for open_meld_count in 1..=3 {
        let context = open_hand_measurement_context(open_meld_count);
        let actions: Vec<LegalAction> = context
            .hand_tiles()
            .iter()
            .copied()
            .map(|tile| LegalAction::Dahai { tile })
            .collect();
        let targets = high_open_hand_threat_players_from_context(&context);
        assert_eq!(targets, vec![1]);

        let mut tile_types = Vec::new();
        let mut seen = [false; TileType::COUNT];
        for action in &actions {
            let LegalAction::Dahai { tile } = action else {
                continue;
            };
            let tile_type = tile.tile_type();
            if !seen[tile_type.index()] {
                seen[tile_type.index()] = true;
                tile_types.push(tile_type);
            }
        }

        let mut vectors = vec![Vec::with_capacity(targets.len()); tile_types.len()];
        let mut construction_total = Duration::ZERO;
        let mut evaluation_total = Duration::ZERO;
        for &player in &targets {
            let construction_start = Instant::now();
            let mut states = CompressedStructuralTenpaiHiddenHandStates::new(player, &context)
                .expect("High OpenHand target");
            let construction = construction_start.elapsed();
            construction_total += construction;
            let tenpai = states.tenpai_state_weight();

            let evaluation_start = Instant::now();
            for (vector, &tile) in vectors.iter_mut().zip(&tile_types) {
                let evidence = states
                    .ron_risk_evidence(tile)
                    .expect("known OpenHand winning context");
                assert_eq!(evidence.tenpai_weight, tenpai.weight);
                vector.push(PlayerRonRiskEvidence { player, evidence });
            }
            let evaluation = evaluation_start.elapsed();
            evaluation_total += evaluation;

            println!(
                "  player {player}: model construction={construction:?}, all candidate R/T={evaluation:?}, T={} states={}",
                tenpai.weight, tenpai.states
            );
            compressed_report(tenpai, states.metrics(), construction, evaluation);
            open_hand_ron_enumerator_report(
                states
                    .ron_metrics()
                    .expect("all candidate R/T initialized the OpenHand R enumerator"),
            );
        }

        let comparison_start = Instant::now();
        let mut best = 0;
        for candidate in 1..vectors.len() {
            if compare_lexicographic_minimax_ron_risk(&vectors[candidate], &vectors[best])
                == Some(std::cmp::Ordering::Less)
            {
                best = candidate;
            }
        }
        let minimax_comparison = comparison_start.elapsed();

        let selector_start = Instant::now();
        let selected =
            select_open_hand_defense_fallback_action_with_kind(&context, &actions, &targets)
                .expect("production OpenHand fallback");
        let production_selector = selector_start.elapsed();
        assert_eq!(selected.1, OpenHandDefenseCategory::ExactRonRisk);
        let LegalAction::Dahai {
            tile: selected_tile,
        } = selected.0
        else {
            panic!("OpenHand fallback must select Dahai");
        };
        assert_eq!(selected_tile.tile_type(), tile_types[best]);

        println!("production-like OpenHand exact defense fallback ({open_meld_count} open melds):");
        println!("  open meld count:              {open_meld_count}");
        println!("  legal Dahai actions:          {}", actions.len());
        println!("  unique TileType evaluations:  {}", tile_types.len());
        println!("  exact target count:           {}", targets.len());
        println!("  model construction total:     {construction_total:?}");
        println!("  all candidate R/T total:      {evaluation_total:?}");
        println!("  minimax comparison:           {minimax_comparison:?}");
        println!("  production selector total:    {production_selector:?}");
        println!(
            "  selected tile:                {}",
            selected_tile.to_mjai_string()
        );
        println!("  selected category:            {:?}", selected.1);
    }
}

// representative context で enumerating implementation と compressed counting が一致すること。
// enumerator が target あたり10秒規模なので、通常の test では走らせない。
#[test]
#[ignore = "enumerating oracle との突き合わせ用。target あたり10秒規模で CI では走らせない"]
fn verify_compressed_matches_enumerator_on_representative_targets() {
    let context = representative_context();
    let targets: Vec<TileType> = ["6m", "5p", "E"]
        .iter()
        .map(|mjai| held_target(mjai))
        .collect();

    let mut enumerated = ReachedHiddenHandStates::new(1, &context).expect("menzen reached player");
    let mut compressed =
        CompressedHiddenHandStates::new(1, &context).expect("menzen reached player");

    for target in targets {
        let start = Instant::now();
        let expected = enumerated.ron_capable_state_weight(target);
        let enumerated_elapsed = start.elapsed();

        let start = Instant::now();
        let actual = compressed.ron_capable_state_weight(target);
        let compressed_elapsed = start.elapsed();

        println!(
            "  {}: weight={} states={} enumerator={:?} compressed={:?}",
            target.to_mjai_string(),
            expected.weight,
            expected.states,
            enumerated_elapsed,
            compressed_elapsed
        );
        assert_eq!(actual, expected, "target: {}", target.to_mjai_string());
    }
}
