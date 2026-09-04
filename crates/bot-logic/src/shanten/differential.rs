//! 色ごとの分解による通常形向聴数が、置き換え前の探索 ([`super::reference`]) と完全に一致する
//! ことを確かめる差分検証。
//!
//! 入力空間を数え切れる範囲 (1色だけの牌姿・字牌だけの牌姿・0〜4枚の手牌) は全列挙し、それ以外は
//! 固定 seed の擬似乱数で作った手牌で埋める。副露済み面子数は毎回 0..=4 の全てを比較する。
//!
//! debug build では全列挙が現実的な時間に収まらないため間引く。全列挙は `cargo test --release`
//! で走る。

use super::reference;
use crate::shanten::{FixedMeldCount, standard_shanten_with_fixed_melds};
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

#[track_caller]
fn assert_matches_reference(counts: &TileCounts) {
    for fixed in 0..=FixedMeldCount::MAX {
        let fixed_meld_count = FixedMeldCount::new(fixed).expect("0..=4 は正当な副露済み面子数");
        let actual = standard_shanten_with_fixed_melds(counts, fixed_meld_count);
        let expected = reference::standard_shanten_with_fixed_melds(counts, fixed_meld_count);
        assert_eq!(
            actual,
            expected,
            "hand={:?} fixed_meld_count={fixed}",
            hand_label(counts)
        );
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
fn matches_reference_for_every_single_suit_pattern() {
    let patterns = 5usize.pow(SUIT_TILES as u32);
    let step = stride(23);
    let mut checked = 0usize;

    for index in (0..patterns).step_by(step) {
        let Some(pattern) = decode_pattern(index, SUIT_TILES) else {
            continue;
        };
        // 色を跨がない以上、萬子で確かめれば筒子・索子も同じ表を引く。
        assert_matches_reference(&hand_from_group(&pattern, 0));
        checked += 1;
    }

    assert!(checked > 10_000, "検証した牌姿が少なすぎる: {checked}");
}

#[test]
fn matches_reference_for_every_suit_pattern_in_each_suit() {
    // 表は色ごとに引き分けられているので、同じ牌姿を3色それぞれで確かめる。
    let patterns = 5usize.pow(SUIT_TILES as u32);
    let step = stride(97) * 13;

    for index in (0..patterns).step_by(step) {
        let Some(pattern) = decode_pattern(index, SUIT_TILES) else {
            continue;
        };
        for offset in [0, 9, 18] {
            assert_matches_reference(&hand_from_group(&pattern, offset));
        }
    }
}

#[test]
fn matches_reference_for_every_honor_pattern() {
    let patterns = 5usize.pow(HONOR_TILES as u32);
    let step = stride(5);
    let mut checked = 0usize;

    for index in (0..patterns).step_by(step) {
        let Some(pattern) = decode_pattern(index, HONOR_TILES) else {
            continue;
        };
        assert_matches_reference(&hand_from_group(&pattern, 27));
        checked += 1;
    }

    assert!(checked > 5_000, "検証した牌姿が少なすぎる: {checked}");
}

#[test]
fn matches_reference_for_every_hand_up_to_four_tiles() {
    // 2手先評価の途中に現れる少牌数の手牌を全列挙する。
    let mut counts = [0u8; TileType::COUNT];
    let mut checked = 0usize;
    enumerate_small_hands(&mut counts, 0, 4, &mut checked);
    assert!(checked > 50_000, "検証した手牌が少なすぎる: {checked}");
}

fn enumerate_small_hands(
    counts: &mut [u8; TileType::COUNT],
    index: usize,
    remaining: u8,
    checked: &mut usize,
) {
    if index == TileType::COUNT {
        let hand = TileCounts::try_from(*counts).expect("各牌種4枚以下");
        // debug build では組み合わせが多すぎるので間引く。
        if checked.is_multiple_of(stride(9)) {
            assert_matches_reference(&hand);
        }
        *checked += 1;
        return;
    }

    for count in 0..=remaining.min(4) {
        counts[index] = count;
        enumerate_small_hands(counts, index + 1, remaining - count, checked);
    }
    counts[index] = 0;
}

struct Rng(u64);

impl Rng {
    fn new(seed: u64) -> Self {
        Self(seed)
    }

    fn next_u64(&mut self) -> u64 {
        let mut value = self.0;
        value ^= value << 13;
        value ^= value >> 7;
        value ^= value << 17;
        self.0 = value;
        value
    }

    fn below(&mut self, bound: usize) -> usize {
        (self.next_u64() % bound as u64) as usize
    }
}

fn random_hand(rng: &mut Rng, size: usize) -> TileCounts {
    let mut counts = [0u8; TileType::COUNT];
    let mut placed = 0;
    while placed < size {
        let index = rng.below(TileType::COUNT);
        if counts[index] < 4 {
            counts[index] += 1;
            placed += 1;
        }
    }
    TileCounts::try_from(counts).expect("各牌種4枚以下")
}

/// 面子・搭子から組み立てた、向聴数の低い手牌。
///
/// 一様乱数だけでは聴牌形や和了形がほとんど出ないため、ブロックを積んだ手牌も混ぜる。
fn structured_hand(rng: &mut Rng, size: usize) -> TileCounts {
    let mut counts = [0u8; TileType::COUNT];
    let mut placed = 0usize;

    while placed < size {
        let block: &[usize] = match rng.below(6) {
            0 => &[0, 1, 2],
            1 => &[0, 0, 0],
            2 => &[0, 0],
            3 => &[0, 1],
            4 => &[0, 2],
            _ => &[0],
        };
        if placed + block.len() > size {
            if placed + 1 > size {
                break;
            }
            let index = rng.below(TileType::COUNT);
            if counts[index] < 4 {
                counts[index] += 1;
                placed += 1;
            }
            continue;
        }

        let start = if block.len() == 1 || block == [0, 0] || block == [0, 0, 0] {
            rng.below(TileType::COUNT)
        } else {
            // 数牌の並びを使うブロックは色を跨がない開始位置だけを選ぶ。
            let suit = rng.below(3) * 9;
            suit + rng.below(9 - block[block.len() - 1])
        };

        if block
            .iter()
            .all(|offset| counts[start + offset] < 4 && start + offset < TileType::COUNT)
        {
            let mut applied = true;
            let mut used = [0u8; TileType::COUNT];
            for offset in block {
                used[start + offset] += 1;
            }
            for index in 0..TileType::COUNT {
                if counts[index] + used[index] > 4 {
                    applied = false;
                }
            }
            if applied {
                for index in 0..TileType::COUNT {
                    counts[index] += used[index];
                }
                placed += block.len();
            }
        }
    }

    TileCounts::try_from(counts).expect("各牌種4枚以下")
}

#[test]
fn matches_reference_for_random_hands() {
    let mut rng = Rng::new(0x5bd1_e995_1234_5678);
    let samples = sample_count(2_000, 60_000);

    for _ in 0..samples {
        let size = rng.below(15);
        assert_matches_reference(&random_hand(&mut rng, size));
    }
}

#[test]
fn matches_reference_for_random_structured_hands() {
    let mut rng = Rng::new(0x0123_4567_89ab_cdef);
    let samples = sample_count(2_000, 60_000);
    let mut seen = [false; 11];

    for _ in 0..samples {
        let size = rng.below(15);
        let hand = structured_hand(&mut rng, size);
        assert_matches_reference(&hand);

        let shanten = standard_shanten_with_fixed_melds(&hand, FixedMeldCount::NONE);
        seen[usize::try_from(shanten + 1).expect("向聴数は -1 以上")] = true;
    }

    // 和了形・聴牌・1向聴・2向聴以上が検証対象に含まれていることを確かめる。
    for shanten in -1..=2 {
        assert!(
            seen[usize::try_from(shanten + 1).expect("向聴数は -1 以上")],
            "向聴数 {shanten} の手牌が検証されていない"
        );
    }
}

#[test]
fn matches_reference_for_random_thirteen_and_fourteen_tile_hands() {
    let mut rng = Rng::new(0xdead_beef_cafe_0001);
    let samples = sample_count(1_500, 40_000);

    for index in 0..samples {
        let size = if index % 2 == 0 { 13 } else { 14 };
        assert_matches_reference(&random_hand(&mut rng, size));
        assert_matches_reference(&structured_hand(&mut rng, size));
    }
}

#[test]
fn matches_reference_for_fixed_meld_hand_sizes() {
    // 副露済み面子数ごとに、その副露数で成立する手牌枚数を集中的に確かめる。
    let mut rng = Rng::new(0x00c0_ffee_0000_0002);
    let samples = sample_count(500, 15_000);

    for fixed in 0..=FixedMeldCount::MAX {
        let fixed_meld_count = FixedMeldCount::new(fixed).expect("0..=4 は正当な副露済み面子数");
        let concealed = usize::from(13 - 3 * fixed);

        for index in 0..samples {
            let size = if index % 2 == 0 {
                concealed
            } else {
                concealed + 1
            };
            for hand in [random_hand(&mut rng, size), structured_hand(&mut rng, size)] {
                assert_eq!(
                    standard_shanten_with_fixed_melds(&hand, fixed_meld_count),
                    reference::standard_shanten_with_fixed_melds(&hand, fixed_meld_count),
                    "hand={:?} fixed_meld_count={fixed}",
                    hand_label(&hand)
                );
            }
        }
    }
}

#[test]
fn matches_reference_for_lookahead_style_intermediate_hands() {
    // 2手先評価は「1枚加える」「1枚減らす」を繰り返すので、その途中の手牌も比べる。
    let mut rng = Rng::new(0xfeed_face_0000_0003);
    let samples = sample_count(200, 4_000);

    for _ in 0..samples {
        let mut hand = structured_hand(&mut rng, 13);
        assert_matches_reference(&hand);

        for _ in 0..8 {
            let tile = TileType::new(rng.below(TileType::COUNT) as u8).expect("0..34 は正当な牌種");
            if rng.below(2) == 0 {
                let _ = hand.try_add(tile);
            } else {
                let _ = hand.remove(tile);
            }
            assert_matches_reference(&hand);
        }
    }
}
