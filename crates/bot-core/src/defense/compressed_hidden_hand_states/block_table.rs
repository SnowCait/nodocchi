use std::sync::OnceLock;

const MAX_TILE_COPIES: usize = 4;

// 数牌1色分の牌種数と字牌の牌種数。
pub(super) const SUITED_TILES: usize = 9;
pub(super) const HONOR_TILES: usize = 7;

// 隠れ手牌に入り得る1群あたりの面子数上限。4面子1雀頭を1群へ寄せても4面子で足りる。
const MAX_BLOCKS: usize = 4;

// 群ごとの牌数ベクタを 0..=4 の5進数へ詰めた index の桁重み。
pub(super) const POW5: [usize; SUITED_TILES + 1] =
    [1, 5, 25, 125, 625, 3125, 15625, 78125, 390625, 1953125];

// 群ごとの牌数ベクタ index から引く、完成形と待ちの事前計算表。
//
// `complete` は「ちょうど size/3 面子へ分解できる」、`complete_with_pair` は「(size-2)/3 面子と
// 雀頭1つへ分解できる」。`wait_*` はそれぞれ、1枚足すとその形になる牌種の bit mask。
// 表は牌数ベクタだけで決まるので残枚数に依存せず、process 全体で共有できる。
const WAIT_WITH_PAIR_SHIFT: u32 = 9;
const COMPLETE_BIT: u32 = 1 << 18;
const COMPLETE_WITH_PAIR_BIT: u32 = 1 << 19;
const WAIT_MASK: u32 = (1 << SUITED_TILES) - 1;

#[derive(Debug, Clone, Copy)]
pub(super) struct BlockInfo {
    pub(super) complete: bool,
    pub(super) complete_with_pair: bool,
    pub(super) wait_to_complete: u16,
    pub(super) wait_to_complete_with_pair: u16,
}

impl BlockInfo {
    // 完成形にも待ちにも寄与しない群は、どの config でも標準形の待ちを空にする。
    pub(super) fn is_inert(&self) -> bool {
        !self.complete
            && !self.complete_with_pair
            && self.wait_to_complete == 0
            && self.wait_to_complete_with_pair == 0
    }
}

pub(super) struct BlockTable {
    entries: Vec<u32>,
}

impl BlockTable {
    pub(super) fn suited() -> &'static Self {
        static TABLE: OnceLock<BlockTable> = OnceLock::new();
        TABLE.get_or_init(|| Self::build(SUITED_TILES, &suited_block_shapes()))
    }

    pub(super) fn honor() -> &'static Self {
        static TABLE: OnceLock<BlockTable> = OnceLock::new();
        TABLE.get_or_init(|| Self::build(HONOR_TILES, &honor_block_shapes()))
    }

    // 面子の多重集合から完成形 index を直接作り、その index から1枚抜いた先へ待ち bit を配る。
    // 全 index を走査せずに済むので、表の構築は完成形の個数だけで決まる。
    fn build(tiles: usize, shapes: &[BlockShape]) -> Self {
        let mut marks = BlockMarks::default();
        let mut counts = [0u8; SUITED_TILES];
        mark_block_indices(shapes, tiles, 0, 0, 0, &mut counts, &mut marks);
        marks.complete.sort_unstable();
        marks.complete.dedup();
        marks.with_pair.sort_unstable();
        marks.with_pair.dedup();

        let mut entries = vec![0u32; POW5[tiles]];
        for &index in &marks.complete {
            entries[index] |= COMPLETE_BIT;
            spread_wait(&mut entries, tiles, index, 0);
        }
        for &index in &marks.with_pair {
            entries[index] |= COMPLETE_WITH_PAIR_BIT;
            spread_wait(&mut entries, tiles, index, WAIT_WITH_PAIR_SHIFT);
        }
        Self { entries }
    }

    pub(super) fn info(&self, index: usize) -> BlockInfo {
        let entry = self.entries[index];
        BlockInfo {
            complete: entry & COMPLETE_BIT != 0,
            complete_with_pair: entry & COMPLETE_WITH_PAIR_BIT != 0,
            wait_to_complete: (entry & WAIT_MASK) as u16,
            wait_to_complete_with_pair: ((entry >> WAIT_WITH_PAIR_SHIFT) & WAIT_MASK) as u16,
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct BlockShape {
    counts: [u8; SUITED_TILES],
    index: usize,
}

#[derive(Debug, Default)]
struct BlockMarks {
    complete: Vec<usize>,
    with_pair: Vec<usize>,
}

fn suited_block_shapes() -> Vec<BlockShape> {
    let mut shapes = Vec::new();
    for tile in 0..SUITED_TILES {
        let mut counts = [0u8; SUITED_TILES];
        counts[tile] = 3;
        shapes.push(BlockShape {
            counts,
            index: 3 * POW5[tile],
        });
    }
    for start in 0..SUITED_TILES - 2 {
        let mut counts = [0u8; SUITED_TILES];
        for offset in 0..3 {
            counts[start + offset] = 1;
        }
        shapes.push(BlockShape {
            counts,
            index: POW5[start] + POW5[start + 1] + POW5[start + 2],
        });
    }
    shapes
}

fn honor_block_shapes() -> Vec<BlockShape> {
    (0..HONOR_TILES)
        .map(|tile| {
            let mut counts = [0u8; SUITED_TILES];
            counts[tile] = 3;
            BlockShape {
                counts,
                index: 3 * POW5[tile],
            }
        })
        .collect()
}

fn mark_block_indices(
    shapes: &[BlockShape],
    tiles: usize,
    start: usize,
    depth: usize,
    index: usize,
    counts: &mut [u8; SUITED_TILES],
    marks: &mut BlockMarks,
) {
    marks.complete.push(index);
    for (pair, &count) in counts.iter().take(tiles).enumerate() {
        if usize::from(count) + 2 <= MAX_TILE_COPIES {
            marks.with_pair.push(index + 2 * POW5[pair]);
        }
    }
    if depth == MAX_BLOCKS {
        return;
    }
    for (position, shape) in shapes.iter().enumerate().skip(start) {
        if (0..tiles).any(|tile| usize::from(counts[tile] + shape.counts[tile]) > MAX_TILE_COPIES) {
            continue;
        }
        for (count, added) in counts.iter_mut().zip(shape.counts).take(tiles) {
            *count += added;
        }
        mark_block_indices(
            shapes,
            tiles,
            position,
            depth + 1,
            index + shape.index,
            counts,
            marks,
        );
        for (count, added) in counts.iter_mut().zip(shape.counts).take(tiles) {
            *count -= added;
        }
    }
}

fn spread_wait(entries: &mut [u32], tiles: usize, index: usize, shift: u32) {
    for tile in 0..tiles {
        if (index / POW5[tile]) % 5 >= 1 {
            entries[index - POW5[tile]] |= 1 << (shift + tile as u32);
        }
    }
}
