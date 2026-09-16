use super::*;

use bot_core::{
    CombinedDefenseCategory, DefenseFallbackKind, HonorSafetyRank, LegalAction,
    OpenHandDefenseCategory, SuitedSafetyRank,
};
use bot_logic::{TileId, TileType};

fn dahai(mjai: &str) -> LegalAction {
    let tile_type = TileType::from_mjai_type_str(mjai).expect("牌種として読める");
    LegalAction::Dahai {
        tile: TileId::copies(tile_type)
            .find(|tile| !tile.is_red())
            .expect("黒牌がある"),
    }
}

fn evidence(player: usize, ron_capable_weight: u128, tenpai_weight: u128) -> PlayerRonRiskEvidence {
    PlayerRonRiskEvidence {
        player,
        evidence: RonRiskEvidence {
            ron_capable_weight,
            tenpai_weight,
        },
    }
}

fn genbutsu(mjai: &str, rank: usize) -> ForcedFoldRankedCandidate {
    ForcedFoldRankedCandidate {
        action: dahai(mjai),
        rank,
        defense_kind: ForcedFoldDefenseKind::Reach(DefenseFallbackKind::Genbutsu),
        player_ron_risk_evidence: Some(vec![evidence(1, 0, 3_812)]),
    }
}

fn exact(mjai: &str, rank: usize, ron_capable_weight: u128) -> ForcedFoldRankedCandidate {
    ForcedFoldRankedCandidate {
        action: dahai(mjai),
        rank,
        defense_kind: ForcedFoldDefenseKind::Reach(DefenseFallbackKind::ExactRonRisk),
        player_ron_risk_evidence: Some(vec![evidence(1, ron_capable_weight, 3_791)]),
    }
}

fn heuristic(mjai: &str, rank: usize, kind: DefenseFallbackKind) -> ForcedFoldRankedCandidate {
    ForcedFoldRankedCandidate {
        action: dahai(mjai),
        rank,
        defense_kind: ForcedFoldDefenseKind::Reach(kind),
        player_ron_risk_evidence: None,
    }
}

fn diagnostic(
    defense_kind: ForcedFoldDefenseKind,
    ranked_candidates: Vec<ForcedFoldRankedCandidate>,
) -> ForcedFoldResult {
    let selected_action = ranked_candidates
        .first()
        .expect("候補が1件以上ある")
        .action
        .clone();
    Ok(ForcedFoldDiagnostic {
        selected_action,
        defense_kind,
        ranked_candidates,
        defense: None,
        open_hand_defense: None,
        combined_defense: None,
    })
}

fn reach_diagnostic(ranked_candidates: Vec<ForcedFoldRankedCandidate>) -> ForcedFoldResult {
    diagnostic(
        ForcedFoldDefenseKind::Reach(DefenseFallbackKind::Genbutsu),
        ranked_candidates,
    )
}

fn ranks(summary: &str) -> Vec<String> {
    summary
        .lines()
        .filter(|line| line.starts_with("  rank "))
        .map(|line| line.trim().to_string())
        .collect()
}

#[test]
fn summary_shows_the_production_top_three_candidates() {
    let summary = format_forced_fold_summary(&reach_diagnostic(vec![
        genbutsu("E", 1),
        exact("N", 2, 0),
        exact("1m", 3, 104),
        exact("9p", 4, 210),
        exact("7s", 5, 320),
    ]));

    assert_eq!(
        summary,
        "Summary\n  mode: ForcedFold\n  source: DefenseFallback\
         \n\n  rank 1: E\n    ron safe: yes\n    reason: Genbutsu\
         \n\n  rank 2: N\n    ron safe: no\n    model risk: 0.00%\n    evidence: 0 / 3791\
         \n\n  rank 3: 1m\n    ron safe: no\n    model risk: 2.74%\n    evidence: 104 / 3791"
    );
}

#[test]
fn summary_distinguishes_a_hard_safe_candidate_from_an_exact_zero_ron_risk() {
    let summary = format_forced_fold_summary(&reach_diagnostic(vec![
        genbutsu("E", 1),
        exact("N", 2, 0),
        exact("1m", 3, 104),
    ]));

    // hard-safe はルール上の根拠、exact `R == 0` は hidden-hand model 上の integer evidence。
    assert!(
        summary.contains("  rank 1: E\n    ron safe: yes\n    reason: Genbutsu"),
        "{summary}"
    );
    assert!(
        summary.contains(
            "  rank 2: N\n    ron safe: no\n    model risk: 0.00%\n    evidence: 0 / 3791"
        ),
        "{summary}"
    );
}

#[test]
fn summary_keeps_every_exact_zero_risk_candidate_beyond_the_top_three() {
    let summary = format_forced_fold_summary(&reach_diagnostic(vec![
        genbutsu("E", 1),
        exact("N", 2, 12),
        exact("1m", 3, 104),
        exact("9p", 4, 210),
        exact("7s", 5, 0),
    ]));

    // 0-risk candidate は上位3件の外でも production rank のまま表示する。
    assert_eq!(
        ranks(&summary),
        vec!["rank 1: E", "rank 2: N", "rank 3: 1m", "rank 5: 7s"]
    );
}

#[test]
fn summary_keeps_every_hard_safe_candidate_beyond_the_top_three() {
    let summary = format_forced_fold_summary(&reach_diagnostic(vec![
        genbutsu("E", 1),
        genbutsu("S", 2),
        genbutsu("W", 3),
        genbutsu("N", 4),
        genbutsu("P", 5),
    ]));

    assert_eq!(
        ranks(&summary),
        vec![
            "rank 1: E",
            "rank 2: S",
            "rank 3: W",
            "rank 4: N",
            "rank 5: P"
        ]
    );
}

#[test]
fn summary_shows_only_the_existing_candidates_when_fewer_than_three() {
    let summary = format_forced_fold_summary(&reach_diagnostic(vec![
        genbutsu("E", 1),
        exact("1m", 2, 104),
    ]));

    assert_eq!(ranks(&summary), vec!["rank 1: E", "rank 2: 1m"]);
}

#[test]
fn summary_stops_at_the_top_three_when_only_two_candidates_are_zero_risk() {
    let summary = format_forced_fold_summary(&reach_diagnostic(vec![
        genbutsu("E", 1),
        exact("N", 2, 0),
        exact("1m", 3, 104),
        exact("9p", 4, 210),
        exact("7s", 5, 320),
    ]));

    assert_eq!(
        ranks(&summary),
        vec!["rank 1: E", "rank 2: N", "rank 3: 1m"]
    );
}

#[test]
fn summary_does_not_treat_a_rounded_zero_percent_as_zero_risk() {
    let summary = format_forced_fold_summary(&reach_diagnostic(vec![
        genbutsu("E", 1),
        exact("N", 2, 12),
        exact("1m", 3, 104),
        exact("9p", 4, 210),
        ForcedFoldRankedCandidate {
            action: dahai("7s"),
            rank: 5,
            defense_kind: ForcedFoldDefenseKind::Reach(DefenseFallbackKind::ExactRonRisk),
            player_ron_risk_evidence: Some(vec![evidence(1, 1, 50_000)]),
        },
    ]));

    // 表示上 0.00% でも R > 0 なので 0-risk として追加表示しない。
    assert_eq!(
        ranks(&summary),
        vec!["rank 1: E", "rank 2: N", "rank 3: 1m"]
    );
    assert!(!summary.contains("7s"), "{summary}");
}

#[test]
fn summary_does_not_treat_a_three_visible_honor_as_zero_risk() {
    let visible_honor = heuristic(
        "N",
        4,
        DefenseFallbackKind::HonorSafety(HonorSafetyRank::ThreeOrMoreVisible),
    );
    let summary = format_forced_fold_summary(&reach_diagnostic(vec![
        heuristic("E", 1, DefenseFallbackKind::Genbutsu),
        heuristic(
            "9m",
            2,
            DefenseFallbackKind::SuitedSafety(SuitedSafetyRank::Suji),
        ),
        heuristic(
            "S",
            3,
            DefenseFallbackKind::HonorSafety(HonorSafetyRank::OneVisible),
        ),
        visible_honor,
    ]));

    // ThreeOrMoreVisible は heuristic safety で、exact model がない限り安全確定とみなさない。
    assert_eq!(
        ranks(&summary),
        vec!["rank 1: E", "rank 2: 9m", "rank 3: S"]
    );
}

#[test]
fn summary_reports_an_unavailable_exact_model_without_a_percentage() {
    let summary = format_forced_fold_summary(&reach_diagnostic(vec![
        heuristic(
            "9m",
            1,
            DefenseFallbackKind::SuitedSafety(SuitedSafetyRank::Suji),
        ),
        heuristic(
            "N",
            2,
            DefenseFallbackKind::HonorSafety(HonorSafetyRank::ThreeOrMoreVisible),
        ),
    ]));

    assert!(
        summary.contains(
            "  rank 2: N\n    ron safe: no\n    model risk: unavailable\n    heuristic: HonorSafety(ThreeOrMoreVisible)"
        ),
        "{summary}"
    );
    assert!(!summary.contains('%'), "{summary}");
}

#[test]
fn summary_lists_every_target_risk_worst_first_for_multiple_targets() {
    let summary = format_forced_fold_summary(&diagnostic(
        ForcedFoldDefenseKind::Combined(CombinedDefenseCategory::ExactRonRisk),
        vec![
            ForcedFoldRankedCandidate {
                action: dahai("9s"),
                rank: 1,
                defense_kind: ForcedFoldDefenseKind::Combined(
                    CombinedDefenseCategory::ExactRonRisk,
                ),
                player_ron_risk_evidence: Some(vec![
                    evidence(1, 123, 1_498),
                    evidence(3, 41, 1_750),
                ]),
            },
            ForcedFoldRankedCandidate {
                action: dahai("1m"),
                rank: 2,
                defense_kind: ForcedFoldDefenseKind::Combined(
                    CombinedDefenseCategory::ExactRonRisk,
                ),
                player_ron_risk_evidence: Some(vec![
                    evidence(1, 0, 1_498),
                    evidence(3, 200, 1_750),
                ]),
            },
        ],
    ));

    // player ごとの evidence を production comparator と同じ worst-first で並べる。
    assert!(
        summary.contains(
            "  rank 1: 9s\n    ron safe: no\n    model risk:\n      player 1: 8.21% (123 / 1498)\n      player 3: 2.34% (41 / 1750)"
        ),
        "{summary}"
    );
    // 一部 target だけ R == 0 の候補は 0-risk ではない。
    assert!(
        summary.contains("  rank 2: 1m\n    ron safe: no\n    model risk:\n"),
        "{summary}"
    );
}

#[test]
fn summary_reports_the_unavailable_reason_without_candidates() {
    assert_eq!(
        format_forced_fold_summary(&Err(ForcedFoldUnavailable::NoClearThreat)),
        "Summary\n  mode: ForcedFold\n  forced fold unavailable: no clear threat"
    );
    assert_eq!(
        format_forced_fold_summary(&Err(ForcedFoldUnavailable::NoDefenseSelection)),
        "Summary\n  mode: ForcedFold\n  forced fold unavailable: no defense discard"
    );
}

#[test]
fn summary_names_the_defense_family_of_each_routing() {
    let open_hand = format_forced_fold_summary(&diagnostic(
        ForcedFoldDefenseKind::OpenHand(OpenHandDefenseCategory::SafeAgainstAllTargets),
        vec![ForcedFoldRankedCandidate {
            action: dahai("5m"),
            rank: 1,
            defense_kind: ForcedFoldDefenseKind::OpenHand(
                OpenHandDefenseCategory::SafeAgainstAllTargets,
            ),
            player_ron_risk_evidence: None,
        }],
    ));
    assert!(
        open_hand.contains("  source: OpenHandDefenseFallback"),
        "{open_hand}"
    );
    assert!(
        open_hand.contains("  rank 1: 5m\n    ron safe: yes\n    reason: SafeAgainstAllTargets"),
        "{open_hand}"
    );

    let combined = format_forced_fold_summary(&diagnostic(
        ForcedFoldDefenseKind::Combined(CombinedDefenseCategory::SafeAgainstAllThreats),
        vec![ForcedFoldRankedCandidate {
            action: dahai("5m"),
            rank: 1,
            defense_kind: ForcedFoldDefenseKind::Combined(
                CombinedDefenseCategory::SafeAgainstAllThreats,
            ),
            player_ron_risk_evidence: None,
        }],
    ));
    assert!(
        combined.contains("  source: CombinedThreatDefenseFallback"),
        "{combined}"
    );
    assert!(
        combined.contains("  rank 1: 5m\n    ron safe: yes\n    reason: SafeAgainstAllThreats"),
        "{combined}"
    );
}
