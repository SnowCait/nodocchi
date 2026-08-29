use std::collections::HashMap;

use bot_logic::TileType;

use super::block_table::{BlockTable, HONOR_TILES, POW5, SUITED_TILES};

// 手牌は面子・雀頭が色をまたがないので、この4群に分けて数える。
pub(super) const GROUP_COUNT: usize = 4;

// 牌種ごとの残枚数 (最大4枚) から k 枚を選ぶ組み合わせ数。
const BINOMIAL: [[u64; 5]; 5] = [
    [1, 0, 0, 0, 0],
    [1, 1, 0, 0, 0],
    [1, 2, 1, 0, 0],
    [1, 3, 3, 1, 0],
    [1, 4, 6, 4, 1],
];

// 七対子の並び。手牌全体で対子6・単騎1でなければ七対子テンパイにならない。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(super) enum ChiitoitsuShape {
    Broken,
    Pairs { pairs: u8, single: Option<u8> },
}

// 群単位で圧縮した隠れ手牌の部分状態。同じ key の牌数ベクタはまとめて数える。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(super) struct GroupClassKey {
    pub(super) size: u8,
    pub(super) complete: bool,
    pub(super) complete_with_pair: bool,
    pub(super) wait_to_complete: u16,
    pub(super) wait_to_complete_with_pair: u16,
    pub(super) chiitoitsu: ChiitoitsuShape,
}

#[derive(Debug, Clone, Copy)]
pub(super) struct GroupClass {
    pub(super) key: GroupClassKey,
    pub(super) weight: u64,
    pub(super) states: u64,
}

#[derive(Clone, Copy)]
pub(super) struct GroupSpec {
    pub(super) base: usize,
    pub(super) tiles: usize,
    table: &'static BlockTable,
}

impl GroupSpec {
    pub(super) fn all() -> [Self; GROUP_COUNT] {
        [
            GroupSpec {
                base: 0,
                tiles: SUITED_TILES,
                table: BlockTable::suited(),
            },
            GroupSpec {
                base: SUITED_TILES,
                tiles: SUITED_TILES,
                table: BlockTable::suited(),
            },
            GroupSpec {
                base: 2 * SUITED_TILES,
                tiles: SUITED_TILES,
                table: BlockTable::suited(),
            },
            GroupSpec {
                base: 3 * SUITED_TILES,
                tiles: HONOR_TILES,
                table: BlockTable::honor(),
            },
        ]
    }
}

// 群1つ分の牌数ベクタを列挙し、target に依存しない class へ畳み込む。返り値の2つめは畳み込む
// 前に列挙した牌数ベクタ数で、計測用。
pub(super) fn enumerate_group_classes(
    spec: GroupSpec,
    remaining: &[u8; TileType::COUNT],
    hand_len: u8,
    chiitoitsu_possible: bool,
) -> (Vec<GroupClass>, u64) {
    let mut enumeration = GroupEnumeration {
        spec,
        remaining,
        hand_len,
        chiitoitsu_possible,
        classes: HashMap::new(),
        visited: 0,
    };
    enumeration.walk(
        0,
        PartialGroup {
            index: 0,
            size: 0,
            weight: 1,
            pairs: 0,
            single: None,
            chiitoitsu_compatible: true,
        },
    );

    let visited = enumeration.visited;
    let classes = enumeration
        .classes
        .into_iter()
        .map(|(key, (weight, states))| GroupClass {
            key,
            weight,
            states,
        })
        .collect();
    (classes, visited)
}

#[derive(Debug, Clone, Copy)]
struct PartialGroup {
    index: usize,
    size: u8,
    weight: u64,
    pairs: u8,
    single: Option<u8>,
    chiitoitsu_compatible: bool,
}

struct GroupEnumeration<'a> {
    spec: GroupSpec,
    remaining: &'a [u8; TileType::COUNT],
    hand_len: u8,
    chiitoitsu_possible: bool,
    classes: HashMap<GroupClassKey, (u64, u64)>,
    visited: u64,
}

impl GroupEnumeration<'_> {
    // 群内の牌種ごとに枚数を選び、牌数ベクタを網羅する。完成状態は作らず、牌数ベクタのまま
    // class へ畳み込むので、同じ牌数ベクタが複数の面子分解を持っていても1状態のままになる。
    fn walk(&mut self, tile: usize, partial: PartialGroup) {
        if tile == self.spec.tiles {
            self.record(partial);
            return;
        }
        let remaining = usize::from(self.remaining[self.spec.base + tile]);
        for (count, &combinations) in BINOMIAL[remaining].iter().take(remaining + 1).enumerate() {
            let size = partial.size + count as u8;
            if size > self.hand_len {
                break;
            }
            self.walk(
                tile + 1,
                PartialGroup {
                    index: partial.index + count * POW5[tile],
                    size,
                    weight: partial.weight * combinations,
                    pairs: partial.pairs + u8::from(count == 2),
                    single: if count == 1 {
                        partial.single.or(Some(tile as u8))
                    } else {
                        partial.single
                    },
                    chiitoitsu_compatible: partial.chiitoitsu_compatible
                        && count <= 2
                        && !(count == 1 && partial.single.is_some()),
                },
            );
        }
    }

    fn record(&mut self, partial: PartialGroup) {
        self.visited += 1;
        let info = self.spec.table.info(partial.index);
        let chiitoitsu = if self.chiitoitsu_possible && partial.chiitoitsu_compatible {
            ChiitoitsuShape::Pairs {
                pairs: partial.pairs,
                single: partial.single,
            }
        } else {
            ChiitoitsuShape::Broken
        };
        if info.is_inert() && chiitoitsu == ChiitoitsuShape::Broken {
            return;
        }

        let key = GroupClassKey {
            size: partial.size,
            complete: info.complete,
            complete_with_pair: info.complete_with_pair,
            wait_to_complete: info.wait_to_complete,
            wait_to_complete_with_pair: info.wait_to_complete_with_pair,
            chiitoitsu,
        };
        let entry = self.classes.entry(key).or_insert((0, 0));
        entry.0 += partial.weight;
        entry.1 += 1;
    }
}
