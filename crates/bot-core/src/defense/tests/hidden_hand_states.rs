use std::collections::HashSet;

use super::common::*;
use crate::context::GameContext;
use crate::defense::*;
use crate::meld::{Meld, MeldKind, fixed_meld_count};
use bot_logic::{
    TileCounts, TileId, TileType, calculate_shanten_with_fixed_melds,
    structural_acceptance_tile_types_with_fixed_melds,
};

fn ankan(mjai: &str) -> Meld {
    Meld::new(
        MeldKind::Ankan,
        TileId::copies(tile_type(mjai)).collect(),
        None,
    )
}

fn pon(mjai: &str) -> Meld {
    let tiles: Vec<TileId> = TileId::copies(tile_type(mjai)).take(3).collect();
    let called = tiles[0];
    Meld::new(MeldKind::Pon, tiles, Some(called))
}

// player 1 をリーチ者とし、pool に挙げた牌種だけへ残枚数を残した局面を作る。
// pool に無い牌種は4枚見え扱いになるので、暗槓に使う牌種も pool から外す。
fn reached_fixture(
    pool: &[(&str, u8)],
    melds: Vec<Meld>,
    discards: &[&str],
    post_reach_passed: &[&str],
) -> GameContext {
    let mut remaining = [0u8; TileType::COUNT];
    for (mjai, copies) in pool {
        remaining[tile_type(mjai).index()] = *copies;
    }
    let visible: Vec<TileId> = TileType::all()
        .flat_map(|tile| TileId::copies(tile).take(usize::from(4 - remaining[tile.index()])))
        .collect();

    let mut all_melds: [Vec<Meld>; 4] = Default::default();
    all_melds[1] = melds;
    let mut all_discards: [Vec<TileId>; 4] = Default::default();
    all_discards[1] = discards.iter().map(|mjai| discarded(mjai)).collect();
    let mut all_passed: [Vec<TileType>; 4] = Default::default();
    all_passed[1] = post_reach_passed
        .iter()
        .map(|mjai| tile_type(mjai))
        .collect();

    GameContext::from_parts_with_melds(
        None,
        vec![],
        vec![],
        None,
        None,
        visible,
        Some(0),
        None,
        all_discards,
        [false, true, false, false],
        all_melds,
    )
    .with_post_reach_passed_tiles(all_passed)
}

fn weight_of(target: &str, context: &GameContext) -> RonCapableStateWeight {
    ron_capable_hidden_hand_weight(tile_type(target), 1, context).expect("supported reached state")
}

// test 専用の素朴な参照実装。未知の物理牌から concealed hand 枚数分の subset を全列挙する。
fn choose_physical(
    pool: &[TileType],
    start: usize,
    left: usize,
    counts: &mut TileCounts,
    visit: &mut dyn FnMut(&TileCounts),
) {
    if left == 0 {
        visit(counts);
        return;
    }
    if pool.len() < start + left {
        return;
    }
    for index in start..=pool.len() - left {
        counts.add(pool[index]);
        choose_physical(pool, index + 1, left - 1, counts, visit);
        counts.remove(pool[index]).expect("just added");
    }
}

fn brute_force_weight(target: &str, context: &GameContext) -> RonCapableStateWeight {
    let target = tile_type(target);
    let melds = context.melds_of(1).expect("player 1 exists");
    let fixed = fixed_meld_count(melds).expect("at most four melds");
    let hand_len = usize::from(13 - 3 * fixed.get());

    let pool: Vec<TileType> = TileType::all()
        .flat_map(|tile| {
            std::iter::repeat_n(tile, usize::from(remaining_tile_copies(tile, context)))
        })
        .collect();
    let forbidden: Vec<TileType> = TileType::all()
        .filter(|&tile| is_genbutsu_for(tile, 1, context))
        .collect();

    let mut total = RonCapableStateWeight::default();
    let mut states: HashSet<[u8; TileType::COUNT]> = HashSet::new();
    let mut counts = TileCounts::new();
    choose_physical(
        &pool,
        0,
        hand_len,
        &mut counts,
        &mut |counts: &TileCounts| {
            if calculate_shanten_with_fixed_melds(counts, fixed).min() != 0 {
                return;
            }
            let waits = structural_acceptance_tile_types_with_fixed_melds(counts, fixed);
            if !waits.contains(&target) || waits.iter().any(|wait| forbidden.contains(wait)) {
                return;
            }
            total.weight += 1;
            if states.insert(*counts.as_array()) {
                total.states += 1;
            }
        },
    );
    total
}

fn assert_matches_brute_force(target: &str, context: &GameContext) -> RonCapableStateWeight {
    let exact = weight_of(target, context);
    assert_eq!(exact, brute_force_weight(target, context));
    exact
}

#[test]
fn reduced_pool_matches_physical_brute_force_with_three_ankan() {
    let context = reached_fixture(
        &[
            ("1m", 2),
            ("2m", 2),
            ("3m", 2),
            ("4m", 2),
            ("5m", 2),
            ("6m", 2),
        ],
        vec![ankan("1p"), ankan("2p"), ankan("3p")],
        &[],
        &[],
    );

    let exact = assert_matches_brute_force("3m", &context);
    assert!(exact.weight > 0);
    assert!(exact.states > 0);
}

#[test]
fn reduced_pool_matches_physical_brute_force_with_two_ankan() {
    let context = reached_fixture(
        &[("1m", 3), ("2m", 3), ("3m", 3), ("4m", 3), ("5m", 3)],
        vec![ankan("1p"), ankan("2p")],
        &[],
        &[],
    );

    let exact = assert_matches_brute_force("3m", &context);
    assert!(exact.weight > 0);
}

#[test]
fn reduced_pool_matches_physical_brute_force_without_melds() {
    let context = reached_fixture(
        &[
            ("1m", 2),
            ("2m", 2),
            ("3m", 2),
            ("4m", 2),
            ("5m", 2),
            ("6m", 2),
            ("7m", 2),
            ("8m", 1),
        ],
        vec![],
        &[],
        &[],
    );

    let exact = assert_matches_brute_force("3m", &context);
    assert!(exact.weight > 0);
}

#[test]
fn reduced_pool_matches_physical_brute_force_for_kokushi() {
    let context = reached_fixture(
        &[
            ("1m", 1),
            ("9m", 1),
            ("1p", 2),
            ("9p", 2),
            ("1s", 1),
            ("9s", 1),
            ("E", 1),
            ("S", 1),
            ("W", 1),
            ("N", 1),
            ("P", 1),
            ("F", 1),
            ("C", 1),
        ],
        vec![],
        &[],
        &[],
    );

    let exact = assert_matches_brute_force("1m", &context);
    assert_eq!(
        exact,
        RonCapableStateWeight {
            weight: 8,
            states: 3,
        }
    );
}

#[test]
fn reduced_pool_matches_physical_brute_force_across_suits_and_honors() {
    let context = reached_fixture(
        &[
            ("1m", 1),
            ("2m", 1),
            ("3m", 1),
            ("4m", 1),
            ("5m", 1),
            ("6m", 1),
            ("7m", 1),
            ("8m", 1),
            ("9m", 1),
            ("E", 2),
            ("S", 2),
        ],
        vec![ankan("1p"), ankan("2p"), ankan("3p")],
        &[],
        &[],
    );

    let exact = assert_matches_brute_force("3m", &context);
    assert!(exact.weight > 0);
}

#[test]
fn reduced_pool_matches_physical_brute_force_with_an_honor_triplet() {
    let context = reached_fixture(
        &[
            ("1m", 2),
            ("2m", 2),
            ("3m", 2),
            ("4m", 2),
            ("5m", 2),
            ("E", 3),
        ],
        vec![ankan("1p"), ankan("2p")],
        &[],
        &[],
    );

    let exact = assert_matches_brute_force("3m", &context);
    assert!(exact.weight > 0);
}

#[test]
fn reduced_pool_matches_physical_brute_force_with_a_second_suit() {
    let context = reached_fixture(
        &[
            ("1m", 2),
            ("2m", 2),
            ("3m", 2),
            ("4m", 2),
            ("5m", 2),
            ("6m", 2),
            ("7m", 2),
            ("1s", 2),
        ],
        vec![],
        &[],
        &[],
    );

    let exact = assert_matches_brute_force("3m", &context);
    assert!(exact.weight > 0);
}

#[test]
fn duplicate_decomposition_adds_one_hand_state() {
    // 未知牌がちょうど13枚なので隠れ手牌は 111222333m 44p 5p6p の1状態しかない。
    // その手牌は 111m/222m/333m とも 123m/123m/123m とも分解できる。
    let context = reached_fixture(
        &[
            ("1m", 3),
            ("2m", 3),
            ("3m", 3),
            ("4p", 2),
            ("5p", 1),
            ("6p", 1),
        ],
        vec![],
        &[],
        &[],
    );

    assert_eq!(
        weight_of("7p", &context),
        RonCapableStateWeight {
            weight: 1,
            states: 1,
        }
    );
    assert_eq!(
        weight_of("7p", &context),
        brute_force_weight("7p", &context)
    );
}

#[test]
fn visible_required_tiles_remove_the_candidate() {
    let ryanmen = reached_fixture(
        &[("4m", 1), ("5m", 1), ("9s", 2)],
        vec![ankan("1p"), ankan("2p"), ankan("3p")],
        &[],
        &[],
    );
    assert_eq!(
        weight_of("3m", &ryanmen),
        RonCapableStateWeight {
            weight: 1,
            states: 1,
        }
    );

    let five_man_exhausted = reached_fixture(
        &[("4m", 1), ("9s", 2)],
        vec![ankan("1p"), ankan("2p"), ankan("3p")],
        &[],
        &[],
    );
    assert_eq!(
        weight_of("3m", &five_man_exhausted),
        RonCapableStateWeight::default()
    );
}

#[test]
fn other_wait_in_own_river_removes_the_candidate() {
    let furiten = reached_fixture(
        &[("4m", 1), ("5m", 1), ("9s", 2)],
        vec![ankan("1p"), ankan("2p"), ankan("3p")],
        &["6m"],
        &[],
    );

    assert!(!is_discarded_by_player(tile_type("3m"), 1, &furiten));
    assert_eq!(weight_of("3m", &furiten), RonCapableStateWeight::default());
}

#[test]
fn other_wait_passed_after_reach_removes_the_candidate() {
    let passed = reached_fixture(
        &[("4m", 1), ("5m", 1), ("9s", 2)],
        vec![ankan("1p"), ankan("2p"), ankan("3p")],
        &[],
        &["6m"],
    );

    assert!(!is_genbutsu_for(tile_type("3m"), 1, &passed));
    assert_eq!(weight_of("3m", &passed), RonCapableStateWeight::default());
}

#[test]
fn sequence_and_shanpon_and_tanki_wait_families_are_collected() {
    let three_ankan = || vec![ankan("1p"), ankan("2p"), ankan("3p")];
    let single_state = RonCapableStateWeight {
        weight: 1,
        states: 1,
    };

    for (family, pool) in [
        ("ryanmen", vec![("4m", 1), ("5m", 1), ("9s", 2)]),
        ("kanchan", vec![("2m", 1), ("4m", 1), ("9s", 2)]),
        ("penchan", vec![("1m", 1), ("2m", 1), ("9s", 2)]),
        ("shanpon", vec![("3m", 2), ("9s", 2)]),
        ("tanki", vec![("3m", 1), ("1s", 3)]),
    ] {
        let context = reached_fixture(&pool, three_ankan(), &[], &[]);
        assert_eq!(weight_of("3m", &context), single_state, "family: {family}");
        assert_eq!(
            brute_force_weight("3m", &context),
            single_state,
            "family: {family}"
        );
    }
}

#[test]
fn chiitoitsu_wait_family_is_collected() {
    let context = reached_fixture(
        &[
            ("E", 2),
            ("S", 2),
            ("W", 2),
            ("N", 2),
            ("P", 2),
            ("F", 2),
            ("C", 1),
        ],
        vec![],
        &[],
        &[],
    );

    assert_eq!(
        weight_of("C", &context),
        RonCapableStateWeight {
            weight: 1,
            states: 1,
        }
    );
    assert_eq!(weight_of("C", &context), brute_force_weight("C", &context));
}

#[test]
fn ankan_is_reflected_in_the_concealed_hand_length() {
    for melds in 0..=4usize {
        let all: Vec<Meld> = ["1p", "2p", "3p", "4p"]
            .iter()
            .take(melds)
            .map(|mjai| ankan(mjai))
            .collect();
        let context = reached_fixture(&[("3m", 4), ("1s", 4)], all, &[], &[]);
        let states = ReachedHiddenHandStates::new(1, &context).expect("menzen reached player");

        assert_eq!(states.fixed_meld_count().get(), melds as u8);
        assert_eq!(states.concealed_hand_len(), 13 - 3 * melds as u8);
    }
}

#[test]
fn four_ankan_leaves_only_the_tanki_state() {
    let context = reached_fixture(
        &[("3m", 4)],
        vec![ankan("1p"), ankan("2p"), ankan("3p"), ankan("4p")],
        &[],
        &[],
    );
    let states = ReachedHiddenHandStates::new(1, &context).expect("menzen reached player");
    assert_eq!(states.concealed_hand_len(), 1);

    assert_eq!(
        weight_of("3m", &context),
        RonCapableStateWeight {
            weight: 4,
            states: 1,
        }
    );
}

#[test]
fn contradictory_reached_states_are_unsupported() {
    let open = reached_fixture(&[("3m", 4)], vec![pon("1p")], &[], &[]);
    assert_eq!(
        ron_capable_hidden_hand_weight(tile_type("3m"), 1, &open),
        Err(HiddenHandStateUnsupported::OpenMeld)
    );

    let mixed = reached_fixture(&[("3m", 4)], vec![ankan("2p"), pon("1p")], &[], &[]);
    assert_eq!(
        ron_capable_hidden_hand_weight(tile_type("3m"), 1, &mixed),
        Err(HiddenHandStateUnsupported::OpenMeld)
    );

    let not_reached = reached_fixture(&[("3m", 4)], vec![], &[], &[]);
    assert_eq!(
        ron_capable_hidden_hand_weight(tile_type("3m"), 0, &not_reached),
        Err(HiddenHandStateUnsupported::NotReached)
    );

    assert_eq!(
        ron_capable_hidden_hand_weight(tile_type("3m"), 4, &not_reached),
        Err(HiddenHandStateUnsupported::UnknownPlayer)
    );

    let too_many = reached_fixture(
        &[("3m", 4)],
        vec![
            ankan("1p"),
            ankan("2p"),
            ankan("3p"),
            ankan("4p"),
            ankan("5p"),
        ],
        &[],
        &[],
    );
    assert_eq!(
        ron_capable_hidden_hand_weight(tile_type("3m"), 1, &too_many),
        Err(HiddenHandStateUnsupported::TooManyMelds)
    );
}

#[test]
fn target_in_own_river_has_no_ron_capable_state() {
    let context = reached_fixture(
        &[("4m", 1), ("5m", 1), ("9s", 2)],
        vec![ankan("1p"), ankan("2p"), ankan("3p")],
        &["3m"],
        &[],
    );

    assert_eq!(weight_of("3m", &context), RonCapableStateWeight::default());
    assert_eq!(
        brute_force_weight("3m", &context),
        RonCapableStateWeight::default()
    );
}

#[test]
fn repeated_targets_share_the_wait_cache_without_changing_results() {
    let context = reached_fixture(
        &[
            ("1m", 2),
            ("2m", 2),
            ("3m", 2),
            ("4m", 2),
            ("5m", 2),
            ("6m", 2),
        ],
        vec![ankan("1p"), ankan("2p"), ankan("3p")],
        &[],
        &[],
    );
    let mut states = ReachedHiddenHandStates::new(1, &context).expect("menzen reached player");

    for target in ["3m", "4m", "3m", "4m"] {
        assert_eq!(
            states.ron_capable_state_weight(tile_type(target)),
            weight_of(target, &context),
            "target: {target}"
        );
    }
}
