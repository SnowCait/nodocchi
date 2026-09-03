//! Fold 時に threat の組み合わせから defense family を選び、既存 evaluator を呼び出す。

use crate::action::LegalAction;
use crate::combined_defense::{
    CombinedDefenseCategory, CombinedDefenseEvaluation, combined_threat_defense_targets,
    evaluate_combined_threat_defense_fallback_action_with_kind,
};
use crate::context::GameContext;
use crate::defense::{
    DefenseFallbackEvaluation, DefenseFallbackKind, evaluate_defense_fallback_action_with_kind,
    log_defense_fallback_evaluation,
};
use crate::open_hand_defense::{
    OpenHandDefenseCategory, OpenHandDefenseEvaluation,
    evaluate_open_hand_defense_fallback_action_with_kind, high_open_hand_threat_players,
};
use crate::push_pull::PushPullInputs;

/// Fold defense が選んだ defense family と、その family 内の選択種別。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FoldDefenseKind {
    Reach(DefenseFallbackKind),
    OpenHand(OpenHandDefenseCategory),
    Combined(CombinedDefenseCategory),
}

/// Fold defense が選んだ action と defense 側の意味。
#[derive(Debug, Clone, Copy)]
pub(crate) struct FoldDefenseSelection<'a> {
    pub(crate) action: &'a LegalAction,
    pub(crate) kind: FoldDefenseKind,
}

/// Fold defense が一度だけ行った production evaluation。
///
/// variant が routing 結果を表し、内側の facts は diagnostics が同じ evaluation を再利用する。
#[derive(Debug)]
pub(crate) enum FoldDefenseEvaluation<'a> {
    Reach(DefenseFallbackEvaluation<'a>),
    OpenHand(OpenHandDefenseEvaluation<'a>),
    Combined(CombinedDefenseEvaluation<'a>),
}

impl<'a> FoldDefenseEvaluation<'a> {
    pub(crate) fn selected(&self) -> Option<FoldDefenseSelection<'a>> {
        match self {
            Self::Reach(evaluation) => {
                evaluation
                    .selected
                    .map(|(action, kind)| FoldDefenseSelection {
                        action,
                        kind: FoldDefenseKind::Reach(kind),
                    })
            }
            Self::OpenHand(evaluation) => {
                evaluation
                    .selected
                    .map(|(action, category)| FoldDefenseSelection {
                        action,
                        kind: FoldDefenseKind::OpenHand(category),
                    })
            }
            Self::Combined(evaluation) => {
                evaluation
                    .selected
                    .map(|(action, category)| FoldDefenseSelection {
                        action,
                        kind: FoldDefenseKind::Combined(category),
                    })
            }
        }
    }
}

/// Fold 時の threat 構成に対応する既存 defense evaluator を一度だけ呼ぶ。
///
/// routing 条件は [`PushPullInputs`] が持つ production facts / classification をそのまま使う。
/// `collect_exact_evidence_for_genbutsu` は diagnostics が有効な場合だけ true で、通常の `act()`
/// では現物選択後に exact model を追加実行しない。
pub(crate) fn evaluate_fold_defense<'a>(
    context: &GameContext,
    legal_actions: &'a [LegalAction],
    inputs: &PushPullInputs,
    collect_exact_evidence_for_genbutsu: bool,
) -> FoldDefenseEvaluation<'a> {
    if inputs.has_combined_threat() {
        let targets =
            combined_threat_defense_targets(&inputs.player_threats, &inputs.open_hand_threats);
        return FoldDefenseEvaluation::Combined(
            evaluate_combined_threat_defense_fallback_action_with_kind(
                context,
                legal_actions,
                &targets,
            ),
        );
    }

    if inputs.opponent_reach_count > 0 {
        let evaluation =
            evaluate_reach_defense(context, legal_actions, collect_exact_evidence_for_genbutsu);
        log_defense_fallback_evaluation(context, &evaluation, legal_actions);
        return FoldDefenseEvaluation::Reach(evaluation);
    }

    let targets = high_open_hand_threat_players(&inputs.open_hand_threats);
    FoldDefenseEvaluation::OpenHand(evaluate_open_hand_defense_fallback_action_with_kind(
        context,
        legal_actions,
        &targets,
    ))
}

/// Push / Neutral の最終 fallback と Fold の reach-defense 枝が共有する既存 evaluator 呼び出し。
pub(crate) fn evaluate_reach_defense<'a>(
    context: &GameContext,
    legal_actions: &'a [LegalAction],
    collect_exact_evidence_for_genbutsu: bool,
) -> DefenseFallbackEvaluation<'a> {
    evaluate_defense_fallback_action_with_kind(
        context,
        legal_actions,
        collect_exact_evidence_for_genbutsu,
    )
}

#[cfg(test)]
mod tests;
