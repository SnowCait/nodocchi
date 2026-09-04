use crate::tile::{TileId, TileType};
use thiserror::Error;

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum TileCountError {
    #[error("no tile of type {0:?} left to remove")]
    Underflow(TileType),

    #[error("too many tiles of type {0:?}")]
    Overflow(TileType),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TileCounts {
    counts: [u8; 34],
}

impl TileCounts {
    pub fn new() -> Self {
        Self { counts: [0; 34] }
    }

    pub fn from_tiles(tiles: impl IntoIterator<Item = TileId>) -> Self {
        let mut counts = Self::new();
        for tile in tiles {
            counts.increment(tile);
        }
        counts
    }

    pub fn from_tile_types<I: IntoIterator<Item = TileType>>(tiles: I) -> Self {
        let mut counts = Self::new();
        for tile in tiles {
            counts.add(tile);
        }
        counts
    }

    pub fn increment(&mut self, tile: TileId) {
        self.counts[tile.tile_type().index()] += 1;
    }

    pub fn try_add(&mut self, tile: TileType) -> Result<(), TileCountError> {
        let slot = &mut self.counts[tile.index()];
        if *slot >= 4 {
            return Err(TileCountError::Overflow(tile));
        }
        *slot += 1;
        Ok(())
    }

    pub fn add(&mut self, tile: TileType) {
        self.try_add(tile)
            .expect("TileCounts cannot contain more than four copies of one tile type");
    }

    pub fn remove(&mut self, tile: TileType) -> Result<(), TileCountError> {
        let slot = &mut self.counts[tile.index()];
        if *slot == 0 {
            return Err(TileCountError::Underflow(tile));
        }
        *slot -= 1;
        Ok(())
    }

    pub fn count(&self, tile_type: TileType) -> u8 {
        self.counts[tile_type.index()]
    }

    pub fn can_remove_pair(&self, tile: TileType) -> bool {
        self.count(tile) >= 2
    }

    pub fn can_remove_triplet(&self, tile: TileType) -> bool {
        self.count(tile) >= 3
    }

    pub fn can_remove_sequence(&self, start: TileType) -> bool {
        start
            .sequence()
            .is_some_and(|tiles| tiles.iter().all(|&tile| self.count(tile) >= 1))
    }

    pub fn remove_pair(&mut self, tile: TileType) -> Result<(), TileCountError> {
        if !self.can_remove_pair(tile) {
            return Err(TileCountError::Underflow(tile));
        }
        self.counts[tile.index()] -= 2;
        Ok(())
    }

    pub fn remove_triplet(&mut self, tile: TileType) -> Result<(), TileCountError> {
        if !self.can_remove_triplet(tile) {
            return Err(TileCountError::Underflow(tile));
        }
        self.counts[tile.index()] -= 3;
        Ok(())
    }

    pub fn remove_sequence(&mut self, start: TileType) -> Result<(), TileCountError> {
        let tiles = start.sequence().ok_or(TileCountError::Underflow(start))?;
        for tile in tiles {
            if self.count(tile) == 0 {
                return Err(TileCountError::Underflow(tile));
            }
        }
        for tile in tiles {
            self.counts[tile.index()] -= 1;
        }
        Ok(())
    }

    pub fn can_remove_adjacent_wait(&self, start: TileType) -> bool {
        start
            .next_in_suit()
            .is_some_and(|next| self.count(start) >= 1 && self.count(next) >= 1)
    }

    pub fn can_remove_skip_wait(&self, start: TileType) -> bool {
        start
            .second_next_in_suit()
            .is_some_and(|second| self.count(start) >= 1 && self.count(second) >= 1)
    }

    pub fn remove_adjacent_wait(&mut self, start: TileType) -> Result<(), TileCountError> {
        let next = start
            .next_in_suit()
            .ok_or(TileCountError::Underflow(start))?;
        self.remove_one_each([start, next])
    }

    pub fn remove_skip_wait(&mut self, start: TileType) -> Result<(), TileCountError> {
        let second = start
            .second_next_in_suit()
            .ok_or(TileCountError::Underflow(start))?;
        self.remove_one_each([start, second])
    }

    fn remove_one_each(&mut self, tiles: [TileType; 2]) -> Result<(), TileCountError> {
        for tile in tiles {
            if self.count(tile) == 0 {
                return Err(TileCountError::Underflow(tile));
            }
        }
        for tile in tiles {
            self.counts[tile.index()] -= 1;
        }
        Ok(())
    }

    pub fn total(&self) -> u8 {
        self.counts.iter().sum()
    }

    pub fn is_empty(&self) -> bool {
        self.counts.iter().all(|&count| count == 0)
    }

    pub fn remaining_count(&self, tile: TileType) -> u8 {
        4u8.saturating_sub(self.count(tile))
    }

    pub fn as_array(&self) -> &[u8; 34] {
        &self.counts
    }

    pub fn iter(&self) -> impl Iterator<Item = (TileType, u8)> {
        TileType::all().map(|tile| (tile, self.count(tile)))
    }
}

impl Default for TileCounts {
    fn default() -> Self {
        Self::new()
    }
}

impl TryFrom<[u8; 34]> for TileCounts {
    type Error = TileCountError;

    fn try_from(counts: [u8; 34]) -> Result<Self, Self::Error> {
        for tile in TileType::all() {
            if counts[tile.index()] > 4 {
                return Err(TileCountError::Overflow(tile));
            }
        }
        Ok(Self { counts })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tt(value: u8) -> TileType {
        TileType::new(value).unwrap()
    }

    fn id(value: u8) -> TileId {
        TileId::new(value).unwrap()
    }

    #[test]
    fn new_has_all_zero_counts() {
        let counts = TileCounts::new();
        assert_eq!(counts.total(), 0);
        assert!(counts.as_array().iter().all(|&count| count == 0));
    }

    #[test]
    fn default_equals_new() {
        assert_eq!(TileCounts::default(), TileCounts::new());
    }

    #[test]
    fn from_tiles_empty_is_empty() {
        let counts = TileCounts::from_tiles(Vec::new());
        assert!(counts.is_empty());
        assert_eq!(counts.total(), 0);
    }

    #[test]
    fn from_tiles_counts_tile_types() {
        let counts = TileCounts::from_tiles(vec![id(0), id(1), id(104)]);
        assert_eq!(counts.count(tt(0)), 2);
        assert_eq!(counts.count(tt(26)), 1);
        assert_eq!(counts.count(tt(1)), 0);
        assert_eq!(counts.total(), 3);
    }

    #[test]
    fn red_and_black_five_count_as_same_type() {
        let counts = TileCounts::from_tiles(vec![id(16), id(17)]);
        assert_eq!(counts.count(tt(4)), 2);
        assert_eq!(counts.total(), 2);
    }

    #[test]
    fn from_tiles_counts_honors() {
        let counts = TileCounts::from_tiles(vec![id(108), id(132), id(133)]);
        assert_eq!(counts.count(tt(27)), 1);
        assert_eq!(counts.count(tt(33)), 2);
        assert_eq!(counts.total(), 3);
    }

    #[test]
    fn increment_increases_count() {
        let mut counts = TileCounts::new();
        counts.increment(id(20));
        assert_eq!(counts.count(tt(5)), 1);
        counts.increment(id(21));
        assert_eq!(counts.count(tt(5)), 2);
    }

    #[test]
    fn increment_does_not_check_overflow() {
        let mut counts = TileCounts::new();
        for _ in 0..5 {
            counts.increment(id(0));
        }
        assert_eq!(counts.count(tt(0)), 5);
    }

    #[test]
    fn is_empty_reflects_contents() {
        let mut counts = TileCounts::new();
        assert!(counts.is_empty());
        counts.increment(id(0));
        assert!(!counts.is_empty());
    }

    #[test]
    fn from_tile_types_and_total() {
        let counts = TileCounts::from_tile_types(vec![tt(0), tt(0), tt(4), tt(33)]);
        assert_eq!(counts.total(), 4);
        assert_eq!(counts.count(tt(0)), 2);
        assert_eq!(counts.count(tt(4)), 1);
        assert_eq!(counts.count(tt(33)), 1);
        assert_eq!(counts.count(tt(1)), 0);
    }

    #[test]
    fn try_from_counts_preserves_valid_counts() {
        let raw = std::array::from_fn(|index| (index % 5) as u8);
        let counts = TileCounts::try_from(raw).expect("zero through four copies are valid");

        for tile in TileType::all() {
            assert_eq!(counts.count(tile), raw[tile.index()]);
        }
        assert_eq!(counts.total(), raw.into_iter().sum());

        let from_types = TileCounts::from_tile_types(
            TileType::all()
                .flat_map(|tile| std::iter::repeat_n(tile, usize::from(raw[tile.index()]))),
        );
        assert_eq!(counts, from_types);
    }

    #[test]
    fn try_from_counts_rejects_a_fifth_copy() {
        let mut raw = [0; 34];
        raw[tt(7).index()] = 5;

        assert_eq!(
            TileCounts::try_from(raw),
            Err(TileCountError::Overflow(tt(7)))
        );
    }

    #[test]
    #[should_panic(expected = "more than four copies")]
    fn from_tile_types_panics_on_fifth_copy() {
        TileCounts::from_tile_types(vec![tt(0); 5]);
    }

    #[test]
    fn add_and_remove() {
        let mut counts = TileCounts::new();
        counts.add(tt(5));
        assert_eq!(counts.count(tt(5)), 1);
        counts.remove(tt(5)).unwrap();
        assert_eq!(counts.count(tt(5)), 0);
        assert_eq!(counts.remove(tt(5)), Err(TileCountError::Underflow(tt(5))));
    }

    #[test]
    fn try_add_allows_up_to_four_copies() {
        let mut counts = TileCounts::new();
        for _ in 0..4 {
            counts.try_add(tt(0)).unwrap();
        }
        assert_eq!(counts.count(tt(0)), 4);
        assert_eq!(counts.remaining_count(tt(0)), 0);
        assert_eq!(counts.try_add(tt(0)), Err(TileCountError::Overflow(tt(0))));
        assert_eq!(counts.count(tt(0)), 4);
    }

    #[test]
    fn try_add_succeeds_again_after_remove() {
        let mut counts = TileCounts::from_tile_types(vec![tt(7); 4]);
        assert_eq!(counts.try_add(tt(7)), Err(TileCountError::Overflow(tt(7))));
        counts.remove(tt(7)).unwrap();
        counts.try_add(tt(7)).unwrap();
        assert_eq!(counts.count(tt(7)), 4);
    }

    #[test]
    fn remaining_count_subtracts_from_four() {
        let mut counts = TileCounts::new();
        assert_eq!(counts.remaining_count(tt(0)), 4);
        counts.add(tt(0));
        counts.add(tt(0));
        assert_eq!(counts.remaining_count(tt(0)), 2);
        counts.add(tt(0));
        counts.add(tt(0));
        assert_eq!(counts.remaining_count(tt(0)), 0);
    }

    #[test]
    fn as_array_exposes_counts() {
        let counts = TileCounts::from_tile_types(vec![tt(3), tt(3)]);
        let array = counts.as_array();
        assert_eq!(array.len(), 34);
        assert_eq!(array[3], 2);
        assert_eq!(array[0], 0);
    }

    #[test]
    fn iter_yields_all_34_entries() {
        let counts = TileCounts::from_tile_types(vec![tt(3), tt(3)]);
        let entries: Vec<_> = counts.iter().collect();
        assert_eq!(entries.len(), 34);
        assert_eq!(entries[3], (tt(3), 2));
        assert_eq!(entries[0], (tt(0), 0));
        assert_eq!(entries[33], (tt(33), 0));
    }

    #[test]
    fn can_remove_pair_requires_two_copies() {
        assert!(TileCounts::from_tile_types(vec![tt(0), tt(0)]).can_remove_pair(tt(0)));
        assert!(!TileCounts::from_tile_types(vec![tt(0)]).can_remove_pair(tt(0)));
        assert!(!TileCounts::new().can_remove_pair(tt(0)));
    }

    #[test]
    fn can_remove_triplet_requires_three_copies() {
        assert!(TileCounts::from_tile_types(vec![tt(0); 3]).can_remove_triplet(tt(0)));
        assert!(!TileCounts::from_tile_types(vec![tt(0), tt(0)]).can_remove_triplet(tt(0)));
        assert!(!TileCounts::new().can_remove_triplet(tt(0)));
    }

    #[test]
    fn can_remove_sequence_requires_all_three_tiles() {
        let counts = TileCounts::from_tile_types(vec![tt(0), tt(1), tt(2)]);
        assert!(counts.can_remove_sequence(tt(0)));

        let missing_third = TileCounts::from_tile_types(vec![tt(0), tt(1)]);
        assert!(!missing_third.can_remove_sequence(tt(0)));
    }

    #[test]
    fn can_remove_sequence_stays_within_suit() {
        let counts = TileCounts::from_tile_types(vec![tt(24), tt(25), tt(26)]);
        assert!(counts.can_remove_sequence(tt(24)));
        assert!(!counts.can_remove_sequence(tt(25)));
        assert!(!counts.can_remove_sequence(tt(26)));
    }

    #[test]
    fn can_remove_sequence_rejects_honors() {
        let counts = TileCounts::from_tile_types(vec![tt(27); 3]);
        assert!(!counts.can_remove_sequence(tt(27)));
    }

    #[test]
    fn remove_pair_removes_two_copies() {
        let mut counts = TileCounts::from_tile_types(vec![tt(0), tt(0)]);
        counts.remove_pair(tt(0)).unwrap();
        assert_eq!(counts.count(tt(0)), 0);
        assert_eq!(counts.total(), 0);
    }

    #[test]
    fn remove_pair_fails_without_changing_counts() {
        let mut counts = TileCounts::from_tile_types(vec![tt(0)]);
        assert_eq!(
            counts.remove_pair(tt(0)),
            Err(TileCountError::Underflow(tt(0)))
        );
        assert_eq!(counts.count(tt(0)), 1);
        assert_eq!(counts.total(), 1);
    }

    #[test]
    fn remove_triplet_removes_three_copies() {
        let mut counts = TileCounts::from_tile_types(vec![tt(0); 3]);
        counts.remove_triplet(tt(0)).unwrap();
        assert_eq!(counts.count(tt(0)), 0);
        assert_eq!(counts.total(), 0);
    }

    #[test]
    fn remove_triplet_fails_without_changing_counts() {
        let mut counts = TileCounts::from_tile_types(vec![tt(0), tt(0)]);
        assert_eq!(
            counts.remove_triplet(tt(0)),
            Err(TileCountError::Underflow(tt(0)))
        );
        assert_eq!(counts.count(tt(0)), 2);
        assert_eq!(counts.total(), 2);
    }

    #[test]
    fn remove_sequence_removes_one_of_each() {
        let mut counts = TileCounts::from_tile_types(vec![tt(0), tt(1), tt(2)]);
        counts.remove_sequence(tt(0)).unwrap();
        assert_eq!(counts.count(tt(0)), 0);
        assert_eq!(counts.count(tt(1)), 0);
        assert_eq!(counts.count(tt(2)), 0);
        assert_eq!(counts.total(), 0);
    }

    #[test]
    fn remove_sequence_fails_on_missing_tile_without_changing_counts() {
        let mut counts = TileCounts::from_tile_types(vec![tt(0), tt(1)]);
        assert_eq!(
            counts.remove_sequence(tt(0)),
            Err(TileCountError::Underflow(tt(2)))
        );
        assert_eq!(counts.count(tt(0)), 1);
        assert_eq!(counts.count(tt(1)), 1);
        assert_eq!(counts.total(), 2);
    }

    #[test]
    fn remove_sequence_stays_within_suit() {
        let mut counts = TileCounts::from_tile_types(vec![tt(24), tt(25), tt(26)]);
        counts.remove_sequence(tt(24)).unwrap();
        assert_eq!(counts.count(tt(24)), 0);
        assert_eq!(counts.count(tt(25)), 0);
        assert_eq!(counts.count(tt(26)), 0);

        let mut counts = TileCounts::from_tile_types(vec![tt(24), tt(25), tt(26)]);
        assert_eq!(
            counts.remove_sequence(tt(25)),
            Err(TileCountError::Underflow(tt(25)))
        );
        assert_eq!(
            counts.remove_sequence(tt(26)),
            Err(TileCountError::Underflow(tt(26)))
        );
        assert_eq!(counts.total(), 3);
    }

    #[test]
    fn remove_sequence_rejects_honors_without_changing_counts() {
        let mut counts = TileCounts::from_tile_types(vec![tt(27); 3]);
        assert_eq!(
            counts.remove_sequence(tt(27)),
            Err(TileCountError::Underflow(tt(27)))
        );
        assert_eq!(counts.count(tt(27)), 3);
        assert_eq!(counts.total(), 3);
    }

    #[test]
    fn can_remove_adjacent_wait_requires_both_tiles() {
        assert!(TileCounts::from_tile_types(vec![tt(0), tt(1)]).can_remove_adjacent_wait(tt(0)));
        assert!(!TileCounts::from_tile_types(vec![tt(0)]).can_remove_adjacent_wait(tt(0)));
        assert!(!TileCounts::from_tile_types(vec![tt(1)]).can_remove_adjacent_wait(tt(0)));
    }

    #[test]
    fn can_remove_adjacent_wait_allows_suit_edge() {
        let counts = TileCounts::from_tile_types(vec![tt(7), tt(8)]);
        assert!(counts.can_remove_adjacent_wait(tt(7)));
    }

    #[test]
    fn can_remove_adjacent_wait_stays_within_suit() {
        let counts = TileCounts::from_tile_types(vec![tt(8), tt(9)]);
        assert!(!counts.can_remove_adjacent_wait(tt(8)));
    }

    #[test]
    fn can_remove_adjacent_wait_rejects_honors() {
        let counts = TileCounts::from_tile_types(vec![tt(27), tt(28)]);
        assert!(!counts.can_remove_adjacent_wait(tt(27)));
    }

    #[test]
    fn can_remove_skip_wait_requires_both_tiles() {
        assert!(TileCounts::from_tile_types(vec![tt(0), tt(2)]).can_remove_skip_wait(tt(0)));
        assert!(!TileCounts::from_tile_types(vec![tt(0)]).can_remove_skip_wait(tt(0)));
        assert!(!TileCounts::from_tile_types(vec![tt(2)]).can_remove_skip_wait(tt(0)));
    }

    #[test]
    fn can_remove_skip_wait_allows_suit_edge() {
        let counts = TileCounts::from_tile_types(vec![tt(24), tt(26)]);
        assert!(counts.can_remove_skip_wait(tt(24)));
    }

    #[test]
    fn can_remove_skip_wait_stays_within_suit() {
        let counts = TileCounts::from_tile_types(vec![tt(25), tt(26), tt(27), tt(28)]);
        assert!(!counts.can_remove_skip_wait(tt(25)));
        assert!(!counts.can_remove_skip_wait(tt(26)));
    }

    #[test]
    fn can_remove_skip_wait_rejects_honors() {
        let counts = TileCounts::from_tile_types(vec![tt(27), tt(29)]);
        assert!(!counts.can_remove_skip_wait(tt(27)));
    }

    #[test]
    fn remove_adjacent_wait_removes_one_of_each() {
        let mut counts = TileCounts::from_tile_types(vec![tt(0), tt(1)]);
        counts.remove_adjacent_wait(tt(0)).unwrap();
        assert_eq!(counts.count(tt(0)), 0);
        assert_eq!(counts.count(tt(1)), 0);
        assert_eq!(counts.total(), 0);
    }

    #[test]
    fn remove_adjacent_wait_removes_at_suit_edge() {
        let mut counts = TileCounts::from_tile_types(vec![tt(7), tt(8)]);
        counts.remove_adjacent_wait(tt(7)).unwrap();
        assert_eq!(counts.count(tt(7)), 0);
        assert_eq!(counts.count(tt(8)), 0);
        assert_eq!(counts.total(), 0);
    }

    #[test]
    fn remove_adjacent_wait_fails_on_missing_tile_without_changing_counts() {
        let mut counts = TileCounts::from_tile_types(vec![tt(0)]);
        assert_eq!(
            counts.remove_adjacent_wait(tt(0)),
            Err(TileCountError::Underflow(tt(1)))
        );
        assert_eq!(counts.count(tt(0)), 1);
        assert_eq!(counts.total(), 1);
    }

    #[test]
    fn remove_adjacent_wait_stays_within_suit() {
        let mut counts = TileCounts::from_tile_types(vec![tt(8), tt(9)]);
        assert_eq!(
            counts.remove_adjacent_wait(tt(8)),
            Err(TileCountError::Underflow(tt(8)))
        );
        assert_eq!(counts.count(tt(8)), 1);
        assert_eq!(counts.count(tt(9)), 1);
        assert_eq!(counts.total(), 2);
    }

    #[test]
    fn remove_adjacent_wait_rejects_honors_without_changing_counts() {
        let mut counts = TileCounts::from_tile_types(vec![tt(27), tt(28)]);
        assert_eq!(
            counts.remove_adjacent_wait(tt(27)),
            Err(TileCountError::Underflow(tt(27)))
        );
        assert_eq!(counts.count(tt(27)), 1);
        assert_eq!(counts.count(tt(28)), 1);
        assert_eq!(counts.total(), 2);
    }

    #[test]
    fn remove_skip_wait_removes_one_of_each() {
        let mut counts = TileCounts::from_tile_types(vec![tt(0), tt(2)]);
        counts.remove_skip_wait(tt(0)).unwrap();
        assert_eq!(counts.count(tt(0)), 0);
        assert_eq!(counts.count(tt(2)), 0);
        assert_eq!(counts.total(), 0);
    }

    #[test]
    fn remove_skip_wait_removes_at_suit_edge() {
        let mut counts = TileCounts::from_tile_types(vec![tt(24), tt(26)]);
        counts.remove_skip_wait(tt(24)).unwrap();
        assert_eq!(counts.count(tt(24)), 0);
        assert_eq!(counts.count(tt(26)), 0);
        assert_eq!(counts.total(), 0);
    }

    #[test]
    fn remove_skip_wait_fails_on_missing_tile_without_changing_counts() {
        let mut counts = TileCounts::from_tile_types(vec![tt(0)]);
        assert_eq!(
            counts.remove_skip_wait(tt(0)),
            Err(TileCountError::Underflow(tt(2)))
        );
        assert_eq!(counts.count(tt(0)), 1);
        assert_eq!(counts.total(), 1);
    }

    #[test]
    fn remove_skip_wait_stays_within_suit() {
        let mut counts = TileCounts::from_tile_types(vec![tt(25), tt(26), tt(27), tt(28)]);
        assert_eq!(
            counts.remove_skip_wait(tt(25)),
            Err(TileCountError::Underflow(tt(25)))
        );
        assert_eq!(
            counts.remove_skip_wait(tt(26)),
            Err(TileCountError::Underflow(tt(26)))
        );
        assert_eq!(counts.total(), 4);
    }

    #[test]
    fn remove_skip_wait_rejects_honors_without_changing_counts() {
        let mut counts = TileCounts::from_tile_types(vec![tt(27), tt(29)]);
        assert_eq!(
            counts.remove_skip_wait(tt(27)),
            Err(TileCountError::Underflow(tt(27)))
        );
        assert_eq!(counts.count(tt(27)), 1);
        assert_eq!(counts.count(tt(29)), 1);
        assert_eq!(counts.total(), 2);
    }
}
