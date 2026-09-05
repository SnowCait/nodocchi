//! まとめて求めたツモ後向聴数 ([`calculate_shanten_with_after_draws`]) が、牌種ごとに
//! [`calculate_shanten_with_fixed_melds`] を呼び直した場合と完全に一致することを確かめる差分検証。
//!
//! 比較対象は既存の向聴数計算そのもので、ここに向聴数の規則は持たない。副露済み面子数は毎回
//! 0..=4 の全てを比較する。
//!
//! 手牌は「1色だけ」「字牌だけ」「少牌」といった数え切れる範囲を全列挙し、それ以外は固定 seed の
//! 擬似乱数で作る。debug build では全列挙が現実的な時間に収まらないため間引く。

use super::{
    FixedMeldCount, calculate_shanten_with_after_draws, calculate_shanten_with_fixed_melds,
};
use crate::tile::TileType;
use crate::tile_counts::TileCounts;

const SUIT_TILES: usize = 9;
const HONOR_TILES: usize = 7;
const MAX_HAND_TILES: u32 = 14;

fn stride(debug_stride: usize) -> usize {
    if cfg!(debug_assertions) {
        debug_stride
    } else {
        1
    }
}

fn sample_count(debug_count: usize, release_count: usize) -> usize {
    if cfg!(debug_assertions) {
        debug_count
    } else {
        release_count
    }
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

/// まとめた結果と、牌種ごとに呼び直した結果を突き合わせる。
#[track_caller]
fn assert_matches_individual_calls(counts: &TileCounts) {
    for fixed in 0..=FixedMeldCount::MAX {
        let fixed_meld_count = FixedMeldCount::new(fixed).expect("0..=4 は正当な副露済み面子数");
        let batch = calculate_shanten_with_after_draws(counts, fixed_meld_count);

        assert_eq!(
            batch.current,
            calculate_shanten_with_fixed_melds(counts, fixed_meld_count),
            "current hand={} fixed_meld_count={fixed}",
            hand_label(counts)
        );

        for tile in TileType::all() {
            let mut drawn = *counts;
            let expected = drawn
                .try_add(tile)
                .is_ok()
                .then(|| calculate_shanten_with_fixed_melds(&drawn, fixed_meld_count));

            assert_eq!(
                batch.after_draw[tile.index()],
                expected,
                "after draw {} hand={} fixed_meld_count={fixed}",
                tile.to_mjai_string(),
                hand_label(counts)
            );
        }
    }
}

fn decode_pattern(mut index: usize, tiles: usize) -> Option<Vec<u8>> {
    let mut pattern = Vec::with_capacity(tiles);
    let mut total = 0u32;
    for _ in 0..tiles {
        let count = (index % 5) as u8;
        index /= 5;
        total += u32::from(count);
        pattern.push(count);
    }
    (total <= MAX_HAND_TILES).then_some(pattern)
}

fn hand_from_group(pattern: &[u8], offset: usize) -> TileCounts {
    let mut counts = [0u8; TileType::COUNT];
    counts[offset..offset + pattern.len()].copy_from_slice(pattern);
    TileCounts::try_from(counts).expect("各牌種4枚以下")
}

#[test]
fn matches_individual_calls_for_every_single_suit_pattern() {
    let patterns = 5usize.pow(SUIT_TILES as u32);
    for index in (0..patterns).step_by(stride(97)) {
        let Some(pattern) = decode_pattern(index, SUIT_TILES) else {
            continue;
        };
        assert_matches_individual_calls(&hand_from_group(&pattern, 0));
    }
}

#[test]
fn matches_individual_calls_for_every_honor_pattern() {
    let patterns = 5usize.pow(HONOR_TILES as u32);
    for index in (0..patterns).step_by(stride(11)) {
        let Some(pattern) = decode_pattern(index, HONOR_TILES) else {
            continue;
        };
        assert_matches_individual_calls(&hand_from_group(&pattern, 27));
    }
}

/// 4枚持ちの牌種を必ず含む牌姿。5枚目が無い牌種が [`None`] になる経路を通す。
#[test]
fn matches_individual_calls_when_a_tile_type_is_already_full() {
    for tile in TileType::all() {
        for other in TileType::all().filter(|other| *other != tile) {
            let mut counts = TileCounts::new();
            for _ in 0..4 {
                counts.add(tile);
                counts.add(other);
            }
            assert_matches_individual_calls(&counts);
        }
    }
}

/// 対子・刻子・槓子・単騎が混ざる牌姿を、手牌枚数を変えながら比較する。
#[test]
fn matches_individual_calls_for_random_hands() {
    let mut random = 0x2545_f491_4f6c_dd1du64;
    let mut next = move || {
        random ^= random << 13;
        random ^= random >> 7;
        random ^= random << 17;
        random
    };

    for tiles in 0..=MAX_HAND_TILES {
        for _ in 0..sample_count(40, 900) {
            let mut counts = TileCounts::new();
            let mut drawn = 0;
            while drawn < tiles {
                let tile = TileType::new((next() % TileType::COUNT as u64) as u8)
                    .expect("0..34 は正当な牌種");
                if counts.try_add(tile).is_ok() {
                    drawn += 1;
                }
            }
            assert_matches_individual_calls(&counts);
        }
    }
}

/// 七対子・国士無双が最小になる牌姿を明示的に通す。
#[test]
fn matches_individual_calls_for_chiitoitsu_and_kokushi_shapes() {
    let hands: [&[&str]; 6] = [
        &[
            "1m", "1m", "3m", "3m", "5m", "5m", "7m", "7m", "9m", "9m", "1p", "1p", "3p",
        ],
        &[
            "1m", "1m", "3m", "3m", "5m", "5m", "7m", "7m", "9m", "9m", "1p", "1p", "3p", "3p",
        ],
        &[
            "1m", "9m", "1p", "9p", "1s", "9s", "E", "S", "W", "N", "P", "F", "C",
        ],
        &[
            "1m", "9m", "1p", "9p", "1s", "9s", "E", "S", "W", "N", "P", "F", "C", "C",
        ],
        &[
            "1m", "1m", "1m", "9m", "9m", "1p", "9p", "1s", "9s", "E", "S", "W", "N",
        ],
        &["1m", "1m", "1m", "1m", "9m", "9m", "9m", "9m"],
    ];

    for hand in hands {
        let counts = TileCounts::from_tile_types(
            hand.iter()
                .map(|name| TileType::from_mjai_type_str(name).expect("正当な牌種表記")),
        );
        assert_matches_individual_calls(&counts);
    }
}
