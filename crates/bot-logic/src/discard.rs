use crate::acceptance::{
    EffectiveAcceptance, additional_seen, calculate_acceptance_with_fixed_melds_and_seen,
};
use crate::iishanten::{IishantenShape, classify_standard_iishanten_shape_with_standard_shanten};
use crate::selection::{
    DiscardSelectionCandidate, ForwardMetrics, NextAcceptanceMetric, TenpaiWaitMetric,
    best_discard_selection_index_with_forward_metrics, compare_discard_selection_candidates,
};
use crate::shanten::{EffectiveShanten, FixedMeldCount};
use crate::tile::{TileId, TileType, count_indicated_dora};
use crate::tile_counts::TileCounts;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscardEvaluation {
    pub discard: TileType,
    pub count_before_discard: u8,
    /// 打牌後の向聴数。副露済み面子数が 0 なら [`EffectiveShanten::Concealed`] で門前どおり
    /// 七対子・国士を含み、1 組以上なら [`EffectiveShanten::Melded`] で通常形のみになる。
    /// 副露時に意味を持たない七対子・国士の値をこの型へ詰めることはしない。
    pub shanten_after_discard: EffectiveShanten,
    /// 打牌後の受け入れ。`shanten_after_discard` と同じ副露済み面子数で求める。
    pub acceptance_after_discard: EffectiveAcceptance,
    pub shape_penalty: i16,
    pub floating_tile_value: i16,
    pub discarded_dora_count: u8,
    pub discarded_value_honor_count: u8,
    pub discards_red_five: bool,
    /// この打牌が純粋な手牌構造上の孤立単騎牌を切るかどうか。判定は手牌構造だけに基づき、
    /// visible tiles や特殊牌情報の影響を受けない。[`floating_tile_value_breakdown_for_discard`]
    /// の `is_isolated` と同じ契約で、評価生成時にその helper を一度だけ呼んで
    /// `floating_tile_value` と同時に設定する。
    ///
    /// 多向聴時の比較軸 [`DiscardComparisonReason::IsolatedTile`] の優先対象判定
    /// (`isolated_tile_priority_eligible`) は、これに加えて孤立ドラ・孤立赤5を
    /// 除外する。孤立牌であることと比較上の優先対象であることを混同しないこと。
    pub discards_isolated_tile: bool,
    /// 打牌後13枚の通常形一向聴の形分類。通常形一向聴でなければ [`IishantenShape::Unknown`]。
    /// 評価生成時に打牌後 counts から一度だけ算出し、比較と診断ログで再利用する。
    ///
    /// 分類器は門前13枚専用なので、副露済み面子が 1 組以上ある手牌は分類せず常に
    /// [`IishantenShape::Unknown`] にする。副露手を門前13枚分類器へ押し込まない。
    pub standard_iishanten_shape_after_discard: IishantenShape,
}

impl DiscardEvaluation {
    pub fn min_shanten_after_discard(&self) -> i8 {
        self.shanten_after_discard.min()
    }

    pub fn acceptance_type_count(&self) -> usize {
        self.acceptance_after_discard.tiles.len()
    }

    pub fn acceptance_total_remaining(&self) -> u8 {
        self.acceptance_after_discard.total_remaining()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ShapeBreakdown {
    pub breaks_pair: bool,
    pub breaks_triplet: bool,
    pub breaks_honor_triplet: bool,
    pub breaks_ryanmen: bool,
    pub breaks_kanchan: bool,
    pub breaks_penchan: bool,
    pub breaks_sequence: bool,
    pub adjacent_count: u8,
    pub same_type_count: u8,
    pub preserves_sequence_after_discard: bool,
    pub preserves_ryanmen_after_discard: bool,
    pub preserves_pair_after_discard: bool,
}

pub fn shape_breakdown_for_discard(counts: &TileCounts, discard: TileType) -> ShapeBreakdown {
    let same_type_count = counts.count(discard);
    if same_type_count == 0 {
        return ShapeBreakdown::default();
    }

    let mut breakdown = ShapeBreakdown {
        same_type_count,
        ..ShapeBreakdown::default()
    };
    if same_type_count >= 2 {
        breakdown.breaks_pair = true;
    }
    if same_type_count >= 3 {
        breakdown.breaks_triplet = true;
        if discard.is_honor() {
            breakdown.breaks_honor_triplet = true;
        }
    }
    breakdown.preserves_pair_after_discard = same_type_count >= 3;

    let Some(number) = discard.number() else {
        return breakdown;
    };

    let base = discard.raw() - (number - 1);
    let has = |n: i8| -> bool {
        if !(1..=9).contains(&n) {
            return false;
        }
        let tile = TileType::new(base + (n as u8 - 1)).expect("same-suit tile is valid");
        counts.count(tile) > 0
    };

    let d = number as i8;

    for delta in [-2i8, -1, 1, 2] {
        if has(d + delta) {
            breakdown.adjacent_count += 1;
        }
    }

    breakdown.breaks_sequence =
        (has(d - 2) && has(d - 1)) || (has(d - 1) && has(d + 1)) || (has(d + 1) && has(d + 2));

    for a in [d - 1, d] {
        if has(a) && has(a + 1) {
            if a == 1 || a + 1 == 9 {
                breakdown.breaks_penchan = true;
            } else {
                breakdown.breaks_ryanmen = true;
            }
        }
    }

    breakdown.breaks_kanchan = has(d - 2) || has(d + 2);

    breakdown.preserves_sequence_after_discard = preserves_sequence_after_discard(counts, discard);
    breakdown.preserves_ryanmen_after_discard = preserves_ryanmen_after_discard(counts, discard);

    breakdown
}

fn preserves_sequence_after_discard(counts: &TileCounts, discard: TileType) -> bool {
    if counts.count(discard) < 2 {
        return false;
    }
    let Some(number) = discard.number() else {
        return false;
    };

    let mut after = *counts;
    if after.remove(discard).is_err() {
        return false;
    }

    let base = discard.raw() - (number - 1);
    let has = |n: i8| -> bool {
        if !(1..=9).contains(&n) {
            return false;
        }
        let tile = TileType::new(base + (n as u8 - 1)).expect("same-suit tile is valid");
        after.count(tile) > 0
    };

    let d = number as i8;
    (has(d - 2) && has(d - 1)) || (has(d - 1) && has(d + 1)) || (has(d + 1) && has(d + 2))
}

fn preserves_ryanmen_after_discard(counts: &TileCounts, discard: TileType) -> bool {
    if counts.count(discard) < 2 {
        return false;
    }
    let Some(number) = discard.number() else {
        return false;
    };

    let mut after = *counts;
    if after.remove(discard).is_err() {
        return false;
    }

    let base = discard.raw() - (number - 1);
    let has = |n: i8| -> bool {
        if !(1..=9).contains(&n) {
            return false;
        }
        let tile = TileType::new(base + (n as u8 - 1)).expect("same-suit tile is valid");
        after.count(tile) > 0
    };

    let d = number as i8;
    for a in [d - 1, d] {
        if has(a) && has(a + 1) && a != 1 && a + 1 != 9 {
            return true;
        }
    }
    false
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PairContext {
    pub pair_like_type_count: u8,
    pub other_pair_like_type_count: u8,
    pub is_only_pair_candidate: bool,
    pub leaves_pair_after_discard: bool,
}

pub fn pair_context_for_discard(counts: &TileCounts, discard: TileType) -> PairContext {
    let count_before_discard = counts.count(discard);
    if count_before_discard == 0 {
        return PairContext::default();
    }

    let mut pair_like_type_count = 0u8;
    let mut other_pair_like_type_count = 0u8;
    for tile in TileType::all() {
        if counts.count(tile) >= 2 {
            pair_like_type_count += 1;
            if tile != discard {
                other_pair_like_type_count += 1;
            }
        }
    }

    PairContext {
        pair_like_type_count,
        other_pair_like_type_count,
        is_only_pair_candidate: count_before_discard >= 2 && other_pair_like_type_count == 0,
        leaves_pair_after_discard: count_before_discard >= 3,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct HandShapeSummary {
    pub sequence_count: u8,
    pub triplet_count: u8,
    pub pair_like_type_count: u8,
    pub ryanmen_taatsu_count: u8,
    pub kanchan_taatsu_count: u8,
    pub penchan_taatsu_count: u8,
    pub isolated_tile_type_count: u8,
    pub estimated_block_count: u8,
}

fn is_isolated_tile(counts: &TileCounts, tile: TileType) -> bool {
    let same_type_count = counts.count(tile);
    if same_type_count == 0 || same_type_count >= 2 {
        return false;
    }

    let Some(number) = tile.number() else {
        return true;
    };

    let base = tile.raw() - (number - 1);
    let has = |n: i8| -> bool {
        if !(1..=9).contains(&n) {
            return false;
        }
        let neighbor = TileType::new(base + (n as u8 - 1)).expect("same-suit tile is valid");
        counts.count(neighbor) > 0
    };

    let d = number as i8;
    for delta in [-2i8, -1, 1, 2] {
        if has(d + delta) {
            return false;
        }
    }
    true
}

pub fn hand_shape_summary(counts: &TileCounts) -> HandShapeSummary {
    let mut summary = HandShapeSummary::default();

    for tile in TileType::all() {
        let same_type_count = counts.count(tile);
        if same_type_count == 0 {
            continue;
        }
        if same_type_count >= 3 {
            summary.triplet_count += 1;
        }
        if same_type_count >= 2 {
            summary.pair_like_type_count += 1;
        }
        if is_isolated_tile(counts, tile) {
            summary.isolated_tile_type_count += 1;
        }
    }

    for suit_base in [0u8, 9, 18] {
        let has = |n: i8| -> bool {
            if !(1..=9).contains(&n) {
                return false;
            }
            let tile = TileType::new(suit_base + (n as u8 - 1)).expect("same-suit tile is valid");
            counts.count(tile) > 0
        };

        for n in 1..=9i8 {
            if !has(n) {
                continue;
            }
            if has(n + 1) && has(n + 2) {
                summary.sequence_count += 1;
            }
            if has(n + 1) {
                if n == 1 || n + 1 == 9 {
                    summary.penchan_taatsu_count += 1;
                } else {
                    summary.ryanmen_taatsu_count += 1;
                }
            }
            if has(n + 2) {
                summary.kanchan_taatsu_count += 1;
            }
        }
    }

    summary.estimated_block_count = summary.sequence_count
        + summary.triplet_count
        + summary.pair_like_type_count
        + summary.ryanmen_taatsu_count
        + summary.kanchan_taatsu_count
        + summary.penchan_taatsu_count;

    summary
}

/// 打牌前後のブロック数の内訳。
///
/// `before` / `after` は [`hand_shape_summary`] どおり concealed hand の形だけを見る。副露済み
/// 面子は concealed tiles から消えているため含まれない。副露済み面子数の補正は
/// `leaves_under_five_blocks` にだけ効き、`before` / `after` の意味は変えない。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct DiscardBlockContext {
    pub before: HandShapeSummary,
    pub after: HandShapeSummary,
    pub reduces_estimated_block_count: bool,
    /// 打牌後の実効ブロック数(concealed の推定ブロック数 + 副露済み面子数)が5未満かどうか。
    pub leaves_under_five_blocks: bool,
}

pub fn discard_block_context(counts: &TileCounts, discard: TileType) -> DiscardBlockContext {
    discard_block_context_with_fixed_melds(counts, discard, FixedMeldCount::NONE)
}

/// 副露済み面子数を考慮して打牌前後のブロック文脈を求める。
///
/// concealed tiles だけを見ると完成済み副露が消えているため、ブロック不足の判定
/// (`leaves_under_five_blocks`) には副露済み面子数を加えた実効ブロック数を使う。
/// `reduces_estimated_block_count` は打牌前後の差なので副露済み面子数の影響を受けない。
/// `fixed_meld_count == FixedMeldCount::NONE` では [`discard_block_context`] と一致する。
pub fn discard_block_context_with_fixed_melds(
    counts: &TileCounts,
    discard: TileType,
    fixed_meld_count: FixedMeldCount,
) -> DiscardBlockContext {
    if counts.count(discard) == 0 {
        return DiscardBlockContext::default();
    }

    let before = hand_shape_summary(counts);

    let mut after_counts = *counts;
    if after_counts.remove(discard).is_err() {
        return DiscardBlockContext::default();
    }
    let after = hand_shape_summary(&after_counts);

    let effective_block_count_after = after
        .estimated_block_count
        .saturating_add(fixed_meld_count.get());

    DiscardBlockContext {
        before,
        after,
        reduces_estimated_block_count: after.estimated_block_count < before.estimated_block_count,
        leaves_under_five_blocks: effective_block_count_after < 5,
    }
}

const VALUE_HONOR_TRIPLET_PENALTY: i16 = 15;

pub fn shape_penalty_for_discard(counts: &TileCounts, discard: TileType) -> i16 {
    shape_penalty_for_discard_with_fixed_melds(counts, discard, FixedMeldCount::NONE)
}

/// 副露済み面子数を考慮した形ペナルティ。
///
/// ペナルティの各項は concealed hand の形だけを見る既存のままで、副露済み面子数は
/// ブロック不足判定 ([`discard_block_context_with_fixed_melds`]) にだけ反映する。
/// `fixed_meld_count == FixedMeldCount::NONE` では [`shape_penalty_for_discard`] と一致する。
pub fn shape_penalty_for_discard_with_fixed_melds(
    counts: &TileCounts,
    discard: TileType,
    fixed_meld_count: FixedMeldCount,
) -> i16 {
    let breakdown = shape_breakdown_for_discard(counts, discard);
    shape_penalty_for_discard_impl(counts, discard, &breakdown, false, fixed_meld_count)
}

pub fn shape_penalty_for_discard_with_context(
    counts: &TileCounts,
    discard: TileType,
    round_wind: Option<TileType>,
    seat_wind: Option<TileType>,
) -> i16 {
    shape_penalty_for_discard_with_fixed_melds_and_context(
        counts,
        discard,
        FixedMeldCount::NONE,
        round_wind,
        seat_wind,
    )
}

/// 副露済み面子数と場風・自風を考慮した形ペナルティ。
///
/// `fixed_meld_count == FixedMeldCount::NONE` では [`shape_penalty_for_discard_with_context`]
/// と一致する。
pub fn shape_penalty_for_discard_with_fixed_melds_and_context(
    counts: &TileCounts,
    discard: TileType,
    fixed_meld_count: FixedMeldCount,
    round_wind: Option<TileType>,
    seat_wind: Option<TileType>,
) -> i16 {
    let breakdown = shape_breakdown_for_discard(counts, discard);
    let breaks_value_honor_triplet =
        breakdown.breaks_triplet && discard.is_value_honor(round_wind, seat_wind);
    shape_penalty_for_discard_impl(
        counts,
        discard,
        &breakdown,
        breaks_value_honor_triplet,
        fixed_meld_count,
    )
}

fn shape_penalty_for_discard_impl(
    counts: &TileCounts,
    discard: TileType,
    breakdown: &ShapeBreakdown,
    breaks_value_honor_triplet: bool,
    fixed_meld_count: FixedMeldCount,
) -> i16 {
    let mut penalty = 0i16;
    if breakdown.breaks_sequence {
        penalty += 40;
    }
    if breakdown.breaks_ryanmen {
        penalty += 30;
    }
    if breakdown.breaks_pair {
        penalty += 20;
    }
    if breakdown.breaks_kanchan {
        penalty += 12;
    }
    if breakdown.breaks_penchan {
        penalty += 8;
    }
    penalty += i16::from(breakdown.adjacent_count) * 3;
    if breakdown.same_type_count >= 3 {
        penalty += 10;
    }
    if breakdown.breaks_triplet {
        penalty += 35;
    }
    if breakdown.breaks_honor_triplet {
        penalty += 20;
    }

    if breakdown.preserves_sequence_after_discard {
        penalty -= 15;
    }
    if breakdown.preserves_ryanmen_after_discard {
        penalty -= 15;
    }
    let preserves_shape =
        breakdown.preserves_sequence_after_discard || breakdown.preserves_ryanmen_after_discard;
    if breakdown.preserves_pair_after_discard {
        if !breakdown.breaks_honor_triplet {
            penalty -= 12;
        }
    } else if breakdown.same_type_count == 2 && preserves_shape {
        penalty -= 8;
    }

    let pair_context = pair_context_for_discard(counts, discard);
    if pair_context.is_only_pair_candidate
        && !pair_context.leaves_pair_after_discard
        && !preserves_shape
    {
        penalty += 8;
    }
    if breakdown.same_type_count == 2 && pair_context.other_pair_like_type_count >= 1 {
        penalty -= 6;
    }
    if breakdown.same_type_count == 2 && pair_context.pair_like_type_count >= 3 {
        penalty -= 4;
    }

    let block_context = discard_block_context_with_fixed_melds(counts, discard, fixed_meld_count);
    if block_context.reduces_estimated_block_count {
        if block_context.leaves_under_five_blocks {
            penalty += 10;
        } else {
            penalty += 4;
        }
    }

    if breaks_value_honor_triplet {
        penalty += VALUE_HONOR_TRIPLET_PENALTY;
    }

    penalty.max(0)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct FloatingTileValue {
    pub value: i16,
    pub is_isolated: bool,
}

pub fn floating_tile_value_breakdown_for_discard(
    counts: &TileCounts,
    discard: TileType,
) -> FloatingTileValue {
    let same_type_count = counts.count(discard);
    if same_type_count != 1 {
        return FloatingTileValue::default();
    }

    let Some(number) = discard.number() else {
        return FloatingTileValue {
            value: 0,
            is_isolated: true,
        };
    };

    let base = discard.raw() - (number - 1);
    let has = |n: i8| -> bool {
        if !(1..=9).contains(&n) {
            return false;
        }
        let tile = TileType::new(base + (n as u8 - 1)).expect("same-suit tile is valid");
        counts.count(tile) > 0
    };

    let d = number as i8;
    for delta in [-2i8, -1, 1, 2] {
        if has(d + delta) {
            return FloatingTileValue::default();
        }
    }

    let value = i16::from(number.min(10 - number));
    FloatingTileValue {
        value,
        is_isolated: true,
    }
}

pub fn floating_tile_value_for_discard(counts: &TileCounts, discard: TileType) -> i16 {
    floating_tile_value_breakdown_for_discard(counts, discard).value
}

pub fn select_best_discard(counts: &TileCounts) -> Option<DiscardEvaluation> {
    select_best(evaluate_discards(counts))
}

pub fn select_best_discard_with_visible_tiles(
    counts: &TileCounts,
    visible_tiles: &[TileId],
) -> Option<DiscardEvaluation> {
    select_best(evaluate_discards_with_visible_tiles(counts, visible_tiles))
}

pub fn select_best_discard_from_tiles(tiles: &[TileId]) -> Option<DiscardEvaluation> {
    select_best_discard_from_tiles_with_dora(tiles, &[])
}

pub fn select_best_discard_from_tiles_with_dora(
    tiles: &[TileId],
    dora_indicators: &[TileId],
) -> Option<DiscardEvaluation> {
    select_best(evaluate_discards_from_tiles_with_dora(
        tiles,
        dora_indicators,
    ))
}

pub fn select_best_discard_from_tiles_with_context(
    tiles: &[TileId],
    dora_indicators: &[TileId],
    round_wind: Option<TileType>,
    seat_wind: Option<TileType>,
) -> Option<DiscardEvaluation> {
    select_best(evaluate_discards_from_tiles_with_context(
        tiles,
        dora_indicators,
        round_wind,
        seat_wind,
    ))
}

pub fn select_best_discard_from_tiles_with_visible_tiles(
    tiles: &[TileId],
    dora_indicators: &[TileId],
    round_wind: Option<TileType>,
    seat_wind: Option<TileType>,
    visible_tiles: &[TileId],
) -> Option<DiscardEvaluation> {
    select_best(evaluate_discards_from_tiles_with_visible_tiles(
        tiles,
        dora_indicators,
        round_wind,
        seat_wind,
        visible_tiles,
    ))
}

fn best_discard_index(evaluations: &[DiscardEvaluation]) -> Option<usize> {
    let mut best: Option<usize> = None;
    for (index, candidate) in evaluations.iter().enumerate() {
        match best {
            Some(best_index) if !is_better_discard(candidate, &evaluations[best_index]) => {}
            _ => best = Some(index),
        }
    }
    best
}

pub(crate) fn select_best(mut evaluations: Vec<DiscardEvaluation>) -> Option<DiscardEvaluation> {
    let index = best_discard_index(&evaluations)?;
    Some(evaluations.swap_remove(index))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiscardComparisonReason {
    Shanten,
    IsolatedTile,
    IsolatedHonor,
    /// 打点込みの前方評価。Σ(残枚数 × そのテンパイの Σ(和了牌残枚数 × 支払い合計))。
    ///
    /// 現在打牌の比較では1手目の物理牌 variant 残枚数で重み付けし、2手目の打牌候補の比較では
    /// そのテンパイ自身の値をそのまま比べる。両側の値が確定している場合だけ決着させる。
    WeightedProspectiveValue,
    /// 1向聴限定の前方評価。Σ(受け入れ残枚数 × そのテンパイの和了牌残枚数)。
    WeightedTenpaiWaitRemaining,
    /// 1向聴限定の前方評価。Σ(受け入れ残枚数 × そのテンパイの待ち牌種類数)。
    WeightedTenpaiWaitTypeCount,
    /// 2向聴以上限定。Σ(受け入れ残枚数 × 次打牌後の受け入れ残枚数)。
    WeightedNextAcceptanceRemaining,
    /// 2向聴以上限定。Σ(受け入れ残枚数 × 次打牌後の受け入れ牌種類数)。
    WeightedNextAcceptanceTypeCount,
    AcceptanceRemaining,
    AcceptanceTypeCount,
    /// 七対子テンパイ限定。単騎待ち牌の固定順位。
    ChiitoitsuWaitQuality,
    IishantenShape,
    ShapePenalty,
    FloatingTileValue,
    Dora,
    ValueHonor,
    RedFive,
    StableOrder,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DiscardComparison {
    pub candidate_is_better: bool,
    pub reason: DiscardComparisonReason,
}

/// 1手の打牌評価だけで打牌候補を比較する。
///
/// 前方評価を持つ selection 経路は [`crate::selection::compare_discard_selection_candidates`] を
/// 使う。比較順の前半 ([`compare_discard_before_acceptance`]) と後半
/// ([`compare_discard_from_acceptance`]) は selection 経路と共有し、比較順を二重定義しない。
pub fn compare_discard_evaluations(
    candidate: &DiscardEvaluation,
    current_best: &DiscardEvaluation,
) -> DiscardComparison {
    compare_discard_before_acceptance(candidate, current_best)
        .unwrap_or_else(|| compare_discard_from_acceptance(candidate, current_best))
}

// 比較順のうち、向聴数と多向聴限定の孤立牌比較まで。決着しなければ `None` を返す。
// `None` を返した時点で両候補の最小向聴数は等しい。
pub(crate) fn compare_discard_before_acceptance(
    candidate: &DiscardEvaluation,
    current_best: &DiscardEvaluation,
) -> Option<DiscardComparison> {
    let candidate_shanten = candidate.min_shanten_after_discard();
    let best_shanten = current_best.min_shanten_after_discard();
    if candidate_shanten != best_shanten {
        return Some(DiscardComparison {
            candidate_is_better: candidate_shanten < best_shanten,
            reason: DiscardComparisonReason::Shanten,
        });
    }

    if let Some(comparison) = compare_isolated_tile_discard(candidate, current_best) {
        return Some(comparison);
    }

    compare_isolated_honor_discard(candidate, current_best)
}

// 比較順のうち、受け入れ以降。最後は StableOrder になるので必ず決着する。
pub(crate) fn compare_discard_from_acceptance(
    candidate: &DiscardEvaluation,
    current_best: &DiscardEvaluation,
) -> DiscardComparison {
    let candidate_remaining = candidate.acceptance_total_remaining();
    let best_remaining = current_best.acceptance_total_remaining();
    if candidate_remaining != best_remaining {
        return DiscardComparison {
            candidate_is_better: candidate_remaining > best_remaining,
            reason: DiscardComparisonReason::AcceptanceRemaining,
        };
    }

    let candidate_type_count = candidate.acceptance_type_count();
    let best_type_count = current_best.acceptance_type_count();
    if candidate_type_count != best_type_count {
        return DiscardComparison {
            candidate_is_better: candidate_type_count > best_type_count,
            reason: DiscardComparisonReason::AcceptanceTypeCount,
        };
    }

    if let Some(comparison) = compare_chiitoitsu_wait_quality(candidate, current_best) {
        return comparison;
    }

    if let Some(comparison) = compare_standard_iishanten_shape(candidate, current_best) {
        return comparison;
    }

    if candidate.shape_penalty != current_best.shape_penalty {
        return DiscardComparison {
            candidate_is_better: candidate.shape_penalty < current_best.shape_penalty,
            reason: DiscardComparisonReason::ShapePenalty,
        };
    }

    if candidate.floating_tile_value != current_best.floating_tile_value {
        return DiscardComparison {
            candidate_is_better: candidate.floating_tile_value < current_best.floating_tile_value,
            reason: DiscardComparisonReason::FloatingTileValue,
        };
    }

    if candidate.discarded_dora_count != current_best.discarded_dora_count {
        return DiscardComparison {
            candidate_is_better: candidate.discarded_dora_count < current_best.discarded_dora_count,
            reason: DiscardComparisonReason::Dora,
        };
    }

    if candidate.discarded_value_honor_count != current_best.discarded_value_honor_count {
        return DiscardComparison {
            candidate_is_better: candidate.discarded_value_honor_count
                < current_best.discarded_value_honor_count,
            reason: DiscardComparisonReason::ValueHonor,
        };
    }

    if candidate.discards_red_five != current_best.discards_red_five {
        return DiscardComparison {
            candidate_is_better: !candidate.discards_red_five && current_best.discards_red_five,
            reason: DiscardComparisonReason::RedFive,
        };
    }

    DiscardComparison {
        candidate_is_better: false,
        reason: DiscardComparisonReason::StableOrder,
    }
}

// 候補単独で「孤立牌の優先対象」かどうかを判定する。手牌構造上の孤立牌
// (discards_isolated_tile) であっても、孤立ドラ・孤立赤5は優先対象外とし、
// これらの温存は後続の Dora / RedFive 比較へ委ねる。孤立役牌は優先対象へ含め、
// 非ドラの孤立字牌を孤立数牌より先に切れるようにする。役牌の温存判断は後続の
// ValueHonor 比較へ委ねる。
//
// 比較相手に依存しない候補固有の値なので、辞書順比較の推移律を壊さない。
fn isolated_tile_priority_eligible(evaluation: &DiscardEvaluation) -> bool {
    evaluation.discards_isolated_tile
        && evaluation.discarded_dora_count == 0
        && !evaluation.discards_red_five
}

// 多向聴時に限り、同じ向聴数を維持する候補間で、通常孤立牌を切る打牌を搭子候補を壊す打牌より
// 優先する限定的な比較軸。以下の条件をすべて満たす場合だけ決着させ、それ以外は None を返して
// 後続の受け入れ以下の既存比較へ委ねる。
//
// - 両候補の最小向聴数が等しい
// - 最小向聴数が2以上（テンパイ・一向聴には適用しない）
// - isolated_tile_priority_eligible() の値が異なる
//
// 各候補単独で通常孤立牌かどうかを判定する。孤立ドラ・孤立役牌・孤立赤5は優先対象外。
// 比較相手に依存する条件を持たず、辞書順比較の推移律を維持する。
fn compare_isolated_tile_discard(
    candidate: &DiscardEvaluation,
    current_best: &DiscardEvaluation,
) -> Option<DiscardComparison> {
    let candidate_shanten = candidate.min_shanten_after_discard();
    let best_shanten = current_best.min_shanten_after_discard();
    if candidate_shanten != best_shanten {
        return None;
    }
    if candidate_shanten < 2 {
        return None;
    }

    let candidate_eligible = isolated_tile_priority_eligible(candidate);
    let best_eligible = isolated_tile_priority_eligible(current_best);
    if candidate_eligible == best_eligible {
        return None;
    }

    Some(DiscardComparison {
        candidate_is_better: candidate_eligible,
        reason: DiscardComparisonReason::IsolatedTile,
    })
}

// 多向聴時に限り、両候補とも孤立牌優先対象で片方が字牌・もう片方が数牌のとき、孤立字牌を
// 切る打牌を孤立数牌を切る打牌より優先する限定的な比較軸。以下の条件をすべて満たす場合だけ
// 決着させ、それ以外は None を返して後続の受け入れ以下の既存比較へ委ねる。
//
// - 両候補の最小向聴数が等しい
// - 最小向聴数が2以上（テンパイ・一向聴には適用しない）
// - 両候補とも isolated_tile_priority_eligible() == true
// - 片方が字牌・もう片方が数牌
//
// 役牌かどうかは使用しない。孤立ドラ字牌は isolated_tile_priority_eligible() を満たさないため
// 対象外となり、既存のドラ保護を維持する。両方字牌・両方数牌の場合はこの軸で決着させない。
// 比較相手に依存する条件を持たず、辞書順比較の推移律を維持する。
fn compare_isolated_honor_discard(
    candidate: &DiscardEvaluation,
    current_best: &DiscardEvaluation,
) -> Option<DiscardComparison> {
    let candidate_shanten = candidate.min_shanten_after_discard();
    let best_shanten = current_best.min_shanten_after_discard();
    if candidate_shanten != best_shanten {
        return None;
    }
    if candidate_shanten < 2 {
        return None;
    }

    if !isolated_tile_priority_eligible(candidate) || !isolated_tile_priority_eligible(current_best)
    {
        return None;
    }

    let candidate_is_honor = candidate.discard.is_honor();
    let best_is_honor = current_best.discard.is_honor();
    if candidate_is_honor == best_is_honor {
        return None;
    }

    Some(DiscardComparison {
        candidate_is_better: candidate_is_honor,
        reason: DiscardComparisonReason::IsolatedHonor,
    })
}

// 七対子単騎待ちの固定順位。値が小さいほど良い待ちとする。
//
// 字牌 > 1/9 > 2/8 > 3/7 > 4/6 > 5 だけを表現し、スートや場風・自風・役牌は区別しない。
// 七対子 tie-break 専用で、一般的な牌の安全度や待ちの良さとして使わない。
fn chiitoitsu_wait_quality_rank(wait: TileType) -> u8 {
    match wait.number() {
        None => 0,
        Some(number) => number.min(10 - number),
    }
}

// 打牌後が七対子テンパイである候補の、七対子を完成させる単騎待ち牌。
//
// 判定は既存評価が持つ値だけで行い、打牌後13枚の再構築も向聴・受け入れの再計算もしない。
// 副露形は七対子の対象外なので `Concealed` に限る。通常形と七対子が同時にテンパイしている
// 場合に受け入れ全体を七対子待ちと誤認しないよう、七対子が和了になる (`chiitoitsu == -1`)
// 受け入れ牌だけを対象にし、一意に定まらない場合は `None` を返して tie-break しない。
fn chiitoitsu_tenpai_wait(evaluation: &DiscardEvaluation) -> Option<TileType> {
    if evaluation.min_shanten_after_discard() != 0 {
        return None;
    }
    if evaluation.shanten_after_discard.concealed()?.chiitoitsu != 0 {
        return None;
    }

    let mut waits = evaluation
        .acceptance_after_discard
        .tiles
        .iter()
        .filter(|acceptance| {
            acceptance
                .shanten_after_draw
                .concealed()
                .is_some_and(|shanten| shanten.chiitoitsu == -1)
        });
    let wait = waits.next()?;
    waits.next().is_none().then_some(wait.tile)
}

// 両候補とも七対子テンパイで単騎待ち牌が一意に定まる場合だけ、待ち牌の固定順位で決着させる
// 限定的な tie-break。受け入れ残枚数・種類数の比較より後に置き、同値のときだけ使う。
// 同順位・非対象は None を返して後続の既存比較へ委ねる。
fn compare_chiitoitsu_wait_quality(
    candidate: &DiscardEvaluation,
    current_best: &DiscardEvaluation,
) -> Option<DiscardComparison> {
    let candidate_rank = chiitoitsu_wait_quality_rank(chiitoitsu_tenpai_wait(candidate)?);
    let best_rank = chiitoitsu_wait_quality_rank(chiitoitsu_tenpai_wait(current_best)?);
    if candidate_rank == best_rank {
        return None;
    }

    Some(DiscardComparison {
        candidate_is_better: candidate_rank < best_rank,
        reason: DiscardComparisonReason::ChiitoitsuWaitQuality,
    })
}

// 完全一向聴を維持する限定的な tie-break。両候補とも通常形一向聴（打牌後13枚・standard 一向聴）
// かつ全体最小向聴も一向聴で、片方だけが完全一向聴のときだけ完全一向聴を優先する。
// 完全一向聴同士・非完全一向聴同士には順位を付けず、後続の shape_penalty 以下へ委ねる。
fn compare_standard_iishanten_shape(
    candidate: &DiscardEvaluation,
    current_best: &DiscardEvaluation,
) -> Option<DiscardComparison> {
    // 全体テンパイ（七対子・国士など）を含む候補には適用しない。両候補とも全体最小向聴が一向聴。
    if candidate.min_shanten_after_discard() != 1 || current_best.min_shanten_after_discard() != 1 {
        return None;
    }
    // 両候補とも通常形一向聴である場合に限定する。片方だけが通常形一向聴なら決着させない。
    if candidate.shanten_after_discard.standard() != 1
        || current_best.shanten_after_discard.standard() != 1
    {
        return None;
    }

    let candidate_complete =
        candidate.standard_iishanten_shape_after_discard == IishantenShape::Complete;
    let best_complete =
        current_best.standard_iishanten_shape_after_discard == IishantenShape::Complete;
    // 完全一向聴同士・非完全一向聴同士は順位を付けない。
    if candidate_complete == best_complete {
        return None;
    }

    Some(DiscardComparison {
        candidate_is_better: candidate_complete,
        reason: DiscardComparisonReason::IishantenShape,
    })
}

fn is_better_discard(candidate: &DiscardEvaluation, best: &DiscardEvaluation) -> bool {
    compare_discard_evaluations(candidate, best).candidate_is_better
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscardDecisionDiagnostic {
    pub selected: Option<DiscardEvaluation>,
    pub candidates: Vec<DiscardCandidateDiagnostic>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscardCandidateDiagnostic {
    pub evaluation: DiscardEvaluation,
    pub selected: bool,
    pub selected_is_strictly_better_than_candidate: bool,
    pub comparison_reason: DiscardComparisonReason,
    pub shape_breakdown: ShapeBreakdown,
    pub pair_context: PairContext,
    pub block_context: DiscardBlockContext,
    pub floating_tile_value_breakdown: FloatingTileValue,
    /// 打牌選択に使った1向聴限定の前方集計値。前方評価を計算しなかった候補は `None`。
    ///
    /// 診断のために再計算せず、選択で使った値をそのまま保持する。待ちがすべて死んでいる場合の
    /// 有効な 0 と、計算していない `None` を区別する。
    pub tenpai_wait: Option<TenpaiWaitMetric>,
    /// 2向聴以上で打牌選択に使った weighted next acceptance。
    pub next_acceptance: Option<NextAcceptanceMetric>,
    /// 打牌選択に使った打点込みの前方集計値。確定しなかった候補と計算しなかった候補は `None`。
    pub prospective_value: Option<u64>,
}

pub fn diagnose_discard_evaluations(
    counts: &TileCounts,
    evaluations: &[DiscardEvaluation],
) -> DiscardDecisionDiagnostic {
    diagnose_discard_evaluations_with_fixed_melds(counts, FixedMeldCount::NONE, evaluations)
}

/// 副露済み面子数を考慮して打牌候補の診断を構築する。
///
/// 比較・選択は [`compare_discard_evaluations`] を通る本番と同じ経路で、診断専用の比較ロジックは
/// 持たない。`fixed_meld_count` は block context のブロック不足判定にだけ使い、本番評価と同じ
/// 補正を診断へ反映する。`FixedMeldCount::NONE` では [`diagnose_discard_evaluations`] と一致する。
pub fn diagnose_discard_evaluations_with_fixed_melds(
    counts: &TileCounts,
    fixed_meld_count: FixedMeldCount,
    evaluations: &[DiscardEvaluation],
) -> DiscardDecisionDiagnostic {
    diagnose_discard_evaluations_with_fixed_melds_and_forward_metrics(
        counts,
        fixed_meld_count,
        evaluations,
        &[],
    )
}

/// 打牌選択で使った1向聴限定の前方集計値も含めて打牌候補の診断を構築する。
///
/// 比較・選択は本番と同じ [`compare_discard_selection_candidates`] を通り、診断専用の比較
/// ロジックは持たない。`tenpai_wait` は `evaluations` と同じ順序で、範囲外の index は前方評価
/// なしとして扱う。空スライスを渡すと [`diagnose_discard_evaluations_with_fixed_melds`] と
/// 一致する。前方集計値は診断のために再計算せず、渡された値をそのまま候補診断へ載せる。
pub fn diagnose_discard_evaluations_with_fixed_melds_and_forward_metrics(
    counts: &TileCounts,
    fixed_meld_count: FixedMeldCount,
    evaluations: &[DiscardEvaluation],
    forward_metrics: &[ForwardMetrics],
) -> DiscardDecisionDiagnostic {
    let candidate_at = |index: usize| DiscardSelectionCandidate {
        evaluation: &evaluations[index],
        tenpai_wait: forward_metrics
            .get(index)
            .and_then(|metric| metric.tenpai_wait),
        next_acceptance: forward_metrics
            .get(index)
            .and_then(|metric| metric.next_acceptance),
        prospective_value: forward_metrics
            .get(index)
            .and_then(|metric| metric.prospective_value),
    };

    let best_index =
        best_discard_selection_index_with_forward_metrics(evaluations, forward_metrics);
    let selected = best_index.map(|index| evaluations[index].clone());

    let candidates = evaluations
        .iter()
        .enumerate()
        .map(|(index, evaluation)| {
            let is_selected = Some(index) == best_index;
            let (selected_is_strictly_better_than_candidate, comparison_reason) = if is_selected {
                (false, DiscardComparisonReason::StableOrder)
            } else {
                let best_index = best_index
                    .expect("non-selected candidate implies a selected evaluation exists");
                let comparison = compare_discard_selection_candidates(
                    &candidate_at(best_index),
                    &candidate_at(index),
                );
                if comparison.candidate_is_better {
                    (true, comparison.reason)
                } else {
                    (false, DiscardComparisonReason::StableOrder)
                }
            };

            DiscardCandidateDiagnostic {
                evaluation: evaluation.clone(),
                selected: is_selected,
                selected_is_strictly_better_than_candidate,
                comparison_reason,
                shape_breakdown: shape_breakdown_for_discard(counts, evaluation.discard),
                pair_context: pair_context_for_discard(counts, evaluation.discard),
                block_context: discard_block_context_with_fixed_melds(
                    counts,
                    evaluation.discard,
                    fixed_meld_count,
                ),
                floating_tile_value_breakdown: floating_tile_value_breakdown_for_discard(
                    counts,
                    evaluation.discard,
                ),
                tenpai_wait: forward_metrics
                    .get(index)
                    .and_then(|metric| metric.tenpai_wait),
                next_acceptance: forward_metrics
                    .get(index)
                    .and_then(|metric| metric.next_acceptance),
                prospective_value: forward_metrics
                    .get(index)
                    .and_then(|metric| metric.prospective_value),
            }
        })
        .collect();

    DiscardDecisionDiagnostic {
        selected,
        candidates,
    }
}

/// Compatibility entry point for callers that only provide the existing 1向聴 metric.
pub fn diagnose_discard_evaluations_with_fixed_melds_and_tenpai_wait(
    counts: &TileCounts,
    fixed_meld_count: FixedMeldCount,
    evaluations: &[DiscardEvaluation],
    tenpai_wait: &[Option<TenpaiWaitMetric>],
) -> DiscardDecisionDiagnostic {
    let metrics: Vec<_> = tenpai_wait
        .iter()
        .map(|&tenpai_wait| ForwardMetrics {
            tenpai_wait,
            next_acceptance: None,
            prospective_value: tenpai_wait.and_then(|metric| metric.prospective_value),
        })
        .collect();
    diagnose_discard_evaluations_with_fixed_melds_and_forward_metrics(
        counts,
        fixed_meld_count,
        evaluations,
        &metrics,
    )
}

fn value_honor_count(
    tile: TileType,
    round_wind: Option<TileType>,
    seat_wind: Option<TileType>,
) -> u8 {
    let mut count = u8::from(tile.is_dragon());
    if tile.is_wind() {
        count += u8::from(round_wind == Some(tile));
        count += u8::from(seat_wind == Some(tile));
    }
    count
}

// 打牌候補ごとの受け入れ計算で使う seen 牌の扱い。
//
// - `base`: 手牌以外に見えている枚数。visible tiles が無い経路では 0 のままで、打牌後 counts
//   だけを seen とする既存 semantics になる。
// - `counts_candidate_discard`: 今から切る候補牌1枚を seen へ加えるかどうか。visible tiles を
//   持つ経路だけ `true` にして、自分が今切った牌を山に残っている牌として数えない。visible tiles
//   が無い経路では既存どおり `false` で、候補牌を seen に数えない。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct CandidateSeen {
    base: [u8; TileType::COUNT],
    counts_candidate_discard: bool,
}

impl CandidateSeen {
    pub(crate) const fn hand_only() -> Self {
        Self {
            base: [0u8; TileType::COUNT],
            counts_candidate_discard: false,
        }
    }

    // 自分の手牌以外に見えている枚数を、visible tiles と手牌から求める。残枚数計算と同じ
    // 手牌差し引きを使うため、acceptance 側の helper へ委譲する。
    pub(crate) fn from_visible_tiles(counts: &TileCounts, visible_tiles: &[TileId]) -> Self {
        Self {
            base: additional_seen(counts, visible_tiles),
            counts_candidate_discard: true,
        }
    }

    // `discard` を1枚切った後の seen 状態。切った牌は以降どの経路でも見え牌なので base へ
    // 1枚加える。2手先診断が1手目の打牌を2手目の seen として引き継ぐために使う。
    pub(crate) fn after_discard(&self, discard: TileType) -> Self {
        let mut base = self.base;
        base[discard.index()] = base[discard.index()].saturating_add(1);
        Self { base, ..*self }
    }

    fn additional_seen(&self, discard: TileType) -> [u8; TileType::COUNT] {
        let mut additional_seen = self.base;
        if self.counts_candidate_discard {
            additional_seen[discard.index()] = additional_seen[discard.index()].saturating_add(1);
        }
        additional_seen
    }
}

// 打牌候補評価の本体。門前・副露と visible tiles の有無で共有する唯一の生成経路。
//
// 副露済み面子数は受け入れ計算 (PR #108 の fixed meld 対応 API)・一向聴形分類・形ペナルティの
// ブロック不足判定へ渡す。打牌しても副露済み面子数は変わらないため、候補打牌後も同じ値を使う。
pub(crate) fn evaluate_discards_with_seen(
    counts: &TileCounts,
    fixed_meld_count: FixedMeldCount,
    seen: &CandidateSeen,
) -> Vec<DiscardEvaluation> {
    let mut evaluations = Vec::new();

    for tile in TileType::all() {
        let count_before_discard = counts.count(tile);
        if count_before_discard == 0 {
            continue;
        }

        let mut after_discard = *counts;
        if after_discard.remove(tile).is_err() {
            continue;
        }

        let acceptance_after_discard = calculate_acceptance_with_fixed_melds_and_seen(
            &after_discard,
            fixed_meld_count,
            &seen.additional_seen(tile),
        );
        let shanten_after_discard = acceptance_after_discard.current;
        let standard_iishanten_shape_after_discard =
            iishanten_shape_after_discard(&after_discard, shanten_after_discard, fixed_meld_count);
        // floating_tile_value と孤立牌判定は同じ breakdown から一度だけ取得する。
        // 孤立牌判定は手牌構造だけを使う。visible tiles は受け入れ計算のみに影響させる。
        let floating = floating_tile_value_breakdown_for_discard(counts, tile);

        evaluations.push(DiscardEvaluation {
            discard: tile,
            count_before_discard,
            shanten_after_discard,
            acceptance_after_discard,
            shape_penalty: shape_penalty_for_discard_with_fixed_melds(
                counts,
                tile,
                fixed_meld_count,
            ),
            floating_tile_value: floating.value,
            discarded_dora_count: 0,
            discarded_value_honor_count: 0,
            discards_red_five: false,
            discards_isolated_tile: floating.is_isolated,
            standard_iishanten_shape_after_discard,
        });
    }

    evaluations
}

// 打牌後の通常形一向聴の形分類。分類器は門前13枚専用なので、副露済み面子がある手牌は
// 分類せず Unknown にする。門前では計算済みの通常形向聴数を再利用し、分類のための
// standard_shanten() 再計算を避ける。
fn iishanten_shape_after_discard(
    after_discard: &TileCounts,
    shanten_after_discard: EffectiveShanten,
    fixed_meld_count: FixedMeldCount,
) -> IishantenShape {
    if fixed_meld_count.has_melds() {
        return IishantenShape::Unknown;
    }
    classify_standard_iishanten_shape_with_standard_shanten(
        after_discard,
        shanten_after_discard.standard(),
    )
}

pub fn evaluate_discards(counts: &TileCounts) -> Vec<DiscardEvaluation> {
    evaluate_discards_with_fixed_melds(counts, FixedMeldCount::NONE)
}

/// 副露済み面子数を考慮して全打牌候補を評価する。
///
/// 候補列挙・受け入れ計算・形評価・比較は [`evaluate_discards`] と同じ実装を共有する。
/// `fixed_meld_count == FixedMeldCount::NONE` では [`evaluate_discards`] と一致する。
pub fn evaluate_discards_with_fixed_melds(
    counts: &TileCounts,
    fixed_meld_count: FixedMeldCount,
) -> Vec<DiscardEvaluation> {
    evaluate_discards_with_seen(counts, fixed_meld_count, &CandidateSeen::hand_only())
}

pub fn evaluate_discards_with_visible_tiles(
    counts: &TileCounts,
    visible_tiles: &[TileId],
) -> Vec<DiscardEvaluation> {
    evaluate_discards_with_fixed_melds_and_visible_tiles(
        counts,
        FixedMeldCount::NONE,
        visible_tiles,
    )
}

/// 副露済み面子数と visible tiles を考慮して全打牌候補を評価する。
///
/// 受け入れの残枚数は「自分の手牌以外に見えている牌 + 今から切る候補牌1枚」を seen として
/// 求める既存 semantics を維持する。`fixed_meld_count == FixedMeldCount::NONE` では
/// [`evaluate_discards_with_visible_tiles`] と一致する。
pub fn evaluate_discards_with_fixed_melds_and_visible_tiles(
    counts: &TileCounts,
    fixed_meld_count: FixedMeldCount,
    visible_tiles: &[TileId],
) -> Vec<DiscardEvaluation> {
    if visible_tiles.is_empty() {
        return evaluate_discards_with_fixed_melds(counts, fixed_meld_count);
    }

    evaluate_discards_with_seen(
        counts,
        fixed_meld_count,
        &CandidateSeen::from_visible_tiles(counts, visible_tiles),
    )
}

pub fn evaluate_discards_from_tiles(tiles: &[TileId]) -> Vec<DiscardEvaluation> {
    evaluate_discards_from_tiles_with_dora(tiles, &[])
}

pub fn evaluate_discards_from_tiles_with_dora(
    tiles: &[TileId],
    dora_indicators: &[TileId],
) -> Vec<DiscardEvaluation> {
    let counts = TileCounts::from_tiles(tiles.iter().copied());
    let mut evaluations = evaluate_discards(&counts);
    decorate_evaluations(
        &mut evaluations,
        &counts,
        &DecorationContext {
            tiles,
            dora_indicators,
            round_wind: None,
            seat_wind: None,
            shape_penalty: ShapePenaltyMode::ContextFree,
            unresolved_red_tile: None,
        },
    );
    evaluations
}

pub fn evaluate_discards_from_tiles_with_context(
    tiles: &[TileId],
    dora_indicators: &[TileId],
    round_wind: Option<TileType>,
    seat_wind: Option<TileType>,
) -> Vec<DiscardEvaluation> {
    evaluate_discards_from_tiles_with_fixed_melds_and_context(
        tiles,
        FixedMeldCount::NONE,
        dora_indicators,
        round_wind,
        seat_wind,
    )
}

/// 副露済み面子数を考慮して物理牌一覧から全打牌候補を評価する。
///
/// `fixed_meld_count == FixedMeldCount::NONE` では
/// [`evaluate_discards_from_tiles_with_context`] と一致する。
pub fn evaluate_discards_from_tiles_with_fixed_melds_and_context(
    tiles: &[TileId],
    fixed_meld_count: FixedMeldCount,
    dora_indicators: &[TileId],
    round_wind: Option<TileType>,
    seat_wind: Option<TileType>,
) -> Vec<DiscardEvaluation> {
    let counts = TileCounts::from_tiles(tiles.iter().copied());
    let mut evaluations = evaluate_discards_with_fixed_melds(&counts, fixed_meld_count);
    decorate_evaluations(
        &mut evaluations,
        &counts,
        &DecorationContext {
            tiles,
            dora_indicators,
            round_wind,
            seat_wind,
            shape_penalty: ShapePenaltyMode::WithContext {
                round_wind,
                seat_wind,
                fixed_meld_count,
            },
            unresolved_red_tile: None,
        },
    );
    evaluations
}

pub fn evaluate_discards_from_tiles_with_visible_tiles(
    tiles: &[TileId],
    dora_indicators: &[TileId],
    round_wind: Option<TileType>,
    seat_wind: Option<TileType>,
    visible_tiles: &[TileId],
) -> Vec<DiscardEvaluation> {
    evaluate_discards_from_tiles_with_fixed_melds_and_visible_tiles(
        tiles,
        FixedMeldCount::NONE,
        dora_indicators,
        round_wind,
        seat_wind,
        visible_tiles,
    )
}

/// 副露済み面子数と visible tiles を考慮して物理牌一覧から全打牌候補を評価する。
///
/// `fixed_meld_count == FixedMeldCount::NONE` では
/// [`evaluate_discards_from_tiles_with_visible_tiles`] と一致する。
#[allow(clippy::too_many_arguments)]
pub fn evaluate_discards_from_tiles_with_fixed_melds_and_visible_tiles(
    tiles: &[TileId],
    fixed_meld_count: FixedMeldCount,
    dora_indicators: &[TileId],
    round_wind: Option<TileType>,
    seat_wind: Option<TileType>,
    visible_tiles: &[TileId],
) -> Vec<DiscardEvaluation> {
    let counts = TileCounts::from_tiles(tiles.iter().copied());
    let mut evaluations = evaluate_discards_with_fixed_melds_and_visible_tiles(
        &counts,
        fixed_meld_count,
        visible_tiles,
    );
    decorate_evaluations(
        &mut evaluations,
        &counts,
        &DecorationContext {
            tiles,
            dora_indicators,
            round_wind,
            seat_wind,
            shape_penalty: ShapePenaltyMode::WithContext {
                round_wind,
                seat_wind,
                fixed_meld_count,
            },
            unresolved_red_tile: None,
        },
    );
    evaluations
}

pub(crate) enum ShapePenaltyMode {
    ContextFree,
    WithContext {
        round_wind: Option<TileType>,
        seat_wind: Option<TileType>,
        fixed_meld_count: FixedMeldCount,
    },
}

/// 打牌候補評価へ後付けする文脈。
///
/// 牌種だけで決まる項目 (通常ドラ・役牌・形ペナルティ) と、物理牌が必要な項目 (赤5・赤ドラ) を
/// 同じ経路で反映するための入力。通常打牌評価と2手先診断が共有する。
pub(crate) struct DecorationContext<'a> {
    /// 切る物理牌を決めるための手牌。牌種ごとに黒牌を赤牌より優先して選ぶ。
    pub tiles: &'a [TileId],
    pub dora_indicators: &'a [TileId],
    pub round_wind: Option<TileType>,
    pub seat_wind: Option<TileType>,
    pub shape_penalty: ShapePenaltyMode,
    /// 物理牌が一意に決まらず、赤5かどうかを解決できない牌種。
    ///
    /// 2手先診断の仮想ツモ牌だけが該当する。受け入れは34種の牌種単位なので、ツモった5が赤か
    /// 黒かは決まらない。この牌種を切る候補では赤5扱いにせず、牌種から確定する通常ドラだけを
    /// 反映する。通常打牌評価は物理牌がすべて分かっているため `None` を渡す。
    pub unresolved_red_tile: Option<TileType>,
}

pub(crate) fn decorate_evaluations(
    evaluations: &mut [DiscardEvaluation],
    counts: &TileCounts,
    context: &DecorationContext,
) {
    for evaluation in evaluations {
        // 赤5かどうかが解決できない牌種では赤扱いにしない。手牌に黒の同種牌があれば通常打牌
        // 評価も黒を選ぶため結果は一致し、赤1枚しか無い場合はその赤が手牌に見えている以上
        // 仮想ツモは黒に確定する。同種牌が1枚も無い場合だけ赤かどうかが未解決のまま残る。
        let resolves_red_five = context.unresolved_red_tile != Some(evaluation.discard);
        let discarded_tile = discarded_tile_id_for_type(evaluation.discard, context.tiles, None);
        evaluation.discards_red_five =
            resolves_red_five && discarded_tile.map(TileId::is_red).unwrap_or(false);
        // 通常ドラは牌種だけで決まるため、物理牌が分からなくても必ず反映する。
        evaluation.discarded_dora_count =
            count_indicated_dora(evaluation.discard, context.dora_indicators)
                + u8::from(evaluation.discards_red_five);
        evaluation.discarded_value_honor_count =
            value_honor_count(evaluation.discard, context.round_wind, context.seat_wind);
        if let ShapePenaltyMode::WithContext {
            round_wind,
            seat_wind,
            fixed_meld_count,
        } = context.shape_penalty
        {
            evaluation.shape_penalty = shape_penalty_for_discard_with_fixed_melds_and_context(
                counts,
                evaluation.discard,
                fixed_meld_count,
                round_wind,
                seat_wind,
            );
        }
    }
}

/// 物理牌一覧から、打牌候補1件が実際に切る物理牌を1枚だけ切り離す。
///
/// 一致条件は牌種と赤フラグ (`discards_red_five`) の両方で、一致する物理牌が無ければ別の牌で
/// 代用せず `None`。返り値は切る物理牌と、それを除いた残りの物理牌一覧。赤5と通常5では打点も
/// 評価も変わるため、牌種だけで代用しない。
///
/// 打牌後の手牌を必要とする経路 (2手先評価の仮想局面・押し引きの打点 proxy・ダマ打点) はこの
/// 1本を共有し、同じ組み立てを複製しない。除去は1枚だけで、残りの牌の並びには意味を持たせない。
pub fn split_discarded_tile(
    mut tiles: Vec<TileId>,
    evaluation: &DiscardEvaluation,
) -> Option<(TileId, Vec<TileId>)> {
    let discarded = tiles.iter().position(|tile| {
        tile.tile_type() == evaluation.discard && tile.is_red() == evaluation.discards_red_five
    })?;
    Some((tiles.remove(discarded), tiles))
}

/// 指定牌種を切るときに使う物理牌を返す。
///
/// `discards_red_five` は「実際に切る牌が赤5かどうか」が上位層で確定している場合のその値。
/// 確定していれば一致する物理牌を優先し、確定していない (`None`) 場合や一致する牌が無い場合は
/// 通常牌を赤牌より優先する既定の規則で選ぶ。物理牌の選択規則はこの1箇所だけに置く。
pub(crate) fn discarded_tile_id_for_type(
    discard: TileType,
    tiles: &[TileId],
    discards_red_five: Option<bool>,
) -> Option<TileId> {
    let mut red = None;
    let mut black = None;
    for &tile in tiles {
        if tile.tile_type() != discard {
            continue;
        }
        if tile.is_red() {
            red.get_or_insert(tile);
        } else {
            black.get_or_insert(tile);
        }
    }

    if discards_red_five == Some(true) {
        return red.or(black);
    }
    black.or(red)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::acceptance::{
        Acceptance, calculate_acceptance, calculate_acceptance_with_fixed_melds,
        calculate_acceptance_with_fixed_melds_and_visible_tiles,
    };
    use crate::shanten::{Shanten, standard_shanten_with_fixed_melds};

    fn tile(s: &str) -> TileType {
        TileType::from_mjai_type_str(s).unwrap()
    }

    fn fixed(value: u8) -> FixedMeldCount {
        FixedMeldCount::new(value).unwrap()
    }

    fn concealed(shanten: Shanten) -> EffectiveShanten {
        EffectiveShanten::Concealed(shanten)
    }

    fn counts(strings: &[&str]) -> TileCounts {
        TileCounts::from_tile_types(strings.iter().map(|s| tile(s)))
    }

    fn discard_tiles(evaluations: &[DiscardEvaluation]) -> Vec<TileType> {
        evaluations.iter().map(|entry| entry.discard).collect()
    }

    #[test]
    fn empty_hand_has_no_candidates() {
        assert!(evaluate_discards(&TileCounts::new()).is_empty());
    }

    #[test]
    fn only_existing_tile_types_are_candidates() {
        let counts = counts(&["1m", "1m", "2m", "3m", "E"]);
        let evaluations = evaluate_discards(&counts);
        assert_eq!(
            discard_tiles(&evaluations),
            vec![tile("1m"), tile("2m"), tile("3m"), tile("E")]
        );
        let first = &evaluations[0];
        assert_eq!(first.discard, tile("1m"));
        assert_eq!(first.count_before_discard, 2);
    }

    #[test]
    fn does_not_modify_input_counts() {
        let counts = counts(&[
            "1m", "2m", "3m", "4m", "5m", "6m", "7m", "8m", "9m", "1p", "2p", "3p", "5s", "5s",
        ]);
        let before = counts;
        let _ = evaluate_discards(&counts);
        assert_eq!(counts, before);
    }

    #[test]
    fn results_are_ordered_by_tile_type() {
        let counts = counts(&["1m", "5m", "9m", "1p", "5p", "9p", "1s", "5s", "9s", "E"]);
        let evaluations = evaluate_discards(&counts);
        assert!(evaluations.len() > 1);
        assert!(
            evaluations
                .windows(2)
                .all(|pair| pair[0].discard.raw() < pair[1].discard.raw())
        );
    }

    #[test]
    fn evaluates_standard_winning_hand() {
        let counts = counts(&[
            "1m", "2m", "3m", "4m", "5m", "6m", "7m", "8m", "9m", "1p", "2p", "3p", "5s", "5s",
        ]);
        let evaluations = evaluate_discards(&counts);
        assert!(!evaluations.is_empty());
        for evaluation in &evaluations {
            assert_eq!(
                evaluation.shanten_after_discard,
                evaluation.acceptance_after_discard.current
            );
            assert_eq!(
                evaluation.min_shanten_after_discard(),
                evaluation.shanten_after_discard.min()
            );
            assert_eq!(
                evaluation.acceptance_type_count(),
                evaluation.acceptance_after_discard.tiles.len()
            );
            assert_eq!(
                evaluation.acceptance_total_remaining(),
                evaluation.acceptance_after_discard.total_remaining()
            );
        }
    }

    #[test]
    fn acceptance_can_be_compared_between_candidates() {
        let counts = counts(&[
            "1m", "2m", "3m", "4m", "5m", "6m", "7m", "8m", "9m", "2p", "3p", "5s", "5s",
        ]);
        let evaluations = evaluate_discards(&counts);
        assert!(evaluations.len() > 1);
        assert!(
            evaluations
                .iter()
                .any(|evaluation| evaluation.acceptance_total_remaining() > 0)
        );
    }

    #[test]
    fn count_before_discard_is_correct() {
        let counts = counts(&["1m", "1m", "1m", "2m", "3m"]);
        let evaluations = evaluate_discards(&counts);
        let ones: Vec<_> = evaluations
            .iter()
            .filter(|evaluation| evaluation.discard == tile("1m"))
            .collect();
        assert_eq!(ones.len(), 1);
        assert_eq!(ones[0].count_before_discard, 3);
    }

    #[test]
    fn evaluates_state_after_one_tile_removed() {
        let counts = counts(&["1m", "1m", "2m", "3m"]);
        let evaluations = evaluate_discards(&counts);
        let one = evaluations
            .iter()
            .find(|evaluation| evaluation.discard == tile("1m"))
            .expect("1m should be a candidate");
        assert_eq!(one.count_before_discard, 2);
        assert_eq!(
            one.shanten_after_discard,
            one.acceptance_after_discard.current
        );
    }

    #[test]
    fn select_best_discard_returns_none_for_empty_hand() {
        assert_eq!(select_best_discard(&TileCounts::new()), None);
    }

    #[test]
    fn select_best_discard_returns_single_candidate() {
        let counts = counts(&["1m"]);
        let selected = select_best_discard(&counts).expect("1m should be selected");
        assert_eq!(selected.discard, tile("1m"));
        assert_eq!(selected.count_before_discard, 1);
    }

    #[test]
    fn select_best_discard_prefers_lower_shanten() {
        let counts = counts(&[
            "1m", "2m", "3m", "4m", "5m", "6m", "7m", "8m", "9m", "1p", "2p", "3p", "5s", "W",
        ]);
        let evaluations = evaluate_discards(&counts);
        let selected = select_best_discard(&counts).unwrap();
        let best_shanten = evaluations
            .iter()
            .map(|evaluation| evaluation.min_shanten_after_discard())
            .min()
            .unwrap();
        assert_eq!(selected.min_shanten_after_discard(), best_shanten);
    }

    #[test]
    fn select_best_discard_prefers_more_acceptance_remaining() {
        let counts = counts(&[
            "1m", "2m", "3m", "5m", "6m", "9m", "1p", "2p", "3p", "5s", "5s", "E", "E", "W",
        ]);
        let evaluations = evaluate_discards(&counts);
        let selected = select_best_discard(&counts).unwrap();

        let best_shanten = evaluations
            .iter()
            .map(|evaluation| evaluation.min_shanten_after_discard())
            .min()
            .unwrap();
        let best_remaining = evaluations
            .iter()
            .filter(|evaluation| evaluation.min_shanten_after_discard() == best_shanten)
            .map(|evaluation| evaluation.acceptance_total_remaining())
            .max()
            .unwrap();

        assert_eq!(selected.min_shanten_after_discard(), best_shanten);
        assert_eq!(selected.acceptance_total_remaining(), best_remaining);
    }

    #[test]
    fn select_best_discard_prefers_more_acceptance_types() {
        let counts = counts(&[
            "1m", "2m", "3m", "4m", "6m", "8m", "1p", "2p", "3p", "5s", "5s", "7s", "8s", "W",
        ]);
        let evaluations = evaluate_discards(&counts);
        let selected = select_best_discard(&counts).unwrap();

        let best_shanten = evaluations
            .iter()
            .map(|evaluation| evaluation.min_shanten_after_discard())
            .min()
            .unwrap();
        let best_remaining = evaluations
            .iter()
            .filter(|evaluation| evaluation.min_shanten_after_discard() == best_shanten)
            .map(|evaluation| evaluation.acceptance_total_remaining())
            .max()
            .unwrap();
        let best_type_count = evaluations
            .iter()
            .filter(|evaluation| {
                evaluation.min_shanten_after_discard() == best_shanten
                    && evaluation.acceptance_total_remaining() == best_remaining
            })
            .map(|evaluation| evaluation.acceptance_type_count())
            .max()
            .unwrap();

        assert_eq!(selected.min_shanten_after_discard(), best_shanten);
        assert_eq!(selected.acceptance_total_remaining(), best_remaining);
        assert_eq!(selected.acceptance_type_count(), best_type_count);
    }

    #[test]
    fn select_best_discard_keeps_first_candidate_on_tie() {
        // 全て孤立数牌のみ。字牌を含めると孤立字牌軸で決着してしまうため数牌だけにする。
        let counts = counts(&["1m", "5m", "9m", "1p", "5p", "9p", "1s", "5s", "9s"]);
        let evaluations = evaluate_discards(&counts);
        let selected = select_best_discard(&counts).unwrap();
        let first_equal = evaluations
            .iter()
            .find(|evaluation| {
                evaluation.min_shanten_after_discard() == selected.min_shanten_after_discard()
                    && evaluation.acceptance_total_remaining()
                        == selected.acceptance_total_remaining()
                    && evaluation.acceptance_type_count() == selected.acceptance_type_count()
            })
            .unwrap();
        assert_eq!(selected, *first_equal);
    }

    #[test]
    fn select_best_discard_does_not_modify_input_counts() {
        let counts = counts(&[
            "1m", "2m", "3m", "4m", "5m", "6m", "7m", "8m", "9m", "1p", "2p", "3p", "5s", "5s",
        ]);
        let before = counts;
        let _ = select_best_discard(&counts);
        assert_eq!(counts, before);
    }

    #[test]
    fn select_best_discard_is_among_evaluated_candidates() {
        let counts = counts(&[
            "1m", "2m", "3m", "4m", "5m", "6m", "7m", "8m", "9m", "1p", "2p", "3p", "5s", "W",
        ]);
        let evaluations = evaluate_discards(&counts);
        let selected = select_best_discard(&counts).unwrap();
        assert!(
            evaluations
                .iter()
                .any(|evaluation| evaluation.discard == selected.discard)
        );
    }

    use crate::acceptance::{AcceptanceTile, EffectiveAcceptanceTile};
    use crate::tile::TileId;

    fn ids(values: &[u8]) -> Vec<TileId> {
        values.iter().map(|&v| TileId::new(v).unwrap()).collect()
    }

    // mjai 牌文字列から TileId 列を作る。同種牌は variant を 1 から順に割り当てるため、
    // variant 0 に割り当てられる赤5は選ばれず、赤ドラ保護と孤立字牌軸の検証が混ざらない。
    fn ids_of(strs: &[&str]) -> Vec<TileId> {
        let mut used: std::collections::HashMap<u8, u8> = std::collections::HashMap::new();
        strs.iter()
            .map(|s| {
                let index = tile(s).index() as u8;
                let variant = used.entry(index).or_insert(1);
                let id = TileId::new(index * 4 + *variant).unwrap();
                *variant += 1;
                id
            })
            .collect()
    }

    fn shanten_min(min: i8) -> EffectiveShanten {
        concealed(Shanten {
            standard: min,
            chiitoitsu: 127,
            kokushi: 127,
        })
    }

    fn evaluation(
        min: i8,
        remaining: u8,
        type_count: usize,
        dora: u8,
        red: bool,
    ) -> DiscardEvaluation {
        evaluation_with_value_honor(min, remaining, type_count, dora, 0, red)
    }

    fn evaluation_with_value_honor(
        min: i8,
        remaining: u8,
        type_count: usize,
        dora: u8,
        value_honor: u8,
        red: bool,
    ) -> DiscardEvaluation {
        evaluation_with_shape_penalty(min, remaining, type_count, 0, dora, value_honor, red)
    }

    fn evaluation_with_shape_penalty(
        min: i8,
        remaining: u8,
        type_count: usize,
        shape_penalty: i16,
        dora: u8,
        value_honor: u8,
        red: bool,
    ) -> DiscardEvaluation {
        let tiles: Vec<EffectiveAcceptanceTile> = (0..type_count)
            .map(|i| AcceptanceTile {
                tile: TileType::new(i as u8).unwrap(),
                remaining: if i == 0 { remaining } else { 0 },
                shanten_after_draw: shanten_min(min - 1),
            })
            .collect();

        DiscardEvaluation {
            discard: TileType::new(0).unwrap(),
            count_before_discard: 1,
            shanten_after_discard: shanten_min(min),
            acceptance_after_discard: Acceptance {
                current: shanten_min(min),
                tiles,
            },
            shape_penalty,
            floating_tile_value: 0,
            discarded_dora_count: dora,
            discarded_value_honor_count: value_honor,
            discards_red_five: red,
            discards_isolated_tile: false,
            standard_iishanten_shape_after_discard: IishantenShape::Unknown,
        }
    }

    // 上位3軸を固定した通常形一向聴の評価。tie-break が適用される min==1・standard==1 の前提を満たす。
    fn evaluation_with_iishanten_shape(
        remaining: u8,
        type_count: usize,
        shape: IishantenShape,
    ) -> DiscardEvaluation {
        evaluation_with_shape_penalty_and_iishanten_shape(remaining, type_count, 0, shape)
    }

    fn evaluation_with_shape_penalty_and_iishanten_shape(
        remaining: u8,
        type_count: usize,
        shape_penalty: i16,
        shape: IishantenShape,
    ) -> DiscardEvaluation {
        let mut evaluation =
            evaluation_with_shape_penalty(1, remaining, type_count, shape_penalty, 0, 0, false);
        evaluation.standard_iishanten_shape_after_discard = shape;
        evaluation
    }

    #[test]
    fn shanten_outranks_red_five_tiebreak() {
        let low_shanten_red = evaluation(0, 4, 1, 0, true);
        let high_shanten_keep = evaluation(1, 40, 5, 0, false);
        assert!(is_better_discard(&low_shanten_red, &high_shanten_keep));
    }

    #[test]
    fn acceptance_remaining_outranks_red_five_tiebreak() {
        let more_remaining_red = evaluation(1, 20, 1, 0, true);
        let less_remaining_keep = evaluation(1, 10, 1, 0, false);
        assert!(is_better_discard(&more_remaining_red, &less_remaining_keep));
    }

    #[test]
    fn acceptance_types_outrank_red_five_tiebreak() {
        let more_types_red = evaluation(1, 10, 3, 0, true);
        let fewer_types_keep = evaluation(1, 10, 2, 0, false);
        assert!(is_better_discard(&more_types_red, &fewer_types_keep));
    }

    #[test]
    fn shanten_outranks_dora_tiebreak() {
        let low_shanten_dora = evaluation(0, 4, 1, 2, false);
        let high_shanten_keep = evaluation(1, 40, 5, 0, false);
        assert!(is_better_discard(&low_shanten_dora, &high_shanten_keep));
    }

    #[test]
    fn acceptance_remaining_outranks_dora_tiebreak() {
        let more_remaining_dora = evaluation(1, 20, 1, 2, false);
        let less_remaining_keep = evaluation(1, 10, 1, 0, false);
        assert!(is_better_discard(
            &more_remaining_dora,
            &less_remaining_keep
        ));
    }

    #[test]
    fn acceptance_types_outrank_dora_tiebreak() {
        let more_types_dora = evaluation(1, 10, 3, 2, false);
        let fewer_types_keep = evaluation(1, 10, 2, 0, false);
        assert!(is_better_discard(&more_types_dora, &fewer_types_keep));
    }

    #[test]
    fn dora_tiebreak_prefers_fewer_dora() {
        let keep_dora = evaluation(1, 10, 2, 0, false);
        let discard_dora = evaluation(1, 10, 2, 1, false);
        assert!(is_better_discard(&keep_dora, &discard_dora));
        assert!(!is_better_discard(&discard_dora, &keep_dora));
    }

    #[test]
    fn dora_tiebreak_outranks_red_five_tiebreak() {
        let fewer_dora_discards_red = evaluation(1, 10, 2, 0, true);
        let more_dora_keeps_red = evaluation(1, 10, 2, 1, false);
        assert!(is_better_discard(
            &fewer_dora_discards_red,
            &more_dora_keeps_red
        ));
    }

    #[test]
    fn red_five_is_the_final_tiebreak() {
        let keep_red = evaluation(1, 10, 2, 0, false);
        let discard_red = evaluation(1, 10, 2, 0, true);
        assert!(is_better_discard(&keep_red, &discard_red));
        assert!(!is_better_discard(&discard_red, &keep_red));
    }

    #[test]
    fn value_honor_count_for_dragons() {
        assert_eq!(value_honor_count(tile("P"), None, None), 1);
        assert_eq!(value_honor_count(tile("F"), None, None), 1);
        assert_eq!(value_honor_count(tile("C"), None, None), 1);
    }

    #[test]
    fn value_honor_count_for_round_and_seat_winds() {
        assert_eq!(value_honor_count(tile("E"), Some(tile("E")), None), 1);
        assert_eq!(value_honor_count(tile("E"), None, Some(tile("E"))), 1);
        assert_eq!(
            value_honor_count(tile("E"), Some(tile("E")), Some(tile("E"))),
            2
        );
    }

    #[test]
    fn value_honor_count_for_guest_and_number_tiles() {
        assert_eq!(
            value_honor_count(tile("W"), Some(tile("E")), Some(tile("S"))),
            0
        );
        assert_eq!(
            value_honor_count(tile("1m"), Some(tile("E")), Some(tile("S"))),
            0
        );
        assert_eq!(value_honor_count(tile("E"), None, None), 0);
    }

    #[test]
    fn value_honor_tiebreak_prefers_keeping_value_honor() {
        let keep_honor = evaluation_with_value_honor(1, 10, 2, 0, 0, false);
        let discard_honor = evaluation_with_value_honor(1, 10, 2, 0, 1, false);
        assert!(is_better_discard(&keep_honor, &discard_honor));
        assert!(!is_better_discard(&discard_honor, &keep_honor));
    }

    #[test]
    fn value_honor_tiebreak_outranks_red_five() {
        let keep_honor_discards_red = evaluation_with_value_honor(1, 10, 2, 0, 0, true);
        let discard_honor_keeps_red = evaluation_with_value_honor(1, 10, 2, 0, 1, false);
        assert!(is_better_discard(
            &keep_honor_discards_red,
            &discard_honor_keeps_red
        ));
    }

    #[test]
    fn dora_tiebreak_outranks_value_honor() {
        let keep_dora_discards_honor = evaluation_with_value_honor(1, 10, 2, 0, 1, false);
        let discard_dora_keeps_honor = evaluation_with_value_honor(1, 10, 2, 1, 0, false);
        assert!(is_better_discard(
            &keep_dora_discards_honor,
            &discard_dora_keeps_honor
        ));
    }

    #[test]
    fn double_wind_is_harder_to_discard_than_single_value_honor() {
        let single_honor = evaluation_with_value_honor(1, 10, 2, 0, 1, false);
        let double_wind = evaluation_with_value_honor(1, 10, 2, 0, 2, false);
        assert!(is_better_discard(&single_honor, &double_wind));
        assert!(!is_better_discard(&double_wind, &single_honor));
    }

    #[test]
    fn with_context_sets_value_honor_count() {
        // 123m 456m 789m 123p + 中(浮き) 1p(浮き)
        let tiles = ids(&[0, 4, 8, 12, 17, 20, 24, 28, 32, 36, 40, 44, 132, 37]);
        let evaluations = evaluate_discards_from_tiles_with_context(
            &tiles,
            &[],
            Some(tile("E")),
            Some(tile("S")),
        );
        let dragon = evaluations
            .iter()
            .find(|evaluation| evaluation.discard == tile("C"))
            .unwrap();
        assert_eq!(dragon.discarded_value_honor_count, 1);
        let number = evaluations
            .iter()
            .find(|evaluation| evaluation.discard == tile("1m"))
            .unwrap();
        assert_eq!(number.discarded_value_honor_count, 0);
    }

    #[test]
    fn without_context_only_dragons_are_value_honors() {
        // context 無しの _with_dora では三元牌だけ役牌として数える
        let tiles = ids(&[0, 4, 8, 12, 17, 20, 24, 28, 32, 36, 40, 44, 132, 108]);
        let evaluations = evaluate_discards_from_tiles_with_dora(&tiles, &[]);
        let dragon = evaluations
            .iter()
            .find(|evaluation| evaluation.discard == tile("C"))
            .unwrap();
        assert_eq!(dragon.discarded_value_honor_count, 1);
        let wind = evaluations
            .iter()
            .find(|evaluation| evaluation.discard == tile("E"))
            .unwrap();
        assert_eq!(wind.discarded_value_honor_count, 0);
    }

    #[test]
    fn shanten_outranks_value_honor_tiebreak() {
        let low_shanten_honor = evaluation_with_value_honor(0, 4, 1, 0, 1, false);
        let high_shanten_keep = evaluation_with_value_honor(1, 40, 5, 0, 0, false);
        assert!(is_better_discard(&low_shanten_honor, &high_shanten_keep));
    }

    #[test]
    fn acceptance_remaining_outranks_value_honor_tiebreak() {
        let more_remaining_honor = evaluation_with_value_honor(1, 20, 1, 0, 1, false);
        let less_remaining_keep = evaluation_with_value_honor(1, 10, 1, 0, 0, false);
        assert!(is_better_discard(
            &more_remaining_honor,
            &less_remaining_keep
        ));
    }

    #[test]
    fn acceptance_types_outrank_value_honor_tiebreak() {
        let more_types_honor = evaluation_with_value_honor(1, 10, 3, 0, 1, false);
        let fewer_types_keep = evaluation_with_value_honor(1, 10, 2, 0, 0, false);
        assert!(is_better_discard(&more_types_honor, &fewer_types_keep));
    }

    #[test]
    fn context_perfect_tie_keeps_value_honor() {
        // 123m 456m 789m 123p + 中(浮き) 北(浮き)
        // どちらを切っても同じ単騎テンパイ。役牌でない北(客風)を優先して切る
        let tiles = ids(&[0, 4, 8, 12, 17, 20, 24, 28, 32, 36, 40, 44, 132, 120]);
        let selected =
            select_best_discard_from_tiles_with_context(&tiles, &[], None, None).unwrap();
        assert_eq!(selected.discard, tile("N"));
        assert_eq!(selected.discarded_value_honor_count, 0);
        assert_eq!(selected.min_shanten_after_discard(), 0);
    }

    #[test]
    fn context_double_wind_kept_over_single_value_honor() {
        // 東場東家。中(単役牌) と 東(ダブル東) の孤立牌があるとき、東を温存し中を切る
        let tiles = ids(&[0, 4, 8, 12, 17, 20, 24, 28, 32, 36, 40, 44, 132, 108]);
        let selected = select_best_discard_from_tiles_with_context(
            &tiles,
            &[],
            Some(tile("E")),
            Some(tile("E")),
        )
        .unwrap();
        assert_eq!(selected.discard, tile("C"));
        assert_eq!(selected.discarded_value_honor_count, 1);
    }

    #[test]
    fn context_dora_outranks_value_honor() {
        // 中(役牌・非ドラ) と 北(客風・ドラ) の孤立牌。ドラを温存し役牌の中を切る
        // ドラ表示 西 -> 北 がドラ
        let tiles = ids(&[0, 4, 8, 12, 17, 20, 24, 28, 32, 36, 40, 44, 132, 120]);
        let indicators = ids(&[116]);
        let selected = select_best_discard_from_tiles_with_context(
            &tiles,
            &indicators,
            Some(tile("E")),
            Some(tile("S")),
        )
        .unwrap();
        assert_eq!(selected.discard, tile("C"));
        assert_eq!(selected.discarded_value_honor_count, 1);
    }

    #[test]
    fn with_context_none_winds_matches_with_dora() {
        let tiles = ids(&[0, 4, 8, 12, 17, 20, 24, 28, 32, 36, 40, 44, 132, 108]);
        let with_context = evaluate_discards_from_tiles_with_context(&tiles, &[], None, None);
        let with_dora = evaluate_discards_from_tiles_with_dora(&tiles, &[]);
        assert_eq!(with_context, with_dora);
    }

    #[test]
    fn from_tiles_marks_forced_red_five_discard() {
        let tiles = ids(&[0, 16, 32, 36, 53, 68, 72, 89, 104, 108]);
        let evaluations = evaluate_discards_from_tiles(&tiles);
        let five = evaluations
            .iter()
            .find(|evaluation| evaluation.discard == tile("5m"))
            .unwrap();
        assert!(five.discards_red_five);
        let one_man = evaluations
            .iter()
            .find(|evaluation| evaluation.discard == tile("1m"))
            .unwrap();
        assert!(!one_man.discards_red_five);
    }

    #[test]
    fn from_tiles_does_not_mark_when_black_copy_present() {
        let tiles = ids(&[16, 17, 0, 8]);
        let evaluations = evaluate_discards_from_tiles(&tiles);
        let five = evaluations
            .iter()
            .find(|evaluation| evaluation.discard == tile("5m"))
            .unwrap();
        assert!(!five.discards_red_five);
    }

    #[test]
    fn from_tiles_tie_break_keeps_lone_red_five() {
        let tiles = ids(&[0, 16, 32, 36, 53, 68, 72, 89, 104, 108]);
        let selected = select_best_discard_from_tiles(&tiles).unwrap();
        assert!(!selected.discards_red_five);
        assert_ne!(selected.discard, tile("5m"));
    }

    #[test]
    fn from_tiles_shanten_outranks_red_five() {
        let tiles = ids(&[40, 44, 48, 56, 60, 64, 76, 80, 84, 108, 109, 96, 100, 16]);
        let selected = select_best_discard_from_tiles(&tiles).unwrap();
        assert_eq!(selected.discard, tile("5m"));
        assert!(selected.discards_red_five);
        assert_eq!(selected.min_shanten_after_discard(), 0);
    }

    #[test]
    fn from_tiles_without_red_matches_tile_counts_behavior() {
        let tiles = ids(&[0, 17, 32, 36, 53, 68, 72, 89, 104, 108]);
        let from_tiles = select_best_discard_from_tiles(&tiles).unwrap();
        let counts = TileCounts::from_tiles(tiles.iter().copied());
        let from_counts = select_best_discard(&counts).unwrap();
        assert_eq!(from_tiles.discard, from_counts.discard);
        assert!(!from_tiles.discards_red_five);
    }

    #[test]
    fn from_tiles_empty_hand_has_no_selection() {
        assert_eq!(select_best_discard_from_tiles(&[]), None);
    }

    #[test]
    fn with_empty_dora_matches_from_tiles_behavior() {
        let tiles = ids(&[0, 17, 32, 36, 53, 68, 72, 89, 104, 108]);
        let with_dora = select_best_discard_from_tiles_with_dora(&tiles, &[]).unwrap();
        let without = select_best_discard_from_tiles(&tiles).unwrap();
        assert_eq!(with_dora, without);
    }

    #[test]
    fn empty_dora_indicators_yield_zero_dora_count() {
        let tiles = ids(&[0, 17, 32, 36, 53, 68, 72, 89, 104, 108]);
        let evaluations = evaluate_discards_from_tiles_with_dora(&tiles, &[]);
        assert!(
            evaluations
                .iter()
                .all(|evaluation| evaluation.discarded_dora_count == 0)
        );
    }

    #[test]
    fn normal_dora_discard_has_positive_dora_count() {
        // 1m 2m 3m 4m 5m 6m 7m 8m 9m 1p 2p 3p 1m(浮き) 1p(浮き), ドラ表示 9p -> 1p がドラ
        let tiles = ids(&[0, 4, 8, 12, 17, 20, 24, 28, 32, 36, 40, 44, 1, 37]);
        let indicators = ids(&[68]);
        let evaluations = evaluate_discards_from_tiles_with_dora(&tiles, &indicators);
        let dora_tile = evaluations
            .iter()
            .find(|evaluation| evaluation.discard == tile("1p"))
            .unwrap();
        assert!(dora_tile.discarded_dora_count > 0);
        let non_dora = evaluations
            .iter()
            .find(|evaluation| evaluation.discard == tile("1m"))
            .unwrap();
        assert_eq!(non_dora.discarded_dora_count, 0);
    }

    #[test]
    fn lone_red_five_counts_red_dora() {
        let tiles = ids(&[0, 16, 32, 36, 53, 68, 72, 89, 104, 108]);
        let evaluations = evaluate_discards_from_tiles_with_dora(&tiles, &[]);
        let five = evaluations
            .iter()
            .find(|evaluation| evaluation.discard == tile("5m"))
            .unwrap();
        assert_eq!(five.discarded_dora_count, 1);
        assert!(five.discards_red_five);
    }

    #[test]
    fn black_five_present_does_not_count_red_dora() {
        let tiles = ids(&[16, 17, 0, 8]);
        let evaluations = evaluate_discards_from_tiles_with_dora(&tiles, &[]);
        let five = evaluations
            .iter()
            .find(|evaluation| evaluation.discard == tile("5m"))
            .unwrap();
        assert_eq!(five.discarded_dora_count, 0);
        assert!(!five.discards_red_five);
    }

    #[test]
    fn perfect_tie_prefers_keeping_dora() {
        // 123m 456m 789m 123p + 東(浮き) 西(浮き), ドラ表示 南 -> 西 がドラ
        // 東と西のどちらを切っても同じ単騎テンパイになり、ドラでない東が優先される
        let tiles = ids(&[0, 4, 8, 12, 17, 20, 24, 28, 32, 36, 40, 44, 108, 116]);
        let indicators = ids(&[112]);
        let selected = select_best_discard_from_tiles_with_dora(&tiles, &indicators).unwrap();
        assert_eq!(selected.discard, tile("E"));
        assert_eq!(selected.discarded_dora_count, 0);
        assert_eq!(selected.min_shanten_after_discard(), 0);
    }

    #[test]
    fn shanten_outranks_keeping_dora() {
        // 5m を切るとテンパイになる形。5m がドラでも向聴を優先して切る
        let tiles = ids(&[40, 44, 48, 56, 60, 64, 76, 80, 84, 108, 109, 96, 100, 16]);
        let indicators = ids(&[12]);
        let selected = select_best_discard_from_tiles_with_dora(&tiles, &indicators).unwrap();
        assert_eq!(selected.discard, tile("5m"));
        assert!(selected.discarded_dora_count > 0);
        assert_eq!(selected.min_shanten_after_discard(), 0);
    }

    fn discard_evaluation(
        evaluations: &[DiscardEvaluation],
        discard: TileType,
    ) -> &DiscardEvaluation {
        evaluations
            .iter()
            .find(|evaluation| evaluation.discard == discard)
            .expect("discard candidate should exist")
    }

    fn acceptance_remaining(evaluation: &DiscardEvaluation, wait: TileType) -> Option<u8> {
        evaluation
            .acceptance_after_discard
            .tiles
            .iter()
            .find(|entry| entry.tile == wait)
            .map(|entry| entry.remaining)
    }

    #[test]
    fn visible_tiles_empty_matches_plain_evaluate_discards() {
        let counts = counts(&[
            "1m", "2m", "3m", "4m", "5m", "6m", "7m", "8m", "9m", "1p", "2p", "3p", "5s", "5s",
        ]);
        assert_eq!(
            evaluate_discards_with_visible_tiles(&counts, &[]),
            evaluate_discards(&counts)
        );
    }

    #[test]
    fn select_best_with_empty_visible_tiles_matches_plain_select() {
        let counts = counts(&[
            "1m", "2m", "3m", "4m", "5m", "6m", "7m", "8m", "9m", "1p", "2p", "3p", "5s", "W",
        ]);
        assert_eq!(
            select_best_discard_with_visible_tiles(&counts, &[]),
            select_best_discard(&counts)
        );
    }

    #[test]
    fn does_not_double_count_own_hand_tiles() {
        let hand = ids(&[0, 4, 8, 12, 17, 20, 24, 28, 32, 36, 40, 44, 108, 109]);
        let counts = TileCounts::from_tiles(hand.iter().copied());
        let evaluations = evaluate_discards_with_visible_tiles(&counts, &hand);
        let east = discard_evaluation(&evaluations, tile("E"));
        assert_eq!(acceptance_remaining(east, tile("E")), Some(2));
    }

    #[test]
    fn candidate_discard_is_counted_as_visible_after_discard() {
        let hand = ids(&[0, 4, 8, 12, 17, 20, 24, 28, 32, 36, 40, 44, 108, 109]);
        let counts = TileCounts::from_tiles(hand.iter().copied());

        let plain_evaluations = evaluate_discards(&counts);
        let plain = discard_evaluation(&plain_evaluations, tile("E"));
        let plain_remaining = acceptance_remaining(plain, tile("E")).unwrap();

        let visible_evaluations = evaluate_discards_with_visible_tiles(&counts, &hand);
        let visible = discard_evaluation(&visible_evaluations, tile("E"));
        let visible_remaining = acceptance_remaining(visible, tile("E")).unwrap();

        assert_eq!(plain_remaining, 3);
        assert_eq!(visible_remaining, plain_remaining - 1);
    }

    #[test]
    fn single_visible_wait_tile_reduces_remaining_by_one() {
        let hand = ids(&[0, 4, 8, 12, 17, 20, 24, 28, 32, 53, 54, 72, 80, 108]);
        let counts = TileCounts::from_tiles(hand.iter().copied());

        let mut visible = hand.clone();
        let baseline = discard_evaluation(
            &evaluate_discards_with_visible_tiles(&counts, &visible),
            tile("E"),
        )
        .clone();
        assert_eq!(acceptance_remaining(&baseline, tile("2s")), Some(4));

        visible.extend(ids(&[76]));
        let reduced = discard_evaluation(
            &evaluate_discards_with_visible_tiles(&counts, &visible),
            tile("E"),
        )
        .clone();
        assert_eq!(acceptance_remaining(&reduced, tile("2s")), Some(3));
    }

    #[test]
    fn multiple_visible_wait_tiles_reduce_remaining_by_count() {
        let hand = ids(&[0, 4, 8, 12, 17, 20, 24, 28, 32, 53, 54, 72, 80, 108]);
        let counts = TileCounts::from_tiles(hand.iter().copied());

        let mut visible = hand.clone();
        visible.extend(ids(&[76, 77]));
        let evaluation = discard_evaluation(
            &evaluate_discards_with_visible_tiles(&counts, &visible),
            tile("E"),
        )
        .clone();
        assert_eq!(acceptance_remaining(&evaluation, tile("2s")), Some(2));
    }

    #[test]
    fn fully_visible_wait_tile_is_excluded_from_acceptance() {
        let hand = ids(&[0, 4, 8, 12, 17, 20, 24, 28, 32, 53, 54, 72, 80, 108]);
        let counts = TileCounts::from_tiles(hand.iter().copied());

        let mut visible = hand.clone();
        visible.extend(ids(&[76, 77, 78, 79]));
        let evaluation = discard_evaluation(
            &evaluate_discards_with_visible_tiles(&counts, &visible),
            tile("E"),
        )
        .clone();
        assert_eq!(acceptance_remaining(&evaluation, tile("2s")), None);
        assert_eq!(evaluation.acceptance_total_remaining(), 0);
        assert_eq!(evaluation.acceptance_type_count(), 0);
    }

    #[test]
    fn shanten_is_preferred_over_visible_correction() {
        let hand = ids(&[0, 4, 8, 12, 17, 20, 24, 28, 32, 80, 84, 40, 41, 108]);
        let counts = TileCounts::from_tiles(hand.iter().copied());

        let mut visible = hand.clone();
        visible.extend(ids(&[76, 77, 78, 79, 88, 89, 90, 91]));

        let selected = select_best_discard_with_visible_tiles(&counts, &visible).unwrap();
        assert_eq!(selected.discard, tile("E"));
        assert_eq!(selected.min_shanten_after_discard(), 0);
    }

    #[test]
    fn visible_correction_changes_choice_between_same_shanten_candidates() {
        let hand = ids(&[0, 4, 8, 12, 17, 20, 24, 28, 32, 48, 53, 56, 36, 68]);
        let counts = TileCounts::from_tiles(hand.iter().copied());

        assert_eq!(
            select_best_discard_with_visible_tiles(&counts, &[])
                .unwrap()
                .discard,
            select_best_discard(&counts).unwrap().discard
        );
        assert_eq!(select_best_discard(&counts).unwrap().discard, tile("1p"));

        let mut visible = hand.clone();
        visible.extend(ids(&[69, 70, 71]));
        let selected = select_best_discard_with_visible_tiles(&counts, &visible).unwrap();
        assert_eq!(selected.discard, tile("9p"));
    }

    #[test]
    fn shape_breakdown_absent_discard_returns_default() {
        let counts = counts(&["1m", "2m"]);
        assert_eq!(
            shape_breakdown_for_discard(&counts, tile("9s")),
            ShapeBreakdown::default()
        );
    }

    #[test]
    fn shape_breakdown_honor_single_has_no_adjacent_shapes() {
        let counts = counts(&["E"]);
        let breakdown = shape_breakdown_for_discard(&counts, tile("E"));
        assert!(!breakdown.breaks_pair);
        assert!(!breakdown.breaks_ryanmen);
        assert!(!breakdown.breaks_kanchan);
        assert!(!breakdown.breaks_penchan);
        assert!(!breakdown.breaks_sequence);
        assert_eq!(breakdown.adjacent_count, 0);
        assert_eq!(breakdown.same_type_count, 1);
        assert_eq!(shape_penalty_for_discard(&counts, tile("E")), 0);
    }

    #[test]
    fn shape_breakdown_honor_pair_breaks_pair() {
        let counts = counts(&["E", "E"]);
        let breakdown = shape_breakdown_for_discard(&counts, tile("E"));
        assert!(breakdown.breaks_pair);
        assert_eq!(breakdown.same_type_count, 2);
        assert!(!breakdown.breaks_ryanmen);
        assert!(!breakdown.breaks_kanchan);
        assert!(!breakdown.breaks_penchan);
        assert!(!breakdown.breaks_sequence);
        assert_eq!(breakdown.adjacent_count, 0);
    }

    #[test]
    fn shape_breakdown_number_pair_breaks_pair() {
        let counts = counts(&["5m", "5m"]);
        let breakdown = shape_breakdown_for_discard(&counts, tile("5m"));
        assert!(breakdown.breaks_pair);
        assert_eq!(breakdown.same_type_count, 2);
    }

    #[test]
    fn shape_breakdown_penchan_one_two() {
        let counts = counts(&["1m", "2m"]);
        assert!(shape_breakdown_for_discard(&counts, tile("1m")).breaks_penchan);
        assert!(shape_breakdown_for_discard(&counts, tile("2m")).breaks_penchan);
        assert!(!shape_breakdown_for_discard(&counts, tile("1m")).breaks_ryanmen);
        assert!(!shape_breakdown_for_discard(&counts, tile("2m")).breaks_ryanmen);
    }

    #[test]
    fn shape_breakdown_penchan_eight_nine() {
        let counts = counts(&["8m", "9m"]);
        assert!(shape_breakdown_for_discard(&counts, tile("8m")).breaks_penchan);
        assert!(shape_breakdown_for_discard(&counts, tile("9m")).breaks_penchan);
        assert!(!shape_breakdown_for_discard(&counts, tile("8m")).breaks_ryanmen);
        assert!(!shape_breakdown_for_discard(&counts, tile("9m")).breaks_ryanmen);
    }

    #[test]
    fn shape_breakdown_ryanmen_two_three() {
        let counts = counts(&["2m", "3m"]);
        assert!(shape_breakdown_for_discard(&counts, tile("2m")).breaks_ryanmen);
        assert!(shape_breakdown_for_discard(&counts, tile("3m")).breaks_ryanmen);
        assert!(!shape_breakdown_for_discard(&counts, tile("2m")).breaks_penchan);
        assert!(!shape_breakdown_for_discard(&counts, tile("3m")).breaks_penchan);
    }

    #[test]
    fn shape_breakdown_ryanmen_seven_eight() {
        let counts = counts(&["7m", "8m"]);
        assert!(shape_breakdown_for_discard(&counts, tile("7m")).breaks_ryanmen);
        assert!(shape_breakdown_for_discard(&counts, tile("8m")).breaks_ryanmen);
    }

    #[test]
    fn shape_breakdown_ryanmen_four_five() {
        let counts = counts(&["4m", "5m"]);
        assert!(shape_breakdown_for_discard(&counts, tile("4m")).breaks_ryanmen);
        assert!(shape_breakdown_for_discard(&counts, tile("5m")).breaks_ryanmen);
    }

    #[test]
    fn shape_breakdown_kanchan_one_three() {
        let counts = counts(&["1m", "3m"]);
        assert!(shape_breakdown_for_discard(&counts, tile("1m")).breaks_kanchan);
        assert!(shape_breakdown_for_discard(&counts, tile("3m")).breaks_kanchan);
        assert!(!shape_breakdown_for_discard(&counts, tile("1m")).breaks_ryanmen);
    }

    #[test]
    fn shape_breakdown_kanchan_four_six() {
        let counts = counts(&["4m", "6m"]);
        assert!(shape_breakdown_for_discard(&counts, tile("4m")).breaks_kanchan);
        assert!(shape_breakdown_for_discard(&counts, tile("6m")).breaks_kanchan);
    }

    #[test]
    fn shape_breakdown_sequence_on_middle() {
        assert!(
            shape_breakdown_for_discard(&counts(&["1m", "2m", "3m"]), tile("2m")).breaks_sequence
        );
        assert!(
            shape_breakdown_for_discard(&counts(&["3m", "4m", "5m"]), tile("4m")).breaks_sequence
        );
    }

    #[test]
    fn shape_breakdown_sequence_on_terminal() {
        assert!(
            shape_breakdown_for_discard(&counts(&["7m", "8m", "9m"]), tile("9m")).breaks_sequence
        );
    }

    #[test]
    fn shape_breakdown_adjacent_count_covers_plus_minus_one_and_two() {
        let counts = counts(&["3m", "4m", "5m", "6m", "7m"]);
        let breakdown = shape_breakdown_for_discard(&counts, tile("5m"));
        assert_eq!(breakdown.adjacent_count, 4);
    }

    #[test]
    fn shape_breakdown_adjacent_count_counts_tile_types_not_copies() {
        let counts = counts(&["3m", "3m", "5m", "7m"]);
        let breakdown = shape_breakdown_for_discard(&counts, tile("5m"));
        assert_eq!(breakdown.adjacent_count, 2);
    }

    #[test]
    fn shape_breakdown_adjacent_count_ignores_other_suits() {
        let counts = counts(&["5m", "4p", "6p", "4s", "6s"]);
        let breakdown = shape_breakdown_for_discard(&counts, tile("5m"));
        assert_eq!(breakdown.adjacent_count, 0);
    }

    #[test]
    fn shape_breakdown_same_type_count_reflects_count_before_discard() {
        let counts = counts(&["5m", "5m", "5m"]);
        let breakdown = shape_breakdown_for_discard(&counts, tile("5m"));
        assert_eq!(breakdown.same_type_count, 3);
        assert!(breakdown.breaks_pair);
    }

    #[test]
    fn shape_penalty_orders_shapes_by_severity() {
        let sequence = shape_penalty_for_discard(&counts(&["1m", "2m", "3m"]), tile("2m"));
        let ryanmen = shape_penalty_for_discard(&counts(&["4m", "5m"]), tile("4m"));
        let pair = shape_penalty_for_discard(&counts(&["5m", "5m"]), tile("5m"));
        let kanchan = shape_penalty_for_discard(&counts(&["1m", "3m"]), tile("1m"));
        let penchan = shape_penalty_for_discard(&counts(&["1m", "2m"]), tile("1m"));
        assert!(sequence > ryanmen);
        assert!(ryanmen > pair);
        assert!(pair > kanchan);
        assert!(kanchan > penchan);
    }

    #[test]
    fn sequence_penalty_present_for_simple_sequence() {
        assert!(shape_penalty_for_discard(&counts(&["3m", "4m", "5m"]), tile("3m")) > 0);
        assert!(shape_penalty_for_discard(&counts(&["4m", "5m", "6m"]), tile("4m")) > 0);
        assert!(
            shape_breakdown_for_discard(&counts(&["3m", "4m", "5m"]), tile("3m")).breaks_sequence
        );
        assert!(
            shape_breakdown_for_discard(&counts(&["4m", "5m", "6m"]), tile("4m")).breaks_sequence
        );
    }

    #[test]
    fn ryanmen_penalty_present_for_simple_ryanmen() {
        assert!(shape_penalty_for_discard(&counts(&["2m", "3m"]), tile("2m")) > 0);
        assert!(shape_penalty_for_discard(&counts(&["7m", "8m"]), tile("8m")) > 0);
        assert!(shape_breakdown_for_discard(&counts(&["2m", "3m"]), tile("2m")).breaks_ryanmen);
        assert!(shape_breakdown_for_discard(&counts(&["7m", "8m"]), tile("8m")).breaks_ryanmen);
    }

    #[test]
    fn redundant_third_tile_keeps_sequence_penalty_lower() {
        // 3m3m4m5m の 3m は 1枚切っても 345m が残る
        let redundant = shape_penalty_for_discard(&counts(&["3m", "3m", "4m", "5m"]), tile("3m"));
        let plain = shape_penalty_for_discard(&counts(&["3m", "4m", "5m"]), tile("3m"));
        assert!(redundant < plain);
        assert!(
            shape_breakdown_for_discard(&counts(&["3m", "3m", "4m", "5m"]), tile("3m"))
                .preserves_sequence_after_discard
        );
    }

    #[test]
    fn redundant_upper_tile_keeps_sequence_penalty_lower() {
        // 3m4m5m5m の 5m は 1枚切っても 345m が残る
        let redundant = shape_penalty_for_discard(&counts(&["3m", "4m", "5m", "5m"]), tile("5m"));
        let plain = shape_penalty_for_discard(&counts(&["3m", "4m", "5m"]), tile("5m"));
        assert!(redundant < plain);
    }

    #[test]
    fn redundant_lower_tile_keeps_sequence_penalty_lower() {
        // 4m4m5m6m の 4m は 1枚切っても 456m が残る
        let redundant = shape_penalty_for_discard(&counts(&["4m", "4m", "5m", "6m"]), tile("4m"));
        let plain = shape_penalty_for_discard(&counts(&["4m", "5m", "6m"]), tile("4m"));
        assert!(redundant < plain);
    }

    #[test]
    fn redundant_tile_keeps_ryanmen_penalty_lower() {
        // 2m2m3m の 2m は 1枚切っても 2m3m 両面が残る
        let redundant_low = shape_penalty_for_discard(&counts(&["2m", "2m", "3m"]), tile("2m"));
        let plain_low = shape_penalty_for_discard(&counts(&["2m", "3m"]), tile("2m"));
        assert!(redundant_low < plain_low);
        // 7m8m8m の 8m は 1枚切っても 7m8m 両面が残る
        let redundant_high = shape_penalty_for_discard(&counts(&["7m", "8m", "8m"]), tile("8m"));
        let plain_high = shape_penalty_for_discard(&counts(&["7m", "8m"]), tile("8m"));
        assert!(redundant_high < plain_high);
        assert!(
            shape_breakdown_for_discard(&counts(&["2m", "2m", "3m"]), tile("2m"))
                .preserves_ryanmen_after_discard
        );
    }

    #[test]
    fn only_pair_candidate_is_heavier_than_base() {
        // 唯一の対子候補を壊すと、ヘッドを失うため対子20に唯一対子8を加える
        // さらに推定ブロックが減り5ブロック未満になるため +10
        assert_eq!(
            shape_penalty_for_discard(&counts(&["5m", "5m"]), tile("5m")),
            38
        );
    }

    #[test]
    fn same_type_two_relief_applies_in_complex_shape() {
        // 5m5m6m の 5m は 1枚切っても 5m6m 両面が残る余剰対子
        // 主要形が残るため唯一対子 penalty は加えない
        // 対子20 + 両面30 + 隣接3 - 両面存続15 - 同種2枚8 = 30
        // さらに推定ブロックが減り5ブロック未満になるため +10 で 40
        let redundant = shape_penalty_for_discard(&counts(&["5m", "5m", "6m"]), tile("5m"));
        assert_eq!(redundant, 40);
        let plain = shape_penalty_for_discard(&counts(&["5m", "6m"]), tile("5m"));
        assert!(redundant < plain);
    }

    #[test]
    fn number_triplet_penalty_is_heavier_than_pair() {
        // 数牌刻子は完成面子なので刻子破壊 +35 を加える
        // 対子20 + 同種3枚 +10 + 刻子35 - 対子存続12 + ブロック補正10 = 63
        assert_eq!(
            shape_penalty_for_discard(&counts(&["5m", "5m", "5m"]), tile("5m")),
            63
        );
        // 数牌刻子は完成面子なので、対子を壊すより重くする
        assert!(
            shape_penalty_for_discard(&counts(&["5m", "5m", "5m"]), tile("5m"))
                > shape_penalty_for_discard(&counts(&["5m", "5m"]), tile("5m"))
        );
    }

    #[test]
    fn shape_penalty_never_negative() {
        for hand in [
            counts(&["2m", "2m", "3m"]),
            counts(&["3m", "3m", "4m", "5m"]),
            counts(&["5m", "5m"]),
            counts(&["E", "E"]),
        ] {
            for tile in TileType::all() {
                assert!(shape_penalty_for_discard(&hand, tile) >= 0);
            }
        }
    }

    #[test]
    fn honor_single_penalty_stays_zero() {
        assert_eq!(shape_penalty_for_discard(&counts(&["E"]), tile("E")), 0);
    }

    #[test]
    fn honor_pair_penalty_positive() {
        // 字牌対子も唯一の対子候補なら対子20に唯一対子8を加える
        // さらに推定ブロックが減り5ブロック未満になるため +10 で 38
        let penalty = shape_penalty_for_discard(&counts(&["E", "E"]), tile("E"));
        assert!(penalty > 0);
        assert_eq!(penalty, 38);
    }

    #[test]
    fn lower_shape_penalty_does_not_override_shanten_or_acceptance() {
        let low_penalty_worse_shanten = evaluation_with_shape_penalty(1, 40, 5, 0, 0, 0, false);
        let high_penalty_better_shanten = evaluation_with_shape_penalty(0, 4, 1, 40, 0, 0, false);
        assert!(is_better_discard(
            &high_penalty_better_shanten,
            &low_penalty_worse_shanten
        ));

        let low_penalty_less_remaining = evaluation_with_shape_penalty(1, 10, 1, 0, 0, 0, false);
        let high_penalty_more_remaining = evaluation_with_shape_penalty(1, 20, 1, 40, 0, 0, false);
        assert!(is_better_discard(
            &high_penalty_more_remaining,
            &low_penalty_less_remaining
        ));

        let low_penalty_fewer_types = evaluation_with_shape_penalty(1, 10, 2, 0, 0, 0, false);
        let high_penalty_more_types = evaluation_with_shape_penalty(1, 10, 3, 40, 0, 0, false);
        assert!(is_better_discard(
            &high_penalty_more_types,
            &low_penalty_fewer_types
        ));
    }

    fn ryanmen_penalty() -> i16 {
        shape_penalty_for_discard(&counts(&["4m", "5m"]), tile("4m"))
    }

    fn sequence_penalty() -> i16 {
        shape_penalty_for_discard(&counts(&["1m", "2m", "3m"]), tile("2m"))
    }

    fn pair_penalty() -> i16 {
        shape_penalty_for_discard(&counts(&["5m", "5m"]), tile("5m"))
    }

    fn isolated_penalty() -> i16 {
        shape_penalty_for_discard(&counts(&["9p"]), tile("9p"))
    }

    #[test]
    fn shape_penalty_tiebreak_prefers_lower_penalty() {
        let low = evaluation_with_shape_penalty(1, 10, 2, 3, 0, 0, false);
        let high = evaluation_with_shape_penalty(1, 10, 2, 40, 0, 0, false);
        assert!(is_better_discard(&low, &high));
        assert!(!is_better_discard(&high, &low));
    }

    #[test]
    fn shanten_outranks_shape_penalty_tiebreak() {
        let low_shanten_high_penalty = evaluation_with_shape_penalty(0, 4, 1, 40, 0, 0, false);
        let high_shanten_low_penalty = evaluation_with_shape_penalty(1, 40, 5, 0, 0, 0, false);
        assert!(is_better_discard(
            &low_shanten_high_penalty,
            &high_shanten_low_penalty
        ));
    }

    #[test]
    fn acceptance_remaining_outranks_shape_penalty_tiebreak() {
        let more_remaining_high_penalty = evaluation_with_shape_penalty(1, 20, 1, 40, 0, 0, false);
        let less_remaining_low_penalty = evaluation_with_shape_penalty(1, 10, 1, 0, 0, 0, false);
        assert!(is_better_discard(
            &more_remaining_high_penalty,
            &less_remaining_low_penalty
        ));
    }

    #[test]
    fn acceptance_types_outrank_shape_penalty_tiebreak() {
        let more_types_high_penalty = evaluation_with_shape_penalty(1, 10, 3, 40, 0, 0, false);
        let fewer_types_low_penalty = evaluation_with_shape_penalty(1, 10, 2, 0, 0, 0, false);
        assert!(is_better_discard(
            &more_types_high_penalty,
            &fewer_types_low_penalty
        ));
    }

    #[test]
    fn shape_penalty_outranks_dora_tiebreak() {
        let low_penalty_discards_dora = evaluation_with_shape_penalty(1, 10, 2, 0, 1, 0, false);
        let high_penalty_keeps_dora = evaluation_with_shape_penalty(1, 10, 2, 33, 0, 0, false);
        assert!(is_better_discard(
            &low_penalty_discards_dora,
            &high_penalty_keeps_dora
        ));
    }

    #[test]
    fn shape_penalty_outranks_value_honor_tiebreak() {
        let low_penalty_discards_honor = evaluation_with_shape_penalty(1, 10, 2, 0, 0, 1, false);
        let high_penalty_keeps_honor = evaluation_with_shape_penalty(1, 10, 2, 33, 0, 0, false);
        assert!(is_better_discard(
            &low_penalty_discards_honor,
            &high_penalty_keeps_honor
        ));
    }

    #[test]
    fn shape_penalty_outranks_red_five_tiebreak() {
        let low_penalty_discards_red = evaluation_with_shape_penalty(1, 10, 2, 0, 0, 0, true);
        let high_penalty_keeps_red = evaluation_with_shape_penalty(1, 10, 2, 33, 0, 0, false);
        assert!(is_better_discard(
            &low_penalty_discards_red,
            &high_penalty_keeps_red
        ));
    }

    #[test]
    fn tiebreak_prefers_isolated_over_breaking_ryanmen() {
        let isolated = evaluation_with_shape_penalty(1, 10, 2, isolated_penalty(), 0, 0, false);
        let breaks_ryanmen =
            evaluation_with_shape_penalty(1, 10, 2, ryanmen_penalty(), 0, 0, false);
        assert!(is_better_discard(&isolated, &breaks_ryanmen));
        assert!(!is_better_discard(&breaks_ryanmen, &isolated));
    }

    #[test]
    fn tiebreak_prefers_isolated_over_breaking_sequence() {
        let isolated = evaluation_with_shape_penalty(1, 10, 2, isolated_penalty(), 0, 0, false);
        let breaks_sequence =
            evaluation_with_shape_penalty(1, 10, 2, sequence_penalty(), 0, 0, false);
        assert!(is_better_discard(&isolated, &breaks_sequence));
        assert!(!is_better_discard(&breaks_sequence, &isolated));
    }

    #[test]
    fn tiebreak_prefers_isolated_over_breaking_pair() {
        let isolated = evaluation_with_shape_penalty(1, 10, 2, isolated_penalty(), 0, 0, false);
        let breaks_pair = evaluation_with_shape_penalty(1, 10, 2, pair_penalty(), 0, 0, false);
        assert!(is_better_discard(&isolated, &breaks_pair));
        assert!(!is_better_discard(&breaks_pair, &isolated));
    }

    #[test]
    fn evaluate_discards_sets_shape_penalty_from_counts_before_discard() {
        let counts = counts(&["4m", "5m", "9p"]);
        let evaluations = evaluate_discards(&counts);
        let four = discard_evaluation(&evaluations, tile("4m"));
        assert_eq!(
            four.shape_penalty,
            shape_penalty_for_discard(&counts, tile("4m"))
        );
        assert!(four.shape_penalty > 0);
        let nine = discard_evaluation(&evaluations, tile("9p"));
        assert_eq!(nine.shape_penalty, 0);
    }

    #[test]
    fn evaluate_discards_with_visible_tiles_sets_shape_penalty() {
        let hand = ids(&[0, 4, 8, 12, 17, 20, 24, 28, 32, 36, 40, 44, 132, 37]);
        let counts = TileCounts::from_tiles(hand.iter().copied());
        let evaluations = evaluate_discards_with_visible_tiles(&counts, &hand);
        assert!(!evaluations.is_empty());
        for evaluation in &evaluations {
            assert_eq!(
                evaluation.shape_penalty,
                shape_penalty_for_discard(&counts, evaluation.discard)
            );
        }
    }

    #[test]
    fn from_tiles_preserves_shape_penalty_after_decorate() {
        let tiles = ids(&[0, 4, 8, 12, 17, 20, 24, 28, 32, 36, 40, 44, 132, 37]);
        let counts = TileCounts::from_tiles(tiles.iter().copied());
        let with_context = evaluate_discards_from_tiles_with_context(
            &tiles,
            &[],
            Some(tile("E")),
            Some(tile("S")),
        );
        let with_visible = evaluate_discards_from_tiles_with_visible_tiles(
            &tiles,
            &[],
            Some(tile("E")),
            Some(tile("S")),
            &tiles,
        );
        assert!(!with_context.is_empty());
        for evaluation in &with_context {
            assert_eq!(
                evaluation.shape_penalty,
                shape_penalty_for_discard(&counts, evaluation.discard)
            );
        }
        for evaluation in &with_visible {
            assert_eq!(
                evaluation.shape_penalty,
                shape_penalty_for_discard(&counts, evaluation.discard)
            );
        }
    }

    fn evaluation_with_floating(
        min: i8,
        remaining: u8,
        type_count: usize,
        shape_penalty: i16,
        floating: i16,
    ) -> DiscardEvaluation {
        let mut evaluation =
            evaluation_with_shape_penalty(min, remaining, type_count, shape_penalty, 0, 0, false);
        evaluation.floating_tile_value = floating;
        evaluation
    }

    #[test]
    fn floating_absent_discard_is_zero() {
        let counts = counts(&["1m", "2m"]);
        assert_eq!(floating_tile_value_for_discard(&counts, tile("9s")), 0);
        let breakdown = floating_tile_value_breakdown_for_discard(&counts, tile("9s"));
        assert_eq!(breakdown, FloatingTileValue::default());
        assert!(!breakdown.is_isolated);
    }

    #[test]
    fn floating_lone_honor_is_zero() {
        let counts = counts(&["E"]);
        assert_eq!(floating_tile_value_for_discard(&counts, tile("E")), 0);
        let breakdown = floating_tile_value_breakdown_for_discard(&counts, tile("E"));
        assert!(breakdown.is_isolated);
        assert_eq!(breakdown.value, 0);
    }

    #[test]
    fn floating_pair_is_not_isolated() {
        let counts = counts(&["4m", "4m"]);
        assert_eq!(floating_tile_value_for_discard(&counts, tile("4m")), 0);
        assert!(!floating_tile_value_breakdown_for_discard(&counts, tile("4m")).is_isolated);
    }

    #[test]
    fn floating_triplet_is_not_isolated() {
        let counts = counts(&["4m", "4m", "4m"]);
        assert_eq!(floating_tile_value_for_discard(&counts, tile("4m")), 0);
    }

    #[test]
    fn floating_tile_with_neighbor_is_not_isolated() {
        let plus_one = counts(&["4m", "5m"]);
        assert_eq!(floating_tile_value_for_discard(&plus_one, tile("4m")), 0);
        let minus_one = counts(&["4m", "3m"]);
        assert_eq!(floating_tile_value_for_discard(&minus_one, tile("4m")), 0);
        let plus_two = counts(&["4m", "6m"]);
        assert_eq!(floating_tile_value_for_discard(&plus_two, tile("4m")), 0);
        let minus_two = counts(&["4m", "2m"]);
        assert_eq!(floating_tile_value_for_discard(&minus_two, tile("4m")), 0);
    }

    #[test]
    fn floating_neighbor_in_other_suit_stays_isolated() {
        let counts = counts(&["4m", "3p", "5p", "4s"]);
        assert_eq!(floating_tile_value_for_discard(&counts, tile("4m")), 4);
        assert!(floating_tile_value_breakdown_for_discard(&counts, tile("4m")).is_isolated);
    }

    #[test]
    fn floating_isolated_terminals_value_one() {
        assert_eq!(
            floating_tile_value_for_discard(&counts(&["1m"]), tile("1m")),
            1
        );
        assert_eq!(
            floating_tile_value_for_discard(&counts(&["9s"]), tile("9s")),
            1
        );
    }

    #[test]
    fn floating_isolated_two_and_eight_value_two() {
        assert_eq!(
            floating_tile_value_for_discard(&counts(&["2p"]), tile("2p")),
            2
        );
        assert_eq!(
            floating_tile_value_for_discard(&counts(&["8m"]), tile("8m")),
            2
        );
    }

    #[test]
    fn floating_isolated_three_and_seven_value_three() {
        assert_eq!(
            floating_tile_value_for_discard(&counts(&["3s"]), tile("3s")),
            3
        );
        assert_eq!(
            floating_tile_value_for_discard(&counts(&["7p"]), tile("7p")),
            3
        );
    }

    #[test]
    fn floating_isolated_four_and_six_value_four() {
        assert_eq!(
            floating_tile_value_for_discard(&counts(&["4m"]), tile("4m")),
            4
        );
        assert_eq!(
            floating_tile_value_for_discard(&counts(&["6s"]), tile("6s")),
            4
        );
    }

    #[test]
    fn floating_isolated_five_value_five() {
        assert_eq!(
            floating_tile_value_for_discard(&counts(&["5p"]), tile("5p")),
            5
        );
    }

    #[test]
    fn evaluate_discards_sets_floating_from_counts_before_discard() {
        let counts = counts(&["1m", "5s", "E"]);
        let evaluations = evaluate_discards(&counts);
        let one = discard_evaluation(&evaluations, tile("1m"));
        assert_eq!(one.floating_tile_value, 1);
        let five = discard_evaluation(&evaluations, tile("5s"));
        assert_eq!(five.floating_tile_value, 5);
        let honor = discard_evaluation(&evaluations, tile("E"));
        assert_eq!(honor.floating_tile_value, 0);
    }

    #[test]
    fn evaluate_discards_with_visible_tiles_sets_floating() {
        let hand = ids(&[0, 4, 8, 12, 17, 20, 24, 28, 32, 36, 40, 44, 88, 132]);
        let counts = TileCounts::from_tiles(hand.iter().copied());
        let evaluations = evaluate_discards_with_visible_tiles(&counts, &hand);
        assert!(!evaluations.is_empty());
        for evaluation in &evaluations {
            assert_eq!(
                evaluation.floating_tile_value,
                floating_tile_value_for_discard(&counts, evaluation.discard)
            );
        }
    }

    #[test]
    fn from_tiles_preserves_floating_after_decorate() {
        let tiles = ids(&[0, 4, 8, 12, 17, 20, 24, 28, 32, 36, 40, 44, 88, 132]);
        let counts = TileCounts::from_tiles(tiles.iter().copied());
        let with_context = evaluate_discards_from_tiles_with_context(
            &tiles,
            &[],
            Some(tile("E")),
            Some(tile("S")),
        );
        let with_visible = evaluate_discards_from_tiles_with_visible_tiles(
            &tiles,
            &[],
            Some(tile("E")),
            Some(tile("S")),
            &tiles,
        );
        assert!(!with_context.is_empty());
        for evaluation in &with_context {
            assert_eq!(
                evaluation.floating_tile_value,
                floating_tile_value_for_discard(&counts, evaluation.discard)
            );
        }
        for evaluation in &with_visible {
            assert_eq!(
                evaluation.floating_tile_value,
                floating_tile_value_for_discard(&counts, evaluation.discard)
            );
        }
    }

    #[test]
    fn floating_tiebreak_prefers_lower_value() {
        let low = evaluation_with_floating(1, 10, 2, 0, 1);
        let high = evaluation_with_floating(1, 10, 2, 0, 5);
        assert!(is_better_discard(&low, &high));
        assert!(!is_better_discard(&high, &low));
    }

    #[test]
    fn discards_isolated_one_over_isolated_five() {
        // 123m 789m 123p 東東東 + 1s(浮き) 5s(浮き)
        // 同じ単騎テンパイ。孤立 5s より孤立 1s を切る
        let tiles = ids(&[0, 4, 8, 24, 28, 32, 36, 40, 44, 108, 109, 110, 72, 89]);
        let selected = select_best_discard_from_tiles(&tiles).unwrap();
        assert_eq!(selected.discard, tile("1s"));
        assert_eq!(selected.min_shanten_after_discard(), 0);
    }

    #[test]
    fn discards_isolated_nine_over_isolated_four() {
        // 123m 789m 123p 東東東 + 9p(浮き) 4s(浮き)
        // 同じ単騎テンパイ。孤立 4s より孤立 9p を切る
        let tiles = ids(&[0, 4, 8, 24, 28, 32, 36, 40, 44, 108, 109, 110, 68, 84]);
        let selected = select_best_discard_from_tiles(&tiles).unwrap();
        assert_eq!(selected.discard, tile("9p"));
        assert_eq!(selected.min_shanten_after_discard(), 0);
    }

    #[test]
    fn shape_penalty_outranks_floating_tiebreak() {
        let low_floating_high_penalty = evaluation_with_floating(1, 10, 2, 40, 1);
        let high_floating_low_penalty = evaluation_with_floating(1, 10, 2, 0, 5);
        assert!(is_better_discard(
            &high_floating_low_penalty,
            &low_floating_high_penalty
        ));
    }

    #[test]
    fn shanten_outranks_floating_tiebreak() {
        let low_shanten_high_floating = evaluation_with_floating(0, 4, 1, 0, 5);
        let high_shanten_low_floating = evaluation_with_floating(1, 40, 5, 0, 1);
        assert!(is_better_discard(
            &low_shanten_high_floating,
            &high_shanten_low_floating
        ));
    }

    #[test]
    fn acceptance_remaining_outranks_floating_tiebreak() {
        let more_remaining_high_floating = evaluation_with_floating(1, 20, 1, 0, 5);
        let less_remaining_low_floating = evaluation_with_floating(1, 10, 1, 0, 1);
        assert!(is_better_discard(
            &more_remaining_high_floating,
            &less_remaining_low_floating
        ));
    }

    #[test]
    fn acceptance_types_outrank_floating_tiebreak() {
        let more_types_high_floating = evaluation_with_floating(1, 10, 3, 0, 5);
        let fewer_types_low_floating = evaluation_with_floating(1, 10, 2, 0, 1);
        assert!(is_better_discard(
            &more_types_high_floating,
            &fewer_types_low_floating
        ));
    }

    #[test]
    fn floating_tiebreak_outranks_dora() {
        let mut low_floating_discards_dora = evaluation_with_floating(1, 10, 2, 0, 1);
        low_floating_discards_dora.discarded_dora_count = 1;
        let high_floating_keeps_dora = evaluation_with_floating(1, 10, 2, 0, 5);
        assert!(is_better_discard(
            &low_floating_discards_dora,
            &high_floating_keeps_dora
        ));
    }

    #[test]
    fn pair_context_absent_discard_returns_default() {
        let counts = counts(&["1m", "2m"]);
        assert_eq!(
            pair_context_for_discard(&counts, tile("9s")),
            PairContext::default()
        );
    }

    #[test]
    fn pair_context_counts_number_and_honor_pairs() {
        // 5m5m と EE の2種類の対子。5s は単騎
        let counts = counts(&["5m", "5m", "E", "E", "5s"]);
        let context = pair_context_for_discard(&counts, tile("5m"));
        assert_eq!(context.pair_like_type_count, 2);
        assert_eq!(context.other_pair_like_type_count, 1);
        assert!(!context.is_only_pair_candidate);
        assert!(!context.leaves_pair_after_discard);
    }

    #[test]
    fn pair_context_detects_only_pair_candidate() {
        let counts = counts(&["5m", "5m", "1p", "3s"]);
        let context = pair_context_for_discard(&counts, tile("5m"));
        assert_eq!(context.pair_like_type_count, 1);
        assert_eq!(context.other_pair_like_type_count, 0);
        assert!(context.is_only_pair_candidate);
        assert!(!context.leaves_pair_after_discard);
    }

    #[test]
    fn pair_context_triplet_leaves_pair_after_discard() {
        let counts = counts(&["5m", "5m", "5m"]);
        let context = pair_context_for_discard(&counts, tile("5m"));
        assert!(context.leaves_pair_after_discard);
        assert!(context.is_only_pair_candidate);
        assert_eq!(context.pair_like_type_count, 1);
    }

    #[test]
    fn pair_context_single_tile_discard_is_not_only_pair() {
        // 5m は単騎で、対子は EE のみ
        let counts = counts(&["5m", "E", "E"]);
        let context = pair_context_for_discard(&counts, tile("5m"));
        assert!(!context.is_only_pair_candidate);
        assert_eq!(context.pair_like_type_count, 1);
        assert_eq!(context.other_pair_like_type_count, 1);
    }

    #[test]
    fn breaking_only_pair_is_heavier_than_one_of_many() {
        let only_pair = shape_penalty_for_discard(&counts(&["5m", "5m"]), tile("5m"));
        let one_of_two = shape_penalty_for_discard(&counts(&["E", "E", "S", "S"]), tile("E"));
        assert!(only_pair > one_of_two);
    }

    #[test]
    fn breaking_pair_with_surplus_heads_is_lightest() {
        let one_of_two = shape_penalty_for_discard(&counts(&["E", "E", "S", "S"]), tile("E"));
        let one_of_three =
            shape_penalty_for_discard(&counts(&["E", "E", "S", "S", "W", "W"]), tile("E"));
        assert!(one_of_three < one_of_two);
    }

    #[test]
    fn triplet_discard_skips_only_pair_penalty() {
        // 暗刻から1枚落としても対子が残るため唯一対子 penalty は加えない
        // 対子20 + 同種3枚10 + 刻子35 - 対子存続12 + ブロック補正10 = 63
        let triplet = shape_penalty_for_discard(&counts(&["5m", "5m", "5m"]), tile("5m"));
        let only_pair = shape_penalty_for_discard(&counts(&["5m", "5m"]), tile("5m"));
        assert_eq!(triplet, 63);
        // 完成刻子は対子より重い
        assert!(triplet > only_pair);
    }

    #[test]
    fn only_pair_penalty_skipped_when_major_shape_survives() {
        // 2m2m3m の 2m は唯一の対子候補だが、切っても両面が残るため唯一対子 penalty は加えない
        // 対子20 + 両面30 + 隣接3 - 両面存続15 - 同種2枚8 = 30
        // さらに推定ブロックが減り5ブロック未満になるため +10 で 40
        assert_eq!(
            shape_penalty_for_discard(&counts(&["2m", "2m", "3m"]), tile("2m")),
            40
        );
    }

    #[test]
    fn pair_relief_never_makes_penalty_negative() {
        for hand in [
            counts(&["E", "E", "S", "S", "W", "W"]),
            counts(&["E", "E", "S", "S"]),
        ] {
            for tile in TileType::all() {
                assert!(shape_penalty_for_discard(&hand, tile) >= 0);
            }
        }
    }

    #[test]
    fn shape_breakdown_number_triplet_breaks_triplet() {
        let breakdown = shape_breakdown_for_discard(&counts(&["5m", "5m", "5m"]), tile("5m"));
        assert!(breakdown.breaks_triplet);
        assert!(!breakdown.breaks_honor_triplet);
    }

    #[test]
    fn shape_breakdown_honor_triplet_breaks_honor_triplet() {
        let breakdown = shape_breakdown_for_discard(&counts(&["E", "E", "E"]), tile("E"));
        assert!(breakdown.breaks_triplet);
        assert!(breakdown.breaks_honor_triplet);
    }

    #[test]
    fn shape_breakdown_honor_single_is_not_triplet() {
        let breakdown = shape_breakdown_for_discard(&counts(&["E"]), tile("E"));
        assert!(!breakdown.breaks_triplet);
        assert!(!breakdown.breaks_honor_triplet);
    }

    #[test]
    fn shape_breakdown_honor_pair_is_not_triplet() {
        let breakdown = shape_breakdown_for_discard(&counts(&["E", "E"]), tile("E"));
        assert!(!breakdown.breaks_triplet);
        assert!(!breakdown.breaks_honor_triplet);
    }

    #[test]
    fn number_triplet_penalty_is_heavier_than_number_pair() {
        let triplet = shape_penalty_for_discard(&counts(&["5m", "5m", "5m"]), tile("5m"));
        let pair = shape_penalty_for_discard(&counts(&["5m", "5m"]), tile("5m"));
        assert!(triplet > pair);
    }

    #[test]
    fn honor_triplet_penalty_is_heavier_than_honor_pair() {
        let triplet = shape_penalty_for_discard(&counts(&["E", "E", "E"]), tile("E"));
        let pair = shape_penalty_for_discard(&counts(&["E", "E"]), tile("E"));
        assert!(triplet > pair);
    }

    #[test]
    fn honor_triplet_penalty_is_heavier_than_number_triplet() {
        let honor = shape_penalty_for_discard(&counts(&["E", "E", "E"]), tile("E"));
        let number = shape_penalty_for_discard(&counts(&["5m", "5m", "5m"]), tile("5m"));
        assert!(honor > number);
    }

    #[test]
    fn honor_triplet_penalty_value() {
        // 対子20 + 同種3枚10 + 刻子35 + 字牌刻子20 + ブロック補正10 = 95
        // 字牌刻子は順子化できない完成面子なので対子存続 -12 は適用しない
        assert_eq!(
            shape_penalty_for_discard(&counts(&["E", "E", "E"]), tile("E")),
            95
        );
    }

    #[test]
    fn honor_triplet_penalty_not_softened_by_pair_relief() {
        // 字牌刻子は preserves_pair_after_discard による軽減を受けないため対子より十分に重い
        let honor_triplet = shape_penalty_for_discard(&counts(&["E", "E", "E"]), tile("E"));
        let honor_pair = shape_penalty_for_discard(&counts(&["E", "E"]), tile("E"));
        assert!(honor_triplet >= honor_pair + 35);
    }

    #[test]
    fn triplet_penalty_never_negative() {
        for hand in [
            counts(&["5m", "5m", "5m"]),
            counts(&["E", "E", "E"]),
            counts(&["C", "C", "C"]),
        ] {
            for tile in TileType::all() {
                assert!(shape_penalty_for_discard(&hand, tile) >= 0);
            }
        }
    }

    #[test]
    fn context_free_shape_penalty_unchanged_for_honor_triplets() {
        // context なし API では場風・自風・客風・三元牌の区別なく同一値
        for name in ["E", "S", "W", "N", "P", "F", "C"] {
            assert_eq!(
                shape_penalty_for_discard(&counts(&[name, name, name]), tile(name)),
                95
            );
        }
    }

    #[test]
    fn context_shape_penalty_matches_context_free_for_number_triplet() {
        // 数牌刻子には追加 penalty を適用しない
        let counts = counts(&["5m", "5m", "5m"]);
        let base = shape_penalty_for_discard(&counts, tile("5m"));
        assert_eq!(
            shape_penalty_for_discard_with_context(
                &counts,
                tile("5m"),
                Some(tile("E")),
                Some(tile("S")),
            ),
            base
        );
    }

    #[test]
    fn context_shape_penalty_adds_for_dragon_triplet() {
        // 白・發・中は場風・自風が None でも役牌として +15
        for name in ["P", "F", "C"] {
            let counts = counts(&[name, name, name]);
            let base = shape_penalty_for_discard(&counts, tile(name));
            assert_eq!(
                shape_penalty_for_discard_with_context(&counts, tile(name), None, None),
                base + VALUE_HONOR_TRIPLET_PENALTY
            );
        }
    }

    #[test]
    fn context_free_dragon_triplet_has_no_extra_penalty() {
        // context なし API では三元牌刻子にも +15 を適用しない
        let counts = counts(&["C", "C", "C"]);
        assert_eq!(shape_penalty_for_discard(&counts, tile("C")), 95);
        assert_eq!(
            shape_penalty_for_discard_with_context(&counts, tile("C"), None, None),
            95 + VALUE_HONOR_TRIPLET_PENALTY
        );
    }

    #[test]
    fn context_shape_penalty_adds_for_round_wind_triplet() {
        // 場風が東のとき東刻子を崩すと +15、場風でも自風でもなければ追加なし
        let counts = counts(&["E", "E", "E"]);
        let base = shape_penalty_for_discard(&counts, tile("E"));
        assert_eq!(
            shape_penalty_for_discard_with_context(
                &counts,
                tile("E"),
                Some(tile("E")),
                Some(tile("S")),
            ),
            base + VALUE_HONOR_TRIPLET_PENALTY
        );
        assert_eq!(
            shape_penalty_for_discard_with_context(
                &counts,
                tile("E"),
                Some(tile("S")),
                Some(tile("W")),
            ),
            base
        );
    }

    #[test]
    fn context_shape_penalty_adds_for_seat_wind_triplet() {
        // 自風が南のとき南刻子を崩すと +15、場風でも自風でもなければ追加なし
        let counts = counts(&["S", "S", "S"]);
        let base = shape_penalty_for_discard(&counts, tile("S"));
        assert_eq!(
            shape_penalty_for_discard_with_context(
                &counts,
                tile("S"),
                Some(tile("E")),
                Some(tile("S")),
            ),
            base + VALUE_HONOR_TRIPLET_PENALTY
        );
        assert_eq!(
            shape_penalty_for_discard_with_context(
                &counts,
                tile("S"),
                Some(tile("E")),
                Some(tile("W")),
            ),
            base
        );
    }

    #[test]
    fn context_shape_penalty_double_wind_adds_only_once() {
        // 場風と自風が同じ東でも追加は +15 の1回だけ
        let counts = counts(&["E", "E", "E"]);
        let base = shape_penalty_for_discard(&counts, tile("E"));
        assert_eq!(
            shape_penalty_for_discard_with_context(
                &counts,
                tile("E"),
                Some(tile("E")),
                Some(tile("E")),
            ),
            base + VALUE_HONOR_TRIPLET_PENALTY
        );
    }

    #[test]
    fn context_shape_penalty_guest_wind_triplet_has_no_extra() {
        // 場風東・自風南のとき西・北の客風刻子には追加しない
        for name in ["W", "N"] {
            let counts = counts(&[name, name, name]);
            let base = shape_penalty_for_discard(&counts, tile(name));
            assert_eq!(
                shape_penalty_for_discard_with_context(
                    &counts,
                    tile(name),
                    Some(tile("E")),
                    Some(tile("S")),
                ),
                base
            );
        }
    }

    #[test]
    fn context_shape_penalty_value_honor_pair_has_no_extra() {
        // 役牌でも2枚なら追加しない
        let counts = counts(&["C", "C"]);
        assert_eq!(
            shape_penalty_for_discard_with_context(&counts, tile("C"), None, None),
            shape_penalty_for_discard(&counts, tile("C"))
        );
    }

    #[test]
    fn context_shape_penalty_value_honor_single_has_no_extra() {
        // 役牌でも1枚なら追加しない
        let counts = counts(&["C"]);
        assert_eq!(
            shape_penalty_for_discard_with_context(&counts, tile("C"), None, None),
            shape_penalty_for_discard(&counts, tile("C"))
        );
    }

    #[test]
    fn context_shape_penalty_value_honor_quad_adds_once() {
        // 役牌を4枚持つ状態から切っても刻子を含む完成形を崩すため +15 を1回適用
        let counts = counts(&["C", "C", "C", "C"]);
        let base = shape_penalty_for_discard(&counts, tile("C"));
        assert_eq!(
            shape_penalty_for_discard_with_context(&counts, tile("C"), None, None),
            base + VALUE_HONOR_TRIPLET_PENALTY
        );
    }

    fn value_honor_triplet_context_penalty() -> i16 {
        shape_penalty_for_discard_with_context(&counts(&["C", "C", "C"]), tile("C"), None, None)
    }

    #[test]
    fn tiebreak_prefers_not_breaking_value_honor_triplet() {
        // 同じ向聴・受け入れなら役牌刻子を崩す候補より客風刻子を崩す候補を優先する
        let breaks_guest_triplet =
            evaluation_with_shape_penalty(1, 10, 2, honor_triplet_penalty(), 0, 0, false);
        let breaks_value_honor_triplet = evaluation_with_shape_penalty(
            1,
            10,
            2,
            value_honor_triplet_context_penalty(),
            0,
            0,
            false,
        );
        assert!(is_better_discard(
            &breaks_guest_triplet,
            &breaks_value_honor_triplet
        ));
        assert!(!is_better_discard(
            &breaks_value_honor_triplet,
            &breaks_guest_triplet
        ));
    }

    #[test]
    fn value_honor_triplet_penalty_does_not_override_shanten() {
        // 役牌刻子を崩す方が向聴数で優れていればそちらを選ぶ
        let break_triplet_better_shanten = evaluation_with_shape_penalty(
            0,
            4,
            1,
            value_honor_triplet_context_penalty(),
            0,
            0,
            false,
        );
        let keep_triplet_worse_shanten = evaluation_with_shape_penalty(1, 40, 5, 0, 0, 0, false);
        assert!(is_better_discard(
            &break_triplet_better_shanten,
            &keep_triplet_worse_shanten
        ));
    }

    #[test]
    fn value_honor_triplet_penalty_does_not_override_acceptance() {
        // 役牌刻子を崩す方が受け入れで優れていればそちらを選ぶ
        let break_triplet_more_remaining = evaluation_with_shape_penalty(
            1,
            20,
            1,
            value_honor_triplet_context_penalty(),
            0,
            0,
            false,
        );
        let keep_triplet_less_remaining = evaluation_with_shape_penalty(1, 10, 1, 0, 0, 0, false);
        assert!(is_better_discard(
            &break_triplet_more_remaining,
            &keep_triplet_less_remaining
        ));
    }

    #[test]
    fn evaluate_with_context_adds_value_honor_triplet_penalty() {
        // 123m 456m 789m 1p 2p 中中中
        let tiles = ids(&[0, 4, 8, 12, 16, 20, 24, 28, 32, 36, 40, 132, 133, 134]);
        let counts = TileCounts::from_tiles(tiles.iter().copied());
        let base = shape_penalty_for_discard(&counts, tile("C"));

        let with_context = evaluate_discards_from_tiles_with_context(
            &tiles,
            &[],
            Some(tile("E")),
            Some(tile("S")),
        );
        let dragon = with_context
            .iter()
            .find(|evaluation| evaluation.discard == tile("C"))
            .unwrap();
        assert_eq!(dragon.shape_penalty, base + VALUE_HONOR_TRIPLET_PENALTY);

        let visible = ids(&[72, 76]);
        for visible_tiles in [&[][..], &visible[..]] {
            let with_visible = evaluate_discards_from_tiles_with_visible_tiles(
                &tiles,
                &[],
                Some(tile("E")),
                Some(tile("S")),
                visible_tiles,
            );
            let dragon_visible = with_visible
                .iter()
                .find(|evaluation| evaluation.discard == tile("C"))
                .unwrap();
            assert_eq!(
                dragon_visible.shape_penalty,
                base + VALUE_HONOR_TRIPLET_PENALTY
            );
        }
    }

    #[test]
    fn evaluate_context_free_omits_value_honor_triplet_penalty() {
        // 123m 456m 789m 1p 2p 中中中
        let tiles = ids(&[0, 4, 8, 12, 16, 20, 24, 28, 32, 36, 40, 132, 133, 134]);
        let counts = TileCounts::from_tiles(tiles.iter().copied());
        let base = shape_penalty_for_discard(&counts, tile("C"));

        let from_tiles = evaluate_discards_from_tiles(&tiles);
        let dragon = from_tiles
            .iter()
            .find(|evaluation| evaluation.discard == tile("C"))
            .unwrap();
        assert_eq!(dragon.shape_penalty, base);

        // with_dora 経路へ context 付き penalty が漏れていないこと
        let with_dora = evaluate_discards_from_tiles_with_dora(&tiles, &[]);
        let dragon_dora = with_dora
            .iter()
            .find(|evaluation| evaluation.discard == tile("C"))
            .unwrap();
        assert_eq!(dragon_dora.shape_penalty, base);
    }

    #[test]
    fn select_context_free_omits_value_honor_triplet_penalty() {
        // 中刻子だけの入力: context なし selector は追加 penalty を適用しない
        let tiles = ids(&[132, 133, 134]);
        let counts = TileCounts::from_tiles(tiles.iter().copied());
        let expected = shape_penalty_for_discard(&counts, tile("C"));

        let selected = select_best_discard_from_tiles(&tiles).expect("candidate should exist");
        assert_eq!(selected.discard, tile("C"));
        assert_eq!(selected.shape_penalty, expected);

        let selected =
            select_best_discard_from_tiles_with_dora(&tiles, &[]).expect("candidate should exist");
        assert_eq!(selected.discard, tile("C"));
        assert_eq!(selected.shape_penalty, expected);

        // context 付き selector では引き続き追加 penalty が適用される
        let selected = select_best_discard_from_tiles_with_context(&tiles, &[], None, None)
            .expect("candidate should exist");
        assert_eq!(
            selected.shape_penalty,
            expected + VALUE_HONOR_TRIPLET_PENALTY
        );
    }

    #[test]
    fn select_with_dora_matches_evaluate_with_dora() {
        // selector の戻り値は evaluate 一覧から既存比較順で選ばれた候補と一致する
        let tiles = ids(&[0, 4, 8, 12, 16, 20, 24, 28, 32, 36, 40, 132, 133, 134]);
        let evaluations = evaluate_discards_from_tiles_with_dora(&tiles, &[]);
        let expected = select_best(evaluations.clone()).expect("candidate should exist");
        let selected =
            select_best_discard_from_tiles_with_dora(&tiles, &[]).expect("candidate should exist");
        assert_eq!(selected, expected);
        assert!(evaluations.contains(&selected));
    }

    #[test]
    fn context_shape_penalty_leaves_other_fields_untouched() {
        // 123m 456m 789m 1p 2p 中中中
        let tiles = ids(&[0, 4, 8, 12, 16, 20, 24, 28, 32, 36, 40, 132, 133, 134]);
        let free = evaluate_discards_from_tiles_with_dora(&tiles, &[]);
        let ctx = evaluate_discards_from_tiles_with_context(&tiles, &[], None, None);
        for (a, b) in free.iter().zip(ctx.iter()) {
            assert_eq!(a.discard, b.discard);
            assert_eq!(a.shanten_after_discard, b.shanten_after_discard);
            assert_eq!(a.acceptance_after_discard, b.acceptance_after_discard);
            assert_eq!(a.floating_tile_value, b.floating_tile_value);
            assert_eq!(a.discarded_dora_count, b.discarded_dora_count);
            assert_eq!(a.discarded_value_honor_count, b.discarded_value_honor_count);
            assert_eq!(a.discards_red_five, b.discards_red_five);
        }
        let dragon_free = free
            .iter()
            .find(|evaluation| evaluation.discard == tile("C"))
            .unwrap();
        let dragon_ctx = ctx
            .iter()
            .find(|evaluation| evaluation.discard == tile("C"))
            .unwrap();
        assert_eq!(
            dragon_ctx.shape_penalty,
            dragon_free.shape_penalty + VALUE_HONOR_TRIPLET_PENALTY
        );
    }

    fn number_triplet_penalty() -> i16 {
        shape_penalty_for_discard(&counts(&["5m", "5m", "5m"]), tile("5m"))
    }

    fn honor_triplet_penalty() -> i16 {
        shape_penalty_for_discard(&counts(&["E", "E", "E"]), tile("E"))
    }

    fn isolated_honor_penalty() -> i16 {
        shape_penalty_for_discard(&counts(&["W"]), tile("W"))
    }

    fn surplus_pair_penalty() -> i16 {
        shape_penalty_for_discard(&counts(&["E", "E", "S", "S", "W", "W"]), tile("E"))
    }

    #[test]
    fn tiebreak_prefers_isolated_over_breaking_honor_triplet() {
        let isolated = evaluation_with_shape_penalty(1, 10, 2, isolated_penalty(), 0, 0, false);
        let breaks_honor_triplet =
            evaluation_with_shape_penalty(1, 10, 2, honor_triplet_penalty(), 0, 0, false);
        assert!(is_better_discard(&isolated, &breaks_honor_triplet));
        assert!(!is_better_discard(&breaks_honor_triplet, &isolated));
    }

    #[test]
    fn tiebreak_prefers_isolated_honor_over_breaking_honor_triplet() {
        let isolated_honor =
            evaluation_with_shape_penalty(1, 10, 2, isolated_honor_penalty(), 0, 0, false);
        let breaks_honor_triplet =
            evaluation_with_shape_penalty(1, 10, 2, honor_triplet_penalty(), 0, 0, false);
        assert!(is_better_discard(&isolated_honor, &breaks_honor_triplet));
        assert!(!is_better_discard(&breaks_honor_triplet, &isolated_honor));
    }

    #[test]
    fn tiebreak_prefers_surplus_pair_over_breaking_honor_triplet() {
        let surplus_pair =
            evaluation_with_shape_penalty(1, 10, 2, surplus_pair_penalty(), 0, 0, false);
        let breaks_honor_triplet =
            evaluation_with_shape_penalty(1, 10, 2, honor_triplet_penalty(), 0, 0, false);
        assert!(is_better_discard(&surplus_pair, &breaks_honor_triplet));
        assert!(!is_better_discard(&breaks_honor_triplet, &surplus_pair));
    }

    #[test]
    fn tiebreak_prefers_breaking_number_triplet_over_honor_triplet() {
        let breaks_number_triplet =
            evaluation_with_shape_penalty(1, 10, 2, number_triplet_penalty(), 0, 0, false);
        let breaks_honor_triplet =
            evaluation_with_shape_penalty(1, 10, 2, honor_triplet_penalty(), 0, 0, false);
        assert!(is_better_discard(
            &breaks_number_triplet,
            &breaks_honor_triplet
        ));
    }

    #[test]
    fn honor_triplet_penalty_does_not_override_shanten() {
        let break_honor_triplet_better_shanten =
            evaluation_with_shape_penalty(0, 4, 1, honor_triplet_penalty(), 0, 0, false);
        let keep_worse_shanten = evaluation_with_shape_penalty(1, 40, 5, 0, 0, 0, false);
        assert!(is_better_discard(
            &break_honor_triplet_better_shanten,
            &keep_worse_shanten
        ));
    }

    #[test]
    fn honor_triplet_penalty_does_not_override_acceptance() {
        let break_honor_triplet_more_remaining =
            evaluation_with_shape_penalty(1, 20, 1, honor_triplet_penalty(), 0, 0, false);
        let keep_less_remaining = evaluation_with_shape_penalty(1, 10, 1, 0, 0, 0, false);
        assert!(is_better_discard(
            &break_honor_triplet_more_remaining,
            &keep_less_remaining
        ));

        let break_honor_triplet_more_types =
            evaluation_with_shape_penalty(1, 10, 3, honor_triplet_penalty(), 0, 0, false);
        let keep_fewer_types = evaluation_with_shape_penalty(1, 10, 2, 0, 0, 0, false);
        assert!(is_better_discard(
            &break_honor_triplet_more_types,
            &keep_fewer_types
        ));
    }

    #[test]
    fn hand_shape_summary_counts_sequences() {
        let summary = hand_shape_summary(&counts(&["3m", "4m", "5m"]));
        assert_eq!(summary.sequence_count, 1);
    }

    #[test]
    fn hand_shape_summary_counts_triplets() {
        let summary = hand_shape_summary(&counts(&["5p", "5p", "5p"]));
        assert_eq!(summary.triplet_count, 1);
        assert_eq!(summary.pair_like_type_count, 1);
    }

    #[test]
    fn hand_shape_summary_counts_honor_and_number_pairs() {
        let summary = hand_shape_summary(&counts(&["E", "E", "3s", "3s"]));
        assert_eq!(summary.pair_like_type_count, 2);
    }

    #[test]
    fn hand_shape_summary_counts_ryanmen() {
        for pair in [
            ["2m", "3m"],
            ["3m", "4m"],
            ["4m", "5m"],
            ["5m", "6m"],
            ["6m", "7m"],
            ["7m", "8m"],
        ] {
            let summary = hand_shape_summary(&counts(&pair));
            assert_eq!(summary.ryanmen_taatsu_count, 1, "{pair:?}");
            assert_eq!(summary.penchan_taatsu_count, 0, "{pair:?}");
        }
    }

    #[test]
    fn hand_shape_summary_counts_penchan() {
        for pair in [["1m", "2m"], ["8s", "9s"]] {
            let summary = hand_shape_summary(&counts(&pair));
            assert_eq!(summary.penchan_taatsu_count, 1, "{pair:?}");
            assert_eq!(summary.ryanmen_taatsu_count, 0, "{pair:?}");
        }
    }

    #[test]
    fn hand_shape_summary_counts_kanchan() {
        for pair in [["1m", "3m"], ["4p", "6p"], ["7s", "9s"]] {
            let summary = hand_shape_summary(&counts(&pair));
            assert_eq!(summary.kanchan_taatsu_count, 1, "{pair:?}");
            assert_eq!(summary.ryanmen_taatsu_count, 0, "{pair:?}");
            assert_eq!(summary.penchan_taatsu_count, 0, "{pair:?}");
        }
    }

    #[test]
    fn hand_shape_summary_counts_honor_tanki_as_isolated() {
        let summary = hand_shape_summary(&counts(&["E"]));
        assert_eq!(summary.isolated_tile_type_count, 1);
    }

    #[test]
    fn hand_shape_summary_counts_fully_isolated_number() {
        let summary = hand_shape_summary(&counts(&["2m", "5m", "8m"]));
        assert_eq!(summary.isolated_tile_type_count, 3);
    }

    #[test]
    fn hand_shape_summary_ignores_cross_suit_shapes() {
        let summary = hand_shape_summary(&counts(&["3m", "4s", "5p"]));
        assert_eq!(summary.sequence_count, 0);
        assert_eq!(summary.ryanmen_taatsu_count, 0);
        assert_eq!(summary.kanchan_taatsu_count, 0);
        assert_eq!(summary.penchan_taatsu_count, 0);
        assert_eq!(summary.isolated_tile_type_count, 3);
    }

    #[test]
    fn estimated_block_count_is_simple_sum() {
        let summary = hand_shape_summary(&counts(&["1m", "1m", "2m", "3m", "5p", "6p"]));
        assert_eq!(
            summary.estimated_block_count,
            summary.sequence_count
                + summary.triplet_count
                + summary.pair_like_type_count
                + summary.ryanmen_taatsu_count
                + summary.kanchan_taatsu_count
                + summary.penchan_taatsu_count
        );
    }

    #[test]
    fn discard_block_context_returns_default_for_missing_discard() {
        let context = discard_block_context(&counts(&["1m"]), tile("9s"));
        assert_eq!(context, DiscardBlockContext::default());
    }

    #[test]
    fn discard_block_context_sets_before_and_after() {
        let hand = counts(&["3m", "4m", "9s"]);
        let context = discard_block_context(&hand, tile("3m"));
        assert_eq!(context.before, hand_shape_summary(&hand));
        let mut after = hand;
        after.remove(tile("3m")).unwrap();
        assert_eq!(context.after, hand_shape_summary(&after));
    }

    #[test]
    fn discard_block_context_flags_block_reduction() {
        let context = discard_block_context(&counts(&["3m", "4m"]), tile("3m"));
        assert!(context.reduces_estimated_block_count);
        assert!(context.leaves_under_five_blocks);
    }

    #[test]
    fn discard_block_context_reduces_but_keeps_five_blocks() {
        let hand = counts(&["2m", "3m", "4m", "5m", "6m", "7m", "8m"]);
        let context = discard_block_context(&hand, tile("2m"));
        assert!(context.reduces_estimated_block_count);
        assert!(!context.leaves_under_five_blocks);
    }

    #[test]
    fn discard_block_context_no_reduction_for_isolated_tile() {
        let context = discard_block_context(&counts(&["3m", "4m", "9s"]), tile("9s"));
        assert!(!context.reduces_estimated_block_count);
    }

    #[test]
    fn block_correction_is_heavier_when_leaving_under_five_blocks() {
        let heavy = shape_penalty_for_discard(&counts(&["3m", "4m"]), tile("3m"));
        let light = shape_penalty_for_discard(
            &counts(&["3m", "4m", "1p", "2p", "3p", "4p", "5p", "6p"]),
            tile("3m"),
        );
        assert!(heavy > light);
        assert_eq!(heavy, light + 6);
    }

    #[test]
    fn no_block_correction_when_block_count_unchanged() {
        let hand = counts(&["3m", "4m", "9s"]);
        assert_eq!(shape_penalty_for_discard(&hand, tile("9s")), 0);
    }

    #[test]
    fn shanten_outranks_shape_penalty_block_correction() {
        let low_shanten_high_penalty = evaluation_with_shape_penalty(0, 4, 1, 50, 0, 0, false);
        let high_shanten_low_penalty = evaluation_with_shape_penalty(1, 40, 5, 0, 0, 0, false);
        assert!(is_better_discard(
            &low_shanten_high_penalty,
            &high_shanten_low_penalty
        ));
    }

    #[test]
    fn compare_reports_shanten_reason() {
        let candidate = evaluation(0, 4, 1, 2, false);
        let current_best = evaluation(1, 40, 5, 0, false);
        let comparison = compare_discard_evaluations(&candidate, &current_best);
        assert!(comparison.candidate_is_better);
        assert_eq!(comparison.reason, DiscardComparisonReason::Shanten);
    }

    #[test]
    fn compare_reports_shanten_reason_when_candidate_is_worse() {
        let candidate = evaluation(1, 40, 5, 0, false);
        let current_best = evaluation(0, 4, 1, 0, false);
        let comparison = compare_discard_evaluations(&candidate, &current_best);
        assert!(!comparison.candidate_is_better);
        assert_eq!(comparison.reason, DiscardComparisonReason::Shanten);
    }

    #[test]
    fn compare_reports_acceptance_remaining_reason() {
        let candidate = evaluation_with_shape_penalty(1, 20, 1, 50, 0, 0, false);
        let current_best = evaluation_with_shape_penalty(1, 10, 1, 0, 0, 0, false);
        let comparison = compare_discard_evaluations(&candidate, &current_best);
        assert!(comparison.candidate_is_better);
        assert_eq!(
            comparison.reason,
            DiscardComparisonReason::AcceptanceRemaining
        );
    }

    #[test]
    fn compare_reports_acceptance_type_count_reason() {
        let candidate = evaluation(1, 10, 3, 0, false);
        let current_best = evaluation(1, 10, 2, 0, false);
        let comparison = compare_discard_evaluations(&candidate, &current_best);
        assert!(comparison.candidate_is_better);
        assert_eq!(
            comparison.reason,
            DiscardComparisonReason::AcceptanceTypeCount
        );
    }

    #[test]
    fn compare_reports_shape_penalty_reason() {
        let candidate = evaluation_with_shape_penalty(1, 10, 2, 10, 2, 0, false);
        let current_best = evaluation_with_shape_penalty(1, 10, 2, 40, 0, 0, false);
        let comparison = compare_discard_evaluations(&candidate, &current_best);
        assert!(comparison.candidate_is_better);
        assert_eq!(comparison.reason, DiscardComparisonReason::ShapePenalty);
    }

    #[test]
    fn compare_reports_floating_tile_value_reason() {
        let candidate = evaluation_with_floating(1, 10, 2, 0, 1);
        let current_best = evaluation_with_floating(1, 10, 2, 0, 5);
        let comparison = compare_discard_evaluations(&candidate, &current_best);
        assert!(comparison.candidate_is_better);
        assert_eq!(
            comparison.reason,
            DiscardComparisonReason::FloatingTileValue
        );
    }

    #[test]
    fn compare_reports_dora_reason() {
        let candidate = evaluation_with_value_honor(1, 10, 2, 0, 1, false);
        let current_best = evaluation_with_value_honor(1, 10, 2, 1, 0, false);
        let comparison = compare_discard_evaluations(&candidate, &current_best);
        assert!(comparison.candidate_is_better);
        assert_eq!(comparison.reason, DiscardComparisonReason::Dora);
    }

    #[test]
    fn compare_reports_value_honor_reason() {
        let candidate = evaluation_with_value_honor(1, 10, 2, 0, 0, true);
        let current_best = evaluation_with_value_honor(1, 10, 2, 0, 1, false);
        let comparison = compare_discard_evaluations(&candidate, &current_best);
        assert!(comparison.candidate_is_better);
        assert_eq!(comparison.reason, DiscardComparisonReason::ValueHonor);
    }

    #[test]
    fn compare_reports_red_five_reason() {
        let candidate = evaluation(1, 10, 2, 0, false);
        let current_best = evaluation(1, 10, 2, 0, true);
        let comparison = compare_discard_evaluations(&candidate, &current_best);
        assert!(comparison.candidate_is_better);
        assert_eq!(comparison.reason, DiscardComparisonReason::RedFive);
    }

    #[test]
    fn compare_reports_red_five_reason_when_candidate_is_worse() {
        let candidate = evaluation(1, 10, 2, 0, true);
        let current_best = evaluation(1, 10, 2, 0, false);
        let comparison = compare_discard_evaluations(&candidate, &current_best);
        assert!(!comparison.candidate_is_better);
        assert_eq!(comparison.reason, DiscardComparisonReason::RedFive);
    }

    #[test]
    fn compare_reports_stable_order_on_perfect_tie() {
        let candidate = evaluation_with_value_honor(1, 10, 2, 1, 1, true);
        let current_best = evaluation_with_value_honor(1, 10, 2, 1, 1, true);
        let comparison = compare_discard_evaluations(&candidate, &current_best);
        assert!(!comparison.candidate_is_better);
        assert_eq!(comparison.reason, DiscardComparisonReason::StableOrder);
    }

    #[test]
    fn compare_matches_is_better_discard() {
        let candidate = evaluation_with_value_honor(1, 10, 2, 1, 1, false);
        let current_best = evaluation_with_value_honor(1, 12, 2, 1, 1, false);
        assert_eq!(
            compare_discard_evaluations(&candidate, &current_best).candidate_is_better,
            is_better_discard(&candidate, &current_best)
        );
    }

    // standard==1 だが全体最小向聴を 0 にした（七対子テンパイ相当の）通常形一向聴評価。
    fn tenpai_overall_standard_iishanten(
        remaining: u8,
        type_count: usize,
        shape: IishantenShape,
    ) -> DiscardEvaluation {
        let mut evaluation = evaluation_with_iishanten_shape(remaining, type_count, shape);
        let shanten = concealed(Shanten {
            standard: 1,
            chiitoitsu: 0,
            kokushi: 127,
        });
        evaluation.shanten_after_discard = shanten;
        evaluation.acceptance_after_discard.current = shanten;
        evaluation
    }

    #[test]
    fn shanten_outranks_iishanten_shape() {
        let better_shanten_non_complete = evaluation(0, 4, 1, 0, false);
        let worse_shanten_complete =
            evaluation_with_iishanten_shape(40, 5, IishantenShape::Complete);
        let comparison =
            compare_discard_evaluations(&better_shanten_non_complete, &worse_shanten_complete);
        assert!(comparison.candidate_is_better);
        assert_eq!(comparison.reason, DiscardComparisonReason::Shanten);
    }

    #[test]
    fn acceptance_remaining_outranks_iishanten_shape() {
        let more_remaining_non_complete =
            evaluation_with_iishanten_shape(20, 1, IishantenShape::Weak);
        let less_remaining_complete =
            evaluation_with_iishanten_shape(10, 1, IishantenShape::Complete);
        let comparison =
            compare_discard_evaluations(&more_remaining_non_complete, &less_remaining_complete);
        assert!(comparison.candidate_is_better);
        assert_eq!(
            comparison.reason,
            DiscardComparisonReason::AcceptanceRemaining
        );
    }

    #[test]
    fn acceptance_type_count_outranks_iishanten_shape() {
        let more_types_non_complete = evaluation_with_iishanten_shape(10, 3, IishantenShape::Weak);
        let fewer_types_complete = evaluation_with_iishanten_shape(10, 2, IishantenShape::Complete);
        let comparison =
            compare_discard_evaluations(&more_types_non_complete, &fewer_types_complete);
        assert!(comparison.candidate_is_better);
        assert_eq!(
            comparison.reason,
            DiscardComparisonReason::AcceptanceTypeCount
        );
    }

    #[test]
    fn complete_outranks_non_complete_when_top_axes_tie() {
        let complete = evaluation_with_iishanten_shape(10, 2, IishantenShape::Complete);
        let weak = evaluation_with_iishanten_shape(10, 2, IishantenShape::Weak);

        let forward = compare_discard_evaluations(&complete, &weak);
        assert!(forward.candidate_is_better);
        assert_eq!(forward.reason, DiscardComparisonReason::IishantenShape);

        let backward = compare_discard_evaluations(&weak, &complete);
        assert!(!backward.candidate_is_better);
        assert_eq!(backward.reason, DiscardComparisonReason::IishantenShape);
    }

    #[test]
    fn complete_outranks_larger_shape_penalty() {
        let complete_high_penalty =
            evaluation_with_shape_penalty_and_iishanten_shape(10, 2, 40, IishantenShape::Complete);
        let weak_low_penalty =
            evaluation_with_shape_penalty_and_iishanten_shape(10, 2, 0, IishantenShape::Weak);
        let comparison = compare_discard_evaluations(&complete_high_penalty, &weak_low_penalty);
        assert!(comparison.candidate_is_better);
        assert_eq!(comparison.reason, DiscardComparisonReason::IishantenShape);
    }

    #[test]
    fn non_complete_shapes_have_no_fixed_order() {
        for (a, b) in [
            (IishantenShape::Headless, IishantenShape::Kuttsuki),
            (IishantenShape::Kuttsuki, IishantenShape::Weak),
            (IishantenShape::Headless, IishantenShape::Weak),
            (IishantenShape::Weak, IishantenShape::Unknown),
        ] {
            let left = evaluation_with_iishanten_shape(10, 2, a);
            let right = evaluation_with_iishanten_shape(10, 2, b);
            let forward = compare_discard_evaluations(&left, &right);
            let backward = compare_discard_evaluations(&right, &left);
            // 一向聴形だけでは決着せず、後続軸（ここでは全同値なので StableOrder）で決まる。
            assert_ne!(forward.reason, DiscardComparisonReason::IishantenShape);
            assert_ne!(backward.reason, DiscardComparisonReason::IishantenShape);
            assert_eq!(forward.reason, DiscardComparisonReason::StableOrder);
            assert!(!forward.candidate_is_better);
            assert!(!backward.candidate_is_better);
        }
    }

    #[test]
    fn iishanten_shape_not_applied_when_overall_tenpai() {
        // standard==1 でも全体（七対子）テンパイなら完全一向聴だけを理由に優先しない。
        let complete_tenpai = tenpai_overall_standard_iishanten(10, 2, IishantenShape::Complete);
        let weak_tenpai = tenpai_overall_standard_iishanten(10, 2, IishantenShape::Weak);
        let comparison = compare_discard_evaluations(&complete_tenpai, &weak_tenpai);
        assert_ne!(comparison.reason, DiscardComparisonReason::IishantenShape);
        assert!(!comparison.candidate_is_better);
    }

    #[test]
    fn iishanten_shape_not_applied_when_only_one_side_is_standard_iishanten() {
        // 片方だけが通常形一向聴（standard==1）の場合は IishantenShape で決着しない。
        let complete = evaluation_with_iishanten_shape(10, 2, IishantenShape::Complete);
        let mut non_standard = evaluation_with_iishanten_shape(10, 2, IishantenShape::Unknown);
        // 全体最小向聴は一向聴のまま standard だけ二向聴にする。
        let shanten = concealed(Shanten {
            standard: 2,
            chiitoitsu: 1,
            kokushi: 127,
        });
        non_standard.shanten_after_discard = shanten;
        non_standard.acceptance_after_discard.current = shanten;

        let comparison = compare_discard_evaluations(&complete, &non_standard);
        assert_ne!(comparison.reason, DiscardComparisonReason::IishantenShape);
    }

    #[test]
    fn evaluate_discards_classifies_iishanten_shape_after_discard() {
        // 既存の一向聴分類テストで使用する13枚形へ14枚目を加え、対象牌を切ると元の形へ戻る。
        let cases = [
            (
                vec![
                    "1m", "2m", "3m", "4m", "5m", "6m", "E", "E", "2p", "3p", "5s", "6s", "C", "1s",
                ],
                "1s",
                IishantenShape::Complete,
            ),
            (
                vec![
                    "1m", "2m", "3m", "4m", "5m", "6m", "7m", "8m", "9m", "2p", "3p", "5s", "6s",
                    "1s",
                ],
                "1s",
                IishantenShape::Headless,
            ),
            (
                vec![
                    "1m", "2m", "3m", "4m", "5m", "6m", "7m", "8m", "9m", "5p", "5p", "2s", "8s",
                    "9s",
                ],
                "9s",
                IishantenShape::Kuttsuki,
            ),
            (
                vec![
                    "1m", "2m", "3m", "4m", "5m", "6m", "7m", "8m", "9m", "2p", "3p", "5s", "E",
                    "1s",
                ],
                "1s",
                IishantenShape::Weak,
            ),
            (
                // 1s を切ると通常形テンパイなので Unknown。
                vec![
                    "1m", "2m", "3m", "4m", "5m", "6m", "7m", "8m", "9m", "2p", "3p", "5s", "5s",
                    "1s",
                ],
                "1s",
                IishantenShape::Unknown,
            ),
        ];

        for (hand, discard, expected) in cases {
            let counts = counts(&hand);
            let evaluations = evaluate_discards(&counts);
            let evaluation = discard_evaluation(&evaluations, tile(discard));
            assert_eq!(
                evaluation.standard_iishanten_shape_after_discard, expected,
                "{hand:?} discard {discard}"
            );
        }
    }

    #[test]
    fn evaluate_discards_with_visible_tiles_matches_iishanten_shape() {
        // 完全一向聴の14枚形。visible tiles 経路でも分類は打牌後 counts のみに依存し一致する。
        let hand = ids(&[0, 4, 8, 12, 17, 20, 108, 109, 40, 44, 88, 92, 132, 72]);
        let counts = TileCounts::from_tiles(hand.iter().copied());

        let plain = evaluate_discards(&counts);
        let visible = evaluate_discards_with_visible_tiles(&counts, &hand);
        assert!(!plain.is_empty());
        for (a, b) in plain.iter().zip(visible.iter()) {
            assert_eq!(a.discard, b.discard);
            assert_eq!(
                a.standard_iishanten_shape_after_discard,
                b.standard_iishanten_shape_after_discard
            );
        }
        let complete = discard_evaluation(&plain, tile("1s"));
        assert_eq!(
            complete.standard_iishanten_shape_after_discard,
            IishantenShape::Complete
        );
    }

    #[test]
    fn diagnose_reports_iishanten_shape_reason() {
        // 上位3軸が同値で片方だけ Complete のとき、非選択候補の理由は IishantenShape。
        let winner = evaluation_with_iishanten_shape(10, 2, IishantenShape::Complete);
        let loser = evaluation_with_iishanten_shape(10, 2, IishantenShape::Weak);
        let candidate = loser_candidate(winner, loser);
        assert!(candidate.selected_is_strictly_better_than_candidate);
        assert_eq!(
            candidate.comparison_reason,
            DiscardComparisonReason::IishantenShape
        );
    }

    // ---- 七対子単騎待ちの tie-break ----

    // 6対子。孤立牌を2枚足すと、どちらを切っても他方の七対子単騎テンパイになる14枚になる。
    const CHIITOITSU_PAIRS: [&str; 12] = [
        "1m", "1m", "4m", "4m", "7m", "7m", "1p", "1p", "4p", "4p", "7p", "7p",
    ];

    fn chiitoitsu_tanki_hand<'a>(left: &'a str, right: &'a str) -> Vec<&'a str> {
        let mut hand: Vec<&'a str> = CHIITOITSU_PAIRS.to_vec();
        hand.push(left);
        hand.push(right);
        hand
    }

    #[test]
    fn chiitoitsu_wait_quality_orders_the_tanki_wait() {
        // 生き枚数が同じ七対子単騎同士は 字牌 > 1/9 > 2/8 > 3/7 > 4/6 > 5 の順で待ちを選ぶ。
        for (better, worse) in [
            ("E", "1s"),
            ("1s", "2s"),
            ("2s", "3s"),
            ("3s", "4s"),
            ("4s", "5s"),
        ] {
            let counts = counts(&chiitoitsu_tanki_hand(better, worse));
            let evaluations = evaluate_discards(&counts);
            // 品質の良い待ちを残す打牌と、悪い待ちを残す打牌。
            let keeps_better = discard_evaluation(&evaluations, tile(worse));
            let keeps_worse = discard_evaluation(&evaluations, tile(better));

            assert_eq!(
                keeps_better.min_shanten_after_discard(),
                0,
                "{better}/{worse}"
            );
            assert_eq!(
                keeps_better.acceptance_total_remaining(),
                keeps_worse.acceptance_total_remaining(),
                "{better}/{worse}"
            );
            assert_eq!(
                keeps_better.acceptance_type_count(),
                keeps_worse.acceptance_type_count(),
                "{better}/{worse}"
            );

            let comparison = compare_discard_evaluations(keeps_better, keeps_worse);
            assert!(comparison.candidate_is_better, "{better}/{worse}");
            assert_eq!(
                comparison.reason,
                DiscardComparisonReason::ChiitoitsuWaitQuality,
                "{better}/{worse}"
            );
            assert_eq!(
                select_best_discard(&counts).expect("打牌を選べる").discard,
                tile(worse),
                "{better}/{worse}"
            );
        }
    }

    #[test]
    fn acceptance_remaining_outranks_the_chiitoitsu_wait_quality() {
        // 待ちの品質は 北 単騎の方が上でも、生き枚数の多い 5m 単騎を選ぶ。
        let hand = chiitoitsu_tanki_hand("5m", "N");
        let counts = counts(&hand);
        let mut visible = hand.clone();
        visible.push("N");
        let evaluations = evaluate_discards_with_visible_tiles(&counts, &ids_of(&visible));

        let keeps_five = discard_evaluation(&evaluations, tile("N"));
        let keeps_north = discard_evaluation(&evaluations, tile("5m"));
        assert_eq!(acceptance_remaining(keeps_five, tile("5m")), Some(3));
        assert_eq!(acceptance_remaining(keeps_north, tile("N")), Some(2));

        let comparison = compare_discard_evaluations(keeps_five, keeps_north);
        assert!(comparison.candidate_is_better);
        assert_eq!(
            comparison.reason,
            DiscardComparisonReason::AcceptanceRemaining
        );
        assert_eq!(
            select_best(evaluations).expect("打牌を選べる").discard,
            tile("N")
        );
    }

    #[test]
    fn chiitoitsu_wait_quality_is_not_applied_to_a_standard_tanki_tenpai() {
        // 通常形の単騎テンパイ同士には七対子専用の固定順位を広げない。
        let counts = counts(&[
            "1m", "2m", "3m", "4m", "5m", "6m", "7m", "8m", "9m", "1p", "2p", "3p", "5s", "N",
        ]);
        let evaluations = evaluate_discards(&counts);
        let keeps_five_sou = discard_evaluation(&evaluations, tile("N"));
        let keeps_north = discard_evaluation(&evaluations, tile("5s"));

        for evaluation in [keeps_five_sou, keeps_north] {
            assert_eq!(evaluation.min_shanten_after_discard(), 0);
            assert_ne!(
                evaluation
                    .shanten_after_discard
                    .concealed()
                    .expect("門前")
                    .chiitoitsu,
                0
            );
        }
        assert_ne!(
            compare_discard_evaluations(keeps_five_sou, keeps_north).reason,
            DiscardComparisonReason::ChiitoitsuWaitQuality
        );
    }

    // 七対子テンパイの合成評価。適用範囲だけを検証するため、単騎待ち牌を明示して作る。
    fn chiitoitsu_tenpai_evaluation(waits: &[&str]) -> DiscardEvaluation {
        let tenpai = concealed(Shanten {
            standard: 3,
            chiitoitsu: 0,
            kokushi: 127,
        });
        let hora = concealed(Shanten {
            standard: 2,
            chiitoitsu: -1,
            kokushi: 127,
        });
        let mut evaluation = evaluation(0, 0, 0, 0, false);
        evaluation.shanten_after_discard = tenpai;
        evaluation.acceptance_after_discard = Acceptance {
            current: tenpai,
            tiles: waits
                .iter()
                .map(|wait| AcceptanceTile {
                    tile: tile(wait),
                    remaining: 3,
                    shanten_after_draw: hora,
                })
                .collect(),
        };
        evaluation
    }

    #[test]
    fn chiitoitsu_wait_quality_needs_a_unique_wait() {
        // 七対子完成牌を一意に決められない候補では tie-break せず、後続の既存比較へ委ねる。
        let ambiguous = chiitoitsu_tenpai_evaluation(&["E", "5m"]);
        let also_ambiguous = chiitoitsu_tenpai_evaluation(&["5m", "E"]);
        assert_ne!(
            compare_discard_evaluations(&ambiguous, &also_ambiguous).reason,
            DiscardComparisonReason::ChiitoitsuWaitQuality
        );

        // 待ちが一意なら同じ構成でも tie-break が働く。
        assert_eq!(
            compare_discard_evaluations(
                &chiitoitsu_tenpai_evaluation(&["E"]),
                &chiitoitsu_tenpai_evaluation(&["5m"]),
            )
            .reason,
            DiscardComparisonReason::ChiitoitsuWaitQuality
        );
    }

    #[test]
    fn chiitoitsu_wait_quality_is_not_applied_to_a_melded_hand() {
        // 副露形は七対子の対象外なので、待ち牌が同じ形でも固定順位を使わない。
        let melded = |wait: &str| {
            let mut evaluation = chiitoitsu_tenpai_evaluation(&[wait]);
            evaluation.shanten_after_discard = EffectiveShanten::Melded { standard: 0 };
            evaluation.acceptance_after_discard.current = EffectiveShanten::Melded { standard: 0 };
            for acceptance in &mut evaluation.acceptance_after_discard.tiles {
                acceptance.shanten_after_draw = EffectiveShanten::Melded { standard: -1 };
            }
            evaluation
        };
        assert_ne!(
            compare_discard_evaluations(&melded("E"), &melded("5m")).reason,
            DiscardComparisonReason::ChiitoitsuWaitQuality
        );
    }

    #[test]
    fn diagnose_reports_chiitoitsu_wait_quality_reason() {
        // 診断は production comparator が返した理由をそのまま載せる。
        let counts = counts(&chiitoitsu_tanki_hand("E", "1s"));
        let evaluations = evaluate_discards(&counts);
        let report = diagnose_discard_evaluations(&counts, &evaluations);

        let selected = report.selected.as_ref().expect("打牌を選べる");
        assert_eq!(selected.discard, tile("1s"));
        let loser = report
            .candidates
            .iter()
            .find(|candidate| candidate.evaluation.discard == tile("E"))
            .expect("打 E も候補になる");
        assert!(loser.selected_is_strictly_better_than_candidate);
        assert_eq!(
            loser.comparison_reason,
            DiscardComparisonReason::ChiitoitsuWaitQuality
        );
    }

    fn floating_evaluation(floating: i16) -> DiscardEvaluation {
        let mut evaluation = evaluation(1, 10, 2, 0, false);
        evaluation.floating_tile_value = floating;
        evaluation
    }

    fn loser_candidate(
        winner: DiscardEvaluation,
        loser: DiscardEvaluation,
    ) -> DiscardCandidateDiagnostic {
        let report = diagnose_discard_evaluations(&TileCounts::new(), &[winner, loser]);
        assert!(report.candidates[0].selected);
        assert!(!report.candidates[1].selected);
        report.candidates[1].clone()
    }

    #[test]
    fn diagnose_selected_matches_select_best_discard() {
        let counts = counts(&[
            "1m", "2m", "3m", "5m", "6m", "9m", "1p", "2p", "3p", "5s", "5s", "E", "E", "W",
        ]);
        let evaluations = evaluate_discards(&counts);
        let selected = select_best_discard(&counts).unwrap();
        let report = diagnose_discard_evaluations(&counts, &evaluations);
        assert_eq!(report.selected.as_ref(), Some(&selected));

        let report_discards: Vec<_> = report
            .candidates
            .iter()
            .map(|candidate| candidate.evaluation.discard)
            .collect();
        assert_eq!(report_discards, discard_tiles(&evaluations));

        let selected_candidates: Vec<_> = report
            .candidates
            .iter()
            .filter(|candidate| candidate.selected)
            .collect();
        assert_eq!(selected_candidates.len(), 1);
        assert_eq!(selected_candidates[0].evaluation, selected);
        assert!(!selected_candidates[0].selected_is_strictly_better_than_candidate);
        assert_eq!(
            selected_candidates[0].comparison_reason,
            DiscardComparisonReason::StableOrder
        );
    }

    #[test]
    fn diagnose_empty_evaluations_has_no_selection() {
        let report = diagnose_discard_evaluations(&TileCounts::new(), &[]);
        assert_eq!(report.selected, None);
        assert!(report.candidates.is_empty());
    }

    #[test]
    fn diagnose_single_candidate_is_selected() {
        let candidate = evaluation(1, 10, 2, 0, false);
        let report =
            diagnose_discard_evaluations(&TileCounts::new(), std::slice::from_ref(&candidate));
        assert_eq!(report.selected, Some(candidate.clone()));
        assert_eq!(report.candidates.len(), 1);
        assert!(report.candidates[0].selected);
        assert!(!report.candidates[0].selected_is_strictly_better_than_candidate);
        assert_eq!(
            report.candidates[0].comparison_reason,
            DiscardComparisonReason::StableOrder
        );
    }

    #[test]
    fn diagnose_reports_shanten_reason() {
        let winner = evaluation(0, 10, 2, 0, false);
        let loser = evaluation(1, 10, 2, 0, false);
        let candidate = loser_candidate(winner, loser);
        assert!(candidate.selected_is_strictly_better_than_candidate);
        assert_eq!(
            candidate.comparison_reason,
            DiscardComparisonReason::Shanten
        );
    }

    #[test]
    fn diagnose_reports_acceptance_remaining_reason() {
        let winner = evaluation(1, 20, 1, 0, false);
        let loser = evaluation(1, 10, 1, 0, false);
        let candidate = loser_candidate(winner, loser);
        assert!(candidate.selected_is_strictly_better_than_candidate);
        assert_eq!(
            candidate.comparison_reason,
            DiscardComparisonReason::AcceptanceRemaining
        );
    }

    #[test]
    fn diagnose_reports_acceptance_type_count_reason() {
        let winner = evaluation(1, 10, 3, 0, false);
        let loser = evaluation(1, 10, 2, 0, false);
        let candidate = loser_candidate(winner, loser);
        assert!(candidate.selected_is_strictly_better_than_candidate);
        assert_eq!(
            candidate.comparison_reason,
            DiscardComparisonReason::AcceptanceTypeCount
        );
    }

    #[test]
    fn diagnose_reports_shape_penalty_reason() {
        let winner = evaluation_with_shape_penalty(1, 10, 2, 0, 0, 0, false);
        let loser = evaluation_with_shape_penalty(1, 10, 2, 10, 0, 0, false);
        let candidate = loser_candidate(winner, loser);
        assert!(candidate.selected_is_strictly_better_than_candidate);
        assert_eq!(
            candidate.comparison_reason,
            DiscardComparisonReason::ShapePenalty
        );
    }

    #[test]
    fn diagnose_reports_floating_tile_value_reason() {
        let winner = floating_evaluation(0);
        let loser = floating_evaluation(5);
        let candidate = loser_candidate(winner, loser);
        assert!(candidate.selected_is_strictly_better_than_candidate);
        assert_eq!(
            candidate.comparison_reason,
            DiscardComparisonReason::FloatingTileValue
        );
    }

    #[test]
    fn diagnose_reports_dora_reason() {
        let winner = evaluation(1, 10, 2, 0, false);
        let loser = evaluation(1, 10, 2, 1, false);
        let candidate = loser_candidate(winner, loser);
        assert!(candidate.selected_is_strictly_better_than_candidate);
        assert_eq!(candidate.comparison_reason, DiscardComparisonReason::Dora);
    }

    #[test]
    fn diagnose_reports_value_honor_reason() {
        let winner = evaluation_with_value_honor(1, 10, 2, 0, 0, false);
        let loser = evaluation_with_value_honor(1, 10, 2, 0, 1, false);
        let candidate = loser_candidate(winner, loser);
        assert!(candidate.selected_is_strictly_better_than_candidate);
        assert_eq!(
            candidate.comparison_reason,
            DiscardComparisonReason::ValueHonor
        );
    }

    #[test]
    fn diagnose_reports_red_five_reason() {
        let winner = evaluation(1, 10, 2, 0, false);
        let loser = evaluation(1, 10, 2, 0, true);
        let candidate = loser_candidate(winner, loser);
        assert!(candidate.selected_is_strictly_better_than_candidate);
        assert_eq!(
            candidate.comparison_reason,
            DiscardComparisonReason::RedFive
        );
    }

    #[test]
    fn diagnose_perfect_tie_keeps_first_candidate() {
        let first = evaluation_with_value_honor(1, 10, 2, 1, 1, true);
        let second = evaluation_with_value_honor(1, 10, 2, 1, 1, true);
        let report = diagnose_discard_evaluations(&TileCounts::new(), &[first.clone(), second]);

        assert_eq!(report.selected, Some(first));
        assert!(report.candidates[0].selected);
        assert!(!report.candidates[1].selected);
        assert!(!report.candidates[1].selected_is_strictly_better_than_candidate);
        assert_eq!(
            report.candidates[1].comparison_reason,
            DiscardComparisonReason::StableOrder
        );
    }

    #[test]
    fn diagnose_exposes_shape_breakdown_per_candidate() {
        let counts = counts(&["2m", "3m", "5m", "7m", "1p", "1p"]);
        let evaluations = evaluate_discards(&counts);
        let report = diagnose_discard_evaluations(&counts, &evaluations);

        for candidate in &report.candidates {
            let discard = candidate.evaluation.discard;
            assert_eq!(
                candidate.shape_breakdown,
                shape_breakdown_for_discard(&counts, discard)
            );
            assert_eq!(
                candidate.pair_context,
                pair_context_for_discard(&counts, discard)
            );
            assert_eq!(
                candidate.block_context,
                discard_block_context(&counts, discard)
            );
            assert_eq!(
                candidate.floating_tile_value_breakdown,
                floating_tile_value_breakdown_for_discard(&counts, discard)
            );
            assert_eq!(
                candidate.evaluation.shape_penalty,
                shape_penalty_for_discard(&counts, discard)
            );
        }

        let ryanmen = report
            .candidates
            .iter()
            .find(|candidate| candidate.evaluation.discard == tile("2m"))
            .unwrap();
        assert!(ryanmen.shape_breakdown.breaks_ryanmen);

        let kanchan = report
            .candidates
            .iter()
            .find(|candidate| candidate.evaluation.discard == tile("5m"))
            .unwrap();
        assert!(kanchan.shape_breakdown.breaks_kanchan);
    }

    #[test]
    fn diagnose_does_not_modify_inputs() {
        let counts = counts(&[
            "1m", "2m", "3m", "4m", "5m", "6m", "7m", "8m", "9m", "1p", "2p", "3p", "5s", "5s",
        ]);
        let evaluations = evaluate_discards(&counts);
        let counts_before = counts;
        let evaluations_before = evaluations.clone();
        let _ = diagnose_discard_evaluations(&counts, &evaluations);
        assert_eq!(counts, counts_before);
        assert_eq!(evaluations, evaluations_before);
    }

    fn isolated_evaluation(
        min: i8,
        remaining: u8,
        type_count: usize,
        isolated: bool,
    ) -> DiscardEvaluation {
        let mut evaluation = evaluation(min, remaining, type_count, 0, false);
        evaluation.discards_isolated_tile = isolated;
        evaluation
    }

    #[test]
    fn real_hand_prefers_isolated_tile_over_taatsu_tile() {
        // 2m 6m 2p 3p 4p 5p 8p 9p 1s 1s 2s 2s 4s 8s, ドラ表示 7p, 場風 S, 自風 W
        // 4s は 2s2s4s の嵌張候補を構成するため孤立牌ではない。2m・6m・8s は孤立単騎牌。
        // 3向聴のまま孤立牌を優先して切るため、選択牌は 4s ではなく孤立牌側になる。
        let tiles = ids(&[4, 20, 40, 44, 48, 53, 64, 68, 72, 73, 76, 77, 84, 100]);
        let indicators = ids(&[60]);
        let evaluations = evaluate_discards_from_tiles_with_context(
            &tiles,
            &indicators,
            Some(tile("S")),
            Some(tile("W")),
        );

        let four_s = discard_evaluation(&evaluations, tile("4s"));
        assert!(!four_s.discards_isolated_tile);
        for isolated in ["2m", "6m", "8s"] {
            let evaluation = discard_evaluation(&evaluations, tile(isolated));
            assert!(
                evaluation.discards_isolated_tile,
                "{isolated} should be isolated"
            );
        }

        let selected = select_best(evaluations).unwrap();
        assert_ne!(selected.discard, tile("4s"));
        assert!(selected.discards_isolated_tile);
        assert_eq!(selected.discard, tile("2m"));
    }

    #[test]
    fn shanten_outranks_isolated_tile() {
        // 孤立牌を切ると3向聴、非孤立牌を切ると2向聴。向聴改善が優先される。
        let isolated_worse_shanten = isolated_evaluation(3, 10, 2, true);
        let taatsu_better_shanten = isolated_evaluation(2, 4, 1, false);
        let comparison =
            compare_discard_evaluations(&taatsu_better_shanten, &isolated_worse_shanten);
        assert!(comparison.candidate_is_better);
        assert_eq!(comparison.reason, DiscardComparisonReason::Shanten);
    }

    #[test]
    fn multi_shanten_isolated_outranks_acceptance() {
        // 両候補とも3向聴・特殊牌情報同一。孤立牌候補は受け入れが少なくても優先される。
        let isolated = isolated_evaluation(3, 10, 1, true);
        let taatsu = isolated_evaluation(3, 40, 5, false);
        let comparison = compare_discard_evaluations(&isolated, &taatsu);
        assert!(comparison.candidate_is_better);
        assert_eq!(comparison.reason, DiscardComparisonReason::IsolatedTile);
    }

    #[test]
    fn iishanten_does_not_apply_isolated_tile() {
        // 一向聴では孤立牌軸を適用せず、受け入れ枚数が優先される。
        let isolated = isolated_evaluation(1, 10, 1, true);
        let taatsu = isolated_evaluation(1, 40, 5, false);
        let comparison = compare_discard_evaluations(&taatsu, &isolated);
        assert!(comparison.candidate_is_better);
        assert_eq!(
            comparison.reason,
            DiscardComparisonReason::AcceptanceRemaining
        );
    }

    #[test]
    fn tenpai_does_not_apply_isolated_tile() {
        // テンパイでは孤立牌軸で決着しない。孤立牌情報以外を同一にすると StableOrder になる。
        let isolated = isolated_evaluation(0, 10, 2, true);
        let taatsu = isolated_evaluation(0, 10, 2, false);
        let comparison = compare_discard_evaluations(&isolated, &taatsu);
        assert!(!comparison.candidate_is_better);
        assert_eq!(comparison.reason, DiscardComparisonReason::StableOrder);
        assert!(compare_isolated_tile_discard(&isolated, &taatsu).is_none());
    }

    #[test]
    fn isolated_axis_undecided_between_two_isolated() {
        // 両候補とも孤立牌なら孤立牌軸では決着せず、受け入れ以下の既存比較へ進む。
        let more = isolated_evaluation(3, 40, 5, true);
        let less = isolated_evaluation(3, 10, 1, true);
        assert!(compare_isolated_tile_discard(&more, &less).is_none());
        let comparison = compare_discard_evaluations(&more, &less);
        assert_eq!(
            comparison.reason,
            DiscardComparisonReason::AcceptanceRemaining
        );
    }

    #[test]
    fn isolated_axis_undecided_between_two_non_isolated() {
        // 両候補とも非孤立牌なら孤立牌軸では決着しない。
        let more = isolated_evaluation(3, 40, 5, false);
        let less = isolated_evaluation(3, 10, 1, false);
        assert!(compare_isolated_tile_discard(&more, &less).is_none());
    }

    #[test]
    fn isolated_axis_yields_to_dora_difference() {
        // 孤立牌候補がドラを切る場合は孤立牌軸で決着させない。
        let isolated_dora = isolated_evaluation(3, 10, 1, true);
        let mut isolated_dora = isolated_dora;
        isolated_dora.discarded_dora_count = 1;
        let taatsu = isolated_evaluation(3, 40, 5, false);
        assert!(compare_isolated_tile_discard(&isolated_dora, &taatsu).is_none());
        let comparison = compare_discard_evaluations(&isolated_dora, &taatsu);
        assert_ne!(comparison.reason, DiscardComparisonReason::IsolatedTile);
    }

    #[test]
    fn isolated_value_honor_is_eligible_like_normal_isolated() {
        // 非ドラの孤立役牌は孤立牌優先対象になり、非孤立候補に対して IsolatedTile 軸で先に切られる。
        let mut isolated_honor = isolated_evaluation(3, 10, 1, true);
        isolated_honor.discarded_value_honor_count = 1;
        let taatsu = isolated_evaluation(3, 40, 5, false);
        let comparison = compare_isolated_tile_discard(&isolated_honor, &taatsu).unwrap();
        assert!(comparison.candidate_is_better);
        assert_eq!(comparison.reason, DiscardComparisonReason::IsolatedTile);
    }

    #[test]
    fn isolated_axis_yields_to_red_five_difference() {
        // 孤立牌候補が赤5を切る場合は孤立牌軸で決着させない。
        let mut isolated_red = isolated_evaluation(3, 10, 1, true);
        isolated_red.discards_red_five = true;
        let taatsu = isolated_evaluation(3, 40, 5, false);
        assert!(compare_isolated_tile_discard(&isolated_red, &taatsu).is_none());
        let comparison = compare_discard_evaluations(&isolated_red, &taatsu);
        assert_ne!(comparison.reason, DiscardComparisonReason::IsolatedTile);
    }

    #[test]
    fn visible_tiles_do_not_change_isolation_flag() {
        // 通常経路と visible tiles 経路で、同じ打牌候補の floating_tile_value と
        // discards_isolated_tile が一致する。visible tiles は孤立牌判定へ影響しない。
        let tiles = ids(&[4, 20, 40, 44, 48, 53, 64, 68, 72, 73, 76, 77, 84, 100]);
        let indicators = ids(&[60]);
        let plain = evaluate_discards_from_tiles_with_context(
            &tiles,
            &indicators,
            Some(tile("S")),
            Some(tile("W")),
        );

        let mut visible = tiles.clone();
        visible.extend(ids(&[85, 86, 101, 102]));
        let with_visible = evaluate_discards_from_tiles_with_visible_tiles(
            &tiles,
            &indicators,
            Some(tile("S")),
            Some(tile("W")),
            &visible,
        );

        for plain_evaluation in &plain {
            let visible_evaluation = discard_evaluation(&with_visible, plain_evaluation.discard);
            assert_eq!(
                plain_evaluation.floating_tile_value,
                visible_evaluation.floating_tile_value
            );
            assert_eq!(
                plain_evaluation.discards_isolated_tile,
                visible_evaluation.discards_isolated_tile
            );
        }
    }

    #[test]
    fn diagnostic_reports_isolated_tile_reason() {
        // 多向聴の孤立牌候補が選ばれ、非孤立の 4s 候補の comparison_reason が IsolatedTile。
        let tiles = ids(&[4, 20, 40, 44, 48, 53, 64, 68, 72, 73, 76, 77, 84, 100]);
        let indicators = ids(&[60]);
        let evaluations = evaluate_discards_from_tiles_with_context(
            &tiles,
            &indicators,
            Some(tile("S")),
            Some(tile("W")),
        );
        let counts = TileCounts::from_tiles(tiles.iter().copied());
        let diagnostic = diagnose_discard_evaluations(&counts, &evaluations);

        let selected = diagnostic.selected.as_ref().unwrap();
        assert_eq!(selected.discard, tile("2m"));
        assert!(selected.discards_isolated_tile);

        let four_s = diagnostic
            .candidates
            .iter()
            .find(|candidate| candidate.evaluation.discard == tile("4s"))
            .unwrap();
        assert!(!four_s.selected);
        assert_eq!(
            four_s.comparison_reason,
            DiscardComparisonReason::IsolatedTile
        );
    }

    // 3向聴・受け入れ牌種2で固定した比較キー検証用の候補。孤立牌フラグと特殊牌情報、
    // 打牌牌種、受け入れ枚数だけを変えて eligibility の推移律を確認する。
    fn priority_candidate(
        discard_index: u8,
        remaining: u8,
        isolated: bool,
        dora: u8,
        value_honor: u8,
        red: bool,
    ) -> DiscardEvaluation {
        let mut evaluation = evaluation(3, remaining, 2, dora, red);
        evaluation.discard = TileType::new(discard_index).unwrap();
        evaluation.discards_isolated_tile = isolated;
        evaluation.discarded_value_honor_count = value_honor;
        evaluation
    }

    // 再現例の A/B/C。A は通常孤立牌、B は通常非孤立牌、C は孤立ドラ相当(非eligible)。
    fn cycle_candidate_a() -> DiscardEvaluation {
        priority_candidate(0, 10, true, 0, 0, false)
    }
    fn cycle_candidate_b() -> DiscardEvaluation {
        priority_candidate(1, 40, false, 0, 0, false)
    }
    fn cycle_candidate_c() -> DiscardEvaluation {
        priority_candidate(2, 20, false, 1, 0, false)
    }

    #[test]
    fn isolated_priority_eligibility_is_candidate_intrinsic() {
        // 非ドラの孤立牌は役牌でも eligible。孤立ドラ・孤立赤5・非孤立牌は eligible=false。
        assert!(isolated_tile_priority_eligible(&priority_candidate(
            0, 10, true, 0, 0, false
        )));
        assert!(!isolated_tile_priority_eligible(&priority_candidate(
            0, 10, true, 1, 0, false
        )));
        // 非ドラの孤立役牌（value_honor=1）は eligible。
        assert!(isolated_tile_priority_eligible(&priority_candidate(
            0, 10, true, 0, 1, false
        )));
        // 連風牌相当（value_honor=2）も eligible。
        assert!(isolated_tile_priority_eligible(&priority_candidate(
            0, 10, true, 0, 2, false
        )));
        // ドラ役牌は eligible=false。
        assert!(!isolated_tile_priority_eligible(&priority_candidate(
            0, 10, true, 1, 1, false
        )));
        assert!(!isolated_tile_priority_eligible(&priority_candidate(
            0, 10, true, 0, 0, true
        )));
        assert!(!isolated_tile_priority_eligible(&priority_candidate(
            0, 10, false, 0, 0, false
        )));
    }

    #[test]
    fn resolves_former_comparison_cycle() {
        // 旧実装は A>B(IsolatedTile), B>C(AcceptanceRemaining), C>A(AcceptanceRemaining) で循環した。
        // 修正後は C>A が成立せず A>C(IsolatedTile) となり循環が解消する。
        let a = cycle_candidate_a();
        let b = cycle_candidate_b();
        let c = cycle_candidate_c();

        let ab = compare_discard_evaluations(&a, &b);
        assert!(ab.candidate_is_better);
        assert_eq!(ab.reason, DiscardComparisonReason::IsolatedTile);

        let ac = compare_discard_evaluations(&a, &c);
        assert!(ac.candidate_is_better);
        assert_eq!(ac.reason, DiscardComparisonReason::IsolatedTile);

        let bc = compare_discard_evaluations(&b, &c);
        assert!(bc.candidate_is_better);
        assert_eq!(bc.reason, DiscardComparisonReason::AcceptanceRemaining);

        // C は A に勝たない(旧実装の循環要因)。
        assert!(!compare_discard_evaluations(&c, &a).candidate_is_better);
    }

    #[test]
    fn selection_is_order_independent_across_permutations() {
        let a = cycle_candidate_a();
        let b = cycle_candidate_b();
        let c = cycle_candidate_c();
        let permutations = [
            [a.clone(), b.clone(), c.clone()],
            [a.clone(), c.clone(), b.clone()],
            [b.clone(), a.clone(), c.clone()],
            [b.clone(), c.clone(), a.clone()],
            [c.clone(), a.clone(), b.clone()],
            [c.clone(), b.clone(), a.clone()],
        ];
        for permutation in permutations {
            let selected = select_best(permutation.to_vec()).unwrap();
            assert_eq!(selected.discard, a.discard);
        }
    }

    #[test]
    fn comparison_is_transitive_for_cycle_candidates() {
        let a = cycle_candidate_a();
        let b = cycle_candidate_b();
        let c = cycle_candidate_c();
        assert!(compare_discard_evaluations(&a, &b).candidate_is_better);
        assert!(compare_discard_evaluations(&b, &c).candidate_is_better);
        assert!(compare_discard_evaluations(&a, &c).candidate_is_better);
    }

    #[test]
    fn comparison_is_antisymmetric_for_cycle_candidates() {
        let a = cycle_candidate_a();
        let b = cycle_candidate_b();
        let c = cycle_candidate_c();
        for (x, y) in [(&a, &b), (&a, &c), (&b, &c)] {
            assert!(compare_discard_evaluations(x, y).candidate_is_better);
            assert!(!compare_discard_evaluations(y, x).candidate_is_better);
        }
    }

    #[test]
    fn normal_isolated_beats_non_isolated_special_tiles() {
        // 通常孤立牌は、非孤立のドラ・役牌・赤5より IsolatedTile で優先される。
        let normal_isolated = priority_candidate(0, 10, true, 0, 0, false);
        let non_isolated_dora = priority_candidate(1, 40, false, 1, 0, false);
        let non_isolated_honor = priority_candidate(2, 40, false, 0, 1, false);
        let non_isolated_red = priority_candidate(3, 40, false, 0, 0, true);
        for opponent in [&non_isolated_dora, &non_isolated_honor, &non_isolated_red] {
            let comparison = compare_discard_evaluations(&normal_isolated, opponent);
            assert!(comparison.candidate_is_better);
            assert_eq!(comparison.reason, DiscardComparisonReason::IsolatedTile);
        }
    }

    #[test]
    fn isolated_dora_is_protected_by_dora_axis_when_else_equal() {
        // 孤立ドラと通常非孤立牌は共に非eligible。他軸同値なら Dora 比較で非孤立牌が勝つ。
        let isolated_dora = priority_candidate(0, 10, true, 1, 0, false);
        let non_isolated = priority_candidate(1, 10, false, 0, 0, false);
        assert!(compare_isolated_tile_discard(&non_isolated, &isolated_dora).is_none());
        let comparison = compare_discard_evaluations(&non_isolated, &isolated_dora);
        assert!(comparison.candidate_is_better);
        assert_eq!(comparison.reason, DiscardComparisonReason::Dora);
    }

    #[test]
    fn isolated_value_honor_is_preferred_over_non_isolated() {
        // 非ドラの孤立役牌は eligible。非孤立候補に対して IsolatedTile 軸で孤立役牌側が先に切られる。
        let isolated_honor = priority_candidate(0, 10, true, 0, 1, false);
        let non_isolated = priority_candidate(1, 10, false, 0, 0, false);
        let comparison = compare_discard_evaluations(&isolated_honor, &non_isolated);
        assert!(comparison.candidate_is_better);
        assert_eq!(comparison.reason, DiscardComparisonReason::IsolatedTile);
    }

    #[test]
    fn isolated_red_five_is_protected_by_red_five_axis_when_else_equal() {
        let isolated_red = priority_candidate(0, 10, true, 0, 0, true);
        let non_isolated = priority_candidate(1, 10, false, 0, 0, false);
        assert!(compare_isolated_tile_discard(&non_isolated, &isolated_red).is_none());
        let comparison = compare_discard_evaluations(&non_isolated, &isolated_red);
        assert!(comparison.candidate_is_better);
        assert_eq!(comparison.reason, DiscardComparisonReason::RedFive);
    }

    #[test]
    fn isolated_axis_undecided_between_non_eligible_candidates() {
        // 非eligible同士(非孤立牌・孤立ドラ・孤立赤5)は IsolatedTile で決着しない。
        // 孤立役牌は eligible になったためこの群には含めない。
        let non_isolated = priority_candidate(0, 10, false, 0, 0, false);
        let isolated_dora = priority_candidate(1, 10, true, 1, 0, false);
        let isolated_red = priority_candidate(2, 10, true, 0, 0, true);
        let candidates = [&non_isolated, &isolated_dora, &isolated_red];
        for (i, x) in candidates.iter().enumerate() {
            for y in candidates.iter().skip(i + 1) {
                assert!(compare_isolated_tile_discard(x, y).is_none());
                assert!(compare_isolated_tile_discard(y, x).is_none());
            }
        }
    }

    #[test]
    fn diagnostic_is_consistent_for_cycle_candidates() {
        // A/B/C を診断すると A が選ばれ、B・C の理由が IsolatedTile。
        // どの非選択候補も selected に直接勝たない。
        let a = cycle_candidate_a();
        let b = cycle_candidate_b();
        let c = cycle_candidate_c();
        let evaluations = vec![a.clone(), b.clone(), c.clone()];
        let counts = TileCounts::new();
        let diagnostic = diagnose_discard_evaluations(&counts, &evaluations);

        let selected = diagnostic.selected.as_ref().unwrap();
        assert_eq!(selected.discard, a.discard);

        for candidate in &diagnostic.candidates {
            if candidate.selected {
                continue;
            }
            assert_eq!(
                candidate.comparison_reason,
                DiscardComparisonReason::IsolatedTile
            );
            assert!(
                !compare_discard_evaluations(&candidate.evaluation, selected).candidate_is_better
            );
        }
    }

    // 孤立字牌軸(IsolatedHonor)検証用の合成候補。字牌(index>=27)と数牌を、eligibility 要素と
    // 向聴を指定して作る。受け入れ牌種は2で固定する。
    fn isolated_axis_candidate(
        discard_index: u8,
        min: i8,
        remaining: u8,
        isolated: bool,
        dora: u8,
        value_honor: u8,
        red: bool,
    ) -> DiscardEvaluation {
        let mut evaluation = evaluation(min, remaining, 2, dora, red);
        evaluation.discard = TileType::new(discard_index).unwrap();
        evaluation.discards_isolated_tile = isolated;
        evaluation.discarded_value_honor_count = value_honor;
        evaluation
    }

    #[test]
    fn isolated_honor_outranks_isolated_number() {
        // 孤立役牌(C 中, index 33)と孤立数牌(1m)。受け入れが数牌側で大きくても字牌を先に切る。
        let honor = isolated_axis_candidate(33, 3, 10, true, 0, 1, false);
        let number = isolated_axis_candidate(0, 3, 40, true, 0, 0, false);
        let forward = compare_discard_evaluations(&honor, &number);
        assert!(forward.candidate_is_better);
        assert_eq!(forward.reason, DiscardComparisonReason::IsolatedHonor);
        // 候補順を入れ替えても数牌側は勝たず、軸も同じ。
        let backward = compare_discard_evaluations(&number, &honor);
        assert!(!backward.candidate_is_better);
        assert_eq!(backward.reason, DiscardComparisonReason::IsolatedHonor);
    }

    #[test]
    fn isolated_guest_wind_outranks_isolated_number() {
        // 孤立客風(E, value_honor=0)と孤立数牌。役牌でなくても字牌を先に切る。
        let wind = isolated_axis_candidate(27, 3, 10, true, 0, 0, false);
        let number = isolated_axis_candidate(0, 3, 40, true, 0, 0, false);
        let comparison = compare_discard_evaluations(&wind, &number);
        assert!(comparison.candidate_is_better);
        assert_eq!(comparison.reason, DiscardComparisonReason::IsolatedHonor);
    }

    #[test]
    fn isolated_honor_axis_undecided_between_two_honors() {
        // 両方字牌(客風 E と 役牌 C)なら新軸では決着せず、受け入れ以下の既存比較へ進む。
        // 受け入れ・牌種を揃えると後段の ValueHonor で客風側が切られる。
        let guest_wind = isolated_axis_candidate(27, 3, 10, true, 0, 0, false);
        let dragon = isolated_axis_candidate(33, 3, 10, true, 0, 1, false);
        assert!(compare_isolated_honor_discard(&guest_wind, &dragon).is_none());
        let comparison = compare_discard_evaluations(&guest_wind, &dragon);
        assert!(comparison.candidate_is_better);
        assert_eq!(comparison.reason, DiscardComparisonReason::ValueHonor);
    }

    #[test]
    fn isolated_honor_axis_undecided_between_honor_and_double_wind() {
        // 孤立役牌(白, value_honor=1)と孤立連風牌(E, value_honor=2)。両方字牌なので新軸では決着しない。
        let yakuhai = isolated_axis_candidate(31, 3, 10, true, 0, 1, false);
        let double_wind = isolated_axis_candidate(27, 3, 10, true, 0, 2, false);
        assert!(compare_isolated_honor_discard(&yakuhai, &double_wind).is_none());
    }

    #[test]
    fn isolated_honor_axis_undecided_between_two_numbers() {
        // 両方数牌(1m と 5m)なら新軸では決着させない。
        let terminal = isolated_axis_candidate(0, 3, 10, true, 0, 0, false);
        let middle = isolated_axis_candidate(4, 3, 10, true, 0, 0, false);
        assert!(compare_isolated_honor_discard(&terminal, &middle).is_none());
    }

    #[test]
    fn isolated_dora_honor_does_not_apply_honor_axis() {
        // 孤立ドラ字牌は eligible でないため新軸の対象外。孤立数牌を切る。
        let dora_honor = isolated_axis_candidate(33, 3, 10, true, 1, 1, false);
        let number = isolated_axis_candidate(0, 3, 40, true, 0, 0, false);
        assert!(compare_isolated_honor_discard(&dora_honor, &number).is_none());
        // 数牌が eligible、ドラ字牌は非eligible → IsolatedTile 軸で数牌切りが優先。
        let comparison = compare_discard_evaluations(&number, &dora_honor);
        assert!(comparison.candidate_is_better);
        assert_eq!(comparison.reason, DiscardComparisonReason::IsolatedTile);
    }

    #[test]
    fn isolated_non_dora_honor_outranks_isolated_dora_number() {
        // 孤立非ドラ字牌 vs 孤立ドラ数牌。字牌側が eligible、ドラ数牌は非eligible → 字牌切り。
        let honor = isolated_axis_candidate(33, 3, 10, true, 0, 1, false);
        let dora_number = isolated_axis_candidate(0, 3, 40, true, 1, 0, false);
        let comparison = compare_discard_evaluations(&honor, &dora_number);
        assert!(comparison.candidate_is_better);
        assert_eq!(comparison.reason, DiscardComparisonReason::IsolatedTile);
    }

    #[test]
    fn shanten_outranks_isolated_honor_axis() {
        // 孤立字牌切りが3向聴、孤立数牌切りが2向聴なら向聴改善が優先。
        let honor = isolated_axis_candidate(33, 3, 10, true, 0, 1, false);
        let number = isolated_axis_candidate(0, 2, 40, true, 0, 0, false);
        let comparison = compare_discard_evaluations(&number, &honor);
        assert!(comparison.candidate_is_better);
        assert_eq!(comparison.reason, DiscardComparisonReason::Shanten);
        assert!(compare_isolated_honor_discard(&honor, &number).is_none());
    }

    #[test]
    fn iishanten_does_not_apply_isolated_honor_axis() {
        // 一向聴では新軸を適用しない。
        let honor = isolated_axis_candidate(33, 1, 10, true, 0, 1, false);
        let number = isolated_axis_candidate(0, 1, 40, true, 0, 0, false);
        assert!(compare_isolated_honor_discard(&honor, &number).is_none());
    }

    #[test]
    fn tenpai_does_not_apply_isolated_honor_axis() {
        // テンパイでは新軸を適用しない。
        let honor = isolated_axis_candidate(33, 0, 10, true, 0, 1, false);
        let number = isolated_axis_candidate(0, 0, 40, true, 0, 0, false);
        assert!(compare_isolated_honor_discard(&honor, &number).is_none());
    }

    #[test]
    fn isolated_honor_axis_selection_is_order_independent() {
        // 孤立役牌 > 孤立数牌 > 非孤立牌。3候補の全順列で孤立役牌が選ばれる。
        let honor = isolated_axis_candidate(33, 3, 10, true, 0, 1, false);
        let number = isolated_axis_candidate(0, 3, 40, true, 0, 0, false);
        let non_isolated = isolated_axis_candidate(4, 3, 40, false, 0, 0, false);
        let permutations = [
            [honor.clone(), number.clone(), non_isolated.clone()],
            [honor.clone(), non_isolated.clone(), number.clone()],
            [number.clone(), honor.clone(), non_isolated.clone()],
            [number.clone(), non_isolated.clone(), honor.clone()],
            [non_isolated.clone(), honor.clone(), number.clone()],
            [non_isolated.clone(), number.clone(), honor.clone()],
        ];
        for permutation in permutations {
            let selected = select_best(permutation.to_vec()).unwrap();
            assert_eq!(selected.discard, honor.discard);
        }
    }

    #[test]
    fn regression_guest_wind_discarded_before_isolated_number() {
        // 場風 E, 自風 S。W は客風(非役牌)の孤立単騎、8s も孤立単騎。ドラ表示なし。
        // 手牌 3m4m5m 3p4p5p6p7p 2s2s 5s 8s 9m W は2向聴で、W を切っても 8s を切っても2向聴で不変。
        // 旧比較では客風 W と孤立数牌がともに孤立牌優先対象で IsolatedTile 軸では決着せず、
        // 受け入れ比較(W が受け入れ最大=50)で W が選ばれていた(reason=AcceptanceRemaining)。
        // 新比較では孤立字牌軸 IsolatedHonor で W を数牌より先に切り、決着理由を明示する。
        let tiles = ids_of(&[
            "3m", "4m", "5m", "3p", "4p", "5p", "6p", "7p", "2s", "2s", "5s", "8s", "9m", "W",
        ]);
        let evaluations = evaluate_discards_from_tiles_with_context(
            &tiles,
            &[],
            Some(tile("E")),
            Some(tile("S")),
        );

        let wind = discard_evaluation(&evaluations, tile("W"));
        let number = discard_evaluation(&evaluations, tile("8s"));
        assert!(wind.discards_isolated_tile);
        assert!(number.discards_isolated_tile);
        assert_eq!(wind.discarded_value_honor_count, 0);
        assert_eq!(wind.discarded_dora_count, 0);
        // どちらを切っても同じ2向聴。
        assert_eq!(wind.min_shanten_after_discard(), 2);
        assert_eq!(number.min_shanten_after_discard(), 2);

        let selected = select_best(evaluations.clone()).unwrap();
        assert_eq!(selected.discard, tile("W"));

        let counts = TileCounts::from_tiles(tiles.iter().copied());
        let diagnostic = diagnose_discard_evaluations(&counts, &evaluations);
        let eight_s = diagnostic
            .candidates
            .iter()
            .find(|c| c.evaluation.discard == tile("8s"))
            .unwrap();
        assert!(!eight_s.selected);
        assert_eq!(
            eight_s.comparison_reason,
            DiscardComparisonReason::IsolatedHonor
        );
    }

    #[test]
    fn regression_yakuhai_discarded_before_isolated_number() {
        // 場風 E, 自風 S。C(中)は三元牌=役牌の孤立単騎、8s も孤立単騎。C はドラではない。
        // 手牌 3m4m5m 3p4p5p6p7p 2s2s 5s 8s 9m C は2向聴で、C を切っても 8s を切っても2向聴で不変。
        // 旧比較では役牌 C が孤立牌優先対象から除外され、孤立数牌 8s が eligible だったため、
        // IsolatedTile 軸で 8s が先に切られていた(reason=IsolatedTile で数牌切り)。
        // 新比較では C を孤立牌優先対象へ含め、孤立字牌軸 IsolatedHonor で C を数牌より先に切る。
        let tiles = ids_of(&[
            "3m", "4m", "5m", "3p", "4p", "5p", "6p", "7p", "2s", "2s", "5s", "8s", "9m", "C",
        ]);
        let evaluations = evaluate_discards_from_tiles_with_context(
            &tiles,
            &[],
            Some(tile("E")),
            Some(tile("S")),
        );

        let dragon = discard_evaluation(&evaluations, tile("C"));
        let number = discard_evaluation(&evaluations, tile("8s"));
        assert!(dragon.discards_isolated_tile);
        assert!(number.discards_isolated_tile);
        assert!(dragon.discarded_value_honor_count > 0);
        assert_eq!(dragon.discarded_dora_count, 0);
        // どちらを切っても同じ2向聴。
        assert_eq!(dragon.min_shanten_after_discard(), 2);
        assert_eq!(number.min_shanten_after_discard(), 2);

        let selected = select_best(evaluations.clone()).unwrap();
        assert_eq!(selected.discard, tile("C"));

        let counts = TileCounts::from_tiles(tiles.iter().copied());
        let diagnostic = diagnose_discard_evaluations(&counts, &evaluations);
        let eight_s = diagnostic
            .candidates
            .iter()
            .find(|c| c.evaluation.discard == tile("8s"))
            .unwrap();
        assert!(!eight_s.selected);
        assert_eq!(
            eight_s.comparison_reason,
            DiscardComparisonReason::IsolatedHonor
        );
    }

    // ---- 副露済み面子数を考慮した打牌評価 ----

    fn menzen_hands() -> Vec<TileCounts> {
        vec![
            counts(&[
                "1m", "2m", "3m", "4m", "5m", "6m", "7m", "8m", "9m", "1p", "2p", "3p", "5s", "5s",
            ]),
            counts(&[
                "1m", "1m", "2m", "2m", "3m", "3m", "4p", "4p", "5p", "5p", "6s", "6s", "E", "E",
            ]),
            counts(&[
                "1m", "9m", "1p", "9p", "1s", "9s", "E", "S", "W", "N", "P", "F", "C", "1m",
            ]),
            counts(&[
                "3m", "4m", "5m", "3p", "4p", "5p", "6p", "7p", "2s", "2s", "5s", "8s", "9m", "C",
            ]),
            counts(&["1m", "1m", "2m", "3m", "E"]),
        ]
    }

    fn acceptance_summary(evaluation: &DiscardEvaluation) -> Vec<(TileType, u8, i8)> {
        evaluation
            .acceptance_after_discard
            .tiles
            .iter()
            .map(|entry| (entry.tile, entry.remaining, entry.shanten_after_draw.min()))
            .collect()
    }

    #[test]
    fn fixed_meld_none_matches_concealed_evaluations() {
        for hand in menzen_hands() {
            let expected = evaluate_discards(&hand);
            let actual = evaluate_discards_with_fixed_melds(&hand, FixedMeldCount::NONE);
            assert_eq!(actual, expected);

            for (actual, expected) in actual.iter().zip(expected.iter()) {
                assert_eq!(actual.discard, expected.discard);
                assert_eq!(
                    actual.min_shanten_after_discard(),
                    expected.min_shanten_after_discard()
                );
                assert_eq!(acceptance_summary(actual), acceptance_summary(expected));
                assert_eq!(
                    actual.acceptance_total_remaining(),
                    expected.acceptance_total_remaining()
                );
                assert_eq!(actual.shape_penalty, expected.shape_penalty);
                assert_eq!(
                    actual.standard_iishanten_shape_after_discard,
                    expected.standard_iishanten_shape_after_discard
                );
                // 門前では七対子・国士の向聴数を従来どおり保持する。
                assert!(actual.shanten_after_discard.concealed().is_some());
            }

            assert_eq!(select_best(actual), select_best(expected));
        }
    }

    #[test]
    fn fixed_meld_none_evaluations_match_the_concealed_acceptance_api() {
        for hand in menzen_hands() {
            for evaluation in evaluate_discards_with_fixed_melds(&hand, FixedMeldCount::NONE) {
                let mut after_discard = hand;
                after_discard.remove(evaluation.discard).unwrap();
                let expected = calculate_acceptance(&after_discard);

                assert_eq!(
                    evaluation.shanten_after_discard.concealed(),
                    Some(expected.current)
                );
                assert_eq!(
                    evaluation.min_shanten_after_discard(),
                    expected.current.min()
                );
                assert_eq!(evaluation.acceptance_type_count(), expected.tiles.len());
                assert_eq!(
                    evaluation.acceptance_total_remaining(),
                    expected.total_remaining()
                );
            }
        }
    }

    #[test]
    fn fixed_meld_none_keeps_selection_and_comparison_reason() {
        for hand in menzen_hands() {
            let expected = evaluate_discards(&hand);
            let actual = evaluate_discards_with_fixed_melds(&hand, FixedMeldCount::NONE);

            let expected_diagnostic = diagnose_discard_evaluations(&hand, &expected);
            let actual_diagnostic =
                diagnose_discard_evaluations_with_fixed_melds(&hand, FixedMeldCount::NONE, &actual);

            assert_eq!(actual_diagnostic.selected, expected_diagnostic.selected);
            for (actual, expected) in actual_diagnostic
                .candidates
                .iter()
                .zip(expected_diagnostic.candidates.iter())
            {
                assert_eq!(actual.selected, expected.selected);
                assert_eq!(actual.comparison_reason, expected.comparison_reason);
                assert_eq!(actual.block_context, expected.block_context);
            }
        }
    }

    // 副露1組 + 123456m78p55s + ツモ N。N を切ると固定面子1・123m・456m・78p・55s の通常形テンパイ。
    fn one_meld_hand() -> TileCounts {
        counts(&[
            "1m", "2m", "3m", "4m", "5m", "6m", "7p", "8p", "5s", "5s", "N",
        ])
    }

    #[test]
    fn one_fixed_meld_discard_reaches_standard_tenpai() {
        let hand = one_meld_hand();
        let evaluations = evaluate_discards_with_fixed_melds(&hand, fixed(1));
        let north = discard_evaluation(&evaluations, tile("N"));

        assert_eq!(north.min_shanten_after_discard(), 0);
        assert_eq!(north.shanten_after_discard.standard(), 0);
        assert_eq!(
            acceptance_summary(north),
            vec![(tile("6p"), 4, -1), (tile("9p"), 4, -1)]
        );
        assert_eq!(north.acceptance_type_count(), 2);
        assert_eq!(north.acceptance_total_remaining(), 8);

        assert_eq!(select_best(evaluations).unwrap().discard, tile("N"));
    }

    #[test]
    fn one_fixed_meld_does_not_use_chiitoitsu_or_kokushi() {
        let hand = one_meld_hand();
        let evaluations = evaluate_discards_with_fixed_melds(&hand, fixed(1));

        for evaluation in &evaluations {
            assert_eq!(evaluation.shanten_after_discard.concealed(), None);
            assert_eq!(
                evaluation.min_shanten_after_discard(),
                evaluation.shanten_after_discard.standard()
            );
            assert!(
                evaluation
                    .acceptance_after_discard
                    .tiles
                    .iter()
                    .all(|entry| entry.shanten_after_draw.concealed().is_none())
            );
        }

        // 門前評価では同じ手牌が2向聴で、N 切りの受け入れも別物になる。
        let menzen = discard_evaluation(&evaluate_discards(&hand), tile("N")).clone();
        assert_eq!(menzen.min_shanten_after_discard(), 2);
        assert_ne!(
            menzen.acceptance_total_remaining(),
            discard_evaluation(&evaluations, tile("N")).acceptance_total_remaining()
        );
    }

    #[test]
    fn one_fixed_meld_keeps_iishanten_shape_unknown() {
        // 一向聴形分類は門前13枚専用。副露手を門前分類器へ押し込まない。
        let hand = counts(&[
            "1m", "2m", "3m", "4m", "5m", "6m", "E", "E", "2p", "3p", "5s",
        ]);
        for evaluation in evaluate_discards_with_fixed_melds(&hand, fixed(1)) {
            assert_eq!(
                evaluation.standard_iishanten_shape_after_discard,
                IishantenShape::Unknown
            );
        }
    }

    #[test]
    fn one_fixed_meld_visible_tiles_reduce_acceptance_remaining() {
        // 手牌 123456m78p55s + ツモ N に、他家に見えている 6p 2枚を加える。
        let tiles = ids_of(&[
            "1m", "2m", "3m", "4m", "5m", "6m", "7p", "8p", "5s", "5s", "N",
        ]);
        let hand = TileCounts::from_tiles(tiles.iter().copied());
        let mut visible = tiles.clone();
        visible.extend(ids(&[56, 57]));

        let evaluations =
            evaluate_discards_with_fixed_melds_and_visible_tiles(&hand, fixed(1), &visible);
        let north = discard_evaluation(&evaluations, tile("N"));

        assert_eq!(north.min_shanten_after_discard(), 0);
        assert_eq!(
            acceptance_summary(north),
            vec![(tile("6p"), 2, -1), (tile("9p"), 4, -1)]
        );
        assert_eq!(north.acceptance_total_remaining(), 6);
    }

    #[test]
    fn one_fixed_meld_counts_the_candidate_discard_as_seen() {
        // 手牌 123456m78p55s + ツモ 6p。6p を切った後の待ちは 6p / 9p で、今切る 6p 自身を
        // 山に残っている牌として数えない。
        let tiles = ids_of(&[
            "1m", "2m", "3m", "4m", "5m", "6m", "7p", "8p", "5s", "5s", "6p",
        ]);
        let hand = TileCounts::from_tiles(tiles.iter().copied());

        let with_visible =
            evaluate_discards_with_fixed_melds_and_visible_tiles(&hand, fixed(1), &tiles);
        let six_pin = discard_evaluation(&with_visible, tile("6p"));
        assert_eq!(six_pin.min_shanten_after_discard(), 0);
        assert_eq!(
            acceptance_summary(six_pin),
            vec![(tile("6p"), 3, -1), (tile("9p"), 4, -1)]
        );
        assert_eq!(six_pin.acceptance_total_remaining(), 7);

        // visible tiles を渡さない経路では候補打牌補正を行わない既存 semantics のまま。
        let without_visible = evaluate_discards_with_fixed_melds(&hand, fixed(1));
        assert_eq!(
            discard_evaluation(&without_visible, tile("6p")).acceptance_total_remaining(),
            8
        );
    }

    #[test]
    fn fixed_meld_visible_tiles_match_the_pure_acceptance_api() {
        let tiles = ids_of(&[
            "1m", "2m", "3m", "4m", "5m", "6m", "7p", "8p", "5s", "5s", "N",
        ]);
        let hand = TileCounts::from_tiles(tiles.iter().copied());
        let mut visible = tiles.clone();
        visible.extend(ids(&[56, 57]));

        let north = discard_evaluation(
            &evaluate_discards_with_fixed_melds_and_visible_tiles(&hand, fixed(1), &visible),
            tile("N"),
        )
        .clone();

        let mut after_discard = hand;
        after_discard.remove(tile("N")).unwrap();
        // 打牌後の手牌に N は無く visible には残るため、pure API 側でも候補打牌1枚が seen に入る。
        let expected = calculate_acceptance_with_fixed_melds_and_visible_tiles(
            &after_discard,
            fixed(1),
            &visible,
        );

        assert_eq!(north.acceptance_after_discard, expected);
    }

    #[test]
    fn two_fixed_melds_match_the_pure_shanten_and_acceptance_api() {
        // 副露2組 + 123456m5s + ツモ N。N を切ると固定面子2・123m・456m・5s 単騎のテンパイ。
        let hand = counts(&["1m", "2m", "3m", "4m", "5m", "6m", "5s", "N"]);
        let evaluations = evaluate_discards_with_fixed_melds(&hand, fixed(2));
        let north = discard_evaluation(&evaluations, tile("N"));

        let mut after_discard = hand;
        after_discard.remove(tile("N")).unwrap();

        assert_eq!(
            north.shanten_after_discard.standard(),
            standard_shanten_with_fixed_melds(&after_discard, fixed(2))
        );
        assert_eq!(
            north.acceptance_after_discard,
            calculate_acceptance_with_fixed_melds(&after_discard, fixed(2))
        );
        assert_eq!(north.min_shanten_after_discard(), 0);
        assert_eq!(acceptance_summary(north), vec![(tile("5s"), 3, -1)]);
        assert_eq!(north.acceptance_total_remaining(), 3);
        assert_eq!(select_best(evaluations).unwrap().discard, tile("N"));
    }

    #[test]
    fn fixed_melds_do_not_change_after_a_candidate_discard() {
        // 打牌しただけでは副露済み面子数は変わらないので、打牌後の受け入れも同じ面子数で求める。
        let hand = one_meld_hand();
        for evaluation in evaluate_discards_with_fixed_melds(&hand, fixed(1)) {
            let mut after_discard = hand;
            after_discard.remove(evaluation.discard).unwrap();
            assert_eq!(
                evaluation.acceptance_after_discard,
                calculate_acceptance_with_fixed_melds(&after_discard, fixed(1))
            );
        }
    }

    // ---- shape penalty の副露補正 ----

    #[test]
    fn shape_penalty_with_zero_fixed_melds_matches_existing_penalty() {
        for hand in menzen_hands() {
            for tile in TileType::all() {
                if hand.count(tile) == 0 {
                    continue;
                }
                assert_eq!(
                    shape_penalty_for_discard_with_fixed_melds(&hand, tile, FixedMeldCount::NONE),
                    shape_penalty_for_discard(&hand, tile)
                );
                assert_eq!(
                    shape_penalty_for_discard_with_fixed_melds_and_context(
                        &hand,
                        tile,
                        FixedMeldCount::NONE,
                        Some(TileType::from_mjai_type_str("E").unwrap()),
                        Some(TileType::from_mjai_type_str("S").unwrap()),
                    ),
                    shape_penalty_for_discard_with_context(
                        &hand,
                        tile,
                        Some(TileType::from_mjai_type_str("E").unwrap()),
                        Some(TileType::from_mjai_type_str("S").unwrap()),
                    )
                );
                assert_eq!(
                    discard_block_context_with_fixed_melds(&hand, tile, FixedMeldCount::NONE),
                    discard_block_context(&hand, tile)
                );
            }
        }
    }

    #[test]
    fn block_shortage_counts_fixed_melds_as_blocks() {
        // concealed の推定ブロック数は打牌後2で、副露済み面子を足すと5に届く。
        let hand = counts(&["1m", "1m", "9m", "9m", "1p", "2p"]);
        let discard = tile("2p");

        let menzen = discard_block_context(&hand, discard);
        assert_eq!(menzen.after.estimated_block_count, 2);
        assert!(menzen.reduces_estimated_block_count);
        assert!(menzen.leaves_under_five_blocks);

        let three_melds = discard_block_context_with_fixed_melds(&hand, discard, fixed(3));
        assert!(three_melds.reduces_estimated_block_count);
        assert!(!three_melds.leaves_under_five_blocks);
        // concealed hand の形そのものの意味は変えない。
        assert_eq!(three_melds.before, menzen.before);
        assert_eq!(three_melds.after, menzen.after);

        // 足りない場合はブロック不足のまま。
        assert!(
            discard_block_context_with_fixed_melds(&hand, discard, fixed(1))
                .leaves_under_five_blocks
        );
    }

    #[test]
    fn shape_penalty_block_shortage_is_relaxed_by_fixed_melds() {
        let hand = counts(&["1m", "1m", "9m", "9m", "1p", "2p"]);
        let discard = tile("2p");

        let menzen = shape_penalty_for_discard(&hand, discard);
        let three_melds = shape_penalty_for_discard_with_fixed_melds(&hand, discard, fixed(3));

        // ブロック不足 (+10) とブロック減少のみ (+4) の差。
        assert_eq!(menzen - three_melds, 6);
        assert_eq!(
            shape_penalty_for_discard_with_fixed_melds(&hand, discard, fixed(1)),
            menzen
        );
    }

    #[test]
    fn evaluations_use_the_fixed_meld_aware_shape_penalty() {
        let hand = counts(&["1m", "1m", "9m", "9m", "1p", "2p"]);
        let evaluations = evaluate_discards_with_fixed_melds(&hand, fixed(3));
        let two_pin = discard_evaluation(&evaluations, tile("2p"));

        assert_eq!(
            two_pin.shape_penalty,
            shape_penalty_for_discard_with_fixed_melds(&hand, tile("2p"), fixed(3))
        );
        assert_ne!(
            two_pin.shape_penalty,
            shape_penalty_for_discard(&hand, tile("2p"))
        );
    }

    #[test]
    fn diagnostic_block_context_matches_the_evaluation_fixed_melds() {
        let hand = counts(&["1m", "1m", "9m", "9m", "1p", "2p"]);
        let evaluations = evaluate_discards_with_fixed_melds(&hand, fixed(3));
        let diagnostic =
            diagnose_discard_evaluations_with_fixed_melds(&hand, fixed(3), &evaluations);

        let two_pin = diagnostic
            .candidates
            .iter()
            .find(|candidate| candidate.evaluation.discard == tile("2p"))
            .unwrap();
        assert!(!two_pin.block_context.leaves_under_five_blocks);
        assert_eq!(
            two_pin.block_context,
            discard_block_context_with_fixed_melds(&hand, tile("2p"), fixed(3))
        );
    }

    #[test]
    fn from_tiles_with_fixed_melds_matches_the_counts_api() {
        let tiles = ids_of(&[
            "1m", "2m", "3m", "4m", "5m", "6m", "7p", "8p", "5s", "5s", "N",
        ]);
        let hand = TileCounts::from_tiles(tiles.iter().copied());

        let from_tiles = evaluate_discards_from_tiles_with_fixed_melds_and_context(
            &tiles,
            fixed(1),
            &[],
            None,
            None,
        );
        let north = discard_evaluation(&from_tiles, tile("N"));
        assert_eq!(north.min_shanten_after_discard(), 0);
        assert_eq!(north.acceptance_total_remaining(), 8);
        assert_eq!(
            north.shanten_after_discard,
            discard_evaluation(
                &evaluate_discards_with_fixed_melds(&hand, fixed(1)),
                tile("N")
            )
            .shanten_after_discard
        );

        let mut visible = tiles.clone();
        visible.extend(ids(&[56, 57]));
        let from_tiles_with_visible =
            evaluate_discards_from_tiles_with_fixed_melds_and_visible_tiles(
                &tiles,
                fixed(1),
                &[],
                None,
                None,
                &visible,
            );
        assert_eq!(
            discard_evaluation(&from_tiles_with_visible, tile("N")).acceptance_total_remaining(),
            6
        );
    }

    #[test]
    fn from_tiles_with_zero_fixed_melds_matches_the_existing_api() {
        let tiles = ids_of(&[
            "3m", "4m", "5m", "3p", "4p", "5p", "6p", "7p", "2s", "2s", "5s", "8s", "9m", "C",
        ]);
        let mut visible = tiles.clone();
        visible.extend(ids_of(&["9m", "9m"]));

        assert_eq!(
            evaluate_discards_from_tiles_with_fixed_melds_and_context(
                &tiles,
                FixedMeldCount::NONE,
                &[],
                Some(tile("E")),
                Some(tile("S")),
            ),
            evaluate_discards_from_tiles_with_context(
                &tiles,
                &[],
                Some(tile("E")),
                Some(tile("S"))
            )
        );
        assert_eq!(
            evaluate_discards_from_tiles_with_fixed_melds_and_visible_tiles(
                &tiles,
                FixedMeldCount::NONE,
                &[],
                Some(tile("E")),
                Some(tile("S")),
                &visible,
            ),
            evaluate_discards_from_tiles_with_visible_tiles(
                &tiles,
                &[],
                Some(tile("E")),
                Some(tile("S")),
                &visible,
            )
        );
    }

    #[test]
    #[ignore]
    fn benchmark_evaluate_discards_sample_hand() {
        let counts = counts(&[
            "2m", "3m", "4m", "5m", "6m", "7m", "8m", "2p", "3p", "4p", "5p", "6p", "7p", "5s",
        ]);
        let start = std::time::Instant::now();
        let iterations = 100;
        for _ in 0..iterations {
            let _ = select_best_discard(&counts);
        }
        let elapsed = start.elapsed();
        println!(
            "select_best_discard: {:?} total, {:?} per call",
            elapsed,
            elapsed / iterations
        );
    }
}
