use super::common::*;
use crate::action::LegalAction;
use crate::defense::*;

#[test]
fn defense_candidate_diagnostic_reports_half_suji() {
    // 壁なしの片スジ。bool は false でも、純粋なスジ rank から HalfSuji と分かる。
    let context = half_suji_regression_context();
    let action = LegalAction::Dahai { tile: tile(48) };
    let candidate = DefenseCandidateDiagnostic::for_dahai_action(&context, &action, false).unwrap();

    assert_eq!(candidate.wall_rank, Some(WallRank::NoWall));
    assert_eq!(candidate.suji_for_all_reached, Some(false));
    assert_eq!(
        candidate.suji_safety_rank_for_all_reached,
        Some(SujiSafetyRank::HalfSuji)
    );
    assert_eq!(
        candidate.suited_safety_rank,
        Some(SuitedSafetyRank::HalfSuji)
    );
}

#[test]
fn defense_candidate_diagnostic_reports_half_suji_behind_one_chance_wall() {
    // 4p は 1p だけ河にある片スジ。経路 [2p,3p] は 2p 4枚で Blocked、[5p,6p] は 5p 3枚で
    // OneChance。suited_safety_rank は壁由来の OneChance になるが、純粋なスジ rank は HalfSuji。
    let visible = vec![
        tile(40),
        tile(41),
        tile(42),
        tile(43),
        tile(52),
        tile(53),
        tile(54),
    ];
    let context = suited_context(
        visible,
        [vec![], vec![tile(36)], vec![], vec![]],
        [false, true, false, false],
    );
    let action = LegalAction::Dahai { tile: tile(48) };
    let candidate = DefenseCandidateDiagnostic::for_dahai_action(&context, &action, false).unwrap();

    assert_eq!(candidate.wall_rank, Some(WallRank::OneChance));
    assert_eq!(candidate.suji_for_all_reached, Some(false));
    assert_eq!(
        candidate.suji_safety_rank_for_all_reached,
        Some(SujiSafetyRank::HalfSuji)
    );
    assert_eq!(
        candidate.suited_safety_rank,
        Some(SuitedSafetyRank::OneChance)
    );
}

#[test]
fn defense_candidate_diagnostic_reports_no_suji_and_full_suji() {
    // 無スジの 1m は NoSuji、完全スジの 7s は Suji。bool と rank の対応も確認する。
    let context = half_suji_regression_context();

    let action = LegalAction::Dahai { tile: tile(0) };
    let candidate = DefenseCandidateDiagnostic::for_dahai_action(&context, &action, false).unwrap();
    assert_eq!(candidate.suji_for_all_reached, Some(false));
    assert_eq!(
        candidate.suji_safety_rank_for_all_reached,
        Some(SujiSafetyRank::NoSuji)
    );

    let action = LegalAction::Dahai { tile: tile(96) };
    let candidate = DefenseCandidateDiagnostic::for_dahai_action(&context, &action, true).unwrap();
    assert_eq!(candidate.suji_for_all_reached, Some(true));
    assert_eq!(
        candidate.suji_safety_rank_for_all_reached,
        Some(SujiSafetyRank::Suji)
    );
    assert_eq!(candidate.suited_safety_rank, Some(SuitedSafetyRank::Suji));
}

#[test]
fn defense_fallback_diagnostic_reports_pure_suji_safety_rank() {
    // 選択牌側でも同じ rank を保持する。7s は完全スジ、4p は片スジ。
    let context = half_suji_regression_context();
    let actions = vec![
        LegalAction::Dahai { tile: tile(48) },
        LegalAction::Dahai { tile: tile(96) },
    ];
    let (action, kind) = select_defense_fallback_action_with_kind(&context, &actions).unwrap();
    let diagnostic = DefenseFallbackDiagnostic::from_selection(&context, action, kind);

    assert_eq!(diagnostic.selected_action, "7s");
    assert_eq!(diagnostic.selected_suji_for_all_reached, Some(true));
    assert_eq!(
        diagnostic.selected_suji_safety_rank_for_all_reached,
        Some(SujiSafetyRank::Suji)
    );

    let half_suji = LegalAction::Dahai { tile: tile(48) };
    let diagnostic = DefenseFallbackDiagnostic::from_selection(
        &context,
        &half_suji,
        DefenseFallbackKind::SuitedSafety(SuitedSafetyRank::HalfSuji),
    );
    assert_eq!(diagnostic.selected_suji_for_all_reached, Some(false));
    assert_eq!(
        diagnostic.selected_suji_safety_rank_for_all_reached,
        Some(SujiSafetyRank::HalfSuji)
    );
}

#[test]
fn defense_fallback_diagnostic_from_selection_for_suited_suji() {
    // 1s をスジとして選んだ場合の診断データ。壁は NoWall、suji は true、suited safety は Suji。
    let context = suited_context(
        vec![tile(56), tile(57), tile(58)],
        [vec![], vec![tile(84)], vec![], vec![]],
        [false, true, false, false],
    );
    let action = LegalAction::Dahai { tile: tile(72) };
    let diagnostic = DefenseFallbackDiagnostic::from_selection(
        &context,
        &action,
        DefenseFallbackKind::SuitedSafety(SuitedSafetyRank::Suji),
    );
    assert_eq!(diagnostic.selected_action, "1s");
    assert_eq!(
        diagnostic.selected_kind,
        DefenseFallbackKind::SuitedSafety(SuitedSafetyRank::Suji)
    );
    assert_eq!(diagnostic.opponent_reach_count, 1);
    assert!(!diagnostic.selected_genbutsu_for_all);
    assert_eq!(diagnostic.selected_honor_safety_rank, None);
    assert_eq!(diagnostic.selected_wall_rank, Some(WallRank::NoWall));
    assert_eq!(diagnostic.selected_suji_for_all_reached, Some(true));
    assert_eq!(
        diagnostic.selected_suji_safety_rank_for_all_reached,
        Some(SujiSafetyRank::Suji)
    );
    assert_eq!(
        diagnostic.selected_suited_safety_rank,
        Some(SuitedSafetyRank::Suji)
    );
}

#[test]
fn defense_fallback_diagnostic_from_selection_for_genbutsu() {
    // 6p を現物として選んだ場合の診断データ。genbutsu は true、数牌 safety も算出される。
    let context = suited_context(
        vec![tile(56), tile(57), tile(58)],
        [vec![], vec![tile(59)], vec![], vec![]],
        [false, true, false, false],
    );
    let action = LegalAction::Dahai { tile: tile(56) };
    let diagnostic =
        DefenseFallbackDiagnostic::from_selection(&context, &action, DefenseFallbackKind::Genbutsu);
    assert_eq!(diagnostic.selected_action, "6p");
    assert_eq!(diagnostic.selected_kind, DefenseFallbackKind::Genbutsu);
    assert!(diagnostic.selected_genbutsu_for_all);
    assert_eq!(diagnostic.selected_wall_rank, Some(WallRank::NoWall));
}

#[test]
fn defense_fallback_diagnostic_from_selection_for_honor() {
    // 字牌を選んだ場合、壁・スジ・数牌 safety は None で字牌 safety だけ算出される。
    let context = suited_context(
        vec![tile(108), tile(109)],
        Default::default(),
        [false, true, false, false],
    );
    let action = LegalAction::Dahai { tile: tile(108) };
    let diagnostic = DefenseFallbackDiagnostic::from_selection(
        &context,
        &action,
        DefenseFallbackKind::HonorSafety(HonorSafetyRank::TwoVisible),
    );
    assert_eq!(diagnostic.selected_action, "E");
    assert_eq!(
        diagnostic.selected_honor_safety_rank,
        Some(HonorSafetyRank::TwoVisible)
    );
    assert_eq!(diagnostic.selected_wall_rank, None);
    assert_eq!(diagnostic.selected_suji_for_all_reached, None);
    assert_eq!(diagnostic.selected_suji_safety_rank_for_all_reached, None);
    assert_eq!(diagnostic.selected_suited_safety_rank, None);
}

// ---- 合法 Dahai ごとの防御候補診断 ----

#[test]
fn defense_candidate_diagnostic_for_suited_tile() {
    // 2m は 4m 4枚見えで NoChance。スジではないので suji は false。
    let context = suited_context(
        vec![tile(12), tile(13), tile(14), tile(15)],
        Default::default(),
        [false, true, false, false],
    );
    let action = LegalAction::Dahai { tile: tile(4) };
    let candidate = DefenseCandidateDiagnostic::for_dahai_action(&context, &action, true).unwrap();

    assert_eq!(candidate.action, action);
    assert_eq!(candidate.tile, tile(4).tile_type());
    assert!(candidate.selected);
    assert!(!candidate.genbutsu_for_all);
    assert_eq!(candidate.honor_safety_rank, None);
    assert_eq!(candidate.wall_rank, Some(WallRank::NoChance));
    assert_eq!(candidate.suji_for_all_reached, Some(false));
    assert_eq!(
        candidate.suited_safety_rank,
        Some(SuitedSafetyRank::NoChance)
    );
}

#[test]
fn defense_candidate_diagnostic_for_honor_tile() {
    // 東が2枚見え。字牌なので壁・スジ・数牌 safety は None。
    let context = suited_context(
        vec![tile(108), tile(109)],
        Default::default(),
        [false, true, false, false],
    );
    let action = LegalAction::Dahai { tile: tile(108) };
    let candidate = DefenseCandidateDiagnostic::for_dahai_action(&context, &action, false).unwrap();

    assert_eq!(candidate.tile, tile(108).tile_type());
    assert!(!candidate.selected);
    assert_eq!(
        candidate.honor_safety_rank,
        Some(HonorSafetyRank::TwoVisible)
    );
    assert_eq!(candidate.wall_rank, None);
    assert_eq!(candidate.suji_for_all_reached, None);
    assert_eq!(candidate.suji_safety_rank_for_all_reached, None);
    assert_eq!(candidate.suited_safety_rank, None);
}

#[test]
fn defense_candidate_diagnostic_reports_opponent_honor_value() {
    let context = single_reacher_honor_context(1);
    let candidates: Vec<Option<OpponentHonorValue>> = [tile(108), tile(120), tile(0)]
        .into_iter()
        .map(|tile| {
            let action = LegalAction::Dahai { tile };
            DefenseCandidateDiagnostic::for_dahai_action(&context, &action, false)
                .unwrap()
                .opponent_honor_value
        })
        .collect();

    assert_eq!(
        candidates,
        vec![
            Some(OpponentHonorValue::DoubleWind),
            Some(OpponentHonorValue::GuestWind),
            None,
        ]
    );
}

#[test]
fn defense_candidate_diagnostic_opponent_honor_value_excludes_genbutsu_player() {
    let discards = [vec![], vec![], vec![tile(116)], vec![]];
    let context = honor_value_context(
        Some(honor(EAST)),
        Some(0),
        [false, true, true, false],
        discards,
        vec![],
    );
    let action = LegalAction::Dahai { tile: tile(117) };
    let candidate = DefenseCandidateDiagnostic::for_dahai_action(&context, &action, false).unwrap();

    assert_eq!(
        candidate.opponent_honor_value,
        Some(OpponentHonorValue::GuestWind)
    );
}

#[test]
fn defense_fallback_diagnostic_reports_selected_opponent_honor_value() {
    let context = single_reacher_honor_context(1);
    let action = LegalAction::Dahai { tile: tile(120) };
    let diagnostic = DefenseFallbackDiagnostic::from_selection(
        &context,
        &action,
        DefenseFallbackKind::HonorSafety(HonorSafetyRank::NoVisible),
    );

    assert_eq!(diagnostic.selected_action, "N");
    assert_eq!(
        diagnostic.selected_honor_safety_rank,
        Some(HonorSafetyRank::NoVisible)
    );
    assert_eq!(
        diagnostic.selected_opponent_honor_value,
        Some(OpponentHonorValue::GuestWind)
    );
}

#[test]
fn defense_fallback_diagnostic_has_no_opponent_honor_value_for_suited_tile() {
    let context = single_reacher_honor_context(1);
    let action = LegalAction::Dahai { tile: tile(0) };
    let diagnostic = DefenseFallbackDiagnostic::from_selection(
        &context,
        &action,
        DefenseFallbackKind::SuitedSafety(SuitedSafetyRank::NoSafety),
    );

    assert_eq!(diagnostic.selected_opponent_honor_value, None);
}

#[test]
fn defense_candidate_diagnostic_marks_genbutsu() {
    let discards = [vec![], vec![tile(16)], vec![], vec![]];
    let context = table_state_context(Some(0), None, discards, [false, true, false, false]);
    let action = LegalAction::Dahai { tile: tile(17) };
    let candidate = DefenseCandidateDiagnostic::for_dahai_action(&context, &action, false).unwrap();

    assert!(candidate.genbutsu_for_all);
}

#[test]
fn defense_candidate_diagnostic_skips_non_dahai_actions() {
    let context = suited_context(vec![], Default::default(), [false, true, false, false]);
    assert_eq!(
        DefenseCandidateDiagnostic::for_dahai_action(&context, &LegalAction::None, false),
        None
    );
    assert_eq!(
        DefenseCandidateDiagnostic::for_dahai_action(
            &context,
            &LegalAction::Pon {
                tile: tile(108),
                consumed: vec![tile(109), tile(110)],
            },
            false
        ),
        None
    );
}

#[test]
fn defense_candidates_keep_legal_action_order_and_mark_selected() {
    let discards = [vec![], vec![tile(16)], vec![], vec![]];
    let context = table_state_context(Some(0), None, discards, [false, true, false, false]);
    let actions = vec![
        LegalAction::Dahai { tile: tile(108) },
        LegalAction::None,
        LegalAction::Dahai { tile: tile(17) },
    ];
    let selected = LegalAction::Dahai { tile: tile(17) };

    let candidates =
        DefenseCandidateDiagnostic::for_legal_actions(&context, &actions, Some(&selected));

    assert_eq!(candidates.len(), 2);
    assert_eq!(candidates[0].tile, tile(108).tile_type());
    assert!(!candidates[0].selected);
    assert_eq!(candidates[1].tile, tile(17).tile_type());
    assert!(candidates[1].selected);
}

#[test]
fn defense_decision_diagnostic_holds_actual_selection() {
    // 現物 5m が選ばれる局面。実際の選択結果をそのまま保持し、候補評価も全合法 Dahai 分持つ。
    let discards = [vec![], vec![tile(16)], vec![], vec![]];
    let context = table_state_context(Some(0), None, discards, [false, true, false, false]);
    let actions = vec![
        LegalAction::Dahai { tile: tile(108) },
        LegalAction::Dahai { tile: tile(17) },
    ];
    let selected = select_defense_fallback_action_with_kind(&context, &actions);

    let diagnostic = DefenseDecisionDiagnostic::from_selection(&context, &actions, selected);

    assert_eq!(
        diagnostic.selected_kind(),
        Some(DefenseFallbackKind::Genbutsu)
    );
    assert_eq!(
        diagnostic.selected.as_ref().unwrap().selected_action,
        "5m".to_string()
    );
    assert_eq!(diagnostic.candidates.len(), 2);
    assert_eq!(
        diagnostic
            .candidates
            .iter()
            .filter(|candidate| candidate.selected)
            .count(),
        1
    );
}

#[test]
fn defense_decision_diagnostic_keeps_candidates_without_selection() {
    // 防御 fallback 候補が無い(全て NoSafety)局面でも、候補評価は保持する。
    let context = suited_context(vec![], Default::default(), [false, true, false, false]);
    let actions = vec![
        LegalAction::Dahai { tile: tile(0) },
        LegalAction::Dahai { tile: tile(56) },
    ];
    let selected = select_defense_fallback_action_with_kind(&context, &actions);
    assert_eq!(selected, None);

    let diagnostic = DefenseDecisionDiagnostic::from_selection(&context, &actions, selected);

    assert_eq!(diagnostic.selected, None);
    assert_eq!(diagnostic.selected_kind(), None);
    assert_eq!(diagnostic.candidates.len(), 2);
    assert!(
        diagnostic
            .candidates
            .iter()
            .all(|candidate| !candidate.selected)
    );
    assert!(
        diagnostic
            .candidates
            .iter()
            .all(|candidate| candidate.suited_safety_rank == Some(SuitedSafetyRank::NoSafety))
    );
}

#[test]
fn defense_diagnostics_carry_the_suited_safety_evidence() {
    // 1m は 4m 河でスジ、経路 [2m,3m] は 2m 3枚で OneChance。診断からは両方の根拠が分かる。
    let context = suited_context(
        vec![tile(4), tile(5), tile(6)],
        [vec![], vec![tile(12)], vec![], vec![]],
        [false, true, false, false],
    );
    let action = LegalAction::Dahai { tile: tile(0) };
    let expected = Some(SuitedSafetyEvidence {
        wall_rank: WallRank::OneChance,
        suji_rank: SujiSafetyRank::Suji,
    });

    let candidate = DefenseCandidateDiagnostic::for_dahai_action(&context, &action, true).unwrap();
    assert_eq!(candidate.suited_safety_evidence, expected);
    assert_eq!(
        candidate.suited_safety_rank,
        Some(SuitedSafetyRank::OneChance)
    );

    let diagnostic = DefenseFallbackDiagnostic::from_selection(
        &context,
        &action,
        DefenseFallbackKind::SuitedSafety(SuitedSafetyRank::OneChance),
    );
    assert_eq!(diagnostic.selected_suited_safety_evidence, expected);
    assert_eq!(
        diagnostic.selected_suited_safety_rank,
        Some(SuitedSafetyRank::OneChance)
    );
}

#[test]
fn defense_diagnostics_have_no_suited_safety_evidence_for_honor() {
    let context = suited_context(vec![], Default::default(), [false, true, false, false]);
    let action = LegalAction::Dahai { tile: tile(108) };

    let candidate = DefenseCandidateDiagnostic::for_dahai_action(&context, &action, false).unwrap();
    assert_eq!(candidate.suited_safety_evidence, None);

    let diagnostic =
        DefenseFallbackDiagnostic::from_selection(&context, &action, DefenseFallbackKind::Genbutsu);
    assert_eq!(diagnostic.selected_suited_safety_evidence, None);
}
