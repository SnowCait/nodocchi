//! `--force-fold` の出力。通常診断とは別の hypothetical evaluation なので、偽の
//! `ShantenDecisionDiagnostic` を組み立てず専用の formatter を持つ。
//!
//! Summary の候補は `ForcedFoldDiagnostic::ranked_candidates` をそのまま表示する。順位付けは
//! 既存 production defense policy が source of truth で、ここで並べ替えや risk score を作らない。
//! `--summary-only` は Summary 以外の詳細 section を省くだけで、Summary の内容も候補評価も通常
//! 出力と同じ。

use bot_core::{
    ForcedFoldDefenseKind, ForcedFoldDiagnostic, ForcedFoldRankedCandidate, ForcedFoldUnavailable,
    PlayerRonRiskEvidence, RonRiskEvidence,
};

use crate::format::{
    action_label, format_combined_defense, format_defense, format_defense_candidates,
    format_open_hand_defense, format_scenario, format_table_state,
};
use crate::scenario::Scenario;

const MODE: &str = "ForcedFold";

// Summary に常に表示する production ordering 上位の件数。0-risk candidate はこれを超えても全件
// 表示する。
const SUMMARY_TOP_RANKS: usize = 3;

pub type ForcedFoldResult = Result<ForcedFoldDiagnostic, ForcedFoldUnavailable>;

pub fn format_forced_fold(scenario: &Scenario, result: &ForcedFoldResult, verbose: bool) -> String {
    let mut sections = vec![
        format_scenario(scenario, verbose),
        format_table_state(&scenario.context),
        format_forced_fold_section(result),
    ];

    if let Ok(diagnostic) = result {
        if let Some(defense) = diagnostic.defense.as_ref() {
            sections.push(format_defense(Some(defense)));
            if let Some(section) = format_defense_candidates(Some(defense)) {
                sections.push(section);
            }
        }
        if let Some(open_hand_defense) = diagnostic.open_hand_defense.as_ref() {
            sections.push(format_open_hand_defense(open_hand_defense));
        }
        if let Some(combined_defense) = diagnostic.combined_defense.as_ref() {
            sections.push(format_combined_defense(combined_defense));
        }
    }

    sections.push(format_forced_fold_summary(result));
    sections.join("\n\n")
}

/// forced fold の Summary。`--summary-only` でも通常出力でも同じ内容を返す。
///
/// 表示するのは production ordering の上位 [`SUMMARY_TOP_RANKS`] 件と、そこに含まれない 0-risk
/// candidate 全件。0-risk candidate を追加表示するために順位を付け替えない。
pub fn format_forced_fold_summary(result: &ForcedFoldResult) -> String {
    let mut lines = vec!["Summary".to_string(), format!("  mode: {MODE}")];
    match result {
        Ok(diagnostic) => {
            lines.push(format!(
                "  source: {}",
                source_label(diagnostic.defense_kind)
            ));
            for candidate in summary_candidates(&diagnostic.ranked_candidates) {
                lines.push(String::new());
                lines.extend(ranked_candidate_lines(candidate));
            }
        }
        Err(unavailable) => lines.push(unavailable_line(*unavailable)),
    }
    lines.join("\n")
}

// production ordering の上位 SUMMARY_TOP_RANKS 件と、上位に入らなかった 0-risk candidate 全件を
// production rank 順のまま返す。候補が SUMMARY_TOP_RANKS 件未満なら存在する候補だけ。
fn summary_candidates(
    ranked_candidates: &[ForcedFoldRankedCandidate],
) -> impl Iterator<Item = &ForcedFoldRankedCandidate> {
    ranked_candidates
        .iter()
        .enumerate()
        .filter(|(index, candidate)| *index < SUMMARY_TOP_RANKS || candidate.is_zero_risk())
        .map(|(_, candidate)| candidate)
}

// 全 ranked candidate 共通の表示。hard-safe はルール上の確定根拠、exact `R == 0` は hidden-hand
// model 上の integer evidence で、どちらも `ron safe` と根拠行で区別できるようにする。
fn ranked_candidate_lines(candidate: &ForcedFoldRankedCandidate) -> Vec<String> {
    let mut lines = vec![format!(
        "  rank {}: {}",
        candidate.rank,
        action_label(&candidate.action)
    )];

    let hard_safe = candidate.is_hard_safe();
    lines.push(format!(
        "    ron safe: {}",
        if hard_safe { "yes" } else { "no" }
    ));

    if hard_safe {
        lines.push(format!(
            "    reason: {}",
            defense_kind_label(candidate.defense_kind)
        ));
        return lines;
    }

    match candidate.worst_first_player_ron_risk_evidence().as_deref() {
        // 単独 target では canonical な1要素 vector をそのまま1行で出す。
        Some([evidence]) => {
            lines.push(format!(
                "    model risk: {}",
                model_risk_label(evidence.evidence)
            ));
            lines.push(format!(
                "    evidence: {} / {}",
                evidence.evidence.ron_capable_weight, evidence.evidence.tenpai_weight
            ));
        }
        Some(evidence) => {
            lines.push("    model risk:".to_string());
            lines.extend(evidence.iter().map(|evidence| player_risk_line(evidence)));
        }
        // exact model が使えない候補には存在しない percentage を作らず、順位を決めた既存
        // heuristic の根拠だけを出す。
        None => {
            lines.push("    model risk: unavailable".to_string());
            lines.push(format!(
                "    heuristic: {}",
                defense_kind_label(candidate.defense_kind)
            ));
        }
    }

    lines
}

fn player_risk_line(evidence: &PlayerRonRiskEvidence) -> String {
    format!(
        "      player {}: {} ({} / {})",
        evidence.player,
        model_risk_label(evidence.evidence),
        evidence.evidence.ron_capable_weight,
        evidence.evidence.tenpai_weight
    )
}

// exact evidence の `R/T` を表示用の百分率へ写す。selection / ranking / 0-risk 判定はどれも
// integer evidence だけを使い、この値を comparator に使わない。
//
// `R/T` は実際の放銃率ではなく、公開情報と整合する structural tenpai hidden-hand states のうち
// その牌で現在ロン可能な state の比率なので、表示も model risk と呼ぶ。
fn model_risk_label(evidence: RonRiskEvidence) -> String {
    let ratio = evidence.ron_capable_weight as f64 / evidence.tenpai_weight as f64;
    format!("{:.2}%", ratio * 100.0)
}

fn format_forced_fold_section(result: &ForcedFoldResult) -> String {
    let mut lines = vec!["Forced fold".to_string(), format!("  mode: {MODE}")];
    match result {
        Ok(diagnostic) => {
            lines.push(format!(
                "  selected action: {}",
                action_label(&diagnostic.selected_action)
            ));
            lines.push(format!(
                "  source: {}",
                source_label(diagnostic.defense_kind)
            ));
            lines.push(defense_kind_line(diagnostic.defense_kind));
            lines.push(format!(
                "  ranked candidates: {}",
                diagnostic.ranked_candidates.len()
            ));
        }
        Err(unavailable) => lines.push(unavailable_line(*unavailable)),
    }
    lines.join("\n")
}

// 防御 family の名前は既存 AgentActionSource の経路名と同じにする。
fn source_label(kind: ForcedFoldDefenseKind) -> &'static str {
    match kind {
        ForcedFoldDefenseKind::Reach(_) => "DefenseFallback",
        ForcedFoldDefenseKind::OpenHand(_) => "OpenHandDefenseFallback",
        ForcedFoldDefenseKind::Combined(_) => "CombinedThreatDefenseFallback",
    }
}

// family 内の選択種別は既存 defense diagnostics と同じ呼び方にする。リーチ者向けは kind、
// OpenHand / 複合 threat 向けは category。
fn defense_kind_line(kind: ForcedFoldDefenseKind) -> String {
    match kind {
        ForcedFoldDefenseKind::Reach(kind) => format!("  defense kind: {kind:?}"),
        ForcedFoldDefenseKind::OpenHand(category) => format!("  defense category: {category:?}"),
        ForcedFoldDefenseKind::Combined(category) => format!("  defense category: {category:?}"),
    }
}

// 候補の根拠は family 内の種別そのままで、表示側で安全度を作り直さない。
fn defense_kind_label(kind: ForcedFoldDefenseKind) -> String {
    match kind {
        ForcedFoldDefenseKind::Reach(kind) => format!("{kind:?}"),
        ForcedFoldDefenseKind::OpenHand(category) => format!("{category:?}"),
        ForcedFoldDefenseKind::Combined(category) => format!("{category:?}"),
    }
}

fn unavailable_line(unavailable: ForcedFoldUnavailable) -> String {
    let reason = match unavailable {
        ForcedFoldUnavailable::NoClearThreat => "no clear threat",
        ForcedFoldUnavailable::NoDefenseSelection => "no defense discard",
    };
    format!("  forced fold unavailable: {reason}")
}

#[cfg(test)]
mod tests;
