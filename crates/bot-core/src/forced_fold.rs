//! 押し引き判断とは独立に、ベタ降りを仮定した場合の防御打牌を評価する hypothetical 入口。
//!
//! production の打牌選択には接続しない。`ShantenAgent::act()` / `diagnose*()` はこの module を
//! 呼ばず、ここでの評価も production decision を書き換えない。
//!
//! routing も防御選択も既存 [`evaluate_fold_defense`] をそのまま source of truth にし、threat の
//! 有無も既存 [`has_clear_threat`] と同じ classification を使う。forced fold のために防御ロジックも
//! High 条件も書き直さない。
//!
//! 候補 ranking も既存 production defense policy の ordering をそのまま使い、選択・ranking・
//! 詳細診断は1回の evaluation から得た同じ candidate evidence を共有する。

mod ranking;

use crate::action::LegalAction;
use crate::combined_defense::{
    CombinedDefenseCategory, CombinedDefenseDiagnostic, combined_threat_defense_targets,
    ordered_combined_defense_candidates,
};
use crate::context::GameContext;
use crate::defense::{
    DahaiRonRiskVector, DefenseDecisionDiagnostic, DefenseFallbackKind,
    ordered_defense_fallback_candidates, player_ron_risk_evidence_for_action,
};
use crate::fold_defense::{FoldDefenseEvaluation, FoldDefenseKind, evaluate_fold_defense};
use crate::open_hand_defense::{
    OpenHandDefenseCategory, OpenHandDefenseDiagnostic, high_open_hand_threat_players,
    ordered_open_hand_defense_candidates,
};
use crate::push_pull::{has_clear_threat, push_pull_inputs_from_threat_facts};
use crate::threat::player_threat_facts_from_context;

pub use ranking::ForcedFoldRankedCandidate;

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
    /// リーチ者も High OpenHandThreat の相手もいない。防御対象がないので評価しない。
    NoClearThreat,
    /// 防御対象はいるが、既存 evaluator が防御打牌を選べなかった。合法 Dahai がない局面を含む。
    NoDefenseSelection,
}

/// forced fold の評価結果。
///
/// `selected_action` と `defense_kind` は既存 Fold defense evaluator の選択そのもので、
/// `ranked_candidates` と防御診断は選択に使った同じ evaluation から構築する。診断は防御 family に
/// 対応するものだけが `Some` になる。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ForcedFoldDiagnostic {
    pub selected_action: LegalAction,
    pub defense_kind: ForcedFoldDefenseKind,
    /// 全合法 Dahai を production ordering どおりに並べた防御候補。
    ///
    /// 先頭が `selected_action` と一致するのは ranking 全体が既存 production selector と同じ
    /// ordering だからで、rank 1 用の特別処理は持たない。
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

    let evaluation = evaluate_fold_defense(context, legal_actions, &inputs, true);
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

    match &evaluation {
        FoldDefenseEvaluation::Reach(evaluation) => {
            let ron_risk_vectors = evaluation.ron_risk_vectors.as_deref();
            diagnostic.ranked_candidates = ranked_candidates(
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
            let targets = high_open_hand_threat_players(&inputs.open_hand_threats);
            let ron_risk_vectors = evaluation.ron_risk_vectors.as_deref();
            diagnostic.ranked_candidates = ranked_candidates(
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
            let ron_risk_vectors = evaluation.ron_risk_vectors.as_deref();
            diagnostic.ranked_candidates = ranked_candidates(
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

    Ok(diagnostic)
}

// production ordering の並びと、同じ evaluation が構築した exact evidence を ranked candidate へ
// 写す。順序も evidence もここでは作り直さない。
fn ranked_candidates<K: Copy>(
    ordered: Vec<(&LegalAction, K)>,
    ron_risk_vectors: Option<&[DahaiRonRiskVector<'_>]>,
    defense_kind: impl Fn(K) -> ForcedFoldDefenseKind,
) -> Vec<ForcedFoldRankedCandidate> {
    ordered
        .into_iter()
        .enumerate()
        .map(|(index, (action, kind))| ForcedFoldRankedCandidate {
            action: action.clone(),
            rank: index + 1,
            defense_kind: defense_kind(kind),
            player_ron_risk_evidence: player_ron_risk_evidence_for_action(ron_risk_vectors, action)
                .map(<[_]>::to_vec),
        })
        .collect()
}

#[cfg(test)]
mod tests;
