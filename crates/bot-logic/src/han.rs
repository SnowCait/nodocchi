use crate::completed_hand::{CompletedHandAnalysis, CompletedHandDecomposition};
use crate::meld::{Meld, is_menzen};
use crate::tile::TileType;
use crate::winning_context::WinningContext;
use crate::winning_tile::WinningTileInterpretation;
use crate::winning_yaku::{WinningYakuEvaluation, evaluate_winning_yaku};
use crate::yaku::Yaku;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct YakuHan {
    yaku: Yaku,
    han: u8,
}

impl YakuHan {
    fn new(yaku: Yaku, menzen: bool) -> Self {
        Self {
            yaku,
            han: han(yaku, menzen),
        }
    }

    pub fn yaku(self) -> Yaku {
        self.yaku
    }

    pub fn han(self) -> u8 {
        self.han
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WinningYakuHanEvaluation<'a> {
    interpretation: WinningTileInterpretation<'a>,
    yaku_han: Vec<YakuHan>,
}

impl<'a> WinningYakuHanEvaluation<'a> {
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

    pub fn is_empty(&self) -> bool {
        self.yaku_han.is_empty()
    }

    pub(crate) fn into_yaku_han(self) -> Vec<YakuHan> {
        self.yaku_han
    }
}

pub fn evaluate_winning_yaku_han(
    analysis: &CompletedHandAnalysis,
    context: WinningContext,
    winning_tile: TileType,
) -> Vec<WinningYakuHanEvaluation<'_>> {
    winning_yaku_han_evaluations(
        &evaluate_winning_yaku(analysis, context, winning_tile),
        analysis.fixed_melds(),
    )
}

/// 評価済みの待ちごとの役に翻を付ける。
///
/// 役判定は既存の役評価そのもので、ここでは食い下がりを含む翻を付けるだけ。役評価を持って
/// いる呼び出し側が同じ判定をもう一度走らせないための入口で、結果は
/// [`evaluate_winning_yaku_han`] と同じ。
pub(crate) fn winning_yaku_han_evaluations<'a>(
    yaku_evaluations: &[WinningYakuEvaluation<'a>],
    fixed_melds: &[Meld],
) -> Vec<WinningYakuHanEvaluation<'a>> {
    let menzen = is_menzen(fixed_melds);
    yaku_evaluations
        .iter()
        .map(|evaluation| winning_yaku_han(evaluation, menzen))
        .collect()
}

pub(crate) fn winning_yaku_han<'a>(
    evaluation: &WinningYakuEvaluation<'a>,
    menzen: bool,
) -> WinningYakuHanEvaluation<'a> {
    WinningYakuHanEvaluation {
        interpretation: evaluation.interpretation(),
        yaku_han: evaluation
            .yaku()
            .iter()
            .map(|yaku| YakuHan::new(*yaku, menzen))
            .collect(),
    }
}

fn han(yaku: Yaku, menzen: bool) -> u8 {
    match yaku {
        Yaku::Pinfu
        | Yaku::Tanyao
        | Yaku::Iipeikou
        | Yaku::YakuhaiWhite
        | Yaku::YakuhaiGreen
        | Yaku::YakuhaiRed
        | Yaku::YakuhaiRoundWind
        | Yaku::YakuhaiSeatWind
        | Yaku::Riichi
        | Yaku::Ippatsu
        | Yaku::MenzenTsumo
        | Yaku::Chankan
        | Yaku::RinshanKaihou
        | Yaku::Haitei
        | Yaku::Houtei => 1,
        Yaku::Chiitoitsu
        | Yaku::Toitoi
        | Yaku::Sanankou
        | Yaku::SanshokuDoukou
        | Yaku::Sankantsu
        | Yaku::Shousangen
        | Yaku::Honroutou
        | Yaku::DoubleRiichi => 2,
        Yaku::SanshokuDoujun | Yaku::Ittsu | Yaku::Chanta => kuisagari_han(2, menzen),
        Yaku::Ryanpeikou => 3,
        Yaku::Honitsu | Yaku::Junchan => kuisagari_han(3, menzen),
        Yaku::Chinitsu => kuisagari_han(6, menzen),
    }
}

fn kuisagari_han(closed_han: u8, menzen: bool) -> u8 {
    closed_han - u8::from(!menzen)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::completed_hand::analyze_completed_hand;
    use crate::meld::{Meld, MeldKind};
    use crate::tile::TileId;
    use crate::winning_context::{RiichiStatus, WinMethod};

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

    fn breakdowns(
        analysis: &CompletedHandAnalysis,
        context: WinningContext,
        winning_tile: &str,
    ) -> Vec<Vec<(Yaku, u8)>> {
        evaluate_winning_yaku_han(analysis, context, tile_type(winning_tile))
            .into_iter()
            .map(|evaluation| {
                evaluation
                    .yaku_han()
                    .iter()
                    .map(|yaku_han| (yaku_han.yaku(), yaku_han.han()))
                    .collect()
            })
            .collect()
    }

    fn totals(
        analysis: &CompletedHandAnalysis,
        context: WinningContext,
        winning_tile: &str,
    ) -> Vec<u8> {
        evaluate_winning_yaku_han(analysis, context, tile_type(winning_tile))
            .iter()
            .map(WinningYakuHanEvaluation::yaku_han_total)
            .collect()
    }

    fn only_breakdown(
        analysis: &CompletedHandAnalysis,
        context: WinningContext,
        winning_tile: &str,
    ) -> Vec<(Yaku, u8)> {
        let breakdowns = breakdowns(analysis, context, winning_tile);
        assert_eq!(breakdowns.len(), 1, "breakdowns: {breakdowns:?}");
        breakdowns.into_iter().next().unwrap()
    }

    fn only_total(
        analysis: &CompletedHandAnalysis,
        context: WinningContext,
        winning_tile: &str,
    ) -> u8 {
        let totals = totals(analysis, context, winning_tile);
        assert_eq!(totals.len(), 1, "totals: {totals:?}");
        totals[0]
    }

    const YAKU_HAN: [(Yaku, u8, u8); 30] = [
        (Yaku::Pinfu, 1, 1),
        (Yaku::Tanyao, 1, 1),
        (Yaku::Chiitoitsu, 2, 2),
        (Yaku::Toitoi, 2, 2),
        (Yaku::Sanankou, 2, 2),
        (Yaku::Iipeikou, 1, 1),
        (Yaku::Ryanpeikou, 3, 3),
        (Yaku::SanshokuDoujun, 2, 1),
        (Yaku::Ittsu, 2, 1),
        (Yaku::Chanta, 2, 1),
        (Yaku::Junchan, 3, 2),
        (Yaku::Honroutou, 2, 2),
        (Yaku::SanshokuDoukou, 2, 2),
        (Yaku::Sankantsu, 2, 2),
        (Yaku::Shousangen, 2, 2),
        (Yaku::Honitsu, 3, 2),
        (Yaku::Chinitsu, 6, 5),
        (Yaku::YakuhaiWhite, 1, 1),
        (Yaku::YakuhaiGreen, 1, 1),
        (Yaku::YakuhaiRed, 1, 1),
        (Yaku::YakuhaiRoundWind, 1, 1),
        (Yaku::YakuhaiSeatWind, 1, 1),
        (Yaku::Riichi, 1, 1),
        (Yaku::DoubleRiichi, 2, 2),
        (Yaku::Ippatsu, 1, 1),
        (Yaku::MenzenTsumo, 1, 1),
        (Yaku::Chankan, 1, 1),
        (Yaku::RinshanKaihou, 1, 1),
        (Yaku::Haitei, 1, 1),
        (Yaku::Houtei, 1, 1),
    ];

    #[test]
    fn every_yaku_has_a_han_value() {
        for (yaku, closed_han, open_han) in YAKU_HAN {
            assert_eq!(YakuHan::new(yaku, true).han(), closed_han, "yaku: {yaku:?}");
            assert_eq!(YakuHan::new(yaku, false).han(), open_han, "yaku: {yaku:?}");
        }
    }

    #[test]
    fn the_han_table_covers_every_yaku_variant() {
        let mut listed: Vec<Yaku> = YAKU_HAN.iter().map(|(yaku, _, _)| *yaku).collect();
        listed.sort_unstable();
        listed.dedup();

        assert_eq!(listed.len(), YAKU_HAN.len());
        for yaku in listed {
            match yaku {
                Yaku::Pinfu
                | Yaku::Tanyao
                | Yaku::Chiitoitsu
                | Yaku::Toitoi
                | Yaku::Sanankou
                | Yaku::Iipeikou
                | Yaku::Ryanpeikou
                | Yaku::SanshokuDoujun
                | Yaku::Ittsu
                | Yaku::Chanta
                | Yaku::Junchan
                | Yaku::Honroutou
                | Yaku::SanshokuDoukou
                | Yaku::Sankantsu
                | Yaku::Shousangen
                | Yaku::Honitsu
                | Yaku::Chinitsu
                | Yaku::YakuhaiWhite
                | Yaku::YakuhaiGreen
                | Yaku::YakuhaiRed
                | Yaku::YakuhaiRoundWind
                | Yaku::YakuhaiSeatWind
                | Yaku::Riichi
                | Yaku::DoubleRiichi
                | Yaku::Ippatsu
                | Yaku::MenzenTsumo
                | Yaku::Chankan
                | Yaku::RinshanKaihou
                | Yaku::Haitei
                | Yaku::Houtei => {}
            }
        }
    }

    #[test]
    fn only_kuisagari_yaku_lose_one_han_when_the_hand_is_open() {
        let kuisagari = [
            (Yaku::SanshokuDoujun, 2, 1),
            (Yaku::Ittsu, 2, 1),
            (Yaku::Chanta, 2, 1),
            (Yaku::Honitsu, 3, 2),
            (Yaku::Junchan, 3, 2),
            (Yaku::Chinitsu, 6, 5),
        ];

        for (yaku, closed_han, open_han) in kuisagari {
            assert_eq!(YakuHan::new(yaku, true).han(), closed_han, "yaku: {yaku:?}");
            assert_eq!(YakuHan::new(yaku, false).han(), open_han, "yaku: {yaku:?}");
        }
        for (yaku, closed_han, open_han) in YAKU_HAN {
            let reduced = closed_han != open_han;
            assert_eq!(
                reduced,
                kuisagari.iter().any(|(listed, _, _)| *listed == yaku),
                "yaku: {yaku:?}"
            );
        }
    }

    #[test]
    fn a_menzen_only_yaku_keeps_its_han_without_re_checking_the_hand() {
        for yaku in [
            Yaku::Pinfu,
            Yaku::Iipeikou,
            Yaku::Riichi,
            Yaku::Ippatsu,
            Yaku::MenzenTsumo,
            Yaku::Chiitoitsu,
        ] {
            assert_eq!(
                YakuHan::new(yaku, false).han(),
                YakuHan::new(yaku, true).han()
            );
            assert!(YakuHan::new(yaku, false).han() > 0);
        }
    }

    #[test]
    fn an_ankan_keeps_the_menzen_han_of_ittsu_and_honitsu() {
        let analysis = analyze(
            &[
                "1m", "2m", "3m", "4m", "5m", "6m", "7m", "8m", "9m", "E", "E",
            ],
            &[(MeldKind::Ankan, &["P", "P", "P", "P"])],
        );

        assert!(is_menzen(analysis.fixed_melds()));
        assert_eq!(
            only_breakdown(&analysis, ron(), "1m"),
            vec![
                (Yaku::Ittsu, 2),
                (Yaku::Honitsu, 3),
                (Yaku::YakuhaiWhite, 1),
            ]
        );
        assert_eq!(only_total(&analysis, ron(), "1m"), 6);
    }

    #[test]
    fn an_open_hand_gets_the_reduced_han_of_ittsu_and_honitsu() {
        let analysis = analyze(
            &[
                "1m", "2m", "3m", "4m", "5m", "6m", "7m", "8m", "9m", "E", "E",
            ],
            &[(MeldKind::Pon, &["P", "P", "P"])],
        );

        assert!(!is_menzen(analysis.fixed_melds()));
        assert_eq!(
            only_breakdown(&analysis, ron(), "1m"),
            vec![
                (Yaku::Ittsu, 1),
                (Yaku::Honitsu, 2),
                (Yaku::YakuhaiWhite, 1),
            ]
        );
        assert_eq!(only_total(&analysis, ron(), "1m"), 4);
    }

    #[test]
    fn an_ankan_keeps_the_menzen_han_of_chinitsu() {
        let analysis = analyze(
            &[
                "1m", "1m", "1m", "2m", "3m", "4m", "6m", "7m", "8m", "9m", "9m",
            ],
            &[(MeldKind::Ankan, &["5m", "5m", "5m", "5m"])],
        );

        assert!(is_menzen(analysis.fixed_melds()));
        assert_eq!(
            only_breakdown(&analysis, ron(), "9m"),
            vec![(Yaku::Chinitsu, 6)]
        );
    }

    #[test]
    fn an_open_toitoi_keeps_two_han() {
        let analysis = analyze(
            &["1m", "1m", "1m", "2m", "2m", "2m", "9p", "9p"],
            &[
                (MeldKind::Pon, &["5s", "5s", "5s"]),
                (MeldKind::Pon, &["3p", "3p", "3p"]),
            ],
        );

        assert!(!is_menzen(analysis.fixed_melds()));
        assert_eq!(
            only_breakdown(&analysis, ron(), "9p"),
            vec![(Yaku::Toitoi, 2)]
        );
    }

    #[test]
    fn a_double_wind_triplet_is_two_han() {
        let analysis = analyze(
            &[
                "E", "E", "E", "2m", "3m", "4m", "5m", "5m", "5p", "6p", "7p", "7s", "8s", "9s",
            ],
            &[],
        );
        let context = ron()
            .with_round_wind(Some(tile_type("E")))
            .with_seat_wind(Some(tile_type("E")));

        assert_eq!(
            only_breakdown(&analysis, context, "2m"),
            vec![(Yaku::YakuhaiRoundWind, 1), (Yaku::YakuhaiSeatWind, 1)]
        );
        assert_eq!(only_total(&analysis, context, "2m"), 2);
    }

    #[test]
    fn shousangen_is_four_han_through_its_dragon_yakuhai() {
        let analysis = analyze(
            &[
                "P", "P", "P", "F", "F", "F", "C", "C", "1m", "2m", "3m", "4p", "5p", "6p",
            ],
            &[],
        );

        assert_eq!(
            only_breakdown(&analysis, ron(), "1m"),
            vec![
                (Yaku::Shousangen, 2),
                (Yaku::YakuhaiWhite, 1),
                (Yaku::YakuhaiGreen, 1),
            ]
        );
        assert_eq!(only_total(&analysis, ron(), "1m"), 4);
    }

    #[test]
    fn honroutou_with_toitoi_is_four_han() {
        let analysis = analyze(
            &["1m", "1m", "1m", "9m", "9m", "9m", "1s", "1s"],
            &[
                (MeldKind::Pon, &["E", "E", "E"]),
                (MeldKind::Pon, &["9p", "9p", "9p"]),
            ],
        );

        assert_eq!(
            only_breakdown(&analysis, ron(), "1s"),
            vec![(Yaku::Toitoi, 2), (Yaku::Honroutou, 2)]
        );
        assert_eq!(only_total(&analysis, ron(), "1s"), 4);
    }

    #[test]
    fn honroutou_with_chiitoitsu_is_four_han() {
        let analysis = analyze(
            &[
                "1m", "1m", "9m", "9m", "1p", "1p", "9p", "9p", "1s", "1s", "9s", "9s", "E", "E",
            ],
            &[],
        );

        assert_eq!(
            only_breakdown(&analysis, ron(), "E"),
            vec![(Yaku::Chiitoitsu, 2), (Yaku::Honroutou, 2)]
        );
        assert_eq!(only_total(&analysis, ron(), "E"), 4);
    }

    #[test]
    fn riichi_and_double_riichi_add_ippatsu() {
        let analysis = analyze(
            &[
                "1m", "2m", "3m", "5p", "6p", "7p", "7s", "8s", "9s", "E", "E", "E", "5s", "5s",
            ],
            &[],
        );
        let riichi = ron()
            .with_riichi(RiichiStatus::Riichi)
            .with_ippatsu(Some(true));
        let double_riichi = ron()
            .with_riichi(RiichiStatus::DoubleRiichi)
            .with_ippatsu(Some(true));

        assert_eq!(
            only_breakdown(&analysis, riichi, "1m"),
            vec![(Yaku::Riichi, 1), (Yaku::Ippatsu, 1)]
        );
        assert_eq!(only_total(&analysis, riichi, "1m"), 2);
        assert_eq!(
            only_breakdown(&analysis, double_riichi, "1m"),
            vec![(Yaku::DoubleRiichi, 2), (Yaku::Ippatsu, 1)]
        );
        assert_eq!(only_total(&analysis, double_riichi, "1m"), 3);
    }

    #[test]
    fn ryanpeikou_does_not_add_iipeikou() {
        let analysis = analyze(
            &[
                "1m", "1m", "2m", "2m", "3m", "3m", "4p", "4p", "5p", "5p", "6p", "6p", "9s", "9s",
            ],
            &[],
        );

        let breakdowns = breakdowns(&analysis, ron(), "3m");

        assert!(
            breakdowns.contains(&vec![(Yaku::Ryanpeikou, 3)]),
            "breakdowns: {breakdowns:?}"
        );
        for breakdown in &breakdowns {
            assert!(
                !breakdown.iter().any(|(yaku, _)| *yaku == Yaku::Iipeikou),
                "breakdowns: {breakdowns:?}"
            );
        }
    }

    #[test]
    fn chinitsu_does_not_add_honitsu() {
        let analysis = analyze(
            &[
                "1m", "1m", "1m", "2m", "3m", "4m", "6m", "7m", "8m", "9m", "9m",
            ],
            &[(MeldKind::Ankan, &["5m", "5m", "5m", "5m"])],
        );

        let breakdown = only_breakdown(&analysis, ron(), "9m");

        assert!(!breakdown.iter().any(|(yaku, _)| *yaku == Yaku::Honitsu));
    }

    #[test]
    fn junchan_does_not_add_chanta() {
        let analysis = analyze(
            &[
                "1m", "2m", "3m", "7m", "8m", "9m", "1p", "2p", "3p", "9s", "9s", "9s", "1s", "1s",
            ],
            &[],
        );

        let breakdown = only_breakdown(&analysis, ron(), "1m");

        assert_eq!(breakdown, vec![(Yaku::Junchan, 3)]);
    }

    #[test]
    fn one_decomposition_keeps_a_han_per_interpretation() {
        let analysis = analyze(
            &[
                "1m", "2m", "3m", "3m", "4m", "5m", "4p", "5p", "6p", "5p", "5p", "7s", "8s", "9s",
            ],
            &[],
        );

        let evaluations = evaluate_winning_yaku_han(&analysis, ron(), tile_type("3m"));
        let yaku_evaluations = evaluate_winning_yaku(&analysis, ron(), tile_type("3m"));

        assert_eq!(evaluations.len(), yaku_evaluations.len());
        for (evaluation, yaku_evaluation) in evaluations.iter().zip(&yaku_evaluations) {
            assert_eq!(
                evaluation.interpretation(),
                yaku_evaluation.interpretation()
            );
            assert_eq!(evaluation.decomposition(), yaku_evaluation.decomposition());
        }
        assert_eq!(
            evaluations[0].decomposition(),
            evaluations[1].decomposition()
        );
        assert_eq!(
            breakdowns(&analysis, ron(), "3m"),
            vec![Vec::new(), vec![(Yaku::Pinfu, 1)]]
        );
        assert_eq!(totals(&analysis, ron(), "3m"), vec![0, 1]);
    }

    #[test]
    fn a_kokushi_hand_gets_no_yaku_han() {
        let analysis = analyze(
            &[
                "1m", "9m", "1p", "9p", "1s", "9s", "E", "S", "W", "N", "P", "F", "C", "9s",
            ],
            &[],
        );

        let evaluations = evaluate_winning_yaku_han(&analysis, ron(), tile_type("1m"));

        assert!(!evaluations.is_empty());
        for evaluation in &evaluations {
            assert!(evaluation.is_empty());
            assert_eq!(evaluation.yaku_han_total(), 0);
        }
    }

    #[test]
    fn breakdowns_follow_the_yaku_evaluations_and_stay_deterministic() {
        let analysis = analyze(
            &[
                "1m", "1m", "1m", "2m", "2m", "2m", "3m", "3m", "3m", "4m", "4m", "4m", "5m", "5m",
            ],
            &[],
        );

        let evaluations = evaluate_winning_yaku_han(&analysis, ron(), tile_type("3m"));
        let yaku_evaluations = evaluate_winning_yaku(&analysis, ron(), tile_type("3m"));

        assert_eq!(evaluations.len(), yaku_evaluations.len());
        for (evaluation, yaku_evaluation) in evaluations.iter().zip(&yaku_evaluations) {
            assert_eq!(
                evaluation.interpretation(),
                yaku_evaluation.interpretation()
            );
            let yaku: Vec<Yaku> = evaluation
                .yaku_han()
                .iter()
                .map(|yaku_han| yaku_han.yaku())
                .collect();
            assert_eq!(yaku, yaku_evaluation.yaku());
            assert_eq!(
                evaluation.yaku_han_total(),
                evaluation
                    .yaku_han()
                    .iter()
                    .map(|yaku_han| yaku_han.han())
                    .sum::<u8>()
            );
        }
        assert_eq!(
            evaluate_winning_yaku_han(&analysis, ron(), tile_type("3m")),
            evaluations
        );
    }

    #[test]
    fn the_entry_point_derives_menzen_from_the_analysis() {
        let analysis = analyze(
            &[
                "1m", "2m", "3m", "4m", "5m", "6m", "7m", "8m", "9m", "E", "E",
            ],
            &[(MeldKind::Pon, &["P", "P", "P"])],
        );
        let yaku_evaluations = evaluate_winning_yaku(&analysis, ron(), tile_type("1m"));

        let converted: Vec<WinningYakuHanEvaluation<'_>> = yaku_evaluations
            .iter()
            .map(|evaluation| winning_yaku_han(evaluation, is_menzen(analysis.fixed_melds())))
            .collect();

        assert_eq!(
            converted,
            evaluate_winning_yaku_han(&analysis, ron(), tile_type("1m"))
        );
        assert_eq!(converted[0].yaku_han_total(), 4);
        assert_ne!(
            yaku_evaluations
                .iter()
                .map(|evaluation| winning_yaku_han(evaluation, true))
                .collect::<Vec<_>>(),
            converted
        );
    }
}
