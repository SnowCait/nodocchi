use crate::count_hasher::CountHasherBuilder;
use crate::shanten::{
    EffectiveShanten, FixedMeldCount, MinShanten, Shanten, calculate_shanten,
    calculate_shanten_with_after_draws,
};
use crate::tile::{TileId, TileType};
use crate::tile_counts::TileCounts;
use std::collections::HashMap;

#[cfg(test)]
mod differential;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AcceptanceTile<S = Shanten> {
    pub tile: TileType,
    pub remaining: u8,
    pub shanten_after_draw: S,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Acceptance<S = Shanten> {
    pub current: S,
    pub tiles: Vec<AcceptanceTile<S>>,
}

impl<S> Acceptance<S> {
    pub fn is_empty(&self) -> bool {
        self.tiles.is_empty()
    }

    pub fn total_remaining(&self) -> u8 {
        self.tiles.iter().map(|tile| tile.remaining).sum()
    }

    /// 受け入れ牌種の一覧。残枚数 0 の牌種を含めない既存 semantics のままの「実際に残っている
    /// 受け入れ」。
    pub fn tile_types(&self) -> Vec<TileType> {
        self.tiles.iter().map(|tile| tile.tile).collect()
    }
}

impl<S: MinShanten> Acceptance<S> {
    pub fn current_min_shanten(&self) -> i8 {
        self.current.min_shanten()
    }
}

pub type EffectiveAcceptance = Acceptance<EffectiveShanten>;
pub type EffectiveAcceptanceTile = AcceptanceTile<EffectiveShanten>;

pub fn calculate_acceptance(counts: &TileCounts) -> Acceptance {
    calculate_acceptance_with_seen(counts, &[0; TileType::COUNT])
}

pub fn calculate_acceptance_with_visible_tiles(
    counts: &TileCounts,
    visible_tiles: &[TileId],
) -> Acceptance {
    calculate_acceptance_with_seen(counts, &additional_seen(counts, visible_tiles))
}

pub fn calculate_acceptance_with_fixed_melds(
    counts: &TileCounts,
    fixed_meld_count: FixedMeldCount,
) -> EffectiveAcceptance {
    calculate_acceptance_with_fixed_melds_and_seen(counts, fixed_meld_count, &[0; TileType::COUNT])
}

pub fn calculate_acceptance_with_fixed_melds_and_visible_tiles(
    counts: &TileCounts,
    fixed_meld_count: FixedMeldCount,
    visible_tiles: &[TileId],
) -> EffectiveAcceptance {
    calculate_acceptance_with_fixed_melds_and_seen(
        counts,
        fixed_meld_count,
        &additional_seen(counts, visible_tiles),
    )
}

/// 見え牌を反映しない「構造上の受け入れ牌種」。
///
/// 判定は既存の受け入れと同じ「牌種を1枚足すと向聴が下がるか」で、見え牌による残枚数の絞り込み
/// だけを行わない。したがって4枚とも他家に見えている牌種も含む。
///
/// 恒常フリテンのように「山や他家にその牌が残っているか」で結論が変わらない判定へ渡すためのもの。
/// [`Acceptance`] が残枚数 0 の牌種を受け入れに含めない semantics 自体は変えない。
///
/// 手牌に4枚持っている牌種は5枚目が存在せずアガリ牌になり得ないため、見え牌が無くても含めない。
pub fn structural_acceptance_tile_types(counts: &TileCounts) -> Vec<TileType> {
    calculate_acceptance(counts).tile_types()
}

/// 副露済み面子数を考慮した構造上の受け入れ牌種。
///
/// 判定の共有と見え牌の扱いは [`structural_acceptance_tile_types`] と同じ。
pub fn structural_acceptance_tile_types_with_fixed_melds(
    counts: &TileCounts,
    fixed_meld_count: FixedMeldCount,
) -> Vec<TileType> {
    calculate_acceptance_with_fixed_melds(counts, fixed_meld_count).tile_types()
}

pub(crate) fn calculate_acceptance_with_seen(
    counts: &TileCounts,
    additional_seen: &[u8; TileType::COUNT],
) -> Acceptance {
    collect_acceptance(counts, additional_seen, calculate_shanten, Shanten::min)
}

/// 手牌以外に見えている枚数を直接渡して、副露済み面子数を考慮した受け入れを求める。
///
/// 打牌候補評価のように「公開牌 + 今から切る候補牌1枚」を seen として扱う経路と、残枚数計算を
/// 共有するための crate-private helper。同じ残枚数計算を呼び出し側へ複製しないこと。
pub(crate) fn calculate_acceptance_with_fixed_melds_and_seen(
    counts: &TileCounts,
    fixed_meld_count: FixedMeldCount,
    additional_seen: &[u8; TileType::COUNT],
) -> EffectiveAcceptance {
    let skeleton = acceptance_skeleton(counts, fixed_meld_count);
    let current_min = skeleton.current.min();
    let mut tiles = Vec::new();

    skeleton.for_each_drawable_tile(
        counts,
        additional_seen,
        |tile, remaining, shanten_after_draw| {
            // 受け入れの条件は「その牌を1枚加えると向聴数が下がる」。維持する牌を混ぜない。
            if shanten_after_draw.min() < current_min {
                tiles.push(AcceptanceTile {
                    tile,
                    remaining,
                    shanten_after_draw,
                });
            }
        },
    );

    Acceptance {
        current: skeleton.current,
        tiles,
    }
}

/// 手牌以外に見えている枚数を visible tiles と手牌から求める。
///
/// visible tiles は自分の手牌を含むため、手牌分を差し引いて二重計上を防ぐ。打牌候補評価のように
/// visible tiles から seen を組み立てる経路と計算を共有するための crate-private helper。
pub(crate) fn additional_seen(
    counts: &TileCounts,
    visible_tiles: &[TileId],
) -> [u8; TileType::COUNT] {
    let visible_counts = TileCounts::from_tiles(visible_tiles.iter().copied());
    let mut additional_seen = [0u8; TileType::COUNT];
    for tile in TileType::all() {
        additional_seen[tile.index()] = visible_counts
            .count(tile)
            .saturating_sub(counts.count(tile));
    }
    additional_seen
}

/// 見え牌を反映して実際にツモり得る牌1牌種分の、ツモ後の向聴数付きの列挙結果。
///
/// 受け入れ ([`Acceptance`]) は「その牌を1枚加えると向聴数が下がる牌」だけを持つ source of
/// truth なので、向聴数を維持する牌をそこへ混ぜない。2手先評価 ([`crate::lookahead`]) が仮想
/// ツモ対象を列挙するための型で、判定条件は列挙する側が持つ。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct DrawableTile {
    pub(crate) tile: TileType,
    /// 見え牌を反映した残枚数。残枚数 0 の牌種は列挙しない。
    pub(crate) remaining: u8,
    pub(crate) shanten_after_draw: EffectiveShanten,
}

/// 見え牌を反映して実際にツモり得る牌のうち、向聴数を維持する牌を列挙する。
///
/// 残枚数もツモ後の向聴数も受け入れと同じ列挙 ([`for_each_drawable_tile`]) から求め、条件だけが
/// 「下がる (`<`)」ではなく「維持する (`==`)」になる。向聴数が悪化する牌は対象外。
/// 受け入れの条件を変えないため、ここで列挙した牌が [`Acceptance`] へ入ることはない。
pub(crate) fn same_shanten_draws_with_fixed_melds_and_seen(
    counts: &TileCounts,
    fixed_meld_count: FixedMeldCount,
    additional_seen: &[u8; TileType::COUNT],
) -> Vec<DrawableTile> {
    let skeleton = acceptance_skeleton(counts, fixed_meld_count);
    let current_min = skeleton.current.min();
    let mut tiles = Vec::new();

    skeleton.for_each_drawable_tile(
        counts,
        additional_seen,
        |tile, remaining, shanten_after_draw| {
            if shanten_after_draw.min() == current_min {
                tiles.push(DrawableTile {
                    tile,
                    remaining,
                    shanten_after_draw,
                });
            }
        },
    );

    tiles
}

/// 見え牌によらない受け入れの骨格。現在の向聴数と、牌種を1枚加えた後の向聴数だけを持つ。
///
/// 受け入れのうち見え牌で変わるのは残枚数 ([`remaining_copies`]) だけで、向聴数はどれも手牌と
/// 副露済み面子数だけで決まる。2手先評価は同じ手牌を経路ごとに違う見え牌で何度も評価するため、
/// 向聴数探索をこの単位で共有する。受け入れの判定条件も列挙する牌も骨格の外にあるので、共有の
/// 有無で受け入れそのものは変わらない。
#[derive(Clone, Copy)]
struct AcceptanceSkeleton {
    current: EffectiveShanten,
    // 牌種を1枚加えられる場合だけ、加えた後の向聴数を持つ。
    after_draw: [Option<EffectiveShanten>; TileType::COUNT],
}

impl AcceptanceSkeleton {
    fn calculate(counts: &TileCounts, fixed_meld_count: FixedMeldCount) -> Self {
        // 現在の向聴数とツモ後の向聴数は必ず揃って要るので、向聴計算側でまとめて求める。骨格は
        // その結果を持つだけで、向聴数の規則も牌種の絞り込みも持たない。
        let batch = calculate_shanten_with_after_draws(counts, fixed_meld_count);

        Self {
            current: batch.current,
            after_draw: batch.after_draw,
        }
    }

    // 見え牌を反映して実際にツモり得る牌を1牌種ずつ、その牌を1枚加えた後の向聴数と一緒に渡す。
    // 除外する条件も渡す値も [`for_each_drawable_tile`] と同じで、向聴数だけを骨格から取る。
    fn for_each_drawable_tile(
        &self,
        counts: &TileCounts,
        additional_seen: &[u8; TileType::COUNT],
        mut visit: impl FnMut(TileType, u8, EffectiveShanten),
    ) {
        for tile in TileType::all() {
            let remaining = remaining_copies(counts, additional_seen, tile);
            if remaining == 0 {
                continue;
            }

            let Some(shanten_after_draw) = self.after_draw[tile.index()] else {
                continue;
            };

            visit(tile, remaining, shanten_after_draw);
        }
    }
}

type SkeletonMemo =
    HashMap<([u8; TileType::COUNT], FixedMeldCount), AcceptanceSkeleton, CountHasherBuilder>;

// 骨格を保持する上限エントリ数。超えたら丸ごと捨てて使用量を上限内に保つ。
const SKELETON_MEMO_CAPACITY: usize = 1 << 17;

// 見え牌によらない受け入れの骨格。
//
// [`AcceptanceSkeleton::calculate`] は (手牌, 副露済み面子数) に対する純関数で、結果は memo の
// 中身に依存しない。2手先評価は同じ手牌を経路ごとに違う見え牌で何度も評価するため、呼び出し
// ごとに向聴数探索をやり直さずスレッドローカルで使い回す。受け入れそのものは変わらない。
thread_local! {
    static SKELETON_MEMO: std::cell::RefCell<SkeletonMemo> =
        std::cell::RefCell::new(SkeletonMemo::default());
}

fn acceptance_skeleton(
    counts: &TileCounts,
    fixed_meld_count: FixedMeldCount,
) -> AcceptanceSkeleton {
    let key = (*counts.as_array(), fixed_meld_count);
    if let Some(cached) = SKELETON_MEMO.with_borrow(|memo| memo.get(&key).copied()) {
        return cached;
    }

    let skeleton = AcceptanceSkeleton::calculate(counts, fixed_meld_count);
    SKELETON_MEMO.with_borrow_mut(|memo| {
        if memo.len() >= SKELETON_MEMO_CAPACITY {
            memo.clear();
        }
        memo.insert(key, skeleton);
    });
    skeleton
}

fn collect_acceptance<S: Copy>(
    counts: &TileCounts,
    additional_seen: &[u8; TileType::COUNT],
    evaluate: impl Fn(&TileCounts) -> S,
    effective: impl Fn(S) -> i8,
) -> Acceptance<S> {
    let current = evaluate(counts);
    let current_min = effective(current);
    let mut tiles = Vec::new();

    for_each_drawable_tile(
        counts,
        additional_seen,
        &evaluate,
        |tile, remaining, shanten_after_draw| {
            // 受け入れの条件は「その牌を1枚加えると向聴数が下がる」。維持する牌を混ぜない。
            if effective(shanten_after_draw) < current_min {
                tiles.push(AcceptanceTile {
                    tile,
                    remaining,
                    shanten_after_draw,
                });
            }
        },
    );

    Acceptance { current, tiles }
}

/// 見え牌を反映した、まだ自分から見えていない物理牌の総数。
///
/// 牌種ごとの残枚数の合計そのもので、受け入れの残枚数と同じ数え方
/// ([`remaining_copies`]) を共有する。山の残枚数ではなく、自分がまだ物理的に確認していない牌の
/// 枚数を表す。
pub(crate) fn unknown_tile_count(
    counts: &TileCounts,
    additional_seen: &[u8; TileType::COUNT],
) -> u32 {
    TileType::all()
        .map(|tile| u32::from(remaining_copies(counts, additional_seen, tile)))
        .sum()
}

// 牌種1つ分の残枚数 (4枚 - 手牌 - 手牌以外の見え牌)。受け入れ・2手先評価の仮想ツモ候補・
// 未確認牌の総数はこの1本を共有し、残枚数の数え方を複製しない。
fn remaining_copies(
    counts: &TileCounts,
    additional_seen: &[u8; TileType::COUNT],
    tile: TileType,
) -> u8 {
    4u8.saturating_sub(counts.count(tile) + additional_seen[tile.index()])
}

// 見え牌を反映して実際にツモり得る牌を1牌種ずつ、その牌を1枚加えた後の向聴数と一緒に渡す。
//
// 1枚加えた後の向聴数の求め方はここ1本だけが持ち、受け入れと2手先評価の仮想ツモ候補で
// 共有する。どの牌を採用するかは呼び出し側の条件で決める。
fn for_each_drawable_tile<S>(
    counts: &TileCounts,
    additional_seen: &[u8; TileType::COUNT],
    evaluate: impl Fn(&TileCounts) -> S,
    mut visit: impl FnMut(TileType, u8, S),
) {
    for tile in TileType::all() {
        let remaining = remaining_copies(counts, additional_seen, tile);
        if remaining == 0 {
            continue;
        }

        let mut added = *counts;
        if added.try_add(tile).is_err() {
            continue;
        }

        visit(tile, remaining, evaluate(&added));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shanten::calculate_shanten_with_fixed_melds;

    fn tile(s: &str) -> TileType {
        TileType::from_mjai_type_str(s).unwrap()
    }

    fn counts(strings: &[&str]) -> TileCounts {
        TileCounts::from_tile_types(strings.iter().map(|s| tile(s)))
    }

    fn accepted_tiles(acceptance: &Acceptance) -> Vec<TileType> {
        acceptance.tiles.iter().map(|entry| entry.tile).collect()
    }

    fn ids(values: &[u8]) -> Vec<TileId> {
        values.iter().map(|&v| TileId::new(v).unwrap()).collect()
    }

    fn remaining_of(acceptance: &Acceptance, wait: TileType) -> Option<u8> {
        acceptance
            .tiles
            .iter()
            .find(|entry| entry.tile == wait)
            .map(|entry| entry.remaining)
    }

    // 13m 68m 456789p 5s EE の1向聴。4m は向聴数を維持し、2m / 7m は向聴数を下げる。
    fn same_shanten_counts() -> TileCounts {
        counts(&[
            "1m", "3m", "6m", "8m", "4p", "5p", "6p", "7p", "8p", "9p", "5s", "E", "E",
        ])
    }

    #[test]
    fn same_shanten_draws_share_the_acceptance_calculation_with_a_different_condition() {
        let counts = same_shanten_counts();
        let fixed_meld_count = FixedMeldCount::NONE;
        let seen = [0u8; TileType::COUNT];
        let acceptance = calculate_acceptance_with_fixed_melds(&counts, fixed_meld_count);
        let current = acceptance.current.min();
        let draws = same_shanten_draws_with_fixed_melds_and_seen(&counts, fixed_meld_count, &seen);

        for tile in TileType::all() {
            let listed = draws.iter().find(|drawable| drawable.tile == tile);
            let accepted = acceptance.tiles.iter().find(|entry| entry.tile == tile);

            let mut added = counts;
            let remaining = 4u8.saturating_sub(counts.count(tile));
            if remaining == 0 || added.try_add(tile).is_err() {
                assert_eq!(listed, None, "{tile:?}");
                assert!(accepted.is_none(), "{tile:?}");
                continue;
            }

            let after_draw = calculate_shanten_with_fixed_melds(&added, fixed_meld_count);
            // 受け入れは「下がる」、lookahead 専用の列挙は「維持する」。同じ shanten calculator の
            // 結果を同じ牌へ適用して、条件だけが違う。
            assert_eq!(listed.is_some(), after_draw.min() == current, "{tile:?}");
            assert_eq!(accepted.is_some(), after_draw.min() < current, "{tile:?}");
            if let Some(listed) = listed {
                assert_eq!(listed.remaining, remaining);
                assert_eq!(listed.shanten_after_draw, after_draw);
            }
        }

        assert!(draws.iter().any(|drawable| drawable.tile == tile("4m")));
        assert_eq!(
            accepted_tiles(&calculate_acceptance(&counts)),
            vec![tile("2m"), tile("7m")]
        );
    }

    #[test]
    fn same_shanten_draws_skip_the_tiles_that_are_all_seen() {
        let counts = same_shanten_counts();
        let fixed_meld_count = FixedMeldCount::NONE;
        let mut seen = [0u8; TileType::COUNT];
        seen[tile("4m").index()] = 4;

        let draws = same_shanten_draws_with_fixed_melds_and_seen(&counts, fixed_meld_count, &seen);
        assert!(!draws.iter().any(|drawable| drawable.tile == tile("4m")));
        assert!(draws.iter().all(|drawable| drawable.remaining > 0));
    }

    #[test]
    fn empty_hand_has_no_acceptance() {
        let acceptance = calculate_acceptance(&TileCounts::new());
        assert_eq!(acceptance.current.min(), 8);
        assert!(acceptance.is_empty());
        assert_eq!(acceptance.total_remaining(), 0);
    }

    #[test]
    fn standard_tenpai_accepts_winning_tile() {
        let counts = counts(&[
            "1m", "2m", "3m", "4m", "5m", "6m", "7m", "8m", "9m", "1p", "2p", "3p", "5s",
        ]);
        let acceptance = calculate_acceptance(&counts);
        assert_eq!(acceptance.current.min(), 0);
        assert_eq!(accepted_tiles(&acceptance), vec![tile("5s")]);
        assert_eq!(acceptance.tiles[0].shanten_after_draw.min(), -1);
        assert_eq!(acceptance.tiles[0].remaining, 3);
        assert_eq!(acceptance.total_remaining(), 3);
    }

    #[test]
    fn chiitoitsu_tenpai_accepts_pair_tile() {
        let counts = counts(&[
            "1m", "1m", "2m", "2m", "3m", "3m", "4p", "4p", "5p", "5p", "6s", "6s", "E",
        ]);
        let acceptance = calculate_acceptance(&counts);
        assert_eq!(acceptance.current.min(), 0);
        let east = acceptance
            .tiles
            .iter()
            .find(|entry| entry.tile == tile("E"))
            .expect("E should be accepted");
        assert_eq!(east.shanten_after_draw.min(), -1);
        assert_eq!(east.remaining, 3);
    }

    #[test]
    fn kokushi_thirteen_wait_accepts_thirteen_tiles() {
        let counts = counts(&[
            "1m", "9m", "1p", "9p", "1s", "9s", "E", "S", "W", "N", "P", "F", "C",
        ]);
        let acceptance = calculate_acceptance(&counts);
        assert_eq!(acceptance.current.min(), 0);
        let expected: Vec<TileType> = [
            "1m", "9m", "1p", "9p", "1s", "9s", "E", "S", "W", "N", "P", "F", "C",
        ]
        .iter()
        .map(|s| tile(s))
        .collect();
        assert_eq!(accepted_tiles(&acceptance), expected);
        assert!(
            acceptance
                .tiles
                .iter()
                .all(|entry| entry.shanten_after_draw.min() == -1)
        );
        assert!(acceptance.tiles.iter().all(|entry| entry.remaining == 3));
        assert_eq!(acceptance.total_remaining(), 39);
    }

    #[test]
    fn tile_with_no_remaining_is_excluded() {
        let counts = counts(&["1m", "1m", "1m", "1m"]);
        assert_eq!(counts.remaining_count(tile("1m")), 0);
        let acceptance = calculate_acceptance(&counts);
        let tiles = accepted_tiles(&acceptance);
        assert!(!tiles.contains(&tile("1m")));
        assert!(tiles.contains(&tile("2m")));
    }

    #[test]
    fn does_not_modify_input_counts() {
        let counts = counts(&[
            "1m", "2m", "3m", "4m", "5m", "6m", "7m", "8m", "9m", "1p", "2p", "3p", "5s",
        ]);
        let before = counts;
        let _ = calculate_acceptance(&counts);
        assert_eq!(counts, before);
    }

    #[test]
    fn visible_empty_matches_plain_acceptance() {
        let hand = ids(&[0, 4, 8, 12, 17, 20, 24, 28, 32, 36, 40, 44, 89]);
        let counts = TileCounts::from_tiles(hand.iter().copied());
        assert_eq!(
            calculate_acceptance_with_visible_tiles(&counts, &[]),
            calculate_acceptance(&counts)
        );
    }

    #[test]
    fn visible_does_not_double_count_own_hand() {
        let hand = ids(&[0, 4, 8, 12, 17, 20, 24, 28, 32, 36, 40, 44, 89]);
        let counts = TileCounts::from_tiles(hand.iter().copied());
        let acceptance = calculate_acceptance_with_visible_tiles(&counts, &hand);
        assert_eq!(remaining_of(&acceptance, tile("5s")), Some(3));
        assert_eq!(acceptance.total_remaining(), 3);
    }

    #[test]
    fn visible_wait_tile_reduces_remaining_by_one() {
        let hand = ids(&[0, 4, 8, 12, 17, 20, 24, 28, 32, 36, 40, 44, 89]);
        let counts = TileCounts::from_tiles(hand.iter().copied());
        let mut visible = hand.clone();
        visible.extend(ids(&[90]));
        let acceptance = calculate_acceptance_with_visible_tiles(&counts, &visible);
        assert_eq!(remaining_of(&acceptance, tile("5s")), Some(2));
    }

    #[test]
    fn visible_removes_wait_when_all_copies_seen() {
        let hand = ids(&[0, 4, 8, 12, 17, 20, 24, 28, 32, 36, 40, 44, 89]);
        let counts = TileCounts::from_tiles(hand.iter().copied());
        let mut visible = hand.clone();
        visible.extend(ids(&[88, 90, 91]));
        let acceptance = calculate_acceptance_with_visible_tiles(&counts, &visible);
        assert_eq!(remaining_of(&acceptance, tile("5s")), None);
        assert_eq!(acceptance.total_remaining(), 0);
        assert_eq!(acceptance.current.min(), 0);
    }

    #[test]
    fn visible_does_not_apply_candidate_discard_correction() {
        let hand = ids(&[0, 4, 8, 12, 17, 20, 24, 28, 32, 36, 40, 44, 89]);
        let counts = TileCounts::from_tiles(hand.iter().copied());
        let plain = calculate_acceptance(&counts);
        let visible = calculate_acceptance_with_visible_tiles(&counts, &hand);
        assert_eq!(
            remaining_of(&visible, tile("5s")),
            remaining_of(&plain, tile("5s"))
        );
    }

    fn fixed(value: u8) -> FixedMeldCount {
        FixedMeldCount::new(value).unwrap()
    }

    fn effective_accepted_tiles(acceptance: &EffectiveAcceptance) -> Vec<TileType> {
        acceptance.tiles.iter().map(|entry| entry.tile).collect()
    }

    fn effective_remaining_of(acceptance: &EffectiveAcceptance, wait: TileType) -> Option<u8> {
        acceptance
            .tiles
            .iter()
            .find(|entry| entry.tile == wait)
            .map(|entry| entry.remaining)
    }

    fn one_meld_tenpai_hand() -> Vec<TileId> {
        ids(&[0, 4, 8, 12, 17, 20, 24, 28, 32, 53])
    }

    #[test]
    fn one_fixed_meld_tenpai_accepts_only_winning_tile() {
        let counts = counts(&["1m", "2m", "3m", "4m", "5m", "6m", "7m", "8m", "9m", "5p"]);
        let acceptance = calculate_acceptance_with_fixed_melds(&counts, fixed(1));

        assert_eq!(acceptance.current.min(), 0);
        assert_eq!(effective_accepted_tiles(&acceptance), vec![tile("5p")]);
        assert_eq!(acceptance.tiles[0].shanten_after_draw.min(), -1);
        assert_eq!(acceptance.tiles[0].remaining, 3);
        assert_eq!(acceptance.total_remaining(), 3);
    }

    #[test]
    fn fixed_meld_acceptance_keeps_effective_shanten_standard_only() {
        let counts = counts(&["1m", "9m", "1p", "9p", "1s", "9s", "E", "S", "W", "N"]);
        let acceptance = calculate_acceptance_with_fixed_melds(&counts, fixed(1));

        assert_eq!(acceptance.current.min(), 6);
        assert_eq!(acceptance.current.concealed(), None);
        assert!(
            acceptance
                .tiles
                .iter()
                .all(|entry| entry.shanten_after_draw.concealed().is_none())
        );
        assert!(!effective_accepted_tiles(&acceptance).contains(&tile("C")));
    }

    #[test]
    fn fixed_meld_visible_tiles_reduce_remaining() {
        let hand = one_meld_tenpai_hand();
        let counts = TileCounts::from_tiles(hand.iter().copied());
        let mut visible = hand.clone();
        visible.extend(ids(&[54, 55]));

        let acceptance =
            calculate_acceptance_with_fixed_melds_and_visible_tiles(&counts, fixed(1), &visible);

        assert_eq!(acceptance.current.min(), 0);
        assert_eq!(effective_accepted_tiles(&acceptance), vec![tile("5p")]);
        assert_eq!(effective_remaining_of(&acceptance, tile("5p")), Some(1));
        assert_eq!(acceptance.total_remaining(), 1);
    }

    #[test]
    fn fixed_meld_visible_tiles_do_not_double_count_own_hand() {
        let hand = one_meld_tenpai_hand();
        let counts = TileCounts::from_tiles(hand.iter().copied());

        assert_eq!(
            calculate_acceptance_with_fixed_melds_and_visible_tiles(&counts, fixed(1), &hand),
            calculate_acceptance_with_fixed_melds(&counts, fixed(1))
        );
        assert_eq!(
            calculate_acceptance_with_fixed_melds_and_visible_tiles(&counts, fixed(1), &[]),
            calculate_acceptance_with_fixed_melds(&counts, fixed(1))
        );
    }

    #[test]
    fn zero_fixed_melds_matches_concealed_acceptance() {
        let hands = [
            counts(&[
                "1m", "2m", "3m", "4m", "5m", "6m", "7m", "8m", "9m", "1p", "2p", "3p", "5s",
            ]),
            counts(&[
                "1m", "1m", "2m", "2m", "3m", "3m", "4p", "4p", "5p", "5p", "6s", "6s", "E",
            ]),
            counts(&[
                "1m", "9m", "1p", "9p", "1s", "9s", "E", "S", "W", "N", "P", "F", "C",
            ]),
            counts(&[
                "1m", "2m", "3m", "4m", "5m", "6m", "7m", "8m", "9m", "1p", "5s", "E", "E",
            ]),
        ];

        for hand in hands {
            let expected = calculate_acceptance(&hand);
            let actual = calculate_acceptance_with_fixed_melds(&hand, FixedMeldCount::NONE);

            assert_eq!(actual.current.concealed(), Some(expected.current));
            assert_eq!(effective_accepted_tiles(&actual), accepted_tiles(&expected));
            assert_eq!(actual.total_remaining(), expected.total_remaining());
            for (actual_tile, expected_tile) in actual.tiles.iter().zip(expected.tiles.iter()) {
                assert_eq!(actual_tile.remaining, expected_tile.remaining);
                assert_eq!(
                    actual_tile.shanten_after_draw.concealed(),
                    Some(expected_tile.shanten_after_draw)
                );
            }
        }
    }

    #[test]
    fn zero_fixed_melds_visible_tiles_match_concealed_acceptance() {
        let hand = ids(&[0, 4, 8, 12, 17, 20, 24, 28, 32, 36, 40, 44, 89]);
        let counts = TileCounts::from_tiles(hand.iter().copied());
        let mut visible = hand.clone();
        visible.extend(ids(&[90]));

        let expected = calculate_acceptance_with_visible_tiles(&counts, &visible);
        let actual = calculate_acceptance_with_fixed_melds_and_visible_tiles(
            &counts,
            FixedMeldCount::NONE,
            &visible,
        );

        assert_eq!(actual.current.concealed(), Some(expected.current));
        assert_eq!(effective_accepted_tiles(&actual), accepted_tiles(&expected));
        assert_eq!(
            effective_remaining_of(&actual, tile("5s")),
            remaining_of(&expected, tile("5s"))
        );
    }

    #[test]
    fn structural_tile_types_ignore_visible_tiles() {
        // 待ちが4枚とも見えていても構造上のアガリ牌種からは消えない。既存受け入れは従来どおり
        // 残枚数 0 の牌種を含めない。
        let hand = ids(&[0, 4, 8, 12, 17, 20, 24, 28, 32, 36, 40, 44, 89]);
        let counts = TileCounts::from_tiles(hand.iter().copied());
        let mut visible = hand.clone();
        visible.extend(ids(&[88, 90, 91]));

        assert_eq!(structural_acceptance_tile_types(&counts), vec![tile("5s")]);
        assert!(
            calculate_acceptance_with_visible_tiles(&counts, &visible)
                .tiles
                .is_empty()
        );
    }

    #[test]
    fn structural_tile_types_match_the_acceptance_without_visible_tiles() {
        let hands = [
            counts(&[
                "1m", "2m", "3m", "4m", "5m", "6m", "7m", "8m", "9m", "1p", "2p", "3p", "5s",
            ]),
            counts(&[
                "1m", "1m", "2m", "2m", "3m", "3m", "4p", "4p", "5p", "5p", "6s", "6s", "E",
            ]),
            counts(&[
                "1m", "9m", "1p", "9p", "1s", "9s", "E", "S", "W", "N", "P", "F", "C",
            ]),
        ];

        for hand in hands {
            assert_eq!(
                structural_acceptance_tile_types(&hand),
                accepted_tiles(&calculate_acceptance(&hand))
            );
            assert_eq!(
                structural_acceptance_tile_types_with_fixed_melds(&hand, FixedMeldCount::NONE),
                structural_acceptance_tile_types(&hand)
            );
        }
    }

    #[test]
    fn structural_tile_types_exclude_a_tile_type_held_four_times() {
        // 手牌に4枚ある牌種は5枚目が存在せず、見え牌が無くてもアガリ牌になり得ない。
        let counts = counts(&[
            "1m", "1m", "1m", "1m", "2m", "3m", "4m", "5m", "6m", "7m", "8m", "9m", "9m",
        ]);
        assert!(!structural_acceptance_tile_types(&counts).contains(&tile("1m")));
    }

    #[test]
    fn structural_tile_types_with_fixed_melds_use_the_effective_acceptance() {
        let counts = counts(&["1m", "2m", "3m", "4m", "5m", "6m", "7m", "8m", "9m", "5p"]);
        assert_eq!(
            structural_acceptance_tile_types_with_fixed_melds(&counts, fixed(1)),
            vec![tile("5p")]
        );
    }

    #[test]
    fn tile_types_expose_the_live_acceptance_tiles() {
        let counts = counts(&[
            "1m", "2m", "3m", "4m", "5m", "6m", "7m", "8m", "9m", "1p", "2p", "3p", "5s",
        ]);
        let acceptance = calculate_acceptance(&counts);
        assert_eq!(acceptance.tile_types(), accepted_tiles(&acceptance));
    }

    #[test]
    fn tiles_are_ordered_by_tile_type() {
        let counts = counts(&[
            "1m", "2m", "3m", "4m", "5m", "6m", "7m", "8m", "9m", "2p", "3p", "5s", "5s",
        ]);
        let acceptance = calculate_acceptance(&counts);
        assert_eq!(accepted_tiles(&acceptance), vec![tile("1p"), tile("4p")]);
        assert!(
            acceptance
                .tiles
                .windows(2)
                .all(|pair| pair[0].tile.raw() < pair[1].tile.raw())
        );
    }
}
