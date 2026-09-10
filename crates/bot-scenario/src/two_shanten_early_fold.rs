//! 明確な threat に対して打牌後が二向聴以上になる、確定 Fold 局面の scenario 回帰テスト。
//!
//! `scenarios/two_shanten_fold_reach_genbutsu.json` と
//! `scenarios/two_shanten_fold_reach_genbutsu_heavy.json` は、単独の子リーチに対して打牌後が
//! 二向聴以上のまま、現物の南を切って降りる局面。実戦 capture で重かった request と同じ
//! 「push/pull = Fold / reason = TwoOrMoreShantenAgainstReach / final action = S /
//! source = DefenseFallback」という判断をそのまま持つ。
//!
//! この局面では最終 action に通常打牌選択の結果を使わないので、production の `act()` は
//! 2向聴 ExpectedSelfTsumoValue を評価しない。fixture はその判断と、通常打牌選択の重い phase が
//! 発火しないことの両方を固定する。heavy 側は同じ判断のまま探索がさらに深くなる局面で、
//! 省略の効果を実測するための入口でもある。

use std::time::Duration;

use bot_core::{
    Agent, AgentActionSource, DefenseFallbackKind, DiagnosticOptions, LegalAction, PushPullMode,
    PushPullReason, ShantenAgent,
};

use crate::scenario::{Scenario, ScenarioSpec};

const REACH_GENBUTSU: &str = include_str!("../scenarios/two_shanten_fold_reach_genbutsu.json");
const REACH_GENBUTSU_HEAVY: &str =
    include_str!("../scenarios/two_shanten_fold_reach_genbutsu_heavy.json");

fn scenario(spec: &str) -> Scenario {
    let spec: ScenarioSpec = serde_json::from_str(spec).expect("scenario spec");
    Scenario::resolve(&spec).expect("scenario")
}

fn selected_tile(action: &LegalAction) -> String {
    match action {
        LegalAction::Dahai { tile } => tile.to_mjai_string(),
        other => format!("{other:?}"),
    }
}

#[test]
fn two_shanten_fold_against_a_reach_discards_the_genbutsu_without_the_deep_evaluation() {
    for (name, spec) in [("light", REACH_GENBUTSU), ("heavy", REACH_GENBUTSU_HEAVY)] {
        let scenario = scenario(spec);
        let timed = ShantenAgent.act_with_phase_timing(&scenario.context, &scenario.legal_actions);
        let phases = timed.phases.normal_discard_phases;

        assert_eq!(selected_tile(&timed.action), "S", "{name}");

        // 2向聴 ExpectedSelfTsumoValue も前方集計値も候補比較も通らない。
        assert_eq!(phases.two_shanten_self_tsumo, Duration::ZERO, "{name}");
        assert_eq!(timed.two_shanten_self_tsumo_candidates().len(), 0, "{name}");
        assert_eq!(phases.three_shanten_self_tsumo, Duration::ZERO, "{name}");
        assert_eq!(phases.forward_metrics, Duration::ZERO, "{name}");
        assert_eq!(phases.selection_finalize, Duration::ZERO, "{name}");

        // 向聴数の判定に使う合法打牌候補の基本評価だけは通る。
        assert!(phases.base_evaluation > Duration::ZERO, "{name}");
    }
}

#[test]
fn the_early_fold_keeps_the_push_pull_reason_and_the_defense_source() {
    // 判断内訳は診断経路が従来どおり通常打牌選択まで通して組み立てる。production が省略しても
    // 最終 action・押し引き・防御 fallback の種別は同じになる。
    let scenario = scenario(REACH_GENBUTSU);
    let diagnostic = ShantenAgent::diagnose_with_options(
        &scenario.context,
        &scenario.legal_actions,
        DiagnosticOptions::NONE,
    );

    assert_eq!(selected_tile(&diagnostic.selected_action), "S");
    assert_eq!(
        diagnostic.selected_source,
        AgentActionSource::DefenseFallback(DefenseFallbackKind::Genbutsu)
    );
    assert_eq!(
        diagnostic
            .push_pull_decision
            .map(|decision| (decision.mode, decision.reason)),
        Some((
            PushPullMode::Fold,
            PushPullReason::TwoOrMoreShantenAgainstReach,
        ))
    );
    assert_eq!(
        diagnostic.selected_action,
        ShantenAgent.act(&scenario.context, &scenario.legal_actions)
    );
}
