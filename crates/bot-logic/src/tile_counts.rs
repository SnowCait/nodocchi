use crate::tile::TileType;
use thiserror::Error;

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum TileCountError {
    #[error("no tile of type {0:?} left to remove")]
    Underflow(TileType),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TileCounts {
    pub counts: [u8; 34],
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

    pub fn add(&mut self, tile: TileType) {
        self.counts[tile.index()] += 1;
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
    fn add_and_remove() {
        let mut counts = TileCounts::empty();
        counts.add(tt(5));
        assert_eq!(counts.get(tt(5)), 1);
        counts.remove(tt(5)).unwrap();
        assert_eq!(counts.get(tt(5)), 0);
        assert_eq!(counts.remove(tt(5)), Err(TileCountError::Underflow(tt(5))));
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
    fn iter_yields_all_34_entries() {
        let counts = TileCounts::from_tile_types(vec![tt(3), tt(3)]);
        let entries: Vec<_> = counts.iter().collect();
        assert_eq!(entries.len(), 34);
        assert_eq!(entries[3], (tt(3), 2));
        assert_eq!(entries[0], (tt(0), 0));
        assert_eq!(entries[33], (tt(33), 0));
    }
}
