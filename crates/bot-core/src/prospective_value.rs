//! 1向聴 lookahead の各枝の先にあるテンパイの、将来打点を求める policy 層。
//!
//! 既存の2手先評価 ([`LookaheadDiagnostic`]) が持つ枝
//!
//! ```text
//! 現在打牌 → 仮想ツモ (物理牌 variant) → 打点込みの比較で選んだ next discard → テンパイ
//! ```
//!
//! の最後のテンパイについて、既存の待ちごとの手牌価値 ([`TenpaiHandValueProfile`]) をそのまま
//! 評価する。向聴・受け入れ・待ち・完成手の組み立て・点数計算・比較はすべて既存 layer を
//! source of truth にし、この層は「どの局面をどの baseline で評価し、その結果をどう保持するか」
//! だけを持つ。
//!
//! ツモ和了の baseline と variant 集約は current / prospective のどちらにも属さない共通
//! primitive ([`crate::tenpai_scoring`]) が持ち、この層は未来テンパイの評価材料をそこへ渡す
//! だけである。
//!
//! # 打牌選択が使う値
//!
//! [`ProductionProspectiveValuator`] は bot-logic の2手先評価へ渡す評価器で、テンパイ1つ分の
//!
//! ```text
//! Σ(最終和了牌の物理牌 variant 残枚数 × Payment.total())
//! ```
//!
//! を返す。この値は2手目の打牌候補の比較にそのまま使われ、さらに1手目の物理牌 variant 残枚数で
//! 重み付けして現在打牌の比較に使われる。平均へ正規化せず、整数除算もしない。本場・供託は
//! 加えず、Ron / Tsumo 確率も放銃率も含めない。
//!
//! [`ProspectiveLookaheadDiagnostic`] は同じ枝を表示用に展開した診断で、選択に使った値
//! ([`ProspectiveDrawVariantValue::selection_value`]) は評価器が返した値そのものを持つ。診断が
//! 別の打点を求め直して production と食い違うことはない。
//!
//! # 評価対象の手牌状態
//!
//! [`ProductionProspectiveValuator`] は卓・局の事実 (ドラ表示牌・場風 / 自風・持ち点・既リーチ・
//! 自分の河・履歴依存フリテン・見え牌・山残枚数) を `GameContext` から取り、手牌側の状態だけを
//! 構築時に確定した evaluation hand state から取る。この分離により、仮想的な鳴きの後の手牌状態を
//! synthetic な `GameContext` を作らずに評価できる。
//!
//! 手牌側の入力は評価対象の副露1つだけで、
//!
//! ```text
//! evaluation melds
//!     ├─ 完成手 (tenpai_completed_hands) へ渡す副露
//!     ├─ 副露済み面子数 (evaluation_fixed_meld_count_of)
//!     └─ 門前 (is_menzen) → 将来 Reach legality
//! ```
//!
//! と一方向に導出する。副露と副露済み面子数を別々に受け取らないので、同じ評価器の中で両者が
//! 食い違う state は作れない。
//!
//! 副露の unknown (`GameContext::own_melds() == None`) は副露0件と別物で、完成手へは既存
//! fallback と同じ空の副露を、副露済み面子数も既存の unknown fallback を使いつつ、門前判定は
//! unknown のままにする。
//!
//! # Reach / Damaten
//!
//! 将来テンパイでリーチするかどうかは、production のリーチ判断と同じ [`decide_reach_reason`] で
//! 決める。この層に threshold も新しい policy も作らない。リーチの合法性も現在局面の
//! `legal_actions` を流用せず、共有条件 ([`is_reach_legal`]) を将来テンパイの材料で評価する。
//! 未来時点の山残枚数は確定できないため unknown として渡し、共有条件の unknown 規則
//! (明示的に不可能と分かる場合だけ不可) をそのまま適用する。
//!
//! 自分の席を特定できず既リーチかどうかも判断できない場合は、未リーチだともリーチ済みだとも
//! 推測せず [`TenpaiOffenseMode::Unknown`] にして打点も確定しない。
//!
//! # terminal Tsumo counterfactual
//!
//! 現在聴牌を1巡 defer する診断では、同じ terminal tenpai を production / forced Reach /
//! forced Damaten の3 mode で観測する。production の Tsumo value は2手先評価が構築済みの
//! `tsumo_continuation` をそのまま保持し、forced counterfactual は同じ [`ProspectiveFacts`] を
//! 共通 Tsumo scoring へ異なる baseline だけで渡す。production と同じ mode は再評価しない。
//! Reach legality も [`is_reach_legal`] の既存評価結果を保持し、production `mode` から逆算しない。
//!
//! # ロン可否
//!
//! ダマ打点はロン和了を前提にした baseline なので、production ([`crate::offense_value`]) と
//! 同じく「ダマでロンできると確定した場合」だけ確定値として使う。ロン可否は既存のフリテン基盤
//! ([`tenpai_wait_availability`]) が source of truth で、この層で判定規則を書き直さない。
//!
//! 未来テンパイの恒常フリテンは、現在の自分の河へその枝でここまでに切った全打牌
//! ([`ProspectiveTenpai::discarded_tiles`]) を足した河 ([`OwnDiscards::with_discards`]) で
//! 判定できる。枝が何手先まで進んでいるかはこの層の判定に影響しない。自分の河を特定できない
//! 場合は既存どおり [`PermanentFuriten::Unknown`](bot_logic::PermanentFuriten) のままにし、
//! 非フリテンだと推測しない。履歴依存フリテンは、テンパイへ至る打牌が自分のツモを経ていること
//! だけを評価時点の事実として補正し ([`HistoryFuritenFacts::after_discard`])、未確定の軸を
//! `false` で埋めない。
//!
//! ロン可否が確定しない場合はダマ打点を判断材料にせず、`damaten_verdict = None` として既存の
//! リーチ判断 ([`decide_reach_reason`]) の fallback へ委ねる。その結果ダマを選んだ枝は、ロン
//! できると確定していない限り確定打点を持たない。将来フリテンによる価値補正や EV 補正は
//! 追加しない。
//!
//! # 仮想ツモ牌の赤5
//!
//! 受け入れは34種の牌種単位だが、2手先評価が赤5 / 黒5の物理牌 variant へ分けた枝を返すため、
//! 和了手の赤ドラ枚数も枝ごとに確定する。赤5の確率を推測する必要はない。
//!
//! # 集約
//!
//! 待ち牌種ごと・和了牌の物理牌 (赤5 / 黒5) ごとの支払いをそのまま保持したうえで、診断用に
//! 残枚数の加重平均も持つ。集約規則は押し引きの攻撃打点と同じ helper を共有し、
//! 役なし・裏ドラ未確定・点数計算の入力不足を0点として平均へ入れない。残枚数0の variant は
//! 生きていないので平均へ寄与させない。ダマとリーチは別々に集約する。整数除算した平均値は
//! 表示専用で、threshold 判定にも選択にも使わない。

use bot_logic::{
    DiscardEvaluation, DiscardLookaheadDiagnostic, DrawLookaheadDiagnostic,
    DrawVariantLookaheadDiagnostic, FixedMeldCount, HistoryFuritenFacts, LookaheadDiagnostic, Meld,
    OwnDiscards, ProspectiveTenpai, ProspectiveTenpaiValuator, ProspectiveTsumoValuator,
    TenpaiCompletedHands, TenpaiHandValueProfile, TenpaiTsumoValue, TenpaiWaitAvailability,
    TileCounts, TileId, TileType, WinningContext, evaluate_tenpai_hand_value, is_menzen,
    split_discarded_tile, structural_acceptance_tile_types_with_fixed_melds,
    tenpai_completed_hands, tenpai_wait_availability,
};

use crate::context::GameContext;
use crate::damaten_value::{damaten_baseline_context, damaten_value_from_hands};
use crate::discard_selection::evaluation_fixed_meld_count_of;
use crate::offense_value::{
    BASELINE_URA_DORA_INDICATORS, OffenseValue, TenpaiOffenseMode, reach_baseline_context,
    variant_total, weighted_average,
};
use crate::reach_policy::{ReachLegalityFacts, decide_reach_reason, is_reach_legal};
use crate::tenpai_scoring::{
    TenpaiVariantValue, TsumoVariantOutcomes, tenpai_tsumo_value_from_hands,
    tenpai_tsumo_variant_outcomes, tenpai_variant_value,
};

// テンパイの向聴数。
const TENPAI_SHANTEN: i8 = 0;

// 枝の中の打牌はどれも必ず仮想ツモを経る。自摸 → 打牌で同巡内フリテンは解除されるので、未来
// テンパイ時点の履歴依存フリテンはこの事実だけで補正できる。
const FUTURE_AFTER_OWN_DRAW: bool = true;

/// 和了牌の物理牌1つ分の将来打点と、その残枚数。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProspectiveWinningTileValue {
    pub winning_tile: TileId,
    /// この variant の残枚数。待ち全体の残枚数のうち、赤 / 黒それぞれの枚数。
    pub remaining: u8,
    pub value: TenpaiVariantValue,
}

impl ProspectiveWinningTileValue {
    pub fn is_red(&self) -> bool {
        self.winning_tile.is_red()
    }
}

/// 待ち1牌種分の将来打点。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProspectiveWaitValue {
    pub winning_tile: TileType,
    /// この待ち全体の残枚数。既存 [`TenpaiHandValueProfile`] の値そのもの。
    pub remaining: u8,
    /// 和了牌の物理牌ごとの将来打点。赤5と黒5のどちらもあり得る場合は両方を含む。
    pub winning_tiles: Vec<ProspectiveWinningTileValue>,
}

/// baseline 1つ分の将来打点。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProspectiveBaselineValue {
    /// 評価に使った hypothetical baseline。未来の実際の和了状況ではない。
    pub baseline: WinningContext,
    /// 待ち牌種ごとの将来打点。待ちと残枚数は `next_discard` の受け入れそのもの。
    pub waits: Vec<ProspectiveWaitValue>,
    /// 生きた variant の残枚数加重平均打点。診断表示専用で、選択にも threshold にも使わない。
    pub weighted_average: OffenseValue,
}

impl ProspectiveBaselineValue {
    /// 和了牌の物理牌ごとの将来打点を待ちの順に並べた iterator。
    pub fn winning_tile_values(&self) -> impl Iterator<Item = &ProspectiveWinningTileValue> {
        self.waits.iter().flat_map(|wait| wait.winning_tiles.iter())
    }
}

/// `next_discard` 後のテンパイ1件分の将来打点。
///
/// ダマ / リーチ両方の baseline の打点を並べて保持し、そのうち production のリーチ判断が採用した
/// baseline を `mode` が指す。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProspectiveTenpaiValue {
    pub damaten: ProspectiveBaselineValue,
    pub reach: ProspectiveBaselineValue,
    /// production のリーチ判断と同じ policy が選んだ攻撃モード。
    pub mode: TenpaiOffenseMode,
    /// 既存の将来テンパイ Reach legality 判定。その結論を counterfactual でも source of truth
    /// として共有し、`mode` から逆算しない。
    pub future_reach_legal: bool,
    /// production mode に関係なく Reach Tsumo baseline で評価した terminal self-tsumo value。
    ///
    /// 将来リーチが不可能な場合と、ツモ打点を確定できない場合は `None`。production mode が
    /// Reach の場合は既存 lookahead の `tsumo_continuation` そのものを保持する。
    pub forced_reach_tsumo: Option<TenpaiTsumoValue>,
    /// production mode に関係なく Damaten Tsumo baseline で評価した terminal self-tsumo value。
    ///
    /// production mode が Damaten の場合は既存 lookahead の `tsumo_continuation` そのものを
    /// 保持する。Tsumo baseline なので Ron 可否には依存しない。
    pub forced_damaten_tsumo: Option<TenpaiTsumoValue>,
    /// 既存のフリテン基盤による、この未来テンパイの総合ロン可否。判断できない場合は `None`。
    ///
    /// `Some(true)` の場合だけダマ打点を判断材料と確定値に使う。
    pub can_ron: Option<bool>,
}

impl ProspectiveTenpaiValue {
    /// このテンパイの待ちと残枚数。
    ///
    /// ダマとリーチは同じ完成手集合を別の baseline で評価したものなので、待ちも残枚数も baseline
    /// に依らない。支払いだけが baseline ごとに違うため、待ちを見るだけならどちらでもよい。
    pub fn waits(&self) -> &[ProspectiveWaitValue] {
        &self.damaten.waits
    }
}

/// 将来打点を評価できない理由。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProspectiveUnavailable {
    /// 仮想局面の物理牌一覧を作れない、または完成手を解析できない。
    CompletedHand,
}

/// 受け入れ牌の物理牌 variant 1つ分の枝の将来打点。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProspectiveOutcome {
    /// 2手目の打牌候補が1件も無い (`next_discard == None`)。将来打点を持たない。
    NoNextDiscard,
    /// `next_discard` 後がテンパイでない。将来打点を持たない。
    NotTenpai,
    /// テンパイだが評価できない。
    Unavailable(ProspectiveUnavailable),
    /// テンパイの待ちごとにダマ / リーチ baseline で評価した。
    Evaluated(ProspectiveTenpaiValue),
}

impl ProspectiveOutcome {
    pub fn evaluated(&self) -> Option<&ProspectiveTenpaiValue> {
        match self {
            Self::Evaluated(value) => Some(value),
            Self::NoNextDiscard | Self::NotTenpai | Self::Unavailable(_) => None,
        }
    }
}

/// 仮想ツモ牌の物理牌1つ分の枝の将来打点。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProspectiveDrawVariantValue {
    /// 仮想的にツモった物理牌。
    pub drawn_tile: TileId,
    /// この variant の残枚数。
    pub remaining: u8,
    /// 打点込みの比較で選ばれた2手目の打牌。
    pub next_discard: Option<TileType>,
    /// 打牌選択が実際に使った将来打点。2手先評価が持つ値そのもので、診断のために求め直さない。
    pub selection_value: Option<u64>,
    pub outcome: ProspectiveOutcome,
}

/// 現在打牌後の受け入れ牌1牌種分の将来打点。
///
/// `draw` / `remaining` は既存2手先評価 ([`DrawLookaheadDiagnostic`]) の値そのもので、診断のために
/// 求め直さない。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProspectiveDrawValue {
    pub draw: TileType,
    /// 牌種単位の残枚数。`variants` の残枚数の合計と一致する。
    pub remaining: u8,
    pub variants: Vec<ProspectiveDrawVariantValue>,
}

impl ProspectiveDrawValue {
    pub fn variant(&self, drawn_tile: TileId) -> Option<&ProspectiveDrawVariantValue> {
        self.variants
            .iter()
            .find(|variant| variant.drawn_tile == drawn_tile)
    }
}

/// 現在の打牌候補1件分の将来打点。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProspectiveDiscardValue {
    pub discard: TileType,
    /// 受け入れ牌ごとの枝。順序と対象牌は既存2手先評価と同じ。
    pub draws: Vec<ProspectiveDrawValue>,
}

impl ProspectiveDiscardValue {
    pub fn draw(&self, tile: TileType) -> Option<&ProspectiveDrawValue> {
        self.draws.iter().find(|draw| draw.draw == tile)
    }
}

/// 全打牌候補分の将来打点。
///
/// `candidates` は既存2手先評価と同じ順序・同じ件数で、selected 候補だけでなく runner-up を含む
/// 全候補に対応する。
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ProspectiveLookaheadDiagnostic {
    pub candidates: Vec<ProspectiveDiscardValue>,
}

impl ProspectiveLookaheadDiagnostic {
    pub fn candidate(&self, discard: TileType) -> Option<&ProspectiveDiscardValue> {
        self.candidates
            .iter()
            .find(|candidate| candidate.discard == discard)
    }
}

/// 2手先評価が選んだ2手目の打牌後のテンパイを、production のリーチ判断と同じ policy で評価する
/// 評価器。
///
/// bot-logic の2手先評価へ渡され、2手目の打牌比較と現在打牌の集計の両方で同じ値が使われる。
pub(crate) struct ProductionProspectiveValuator<'a> {
    context: &'a GameContext,
    // 評価対象の副露。unknown (`None`) と副露0件 (`Some(&[])`) は別物なので潰さない。完成手・
    // 門前判定・リーチ合法性はすべてこの1つを source of truth にし、`GameContext` から取り直さない。
    melds: Option<&'a [Meld]>,
    // 評価対象の副露済み面子数。`melds` から導出した値だけを持ち、構築後に食い違わない。
    // 2手先評価 ([`crate::discard_selection::lookahead_inputs`]) へもここから渡す。
    fixed_meld_count: FixedMeldCount,
    // ダマ / リーチ両方の hypothetical baseline。枝ごとに組み立て直さない。
    damaten: WinningContext,
    reach: WinningContext,
    // 将来テンパイでリーチが合法か。手牌の副露・持ち点・既リーチはこの局から変わらないので、
    // 枝ごとに求め直さない。
    reach_legal: bool,
    // 自分が既にリーチしているか。自分の席を特定できない場合は `None`。
    own_reached: Option<bool>,
    // 現在の自分の河。枝ごとに、その枝でここまでに切った全打牌を足して恒常フリテンを判定する。
    own_discards: OwnDiscards,
    // 未来テンパイ時点へ補正した履歴依存フリテン。未確定の軸は unknown のまま持ち回る。
    history_furiten: HistoryFuritenFacts,
}

impl<'a> ProductionProspectiveValuator<'a> {
    /// 現在の `GameContext` の副露状態をそのまま評価対象にする入口。
    pub(crate) fn new(context: &'a GameContext) -> Self {
        Self::new_with_hand_state(context, context.own_melds())
    }

    /// 評価対象の副露状態を明示する入口。
    ///
    /// 卓・局の事実 (ドラ表示牌・場風 / 自風・持ち点・既リーチ・自分の河・履歴依存フリテン・
    /// 見え牌) は渡された `context` のままで、手牌側の状態だけを差し替える。仮想的な鳴きの後を
    /// 評価するために synthetic な `GameContext` を組み立てない。
    ///
    /// 副露済み面子数と門前はこの `melds` から導出する。caller が副露と面子数を別々に渡せない
    /// ので、同じ評価器の中で両者が食い違う state は作れない。
    ///
    /// `melds` の `None` は「自分の副露が分からない」で、副露0件 (`Some(&[])`) と区別する。
    /// 完成手へは既存 fallback と同じ空の副露を渡し、副露済み面子数も既存の unknown fallback
    /// (`FixedMeldCount::NONE`) にしたうえで、門前判定は unknown のままにする。
    pub(crate) fn new_with_hand_state(context: &'a GameContext, melds: Option<&'a [Meld]>) -> Self {
        Self {
            context,
            melds,
            fixed_meld_count: evaluation_fixed_meld_count_of(melds),
            damaten: damaten_baseline_context(context),
            reach: reach_baseline_context(context),
            reach_legal: future_reach_legal(context, melds.map(is_menzen)),
            own_reached: context.own_reached(),
            own_discards: OwnDiscards::from_optional_river(context.own_discards()),
            history_furiten: context
                .history_furiten()
                .after_discard(FUTURE_AFTER_OWN_DRAW),
        }
    }

    /// 評価対象の副露済み面子数。2手先評価へ渡す値もここから取り、評価器と食い違わせない。
    pub(crate) fn fixed_meld_count(&self) -> FixedMeldCount {
        self.fixed_meld_count
    }

    #[cfg(test)]
    pub(crate) fn reach_legal(&self) -> bool {
        self.reach_legal
    }

    // 枝1つ分の評価材料。完成手と、既存フリテン基盤で求めた待ち・ロン可否を組にする。
    //
    // 物理牌を組み立てられない場合と完成手を解析できない場合だけ `None`。
    pub(crate) fn tenpai_facts(&self, tenpai: &ProspectiveTenpai<'_>) -> Option<ProspectiveFacts> {
        let availability = self.wait_availability(tenpai);

        // その枝でここまでに切った牌はテンパイ時点で見え牌になる。赤5が見えているかの判定に使う。
        let mut visible = self.context.visible_tiles().to_vec();
        visible.extend_from_slice(tenpai.discarded_tiles);

        let hands = tenpai_completed_hands(
            tenpai.concealed_tiles,
            self.melds.unwrap_or_default(),
            tenpai.acceptance,
            availability.as_ref(),
            &visible,
        )
        .ok()?;

        Some(ProspectiveFacts {
            hands,
            availability,
        })
    }

    // 未来テンパイの待ちとロン可否。既存のフリテン基盤へ同じ入力を渡すだけで、判定規則をこの層で
    // 書き直さない。
    //
    // 恒常フリテンは「現在の自分の河 + その枝でここまでに切った全打牌 (`discarded_tiles`)」で
    // 判定する。何手先の枝でも渡された打牌をそのまま河として扱う。自分の河を特定できない場合は
    // 既存どおり Unknown のままで、非フリテンだと推測しない。履歴依存フリテンは未来テンパイ
    // 時点へ補正済みの値をそのまま渡す。
    fn wait_availability(&self, tenpai: &ProspectiveTenpai<'_>) -> Option<TenpaiWaitAvailability> {
        let counts = TileCounts::from_tiles(tenpai.concealed_tiles.iter().copied());
        tenpai_wait_availability(
            tenpai.acceptance,
            &structural_acceptance_tile_types_with_fixed_melds(&counts, self.fixed_meld_count),
            &self
                .own_discards
                .with_discards(tenpai.discarded_tiles.iter().map(|tile| tile.tile_type())),
            self.history_furiten,
        )
    }

    // 攻撃を継続した場合の攻撃モード。production のリーチ判断と同じ policy をそのまま使う。
    //
    // ダマでロンできると確定した場合だけダマ打点を判断材料にする。ロン可否 unknown を非フリテン
    // だと推測せず、`damaten_verdict = None` として既存の待ち枚数だけを見る fallback へ委ねる。
    pub(crate) fn offense_mode(&self, facts: &ProspectiveFacts) -> TenpaiOffenseMode {
        match self.own_reached {
            None => TenpaiOffenseMode::Unknown,
            Some(true) => TenpaiOffenseMode::Reach,
            Some(false) => {
                let damaten_verdict = facts
                    .can_ron()
                    .then(|| damaten_value_from_hands(self.context, &facts.hands).verdict);
                let reason =
                    decide_reach_reason(self.reach_legal, damaten_verdict, facts.tsumo_remaining());
                if reason.selects_reach() {
                    TenpaiOffenseMode::Reach
                } else {
                    TenpaiOffenseMode::Damaten
                }
            }
        }
    }

    // 攻撃モードごとの hypothetical baseline と裏ドラ表示牌。確定できない場合は `None`。
    //
    // ダマのまま進む手の打点はロン和了を前提にした baseline なので、ダマでロンできると確定した
    // 場合しか使えない。押し引きの攻撃打点と同じ入口条件で、ロンできない打点を確定値にしない。
    fn scoring_inputs(
        &self,
        mode: TenpaiOffenseMode,
        can_ron: bool,
    ) -> Option<(WinningContext, Option<&'static [TileId]>)> {
        match mode {
            TenpaiOffenseMode::Reach => Some((self.reach, Some(BASELINE_URA_DORA_INDICATORS))),
            TenpaiOffenseMode::Damaten => can_ron.then_some((self.damaten, None)),
            TenpaiOffenseMode::Unknown => None,
        }
    }

    /// 未来テンパイの評価材料を、指定した攻撃モードの共通 Tsumo scoring
    /// ([`tenpai_tsumo_value_from_hands`]) へ渡す。
    ///
    /// production のリーチ判断が決めたモードで評価する [`ProspectiveTsumoValuator`] と、現在
    /// 聴牌を forced Reach / forced Damaten で評価する診断が同じ scoring 経路を共有するための
    /// 入口。baseline の組み立ても集約規則もこの層は持たない。
    pub(crate) fn tsumo_value_with_mode(
        &self,
        facts: &ProspectiveFacts,
        mode: TenpaiOffenseMode,
    ) -> Option<TenpaiTsumoValue> {
        tenpai_tsumo_value_from_hands(self.context, &facts.hands, mode)
    }

    /// 未来テンパイの評価材料を、指定した攻撃モードの共通 Tsumo scoring
    /// ([`tenpai_tsumo_variant_outcomes`]) へ渡し、和了牌の物理牌 variant ごとの結論を得る。
    pub(crate) fn tsumo_variant_outcomes(
        &self,
        facts: &ProspectiveFacts,
        mode: TenpaiOffenseMode,
    ) -> TsumoVariantOutcomes {
        tenpai_tsumo_variant_outcomes(self.context, &facts.hands, mode)
    }

    // 選択に使う Σ(和了牌 variant 残枚数 × 支払い合計)。確定できない場合は `None`。
    fn selection_value(&self, facts: &ProspectiveFacts) -> Option<u64> {
        let (baseline, ura_dora) =
            self.scoring_inputs(self.offense_mode(facts), facts.can_ron())?;
        let profile = evaluate_tenpai_hand_value(
            &facts.hands,
            baseline,
            self.context.dora_indicators(),
            ura_dora,
        );
        weighted_total(&profile)
    }
}

impl ProspectiveTenpaiValuator for ProductionProspectiveValuator<'_> {
    fn tenpai_value(&self, tenpai: &ProspectiveTenpai<'_>) -> Option<u64> {
        self.selection_value(&self.tenpai_facts(tenpai)?)
    }
}

impl ProspectiveTsumoValuator for ProductionProspectiveValuator<'_> {
    fn tenpai_tsumo_value(&self, tenpai: &ProspectiveTenpai<'_>) -> Option<TenpaiTsumoValue> {
        let facts = self.tenpai_facts(tenpai)?;
        self.tsumo_value_with_mode(&facts, self.offense_mode(&facts))
    }
}

/// 未来テンパイ1件分の評価材料。完成手と、既存フリテン基盤による待ち・ロン可否。
///
/// 打牌選択と診断はこの1組を共有し、同じ枝のロン可否や完成手を別々に組み立てない。
pub(crate) struct ProspectiveFacts {
    hands: TenpaiCompletedHands,
    availability: Option<TenpaiWaitAvailability>,
}

impl ProspectiveFacts {
    /// 既存フリテン基盤による総合ロン可否。判断できない場合は `None`。
    pub(crate) fn ron_availability(&self) -> Option<bool> {
        self.availability
            .as_ref()
            .and_then(TenpaiWaitAvailability::can_ron)
    }

    // ダマ打点を確定値として使えるか。unknown はロンできると推測しない。
    fn can_ron(&self) -> bool {
        self.ron_availability() == Some(true)
    }

    // 生きた待ちの残枚数。既存の受け入れそのもので、リーチ判断の fallback へ渡す。
    fn tsumo_remaining(&self) -> u8 {
        self.availability
            .as_ref()
            .map_or(0, |availability| availability.tsumo_remaining)
    }
}

/// 将来テンパイでリーチが合法か。
///
/// 現在局面の `legal_actions` を未来へ流用せず、共有条件 ([`is_reach_legal`]) を将来テンパイの
/// 材料で評価する。既リーチ・持ち点は自分のツモと打牌では変わらないので現在の既知 fact を
/// そのまま使い、打牌後テンパイは枝の構成上必ず満たす。未来時点の山残枚数だけは確定できないので
/// 現在の枚数で代用せず unknown として渡し、共有条件の unknown 規則へ委ねる。
///
/// `menzen` は評価対象の副露状態から求めた値を渡す。`context` の副露から取り直さないので、
/// 仮想的な鳴きを含む evaluation hand state ではその副露がそのままリーチ合法性へ効く。
fn future_reach_legal(context: &GameContext, menzen: Option<bool>) -> bool {
    is_reach_legal(ReachLegalityFacts {
        menzen,
        already_reached: context.own_reached(),
        score: context.own_score(),
        remaining_tiles: None,
        tenpai_after_discard: true,
    })
}

/// 待ちごとの評価結果を Σ(生きた variant の残枚数 × 支払い合計) へ畳む。
///
/// 生きた variant のどれか1つでも支払いを確定できない場合は `None`。役なし・裏ドラ未確定・
/// 点数計算の入力不足はどれも「確定しない」で、0点として扱わない。集約規則は押し引きの攻撃打点
/// と同じ helper を共有する。
///
/// 生きた variant が1つも無いテンパイ (待ちが全て見えている) は和了しようが無いので、確定
/// できない値ではなく 0 になる。既存 weighted wait が死にテンを寄与 0 として扱うのと同じ。
fn weighted_total(profile: &TenpaiHandValueProfile<'_>) -> Option<u64> {
    let variants = || {
        profile
            .waits()
            .iter()
            .flat_map(|wait| wait.winning_tiles().iter())
    };
    if variants().all(|variant| variant.remaining() == 0) {
        return Some(0);
    }

    match weighted_average(variants().map(|variant| (variant_total(variant), variant.remaining())))
    {
        OffenseValue::Known { weighted_total, .. } => Some(weighted_total),
        OffenseValue::Unknown => None,
    }
}

/// 構築済みの2手先評価の各枝について、`next_discard` 後のテンパイの将来打点を展開する。
///
/// `tiles` は現在打牌の前の全物理牌 (手牌 + ツモ牌)、`evaluations` はその手牌に対する既存の
/// 打牌候補評価で、`lookahead` はその評価から構築した2手先評価。候補の順序・牌種が対応しない
/// 場合は推測せず、その候補の枝を空にする。
///
/// 探索も比較もやり直さない。2手先評価が打点込みの比較で選んだ `next_discard` をそのまま評価
/// 対象にし、打牌選択が使った打点も 2手先評価が持つ値をそのまま載せる。
pub(crate) fn evaluate_prospective_lookahead_value(
    context: &GameContext,
    tiles: &[TileId],
    evaluations: &[DiscardEvaluation],
    lookahead: &LookaheadDiagnostic,
) -> ProspectiveLookaheadDiagnostic {
    if lookahead.candidates.len() != evaluations.len() {
        return ProspectiveLookaheadDiagnostic::default();
    }

    let valuator = ProductionProspectiveValuator::new(context);
    ProspectiveLookaheadDiagnostic {
        candidates: lookahead
            .candidates
            .iter()
            .zip(evaluations)
            .map(|(candidate, evaluation)| {
                evaluate_prospective_candidate_value(&valuator, tiles, evaluation, candidate)
            })
            .collect(),
    }
}

/// 構築済みの2手先評価のうち、打牌候補1件分の枝だけについて将来打点を展開する。
///
/// 展開する枝も評価器も [`evaluate_prospective_lookahead_value`] と同じで、対象を渡された1候補に
/// 限定するだけ。全候補分の [`ProspectiveLookaheadDiagnostic`] は構築しない。
pub(crate) fn evaluate_prospective_candidate_value(
    valuator: &ProductionProspectiveValuator,
    tiles: &[TileId],
    evaluation: &DiscardEvaluation,
    candidate: &DiscardLookaheadDiagnostic,
) -> ProspectiveDiscardValue {
    ProspectiveDiscardValue {
        discard: candidate.discard,
        draws: candidate_draws(valuator, tiles, candidate, evaluation),
    }
}

// 現在の打牌候補1件分の枝。候補の牌種が既存評価と対応しない場合は推測せず枝を持たない。
fn candidate_draws(
    valuator: &ProductionProspectiveValuator,
    tiles: &[TileId],
    candidate: &DiscardLookaheadDiagnostic,
    evaluation: &DiscardEvaluation,
) -> Vec<ProspectiveDrawValue> {
    if candidate.discard != evaluation.discard {
        return Vec::new();
    }

    // 1手目に切る物理牌は候補ごとに1つなので、受け入れ牌ごとに求め直さない。
    let (first_discarded, after_first) = match split_discarded_tile(tiles.to_vec(), evaluation) {
        Some((discarded, remaining)) => (Some(discarded), remaining),
        None => (None, tiles.to_vec()),
    };
    candidate
        .draws
        .iter()
        .map(|draw| draw_value(valuator, first_discarded, &after_first, draw))
        .collect()
}

fn draw_value(
    valuator: &ProductionProspectiveValuator,
    first_discarded: Option<TileId>,
    after_first: &[TileId],
    draw: &DrawLookaheadDiagnostic,
) -> ProspectiveDrawValue {
    ProspectiveDrawValue {
        draw: draw.draw,
        remaining: draw.remaining,
        variants: draw
            .variants
            .iter()
            .map(|variant| ProspectiveDrawVariantValue {
                drawn_tile: variant.drawn_tile,
                remaining: variant.remaining,
                next_discard: variant.next_discard_tile(),
                selection_value: variant.prospective_value,
                outcome: variant_outcome(valuator, first_discarded, after_first, variant),
            })
            .collect(),
    }
}

// 枝1件分の将来打点。テンパイでない枝と評価できない枝を打点0へ潰さず、理由のまま返す。
fn variant_outcome(
    valuator: &ProductionProspectiveValuator,
    first_discarded: Option<TileId>,
    after_first: &[TileId],
    variant: &DrawVariantLookaheadDiagnostic,
) -> ProspectiveOutcome {
    let Some(next) = variant.next_discard.as_ref() else {
        return ProspectiveOutcome::NoNextDiscard;
    };
    if next.min_shanten_after_discard() != TENPAI_SHANTEN {
        return ProspectiveOutcome::NotTenpai;
    }

    let mut after_draw = after_first.to_vec();
    after_draw.push(variant.drawn_tile);
    let Some((next_discarded, concealed_tiles)) = split_discarded_tile(after_draw, next) else {
        return ProspectiveOutcome::Unavailable(ProspectiveUnavailable::CompletedHand);
    };
    let discarded_tiles: Vec<_> = first_discarded
        .into_iter()
        .chain([next_discarded])
        .collect();

    let tenpai = ProspectiveTenpai {
        concealed_tiles: &concealed_tiles,
        acceptance: &next.acceptance_after_discard,
        discarded_tiles: &discarded_tiles,
    };
    let Some(facts) = valuator.tenpai_facts(&tenpai) else {
        return ProspectiveOutcome::Unavailable(ProspectiveUnavailable::CompletedHand);
    };

    let dora_indicators = valuator.context.dora_indicators();
    let mode = valuator.offense_mode(&facts);
    ProspectiveOutcome::Evaluated(ProspectiveTenpaiValue {
        damaten: baseline_value(&facts.hands, valuator.damaten, dora_indicators, None),
        reach: baseline_value(
            &facts.hands,
            valuator.reach,
            dora_indicators,
            Some(BASELINE_URA_DORA_INDICATORS),
        ),
        mode,
        future_reach_legal: valuator.reach_legal,
        forced_reach_tsumo: valuator
            .reach_legal
            .then(|| {
                terminal_tsumo_value_with_mode(
                    valuator,
                    &facts,
                    mode,
                    variant.tsumo_continuation,
                    TenpaiOffenseMode::Reach,
                )
            })
            .flatten(),
        forced_damaten_tsumo: terminal_tsumo_value_with_mode(
            valuator,
            &facts,
            mode,
            variant.tsumo_continuation,
            TenpaiOffenseMode::Damaten,
        ),
        can_ron: facts.ron_availability(),
    })
}

// 同じ terminal tenpai を指定 Tsumo baseline で評価する。production mode と一致する場合は
// lookahead が既に構築した値をそのまま返し、同じ scoring を診断側でもう一度実行しない。
fn terminal_tsumo_value_with_mode(
    valuator: &ProductionProspectiveValuator<'_>,
    facts: &ProspectiveFacts,
    production_mode: TenpaiOffenseMode,
    production_value: Option<TenpaiTsumoValue>,
    mode: TenpaiOffenseMode,
) -> Option<TenpaiTsumoValue> {
    // production continuation が構築されていない局面では counterfactual だけを追加評価しない。
    // `SelfTsumoFacts` 不足や terminal scoring 不足を、別 mode なら既知だと推測しないためでもある。
    let production_value = production_value?;
    if production_mode == mode {
        Some(production_value)
    } else {
        valuator.tsumo_value_with_mode(facts, mode)
    }
}

// テンパイの完成手を baseline 1つで評価し、待ちごとの打点と残枚数加重平均へ畳む。
fn baseline_value(
    hands: &TenpaiCompletedHands,
    baseline: WinningContext,
    dora_indicators: &[TileId],
    ura_dora_indicators: Option<&[TileId]>,
) -> ProspectiveBaselineValue {
    let profile = evaluate_tenpai_hand_value(hands, baseline, dora_indicators, ura_dora_indicators);
    let waits = wait_values(&profile);
    let weighted_average = weighted_average(
        waits
            .iter()
            .flat_map(|wait| wait.winning_tiles.iter())
            .map(|variant| (variant.value.total(), variant.remaining)),
    );

    ProspectiveBaselineValue {
        baseline,
        waits,
        weighted_average,
    }
}

fn wait_values(profile: &TenpaiHandValueProfile<'_>) -> Vec<ProspectiveWaitValue> {
    profile
        .waits()
        .iter()
        .map(|wait| ProspectiveWaitValue {
            winning_tile: wait.winning_tile(),
            remaining: wait.remaining(),
            winning_tiles: wait
                .winning_tiles()
                .iter()
                .map(|winning_tile| ProspectiveWinningTileValue {
                    winning_tile: winning_tile.winning_tile(),
                    remaining: winning_tile.remaining(),
                    value: tenpai_variant_value(winning_tile.outcome()),
                })
                .collect(),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::sync::LazyLock;

    use bot_logic::{
        DrawTransition, EffectiveAcceptance, ForwardMetrics, MissingScoringFact,
        NormalScoringError, RiichiStatus, WinMethod, calculate_acceptance_with_fixed_melds,
        diagnose_lookahead, evaluate_discards_from_tiles_with_fixed_melds_and_context,
        evaluate_payment, forward_metrics_from_lookahead,
    };

    use crate::action::LegalAction;
    use crate::context::TableStateFacts;
    use crate::damaten_value::DAMATEN_MIN_TOTAL;
    use crate::discard_selection::{
        LookaheadDiagnosticScope, lookahead_inputs, select_discard_action_with_diagnostic,
    };
    use crate::meld::MeldKind;
    use crate::tenpai_scoring::{TenpaiVariantUnknownReason, tsumo_scoring_inputs};

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

        fn tile(&mut self, s: &str) -> TileId {
            let red = s.ends_with('r');
            let id = TileId::copies(tile(s))
                .find(|id| id.is_red() == red && !self.used[id.index()])
                .expect("同じ物理牌を使い回していない");
            self.used[id.index()] = true;
            id
        }
    }

    fn tile(s: &str) -> TileType {
        TileType::from_mjai_type_str(s.trim_end_matches('r')).expect("牌種として読める")
    }

    // 1向聴の局面と、その2手先診断・将来打点。2手先探索は重いので、同じ局面を使う複数のテストで
    // 構築結果を共有する。
    struct Case {
        ctx: GameContext,
        lookahead: LookaheadDiagnostic,
        value: ProspectiveLookaheadDiagnostic,
        /// 通常打牌選択が選んだ action。診断の範囲で選択結果が変わらないことの確認に使う。
        selected: Option<LegalAction>,
    }

    impl Case {
        fn draw(&self, discard: &str, draw: &str) -> &ProspectiveDrawValue {
            self.value
                .candidate(tile(discard))
                .expect("打牌候補がある")
                .draw(tile(draw))
                .expect("受け入れ牌の枝がある")
        }

        // 物理牌 variant が1つだけの受け入れ牌の枝。
        fn variant(&self, discard: &str, draw: &str) -> &ProspectiveDrawVariantValue {
            let [variant] = self.draw(discard, draw).variants.as_slice() else {
                panic!("物理牌 variant が1つだけの枝を指定する");
            };
            variant
        }

        // 赤 / 黒を指定した物理牌 variant の枝。
        fn red_variant(
            &self,
            discard: &str,
            draw: &str,
            red: bool,
        ) -> &ProspectiveDrawVariantValue {
            self.draw(discard, draw)
                .variants
                .iter()
                .find(|variant| variant.drawn_tile.is_red() == red)
                .expect("指定した物理牌 variant がある")
        }

        fn evaluated(&self, discard: &str, draw: &str) -> &ProspectiveTenpaiValue {
            self.variant(discard, draw)
                .outcome
                .evaluated()
                .expect("テンパイ枝の将来打点を評価できる")
        }

        // 打牌選択が使う前方集計値。構築済みの2手先評価から集計し、探索し直さない。
        fn metrics(&self) -> Vec<ForwardMetrics> {
            let evaluations = evaluations(&self.lookahead, &self.ctx);
            let tiles: Vec<TileId> = self
                .ctx
                .hand_tiles()
                .iter()
                .copied()
                .chain(self.ctx.drawn_tile())
                .collect();
            let valuator = ProductionProspectiveValuator::new(&self.ctx);
            forward_metrics_from_lookahead(
                &lookahead_inputs(&self.ctx, &tiles, &valuator, LookaheadDiagnosticScope::None),
                &evaluations,
                &self.lookahead,
            )
        }
    }

    // 場風・自風・見え牌を既知にした門前14枚の局面。自分は子 (南家)。`winds == false` では
    // 場風・自風を渡さず、点数計算の入力が足りない局面にする。
    // 局面のツモ以外の材料。ロン可否と合法リーチの条件を変える検証だけがここを触る。
    struct CaseSpec<'a> {
        hand: &'a [&'a str],
        dora_indicator: &'a str,
        extra_visible: &'a [&'a str],
        winds: bool,
        /// 自分の河。未来テンパイの恒常フリテンを作るために使う。
        own_river: &'a [&'a str],
        history_furiten: HistoryFuritenFacts,
        table_state: TableStateFacts,
        /// same-shanten の枝をテンパイまで追うか。
        downstream: bool,
    }

    // 履歴依存フリテンまで観測済みの既定値。production の局開始時と同じ facts。
    fn known_history_furiten() -> HistoryFuritenFacts {
        HistoryFuritenFacts {
            same_turn: Some(false),
            riichi_missed_win: Some(false),
        }
    }

    impl<'a> CaseSpec<'a> {
        fn new(hand: &'a [&'a str], dora_indicator: &'a str) -> Self {
            Self {
                hand,
                dora_indicator,
                extra_visible: &[],
                winds: true,
                own_river: &[],
                history_furiten: known_history_furiten(),
                table_state: TableStateFacts::default(),
                downstream: false,
            }
        }

        fn build(self) -> Case {
            let mut source = TileIdSource::new();
            let hand_tiles = source.tiles(&self.hand[..self.hand.len() - 1]);
            let drawn_tile = source.tile(self.hand[self.hand.len() - 1]);
            let dora_indicators = source.tiles(&[self.dora_indicator]);
            let extra_visible = source.tiles(self.extra_visible);
            let own_river = source.tiles(self.own_river);

            // 自分の河も見え牌になる。受け入れの残枚数と赤5の解決を局面と食い違わせない。
            let visible: Vec<TileId> = hand_tiles
                .iter()
                .chain([&drawn_tile])
                .chain(dora_indicators.iter())
                .chain(extra_visible.iter())
                .chain(own_river.iter())
                .copied()
                .collect();
            let actions: Vec<LegalAction> = hand_tiles
                .iter()
                .chain([&drawn_tile])
                .map(|&tile| LegalAction::Dahai { tile })
                .collect();

            let ctx = GameContext::from_parts_with_table_state(
                Some(drawn_tile),
                hand_tiles,
                dora_indicators,
                self.winds.then(|| tile("E")),
                self.winds.then(|| tile("S")),
                visible,
                Some(0),
                Some(3),
                [own_river, Vec::new(), Vec::new(), Vec::new()],
                [false; 4],
            )
            .with_table_state_facts(self.table_state)
            .with_history_furiten_facts(self.history_furiten);

            let scope = if self.downstream {
                LookaheadDiagnosticScope::SAME_SHANTEN_DOWNSTREAM
            } else {
                LookaheadDiagnosticScope::LOOKAHEAD
            };
            let selection = select_discard_action_with_diagnostic(&ctx, &actions, scope);
            Case {
                ctx,
                lookahead: selection.lookahead.expect("2手先診断が構築されている"),
                value: selection.lookahead_value.expect("将来打点が構築されている"),
                selected: selection.selection.action,
            }
        }
    }

    fn case_of(hand: &[&str], extra_visible: &[&str], winds: bool) -> Case {
        case_with_dora(hand, "1m", extra_visible, winds)
    }

    fn case_with_dora(
        hand: &[&str],
        dora_indicator: &str,
        extra_visible: &[&str],
        winds: bool,
    ) -> Case {
        CaseSpec {
            extra_visible,
            winds,
            ..CaseSpec::new(hand, dora_indicator)
        }
        .build()
    }

    // 123m 456m 78m 55p 13s 9s + ツモ 1p の1向聴。打 1p / 打 9s から 3m / 6m / 9m を引くと
    // 9s を切って 2s の嵌張テンパイになる。2s では么九牌が残って断幺が付かず、嵌張なので平和も
    // 付かないため、ダマでは役が無い枝になる。
    const NO_YAKU_HAND: [&str; 14] = [
        "1m", "2m", "3m", "4m", "5m", "6m", "7m", "8m", "5p", "5p", "1s", "3s", "9s", "1p",
    ];

    // 123m 456m 78m 55p 34s 9s + ツモ 1p の1向聴。打 1p / 打 9s から 3m / 6m / 9m を引くと
    // 9s を切って 2s / 5s の両面テンパイになり、和了牌 5s は赤 / 黒へ分かれる。
    const RED_FIVE_HAND: [&str; 14] = [
        "1m", "2m", "3m", "4m", "5m", "6m", "7m", "8m", "5p", "5p", "3s", "4s", "9s", "1p",
    ];

    static NO_YAKU: LazyLock<Case> = LazyLock::new(|| case_of(&NO_YAKU_HAND, &[], true));
    static RED_FIVE: LazyLock<Case> = LazyLock::new(|| case_of(&RED_FIVE_HAND, &[], true));
    static RED_FIVE_SEEN: LazyLock<Case> =
        LazyLock::new(|| case_of(&RED_FIVE_HAND, &["5sr"], true));
    static UNKNOWN_WINDS: LazyLock<Case> = LazyLock::new(|| case_of(&RED_FIVE_HAND, &[], false));

    fn known(total: u32) -> TenpaiVariantValue {
        TenpaiVariantValue::Known {
            payment: evaluate_payment(total / 4, false, WinMethod::Ron).expect("子のロン"),
            is_yakuman: false,
        }
    }

    fn variant(value: &ProspectiveWinningTileValue) -> (String, u8, Option<u32>) {
        (
            value.winning_tile.to_mjai_string(),
            value.remaining,
            value.value.total(),
        )
    }

    fn variants(baseline: &ProspectiveBaselineValue) -> Vec<(String, u8, Option<u32>)> {
        baseline.winning_tile_values().map(variant).collect()
    }

    #[test]
    fn a_tenpai_branch_keeps_the_damaten_value() {
        // 1向聴の枝がテンパイし、ダマ打点を待ちごとに取得できる。
        let damaten = &NO_YAKU.evaluated("1p", "9m").damaten;

        assert_eq!(
            variants(damaten),
            vec![("2s".to_string(), 4, Some(5200))],
            "打 1p → ツモ 9m → 打 9s の 2s テンパイ"
        );
        assert_eq!(damaten.weighted_average.average_total(), Some(5200));
    }

    #[test]
    fn the_damaten_baseline_is_the_existing_one() {
        // ダマ baseline は既存 policy そのもので、ここで組み立て直さない。
        assert_eq!(
            NO_YAKU.evaluated("1p", "9m").damaten.baseline,
            damaten_baseline_context(&NO_YAKU.ctx)
        );
    }

    #[test]
    fn the_reach_value_adds_the_riichi_han() {
        // 同じ枝のリーチ打点にはリーチ1翻が入る。30符2翻 2000 に対し 30符3翻 3900。
        let value = RED_FIVE.evaluated("1p", "3m");

        assert_eq!(
            variants(&value.damaten),
            vec![
                ("2s".to_string(), 4, Some(2000)),
                ("5sr".to_string(), 1, Some(3900)),
                ("5s".to_string(), 3, Some(2000)),
            ]
        );
        assert_eq!(
            variants(&value.reach),
            vec![
                ("2s".to_string(), 4, Some(3900)),
                ("5sr".to_string(), 1, Some(7700)),
                ("5s".to_string(), 3, Some(3900)),
            ]
        );
    }

    #[test]
    fn the_reach_baseline_is_the_existing_one() {
        // リーチ baseline は PR #173 で導入した既存 policy そのもの。
        let reach = &RED_FIVE.evaluated("1p", "3m").reach;

        assert_eq!(reach.baseline, reach_baseline_context(&RED_FIVE.ctx));
        assert_eq!(reach.baseline.riichi(), RiichiStatus::Riichi);
        assert_eq!(reach.baseline.ippatsu(), Some(false));
        assert_eq!(reach.baseline.chankan(), Some(false));
        assert!(!reach.baseline.is_last_live_tile());
    }

    #[test]
    fn the_reach_value_is_fixed_with_no_ura_dora() {
        // 裏ドラ表示牌は未観測ではなく「観測済みで0枚」。裏ドラ未確定にしない。
        assert!(BASELINE_URA_DORA_INDICATORS.is_empty());

        for variant in RED_FIVE.evaluated("1p", "3m").reach.winning_tile_values() {
            assert!(
                matches!(variant.value, TenpaiVariantValue::Known { .. }),
                "{variant:?}"
            );
        }
    }

    #[test]
    fn a_damaten_hand_without_yaku_is_not_zero() {
        // ダマ役なしは0点ではなく NoYaku。加重平均も0にせず確定しないままにする。
        let value = NO_YAKU.evaluated("1p", "3m");

        assert_eq!(
            variants(&value.damaten),
            vec![("2s".to_string(), 4, None)],
            "2s は嵌張で断幺も平和も付かない"
        );
        assert!(
            value
                .damaten
                .winning_tile_values()
                .all(|variant| variant.value == TenpaiVariantValue::NoYaku)
        );
        assert_eq!(value.damaten.weighted_average, OffenseValue::Unknown);

        // 同じ枝でもリーチならリーチ1翻で役が付く。
        assert_eq!(
            variants(&value.reach),
            vec![("2s".to_string(), 4, Some(2600))]
        );
    }

    #[test]
    fn red_and_black_winning_fives_are_separate_variants() {
        // 和了牌の赤5 / 黒5は既存 TenpaiHandValueProfile の variant 分割そのままで、赤ドラ1翻分
        // 打点が変わる。variant の残枚数の合計は待ち全体の残枚数と一致する。
        let damaten = &RED_FIVE.evaluated("1p", "3m").damaten;
        let wait = damaten
            .waits
            .iter()
            .find(|wait| wait.winning_tile == tile("5s"))
            .expect("5s の待ちがある");

        assert_eq!(wait.remaining, 4);
        assert_eq!(
            wait.winning_tiles
                .iter()
                .map(|variant| u32::from(variant.remaining))
                .sum::<u32>(),
            u32::from(wait.remaining)
        );
        let [red, black] = wait.winning_tiles.as_slice() else {
            panic!("赤 / 黒の2 variant に分かれる: {:?}", wait.winning_tiles);
        };
        assert!(red.is_red());
        assert!(!black.is_red());
        assert_eq!((red.remaining, red.value.total()), (1, Some(3900)));
        assert_eq!((black.remaining, black.value.total()), (3, Some(2000)));
    }

    #[test]
    fn a_seen_red_five_leaves_only_the_black_variant() {
        // 赤5が既に見えていれば赤 variant は生きていないので、集約にも入れない。
        let damaten = &RED_FIVE_SEEN.evaluated("1p", "3m").damaten;
        let wait = damaten
            .waits
            .iter()
            .find(|wait| wait.winning_tile == tile("5s"))
            .expect("5s の待ちがある");

        assert_eq!(wait.remaining, 3);
        assert_eq!(
            wait.winning_tiles.iter().map(variant).collect::<Vec<_>>(),
            vec![("5s".to_string(), 3, Some(2000))]
        );
    }

    #[test]
    fn every_wait_keeps_its_own_payment() {
        // 複数待ちは待ちごとに別の支払いを持ち、平均へ潰さずに保持する。
        let value = RED_FIVE.evaluated("1p", "2s");

        assert_eq!(
            variants(&value.damaten),
            vec![
                ("3m".to_string(), 3, Some(2000)),
                ("6m".to_string(), 3, Some(2000)),
                ("9m".to_string(), 4, Some(7700)),
            ]
        );
        // (2000 * 3 + 2000 * 3 + 7700 * 4) / 10 = 4280。
        assert_eq!(
            value.damaten.weighted_average,
            OffenseValue::Known {
                weighted_total: 42800,
                total_remaining: 10,
            }
        );
        assert_eq!(value.damaten.weighted_average.average_total(), Some(4280));
    }

    #[test]
    fn a_variant_without_remaining_is_not_aggregated() {
        // 残枚数0の variant は生きていないので、確定していてもいなくても集約へ入れない。
        let live = |variants: &[(TenpaiVariantValue, u8)]| {
            weighted_average(
                variants
                    .iter()
                    .map(|(value, remaining)| (value.total(), *remaining)),
            )
        };

        let with_dead = live(&[(known(7700), 4), (TenpaiVariantValue::NoYaku, 0)]);
        assert_eq!(with_dead, live(&[(known(7700), 4)]));
        assert_eq!(with_dead.average_total(), Some(7700));
        // 生きた役なしは0点として平均へ入れず、加重平均そのものを確定しないままにする。
        assert_eq!(
            live(&[(known(7700), 4), (TenpaiVariantValue::NoYaku, 1)]),
            OffenseValue::Unknown
        );
    }

    #[test]
    fn a_missing_scoring_context_stays_unknown() {
        // 場風・自風が不明な局面では推測せず、点数計算のエラーをそのまま保持する。
        let damaten = &UNKNOWN_WINDS.evaluated("1p", "3m").damaten;

        assert!(!damaten.waits.is_empty(), "待ちそのものは求まる");
        for variant in damaten.winning_tile_values() {
            assert_eq!(
                variant.value,
                TenpaiVariantValue::Unknown(TenpaiVariantUnknownReason::Scoring(
                    NormalScoringError::IncompleteContext(MissingScoringFact::RoundWind).into()
                )),
                "{variant:?}"
            );
        }
        assert_eq!(damaten.weighted_average, OffenseValue::Unknown);
        assert_eq!(
            UNKNOWN_WINDS.evaluated("1p", "3m").reach.weighted_average,
            OffenseValue::Unknown
        );
    }

    #[test]
    fn a_red_five_draw_is_split_into_physical_variants() {
        // 仮想ツモ牌の赤5は確率を推測するのではなく、赤 / 黒の物理牌 variant へ分けて別々に
        // 評価する。残枚数の合計は牌種単位の残枚数と一致する。
        let draw = RED_FIVE.draw("1p", "5s");

        assert_eq!(
            draw.variants
                .iter()
                .map(|variant| u32::from(variant.remaining))
                .sum::<u32>(),
            u32::from(draw.remaining),
        );
        let red = RED_FIVE.red_variant("1p", "5s", true);
        let black = RED_FIVE.red_variant("1p", "5s", false);
        assert_eq!(red.remaining, 1);
        assert_eq!(black.remaining, draw.remaining - 1);
        assert!(red.outcome.evaluated().is_some());
        assert!(black.outcome.evaluated().is_some());
    }

    #[test]
    fn a_seen_red_five_draw_has_only_the_black_variant() {
        // 赤5が既に見えていれば仮想ツモ牌は黒に確定し、variant は1つだけになる。
        let draw = RED_FIVE_SEEN.draw("1p", "5s");

        assert_eq!(draw.variants.len(), 1);
        assert!(!draw.variants[0].drawn_tile.is_red());
        assert!(draw.variants[0].outcome.evaluated().is_some());
    }

    #[test]
    fn a_branch_that_is_not_tenpai_has_no_value() {
        // 2手目の打牌後がテンパイでなければ将来打点を持たない。
        let variant = RED_FIVE.variant("1m", "9m");

        assert!(variant.next_discard.is_some());
        assert_eq!(variant.outcome, ProspectiveOutcome::NotTenpai);
        assert!(variant.outcome.evaluated().is_none());
        assert_eq!(variant.selection_value, None);
    }

    #[test]
    fn a_branch_without_a_next_discard_has_no_value() {
        // 2手目の打牌候補が無い枝も将来打点を持たない。
        let mut lookahead = RED_FIVE.lookahead.clone();
        for candidate in &mut lookahead.candidates {
            for draw in &mut candidate.draws {
                for variant in &mut draw.variants {
                    variant.next_discard = None;
                }
            }
        }

        let value = evaluate_prospective_lookahead_value(
            &RED_FIVE.ctx,
            &hand_tiles(&RED_FIVE.ctx),
            &evaluations(&RED_FIVE.lookahead, &RED_FIVE.ctx),
            &lookahead,
        );

        for variant in value
            .candidates
            .iter()
            .flat_map(|candidate| candidate.draws.iter())
            .flat_map(|draw| draw.variants.iter())
        {
            assert_eq!(variant.next_discard, None);
            assert_eq!(variant.outcome, ProspectiveOutcome::NoNextDiscard);
        }
    }

    #[test]
    fn the_prospective_value_follows_the_selected_next_discard() {
        // 評価対象は2手先評価が打点込みの比較で選んだ2手目打牌そのもの。診断側で選び直さない。
        assert_eq!(
            RED_FIVE.value.candidates.len(),
            RED_FIVE.lookahead.candidates.len()
        );
        for (value, lookahead) in RED_FIVE
            .value
            .candidates
            .iter()
            .zip(RED_FIVE.lookahead.candidates.iter())
        {
            assert_eq!(value.discard, lookahead.discard);
            assert_eq!(value.draws.len(), lookahead.draws.len());
            for (draw, branch) in value.draws.iter().zip(lookahead.draws.iter()) {
                assert_eq!(draw.draw, branch.draw);
                assert_eq!(draw.remaining, branch.remaining);
                assert_eq!(draw.variants.len(), branch.variants.len());
                for (variant, branch_variant) in draw.variants.iter().zip(branch.variants.iter()) {
                    assert_eq!(variant.drawn_tile, branch_variant.drawn_tile);
                    assert_eq!(variant.remaining, branch_variant.remaining);
                    assert_eq!(variant.next_discard, branch_variant.next_discard_tile());
                    assert_eq!(variant.selection_value, branch_variant.prospective_value);
                }
            }
        }
    }

    #[test]
    fn mismatched_candidates_are_not_guessed() {
        // 候補の順序・件数が対応しない入力では推測せず、枝を持たない診断にする。
        let value = evaluate_prospective_lookahead_value(
            &RED_FIVE.ctx,
            &hand_tiles(&RED_FIVE.ctx),
            &[],
            &RED_FIVE.lookahead,
        );

        assert!(value.candidates.is_empty());
    }

    // 打牌前の全物理牌 (手牌 + ツモ牌)。
    fn hand_tiles(context: &GameContext) -> Vec<TileId> {
        context
            .hand_tiles()
            .iter()
            .copied()
            .chain(context.drawn_tile())
            .collect()
    }

    // 11m 2m 33m 7m 8m 9m 2p 3p 4p 7p 9p E の1向聴。ドラ表示牌 2m でドラは 3m。
    //
    // 打 7p から 2m を引いて E を切ると 9p 単騎テンパイになり、ダマ 5200 / リーチ 8000 になる。
    const LOW_DAMATEN_HAND: [&str; 14] = [
        "3p", "4p", "9p", "1m", "8m", "1m", "2p", "3m", "7m", "3m", "2m", "9m", "E", "7p",
    ];

    // 1m 2m 3m 4m 4m 5m 6m 8m 9m 2p 2p 4p 7p 8p 9p 相当の1向聴。ドラ表示牌 1p でドラは 2p。
    //
    // 打 8m から赤5p を引いて 9m を切ると 3p / 6p の両面テンパイになり、ダマ 7700 / リーチ 8000。
    const HIGH_DAMATEN_HAND: [&str; 14] = [
        "3m", "9m", "5m", "8p", "2m", "2p", "6m", "2p", "1m", "9p", "8m", "4p", "4m", "7p",
    ];

    static LOW_DAMATEN: LazyLock<Case> =
        LazyLock::new(|| case_with_dora(&LOW_DAMATEN_HAND, "2m", &[], true));
    static HIGH_DAMATEN: LazyLock<Case> =
        LazyLock::new(|| case_with_dora(&HIGH_DAMATEN_HAND, "1p", &[], true));

    // 同じ局面で、未来テンパイの待ち 3p を自分が既に捨てている。恒常フリテンが確定するので
    // ダマではロンできない。
    static HIGH_DAMATEN_FURITEN: LazyLock<Case> = LazyLock::new(|| {
        CaseSpec {
            own_river: &["3p"],
            ..CaseSpec::new(&HIGH_DAMATEN_HAND, "1p")
        }
        .build()
    });

    // 同じ局面で、履歴依存フリテンが未観測。恒常フリテンは非フリテンでも総合ロン可否は
    // 確定しない。
    static HIGH_DAMATEN_UNKNOWN_RON: LazyLock<Case> = LazyLock::new(|| {
        CaseSpec {
            history_furiten: HistoryFuritenFacts::default(),
            ..CaseSpec::new(&HIGH_DAMATEN_HAND, "1p")
        }
        .build()
    });

    // ロン可否が確定せず、持ち点が足りずリーチも打てない局面。攻撃モードはダマになるが、
    // ダマ Ron baseline を確定値として使えない。
    static HIGH_DAMATEN_NO_REACH: LazyLock<Case> = LazyLock::new(|| {
        CaseSpec {
            history_furiten: HistoryFuritenFacts::default(),
            table_state: TableStateFacts {
                scores: Some([500, 25_000, 25_000, 25_000]),
                ..TableStateFacts::default()
            },
            ..CaseSpec::new(&HIGH_DAMATEN_HAND, "1p")
        }
        .build()
    });

    fn totals(baseline: &ProspectiveBaselineValue) -> Vec<(u8, Option<u32>)> {
        baseline
            .winning_tile_values()
            .map(|value| (value.remaining, value.value.total()))
            .collect()
    }

    #[test]
    fn a_low_damaten_branch_uses_the_reach_value() {
        // ダマ 5200 / リーチ 8000 で production 判断がリーチなら、選択値もリーチ打点で作る。
        let variant = LOW_DAMATEN.variant("7p", "2m");
        let tenpai = variant.outcome.evaluated().expect("テンパイ枝");

        assert_eq!(totals(&tenpai.damaten), vec![(3, Some(5200))]);
        assert_eq!(totals(&tenpai.reach), vec![(3, Some(8000))]);
        assert_eq!(tenpai.mode, TenpaiOffenseMode::Reach);
        assert_eq!(variant.selection_value, Some(3 * 8000));
    }

    #[test]
    fn a_high_damaten_branch_uses_the_damaten_value() {
        // ダマでロンできると確定していて、ダマ 7700 / リーチ 8000。production 判断はダマなので
        // 選択値もダマ打点で作る。
        let variant = HIGH_DAMATEN.red_variant("8m", "5p", true);
        let tenpai = variant.outcome.evaluated().expect("テンパイ枝");

        assert_eq!(tenpai.can_ron, Some(true));
        assert_eq!(
            totals(&tenpai.damaten),
            vec![(4, Some(7700)), (4, Some(7700))]
        );
        assert_eq!(
            totals(&tenpai.reach),
            vec![(4, Some(8000)), (4, Some(8000))]
        );
        assert_eq!(tenpai.mode, TenpaiOffenseMode::Damaten);
        assert_eq!(variant.selection_value, Some(8 * 7700));
    }

    #[test]
    fn a_known_permanent_furiten_does_not_use_the_damaten_verdict() {
        // 未来テンパイの待ちを自分が既に捨てていれば、既存 furiten helper が恒常フリテンを
        // 確定させる。ダマではロンできないのでダマ打点を判断材料にしない。
        let variant = HIGH_DAMATEN_FURITEN.red_variant("8m", "5p", true);
        let tenpai = variant.outcome.evaluated().expect("テンパイ枝");

        assert_eq!(tenpai.can_ron, Some(false));
        // フリテンでも打点そのものは変わらない。将来フリテンに倍率や EV 補正を掛けていない。
        assert_eq!(
            damaten_totals(tenpai),
            damaten_totals(
                HIGH_DAMATEN
                    .red_variant("8m", "5p", true)
                    .outcome
                    .evaluated()
                    .expect("テンパイ枝")
            ),
        );
        // ダマ打点そのものは threshold 以上のまま。それでもダマは選ばない。
        assert!(damaten_meets_threshold(tenpai));
        assert_eq!(tenpai.mode, TenpaiOffenseMode::Reach);
        assert_eq!(
            variant.selection_value,
            reach_weighted_total(tenpai),
            "リーチ baseline の打点で選択値を作る"
        );
    }

    #[test]
    fn an_unknown_ron_availability_does_not_use_the_damaten_verdict() {
        // ロン可否が確定しない枝では、ダマ打点が threshold 以上でも HighValueDamaten と
        // 確定しない。既存の待ち枚数だけを見る fallback へ委ねる。
        let variant = HIGH_DAMATEN_UNKNOWN_RON.red_variant("8m", "5p", true);
        let tenpai = variant.outcome.evaluated().expect("テンパイ枝");

        assert_eq!(tenpai.can_ron, None);
        assert!(damaten_meets_threshold(tenpai));
        assert_eq!(tenpai.mode, TenpaiOffenseMode::Reach);
        assert_eq!(variant.selection_value, reach_weighted_total(tenpai));
    }

    #[test]
    fn a_damaten_branch_without_a_certain_ron_has_no_value() {
        // 合法リーチが無くダマになる枝でも、ロンできると確定しない限りダマ Ron baseline を
        // 確定値として使わない。0点にもせず、打点を持たないままにする。
        let variant = HIGH_DAMATEN_NO_REACH.red_variant("8m", "5p", true);
        let tenpai = variant.outcome.evaluated().expect("テンパイ枝");

        assert_eq!(tenpai.can_ron, None);
        assert_eq!(tenpai.mode, TenpaiOffenseMode::Damaten);
        assert!(
            tenpai
                .damaten
                .winning_tile_values()
                .all(|value| value.value.total().is_some()),
            "ダマ打点そのものは求まる"
        );
        assert_eq!(variant.selection_value, None);
    }

    fn damaten_totals(tenpai: &ProspectiveTenpaiValue) -> Vec<Option<u32>> {
        tenpai
            .damaten
            .winning_tile_values()
            .map(|value| value.value.total())
            .collect()
    }

    // 全ての生きた待ちのダマ打点が threshold 以上か。
    fn damaten_meets_threshold(tenpai: &ProspectiveTenpaiValue) -> bool {
        tenpai.damaten.winning_tile_values().all(|value| {
            value
                .value
                .total()
                .is_some_and(|total| total >= DAMATEN_MIN_TOTAL)
        })
    }

    // リーチ baseline での Σ(和了牌 variant 残枚数 × 支払い合計)。
    fn reach_weighted_total(tenpai: &ProspectiveTenpaiValue) -> Option<u64> {
        tenpai
            .reach
            .winning_tile_values()
            .map(|value| Some(u64::from(value.value.total()?) * u64::from(value.remaining)))
            .sum()
    }

    #[test]
    fn the_reach_and_damaten_choice_uses_the_existing_threshold() {
        // 攻撃モードの分かれ目は既存 policy の threshold そのもので、この層に新しい threshold を
        // 作らない。
        let low = LOW_DAMATEN
            .variant("7p", "2m")
            .outcome
            .evaluated()
            .expect("テンパイ枝");
        let high = HIGH_DAMATEN
            .red_variant("8m", "5p", true)
            .outcome
            .evaluated()
            .expect("テンパイ枝");

        assert!(low.damaten.winning_tile_values().all(|value| {
            value
                .value
                .total()
                .is_some_and(|total| total < DAMATEN_MIN_TOTAL)
        }));
        assert!(high.damaten.winning_tile_values().all(|value| {
            value
                .value
                .total()
                .is_some_and(|total| total >= DAMATEN_MIN_TOTAL)
        }));
        assert_eq!(low.mode, TenpaiOffenseMode::Reach);
        assert_eq!(high.mode, TenpaiOffenseMode::Damaten);
        // どちらもダマでロンできると確定した枝。threshold 判定はその場合だけ効く。
        assert_eq!(low.can_ron, Some(true));
        assert_eq!(high.can_ron, Some(true));
    }

    #[test]
    fn an_unknown_branch_value_is_not_zero() {
        // 点数計算の入力が足りない局面では打点を確定できないので、0点ではなく値を持たない。
        let variant = UNKNOWN_WINDS.variant("1p", "3m");

        assert!(variant.outcome.evaluated().is_some(), "テンパイ枝ではある");
        assert_eq!(variant.selection_value, None);
    }

    #[test]
    fn an_unknown_value_leaves_the_weighted_wait_to_decide() {
        // 打点を確定できない局面では打点込みの集計値を持たず、既存 weighted wait へ委ねる。
        let metrics = UNKNOWN_WINDS.metrics();

        assert!(
            metrics.iter().any(|metric| metric.tenpai_wait.is_some()),
            "既存 weighted wait は求まる"
        );
        assert!(
            metrics
                .iter()
                .all(|metric| metric.prospective_value.is_none())
        );
    }

    // 2手先診断と同じ順序・同じ牌種の打牌候補評価。物理牌の赤フラグは本番評価と同じ値になる。
    fn evaluations(
        lookahead: &LookaheadDiagnostic,
        context: &GameContext,
    ) -> Vec<DiscardEvaluation> {
        let actions: Vec<LegalAction> = hand_tiles(context)
            .into_iter()
            .map(|tile| LegalAction::Dahai { tile })
            .collect();
        let evaluations = select_discard_action_with_diagnostic(
            context,
            &actions,
            LookaheadDiagnosticScope::None,
        )
        .diagnostic
        .candidates
        .into_iter()
        .map(|candidate| candidate.evaluation)
        .collect::<Vec<_>>();

        assert_eq!(evaluations.len(), lookahead.candidates.len());
        evaluations
    }

    // ---- same-shanten の枝の先にあるテンパイ ----

    // 深い枝1件分の最終テンパイ。1手目・2手目・3手目に切った物理牌とテンパイ時点の concealed
    // 手牌は既存 helper ([`split_discarded_tile`]) だけで組み立て、待ちは3手目の打牌評価が持つ
    // 受け入れそのもの。
    struct DownstreamBranch {
        concealed: Vec<TileId>,
        discarded: Vec<TileId>,
        acceptance: EffectiveAcceptance,
        // 2手先評価が枝に持たせた将来打点。
        value: Option<u64>,
    }

    impl DownstreamBranch {
        fn tenpai(&self) -> ProspectiveTenpai<'_> {
            ProspectiveTenpai {
                concealed_tiles: &self.concealed,
                acceptance: &self.acceptance,
                discarded_tiles: &self.discarded,
            }
        }

        // 将来テンパイ時点で自分の河にある牌種。現在の河にこの3枚を足したものが判定材料になる。
        fn discarded_types(&self) -> Vec<TileType> {
            self.discarded.iter().map(|tile| tile.tile_type()).collect()
        }

        fn waits(&self) -> Vec<TileType> {
            self.acceptance.tiles.iter().map(|wait| wait.tile).collect()
        }
    }

    // 構築済みの2手先評価から、same-shanten の枝の先にある最終テンパイをすべて取り出す。
    // 探索も打牌比較もやり直さず、枝が持つ打牌評価から仮想手牌を組み立てるだけ。
    fn downstream_branches(case: &Case) -> Vec<DownstreamBranch> {
        let tiles = hand_tiles(&case.ctx);
        let evaluations = evaluations(&case.lookahead, &case.ctx);
        let mut branches = Vec::new();

        for (candidate, evaluation) in case.lookahead.candidates.iter().zip(evaluations.iter()) {
            let Some((first, after_first)) = split_discarded_tile(tiles.clone(), evaluation) else {
                continue;
            };
            for draw in candidate.draws_with(DrawTransition::SameShanten) {
                for variant in &draw.variants {
                    let (Some(downstream), Some(next)) =
                        (variant.downstream.as_ref(), variant.next_discard.as_ref())
                    else {
                        continue;
                    };
                    let mut after_draw = after_first.clone();
                    after_draw.push(variant.drawn_tile);
                    let Some((second, after_second)) = split_discarded_tile(after_draw, next)
                    else {
                        continue;
                    };

                    for downstream_draw in &downstream.draws {
                        for downstream_variant in &downstream_draw.variants {
                            let Some(third) = downstream_variant.next_discard.as_ref() else {
                                continue;
                            };
                            let mut after_downstream_draw = after_second.clone();
                            after_downstream_draw.push(downstream_variant.drawn_tile);
                            let Some((third_discarded, concealed)) =
                                split_discarded_tile(after_downstream_draw, third)
                            else {
                                continue;
                            };
                            branches.push(DownstreamBranch {
                                concealed,
                                discarded: vec![first, second, third_discarded],
                                acceptance: third.acceptance_after_discard.clone(),
                                value: downstream_variant.prospective_value,
                            });
                        }
                    }
                }
            }
        }
        branches
    }

    // 既存の 11m 2m 33m 7m 8m 9m 2p 3p 4p 7p 9p E の1向聴を、same-shanten の枝の先にある
    // テンパイまで追った case。深い探索は重いので、同じ case を複数のテストで共有する。
    static LOW_DAMATEN_DOWNSTREAM: LazyLock<Case> = LazyLock::new(|| {
        CaseSpec {
            downstream: true,
            ..CaseSpec::new(&LOW_DAMATEN_HAND, "2m")
        }
        .build()
    });

    #[test]
    fn a_downstream_tenpai_keeps_the_production_prospective_value() {
        // 深い枝が持つ将来打点は、同じテンパイを既存 production 評価器へ渡した値と一致する。
        let case = &*LOW_DAMATEN_DOWNSTREAM;
        let valuator = ProductionProspectiveValuator::new(&case.ctx);

        let branches = downstream_branches(case);
        assert!(!branches.is_empty(), "先の枝を持つ局面が必要");

        let mut known = 0;
        for branch in &branches {
            assert_eq!(branch.value, valuator.tenpai_value(&branch.tenpai()));
            known += usize::from(branch.value.is_some());
        }
        assert!(known > 0, "打点を確定できる枝がある局面が必要");
    }

    #[test]
    fn a_downstream_value_aggregates_into_the_candidate_metric() {
        // 候補ごとの集計値は Σ(same-shanten 残枚数 × Σ(3手目のツモ残枚数 × 最終テンパイの打点))。
        let case = &*LOW_DAMATEN_DOWNSTREAM;

        let mut aggregated = 0;
        for candidate in &case.lookahead.candidates {
            let Some(value) = candidate.same_shanten_downstream_value() else {
                continue;
            };
            let mut expected = 0u64;
            for draw in candidate.draws_with(DrawTransition::SameShanten) {
                for variant in &draw.variants {
                    expected += u64::from(variant.remaining)
                        * variant.downstream_value().expect("先の枝の打点がある");
                }
            }
            assert_eq!(value, expected, "{:?}", candidate.discard);
            assert!(value > 0);
            aggregated += 1;
        }
        assert!(aggregated > 0, "打点を確定できる候補がある局面が必要");
    }

    #[test]
    fn a_future_river_wait_is_furiten_for_the_downstream_tenpai() {
        // 途中で切った牌が最終待ちに含まれる枝は、既存フリテン基盤がロン不可と判断する。
        let case = &*LOW_DAMATEN_DOWNSTREAM;
        let valuator = ProductionProspectiveValuator::new(&case.ctx);
        assert!(
            case.ctx.own_discards() == Some(&[][..]),
            "現在の河が空で、将来の河だけでフリテンになる局面が必要"
        );

        let mut furiten = 0;
        for branch in downstream_branches(case) {
            let discarded = branch.discarded_types();
            let hits_river = branch.waits().iter().any(|wait| discarded.contains(wait));
            let facts = valuator
                .tenpai_facts(&branch.tenpai())
                .expect("完成手を解析できる");

            assert_eq!(facts.ron_availability(), Some(!hits_river));
            furiten += usize::from(hits_river);
        }
        assert!(furiten > 0, "将来の河でフリテンになる枝がある局面が必要");
    }

    #[test]
    fn following_the_same_shanten_branch_keeps_the_selection_unchanged() {
        // 深い枝を追っても、打牌選択の結果も選択に使う前方集計値も変わらない。
        let followed = &*LOW_DAMATEN_DOWNSTREAM;
        let plain = &*LOW_DAMATEN;

        assert_eq!(
            evaluations(&followed.lookahead, &followed.ctx),
            evaluations(&plain.lookahead, &plain.ctx),
        );
        assert_eq!(followed.metrics(), plain.metrics());

        // 2手先診断を作らない通常経路とも同じ action を選ぶ。
        let actions: Vec<LegalAction> = hand_tiles(&followed.ctx)
            .into_iter()
            .map(|tile| LegalAction::Dahai { tile })
            .collect();
        assert!(followed.selected.is_some());
        assert_eq!(followed.selected, plain.selected);
        assert_eq!(
            followed.selected,
            select_discard_action_with_diagnostic(
                &followed.ctx,
                &actions,
                LookaheadDiagnosticScope::None,
            )
            .selection
            .action,
        );
    }

    // ---- ツモ baseline の continuation ----

    // 山の残枚数が既知の局面。self-tsumo continuation の材料が揃う。
    fn tsumo_case(hand: &[&str], dora_indicator: &str, extra_visible: &[&str]) -> Case {
        CaseSpec {
            extra_visible,
            table_state: TableStateFacts {
                remaining_tiles: Some(60),
                ..TableStateFacts::default()
            },
            ..CaseSpec::new(hand, dora_indicator)
        }
        .build()
    }

    static TSUMO_NO_YAKU: LazyLock<Case> = LazyLock::new(|| tsumo_case(&NO_YAKU_HAND, "1m", &[]));
    static TSUMO_RED_FIVE: LazyLock<Case> = LazyLock::new(|| tsumo_case(&RED_FIVE_HAND, "1m", &[]));
    static TSUMO_LOW_DAMATEN: LazyLock<Case> =
        LazyLock::new(|| tsumo_case(&LOW_DAMATEN_HAND, "2m", &[]));
    static TSUMO_HIGH_DAMATEN: LazyLock<Case> =
        LazyLock::new(|| tsumo_case(&HIGH_DAMATEN_HAND, "1p", &[]));

    // ツモ打点を確定できない局面。場風・自風が分からず点数計算の入力が足りない。
    static TSUMO_UNKNOWN_WINDS: LazyLock<Case> = LazyLock::new(|| {
        CaseSpec {
            winds: false,
            table_state: TableStateFacts {
                remaining_tiles: Some(60),
                ..TableStateFacts::default()
            },
            ..CaseSpec::new(&RED_FIVE_HAND, "1m")
        }
        .build()
    });

    // 2手先評価の枝が持つツモ continuation。表示のためでも選択のためでも同じ値。
    fn continuation(case: &Case, discard: &str, draw: &str) -> Option<TenpaiTsumoValue> {
        case.lookahead
            .candidate(tile(discard))
            .expect("打牌候補がある")
            .draw(tile(draw))
            .expect("受け入れ牌の枝がある")
            .variants
            .first()
            .expect("物理牌 variant がある")
            .tsumo_continuation
    }

    #[test]
    fn the_tsumo_baseline_is_not_the_ron_baseline() {
        // ロン baseline の値を流用せず、門前ツモが付いた別の打点になる。
        let tenpai = TSUMO_RED_FIVE.evaluated("1p", "3m");
        assert!(tenpai.reach.baseline.win_method().is_ron());

        let tsumo = continuation(&TSUMO_RED_FIVE, "1p", "3m").expect("ツモ打点を確定できる");
        let ron_weighted: u64 = tenpai
            .reach
            .winning_tile_values()
            .map(|variant| {
                u64::from(variant.value.total().unwrap_or_default()) * u64::from(variant.remaining)
            })
            .sum();

        assert_eq!(tsumo.winning_remaining, 8);
        assert_ne!(tsumo.weighted_total, ron_weighted);
    }

    #[test]
    fn the_tsumo_baseline_keeps_the_production_offense_mode() {
        // 攻撃モードは既存 production policy の結論そのままで、ここで別の判断を作らない。
        let mode = TSUMO_RED_FIVE.evaluated("1p", "3m").mode;
        let (baseline, ura_dora) =
            tsumo_scoring_inputs(&TSUMO_RED_FIVE.ctx, mode).expect("baseline を作れる");

        assert_eq!(mode, TenpaiOffenseMode::Reach);
        assert!(baseline.win_method().is_tsumo());
        assert_eq!(baseline.riichi(), RiichiStatus::Riichi);
        // 裏ドラは既存の最低保証 baseline (裏0) と揃える。
        assert_eq!(ura_dora, Some(BASELINE_URA_DORA_INDICATORS));
    }

    #[test]
    fn forced_reach_reuses_the_reach_baseline_when_production_selects_damaten() {
        let case = &*TSUMO_HIGH_DAMATEN;
        let production = continuation(case, "8m", "5p").expect("production ツモ打点がある");
        let tenpai = case
            .red_variant("8m", "5p", true)
            .outcome
            .evaluated()
            .expect("テンパイ枝");

        assert_eq!(tenpai.mode, TenpaiOffenseMode::Damaten);
        assert!(tenpai.future_reach_legal);
        assert_eq!(tenpai.forced_damaten_tsumo, Some(production));
        assert!(tenpai.forced_reach_tsumo.is_some());
        assert_ne!(tenpai.forced_reach_tsumo, Some(production));
    }

    #[test]
    fn forced_damaten_reuses_the_damaten_baseline_when_production_selects_reach() {
        let case = &*TSUMO_LOW_DAMATEN;
        let production = continuation(case, "7p", "2m").expect("production ツモ打点がある");
        let tenpai = case.evaluated("7p", "2m");

        assert_eq!(tenpai.mode, TenpaiOffenseMode::Reach);
        assert!(tenpai.future_reach_legal);
        assert_eq!(tenpai.forced_reach_tsumo, Some(production));
        assert!(tenpai.forced_damaten_tsumo.is_some());
        assert_ne!(tenpai.forced_damaten_tsumo, Some(production));
    }

    #[test]
    fn a_damaten_tsumo_baseline_has_no_riichi_han() {
        let (baseline, ura_dora) =
            tsumo_scoring_inputs(&TSUMO_RED_FIVE.ctx, TenpaiOffenseMode::Damaten)
                .expect("baseline を作れる");

        assert!(baseline.win_method().is_tsumo());
        assert_eq!(baseline.riichi(), RiichiStatus::NotDeclared);
        assert_eq!(ura_dora, None);
    }

    #[test]
    fn an_unknown_offense_mode_has_no_tsumo_baseline() {
        assert!(tsumo_scoring_inputs(&TSUMO_RED_FIVE.ctx, TenpaiOffenseMode::Unknown).is_none());
    }

    #[test]
    fn the_red_five_variants_are_aggregated_by_physical_tile() {
        // 赤5 / 黒5で打点が違う待ちを、牌種へ潰さず variant ごとに重み付けする。
        let tenpai = TSUMO_RED_FIVE.evaluated("1p", "3m");
        let reach: Vec<_> = tenpai
            .reach
            .winning_tile_values()
            .map(|variant| (variant.winning_tile.is_red(), variant.remaining))
            .collect();
        assert_eq!(reach, vec![(false, 4), (true, 1), (false, 3)]);

        let tsumo = continuation(&TSUMO_RED_FIVE, "1p", "3m").expect("ツモ打点を確定できる");
        // 2s 4枚 + 5s 4枚 (赤1枚 + 黒3枚)。赤5だけ打点が高いので、牌種単位へ潰した集約とは
        // 一致しない。
        assert_eq!(tsumo.winning_remaining, 8);
    }

    #[test]
    fn a_seen_red_five_changes_the_weighted_tsumo_payment() {
        // 赤5が既に見えている局面では、その1枚が待ちから消え、重み付き打点も赤の分だけ下がる。
        let seen = tsumo_case(&RED_FIVE_HAND, "1m", &["5sr"]);
        let with_red = continuation(&TSUMO_RED_FIVE, "1p", "3m").expect("ツモ打点を確定できる");
        let without_red = continuation(&seen, "1p", "3m").expect("ツモ打点を確定できる");

        assert_eq!(
            with_red.winning_remaining,
            without_red.winning_remaining + 1
        );
        assert!(with_red.weighted_total > without_red.weighted_total);
        // 赤5以外の variant の打点は変わらないので、平均は赤がある側の方が高い。
        assert!(
            u64::from(with_red.winning_remaining) * without_red.weighted_total
                < u64::from(without_red.winning_remaining) * with_red.weighted_total
        );
    }

    #[test]
    fn the_menzen_tsumo_yaku_comes_from_the_existing_hand_value() {
        // ダマ (ロン) では役なしになり得る待ちでも、門前ツモは既存の役判定が付ける。この層で
        // 1翻を足していないことを、ロン baseline との違いで確認する。
        let damaten = &TSUMO_NO_YAKU.evaluated("1p", "9m").damaten;
        assert_eq!(variants(damaten), vec![("2s".to_string(), 4, Some(5200))]);

        let tsumo = continuation(&TSUMO_NO_YAKU, "1p", "9m").expect("ツモ打点を確定できる");
        assert_eq!(tsumo.winning_remaining, 4);
        assert!(tsumo.weighted_total > 0);
    }

    #[test]
    fn an_unknown_scoring_input_makes_the_continuation_unknown() {
        // 点数計算の入力が足りない局面では、推測で打点を作らず continuation を持たない。
        let unknown = &*TSUMO_UNKNOWN_WINDS;
        assert!(
            unknown
                .lookahead
                .candidates
                .iter()
                .flat_map(|candidate| candidate.draws.iter())
                .flat_map(|draw| draw.variants.iter())
                .filter(|variant| variant
                    .next_discard
                    .as_ref()
                    .is_some_and(|next| next.min_shanten_after_discard() == TENPAI_SHANTEN))
                .all(|variant| variant.tsumo_continuation.is_none())
        );
        assert!(
            unknown
                .metrics()
                .iter()
                .all(|metric| metric.expected_self_tsumo_value.is_none())
        );
    }

    #[test]
    fn an_unknown_remaining_wall_leaves_the_new_axis_unavailable() {
        // 山の残枚数が unknown な局面では新しい軸を使わず、ツモ点数計算も行わない。
        let unknown = &*RED_FIVE;
        assert_eq!(unknown.ctx.remaining_tiles(), None);
        assert!(continuation(unknown, "1p", "3m").is_none());
        assert!(
            unknown
                .metrics()
                .iter()
                .all(|metric| metric.expected_self_tsumo_value.is_none())
        );
    }

    #[test]
    fn the_iishanten_candidates_share_the_same_unknown_pool() {
        // 未確認牌の総数は打牌候補によらず同じで、残り自摸機会は山の残枚数の4分の1。
        let case = &*TSUMO_RED_FIVE;
        let tiles: Vec<TileId> = hand_tiles(&case.ctx);
        let valuator = ProductionProspectiveValuator::new(&case.ctx);
        let facts = lookahead_inputs(&case.ctx, &tiles, &valuator, LookaheadDiagnosticScope::None)
            .self_tsumo_facts()
            .expect("材料が揃っている");

        assert_eq!(facts.own_future_draws, 15);
        // 手牌14枚 + ドラ表示牌1枚が見えている。
        assert_eq!(facts.unknown_tiles, 121);
    }

    #[test]
    fn the_expected_self_tsumo_value_reaches_the_iishanten_candidates() {
        let metrics = TSUMO_RED_FIVE.metrics();
        assert!(metrics.iter().any(|metric| {
            metric
                .expected_self_tsumo_value
                .is_some_and(|value| value > 0)
        }));
    }

    // 卓・局の事実だけを固定した base 局面。副露状態を評価器へ明示的に渡す test のために、
    // context 側の副露は呼び出し側が指定する。
    fn hand_state_context(
        hand: Vec<TileId>,
        dora_indicators: Vec<TileId>,
        own_melds: Vec<Meld>,
    ) -> GameContext {
        let visible: Vec<TileId> = hand.iter().chain(dora_indicators.iter()).copied().collect();
        GameContext::from_parts_with_melds(
            None,
            hand,
            dora_indicators,
            Some(tile("E")),
            Some(tile("S")),
            visible,
            Some(0),
            Some(3),
            Default::default(),
            [false; 4],
            [own_melds, Vec::new(), Vec::new(), Vec::new()],
        )
        .with_table_state_facts(TableStateFacts {
            scores: Some([25000; 4]),
            ..TableStateFacts::default()
        })
        .with_history_furiten_facts(known_history_furiten())
    }

    // 白ポン1件。仮想的な鳴きの後の evaluation hand state として渡す。
    fn haku_pon(source: &mut TileIdSource) -> Vec<Meld> {
        let tiles = source.tiles(&["P", "P", "P"]);
        let called = tiles[0];
        vec![Meld::new(MeldKind::Pon, tiles, Some(called))]
    }

    // 123s のチー1件。
    fn chi_123s(source: &mut TileIdSource) -> Meld {
        let tiles = source.tiles(&["1s", "2s", "3s"]);
        let called = tiles[0];
        Meld::new(MeldKind::Chi, tiles, Some(called))
    }

    fn fixed(count: u8) -> FixedMeldCount {
        FixedMeldCount::new(count).expect("副露済み面子数として読める")
    }

    // 副露済み面子数が届いた枝だけがテンパイまで進むので、その枝数を数える。
    fn tenpai_branch_count(lookahead: &LookaheadDiagnostic) -> usize {
        lookahead
            .candidates
            .iter()
            .flat_map(|candidate| candidate.draws.iter())
            .flat_map(|draw| draw.variants.iter())
            .filter(|variant| {
                variant
                    .next_discard
                    .as_ref()
                    .is_some_and(|next| next.min_shanten_after_discard() == TENPAI_SHANTEN)
            })
            .count()
    }

    // 現在 context の副露状態を明示的に渡した場合と、context から取る既存入口の評価状態が
    // 一致することの確認。unknown / 門前 / 副露済みのどれでも同じ hand state になる。
    fn assert_matches_context_hand_state(context: &GameContext) {
        let from_context = ProductionProspectiveValuator::new(context);
        let explicit =
            ProductionProspectiveValuator::new_with_hand_state(context, context.own_melds());

        assert_eq!(from_context.melds, explicit.melds);
        assert_eq!(from_context.fixed_meld_count, explicit.fixed_meld_count);
        assert_eq!(from_context.reach_legal, explicit.reach_legal);
    }

    #[test]
    fn the_context_constructor_keeps_the_known_menzen_hand_state() {
        // 副露0件と分かっている局面。
        let mut source = TileIdSource::new();
        let context = hand_state_context(
            source.tiles(&["1m", "2m", "3m"]),
            source.tiles(&["1p"]),
            Vec::new(),
        );
        let valuator = ProductionProspectiveValuator::new(&context);

        assert_eq!(valuator.melds, Some([].as_slice()));
        assert_eq!(valuator.fixed_meld_count, FixedMeldCount::NONE);
        assert!(valuator.reach_legal);
        assert_matches_context_hand_state(&context);
    }

    #[test]
    fn the_context_constructor_keeps_the_known_open_hand_state() {
        // 副露済みと分かっている局面。門前でないのでリーチは合法にならない。
        let mut source = TileIdSource::new();
        let melds = haku_pon(&mut source);
        let context = hand_state_context(
            source.tiles(&["1m", "2m", "3m"]),
            source.tiles(&["1p"]),
            melds.clone(),
        );
        let valuator = ProductionProspectiveValuator::new(&context);

        assert_eq!(valuator.melds, Some(melds.as_slice()));
        assert_eq!(valuator.fixed_meld_count, fixed(1));
        assert!(!valuator.reach_legal);
        assert_matches_context_hand_state(&context);
    }

    #[test]
    fn the_context_constructor_keeps_the_unknown_hand_state() {
        // 自分の席を特定できず副露が unknown な局面。副露0件と確定したことにしない。
        let mut source = TileIdSource::new();
        let context = GameContext::from_parts_with_dora(
            None,
            source.tiles(&["1m", "2m", "3m"]),
            source.tiles(&["1p"]),
        );
        let valuator = ProductionProspectiveValuator::new(&context);

        assert_eq!(context.own_melds(), None);
        assert_eq!(valuator.melds, None);
        assert_eq!(valuator.melds.map(is_menzen), None);
        assert_eq!(valuator.fixed_meld_count, FixedMeldCount::NONE);
        assert_matches_context_hand_state(&context);
    }

    #[test]
    fn an_explicit_open_hand_state_makes_the_future_reach_illegal() {
        // base 局面自体は門前でリーチ条件を満たすが、評価対象の hand state が副露済みなら
        // 将来リーチは合法にならない。
        let mut source = TileIdSource::new();
        let melds = haku_pon(&mut source);
        let context = hand_state_context(
            source.tiles(&["1m", "2m", "3m"]),
            source.tiles(&["1p"]),
            Vec::new(),
        );

        assert_eq!(context.own_melds(), Some([].as_slice()));
        assert!(ProductionProspectiveValuator::new(&context).reach_legal);

        // 副露済み面子数は渡した副露から導出されるので、テスト側から別入力しない。
        let valuator = ProductionProspectiveValuator::new_with_hand_state(&context, Some(&melds));

        assert!(!valuator.reach_legal);
        assert_eq!(valuator.fixed_meld_count, fixed(1));
    }

    #[test]
    fn the_evaluation_hand_state_derives_the_fixed_meld_count() {
        // 副露済み面子数は評価対象の副露から導出する。caller は副露しか渡せないので、副露と
        // 面子数が食い違う評価状態を作れない。
        let mut source = TileIdSource::new();
        let mut melds = haku_pon(&mut source);
        melds.push(chi_123s(&mut source));
        let context = hand_state_context(
            source.tiles(&["1m", "2m", "3m"]),
            source.tiles(&["1p"]),
            Vec::new(),
        );

        let valuator = ProductionProspectiveValuator::new_with_hand_state(&context, Some(&melds));

        assert_eq!(valuator.melds, Some(melds.as_slice()));
        assert_eq!(valuator.fixed_meld_count, fixed(2));
        assert_eq!(
            valuator.fixed_meld_count,
            evaluation_fixed_meld_count_of(valuator.melds)
        );
        assert!(!valuator.reach_legal);
    }

    #[test]
    fn the_derived_fixed_meld_count_reaches_the_lookahead() {
        // 234m 567m 99p 3s 6s + 1p の11枚。白ポン1件と合わせた場合だけ1向聴で、打 1p の枝が
        // テンパイまで進む。2手先評価の副露済み面子数を context から取り直していれば、同じ
        // 手牌でも門前評価になってテンパイの枝が現れない。
        let mut source = TileIdSource::new();
        let melds = haku_pon(&mut source);
        let tiles = source.tiles(&[
            "2m", "3m", "4m", "5m", "6m", "7m", "9p", "9p", "3s", "6s", "1p",
        ]);
        let context = hand_state_context(tiles.clone(), source.tiles(&["1p"]), Vec::new());

        let open = ProductionProspectiveValuator::new_with_hand_state(&context, Some(&melds));
        let menzen = ProductionProspectiveValuator::new(&context);
        assert_eq!(open.fixed_meld_count, fixed(1));
        assert_eq!(menzen.fixed_meld_count, FixedMeldCount::NONE);

        // 打牌評価も評価器が導出した副露済み面子数で作り、テスト側で別の値を組み合わせない。
        let evaluations = evaluate_discards_from_tiles_with_fixed_melds_and_context(
            &tiles,
            open.fixed_meld_count(),
            context.dora_indicators(),
            context.round_wind(),
            context.seat_wind(),
        );

        let with_melds = diagnose_lookahead(
            &lookahead_inputs(&context, &tiles, &open, LookaheadDiagnosticScope::None),
            &evaluations,
        );
        let without_melds = diagnose_lookahead(
            &lookahead_inputs(&context, &tiles, &menzen, LookaheadDiagnosticScope::None),
            &evaluations,
        );

        assert!(tenpai_branch_count(&with_melds) > 0);
        assert_eq!(tenpai_branch_count(&without_melds), 0);
    }

    #[test]
    fn an_explicit_meld_reaches_the_completed_hands() {
        // 234m 567m 99p 7s 8s の10枚。白ポン1件と合わせて 6s / 9s 待ちのテンパイになり、
        // 完成手の役は副露した白だけになる。
        let mut source = TileIdSource::new();
        let melds = haku_pon(&mut source);
        let concealed = source.tiles(&["2m", "3m", "4m", "5m", "6m", "7m", "9p", "9p", "7s", "8s"]);
        let context = hand_state_context(concealed.clone(), source.tiles(&["1p"]), Vec::new());

        let open = ProductionProspectiveValuator::new_with_hand_state(&context, Some(&melds));
        let counts = TileCounts::from_tiles(concealed.iter().copied());
        let acceptance = calculate_acceptance_with_fixed_melds(&counts, open.fixed_meld_count());
        let tenpai = ProspectiveTenpai {
            concealed_tiles: &concealed,
            acceptance: &acceptance,
            discarded_tiles: &[],
        };

        let damaten_totals = |valuator: &ProductionProspectiveValuator<'_>| {
            let facts = valuator.tenpai_facts(&tenpai)?;
            let profile = evaluate_tenpai_hand_value(
                &facts.hands,
                damaten_baseline_context(&context),
                context.dora_indicators(),
                None,
            );
            Some(
                profile
                    .waits()
                    .iter()
                    .flat_map(|wait| wait.winning_tiles().iter())
                    .map(|variant| {
                        (
                            variant.winning_tile().to_mjai_string(),
                            variant.remaining(),
                            variant_total(variant),
                        )
                    })
                    .collect::<Vec<_>>(),
            )
        };

        // 役牌 白 の1翻30符。副露が完成手へ届いていなければこの打点にならない。
        assert_eq!(
            damaten_totals(&open),
            Some(vec![
                ("6s".to_string(), 4, Some(1000)),
                ("9s".to_string(), 4, Some(1000)),
            ])
        );

        // base context の副露状態 (門前) を使うと、白の面子が無い完成手になって打点が確定しない。
        let menzen = ProductionProspectiveValuator::new(&context);
        assert_eq!(
            damaten_totals(&menzen),
            Some(vec![
                ("6s".to_string(), 4, None),
                ("9s".to_string(), 4, None)
            ])
        );
    }
}
