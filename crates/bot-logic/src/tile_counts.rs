use crate::tile::TileType;
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
    pub fn empty() -> Self {
        Self { counts: [0; 34] }
    }

    pub fn from_tile_types<I: IntoIterator<Item = TileType>>(tiles: I) -> Self {
        let mut counts = Self::empty();
        for tile in tiles {
            counts.add(tile);
        }
        counts
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

    pub fn get(&self, tile: TileType) -> u8 {
        self.counts[tile.index()]
    }

    pub fn total(&self) -> u8 {
        self.counts.iter().sum()
    }

    pub fn remaining_count(&self, tile: TileType) -> u8 {
        4u8.saturating_sub(self.get(tile))
    }

    pub fn as_array(&self) -> &[u8; 34] {
        &self.counts
    }

    pub fn iter(&self) -> impl Iterator<Item = (TileType, u8)> {
        TileType::all().map(|tile| (tile, self.get(tile)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tt(value: u8) -> TileType {
        TileType::new(value).unwrap()
    }

    #[test]
    fn empty_has_zero_total() {
        let counts = TileCounts::empty();
        assert_eq!(counts.total(), 0);
        assert_eq!(counts.get(tt(0)), 0);
    }

    #[test]
    fn from_tile_types_and_total() {
        let counts = TileCounts::from_tile_types(vec![tt(0), tt(0), tt(4), tt(33)]);
        assert_eq!(counts.total(), 4);
        assert_eq!(counts.get(tt(0)), 2);
        assert_eq!(counts.get(tt(4)), 1);
        assert_eq!(counts.get(tt(33)), 1);
        assert_eq!(counts.get(tt(1)), 0);
    }

    #[test]
    #[should_panic(expected = "more than four copies")]
    fn from_tile_types_panics_on_fifth_copy() {
        TileCounts::from_tile_types(vec![tt(0); 5]);
    }

    #[test]
    fn add_and_remove() {
        let mut counts = TileCounts::empty();
        counts.add(tt(5));
        assert_eq!(counts.get(tt(5)), 1);
        counts.remove(tt(5)).unwrap();
        assert_eq!(counts.get(tt(5)), 0);
        assert_eq!(counts.remove(tt(5)), Err(TileCountError::Underflow(tt(5))));
    }

    #[test]
    fn try_add_allows_up_to_four_copies() {
        let mut counts = TileCounts::empty();
        for _ in 0..4 {
            counts.try_add(tt(0)).unwrap();
        }
        assert_eq!(counts.get(tt(0)), 4);
        assert_eq!(counts.remaining_count(tt(0)), 0);
        assert_eq!(counts.try_add(tt(0)), Err(TileCountError::Overflow(tt(0))));
        assert_eq!(counts.get(tt(0)), 4);
    }

    #[test]
    fn try_add_succeeds_again_after_remove() {
        let mut counts = TileCounts::from_tile_types(vec![tt(7); 4]);
        assert_eq!(counts.try_add(tt(7)), Err(TileCountError::Overflow(tt(7))));
        counts.remove(tt(7)).unwrap();
        counts.try_add(tt(7)).unwrap();
        assert_eq!(counts.get(tt(7)), 4);
    }

    #[test]
    fn remaining_count_subtracts_from_four() {
        let mut counts = TileCounts::empty();
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
