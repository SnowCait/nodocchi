//! Chi / Pon の鳴き判断 policy 層。
//!
//! 既存の production 対象は
//!
//! ```text
//! 現在1向聴 → Chi / Pon → 打牌 → テンパイ
//! ```
//!
//! で、これを満たさなかった候補のうち
//!
//! ```text
//! 現在1向聴 → Chi / Pon → 打牌 → 1向聴
//! ```
//!
//! だけを Pass と同じ self-tsumo continuation 尺度で比較する。Chi と Pon は同じ評価 path を
//! 通り、鳴き種別ごとの専用 rule や牌種による gating は持たない。
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
//! | 鳴き後の打牌選択 | [`select_discard_action_with_evaluation`] |
//! | 2向聴 Call observation の打牌候補 | [`post_call_discard_evaluations`] |
//! | Pass の継続評価 | [`awaiting_draw_expected_self_tsumo_value`] |
//! | 2向聴 Pass の継続評価 | [`awaiting_draw_two_shanten_expected_self_tsumo_value`] |
//! | 待ちと残枚数 | [`DiscardEvaluation::acceptance_after_discard`] / [`TenpaiWaitAvailability`] |
//! | ロン可否 | [`TenpaiWaitAvailability::can_ron`] |
//! | 役の有無 | [`evaluate_tenpai_hand_value`] |
//! | threat に対する押し引き | [`decide_push_pull`] |
//!
//! 向聴・受け入れ・待ち・フリテン・役・点数をこの層で計算し直さない。
//!
//! # 同じ鳴きになる合法 action
//!
//! 同じ牌種の物理牌を複数持つ手牌では、消費する物理牌の組み合わせだけが違う Chi / Pon が複数の
//! 合法 action として並ぶ。鳴き後の判断が読むのは [`CallEvaluationKey`] が示す物理牌 semantics
//! だけなので、key が一致する候補は高コストな鳴き後の打牌評価を1回だけ行い、結果を各 action へ
//! 配る。候補の件数・順序・`action` と、同値時に先頭の合法 action を採る tie-break は変えない。
//!
//! # 鳴き後の打牌
//!
//! 鳴いた直後に切れない牌 (喰い替え) は戦術ではなく合法手の制約なので、鳴き後の仮想合法
//! `Dahai` から先に取り除き、残った候補を通常打牌の production selector へ渡す。したがって
//! 「喰い替え禁止牌を切ればテンパイする」を理由に鳴くことはなく、鳴き専用の比較順も持たない。
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
//! AND 鳴き後の最良打牌を既存 Push/Pull policy が Push と判定する
//! ```
//!
//! 即テンパイ候補は従来どおり最優先する。それが無い場合だけ Call 後1向聴の ExpectedSelfTsumoValue
//! と Pass を同じ流局 horizon で比較し、Call が厳密に高い場合だけ鳴く。同値・unknown は鳴かない。
//! 他家にリーチ者がいる局面の鳴きは押し引きへ通さず、打点による例外も持たない。
//!
//! 比較する2つの値は同じ1向聴 continuation の設定で求める。Call 側は鳴いた後の打牌候補比較
//! ([`select_discard_action_with_evaluation`] / [`select_best_iishanten_post_call_discard`]) が、
//! Pass 側は継続評価が、どちらも [`with_production_iishanten_continuation`] と同じ production の
//! 深度・探索内 memo を通る。片側だけ深い評価にして、比較が尺度の違いを拾うことがないように
//! する。
//!
//! # 片和了
//!
//! 役の有無は牌種単位ではなく、和了牌の物理牌 (赤5 / 黒5) ごとの variant 単位で見る。残枚数が
//! 0 の variant は現在ロンできないので判定対象にせず、残枚数 > 0 の variant に1つでも役なしが
//! あれば鳴かない。役の有無を確定できない variant がある場合も、役ありだと推測せず鳴かない。
//!
//! # 1向聴のまま鳴く候補
//!
//! ```text
//! 現在1向聴 → 鳴く → 最良打牌 → 1向聴のまま
//! ```
//!
//! は [`ForwardMetrics::expected_self_tsumo_value`](bot_logic::ForwardMetrics) と同じ Progress /
//! SameShanten 探索・terminal scoring・確率模型で評価する。Pass は架空の現在打牌を作らず、既に
//! action が終わり次の自摸を待つ state 用の共有入口から同じ探索へ入る。
//!
//! raw acceptance と固定面子の役保証は [`CallIishantenAcceptanceDiagnostic`] に観測用として残すが、
//! policy は読まない。diagnostics の有無で action は変わらず、production が使った self-tsumo 値を
//! [`CallIishantenSelfTsumoDiagnostic`] へそのまま保持する。
//!
//! # 2向聴から1向聴になる鳴きの観測
//!
//! diagnostics では、現在2向聴から Chi / Pon 後の最良打牌で1向聴になる候補だけ、Call と Pass
//! の self-tsumo value を観測する。Call は既存の1向聴 post-call selector が返した値、Pass は
//! 次の自摸を待つ2向聴 state の Full 値を使う。Progress-only は2向聴を維持する枝を含まず、
//! Call 側の完全な1向聴 continuation と比較すると Call に有利なため、この比較には使わない。
//! Pass は対象候補が1件以上ある場合に1回だけ評価し、打牌候補ごとの Full 探索は行わない。
//!
//! この比較は observation-only で、candidate の `eligible` / `reason` と production の action
//! selection には接続しない。diagnostics を無効にした通常の `act()` では追加探索もしない。

use bot_logic::{
    DiscardEvaluation, FixedMeldCount, HandValueError, HandValueOutcome, Meld, MeldKind,
    OwnDiscards, TenpaiWaitAvailability, TileCounts, TileId, TileType,
    awaiting_draw_expected_self_tsumo_value, awaiting_draw_two_shanten_expected_self_tsumo_value,
    best_discard_selection_index, calculate_acceptance_with_fixed_melds_and_visible_tiles,
    calculate_shanten_with_fixed_melds, discard_tenpai_wait_availability,
    evaluate_tenpai_hand_value, fixed_melds_guarantee_yaku, split_discarded_tile,
    tenpai_completed_hands,
};

use crate::action::LegalAction;
use crate::context::GameContext;
use crate::damaten_value::damaten_baseline_context;
use crate::decision_timing::{CallCandidateTimer, CallDecisionTimer};
use crate::discard_selection::{
    DiscardActionSelection, LookaheadDiagnosticScope, lookahead_inputs_with_own_future_draws,
    post_call_discard_evaluations, select_best_iishanten_post_call_discard,
    select_discard_action_with_evaluation, with_production_iishanten_continuation,
};
use crate::kuikae::forbidden_discards_after_call;
use crate::prospective_value::ProductionProspectiveValuator;
use crate::push_pull::{
    PushPullDecision, PushPullMode, decide_push_pull, push_pull_inputs_from_selected_tenpai,
};

/// 鳴きを検討する現在の向聴数。
pub const CALL_CURRENT_SHANTEN: i8 = 1;

/// 鳴き後の打牌でテンパイと判断する向聴数。
pub const CALL_TENPAI_SHANTEN: i8 = 0;

/// 鳴くために必要な、鳴き後テンパイの生きた待ちの残枚数合計 [枚]。inclusive。
pub const CALL_MIN_LIVE_WAIT_REMAINING: u8 = 3;

// Chi / Pon の consumed 枚数。
const CALL_CONSUMED_TILE_COUNT: usize = 2;

// observation-only の2向聴 Call / Pass 比較が対象にする現在の向聴数。
const CALL_TWO_SHANTEN_OBSERVATION_SHANTEN: i8 = 2;

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
/// `EligibleTenpai` / `EligibleIishantenSelfTsumo` 以外はすべて「今回は鳴かない」理由であり、
/// 最初に落ちた条件を1つだけ表す。判定順は [`CallCandidateDiagnostic`] のフィールドが埋まる順と
/// 一致する。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CallDecisionReason {
    /// 全条件を満たし、鳴き後に生きた待ちのテンパイになる。
    EligibleTenpai,
    /// 鳴き後も1向聴だが、同じ horizon の ExpectedSelfTsumoValue が Pass より厳密に高い。
    EligibleIishantenSelfTsumo,
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
    /// 鳴き後1向聴と Pass の ExpectedSelfTsumoValue のどちらかを確定できない。
    IishantenSelfTsumoUnknown,
    /// Pass の正確な horizon に必要な reaction 元 player が不明。
    ReactionSourceUnknown,
    /// Pass の ExpectedSelfTsumoValue が Call 以上。同値でも鳴かない。
    PassSelfTsumoNotLower,
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
    /// 鳴き後の最良打牌を既存 Push/Pull policy が Push と判定しない。
    PostCallNotPush,
    /// 鳴き後の仮想局面を既存の通常打牌・Push/Pull policy へ渡せない。
    PostCallEvaluationUnavailable,
}

/// `Call -> 打牌 -> 1向聴` と Pass の self-tsumo continuation 比較結果。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CallIishantenComparison {
    CallHigher,
    PassNotLower,
    Unknown,
}

/// production が使用した1向聴 Call / Pass の ExpectedSelfTsumoValue。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CallIishantenSelfTsumoDiagnostic {
    pub reaction_source_player: Option<u8>,
    pub pass_expected_self_tsumo_value: Option<u64>,
    pub call_expected_self_tsumo_value: Option<u64>,
    pub comparison: CallIishantenComparison,
}

/// 2向聴 Pass 側で使った既存 self-tsumo 評価。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CallTwoShantenPassEvaluation {
    /// Progress と、一度だけの SameShanten → Progress を含む Full 値。
    Full,
}

/// observation-only の `現在2向聴 → Call → 打牌 → 1向聴` と Pass の比較。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CallTwoShantenSelfTsumoDiagnostic {
    pub reaction_source_player: Option<u8>,
    pub pass_evaluation: CallTwoShantenPassEvaluation,
    pub pass_expected_self_tsumo_value: Option<u64>,
    pub call_expected_self_tsumo_value: Option<u64>,
    pub comparison: CallIishantenComparison,
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
/// `None` のままにする。production が使う値も observation-only の値も既存 selector / helper の
/// 結果そのもので、診断専用の向聴・受け入れ・待ち・役・点数計算は持たない。
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
    /// production または2向聴 observation の打牌選択が実際に除外に使った値そのもので、
    /// 診断表示のために求め直さない。
    pub post_call_forbidden_discards: Option<Vec<TileType>>,
    /// 喰い替え禁止牌を除いた合法な打牌候補の中の最良打牌評価。
    pub post_call_discard: Option<DiscardEvaluation>,
    /// 鳴き後の打牌でテンパイになる場合の待ちとロン可否。
    pub post_call_wait: Option<TenpaiWaitAvailability>,
    /// 鳴き後テンパイの和了牌の物理牌ごとの役診断。役を評価しなかった場合は `None`。
    pub post_call_wait_yaku: Option<Vec<CallWaitYakuDiagnostic>>,
    /// 既存 Push/Pull policy による鳴き後の最良打牌の判定。そこまで評価しなかった場合は `None`。
    pub post_call_push_pull: Option<PushPullDecision>,
    /// 鳴いても1向聴のままの候補についてだけ求める観測用の受け入れ比較。対象外の候補と、
    /// そこまで評価が進まなかった候補では `None`。
    pub iishanten_acceptance: Option<CallIishantenAcceptanceDiagnostic>,
    /// 鳴き後も1向聴の候補に対して production が実際に使った Call / Pass 比較。
    pub iishanten_self_tsumo: Option<CallIishantenSelfTsumoDiagnostic>,
    /// 現在2向聴から鳴き後の最良打牌で1向聴になる候補の Call / Pass 観測値。
    /// production の鳴き判断には使わない。
    pub two_shanten_self_tsumo: Option<CallTwoShantenSelfTsumoDiagnostic>,
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
// `collect_observations` は解析専用の受け入れ比較と2向聴 Call / Pass 比較を集めるかどうかだけを
// 切り替える。判断に使う fact の評価と候補の選択は切り替えの影響を受けない。
pub(crate) fn evaluate_call_decision(
    ctx: &GameContext,
    legal_actions: &[LegalAction],
    collect_observations: bool,
    timing: &mut CallDecisionTimer,
) -> Option<CallDecisionDiagnostic> {
    let mut candidates: Vec<CallCandidateDiagnostic> = Vec::new();
    // 既に評価した semantic key と、その結果を持つ candidate の index。合法な Chi / Pon は
    // 1局面あたり数件なので線形探索で足りる。
    let mut evaluated: Vec<(CallEvaluationKey, usize)> = Vec::new();
    for action in legal_actions {
        let Some((kind, tile, consumed)) = normalize_call(action) else {
            continue;
        };

        let key = call_meld_and_concealed_tiles(ctx.hand_tiles(), kind, tile, consumed)
            .map(|(meld, post_call_tiles)| CallEvaluationKey::new(&meld, &post_call_tiles));
        if let Some(key) = key.as_ref()
            && let Some(&(_, source)) = evaluated.iter().find(|(known, _)| known == key)
        {
            // 同じ post-call state を作る候補なので、評価結果をそのまま複製して action だけ
            // 元の合法 action に戻す。高コスト評価は行わない。
            let mut candidate = candidates[source].clone();
            candidate.action = action.clone();
            candidates.push(candidate);
            timing.record_reused_candidate(kind, tile, consumed);
            continue;
        }

        let mut candidate_timing = timing.candidate_timer();
        let candidate = evaluate_call_candidate(
            ctx,
            action,
            kind,
            tile,
            consumed,
            collect_observations,
            &mut candidate_timing,
        );
        timing.record_candidate(kind, tile, consumed, candidate_timing.finish());
        if let Some(key) = key {
            evaluated.push((key, candidates.len()));
        }
        candidates.push(candidate);
    }

    if candidates.is_empty() {
        return None;
    }

    apply_iishanten_self_tsumo_policy(ctx, &mut candidates, timing);
    apply_two_shanten_self_tsumo_observation(ctx, &mut candidates);

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

// 評価に効く物理牌の属性だけを取り出した表現。
//
// `TileId` は同じ牌種の4枚を別 ID で持つが、向聴・受け入れ・喰い替え・打点のどれも
// `TileId::tile_type()` と `TileId::is_red()` しか読まない (`TileId::copy_index()` は評価経路の
// どこにも現れない)。したがって牌種と赤5かどうかが一致する物理牌は評価上は交換可能で、赤5と
// 黒5は別物として残る。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct PhysicalTile {
    tile_type: TileType,
    red: bool,
}

impl PhysicalTile {
    fn new(tile: TileId) -> Self {
        Self {
            tile_type: tile.tile_type(),
            red: tile.is_red(),
        }
    }

    fn sequence(tiles: &[TileId]) -> Vec<Self> {
        tiles.iter().copied().map(Self::new).collect()
    }
}

/// 鳴き候補1件の評価入力を physical tile semantics へ落とした key。
///
/// `evaluate_call_conditions()` は `call_meld_and_concealed_tiles()` を通した後、判断に使う入力
/// として `GameContext` (全候補で共通) と `meld` / `post_call_tiles` しか読まない。鳴いた牌と
/// consumed もこの2つを組み立てるためだけに使う。したがってこの2つが物理牌 semantics まで一致
/// すれば、
///
/// ```text
/// 鳴き後の concealed hand / Meld / 喰い替え禁止牌 / 鳴き後の副露一覧 / 鳴き後の GameContext
/// / 鳴き後の合法 Dahai / 本番の打牌評価 / 候補の判断結果
/// ```
///
/// はすべて同じになる。
///
/// - 喰い替え禁止牌は [`forbidden_discards_after_call`] が `meld` の種別と牌種だけから決める
/// - 鳴き後の合法 Dahai は concealed hand の物理牌から禁止牌種を除いたもの
/// - 打牌評価と打点は牌種と赤5かどうかだけを読む ([`PhysicalTile`])
///
/// 並び順も含めて比較するため、手牌の並びが違えば別候補として個別に評価する。表示上の
/// `tile` / `consumed` が同じでも、赤5 / 黒5が違えば `red` で別 key になる。
#[derive(Debug, Clone, PartialEq, Eq)]
struct CallEvaluationKey {
    meld_kind: MeldKind,
    meld_called_tile: Option<PhysicalTile>,
    meld_tiles: Vec<PhysicalTile>,
    post_call_concealed: Vec<PhysicalTile>,
}

impl CallEvaluationKey {
    fn new(meld: &Meld, post_call_tiles: &[TileId]) -> Self {
        Self {
            meld_kind: meld.kind(),
            meld_called_tile: meld.called_tile().map(PhysicalTile::new),
            meld_tiles: PhysicalTile::sequence(meld.tiles()),
            post_call_concealed: PhysicalTile::sequence(post_call_tiles),
        }
    }
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
        .filter(|(_, candidate)| candidate.reason == CallDecisionReason::EligibleTenpai)
        .filter_map(|(index, candidate)| {
            candidate
                .post_call_discard
                .clone()
                .map(|evaluation| (index, evaluation))
        })
        .unzip();

    if let Some(best) = best_discard_selection_index(&evaluations, &[]) {
        return Some(indices[best]);
    }

    // 既存の即テンパイ候補が無い場合だけ、Pass より厳密に高い1向聴 Call の最大値を選ぶ。
    // 同値の Call 候補では合法 action の先頭を維持する。
    let mut best: Option<(usize, u64)> = None;
    for (index, candidate) in candidates.iter().enumerate() {
        if candidate.reason != CallDecisionReason::EligibleIishantenSelfTsumo {
            continue;
        }
        let Some(value) = candidate
            .iishanten_self_tsumo
            .and_then(|diagnostic| diagnostic.call_expected_self_tsumo_value)
        else {
            continue;
        };
        if best.is_none_or(|(_, best_value)| value > best_value) {
            best = Some((index, value));
        }
    }
    best.map(|(index, _)| index)
}

fn evaluate_call_candidate(
    ctx: &GameContext,
    action: &LegalAction,
    kind: CallKind,
    tile: TileId,
    consumed: &[TileId],
    collect_observations: bool,
    timing: &mut CallCandidateTimer,
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
        post_call_push_pull: None,
        iishanten_acceptance: None,
        iishanten_self_tsumo: None,
        two_shanten_self_tsumo: None,
        eligible: false,
        selected: false,
        reason: CallDecisionReason::EligibleTenpai,
    };

    let reason = evaluate_call_conditions(
        ctx,
        kind,
        tile,
        consumed,
        collect_observations,
        &mut candidate,
        timing,
    );
    candidate.eligible = reason == CallDecisionReason::EligibleTenpai;
    candidate.reason = reason;
    candidate
}

// 鳴き成立条件を順に評価し、最初に落ちた条件を理由として返す。評価が進んだ範囲の値だけを
// candidate へ書き込み、評価しなかった項目は None のままにする。
//
// 判断に使う fact は `collect_observations` にかかわらず常に同じ順序で評価する。この flag が
// 切り替えるのは、判断に使わない観測値を足すかどうかだけ。
fn evaluate_call_conditions(
    ctx: &GameContext,
    kind: CallKind,
    tile: TileId,
    consumed: &[TileId],
    collect_observations: bool,
    candidate: &mut CallCandidateDiagnostic,
    timing: &mut CallCandidateTimer,
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
        if collect_observations && current_shanten == CALL_TWO_SHANTEN_OBSERVATION_SHANTEN {
            observe_two_shanten_call_to_iishanten(
                ctx,
                &meld,
                &post_call_tiles,
                post_call_fixed_meld_count,
                candidate,
            );
        }
        return CallDecisionReason::CurrentShantenNotOne;
    }

    // 喰い替え禁止牌は鳴き直後だけの合法手制約なので、仮想 legal actions から先に除く。
    // 残った合法 Dahai は実際の通常打牌と同じ production selector へ渡す。
    let forbidden_discards = forbidden_discards_after_call(&meld);
    let legal_actions = post_call_legal_dahai_actions(&post_call_tiles, &forbidden_discards);
    candidate.post_call_forbidden_discards = Some(forbidden_discards);
    let mut post_call_melds = ctx.own_melds().unwrap_or_default().to_vec();
    post_call_melds.push(meld.clone());
    let Some(post_call_context) = ctx.with_own_hand_state(post_call_tiles.clone(), post_call_melds)
    else {
        return CallDecisionReason::PostCallEvaluationUnavailable;
    };
    let selection = timing.measure_post_call_discard_selection(|| {
        select_discard_action_with_evaluation(&post_call_context, &legal_actions)
    });
    let Some(evaluation) = selection.evaluation.as_ref() else {
        return CallDecisionReason::NoPostCallDiscard;
    };

    if evaluation.min_shanten_after_discard() != CALL_TENPAI_SHANTEN {
        if evaluation.min_shanten_after_discard() == CALL_CURRENT_SHANTEN {
            candidate.iishanten_self_tsumo = Some(CallIishantenSelfTsumoDiagnostic {
                reaction_source_player: ctx.reaction_source_player(),
                pass_expected_self_tsumo_value: None,
                call_expected_self_tsumo_value: selection
                    .iishanten_forward_metrics
                    .and_then(|metrics| metrics.expected_self_tsumo_value),
                comparison: CallIishantenComparison::Unknown,
            });
        }

        // production が選んだ打牌評価を診断にもそのまま載せる。
        if collect_observations {
            candidate.iishanten_acceptance = iishanten_acceptance_diagnostic(
                ctx,
                &counts,
                current_fixed_meld_count,
                &meld,
                evaluation,
            );
        }
        candidate.post_call_discard = Some(evaluation.clone());
        return CallDecisionReason::PostCallNotTenpai;
    }

    let Some(wait) = selection.tenpai_wait.clone().or_else(|| {
        discard_tenpai_wait_availability(
            &TileCounts::from_tiles(post_call_tiles.iter().copied()),
            post_call_fixed_meld_count,
            evaluation,
            &OwnDiscards::from_optional_river(ctx.own_discards()),
            ctx.history_furiten_after_own_discard(),
        )
    }) else {
        candidate.post_call_discard = Some(evaluation.clone());
        return CallDecisionReason::PostCallNotTenpai;
    };

    let reason =
        evaluate_post_call_conditions(ctx, &meld, &post_call_tiles, evaluation, &wait, candidate);
    let reason = if reason == CallDecisionReason::EligibleTenpai {
        let decision =
            post_call_push_pull_decision(&post_call_context, &selection, &wait, &legal_actions);
        candidate.post_call_push_pull = Some(decision);
        if decision.mode == PushPullMode::Push {
            CallDecisionReason::EligibleTenpai
        } else {
            CallDecisionReason::PostCallNotPush
        }
    } else {
        reason
    };
    candidate.post_call_discard = Some(evaluation.clone());
    candidate.post_call_wait = Some(wait);
    reason
}

// 鳴き後に切れる全物理牌を、喰い替え禁止牌だけ除いて仮想 legal actions にする。牌種の比較と
// 同牌種内の赤黒 preference は通常打牌 selector に委ねる。
fn post_call_legal_dahai_actions(
    post_call_tiles: &[TileId],
    forbidden_discards: &[TileType],
) -> Vec<LegalAction> {
    post_call_tiles
        .iter()
        .copied()
        .filter(|tile| !forbidden_discards.contains(&tile.tile_type()))
        .map(|tile| LegalAction::Dahai { tile })
        .collect()
}

// production selector が選んだ evaluation / wait / offense と同じ仮想 legal actions を既存
// Push/Pull 入力へ接続する。threat classification と threshold は push_pull 側に委ねる。
fn post_call_push_pull_decision(
    post_call_context: &GameContext,
    selection: &DiscardActionSelection,
    wait: &TenpaiWaitAvailability,
    legal_actions: &[LegalAction],
) -> PushPullDecision {
    let evaluation = selection
        .evaluation
        .as_ref()
        .expect("production selector が選んだ評価を渡す");
    let inputs = push_pull_inputs_from_selected_tenpai(
        post_call_context,
        evaluation,
        wait,
        selection.tenpai_offense_value,
        legal_actions,
    );
    decide_push_pull(&inputs)
}

// 現在2向聴から Chi / Pon 後の最良打牌で1向聴になる候補だけを、既存の post-call
// selector へ通す。候補生成・喰い替え・向聴・acceptance・打牌比較・Call 側 value は
// すべて既存経路の結果をそのまま使う。
fn observe_two_shanten_call_to_iishanten(
    ctx: &GameContext,
    meld: &Meld,
    post_call_tiles: &[TileId],
    post_call_fixed_meld_count: FixedMeldCount,
    candidate: &mut CallCandidateDiagnostic,
) {
    let forbidden_discards = forbidden_discards_after_call(meld);
    let evaluations = post_call_discard_evaluations(
        ctx,
        post_call_tiles,
        post_call_fixed_meld_count,
        &forbidden_discards,
    );
    candidate.post_call_forbidden_discards = Some(forbidden_discards);

    let mut melds: Vec<Meld> = ctx.own_melds().unwrap_or_default().to_vec();
    melds.push(meld.clone());
    let Some((evaluation, call_value)) =
        select_best_iishanten_post_call_discard(ctx, post_call_tiles, &melds, &evaluations)
    else {
        return;
    };
    let post_call_shanten = evaluation.min_shanten_after_discard();
    candidate.post_call_discard = Some(evaluation);
    if post_call_shanten != CALL_CURRENT_SHANTEN {
        return;
    }

    candidate.two_shanten_self_tsumo = Some(CallTwoShantenSelfTsumoDiagnostic {
        reaction_source_player: ctx.reaction_source_player(),
        pass_evaluation: CallTwoShantenPassEvaluation::Full,
        pass_expected_self_tsumo_value: None,
        call_expected_self_tsumo_value: call_value,
        comparison: CallIishantenComparison::Unknown,
    });
}

// 1向聴のままの Call 候補がある場合だけ Pass を1回評価し、全候補へ同じ値を配る。
fn apply_iishanten_self_tsumo_policy(
    ctx: &GameContext,
    candidates: &mut [CallCandidateDiagnostic],
    timing: &mut CallDecisionTimer,
) {
    if !candidates
        .iter()
        .any(|candidate| candidate.iishanten_self_tsumo.is_some())
    {
        return;
    }

    let reaction_source_known = reaction_draw_distance(ctx).is_some();
    let pass_value = reaction_source_known
        .then(|| {
            timing
                .measure_pass_iishanten_self_tsumo(|| pass_iishanten_expected_self_tsumo_value(ctx))
        })
        .flatten();

    for candidate in candidates {
        let Some(mut diagnostic) = candidate.iishanten_self_tsumo else {
            continue;
        };
        diagnostic.pass_expected_self_tsumo_value = pass_value;
        let (comparison, reason) = compare_call_pass_self_tsumo_values(
            reaction_source_known,
            diagnostic.call_expected_self_tsumo_value,
            diagnostic.pass_expected_self_tsumo_value,
        );
        diagnostic.comparison = comparison;
        candidate.iishanten_self_tsumo = Some(diagnostic);
        candidate.eligible = reason == CallDecisionReason::EligibleIishantenSelfTsumo;
        candidate.reason = reason;
    }
}

// 観測対象がある場合だけ Pass の2向聴 Full 値を1回求め、全候補へ共有する。比較結果は
// candidate の production reason / eligible へ接続しない。
fn apply_two_shanten_self_tsumo_observation(
    ctx: &GameContext,
    candidates: &mut [CallCandidateDiagnostic],
) {
    if !candidates
        .iter()
        .any(|candidate| candidate.two_shanten_self_tsumo.is_some())
    {
        return;
    }

    let reaction_source_known = reaction_draw_distance(ctx).is_some();
    let pass_value = reaction_source_known
        .then(|| pass_two_shanten_expected_self_tsumo_value(ctx))
        .flatten();

    for candidate in candidates {
        let Some(mut diagnostic) = candidate.two_shanten_self_tsumo else {
            continue;
        };
        diagnostic.pass_expected_self_tsumo_value = pass_value;
        diagnostic.comparison = compare_call_pass_self_tsumo_values(
            reaction_source_known,
            diagnostic.call_expected_self_tsumo_value,
            diagnostic.pass_expected_self_tsumo_value,
        )
        .0;
        candidate.two_shanten_self_tsumo = Some(diagnostic);
    }
}

fn compare_call_pass_self_tsumo_values(
    reaction_source_known: bool,
    call: Option<u64>,
    pass: Option<u64>,
) -> (CallIishantenComparison, CallDecisionReason) {
    if !reaction_source_known {
        return (
            CallIishantenComparison::Unknown,
            CallDecisionReason::ReactionSourceUnknown,
        );
    }
    match (call, pass) {
        (Some(call), Some(pass)) if call > pass => (
            CallIishantenComparison::CallHigher,
            CallDecisionReason::EligibleIishantenSelfTsumo,
        ),
        (Some(_), Some(_)) => (
            CallIishantenComparison::PassNotLower,
            CallDecisionReason::PassSelfTsumoNotLower,
        ),
        _ => (
            CallIishantenComparison::Unknown,
            CallDecisionReason::IishantenSelfTsumoUnknown,
        ),
    }
}

// reaction 元の次巡から自分の次の自摸までに山から引かれる枚数。観測できない席や自家打牌は
// reaction として不整合なので None のままにする。
fn reaction_draw_distance(ctx: &GameContext) -> Option<u32> {
    let own = ctx.player_id()?;
    let source = ctx.reaction_source_player()?;
    if own >= 4 || source >= 4 || own == source {
        return None;
    }
    Some(u32::from((own + 4 - source) % 4))
}

// Pass 後から流局までの自分の自摸回数。source の次席から順に残り山を配るため、通常打牌後や
// Call 後の floor(remaining / 4) とは最初の自摸位置だけが異なる。
fn pass_own_future_draws(ctx: &GameContext) -> Option<u32> {
    let remaining = ctx.remaining_tiles()?;
    let distance = reaction_draw_distance(ctx)?;
    if remaining < distance {
        Some(0)
    } else {
        Some(1 + (remaining - distance) / 4)
    }
}

// 架空の現在打牌を作らず、現在の13枚を「action 済みで次の自摸を待つ state」として既存
// lookahead へ渡す。
fn pass_expected_self_tsumo_value(
    ctx: &GameContext,
    expected_shanten: i8,
    evaluate: impl FnOnce(
        &bot_logic::LookaheadInputs<'_>,
        &bot_logic::EffectiveAcceptance,
    ) -> Option<u64>,
) -> Option<u64> {
    let fixed_meld_count = ctx.own_fixed_meld_count()?;
    let counts = TileCounts::from_tiles(ctx.hand_tiles().iter().copied());
    let acceptance = calculate_acceptance_with_fixed_melds_and_visible_tiles(
        &counts,
        fixed_meld_count,
        ctx.visible_tiles(),
    );
    if acceptance.current_min_shanten() != expected_shanten {
        return None;
    }

    let valuator = ProductionProspectiveValuator::new_with_hand_state(ctx, ctx.own_melds());
    // Call 側は鳴いた後の production 打牌選択が求めるので、Pass 側も同じ1向聴 continuation の
    // 設定で評価する。片側だけ深度が違うと、比較そのものが尺度の違いを拾ってしまう。
    let inputs = with_production_iishanten_continuation(lookahead_inputs_with_own_future_draws(
        ctx,
        ctx.hand_tiles(),
        &valuator,
        LookaheadDiagnosticScope::None,
        Some(pass_own_future_draws(ctx)?),
    ));
    evaluate(&inputs, &acceptance)
}

fn pass_iishanten_expected_self_tsumo_value(ctx: &GameContext) -> Option<u64> {
    pass_expected_self_tsumo_value(
        ctx,
        CALL_CURRENT_SHANTEN,
        awaiting_draw_expected_self_tsumo_value,
    )
}

fn pass_two_shanten_expected_self_tsumo_value(ctx: &GameContext) -> Option<u64> {
    pass_expected_self_tsumo_value(
        ctx,
        CALL_TWO_SHANTEN_OBSERVATION_SHANTEN,
        awaiting_draw_two_shanten_expected_self_tsumo_value,
    )
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

    use std::time::Duration;

    use bot_logic::MeldShape;

    use crate::decision_timing::{CallCandidateDuration, CallDecisionDurations};

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

    fn valued_reaction_context(
        hand: &[u8],
        target: u8,
        source: u8,
        remaining_tiles: u32,
    ) -> GameContext {
        reaction_context(hand, target)
            .with_reaction_source_player(Some(source))
            .with_table_state_facts(crate::context::TableStateFacts {
                remaining_tiles: Some(remaining_tiles),
                ..Default::default()
            })
    }

    // 既存2副露を持つ小さい局面。残る concealed hand を小さくして、2向聴 Full の focused
    // test が不要に大きな探索にならないようにする。
    fn valued_two_shanten_reaction_context(
        hand: &[u8],
        target: u8,
        source: Option<u8>,
        remaining_tiles: Option<u32>,
    ) -> GameContext {
        let hand_tiles = tiles(hand);
        let melds = vec![
            Meld::new(MeldKind::Pon, tiles(&[124, 125, 126]), Some(tile(124))),
            Meld::new(MeldKind::Pon, tiles(&[128, 129, 130]), Some(tile(128))),
        ];
        let mut visible = hand_tiles.clone();
        visible.push(tile(target));
        visible.extend(melds.iter().flat_map(|meld| meld.tiles().iter().copied()));

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
            [melds, vec![], vec![], vec![]],
        )
        .with_history_furiten_facts(bot_logic::HistoryFuritenFacts {
            same_turn: Some(false),
            riichi_missed_win: Some(false),
        })
        .with_reaction_source_player(source)
        .with_table_state_facts(crate::context::TableStateFacts {
            remaining_tiles,
            ..Default::default()
        })
    }

    #[test]
    fn an_iishanten_call_with_a_higher_expected_self_tsumo_value_is_selected() {
        // 門前のまま進めた方が手変わりの経路を多く持つため、Call が上回るのは残り自摸機会が
        // 少ない局面。1向聴 continuation の深度はどちらの側も production のものを使う。
        let action = pon_action(IISHANTEN_PON_TARGET, &IISHANTEN_PON_CONSUMED);
        let ctx = valued_reaction_context(&IISHANTEN_PON_HAND, IISHANTEN_PON_TARGET, 1, 12);
        let (decision, candidate) = single_candidate(&ctx, &action, true);
        let comparison = candidate.iishanten_self_tsumo.expect("comparison");

        assert_eq!(
            candidate.reason,
            CallDecisionReason::EligibleIishantenSelfTsumo
        );
        assert!(candidate.eligible);
        assert_eq!(decision.selected, Some(action));
        assert_eq!(comparison.comparison, CallIishantenComparison::CallHigher);
        assert_eq!(comparison.pass_expected_self_tsumo_value, Some(20_956_462));
        assert_eq!(comparison.call_expected_self_tsumo_value, Some(21_531_497));
        assert!(
            comparison.call_expected_self_tsumo_value > comparison.pass_expected_self_tsumo_value
        );
        // forward-selected discard is legal after the call; the forbidden called tile is excluded.
        assert!(
            !candidate
                .post_call_forbidden_discards
                .as_ref()
                .unwrap()
                .contains(&candidate.post_call_discard.as_ref().unwrap().discard)
        );
        assert_eq!(candidate.post_call_fixed_meld_count, FixedMeldCount::new(1));

        let meld = Meld::new(
            MeldKind::Pon,
            tiles(&[
                IISHANTEN_PON_TARGET,
                IISHANTEN_PON_CONSUMED[0],
                IISHANTEN_PON_CONSUMED[1],
            ]),
            Some(tile(IISHANTEN_PON_TARGET)),
        );
        let melds = [meld];
        let post_call = ProductionProspectiveValuator::new_with_hand_state(&ctx, Some(&melds));
        assert_eq!(
            post_call.fixed_meld_count(),
            FixedMeldCount::new(1).unwrap()
        );
        assert!(!post_call.reach_legal());
    }

    #[test]
    fn an_iishanten_pass_with_a_higher_expected_self_tsumo_value_is_kept() {
        let action = pon_action(IISHANTEN_PON_TARGET, &IISHANTEN_PON_CONSUMED);
        let ctx = valued_reaction_context(&IISHANTEN_PON_HAND, IISHANTEN_PON_TARGET, 1, 63);
        let (decision, candidate) = single_candidate(&ctx, &action, true);
        let comparison = candidate.iishanten_self_tsumo.expect("comparison");

        assert_eq!(candidate.reason, CallDecisionReason::PassSelfTsumoNotLower);
        assert!(!candidate.eligible);
        assert_eq!(decision.selected, None);
        assert_eq!(comparison.comparison, CallIishantenComparison::PassNotLower);
        assert!(
            comparison.pass_expected_self_tsumo_value > comparison.call_expected_self_tsumo_value
        );
    }

    #[test]
    fn the_call_and_pass_values_both_use_the_production_iishanten_continuation_depth() {
        // Call 側は鳴いた後の production 打牌選択、Pass 側は同じ設定を適用した継続評価が求める。
        // どちらも production の手変わり深度で、片側だけ旧 shallow へ戻ると値が動く局面を使う
        // (手変わり1回までの旧設定では pass 176.885897 / call 165.908530 になる)。
        let action = pon_action(IISHANTEN_PON_TARGET, &IISHANTEN_PON_CONSUMED);
        let ctx = valued_reaction_context(&IISHANTEN_PON_HAND, IISHANTEN_PON_TARGET, 1, 63);
        let (_, candidate) = single_candidate(&ctx, &action, true);
        let comparison = candidate.iishanten_self_tsumo.expect("comparison");

        assert_eq!(
            comparison.pass_expected_self_tsumo_value,
            Some(284_875_812),
            "pass",
        );
        assert_eq!(
            comparison.call_expected_self_tsumo_value,
            Some(239_138_199),
            "call",
        );
    }

    #[test]
    fn equal_production_iishanten_values_keep_the_pass() {
        let action = pon_action(IISHANTEN_PON_TARGET, &IISHANTEN_PON_CONSUMED);
        let ctx = valued_reaction_context(&IISHANTEN_PON_HAND, IISHANTEN_PON_TARGET, 1, 0);
        let (decision, candidate) = single_candidate(&ctx, &action, false);
        let comparison = candidate.iishanten_self_tsumo.expect("comparison");

        assert_eq!(comparison.pass_expected_self_tsumo_value, Some(0));
        assert_eq!(comparison.call_expected_self_tsumo_value, Some(0));
        assert_eq!(comparison.comparison, CallIishantenComparison::PassNotLower);
        assert_eq!(candidate.reason, CallDecisionReason::PassSelfTsumoNotLower);
        assert_eq!(decision.selected, None);
    }

    #[test]
    fn an_unknown_production_iishanten_value_keeps_the_pass() {
        let action = pon_action(IISHANTEN_PON_TARGET, &IISHANTEN_PON_CONSUMED);
        let ctx = reaction_context(&IISHANTEN_PON_HAND, IISHANTEN_PON_TARGET)
            .with_reaction_source_player(Some(1));
        let (decision, candidate) = single_candidate(&ctx, &action, false);
        let comparison = candidate.iishanten_self_tsumo.expect("comparison");

        assert_eq!(comparison.pass_expected_self_tsumo_value, None);
        assert_eq!(comparison.call_expected_self_tsumo_value, None);
        assert_eq!(comparison.comparison, CallIishantenComparison::Unknown);
        assert_eq!(
            candidate.reason,
            CallDecisionReason::IishantenSelfTsumoUnknown
        );
        assert_eq!(decision.selected, None);
    }

    #[test]
    fn equal_and_unknown_iishanten_values_keep_the_pass() {
        assert_eq!(
            compare_call_pass_self_tsumo_values(true, Some(100), Some(100)),
            (
                CallIishantenComparison::PassNotLower,
                CallDecisionReason::PassSelfTsumoNotLower
            )
        );
        for values in [(None, Some(100)), (Some(100), None), (None, None)] {
            assert_eq!(
                compare_call_pass_self_tsumo_values(true, values.0, values.1),
                (
                    CallIishantenComparison::Unknown,
                    CallDecisionReason::IishantenSelfTsumoUnknown
                )
            );
        }
    }

    #[test]
    fn an_unknown_reaction_source_is_not_inferred() {
        assert_eq!(
            reaction_context(&IISHANTEN_PON_HAND, IISHANTEN_PON_TARGET).reaction_source_player(),
            None
        );
        assert_eq!(
            compare_call_pass_self_tsumo_values(false, Some(200), Some(100)),
            (
                CallIishantenComparison::Unknown,
                CallDecisionReason::ReactionSourceUnknown
            )
        );
    }

    #[test]
    fn pass_draw_count_uses_the_observed_source_position() {
        for (remaining, expected) in [(60, 15), (63, 16)] {
            let context =
                valued_reaction_context(&IISHANTEN_PON_HAND, IISHANTEN_PON_TARGET, 1, remaining);
            assert_eq!(pass_own_future_draws(&context), Some(expected));
        }
        assert_eq!(
            pass_own_future_draws(&reaction_context(&IISHANTEN_PON_HAND, IISHANTEN_PON_TARGET)),
            None
        );
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
        collect_observations: bool,
    ) -> (CallDecisionDiagnostic, CallCandidateDiagnostic) {
        let decision = evaluate_call_decision(
            ctx,
            &[action.clone(), LegalAction::None],
            collect_observations,
            &mut CallDecisionTimer::disabled(),
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

    // 既存2副露 + CC 55p E S W の2向聴。C を Pon した後、最良打牌で1向聴になる。
    const TWO_SHANTEN_CALL_PON_HAND: [u8; 7] = [132, 133, 52, 53, 108, 112, 116];
    const TWO_SHANTEN_CALL_PON_TARGET: u8 = 134;
    const TWO_SHANTEN_CALL_PON_CONSUMED: [u8; 2] = [132, 133];

    // 既存2副露 + 68m 55p E S W の2向聴。6m8m で7mを Chi した後、Pon と同じ
    // post-call concealed hand になり、共通の評価 path で1向聴になる。
    const TWO_SHANTEN_CALL_CHI_HAND: [u8; 7] = [20, 28, 52, 53, 108, 112, 116];
    const TWO_SHANTEN_CALL_CHI_TARGET: u8 = 24;
    const TWO_SHANTEN_CALL_CHI_CONSUMED: [u8; 2] = [20, 28];

    #[test]
    fn two_shanten_chi_and_pon_observe_the_same_call_pass_value_path() {
        let cases = [
            (
                CallKind::Pon,
                &TWO_SHANTEN_CALL_PON_HAND[..],
                pon_action(TWO_SHANTEN_CALL_PON_TARGET, &TWO_SHANTEN_CALL_PON_CONSUMED),
                TWO_SHANTEN_CALL_PON_TARGET,
                1,
            ),
            (
                CallKind::Chi,
                &TWO_SHANTEN_CALL_CHI_HAND[..],
                chi_action(TWO_SHANTEN_CALL_CHI_TARGET, &TWO_SHANTEN_CALL_CHI_CONSUMED),
                TWO_SHANTEN_CALL_CHI_TARGET,
                3,
            ),
        ];

        for (kind, hand, action, target, source) in cases {
            let ctx = valued_two_shanten_reaction_context(hand, target, Some(source), Some(32));
            let (decision, candidate) = single_candidate(&ctx, &action, true);
            let comparison = candidate
                .two_shanten_self_tsumo
                .expect("2向聴 Call / Pass 観測対象");

            assert_eq!(candidate.action, action);
            assert_eq!(candidate.kind, kind);
            assert_eq!(candidate.current_shanten, Some(2));
            assert_eq!(candidate.post_call_shanten(), Some(1));
            assert_eq!(candidate.reason, CallDecisionReason::CurrentShantenNotOne);
            assert!(!candidate.eligible);
            assert_eq!(decision.selected, None);
            assert_eq!(comparison.reaction_source_player, Some(source));
            assert_eq!(
                comparison.pass_evaluation,
                CallTwoShantenPassEvaluation::Full
            );
            assert!(comparison.pass_expected_self_tsumo_value.is_some());
            assert!(comparison.call_expected_self_tsumo_value.is_some());
            assert_eq!(comparison.comparison, CallIishantenComparison::CallHigher);

            // 喰い替え禁止牌を除いた既存候補だけが comparator に渡される。
            let forbidden = candidate
                .post_call_forbidden_discards
                .as_ref()
                .expect("喰い替え制約を評価済み");
            assert!(!forbidden.is_empty());
            assert!(!forbidden.contains(&candidate.post_call_discard.as_ref().unwrap().discard));
            let (_, called_tile, consumed) = normalize_call(&action).expect("Chi / Pon");
            let (_, post_call_tiles) =
                call_meld_and_concealed_tiles(ctx.hand_tiles(), kind, called_tile, consumed)
                    .expect("合法な鳴き");
            let evaluations = post_call_discard_evaluations(
                &ctx,
                &post_call_tiles,
                candidate.post_call_fixed_meld_count.unwrap(),
                forbidden,
            );
            assert!(
                evaluations
                    .iter()
                    .all(|evaluation| !forbidden.contains(&evaluation.discard))
            );
        }
    }

    #[test]
    fn two_shanten_call_pass_observation_keeps_all_three_comparison_outcomes() {
        for (call, pass, expected) in [
            (Some(101), Some(100), CallIishantenComparison::CallHigher),
            (Some(100), Some(100), CallIishantenComparison::PassNotLower),
            (None, Some(100), CallIishantenComparison::Unknown),
        ] {
            assert_eq!(
                compare_call_pass_self_tsumo_values(true, call, pass).0,
                expected
            );
        }

        let ctx = valued_two_shanten_reaction_context(
            &TWO_SHANTEN_CALL_PON_HAND,
            TWO_SHANTEN_CALL_PON_TARGET,
            None,
            Some(32),
        );
        let action = pon_action(TWO_SHANTEN_CALL_PON_TARGET, &TWO_SHANTEN_CALL_PON_CONSUMED);
        let (_, candidate) = single_candidate(&ctx, &action, true);
        let comparison = candidate.two_shanten_self_tsumo.expect("観測対象");
        assert!(comparison.call_expected_self_tsumo_value.is_some());
        assert_eq!(comparison.pass_expected_self_tsumo_value, None);
        assert_eq!(comparison.comparison, CallIishantenComparison::Unknown);

        let zero_draws = valued_two_shanten_reaction_context(
            &TWO_SHANTEN_CALL_PON_HAND,
            TWO_SHANTEN_CALL_PON_TARGET,
            Some(1),
            Some(0),
        );
        let (_, candidate) = single_candidate(&zero_draws, &action, true);
        let comparison = candidate.two_shanten_self_tsumo.expect("観測対象");
        assert_eq!(comparison.pass_expected_self_tsumo_value, Some(0));
        assert_eq!(comparison.call_expected_self_tsumo_value, Some(0));
        assert_eq!(comparison.comparison, CallIishantenComparison::PassNotLower);
    }

    #[test]
    fn the_two_shanten_post_call_iishanten_value_uses_the_production_continuation_depth() {
        // 2向聴からの鳴きを観測する経路も、鳴いた後の1向聴候補比較は production の手変わり深度を
        // 通る。手変わり1回までの旧設定では call 2239.229406 になる局面を使う。Pass 側は次の
        // 自摸を待つ2向聴 state の既存 Full evaluation そのままで、深度には依らない。
        let ctx = valued_two_shanten_reaction_context(
            &TWO_SHANTEN_CALL_PON_HAND,
            TWO_SHANTEN_CALL_PON_TARGET,
            Some(1),
            Some(32),
        );
        let action = pon_action(TWO_SHANTEN_CALL_PON_TARGET, &TWO_SHANTEN_CALL_PON_CONSUMED);
        let (_, candidate) = single_candidate(&ctx, &action, true);
        let comparison = candidate.two_shanten_self_tsumo.expect("観測対象");

        assert_eq!(
            comparison.call_expected_self_tsumo_value,
            Some(4_103_395_595),
            "call",
        );
        assert_eq!(
            comparison.pass_expected_self_tsumo_value,
            Some(189_935_840),
            "pass",
        );
    }

    #[test]
    fn two_shanten_call_pass_observation_does_not_change_the_selected_action() {
        let ctx = valued_two_shanten_reaction_context(
            &TWO_SHANTEN_CALL_PON_HAND,
            TWO_SHANTEN_CALL_PON_TARGET,
            Some(1),
            Some(32),
        );
        let action = pon_action(TWO_SHANTEN_CALL_PON_TARGET, &TWO_SHANTEN_CALL_PON_CONSUMED);

        let (production, production_candidate) = single_candidate(&ctx, &action, false);
        let (observed, observed_candidate) = single_candidate(&ctx, &action, true);
        assert_eq!(production.selected, observed.selected);
        assert_eq!(production_candidate.two_shanten_self_tsumo, None);
        assert!(observed_candidate.two_shanten_self_tsumo.is_some());
        assert_eq!(
            CallCandidateDiagnostic {
                post_call_forbidden_discards: None,
                post_call_discard: None,
                two_shanten_self_tsumo: None,
                ..observed_candidate
            },
            production_candidate
        );
    }

    #[test]
    fn an_iishanten_call_that_stays_iishanten_compares_the_pass_and_post_call_acceptance() {
        let ctx = reaction_context(&IISHANTEN_PON_HAND, IISHANTEN_PON_TARGET);
        let action = pon_action(IISHANTEN_PON_TARGET, &IISHANTEN_PON_CONSUMED);
        let (decision, candidate) = single_candidate(&ctx, &action, true);

        assert_eq!(candidate.reason, CallDecisionReason::ReactionSourceUnknown);
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

        assert_eq!(candidate.reason, CallDecisionReason::ReactionSourceUnknown);
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

    fn measured_call_decision(
        ctx: &GameContext,
        legal_actions: &[LegalAction],
        collect_observations: bool,
    ) -> (
        Option<CallDecisionDiagnostic>,
        CallDecisionDurations,
        Vec<CallCandidateDuration>,
    ) {
        let mut timing = CallDecisionTimer::armed();
        let decision =
            evaluate_call_decision(ctx, legal_actions, collect_observations, &mut timing);
        let (durations, candidates) = timing.finish();
        (decision, durations, candidates)
    }

    #[test]
    fn a_request_without_a_legal_call_measures_nothing() {
        let ctx = reaction_context(&IISHANTEN_PON_HAND, IISHANTEN_PON_TARGET);
        let legal_actions = [
            LegalAction::Dahai {
                tile: tile(IISHANTEN_PON_HAND[0]),
            },
            LegalAction::None,
        ];
        let (decision, durations, candidates) = measured_call_decision(&ctx, &legal_actions, false);

        assert_eq!(decision, None);
        assert_eq!(durations, CallDecisionDurations::default());
        assert!(candidates.is_empty());
    }

    #[test]
    fn every_call_candidate_is_measured_in_the_legal_action_order() {
        // 重複候補も行をまとめず、合法 action の順にそれぞれ1件ずつ並ぶ。semantic に同一な
        // 2件目は評価を再利用するので、実測は 0 で reused になる。
        let chi = chi_action(IISHANTEN_CHI_TARGET, &IISHANTEN_CHI_CONSUMED);
        let ctx = valued_reaction_context(&IISHANTEN_CHI_HAND, IISHANTEN_CHI_TARGET, 1, 12);
        let legal_actions = [chi.clone(), chi.clone(), LegalAction::None];
        let (decision, durations, candidates) = measured_call_decision(&ctx, &legal_actions, false);

        assert_eq!(decision.expect("evaluated").candidates.len(), 2);
        assert_eq!(candidates.len(), 2);
        for candidate in &candidates {
            assert_eq!(candidate.kind, CallKind::Chi);
            assert_eq!(candidate.tile, tile(IISHANTEN_CHI_TARGET));
            assert_eq!(candidate.consumed, tiles(&IISHANTEN_CHI_CONSUMED));
        }
        assert!(!candidates[0].reused);
        assert!(candidates[0].elapsed > Duration::ZERO);
        assert!(candidates[0].post_call_discard_selection > Duration::ZERO);
        assert!(candidates[0].post_call_discard_selection <= candidates[0].elapsed);
        assert!(candidates[1].reused);
        assert_eq!(candidates[1].elapsed, Duration::ZERO);
        assert_eq!(candidates[1].post_call_discard_selection, Duration::ZERO);
        assert_eq!(
            durations.candidates,
            candidates
                .iter()
                .map(|candidate| candidate.elapsed)
                .sum::<Duration>()
        );
        assert!(durations.total >= durations.candidates);
    }

    #[test]
    fn the_shared_iishanten_pass_comparison_is_measured_once() {
        let action = pon_action(IISHANTEN_PON_TARGET, &IISHANTEN_PON_CONSUMED);
        let ctx = valued_reaction_context(&IISHANTEN_PON_HAND, IISHANTEN_PON_TARGET, 1, 12);
        let legal_actions = [action, LegalAction::None];
        let (decision, durations, candidates) = measured_call_decision(&ctx, &legal_actions, false);
        let candidate = &decision.expect("evaluated").candidates[0];

        assert!(candidate.iishanten_self_tsumo.is_some());
        assert!(durations.pass_iishanten_self_tsumo > Duration::ZERO);
        assert!(durations.total >= durations.candidates + durations.pass_iishanten_self_tsumo);
        assert_eq!(candidates.len(), 1);
    }

    #[test]
    fn a_call_without_the_iishanten_comparison_does_not_measure_the_pass() {
        // 即テンパイ候補は Call / Pass 比較へ入らないので、Pass の計測も 0 のままになる。
        let ctx = reaction_context(&TENPAI_PON_HAND, TENPAI_PON_TARGET);
        let action = pon_action(TENPAI_PON_TARGET, &TENPAI_PON_CONSUMED);
        let legal_actions = [action.clone(), LegalAction::None];
        let (decision, durations, candidates) = measured_call_decision(&ctx, &legal_actions, false);
        let decision = decision.expect("evaluated");

        assert_eq!(decision.selected, Some(action));
        assert_eq!(decision.candidates[0].iishanten_self_tsumo, None);
        assert_eq!(durations.pass_iishanten_self_tsumo, Duration::ZERO);
        assert_eq!(candidates.len(), 1);
        assert!(candidates[0].elapsed > Duration::ZERO);
    }

    #[test]
    fn the_call_decision_is_the_same_with_and_without_the_timing_and_the_diagnostics() {
        let action = pon_action(IISHANTEN_PON_TARGET, &IISHANTEN_PON_CONSUMED);
        let ctx = valued_reaction_context(&IISHANTEN_PON_HAND, IISHANTEN_PON_TARGET, 1, 12);
        let legal_actions = [action.clone(), LegalAction::None];
        let untimed = evaluate_call_decision(
            &ctx,
            &legal_actions,
            false,
            &mut CallDecisionTimer::disabled(),
        )
        .expect("evaluated");
        let (timed, _, _) = measured_call_decision(&ctx, &legal_actions, false);
        let (diagnosed, _, _) = measured_call_decision(&ctx, &legal_actions, true);

        assert_eq!(untimed.selected, Some(action));
        assert_eq!(timed.expect("evaluated").selected, untimed.selected);
        assert_eq!(diagnosed.expect("evaluated").selected, untimed.selected);
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
    fn two_shanten_call_that_stays_two_shanten_is_not_observed() {
        let ctx = reaction_context(&RYANSHANTEN_PON_HAND, IISHANTEN_PON_TARGET);
        let action = pon_action(IISHANTEN_PON_TARGET, &IISHANTEN_PON_CONSUMED);
        let (decision, candidate) = single_candidate(&ctx, &action, true);

        assert_eq!(candidate.reason, CallDecisionReason::CurrentShantenNotOne);
        assert_eq!(candidate.current_shanten, Some(2));
        assert_eq!(candidate.post_call_shanten(), Some(2));
        assert_eq!(candidate.iishanten_acceptance, None);
        assert_eq!(candidate.two_shanten_self_tsumo, None);
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
        assert_eq!(candidate.reason, CallDecisionReason::ReactionSourceUnknown);
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
        let ctx = valued_reaction_context(&IISHANTEN_PON_HAND, IISHANTEN_PON_TARGET, 1, 12);
        let action = pon_action(IISHANTEN_PON_TARGET, &IISHANTEN_PON_CONSUMED);
        let actions = [action.clone(), LegalAction::None];

        let mut agent = crate::agents::ShantenAgent;
        let acted = crate::agent::Agent::act(&mut agent, &ctx, &actions);
        assert_eq!(acted, action);

        // diagnose() は観測値を集めるが、選ぶ action は act() と同じ。
        let diagnostic = crate::agents::ShantenAgent::diagnose(&ctx, &actions);
        assert_eq!(diagnostic.selected_action, acted);
        let call = diagnostic.call.as_ref().expect("evaluated");
        assert_eq!(call.selected, Some(acted));
        assert!(call.candidates[0].iishanten_acceptance.is_some());
        assert_eq!(
            call.candidates[0]
                .iishanten_self_tsumo
                .expect("production comparison")
                .comparison,
            CallIishantenComparison::CallHigher
        );
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

    // 3m を 4m5m で Chi できる 4m が2枚ある一向聴。456m 78s ではなく 678m 234s を使う形で、
    // 消費する物理牌だけが違う同じ Chi が2件並ぶ。鳴いた後は余った 4m を切ると 3p / 7s の
    // シャンポン待ちテンパイになり、全て中張牌なので役もある。
    const DUPLICATE_CHI_HAND: [u8; 13] = [12, 13, 17, 20, 24, 28, 44, 45, 76, 80, 84, 96, 97];
    const DUPLICATE_CHI_TARGET: u8 = 8;
    const DUPLICATE_CHI_CONSUMED: [[u8; 2]; 2] = [[12, 17], [13, 17]];

    // 同じ形で 4m を1枚にし、5m を赤5と黒5の2枚にしたもの。2件の Chi は consumed の赤5と、
    // 鳴き後に手牌へ残る5の赤5が入れ替わる。
    const RED_FIVE_CHI_HAND: [u8; 13] = [12, 16, 17, 20, 24, 28, 44, 45, 76, 80, 84, 96, 97];
    const RED_FIVE_CHI_CONSUMED: [[u8; 2]; 2] = [[12, 16], [12, 17]];

    fn duplicate_chi_actions(consumed: &[[u8; 2]; 2]) -> Vec<LegalAction> {
        consumed
            .iter()
            .map(|consumed| chi_action(DUPLICATE_CHI_TARGET, consumed))
            .collect()
    }

    // 候補1件を単独で評価した結果。dedup で複製した候補が、独立に評価した場合と同じ内容かを
    // 比べるための基準にする。`selected` は候補集合で決まるので比較対象から外す。
    fn independently_evaluated_candidate(
        ctx: &GameContext,
        action: &LegalAction,
        collect_observations: bool,
    ) -> CallCandidateDiagnostic {
        let (_, mut candidate) = single_candidate(ctx, action, collect_observations);
        candidate.selected = false;
        candidate
    }

    #[test]
    fn semantically_equal_calls_evaluate_the_post_call_discard_once() {
        let ctx = valued_reaction_context(&DUPLICATE_CHI_HAND, DUPLICATE_CHI_TARGET, 1, 12);
        let actions = duplicate_chi_actions(&DUPLICATE_CHI_CONSUMED);
        let mut legal_actions = actions.clone();
        legal_actions.push(LegalAction::None);
        let (decision, durations, candidates) = measured_call_decision(&ctx, &legal_actions, false);
        let decision = decision.expect("evaluated");

        // 件数・順序・action は元の合法 action のまま。
        assert_eq!(decision.candidates.len(), actions.len());
        for (candidate, action) in decision.candidates.iter().zip(&actions) {
            assert_eq!(&candidate.action, action);
        }

        // 高コストな鳴き後の打牌評価は1回だけ。
        assert_eq!(candidates.len(), actions.len());
        assert!(!candidates[0].reused);
        assert!(candidates[0].post_call_discard_selection > Duration::ZERO);
        assert!(candidates[1].reused);
        assert_eq!(candidates[1].elapsed, Duration::ZERO);
        assert_eq!(candidates[1].post_call_discard_selection, Duration::ZERO);
        assert_eq!(durations.candidates, candidates[0].elapsed);

        // 再利用した候補は consumed の物理牌だけが違い、判断内容は独立評価と一致する。
        assert_eq!(candidates[1].consumed, tiles(&DUPLICATE_CHI_CONSUMED[1]));
        for (candidate, action) in decision.candidates.iter().zip(&actions) {
            let mut expected = independently_evaluated_candidate(&ctx, action, false);
            expected.selected = candidate.selected;
            assert_eq!(candidate, &expected);
        }
    }

    #[test]
    fn semantically_equal_calls_keep_the_first_legal_action_as_the_selected_call() {
        let ctx = valued_reaction_context(&DUPLICATE_CHI_HAND, DUPLICATE_CHI_TARGET, 1, 12);
        let actions = duplicate_chi_actions(&DUPLICATE_CHI_CONSUMED);
        let mut legal_actions = actions.clone();
        legal_actions.push(LegalAction::None);
        let decision = evaluate_call_decision(
            &ctx,
            &legal_actions,
            false,
            &mut CallDecisionTimer::disabled(),
        )
        .expect("evaluated");

        // 完全同値の候補なので、既存 tie-break どおり先頭の合法 action を採る。
        assert_eq!(decision.reason, CallDecisionReason::EligibleTenpai);
        assert_eq!(decision.selected.as_ref(), Some(&actions[0]));
        assert!(decision.candidates[0].selected);
        assert!(!decision.candidates[1].selected);

        // 候補が1件だけの場合と同じ action を選ぶ。
        let (single, _) = single_candidate(&ctx, &actions[0], false);
        assert_eq!(single.selected, decision.selected);
    }

    #[test]
    fn calls_that_differ_only_in_the_red_five_are_evaluated_separately() {
        let ctx = valued_reaction_context(&RED_FIVE_CHI_HAND, DUPLICATE_CHI_TARGET, 1, 12);
        let actions = duplicate_chi_actions(&RED_FIVE_CHI_CONSUMED);
        let mut legal_actions = actions.clone();
        legal_actions.push(LegalAction::None);
        let (decision, _, candidates) = measured_call_decision(&ctx, &legal_actions, false);
        let decision = decision.expect("evaluated");

        assert_eq!(decision.candidates.len(), actions.len());
        for (candidate, action) in decision.candidates.iter().zip(&actions) {
            assert_eq!(&candidate.action, action);
        }
        // 表示上は同じ 3m<-4m,5m でも赤5の位置が違うので、どちらも実際に評価する。
        assert_eq!(candidates.len(), actions.len());
        for candidate in &candidates {
            assert!(!candidate.reused);
            assert!(candidate.post_call_discard_selection > Duration::ZERO);
        }
    }

    fn evaluation_key(hand: &[u8], target: u8, consumed: &[u8]) -> CallEvaluationKey {
        let (meld, post_call_tiles) = call_meld_and_concealed_tiles(
            &tiles(hand),
            CallKind::Chi,
            tile(target),
            &tiles(consumed),
        )
        .expect("valid chi");
        CallEvaluationKey::new(&meld, &post_call_tiles)
    }

    #[test]
    fn the_semantic_key_only_ignores_the_physical_copy_of_the_same_tile() {
        // 同じ牌種・同じ赤黒の別コピーを消費する Chi は同じ key。
        assert_eq!(
            evaluation_key(
                &DUPLICATE_CHI_HAND,
                DUPLICATE_CHI_TARGET,
                &DUPLICATE_CHI_CONSUMED[0]
            ),
            evaluation_key(
                &DUPLICATE_CHI_HAND,
                DUPLICATE_CHI_TARGET,
                &DUPLICATE_CHI_CONSUMED[1]
            )
        );

        // 赤5と黒5のどちらを鳴くかは別 key。
        assert_ne!(
            evaluation_key(
                &RED_FIVE_CHI_HAND,
                DUPLICATE_CHI_TARGET,
                &RED_FIVE_CHI_CONSUMED[0]
            ),
            evaluation_key(
                &RED_FIVE_CHI_HAND,
                DUPLICATE_CHI_TARGET,
                &RED_FIVE_CHI_CONSUMED[1]
            )
        );

        // 喰い替え禁止牌が変わる鳴き方も別 key。
        assert_ne!(
            evaluation_key(&DUPLICATE_CHI_HAND, 20, &[12, 17]),
            evaluation_key(&DUPLICATE_CHI_HAND, DUPLICATE_CHI_TARGET, &[12, 17])
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
