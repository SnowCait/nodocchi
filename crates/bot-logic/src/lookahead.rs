//! 打牌候補ごとの2手先診断。
//!
//! 「現在打牌 → その打牌後の各受け入れ牌を1枚ツモった仮想手牌 → 既存打牌評価による次の最良打牌」
//! を構造化して返す解析専用の pure な基盤。向聴・受け入れ・一向聴形分類・文脈反映・打牌比較は
//! すべて既存実装を呼び出し、2手先専用の計算器は持たない。
//!
//! # 仮想ツモ牌の物理牌
//!
//! 受け入れ ([`EffectiveAcceptanceTile`]) は 34 種の [`TileType`] 単位なので、仮想ツモ牌の物理牌
//! ([`TileId`]) は決まらない。仮想ツモ牌以外の牌は現在の手牌の物理牌をそのまま引き継ぐため、
//! 通常打牌評価と同じ文脈で評価できる。
//!
//! - 副露済み面子数・見え牌・場風・自風・ドラ表示牌: すべて通常打牌評価と同じ値を反映する
//! - 役牌 (`discarded_value_honor_count`): 牌種と場風・自風だけで決まるので必ず反映する
//! - 通常ドラ (ドラ表示牌が示す分): 牌種だけで決まるので必ず反映する
//! - 1手目に切る物理牌: 通常打牌評価が合法 Dahai に合わせて確定した牌をそのまま除去する
//! - 赤5 (`discards_red_five` と赤ドラ分): 仮想ツモ牌の牌種を2手目に切る候補でだけ解決できない
//!
//! 最後の未解決分はさらに絞り込める。手牌に同種の黒牌が残っていれば通常打牌評価も黒を選ぶので
//! 結果は一致し、赤1枚しか無い場合もその赤が手牌に見えている以上、仮想ツモは黒に確定する。
//! したがって実際に未解決なのは「仮想ツモ牌と同種の牌が手牌に1枚も残っていない場合」だけで、
//! そのとき赤5扱いにはせず赤ドラ分も加算しない。赤5の確率モデルなど新しい仕様は導入しない。

use crate::acceptance::EffectiveAcceptanceTile;
use crate::discard::{
    CandidateSeen, DecorationContext, DiscardEvaluation, ShapePenaltyMode, decorate_evaluations,
    discarded_tile_id_for_type, evaluate_discards_with_seen, select_best,
};
use crate::iishanten::IishantenShape;
use crate::selection::{
    ForwardMetrics, TenpaiWaitMetric, WeightedForwardMetric, forward_target_mask,
    requires_forward_metrics,
};
use crate::shanten::{EffectiveShanten, FixedMeldCount};
use crate::tile::{TileId, TileType};
use crate::tile_counts::TileCounts;

/// 現在の打牌候補1件について、その打牌後の受け入れ牌を1枚ツモった仮想手牌を既存打牌評価へ
/// かけた2手先診断。
///
/// pure なデータであり、押し引き・鳴き・リーチ判断のどれにも使用しない。打牌選択が使うのは
/// [`DiscardLookaheadDiagnostic::weighted_forward_metric`] が返す集計値だけで、通常の選択経路は
/// この診断そのものを構築しない。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscardLookaheadDiagnostic {
    /// 現在の打牌候補の牌種。
    pub discard: TileType,
    /// 現在打牌後の受け入れ牌ごとの2手先診断。順序と対象牌は現在打牌後の受け入れと同じ。
    pub draws: Vec<DrawLookaheadDiagnostic>,
}

impl DiscardLookaheadDiagnostic {
    pub fn draw(&self, tile: TileType) -> Option<&DrawLookaheadDiagnostic> {
        self.draws.iter().find(|draw| draw.draw == tile)
    }

    /// 構築済みの枝から打牌選択用の weighted tenpai wait を集計する。
    ///
    /// 集計規則は選択専用経路 ([`tenpai_wait_metrics_with_fixed_melds`]) と共有するため、詳細
    /// 診断を構築した場合に同じ枝を2回評価しなくてよい。
    pub fn tenpai_wait_metric(&self) -> TenpaiWaitMetric {
        self.weighted_forward_metric(0)
    }

    pub fn weighted_forward_metric(&self, required_next_shanten: i8) -> WeightedForwardMetric {
        let mut metric = WeightedForwardMetric::default();
        for draw in &self.draws {
            metric.accumulate(
                draw.remaining,
                required_next_shanten,
                draw.next_discard.as_ref(),
            );
        }
        metric
    }
}

/// 現在打牌後の受け入れ牌1枚を仮想的にツモった場合の2手先診断。
///
/// `draw` / `remaining` / `shanten_after_draw` は現在打牌の [`DiscardEvaluation`] が持つ
/// 受け入れ ([`EffectiveAcceptanceTile`]) の値そのもので、診断のために再計算しない。
/// `remaining` は牌種ごとの残枚数を生データのまま保持し、期待値や加重平均へ潰さない。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DrawLookaheadDiagnostic {
    pub draw: TileType,
    pub remaining: u8,
    pub shanten_after_draw: EffectiveShanten,
    /// 仮想ツモ後の手牌に既存打牌評価と既存比較順を適用した最良打牌。打牌候補が1件も無い場合だけ
    /// `None`。数値のコピーではなく評価そのものを保持する。
    ///
    /// 副露済み面子数・見え牌・場風・自風・ドラ表示牌は現在の打牌評価と同じ値を反映する。
    /// 赤5だけはモジュールの「仮想ツモ牌の物理牌」どおり解決できない場合がある。
    pub next_discard: Option<DiscardEvaluation>,
}

impl DrawLookaheadDiagnostic {
    pub fn next_discard_tile(&self) -> Option<TileType> {
        self.next_discard.as_ref().map(|next| next.discard)
    }

    pub fn next_min_shanten(&self) -> Option<i8> {
        self.next_discard
            .as_ref()
            .map(DiscardEvaluation::min_shanten_after_discard)
    }

    pub fn next_acceptance_total_remaining(&self) -> Option<u8> {
        self.next_discard
            .as_ref()
            .map(DiscardEvaluation::acceptance_total_remaining)
    }

    pub fn next_acceptance_type_count(&self) -> Option<usize> {
        self.next_discard
            .as_ref()
            .map(DiscardEvaluation::acceptance_type_count)
    }

    pub fn next_standard_iishanten_shape(&self) -> Option<IishantenShape> {
        self.next_discard
            .as_ref()
            .map(|next| next.standard_iishanten_shape_after_discard)
    }
}

/// 全打牌候補分の2手先診断。
///
/// `candidates` は入力の打牌候補評価と同じ順序・同じ件数で、selected 候補だけでなく runner-up を
/// 含む全候補に対応する。
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct LookaheadDiagnostic {
    pub candidates: Vec<DiscardLookaheadDiagnostic>,
}

impl LookaheadDiagnostic {
    pub fn candidate(&self, discard: TileType) -> Option<&DiscardLookaheadDiagnostic> {
        self.candidates
            .iter()
            .find(|candidate| candidate.discard == discard)
    }
}

// 2手先診断の評価入力。通常打牌評価が使う値と同じものだけを持ち、上位層の局面型には依存しない。
struct LookaheadInputs<'a> {
    counts: TileCounts,
    tiles: &'a [TileId],
    fixed_meld_count: FixedMeldCount,
    dora_indicators: &'a [TileId],
    round_wind: Option<TileType>,
    seat_wind: Option<TileType>,
    seen: CandidateSeen,
}

/// 副露済み面子数と場風・自風・ドラ表示牌を考慮して全打牌候補の2手先診断を構築する。
///
/// `tiles` は打牌前の全手牌（物理牌）、`evaluations` はその手牌に対する既存の打牌候補評価で、
/// `fixed_meld_count` / `dora_indicators` / `round_wind` / `seat_wind` はその評価に使ったものと
/// 同じ値を渡す。現在打牌後の受け入れは `evaluations` が持つ値をそのまま使い、診断のために
/// 再計算しない。
///
/// 2手目の打牌評価・受け入れ・向聴・一向聴形分類・文脈反映・比較は既存の打牌評価経路をそのまま
/// 呼び出す。2手先専用の shanten / acceptance / comparator / shape evaluator は持たない。
pub fn diagnose_lookahead_with_fixed_melds(
    tiles: &[TileId],
    fixed_meld_count: FixedMeldCount,
    dora_indicators: &[TileId],
    round_wind: Option<TileType>,
    seat_wind: Option<TileType>,
    evaluations: &[DiscardEvaluation],
) -> LookaheadDiagnostic {
    diagnose_lookahead(
        &LookaheadInputs {
            counts: TileCounts::from_tiles(tiles.iter().copied()),
            tiles,
            fixed_meld_count,
            dora_indicators,
            round_wind,
            seat_wind,
            seen: CandidateSeen::hand_only(),
        },
        evaluations,
    )
}

/// visible tiles も考慮して全打牌候補の2手先診断を構築する。
///
/// 2手目の残枚数は「1手目の打牌前に見えていた牌 + 1手目に切った牌1枚」を seen として求める。
/// 1手目に切った牌は2手目時点で見え牌なので、山に残っている牌として数え直さない。
/// visible tiles は自分の手牌を含むため、既存の残枚数計算と同じ手牌差し引きで二重計上を防ぐ。
///
/// `visible_tiles` が空の場合は [`diagnose_lookahead_with_fixed_melds`] と同じ seen 扱いになる。
#[allow(clippy::too_many_arguments)]
pub fn diagnose_lookahead_with_fixed_melds_and_visible_tiles(
    tiles: &[TileId],
    fixed_meld_count: FixedMeldCount,
    dora_indicators: &[TileId],
    round_wind: Option<TileType>,
    seat_wind: Option<TileType>,
    visible_tiles: &[TileId],
    evaluations: &[DiscardEvaluation],
) -> LookaheadDiagnostic {
    if visible_tiles.is_empty() {
        return diagnose_lookahead_with_fixed_melds(
            tiles,
            fixed_meld_count,
            dora_indicators,
            round_wind,
            seat_wind,
            evaluations,
        );
    }

    let counts = TileCounts::from_tiles(tiles.iter().copied());
    diagnose_lookahead(
        &LookaheadInputs {
            counts,
            tiles,
            fixed_meld_count,
            dora_indicators,
            round_wind,
            seat_wind,
            seen: CandidateSeen::from_visible_tiles(&counts, visible_tiles),
        },
        evaluations,
    )
}

/// 副露済み面子数と場風・自風・ドラ表示牌を考慮して、打牌選択用の前方集計値を求める。
///
/// 戻り値は `evaluations` と同じ順序・同じ件数で、前方評価を計算しなかった候補は `None`。
/// 計算対象は最善向聴数が1以上で、それを維持する候補が複数ある場合の最善候補だけ。
/// 1向聴では weighted tenpai wait、2向聴以上では weighted next acceptance を返す。
///
/// 枝の評価は詳細診断 ([`diagnose_lookahead_with_fixed_melds`]) と同じ helper を共有し、選択用に
/// [`LookaheadDiagnostic`] を構築しない。
pub fn forward_metrics_with_fixed_melds(
    tiles: &[TileId],
    fixed_meld_count: FixedMeldCount,
    dora_indicators: &[TileId],
    round_wind: Option<TileType>,
    seat_wind: Option<TileType>,
    evaluations: &[DiscardEvaluation],
) -> Vec<ForwardMetrics> {
    if !requires_forward_metrics(evaluations) {
        return vec![ForwardMetrics::default(); evaluations.len()];
    }

    forward_metrics(
        &LookaheadInputs {
            counts: TileCounts::from_tiles(tiles.iter().copied()),
            tiles,
            fixed_meld_count,
            dora_indicators,
            round_wind,
            seat_wind,
            seen: CandidateSeen::hand_only(),
        },
        evaluations,
    )
}

pub fn tenpai_wait_metrics_with_fixed_melds(
    tiles: &[TileId],
    fixed_meld_count: FixedMeldCount,
    dora_indicators: &[TileId],
    round_wind: Option<TileType>,
    seat_wind: Option<TileType>,
    evaluations: &[DiscardEvaluation],
) -> Vec<Option<TenpaiWaitMetric>> {
    forward_metrics_with_fixed_melds(
        tiles,
        fixed_meld_count,
        dora_indicators,
        round_wind,
        seat_wind,
        evaluations,
    )
    .into_iter()
    .map(|metric| metric.tenpai_wait)
    .collect()
}

/// visible tiles も考慮して打牌選択用の weighted tenpai wait を求める。
///
/// 2手目の残枚数の扱いは [`diagnose_lookahead_with_fixed_melds_and_visible_tiles`] と同じで、
/// 1手目に切った牌も2手目時点の見え牌として数える。
#[allow(clippy::too_many_arguments)]
pub fn forward_metrics_with_fixed_melds_and_visible_tiles(
    tiles: &[TileId],
    fixed_meld_count: FixedMeldCount,
    dora_indicators: &[TileId],
    round_wind: Option<TileType>,
    seat_wind: Option<TileType>,
    visible_tiles: &[TileId],
    evaluations: &[DiscardEvaluation],
) -> Vec<ForwardMetrics> {
    if visible_tiles.is_empty() {
        return forward_metrics_with_fixed_melds(
            tiles,
            fixed_meld_count,
            dora_indicators,
            round_wind,
            seat_wind,
            evaluations,
        );
    }

    if !requires_forward_metrics(evaluations) {
        return vec![ForwardMetrics::default(); evaluations.len()];
    }

    let counts = TileCounts::from_tiles(tiles.iter().copied());
    forward_metrics(
        &LookaheadInputs {
            counts,
            tiles,
            fixed_meld_count,
            dora_indicators,
            round_wind,
            seat_wind,
            seen: CandidateSeen::from_visible_tiles(&counts, visible_tiles),
        },
        evaluations,
    )
}

#[allow(clippy::too_many_arguments)]
pub fn tenpai_wait_metrics_with_fixed_melds_and_visible_tiles(
    tiles: &[TileId],
    fixed_meld_count: FixedMeldCount,
    dora_indicators: &[TileId],
    round_wind: Option<TileType>,
    seat_wind: Option<TileType>,
    visible_tiles: &[TileId],
    evaluations: &[DiscardEvaluation],
) -> Vec<Option<TenpaiWaitMetric>> {
    forward_metrics_with_fixed_melds_and_visible_tiles(
        tiles,
        fixed_meld_count,
        dora_indicators,
        round_wind,
        seat_wind,
        visible_tiles,
        evaluations,
    )
    .into_iter()
    .map(|metric| metric.tenpai_wait)
    .collect()
}

/// 構築済みの2手先診断から打牌選択用の weighted tenpai wait を求める。
///
/// 詳細診断を作る経路で、同じ「現在打牌 × 受け入れ牌 × 次打牌評価」を2回計算しないための入口。
/// 対象候補の条件と集計規則は選択専用経路と同じなので、詳細診断の有無で選択結果は変わらない。
///
/// `lookahead` は `evaluations` から構築したものを渡す。候補の順序・牌種が対応しない場合は
/// 推測せず `None` にする。
pub fn forward_metrics_from_lookahead(
    evaluations: &[DiscardEvaluation],
    lookahead: &LookaheadDiagnostic,
) -> Vec<ForwardMetrics> {
    if !requires_forward_metrics(evaluations) || lookahead.candidates.len() != evaluations.len() {
        return vec![ForwardMetrics::default(); evaluations.len()];
    }

    let best_shanten = evaluations
        .iter()
        .map(DiscardEvaluation::min_shanten_after_discard)
        .min()
        .unwrap_or(i8::MAX);

    let targets = forward_target_mask(evaluations);
    evaluations
        .iter()
        .zip(lookahead.candidates.iter())
        .zip(targets)
        .map(|((evaluation, candidate), target)| {
            if !target || candidate.discard != evaluation.discard {
                return ForwardMetrics::default();
            }
            let metric = candidate.weighted_forward_metric(best_shanten - 1);
            if best_shanten == 1 {
                ForwardMetrics {
                    tenpai_wait: Some(metric),
                    next_acceptance: None,
                }
            } else {
                ForwardMetrics {
                    tenpai_wait: None,
                    next_acceptance: Some(metric),
                }
            }
        })
        .collect()
}

pub fn tenpai_wait_metrics_from_lookahead(
    evaluations: &[DiscardEvaluation],
    lookahead: &LookaheadDiagnostic,
) -> Vec<Option<TenpaiWaitMetric>> {
    forward_metrics_from_lookahead(evaluations, lookahead)
        .into_iter()
        .map(|metric| metric.tenpai_wait)
        .collect()
}

fn diagnose_lookahead(
    inputs: &LookaheadInputs,
    evaluations: &[DiscardEvaluation],
) -> LookaheadDiagnostic {
    LookaheadDiagnostic {
        candidates: evaluations
            .iter()
            .map(|evaluation| lookahead_for_candidate(inputs, evaluation))
            .collect(),
    }
}

// 前方評価の対象候補だけ枝を評価して集計する。対象外の候補は探索しない。
fn forward_metrics(
    inputs: &LookaheadInputs,
    evaluations: &[DiscardEvaluation],
) -> Vec<ForwardMetrics> {
    let best_shanten = evaluations
        .iter()
        .map(DiscardEvaluation::min_shanten_after_discard)
        .min()
        .unwrap_or(i8::MAX);
    let targets = forward_target_mask(evaluations);
    evaluations
        .iter()
        .zip(targets)
        .map(|(evaluation, target)| {
            if !target {
                return ForwardMetrics::default();
            }
            let metric =
                weighted_forward_metric_for_candidate(inputs, evaluation, best_shanten - 1);
            if best_shanten == 1 {
                ForwardMetrics {
                    tenpai_wait: Some(metric),
                    next_acceptance: None,
                }
            } else {
                ForwardMetrics {
                    tenpai_wait: None,
                    next_acceptance: Some(metric),
                }
            }
        })
        .collect()
}

// 現在の打牌候補1件分の2手先診断。入力は変更せず、打牌後の手牌を copy で作る。
fn lookahead_for_candidate(
    inputs: &LookaheadInputs,
    evaluation: &DiscardEvaluation,
) -> DiscardLookaheadDiagnostic {
    let Some(branch) = CandidateBranch::new(inputs, evaluation) else {
        return DiscardLookaheadDiagnostic {
            discard: evaluation.discard,
            draws: Vec::new(),
        };
    };

    let draws = evaluation
        .acceptance_after_discard
        .tiles
        .iter()
        .map(|tile| DrawLookaheadDiagnostic {
            draw: tile.tile,
            remaining: tile.remaining,
            shanten_after_draw: tile.shanten_after_draw,
            next_discard: branch.next_discard(inputs, tile),
        })
        .collect();

    DiscardLookaheadDiagnostic {
        discard: evaluation.discard,
        draws,
    }
}

// 現在の打牌候補1件分の weighted tenpai wait。枝の評価は詳細診断と同じ helper を共有し、
// 診断 object を作らずに集計値だけを求める。
fn weighted_forward_metric_for_candidate(
    inputs: &LookaheadInputs,
    evaluation: &DiscardEvaluation,
    required_next_shanten: i8,
) -> WeightedForwardMetric {
    let mut metric = WeightedForwardMetric::default();
    let Some(branch) = CandidateBranch::new(inputs, evaluation) else {
        return metric;
    };

    for tile in &evaluation.acceptance_after_discard.tiles {
        metric.accumulate(
            tile.remaining,
            required_next_shanten,
            branch.next_discard(inputs, tile).as_ref(),
        );
    }
    metric
}

// 現在の打牌候補1件について、その打牌後の受け入れ牌を仮想ツモした2手目評価に必要な状態。
// 受け入れ牌ごとに作り直さず、詳細診断と選択用集計で共有する。
struct CandidateBranch {
    after_discard: TileCounts,
    next_tiles: Vec<TileId>,
    seen: CandidateSeen,
}

impl CandidateBranch {
    // 打牌候補の牌種を手牌から除けない場合だけ `None`。
    fn new(inputs: &LookaheadInputs, evaluation: &DiscardEvaluation) -> Option<Self> {
        let mut after_discard = inputs.counts;
        after_discard.remove(evaluation.discard).ok()?;

        Some(Self {
            after_discard,
            // 1手目に実際に切られる物理牌を2手目の物理牌一覧から外す。赤5と黒5の両方を持ち
            // 片方だけが合法な局面でも、通常打牌評価が確定した物理牌をそのまま引き継ぐ。
            next_tiles: tiles_after_discard(
                inputs.tiles,
                evaluation.discard,
                evaluation.discards_red_five,
            ),
            // 1手目に切った牌は2手目時点で見え牌になる。
            seen: inputs.seen.after_discard(evaluation.discard),
        })
    }

    // 受け入れ牌1枚を仮想的にツモった手牌を作り、既存打牌評価・既存文脈反映・既存比較順で最良
    // 打牌を求める。仮想ツモ牌は物理牌が決まらないため `next_tiles` には含めず、赤5が解決
    // できない牌種として既存の decoration へ渡す。
    fn next_discard(
        &self,
        inputs: &LookaheadInputs,
        tile: &EffectiveAcceptanceTile,
    ) -> Option<DiscardEvaluation> {
        let mut hypothetical = self.after_discard;
        hypothetical.try_add(tile.tile).ok()?;

        let mut evaluations =
            evaluate_discards_with_seen(&hypothetical, inputs.fixed_meld_count, &self.seen);
        decorate_evaluations(
            &mut evaluations,
            &hypothetical,
            &DecorationContext {
                tiles: &self.next_tiles,
                dora_indicators: inputs.dora_indicators,
                round_wind: inputs.round_wind,
                seat_wind: inputs.seat_wind,
                shape_penalty: ShapePenaltyMode::WithContext {
                    round_wind: inputs.round_wind,
                    seat_wind: inputs.seat_wind,
                    fixed_meld_count: inputs.fixed_meld_count,
                },
                unresolved_red_tile: Some(tile.tile),
            },
        );
        select_best(evaluations)
    }
}

// 指定牌種を1枚切った後の物理牌一覧。
//
// `discards_red_five` は通常打牌評価が確定した「実際に切る牌が赤5かどうか」。合法 Dahai へ
// 補正済みの [`DiscardEvaluation`] が持つ値をそのまま渡すことで、赤5だけ・黒5だけが合法な
// 局面でも実際に切られる物理牌を除ける。物理牌の選択は既存 helper へ委ねる。
fn tiles_after_discard(
    tiles: &[TileId],
    discard: TileType,
    discards_red_five: bool,
) -> Vec<TileId> {
    let mut remaining = tiles.to_vec();
    let Some(discarded) = discarded_tile_id_for_type(discard, tiles, Some(discards_red_five))
    else {
        return remaining;
    };
    if let Some(position) = remaining.iter().position(|tile| *tile == discarded) {
        remaining.remove(position);
    }
    remaining
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::discard::{
        evaluate_discards_from_tiles_with_fixed_melds_and_context,
        evaluate_discards_from_tiles_with_fixed_melds_and_visible_tiles,
        select_best_discard_from_tiles_with_context,
        select_best_discard_from_tiles_with_visible_tiles,
    };
    use crate::tile::count_indicated_dora;
    use std::sync::LazyLock;

    // 打牌評価までを持つ検証用の局面。全候補分の2手先診断は重いので、1枝だけを見るテストは
    // この局面から必要な枝だけを構築する。
    struct Situation {
        tiles: Vec<TileId>,
        counts: TileCounts,
        visible: Vec<TileId>,
        dora_indicators: Vec<TileId>,
        round_wind: Option<TileType>,
        seat_wind: Option<TileType>,
        fixed_meld_count: FixedMeldCount,
        evaluations: Vec<DiscardEvaluation>,
    }

    // 局面と全候補分の2手先診断を1組にした検証用の case。2手先探索は重いので、同じ局面を使う
    // 複数のテストで構築結果を共有する。
    struct Case {
        situation: Situation,
        lookahead: LookaheadDiagnostic,
    }

    fn tile(s: &str) -> TileType {
        TileType::from_mjai_type_str(s).unwrap()
    }

    fn ids(values: &[u8]) -> Vec<TileId> {
        values.iter().map(|&v| TileId::new(v).unwrap()).collect()
    }

    fn fixed(value: u8) -> FixedMeldCount {
        FixedMeldCount::new(value).unwrap()
    }

    // 門前14枚 112233m 456p 78s 11z 2z。赤5を含まない物理牌で構成する。
    fn concealed_hand() -> Vec<TileId> {
        ids(&[0, 1, 4, 5, 8, 9, 48, 53, 57, 96, 100, 108, 109, 112])
    }

    // 手牌以外に見えている牌。1m 2枚・5p 1枚・W 1枚。
    fn public_visible_tiles() -> Vec<TileId> {
        ids(&[2, 3, 55, 116])
    }

    // 2副露済みとみなした concealed 7枚 + ツモ 9p の8枚。123m 12p 55s 9p。
    fn melded_hand() -> Vec<TileId> {
        ids(&[0, 4, 8, 36, 40, 89, 90, 68])
    }

    fn hand_only_situation(
        tiles: &[TileId],
        fixed_meld_count: FixedMeldCount,
        dora_indicators: Vec<TileId>,
        round_wind: Option<TileType>,
        seat_wind: Option<TileType>,
    ) -> Situation {
        let evaluations = evaluate_discards_from_tiles_with_fixed_melds_and_context(
            tiles,
            fixed_meld_count,
            &dora_indicators,
            round_wind,
            seat_wind,
        );
        Situation {
            tiles: tiles.to_vec(),
            counts: TileCounts::from_tiles(tiles.iter().copied()),
            visible: Vec::new(),
            dora_indicators,
            round_wind,
            seat_wind,
            fixed_meld_count,
            evaluations,
        }
    }

    fn visible_situation(
        tiles: &[TileId],
        fixed_meld_count: FixedMeldCount,
        dora_indicators: Vec<TileId>,
        round_wind: Option<TileType>,
        seat_wind: Option<TileType>,
        visible: Vec<TileId>,
    ) -> Situation {
        let evaluations = evaluate_discards_from_tiles_with_fixed_melds_and_visible_tiles(
            tiles,
            fixed_meld_count,
            &dora_indicators,
            round_wind,
            seat_wind,
            &visible,
        );
        Situation {
            tiles: tiles.to_vec(),
            counts: TileCounts::from_tiles(tiles.iter().copied()),
            visible,
            dora_indicators,
            round_wind,
            seat_wind,
            fixed_meld_count,
            evaluations,
        }
    }

    // 局面が持つ見え牌の有無に合わせて、既存の2手先診断エントリを呼び分ける。
    fn diagnose(situation: &Situation, evaluations: &[DiscardEvaluation]) -> LookaheadDiagnostic {
        if situation.visible.is_empty() {
            diagnose_lookahead_with_fixed_melds(
                &situation.tiles,
                situation.fixed_meld_count,
                &situation.dora_indicators,
                situation.round_wind,
                situation.seat_wind,
                evaluations,
            )
        } else {
            diagnose_lookahead_with_fixed_melds_and_visible_tiles(
                &situation.tiles,
                situation.fixed_meld_count,
                &situation.dora_indicators,
                situation.round_wind,
                situation.seat_wind,
                &situation.visible,
                evaluations,
            )
        }
    }

    fn full_case(situation: Situation) -> Case {
        let lookahead = diagnose(&situation, &situation.evaluations);
        Case {
            situation,
            lookahead,
        }
    }

    fn hand_only_case(
        tiles: &[TileId],
        fixed_meld_count: FixedMeldCount,
        dora_indicators: Vec<TileId>,
        round_wind: Option<TileType>,
        seat_wind: Option<TileType>,
    ) -> Case {
        full_case(hand_only_situation(
            tiles,
            fixed_meld_count,
            dora_indicators,
            round_wind,
            seat_wind,
        ))
    }

    fn visible_case(
        tiles: &[TileId],
        fixed_meld_count: FixedMeldCount,
        dora_indicators: Vec<TileId>,
        round_wind: Option<TileType>,
        seat_wind: Option<TileType>,
        visible: Vec<TileId>,
    ) -> Case {
        full_case(visible_situation(
            tiles,
            fixed_meld_count,
            dora_indicators,
            round_wind,
            seat_wind,
            visible,
        ))
    }

    // 現在打牌1つ・受け入れ牌1枚だけへ絞った2手先の最良打牌。既存の受け入れから対象の1枚だけを
    // 残した打牌評価を production の2手先診断へ渡すので、全候補分を構築した場合の同じ枝と同じ
    // 結果になる。context-specific regression が full lookahead を作らないための test 専用 helper。
    fn branch_next_discard(
        situation: &Situation,
        discard: TileType,
        draw: TileType,
    ) -> DiscardEvaluation {
        let mut evaluation = situation
            .evaluations
            .iter()
            .find(|evaluation| evaluation.discard == discard)
            .expect("current discard evaluation exists")
            .clone();
        evaluation
            .acceptance_after_discard
            .tiles
            .retain(|accepted| accepted.tile == draw);
        assert_eq!(
            evaluation.acceptance_after_discard.tiles.len(),
            1,
            "打牌 {discard:?} の受け入れに {draw:?} が含まれている必要がある"
        );

        diagnose(situation, std::slice::from_ref(&evaluation))
            .candidate(discard)
            .and_then(|candidate| candidate.draw(draw))
            .and_then(|draw| draw.next_discard.clone())
            .expect("next discard exists")
    }

    static CONCEALED_HAND_ONLY: LazyLock<Case> = LazyLock::new(|| {
        hand_only_case(
            &concealed_hand(),
            FixedMeldCount::NONE,
            Vec::new(),
            None,
            None,
        )
    });

    static CONCEALED_WITH_VISIBLE: LazyLock<Case> = LazyLock::new(|| {
        let mut visible = concealed_hand();
        visible.extend(public_visible_tiles());
        visible_case(
            &concealed_hand(),
            FixedMeldCount::NONE,
            Vec::new(),
            None,
            None,
            visible,
        )
    });

    // 指定牌種のうち、まだ使われていない物理牌を1枚返す。赤5の曖昧さを持ち込まないよう黒牌を
    // 優先する。
    fn unused_copy(tile_type: TileType, used: &[TileId]) -> TileId {
        let copies: Vec<TileId> = (0..4)
            .filter_map(|offset| TileId::new(tile_type.raw() * 4 + offset))
            .filter(|id| !used.contains(id))
            .collect();
        copies
            .iter()
            .copied()
            .find(|id| !id.is_red())
            .or_else(|| copies.first().copied())
            .expect("an unused physical copy exists")
    }

    #[test]
    fn covers_every_current_discard_candidate() {
        let case = &*CONCEALED_HAND_ONLY;

        assert!(case.situation.evaluations.len() > 1);
        assert_eq!(
            case.lookahead.candidates.len(),
            case.situation.evaluations.len()
        );
        for (candidate, evaluation) in case
            .lookahead
            .candidates
            .iter()
            .zip(case.situation.evaluations.iter())
        {
            assert_eq!(candidate.discard, evaluation.discard);
            assert_eq!(
                case.lookahead
                    .candidate(evaluation.discard)
                    .map(|found| found.discard),
                Some(evaluation.discard)
            );
        }
    }

    #[test]
    fn draws_reuse_the_existing_acceptance_of_the_current_discard() {
        let case = &*CONCEALED_WITH_VISIBLE;

        for (candidate, evaluation) in case
            .lookahead
            .candidates
            .iter()
            .zip(case.situation.evaluations.iter())
        {
            let acceptance = &evaluation.acceptance_after_discard.tiles;
            assert_eq!(candidate.draws.len(), acceptance.len());
            for (draw, accepted) in candidate.draws.iter().zip(acceptance.iter()) {
                assert_eq!(draw.draw, accepted.tile);
                assert_eq!(draw.remaining, accepted.remaining);
                assert_eq!(draw.shanten_after_draw, accepted.shanten_after_draw);
            }
        }
    }

    // 仮想ツモ後の手牌を物理牌一覧として組み立てる。ツモ牌には未使用の黒牌を割り当てるため、
    // 既存の context-aware 評価 API へそのまま渡せる。
    fn hypothetical_tiles(situation: &Situation, discard: TileType, draw: TileType) -> Vec<TileId> {
        let discards_red_five = situation
            .evaluations
            .iter()
            .find(|evaluation| evaluation.discard == discard)
            .map(|evaluation| evaluation.discards_red_five)
            .expect("current discard evaluation exists");
        let mut tiles = tiles_after_discard(&situation.tiles, discard, discards_red_five);
        let mut used = tiles.clone();
        used.extend(situation.visible.iter().copied());
        used.extend(situation.dora_indicators.iter().copied());
        tiles.push(unused_copy(draw, &used));
        tiles
    }

    // 仮想ツモ牌を見え牌へ足した visible tiles。1手目の打牌は手牌から消えても visible に残る。
    fn hypothetical_visible(situation: &Situation, drawn_tile: TileId) -> Vec<TileId> {
        let mut visible = situation.visible.clone();
        visible.push(drawn_tile);
        visible
    }

    // 2手目の最良打牌を、既存の context-aware 評価 API だけで求める。lookahead 側の期待値を
    // テスト内で再実装しないための共通 helper。
    fn expected_next_discard(
        situation: &Situation,
        discard: TileType,
        draw: TileType,
    ) -> Option<DiscardEvaluation> {
        let tiles = hypothetical_tiles(situation, discard, draw);
        let drawn_tile = *tiles.last().expect("the drawn tile was pushed last");
        assert!(!drawn_tile.is_red(), "赤5の曖昧さが無い局面で検証する");

        if situation.visible.is_empty() {
            select_best_discard_from_tiles_with_context(
                &tiles,
                &situation.dora_indicators,
                situation.round_wind,
                situation.seat_wind,
            )
        } else {
            select_best_discard_from_tiles_with_visible_tiles(
                &tiles,
                &situation.dora_indicators,
                situation.round_wind,
                situation.seat_wind,
                &hypothetical_visible(situation, drawn_tile),
            )
        }
    }

    // 全候補 × 全受け入れ牌で、2手先の next discard が既存 context-aware 評価と一致することを
    // 確認する。戻り値は検証した件数。
    fn assert_next_discard_matches_existing_evaluation(case: &Case) -> usize {
        assert_eq!(case.situation.fixed_meld_count, FixedMeldCount::NONE);

        let mut checked = 0;
        for candidate in &case.lookahead.candidates {
            for draw in &candidate.draws {
                assert_eq!(
                    draw.next_discard,
                    expected_next_discard(&case.situation, candidate.discard, draw.draw),
                    "discard {:?} draw {:?}",
                    candidate.discard,
                    draw.draw,
                );
                checked += 1;
            }
        }
        checked
    }

    #[test]
    fn next_discard_matches_the_existing_evaluation_and_comparator() {
        assert!(assert_next_discard_matches_existing_evaluation(&CONCEALED_WITH_VISIBLE) > 0);
    }

    // ---- context-aware な next discard ----

    // 役牌・通常ドラ検証用の門前14枚 123m456m789m 1p 55p S W。
    //
    // 孤立字牌 S と W だけが打牌候補として同格になり、場風・自風やドラ表示牌が無ければ
    // 比較は StableOrder まで落ちる。赤5は含まない。
    fn honor_choice_hand() -> Vec<TileId> {
        ids(&[0, 4, 8, 12, 17, 20, 24, 28, 32, 36, 53, 54, 112, 116])
    }

    fn honor_choice_situation(
        dora_indicators: Vec<TileId>,
        round_wind: Option<TileType>,
        seat_wind: Option<TileType>,
    ) -> Situation {
        let hand = honor_choice_hand();
        let mut visible = hand.clone();
        visible.extend(dora_indicators.iter().copied());
        visible_situation(
            &hand,
            FixedMeldCount::NONE,
            dora_indicators,
            round_wind,
            seat_wind,
            visible,
        )
    }

    // 東場南家。S が自風の役牌、W は無関係な客風。
    static VALUE_HONOR_SITUATION: LazyLock<Situation> =
        LazyLock::new(|| honor_choice_situation(Vec::new(), Some(tile("E")), Some(tile("S"))));

    // ドラ表示牌 E → ドラは S。赤5とは無関係に牌種だけで決まる通常ドラ。
    static NORMAL_DORA_SITUATION: LazyLock<Situation> =
        LazyLock::new(|| honor_choice_situation(ids(&[108]), None, None));

    // 場風・自風もドラ表示牌も持たない対照局面。役牌 / 通常ドラの両方の比較に使う。
    static HONOR_CHOICE_FREE_SITUATION: LazyLock<Situation> =
        LazyLock::new(|| honor_choice_situation(Vec::new(), None, None));

    // context の有無で next discard が変わる枝。1p を切って 5p をツモると
    // 123m456m789m 555p S W になり、2手目は孤立字牌 S と W のどちらを切るかだけになる。
    // 場風・自風もドラ表示牌も無ければ S を切り、S が自風の役牌になる場でも S がドラになる場でも
    // S を残して W へ変わる。
    fn honor_choice_branch() -> (TileType, TileType) {
        (tile("1p"), tile("5p"))
    }

    #[test]
    fn next_discard_reflects_value_honor_context() {
        let situation = &*VALUE_HONOR_SITUATION;
        let (discard, draw) = honor_choice_branch();

        let next = branch_next_discard(situation, discard, draw);
        assert_eq!(
            Some(&next),
            expected_next_discard(situation, discard, draw).as_ref(),
        );

        // 役牌は牌種と場風・自風だけで決まるので、赤5の曖昧さとは無関係に必ず反映される。
        assert_eq!(
            next.discarded_value_honor_count,
            u8::from(
                next.discard
                    .is_value_honor(situation.round_wind, situation.seat_wind)
            ),
        );
    }

    #[test]
    fn value_honor_context_changes_the_next_discard() {
        // context-free では S を切る枝が、役牌保護によって W へ変わることを固定する。
        let (discard, draw) = honor_choice_branch();

        let context_free = branch_next_discard(&HONOR_CHOICE_FREE_SITUATION, discard, draw);
        let with_context = branch_next_discard(&VALUE_HONOR_SITUATION, discard, draw);

        assert_ne!(
            with_context.discard, context_free.discard,
            "役牌 context が next discard を変える枝である必要がある"
        );
        assert!(
            context_free.discard.is_value_honor(
                VALUE_HONOR_SITUATION.round_wind,
                VALUE_HONOR_SITUATION.seat_wind
            ),
            "役牌 context で守られる牌が context-free では切られる枝である必要がある"
        );
    }

    #[test]
    fn next_discard_reflects_normal_dora_context() {
        let situation = &*NORMAL_DORA_SITUATION;
        let (discard, draw) = honor_choice_branch();

        let next = branch_next_discard(situation, discard, draw);
        assert_eq!(
            Some(&next),
            expected_next_discard(situation, discard, draw).as_ref(),
        );

        // 通常ドラは牌種から決まるので、仮想ツモ牌を切る候補でも 0 に潰さない。
        assert_eq!(
            next.discarded_dora_count,
            count_indicated_dora(next.discard, &situation.dora_indicators)
                + u8::from(next.discards_red_five),
        );
    }

    #[test]
    fn normal_dora_context_changes_the_next_discard() {
        // context-free では S を切る枝が、通常ドラ保護によって W へ変わることを固定する。
        let (discard, draw) = honor_choice_branch();

        let context_free = branch_next_discard(&HONOR_CHOICE_FREE_SITUATION, discard, draw);
        let with_context = branch_next_discard(&NORMAL_DORA_SITUATION, discard, draw);

        assert_ne!(
            with_context.discard, context_free.discard,
            "通常ドラ context が next discard を変える枝である必要がある"
        );
        assert_eq!(
            count_indicated_dora(context_free.discard, &NORMAL_DORA_SITUATION.dora_indicators),
            1,
            "通常ドラ context で守られる牌が context-free では切られる枝である必要がある"
        );
    }

    // 仮想ツモ牌をそのまま2手目に切る局面を含む case。
    //
    // 門前14枚 112233m 456p 78s EE S で S を切ると 9s が受け入れになり、9s を引いた後の最良打牌は
    // その 9s になる。ドラ表示牌 8s でドラは 9s なので、仮想ツモ牌を切る候補でも通常ドラが
    // 反映されることを確認できる。
    static DRAWN_TILE_DISCARD_CASE: LazyLock<Case> = LazyLock::new(|| {
        let hand = concealed_hand();
        let dora_indicators = ids(&[101]);
        let mut visible = hand.clone();
        visible.extend(dora_indicators.iter().copied());
        visible_case(
            &hand,
            FixedMeldCount::NONE,
            dora_indicators,
            None,
            None,
            visible,
        )
    });

    #[test]
    fn drawn_tile_type_keeps_indicated_dora_without_resolving_red_five() {
        // 仮想ツモ牌を2手目に切る候補でも、牌種から確定する通常ドラは反映し、赤5扱いにはしない。
        let case = &*DRAWN_TILE_DISCARD_CASE;

        let mut checked = 0;
        for candidate in &case.lookahead.candidates {
            for draw in &candidate.draws {
                let next = draw.next_discard.as_ref().expect("next discard exists");
                if next.discard != draw.draw {
                    continue;
                }
                assert!(!next.discards_red_five);
                assert_eq!(
                    next.discarded_dora_count,
                    count_indicated_dora(next.discard, &case.situation.dora_indicators),
                );
                checked += 1;
            }
        }
        assert!(checked > 0, "仮想ツモ牌を2手目に切る候補が必要");
    }

    #[test]
    fn drawn_dora_tile_discard_keeps_the_indicated_dora_count() {
        // ドラそのものを仮想ツモしてそのまま切る候補で、通常ドラを 0 に潰していないことを固定する。
        let case = &*DRAWN_TILE_DISCARD_CASE;
        let dora = tile("9s");

        let hit = case
            .lookahead
            .candidates
            .iter()
            .flat_map(|candidate| candidate.draws.iter())
            .filter(|draw| draw.draw == dora)
            .filter_map(|draw| draw.next_discard.as_ref())
            .find(|next| next.discard == dora)
            .expect("ドラを仮想ツモしてそのまま切る候補が必要");

        assert_eq!(hit.discarded_dora_count, 1);
        assert!(!hit.discards_red_five);
    }

    // ---- seen の扱い ----

    // 2手目の受け入れ残枚数が、期待する seen 集合から計算した値と一致することを全候補で確認する。
    //
    // `public_visible` は手牌以外に見えている枚数、`counts_candidate_discard` は2手目の打牌候補を
    // seen に数えるかどうか。1手目の打牌はどちらの経路でも seen に数える。
    // 戻り値は「1手目に切った牌が2手目の受け入れに現れた回数」で、山への復活検証が効いた件数。
    fn assert_lookahead_remaining(
        case: &Case,
        public_visible: &[(TileType, u8)],
        counts_candidate_discard: bool,
    ) -> usize {
        let public_visible_count = |tile: TileType| -> u8 {
            public_visible
                .iter()
                .find(|(seen, _)| *seen == tile)
                .map(|(_, count)| *count)
                .unwrap_or(0)
        };

        let mut first_discard_hits = 0;
        for candidate in &case.lookahead.candidates {
            for draw in &candidate.draws {
                let next = draw.next_discard.as_ref().expect("next discard exists");

                let mut after_next = case.situation.counts;
                after_next.remove(candidate.discard).unwrap();
                after_next.try_add(draw.draw).unwrap();
                after_next.remove(next.discard).unwrap();

                for accepted in &next.acceptance_after_discard.tiles {
                    let seen = after_next.count(accepted.tile)
                        + public_visible_count(accepted.tile)
                        + u8::from(accepted.tile == candidate.discard)
                        + u8::from(counts_candidate_discard && accepted.tile == next.discard);
                    assert_eq!(
                        accepted.remaining,
                        4u8.saturating_sub(seen),
                        "discard {:?} draw {:?} next {:?} tile {:?}",
                        candidate.discard,
                        draw.draw,
                        next.discard,
                        accepted.tile,
                    );
                    if accepted.tile == candidate.discard {
                        first_discard_hits += 1;
                    }
                }
            }
        }
        first_discard_hits
    }

    #[test]
    fn first_discard_stays_seen_without_visible_tiles() {
        // visible tiles が無い経路では2手目の打牌候補を seen に数えない既存 semantics を保ちつつ、
        // 1手目に切った牌だけは見え牌として残す。
        let hits = assert_lookahead_remaining(&CONCEALED_HAND_ONLY, &[], false);
        assert!(hits > 0, "1手目の打牌が2手目の受け入れに現れる局面が必要");
    }

    #[test]
    fn first_discard_stays_seen_with_visible_tiles() {
        let public_visible = [(tile("1m"), 2), (tile("5p"), 1), (tile("W"), 1)];
        let hits = assert_lookahead_remaining(&CONCEALED_WITH_VISIBLE, &public_visible, true);
        assert!(hits > 0, "1手目の打牌が2手目の受け入れに現れる局面が必要");
    }

    #[test]
    fn does_not_double_count_the_own_hand_in_visible_tiles() {
        // visible tiles が自分の手牌そのものだけなら、手牌以外に見えている牌は無い扱いになる。
        let hand = melded_hand();
        let case = visible_case(&hand, fixed(2), Vec::new(), None, None, hand.clone());

        let hits = assert_lookahead_remaining(&case, &[], true);
        assert!(hits > 0, "1手目の打牌が2手目の受け入れに現れる局面が必要");
    }

    #[test]
    fn empty_visible_tiles_match_the_fixed_meld_entry() {
        let case = hand_only_case(&melded_hand(), fixed(2), Vec::new(), None, None);

        assert_eq!(
            diagnose_lookahead_with_fixed_melds_and_visible_tiles(
                &case.situation.tiles,
                fixed(2),
                &[],
                None,
                None,
                &[],
                &case.situation.evaluations,
            ),
            case.lookahead,
        );
    }

    #[test]
    fn fixed_melds_keep_the_existing_effective_shanten() {
        let case = hand_only_case(&melded_hand(), fixed(2), Vec::new(), None, None);

        let mut checked = 0;
        for candidate in &case.lookahead.candidates {
            for draw in &candidate.draws {
                // 副露手では七対子・国士を復活させず、通常形のみの effective shanten になる。
                assert_eq!(draw.shanten_after_draw.concealed(), None);
                let next = draw.next_discard.as_ref().expect("next discard exists");
                assert_eq!(next.shanten_after_discard.concealed(), None);
                assert_eq!(
                    next.standard_iishanten_shape_after_discard,
                    IishantenShape::Unknown
                );
                for accepted in &next.acceptance_after_discard.tiles {
                    assert_eq!(accepted.shanten_after_draw.concealed(), None);
                }
                checked += 1;
            }
        }
        assert!(checked > 0);
    }

    // ---- 打牌選択用の weighted tenpai wait ----

    // 局面が持つ見え牌の有無に合わせて、選択用集計の入口を呼び分ける。
    fn metrics(situation: &Situation) -> Vec<Option<TenpaiWaitMetric>> {
        if situation.visible.is_empty() {
            tenpai_wait_metrics_with_fixed_melds(
                &situation.tiles,
                situation.fixed_meld_count,
                &situation.dora_indicators,
                situation.round_wind,
                situation.seat_wind,
                &situation.evaluations,
            )
        } else {
            tenpai_wait_metrics_with_fixed_melds_and_visible_tiles(
                &situation.tiles,
                situation.fixed_meld_count,
                &situation.dora_indicators,
                situation.round_wind,
                situation.seat_wind,
                &situation.visible,
                &situation.evaluations,
            )
        }
    }

    // 集計 helper を使わずに Σ(受け入れ残枚数 × テンパイ後の和了牌残枚数) を組み立てる。
    // 期待値を診断の生の値から作ることで、集計規則そのものを固定する。
    fn expected_metric(candidate: &DiscardLookaheadDiagnostic) -> TenpaiWaitMetric {
        let mut expected = TenpaiWaitMetric::default();
        for draw in &candidate.draws {
            let Some(next) = draw.next_discard.as_ref() else {
                continue;
            };
            if next.min_shanten_after_discard() != 0 {
                continue;
            }
            expected.weighted_remaining +=
                u32::from(draw.remaining) * u32::from(next.acceptance_total_remaining());
            expected.weighted_type_count +=
                u32::from(draw.remaining) * next.acceptance_type_count() as u32;
        }
        expected
    }

    // 1向聴を維持する打牌候補が複数ある門前14枚 12m 68m 444p 5p 789p 567s。
    // 打 5p は受け入れが最も広く、打 1m / 2m は 45p の両面を残してテンパイ後の待ちが広くなる。
    fn iishanten_wait_hand() -> Vec<TileId> {
        ids(&[0, 4, 20, 28, 48, 49, 50, 53, 60, 64, 68, 89, 92, 96])
    }

    static IISHANTEN_WAIT_CASE: LazyLock<Case> = LazyLock::new(|| {
        hand_only_case(
            &iishanten_wait_hand(),
            FixedMeldCount::NONE,
            Vec::new(),
            None,
            None,
        )
    });

    // 手牌以外に 3p 3枚・6p 3枚が見えている同じ局面。テンパイ後の待ちが実際に減る。
    static IISHANTEN_WAIT_WITH_VISIBLE: LazyLock<Case> = LazyLock::new(|| {
        let hand = iishanten_wait_hand();
        let mut visible = hand.clone();
        visible.extend(ids(&[44, 45, 46, 56, 57, 58]));
        visible_case(&hand, FixedMeldCount::NONE, Vec::new(), None, None, visible)
    });

    #[test]
    fn weighted_wait_is_computed_for_every_iishanten_candidate() {
        let case = &*IISHANTEN_WAIT_CASE;
        let metrics = metrics(&case.situation);

        assert_eq!(metrics.len(), case.situation.evaluations.len());
        let mut iishanten = 0;
        for (evaluation, metric) in case.situation.evaluations.iter().zip(metrics.iter()) {
            if evaluation.min_shanten_after_discard() == 1 {
                assert!(metric.is_some(), "1向聴候補 {:?}", evaluation.discard);
                iishanten += 1;
            } else {
                // 1向聴以外は前方探索そのものを行わないので None のままにする。
                assert_eq!(*metric, None, "非1向聴候補 {:?}", evaluation.discard);
            }
        }
        assert!(iishanten > 1, "1向聴候補が複数ある局面が必要");
    }

    #[test]
    fn weighted_wait_aggregates_the_branch_evaluations() {
        let case = &*IISHANTEN_WAIT_CASE;
        let metrics = metrics(&case.situation);

        let mut checked = 0;
        for (candidate, metric) in case.lookahead.candidates.iter().zip(metrics.iter()) {
            let Some(metric) = metric else {
                continue;
            };
            assert_eq!(
                *metric,
                expected_metric(candidate),
                "{:?}",
                candidate.discard
            );
            checked += 1;
        }
        assert!(checked > 1);
    }

    #[test]
    fn weighted_wait_matches_the_detailed_lookahead() {
        // 詳細診断から集計しても選択専用経路と同じ値になる。同じ枝を2回計算する必要はない。
        let case = &*IISHANTEN_WAIT_CASE;

        assert_eq!(
            tenpai_wait_metrics_from_lookahead(&case.situation.evaluations, &case.lookahead),
            metrics(&case.situation),
        );
    }

    #[test]
    fn weighted_wait_prefers_the_wider_tenpai_over_the_wider_acceptance() {
        // 受け入れが最も広い打牌より、テンパイ後の待ちが広い打牌の方が weighted wait が大きい。
        let case = &*IISHANTEN_WAIT_CASE;
        let metrics = metrics(&case.situation);

        let metric_of = |discard: TileType| {
            case.situation
                .evaluations
                .iter()
                .position(|evaluation| evaluation.discard == discard)
                .and_then(|index| metrics[index])
                .expect("1向聴候補の集計値がある")
        };
        let acceptance_of = |discard: TileType| {
            case.situation
                .evaluations
                .iter()
                .find(|evaluation| evaluation.discard == discard)
                .map(DiscardEvaluation::acceptance_total_remaining)
                .expect("打牌候補がある")
        };

        assert!(acceptance_of(tile("5p")) > acceptance_of(tile("1m")));
        assert!(
            metric_of(tile("1m")).weighted_remaining > metric_of(tile("5p")).weighted_remaining
        );
    }

    #[test]
    fn tenpai_hands_do_not_compute_the_weighted_wait() {
        // 最善向聴数がテンパイの局面では前方評価を計算しない。
        let situation = hand_only_situation(
            &ids(&[0, 4, 8, 12, 17, 20, 24, 28, 32, 36, 40, 89, 90, 68]),
            FixedMeldCount::NONE,
            Vec::new(),
            None,
            None,
        );
        assert_eq!(
            situation
                .evaluations
                .iter()
                .map(DiscardEvaluation::min_shanten_after_discard)
                .min(),
            Some(0)
        );

        assert!(metrics(&situation).iter().all(Option::is_none));
    }

    #[test]
    fn multi_shanten_hands_do_not_compute_the_weighted_wait() {
        // 2向聴・3向聴以上の局面では1向聴を維持する候補が無いので前方評価を計算しない。
        for hand in [
            ids(&[0, 4, 8, 12, 17, 20, 48, 53, 72, 76, 108, 112, 116, 120]),
            ids(&[0, 8, 20, 28, 48, 56, 68, 76, 88, 100, 108, 116, 124, 132]),
        ] {
            let situation =
                hand_only_situation(&hand, FixedMeldCount::NONE, Vec::new(), None, None);
            let best = situation
                .evaluations
                .iter()
                .map(DiscardEvaluation::min_shanten_after_discard)
                .min()
                .expect("打牌候補がある");
            assert!(best >= 2, "2向聴以上の局面が必要");

            assert!(metrics(&situation).iter().all(Option::is_none));
        }
    }

    #[test]
    fn a_single_iishanten_candidate_does_not_compute_the_weighted_wait() {
        // 1向聴を維持する候補が1件だけなら Shanten 比較で決着するので前方評価は不要。
        let case = &*IISHANTEN_WAIT_CASE;
        let single: Vec<_> = case
            .situation
            .evaluations
            .iter()
            .filter(|evaluation| evaluation.min_shanten_after_discard() != 1)
            .cloned()
            .chain(
                case.situation
                    .evaluations
                    .iter()
                    .find(|evaluation| evaluation.min_shanten_after_discard() == 1)
                    .cloned(),
            )
            .collect();

        let metrics = tenpai_wait_metrics_with_fixed_melds(
            &case.situation.tiles,
            FixedMeldCount::NONE,
            &[],
            None,
            None,
            &single,
        );
        assert!(metrics.iter().all(Option::is_none));
    }

    #[test]
    fn visible_tiles_reduce_the_weighted_wait() {
        // テンパイ後の待ち牌が他家に見えている分だけ、weighted wait が実際に減る。
        let hand_only = metrics(&IISHANTEN_WAIT_CASE.situation);
        let with_visible = metrics(&IISHANTEN_WAIT_WITH_VISIBLE.situation);

        let mut reduced = 0;
        for (without, with) in hand_only.iter().zip(with_visible.iter()) {
            let (Some(without), Some(with)) = (without, with) else {
                continue;
            };
            assert!(with.weighted_remaining <= without.weighted_remaining);
            if with.weighted_remaining < without.weighted_remaining {
                reduced += 1;
            }
        }
        assert!(reduced > 0, "見え牌で待ちが減る候補が必要");
    }

    #[test]
    fn dead_wait_tenpai_branches_contribute_zero() {
        // 待ちがすべて見えているテンパイへ進む枝は寄与 0。計算していない None とは区別する。
        //
        // 123m456m789m 99s E S + ツモ W。E を切って S を引くと 3面子 + 99s + SS + W になり、
        // 2手目は W を切って 9s / S のシャンポン待ちテンパイになる。9s と S を残り全部見え牌に
        // しておくと、そのテンパイの和了牌は1枚も残らない。
        let hand = ids(&[0, 4, 8, 12, 17, 20, 24, 28, 32, 104, 105, 108, 112]);
        let mut tiles = hand.clone();
        tiles.push(ids(&[116])[0]);
        let mut visible = tiles.clone();
        visible.extend(ids(&[106, 107, 113, 114]));
        let case = visible_case(
            &tiles,
            FixedMeldCount::NONE,
            Vec::new(),
            None,
            None,
            visible,
        );
        let metrics = metrics(&case.situation);

        let mut dead = 0;
        for (candidate, metric) in case.lookahead.candidates.iter().zip(metrics.iter()) {
            let Some(metric) = metric else {
                continue;
            };
            for draw in &candidate.draws {
                let Some(next) = draw.next_discard.as_ref() else {
                    continue;
                };
                if next.min_shanten_after_discard() == 0 && next.acceptance_total_remaining() == 0 {
                    dead += 1;
                }
            }
            assert_eq!(*metric, expected_metric(candidate));
        }
        assert!(dead > 0, "死にテンへ進む枝がある局面が必要");
    }

    #[test]
    fn discarded_tiles_are_not_returned_to_the_wall() {
        // 集計対象の局面でも、1手目・2手目に切った牌を山へ戻さない既存 seen 扱いを維持する。
        // 残枚数の検証は既存の枝と同じ helper を使い、集計はその残枚数から組み立てる。
        let _ = assert_lookahead_remaining(&IISHANTEN_WAIT_CASE, &[], false);
        let _ = assert_lookahead_remaining(
            &IISHANTEN_WAIT_WITH_VISIBLE,
            &[(tile("3p"), 3), (tile("6p"), 3)],
            true,
        );
    }

    #[test]
    fn fixed_melds_keep_the_effective_shanten_semantics_in_the_weighted_wait() {
        // 副露済み手牌でも既存 EffectiveShanten のまま集計し、詳細診断と同じ値になる。
        let hand = ids(&[0, 4, 8, 36, 40, 60, 64, 89]);
        let case = hand_only_case(&hand, fixed(2), Vec::new(), None, None);

        let metrics = metrics(&case.situation);
        assert!(metrics.iter().any(Option::is_some), "1向聴候補が必要");
        assert_eq!(
            metrics,
            tenpai_wait_metrics_from_lookahead(&case.situation.evaluations, &case.lookahead),
        );

        for (candidate, metric) in case.lookahead.candidates.iter().zip(metrics.iter()) {
            if metric.is_none() {
                continue;
            }
            for draw in &candidate.draws {
                assert_eq!(draw.shanten_after_draw.concealed(), None);
                let next = draw.next_discard.as_ref().expect("next discard exists");
                assert_eq!(next.shanten_after_discard.concealed(), None);
            }
        }
    }

    #[test]
    fn red_five_handling_matches_the_detailed_lookahead() {
        // 赤5を含む物理牌でも、選択用集計は詳細診断と同じ枝評価を共有する。
        let mut hand = iishanten_wait_hand();
        // 黒5s を赤5s へ置き換える。
        let position = hand.iter().position(|tile| *tile == ids(&[89])[0]).unwrap();
        hand[position] = ids(&[88])[0];
        let case = hand_only_case(&hand, FixedMeldCount::NONE, Vec::new(), None, None);

        assert!(hand.iter().any(|tile| tile.is_red()));
        let metrics = metrics(&case.situation);
        assert!(metrics.iter().any(Option::is_some));
        assert_eq!(
            metrics,
            tenpai_wait_metrics_from_lookahead(&case.situation.evaluations, &case.lookahead),
        );
    }

    #[test]
    fn empty_visible_tiles_match_the_fixed_meld_weighted_wait() {
        let case = &*IISHANTEN_WAIT_CASE;

        assert_eq!(
            tenpai_wait_metrics_with_fixed_melds_and_visible_tiles(
                &case.situation.tiles,
                FixedMeldCount::NONE,
                &[],
                None,
                None,
                &[],
                &case.situation.evaluations,
            ),
            metrics(&case.situation),
        );
    }

    #[test]
    fn weighted_wait_from_a_mismatched_lookahead_is_absent() {
        // 候補集合と対応しない診断を渡された場合は推測せず None にする。
        let case = &*IISHANTEN_WAIT_CASE;

        assert!(
            tenpai_wait_metrics_from_lookahead(
                &case.situation.evaluations,
                &LookaheadDiagnostic::default(),
            )
            .iter()
            .all(Option::is_none)
        );
    }

    #[test]
    fn accessors_read_the_stored_next_evaluation() {
        let draw = &CONCEALED_HAND_ONLY.lookahead.candidates[0].draws[0];
        let next = draw.next_discard.as_ref().unwrap();

        assert_eq!(draw.next_discard_tile(), Some(next.discard));
        assert_eq!(
            draw.next_min_shanten(),
            Some(next.min_shanten_after_discard())
        );
        assert_eq!(
            draw.next_acceptance_total_remaining(),
            Some(next.acceptance_total_remaining())
        );
        assert_eq!(
            draw.next_acceptance_type_count(),
            Some(next.acceptance_type_count())
        );
        assert_eq!(
            draw.next_standard_iishanten_shape(),
            Some(next.standard_iishanten_shape_after_discard)
        );
    }

    #[test]
    fn does_not_modify_the_input_tiles() {
        let tiles = melded_hand();
        let before = tiles.clone();
        let evaluations = evaluate_discards_from_tiles_with_fixed_melds_and_context(
            &tiles,
            fixed(2),
            &[],
            None,
            None,
        );
        let _ =
            diagnose_lookahead_with_fixed_melds(&tiles, fixed(2), &[], None, None, &evaluations);
        assert_eq!(tiles, before);
    }
}
