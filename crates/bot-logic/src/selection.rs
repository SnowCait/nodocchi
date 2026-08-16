//! 打牌選択用の前方集計値と、それを含む打牌比較。
//!
//! [`DiscardEvaluation`] は「その打牌をした直後の13枚」を1手だけ見る評価で、この module が持つ
//! のは「有効牌を引いた後の形」を比較する selection 専用の集計値。1向聴ではテンパイ待ち、
//! 2向聴以上では次打牌後の受け入れとして解釈する。責務が
//! 別なので前方集計値を [`DiscardEvaluation`] へ持ち込まず、`evaluate_discards_with_seen()` が
//! 再帰的に前方探索を始める構造も作らない。
//!
//! 集計値そのものは既存の2手先評価 ([`crate::lookahead`]) が返す next best
//! [`DiscardEvaluation`] から求める。テンパイ専用の待ち計算器は持たず、テンパイ形の
//! `acceptance_total_remaining()` / `acceptance_type_count()` をそのまま和了牌の残枚数・待ち牌
//! 種類数として使う。

use crate::discard::{
    DiscardComparison, DiscardComparisonReason, DiscardEvaluation,
    compare_discard_before_acceptance, compare_discard_from_acceptance,
};

/// 既存 weighted tenpai wait を適用する現在打牌後の向聴数。
pub(crate) const TENPAI_WAIT_TARGET_SHANTEN: i8 = 1;

/// 1手目の有効牌の残枚数で、その後の受け入れを重み付けした共通集計値。
///
/// - `weighted_remaining` = Σ(first draw remaining × next discard acceptance remaining)
/// - `weighted_type_count` = Σ(first draw remaining × next discard acceptance type count)
///
/// `next_discard.min_shanten_after_discard()` が呼び出し側の `required_next_shanten` と一致する
/// 枝だけを集計する。1向聴では `required_next_shanten = 0` なので next acceptance をテンパイ待ち
/// として解釈し、2向聴以上では1手進んだ後の次の有効牌として解釈する。
/// どちらも平均や確率へ正規化しない生の重み付き合計で、桁溢れを避けるため `u32` で保持する。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct WeightedForwardMetric {
    pub weighted_remaining: u32,
    pub weighted_type_count: u32,
}

impl WeightedForwardMetric {
    /// 受け入れ牌1枚分の枝を加算する。
    ///
    /// `next_discard` は既存評価が選んだ2手目の最良打牌。`None` の場合、または次打牌後の向聴数が
    /// `required_next_shanten` と一致しない場合、その枝の寄与は 0 にする。
    pub(crate) fn accumulate(
        &mut self,
        first_draw_remaining: u8,
        required_next_shanten: i8,
        next_discard: Option<&DiscardEvaluation>,
    ) {
        let Some(next) = next_discard else {
            return;
        };
        if next.min_shanten_after_discard() != required_next_shanten {
            return;
        }

        let weight = u32::from(first_draw_remaining);
        self.weighted_remaining += weight * u32::from(next.acceptance_total_remaining());
        self.weighted_type_count += weight * next.acceptance_type_count() as u32;
    }
}

/// 1向聴で next acceptance をテンパイ待ちとして解釈する alias。
pub type TenpaiWaitMetric = WeightedForwardMetric;
/// 2向聴以上で next acceptance を1手進んだ後の次の有効牌として解釈する alias。
pub type NextAcceptanceMetric = WeightedForwardMetric;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ForwardMetrics {
    pub tenpai_wait: Option<TenpaiWaitMetric>,
    pub next_acceptance: Option<NextAcceptanceMetric>,
}

/// 打牌選択1候補分の入力。1手評価と向聴数別の前方集計値を組にする。
///
/// `tenpai_wait` は前方評価を実際に計算した場合だけ `Some`。計算しなかった候補と
/// 「計算した結果すべての待ちが死んでいた」候補を区別するため、意味の無い 0 で埋めない。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DiscardSelectionCandidate<'a> {
    pub evaluation: &'a DiscardEvaluation,
    pub tenpai_wait: Option<TenpaiWaitMetric>,
    pub next_acceptance: Option<NextAcceptanceMetric>,
}

impl<'a> DiscardSelectionCandidate<'a> {
    /// 前方評価を持たない候補。既存の1手比較と同じ結果になる。
    pub fn without_tenpai_wait(evaluation: &'a DiscardEvaluation) -> Self {
        Self {
            evaluation,
            tenpai_wait: None,
            next_acceptance: None,
        }
    }
}

/// 前方集計値を含めて打牌候補を比較する。
///
/// 比較順は既存の1手比較へ1向聴限定の軸を差し込んだもので、既存の軸は再定義しない。
///
/// ```text
/// Shanten → IsolatedTile → IsolatedHonor
///   → [1向聴のみ] WeightedTenpaiWaitRemaining → WeightedTenpaiWaitTypeCount
///   → [2向聴以上] WeightedNextAcceptanceRemaining → WeightedNextAcceptanceTypeCount
///   → AcceptanceRemaining → AcceptanceTypeCount → IishantenShape → ...
/// ```
///
/// 各軸は両候補の向聴数が等しく、対応する前方集計値が両方にある場合だけ決着させる。
pub fn compare_discard_selection_candidates(
    candidate: &DiscardSelectionCandidate,
    current_best: &DiscardSelectionCandidate,
) -> DiscardComparison {
    if let Some(comparison) =
        compare_discard_before_acceptance(candidate.evaluation, current_best.evaluation)
    {
        return comparison;
    }

    if let Some(comparison) = compare_weighted_tenpai_wait(candidate, current_best) {
        return comparison;
    }

    if let Some(comparison) = compare_weighted_next_acceptance(candidate, current_best) {
        return comparison;
    }

    compare_discard_from_acceptance(candidate.evaluation, current_best.evaluation)
}

/// 前方集計値を含む比較順で最善候補の index を返す。完全同値では先に現れた候補を維持する。
///
/// `tenpai_wait` は `evaluations` と同じ順序で、範囲外の index は前方評価なし (`None`) として
/// 扱う。空スライスを渡すと既存の1手比較だけで選ぶ。
pub fn best_discard_selection_index(
    evaluations: &[DiscardEvaluation],
    tenpai_wait: &[Option<TenpaiWaitMetric>],
) -> Option<usize> {
    let metrics: Vec<_> = tenpai_wait
        .iter()
        .map(|&tenpai_wait| ForwardMetrics {
            tenpai_wait,
            next_acceptance: None,
        })
        .collect();
    best_discard_selection_index_with_forward_metrics(evaluations, &metrics)
}

pub fn best_discard_selection_index_with_forward_metrics(
    evaluations: &[DiscardEvaluation],
    forward_metrics: &[ForwardMetrics],
) -> Option<usize> {
    let metrics_at = |index: usize| forward_metrics.get(index).copied().unwrap_or_default();
    let candidate_at = |index: usize| DiscardSelectionCandidate {
        evaluation: &evaluations[index],
        tenpai_wait: metrics_at(index).tenpai_wait,
        next_acceptance: metrics_at(index).next_acceptance,
    };

    let mut best: Option<usize> = None;
    for index in 0..evaluations.len() {
        match best {
            Some(best_index)
                if !compare_discard_selection_candidates(
                    &candidate_at(index),
                    &candidate_at(best_index),
                )
                .candidate_is_better => {}
            _ => best = Some(index),
        }
    }
    best
}

// 1向聴限定の前方評価による比較。決着しなければ `None` を返して既存比較へ委ねる。
//
// 呼び出し時点で両候補の最小向聴数は等しいが、対象を1向聴に限定するため向聴数も明示的に確認
// する。前方集計値が片方しか無い状態では順位を付けず、比較の推移律を保つ。
fn compare_weighted_tenpai_wait(
    candidate: &DiscardSelectionCandidate,
    current_best: &DiscardSelectionCandidate,
) -> Option<DiscardComparison> {
    if candidate.evaluation.min_shanten_after_discard() != TENPAI_WAIT_TARGET_SHANTEN
        || current_best.evaluation.min_shanten_after_discard() != TENPAI_WAIT_TARGET_SHANTEN
    {
        return None;
    }

    let candidate_wait = candidate.tenpai_wait?;
    let best_wait = current_best.tenpai_wait?;

    if candidate_wait.weighted_remaining != best_wait.weighted_remaining {
        return Some(DiscardComparison {
            candidate_is_better: candidate_wait.weighted_remaining > best_wait.weighted_remaining,
            reason: DiscardComparisonReason::WeightedTenpaiWaitRemaining,
        });
    }

    if candidate_wait.weighted_type_count != best_wait.weighted_type_count {
        return Some(DiscardComparison {
            candidate_is_better: candidate_wait.weighted_type_count > best_wait.weighted_type_count,
            reason: DiscardComparisonReason::WeightedTenpaiWaitTypeCount,
        });
    }

    None
}

fn compare_weighted_next_acceptance(
    candidate: &DiscardSelectionCandidate,
    current_best: &DiscardSelectionCandidate,
) -> Option<DiscardComparison> {
    let shanten = candidate.evaluation.min_shanten_after_discard();
    if shanten < 2 || current_best.evaluation.min_shanten_after_discard() != shanten {
        return None;
    }

    let candidate_metric = candidate.next_acceptance?;
    let best_metric = current_best.next_acceptance?;
    if candidate_metric.weighted_remaining != best_metric.weighted_remaining {
        return Some(DiscardComparison {
            candidate_is_better: candidate_metric.weighted_remaining
                > best_metric.weighted_remaining,
            reason: DiscardComparisonReason::WeightedNextAcceptanceRemaining,
        });
    }
    if candidate_metric.weighted_type_count != best_metric.weighted_type_count {
        return Some(DiscardComparison {
            candidate_is_better: candidate_metric.weighted_type_count
                > best_metric.weighted_type_count,
            reason: DiscardComparisonReason::WeightedNextAcceptanceTypeCount,
        });
    }
    None
}

/// 打牌候補集合が前方評価の対象かどうか。
///
/// 全合法候補の1手評価から求めた最善向聴数が1向聴で、かつ1向聴を維持する候補が複数ある場合
/// だけ `true`。候補が1件だけなら Shanten 比較で決着するため前方評価は不要で、最善向聴数が
/// 1向聴でなければ今回の評価軸は使わない。
pub(crate) fn requires_forward_metrics(evaluations: &[DiscardEvaluation]) -> bool {
    forward_target_mask(evaluations)
        .into_iter()
        .filter(|&target| target)
        .count()
        > 1
}

#[cfg(test)]
fn requires_tenpai_wait(evaluations: &[DiscardEvaluation]) -> bool {
    requires_forward_metrics(evaluations)
        && evaluations
            .iter()
            .map(DiscardEvaluation::min_shanten_after_discard)
            .min()
            == Some(1)
}

/// この打牌候補が前方評価の対象かどうか。
pub(crate) fn forward_target_mask(evaluations: &[DiscardEvaluation]) -> Vec<bool> {
    let Some(mut best_index) = (!evaluations.is_empty()).then_some(0) else {
        return Vec::new();
    };
    for index in 1..evaluations.len() {
        if compare_discard_before_acceptance(&evaluations[index], &evaluations[best_index])
            .is_some_and(|comparison| comparison.candidate_is_better)
        {
            best_index = index;
        }
    }
    if evaluations[best_index].min_shanten_after_discard() < TENPAI_WAIT_TARGET_SHANTEN {
        return vec![false; evaluations.len()];
    }
    evaluations
        .iter()
        .map(|evaluation| {
            compare_discard_before_acceptance(evaluation, &evaluations[best_index]).is_none()
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::acceptance::{Acceptance, AcceptanceTile, EffectiveAcceptance};
    use crate::discard::compare_discard_evaluations;
    use crate::iishanten::IishantenShape;
    use crate::shanten::{EffectiveShanten, Shanten};
    use crate::tile::TileType;

    fn tile(s: &str) -> TileType {
        TileType::from_mjai_type_str(s).unwrap()
    }

    fn shanten(value: i8) -> EffectiveShanten {
        EffectiveShanten::Concealed(Shanten {
            standard: value,
            chiitoitsu: value + 2,
            kokushi: value + 4,
        })
    }

    // 受け入れ枚数・種類数だけを指定した検証用の受け入れ。
    fn acceptance(current: i8, tiles: &[(&str, u8)]) -> EffectiveAcceptance {
        Acceptance {
            current: shanten(current),
            tiles: tiles
                .iter()
                .map(|(name, remaining)| AcceptanceTile {
                    tile: tile(name),
                    remaining: *remaining,
                    shanten_after_draw: shanten(current - 1),
                })
                .collect(),
        }
    }

    fn evaluation(
        discard: &str,
        shanten_after_discard: i8,
        acceptance_tiles: &[(&str, u8)],
    ) -> DiscardEvaluation {
        DiscardEvaluation {
            discard: tile(discard),
            count_before_discard: 1,
            shanten_after_discard: shanten(shanten_after_discard),
            acceptance_after_discard: acceptance(shanten_after_discard, acceptance_tiles),
            shape_penalty: 0,
            floating_tile_value: 0,
            discarded_dora_count: 0,
            discarded_value_honor_count: 0,
            discards_red_five: false,
            discards_isolated_tile: false,
            standard_iishanten_shape_after_discard: IishantenShape::Unknown,
        }
    }

    fn metric(remaining: u32, type_count: u32) -> Option<TenpaiWaitMetric> {
        Some(TenpaiWaitMetric {
            weighted_remaining: remaining,
            weighted_type_count: type_count,
        })
    }

    fn candidate<'a>(
        evaluation: &'a DiscardEvaluation,
        tenpai_wait: Option<TenpaiWaitMetric>,
    ) -> DiscardSelectionCandidate<'a> {
        DiscardSelectionCandidate {
            evaluation,
            tenpai_wait,
            next_acceptance: None,
        }
    }

    fn next_candidate<'a>(
        evaluation: &'a DiscardEvaluation,
        next_acceptance: Option<NextAcceptanceMetric>,
    ) -> DiscardSelectionCandidate<'a> {
        DiscardSelectionCandidate {
            evaluation,
            tenpai_wait: None,
            next_acceptance,
        }
    }

    #[test]
    fn accumulates_the_weighted_wait_of_a_tenpai_branch() {
        let mut metric = TenpaiWaitMetric::default();
        let tenpai = evaluation("E", 0, &[("3m", 4), ("6m", 4)]);

        metric.accumulate(4, 0, Some(&tenpai));

        assert_eq!(metric.weighted_remaining, 4 * 8);
        assert_eq!(metric.weighted_type_count, 4 * 2);
    }

    #[test]
    fn skips_branches_without_a_next_discard() {
        let mut metric = TenpaiWaitMetric::default();
        metric.accumulate(4, 0, None);
        assert_eq!(metric, TenpaiWaitMetric::default());
    }

    #[test]
    fn skips_branches_that_do_not_reach_tenpai() {
        let mut metric = TenpaiWaitMetric::default();
        let iishanten = evaluation("E", 1, &[("3m", 4)]);

        metric.accumulate(4, 0, Some(&iishanten));

        assert_eq!(metric, TenpaiWaitMetric::default());
    }

    #[test]
    fn dead_wait_tenpai_contributes_zero() {
        // 待ちがすべて見えているテンパイへ進む枝は寄与 0。計算していない状態とは区別する。
        let mut metric = TenpaiWaitMetric::default();
        let dead = evaluation("E", 0, &[]);

        metric.accumulate(4, 0, Some(&dead));

        assert_eq!(metric, TenpaiWaitMetric::default());
    }

    #[test]
    fn accumulates_weighted_next_acceptance() {
        let mut metric = WeightedForwardMetric::default();
        let first = evaluation(
            "E",
            2,
            &[("3m", 4), ("6m", 4), ("9m", 4), ("3p", 4), ("6p", 4)],
        );
        let second = evaluation("S", 2, &[("3s", 4), ("6s", 4), ("9s", 2)]);
        metric.accumulate(4, 2, Some(&first));
        metric.accumulate(2, 2, Some(&second));
        assert_eq!(metric.weighted_remaining, 100);
        assert_eq!(metric.weighted_type_count, 26);
    }

    #[test]
    fn next_acceptance_skips_a_branch_that_loses_progress() {
        let mut metric = WeightedForwardMetric::default();
        let back_to_three = evaluation("E", 3, &[("3m", 4)]);
        metric.accumulate(4, 2, Some(&back_to_three));
        assert_eq!(metric, WeightedForwardMetric::default());
    }

    #[test]
    fn weighted_next_acceptance_remaining_outranks_current_acceptance() {
        let wide = evaluation("1m", 2, &[("3m", 4), ("6m", 4), ("9m", 4)]);
        let narrow = evaluation("9p", 2, &[("3p", 4), ("6p", 4)]);
        let comparison = compare_discard_selection_candidates(
            &next_candidate(&narrow, metric(101, 20)),
            &next_candidate(&wide, metric(100, 30)),
        );
        assert!(comparison.candidate_is_better);
        assert_eq!(
            comparison.reason,
            DiscardComparisonReason::WeightedNextAcceptanceRemaining
        );
    }

    #[test]
    fn weighted_next_acceptance_type_count_breaks_a_remaining_tie() {
        let first = evaluation("1m", 3, &[("3m", 4)]);
        let second = evaluation("9p", 3, &[("3p", 4)]);
        let comparison = compare_discard_selection_candidates(
            &next_candidate(&second, metric(100, 31)),
            &next_candidate(&first, metric(100, 30)),
        );
        assert!(comparison.candidate_is_better);
        assert_eq!(
            comparison.reason,
            DiscardComparisonReason::WeightedNextAcceptanceTypeCount
        );
    }

    #[test]
    fn equal_next_acceptance_falls_through_to_current_acceptance() {
        let wide = evaluation("1m", 2, &[("3m", 4), ("6m", 4)]);
        let narrow = evaluation("9p", 2, &[("3p", 4)]);
        let comparison = compare_discard_selection_candidates(
            &next_candidate(&wide, metric(100, 20)),
            &next_candidate(&narrow, metric(100, 20)),
        );
        assert_eq!(
            comparison.reason,
            DiscardComparisonReason::AcceptanceRemaining
        );
    }

    #[test]
    fn shanten_outranks_weighted_next_acceptance() {
        let two = evaluation("1m", 2, &[("3m", 1)]);
        let three = evaluation("9p", 3, &[("3p", 4)]);
        let comparison = compare_discard_selection_candidates(
            &next_candidate(&three, metric(9999, 999)),
            &next_candidate(&two, metric(0, 0)),
        );
        assert!(!comparison.candidate_is_better);
        assert_eq!(comparison.reason, DiscardComparisonReason::Shanten);
    }

    #[test]
    fn weighted_remaining_outranks_current_acceptance() {
        // 受け入れは A が多いが、weighted wait remaining は B が多い。
        let wide_acceptance = evaluation("1m", 1, &[("3m", 4), ("6m", 4), ("9m", 4)]);
        let narrow_acceptance = evaluation("9p", 1, &[("3p", 4), ("6p", 4)]);

        let comparison = compare_discard_selection_candidates(
            &candidate(&narrow_acceptance, metric(64, 16)),
            &candidate(&wide_acceptance, metric(24, 12)),
        );

        assert!(comparison.candidate_is_better);
        assert_eq!(
            comparison.reason,
            DiscardComparisonReason::WeightedTenpaiWaitRemaining
        );
        // 既存の1手比較では受け入れの多い A が勝つ局面である。
        assert!(
            !compare_discard_evaluations(&narrow_acceptance, &wide_acceptance).candidate_is_better
        );
    }

    #[test]
    fn type_count_breaks_equal_weighted_remaining() {
        let first = evaluation("1m", 1, &[("3m", 4)]);
        let second = evaluation("9p", 1, &[("3p", 4)]);

        let comparison = compare_discard_selection_candidates(
            &candidate(&second, metric(64, 20)),
            &candidate(&first, metric(64, 16)),
        );

        assert!(comparison.candidate_is_better);
        assert_eq!(
            comparison.reason,
            DiscardComparisonReason::WeightedTenpaiWaitTypeCount
        );
    }

    #[test]
    fn equal_weighted_wait_falls_through_to_acceptance() {
        let wide = evaluation("1m", 1, &[("3m", 4), ("6m", 4)]);
        let narrow = evaluation("9p", 1, &[("3p", 4)]);

        let comparison = compare_discard_selection_candidates(
            &candidate(&wide, metric(64, 16)),
            &candidate(&narrow, metric(64, 16)),
        );

        assert!(comparison.candidate_is_better);
        assert_eq!(
            comparison.reason,
            DiscardComparisonReason::AcceptanceRemaining
        );
    }

    #[test]
    fn tenpai_candidates_ignore_the_weighted_wait() {
        let wide = evaluation("1m", 0, &[("3m", 4), ("6m", 4)]);
        let narrow = evaluation("9p", 0, &[("3p", 4)]);

        let comparison = compare_discard_selection_candidates(
            &candidate(&narrow, metric(999, 999)),
            &candidate(&wide, metric(1, 1)),
        );

        assert_eq!(
            comparison,
            compare_discard_evaluations(&narrow, &wide),
            "テンパイ同士では既存比較と一致する"
        );
    }

    #[test]
    fn multi_shanten_candidates_ignore_the_weighted_wait() {
        for shanten_value in [2, 3] {
            let wide = evaluation("1m", shanten_value, &[("3m", 4), ("6m", 4)]);
            let narrow = evaluation("9p", shanten_value, &[("3p", 4)]);

            let comparison = compare_discard_selection_candidates(
                &candidate(&narrow, metric(999, 999)),
                &candidate(&wide, metric(1, 1)),
            );

            assert_eq!(comparison, compare_discard_evaluations(&narrow, &wide));
        }
    }

    #[test]
    fn missing_metrics_fall_through_to_the_existing_comparison() {
        let wide = evaluation("1m", 1, &[("3m", 4), ("6m", 4)]);
        let narrow = evaluation("9p", 1, &[("3p", 4)]);

        assert_eq!(
            compare_discard_selection_candidates(
                &candidate(&narrow, metric(999, 999)),
                &candidate(&wide, None),
            ),
            compare_discard_evaluations(&narrow, &wide),
        );
    }

    #[test]
    fn without_metrics_matches_the_existing_comparison() {
        let wide = evaluation("1m", 1, &[("3m", 4), ("6m", 4)]);
        let narrow = evaluation("9p", 1, &[("3p", 4)]);

        assert_eq!(
            compare_discard_selection_candidates(
                &DiscardSelectionCandidate::without_tenpai_wait(&narrow),
                &DiscardSelectionCandidate::without_tenpai_wait(&wide),
            ),
            compare_discard_evaluations(&narrow, &wide),
        );
    }

    #[test]
    fn shanten_still_outranks_the_weighted_wait() {
        let tenpai = evaluation("1m", 0, &[("3m", 1)]);
        let iishanten = evaluation("9p", 1, &[("3p", 4), ("6p", 4)]);

        let comparison = compare_discard_selection_candidates(
            &candidate(&tenpai, None),
            &candidate(&iishanten, metric(999, 999)),
        );

        assert!(comparison.candidate_is_better);
        assert_eq!(comparison.reason, DiscardComparisonReason::Shanten);
    }

    #[test]
    fn best_index_uses_the_weighted_wait() {
        let evaluations = vec![
            evaluation("1m", 1, &[("3m", 4), ("6m", 4), ("9m", 4)]),
            evaluation("9p", 1, &[("3p", 4), ("6p", 4)]),
        ];
        let metrics = vec![metric(24, 12), metric(64, 16)];

        assert_eq!(
            best_discard_selection_index(&evaluations, &metrics),
            Some(1)
        );
        assert_eq!(best_discard_selection_index(&evaluations, &[]), Some(0));
    }

    #[test]
    fn comparison_is_antisymmetric_and_transitive_with_metrics() {
        let evaluations = [
            evaluation("1m", 1, &[("3m", 4), ("6m", 4), ("9m", 4)]),
            evaluation("9p", 1, &[("3p", 4), ("6p", 4)]),
            evaluation("1s", 1, &[("4s", 4)]),
        ];
        let metrics = [metric(64, 16), metric(64, 20), metric(24, 12)];

        let candidates: Vec<_> = evaluations
            .iter()
            .zip(metrics.iter())
            .map(|(evaluation, tenpai_wait)| candidate(evaluation, *tenpai_wait))
            .collect();

        for left in &candidates {
            for right in &candidates {
                let forward = compare_discard_selection_candidates(left, right);
                let backward = compare_discard_selection_candidates(right, left);
                if std::ptr::eq(left.evaluation, right.evaluation) {
                    assert!(!forward.candidate_is_better);
                    continue;
                }
                assert_ne!(
                    forward.candidate_is_better, backward.candidate_is_better,
                    "反対称性"
                );
                assert_eq!(forward.reason, backward.reason);
            }
        }

        // 推移律: index 1 > index 0 > index 2 の順に強い。
        assert!(
            compare_discard_selection_candidates(&candidates[1], &candidates[0])
                .candidate_is_better
        );
        assert!(
            compare_discard_selection_candidates(&candidates[0], &candidates[2])
                .candidate_is_better
        );
        assert!(
            compare_discard_selection_candidates(&candidates[1], &candidates[2])
                .candidate_is_better
        );
    }

    #[test]
    fn requires_tenpai_wait_only_for_multiple_iishanten_candidates() {
        let iishanten_a = evaluation("1m", 1, &[("3m", 4)]);
        let iishanten_b = evaluation("9p", 1, &[("3p", 4)]);
        let tenpai = evaluation("1s", 0, &[("4s", 4)]);
        let two_shanten = evaluation("2s", 2, &[("5s", 4)]);

        assert!(requires_tenpai_wait(&[
            iishanten_a.clone(),
            iishanten_b.clone()
        ]));
        assert!(!requires_tenpai_wait(std::slice::from_ref(&iishanten_a)));
        assert!(!requires_tenpai_wait(&[
            iishanten_a.clone(),
            two_shanten.clone()
        ]));
        assert!(!requires_tenpai_wait(&[
            tenpai.clone(),
            iishanten_a.clone(),
            iishanten_b.clone()
        ]));
        assert!(!requires_tenpai_wait(std::slice::from_ref(&two_shanten)));
        assert!(!requires_tenpai_wait(&[]));
    }

    #[test]
    fn forward_metrics_require_multiple_best_candidates_at_one_or_more_shanten() {
        let two_a = evaluation("1m", 2, &[]);
        let two_b = evaluation("2m", 2, &[]);
        let three = evaluation("3m", 3, &[]);
        let tenpai_a = evaluation("4m", 0, &[]);
        let tenpai_b = evaluation("5m", 0, &[]);

        assert!(requires_forward_metrics(&[two_a.clone(), two_b]));
        assert!(!requires_forward_metrics(&[two_a.clone(), three]));
        assert!(!requires_forward_metrics(std::slice::from_ref(&two_a)));
        assert!(!requires_forward_metrics(&[tenpai_a, tenpai_b]));
    }

    #[test]
    fn forward_target_is_only_iishanten() {
        assert_eq!(forward_target_mask(&[evaluation("1m", 1, &[])]), vec![true]);
        assert_eq!(
            forward_target_mask(&[evaluation("1m", 0, &[])]),
            vec![false]
        );
    }
}
