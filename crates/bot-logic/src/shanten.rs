use crate::tile::TileType;
use crate::tile_counts::{TileCountError, TileCounts};

pub fn standard_shanten(counts: &TileCounts) -> i8 {
    let mut best = i8::MAX;
    search(
        *counts,
        SearchState {
            melds: 0,
            has_pair: false,
            partials: 0,
        },
        &mut best,
    );
    best
}

pub fn chiitoitsu_shanten(counts: &TileCounts) -> i8 {
    let pairs = counts
        .iter()
        .filter(|(_, count)| *count >= 2)
        .count()
        .min(7) as i8;

    let unique = counts
        .iter()
        .filter(|(_, count)| *count >= 1)
        .count()
        .min(7) as i8;

    6 - pairs + (7 - unique)
}

pub fn kokushi_shanten(counts: &TileCounts) -> i8 {
    let unique_yaochu = counts
        .iter()
        .filter(|(tile, count)| tile.is_yaochu() && *count >= 1)
        .count() as i8;

    let has_yaochu_pair = counts
        .iter()
        .any(|(tile, count)| tile.is_yaochu() && count >= 2);

    13 - unique_yaochu - i8::from(has_yaochu_pair)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct SearchState {
    melds: u8,
    has_pair: bool,
    partials: u8,
}

impl SearchState {
    fn shanten(self) -> i8 {
        let melds = self.melds.min(4);
        let capped_partials = self.partials.min(4 - melds);
        let pair_bonus = i8::from(self.has_pair);
        8 - 2 * melds as i8 - capped_partials as i8 - pair_bonus
    }

    fn with_meld(self) -> Self {
        Self {
            melds: self.melds + 1,
            ..self
        }
    }

    fn with_pair_head(self) -> Self {
        Self {
            has_pair: true,
            ..self
        }
    }

    fn with_partial(self) -> Self {
        Self {
            partials: self.partials + 1,
            ..self
        }
    }
}

fn search(counts: TileCounts, state: SearchState, best: &mut i8) {
    *best = (*best).min(state.shanten());

    let Some(tile) = counts
        .iter()
        .find_map(|(tile, count)| (count >= 1).then_some(tile))
    else {
        return;
    };

    if state.melds < 4 {
        try_branch(
            counts,
            tile,
            TileCounts::remove_triplet,
            state.with_meld(),
            best,
        );
        try_branch(
            counts,
            tile,
            TileCounts::remove_sequence,
            state.with_meld(),
            best,
        );
    }

    if !state.has_pair {
        try_branch(
            counts,
            tile,
            TileCounts::remove_pair,
            state.with_pair_head(),
            best,
        );
    }

    if state.melds + state.partials < 4 {
        try_branch(
            counts,
            tile,
            TileCounts::remove_pair,
            state.with_partial(),
            best,
        );
        try_branch(
            counts,
            tile,
            TileCounts::remove_adjacent_wait,
            state.with_partial(),
            best,
        );
        try_branch(
            counts,
            tile,
            TileCounts::remove_skip_wait,
            state.with_partial(),
            best,
        );
    }

    let mut removed = counts;
    if removed.remove(tile).is_ok() {
        search(removed, state, best);
    }
}

fn try_branch(
    counts: TileCounts,
    tile: TileType,
    remove: fn(&mut TileCounts, TileType) -> Result<(), TileCountError>,
    next_state: SearchState,
    best: &mut i8,
) {
    let mut removed = counts;
    if remove(&mut removed, tile).is_ok() {
        search(removed, next_state, best);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn counts(strings: &[&str]) -> TileCounts {
        TileCounts::from_tile_types(
            strings
                .iter()
                .map(|s| TileType::from_mjai_type_str(s).unwrap()),
        )
    }

    #[test]
    fn complete_hand_returns_minus_one() {
        let counts = counts(&[
            "1m", "2m", "3m", "4m", "5m", "6m", "7m", "8m", "9m", "1p", "2p", "3p", "5s", "5s",
        ]);
        assert_eq!(standard_shanten(&counts), -1);
    }

    #[test]
    fn tenpai_hand_returns_zero() {
        let counts = counts(&[
            "1m", "2m", "3m", "4m", "5m", "6m", "7m", "8m", "9m", "1p", "2p", "3p", "5s",
        ]);
        assert_eq!(standard_shanten(&counts), 0);
    }

    #[test]
    fn one_shanten_hand_returns_one() {
        let counts = counts(&[
            "1m", "2m", "3m", "4m", "5m", "6m", "7m", "8m", "9m", "1p", "5s", "E", "E",
        ]);
        assert_eq!(standard_shanten(&counts), 1);
    }

    #[test]
    fn pair_counts_as_partial_when_head_is_taken() {
        let counts = counts(&[
            "1m", "2m", "3m", "4m", "5m", "6m", "7m", "8m", "9m", "1p", "1p", "2s", "2s", "E",
        ]);
        assert_eq!(standard_shanten(&counts), 0);
    }

    #[test]
    fn complete_hand_with_triplet_and_sequences_returns_minus_one() {
        let counts = counts(&[
            "1m", "1m", "1m", "2m", "3m", "4m", "3p", "4p", "5p", "7s", "8s", "9s", "E", "E",
        ]);
        assert_eq!(standard_shanten(&counts), -1);
    }

    #[test]
    fn empty_hand_returns_eight() {
        assert_eq!(standard_shanten(&TileCounts::new()), 8);
    }

    #[test]
    fn isolated_tiles_only_returns_six() {
        let counts = counts(&["1m", "3m", "5m", "7m", "9m", "E", "S", "W"]);
        assert_eq!(standard_shanten(&counts), 6);
    }

    #[test]
    fn partial_does_not_cross_suits() {
        assert_eq!(standard_shanten(&counts(&["9m", "1p"])), 8);
        assert_eq!(standard_shanten(&counts(&["8m", "9m", "1p"])), 7);
    }

    #[test]
    fn sequence_does_not_cross_suits() {
        let counts = counts(&["8m", "9m", "1p", "E", "E"]);
        assert_eq!(standard_shanten(&counts), 6);
    }

    #[test]
    fn chiitoitsu_complete_hand_returns_minus_one() {
        let counts = counts(&[
            "1m", "1m", "2m", "2m", "3m", "3m", "4p", "4p", "5p", "5p", "6s", "6s", "E", "E",
        ]);
        assert_eq!(chiitoitsu_shanten(&counts), -1);
    }

    #[test]
    fn chiitoitsu_tenpai_hand_returns_zero() {
        let counts = counts(&[
            "1m", "1m", "2m", "2m", "3m", "3m", "4p", "4p", "5p", "5p", "6s", "6s", "E",
        ]);
        assert_eq!(chiitoitsu_shanten(&counts), 0);
    }

    #[test]
    fn chiitoitsu_one_shanten_hand_returns_one() {
        let counts = counts(&[
            "1m", "1m", "2m", "2m", "3m", "3m", "4p", "4p", "5p", "5p", "6s", "E",
        ]);
        assert_eq!(chiitoitsu_shanten(&counts), 1);
    }

    #[test]
    fn chiitoitsu_quad_counts_as_one_pair() {
        let counts = counts(&[
            "1m", "1m", "1m", "1m", "2m", "2m", "3m", "3m", "4p", "4p", "5p", "5p", "6s", "6s", "E",
        ]);
        assert_eq!(chiitoitsu_shanten(&counts), 0);
    }

    #[test]
    fn chiitoitsu_empty_hand_returns_thirteen() {
        assert_eq!(chiitoitsu_shanten(&TileCounts::new()), 13);
    }

    #[test]
    fn kokushi_complete_hand_returns_minus_one() {
        let counts = counts(&[
            "1m", "9m", "1p", "9p", "1s", "9s", "E", "S", "W", "N", "P", "F", "C", "1m",
        ]);
        assert_eq!(kokushi_shanten(&counts), -1);
    }

    #[test]
    fn kokushi_thirteen_wait_tenpai_returns_zero() {
        let counts = counts(&[
            "1m", "9m", "1p", "9p", "1s", "9s", "E", "S", "W", "N", "P", "F", "C",
        ]);
        assert_eq!(kokushi_shanten(&counts), 0);
    }

    #[test]
    fn kokushi_single_wait_tenpai_returns_zero() {
        let counts = counts(&[
            "1m", "9m", "1p", "9p", "1s", "9s", "E", "S", "W", "N", "P", "F", "1m",
        ]);
        assert_eq!(kokushi_shanten(&counts), 0);
    }

    #[test]
    fn kokushi_one_shanten_hand_returns_one() {
        let counts = counts(&[
            "1m", "9m", "1p", "9p", "1s", "9s", "E", "S", "W", "N", "P", "1m",
        ]);
        assert_eq!(kokushi_shanten(&counts), 1);
    }

    #[test]
    fn kokushi_middle_tile_does_not_count_as_yaochu() {
        let counts = counts(&[
            "1m", "9m", "1p", "9p", "1s", "9s", "E", "S", "W", "N", "P", "F", "5m",
        ]);
        assert_eq!(kokushi_shanten(&counts), 1);
    }

    #[test]
    fn kokushi_empty_hand_returns_thirteen() {
        assert_eq!(kokushi_shanten(&TileCounts::new()), 13);
    }
}
