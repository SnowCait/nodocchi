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
//! # ロン可否
//!
//! 未来のフリテンは未来の自分の河に依存するため推測できない。既存のフリテン診断を渡さず、
//! ロン可否は unknown のままにする。フリテンは点数計算の入力ではないので、打点そのものは
//! ロン可否によらず求まる。将来フリテンによる価値補正は今回の対象外で、ダマ baseline は
//! 元々ロン和了を前提にした hypothetical baseline のまま使う。
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
    DrawVariantLookaheadDiagnostic, HandValueError, HandValueOutcome, LookaheadDiagnostic, Meld,
    Payment, ProspectiveTenpai, ProspectiveTenpaiValuator, TenpaiCompletedHands,
    TenpaiHandValueProfile, TileId, TileType, WinningContext, evaluate_tenpai_hand_value,
    is_menzen, split_discarded_tile, tenpai_completed_hands,
};

use crate::context::GameContext;
use crate::damaten_value::{damaten_baseline_context, damaten_value_from_hands};
use crate::offense_value::{
    BASELINE_URA_DORA_INDICATORS, OffenseValue, TenpaiOffenseMode, reach_baseline_context,
    variant_total, weighted_average,
};
use crate::reach_policy::{ReachLegalityFacts, decide_reach_reason, is_reach_legal};

// テンパイの向聴数。
const TENPAI_SHANTEN: i8 = 0;

/// 和了牌の物理牌1つ分の将来打点。
///
/// 既存 [`HandValueOutcome`] の結論を潰さずに区別する。役なしは0点ではなく [`Self::NoYaku`]。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProspectiveValue {
    /// 役があり、支払いまで確定した。
    Known {
        payment: Payment,
        /// 名前の付いた役満として確定したか。
        is_yakuman: bool,
    },
    /// 役が無い ([`HandValueOutcome::NoCandidate`])。0点ではない。
    NoYaku,
    /// 打点を確定できない。理由を潰さずに保持する。
    Unknown(ProspectiveUnknownReason),
}

impl ProspectiveValue {
    pub fn payment(self) -> Option<Payment> {
        match self {
            Self::Known { payment, .. } => Some(payment),
            Self::NoYaku | Self::Unknown(_) => None,
        }
    }

    /// 確定した支払い合計 [点]。確定しない場合と役なしの場合は `None`。
    pub fn total(self) -> Option<u32> {
        self.payment().map(Payment::total)
    }

    pub fn is_yakuman(self) -> bool {
        matches!(
            self,
            Self::Known {
                is_yakuman: true,
                ..
            }
        )
    }
}

/// 将来打点を確定できない理由。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProspectiveUnknownReason {
    /// bonus 翻が確定しない ([`HandValueOutcome::IndeterminateBonusHan`])。
    IndeterminateBonusHan,
    /// 点数計算の入力が足りない。場風・自風が不明な場合など。
    Scoring(HandValueError),
    /// 役はあるが支払いを求められない。
    MissingPayment,
}

/// 和了牌の物理牌1つ分の将来打点と、その残枚数。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProspectiveWinningTileValue {
    pub winning_tile: TileId,
    /// この variant の残枚数。待ち全体の残枚数のうち、赤 / 黒それぞれの枚数。
    pub remaining: u8,
    pub value: ProspectiveValue,
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
    melds: &'a [Meld],
    // ダマ / リーチ両方の hypothetical baseline。枝ごとに組み立て直さない。
    damaten: WinningContext,
    reach: WinningContext,
    // 将来テンパイでリーチが合法か。手牌の副露・持ち点・既リーチはこの局から変わらないので、
    // 枝ごとに求め直さない。
    reach_legal: bool,
    // 自分が既にリーチしているか。自分の席を特定できない場合は `None`。
    own_reached: Option<bool>,
}

impl<'a> ProductionProspectiveValuator<'a> {
    pub(crate) fn new(context: &'a GameContext) -> Self {
        Self {
            context,
            melds: context.own_melds().unwrap_or_default(),
            damaten: damaten_baseline_context(context),
            reach: reach_baseline_context(context),
            reach_legal: future_reach_legal(context),
            own_reached: context.own_reached(),
        }
    }

    // 枝1つ分のテンパイの完成手。物理牌を組み立てられない場合と解析できない場合は `None`。
    fn completed_hands(&self, tenpai: &ProspectiveTenpai<'_>) -> Option<TenpaiCompletedHands> {
        // 1手目と2手目に切った牌はテンパイ時点で見え牌になる。赤5が見えているかの判定に使う。
        let mut visible = self.context.visible_tiles().to_vec();
        visible.extend_from_slice(tenpai.discarded_tiles);

        // 未来のフリテンは未来の自分の河に依存するため推測しない。ロン可否は unknown のままにする。
        tenpai_completed_hands(
            tenpai.concealed_tiles,
            self.melds,
            tenpai.acceptance,
            None,
            &visible,
        )
        .ok()
    }

    // 攻撃を継続した場合の攻撃モード。production のリーチ判断と同じ policy をそのまま使う。
    fn offense_mode(&self, hands: &TenpaiCompletedHands, tsumo_remaining: u8) -> TenpaiOffenseMode {
        match self.own_reached {
            None => TenpaiOffenseMode::Unknown,
            Some(true) => TenpaiOffenseMode::Reach,
            Some(false) => {
                let verdict = damaten_value_from_hands(self.context, hands).verdict;
                let reason = decide_reach_reason(self.reach_legal, Some(verdict), tsumo_remaining);
                if reason.selects_reach() {
                    TenpaiOffenseMode::Reach
                } else {
                    TenpaiOffenseMode::Damaten
                }
            }
        }
    }

    // 攻撃モードごとの hypothetical baseline と裏ドラ表示牌。確定できない場合は `None`。
    fn scoring_inputs(
        &self,
        mode: TenpaiOffenseMode,
    ) -> Option<(WinningContext, Option<&'static [TileId]>)> {
        match mode {
            TenpaiOffenseMode::Reach => Some((self.reach, Some(BASELINE_URA_DORA_INDICATORS))),
            TenpaiOffenseMode::Damaten => Some((self.damaten, None)),
            TenpaiOffenseMode::Unknown => None,
        }
    }

    // 選択に使う Σ(和了牌 variant 残枚数 × 支払い合計)。確定できない場合は `None`。
    fn selection_value(&self, hands: &TenpaiCompletedHands, tsumo_remaining: u8) -> Option<u64> {
        let (baseline, ura_dora) =
            self.scoring_inputs(self.offense_mode(hands, tsumo_remaining))?;
        let profile =
            evaluate_tenpai_hand_value(hands, baseline, self.context.dora_indicators(), ura_dora);
        weighted_total(&profile)
    }
}

impl ProspectiveTenpaiValuator for ProductionProspectiveValuator<'_> {
    fn tenpai_value(&self, tenpai: &ProspectiveTenpai<'_>) -> Option<u64> {
        let hands = self.completed_hands(tenpai)?;
        self.selection_value(&hands, tenpai.acceptance.total_remaining())
    }
}

/// 将来テンパイでリーチが合法か。
///
/// 現在局面の `legal_actions` を未来へ流用せず、共有条件 ([`is_reach_legal`]) を将来テンパイの
/// 材料で評価する。門前・既リーチ・持ち点は自分のツモと打牌では変わらないので現在の既知 fact を
/// そのまま使い、打牌後テンパイは枝の構成上必ず満たす。未来時点の山残枚数だけは確定できないので
/// 現在の枚数で代用せず unknown として渡し、共有条件の unknown 規則へ委ねる。
fn future_reach_legal(context: &GameContext) -> bool {
    is_reach_legal(ReachLegalityFacts {
        menzen: context.own_melds().map(is_menzen),
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
            .map(|(candidate, evaluation)| ProspectiveDiscardValue {
                discard: candidate.discard,
                draws: candidate_draws(&valuator, tiles, candidate, evaluation),
            })
            .collect(),
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
    let Some(hands) = valuator.completed_hands(&tenpai) else {
        return ProspectiveOutcome::Unavailable(ProspectiveUnavailable::CompletedHand);
    };

    let dora_indicators = valuator.context.dora_indicators();
    ProspectiveOutcome::Evaluated(ProspectiveTenpaiValue {
        damaten: baseline_value(&hands, valuator.damaten, dora_indicators, None),
        reach: baseline_value(
            &hands,
            valuator.reach,
            dora_indicators,
            Some(BASELINE_URA_DORA_INDICATORS),
        ),
        mode: valuator.offense_mode(&hands, next.acceptance_total_remaining()),
    })
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
                    value: prospective_value(winning_tile.outcome()),
                })
                .collect(),
        })
        .collect()
}

// 既存の手牌価値の結果を将来打点へ畳む。役なしと確定しない理由を潰さずに区別して持つ。
fn prospective_value(outcome: Result<&HandValueOutcome<'_>, HandValueError>) -> ProspectiveValue {
    match outcome {
        Ok(HandValueOutcome::Known(hand_value)) => match hand_value.payment() {
            Some(payment) => ProspectiveValue::Known {
                payment,
                is_yakuman: hand_value.is_yakuman(),
            },
            None => ProspectiveValue::Unknown(ProspectiveUnknownReason::MissingPayment),
        },
        Ok(HandValueOutcome::NoCandidate) => ProspectiveValue::NoYaku,
        Ok(HandValueOutcome::IndeterminateBonusHan) => {
            ProspectiveValue::Unknown(ProspectiveUnknownReason::IndeterminateBonusHan)
        }
        Err(error) => ProspectiveValue::Unknown(ProspectiveUnknownReason::Scoring(error)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::sync::LazyLock;

    use bot_logic::{
        ForwardMetrics, MissingScoringFact, NormalScoringError, RiichiStatus, WinMethod,
        evaluate_payment, forward_metrics_from_lookahead,
    };

    use crate::action::LegalAction;
    use crate::damaten_value::DAMATEN_MIN_TOTAL;
    use crate::discard_selection::select_discard_action_with_diagnostic;

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
            forward_metrics_from_lookahead(
                &evaluations(&self.lookahead, &self.ctx),
                &self.lookahead,
            )
        }
    }

    // 場風・自風・見え牌を既知にした門前14枚の局面。自分は子 (南家)。`winds == false` では
    // 場風・自風を渡さず、点数計算の入力が足りない局面にする。
    fn case_of(hand: &[&str], extra_visible: &[&str], winds: bool) -> Case {
        case_with_dora(hand, "1m", extra_visible, winds)
    }

    fn case_with_dora(
        hand: &[&str],
        dora_indicator: &str,
        extra_visible: &[&str],
        winds: bool,
    ) -> Case {
        let mut source = TileIdSource::new();
        let hand_tiles = source.tiles(&hand[..hand.len() - 1]);
        let drawn_tile = source.tile(hand[hand.len() - 1]);
        let dora_indicators = source.tiles(&[dora_indicator]);
        let extra_visible = source.tiles(extra_visible);

        let visible: Vec<TileId> = hand_tiles
            .iter()
            .chain([&drawn_tile])
            .chain(dora_indicators.iter())
            .chain(extra_visible.iter())
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
            winds.then(|| tile("E")),
            winds.then(|| tile("S")),
            visible,
            Some(0),
            Some(3),
            Default::default(),
            [false; 4],
        );

        let selection = select_discard_action_with_diagnostic(&ctx, &actions, true);
        Case {
            ctx,
            lookahead: selection.lookahead.expect("2手先診断が構築されている"),
            value: selection.lookahead_value.expect("将来打点が構築されている"),
        }
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

    fn known(total: u32) -> ProspectiveValue {
        ProspectiveValue::Known {
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
                matches!(variant.value, ProspectiveValue::Known { .. }),
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
                .all(|variant| variant.value == ProspectiveValue::NoYaku)
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
        let live = |variants: &[(ProspectiveValue, u8)]| {
            weighted_average(
                variants
                    .iter()
                    .map(|(value, remaining)| (value.total(), *remaining)),
            )
        };

        let with_dead = live(&[(known(7700), 4), (ProspectiveValue::NoYaku, 0)]);
        assert_eq!(with_dead, live(&[(known(7700), 4)]));
        assert_eq!(with_dead.average_total(), Some(7700));
        // 生きた役なしは0点として平均へ入れず、加重平均そのものを確定しないままにする。
        assert_eq!(
            live(&[(known(7700), 4), (ProspectiveValue::NoYaku, 1)]),
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
                ProspectiveValue::Unknown(ProspectiveUnknownReason::Scoring(
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
        // ダマ 7700 / リーチ 8000 で production 判断がダマなら、選択値もダマ打点で作る。
        let variant = HIGH_DAMATEN.red_variant("8m", "5p", true);
        let tenpai = variant.outcome.evaluated().expect("テンパイ枝");

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
        let evaluations = select_discard_action_with_diagnostic(context, &actions, false)
            .diagnostic
            .candidates
            .into_iter()
            .map(|candidate| candidate.evaluation)
            .collect::<Vec<_>>();

        assert_eq!(evaluations.len(), lookahead.candidates.len());
        evaluations
    }
}
