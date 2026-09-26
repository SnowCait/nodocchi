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
//! # 将来自摸機会の表示
//!
//! 通常打牌後・Call 後の将来自摸機会 (baseline) は `floor(remaining_tiles / 4)` だが、Chi / Pon
//! への反応 request で鳴き判断が評価する Pass 側の continuation は、反応元の席から自分の次の
//! 自摸までの位置で別の自摸回数を使う。同じ request で両者が異なり得るので、baseline と Pass 側を
//! 別の値として持つ。どちらも production の helper ([`own_future_draws`] /
//! [`pass_own_future_draws`] と [`effective_own_future_draws`]) から読み、式を複製しない。
//!
//! Pass 側の値を持つのは、production の鳴き判断が実際に Call / Pass self-tsumo 比較まで進めた
//! 候補を持つ request だけ。判定は [`ShantenAgent::decide`] が返した鳴き判断の候補
//! ([`CallCandidateDiagnostic`] の `iishanten_self_tsumo` / `two_shanten_self_tsumo` /
//! `three_shanten_self_tsumo`) をそのまま読み、候補の準備も向聴判定も評価し直さない。
//!
//! # 計測条件
//!
//! 向聴・受け入れ・一向聴形の memo は thread-local なので、horizon ごとの判断は必ず新しい
//! thread で行い、どの horizon も同じ cold な memo から始める。先に評価した horizon の memo を
//! 後の horizon が共有することはない。

use bot_logic::SelfTsumoHorizon;

use crate::action::LegalAction;
use crate::agents::{AgentActionSource, ShantenAgent};
use crate::call_decision::{
    CallCandidateDiagnostic, CallDecisionDiagnostic, pass_own_future_draws,
};
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

/// 反応 request の Pass 側 continuation が使う将来自摸機会。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PassFutureDraws {
    /// production の鳴き判断が Call / Pass self-tsumo 比較を行わなかった request。Chi / Pon が
    /// 無い request のほか、即テンパイ Call しか無く Pass continuation を必要としなかった request も
    /// 含む。
    NotApplicable,
    /// Call / Pass self-tsumo 比較の対象候補はあるが、反応元の席や残り山が観測できず、production も
    /// Pass 側の自摸回数を確定できない。
    Unknown,
    Known(u32),
    /// horizon によって Pass continuation の対象かどうかが分かれた。鳴き判断の対象判定は horizon に
    /// 依らないので起こらないはずだが、起きた場合はどれか1つを採用せずこの状態で表す。
    DiffersAcrossHorizons,
}

impl PassFutureDraws {
    fn from_production(draws: Option<u32>) -> Self {
        draws.map_or(Self::Unknown, Self::Known)
    }
}

// production の鳴き判断が Call / Pass self-tsumo 比較まで進めた候補を持つか。候補の各 field は
// production がその比較へ進んだ場合だけ書き込むもので、ここでは読むだけにする。
fn evaluates_pass_continuation(call: Option<&CallDecisionDiagnostic>) -> bool {
    call.is_some_and(|call| {
        call.candidates
            .iter()
            .any(compares_call_with_pass_continuation)
    })
}

fn compares_call_with_pass_continuation(candidate: &CallCandidateDiagnostic) -> bool {
    candidate.iishanten_self_tsumo.is_some()
        || candidate.two_shanten_self_tsumo.is_some()
        || candidate.three_shanten_self_tsumo.is_some()
}

// Pass 側の raw 自摸回数と、Pass continuation の lookahead 入力と同じ入口で context の soft
// horizon を適用した値。
fn pass_future_draws(
    context: &GameContext,
    evaluates_pass_continuation: bool,
) -> (PassFutureDraws, PassFutureDraws) {
    if !evaluates_pass_continuation {
        return (
            PassFutureDraws::NotApplicable,
            PassFutureDraws::NotApplicable,
        );
    }
    let raw = pass_own_future_draws(context);
    (
        PassFutureDraws::from_production(raw),
        PassFutureDraws::from_production(effective_own_future_draws(context, raw)),
    )
}

// raw の Pass 側自摸回数は horizon に依らないので、4つの horizon で一致した値を request の値にする。
fn common_pass_raw_future_draws(decisions: &[SelfTsumoHorizonDecision; 4]) -> PassFutureDraws {
    let first = decisions[0].pass_raw_future_draws;
    if decisions
        .iter()
        .all(|decision| decision.pass_raw_future_draws == first)
    {
        first
    } else {
        PassFutureDraws::DiffersAcrossHorizons
    }
}

/// 1つの horizon で production と同じ判断経路を通した結果。
///
/// 値はどれも production の判断が持っていたものの転記で、比較のために求め直さない。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SelfTsumoHorizonDecision {
    pub horizon: SelfTsumoHorizon,
    /// 通常打牌後・Call 後 (baseline) の将来自摸機会へこの horizon を適用した値。残り山が
    /// unknown なら `None`。
    pub baseline_effective_future_draws: Option<u32>,
    /// この horizon の判断で production が Pass continuation を評価した場合の、horizon を適用する
    /// 前の Pass 側の将来自摸機会。
    pub pass_raw_future_draws: PassFutureDraws,
    /// 反応 request の Pass 側の将来自摸機会へこの horizon を適用した値。
    pub pass_effective_future_draws: PassFutureDraws,
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
    /// 通常打牌後・Call 後 (baseline) の horizon を適用する前の流局までの将来自摸機会
    /// `floor(remaining_tiles / 4)`。残り山が unknown なら `None`。局面の時期の分類はこの値で行う。
    pub baseline_raw_future_draws: Option<u32>,
    /// 反応 request の Pass 側の horizon を適用する前の将来自摸機会。各 horizon の値が一致しない
    /// 場合は [`PassFutureDraws::DiffersAcrossHorizons`]。
    pub pass_raw_future_draws: PassFutureDraws,
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
    let decisions = COMPARED_SELF_TSUMO_HORIZONS
        .map(|horizon| decide_with_self_tsumo_horizon(context, legal_actions, horizon));
    SelfTsumoHorizonComparison {
        baseline_raw_future_draws: own_future_draws(context),
        pass_raw_future_draws: common_pass_raw_future_draws(&decisions),
        decisions,
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
    let (pass_raw_future_draws, pass_effective_future_draws) = pass_future_draws(
        &context,
        evaluates_pass_continuation(decision.call.as_ref()),
    );
    SelfTsumoHorizonDecision {
        horizon,
        baseline_effective_future_draws: effective_own_future_draws(
            &context,
            own_future_draws(&context),
        ),
        pass_raw_future_draws,
        pass_effective_future_draws,
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
    use crate::meld::{Meld, MeldKind};
    use crate::shanten_test_support::{tenpai_actions, tenpai_context, tile};
    use bot_logic::TileType;

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

        // 通常ツモ番の baseline は従来どおり floor(remaining_tiles / 4)。鳴き判断は無く、Pass は
        // 存在しない。
        assert!(ShantenAgent.decide(&context, &actions).call.is_none());
        assert_eq!(comparison.baseline_raw_future_draws, Some(10));
        assert_eq!(
            comparison.baseline_raw_future_draws,
            own_future_draws(&context)
        );
        assert_eq!(
            comparison.pass_raw_future_draws,
            PassFutureDraws::NotApplicable
        );
        for decision in &comparison.decisions {
            assert_eq!(
                decision.pass_effective_future_draws,
                PassFutureDraws::NotApplicable
            );
        }
        assert_eq!(
            comparison
                .decisions
                .each_ref()
                .map(|decision| decision.baseline_effective_future_draws),
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

        assert_eq!(comparison.baseline_raw_future_draws, None);
        for decision in &comparison.decisions {
            assert_eq!(decision.baseline_effective_future_draws, None);
        }
    }

    // 鳴き判断の既存 test と同じ Pon 反応局面。東家 (player 0) が反応する。
    struct ReactionFixture {
        hand: &'static [u8],
        melds: fn() -> Vec<Meld>,
        target: u8,
        consumed: [u8; 2],
    }

    // 234m 6m 8m 6p 8p 24s E FF の1向聴。F を Pon しても1向聴のまま。
    const IISHANTEN_PON: ReactionFixture = ReactionFixture {
        hand: &[4, 8, 12, 17, 20, 24, 56, 64, 76, 84, 108, 128, 129],
        melds: Vec::new,
        target: 130,
        consumed: [128, 129],
    };

    // 123456m 55p 78s N PP の1向聴。PP を Pon して N を切ると即テンパイ。
    const TENPAI_PON: ReactionFixture = ReactionFixture {
        hand: &[0, 4, 8, 12, 17, 20, 53, 54, 96, 100, 120, 124, 125],
        melds: Vec::new,
        target: 126,
        consumed: [124, 125],
    };

    // 白 Pon + 發 Pon の2副露。どの完成形にも役があるので、比較が役の有無で動かない。
    fn two_value_pons() -> Vec<Meld> {
        vec![
            Meld::new(
                MeldKind::Pon,
                vec![tile(124), tile(125), tile(126)],
                Some(tile(124)),
            ),
            Meld::new(
                MeldKind::Pon,
                vec![tile(128), tile(129), tile(130)],
                Some(tile(128)),
            ),
        ]
    }

    // 既存2副露 + CC 55p E S W の2向聴。C を Pon した後、最良打牌で1向聴になる。
    const TWO_SHANTEN_PON: ReactionFixture = ReactionFixture {
        hand: &[132, 133, 52, 53, 108, 112, 116],
        melds: two_value_pons,
        target: 134,
        consumed: [132, 133],
    };

    // 既存2副露 + CC 5p E S W N の3向聴。C を Pon した後、最良打牌で2向聴になる。
    const THREE_SHANTEN_PON: ReactionFixture = ReactionFixture {
        hand: &[132, 133, 53, 108, 112, 116, 120],
        melds: two_value_pons,
        target: 134,
        consumed: [132, 133],
    };

    fn reaction_context(
        fixture: &ReactionFixture,
        source: Option<u8>,
        remaining_tiles: Option<u32>,
    ) -> (GameContext, Vec<LegalAction>) {
        let hand: Vec<_> = fixture.hand.iter().map(|&value| tile(value)).collect();
        let melds = (fixture.melds)();
        let mut visible = hand.clone();
        visible.push(tile(fixture.target));
        visible.extend(melds.iter().flat_map(|meld| meld.tiles().iter().copied()));
        let east = TileType::new(27);
        let context = GameContext::from_parts_with_melds(
            None,
            hand,
            vec![],
            east,
            east,
            visible,
            Some(0),
            Some(0),
            [vec![], vec![tile(fixture.target)], vec![], vec![]],
            [false; 4],
            [melds, vec![], vec![], vec![]],
        )
        .with_history_furiten_facts(bot_logic::HistoryFuritenFacts {
            same_turn: Some(false),
            riichi_missed_win: Some(false),
        })
        .with_reaction_source_player(source)
        .with_table_state_facts(TableStateFacts {
            remaining_tiles,
            ..TableStateFacts::default()
        });
        let actions = vec![
            LegalAction::Pon {
                tile: tile(fixture.target),
                consumed: fixture.consumed.iter().map(|&value| tile(value)).collect(),
            },
            LegalAction::None,
        ];
        (context, actions)
    }

    // production の鳴き判断が持つ候補。比較と同じ判断経路の結果で、この test の確認用。
    fn production_call(context: &GameContext, actions: &[LegalAction]) -> CallDecisionDiagnostic {
        ShantenAgent
            .decide(context, actions)
            .call
            .expect("Chi / Pon の候補を評価する")
    }

    // 4つの horizon の Pass 側 raw 自摸回数は horizon に依らず一致し、request の値と同じになる。
    fn assert_the_pass_side_does_not_depend_on_the_horizon(
        comparison: &SelfTsumoHorizonComparison,
    ) {
        assert_ne!(
            comparison.pass_raw_future_draws,
            PassFutureDraws::DiffersAcrossHorizons
        );
        for decision in &comparison.decisions {
            assert_eq!(
                decision.pass_raw_future_draws,
                comparison.pass_raw_future_draws
            );
        }
    }

    // 表示用の値を足しても各 horizon の判断は production の act() のまま。
    fn assert_the_decisions_follow_production(
        comparison: &SelfTsumoHorizonComparison,
        context: &GameContext,
        actions: &[LegalAction],
    ) {
        for decision in &comparison.decisions {
            let configured = context.clone().with_self_tsumo_horizon(decision.horizon);
            assert_eq!(decision.action, ShantenAgent.act(&configured, actions));
            let production = ShantenAgent.decide(&configured, actions);
            assert_eq!(decision.source, production.source);
            assert_eq!(decision.normal_discard, production.normal_discard);
        }
    }

    #[test]
    fn a_reaction_request_keeps_the_pass_draws_apart_from_the_baseline() {
        // 1向聴 -> Pon -> 1向聴。反応元は下家 (player 1)。残り 63 枚では baseline は
        // floor(63 / 4) = 15、Pass 側は 1 + (63 - 3) / 4 = 16。
        let (context, actions) = reaction_context(&IISHANTEN_PON, Some(1), Some(63));
        assert!(
            production_call(&context, &actions)
                .candidates
                .iter()
                .all(|candidate| candidate.iishanten_self_tsumo.is_some())
        );
        let comparison = compare_self_tsumo_horizons(&context, &actions);

        assert_eq!(comparison.baseline_raw_future_draws, Some(15));
        assert_eq!(comparison.pass_raw_future_draws, PassFutureDraws::Known(16));
        assert_eq!(
            comparison.pass_raw_future_draws,
            PassFutureDraws::Known(pass_own_future_draws(&context).unwrap())
        );
        assert_eq!(
            comparison
                .decisions
                .each_ref()
                .map(|decision| decision.baseline_effective_future_draws),
            [Some(9), Some(11), Some(13), Some(15)]
        );
        assert_eq!(
            comparison
                .decisions
                .each_ref()
                .map(|decision| decision.pass_effective_future_draws),
            [10, 12, 14, 16].map(PassFutureDraws::Known)
        );
        for decision in &comparison.decisions {
            assert_eq!(
                decision.pass_effective_future_draws,
                PassFutureDraws::Known(decision.horizon.effective_future_draws(16))
            );
        }
        assert_the_pass_side_does_not_depend_on_the_horizon(&comparison);
        assert_the_decisions_follow_production(&comparison, &context, &actions);
    }

    #[test]
    fn an_immediate_tenpai_call_does_not_make_the_pass_side_applicable() {
        // Pon は合法で production は Call 候補を評価するが、即テンパイ Call なので Call / Pass
        // self-tsumo 比較へは進まない。
        let (context, actions) = reaction_context(&TENPAI_PON, Some(1), Some(63));
        let call = production_call(&context, &actions);
        assert!(!call.candidates.is_empty());
        for candidate in &call.candidates {
            assert_eq!(candidate.post_call_shanten(), Some(0));
            assert!(!compares_call_with_pass_continuation(candidate));
        }
        let comparison = compare_self_tsumo_horizons(&context, &actions);

        assert_eq!(comparison.baseline_raw_future_draws, Some(15));
        assert_eq!(
            comparison.pass_raw_future_draws,
            PassFutureDraws::NotApplicable
        );
        for decision in &comparison.decisions {
            assert_eq!(
                decision.pass_effective_future_draws,
                PassFutureDraws::NotApplicable
            );
        }
        assert_the_pass_side_does_not_depend_on_the_horizon(&comparison);
        assert_the_decisions_follow_production(&comparison, &context, &actions);
    }

    #[test]
    fn the_two_and_three_shanten_call_comparisons_make_the_pass_side_applicable() {
        for (fixture, compares) in [
            (
                &TWO_SHANTEN_PON,
                (|candidate| candidate.two_shanten_self_tsumo.is_some())
                    as fn(&CallCandidateDiagnostic) -> bool,
            ),
            (&THREE_SHANTEN_PON, |candidate| {
                candidate.three_shanten_self_tsumo.is_some()
            }),
        ] {
            let (context, actions) = reaction_context(fixture, Some(1), Some(48));
            let call = production_call(&context, &actions);
            assert!(call.candidates.iter().all(compares));
            assert!(
                call.candidates
                    .iter()
                    .all(|candidate| candidate.iishanten_self_tsumo.is_none())
            );
            let comparison = compare_self_tsumo_horizons(&context, &actions);

            // 残り 48 枚。baseline は 12、Pass 側は 1 + (48 - 3) / 4 = 12。
            assert_eq!(comparison.baseline_raw_future_draws, Some(12));
            assert_eq!(comparison.pass_raw_future_draws, PassFutureDraws::Known(12));
            for decision in &comparison.decisions {
                assert_eq!(
                    decision.pass_effective_future_draws,
                    PassFutureDraws::Known(decision.horizon.effective_future_draws(12))
                );
            }
            assert_the_pass_side_does_not_depend_on_the_horizon(&comparison);
            assert_the_decisions_follow_production(&comparison, &context, &actions);
        }
    }

    #[test]
    fn an_unknown_reaction_source_or_wall_leaves_the_applicable_pass_side_unknown() {
        for (source, remaining_tiles) in [(None, Some(63)), (Some(1), None)] {
            let (context, actions) = reaction_context(&IISHANTEN_PON, source, remaining_tiles);
            // Call / Pass self-tsumo 比較の対象候補はあり、Pass 側は not applicable に落とさない。
            assert!(
                production_call(&context, &actions)
                    .candidates
                    .iter()
                    .any(compares_call_with_pass_continuation)
            );
            let comparison = compare_self_tsumo_horizons(&context, &actions);

            assert_eq!(comparison.pass_raw_future_draws, PassFutureDraws::Unknown);
            for decision in &comparison.decisions {
                assert_eq!(
                    decision.pass_effective_future_draws,
                    PassFutureDraws::Unknown
                );
            }
            assert_the_pass_side_does_not_depend_on_the_horizon(&comparison);
        }
    }

    #[test]
    fn horizons_that_disagree_on_the_pass_side_are_not_merged_silently() {
        let (context, actions) = context(Some(40));
        let mut comparison = compare_self_tsumo_horizons(&context, &actions);
        comparison.decisions[3].pass_raw_future_draws = PassFutureDraws::Known(10);
        assert_eq!(
            common_pass_raw_future_draws(&comparison.decisions),
            PassFutureDraws::DiffersAcrossHorizons
        );
    }

    #[test]
    fn the_comparison_reads_the_draw_counts_from_the_production_helpers() {
        // 自摸回数の式は production の helper だけが持ち、比較側では組み立て直さない。
        let source = include_str!("self_tsumo_horizon_comparison.rs");
        let implementation: String = source
            .split("#[cfg(test)]")
            .next()
            .expect("実装部分がある")
            .lines()
            .filter(|line| !line.trim_start().starts_with("//"))
            .collect::<Vec<_>>()
            .join("\n");
        for duplicated in [
            "/ 4",
            "remaining_tiles()",
            "reaction_draw_distance",
            "soft_horizon_future_draws",
        ] {
            assert!(
                !implementation.contains(duplicated),
                "{duplicated} は production helper に任せる"
            );
        }
        // Pass continuation の対象かどうかは production の判断結果を読むだけで、鳴き候補の準備・
        // 向聴判定・Call 候補の評価を比較側でやり直さない。production の判断は horizon ごとに
        // decide() の1回だけ。
        for reevaluation in [
            "prepare_call_candidates",
            "pass_continuation_is_required",
            "evaluate_call_decision",
            "normalize_call",
            "calculate_shanten",
            "call_meld_and_concealed_tiles",
            "select_discard_action",
        ] {
            assert!(
                !implementation.contains(reevaluation),
                "{reevaluation} を比較側から呼ばない"
            );
        }
        assert_eq!(implementation.matches("ShantenAgent.decide(").count(), 1);
        assert!(implementation.contains("evaluates_pass_continuation(decision.call.as_ref())"));
        assert!(implementation.contains("pass_own_future_draws(context)"));
        assert!(implementation.contains("own_future_draws(context)"));
    }
}
