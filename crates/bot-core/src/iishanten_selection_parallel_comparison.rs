//! 1向聴の追加深度 B を、深く評価する候補ごとに並行評価した場合の wall-clock を観測する。
//!
//! 比べるのは同じ深度 B
//! ([`crate::iishanten_selection_depth_comparison::IishantenSelectionDepth::TwiceWithExactMemo`])
//! の中での評価の分け方だけで、深度も比較器も値も枝も scoring semantics も一切変えない。
//!
//! ```text
//! S:  候補1 → 候補2 → ... → 候補N        (現行 B)
//! P:  候補1 ─┐
//!     候補2 ─┤
//!     ...    ├→ 候補 index へ書き戻し → 既存の軸解決 / comparator
//!     候補N ─┘
//! ```
//!
//! 並行にするのは、深い前方評価の対象になった候補 ([`bot_logic::forward_target_mask`]) 1件分の
//! [`bot_logic::forward_metrics_for_candidate`] だけ。候補の絞り込みも、候補1件の評価も、
//! cohort 単位の unknown 軸解決も、比較順も、安定順序も、最終選択も production selection の
//! 経路そのままで、この module はそれらを複製しない。
//!
//! # exact である理由
//!
//! 候補1件の前方集計値は、その候補の打牌評価と探索設定だけで決まる純関数の値で、他候補の評価を
//! 入力にしない。探索基盤 (base 評価 memo・同一 state memo・thread-local の向聴 / 受け入れ
//! memo) は同じ入力に同じ値を返す cache でしかないため、どこまで共有できたかは値を変えない。
//! 候補の評価結果は候補 index へ書き戻すので、thread の終了順にも worker の数にも依らない。
//! したがって cohort・候補ごとの `ExpectedSelfTsumoValue`・unknown 軸の解決・選ばれた打牌・
//! 比較理由は S と P で bit-exact に一致する。
//!
//! # 増える総仕事量
//!
//! 逐次評価では1本の探索基盤を候補間で共有できる。候補を thread へ分けると worker ごとに
//! 基盤を作り直すため、wall-clock が縮む一方で base 評価・構造評価・terminal scoring・memo
//! miss は増える。速くなったかだけでなく、共有を失って総仕事量がどれだけ増えたかも併せて
//! 観測する。
//!
//! # 計測条件
//!
//! [`crate::iishanten_selection_depth_comparison`] と同じく、方式ごとに run を2本取る。
//! `elapsed` は探索規模の計上も phase timer も持たない計測 run のもので、cohort・値・stats は
//! 観測 run のもの。どちらの run も新しい thread で行うため、先に走った方式が後の方式の
//! thread-local memo を暖めない。
//!
//! production の打牌選択は A のままで、この module は B も並列評価も production へ接続しない。
//! threading はこの診断層だけが持ち、bot-logic の純粋な評価は platform threading を前提に
//! しない。

use std::num::NonZeroUsize;
use std::time::Duration;

use bot_logic::{SearchStateMemoStats, ThreeShantenSearchStats, TileType};

use crate::action::LegalAction;
use crate::context::GameContext;
use crate::decision_timing::NormalDiscardPhaseTimer;
use crate::discard_selection::IishantenContinuationSettings;
use crate::iishanten_selection_depth_comparison::{
    IishantenSelectionDepth, IishantenSelectionDepthRun, measured_on_a_fresh_thread,
    run_on_the_measuring_thread,
};

/// 深く評価する候補の分け方。深度も比較器も変わらず、変わるのはこの1点だけ。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IishantenSelectionParallelism {
    /// S: 現行 B。候補を1本の探索基盤で順に評価する。
    Sequential,
    /// P<N>: 候補を最大 N thread で並行に評価する。
    Workers(NonZeroUsize),
    /// PA: [`std::thread::available_parallelism`] を上限にする。
    AvailableParallelism,
}

impl IishantenSelectionParallelism {
    /// 比較する方式。S を基準に、worker 数を増やした方式を並べる。
    pub const ALL: [Self; 4] = [
        Self::Sequential,
        Self::Workers(NonZeroUsize::new(2).expect("2 は 0 でない")),
        Self::Workers(NonZeroUsize::new(4).expect("4 は 0 でない")),
        Self::AvailableParallelism,
    ];

    pub fn label(self) -> String {
        match self {
            Self::Sequential => "S sequential (current B)".to_string(),
            Self::Workers(workers) => {
                format!("P{workers} candidate-level parallel B (up to {workers} workers)")
            }
            Self::AvailableParallelism => format!(
                "PA candidate-level parallel B (up to available_parallelism = {})",
                available_parallelism(),
            ),
        }
    }

    /// 要求する worker 数の上限。実際に使う数は深く評価する候補数で頭打ちになる。
    pub fn requested_workers(self) -> usize {
        match self {
            Self::Sequential => 1,
            Self::Workers(workers) => workers.get(),
            Self::AvailableParallelism => available_parallelism(),
        }
    }

    // 探索そのものの設定。深度も memo も B のままで、候補評価の分け方だけを差し替える。
    fn continuation(self) -> IishantenContinuationSettings {
        let depth = IishantenSelectionDepth::TwiceWithExactMemo.continuation();
        match self {
            Self::Sequential => depth,
            Self::Workers(_) | Self::AvailableParallelism => IishantenContinuationSettings {
                forward_workers: NonZeroUsize::new(self.requested_workers()),
                ..depth
            },
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

fn available_parallelism() -> usize {
    std::thread::available_parallelism()
        .map(NonZeroUsize::get)
        .unwrap_or(1)
}

/// 1方式の計測結果。計測 run と観測 run の組は深度 A/B の観測と同じ。
#[derive(Debug, Clone)]
pub struct IishantenSelectionParallelDecision {
    pub parallelism: IishantenSelectionParallelism,
    /// instrumentation を持たない計測 run。`elapsed` はこの run のもの。
    pub timing: IishantenSelectionParallelRun,
    /// 探索規模の計上と phase timer を有効にした観測 run。cohort・値・stats はこの run のもの。
    pub observation: IishantenSelectionParallelRun,
}

/// 計測 run / 観測 run 1本。中身は深度 A/B の run そのもの。
pub type IishantenSelectionParallelRun = IishantenSelectionDepthRun;

impl IishantenSelectionParallelDecision {
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

    /// 深い候補評価に実際に使った thread 数。深く評価する候補数を超えない。
    pub fn workers(&self) -> usize {
        self.timing.forward_workers
    }

    /// 計測 run と観測 run が同じ選択・同じ cohort・同じ候補の値になったか。
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

/// 同じ局面・同じ深度 B を、候補評価の分け方だけ変えて計測した比較結果。
#[derive(Debug, Clone)]
pub struct IishantenSelectionParallelComparison {
    pub sequential: IishantenSelectionParallelDecision,
    /// 並行評価の方式。`IishantenSelectionParallelism::ALL` の順そのまま。
    pub parallel: Vec<IishantenSelectionParallelDecision>,
}

impl IishantenSelectionParallelComparison {
    pub fn decisions(&self) -> impl Iterator<Item = &IishantenSelectionParallelDecision> {
        std::iter::once(&self.sequential).chain(self.parallel.iter())
    }

    /// この方式が S と bit-exact に一致したか。
    ///
    /// 比べるのは選ばれた打牌と、候補ごとの deep 対象・確定した `ExpectedSelfTsumoValue`・
    /// 軸解決後の値・比較理由・選択そのもので、どれも選択が実際に使った値。
    pub fn matches_sequential(&self, decision: &IishantenSelectionParallelDecision) -> bool {
        decision.timing.selected == self.sequential.timing.selected
            && decision.timing.candidates == self.sequential.timing.candidates
            && decision.observation.selected == self.sequential.observation.selected
            && decision.observation.candidates == self.sequential.observation.candidates
    }

    /// 全方式が S と bit-exact に一致したか。
    pub fn every_mode_matches_sequential(&self) -> bool {
        self.parallel
            .iter()
            .all(|decision| self.matches_sequential(decision))
    }

    /// S / この方式の比。この方式が S の何倍速いか。
    pub fn speedup(&self, decision: &IishantenSelectionParallelDecision) -> Option<f64> {
        let elapsed = decision.elapsed().as_secs_f64();
        (elapsed > 0.0).then(|| self.sequential.elapsed().as_secs_f64() / elapsed)
    }
}

/// 同じ局面について、深度 B を S / P2 / P4 / PA で1回ずつ選択する。
///
/// 評価順は固定だが、どの方式も自分専用の thread で計るため、先に走った方式が後の方式の
/// thread-local memo を暖めることはない。値も選択も評価順に依らない。
pub fn compare_iishanten_selection_parallelism(
    context: &GameContext,
    legal_actions: &[LegalAction],
) -> IishantenSelectionParallelComparison {
    let mut decisions = IishantenSelectionParallelism::ALL
        .into_iter()
        .map(|parallelism| {
            decide_with_iishanten_selection_parallelism(context, legal_actions, parallelism)
        });
    let sequential = decisions.next().expect("S は必ずある");
    IishantenSelectionParallelComparison {
        sequential,
        parallel: decisions.collect(),
    }
}

/// 指定した分け方で深度 B の打牌選択を計測 run と観測 run の2回行う。
///
/// どちらの run も新しい thread の中で行うため、先に走った run が後の run の thread-local memo
/// を暖めない。観測 run を先に走らせるのも深度 A/B と同じで、`elapsed` は process が暖まった
/// 状態の値になる。
pub fn decide_with_iishanten_selection_parallelism(
    context: &GameContext,
    legal_actions: &[LegalAction],
    parallelism: IishantenSelectionParallelism,
) -> IishantenSelectionParallelDecision {
    let observation = measured_on_a_fresh_thread(|| {
        run_on_the_measuring_thread(
            context,
            legal_actions,
            parallelism.observation_settings(),
            NormalDiscardPhaseTimer::started(),
        )
    });
    let timing = measured_on_a_fresh_thread(|| {
        run_on_the_measuring_thread(
            context,
            legal_actions,
            parallelism.timing_settings(),
            NormalDiscardPhaseTimer::disabled(),
        )
    });
    IishantenSelectionParallelDecision {
        parallelism,
        timing,
        observation,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::iishanten_selection_depth_comparison::test_support::iishanten_context;

    // 対象 fixture の1件で全方式を1回ずつ通す。B の探索は重いので、exact であることも worker 数
    // の上限も同じ1本の比較から確かめ、方式を何度も走らせ直さない。
    #[test]
    fn every_parallel_mode_keeps_the_sequential_selection_and_values() {
        let (context, actions) = iishanten_context();
        let comparison = compare_iishanten_selection_parallelism(&context, &actions);

        // 選ばれた打牌・cohort・候補ごとの値・軸解決・比較理由まで S と一致する。
        assert!(comparison.every_mode_matches_sequential());
        assert_eq!(comparison.sequential.workers(), 1);

        let cohort = comparison
            .sequential
            .observation
            .deep_evaluated_candidates()
            .len();
        for decision in comparison.decisions() {
            let label = decision.parallelism.label();
            assert!(decision.runs_agree(), "{label}");
            assert_eq!(
                decision.selected_discard(),
                comparison.sequential.selected_discard(),
                "{label}",
            );
            // worker は深く評価する候補数を超えない。
            assert!(decision.workers() <= cohort, "{label}");
            assert!(
                decision.workers() <= decision.parallelism.requested_workers(),
                "{label}",
            );
        }
    }
}
