use crate::completed_hand::{
    CompletedHandAnalysis, CompletedHandDecomposition, StandardDecomposition,
};
use crate::meld::{Meld, MeldShape};
use crate::tile::{Dragon, Suit, TileType, TileTypeSet};
use crate::tile_counts::TileCounts;
use crate::yaku::{hand_tile_types, standard_meld_shapes, triplet_tile_types};

#[cfg(test)]
mod differential;
#[cfg(test)]
mod reference;

const NUMBER_COUNT: usize = 9;
const CHUUREN_TILE_COUNT: u8 = 14;
const CHUUREN_TERMINAL_COUNT: u8 = 3;
const RYUUIISOU_SOU_NUMBERS: [u8; 5] = [2, 3, 4, 6, 8];
const SUUKANTSU_KAN_COUNT: usize = 4;
const DAISANGEN_DRAGON_SET_COUNT: usize = 3;
const SHOUSUUSHII_WIND_SET_COUNT: usize = 3;
const DAISUUSHII_WIND_SET_COUNT: usize = 4;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Yakuman {
    KokushiMusou,
    ChuurenPoutou,
    Ryuuiisou,
    Suuankou,
    Suukantsu,
    Chinroutou,
    Tsuuiisou,
    Daisangen,
    Shousuushii,
    Daisuushii,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct YakumanEvaluation<'a> {
    decomposition: &'a CompletedHandDecomposition,
    yakuman: Vec<Yakuman>,
}

impl<'a> YakumanEvaluation<'a> {
    pub fn decomposition(&self) -> &'a CompletedHandDecomposition {
        self.decomposition
    }

    pub fn yakuman(&self) -> &[Yakuman] {
        &self.yakuman
    }

    pub fn contains(&self, yakuman: Yakuman) -> bool {
        self.yakuman.contains(&yakuman)
    }

    pub fn is_empty(&self) -> bool {
        self.yakuman.is_empty()
    }
}

pub fn evaluate_yakuman(analysis: &CompletedHandAnalysis) -> Vec<YakumanEvaluation<'_>> {
    let counts = analysis.tile_type_counts();
    analysis
        .decompositions()
        .iter()
        .map(|decomposition| YakumanEvaluation {
            decomposition,
            yakuman: decomposition_yakuman(decomposition, analysis.fixed_melds(), counts),
        })
        .collect()
}

pub(crate) fn decomposition_yakuman(
    decomposition: &CompletedHandDecomposition,
    fixed_melds: &[Meld],
    counts: &TileCounts,
) -> Vec<Yakuman> {
    let mut yakuman = match decomposition {
        CompletedHandDecomposition::Standard(standard) => {
            standard_yakuman(standard, fixed_melds, counts)
        }
        CompletedHandDecomposition::Chiitoitsu(_) => tile_composition_yakuman(counts, fixed_melds),
        CompletedHandDecomposition::Kokushi(_) => vec![Yakuman::KokushiMusou],
    };
    yakuman.sort_unstable();
    yakuman.dedup();
    yakuman
}

fn standard_yakuman(
    standard: &StandardDecomposition,
    fixed_melds: &[Meld],
    counts: &TileCounts,
) -> Vec<Yakuman> {
    let Some(melds) = standard_meld_shapes(standard, fixed_melds) else {
        return Vec::new();
    };

    let mut yakuman = tile_composition_yakuman(counts, fixed_melds);
    if melds.iter().filter(|meld| meld.is_kan()).count() == SUUKANTSU_KAN_COUNT {
        yakuman.push(Yakuman::Suukantsu);
    }
    if dragon_set_tiles(&melds).len() == DAISANGEN_DRAGON_SET_COUNT {
        yakuman.push(Yakuman::Daisangen);
    }
    yakuman.extend(wind_yakuman(standard.pair(), &melds));
    yakuman
}

fn wind_yakuman(pair: TileType, melds: &[MeldShape]) -> Option<Yakuman> {
    let winds = wind_set_tiles(melds);
    if winds.len() == DAISUUSHII_WIND_SET_COUNT {
        return Some(Yakuman::Daisuushii);
    }
    let shousuushii =
        winds.len() == SHOUSUUSHII_WIND_SET_COUNT && pair.is_wind() && !winds.contains(pair);
    shousuushii.then_some(Yakuman::Shousuushii)
}

fn dragon_set_tiles(melds: &[MeldShape]) -> TileTypeSet {
    triplet_tile_types(melds, TileType::is_dragon)
}

fn wind_set_tiles(melds: &[MeldShape]) -> TileTypeSet {
    triplet_tile_types(melds, TileType::is_wind)
}

fn tile_composition_yakuman(counts: &TileCounts, fixed_melds: &[Meld]) -> Vec<Yakuman> {
    let mut yakuman = Vec::new();
    if counts.total() == 0 {
        return yakuman;
    }

    if is_chuuren_poutou(counts, fixed_melds) {
        yakuman.push(Yakuman::ChuurenPoutou);
    }
    if hand_tile_types(counts).all(is_green) {
        yakuman.push(Yakuman::Ryuuiisou);
    }
    if hand_tile_types(counts).all(|tile| tile.is_terminal()) {
        yakuman.push(Yakuman::Chinroutou);
    }
    if hand_tile_types(counts).all(|tile| tile.is_honor()) {
        yakuman.push(Yakuman::Tsuuiisou);
    }

    yakuman
}

fn is_chuuren_poutou(counts: &TileCounts, fixed_melds: &[Meld]) -> bool {
    if !fixed_melds.is_empty() {
        return false;
    }
    if counts.total() != CHUUREN_TILE_COUNT {
        return false;
    }

    let mut suit: Option<Suit> = None;
    let mut numbers = [0u8; NUMBER_COUNT];
    for (tile, count) in counts.iter().filter(|(_, count)| *count > 0) {
        let (Some(tile_suit), Some(number)) = (tile.suit(), tile.number()) else {
            return false;
        };
        if *suit.get_or_insert(tile_suit) != tile_suit {
            return false;
        }
        numbers[usize::from(number - 1)] = count;
    }

    numbers[0] >= CHUUREN_TERMINAL_COUNT
        && numbers[NUMBER_COUNT - 1] >= CHUUREN_TERMINAL_COUNT
        && numbers[1..NUMBER_COUNT - 1].iter().all(|count| *count >= 1)
}

fn is_green(tile: TileType) -> bool {
    if tile.dragon() == Some(Dragon::Green) {
        return true;
    }
    tile.is_sou()
        && tile
            .number()
            .is_some_and(|number| RYUUIISOU_SOU_NUMBERS.contains(&number))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::completed_hand::analyze_completed_hand;
    use crate::meld::{MeldKind, is_menzen};
    use crate::tile::TileId;
    use crate::yaku::{Yaku, evaluate_structural_yaku};

    struct TileIdSource {
        used: [u8; TileType::COUNT],
    }

    impl TileIdSource {
        fn new() -> Self {
            Self {
                used: [0; TileType::COUNT],
            }
        }

        fn tiles(&mut self, strings: &[&str]) -> Vec<TileId> {
            strings.iter().map(|s| self.tile(s)).collect()
        }

        fn meld(&mut self, kind: MeldKind, strings: &[&str]) -> Meld {
            let tiles = self.tiles(strings);
            let called_tile = kind.is_open().then(|| tiles[0]);
            Meld::new(kind, tiles, called_tile)
        }

        fn tile(&mut self, s: &str) -> TileId {
            let tile_type = tile_type(s);
            let copy = &mut self.used[tile_type.index()];
            let id = TileId::new(tile_type.raw() * 4 + *copy).unwrap();
            *copy += 1;
            id
        }
    }

    fn tile_type(s: &str) -> TileType {
        TileType::from_mjai_type_str(s).unwrap()
    }

    fn analyze(concealed: &[&str], fixed: &[(MeldKind, &[&str])]) -> CompletedHandAnalysis {
        let mut source = TileIdSource::new();
        let fixed_melds: Vec<Meld> = fixed
            .iter()
            .map(|(kind, tiles)| source.meld(*kind, tiles))
            .collect();
        let tiles = source.tiles(concealed);
        analyze_completed_hand(&tiles, &fixed_melds).unwrap()
    }

    fn yakuman_sets(analysis: &CompletedHandAnalysis) -> Vec<Vec<Yakuman>> {
        evaluate_yakuman(analysis)
            .into_iter()
            .map(|evaluation| evaluation.yakuman().to_vec())
            .collect()
    }

    fn only_yakuman(analysis: &CompletedHandAnalysis) -> Vec<Yakuman> {
        let sets = yakuman_sets(analysis);
        assert_eq!(sets.len(), 1, "sets: {sets:?}");
        sets.into_iter().next().unwrap()
    }

    const KOKUSHI: [&str; 14] = [
        "1m", "1m", "9m", "1p", "9p", "1s", "9s", "E", "S", "W", "N", "P", "F", "C",
    ];
    const CHUUREN_MIDDLE_EXCESS: [&str; 14] = [
        "1m", "1m", "1m", "2m", "3m", "4m", "5m", "5m", "6m", "7m", "8m", "9m", "9m", "9m",
    ];
    const CHUUREN_TERMINAL_EXCESS: [&str; 14] = [
        "1m", "1m", "1m", "1m", "2m", "3m", "4m", "5m", "6m", "7m", "8m", "9m", "9m", "9m",
    ];
    const ALL_GREEN: [&str; 14] = [
        "2s", "3s", "4s", "2s", "3s", "4s", "6s", "6s", "6s", "8s", "8s", "8s", "F", "F",
    ];
    const ALL_TERMINALS: [&str; 14] = [
        "1m", "1m", "1m", "9m", "9m", "9m", "1p", "1p", "1p", "9p", "9p", "9p", "1s", "1s",
    ];
    const ALL_HONORS: [&str; 14] = [
        "E", "E", "E", "S", "S", "S", "W", "W", "W", "P", "P", "P", "F", "F",
    ];
    const ALL_HONOR_PAIRS: [&str; 14] = [
        "E", "E", "S", "S", "W", "W", "N", "N", "P", "P", "F", "F", "C", "C",
    ];
    const BIG_THREE_DRAGONS: [&str; 14] = [
        "P", "P", "P", "F", "F", "F", "C", "C", "C", "2m", "3m", "4m", "5m", "5m",
    ];
    const LITTLE_FOUR_WINDS: [&str; 14] = [
        "E", "E", "E", "S", "S", "S", "W", "W", "W", "N", "N", "2m", "3m", "4m",
    ];
    const BIG_FOUR_WINDS: [&str; 14] = [
        "E", "E", "E", "S", "S", "S", "W", "W", "W", "N", "N", "N", "5m", "5m",
    ];
    const GREEN_TWO_DECOMPOSITIONS: [&str; 14] = [
        "2s", "2s", "2s", "3s", "3s", "3s", "4s", "4s", "4s", "6s", "6s", "8s", "8s", "8s",
    ];

    #[test]
    fn a_kokushi_decomposition_is_kokushi_musou() {
        let analysis = analyze(&KOKUSHI, &[]);

        assert_eq!(only_yakuman(&analysis), vec![Yakuman::KokushiMusou]);
    }

    #[test]
    fn nine_gates_with_a_middle_excess_tile_is_chuuren_poutou() {
        let analysis = analyze(&CHUUREN_MIDDLE_EXCESS, &[]);

        assert_eq!(only_yakuman(&analysis), vec![Yakuman::ChuurenPoutou]);
    }

    #[test]
    fn nine_gates_with_a_terminal_excess_tile_is_chuuren_poutou() {
        let analysis = analyze(&CHUUREN_TERMINAL_EXCESS, &[]);

        assert!(
            yakuman_sets(&analysis)
                .iter()
                .all(|yakuman| yakuman == &[Yakuman::ChuurenPoutou]),
            "sets: {:?}",
            yakuman_sets(&analysis)
        );
    }

    #[test]
    fn a_missing_middle_tile_is_not_chuuren_poutou() {
        let analysis = analyze(
            &[
                "1m", "1m", "1m", "2m", "2m", "2m", "4m", "5m", "6m", "7m", "8m", "9m", "9m", "9m",
            ],
            &[],
        );

        for yakuman in yakuman_sets(&analysis) {
            assert!(!yakuman.contains(&Yakuman::ChuurenPoutou), "{yakuman:?}");
        }
    }

    #[test]
    fn an_honor_tile_is_not_chuuren_poutou() {
        let analysis = analyze(
            &[
                "1m", "1m", "1m", "2m", "3m", "4m", "5m", "6m", "7m", "9m", "9m", "9m", "E", "E",
            ],
            &[],
        );

        assert_eq!(only_yakuman(&analysis), []);
    }

    #[test]
    fn another_suit_is_not_chuuren_poutou() {
        let analysis = analyze(
            &[
                "1m", "1m", "1m", "2m", "3m", "4m", "5m", "6m", "7m", "9m", "9m", "9m", "8p", "8p",
            ],
            &[],
        );

        assert_eq!(only_yakuman(&analysis), []);
    }

    #[test]
    fn a_concealed_quad_is_not_chuuren_poutou() {
        let analysis = analyze(
            &[
                "2m", "3m", "4m", "5m", "6m", "7m", "9m", "9m", "9m", "8m", "8m",
            ],
            &[(MeldKind::Ankan, &["1m", "1m", "1m", "1m"])],
        );

        assert!(is_menzen(analysis.fixed_melds()));
        assert_eq!(only_yakuman(&analysis), []);
    }

    #[test]
    fn an_open_meld_is_not_chuuren_poutou() {
        let analysis = analyze(
            &[
                "2m", "3m", "4m", "5m", "6m", "7m", "9m", "9m", "9m", "8m", "8m",
            ],
            &[(MeldKind::Pon, &["1m", "1m", "1m"])],
        );

        assert_eq!(only_yakuman(&analysis), []);
    }

    #[test]
    fn only_green_tiles_are_ryuuiisou() {
        let analysis = analyze(&ALL_GREEN, &[]);

        assert_eq!(only_yakuman(&analysis), vec![Yakuman::Ryuuiisou]);
    }

    #[test]
    fn ryuuiisou_does_not_require_the_green_dragon() {
        let analysis = analyze(
            &[
                "2s", "3s", "4s", "2s", "3s", "4s", "6s", "6s", "6s", "8s", "8s", "8s", "2s", "2s",
            ],
            &[],
        );

        assert!(
            yakuman_sets(&analysis)
                .iter()
                .all(|yakuman| yakuman.contains(&Yakuman::Ryuuiisou)),
            "sets: {:?}",
            yakuman_sets(&analysis)
        );
    }

    #[test]
    fn a_non_green_bamboo_tile_is_not_ryuuiisou() {
        let analysis = analyze(
            &[
                "2s", "3s", "4s", "2s", "3s", "4s", "6s", "6s", "6s", "6s", "7s", "8s", "F", "F",
            ],
            &[],
        );

        for yakuman in yakuman_sets(&analysis) {
            assert!(!yakuman.contains(&Yakuman::Ryuuiisou), "{yakuman:?}");
        }
    }

    #[test]
    fn another_dragon_is_not_ryuuiisou() {
        let analysis = analyze(
            &[
                "2s", "3s", "4s", "2s", "3s", "4s", "6s", "6s", "6s", "8s", "8s", "8s", "P", "P",
            ],
            &[],
        );

        assert_eq!(only_yakuman(&analysis), []);
    }

    #[test]
    fn only_terminals_are_chinroutou() {
        let analysis = analyze(&ALL_TERMINALS, &[]);

        assert_eq!(only_yakuman(&analysis), vec![Yakuman::Chinroutou]);
    }

    #[test]
    fn an_honor_tile_is_not_chinroutou() {
        let analysis = analyze(
            &[
                "1m", "1m", "1m", "9m", "9m", "9m", "1p", "1p", "1p", "9p", "9p", "9p", "E", "E",
            ],
            &[],
        );

        for yakuman in yakuman_sets(&analysis) {
            assert!(!yakuman.contains(&Yakuman::Chinroutou), "{yakuman:?}");
        }
    }

    #[test]
    fn only_honors_are_tsuuiisou() {
        let analysis = analyze(&ALL_HONORS, &[]);

        assert_eq!(only_yakuman(&analysis), vec![Yakuman::Tsuuiisou]);
    }

    #[test]
    fn seven_honor_pairs_are_tsuuiisou() {
        let analysis = analyze(&ALL_HONOR_PAIRS, &[]);

        assert_eq!(
            analysis.decompositions(),
            [CompletedHandDecomposition::Chiitoitsu(
                *analysis.chiitoitsu_decomposition().unwrap()
            )]
        );
        assert_eq!(only_yakuman(&analysis), vec![Yakuman::Tsuuiisou]);
    }

    #[test]
    fn three_dragon_sets_are_daisangen() {
        let analysis = analyze(&BIG_THREE_DRAGONS, &[]);

        assert_eq!(only_yakuman(&analysis), vec![Yakuman::Daisangen]);
    }

    #[test]
    fn two_dragon_sets_are_not_daisangen() {
        let analysis = analyze(
            &[
                "P", "P", "P", "F", "F", "F", "2m", "3m", "4m", "5m", "6m", "7m", "5m", "5m",
            ],
            &[],
        );

        for yakuman in yakuman_sets(&analysis) {
            assert!(!yakuman.contains(&Yakuman::Daisangen), "{yakuman:?}");
        }
    }

    #[test]
    fn an_open_dragon_set_still_counts_for_daisangen() {
        let analysis = analyze(
            &["F", "F", "F", "C", "C", "C", "2m", "3m", "4m", "5m", "5m"],
            &[(MeldKind::Pon, &["P", "P", "P"])],
        );

        assert_eq!(only_yakuman(&analysis), vec![Yakuman::Daisangen]);
    }

    #[test]
    fn three_wind_sets_with_a_wind_pair_are_shousuushii() {
        let analysis = analyze(&LITTLE_FOUR_WINDS, &[]);

        assert_eq!(only_yakuman(&analysis), vec![Yakuman::Shousuushii]);
    }

    #[test]
    fn three_wind_sets_without_a_wind_pair_are_not_shousuushii() {
        let analysis = analyze(
            &[
                "E", "E", "E", "S", "S", "S", "W", "W", "W", "2m", "3m", "4m", "5m", "5m",
            ],
            &[],
        );

        assert_eq!(only_yakuman(&analysis), []);
    }

    #[test]
    fn four_wind_sets_are_daisuushii_only() {
        let analysis = analyze(&BIG_FOUR_WINDS, &[]);

        assert_eq!(only_yakuman(&analysis), vec![Yakuman::Daisuushii]);
    }

    #[test]
    fn four_quads_are_suukantsu() {
        let analysis = analyze(
            &["5m", "5m"],
            &[
                (MeldKind::Ankan, &["1m", "1m", "1m", "1m"]),
                (MeldKind::Ankan, &["2m", "2m", "2m", "2m"]),
                (MeldKind::Ankan, &["3m", "3m", "3m", "3m"]),
                (MeldKind::Ankan, &["4m", "4m", "4m", "4m"]),
            ],
        );

        assert_eq!(only_yakuman(&analysis), vec![Yakuman::Suukantsu]);
        assert!(!evaluate_structural_yaku(&analysis)[0].contains(Yaku::Sankantsu));
    }

    #[test]
    fn three_quads_are_not_suukantsu() {
        let analysis = analyze(
            &["5m", "6m", "7m", "9m", "9m"],
            &[
                (MeldKind::Ankan, &["1m", "1m", "1m", "1m"]),
                (MeldKind::Ankan, &["2m", "2m", "2m", "2m"]),
                (MeldKind::Ankan, &["3m", "3m", "3m", "3m"]),
            ],
        );

        assert_eq!(only_yakuman(&analysis), []);
        assert!(evaluate_structural_yaku(&analysis)[0].contains(Yaku::Sankantsu));
    }

    #[test]
    fn a_malformed_fixed_meld_gets_no_yakuman() {
        let mut source = TileIdSource::new();
        let fixed = vec![source.meld(MeldKind::Pon, &["E", "S", "W"])];
        let concealed = source.tiles(&["P", "P", "P", "F", "F", "F", "C", "C", "C", "N", "N"]);
        let analysis = analyze_completed_hand(&concealed, &fixed).unwrap();

        assert_eq!(fixed[0].shape(), None);
        assert!(analysis.is_complete());
        assert_eq!(only_yakuman(&analysis), []);
    }

    #[test]
    fn every_decomposition_keeps_its_own_yakuman() {
        let analysis = analyze(&GREEN_TWO_DECOMPOSITIONS, &[]);
        let evaluations = evaluate_yakuman(&analysis);

        assert_eq!(evaluations.len(), analysis.decompositions().len());
        assert_eq!(evaluations.len(), 2);
        for (evaluation, decomposition) in evaluations.iter().zip(analysis.decompositions()) {
            assert_eq!(evaluation.decomposition(), decomposition);
            assert_eq!(evaluation.yakuman(), [Yakuman::Ryuuiisou]);
        }
    }

    #[test]
    fn yakuman_lists_are_sorted_and_deduplicated() {
        let analysis = analyze(
            &[
                "E", "E", "E", "S", "S", "S", "W", "W", "W", "N", "N", "N", "P", "P",
            ],
            &[],
        );

        assert_eq!(
            only_yakuman(&analysis),
            vec![Yakuman::Tsuuiisou, Yakuman::Daisuushii]
        );
    }

    #[test]
    fn an_incomplete_hand_has_no_evaluation() {
        let analysis = analyze(
            &[
                "1m", "1m", "1m", "2m", "3m", "4m", "5m", "6m", "7m", "8m", "9m", "9m", "9m", "1p",
            ],
            &[],
        );

        assert!(!analysis.is_complete());
        assert!(evaluate_yakuman(&analysis).is_empty());
    }
}
