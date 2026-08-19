use crate::completed_hand::CompletedHandAnalysis;
use crate::tile::{TileId, count_indicated_dora};
use crate::winning_context::WinningContext;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum UraDoraHan {
    Known(u8),
    Ineligible,
    Unknown,
}

impl UraDoraHan {
    pub fn han(self) -> Option<u8> {
        match self {
            Self::Known(han) => Some(han),
            Self::Ineligible => Some(0),
            Self::Unknown => None,
        }
    }

    pub fn is_unknown(self) -> bool {
        matches!(self, Self::Unknown)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct BonusHanBreakdown {
    indicated_dora: u8,
    aka_dora: u8,
    ura_dora: UraDoraHan,
}

impl BonusHanBreakdown {
    pub fn indicated_dora(self) -> u8 {
        self.indicated_dora
    }

    pub fn aka_dora(self) -> u8 {
        self.aka_dora
    }

    pub fn ura_dora(self) -> UraDoraHan {
        self.ura_dora
    }

    pub fn non_ura_bonus_han(self) -> u8 {
        self.indicated_dora + self.aka_dora
    }

    pub fn bonus_han_total(self) -> Option<u8> {
        self.ura_dora
            .han()
            .map(|ura_dora| self.non_ura_bonus_han() + ura_dora)
    }
}

pub fn evaluate_bonus_han(
    analysis: &CompletedHandAnalysis,
    context: WinningContext,
    dora_indicators: &[TileId],
    ura_dora_indicators: Option<&[TileId]>,
) -> BonusHanBreakdown {
    BonusHanBreakdown {
        indicated_dora: indicated_dora_han(analysis, dora_indicators),
        aka_dora: hand_tiles(analysis).filter(|tile| tile.is_red()).count() as u8,
        ura_dora: ura_dora_han(analysis, context, ura_dora_indicators),
    }
}

fn ura_dora_han(
    analysis: &CompletedHandAnalysis,
    context: WinningContext,
    ura_dora_indicators: Option<&[TileId]>,
) -> UraDoraHan {
    match context.riichi().is_declared() {
        Some(true) => match ura_dora_indicators {
            Some(indicators) => UraDoraHan::Known(indicated_dora_han(analysis, indicators)),
            None => UraDoraHan::Unknown,
        },
        Some(false) => UraDoraHan::Ineligible,
        None => UraDoraHan::Unknown,
    }
}

fn indicated_dora_han(analysis: &CompletedHandAnalysis, indicators: &[TileId]) -> u8 {
    hand_tiles(analysis)
        .map(|tile| count_indicated_dora(tile.tile_type(), indicators))
        .sum()
}

fn hand_tiles(analysis: &CompletedHandAnalysis) -> impl Iterator<Item = TileId> + '_ {
    analysis
        .concealed_tiles()
        .iter()
        .chain(analysis.fixed_melds().iter().flat_map(|meld| meld.tiles()))
        .copied()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::completed_hand::analyze_completed_hand;
    use crate::han::evaluate_winning_yaku_han;
    use crate::meld::{Meld, MeldKind};
    use crate::tile::TileType;
    use crate::winning_context::{RiichiStatus, WinMethod};
    use crate::winning_yaku::evaluate_winning_yaku;

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
        TileType::from_mjai_type_str(s).unwrap()
    }

    struct Hand {
        analysis: CompletedHandAnalysis,
        source: TileIdSource,
    }

    impl Hand {
        fn new(concealed: &[&str], fixed: &[(MeldKind, &[&str])]) -> Self {
            let mut source = TileIdSource::new();
            let fixed_melds: Vec<Meld> = fixed
                .iter()
                .map(|(kind, tiles)| source.meld(*kind, tiles))
                .collect();
            let tiles = source.tiles(concealed);
            Self {
                analysis: analyze_completed_hand(&tiles, &fixed_melds).unwrap(),
                source,
            }
        }

        fn indicators(&mut self, strings: &[&str]) -> Vec<TileId> {
            self.source.tiles(strings)
        }

        fn bonus_han(
            &mut self,
            context: WinningContext,
            dora: &[&str],
            ura: Option<&[&str]>,
        ) -> BonusHanBreakdown {
            let dora_indicators = self.indicators(dora);
            let ura_indicators = ura.map(|ura| self.indicators(ura));
            evaluate_bonus_han(
                &self.analysis,
                context,
                &dora_indicators,
                ura_indicators.as_deref(),
            )
        }

        fn dora_han(&mut self, dora: &[&str]) -> BonusHanBreakdown {
            self.bonus_han(ron(), dora, None)
        }
    }

    fn ron() -> WinningContext {
        WinningContext::new(WinMethod::Ron)
    }

    fn pinfu_hand() -> Hand {
        Hand::new(
            &[
                "2m", "3m", "4m", "5m", "6m", "7m", "3p", "4p", "5p", "6s", "7s", "8s", "9s", "9s",
            ],
            &[],
        )
    }

    #[test]
    fn indicator_points_to_the_next_tile() {
        assert_eq!(pinfu_hand().dora_han(&["4m"]).indicated_dora(), 1);
        assert_eq!(pinfu_hand().dora_han(&["3p"]).indicated_dora(), 1);
    }

    #[test]
    fn indicator_wraps_within_the_suit() {
        let mut hand = Hand::new(
            &[
                "1m", "1m", "1m", "2m", "3m", "4m", "5m", "6m", "7m", "8m", "9m", "5p", "5p", "5p",
            ],
            &[],
        );

        assert_eq!(hand.dora_han(&["9m"]).indicated_dora(), 3);
    }

    #[test]
    fn indicator_wraps_within_winds_and_dragons() {
        let mut hand = Hand::new(
            &[
                "E", "E", "E", "P", "P", "P", "1m", "2m", "3m", "5s", "6s", "7s", "9s", "9s",
            ],
            &[],
        );

        assert_eq!(hand.dora_han(&["N"]).indicated_dora(), 3);
        assert_eq!(hand.dora_han(&["C"]).indicated_dora(), 3);
        assert_eq!(hand.dora_han(&["N", "C"]).indicated_dora(), 6);
    }

    #[test]
    fn indicated_dora_counts_every_copy() {
        let mut hand = Hand::new(
            &[
                "5m", "5m", "5m", "1m", "2m", "3m", "4p", "5p", "6p", "7s", "8s", "9s", "9p", "9p",
            ],
            &[],
        );

        assert_eq!(hand.dora_han(&["4m"]).indicated_dora(), 3);
    }

    #[test]
    fn repeated_indicator_multiplies_the_same_dora() {
        assert_eq!(pinfu_hand().dora_han(&["4m", "4m"]).indicated_dora(), 2);
    }

    #[test]
    fn unrelated_indicator_gives_no_dora() {
        assert_eq!(pinfu_hand().dora_han(&["1s"]).indicated_dora(), 0);
        assert_eq!(pinfu_hand().dora_han(&[]).indicated_dora(), 0);
    }

    #[test]
    fn kan_dora_indicators_are_counted_like_the_first_indicator() {
        let mut hand = Hand::new(
            &[
                "2m", "3m", "4m", "5m", "6m", "7m", "3p", "4p", "5p", "6s", "7s", "8s", "9s", "9s",
            ],
            &[],
        );

        assert_eq!(hand.dora_han(&["4m", "2p", "5s", "7s"]).indicated_dora(), 4);
    }

    #[test]
    fn aka_dora_counts_concealed_red_five() {
        let mut hand = Hand::new(
            &[
                "2m", "3m", "4m", "5mr", "6m", "7m", "3p", "4p", "5p", "6s", "7s", "8s", "9s", "9s",
            ],
            &[],
        );

        assert_eq!(hand.dora_han(&[]).aka_dora(), 1);
    }

    #[test]
    fn aka_dora_counts_red_five_in_a_fixed_meld() {
        let mut hand = Hand::new(
            &[
                "2m", "3m", "4m", "5m", "6m", "7m", "6s", "7s", "8s", "9s", "9s",
            ],
            &[(MeldKind::Chi, &["3p", "4p", "5pr"])],
        );

        assert_eq!(hand.dora_han(&[]).aka_dora(), 1);
    }

    #[test]
    fn hand_without_red_five_has_no_aka_dora() {
        assert_eq!(pinfu_hand().dora_han(&["4m"]).aka_dora(), 0);
    }

    #[test]
    fn indicated_dora_and_aka_dora_stack_on_the_same_tile() {
        let mut hand = Hand::new(
            &[
                "2m", "3m", "4m", "5mr", "6m", "7m", "3p", "4p", "5p", "6s", "7s", "8s", "9s", "9s",
            ],
            &[],
        );

        let breakdown = hand.dora_han(&["4m"]);

        assert_eq!(breakdown.indicated_dora(), 1);
        assert_eq!(breakdown.aka_dora(), 1);
        assert_eq!(breakdown.non_ura_bonus_han(), 2);
    }

    #[test]
    fn pon_counts_all_three_physical_tiles() {
        let mut hand = Hand::new(
            &[
                "2m", "3m", "4m", "5m", "6m", "7m", "3p", "4p", "5p", "9s", "9s",
            ],
            &[(MeldKind::Pon, &["1s", "1s", "1s"])],
        );

        assert_eq!(hand.dora_han(&["9s"]).indicated_dora(), 3);
    }

    #[test]
    fn kan_counts_all_four_physical_tiles() {
        for kind in [MeldKind::Daiminkan, MeldKind::Ankan, MeldKind::Kakan] {
            let mut hand = Hand::new(
                &[
                    "2m", "3m", "4m", "5m", "6m", "7m", "3p", "4p", "5p", "9s", "9s",
                ],
                &[(kind, &["1s", "1s", "1s", "1s"])],
            );

            assert_eq!(hand.dora_han(&["9s"]).indicated_dora(), 4, "{kind:?}");
        }
    }

    #[test]
    fn chi_dora_is_not_dropped() {
        let mut hand = Hand::new(
            &[
                "2m", "3m", "4m", "5m", "6m", "7m", "6s", "7s", "8s", "9s", "9s",
            ],
            &[(MeldKind::Chi, &["3p", "4p", "5p"])],
        );

        assert_eq!(hand.dora_han(&["2p"]).indicated_dora(), 1);
    }

    #[test]
    fn winning_tile_is_not_counted_twice() {
        let mut hand = Hand::new(
            &[
                "2m", "3m", "4m", "5m", "6m", "7m", "3p", "4p", "5p", "6s", "7s", "8s", "9s", "9s",
            ],
            &[],
        );

        assert_eq!(hand.dora_han(&["4m"]).indicated_dora(), 1);
    }

    #[test]
    fn riichi_counts_ura_dora() {
        let mut hand = pinfu_hand();
        let context = ron().with_riichi(RiichiStatus::Riichi);

        let breakdown = hand.bonus_han(context, &["4m"], Some(&["8s"]));

        assert_eq!(breakdown.indicated_dora(), 1);
        assert_eq!(breakdown.ura_dora(), UraDoraHan::Known(2));
        assert_eq!(breakdown.non_ura_bonus_han(), 1);
        assert_eq!(breakdown.bonus_han_total(), Some(3));
    }

    #[test]
    fn double_riichi_counts_ura_dora() {
        let mut hand = pinfu_hand();
        let context = ron().with_riichi(RiichiStatus::DoubleRiichi);

        assert_eq!(
            hand.bonus_han(context, &[], Some(&["8s"])).ura_dora(),
            UraDoraHan::Known(2)
        );
    }

    #[test]
    fn riichi_with_known_empty_ura_indicators_has_zero_ura_dora() {
        let mut hand = pinfu_hand();
        let context = ron().with_riichi(RiichiStatus::Riichi);

        let breakdown = hand.bonus_han(context, &["4m"], Some(&[]));

        assert_eq!(breakdown.ura_dora(), UraDoraHan::Known(0));
        assert_eq!(breakdown.ura_dora().han(), Some(0));
        assert_eq!(breakdown.bonus_han_total(), Some(1));
    }

    #[test]
    fn riichi_with_unobserved_ura_indicators_leaves_ura_dora_unknown() {
        let mut hand = pinfu_hand();
        let context = ron().with_riichi(RiichiStatus::Riichi);

        let breakdown = hand.bonus_han(context, &["4m"], None);

        assert_eq!(breakdown.ura_dora(), UraDoraHan::Unknown);
        assert_eq!(breakdown.non_ura_bonus_han(), 1);
        assert_eq!(breakdown.bonus_han_total(), None);
    }

    #[test]
    fn double_riichi_with_unobserved_ura_indicators_leaves_ura_dora_unknown() {
        let mut hand = pinfu_hand();
        let context = ron().with_riichi(RiichiStatus::DoubleRiichi);

        assert_eq!(
            hand.bonus_han(context, &["4m"], None).ura_dora(),
            UraDoraHan::Unknown
        );
    }

    #[test]
    fn known_empty_ura_indicators_are_not_unobserved_ura_indicators() {
        let mut known_empty = pinfu_hand();
        let mut unobserved = pinfu_hand();
        let context = ron().with_riichi(RiichiStatus::Riichi);

        let known_empty = known_empty.bonus_han(context, &["4m"], Some(&[]));
        let unobserved = unobserved.bonus_han(context, &["4m"], None);

        assert_eq!(known_empty.ura_dora(), UraDoraHan::Known(0));
        assert_eq!(unobserved.ura_dora(), UraDoraHan::Unknown);
        assert_eq!(known_empty.bonus_han_total(), Some(1));
        assert_eq!(unobserved.bonus_han_total(), None);
        assert_eq!(
            known_empty.non_ura_bonus_han(),
            unobserved.non_ura_bonus_han()
        );
    }

    #[test]
    fn not_declared_has_no_ura_dora() {
        let mut hand = Hand::new(
            &[
                "2m", "3m", "4m", "5mr", "6m", "7m", "3p", "4p", "5p", "6s", "7s", "8s", "9s", "9s",
            ],
            &[],
        );
        let context = ron().with_riichi(RiichiStatus::NotDeclared);

        let breakdown = hand.bonus_han(context, &["4m"], Some(&["8s"]));

        assert_eq!(breakdown.ura_dora(), UraDoraHan::Ineligible);
        assert_eq!(breakdown.ura_dora().han(), Some(0));
        assert_eq!(breakdown.indicated_dora(), 1);
        assert_eq!(breakdown.aka_dora(), 1);
        assert_eq!(breakdown.bonus_han_total(), Some(2));
    }

    #[test]
    fn not_declared_with_unobserved_ura_indicators_has_no_ura_dora() {
        let mut hand = pinfu_hand();
        let context = ron().with_riichi(RiichiStatus::NotDeclared);

        let breakdown = hand.bonus_han(context, &["4m"], None);

        assert_eq!(breakdown.ura_dora(), UraDoraHan::Ineligible);
        assert_eq!(breakdown.bonus_han_total(), Some(1));
    }

    #[test]
    fn unknown_riichi_leaves_ura_dora_unknown() {
        let mut hand = pinfu_hand();

        let breakdown = hand.bonus_han(ron(), &["4m"], None);

        assert_eq!(breakdown.ura_dora(), UraDoraHan::Unknown);
        assert!(breakdown.ura_dora().is_unknown());
        assert_eq!(breakdown.ura_dora().han(), None);
        assert_eq!(breakdown.bonus_han_total(), None);
    }

    #[test]
    fn unknown_riichi_does_not_infer_riichi_from_ura_indicators() {
        let mut hand = pinfu_hand();

        let breakdown = hand.bonus_han(ron(), &["4m"], Some(&["8s"]));

        assert_eq!(breakdown.ura_dora(), UraDoraHan::Unknown);
        assert_eq!(breakdown.bonus_han_total(), None);
    }

    #[test]
    fn unknown_riichi_is_not_not_declared() {
        let mut unknown = pinfu_hand();
        let mut not_declared = pinfu_hand();
        let context = ron().with_riichi(RiichiStatus::NotDeclared);

        let unknown = unknown.bonus_han(ron(), &["4m"], Some(&["8s"]));
        let not_declared = not_declared.bonus_han(context, &["4m"], Some(&["8s"]));

        assert_ne!(unknown.ura_dora(), not_declared.ura_dora());
        assert_ne!(unknown.bonus_han_total(), not_declared.bonus_han_total());
        assert_eq!(
            unknown.non_ura_bonus_han(),
            not_declared.non_ura_bonus_han()
        );
    }

    #[test]
    fn unknown_riichi_keeps_non_ura_bonus_han() {
        let mut hand = Hand::new(
            &[
                "2m", "3m", "4m", "5mr", "6m", "7m", "3p", "4p", "5p", "6s", "7s", "8s", "9s", "9s",
            ],
            &[],
        );

        let breakdown = hand.bonus_han(ron(), &["4m"], None);

        assert_eq!(breakdown.non_ura_bonus_han(), 2);
        assert_eq!(breakdown.bonus_han_total(), None);
    }

    #[test]
    fn non_ura_bonus_han_excludes_known_ura_dora() {
        let mut hand = Hand::new(
            &[
                "2m", "3m", "4m", "5mr", "6m", "7m", "3p", "4p", "5p", "6s", "7s", "8s", "9s", "9s",
            ],
            &[],
        );
        let context = ron().with_riichi(RiichiStatus::Riichi);

        let breakdown = hand.bonus_han(context, &["4m"], Some(&["8s"]));

        assert_eq!(breakdown.indicated_dora(), 1);
        assert_eq!(breakdown.aka_dora(), 1);
        assert_eq!(breakdown.ura_dora(), UraDoraHan::Known(2));
        assert_eq!(breakdown.non_ura_bonus_han(), 2);
        assert_eq!(breakdown.bonus_han_total(), Some(4));
    }

    #[test]
    fn bonus_han_does_not_add_yaku() {
        let mut hand = Hand::new(
            &[
                "2m", "3m", "4m", "6m", "7m", "8m", "3p", "4p", "5pr", "9s", "9s",
            ],
            &[(MeldKind::Chi, &["6s", "7s", "8s"])],
        );

        let breakdown = hand.dora_han(&["1m", "1m", "1m"]);

        assert_eq!(breakdown.indicated_dora(), 3);
        assert_eq!(breakdown.aka_dora(), 1);
        assert_eq!(breakdown.non_ura_bonus_han(), 4);

        let yaku: Vec<usize> = evaluate_winning_yaku(&hand.analysis, ron(), tile_type("9s"))
            .iter()
            .map(|evaluation| evaluation.yaku().len())
            .collect();
        assert_eq!(yaku, vec![0]);

        let han: Vec<u8> = evaluate_winning_yaku_han(&hand.analysis, ron(), tile_type("9s"))
            .iter()
            .map(|evaluation| evaluation.yaku_han_total())
            .collect();
        assert_eq!(han, vec![0]);
    }

    #[test]
    fn bonus_han_is_one_hand_wide_fact_over_decompositions() {
        let mut hand = Hand::new(
            &[
                "1m", "1m", "2m", "2m", "3m", "3m", "4m", "4m", "5m", "5m", "6m", "6m", "7m", "7m",
            ],
            &[],
        );

        assert!(hand.analysis.decompositions().len() > 1);
        assert_eq!(hand.dora_han(&["4m"]).indicated_dora(), 2);
    }

    #[test]
    fn same_input_gives_the_same_breakdown() {
        let mut hand = Hand::new(
            &[
                "2m", "3m", "4m", "5mr", "6m", "7m", "3p", "4p", "5p", "6s", "7s", "8s", "9s", "9s",
            ],
            &[],
        );
        let context = ron().with_riichi(RiichiStatus::Riichi);
        let dora_indicators = hand.indicators(&["4m"]);
        let ura_indicators = hand.indicators(&["8s"]);

        let first = evaluate_bonus_han(
            &hand.analysis,
            context,
            &dora_indicators,
            Some(&ura_indicators),
        );
        let second = evaluate_bonus_han(
            &hand.analysis,
            context,
            &dora_indicators,
            Some(&ura_indicators),
        );

        assert_eq!(first, second);
        assert_eq!(first.bonus_han_total(), second.bonus_han_total());
    }
}
