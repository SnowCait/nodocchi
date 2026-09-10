//! production の2向聴 selection で、ドラ差 gate を通った provisional 上位2候補の Full 追加
//! 評価を2並列にした場合の wall-clock を観測する。
//!
//! 比べるのはこの2候補の実行 orchestration だけで、Progress-first も ForwardTargets も
//! 上位2候補の選び方も Full gate も comparator も値の意味も一切変えない。
//!
//! ```text
//! S:  Progress cohort → gate → 候補1 → 候補2   (1本の LookaheadInputs で逐次評価)
//! P2: Progress cohort → gate → 候補1 ─┐
//!                                候補2 ─┴→ pair index へ書き戻し → 既存 comparator
//! ```
//!
//! # exact である理由
//!
//! 候補1件の Full `ExpectedSelfTsumoValue` は、その候補の打牌評価と探索設定と、Progress 段で
//! 確定済みの寄与だけで決まる純関数の値で、もう一方の候補の評価を入力にしない。探索基盤
//! (base 評価 memo・構造評価 memo・thread-local の向聴 / 受け入れ memo) は同じ入力に同じ値を
//! 返す cache でしかないため、どこまで共有できたかは値を変えない。結果は pair index へ書き戻す
//! ので、thread の終了順にも worker の数にも依らない。したがって2候補の Full 値・最終 selected
//! index・selected discard・比較理由は S と P2 で bit-exact に一致する。
//!
//! # 増える総仕事量
//!
//! 逐次評価では Progress 段で暖まった1本の探索基盤を Full の2候補が共有できる。2候補を thread
//! へ分けると worker ごとに基盤を作り直すため、wall-clock が縮む一方で base 評価・構造評価・
//! terminal scoring・same-shanten 列挙は増える。速くなったかだけでなく、共有を失って総仕事量が
//! どれだけ増えたかも併せて観測する。
//!
//! # 計測条件
//!
//! [`crate::iishanten_selection_parallel_comparison`] と同じく、方式ごとに run を2本取る。
//! `elapsed` は探索規模の計上も phase timer も持たない計測 run のもので、値・stats・phase は
//! 観測 run のもの。どちらの run も新しい thread で行うため、先に走った方式が後の方式の
//! thread-local memo を暖めない。
//!
//! threading は production でもこの診断層でも `bot-core` の orchestration 側だけが持ち、
//! bot-logic の純粋な評価は platform threading を前提にしない。

use std::time::Duration;

use bot_logic::{DiscardComparisonReason, SearchStateMemoStats, ThreeShantenSearchStats, TileType};

use crate::action::LegalAction;
use crate::context::GameContext;
use crate::decision_timing::{NormalDiscardPhaseDurations, NormalDiscardPhaseTimer};
use crate::discard_selection::{
    IishantenContinuationSelection, IishantenContinuationSettings, available_parallelism,
    production_iishanten_continuation_settings,
    select_discard_action_with_iishanten_continuation_settings,
};
use crate::iishanten_selection_depth_comparison::measured_on_a_fresh_thread;

/// Full 追加評価の対象になる provisional 候補数。この診断でも production でも2を超えない。
const FULL_PAIR_LEN: usize = 2;

/// gate を通った上位2候補の Full 追加評価の分け方。他は production のまま。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TwoShantenFullParallelism {
    /// S: 2候補を1本の探索基盤で順に評価する baseline。
    Sequential,
    /// P2: 2候補を最大2 thread で並行に評価する。現在の production と同じ方式。
    Parallel,
}

impl TwoShantenFullParallelism {
    /// 比較する方式。S が baseline、P2 が production の方式。
    pub const ALL: [Self; 2] = [Self::Sequential, Self::Parallel];

    pub fn label(self) -> String {
        match self {
            Self::Sequential => {
                "S sequential (production selection, the gated top-2 evaluated in order)"
                    .to_string()
            }
            Self::Parallel => format!(
                "P2 pair-level parallel (up to min(2, available_parallelism = {}) workers, the \
                 current production configuration)",
                available_parallelism(),
            ),
        }
    }

    /// 要求する worker 数の上限。実際に使う数は Full 対象の2候補で頭打ちになり、並列度 1 の
    /// 環境では逐次評価へ落ちる。
    pub fn requested_workers(self) -> usize {
        match self {
            Self::Sequential => 1,
            Self::Parallel => available_parallelism().min(FULL_PAIR_LEN),
        }
    }

    // 探索そのものの設定は production のままで、Full 追加評価の分け方だけを差し替える。
    fn continuation(self) -> IishantenContinuationSettings {
        let production = production_iishanten_continuation_settings();
        match self {
            Self::Sequential => IishantenContinuationSettings {
                two_shanten_full_workers: None,
                ..production
            },
            Self::Parallel => production,
        }
    }

    fn timing_settings(self) -> IishantenContinuationSettings {
        IishantenContinuationSettings {
            search_stats: false,
            ..self.continuation()
        }
    }

    fn observation_settings(self) -> IishantenContinuationSettings {
        IishantenContinuationSettings {
            search_stats: true,
            ..self.continuation()
        }
    }
}

/// 選択が実際に使った2向聴候補1件分の値。表示のために比較をやり直さない。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TwoShantenFullParallelCandidate {
    pub discard: TileType,
    /// ForwardTargets cohort の Progress 寄与。cohort 外は `None`。
    pub progress_self_tsumo_value: Option<u64>,
    /// ドラ差 gate を通った上位2候補だけが持つ Full `ExpectedSelfTsumoValue`。
    pub expected_self_tsumo_value: Option<u64>,
    pub selected: bool,
    pub comparison_reason: DiscardComparisonReason,
}

/// 1局面を1方式で1回選択した run。計測 run と観測 run は同じ型。
#[derive(Debug, Clone)]
pub struct TwoShantenFullParallelRun {
    pub selected: Option<LegalAction>,
    /// 全合法候補。順序は既存 selection の候補順そのもの。
    pub candidates: Vec<TwoShantenFullParallelCandidate>,
    /// 診断の構築を含まない、打牌選択1回の実測時間。
    pub elapsed: Duration,
    /// phase 別の内訳。phase timer を持たない計測 run では 0 のまま。
    pub phases: NormalDiscardPhaseDurations,
    /// production comparator が評価した候補ごとの実測。Progress cohort のあとに Full の2候補が
    /// 続くため、gate を通った牌種は2回現れる。並行評価した2候補は同時に走るので、この2件の
    /// 合計は phase の `two_shanten_self_tsumo` を超え得る。
    pub two_shanten_self_tsumo_candidates: Vec<(TileType, Duration)>,
    /// 探索規模。計上しない計測 run では 0 のまま。
    pub search: ThreeShantenSearchStats,
    /// 探索内の同一 state memo の利用数。2向聴局面では memo を持たないため 0 のまま。
    pub memo: SearchStateMemoStats,
    /// Full 追加評価に実際に使った thread 数。逐次評価と gate 不発では 1。
    pub full_workers: usize,
}

impl TwoShantenFullParallelRun {
    pub fn selected_discard(&self) -> Option<TileType> {
        match self.selected {
            Some(LegalAction::Dahai { tile }) => Some(tile.tile_type()),
            _ => None,
        }
    }

    /// ドラ差 gate を通って Full 値を確定した候補。production では最大2件。
    pub fn full_evaluated_candidates(&self) -> Vec<TileType> {
        self.candidates
            .iter()
            .filter(|candidate| candidate.expected_self_tsumo_value.is_some())
            .map(|candidate| candidate.discard)
            .collect()
    }

    /// Progress 寄与を確定した候補。ForwardTargets cohort そのもの。
    pub fn progress_evaluated_candidates(&self) -> Vec<TileType> {
        self.candidates
            .iter()
            .filter(|candidate| candidate.progress_self_tsumo_value.is_some())
            .map(|candidate| candidate.discard)
            .collect()
    }
}

/// 1局面を1方式で選択した結果。時間は計測 run から、探索の内訳は観測 run から取る。
#[derive(Debug, Clone)]
pub struct TwoShantenFullParallelDecision {
    pub parallelism: TwoShantenFullParallelism,
    pub timing: TwoShantenFullParallelRun,
    pub observation: TwoShantenFullParallelRun,
}

impl TwoShantenFullParallelDecision {
    /// instrumentation を含まない打牌選択1回の実測時間。
    pub fn elapsed(&self) -> Duration {
        self.timing.elapsed
    }

    pub fn selected(&self) -> Option<&LegalAction> {
        self.timing.selected.as_ref()
    }

    pub fn selected_discard(&self) -> Option<TileType> {
        self.timing.selected_discard()
    }

    /// Full 追加評価に実際に使った thread 数。
    pub fn full_workers(&self) -> usize {
        self.timing.full_workers
    }

    /// 計測 run と観測 run が同じ選択・同じ候補の値になったか。
    pub fn runs_agree(&self) -> bool {
        self.timing.selected == self.observation.selected
            && self.timing.candidates == self.observation.candidates
    }

    pub fn search(&self) -> &ThreeShantenSearchStats {
        &self.observation.search
    }

    pub fn memo(&self) -> &SearchStateMemoStats {
        &self.observation.memo
    }
}

/// 同じ局面を S / P2 で1回ずつ選択した比較結果。
#[derive(Debug, Clone)]
pub struct TwoShantenFullParallelComparison {
    pub sequential: TwoShantenFullParallelDecision,
    pub parallel: TwoShantenFullParallelDecision,
}

impl TwoShantenFullParallelComparison {
    pub fn decisions(&self) -> impl Iterator<Item = &TwoShantenFullParallelDecision> {
        [&self.sequential, &self.parallel].into_iter()
    }

    /// P2 が S と bit-exact に一致したか。
    ///
    /// 比べるのは2候補の Full 値・selected discard・比較理由・全候補の値で、どれも選択が実際に
    /// 使ったもの。
    pub fn parallel_matches_sequential(&self) -> bool {
        self.parallel.timing.selected == self.sequential.timing.selected
            && self.parallel.timing.candidates == self.sequential.timing.candidates
            && self.parallel.observation.selected == self.sequential.observation.selected
            && self.parallel.observation.candidates == self.sequential.observation.candidates
    }

    /// S / P2 の比。P2 が S の何倍速いか。
    pub fn speedup(&self) -> Option<f64> {
        let elapsed = self.parallel.elapsed().as_secs_f64();
        (elapsed > 0.0).then(|| self.sequential.elapsed().as_secs_f64() / elapsed)
    }
}

/// 同じ局面について、production の2向聴 selection を S / P2 で1回ずつ行う。
///
/// 評価順は固定だが、どの方式も自分専用の thread で計るため、先に走った方式が後の方式の
/// thread-local memo を暖めることはない。値も選択も評価順に依らない。
pub fn compare_two_shanten_full_parallelism(
    context: &GameContext,
    legal_actions: &[LegalAction],
) -> TwoShantenFullParallelComparison {
    TwoShantenFullParallelComparison {
        sequential: decide_with_two_shanten_full_parallelism(
            context,
            legal_actions,
            TwoShantenFullParallelism::Sequential,
        ),
        parallel: decide_with_two_shanten_full_parallelism(
            context,
            legal_actions,
            TwoShantenFullParallelism::Parallel,
        ),
    }
}

/// 指定した分け方で production の打牌選択を計測 run と観測 run の2回行う。
pub fn decide_with_two_shanten_full_parallelism(
    context: &GameContext,
    legal_actions: &[LegalAction],
    parallelism: TwoShantenFullParallelism,
) -> TwoShantenFullParallelDecision {
    let observation = measured_on_a_fresh_thread(|| {
        run(
            context,
            legal_actions,
            parallelism.observation_settings(),
            NormalDiscardPhaseTimer::started(),
        )
    });
    let timing = measured_on_a_fresh_thread(|| {
        run(
            context,
            legal_actions,
            parallelism.timing_settings(),
            NormalDiscardPhaseTimer::disabled(),
        )
    });
    TwoShantenFullParallelDecision {
        parallelism,
        timing,
        observation,
    }
}

/// 指定した分け方で production の打牌選択を1回だけ行う。
///
/// instrumentation を一切持たない run なので、`search` / `memo` / `phases` は既定値のまま。
/// 値と選択だけを確かめる回帰で、比較のために同じ探索を4回走らせないための入口。
pub fn select_with_two_shanten_full_parallelism(
    context: &GameContext,
    legal_actions: &[LegalAction],
    parallelism: TwoShantenFullParallelism,
) -> TwoShantenFullParallelRun {
    measured_on_a_fresh_thread(|| {
        run(
            context,
            legal_actions,
            parallelism.timing_settings(),
            NormalDiscardPhaseTimer::disabled(),
        )
    })
}

fn run(
    context: &GameContext,
    legal_actions: &[LegalAction],
    continuation: IishantenContinuationSettings,
    timing: NormalDiscardPhaseTimer,
) -> TwoShantenFullParallelRun {
    let observed = select_discard_action_with_iishanten_continuation_settings(
        context,
        legal_actions,
        continuation,
        timing,
    );
    TwoShantenFullParallelRun {
        selected: observed.selection.action.clone(),
        candidates: candidates_from_observation(&observed),
        elapsed: observed.elapsed,
        phases: observed.phases,
        two_shanten_self_tsumo_candidates: observed
            .two_shanten_self_tsumo_candidates
            .iter()
            .map(|candidate| (candidate.discard, candidate.elapsed))
            .collect(),
        search: observed.search,
        memo: observed.memo,
        full_workers: observed.two_shanten_full_workers,
    }
}

// 候補の値も比較理由も選択が使ったものそのままで、表示のために比較をやり直さない。
fn candidates_from_observation(
    observed: &IishantenContinuationSelection,
) -> Vec<TwoShantenFullParallelCandidate> {
    observed
        .diagnostic
        .candidates
        .iter()
        .map(|candidate| TwoShantenFullParallelCandidate {
            discard: candidate.evaluation.discard,
            progress_self_tsumo_value: candidate.two_shanten_progress_self_tsumo_value,
            expected_self_tsumo_value: candidate.two_shanten_expected_self_tsumo_value,
            selected: candidate.selected,
            comparison_reason: candidate.comparison_reason,
        })
        .collect()
}

/// この runtime で P2 が実際に thread を分けられるか。並列度 1 の環境では S と同じ経路になる。
pub fn two_shanten_full_parallelism_is_available() -> bool {
    available_parallelism() >= FULL_PAIR_LEN
}

#[cfg(test)]
mod tests {
    use std::num::NonZeroUsize;

    use super::*;
    use crate::discard_selection::production_iishanten_continuation_settings;

    // production の worker 決定。Full 対象は常に2候補なので上限も2で、並列度 1 の環境では
    // 逐次評価へ落ちる。固定で2 thread を起こす実装ではないことをここで固定する。
    #[test]
    fn the_production_worker_limit_is_the_runtime_parallelism_capped_by_the_full_pair() {
        let workers = production_iishanten_continuation_settings().two_shanten_full_workers;

        match available_parallelism() {
            1 => assert_eq!(workers, None),
            _ => assert_eq!(workers.map(NonZeroUsize::get), Some(FULL_PAIR_LEN)),
        }
        assert_eq!(
            TwoShantenFullParallelism::Sequential
                .continuation()
                .two_shanten_full_workers,
            None,
        );
        assert_eq!(
            TwoShantenFullParallelism::Parallel
                .continuation()
                .two_shanten_full_workers,
            workers,
        );
        assert!(TwoShantenFullParallelism::Parallel.requested_workers() <= FULL_PAIR_LEN);
        assert_eq!(
            two_shanten_full_parallelism_is_available(),
            workers.is_some(),
        );
    }
}
