use crate::shanten::{Shanten, calculate_shanten};
use crate::tile::{TileId, TileType};
use crate::tile_counts::TileCounts;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AcceptanceTile {
    pub tile: TileType,
    pub remaining: u8,
    pub shanten_after_draw: Shanten,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Acceptance {
    pub current: Shanten,
    pub tiles: Vec<AcceptanceTile>,
}

impl Acceptance {
    pub fn is_empty(&self) -> bool {
        self.tiles.is_empty()
    }

    pub fn total_remaining(&self) -> u8 {
        self.tiles.iter().map(|tile| tile.remaining).sum()
    }
}

pub fn calculate_acceptance(counts: &TileCounts) -> Acceptance {
    calculate_acceptance_with_seen(counts, &[0; TileType::COUNT])
}

pub fn calculate_acceptance_with_visible_tiles(
    counts: &TileCounts,
    visible_tiles: &[TileId],
) -> Acceptance {
    if visible_tiles.is_empty() {
        return calculate_acceptance(counts);
    }

    let visible_counts = TileCounts::from_tiles(visible_tiles.iter().copied());
    let mut additional_seen = [0u8; TileType::COUNT];
    for tile in TileType::all() {
        additional_seen[tile.index()] = visible_counts
            .count(tile)
            .saturating_sub(counts.count(tile));
    }

    calculate_acceptance_with_seen(counts, &additional_seen)
}

pub(crate) fn calculate_acceptance_with_seen(
    counts: &TileCounts,
    additional_seen: &[u8; TileType::COUNT],
) -> Acceptance {
    let current = calculate_shanten(counts);
    let current_min = current.min();
    let mut tiles = Vec::new();

    for tile in TileType::all() {
        let seen = counts.count(tile) + additional_seen[tile.index()];
        let remaining = 4u8.saturating_sub(seen);
        if remaining == 0 {
            continue;
        }

        let mut added = *counts;
        if added.try_add(tile).is_err() {
            continue;
        }

        let shanten_after_draw = calculate_shanten(&added);
        if shanten_after_draw.min() < current_min {
            tiles.push(AcceptanceTile {
                tile,
                remaining,
                shanten_after_draw,
            });
        }
    }

    Acceptance { current, tiles }
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

    fn accepted_tiles(acceptance: &Acceptance) -> Vec<TileType> {
        acceptance.tiles.iter().map(|entry| entry.tile).collect()
    }

    fn ids(values: &[u8]) -> Vec<TileId> {
        values.iter().map(|&v| TileId::new(v).unwrap()).collect()
    }

    fn remaining_of(acceptance: &Acceptance, wait: TileType) -> Option<u8> {
        acceptance
            .tiles
            .iter()
            .find(|entry| entry.tile == wait)
            .map(|entry| entry.remaining)
    }

    #[test]
    fn empty_hand_has_no_acceptance() {
        let acceptance = calculate_acceptance(&TileCounts::new());
        assert_eq!(acceptance.current.min(), 8);
        assert!(acceptance.is_empty());
        assert_eq!(acceptance.total_remaining(), 0);
    }

    #[test]
    fn standard_tenpai_accepts_winning_tile() {
        let counts = counts(&[
            "1m", "2m", "3m", "4m", "5m", "6m", "7m", "8m", "9m", "1p", "2p", "3p", "5s",
        ]);
        let acceptance = calculate_acceptance(&counts);
        assert_eq!(acceptance.current.min(), 0);
        assert_eq!(accepted_tiles(&acceptance), vec![tile("5s")]);
        assert_eq!(acceptance.tiles[0].shanten_after_draw.min(), -1);
        assert_eq!(acceptance.tiles[0].remaining, 3);
        assert_eq!(acceptance.total_remaining(), 3);
    }

    #[test]
    fn chiitoitsu_tenpai_accepts_pair_tile() {
        let counts = counts(&[
            "1m", "1m", "2m", "2m", "3m", "3m", "4p", "4p", "5p", "5p", "6s", "6s", "E",
        ]);
        let acceptance = calculate_acceptance(&counts);
        assert_eq!(acceptance.current.min(), 0);
        let east = acceptance
            .tiles
            .iter()
            .find(|entry| entry.tile == tile("E"))
            .expect("E should be accepted");
        assert_eq!(east.shanten_after_draw.min(), -1);
        assert_eq!(east.remaining, 3);
    }

    #[test]
    fn kokushi_thirteen_wait_accepts_thirteen_tiles() {
        let counts = counts(&[
            "1m", "9m", "1p", "9p", "1s", "9s", "E", "S", "W", "N", "P", "F", "C",
        ]);
        let acceptance = calculate_acceptance(&counts);
        assert_eq!(acceptance.current.min(), 0);
        let expected: Vec<TileType> = [
            "1m", "9m", "1p", "9p", "1s", "9s", "E", "S", "W", "N", "P", "F", "C",
        ]
        .iter()
        .map(|s| tile(s))
        .collect();
        assert_eq!(accepted_tiles(&acceptance), expected);
        assert!(
            acceptance
                .tiles
                .iter()
                .all(|entry| entry.shanten_after_draw.min() == -1)
        );
        assert!(acceptance.tiles.iter().all(|entry| entry.remaining == 3));
        assert_eq!(acceptance.total_remaining(), 39);
    }

    #[test]
    fn tile_with_no_remaining_is_excluded() {
        let counts = counts(&["1m", "1m", "1m", "1m"]);
        assert_eq!(counts.remaining_count(tile("1m")), 0);
        let acceptance = calculate_acceptance(&counts);
        let tiles = accepted_tiles(&acceptance);
        assert!(!tiles.contains(&tile("1m")));
        assert!(tiles.contains(&tile("2m")));
    }

    #[test]
    fn does_not_modify_input_counts() {
        let counts = counts(&[
            "1m", "2m", "3m", "4m", "5m", "6m", "7m", "8m", "9m", "1p", "2p", "3p", "5s",
        ]);
        let before = counts;
        let _ = calculate_acceptance(&counts);
        assert_eq!(counts, before);
    }

    #[test]
    fn visible_empty_matches_plain_acceptance() {
        let hand = ids(&[0, 4, 8, 12, 17, 20, 24, 28, 32, 36, 40, 44, 89]);
        let counts = TileCounts::from_tiles(hand.iter().copied());
        assert_eq!(
            calculate_acceptance_with_visible_tiles(&counts, &[]),
            calculate_acceptance(&counts)
        );
    }

    #[test]
    fn visible_does_not_double_count_own_hand() {
        let hand = ids(&[0, 4, 8, 12, 17, 20, 24, 28, 32, 36, 40, 44, 89]);
        let counts = TileCounts::from_tiles(hand.iter().copied());
        let acceptance = calculate_acceptance_with_visible_tiles(&counts, &hand);
        assert_eq!(remaining_of(&acceptance, tile("5s")), Some(3));
        assert_eq!(acceptance.total_remaining(), 3);
    }

    #[test]
    fn visible_wait_tile_reduces_remaining_by_one() {
        let hand = ids(&[0, 4, 8, 12, 17, 20, 24, 28, 32, 36, 40, 44, 89]);
        let counts = TileCounts::from_tiles(hand.iter().copied());
        let mut visible = hand.clone();
        visible.extend(ids(&[90]));
        let acceptance = calculate_acceptance_with_visible_tiles(&counts, &visible);
        assert_eq!(remaining_of(&acceptance, tile("5s")), Some(2));
    }

    #[test]
    fn visible_removes_wait_when_all_copies_seen() {
        let hand = ids(&[0, 4, 8, 12, 17, 20, 24, 28, 32, 36, 40, 44, 89]);
        let counts = TileCounts::from_tiles(hand.iter().copied());
        let mut visible = hand.clone();
        visible.extend(ids(&[88, 90, 91]));
        let acceptance = calculate_acceptance_with_visible_tiles(&counts, &visible);
        assert_eq!(remaining_of(&acceptance, tile("5s")), None);
        assert_eq!(acceptance.total_remaining(), 0);
        assert_eq!(acceptance.current.min(), 0);
    }

    #[test]
    fn visible_does_not_apply_candidate_discard_correction() {
        let hand = ids(&[0, 4, 8, 12, 17, 20, 24, 28, 32, 36, 40, 44, 89]);
        let counts = TileCounts::from_tiles(hand.iter().copied());
        let plain = calculate_acceptance(&counts);
        let visible = calculate_acceptance_with_visible_tiles(&counts, &hand);
        assert_eq!(
            remaining_of(&visible, tile("5s")),
            remaining_of(&plain, tile("5s"))
        );
    }

    #[test]
    fn tiles_are_ordered_by_tile_type() {
        let counts = counts(&[
            "1m", "2m", "3m", "4m", "5m", "6m", "7m", "8m", "9m", "2p", "3p", "5s", "5s",
        ]);
        let acceptance = calculate_acceptance(&counts);
        assert_eq!(accepted_tiles(&acceptance), vec![tile("1p"), tile("4p")]);
        assert!(
            acceptance
                .tiles
                .windows(2)
                .all(|pair| pair[0].tile.raw() < pair[1].tile.raw())
        );
    }
}
