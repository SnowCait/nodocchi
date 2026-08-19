use crate::completed_hand::{CompletedHandAnalysis, CompletedHandDecomposition};
use crate::meld::{Meld, is_menzen};
use crate::tile::TileType;
use crate::winning_context::WinningContext;
use crate::winning_tile::{WinningTileInterpretation, interpret_winning_tile};
use crate::winning_yaku::concealed_set_count;
use crate::yaku::standard_meld_shapes;
use crate::yakuman::{Yakuman, evaluate_yakuman};

const SUUANKOU_CONCEALED_SET_COUNT: usize = 4;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WinningYakumanEvaluation<'a> {
    interpretation: WinningTileInterpretation<'a>,
    yakuman: Vec<Yakuman>,
}

impl<'a> WinningYakumanEvaluation<'a> {
    pub fn interpretation(&self) -> WinningTileInterpretation<'a> {
        self.interpretation
    }

    pub fn decomposition(&self) -> &'a CompletedHandDecomposition {
        self.interpretation.decomposition()
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

pub fn evaluate_winning_yakuman(
    analysis: &CompletedHandAnalysis,
    context: WinningContext,
    winning_tile: TileType,
) -> Vec<WinningYakumanEvaluation<'_>> {
    let evaluations = evaluate_yakuman(analysis);
    interpret_winning_tile(analysis, winning_tile)
        .into_iter()
        .map(|interpretation| {
            let mut yakuman = evaluations
                .iter()
                .find(|evaluation| evaluation.decomposition() == interpretation.decomposition())
                .map(|evaluation| evaluation.yakuman().to_vec())
                .unwrap_or_default();
            yakuman.extend(winning_tile_yakuman(
                analysis.fixed_melds(),
                context,
                &interpretation,
            ));
            yakuman.sort_unstable();
            yakuman.dedup();
            WinningYakumanEvaluation {
                interpretation,
                yakuman,
            }
        })
        .collect()
}

fn winning_tile_yakuman(
    fixed_melds: &[Meld],
    context: WinningContext,
    interpretation: &WinningTileInterpretation<'_>,
) -> Option<Yakuman> {
    let standard = interpretation.decomposition().as_standard()?;
    standard_meld_shapes(standard, fixed_melds)?;
    is_suuankou(fixed_melds, context, interpretation).then_some(Yakuman::Suuankou)
}

fn is_suuankou(
    fixed_melds: &[Meld],
    context: WinningContext,
    interpretation: &WinningTileInterpretation<'_>,
) -> bool {
    if !is_menzen(fixed_melds) {
        return false;
    }
    if concealed_set_count(interpretation, fixed_melds, context.win_method())
        != SUUANKOU_CONCEALED_SET_COUNT
    {
        return false;
    }
    context.win_method().is_tsumo() || interpretation.group().is_pair()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::completed_hand::analyze_completed_hand;
    use crate::meld::MeldKind;
    use crate::tile::TileId;
    use crate::winning_context::WinMethod;
    use crate::winning_tile::{WaitType, WinningGroup};
    use crate::winning_yaku::evaluate_winning_yaku;
    use crate::yaku::Yaku;

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

    fn ron() -> WinningContext {
        WinningContext::new(WinMethod::Ron)
    }

    fn tsumo() -> WinningContext {
        WinningContext::new(WinMethod::Tsumo)
    }

    fn yakuman_sets(
        analysis: &CompletedHandAnalysis,
        context: WinningContext,
        winning_tile: &str,
    ) -> Vec<Vec<Yakuman>> {
        evaluate_winning_yakuman(analysis, context, tile_type(winning_tile))
            .into_iter()
            .map(|evaluation| evaluation.yakuman().to_vec())
            .collect()
    }

    fn only_yakuman(
        analysis: &CompletedHandAnalysis,
        context: WinningContext,
        winning_tile: &str,
    ) -> Vec<Yakuman> {
        let sets = yakuman_sets(analysis, context, winning_tile);
        assert_eq!(sets.len(), 1, "sets: {sets:?}");
        sets.into_iter().next().unwrap()
    }

    fn waits(
        analysis: &CompletedHandAnalysis,
        context: WinningContext,
        winning_tile: &str,
    ) -> Vec<(WinningGroup, WaitType, Vec<Yakuman>)> {
        evaluate_winning_yakuman(analysis, context, tile_type(winning_tile))
            .into_iter()
            .map(|evaluation| {
                (
                    evaluation.interpretation().group(),
                    evaluation.interpretation().wait(),
                    evaluation.yakuman().to_vec(),
                )
            })
            .collect()
    }

    const KOKUSHI: [&str; 14] = [
        "1m", "1m", "9m", "1p", "9p", "1s", "9s", "E", "S", "W", "N", "P", "F", "C",
    ];
    const FOUR_CONCEALED_TRIPLETS: [&str; 14] = [
        "1m", "1m", "1m", "2m", "2m", "2m", "3p", "3p", "3p", "4s", "4s", "4s", "9p", "9p",
    ];
    const THREE_CONCEALED_TRIPLETS_REST: [&str; 11] = [
        "2m", "2m", "2m", "3p", "3p", "3p", "4s", "4s", "4s", "9p", "9p",
    ];
    const BIG_FOUR_WINDS_ALL_HONORS: [&str; 14] = [
        "E", "E", "E", "S", "S", "S", "W", "W", "W", "N", "N", "N", "P", "P",
    ];
    const GREEN_TWO_DECOMPOSITIONS: [&str; 14] = [
        "2s", "2s", "2s", "3s", "3s", "3s", "4s", "4s", "4s", "6s", "6s", "8s", "8s", "8s",
    ];

    #[test]
    fn a_single_tile_kokushi_wait_is_one_kokushi_musou() {
        let analysis = analyze(&KOKUSHI, &[]);

        assert_eq!(
            waits(&analysis, ron(), "9m"),
            vec![(
                WinningGroup::KokushiSingle {
                    tile: tile_type("9m")
                },
                WaitType::KokushiSingle,
                vec![Yakuman::KokushiMusou],
            )]
        );
    }

    #[test]
    fn a_thirteen_sided_kokushi_wait_is_one_kokushi_musou() {
        let analysis = analyze(&KOKUSHI, &[]);

        assert_eq!(
            waits(&analysis, ron(), "1m"),
            vec![(
                WinningGroup::Pair {
                    tile: tile_type("1m")
                },
                WaitType::KokushiThirteenSided,
                vec![Yakuman::KokushiMusou],
            )]
        );
    }

    #[test]
    fn four_concealed_sets_completed_by_a_self_draw_are_suuankou() {
        let analysis = analyze(&FOUR_CONCEALED_TRIPLETS, &[]);

        assert_eq!(
            only_yakuman(&analysis, tsumo(), "4s"),
            vec![Yakuman::Suuankou]
        );
    }

    #[test]
    fn four_concealed_sets_with_a_pair_completed_by_a_discard_are_suuankou() {
        let analysis = analyze(&FOUR_CONCEALED_TRIPLETS, &[]);

        assert_eq!(
            waits(&analysis, ron(), "9p"),
            vec![(
                WinningGroup::Pair {
                    tile: tile_type("9p")
                },
                WaitType::Tanki,
                vec![Yakuman::Suuankou],
            )]
        );
    }

    #[test]
    fn a_triplet_completed_by_a_discard_is_not_suuankou() {
        let analysis = analyze(&FOUR_CONCEALED_TRIPLETS, &[]);

        assert_eq!(only_yakuman(&analysis, ron(), "4s"), []);
        assert!(
            evaluate_winning_yaku(&analysis, ron(), tile_type("4s"))[0].contains(Yaku::Sanankou)
        );
    }

    #[test]
    fn a_concealed_quad_counts_as_a_concealed_set_for_suuankou() {
        let analysis = analyze(
            &THREE_CONCEALED_TRIPLETS_REST,
            &[(MeldKind::Ankan, &["1m", "1m", "1m", "1m"])],
        );

        assert_eq!(
            only_yakuman(&analysis, tsumo(), "4s"),
            vec![Yakuman::Suuankou]
        );
    }

    #[test]
    fn an_open_triplet_does_not_count_as_a_concealed_set_for_suuankou() {
        let analysis = analyze(
            &THREE_CONCEALED_TRIPLETS_REST,
            &[(MeldKind::Pon, &["1m", "1m", "1m"])],
        );

        assert_eq!(only_yakuman(&analysis, tsumo(), "4s"), []);
    }

    #[test]
    fn an_open_quad_does_not_count_as_a_concealed_set_for_suuankou() {
        for kind in [MeldKind::Daiminkan, MeldKind::Kakan] {
            let analysis = analyze(
                &THREE_CONCEALED_TRIPLETS_REST,
                &[(kind, &["1m", "1m", "1m", "1m"])],
            );

            assert_eq!(only_yakuman(&analysis, tsumo(), "4s"), [], "kind: {kind:?}");
        }
    }

    #[test]
    fn several_yakuman_are_all_kept() {
        let analysis = analyze(&BIG_FOUR_WINDS_ALL_HONORS, &[]);

        assert_eq!(
            only_yakuman(&analysis, tsumo(), "E"),
            vec![Yakuman::Suuankou, Yakuman::Tsuuiisou, Yakuman::Daisuushii]
        );
    }

    #[test]
    fn structural_yakuman_are_carried_into_every_interpretation() {
        let analysis = analyze(
            &[
                "P", "P", "P", "F", "F", "F", "C", "C", "C", "2m", "3m", "4m", "5m", "5m",
            ],
            &[],
        );

        assert_eq!(
            only_yakuman(&analysis, ron(), "2m"),
            vec![Yakuman::Daisangen]
        );
    }

    #[test]
    fn each_interpretation_keeps_its_own_yakuman() {
        let analysis = analyze(&GREEN_TWO_DECOMPOSITIONS, &[]);
        let evaluations = evaluate_winning_yakuman(&analysis, ron(), tile_type("6s"));

        assert_eq!(evaluations.len(), 2);
        assert_eq!(
            evaluations
                .iter()
                .map(|evaluation| evaluation.yakuman().to_vec())
                .collect::<Vec<_>>(),
            vec![
                vec![Yakuman::Ryuuiisou],
                vec![Yakuman::Ryuuiisou, Yakuman::Suuankou],
            ]
        );
        for evaluation in &evaluations {
            assert_eq!(
                evaluation.decomposition(),
                evaluation.interpretation().decomposition()
            );
        }
    }

    #[test]
    fn interpretations_sharing_a_yakuman_list_are_not_deduplicated() {
        let analysis = analyze(&GREEN_TWO_DECOMPOSITIONS, &[]);

        assert_eq!(
            yakuman_sets(&analysis, ron(), "8s"),
            vec![vec![Yakuman::Ryuuiisou], vec![Yakuman::Ryuuiisou]]
        );
    }

    #[test]
    fn a_malformed_fixed_meld_gets_no_yakuman() {
        let mut source = TileIdSource::new();
        let fixed = vec![source.meld(MeldKind::Ankan, &["1m", "1m", "1m", "2m"])];
        let concealed = source.tiles(&[
            "3m", "3m", "3m", "4p", "4p", "4p", "5s", "5s", "5s", "9p", "9p",
        ]);
        let analysis = analyze_completed_hand(&concealed, &fixed).unwrap();

        assert_eq!(fixed[0].shape(), None);
        assert!(is_menzen(&fixed));
        assert!(analysis.is_complete());
        assert_eq!(only_yakuman(&analysis, tsumo(), "5s"), []);
    }

    #[test]
    fn an_unknown_winning_tile_has_no_evaluation() {
        let analysis = analyze(&FOUR_CONCEALED_TRIPLETS, &[]);

        assert!(evaluate_winning_yakuman(&analysis, tsumo(), tile_type("5m")).is_empty());
    }
}
