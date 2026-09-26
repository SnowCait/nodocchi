//! self-tsumo continuation の soft horizon を変えた場合の production 判断の比較。
//!
//! 比較する horizon は `horizon_turn` = 12 / 14 / 16 / 18 の4通りで、`late_min_future_draws` は
//! production と同じ 2 に固定する。各 horizon の判断は context の horizon だけを差し替えて
//! `ShantenAgent::act()` と同じ判断経路 ([`ShantenAgent::decide`]) を1回ずつ通したもので、
//! 比較専用の打牌選択・Call 判断は持たない。構造化診断は構築しない。
//!
//! どの horizon が正しいかを判定するものではなく、horizon を変えた場合に production 判断が
//! どの程度・どの局面で変わるかを観測するための診断である。1向聴 Push/Fold の固定 threshold と
//! 比較する値は horizon にかかわらず [`SelfTsumoHorizon::UNTIL_RYUKYOKU`] のままで、この比較は
//! その semantics を変えない。
//!
//! # 計測条件
//!
//! 向聴・受け入れ・一向聴形の memo は thread-local なので、horizon ごとの判断は必ず新しい
//! thread で行い、どの horizon も同じ cold な memo から始める。先に評価した horizon の memo を
//! 後の horizon が共有することはない。

use bot_logic::SelfTsumoHorizon;

use crate::action::LegalAction;
use crate::agents::{AgentActionSource, ShantenAgent};
use crate::context::GameContext;
use crate::discard_selection::{effective_own_future_draws, own_future_draws};
use crate::iishanten_selection_depth_comparison::measured_on_a_fresh_thread;
use crate::push_pull::{PushPullMode, PushPullOffenseState};

/// 比較する soft horizon の巡目。
pub const COMPARED_SELF_TSUMO_HORIZON_TURNS: [u32; 4] = [12, 14, 16, 18];

/// 比較する soft horizon。`late_min_future_draws` は production と同じ。
pub const COMPARED_SELF_TSUMO_HORIZONS: [SelfTsumoHorizon; 4] = [
    compared_horizon(COMPARED_SELF_TSUMO_HORIZON_TURNS[0]),
    compared_horizon(COMPARED_SELF_TSUMO_HORIZON_TURNS[1]),
    compared_horizon(COMPARED_SELF_TSUMO_HORIZON_TURNS[2]),
    compared_horizon(COMPARED_SELF_TSUMO_HORIZON_TURNS[3]),
];

const fn compared_horizon(horizon_turn: u32) -> SelfTsumoHorizon {
    SelfTsumoHorizon {
        horizon_turn,
        late_min_future_draws: SelfTsumoHorizon::PRODUCTION.late_min_future_draws,
    }
}

/// 1つの horizon で production と同じ判断経路を通した結果。
///
/// 値はどれも production の判断が持っていたものの転記で、比較のために求め直さない。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SelfTsumoHorizonDecision {
    pub horizon: SelfTsumoHorizon,
    /// この horizon を適用した将来自摸機会。残り山が unknown なら `None`。
    pub effective_future_draws: Option<u32>,
    /// 最終 action。`ShantenAgent::act()` の結果と一致する。
    pub action: LegalAction,
    pub source: AgentActionSource,
    /// 通常打牌選択が選んだ Dahai。通常打牌選択を通らなかった場合 (Hora / 九種九牌 / 鳴き /
    /// 確定 Fold の早期決着) は `None`。
    pub normal_discard: Option<LegalAction>,
    /// 押し引き判定の結論。押し引きまで進まなかった場合は `None`。
    pub push_pull_mode: Option<PushPullMode>,
    /// 通常打牌選択が選んだ打牌の offense state。押し引き入力として production が構築した
    /// ものそのもの。
    pub offense: Option<PushPullOffenseState>,
}

/// 1局面を4つの horizon で判断した結果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SelfTsumoHorizonComparison {
    /// horizon を適用する前の流局までの将来自摸機会。残り山が unknown なら `None`。
    pub raw_future_draws: Option<u32>,
    /// [`COMPARED_SELF_TSUMO_HORIZONS`] と同じ順序。
    pub decisions: [SelfTsumoHorizonDecision; 4],
}

impl SelfTsumoHorizonComparison {
    /// 4つの horizon で最終 action がすべて一致するか。
    pub fn all_final_actions_agree(&self) -> bool {
        self.decisions
            .iter()
            .all(|decision| decision.action == self.decisions[0].action)
    }

    /// 4つの horizon で通常打牌選択がすべて一致するか。通常打牌選択を通らなかった horizon は
    /// `None` 同士として比べる。
    pub fn all_normal_discards_agree(&self) -> bool {
        self.decisions
            .iter()
            .all(|decision| decision.normal_discard == self.decisions[0].normal_discard)
    }
}

/// 4つの horizon で production と同じ判断を行う。
pub fn compare_self_tsumo_horizons(
    context: &GameContext,
    legal_actions: &[LegalAction],
) -> SelfTsumoHorizonComparison {
    SelfTsumoHorizonComparison {
        raw_future_draws: own_future_draws(context),
        decisions: COMPARED_SELF_TSUMO_HORIZONS
            .map(|horizon| decide_with_self_tsumo_horizon(context, legal_actions, horizon)),
    }
}

/// context の soft horizon だけを差し替えて、production と同じ判断を新しい thread で1回行う。
pub fn decide_with_self_tsumo_horizon(
    context: &GameContext,
    legal_actions: &[LegalAction],
    horizon: SelfTsumoHorizon,
) -> SelfTsumoHorizonDecision {
    let context = context.clone().with_self_tsumo_horizon(horizon);
    let decision = measured_on_a_fresh_thread(|| ShantenAgent.decide(&context, legal_actions));
    SelfTsumoHorizonDecision {
        horizon,
        effective_future_draws: effective_own_future_draws(&context, own_future_draws(&context)),
        action: decision.action,
        source: decision.source,
        normal_discard: decision.normal_discard,
        push_pull_mode: decision.push_pull.map(|push_pull| push_pull.mode),
        offense: decision.push_pull_inputs.and_then(|inputs| inputs.offense),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::Agent;
    use crate::context::TableStateFacts;
    use crate::shanten_test_support::{tenpai_actions, tenpai_context};

    fn context(remaining_tiles: Option<u32>) -> (GameContext, Vec<LegalAction>) {
        let context = tenpai_context(&[]).with_table_state_facts(TableStateFacts {
            remaining_tiles,
            ..TableStateFacts::default()
        });
        (context, tenpai_actions())
    }

    #[test]
    fn the_compared_horizons_keep_the_production_late_minimum() {
        assert_eq!(
            COMPARED_SELF_TSUMO_HORIZONS.map(|horizon| horizon.horizon_turn),
            [12, 14, 16, 18]
        );
        for horizon in COMPARED_SELF_TSUMO_HORIZONS {
            assert_eq!(horizon.late_min_future_draws, 2);
        }
        assert_eq!(
            COMPARED_SELF_TSUMO_HORIZONS[0],
            SelfTsumoHorizon::PRODUCTION
        );
        assert_eq!(
            SelfTsumoHorizon::PRODUCTION,
            SelfTsumoHorizon {
                horizon_turn: 12,
                late_min_future_draws: 2,
            }
        );
    }

    #[test]
    fn each_horizon_follows_the_production_decision_path() {
        let (context, actions) = context(Some(40));
        let comparison = compare_self_tsumo_horizons(&context, &actions);

        assert_eq!(comparison.raw_future_draws, Some(10));
        assert_eq!(
            comparison
                .decisions
                .each_ref()
                .map(|decision| decision.effective_future_draws),
            [Some(4), Some(6), Some(8), Some(10)]
        );
        // 比較元の context は production の horizon のまま。
        assert_eq!(context.self_tsumo_horizon(), SelfTsumoHorizon::PRODUCTION);
        for decision in &comparison.decisions {
            let configured = context.clone().with_self_tsumo_horizon(decision.horizon);
            assert_eq!(decision.action, ShantenAgent.act(&configured, &actions));
            let diagnostic = ShantenAgent::diagnose(&configured, &actions);
            assert_eq!(decision.source, diagnostic.selected_source);
            assert_eq!(decision.normal_discard, diagnostic.normal_discard_action);
            assert_eq!(
                decision.push_pull_mode,
                diagnostic
                    .push_pull_decision
                    .map(|push_pull| push_pull.mode)
            );
            assert_eq!(
                decision.offense,
                diagnostic
                    .push_pull_inputs
                    .and_then(|inputs| inputs.offense)
            );
        }
    }

    #[test]
    fn an_unknown_wall_stays_unknown_for_every_horizon() {
        let (context, actions) = context(None);
        let comparison = compare_self_tsumo_horizons(&context, &actions);

        assert_eq!(comparison.raw_future_draws, None);
        for decision in &comparison.decisions {
            assert_eq!(decision.effective_future_draws, None);
        }
    }
}
