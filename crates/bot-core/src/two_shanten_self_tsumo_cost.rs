//! 2向聴候補の ExpectedSelfTsumoValue の実行コスト計測。
//!
//! 打牌選択にも押し引きにも一切使わない解析専用の入口で、production path はこの module を
//! 通らない。診断が使う入力・絞り込み・探索は [`crate::discard_selection`] と同じものを
//! そのまま呼び、計測のために別の評価器も別の探索も持たない。
//!
//! 対象候補の範囲は [`TwoShantenSelfTsumoScope`] がそのまま決める。production の比較対象へ
//! 絞る場合の判定は打牌選択が使う前方評価の絞り込みそのもので、ここに同じ条件を持たない。

use std::time::{Duration, Instant};

use bot_logic::{
    DiscardEvaluation, TileType, TwoShantenSelfTsumoDiagnostic, TwoShantenSelfTsumoObserver,
    TwoShantenSelfTsumoScope, diagnose_two_shanten_self_tsumo_instrumented,
};

use crate::action::LegalAction;
use crate::context::GameContext;
use crate::discard_selection::{
    LookaheadDiagnosticScope, legal_discard_evaluations, lookahead_inputs,
};
use crate::prospective_value::ProductionProspectiveValuator;

/// 打牌後が2向聴の向聴数。
const RYANSHANTEN_SHANTEN: i8 = 2;

/// 2向聴 ExpectedSelfTsumoValue の1回分の実測。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TwoShantenSelfTsumoCost {
    /// 求めた ExpectedSelfTsumoValue。`scope` の対象候補だけを持つ。
    pub diagnostic: TwoShantenSelfTsumoDiagnostic,
    /// 打牌後が2向聴の全候補数。`scope` によらず同じ値になる。
    pub two_shanten_candidates: usize,
    /// 対象候補の評価の合計。診断の入力を組み立てる時間は含まない。
    pub total: Duration,
    /// 対象候補ごとの実測。`diagnostic` と同じ候補・同じ順序。
    pub candidates: Vec<(TileType, Duration)>,
}

/// 通常打牌選択と同じ入力で2向聴 ExpectedSelfTsumoValue を1回求め、その実測時間を返す。
///
/// 値は [`bot_logic::diagnose_two_shanten_self_tsumo`] そのもので、計測の有無でも `scope` の
/// 違いでも、残った候補の値は変わらない。
pub fn measure_two_shanten_self_tsumo(
    context: &GameContext,
    legal_actions: &[LegalAction],
    scope: TwoShantenSelfTsumoScope,
) -> TwoShantenSelfTsumoCost {
    let legal = legal_discard_evaluations(context, legal_actions);
    let valuator = ProductionProspectiveValuator::new(context);
    let inputs = lookahead_inputs(
        context,
        &legal.tiles,
        &valuator,
        LookaheadDiagnosticScope::TWO_SHANTEN_SELF_TSUMO,
    );

    let mut timer = CandidateTimer::default();
    let started = Instant::now();
    let diagnostic = diagnose_two_shanten_self_tsumo_instrumented(
        &inputs,
        &legal.evaluations,
        scope,
        &mut timer,
    );
    let total = started.elapsed();

    TwoShantenSelfTsumoCost {
        diagnostic,
        two_shanten_candidates: two_shanten_candidate_count(&legal.evaluations),
        total,
        candidates: timer.finish(),
    }
}

fn two_shanten_candidate_count(evaluations: &[DiscardEvaluation]) -> usize {
    evaluations
        .iter()
        .filter(|evaluation| evaluation.min_shanten_after_discard() == RYANSHANTEN_SHANTEN)
        .count()
}

// 候補の区切りをそのまま実測へ変える。区切りを1つも受け取らなければ何も記録しない。
#[derive(Debug, Default)]
struct CandidateTimer {
    current: Option<(TileType, Instant)>,
    elapsed: Vec<(TileType, Duration)>,
}

impl CandidateTimer {
    // 最後の候補の区切りを閉じて結果を返す。
    fn finish(mut self) -> Vec<(TileType, Duration)> {
        self.flush();
        self.elapsed
    }

    fn flush(&mut self) {
        if let Some((discard, since)) = self.current.take() {
            self.elapsed.push((discard, since.elapsed()));
        }
    }
}

impl TwoShantenSelfTsumoObserver for CandidateTimer {
    fn enter_candidate(&mut self, discard: TileType) {
        self.flush();
        self.current = Some((discard, Instant::now()));
    }
}
