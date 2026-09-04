use super::*;

use crate::action::LegalAction;
use crate::agent::Agent;
use crate::agents::{AgentActionSource, ShantenAgent};
use crate::context::{GameContext, TableStateFacts};
use crate::defense::{DefenseFallbackKind, HonorSafetyRank};
use crate::discard_selection::{select_discard_action, select_discard_action_with_evaluation};
use crate::meld::{Meld, MeldKind};
use crate::push_pull::{
    PushPullMode, PushPullReason, decide_push_pull, push_pull_inputs_from_context_with_evaluation,
};
use crate::reach_policy::ReachDecisionReason;
use crate::ryukyoku_decision::tests::{KOKUSHI_FOUR_HAND, hand_tiles};
use crate::shanten_test_support::{
    OPPONENT_MELD_DRAW, OPPONENT_MELD_HAND, dahai, fold_actions, fold_under_reach_context,
    opponent_meld_actions, opponent_reach_context, opponent_reach_context_with_visible, pon_meld,
    suited_reach_context, suited_reach_context_with_reached, tenpai_actions, tenpai_context,
    tenpai_dahai_actions, tenpai_under_reach_context, tile, weak_tenpai_actions,
    weak_tenpai_under_reach_context,
};
use crate::threat::diagnose_player_threats;
use bot_logic::{
    DiscardComparisonReason, DiscardFuritenDiagnostic, DrawTransition, FixedMeldCount,
    PermanentFuriten, TenpaiWaitAvailability, TileId, TileType, compare_discard_evaluations,
};

// ---- 構造化診断 (ShantenAgent::diagnose) テスト ----
// 診断は act() と同じ selection logic を通るため、最終 action は常に act() と一致する。

// 診断の最終 action / source が act() と一致することを確認し、診断を返す共通 helper。
fn diagnose_matching_act(ctx: &GameContext, actions: &[LegalAction]) -> ShantenDecisionDiagnostic {
    let mut agent = ShantenAgent;
    let expected = agent.act(ctx, actions);
    let diagnostic = ShantenAgent::diagnose(ctx, actions);
    assert_eq!(diagnostic.selected_action, expected);
    assert_eq!(
        diagnostic.selected_source,
        agent.decide(ctx, actions).source
    );
    diagnostic
}

fn pinfu_tanyao_context_and_actions() -> (GameContext, Vec<LegalAction>) {
    const HAND: [&str; 13] = [
        "2m", "3m", "4m", "6m", "7m", "8m", "2p", "2p", "3s", "4s", "5s", "4s", "5s",
    ];

    let mut used = [false; 136];
    let mut allocate = |value: &str| {
        let tile_type = TileType::from_mjai_type_str(value).unwrap();
        let tile = TileId::copies(tile_type)
            .find(|tile| !tile.is_red() && !used[tile.index()])
            .expect("fixture does not reuse a physical tile");
        used[tile.index()] = true;
        tile
    };
    let hand: Vec<_> = HAND.iter().map(|value| allocate(value)).collect();
    let drawn = allocate("N");
    let visible = hand.iter().chain([&drawn]).copied().collect();
    let actions = hand
        .iter()
        .chain([&drawn])
        .map(|&tile| LegalAction::Dahai { tile })
        .chain([LegalAction::Reach])
        .collect();
    let context = GameContext::from_parts_with_table_state(
        Some(drawn),
        hand,
        vec![],
        TileType::from_mjai_type_str("E").ok(),
        TileType::from_mjai_type_str("S").ok(),
        visible,
        Some(0),
        Some(3),
        Default::default(),
        [false; 4],
    )
    .with_history_furiten_facts(bot_logic::HistoryFuritenFacts {
        same_turn: Some(false),
        riichi_missed_win: Some(false),
    });

    (context, actions)
}

#[test]
fn diagnose_free_function_matches_associated_function() {
    let ctx = fold_under_reach_context();
    let actions = fold_actions();
    assert_eq!(
        diagnose_shanten_decision(&ctx, &actions),
        ShantenAgent::diagnose(&ctx, &actions)
    );
}

#[test]
fn diagnose_reports_hora_without_other_judgments() {
    let ctx = opponent_reach_context(Some(0), &[]);
    let actions = vec![dahai(16), LegalAction::Hora];
    let diagnostic = diagnose_matching_act(&ctx, &actions);

    assert_eq!(diagnostic.selected_action, LegalAction::Hora);
    assert_eq!(diagnostic.selected_source, AgentActionSource::Hora);
    assert_eq!(diagnostic.normal_discard, None);
    assert_eq!(diagnostic.normal_discard_action, None);
    assert_eq!(diagnostic.push_pull_inputs, None);
    assert_eq!(diagnostic.push_pull_decision, None);
    assert_eq!(diagnostic.defense, None);
    assert_eq!(diagnostic.defense_fallback_kind(), None);
}

#[test]
fn diagnose_reports_ryukyoku_without_other_judgments() {
    let ctx = opponent_reach_context(Some(0), &[]);
    let actions = vec![dahai(16), LegalAction::Ryukyoku];
    let diagnostic = diagnose_matching_act(&ctx, &actions);

    assert_eq!(diagnostic.selected_action, LegalAction::Ryukyoku);
    assert_eq!(diagnostic.selected_source, AgentActionSource::Ryukyoku);
    assert_eq!(diagnostic.normal_discard, None);
    assert_eq!(diagnostic.normal_discard_action, None);
    assert_eq!(diagnostic.push_pull_inputs, None);
    assert_eq!(diagnostic.push_pull_decision, None);
    assert_eq!(diagnostic.defense, None);
}

#[test]
fn diagnose_reports_reach_source() {
    // 待ち枚数が十分なテンパイで Reach が選ばれる局面。
    let ctx = tenpai_context(&[]);
    let actions = tenpai_actions();
    let diagnostic = diagnose_matching_act(&ctx, &actions);

    assert_eq!(diagnostic.selected_action, LegalAction::Reach);
    assert_eq!(diagnostic.selected_source, AgentActionSource::Reach);
    // Reach 経路でも通常打牌評価は実行済みなので、比較用の通常打牌は保持する。
    assert!(diagnostic.normal_discard.is_some());
    assert!(diagnostic.normal_discard_action.is_some());
    // リーチ判断は通常打牌 selection が選んだ打牌に基づく。
    let reach = diagnostic.reach.as_ref().expect("リーチを検討している");
    assert!(reach.should_reach());
    assert_eq!(reach.reason, ReachDecisionReason::Eligible);
    assert_eq!(
        reach.selected_discard.as_ref(),
        diagnostic.normal_discard_action.as_ref()
    );
    assert_eq!(
        diagnostic.push_pull_decision.map(|decision| decision.mode),
        Some(PushPullMode::Push)
    );
    // Reach を採用したので防御 fallback は検討していない。
    assert_eq!(diagnostic.defense, None);
}

#[test]
fn diagnose_reports_normal_discard_source_with_matching_selection() {
    let hand_values = [0, 4, 8, 12, 17, 20, 24, 28, 32, 36, 40, 44, 89];
    let ctx = GameContext::from_parts(
        Some(tile(116)),
        hand_values.iter().map(|&value| tile(value)).collect(),
    );
    let actions: Vec<LegalAction> = hand_values
        .iter()
        .map(|&value| dahai(value))
        .chain([dahai(116)])
        .collect();
    let normal = select_discard_action(&ctx, &actions).unwrap();

    let diagnostic = diagnose_matching_act(&ctx, &actions);

    assert_eq!(diagnostic.selected_source, AgentActionSource::NormalDiscard);
    assert_eq!(diagnostic.selected_action, normal);
    assert_eq!(diagnostic.normal_discard_action, Some(normal.clone()));

    // 診断内の selected discard が実 action の牌種と一致する。
    let LegalAction::Dahai {
        tile: selected_tile,
    } = normal
    else {
        panic!("expected dahai");
    };
    let selected = diagnostic
        .normal_discard
        .as_ref()
        .unwrap()
        .selected
        .as_ref();
    assert_eq!(selected.map(|e| e.discard), Some(selected_tile.tile_type()));
    assert_eq!(diagnostic.defense, None);
}

#[test]
fn diagnose_keeps_only_legal_discard_candidates_with_comparison_reasons() {
    // 手牌には他の候補があるが、合法 Dahai は 1m / 5s / 北 の3種だけ。
    let hand_values = [0, 4, 8, 12, 17, 20, 24, 28, 32, 36, 40, 44, 89];
    let ctx = GameContext::from_parts(
        Some(tile(116)),
        hand_values.iter().map(|&value| tile(value)).collect(),
    );
    let actions = vec![dahai(0), dahai(89), dahai(116)];

    let diagnostic = diagnose_matching_act(&ctx, &actions);
    let normal_discard = diagnostic.normal_discard.as_ref().unwrap();

    let legal_types: Vec<_> = [0u8, 89, 116]
        .iter()
        .map(|&value| tile(value).tile_type())
        .collect();
    let candidate_types: Vec<_> = normal_discard
        .candidates
        .iter()
        .map(|candidate| candidate.evaluation.discard)
        .collect();
    assert_eq!(candidate_types, legal_types);

    // 選択された候補は1件で、最終 action と牌種が一致する。
    let selected: Vec<_> = normal_discard
        .candidates
        .iter()
        .filter(|candidate| candidate.selected)
        .collect();
    assert_eq!(selected.len(), 1);
    let LegalAction::Dahai {
        tile: selected_tile,
    } = &diagnostic.selected_action
    else {
        panic!("expected dahai");
    };
    assert_eq!(selected[0].evaluation.discard, selected_tile.tile_type());
    assert_eq!(
        selected[0].comparison_reason,
        DiscardComparisonReason::StableOrder
    );

    // 非選択候補は「何の比較軸で負けたか」を持つ。
    for candidate in normal_discard
        .candidates
        .iter()
        .filter(|candidate| !candidate.selected)
    {
        assert_eq!(
            candidate.selected_is_strictly_better_than_candidate,
            compare_discard_evaluations(
                normal_discard.selected.as_ref().unwrap(),
                &candidate.evaluation
            )
            .candidate_is_better
        );
    }
}

#[test]
fn diagnose_matches_act_at_physical_tile_level_for_black_and_red_five() {
    // 赤5m と黒5m が同一牌種として合法。黒5優先を維持し、評価も黒5mの物理牌情報に合わせる。
    let ctx = GameContext::from_parts_with_dora(None, vec![tile(16), tile(17)], vec![tile(12)]);
    let actions = vec![dahai(16), dahai(17)];

    let diagnostic = diagnose_matching_act(&ctx, &actions);
    assert_eq!(diagnostic.selected_action, dahai(17));
    assert_eq!(diagnostic.selected_source, AgentActionSource::NormalDiscard);

    let selected = diagnostic
        .normal_discard
        .as_ref()
        .unwrap()
        .selected
        .as_ref()
        .unwrap();
    assert!(!selected.discards_red_five);
    assert_eq!(selected.discarded_dora_count, 1);
}

#[test]
fn diagnose_matches_act_when_only_red_five_is_legal() {
    let ctx = GameContext::from_parts_with_dora(None, vec![tile(16), tile(17)], vec![tile(12)]);
    let actions = vec![dahai(16)];

    let diagnostic = diagnose_matching_act(&ctx, &actions);
    assert_eq!(diagnostic.selected_action, dahai(16));

    let selected = diagnostic
        .normal_discard
        .as_ref()
        .unwrap()
        .selected
        .as_ref()
        .unwrap();
    assert!(selected.discards_red_five);
    assert_eq!(selected.discarded_dora_count, 2);
}

// 押し引き診断が実際の push-pull 結果と一致することを確認する共通 helper。
fn assert_push_pull_diagnostic(
    ctx: &GameContext,
    actions: &[LegalAction],
    expected_mode: PushPullMode,
) -> ShantenDecisionDiagnostic {
    let diagnostic = diagnose_matching_act(ctx, actions);
    let inputs = diagnostic.push_pull_inputs.unwrap();
    let decision = diagnostic.push_pull_decision.unwrap();

    let selection = select_discard_action_with_evaluation(ctx, actions);
    assert_eq!(
        inputs,
        push_pull_inputs_from_context_with_evaluation(ctx, selection.evaluation.as_ref(), actions)
    );
    assert_eq!(decision, decide_push_pull(&inputs));
    assert_eq!(decision.mode, expected_mode);
    diagnostic
}

#[test]
fn diagnose_holds_push_inputs_and_decision() {
    // 強いテンパイで単独の子リーチに対する Push。
    let ctx = tenpai_under_reach_context(None, [false, true, false, false]);
    let actions = tenpai_dahai_actions();

    let diagnostic = assert_push_pull_diagnostic(&ctx, &actions, PushPullMode::Push);
    let inputs = diagnostic.push_pull_inputs.unwrap();
    assert_eq!(inputs.opponent_reach_count, 1);
    assert!(!inputs.dealer_reacher);
    assert!(!inputs.self_dealer);
    assert_eq!(
        inputs.offense.unwrap().min_shanten_after_discard,
        diagnostic
            .normal_discard
            .as_ref()
            .unwrap()
            .selected
            .as_ref()
            .unwrap()
            .min_shanten_after_discard()
    );
    assert_eq!(
        diagnostic.push_pull_decision.unwrap().reason,
        PushPullReason::StrongTenpaiAgainstReach
    );
    assert_eq!(diagnostic.selected_source, AgentActionSource::NormalDiscard);
}

#[test]
fn diagnose_holds_weak_tenpai_fold_inputs_and_decision() {
    // 待ち枚数が足りないテンパイは Fold。防御 fallback を通常打牌より優先する。
    let ctx = weak_tenpai_under_reach_context();
    let actions = weak_tenpai_actions();

    let diagnostic = assert_push_pull_diagnostic(&ctx, &actions, PushPullMode::Fold);
    assert_eq!(
        diagnostic.push_pull_decision.unwrap().reason,
        PushPullReason::WeakTenpaiAgainstReach
    );
    assert_eq!(
        diagnostic.selected_source,
        AgentActionSource::DefenseFallback(DefenseFallbackKind::Genbutsu)
    );
}

#[test]
fn diagnose_holds_fold_inputs_and_decision() {
    let ctx = fold_under_reach_context();
    let actions = fold_actions();

    let diagnostic = assert_push_pull_diagnostic(&ctx, &actions, PushPullMode::Fold);
    assert_eq!(
        diagnostic.push_pull_decision.unwrap().reason,
        PushPullReason::TwoOrMoreShantenAgainstReach
    );
}

#[test]
fn diagnose_reports_genbutsu_defense_fallback() {
    let ctx = fold_under_reach_context();
    let actions = fold_actions();
    let normal = select_discard_action(&ctx, &actions).unwrap();

    let diagnostic = diagnose_matching_act(&ctx, &actions);

    assert_eq!(
        diagnostic.selected_source,
        AgentActionSource::DefenseFallback(DefenseFallbackKind::Genbutsu)
    );
    assert_eq!(
        diagnostic.defense_fallback_kind(),
        Some(DefenseFallbackKind::Genbutsu)
    );
    assert_eq!(diagnostic.selected_action, dahai(89));
    // 通常打牌とは異なる action を選んでいる。
    assert_eq!(diagnostic.normal_discard_action, Some(normal.clone()));
    assert_ne!(diagnostic.normal_discard_action, Some(dahai(89)));

    let defense = diagnostic.defense.as_ref().unwrap();
    let selected = defense.selected.as_ref().unwrap();
    assert_eq!(selected.selected_kind, DefenseFallbackKind::Genbutsu);
    assert_eq!(selected.selected_action, "5s".to_string());
    assert!(selected.selected_genbutsu_for_all);

    // 候補診断は合法 Dahai 全件を持ち、選択された候補が最終 action と一致する。
    assert_eq!(defense.candidates.len(), actions.len());
    let selected_candidates: Vec<_> = defense
        .candidates
        .iter()
        .filter(|candidate| candidate.selected)
        .collect();
    assert_eq!(selected_candidates.len(), 1);
    assert_eq!(selected_candidates[0].action, diagnostic.selected_action);
    assert!(selected_candidates[0].genbutsu_for_all);
}

#[test]
fn diagnose_reports_honor_safety_defense_candidates() {
    // 共通現物なし。東は2枚見え、南は0枚見え。より安全な東を切る。
    let ctx = opponent_reach_context_with_visible(Some(112), &[], &[108, 109]);
    let actions = vec![dahai(112), dahai(108)];

    let diagnostic = diagnose_matching_act(&ctx, &actions);

    assert_eq!(diagnostic.selected_action, dahai(108));
    assert_eq!(
        diagnostic.defense_fallback_kind(),
        Some(DefenseFallbackKind::ExactRonRisk)
    );

    let defense = diagnostic.defense.as_ref().unwrap();
    assert_eq!(
        defense
            .selected
            .as_ref()
            .unwrap()
            .selected_honor_safety_rank,
        Some(HonorSafetyRank::TwoVisible)
    );

    let south = &defense.candidates[0];
    let east = &defense.candidates[1];
    assert_eq!(south.tile, tile(112).tile_type());
    assert_eq!(south.honor_safety_rank, Some(HonorSafetyRank::NoVisible));
    assert!(!south.genbutsu_for_all);
    assert_eq!(south.wall_rank, None);
    assert_eq!(east.tile, tile(108).tile_type());
    assert_eq!(east.honor_safety_rank, Some(HonorSafetyRank::TwoVisible));
    assert!(east.ron_risk_evidence().is_some());
    assert!(east.selected);
}

#[test]
fn diagnose_reports_suited_safety_defense_candidates() {
    use crate::defense::{SuitedSafetyRank, WallRank};

    // 共通現物も字牌もなし。4m を4枚見せて 2m を NoChance にする。
    let ctx = suited_reach_context(Some(0), &[], &[12, 13, 14, 15], &[]);
    let actions = vec![dahai(0), dahai(4)];

    let diagnostic = diagnose_matching_act(&ctx, &actions);

    assert_eq!(diagnostic.selected_action, dahai(4));
    assert_eq!(
        diagnostic.defense_fallback_kind(),
        Some(DefenseFallbackKind::ExactRonRisk)
    );

    let defense = diagnostic.defense.as_ref().unwrap();
    let one_man = &defense.candidates[0];
    let two_man = &defense.candidates[1];

    assert_eq!(one_man.tile, tile(0).tile_type());
    assert_eq!(one_man.wall_rank, Some(WallRank::NoWall));
    assert_eq!(one_man.suji_for_all_reached, Some(false));
    assert_eq!(one_man.suited_safety_rank, Some(SuitedSafetyRank::NoSafety));
    assert!(!one_man.selected);

    assert_eq!(two_man.tile, tile(4).tile_type());
    assert_eq!(two_man.wall_rank, Some(WallRank::NoChance));
    assert_eq!(two_man.suji_for_all_reached, Some(false));
    assert_eq!(two_man.suited_safety_rank, Some(SuitedSafetyRank::NoChance));
    assert!(two_man.ron_risk_evidence().is_some());
    assert!(two_man.selected);
}

#[test]
fn diagnose_keeps_defense_candidates_when_fallback_is_not_adopted() {
    // Fold だが防御候補が無い局面。防御を検討した記録として候補評価だけ残る。
    let ctx = suited_reach_context_with_reached(Some(0), &[], &[], &[], [false, true, true, false]);
    let actions = vec![LegalAction::Reach, dahai(0), dahai(4)];

    let diagnostic = diagnose_matching_act(&ctx, &actions);

    assert_eq!(diagnostic.selected_source, AgentActionSource::NormalDiscard);
    assert_eq!(diagnostic.defense_fallback_kind(), None);

    let defense = diagnostic.defense.as_ref().unwrap();
    assert_eq!(defense.selected, None);
    assert_eq!(defense.selected_kind(), None);
    assert_eq!(defense.candidates.len(), 2);
    assert!(
        defense
            .candidates
            .iter()
            .all(|candidate| !candidate.selected)
    );
}

#[test]
fn diagnose_reports_legal_dahai_fallback_source() {
    // 手牌情報が無く通常打牌も防御 fallback も選べない局面。合法 Dahai へ落ちる。
    let ctx = GameContext::default();
    let actions = vec![dahai(16), dahai(17)];

    let diagnostic = diagnose_matching_act(&ctx, &actions);

    assert_eq!(diagnostic.selected_action, dahai(17));
    assert_eq!(
        diagnostic.selected_source,
        AgentActionSource::LegalDahaiFallback
    );
    assert_eq!(diagnostic.normal_discard_action, None);
    assert!(diagnostic.push_pull_inputs.is_some());
}

#[test]
fn diagnose_reports_none_source_for_empty_actions() {
    let ctx = GameContext::default();
    let diagnostic = diagnose_matching_act(&ctx, &[]);

    assert_eq!(diagnostic.selected_action, LegalAction::None);
    assert_eq!(diagnostic.selected_source, AgentActionSource::None);
    assert_eq!(diagnostic.normal_discard_action, None);
    // 通常打牌評価は実行したが合法候補が無いので、候補は空。
    let normal_discard = diagnostic.normal_discard.as_ref().unwrap();
    assert_eq!(normal_discard.selected, None);
    assert!(normal_discard.candidates.is_empty());
}

fn context_with_own_melds(
    player_id: Option<u8>,
    hand_values: &[u8],
    drawn_tile: Option<u8>,
    own_melds: Vec<crate::meld::Meld>,
) -> GameContext {
    let mut melds: [Vec<crate::meld::Meld>; 4] = Default::default();
    if let Some(player_id) = player_id {
        melds[usize::from(player_id)] = own_melds;
    }
    GameContext::from_parts_with_melds(
        drawn_tile.map(tile),
        hand_values.iter().map(|&value| tile(value)).collect(),
        vec![],
        None,
        None,
        Vec::new(),
        player_id,
        None,
        Default::default(),
        [false; 4],
        melds,
    )
}

#[test]
fn diagnose_reports_own_fixed_meld_count() {
    let hand_values = [0, 4, 8, 12, 17, 20, 24, 28, 32, 36];
    let ctx = context_with_own_melds(Some(0), &hand_values, Some(40), vec![pon_meld()]);
    let actions: Vec<LegalAction> = hand_values
        .iter()
        .map(|&value| dahai(value))
        .chain([dahai(40)])
        .collect();
    let diagnostic = diagnose_matching_act(&ctx, &actions);

    assert_eq!(
        diagnostic.own_fixed_meld_count.map(FixedMeldCount::get),
        Some(1)
    );
}

// 自分の副露だけが違い、手牌・向聴数・受け入れは同じ2つの context。白ポンだけが役牌翻を持つ。
fn own_meld_value_contexts() -> (GameContext, GameContext) {
    let hand_values = [0u8, 4, 8, 12, 17, 20, 24, 28, 32, 36];
    let build = |own_meld: crate::meld::Meld| {
        let mut melds: [Vec<crate::meld::Meld>; 4] = Default::default();
        melds[0] = vec![own_meld];
        GameContext::from_parts_with_melds(
            Some(tile(40)),
            hand_values.iter().map(|&value| tile(value)).collect(),
            vec![],
            None,
            None,
            Vec::new(),
            Some(0),
            None,
            Default::default(),
            [false, true, false, false],
            melds,
        )
    };

    (
        build(pon_meld()),
        build(crate::meld::Meld::new(
            crate::meld::MeldKind::Pon,
            vec![tile(124), tile(125), tile(126)],
            Some(tile(124)),
        )),
    )
}

fn own_meld_value_actions() -> Vec<LegalAction> {
    [0u8, 4, 8, 12, 17, 20, 24, 28, 32, 36, 40]
        .iter()
        .map(|&value| dahai(value))
        .collect()
}

#[test]
fn own_fixed_meld_value_does_not_change_the_selected_action() {
    // 東ポン (風不明で0翻) と白ポン (1翻) で簡易打点 proxy だけが変わる。
    let (plain, valuable) = own_meld_value_contexts();
    let actions = own_meld_value_actions();

    let plain_diagnostic = diagnose_matching_act(&plain, &actions);
    let valuable_diagnostic = diagnose_matching_act(&valuable, &actions);
    assert_eq!(
        valuable_diagnostic.selected_action,
        plain_diagnostic.selected_action
    );

    for ctx in [&plain, &valuable] {
        let with_lookahead =
            ShantenAgent::diagnose_with_options(ctx, &actions, DiagnosticOptions::WITH_LOOKAHEAD);
        assert_eq!(
            with_lookahead.selected_action,
            ShantenAgent::diagnose(ctx, &actions).selected_action
        );
    }

    let plain_offense = plain_diagnostic
        .push_pull_inputs
        .and_then(|inputs| inputs.offense)
        .expect("offense should be present");
    let valuable_offense = valuable_diagnostic
        .push_pull_inputs
        .and_then(|inputs| inputs.offense)
        .expect("offense should be present");
    assert_eq!(plain_offense.simple_value_proxy_after_discard(), 0);
    assert_eq!(valuable_offense.simple_value_proxy_after_discard(), 1);

    // 打点 proxy が増えても押し引きの mode / reason は変わらない。
    assert_eq!(
        plain_diagnostic
            .push_pull_decision
            .map(|decision| decision.mode),
        valuable_diagnostic
            .push_pull_decision
            .map(|decision| decision.mode)
    );
    assert_eq!(
        plain_diagnostic
            .push_pull_decision
            .map(|decision| decision.reason),
        valuable_diagnostic
            .push_pull_decision
            .map(|decision| decision.reason)
    );
}

#[test]
fn diagnose_reports_no_own_fixed_meld_count_without_player_id() {
    let ctx = GameContext::default();
    let diagnostic = diagnose_matching_act(&ctx, &[]);
    assert_eq!(diagnostic.own_fixed_meld_count, None);
}

fn red_five_chi() -> crate::meld::Meld {
    crate::meld::Meld::new(
        crate::meld::MeldKind::Chi,
        vec![tile(13), tile(16), tile(21)],
        Some(tile(13)),
    )
}

fn opponent_meld_context(
    player_id: Option<u8>,
    opponent_melds: Vec<crate::meld::Meld>,
) -> GameContext {
    opponent_meld_context_with_reach(player_id, opponent_melds, [false; 4])
}

fn opponent_meld_context_with_reach(
    player_id: Option<u8>,
    opponent_melds: Vec<crate::meld::Meld>,
    reached: [bool; 4],
) -> GameContext {
    opponent_meld_context_with_reach_and_oya(player_id, Some(0), opponent_melds, reached)
}

fn opponent_meld_context_with_reach_and_oya(
    player_id: Option<u8>,
    oya: Option<u8>,
    opponent_melds: Vec<crate::meld::Meld>,
    reached: [bool; 4],
) -> GameContext {
    let mut melds: [Vec<crate::meld::Meld>; 4] = Default::default();
    melds[1] = opponent_melds;

    GameContext::from_parts_with_melds(
        Some(tile(OPPONENT_MELD_DRAW)),
        OPPONENT_MELD_HAND
            .iter()
            .map(|&value| tile(value))
            .collect(),
        vec![],
        TileType::new(27),
        TileType::new(27),
        Vec::new(),
        player_id,
        oya,
        Default::default(),
        reached,
        melds,
    )
}

// 他家 (player 1) だけが副露している局面を、自分の自摸後14枚を指定して作る。手牌の最後の
// 1枚をツモ牌として渡す。
fn opponent_meld_context_with_hand(
    hand: &[&str],
    opponent_melds: Vec<crate::meld::Meld>,
) -> GameContext {
    let mut melds: [Vec<crate::meld::Meld>; 4] = Default::default();
    melds[1] = opponent_melds;

    let mut tiles = hand_tiles(hand);
    let drawn_tile = tiles.pop().expect("手牌が空でない");

    GameContext::from_parts_with_melds(
        Some(drawn_tile),
        tiles,
        vec![],
        TileType::new(27),
        TileType::new(27),
        Vec::new(),
        Some(0),
        Some(0),
        Default::default(),
        [false; 4],
        melds,
    )
}

#[test]
fn diagnose_reports_player_threats_for_every_seat() {
    let ctx = opponent_meld_context(Some(0), vec![white_dragon_pon(), red_five_chi()]);
    let diagnostic = diagnose_matching_act(&ctx, &opponent_meld_actions());

    // 表示・解析用に別実装で数え直さず、production の診断値をそのまま持つ。
    assert_eq!(diagnostic.player_threats, diagnose_player_threats(&ctx));

    let threat = &diagnostic.player_threats[1];
    assert_eq!(threat.facts.player, 1);
    assert_eq!(threat.facts.is_opponent(), Some(true));
    assert!(!threat.facts.reached);
    assert_eq!(threat.facts.is_dealer, Some(false));
    assert_eq!(threat.facts.meld_count, 2);
    assert_eq!(threat.facts.open_meld_count, 2);
    assert_eq!(threat.facts.kan_count, 0);
    assert!(threat.melds[0].facts.value_honor.unwrap().is_dragon);
    assert_eq!(threat.melds[1].facts.red_dora_count, 1);

    assert_eq!(diagnostic.player_threats[0].facts.is_self, Some(true));
    assert_eq!(diagnostic.player_threats[0].facts.meld_count, 0);
}

#[test]
fn diagnose_classifies_the_open_hand_threat_from_the_same_facts() {
    use crate::open_hand_threat::{
        OpenHandThreatAssessment, OpenHandThreatDecision, OpenHandThreatExclusion,
        OpenHandThreatLevel, OpenHandThreatReason, classify_open_hand_threat,
    };

    let ctx = opponent_meld_context(Some(0), vec![white_dragon_pon(), red_five_chi()]);
    let diagnostic = diagnose_matching_act(&ctx, &opponent_meld_actions());

    // 表示・解析用に分類し直さず、診断が持つ facts から求めた結果をそのまま持つ。
    for threat in &diagnostic.player_threats {
        assert_eq!(
            threat.open_hand_threat,
            classify_open_hand_threat(threat.facts)
        );
    }

    // 白 Pon + Chi の2副露で確定役牌があるので High。
    assert_eq!(
        diagnostic.player_threats[1].open_hand_threat,
        OpenHandThreatAssessment::Classified(OpenHandThreatDecision {
            level: OpenHandThreatLevel::High,
            reason: OpenHandThreatReason::TwoOrMoreWithVisibleHan,
        })
    );
    assert_eq!(
        diagnostic.player_threats[0].open_hand_threat.exclusion(),
        Some(OpenHandThreatExclusion::SelfSeat)
    );
    assert_eq!(
        diagnostic.player_threats[2].open_hand_threat.level(),
        Some(OpenHandThreatLevel::None)
    );
}

#[test]
fn diagnose_reports_the_open_hand_defense_of_the_high_threats() {
    use crate::open_hand_defense::OpenHandDefenseDiagnostic;

    let ctx = opponent_meld_context(Some(0), vec![white_dragon_pon(), red_five_chi()]);
    let actions = opponent_meld_actions();
    let diagnostic = diagnose_matching_act(&ctx, &actions);

    // target は player_threats の classification と同じ source of truth から選ぶ。
    assert_eq!(diagnostic.open_hand_defense.targets, vec![1]);
    assert!(diagnostic.open_hand_defense.has_target());
    // この局面は待ち 3 枚の弱いテンパイなので Fold。OpenHand 防御 fallback を採用する。
    let selected = diagnostic
        .open_hand_defense
        .selected
        .as_ref()
        .expect("OpenHand 防御 fallback を採用している");
    assert_eq!(selected.selected_action, diagnostic.selected_action);
    let category = diagnostic
        .open_hand_defense_category()
        .expect("採用した category がある");
    assert_eq!(
        diagnostic.open_hand_defense,
        OpenHandDefenseDiagnostic::from_context(
            &ctx,
            &actions,
            Some((&diagnostic.selected_action, category))
        )
    );

    // 合法 Dahai の順序をそのまま保つ。
    assert_eq!(
        diagnostic
            .open_hand_defense
            .candidates
            .iter()
            .map(|candidate| candidate.action.clone())
            .collect::<Vec<LegalAction>>(),
        actions
    );
    for candidate in &diagnostic.open_hand_defense.candidates {
        assert_eq!(
            candidate
                .targets
                .iter()
                .map(|target| target.player)
                .collect::<Vec<usize>>(),
            vec![1]
        );
    }
}

#[test]
fn diagnose_reports_no_open_hand_defense_target_without_a_high_threat() {
    use crate::open_hand_threat::OpenHandThreatLevel;

    // 役牌 Pon 1副露だけの相手は Present なので、防御 target にしない。
    let ctx = opponent_meld_context(Some(0), vec![white_dragon_pon()]);
    let diagnostic = diagnose_matching_act(&ctx, &opponent_meld_actions());

    assert_eq!(
        diagnostic.player_threats[1].open_hand_threat.level(),
        Some(OpenHandThreatLevel::Present)
    );
    assert!(!diagnostic.open_hand_defense.has_target());
    assert!(diagnostic.open_hand_defense.targets.is_empty());
    assert!(diagnostic.open_hand_defense.candidates.is_empty());
}

#[test]
fn a_reached_player_is_not_an_open_hand_defense_target() {
    // リーチ者の防御は既存の Defense fallback が source of truth で、二重適用しない。
    let ctx = opponent_meld_context_with_reach(
        Some(0),
        vec![white_dragon_pon(), red_five_chi()],
        [false, true, false, false],
    );
    let diagnostic = diagnose_matching_act(&ctx, &opponent_meld_actions());

    assert!(diagnostic.player_threats[1].facts.reached);
    assert_eq!(diagnostic.player_threats[1].open_hand_threat.level(), None);
    assert!(!diagnostic.open_hand_defense.has_target());
}

#[test]
fn a_high_open_hand_threat_folds_from_a_weak_tenpai() {
    use crate::open_hand_threat::OpenHandThreatLevel;

    let actions = opponent_meld_actions();
    let with_high = opponent_meld_context(Some(0), vec![white_dragon_pon(), red_five_chi()]);
    let without_melds = opponent_meld_context(Some(0), vec![]);

    let melded = diagnose_matching_act(&with_high, &actions);
    let plain = diagnose_matching_act(&without_melds, &actions);

    assert_eq!(
        melded.player_threats[1].open_hand_threat.level(),
        Some(OpenHandThreatLevel::High)
    );
    assert!(melded.open_hand_defense.has_target());
    assert!(!plain.open_hand_defense.has_target());

    // 待ち 3 枚の弱いテンパイなので、High の副露相手がいれば降りる。
    let melded_decision = melded.push_pull_decision.expect("押し引きを判定している");
    let plain_decision = plain.push_pull_decision.expect("押し引きを判定している");
    assert_eq!(
        melded
            .push_pull_inputs
            .and_then(|inputs| inputs.offense)
            .and_then(|offense| offense.tenpai_wait_after_discard)
            .map(|wait| wait.tsumo_remaining),
        Some(3)
    );
    assert!(
        !melded
            .push_pull_inputs
            .expect("押し引き入力がある")
            .selected_normal_discard_hard_safe_for_all_high_open_hand_targets
    );
    assert_eq!(melded_decision.mode, PushPullMode::Fold);
    assert_eq!(
        melded_decision.reason,
        PushPullReason::WeakTenpaiAgainstHighOpenHand
    );

    // threat が無ければ従来どおり通常打牌のまま。
    assert_eq!(plain_decision.mode, PushPullMode::Push);
    assert_eq!(plain_decision.reason, PushPullReason::NoThreat);
    assert_eq!(plain.selected_source, AgentActionSource::NormalDiscard);

    // Fold では OpenHand 防御 fallback を通常打牌より優先する。
    assert!(matches!(
        melded.selected_source,
        AgentActionSource::OpenHandDefenseFallback(_)
    ));
    assert!(melded.open_hand_defense_category().is_some());
    assert_eq!(melded.defense, plain.defense);
}

#[test]
fn diagnose_does_not_guess_the_self_seat_without_player_id() {
    let ctx = opponent_meld_context(None, vec![white_dragon_pon()]);
    let diagnostic = diagnose_matching_act(&ctx, &opponent_meld_actions());

    assert_eq!(diagnostic.player_threats.len(), 4);
    for (player, threat) in diagnostic.player_threats.iter().enumerate() {
        assert_eq!(threat.facts.player, player);
        assert_eq!(threat.facts.is_self, None);
        assert_eq!(threat.facts.is_opponent(), None);
    }
    assert_eq!(diagnostic.player_threats[1].facts.meld_count, 1);
}

#[test]
fn opponent_melds_keep_the_same_offense_and_normal_discard() {
    // 副露 facts から High OpenHandThreat になっても、通常打牌評価と offense は変わらない。
    // 変わるのは threat と、そこから決まる押し引き・選択経路だけ。
    let actions = opponent_meld_actions();
    let with_melds = opponent_meld_context(Some(0), vec![white_dragon_pon(), red_five_chi()]);
    let without_melds = opponent_meld_context(Some(0), vec![]);

    let melded = diagnose_matching_act(&with_melds, &actions);
    let plain = diagnose_matching_act(&without_melds, &actions);

    assert_eq!(melded.normal_discard_action, plain.normal_discard_action);
    assert_eq!(melded.defense, plain.defense);
    assert_eq!(melded.call, plain.call);
    assert_ne!(melded.player_threats, plain.player_threats);
    // Fold ではリーチを検討しない。
    assert_eq!(melded.reach, None);

    // 押し引き入力は副露由来の facts と classification だけが異なる。
    let melded_inputs = melded.push_pull_inputs.expect("押し引き入力がある");
    let plain_inputs = plain.push_pull_inputs.expect("押し引き入力がある");
    assert_eq!(
        melded_inputs.opponent_reach_count,
        plain_inputs.opponent_reach_count
    );
    assert_eq!(melded_inputs.dealer_reacher, plain_inputs.dealer_reacher);
    assert_eq!(melded_inputs.self_dealer, plain_inputs.self_dealer);
    assert_eq!(melded_inputs.offense, plain_inputs.offense);
    assert_ne!(melded_inputs.player_threats, plain_inputs.player_threats);
    assert!(melded_inputs.has_high_open_hand_threat());
    assert!(!plain_inputs.has_high_open_hand_threat());

    // 待ち 3 枚の弱いテンパイなので、High の副露相手がいれば降りる。
    let decision = melded.push_pull_decision.expect("押し引きを判定している");
    assert_eq!(decision.mode, PushPullMode::Fold);
    assert_eq!(
        decision.reason,
        crate::push_pull::PushPullReason::WeakTenpaiAgainstHighOpenHand
    );
    assert!(matches!(
        melded.selected_source,
        AgentActionSource::OpenHandDefenseFallback(_)
    ));
    assert_eq!(plain.selected_source, AgentActionSource::NormalDiscard);
    assert_eq!(melded_inputs.opponent_reach_count, 0);

    let plain_decision = plain.push_pull_decision.expect("押し引きを判定している");
    assert_eq!(plain_decision.mode, PushPullMode::Push);
    assert_eq!(
        plain_decision.reason,
        crate::push_pull::PushPullReason::NoThreat
    );

    assert_eq!(
        melded.player_threats[1].open_hand_threat.level(),
        Some(crate::open_hand_threat::OpenHandThreatLevel::High)
    );
}

#[test]
fn player_threats_keep_act_and_diagnose_consistent() {
    let ctx = opponent_meld_context(Some(0), vec![white_dragon_pon(), red_five_chi()]);
    let actions = opponent_meld_actions();

    let mut agent = ShantenAgent;
    let acted = agent.act(&ctx, &actions);
    let diagnosed = ShantenAgent::diagnose(&ctx, &actions);
    let with_lookahead =
        ShantenAgent::diagnose_with_options(&ctx, &actions, DiagnosticOptions::WITH_LOOKAHEAD);

    assert_eq!(diagnosed.selected_action, acted);
    assert_eq!(with_lookahead.selected_action, acted);
    assert_eq!(with_lookahead.player_threats, diagnosed.player_threats);
}

#[test]
fn player_threats_keep_reach_and_meld_facts_together() {
    let ctx = opponent_meld_context_with_reach(
        Some(0),
        vec![white_dragon_pon()],
        [false, true, false, false],
    );
    let diagnostic = diagnose_matching_act(&ctx, &opponent_meld_actions());

    let threat = &diagnostic.player_threats[1];
    assert!(threat.facts.reached);
    assert_eq!(threat.facts.meld_count, 1);
    assert_eq!(threat.facts.open_meld_count, 1);
    assert_eq!(
        diagnostic
            .push_pull_inputs
            .expect("押し引き入力がある")
            .opponent_reach_count,
        1
    );
}

#[test]
fn push_pull_and_diagnostics_share_the_same_threat_facts() {
    // 押し引きへ渡した軽量 facts と診断の集計値が同じものであることを固定する。
    let ctx = opponent_meld_context_with_reach(
        Some(0),
        vec![white_dragon_pon(), red_five_chi()],
        [false, false, false, true],
    );
    let actions = opponent_meld_actions();
    let diagnostic = diagnose_matching_act(&ctx, &actions);
    let inputs = diagnostic.push_pull_inputs.expect("押し引き入力がある");

    for player in 0..4 {
        assert_eq!(
            inputs.player_threats[player], diagnostic.player_threats[player].facts,
            "player {player}"
        );
    }
    assert_eq!(
        inputs.player_threats,
        crate::threat::player_threat_facts_from_context(&ctx)
    );
    assert_eq!(diagnostic.player_threats, diagnose_player_threats(&ctx));

    // 2手先診断を有効にしても facts は変わらない。
    let with_lookahead =
        ShantenAgent::diagnose_with_options(&ctx, &actions, DiagnosticOptions::WITH_LOOKAHEAD);
    assert_eq!(with_lookahead.push_pull_inputs, diagnostic.push_pull_inputs);
    assert_eq!(with_lookahead.player_threats, diagnostic.player_threats);
}

#[test]
fn opponent_melds_do_not_change_the_reach_branches() {
    // 単独子リーチ・親リーチ・複数リーチのどれでも境界は同じ。副露している player 1 は
    // 同時にリーチ者なので OpenHandThreat の対象外で、reason はリーチだけのものになる。
    // この局面は待ち 3 枚の弱いテンパイなので、どのリーチでも Fold。
    let actions = opponent_meld_actions();
    let cases = [
        (Some(0u8), [false, true, false, false]),
        (Some(1), [false, true, false, false]),
        (Some(0), [false, true, true, false]),
    ];

    for (oya, reached) in cases {
        let with_melds = opponent_meld_context_with_reach_and_oya(
            Some(0),
            oya,
            vec![white_dragon_pon(), red_five_chi()],
            reached,
        );
        let without_melds = opponent_meld_context_with_reach_and_oya(Some(0), oya, vec![], reached);

        let melded = diagnose_matching_act(&with_melds, &actions);
        let plain = diagnose_matching_act(&without_melds, &actions);

        assert!(
            !melded.open_hand_defense.has_target(),
            "{oya:?} {reached:?}"
        );
        assert_eq!(
            melded.push_pull_decision, plain.push_pull_decision,
            "{oya:?} {reached:?}"
        );
        let decision = melded.push_pull_decision.expect("押し引きを判定している");
        assert_eq!(decision.mode, PushPullMode::Fold, "{oya:?} {reached:?}");
        assert_eq!(
            decision.reason,
            PushPullReason::WeakTenpaiAgainstReach,
            "{oya:?} {reached:?}"
        );

        let melded_inputs = melded.push_pull_inputs.expect("押し引き入力がある");
        let plain_inputs = plain.push_pull_inputs.expect("押し引き入力がある");
        assert_eq!(
            melded_inputs.opponent_reach_count,
            plain_inputs.opponent_reach_count
        );
        assert_eq!(melded_inputs.dealer_reacher, plain_inputs.dealer_reacher);
        assert_eq!(melded_inputs.self_dealer, plain_inputs.self_dealer);
        assert_eq!(melded_inputs.offense, plain_inputs.offense);
        assert_eq!(melded_inputs.player_threats[1].open_meld_count, 2);
        assert_eq!(plain_inputs.player_threats[1].open_meld_count, 0);
    }
}

#[test]
fn threat_facts_are_built_for_early_return_paths() {
    // 和了・九種九牌で早期終了した場合も、診断は4席分の facts を持つ。九種九牌は宣言する
    // 遠い手牌でだけ早期終了するので、そちらは専用の手牌で確認する。
    for (ctx, actions) in [
        (
            opponent_meld_context(Some(0), vec![white_dragon_pon()]),
            vec![LegalAction::Hora],
        ),
        (
            opponent_meld_context_with_hand(&KOKUSHI_FOUR_HAND, vec![white_dragon_pon()]),
            vec![LegalAction::Ryukyoku],
        ),
    ] {
        let diagnostic = diagnose_matching_act(&ctx, &actions);
        assert_eq!(diagnostic.selected_action, actions[0]);
        assert_eq!(diagnostic.push_pull_inputs, None);
        assert_eq!(diagnostic.player_threats, diagnose_player_threats(&ctx));
        assert_eq!(diagnostic.player_threats[1].facts.open_meld_count, 1);
    }
}

// 白ポン1組。副露の種類によらず完成済み面子1として数える。
fn white_dragon_pon() -> crate::meld::Meld {
    crate::meld::Meld::new(
        crate::meld::MeldKind::Pon,
        vec![tile(124), tile(125), tile(126)],
        Some(tile(124)),
    )
}

#[test]
fn act_uses_the_fixed_meld_aware_normal_discard() {
    // 白ポン1組 + 123456m 78p 55s + ツモ N。N を切ると副露込みの通常形テンパイ (待ち 6p / 9p)。
    let hand_values = [0u8, 4, 8, 12, 17, 20, 60, 64, 89, 90];
    let ctx = context_with_own_melds(Some(0), &hand_values, Some(120), vec![white_dragon_pon()]);
    let actions: Vec<LegalAction> = hand_values
        .iter()
        .map(|&value| dahai(value))
        .chain([dahai(120)])
        .collect();

    let mut agent = ShantenAgent;
    assert_eq!(agent.act(&ctx, &actions), dahai(120));

    let diagnostic = diagnose_matching_act(&ctx, &actions);
    assert_eq!(diagnostic.selected_source, AgentActionSource::NormalDiscard);
    assert_eq!(diagnostic.normal_discard_action, Some(dahai(120)));
    assert_eq!(
        diagnostic.own_fixed_meld_count.map(FixedMeldCount::get),
        Some(1)
    );

    let selected = diagnostic
        .normal_discard
        .as_ref()
        .unwrap()
        .selected
        .as_ref()
        .unwrap();
    assert_eq!(selected.min_shanten_after_discard(), 0);
    assert_eq!(selected.shanten_after_discard.standard(), 0);
    assert_eq!(selected.acceptance_total_remaining(), 8);
    let acceptance: Vec<String> = selected
        .acceptance_after_discard
        .tiles
        .iter()
        .map(|entry| entry.tile.to_mjai_string())
        .collect();
    assert_eq!(acceptance, vec!["6p".to_string(), "9p".to_string()]);

    // 同じ評価が押し引き入力へ共有される。
    let offense = diagnostic.push_pull_inputs.unwrap().offense.unwrap();
    assert_eq!(offense.min_shanten_after_discard, 0);
    assert_eq!(offense.acceptance_total_remaining, 8);
}

#[test]
fn act_without_own_melds_keeps_the_concealed_evaluation() {
    // 同じ手牌でも副露が無ければ従来どおり二向聴のまま評価する。
    let hand_values = [0u8, 4, 8, 12, 17, 20, 60, 64, 89, 90];
    let ctx = context_with_own_melds(Some(0), &hand_values, Some(120), vec![]);
    let actions: Vec<LegalAction> = hand_values
        .iter()
        .map(|&value| dahai(value))
        .chain([dahai(120)])
        .collect();

    let diagnostic = diagnose_matching_act(&ctx, &actions);
    let selected = diagnostic
        .normal_discard
        .as_ref()
        .unwrap()
        .selected
        .as_ref()
        .unwrap();
    assert_eq!(selected.min_shanten_after_discard(), 2);
    assert!(selected.shanten_after_discard.concealed().is_some());
}

#[test]
fn melds_do_not_change_the_selected_action() {
    let hand_values = [0, 4, 8, 12, 17, 20, 24, 28, 32, 36];
    let actions: Vec<LegalAction> = hand_values
        .iter()
        .map(|&value| dahai(value))
        .chain([dahai(40)])
        .collect();
    let without_melds = context_with_own_melds(Some(0), &hand_values, Some(40), vec![]);
    let with_melds = context_with_own_melds(Some(0), &hand_values, Some(40), vec![pon_meld()]);

    let mut agent = ShantenAgent;
    assert_eq!(
        agent.act(&with_melds, &actions),
        agent.act(&without_melds, &actions)
    );
    assert_eq!(
        ShantenAgent::diagnose(&with_melds, &actions).selected_action,
        ShantenAgent::diagnose(&without_melds, &actions).selected_action
    );
}

#[test]
fn diagnose_reports_none_source_without_dahai_actions() {
    let ctx = GameContext::default();
    let actions = vec![
        LegalAction::Pon {
            tile: tile(108),
            consumed: vec![tile(109), tile(110)],
        },
        LegalAction::None,
    ];
    let diagnostic = diagnose_matching_act(&ctx, &actions);

    assert_eq!(diagnostic.selected_action, LegalAction::None);
    assert_eq!(diagnostic.selected_source, AgentActionSource::None);
}

// ---- 診断収集が action 選択へ影響しないことの確認 ----

#[test]
fn act_path_does_not_build_analysis_diagnostics() {
    // 通常の act() 経路では、解析専用の追加診断(候補ごとの形の内訳・全防御候補評価)を作らない。
    let ctx = fold_under_reach_context();
    let actions = fold_actions();

    let mut diagnostics = DecisionDiagnostics::disabled();
    let decision = ShantenAgent.decide_with_diagnostics(&ctx, &actions, &mut diagnostics);

    assert_eq!(decision.action, dahai(89));
    assert!(diagnostics.normal_discard.is_none());
    assert!(diagnostics.defense.is_none());
}

#[test]
fn act_path_does_not_build_the_reach_damaten_comparison() {
    // リーチを検討する Push mode でも、診断が無効な act() 経路では統合診断を構築しない。
    // 統合診断だけが必要とする完成手の組み立てとリーチ Ron baseline の点数計算は、この経路
    // へ入る唯一の入口 (diagnose_reach_damaten_comparison) を通らないので実行されない。
    let ctx = tenpai_context(&[]);
    let actions = tenpai_actions();

    let mut diagnostics = DecisionDiagnostics::disabled();
    let decision = ShantenAgent.decide_with_diagnostics(&ctx, &actions, &mut diagnostics);

    assert_eq!(decision.action, LegalAction::Reach);
    assert!(decision.reach.is_some());
    assert!(diagnostics.reach_damaten_comparison.is_none());
}

#[test]
fn the_reach_damaten_comparison_is_built_only_with_diagnostics() {
    // 診断経路では同じ判断のまま統合診断を構築する。リーチが合法で Ron availability も
    // Some(true) の通常ケースでは、リーチ Ron baseline も構築する。
    let (ctx, actions) = pinfu_tanyao_context_and_actions();

    let mut agent = ShantenAgent;
    let diagnostic = ShantenAgent::diagnose(&ctx, &actions);
    let comparison = diagnostic
        .reach_damaten_comparison
        .as_ref()
        .expect("統合診断を構築している");

    assert_eq!(diagnostic.selected_action, agent.act(&ctx, &actions));
    assert!(comparison.reach_legal);
    assert_eq!(comparison.can_ron, Some(true));
    assert!(comparison.reach_ron_baseline.is_some());
}

#[test]
fn enabling_diagnostics_does_not_change_decision() {
    let cases: Vec<(GameContext, Vec<LegalAction>)> = vec![
        (fold_under_reach_context(), fold_actions()),
        (tenpai_context(&[]), tenpai_actions()),
        (GameContext::default(), vec![dahai(16), dahai(17)]),
        (GameContext::default(), vec![]),
        (
            opponent_reach_context(Some(0), &[]),
            vec![dahai(16), LegalAction::Hora],
        ),
        (
            suited_reach_context(Some(0), &[], &[12, 13, 14, 15], &[]),
            vec![dahai(0), dahai(4)],
        ),
    ];

    for (ctx, actions) in cases {
        let agent = ShantenAgent;
        let production = agent.decide(&ctx, &actions);
        let with_diagnostics =
            agent.decide_with_diagnostics(&ctx, &actions, &mut DecisionDiagnostics::enabled());
        assert_eq!(production, with_diagnostics);
    }
}

// ---- 2向聴の ExpectedSelfTsumoValue (DiagnosticOptions::WITH_TWO_SHANTEN_SELF_TSUMO) テスト ----

// 3副露済みの concealed 5枚 1m 5p 9s 白 發。どの牌を切っても2向聴のままになる小さい局面で、
// 診断の構造と選択への非干渉を確認する。
fn two_shanten_context(remaining_tiles: Option<u32>) -> GameContext {
    let hand: Vec<_> = [0u8, 53, 104, 124].iter().map(|&v| tile(v)).collect();
    two_shanten_melded_context(hand, tile(128), remaining_tiles)
}

// 東・南・西のポン3組を持つ副露局面。concealed 5枚だけを差し替えて向聴数を変える。
fn two_shanten_melded_context(
    hand: Vec<TileId>,
    drawn: TileId,
    remaining_tiles: Option<u32>,
) -> GameContext {
    let mut visible = hand.clone();
    visible.push(drawn);
    let melds = [
        vec![
            Meld::new(
                MeldKind::Pon,
                vec![tile(108), tile(109), tile(110)],
                Some(tile(108)),
            ),
            Meld::new(
                MeldKind::Pon,
                vec![tile(112), tile(113), tile(114)],
                Some(tile(112)),
            ),
            Meld::new(
                MeldKind::Pon,
                vec![tile(116), tile(117), tile(118)],
                Some(tile(116)),
            ),
        ],
        Vec::new(),
        Vec::new(),
        Vec::new(),
    ];
    visible.extend(
        melds[0]
            .iter()
            .flat_map(|meld| meld.tiles().iter().copied()),
    );

    GameContext::from_parts_with_melds(
        Some(drawn),
        hand,
        vec![],
        TileType::from_mjai_type_str("E").ok(),
        TileType::from_mjai_type_str("N").ok(),
        visible,
        Some(0),
        Some(1),
        Default::default(),
        [false; 4],
        melds,
    )
    .with_table_state_facts(TableStateFacts {
        remaining_tiles,
        ..Default::default()
    })
}

fn two_shanten_actions() -> Vec<LegalAction> {
    [0u8, 53, 104, 124, 128].iter().map(|&v| dahai(v)).collect()
}

// 同じ3副露で concealed 5枚 1m 2m 5p 9s 白。どの牌を切っても1向聴のままで、2向聴診断は空に
// なるが same-shanten の枝は持つ。追加探索が互いに独立であることの確認に使う。
fn iishanten_context() -> GameContext {
    let hand: Vec<_> = [0u8, 4, 53, 104].iter().map(|&v| tile(v)).collect();
    two_shanten_melded_context(hand, tile(124), None)
}

fn iishanten_actions() -> Vec<LegalAction> {
    [0u8, 4, 53, 104, 124].iter().map(|&v| dahai(v)).collect()
}

// 構築済みの2手先診断が持つ same-shanten downstream の枝数。
fn same_shanten_downstream_count(lookahead: &LookaheadDiagnostic) -> usize {
    lookahead
        .candidates
        .iter()
        .flat_map(|candidate| candidate.draws.iter())
        .flat_map(|draw| draw.variants.iter())
        .filter(|variant| variant.downstream.is_some())
        .count()
}

#[test]
fn the_diagnostic_options_map_to_independent_lookahead_scopes() {
    // 2つの追加探索は互いに含まない。指定した組み合わせがそのまま scope になる。
    assert_eq!(
        DiagnosticOptions::NONE.lookahead_scope(),
        LookaheadDiagnosticScope::None
    );
    assert_eq!(
        DiagnosticOptions::WITH_LOOKAHEAD.lookahead_scope(),
        LookaheadDiagnosticScope::Lookahead {
            same_shanten_downstream: false,
            two_shanten_self_tsumo: false,
        }
    );
    assert_eq!(
        DiagnosticOptions::WITH_SAME_SHANTEN_DOWNSTREAM.lookahead_scope(),
        LookaheadDiagnosticScope::Lookahead {
            same_shanten_downstream: true,
            two_shanten_self_tsumo: false,
        }
    );
    assert_eq!(
        DiagnosticOptions::WITH_TWO_SHANTEN_SELF_TSUMO.lookahead_scope(),
        LookaheadDiagnosticScope::Lookahead {
            same_shanten_downstream: false,
            two_shanten_self_tsumo: true,
        }
    );
    assert_eq!(
        DiagnosticOptions::WITH_SAME_SHANTEN_DOWNSTREAM_AND_TWO_SHANTEN_SELF_TSUMO
            .lookahead_scope(),
        LookaheadDiagnosticScope::Lookahead {
            same_shanten_downstream: true,
            two_shanten_self_tsumo: true,
        }
    );
}

#[test]
fn the_two_shanten_self_tsumo_diagnostic_does_not_build_the_same_shanten_downstream() {
    // 1向聴局面で2向聴診断だけを要求しても、2向聴診断は空のままで重い downstream 探索は
    // 走らない。2手先診断は `WITH_LOOKAHEAD` と同じものになる。
    let ctx = iishanten_context();
    let actions = iishanten_actions();

    let lookahead_only =
        ShantenAgent::diagnose_with_options(&ctx, &actions, DiagnosticOptions::WITH_LOOKAHEAD);
    let downstream = ShantenAgent::diagnose_with_options(
        &ctx,
        &actions,
        DiagnosticOptions::WITH_SAME_SHANTEN_DOWNSTREAM,
    );
    let two_shanten = ShantenAgent::diagnose_with_options(
        &ctx,
        &actions,
        DiagnosticOptions::WITH_TWO_SHANTEN_SELF_TSUMO,
    );

    let count = |diagnostic: &ShantenDecisionDiagnostic| {
        same_shanten_downstream_count(
            diagnostic
                .normal_discard_lookahead
                .as_ref()
                .expect("2手先診断が構築されている"),
        )
    };
    assert_eq!(count(&lookahead_only), 0);
    assert!(count(&downstream) > 0);
    assert_eq!(count(&two_shanten), 0);
    assert_eq!(
        two_shanten.normal_discard_lookahead,
        lookahead_only.normal_discard_lookahead
    );

    // 最善向聴数が2向聴でないので、2向聴診断そのものは空になる。
    assert!(
        two_shanten
            .normal_discard_two_shanten_self_tsumo
            .as_ref()
            .expect("2向聴診断が構築されている")
            .candidates
            .is_empty()
    );

    // scope の違いは選ぶ action を変えない。
    let mut agent = ShantenAgent;
    let expected = agent.act(&ctx, &actions);
    for diagnostic in [&lookahead_only, &downstream, &two_shanten] {
        assert_eq!(diagnostic.selected_action, expected);
    }
}

#[test]
fn the_two_shanten_self_tsumo_diagnostic_is_opt_in() {
    // 明示的に要求した場合だけ構築する。2手先診断だけでは持たない。
    let ctx = two_shanten_context(None);
    let actions = two_shanten_actions();

    for options in [
        DiagnosticOptions::NONE,
        DiagnosticOptions::WITH_LOOKAHEAD,
        DiagnosticOptions::WITH_SAME_SHANTEN_DOWNSTREAM,
    ] {
        assert!(
            ShantenAgent::diagnose_with_options(&ctx, &actions, options)
                .normal_discard_two_shanten_self_tsumo
                .is_none()
        );
    }
}

#[test]
fn the_two_shanten_self_tsumo_diagnostic_does_not_change_the_selected_action() {
    // 山の残枚数が unknown な局面では self-tsumo 確率模型の材料が揃わないので、候補は並ぶが値は
    // 確定しない。構築しても選択も他の診断も変わらない。
    let ctx = two_shanten_context(None);
    let actions = two_shanten_actions();

    let mut agent = ShantenAgent;
    let expected = agent.act(&ctx, &actions);
    let without = ShantenAgent::diagnose(&ctx, &actions);
    let with = ShantenAgent::diagnose_with_options(
        &ctx,
        &actions,
        DiagnosticOptions::WITH_TWO_SHANTEN_SELF_TSUMO,
    );

    assert_eq!(with.selected_action, expected);
    let two_shanten = with
        .normal_discard_two_shanten_self_tsumo
        .clone()
        .expect("2向聴診断が構築されている");
    let candidates = with
        .normal_discard
        .as_ref()
        .expect("normal discard evaluated")
        .candidates
        .clone();
    assert_eq!(two_shanten.candidates.len(), candidates.len());
    for (candidate, evaluated) in two_shanten.candidates.iter().zip(candidates.iter()) {
        assert_eq!(candidate.discard, evaluated.evaluation.discard);
        assert_eq!(evaluated.evaluation.min_shanten_after_discard(), 2);
        // 残り自摸機会が分からない局面では 0 点で補完せず unknown のままにする。
        assert_eq!(candidate.expected_self_tsumo_value, None);
    }

    // 2向聴診断以外の診断はすべて既定の診断と一致する。
    assert_eq!(
        ShantenDecisionDiagnostic {
            normal_discard_lookahead: None,
            normal_discard_lookahead_value: None,
            normal_discard_tenpai_continuation: None,
            normal_discard_current_tenpai_continuation: None,
            normal_discard_two_shanten_self_tsumo: None,
            ..with
        },
        without
    );
}

#[test]
fn only_a_two_shanten_candidate_set_has_the_two_shanten_self_tsumo_value() {
    // 打牌候補集合の最善向聴数が2向聴でない局面では、比較できる候補が無いので空の診断になる。
    let ctx = lookahead_context();
    let actions = lookahead_actions();

    let diagnostic = ShantenAgent::diagnose_with_options(
        &ctx,
        &actions,
        DiagnosticOptions::WITH_TWO_SHANTEN_SELF_TSUMO,
    );

    assert_ne!(
        diagnostic
            .normal_discard
            .as_ref()
            .expect("normal discard evaluated")
            .candidates
            .iter()
            .map(|candidate| candidate.evaluation.min_shanten_after_discard())
            .min(),
        Some(2)
    );
    assert!(
        diagnostic
            .normal_discard_two_shanten_self_tsumo
            .expect("2向聴診断が構築されている")
            .candidates
            .is_empty()
    );
}

// ---- 2手先診断 (DiagnosticOptions::WITH_LOOKAHEAD) テスト ----

// 2手先診断テスト用の小さい局面。2手先は「打牌候補 × 受け入れ牌 × 次打牌候補」の探索に
// なるため、診断の構造と選択への非干渉を確認するのに十分な最小の手牌で回す。
fn lookahead_context() -> GameContext {
    let hand: Vec<_> = [0u8, 4, 36, 40, 89].iter().map(|&v| tile(v)).collect();
    let drawn = tile(90);
    let mut visible = hand.clone();
    visible.push(drawn);
    visible.push(tile(1));
    GameContext::from_parts_with_visible_tiles(Some(drawn), hand, vec![], None, None, visible)
}

fn lookahead_actions() -> Vec<LegalAction> {
    [0u8, 4, 36, 40, 89, 90].iter().map(|&v| dahai(v)).collect()
}

#[test]
fn act_path_does_not_build_lookahead() {
    // 通常の act() 経路では2手先診断を構築しない。
    let ctx = lookahead_context();
    let actions = lookahead_actions();

    let mut diagnostics = DecisionDiagnostics::disabled();
    let _ = ShantenAgent.decide_with_diagnostics(&ctx, &actions, &mut diagnostics);

    assert!(diagnostics.normal_discard_lookahead.is_none());
}

#[test]
fn diagnose_does_not_build_lookahead_by_default() {
    // 既定の診断でも2手先は構築しない。構築するのは明示的に要求した場合だけ。
    let ctx = lookahead_context();
    let actions = lookahead_actions();

    assert!(
        ShantenAgent::diagnose(&ctx, &actions)
            .normal_discard_lookahead
            .is_none()
    );
    assert!(
        ShantenAgent::diagnose_with_options(&ctx, &actions, DiagnosticOptions::NONE)
            .normal_discard_lookahead
            .is_none()
    );
}

#[test]
fn lookahead_does_not_change_the_selected_action() {
    let ctx = lookahead_context();
    let actions = lookahead_actions();

    let mut agent = ShantenAgent;
    let expected = agent.act(&ctx, &actions);
    let without = ShantenAgent::diagnose(&ctx, &actions);
    let with =
        ShantenAgent::diagnose_with_options(&ctx, &actions, DiagnosticOptions::WITH_LOOKAHEAD);

    assert_eq!(with.selected_action, expected);
    assert!(
        with.normal_discard_lookahead.is_some(),
        "2手先診断が構築されていない"
    );
    assert!(
        with.normal_discard_lookahead_value.is_some(),
        "2手先の将来打点が構築されていない"
    );
    // 2手先とその将来打点以外の診断はすべて既定の診断と一致する。
    assert_eq!(
        ShantenDecisionDiagnostic {
            normal_discard_lookahead: None,
            normal_discard_lookahead_value: None,
            normal_discard_tenpai_continuation: None,
            normal_discard_current_tenpai_continuation: None,
            ..with
        },
        without
    );
}

#[test]
fn lookahead_covers_every_normal_discard_candidate() {
    let ctx = lookahead_context();
    let actions = lookahead_actions();
    let diagnostic =
        ShantenAgent::diagnose_with_options(&ctx, &actions, DiagnosticOptions::WITH_LOOKAHEAD);

    let normal_discard = diagnostic.normal_discard.expect("normal discard evaluated");
    let lookahead = diagnostic
        .normal_discard_lookahead
        .expect("lookahead built");

    assert!(normal_discard.candidates.len() > 1);
    assert_eq!(lookahead.candidates.len(), normal_discard.candidates.len());
    for (candidate_lookahead, candidate) in lookahead
        .candidates
        .iter()
        .zip(normal_discard.candidates.iter())
    {
        assert_eq!(candidate_lookahead.discard, candidate.evaluation.discard);
        // 現在打牌後の受け入れをそのまま引き継ぐので、対象牌と残枚数が一致する。
        let acceptance = &candidate.evaluation.acceptance_after_discard.tiles;
        let progress: Vec<_> = candidate_lookahead
            .draws_with(DrawTransition::Progress)
            .collect();
        assert_eq!(progress.len(), acceptance.len());
        for (draw, accepted) in progress.into_iter().zip(acceptance.iter()) {
            assert_eq!(draw.draw, accepted.tile);
            assert_eq!(draw.remaining, accepted.remaining);
        }
        // 向聴数を維持する仮想ツモは受け入れへ混ざらない。
        let accepted_tiles = candidate.evaluation.acceptance_after_discard.tile_types();
        assert!(
            candidate_lookahead
                .draws_with(DrawTransition::SameShanten)
                .all(|draw| !accepted_tiles.contains(&draw.draw))
        );
    }
}

#[test]
fn lookahead_free_function_matches_associated_function() {
    let ctx = lookahead_context();
    let actions = lookahead_actions();
    assert_eq!(
        diagnose_shanten_decision_with_options(&ctx, &actions, DiagnosticOptions::WITH_LOOKAHEAD),
        ShantenAgent::diagnose_with_options(&ctx, &actions, DiagnosticOptions::WITH_LOOKAHEAD)
    );
}

fn table_state_variants() -> Vec<TableStateFacts> {
    vec![
        TableStateFacts::default(),
        TableStateFacts {
            remaining_tiles: Some(0),
            honba: Some(0),
            kyotaku_points: Some(0),
            scores: Some([25000; 4]),
            kyoku: Some(1),
        },
        TableStateFacts {
            remaining_tiles: Some(70),
            honba: Some(5),
            kyotaku_points: Some(3000),
            scores: Some([12300, 28700, 40100, 18900]),
            kyoku: Some(4),
        },
        TableStateFacts {
            remaining_tiles: Some(3),
            honba: None,
            kyotaku_points: None,
            scores: Some([-1000, 51000, 25000, 25000]),
            kyoku: None,
        },
    ]
}

#[test]
fn table_state_facts_keep_every_diagnose_entry_point_in_agreement() {
    let base = lookahead_context();
    let actions = lookahead_actions();

    for facts in table_state_variants() {
        let ctx = base.clone().with_table_state_facts(facts);
        let mut agent = ShantenAgent;
        let acted = agent.act(&ctx, &actions);
        let diagnostic = ShantenAgent::diagnose(&ctx, &actions);
        let with_lookahead =
            ShantenAgent::diagnose_with_options(&ctx, &actions, DiagnosticOptions::WITH_LOOKAHEAD);

        assert_eq!(diagnostic.selected_action, acted, "{facts:?}");
        assert_eq!(with_lookahead.selected_action, acted, "{facts:?}");
        assert_eq!(
            with_lookahead.selected_source, diagnostic.selected_source,
            "{facts:?}"
        );
    }
}

// ---- 1向聴の weighted tenpai wait ----

// 12m 68m 444p 5p 789p 567s の門前14枚 (打牌選択側と同じ fixture)。
//
// 打 5p は受け入れが最も広く、打 1m は 45p の両面を残してテンパイ後の待ちが広くなる。
// 合法 Dahai をこの2候補だけに絞り、新しい比較軸で選択が決まる局面にする。
use crate::discard_selection::tests::iishanten_wait_context;

fn iishanten_wait_actions() -> Vec<LegalAction> {
    vec![dahai(0), dahai(53)]
}

#[test]
fn weighted_tenpai_wait_keeps_act_and_diagnose_consistent() {
    // act() / diagnose() / diagnose_with_options(WITH_LOOKAHEAD) の選択が一致する。
    let ctx = iishanten_wait_context();
    let actions = iishanten_wait_actions();

    let mut agent = ShantenAgent;
    let acted = agent.act(&ctx, &actions);
    let diagnosed = ShantenAgent::diagnose(&ctx, &actions);
    let with_lookahead =
        ShantenAgent::diagnose_with_options(&ctx, &actions, DiagnosticOptions::WITH_LOOKAHEAD);

    assert_eq!(diagnosed.selected_action, acted);
    assert_eq!(with_lookahead.selected_action, acted);
    assert_eq!(with_lookahead.normal_discard, diagnosed.normal_discard);

    // 新しい比較軸で選択が決まっている局面であることを固定する。
    let normal_discard = diagnosed.normal_discard.as_ref().expect("evaluated");
    let runner_up = normal_discard
        .candidates
        .iter()
        .find(|candidate| !candidate.selected)
        .expect("runner-up exists");
    assert_eq!(
        runner_up.comparison_reason,
        bot_logic::DiscardComparisonReason::WeightedTenpaiWaitRemaining
    );
    assert_eq!(acted, dahai(0));
}

#[test]
fn push_pull_shares_the_selected_normal_discard() {
    // 押し引きへ渡る攻撃評価は、weighted tenpai wait で選ばれた通常打牌評価そのもの。
    let ctx = iishanten_wait_context();
    let actions = iishanten_wait_actions();

    let diagnostic = ShantenAgent::diagnose(&ctx, &actions);
    let selected = diagnostic
        .normal_discard
        .as_ref()
        .and_then(|normal_discard| normal_discard.selected.as_ref())
        .expect("selected evaluation exists");
    let offense = diagnostic
        .push_pull_inputs
        .as_ref()
        .and_then(|inputs| inputs.offense.as_ref())
        .expect("offense state exists");

    assert_eq!(
        offense.min_shanten_after_discard,
        selected.min_shanten_after_discard()
    );
    assert_eq!(
        offense.acceptance_total_remaining,
        selected.acceptance_total_remaining()
    );
    assert_eq!(
        offense.acceptance_type_count,
        selected.acceptance_type_count()
    );

    // 受け入れの多い runner-up (1手評価だけなら選ばれる候補) の評価は渡っていない。
    let runner_up = diagnostic
        .normal_discard
        .as_ref()
        .expect("evaluated")
        .candidates
        .iter()
        .find(|candidate| !candidate.selected)
        .expect("runner-up exists");
    assert!(runner_up.evaluation.acceptance_total_remaining() > offense.acceptance_total_remaining);
}

// ---- 恒常フリテン診断 ----

// 123m456m789m 123p 5s + ツモ 9s。打 9s で 5s 単騎テンパイ、打 1m では1向聴に落ちる。
fn furiten_hand() -> Vec<TileId> {
    [0u8, 4, 8, 12, 17, 20, 24, 28, 32, 36, 40, 44, 89]
        .iter()
        .map(|&value| tile(value))
        .collect()
}

const FURITEN_DRAWN: u8 = 104;
const FURITEN_WAIT: u8 = 90;

fn furiten_actions() -> Vec<LegalAction> {
    vec![dahai(FURITEN_DRAWN), dahai(0)]
}

// 恒常フリテンだけを見る局面。履歴依存フリテンは両軸とも非該当だと確認済みにして、総合
// ロン可否が自分の河だけで決まるようにする。
fn furiten_context(player_id: Option<u8>, discards: [Vec<TileId>; 4]) -> GameContext {
    let hand = furiten_hand();
    let drawn = tile(FURITEN_DRAWN);
    let mut visible = hand.clone();
    visible.push(drawn);
    for river in &discards {
        visible.extend(river.iter().copied());
    }

    GameContext::from_parts_with_table_state(
        Some(drawn),
        hand,
        vec![],
        None,
        None,
        visible,
        player_id,
        Some(0),
        discards,
        [false; 4],
    )
    .with_history_furiten_facts(bot_logic::HistoryFuritenFacts {
        same_turn: Some(false),
        riichi_missed_win: Some(false),
    })
}

fn furiten_of(
    diagnostic: &ShantenDecisionDiagnostic,
    discard: TileType,
) -> DiscardFuritenDiagnostic {
    diagnostic
        .normal_discard_furiten
        .as_ref()
        .expect("恒常フリテン診断がある")
        .iter()
        .find(|furiten| furiten.discard == discard)
        .expect("打牌候補がある")
        .clone()
}

#[test]
fn own_river_makes_the_reached_tenpai_permanently_furiten() {
    let ctx = furiten_context(
        Some(0),
        [vec![tile(FURITEN_WAIT), tile(108)], vec![], vec![], vec![]],
    );
    let diagnostic = diagnose_matching_act(&ctx, &furiten_actions());

    let furiten = furiten_of(&diagnostic, tile(FURITEN_DRAWN).tile_type());
    let tenpai = furiten.tenpai.as_ref().expect("テンパイになる");
    assert_eq!(
        tenpai.structural_waits,
        vec![tile(FURITEN_WAIT).tile_type()]
    );
    assert_eq!(tenpai.live_waits, tenpai.structural_waits);
    assert_eq!(furiten.permanent_furiten(), Some(PermanentFuriten::Yes));
    assert_eq!(furiten.discarded_waits(), [tile(FURITEN_WAIT).tile_type()]);
    assert_eq!(tenpai.can_ron(), Some(false));
    // フリテンでもツモ側は既存受け入れのまま。
    assert!(tenpai.tsumo_remaining > 0);
}

#[test]
fn only_the_own_river_makes_the_tenpai_furiten() {
    // 同じ待ち牌が他家の河にあるだけではフリテンにならない。
    let ctx = furiten_context(Some(0), [vec![], vec![tile(FURITEN_WAIT)], vec![], vec![]]);
    let diagnostic = diagnose_matching_act(&ctx, &furiten_actions());

    let furiten = furiten_of(&diagnostic, tile(FURITEN_DRAWN).tile_type());
    assert_eq!(furiten.permanent_furiten(), Some(PermanentFuriten::No));
    assert!(furiten.discarded_waits().is_empty());
    assert_eq!(
        furiten.tenpai.as_ref().expect("テンパイになる").can_ron(),
        Some(true)
    );
}

#[test]
fn an_unknown_player_id_leaves_the_furiten_diagnostic_unknown() {
    // player_id が無い場合、player 0 の河を自分の河と推測しない。
    let ctx = furiten_context(None, [vec![tile(FURITEN_WAIT)], vec![], vec![], vec![]]);
    let diagnostic = diagnose_matching_act(&ctx, &furiten_actions());

    assert_eq!(ctx.own_discards(), None);
    let furiten = furiten_of(&diagnostic, tile(FURITEN_DRAWN).tile_type());
    assert_eq!(furiten.permanent_furiten(), Some(PermanentFuriten::Unknown));
    assert_eq!(
        furiten.tenpai.as_ref().expect("テンパイになる").can_ron(),
        None
    );
}

#[test]
fn candidates_that_do_not_reach_tenpai_have_no_furiten_diagnostic() {
    let ctx = furiten_context(Some(0), [vec![tile(FURITEN_WAIT)], vec![], vec![], vec![]]);
    let diagnostic = diagnose_matching_act(&ctx, &furiten_actions());

    let furiten = furiten_of(&diagnostic, tile(0).tile_type());
    assert!(furiten.tenpai.is_none());
    assert_eq!(furiten.permanent_furiten(), None);
    assert!(furiten.discarded_waits().is_empty());
}

#[test]
fn furiten_diagnostic_covers_every_normal_discard_candidate() {
    let ctx = furiten_context(Some(0), [vec![tile(FURITEN_WAIT)], vec![], vec![], vec![]]);
    let diagnostic = diagnose_matching_act(&ctx, &furiten_actions());

    let normal_discard = diagnostic
        .normal_discard
        .as_ref()
        .expect("normal discard evaluated");
    let furiten = diagnostic
        .normal_discard_furiten
        .as_ref()
        .expect("恒常フリテン診断がある");

    assert_eq!(furiten.len(), normal_discard.candidates.len());
    for (furiten, candidate) in furiten.iter().zip(normal_discard.candidates.iter()) {
        assert_eq!(furiten.discard, candidate.evaluation.discard);
        let Some(tenpai) = furiten.tenpai.as_ref() else {
            continue;
        };
        // 待ちと残枚数は既存の受け入れそのままで、フリテンでも書き換えない。
        assert_eq!(
            tenpai.tsumo_remaining,
            candidate.evaluation.acceptance_total_remaining()
        );
        assert_eq!(
            tenpai.tsumo_type_count,
            candidate.evaluation.acceptance_type_count()
        );
    }
}

#[test]
fn the_furiten_diagnostic_does_not_change_the_selected_action() {
    for player_id in [Some(0), None] {
        let ctx = furiten_context(
            player_id,
            [vec![tile(FURITEN_WAIT)], vec![], vec![], vec![]],
        );
        let actions = furiten_actions();

        let mut agent = ShantenAgent;
        let acted = agent.act(&ctx, &actions);
        let diagnostic = ShantenAgent::diagnose(&ctx, &actions);
        let with_lookahead =
            ShantenAgent::diagnose_with_options(&ctx, &actions, DiagnosticOptions::WITH_LOOKAHEAD);

        assert_eq!(acted, dahai(FURITEN_DRAWN));
        assert_eq!(diagnostic.selected_action, acted);
        assert_eq!(with_lookahead.selected_action, acted);
        assert_eq!(
            with_lookahead.normal_discard_furiten,
            diagnostic.normal_discard_furiten
        );
    }
}

// 123m456m 3456789s + ツモ 1p。打 1p で 3s / 6s / 9s の3面待ちテンパイになる。
// 3s は手牌に1枚だけなので、残り3枚 (81 / 82 / 83) を見え牌にできる。
fn three_sided_hand() -> Vec<TileId> {
    [0u8, 4, 8, 12, 17, 20, 80, 84, 89, 92, 96, 100, 104]
        .iter()
        .map(|&value| tile(value))
        .collect()
}

fn three_sided_context(extra_visible: &[u8], own_river: &[u8]) -> GameContext {
    three_sided_context_with_player_id(Some(0), extra_visible, own_river)
}

fn three_sided_context_with_player_id(
    player_id: Option<u8>,
    extra_visible: &[u8],
    own_river: &[u8],
) -> GameContext {
    let hand = three_sided_hand();
    let drawn = tile(36);
    let discards = [
        own_river.iter().map(|&value| tile(value)).collect(),
        vec![],
        vec![],
        vec![],
    ];

    let mut visible = hand.clone();
    visible.push(drawn);
    visible.extend(extra_visible.iter().map(|&value| tile(value)));
    for river in &discards {
        visible.extend(river.iter().copied());
    }

    GameContext::from_parts_with_table_state(
        Some(drawn),
        hand,
        vec![],
        None,
        None,
        visible,
        player_id,
        Some(0),
        discards,
        [false; 4],
    )
}

#[test]
fn a_fully_visible_discarded_wait_still_reports_furiten() {
    // 待ち 3s を自分が捨てていて 3s が4枚とも見えている局面。3s は既存受け入れから消えるが、
    // 恒常フリテンは解除されない。
    let ctx = three_sided_context(&[82, 83], &[81]);
    let actions = vec![dahai(36), dahai(0)];
    let diagnostic = diagnose_matching_act(&ctx, &actions);

    let furiten = furiten_of(&diagnostic, tile(36).tile_type());
    let tenpai = furiten.tenpai.as_ref().expect("テンパイになる");
    let three_sou = tile(80).tile_type();

    assert_eq!(tenpai.structural_waits.len(), 3);
    assert!(tenpai.structural_waits.contains(&three_sou));
    assert!(!tenpai.live_waits.contains(&three_sou));
    assert_eq!(furiten.permanent_furiten(), Some(PermanentFuriten::Yes));
    assert_eq!(furiten.discarded_waits(), [three_sou]);
    assert_eq!(tenpai.can_ron(), Some(false));

    // ツモ側は見え牌を反映した既存受け入れのまま。
    let evaluation = diagnostic
        .normal_discard
        .as_ref()
        .expect("normal discard evaluated")
        .candidates
        .iter()
        .find(|candidate| candidate.evaluation.discard == tile(36).tile_type())
        .map(|candidate| candidate.evaluation.clone())
        .expect("打 1p の評価がある");
    assert_eq!(
        tenpai.tsumo_remaining,
        evaluation.acceptance_total_remaining()
    );
    assert_eq!(tenpai.tsumo_type_count, evaluation.acceptance_type_count());
    assert_eq!(tenpai.tsumo_type_count, 2);
}

#[test]
fn the_last_visible_copy_of_a_wait_does_not_change_the_furiten_diagnostic() {
    // 残枚数 1 → 0 の境界を跨いでも、フリテン判定・重複した待ち牌・ロン可否は変わらない。
    let actions = vec![dahai(36), dahai(0)];
    let one_left = diagnose_matching_act(&three_sided_context(&[82], &[81]), &actions);
    let none_left = diagnose_matching_act(&three_sided_context(&[82, 83], &[81]), &actions);

    let discard = tile(36).tile_type();
    let with_one_left = furiten_of(&one_left, discard);
    let with_none_left = furiten_of(&none_left, discard);

    assert_eq!(
        with_one_left.tenpai.as_ref().unwrap().live_waits.len(),
        with_none_left.tenpai.as_ref().unwrap().live_waits.len() + 1
    );
    assert_eq!(
        with_one_left.tenpai.as_ref().unwrap().furiten,
        with_none_left.tenpai.as_ref().unwrap().furiten
    );
    assert_eq!(
        with_one_left.permanent_furiten(),
        with_none_left.permanent_furiten()
    );
    assert_eq!(
        with_one_left.discarded_waits(),
        with_none_left.discarded_waits()
    );
    assert_eq!(
        with_one_left.tenpai.as_ref().unwrap().can_ron(),
        with_none_left.tenpai.as_ref().unwrap().can_ron()
    );
    assert_eq!(
        with_none_left.permanent_furiten(),
        Some(PermanentFuriten::Yes)
    );
}

#[test]
fn act_path_does_not_build_the_furiten_diagnostic() {
    let ctx = furiten_context(Some(0), [vec![tile(FURITEN_WAIT)], vec![], vec![], vec![]]);

    let mut diagnostics = DecisionDiagnostics::disabled();
    let _ = ShantenAgent.decide_with_diagnostics(&ctx, &furiten_actions(), &mut diagnostics);

    assert!(diagnostics.normal_discard_furiten.is_none());
}

// ---- 履歴依存フリテンの評価時点 ----

fn history_facts(
    same_turn: Option<bool>,
    riichi_missed_win: Option<bool>,
) -> bot_logic::HistoryFuritenFacts {
    bot_logic::HistoryFuritenFacts {
        same_turn,
        riichi_missed_win,
    }
}

// 打 9s で 5s 単騎テンパイになる門前局面。自分の河は空だと確定していて恒常フリテンにはなら
// ないので、総合ロン可否の差は履歴依存フリテンと評価時点だけで決まる。
//
// `drawn` が true なら 9s を自摸牌として渡し、false なら同じ14枚を手牌として渡して
// 「自分のツモを経たと確認できない打牌」にする。
fn history_furiten_context(
    drawn: bool,
    history: bot_logic::HistoryFuritenFacts,
    own_river: &[TileId],
) -> GameContext {
    let mut hand = furiten_hand();
    let drawn_tile = tile(FURITEN_DRAWN);
    let mut visible = hand.clone();
    visible.push(drawn_tile);
    visible.extend(own_river.iter().copied());

    let drawn_tile = if drawn {
        Some(drawn_tile)
    } else {
        hand.push(drawn_tile);
        None
    };

    let mut discards: [Vec<TileId>; 4] = Default::default();
    discards[0] = own_river.to_vec();

    GameContext::from_parts_with_table_state(
        drawn_tile,
        hand,
        vec![],
        None,
        None,
        visible,
        Some(0),
        Some(0),
        discards,
        [false; 4],
    )
    .with_history_furiten_facts(history)
}

// 1z の Pon 済みで、打 9s で 5s 単騎テンパイになる局面。自摸牌は渡さないので、鳴きの直後に
// 切る打牌と同じく「自分のツモを経ていない打牌」になる。
fn history_furiten_meld_context(history: bot_logic::HistoryFuritenFacts) -> GameContext {
    let hand: Vec<TileId> = [0u8, 4, 8, 12, 17, 20, 24, 28, 32, 89, FURITEN_DRAWN]
        .iter()
        .map(|&value| tile(value))
        .collect();
    let mut melds: [Vec<crate::meld::Meld>; 4] = Default::default();
    melds[0] = vec![pon_meld()];

    GameContext::from_parts_with_melds(
        None,
        hand.clone(),
        vec![],
        None,
        None,
        hand,
        Some(0),
        Some(0),
        Default::default(),
        [false; 4],
        melds,
    )
    .with_history_furiten_facts(history)
}

// 選択した打牌後の待ち診断。全候補診断と押し引きが同じ評価時点の値を共有することも固定する。
//
// リーチを選んだ場合も切る牌は通常打牌 selection の結果なので、待ちは通常打牌から引く。
fn selected_tenpai_wait(
    ctx: &GameContext,
    actions: &[LegalAction],
) -> (ShantenDecisionDiagnostic, TenpaiWaitAvailability) {
    let diagnostic = diagnose_matching_act(ctx, actions);
    let Some(LegalAction::Dahai { tile }) = &diagnostic.normal_discard_action else {
        panic!("打牌が選ばれる: {:?}", diagnostic.normal_discard_action);
    };
    let tenpai = furiten_of(&diagnostic, tile.tile_type())
        .tenpai
        .expect("テンパイになる");

    // 押し引きへ転記されるロン可否も同じ総合値になる。
    assert_eq!(
        diagnostic
            .push_pull_inputs
            .as_ref()
            .and_then(|inputs| inputs.offense)
            .and_then(|offense| offense.tenpai_wait_after_discard)
            .map(|wait| wait.can_ron),
        Some(tenpai.can_ron())
    );
    (diagnostic, tenpai)
}

#[test]
fn a_normal_draw_discard_clears_the_same_turn_furiten() {
    // 現在は同巡内フリテンでも、自摸 → 今回の打牌を終えた時点では解除されている。
    let ctx = history_furiten_context(true, history_facts(Some(true), Some(false)), &[]);
    let (diagnostic, tenpai) = selected_tenpai_wait(&ctx, &furiten_actions());

    assert_eq!(diagnostic.history_furiten.same_turn, Some(true));
    assert_eq!(tenpai.permanent_furiten(), PermanentFuriten::No);
    assert_eq!(tenpai.history_furiten().same_turn, Some(false));
    assert_eq!(tenpai.history_furiten().riichi_missed_win, Some(false));
    assert_eq!(tenpai.can_ron(), Some(true));
}

#[test]
fn a_discard_after_a_meld_keeps_the_same_turn_furiten() {
    // 鳴きの後は自分のツモを経ていないので、同巡内フリテンは解除しない。
    let ctx = history_furiten_meld_context(history_facts(Some(true), Some(false)));
    let actions = vec![dahai(FURITEN_DRAWN), dahai(0)];
    let (_, tenpai) = selected_tenpai_wait(&ctx, &actions);

    assert!(!ctx.is_after_own_draw());
    assert_eq!(tenpai.permanent_furiten(), PermanentFuriten::No);
    assert_eq!(tenpai.history_furiten().same_turn, Some(true));
    assert_eq!(tenpai.can_ron(), Some(false));
}

#[test]
fn the_riichi_missed_win_furiten_survives_a_normal_draw_discard() {
    // 同巡内フリテンだけが解除され、リーチ後見逃しは局終了まで残る。
    let ctx = history_furiten_context(true, history_facts(Some(true), Some(true)), &[]);
    let (_, tenpai) = selected_tenpai_wait(&ctx, &furiten_actions());

    assert_eq!(tenpai.permanent_furiten(), PermanentFuriten::No);
    assert_eq!(tenpai.history_furiten().same_turn, Some(false));
    assert_eq!(tenpai.history_furiten().riichi_missed_win, Some(true));
    assert_eq!(tenpai.can_ron(), Some(false));
}

#[test]
fn a_normal_draw_discard_resolves_an_unknown_same_turn_furiten() {
    // 現在は unknown でも、自摸 → 打牌を終えた時点なら Some(false) と確定できる。
    let ctx = history_furiten_context(true, history_facts(None, Some(false)), &[]);
    let (diagnostic, tenpai) = selected_tenpai_wait(&ctx, &furiten_actions());

    assert_eq!(diagnostic.history_furiten.same_turn, None);
    assert_eq!(tenpai.history_furiten().same_turn, Some(false));
    assert_eq!(tenpai.can_ron(), Some(true));
}

#[test]
fn an_unconfirmed_own_draw_keeps_the_same_turn_furiten_unknown() {
    // 自摸を確認できない打牌では unknown を Some(false) と推測しない。
    let ctx = history_furiten_context(false, history_facts(None, Some(false)), &[]);
    let (_, tenpai) = selected_tenpai_wait(&ctx, &furiten_actions());

    assert!(!ctx.is_after_own_draw());
    assert_eq!(tenpai.permanent_furiten(), PermanentFuriten::No);
    assert_eq!(tenpai.history_furiten().same_turn, None);
    assert_eq!(tenpai.can_ron(), None);
}

#[test]
fn permanent_furiten_makes_ron_impossible_even_with_unknown_history() {
    // 履歴依存フリテンが両軸とも unknown でも、恒常フリテンが確定していればロンできない。
    let ctx = history_furiten_context(
        true,
        bot_logic::HistoryFuritenFacts::default(),
        &[tile(FURITEN_WAIT)],
    );
    let (_, tenpai) = selected_tenpai_wait(&ctx, &furiten_actions());

    assert_eq!(tenpai.permanent_furiten(), PermanentFuriten::Yes);
    assert_eq!(tenpai.history_furiten().riichi_missed_win, None);
    assert_eq!(tenpai.can_ron(), Some(false));
}

#[test]
fn every_candidate_uses_the_same_evaluation_point_as_the_selected_discard() {
    // 選択候補だけ履歴依存フリテンを補正して、全候補診断は現在時点のまま、という不一致を
    // 作らない。
    let ctx = history_furiten_context(true, history_facts(Some(true), Some(false)), &[]);
    let diagnostic = diagnose_matching_act(&ctx, &furiten_actions());
    let furiten = diagnostic
        .normal_discard_furiten
        .as_ref()
        .expect("フリテン診断がある");

    assert!(furiten.iter().any(|candidate| candidate.tenpai.is_some()));
    for candidate in furiten {
        let Some(tenpai) = candidate.tenpai.as_ref() else {
            continue;
        };
        assert_eq!(
            tenpai.history_furiten(),
            ctx.history_furiten_after_own_discard()
        );
    }
}

#[test]
fn history_furiten_does_not_change_the_reach_or_push_pull_policy() {
    // ロン可否が変わっても、リーチ採否・押し引きの mode / reason・最終 action は変えない。
    let actions: Vec<LegalAction> = furiten_actions()
        .into_iter()
        .chain([LegalAction::Reach])
        .collect();
    let can_ron = history_furiten_context(true, history_facts(Some(false), Some(false)), &[]);
    let cannot_ron = history_furiten_context(true, history_facts(Some(true), Some(true)), &[]);

    let (can_ron_diagnostic, can_ron_wait) = selected_tenpai_wait(&can_ron, &actions);
    let (cannot_ron_diagnostic, cannot_ron_wait) = selected_tenpai_wait(&cannot_ron, &actions);

    // 前提として総合ロン可否だけが違う。
    assert_eq!(can_ron_wait.can_ron(), Some(true));
    assert_eq!(cannot_ron_wait.can_ron(), Some(false));
    assert_eq!(
        can_ron_wait.permanent_furiten(),
        cannot_ron_wait.permanent_furiten()
    );

    assert_eq!(
        can_ron_diagnostic.selected_action,
        cannot_ron_diagnostic.selected_action
    );
    assert_eq!(
        can_ron_diagnostic.selected_source,
        cannot_ron_diagnostic.selected_source
    );

    let reach_of = |diagnostic: &ShantenDecisionDiagnostic| {
        let reach = diagnostic.reach.as_ref().expect("リーチを検討している");
        (reach.selected.clone(), reach.reason, reach.should_reach())
    };
    assert_eq!(
        reach_of(&can_ron_diagnostic),
        reach_of(&cannot_ron_diagnostic)
    );

    let push_pull_of = |diagnostic: &ShantenDecisionDiagnostic| {
        let decision = diagnostic
            .push_pull_decision
            .as_ref()
            .expect("押し引きを判定している");
        (decision.mode, decision.reason)
    };
    assert_eq!(
        push_pull_of(&can_ron_diagnostic),
        push_pull_of(&cannot_ron_diagnostic)
    );
}
