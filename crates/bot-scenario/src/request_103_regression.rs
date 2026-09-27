//! request 103 の1向聴 StableOrder fallback を固定する回帰テスト。
//!
//! `scenarios/request_103_iishanten_stable_order.json` は実戦 request 103 の局面を synthetic に
//! 再現したもの。production horizon (h12) では 4m と 6s が StableOrder の手前までの全既存軸で
//! 完全同値になり、従来は列挙順で 4m を選んでいた。fallback はこの完全同値 cohort だけを
//! `UNTIL_RYUKYOKU` の ExpectedSelfTsumoValue で比べ直す。値は既存 evaluator が求めたものを
//! 比べるだけで、テスト側に期待値を持たない。

use bot_analysis::{Scenario, ScenarioSpec};
use bot_core::{
    AgentActionSource, GameContext, LegalAction, ShantenAgent, ShantenDecisionDiagnostic,
};
use bot_logic::{
    DiscardCandidateDiagnostic, DiscardComparisonReason, DiscardEvaluation, ForwardMetrics,
    IishantenStableOrderFallbackMetrics, SelfTsumoHorizon, TileType,
    best_discard_selection_index_with_stable_order_fallback,
    best_discard_selection_index_with_three_shanten_metrics,
    diagnose_discard_evaluations_with_three_shanten_metrics,
    iishanten_stable_order_fallback_cohort,
};

const REQUEST_103: &str = include_str!("../scenarios/request_103_iishanten_stable_order.json");

fn resolve() -> Scenario {
    let spec: ScenarioSpec = serde_json::from_str(REQUEST_103).expect("scenario spec");
    Scenario::resolve(&spec).expect("scenario")
}

fn tile_type(mjai: &str) -> TileType {
    TileType::from_mjai_type_str(mjai).unwrap()
}

fn diagnose(context: &GameContext, legal_actions: &[LegalAction]) -> ShantenDecisionDiagnostic {
    ShantenAgent::diagnose(context, legal_actions)
}

fn candidates(diagnostic: &ShantenDecisionDiagnostic) -> &[DiscardCandidateDiagnostic] {
    &diagnostic
        .normal_discard
        .as_ref()
        .expect("normal discard diagnostic")
        .candidates
}

fn candidate<'a>(
    diagnostic: &'a ShantenDecisionDiagnostic,
    discard: &str,
) -> &'a DiscardCandidateDiagnostic {
    candidates(diagnostic)
        .iter()
        .find(|candidate| candidate.evaluation.discard == tile_type(discard))
        .unwrap_or_else(|| panic!("{discard} candidate"))
}

// production comparator が使った入力を診断から取り出す。値は選択が使ったものそのもの。
struct ComparatorInputs {
    evaluations: Vec<DiscardEvaluation>,
    forward: Vec<ForwardMetrics>,
    fallback: Vec<IishantenStableOrderFallbackMetrics>,
}

fn comparator_inputs(diagnostic: &ShantenDecisionDiagnostic) -> ComparatorInputs {
    let candidates = candidates(diagnostic);
    ComparatorInputs {
        evaluations: candidates
            .iter()
            .map(|candidate| candidate.evaluation.clone())
            .collect(),
        forward: candidates
            .iter()
            .map(|candidate| ForwardMetrics {
                tenpai_wait: candidate.tenpai_wait,
                next_acceptance: candidate.next_acceptance,
                prospective_value: candidate.prospective_value,
                expected_self_tsumo_value: candidate.expected_self_tsumo_value,
            })
            .collect(),
        fallback: candidates
            .iter()
            .map(|candidate| IishantenStableOrderFallbackMetrics {
                until_ryukyoku_expected_self_tsumo_value: candidate
                    .until_ryukyoku_expected_self_tsumo_value,
            })
            .collect(),
    }
}

fn discards_of(evaluations: &[DiscardEvaluation], indices: &[usize]) -> Vec<TileType> {
    let mut discards: Vec<_> = indices
        .iter()
        .map(|&index| evaluations[index].discard)
        .collect();
    discards.sort();
    discards
}

#[test]
fn the_h12_production_comparator_ties_4m_and_6s_down_to_the_stable_order() {
    let scenario = resolve();
    assert_eq!(
        scenario.context.self_tsumo_horizon(),
        SelfTsumoHorizon::PRODUCTION
    );
    let diagnostic = diagnose(&scenario.context, &scenario.legal_actions);
    let four_man = candidate(&diagnostic, "4m");
    let six_sou = candidate(&diagnostic, "6s");

    // 実ログの h12 指標そのもの。
    for tied in [four_man, six_sou] {
        assert_eq!(tied.evaluation.min_shanten_after_discard(), 1);
        assert_eq!(tied.evaluation.acceptance_total_remaining(), 11);
        assert_eq!(tied.evaluation.acceptance_type_count(), 5);
        let wait = tied.tenpai_wait.expect("weighted tenpai wait");
        assert_eq!(
            (wait.weighted_remaining, wait.weighted_type_count),
            (46, 22)
        );
        let value = tied
            .expected_self_tsumo_value
            .expect("h12 self-tsumo value");
        assert_eq!((value + 500) / 1_000, 59_667);
        assert_eq!(tied.evaluation.shape_penalty, 52);
    }
    assert_eq!(
        four_man.expected_self_tsumo_value,
        six_sou.expected_self_tsumo_value
    );
    assert_eq!(
        four_man.evaluation.standard_iishanten_shape_after_discard,
        six_sou.evaluation.standard_iishanten_shape_after_discard,
    );

    // fallback を渡さない h12 comparator は従来どおり 4m を選び、6s は StableOrder で負ける。
    let inputs = comparator_inputs(&diagnostic);
    let before = best_discard_selection_index_with_three_shanten_metrics(
        &inputs.evaluations,
        &inputs.forward,
        &[],
        &[],
        &[],
    )
    .expect("selected");
    assert_eq!(inputs.evaluations[before].discard, tile_type("4m"));
    let before_diagnostic = diagnose_discard_evaluations_with_three_shanten_metrics(
        &bot_logic::TileCounts::from_tiles(
            scenario
                .context
                .hand_tiles()
                .iter()
                .copied()
                .chain(scenario.context.drawn_tile()),
        ),
        bot_logic::FixedMeldCount::NONE,
        &inputs.evaluations,
        &inputs.forward,
        &[],
        &[],
        &[],
    );
    let six_sou_before = before_diagnostic
        .candidates
        .iter()
        .find(|candidate| candidate.evaluation.discard == tile_type("6s"))
        .unwrap();
    assert_eq!(
        six_sou_before.comparison_reason,
        DiscardComparisonReason::StableOrder
    );
    assert!(!six_sou_before.selected_is_strictly_better_than_candidate);

    // h12 の完全同値 cohort はちょうど 4m と 6s。
    let cohort =
        iishanten_stable_order_fallback_cohort(&inputs.evaluations, &inputs.forward, &[], &[], &[]);
    assert_eq!(
        discards_of(&inputs.evaluations, &cohort),
        vec![tile_type("4m"), tile_type("6s")]
    );
}

#[test]
fn the_until_ryukyoku_fallback_selects_6s_with_its_own_reason() {
    let scenario = resolve();
    let diagnostic = diagnose(&scenario.context, &scenario.legal_actions);
    let four_man = candidate(&diagnostic, "4m");
    let six_sou = candidate(&diagnostic, "6s");

    let four_man_value = four_man
        .until_ryukyoku_expected_self_tsumo_value
        .expect("4m fallback value");
    let six_sou_value = six_sou
        .until_ryukyoku_expected_self_tsumo_value
        .expect("6s fallback value");
    assert!(six_sou_value > four_man_value);

    assert_eq!(diagnostic.selected_source, AgentActionSource::NormalDiscard);
    assert!(matches!(
        diagnostic.selected_action,
        LegalAction::Dahai { tile } if tile.tile_type() == tile_type("6s")
    ));
    assert!(six_sou.selected);
    assert!(four_man.selected_is_strictly_better_than_candidate);
    assert_eq!(
        four_man.comparison_reason,
        DiscardComparisonReason::UntilRyukyokuExpectedSelfTsumoValue
    );

    // fallback の値は cohort の2候補だけに載り、h12 の値は h12 のまま。
    for other in candidates(&diagnostic).iter().filter(|candidate| {
        ![tile_type("4m"), tile_type("6s")].contains(&candidate.evaluation.discard)
    }) {
        assert_eq!(other.until_ryukyoku_expected_self_tsumo_value, None);
        assert_ne!(
            other.comparison_reason,
            DiscardComparisonReason::UntilRyukyokuExpectedSelfTsumoValue
        );
    }
    assert_eq!(
        six_sou.expected_self_tsumo_value,
        four_man.expected_self_tsumo_value
    );

    // production act() も同じ打牌を選ぶ。
    let mut agent = ShantenAgent;
    let action = bot_core::Agent::act(&mut agent, &scenario.context, &scenario.legal_actions);
    assert_eq!(action, diagnostic.selected_action);
}

#[test]
fn the_fallback_value_is_the_existing_until_ryukyoku_evaluator() {
    // fallback 値は horizon だけを UNTIL_RYUKYOKU にした既存1向聴 evaluator の値と一致する。
    let scenario = resolve();
    let production = diagnose(&scenario.context, &scenario.legal_actions);
    let until_ryukyoku_context = scenario
        .context
        .clone()
        .with_self_tsumo_horizon(SelfTsumoHorizon::UNTIL_RYUKYOKU);
    let until_ryukyoku = diagnose(&until_ryukyoku_context, &scenario.legal_actions);

    for discard in ["4m", "6s"] {
        assert_eq!(
            candidate(&production, discard).until_ryukyoku_expected_self_tsumo_value,
            candidate(&until_ryukyoku, discard).expected_self_tsumo_value,
            "{discard}"
        );
    }
    // configured horizon がすでに UNTIL_RYUKYOKU なら fallback は評価しない。
    assert!(
        candidates(&until_ryukyoku)
            .iter()
            .all(|candidate| candidate.until_ryukyoku_expected_self_tsumo_value.is_none())
    );
}

#[test]
fn the_selected_iishanten_forward_metrics_stay_on_the_production_horizon() {
    let scenario = resolve();
    let diagnostic = diagnose(&scenario.context, &scenario.legal_actions);
    let six_sou = candidate(&diagnostic, "6s");
    let offense = diagnostic
        .push_pull_inputs
        .as_ref()
        .and_then(|inputs| inputs.offense.as_ref())
        .expect("offense state");
    let metrics = offense
        .iishanten_forward_metrics
        .expect("selected iishanten forward metrics");

    // 選んだ 6s の h12 前方集計値で、fallback の UNTIL_RYUKYOKU 値を混ぜない。
    assert_eq!(
        metrics.expected_self_tsumo_value,
        six_sou.expected_self_tsumo_value
    );
    assert_eq!(metrics.tenpai_wait, six_sou.tenpai_wait);
    assert_ne!(
        metrics.expected_self_tsumo_value,
        six_sou.until_ryukyoku_expected_self_tsumo_value
    );
    assert_eq!(
        offense.acceptance_total_remaining,
        six_sou.evaluation.acceptance_total_remaining()
    );
}

#[test]
fn the_fallback_cohort_and_winner_do_not_depend_on_the_candidate_order() {
    let scenario = resolve();
    let diagnostic = diagnose(&scenario.context, &scenario.legal_actions);
    let inputs = comparator_inputs(&diagnostic);
    let len = inputs.evaluations.len();

    // 回転と反転の全組み合わせで候補順を並べ替える。
    for rotation in 0..len {
        for reversed in [false, true] {
            let mut order: Vec<usize> = (0..len).map(|index| (index + rotation) % len).collect();
            if reversed {
                order.reverse();
            }
            let evaluations: Vec<_> = order
                .iter()
                .map(|&index| inputs.evaluations[index].clone())
                .collect();
            let forward: Vec<_> = order.iter().map(|&index| inputs.forward[index]).collect();
            let fallback: Vec<_> = order.iter().map(|&index| inputs.fallback[index]).collect();

            let cohort =
                iishanten_stable_order_fallback_cohort(&evaluations, &forward, &[], &[], &[]);
            assert_eq!(
                discards_of(&evaluations, &cohort),
                vec![tile_type("4m"), tile_type("6s")],
                "{order:?}"
            );
            let winner = best_discard_selection_index_with_stable_order_fallback(
                &evaluations,
                &forward,
                &[],
                &[],
                &[],
                &fallback,
            )
            .expect("selected");
            assert_eq!(evaluations[winner].discard, tile_type("6s"), "{order:?}");
        }
    }
}

#[test]
fn an_until_ryukyoku_configured_horizon_skips_the_fallback_regardless_of_the_late_minimum() {
    // horizon_turn >= 18 はすでに流局までの評価なので、late minimum が残っていても fallback の
    // 追加探索を行わない。production (h12) では同じ局面で発火する。
    let scenario = resolve();
    let mut agent = ShantenAgent;
    let production = agent.act_with_phase_timing(&scenario.context, &scenario.legal_actions);
    assert_eq!(production.iishanten_stable_order_fallback_candidates(), 2);
    assert!(
        production
            .phases
            .normal_discard_phases
            .iishanten_stable_order_fallback
            > std::time::Duration::ZERO
    );

    for horizon in [
        SelfTsumoHorizon::UNTIL_RYUKYOKU,
        SelfTsumoHorizon {
            horizon_turn: 18,
            late_min_future_draws: 2,
        },
        SelfTsumoHorizon {
            horizon_turn: 20,
            late_min_future_draws: 2,
        },
    ] {
        assert!(horizon.is_until_ryukyoku());
        let context = scenario.context.clone().with_self_tsumo_horizon(horizon);
        let timed = agent.act_with_phase_timing(&context, &scenario.legal_actions);
        assert_eq!(
            timed.iishanten_stable_order_fallback_candidates(),
            0,
            "{horizon:?}"
        );
        assert_eq!(
            timed
                .phases
                .normal_discard_phases
                .iishanten_stable_order_fallback,
            std::time::Duration::ZERO,
            "{horizon:?}"
        );
        let diagnostic = diagnose(&context, &scenario.legal_actions);
        assert!(
            candidates(&diagnostic).iter().all(|candidate| {
                candidate.until_ryukyoku_expected_self_tsumo_value.is_none()
                    && candidate.comparison_reason
                        != DiscardComparisonReason::UntilRyukyokuExpectedSelfTsumoValue
            }),
            "{horizon:?}"
        );
    }
}
