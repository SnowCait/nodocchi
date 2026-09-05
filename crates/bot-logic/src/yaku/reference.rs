//! 差分検証用に残した、置き換え前の実装。
//!
//! 手牌の牌種を毎回 `Vec<TileType>` に並べ直していた頃のものと、役を段ごとに `Vec<Yaku>` へ
//! 集めてから連結していた頃の合成順で、production からは呼ばない。新しい実装が同じ役を返す
//! ことを確かめる reference としてだけ使う。役の成立規則は production 側の関数を呼ぶだけで、
//! ここには書き直さない。

use crate::completed_hand::{CompletedHandAnalysis, CompletedHandDecomposition};
use crate::meld::{Meld, MeldShape};
use crate::tile::{Suit, TileType};
use crate::tile_counts::TileCounts;
use crate::winning_context::WinningContext;
use crate::yaku::{
    SHOUSANGEN_DRAGON_SET_COUNT, Yaku, decomposition_yaku, push_win_context_yaku,
    push_yakuhai_yaku, standard_meld_shapes,
};

/// 置き換え前の [`crate::yaku::decomposition_yaku_with_context`]。
///
/// 構成役の `Vec` を作ってから状況役の `Vec` を連結し、最後に並べ替えて重複を消していた。
pub(crate) fn decomposition_yaku_with_context(
    decomposition: &CompletedHandDecomposition,
    fixed_melds: &[Meld],
    counts: &TileCounts,
    context: WinningContext,
    menzen: bool,
) -> Vec<Yaku> {
    let mut yaku = decomposition_yaku(decomposition, fixed_melds, counts, menzen);
    yaku.extend(contextual_yaku(decomposition, fixed_melds, context, menzen));
    yaku.sort_unstable();
    yaku.dedup();
    yaku
}

fn contextual_yaku(
    decomposition: &CompletedHandDecomposition,
    fixed_melds: &[Meld],
    context: WinningContext,
    menzen: bool,
) -> Vec<Yaku> {
    match decomposition {
        CompletedHandDecomposition::Standard(standard) => {
            let Some(melds) = standard_meld_shapes(standard, fixed_melds) else {
                return Vec::new();
            };
            let mut yaku = yakuhai_yaku(&melds, context);
            yaku.extend(win_context_yaku(context, menzen));
            yaku
        }
        CompletedHandDecomposition::Chiitoitsu(_) | CompletedHandDecomposition::Kokushi(_) => {
            win_context_yaku(context, menzen)
        }
    }
}

fn yakuhai_yaku(melds: &[MeldShape], context: WinningContext) -> Vec<Yaku> {
    let mut yaku = Vec::new();
    push_yakuhai_yaku(&mut yaku, melds, context);
    yaku
}

fn win_context_yaku(context: WinningContext, menzen: bool) -> Vec<Yaku> {
    let mut yaku = Vec::new();
    push_win_context_yaku(&mut yaku, context, menzen);
    yaku
}

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
