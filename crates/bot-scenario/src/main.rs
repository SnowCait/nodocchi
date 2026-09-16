mod benchmark;
mod cli;
#[cfg(test)]
mod combined_defense;
mod error;
mod forced_fold;
mod format;
mod iishanten_continuation_depth;
mod iishanten_selection_depth;
mod iishanten_selection_parallel;
mod input;
#[cfg(test)]
mod open_hand_defense;
#[cfg(test)]
mod open_hand_threat;
mod replay;
mod scenario;
mod three_shanten_continuation;
mod tiles;
#[cfg(test)]
mod two_shanten_early_fold;
mod two_shanten_full_parallel;
#[cfg(test)]
mod two_shanten_full_parallel_regression;

use std::process::ExitCode;

use bot_core::{
    DiagnosticOptions, ShantenAgent, evaluate_forced_fold, measure_two_shanten_progress_self_tsumo,
    measure_two_shanten_self_tsumo,
};

use crate::benchmark::run_capture_benchmark;
use crate::cli::{CliArgs, ScenarioSource, USAGE};
use crate::error::ScenarioError;
use crate::forced_fold::{format_forced_fold, format_forced_fold_summary};
use crate::format::{
    format_diagnostic, format_summary, format_two_shanten_progress_self_tsumo_cost,
    format_two_shanten_self_tsumo_cost,
};
use crate::replay::load_captured_scenario;
use crate::scenario::{Scenario, ScenarioSpec};

fn main() -> ExitCode {
    match run(std::env::args().skip(1)) {
        Ok(output) => {
            println!("{output}");
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("error: {error}");
            if error.is_usage_error() {
                eprintln!("{USAGE}");
            }
            ExitCode::FAILURE
        }
    }
}

fn run<I>(args: I) -> Result<String, ScenarioError>
where
    I: IntoIterator<Item = String>,
{
    let args = CliArgs::parse(args)?;
    let (header, scenario) = match &args.source {
        ScenarioSource::Json(path) => (None, Scenario::resolve(&read_spec(path)?)?),
        ScenarioSource::Inline(spec) => (None, Scenario::resolve(spec)?),
        ScenarioSource::RiichilabCapture { path, request_id } => {
            let captured = load_captured_scenario(path, *request_id)?;
            (Some(captured.header()), captured.scenario)
        }
        ScenarioSource::RiichilabCaptureBenchmark(spec) => return run_capture_benchmark(spec),
        ScenarioSource::RiichilabCaptureComparison(spec) => {
            return three_shanten_continuation::run_capture_comparison(spec);
        }
    };

    // ベタ降り仮定の評価は通常打牌選択・押し引き・リーチ判断を一切走らせず、既存 Fold
    // defense をそのまま実行する。production の判断は変わらない。
    //
    // --summary-only でも同じ評価を行い、Summary の内容と ranked candidates も通常出力と同じ。
    // 違いは Summary 以外の詳細 section を省くことだけで、候補の risk evaluation は省略しない。
    if args.force_fold {
        let result = evaluate_forced_fold(&scenario.context, &scenario.legal_actions);
        let output = if args.summary_only {
            format_forced_fold_summary(&result)
        } else {
            format_forced_fold(&scenario, &result, args.verbose)
        };
        return Ok(match header {
            Some(header) => format!("{header}\n\n{output}"),
            None => output,
        });
    }

    // 2向聴 Full pair の分け方の計測も同じく他の診断を走らせず、cold memo 条件で計る。
    if args.two_shanten_full_parallel_comparison {
        let output = two_shanten_full_parallel::format_scenario_comparison(&scenario);
        return Ok(match header {
            Some(header) => format!("{header}\n\n{output}"),
            None => output,
        });
    }

    // 候補並列の計測も同じく他の診断を走らせず、cold memo 条件で計る。
    if args.iishanten_selection_parallel_comparison {
        let output = iishanten_selection_parallel::format_scenario_comparison(&scenario);
        return Ok(match header {
            Some(header) => format!("{header}\n\n{output}"),
            None => output,
        });
    }

    // production comparator を通した深度 A/B も同じく他の診断を走らせず、cold memo 条件で計る。
    if args.iishanten_selection_depth_comparison {
        let output = iishanten_selection_depth::format_scenario_comparison(&scenario);
        return Ok(match header {
            Some(header) => format!("{header}\n\n{output}"),
            None => output,
        });
    }

    // 1向聴 continuation の深度 A/B も同じく他の診断を走らせず、cold memo 条件で計る。
    if args.iishanten_continuation_depth_comparison {
        let output = iishanten_continuation_depth::format_scenario_comparison(&scenario);
        return Ok(match header {
            Some(header) => format!("{header}\n\n{output}"),
            None => output,
        });
    }

    // A/B 比較は他の診断を一切走らせず、どちらの方式も同じ cold memo 条件で計る。
    if args.three_shanten_continuation_comparison {
        let output = three_shanten_continuation::format_scenario_comparison(&scenario);
        return Ok(match header {
            Some(header) => format!("{header}\n\n{output}"),
            None => output,
        });
    }

    // cost 計測は production selection が同じ2向聴探索を走らせる前に取り、baseline と同じ
    // cold memo 条件を保つ。表示順は従来どおり診断の後にする。
    let three_shanten_cost = args.three_shanten_progress_self_tsumo.then(|| {
        bot_core::measure_three_shanten_progress_self_tsumo(
            &scenario.context,
            &scenario.legal_actions,
        )
    });
    let two_shanten_self_tsumo_cost = args.two_shanten_self_tsumo_cost.map(|scope| {
        (
            scope,
            measure_two_shanten_self_tsumo(&scenario.context, &scenario.legal_actions, scope),
        )
    });
    let two_shanten_progress_self_tsumo_cost =
        args.two_shanten_progress_self_tsumo_cost.map(|scope| {
            (
                scope,
                measure_two_shanten_progress_self_tsumo(
                    &scenario.context,
                    &scenario.legal_actions,
                    scope,
                ),
            )
        });
    let diagnostic = ShantenAgent::diagnose_with_options(
        &scenario.context,
        &scenario.legal_actions,
        diagnostic_options(&args),
    );

    let output = if args.summary_only {
        format_summary(&scenario, &diagnostic)
    } else {
        format_diagnostic(&scenario, &diagnostic, args.verbose)
    };
    // 計測は他の診断とは別の section として後ろに足すだけで、その手前の判断も表示も変えない。
    let output = match two_shanten_self_tsumo_cost {
        Some((scope, cost)) => {
            let section = format_two_shanten_self_tsumo_cost(
                scope,
                &cost,
                diagnostic.normal_discard_self_tsumo_facts,
            );
            format!("{output}\n\n{section}")
        }
        None => output,
    };
    let output = match two_shanten_progress_self_tsumo_cost {
        Some((scope, cost)) => {
            let section = format_two_shanten_progress_self_tsumo_cost(
                scope,
                &cost,
                diagnostic.normal_discard_self_tsumo_facts,
            );
            format!("{output}\n\n{section}")
        }
        None => output,
    };
    let output = match three_shanten_cost {
        Some(cost) => format!(
            "{output}\n\n{}",
            format::format_three_shanten_progress_self_tsumo_cost(
                &cost,
                diagnostic.normal_discard_self_tsumo_facts,
            )
        ),
        None => output,
    };
    Ok(match header {
        Some(header) => format!("{header}\n\n{output}"),
        None => output,
    })
}

// CLI option から構築する診断の範囲。追加の深い探索は互いに独立で、要求されたものだけを
// 構築する。same-shanten の枝をテンパイまで追う探索は枝の詳細を出す --verbose と組み合わせた
// 場合だけ、2向聴候補の ExpectedSelfTsumoValue は --two-shanten-self-tsumo を指定した場合だけに
// なる。診断の範囲は選択結果を変えない。
fn diagnostic_options(args: &CliArgs) -> DiagnosticOptions {
    DiagnosticOptions {
        lookahead: args.lookahead,
        same_shanten_downstream: args.lookahead && args.verbose,
        two_shanten_self_tsumo: args.two_shanten_self_tsumo,
    }
}

fn read_spec(path: &str) -> Result<ScenarioSpec, ScenarioError> {
    let text = std::fs::read_to_string(path).map_err(|error| ScenarioError::ReadFile {
        path: path.to_string(),
        message: error.to_string(),
    })?;

    serde_json::from_str(&text).map_err(|error| ScenarioError::Json {
        path: path.to_string(),
        message: error.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run_args(args: &[&str]) -> Result<String, ScenarioError> {
        run(args.iter().map(|arg| arg.to_string()))
    }

    fn options_of(args: &[&str]) -> DiagnosticOptions {
        let parsed = CliArgs::parse(args.iter().map(|arg| arg.to_string())).unwrap();
        diagnostic_options(&parsed)
    }

    #[test]
    fn the_diagnostic_scope_options_are_independent() {
        // 追加の深い探索は互いに含まない。--two-shanten-self-tsumo 単独では
        // same-shanten downstream を構築しない。
        assert_eq!(options_of(&["--hand", "123m"]), DiagnosticOptions::NONE);
        assert_eq!(
            options_of(&["--hand", "123m", "--lookahead"]),
            DiagnosticOptions::WITH_LOOKAHEAD
        );
        assert_eq!(
            options_of(&["--hand", "123m", "--lookahead", "--verbose"]),
            DiagnosticOptions::WITH_SAME_SHANTEN_DOWNSTREAM
        );
        assert_eq!(
            options_of(&["--hand", "123m", "--two-shanten-self-tsumo"]),
            DiagnosticOptions::WITH_TWO_SHANTEN_SELF_TSUMO
        );
        assert_eq!(
            options_of(&["--hand", "123m", "--two-shanten-self-tsumo", "--verbose"]),
            DiagnosticOptions::WITH_SAME_SHANTEN_DOWNSTREAM_AND_TWO_SHANTEN_SELF_TSUMO
        );
    }

    #[test]
    fn runs_a_simple_cli_scenario() {
        let output = run_args(&["--hand", "234m455p789s1123z", "--draw", "N"]).unwrap();
        assert!(output.starts_with("Scenario\n"), "{output}");
        assert!(output.contains("\n\nFinal decision\n"), "{output}");
        assert!(output.contains("\n\nNormal discard candidates"), "{output}");
    }

    #[test]
    fn the_three_shanten_continuation_comparison_is_a_separate_report() {
        // 比較専用の出力で、production の打牌診断は一切出さない。
        let args = ["--hand", "3479m478p237s1223z", "--dora-indicator", "1p"];
        let normal = run_args(&args).unwrap();
        assert!(!normal.contains("Three-shanten continuation scope comparison"));

        let mut compared = args.to_vec();
        compared.push("--three-shanten-continuation-comparison");
        let output = run_args(&compared).unwrap();
        assert!(output.starts_with("Three-shanten continuation scope comparison"));
        assert!(
            output.contains("A progress+same-shanten values"),
            "{output}"
        );
        assert!(output.contains("B progress-only values"), "{output}");
        assert!(output.contains("Search size A -> B"), "{output}");
        assert!(!output.contains("Final decision"), "{output}");

        // production の打牌は比較の B と同じ。
        let selected = normal
            .split("\nFinal decision\n  action: ")
            .nth(1)
            .and_then(|rest| rest.lines().next())
            .expect("最終 action がある");
        assert!(
            output.contains(&format!("  B progress-only: {selected} (discard selection")),
            "{selected}: {output}"
        );
    }

    #[test]
    fn three_shanten_progress_drives_the_production_discard_and_stays_opt_in() {
        let args = [
            "--hand",
            "45m46899p1124579s",
            "--dora-indicator",
            "E",
            "--round-wind",
            "E",
            "--seat-wind",
            "N",
            "--player-id",
            "0",
            "--oya",
            "1",
            "--remaining-tiles",
            "66",
        ];
        let normal = run_args(&args).unwrap();
        assert!(!normal.contains("Three-shanten progress"));
        // production の通常打牌は3向聴 Progress 軸で決まる。
        assert!(
            normal.contains("\nFinal decision\n  action: 2s\n"),
            "{normal}"
        );
        assert!(
            normal.contains("  choice 2 lost by: ThreeShantenProgressSelfTsumoValue"),
            "{normal}"
        );
        let mut enabled = args.to_vec();
        enabled.push("--three-shanten-progress-self-tsumo");
        let measured = run_args(&enabled).unwrap();
        assert!(measured.starts_with(&normal));
        assert!(measured.contains("evaluated candidates: 12"));
        let section = measured
            .split("Three-shanten progress self-tsumo value")
            .nth(1)
            .unwrap();
        // 診断が表示する値は production 打牌比較が使った値そのもの。
        for discard in ["2s", "4s"] {
            let line = section
                .lines()
                .find(|line| line.starts_with(&format!("  {discard}:")))
                .unwrap();
            assert!(!line.contains("unknown"), "{line}");
            let value = line
                .split_whitespace()
                .nth(1)
                .unwrap()
                .trim_end_matches(',');
            let (integer, fraction) = value.split_once('.').unwrap();
            assert!(
                normal.contains(&format!(
                    "  three-shanten progress self-tsumo value: {integer}.{}",
                    &fraction[..3]
                )),
                "{discard}: {value}"
            );
        }
        for option in [
            "--lookahead",
            "--verbose",
            "--summary-only",
            "--two-shanten-self-tsumo",
        ] {
            let mut conflicting = enabled.clone();
            conflicting.push(option);
            assert!(run_args(&conflicting).is_err());
        }
    }

    // 追加オプション無しの何切る CLI でも、打 W のテンパイからリーチが生成されて選ばれる。
    #[test]
    fn a_menzen_tenpai_cli_scenario_selects_reach_without_any_option() {
        let output = run_args(&[
            "--hand",
            "12388m56p234789s3z",
            "--dora-indicator",
            "7s",
            "--summary-only",
        ])
        .unwrap();

        assert!(
            output.starts_with(
                "Summary\n  choice 1: Reach\n  choice 1 discard: W\n  choice 1 source: Reach\n"
            ),
            "{output}"
        );
    }

    #[test]
    fn reported_reach_scenario_shows_three_production_choices() {
        let args = ["--hand", "34599m235p345567s"];
        let full = run_args(&args).unwrap();
        let summary = run_args(&[args.as_slice(), &["--summary-only"]].concat()).unwrap();

        assert!(
            full.contains("Final decision\n  action: Reach\n  discard: 2p\n  source: Reach"),
            "{full}"
        );
        assert!(
            summary.starts_with(
                "Summary\n  choice 1: Reach\n  choice 1 discard: 2p\n  choice 1 source: Reach"
            ),
            "{summary}"
        );
        assert!(
            summary.contains("  choice 2: 2p\n  choice 2 source: NormalDiscard"),
            "{summary}"
        );
        assert!(
            summary.contains("  choice 3: 5p\n  choice 3 source: NormalDiscard"),
            "{summary}"
        );
        assert!(
            summary.contains("  choice 3 lost by: CurrentTenpaiOffenseWeightedTotal"),
            "{summary}"
        );
        assert!(
            full.contains("  current tenpai offense weighted total: 20800"),
            "{full}"
        );
        assert!(
            full.contains("  current tenpai offense weighted total: 16000"),
            "{full}"
        );
        assert!(full.ends_with(&summary), "{full}");
    }

    #[test]
    fn request_407_safe_tenpai_discard_pushes_in_the_summary() {
        let path = format!(
            "{}/scenarios/request_407_safe_tenpai.json",
            env!("CARGO_MANIFEST_DIR")
        );
        let output = run([path, "--summary-only".to_string()]).unwrap();

        assert!(
            output.starts_with(
                "Summary\n  choice 1: Reach\n  choice 1 discard: 5m\n  choice 1 source: Reach"
            ),
            "{output}"
        );
        assert!(output.contains("  push/pull: Push"), "{output}");
        assert!(
            output.contains("  push/pull reason: SafeTenpaiAgainstHighOpenHand"),
            "{output}"
        );
        assert!(
            output.contains("  offense live wait: 5 remaining / 2 types"),
            "{output}"
        );
        assert!(output.contains("  offense furiten: no"), "{output}");
        assert!(
            output.contains("  offense value: Reach 2600 / total: 13000"),
            "{output}"
        );
        assert!(
            output.contains("  strong tenpai requirement: weighted total >= 15600"),
            "{output}"
        );
    }

    #[test]
    fn explicit_inline_baseline_facts_select_the_same_current_tenpai_value() {
        let args = [
            "--hand",
            "34599m235p345567s",
            "--player-id",
            "0",
            "--oya",
            "1",
            "--round-wind",
            "E",
            "--no-history-furiten",
        ];
        let full = run_args(&args).unwrap();
        let summary = run_args(&[args.as_slice(), &["--summary-only"]].concat()).unwrap();

        assert!(
            summary.starts_with(
                "Summary\n  choice 1: Reach\n  choice 1 discard: 2p\n  choice 1 source: Reach"
            ),
            "{summary}"
        );
        assert!(
            summary.contains("  choice 3: 5p\n  choice 3 source: NormalDiscard\n  choice 3 lost by: CurrentTenpaiOffenseWeightedTotal"),
            "{summary}"
        );
        assert!(
            full.contains("  current tenpai offense weighted total: 20800"),
            "{full}"
        );
        assert!(
            full.contains("  current tenpai offense weighted total: 16000"),
            "{full}"
        );
        assert!(full.ends_with(&summary), "{full}");
    }

    #[test]
    fn runs_a_simple_cli_scenario_with_red_five() {
        let output = run_args(&["--hand", "340m455p789s1123z", "--draw", "N"]).unwrap();
        assert!(output.contains("5mr"), "{output}");
    }

    #[test]
    #[ignore = "heavy CLI display E2E; run by the slow-tests workflow with --run-ignored=only"]
    fn dora_indicator_fills_the_scenario_dora_indicators() {
        let output = run_args(&[
            "--hand",
            "234m455p789s1123z",
            "--draw",
            "N",
            "--dora-indicator",
            "3p E",
        ])
        .unwrap();
        assert!(output.contains("  dora indicators: 3p E"), "{output}");

        let alias = run_args(&[
            "--hand",
            "234m455p789s1123z",
            "--draw",
            "N",
            "--dora",
            "3p E",
        ])
        .unwrap();
        assert_eq!(output, alias);
    }

    #[test]
    fn summary_only_prints_the_summary_section_only() {
        let hand = ["--hand", "234m455p789s1123z", "--draw", "N"];
        let full = run_args(&hand).unwrap();
        let summary = run_args(&[hand.as_slice(), &["--summary-only"]].concat()).unwrap();

        assert!(summary.starts_with("Summary\n"), "{summary}");
        for name in [
            "Scenario",
            "Table state",
            "History furiten",
            "Final decision",
            "Normal discard",
            "Push/Pull",
            "Reach",
            "Defense",
            "Player threats",
        ] {
            let header = format!("\n\n{name}\n");
            assert!(!summary.contains(&header), "{name} in {summary}");
            assert!(
                full.contains(&header) || full.starts_with(&format!("{name}\n")),
                "{name} missing from {full}"
            );
        }
        assert!(full.ends_with(&summary), "{full}");
    }

    #[test]
    fn summary_only_keeps_the_capture_header() {
        let observation = riichilab_client::observation::fixture_base64(
            0,
            Some(59),
            vec![0, 4, 8, 12, 17, 20, 53, 54, 96, 100, 120, 124, 125],
        );
        let line = riichilab_client::capture::record_line(
            riichilab_client::CaptureDirection::Server,
            &format!(
                r#"{{"type":"request_action","request_id":425,"possible_actions":[{{"type":"dahai","pai":"1m","tsumogiri":false}},{{"type":"dahai","pai":"6p","tsumogiri":true}}],"observation":"{observation}"}}"#
            ),
        )
        .unwrap();
        let path = std::env::temp_dir().join(format!(
            "bot-scenario-main-summary-only-capture-{}.jsonl",
            std::process::id()
        ));
        std::fs::write(&path, format!("{line}\n")).unwrap();

        let full = run_args(&["--riichilab-capture", path.to_str().unwrap()]).unwrap();
        let summary_only = run_args(&[
            "--riichilab-capture",
            path.to_str().unwrap(),
            "--summary-only",
        ])
        .unwrap();
        std::fs::remove_file(&path).unwrap();

        assert!(
            summary_only.starts_with("RiichiLab capture\n"),
            "{summary_only}"
        );
        assert!(summary_only.contains("  request_id: 425"), "{summary_only}");
        assert!(summary_only.contains("\n\nSummary\n"), "{summary_only}");
        assert!(!summary_only.contains("\n\nScenario\n"), "{summary_only}");
        assert!(
            !summary_only.contains("\n\nPlayer threats\n"),
            "{summary_only}"
        );

        let header = summary_only.split("\n\nSummary\n").next().unwrap();
        assert!(full.starts_with(header), "{full}");
        assert!(
            full.ends_with(summary_only.split_once("\n\n").unwrap().1),
            "{full}"
        );
    }

    #[test]
    fn reports_summary_only_conflicts_as_usage_errors() {
        for args in [
            ["--hand", "123m", "--summary-only", "--lookahead"],
            ["--hand", "123m", "--summary-only", "--verbose"],
        ] {
            let error = run_args(&args).unwrap_err();
            assert!(error.is_usage_error(), "{error:?}");
            assert!(
                error
                    .to_string()
                    .starts_with("--summary-only cannot be combined with"),
                "{error}"
            );
        }
    }

    #[test]
    fn reports_missing_hand_as_usage_error() {
        let error = run_args(&[]).unwrap_err();
        assert!(error.is_usage_error(), "{error:?}");
        assert_eq!(error.to_string(), "--hand is required");
    }

    #[test]
    fn reports_invalid_tiles() {
        let error = run_args(&["--hand", "123x"]).unwrap_err();
        assert!(!error.is_usage_error(), "{error:?}");
        assert!(error.to_string().contains("hand"), "{error}");
        assert!(error.to_string().contains("123x"), "{error}");
    }

    #[test]
    fn reports_missing_scenario_file() {
        let error = run_args(&["missing-scenario.json"]).unwrap_err();
        assert!(
            matches!(&error, ScenarioError::ReadFile { path, .. } if path == "missing-scenario.json"),
            "{error:?}"
        );
    }

    #[test]
    fn reports_invalid_scenario_json() {
        let path = std::env::temp_dir().join("bot-scenario-invalid-json.json");
        std::fs::write(&path, "{ \"hand\": ").unwrap();
        let error = run_args(&[path.to_str().unwrap()]).unwrap_err();
        std::fs::remove_file(&path).unwrap();
        assert!(matches!(&error, ScenarioError::Json { .. }), "{error:?}");
    }

    #[test]
    fn runs_a_json_scenario() {
        let path = std::env::temp_dir().join("bot-scenario-json-scenario.json");
        std::fs::write(
            &path,
            r#"{
                "hand": "234m455p789s1123z",
                "draw": "N",
                "dora_indicators": "3p",
                "round_wind": "E",
                "seat_wind": "N",
                "player_id": 0,
                "oya": 1,
                "reached": [false, true, false, false],
                "discards": ["", "1m 4m 7p E", "", ""]
            }"#,
        )
        .unwrap();
        let output = run_args(&[path.to_str().unwrap()]).unwrap();
        std::fs::remove_file(&path).unwrap();

        assert!(output.contains("  reached players: 1"), "{output}");
        assert!(output.contains("  discards[1]: 1m 4m 7p E"), "{output}");
        assert!(output.contains("\n\nPush/Pull\n"), "{output}");
        assert!(output.contains("\n\nDefense\n"), "{output}");
    }

    #[test]
    fn runs_a_captured_riichilab_request() {
        let observation = riichilab_client::observation::fixture_base64(
            0,
            Some(59),
            vec![0, 4, 8, 12, 17, 20, 53, 54, 96, 100, 120, 124, 125],
        );
        let line = riichilab_client::capture::record_line(
            riichilab_client::CaptureDirection::Server,
            &format!(
                r#"{{"type":"request_action","request_id":425,"possible_actions":[{{"type":"dahai","pai":"1m","tsumogiri":false}},{{"type":"dahai","pai":"6p","tsumogiri":true}}],"observation":"{observation}"}}"#
            ),
        )
        .unwrap();
        let path = std::env::temp_dir().join(format!(
            "bot-scenario-main-capture-{}.jsonl",
            std::process::id()
        ));
        std::fs::write(&path, format!("{line}\n")).unwrap();

        let output = run_args(&["--riichilab-capture", path.to_str().unwrap()]).unwrap();
        let selected = run_args(&[
            "--riichilab-capture",
            path.to_str().unwrap(),
            "--request-id",
            "425",
        ])
        .unwrap();
        std::fs::remove_file(&path).unwrap();

        assert_eq!(output, selected);
        assert!(output.starts_with("RiichiLab capture\n"), "{output}");
        assert!(output.contains("  request_id: 425"), "{output}");
        assert!(output.contains("\n\nScenario\n"), "{output}");
        assert!(output.contains("\n\nPush/Pull\n"), "{output}");
        assert!(output.contains("\n\nPlayer threats\n"), "{output}");
        assert!(output.contains("\n\nSummary\n"), "{output}");
    }

    fn write_benchmark_capture(name: &str, request_ids: &[u64]) -> String {
        let observation = riichilab_client::observation::fixture_base64(
            0,
            Some(128),
            vec![0, 12, 24, 36, 48, 60, 72, 84, 96, 108, 116, 124, 132],
        );
        let text = request_ids
            .iter()
            .map(|request_id| {
                riichilab_client::capture::record_line(
                    riichilab_client::CaptureDirection::Server,
                    &format!(
                        r#"{{"type":"request_action","request_id":{request_id},"possible_actions":[{{"type":"dahai","pai":"1m","tsumogiri":false}},{{"type":"dahai","pai":"F","tsumogiri":true}}],"observation":"{observation}"}}"#
                    ),
                )
                .unwrap()
            })
            .collect::<Vec<_>>()
            .join("\n");
        let path = std::env::temp_dir().join(format!(
            "bot-scenario-main-benchmark-{name}-{}.jsonl",
            std::process::id()
        ));
        std::fs::write(&path, format!("{text}\n")).unwrap();
        path.to_str().unwrap().to_string()
    }

    #[test]
    fn benchmarks_every_request_of_multiple_captures() {
        let first = write_benchmark_capture("first", &[425, 426]);
        let second = write_benchmark_capture("second", &[517]);

        let output = run_args(&[
            "--benchmark-riichilab-capture",
            first.as_str(),
            second.as_str(),
        ])
        .unwrap();
        let _ = std::fs::remove_file(&first);
        let _ = std::fs::remove_file(&second);

        assert!(
            output.starts_with("RiichiLab production latency benchmark\n"),
            "{output}"
        );
        assert!(output.contains("\n  captures: 2\n"), "{output}");
        assert!(output.contains("\n  requests: 3\n"), "{output}");
        assert!(output.contains("\n  p99: "), "{output}");
        assert!(output.contains("\n  > 3 s: "), "{output}");
        assert!(output.contains("\n\nSlowest requests\n"), "{output}");
        for request_id in [425, 426, 517] {
            assert!(
                output.contains(&format!("request_id={request_id}  early=")),
                "{output}"
            );
        }
        assert!(
            output.contains("  normal_discard=") && output.contains("  post_discard="),
            "{output}"
        );
        assert!(
            output.contains(" (base=")
                && output.contains(" forward=")
                && output.contains(" finalize="),
            "{output}"
        );
        assert!(output.contains("  selected="), "{output}");
        assert!(output.contains(&first), "{output}");
        assert!(output.contains(&second), "{output}");

        assert!(!output.contains("Push/Pull"), "{output}");
        assert!(!output.contains("Player threats"), "{output}");
    }

    #[test]
    fn benchmark_writes_the_machine_readable_output() {
        let capture = write_benchmark_capture("json", &[425, 426]);
        let json_path = std::env::temp_dir().join(format!(
            "bot-scenario-main-benchmark-json-{}.json",
            std::process::id()
        ));

        run_args(&[
            "--benchmark-riichilab-capture",
            capture.as_str(),
            "--benchmark-json",
            json_path.to_str().unwrap(),
        ])
        .unwrap();
        let text = std::fs::read_to_string(&json_path).unwrap();
        let _ = std::fs::remove_file(&capture);
        let _ = std::fs::remove_file(&json_path);

        let json: crate::benchmark::BenchmarkJson = serde_json::from_str(&text).unwrap();
        assert_eq!(json.summary.captures, 1);
        assert_eq!(json.summary.requests, 2);
        assert_eq!(
            json.requests
                .iter()
                .map(|request| request.request_id)
                .collect::<Vec<_>>(),
            vec![425, 426]
        );
        assert!(
            json.requests
                .iter()
                .all(|request| request.capture == capture && !request.selected.is_empty())
        );
    }

    #[test]
    fn reports_a_missing_benchmark_capture_file() {
        let error = run_args(&[
            "--benchmark-riichilab-capture",
            "missing-benchmark-capture.jsonl",
        ])
        .unwrap_err();

        assert!(
            matches!(&error, ScenarioError::ReadFile { path, .. } if path == "missing-benchmark-capture.jsonl"),
            "{error:?}"
        );
    }

    #[test]
    fn reports_a_missing_capture_file() {
        let error = run_args(&["--riichilab-capture", "missing-capture.jsonl"]).unwrap_err();
        assert!(
            matches!(&error, ScenarioError::ReadFile { path, .. } if path == "missing-capture.jsonl"),
            "{error:?}"
        );
    }

    #[test]
    fn lookahead_is_opt_in() {
        // 2手先は重い探索なので既定では計算せず表示もしない。小さい手牌で確認する。
        let default = run_args(&["--hand", "12m12p55s", "--draw", "9p"]).unwrap();
        assert!(!default.contains("Lookahead"), "{default}");

        let lookahead = run_args(&["--hand", "12m12p55s", "--draw", "9p", "--lookahead"]).unwrap();
        assert!(lookahead.contains("\n\nLookahead\n"), "{lookahead}");
        assert!(lookahead.contains("draws: "), "{lookahead}");
        assert!(!lookahead.contains("next discard:"), "{lookahead}");
    }

    #[test]
    fn tenpai_continuation_follows_the_lookahead_option() {
        // 現在聴牌のダマ継続は既存の --lookahead と同じ範囲でだけ出す。
        let hand = ["--hand", "123m456m789m123p1z", "--draw", "2z"];
        let default = run_args(&hand).unwrap();
        assert!(!default.contains("Tenpai continuation"), "{default}");

        let summary_only = run_args(&[hand.as_slice(), &["--summary-only"]].concat()).unwrap();
        assert!(
            !summary_only.contains("Tenpai continuation"),
            "{summary_only}"
        );
        assert!(!summary_only.contains("Lookahead"), "{summary_only}");
        // self-tsumo 比較のための点数計算も Summary だけの経路では行わない。
        assert!(
            !summary_only.contains("self-tsumo comparison"),
            "{summary_only}"
        );

        let lookahead = run_args(&[hand.as_slice(), &["--lookahead"]].concat()).unwrap();
        assert!(
            lookahead.contains("\n\nTenpai continuation\n"),
            "{lookahead}"
        );
        assert!(lookahead.contains("    current wait: "), "{lookahead}");
        assert!(
            lookahead.contains("    continuation branches: "),
            "{lookahead}"
        );
        assert!(
            lookahead.contains("    self-tsumo comparison"),
            "{lookahead}"
        );
        assert!(!lookahead.contains("      new wait: "), "{lookahead}");
    }

    #[test]
    #[ignore = "heavy diagnostics E2E; run by the slow-tests workflow with --run-ignored=only"]
    fn the_reach_damaten_comparison_stays_out_of_the_summary() {
        // 統合表示は detailed diagnostics の section で、Summary には足さない。
        let hand = ["--hand", "340678m789p34789s", "--remaining-tiles", "70"];
        let default = run_args(&hand).unwrap();
        assert!(
            default.contains("\n\nReach / Damaten comparison\n"),
            "{default}"
        );
        // 2手先探索を要求していない局面では self-tsumo の材料を作らない。
        assert!(default.contains("  self-tsumo: unavailable"), "{default}");
        assert!(default.contains("    reach baseline"), "{default}");

        let summary_only = run_args(&[hand.as_slice(), &["--summary-only"]].concat()).unwrap();
        assert!(
            !summary_only.contains("Reach / Damaten comparison"),
            "{summary_only}"
        );
        assert!(!summary_only.contains("reach baseline"), "{summary_only}");
        assert!(!summary_only.contains("damaten baseline"), "{summary_only}");

        // --lookahead を付けた場合だけ self-tsumo の比較まで並ぶ。
        let lookahead = run_args(&[hand.as_slice(), &["--lookahead"]].concat()).unwrap();
        assert!(
            lookahead.contains("  self-tsumo (expected tsumo payment)"),
            "{lookahead}"
        );
        assert!(lookahead.contains("    reach now: 1460.235"), "{lookahead}");
    }

    #[test]
    #[ignore = "heavy diagnostics E2E; run by the slow-tests workflow with --run-ignored=only"]
    fn the_inline_baseline_supplies_remaining_tiles_to_the_self_tsumo_comparison() {
        let output = run_args(&["--hand", "340678m789p34789s", "--lookahead"]).unwrap();

        assert!(output.contains("      defer one draw"), "{output}");
        for label in [
            "      reach now: ",
            "        production policy: ",
            "        forced Reach: ",
            "        forced Damaten: ",
            "        immediate Damaten tsumo: ",
        ] {
            assert!(output.contains(label), "{label}\n{output}");
            assert!(
                !output.contains(&format!("{label}unknown")),
                "{label}\n{output}"
            );
        }
    }

    #[test]
    fn explicit_north_seat_wind_enables_two_shanten_self_tsumo_selection() {
        let output = run_args(&[
            "--hand",
            "11258m234789p13s",
            "--draw",
            "9s",
            "--seat-wind",
            "N",
            "--summary-only",
        ])
        .unwrap();

        assert!(
            output.starts_with("Summary\n  choice 1: 8m\n  choice 1 source: NormalDiscard"),
            "{output}"
        );
        assert!(
            output.contains("choice 2 lost by: TwoShantenProgressSelfTsumoValue"),
            "{output}"
        );
    }

    #[test]
    fn verbose_lookahead_adds_each_draw() {
        let summary = run_args(&["--hand", "12m12p55s", "--draw", "9p", "--lookahead"]).unwrap();
        let verbose = run_args(&[
            "--hand",
            "12m12p55s",
            "--draw",
            "9p",
            "--lookahead",
            "--verbose",
        ])
        .unwrap();

        assert!(verbose.len() > summary.len());
        assert!(verbose.contains("      next discard: "), "{verbose}");
    }

    #[test]
    fn verbose_output_is_longer() {
        let default = run_args(&["--hand", "234m455p789s1123z", "--draw", "N"]).unwrap();
        let verbose =
            run_args(&["--hand", "234m455p789s1123z", "--draw", "N", "--verbose"]).unwrap();
        assert!(verbose.len() > default.len());
    }

    fn scenario_path(name: &str) -> String {
        format!("{}/scenarios/{name}.json", env!("CARGO_MANIFEST_DIR"))
    }

    fn run_scenario(name: &str, args: &[&str]) -> String {
        let mut all = vec![scenario_path(name)];
        all.extend(args.iter().map(|arg| arg.to_string()));
        run(all).unwrap()
    }

    #[test]
    fn force_fold_uses_the_relative_seat_river_and_reach_for_the_defense() {
        let output = run_args(&[
            "--hand",
            "234m455p789s1123z",
            "--draw",
            "N",
            "--discards-shimocha",
            "1m 7p 4s 7p E",
            "--riichi-shimocha",
            "4",
            "--remaining-tiles",
            "42",
            "--force-fold",
            "--summary-only",
        ])
        .unwrap();

        assert_eq!(
            output,
            concat!(
                "Summary\n  mode: ForcedFold\n  source: DefenseFallback\n",
                "\n  rank 1: E\n    ron safe: yes\n    reason: Genbutsu\n",
                "\n  rank 2: S\n    ron safe: no\n    model risk: 1.68%\n",
                "    evidence: 95487355974 / 5679785375284\n",
                "    copies: 1\n    fold risk: 1.68%\n",
                "\n  rank 3: W\n    ron safe: no\n    model risk: 1.68%\n",
                "    evidence: 95487355974 / 5679785375284\n",
                "    copies: 1\n    fold risk: 1.68%",
            )
        );
    }

    #[test]
    fn force_fold_ranks_a_duplicated_tile_by_its_fold_risk() {
        // 手牌に3枚ある 8p は、1枚切る model risk が 5m より高くても、ベタ降りで3巡ぶんしのげる
        // ぶんだけ fold risk が低いので上位になる。model risk 自体は補正しない。
        let output = run_args(&[
            "--hand",
            "3567m46888p12457s",
            "--draw",
            "",
            "--discards-shimocha",
            "2z 1p 2m 7z 3s 6s 5p 8m",
            "--riichi-shimocha",
            "5",
            "--remaining-tiles",
            "42",
            "--force-fold",
            "--summary-only",
        ])
        .unwrap();

        assert_eq!(
            output,
            concat!(
                "Summary\n  mode: ForcedFold\n  source: DefenseFallback\n",
                "\n  rank 1: 8p\n    ron safe: no\n    model risk: 2.62%\n",
                "    evidence: 64230213477 / 2452679059162\n",
                "    copies: 3\n    fold risk: 0.88%\n",
                "\n  rank 2: 5m\n    ron safe: no\n    model risk: 2.35%\n",
                "    evidence: 57655088883 / 2452679059162\n",
                "    copies: 1\n    fold risk: 2.35%\n",
                "\n  rank 3: 1s\n    ron safe: no\n    model risk: 2.81%\n",
                "    evidence: 68849401266 / 2452679059162\n",
                "    copies: 1\n    fold risk: 2.81%",
            )
        );
    }

    #[test]
    fn force_fold_reports_the_open_hand_and_combined_defense_family() {
        let open_hand = run_scenario("open_hand_defense", &["--force-fold", "--summary-only"]);
        assert!(
            open_hand.contains("  source: OpenHandDefenseFallback"),
            "{open_hand}"
        );
        assert!(
            open_hand
                .contains("  rank 1: 5m\n    ron safe: yes\n    reason: SafeAgainstAllTargets"),
            "{open_hand}"
        );

        let combined = run_scenario(
            "combined_threat_defense",
            &["--force-fold", "--summary-only"],
        );
        assert!(
            combined.contains("  source: CombinedThreatDefenseFallback"),
            "{combined}"
        );
        assert!(
            combined.contains("  rank 1: 5m\n    ron safe: yes\n    reason: SafeAgainstAllThreats"),
            "{combined}"
        );
    }

    #[test]
    fn force_fold_returns_a_defense_discard_where_the_normal_decision_pushes() {
        let normal = run_scenario("request_407_safe_tenpai", &["--summary-only"]);
        assert!(normal.contains("  push/pull: Push"), "{normal}");
        assert!(normal.contains("  choice 1 source: Reach"), "{normal}");

        let forced = run_scenario(
            "request_407_safe_tenpai",
            &["--force-fold", "--summary-only"],
        );
        assert_eq!(
            forced,
            concat!(
                "Summary\n  mode: ForcedFold\n  source: OpenHandDefenseFallback\n",
                "\n  rank 1: 5m\n    ron safe: yes\n    reason: SafeAgainstAllTargets\n",
                "\n  rank 2: 2m\n    ron safe: no\n    model risk: unavailable\n",
                "    heuristic: SuitedSafety(Suji)\n",
                "\n  rank 3: 6m\n    ron safe: no\n    model risk: unavailable\n",
                "    heuristic: SuitedSafety(HalfSuji)",
            )
        );

        // 押し引き・リーチ判断は forced fold の出力に出てこない。
        assert!(!forced.contains("push/pull"), "{forced}");
        assert!(!forced.contains("reach"), "{forced}");
    }

    #[test]
    fn force_fold_is_unavailable_without_a_clear_threat() {
        let output = run_args(&[
            "--hand",
            "234m455p789s1123z",
            "--draw",
            "N",
            "--force-fold",
            "--summary-only",
        ])
        .unwrap();

        assert_eq!(
            output,
            "Summary\n  mode: ForcedFold\n  forced fold unavailable: no clear threat"
        );
    }

    #[test]
    fn force_fold_shows_the_existing_defense_candidates_in_the_full_output() {
        let output = run_args(&[
            "--hand",
            "234m455p789s1123z",
            "--draw",
            "N",
            "--discards-shimocha",
            "1m 7p 4s 7p E",
            "--riichi-shimocha",
            "4",
            "--force-fold",
        ])
        .unwrap();

        assert!(output.starts_with("Scenario\n"), "{output}");
        assert!(
            output.contains("\n\nForced fold\n  mode: ForcedFold\n"),
            "{output}"
        );
        assert!(output.contains("\n\nDefense\n  evaluated\n"), "{output}");
        assert!(output.contains("\n\nDefense candidates\n"), "{output}");
        // 通常打牌の診断は構築しない。
        assert!(!output.contains("Normal discard"), "{output}");
        assert!(
            output.ends_with(concat!(
                "Summary\n  mode: ForcedFold\n  source: DefenseFallback\n",
                "\n  rank 1: E\n    ron safe: yes\n    reason: Genbutsu\n",
                "\n  rank 2: S\n    ron safe: no\n    model risk: 1.68%\n",
                "    evidence: 95487355974 / 5679785375284\n",
                "    copies: 1\n    fold risk: 1.68%\n",
                "\n  rank 3: W\n    ron safe: no\n    model risk: 1.68%\n",
                "    evidence: 95487355974 / 5679785375284\n",
                "    copies: 1\n    fold risk: 1.68%",
            )),
            "{output}"
        );
    }

    #[test]
    fn force_fold_shows_the_same_summary_with_and_without_summary_only() {
        // --summary-only は Summary 以外の詳細 section を省くだけで、Summary の内容と
        // ranked candidates は full output と同じ。候補の risk evaluation も省略しない。
        let reach = [
            "--hand",
            "234m455p789s1123z",
            "--draw",
            "N",
            "--discards-shimocha",
            "1m 7p 4s 7p E",
            "--riichi-shimocha",
            "4",
            "--force-fold",
        ];
        let mut summary_args = reach.to_vec();
        summary_args.push("--summary-only");
        let summary = run_args(&summary_args).unwrap();
        assert!(run_args(&reach).unwrap().ends_with(&summary), "{summary}");

        for name in [
            "defense",
            "defense_multi_riichi_double_wind",
            "open_hand_defense",
            "combined_threat_defense",
            "request_407_safe_tenpai",
        ] {
            let summary = run_scenario(name, &["--force-fold", "--summary-only"]);
            let full = run_scenario(name, &["--force-fold"]);
            assert!(full.ends_with(&summary), "{name}: {full}");

            // 詳細 section だけが summary-only で消える。
            assert!(summary.starts_with("Summary\n"), "{name}: {summary}");
            for section in [
                "\n\nScenario\n",
                "\n\nForced fold\n",
                "\n\nDefense\n",
                "\n\nDefense candidates\n",
                "\n\nOpenHand defense\n",
                "\n\nCombined defense\n",
            ] {
                assert!(!summary.contains(section), "{name}: {summary}");
            }
        }
    }

    #[test]
    fn force_fold_ranks_every_candidate_through_the_same_summary_shape() {
        for name in [
            "defense",
            "defense_multi_riichi_double_wind",
            "open_hand_defense",
            "combined_threat_defense",
            "request_407_safe_tenpai",
        ] {
            let summary = run_scenario(name, &["--force-fold", "--summary-only"]);
            let ranks: Vec<_> = summary
                .lines()
                .filter(|line| line.starts_with("  rank "))
                .collect();
            assert!(!ranks.is_empty(), "{name}: {summary}");

            // 全候補が同じ表示経路を通る。rank 1 だけの別表記を持たない。
            for rank in &ranks {
                let (_, action) = rank.split_once(": ").expect("rank 行に action がある");
                assert!(!action.is_empty(), "{name}: {summary}");
            }
            assert_eq!(
                summary
                    .lines()
                    .filter(|line| line.starts_with("    ron safe: "))
                    .count(),
                ranks.len(),
                "{name}: {summary}"
            );

            // rank 1 は詳細出力の selected action と一致する。
            let full = run_scenario(name, &["--force-fold"]);
            let selected = full
                .lines()
                .find_map(|line| line.strip_prefix("  selected action: "))
                .expect("Forced fold section が選択打牌を出す");
            assert_eq!(ranks[0], format!("  rank 1: {selected}"), "{name}: {full}");
        }
    }

    #[test]
    fn force_fold_does_not_change_the_production_decision() {
        let args = [
            "--hand",
            "234m455p789s1123z",
            "--draw",
            "N",
            "--discards-shimocha",
            "1m 7p 4s 7p E",
            "--riichi-shimocha",
            "4",
        ];

        let before = run_args(&args).unwrap();
        let mut forced_args = args.to_vec();
        forced_args.push("--force-fold");
        let _ = run_args(&forced_args).unwrap();
        let after = run_args(&args).unwrap();

        assert_eq!(before, after);
    }
}
