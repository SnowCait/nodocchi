use crate::acceptance::{Acceptance, calculate_acceptance};
use crate::shanten::Shanten;
use crate::tile::TileType;
use crate::tile_counts::TileCounts;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscardEvaluation {
    pub discard: TileType,
    pub count_before_discard: u8,
    pub shanten_after_discard: Shanten,
    pub acceptance_after_discard: Acceptance,
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
    evaluate_discards(counts)
        .into_iter()
        .reduce(|best, candidate| {
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

    candidate.acceptance_type_count() > best.acceptance_type_count()
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
        });
    }

    evaluations
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
