use crate::meld::{Meld, MeldShape, fixed_meld_count};
use crate::shanten::FixedMeldCount;
use crate::tile::{TileId, TileType};
use crate::tile_counts::{TileCountError, TileCounts};
use thiserror::Error;

const CHIITOITSU_PAIR_COUNT: usize = 7;
const COMPLETED_HAND_TILE_COUNT: u8 = 14;

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum CompletedHandError {
    #[error("too many fixed melds: {0}")]
    TooManyFixedMelds(usize),

    #[error(transparent)]
    TileCount(#[from] TileCountError),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ConcealedMeld {
    Sequence { start: TileType },
    Triplet { tile: TileType },
}

impl ConcealedMeld {
    pub fn is_sequence(self) -> bool {
        matches!(self, Self::Sequence { .. })
    }

    pub fn is_triplet(self) -> bool {
        matches!(self, Self::Triplet { .. })
    }

    pub fn first_tile_type(self) -> TileType {
        match self {
            Self::Sequence { start } => start,
            Self::Triplet { tile } => tile,
        }
    }

    pub fn tile_types(self) -> Option<[TileType; 3]> {
        match self {
            Self::Sequence { start } => start.sequence(),
            Self::Triplet { tile } => Some([tile, tile, tile]),
        }
    }

    pub fn shape(self) -> MeldShape {
        match self {
            Self::Sequence { start } => MeldShape::Sequence { start },
            Self::Triplet { tile } => MeldShape::Triplet { tile },
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct StandardDecomposition {
    pair: TileType,
    concealed_melds: Vec<ConcealedMeld>,
    fixed_meld_count: FixedMeldCount,
}

impl StandardDecomposition {
    pub fn pair(&self) -> TileType {
        self.pair
    }

    pub fn concealed_melds(&self) -> &[ConcealedMeld] {
        &self.concealed_melds
    }

    pub fn fixed_meld_count(&self) -> FixedMeldCount {
        self.fixed_meld_count
    }

    pub fn meld_count(&self) -> u8 {
        self.concealed_melds.len() as u8 + self.fixed_meld_count.get()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ChiitoitsuDecomposition {
    pairs: [TileType; CHIITOITSU_PAIR_COUNT],
}

impl ChiitoitsuDecomposition {
    pub fn pairs(&self) -> &[TileType; CHIITOITSU_PAIR_COUNT] {
        &self.pairs
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct KokushiDecomposition {
    pair: TileType,
}

impl KokushiDecomposition {
    pub fn pair(&self) -> TileType {
        self.pair
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum CompletedHandDecomposition {
    Standard(StandardDecomposition),
    Chiitoitsu(ChiitoitsuDecomposition),
    Kokushi(KokushiDecomposition),
}

impl CompletedHandDecomposition {
    pub fn as_standard(&self) -> Option<&StandardDecomposition> {
        match self {
            Self::Standard(decomposition) => Some(decomposition),
            _ => None,
        }
    }

    pub fn as_chiitoitsu(&self) -> Option<&ChiitoitsuDecomposition> {
        match self {
            Self::Chiitoitsu(decomposition) => Some(decomposition),
            _ => None,
        }
    }

    pub fn as_kokushi(&self) -> Option<&KokushiDecomposition> {
        match self {
            Self::Kokushi(decomposition) => Some(decomposition),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompletedHandAnalysis {
    concealed_tiles: Vec<TileId>,
    fixed_melds: Vec<Meld>,
    decompositions: Vec<CompletedHandDecomposition>,
}

impl CompletedHandAnalysis {
    pub fn concealed_tiles(&self) -> &[TileId] {
        &self.concealed_tiles
    }

    pub fn fixed_melds(&self) -> &[Meld] {
        &self.fixed_melds
    }

    pub fn decompositions(&self) -> &[CompletedHandDecomposition] {
        &self.decompositions
    }

    pub fn is_complete(&self) -> bool {
        !self.decompositions.is_empty()
    }

    pub fn standard_decompositions(&self) -> impl Iterator<Item = &StandardDecomposition> {
        self.decompositions
            .iter()
            .filter_map(CompletedHandDecomposition::as_standard)
    }

    pub fn chiitoitsu_decomposition(&self) -> Option<&ChiitoitsuDecomposition> {
        self.decompositions
            .iter()
            .find_map(CompletedHandDecomposition::as_chiitoitsu)
    }

    pub fn kokushi_decomposition(&self) -> Option<&KokushiDecomposition> {
        self.decompositions
            .iter()
            .find_map(CompletedHandDecomposition::as_kokushi)
    }
}

pub fn analyze_completed_hand(
    concealed_tiles: &[TileId],
    fixed_melds: &[Meld],
) -> Result<CompletedHandAnalysis, CompletedHandError> {
    let fixed_meld_count = fixed_meld_count(fixed_melds)
        .ok_or(CompletedHandError::TooManyFixedMelds(fixed_melds.len()))?;

    let mut visible = TileCounts::new();
    for tile in concealed_tiles
        .iter()
        .chain(fixed_melds.iter().flat_map(|meld| meld.tiles()))
    {
        visible.try_add(tile.tile_type())?;
    }

    let concealed = TileCounts::from_tiles(concealed_tiles.iter().copied());
    Ok(CompletedHandAnalysis {
        concealed_tiles: concealed_tiles.to_vec(),
        fixed_melds: fixed_melds.to_vec(),
        decompositions: decompositions(&concealed, fixed_meld_count),
    })
}

fn decompositions(
    concealed: &TileCounts,
    fixed_meld_count: FixedMeldCount,
) -> Vec<CompletedHandDecomposition> {
    let mut decompositions: Vec<_> = standard_decompositions(concealed, fixed_meld_count)
        .into_iter()
        .map(CompletedHandDecomposition::Standard)
        .collect();

    if !fixed_meld_count.has_melds() {
        decompositions.extend(
            chiitoitsu_decomposition(concealed).map(CompletedHandDecomposition::Chiitoitsu),
        );
        decompositions
            .extend(kokushi_decomposition(concealed).map(CompletedHandDecomposition::Kokushi));
    }

    decompositions
}

fn standard_decompositions(
    concealed: &TileCounts,
    fixed_meld_count: FixedMeldCount,
) -> Vec<StandardDecomposition> {
    let Some(concealed_meld_count) = completed_concealed_meld_count(concealed, fixed_meld_count)
    else {
        return Vec::new();
    };

    let mut decompositions = Vec::new();
    for (pair, count) in concealed.iter() {
        if count < 2 {
            continue;
        }

        let mut rest = *concealed;
        if rest.remove_pair(pair).is_err() {
            continue;
        }

        let mut visitor = DecompositionCollector {
            melds: Vec::with_capacity(concealed_meld_count),
            found: Vec::new(),
        };
        visit_concealed_melds(rest, concealed_meld_count, &mut visitor);

        decompositions.extend(visitor.found.into_iter().map(|mut concealed_melds| {
            concealed_melds.sort_unstable();
            StandardDecomposition {
                pair,
                concealed_melds,
                fixed_meld_count,
            }
        }));
    }

    decompositions.sort_unstable();
    decompositions.dedup();
    decompositions
}

/// 固定面子を含めて Standard hand が構造上完成しているか。
///
/// [`analyze_completed_hand`] の Standard decomposition と同じ探索を使うが、最初の完成形で
/// 打ち切り、decomposition や物理牌列を構築しない。七対子・国士無双は対象外。
pub fn is_standard_hand_complete(concealed: &TileCounts, fixed_meld_count: FixedMeldCount) -> bool {
    let Some(concealed_meld_count) = completed_concealed_meld_count(concealed, fixed_meld_count)
    else {
        return false;
    };

    for (pair, count) in concealed.iter() {
        if count < 2 {
            continue;
        }

        let mut rest = *concealed;
        if rest.remove_pair(pair).is_err() {
            continue;
        }
        if visit_concealed_melds(rest, concealed_meld_count, &mut CompletionFinder) {
            return true;
        }
    }
    false
}

/// 指定した牌種集合のいずれか1枚で Standard hand が構造上完成するか。
///
/// 完成形を牌種ごとに作り直さず、1枚不足した concealed hand を一度探索する。不足牌を雀頭か
/// 面子へ割り当てた後は [`is_standard_hand_complete`] と同じ [`visit_concealed_melds`] に合流
/// する。手牌に4枚ある牌種は5枚目になり得ないため candidate から除外する。
pub fn standard_completion_intersects(
    concealed: &TileCounts,
    fixed_meld_count: FixedMeldCount,
    candidate_tiles: &[TileType],
) -> bool {
    let concealed_meld_count = usize::from(FixedMeldCount::MAX - fixed_meld_count.get());
    if usize::from(concealed.total()) != 1 + 3 * concealed_meld_count {
        return false;
    }

    let candidates = CompletionCandidates::new(concealed, candidate_tiles);
    if candidates.is_empty() {
        return false;
    }

    for (pair, count) in concealed.iter() {
        if count >= 1 && candidates.contains(pair) {
            let mut rest = *concealed;
            rest.remove(pair).expect("pair candidate tile is present");
            if visit_concealed_melds(rest, concealed_meld_count, &mut CompletionFinder) {
                return true;
            }
        }

        if count >= 2 {
            let mut rest = *concealed;
            rest.remove_pair(pair).expect("checked pair count");
            if visit_concealed_melds_with_candidate(rest, concealed_meld_count, &candidates) {
                return true;
            }
        }
    }
    false
}

#[derive(Debug, Clone, Copy)]
struct CompletionCandidates([bool; TileType::COUNT]);

impl CompletionCandidates {
    fn new(concealed: &TileCounts, candidate_tiles: &[TileType]) -> Self {
        let mut candidates = [false; TileType::COUNT];
        for &tile in candidate_tiles {
            if concealed.count(tile) < 4 {
                candidates[tile.index()] = true;
            }
        }
        Self(candidates)
    }

    fn contains(&self, tile: TileType) -> bool {
        self.0[tile.index()]
    }

    fn is_empty(&self) -> bool {
        !self.0.iter().any(|&candidate| candidate)
    }
}

fn completed_concealed_meld_count(
    concealed: &TileCounts,
    fixed_meld_count: FixedMeldCount,
) -> Option<usize> {
    let concealed_meld_count = usize::from(FixedMeldCount::MAX - fixed_meld_count.get());
    (usize::from(concealed.total()) == 2 + 3 * concealed_meld_count).then_some(concealed_meld_count)
}

trait ConcealedMeldVisitor {
    // `true` stops the traversal after the first result needed by a boolean caller.
    fn complete(&mut self) -> bool;
    fn push(&mut self, meld: ConcealedMeld);
    fn pop(&mut self);
}

struct DecompositionCollector {
    melds: Vec<ConcealedMeld>,
    found: Vec<Vec<ConcealedMeld>>,
}

impl ConcealedMeldVisitor for DecompositionCollector {
    fn complete(&mut self) -> bool {
        self.found.push(self.melds.clone());
        false
    }

    fn push(&mut self, meld: ConcealedMeld) {
        self.melds.push(meld);
    }

    fn pop(&mut self) {
        self.melds.pop();
    }
}

struct CompletionFinder;

impl ConcealedMeldVisitor for CompletionFinder {
    fn complete(&mut self) -> bool {
        true
    }

    fn push(&mut self, _meld: ConcealedMeld) {}

    fn pop(&mut self) {}
}

fn visit_concealed_melds(
    counts: TileCounts,
    remaining: usize,
    visitor: &mut impl ConcealedMeldVisitor,
) -> bool {
    if remaining == 0 {
        return counts.is_empty() && visitor.complete();
    }

    let Some(tile) = counts
        .iter()
        .find_map(|(tile, count)| (count >= 1).then_some(tile))
    else {
        return false;
    };

    let mut triplet_removed = counts;
    if triplet_removed.remove_triplet(tile).is_ok() {
        visitor.push(ConcealedMeld::Triplet { tile });
        let complete = visit_concealed_melds(triplet_removed, remaining - 1, visitor);
        visitor.pop();
        if complete {
            return true;
        }
    }

    let mut sequence_removed = counts;
    if sequence_removed.remove_sequence(tile).is_ok() {
        visitor.push(ConcealedMeld::Sequence { start: tile });
        let complete = visit_concealed_melds(sequence_removed, remaining - 1, visitor);
        visitor.pop();
        if complete {
            return true;
        }
    }
    false
}

// 1枚不足した面子集合を探索する。不足牌を含まない完全な面子は canonical traversal と同じ
// triplet / sequence removal を行い、不足牌を含む面子を確定した時点で visit_concealed_melds()
// へ合流する。
fn visit_concealed_melds_with_candidate(
    counts: TileCounts,
    remaining: usize,
    candidates: &CompletionCandidates,
) -> bool {
    if remaining == 0 {
        return false;
    }

    let Some(tile) = counts
        .iter()
        .find_map(|(tile, count)| (count >= 1).then_some(tile))
    else {
        return false;
    };

    let mut triplet_removed = counts;
    if triplet_removed.remove_triplet(tile).is_ok()
        && visit_concealed_melds_with_candidate(triplet_removed, remaining - 1, candidates)
    {
        return true;
    }

    let mut sequence_removed = counts;
    if sequence_removed.remove_sequence(tile).is_ok()
        && visit_concealed_melds_with_candidate(sequence_removed, remaining - 1, candidates)
    {
        return true;
    }

    let mut pair_removed = counts;
    if candidates.contains(tile)
        && pair_removed.remove_pair(tile).is_ok()
        && visit_concealed_melds(pair_removed, remaining - 1, &mut CompletionFinder)
    {
        return true;
    }

    let mut adjacent_removed = counts;
    let adjacent_candidate = tile
        .previous_in_suit()
        .is_some_and(|candidate| candidates.contains(candidate))
        || tile
            .second_next_in_suit()
            .is_some_and(|candidate| candidates.contains(candidate));
    if adjacent_candidate
        && adjacent_removed.remove_adjacent_wait(tile).is_ok()
        && visit_concealed_melds(adjacent_removed, remaining - 1, &mut CompletionFinder)
    {
        return true;
    }

    let mut skip_removed = counts;
    if tile
        .next_in_suit()
        .is_some_and(|candidate| candidates.contains(candidate))
        && skip_removed.remove_skip_wait(tile).is_ok()
        && visit_concealed_melds(skip_removed, remaining - 1, &mut CompletionFinder)
    {
        return true;
    }

    false
}

fn chiitoitsu_decomposition(concealed: &TileCounts) -> Option<ChiitoitsuDecomposition> {
    if concealed.total() != COMPLETED_HAND_TILE_COUNT {
        return None;
    }

    let mut pairs = Vec::with_capacity(CHIITOITSU_PAIR_COUNT);
    for (tile, count) in concealed.iter() {
        match count {
            0 => {}
            2 => pairs.push(tile),
            _ => return None,
        }
    }

    let pairs: [TileType; CHIITOITSU_PAIR_COUNT] = pairs.try_into().ok()?;
    Some(ChiitoitsuDecomposition { pairs })
}

fn kokushi_decomposition(concealed: &TileCounts) -> Option<KokushiDecomposition> {
    if concealed.total() != COMPLETED_HAND_TILE_COUNT {
        return None;
    }

    let mut pair = None;
    for (tile, count) in concealed.iter() {
        if !tile.is_yaochu() {
            if count > 0 {
                return None;
            }
            continue;
        }

        match count {
            1 => {}
            2 if pair.is_none() => pair = Some(tile),
            _ => return None,
        }
    }

    pair.map(|pair| KokushiDecomposition { pair })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::acceptance::structural_acceptance_tile_types_with_fixed_melds;
    use crate::meld::MeldKind;
    use crate::shanten::{
        calculate_shanten, calculate_shanten_with_fixed_melds, chiitoitsu_shanten, kokushi_shanten,
        standard_shanten, standard_shanten_with_fixed_melds,
    };

    struct TileIdSource {
        used: [u8; TileType::COUNT],
    }

    impl TileIdSource {
        fn new() -> Self {
            Self {
                used: [0; TileType::COUNT],
            }
        }

        fn tiles(&mut self, strings: &[&str]) -> Vec<TileId> {
            strings.iter().map(|s| self.tile(s)).collect()
        }

        fn meld(&mut self, kind: MeldKind, strings: &[&str]) -> Meld {
            let tiles = self.tiles(strings);
            let called_tile = kind.is_open().then(|| tiles[0]);
            Meld::new(kind, tiles, called_tile)
        }

        fn tile(&mut self, s: &str) -> TileId {
            let tile_type = tile_type(s);
            let copy = &mut self.used[tile_type.index()];
            let id = TileId::new(tile_type.raw() * 4 + *copy).unwrap();
            *copy += 1;
            id
        }
    }

    fn tile_type(s: &str) -> TileType {
        TileType::from_mjai_type_str(s).unwrap()
    }

    fn sequence(s: &str) -> ConcealedMeld {
        ConcealedMeld::Sequence {
            start: tile_type(s),
        }
    }

    fn triplet(s: &str) -> ConcealedMeld {
        ConcealedMeld::Triplet { tile: tile_type(s) }
    }

    fn counts_of(tiles: &[TileId]) -> TileCounts {
        TileCounts::from_tiles(tiles.iter().copied())
    }

    fn fixed_pons(source: &mut TileIdSource, count: usize) -> Vec<Meld> {
        ["P", "F", "C"]
            .iter()
            .take(count)
            .map(|honor| source.meld(MeldKind::Pon, &[honor, honor, honor]))
            .collect()
    }

    fn fixed_count(value: u8) -> FixedMeldCount {
        FixedMeldCount::new(value).unwrap()
    }

    fn narrow_standard_completion_intersects(
        counts: &TileCounts,
        fixed_meld_count: FixedMeldCount,
        candidates: &[TileType],
    ) -> bool {
        candidates.iter().any(|&tile| {
            let mut completed = *counts;
            completed.try_add(tile).is_ok()
                && is_standard_hand_complete(&completed, fixed_meld_count)
        })
    }

    fn assert_completion_intersection_oracles(
        counts: &TileCounts,
        fixed_meld_count: FixedMeldCount,
        candidates: &[TileType],
    ) {
        let batch = standard_completion_intersects(counts, fixed_meld_count, candidates);
        let narrow = narrow_standard_completion_intersects(counts, fixed_meld_count, candidates);
        assert_eq!(
            batch, narrow,
            "counts={counts:?}, candidates={candidates:?}"
        );

        let waits = structural_acceptance_tile_types_with_fixed_melds(counts, fixed_meld_count);
        let acceptance = candidates.iter().any(|candidate| waits.contains(candidate));
        assert_eq!(
            batch, acceptance,
            "acceptance counts={counts:?}, candidates={candidates:?}, waits={waits:?}"
        );
    }

    #[test]
    fn boolean_standard_completion_matches_analysis_across_open_hand_counts() {
        let palette = ["1m", "2m", "3m", "4m", "5m", "E"];

        for meld_count in 1..=3usize {
            let mut source = TileIdSource::new();
            let fixed = fixed_pons(&mut source, meld_count);
            let concealed_len = 14 - 3 * meld_count;

            // 6牌種の各0..=4枚を横断する。順子・刻子・字牌・複数分解候補を含み、
            // OpenHand 1/2/3副露の正しい concealed 枚数だけを materialized analysis と比較する。
            for mut encoded in 0..5u32.pow(palette.len() as u32) {
                let mut counts = TileCounts::new();
                let mut concealed = Vec::with_capacity(concealed_len);
                for mjai in palette {
                    let copies = (encoded % 5) as usize;
                    encoded /= 5;
                    let tile = tile_type(mjai);
                    for id in TileId::copies(tile).take(copies) {
                        counts.add(tile);
                        concealed.push(id);
                    }
                }
                if concealed.len() != concealed_len {
                    continue;
                }

                let materialized = analyze_completed_hand(&concealed, &fixed)
                    .expect("at most four copies")
                    .standard_decompositions()
                    .next()
                    .is_some();
                assert_eq!(
                    is_standard_hand_complete(&counts, fixed_count(meld_count as u8)),
                    materialized,
                    "meld count={meld_count}, counts={:?}",
                    counts.as_array()
                );
            }
        }
    }

    #[test]
    fn boolean_standard_completion_covers_open_hand_wait_boundaries() {
        let cases: &[(&str, u8, &[&str], &str, bool)] = &[
            (
                "ryanmen",
                1,
                &["2m", "3m", "4p", "5p", "6p", "7s", "8s", "9s", "E", "E"],
                "1m",
                true,
            ),
            (
                "kanchan",
                1,
                &["1m", "3m", "4p", "5p", "6p", "7s", "8s", "9s", "E", "E"],
                "2m",
                true,
            ),
            (
                "penchan",
                1,
                &["1m", "2m", "4p", "5p", "6p", "7s", "8s", "9s", "E", "E"],
                "3m",
                true,
            ),
            (
                "shanpon",
                1,
                &["1m", "1m", "4p", "5p", "6p", "7s", "8s", "9s", "E", "E"],
                "1m",
                true,
            ),
            (
                "honor shanpon",
                1,
                &["1m", "2m", "3m", "4p", "5p", "6p", "E", "E", "S", "S"],
                "E",
                true,
            ),
            (
                "tanki",
                1,
                &["1m", "2m", "3m", "4p", "5p", "6p", "7s", "8s", "9s", "E"],
                "E",
                true,
            ),
            (
                "multiple decompositions",
                1,
                &["1m", "1m", "1m", "2m", "2m", "2m", "3m", "3m", "3m", "5m"],
                "5m",
                true,
            ),
            (
                "two fixed melds",
                2,
                &["1m", "2m", "3m", "4p", "5p", "6p", "E"],
                "E",
                true,
            ),
            ("three fixed melds", 3, &["1m", "2m", "E", "E"], "3m", true),
            (
                "incomplete",
                1,
                &["1m", "3m", "4p", "6p", "7s", "9s", "E", "E", "S", "S"],
                "2m",
                false,
            ),
        ];

        for &(name, meld_count, hidden, wait, expected) in cases {
            let mut counts = TileCounts::from_tile_types(hidden.iter().map(|mjai| tile_type(mjai)));
            counts.try_add(tile_type(wait)).expect("not a fifth copy");

            let mut source = TileIdSource::new();
            let fixed = fixed_pons(&mut source, usize::from(meld_count));
            let concealed: Vec<TileId> = TileType::all()
                .flat_map(|tile| TileId::copies(tile).take(usize::from(counts.count(tile))))
                .collect();
            let materialized = analyze_completed_hand(&concealed, &fixed)
                .expect("physical hand")
                .standard_decompositions()
                .next()
                .is_some();

            assert_eq!(materialized, expected, "oracle case: {name}");
            assert_eq!(
                is_standard_hand_complete(&counts, fixed_count(meld_count)),
                materialized,
                "boolean case: {name}"
            );
        }

        let mut four_copies = TileCounts::from_tile_types([
            tile_type("1m"),
            tile_type("1m"),
            tile_type("1m"),
            tile_type("1m"),
        ]);
        assert!(four_copies.try_add(tile_type("1m")).is_err());
    }

    #[test]
    fn standard_completion_intersection_covers_wait_boundaries() {
        let cases: &[(&str, u8, &[&str], &[&str])] = &[
            (
                "ryanmen",
                1,
                &["2m", "3m", "4p", "5p", "6p", "7s", "8s", "9s", "E", "E"],
                &["1m", "4m"],
            ),
            (
                "kanchan",
                1,
                &["1m", "3m", "4p", "5p", "6p", "7s", "8s", "9s", "E", "E"],
                &["2m"],
            ),
            (
                "penchan",
                1,
                &["1m", "2m", "4p", "5p", "6p", "7s", "8s", "9s", "E", "E"],
                &["3m"],
            ),
            (
                "shanpon",
                1,
                &["1m", "1m", "4p", "5p", "6p", "7s", "8s", "9s", "E", "E"],
                &["1m", "E"],
            ),
            (
                "honor shanpon",
                1,
                &["1m", "2m", "3m", "4p", "5p", "6p", "E", "E", "S", "S"],
                &["E", "S"],
            ),
            (
                "tanki",
                1,
                &["1m", "2m", "3m", "4p", "5p", "6p", "7s", "8s", "9s", "E"],
                &["E"],
            ),
            (
                "multiple decompositions",
                1,
                &["1m", "1m", "1m", "2m", "2m", "2m", "3m", "3m", "3m", "5m"],
                &["2m", "3m", "4m", "5m"],
            ),
            (
                "two fixed melds",
                2,
                &["1m", "2m", "3m", "4p", "5p", "6p", "E"],
                &["E"],
            ),
            ("three fixed melds", 3, &["1m", "2m", "E", "E"], &["3m"]),
        ];

        for &(name, meld_count, hidden, waits) in cases {
            let counts = TileCounts::from_tile_types(hidden.iter().map(|mjai| tile_type(mjai)));
            let fixed_meld_count = fixed_count(meld_count);
            let waits: Vec<_> = waits.iter().map(|mjai| tile_type(mjai)).collect();
            let non_wait = tile_type("9p");

            for candidates in [
                Vec::new(),
                vec![non_wait],
                vec![non_wait, waits[0]],
                waits.clone(),
            ] {
                assert_completion_intersection_oracles(&counts, fixed_meld_count, &candidates);
            }
            assert!(
                standard_completion_intersects(&counts, fixed_meld_count, &waits),
                "case: {name}"
            );
        }

        let four_copies = TileCounts::from_tile_types([tile_type("5m"); 4]);
        assert_completion_intersection_oracles(&four_copies, fixed_count(3), &[tile_type("5m")]);
        assert!(!standard_completion_intersects(
            &four_copies,
            fixed_count(3),
            &[tile_type("5m")]
        ));
    }

    #[test]
    fn standard_completion_intersection_matches_oracles_across_open_hand_counts() {
        let palette = ["1m", "2m", "3m", "4m", "5m", "E"];

        for meld_count in 1..=3u8 {
            let fixed_meld_count = fixed_count(meld_count);
            let concealed_len = 13 - 3 * usize::from(meld_count);
            let mut checked = 0;

            for mut encoded in 0..5u32.pow(palette.len() as u32) {
                let mut counts = TileCounts::new();
                for mjai in palette {
                    let copies = (encoded % 5) as usize;
                    encoded /= 5;
                    let tile = tile_type(mjai);
                    for _ in 0..copies {
                        counts.add(tile);
                    }
                }
                if usize::from(counts.total()) != concealed_len
                    || standard_shanten_with_fixed_melds(&counts, fixed_meld_count) != 0
                {
                    continue;
                }
                checked += 1;

                let candidate_sets = [
                    Vec::new(),
                    vec![tile_type("9p")],
                    vec![tile_type("1m")],
                    vec![tile_type("2m"), tile_type("4m")],
                    vec![tile_type("9p"), tile_type("3m"), tile_type("E")],
                    TileType::all().collect(),
                ];
                for candidates in candidate_sets {
                    assert_completion_intersection_oracles(&counts, fixed_meld_count, &candidates);
                }
            }

            assert!(checked > 0, "meld count: {meld_count}");
        }
    }

    fn standard_shapes(analysis: &CompletedHandAnalysis) -> Vec<(TileType, Vec<ConcealedMeld>)> {
        analysis
            .standard_decompositions()
            .map(|decomposition| {
                (
                    decomposition.pair(),
                    decomposition.concealed_melds().to_vec(),
                )
            })
            .collect()
    }

    #[test]
    fn unique_standard_hand_has_one_decomposition() {
        let mut source = TileIdSource::new();
        let concealed = source.tiles(&[
            "1m", "2m", "3m", "4m", "5m", "6m", "7m", "8m", "9m", "1p", "2p", "3p", "5s", "5s",
        ]);

        let analysis = analyze_completed_hand(&concealed, &[]).unwrap();

        assert!(analysis.is_complete());
        assert_eq!(
            standard_shapes(&analysis),
            vec![(
                tile_type("5s"),
                vec![
                    sequence("1m"),
                    sequence("4m"),
                    sequence("7m"),
                    sequence("1p"),
                ],
            )]
        );
        assert_eq!(analysis.decompositions().len(), 1);
        assert_eq!(standard_shanten(&counts_of(&concealed)), -1);
    }

    #[test]
    fn concealed_melds_map_to_neutral_meld_shapes() {
        assert_eq!(
            sequence("2m").shape(),
            MeldShape::Sequence {
                start: tile_type("2m")
            }
        );
        assert_eq!(
            triplet("9s").shape(),
            MeldShape::Triplet {
                tile: tile_type("9s")
            }
        );
        assert!(!triplet("9s").shape().is_kan());
    }

    #[test]
    fn analysis_keeps_physical_tiles_and_fixed_melds() {
        let mut source = TileIdSource::new();
        let concealed = source.tiles(&[
            "1m", "2m", "3m", "4m", "5m", "6m", "7m", "8m", "9m", "1p", "2p", "3p", "5s", "5s",
        ]);

        let analysis = analyze_completed_hand(&concealed, &[]).unwrap();

        assert_eq!(analysis.concealed_tiles(), concealed);
        assert!(analysis.fixed_melds().is_empty());
    }

    #[test]
    fn standard_hand_with_triplets_keeps_meld_kinds() {
        let mut source = TileIdSource::new();
        let concealed = source.tiles(&[
            "1m", "1m", "1m", "2m", "3m", "4m", "5p", "6p", "7p", "9s", "9s", "9s", "E", "E",
        ]);

        let analysis = analyze_completed_hand(&concealed, &[]).unwrap();

        assert_eq!(
            standard_shapes(&analysis),
            vec![(
                tile_type("E"),
                vec![sequence("2m"), sequence("5p"), triplet("1m"), triplet("9s"),],
            )]
        );
        let melds: Vec<_> = analysis
            .standard_decompositions()
            .flat_map(|decomposition| decomposition.concealed_melds().iter().copied())
            .collect();
        assert_eq!(melds.iter().filter(|meld| meld.is_sequence()).count(), 2);
        assert_eq!(melds.iter().filter(|meld| meld.is_triplet()).count(), 2);
        assert_eq!(triplet("1m").tile_types(), Some([tile_type("1m"); 3]));
        assert_eq!(
            sequence("2m").tile_types(),
            Some([tile_type("2m"), tile_type("3m"), tile_type("4m")])
        );
        assert_eq!(standard_shanten(&counts_of(&concealed)), -1);
    }

    #[test]
    fn fixed_pon_leaves_three_concealed_melds() {
        let mut source = TileIdSource::new();
        let fixed = vec![source.meld(MeldKind::Pon, &["E", "E", "E"])];
        let concealed = source.tiles(&[
            "1m", "2m", "3m", "4m", "5m", "6m", "7m", "8m", "9m", "5s", "5s",
        ]);

        let analysis = analyze_completed_hand(&concealed, &fixed).unwrap();
        let decompositions: Vec<_> = analysis.standard_decompositions().collect();

        assert_eq!(decompositions.len(), 1);
        let decomposition = decompositions[0];
        assert_eq!(decomposition.pair(), tile_type("5s"));
        assert_eq!(decomposition.fixed_meld_count(), fixed_count(1));
        assert_eq!(decomposition.concealed_melds().len(), 3);
        assert_eq!(decomposition.meld_count(), 4);
        assert_eq!(
            decomposition.concealed_melds(),
            [sequence("1m"), sequence("4m"), sequence("7m")]
        );
        assert!(
            !decomposition
                .concealed_melds()
                .iter()
                .any(|meld| meld.first_tile_type() == tile_type("E"))
        );
        assert_eq!(
            standard_shanten_with_fixed_melds(&counts_of(&concealed), fixed_count(1)),
            -1
        );
    }

    #[test]
    fn fixed_chi_leaves_three_concealed_melds() {
        let mut source = TileIdSource::new();
        let fixed = vec![source.meld(MeldKind::Chi, &["1p", "2p", "3p"])];
        let concealed = source.tiles(&[
            "1m", "1m", "1m", "4m", "5m", "6m", "7m", "8m", "9m", "E", "E",
        ]);

        let analysis = analyze_completed_hand(&concealed, &fixed).unwrap();
        let decompositions: Vec<_> = analysis.standard_decompositions().collect();

        assert_eq!(decompositions.len(), 1);
        assert_eq!(decompositions[0].pair(), tile_type("E"));
        assert_eq!(
            decompositions[0].concealed_melds(),
            [sequence("4m"), sequence("7m"), triplet("1m")]
        );
        assert_eq!(decompositions[0].meld_count(), 4);
    }

    #[test]
    fn ankan_counts_as_one_fixed_meld() {
        let mut source = TileIdSource::new();
        let fixed = vec![source.meld(MeldKind::Ankan, &["1m", "1m", "1m", "1m"])];
        let concealed = source.tiles(&[
            "2m", "3m", "4m", "5p", "6p", "7p", "7s", "8s", "9s", "E", "E",
        ]);

        let analysis = analyze_completed_hand(&concealed, &fixed).unwrap();
        let decompositions: Vec<_> = analysis.standard_decompositions().collect();

        assert_eq!(fixed[0].tiles().len(), 4);
        assert!(!fixed[0].is_open());
        assert_eq!(fixed_meld_count(&fixed), Some(fixed_count(1)));
        assert_eq!(decompositions.len(), 1);
        assert_eq!(decompositions[0].fixed_meld_count(), fixed_count(1));
        assert_eq!(decompositions[0].concealed_melds().len(), 3);
        assert_eq!(decompositions[0].meld_count(), 4);
        assert_eq!(decompositions[0].pair(), tile_type("E"));
        assert_eq!(
            calculate_shanten_with_fixed_melds(&counts_of(&concealed), fixed_count(1)).standard(),
            -1
        );
    }

    #[test]
    fn chiitoitsu_needs_seven_distinct_pairs() {
        let mut source = TileIdSource::new();
        let concealed = source.tiles(&[
            "1m", "1m", "3m", "3m", "5m", "5m", "7m", "7m", "9m", "9m", "1p", "1p", "E", "E",
        ]);

        let analysis = analyze_completed_hand(&concealed, &[]).unwrap();

        assert_eq!(
            analysis.chiitoitsu_decomposition().map(|it| *it.pairs()),
            Some([
                tile_type("1m"),
                tile_type("3m"),
                tile_type("5m"),
                tile_type("7m"),
                tile_type("9m"),
                tile_type("1p"),
                tile_type("E"),
            ])
        );
        assert_eq!(analysis.standard_decompositions().count(), 0);
        assert_eq!(chiitoitsu_shanten(&counts_of(&concealed)), -1);
    }

    #[test]
    fn four_of_a_kind_is_not_two_chiitoitsu_pairs() {
        let mut source = TileIdSource::new();
        let concealed = source.tiles(&[
            "1m", "1m", "1m", "1m", "2m", "2m", "2m", "2m", "3m", "3m", "3m", "3m", "4m", "4m",
        ]);

        let analysis = analyze_completed_hand(&concealed, &[]).unwrap();

        assert_eq!(analysis.chiitoitsu_decomposition(), None);
        assert_ne!(chiitoitsu_shanten(&counts_of(&concealed)), -1);
        assert_eq!(
            standard_shapes(&analysis),
            vec![
                (
                    tile_type("1m"),
                    vec![
                        sequence("1m"),
                        sequence("1m"),
                        sequence("2m"),
                        sequence("2m"),
                    ],
                ),
                (
                    tile_type("4m"),
                    vec![
                        sequence("1m"),
                        sequence("1m"),
                        sequence("1m"),
                        sequence("1m"),
                    ],
                ),
                (
                    tile_type("4m"),
                    vec![sequence("1m"), triplet("1m"), triplet("2m"), triplet("3m"),],
                ),
            ]
        );
    }

    #[test]
    fn kokushi_keeps_the_pair_tile() {
        let mut source = TileIdSource::new();
        let concealed = source.tiles(&[
            "1m", "9m", "1p", "9p", "1s", "9s", "E", "S", "W", "N", "P", "F", "C", "9s",
        ]);

        let analysis = analyze_completed_hand(&concealed, &[]).unwrap();

        assert_eq!(
            analysis
                .kokushi_decomposition()
                .map(KokushiDecomposition::pair),
            Some(tile_type("9s"))
        );
        assert_eq!(analysis.standard_decompositions().count(), 0);
        assert_eq!(analysis.chiitoitsu_decomposition(), None);
        assert_eq!(kokushi_shanten(&counts_of(&concealed)), -1);
    }

    #[test]
    fn thirteen_orphans_without_a_pair_is_not_kokushi() {
        let mut source = TileIdSource::new();
        let concealed = source.tiles(&[
            "1m", "9m", "1p", "9p", "1s", "9s", "E", "S", "W", "N", "P", "F", "C",
        ]);

        let analysis = analyze_completed_hand(&concealed, &[]).unwrap();

        assert!(!analysis.is_complete());
        assert_eq!(kokushi_shanten(&counts_of(&concealed)), 0);
    }

    #[test]
    fn fixed_melds_suppress_chiitoitsu_and_kokushi() {
        let mut source = TileIdSource::new();
        let fixed = vec![source.meld(MeldKind::Pon, &["2p", "2p", "2p"])];
        let chiitoitsu = source.tiles(&[
            "1m", "1m", "3m", "3m", "5m", "5m", "7m", "7m", "9m", "9m", "1p", "1p", "E", "E",
        ]);
        let kokushi = source.tiles(&[
            "1m", "9m", "1p", "9p", "1s", "9s", "E", "S", "W", "N", "P", "F", "C", "C",
        ]);

        let with_chiitoitsu = analyze_completed_hand(&chiitoitsu, &fixed).unwrap();
        let with_kokushi = analyze_completed_hand(&kokushi, &fixed).unwrap();

        assert_eq!(with_chiitoitsu.chiitoitsu_decomposition(), None);
        assert!(!with_chiitoitsu.is_complete());
        assert_eq!(with_kokushi.kokushi_decomposition(), None);
        assert!(!with_kokushi.is_complete());
    }

    #[test]
    fn standard_and_chiitoitsu_are_both_reported() {
        let mut source = TileIdSource::new();
        let concealed = source.tiles(&[
            "1m", "1m", "2m", "2m", "3m", "3m", "4m", "4m", "5m", "5m", "6m", "6m", "7m", "7m",
        ]);

        let analysis = analyze_completed_hand(&concealed, &[]).unwrap();

        assert_eq!(
            standard_shapes(&analysis),
            vec![
                (
                    tile_type("1m"),
                    vec![
                        sequence("2m"),
                        sequence("2m"),
                        sequence("5m"),
                        sequence("5m"),
                    ],
                ),
                (
                    tile_type("4m"),
                    vec![
                        sequence("1m"),
                        sequence("1m"),
                        sequence("5m"),
                        sequence("5m"),
                    ],
                ),
                (
                    tile_type("7m"),
                    vec![
                        sequence("1m"),
                        sequence("1m"),
                        sequence("4m"),
                        sequence("4m"),
                    ],
                ),
            ]
        );
        assert!(analysis.chiitoitsu_decomposition().is_some());
        assert_eq!(analysis.decompositions().len(), 4);

        let counts = counts_of(&concealed);
        assert_eq!(standard_shanten(&counts), -1);
        assert_eq!(chiitoitsu_shanten(&counts), -1);
        assert_eq!(calculate_shanten(&counts).min(), -1);
    }

    #[test]
    fn multiple_standard_decompositions_are_all_reported() {
        let mut source = TileIdSource::new();
        let concealed = source.tiles(&[
            "1m", "1m", "1m", "2m", "2m", "2m", "3m", "3m", "3m", "4m", "4m", "4m", "5m", "5m",
        ]);

        let analysis = analyze_completed_hand(&concealed, &[]).unwrap();

        assert_eq!(
            standard_shapes(&analysis),
            vec![
                (
                    tile_type("2m"),
                    vec![
                        sequence("2m"),
                        sequence("3m"),
                        sequence("3m"),
                        triplet("1m"),
                    ],
                ),
                (
                    tile_type("5m"),
                    vec![
                        sequence("1m"),
                        sequence("1m"),
                        sequence("1m"),
                        triplet("4m"),
                    ],
                ),
                (
                    tile_type("5m"),
                    vec![
                        sequence("2m"),
                        sequence("2m"),
                        sequence("2m"),
                        triplet("1m"),
                    ],
                ),
                (
                    tile_type("5m"),
                    vec![triplet("1m"), triplet("2m"), triplet("3m"), triplet("4m"),],
                ),
            ]
        );
        assert_eq!(standard_shanten(&counts_of(&concealed)), -1);
    }

    #[test]
    fn decompositions_are_deduplicated() {
        let mut source = TileIdSource::new();
        let concealed = source.tiles(&[
            "1m", "1m", "1m", "1m", "2m", "3m", "4p", "5p", "6p", "7p", "8p", "9p", "5s", "5s",
        ]);

        let analysis = analyze_completed_hand(&concealed, &[]).unwrap();

        assert_eq!(
            standard_shapes(&analysis),
            vec![(
                tile_type("5s"),
                vec![
                    sequence("1m"),
                    sequence("4p"),
                    sequence("7p"),
                    triplet("1m"),
                ],
            )]
        );

        let mut unique = analysis.decompositions().to_vec();
        unique.sort();
        unique.dedup();
        assert_eq!(unique.len(), analysis.decompositions().len());
    }

    #[test]
    fn incomplete_hands_have_no_decomposition() {
        let mut source = TileIdSource::new();
        let concealed = source.tiles(&[
            "1m", "2m", "3m", "4m", "5m", "6m", "7m", "8m", "9m", "1p", "3p", "5s", "7s", "9s",
        ]);

        let analysis = analyze_completed_hand(&concealed, &[]).unwrap();

        assert!(!analysis.is_complete());
        assert!(analysis.decompositions().is_empty());
        assert!(calculate_shanten(&counts_of(&concealed)).min() > 0);
    }

    #[test]
    fn tenpai_hand_has_no_decomposition() {
        let mut source = TileIdSource::new();
        let concealed = source.tiles(&[
            "1m", "2m", "3m", "4m", "5m", "6m", "7m", "8m", "9m", "1p", "2p", "3p", "5s",
        ]);

        let analysis = analyze_completed_hand(&concealed, &[]).unwrap();

        assert!(!analysis.is_complete());
        assert_eq!(calculate_shanten(&counts_of(&concealed)).min(), 0);
    }

    #[test]
    fn incomplete_hand_with_fixed_meld_has_no_decomposition() {
        let mut source = TileIdSource::new();
        let fixed = vec![source.meld(MeldKind::Pon, &["E", "E", "E"])];
        let concealed = source.tiles(&[
            "1m", "2m", "3m", "4m", "5m", "6m", "7m", "9m", "1p", "5s", "5s",
        ]);

        let analysis = analyze_completed_hand(&concealed, &fixed).unwrap();

        assert!(!analysis.is_complete());
        assert!(standard_shanten_with_fixed_melds(&counts_of(&concealed), fixed_count(1)) > -1);
    }

    #[test]
    fn too_many_fixed_melds_is_rejected() {
        let mut source = TileIdSource::new();
        let fixed: Vec<Meld> = ["E", "S", "W", "N", "P"]
            .iter()
            .map(|honor| source.meld(MeldKind::Pon, &[honor, honor, honor]))
            .collect();

        assert_eq!(
            analyze_completed_hand(&[], &fixed),
            Err(CompletedHandError::TooManyFixedMelds(5))
        );
    }

    #[test]
    fn more_than_four_copies_of_one_tile_type_is_rejected() {
        let mut source = TileIdSource::new();
        let fixed = vec![source.meld(MeldKind::Ankan, &["1m", "1m", "1m", "1m"])];
        let concealed = source.tiles(&["2m", "3m", "4m", "5p", "6p", "7p", "7s", "8s", "9s", "E"]);
        let concealed = [concealed, vec![TileId::new(0).unwrap()]].concat();

        assert_eq!(
            analyze_completed_hand(&concealed, &fixed),
            Err(CompletedHandError::TileCount(TileCountError::Overflow(
                tile_type("1m")
            )))
        );
    }
}
