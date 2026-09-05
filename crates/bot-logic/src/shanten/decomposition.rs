//! 通常形向聴数を色ごとの分解から厳密に求める。
//!
//! 面子も搭子も色を跨がないので、手牌は萬子・筒子・索子・字牌の4群へ完全に分割できる。各群から
//! 取り出せる (面子数, 雀頭の有無) ごとの搭子最大数だけが向聴数に効き、群同士は牌を共有しない。
//! そこで群ごとの取り出し方を牌姿だけで決まる要約 ([`BlockProfile`]) にして memo し、手牌ごとの
//! 計算は要約4つの組み合わせだけにする。
//!
//! 群ごとの列挙は牌姿だけで決まるので、副露済み面子数は組み合わせ側で足す。
//!
//! 1牌だけ加えた牌姿は元の牌姿と3群を共有するので、加えた牌が属する群の要約だけを取り直し、
//! 残り3群の畳み込み結果を使い回す ([`standard_shanten_with_after_draws`])。

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

/// 面子も搭子も跨がない4群 (萬子・筒子・索子・字牌) の範囲と、順子を作れるかどうか。
const GROUPS: [(usize, usize, bool); 4] = [
    (0, 9, true),
    (9, 18, true),
    (18, 27, true),
    (27, TileType::COUNT, false),
];

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

    /// どの群からも何も取っていない状態。群を足し合わせる畳み込みの単位元。
    const NOTHING_TAKEN: Self = {
        let mut max_partials = [[INFEASIBLE; 2]; MAX_BLOCKS + 1];
        max_partials[0][0] = 0;
        Self { max_partials }
    };

    /// 牌を共有しない2つの取り出し方を足し合わせる。
    ///
    /// 面子数は4を超えても、搭子数は `4 - 面子数` を超えても向聴数に効かないため、いずれも途中で
    /// 頭打ちにする。頭打ちにした組み合わせと同じ値を持つ「面子も搭子も4個以内」の取り出し方は
    /// 必ず存在する (余分な面子や搭子を捨てるだけ) ので、これで過大評価は起きない。
    ///
    /// 雀頭は手牌全体で1つだけなので、両側が雀頭を取った組み合わせは作らない。どちらの側が雀頭を
    /// 出しても同じ枠へ入れるので、群をどの順で足しても同じ結果になる。
    ///
    /// 面子数を減らせば必ず取り出せる (余った面子を崩すだけ) ので、雀頭なしで取り出せない面子数
    /// より先は両側とも見なくてよい。この性質は足し合わせた結果でも保たれる。
    fn merge(&self, other: &Self) -> Self {
        let mut merged = Self::EMPTY;

        for (melds, by_pair) in self.max_partials.iter().enumerate() {
            if by_pair[0] < 0 {
                break;
            }

            for (added, added_by_pair) in other.max_partials.iter().enumerate() {
                let without_pair = added_by_pair[0];
                if without_pair < 0 {
                    break;
                }
                let with_pair = added_by_pair[1];
                let total = (melds + added).min(MAX_BLOCKS);

                for (pair, &partials) in by_pair.iter().enumerate() {
                    if partials < 0 {
                        continue;
                    }

                    let combined = (partials + without_pair).min(MAX_BLOCKS as i8);
                    if combined > merged.max_partials[total][pair] {
                        merged.max_partials[total][pair] = combined;
                    }

                    if pair == 0 && with_pair >= 0 {
                        let combined = (partials + with_pair).min(MAX_BLOCKS as i8);
                        if combined > merged.max_partials[total][1] {
                            merged.max_partials[total][1] = combined;
                        }
                    }
                }
            }
        }

        merged
    }

    /// 手牌全体の取り出し方から向聴数を求める。副露済み面子数は面子として数える。
    fn shanten(&self, fixed_melds: u8) -> i8 {
        let mut value = 0i8;
        for (melds, by_pair) in self.max_partials.iter().enumerate() {
            let total = (usize::from(fixed_melds) + melds).min(MAX_BLOCKS) as i8;
            for (pair, &partials) in by_pair.iter().enumerate() {
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
    PROFILE_TABLES.with_borrow_mut(|tables| {
        let mut combined = BlockProfile::NOTHING_TAKEN;
        for &(start, end, suited) in &GROUPS {
            combined = combined.merge(&tables.profile(&counts[start..end], suited));
        }
        combined.shanten(fixed_melds)
    })
}

/// 現在の牌姿と、牌種を1枚ずつ加えた牌姿の通常形向聴数をまとめて求める。
///
/// 1牌加えても要約が変わるのはその牌が属する1群だけなので、群ごとに「自分以外の3群の畳み込み」を
/// 一度だけ作り、牌ごとには取り直した1群を足すだけにする。牌ごとに4群を組み立て直した場合と同じ
/// 畳み込みを同じ順序制約なしで得るための構造で、向聴数そのものは [`standard_shanten`] と同じ。
///
/// `after_draw[牌種]` は5枚目が無い (既に4枚持っている) 牌種では [`None`]。
pub(super) fn standard_shanten_with_after_draws(
    counts: &[u8; TileType::COUNT],
    fixed_melds: u8,
    after_draw: &mut [Option<i8>; TileType::COUNT],
) -> i8 {
    PROFILE_TABLES.with_borrow_mut(|tables| {
        let base: [BlockProfile; GROUPS.len()] = std::array::from_fn(|group| {
            let (start, end, suited) = GROUPS[group];
            tables.profile(&counts[start..end], suited)
        });

        // prefix[i] は群 0..i の、suffix[i] は群 i.. の畳み込み。
        let mut prefix = [BlockProfile::NOTHING_TAKEN; GROUPS.len() + 1];
        for group in 0..GROUPS.len() {
            prefix[group + 1] = prefix[group].merge(&base[group]);
        }
        let mut suffix = [BlockProfile::NOTHING_TAKEN; GROUPS.len() + 1];
        for group in (0..GROUPS.len()).rev() {
            suffix[group] = suffix[group + 1].merge(&base[group]);
        }

        let mut working = [0u8; SUIT_TILES];
        for (group, &(start, end, suited)) in GROUPS.iter().enumerate() {
            let others = prefix[group].merge(&suffix[group + 1]);
            let width = end - start;
            working[..width].copy_from_slice(&counts[start..end]);

            for offset in 0..width {
                let tile = start + offset;
                if counts[tile] >= MAX_COPIES {
                    after_draw[tile] = None;
                    continue;
                }

                working[offset] += 1;
                let drawn = tables.profile(&working[..width], suited);
                working[offset] -= 1;

                after_draw[tile] = Some(others.merge(&drawn).shanten(fixed_melds));
            }
        }

        prefix[GROUPS.len()].shanten(fixed_melds)
    })
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
