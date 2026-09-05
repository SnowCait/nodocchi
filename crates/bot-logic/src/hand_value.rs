use thiserror::Error;

use crate::completed_hand::{CompletedHandAnalysis, CompletedHandDecomposition};
use crate::normal_hand_scoring::{
    NormalScoringCandidate, NormalScoringError, for_each_normal_scoring_candidate,
};
use crate::payment::Payment;
use crate::scoring_selection::{ScoringCandidateRef, ScoringSelection, ScoringSelector};
use crate::tile::{TileId, TileType};
use crate::winning_context::WinningContext;
use crate::winning_tile::{WinningTileInterpretation, interpret_winning_tile};
use crate::yakuman_scoring::{
    YakumanScoringCandidate, YakumanScoringError, for_each_yakuman_scoring_candidate,
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
    // 和了牌の解釈は和了状況にもドラにも依らないので、役と役満で1回だけ求める。
    let interpretations = interpret_winning_tile(analysis, winning_tile);
    let mut selection = ScoringSelector::new();
    let normal_scoring = for_each_normal_scoring_candidate(
        analysis,
        context,
        dora_indicators,
        ura_dora_indicators,
        &interpretations,
        |candidate| selection.consider_normal(candidate),
    );
    if let Err(yakuman_error) =
        for_each_yakuman_scoring_candidate(analysis, context, &interpretations, |candidate| {
            selection.consider_yakuman(candidate)
        })
    {
        return Err(match normal_scoring {
            Err(normal_error) => normal_error.into(),
            Ok(()) => yakuman_error.into(),
        });
    }
    match normal_scoring {
        Ok(()) => {}
        Err(NormalScoringError::IncompleteContext(_)) if selection.has_yakuman() => {
            selection.discard_normal();
        }
        Err(normal_error) => return Err(normal_error.into()),
    }

    Ok(match selection.finish() {
        ScoringSelection::NoCandidate => HandValueOutcome::NoCandidate,
        ScoringSelection::IndeterminateBonusHan => HandValueOutcome::IndeterminateBonusHan,
        ScoringSelection::Normal(candidate) => {
            HandValueOutcome::Known(HandValue::Normal(candidate))
        }
        ScoringSelection::Yakuman(candidate) => {
            HandValueOutcome::Known(HandValue::Yakuman(candidate))
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scoring_selection::{BestScoringSelection, select_best_scoring_candidate};

    use crate::completed_hand::analyze_completed_hand;
    use crate::meld::{Meld, MeldKind};
    use crate::normal_hand_scoring::MissingScoringFact;
    use crate::normal_hand_scoring::evaluate_normal_hand_scoring;
    use crate::normal_score::LimitClass;
    use crate::winning_context::{RiichiStatus, WinMethod};
    use crate::yaku::Yaku;
    use crate::yakuman::Yakuman;
    use crate::yakuman_scoring::evaluate_yakuman_scoring;

    fn hand_value<'h>(candidate: ScoringCandidateRef<'_, 'h>) -> HandValue<'h> {
        match candidate {
            ScoringCandidateRef::Normal(candidate) => HandValue::Normal(candidate.clone()),
            ScoringCandidateRef::Yakuman(candidate) => HandValue::Yakuman(candidate.clone()),
        }
    }

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

    fn yakuman_only_context(win_method: WinMethod) -> WinningContext {
        WinningContext::new(win_method).with_seat_wind(Some(tile_type("S")))
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
    const PINFU_TANYAO_AKA_HAND: [&str; 14] = [
        "2m", "3m", "4m", "3m", "4m", "5m", "4p", "5pr", "6p", "6p", "7p", "8p", "5s", "5s",
    ];
    const CHIITOITSU_HAND: [&str; 14] = [
        "1m", "1m", "3m", "3m", "5m", "5m", "7m", "7m", "9m", "9m", "1p", "1p", "E", "E",
    ];
    const VALUE_HONOR_HAND: [&str; 14] = [
        "P", "P", "P", "2m", "3m", "4m", "6p", "7p", "8p", "2s", "3s", "4s", "9m", "9m",
    ];
    const OPEN_HONITSU_REST: [&str; 11] = [
        "1p", "1p", "2p", "3p", "4p", "6p", "7p", "8p", "7p", "8p", "9p",
    ];

    // 差分検証の完成手。門前 / 副露・赤5 / 黒5・複数解釈・七対子・通常形・名前の付いた役満・
    // 数え役満にならない役・役なし・役牌・平和 / 非平和を含む。
    // 差分検証の完成手1件。(門前の牌, 固定面子, 和了牌)。
    type DifferentialHand = (
        &'static [&'static str],
        &'static [(MeldKind, &'static [&'static str])],
        &'static str,
    );

    const DIFFERENTIAL_HANDS: &[DifferentialHand] = &[
        (&PINFU_TANYAO_HAND, &[], "2m"),
        (&PINFU_TANYAO_AKA_HAND, &[], "2m"),
        (&SANANKOU_AND_IIPEIKOU, &[], "1m"),
        (&TANYAO_SUUANKOU, &[], "6p"),
        (&GREEN_TWO_DECOMPOSITIONS, &[], "6s"),
        (&CHIITOITSU_HAND, &[], "E"),
        (&KOKUSHI_HAND, &[], "1m"),
        (&VALUE_HONOR_HAND, &[], "9m"),
        (
            &NO_YAKU_OPEN_REST,
            &[(MeldKind::Chi, &["1m", "2m", "3m"])],
            "5s",
        ),
        (
            &OPEN_HONITSU_REST,
            &[(MeldKind::Pon, &["P", "P", "P"])],
            "9p",
        ),
    ];

    // 差分検証のドラ表示牌と裏ドラ表示牌。裏ドラ未確定 (`None`) と裏ドラ 0 枚も含む。
    const DIFFERENTIAL_DORA: &[(&[&str], Option<&[&str]>)] = &[
        (&[], None),
        (&["1m"], None),
        (&["1m"], Some(&[])),
        (&["1m"], Some(&["4p"])),
    ];

    // 差分検証の和了状況。ロン / ツモ・リーチ / ダマ・点数計算の入力不足 (役満だけ求まる
    // 局面を含む) を並べる。
    fn differential_contexts() -> Vec<WinningContext> {
        vec![
            ron(),
            tsumo(),
            riichi(ron()),
            riichi(tsumo()),
            ron().with_round_wind(None),
            yakuman_only_context(WinMethod::Ron),
            yakuman_only_context(WinMethod::Tsumo),
            WinningContext::new(WinMethod::Ron),
        ]
    }

    // 変更前の合成順。役と役満がそれぞれ独立に和了牌を解釈する既存 public 入口
    // ([`evaluate_normal_hand_scoring`] / [`evaluate_yakuman_scoring`]) を呼び、和了牌の解釈を
    // 2回列挙する。結論の選び方は実装と同じ既存 helper をそのまま使う。
    fn reference_outcome<'a>(
        analysis: &'a CompletedHandAnalysis,
        context: WinningContext,
        winning_tile: TileType,
        dora_indicators: &[TileId],
        ura_dora_indicators: Option<&[TileId]>,
    ) -> Result<HandValueOutcome<'a>, HandValueError> {
        let normal_scoring = evaluate_normal_hand_scoring(
            analysis,
            context,
            winning_tile,
            dora_indicators,
            ura_dora_indicators,
        );
        let yakuman_candidates = match evaluate_yakuman_scoring(analysis, context, winning_tile) {
            Ok(candidates) => candidates,
            Err(yakuman_error) => {
                return Err(match normal_scoring {
                    Err(normal_error) => normal_error.into(),
                    Ok(_) => yakuman_error.into(),
                });
            }
        };
        let normal_candidates = match normal_scoring {
            Ok(candidates) => candidates,
            Err(NormalScoringError::IncompleteContext(_)) if !yakuman_candidates.is_empty() => {
                Vec::new()
            }
            Err(normal_error) => return Err(normal_error.into()),
        };

        Ok(
            match select_best_scoring_candidate(&normal_candidates, &yakuman_candidates) {
                BestScoringSelection::NoCandidate => HandValueOutcome::NoCandidate,
                BestScoringSelection::IndeterminateBonusHan => {
                    HandValueOutcome::IndeterminateBonusHan
                }
                BestScoringSelection::Known(candidate) => {
                    HandValueOutcome::Known(hand_value(candidate))
                }
            },
        )
    }

    #[test]
    fn streaming_matches_materialized_hand_value_over_the_completed_hand_corpus() {
        for analysis in crate::completed_hand_corpus::analyses() {
            for context in crate::completed_hand_corpus::winning_contexts() {
                for raw in 0..TileType::COUNT as u8 {
                    let winning_tile = TileType::new(raw).unwrap();
                    for ura in [None, Some(&[][..])] {
                        assert_eq!(
                            evaluate_hand_value(&analysis, context, winning_tile, &[], ura),
                            reference_outcome(&analysis, context, winning_tile, &[], ura),
                            "{analysis:?} {context:?} {winning_tile:?} {ura:?}"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn normal_error_takes_precedence_when_both_routes_fail() {
        let setup = hand(&KOKUSHI_HAND);
        for (context, missing) in [
            (
                WinningContext::new(WinMethod::Ron),
                MissingScoringFact::RoundWind,
            ),
            (ron().with_seat_wind(None), MissingScoringFact::SeatWind),
        ] {
            assert_eq!(
                evaluate_yakuman_scoring(&setup.analysis, context, tile_type("9m")),
                Err(YakumanScoringError::MissingSeatWind)
            );
            assert_eq!(
                evaluate_hand_value(&setup.analysis, context, tile_type("9m"), &[], None),
                Err(HandValueError::NormalScoring(
                    NormalScoringError::IncompleteContext(missing)
                ))
            );
        }
    }

    #[test]
    fn the_shared_winning_tile_interpretation_matches_interpreting_it_twice() {
        // 役と役満で和了牌の解釈を共有する現在の実装と、それぞれが独立に解釈する変更前の
        // 合成順は、同じ完成手・同じ和了牌・同じ和了状況・同じドラに対して同じ結論になる。
        let mut known = 0;
        let mut named_yakuman = 0;
        let mut no_candidate = 0;
        let mut indeterminate = 0;
        let mut error = 0;

        for (concealed, fixed, winning_tile) in DIFFERENTIAL_HANDS {
            for (dora, ura) in DIFFERENTIAL_DORA {
                let mut source = TileIdSource::new();
                let fixed_melds: Vec<Meld> = fixed
                    .iter()
                    .map(|(kind, tiles)| source.meld(*kind, tiles))
                    .collect();
                let tiles = source.tiles(concealed);
                let analysis = analyze_completed_hand(&tiles, &fixed_melds).unwrap();
                let dora_indicators = source.tiles(dora);
                let ura_dora_indicators = ura.map(|ura| source.tiles(ura));

                for context in differential_contexts() {
                    let outcome = evaluate_hand_value(
                        &analysis,
                        context,
                        tile_type(winning_tile),
                        &dora_indicators,
                        ura_dora_indicators.as_deref(),
                    );
                    assert_eq!(
                        outcome,
                        reference_outcome(
                            &analysis,
                            context,
                            tile_type(winning_tile),
                            &dora_indicators,
                            ura_dora_indicators.as_deref(),
                        ),
                        "{concealed:?} {fixed:?} {winning_tile} {context:?} {dora:?} {ura:?}"
                    );

                    match &outcome {
                        Ok(HandValueOutcome::Known(hand_value)) => {
                            known += 1;
                            named_yakuman += usize::from(hand_value.is_yakuman());
                        }
                        Ok(HandValueOutcome::NoCandidate) => no_candidate += 1,
                        Ok(HandValueOutcome::IndeterminateBonusHan) => indeterminate += 1,
                        Err(_) => error += 1,
                    }
                }
            }
        }

        // 一致だけの空振りにせず、確定した点数・名前の付いた役満・役なし・裏ドラ未確定・
        // 点数計算の入力不足がどれも検証対象に含まれていることを確かめる。
        assert!(known > 0);
        assert!(named_yakuman > 0);
        assert!(no_candidate > 0);
        assert!(indeterminate > 0);
        assert!(error > 0);
    }

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
    fn a_named_yakuman_does_not_need_the_normal_exact_context() {
        let setup = hand(&KOKUSHI_HAND);
        let context = yakuman_only_context(WinMethod::Ron);

        assert_eq!(
            evaluate_normal_hand_scoring(&setup.analysis, context, tile_type("9m"), &[], None),
            Err(NormalScoringError::IncompleteContext(
                MissingScoringFact::RoundWind
            ))
        );

        let outcome = setup.outcome(context, "9m", None);
        let hand_value = outcome.known().unwrap();

        assert!(hand_value.is_yakuman());
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
    fn a_normal_only_hand_still_needs_the_normal_exact_context() {
        let setup = hand(&PINFU_TANYAO_HAND);
        let context = yakuman_only_context(WinMethod::Ron);

        assert_eq!(
            setup.yakuman_candidates(context, "2m"),
            [],
            "the tolerated context shortage needs a hand without a named yakuman"
        );
        assert_eq!(
            evaluate_hand_value(&setup.analysis, context, tile_type("2m"), &[], None),
            Err(HandValueError::NormalScoring(
                NormalScoringError::IncompleteContext(MissingScoringFact::RoundWind)
            ))
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
