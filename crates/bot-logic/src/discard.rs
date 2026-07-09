use crate::acceptance::{Acceptance, calculate_acceptance, calculate_acceptance_with_seen};
use crate::shanten::Shanten;
use crate::tile::{TileId, TileType, count_dora};
use crate::tile_counts::TileCounts;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscardEvaluation {
    pub discard: TileType,
    pub count_before_discard: u8,
    pub shanten_after_discard: Shanten,
    pub acceptance_after_discard: Acceptance,
    pub shape_penalty: i16,
    pub floating_tile_value: i16,
    pub discarded_dora_count: u8,
    pub discarded_value_honor_count: u8,
    pub discards_red_five: bool,
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

pub fn shape_penalty_for_discard(counts: &TileCounts, discard: TileType) -> i16 {
    let breakdown = shape_breakdown_for_discard(counts, discard);
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

    if breakdown.preserves_sequence_after_discard {
        penalty -= 15;
    }
    if breakdown.preserves_ryanmen_after_discard {
        penalty -= 15;
    }
    let preserves_shape =
        breakdown.preserves_sequence_after_discard || breakdown.preserves_ryanmen_after_discard;
    if breakdown.preserves_pair_after_discard {
        penalty -= 12;
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
    select_best_discard_from_tiles_with_context(tiles, dora_indicators, None, None)
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

fn select_best(evaluations: Vec<DiscardEvaluation>) -> Option<DiscardEvaluation> {
    evaluations.into_iter().reduce(|best, candidate| {
        if is_better_discard(&candidate, &best) {
            candidate
        } else {
            best
        }
    })
}

fn is_better_discard(candidate: &DiscardEvaluation, best: &DiscardEvaluation) -> bool {
    let candidate_shanten = candidate.min_shanten_after_discard();
    let best_shanten = best.min_shanten_after_discard();
    if candidate_shanten != best_shanten {
        return candidate_shanten < best_shanten;
    }

    let candidate_remaining = candidate.acceptance_total_remaining();
    let best_remaining = best.acceptance_total_remaining();
    if candidate_remaining != best_remaining {
        return candidate_remaining > best_remaining;
    }

    let candidate_type_count = candidate.acceptance_type_count();
    let best_type_count = best.acceptance_type_count();
    if candidate_type_count != best_type_count {
        return candidate_type_count > best_type_count;
    }

    if candidate.shape_penalty != best.shape_penalty {
        return candidate.shape_penalty < best.shape_penalty;
    }

    if candidate.floating_tile_value != best.floating_tile_value {
        return candidate.floating_tile_value < best.floating_tile_value;
    }

    if candidate.discarded_dora_count != best.discarded_dora_count {
        return candidate.discarded_dora_count < best.discarded_dora_count;
    }

    if candidate.discarded_value_honor_count != best.discarded_value_honor_count {
        return candidate.discarded_value_honor_count < best.discarded_value_honor_count;
    }

    !candidate.discards_red_five && best.discards_red_five
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

pub fn evaluate_discards(counts: &TileCounts) -> Vec<DiscardEvaluation> {
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

        let acceptance_after_discard = calculate_acceptance(&after_discard);
        let shanten_after_discard = acceptance_after_discard.current;

        evaluations.push(DiscardEvaluation {
            discard: tile,
            count_before_discard,
            shanten_after_discard,
            acceptance_after_discard,
            shape_penalty: shape_penalty_for_discard(counts, tile),
            floating_tile_value: floating_tile_value_for_discard(counts, tile),
            discarded_dora_count: 0,
            discarded_value_honor_count: 0,
            discards_red_five: false,
        });
    }

    evaluations
}

pub fn evaluate_discards_with_visible_tiles(
    counts: &TileCounts,
    visible_tiles: &[TileId],
) -> Vec<DiscardEvaluation> {
    if visible_tiles.is_empty() {
        return evaluate_discards(counts);
    }

    let visible_counts = TileCounts::from_tiles(visible_tiles.iter().copied());
    let mut public_visible = [0u8; TileType::COUNT];
    for tile in TileType::all() {
        public_visible[tile.index()] = visible_counts
            .count(tile)
            .saturating_sub(counts.count(tile));
    }

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

        let mut additional_seen = public_visible;
        additional_seen[tile.index()] = additional_seen[tile.index()].saturating_add(1);

        let acceptance_after_discard =
            calculate_acceptance_with_seen(&after_discard, &additional_seen);
        let shanten_after_discard = acceptance_after_discard.current;

        evaluations.push(DiscardEvaluation {
            discard: tile,
            count_before_discard,
            shanten_after_discard,
            acceptance_after_discard,
            shape_penalty: shape_penalty_for_discard(counts, tile),
            floating_tile_value: floating_tile_value_for_discard(counts, tile),
            discarded_dora_count: 0,
            discarded_value_honor_count: 0,
            discards_red_five: false,
        });
    }

    evaluations
}

pub fn evaluate_discards_from_tiles(tiles: &[TileId]) -> Vec<DiscardEvaluation> {
    evaluate_discards_from_tiles_with_dora(tiles, &[])
}

pub fn evaluate_discards_from_tiles_with_dora(
    tiles: &[TileId],
    dora_indicators: &[TileId],
) -> Vec<DiscardEvaluation> {
    evaluate_discards_from_tiles_with_context(tiles, dora_indicators, None, None)
}

pub fn evaluate_discards_from_tiles_with_context(
    tiles: &[TileId],
    dora_indicators: &[TileId],
    round_wind: Option<TileType>,
    seat_wind: Option<TileType>,
) -> Vec<DiscardEvaluation> {
    let counts = TileCounts::from_tiles(tiles.iter().copied());
    let mut evaluations = evaluate_discards(&counts);
    decorate_evaluations(
        &mut evaluations,
        tiles,
        dora_indicators,
        round_wind,
        seat_wind,
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
    let counts = TileCounts::from_tiles(tiles.iter().copied());
    let mut evaluations = evaluate_discards_with_visible_tiles(&counts, visible_tiles);
    decorate_evaluations(
        &mut evaluations,
        tiles,
        dora_indicators,
        round_wind,
        seat_wind,
    );
    evaluations
}

fn decorate_evaluations(
    evaluations: &mut [DiscardEvaluation],
    tiles: &[TileId],
    dora_indicators: &[TileId],
    round_wind: Option<TileType>,
    seat_wind: Option<TileType>,
) {
    for evaluation in evaluations {
        let discarded_tile = discarded_tile_id_for_type(evaluation.discard, tiles);
        evaluation.discards_red_five = discarded_tile.map(TileId::is_red).unwrap_or(false);
        evaluation.discarded_dora_count = discarded_tile
            .map(|tile| count_dora(tile, dora_indicators))
            .unwrap_or(0);
        evaluation.discarded_value_honor_count =
            value_honor_count(evaluation.discard, round_wind, seat_wind);
    }
}

fn discarded_tile_id_for_type(discard: TileType, tiles: &[TileId]) -> Option<TileId> {
    let mut red = None;
    for &tile in tiles {
        if tile.tile_type() != discard {
            continue;
        }
        if tile.is_red() {
            red.get_or_insert(tile);
        } else {
            return Some(tile);
        }
    }
    red
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tile(s: &str) -> TileType {
        TileType::from_mjai_type_str(s).unwrap()
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
        let counts = counts(&["1m", "5m", "9m", "1p", "5p", "9p", "1s", "5s", "9s", "E"]);
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

    use crate::acceptance::AcceptanceTile;
    use crate::tile::TileId;

    fn ids(values: &[u8]) -> Vec<TileId> {
        values.iter().map(|&v| TileId::new(v).unwrap()).collect()
    }

    fn shanten_min(min: i8) -> Shanten {
        Shanten {
            standard: min,
            chiitoitsu: 127,
            kokushi: 127,
        }
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
        let tiles: Vec<AcceptanceTile> = (0..type_count)
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
        }
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
        assert_eq!(
            shape_penalty_for_discard(&counts(&["5m", "5m"]), tile("5m")),
            28
        );
    }

    #[test]
    fn same_type_two_relief_applies_in_complex_shape() {
        // 5m5m6m の 5m は 1枚切っても 5m6m 両面が残る余剰対子
        // 主要形が残るため唯一対子 penalty は加えない
        // 対子20 + 両面30 + 隣接3 - 両面存続15 - 同種2枚8 = 30
        let redundant = shape_penalty_for_discard(&counts(&["5m", "5m", "6m"]), tile("5m"));
        assert_eq!(redundant, 30);
        let plain = shape_penalty_for_discard(&counts(&["5m", "6m"]), tile("5m"));
        assert!(redundant < plain);
    }

    #[test]
    fn same_type_three_relief_lowers_triplet_penalty() {
        // 暗刻は 1枚切っても対子が残る余剰形
        // 対子20 + 同種3枚 +10 - 対子存続12 = 18
        assert_eq!(
            shape_penalty_for_discard(&counts(&["5m", "5m", "5m"]), tile("5m")),
            18
        );
        // 暗刻は対子より切りやすい（対子は落とすと何も残らない）
        assert!(
            shape_penalty_for_discard(&counts(&["5m", "5m", "5m"]), tile("5m"))
                < shape_penalty_for_discard(&counts(&["5m", "5m"]), tile("5m"))
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
        let penalty = shape_penalty_for_discard(&counts(&["E", "E"]), tile("E"));
        assert!(penalty > 0);
        assert_eq!(penalty, 28);
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
        let triplet = shape_penalty_for_discard(&counts(&["5m", "5m", "5m"]), tile("5m"));
        let only_pair = shape_penalty_for_discard(&counts(&["5m", "5m"]), tile("5m"));
        assert_eq!(triplet, 18);
        assert!(triplet < only_pair);
    }

    #[test]
    fn only_pair_penalty_skipped_when_major_shape_survives() {
        // 2m2m3m の 2m は唯一の対子候補だが、切っても両面が残るため唯一対子 penalty は加えない
        // 対子20 + 両面30 + 隣接3 - 両面存続15 - 同種2枚8 = 30
        assert_eq!(
            shape_penalty_for_discard(&counts(&["2m", "2m", "3m"]), tile("2m")),
            30
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
