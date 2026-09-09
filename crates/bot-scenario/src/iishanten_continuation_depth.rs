//! 1向聴 ExpectedSelfTsumoValue の手変わり深度 A/B 比較の表示。
//!
//! A (same-shanten once) は production の continuation で `Progress` と
//! `SameShanten -> Progress` まで、B (same-shanten twice) は
//! `SameShanten -> SameShanten -> Progress` をもう1段だけ許した診断専用の追加深度。段数が違えば
//! 経路確率も違うため、どちらの深度の値かを必ず添えて表示する。
//!
//! 表示する順位は ExpectedSelfTsumoValue 単独の ranking で、production の最終打牌選択ではない。
//! production は既存 comparator の pre-acceptance 軸まで同順位の cohort の中だけでこの値を比べ、
//! その cohort に unknown が1件でもあれば軸ごと落とす。この module はその comparator を複製せず、
//! 値の高い順に並べるだけになっている。

use std::time::Duration;

use bot_core::{
    IishantenContinuationDepthComparison, IishantenContinuationDepthProfile,
    compare_iishanten_continuation_depths,
};
use bot_logic::{DrawTransition, SELF_TSUMO_VALUE_SCALE, ThreeShantenSearchStats, TileType};

use crate::scenario::Scenario;

/// 1局面の A/B 比較。値も探索規模も profile そのもので、表示のために評価し直さない。
pub fn format_scenario_comparison(scenario: &Scenario) -> String {
    let comparison =
        compare_iishanten_continuation_depths(&scenario.context, scenario.legal_actions.as_slice());

    let mut lines = vec![
        "Iishanten continuation depth comparison".to_string(),
        "  A same-shanten once: Progress, SameShanten -> Progress (production)".to_string(),
        "  B same-shanten twice: A plus SameShanten -> SameShanten -> Progress".to_string(),
        "  acceptance, comparator, terminal scoring, Reach / Damaten, probability and horizon \
         are shared"
            .to_string(),
        "  each depth is measured on its own fresh thread, so neither warms the other".to_string(),
        "  production discard selection uses A; B is a diagnostics-only experiment".to_string(),
        String::new(),
    ];
    lines.extend(format_profile(&comparison.once));
    lines.push(String::new());
    lines.extend(format_profile(&comparison.twice));
    lines.push(String::new());
    lines.extend(format_delta(&comparison));
    lines.join("\n")
}

fn format_profile(profile: &IishantenContinuationDepthProfile) -> Vec<String> {
    let mut lines = vec![
        format!("{} values", profile.depth.label()),
        format!("  evaluated candidates: {}", profile.candidates.len()),
        format!("  total elapsed: {}", format_duration(profile.total)),
    ];
    for (rank, (discard, value)) in profile.ranked().iter().enumerate() {
        let elapsed = profile
            .candidates
            .iter()
            .find(|(candidate, ..)| candidate == discard)
            .map(|&(_, _, elapsed)| elapsed)
            .unwrap_or_default();
        lines.push(format!(
            "  {}. {}: {}, elapsed: {}",
            rank + 1,
            discard.to_mjai_string(),
            format_value(*value),
            format_duration(elapsed),
        ));
    }
    lines
}

// 表示する数え上げ1件。label と、その値を取り出す関数の組。
type SearchCounter = (&'static str, fn(&ThreeShantenSearchStats) -> u64);

fn format_delta(comparison: &IishantenContinuationDepthComparison) -> Vec<String> {
    let search: [SearchCounter; 6] = [
        ("leaf draw states", |stats| stats.draw_variants),
        ("base evaluation calls", |stats| stats.base_evaluation_calls),
        ("base evaluation misses", |stats| {
            stats.base_evaluation_misses
        }),
        ("shanten / acceptance rebuilds", |stats| {
            stats.structural_evaluation_misses
        }),
        ("same-shanten enumerations", |stats| {
            stats.same_shanten_enumerations
        }),
        ("terminal scorings", |stats| stats.terminal_scorings),
    ];

    let mut lines = vec!["Value A -> B".to_string()];
    for (discard, once) in comparison.once.ranked() {
        let twice = comparison.twice.value(discard).flatten();
        lines.push(format!(
            "  {}: {} -> {} ({})",
            discard.to_mjai_string(),
            format_value(once),
            format_value(twice),
            format_added(once, twice),
        ));
    }
    lines.push(String::new());
    lines.push("Top ExpectedSelfTsumoValue candidate".to_string());
    lines.push(
        "  this axis alone, not the production discard selection: production compares it only \
         inside a cohort tied through the pre-acceptance axes and drops the axis when that cohort \
         holds an unknown"
            .to_string(),
    );
    for profile in [&comparison.once, &comparison.twice] {
        lines.push(format!(
            "  {}: {}",
            profile.depth.label(),
            format_top_candidate(profile.top_expected_self_tsumo_value_candidate()),
        ));
    }
    lines.push(format!(
        "  same top ExpectedSelfTsumoValue candidate: {}",
        comparison
            .shares_the_top_expected_self_tsumo_value_candidate()
            .map(|same| same.to_string())
            .unwrap_or_else(|| "unknown".to_string()),
    ));

    lines.push(String::new());
    lines.extend(format_first_draw_breakdown(comparison));

    lines.push(String::new());
    lines.push("Search size A -> B".to_string());
    lines.push(format!(
        "  total elapsed: {} -> {} ({})",
        format_duration(comparison.once.total),
        format_duration(comparison.twice.total),
        format_slowdown(comparison.once.total, comparison.twice.total),
    ));
    for (label, value) in search {
        let a = value(&comparison.once.search);
        let b = value(&comparison.twice.search);
        lines.push(format!("  {label}: {a} -> {b} ({})", format_ratio(a, b)));
    }
    lines
}

// 候補ごとの、最初のツモ1牌種単位の内訳。A / B のどちらも候補全体の値の内訳そのもので、表示の
// ために集計をやり直さない。
fn format_first_draw_breakdown(comparison: &IishantenContinuationDepthComparison) -> Vec<String> {
    let mut lines = vec!["First-draw contribution A -> B".to_string()];
    for breakdown in &comparison.once.breakdown {
        lines.push(format!("  {}", breakdown.discard.to_mjai_string()));
        let twice = comparison.twice.breakdown_of(breakdown.discard);
        for contribution in &breakdown.draws {
            let after = twice
                .and_then(|breakdown| breakdown.value(contribution.draw))
                .flatten();
            lines.push(format!(
                "    draw {}: {} remaining, {} {} -> {} ({})",
                contribution.draw.to_mjai_string(),
                contribution.remaining,
                transition_label(contribution.transition),
                format_value(contribution.value),
                format_value(after),
                format_added(contribution.value, after),
            ));
        }
    }
    lines
}

fn transition_label(transition: DrawTransition) -> &'static str {
    match transition {
        DrawTransition::Progress => "Progress",
        DrawTransition::SameShanten => "SameShanten",
    }
}

// ExpectedSelfTsumoValue ranking の1位。production が選ぶ打牌ではない。
fn format_top_candidate(top: Option<(TileType, u64)>) -> String {
    match top {
        Some((discard, value)) => {
            format!(
                "{} ({})",
                discard.to_mjai_string(),
                format_value(Some(value))
            )
        }
        None => "unknown".to_string(),
    }
}

// B が A へ足した分。どちらかが確定しない場合は差を作れない。
fn format_added(once: Option<u64>, twice: Option<u64>) -> String {
    let (Some(once), Some(twice)) = (once, twice) else {
        return "added unknown".to_string();
    };
    format!("added {}", format_value(Some(twice.saturating_sub(once))))
}

fn format_duration(duration: Duration) -> String {
    format!("{:.3} ms", duration.as_secs_f64() * 1_000.0)
}

// A / B の比。A が 0 の場合は比を作れないので出さない。
fn format_slowdown(once: Duration, twice: Duration) -> String {
    if once.is_zero() {
        return "slowdown n/a".to_string();
    }
    format!("slowdown {:.3}x", twice.as_secs_f64() / once.as_secs_f64())
}

fn format_ratio(once: u64, twice: u64) -> String {
    if once == 0 {
        return "n/a".to_string();
    }
    format!("{:.1}%", twice as f64 / once as f64 * 100.0)
}

fn format_value(scaled: Option<u64>) -> String {
    let Some(scaled) = scaled else {
        return "unknown".to_string();
    };
    format!(
        "{}.{:06}",
        scaled / SELF_TSUMO_VALUE_SCALE,
        scaled % SELF_TSUMO_VALUE_SCALE
    )
}
