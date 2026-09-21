//! カン判断 policy 層。Chi / Pon の鳴き判断 ([`crate::call_decision`]) とは別の責務として持つ。
//!
//! Chi / Pon は他家打牌への reaction で「鳴く → 直後に打牌する」を1つの評価単位にできるが、
//! カンはそうならない。
//!
//! | 種別 | 契機 | 直後の手番 |
//! | --- | --- | --- |
//! | Ankan | 自分のツモ番 | 嶺上牌を引いてから打牌 |
//! | Kakan | 自分のツモ番 | 嶺上牌を引いてから打牌 (搶槓あり) |
//! | Daiminkan | 他家打牌への reaction | 嶺上牌を引いてから打牌 |
//!
//! どの種別も「カン → 未知の嶺上牌 → 打牌」なので、Chi / Pon の
//! `Call → post-call discard` 評価モデルをそのまま当てはめられない。そのため鳴き判断へ混ぜず、
//! この層を分ける。
//!
//! # 今回 production へ接続する範囲
//!
//! production で選べるのは [`KanKind::Ankan`] だけである。[`KanKind::Kakan`] と
//! [`KanKind::Daiminkan`] は候補として診断には並ぶが、理由
//! ([`KanDecisionReason::KakanNotConnected`] / [`KanDecisionReason::DaiminkanNotConnected`])
//! を付けて必ず選ばない。
//!
//! # source of truth
//!
//! | 材料 | source of truth |
//! | --- | --- |
//! | カンの合法性 | 入力の `legal_actions` (`possible_actions` 由来) |
//! | 面子の形の検証 | [`Meld::shape`] |
//! | 副露済み面子数 | [`GameContext::own_fixed_meld_count`] / [`FixedMeldCount`] |
//! | カン後の向聴数 | [`calculate_shanten_with_fixed_melds`] |
//! | カン後の受け入れ | [`calculate_acceptance_with_fixed_melds`] / [`calculate_acceptance_with_fixed_melds_and_visible_tiles`] |
//! | 暗槓しない場合の打牌 | production の通常打牌選択が選んだ [`DiscardEvaluation`] |
//! | 押し引き | [`decide_push_pull`](crate::push_pull::decide_push_pull) の結論 |
//!
//! リーチ後に暗槓できるか (待ちが変わらないか) も含めて、合法性の判定はこの層に持たない。
//! `legal_actions` に [`LegalAction::Ankan`] が並んでいることだけを合法の根拠にする。
//!
//! # 判断する位置
//!
//! `ShantenAgent` は
//!
//! ```text
//! Hora → 九種九牌 → Chi / Pon → 押し引き → Reach → Kan → 通常打牌 → 防御 fallback
//! ```
//!
//! の順で action を決める。カンは Push mode でリーチを採用しなかった後にだけ検討するので、
//! 既存の Hora / 九種九牌 / Reach の優先順位は変わらない。押し引きの gate 自体はこの層が持ち、
//! 呼び出し側は結論 ([`PushPullMode`]) をそのまま渡す。
//!
//! # 暗槓の成立条件
//!
//! ```text
//! 合法な Ankan がある
//! AND 他家にリーチ者がいない
//! AND 既存 Push/Pull policy が Push と判定している
//! AND 自分のツモを経たと確認できる (drawn_tile がある)
//! AND 自分の副露済み面子数が分かり、暗槓後も上限内
//! AND consumed 4枚を手牌 + ツモ牌から取り除いてカンの形になる
//! AND 暗槓しない場合に選ぶ通常打牌の評価がある
//! AND 暗槓後の向聴数 <= その通常打牌後の向聴数
//! AND 暗槓後の受け入れ残枚数 >= その通常打牌後の受け入れ残枚数
//! AND 暗槓後の受け入れ牌種数 >= その通常打牌後の受け入れ牌種数
//! ```
//!
//! ## 比較する2つの state
//!
//! 暗槓する場合としない場合を、どちらも「13枚相当で次のツモを待つ state」に揃えて比べる。
//!
//! ```text
//! 暗槓しない: 14枚 → 通常打牌 → 13枚 (副露 N)        → 次のツモを待つ
//! 暗槓する  : 14枚 → 暗槓     → 10枚 (副露 N+1)      → 嶺上牌を待つ
//! ```
//!
//! `10枚 + 副露 N+1` と `13枚 + 副露 N` はどちらも `13 - 3 * 副露数` 枚の同じ大きさの手牌なので、
//! 既存の向聴・受け入れをそのまま同じ尺度で比べられる。比較の基準に使う打牌は production の
//! 通常打牌選択が実際に選んだ1件そのもので、この層で打牌を選び直さない。
//!
//! この条件を満たす暗槓は「4枚が面子以外の使い道を持っていなかった」ことを意味する。速度
//! (向聴・受け入れ) を既存評価の範囲で悪化させないカンだけを選ぶ、という条件であり、枚数や
//! 向聴だけを見た閾値は持たない。
//!
//! ## 今回評価に含めないもの
//!
//! 暗槓には既存評価だけでは値を確定できない要素があり、係数や推定値を置かずに「評価に含め
//! ない」ままにする。含めていないものは次のとおりで、いずれも TODO として残す。
//!
//! - **新ドラ**: カンで増えるドラ表示牌の中身は未知なので、自分の打点にも他家の打点にも
//!   加算しない。他家リーチ中に暗槓しない ([`KanDecisionReason::OpponentReached`]) のは、
//!   この未知のドラがリーチ者の打点をどれだけ押し上げるかを既存評価で測れないためである。
//! - **嶺上牌**: 引く牌は未知なので、特定の牌を引いた後の state として評価しない。暗槓が1回
//!   分多くツモれることも評価へ足さない。
//! - **符とカンの打点**: 暗刻が暗槓になることで増える符は既存 scoring が持つが、暗槓後の
//!   将来打点をこの層では評価しない。
//!
//! したがって今回の条件は「既存評価で測れる速度が悪化しない範囲」に限定した最小の production
//! behavior であり、暗槓の打点上昇も新ドラのリスクも判断材料に入っていない。
//!
//! # 判断にかかるコスト
//!
//! 比較の基準は production の通常打牌選択が既に求めた評価そのものなので、この層が新たに行う
//! のは暗槓後10枚の向聴と受け入れの計算1回だけである。前方探索も点数計算も通らない。しかも
//! 合法なカンが1件も無い局面では候補の列挙で終わり、他家リーチ中と Push 以外の押し引きでは
//! 手牌を組み立てる前に落ちるので、通常の打牌局面へ載るコストは無い。
//!
//! TODO: 暗槓前後を ExpectedSelfTsumoValue で比較する。暗槓後は未知の嶺上牌を待つ state なので
//! `awaiting_draw_*` 系の既存評価を当てられるが、嶺上牌ぶんの追加ツモと新ドラを片側だけ持つ
//! 非対称な比較になる。補正を推測で置かないため、今回は接続しない。
//!
//! TODO: Kakan の搶槓リスクと Daiminkan の reaction モデルを評価できるようにしてから、
//! [`KanKind`] の残り2種別を production へ接続する。

use bot_logic::{
    DiscardEvaluation, FixedMeldCount, Meld, MeldKind, TileCounts, TileId, TileType,
    calculate_acceptance_with_fixed_melds, calculate_acceptance_with_fixed_melds_and_visible_tiles,
    calculate_shanten_with_fixed_melds,
};

use crate::action::LegalAction;
use crate::context::GameContext;
use crate::push_pull::PushPullMode;

/// 暗槓が消費する物理牌の枚数。
const ANKAN_CONSUMED_TILE_COUNT: usize = 4;

/// 評価対象のカン種別。
///
/// 今回 production で選べるのは [`Self::Ankan`] だけで、残り2種別は候補として診断に並ぶだけ。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KanKind {
    Ankan,
    Kakan,
    Daiminkan,
}

impl KanKind {
    /// 対応する既存の副露種別。
    pub fn meld_kind(self) -> MeldKind {
        match self {
            Self::Ankan => MeldKind::Ankan,
            Self::Kakan => MeldKind::Kakan,
            Self::Daiminkan => MeldKind::Daiminkan,
        }
    }

    /// 今回 production で選択できる種別か。
    pub fn is_production_connected(self) -> bool {
        matches!(self, Self::Ankan)
    }

    // 今回 production 接続していない種別の理由。
    fn not_connected_reason(self) -> Option<KanDecisionReason> {
        match self {
            Self::Ankan => None,
            Self::Kakan => Some(KanDecisionReason::KakanNotConnected),
            Self::Daiminkan => Some(KanDecisionReason::DaiminkanNotConnected),
        }
    }
}

/// カンを採用した / しなかった理由。
///
/// [`Self::EligibleAnkanNoRegression`] 以外はすべて「今回はカンしない」理由であり、最初に落ちた
/// 条件を1つだけ表す。判定順は [`KanCandidateDiagnostic`] のフィールドが埋まる順と一致する。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KanDecisionReason {
    /// 全条件を満たし、既存評価で測れる向聴・受け入れが暗槓で悪化しない。
    EligibleAnkanNoRegression,
    /// 加槓はまだ production へ接続していない。搶槓リスクを評価できていない。
    KakanNotConnected,
    /// 大明槓はまだ production へ接続していない。reaction としての評価モデルが無い。
    DaiminkanNotConnected,
    /// 他家にリーチ者がいる。新ドラがリーチ者の打点へ与える影響を既存評価で測れない。
    OpponentReached,
    /// 既存 Push/Pull policy が Push と判定していない。
    NotPush,
    /// 自分のツモを経たと確認できない。暗槓はツモ番の action なので、この局面では判断しない。
    NotAfterOwnDraw,
    /// 自分の副露済み面子数が不明。0副露と推測しない。
    FixedMeldCountUnknown,
    /// 暗槓後の副露済み面子数が上限を超える。
    FixedMeldCountOverflow,
    /// consumed が4枚でない・手牌に無い・カンの形にならない。
    InvalidConsumed,
    /// 比較の基準になる通常打牌評価が無く、暗槓しない場合と比べられない。
    NormalDiscardUnavailable,
    /// 暗槓後の向聴数が、暗槓しない場合の通常打牌後より悪くなる。
    ShantenRegresses,
    /// 向聴数は悪化しないが、暗槓後の受け入れが通常打牌後より減る。
    AcceptanceRegresses,
}

impl KanDecisionReason {
    /// カンを採用した理由か。
    pub fn is_eligible(self) -> bool {
        matches!(self, Self::EligibleAnkanNoRegression)
    }
}

/// 比較に使った13枚相当 state 1つ分の既存評価。
///
/// 値はすべて既存 layer が求めたもので、診断のために向聴も受け入れも計算し直さない。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KanHandDiagnostic {
    /// 副露済み面子数を含めた effective shanten。
    pub shanten: i8,
    /// 受け入れ残枚数 [枚]。
    pub acceptance_remaining: u8,
    /// 受け入れ牌種数。
    pub acceptance_type_count: usize,
}

/// 合法な `LegalAction::Ankan` / `LegalAction::Kakan` / `LegalAction::Daiminkan` 1件ごとの判断内訳。
///
/// 各フィールドは判定が実際にそこまで進んだ場合だけ `Some` になり、進まなかった判定は推測せず
/// `None` のままにする。評価不能で落ちた候補は `reason` がその理由を表す。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KanCandidateDiagnostic {
    pub action: LegalAction,
    pub kind: KanKind,
    /// カンの対象牌種。Ankan は consumed の牌種、Kakan / Daiminkan は対象の打牌 / 手出し牌。
    pub tile: Option<TileType>,
    pub current_fixed_meld_count: Option<FixedMeldCount>,
    pub post_kan_fixed_meld_count: Option<FixedMeldCount>,
    /// カンしない場合に採用する通常打牌と、その打牌後13枚の既存評価。
    ///
    /// production の通常打牌選択が選んだ1件そのもので、この層で選び直さない。
    pub baseline_discard: Option<TileType>,
    pub baseline: Option<KanHandDiagnostic>,
    /// 暗槓後の13枚相当 state の既存評価。
    pub post_kan: Option<KanHandDiagnostic>,
    pub eligible: bool,
    pub selected: bool,
    pub reason: KanDecisionReason,
}

impl KanCandidateDiagnostic {
    /// 暗槓後 - 暗槓しない場合の向聴数差。負なら暗槓後の方が良い。両方を評価した場合だけ `Some`。
    pub fn shanten_delta(&self) -> Option<i8> {
        Some(self.post_kan?.shanten - self.baseline?.shanten)
    }

    /// 暗槓後 - 暗槓しない場合の受け入れ残枚数差 [枚]。符号付き。
    pub fn acceptance_remaining_delta(&self) -> Option<i16> {
        Some(
            i16::from(self.post_kan?.acceptance_remaining)
                - i16::from(self.baseline?.acceptance_remaining),
        )
    }

    /// 暗槓後 - 暗槓しない場合の受け入れ牌種数差。符号付き。
    pub fn acceptance_type_delta(&self) -> Option<isize> {
        Some(
            self.post_kan?.acceptance_type_count as isize
                - self.baseline?.acceptance_type_count as isize,
        )
    }
}

/// カン判断の構造化診断。
///
/// `selected` は `ShantenAgent::act()` が実際に採用したカンそのもので、診断用の別判断ロジック
/// は持たない。採用が無い場合の `reason` は最初の候補が落ちた理由。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KanDecisionDiagnostic {
    pub selected: Option<LegalAction>,
    pub reason: KanDecisionReason,
    pub candidates: Vec<KanCandidateDiagnostic>,
}

// カン判断の本体。act() と構造化診断はこの1本を共有し、診断は結果を載せるだけにする。
//
// 合法なカンが1件も無ければ検討自体を行わず None。1件以上ある場合は候補ごとに独立して条件を
// 評価し、成立した候補のうち合法 action の列挙順で最初の1件を選ぶ。同時に2つの暗槓が合法に
// なる局面は稀で、どちらも「既存評価を悪化させない」ことしか確認していないため、順序以外の
// tie-break は持たない。
//
// `normal_discard` は暗槓しない場合に採用する通常打牌の評価。production の通常打牌選択が選んだ
// ものをそのまま受け取り、この層で打牌を選び直さない。
pub(crate) fn evaluate_kan_decision(
    ctx: &GameContext,
    legal_actions: &[LegalAction],
    mode: PushPullMode,
    normal_discard: Option<&DiscardEvaluation>,
) -> Option<KanDecisionDiagnostic> {
    let mut candidates: Vec<KanCandidateDiagnostic> = Vec::new();
    for action in legal_actions {
        let Some((kind, called_tile, consumed)) = normalize_kan(action) else {
            continue;
        };
        let mut candidate = new_kan_candidate(action, kind, called_tile, consumed);
        let reason =
            evaluate_kan_candidate(ctx, kind, consumed, mode, normal_discard, &mut candidate);
        candidate.eligible = reason.is_eligible();
        candidate.reason = reason;
        candidates.push(candidate);
    }

    if candidates.is_empty() {
        return None;
    }

    let selected_index = candidates.iter().position(|candidate| candidate.eligible);
    if let Some(index) = selected_index {
        candidates[index].selected = true;
    }

    let reason = candidates[selected_index.unwrap_or(0)].reason;
    let selected = selected_index.map(|index| candidates[index].action.clone());

    Some(KanDecisionDiagnostic {
        selected,
        reason,
        candidates,
    })
}

// 合法 action をカンの共通表現へ正規化する。それ以外の action は対象外。
fn normalize_kan(action: &LegalAction) -> Option<(KanKind, Option<TileId>, &[TileId])> {
    match action {
        LegalAction::Ankan { consumed } => Some((KanKind::Ankan, None, consumed)),
        LegalAction::Kakan { tile, consumed } => Some((KanKind::Kakan, Some(*tile), consumed)),
        LegalAction::Daiminkan { tile, consumed } => {
            Some((KanKind::Daiminkan, Some(*tile), consumed))
        }
        _ => None,
    }
}

fn new_kan_candidate(
    action: &LegalAction,
    kind: KanKind,
    called_tile: Option<TileId>,
    consumed: &[TileId],
) -> KanCandidateDiagnostic {
    KanCandidateDiagnostic {
        action: action.clone(),
        kind,
        tile: called_tile
            .or_else(|| consumed.first().copied())
            .map(TileId::tile_type),
        current_fixed_meld_count: None,
        post_kan_fixed_meld_count: None,
        baseline_discard: None,
        baseline: None,
        post_kan: None,
        eligible: false,
        selected: false,
        reason: KanDecisionReason::EligibleAnkanNoRegression,
    }
}

// 候補1件の条件を上から順に評価し、最初に落ちた理由を返す。評価が進んだ範囲の値だけを
// candidate へ書き込み、評価しなかった項目は None のままにする。
fn evaluate_kan_candidate(
    ctx: &GameContext,
    kind: KanKind,
    consumed: &[TileId],
    mode: PushPullMode,
    normal_discard: Option<&DiscardEvaluation>,
    candidate: &mut KanCandidateDiagnostic,
) -> KanDecisionReason {
    if let Some(reason) = kind.not_connected_reason() {
        return reason;
    }

    if ctx.any_opponent_reached() {
        return KanDecisionReason::OpponentReached;
    }

    if mode != PushPullMode::Push {
        return KanDecisionReason::NotPush;
    }

    if !ctx.is_after_own_draw() {
        return KanDecisionReason::NotAfterOwnDraw;
    }

    let Some(current_fixed_meld_count) = ctx.own_fixed_meld_count() else {
        return KanDecisionReason::FixedMeldCountUnknown;
    };
    candidate.current_fixed_meld_count = Some(current_fixed_meld_count);

    let Some(post_kan_fixed_meld_count) = FixedMeldCount::new(current_fixed_meld_count.get() + 1)
    else {
        return KanDecisionReason::FixedMeldCountOverflow;
    };
    candidate.post_kan_fixed_meld_count = Some(post_kan_fixed_meld_count);

    let Some(post_kan_tiles) = ankan_concealed_tiles(ctx, consumed) else {
        return KanDecisionReason::InvalidConsumed;
    };

    let Some(normal_discard) = normal_discard else {
        return KanDecisionReason::NormalDiscardUnavailable;
    };
    candidate.baseline_discard = Some(normal_discard.discard);
    let baseline = KanHandDiagnostic {
        shanten: normal_discard.min_shanten_after_discard(),
        acceptance_remaining: normal_discard.acceptance_total_remaining(),
        acceptance_type_count: normal_discard.acceptance_type_count(),
    };
    candidate.baseline = Some(baseline);

    let post_kan = evaluate_post_kan_hand(ctx, &post_kan_tiles, post_kan_fixed_meld_count);
    candidate.post_kan = Some(post_kan);

    if post_kan.shanten > baseline.shanten {
        return KanDecisionReason::ShantenRegresses;
    }

    if post_kan.acceptance_remaining < baseline.acceptance_remaining
        || post_kan.acceptance_type_count < baseline.acceptance_type_count
    {
        return KanDecisionReason::AcceptanceRegresses;
    }

    KanDecisionReason::EligibleAnkanNoRegression
}

// 暗槓で consumed 4枚を取り除いた後の concealed hand。
//
// 手牌 + ツモ牌から consumed の物理牌をちょうど1枚ずつ取り除き、取り除いた4枚がカンの形に
// なることを既存 [`Meld::shape`] で確かめる。形の規則をこの層で持たない。
fn ankan_concealed_tiles(ctx: &GameContext, consumed: &[TileId]) -> Option<Vec<TileId>> {
    if consumed.len() != ANKAN_CONSUMED_TILE_COUNT {
        return None;
    }

    let mut remaining: Vec<TileId> = ctx
        .hand_tiles()
        .iter()
        .copied()
        .chain(ctx.drawn_tile())
        .collect();
    for consumed_tile in consumed {
        let position = remaining.iter().position(|held| held == consumed_tile)?;
        remaining.remove(position);
    }

    let meld = Meld::new(MeldKind::Ankan, consumed.to_vec(), None);
    meld.shape()?;
    Some(remaining)
}

// 暗槓後13枚相当 state の既存評価。見え牌の有無による経路分岐は通常打牌評価と揃える。
fn evaluate_post_kan_hand(
    ctx: &GameContext,
    tiles: &[TileId],
    fixed_meld_count: FixedMeldCount,
) -> KanHandDiagnostic {
    let counts = TileCounts::from_tiles(tiles.iter().copied());
    let shanten = calculate_shanten_with_fixed_melds(&counts, fixed_meld_count).min();
    let acceptance = if ctx.visible_tiles().is_empty() {
        calculate_acceptance_with_fixed_melds(&counts, fixed_meld_count)
    } else {
        calculate_acceptance_with_fixed_melds_and_visible_tiles(
            &counts,
            fixed_meld_count,
            ctx.visible_tiles(),
        )
    };

    KanHandDiagnostic {
        shanten,
        acceptance_remaining: acceptance.total_remaining(),
        acceptance_type_count: acceptance.tiles.len(),
    }
}

#[cfg(test)]
mod tests;
