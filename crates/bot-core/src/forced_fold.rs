//! 押し引き判断とは独立に、ベタ降りを仮定した場合の防御打牌を評価する hypothetical 入口。
//!
//! production の打牌選択には接続しない。`ShantenAgent::act()` / `diagnose*()` はこの module を
//! 呼ばず、ここでの評価も production decision を書き換えない。
//!
//! routing も防御選択も既存 [`evaluate_fold_defense`] をそのまま source of truth にし、threat の
//! 有無も既存 [`has_clear_threat`] と同じ classification を使う。forced fold のために防御ロジックも
//! Caution / Danger 条件も書き直さない。
//!
//! 候補 ranking の基礎も既存 production defense policy の ordering をそのまま使い、選択・
//! ranking・詳細診断は1回の evaluation から得た同じ candidate evidence を共有する。そのうえで
//! forced fold だけは、手牌内の同一牌枚数を織り込んだ [`effective_fold_risk`] で exact 段を
//! 並べ替える。1枚目が通った後の同一牌の継続価値を近似する ranking 用 heuristic で、Reach /
//! OpenHand / Combined のどの exact 段にも同じように適用する。production defense policy 側の
//! ordering は書き換えないので、ベタ降り以外の判断には影響しない。

mod ranking;

use crate::action::LegalAction;
use crate::combined_defense::{
    CombinedDefenseCategory, CombinedDefenseDiagnostic,
    collect_combined_candidate_ron_risk_evidence, combined_threat_defense_targets,
    ordered_combined_defense_candidates,
};
use crate::context::GameContext;
use crate::defense::{
    DahaiRonRiskVector, DefenseDecisionDiagnostic, DefenseFallbackKind,
    ordered_defense_fallback_candidates, player_ron_risk_evidence_for_action,
};
use crate::fold_defense::{FoldDefenseEvaluation, FoldDefenseKind, evaluate_fold_defense};
use crate::open_hand_defense::{
    OpenHandDefenseCategory, OpenHandDefenseDiagnostic, actionable_open_hand_threat_players,
    collect_open_hand_candidate_ron_risk_evidence, ordered_open_hand_defense_candidates,
};
use crate::push_pull::{has_clear_threat, push_pull_inputs_from_threat_facts};
use crate::threat::player_threat_facts_from_context;
use bot_logic::TileType;

use ranking::rerank_by_effective_fold_risk;

pub use ranking::{ForcedFoldRankedCandidate, effective_fold_risk};

/// forced fold が選んだ defense family と、その family 内の選択種別。
///
/// 既存 Fold defense の routing 結果そのもので、forced fold 用の分類を別に持たない。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ForcedFoldDefenseKind {
    Reach(DefenseFallbackKind),
    OpenHand(OpenHandDefenseCategory),
    Combined(CombinedDefenseCategory),
}

/// forced fold を評価できなかった理由。
///
/// どちらの場合も通常打牌を「ベタ降り最善打牌」として返さない。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ForcedFoldUnavailable {
    /// リーチ者も actionable OpenHandThreat の相手もいない。防御対象がないので評価しない。
    NoClearThreat,
    /// 防御対象はいるが、既存 evaluator が防御打牌を選べなかった。合法 Dahai がない局面を含む。
    NoDefenseSelection,
}

/// forced fold の評価結果。
///
/// `defense_kind` は既存 Fold defense evaluator の routing 結果そのもので、`ranked_candidates` と
/// 防御診断は選択に使った同じ evaluation から構築する。`selected_action` は forced fold ranking の
/// 先頭で、exact 段が [`effective_fold_risk`] で並ぶぶんだけ production selection と異なり得る。
/// 診断は防御 family に対応するものだけが `Some` になる。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ForcedFoldDiagnostic {
    pub selected_action: LegalAction,
    pub defense_kind: ForcedFoldDefenseKind,
    /// 全合法 Dahai を forced fold ordering どおりに並べた防御候補。
    ///
    /// 先頭が `selected_action` と一致するのは forced fold の選択がこの ranking の先頭そのもの
    /// だからで、rank 1 用の特別処理は持たない。
    pub ranked_candidates: Vec<ForcedFoldRankedCandidate>,
    pub defense: Option<DefenseDecisionDiagnostic>,
    pub open_hand_defense: Option<OpenHandDefenseDiagnostic>,
    pub combined_defense: Option<CombinedDefenseDiagnostic>,
}

/// 通常の押し引き判断とは無関係に、ベタ降りすると仮定した場合の防御打牌を評価する。
///
/// 通常打牌選択・2手先探索・Reach / Damaten・押し引き判定はどれも走らせない。threat facts は
/// 押し引きが使うものと同じ構築経路から攻撃評価なしで作り、classification を別実装しない。
///
/// 評価は1回だけで、`ranked_candidates` と詳細診断は同じ evaluation の candidate evidence を
/// 共有する。exact model を ranking 用に二重計算しない。
///
/// OpenHand / 複合 threat 防御が hard-safe / same-hand passed で決着した局面では、既存
/// evaluator が exact model を走らせないまま早期 return する。その場合だけ、選択を変えずに
/// 診断用の candidate exact evidence を追加収集する。Reach 防御が共通現物で決着した後も exact
/// candidate evidence を収集するのと同じ考え方で、production selection は変わらない。
///
/// `selected_action` は forced fold ranking の先頭にする。exact 段だけは手牌内の同一牌枚数を
/// 織り込んだ [`effective_fold_risk`] で並ぶので、同じ牌を複数枚持つ局面では production
/// selection と異なる牌になり得る。production defense policy 側の選択は書き換えない。
/// [`effective_fold_risk`] は ranking 用の heuristic で、実際の放銃確率でも、通過後の safety の
/// 寿命を target 種別ごとに厳密にモデル化した値でもない。
pub fn evaluate_forced_fold(
    context: &GameContext,
    legal_actions: &[LegalAction],
) -> Result<ForcedFoldDiagnostic, ForcedFoldUnavailable> {
    let inputs = push_pull_inputs_from_threat_facts(
        context,
        player_threat_facts_from_context(context),
        None,
        None,
        None,
        None,
        legal_actions,
    );

    if !has_clear_threat(&inputs) {
        return Err(ForcedFoldUnavailable::NoClearThreat);
    }

    let mut evaluation = evaluate_fold_defense(context, legal_actions, &inputs, true);
    let selection = evaluation
        .selected()
        .ok_or(ForcedFoldUnavailable::NoDefenseSelection)?;

    let defense_kind = match selection.kind {
        FoldDefenseKind::Reach(kind) => ForcedFoldDefenseKind::Reach(kind),
        FoldDefenseKind::OpenHand(category) => ForcedFoldDefenseKind::OpenHand(category),
        FoldDefenseKind::Combined(category) => ForcedFoldDefenseKind::Combined(category),
    };

    let mut diagnostic = ForcedFoldDiagnostic {
        selected_action: selection.action.clone(),
        defense_kind,
        ranked_candidates: Vec::new(),
        defense: None,
        open_hand_defense: None,
        combined_defense: None,
    };

    match &mut evaluation {
        FoldDefenseEvaluation::Reach(evaluation) => {
            let ron_risk_vectors = evaluation.ron_risk_vectors.as_deref();
            diagnostic.ranked_candidates = ranked_candidates(
                context,
                ordered_defense_fallback_candidates(context, legal_actions, ron_risk_vectors),
                ron_risk_vectors,
                ForcedFoldDefenseKind::Reach,
            );
            diagnostic.defense = Some(DefenseDecisionDiagnostic::from_evaluation(
                context,
                legal_actions,
                evaluation,
            ));
        }
        FoldDefenseEvaluation::OpenHand(evaluation) => {
            let targets = actionable_open_hand_threat_players(&inputs.open_hand_threats);
            // hard-safe / same-hand passed で selection が確定して exact model が走っていない
            // 場合だけ、診断用の candidate evidence を追加収集する。選択は変わらない。
            collect_open_hand_candidate_ron_risk_evidence(
                context,
                legal_actions,
                &targets,
                evaluation,
            );
            let ron_risk_vectors = evaluation.ron_risk_vectors.as_deref();
            diagnostic.ranked_candidates = ranked_candidates(
                context,
                ordered_open_hand_defense_candidates(
                    context,
                    legal_actions,
                    &targets,
                    ron_risk_vectors,
                ),
                ron_risk_vectors,
                ForcedFoldDefenseKind::OpenHand,
            );
            diagnostic.open_hand_defense = Some(OpenHandDefenseDiagnostic::from_evaluation(
                context,
                legal_actions,
                &inputs.open_hand_threats,
                evaluation,
            ));
        }
        FoldDefenseEvaluation::Combined(evaluation) => {
            let targets =
                combined_threat_defense_targets(&inputs.player_threats, &inputs.open_hand_threats);
            collect_combined_candidate_ron_risk_evidence(
                context,
                legal_actions,
                &targets,
                evaluation,
            );
            let ron_risk_vectors = evaluation.ron_risk_vectors.as_deref();
            diagnostic.ranked_candidates = ranked_candidates(
                context,
                ordered_combined_defense_candidates(
                    context,
                    legal_actions,
                    &targets,
                    ron_risk_vectors,
                ),
                ron_risk_vectors,
                ForcedFoldDefenseKind::Combined,
            );
            diagnostic.combined_defense = Some(CombinedDefenseDiagnostic::from_evaluation(
                context,
                legal_actions,
                &inputs.player_threats,
                &inputs.open_hand_threats,
                evaluation,
            ));
        }
    }

    // forced fold の答えは ranking の先頭そのもの。rank 1 用の特別処理を持たないために、
    // 並べ替え後の先頭を選択へ書き戻す。
    if let Some(first) = diagnostic.ranked_candidates.first() {
        diagnostic.selected_action = first.action.clone();
    }

    Ok(diagnostic)
}

// production ordering の並びと、同じ evaluation が構築した exact evidence を ranked candidate へ
// 写したうえで、forced fold 用に effective fold risk で並べ替える。evidence 自体はここで作り
// 直さず、production ordering も書き換えない。
fn ranked_candidates<K: Copy>(
    context: &GameContext,
    ordered: Vec<(&LegalAction, K)>,
    ron_risk_vectors: Option<&[DahaiRonRiskVector<'_>]>,
    defense_kind: impl Fn(K) -> ForcedFoldDefenseKind,
) -> Vec<ForcedFoldRankedCandidate> {
    let copies = hand_tile_type_counts(context);
    let mut candidates: Vec<_> = ordered
        .into_iter()
        .enumerate()
        .map(|(index, (action, kind))| ForcedFoldRankedCandidate {
            action: action.clone(),
            rank: index + 1,
            defense_kind: defense_kind(kind),
            player_ron_risk_evidence: player_ron_risk_evidence_for_action(ron_risk_vectors, action)
                .map(<[_]>::to_vec),
            copies: action_copies(&copies, action),
        })
        .collect();
    rerank_by_effective_fold_risk(&mut candidates);
    candidates
}

// 手牌 (自摸牌を含む) の牌種別枚数。`TileId::tile_type` は赤5と黒5を同じ牌種へ写すので、
// 待ち判定上同一の牌種として合算される。
fn hand_tile_type_counts(context: &GameContext) -> [usize; TileType::COUNT] {
    let mut counts = [0; TileType::COUNT];
    for tile in context
        .hand_tiles()
        .iter()
        .copied()
        .chain(context.drawn_tile())
    {
        counts[tile.tile_type().index()] += 1;
    }
    counts
}

// 候補牌と同じ牌種を手牌に持っている枚数。手牌にない牌 (Dahai 以外の action) では 1 とする。
fn action_copies(counts: &[usize; TileType::COUNT], action: &LegalAction) -> usize {
    let LegalAction::Dahai { tile } = action else {
        return 1;
    };
    counts[tile.tile_type().index()].max(1)
}

#[cfg(test)]
mod tests;
