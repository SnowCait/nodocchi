use bot_logic::TileId;
use thiserror::Error;

use crate::input::LogicalTile;

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum TileAllocationError {
    #[error("{tile} has no physical copy left, a tile type has only 4 copies")]
    NoCopyLeft { tile: String },

    #[error("{tile} has no black copy left, only 3 black copies exist beside the red five")]
    NoBlackCopyLeft { tile: String },

    #[error("red five {tile} appears more than once")]
    DuplicateRedFive { tile: String },

    #[error("{tile} has no red five")]
    NoRedFive { tile: String },

    #[error("physical tile {tile} is used more than once")]
    DuplicatePhysicalTile { tile: String },
}

#[derive(Debug)]
pub struct TileAllocator {
    used: [bool; TileId::COUNT],
}

impl Default for TileAllocator {
    fn default() -> Self {
        Self {
            used: [false; TileId::COUNT],
        }
    }
}

impl TileAllocator {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn allocate(&mut self, tile: LogicalTile) -> Result<TileId, TileAllocationError> {
        if tile.red {
            self.allocate_red(tile)
        } else {
            self.allocate_black(tile)
        }
    }

    fn allocate_red(&mut self, tile: LogicalTile) -> Result<TileId, TileAllocationError> {
        let id = physical_copies(tile)
            .into_iter()
            .find(|id| id.is_red())
            .ok_or_else(|| TileAllocationError::NoRedFive {
                tile: tile.to_mjai_string(),
            })?;

        if self.used[id.index()] {
            return Err(TileAllocationError::DuplicateRedFive {
                tile: tile.to_mjai_string(),
            });
        }

        self.used[id.index()] = true;
        Ok(id)
    }

    fn allocate_black(&mut self, tile: LogicalTile) -> Result<TileId, TileAllocationError> {
        let copies = physical_copies(tile);

        if let Some(id) = copies
            .iter()
            .copied()
            .find(|id| !id.is_red() && !self.used[id.index()])
        {
            self.used[id.index()] = true;
            return Ok(id);
        }

        if copies.iter().all(|id| self.used[id.index()]) {
            Err(TileAllocationError::NoCopyLeft {
                tile: tile.to_mjai_string(),
            })
        } else {
            Err(TileAllocationError::NoBlackCopyLeft {
                tile: tile.to_mjai_string(),
            })
        }
    }
}

fn physical_copies(tile: LogicalTile) -> Vec<TileId> {
    let base = tile.tile_type.raw() * 4;
    (0..4)
        .filter_map(|offset| TileId::new(base + offset))
        .collect()
}

pub fn validate_unique_physical_tiles(tiles: &[TileId]) -> Result<(), TileAllocationError> {
    let mut seen = [false; TileId::COUNT];
    for tile in tiles {
        if seen[tile.index()] {
            return Err(TileAllocationError::DuplicatePhysicalTile {
                tile: tile.to_mjai_string(),
            });
        }
        seen[tile.index()] = true;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::input::parse_tiles;

    fn allocate_all(input: &str) -> Result<Vec<TileId>, TileAllocationError> {
        let mut allocator = TileAllocator::new();
        parse_tiles(input)
            .unwrap()
            .into_iter()
            .map(|tile| allocator.allocate(tile))
            .collect()
    }

    fn allocate_labels(input: &str) -> Vec<String> {
        allocate_all(input)
            .unwrap()
            .into_iter()
            .map(|tile| tile.to_mjai_string())
            .collect()
    }

    #[test]
    fn same_tile_type_gets_distinct_physical_tiles() {
        let tiles = allocate_all("111m").unwrap();
        assert_eq!(tiles.len(), 3);
        assert_eq!(tiles[0].raw(), 0);
        assert_eq!(tiles[1].raw(), 1);
        assert_eq!(tiles[2].raw(), 2);
        assert!(validate_unique_physical_tiles(&tiles).is_ok());
    }

    #[test]
    fn allocation_order_is_deterministic() {
        assert_eq!(
            allocate_all("1m1m1m").unwrap(),
            allocate_all("111m").unwrap()
        );
    }

    #[test]
    fn red_fives_use_existing_red_tile_ids() {
        assert_eq!(allocate_all("0m").unwrap()[0].raw(), 16);
        assert_eq!(allocate_all("0p").unwrap()[0].raw(), 52);
        assert_eq!(allocate_all("0s").unwrap()[0].raw(), 88);
        assert_eq!(allocate_all("5mr").unwrap()[0].raw(), 16);
        assert_eq!(allocate_all("5pr").unwrap()[0].raw(), 52);
        assert_eq!(allocate_all("5sr").unwrap()[0].raw(), 88);
    }

    #[test]
    fn black_five_never_uses_the_red_tile_id() {
        let tiles = allocate_all("555m").unwrap();
        assert!(tiles.iter().all(|tile| !tile.is_red()));
        assert_eq!(allocate_labels("555m"), ["5m", "5m", "5m"]);
    }

    #[test]
    fn black_and_red_five_are_distinguished() {
        let tiles = allocate_all("5m 0m").unwrap();
        assert!(!tiles[0].is_red());
        assert!(tiles[1].is_red());
        assert_eq!(allocate_labels("5m 0m"), ["5m", "5mr"]);
    }

    #[test]
    fn rejects_fifth_copy_of_a_tile_type() {
        assert_eq!(
            allocate_all("11111m"),
            Err(TileAllocationError::NoCopyLeft {
                tile: "1m".to_string(),
            })
        );
    }

    #[test]
    fn rejects_fifth_copy_of_a_five_including_the_red_one() {
        assert_eq!(
            allocate_all("0m 5555m"),
            Err(TileAllocationError::NoCopyLeft {
                tile: "5m".to_string(),
            })
        );
    }

    #[test]
    fn rejects_fourth_black_five() {
        assert_eq!(
            allocate_all("5555m"),
            Err(TileAllocationError::NoBlackCopyLeft {
                tile: "5m".to_string(),
            })
        );
    }

    #[test]
    fn rejects_duplicated_red_five() {
        assert_eq!(
            allocate_all("0m 5mr"),
            Err(TileAllocationError::DuplicateRedFive {
                tile: "5mr".to_string(),
            })
        );
    }

    #[test]
    fn rejects_red_tile_without_red_physical_copy() {
        let mut allocator = TileAllocator::new();
        let tile = LogicalTile::red(bot_logic::TileType::from_mjai_type_str("1m").unwrap());
        assert_eq!(
            allocator.allocate(tile),
            Err(TileAllocationError::NoRedFive {
                tile: "1mr".to_string(),
            })
        );
    }

    #[test]
    fn allocator_is_shared_across_zones() {
        let mut allocator = TileAllocator::new();
        let first: Vec<_> = parse_tiles("11m")
            .unwrap()
            .into_iter()
            .map(|tile| allocator.allocate(tile).unwrap())
            .collect();
        let second: Vec<_> = parse_tiles("11m")
            .unwrap()
            .into_iter()
            .map(|tile| allocator.allocate(tile).unwrap())
            .collect();
        let all: Vec<_> = first.into_iter().chain(second).collect();
        assert_eq!(all.len(), 4);
        assert!(validate_unique_physical_tiles(&all).is_ok());
    }

    #[test]
    fn validate_unique_physical_tiles_rejects_duplicates() {
        let tile = TileId::new(16).unwrap();
        assert_eq!(
            validate_unique_physical_tiles(&[tile, tile]),
            Err(TileAllocationError::DuplicatePhysicalTile {
                tile: "5mr".to_string(),
            })
        );
    }

    #[test]
    fn validate_unique_physical_tiles_accepts_distinct_tiles() {
        let tiles: Vec<_> = (0..4).filter_map(TileId::new).collect();
        assert!(validate_unique_physical_tiles(&tiles).is_ok());
    }
}
