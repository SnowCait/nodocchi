use crate::completed_hand::{
    ChiitoitsuDecomposition, CompletedHandAnalysis, CompletedHandDecomposition, ConcealedMeld,
    KokushiDecomposition, StandardDecomposition,
};
use crate::meld::MeldShape;
use crate::tile::TileType;

const EDGE_WAIT_LOW_SEQUENCE_START: u8 = 1;
const EDGE_WAIT_HIGH_SEQUENCE_START: u8 = 7;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum WaitType {
    Ryanmen,
    Kanchan,
    Penchan,
    Tanki,
    Shanpon,
    KokushiSingle,
    KokushiThirteenSided,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum WinningGroup {
    Pair { tile: TileType },
    Sequence { start: TileType },
    Triplet { tile: TileType },
    KokushiSingle { tile: TileType },
}

impl WinningGroup {
    pub fn is_pair(self) -> bool {
        matches!(self, Self::Pair { .. })
    }

    pub fn is_sequence(self) -> bool {
        matches!(self, Self::Sequence { .. })
    }

    pub fn is_triplet(self) -> bool {
        matches!(self, Self::Triplet { .. })
    }

    pub fn meld_shape(self) -> Option<MeldShape> {
        match self {
            Self::Sequence { start } => Some(MeldShape::Sequence { start }),
            Self::Triplet { tile } => Some(MeldShape::Triplet { tile }),
            Self::Pair { .. } | Self::KokushiSingle { .. } => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct WinningTileInterpretation<'a> {
    decomposition: &'a CompletedHandDecomposition,
    winning_tile: TileType,
    group: WinningGroup,
    wait: WaitType,
}

impl<'a> WinningTileInterpretation<'a> {
    pub fn decomposition(&self) -> &'a CompletedHandDecomposition {
        self.decomposition
    }

    pub fn winning_tile(&self) -> TileType {
        self.winning_tile
    }

    pub fn group(&self) -> WinningGroup {
        self.group
    }

    pub fn wait(&self) -> WaitType {
        self.wait
    }
}

pub fn interpret_winning_tile(
    analysis: &CompletedHandAnalysis,
    winning_tile: TileType,
) -> Vec<WinningTileInterpretation<'_>> {
    if !concealed_contains(analysis, winning_tile) {
        return Vec::new();
    }

    analysis
        .decompositions()
        .iter()
        .flat_map(|decomposition| {
            let mut interpretations: Vec<_> = winning_groups(decomposition, winning_tile)
                .into_iter()
                .map(|(group, wait)| WinningTileInterpretation {
                    decomposition,
                    winning_tile,
                    group,
                    wait,
                })
                .collect();
            interpretations.sort_unstable();
            interpretations.dedup();
            interpretations
        })
        .collect()
}

fn concealed_contains(analysis: &CompletedHandAnalysis, winning_tile: TileType) -> bool {
    analysis
        .concealed_tiles()
        .iter()
        .any(|tile| tile.tile_type() == winning_tile)
}

fn winning_groups(
    decomposition: &CompletedHandDecomposition,
    winning_tile: TileType,
) -> Vec<(WinningGroup, WaitType)> {
    match decomposition {
        CompletedHandDecomposition::Standard(standard) => {
            standard_winning_groups(standard, winning_tile)
        }
        CompletedHandDecomposition::Chiitoitsu(chiitoitsu) => {
            chiitoitsu_winning_group(chiitoitsu, winning_tile)
                .into_iter()
                .collect()
        }
        CompletedHandDecomposition::Kokushi(kokushi) => {
            vec![kokushi_winning_group(kokushi, winning_tile)]
        }
    }
}

fn standard_winning_groups(
    standard: &StandardDecomposition,
    winning_tile: TileType,
) -> Vec<(WinningGroup, WaitType)> {
    let mut groups = Vec::new();

    if standard.pair() == winning_tile {
        groups.push((WinningGroup::Pair { tile: winning_tile }, WaitType::Tanki));
    }

    for meld in standard.concealed_melds() {
        match *meld {
            ConcealedMeld::Sequence { start } => {
                if let Some(wait) = sequence_wait(start, winning_tile) {
                    groups.push((WinningGroup::Sequence { start }, wait));
                }
            }
            ConcealedMeld::Triplet { tile } if tile == winning_tile => {
                groups.push((WinningGroup::Triplet { tile }, WaitType::Shanpon));
            }
            ConcealedMeld::Triplet { .. } => {}
        }
    }

    groups
}

fn sequence_wait(start: TileType, winning_tile: TileType) -> Option<WaitType> {
    let position = start
        .sequence()?
        .iter()
        .position(|tile| *tile == winning_tile)?;

    match (position, start.number()?) {
        (1, _) => Some(WaitType::Kanchan),
        (0, EDGE_WAIT_HIGH_SEQUENCE_START) | (2, EDGE_WAIT_LOW_SEQUENCE_START) => {
            Some(WaitType::Penchan)
        }
        (0 | 2, _) => Some(WaitType::Ryanmen),
        _ => None,
    }
}

fn chiitoitsu_winning_group(
    chiitoitsu: &ChiitoitsuDecomposition,
    winning_tile: TileType,
) -> Option<(WinningGroup, WaitType)> {
    chiitoitsu
        .pairs()
        .contains(&winning_tile)
        .then_some((WinningGroup::Pair { tile: winning_tile }, WaitType::Tanki))
}

fn kokushi_winning_group(
    kokushi: &KokushiDecomposition,
    winning_tile: TileType,
) -> (WinningGroup, WaitType) {
    if kokushi.pair() == winning_tile {
        (
            WinningGroup::Pair { tile: winning_tile },
            WaitType::KokushiThirteenSided,
        )
    } else {
        (
            WinningGroup::KokushiSingle { tile: winning_tile },
            WaitType::KokushiSingle,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::completed_hand::analyze_completed_hand;
    use crate::meld::{Meld, MeldKind};
    use crate::tile::TileId;

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

    fn pair(s: &str) -> WinningGroup {
        WinningGroup::Pair { tile: tile_type(s) }
    }

    fn sequence(s: &str) -> WinningGroup {
        WinningGroup::Sequence {
            start: tile_type(s),
        }
    }

    fn triplet(s: &str) -> WinningGroup {
        WinningGroup::Triplet { tile: tile_type(s) }
    }

    fn kokushi_single(s: &str) -> WinningGroup {
        WinningGroup::KokushiSingle { tile: tile_type(s) }
    }

    fn interpret(
        concealed: &[&str],
        fixed_melds: &[Meld],
        winning_tile: &str,
    ) -> Vec<(WinningGroup, WaitType)> {
        let mut source = TileIdSource::new();
        let tiles = source.tiles(concealed);
        let analysis = analyze_completed_hand(&tiles, fixed_melds).unwrap();
        interpret_winning_tile(&analysis, tile_type(winning_tile))
            .into_iter()
            .map(|interpretation| (interpretation.group(), interpretation.wait()))
            .collect()
    }

    const HAND_123M: [&str; 14] = [
        "1m", "2m", "3m", "4p", "5p", "6p", "7p", "8p", "9p", "1s", "2s", "3s", "5s", "5s",
    ];
    const HAND_345M: [&str; 14] = [
        "3m", "4m", "5m", "4p", "5p", "6p", "7p", "8p", "9p", "1s", "2s", "3s", "5s", "5s",
    ];
    const HAND_789M: [&str; 14] = [
        "7m", "8m", "9m", "4p", "5p", "6p", "7p", "8p", "9p", "1s", "2s", "3s", "5s", "5s",
    ];

    #[test]
    fn sequence_completed_from_two_sided_shape_is_ryanmen() {
        assert_eq!(
            interpret(&HAND_345M, &[], "3m"),
            vec![(sequence("3m"), WaitType::Ryanmen)]
        );
    }

    #[test]
    fn sequence_completed_at_the_middle_is_kanchan() {
        assert_eq!(
            interpret(&HAND_123M, &[], "2m"),
            vec![(sequence("1m"), WaitType::Kanchan)]
        );
    }

    #[test]
    fn three_completing_one_two_is_penchan() {
        assert_eq!(
            interpret(&HAND_123M, &[], "3m"),
            vec![(sequence("1m"), WaitType::Penchan)]
        );
    }

    #[test]
    fn seven_completing_eight_nine_is_penchan() {
        assert_eq!(
            interpret(&HAND_789M, &[], "7m"),
            vec![(sequence("7m"), WaitType::Penchan)]
        );
    }

    #[test]
    fn one_completing_two_three_is_ryanmen() {
        assert_eq!(
            interpret(&HAND_123M, &[], "1m"),
            vec![(sequence("1m"), WaitType::Ryanmen)]
        );
    }

    #[test]
    fn nine_completing_seven_eight_is_ryanmen() {
        assert_eq!(
            interpret(&HAND_789M, &[], "9m"),
            vec![(sequence("7m"), WaitType::Ryanmen)]
        );
    }

    #[test]
    fn pair_completion_is_tanki() {
        assert_eq!(
            interpret(&HAND_123M, &[], "5s"),
            vec![(pair("5s"), WaitType::Tanki)]
        );
    }

    #[test]
    fn concealed_triplet_completion_is_shanpon() {
        let concealed = [
            "1m", "1m", "1m", "4p", "5p", "6p", "7p", "8p", "9p", "1s", "2s", "3s", "5s", "5s",
        ];

        assert_eq!(
            interpret(&concealed, &[], "1m"),
            vec![(triplet("1m"), WaitType::Shanpon)]
        );
    }

    #[test]
    fn one_decomposition_can_hold_penchan_and_ryanmen() {
        let concealed = [
            "1m", "2m", "3m", "3m", "4m", "5m", "4p", "5p", "6p", "5p", "5p", "7s", "8s", "9s",
        ];

        assert_eq!(
            interpret(&concealed, &[], "3m"),
            vec![
                (sequence("1m"), WaitType::Penchan),
                (sequence("3m"), WaitType::Ryanmen),
            ]
        );
    }

    #[test]
    fn one_decomposition_can_hold_tanki_and_sequence_wait() {
        let concealed = [
            "1m", "2m", "3m", "3m", "3m", "4p", "5p", "6p", "7p", "8p", "9p", "1s", "2s", "3s",
        ];

        assert_eq!(
            interpret(&concealed, &[], "3m"),
            vec![
                (pair("3m"), WaitType::Tanki),
                (sequence("1m"), WaitType::Penchan),
            ]
        );
    }

    #[test]
    fn identical_sequences_are_not_reported_twice() {
        let concealed = [
            "1m", "2m", "3m", "1m", "2m", "3m", "4p", "5p", "6p", "7p", "8p", "9p", "5s", "5s",
        ];

        assert_eq!(
            interpret(&concealed, &[], "1m"),
            vec![(sequence("1m"), WaitType::Ryanmen)]
        );
        assert_eq!(
            interpret(&concealed, &[], "3m"),
            vec![(sequence("1m"), WaitType::Penchan)]
        );
    }

    #[test]
    fn fixed_meld_is_not_a_winning_group() {
        let mut source = TileIdSource::new();
        let fixed = vec![source.meld(MeldKind::Pon, &["1m", "1m", "1m"])];
        let concealed = source.tiles(&[
            "1m", "2m", "3m", "4p", "5p", "6p", "7p", "8p", "9p", "5s", "5s",
        ]);
        let analysis = analyze_completed_hand(&concealed, &fixed).unwrap();

        let interpretations: Vec<_> = interpret_winning_tile(&analysis, tile_type("1m"))
            .into_iter()
            .map(|interpretation| (interpretation.group(), interpretation.wait()))
            .collect();

        assert_eq!(interpretations, vec![(sequence("1m"), WaitType::Ryanmen)]);
        assert!(
            !interpretations
                .iter()
                .any(|(group, wait)| group.is_triplet() || *wait == WaitType::Shanpon)
        );
    }

    #[test]
    fn chiitoitsu_pair_completion_is_tanki() {
        let concealed = [
            "1m", "1m", "3m", "3m", "5m", "5m", "7m", "7m", "9m", "9m", "1p", "1p", "E", "E",
        ];

        assert_eq!(
            interpret(&concealed, &[], "E"),
            vec![(pair("E"), WaitType::Tanki)]
        );
    }

    #[test]
    fn kokushi_pair_tile_is_thirteen_sided() {
        let concealed = [
            "1m", "9m", "1p", "9p", "1s", "9s", "E", "S", "W", "N", "P", "F", "C", "9s",
        ];

        assert_eq!(
            interpret(&concealed, &[], "9s"),
            vec![(pair("9s"), WaitType::KokushiThirteenSided)]
        );
    }

    #[test]
    fn kokushi_non_pair_tile_is_a_single_wait() {
        let concealed = [
            "1m", "9m", "1p", "9p", "1s", "9s", "E", "S", "W", "N", "P", "F", "C", "9s",
        ];

        assert_eq!(
            interpret(&concealed, &[], "1m"),
            vec![(kokushi_single("1m"), WaitType::KokushiSingle)]
        );
    }

    #[test]
    fn winning_tile_outside_the_concealed_hand_has_no_interpretation() {
        let mut source = TileIdSource::new();
        let fixed = vec![source.meld(MeldKind::Pon, &["E", "E", "E"])];
        let concealed = source.tiles(&[
            "1m", "2m", "3m", "4p", "5p", "6p", "7p", "8p", "9p", "5s", "5s",
        ]);
        let analysis = analyze_completed_hand(&concealed, &fixed).unwrap();

        assert!(analysis.is_complete());
        assert!(interpret_winning_tile(&analysis, tile_type("E")).is_empty());
    }

    #[test]
    fn incomplete_hand_has_no_interpretation() {
        let mut source = TileIdSource::new();
        let concealed = source.tiles(&[
            "1m", "2m", "3m", "4m", "5m", "6m", "7m", "8m", "9m", "1p", "2p", "3p", "5s",
        ]);
        let analysis = analyze_completed_hand(&concealed, &[]).unwrap();

        assert!(!analysis.is_complete());
        assert!(interpret_winning_tile(&analysis, tile_type("5s")).is_empty());
    }

    #[test]
    fn interpretations_are_deduplicated_and_deterministic() {
        let mut source = TileIdSource::new();
        let concealed = source.tiles(&[
            "1m", "1m", "1m", "2m", "2m", "2m", "3m", "3m", "3m", "4m", "4m", "4m", "5m", "5m",
        ]);
        let analysis = analyze_completed_hand(&concealed, &[]).unwrap();

        let interpretations = interpret_winning_tile(&analysis, tile_type("3m"));
        let shapes: Vec<_> = interpretations
            .iter()
            .map(|interpretation| (interpretation.group(), interpretation.wait()))
            .collect();

        assert_eq!(analysis.decompositions().len(), 4);
        assert_eq!(
            shapes,
            vec![
                (sequence("2m"), WaitType::Kanchan),
                (sequence("3m"), WaitType::Ryanmen),
                (sequence("1m"), WaitType::Penchan),
                (sequence("2m"), WaitType::Kanchan),
                (triplet("3m"), WaitType::Shanpon),
            ]
        );
        assert_eq!(
            interpret_winning_tile(&analysis, tile_type("3m")),
            interpretations
        );
        assert!(
            interpretations
                .iter()
                .all(|interpretation| interpretation.winning_tile() == tile_type("3m"))
        );
    }

    #[test]
    fn pinfu_layer_can_read_sequence_completion_and_ryanmen() {
        let mut source = TileIdSource::new();
        let concealed = source.tiles(&HAND_345M);
        let analysis = analyze_completed_hand(&concealed, &[]).unwrap();

        let interpretations = interpret_winning_tile(&analysis, tile_type("3m"));
        let interpretation = interpretations.first().unwrap();

        assert!(interpretation.group().is_sequence());
        assert!(!interpretation.group().is_pair());
        assert_eq!(interpretation.wait(), WaitType::Ryanmen);
        assert_eq!(
            interpretation.group().meld_shape(),
            Some(MeldShape::Sequence {
                start: tile_type("3m")
            })
        );
        assert_eq!(
            interpretation.decomposition(),
            &analysis.decompositions()[0]
        );
    }
}
