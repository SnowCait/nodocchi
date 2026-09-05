use std::borrow::Borrow;

use crate::normal_hand_scoring::{NormalScoringCandidate, NormalScoringState};
use crate::payment::Payment;
use crate::winning_tile::WinningTileInterpretation;
use crate::yakuman_scoring::YakumanScoringCandidate;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScoringCandidateRef<'a, 'h> {
    Normal(&'a NormalScoringCandidate<'h>),
    Yakuman(&'a YakumanScoringCandidate<'h>),
}

impl<'a, 'h> ScoringCandidateRef<'a, 'h> {
    pub fn normal(self) -> Option<&'a NormalScoringCandidate<'h>> {
        match self {
            Self::Normal(candidate) => Some(candidate),
            Self::Yakuman(_) => None,
        }
    }

    pub fn yakuman(self) -> Option<&'a YakumanScoringCandidate<'h>> {
        match self {
            Self::Yakuman(candidate) => Some(candidate),
            Self::Normal(_) => None,
        }
    }

    pub fn is_yakuman(self) -> bool {
        matches!(self, Self::Yakuman(_))
    }

    pub fn interpretation(self) -> WinningTileInterpretation<'h> {
        match self {
            Self::Normal(candidate) => candidate.interpretation(),
            Self::Yakuman(candidate) => candidate.interpretation(),
        }
    }

    pub fn payment(self) -> Option<Payment> {
        match self {
            Self::Normal(candidate) => candidate.payment(),
            Self::Yakuman(candidate) => Some(candidate.payment()),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BestScoringSelection<'a, 'h> {
    NoCandidate,
    Known(ScoringCandidateRef<'a, 'h>),
    IndeterminateBonusHan,
}

impl<'a, 'h> BestScoringSelection<'a, 'h> {
    pub fn known(self) -> Option<ScoringCandidateRef<'a, 'h>> {
        match self {
            Self::Known(candidate) => Some(candidate),
            Self::NoCandidate | Self::IndeterminateBonusHan => None,
        }
    }

    pub fn is_known(self) -> bool {
        matches!(self, Self::Known(_))
    }

    pub fn is_indeterminate(self) -> bool {
        matches!(self, Self::IndeterminateBonusHan)
    }
}

pub fn select_best_scoring_candidate<'a, 'h>(
    normal_candidates: &'a [NormalScoringCandidate<'h>],
    yakuman_candidates: &'a [YakumanScoringCandidate<'h>],
) -> BestScoringSelection<'a, 'h> {
    let mut selection = ScoringSelector::new();
    for candidate in normal_candidates {
        selection.consider_normal(candidate);
    }
    for candidate in yakuman_candidates {
        selection.consider_yakuman(candidate);
    }
    match selection.finish() {
        ScoringSelection::NoCandidate => BestScoringSelection::NoCandidate,
        ScoringSelection::Normal(candidate) => {
            BestScoringSelection::Known(ScoringCandidateRef::Normal(candidate))
        }
        ScoringSelection::Yakuman(candidate) => {
            BestScoringSelection::Known(ScoringCandidateRef::Yakuman(candidate))
        }
        ScoringSelection::IndeterminateBonusHan => BestScoringSelection::IndeterminateBonusHan,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct KnownNormalScore {
    payment_total: u32,
    total_han: u8,
    fu: u8,
}

pub(crate) enum ScoringSelection<N, Y> {
    NoCandidate,
    Normal(N),
    Yakuman(Y),
    IndeterminateBonusHan,
}

/// public slice API は参照を、best-value 経路は候補そのものを保持する。
/// 比較規則と unknown の扱いは両方で共有し、完全同点なら先の候補を残す。
pub(crate) struct ScoringSelector<N, Y> {
    best_normal: Option<(N, KnownNormalScore)>,
    best_yakuman: Option<Y>,
    unknown_normal: bool,
}

impl<'h, N, Y> ScoringSelector<N, Y>
where
    N: Borrow<NormalScoringCandidate<'h>>,
    Y: Borrow<YakumanScoringCandidate<'h>>,
{
    pub(crate) fn new() -> Self {
        Self {
            best_normal: None,
            best_yakuman: None,
            unknown_normal: false,
        }
    }

    pub(crate) fn consider_normal(&mut self, candidate: N) {
        let Some(score) = known_normal_score(candidate.borrow()) else {
            self.unknown_normal = true;
            return;
        };
        match self.best_normal {
            Some((_, best_score)) if !normal_score_is_better(score, best_score) => {}
            _ => self.best_normal = Some((candidate, score)),
        }
    }

    pub(crate) fn consider_yakuman(&mut self, candidate: Y) {
        match &self.best_yakuman {
            Some(best) if !yakuman_candidate_is_better(candidate.borrow(), best.borrow()) => {}
            _ => self.best_yakuman = Some(candidate),
        }
    }

    pub(crate) fn has_yakuman(&self) -> bool {
        self.best_yakuman.is_some()
    }

    pub(crate) fn discard_normal(&mut self) {
        self.best_normal = None;
        self.unknown_normal = false;
    }

    pub(crate) fn finish(self) -> ScoringSelection<N, Y> {
        if let Some(yakuman) = self.best_yakuman {
            return match self.best_normal {
                Some((normal, score))
                    if score.payment_total > yakuman.borrow().payment().total() =>
                {
                    ScoringSelection::Normal(normal)
                }
                _ => ScoringSelection::Yakuman(yakuman),
            };
        }
        if self.unknown_normal {
            return ScoringSelection::IndeterminateBonusHan;
        }
        match self.best_normal {
            Some((candidate, _)) => ScoringSelection::Normal(candidate),
            None => ScoringSelection::NoCandidate,
        }
    }
}

fn known_normal_score(candidate: &NormalScoringCandidate<'_>) -> Option<KnownNormalScore> {
    match candidate.state() {
        NormalScoringState::Known {
            total_han,
            score_base,
            payment,
        } => Some(KnownNormalScore {
            payment_total: payment.total(),
            total_han,
            fu: score_base.fu(),
        }),
        NormalScoringState::UnknownBonusHan => None,
    }
}

fn normal_score_is_better(candidate: KnownNormalScore, best: KnownNormalScore) -> bool {
    if candidate.payment_total != best.payment_total {
        return candidate.payment_total > best.payment_total;
    }
    if candidate.total_han != best.total_han {
        return candidate.total_han > best.total_han;
    }
    candidate.fu > best.fu
}

fn yakuman_candidate_is_better(
    candidate: &YakumanScoringCandidate<'_>,
    best: &YakumanScoringCandidate<'_>,
) -> bool {
    if candidate.payment().total() != best.payment().total() {
        return candidate.payment().total() > best.payment().total();
    }
    candidate.total_multiplier() > best.total_multiplier()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::completed_hand::{CompletedHandAnalysis, analyze_completed_hand};
    use crate::meld::{Meld, MeldKind};
    use crate::normal_hand_scoring::evaluate_normal_hand_scoring;
    use crate::normal_score::LimitClass;
    use crate::tile::{TileId, TileType};
    use crate::winning_context::{RiichiStatus, WinMethod, WinningContext};
    use crate::winning_tile::WaitType;
    use crate::yaku::Yaku;
    use crate::yakuman::Yakuman;
    use crate::yakuman_scoring::evaluate_yakuman_scoring;

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

    fn payment_totals(candidates: &[NormalScoringCandidate<'_>]) -> Vec<Option<u32>> {
        candidates
            .iter()
            .map(|candidate| candidate.payment().map(|payment| payment.total()))
            .collect()
    }

    const SANANKOU_AND_IIPEIKOU: [&str; 14] = [
        "7p", "7p", "7p", "1m", "2m", "3m", "6p", "6p", "6p", "8p", "8p", "8p", "3s", "3s",
    ];
    const HONITSU_SOUZU: [&str; 14] = [
        "6s", "7s", "8s", "7s", "8s", "9s", "6s", "7s", "8s", "5sr", "6s", "7s", "N", "N",
    ];
    const ANKAN_HONITSU_REST: [&str; 11] = [
        "7p", "8p", "9p", "6p", "7p", "8p", "E", "E", "E", "6p", "6p",
    ];
    const TWO_KANCHAN_DECOMPOSITIONS: [&str; 14] = [
        "8m", "8m", "8m", "8p", "8p", "8p", "5p", "6p", "7p", "4p", "5p", "6p", "2s", "2s",
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
    fn the_highest_payment_wins_among_known_normal_candidates() {
        let setup = hand(&SANANKOU_AND_IIPEIKOU);
        let normal = setup.normal_candidates(ron(), "1m");
        let yakuman = setup.yakuman_candidates(ron(), "1m");

        assert_eq!(payment_totals(&normal), vec![Some(2000), Some(3200)]);
        assert_eq!(
            select_best_scoring_candidate(&normal, &yakuman),
            BestScoringSelection::Known(ScoringCandidateRef::Normal(&normal[1]))
        );
    }

    #[test]
    fn the_selected_candidate_keeps_its_diagnostics() {
        let setup = hand(&SANANKOU_AND_IIPEIKOU);
        let normal = setup.normal_candidates(ron(), "1m");
        let yakuman = setup.yakuman_candidates(ron(), "1m");
        let best = select_best_scoring_candidate(&normal, &yakuman)
            .known()
            .unwrap();
        let candidate = best.normal().unwrap();

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
        assert_eq!(candidate.score_base().unwrap().basic_points(), 800);
        assert_eq!(best.interpretation(), candidate.interpretation());
        assert_eq!(best.payment(), candidate.payment());
    }

    #[test]
    fn a_lower_han_candidate_with_a_higher_payment_is_better() {
        let mangan = KnownNormalScore {
            payment_total: 8000,
            total_han: 3,
            fu: 70,
        };
        let four_han = KnownNormalScore {
            payment_total: 7700,
            total_han: 4,
            fu: 30,
        };

        assert!(normal_score_is_better(mangan, four_han));
        assert!(!normal_score_is_better(four_han, mangan));
    }

    #[test]
    fn a_named_yakuman_wins_against_a_cheaper_normal_candidate() {
        let setup = hand(&TANYAO_SUUANKOU);
        let normal = setup.normal_candidates(tsumo(), "5s");
        let yakuman = setup.yakuman_candidates(tsumo(), "5s");

        assert_eq!(payment_totals(&normal), vec![Some(8000)]);
        assert_eq!(yakuman[0].payment().total(), 32000);
        assert_eq!(
            select_best_scoring_candidate(&normal, &yakuman),
            BestScoringSelection::Known(ScoringCandidateRef::Yakuman(&yakuman[0]))
        );
    }

    #[test]
    fn a_named_yakuman_wins_the_tie_against_a_kazoe_yakuman() {
        let setup = Setup::new(&TANYAO_SUUANKOU, &[], &["1m", "2m", "3p"]);
        let normal = setup.normal_candidates(tsumo(), "5s");
        let yakuman = setup.yakuman_candidates(tsumo(), "5s");
        let score_base = normal[0].score_base().unwrap();

        assert_eq!(score_base.han(), 13);
        assert_eq!(score_base.limit(), LimitClass::KazoeYakuman);
        assert_eq!(payment_totals(&normal), vec![Some(32000)]);
        assert_eq!(yakuman[0].total_multiplier(), 1);
        assert_eq!(yakuman[0].payment().total(), 32000);
        assert_eq!(
            select_best_scoring_candidate(&normal, &yakuman),
            BestScoringSelection::Known(ScoringCandidateRef::Yakuman(&yakuman[0]))
        );
    }

    #[test]
    fn the_highest_payment_wins_among_yakuman_candidates() {
        let setup = hand(&GREEN_TWO_DECOMPOSITIONS);
        let normal = setup.normal_candidates(ron(), "6s");
        let yakuman = setup.yakuman_candidates(ron(), "6s");

        assert_eq!(
            yakuman
                .iter()
                .map(|candidate| (candidate.total_multiplier(), candidate.payment().total()))
                .collect::<Vec<_>>(),
            vec![(1, 32000), (3, 96000)]
        );
        assert_eq!(
            select_best_scoring_candidate(&normal, &yakuman),
            BestScoringSelection::Known(ScoringCandidateRef::Yakuman(&yakuman[1]))
        );
        assert_eq!(
            yakuman[1].multiplier_of(Yakuman::Suuankou),
            Some(2),
            "the yakuman breakdown survives the selection"
        );
    }

    #[test]
    fn equal_normal_payments_are_broken_by_the_higher_han() {
        let setup = hand(&HONITSU_SOUZU);
        let normal = setup.normal_candidates(tsumo(), "6s");
        let yakuman = setup.yakuman_candidates(tsumo(), "6s");

        assert_eq!(payment_totals(&normal), vec![Some(12000), Some(12000)]);
        assert_eq!(
            normal
                .iter()
                .map(|candidate| (candidate.total_han(), candidate.fu().fu()))
                .collect::<Vec<_>>(),
            vec![(Some(6), 30), (Some(7), 20)]
        );
        assert_eq!(
            select_best_scoring_candidate(&normal, &yakuman),
            BestScoringSelection::Known(ScoringCandidateRef::Normal(&normal[1]))
        );
    }

    #[test]
    fn equal_normal_payments_and_han_are_broken_by_the_higher_fu() {
        let setup = Setup::new(
            &ANKAN_HONITSU_REST,
            &[(MeldKind::Ankan, &["W", "W", "W", "W"])],
            &[],
        );
        let normal = setup.normal_candidates(ron(), "8p");
        let yakuman = setup.yakuman_candidates(ron(), "8p");

        assert_eq!(payment_totals(&normal), vec![Some(8000), Some(8000)]);
        assert_eq!(
            normal
                .iter()
                .map(|candidate| (candidate.total_han(), candidate.fu().fu()))
                .collect::<Vec<_>>(),
            vec![(Some(4), 70), (Some(4), 80)]
        );
        assert_eq!(
            select_best_scoring_candidate(&normal, &yakuman),
            BestScoringSelection::Known(ScoringCandidateRef::Normal(&normal[1]))
        );
    }

    #[test]
    fn a_complete_tie_keeps_the_candidate_order() {
        let setup = hand(&TWO_KANCHAN_DECOMPOSITIONS);

        for (winning_tile, first_wait) in [("5p", WaitType::Kanchan), ("6p", WaitType::Ryanmen)] {
            let normal = setup.normal_candidates(ron(), winning_tile);
            let yakuman = setup.yakuman_candidates(ron(), winning_tile);

            assert_eq!(
                payment_totals(&normal),
                vec![Some(1300), Some(1300)],
                "winning tile: {winning_tile}"
            );
            assert_eq!(
                normal
                    .iter()
                    .map(|candidate| (candidate.total_han(), candidate.fu().fu()))
                    .collect::<Vec<_>>(),
                vec![(Some(1), 40), (Some(1), 40)],
                "winning tile: {winning_tile}"
            );
            assert_eq!(
                normal[0].interpretation().wait(),
                first_wait,
                "winning tile: {winning_tile}"
            );
            assert_eq!(
                select_best_scoring_candidate(&normal, &yakuman)
                    .known()
                    .map(|best| best.interpretation()),
                Some(normal[0].interpretation()),
                "winning tile: {winning_tile}"
            );
        }
    }

    #[test]
    fn an_unknown_ura_dora_without_a_yakuman_has_no_exact_best() {
        let setup = hand(&PINFU_TANYAO_HAND);
        let normal = setup.normal_candidates(riichi(ron()), "2m");
        let yakuman = setup.yakuman_candidates(riichi(ron()), "2m");

        assert_eq!(payment_totals(&normal), vec![None]);
        assert_eq!(yakuman, []);
        assert_eq!(
            select_best_scoring_candidate(&normal, &yakuman),
            BestScoringSelection::IndeterminateBonusHan
        );
    }

    #[test]
    fn a_named_yakuman_resolves_an_unknown_ura_dora() {
        let setup = hand(&TANYAO_SUUANKOU);
        let normal = setup.normal_candidates(riichi(tsumo()), "5s");
        let yakuman = setup.yakuman_candidates(riichi(tsumo()), "5s");

        assert_eq!(payment_totals(&normal), vec![None]);
        assert_eq!(
            select_best_scoring_candidate(&normal, &yakuman),
            BestScoringSelection::Known(ScoringCandidateRef::Yakuman(&yakuman[0]))
        );
    }

    #[test]
    fn a_yakuman_allows_known_normal_payment_to_win_despite_an_unknown_normal() {
        let setup = Setup::new(&TANYAO_SUUANKOU, &[], &["1m", "2m", "3p"]);
        // public selection は異なる状況の候補も受け取れる。親の数え役満を子の役満と比較する。
        let mut normal =
            setup.normal_candidates(tsumo().with_seat_wind(Some(tile_type("E"))), "5s");
        normal.extend(setup.normal_candidates(riichi(tsumo()), "5s"));
        let yakuman = setup.yakuman_candidates(tsumo(), "5s");
        assert_eq!(payment_totals(&normal), vec![Some(48000), None]);
        assert_eq!(yakuman[0].payment().total(), 32000);
        assert_eq!(
            select_best_scoring_candidate(&normal, &[]),
            BestScoringSelection::IndeterminateBonusHan
        );
        for reverse in [false, true] {
            if reverse {
                normal.reverse();
            }
            let best_index = usize::from(reverse);
            assert_eq!(
                select_best_scoring_candidate(&normal, &yakuman),
                BestScoringSelection::Known(ScoringCandidateRef::Normal(&normal[best_index]))
            );
            let mut owned = ScoringSelector::new();
            for candidate in normal.clone() {
                owned.consider_normal(candidate);
            }
            for candidate in yakuman.clone() {
                owned.consider_yakuman(candidate);
            }
            assert!(
                matches!(owned.finish(), ScoringSelection::Normal(candidate) if candidate == normal[best_index])
            );
        }
    }

    #[test]
    fn equal_yakuman_payments_choose_the_higher_multiplier() {
        let double = hand(&TANYAO_SUUANKOU);
        let triple = hand(&GREEN_TWO_DECOMPOSITIONS);
        let mut yakuman =
            double.yakuman_candidates(ron().with_seat_wind(Some(tile_type("E"))), "6p");
        yakuman.push(triple.yakuman_candidates(ron(), "6s").remove(1));
        assert_eq!(
            yakuman
                .iter()
                .map(|c| (c.payment().total(), c.total_multiplier()))
                .collect::<Vec<_>>(),
            vec![(96000, 2), (96000, 3)]
        );
        for reverse in [false, true] {
            if reverse {
                yakuman.reverse();
            }
            assert_eq!(
                select_best_scoring_candidate(&[], &yakuman),
                BestScoringSelection::Known(ScoringCandidateRef::Yakuman(
                    &yakuman[usize::from(!reverse)]
                ))
            );
        }
    }

    #[test]
    fn equal_yakuman_scores_keep_the_first_candidate() {
        let setup = hand(&KOKUSHI_HAND);
        let mut yakuman = setup.yakuman_candidates(ron(), "9m");
        yakuman.extend(setup.yakuman_candidates(ron(), "9p"));
        for reverse in [false, true] {
            if reverse {
                yakuman.reverse();
            }
            assert_eq!(
                select_best_scoring_candidate(&[], &yakuman),
                BestScoringSelection::Known(ScoringCandidateRef::Yakuman(&yakuman[0]))
            );
        }
    }

    #[test]
    fn no_candidate_at_all_is_not_indeterminate() {
        let setup = Setup::new(
            &NO_YAKU_OPEN_REST,
            &[(MeldKind::Chi, &["1m", "2m", "3m"])],
            &["3p", "4s"],
        );
        let normal = setup.normal_candidates(ron(), "5s");
        let yakuman = setup.yakuman_candidates(ron(), "5s");

        assert_eq!(normal, []);
        assert_eq!(yakuman, []);
        assert_eq!(
            select_best_scoring_candidate(&normal, &yakuman),
            BestScoringSelection::NoCandidate
        );
    }

    #[test]
    fn a_yakuman_candidate_without_a_normal_candidate_is_selected() {
        let setup = hand(&KOKUSHI_HAND);
        let normal = setup.normal_candidates(ron(), "9m");
        let yakuman = setup.yakuman_candidates(ron(), "9m");

        assert_eq!(normal, []);
        assert_eq!(
            select_best_scoring_candidate(&normal, &yakuman),
            BestScoringSelection::Known(ScoringCandidateRef::Yakuman(&yakuman[0]))
        );
    }

    #[test]
    fn a_single_known_normal_candidate_is_selected() {
        let setup = hand(&PINFU_TANYAO_HAND);
        let normal = setup.normal_candidates(ron(), "2m");
        let yakuman = setup.yakuman_candidates(ron(), "2m");

        assert_eq!(payment_totals(&normal), vec![Some(2000)]);
        assert_eq!(
            select_best_scoring_candidate(&normal, &yakuman),
            BestScoringSelection::Known(ScoringCandidateRef::Normal(&normal[0]))
        );
    }

    #[test]
    fn the_same_candidates_give_the_same_selection() {
        let setup = hand(&GREEN_TWO_DECOMPOSITIONS);
        let normal = setup.normal_candidates(ron(), "6s");
        let yakuman = setup.yakuman_candidates(ron(), "6s");

        let first = select_best_scoring_candidate(&normal, &yakuman);
        let second = select_best_scoring_candidate(&normal, &yakuman);

        assert_eq!(first, second);
        assert_eq!(
            first.known().map(ScoringCandidateRef::is_yakuman),
            Some(true)
        );
        assert_eq!(
            first.known().map(ScoringCandidateRef::interpretation),
            second.known().map(ScoringCandidateRef::interpretation)
        );
    }
}
