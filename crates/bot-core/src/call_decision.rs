//! Chi / Pon の鳴き判断 policy 層。
//!
//! 対象は
//!
//! ```text
//! 現在1向聴 → Chi / Pon → 打牌 → テンパイ
//! ```
//!
//! だけで、Chi と Pon は同じ評価 path を通る。鳴き種別ごとの専用 rule は持たず、役牌 Pon の
//! ような牌種による gating も行わない。
//!
//! # source of truth
//!
//! この層は「どの条件で鳴くか」だけを持ち、判断材料は既存 layer の結果をそのまま使う。
//!
//! | 材料 | source of truth |
//! | --- | --- |
//! | 面子の形の検証 | [`Meld::shape`] |
//! | 副露込みの向聴数 | [`calculate_shanten_with_fixed_melds`] |
//! | 鳴き後の打牌選択 | [`select_best_one_step_discard_evaluation_with_fixed_meld_count`] |
//! | 待ちと残枚数 | [`DiscardEvaluation::acceptance_after_discard`] / [`TenpaiWaitAvailability`] |
//! | ロン可否 | [`TenpaiWaitAvailability::can_ron`] |
//! | 役の有無 | [`evaluate_tenpai_hand_value`] |
//!
//! 向聴・受け入れ・待ち・フリテン・役・点数をこの層で計算し直さない。
//!
//! # 成立条件
//!
//! ```text
//! 他家リーチなし
//! AND 現在の effective shanten == 1
//! AND 合法な Chi または Pon
//! AND 鳴いた後の最良打牌で effective shanten == 0
//! AND can_ron == Some(true)
//! AND 生きた待ちの残枚数合計 >= CALL_MIN_LIVE_WAIT_REMAINING
//! AND 残枚数 > 0 の全ての和了牌 variant に役がある
//! ```
//!
//! これ以外では鳴かない。他家にリーチ者がいる局面の鳴きは押し引きへ通さず、打点による例外も
//! 持たない。
//!
//! # 片和了
//!
//! 役の有無は牌種単位ではなく、和了牌の物理牌 (赤5 / 黒5) ごとの variant 単位で見る。残枚数が
//! 0 の variant は現在ロンできないので判定対象にせず、残枚数 > 0 の variant に1つでも役なしが
//! あれば鳴かない。役の有無を確定できない variant がある場合も、役ありだと推測せず鳴かない。

use bot_logic::{
    DiscardEvaluation, FixedMeldCount, HandValueError, HandValueOutcome, Meld, MeldKind,
    OwnDiscards, TenpaiWaitAvailability, TileCounts, TileId, best_discard_selection_index,
    calculate_shanten_with_fixed_melds, discard_tenpai_wait_availability,
    evaluate_tenpai_hand_value, split_discarded_tile, tenpai_completed_hands,
};

use crate::action::LegalAction;
use crate::context::GameContext;
use crate::damaten_value::damaten_baseline_context;
use crate::discard_selection::select_best_one_step_discard_evaluation_with_fixed_meld_count;

/// 鳴きを検討する現在の向聴数。今回は 1向聴 → テンパイ だけを対象にする。
pub const CALL_CURRENT_SHANTEN: i8 = 1;

/// 鳴き後の打牌でテンパイと判断する向聴数。
pub const CALL_TENPAI_SHANTEN: i8 = 0;

/// 鳴くために必要な、鳴き後テンパイの生きた待ちの残枚数合計 [枚]。inclusive。
pub const CALL_MIN_LIVE_WAIT_REMAINING: u8 = 3;

// Chi / Pon の consumed 枚数。
const CALL_CONSUMED_TILE_COUNT: usize = 2;

/// 評価対象の鳴き種別。今回の対象は Chi と Pon だけで、Kan は含まない。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CallKind {
    Chi,
    Pon,
}

impl CallKind {
    /// 対応する既存の副露種別。
    pub fn meld_kind(self) -> MeldKind {
        match self {
            Self::Chi => MeldKind::Chi,
            Self::Pon => MeldKind::Pon,
        }
    }
}

/// 鳴きを採用した / しなかった理由。
///
/// [`Self::EligibleTenpai`] 以外はすべて「今回は鳴かない」理由であり、最初に落ちた条件を1つ
/// だけ表す。判定順は [`CallCandidateDiagnostic`] のフィールドが埋まる順と一致する。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CallDecisionReason {
    /// 全条件を満たし、鳴き後に生きた待ちのテンパイになる。
    EligibleTenpai,
    /// 他家にリーチ者がいる。今回の鳴きは押し引きへ通さない。
    OpponentReached,
    /// reaction context に `drawn_tile` があり局面として不整合。14枚扱いで判断しない。
    UnexpectedDrawnTile,
    /// consumed が2枚でない・手牌に無い・物理牌が重複している・面子の形にならないなどで
    /// 鳴き後の手牌を組み立てられない。
    InvalidConsumed,
    /// 自分の副露済み面子数が不明。0副露と推測しない。
    FixedMeldCountUnknown,
    /// 鳴き後の副露済み面子数が上限を超える。
    FixedMeldCountOverflow,
    /// 現在の effective shanten が1向聴ではない。
    CurrentShantenNotOne,
    /// 鳴き後の手牌から打牌候補を評価できない。
    NoPostCallDiscard,
    /// 鳴き後の最良打牌でもテンパイにならない。
    PostCallNotTenpai,
    /// 鳴き後はテンパイだが、待ち牌がすべて見えている。
    NoLiveAcceptance,
    /// 生きた待ちはあるが、残枚数合計が [`CALL_MIN_LIVE_WAIT_REMAINING`] 未満。
    TooFewLiveWaits,
    /// 鳴き後テンパイでロンできない。フリテンとロン可否 unknown のどちらもここに含む。
    CannotRon,
    /// 残枚数 > 0 の和了牌 variant に役なしがある。片和了は許可しない。
    YakuMissing,
    /// 残枚数 > 0 の和了牌 variant に、役の有無を確定できないものがある。
    HandValueUnknown,
}

/// 和了牌の物理牌1つ分の役の有無。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CallWaitYaku {
    /// 既存 [`HandValueOutcome::Known`] として役が確定した。
    Present,
    /// 既存 [`HandValueOutcome::NoCandidate`] で役が無いと確定した。
    Absent,
    /// 役の有無を確定できない。点数計算の入力不足や裏ドラ未確定の場合。
    Unknown,
}

/// 鳴き後テンパイの和了牌の物理牌1つ分の役診断。
///
/// 赤5と黒5は別の variant として並ぶ。`remaining` は既存受け入れの残枚数を赤 / 黒へ分けた値で、
/// ここで数え直さない。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CallWaitYakuDiagnostic {
    pub winning_tile: TileId,
    /// この variant の残枚数。
    pub remaining: u8,
    pub yaku: CallWaitYaku,
}

impl CallWaitYakuDiagnostic {
    /// 現在まだロンできる variant か。`remaining == 0` の variant は片和了判定の対象外。
    pub fn is_live(&self) -> bool {
        self.remaining > 0
    }

    pub fn is_red(&self) -> bool {
        self.winning_tile.is_red()
    }
}

/// 合法な `LegalAction::Chi` / `LegalAction::Pon` 1件ごとの判断内訳。
///
/// 各フィールドは判定が実際にそこまで進んだ場合だけ `Some` になり、進まなかった判定は推測せず
/// `None` のままにする。値はすべて本番の selector が使った結果そのもので、診断のために向聴・
/// 待ち・役を計算し直さない。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CallCandidateDiagnostic {
    pub action: LegalAction,
    pub kind: CallKind,
    pub current_fixed_meld_count: Option<FixedMeldCount>,
    /// `calculate_shanten_with_fixed_melds()` で求めた現在の effective shanten。
    pub current_shanten: Option<i8>,
    pub post_call_fixed_meld_count: Option<FixedMeldCount>,
    /// 鳴き後の最良打牌評価。
    pub post_call_discard: Option<DiscardEvaluation>,
    /// 鳴き後の打牌でテンパイになる場合の待ちとロン可否。
    pub post_call_wait: Option<TenpaiWaitAvailability>,
    /// 鳴き後テンパイの和了牌の物理牌ごとの役診断。役を評価しなかった場合は `None`。
    pub post_call_wait_yaku: Option<Vec<CallWaitYakuDiagnostic>>,
    pub eligible: bool,
    pub selected: bool,
    pub reason: CallDecisionReason,
}

impl CallCandidateDiagnostic {
    pub fn post_call_shanten(&self) -> Option<i8> {
        self.post_call_discard
            .as_ref()
            .map(DiscardEvaluation::min_shanten_after_discard)
    }

    pub fn post_call_acceptance_total_remaining(&self) -> Option<u8> {
        self.post_call_discard
            .as_ref()
            .map(DiscardEvaluation::acceptance_total_remaining)
    }

    pub fn post_call_acceptance_type_count(&self) -> Option<usize> {
        self.post_call_discard
            .as_ref()
            .map(DiscardEvaluation::acceptance_type_count)
    }

    /// 鳴き後テンパイでツモ和了できる待ちの残枚数合計。テンパイにならない場合は `None`。
    pub fn live_wait_remaining(&self) -> Option<u8> {
        self.post_call_wait
            .as_ref()
            .map(|wait| wait.tsumo_remaining)
    }

    /// 鳴き後テンパイの総合ロン可否。テンパイにならない場合と判断できない場合は `None`。
    pub fn can_ron(&self) -> Option<bool> {
        self.post_call_wait
            .as_ref()
            .and_then(TenpaiWaitAvailability::can_ron)
    }

    /// 残枚数 > 0 の和了牌 variant すべてで役ありを確定できたか。役を評価しなかった場合は
    /// `None`。
    pub fn live_waits_have_yaku(&self) -> Option<bool> {
        self.post_call_wait_yaku.as_ref().map(|waits| {
            waits
                .iter()
                .filter(|wait| wait.is_live())
                .all(|wait| wait.yaku == CallWaitYaku::Present)
        })
    }
}

/// 鳴き判断の構造化診断。
///
/// `selected` は `ShantenAgent::act()` が実際に採用した鳴きそのもので、診断用の別判断ロジック
/// は持たない。採用が無い場合の `reason` は最初の候補が落ちた理由。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CallDecisionDiagnostic {
    pub selected: Option<LegalAction>,
    pub reason: CallDecisionReason,
    pub candidates: Vec<CallCandidateDiagnostic>,
}

// 鳴き判断の本体。act() と構造化診断はこの1本を共有し、診断は結果を載せるだけにする。
//
// 合法な Chi / Pon が1件も無ければ検討自体を行わず None。1件以上ある場合は候補ごとに独立して
// 条件を評価し、成立した候補の中から1件を選ぶ。
pub(crate) fn evaluate_call_decision(
    ctx: &GameContext,
    legal_actions: &[LegalAction],
) -> Option<CallDecisionDiagnostic> {
    let mut candidates: Vec<CallCandidateDiagnostic> = legal_actions
        .iter()
        .filter_map(|action| {
            normalize_call(action).map(|(kind, tile, consumed)| {
                evaluate_call_candidate(ctx, action, kind, tile, consumed)
            })
        })
        .collect();

    if candidates.is_empty() {
        return None;
    }

    let selected_index = select_eligible_candidate(&candidates);
    if let Some(index) = selected_index {
        candidates[index].selected = true;
    }

    let reason = candidates[selected_index.unwrap_or(0)].reason;
    let selected = selected_index.map(|index| candidates[index].action.clone());

    Some(CallDecisionDiagnostic {
        selected,
        reason,
        candidates,
    })
}

// 合法 action を Chi / Pon の共通表現へ正規化する。それ以外の action は対象外。
fn normalize_call(action: &LegalAction) -> Option<(CallKind, TileId, &[TileId])> {
    match action {
        LegalAction::Chi { tile, consumed } => Some((CallKind::Chi, *tile, consumed)),
        LegalAction::Pon { tile, consumed } => Some((CallKind::Pon, *tile, consumed)),
        _ => None,
    }
}

// 成立した候補の中から採用する1件を選ぶ。
//
// 比較軸は鳴き後の最良打牌評価で、通常打牌選択と同じ既存 comparator をそのまま使う。鳴き専用の
// EV や重み付けは持たない。完全に同値な候補では先に現れた候補を維持するため、合法 action の
// 列挙順が安定した tie-break になる。
fn select_eligible_candidate(candidates: &[CallCandidateDiagnostic]) -> Option<usize> {
    let (indices, evaluations): (Vec<usize>, Vec<DiscardEvaluation>) = candidates
        .iter()
        .enumerate()
        .filter(|(_, candidate)| candidate.eligible)
        .filter_map(|(index, candidate)| {
            candidate
                .post_call_discard
                .clone()
                .map(|evaluation| (index, evaluation))
        })
        .unzip();

    best_discard_selection_index(&evaluations, &[]).map(|best| indices[best])
}

fn evaluate_call_candidate(
    ctx: &GameContext,
    action: &LegalAction,
    kind: CallKind,
    tile: TileId,
    consumed: &[TileId],
) -> CallCandidateDiagnostic {
    let mut candidate = CallCandidateDiagnostic {
        action: action.clone(),
        kind,
        current_fixed_meld_count: None,
        current_shanten: None,
        post_call_fixed_meld_count: None,
        post_call_discard: None,
        post_call_wait: None,
        post_call_wait_yaku: None,
        eligible: false,
        selected: false,
        reason: CallDecisionReason::EligibleTenpai,
    };

    let reason = evaluate_call_conditions(ctx, kind, tile, consumed, &mut candidate);
    candidate.eligible = reason == CallDecisionReason::EligibleTenpai;
    candidate.reason = reason;
    candidate
}

// 鳴き成立条件を順に評価し、最初に落ちた条件を理由として返す。評価が進んだ範囲の値だけを
// candidate へ書き込み、評価しなかった項目は None のままにする。
fn evaluate_call_conditions(
    ctx: &GameContext,
    kind: CallKind,
    tile: TileId,
    consumed: &[TileId],
    candidate: &mut CallCandidateDiagnostic,
) -> CallDecisionReason {
    if ctx.any_opponent_reached() {
        return CallDecisionReason::OpponentReached;
    }

    // Chi / Pon は他家捨て牌への reaction なので、既存 client の reaction context に drawn_tile は
    // 無い。drawn_tile がある不整合な context では、それを混ぜても無視しても正しい局面を復元
    // できないため鳴きを検討しない。
    if ctx.drawn_tile().is_some() {
        return CallDecisionReason::UnexpectedDrawnTile;
    }

    let hand_tiles = ctx.hand_tiles();
    let Some((meld, post_call_tiles)) =
        call_meld_and_concealed_tiles(hand_tiles, kind, tile, consumed)
    else {
        return CallDecisionReason::InvalidConsumed;
    };

    let Some(current_fixed_meld_count) = ctx.own_fixed_meld_count() else {
        return CallDecisionReason::FixedMeldCountUnknown;
    };
    candidate.current_fixed_meld_count = Some(current_fixed_meld_count);

    let Some(post_call_fixed_meld_count) = FixedMeldCount::new(current_fixed_meld_count.get() + 1)
    else {
        return CallDecisionReason::FixedMeldCountOverflow;
    };
    candidate.post_call_fixed_meld_count = Some(post_call_fixed_meld_count);

    let counts = TileCounts::from_tiles(hand_tiles.iter().copied());
    let current_shanten =
        calculate_shanten_with_fixed_melds(&counts, current_fixed_meld_count).min();
    candidate.current_shanten = Some(current_shanten);
    if current_shanten != CALL_CURRENT_SHANTEN {
        return CallDecisionReason::CurrentShantenNotOne;
    }

    let Some(evaluation) = select_best_one_step_discard_evaluation_with_fixed_meld_count(
        ctx,
        &post_call_tiles,
        post_call_fixed_meld_count,
    ) else {
        return CallDecisionReason::NoPostCallDiscard;
    };

    if evaluation.min_shanten_after_discard() != CALL_TENPAI_SHANTEN {
        candidate.post_call_discard = Some(evaluation);
        return CallDecisionReason::PostCallNotTenpai;
    }

    let Some(wait) = discard_tenpai_wait_availability(
        &TileCounts::from_tiles(post_call_tiles.iter().copied()),
        post_call_fixed_meld_count,
        &evaluation,
        &OwnDiscards::from_optional_river(ctx.own_discards()),
        ctx.history_furiten_after_own_discard(),
    ) else {
        candidate.post_call_discard = Some(evaluation);
        return CallDecisionReason::PostCallNotTenpai;
    };

    let reason =
        evaluate_post_call_conditions(ctx, &meld, &post_call_tiles, &evaluation, &wait, candidate);
    candidate.post_call_discard = Some(evaluation);
    candidate.post_call_wait = Some(wait);
    reason
}

// 鳴き後テンパイが確定してからの条件を評価する。待ち枚数・ロン可否・役の順に見る。
fn evaluate_post_call_conditions(
    ctx: &GameContext,
    meld: &Meld,
    post_call_tiles: &[TileId],
    evaluation: &DiscardEvaluation,
    wait: &TenpaiWaitAvailability,
    candidate: &mut CallCandidateDiagnostic,
) -> CallDecisionReason {
    if wait.tsumo_remaining == 0 {
        return CallDecisionReason::NoLiveAcceptance;
    }
    if wait.tsumo_remaining < CALL_MIN_LIVE_WAIT_REMAINING {
        return CallDecisionReason::TooFewLiveWaits;
    }
    // フリテンとロン可否 unknown はどちらも鳴かない。非フリテンだと推測しない。
    if wait.can_ron() != Some(true) {
        return CallDecisionReason::CannotRon;
    }

    let Some(wait_yaku) = post_call_wait_yaku(ctx, meld, post_call_tiles, evaluation, wait) else {
        return CallDecisionReason::HandValueUnknown;
    };
    let reason = live_wait_yaku_reason(&wait_yaku);
    candidate.post_call_wait_yaku = Some(wait_yaku);
    reason
}

// 鳴き後テンパイの和了牌の物理牌ごとに、既存 HandValue でロン和了できるかを評価する。
//
// 待ち牌種と残枚数は鳴き後の打牌評価が持つ受け入れがそのまま source of truth で、ここで待ちを
// 数え直さない。赤5 / 黒5 の分割も既存の physical variant 規則に任せる。和了状況は既存の
// hypothetical ロン baseline をそのまま使い、鳴き判断専用の和了状況を組み立てない。
//
// 打牌後の手牌を組み立てられない場合と完成手を解析できない場合は None。役ありだと推測しない。
fn post_call_wait_yaku(
    ctx: &GameContext,
    meld: &Meld,
    post_call_tiles: &[TileId],
    evaluation: &DiscardEvaluation,
    wait: &TenpaiWaitAvailability,
) -> Option<Vec<CallWaitYakuDiagnostic>> {
    let (_, concealed_tiles) = split_discarded_tile(post_call_tiles.to_vec(), evaluation)?;

    let mut melds: Vec<Meld> = ctx.own_melds().unwrap_or_default().to_vec();
    melds.push(meld.clone());

    let hands = tenpai_completed_hands(
        &concealed_tiles,
        &melds,
        &evaluation.acceptance_after_discard,
        Some(wait),
        ctx.visible_tiles(),
    )
    .ok()?;
    let profile = evaluate_tenpai_hand_value(
        &hands,
        damaten_baseline_context(ctx),
        ctx.dora_indicators(),
        None,
    );

    Some(
        profile
            .waits()
            .iter()
            .flat_map(|wait| wait.winning_tiles())
            .map(|winning_tile| CallWaitYakuDiagnostic {
                winning_tile: winning_tile.winning_tile(),
                remaining: winning_tile.remaining(),
                yaku: wait_yaku(winning_tile.outcome()),
            })
            .collect(),
    )
}

// 既存の手牌価値の結果を役の有無へ畳む。役なしと確定できない理由を潰さずに区別して持つ。
fn wait_yaku(outcome: Result<&HandValueOutcome<'_>, HandValueError>) -> CallWaitYaku {
    match outcome {
        Ok(HandValueOutcome::Known(_)) => CallWaitYaku::Present,
        Ok(HandValueOutcome::NoCandidate) => CallWaitYaku::Absent,
        Ok(HandValueOutcome::IndeterminateBonusHan) | Err(_) => CallWaitYaku::Unknown,
    }
}

// 残枚数 > 0 の variant だけを見て役の結論を出す。役なしが1つでもあれば片和了として鳴かない。
// 確定できない variant は役ありだと推測しない。残枚数 0 の variant は現在ロンできないので
// 判定対象にしない。
fn live_wait_yaku_reason(waits: &[CallWaitYakuDiagnostic]) -> CallDecisionReason {
    let live = || waits.iter().filter(|wait| wait.is_live());

    if live().any(|wait| wait.yaku == CallWaitYaku::Absent) {
        return CallDecisionReason::YakuMissing;
    }
    if live().any(|wait| wait.yaku == CallWaitYaku::Unknown) {
        return CallDecisionReason::HandValueUnknown;
    }
    CallDecisionReason::EligibleTenpai
}

// 鳴き後の副露面子と concealed hand を組み立てる。
//
// consumed は牌種単位で減らすのではなく物理牌 ID で除去するため、赤5を含む鳴きでも semantics を
// 保つ。枚数が2枚でない・手牌に無い・同じ物理牌が重複している場合は None。面子の形の検証は
// 既存 Meld::shape() が source of truth で、Chi なのに連続3牌でない・Pon なのに同一牌でない
// 場合も None になる。
fn call_meld_and_concealed_tiles(
    hand_tiles: &[TileId],
    kind: CallKind,
    tile: TileId,
    consumed: &[TileId],
) -> Option<(Meld, Vec<TileId>)> {
    if consumed.len() != CALL_CONSUMED_TILE_COUNT {
        return None;
    }

    let mut remaining = hand_tiles.to_vec();
    for consumed_tile in consumed {
        let position = remaining.iter().position(|held| held == consumed_tile)?;
        remaining.remove(position);
    }

    let mut tiles = Vec::with_capacity(consumed.len() + 1);
    tiles.push(tile);
    tiles.extend_from_slice(consumed);

    let meld = Meld::new(kind.meld_kind(), tiles, Some(tile));
    meld.shape()?;
    Some((meld, remaining))
}

#[cfg(test)]
mod tests {
    use super::*;

    use bot_logic::MeldShape;

    fn tile(value: u8) -> TileId {
        TileId::new(value).unwrap()
    }

    fn tiles(values: &[u8]) -> Vec<TileId> {
        values.iter().map(|&value| tile(value)).collect()
    }

    fn wait(winning_tile: u8, remaining: u8, yaku: CallWaitYaku) -> CallWaitYakuDiagnostic {
        CallWaitYakuDiagnostic {
            winning_tile: tile(winning_tile),
            remaining,
            yaku,
        }
    }

    #[test]
    fn normalizes_only_chi_and_pon() {
        let chi = LegalAction::Chi {
            tile: tile(89),
            consumed: tiles(&[84, 92]),
        };
        let pon = LegalAction::Pon {
            tile: tile(126),
            consumed: tiles(&[124, 125]),
        };

        assert_eq!(
            normalize_call(&chi).map(|(kind, ..)| kind),
            Some(CallKind::Chi)
        );
        assert_eq!(
            normalize_call(&pon).map(|(kind, ..)| kind),
            Some(CallKind::Pon)
        );

        for action in [
            LegalAction::Daiminkan {
                tile: tile(104),
                consumed: tiles(&[105, 106, 107]),
            },
            LegalAction::Ankan {
                consumed: tiles(&[72, 73, 74, 75]),
            },
            LegalAction::Kakan {
                tile: tile(124),
                consumed: tiles(&[125, 126, 127]),
            },
            LegalAction::Dahai { tile: tile(0) },
            LegalAction::Reach,
            LegalAction::None,
        ] {
            assert!(normalize_call(&action).is_none(), "{action:?}");
        }
    }

    #[test]
    fn builds_the_pon_meld_and_removes_the_consumed_physical_tiles() {
        let hand = tiles(&[0, 124, 125, 126]);
        let (meld, remaining) =
            call_meld_and_concealed_tiles(&hand, CallKind::Pon, tile(127), &tiles(&[124, 125]))
                .unwrap();

        assert_eq!(meld.kind(), MeldKind::Pon);
        assert_eq!(meld.called_tile(), Some(tile(127)));
        assert!(meld.shape().unwrap().is_triplet_like());
        // 暗刻から鳴いた場合も、除去は consumed の物理牌2枚だけ。
        assert_eq!(remaining, tiles(&[0, 126]));
    }

    #[test]
    fn builds_the_chi_meld_and_removes_the_consumed_physical_tiles() {
        let hand = tiles(&[0, 84, 92]);
        let (meld, remaining) =
            call_meld_and_concealed_tiles(&hand, CallKind::Chi, tile(89), &tiles(&[84, 92]))
                .unwrap();

        assert_eq!(meld.kind(), MeldKind::Chi);
        assert_eq!(
            meld.shape(),
            Some(MeldShape::Sequence {
                start: tile(84).tile_type()
            })
        );
        assert_eq!(remaining, tiles(&[0]));
    }

    #[test]
    fn rejects_calls_that_cannot_build_a_meld() {
        let hand = tiles(&[0, 84, 92, 124, 125, 126]);

        for (kind, called, consumed) in [
            // 枚数が2枚でない
            (CallKind::Pon, 127u8, vec![124u8]),
            (CallKind::Pon, 127, vec![124, 125, 126]),
            // 手牌に無い物理牌
            (CallKind::Pon, 127, vec![124, 127]),
            // 同じ物理牌の重複
            (CallKind::Pon, 127, vec![124, 124]),
            // 刻子にならない
            (CallKind::Pon, 127, vec![124, 0]),
            // 順子にならない
            (CallKind::Chi, 89, vec![124, 125]),
            (CallKind::Chi, 89, vec![0, 84]),
        ] {
            assert!(
                call_meld_and_concealed_tiles(&hand, kind, tile(called), &tiles(&consumed))
                    .is_none(),
                "{kind:?} {called} {consumed:?}"
            );
        }
    }

    #[test]
    fn every_live_variant_needs_a_yaku() {
        assert_eq!(
            live_wait_yaku_reason(&[
                wait(89, 2, CallWaitYaku::Present),
                wait(100, 4, CallWaitYaku::Present),
            ]),
            CallDecisionReason::EligibleTenpai
        );
    }

    #[test]
    fn a_live_variant_without_a_yaku_blocks_the_call() {
        // 役なしが確定した variant は、確定できない variant より先に理由になる。
        assert_eq!(
            live_wait_yaku_reason(&[
                wait(89, 2, CallWaitYaku::Present),
                wait(100, 1, CallWaitYaku::Absent),
                wait(104, 3, CallWaitYaku::Unknown),
            ]),
            CallDecisionReason::YakuMissing
        );
    }

    #[test]
    fn an_indeterminate_live_variant_blocks_the_call() {
        assert_eq!(
            live_wait_yaku_reason(&[
                wait(89, 2, CallWaitYaku::Present),
                wait(100, 1, CallWaitYaku::Unknown),
            ]),
            CallDecisionReason::HandValueUnknown
        );
    }

    #[test]
    fn dead_variants_are_not_part_of_the_yaku_judgement() {
        assert_eq!(
            live_wait_yaku_reason(&[
                wait(89, 3, CallWaitYaku::Present),
                wait(100, 0, CallWaitYaku::Absent),
                wait(104, 0, CallWaitYaku::Unknown),
            ]),
            CallDecisionReason::EligibleTenpai
        );
    }
}
