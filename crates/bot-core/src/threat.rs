use bot_logic::{TileId, TileType, count_dora};

use crate::context::GameContext;
use crate::meld::{Meld, MeldKind};
use crate::open_hand_threat::{OpenHandThreatAssessment, classify_open_hand_threat};

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
            counts.add(meld.kind());
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

    /// 全 [`MeldKind`] の合計。`PlayerThreatFacts::meld_count` と一致する。
    pub fn total(self) -> usize {
        self.chi + self.pon + self.daiminkan + self.ankan + self.kakan
    }

    fn add(&mut self, kind: MeldKind) {
        *self.get_mut(kind) += 1;
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
pub struct ValueHonorMeldFacts {
    pub tile: TileType,
    pub is_dragon: bool,
    pub is_round_wind: Option<bool>,
    pub is_seat_wind: Option<bool>,
}

impl ValueHonorMeldFacts {
    /// 役牌と確定しているか。三元牌、または場風・自風との一致が確定した風牌。
    pub fn is_confirmed(self) -> bool {
        self.is_dragon || self.is_round_wind == Some(true) || self.is_seat_wind == Some(true)
    }

    /// 場風または自風が不明で、役牌かどうかを確定できない風牌か。
    ///
    /// 情報不足を「役牌ではない」と確定させないための区別で、[`Self::is_confirmed`] とは
    /// 排他になる。
    pub fn is_unconfirmed_wind(self) -> bool {
        !self.is_confirmed() && (self.is_round_wind.is_none() || self.is_seat_wind.is_none())
    }
}

/// player 1人分の役牌副露の集計。翻数へは潰さず、unknown を `false` と確定させない。
///
/// `dragon` / `round_wind` / `seat_wind` は軸ごとの面子数で、ダブ風は `round_wind` と
/// `seat_wind` の両方に数える。何面子が役牌かを見たい場合は `confirmed` を使う。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ValueHonorMeldCounts {
    /// 三元牌の刻子・槓子の数。風情報が無くても確定する。
    pub dragon: usize,
    /// 場風と確定した風牌の刻子・槓子の数。
    pub round_wind: usize,
    /// 自風と確定した風牌の刻子・槓子の数。
    pub seat_wind: usize,
    /// 役牌と確定した面子の数。ダブ風でも1面子として一度だけ数える。
    pub confirmed: usize,
    /// 場風・自風が不明で、役牌かどうかを確定できない風牌の刻子・槓子の数。
    /// `confirmed` とは重複しない。
    pub unconfirmed_wind: usize,
}

impl ValueHonorMeldCounts {
    /// 公開情報から確定している役牌の翻数。ダブ風は場風・自風の2翻として数え、
    /// `unconfirmed_wind` は推測して加算しない。
    pub fn confirmed_han(self) -> usize {
        self.dragon + self.round_wind + self.seat_wind
    }

    fn add(&mut self, facts: ValueHonorMeldFacts) {
        self.dragon += usize::from(facts.is_dragon);
        self.round_wind += usize::from(facts.is_round_wind == Some(true));
        self.seat_wind += usize::from(facts.is_seat_wind == Some(true));
        self.confirmed += usize::from(facts.is_confirmed());
        self.unconfirmed_wind += usize::from(facts.is_unconfirmed_wind());
    }
}

/// fixed meld 1つ分の軽量な観測事実。allocation を持たず `Copy`。
///
/// [`MeldThreatDiagnostic`] はこの facts に物理牌を足したもので、副露の種類・ドラ・役牌の判定は
/// production と診断で必ずこの型を経由する。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MeldThreatFacts {
    pub kind: MeldKind,
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
    pub value_honor: Option<ValueHonorMeldFacts>,
}

/// player 1人分の軽量な観測事実。allocation を持たず `Copy`。
///
/// 押し引きが参照できる threat の source of truth で、[`PlayerThreatDiagnostic`] はこの facts に
/// meld ごとの詳細を足したもの。副露数やドラ枚数からテンパイ・向聴数を推測しない。ここにあるのは
/// 観測できた事実だけで、そこから threat level を決める policy は呼び出し側の責務。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlayerThreatFacts {
    pub player: usize,
    /// 自分の席か。`player_id` が不明なら推測せず `None` (unknown)。
    pub is_self: Option<bool>,
    /// 親の席か。`oya` が不明なら推測せず `None` (unknown)。
    pub is_dealer: Option<bool>,
    pub reached: bool,
    /// この player の自風。`oya` から導出できない場合は推測せず `None`。
    pub seat_wind: Option<TileType>,
    /// この player が既に河へ切った牌の枚数。
    /// [`GameContext::discards_of`] の長さがそのまま source of truth で、UI 上の「河2段目」
    /// のような表現からは導出しない。
    pub discard_count: usize,
    /// Chi / Pon / Daiminkan / Ankan / Kakan を含む fixed meld の総数。
    pub meld_count: usize,
    /// `meld_count` のうち [`MeldKind::is_open`] が `true` のものだけ。Ankan は含まない。
    pub open_meld_count: usize,
    /// `meld_count` のうち [`MeldKind::is_kan`] が `true` のものだけ。Ankan を含む。
    pub kan_count: usize,
    pub meld_kinds: MeldKindCounts,
    /// 全 fixed meld の `dora_count` 合計。Ankan の分も含む。公開分だけを見たい policy は
    /// `open_meld_dora_count` を使う。
    pub meld_dora_count: u8,
    /// 全 fixed meld の `red_dora_count` 合計。`meld_dora_count` の内数。
    pub meld_red_dora_count: u8,
    /// 役牌副露の集計。Ankan を含む。翻数へは潰さず、情報不足も `unconfirmed_wind` として残す。
    pub value_honor_melds: ValueHonorMeldCounts,
    /// `is_open` な meld だけの `dora_count` 合計。Ankan は含まない。
    /// 判定は `meld_dora_count` と同じ meld ごとの facts の集計で、ドラを数え直さない。
    pub open_meld_dora_count: u8,
    /// `is_open` な meld だけの `red_dora_count` 合計。`open_meld_dora_count` の内数。
    pub open_meld_red_dora_count: u8,
    /// `is_open` な meld だけの役牌副露の集計。Ankan は含まない。
    pub open_value_honor_melds: ValueHonorMeldCounts,
}

impl PlayerThreatFacts {
    /// 公開副露だけから確定して確認できる翻数の下限 proxy。
    ///
    /// 確定役牌翻と、既存の open meld 内ドラ枚数を合計する。一般役の推定は行わず、
    /// unknown wind と Ankan は open facts の既存 semantics により含まれない。
    pub fn open_visible_han_proxy(&self) -> usize {
        self.open_value_honor_melds.confirmed_han() + usize::from(self.open_meld_dora_count)
    }

    /// 他家の席か。`player_id` が不明なら推測せず `None` (unknown)。
    pub fn is_opponent(&self) -> Option<bool> {
        self.is_self.map(|is_self| !is_self)
    }

    /// 他家リーチとして数える席か。
    ///
    /// [`GameContext::reached_opponents`] と同じ semantics で、`is_self` が unknown の席は
    /// 自分と断定せずリーチ者として数える。
    pub fn is_reached_opponent(&self) -> bool {
        self.reached && self.is_self != Some(true)
    }
}

/// fixed meld 1つ分の観測事実と、その meld の物理牌。危険度の判断は持たない。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MeldThreatDiagnostic {
    /// production と共有する軽量な観測事実。
    pub facts: MeldThreatFacts,
    /// meld を構成する物理牌。Kakan は加槓牌を含む4枚。診断・表示のためだけに持つ詳細。
    pub tiles: Vec<TileId>,
}

/// player 1人分の観測事実と、meld ごとの詳細。
///
/// 集計値は [`PlayerThreatFacts`] そのもので、診断のために数え直さない。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlayerThreatDiagnostic {
    /// production と共有する軽量な観測事実。
    pub facts: PlayerThreatFacts,
    /// fixed meld ごとの観測事実。`melds` の順序は `GameContext` の順序そのまま。
    pub melds: Vec<MeldThreatDiagnostic>,
    /// `facts` だけから求めた非リーチ副露相手の暫定 classification
    /// ([`classify_open_hand_threat`])。observed facts とは分けて持ち、`GameContext` を
    /// 解析し直さない。押し引き・防御にはまだ使わない診断専用の情報。
    pub open_hand_threat: OpenHandThreatAssessment,
}

/// [`player_threat_facts`] / [`diagnose_player_threat`] の入力。`GameContext` から取り出した
/// 観測事実だけを持つ。
///
/// `GameContext` からデータを取り出す adapter ([`player_threat_inputs`]) と、そこから facts を
/// 組み立てる pure な logic を分けるための型。ここで不足情報を補完しない。
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
    /// 対象 player の河。局進行は枚数だけを facts に載せる。
    pub discards: &'a [TileId],
    pub dora_indicators: &'a [TileId],
}

/// fixed meld 1つ分の軽量な観測事実を作る pure helper。
///
/// ドラ判定は既存の [`count_dora`] と [`TileId::is_red`] をそのまま使い、別の判定器を作らない。
/// `seat_wind` は meld を持つ player 自身の自風。
pub fn meld_threat_facts(
    meld: &Meld,
    dora_indicators: &[TileId],
    round_wind: Option<TileType>,
    seat_wind: Option<TileType>,
) -> MeldThreatFacts {
    let mut dora_count = 0u8;
    let mut red_dora_count = 0u8;
    for &tile in meld.tiles() {
        dora_count = dora_count.saturating_add(count_dora(tile, dora_indicators));
        if tile.is_red() {
            red_dora_count = red_dora_count.saturating_add(1);
        }
    }

    MeldThreatFacts {
        kind: meld.kind(),
        is_open: meld.is_open(),
        is_kan: meld.kind().is_kan(),
        dora_count,
        red_dora_count,
        value_honor: value_honor_meld_facts(meld, round_wind, seat_wind),
    }
}

/// fixed meld 1つ分の診断を作る pure helper。
///
/// 判定は [`meld_threat_facts`] のものをそのまま使い、診断のために物理牌を足すだけ。
pub fn diagnose_meld_threat(
    meld: &Meld,
    dora_indicators: &[TileId],
    round_wind: Option<TileType>,
    seat_wind: Option<TileType>,
) -> MeldThreatDiagnostic {
    MeldThreatDiagnostic {
        facts: meld_threat_facts(meld, dora_indicators, round_wind, seat_wind),
        tiles: meld.tiles().to_vec(),
    }
}

/// player 1人分の軽量な観測事実を作る pure helper。向聴数・受け入れ・待ちの再計算は行わない。
///
/// meld ごとの物理牌を持たないため allocation を行わない。
pub fn player_threat_facts(inputs: PlayerThreatInputs<'_>) -> PlayerThreatFacts {
    aggregate_player_threat_facts(
        inputs,
        inputs.melds.iter().map(|meld| {
            meld_threat_facts(
                meld,
                inputs.dora_indicators,
                inputs.round_wind,
                inputs.seat_wind,
            )
        }),
    )
}

/// player 1人分の診断を作る pure helper。
///
/// 集計値は [`player_threat_facts`] と同じ pure logic で求め、診断のために meld ごとの詳細を
/// 追加する。
pub fn diagnose_player_threat(inputs: PlayerThreatInputs<'_>) -> PlayerThreatDiagnostic {
    let melds = meld_threat_diagnostics(inputs);
    let facts = aggregate_player_threat_facts(inputs, melds.iter().map(|meld| meld.facts));
    PlayerThreatDiagnostic {
        facts,
        melds,
        open_hand_threat: classify_open_hand_threat(facts),
    }
}

/// 構築済みの [`PlayerThreatFacts`] へ meld ごとの詳細を足して診断にする pure helper。
///
/// production で使った facts をそのまま診断へ載せるための入口で、集計値を作り直さない。
/// `facts` は同じ `inputs` から作られたものであることを前提にする。
pub fn diagnose_player_threat_with_facts(
    facts: PlayerThreatFacts,
    inputs: PlayerThreatInputs<'_>,
) -> PlayerThreatDiagnostic {
    PlayerThreatDiagnostic {
        facts,
        melds: meld_threat_diagnostics(inputs),
        open_hand_threat: classify_open_hand_threat(facts),
    }
}

/// `GameContext` から指定 player の入力を取り出す adapter。
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
        discards: context.discards_of(player).unwrap_or_default(),
        dora_indicators: context.dora_indicators(),
    }
}

/// `GameContext` から全4席分の軽量な観測事実を作る adapter。
///
/// 通常の `act()` 経路が使う入口で、meld ごとの `Vec` を作らない。`player_id` が不明でもどの席も
/// 除外せず、常に4席分を返す。自分と他家の区別は各 facts の `is_self` / `is_opponent()` が
/// unknown で表す。
pub fn player_threat_facts_from_context(context: &GameContext) -> [PlayerThreatFacts; 4] {
    std::array::from_fn(|player| player_threat_facts(player_threat_inputs(context, player)))
}

/// `GameContext` から全4席分の診断を作る adapter。
pub fn diagnose_player_threats(context: &GameContext) -> [PlayerThreatDiagnostic; 4] {
    std::array::from_fn(|player| diagnose_player_threat(player_threat_inputs(context, player)))
}

/// 構築済みの facts から全4席分の診断を作る adapter。
///
/// 集計値を作り直さないので、production で押し引きへ渡した facts と診断の集計値が必ず一致する。
/// `facts` は同じ `context` から作られたものであることを前提にする。
pub fn diagnose_player_threats_with_facts(
    context: &GameContext,
    facts: &[PlayerThreatFacts; 4],
) -> [PlayerThreatDiagnostic; 4] {
    std::array::from_fn(|player| {
        diagnose_player_threat_with_facts(facts[player], player_threat_inputs(context, player))
    })
}

/// 全4席分の facts から他家リーチ者数を数える。
///
/// [`GameContext::reached_opponents`] と同じ semantics で、`player_id` が不明な場合はリーチ
/// フラグが立っている全席を数える。席数は4固定なので `u8` に収まる。
pub fn reached_opponent_count(facts: &[PlayerThreatFacts; 4]) -> u8 {
    facts
        .iter()
        .filter(|facts| facts.is_reached_opponent())
        .count() as u8
}

/// 全4席分の facts で、他家リーチ者に親が含まれるか。
///
/// `oya` が不明な席は `is_dealer` が unknown なので、親リーチと確定させない。
pub fn has_reached_dealer(facts: &[PlayerThreatFacts; 4]) -> bool {
    facts
        .iter()
        .any(|facts| facts.is_reached_opponent() && facts.is_dealer == Some(true))
}

// meld ごとの診断を作る。判定は meld_threat_facts に一本化し、ここでは物理牌を足すだけ。
fn meld_threat_diagnostics(inputs: PlayerThreatInputs<'_>) -> Vec<MeldThreatDiagnostic> {
    inputs
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
        .collect()
}

// meld ごとの facts から player 1人分の集計を作る。facts 経路と診断経路はこの1本を共有する。
fn aggregate_player_threat_facts(
    inputs: PlayerThreatInputs<'_>,
    melds: impl Iterator<Item = MeldThreatFacts>,
) -> PlayerThreatFacts {
    let mut facts = PlayerThreatFacts {
        player: inputs.player,
        is_self: inputs.is_self,
        is_dealer: inputs.is_dealer,
        reached: inputs.reached,
        seat_wind: inputs.seat_wind,
        discard_count: inputs.discards.len(),
        meld_count: 0,
        open_meld_count: 0,
        kan_count: 0,
        meld_kinds: MeldKindCounts::default(),
        meld_dora_count: 0,
        meld_red_dora_count: 0,
        value_honor_melds: ValueHonorMeldCounts::default(),
        open_meld_dora_count: 0,
        open_meld_red_dora_count: 0,
        open_value_honor_melds: ValueHonorMeldCounts::default(),
    };

    for meld in melds {
        facts.meld_count += 1;
        facts.open_meld_count += usize::from(meld.is_open);
        facts.kan_count += usize::from(meld.is_kan);
        facts.meld_kinds.add(meld.kind);
        facts.meld_dora_count = facts.meld_dora_count.saturating_add(meld.dora_count);
        facts.meld_red_dora_count = facts
            .meld_red_dora_count
            .saturating_add(meld.red_dora_count);
        if let Some(value_honor) = meld.value_honor {
            facts.value_honor_melds.add(value_honor);
        }

        // 公開されていない Ankan は open hand の威圧材料として数えない。判定は meld ごとの
        // facts をそのまま使い、ドラ・赤ドラ・役牌を別実装で数え直さない。
        if !meld.is_open {
            continue;
        }
        facts.open_meld_dora_count = facts.open_meld_dora_count.saturating_add(meld.dora_count);
        facts.open_meld_red_dora_count = facts
            .open_meld_red_dora_count
            .saturating_add(meld.red_dora_count);
        if let Some(value_honor) = meld.value_honor {
            facts.open_value_honor_melds.add(value_honor);
        }
    }

    facts
}

// 刻子・槓子の牌種から役牌 facts を作る。Chi は牌種が揃わないので対象外。
fn value_honor_meld_facts(
    meld: &Meld,
    round_wind: Option<TileType>,
    seat_wind: Option<TileType>,
) -> Option<ValueHonorMeldFacts> {
    if matches!(meld.kind(), MeldKind::Chi) {
        return None;
    }
    let tile = meld.tiles().first()?.tile_type();
    if !tile.is_honor() {
        return None;
    }

    Some(ValueHonorMeldFacts {
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
        context_with_discards(
            player_id,
            oya,
            round_wind,
            dora_indicators,
            reached,
            Default::default(),
            melds,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn context_with_discards(
        player_id: Option<u8>,
        oya: Option<u8>,
        round_wind: Option<TileType>,
        dora_indicators: Vec<TileId>,
        reached: [bool; 4],
        discards: [Vec<TileId>; 4],
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
            discards,
            reached,
            melds,
        )
    }

    // 河を持つ context。牌種は問わないので、指定枚数だけ別の物理牌を並べる。
    fn river(count: usize) -> Vec<TileId> {
        (0..count).map(|index| tile(index as u8)).collect()
    }

    // 4席すべてに別の副露・リーチ・ドラ・河を持たせた context。facts と診断の一致確認に使う。
    fn mixed_threat_context() -> GameContext {
        context_with_discards(
            Some(0),
            Some(1),
            Some(honor(EAST)),
            vec![tile(12)],
            [false, true, false, false],
            [river(0), river(3), river(9), river(12)],
            [
                vec![pon(HAKU)],
                vec![chi(), kakan(SOUTH)],
                vec![ankan(WEST)],
                vec![daiminkan(EAST), chi(), pon(SOUTH)],
            ],
        )
    }

    fn threat_of(context: &GameContext, player: usize) -> PlayerThreatDiagnostic {
        diagnose_player_threats(context)[player].clone()
    }

    fn facts_of(context: &GameContext, player: usize) -> PlayerThreatFacts {
        player_threat_facts_from_context(context)[player]
    }

    #[test]
    fn player_without_melds_or_reach_has_no_facts() {
        let threat = threat_of(&context(Default::default()), 1);
        assert_eq!(threat.facts.player, 1);
        assert_eq!(threat.facts.meld_count, 0);
        assert_eq!(threat.facts.open_meld_count, 0);
        assert_eq!(threat.facts.kan_count, 0);
        assert!(!threat.facts.reached);
        assert!(threat.melds.is_empty());
        assert_eq!(threat.facts.meld_kinds, MeldKindCounts::default());
        assert_eq!(threat.facts.meld_dora_count, 0);
        assert_eq!(threat.facts.meld_red_dora_count, 0);
        assert_eq!(
            threat.facts.value_honor_melds,
            ValueHonorMeldCounts::default()
        );
    }

    #[test]
    fn chi_is_one_open_meld() {
        let context = context([vec![], vec![chi()], vec![], vec![]]);
        let facts = facts_of(&context, 1);
        assert_eq!(facts.meld_count, 1);
        assert_eq!(facts.open_meld_count, 1);
        assert_eq!(facts.kan_count, 0);
        assert_eq!(facts.meld_kinds.chi, 1);

        let threat = threat_of(&context, 1);
        assert_eq!(threat.melds[0].facts.kind, MeldKind::Chi);
        assert!(threat.melds[0].facts.is_open);
        assert!(!threat.melds[0].facts.is_kan);
        assert_eq!(threat.melds[0].tiles, [tile(12), tile(16), tile(20)]);
    }

    #[test]
    fn pon_is_one_open_meld() {
        let context = context([vec![], vec![pon(HAKU)], vec![], vec![]]);
        let facts = facts_of(&context, 1);
        assert_eq!(facts.meld_count, 1);
        assert_eq!(facts.open_meld_count, 1);
        assert_eq!(facts.kan_count, 0);
        assert_eq!(facts.meld_kinds.pon, 1);

        let threat = threat_of(&context, 1);
        assert_eq!(threat.melds[0].facts.kind, MeldKind::Pon);
        assert!(threat.melds[0].facts.is_open);
    }

    #[test]
    fn daiminkan_is_an_open_kan() {
        let context = context([vec![], vec![daiminkan(HAKU)], vec![], vec![]]);
        let facts = facts_of(&context, 1);
        assert_eq!(facts.meld_count, 1);
        assert_eq!(facts.open_meld_count, 1);
        assert_eq!(facts.kan_count, 1);
        assert_eq!(facts.meld_kinds.daiminkan, 1);

        let threat = threat_of(&context, 1);
        assert!(threat.melds[0].facts.is_open);
        assert!(threat.melds[0].facts.is_kan);
    }

    #[test]
    fn ankan_is_a_kan_but_not_an_open_meld() {
        let context = context([vec![], vec![ankan(HAKU)], vec![], vec![]]);
        let facts = facts_of(&context, 1);
        assert_eq!(facts.meld_count, 1);
        assert_eq!(facts.open_meld_count, 0);
        assert_eq!(facts.kan_count, 1);
        assert_eq!(facts.meld_kinds.ankan, 1);

        let threat = threat_of(&context, 1);
        assert!(!threat.melds[0].facts.is_open);
        assert!(threat.melds[0].facts.is_kan);
    }

    #[test]
    fn kakan_is_an_open_kan() {
        let context = context([vec![], vec![kakan(HAKU)], vec![], vec![]]);
        let facts = facts_of(&context, 1);
        assert_eq!(facts.meld_count, 1);
        assert_eq!(facts.open_meld_count, 1);
        assert_eq!(facts.kan_count, 1);
        assert_eq!(facts.meld_kinds.kakan, 1);

        let threat = threat_of(&context, 1);
        assert!(threat.melds[0].facts.is_open);
        assert!(threat.melds[0].facts.is_kan);
    }

    #[test]
    fn multiple_melds_are_aggregated_by_kind_open_and_kan() {
        let melds = vec![chi(), pon(HAKU), ankan(EAST), kakan(SOUTH)];
        let context = context([vec![], melds, vec![], vec![]]);
        let facts = facts_of(&context, 1);

        assert_eq!(facts.meld_count, 4);
        assert_eq!(facts.open_meld_count, 3);
        assert_eq!(facts.kan_count, 2);
        assert_eq!(
            facts.meld_kinds,
            MeldKindCounts {
                chi: 1,
                pon: 1,
                daiminkan: 0,
                ankan: 1,
                kakan: 1,
            }
        );
        assert_eq!(facts.meld_kinds.total(), facts.meld_count);

        let threat = threat_of(&context, 1);
        assert_eq!(
            threat
                .melds
                .iter()
                .map(|meld| meld.facts.kind)
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
                .map(|meld| meld.facts.is_open)
                .collect::<Vec<_>>(),
            [true, true, false, true]
        );
        assert_eq!(
            threat
                .melds
                .iter()
                .map(|meld| meld.facts.is_kan)
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

        assert_eq!(threat.melds[0].facts.dora_count, expected);
        assert_eq!(threat.melds[0].facts.dora_count, 1);
        assert_eq!(threat.melds[0].facts.red_dora_count, 0);
        assert_eq!(threat.facts.meld_dora_count, expected);
        assert_eq!(facts_of(&context, 1).meld_dora_count, expected);
    }

    #[test]
    fn red_five_is_counted_as_red_dora() {
        // 赤5m (tile 16) を含む Chi。表示牌が無いので通常ドラは赤5の分だけ。
        let context = context([vec![], vec![chi()], vec![], vec![]]);
        let threat = threat_of(&context, 1);

        assert_eq!(threat.melds[0].facts.red_dora_count, 1);
        assert_eq!(threat.melds[0].facts.dora_count, 1);
        assert_eq!(threat.facts.meld_red_dora_count, 1);
        assert_eq!(threat.facts.meld_dora_count, 1);
        assert_eq!(facts_of(&context, 1).meld_red_dora_count, 1);
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
        assert_eq!(threat.melds[0].facts.dora_count, 2);
        assert_eq!(threat.melds[0].facts.red_dora_count, 1);
        assert_eq!(threat.facts.meld_dora_count, 2);
        assert_eq!(threat.facts.meld_red_dora_count, 1);
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
        let value_honor = threat_of(&context, 1).melds[0].facts.value_honor.unwrap();

        assert_eq!(value_honor.tile, honor(HAKU));
        assert!(value_honor.is_dragon);
        assert_eq!(value_honor.is_round_wind, Some(false));
        assert_eq!(value_honor.is_seat_wind, Some(false));
        assert!(value_honor.is_confirmed());
        assert!(!value_honor.is_unconfirmed_wind());

        assert_eq!(
            facts_of(&context, 1).value_honor_melds,
            ValueHonorMeldCounts {
                dragon: 1,
                round_wind: 0,
                seat_wind: 0,
                confirmed: 1,
                unconfirmed_wind: 0,
            }
        );
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
        let value_honor = threat.melds[0].facts.value_honor.unwrap();

        assert_eq!(threat.facts.seat_wind, Some(honor(SOUTH)));
        assert!(!value_honor.is_dragon);
        assert_eq!(value_honor.is_round_wind, Some(true));
        assert_eq!(value_honor.is_seat_wind, Some(false));

        assert_eq!(
            facts_of(&context, 1).value_honor_melds,
            ValueHonorMeldCounts {
                dragon: 0,
                round_wind: 1,
                seat_wind: 0,
                confirmed: 1,
                unconfirmed_wind: 0,
            }
        );
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
        let value_honor = threat.melds[0].facts.value_honor.unwrap();

        assert_eq!(threat.facts.seat_wind, Some(honor(SOUTH)));
        assert_eq!(value_honor.is_round_wind, Some(false));
        assert_eq!(value_honor.is_seat_wind, Some(true));

        assert_eq!(
            facts_of(&context, 1).value_honor_melds,
            ValueHonorMeldCounts {
                dragon: 0,
                round_wind: 0,
                seat_wind: 1,
                confirmed: 1,
                unconfirmed_wind: 0,
            }
        );
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
            let value_honor = threat.melds[0].facts.value_honor.unwrap();

            assert_eq!(threat.facts.seat_wind, Some(honor(SOUTH)));
            assert_eq!(value_honor.is_round_wind, Some(true), "{meld:?}");
            assert_eq!(value_honor.is_seat_wind, Some(true), "{meld:?}");

            // ダブ風でも confirmed は1面子だが、確定翻の派生値では場風・自風の2翻になる。
            let facts = facts_of(&context, 0);
            assert_eq!(
                facts.value_honor_melds,
                ValueHonorMeldCounts {
                    dragon: 0,
                    round_wind: 1,
                    seat_wind: 1,
                    confirmed: 1,
                    unconfirmed_wind: 0,
                },
                "{meld:?}"
            );
            assert_eq!(facts.value_honor_melds.confirmed_han(), 2, "{meld:?}");
            assert_eq!(
                facts.open_visible_han_proxy(),
                if meld.kind().is_open() { 2 } else { 0 },
                "{meld:?}"
            );
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
        let value_honor = threat.melds[0].facts.value_honor.unwrap();

        assert_eq!(threat.facts.seat_wind, None);
        assert_eq!(threat.facts.is_dealer, None);
        assert_eq!(value_honor.is_round_wind, Some(false));
        assert_eq!(value_honor.is_seat_wind, None);
        assert!(!value_honor.is_confirmed());
        assert!(value_honor.is_unconfirmed_wind());

        // 情報不足を「役牌ではない」と確定させず、unknown として残す。
        assert_eq!(
            facts_of(&context, 1).value_honor_melds,
            ValueHonorMeldCounts {
                dragon: 0,
                round_wind: 0,
                seat_wind: 0,
                confirmed: 0,
                unconfirmed_wind: 1,
            }
        );
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
        let value_honor = threat_of(&context, 1).melds[0].facts.value_honor.unwrap();

        assert_eq!(value_honor.is_round_wind, None);
        assert_eq!(value_honor.is_seat_wind, Some(true));
        // 自風と確定しているので、場風が不明でも役牌としては確定している。
        assert!(value_honor.is_confirmed());
        assert!(!value_honor.is_unconfirmed_wind());

        assert_eq!(
            facts_of(&context, 1).value_honor_melds,
            ValueHonorMeldCounts {
                dragon: 0,
                round_wind: 0,
                seat_wind: 1,
                confirmed: 1,
                unconfirmed_wind: 0,
            }
        );
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

        assert_eq!(threat.melds[0].facts.value_honor, None);
        assert_eq!(threat.melds[1].facts.value_honor, None);
        assert_eq!(
            facts_of(&context, 1).value_honor_melds,
            ValueHonorMeldCounts::default()
        );
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
        let facts = player_threat_facts_from_context(&context);

        assert_eq!(facts[2].is_self, Some(true));
        assert_eq!(facts[2].is_opponent(), Some(false));
        for player in [0, 1, 3] {
            assert_eq!(facts[player].is_self, Some(false));
            assert_eq!(facts[player].is_opponent(), Some(true));
        }
        assert_eq!(facts[0].is_dealer, Some(true));
        assert_eq!(facts[1].is_dealer, Some(false));
    }

    #[test]
    fn unknown_player_id_does_not_guess_the_self_seat() {
        let context = context([vec![pon(HAKU)], vec![], vec![], vec![]]);
        let facts = player_threat_facts_from_context(&context);

        assert_eq!(facts.len(), 4);
        for (player, facts) in facts.iter().enumerate() {
            assert_eq!(facts.player, player);
            assert_eq!(facts.is_self, None);
            assert_eq!(facts.is_opponent(), None);
        }
        assert_eq!(facts[0].meld_count, 1);
    }

    #[test]
    fn unknown_oya_does_not_guess_the_dealer_or_seat_wind() {
        let context = context_with(
            Some(0),
            None,
            Some(honor(EAST)),
            vec![],
            [false; 4],
            Default::default(),
        );

        for facts in player_threat_facts_from_context(&context) {
            assert_eq!(facts.is_dealer, None);
            assert_eq!(facts.seat_wind, None);
        }
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

        assert!(threat.facts.reached);
        assert_eq!(threat.facts.meld_count, 2);
        assert_eq!(threat.facts.open_meld_count, 2);
        assert!(threat.melds[0].facts.value_honor.unwrap().is_dragon);
    }

    #[test]
    fn diagnostics_cover_every_seat_in_order() {
        let context = context([vec![], vec![chi()], vec![], vec![ankan(HAKU)]]);
        let threats = diagnose_player_threats(&context);

        assert_eq!(
            threats
                .iter()
                .map(|threat| (threat.facts.player, threat.facts.meld_count))
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
            discards: context.discards_of(1).unwrap(),
            dora_indicators: context.dora_indicators(),
        };

        assert_eq!(diagnose_player_threat(inputs), threat_of(&context, 1));
        assert_eq!(player_threat_facts(inputs), facts_of(&context, 1));
    }

    // ---- 軽量 facts と full diagnostic の一致 ----

    #[test]
    fn lightweight_facts_match_the_full_diagnostic_for_every_seat() {
        let context = mixed_threat_context();
        let facts = player_threat_facts_from_context(&context);
        let threats = diagnose_player_threats(&context);

        for player in 0..4 {
            assert_eq!(facts[player], threats[player].facts, "player {player}");
        }
    }

    #[test]
    fn diagnostic_melds_share_the_meld_facts() {
        let context = mixed_threat_context();
        let threats = diagnose_player_threats(&context);

        for (player, threat) in threats.iter().enumerate() {
            let melds = context.melds_of(player).unwrap();
            assert_eq!(threat.melds.len(), melds.len());
            for (meld, diagnostic) in melds.iter().zip(&threat.melds) {
                assert_eq!(
                    diagnostic.facts,
                    meld_threat_facts(
                        meld,
                        context.dora_indicators(),
                        context.round_wind(),
                        context.seat_wind_of(player),
                    )
                );
                assert_eq!(diagnostic.tiles, meld.tiles());
            }
        }
    }

    #[test]
    fn diagnosing_with_facts_keeps_the_given_facts() {
        let context = mixed_threat_context();
        let facts = player_threat_facts_from_context(&context);

        assert_eq!(
            diagnose_player_threats_with_facts(&context, &facts),
            diagnose_player_threats(&context)
        );
    }

    #[test]
    fn meld_aggregates_match_the_sum_of_meld_facts() {
        let context = mixed_threat_context();
        let facts = player_threat_facts_from_context(&context);
        let threats = diagnose_player_threats(&context);

        for (player, threat) in threats.iter().enumerate() {
            let expected_dora: u8 = threat.melds.iter().map(|meld| meld.facts.dora_count).sum();
            let expected_red: u8 = threat
                .melds
                .iter()
                .map(|meld| meld.facts.red_dora_count)
                .sum();
            let open_melds = || threat.melds.iter().filter(|meld| meld.facts.is_open);
            let expected_open_dora: u8 = open_melds().map(|meld| meld.facts.dora_count).sum();
            let expected_open_red: u8 = open_melds().map(|meld| meld.facts.red_dora_count).sum();

            assert_eq!(facts[player].meld_count, threat.melds.len());
            assert_eq!(
                facts[player].open_meld_count,
                threat.melds.iter().filter(|m| m.facts.is_open).count()
            );
            assert_eq!(
                facts[player].kan_count,
                threat.melds.iter().filter(|m| m.facts.is_kan).count()
            );
            assert_eq!(facts[player].meld_dora_count, expected_dora);
            assert_eq!(facts[player].meld_red_dora_count, expected_red);
            assert_eq!(facts[player].open_meld_dora_count, expected_open_dora);
            assert_eq!(facts[player].open_meld_red_dora_count, expected_open_red);
            assert_eq!(
                facts[player].meld_kinds,
                MeldKindCounts::of(context.melds_of(player).unwrap())
            );
        }
    }

    // ---- 河の枚数 ----

    #[test]
    fn discard_count_matches_the_context_river_length() {
        let context = context_with_discards(
            Some(0),
            Some(1),
            None,
            vec![],
            [false; 4],
            [river(0), river(1), river(9), river(12)],
            Default::default(),
        );
        let facts = player_threat_facts_from_context(&context);

        for (player, expected) in [0usize, 1, 9, 12].into_iter().enumerate() {
            assert_eq!(facts[player].discard_count, expected, "player {player}");
            assert_eq!(
                facts[player].discard_count,
                context.discards_of(player).unwrap().len(),
                "player {player}"
            );
        }
    }

    #[test]
    fn discard_count_is_zero_without_a_river() {
        let context = context(Default::default());
        for facts in player_threat_facts_from_context(&context) {
            assert_eq!(facts.discard_count, 0);
        }
    }

    // ---- open meld 限定 facts ----

    #[test]
    fn open_meld_facts_exclude_ankan() {
        // 4m 表示で 5m がドラ。Ankan は 5m 4枚で、Chi は役牌にならない数牌。
        let ankan_dora = Meld::new(
            MeldKind::Ankan,
            vec![tile(16), tile(17), tile(18), tile(19)],
            None,
        );
        let context = context_with(
            Some(0),
            Some(1),
            Some(honor(EAST)),
            vec![tile(12)],
            [false; 4],
            [vec![], vec![], vec![], vec![ankan_dora, pon(HAKU)]],
        );
        let facts = facts_of(&context, 3);

        assert_eq!(facts.meld_count, 2);
        assert_eq!(facts.open_meld_count, 1);
        // fixed meld 全体には Ankan のドラと役牌を含む。
        assert_eq!(facts.meld_dora_count, 5);
        assert_eq!(facts.meld_red_dora_count, 1);
        assert_eq!(facts.value_honor_melds.confirmed, 1);
        // open meld 限定では Ankan を除く。
        assert_eq!(facts.open_meld_dora_count, 0);
        assert_eq!(facts.open_meld_red_dora_count, 0);
        assert_eq!(facts.open_value_honor_melds.confirmed, 1);
    }

    #[test]
    fn ankan_only_player_has_no_open_meld_facts() {
        let context = context_with(
            Some(0),
            Some(1),
            Some(honor(EAST)),
            vec![],
            [false; 4],
            [vec![], vec![], vec![], vec![ankan(HAKU)]],
        );
        let facts = facts_of(&context, 3);

        assert_eq!(facts.meld_count, 1);
        assert_eq!(facts.open_meld_count, 0);
        assert_eq!(facts.value_honor_melds.confirmed, 1);
        assert_eq!(facts.open_meld_dora_count, 0);
        assert_eq!(facts.open_meld_red_dora_count, 0);
        assert_eq!(
            facts.open_value_honor_melds,
            ValueHonorMeldCounts::default()
        );
    }

    #[test]
    fn open_value_honor_melds_keep_the_unconfirmed_wind() {
        // oya 不明で自風が分からない風牌の Pon。open meld 側でも「役牌ではない」と確定させない。
        let context = context_with(
            Some(0),
            None,
            Some(honor(EAST)),
            vec![],
            [false; 4],
            [vec![], vec![], vec![], vec![pon(WEST)]],
        );
        let facts = facts_of(&context, 3);

        assert_eq!(facts.open_value_honor_melds.confirmed, 0);
        assert_eq!(facts.open_value_honor_melds.unconfirmed_wind, 1);
        assert_eq!(facts.open_value_honor_melds, facts.value_honor_melds);
        assert_eq!(facts.open_visible_han_proxy(), 0);
    }

    // ---- リーチ情報の source of truth ----

    #[test]
    fn reach_facts_match_the_context_reach_information() {
        // player_id / oya / reached のあらゆる組み合わせで、facts 由来のリーチ情報が
        // GameContext のリーチ情報と一致する。
        let reach_patterns = [
            [false, false, false, false],
            [true, false, false, false],
            [false, true, false, false],
            [true, true, false, false],
            [false, true, true, false],
            [true, true, true, true],
        ];

        for player_id in [None, Some(0), Some(2)] {
            for oya in [None, Some(0), Some(1)] {
                for reached in reach_patterns {
                    let context =
                        context_with(player_id, oya, None, vec![], reached, Default::default());
                    let facts = player_threat_facts_from_context(&context);
                    let reached_opponents = context.reached_opponents();

                    for (player, facts) in facts.iter().enumerate() {
                        assert_eq!(facts.reached, context.is_reached(player));
                        assert_eq!(
                            facts.is_reached_opponent(),
                            reached_opponents.contains(&player),
                            "{player_id:?} {oya:?} {reached:?} player {player}"
                        );
                    }

                    assert_eq!(
                        usize::from(reached_opponent_count(&facts)),
                        reached_opponents.len(),
                        "{player_id:?} {oya:?} {reached:?}"
                    );
                    assert_eq!(
                        has_reached_dealer(&facts),
                        oya.is_some_and(|oya| reached_opponents.contains(&usize::from(oya))),
                        "{player_id:?} {oya:?} {reached:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn unknown_player_id_keeps_every_reacher_as_an_opponent() {
        // player_id 不明でリーチ者を0人扱いにしない。
        let context = context_with(
            None,
            None,
            None,
            vec![],
            [true, false, true, false],
            Default::default(),
        );
        let facts = player_threat_facts_from_context(&context);

        assert_eq!(reached_opponent_count(&facts), 2);
        assert!(facts[0].is_reached_opponent());
        assert!(facts[2].is_reached_opponent());
        assert!(!facts[1].is_reached_opponent());
    }

    #[test]
    fn own_reach_is_not_an_opponent_reach() {
        let context = context_with(
            Some(0),
            Some(0),
            None,
            vec![],
            [true, false, false, false],
            Default::default(),
        );
        let facts = player_threat_facts_from_context(&context);

        assert!(facts[0].reached);
        assert!(!facts[0].is_reached_opponent());
        assert_eq!(reached_opponent_count(&facts), 0);
        assert!(!has_reached_dealer(&facts));
    }

    #[test]
    fn unknown_oya_does_not_confirm_a_dealer_reach() {
        let context = context_with(
            Some(0),
            None,
            None,
            vec![],
            [false, true, false, false],
            Default::default(),
        );
        let facts = player_threat_facts_from_context(&context);

        assert_eq!(reached_opponent_count(&facts), 1);
        assert!(!has_reached_dealer(&facts));
    }
}
