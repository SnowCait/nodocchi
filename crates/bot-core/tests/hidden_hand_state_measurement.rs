use std::time::{Duration, Instant};

use bot_core::context::GameContext;
use bot_core::defense::{HiddenHandStateMetrics, ReachedHiddenHandStates, RonCapableStateWeight};
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
        [false, true, false, false],
    )
    .with_post_reach_passed_tiles([vec![], vec![tile_type("4s")], vec![], vec![]])
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
