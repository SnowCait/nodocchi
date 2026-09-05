//! 骨格をまとめて求めるようにした受け入れ列挙が、牌種ごとに向聴数を計算し直していた頃と完全に
//! 一致することを確かめる差分検証。
//!
//! 比較対象の向聴数は既存の [`calculate_shanten_with_fixed_melds`] をそのまま呼ぶ。ここに向聴数の
//! 規則は持たず、受け入れの列挙条件だけを変更前の形で書き下す。
//!
//! 手牌・副露済み面子数・見え牌は固定 seed の擬似乱数で作り、見え牌なしの場合も必ず含める。

use super::{
    DrawableTile, calculate_acceptance_with_fixed_melds_and_seen, remaining_copies,
    same_shanten_draws_with_fixed_melds_and_seen,
};
use crate::shanten::{EffectiveShanten, FixedMeldCount, calculate_shanten_with_fixed_melds};
use crate::tile::TileType;
use crate::tile_counts::TileCounts;

/// 変更前の列挙。牌種ごとに向聴数を計算し直す。
fn reference_drawable_tiles(
    counts: &TileCounts,
    fixed_meld_count: FixedMeldCount,
    additional_seen: &[u8; TileType::COUNT],
) -> Vec<(TileType, u8, EffectiveShanten)> {
    let mut tiles = Vec::new();
    for tile in TileType::all() {
        let remaining = remaining_copies(counts, additional_seen, tile);
        if remaining == 0 {
            continue;
        }

        let mut drawn = *counts;
        if drawn.try_add(tile).is_err() {
            continue;
        }

        tiles.push((
            tile,
            remaining,
            calculate_shanten_with_fixed_melds(&drawn, fixed_meld_count),
        ));
    }
    tiles
}

fn hand_label(counts: &TileCounts) -> String {
    let mut label = String::new();
    for tile in TileType::all() {
        for _ in 0..counts.count(tile) {
            label.push_str(&tile.to_mjai_string());
        }
    }
    label
}

#[track_caller]
fn assert_matches_reference(
    counts: &TileCounts,
    fixed_meld_count: FixedMeldCount,
    additional_seen: &[u8; TileType::COUNT],
) {
    let label = format!(
        "hand={} fixed_meld_count={} seen={additional_seen:?}",
        hand_label(counts),
        fixed_meld_count.get()
    );

    let current = calculate_shanten_with_fixed_melds(counts, fixed_meld_count);
    let current_min = current.min();
    let drawable = reference_drawable_tiles(counts, fixed_meld_count, additional_seen);

    let acceptance =
        calculate_acceptance_with_fixed_melds_and_seen(counts, fixed_meld_count, additional_seen);
    assert_eq!(acceptance.current, current, "current {label}");
    assert_eq!(
        acceptance
            .tiles
            .iter()
            .map(|entry| (entry.tile, entry.remaining, entry.shanten_after_draw))
            .collect::<Vec<_>>(),
        drawable
            .iter()
            .copied()
            .filter(|(_, _, after_draw)| after_draw.min() < current_min)
            .collect::<Vec<_>>(),
        "acceptance {label}"
    );

    let same_shanten =
        same_shanten_draws_with_fixed_melds_and_seen(counts, fixed_meld_count, additional_seen);
    assert_eq!(
        same_shanten,
        drawable
            .iter()
            .copied()
            .filter(|(_, _, after_draw)| after_draw.min() == current_min)
            .map(|(tile, remaining, shanten_after_draw)| DrawableTile {
                tile,
                remaining,
                shanten_after_draw,
            })
            .collect::<Vec<_>>(),
        "same shanten draws {label}"
    );
}

fn sample_count(debug_count: usize, release_count: usize) -> usize {
    if cfg!(debug_assertions) {
        debug_count
    } else {
        release_count
    }
}

#[test]
fn matches_the_per_tile_calculation_for_random_states() {
    let mut random = 0x9e37_79b9_7f4a_7c15u64;
    let mut next = move || {
        random ^= random << 13;
        random ^= random >> 7;
        random ^= random << 17;
        random
    };

    // 副露済み面子数ごとに、その面子数で手に残る枚数 (13枚・14枚) と少牌数を通す。
    for fixed in 0..=FixedMeldCount::MAX {
        let fixed_meld_count = FixedMeldCount::new(fixed).expect("0..=4 は正当な副露済み面子数");
        let concealed = u32::from(13 - fixed * 3);

        for tiles in 0..=concealed + 1 {
            for sample in 0..sample_count(6, 400) {
                let mut counts = TileCounts::new();
                let mut drawn = 0;
                while drawn < tiles {
                    let tile = TileType::new((next() % TileType::COUNT as u64) as u8)
                        .expect("0..34 は正当な牌種");
                    if counts.try_add(tile).is_ok() {
                        drawn += 1;
                    }
                }

                // 見え牌なしと、手牌以外に見えている枚数がある場合の両方を通す。
                let mut additional_seen = [0u8; TileType::COUNT];
                assert_matches_reference(&counts, fixed_meld_count, &additional_seen);

                if sample % 2 == 0 {
                    continue;
                }

                for tile in TileType::all() {
                    let room = 4 - counts.count(tile);
                    additional_seen[tile.index()] = (next() % u64::from(room + 1)) as u8;
                }
                assert_matches_reference(&counts, fixed_meld_count, &additional_seen);
            }
        }
    }
}

/// 4枚とも見えている牌種や、手牌に4枚持っている牌種を必ず含む牌姿。
#[test]
fn matches_the_per_tile_calculation_when_tiles_are_exhausted() {
    let hands: [&[&str]; 4] = [
        &[
            "1m", "1m", "1m", "1m", "2m", "3m", "4m", "5m", "6m", "7m", "8m", "9m", "1p",
        ],
        &[
            "E", "E", "E", "E", "S", "S", "S", "S", "W", "W", "W", "W", "N",
        ],
        &[
            "1m", "1m", "2m", "2m", "3m", "3m", "4m", "4m", "5m", "5m", "6m", "6m", "7m",
        ],
        &[
            "1m", "9m", "1p", "9p", "1s", "9s", "E", "S", "W", "N", "P", "F", "C",
        ],
    ];

    for hand in hands {
        let counts = TileCounts::from_tile_types(
            hand.iter()
                .map(|name| TileType::from_mjai_type_str(name).expect("正当な牌種表記")),
        );

        for fixed in 0..=FixedMeldCount::MAX {
            let fixed_meld_count =
                FixedMeldCount::new(fixed).expect("0..=4 は正当な副露済み面子数");

            let mut additional_seen = [0u8; TileType::COUNT];
            assert_matches_reference(&counts, fixed_meld_count, &additional_seen);

            // 残枚数 0 の牌種を作り、列挙から落ちることまで一致させる。
            for tile in TileType::all() {
                additional_seen[tile.index()] = 4 - counts.count(tile);
            }
            assert_matches_reference(&counts, fixed_meld_count, &additional_seen);
        }
    }
}
