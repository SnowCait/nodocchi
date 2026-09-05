use super::*;
use crate::completed_hand_corpus;
use crate::yaku::evaluate_yaku;

// 変更前の合成順: base を全件 materialize し、解釈ごとに検索・複製する。
// 役の成立規則は既存の評価関数を使い、ここには書き直さない。
fn materialized<'a>(
    analysis: &'a CompletedHandAnalysis,
    context: WinningContext,
    interpretations: &[WinningTileInterpretation<'a>],
) -> Vec<WinningYakuEvaluation<'a>> {
    let evaluations = evaluate_yaku(analysis, context);
    interpretations
        .iter()
        .map(|&interpretation| {
            let mut yaku = evaluations
                .iter()
                .find(|evaluation| evaluation.decomposition() == interpretation.decomposition())
                .map(|evaluation| evaluation.yaku().to_vec())
                .unwrap_or_default();
            winning_tile_yaku(analysis.fixed_melds(), context, &interpretation, &mut yaku);
            yaku.sort_unstable();
            yaku.dedup();
            WinningYakuEvaluation {
                interpretation,
                yaku,
            }
        })
        .collect()
}

#[test]
fn streaming_matches_materialized_yaku_including_interpretation_order() {
    let mut multiple_interpretations = 0;
    let mut multiple_decompositions = 0;
    for analysis in completed_hand_corpus::analyses() {
        multiple_decompositions += usize::from(analysis.decompositions().len() > 1);
        for tile in TileType::all() {
            let mut interpretations = interpret_winning_tile(&analysis, tile);
            multiple_interpretations += interpretations
                .chunk_by(|left, right| left.decomposition() == right.decomposition())
                .filter(|group| group.len() > 1)
                .count();
            for context in completed_hand_corpus::winning_contexts() {
                let expected = materialized(&analysis, context, &interpretations);
                assert_eq!(evaluate_winning_yaku(&analysis, context, tile), expected);
                assert_eq!(
                    winning_yaku_evaluations(&analysis, context, &interpretations)
                        .collect::<Vec<_>>(),
                    expected,
                );
            }
            // 呼び出し側の順序も保つ。最後に base を移しても、先行する解釈の役は変わらない。
            interpretations.reverse();
            let context = WinningContext::new(WinMethod::Ron);
            assert_eq!(
                winning_yaku_evaluations(&analysis, context, &interpretations).collect::<Vec<_>>(),
                materialized(&analysis, context, &interpretations),
            );
        }
    }
    assert!(multiple_interpretations > 0);
    assert!(multiple_decompositions > 0);
}
