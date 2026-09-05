//! 差分検証用に残した、置き換え前の牌種構成まわりの実装。
//!
//! 手牌の牌種を毎回 `Vec<TileType>` に並べ直していた頃のものそのままで、production からは
//! 呼ばない。牌種ごとの枚数 ([`crate::tile_counts::TileCounts`]) を読む新しい実装が同じ役を
//! 返すことを確かめる reference としてだけ使う。

use crate::completed_hand::CompletedHandAnalysis;
use crate::meld::MeldShape;
use crate::tile::{Suit, TileType};
use crate::yaku::{SHOUSANGEN_DRAGON_SET_COUNT, Yaku};

pub(crate) fn hand_tile_types(analysis: &CompletedHandAnalysis) -> Vec<TileType> {
    analysis
        .concealed_tiles()
        .iter()
        .chain(analysis.fixed_melds().iter().flat_map(|meld| meld.tiles()))
        .map(|tile| tile.tile_type())
        .collect()
}

pub(crate) fn tile_composition_yaku(tiles: &[TileType]) -> Vec<Yaku> {
    let mut yaku = Vec::new();
    if tiles.is_empty() {
        return yaku;
    }

    if tiles.iter().all(|tile| !tile.is_yaochu()) {
        yaku.push(Yaku::Tanyao);
    }
    if tiles.iter().all(|tile| tile.is_yaochu()) {
        yaku.push(Yaku::Honroutou);
    }
    if single_suit(tiles).is_some() {
        if tiles.iter().any(|tile| tile.is_honor()) {
            yaku.push(Yaku::Honitsu);
        } else {
            yaku.push(Yaku::Chinitsu);
        }
    }

    yaku
}

pub(crate) fn single_suit(tiles: &[TileType]) -> Option<Suit> {
    let mut found = None;
    for suit in tiles.iter().filter_map(|tile| tile.suit()) {
        match found {
            None => found = Some(suit),
            Some(existing) if existing == suit => {}
            Some(_) => return None,
        }
    }
    found
}

pub(crate) fn is_shousangen(pair: TileType, melds: &[MeldShape]) -> bool {
    if !pair.is_dragon() {
        return false;
    }

    let mut dragons: Vec<TileType> = melds
        .iter()
        .filter_map(|meld| meld.triplet_tile_type())
        .filter(|tile| tile.is_dragon())
        .collect();
    dragons.sort_unstable();
    dragons.dedup();
    dragons.len() == SHOUSANGEN_DRAGON_SET_COUNT && !dragons.contains(&pair)
}

pub(crate) fn set_tiles(melds: &[MeldShape], predicate: fn(TileType) -> bool) -> Vec<TileType> {
    let mut tiles: Vec<TileType> = melds
        .iter()
        .filter_map(|meld| meld.triplet_tile_type())
        .filter(|tile| predicate(*tile))
        .collect();
    tiles.sort_unstable();
    tiles.dedup();
    tiles
}
