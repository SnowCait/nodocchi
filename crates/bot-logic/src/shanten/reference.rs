//! 差分検証用に残した、置き換え前の通常形向聴数探索。
//!
//! 手牌全体を1牌ずつ再帰的に分解する旧実装そのもので、production からは呼ばない。新しい色ごとの
//! 分解が同じ向聴数を返すことを確かめるための reference としてだけ使う。

use crate::count_hasher::CountHasherBuilder;
use crate::shanten::FixedMeldCount;
use crate::tile::TileType;
use crate::tile_counts::{TileCountError, TileCounts};
use std::collections::HashMap;

type SearchMemo = HashMap<([u8; 34], SearchState), i8, CountHasherBuilder>;

pub(crate) fn standard_shanten_with_fixed_melds(
    counts: &TileCounts,
    fixed_meld_count: FixedMeldCount,
) -> i8 {
    let state = SearchState {
        melds: fixed_meld_count.get(),
        has_pair: false,
        partials: 0,
    };

    SEARCH_MEMO.with_borrow_mut(|memo| {
        if memo.len() >= SEARCH_MEMO_CAPACITY {
            memo.clear();
        }
        search(*counts, state, memo)
    })
}

// 置き換え前と同じ探索 memo。差分検証で同じ牌姿を何度も評価するため、reference 側でも使い回す。
const SEARCH_MEMO_CAPACITY: usize = 1 << 17;

thread_local! {
    static SEARCH_MEMO: std::cell::RefCell<SearchMemo> =
        std::cell::RefCell::new(SearchMemo::default());
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct SearchState {
    melds: u8,
    has_pair: bool,
    partials: u8,
}

impl SearchState {
    fn shanten(self) -> i8 {
        let melds = self.melds.min(4);
        let capped_partials = self.partials.min(4 - melds);
        let pair_bonus = i8::from(self.has_pair);
        8 - 2 * melds as i8 - capped_partials as i8 - pair_bonus
    }

    fn with_meld(self) -> Self {
        Self {
            melds: self.melds + 1,
            ..self
        }
    }

    fn with_pair_head(self) -> Self {
        Self {
            has_pair: true,
            ..self
        }
    }

    fn with_partial(self) -> Self {
        Self {
            partials: self.partials + 1,
            ..self
        }
    }
}

fn search(counts: TileCounts, state: SearchState, memo: &mut SearchMemo) -> i8 {
    let mut best = state.shanten();

    let Some(tile) = counts
        .iter()
        .find_map(|(tile, count)| (count >= 1).then_some(tile))
    else {
        return best;
    };

    let key = (*counts.as_array(), state);
    if let Some(&cached) = memo.get(&key) {
        return cached;
    }

    if state.melds < 4 {
        best = best.min(try_branch(
            counts,
            tile,
            TileCounts::remove_triplet,
            state.with_meld(),
            memo,
        ));
        best = best.min(try_branch(
            counts,
            tile,
            TileCounts::remove_sequence,
            state.with_meld(),
            memo,
        ));
    }

    if !state.has_pair {
        best = best.min(try_branch(
            counts,
            tile,
            TileCounts::remove_pair,
            state.with_pair_head(),
            memo,
        ));
    }

    if state.melds + state.partials < 4 {
        best = best.min(try_branch(
            counts,
            tile,
            TileCounts::remove_pair,
            state.with_partial(),
            memo,
        ));
        best = best.min(try_branch(
            counts,
            tile,
            TileCounts::remove_adjacent_wait,
            state.with_partial(),
            memo,
        ));
        best = best.min(try_branch(
            counts,
            tile,
            TileCounts::remove_skip_wait,
            state.with_partial(),
            memo,
        ));
    }

    let mut removed = counts;
    if removed.remove(tile).is_ok() {
        best = best.min(search(removed, state, memo));
    }

    memo.insert(key, best);
    best
}

fn try_branch(
    counts: TileCounts,
    tile: TileType,
    remove: fn(&mut TileCounts, TileType) -> Result<(), TileCountError>,
    next_state: SearchState,
    memo: &mut SearchMemo,
) -> i8 {
    let mut removed = counts;
    if remove(&mut removed, tile).is_ok() {
        search(removed, next_state, memo)
    } else {
        i8::MAX
    }
}
