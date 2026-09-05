use super::*;
use crate::completed_hand_corpus;
use crate::winning_context::WinMethod;
use crate::yakuman::evaluate_yakuman;

// 変更前の合成順だけを残し、役満の成立規則は既存の評価関数を使う。
fn materialized<'a>(
    analysis: &'a CompletedHandAnalysis,
    context: WinningContext,
    interpretations: &[WinningTileInterpretation<'a>],
) -> Vec<WinningYakumanEvaluation<'a>> {
    let evaluations = evaluate_yakuman(analysis);
    interpretations
        .iter()
        .map(|&interpretation| {
            let mut yakuman = evaluations
                .iter()
                .find(|evaluation| evaluation.decomposition() == interpretation.decomposition())
                .map(|evaluation| evaluation.yakuman().to_vec())
                .unwrap_or_default();
            yakuman.extend(winning_tile_yakuman(
                analysis.fixed_melds(),
                context,
                &interpretation,
            ));
            yakuman.sort_unstable();
            yakuman.dedup();
            WinningYakumanEvaluation {
                interpretation,
                yakuman,
            }
        })
        .collect()
}

#[test]
fn streaming_matches_materialized_yakuman_including_interpretation_order() {
    let mut nonempty_multiple_interpretations = 0;
    for analysis in completed_hand_corpus::analyses() {
        for tile in TileType::all() {
            let mut interpretations = interpret_winning_tile(&analysis, tile);
            for context in completed_hand_corpus::winning_contexts() {
                let expected = materialized(&analysis, context, &interpretations);
                nonempty_multiple_interpretations += expected
                    .chunk_by(|left, right| left.decomposition() == right.decomposition())
                    .filter(|group| group.len() > 1 && group.iter().any(|entry| !entry.is_empty()))
                    .count();
                assert_eq!(evaluate_winning_yakuman(&analysis, context, tile), expected);
                assert_eq!(
                    winning_yakuman_evaluations(&analysis, context, &interpretations)
                        .collect::<Vec<_>>(),
                    expected,
                );
            }
            interpretations.reverse();
            let context = WinningContext::new(WinMethod::Ron);
            assert_eq!(
                winning_yakuman_evaluations(&analysis, context, &interpretations)
                    .collect::<Vec<_>>(),
                materialized(&analysis, context, &interpretations),
            );
        }
    }
    assert!(nonempty_multiple_interpretations > 0);
}
