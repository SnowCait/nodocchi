//! `Observation` から復元した山の残りツモ可能枚数が、RiichiEnv の `GameState` が持つ値と
//! 一致することを確認する regression。
//!
//! `Observation` は山の残枚数を持たないため、riichilab-client は見えている牌から復元する。
//! upstream の `WallState::drawable_count` を source of truth として、実際に局を進めながら
//! 突き合わせる。配牌直後・局の中盤・鳴きがある局面・槓がある局面をすべて通る。

use std::collections::HashMap;

use riichienv_core::action::{Action, ActionType};
use riichienv_core::observation::Observation;
use riichienv_core::rule::GameRule;
use riichienv_core::state::GameState;
use riichienv_core::types::MeldType;
use riichilab_client::ObservationPayload;

// 局を進めるときに選ぶ action の優先順。鳴きと槓を通すために先に取り、和了とリーチは局が
// 短くなるので取らない。
const PREFERRED: [ActionType; 5] = [
    ActionType::Ankan,
    ActionType::Kakan,
    ActionType::Pon,
    ActionType::Chi,
    ActionType::Discard,
];

// 突き合わせた観測の内訳。局面の種類を網羅できたことを確認するために数える。
#[derive(Debug, Default)]
struct Coverage {
    checked: usize,
    early_kyoku: usize,
    mid_kyoku: usize,
    with_meld: usize,
    with_kan: usize,
}

fn choose(legal: &[Action]) -> Option<Action> {
    PREFERRED
        .iter()
        .find_map(|kind| legal.iter().find(|action| action.action_type == *kind))
        .or_else(|| {
            legal
                .iter()
                .find(|action| action.action_type == ActionType::Pass)
        })
        .cloned()
}

fn has_meld(observation: &Observation) -> bool {
    observation.melds.iter().any(|melds| !melds.is_empty())
}

fn has_kan(observation: &Observation) -> bool {
    observation.melds.iter().flatten().any(|meld| {
        matches!(
            meld.meld_type,
            MeldType::Ankan | MeldType::Daiminkan | MeldType::Kakan
        )
    })
}

fn verify(observation: &Observation, drawable_count: u8, coverage: &mut Coverage) {
    let decoded = ObservationPayload::new(
        observation
            .serialize_to_base64()
            .expect("Observation を base64 へ直列化できる"),
    )
    .decode_4p()
    .expect("Observation を復号できる");

    assert_eq!(
        decoded.table_state.remaining_tiles,
        Some(u32::from(drawable_count)),
        "player {} の観測で山の残枚数が upstream と一致する",
        observation.player_id,
    );

    coverage.checked += 1;
    if drawable_count >= 60 {
        coverage.early_kyoku += 1;
    }
    if drawable_count <= 40 {
        coverage.mid_kyoku += 1;
    }
    if has_meld(observation) {
        coverage.with_meld += 1;
    }
    if has_kan(observation) {
        coverage.with_kan += 1;
    }
}

fn drive(seed: u64, steps: usize) -> Coverage {
    let mut state = GameState::new(2, false, Some(seed), 0, GameRule::default());
    let mut coverage = Coverage::default();

    for _ in 0..steps {
        if state.is_done {
            break;
        }
        if state.needs_initialize_next_round {
            state.step(&HashMap::new());
            continue;
        }

        let mut actions = HashMap::new();
        for player in state.active_players.clone() {
            let observation = state.get_observation(player);
            let legal = observation.legal_actions_method();
            if legal.is_empty() {
                continue;
            }
            verify(&observation, state.wall.drawable_count, &mut coverage);
            if let Some(action) = choose(&legal) {
                actions.insert(player, action);
            }
        }

        if actions.is_empty() {
            break;
        }
        state.step(&actions);
    }

    coverage
}

#[test]
fn the_restored_remaining_wall_matches_the_upstream_game_state() {
    let mut total = Coverage::default();
    for seed in 0..8 {
        let coverage = drive(seed, 600);
        total.checked += coverage.checked;
        total.early_kyoku += coverage.early_kyoku;
        total.mid_kyoku += coverage.mid_kyoku;
        total.with_meld += coverage.with_meld;
        total.with_kan += coverage.with_kan;
    }

    assert!(total.checked > 100, "{total:?}");
    assert!(total.early_kyoku > 0, "{total:?}");
    assert!(total.mid_kyoku > 0, "{total:?}");
    assert!(total.with_meld > 0, "{total:?}");
    assert!(total.with_kan > 0, "{total:?}");
}

#[test]
fn the_dealer_first_discard_sees_the_wall_after_its_own_draw() {
    // 配牌 13枚 × 4人と王牌 14枚を除いた 70枚から、親のツモ1枚を引いた 69枚。
    let mut state = GameState::new(2, false, Some(0), 0, GameRule::default());
    let observation = state.get_observation(state.oya);
    let decoded = ObservationPayload::new(observation.serialize_to_base64().unwrap())
        .decode_4p()
        .unwrap();

    assert_eq!(state.wall.drawable_count, 69);
    assert_eq!(decoded.table_state.remaining_tiles, Some(69));
}
