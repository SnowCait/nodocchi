use crate::completed_hand::{
    CompletedHandAnalysis, CompletedHandDecomposition, StandardDecomposition,
};
use crate::meld::{Meld, MeldShape, is_menzen};
use crate::shanten::FixedMeldCount;
use crate::tile::{Suit, TileType};

const SANKANTSU_KAN_COUNT: usize = 3;
const SHOUSANGEN_DRAGON_SET_COUNT: usize = 2;
const SUIT_COUNT: usize = 3;
const NUMBER_COUNT: usize = 9;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Yaku {
    Tanyao,
    Chiitoitsu,
    Toitoi,
    Iipeikou,
    Ryanpeikou,
    SanshokuDoujun,
    Ittsu,
    Chanta,
    Junchan,
    Honroutou,
    SanshokuDoukou,
    Sankantsu,
    Shousangen,
    Honitsu,
    Chinitsu,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StructuralYakuEvaluation<'a> {
    decomposition: &'a CompletedHandDecomposition,
    yaku: Vec<Yaku>,
}

impl<'a> StructuralYakuEvaluation<'a> {
    pub fn decomposition(&self) -> &'a CompletedHandDecomposition {
        self.decomposition
    }

    pub fn yaku(&self) -> &[Yaku] {
        &self.yaku
    }

    pub fn contains(&self, yaku: Yaku) -> bool {
        self.yaku.contains(&yaku)
    }

    pub fn is_empty(&self) -> bool {
        self.yaku.is_empty()
    }
}

pub fn evaluate_structural_yaku(
    analysis: &CompletedHandAnalysis,
) -> Vec<StructuralYakuEvaluation<'_>> {
    let tiles = hand_tile_types(analysis);
    let menzen = is_menzen(analysis.fixed_melds());
    analysis
        .decompositions()
        .iter()
        .map(|decomposition| StructuralYakuEvaluation {
            decomposition,
            yaku: decomposition_yaku(decomposition, analysis.fixed_melds(), &tiles, menzen),
        })
        .collect()
}

fn decomposition_yaku(
    decomposition: &CompletedHandDecomposition,
    fixed_melds: &[Meld],
    tiles: &[TileType],
    menzen: bool,
) -> Vec<Yaku> {
    let mut yaku = match decomposition {
        CompletedHandDecomposition::Standard(standard) => {
            standard_yaku(standard, fixed_melds, tiles, menzen)
        }
        CompletedHandDecomposition::Chiitoitsu(_) => chiitoitsu_yaku(tiles),
        CompletedHandDecomposition::Kokushi(_) => Vec::new(),
    };
    yaku.sort_unstable();
    yaku.dedup();
    yaku
}

fn chiitoitsu_yaku(tiles: &[TileType]) -> Vec<Yaku> {
    let mut yaku = vec![Yaku::Chiitoitsu];
    yaku.extend(tile_composition_yaku(tiles));
    yaku
}

fn standard_yaku(
    standard: &StandardDecomposition,
    fixed_melds: &[Meld],
    tiles: &[TileType],
    menzen: bool,
) -> Vec<Yaku> {
    let Some(melds) = standard_meld_shapes(standard, fixed_melds) else {
        return Vec::new();
    };

    let pair = standard.pair();
    let sequences = suit_number_grid(melds.iter().filter_map(|meld| meld.sequence_start()));
    let triplets = suit_number_grid(melds.iter().filter_map(|meld| meld.triplet_tile_type()));

    let mut yaku = tile_composition_yaku(tiles);

    if melds.iter().all(|meld| meld.is_triplet_like()) {
        yaku.push(Yaku::Toitoi);
    }
    if menzen {
        match identical_concealed_sequence_pairs(standard) {
            0 => {}
            1 => yaku.push(Yaku::Iipeikou),
            _ => yaku.push(Yaku::Ryanpeikou),
        }
    }
    if has_same_number_in_every_suit(&sequences) {
        yaku.push(Yaku::SanshokuDoujun);
    }
    if has_straight(&sequences) {
        yaku.push(Yaku::Ittsu);
    }
    if let Some(outside) = outside_hand_yaku(pair, &melds) {
        yaku.push(outside);
    }
    if has_same_number_in_every_suit(&triplets) {
        yaku.push(Yaku::SanshokuDoukou);
    }
    if melds.iter().filter(|meld| meld.is_kan()).count() == SANKANTSU_KAN_COUNT {
        yaku.push(Yaku::Sankantsu);
    }
    if is_shousangen(pair, &melds) {
        yaku.push(Yaku::Shousangen);
    }

    yaku
}

fn tile_composition_yaku(tiles: &[TileType]) -> Vec<Yaku> {
    let mut yaku = Vec::new();
    if tiles.is_empty() {
        return yaku;
    }

    if tiles.iter().all(|tile| !tile.is_yaochu()) {
        yaku.push(Yaku::Tanyao);
    }
    if tiles.iter().all(|tile| tile.is_yaochu()) {
        yaku.push(Yaku::Honroutou);
    }
    if single_suit(tiles).is_some() {
        if tiles.iter().any(|tile| tile.is_honor()) {
            yaku.push(Yaku::Honitsu);
        } else {
            yaku.push(Yaku::Chinitsu);
        }
    }

    yaku
}

fn standard_meld_shapes(
    standard: &StandardDecomposition,
    fixed_melds: &[Meld],
) -> Option<Vec<MeldShape>> {
    let mut shapes: Vec<MeldShape> = standard
        .concealed_melds()
        .iter()
        .map(|meld| meld.shape())
        .collect();
    for meld in fixed_melds {
        shapes.push(meld.shape()?);
    }
    (shapes.len() == usize::from(FixedMeldCount::MAX)).then_some(shapes)
}

fn identical_concealed_sequence_pairs(standard: &StandardDecomposition) -> usize {
    let mut counts = [0u8; TileType::COUNT];
    for start in standard
        .concealed_melds()
        .iter()
        .filter_map(|meld| meld.shape().sequence_start())
    {
        counts[start.index()] += 1;
    }
    counts.iter().map(|count| usize::from(count / 2)).sum()
}

fn outside_hand_yaku(pair: TileType, melds: &[MeldShape]) -> Option<Yaku> {
    let outside = pair.is_yaochu() && melds.iter().all(|meld| contains_yaochu(*meld));
    let has_sequence = melds.iter().any(|meld| meld.is_sequence());
    if !outside || !has_sequence {
        return None;
    }

    let has_honor = pair.is_honor()
        || melds
            .iter()
            .filter_map(|meld| meld.triplet_tile_type())
            .any(|tile| tile.is_honor());
    Some(if has_honor {
        Yaku::Chanta
    } else {
        Yaku::Junchan
    })
}

fn is_shousangen(pair: TileType, melds: &[MeldShape]) -> bool {
    if !pair.is_dragon() {
        return false;
    }

    let mut dragons: Vec<TileType> = melds
        .iter()
        .filter_map(|meld| meld.triplet_tile_type())
        .filter(|tile| tile.is_dragon())
        .collect();
    dragons.sort_unstable();
    dragons.dedup();
    dragons.len() == SHOUSANGEN_DRAGON_SET_COUNT && !dragons.contains(&pair)
}

fn contains_yaochu(meld: MeldShape) -> bool {
    meld.tile_types()
        .is_some_and(|tiles| tiles.iter().any(|tile| tile.is_yaochu()))
}

fn single_suit(tiles: &[TileType]) -> Option<Suit> {
    let mut found = None;
    for suit in tiles.iter().filter_map(|tile| tile.suit()) {
        match found {
            None => found = Some(suit),
            Some(existing) if existing == suit => {}
            Some(_) => return None,
        }
    }
    found
}

fn hand_tile_types(analysis: &CompletedHandAnalysis) -> Vec<TileType> {
    analysis
        .concealed_tiles()
        .iter()
        .chain(analysis.fixed_melds().iter().flat_map(|meld| meld.tiles()))
        .map(|tile| tile.tile_type())
        .collect()
}

fn suit_number_grid(tiles: impl Iterator<Item = TileType>) -> [[bool; NUMBER_COUNT]; SUIT_COUNT] {
    let mut grid = [[false; NUMBER_COUNT]; SUIT_COUNT];
    for tile in tiles {
        if let (Some(suit), Some(number)) = (tile.suit(), tile.number()) {
            grid[suit_index(suit)][usize::from(number - 1)] = true;
        }
    }
    grid
}

fn has_same_number_in_every_suit(grid: &[[bool; NUMBER_COUNT]; SUIT_COUNT]) -> bool {
    (0..NUMBER_COUNT).any(|number| grid.iter().all(|suit| suit[number]))
}

fn has_straight(grid: &[[bool; NUMBER_COUNT]; SUIT_COUNT]) -> bool {
    grid.iter().any(|suit| suit[0] && suit[3] && suit[6])
}

fn suit_index(suit: Suit) -> usize {
    match suit {
        Suit::Man => 0,
        Suit::Pin => 1,
        Suit::Sou => 2,
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

    fn only_yaku(evaluations: &[StructuralYakuEvaluation<'_>]) -> Vec<Yaku> {
        assert_eq!(evaluations.len(), 1);
        evaluations[0].yaku().to_vec()
    }

    fn standard_yaku_sets(evaluations: &[StructuralYakuEvaluation<'_>]) -> Vec<Vec<Yaku>> {
        evaluations
            .iter()
            .filter(|evaluation| evaluation.decomposition().as_standard().is_some())
            .map(|evaluation| evaluation.yaku().to_vec())
            .collect()
    }

    fn chiitoitsu_yaku_set(evaluations: &[StructuralYakuEvaluation<'_>]) -> Option<Vec<Yaku>> {
        evaluations
            .iter()
            .find(|evaluation| evaluation.decomposition().as_chiitoitsu().is_some())
            .map(|evaluation| evaluation.yaku().to_vec())
    }

    fn kokushi_yaku_set(evaluations: &[StructuralYakuEvaluation<'_>]) -> Option<Vec<Yaku>> {
        evaluations
            .iter()
            .find(|evaluation| evaluation.decomposition().as_kokushi().is_some())
            .map(|evaluation| evaluation.yaku().to_vec())
    }

    #[test]
    fn evaluations_keep_every_decomposition_in_order() {
        let mut source = TileIdSource::new();
        let concealed = source.tiles(&[
            "1m", "1m", "2m", "2m", "3m", "3m", "4m", "4m", "5m", "5m", "6m", "6m", "7m", "7m",
        ]);

        let analysis = analyze_completed_hand(&concealed, &[]).unwrap();
        let evaluations = evaluate_structural_yaku(&analysis);

        assert_eq!(evaluations.len(), analysis.decompositions().len());
        for (evaluation, decomposition) in evaluations.iter().zip(analysis.decompositions()) {
            assert_eq!(evaluation.decomposition(), decomposition);
        }
    }

    #[test]
    fn incomplete_hand_has_no_evaluation() {
        let mut source = TileIdSource::new();
        let concealed = source.tiles(&[
            "1m", "2m", "3m", "4m", "5m", "6m", "7m", "8m", "9m", "1p", "3p", "5s", "7s", "9s",
        ]);

        let analysis = analyze_completed_hand(&concealed, &[]).unwrap();

        assert!(evaluate_structural_yaku(&analysis).is_empty());
    }

    #[test]
    fn menzen_tanyao_is_scored() {
        let mut source = TileIdSource::new();
        let concealed = source.tiles(&[
            "2m", "3m", "4m", "3m", "4m", "5m", "2p", "3p", "4p", "5p", "6p", "7p", "5s", "5s",
        ]);

        let analysis = analyze_completed_hand(&concealed, &[]).unwrap();

        assert_eq!(
            only_yaku(&evaluate_structural_yaku(&analysis)),
            [Yaku::Tanyao]
        );
    }

    #[test]
    fn open_tanyao_is_scored() {
        let mut source = TileIdSource::new();
        let fixed = vec![source.meld(MeldKind::Chi, &["2p", "3p", "4p"])];
        let concealed = source.tiles(&[
            "2m", "3m", "4m", "3m", "4m", "5m", "5p", "6p", "7p", "5s", "5s",
        ]);

        let analysis = analyze_completed_hand(&concealed, &fixed).unwrap();

        assert!(!is_menzen(&fixed));
        assert_eq!(
            only_yaku(&evaluate_structural_yaku(&analysis)),
            [Yaku::Tanyao]
        );
    }

    #[test]
    fn one_terminal_breaks_tanyao() {
        let mut source = TileIdSource::new();
        let concealed = source.tiles(&[
            "1m", "2m", "3m", "3m", "4m", "5m", "2p", "3p", "4p", "5p", "6p", "7p", "5s", "5s",
        ]);

        let analysis = analyze_completed_hand(&concealed, &[]).unwrap();

        assert_eq!(only_yaku(&evaluate_structural_yaku(&analysis)), []);
    }

    #[test]
    fn one_honor_breaks_tanyao() {
        let mut source = TileIdSource::new();
        let concealed = source.tiles(&[
            "2m", "3m", "4m", "3m", "4m", "5m", "2p", "3p", "4p", "5p", "6p", "7p", "E", "E",
        ]);

        let analysis = analyze_completed_hand(&concealed, &[]).unwrap();

        assert_eq!(only_yaku(&evaluate_structural_yaku(&analysis)), []);
    }

    #[test]
    fn chiitoitsu_comes_from_the_chiitoitsu_decomposition() {
        let mut source = TileIdSource::new();
        let concealed = source.tiles(&[
            "1m", "1m", "3m", "3m", "5m", "5m", "7m", "7m", "9m", "9m", "1p", "1p", "E", "E",
        ]);

        let analysis = analyze_completed_hand(&concealed, &[]).unwrap();
        let evaluations = evaluate_structural_yaku(&analysis);

        assert_eq!(
            chiitoitsu_yaku_set(&evaluations),
            Some(vec![Yaku::Chiitoitsu])
        );
        assert!(standard_yaku_sets(&evaluations).is_empty());
    }

    #[test]
    fn toitoi_accepts_concealed_triplets_with_fixed_pon_and_kan() {
        let mut source = TileIdSource::new();
        let fixed = vec![
            source.meld(MeldKind::Pon, &["2p", "2p", "2p"]),
            source.meld(MeldKind::Daiminkan, &["5s", "5s", "5s", "5s"]),
        ];
        let concealed = source.tiles(&["3m", "3m", "3m", "7m", "7m", "7m", "E", "E"]);

        let analysis = analyze_completed_hand(&concealed, &fixed).unwrap();

        assert_eq!(
            only_yaku(&evaluate_structural_yaku(&analysis)),
            [Yaku::Toitoi]
        );
    }

    #[test]
    fn one_sequence_breaks_toitoi() {
        let mut source = TileIdSource::new();
        let fixed = vec![
            source.meld(MeldKind::Pon, &["2p", "2p", "2p"]),
            source.meld(MeldKind::Daiminkan, &["5s", "5s", "5s", "5s"]),
        ];
        let concealed = source.tiles(&["3m", "4m", "5m", "7m", "7m", "7m", "E", "E"]);

        let analysis = analyze_completed_hand(&concealed, &fixed).unwrap();

        assert_eq!(only_yaku(&evaluate_structural_yaku(&analysis)), []);
    }

    #[test]
    fn menzen_identical_sequences_are_iipeikou() {
        let mut source = TileIdSource::new();
        let concealed = source.tiles(&[
            "2m", "3m", "4m", "2m", "3m", "4m", "5p", "6p", "7p", "3s", "4s", "5s", "9s", "9s",
        ]);

        let analysis = analyze_completed_hand(&concealed, &[]).unwrap();

        assert_eq!(
            only_yaku(&evaluate_structural_yaku(&analysis)),
            [Yaku::Iipeikou]
        );
    }

    #[test]
    fn open_meld_breaks_iipeikou() {
        let mut source = TileIdSource::new();
        let fixed = vec![source.meld(MeldKind::Chi, &["5p", "6p", "7p"])];
        let concealed = source.tiles(&[
            "2m", "3m", "4m", "2m", "3m", "4m", "3s", "4s", "5s", "9s", "9s",
        ]);

        let analysis = analyze_completed_hand(&concealed, &fixed).unwrap();

        assert!(!is_menzen(&fixed));
        assert_eq!(only_yaku(&evaluate_structural_yaku(&analysis)), []);
    }

    #[test]
    fn ankan_keeps_iipeikou_menzen() {
        let mut source = TileIdSource::new();
        let fixed = vec![source.meld(MeldKind::Ankan, &["1m", "1m", "1m", "1m"])];
        let concealed = source.tiles(&[
            "2p", "3p", "4p", "2p", "3p", "4p", "6s", "7s", "8s", "9s", "9s",
        ]);

        let analysis = analyze_completed_hand(&concealed, &fixed).unwrap();

        assert!(is_menzen(&fixed));
        assert_eq!(
            only_yaku(&evaluate_structural_yaku(&analysis)),
            [Yaku::Iipeikou]
        );
    }

    #[test]
    fn two_identical_sequence_pairs_are_ryanpeikou_only() {
        let mut source = TileIdSource::new();
        let concealed = source.tiles(&[
            "2m", "3m", "4m", "2m", "3m", "4m", "5p", "6p", "7p", "5p", "6p", "7p", "9s", "9s",
        ]);

        let analysis = analyze_completed_hand(&concealed, &[]).unwrap();
        let evaluations = evaluate_structural_yaku(&analysis);

        assert_eq!(
            standard_yaku_sets(&evaluations),
            vec![vec![Yaku::Ryanpeikou]]
        );
        assert!(
            !evaluations
                .iter()
                .any(|evaluation| evaluation.contains(Yaku::Iipeikou))
        );
        assert_eq!(
            chiitoitsu_yaku_set(&evaluations),
            Some(vec![Yaku::Chiitoitsu])
        );
    }

    #[test]
    fn sanshoku_doujun_needs_every_suit() {
        let mut source = TileIdSource::new();
        let concealed = source.tiles(&[
            "2m", "3m", "4m", "2p", "3p", "4p", "2s", "3s", "4s", "7m", "8m", "9m", "5s", "5s",
        ]);

        let analysis = analyze_completed_hand(&concealed, &[]).unwrap();

        assert_eq!(
            only_yaku(&evaluate_structural_yaku(&analysis)),
            [Yaku::SanshokuDoujun]
        );
    }

    #[test]
    fn sanshoku_doujun_accepts_fixed_chi() {
        let mut source = TileIdSource::new();
        let fixed = vec![source.meld(MeldKind::Chi, &["2s", "3s", "4s"])];
        let concealed = source.tiles(&[
            "2m", "3m", "4m", "2p", "3p", "4p", "7m", "8m", "9m", "5s", "5s",
        ]);

        let analysis = analyze_completed_hand(&concealed, &fixed).unwrap();

        assert_eq!(
            only_yaku(&evaluate_structural_yaku(&analysis)),
            [Yaku::SanshokuDoujun]
        );
    }

    #[test]
    fn missing_suit_breaks_sanshoku_doujun() {
        let mut source = TileIdSource::new();
        let concealed = source.tiles(&[
            "2m", "3m", "4m", "2p", "3p", "4p", "5s", "6s", "7s", "7m", "8m", "9m", "5s", "5s",
        ]);

        let analysis = analyze_completed_hand(&concealed, &[]).unwrap();

        assert_eq!(only_yaku(&evaluate_structural_yaku(&analysis)), []);
    }

    #[test]
    fn ittsu_needs_one_four_seven_in_one_suit() {
        let mut source = TileIdSource::new();
        let concealed = source.tiles(&[
            "1m", "2m", "3m", "4m", "5m", "6m", "7m", "8m", "9m", "2p", "3p", "4p", "5s", "5s",
        ]);

        let analysis = analyze_completed_hand(&concealed, &[]).unwrap();

        assert_eq!(
            only_yaku(&evaluate_structural_yaku(&analysis)),
            [Yaku::Ittsu]
        );
    }

    #[test]
    fn ittsu_accepts_fixed_chi() {
        let mut source = TileIdSource::new();
        let fixed = vec![source.meld(MeldKind::Chi, &["7m", "8m", "9m"])];
        let concealed = source.tiles(&[
            "1m", "2m", "3m", "4m", "5m", "6m", "2p", "3p", "4p", "5s", "5s",
        ]);

        let analysis = analyze_completed_hand(&concealed, &fixed).unwrap();

        assert_eq!(
            only_yaku(&evaluate_structural_yaku(&analysis)),
            [Yaku::Ittsu]
        );
    }

    #[test]
    fn sequences_spread_over_suits_are_not_ittsu() {
        let mut source = TileIdSource::new();
        let concealed = source.tiles(&[
            "1m", "2m", "3m", "4p", "5p", "6p", "7s", "8s", "9s", "2p", "3p", "4p", "5s", "5s",
        ]);

        let analysis = analyze_completed_hand(&concealed, &[]).unwrap();

        assert_eq!(only_yaku(&evaluate_structural_yaku(&analysis)), []);
    }

    #[test]
    fn chanta_needs_a_sequence_and_an_honor() {
        let mut source = TileIdSource::new();
        let concealed = source.tiles(&[
            "1m", "2m", "3m", "7p", "8p", "9p", "1s", "1s", "1s", "E", "E", "E", "9s", "9s",
        ]);

        let analysis = analyze_completed_hand(&concealed, &[]).unwrap();
        let yaku = only_yaku(&evaluate_structural_yaku(&analysis));

        assert_eq!(yaku, [Yaku::Chanta]);
        assert!(!yaku.contains(&Yaku::Junchan));
    }

    #[test]
    fn honroutou_without_a_sequence_is_not_chanta() {
        let mut source = TileIdSource::new();
        let concealed = source.tiles(&[
            "1m", "1m", "1m", "9m", "9m", "9m", "E", "E", "E", "C", "C", "C", "9s", "9s",
        ]);

        let analysis = analyze_completed_hand(&concealed, &[]).unwrap();
        let yaku = only_yaku(&evaluate_structural_yaku(&analysis));

        assert_eq!(yaku, [Yaku::Toitoi, Yaku::Honroutou]);
        assert!(!yaku.contains(&Yaku::Chanta));
    }

    #[test]
    fn junchan_excludes_chanta() {
        let mut source = TileIdSource::new();
        let concealed = source.tiles(&[
            "1m", "2m", "3m", "7p", "8p", "9p", "1s", "1s", "1s", "9m", "9m", "9m", "9s", "9s",
        ]);

        let analysis = analyze_completed_hand(&concealed, &[]).unwrap();
        let yaku = only_yaku(&evaluate_structural_yaku(&analysis));

        assert_eq!(yaku, [Yaku::Junchan]);
        assert!(!yaku.contains(&Yaku::Chanta));
    }

    #[test]
    fn one_honor_turns_junchan_into_chanta() {
        let mut source = TileIdSource::new();
        let concealed = source.tiles(&[
            "1m", "2m", "3m", "7p", "8p", "9p", "1s", "1s", "1s", "9m", "9m", "9m", "E", "E",
        ]);

        let analysis = analyze_completed_hand(&concealed, &[]).unwrap();
        let yaku = only_yaku(&evaluate_structural_yaku(&analysis));

        assert_eq!(yaku, [Yaku::Chanta]);
        assert!(!yaku.contains(&Yaku::Junchan));
    }

    #[test]
    fn chiitoitsu_of_terminals_and_honors_is_honroutou() {
        let mut source = TileIdSource::new();
        let concealed = source.tiles(&[
            "1m", "1m", "9m", "9m", "1p", "1p", "9p", "9p", "1s", "1s", "E", "E", "C", "C",
        ]);

        let analysis = analyze_completed_hand(&concealed, &[]).unwrap();
        let evaluations = evaluate_structural_yaku(&analysis);

        assert_eq!(
            chiitoitsu_yaku_set(&evaluations),
            Some(vec![Yaku::Chiitoitsu, Yaku::Honroutou])
        );
    }

    #[test]
    fn kokushi_gets_no_structural_yaku() {
        let mut source = TileIdSource::new();
        let concealed = source.tiles(&[
            "1m", "9m", "1p", "9p", "1s", "9s", "E", "S", "W", "N", "P", "F", "C", "9s",
        ]);

        let analysis = analyze_completed_hand(&concealed, &[]).unwrap();
        let evaluations = evaluate_structural_yaku(&analysis);

        assert_eq!(kokushi_yaku_set(&evaluations), Some(Vec::new()));
        assert_eq!(only_yaku(&evaluations), []);
    }

    #[test]
    fn sanshoku_doukou_needs_the_same_number_in_every_suit() {
        let mut source = TileIdSource::new();
        let concealed = source.tiles(&[
            "2m", "2m", "2m", "2p", "2p", "2p", "2s", "2s", "2s", "4m", "5m", "6m", "9s", "9s",
        ]);

        let analysis = analyze_completed_hand(&concealed, &[]).unwrap();

        assert_eq!(
            only_yaku(&evaluate_structural_yaku(&analysis)),
            [Yaku::SanshokuDoukou]
        );
    }

    #[test]
    fn sanshoku_doukou_accepts_fixed_pon_and_kan() {
        let mut source = TileIdSource::new();
        let fixed = vec![
            source.meld(MeldKind::Pon, &["2p", "2p", "2p"]),
            source.meld(MeldKind::Ankan, &["2s", "2s", "2s", "2s"]),
        ];
        let concealed = source.tiles(&["2m", "2m", "2m", "4m", "5m", "6m", "9s", "9s"]);

        let analysis = analyze_completed_hand(&concealed, &fixed).unwrap();

        assert_eq!(
            only_yaku(&evaluate_structural_yaku(&analysis)),
            [Yaku::SanshokuDoukou]
        );
    }

    #[test]
    fn different_numbers_break_sanshoku_doukou() {
        let mut source = TileIdSource::new();
        let concealed = source.tiles(&[
            "2m", "2m", "2m", "2p", "2p", "2p", "3s", "3s", "3s", "4m", "5m", "6m", "9s", "9s",
        ]);

        let analysis = analyze_completed_hand(&concealed, &[]).unwrap();

        assert_eq!(only_yaku(&evaluate_structural_yaku(&analysis)), []);
    }

    #[test]
    fn three_kans_are_sankantsu() {
        let mut source = TileIdSource::new();
        let fixed = vec![
            source.meld(MeldKind::Ankan, &["1m", "1m", "1m", "1m"]),
            source.meld(MeldKind::Daiminkan, &["2p", "2p", "2p", "2p"]),
            source.meld(MeldKind::Kakan, &["3s", "3s", "3s", "3s"]),
        ];
        let concealed = source.tiles(&["4m", "5m", "6m", "9s", "9s"]);

        let analysis = analyze_completed_hand(&concealed, &fixed).unwrap();

        assert_eq!(
            only_yaku(&evaluate_structural_yaku(&analysis)),
            [Yaku::Sankantsu]
        );
    }

    #[test]
    fn two_kans_are_not_sankantsu() {
        let mut source = TileIdSource::new();
        let fixed = vec![
            source.meld(MeldKind::Ankan, &["1m", "1m", "1m", "1m"]),
            source.meld(MeldKind::Daiminkan, &["2p", "2p", "2p", "2p"]),
        ];
        let concealed = source.tiles(&["4m", "5m", "6m", "7s", "8s", "9s", "9s", "9s"]);

        let analysis = analyze_completed_hand(&concealed, &fixed).unwrap();
        let yaku = only_yaku(&evaluate_structural_yaku(&analysis));

        assert!(!yaku.contains(&Yaku::Sankantsu));
    }

    #[test]
    fn four_kans_are_left_to_suukantsu() {
        let mut source = TileIdSource::new();
        let fixed = vec![
            source.meld(MeldKind::Ankan, &["1m", "1m", "1m", "1m"]),
            source.meld(MeldKind::Daiminkan, &["2p", "2p", "2p", "2p"]),
            source.meld(MeldKind::Kakan, &["3s", "3s", "3s", "3s"]),
            source.meld(MeldKind::Ankan, &["7m", "7m", "7m", "7m"]),
        ];
        let concealed = source.tiles(&["9s", "9s"]);

        let analysis = analyze_completed_hand(&concealed, &fixed).unwrap();
        let yaku = only_yaku(&evaluate_structural_yaku(&analysis));

        assert_eq!(yaku, [Yaku::Toitoi]);
        assert!(!yaku.contains(&Yaku::Sankantsu));
    }

    #[test]
    fn two_dragon_sets_with_the_third_pair_are_shousangen() {
        let mut source = TileIdSource::new();
        let concealed = source.tiles(&[
            "P", "P", "P", "F", "F", "F", "C", "C", "2m", "3m", "4m", "5p", "6p", "7p",
        ]);

        let analysis = analyze_completed_hand(&concealed, &[]).unwrap();

        assert_eq!(
            only_yaku(&evaluate_structural_yaku(&analysis)),
            [Yaku::Shousangen]
        );
    }

    #[test]
    fn one_dragon_set_is_not_shousangen() {
        let mut source = TileIdSource::new();
        let concealed = source.tiles(&[
            "P", "P", "P", "C", "C", "2m", "3m", "4m", "5p", "6p", "7p", "7s", "8s", "9s",
        ]);

        let analysis = analyze_completed_hand(&concealed, &[]).unwrap();

        assert_eq!(only_yaku(&evaluate_structural_yaku(&analysis)), []);
    }

    #[test]
    fn three_dragon_sets_are_left_to_daisangen() {
        let mut source = TileIdSource::new();
        let concealed = source.tiles(&[
            "P", "P", "P", "F", "F", "F", "C", "C", "C", "2m", "3m", "4m", "5p", "5p",
        ]);

        let analysis = analyze_completed_hand(&concealed, &[]).unwrap();
        let yaku = only_yaku(&evaluate_structural_yaku(&analysis));

        assert!(!yaku.contains(&Yaku::Shousangen));
        assert_eq!(yaku, []);
    }

    #[test]
    fn one_suit_with_honors_is_honitsu() {
        let mut source = TileIdSource::new();
        let concealed = source.tiles(&[
            "1m", "2m", "3m", "4m", "5m", "6m", "7m", "8m", "9m", "E", "E", "E", "5m", "5m",
        ]);

        let analysis = analyze_completed_hand(&concealed, &[]).unwrap();
        let yaku = only_yaku(&evaluate_structural_yaku(&analysis));

        assert_eq!(yaku, [Yaku::Ittsu, Yaku::Honitsu]);
        assert!(!yaku.contains(&Yaku::Chinitsu));
    }

    #[test]
    fn open_honitsu_is_scored() {
        let mut source = TileIdSource::new();
        let fixed = vec![source.meld(MeldKind::Pon, &["E", "E", "E"])];
        let concealed = source.tiles(&[
            "1m", "2m", "3m", "4m", "5m", "6m", "7m", "8m", "9m", "5m", "5m",
        ]);

        let analysis = analyze_completed_hand(&concealed, &fixed).unwrap();

        assert_eq!(
            only_yaku(&evaluate_structural_yaku(&analysis)),
            [Yaku::Ittsu, Yaku::Honitsu]
        );
    }

    #[test]
    fn honors_only_hand_is_not_honitsu() {
        let mut source = TileIdSource::new();
        let concealed = source.tiles(&[
            "E", "E", "E", "S", "S", "S", "W", "W", "W", "N", "N", "N", "P", "P",
        ]);

        let analysis = analyze_completed_hand(&concealed, &[]).unwrap();
        let yaku = only_yaku(&evaluate_structural_yaku(&analysis));

        assert_eq!(yaku, [Yaku::Toitoi, Yaku::Honroutou]);
        assert!(!yaku.contains(&Yaku::Honitsu));
        assert!(!yaku.contains(&Yaku::Chinitsu));
    }

    #[test]
    fn one_suit_without_honors_is_chinitsu() {
        let mut source = TileIdSource::new();
        let concealed = source.tiles(&[
            "1m", "2m", "3m", "4m", "5m", "6m", "7m", "8m", "9m", "2m", "3m", "4m", "5m", "5m",
        ]);

        let analysis = analyze_completed_hand(&concealed, &[]).unwrap();
        let yaku = only_yaku(&evaluate_structural_yaku(&analysis));

        assert_eq!(yaku, [Yaku::Ittsu, Yaku::Chinitsu]);
        assert!(!yaku.contains(&Yaku::Honitsu));
    }

    #[test]
    fn open_chinitsu_is_scored() {
        let mut source = TileIdSource::new();
        let fixed = vec![source.meld(MeldKind::Chi, &["7m", "8m", "9m"])];
        let concealed = source.tiles(&[
            "1m", "2m", "3m", "4m", "5m", "6m", "2m", "3m", "4m", "5m", "5m",
        ]);

        let analysis = analyze_completed_hand(&concealed, &fixed).unwrap();
        let yaku = only_yaku(&evaluate_structural_yaku(&analysis));

        assert!(!is_menzen(&fixed));
        assert_eq!(yaku, [Yaku::Ittsu, Yaku::Chinitsu]);
        assert!(!yaku.contains(&Yaku::Honitsu));
    }

    #[test]
    fn toitoi_belongs_to_the_triplet_decomposition_only() {
        let mut source = TileIdSource::new();
        let concealed = source.tiles(&[
            "1m", "1m", "1m", "2m", "2m", "2m", "3m", "3m", "3m", "4m", "4m", "4m", "5m", "5m",
        ]);

        let analysis = analyze_completed_hand(&concealed, &[]).unwrap();
        let evaluations = evaluate_structural_yaku(&analysis);

        assert_eq!(
            standard_yaku_sets(&evaluations),
            vec![
                vec![Yaku::Iipeikou, Yaku::Chinitsu],
                vec![Yaku::Iipeikou, Yaku::Chinitsu],
                vec![Yaku::Iipeikou, Yaku::Chinitsu],
                vec![Yaku::Toitoi, Yaku::Chinitsu],
            ]
        );
        assert_eq!(
            evaluations
                .iter()
                .filter(|evaluation| evaluation.contains(Yaku::Toitoi))
                .count(),
            1
        );
        assert!(
            evaluations
                .iter()
                .any(|evaluation| !evaluation.contains(Yaku::Toitoi))
        );
    }

    #[test]
    fn standard_and_chiitoitsu_families_keep_their_own_yaku() {
        let mut source = TileIdSource::new();
        let concealed = source.tiles(&[
            "1m", "1m", "2m", "2m", "3m", "3m", "4m", "4m", "5m", "5m", "6m", "6m", "7m", "7m",
        ]);

        let analysis = analyze_completed_hand(&concealed, &[]).unwrap();
        let evaluations = evaluate_structural_yaku(&analysis);

        assert_eq!(
            standard_yaku_sets(&evaluations),
            vec![vec![Yaku::Ryanpeikou, Yaku::Chinitsu]; 3]
        );
        assert_eq!(
            chiitoitsu_yaku_set(&evaluations),
            Some(vec![Yaku::Chiitoitsu, Yaku::Chinitsu])
        );
        assert!(
            !evaluations
                .iter()
                .any(|evaluation| evaluation.contains(Yaku::Chiitoitsu)
                    && evaluation.contains(Yaku::Ryanpeikou))
        );
    }

    #[test]
    fn malformed_fixed_meld_scores_nothing() {
        let mut source = TileIdSource::new();
        let fixed = vec![source.meld(MeldKind::Pon, &["2m", "3m", "4m"])];
        let concealed = source.tiles(&[
            "2m", "3m", "4m", "5m", "6m", "7m", "2p", "3p", "4p", "5s", "5s",
        ]);

        let analysis = analyze_completed_hand(&concealed, &fixed).unwrap();

        assert_eq!(fixed[0].shape(), None);
        assert!(analysis.is_complete());
        assert_eq!(only_yaku(&evaluate_structural_yaku(&analysis)), []);
    }

    #[test]
    fn yaku_lists_are_sorted_deduplicated_and_deterministic() {
        let mut source = TileIdSource::new();
        let concealed = source.tiles(&[
            "1m", "1m", "1m", "2m", "2m", "2m", "3m", "3m", "3m", "4m", "4m", "4m", "5m", "5m",
        ]);

        let analysis = analyze_completed_hand(&concealed, &[]).unwrap();
        let evaluations = evaluate_structural_yaku(&analysis);

        for evaluation in &evaluations {
            let mut expected = evaluation.yaku().to_vec();
            expected.sort_unstable();
            expected.dedup();
            assert_eq!(evaluation.yaku(), expected);
        }
        assert_eq!(evaluations, evaluate_structural_yaku(&analysis));
    }
}
