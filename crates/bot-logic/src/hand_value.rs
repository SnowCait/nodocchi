use thiserror::Error;

use crate::completed_hand::{CompletedHandAnalysis, CompletedHandDecomposition};
use crate::normal_hand_scoring::{
    NormalScoringCandidate, NormalScoringError, evaluate_normal_hand_scoring,
};
use crate::payment::Payment;
use crate::scoring_selection::{
    BestScoringSelection, ScoringCandidateRef, select_best_scoring_candidate,
};
use crate::tile::{TileId, TileType};
use crate::winning_context::WinningContext;
use crate::winning_tile::WinningTileInterpretation;
use crate::yakuman_scoring::{
    YakumanScoringCandidate, YakumanScoringError, evaluate_yakuman_scoring,
};

#[derive(Debug, Error, Clone, Copy, PartialEq, Eq)]
pub enum HandValueError {
    #[error(transparent)]
    NormalScoring(#[from] NormalScoringError),

    #[error(transparent)]
    YakumanScoring(#[from] YakumanScoringError),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HandValue<'a> {
    Normal(NormalScoringCandidate<'a>),
    Yakuman(YakumanScoringCandidate<'a>),
}

impl<'a> HandValue<'a> {
    pub fn normal(&self) -> Option<&NormalScoringCandidate<'a>> {
        match self {
            Self::Normal(candidate) => Some(candidate),
            Self::Yakuman(_) => None,
        }
    }

    pub fn yakuman(&self) -> Option<&YakumanScoringCandidate<'a>> {
        match self {
            Self::Yakuman(candidate) => Some(candidate),
            Self::Normal(_) => None,
        }
    }

    pub fn candidate(&self) -> ScoringCandidateRef<'_, 'a> {
        match self {
            Self::Normal(candidate) => ScoringCandidateRef::Normal(candidate),
            Self::Yakuman(candidate) => ScoringCandidateRef::Yakuman(candidate),
        }
    }

    pub fn is_yakuman(&self) -> bool {
        self.candidate().is_yakuman()
    }

    pub fn interpretation(&self) -> WinningTileInterpretation<'a> {
        self.candidate().interpretation()
    }

    pub fn decomposition(&self) -> &'a CompletedHandDecomposition {
        self.interpretation().decomposition()
    }

    pub fn payment(&self) -> Option<Payment> {
        self.candidate().payment()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HandValueOutcome<'a> {
    NoCandidate,
    Known(HandValue<'a>),
    IndeterminateBonusHan,
}

impl<'a> HandValueOutcome<'a> {
    pub fn known(&self) -> Option<&HandValue<'a>> {
        match self {
            Self::Known(hand_value) => Some(hand_value),
            Self::NoCandidate | Self::IndeterminateBonusHan => None,
        }
    }

    pub fn into_known(self) -> Option<HandValue<'a>> {
        match self {
            Self::Known(hand_value) => Some(hand_value),
            Self::NoCandidate | Self::IndeterminateBonusHan => None,
        }
    }

    pub fn is_known(&self) -> bool {
        matches!(self, Self::Known(_))
    }

    pub fn is_indeterminate(&self) -> bool {
        matches!(self, Self::IndeterminateBonusHan)
    }
}

pub fn evaluate_hand_value<'a>(
    analysis: &'a CompletedHandAnalysis,
    context: WinningContext,
    winning_tile: TileType,
    dora_indicators: &[TileId],
    ura_dora_indicators: Option<&[TileId]>,
) -> Result<HandValueOutcome<'a>, HandValueError> {
    let normal_candidates = evaluate_normal_hand_scoring(
        analysis,
        context,
        winning_tile,
        dora_indicators,
        ura_dora_indicators,
    )?;
    let yakuman_candidates = evaluate_yakuman_scoring(analysis, context, winning_tile)?;

    Ok(
        match select_best_scoring_candidate(&normal_candidates, &yakuman_candidates) {
            BestScoringSelection::NoCandidate => HandValueOutcome::NoCandidate,
            BestScoringSelection::IndeterminateBonusHan => HandValueOutcome::IndeterminateBonusHan,
            BestScoringSelection::Known(candidate) => {
                HandValueOutcome::Known(hand_value(candidate))
            }
        },
    )
}

fn hand_value<'h>(candidate: ScoringCandidateRef<'_, 'h>) -> HandValue<'h> {
    match candidate {
        ScoringCandidateRef::Normal(candidate) => HandValue::Normal(candidate.clone()),
        ScoringCandidateRef::Yakuman(candidate) => HandValue::Yakuman(candidate.clone()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::completed_hand::analyze_completed_hand;
    use crate::meld::{Meld, MeldKind};
    use crate::normal_hand_scoring::MissingScoringFact;
    use crate::normal_score::LimitClass;
    use crate::winning_context::{RiichiStatus, WinMethod};
    use crate::yaku::Yaku;
    use crate::yakuman::Yakuman;

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
    }

    impl Setup {
        fn new(concealed: &[&str], fixed: &[(MeldKind, &[&str])], dora: &[&str]) -> Self {
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
            }
        }

        fn outcome(
            &self,
            context: WinningContext,
            winning_tile: &str,
            ura_dora_indicators: Option<&[TileId]>,
        ) -> HandValueOutcome<'_> {
            evaluate_hand_value(
                &self.analysis,
                context,
                tile_type(winning_tile),
                &self.dora_indicators,
                ura_dora_indicators,
            )
            .unwrap()
        }

        fn normal_candidates(
            &self,
            context: WinningContext,
            winning_tile: &str,
        ) -> Vec<NormalScoringCandidate<'_>> {
            evaluate_normal_hand_scoring(
                &self.analysis,
                context,
                tile_type(winning_tile),
                &self.dora_indicators,
                None,
            )
            .unwrap()
        }

        fn yakuman_candidates(
            &self,
            context: WinningContext,
            winning_tile: &str,
        ) -> Vec<YakumanScoringCandidate<'_>> {
            evaluate_yakuman_scoring(&self.analysis, context, tile_type(winning_tile)).unwrap()
        }
    }

    fn hand(concealed: &[&str]) -> Setup {
        Setup::new(concealed, &[], &[])
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

    fn riichi(context: WinningContext) -> WinningContext {
        context
            .with_riichi(RiichiStatus::Riichi)
            .with_ippatsu(Some(false))
    }

    const SANANKOU_AND_IIPEIKOU: [&str; 14] = [
        "7p", "7p", "7p", "1m", "2m", "3m", "6p", "6p", "6p", "8p", "8p", "8p", "3s", "3s",
    ];
    const TANYAO_SUUANKOU: [&str; 14] = [
        "2m", "2m", "2m", "3m", "3m", "3m", "4p", "4p", "4p", "5s", "5s", "5s", "6p", "6p",
    ];
    const GREEN_TWO_DECOMPOSITIONS: [&str; 14] = [
        "2s", "2s", "2s", "3s", "3s", "3s", "4s", "4s", "4s", "6s", "6s", "8s", "8s", "8s",
    ];
    const KOKUSHI_HAND: [&str; 14] = [
        "1m", "1m", "9m", "1p", "9p", "1s", "9s", "E", "S", "W", "N", "P", "F", "C",
    ];
    const PINFU_TANYAO_HAND: [&str; 14] = [
        "2m", "3m", "4m", "3m", "4m", "5m", "4p", "5p", "6p", "6p", "7p", "8p", "5s", "5s",
    ];
    const NO_YAKU_OPEN_REST: [&str; 11] = [
        "4p", "5p", "6p", "2s", "3s", "4s", "7s", "8s", "9s", "5s", "5s",
    ];

    #[test]
    fn the_best_normal_candidate_becomes_a_known_hand_value() {
        let setup = hand(&SANANKOU_AND_IIPEIKOU);
        let outcome = setup.outcome(ron(), "1m", None);
        let hand_value = outcome.known().unwrap();

        assert!(!hand_value.is_yakuman());
        assert!(hand_value.yakuman().is_none());
        assert_eq!(
            hand_value.payment().map(|payment| payment.total()),
            Some(3200)
        );
    }

    #[test]
    fn the_best_named_yakuman_candidate_becomes_a_known_hand_value() {
        let setup = hand(&KOKUSHI_HAND);
        let outcome = setup.outcome(ron(), "9m", None);
        let hand_value = outcome.known().unwrap();

        assert!(hand_value.is_yakuman());
        assert!(hand_value.normal().is_none());
        assert_eq!(
            hand_value
                .yakuman()
                .unwrap()
                .multiplier_of(Yakuman::KokushiMusou),
            Some(1)
        );
        assert_eq!(
            hand_value.payment().map(|payment| payment.total()),
            Some(32000)
        );
    }

    #[test]
    fn both_routes_keep_the_existing_selection_semantics() {
        let setup = hand(&TANYAO_SUUANKOU);
        let normal = setup.normal_candidates(tsumo(), "5s");
        let yakuman = setup.yakuman_candidates(tsumo(), "5s");

        assert_eq!(
            normal[0].payment().map(|payment| payment.total()),
            Some(8000)
        );
        assert_eq!(yakuman[0].payment().total(), 32000);
        assert_eq!(
            setup.outcome(tsumo(), "5s", None),
            HandValueOutcome::Known(HandValue::Yakuman(yakuman[0].clone()))
        );
    }

    #[test]
    fn the_facade_follows_the_underlying_selection() {
        for (concealed, context, winning_tile) in [
            (&SANANKOU_AND_IIPEIKOU, ron(), "1m"),
            (&GREEN_TWO_DECOMPOSITIONS, ron(), "6s"),
            (&PINFU_TANYAO_HAND, ron(), "2m"),
        ] {
            let setup = hand(concealed);
            let normal = setup.normal_candidates(context, winning_tile);
            let yakuman = setup.yakuman_candidates(context, winning_tile);
            let selected = select_best_scoring_candidate(&normal, &yakuman);

            assert_eq!(
                setup.outcome(context, winning_tile, None),
                HandValueOutcome::Known(hand_value(selected.known().unwrap())),
                "concealed: {concealed:?}"
            );
        }
    }

    #[test]
    fn no_candidate_at_all_is_not_a_hand_value() {
        let setup = Setup::new(
            &NO_YAKU_OPEN_REST,
            &[(MeldKind::Chi, &["1m", "2m", "3m"])],
            &["3p", "4s"],
        );

        assert_eq!(
            setup.outcome(ron(), "5s", None),
            HandValueOutcome::NoCandidate
        );
    }

    #[test]
    fn an_unknown_ura_dora_without_a_yakuman_has_no_exact_hand_value() {
        let setup = hand(&PINFU_TANYAO_HAND);
        let outcome = setup.outcome(riichi(ron()), "2m", None);

        assert_eq!(outcome, HandValueOutcome::IndeterminateBonusHan);
        assert!(outcome.is_indeterminate());
        assert!(!outcome.is_known());
    }

    #[test]
    fn an_observed_empty_ura_dora_is_not_indeterminate() {
        let setup = hand(&PINFU_TANYAO_HAND);
        let outcome = setup.outcome(riichi(ron()), "2m", Some(&[]));

        assert_eq!(
            outcome
                .known()
                .and_then(HandValue::payment)
                .map(|payment| payment.total()),
            Some(3900)
        );
    }

    #[test]
    fn a_named_yakuman_resolves_an_unknown_ura_dora() {
        let setup = hand(&TANYAO_SUUANKOU);
        let outcome = setup.outcome(riichi(tsumo()), "5s", None);
        let hand_value = outcome.known().unwrap();

        assert!(hand_value.is_yakuman());
        assert_eq!(
            hand_value.payment().map(|payment| payment.total()),
            Some(32000)
        );
    }

    #[test]
    fn the_selected_normal_hand_value_keeps_its_diagnostics() {
        let setup = hand(&SANANKOU_AND_IIPEIKOU);
        let outcome = setup.outcome(ron(), "1m", None);
        let hand_value = outcome.known().unwrap();
        let candidate = hand_value.normal().unwrap();

        assert_eq!(
            candidate
                .yaku_han()
                .iter()
                .map(|yaku_han| (yaku_han.yaku(), yaku_han.han()))
                .collect::<Vec<_>>(),
            vec![(Yaku::Sanankou, 2)]
        );
        assert_eq!(candidate.bonus_han().bonus_han_total(), Some(0));
        assert_eq!(candidate.fu().fu(), 50);
        assert_eq!(candidate.total_han(), Some(2));
        assert_eq!(candidate.score_base().unwrap().basic_points(), 800);
        assert_eq!(candidate.score_base().unwrap().limit(), LimitClass::NoLimit);
        assert_eq!(hand_value.interpretation(), candidate.interpretation());
        assert_eq!(hand_value.decomposition(), candidate.decomposition());
        assert_eq!(hand_value.payment(), candidate.payment());
    }

    #[test]
    fn the_selected_yakuman_hand_value_keeps_its_diagnostics() {
        let setup = hand(&GREEN_TWO_DECOMPOSITIONS);
        let outcome = setup.outcome(ron(), "6s", None);
        let hand_value = outcome.known().unwrap();
        let candidate = hand_value.yakuman().unwrap();

        assert_eq!(
            candidate
                .contributions()
                .iter()
                .map(|contribution| (contribution.yakuman(), contribution.multiplier()))
                .collect::<Vec<_>>(),
            vec![(Yakuman::Ryuuiisou, 1), (Yakuman::Suuankou, 2)]
        );
        assert_eq!(candidate.total_multiplier(), 3);
        assert_eq!(candidate.basic_points(), 24000);
        assert_eq!(hand_value.interpretation(), candidate.interpretation());
        assert_eq!(hand_value.decomposition(), candidate.decomposition());
        assert_eq!(hand_value.payment(), Some(candidate.payment()));
        assert_eq!(candidate.payment().total(), 96000);
    }

    #[test]
    fn the_dora_indicators_reach_the_bonus_han() {
        let setup = Setup::new(&SANANKOU_AND_IIPEIKOU, &[], &["2s"]);
        let outcome = setup.outcome(ron(), "1m", None);
        let candidate = outcome.known().unwrap().normal().unwrap();

        assert_eq!(candidate.bonus_han().indicated_dora(), 2);
        assert_eq!(candidate.total_han(), Some(4));
        assert_eq!(
            candidate.payment().map(|payment| payment.total()),
            Some(8000)
        );
    }

    #[test]
    fn an_incomplete_context_is_reported_as_a_scoring_error() {
        let setup = hand(&SANANKOU_AND_IIPEIKOU);
        let context = ron().with_round_wind(None);

        assert_eq!(
            evaluate_hand_value(&setup.analysis, context, tile_type("1m"), &[], None),
            Err(HandValueError::NormalScoring(
                NormalScoringError::IncompleteContext(MissingScoringFact::RoundWind)
            ))
        );
    }
}
