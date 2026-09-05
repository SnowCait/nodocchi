use crate::completed_hand::{CompletedHandAnalysis, CompletedHandDecomposition};
use crate::meld::{Meld, MeldShape, is_menzen};
use crate::tile::TileType;
use crate::winning_context::{WinMethod, WinningContext};
use crate::winning_tile::{WaitType, WinningTileInterpretation, interpret_winning_tile};
use crate::yaku::{Yaku, decomposition_yaku_with_context, standard_meld_shapes};

const SANANKOU_CONCEALED_SET_COUNT: usize = 3;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WinningYakuEvaluation<'a> {
    interpretation: WinningTileInterpretation<'a>,
    yaku: Vec<Yaku>,
}

impl<'a> WinningYakuEvaluation<'a> {
    pub fn interpretation(&self) -> WinningTileInterpretation<'a> {
        self.interpretation
    }

    pub fn decomposition(&self) -> &'a CompletedHandDecomposition {
        self.interpretation.decomposition()
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

pub fn evaluate_winning_yaku(
    analysis: &CompletedHandAnalysis,
    context: WinningContext,
    winning_tile: TileType,
) -> Vec<WinningYakuEvaluation<'_>> {
    winning_yaku_evaluations(
        analysis,
        context,
        &interpret_winning_tile(analysis, winning_tile),
    )
    .collect()
}

/// 和了牌の解釈を求めてある場合の [`evaluate_winning_yaku`]。
///
/// 和了牌の解釈 ([`interpret_winning_tile`]) は和了状況にもドラにも依らないので、同じ完成手・
/// 同じ和了牌なら役判定と役満判定で1回求めれば足りる。解釈を持っている呼び出し側が同じ列挙を
/// もう一度走らせないための入口で、役の付け方は [`evaluate_winning_yaku`] と同じ。
pub(crate) fn winning_yaku_evaluations<'a>(
    analysis: &'a CompletedHandAnalysis,
    context: WinningContext,
    interpretations: &[WinningTileInterpretation<'a>],
) -> impl Iterator<Item = WinningYakuEvaluation<'a>> {
    let menzen = is_menzen(analysis.fixed_melds());
    // interpret_winning_tile は decomposition ごとに解釈を連続して返す。
    // base の役はその一群で1回だけ求め、最後の解釈には Vec の所有権を渡す。
    // 解釈が複数ある場合だけ、それより前の解釈用に base を複製する。
    interpretations
        .chunk_by(|left, right| left.decomposition() == right.decomposition())
        .flat_map(move |group| {
            let mut base = decomposition_yaku_with_context(
                group[0].decomposition(),
                analysis.fixed_melds(),
                analysis.tile_type_counts(),
                context,
                menzen,
            );
            group
                .iter()
                .enumerate()
                .map(move |(index, &interpretation)| {
                    let mut yaku = if index + 1 == group.len() {
                        std::mem::take(&mut base)
                    } else {
                        base.clone()
                    };
                    winning_tile_yaku(analysis.fixed_melds(), context, &interpretation, &mut yaku);
                    yaku.sort_unstable();
                    yaku.dedup();
                    WinningYakuEvaluation {
                        interpretation,
                        yaku,
                    }
                })
        })
}

pub fn concealed_set_count(
    interpretation: &WinningTileInterpretation<'_>,
    fixed_melds: &[Meld],
    win_method: WinMethod,
) -> usize {
    let Some(standard) = interpretation.decomposition().as_standard() else {
        return 0;
    };

    let concealed = standard
        .concealed_melds()
        .iter()
        .filter(|meld| meld.is_triplet())
        .count()
        + fixed_melds
            .iter()
            .filter(|meld| !meld.is_open())
            .filter(|meld| meld.shape().is_some_and(MeldShape::is_kan))
            .count();

    concealed.saturating_sub(usize::from(completed_as_melded_triplet(
        interpretation,
        win_method,
    )))
}

pub(crate) fn completed_as_melded_triplet(
    interpretation: &WinningTileInterpretation<'_>,
    win_method: WinMethod,
) -> bool {
    win_method.is_ron() && interpretation.group().is_triplet()
}

fn winning_tile_yaku(
    fixed_melds: &[Meld],
    context: WinningContext,
    interpretation: &WinningTileInterpretation<'_>,
    yaku: &mut Vec<Yaku>,
) {
    let Some(standard) = interpretation.decomposition().as_standard() else {
        return;
    };
    let Some(melds) = standard_meld_shapes(standard, fixed_melds) else {
        return;
    };

    if is_pinfu(
        standard.pair(),
        &melds,
        fixed_melds,
        context,
        interpretation,
    ) {
        yaku.push(Yaku::Pinfu);
    }
    if concealed_set_count(interpretation, fixed_melds, context.win_method())
        == SANANKOU_CONCEALED_SET_COUNT
    {
        yaku.push(Yaku::Sanankou);
    }
}

fn is_pinfu(
    pair: TileType,
    melds: &[MeldShape],
    fixed_melds: &[Meld],
    context: WinningContext,
    interpretation: &WinningTileInterpretation<'_>,
) -> bool {
    is_menzen(fixed_melds)
        && melds.iter().all(|meld| meld.is_sequence())
        && pair_is_confirmed_non_value(pair, context)
        && interpretation.group().is_sequence()
        && interpretation.wait() == WaitType::Ryanmen
}

fn pair_is_confirmed_non_value(pair: TileType, context: WinningContext) -> bool {
    if pair.is_dragon() {
        return false;
    }
    if !pair.is_wind() {
        return true;
    }

    let (Some(round_wind), Some(seat_wind)) = (context.round_wind(), context.seat_wind()) else {
        return false;
    };
    !pair.is_value_honor(Some(round_wind), Some(seat_wind))
}

#[cfg(test)]
mod differential;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::completed_hand::analyze_completed_hand;
    use crate::meld::MeldKind;
    use crate::tile::TileId;
    use crate::winning_context::RiichiStatus;
    use crate::winning_tile::WinningGroup;
    use crate::yaku::evaluate_yaku;

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

    fn shapes(
        analysis: &CompletedHandAnalysis,
        context: WinningContext,
        winning_tile: &str,
    ) -> Vec<(WinningGroup, WaitType, Vec<Yaku>)> {
        evaluate_winning_yaku(analysis, context, tile_type(winning_tile))
            .into_iter()
            .map(|evaluation| {
                (
                    evaluation.interpretation().group(),
                    evaluation.interpretation().wait(),
                    evaluation.yaku().to_vec(),
                )
            })
            .collect()
    }

    fn yaku_sets(
        analysis: &CompletedHandAnalysis,
        context: WinningContext,
        winning_tile: &str,
    ) -> Vec<Vec<Yaku>> {
        evaluate_winning_yaku(analysis, context, tile_type(winning_tile))
            .into_iter()
            .map(|evaluation| evaluation.yaku().to_vec())
            .collect()
    }

    fn only_yaku(
        analysis: &CompletedHandAnalysis,
        context: WinningContext,
        winning_tile: &str,
    ) -> Vec<Yaku> {
        let sets = yaku_sets(analysis, context, winning_tile);
        assert_eq!(sets.len(), 1, "sets: {sets:?}");
        sets.into_iter().next().unwrap()
    }

    const PINFU_TANYAO: [&str; 14] = [
        "2m", "3m", "4m", "3m", "4m", "5m", "4p", "5p", "6p", "6p", "7p", "8p", "5s", "5s",
    ];
    const PENCHAN_HAND: [&str; 14] = [
        "1m", "2m", "3m", "4m", "5m", "6m", "4p", "5p", "6p", "6p", "7p", "8p", "5s", "5s",
    ];
    const THREE_CONCEALED_TRIPLETS: [&str; 14] = [
        "1m", "1m", "1m", "2m", "2m", "2m", "3p", "3p", "3p", "4s", "5s", "6s", "9p", "9p",
    ];
    const FOUR_CONCEALED_TRIPLETS: [&str; 14] = [
        "1m", "1m", "1m", "2m", "2m", "2m", "3p", "3p", "3p", "4s", "4s", "4s", "9p", "9p",
    ];

    fn pinfu_hand_with_pair(pair: &str) -> [&str; 14] {
        [
            "2m", "3m", "4m", "3m", "4m", "5m", "4p", "5p", "6p", "6p", "7p", "8p", pair, pair,
        ]
    }

    #[test]
    fn menzen_four_sequences_with_a_two_sided_wait_is_pinfu() {
        let analysis = analyze(&PINFU_TANYAO, &[]);

        assert_eq!(
            only_yaku(&analysis, ron(), "2m"),
            vec![Yaku::Pinfu, Yaku::Tanyao]
        );
    }

    #[test]
    fn an_edge_wait_is_not_pinfu() {
        let analysis = analyze(&PENCHAN_HAND, &[]);

        assert_eq!(
            shapes(&analysis, ron(), "3m"),
            vec![(
                WinningGroup::Sequence {
                    start: tile_type("1m")
                },
                WaitType::Penchan,
                Vec::new(),
            )]
        );
    }

    #[test]
    fn a_middle_wait_is_not_pinfu() {
        let analysis = analyze(&PENCHAN_HAND, &[]);

        assert_eq!(
            shapes(&analysis, ron(), "2m"),
            vec![(
                WinningGroup::Sequence {
                    start: tile_type("1m")
                },
                WaitType::Kanchan,
                Vec::new(),
            )]
        );
    }

    #[test]
    fn a_single_wait_is_not_pinfu() {
        let analysis = analyze(&PINFU_TANYAO, &[]);

        assert_eq!(
            shapes(&analysis, ron(), "5s"),
            vec![(
                WinningGroup::Pair {
                    tile: tile_type("5s")
                },
                WaitType::Tanki,
                vec![Yaku::Tanyao],
            )]
        );
    }

    #[test]
    fn a_dragon_pair_is_not_pinfu() {
        let analysis = analyze(&pinfu_hand_with_pair("P"), &[]);

        assert_eq!(only_yaku(&analysis, ron(), "2m"), Vec::new());
    }

    #[test]
    fn a_round_wind_pair_is_not_pinfu() {
        let analysis = analyze(&pinfu_hand_with_pair("E"), &[]);
        let context = ron()
            .with_round_wind(Some(tile_type("E")))
            .with_seat_wind(Some(tile_type("S")));

        assert_eq!(only_yaku(&analysis, context, "2m"), Vec::new());
    }

    #[test]
    fn a_seat_wind_pair_is_not_pinfu() {
        let analysis = analyze(&pinfu_hand_with_pair("E"), &[]);
        let context = ron()
            .with_round_wind(Some(tile_type("S")))
            .with_seat_wind(Some(tile_type("E")));

        assert_eq!(only_yaku(&analysis, context, "2m"), Vec::new());
    }

    #[test]
    fn a_guest_wind_pair_is_pinfu_when_both_winds_are_known() {
        let analysis = analyze(&pinfu_hand_with_pair("W"), &[]);
        let context = ron()
            .with_round_wind(Some(tile_type("E")))
            .with_seat_wind(Some(tile_type("S")));

        assert_eq!(only_yaku(&analysis, context, "2m"), vec![Yaku::Pinfu]);
    }

    #[test]
    fn an_unknown_wind_is_not_assumed_to_be_a_guest_wind() {
        let analysis = analyze(&pinfu_hand_with_pair("W"), &[]);

        for context in [
            ron().with_round_wind(Some(tile_type("E"))),
            ron().with_seat_wind(Some(tile_type("S"))),
            ron(),
        ] {
            assert_eq!(only_yaku(&analysis, context, "2m"), Vec::new());
        }
    }

    #[test]
    fn an_open_hand_is_not_pinfu() {
        let analysis = analyze(
            &[
                "2m", "3m", "4m", "3m", "4m", "5m", "6p", "7p", "8p", "5s", "5s",
            ],
            &[(MeldKind::Chi, &["4p", "5p", "6p"])],
        );

        assert!(!is_menzen(analysis.fixed_melds()));
        assert_eq!(only_yaku(&analysis, ron(), "2m"), vec![Yaku::Tanyao]);
    }

    #[test]
    fn an_ankan_keeps_the_hand_closed_but_breaks_the_four_sequences() {
        let analysis = analyze(
            &[
                "2m", "3m", "4m", "3m", "4m", "5m", "4p", "5p", "6p", "5p", "5p",
            ],
            &[(MeldKind::Ankan, &["2s", "2s", "2s", "2s"])],
        );

        assert!(is_menzen(analysis.fixed_melds()));
        assert_eq!(only_yaku(&analysis, ron(), "2m"), vec![Yaku::Tanyao]);
    }

    #[test]
    fn one_decomposition_can_be_pinfu_only_through_the_two_sided_interpretation() {
        let analysis = analyze(
            &[
                "1m", "2m", "3m", "3m", "4m", "5m", "4p", "5p", "6p", "5p", "5p", "7s", "8s", "9s",
            ],
            &[],
        );

        let evaluations = evaluate_winning_yaku(&analysis, ron(), tile_type("3m"));

        assert_eq!(
            shapes(&analysis, ron(), "3m"),
            vec![
                (
                    WinningGroup::Sequence {
                        start: tile_type("1m")
                    },
                    WaitType::Penchan,
                    Vec::new(),
                ),
                (
                    WinningGroup::Sequence {
                        start: tile_type("3m")
                    },
                    WaitType::Ryanmen,
                    vec![Yaku::Pinfu],
                ),
            ]
        );
        assert_eq!(
            evaluations[0].decomposition(),
            evaluations[1].decomposition()
        );
    }

    #[test]
    fn a_triplet_completed_by_tsumo_is_a_concealed_set() {
        let analysis = analyze(&THREE_CONCEALED_TRIPLETS, &[]);

        assert_eq!(
            shapes(&analysis, tsumo(), "3p"),
            vec![(
                WinningGroup::Triplet {
                    tile: tile_type("3p")
                },
                WaitType::Shanpon,
                vec![Yaku::Sanankou, Yaku::MenzenTsumo],
            )]
        );
    }

    #[test]
    fn a_triplet_completed_by_ron_is_melded() {
        let analysis = analyze(&THREE_CONCEALED_TRIPLETS, &[]);

        assert_eq!(only_yaku(&analysis, ron(), "3p"), Vec::new());
    }

    #[test]
    fn ron_completing_a_sequence_keeps_every_concealed_triplet() {
        let analysis = analyze(&THREE_CONCEALED_TRIPLETS, &[]);

        assert_eq!(only_yaku(&analysis, ron(), "4s"), vec![Yaku::Sanankou]);
    }

    #[test]
    fn ron_completing_the_pair_keeps_every_concealed_triplet() {
        let analysis = analyze(&THREE_CONCEALED_TRIPLETS, &[]);

        assert_eq!(only_yaku(&analysis, ron(), "9p"), vec![Yaku::Sanankou]);
    }

    #[test]
    fn an_ankan_counts_as_a_concealed_set() {
        let analysis = analyze(
            &[
                "2m", "2m", "2m", "3p", "3p", "3p", "4s", "5s", "6s", "9p", "9p",
            ],
            &[(MeldKind::Ankan, &["1m", "1m", "1m", "1m"])],
        );

        assert_eq!(only_yaku(&analysis, ron(), "4s"), vec![Yaku::Sanankou]);
    }

    #[test]
    fn an_open_meld_is_not_a_concealed_set() {
        for kind in [MeldKind::Pon, MeldKind::Daiminkan, MeldKind::Kakan] {
            let tiles: &[&str] = match kind {
                MeldKind::Pon => &["1m", "1m", "1m"],
                _ => &["1m", "1m", "1m", "1m"],
            };
            let analysis = analyze(
                &[
                    "2m", "2m", "2m", "3p", "3p", "3p", "4s", "5s", "6s", "9p", "9p",
                ],
                &[(kind, tiles)],
            );

            assert_eq!(
                only_yaku(&analysis, ron(), "4s"),
                Vec::new(),
                "kind: {kind:?}"
            );
        }
    }

    #[test]
    fn sanankou_is_not_limited_to_closed_hands() {
        let analysis = analyze(
            &[
                "1m", "1m", "1m", "2m", "2m", "2m", "3p", "3p", "3p", "9p", "9p",
            ],
            &[(MeldKind::Chi, &["4s", "5s", "6s"])],
        );

        assert!(!is_menzen(analysis.fixed_melds()));
        assert_eq!(only_yaku(&analysis, tsumo(), "3p"), vec![Yaku::Sanankou]);
    }

    #[test]
    fn four_concealed_triplets_by_tsumo_are_left_for_suuankou() {
        let analysis = analyze(&FOUR_CONCEALED_TRIPLETS, &[]);

        assert_eq!(
            only_yaku(&analysis, tsumo(), "4s"),
            vec![Yaku::Toitoi, Yaku::MenzenTsumo]
        );
    }

    #[test]
    fn four_concealed_triplets_with_a_pair_ron_are_left_for_suuankou() {
        let analysis = analyze(&FOUR_CONCEALED_TRIPLETS, &[]);

        assert_eq!(only_yaku(&analysis, ron(), "9p"), vec![Yaku::Toitoi]);
    }

    #[test]
    fn four_triplets_with_a_triplet_ron_are_sanankou() {
        let analysis = analyze(&FOUR_CONCEALED_TRIPLETS, &[]);

        assert_eq!(
            only_yaku(&analysis, ron(), "4s"),
            vec![Yaku::Toitoi, Yaku::Sanankou]
        );
    }

    #[test]
    fn one_decomposition_can_lose_sanankou_through_the_triplet_interpretation() {
        let analysis = analyze(
            &[
                "2m", "3m", "4m", "3m", "3m", "3m", "5p", "5p", "5p", "7s", "7s", "7s", "9p", "9p",
            ],
            &[],
        );

        let evaluations = evaluate_winning_yaku(&analysis, ron(), tile_type("3m"));

        assert_eq!(
            shapes(&analysis, ron(), "3m"),
            vec![
                (
                    WinningGroup::Sequence {
                        start: tile_type("2m")
                    },
                    WaitType::Kanchan,
                    vec![Yaku::Sanankou],
                ),
                (
                    WinningGroup::Triplet {
                        tile: tile_type("3m")
                    },
                    WaitType::Shanpon,
                    Vec::new(),
                ),
            ]
        );
        assert_eq!(
            evaluations[0].decomposition(),
            evaluations[1].decomposition()
        );
    }

    #[test]
    fn structural_and_contextual_yaku_are_kept() {
        let analysis = analyze(&PINFU_TANYAO, &[]);
        let context = ron().with_riichi(RiichiStatus::Riichi);

        let evaluations = evaluate_winning_yaku(&analysis, context, tile_type("2m"));
        let evaluation = evaluations.first().unwrap();

        assert_eq!(evaluation.yaku(), [Yaku::Pinfu, Yaku::Tanyao, Yaku::Riichi]);
        assert!(evaluation.contains(Yaku::Pinfu));
        assert!(!evaluation.is_empty());
        for yaku in evaluate_yaku(&analysis, context)[0].yaku() {
            assert!(evaluation.contains(*yaku), "yaku: {yaku:?}");
        }
    }

    #[test]
    fn every_decomposition_keeps_its_own_yaku() {
        let analysis = analyze(
            &[
                "2m", "2m", "2m", "3m", "3m", "3m", "4m", "4m", "4m", "5p", "6p", "7p", "9s", "9s",
            ],
            &[],
        );

        assert_eq!(analysis.decompositions().len(), 2);
        assert_eq!(
            shapes(&analysis, ron(), "4m"),
            vec![
                (
                    WinningGroup::Sequence {
                        start: tile_type("2m")
                    },
                    WaitType::Ryanmen,
                    vec![Yaku::Pinfu, Yaku::Iipeikou],
                ),
                (
                    WinningGroup::Triplet {
                        tile: tile_type("4m")
                    },
                    WaitType::Shanpon,
                    Vec::new(),
                ),
            ]
        );
        assert_eq!(
            shapes(&analysis, tsumo(), "4m"),
            vec![
                (
                    WinningGroup::Sequence {
                        start: tile_type("2m")
                    },
                    WaitType::Ryanmen,
                    vec![Yaku::Pinfu, Yaku::Iipeikou, Yaku::MenzenTsumo],
                ),
                (
                    WinningGroup::Triplet {
                        tile: tile_type("4m")
                    },
                    WaitType::Shanpon,
                    vec![Yaku::Sanankou, Yaku::MenzenTsumo],
                ),
            ]
        );
    }

    #[test]
    fn a_malformed_fixed_meld_gets_no_winning_tile_yaku() {
        let analysis = analyze(
            &[
                "2m", "2m", "2m", "3p", "3p", "3p", "7s", "7s", "7s", "9p", "9p",
            ],
            &[(MeldKind::Pon, &["1m", "2m", "3m"])],
        );

        assert_eq!(analysis.fixed_melds()[0].shape(), None);
        assert!(evaluate_yaku(&analysis, ron())[0].is_empty());
        assert_eq!(only_yaku(&analysis, ron(), "9p"), Vec::new());
    }

    #[test]
    fn a_hand_without_an_interpretation_has_no_evaluation() {
        let analysis = analyze(
            &[
                "1m", "2m", "3m", "4m", "5m", "6m", "7m", "8m", "9m", "1p", "2p", "3p", "5s",
            ],
            &[],
        );

        assert!(!analysis.is_complete());
        assert!(evaluate_winning_yaku(&analysis, ron(), tile_type("5s")).is_empty());
    }

    #[test]
    fn a_winning_tile_outside_the_concealed_hand_has_no_evaluation() {
        let analysis = analyze(
            &[
                "1m", "2m", "3m", "4p", "5p", "6p", "7p", "8p", "9p", "5s", "5s",
            ],
            &[(MeldKind::Pon, &["E", "E", "E"])],
        );

        assert!(analysis.is_complete());
        assert!(evaluate_winning_yaku(&analysis, ron(), tile_type("E")).is_empty());
    }

    #[test]
    fn evaluations_follow_the_interpretations_and_stay_deterministic() {
        let analysis = analyze(
            &[
                "1m", "1m", "1m", "2m", "2m", "2m", "3m", "3m", "3m", "4m", "4m", "4m", "5m", "5m",
            ],
            &[],
        );

        let interpretations = interpret_winning_tile(&analysis, tile_type("3m"));
        let evaluations = evaluate_winning_yaku(&analysis, ron(), tile_type("3m"));

        assert_eq!(evaluations.len(), interpretations.len());
        for (evaluation, interpretation) in evaluations.iter().zip(&interpretations) {
            assert_eq!(evaluation.interpretation(), *interpretation);
            assert_eq!(evaluation.decomposition(), interpretation.decomposition());
            let mut sorted = evaluation.yaku().to_vec();
            sorted.sort_unstable();
            sorted.dedup();
            assert_eq!(evaluation.yaku(), sorted);
        }
        assert_eq!(
            evaluate_winning_yaku(&analysis, ron(), tile_type("3m")),
            evaluations
        );
    }

    #[test]
    fn chiitoitsu_and_kokushi_get_no_winning_tile_yaku() {
        let chiitoitsu = analyze(
            &[
                "1m", "1m", "3m", "3m", "5m", "5m", "7m", "7m", "9m", "9m", "1p", "1p", "E", "E",
            ],
            &[],
        );
        let kokushi = analyze(
            &[
                "1m", "9m", "1p", "9p", "1s", "9s", "E", "S", "W", "N", "P", "F", "C", "9s",
            ],
            &[],
        );

        for yaku in only_yaku(&chiitoitsu, ron(), "E") {
            assert!(!matches!(yaku, Yaku::Pinfu | Yaku::Sanankou));
        }
        assert_eq!(only_yaku(&kokushi, ron(), "1m"), Vec::new());
    }

    #[test]
    fn concealed_set_count_is_neutral_about_the_yaku() {
        let analysis = analyze(&FOUR_CONCEALED_TRIPLETS, &[]);
        let interpretations = interpret_winning_tile(&analysis, tile_type("4s"));
        let interpretation = interpretations.first().unwrap();

        assert_eq!(
            concealed_set_count(interpretation, analysis.fixed_melds(), WinMethod::Tsumo),
            4
        );
        assert_eq!(
            concealed_set_count(interpretation, analysis.fixed_melds(), WinMethod::Ron),
            3
        );
    }
}
