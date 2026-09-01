//! 現在聴牌をダマで継続した場合の、次の1巡分を観測するための診断層。
//!
//! ```text
//! 現在聴牌 → 非和了牌を1枚ツモ → 既存 selector の最善打牌 → 再び聴牌
//! ```
//!
//! という枝だけを取り出す。探索も打牌選択も打点計算もこの層は持たず、既存の2手先評価
//! ([`LookaheadDiagnostic`]) が構築済みの枝と、その枝を評価済みの将来打点
//! ([`ProspectiveLookaheadDiagnostic`]) を絞り込むだけである。
//!
//! # 継続枝に含めるもの
//!
//! 待ちが実際に変わる枝 (手変わり) だけでなく、ツモった牌をそのまま切って元の聴牌・元の待ちを
//! 維持する枝も継続枝として扱う。「今すぐリーチする」と「ダマで継続する」を比べるには、次の
//! 1巡で待ちが変わらない場合の価値も同じ枝集合の中に必要になるためである。枝を待ちの変化で
//! 分類 (据え置き / 待ち改善 / 打点改善) することは、この層ではまだ行わない。
//!
//! # 枝の出どころ
//!
//! 非和了ツモは既存2手先評価の [`DrawTransition::SameShanten`] そのものである。現在打牌後が
//! 聴牌の候補では、和了牌は向聴数を下げるので [`DrawTransition::Progress`] に分類され、向聴数を
//! 維持する牌 = 非和了牌になる。継続枝からの和了牌の除外は、この既存分類をそのまま使うだけで、
//! この層が和了牌を判定し直すことはない。
//!
//! 次打牌も既存の打牌評価と既存 comparator が選んだ [`DrawVariantLookaheadDiagnostic::next_discard`]
//! そのもので、向聴・受け入れ・赤5・ドラ・形・将来打点のどの比較もここで作り直さない。その
//! 打牌後が再び聴牌の枝だけを継続成立として扱い、聴牌に戻らない枝は含めない。
//!
//! horizon は「1ツモ → 1打牌 → 次の聴牌」で必ず打ち切る。2回目の非和了ツモは追わない。
//!
//! # 対象局面
//!
//! 自分が未リーチと確定している局面の、現在打牌後が聴牌の候補だけを対象にする。既にリーチ
//! していればダマで継続する選択肢が無いので探索対象にせず、自分の席が分からず未リーチかどうかを
//! 判断できない局面でも未リーチだと推測しない。どちらも [`TenpaiContinuationDiagnostic`] を
//! 構築しない (`None`)。
//!
//! # self-tsumo 比較
//!
//! 「今すぐリーチする」と「ダマで1巡継続する」を、既存 self-tsumo 確率模型
//! ([`bot_logic::self_tsumo`](bot_logic)) の期待ツモ支払い
//! [[`SELF_TSUMO_VALUE_SCALE`](bot_logic::SELF_TSUMO_VALUE_SCALE)] という同じ単位へ揃える。
//!
//! ```text
//! U0 = 現在打牌後の unknown physical tiles
//! n  = 現在打牌後に残っている自分の自摸機会
//!
//! reach now         = 現在聴牌の forced Reach Tsumo baseline を
//!                     TenpaiTsumoValue::expected_payment(U0, n) で評価
//!
//! damaten           = 現在聴牌の Damaten Tsumo baseline を最初の1自摸だけ
//!   immediate tsumo   評価した expected_payment(U0, 1)
//!
//! damaten           = Σ(非和了牌 variant を最初に引く経路確率
//!   continuation         × その先の terminal tenpai の期待ツモ支払い)
//!   branches
//! ```
//!
//! 経路確率も terminal tenpai の horizon も既存 [`SelfTsumoPath::immediate`] そのままなので、
//! 継続後は自然に `U0 - 1` / `n - 1` になる。この `-1` をこの層が数え直すことはない。
//! terminal tenpai のツモ打点も、既存2手先評価が枝に持っている
//! [`DrawVariantLookaheadDiagnostic::tsumo_continuation`] をそのまま使う。したがって継続後の
//! Reach / Damaten も既存 prospective evaluation が決めたモードのままで、「今ダマを選んだから
//! 未来もダマ」と固定しない。
//!
//! 現在の和了牌は最初の自摸で引く枝 (Damaten Tsumo baseline) としてだけ数え、継続枝
//! ([`DrawTransition::SameShanten`]) には現れないので二重計上にならない。
//!
//! 現在局面でリーチできるかは、production の現在リーチ判断と同じく実際の合法手
//! ([`LegalAction::Reach`](crate::action::LegalAction::Reach)) だけが source of truth になる。
//! 局面から合法条件を組み立て直さない。継続後の未来テンパイのリーチ可否だけは現在の合法手を
//! 流用できないので、既存の将来テンパイ判定 ([`crate::prospective_value`]) がそのまま持つ。
//!
//! ```text
//! 現在の reach now        → 実際の LegalAction::Reach
//! 継続後の未来テンパイ    → 既存の将来テンパイ Reach 判定
//! ```
//!
//! 評価できない値は 0 点にしない。[`SelfTsumoFacts`] を作れない・ツモ打点の入力が足りない・
//! 合法手にリーチが無い・継続枝の terminal ツモ打点が確定しない場合は、その集計値を `None` に
//! する。「探索していない」と「0点」を混同しない。
//!
//! # 打牌選択への接続
//!
//! 現時点では diagnostics 専用で、打牌選択には接続していない。self-tsumo 比較にも winner も
//! `should_reach` も持たせず、現在聴牌の
//! [`OffenseValue`](crate::offense_value::OffenseValue) のような別単位の値と比べる係数も
//! threshold も持たない。

use bot_logic::{
    DiscardEvaluation, DiscardLookaheadDiagnostic, DrawLookaheadDiagnostic, DrawTransition,
    DrawVariantLookaheadDiagnostic, EffectiveAcceptanceTile, LookaheadDiagnostic,
    ProspectiveTenpai, SelfTsumoFacts, SelfTsumoPath, TenpaiTsumoValue, TileId, TileType,
    split_discarded_tile,
};

use crate::context::GameContext;
use crate::offense_value::TenpaiOffenseMode;
use crate::prospective_value::{
    ProductionProspectiveValuator, ProspectiveDiscardValue, ProspectiveDrawValue,
    ProspectiveDrawVariantValue, ProspectiveFacts, ProspectiveLookaheadDiagnostic,
    ProspectiveTenpaiValue, ProspectiveWaitValue,
};

// テンパイの向聴数。
const TENPAI_SHANTEN: i8 = 0;

// ダマで現在の待ちをツモ和了できるのは、手変わりする前の1自摸だけ。2巡目以降は継続枝側が
// terminal tenpai として評価する。
const FIRST_DRAW: u32 = 1;

/// 現在聴牌候補をダマで継続した場合の診断。
///
/// `candidates` は現在打牌後が聴牌の候補だけを、既存2手先評価と同じ順序で並べる。打牌選択にも
/// 押し引きにもリーチ判断にも使わない解析専用の情報で、構築の有無は選択結果を変えない。
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TenpaiContinuationDiagnostic {
    pub candidates: Vec<TenpaiContinuationCandidate>,
}

impl TenpaiContinuationDiagnostic {
    pub fn candidate(&self, discard: TileType) -> Option<&TenpaiContinuationCandidate> {
        self.candidates
            .iter()
            .find(|candidate| candidate.discard == discard)
    }
}

/// 現在聴牌候補1件分の継続枝。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TenpaiContinuationCandidate {
    /// 現在の打牌候補の牌種。この打牌後が現在聴牌になる。
    pub discard: TileType,
    /// 現在聴牌の待ち。既存打牌評価の受け入れそのもので、この診断のために求め直さない。
    pub current_wait: Vec<EffectiveAcceptanceTile>,
    /// 継続が成立した枝。非和了ツモの物理牌 variant 単位で並ぶ。
    ///
    /// 待ちが変わる枝と、ツモ切りで元の待ちを維持する枝の両方を含む。
    pub branches: Vec<TenpaiContinuationBranch>,
    /// 「今すぐリーチ」と「ダマで1巡継続」を同じ期待ツモ支払いで並べた比較。
    pub self_tsumo: TenpaiSelfTsumoComparison,
}

impl TenpaiContinuationCandidate {
    /// 現在聴牌の待ちの残枚数合計。
    pub fn current_wait_remaining(&self) -> u32 {
        self.current_wait
            .iter()
            .map(|tile| u32::from(tile.remaining))
            .sum()
    }

    /// 継続が成立した枝の残枚数合計。
    ///
    /// 期待値でも和了率でもなく、成立枝の物理牌が何枚あるかを数えただけの観測値。継続後の
    /// 打点で重み付けした集計値はこの段階では作らない。
    pub fn branch_remaining(&self) -> u32 {
        self.branches
            .iter()
            .map(|branch| u32::from(branch.remaining()))
            .sum()
    }
}

/// 成立した継続枝1件。
///
/// 枝の中身は既存 prospective diagnostic の物理牌 variant ([`ProspectiveDrawVariantValue`])
/// そのもので、この型はそれが「どの非和了ツモ牌種の枝か」を添えるだけ。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TenpaiContinuationBranch {
    /// 非和了ツモの牌種。
    pub draw: TileType,
    /// その牌種全体の残枚数。`variant` はこのうち赤5 / 黒5どちらか一方の枚数を持つ。
    pub draw_remaining: u8,
    /// 物理牌 variant 単位の枝。次打牌・将来打点は既存診断が持つ値そのもの。
    pub variant: ProspectiveDrawVariantValue,
    /// 継続後の terminal tenpai のツモ打点。既存2手先評価が枝に持っている値そのもの。
    ///
    /// ツモ打点を確定できない枝 (攻撃モードが [`TenpaiOffenseMode::Unknown`]・点数計算の入力
    /// 不足など) は `None`。0点として扱わない。
    pub tsumo_continuation: Option<TenpaiTsumoValue>,
}

impl TenpaiContinuationBranch {
    /// 仮想的にツモった物理牌。赤5と黒5は別の枝になる。
    pub fn drawn_tile(&self) -> TileId {
        self.variant.drawn_tile
    }

    /// この物理牌 variant の残枚数。見え牌を差し引いた枚数そのもの。
    pub fn remaining(&self) -> u8 {
        self.variant.remaining
    }

    /// 既存 selector が選んだ次打牌。ツモ切りで元の待ちを維持する枝ではツモ牌と同じ牌種になる。
    pub fn next_discard(&self) -> Option<TileType> {
        self.variant.next_discard
    }

    /// 継続後のテンパイを production のリーチ判断が評価した攻撃モード。
    ///
    /// 自分の席が分からず既リーチかどうかを判断できない枝は
    /// [`TenpaiOffenseMode::Unknown`]、テンパイを評価できなかった枝は `None`。
    pub fn offense_mode(&self) -> Option<TenpaiOffenseMode> {
        self.evaluated().map(|tenpai| tenpai.mode)
    }

    /// 継続後の待ちと残枚数。評価できなかった枝は空。
    pub fn waits(&self) -> &[ProspectiveWaitValue] {
        self.evaluated().map_or(&[], ProspectiveTenpaiValue::waits)
    }

    /// 継続後の待ちの残枚数合計。
    pub fn wait_remaining(&self) -> u32 {
        self.waits()
            .iter()
            .map(|wait| u32::from(wait.remaining))
            .sum()
    }

    /// 継続後の待ちの牌種数。
    pub fn wait_type_count(&self) -> usize {
        self.waits().len()
    }

    /// production の将来打点。確定できない枝は `None`。
    ///
    /// 既存の [`ProductionProspectiveValuator`](crate::prospective_value) が返した値そのもので、
    /// 表示や診断のために求め直さない。
    pub fn prospective_value(&self) -> Option<u64> {
        self.variant.selection_value
    }

    /// この非和了ツモを最初の1自摸で引く経路の期待ツモ支払い
    /// [[`SELF_TSUMO_VALUE_SCALE`](bot_logic::SELF_TSUMO_VALUE_SCALE)]。
    ///
    /// 経路確率も terminal tenpai の horizon も既存 [`SelfTsumoPath::immediate`] そのままで、
    /// この層は確率も期待支払いも組み立てない。terminal tenpai のツモ打点を確定できない枝と、
    /// 未確認牌が1枚も無く経路を作れない局面は 0 点にせず `None`。
    pub fn expected_self_tsumo_value(&self, facts: SelfTsumoFacts) -> Option<u64> {
        let terminal = self.tsumo_continuation?;
        let path = SelfTsumoPath::immediate(self.remaining(), facts.unknown_tiles)?;
        Some(path.expected_payment(facts, terminal))
    }

    fn evaluated(&self) -> Option<&ProspectiveTenpaiValue> {
        self.variant.outcome.evaluated()
    }
}

/// 現在聴牌1件分の「今すぐリーチ」と「ダマで1巡継続」の比較。
///
/// どちらも既存 self-tsumo 確率模型の期待ツモ支払い
/// [[`SELF_TSUMO_VALUE_SCALE`](bot_logic::SELF_TSUMO_VALUE_SCALE)] で、同じ `U0` / `n` から
/// 組み立てた同じ単位の値になる。どちらを選ぶかの結論 (winner / `should_reach`) はまだ持たない。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct TenpaiSelfTsumoComparison {
    /// 今すぐリーチして手変わりせず、残り自摸機会全体でツモ和了する期待支払い。
    ///
    /// 現在局面の合法手にリーチが無い場合と、ツモ打点を確定できない場合は `None`。
    pub reach_now: Option<u64>,
    /// ダマのまま最初の1自摸で現在の待ちをツモ和了する期待支払い。
    pub damaten_immediate_tsumo: Option<u64>,
    /// 非和了牌を引いて手変わりした先の terminal tenpai の期待支払い合計。
    ///
    /// 継続枝が1件も無い場合は寄与が無いので 0 で、枝の terminal ツモ打点を1つでも確定できない
    /// 場合は `None`。
    pub damaten_continuation_branches: Option<u64>,
}

impl TenpaiSelfTsumoComparison {
    /// ダマで1巡継続した場合の期待ツモ支払い合計。
    ///
    /// 「最初の1自摸で現在の待ちを引く枝」と「非和了牌を引いて手変わりする枝」の和で、どちらかを
    /// 確定できない場合は `None`。
    pub fn damaten_continuation(&self) -> Option<u64> {
        Some(
            self.damaten_immediate_tsumo?
                .saturating_add(self.damaten_continuation_branches?),
        )
    }
}

/// 現在聴牌の継続診断を構築するための材料。
///
/// どれも打牌選択が既に構築・使用した値そのもので、この診断のために作り直したものは無い。
/// `evaluations` / `lookahead` / `value` は同じ候補集合から作った同じ順序のものを渡す。
pub(crate) struct TenpaiContinuationInputs<'a> {
    pub context: &'a GameContext,
    /// 現在打牌前の全物理牌 (手牌 + ツモ牌)。現在聴牌の手牌を組み立てるために使う。
    pub tiles: &'a [TileId],
    /// 打牌選択が使ったものと同じ評価器。ツモ打点の baseline もモード判定もこれが持つ。
    pub valuator: &'a ProductionProspectiveValuator<'a>,
    /// 現在局面の合法手に [`LegalAction::Reach`](crate::action::LegalAction::Reach) があるか。
    ///
    /// production の現在リーチ判断と同じく、現在局面のリーチ可否は実際の合法手だけを source of
    /// truth にする。局面から合法条件を組み立て直さない。
    pub reach_legal: bool,
    /// self-tsumo 確率模型の事実。材料が揃わない局面では `None`。
    pub self_tsumo_facts: Option<SelfTsumoFacts>,
    pub evaluations: &'a [DiscardEvaluation],
    pub lookahead: &'a LookaheadDiagnostic,
    pub value: &'a ProspectiveLookaheadDiagnostic,
}

/// 構築済みの2手先評価とその将来打点から、現在聴牌候補の継続枝を絞り込む。
///
/// 枝の探索も打牌評価も待ち計算も行わず、既に構築済みの枝を選び直すだけ。self-tsumo 比較の
/// ための点数計算だけは、現在打牌後の聴牌が既存2手先評価の対象に無いため既存 Tsumo scoring
/// helper へ委ねる。確率模型も待ちも点数計算規則もこの層は持たない。候補の順序が対応しない
/// 場合は推測せず `None` にする。
///
/// 自分が未リーチと確定していない局面 (既リーチ・自分の席が不明) では `None`。
pub(crate) fn diagnose_tenpai_continuation(
    inputs: &TenpaiContinuationInputs,
) -> Option<TenpaiContinuationDiagnostic> {
    if inputs.context.own_reached() != Some(false) {
        return None;
    }
    if inputs.lookahead.candidates.len() != inputs.evaluations.len()
        || inputs.value.candidates.len() != inputs.evaluations.len()
    {
        return None;
    }

    Some(TenpaiContinuationDiagnostic {
        candidates: inputs
            .evaluations
            .iter()
            .zip(&inputs.lookahead.candidates)
            .zip(&inputs.value.candidates)
            .filter(|((evaluation, candidate), value)| {
                evaluation.min_shanten_after_discard() == TENPAI_SHANTEN
                    && candidate.discard == evaluation.discard
                    && value.discard == evaluation.discard
            })
            .map(|((evaluation, candidate), value)| {
                candidate_continuation(inputs, evaluation, candidate, value)
            })
            .collect(),
    })
}

// 現在聴牌候補1件分の継続枝。非和了ツモは既存分類 (same-shanten) そのもので、和了牌の枝
// (Progress) はここへ入らない。
fn candidate_continuation(
    inputs: &TenpaiContinuationInputs,
    evaluation: &DiscardEvaluation,
    candidate: &DiscardLookaheadDiagnostic,
    value: &ProspectiveDiscardValue,
) -> TenpaiContinuationCandidate {
    let branches: Vec<_> = candidate
        .draws
        .iter()
        .zip(&value.draws)
        .filter(|(draw, value)| {
            draw.transition == DrawTransition::SameShanten && draw.draw == value.draw
        })
        .flat_map(|(draw, value)| draw_branches(draw, value))
        .collect();

    TenpaiContinuationCandidate {
        discard: evaluation.discard,
        current_wait: evaluation.acceptance_after_discard.tiles.clone(),
        self_tsumo: candidate_self_tsumo(inputs, evaluation, &branches),
        branches,
    }
}

// 非和了ツモ1牌種分の枝。既存2手先評価が構造 (次打牌後が聴牌か) と terminal ツモ打点を、
// 既存将来打点が待ち・モード・ロン baseline の打点を持つ。
fn draw_branches<'a>(
    draw: &'a DrawLookaheadDiagnostic,
    value: &'a ProspectiveDrawValue,
) -> impl Iterator<Item = TenpaiContinuationBranch> + 'a {
    draw.variants
        .iter()
        .zip(&value.variants)
        .filter(|(variant, value)| {
            variant.drawn_tile == value.drawn_tile && continues_tenpai(variant)
        })
        .map(|(variant, value)| TenpaiContinuationBranch {
            draw: draw.draw,
            draw_remaining: draw.remaining,
            variant: value.clone(),
            tsumo_continuation: variant.tsumo_continuation,
        })
}

// 現在聴牌候補1件分の self-tsumo 比較。
//
// 確率模型の材料が揃わない局面ではどの値も持たない。現在聴牌の手牌を組み立てられない候補でも
// 継続枝側の集計は成立するので、確定できない値だけを `None` にする。
fn candidate_self_tsumo(
    inputs: &TenpaiContinuationInputs,
    evaluation: &DiscardEvaluation,
    branches: &[TenpaiContinuationBranch],
) -> TenpaiSelfTsumoComparison {
    let Some(facts) = inputs.self_tsumo_facts else {
        return TenpaiSelfTsumoComparison::default();
    };
    let current = current_tenpai_facts(inputs.valuator, inputs.tiles, evaluation);
    let expected_payment = |mode, own_draws| {
        baseline_expected_payment(inputs.valuator, current.as_ref()?, mode, own_draws, facts)
    };

    TenpaiSelfTsumoComparison {
        reach_now: inputs
            .reach_legal
            .then(|| expected_payment(TenpaiOffenseMode::Reach, facts.own_future_draws))
            .flatten(),
        damaten_immediate_tsumo: expected_payment(TenpaiOffenseMode::Damaten, FIRST_DRAW),
        damaten_continuation_branches: branch_total(branches, facts),
    }
}

// 現在打牌後の聴牌を、指定した攻撃モードの Tsumo baseline で `own_draws` 回以内に自摸和了する
// 期待支払いへ畳む。baseline も点数計算も集約も既存 Tsumo scoring helper が持つ。
fn baseline_expected_payment(
    valuator: &ProductionProspectiveValuator,
    current: &ProspectiveFacts,
    mode: TenpaiOffenseMode,
    own_draws: u32,
    facts: SelfTsumoFacts,
) -> Option<u64> {
    Some(
        valuator
            .tsumo_value_with_mode(current, mode)?
            .expected_payment(facts.unknown_tiles, own_draws),
    )
}

// 現在打牌後の聴牌1件分の評価材料。手牌の組み立てもフリテン判定も、既存2手先評価の将来テンパイ
// と同じ helper を通す。
fn current_tenpai_facts(
    valuator: &ProductionProspectiveValuator,
    tiles: &[TileId],
    evaluation: &DiscardEvaluation,
) -> Option<ProspectiveFacts> {
    let (discarded, concealed_tiles) = split_discarded_tile(tiles.to_vec(), evaluation)?;
    valuator.tenpai_facts(&ProspectiveTenpai {
        concealed_tiles: &concealed_tiles,
        acceptance: &evaluation.acceptance_after_discard,
        discarded_tiles: &[discarded],
    })
}

// 継続枝の期待支払い合計。1つでも確定できない枝があれば 0 点へ潰さず `None`。
fn branch_total(branches: &[TenpaiContinuationBranch], facts: SelfTsumoFacts) -> Option<u64> {
    branches.iter().try_fold(0, |total: u64, branch| {
        Some(total.saturating_add(branch.expected_self_tsumo_value(facts)?))
    })
}

// 最善打牌後が再び聴牌になった枝だけが継続成立。打牌候補が1件も無い枝と、聴牌に戻らない枝は
// 成立として扱わない。
fn continues_tenpai(variant: &DrawVariantLookaheadDiagnostic) -> bool {
    variant.next_min_shanten() == Some(TENPAI_SHANTEN)
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::sync::LazyLock;

    use bot_logic::HistoryFuritenFacts;

    use crate::action::LegalAction;
    use crate::context::TableStateFacts;
    use crate::discard_selection::{
        DiscardActionSelectionWithDiagnostic, LookaheadDiagnosticScope,
        select_discard_action_with_diagnostic,
    };
    use crate::reach_policy::{ReachLegalityFacts, is_reach_legal};

    // 123m 456m 789m 123p 東 の門前13枚に南をツモった単騎テンパイ。打 E で南単騎、打 S で東単騎
    // になり、どちらの現在聴牌からも、非和了ツモ1枚とその後の最善打牌でダマ継続できる。
    const HAND: [&str; 13] = [
        "1m", "2m", "3m", "4m", "5m", "6m", "7m", "8m", "9m", "1p", "2p", "3p", "E",
    ];
    const DRAW: &str = "S";

    // 345m 678m 789p 789s + 34s の実戦形 (5m は赤5)。打 3s で 4s 単騎テンパイになり、3m を
    // ツモって 4s を切ると 3m / 6m / 9m の三面待ちへ変わる。
    const REAL_HAND: [&str; 14] = [
        "3m", "4m", "5mr", "6m", "7m", "8m", "7p", "8p", "9p", "3s", "4s", "7s", "8s", "9s",
    ];

    // 山の残枚数。4人で分けて自分の残り自摸機会になる。
    const REMAINING_TILES: u32 = 70;

    // リーチ宣言の条件を満たす持ち点。
    const REACH_SCORE: i32 = 25_000;

    // 同じ牌種の赤5 / 黒5を取り違えないよう、物理牌を1枚ずつ払い出す。
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

    struct CaseSpec<'a> {
        /// 打牌前の手牌。ツモ牌を含まない13枚か、ツモ牌まで含んだ14枚。
        hand: &'a [&'a str],
        /// 直前のツモ牌。`hand` が14枚の局面では `None`。
        draw: Option<&'a str>,
        extra_visible: &'a [&'a str],
        own_reached: bool,
        /// 自分の席。`None` では既リーチかどうかを判断できない。
        player_id: Option<u8>,
        /// 山の残枚数。`None` では self-tsumo 確率模型の材料が揃わない。
        remaining_tiles: Option<u32>,
        /// 全員の持ち点。`None` では持ち点が分からない。
        scores: Option<[i32; 4]>,
        /// 合法手にリーチを含めるか。現在局面のリーチ可否はこの合法手だけが source of truth。
        legal_reach: bool,
        scope: LookaheadDiagnosticScope,
    }

    impl Default for CaseSpec<'_> {
        fn default() -> Self {
            Self {
                hand: &HAND,
                draw: Some(DRAW),
                extra_visible: &[],
                own_reached: false,
                player_id: Some(0),
                remaining_tiles: None,
                scores: None,
                legal_reach: false,
                scope: LookaheadDiagnosticScope::Lookahead,
            }
        }
    }

    // 局面と、その局面で打牌評価の対象になる物理牌。診断が使うものと同じ材料をテストからも
    // 組み立てられるようにする。
    struct CaseContext {
        context: GameContext,
        tiles: Vec<TileId>,
        actions: Vec<LegalAction>,
    }

    impl CaseSpec<'_> {
        fn build(&self) -> DiscardActionSelectionWithDiagnostic {
            let case = self.context();
            select_discard_action_with_diagnostic(&case.context, &case.actions, self.scope)
        }

        fn context(&self) -> CaseContext {
            let mut source = TileIdSource::new();
            let hand_tiles = source.tiles(self.hand);
            let drawn_tile = self.draw.map(|draw| source.tile(draw));
            let extra_visible = source.tiles(self.extra_visible);

            let tiles: Vec<TileId> = hand_tiles.iter().copied().chain(drawn_tile).collect();
            let visible: Vec<TileId> = tiles.iter().chain(extra_visible.iter()).copied().collect();
            let actions: Vec<LegalAction> = tiles
                .iter()
                .map(|&tile| LegalAction::Dahai { tile })
                .chain(self.legal_reach.then_some(LegalAction::Reach))
                .collect();

            let mut reached = [false; 4];
            if let Some(player_id) = self.player_id {
                reached[usize::from(player_id)] = self.own_reached;
            }

            let context = GameContext::from_parts_with_table_state(
                drawn_tile,
                hand_tiles,
                Vec::new(),
                Some(tile("E")),
                Some(tile("S")),
                visible,
                self.player_id,
                Some(3),
                Default::default(),
                reached,
            )
            .with_table_state_facts(TableStateFacts {
                remaining_tiles: self.remaining_tiles,
                scores: self.scores,
                ..Default::default()
            })
            .with_history_furiten_facts(HistoryFuritenFacts {
                same_turn: Some(false),
                riichi_missed_win: Some(false),
            });

            CaseContext {
                context,
                tiles,
                actions,
            }
        }
    }

    // self-tsumo 比較の材料が揃う局面。山の残枚数と持ち点を既知にし、合法手にもリーチを含める。
    fn self_tsumo_spec() -> CaseSpec<'static> {
        CaseSpec {
            remaining_tiles: Some(REMAINING_TILES),
            scores: Some([REACH_SCORE; 4]),
            legal_reach: true,
            ..CaseSpec::default()
        }
    }

    // 打 3s で 4s 単騎になる実戦形。self-tsumo 比較の材料まで揃える。
    fn real_spec() -> CaseSpec<'static> {
        CaseSpec {
            hand: &REAL_HAND,
            draw: None,
            ..self_tsumo_spec()
        }
    }

    // 2手先探索は重いので、同じ局面を使う複数のテストで構築結果を共有する。
    static CASE: LazyLock<DiscardActionSelectionWithDiagnostic> =
        LazyLock::new(|| CaseSpec::default().build());
    static NO_LOOKAHEAD: LazyLock<DiscardActionSelectionWithDiagnostic> = LazyLock::new(|| {
        CaseSpec {
            scope: LookaheadDiagnosticScope::None,
            ..CaseSpec::default()
        }
        .build()
    });
    static SELF_TSUMO_CASE: LazyLock<DiscardActionSelectionWithDiagnostic> =
        LazyLock::new(|| self_tsumo_spec().build());
    static REAL_CASE: LazyLock<DiscardActionSelectionWithDiagnostic> =
        LazyLock::new(|| real_spec().build());

    fn continuation(
        selection: &DiscardActionSelectionWithDiagnostic,
    ) -> &TenpaiContinuationDiagnostic {
        selection
            .tenpai_continuation
            .as_ref()
            .expect("継続診断が構築されている")
    }

    // 打 E (南単騎テンパイ) の継続枝。
    fn discard_east(
        selection: &DiscardActionSelectionWithDiagnostic,
    ) -> &TenpaiContinuationCandidate {
        continuation(selection)
            .candidate(tile("E"))
            .expect("打 E の現在聴牌候補がある")
    }

    fn branch<'a>(
        candidate: &'a TenpaiContinuationCandidate,
        drawn: &str,
    ) -> &'a TenpaiContinuationBranch {
        let drawn_tile = tile(drawn);
        let red = drawn.ends_with('r');
        candidate
            .branches
            .iter()
            .find(|branch| {
                branch.drawn_tile().tile_type() == drawn_tile && branch.drawn_tile().is_red() == red
            })
            .unwrap_or_else(|| panic!("{drawn} の継続枝がある"))
    }

    fn wait_tiles(branch: &TenpaiContinuationBranch) -> Vec<TileType> {
        branch
            .waits()
            .iter()
            .map(|wait| wait.winning_tile)
            .collect()
    }

    // self-tsumo 比較に使われた確率模型の事実。打牌選択が使った値そのもの。
    fn self_tsumo_facts(selection: &DiscardActionSelectionWithDiagnostic) -> SelfTsumoFacts {
        selection
            .self_tsumo_facts
            .expect("self-tsumo 確率模型の材料が揃っている")
    }

    // 打牌候補1件の評価。打牌選択が実際に使った (合法 Dahai へ絞り込み・物理牌補正済みの)
    // 評価そのものを診断から取り出す。
    fn evaluation_of<'a>(
        selection: &'a DiscardActionSelectionWithDiagnostic,
        discard: &str,
    ) -> &'a DiscardEvaluation {
        selection
            .diagnostic
            .candidates
            .iter()
            .map(|candidate| &candidate.evaluation)
            .find(|evaluation| evaluation.discard == tile(discard))
            .unwrap_or_else(|| panic!("打 {discard} の評価がある"))
    }

    // 現在打牌後の聴牌を、指定した攻撃モードの Tsumo baseline で評価したツモ打点。テスト側でも
    // 診断と同じ既存 helper だけを通し、確率も点数計算も書き直さない。
    fn current_tsumo_value(
        case: &CaseContext,
        selection: &DiscardActionSelectionWithDiagnostic,
        discard: &str,
        mode: TenpaiOffenseMode,
    ) -> TenpaiTsumoValue {
        let valuator = ProductionProspectiveValuator::new(&case.context);
        let facts = current_tenpai_facts(&valuator, &case.tiles, evaluation_of(selection, discard))
            .expect("現在聴牌を組み立てられる");
        valuator
            .tsumo_value_with_mode(&facts, mode)
            .expect("ツモ打点を確定できる")
    }

    // 継続枝の terminal tenpai を、既存2手先評価と同じ手順で組み立て直した評価材料。物理牌の
    // 出し入れだけを追い、待ちも点数計算もこの helper は行わない。
    fn branch_tenpai_facts(
        valuator: &ProductionProspectiveValuator,
        case: &CaseContext,
        evaluation: &DiscardEvaluation,
        variant: &DrawVariantLookaheadDiagnostic,
    ) -> ProspectiveFacts {
        let (first, after_first) =
            split_discarded_tile(case.tiles.clone(), evaluation).expect("現在打牌を切れる");
        let mut after_draw = after_first;
        after_draw.push(variant.drawn_tile);
        let next = variant.next_discard.as_ref().expect("次打牌がある");
        let (second, concealed_tiles) =
            split_discarded_tile(after_draw, next).expect("次打牌を切れる");

        valuator
            .tenpai_facts(&ProspectiveTenpai {
                concealed_tiles: &concealed_tiles,
                acceptance: &next.acceptance_after_discard,
                discarded_tiles: &[first, second],
            })
            .expect("継続後の聴牌を組み立てられる")
    }

    // 打 3s (4s 単騎テンパイ) の継続枝。
    fn discard_three_sou(
        selection: &DiscardActionSelectionWithDiagnostic,
    ) -> &TenpaiContinuationCandidate {
        continuation(selection)
            .candidate(tile("3s"))
            .expect("打 3s の現在聴牌候補がある")
    }

    #[test]
    fn only_current_tenpai_candidates_are_searched() {
        let selection = &*CASE;
        let discards: Vec<TileType> = continuation(selection)
            .candidates
            .iter()
            .map(|candidate| candidate.discard)
            .collect();

        // 打 E / 打 S だけが聴牌で、他の打牌は1向聴以上。
        assert_eq!(discards, vec![tile("E"), tile("S")]);
    }

    #[test]
    fn non_winning_draw_reaches_a_new_tenpai() {
        let selection = &*CASE;
        let candidate = discard_east(selection);

        // 現在は南単騎。1m を引くと南を切って三面張へ待ちが変わる。
        assert_eq!(
            candidate
                .current_wait
                .iter()
                .map(|wait| wait.tile)
                .collect::<Vec<_>>(),
            vec![tile("S")]
        );
        assert_eq!(candidate.current_wait_remaining(), 3);

        let branch = branch(candidate, "1m");
        assert_eq!(branch.next_discard(), Some(tile("S")));
        assert_eq!(wait_tiles(branch), vec![tile("1m"), tile("4m"), tile("7m")]);
        assert_eq!(branch.wait_type_count(), 3);
        assert_eq!(branch.wait_remaining(), 8);
        assert_eq!(branch.offense_mode(), Some(TenpaiOffenseMode::Reach));
        assert!(branch.prospective_value().is_some());
    }

    #[test]
    fn tsumogiri_that_holds_the_wait_is_a_continuation_branch() {
        // ツモ切りで元の待ちを維持する枝も継続枝に含める。「今すぐリーチ」と「ダマ継続」を
        // 比べるには待ちが変わらない場合の価値も必要なので、待ちが変わる枝だけへは絞らない。
        let candidate = discard_east(&CASE);
        let branch = branch(candidate, "2m");

        assert_eq!(branch.next_discard(), Some(tile("2m")));
        assert_eq!(wait_tiles(branch), vec![tile("S")]);
        assert_eq!(branch.wait_remaining(), candidate.current_wait_remaining());
    }

    #[test]
    fn current_winning_tile_is_not_a_continuation_branch() {
        let selection = &*CASE;
        let candidate = discard_east(selection);

        // 南は現在の和了牌なので、継続枝には現れない。
        assert!(
            !candidate
                .branches
                .iter()
                .any(|branch| branch.draw == tile("S"))
        );
        // 既存2手先評価では向聴数を下げる枝 (Progress) として残っている。
        let lookahead = selection.lookahead.as_ref().expect("2手先診断がある");
        let draw = lookahead
            .candidate(tile("E"))
            .expect("打 E の2手先評価がある")
            .draw(tile("S"))
            .expect("南の枝がある");
        assert_eq!(draw.transition, DrawTransition::Progress);
    }

    #[test]
    fn every_branch_returns_to_tenpai_after_the_next_discard() {
        let selection = &*CASE;
        let lookahead = selection.lookahead.as_ref().expect("2手先診断がある");

        for candidate in &continuation(selection).candidates {
            let draws = lookahead
                .candidate(candidate.discard)
                .expect("同じ候補の2手先評価がある");
            for branch in &candidate.branches {
                let variant = draws
                    .draw(branch.draw)
                    .expect("同じ牌種の枝がある")
                    .variant(branch.drawn_tile())
                    .expect("同じ物理牌の枝がある");
                assert_eq!(variant.next_min_shanten(), Some(TENPAI_SHANTEN));
            }
        }
    }

    #[test]
    fn seen_tiles_reduce_the_draw_variant_remaining() {
        let visible = CaseSpec {
            extra_visible: &["1m", "1m"],
            ..CaseSpec::default()
        }
        .build();

        // 手牌の1枚に加えて2枚が見えているので、1m の継続枝は残り1枚になる。
        assert_eq!(branch(discard_east(&CASE), "1m").remaining(), 3);
        let seen = branch(discard_east(&visible), "1m");
        assert_eq!(seen.remaining(), 1);
        assert_eq!(seen.draw_remaining, 1);
    }

    #[test]
    fn all_seen_draws_have_no_branch() {
        let visible = CaseSpec {
            extra_visible: &["1m", "1m", "1m"],
            ..CaseSpec::default()
        }
        .build();

        assert!(
            !discard_east(&visible)
                .branches
                .iter()
                .any(|branch| branch.draw == tile("1m"))
        );
    }

    #[test]
    fn red_and_black_five_stay_separate_branches() {
        let selection = &*CASE;
        let candidate = discard_east(selection);

        let red = branch(candidate, "5mr");
        let black = branch(candidate, "5m");
        assert_eq!(red.remaining(), 1);
        assert_eq!(black.remaining(), 2);
        assert_eq!(red.draw_remaining, 3);
        assert_eq!(black.draw_remaining, 3);

        // 待ちは同じでも、赤5を手牌へ残す枝の方が打点が高い。
        assert_eq!(wait_tiles(red), wait_tiles(black));
        assert!(red.prospective_value() > black.prospective_value());
    }

    #[test]
    fn branches_reuse_the_existing_lookahead_next_discard() {
        let selection = &*CASE;
        let lookahead = selection.lookahead.as_ref().expect("2手先診断がある");

        for candidate in &continuation(selection).candidates {
            let draws = lookahead
                .candidate(candidate.discard)
                .expect("同じ候補の2手先評価がある");
            for branch in &candidate.branches {
                let draw = draws.draw(branch.draw).expect("同じ牌種の枝がある");
                let variant = draw
                    .variant(branch.drawn_tile())
                    .expect("同じ物理牌の枝がある");

                assert_eq!(draw.transition, DrawTransition::SameShanten);
                assert_eq!(branch.draw_remaining, draw.remaining);
                assert_eq!(branch.remaining(), variant.remaining);
                assert_eq!(branch.next_discard(), variant.next_discard_tile());
                assert_eq!(branch.prospective_value(), variant.prospective_value);
            }
        }
    }

    #[test]
    fn branches_reuse_the_existing_prospective_value() {
        let selection = &*CASE;
        let value = selection.lookahead_value.as_ref().expect("将来打点がある");

        for candidate in &continuation(selection).candidates {
            let draws = value
                .candidate(candidate.discard)
                .expect("同じ候補の将来打点がある");
            for branch in &candidate.branches {
                let variant = draws
                    .draw(branch.draw)
                    .expect("同じ牌種の枝がある")
                    .variant(branch.drawn_tile())
                    .expect("同じ物理牌の枝がある");
                assert_eq!(&branch.variant, variant);
            }
        }
    }

    #[test]
    fn reached_hand_is_not_searched() {
        let reached = CaseSpec {
            own_reached: true,
            ..CaseSpec::default()
        }
        .build();

        assert!(reached.tenpai_continuation.is_none());
        // 2手先診断そのものは既存どおり構築する。継続診断だけを行わない。
        assert!(reached.lookahead.is_some());
    }

    #[test]
    fn unknown_own_reach_is_not_assumed_to_be_not_reached() {
        let unknown_seat = CaseSpec {
            player_id: None,
            ..CaseSpec::default()
        }
        .build();

        assert!(unknown_seat.tenpai_continuation.is_none());
    }

    #[test]
    fn continuation_is_not_searched_without_lookahead() {
        assert!(NO_LOOKAHEAD.tenpai_continuation.is_none());
    }

    #[test]
    fn continuation_does_not_change_the_normal_selection() {
        assert_eq!(CASE.selection, NO_LOOKAHEAD.selection);
    }

    // ---- self-tsumo 比較 ----

    #[test]
    fn reach_now_uses_the_forced_reach_tsumo_baseline_of_the_current_tenpai() {
        let selection = &*SELF_TSUMO_CASE;
        let case = self_tsumo_spec().context();
        let facts = self_tsumo_facts(selection);
        let reach = current_tsumo_value(&case, selection, "E", TenpaiOffenseMode::Reach);

        // production が現在ダマを選ぶかどうかに依らず、比較対象は forced Reach baseline。
        assert_eq!(
            discard_east(selection).self_tsumo.reach_now,
            Some(reach.expected_payment(facts.unknown_tiles, facts.own_future_draws))
        );

        // ダマ baseline とは別の baseline で、リーチ1翻の分だけツモ打点が高い。
        let damaten = current_tsumo_value(&case, selection, "E", TenpaiOffenseMode::Damaten);
        assert_eq!(reach.winning_remaining, damaten.winning_remaining);
        assert!(reach.weighted_total > damaten.weighted_total);
    }

    #[test]
    fn reach_now_evaluates_every_remaining_own_draw() {
        let selection = &*SELF_TSUMO_CASE;
        let case = self_tsumo_spec().context();
        let facts = self_tsumo_facts(selection);
        let reach = current_tsumo_value(&case, selection, "E", TenpaiOffenseMode::Reach);
        let unknown = facts.unknown_tiles;

        // 手変わりしないまま残り自摸機会全体を既存閉形式で評価した値そのもの。
        assert!(facts.own_future_draws > FIRST_DRAW);
        assert_eq!(
            discard_east(selection).self_tsumo.reach_now,
            Some(reach.expected_payment(unknown, facts.own_future_draws))
        );
        assert!(
            reach.expected_payment(unknown, facts.own_future_draws)
                > reach.expected_payment(unknown, facts.own_future_draws - 1)
        );
        assert!(
            reach.expected_payment(unknown, facts.own_future_draws)
                > reach.expected_payment(unknown, FIRST_DRAW)
        );
    }

    #[test]
    fn the_damaten_continuation_includes_an_immediate_tsumo_of_the_current_wait() {
        let selection = &*SELF_TSUMO_CASE;
        let case = self_tsumo_spec().context();
        let facts = self_tsumo_facts(selection);
        let damaten = current_tsumo_value(&case, selection, "E", TenpaiOffenseMode::Damaten);
        let comparison = discard_east(selection).self_tsumo;

        // 現在の待ちをダマのまま引く枝は、手変わりする前の最初の1自摸だけ。
        assert_eq!(
            comparison.damaten_immediate_tsumo,
            Some(damaten.expected_payment(facts.unknown_tiles, FIRST_DRAW))
        );
        assert!(comparison.damaten_immediate_tsumo > Some(0));
        assert!(
            comparison.damaten_immediate_tsumo
                < Some(damaten.expected_payment(facts.unknown_tiles, facts.own_future_draws))
        );
    }

    #[test]
    fn the_current_wait_is_not_counted_twice_in_the_continuation_branches() {
        let selection = &*SELF_TSUMO_CASE;
        let candidate = discard_east(selection);
        let comparison = candidate.self_tsumo;

        // 現在の和了牌 (南) は即ツモ枝としてだけ数え、継続枝には現れない。
        assert!(
            !candidate
                .branches
                .iter()
                .any(|branch| branch.draw == tile("S"))
        );
        assert_eq!(
            comparison.damaten_continuation(),
            Some(
                comparison
                    .damaten_immediate_tsumo
                    .expect("即ツモ枝を確定できる")
                    + comparison
                        .damaten_continuation_branches
                        .expect("手変わり枝を確定できる")
            )
        );
    }

    #[test]
    fn a_non_winning_draw_branch_uses_the_immediate_path_probability() {
        let selection = &*SELF_TSUMO_CASE;
        let facts = self_tsumo_facts(selection);
        let candidate = discard_east(selection);

        let mut total = 0u64;
        for branch in &candidate.branches {
            let path = SelfTsumoPath::immediate(branch.remaining(), facts.unknown_tiles)
                .expect("経路を作れる");
            let terminal = branch
                .tsumo_continuation
                .expect("terminal ツモ打点を確定できる");
            let value = branch.expected_self_tsumo_value(facts);

            assert_eq!(value, Some(path.expected_payment(facts, terminal)));
            total += value.expect("枝の期待支払いを確定できる");
        }

        // 集計値は枝の期待支払いの単純な合計で、係数も正規化も入らない。
        assert_eq!(
            candidate.self_tsumo.damaten_continuation_branches,
            Some(total)
        );
    }

    #[test]
    fn a_continuation_branch_consumes_one_unknown_tile_and_one_own_draw() {
        let selection = &*SELF_TSUMO_CASE;
        let facts = self_tsumo_facts(selection);
        let branch = branch(discard_east(selection), "1m");
        let path = SelfTsumoPath::immediate(branch.remaining(), facts.unknown_tiles)
            .expect("経路を作れる");

        // 継続後の terminal tenpai は U0 - 1 / n - 1 で、既存 SelfTsumoPath の semantics そのもの。
        assert_eq!(
            path.terminal_unknown_tiles(facts),
            facts.unknown_tiles - FIRST_DRAW
        );
        assert_eq!(
            path.terminal_own_future_draws(facts),
            facts.own_future_draws - FIRST_DRAW
        );
        assert_eq!(
            branch.expected_self_tsumo_value(facts),
            Some(path.expected_payment(
                facts,
                branch.tsumo_continuation.expect("terminal ツモ打点がある")
            ))
        );
    }

    #[test]
    fn the_tsumogiri_branch_that_holds_the_wait_contributes_to_the_aggregate() {
        let selection = &*SELF_TSUMO_CASE;
        let facts = self_tsumo_facts(selection);
        let candidate = discard_east(selection);
        let branch = branch(candidate, "2m");

        // ツモ切りで南単騎のまま継続する枝も、待ちが変わる枝と同じ集計に入る。
        assert_eq!(branch.next_discard(), Some(tile("2m")));
        assert_eq!(wait_tiles(branch), vec![tile("S")]);
        let value = branch
            .expected_self_tsumo_value(facts)
            .expect("枝の期待支払いを確定できる");
        assert!(value > 0);
        assert!(candidate.self_tsumo.damaten_continuation_branches >= Some(value));
    }

    #[test]
    fn a_wait_improving_branch_contributes_to_the_damaten_continuation() {
        let selection = &*REAL_CASE;
        let facts = self_tsumo_facts(selection);
        let candidate = discard_three_sou(selection);
        let branch = branch(candidate, "3m");

        // 4s 単騎から 3m ツモ → 打 4s で 3m / 6m / 9m の9枚待ちへ変わる枝。
        assert_eq!(candidate.current_wait_remaining(), 3);
        assert_eq!(branch.next_discard(), Some(tile("4s")));
        assert_eq!(wait_tiles(branch), vec![tile("3m"), tile("6m"), tile("9m")]);
        assert_eq!(branch.wait_type_count(), 3);
        assert_eq!(branch.wait_remaining(), 9);

        let value = branch
            .expected_self_tsumo_value(facts)
            .expect("枝の期待支払いを確定できる");
        assert!(value > 0);
        assert!(candidate.self_tsumo.damaten_continuation_branches >= Some(value));
    }

    #[test]
    fn red_and_black_five_branches_keep_their_own_tsumo_value() {
        let selection = &*SELF_TSUMO_CASE;
        let facts = self_tsumo_facts(selection);
        let candidate = discard_east(selection);
        let red = branch(candidate, "5mr");
        let black = branch(candidate, "5m");

        // 待ちは同じでも、赤5を手牌へ残す枝の方が継続後のツモ打点が高い。
        let red_terminal = red.tsumo_continuation.expect("terminal ツモ打点がある");
        let black_terminal = black.tsumo_continuation.expect("terminal ツモ打点がある");
        assert_eq!(wait_tiles(red), wait_tiles(black));
        assert_eq!(
            red_terminal.winning_remaining,
            black_terminal.winning_remaining
        );
        assert!(red_terminal.weighted_total > black_terminal.weighted_total);

        // 経路確率は物理牌 variant ごとの残枚数のままで、牌種の残枚数へ潰さない。
        assert_eq!((red.remaining(), black.remaining()), (1, 2));
        assert_eq!(red.draw_remaining, black.draw_remaining);
        for branch in [red, black] {
            let path = SelfTsumoPath::immediate(branch.remaining(), facts.unknown_tiles)
                .expect("経路を作れる");
            assert_eq!(
                branch.expected_self_tsumo_value(facts),
                Some(path.expected_payment(
                    facts,
                    branch.tsumo_continuation.expect("terminal ツモ打点がある")
                ))
            );
        }
    }

    #[test]
    fn a_continuation_branch_keeps_the_mode_of_the_existing_prospective_evaluation() {
        let selection = &*REAL_CASE;
        let case = real_spec().context();
        let valuator = ProductionProspectiveValuator::new(&case.context);
        let evaluation = evaluation_of(selection, "3s");
        let lookahead = selection.lookahead.as_ref().expect("2手先診断がある");
        let candidate = discard_three_sou(selection);

        for branch in &candidate.branches {
            let variant = lookahead
                .candidate(candidate.discard)
                .expect("同じ候補の2手先評価がある")
                .draw(branch.draw)
                .expect("同じ牌種の枝がある")
                .variant(branch.drawn_tile())
                .expect("同じ物理牌の枝がある");
            let mode = branch.offense_mode().expect("攻撃モードが確定している");
            let facts = branch_tenpai_facts(&valuator, &case, evaluation, variant);

            // 継続後のツモ打点は、既存 prospective evaluation が決めたモードの baseline のまま。
            assert_eq!(branch.tsumo_continuation, variant.tsumo_continuation);
            assert_eq!(
                branch.tsumo_continuation,
                valuator.tsumo_value_with_mode(&facts, mode)
            );

            // モードを取り違えれば別の baseline になり、値も変わる。
            let other = match mode {
                TenpaiOffenseMode::Reach => TenpaiOffenseMode::Damaten,
                TenpaiOffenseMode::Damaten | TenpaiOffenseMode::Unknown => TenpaiOffenseMode::Reach,
            };
            assert_ne!(
                branch.tsumo_continuation,
                valuator.tsumo_value_with_mode(&facts, other)
            );
        }
    }

    #[test]
    fn reach_now_follows_the_actual_legal_reach_action() {
        // 現在局面のリーチ可否は production のリーチ判断と同じく、実際の合法手だけが source of
        // truth。局面の条件をすべて満たしていても、合法手にリーチが無ければ値を作らない。
        assert!(is_reach_legal(ReachLegalityFacts {
            menzen: Some(true),
            already_reached: Some(false),
            score: Some(REACH_SCORE),
            remaining_tiles: Some(REMAINING_TILES),
            tenpai_after_discard: true,
        }));

        let without_reach = CaseSpec {
            legal_reach: false,
            ..self_tsumo_spec()
        }
        .build();
        let comparison = discard_east(&without_reach).self_tsumo;
        assert_eq!(comparison.reach_now, None);
        // ダマ側は同じ材料のまま確定する。
        assert!(comparison.damaten_continuation().is_some());

        // 同じ局面へ合法手のリーチを足した場合だけ、forced Reach Tsumo baseline の値になる。
        let selection = &*SELF_TSUMO_CASE;
        let case = self_tsumo_spec().context();
        let facts = self_tsumo_facts(selection);
        let reach = current_tsumo_value(&case, selection, "E", TenpaiOffenseMode::Reach);
        assert_eq!(
            discard_east(selection).self_tsumo.reach_now,
            Some(reach.expected_payment(facts.unknown_tiles, facts.own_future_draws))
        );
    }

    #[test]
    fn unknown_remaining_tiles_do_not_produce_a_guessed_value() {
        // 山の残枚数が分からない局面では自摸機会を推測せず、どの集計値も持たない。
        let comparison = discard_east(&CASE).self_tsumo;

        assert_eq!(CASE.self_tsumo_facts, None);
        assert_eq!(comparison.reach_now, None);
        assert_eq!(comparison.damaten_immediate_tsumo, None);
        assert_eq!(comparison.damaten_continuation_branches, None);
        assert_eq!(comparison.damaten_continuation(), None);
    }

    #[test]
    fn the_self_tsumo_comparison_does_not_change_the_normal_selection() {
        assert_eq!(
            SELF_TSUMO_CASE.selection,
            CaseSpec {
                scope: LookaheadDiagnosticScope::None,
                ..self_tsumo_spec()
            }
            .build()
            .selection
        );
    }
}
