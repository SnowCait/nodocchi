//! 2向聴のドラ差 gate を通った provisional 上位2候補だけを並行評価する production 経路の回帰。
//!
//! 固定するのは、
//!
//! * gate が発火しない局面では並行評価そのものが起きないこと
//! * gate が発火する局面で並行評価の対象が provisional 上位2候補だけであること
//! * Progress cohort は分け方に依らないこと
//! * S (worker 1本) / P2 (worker ごとに閉じた探索基盤) / P2S (worker 間で exact cache を共有)
//!   の2候補の Full `ExpectedSelfTsumoValue`・最終 selected discard・比較理由が bit-exact に
//!   一致すること
//!
//! で、Progress-first も ForwardTargets も上位2候補の選び方も gate も comparator も
//! この module では変えない。

use bot_core::{
    SharedSelectionCacheEntries, TwoShantenFullParallelRun, TwoShantenFullParallelism,
    select_with_two_shanten_full_parallelism, two_shanten_full_parallelism_is_available,
};

use crate::scenario::{Scenario, ScenarioSpec};

const PROGRESS_FIRST_BASELINE: &str =
    include_str!("../scenarios/two_shanten_progress_first_baseline.json");
const DORA_GATE_CHUN: &str = include_str!("../scenarios/two_shanten_dora_gate_chun.json");

fn scenario(spec: &str) -> Scenario {
    let spec: ScenarioSpec = serde_json::from_str(spec).expect("scenario spec");
    Scenario::resolve(&spec).expect("scenario")
}

fn select(
    scenario: &Scenario,
    parallelism: TwoShantenFullParallelism,
) -> TwoShantenFullParallelRun {
    select_with_two_shanten_full_parallelism(
        &scenario.context,
        scenario.legal_actions.as_slice(),
        parallelism,
    )
}

fn selected_discard(run: &TwoShantenFullParallelRun) -> String {
    run.selected_discard()
        .expect("通常打牌を選ぶ")
        .to_mjai_string()
}

// この runtime で P2 が実際に分けられる worker 数。並列度 1 の環境では逐次評価へ落ちる。
fn expected_full_workers() -> usize {
    if two_shanten_full_parallelism_is_available() {
        2
    } else {
        1
    }
}

#[test]
fn the_full_gate_miss_never_splits_the_evaluation() {
    // ドラ差 gate が発火しない局面。Progress cohort だけを評価して決着するので、どちらの方式でも
    // Full 追加評価そのものが走らず、thread も分けない。
    let scenario = scenario(PROGRESS_FIRST_BASELINE);

    for parallelism in TwoShantenFullParallelism::ALL {
        let run = select(&scenario, parallelism);
        let label = parallelism.label();

        assert_eq!(selected_discard(&run), "8m", "{label}");
        assert!(run.full_evaluated_candidates().is_empty(), "{label}");
        assert_eq!(run.full_workers, 1, "{label}");
        assert!(!run.progress_evaluated_candidates().is_empty(), "{label}");
    }
}

#[test]
fn the_gated_pair_is_bit_exact_between_the_sequential_and_the_parallel_evaluation() {
    // ドラ差 gate が発火する局面。並行評価するのは gate を通った2候補だけで、Progress cohort も
    // 候補ごとの値も比較理由も選ばれた打牌も逐次評価と一致する。worker 間で探索基盤の exact
    // cache を共有するかどうかでも、どれ一つ変わらない。
    let scenario = scenario(DORA_GATE_CHUN);
    let sequential = select(&scenario, TwoShantenFullParallelism::Sequential);

    // 逐次評価は1本の探索基盤を使う。
    assert_eq!(sequential.full_workers, 1);
    // S は production の worker 上限を 1 にしただけなので、既存の scenario 回帰が固定している
    // production の打牌と同じ打牌になる。
    assert_eq!(selected_discard(&sequential), "C");

    // Full 追加評価の対象は provisional 上位2候補だけ。
    let full = sequential.full_evaluated_candidates();
    assert_eq!(full.len(), 2);

    for parallelism in [
        TwoShantenFullParallelism::Isolated,
        TwoShantenFullParallelism::Shared,
    ] {
        let parallel = select(&scenario, parallelism);
        let label = parallelism.label();

        assert_eq!(parallel.full_evaluated_candidates(), full, "{label}");

        // Progress cohort は分け方に依らない。
        assert_eq!(
            parallel.progress_evaluated_candidates(),
            sequential.progress_evaluated_candidates(),
            "{label}",
        );

        // 2候補の Full 値も、軸解決も、比較理由も、選ばれた打牌も bit-exact に一致する。
        assert_eq!(parallel.candidates, sequential.candidates, "{label}");
        assert_eq!(parallel.selected, sequential.selected, "{label}");
        assert_eq!(selected_discard(&parallel), "C", "{label}");

        // 並行評価は Full 対象の2候補を超えて thread を作らない。
        assert_eq!(parallel.full_workers, expected_full_workers(), "{label}");
    }
}

#[test]
fn the_shared_cache_only_holds_entries_when_the_pair_is_actually_split() {
    // 共有 cache を持つのは、Full top-2 を実際に worker へ分ける2向聴局面だけ。逐次評価でも
    // gate 不発の局面でも entry は作らない。
    let gated = scenario(DORA_GATE_CHUN);
    let sequential = select(&gated, TwoShantenFullParallelism::Sequential);
    let isolated = select(&gated, TwoShantenFullParallelism::Isolated);
    let shared = select(&gated, TwoShantenFullParallelism::Shared);

    assert_eq!(
        sequential.shared_cache,
        SharedSelectionCacheEntries::default()
    );
    assert_eq!(
        isolated.shared_cache,
        SharedSelectionCacheEntries::default()
    );
    if two_shanten_full_parallelism_is_available() {
        assert!(shared.shared_cache.base_evaluations > 0);
        assert!(shared.shared_cache.structural_evaluations > 0);
        assert!(shared.shared_cache.tenpai_selection_values > 0);
        assert!(shared.shared_cache.tenpai_tsumo_values > 0);
    }

    // gate が発火しない局面では Full 追加評価そのものが走らないので、共有する entry も無い。
    let baseline = scenario(PROGRESS_FIRST_BASELINE);
    for parallelism in TwoShantenFullParallelism::ALL {
        let run = select(&baseline, parallelism);
        assert_eq!(
            run.shared_cache,
            SharedSelectionCacheEntries::default(),
            "{}",
            parallelism.label(),
        );
    }
}
