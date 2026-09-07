//! 3向聴 Progress-only self-tsumo の diagnostics 専用計測。

use std::time::{Duration, Instant};

use bot_logic::{TileType, three_shanten_progress_self_tsumo_value_for_candidate};

use crate::action::LegalAction;
use crate::context::GameContext;
use crate::discard_selection::{
    LookaheadDiagnosticScope, legal_discard_evaluations, lookahead_inputs,
};
use crate::prospective_value::ProductionProspectiveValuator;

/// 全合法3向聴候補の値と実測時間。unknown は `None` のまま保持する。
#[derive(Debug, Clone)]
pub struct ThreeShantenProgressSelfTsumoCost {
    pub memo: bot_logic::ProgressMemoStats,
    pub candidates: Vec<(TileType, Option<u64>, Duration)>,
    /// 入力構築を除く全候補の評価時間。候補間では既存 memo を共有する。
    pub total: Duration,
}

/// 通常打牌と同じ入力・valuator で3向聴候補を評価する。production selection は呼ばない。
pub fn measure_three_shanten_progress_self_tsumo(
    context: &GameContext,
    legal_actions: &[LegalAction],
) -> ThreeShantenProgressSelfTsumoCost {
    let legal = legal_discard_evaluations(context, legal_actions);
    let valuator = ProductionProspectiveValuator::new(context);
    let inputs = lookahead_inputs(
        context,
        &legal.tiles,
        &valuator,
        LookaheadDiagnosticScope::TWO_SHANTEN_SELF_TSUMO,
    )
    .with_three_shanten_progress_memo();
    let started = Instant::now();
    let candidates = legal
        .evaluations
        .iter()
        .filter(|evaluation| evaluation.min_shanten_after_discard() == 3)
        .map(|evaluation| {
            let started = Instant::now();
            let value = three_shanten_progress_self_tsumo_value_for_candidate(&inputs, evaluation);
            (evaluation.discard, value, started.elapsed())
        })
        .collect();
    ThreeShantenProgressSelfTsumoCost {
        memo: inputs.three_shanten_progress_memo_stats(),
        candidates,
        total: started.elapsed(),
    }
}
