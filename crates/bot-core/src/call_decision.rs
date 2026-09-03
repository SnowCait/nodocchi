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
//! | 喰い替え禁止牌 | [`forbidden_discards_after_call`] |
//! | 副露込みの向聴数 | [`calculate_shanten_with_fixed_melds`] |
//! | 鳴き後の打牌選択 | [`select_best_one_step_discard_evaluation_with_fixed_meld_count`] |
//! | 待ちと残枚数 | [`DiscardEvaluation::acceptance_after_discard`] / [`TenpaiWaitAvailability`] |
//! | ロン可否 | [`TenpaiWaitAvailability::can_ron`] |
//! | 役の有無 | [`evaluate_tenpai_hand_value`] |
//!
//! 向聴・受け入れ・待ち・フリテン・役・点数をこの層で計算し直さない。
//!
//! # 鳴き後の打牌
//!
//! 鳴いた直後に切れない牌 (喰い替え) は戦術ではなく合法手の制約なので、鳴き後の打牌候補から
//! 先に取り除いてから既存の打牌比較へ渡す。したがって「喰い替え禁止牌を切ればテンパイする」を
//! 理由に鳴くことはない。鳴き専用の比較順は持たない。
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
//!
//! # 1向聴のまま鳴く候補の観測
//!
//! ```text
//! 現在1向聴 → 鳴く → 最良打牌 → 1向聴のまま
//! ```
//!
//! で [`CallDecisionReason::PostCallNotTenpai`] として落ちる候補についてだけ、
//! [`CallIishantenAcceptanceDiagnostic`] を観測用に残す。成立条件にも候補の選択にも使わない
//! diagnostics 専用の値で、これがあるかどうかで `ShantenAgent::act()` の結果は変わらない。
//!
//! 解析専用なので、収集するのは `diagnose()` 経路だけ。通常の `act()` 経路は
//! `collect_iishanten_acceptance == false` で呼ばれ、鳴かない場合の受け入れも固定面子の役保証も
//! そもそも計算しない。鳴き policy 自体は enabled / disabled で共通の1本のまま。
//!
//! 2向聴から鳴いて1向聴になる候補は対象にしない。鳴かない側と鳴いた側で探索の深さが揃わず、
//! 受け入れ枚数をそのまま比べられないため。

use bot_logic::{
    DiscardEvaluation, FixedMeldCount, HandValueError, HandValueOutcome, Meld, MeldKind,
    OwnDiscards, TenpaiWaitAvailability, TileCounts, TileId, TileType,
    best_discard_selection_index, calculate_acceptance_with_fixed_melds_and_visible_tiles,
    calculate_shanten_with_fixed_melds, discard_tenpai_wait_availability,
    evaluate_tenpai_hand_value, fixed_melds_guarantee_yaku, split_discarded_tile,
    tenpai_completed_hands,
};

use crate::action::LegalAction;
use crate::context::GameContext;
use crate::damaten_value::damaten_baseline_context;
use crate::discard_selection::select_best_one_step_discard_evaluation_with_fixed_meld_count;
use crate::kuikae::forbidden_discards_after_call;

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
    /// 鳴き後の手牌に、喰い替え禁止牌を除いた合法な打牌候補が無い。
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

/// 1向聴のまま鳴く候補についての、鳴かない場合と鳴いた場合の受け入れ比較。
///
/// production の鳴き判断はこの値を読まない。将来
/// 「1向聴 → 鳴いて1向聴だが受け入れが大きく改善する」を policy へ入れるかどうかを実戦局面で
/// 観測するためだけに持つ。閾値も比も置かない。
///
/// | 値 | source of truth |
/// | --- | --- |
/// | 鳴かない場合の受け入れ | [`calculate_acceptance_with_fixed_melds_and_visible_tiles`] |
/// | 鳴いた後の向聴と受け入れ | [`CallCandidateDiagnostic::post_call_discard`] |
/// | 固定面子だけの役保証 | [`fixed_melds_guarantee_yaku`] |
///
/// どれも production 評価が既に求めた値か既存 calculator の結果そのもので、診断のために向聴・
/// 受け入れ・役を計算し直さない。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CallIishantenAcceptanceDiagnostic {
    /// 鳴かずに現在の手牌のまま進めた場合の受け入れ残枚数 [枚]。
    pub pass_acceptance_remaining: u8,
    /// 同じく受け入れ牌種数。
    pub pass_acceptance_type_count: usize,
    /// 鳴き後の最良打牌の向聴数。この診断を持つ候補では常に [`CALL_CURRENT_SHANTEN`]。
    pub post_call_shanten: i8,
    /// 鳴き後の最良打牌の受け入れ残枚数 [枚]。
    pub post_call_acceptance_remaining: u8,
    /// 同じく受け入れ牌種数。
    pub post_call_acceptance_type_count: usize,
    /// 既存副露 + 今回の Chi / Pon の固定面子だけで、将来の完成形に役が保証されるか。
    ///
    /// 場風・自風が不明な場合は既存 semantics のまま `false`。役ありだと推測しない。
    pub fixed_melds_guarantee_yaku: bool,
}

impl CallIishantenAcceptanceDiagnostic {
    /// 鳴いた場合 - 鳴かない場合の受け入れ残枚数差 [枚]。符号付き。
    pub fn acceptance_remaining_delta(&self) -> i16 {
        i16::from(self.post_call_acceptance_remaining) - i16::from(self.pass_acceptance_remaining)
    }

    /// 鳴いた場合 - 鳴かない場合の受け入れ牌種数差。符号付き。
    pub fn acceptance_type_delta(&self) -> isize {
        self.post_call_acceptance_type_count as isize - self.pass_acceptance_type_count as isize
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
    /// 鳴いた直後に切れない牌種。打牌候補を評価しなかった場合は `None`。
    ///
    /// production の打牌選択が実際に除外に使った値そのもので、診断のために求め直さない。
    pub post_call_forbidden_discards: Option<Vec<TileType>>,
    /// 喰い替え禁止牌を除いた合法な打牌候補の中の最良打牌評価。
    pub post_call_discard: Option<DiscardEvaluation>,
    /// 鳴き後の打牌でテンパイになる場合の待ちとロン可否。
    pub post_call_wait: Option<TenpaiWaitAvailability>,
    /// 鳴き後テンパイの和了牌の物理牌ごとの役診断。役を評価しなかった場合は `None`。
    pub post_call_wait_yaku: Option<Vec<CallWaitYakuDiagnostic>>,
    /// 鳴いても1向聴のままの候補についてだけ求める観測用の受け入れ比較。対象外の候補と、
    /// そこまで評価が進まなかった候補では `None`。
    pub iishanten_acceptance: Option<CallIishantenAcceptanceDiagnostic>,
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
//
// `collect_iishanten_acceptance` は解析専用の [`CallIishantenAcceptanceDiagnostic`] を集めるか
// どうかだけを切り替える。判断に使う fact の評価と候補の選択は切り替えの影響を受けない。
pub(crate) fn evaluate_call_decision(
    ctx: &GameContext,
    legal_actions: &[LegalAction],
    collect_iishanten_acceptance: bool,
) -> Option<CallDecisionDiagnostic> {
    let mut candidates: Vec<CallCandidateDiagnostic> = legal_actions
        .iter()
        .filter_map(|action| {
            normalize_call(action).map(|(kind, tile, consumed)| {
                evaluate_call_candidate(
                    ctx,
                    action,
                    kind,
                    tile,
                    consumed,
                    collect_iishanten_acceptance,
                )
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
    collect_iishanten_acceptance: bool,
) -> CallCandidateDiagnostic {
    let mut candidate = CallCandidateDiagnostic {
        action: action.clone(),
        kind,
        current_fixed_meld_count: None,
        current_shanten: None,
        post_call_fixed_meld_count: None,
        post_call_forbidden_discards: None,
        post_call_discard: None,
        post_call_wait: None,
        post_call_wait_yaku: None,
        iishanten_acceptance: None,
        eligible: false,
        selected: false,
        reason: CallDecisionReason::EligibleTenpai,
    };

    let reason = evaluate_call_conditions(
        ctx,
        kind,
        tile,
        consumed,
        collect_iishanten_acceptance,
        &mut candidate,
    );
    candidate.eligible = reason == CallDecisionReason::EligibleTenpai;
    candidate.reason = reason;
    candidate
}

// 鳴き成立条件を順に評価し、最初に落ちた条件を理由として返す。評価が進んだ範囲の値だけを
// candidate へ書き込み、評価しなかった項目は None のままにする。
//
// 判断に使う fact は `collect_iishanten_acceptance` にかかわらず常に同じ順序で評価する。この
// flag が切り替えるのは、判断に使わない観測値を最後に足すかどうかだけ。
fn evaluate_call_conditions(
    ctx: &GameContext,
    kind: CallKind,
    tile: TileId,
    consumed: &[TileId],
    collect_iishanten_acceptance: bool,
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

    // 喰い替え禁止牌は合法手の制約なので、打牌候補を比較する前に取り除く。
    let forbidden_discards = forbidden_discards_after_call(&meld);
    let evaluation = select_best_one_step_discard_evaluation_with_fixed_meld_count(
        ctx,
        &post_call_tiles,
        post_call_fixed_meld_count,
        &forbidden_discards,
    );
    candidate.post_call_forbidden_discards = Some(forbidden_discards);

    let Some(evaluation) = evaluation else {
        return CallDecisionReason::NoPostCallDiscard;
    };

    if evaluation.min_shanten_after_discard() != CALL_TENPAI_SHANTEN {
        // 判断はここで確定していて、以降は解析用の観測値を足すだけ。
        if collect_iishanten_acceptance {
            candidate.iishanten_acceptance = iishanten_acceptance_diagnostic(
                ctx,
                &counts,
                current_fixed_meld_count,
                &meld,
                &evaluation,
            );
        }
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

// 鳴いても1向聴のままの候補について、鳴かない場合と鳴いた場合の受け入れを並べる。
//
// diagnostics が有効な場合だけ呼ばれる。返り値は成立条件にも候補の選択にも使わない。鳴いた
// 後の向聴・受け入れは本番の打牌評価 `evaluation` が持つ値をそのまま読み、鳴かない場合の受け
// 入れは既存の受け入れ計算へそのまま渡す。どちらもここで数え直さない。
//
// 対象は鳴き後の最良打牌が1向聴のままの候補だけ。テンパイになる候補は既存診断で足り、2向聴から
// の鳴きは尺度が揃わないので対象にしない。
fn iishanten_acceptance_diagnostic(
    ctx: &GameContext,
    counts: &TileCounts,
    current_fixed_meld_count: FixedMeldCount,
    meld: &Meld,
    evaluation: &DiscardEvaluation,
) -> Option<CallIishantenAcceptanceDiagnostic> {
    let post_call_shanten = evaluation.min_shanten_after_discard();
    if post_call_shanten != CALL_CURRENT_SHANTEN {
        return None;
    }

    // 鳴かない場合の受け入れは、現在の副露済み面子数と見え牌をそのまま反映した既存計算。
    let pass_acceptance = calculate_acceptance_with_fixed_melds_and_visible_tiles(
        counts,
        current_fixed_meld_count,
        ctx.visible_tiles(),
    );

    // 役保証の対象は既存副露 + 今回の面子。牌種による役牌判定をこの層で持たない。
    let mut fixed_melds: Vec<Meld> = ctx.own_melds().unwrap_or_default().to_vec();
    fixed_melds.push(meld.clone());

    Some(CallIishantenAcceptanceDiagnostic {
        pass_acceptance_remaining: pass_acceptance.total_remaining(),
        pass_acceptance_type_count: pass_acceptance.tiles.len(),
        post_call_shanten,
        post_call_acceptance_remaining: evaluation.acceptance_total_remaining(),
        post_call_acceptance_type_count: evaluation.acceptance_type_count(),
        fixed_melds_guarantee_yaku: fixed_melds_guarantee_yaku(
            &fixed_melds,
            damaten_baseline_context(ctx),
        ),
    })
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

    // 他家 (player 1) の打牌へ反応する局面。東場東家・リーチ者なし・副露なし・ツモ牌なしで、
    // 鳴き判断が読む fact だけを組み立てる。
    fn reaction_context(hand: &[u8], target: u8) -> GameContext {
        let hand_tiles = tiles(hand);
        let mut visible = hand_tiles.clone();
        visible.push(tile(target));

        GameContext::from_parts_with_melds(
            None,
            hand_tiles,
            vec![],
            TileType::new(EAST),
            TileType::new(EAST),
            visible,
            Some(0),
            Some(0),
            [vec![], vec![tile(target)], vec![], vec![]],
            [false; 4],
            Default::default(),
        )
        // 実際の client が局開始で確定させる値。unknown だと全ての鳴きがロン可否不明で落ちる。
        .with_history_furiten_facts(bot_logic::HistoryFuritenFacts {
            same_turn: Some(false),
            riichi_missed_win: Some(false),
        })
    }

    fn pon_action(target: u8, consumed: &[u8]) -> LegalAction {
        LegalAction::Pon {
            tile: tile(target),
            consumed: tiles(consumed),
        }
    }

    fn chi_action(target: u8, consumed: &[u8]) -> LegalAction {
        LegalAction::Chi {
            tile: tile(target),
            consumed: tiles(consumed),
        }
    }

    fn single_candidate(
        ctx: &GameContext,
        action: &LegalAction,
        collect_iishanten_acceptance: bool,
    ) -> (CallDecisionDiagnostic, CallCandidateDiagnostic) {
        let decision = evaluate_call_decision(
            ctx,
            &[action.clone(), LegalAction::None],
            collect_iishanten_acceptance,
        )
        .expect("evaluated");
        assert_eq!(decision.candidates.len(), 1);
        let candidate = decision.candidates[0].clone();
        (decision, candidate)
    }

    const EAST: u8 = 27;

    // 234567m 68p 24s E FF の一向聴。FF を Pon して E を切っても一向聴のままで、雀頭が無い
    // 3面子2搭子になる。
    const IISHANTEN_PON_HAND: [u8; 13] = [4, 8, 12, 17, 20, 24, 56, 64, 76, 84, 108, 128, 129];
    const IISHANTEN_PON_TARGET: u8 = 130;
    const IISHANTEN_PON_CONSUMED: [u8; 2] = [128, 129];

    // 345m 789m 68p 24s E FF の一向聴。4m5m で 3m を Chi して E を切っても一向聴のまま。
    const IISHANTEN_CHI_HAND: [u8; 13] = [8, 12, 17, 24, 28, 32, 56, 64, 76, 84, 108, 128, 129];
    const IISHANTEN_CHI_TARGET: u8 = 9;
    const IISHANTEN_CHI_CONSUMED: [u8; 2] = [12, 17];

    // 123456m 55p 78s N PP の一向聴。PP を Pon して N を切ると即テンパイ。
    const TENPAI_PON_HAND: [u8; 13] = [0, 4, 8, 12, 17, 20, 53, 54, 96, 100, 120, 124, 125];
    const TENPAI_PON_TARGET: u8 = 126;
    const TENPAI_PON_CONSUMED: [u8; 2] = [124, 125];

    // 234m 68m 68p 24s E C FF の二向聴。
    const RYANSHANTEN_PON_HAND: [u8; 13] = [4, 8, 12, 20, 28, 56, 64, 76, 84, 108, 132, 128, 129];

    #[test]
    fn an_iishanten_call_that_stays_iishanten_compares_the_pass_and_post_call_acceptance() {
        let ctx = reaction_context(&IISHANTEN_PON_HAND, IISHANTEN_PON_TARGET);
        let action = pon_action(IISHANTEN_PON_TARGET, &IISHANTEN_PON_CONSUMED);
        let (decision, candidate) = single_candidate(&ctx, &action, true);

        assert_eq!(candidate.reason, CallDecisionReason::PostCallNotTenpai);
        assert_eq!(candidate.current_shanten, Some(CALL_CURRENT_SHANTEN));
        assert_eq!(candidate.post_call_shanten(), Some(CALL_CURRENT_SHANTEN));

        let acceptance = candidate
            .iishanten_acceptance
            .expect("1向聴 → 1向聴 が対象");

        // 鳴かなかった場合の受け入れは、現在の副露済み面子数と見え牌を反映した既存計算そのもの。
        let pass = calculate_acceptance_with_fixed_melds_and_visible_tiles(
            &TileCounts::from_tiles(ctx.hand_tiles().iter().copied()),
            ctx.own_fixed_meld_count().unwrap(),
            ctx.visible_tiles(),
        );
        assert_eq!(acceptance.pass_acceptance_remaining, pass.total_remaining());
        assert_eq!(acceptance.pass_acceptance_type_count, pass.tiles.len());

        // 鳴いた後の向聴と受け入れは、本番の鳴き後打牌評価が持つ値そのもの。
        let evaluation = candidate.post_call_discard.as_ref().unwrap();
        assert_eq!(
            acceptance.post_call_shanten,
            evaluation.min_shanten_after_discard()
        );
        assert_eq!(
            acceptance.post_call_acceptance_remaining,
            evaluation.acceptance_total_remaining()
        );
        assert_eq!(
            acceptance.post_call_acceptance_type_count,
            evaluation.acceptance_type_count()
        );

        assert_eq!(
            (
                acceptance.pass_acceptance_remaining,
                acceptance.pass_acceptance_type_count
            ),
            (8, 2)
        );
        assert_eq!(
            (
                acceptance.post_call_acceptance_remaining,
                acceptance.post_call_acceptance_type_count
            ),
            (20, 6)
        );
        assert_eq!(acceptance.acceptance_remaining_delta(), 12);
        assert_eq!(acceptance.acceptance_type_delta(), 4);

        // 観測用の値で、受け入れが増えても鳴かない判断のまま。
        assert!(!candidate.eligible);
        assert_eq!(decision.selected, None);
    }

    #[test]
    fn the_fixed_meld_yaku_guarantee_comes_from_the_shared_helper() {
        let ctx = reaction_context(&IISHANTEN_PON_HAND, IISHANTEN_PON_TARGET);
        let action = pon_action(IISHANTEN_PON_TARGET, &IISHANTEN_PON_CONSUMED);
        let (_, candidate) = single_candidate(&ctx, &action, true);

        let acceptance = candidate
            .iishanten_acceptance
            .expect("1向聴 → 1向聴 が対象");
        let melds = vec![Meld::new(
            MeldKind::Pon,
            tiles(&[
                IISHANTEN_PON_TARGET,
                IISHANTEN_PON_CONSUMED[0],
                IISHANTEN_PON_CONSUMED[1],
            ]),
            Some(tile(IISHANTEN_PON_TARGET)),
        )];

        assert_eq!(
            acceptance.fixed_melds_guarantee_yaku,
            fixed_melds_guarantee_yaku(&melds, damaten_baseline_context(&ctx))
        );
        assert!(acceptance.fixed_melds_guarantee_yaku);
    }

    #[test]
    fn a_chi_meld_does_not_guarantee_a_yaku() {
        let ctx = reaction_context(&IISHANTEN_CHI_HAND, IISHANTEN_CHI_TARGET);
        let action = chi_action(IISHANTEN_CHI_TARGET, &IISHANTEN_CHI_CONSUMED);
        let (_, candidate) = single_candidate(&ctx, &action, true);

        assert_eq!(candidate.reason, CallDecisionReason::PostCallNotTenpai);
        assert_eq!(candidate.post_call_shanten(), Some(CALL_CURRENT_SHANTEN));

        let acceptance = candidate
            .iishanten_acceptance
            .expect("1向聴 → 1向聴 が対象");
        let melds = vec![Meld::new(
            MeldKind::Chi,
            tiles(&[
                IISHANTEN_CHI_TARGET,
                IISHANTEN_CHI_CONSUMED[0],
                IISHANTEN_CHI_CONSUMED[1],
            ]),
            Some(tile(IISHANTEN_CHI_TARGET)),
        )];

        assert_eq!(
            acceptance.fixed_melds_guarantee_yaku,
            fixed_melds_guarantee_yaku(&melds, damaten_baseline_context(&ctx))
        );
        assert!(!acceptance.fixed_melds_guarantee_yaku);
    }

    #[test]
    fn an_immediate_tenpai_call_keeps_the_existing_eligible_tenpai_decision() {
        let ctx = reaction_context(&TENPAI_PON_HAND, TENPAI_PON_TARGET);
        let action = pon_action(TENPAI_PON_TARGET, &TENPAI_PON_CONSUMED);
        let (decision, candidate) = single_candidate(&ctx, &action, true);

        assert_eq!(candidate.reason, CallDecisionReason::EligibleTenpai);
        assert!(candidate.eligible);
        assert_eq!(decision.selected.as_ref(), Some(&action));
        assert_eq!(candidate.post_call_shanten(), Some(CALL_TENPAI_SHANTEN));
        // 即テンパイ候補は既存診断で足りるので、1向聴 → 1向聴 の観測対象にしない。
        assert_eq!(candidate.iishanten_acceptance, None);

        // 診断を集めない通常経路でも同じ判断。
        let (production, production_candidate) = single_candidate(&ctx, &action, false);
        assert_eq!(production_candidate, candidate);
        assert_eq!(production.selected, decision.selected);
    }

    #[test]
    fn two_shanten_is_not_part_of_the_iishanten_acceptance_diagnostic() {
        let ctx = reaction_context(&RYANSHANTEN_PON_HAND, IISHANTEN_PON_TARGET);
        let action = pon_action(IISHANTEN_PON_TARGET, &IISHANTEN_PON_CONSUMED);
        let (decision, candidate) = single_candidate(&ctx, &action, true);

        assert_eq!(candidate.reason, CallDecisionReason::CurrentShantenNotOne);
        assert_eq!(candidate.current_shanten, Some(2));
        assert_eq!(candidate.iishanten_acceptance, None);
        assert_eq!(decision.selected, None);
    }

    #[test]
    fn the_iishanten_acceptance_is_not_collected_without_diagnostics() {
        let ctx = reaction_context(&IISHANTEN_PON_HAND, IISHANTEN_PON_TARGET);
        let action = pon_action(IISHANTEN_PON_TARGET, &IISHANTEN_PON_CONSUMED);
        let (decision, candidate) = single_candidate(&ctx, &action, false);

        // 解析専用の観測値なので、通常の判断経路では構築しない。
        assert_eq!(candidate.iishanten_acceptance, None);

        // 判断に使う fact と結論は診断の有無で変わらない。
        assert_eq!(candidate.reason, CallDecisionReason::PostCallNotTenpai);
        assert_eq!(candidate.current_shanten, Some(CALL_CURRENT_SHANTEN));
        assert_eq!(candidate.post_call_shanten(), Some(CALL_CURRENT_SHANTEN));
        assert!(!candidate.eligible);
        assert_eq!(decision.selected, None);

        let (_, diagnosed) = single_candidate(&ctx, &action, true);
        assert!(diagnosed.iishanten_acceptance.is_some());
        assert_eq!(
            CallCandidateDiagnostic {
                iishanten_acceptance: None,
                ..diagnosed
            },
            candidate
        );
    }

    #[test]
    fn the_iishanten_acceptance_diagnostic_does_not_change_the_selected_action() {
        let ctx = reaction_context(&IISHANTEN_PON_HAND, IISHANTEN_PON_TARGET);
        let action = pon_action(IISHANTEN_PON_TARGET, &IISHANTEN_PON_CONSUMED);
        let actions = [action, LegalAction::None];

        let mut agent = crate::agents::ShantenAgent;
        let acted = crate::agent::Agent::act(&mut agent, &ctx, &actions);
        assert_eq!(acted, LegalAction::None);

        // diagnose() は観測値を集めるが、選ぶ action は act() と同じ。
        let diagnostic = crate::agents::ShantenAgent::diagnose(&ctx, &actions);
        assert_eq!(diagnostic.selected_action, acted);
        let call = diagnostic.call.as_ref().expect("evaluated");
        assert_eq!(call.selected, None);
        assert!(call.candidates[0].iishanten_acceptance.is_some());
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
