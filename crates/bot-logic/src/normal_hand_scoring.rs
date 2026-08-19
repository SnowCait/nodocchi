use thiserror::Error;

use crate::bonus_han::{BonusHanBreakdown, evaluate_bonus_han};
use crate::completed_hand::{CompletedHandAnalysis, CompletedHandDecomposition};
use crate::fu::{FuBreakdown, WinningFuEvaluation, evaluate_winning_fu};
use crate::han::{WinningYakuHanEvaluation, YakuHan, evaluate_winning_yaku_han};
use crate::normal_score::{NormalScoreBase, NormalScoreError, evaluate_normal_score_base};
use crate::payment::{Payment, PaymentError, evaluate_payment};
use crate::tile::{TileId, TileType};
use crate::winning_context::{WinMethod, WinningContext};
use crate::winning_tile::WinningTileInterpretation;

const DEALER_SEAT_INDEX: u8 = 0;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum MissingScoringFact {
    RoundWind,
    SeatWind,
    RiichiStatus,
    Ippatsu,
    Rinshan,
    Chankan,
    RemainingLiveTiles,
}

#[derive(Debug, Error, Clone, Copy, PartialEq, Eq)]
pub enum NormalScoringError {
    #[error("the winning context is missing an exact scoring fact: {0:?}")]
    IncompleteContext(MissingScoringFact),

    #[error(transparent)]
    NormalScore(#[from] NormalScoreError),

    #[error(transparent)]
    Payment(#[from] PaymentError),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum NormalScoringState {
    Known {
        total_han: u8,
        score_base: NormalScoreBase,
        payment: Payment,
    },
    UnknownBonusHan,
}

impl NormalScoringState {
    pub fn is_known(self) -> bool {
        matches!(self, Self::Known { .. })
    }

    pub fn total_han(self) -> Option<u8> {
        match self {
            Self::Known { total_han, .. } => Some(total_han),
            Self::UnknownBonusHan => None,
        }
    }

    pub fn score_base(self) -> Option<NormalScoreBase> {
        match self {
            Self::Known { score_base, .. } => Some(score_base),
            Self::UnknownBonusHan => None,
        }
    }

    pub fn payment(self) -> Option<Payment> {
        match self {
            Self::Known { payment, .. } => Some(payment),
            Self::UnknownBonusHan => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NormalScoringCandidate<'a> {
    interpretation: WinningTileInterpretation<'a>,
    yaku_han: Vec<YakuHan>,
    fu: FuBreakdown,
    bonus_han: BonusHanBreakdown,
    state: NormalScoringState,
}

impl<'a> NormalScoringCandidate<'a> {
    pub fn interpretation(&self) -> WinningTileInterpretation<'a> {
        self.interpretation
    }

    pub fn decomposition(&self) -> &'a CompletedHandDecomposition {
        self.interpretation.decomposition()
    }

    pub fn yaku_han(&self) -> &[YakuHan] {
        &self.yaku_han
    }

    pub fn yaku_han_total(&self) -> u8 {
        self.yaku_han.iter().map(|yaku_han| yaku_han.han()).sum()
    }

    pub fn bonus_han(&self) -> BonusHanBreakdown {
        self.bonus_han
    }

    pub fn fu(&self) -> &FuBreakdown {
        &self.fu
    }

    pub fn state(&self) -> NormalScoringState {
        self.state
    }

    pub fn total_han(&self) -> Option<u8> {
        self.state.total_han()
    }

    pub fn score_base(&self) -> Option<NormalScoreBase> {
        self.state.score_base()
    }

    pub fn payment(&self) -> Option<Payment> {
        self.state.payment()
    }
}

pub fn evaluate_normal_hand_scoring<'a>(
    analysis: &'a CompletedHandAnalysis,
    context: WinningContext,
    winning_tile: TileType,
    dora_indicators: &[TileId],
    ura_dora_indicators: Option<&[TileId]>,
) -> Result<Vec<NormalScoringCandidate<'a>>, NormalScoringError> {
    let seat_wind = exact_scoring_context(context)?;
    let is_dealer = TileType::wind_from_seat_index(DEALER_SEAT_INDEX) == Some(seat_wind);

    let bonus_han = evaluate_bonus_han(analysis, context, dora_indicators, ura_dora_indicators);
    let fu_evaluations = evaluate_winning_fu(analysis, context, winning_tile);

    evaluate_winning_yaku_han(analysis, context, winning_tile)
        .into_iter()
        .filter(|evaluation| !evaluation.is_empty())
        .filter_map(|evaluation| {
            let fu = fu_breakdown(&fu_evaluations, evaluation.interpretation())?;
            Some(candidate(
                &evaluation,
                fu.clone(),
                bonus_han,
                context,
                is_dealer,
            ))
        })
        .collect()
}

fn exact_scoring_context(context: WinningContext) -> Result<TileType, NormalScoringError> {
    if context.round_wind().is_none() {
        return Err(incomplete_context(MissingScoringFact::RoundWind));
    }
    let Some(seat_wind) = context.seat_wind() else {
        return Err(incomplete_context(MissingScoringFact::SeatWind));
    };

    let Some(riichi_declared) = context.riichi().is_declared() else {
        return Err(incomplete_context(MissingScoringFact::RiichiStatus));
    };
    if riichi_declared && context.ippatsu().is_none() {
        return Err(incomplete_context(MissingScoringFact::Ippatsu));
    }

    let missing_win_method_fact = match context.win_method() {
        WinMethod::Ron => context
            .chankan()
            .is_none()
            .then_some(MissingScoringFact::Chankan),
        WinMethod::Tsumo => context
            .rinshan()
            .is_none()
            .then_some(MissingScoringFact::Rinshan),
    };
    if let Some(fact) = missing_win_method_fact {
        return Err(incomplete_context(fact));
    }

    if context.remaining_live_tiles().is_none() {
        return Err(incomplete_context(MissingScoringFact::RemainingLiveTiles));
    }

    Ok(seat_wind)
}

fn incomplete_context(fact: MissingScoringFact) -> NormalScoringError {
    NormalScoringError::IncompleteContext(fact)
}

fn fu_breakdown<'a, 'b>(
    evaluations: &'a [WinningFuEvaluation<'b>],
    interpretation: WinningTileInterpretation<'b>,
) -> Option<&'a FuBreakdown> {
    evaluations
        .iter()
        .find(|evaluation| evaluation.interpretation() == interpretation)
        .and_then(WinningFuEvaluation::breakdown)
}

fn candidate<'a>(
    evaluation: &WinningYakuHanEvaluation<'a>,
    fu: FuBreakdown,
    bonus_han: BonusHanBreakdown,
    context: WinningContext,
    is_dealer: bool,
) -> Result<NormalScoringCandidate<'a>, NormalScoringError> {
    let state = scoring_state(
        evaluation.yaku_han_total(),
        fu.fu(),
        bonus_han,
        context,
        is_dealer,
    )?;

    Ok(NormalScoringCandidate {
        interpretation: evaluation.interpretation(),
        yaku_han: evaluation.yaku_han().to_vec(),
        fu,
        bonus_han,
        state,
    })
}

fn scoring_state(
    yaku_han_total: u8,
    fu: u8,
    bonus_han: BonusHanBreakdown,
    context: WinningContext,
    is_dealer: bool,
) -> Result<NormalScoringState, NormalScoringError> {
    let Some(bonus_han_total) = bonus_han.bonus_han_total() else {
        return Ok(NormalScoringState::UnknownBonusHan);
    };

    let total_han = yaku_han_total + bonus_han_total;
    let score_base = evaluate_normal_score_base(total_han, fu)?;
    let payment = evaluate_payment(score_base.basic_points(), is_dealer, context.win_method())?;

    Ok(NormalScoringState::Known {
        total_han,
        score_base,
        payment,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bonus_han::UraDoraHan;
    use crate::completed_hand::analyze_completed_hand;
    use crate::fu::{FuKind, evaluate_winning_fu};
    use crate::meld::{Meld, MeldKind};
    use crate::normal_score::LimitClass;
    use crate::payment::PaymentBreakdown;
    use crate::winning_context::{RiichiStatus, WinMethod};
    use crate::winning_tile::WaitType;
    use crate::yaku::Yaku;

    struct TileIdSource {
        used: [bool; TileId::COUNT],
    }

    impl TileIdSource {
        fn new() -> Self {
            Self {
                used: [false; TileId::COUNT],
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
            let red = s.ends_with('r');
            let id = (0..4)
                .filter_map(|copy| TileId::new(tile_type.raw() * 4 + copy))
                .find(|id| id.is_red() == red && !self.used[id.index()])
                .unwrap();
            self.used[id.index()] = true;
            id
        }
    }

    fn tile_type(s: &str) -> TileType {
        TileType::from_mjai_type_str(s.trim_end_matches('r')).unwrap()
    }

    struct Setup {
        analysis: CompletedHandAnalysis,
        dora_indicators: Vec<TileId>,
        ura_dora_indicators: Option<Vec<TileId>>,
    }

    impl Setup {
        fn new(
            concealed: &[&str],
            fixed: &[(MeldKind, &[&str])],
            dora: &[&str],
            ura: Option<&[&str]>,
        ) -> Self {
            let mut source = TileIdSource::new();
            let fixed_melds: Vec<Meld> = fixed
                .iter()
                .map(|(kind, tiles)| source.meld(*kind, tiles))
                .collect();
            let tiles = source.tiles(concealed);
            let analysis = analyze_completed_hand(&tiles, &fixed_melds).unwrap();
            Self {
                analysis,
                dora_indicators: source.tiles(dora),
                ura_dora_indicators: ura.map(|ura| source.tiles(ura)),
            }
        }

        fn try_candidates(
            &self,
            context: WinningContext,
            winning_tile: &str,
        ) -> Result<Vec<NormalScoringCandidate<'_>>, NormalScoringError> {
            evaluate_normal_hand_scoring(
                &self.analysis,
                context,
                tile_type(winning_tile),
                &self.dora_indicators,
                self.ura_dora_indicators.as_deref(),
            )
        }

        fn candidates(
            &self,
            context: WinningContext,
            winning_tile: &str,
        ) -> Vec<NormalScoringCandidate<'_>> {
            self.try_candidates(context, winning_tile).unwrap()
        }

        fn only(&self, context: WinningContext, winning_tile: &str) -> NormalScoringCandidate<'_> {
            let candidates = self.candidates(context, winning_tile);

            assert_eq!(candidates.len(), 1, "candidates: {candidates:?}");
            candidates.into_iter().next().unwrap()
        }
    }

    fn hand(concealed: &[&str]) -> Setup {
        Setup::new(concealed, &[], &[], None)
    }

    fn hand_with_dora(concealed: &[&str], dora: &[&str]) -> Setup {
        Setup::new(concealed, &[], dora, None)
    }

    fn known_context(win_method: WinMethod) -> WinningContext {
        WinningContext::new(win_method)
            .with_round_wind(Some(tile_type("E")))
            .with_seat_wind(Some(tile_type("S")))
            .with_riichi(RiichiStatus::NotDeclared)
            .with_chankan(Some(false))
            .with_rinshan(Some(false))
            .with_remaining_live_tiles(Some(1))
    }

    fn ron() -> WinningContext {
        known_context(WinMethod::Ron)
    }

    fn tsumo() -> WinningContext {
        known_context(WinMethod::Tsumo)
    }

    fn dealer_ron() -> WinningContext {
        ron().with_seat_wind(Some(tile_type("E")))
    }

    fn dealer_tsumo() -> WinningContext {
        tsumo().with_seat_wind(Some(tile_type("E")))
    }

    fn riichi_ron() -> WinningContext {
        ron()
            .with_riichi(RiichiStatus::Riichi)
            .with_ippatsu(Some(false))
    }

    fn yaku(candidate: &NormalScoringCandidate<'_>) -> Vec<(Yaku, u8)> {
        candidate
            .yaku_han()
            .iter()
            .map(|yaku_han| (yaku_han.yaku(), yaku_han.han()))
            .collect()
    }

    fn score(candidate: &NormalScoringCandidate<'_>) -> (u8, u8, u32, LimitClass, u32) {
        let score_base = candidate.score_base().unwrap();
        (
            candidate.total_han().unwrap(),
            score_base.fu(),
            score_base.basic_points(),
            score_base.limit(),
            candidate.payment().unwrap().total(),
        )
    }

    const PINFU_TANYAO_HAND: [&str; 14] = [
        "2m", "3m", "4m", "3m", "4m", "5m", "4p", "5p", "6p", "6p", "7p", "8p", "5s", "5s",
    ];
    const PINFU_TANYAO_AKA_HAND: [&str; 14] = [
        "2m", "3m", "4m", "3m", "4m", "5m", "4p", "5pr", "6p", "6p", "7p", "8p", "5s", "5s",
    ];
    const PENCHAN_WAIT_HAND: [&str; 14] = [
        "1m", "2m", "3m", "4m", "5m", "6m", "4p", "5p", "6p", "6p", "7p", "8p", "5s", "5s",
    ];
    const PENCHAN_AND_RYANMEN: [&str; 14] = [
        "1m", "2m", "3m", "3m", "4m", "5m", "4p", "5p", "6p", "5p", "5p", "7s", "8s", "9s",
    ];
    const CHIITOITSU_HAND: [&str; 14] = [
        "1m", "1m", "3m", "3m", "5m", "5m", "7m", "7m", "9m", "9m", "1p", "1p", "E", "E",
    ];
    const RYANPEIKOU_CHIITOITSU: [&str; 14] = [
        "2p", "2p", "3p", "3p", "4p", "4p", "6p", "6p", "7p", "7p", "8p", "8p", "5s", "5s",
    ];
    const KOKUSHI_HAND: [&str; 14] = [
        "1m", "9m", "1p", "9p", "1s", "9s", "E", "S", "W", "N", "P", "F", "C", "9s",
    ];
    const NO_YAKU_OPEN_REST: [&str; 11] = [
        "4p", "5p", "6p", "2s", "3s", "4s", "7s", "8s", "9s", "5s", "5s",
    ];

    #[test]
    fn a_simple_hand_reaches_its_payment_through_the_existing_layers() {
        let setup = hand(&PINFU_TANYAO_HAND);
        let candidate = setup.only(ron(), "2m");

        assert_eq!(yaku(&candidate), vec![(Yaku::Pinfu, 1), (Yaku::Tanyao, 1)]);
        assert_eq!(candidate.yaku_han_total(), 2);
        assert_eq!(candidate.fu().fu(), 30);
        assert_eq!(score(&candidate), (2, 30, 480, LimitClass::NoLimit, 2000));
        assert_eq!(
            candidate.payment().unwrap().breakdown(),
            PaymentBreakdown::Ron { pay_ron: 2000 }
        );
    }

    #[test]
    fn an_east_seat_is_scored_as_the_dealer() {
        let setup = hand(&PINFU_TANYAO_HAND);
        let candidate = setup.only(dealer_ron(), "2m");

        assert!(candidate.payment().unwrap().is_dealer());
        assert_eq!(score(&candidate), (2, 30, 480, LimitClass::NoLimit, 2900));
        assert_eq!(
            candidate.payment().unwrap().breakdown(),
            PaymentBreakdown::Ron { pay_ron: 2900 }
        );
    }

    #[test]
    fn an_east_seat_tsumo_is_scored_as_the_dealer() {
        let setup = hand(&PINFU_TANYAO_HAND);
        let candidate = setup.only(dealer_tsumo(), "2m");

        assert!(candidate.payment().unwrap().is_dealer());
        assert_eq!(score(&candidate), (3, 20, 640, LimitClass::NoLimit, 3900));
        assert_eq!(
            candidate.payment().unwrap().breakdown(),
            PaymentBreakdown::DealerTsumo {
                pay_from_non_dealer: 1300,
            }
        );
    }

    #[test]
    fn only_the_east_seat_is_scored_as_the_dealer() {
        let setup = hand(&PINFU_TANYAO_HAND);

        for (seat_wind, is_dealer, payment_total) in [
            ("E", true, 2900),
            ("S", false, 2000),
            ("W", false, 2000),
            ("N", false, 2000),
        ] {
            let context = ron().with_seat_wind(Some(tile_type(seat_wind)));
            let candidate = setup.only(context, "2m");

            assert_eq!(
                candidate.payment().unwrap().is_dealer(),
                is_dealer,
                "seat wind: {seat_wind}"
            );
            assert_eq!(
                score(&candidate),
                (2, 30, 480, LimitClass::NoLimit, payment_total),
                "seat wind: {seat_wind}"
            );
        }
    }

    #[test]
    fn indicated_dora_is_added_to_the_scoring_han() {
        for (indicators, indicated_dora, expected) in [
            (Vec::new(), 0, (2, 30, 480, LimitClass::NoLimit, 2000)),
            (vec!["1m"], 1, (3, 30, 960, LimitClass::NoLimit, 3900)),
            (vec!["2m"], 2, (4, 30, 1920, LimitClass::NoLimit, 7700)),
        ] {
            let setup = hand_with_dora(&PINFU_TANYAO_HAND, &indicators);
            let candidate = setup.only(ron(), "2m");

            assert_eq!(
                candidate.bonus_han().indicated_dora(),
                indicated_dora,
                "indicators: {indicators:?}"
            );
            assert_eq!(candidate.yaku_han_total(), 2, "indicators: {indicators:?}");
            assert_eq!(score(&candidate), expected, "indicators: {indicators:?}");
        }
    }

    #[test]
    fn aka_dora_is_added_to_the_scoring_han() {
        let setup = hand(&PINFU_TANYAO_AKA_HAND);
        let candidate = setup.only(ron(), "2m");

        assert_eq!(candidate.bonus_han().indicated_dora(), 0);
        assert_eq!(candidate.bonus_han().aka_dora(), 1);
        assert_eq!(candidate.bonus_han().bonus_han_total(), Some(1));
        assert_eq!(score(&candidate), (3, 30, 960, LimitClass::NoLimit, 3900));
    }

    #[test]
    fn an_indicated_red_five_counts_as_both_dora_and_aka_dora() {
        let setup = hand_with_dora(&PINFU_TANYAO_AKA_HAND, &["4p"]);
        let candidate = setup.only(ron(), "2m");

        assert_eq!(candidate.bonus_han().indicated_dora(), 1);
        assert_eq!(candidate.bonus_han().aka_dora(), 1);
        assert_eq!(candidate.bonus_han().bonus_han_total(), Some(2));
        assert_eq!(score(&candidate), (4, 30, 1920, LimitClass::NoLimit, 7700));
    }

    #[test]
    fn dora_alone_is_not_a_normal_scoring_candidate() {
        let setup = Setup::new(
            &NO_YAKU_OPEN_REST,
            &[(MeldKind::Chi, &["1m", "2m", "3m"])],
            &["3p", "4s"],
            None,
        );

        assert_eq!(
            evaluate_winning_yaku_han(&setup.analysis, ron(), tile_type("4p"))
                .iter()
                .map(WinningYakuHanEvaluation::yaku_han_total)
                .collect::<Vec<_>>(),
            vec![0]
        );
        assert_eq!(
            evaluate_bonus_han(&setup.analysis, ron(), &setup.dora_indicators, None)
                .bonus_han_total(),
            Some(3)
        );
        assert!(setup.candidates(ron(), "4p").is_empty());
    }

    #[test]
    fn known_ura_dora_reaches_the_limit_and_the_payment() {
        let setup = Setup::new(&PINFU_TANYAO_HAND, &[], &[], Some(&["2m", "1m"]));
        let candidate = setup.only(riichi_ron(), "2m");

        assert_eq!(
            yaku(&candidate),
            vec![(Yaku::Pinfu, 1), (Yaku::Tanyao, 1), (Yaku::Riichi, 1)]
        );
        assert_eq!(candidate.bonus_han().ura_dora(), UraDoraHan::Known(3));
        assert_eq!(score(&candidate), (6, 30, 3000, LimitClass::Haneman, 12000));
    }

    #[test]
    fn observed_empty_ura_dora_indicators_still_give_an_exact_score() {
        let setup = Setup::new(&PINFU_TANYAO_HAND, &[], &[], Some(&[]));
        let candidate = setup.only(riichi_ron(), "2m");

        assert_eq!(candidate.bonus_han().ura_dora(), UraDoraHan::Known(0));
        assert_eq!(candidate.bonus_han().bonus_han_total(), Some(0));
        assert_eq!(score(&candidate), (3, 30, 960, LimitClass::NoLimit, 3900));
    }

    #[test]
    fn unknown_ura_dora_keeps_the_candidate_without_an_exact_score() {
        let setup = Setup::new(&PINFU_TANYAO_HAND, &[], &["1m"], None);
        let candidate = setup.only(riichi_ron(), "2m");

        assert_eq!(candidate.bonus_han().ura_dora(), UraDoraHan::Unknown);
        assert_eq!(candidate.bonus_han().non_ura_bonus_han(), 1);
        assert_eq!(candidate.bonus_han().bonus_han_total(), None);
        assert_eq!(candidate.state(), NormalScoringState::UnknownBonusHan);
        assert!(!candidate.state().is_known());
        assert_eq!(candidate.total_han(), None);
        assert_eq!(candidate.score_base(), None);
        assert_eq!(candidate.payment(), None);
        assert_eq!(candidate.yaku_han_total(), 3);
        assert_eq!(candidate.fu().fu(), 30);
    }

    #[test]
    fn a_hand_without_riichi_is_scored_even_when_no_ura_indicator_is_given() {
        let setup = hand(&PINFU_TANYAO_HAND);
        let candidate = setup.only(ron(), "2m");

        assert_eq!(candidate.bonus_han().ura_dora(), UraDoraHan::Ineligible);
        assert_eq!(candidate.bonus_han().bonus_han_total(), Some(0));
        assert!(candidate.state().is_known());
        assert_eq!(score(&candidate), (2, 30, 480, LimitClass::NoLimit, 2000));
    }

    #[test]
    fn a_pinfu_tsumo_keeps_the_twenty_fu_of_the_existing_layer() {
        let setup = hand(&PINFU_TANYAO_HAND);
        let candidate = setup.only(tsumo(), "2m");

        assert_eq!(candidate.fu().kind(), FuKind::PinfuTsumo);
        assert_eq!(candidate.fu().fu(), 20);
        assert_eq!(score(&candidate), (3, 20, 640, LimitClass::NoLimit, 2700));
        assert_eq!(
            candidate.payment().unwrap().breakdown(),
            PaymentBreakdown::NonDealerTsumo {
                pay_from_dealer: 1300,
                pay_from_non_dealer: 700,
            }
        );
    }

    #[test]
    fn a_chiitoitsu_hand_keeps_the_twenty_five_fu_of_the_existing_layer() {
        let setup = hand(&CHIITOITSU_HAND);
        let candidate = setup.only(ron(), "E");

        assert_eq!(candidate.fu().kind(), FuKind::Chiitoitsu);
        assert_eq!(candidate.fu().fu(), 25);
        assert_eq!(score(&candidate), (2, 25, 400, LimitClass::NoLimit, 1600));
    }

    #[test]
    fn four_han_thirty_fu_stays_below_the_mangan() {
        let setup = hand_with_dora(&PINFU_TANYAO_HAND, &["2m"]);
        let candidate = setup.only(ron(), "2m");

        assert_eq!(score(&candidate), (4, 30, 1920, LimitClass::NoLimit, 7700));
    }

    #[test]
    fn four_han_forty_fu_is_a_mangan() {
        let setup = Setup::new(&PENCHAN_WAIT_HAND, &[], &["5p", "3p"], Some(&[]));
        let candidate = setup.only(riichi_ron(), "3m");

        assert_eq!(candidate.interpretation().wait(), WaitType::Penchan);
        assert_eq!(candidate.fu().fu(), 40);
        assert_eq!(candidate.bonus_han().indicated_dora(), 3);
        assert_eq!(score(&candidate), (4, 40, 2000, LimitClass::Mangan, 8000));
    }

    #[test]
    fn thirteen_or_more_han_is_a_single_kazoe_yakuman() {
        for (dora, ura, total_han) in [
            (vec!["2m", "3m", "5p", "4s"], vec!["2m", "3m"], 15),
            (
                vec!["2m", "3m", "5p", "4s", "1m", "4m", "3p", "6p"],
                vec!["2m", "3m", "5p", "4s", "1m", "4m", "3p", "6p"],
                27,
            ),
        ] {
            let setup = Setup::new(&PINFU_TANYAO_HAND, &[], &dora, Some(&ura));
            let candidate = setup.only(riichi_ron(), "2m");

            assert_eq!(
                score(&candidate),
                (total_han, 30, 8000, LimitClass::KazoeYakuman, 32000),
                "dora: {dora:?}"
            );
        }
    }

    #[test]
    fn interpretations_of_one_decomposition_keep_their_own_han_and_fu() {
        let setup = Setup::new(&PENCHAN_AND_RYANMEN, &[], &[], Some(&[]));
        let candidates = setup.candidates(riichi_ron(), "3m");

        assert_eq!(candidates.len(), 2);
        assert_eq!(candidates[0].decomposition(), candidates[1].decomposition());
        assert_eq!(
            candidates
                .iter()
                .map(|candidate| (
                    candidate.interpretation().wait(),
                    candidate.yaku_han_total(),
                    candidate.fu().fu(),
                    candidate.total_han().unwrap(),
                    candidate.payment().unwrap().total(),
                ))
                .collect::<Vec<_>>(),
            vec![
                (WaitType::Penchan, 1, 40, 1, 1300),
                (WaitType::Ryanmen, 2, 30, 2, 2000),
            ]
        );
    }

    #[test]
    fn every_candidate_keeps_the_han_and_fu_of_its_own_interpretation() {
        let setup = Setup::new(&PENCHAN_AND_RYANMEN, &[], &[], Some(&[]));
        let context = riichi_ron();
        let winning_tile = tile_type("3m");
        let yaku_han = evaluate_winning_yaku_han(&setup.analysis, context, winning_tile);
        let fu = evaluate_winning_fu(&setup.analysis, context, winning_tile);
        let candidates = setup.candidates(context, "3m");

        assert!(!candidates.is_empty());
        for candidate in &candidates {
            let interpretation = candidate.interpretation();
            let expected_yaku_han = yaku_han
                .iter()
                .find(|evaluation| evaluation.interpretation() == interpretation)
                .unwrap();
            let expected_fu = fu
                .iter()
                .find(|evaluation| evaluation.interpretation() == interpretation)
                .unwrap();

            assert_eq!(candidate.yaku_han(), expected_yaku_han.yaku_han());
            assert_eq!(Some(candidate.fu()), expected_fu.breakdown());
        }
    }

    #[test]
    fn decompositions_of_one_hand_become_separate_candidates() {
        let setup = Setup::new(&RYANPEIKOU_CHIITOITSU, &[], &[], None);
        let candidates = setup.candidates(ron(), "5s");

        assert_eq!(candidates.len(), 2);
        assert_ne!(candidates[0].decomposition(), candidates[1].decomposition());
        assert_eq!(
            candidates
                .iter()
                .map(|candidate| (
                    candidate.fu().kind(),
                    candidate.fu().fu(),
                    candidate.total_han().unwrap(),
                    candidate.score_base().unwrap().basic_points(),
                    candidate.payment().unwrap().total(),
                ))
                .collect::<Vec<_>>(),
            vec![
                (FuKind::Standard, 40, 4, 2000, 8000),
                (FuKind::Chiitoitsu, 25, 3, 800, 3200),
            ]
        );
    }

    #[test]
    fn an_interpretation_without_fu_is_not_scored_as_a_normal_hand() {
        let setup = Setup::new(&KOKUSHI_HAND, &[], &[], Some(&[]));

        assert!(
            evaluate_winning_fu(&setup.analysis, riichi_ron(), tile_type("9s"))
                .iter()
                .all(|evaluation| evaluation.breakdown().is_none())
        );
        assert!(setup.candidates(riichi_ron(), "9s").is_empty());
    }

    #[test]
    fn an_incomplete_winning_context_is_rejected_before_any_scoring() {
        let setup = hand(&PINFU_TANYAO_HAND);

        for (context, fact) in [
            (ron().with_round_wind(None), MissingScoringFact::RoundWind),
            (ron().with_seat_wind(None), MissingScoringFact::SeatWind),
            (
                ron().with_riichi(RiichiStatus::Unknown),
                MissingScoringFact::RiichiStatus,
            ),
            (riichi_ron().with_ippatsu(None), MissingScoringFact::Ippatsu),
            (ron().with_chankan(None), MissingScoringFact::Chankan),
            (tsumo().with_rinshan(None), MissingScoringFact::Rinshan),
            (
                ron().with_remaining_live_tiles(None),
                MissingScoringFact::RemainingLiveTiles,
            ),
            (
                tsumo().with_remaining_live_tiles(None),
                MissingScoringFact::RemainingLiveTiles,
            ),
        ] {
            assert_eq!(
                setup.try_candidates(context, "2m"),
                Err(NormalScoringError::IncompleteContext(fact)),
                "context: {context:?}"
            );
        }
    }

    #[test]
    fn a_ron_does_not_need_the_rinshan_of_a_tsumo() {
        let setup = hand(&PINFU_TANYAO_HAND);

        assert!(setup.try_candidates(ron().with_rinshan(None), "2m").is_ok());
        assert!(
            setup
                .try_candidates(tsumo().with_chankan(None), "2m")
                .is_ok()
        );
    }

    #[test]
    fn a_hand_without_riichi_does_not_need_the_ippatsu_fact() {
        let setup = hand(&PINFU_TANYAO_HAND);
        let candidate = setup.only(ron().with_ippatsu(None), "2m");

        assert_eq!(score(&candidate), (2, 30, 480, LimitClass::NoLimit, 2000));
    }

    #[test]
    fn an_unknown_chankan_is_not_treated_as_a_confirmed_non_chankan() {
        let setup = hand(&PINFU_TANYAO_HAND);

        assert_eq!(
            setup.try_candidates(ron().with_chankan(None), "2m"),
            Err(NormalScoringError::IncompleteContext(
                MissingScoringFact::Chankan
            ))
        );
        assert_eq!(
            score(&setup.only(ron().with_chankan(Some(false)), "2m")),
            (2, 30, 480, LimitClass::NoLimit, 2000)
        );
        assert_eq!(
            score(&setup.only(ron().with_chankan(Some(true)), "2m")),
            (3, 30, 960, LimitClass::NoLimit, 3900)
        );
    }

    #[test]
    fn an_unknown_rinshan_is_not_treated_as_a_confirmed_non_rinshan() {
        let setup = hand(&PINFU_TANYAO_HAND);

        assert_eq!(
            setup.try_candidates(tsumo().with_rinshan(None), "2m"),
            Err(NormalScoringError::IncompleteContext(
                MissingScoringFact::Rinshan
            ))
        );
        assert_eq!(
            score(&setup.only(tsumo().with_rinshan(Some(false)), "2m")),
            (3, 20, 640, LimitClass::NoLimit, 2700)
        );
        assert_eq!(
            score(&setup.only(tsumo().with_rinshan(Some(true)), "2m")),
            (4, 20, 1280, LimitClass::NoLimit, 5200)
        );
    }

    #[test]
    fn unknown_remaining_live_tiles_are_not_treated_as_a_confirmed_non_houtei() {
        let setup = hand(&PINFU_TANYAO_HAND);

        assert_eq!(
            setup.try_candidates(ron().with_remaining_live_tiles(None), "2m"),
            Err(NormalScoringError::IncompleteContext(
                MissingScoringFact::RemainingLiveTiles
            ))
        );
        assert_eq!(
            score(&setup.only(ron().with_remaining_live_tiles(Some(1)), "2m")),
            (2, 30, 480, LimitClass::NoLimit, 2000)
        );
        assert_eq!(
            score(&setup.only(ron().with_remaining_live_tiles(Some(0)), "2m")),
            (3, 30, 960, LimitClass::NoLimit, 3900)
        );
    }

    #[test]
    fn known_ura_dora_does_not_complete_an_incomplete_context() {
        let setup = Setup::new(&PINFU_TANYAO_HAND, &[], &[], Some(&[]));

        assert_eq!(
            setup.try_candidates(riichi_ron().with_ippatsu(None), "2m"),
            Err(NormalScoringError::IncompleteContext(
                MissingScoringFact::Ippatsu
            ))
        );
        assert_eq!(
            score(&setup.only(riichi_ron(), "2m")),
            (3, 30, 960, LimitClass::NoLimit, 3900)
        );
    }

    #[test]
    fn an_unknown_ura_dora_is_not_an_incomplete_context() {
        let setup = Setup::new(&PINFU_TANYAO_HAND, &[], &[], None);
        let candidates = setup.try_candidates(riichi_ron(), "2m").unwrap();

        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].bonus_han().ura_dora(), UraDoraHan::Unknown);
        assert_eq!(candidates[0].state(), NormalScoringState::UnknownBonusHan);
        assert_eq!(candidates[0].yaku_han_total(), 3);
        assert_eq!(candidates[0].fu().fu(), 30);
    }

    #[test]
    fn candidates_are_deterministic() {
        let setup = Setup::new(&PENCHAN_AND_RYANMEN, &[], &["1m"], Some(&[]));

        assert_eq!(
            setup.candidates(riichi_ron(), "3m"),
            setup.candidates(riichi_ron(), "3m")
        );
    }
}
