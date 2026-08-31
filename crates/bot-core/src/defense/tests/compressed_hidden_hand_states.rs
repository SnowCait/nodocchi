use super::common::*;
use super::hidden_hand_states::{
    ankan, brute_force_open_ron_capable_weight, brute_force_tenpai_state_weight,
    brute_force_weight, open_hand_fixture, open_hand_fixture_with_discards,
    open_hand_fixture_with_ron_facts, reached_fixture, reached_fixture_with_tile_types,
};
use crate::context::GameContext;
use crate::defense::*;
use crate::meld::{Meld, MeldKind};
use bot_logic::{
    RiichiStatus, TileId, TileType, WinMethod, WinningContext, Yakuman, analyze_completed_hand,
    evaluate_winning_yakuman,
};
use std::cmp::Ordering;

fn pon(mjai: &str) -> Meld {
    let tiles: Vec<TileId> = TileId::copies(tile_type(mjai)).take(3).collect();
    let called = tiles[0];
    Meld::new(MeldKind::Pon, tiles, Some(called))
}

fn chi(start: &str) -> Meld {
    let tiles: Vec<TileId> = tile_type(start)
        .sequence()
        .expect("suited sequence start")
        .into_iter()
        .map(|tile| TileId::copies(tile).next().expect("first copy"))
        .collect();
    let called = tiles[0];
    Meld::new(MeldKind::Chi, tiles, Some(called))
}

// enumerating implementation を correctness oracle として、compressed counting の結果と比べる。
fn assert_matches_enumerator(target: &str, context: &GameContext) -> RonCapableStateWeight {
    let target = tile_type(target);
    let enumerated = ron_capable_hidden_hand_weight(target, 1, context)
        .expect("supported reached state for the enumerator");
    let compressed = compressed_ron_capable_hidden_hand_weight(target, 1, context)
        .expect("supported reached state for the compressed counter");
    assert_eq!(
        compressed,
        enumerated,
        "target: {}",
        target.to_mjai_string()
    );
    compressed
}

fn assert_tenpai_matches_brute_force(context: &GameContext) -> TenpaiStateWeight {
    let expected = brute_force_tenpai_state_weight(context);
    let states = CompressedHiddenHandStates::new(1, context).expect("supported reached state");
    assert_eq!(states.tenpai_state_weight(), expected);
    expected
}

fn assert_open_target_matches_enumerator(
    target: &str,
    context: &GameContext,
) -> StructuralCompletionStateWeight {
    let target = tile_type(target);
    let mut enumerated =
        StructuralTenpaiHiddenHandStates::new(1, context).expect("open hand enumerator");
    let mut compressed = CompressedStructuralTenpaiHiddenHandStates::new(1, context)
        .expect("compressed open hand model");
    let enumerated = enumerated.target_completion_state_weight(target);
    let compressed = compressed.target_completion_state_weight(target);
    assert_eq!(
        compressed,
        enumerated,
        "target: {}",
        target.to_mjai_string()
    );
    compressed
}

fn assert_open_tenpai_matches_brute_force(context: &GameContext) -> TenpaiStateWeight {
    let expected = brute_force_tenpai_state_weight(context);
    let states = CompressedStructuralTenpaiHiddenHandStates::new(1, context)
        .expect("compressed open hand model");
    assert_eq!(states.tenpai_state_weight(), expected);
    expected
}

fn assert_open_ron_matches_all_counters(
    target: &str,
    context: &GameContext,
) -> RonCapableStateWeight {
    let target_type = tile_type(target);
    let expected = brute_force_open_ron_capable_weight(target, context);
    let mut enumerated =
        StructuralTenpaiHiddenHandStates::new(1, context).expect("open hand enumerator");
    let enumerated = enumerated
        .ron_capable_state_weight(target_type)
        .expect("known winning context");
    let mut compressed = CompressedStructuralTenpaiHiddenHandStates::new(1, context)
        .expect("compressed open hand model");
    let compressed_weight = compressed
        .ron_capable_state_weight(target_type)
        .expect("known winning context");
    let evidence = compressed
        .ron_risk_evidence(target_type)
        .expect("known winning context");

    assert_eq!(enumerated, expected, "enumerator target: {target}");
    assert_eq!(compressed_weight, expected, "compressed target: {target}");
    assert_eq!(evidence.ron_capable_weight, expected.weight);
    assert_eq!(
        evidence.tenpai_weight,
        compressed.tenpai_state_weight().weight
    );
    assert!(evidence.ron_capable_weight <= evidence.tenpai_weight);
    expected
}

fn remaining_counts(context: &GameContext) -> [u8; TileType::COUNT] {
    let mut remaining = [0; TileType::COUNT];
    for tile in TileType::all() {
        remaining[tile.index()] = remaining_tile_copies(tile, context);
    }
    remaining
}

fn assert_ron_is_subset_for_targets(context: &GameContext, targets: &[&str]) {
    let mut states = CompressedHiddenHandStates::new(1, context).unwrap();
    let tenpai = states.tenpai_state_weight();
    for target in targets {
        let ron = states.ron_capable_state_weight(tile_type(target));
        assert!(ron.weight <= tenpai.weight, "target: {target}");
        assert!(ron.states <= tenpai.states, "target: {target}");
    }
}

#[test]
fn open_hand_exact_model_counts_conditional_tenpai_and_target_completion() {
    // 3副露なので concealed hand は4枚。未知 pool の5枚から4枚を選ぶ5 physical states は
    // すべて Standard tenpai。234m5p の2 physical states だけが5p追加で完成する。
    let context = open_hand_fixture(
        &[("2m", 1), ("3m", 1), ("4m", 1), ("5p", 2)],
        vec![pon("P"), pon("F"), pon("C")],
    );

    assert_eq!(
        assert_open_tenpai_matches_brute_force(&context),
        TenpaiStateWeight {
            weight: 5,
            states: 4,
        }
    );
    assert_eq!(
        assert_open_target_matches_enumerator("5p", &context),
        StructuralCompletionStateWeight {
            weight: 2,
            states: 1,
        }
    );
}

#[test]
fn open_hand_exact_model_reflects_visible_tile_copies() {
    let context = open_hand_fixture(
        &[("2m", 1), ("3m", 1), ("4m", 1), ("5p", 1)],
        vec![pon("P"), pon("F"), pon("C")],
    );

    assert_eq!(
        assert_open_tenpai_matches_brute_force(&context),
        TenpaiStateWeight {
            weight: 1,
            states: 1,
        }
    );
    assert_eq!(
        assert_open_target_matches_enumerator("5p", &context),
        StructuralCompletionStateWeight {
            weight: 1,
            states: 1,
        }
    );
}

#[test]
fn open_hand_target_completion_is_structural_not_a_furiten_judgement() {
    let context = open_hand_fixture_with_discards(
        &[("2m", 1), ("3m", 1), ("4m", 1), ("5p", 1)],
        vec![pon("P"), pon("F"), pon("C")],
        &["5p"],
    );

    assert_eq!(
        assert_open_target_matches_enumerator("5p", &context),
        StructuralCompletionStateWeight {
            weight: 1,
            states: 1,
        }
    );
}

fn open_hand_yakuhai_melds() -> Vec<Meld> {
    vec![pon("P"), chi("1p"), chi("4s")]
}

fn open_hand_no_yaku_melds() -> Vec<Meld> {
    vec![chi("1p"), chi("4p"), chi("7s")]
}

fn known_open_hand_ron_fixture(
    melds: Vec<Meld>,
    discards: &[&str],
    temporary_passed: &[&str],
    remaining_tiles: u32,
) -> GameContext {
    open_hand_fixture_with_ron_facts(
        &[("2m", 1), ("3m", 1), ("4m", 1), ("5m", 1)],
        melds,
        discards,
        Some("E"),
        Some(0),
        Some(remaining_tiles),
        Some(temporary_passed),
    )
}

#[test]
fn open_hand_structural_completion_with_yakuhai_is_ron_capable() {
    let context = known_open_hand_ron_fixture(open_hand_yakuhai_melds(), &[], &[], 1);

    assert_eq!(
        assert_open_target_matches_enumerator("5m", &context),
        StructuralCompletionStateWeight {
            weight: 1,
            states: 1,
        }
    );
    assert_eq!(
        assert_open_ron_matches_all_counters("5m", &context),
        RonCapableStateWeight {
            weight: 1,
            states: 1,
        }
    );
}

#[test]
fn open_hand_ron_metrics_follow_the_actual_evaluator_boundaries() {
    let context = known_open_hand_ron_fixture(open_hand_yakuhai_melds(), &[], &[], 1);
    let mut states = CompressedStructuralTenpaiHiddenHandStates::new(1, &context)
        .expect("compressed open hand model");
    let compressed_metrics = states.metrics();
    assert_eq!(states.ron_metrics(), None);

    let weight = states
        .ron_capable_state_weight(tile_type("5m"))
        .expect("known winning context");
    let metrics = states.ron_metrics().expect("initialized R enumerator");
    assert_eq!(states.metrics(), compressed_metrics);
    assert_eq!(states.ron_metrics(), Some(metrics));
    assert_eq!(metrics.ron_capable_weight, weight.weight);
    assert_eq!(metrics.ron_capable_states, weight.states);
    assert_eq!(metrics.cache_misses, metrics.evaluated_states);
    assert_eq!(
        metrics.completion_checks,
        metrics.furiten_completion_checks + metrics.target_completion_checks
    );
    assert_eq!(metrics.completed_states, 1);
    assert_eq!(metrics.yaku_evaluations, 1);
    assert_eq!(metrics.yaku_successful_states, 1);
    assert_eq!(metrics.yakuman_evaluations, 0);
    assert_eq!(metrics.yakuman_successful_states, 0);

    let context = known_open_hand_ron_fixture(open_hand_no_yaku_melds(), &[], &[], 1);
    let mut states = CompressedStructuralTenpaiHiddenHandStates::new(1, &context)
        .expect("compressed open hand model");
    let weight = states
        .ron_capable_state_weight(tile_type("5m"))
        .expect("known winning context");
    let metrics = states.ron_metrics().expect("initialized R enumerator");
    assert_eq!(metrics.ron_capable_weight, weight.weight);
    assert_eq!(metrics.ron_capable_states, weight.states);
    assert_eq!(metrics.completed_states, 1);
    assert_eq!(metrics.yaku_evaluations, 1);
    assert_eq!(metrics.yaku_successful_states, 0);
    assert_eq!(metrics.yakuman_evaluations, 1);
    assert_eq!(metrics.yakuman_successful_states, 0);
}

#[test]
fn open_hand_named_yakuman_is_ron_capable() {
    let context = known_open_hand_ron_fixture(vec![pon("P"), pon("F"), pon("C")], &[], &[], 1);
    let concealed = ["2m", "3m", "4m", "5m", "5m"]
        .into_iter()
        .enumerate()
        .map(|(index, tile)| {
            TileId::copies(tile_type(tile))
                .nth(usize::from(tile == "5m" && index == 4))
                .expect("physical tile")
        })
        .collect::<Vec<_>>();
    let analysis = analyze_completed_hand(
        &concealed,
        context.melds_of(1).expect("player 1 fixed melds"),
    )
    .expect("completed Daisangen hand");
    let winning_context = WinningContext::new(WinMethod::Ron)
        .with_round_wind(Some(tile_type("E")))
        .with_seat_wind(Some(tile_type("S")))
        .with_riichi(RiichiStatus::NotDeclared)
        .with_ippatsu(Some(false))
        .with_rinshan(Some(false))
        .with_chankan(Some(false))
        .with_remaining_live_tiles(Some(1));
    assert!(
        evaluate_winning_yakuman(&analysis, winning_context, tile_type("5m"))
            .iter()
            .any(|evaluation| evaluation.contains(Yakuman::Daisangen))
    );
    assert_eq!(
        assert_open_ron_matches_all_counters("5m", &context),
        RonCapableStateWeight {
            weight: 1,
            states: 1,
        }
    );
}

#[test]
fn open_hand_structural_completion_without_yaku_has_zero_ron_weight() {
    let context = known_open_hand_ron_fixture(open_hand_no_yaku_melds(), &[], &[], 1);

    assert_eq!(
        assert_open_target_matches_enumerator("5m", &context),
        StructuralCompletionStateWeight {
            weight: 1,
            states: 1,
        }
    );
    assert_eq!(
        assert_open_ron_matches_all_counters("5m", &context),
        RonCapableStateWeight::default()
    );
}

#[test]
fn open_hand_houtei_makes_an_otherwise_yakuless_state_ron_capable() {
    let before_houtei = known_open_hand_ron_fixture(open_hand_no_yaku_melds(), &[], &[], 1);
    let houtei = known_open_hand_ron_fixture(open_hand_no_yaku_melds(), &[], &[], 0);

    assert_eq!(
        assert_open_ron_matches_all_counters("5m", &before_houtei),
        RonCapableStateWeight::default()
    );
    assert_eq!(
        assert_open_ron_matches_all_counters("5m", &houtei),
        RonCapableStateWeight {
            weight: 1,
            states: 1,
        }
    );
}

#[test]
fn open_hand_target_in_own_river_has_zero_ron_weight() {
    let context = known_open_hand_ron_fixture(open_hand_yakuhai_melds(), &["5m"], &[], 1);

    assert_eq!(
        assert_open_ron_matches_all_counters("5m", &context),
        RonCapableStateWeight::default()
    );
}

#[test]
fn open_hand_other_wait_in_own_river_makes_the_multi_wait_furiten() {
    let context = open_hand_fixture_with_ron_facts(
        &[("2m", 1), ("3m", 1), ("5p", 2)],
        open_hand_yakuhai_melds(),
        &["1m"],
        Some("E"),
        Some(0),
        Some(1),
        Some(&[]),
    );

    assert_eq!(
        assert_open_target_matches_enumerator("4m", &context),
        StructuralCompletionStateWeight {
            weight: 1,
            states: 1,
        }
    );
    assert_eq!(
        assert_open_ron_matches_all_counters("4m", &context),
        RonCapableStateWeight::default()
    );
}

#[test]
fn open_hand_current_temporary_passed_has_zero_ron_weight() {
    let context = known_open_hand_ron_fixture(open_hand_yakuhai_melds(), &[], &["5m"], 1);

    assert_eq!(
        assert_open_ron_matches_all_counters("5m", &context),
        RonCapableStateWeight::default()
    );
}

#[test]
fn open_hand_missed_kakan_wait_makes_another_wait_temporarily_furiten() {
    let context = open_hand_fixture_with_ron_facts(
        &[("2m", 1), ("3m", 1), ("5p", 2)],
        open_hand_yakuhai_melds(),
        &[],
        Some("E"),
        Some(0),
        Some(1),
        Some(&["1m"]),
    );

    assert_eq!(
        assert_open_target_matches_enumerator("4m", &context),
        StructuralCompletionStateWeight {
            weight: 1,
            states: 1,
        }
    );
    assert_eq!(
        assert_open_ron_matches_all_counters("4m", &context),
        RonCapableStateWeight::default()
    );
}

#[test]
fn open_hand_same_hand_passed_alone_does_not_remove_ron_capable_states() {
    let mut same_hand_passed: [Vec<TileType>; 4] = Default::default();
    same_hand_passed[1].push(tile_type("5m"));
    let context = known_open_hand_ron_fixture(open_hand_yakuhai_melds(), &[], &[], 1)
        .with_same_hand_passed_tiles(Some(same_hand_passed));

    assert_eq!(
        assert_open_ron_matches_all_counters("5m", &context),
        RonCapableStateWeight {
            weight: 1,
            states: 1,
        }
    );
}

#[test]
fn open_hand_ron_counting_is_unavailable_when_required_context_is_unknown() {
    let cases = [
        (
            None,
            Some(0),
            Some(1),
            Some(&[][..]),
            HiddenHandStateUnsupported::UnknownRoundWind,
        ),
        (
            Some("E"),
            None,
            Some(1),
            Some(&[][..]),
            HiddenHandStateUnsupported::UnknownSeatWind,
        ),
        (
            Some("E"),
            Some(0),
            None,
            Some(&[][..]),
            HiddenHandStateUnsupported::UnknownRemainingTiles,
        ),
        (
            Some("E"),
            Some(0),
            Some(1),
            None,
            HiddenHandStateUnsupported::UnknownTemporaryPassedTiles,
        ),
    ];

    for (round_wind, oya, remaining_tiles, temporary_passed, expected) in cases {
        let context = open_hand_fixture_with_ron_facts(
            &[("2m", 1), ("3m", 1), ("4m", 1), ("5m", 1)],
            open_hand_yakuhai_melds(),
            &[],
            round_wind,
            oya,
            remaining_tiles,
            temporary_passed,
        );
        let mut enumerated =
            StructuralTenpaiHiddenHandStates::new(1, &context).expect("structural model");
        let mut compressed = CompressedStructuralTenpaiHiddenHandStates::new(1, &context)
            .expect("compressed structural model");

        assert_eq!(
            enumerated.ron_capable_state_weight(tile_type("5m")),
            Err(expected)
        );
        assert_eq!(compressed.ron_risk_evidence(tile_type("5m")), Err(expected));
    }
}

#[test]
fn open_hand_exact_model_excludes_chiitoitsu_and_kokushi() {
    let chiitoitsu_only = open_hand_fixture(
        &[("E", 2), ("S", 2), ("W", 2), ("N", 2), ("P", 2)],
        vec![pon("C")],
    );
    let kokushi_only = open_hand_fixture(
        &[
            ("1m", 1),
            ("9m", 1),
            ("1p", 1),
            ("9p", 1),
            ("1s", 1),
            ("9s", 1),
            ("E", 1),
            ("S", 1),
            ("W", 1),
            ("N", 1),
        ],
        vec![pon("C")],
    );

    for (family, context, target) in [
        ("chiitoitsu", chiitoitsu_only, "E"),
        ("kokushi", kokushi_only, "P"),
    ] {
        assert_eq!(
            assert_open_tenpai_matches_brute_force(&context),
            TenpaiStateWeight::default(),
            "family: {family}"
        );
        assert_eq!(
            assert_open_target_matches_enumerator(target, &context),
            StructuralCompletionStateWeight::default(),
            "family: {family}"
        );
    }
}

#[test]
fn tenpai_denominator_matches_physical_brute_force_on_a_reduced_pool() {
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

    let total = assert_tenpai_matches_brute_force(&context);
    assert!(total.weight > 0);
    assert!(total.states > 0);
}

#[test]
fn tenpai_denominator_includes_standard_chiitoitsu_and_kokushi() {
    let standard = reached_fixture(
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
            ("1p", 2),
            ("2s", 1),
            ("3s", 1),
        ],
        vec![],
        &[],
        &[],
    );
    let chiitoitsu = reached_fixture(
        &[
            ("1m", 2),
            ("3m", 2),
            ("5m", 2),
            ("7m", 2),
            ("9m", 2),
            ("1p", 2),
            ("3s", 1),
        ],
        vec![],
        &[],
        &[],
    );
    let kokushi = reached_fixture(
        &[
            ("1m", 1),
            ("9m", 1),
            ("1p", 1),
            ("9p", 1),
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

    for (family, context) in [
        ("standard", standard),
        ("chiitoitsu", chiitoitsu),
        ("kokushi", kokushi),
    ] {
        assert_eq!(
            assert_tenpai_matches_brute_force(&context),
            TenpaiStateWeight {
                weight: 1,
                states: 1,
            },
            "family: {family}"
        );
        assert_ron_is_subset_for_targets(&context, &["1m", "3m", "1p", "E"]);
    }
}

#[test]
fn tenpai_denominator_deduplicates_family_overlap_and_standard_decompositions() {
    let overlap = reached_fixture(
        &[
            ("1m", 2),
            ("2m", 2),
            ("3m", 2),
            ("4m", 2),
            ("5m", 2),
            ("6m", 2),
            ("7m", 1),
        ],
        vec![],
        &[],
        &[],
    );
    let duplicate_decomposition = reached_fixture(
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

    for (case, context) in [
        ("standard / chiitoitsu overlap", overlap),
        ("duplicate standard decomposition", duplicate_decomposition),
    ] {
        assert_eq!(
            assert_tenpai_matches_brute_force(&context),
            TenpaiStateWeight {
                weight: 1,
                states: 1,
            },
            "case: {case}"
        );
    }
}

#[test]
fn furiten_and_post_reach_passed_do_not_directly_filter_the_denominator() {
    let pool = [
        ("1m", 2),
        ("2m", 2),
        ("3m", 2),
        ("4m", 2),
        ("5m", 2),
        ("6m", 2),
    ];
    let melds = || vec![ankan("1p"), ankan("2p"), ankan("3p")];
    let open = reached_fixture(&pool, melds(), &[], &[]);
    let own_river = reached_fixture(&pool, melds(), &["6m"], &[]);
    let passed = reached_fixture(&pool, melds(), &[], &["6m"]);

    assert_eq!(remaining_counts(&open), remaining_counts(&own_river));
    assert_eq!(remaining_counts(&open), remaining_counts(&passed));

    let open_tenpai = assert_tenpai_matches_brute_force(&open);
    assert_eq!(assert_tenpai_matches_brute_force(&own_river), open_tenpai);
    assert_eq!(assert_tenpai_matches_brute_force(&passed), open_tenpai);

    let open_ron = compressed_ron_capable_hidden_hand_weight(tile_type("3m"), 1, &open).unwrap();
    let river_ron =
        compressed_ron_capable_hidden_hand_weight(tile_type("3m"), 1, &own_river).unwrap();
    let passed_ron =
        compressed_ron_capable_hidden_hand_weight(tile_type("3m"), 1, &passed).unwrap();
    assert!(river_ron.weight < open_ron.weight);
    assert_eq!(passed_ron, river_ron);
}

#[test]
fn exhausted_structural_wait_remains_in_the_denominator() {
    // 唯一の hidden hand は 45m + 99s。構造上3m/6m待ちだが、どちらも remaining 0。
    let context = reached_fixture(
        &[("4m", 1), ("5m", 1), ("9s", 2)],
        vec![ankan("1p"), ankan("2p"), ankan("3p")],
        &[],
        &[],
    );
    assert_eq!(remaining_tile_copies(tile_type("3m"), &context), 0);
    assert_eq!(remaining_tile_copies(tile_type("6m"), &context), 0);
    assert_eq!(
        assert_tenpai_matches_brute_force(&context),
        TenpaiStateWeight {
            weight: 1,
            states: 1,
        }
    );
}

#[test]
fn tenpai_denominator_matches_reference_for_zero_through_four_ankan() {
    let pools: [&[(&str, u8)]; 5] = [
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
            ("1s", 2),
            ("2s", 1),
            ("3s", 1),
        ],
        &[
            ("1m", 1),
            ("2m", 1),
            ("3m", 1),
            ("4m", 1),
            ("5m", 1),
            ("6m", 1),
            ("1s", 2),
            ("2s", 1),
            ("3s", 1),
        ],
        &[
            ("1m", 1),
            ("2m", 1),
            ("3m", 1),
            ("1s", 2),
            ("2s", 1),
            ("3s", 1),
        ],
        &[("1m", 2), ("2s", 1), ("3s", 1)],
        &[("1m", 1)],
    ];
    let ankan_tiles = ["1p", "2p", "3p", "4p"];

    for (meld_count, pool) in pools.iter().enumerate() {
        let melds = ankan_tiles
            .iter()
            .take(meld_count)
            .map(|tile| ankan(tile))
            .collect();
        let context = reached_fixture(pool, melds, &[], &[]);
        assert_eq!(
            assert_tenpai_matches_brute_force(&context),
            TenpaiStateWeight {
                weight: 1,
                states: 1,
            },
            "ankan count: {meld_count}"
        );
    }
}

#[test]
fn ron_risk_evidence_is_a_subset_and_reuses_one_denominator() {
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
    let mut states = CompressedHiddenHandStates::new(1, &context).unwrap();
    let tenpai = states.tenpai_state_weight();
    let precomputed_metrics = states.metrics();

    for target in ["1m", "3m", "4m", "6m", "E"] {
        let ron = states.ron_capable_state_weight(tile_type(target));
        assert!(ron.weight <= tenpai.weight, "target: {target}");
        assert!(ron.states <= tenpai.states, "target: {target}");
        assert_eq!(states.tenpai_state_weight(), tenpai);
        assert_eq!(
            states.metrics().tenpai_dp_transitions,
            precomputed_metrics.tenpai_dp_transitions
        );
        assert_eq!(
            states.metrics().tenpai_calculation,
            precomputed_metrics.tenpai_calculation
        );
    }

    let evidence = states.ron_risk_evidence(tile_type("3m"));
    assert_eq!(evidence.tenpai_weight, tenpai.weight);
    assert!(evidence.ron_capable_weight <= evidence.tenpai_weight);
}

#[test]
fn ron_risk_ratio_comparison_is_exact_and_denominator_zero_is_unavailable() {
    let half = RonRiskEvidence {
        ron_capable_weight: 1,
        tenpai_weight: 2,
    };
    let same_denominator_lower = RonRiskEvidence {
        ron_capable_weight: 0,
        tenpai_weight: 2,
    };
    let equivalent = RonRiskEvidence {
        ron_capable_weight: 3,
        tenpai_weight: 6,
    };
    let different_denominator_higher = RonRiskEvidence {
        ron_capable_weight: 2,
        tenpai_weight: 3,
    };
    let unavailable = RonRiskEvidence {
        ron_capable_weight: 0,
        tenpai_weight: 0,
    };
    let invalid = RonRiskEvidence {
        ron_capable_weight: 2,
        tenpai_weight: 1,
    };
    let overflow = RonRiskEvidence {
        ron_capable_weight: u128::MAX,
        tenpai_weight: u128::MAX,
    };

    assert_eq!(
        half.compare_ratio(&same_denominator_lower),
        Some(Ordering::Greater)
    );
    assert_eq!(half.compare_ratio(&equivalent), Some(Ordering::Equal));
    assert_eq!(
        half.compare_ratio(&different_denominator_higher),
        Some(Ordering::Less)
    );
    assert_eq!(half.compare_ratio(&unavailable), None);
    assert_eq!(unavailable.compare_ratio(&half), None);
    assert_eq!(half.compare_ratio(&invalid), None);
    assert_eq!(invalid.compare_ratio(&half), None);
    assert_eq!(overflow.compare_ratio(&half), None);
    assert_eq!(half.compare_ratio(&overflow), None);

    // モデル上限 C(136, 13) = 483,774,556,165,488,000 の cross product も u128 に収まる。
    let maximum = RonRiskEvidence {
        ron_capable_weight: 483_774_556_165_488_000,
        tenpai_weight: 483_774_556_165_488_000,
    };
    assert_eq!(maximum.compare_ratio(&maximum), Some(Ordering::Equal));
}

#[test]
fn inconsistent_context_can_expose_a_zero_denominator() {
    let context = reached_fixture(
        &[],
        vec![ankan("1p"), ankan("2p"), ankan("3p"), ankan("4p")],
        &[],
        &[],
    );
    let mut states = CompressedHiddenHandStates::new(1, &context).unwrap();
    assert_eq!(states.tenpai_state_weight(), TenpaiStateWeight::default());
    let evidence = states.ron_risk_evidence(tile_type("1m"));
    assert_eq!(evidence.tenpai_weight, 0);
    assert_eq!(evidence.compare_ratio(&evidence), None);
}

#[test]
fn matches_the_enumerator_on_reduced_pools() {
    struct ReducedPool {
        name: &'static str,
        pool: Vec<(&'static str, u8)>,
        melds: Vec<Meld>,
        target: &'static str,
    }

    let case = |name, pool, melds, target| ReducedPool {
        name,
        pool,
        melds,
        target,
    };

    let cases = vec![
        case(
            "three ankan",
            vec![
                ("1m", 2),
                ("2m", 2),
                ("3m", 2),
                ("4m", 2),
                ("5m", 2),
                ("6m", 2),
            ],
            vec![ankan("1p"), ankan("2p"), ankan("3p")],
            "3m",
        ),
        case(
            "two ankan",
            vec![("1m", 3), ("2m", 3), ("3m", 3), ("4m", 3), ("5m", 3)],
            vec![ankan("1p"), ankan("2p")],
            "3m",
        ),
        case(
            "menzen single suit",
            vec![
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
            "3m",
        ),
        case(
            "across suits and honors",
            vec![
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
            "3m",
        ),
        case(
            "honor triplet",
            vec![
                ("1m", 2),
                ("2m", 2),
                ("3m", 2),
                ("4m", 2),
                ("5m", 2),
                ("E", 3),
            ],
            vec![ankan("1p"), ankan("2p")],
            "3m",
        ),
        case(
            "second suit",
            vec![
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
            "3m",
        ),
        case(
            "kokushi pool",
            vec![
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
            "1m",
        ),
    ];

    for ReducedPool {
        name,
        pool,
        melds,
        target,
    } in cases
    {
        let context = reached_fixture(&pool, melds, &[], &[]);
        let weight = assert_matches_enumerator(target, &context);
        assert!(weight.weight > 0, "case: {name}");
        assert_eq!(weight, brute_force_weight(target, &context), "case: {name}");
    }
}

#[test]
fn matches_the_enumerator_across_wait_families() {
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
        assert_eq!(
            assert_matches_enumerator("3m", &context),
            single_state,
            "family: {family}"
        );
    }

    let chiitoitsu = reached_fixture(
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
        assert_matches_enumerator("C", &chiitoitsu),
        single_state,
        "family: chiitoitsu"
    );

    let kokushi = reached_fixture(
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
    assert_eq!(
        assert_matches_enumerator("1m", &kokushi),
        RonCapableStateWeight {
            weight: 8,
            states: 3,
        },
        "family: kokushi"
    );
}

#[test]
fn matches_the_enumerator_with_furiten() {
    let pool = [
        ("1m", 2),
        ("2m", 2),
        ("3m", 2),
        ("4m", 2),
        ("5m", 2),
        ("6m", 2),
    ];
    let melds = || vec![ankan("1p"), ankan("2p"), ankan("3p")];

    let open = reached_fixture(&pool, melds(), &[], &[]);
    let open_weight = assert_matches_enumerator("3m", &open);

    // 6m でも待つ候補だけがフリテンで消える。3m 自体は河にない。
    let river = reached_fixture(&pool, melds(), &["6m"], &[]);
    let river_weight = assert_matches_enumerator("3m", &river);
    assert!(river_weight.weight > 0);
    assert!(river_weight.weight < open_weight.weight);

    let passed = reached_fixture(&pool, melds(), &[], &["6m"]);
    assert_eq!(assert_matches_enumerator("3m", &passed), river_weight);

    // target 自身が現物なら状態は残らない。
    let genbutsu = reached_fixture(&pool, melds(), &["3m"], &[]);
    assert_eq!(
        assert_matches_enumerator("3m", &genbutsu),
        RonCapableStateWeight::default()
    );

    // 七対子の単騎がフリテンなら、その手牌は標準形でも数えない。
    let chiitoitsu_pool = [
        ("1m", 2),
        ("3m", 2),
        ("5m", 2),
        ("7m", 2),
        ("9m", 2),
        ("1p", 2),
        ("3s", 1),
    ];
    for forbidden in [&[][..], &["3s"][..], &["1m"][..]] {
        let context = reached_fixture(&chiitoitsu_pool, vec![], forbidden, &[]);
        assert_matches_enumerator("3s", &context);
        assert_matches_enumerator("1p", &context);
    }
}

#[test]
fn matches_the_enumerator_for_every_forbidden_tile() {
    let pool = [
        ("1m", 2),
        ("2m", 2),
        ("3m", 2),
        ("4m", 2),
        ("5m", 2),
        ("6m", 2),
        ("E", 2),
    ];

    let mut rejected = 0;
    for index in 0..TileType::COUNT {
        let forbidden = TileType::new(index as u8).expect("valid tile type");
        let (discards, passed) = if index % 2 == 0 {
            (vec![forbidden], Vec::new())
        } else {
            (Vec::new(), vec![forbidden])
        };
        let context = reached_fixture_with_tile_types(
            &pool,
            vec![ankan("1p"), ankan("2p"), ankan("3p")],
            &discards,
            &passed,
        );
        let weight = assert_matches_enumerator("3m", &context);
        rejected += usize::from(weight.weight == 0);
    }
    assert!(rejected > 0);
}

#[test]
fn matches_the_enumerator_with_ankan_counts() {
    for melds in 0..=4usize {
        let all: Vec<Meld> = ["1p", "2p", "3p", "4p"]
            .iter()
            .take(melds)
            .map(|mjai| ankan(mjai))
            .collect();
        let context = reached_fixture(&[("3m", 4), ("4m", 2), ("5m", 2), ("1s", 4)], all, &[], &[]);
        let states = CompressedHiddenHandStates::new(1, &context).expect("menzen reached player");
        assert_eq!(states.fixed_meld_count().get(), melds as u8);
        assert_eq!(states.concealed_hand_len(), 13 - 3 * melds as u8);

        assert_matches_enumerator("3m", &context);
    }
}

#[test]
fn duplicate_decomposition_counts_one_state() {
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
        assert_matches_enumerator("7p", &context),
        RonCapableStateWeight {
            weight: 1,
            states: 1,
        }
    );
}

#[test]
fn unsupported_states_match_the_enumerator() {
    let open = reached_fixture(&[("3m", 4)], vec![pon("1p")], &[], &[]);
    assert_eq!(
        compressed_ron_capable_hidden_hand_weight(tile_type("3m"), 1, &open),
        Err(HiddenHandStateUnsupported::OpenMeld)
    );

    let not_reached = reached_fixture(&[("3m", 4)], vec![], &[], &[]);
    assert_eq!(
        compressed_ron_capable_hidden_hand_weight(tile_type("3m"), 0, &not_reached),
        Err(HiddenHandStateUnsupported::NotReached)
    );
    assert_eq!(
        compressed_ron_capable_hidden_hand_weight(tile_type("3m"), 4, &not_reached),
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
        compressed_ron_capable_hidden_hand_weight(tile_type("3m"), 1, &too_many),
        Err(HiddenHandStateUnsupported::TooManyMelds)
    );
}

#[test]
fn repeated_targets_reuse_the_compressed_state_space() {
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
    let mut states = CompressedHiddenHandStates::new(1, &context).expect("menzen reached player");
    let precomputation = states.metrics();

    for target in ["3m", "4m", "3m", "4m"] {
        assert_eq!(
            states.ron_capable_state_weight(tile_type(target)),
            compressed_ron_capable_hidden_hand_weight(tile_type(target), 1, &context)
                .expect("supported reached state"),
            "target: {target}"
        );
    }

    let metrics = states.metrics();
    assert_eq!(
        metrics.enumerated_group_vectors,
        precomputation.enumerated_group_vectors
    );
    assert_eq!(
        metrics.retained_group_classes,
        precomputation.retained_group_classes
    );
    assert!(metrics.dp_transitions > 0);
}

// LCG で作った決定的な pool を横断して、enumerating implementation と一致することを確かめる。
struct Lcg(u64);

impl Lcg {
    fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0 >> 33
    }

    fn below(&mut self, bound: u64) -> u64 {
        self.next() % bound
    }
}

#[test]
fn matches_the_enumerator_across_generated_pools() {
    let ankan_tiles = ["1p", "2p", "3p"];
    let mut random = Lcg(0x1234_5678_9abc_def0);

    for case in 0..48u64 {
        let meld_count = (case % 3) as usize;
        let melds: Vec<Meld> = ankan_tiles
            .iter()
            .take(meld_count)
            .map(|mjai| ankan(mjai))
            .collect();
        let hand_len = 13 - 3 * meld_count as u8;

        // pool の広さも case ごとに変え、未知牌が少ない局面と多い局面の両方を通す。
        let width = hand_len + 3 + (case % 4) as u8 * 5;
        let mut remaining = [0u8; TileType::COUNT];
        let mut total = 0u8;
        while total < width {
            let index = random.below(TileType::COUNT as u64) as usize;
            let tile = TileType::new(index as u8).expect("valid tile type");
            if ankan_tiles.contains(&tile.to_mjai_string().as_str()) || remaining[index] > 0 {
                continue;
            }
            let copies = (1 + random.below(4)) as u8;
            remaining[index] = copies;
            total += copies;
        }

        let pool: Vec<(String, u8)> = TileType::all()
            .filter(|tile| remaining[tile.index()] > 0)
            .map(|tile| (tile.to_mjai_string(), remaining[tile.index()]))
            .collect();
        let pool: Vec<(&str, u8)> = pool
            .iter()
            .map(|(mjai, copies)| (mjai.as_str(), *copies))
            .collect();

        let held: Vec<&str> = pool.iter().map(|(mjai, _)| *mjai).collect();
        let target = held[random.below(held.len() as u64) as usize];
        let forbidden = TileType::new(random.below(TileType::COUNT as u64) as u8)
            .expect("valid tile type")
            .to_mjai_string();
        let (discards, passed) = match case % 4 {
            0 => (Vec::new(), Vec::new()),
            1 => (vec![forbidden.as_str()], Vec::new()),
            2 => (Vec::new(), vec![forbidden.as_str()]),
            _ => (vec![forbidden.as_str()], vec![target]),
        };

        let context = reached_fixture(&pool, melds, &discards, &passed);
        assert_matches_enumerator(target, &context);
    }
}

#[test]
fn matches_the_enumerator_when_chiitoitsu_and_standard_overlap() {
    // 未知牌がちょうど13枚なので隠れ手牌は 112233445566m 7m の1状態しかない。
    // この手牌は七対子単騎とも 123m/123m/456m/456m + 7m 単騎とも解釈でき、どちらも 7m 待ち。
    let pool = [
        ("1m", 2),
        ("2m", 2),
        ("3m", 2),
        ("4m", 2),
        ("5m", 2),
        ("6m", 2),
        ("7m", 1),
    ];

    let context = reached_fixture(&pool, vec![], &[], &[]);
    assert_eq!(
        assert_matches_enumerator("7m", &context),
        RonCapableStateWeight {
            weight: 1,
            states: 1,
        }
    );

    for forbidden in ["1m", "4m", "7m", "9s"] {
        let context = reached_fixture(&pool, vec![], &[forbidden], &[]);
        assert_matches_enumerator("7m", &context);
    }
}

#[test]
fn matches_the_enumerator_across_generated_pair_pools() {
    // 対子だけが残る pool では七対子と標準形が同時に成立しやすい。
    let mut random = Lcg(0x0f1e_2d3c_4b5a_6978);

    for case in 0..24u64 {
        let mut remaining = [0u8; TileType::COUNT];
        let mut total = 0u8;
        while total < 16 {
            let index = random.below(TileType::COUNT as u64) as usize;
            if remaining[index] > 0 {
                continue;
            }
            remaining[index] = 2;
            total += 2;
        }

        let pool: Vec<(String, u8)> = TileType::all()
            .filter(|tile| remaining[tile.index()] > 0)
            .map(|tile| (tile.to_mjai_string(), remaining[tile.index()]))
            .collect();
        let pool: Vec<(&str, u8)> = pool
            .iter()
            .map(|(mjai, copies)| (mjai.as_str(), *copies))
            .collect();

        let held: Vec<&str> = pool.iter().map(|(mjai, _)| *mjai).collect();
        let target = held[random.below(held.len() as u64) as usize];
        let forbidden = held[random.below(held.len() as u64) as usize];
        let discards: Vec<&str> = if case % 2 == 0 {
            Vec::new()
        } else {
            vec![forbidden]
        };

        let context = reached_fixture(&pool, vec![], &discards, &[]);
        assert_matches_enumerator(target, &context);
    }
}

#[test]
fn matches_the_enumerator_across_generated_yaochu_pools() {
    // 么九牌だけが残る pool では国士無双が成立する。フリテンで13面待ちが消える場合も含める。
    let yaochu = [
        "1m", "9m", "1p", "9p", "1s", "9s", "E", "S", "W", "N", "P", "F", "C",
    ];
    let mut random = Lcg(0x2718_2818_2845_9045);

    for case in 0..24u64 {
        let pool: Vec<(&str, u8)> = yaochu
            .iter()
            .map(|mjai| (*mjai, (1 + random.below(2)) as u8))
            .collect();
        let target = yaochu[random.below(yaochu.len() as u64) as usize];
        let forbidden = yaochu[random.below(yaochu.len() as u64) as usize];
        let discards: Vec<&str> = match case % 3 {
            0 => Vec::new(),
            1 => vec![forbidden],
            _ => vec!["2m"],
        };

        let context = reached_fixture(&pool, vec![], &discards, &[]);
        assert_matches_enumerator(target, &context);
    }
}
