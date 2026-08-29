use super::common::*;
use super::hidden_hand_states::{
    ankan, brute_force_weight, reached_fixture, reached_fixture_with_tile_types,
};
use crate::context::GameContext;
use crate::defense::*;
use crate::meld::{Meld, MeldKind};
use bot_logic::{TileId, TileType};

fn pon(mjai: &str) -> Meld {
    let tiles: Vec<TileId> = TileId::copies(tile_type(mjai)).take(3).collect();
    let called = tiles[0];
    Meld::new(MeldKind::Pon, tiles, Some(called))
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
