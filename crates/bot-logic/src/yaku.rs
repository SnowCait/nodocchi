use crate::completed_hand::{
    CompletedHandAnalysis, CompletedHandDecomposition, StandardDecomposition,
};
use crate::meld::{Meld, MeldShape, is_menzen};
use crate::shanten::FixedMeldCount;
use crate::tile::{Dragon, Suit, TileType};
use crate::winning_context::{RiichiStatus, WinMethod, WinningContext};

const SANKANTSU_KAN_COUNT: usize = 3;
const SHOUSANGEN_DRAGON_SET_COUNT: usize = 2;
const SUIT_COUNT: usize = 3;
const NUMBER_COUNT: usize = 9;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Yaku {
    Pinfu,
    Tanyao,
    Chiitoitsu,
    Toitoi,
    Sanankou,
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
    YakuhaiWhite,
    YakuhaiGreen,
    YakuhaiRed,
    YakuhaiRoundWind,
    YakuhaiSeatWind,
    Riichi,
    DoubleRiichi,
    Ippatsu,
    MenzenTsumo,
    Chankan,
    RinshanKaihou,
    Haitei,
    Houtei,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct YakuEvaluation<'a> {
    decomposition: &'a CompletedHandDecomposition,
    yaku: Vec<Yaku>,
}

pub type StructuralYakuEvaluation<'a> = YakuEvaluation<'a>;

impl<'a> YakuEvaluation<'a> {
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

pub fn evaluate_structural_yaku(analysis: &CompletedHandAnalysis) -> Vec<YakuEvaluation<'_>> {
    let tiles = hand_tile_types(analysis);
    let menzen = is_menzen(analysis.fixed_melds());
    analysis
        .decompositions()
        .iter()
        .map(|decomposition| YakuEvaluation {
            decomposition,
            yaku: decomposition_yaku(decomposition, analysis.fixed_melds(), &tiles, menzen),
        })
        .collect()
}

pub fn evaluate_yaku(
    analysis: &CompletedHandAnalysis,
    context: WinningContext,
) -> Vec<YakuEvaluation<'_>> {
    let menzen = is_menzen(analysis.fixed_melds());
    evaluate_structural_yaku(analysis)
        .into_iter()
        .map(|evaluation| {
            let YakuEvaluation {
                decomposition,
                mut yaku,
            } = evaluation;
            yaku.extend(contextual_yaku(
                decomposition,
                analysis.fixed_melds(),
                context,
                menzen,
            ));
            yaku.sort_unstable();
            yaku.dedup();
            YakuEvaluation {
                decomposition,
                yaku,
            }
        })
        .collect()
}

fn contextual_yaku(
    decomposition: &CompletedHandDecomposition,
    fixed_melds: &[Meld],
    context: WinningContext,
    menzen: bool,
) -> Vec<Yaku> {
    match decomposition {
        CompletedHandDecomposition::Standard(standard) => {
            let Some(melds) = standard_meld_shapes(standard, fixed_melds) else {
                return Vec::new();
            };
            let mut yaku = yakuhai_yaku(&melds, context);
            yaku.extend(win_context_yaku(context, menzen));
            yaku
        }
        CompletedHandDecomposition::Chiitoitsu(_) | CompletedHandDecomposition::Kokushi(_) => {
            win_context_yaku(context, menzen)
        }
    }
}

fn yakuhai_yaku(melds: &[MeldShape], context: WinningContext) -> Vec<Yaku> {
    melds
        .iter()
        .filter_map(|meld| meld.triplet_tile_type())
        .flat_map(|tile| tile_yakuhai(tile, context))
        .collect()
}

fn tile_yakuhai(tile: TileType, context: WinningContext) -> Vec<Yaku> {
    if let Some(dragon) = tile.dragon() {
        return vec![dragon_yakuhai(dragon)];
    }
    if !tile.is_wind() {
        return Vec::new();
    }

    let mut yaku = Vec::new();
    if context.round_wind() == Some(tile) {
        yaku.push(Yaku::YakuhaiRoundWind);
    }
    if context.seat_wind() == Some(tile) {
        yaku.push(Yaku::YakuhaiSeatWind);
    }
    yaku
}

fn dragon_yakuhai(dragon: Dragon) -> Yaku {
    match dragon {
        Dragon::White => Yaku::YakuhaiWhite,
        Dragon::Green => Yaku::YakuhaiGreen,
        Dragon::Red => Yaku::YakuhaiRed,
    }
}

fn win_context_yaku(context: WinningContext, menzen: bool) -> Vec<Yaku> {
    let mut yaku = Vec::new();
    if menzen {
        yaku.extend(menzen_context_yaku(context));
    }
    if context.win_method().is_tsumo() && context.rinshan() == Some(true) {
        yaku.push(Yaku::RinshanKaihou);
    }
    if context.win_method().is_ron() && context.chankan() == Some(true) {
        yaku.push(Yaku::Chankan);
    }
    yaku.extend(last_live_tile_yaku(context));
    yaku
}

fn menzen_context_yaku(context: WinningContext) -> Vec<Yaku> {
    let mut yaku = Vec::new();
    match context.riichi() {
        RiichiStatus::Riichi => yaku.push(Yaku::Riichi),
        RiichiStatus::DoubleRiichi => yaku.push(Yaku::DoubleRiichi),
        RiichiStatus::Unknown | RiichiStatus::NotDeclared => {}
    }
    if context.riichi().is_declared() == Some(true) && context.ippatsu() == Some(true) {
        yaku.push(Yaku::Ippatsu);
    }
    if context.win_method().is_tsumo() {
        yaku.push(Yaku::MenzenTsumo);
    }
    yaku
}

fn last_live_tile_yaku(context: WinningContext) -> Option<Yaku> {
    if !context.is_last_live_tile() {
        return None;
    }
    match context.win_method() {
        WinMethod::Tsumo => (context.rinshan() == Some(false)).then_some(Yaku::Haitei),
        WinMethod::Ron => (context.chankan() == Some(false)).then_some(Yaku::Houtei),
    }
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

pub(crate) fn standard_meld_shapes(
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

    fn ron_context() -> WinningContext {
        WinningContext::new(WinMethod::Ron)
    }

    fn tsumo_context() -> WinningContext {
        WinningContext::new(WinMethod::Tsumo)
    }

    fn wind_context(round_wind: Option<&str>, seat_wind: Option<&str>) -> WinningContext {
        ron_context()
            .with_round_wind(round_wind.map(tile_type))
            .with_seat_wind(seat_wind.map(tile_type))
    }

    fn honor_set_hand(source: &mut TileIdSource, honor: &str) -> Vec<TileId> {
        source.tiles(&[
            honor, honor, honor, "2m", "3m", "4m", "5m", "6m", "7m", "2p", "3p", "4p", "5s", "5s",
        ])
    }

    fn three_meld_rest(source: &mut TileIdSource) -> Vec<TileId> {
        source.tiles(&[
            "2m", "3m", "4m", "5m", "6m", "7m", "2p", "3p", "4p", "5s", "5s",
        ])
    }

    fn menzen_tanyao_hand(source: &mut TileIdSource) -> Vec<TileId> {
        source.tiles(&[
            "2m", "3m", "4m", "3m", "4m", "5m", "2p", "3p", "4p", "5p", "6p", "7p", "5s", "5s",
        ])
    }

    fn honor_set_yaku(honor: &str, context: WinningContext) -> Vec<Yaku> {
        let mut source = TileIdSource::new();
        let concealed = honor_set_hand(&mut source, honor);
        let analysis = analyze_completed_hand(&concealed, &[]).unwrap();

        only_yaku(&evaluate_yaku(&analysis, context))
    }

    fn menzen_tanyao_yaku(context: WinningContext) -> Vec<Yaku> {
        let mut source = TileIdSource::new();
        let concealed = menzen_tanyao_hand(&mut source);
        let analysis = analyze_completed_hand(&concealed, &[]).unwrap();

        only_yaku(&evaluate_yaku(&analysis, context))
    }

    fn open_tanyao_yaku(context: WinningContext) -> Vec<Yaku> {
        let mut source = TileIdSource::new();
        let fixed = vec![source.meld(MeldKind::Chi, &["2p", "3p", "4p"])];
        let concealed = source.tiles(&[
            "2m", "3m", "4m", "3m", "4m", "5m", "5p", "6p", "7p", "5s", "5s",
        ]);
        let analysis = analyze_completed_hand(&concealed, &fixed).unwrap();

        assert!(!is_menzen(&fixed));
        only_yaku(&evaluate_yaku(&analysis, context))
    }

    #[test]
    fn every_dragon_set_has_its_own_yakuhai() {
        for (honor, expected) in [
            ("P", Yaku::YakuhaiWhite),
            ("F", Yaku::YakuhaiGreen),
            ("C", Yaku::YakuhaiRed),
        ] {
            assert_eq!(
                honor_set_yaku(honor, ron_context()),
                [expected],
                "honor: {honor}"
            );
        }
    }

    #[test]
    fn dragon_yakuhai_ignores_round_and_seat_wind() {
        assert_eq!(
            honor_set_yaku("F", wind_context(Some("S"), Some("W"))),
            [Yaku::YakuhaiGreen]
        );
    }

    #[test]
    fn round_wind_set_is_yakuhai() {
        assert_eq!(
            honor_set_yaku("E", wind_context(Some("E"), Some("S"))),
            [Yaku::YakuhaiRoundWind]
        );
    }

    #[test]
    fn round_wind_kan_is_yakuhai() {
        let mut source = TileIdSource::new();
        let fixed = vec![source.meld(MeldKind::Ankan, &["E", "E", "E", "E"])];
        let concealed = three_meld_rest(&mut source);
        let analysis = analyze_completed_hand(&concealed, &fixed).unwrap();

        assert_eq!(
            only_yaku(&evaluate_yaku(
                &analysis,
                wind_context(Some("E"), Some("S"))
            )),
            [Yaku::YakuhaiRoundWind]
        );
    }

    #[test]
    fn seat_wind_set_is_yakuhai() {
        assert_eq!(
            honor_set_yaku("E", wind_context(Some("S"), Some("E"))),
            [Yaku::YakuhaiSeatWind]
        );
    }

    #[test]
    fn seat_wind_kan_is_yakuhai() {
        let mut source = TileIdSource::new();
        let fixed = vec![source.meld(MeldKind::Ankan, &["E", "E", "E", "E"])];
        let concealed = three_meld_rest(&mut source);
        let analysis = analyze_completed_hand(&concealed, &fixed).unwrap();

        assert_eq!(
            only_yaku(&evaluate_yaku(
                &analysis,
                wind_context(Some("S"), Some("E"))
            )),
            [Yaku::YakuhaiSeatWind]
        );
    }

    #[test]
    fn double_wind_set_keeps_both_yakuhai_facts() {
        assert_eq!(
            honor_set_yaku("E", wind_context(Some("E"), Some("E"))),
            [Yaku::YakuhaiRoundWind, Yaku::YakuhaiSeatWind]
        );
    }

    #[test]
    fn guest_wind_set_is_not_yakuhai() {
        assert_eq!(honor_set_yaku("W", wind_context(Some("E"), Some("S"))), []);
    }

    #[test]
    fn unknown_wind_axis_is_never_guessed() {
        assert_eq!(
            honor_set_yaku("E", wind_context(Some("E"), None)),
            [Yaku::YakuhaiRoundWind]
        );
        assert_eq!(
            honor_set_yaku("E", wind_context(None, Some("E"))),
            [Yaku::YakuhaiSeatWind]
        );
        assert_eq!(honor_set_yaku("E", wind_context(None, None)), []);
    }

    #[test]
    fn yakuhai_covers_concealed_open_and_kan_sets() {
        assert_eq!(honor_set_yaku("P", ron_context()), [Yaku::YakuhaiWhite]);

        for (kind, tiles) in [
            (MeldKind::Pon, vec!["P", "P", "P"]),
            (MeldKind::Ankan, vec!["P", "P", "P", "P"]),
            (MeldKind::Daiminkan, vec!["P", "P", "P", "P"]),
            (MeldKind::Kakan, vec!["P", "P", "P", "P"]),
        ] {
            let mut source = TileIdSource::new();
            let fixed = vec![source.meld(kind, &tiles)];
            let concealed = three_meld_rest(&mut source);
            let analysis = analyze_completed_hand(&concealed, &fixed).unwrap();

            assert_eq!(
                only_yaku(&evaluate_yaku(&analysis, ron_context())),
                [Yaku::YakuhaiWhite],
                "kind: {kind:?}"
            );
        }
    }

    #[test]
    fn shousangen_keeps_both_dragon_yakuhai() {
        let mut source = TileIdSource::new();
        let concealed = source.tiles(&[
            "P", "P", "P", "F", "F", "F", "C", "C", "2m", "3m", "4m", "5m", "6m", "7m",
        ]);

        let analysis = analyze_completed_hand(&concealed, &[]).unwrap();

        assert_eq!(
            only_yaku(&evaluate_yaku(&analysis, ron_context())),
            [
                Yaku::Shousangen,
                Yaku::Honitsu,
                Yaku::YakuhaiWhite,
                Yaku::YakuhaiGreen
            ]
        );
    }

    #[test]
    fn menzen_riichi_is_scored() {
        assert_eq!(
            menzen_tanyao_yaku(ron_context().with_riichi(RiichiStatus::Riichi)),
            [Yaku::Tanyao, Yaku::Riichi]
        );
    }

    #[test]
    fn double_riichi_excludes_riichi() {
        let yaku = menzen_tanyao_yaku(ron_context().with_riichi(RiichiStatus::DoubleRiichi));

        assert!(yaku.contains(&Yaku::DoubleRiichi));
        assert!(!yaku.contains(&Yaku::Riichi));
    }

    #[test]
    fn open_hand_gets_no_riichi() {
        for riichi in [RiichiStatus::Riichi, RiichiStatus::DoubleRiichi] {
            let yaku = open_tanyao_yaku(ron_context().with_riichi(riichi));

            assert_eq!(yaku, [Yaku::Tanyao], "riichi: {riichi:?}");
        }
    }

    #[test]
    fn ankan_keeps_riichi_menzen() {
        let mut source = TileIdSource::new();
        let fixed = vec![source.meld(MeldKind::Ankan, &["2m", "2m", "2m", "2m"])];
        let concealed = source.tiles(&[
            "3m", "4m", "5m", "2p", "3p", "4p", "5p", "6p", "7p", "5s", "5s",
        ]);
        let analysis = analyze_completed_hand(&concealed, &fixed).unwrap();

        assert!(is_menzen(&fixed));
        assert_eq!(
            only_yaku(&evaluate_yaku(
                &analysis,
                ron_context().with_riichi(RiichiStatus::Riichi)
            )),
            [Yaku::Tanyao, Yaku::Riichi]
        );
    }

    #[test]
    fn riichi_ippatsu_is_scored() {
        let context = ron_context()
            .with_riichi(RiichiStatus::Riichi)
            .with_ippatsu(Some(true));

        assert_eq!(
            menzen_tanyao_yaku(context),
            [Yaku::Tanyao, Yaku::Riichi, Yaku::Ippatsu]
        );
    }

    #[test]
    fn double_riichi_ippatsu_is_scored() {
        let context = ron_context()
            .with_riichi(RiichiStatus::DoubleRiichi)
            .with_ippatsu(Some(true));

        assert_eq!(
            menzen_tanyao_yaku(context),
            [Yaku::Tanyao, Yaku::DoubleRiichi, Yaku::Ippatsu]
        );
    }

    #[test]
    fn ippatsu_without_riichi_is_never_guessed() {
        for riichi in [RiichiStatus::Unknown, RiichiStatus::NotDeclared] {
            let context = ron_context().with_riichi(riichi).with_ippatsu(Some(true));

            assert_eq!(
                menzen_tanyao_yaku(context),
                [Yaku::Tanyao],
                "riichi: {riichi:?}"
            );
        }
    }

    #[test]
    fn open_hand_gets_no_ippatsu() {
        let context = ron_context()
            .with_riichi(RiichiStatus::Riichi)
            .with_ippatsu(Some(true));

        assert_eq!(open_tanyao_yaku(context), [Yaku::Tanyao]);
    }

    #[test]
    fn unknown_ippatsu_is_not_scored() {
        let context = ron_context().with_riichi(RiichiStatus::Riichi);

        assert_eq!(menzen_tanyao_yaku(context), [Yaku::Tanyao, Yaku::Riichi]);
    }

    #[test]
    fn menzen_tsumo_needs_a_closed_hand_and_a_self_draw() {
        assert_eq!(
            menzen_tanyao_yaku(tsumo_context()),
            [Yaku::Tanyao, Yaku::MenzenTsumo]
        );
        assert_eq!(open_tanyao_yaku(tsumo_context()), [Yaku::Tanyao]);
        assert_eq!(menzen_tanyao_yaku(ron_context()), [Yaku::Tanyao]);
    }

    #[test]
    fn chankan_needs_a_ron() {
        assert_eq!(
            menzen_tanyao_yaku(ron_context().with_chankan(Some(true))),
            [Yaku::Tanyao, Yaku::Chankan]
        );
        assert_eq!(
            menzen_tanyao_yaku(tsumo_context().with_chankan(Some(true))),
            [Yaku::Tanyao, Yaku::MenzenTsumo]
        );
    }

    #[test]
    fn rinshan_kaihou_combines_with_menzen_tsumo() {
        assert_eq!(
            menzen_tanyao_yaku(tsumo_context().with_rinshan(Some(true))),
            [Yaku::Tanyao, Yaku::MenzenTsumo, Yaku::RinshanKaihou]
        );
        assert_eq!(
            menzen_tanyao_yaku(ron_context().with_rinshan(Some(true))),
            [Yaku::Tanyao]
        );
    }

    #[test]
    fn haitei_needs_the_last_live_tile_drawn_by_self() {
        let context = tsumo_context()
            .with_rinshan(Some(false))
            .with_remaining_live_tiles(Some(0));

        assert_eq!(
            menzen_tanyao_yaku(context),
            [Yaku::Tanyao, Yaku::MenzenTsumo, Yaku::Haitei]
        );
    }

    #[test]
    fn unknown_rinshan_is_not_a_confirmed_haitei() {
        let yaku = menzen_tanyao_yaku(tsumo_context().with_remaining_live_tiles(Some(0)));

        assert_eq!(yaku, [Yaku::Tanyao, Yaku::MenzenTsumo]);
        assert!(!yaku.contains(&Yaku::Haitei));
    }

    #[test]
    fn haitei_and_rinshan_kaihou_are_exclusive() {
        let context = tsumo_context()
            .with_rinshan(Some(true))
            .with_remaining_live_tiles(Some(0));
        let yaku = menzen_tanyao_yaku(context);

        assert!(yaku.contains(&Yaku::RinshanKaihou));
        assert!(!yaku.contains(&Yaku::Haitei));
    }

    #[test]
    fn houtei_needs_a_ron_on_the_last_discard() {
        let ron = menzen_tanyao_yaku(
            ron_context()
                .with_chankan(Some(false))
                .with_remaining_live_tiles(Some(0)),
        );
        assert_eq!(ron, [Yaku::Tanyao, Yaku::Houtei]);

        let tsumo = menzen_tanyao_yaku(
            tsumo_context()
                .with_rinshan(Some(false))
                .with_remaining_live_tiles(Some(0)),
        );
        assert!(!tsumo.contains(&Yaku::Houtei));
        assert!(tsumo.contains(&Yaku::Haitei));
    }

    #[test]
    fn unknown_chankan_is_not_a_confirmed_houtei() {
        let yaku = menzen_tanyao_yaku(ron_context().with_remaining_live_tiles(Some(0)));

        assert_eq!(yaku, [Yaku::Tanyao]);
        assert!(!yaku.contains(&Yaku::Houtei));
    }

    #[test]
    fn chankan_and_houtei_are_exclusive() {
        let context = ron_context()
            .with_chankan(Some(true))
            .with_remaining_live_tiles(Some(0));
        let yaku = menzen_tanyao_yaku(context);

        assert!(yaku.contains(&Yaku::Chankan));
        assert!(!yaku.contains(&Yaku::Houtei));
    }

    #[test]
    fn houtei_does_not_look_at_the_rinshan_fact() {
        for rinshan in [None, Some(false), Some(true)] {
            let context = ron_context()
                .with_chankan(Some(false))
                .with_rinshan(rinshan)
                .with_remaining_live_tiles(Some(0));

            assert!(
                menzen_tanyao_yaku(context).contains(&Yaku::Houtei),
                "rinshan: {rinshan:?}"
            );
        }
    }

    #[test]
    fn unknown_facts_never_become_yaku() {
        assert_eq!(menzen_tanyao_yaku(ron_context()), [Yaku::Tanyao]);
        assert_eq!(
            menzen_tanyao_yaku(tsumo_context()),
            [Yaku::Tanyao, Yaku::MenzenTsumo]
        );

        for context in [
            ron_context().with_remaining_live_tiles(None),
            tsumo_context().with_remaining_live_tiles(None),
            ron_context().with_remaining_live_tiles(Some(0)),
            tsumo_context().with_remaining_live_tiles(Some(0)),
        ] {
            let yaku = menzen_tanyao_yaku(context);

            assert!(!yaku.contains(&Yaku::Haitei), "context: {context:?}");
            assert!(!yaku.contains(&Yaku::Houtei), "context: {context:?}");
        }

        let unknown_events = menzen_tanyao_yaku(
            ron_context()
                .with_riichi(RiichiStatus::Unknown)
                .with_ippatsu(None)
                .with_chankan(None)
                .with_rinshan(None),
        );
        assert_eq!(unknown_events, [Yaku::Tanyao]);
    }

    #[test]
    fn structural_and_contextual_yaku_share_one_decomposition() {
        let mut source = TileIdSource::new();
        let concealed = menzen_tanyao_hand(&mut source);
        let analysis = analyze_completed_hand(&concealed, &[]).unwrap();

        let structural = evaluate_structural_yaku(&analysis);
        let evaluations = evaluate_yaku(&analysis, ron_context().with_riichi(RiichiStatus::Riichi));

        assert_eq!(only_yaku(&evaluations), [Yaku::Tanyao, Yaku::Riichi]);
        assert_eq!(evaluations.len(), structural.len());
        for (evaluation, structural) in evaluations.iter().zip(&structural) {
            assert_eq!(evaluation.decomposition(), structural.decomposition());
        }
    }

    #[test]
    fn contextual_yaku_is_added_to_every_decomposition() {
        let mut source = TileIdSource::new();
        let concealed = source.tiles(&[
            "1m", "1m", "1m", "2m", "2m", "2m", "3m", "3m", "3m", "4m", "4m", "4m", "5m", "5m",
        ]);

        let analysis = analyze_completed_hand(&concealed, &[]).unwrap();
        let context = tsumo_context().with_riichi(RiichiStatus::Riichi);
        let evaluations = evaluate_yaku(&analysis, context);

        let iipeikou = vec![
            Yaku::Iipeikou,
            Yaku::Chinitsu,
            Yaku::Riichi,
            Yaku::MenzenTsumo,
        ];
        let toitoi = vec![
            Yaku::Toitoi,
            Yaku::Chinitsu,
            Yaku::Riichi,
            Yaku::MenzenTsumo,
        ];
        assert_eq!(
            standard_yaku_sets(&evaluations),
            vec![iipeikou.clone(), iipeikou.clone(), iipeikou, toitoi]
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
                .all(|evaluation| evaluation.contains(Yaku::Riichi))
        );
    }

    #[test]
    fn chiitoitsu_and_standard_families_share_the_contextual_yaku() {
        let mut source = TileIdSource::new();
        let concealed = source.tiles(&[
            "1m", "1m", "2m", "2m", "3m", "3m", "4m", "4m", "5m", "5m", "6m", "6m", "7m", "7m",
        ]);

        let analysis = analyze_completed_hand(&concealed, &[]).unwrap();
        let evaluations = evaluate_yaku(&analysis, ron_context().with_riichi(RiichiStatus::Riichi));

        assert_eq!(
            standard_yaku_sets(&evaluations),
            vec![vec![Yaku::Ryanpeikou, Yaku::Chinitsu, Yaku::Riichi]; 3]
        );
        assert_eq!(
            chiitoitsu_yaku_set(&evaluations),
            Some(vec![Yaku::Chiitoitsu, Yaku::Chinitsu, Yaku::Riichi])
        );
    }

    #[test]
    fn malformed_fixed_meld_gets_no_contextual_yaku() {
        let mut source = TileIdSource::new();
        let fixed = vec![source.meld(MeldKind::Ankan, &["E", "E", "E", "S"])];
        let concealed = three_meld_rest(&mut source);
        let analysis = analyze_completed_hand(&concealed, &fixed).unwrap();
        let context = wind_context(Some("E"), Some("E"))
            .with_riichi(RiichiStatus::Riichi)
            .with_remaining_live_tiles(Some(0));

        assert_eq!(fixed[0].shape(), None);
        assert!(is_menzen(&fixed));
        assert!(analysis.is_complete());
        assert_eq!(only_yaku(&evaluate_structural_yaku(&analysis)), []);
        assert_eq!(only_yaku(&evaluate_yaku(&analysis, context)), []);
    }

    #[test]
    fn incomplete_hand_has_no_contextual_evaluation() {
        let mut source = TileIdSource::new();
        let concealed = source.tiles(&[
            "1m", "2m", "3m", "4m", "5m", "6m", "7m", "8m", "9m", "1p", "3p", "5s", "7s", "9s",
        ]);

        let analysis = analyze_completed_hand(&concealed, &[]).unwrap();

        assert!(
            evaluate_yaku(&analysis, ron_context().with_riichi(RiichiStatus::Riichi)).is_empty()
        );
    }

    #[test]
    fn contextual_yaku_lists_are_sorted_deduplicated_and_deterministic() {
        let mut source = TileIdSource::new();
        let concealed = honor_set_hand(&mut source, "E");
        let analysis = analyze_completed_hand(&concealed, &[]).unwrap();
        let context = wind_context(Some("E"), Some("E"))
            .with_riichi(RiichiStatus::DoubleRiichi)
            .with_ippatsu(Some(true))
            .with_chankan(Some(false))
            .with_remaining_live_tiles(Some(0));
        let evaluations = evaluate_yaku(&analysis, context);

        for evaluation in &evaluations {
            let mut expected = evaluation.yaku().to_vec();
            expected.sort_unstable();
            expected.dedup();
            assert_eq!(evaluation.yaku(), expected);
        }
        assert_eq!(
            only_yaku(&evaluations),
            [
                Yaku::YakuhaiRoundWind,
                Yaku::YakuhaiSeatWind,
                Yaku::DoubleRiichi,
                Yaku::Ippatsu,
                Yaku::Houtei
            ]
        );
        assert_eq!(evaluations, evaluate_yaku(&analysis, context));
    }
}
