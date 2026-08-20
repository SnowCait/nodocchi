//! ダマのまま和了した場合の確定打点を待ちごとに求める policy 層。
//!
//! 既存の待ちごとの手牌価値 ([`TenpaiHandValueProfile`]) を、リーチ / ダマ判断が使える形へ
//! 畳む。点数計算そのものは bot-logic の既存 scoring rule に任せ、ここでは「どの和了状況で
//! 評価するか」と「その結果をどう分類するか」だけを持つ。
//!
//! # hypothetical baseline
//!
//! 打点比較のための和了状況は、現在の局面から推測するのではなく明示的な baseline として
//! 組み立てる。
//!
//! ```text
//! WinMethod              = Ron
//! RiichiStatus           = NotDeclared
//! chankan                = false
//! remaining_live_tiles   = 1
//! round_wind / seat_wind = GameContext の既知 fact
//! ```
//!
//! `remaining_live_tiles = 1` は海底 / 河底を付けない baseline を作るための policy input で、
//! 実際の山残枚数ではない。場風・自風は既知の場合だけ使い、不明なら不明のまま渡す。ドラ表示牌は
//! 現在の既知情報をそのまま使い、ダマなので裏ドラは判定材料にしない。
//!
//! 本場・供託は threshold へ加えない。判定は純粋な [`Payment::total`] だけで行う。
//!
//! # 分類
//!
//! 待ちは牌種ごと、さらに和了牌の物理牌 (赤5 / 黒5) ごとの variant に分かれ、variant 1つ1つが
//! 別の打点を持つ。平均値や EV は取らず、全 variant を個別に見て [`DamatenValueVerdict`] へ
//! 畳む。役満は名前の付いた役満として確定した時点で threshold を満たす扱いにする。

use bot_logic::{
    DiscardEvaluation, HandValueError, HandValueOutcome, Payment, RiichiStatus,
    TenpaiCompletedHands, TenpaiHandValueProfile, TenpaiWaitAvailability, TileId, TileType,
    WinMethod, WinningContext, evaluate_tenpai_hand_value, tenpai_completed_hands,
};

use crate::context::GameContext;
use crate::discard_selection::concealed_tiles_after_discard;

/// ダマのままで良いと判断する [`Payment::total`] の下限 [点]。inclusive。
///
/// 親子で別の threshold へ換算せず、実点数をそのまま比較する。
pub const DAMATEN_MIN_TOTAL: u32 = 7700;

/// 海底 / 河底の付かない baseline を作るための残り山枚数。実際の山残枚数ではない。
///
/// ダマ baseline とリーチ baseline のどちらも同じ policy input を使う。
pub(crate) const BASELINE_REMAINING_LIVE_TILES: u32 = 1;

/// 和了牌の物理牌1つ分のダマ打点。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DamatenValue {
    /// ダマで役があり、支払いまで確定した。
    Known {
        payment: Payment,
        /// 名前の付いた役満として確定したか。
        is_yakuman: bool,
    },
    /// ダマでは役が無く、そもそもロンできない。
    NoYaku,
    /// ダマ打点を確定できない。場風・自風のような点数計算の入力が不明な場合。
    Unknown,
}

impl DamatenValue {
    pub fn payment(self) -> Option<Payment> {
        match self {
            Self::Known { payment, .. } => Some(payment),
            Self::NoYaku | Self::Unknown => None,
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

    /// ダマのままで良い打点か。確定しない場合は `None`。
    ///
    /// 名前の付いた役満は threshold を満たす。役なしはロンできないため満たさない。
    pub fn meets_threshold(self) -> Option<bool> {
        match self {
            Self::Known {
                payment,
                is_yakuman,
            } => Some(is_yakuman || payment.total() >= DAMATEN_MIN_TOTAL),
            Self::NoYaku => Some(false),
            Self::Unknown => None,
        }
    }
}

/// 和了牌の物理牌1つ分のダマ打点と、その残枚数。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DamatenWinningTileValue {
    pub winning_tile: TileId,
    /// この variant の残枚数。待ち全体の残枚数のうち、赤 / 黒それぞれの枚数。
    pub remaining: u8,
    pub value: DamatenValue,
}

impl DamatenWinningTileValue {
    pub fn is_red(&self) -> bool {
        self.winning_tile.is_red()
    }
}

/// 待ち1牌種分のダマ打点。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DamatenWaitValue {
    pub winning_tile: TileType,
    /// この待ち全体の残枚数。既存の受け入れの値そのもの。
    pub remaining: u8,
    /// 和了牌の物理牌ごとのダマ打点。赤5と黒5のどちらもあり得る場合は両方を含む。
    pub winning_tiles: Vec<DamatenWinningTileValue>,
}

/// 全ての生きた待ちのダマ打点をまとめた結論。
///
/// 判定順は「役なし → threshold 未満 → 確定しない」で、先に当たったものを1つだけ表す。
/// どれか1 variant でも役なし / threshold 未満なら、他の variant を待たずにリーチ側へ倒れる。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DamatenValueVerdict {
    /// 生きた待ちが1つも無い。
    NoLiveWait,
    /// ダマでは役が無い variant がある。
    NoYaku,
    /// ダマ打点が threshold 未満の variant がある。
    BelowThreshold,
    /// 役なし / threshold 未満の variant は無いが、打点を確定できない variant がある。
    ///
    /// この policy では結論を出さない。
    Indeterminate,
    /// 全ての variant がダマで役ありかつ threshold 以上。
    AboveThreshold,
}

impl DamatenValueVerdict {
    /// この結論だけでリーチ / ダマを決められるか。
    pub fn is_conclusive(self) -> bool {
        !matches!(self, Self::Indeterminate)
    }
}

/// ダマ打点による判断の構造化診断。
///
/// `waits` は評価した待ち牌種と、その和了牌の物理牌ごとのダマ打点をそのまま並べたもの。
/// `verdict` はそこから畳んだ結論で、判断に使う値と診断に出す値を分けない。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DamatenValueDiagnostic {
    /// 打点比較に使った hypothetical baseline。実際の和了状況ではない。
    pub baseline: WinningContext,
    pub waits: Vec<DamatenWaitValue>,
    pub verdict: DamatenValueVerdict,
}

impl DamatenValueDiagnostic {
    /// 和了牌の物理牌ごとのダマ打点を待ちの順に並べた iterator。
    pub fn winning_tile_values(&self) -> impl Iterator<Item = &DamatenWinningTileValue> {
        self.waits.iter().flat_map(|wait| wait.winning_tiles.iter())
    }
}

/// ダマ打点比較用の hypothetical baseline を組み立てる。
///
/// 未来の事実を実際の事実として推測しないため、和了方法・リーチ状態・槍槓・残り山は policy が
/// 決めた baseline の値にする。場風・自風だけを `context` の既知 fact から取り、不明なら不明の
/// まま渡す。
pub fn damaten_baseline_context(context: &GameContext) -> WinningContext {
    WinningContext::new(WinMethod::Ron)
        .with_round_wind(context.round_wind())
        .with_seat_wind(context.seat_wind())
        .with_riichi(RiichiStatus::NotDeclared)
        .with_chankan(Some(false))
        .with_remaining_live_tiles(Some(BASELINE_REMAINING_LIVE_TILES))
}

/// 選択済みの打牌1件について、その打牌後のテンパイのダマ打点を待ちごとに求める。
///
/// `evaluation` は通常打牌 selection が選んだ打牌の評価で、待ち牌種と残枚数はその受け入れ
/// (`acceptance_after_discard`) をそのまま使う。リーチ判断のために向聴・受け入れ・待ち・
/// フリテンを計算し直さない。`wait_availability` は同じ打牌から求めた既存のフリテン診断で、
/// ロン可否を持ち回るためだけに渡す。
///
/// 打牌後の手牌を組み立てられない場合 (打牌の物理牌が手牌に無い、完成手を解析できないなど) は
/// `None`。その場合はダマ打点を推測せず、呼び出し側が既存判断へ委ねる。
pub(crate) fn evaluate_damaten_value(
    context: &GameContext,
    evaluation: &DiscardEvaluation,
    wait_availability: &TenpaiWaitAvailability,
) -> Option<DamatenValueDiagnostic> {
    let hands = tenpai_completed_hands_after_discard(context, evaluation, wait_availability)?;
    Some(damaten_value_from_hands(context, &hands))
}

/// 選択済みの打牌1件について、その打牌後のテンパイの待ちごとの完成手を組み立てる。
///
/// 待ち牌種と残枚数は `evaluation` の受け入れ (`acceptance_after_discard`) がそのまま source of
/// truth で、ここで待ちを計算し直さない。`wait_availability` は同じ打牌から求めた既存のフリテン
/// 診断で、ロン可否を持ち回るためだけに渡す。
///
/// 打牌後の手牌を組み立てられない場合 (打牌の物理牌が手牌に無い、完成手を解析できないなど) は
/// `None`。ダマ打点と押し引きの攻撃打点は同じ完成手を使うため、この1本を共有する。
pub(crate) fn tenpai_completed_hands_after_discard(
    context: &GameContext,
    evaluation: &DiscardEvaluation,
    wait_availability: &TenpaiWaitAvailability,
) -> Option<TenpaiCompletedHands> {
    let concealed_tiles = concealed_tiles_after_discard(context, evaluation)?;
    tenpai_completed_hands(
        &concealed_tiles,
        context.own_melds().unwrap_or_default(),
        &evaluation.acceptance_after_discard,
        Some(wait_availability),
        context.visible_tiles(),
    )
    .ok()
}

/// 組み立て済みの完成手を、ダマの hypothetical baseline で評価して待ちごとのダマ打点へ畳む。
pub(crate) fn damaten_value_from_hands(
    context: &GameContext,
    hands: &TenpaiCompletedHands,
) -> DamatenValueDiagnostic {
    let baseline = damaten_baseline_context(context);
    let profile = evaluate_tenpai_hand_value(hands, baseline, context.dora_indicators(), None);
    let waits = wait_values(&profile);
    let verdict = verdict(&waits);

    DamatenValueDiagnostic {
        baseline,
        waits,
        verdict,
    }
}

fn wait_values(profile: &TenpaiHandValueProfile<'_>) -> Vec<DamatenWaitValue> {
    profile
        .waits()
        .iter()
        .map(|wait| DamatenWaitValue {
            winning_tile: wait.winning_tile(),
            remaining: wait.remaining(),
            winning_tiles: wait
                .winning_tiles()
                .iter()
                .map(|winning_tile| DamatenWinningTileValue {
                    winning_tile: winning_tile.winning_tile(),
                    remaining: winning_tile.remaining(),
                    value: damaten_value(winning_tile.outcome()),
                })
                .collect(),
        })
        .collect()
}

// 既存の手牌価値の結果をダマ打点へ畳む。役なし・確定しない理由を潰さずに区別して持つ。
fn damaten_value(outcome: Result<&HandValueOutcome<'_>, HandValueError>) -> DamatenValue {
    match outcome {
        Ok(HandValueOutcome::Known(hand_value)) => match hand_value.payment() {
            Some(payment) => DamatenValue::Known {
                payment,
                is_yakuman: hand_value.is_yakuman(),
            },
            None => DamatenValue::Unknown,
        },
        Ok(HandValueOutcome::NoCandidate) => DamatenValue::NoYaku,
        Ok(HandValueOutcome::IndeterminateBonusHan) | Err(_) => DamatenValue::Unknown,
    }
}

fn verdict(waits: &[DamatenWaitValue]) -> DamatenValueVerdict {
    let values = || {
        waits
            .iter()
            .flat_map(|wait| wait.winning_tiles.iter())
            .map(|winning_tile| winning_tile.value)
    };

    if values().next().is_none() {
        return DamatenValueVerdict::NoLiveWait;
    }
    if values().any(|value| value == DamatenValue::NoYaku) {
        return DamatenValueVerdict::NoYaku;
    }
    if values().any(|value| value.meets_threshold() == Some(false)) {
        return DamatenValueVerdict::BelowThreshold;
    }
    if values().any(|value| value.meets_threshold().is_none()) {
        return DamatenValueVerdict::Indeterminate;
    }
    DamatenValueVerdict::AboveThreshold
}

#[cfg(test)]
mod tests {
    use super::*;

    use bot_logic::{WinMethod, evaluate_payment};

    fn tile_type(s: &str) -> TileType {
        TileType::from_mjai_type_str(s).unwrap()
    }

    fn winning_tile(s: &str) -> TileId {
        TileId::copies(tile_type(s)).next().unwrap()
    }

    // 子のロンで指定の支払い合計になる基本点から Payment を作る。
    fn payment(basic_points: u32) -> Payment {
        evaluate_payment(basic_points, false, WinMethod::Ron).unwrap()
    }

    fn known(basic_points: u32) -> DamatenValue {
        DamatenValue::Known {
            payment: payment(basic_points),
            is_yakuman: false,
        }
    }

    fn yakuman(basic_points: u32) -> DamatenValue {
        DamatenValue::Known {
            payment: payment(basic_points),
            is_yakuman: true,
        }
    }

    fn wait(tile: &str, values: &[DamatenValue]) -> DamatenWaitValue {
        DamatenWaitValue {
            winning_tile: tile_type(tile),
            remaining: values.len() as u8,
            winning_tiles: values
                .iter()
                .map(|value| DamatenWinningTileValue {
                    winning_tile: winning_tile(tile),
                    remaining: 1,
                    value: *value,
                })
                .collect(),
        }
    }

    #[test]
    fn the_threshold_is_inclusive() {
        assert_eq!(known(1920).total(), Some(DAMATEN_MIN_TOTAL));
        assert_eq!(known(1920).meets_threshold(), Some(true));
        assert_eq!(known(1600).total(), Some(6400));
        assert_eq!(known(1600).meets_threshold(), Some(false));
    }

    #[test]
    fn a_named_yakuman_meets_the_threshold_by_itself() {
        // 役満は実点数ではなく役満であることで threshold を満たす。
        let value = yakuman(1);
        assert!(value.is_yakuman());
        assert!(value.total().unwrap() < DAMATEN_MIN_TOTAL);
        assert_eq!(value.meets_threshold(), Some(true));
    }

    #[test]
    fn a_hand_without_yaku_never_meets_the_threshold() {
        assert_eq!(DamatenValue::NoYaku.total(), None);
        assert_eq!(DamatenValue::NoYaku.meets_threshold(), Some(false));
        assert!(!DamatenValue::NoYaku.is_yakuman());
    }

    #[test]
    fn an_unknown_value_stays_unknown() {
        assert_eq!(DamatenValue::Unknown.total(), None);
        assert_eq!(DamatenValue::Unknown.meets_threshold(), None);
    }

    #[test]
    fn no_live_wait_is_reported_before_any_value() {
        assert_eq!(verdict(&[]), DamatenValueVerdict::NoLiveWait);
        assert!(DamatenValueVerdict::NoLiveWait.is_conclusive());
    }

    #[test]
    fn a_single_wait_without_yaku_decides_the_verdict() {
        let waits = [
            wait("3s", &[yakuman(8000)]),
            wait("6s", &[DamatenValue::NoYaku]),
        ];
        assert_eq!(verdict(&waits), DamatenValueVerdict::NoYaku);
    }

    #[test]
    fn a_single_wait_below_the_threshold_decides_the_verdict() {
        let waits = [wait("3s", &[known(1920)]), wait("6s", &[known(960)])];
        assert_eq!(verdict(&waits), DamatenValueVerdict::BelowThreshold);
    }

    #[test]
    fn red_and_black_fives_are_separate_variants() {
        // 同じ待ちでも赤 / 黒で結論が変わる。片方が threshold 未満なら threshold 未満扱い。
        let waits = [wait("5s", &[known(2000), known(1280)])];
        assert_eq!(verdict(&waits), DamatenValueVerdict::BelowThreshold);
    }

    #[test]
    fn every_variant_above_the_threshold_stays_damaten() {
        let waits = [wait("3s", &[known(2000)]), wait("6s", &[known(1920)])];
        assert_eq!(verdict(&waits), DamatenValueVerdict::AboveThreshold);
    }

    #[test]
    fn an_unknown_variant_only_matters_when_nothing_else_decides() {
        // 確定した役なし / threshold 未満があれば、それだけでリーチ側に倒せる。
        let with_no_yaku = [wait("3s", &[DamatenValue::Unknown, DamatenValue::NoYaku])];
        assert_eq!(verdict(&with_no_yaku), DamatenValueVerdict::NoYaku);

        let with_low_value = [wait("3s", &[DamatenValue::Unknown, known(960)])];
        assert_eq!(
            verdict(&with_low_value),
            DamatenValueVerdict::BelowThreshold
        );

        let with_high_value = [wait("3s", &[DamatenValue::Unknown, known(2000)])];
        let verdict = verdict(&with_high_value);
        assert_eq!(verdict, DamatenValueVerdict::Indeterminate);
        assert!(!verdict.is_conclusive());
    }

    #[test]
    fn the_baseline_keeps_only_the_known_winds() {
        // 場風・自風が不明な局面では推測せず不明のまま渡す。
        let baseline = damaten_baseline_context(&GameContext::default());

        assert_eq!(baseline.round_wind(), None);
        assert_eq!(baseline.seat_wind(), None);
        assert_eq!(baseline.win_method(), WinMethod::Ron);
        assert_eq!(baseline.riichi(), RiichiStatus::NotDeclared);
        assert_eq!(baseline.chankan(), Some(false));
        assert_eq!(baseline.rinshan(), None);
        assert_eq!(baseline.ippatsu(), None);
        assert_eq!(
            baseline.remaining_live_tiles(),
            Some(BASELINE_REMAINING_LIVE_TILES)
        );
    }
}
