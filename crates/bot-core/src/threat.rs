use bot_logic::{TileId, TileType, count_dora};

use crate::context::GameContext;
use crate::meld::{Meld, MeldKind};

/// fixed meld の [`MeldKind`] 別内訳。件数だけを持つ観測事実。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct MeldKindCounts {
    pub chi: usize,
    pub pon: usize,
    pub daiminkan: usize,
    pub ankan: usize,
    pub kakan: usize,
}

impl MeldKindCounts {
    /// fixed meld 一覧から種類別に数える。Ankan も fixed meld として数える。
    pub fn of(melds: &[Meld]) -> Self {
        let mut counts = Self::default();
        for meld in melds {
            *counts.get_mut(meld.kind()) += 1;
        }
        counts
    }

    pub fn get(self, kind: MeldKind) -> usize {
        match kind {
            MeldKind::Chi => self.chi,
            MeldKind::Pon => self.pon,
            MeldKind::Daiminkan => self.daiminkan,
            MeldKind::Ankan => self.ankan,
            MeldKind::Kakan => self.kakan,
        }
    }

    /// 全 [`MeldKind`] の合計。`PlayerThreatDiagnostic::meld_count` と一致する。
    pub fn total(self) -> usize {
        self.chi + self.pon + self.daiminkan + self.ankan + self.kakan
    }

    fn get_mut(&mut self, kind: MeldKind) -> &mut usize {
        match kind {
            MeldKind::Chi => &mut self.chi,
            MeldKind::Pon => &mut self.pon,
            MeldKind::Daiminkan => &mut self.daiminkan,
            MeldKind::Ankan => &mut self.ankan,
            MeldKind::Kakan => &mut self.kakan,
        }
    }
}

/// 刻子・槓子の牌種が役牌になり得るかの観測事実。翻数へは潰さない。
///
/// 場風と自風を別々に持つため、ダブ東・ダブ南も後から正しく扱える。
///
/// `is_round_wind` / `is_seat_wind` は、場風または対象 player の自風が不明な風牌では `None`
/// (unknown)。unknown を `false` として「役牌ではない」と断定しない。三元牌は場風・自風には
/// 決してならないため、風情報が無くても `Some(false)`。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ValueHonorMeldDiagnostic {
    pub tile: TileType,
    pub is_dragon: bool,
    pub is_round_wind: Option<bool>,
    pub is_seat_wind: Option<bool>,
}

/// fixed meld 1つ分の観測事実。危険度の判断は持たない。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MeldThreatDiagnostic {
    pub kind: MeldKind,
    /// meld を構成する物理牌。Kakan は加槓牌を含む4枚。
    pub tiles: Vec<TileId>,
    /// [`MeldKind::is_open`] の結果。Ankan は fixed meld だが `false`。
    pub is_open: bool,
    /// [`MeldKind::is_kan`] の結果。
    pub is_kan: bool,
    /// meld 内の物理牌に対する [`count_dora`] の合計。表示牌ドラと赤ドラを含む既存 semantics で、
    /// 赤5が表示牌ドラでもあれば両方数える。
    pub dora_count: u8,
    /// meld 内の赤ドラ ([`TileId::is_red`]) の枚数。`dora_count` の内数。
    pub red_dora_count: u8,
    /// 字牌の刻子・槓子の場合の役牌診断。Chi と数牌の刻子・槓子は役牌になり得ないため `None`。
    pub value_honor: Option<ValueHonorMeldDiagnostic>,
}

/// player 1人分の観測事実。危険度 (threat level) の判断は持たない。
///
/// 副露数やドラ枚数からテンパイ・向聴数を推測しない。ここにあるのは観測できた事実だけで、
/// そこから threat level を決める policy は呼び出し側の責務。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlayerThreatDiagnostic {
    pub player: usize,
    /// 自分の席か。`player_id` が不明なら推測せず `None` (unknown)。
    pub is_self: Option<bool>,
    /// 親の席か。`oya` が不明なら推測せず `None` (unknown)。
    pub is_dealer: Option<bool>,
    pub reached: bool,
    /// この player の自風。`oya` から導出できない場合は推測せず `None`。
    pub seat_wind: Option<TileType>,
    /// Chi / Pon / Daiminkan / Ankan / Kakan を含む fixed meld の総数。
    pub meld_count: usize,
    /// `meld_count` のうち [`MeldKind::is_open`] が `true` のものだけ。Ankan は含まない。
    pub open_meld_count: usize,
    /// `meld_count` のうち [`MeldKind::is_kan`] が `true` のものだけ。Ankan を含む。
    pub kan_count: usize,
    pub meld_kinds: MeldKindCounts,
    /// fixed meld ごとの観測事実。`melds` の順序は `GameContext` の順序そのまま。
    pub melds: Vec<MeldThreatDiagnostic>,
    /// 全 fixed meld の `dora_count` 合計。Ankan の分も含むので、公開分だけを見たい policy は
    /// meld ごとの `is_open` で絞る。
    pub meld_dora_count: u8,
    /// 全 fixed meld の `red_dora_count` 合計。`meld_dora_count` の内数。
    pub meld_red_dora_count: u8,
}

impl PlayerThreatDiagnostic {
    /// 他家の席か。`player_id` が不明なら推測せず `None` (unknown)。
    pub fn is_opponent(&self) -> Option<bool> {
        self.is_self.map(|is_self| !is_self)
    }
}

/// [`diagnose_player_threat`] の入力。`GameContext` から取り出した観測事実だけを持つ。
///
/// `GameContext` からデータを取り出す adapter ([`player_threat_inputs`]) と、そこから診断を組み立てる
/// pure な logic を分けるための型。ここで不足情報を補完しない。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlayerThreatInputs<'a> {
    pub player: usize,
    pub is_self: Option<bool>,
    pub is_dealer: Option<bool>,
    pub reached: bool,
    pub round_wind: Option<TileType>,
    /// 対象 player 自身の自風。自分の自風ではない。
    pub seat_wind: Option<TileType>,
    pub melds: &'a [Meld],
    pub dora_indicators: &'a [TileId],
}

/// fixed meld 1つ分の観測事実を作る pure helper。
///
/// ドラ判定は既存の [`count_dora`] と [`TileId::is_red`] をそのまま使い、別の判定器を作らない。
/// `seat_wind` は meld を持つ player 自身の自風。
pub fn diagnose_meld_threat(
    meld: &Meld,
    dora_indicators: &[TileId],
    round_wind: Option<TileType>,
    seat_wind: Option<TileType>,
) -> MeldThreatDiagnostic {
    let mut dora_count = 0u8;
    let mut red_dora_count = 0u8;
    for &tile in meld.tiles() {
        dora_count = dora_count.saturating_add(count_dora(tile, dora_indicators));
        if tile.is_red() {
            red_dora_count = red_dora_count.saturating_add(1);
        }
    }

    MeldThreatDiagnostic {
        kind: meld.kind(),
        tiles: meld.tiles().to_vec(),
        is_open: meld.is_open(),
        is_kan: meld.kind().is_kan(),
        dora_count,
        red_dora_count,
        value_honor: value_honor_meld_diagnostic(meld, round_wind, seat_wind),
    }
}

/// player 1人分の観測事実を作る pure helper。向聴数・受け入れ・待ちの再計算は行わない。
pub fn diagnose_player_threat(inputs: PlayerThreatInputs<'_>) -> PlayerThreatDiagnostic {
    let melds: Vec<MeldThreatDiagnostic> = inputs
        .melds
        .iter()
        .map(|meld| {
            diagnose_meld_threat(
                meld,
                inputs.dora_indicators,
                inputs.round_wind,
                inputs.seat_wind,
            )
        })
        .collect();

    let mut meld_dora_count = 0u8;
    let mut meld_red_dora_count = 0u8;
    for meld in &melds {
        meld_dora_count = meld_dora_count.saturating_add(meld.dora_count);
        meld_red_dora_count = meld_red_dora_count.saturating_add(meld.red_dora_count);
    }

    PlayerThreatDiagnostic {
        player: inputs.player,
        is_self: inputs.is_self,
        is_dealer: inputs.is_dealer,
        reached: inputs.reached,
        seat_wind: inputs.seat_wind,
        meld_count: melds.len(),
        open_meld_count: melds.iter().filter(|meld| meld.is_open).count(),
        kan_count: melds.iter().filter(|meld| meld.is_kan).count(),
        meld_kinds: MeldKindCounts::of(inputs.melds),
        melds,
        meld_dora_count,
        meld_red_dora_count,
    }
}

/// `GameContext` から指定 player の診断入力を取り出す adapter。
///
/// `player_id` / `oya` が不明な場合は `is_self` / `is_dealer` / `seat_wind` を unknown のままにし、
/// 「player 0 が自分」「player 0 が東」のような推測をしない。
pub fn player_threat_inputs(context: &GameContext, player: usize) -> PlayerThreatInputs<'_> {
    PlayerThreatInputs {
        player,
        is_self: context
            .player_id()
            .map(|player_id| usize::from(player_id) == player),
        is_dealer: context.oya().map(|oya| usize::from(oya) == player),
        reached: context.is_reached(player),
        round_wind: context.round_wind(),
        seat_wind: context.seat_wind_of(player),
        melds: context.melds_of(player).unwrap_or_default(),
        dora_indicators: context.dora_indicators(),
    }
}

/// `GameContext` から全4席分の観測事実を作る adapter。
///
/// `player_id` が不明でもどの席も除外せず、常に4席分を返す。自分と他家の区別は各診断の
/// `is_self` / `is_opponent()` が unknown で表す。
pub fn diagnose_player_threats(context: &GameContext) -> [PlayerThreatDiagnostic; 4] {
    std::array::from_fn(|player| diagnose_player_threat(player_threat_inputs(context, player)))
}

// 刻子・槓子の牌種から役牌診断を作る。Chi は牌種が揃わないので対象外。
fn value_honor_meld_diagnostic(
    meld: &Meld,
    round_wind: Option<TileType>,
    seat_wind: Option<TileType>,
) -> Option<ValueHonorMeldDiagnostic> {
    if matches!(meld.kind(), MeldKind::Chi) {
        return None;
    }
    let tile = meld.tiles().first()?.tile_type();
    if !tile.is_honor() {
        return None;
    }

    Some(ValueHonorMeldDiagnostic {
        tile,
        is_dragon: tile.is_dragon(),
        is_round_wind: matches_wind(tile, round_wind),
        is_seat_wind: matches_wind(tile, seat_wind),
    })
}

// 風牌でなければ場風・自風には決してならないので `Some(false)`。風牌で相手の風が不明な場合だけ
// unknown にする。
fn matches_wind(tile: TileType, wind: Option<TileType>) -> Option<bool> {
    if !tile.is_wind() {
        return Some(false);
    }
    wind.map(|wind| wind == tile)
}

#[cfg(test)]
mod tests {
    use super::*;

    const EAST: u8 = 27;
    const SOUTH: u8 = 28;
    const WEST: u8 = 29;
    const HAKU: u8 = 31;

    fn tile(value: u8) -> TileId {
        TileId::new(value).unwrap()
    }

    fn honor(value: u8) -> TileType {
        TileType::new(value).unwrap()
    }

    fn honor_tiles(tile_type: u8, count: usize) -> Vec<TileId> {
        (0..count)
            .map(|copy| tile(tile_type * 4 + copy as u8))
            .collect()
    }

    fn chi() -> Meld {
        Meld::new(
            MeldKind::Chi,
            vec![tile(12), tile(16), tile(20)],
            Some(tile(12)),
        )
    }

    fn pon(tile_type: u8) -> Meld {
        let tiles = honor_tiles(tile_type, 3);
        let called_tile = tiles[0];
        Meld::new(MeldKind::Pon, tiles, Some(called_tile))
    }

    fn daiminkan(tile_type: u8) -> Meld {
        let tiles = honor_tiles(tile_type, 4);
        let called_tile = tiles[0];
        Meld::new(MeldKind::Daiminkan, tiles, Some(called_tile))
    }

    fn ankan(tile_type: u8) -> Meld {
        Meld::new(MeldKind::Ankan, honor_tiles(tile_type, 4), None)
    }

    fn kakan(tile_type: u8) -> Meld {
        let tiles = honor_tiles(tile_type, 4);
        let called_tile = tiles[0];
        Meld::new(MeldKind::Kakan, tiles, Some(called_tile))
    }

    fn context(melds: [Vec<Meld>; 4]) -> GameContext {
        context_with(None, None, None, vec![], [false; 4], melds)
    }

    fn context_with(
        player_id: Option<u8>,
        oya: Option<u8>,
        round_wind: Option<TileType>,
        dora_indicators: Vec<TileId>,
        reached: [bool; 4],
        melds: [Vec<Meld>; 4],
    ) -> GameContext {
        GameContext::from_parts_with_melds(
            None,
            vec![],
            dora_indicators,
            round_wind,
            None,
            Vec::new(),
            player_id,
            oya,
            Default::default(),
            reached,
            melds,
        )
    }

    fn threat_of(context: &GameContext, player: usize) -> PlayerThreatDiagnostic {
        diagnose_player_threats(context)[player].clone()
    }

    #[test]
    fn player_without_melds_or_reach_has_no_facts() {
        let threat = threat_of(&context(Default::default()), 1);
        assert_eq!(threat.player, 1);
        assert_eq!(threat.meld_count, 0);
        assert_eq!(threat.open_meld_count, 0);
        assert_eq!(threat.kan_count, 0);
        assert!(!threat.reached);
        assert!(threat.melds.is_empty());
        assert_eq!(threat.meld_kinds, MeldKindCounts::default());
        assert_eq!(threat.meld_dora_count, 0);
        assert_eq!(threat.meld_red_dora_count, 0);
    }

    #[test]
    fn chi_is_one_open_meld() {
        let threat = threat_of(&context([vec![], vec![chi()], vec![], vec![]]), 1);
        assert_eq!(threat.meld_count, 1);
        assert_eq!(threat.open_meld_count, 1);
        assert_eq!(threat.kan_count, 0);
        assert_eq!(threat.meld_kinds.chi, 1);
        assert_eq!(threat.melds[0].kind, MeldKind::Chi);
        assert!(threat.melds[0].is_open);
        assert!(!threat.melds[0].is_kan);
        assert_eq!(threat.melds[0].tiles, [tile(12), tile(16), tile(20)]);
    }

    #[test]
    fn pon_is_one_open_meld() {
        let threat = threat_of(&context([vec![], vec![pon(HAKU)], vec![], vec![]]), 1);
        assert_eq!(threat.meld_count, 1);
        assert_eq!(threat.open_meld_count, 1);
        assert_eq!(threat.kan_count, 0);
        assert_eq!(threat.meld_kinds.pon, 1);
        assert_eq!(threat.melds[0].kind, MeldKind::Pon);
        assert!(threat.melds[0].is_open);
    }

    #[test]
    fn daiminkan_is_an_open_kan() {
        let threat = threat_of(&context([vec![], vec![daiminkan(HAKU)], vec![], vec![]]), 1);
        assert_eq!(threat.meld_count, 1);
        assert_eq!(threat.open_meld_count, 1);
        assert_eq!(threat.kan_count, 1);
        assert_eq!(threat.meld_kinds.daiminkan, 1);
        assert!(threat.melds[0].is_open);
        assert!(threat.melds[0].is_kan);
    }

    #[test]
    fn ankan_is_a_kan_but_not_an_open_meld() {
        let threat = threat_of(&context([vec![], vec![ankan(HAKU)], vec![], vec![]]), 1);
        assert_eq!(threat.meld_count, 1);
        assert_eq!(threat.open_meld_count, 0);
        assert_eq!(threat.kan_count, 1);
        assert_eq!(threat.meld_kinds.ankan, 1);
        assert!(!threat.melds[0].is_open);
        assert!(threat.melds[0].is_kan);
    }

    #[test]
    fn kakan_is_an_open_kan() {
        let threat = threat_of(&context([vec![], vec![kakan(HAKU)], vec![], vec![]]), 1);
        assert_eq!(threat.meld_count, 1);
        assert_eq!(threat.open_meld_count, 1);
        assert_eq!(threat.kan_count, 1);
        assert_eq!(threat.meld_kinds.kakan, 1);
        assert!(threat.melds[0].is_open);
        assert!(threat.melds[0].is_kan);
    }

    #[test]
    fn multiple_melds_are_aggregated_by_kind_open_and_kan() {
        let melds = vec![chi(), pon(HAKU), ankan(EAST), kakan(SOUTH)];
        let threat = threat_of(&context([vec![], melds, vec![], vec![]]), 1);

        assert_eq!(threat.meld_count, 4);
        assert_eq!(threat.open_meld_count, 3);
        assert_eq!(threat.kan_count, 2);
        assert_eq!(
            threat.meld_kinds,
            MeldKindCounts {
                chi: 1,
                pon: 1,
                daiminkan: 0,
                ankan: 1,
                kakan: 1,
            }
        );
        assert_eq!(threat.meld_kinds.total(), threat.meld_count);
        assert_eq!(
            threat
                .melds
                .iter()
                .map(|meld| meld.kind)
                .collect::<Vec<_>>(),
            [
                MeldKind::Chi,
                MeldKind::Pon,
                MeldKind::Ankan,
                MeldKind::Kakan
            ]
        );
        assert_eq!(
            threat
                .melds
                .iter()
                .map(|meld| meld.is_open)
                .collect::<Vec<_>>(),
            [true, true, false, true]
        );
        assert_eq!(
            threat
                .melds
                .iter()
                .map(|meld| meld.is_kan)
                .collect::<Vec<_>>(),
            [false, false, true, true]
        );
    }

    #[test]
    fn meld_kind_counts_get_matches_each_kind() {
        let counts = MeldKindCounts::of(&[chi(), chi(), pon(HAKU), daiminkan(EAST), ankan(SOUTH)]);
        assert_eq!(counts.get(MeldKind::Chi), 2);
        assert_eq!(counts.get(MeldKind::Pon), 1);
        assert_eq!(counts.get(MeldKind::Daiminkan), 1);
        assert_eq!(counts.get(MeldKind::Ankan), 1);
        assert_eq!(counts.get(MeldKind::Kakan), 0);
        assert_eq!(counts.total(), 5);
    }

    #[test]
    fn meld_dora_matches_count_dora() {
        // 4m 表示 → 5m がドラ。Chi 4m 5m 6m の 5m は黒5 (tile 17)。
        let dora_indicators = vec![tile(12)];
        let meld = Meld::new(
            MeldKind::Chi,
            vec![tile(13), tile(17), tile(20)],
            Some(tile(13)),
        );
        let expected: u8 = meld
            .tiles()
            .iter()
            .map(|&tile| count_dora(tile, &dora_indicators))
            .sum();

        let context = context_with(
            None,
            None,
            None,
            dora_indicators,
            [false; 4],
            [vec![], vec![meld], vec![], vec![]],
        );
        let threat = threat_of(&context, 1);

        assert_eq!(threat.melds[0].dora_count, expected);
        assert_eq!(threat.melds[0].dora_count, 1);
        assert_eq!(threat.melds[0].red_dora_count, 0);
        assert_eq!(threat.meld_dora_count, expected);
    }

    #[test]
    fn red_five_is_counted_as_red_dora() {
        // 赤5m (tile 16) を含む Chi。表示牌が無いので通常ドラは赤5の分だけ。
        let context = context([vec![], vec![chi()], vec![], vec![]]);
        let threat = threat_of(&context, 1);

        assert_eq!(threat.melds[0].red_dora_count, 1);
        assert_eq!(threat.melds[0].dora_count, 1);
        assert_eq!(threat.meld_red_dora_count, 1);
        assert_eq!(threat.meld_dora_count, 1);
    }

    #[test]
    fn red_five_that_is_also_an_indicated_dora_keeps_both_facts() {
        // 4m 表示で 5m がドラ、その 5m が赤5 (tile 16)。count_dora の semantics どおり2枚分。
        let dora_indicators = vec![tile(12)];
        let expected = count_dora(tile(16), &dora_indicators);
        let context = context_with(
            None,
            None,
            None,
            dora_indicators,
            [false; 4],
            [vec![], vec![chi()], vec![], vec![]],
        );
        let threat = threat_of(&context, 1);

        assert_eq!(expected, 2);
        assert_eq!(threat.melds[0].dora_count, 2);
        assert_eq!(threat.melds[0].red_dora_count, 1);
        assert_eq!(threat.meld_dora_count, 2);
        assert_eq!(threat.meld_red_dora_count, 1);
    }

    #[test]
    fn dragon_pon_is_diagnosed_as_dragon() {
        let context = context_with(
            None,
            Some(0),
            Some(honor(EAST)),
            vec![],
            [false; 4],
            [vec![], vec![pon(HAKU)], vec![], vec![]],
        );
        let value_honor = threat_of(&context, 1).melds[0].value_honor.unwrap();

        assert_eq!(value_honor.tile, honor(HAKU));
        assert!(value_honor.is_dragon);
        assert_eq!(value_honor.is_round_wind, Some(false));
        assert_eq!(value_honor.is_seat_wind, Some(false));
    }

    #[test]
    fn round_wind_pon_is_diagnosed_as_round_wind() {
        // 東場で player 1 の自風は南。東の Pon は場風だけに一致する。
        let context = context_with(
            None,
            Some(0),
            Some(honor(EAST)),
            vec![],
            [false; 4],
            [vec![], vec![pon(EAST)], vec![], vec![]],
        );
        let threat = threat_of(&context, 1);
        let value_honor = threat.melds[0].value_honor.unwrap();

        assert_eq!(threat.seat_wind, Some(honor(SOUTH)));
        assert!(!value_honor.is_dragon);
        assert_eq!(value_honor.is_round_wind, Some(true));
        assert_eq!(value_honor.is_seat_wind, Some(false));
    }

    #[test]
    fn seat_wind_pon_is_diagnosed_from_the_opponent_seat() {
        // 東場・親 player 0 なら player 1 の自風は南。
        let context = context_with(
            None,
            Some(0),
            Some(honor(EAST)),
            vec![],
            [false; 4],
            [vec![], vec![pon(SOUTH)], vec![], vec![]],
        );
        let threat = threat_of(&context, 1);
        let value_honor = threat.melds[0].value_honor.unwrap();

        assert_eq!(threat.seat_wind, Some(honor(SOUTH)));
        assert_eq!(value_honor.is_round_wind, Some(false));
        assert_eq!(value_honor.is_seat_wind, Some(true));
    }

    #[test]
    fn double_wind_pon_and_kan_keep_both_facts() {
        // 南場で親が player 3 なら player 0 の自風は南。ダブ南。
        for meld in [pon(SOUTH), daiminkan(SOUTH), kakan(SOUTH), ankan(SOUTH)] {
            let context = context_with(
                None,
                Some(3),
                Some(honor(SOUTH)),
                vec![],
                [false; 4],
                [vec![meld.clone()], vec![], vec![], vec![]],
            );
            let threat = threat_of(&context, 0);
            let value_honor = threat.melds[0].value_honor.unwrap();

            assert_eq!(threat.seat_wind, Some(honor(SOUTH)));
            assert_eq!(value_honor.is_round_wind, Some(true), "{meld:?}");
            assert_eq!(value_honor.is_seat_wind, Some(true), "{meld:?}");
        }
    }

    #[test]
    fn unknown_oya_leaves_the_opponent_seat_wind_unknown() {
        let context = context_with(
            None,
            None,
            Some(honor(EAST)),
            vec![],
            [false; 4],
            [vec![], vec![pon(WEST)], vec![], vec![]],
        );
        let threat = threat_of(&context, 1);
        let value_honor = threat.melds[0].value_honor.unwrap();

        assert_eq!(threat.seat_wind, None);
        assert_eq!(threat.is_dealer, None);
        assert_eq!(value_honor.is_round_wind, Some(false));
        assert_eq!(value_honor.is_seat_wind, None);
    }

    #[test]
    fn unknown_round_wind_leaves_the_round_wind_fact_unknown() {
        let context = context_with(
            None,
            Some(0),
            None,
            vec![],
            [false; 4],
            [vec![], vec![pon(SOUTH)], vec![], vec![]],
        );
        let value_honor = threat_of(&context, 1).melds[0].value_honor.unwrap();

        assert_eq!(value_honor.is_round_wind, None);
        assert_eq!(value_honor.is_seat_wind, Some(true));
    }

    #[test]
    fn suited_and_chi_melds_have_no_value_honor_diagnostic() {
        let suited_pon = Meld::new(
            MeldKind::Pon,
            vec![tile(0), tile(1), tile(2)],
            Some(tile(0)),
        );
        let context = context_with(
            None,
            Some(0),
            Some(honor(EAST)),
            vec![],
            [false; 4],
            [vec![], vec![chi(), suited_pon], vec![], vec![]],
        );
        let threat = threat_of(&context, 1);

        assert_eq!(threat.melds[0].value_honor, None);
        assert_eq!(threat.melds[1].value_honor, None);
    }

    #[test]
    fn known_player_id_separates_self_from_opponents() {
        let context = context_with(
            Some(2),
            Some(0),
            None,
            vec![],
            [false; 4],
            Default::default(),
        );
        let threats = diagnose_player_threats(&context);

        assert_eq!(threats[2].is_self, Some(true));
        assert_eq!(threats[2].is_opponent(), Some(false));
        for player in [0, 1, 3] {
            assert_eq!(threats[player].is_self, Some(false));
            assert_eq!(threats[player].is_opponent(), Some(true));
        }
        assert_eq!(threats[0].is_dealer, Some(true));
        assert_eq!(threats[1].is_dealer, Some(false));
    }

    #[test]
    fn unknown_player_id_does_not_guess_the_self_seat() {
        let context = context([vec![pon(HAKU)], vec![], vec![], vec![]]);
        let threats = diagnose_player_threats(&context);

        assert_eq!(threats.len(), 4);
        for (player, threat) in threats.iter().enumerate() {
            assert_eq!(threat.player, player);
            assert_eq!(threat.is_self, None);
            assert_eq!(threat.is_opponent(), None);
        }
        assert_eq!(threats[0].meld_count, 1);
    }

    #[test]
    fn reached_player_keeps_both_reach_and_meld_facts() {
        let context = context_with(
            Some(0),
            Some(0),
            Some(honor(EAST)),
            vec![],
            [false, true, false, false],
            [vec![], vec![pon(HAKU), chi()], vec![], vec![]],
        );
        let threat = threat_of(&context, 1);

        assert!(threat.reached);
        assert_eq!(threat.meld_count, 2);
        assert_eq!(threat.open_meld_count, 2);
        assert!(threat.melds[0].value_honor.unwrap().is_dragon);
    }

    #[test]
    fn diagnostics_cover_every_seat_in_order() {
        let context = context([vec![], vec![chi()], vec![], vec![ankan(HAKU)]]);
        let threats = diagnose_player_threats(&context);

        assert_eq!(
            threats
                .iter()
                .map(|threat| (threat.player, threat.meld_count))
                .collect::<Vec<_>>(),
            [(0, 0), (1, 1), (2, 0), (3, 1)]
        );
    }

    #[test]
    fn pure_helper_matches_the_context_adapter() {
        let context = context_with(
            Some(0),
            Some(1),
            Some(honor(EAST)),
            vec![tile(12)],
            [false, true, false, false],
            [vec![], vec![chi(), pon(HAKU)], vec![], vec![]],
        );
        let melds = context.melds_of(1).unwrap();
        let inputs = PlayerThreatInputs {
            player: 1,
            is_self: Some(false),
            is_dealer: Some(true),
            reached: true,
            round_wind: Some(honor(EAST)),
            seat_wind: Some(honor(EAST)),
            melds,
            dora_indicators: context.dora_indicators(),
        };

        assert_eq!(diagnose_player_threat(inputs), threat_of(&context, 1));
    }
}
