//! 1向聴 lookahead が選んだ2手目打牌の先にあるテンパイの、将来打点を求める診断層。
//!
//! 既存の2手先診断 ([`LookaheadDiagnostic`]) が持つ枝
//!
//! ```text
//! 現在打牌 → 仮想ツモ → 既存打牌評価が選んだ next discard → テンパイ
//! ```
//!
//! の最後のテンパイについて、既存の待ちごとの手牌価値 ([`TenpaiHandValueProfile`]) をそのまま
//! 評価する。向聴・受け入れ・待ち・完成手の組み立て・点数計算・比較はすべて既存 layer を
//! source of truth にし、この層は「どの局面をどの baseline で評価し、その結果をどう保持するか」
//! だけを持つ。
//!
//! # 打牌選択を変えない
//!
//! ここで求める値は解析専用の追加情報で、現在打牌の選択にも2手目 `next_discard` の選択にも
//! 一切使わない。評価対象は既存 lookahead が既存 comparator で決めた `next_discard` そのもので、
//! 打点を見て選び直さない。
//!
//! # ダマ / リーチの両方を保持
//!
//! 未来のテンパイでリーチが合法かどうかも、リーチするかどうかも推測しない。現在の
//! `legal_actions` を未来局面へ流用もしない。代わりに、テンパイした枝については
//! [`damaten_baseline_context`] と [`reach_baseline_context`] の両方で評価し、どちらの打点も
//! そのまま保持する。baseline の semantics は既存 policy をそのまま再利用し、ここで組み立て
//! 直さない。リーチ baseline の裏ドラは既存どおり空の表示牌を明示して裏0で確定させる。
//!
//! # ロン可否
//!
//! 未来のフリテンは未来の自分の河に依存するため推測できない。既存のフリテン診断を渡さず、
//! ロン可否は unknown のままにする。フリテンは点数計算の入力ではないので、打点そのものは
//! ロン可否によらず求まる。
//!
//! # 仮想ツモ牌の赤5
//!
//! 受け入れは34種の牌種単位なので、仮想ツモ牌が赤5か黒5かは決まらない。赤5のある牌種
//! (5m / 5p / 5s) をツモる枝で、その赤5がまだどこにも見えていない場合は、和了手に赤ドラが
//! 何枚あるかを確定できない。赤牌の確率を推測せず、その枝は
//! [`ProspectiveUnavailable::UnresolvedRedFive`] として打点を持たない。赤5が既に手牌・副露・
//! 見え牌のどれかにあれば仮想ツモ牌は黒に確定し、赤5の無い牌種も同じく確定する。
//!
//! # 集約
//!
//! 待ち牌種ごと・和了牌の物理牌 (赤5 / 黒5) ごとの支払いをそのまま保持したうえで、診断用に
//! 残枚数の加重平均も持つ。集約規則は押し引きの攻撃打点と同じ helper を共有し、
//! 役なし・裏ドラ未確定・点数計算の入力不足を0点として平均へ入れない。残枚数0の variant は
//! 生きていないので平均へ寄与させない。ダマとリーチは別々に集約する。整数除算した平均値は
//! 表示専用で、threshold 判定にも選択にも使わない。本場・供託は加えない。

use bot_logic::{
    DiscardEvaluation, DiscardLookaheadDiagnostic, DrawLookaheadDiagnostic, HandValueError,
    HandValueOutcome, LookaheadDiagnostic, Meld, Payment, TenpaiCompletedHands,
    TenpaiHandValueError, TenpaiHandValueProfile, TileId, TileType, WinningContext,
    evaluate_tenpai_hand_value, tenpai_completed_hands,
};

use crate::context::GameContext;
use crate::damaten_value::damaten_baseline_context;
use crate::discard_selection::split_discarded_tile;
use crate::offense_value::{
    BASELINE_URA_DORA_INDICATORS, OffenseValue, reach_baseline_context, weighted_average,
};

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
/// 未来時点でリーチが合法かも、リーチするかも決めない。両方の baseline の打点を並べて保持する。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProspectiveTenpaiValue {
    pub damaten: ProspectiveBaselineValue,
    pub reach: ProspectiveBaselineValue,
}

/// 将来打点を評価できない理由。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProspectiveUnavailable {
    /// 仮想ツモ牌が赤5か黒5かを解決できず、和了手の赤ドラを確定できない。
    UnresolvedRedFive,
    /// 仮想局面の物理牌一覧を作れない、または完成手を解析できない。
    CompletedHand,
}

/// 受け入れ牌1枚分の枝の将来打点。
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

/// 現在打牌後の受け入れ牌1枚を仮想ツモした枝の将来打点。
///
/// `draw` / `remaining` / `next_discard` は既存 2手先診断
/// ([`DrawLookaheadDiagnostic`](bot_logic::DrawLookaheadDiagnostic)) の値そのもので、診断のために
/// 求め直さない。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProspectiveDrawValue {
    pub draw: TileType,
    pub remaining: u8,
    /// 既存 lookahead が既存 comparator で選んだ2手目の打牌。打点で選び直さない。
    pub next_discard: Option<TileType>,
    pub outcome: ProspectiveOutcome,
}

/// 現在の打牌候補1件分の将来打点。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProspectiveDiscardValue {
    pub discard: TileType,
    /// 受け入れ牌ごとの枝。順序と対象牌は既存2手先診断と同じ。
    pub draws: Vec<ProspectiveDrawValue>,
}

impl ProspectiveDiscardValue {
    pub fn draw(&self, tile: TileType) -> Option<&ProspectiveDrawValue> {
        self.draws.iter().find(|draw| draw.draw == tile)
    }
}

/// 全打牌候補分の将来打点。
///
/// `candidates` は既存2手先診断と同じ順序・同じ件数で、selected 候補だけでなく runner-up を含む
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

/// 構築済みの2手先診断の各枝について、`next_discard` 後のテンパイの将来打点を求める。
///
/// `tiles` は現在打牌の前の全物理牌 (手牌 + ツモ牌)、`evaluations` はその手牌に対する既存の
/// 打牌候補評価で、`lookahead` はその評価から構築した2手先診断。候補の順序・牌種が対応しない
/// 場合は推測せず、その候補の枝を空にする。
///
/// 探索も比較もやり直さない。既存 lookahead が選んだ `next_discard` をそのまま評価対象にする。
pub(crate) fn evaluate_prospective_lookahead_value(
    context: &GameContext,
    tiles: &[TileId],
    evaluations: &[DiscardEvaluation],
    lookahead: &LookaheadDiagnostic,
) -> ProspectiveLookaheadDiagnostic {
    if lookahead.candidates.len() != evaluations.len() {
        return ProspectiveLookaheadDiagnostic::default();
    }

    let inputs = ProspectiveInputs {
        context,
        tiles,
        melds: context.own_melds().unwrap_or_default(),
        damaten: damaten_baseline_context(context),
        reach: reach_baseline_context(context),
    };

    ProspectiveLookaheadDiagnostic {
        candidates: lookahead
            .candidates
            .iter()
            .zip(evaluations)
            .map(|(candidate, evaluation)| ProspectiveDiscardValue {
                discard: candidate.discard,
                draws: candidate_draws(&inputs, candidate, evaluation),
            })
            .collect(),
    }
}

// 将来打点の評価入力。枝ごとに組み立て直さない値だけを持つ。
struct ProspectiveInputs<'a> {
    context: &'a GameContext,
    // 現在打牌の前の全物理牌 (手牌 + ツモ牌)。
    tiles: &'a [TileId],
    melds: &'a [Meld],
    // ダマ / リーチ両方の hypothetical baseline。
    damaten: WinningContext,
    reach: WinningContext,
}

// 現在の打牌候補1件分の枝。候補の牌種が既存評価と対応しない場合は推測せず枝を持たない。
fn candidate_draws(
    inputs: &ProspectiveInputs,
    candidate: &DiscardLookaheadDiagnostic,
    evaluation: &DiscardEvaluation,
) -> Vec<ProspectiveDrawValue> {
    if candidate.discard != evaluation.discard {
        return Vec::new();
    }

    // 1手目に切る物理牌は候補ごとに1つなので、受け入れ牌ごとに求め直さない。
    let branch = split_discarded_tile(inputs.tiles.to_vec(), evaluation);
    candidate
        .draws
        .iter()
        .map(|draw| ProspectiveDrawValue {
            draw: draw.draw,
            remaining: draw.remaining,
            next_discard: draw.next_discard_tile(),
            outcome: draw_outcome(inputs, branch.as_ref(), draw),
        })
        .collect()
}

// 枝1件分の将来打点。テンパイでない枝と評価できない枝を打点0へ潰さず、理由のまま返す。
fn draw_outcome(
    inputs: &ProspectiveInputs,
    branch: Option<&(TileId, Vec<TileId>)>,
    draw: &DrawLookaheadDiagnostic,
) -> ProspectiveOutcome {
    let Some(next) = draw.next_discard.as_ref() else {
        return ProspectiveOutcome::NoNextDiscard;
    };
    if next.min_shanten_after_discard() != TENPAI_SHANTEN {
        return ProspectiveOutcome::NotTenpai;
    }

    let Some((first_discarded, after_first)) = branch else {
        return ProspectiveOutcome::Unavailable(ProspectiveUnavailable::CompletedHand);
    };
    // 赤5の解決は現在の既知情報だけで行う。赤牌の確率を推測しない。
    let Some(drawn) = drawn_tile_id(
        draw.draw,
        inputs.tiles,
        inputs.melds,
        inputs.context.visible_tiles(),
    ) else {
        return ProspectiveOutcome::Unavailable(ProspectiveUnavailable::UnresolvedRedFive);
    };

    let mut after_draw = after_first.clone();
    after_draw.push(drawn);
    let Some((next_discarded, concealed)) = split_discarded_tile(after_draw, next) else {
        return ProspectiveOutcome::Unavailable(ProspectiveUnavailable::CompletedHand);
    };

    // 1手目と2手目に切った牌はテンパイ時点で見え牌になる。赤5が見えているかの判定にだけ使う。
    let mut visible = inputs.context.visible_tiles().to_vec();
    visible.push(*first_discarded);
    visible.push(next_discarded);

    // 未来のフリテンは未来の自分の河に依存するため推測しない。ロン可否は unknown のままにする。
    let hands = match tenpai_completed_hands(
        &concealed,
        inputs.melds,
        &next.acceptance_after_discard,
        None,
        &visible,
    ) {
        Ok(hands) => hands,
        Err(TenpaiHandValueError::NotTenpai(_)) => return ProspectiveOutcome::NotTenpai,
        Err(TenpaiHandValueError::CompletedHand(_)) => {
            return ProspectiveOutcome::Unavailable(ProspectiveUnavailable::CompletedHand);
        }
    };

    let dora_indicators = inputs.context.dora_indicators();
    ProspectiveOutcome::Evaluated(ProspectiveTenpaiValue {
        damaten: baseline_value(&hands, inputs.damaten, dora_indicators, None),
        reach: baseline_value(
            &hands,
            inputs.reach,
            dora_indicators,
            Some(BASELINE_URA_DORA_INDICATORS),
        ),
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

/// 仮想ツモ牌の物理牌。赤5かどうかを解決できない場合は `None`。
///
/// 赤5の無い牌種は必ず黒に確定する。赤5のある牌種は、その赤5が既に手牌・副露・見え牌のどれかに
/// 現れていればツモ牌が黒に確定し、まだ見えていなければ赤か黒かを決められない。赤牌の確率は
/// 推測しない。
fn drawn_tile_id(
    draw: TileType,
    tiles: &[TileId],
    melds: &[Meld],
    visible_tiles: &[TileId],
) -> Option<TileId> {
    let red_five_seen = |red: TileId| {
        tiles
            .iter()
            .chain(melds.iter().flat_map(|meld| meld.tiles()))
            .chain(visible_tiles)
            .any(|tile| *tile == red)
    };

    match TileId::copies(draw).find(|tile| tile.is_red()) {
        Some(red) if !red_five_seen(red) => None,
        _ => black_copy(draw, tiles),
    }
}

// 黒牌の物理牌 identity は打点に影響しないため、手牌と重ならない copy を1つ選ぶ。
fn black_copy(draw: TileType, tiles: &[TileId]) -> Option<TileId> {
    let black = || TileId::copies(draw).filter(|tile| !tile.is_red());
    black()
        .find(|tile| !tiles.contains(tile))
        .or_else(|| black().next())
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::sync::LazyLock;

    use bot_logic::{
        MissingScoringFact, NormalScoringError, RiichiStatus, WinMethod, evaluate_payment,
    };

    use crate::action::LegalAction;
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

        fn evaluated(&self, discard: &str, draw: &str) -> &ProspectiveTenpaiValue {
            self.draw(discard, draw)
                .outcome
                .evaluated()
                .expect("テンパイ枝の将来打点を評価できる")
        }
    }

    // 場風・自風・見え牌を既知にした門前14枚の局面。自分は子 (南家)。`winds == false` では
    // 場風・自風を渡さず、点数計算の入力が足りない局面にする。
    fn case_of(hand: &[&str], extra_visible: &[&str], winds: bool) -> Case {
        let mut source = TileIdSource::new();
        let hand_tiles = source.tiles(&hand[..hand.len() - 1]);
        let drawn_tile = source.tile(hand[hand.len() - 1]);
        let dora_indicators = source.tiles(&["1m"]);
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
    fn an_unresolved_red_five_draw_has_no_value() {
        // 仮想ツモ牌の赤5を解決できない枝は、赤牌の確率を推測せず打点を持たない。
        assert_eq!(
            RED_FIVE.draw("1p", "5s").outcome,
            ProspectiveOutcome::Unavailable(ProspectiveUnavailable::UnresolvedRedFive)
        );

        // 赤5が既に見えていれば仮想ツモ牌は黒に確定し、同じ枝を評価できる。
        assert!(RED_FIVE_SEEN.draw("1p", "5s").outcome.evaluated().is_some());
    }

    #[test]
    fn a_branch_that_is_not_tenpai_has_no_value() {
        // 2手目の打牌後がテンパイでなければ将来打点を持たない。
        let draw = RED_FIVE.draw("1m", "9m");

        assert!(draw.next_discard.is_some());
        assert_eq!(draw.outcome, ProspectiveOutcome::NotTenpai);
        assert!(draw.outcome.evaluated().is_none());
    }

    #[test]
    fn a_branch_without_a_next_discard_has_no_value() {
        // 2手目の打牌候補が無い枝も将来打点を持たない。
        let mut lookahead = RED_FIVE.lookahead.clone();
        for candidate in &mut lookahead.candidates {
            for draw in &mut candidate.draws {
                draw.next_discard = None;
            }
        }

        let value = evaluate_prospective_lookahead_value(
            &RED_FIVE.ctx,
            &hand_tiles(&RED_FIVE.ctx),
            &evaluations(&RED_FIVE.lookahead, &RED_FIVE.ctx),
            &lookahead,
        );

        for candidate in &value.candidates {
            for draw in &candidate.draws {
                assert_eq!(draw.next_discard, None);
                assert_eq!(draw.outcome, ProspectiveOutcome::NoNextDiscard);
            }
        }
    }

    #[test]
    fn the_prospective_value_follows_the_existing_next_discard() {
        // 評価対象は既存 lookahead が既存比較順で選んだ2手目打牌そのもの。打点で選び直さない。
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
                assert_eq!(draw.next_discard, branch.next_discard_tile());
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
