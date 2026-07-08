use crate::tile::{TileId, TileType};
use thiserror::Error;

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum TileCountError {
    #[error("no tile of type {0:?} left to remove")]
    Underflow(TileType),

    #[error("too many tiles of type {0:?}")]
    Overflow(TileType),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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
}
