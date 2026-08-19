use thiserror::Error;

use crate::completed_hand::{CompletedHandAnalysis, CompletedHandDecomposition};
use crate::payment::{Payment, PaymentError, evaluate_payment};
use crate::tile::TileType;
use crate::tile_counts::TileCounts;
use crate::winning_context::WinningContext;
use crate::winning_tile::{WaitType, WinningTileInterpretation};
use crate::winning_yakuman::{WinningYakumanEvaluation, evaluate_winning_yakuman};
use crate::yakuman::Yakuman;

const DEALER_SEAT_INDEX: u8 = 0;
const YAKUMAN_BASIC_POINTS: u32 = 8000;
const SINGLE_YAKUMAN_MULTIPLIER: u32 = 1;
const DOUBLE_YAKUMAN_MULTIPLIER: u32 = 2;
const NUMBER_COUNT: usize = 9;
const PURE_CHUUREN_COUNTS: [u8; NUMBER_COUNT] = [3, 1, 1, 1, 1, 1, 1, 1, 3];

#[derive(Debug, Error, Clone, Copy, PartialEq, Eq)]
pub enum YakumanScoringError {
    #[error("the winning context is missing the seat wind")]
    MissingSeatWind,

    #[error(transparent)]
    Payment(#[from] PaymentError),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct YakumanContribution {
    yakuman: Yakuman,
    multiplier: u32,
}

impl YakumanContribution {
    pub fn yakuman(self) -> Yakuman {
        self.yakuman
    }

    pub fn multiplier(self) -> u32 {
        self.multiplier
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct YakumanScoringCandidate<'a> {
    interpretation: WinningTileInterpretation<'a>,
    contributions: Vec<YakumanContribution>,
    total_multiplier: u32,
    basic_points: u32,
    payment: Payment,
}

impl<'a> YakumanScoringCandidate<'a> {
    pub fn interpretation(&self) -> WinningTileInterpretation<'a> {
        self.interpretation
    }

    pub fn decomposition(&self) -> &'a CompletedHandDecomposition {
        self.interpretation.decomposition()
    }

    pub fn contributions(&self) -> &[YakumanContribution] {
        &self.contributions
    }

    pub fn multiplier_of(&self, yakuman: Yakuman) -> Option<u32> {
        self.contributions
            .iter()
            .find(|contribution| contribution.yakuman() == yakuman)
            .map(|contribution| contribution.multiplier())
    }

    pub fn total_multiplier(&self) -> u32 {
        self.total_multiplier
    }

    pub fn basic_points(&self) -> u32 {
        self.basic_points
    }

    pub fn payment(&self) -> Payment {
        self.payment
    }
}

pub fn evaluate_yakuman_scoring<'a>(
    analysis: &'a CompletedHandAnalysis,
    context: WinningContext,
    winning_tile: TileType,
) -> Result<Vec<YakumanScoringCandidate<'a>>, YakumanScoringError> {
    let Some(seat_wind) = context.seat_wind() else {
        return Err(YakumanScoringError::MissingSeatWind);
    };
    let is_dealer = TileType::wind_from_seat_index(DEALER_SEAT_INDEX) == Some(seat_wind);

    evaluate_winning_yakuman(analysis, context, winning_tile)
        .into_iter()
        .filter(|evaluation| !evaluation.is_empty())
        .map(|evaluation| candidate(analysis, &evaluation, context, is_dealer))
        .collect()
}

fn candidate<'a>(
    analysis: &CompletedHandAnalysis,
    evaluation: &WinningYakumanEvaluation<'a>,
    context: WinningContext,
    is_dealer: bool,
) -> Result<YakumanScoringCandidate<'a>, YakumanScoringError> {
    let interpretation = evaluation.interpretation();
    let contributions: Vec<YakumanContribution> = evaluation
        .yakuman()
        .iter()
        .map(|yakuman| YakumanContribution {
            yakuman: *yakuman,
            multiplier: multiplier(*yakuman, analysis, &interpretation),
        })
        .collect();

    let total_multiplier = contributions
        .iter()
        .map(|contribution| contribution.multiplier())
        .sum();
    let basic_points = YAKUMAN_BASIC_POINTS * total_multiplier;

    Ok(YakumanScoringCandidate {
        interpretation,
        contributions,
        total_multiplier,
        basic_points,
        payment: evaluate_payment(basic_points, is_dealer, context.win_method())?,
    })
}

fn multiplier(
    yakuman: Yakuman,
    analysis: &CompletedHandAnalysis,
    interpretation: &WinningTileInterpretation<'_>,
) -> u32 {
    let doubled = match yakuman {
        Yakuman::KokushiMusou => interpretation.wait() == WaitType::KokushiThirteenSided,
        Yakuman::Suuankou => interpretation.group().is_pair(),
        Yakuman::ChuurenPoutou => is_pure_chuuren(analysis, interpretation.winning_tile()),
        Yakuman::Daisuushii => true,
        Yakuman::Ryuuiisou
        | Yakuman::Suukantsu
        | Yakuman::Chinroutou
        | Yakuman::Tsuuiisou
        | Yakuman::Daisangen
        | Yakuman::Shousuushii => false,
    };

    if doubled {
        DOUBLE_YAKUMAN_MULTIPLIER
    } else {
        SINGLE_YAKUMAN_MULTIPLIER
    }
}

fn is_pure_chuuren(analysis: &CompletedHandAnalysis, winning_tile: TileType) -> bool {
    let mut counts = TileCounts::from_tiles(analysis.concealed_tiles().iter().copied());
    if counts.remove(winning_tile).is_err() {
        return false;
    }

    let mut numbers = [0u8; NUMBER_COUNT];
    for (tile, count) in counts.iter().filter(|(_, count)| *count > 0) {
        let Some(number) = tile.number() else {
            return false;
        };
        numbers[usize::from(number - 1)] = count;
    }

    numbers == PURE_CHUUREN_COUNTS
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::completed_hand::analyze_completed_hand;
    use crate::meld::{Meld, MeldKind};
    use crate::payment::PaymentBreakdown;
    use crate::tile::TileId;
    use crate::winning_context::WinMethod;
    use crate::winning_tile::WinningGroup;

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

    fn seated(win_method: WinMethod, seat_wind: &str) -> WinningContext {
        WinningContext::new(win_method).with_seat_wind(Some(tile_type(seat_wind)))
    }

    fn ron() -> WinningContext {
        seated(WinMethod::Ron, "S")
    }

    fn tsumo() -> WinningContext {
        seated(WinMethod::Tsumo, "S")
    }

    fn dealer_ron() -> WinningContext {
        seated(WinMethod::Ron, "E")
    }

    fn candidates<'a>(
        analysis: &'a CompletedHandAnalysis,
        context: WinningContext,
        winning_tile: &str,
    ) -> Vec<YakumanScoringCandidate<'a>> {
        evaluate_yakuman_scoring(analysis, context, tile_type(winning_tile)).unwrap()
    }

    fn only<'a>(
        analysis: &'a CompletedHandAnalysis,
        context: WinningContext,
        winning_tile: &str,
    ) -> YakumanScoringCandidate<'a> {
        let candidates = candidates(analysis, context, winning_tile);

        assert_eq!(candidates.len(), 1, "candidates: {candidates:?}");
        candidates.into_iter().next().unwrap()
    }

    fn contributions(candidate: &YakumanScoringCandidate<'_>) -> Vec<(Yakuman, u32)> {
        candidate
            .contributions()
            .iter()
            .map(|contribution| (contribution.yakuman(), contribution.multiplier()))
            .collect()
    }

    fn score(candidate: &YakumanScoringCandidate<'_>) -> (u32, u32, u32) {
        (
            candidate.total_multiplier(),
            candidate.basic_points(),
            candidate.payment().total(),
        )
    }

    const KOKUSHI: [&str; 14] = [
        "1m", "1m", "9m", "1p", "9p", "1s", "9s", "E", "S", "W", "N", "P", "F", "C",
    ];
    const FOUR_CONCEALED_TRIPLETS: [&str; 14] = [
        "1m", "1m", "1m", "2m", "2m", "2m", "3p", "3p", "3p", "4s", "4s", "4s", "9p", "9p",
    ];
    const BIG_THREE_DRAGONS: [&str; 14] = [
        "P", "P", "P", "F", "F", "F", "C", "C", "C", "2m", "3m", "4m", "5m", "5m",
    ];
    const BIG_FOUR_WINDS_ALL_HONORS: [&str; 14] = [
        "E", "E", "E", "S", "S", "S", "W", "W", "W", "N", "N", "N", "P", "P",
    ];
    const SMALL_FOUR_WINDS: [&str; 14] = [
        "E", "E", "E", "S", "S", "S", "W", "W", "W", "2m", "3m", "4m", "N", "N",
    ];
    const CHUUREN_WITH_EXTRA_FIVE: [&str; 14] = [
        "1m", "1m", "1m", "2m", "3m", "4m", "5m", "5m", "6m", "7m", "8m", "9m", "9m", "9m",
    ];
    const GREEN_TWO_DECOMPOSITIONS: [&str; 14] = [
        "2s", "2s", "2s", "3s", "3s", "3s", "4s", "4s", "4s", "6s", "6s", "8s", "8s", "8s",
    ];
    const PINFU_TANYAO_HAND: [&str; 14] = [
        "2m", "3m", "4m", "3m", "4m", "5m", "4p", "5p", "6p", "6p", "7p", "8p", "5s", "5s",
    ];

    #[test]
    fn a_single_yakuman_is_eight_thousand_basic_points() {
        let analysis = analyze(&BIG_THREE_DRAGONS, &[]);
        let candidate = only(&analysis, ron(), "2m");

        assert_eq!(contributions(&candidate), vec![(Yakuman::Daisangen, 1)]);
        assert_eq!(score(&candidate), (1, 8000, 32000));
    }

    #[test]
    fn a_dealer_gets_the_dealer_ron_multiplier_for_the_same_basic_points() {
        let analysis = analyze(&BIG_THREE_DRAGONS, &[]);
        let candidate = only(&analysis, dealer_ron(), "2m");

        assert!(candidate.payment().is_dealer());
        assert_eq!(score(&candidate), (1, 8000, 48000));
        assert_eq!(
            candidate.payment().breakdown(),
            PaymentBreakdown::Ron { pay_ron: 48000 }
        );
    }

    #[test]
    fn only_the_seat_wind_decides_the_dealer() {
        let analysis = analyze(&BIG_THREE_DRAGONS, &[]);

        for (seat_wind, is_dealer, total) in [
            ("E", true, 48000),
            ("S", false, 32000),
            ("W", false, 32000),
            ("N", false, 32000),
        ] {
            let candidate = only(&analysis, seated(WinMethod::Ron, seat_wind), "2m");

            assert_eq!(candidate.basic_points(), 8000, "seat wind: {seat_wind}");
            assert_eq!(
                candidate.payment().is_dealer(),
                is_dealer,
                "seat wind: {seat_wind}"
            );
            assert_eq!(candidate.payment().total(), total, "seat wind: {seat_wind}");
        }
    }

    #[test]
    fn a_single_tile_kokushi_wait_is_a_single_yakuman() {
        let analysis = analyze(&KOKUSHI, &[]);
        let candidate = only(&analysis, ron(), "9m");

        assert_eq!(candidate.interpretation().wait(), WaitType::KokushiSingle);
        assert_eq!(contributions(&candidate), vec![(Yakuman::KokushiMusou, 1)]);
        assert_eq!(score(&candidate), (1, 8000, 32000));
    }

    #[test]
    fn a_thirteen_sided_kokushi_wait_is_a_double_yakuman() {
        let analysis = analyze(&KOKUSHI, &[]);
        let candidate = only(&analysis, ron(), "1m");

        assert_eq!(
            candidate.interpretation().wait(),
            WaitType::KokushiThirteenSided
        );
        assert_eq!(contributions(&candidate), vec![(Yakuman::KokushiMusou, 2)]);
        assert_eq!(score(&candidate), (2, 16000, 64000));
    }

    #[test]
    fn a_suuankou_completed_outside_the_pair_is_a_single_yakuman() {
        let analysis = analyze(&FOUR_CONCEALED_TRIPLETS, &[]);
        let candidate = only(&analysis, tsumo(), "4s");

        assert_eq!(
            candidate.interpretation().group(),
            WinningGroup::Triplet {
                tile: tile_type("4s")
            }
        );
        assert_eq!(contributions(&candidate), vec![(Yakuman::Suuankou, 1)]);
        assert_eq!(candidate.basic_points(), 8000);
    }

    #[test]
    fn a_suuankou_completed_by_the_pair_is_a_double_yakuman() {
        let analysis = analyze(&FOUR_CONCEALED_TRIPLETS, &[]);

        for context in [ron(), tsumo()] {
            let candidate = only(&analysis, context, "9p");

            assert!(candidate.interpretation().group().is_pair());
            assert_eq!(
                contributions(&candidate),
                vec![(Yakuman::Suuankou, 2)],
                "context: {context:?}"
            );
            assert_eq!(candidate.basic_points(), 16000, "context: {context:?}");
        }
    }

    #[test]
    fn daisuushii_is_a_double_yakuman() {
        let analysis = analyze(&BIG_FOUR_WINDS_ALL_HONORS, &[]);
        let candidate = only(&analysis, tsumo(), "E");

        assert_eq!(candidate.multiplier_of(Yakuman::Daisuushii), Some(2));
    }

    #[test]
    fn shousuushii_is_a_single_yakuman() {
        let analysis = analyze(&SMALL_FOUR_WINDS, &[]);
        let candidate = only(&analysis, ron(), "2m");

        assert_eq!(contributions(&candidate), vec![(Yakuman::Shousuushii, 1)]);
        assert_eq!(score(&candidate), (1, 8000, 32000));
    }

    #[test]
    fn a_chuuren_poutou_that_is_not_a_nine_sided_wait_is_a_single_yakuman() {
        let analysis = analyze(&CHUUREN_WITH_EXTRA_FIVE, &[]);
        let candidate = only(&analysis, ron(), "1m");

        assert_eq!(contributions(&candidate), vec![(Yakuman::ChuurenPoutou, 1)]);
        assert_eq!(score(&candidate), (1, 8000, 32000));
    }

    #[test]
    fn a_pure_chuuren_poutou_is_a_double_yakuman() {
        let analysis = analyze(&CHUUREN_WITH_EXTRA_FIVE, &[]);
        let candidate = only(&analysis, ron(), "5m");

        assert_eq!(contributions(&candidate), vec![(Yakuman::ChuurenPoutou, 2)]);
        assert_eq!(score(&candidate), (2, 16000, 64000));
    }

    #[test]
    fn composite_yakuman_multipliers_are_summed() {
        let analysis = analyze(&BIG_FOUR_WINDS_ALL_HONORS, &[]);
        let candidate = only(&analysis, tsumo(), "E");

        assert_eq!(
            contributions(&candidate),
            vec![
                (Yakuman::Suuankou, 1),
                (Yakuman::Tsuuiisou, 1),
                (Yakuman::Daisuushii, 2),
            ]
        );
        assert_eq!(score(&candidate), (4, 32000, 128000));
        assert_eq!(
            candidate.payment().breakdown(),
            PaymentBreakdown::NonDealerTsumo {
                pay_from_dealer: 64000,
                pay_from_non_dealer: 32000,
            }
        );
    }

    #[test]
    fn a_composite_yakuman_with_a_suuankou_tanki_is_five_times_a_yakuman() {
        let analysis = analyze(&BIG_FOUR_WINDS_ALL_HONORS, &[]);

        for context in [ron(), tsumo()] {
            let candidate = only(&analysis, context, "P");

            assert_eq!(
                contributions(&candidate),
                vec![
                    (Yakuman::Suuankou, 2),
                    (Yakuman::Tsuuiisou, 1),
                    (Yakuman::Daisuushii, 2),
                ],
                "context: {context:?}"
            );
            assert_eq!(candidate.total_multiplier(), 5, "context: {context:?}");
            assert_eq!(candidate.basic_points(), 40000, "context: {context:?}");
        }

        assert_eq!(only(&analysis, ron(), "P").payment().total(), 160000);
    }

    #[test]
    fn an_interpretation_without_a_yakuman_is_not_a_candidate() {
        let analysis = analyze(&PINFU_TANYAO_HAND, &[]);

        assert_eq!(candidates(&analysis, ron(), "5s"), []);
    }

    #[test]
    fn a_missing_seat_wind_has_no_exact_payment() {
        let analysis = analyze(&BIG_THREE_DRAGONS, &[]);

        assert_eq!(
            evaluate_yakuman_scoring(
                &analysis,
                WinningContext::new(WinMethod::Ron),
                tile_type("2m")
            ),
            Err(YakumanScoringError::MissingSeatWind)
        );
    }

    #[test]
    fn each_interpretation_keeps_its_own_multipliers() {
        let analysis = analyze(&GREEN_TWO_DECOMPOSITIONS, &[]);
        let candidates = candidates(&analysis, ron(), "6s");

        assert_eq!(candidates.len(), 2);
        assert_eq!(
            candidates.iter().map(contributions).collect::<Vec<_>>(),
            vec![
                vec![(Yakuman::Ryuuiisou, 1)],
                vec![(Yakuman::Ryuuiisou, 1), (Yakuman::Suuankou, 2)],
            ]
        );
        assert_eq!(
            candidates.iter().map(score).collect::<Vec<_>>(),
            vec![(1, 8000, 32000), (3, 24000, 96000)]
        );
        for candidate in &candidates {
            assert_eq!(
                candidate.decomposition(),
                candidate.interpretation().decomposition()
            );
        }
    }

    #[test]
    fn the_same_input_gives_the_same_candidates() {
        let analysis = analyze(&GREEN_TWO_DECOMPOSITIONS, &[]);

        assert_eq!(
            candidates(&analysis, ron(), "6s"),
            candidates(&analysis, ron(), "6s")
        );
    }
}
