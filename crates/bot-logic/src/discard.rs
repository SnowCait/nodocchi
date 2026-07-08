use crate::acceptance::{Acceptance, calculate_acceptance};
use crate::shanten::Shanten;
use crate::tile::{TileId, TileType, count_dora};
use crate::tile_counts::TileCounts;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscardEvaluation {
    pub discard: TileType,
    pub count_before_discard: u8,
    pub shanten_after_discard: Shanten,
    pub acceptance_after_discard: Acceptance,
    pub discarded_dora_count: u8,
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

pub fn select_best_discard(counts: &TileCounts) -> Option<DiscardEvaluation> {
    select_best(evaluate_discards(counts))
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

    if candidate.discarded_dora_count != best.discarded_dora_count {
        return candidate.discarded_dora_count < best.discarded_dora_count;
    }

    !candidate.discards_red_five && best.discards_red_five
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
            discarded_dora_count: 0,
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
    let counts = TileCounts::from_tiles(tiles.iter().copied());
    let mut evaluations = evaluate_discards(&counts);
    for evaluation in &mut evaluations {
        let discarded_tile = discarded_tile_id_for_type(evaluation.discard, tiles);
        evaluation.discards_red_five = discarded_tile.map(TileId::is_red).unwrap_or(false);
        evaluation.discarded_dora_count = discarded_tile
            .map(|tile| count_dora(tile, dora_indicators))
            .unwrap_or(0);
    }
    evaluations
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
            discarded_dora_count: dora,
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
