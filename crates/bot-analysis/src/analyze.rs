use bot_core::{GameContext, LegalAction, ShantenAgent};

use crate::analysis_result::AnalysisResult;

/// production の判断を1局面ぶん実行して、consumer 向けの [`AnalysisResult`] を返す。
///
/// 解析の入口は `GameContext` と合法手で、局面の出所には依存しない。手入力 / scenario JSON は
/// [`Scenario::resolve()`](crate::Scenario::resolve) が、replay は復元した decision point が、
/// それぞれこの2つを用意すればこの入口へ繋がる。
///
/// primary 診断は production 既定の [`ShantenAgent::diagnose`] でここで1回だけ取り、追加
/// 調査用の診断は構築しない。診断そのものは返さず、[`AnalysisResult`] へ投影した結果だけを
/// 渡す。既に primary 診断を持っている consumer は、再診断せずに
/// [`AnalysisResult::from_decision`] を直接呼ぶ。
///
/// `choice_limit` は [`AnalysisResult::choices`] に並べる件数の上限で、consumer が決める。
pub fn analyze(
    context: &GameContext,
    legal_actions: &[LegalAction],
    choice_limit: usize,
) -> AnalysisResult {
    let diagnostic = ShantenAgent::diagnose(context, legal_actions);
    AnalysisResult::from_decision(context, legal_actions, &diagnostic, choice_limit)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ranked_choice::rank_choices;
    use crate::scenario::{Scenario, ScenarioSpec};
    use bot_core::{DiagnosticOptions, ShantenDecisionDiagnostic};

    const CHOICE_LIMIT: usize = 3;

    const NORMAL_SCENARIO: &str = r#"{
        "hand": "234m455p789s1123z",
        "draw": "N",
        "dora_indicators": "3p",
        "round_wind": "E",
        "seat_wind": "S",
        "player_id": 0,
        "oya": 3
    }"#;

    const SINGLE_ACTION_SCENARIO: &str = r#"{
        "hand": "234m455p789s1123z",
        "draw": "N",
        "legal_dahai": "N"
    }"#;

    fn scenario_from_json(json: &str) -> Scenario {
        let spec: ScenarioSpec = serde_json::from_str(json).unwrap();
        Scenario::resolve(&spec).unwrap()
    }

    fn diagnose(scenario: &Scenario) -> ShantenDecisionDiagnostic {
        ShantenAgent::diagnose(&scenario.context, &scenario.legal_actions)
    }

    // 共通入口は production 診断と既存の投影を組み合わせたものと同じ結果になる。
    #[test]
    fn matches_the_production_diagnosis_projected_by_hand() {
        let scenario = scenario_from_json(NORMAL_SCENARIO);
        let diagnostic = diagnose(&scenario);

        assert_eq!(
            analyze(&scenario.context, &scenario.legal_actions, CHOICE_LIMIT),
            AnalysisResult::from_decision(
                &scenario.context,
                &scenario.legal_actions,
                &diagnostic,
                CHOICE_LIMIT
            )
        );
    }

    // choice 1 は production が選んだ action そのもので、入口を通しても変わらない。
    #[test]
    fn keeps_the_production_selected_action_as_the_first_choice() {
        let scenario = scenario_from_json(NORMAL_SCENARIO);
        let diagnostic = diagnose(&scenario);
        let result = analyze(&scenario.context, &scenario.legal_actions, CHOICE_LIMIT);

        assert_eq!(
            result.choices[0].selected_action,
            diagnostic.selected_action
        );
        assert_eq!(
            result.choices[0].selected_source,
            diagnostic.selected_source
        );
        assert!(result.choices[0].comparison.is_none());
    }

    #[test]
    fn keeps_the_choice_limit() {
        let scenario = scenario_from_json(NORMAL_SCENARIO);
        for limit in 0..=CHOICE_LIMIT {
            let result = analyze(&scenario.context, &scenario.legal_actions, limit);
            assert_eq!(result.choices.len(), limit);
        }

        // 合法手が尽きれば limit より少ないところで打ち切る。
        let single = scenario_from_json(SINGLE_ACTION_SCENARIO);
        assert_eq!(single.legal_actions.len(), 1);
        assert_eq!(
            analyze(&single.context, &single.legal_actions, CHOICE_LIMIT)
                .choices
                .len(),
            1
        );
    }

    // scenario 構築の結果をそのまま渡せる。入口は `Scenario` 型そのものを要求しない。
    #[test]
    fn accepts_a_resolved_scenario_context_and_legal_actions() {
        let scenario = scenario_from_json(NORMAL_SCENARIO);
        let result = analyze(&scenario.context, &scenario.legal_actions, CHOICE_LIMIT);

        assert_eq!(
            result.choices,
            rank_choices(
                &scenario.context,
                &scenario.legal_actions,
                &diagnose(&scenario),
                CHOICE_LIMIT
            )
        );
    }

    // 既に primary 診断を持つ consumer の経路では、渡された診断をそのまま choice 1 に使い、
    // production 診断を取り直さない。
    #[test]
    fn projects_the_given_primary_diagnostic_without_rerunning_it() {
        let scenario = scenario_from_json(NORMAL_SCENARIO);
        let production = diagnose(&scenario);

        // 合法手を狭めて、production が全合法手から選ぶ action とは別の action を選ぶ診断を作る。
        let narrowed_actions: Vec<_> = scenario
            .legal_actions
            .iter()
            .filter(|action| **action != production.selected_action)
            .cloned()
            .collect();
        let narrowed = ShantenAgent::diagnose(&scenario.context, &narrowed_actions);
        assert_ne!(narrowed.selected_action, production.selected_action);

        // 全合法手を渡しても choice 1 は再診断されず、渡した診断の選択のままになる。
        let result = AnalysisResult::from_decision(
            &scenario.context,
            &scenario.legal_actions,
            &narrowed,
            CHOICE_LIMIT,
        );
        assert_eq!(result.choices[0].selected_action, narrowed.selected_action);

        // 詳細診断を渡す経路でも同じく、その診断が choice 1 になる。
        let detailed = ShantenAgent::diagnose_with_options(
            &scenario.context,
            &scenario.legal_actions,
            DiagnosticOptions::WITH_LOOKAHEAD,
        );
        assert!(detailed.normal_discard_lookahead.is_some());
        let detailed_result = AnalysisResult::from_decision(
            &scenario.context,
            &scenario.legal_actions,
            &detailed,
            CHOICE_LIMIT,
        );
        assert_eq!(
            detailed_result.choices[0].selected_action,
            detailed.selected_action
        );
    }
}
