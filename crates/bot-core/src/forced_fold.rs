//! 押し引き判断とは独立に、ベタ降りを仮定した場合の防御打牌を評価する hypothetical 入口。
//!
//! production の打牌選択には接続しない。`ShantenAgent::act()` / `diagnose*()` はこの module を
//! 呼ばず、ここでの評価も production decision を書き換えない。
//!
//! routing も防御選択も既存 [`evaluate_fold_defense`] をそのまま source of truth にし、threat の
//! 有無も既存 [`has_clear_threat`] と同じ classification を使う。forced fold のために防御ロジックも
//! High 条件も書き直さない。

use crate::action::LegalAction;
use crate::combined_defense::{CombinedDefenseCategory, CombinedDefenseDiagnostic};
use crate::context::GameContext;
use crate::defense::{DefenseDecisionDiagnostic, DefenseFallbackKind};
use crate::fold_defense::{FoldDefenseEvaluation, FoldDefenseKind, evaluate_fold_defense};
use crate::open_hand_defense::{OpenHandDefenseCategory, OpenHandDefenseDiagnostic};
use crate::push_pull::{has_clear_threat, push_pull_inputs_from_threat_facts};
use crate::threat::player_threat_facts_from_context;

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

/// forced fold evaluation で追加構築する解析情報の指定。
///
/// 選択結果 (`selected_action` / `defense_kind`) と routing はどちらでも同じで、追加で収集する
/// 解析情報だけが変わる。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ForcedFoldDiagnosticScope {
    /// 選択に必要な評価だけを行う。
    ///
    /// 候補診断を構築せず、Reach 防御が共通現物で決着した場合は exact ron-risk の candidate
    /// evidence も収集しない。既存 evaluator の現物早期 return をそのまま使うだけで、選択の
    /// semantics は変わらない。
    SelectionOnly,
    /// 候補評価まで含む防御診断を構築する。
    ///
    /// 共通現物で決着した場合も exact model の candidate evidence を追加収集する。これは既存
    /// diagnostics 有効時と同じ挙動で、選択結果は変わらない。
    Detailed,
}

/// forced fold の評価結果。
///
/// `selected_action` と `defense_kind` は既存 Fold defense evaluator の選択そのもので、候補評価は
/// 選択に使った同じ evaluation から構築する。診断は
/// [`ForcedFoldDiagnosticScope::Detailed`] を指定した場合だけ構築し、そのときも防御 family に
/// 対応するものだけが `Some` になる。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ForcedFoldDiagnostic {
    pub selected_action: LegalAction,
    pub defense_kind: ForcedFoldDefenseKind,
    pub defense: Option<DefenseDecisionDiagnostic>,
    pub open_hand_defense: Option<OpenHandDefenseDiagnostic>,
    pub combined_defense: Option<CombinedDefenseDiagnostic>,
}

/// 通常の押し引き判断とは無関係に、ベタ降りすると仮定した場合の防御打牌を評価する。
///
/// 通常打牌選択・2手先探索・Reach / Damaten・押し引き判定はどれも走らせない。threat facts は
/// 押し引きが使うものと同じ構築経路から攻撃評価なしで作り、classification を別実装しない。
///
/// `scope` は追加で収集する解析情報だけを決める。選択打牌・defense family・family 内の選択種別は
/// `scope` によらず同じで、routing も既存 [`evaluate_fold_defense`] のまま変わらない。
pub fn evaluate_forced_fold(
    context: &GameContext,
    legal_actions: &[LegalAction],
    scope: ForcedFoldDiagnosticScope,
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

    let detailed = scope == ForcedFoldDiagnosticScope::Detailed;
    let evaluation = evaluate_fold_defense(context, legal_actions, &inputs, detailed);
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
        defense: None,
        open_hand_defense: None,
        combined_defense: None,
    };

    if !detailed {
        return Ok(diagnostic);
    }

    match &evaluation {
        FoldDefenseEvaluation::Reach(evaluation) => {
            diagnostic.defense = Some(DefenseDecisionDiagnostic::from_evaluation(
                context,
                legal_actions,
                evaluation,
            ));
        }
        FoldDefenseEvaluation::OpenHand(evaluation) => {
            diagnostic.open_hand_defense = Some(OpenHandDefenseDiagnostic::from_evaluation(
                context,
                legal_actions,
                &inputs.open_hand_threats,
                evaluation,
            ));
        }
        FoldDefenseEvaluation::Combined(evaluation) => {
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

#[cfg(test)]
mod tests;
