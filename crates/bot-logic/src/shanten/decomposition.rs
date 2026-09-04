//! 通常形向聴数を色ごとの分解から厳密に求める。
//!
//! 面子も搭子も色を跨がないので、手牌は萬子・筒子・索子・字牌の4群へ完全に分割できる。各群から
//! 取り出せる (面子数, 雀頭の有無) ごとの搭子最大数だけが向聴数に効き、群同士は牌を共有しない。
//! そこで群ごとの取り出し方を牌姿だけで決まる要約 ([`BlockProfile`]) にして memo し、手牌ごとの
//! 計算は要約4つの組み合わせだけにする。
//!
//! 群ごとの列挙は牌姿だけで決まるので、副露済み面子数は組み合わせ側で足す。

use crate::tile::TileType;
use std::cell::RefCell;

const SUIT_TILES: usize = 9;
const HONOR_TILES: usize = 7;
const MAX_COPIES: u8 = 4;

// 面子数も搭子数も5個目以降は向聴数に効かないので、群ごとの要約はここで打ち切る。
const MAX_BLOCKS: usize = 4;

const INFEASIBLE: i8 = -1;

// 要約 1 件分の符号化。3 bit × 10 枠 + 計算済み flag。
const FIELD_BITS: u32 = 3;
const FIELD_MASK: u32 = 7;
const FIELD_INFEASIBLE: u32 = 7;
const COMPUTED_FLAG: u32 = 1 << 30;

const SUIT_PATTERNS: usize = 5usize.pow(SUIT_TILES as u32);
const HONOR_PATTERNS: usize = 5usize.pow(HONOR_TILES as u32);

/// 1群から取り出せる面子・搭子・雀頭の組み合わせの要約。
///
/// `max_partials[面子数][雀頭を取ったか]` は、その面子数と雀頭の取り方で同時に取れる搭子の最大数。
/// 取り出せない組み合わせは [`INFEASIBLE`]。向聴数は搭子数について単調非減少なので、(面子数,
/// 雀頭) ごとの最大値だけ持てば足りる。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct BlockProfile {
    max_partials: [[i8; 2]; MAX_BLOCKS + 1],
}

impl BlockProfile {
    const EMPTY: Self = Self {
        max_partials: [[INFEASIBLE; 2]; MAX_BLOCKS + 1],
    };

    fn record(&mut self, taken: Extraction) {
        let slot = &mut self.max_partials[taken.melds][usize::from(taken.has_pair)];
        if taken.partials > *slot {
            *slot = taken.partials;
        }
    }

    fn field_shift(melds: usize, pair: usize) -> u32 {
        FIELD_BITS * (melds * 2 + pair) as u32
    }

    fn encode(self) -> u32 {
        let mut word = COMPUTED_FLAG;
        for (melds, by_pair) in self.max_partials.iter().enumerate() {
            for (pair, &partials) in by_pair.iter().enumerate() {
                let bits = if partials < 0 {
                    FIELD_INFEASIBLE
                } else {
                    partials as u32
                };
                word |= bits << Self::field_shift(melds, pair);
            }
        }
        word
    }

    fn decode(word: u32) -> Self {
        let mut profile = Self::EMPTY;
        for (melds, by_pair) in profile.max_partials.iter_mut().enumerate() {
            for (pair, partials) in by_pair.iter_mut().enumerate() {
                let bits = (word >> Self::field_shift(melds, pair)) & FIELD_MASK;
                *partials = if bits == FIELD_INFEASIBLE {
                    INFEASIBLE
                } else {
                    bits as i8
                };
            }
        }
        profile
    }
}

/// 群ごとの要約を牌姿で引く表。
///
/// 表の中身は牌姿だけで決まる純関数の値なので、いつ埋めても向聴数は変わらない。使う牌姿しか
/// 埋めないように、引いたときに計算して書き戻す。
///
/// 添字は牌姿の5進表現そのもので、数牌9種で 5^9、字牌7種で 5^7 枠。要約1件は 4 byte なので
/// 合わせて約 8 MB を thread ごとに持つ。置き換え前の探索 memo (34 byte の key を 1<<17 件)
/// と同じ桁で、hash も衝突処理も要らない代わりの容量。
struct ProfileTables {
    suit: Vec<u32>,
    honor: Vec<u32>,
}

impl ProfileTables {
    fn new() -> Self {
        Self {
            suit: vec![0; SUIT_PATTERNS],
            honor: vec![0; HONOR_PATTERNS],
        }
    }

    fn profile(&mut self, counts: &[u8], suited: bool) -> BlockProfile {
        let table = if suited {
            &mut self.suit
        } else {
            &mut self.honor
        };

        // 5枚以上持つ牌姿は表の添字に収まらないので、その場でだけ計算する。
        let Some(index) = pattern_index(counts) else {
            return compute_profile(counts, suited);
        };

        let cached = table[index];
        if cached & COMPUTED_FLAG != 0 {
            return BlockProfile::decode(cached);
        }

        let profile = compute_profile(counts, suited);
        table[index] = profile.encode();
        profile
    }
}

thread_local! {
    static PROFILE_TABLES: RefCell<ProfileTables> = RefCell::new(ProfileTables::new());
}

fn pattern_index(counts: &[u8]) -> Option<usize> {
    let mut index = 0usize;
    for &count in counts.iter().rev() {
        if count > MAX_COPIES {
            return None;
        }
        index = index * 5 + usize::from(count);
    }
    Some(index)
}

pub(super) fn standard_shanten(counts: &[u8; TileType::COUNT], fixed_melds: u8) -> i8 {
    let profiles = PROFILE_TABLES.with_borrow_mut(|tables| {
        [
            tables.profile(&counts[0..9], true),
            tables.profile(&counts[9..18], true),
            tables.profile(&counts[18..27], true),
            tables.profile(&counts[27..34], false),
        ]
    });

    combine(&profiles, fixed_melds)
}

/// 群ごとの要約を足し合わせて向聴数にする。
///
/// 面子数は4を超えても、搭子数は `4 - 面子数` を超えても向聴数に効かないため、いずれも途中で
/// 頭打ちにする。頭打ちにした組み合わせと同じ値を持つ「面子も搭子も4個以内」の取り出し方は必ず
/// 存在する (余分な面子や搭子を捨てるだけ) ので、これで過大評価は起きない。
fn combine(profiles: &[BlockProfile; 4], fixed_melds: u8) -> i8 {
    // best[面子数][雀頭を取ったか] = そこまでの群から取れる搭子の最大数。
    let mut best = [[INFEASIBLE; 2]; MAX_BLOCKS + 1];
    best[0][0] = 0;
    let mut reach = 0usize;

    for profile in profiles {
        let mut next = [[INFEASIBLE; 2]; MAX_BLOCKS + 1];

        for (added, by_pair) in profile.max_partials.iter().enumerate() {
            let without_pair = by_pair[0];
            // 面子数を減らせば必ず取り出せるので、取り出せない面子数より先は見なくてよい。
            if without_pair < 0 {
                break;
            }
            let with_pair = by_pair[1];

            for (melds, carried) in best.iter().enumerate().take(reach + 1) {
                let total = (melds + added).min(MAX_BLOCKS);

                for (pair, &partials) in carried.iter().enumerate() {
                    if partials < 0 {
                        continue;
                    }

                    let combined = (partials + without_pair).min(MAX_BLOCKS as i8);
                    if combined > next[total][pair] {
                        next[total][pair] = combined;
                    }

                    if pair == 0 && with_pair >= 0 {
                        let combined = (partials + with_pair).min(MAX_BLOCKS as i8);
                        if combined > next[total][1] {
                            next[total][1] = combined;
                        }
                    }
                }
            }
        }

        best = next;
        reach = best
            .iter()
            .rposition(|carried| carried.iter().any(|&partials| partials >= 0))
            .unwrap_or(0);
    }

    let mut value = 0i8;
    for (melds, carried) in best.iter().enumerate() {
        let total = (usize::from(fixed_melds) + melds).min(MAX_BLOCKS) as i8;
        for (pair, &partials) in carried.iter().enumerate() {
            if partials < 0 {
                continue;
            }
            let candidate = 2 * total + partials.min(MAX_BLOCKS as i8 - total) + pair as i8;
            if candidate > value {
                value = candidate;
            }
        }
    }

    8 - value
}

fn compute_profile(counts: &[u8], suited: bool) -> BlockProfile {
    let mut working = [0u8; SUIT_TILES];
    working[..counts.len()].copy_from_slice(counts);

    let mut profile = BlockProfile::EMPTY;
    explore(
        &mut working[..counts.len()],
        0,
        suited,
        Extraction::NONE,
        &mut profile,
    );
    profile
}

/// 1群から取り出し済みのブロック。
#[derive(Debug, Clone, Copy)]
struct Extraction {
    melds: usize,
    partials: i8,
    has_pair: bool,
}

impl Extraction {
    const NONE: Self = Self {
        melds: 0,
        partials: 0,
        has_pair: false,
    };

    fn with_meld(self) -> Self {
        Self {
            melds: self.melds + 1,
            ..self
        }
    }

    fn with_partial(self) -> Self {
        Self {
            partials: self.partials + 1,
            ..self
        }
    }

    fn with_pair_head(self) -> Self {
        Self {
            has_pair: true,
            ..self
        }
    }
}

/// 1群の牌姿から取り出せる (面子数, 搭子数, 雀頭) を列挙する。
///
/// 枝は置き換え前の探索と同じ「刻子 → 順子 → 雀頭 → 対子搭子 → 両面/辺張 → 嵌張 → 1牌除去」で、
/// 常に残っている最小の牌から始める。どのブロックも最小の牌を含む形なので、この順で列挙すれば
/// 分解を取りこぼさない。
fn explore(
    counts: &mut [u8],
    from: usize,
    suited: bool,
    taken: Extraction,
    profile: &mut BlockProfile,
) {
    profile.record(taken);

    let Some(index) = (from..counts.len()).find(|&index| counts[index] > 0) else {
        return;
    };

    let mut branch = |counts: &mut [u8], removed: &[usize], next: Extraction| {
        for offset in removed {
            counts[index + offset] -= 1;
        }
        explore(counts, index, suited, next, profile);
        for offset in removed {
            counts[index + offset] += 1;
        }
    };

    if taken.melds < MAX_BLOCKS {
        if counts[index] >= 3 {
            branch(counts, &[0, 0, 0], taken.with_meld());
        }
        if suited && index + 2 < counts.len() && counts[index + 1] >= 1 && counts[index + 2] >= 1 {
            branch(counts, &[0, 1, 2], taken.with_meld());
        }
    }

    if !taken.has_pair && counts[index] >= 2 {
        branch(counts, &[0, 0], taken.with_pair_head());
    }

    if taken.partials < MAX_BLOCKS as i8 {
        if counts[index] >= 2 {
            branch(counts, &[0, 0], taken.with_partial());
        }
        if suited && index + 1 < counts.len() && counts[index + 1] >= 1 {
            branch(counts, &[0, 1], taken.with_partial());
        }
        if suited && index + 2 < counts.len() && counts[index + 2] >= 1 {
            branch(counts, &[0, 2], taken.with_partial());
        }
    }

    branch(counts, &[0], taken);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encode_round_trips_every_field() {
        let mut profile = BlockProfile::EMPTY;
        profile.max_partials = [[0, 4], [1, 3], [2, INFEASIBLE], [3, 1], [4, INFEASIBLE]];
        assert_eq!(BlockProfile::decode(profile.encode()), profile);
    }

    #[test]
    fn pattern_index_is_unique_per_pattern() {
        assert_eq!(pattern_index(&[0; SUIT_TILES]), Some(0));
        assert_eq!(pattern_index(&[1, 0, 0, 0, 0, 0, 0, 0, 0]), Some(1));
        assert_eq!(pattern_index(&[0, 1, 0, 0, 0, 0, 0, 0, 0]), Some(5));
        assert_eq!(pattern_index(&[4; SUIT_TILES]), Some(SUIT_PATTERNS - 1));
        assert_eq!(pattern_index(&[5, 0, 0, 0, 0, 0, 0, 0, 0]), None);
    }

    fn cached_profile(counts: &[u8], suited: bool) -> BlockProfile {
        PROFILE_TABLES.with_borrow_mut(|tables| tables.profile(counts, suited))
    }

    #[test]
    fn table_lookup_matches_direct_computation() {
        let pattern = [1u8, 1, 1, 0, 2, 0, 0, 3, 0];
        assert_eq!(
            cached_profile(&pattern, true),
            compute_profile(&pattern, true)
        );
        assert_eq!(
            cached_profile(&pattern, true),
            compute_profile(&pattern, true)
        );

        let honors = [2u8, 0, 3, 0, 1, 0, 0];
        assert_eq!(
            cached_profile(&honors, false),
            compute_profile(&honors, false)
        );
    }

    #[test]
    fn honor_group_has_no_sequence_or_taatsu() {
        // 字牌は隣も跨ぎもないので、単騎2枚から搭子は取れない。
        let honors = [1u8, 1, 0, 0, 0, 0, 0];
        let profile = compute_profile(&honors, false);
        assert_eq!(profile.max_partials[0][0], 0);
        assert_eq!(profile.max_partials[0][1], INFEASIBLE);
    }

    #[test]
    fn suit_group_allows_sequence_and_taatsu() {
        let suit = [1u8, 1, 1, 0, 0, 0, 0, 0, 0];
        let profile = compute_profile(&suit, true);
        assert_eq!(profile.max_partials[1][0], 0);
        assert_eq!(profile.max_partials[0][0], 1);
    }
}
