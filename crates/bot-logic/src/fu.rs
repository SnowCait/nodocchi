use crate::completed_hand::{
    CompletedHandAnalysis, CompletedHandDecomposition, StandardDecomposition,
};
use crate::meld::{Meld, MeldShape, is_menzen};
use crate::tile::TileType;
use crate::winning_context::{WinMethod, WinningContext};
use crate::winning_tile::{WaitType, WinningTileInterpretation};
use crate::winning_yaku::{
    WinningYakuEvaluation, completed_as_melded_triplet, evaluate_winning_yaku,
};
use crate::yaku::{Yaku, standard_meld_shapes};

const BASE_FU: u8 = 20;
const FU_ROUNDING_UNIT: u8 = 10;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum FuContribution {
    Base,
    Chiitoitsu,
    MenzenRon,
    Tsumo,
    OpenPinfuShapeRon,
    DragonPair,
    RoundWindPair,
    SeatWindPair,
    TankiWait,
    KanchanWait,
    PenchanWait,
    OpenSimpleTriplet,
    ClosedSimpleTriplet,
    OpenTerminalOrHonorTriplet,
    ClosedTerminalOrHonorTriplet,
    OpenSimpleKan,
    ClosedSimpleKan,
    OpenTerminalOrHonorKan,
    ClosedTerminalOrHonorKan,
}

impl FuContribution {
    pub fn fu(self) -> u8 {
        match self {
            Self::Base => BASE_FU,
            Self::Chiitoitsu => 25,
            Self::MenzenRon | Self::OpenPinfuShapeRon => 10,
            Self::Tsumo
            | Self::DragonPair
            | Self::RoundWindPair
            | Self::SeatWindPair
            | Self::TankiWait
            | Self::KanchanWait
            | Self::PenchanWait
            | Self::OpenSimpleTriplet => 2,
            Self::ClosedSimpleTriplet | Self::OpenTerminalOrHonorTriplet => 4,
            Self::ClosedTerminalOrHonorTriplet | Self::OpenSimpleKan => 8,
            Self::ClosedSimpleKan | Self::OpenTerminalOrHonorKan => 16,
            Self::ClosedTerminalOrHonorKan => 32,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum FuKind {
    Standard,
    PinfuTsumo,
    Chiitoitsu,
}

impl FuKind {
    fn uses_ten_fu_rounding(self) -> bool {
        matches!(self, Self::Standard)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FuBreakdown {
    kind: FuKind,
    contributions: Vec<FuContribution>,
    raw_fu: u8,
    fu: u8,
}

impl FuBreakdown {
    fn new(kind: FuKind, contributions: Vec<FuContribution>) -> Self {
        let raw_fu = total_fu(&contributions);
        Self {
            kind,
            contributions,
            raw_fu,
            fu: if kind.uses_ten_fu_rounding() {
                raw_fu.div_ceil(FU_ROUNDING_UNIT) * FU_ROUNDING_UNIT
            } else {
                raw_fu
            },
        }
    }

    pub fn kind(&self) -> FuKind {
        self.kind
    }

    pub fn contributions(&self) -> &[FuContribution] {
        &self.contributions
    }

    pub fn raw_fu(&self) -> u8 {
        self.raw_fu
    }

    pub fn fu(&self) -> u8 {
        self.fu
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WinningFuEvaluation<'a> {
    interpretation: WinningTileInterpretation<'a>,
    breakdown: Option<FuBreakdown>,
}

impl<'a> WinningFuEvaluation<'a> {
    pub fn interpretation(&self) -> WinningTileInterpretation<'a> {
        self.interpretation
    }

    pub fn decomposition(&self) -> &'a CompletedHandDecomposition {
        self.interpretation.decomposition()
    }

    pub fn breakdown(&self) -> Option<&FuBreakdown> {
        self.breakdown.as_ref()
    }
}

pub fn evaluate_winning_fu(
    analysis: &CompletedHandAnalysis,
    context: WinningContext,
    winning_tile: TileType,
) -> Vec<WinningFuEvaluation<'_>> {
    winning_fu_evaluations(
        &evaluate_winning_yaku(analysis, context, winning_tile),
        analysis.fixed_melds(),
        context,
    )
}

/// 評価済みの待ちごとの役から符を求める。
///
/// 符は役 (平和) に依存するため、必ず同じ完成手・同じ和了牌の役評価から導く。役評価を持って
/// いる呼び出し側が同じ判定をもう一度走らせないための入口で、結果は
/// [`evaluate_winning_fu`] と同じ。
pub(crate) fn winning_fu_evaluations<'a>(
    yaku_evaluations: &[WinningYakuEvaluation<'a>],
    fixed_melds: &[Meld],
    context: WinningContext,
) -> Vec<WinningFuEvaluation<'a>> {
    yaku_evaluations
        .iter()
        .map(|evaluation| winning_fu(evaluation, fixed_melds, context))
        .collect()
}

pub(crate) fn winning_fu<'a>(
    evaluation: &WinningYakuEvaluation<'a>,
    fixed_melds: &[Meld],
    context: WinningContext,
) -> WinningFuEvaluation<'a> {
    let interpretation = evaluation.interpretation();
    WinningFuEvaluation {
        breakdown: breakdown(
            &interpretation,
            fixed_melds,
            context,
            evaluation.contains(Yaku::Pinfu),
        ),
        interpretation,
    }
}

fn breakdown(
    interpretation: &WinningTileInterpretation<'_>,
    fixed_melds: &[Meld],
    context: WinningContext,
    pinfu: bool,
) -> Option<FuBreakdown> {
    match interpretation.decomposition() {
        CompletedHandDecomposition::Standard(standard) => {
            standard_breakdown(standard, interpretation, fixed_melds, context, pinfu)
        }
        CompletedHandDecomposition::Chiitoitsu(_) => Some(FuBreakdown::new(
            FuKind::Chiitoitsu,
            vec![FuContribution::Chiitoitsu],
        )),
        CompletedHandDecomposition::Kokushi(_) => None,
    }
}

fn standard_breakdown(
    standard: &StandardDecomposition,
    interpretation: &WinningTileInterpretation<'_>,
    fixed_melds: &[Meld],
    context: WinningContext,
    pinfu: bool,
) -> Option<FuBreakdown> {
    standard_meld_shapes(standard, fixed_melds)?;

    let win_method = context.win_method();
    let mut contributions = vec![FuContribution::Base];
    if win_method.is_tsumo() {
        if !pinfu {
            contributions.push(FuContribution::Tsumo);
        }
    } else if is_menzen(fixed_melds) {
        contributions.push(FuContribution::MenzenRon);
    }
    contributions.extend(pair_contributions(standard.pair(), context));
    contributions.extend(wait_contribution(interpretation.wait()));
    contributions.extend(concealed_set_contributions(
        standard,
        interpretation,
        win_method,
    ));
    contributions.extend(fixed_melds.iter().filter_map(fixed_set_contribution));

    if win_method.is_ron() && total_fu(&contributions) == BASE_FU {
        contributions.push(FuContribution::OpenPinfuShapeRon);
    }

    Some(FuBreakdown::new(
        if pinfu && win_method.is_tsumo() {
            FuKind::PinfuTsumo
        } else {
            FuKind::Standard
        },
        contributions,
    ))
}

fn pair_contributions(pair: TileType, context: WinningContext) -> Vec<FuContribution> {
    if pair.is_dragon() {
        return vec![FuContribution::DragonPair];
    }
    if !pair.is_wind() {
        return Vec::new();
    }

    let mut contributions = Vec::new();
    if context.round_wind() == Some(pair) {
        contributions.push(FuContribution::RoundWindPair);
    }
    if context.seat_wind() == Some(pair) {
        contributions.push(FuContribution::SeatWindPair);
    }
    contributions
}

fn wait_contribution(wait: WaitType) -> Option<FuContribution> {
    match wait {
        WaitType::Tanki => Some(FuContribution::TankiWait),
        WaitType::Kanchan => Some(FuContribution::KanchanWait),
        WaitType::Penchan => Some(FuContribution::PenchanWait),
        WaitType::Ryanmen
        | WaitType::Shanpon
        | WaitType::KokushiSingle
        | WaitType::KokushiThirteenSided => None,
    }
}

fn concealed_set_contributions(
    standard: &StandardDecomposition,
    interpretation: &WinningTileInterpretation<'_>,
    win_method: WinMethod,
) -> Vec<FuContribution> {
    let melded = ron_completed_triplet(interpretation, win_method);
    standard
        .concealed_melds()
        .iter()
        .filter_map(|meld| {
            let shape = meld.shape();
            let open = melded.is_some() && shape.triplet_tile_type() == melded;
            set_contribution(shape, open)
        })
        .collect()
}

fn ron_completed_triplet(
    interpretation: &WinningTileInterpretation<'_>,
    win_method: WinMethod,
) -> Option<TileType> {
    completed_as_melded_triplet(interpretation, win_method)
        .then(|| interpretation.group().meld_shape())
        .flatten()
        .and_then(MeldShape::triplet_tile_type)
}

fn fixed_set_contribution(meld: &Meld) -> Option<FuContribution> {
    set_contribution(meld.shape()?, meld.is_open())
}

fn set_contribution(shape: MeldShape, open: bool) -> Option<FuContribution> {
    let tile = shape.triplet_tile_type()?;
    Some(match (shape.is_kan(), open, tile.is_yaochu()) {
        (false, false, false) => FuContribution::ClosedSimpleTriplet,
        (false, false, true) => FuContribution::ClosedTerminalOrHonorTriplet,
        (false, true, false) => FuContribution::OpenSimpleTriplet,
        (false, true, true) => FuContribution::OpenTerminalOrHonorTriplet,
        (true, false, false) => FuContribution::ClosedSimpleKan,
        (true, false, true) => FuContribution::ClosedTerminalOrHonorKan,
        (true, true, false) => FuContribution::OpenSimpleKan,
        (true, true, true) => FuContribution::OpenTerminalOrHonorKan,
    })
}

fn total_fu(contributions: &[FuContribution]) -> u8 {
    contributions
        .iter()
        .map(|contribution| contribution.fu())
        .sum()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::completed_hand::analyze_completed_hand;
    use crate::meld::MeldKind;
    use crate::tile::TileId;
    use crate::winning_yaku::evaluate_winning_yaku;

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

    fn breakdowns(
        analysis: &CompletedHandAnalysis,
        context: WinningContext,
        winning_tile: &str,
    ) -> Vec<Option<FuBreakdown>> {
        evaluate_winning_fu(analysis, context, tile_type(winning_tile))
            .into_iter()
            .map(|evaluation| evaluation.breakdown().cloned())
            .collect()
    }

    fn only_breakdown(
        analysis: &CompletedHandAnalysis,
        context: WinningContext,
        winning_tile: &str,
    ) -> FuBreakdown {
        let breakdowns = breakdowns(analysis, context, winning_tile);
        assert_eq!(breakdowns.len(), 1, "breakdowns: {breakdowns:?}");
        breakdowns.into_iter().next().unwrap().unwrap()
    }

    fn contributions(
        concealed: &[&str],
        fixed: &[(MeldKind, &[&str])],
        context: WinningContext,
        winning_tile: &str,
    ) -> Vec<FuContribution> {
        only_breakdown(&analyze(concealed, fixed), context, winning_tile)
            .contributions()
            .to_vec()
    }

    fn fu(
        concealed: &[&str],
        fixed: &[(MeldKind, &[&str])],
        context: WinningContext,
        winning_tile: &str,
    ) -> (u8, u8) {
        let breakdown = only_breakdown(&analyze(concealed, fixed), context, winning_tile);
        (breakdown.raw_fu(), breakdown.fu())
    }

    const CHIITOITSU_HAND: [&str; 14] = [
        "1m", "1m", "3m", "3m", "5m", "5m", "7m", "7m", "9m", "9m", "1p", "1p", "E", "E",
    ];
    const PINFU_HAND: [&str; 14] = [
        "2m", "3m", "4m", "3m", "4m", "5m", "4p", "5p", "6p", "6p", "7p", "8p", "5s", "5s",
    ];
    const WAIT_HAND: [&str; 14] = [
        "1m", "2m", "3m", "4m", "5m", "6m", "4p", "5p", "6p", "6p", "7p", "8p", "5s", "5s",
    ];
    const KANCHAN_WITH_TERMINAL_TRIPLET: [&str; 14] = [
        "1m", "1m", "1m", "2m", "3m", "4m", "4p", "5p", "6p", "7s", "8s", "9s", "P", "P",
    ];
    const THREE_CONCEALED_TRIPLETS: [&str; 14] = [
        "1m", "1m", "1m", "2m", "2m", "2m", "3p", "3p", "3p", "4s", "5s", "6s", "9p", "9p",
    ];
    const PENCHAN_AND_RYANMEN: [&str; 14] = [
        "1m", "2m", "3m", "3m", "4m", "5m", "4p", "5p", "6p", "5p", "5p", "7s", "8s", "9s",
    ];
    const KOKUSHI_HAND: [&str; 14] = [
        "1m", "9m", "1p", "9p", "1s", "9s", "E", "S", "W", "N", "P", "F", "C", "9s",
    ];
    const RYANPEIKOU_CHIITOITSU: [&str; 14] = [
        "2p", "2p", "3p", "3p", "4p", "4p", "6p", "6p", "7p", "7p", "8p", "8p", "5s", "5s",
    ];
    const OPEN_ALL_SEQUENCES: [&str; 11] = [
        "4p", "5p", "6p", "6p", "7p", "8p", "1s", "2s", "3s", "5s", "5s",
    ];
    const SET_FU_REST: [&str; 11] = [
        "4p", "5p", "6p", "7p", "8p", "9p", "2s", "3s", "4s", "5s", "5s",
    ];

    fn pair_hand(pair: &'static str) -> Vec<&'static str> {
        vec![
            "2m", "3m", "4m", "3m", "4m", "5m", "4p", "5p", "6p", "6p", "7p", "8p", pair, pair,
        ]
    }

    fn concealed_set_hand(tile: &'static str) -> Vec<&'static str> {
        let mut concealed = vec![tile, tile, tile];
        concealed.extend(SET_FU_REST);
        concealed
    }

    #[test]
    fn contributions_keep_the_documented_fu_table() {
        let table = [
            (FuContribution::Base, 20),
            (FuContribution::Chiitoitsu, 25),
            (FuContribution::MenzenRon, 10),
            (FuContribution::Tsumo, 2),
            (FuContribution::OpenPinfuShapeRon, 10),
            (FuContribution::DragonPair, 2),
            (FuContribution::RoundWindPair, 2),
            (FuContribution::SeatWindPair, 2),
            (FuContribution::TankiWait, 2),
            (FuContribution::KanchanWait, 2),
            (FuContribution::PenchanWait, 2),
            (FuContribution::OpenSimpleTriplet, 2),
            (FuContribution::ClosedSimpleTriplet, 4),
            (FuContribution::OpenTerminalOrHonorTriplet, 4),
            (FuContribution::ClosedTerminalOrHonorTriplet, 8),
            (FuContribution::OpenSimpleKan, 8),
            (FuContribution::ClosedSimpleKan, 16),
            (FuContribution::OpenTerminalOrHonorKan, 16),
            (FuContribution::ClosedTerminalOrHonorKan, 32),
        ];

        for (contribution, expected) in table {
            assert_eq!(
                contribution.fu(),
                expected,
                "contribution: {contribution:?}"
            );
        }
    }

    #[test]
    fn chiitoitsu_is_always_twenty_five_fu() {
        let analysis = analyze(&CHIITOITSU_HAND, &[]);

        for context in [ron(), tsumo()] {
            let breakdown = only_breakdown(&analysis, context, "E");

            assert_eq!(breakdown.kind(), FuKind::Chiitoitsu);
            assert_eq!(breakdown.contributions(), [FuContribution::Chiitoitsu]);
            assert_eq!(breakdown.raw_fu(), 25);
            assert_eq!(breakdown.fu(), 25);
        }
    }

    #[test]
    fn chiitoitsu_is_not_rounded_up_to_thirty() {
        let analysis = analyze(&CHIITOITSU_HAND, &[]);
        let breakdown = only_breakdown(&analysis, ron(), "E");

        assert_eq!((breakdown.raw_fu(), breakdown.fu()), (25, 25));
        assert_eq!(breakdown.fu() % FU_ROUNDING_UNIT, 5);
    }

    #[test]
    fn only_the_standard_kind_uses_ten_fu_rounding() {
        assert!(FuKind::Standard.uses_ten_fu_rounding());
        assert!(!FuKind::PinfuTsumo.uses_ten_fu_rounding());
        assert!(!FuKind::Chiitoitsu.uses_ten_fu_rounding());
    }

    #[test]
    fn menzen_ron_pinfu_is_thirty_fu() {
        let analysis = analyze(&PINFU_HAND, &[]);

        assert!(evaluate_winning_yaku(&analysis, ron(), tile_type("2m"))[0].contains(Yaku::Pinfu));
        let breakdown = only_breakdown(&analysis, ron(), "2m");

        assert_eq!(breakdown.kind(), FuKind::Standard);
        assert_eq!(
            breakdown.contributions(),
            [FuContribution::Base, FuContribution::MenzenRon]
        );
        assert_eq!((breakdown.raw_fu(), breakdown.fu()), (30, 30));
    }

    #[test]
    fn menzen_tsumo_pinfu_is_twenty_fu() {
        let analysis = analyze(&PINFU_HAND, &[]);

        assert!(
            evaluate_winning_yaku(&analysis, tsumo(), tile_type("2m"))[0].contains(Yaku::Pinfu)
        );
        let breakdown = only_breakdown(&analysis, tsumo(), "2m");

        assert_eq!(breakdown.kind(), FuKind::PinfuTsumo);
        assert_eq!(breakdown.contributions(), [FuContribution::Base]);
        assert_eq!((breakdown.raw_fu(), breakdown.fu()), (20, 20));
    }

    #[test]
    fn menzen_ron_adds_ten_fu() {
        assert_eq!(
            contributions(&WAIT_HAND, &[], ron(), "3m"),
            [
                FuContribution::Base,
                FuContribution::MenzenRon,
                FuContribution::PenchanWait,
            ]
        );
    }

    #[test]
    fn non_pinfu_tsumo_adds_two_fu() {
        assert_eq!(
            contributions(&WAIT_HAND, &[], tsumo(), "3m"),
            [
                FuContribution::Base,
                FuContribution::Tsumo,
                FuContribution::PenchanWait,
            ]
        );
    }

    #[test]
    fn open_ron_does_not_add_menzen_ron_fu() {
        let fixed: [(MeldKind, &[&str]); 1] = [(MeldKind::Pon, &["1m", "1m", "1m"])];

        assert_eq!(
            contributions(&SET_FU_REST, &fixed, ron(), "4p"),
            [
                FuContribution::Base,
                FuContribution::OpenTerminalOrHonorTriplet,
            ]
        );
    }

    #[test]
    fn an_ankan_keeps_the_menzen_ron_fu() {
        let fixed: [(MeldKind, &[&str]); 1] = [(MeldKind::Ankan, &["3m", "3m", "3m", "3m"])];

        assert_eq!(
            contributions(&SET_FU_REST, &fixed, ron(), "4p"),
            [
                FuContribution::Base,
                FuContribution::MenzenRon,
                FuContribution::ClosedSimpleKan,
            ]
        );
    }

    #[test]
    fn dragon_pair_is_two_fu() {
        assert_eq!(
            contributions(&pair_hand("P"), &[], ron(), "2m"),
            [
                FuContribution::Base,
                FuContribution::MenzenRon,
                FuContribution::DragonPair,
            ]
        );
    }

    #[test]
    fn round_wind_pair_is_two_fu() {
        let context = ron()
            .with_round_wind(Some(tile_type("E")))
            .with_seat_wind(Some(tile_type("S")));

        assert_eq!(
            contributions(&pair_hand("E"), &[], context, "2m"),
            [
                FuContribution::Base,
                FuContribution::MenzenRon,
                FuContribution::RoundWindPair,
            ]
        );
    }

    #[test]
    fn seat_wind_pair_is_two_fu() {
        let context = ron()
            .with_round_wind(Some(tile_type("S")))
            .with_seat_wind(Some(tile_type("E")));

        assert_eq!(
            contributions(&pair_hand("E"), &[], context, "2m"),
            [
                FuContribution::Base,
                FuContribution::MenzenRon,
                FuContribution::SeatWindPair,
            ]
        );
    }

    #[test]
    fn double_wind_pair_is_four_fu() {
        let context = ron()
            .with_round_wind(Some(tile_type("E")))
            .with_seat_wind(Some(tile_type("E")));
        let breakdown = only_breakdown(&analyze(&pair_hand("E"), &[]), context, "2m");

        assert_eq!(
            breakdown.contributions(),
            [
                FuContribution::Base,
                FuContribution::MenzenRon,
                FuContribution::RoundWindPair,
                FuContribution::SeatWindPair,
            ]
        );
        assert_eq!(
            total_fu(&breakdown.contributions()[2..]),
            4,
            "連風牌の雀頭は4符"
        );
        assert_eq!((breakdown.raw_fu(), breakdown.fu()), (34, 40));
    }

    #[test]
    fn non_value_pair_is_no_fu() {
        let context = ron()
            .with_round_wind(Some(tile_type("E")))
            .with_seat_wind(Some(tile_type("S")));

        assert_eq!(
            contributions(&pair_hand("5s"), &[], context, "2m"),
            [FuContribution::Base, FuContribution::MenzenRon]
        );
        assert_eq!(
            contributions(&pair_hand("W"), &[], context, "2m"),
            [FuContribution::Base, FuContribution::MenzenRon]
        );
    }

    #[test]
    fn unknown_seat_wind_does_not_become_a_double_wind_pair() {
        let context = ron().with_round_wind(Some(tile_type("E")));

        assert_eq!(
            contributions(&pair_hand("E"), &[], context, "2m"),
            [
                FuContribution::Base,
                FuContribution::MenzenRon,
                FuContribution::RoundWindPair,
            ]
        );
    }

    #[test]
    fn unknown_winds_add_no_pair_fu() {
        assert_eq!(
            contributions(&pair_hand("E"), &[], ron(), "2m"),
            [FuContribution::Base, FuContribution::MenzenRon]
        );
    }

    #[test]
    fn dragon_pair_needs_no_wind_context() {
        assert_eq!(
            contributions(&pair_hand("C"), &[], ron(), "2m"),
            [
                FuContribution::Base,
                FuContribution::MenzenRon,
                FuContribution::DragonPair,
            ]
        );
    }

    #[test]
    fn incomplete_waits_are_two_fu_and_two_sided_waits_are_none() {
        let table = [
            ("5s", Some(FuContribution::TankiWait)),
            ("5m", Some(FuContribution::KanchanWait)),
            ("3m", Some(FuContribution::PenchanWait)),
            ("1m", None),
        ];

        for (winning_tile, expected) in table {
            let mut expected_contributions = vec![FuContribution::Base, FuContribution::MenzenRon];
            expected_contributions.extend(expected);

            assert_eq!(
                contributions(&WAIT_HAND, &[], ron(), winning_tile),
                expected_contributions,
                "winning tile: {winning_tile}"
            );
        }
    }

    #[test]
    fn a_shanpon_wait_has_no_wait_fu() {
        let analysis = analyze(&THREE_CONCEALED_TRIPLETS, &[]);
        let evaluations = evaluate_winning_fu(&analysis, tsumo(), tile_type("1m"));
        let evaluation = evaluations.first().unwrap();

        assert_eq!(evaluation.interpretation().wait(), WaitType::Shanpon);
        assert_eq!(wait_contribution(WaitType::Shanpon), None);
        assert_eq!(wait_contribution(WaitType::Ryanmen), None);
    }

    #[test]
    fn triplet_fu_follows_the_open_and_terminal_state() {
        let closed = [
            ("3m", FuContribution::ClosedSimpleTriplet),
            ("1m", FuContribution::ClosedTerminalOrHonorTriplet),
            ("C", FuContribution::ClosedTerminalOrHonorTriplet),
        ];
        for (tile, expected) in closed {
            assert_eq!(
                contributions(&concealed_set_hand(tile), &[], tsumo(), "4p"),
                [FuContribution::Base, FuContribution::Tsumo, expected],
                "tile: {tile}"
            );
        }

        let open = [
            ("3m", FuContribution::OpenSimpleTriplet),
            ("1m", FuContribution::OpenTerminalOrHonorTriplet),
            ("C", FuContribution::OpenTerminalOrHonorTriplet),
        ];
        for (tile, expected) in open {
            let fixed: [(MeldKind, &[&str]); 1] = [(MeldKind::Pon, &[tile, tile, tile])];

            assert_eq!(
                contributions(&SET_FU_REST, &fixed, tsumo(), "4p"),
                [FuContribution::Base, FuContribution::Tsumo, expected],
                "tile: {tile}"
            );
        }
    }

    #[test]
    fn kan_fu_follows_the_open_and_terminal_state() {
        let table = [
            (MeldKind::Daiminkan, "3m", FuContribution::OpenSimpleKan),
            (MeldKind::Kakan, "3m", FuContribution::OpenSimpleKan),
            (MeldKind::Ankan, "3m", FuContribution::ClosedSimpleKan),
            (
                MeldKind::Daiminkan,
                "1m",
                FuContribution::OpenTerminalOrHonorKan,
            ),
            (MeldKind::Kakan, "C", FuContribution::OpenTerminalOrHonorKan),
            (
                MeldKind::Ankan,
                "1m",
                FuContribution::ClosedTerminalOrHonorKan,
            ),
        ];

        for (kind, tile, expected) in table {
            let fixed: [(MeldKind, &[&str]); 1] = [(kind, &[tile, tile, tile, tile])];

            assert!(
                contributions(&SET_FU_REST, &fixed, tsumo(), "4p").contains(&expected),
                "kind: {kind:?}, tile: {tile}"
            );
        }
    }

    #[test]
    fn a_triplet_completed_by_ron_is_counted_as_an_open_triplet() {
        let analysis = analyze(&THREE_CONCEALED_TRIPLETS, &[]);

        assert_eq!(
            only_breakdown(&analysis, tsumo(), "1m").contributions(),
            [
                FuContribution::Base,
                FuContribution::Tsumo,
                FuContribution::ClosedTerminalOrHonorTriplet,
                FuContribution::ClosedSimpleTriplet,
                FuContribution::ClosedSimpleTriplet,
            ]
        );
        assert_eq!(
            only_breakdown(&analysis, ron(), "1m").contributions(),
            [
                FuContribution::Base,
                FuContribution::MenzenRon,
                FuContribution::OpenTerminalOrHonorTriplet,
                FuContribution::ClosedSimpleTriplet,
                FuContribution::ClosedSimpleTriplet,
            ]
        );
    }

    #[test]
    fn a_triplet_completed_by_ron_shares_the_sanankou_semantics() {
        let analysis = analyze(&THREE_CONCEALED_TRIPLETS, &[]);

        assert!(
            evaluate_winning_yaku(&analysis, tsumo(), tile_type("1m"))[0].contains(Yaku::Sanankou)
        );
        assert!(
            !evaluate_winning_yaku(&analysis, ron(), tile_type("1m"))[0].contains(Yaku::Sanankou)
        );
        assert_eq!(fu(&THREE_CONCEALED_TRIPLETS, &[], tsumo(), "1m"), (38, 40));
        assert_eq!(fu(&THREE_CONCEALED_TRIPLETS, &[], ron(), "1m"), (42, 50));
    }

    #[test]
    fn an_open_hand_without_any_other_fu_is_thirty_fu_on_ron() {
        let fixed: [(MeldKind, &[&str]); 1] = [(MeldKind::Chi, &["1m", "2m", "3m"])];
        let breakdown = only_breakdown(&analyze(&OPEN_ALL_SEQUENCES, &fixed), ron(), "4p");

        assert_eq!(
            breakdown.contributions(),
            [FuContribution::Base, FuContribution::OpenPinfuShapeRon]
        );
        assert_eq!((breakdown.raw_fu(), breakdown.fu()), (30, 30));
    }

    #[test]
    fn an_open_hand_without_any_other_fu_is_thirty_fu_on_tsumo() {
        let fixed: [(MeldKind, &[&str]); 1] = [(MeldKind::Chi, &["1m", "2m", "3m"])];
        let breakdown = only_breakdown(&analyze(&OPEN_ALL_SEQUENCES, &fixed), tsumo(), "4p");

        assert_eq!(
            breakdown.contributions(),
            [FuContribution::Base, FuContribution::Tsumo]
        );
        assert_eq!((breakdown.raw_fu(), breakdown.fu()), (22, 30));
    }

    #[test]
    fn an_open_pinfu_shape_is_not_a_pinfu_yaku() {
        let fixed: [(MeldKind, &[&str]); 1] = [(MeldKind::Chi, &["1m", "2m", "3m"])];
        let analysis = analyze(&OPEN_ALL_SEQUENCES, &fixed);

        assert!(!evaluate_winning_yaku(&analysis, ron(), tile_type("4p"))[0].contains(Yaku::Pinfu));
        assert_eq!(
            only_breakdown(&analysis, ron(), "4p").kind(),
            FuKind::Standard
        );
    }

    #[test]
    fn raw_fu_is_rounded_up_to_ten_fu() {
        let fixed: [(MeldKind, &[&str]); 1] = [(MeldKind::Chi, &["1m", "2m", "3m"])];

        assert_eq!(fu(&OPEN_ALL_SEQUENCES, &fixed, tsumo(), "4p"), (22, 30));
        assert_eq!(fu(&WAIT_HAND, &[], tsumo(), "3m"), (24, 30));
        assert_eq!(fu(&WAIT_HAND, &[], ron(), "3m"), (32, 40));
        assert_eq!(
            fu(&KANCHAN_WITH_TERMINAL_TRIPLET, &[], ron(), "3m"),
            (42, 50)
        );
    }

    #[test]
    fn a_documented_breakdown_explains_its_raw_fu() {
        let breakdown = only_breakdown(&analyze(&KANCHAN_WITH_TERMINAL_TRIPLET, &[]), ron(), "3m");

        assert_eq!(
            breakdown.contributions(),
            [
                FuContribution::Base,
                FuContribution::MenzenRon,
                FuContribution::DragonPair,
                FuContribution::KanchanWait,
                FuContribution::ClosedTerminalOrHonorTriplet,
            ]
        );
        assert_eq!(breakdown.raw_fu(), total_fu(breakdown.contributions()));
        assert_eq!((breakdown.raw_fu(), breakdown.fu()), (42, 50));
    }

    #[test]
    fn interpretations_of_one_decomposition_keep_their_own_fu() {
        let analysis = analyze(&PENCHAN_AND_RYANMEN, &[]);
        let evaluations = evaluate_winning_fu(&analysis, ron(), tile_type("3m"));

        assert_eq!(evaluations.len(), 2);
        assert_eq!(
            evaluations[0].decomposition(),
            evaluations[1].decomposition()
        );
        assert_eq!(
            evaluations[0].breakdown().unwrap().contributions(),
            [
                FuContribution::Base,
                FuContribution::MenzenRon,
                FuContribution::PenchanWait,
            ]
        );
        assert_eq!(
            evaluations[1].breakdown().unwrap().contributions(),
            [FuContribution::Base, FuContribution::MenzenRon]
        );
        assert_eq!(
            evaluations
                .iter()
                .map(|evaluation| evaluation.breakdown().unwrap().fu())
                .collect::<Vec<_>>(),
            vec![40, 30]
        );
    }

    #[test]
    fn decompositions_of_one_hand_keep_their_own_fu() {
        let analysis = analyze(&RYANPEIKOU_CHIITOITSU, &[]);
        let breakdowns = breakdowns(&analysis, ron(), "5s");

        assert_eq!(
            breakdowns
                .iter()
                .map(|breakdown| {
                    let breakdown = breakdown.as_ref().unwrap();
                    (breakdown.kind(), breakdown.raw_fu(), breakdown.fu())
                })
                .collect::<Vec<_>>(),
            vec![(FuKind::Standard, 32, 40), (FuKind::Chiitoitsu, 25, 25)]
        );
    }

    #[test]
    fn kokushi_has_no_fu() {
        let analysis = analyze(&KOKUSHI_HAND, &[]);

        for winning_tile in ["9s", "1m"] {
            let evaluations = evaluate_winning_fu(&analysis, ron(), tile_type(winning_tile));

            assert!(!evaluations.is_empty());
            assert!(
                evaluations
                    .iter()
                    .all(|evaluation| evaluation.breakdown().is_none()),
                "winning tile: {winning_tile}"
            );
        }
    }

    #[test]
    fn evaluations_are_deterministic() {
        let analysis = analyze(&PENCHAN_AND_RYANMEN, &[]);

        assert_eq!(
            evaluate_winning_fu(&analysis, ron(), tile_type("3m")),
            evaluate_winning_fu(&analysis, ron(), tile_type("3m"))
        );
    }

    #[test]
    fn evaluations_follow_the_winning_yaku_interpretations() {
        let analysis = analyze(&PENCHAN_AND_RYANMEN, &[]);

        assert_eq!(
            evaluate_winning_fu(&analysis, ron(), tile_type("3m"))
                .iter()
                .map(WinningFuEvaluation::interpretation)
                .collect::<Vec<_>>(),
            evaluate_winning_yaku(&analysis, ron(), tile_type("3m"))
                .iter()
                .map(WinningYakuEvaluation::interpretation)
                .collect::<Vec<_>>()
        );
    }
}
