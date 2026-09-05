//! 差分検証用に残した、置き換え前の牌種構成まわりの役満判定。
//!
//! 手牌の牌種を毎回 `Vec<TileType>` に並べ直し、面子から取り出した牌種も `Vec` を並べ替えて
//! 重複を消していた頃のものそのままで、production からは呼ばない。

use crate::meld::{Meld, MeldShape};
use crate::tile::{Suit, TileType};
use crate::tile_counts::TileCounts;
use crate::yaku::reference::set_tiles;
use crate::yakuman::{
    CHUUREN_TERMINAL_COUNT, CHUUREN_TILE_COUNT, DAISUUSHII_WIND_SET_COUNT, NUMBER_COUNT,
    SHOUSUUSHII_WIND_SET_COUNT, Yakuman, is_green,
};

pub(crate) fn tile_composition_yakuman(tiles: &[TileType], fixed_melds: &[Meld]) -> Vec<Yakuman> {
    let mut yakuman = Vec::new();
    if tiles.is_empty() {
        return yakuman;
    }

    if is_chuuren_poutou(tiles, fixed_melds) {
        yakuman.push(Yakuman::ChuurenPoutou);
    }
    if tiles.iter().all(|tile| is_green(*tile)) {
        yakuman.push(Yakuman::Ryuuiisou);
    }
    if tiles.iter().all(|tile| tile.is_terminal()) {
        yakuman.push(Yakuman::Chinroutou);
    }
    if tiles.iter().all(|tile| tile.is_honor()) {
        yakuman.push(Yakuman::Tsuuiisou);
    }

    yakuman
}

pub(crate) fn is_chuuren_poutou(tiles: &[TileType], fixed_melds: &[Meld]) -> bool {
    if !fixed_melds.is_empty() {
        return false;
    }

    let mut counts = TileCounts::new();
    for tile in tiles {
        if counts.try_add(*tile).is_err() {
            return false;
        }
    }
    if counts.total() != CHUUREN_TILE_COUNT {
        return false;
    }

    let mut suit: Option<Suit> = None;
    let mut numbers = [0u8; NUMBER_COUNT];
    for (tile, count) in counts.iter().filter(|(_, count)| *count > 0) {
        let (Some(tile_suit), Some(number)) = (tile.suit(), tile.number()) else {
            return false;
        };
        if *suit.get_or_insert(tile_suit) != tile_suit {
            return false;
        }
        numbers[usize::from(number - 1)] = count;
    }

    numbers[0] >= CHUUREN_TERMINAL_COUNT
        && numbers[NUMBER_COUNT - 1] >= CHUUREN_TERMINAL_COUNT
        && numbers[1..NUMBER_COUNT - 1].iter().all(|count| *count >= 1)
}

pub(crate) fn wind_yakuman(pair: TileType, melds: &[MeldShape]) -> Option<Yakuman> {
    let winds = set_tiles(melds, TileType::is_wind);
    if winds.len() == DAISUUSHII_WIND_SET_COUNT {
        return Some(Yakuman::Daisuushii);
    }
    let shousuushii =
        winds.len() == SHOUSUUSHII_WIND_SET_COUNT && pair.is_wind() && !winds.contains(&pair);
    shousuushii.then_some(Yakuman::Shousuushii)
}
