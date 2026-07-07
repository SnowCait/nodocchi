use crate::tile::{TileId, TileType};
use crate::tile_counts::TileCounts;
use thiserror::Error;

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum HandError {
    #[error("tile {0:?} is not in hand")]
    TileNotFound(TileId),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hand {
    tiles: Vec<TileId>,
    counts: TileCounts,
}

impl Hand {
    pub fn empty() -> Self {
        Self {
            tiles: Vec::new(),
            counts: TileCounts::empty(),
        }
    }

    pub fn from_tiles(tiles: Vec<TileId>) -> Self {
        let counts = TileCounts::from_tile_types(tiles.iter().map(|tile| tile.tile_type()));
        Self { tiles, counts }
    }

    pub fn tiles(&self) -> &[TileId] {
        &self.tiles
    }

    pub fn counts(&self) -> &TileCounts {
        &self.counts
    }

    pub fn add(&mut self, tile: TileId) {
        self.counts.add(tile.tile_type());
        self.tiles.push(tile);
    }

    pub fn remove(&mut self, tile: TileId) -> Result<(), HandError> {
        let position = self
            .tiles
            .iter()
            .position(|&t| t == tile)
            .ok_or(HandError::TileNotFound(tile))?;
        self.tiles.remove(position);
        self.counts
            .remove(tile.tile_type())
            .map_err(|_| HandError::TileNotFound(tile))
    }

    pub fn contains(&self, tile: TileId) -> bool {
        self.tiles.contains(&tile)
    }

    pub fn count_type(&self, tile: TileType) -> u8 {
        self.counts.get(tile)
    }

    pub fn red_count(&self) -> u8 {
        self.tiles.iter().filter(|tile| tile.is_red()).count() as u8
    }

    pub fn sort_by_id(&mut self) {
        self.tiles.sort_unstable();
    }

    pub fn len(&self) -> usize {
        self.tiles.len()
    }

    pub fn is_empty(&self) -> bool {
        self.tiles.is_empty()
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
    fn empty_hand() {
        let hand = Hand::empty();
        assert!(hand.is_empty());
        assert_eq!(hand.len(), 0);
        assert_eq!(hand.counts().total(), 0);
        assert!(hand.tiles().is_empty());
    }

    #[test]
    fn from_tiles_builds_counts() {
        let hand = Hand::from_tiles(vec![id(0), id(1), id(16)]);
        assert_eq!(hand.len(), 3);
        assert_eq!(hand.count_type(tt(0)), 2);
        assert_eq!(hand.count_type(tt(4)), 1);
        assert_eq!(hand.counts().total(), 3);
        assert_eq!(hand.counts().as_array()[0], 2);
    }

    #[test]
    fn add_and_remove_keep_tiles_and_counts_in_sync() {
        let mut hand = Hand::empty();
        hand.add(id(16));
        hand.add(id(17));
        assert_eq!(hand.len(), 2);
        assert_eq!(hand.count_type(tt(4)), 2);
        assert_eq!(hand.counts().total(), 2);

        hand.remove(id(16)).unwrap();
        assert_eq!(hand.len(), 1);
        assert_eq!(hand.count_type(tt(4)), 1);
        assert_eq!(hand.counts().total(), 1);
        assert!(!hand.contains(id(16)));
        assert!(hand.contains(id(17)));
        assert_eq!(hand.tiles(), &[id(17)]);
    }

    #[test]
    #[should_panic(expected = "more than four copies")]
    fn add_panics_on_fifth_copy_of_same_type() {
        let mut hand = Hand::from_tiles(vec![id(0), id(1), id(2), id(3)]);
        hand.add(id(0));
    }

    #[test]
    fn add_keeps_hand_consistent_when_counts_add_panics() {
        let mut hand = Hand::from_tiles(vec![id(0), id(1), id(2), id(3)]);
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            hand.add(id(0));
        }));
        assert!(result.is_err());
        assert_eq!(hand.len(), 4);
        assert_eq!(hand.counts().total(), 4);
        assert_eq!(hand.count_type(tt(0)), 4);
        assert_eq!(hand.tiles(), &[id(0), id(1), id(2), id(3)]);
    }

    #[test]
    fn remove_missing_tile_fails() {
        let mut hand = Hand::from_tiles(vec![id(17)]);
        assert_eq!(hand.remove(id(16)), Err(HandError::TileNotFound(id(16))));
        assert_eq!(hand.len(), 1);
        assert_eq!(hand.counts().total(), 1);
    }

    #[test]
    fn red_count_counts_red_fives() {
        let hand = Hand::from_tiles(vec![id(16), id(52), id(88), id(17)]);
        assert_eq!(hand.red_count(), 3);
        assert_eq!(Hand::empty().red_count(), 0);
    }

    #[test]
    fn sort_by_id_orders_tiles() {
        let mut hand = Hand::from_tiles(vec![id(88), id(0), id(52)]);
        hand.sort_by_id();
        assert_eq!(hand.tiles(), &[id(0), id(52), id(88)]);
    }
}
