//! `--force-fold` の出力。通常診断とは別の hypothetical evaluation なので、偽の
//! `ShantenDecisionDiagnostic` を組み立てず専用の formatter を持つ。

use bot_core::{ForcedFoldDefenseKind, ForcedFoldDiagnostic, ForcedFoldUnavailable};

use crate::format::{
    action_label, format_combined_defense, format_defense, format_defense_candidates,
    format_open_hand_defense, format_scenario, format_table_state,
};
use crate::scenario::Scenario;

const MODE: &str = "ForcedFold";

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

pub fn format_forced_fold_summary(result: &ForcedFoldResult) -> String {
    let mut lines = vec!["Summary".to_string(), format!("  mode: {MODE}")];
    match result {
        Ok(diagnostic) => {
            lines.push(format!(
                "  choice 1: {}",
                action_label(&diagnostic.selected_action)
            ));
            lines.push(format!(
                "  choice 1 source: {}",
                source_label(diagnostic.defense_kind)
            ));
            lines.push(defense_kind_line(diagnostic.defense_kind));
        }
        Err(unavailable) => lines.push(unavailable_line(*unavailable)),
    }
    lines.join("\n")
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

fn unavailable_line(unavailable: ForcedFoldUnavailable) -> String {
    let reason = match unavailable {
        ForcedFoldUnavailable::NoClearThreat => "no clear threat",
        ForcedFoldUnavailable::NoDefenseSelection => "no defense discard",
    };
    format!("  forced fold unavailable: {reason}")
}
