use std::collections::HashMap;

use bot_logic::{
    FixedMeldCount, Meld, TileCounts, TileType, calculate_shanten_with_fixed_melds,
    structural_acceptance_tile_types_with_fixed_melds,
};

use crate::context::GameContext;
use crate::meld::fixed_meld_count;

use super::hard_safety::is_genbutsu_for;
use super::wait_candidates::remaining_tile_copies;
use super::wall::sequence_wait_routes;

/// リーチ者の隠れ手牌状態を数えられない理由。
///
/// リーチの前提と矛盾する入力を推測で補完しないための区別で、`0` 通りという結論とは別物。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HiddenHandStateUnsupported {
    /// player が範囲外で席を取得できない。
    UnknownPlayer,
    /// 対象 player がリーチしていない。
    NotReached,
    /// 副露を持つ player で、リーチ者の門前前提と矛盾する。暗槓は対象外。
    OpenMeld,
    /// 固定面子が5個以上で `FixedMeldCount` にならない。
    TooManyMelds,
}

/// 対象牌で実際にロンできる隠れ手牌状態の重み。
///
/// `weight` は条件を満たす手牌状態ごとの物理牌組み合わせ数 `Π C(remaining[t], count[t])` の総和で、
/// 放銃確率ではない。`states` は重複排除後の手牌状態そのものの個数。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct RonCapableStateWeight {
    pub weight: u128,
    pub states: u64,
}

// 牌種ごとの残枚数 (最大4枚) から k 枚を選ぶ組み合わせ数。
const BINOMIAL: [[u128; 5]; 5] = [
    [1, 0, 0, 0, 0],
    [1, 1, 0, 0, 0],
    [1, 2, 1, 0, 0],
    [1, 3, 3, 1, 0],
    [1, 4, 6, 4, 1],
];

type HandCounts = [u8; TileType::COUNT];

// target をまたいで持ち越す待ち判定 cache の上限 entry 数。target 評価の開始時にだけ確認するので、
// 1つの target 内の重複排除は上限に関係なく完全なままになる。
const EVALUATED_STATE_CAPACITY: usize = 1 << 22;

// 重複排除と待ち判定結果の cache entry。
#[derive(Debug, Clone, Copy)]
struct EvaluatedState {
    // 構造上の待ち牌種 bitmask。テンパイでない手牌は 0。
    waits: u64,
    // この手牌状態を weight へ加算済みの target 世代。
    counted_generation: u32,
}

fn tile_mask(tile: TileType) -> u64 {
    1 << tile.index()
}

// 牌種ごと3bit へ詰めた手牌状態の canonical key。34牌種 * 3bit = 102bit。
fn state_key(hand: &HandCounts) -> u128 {
    hand.iter()
        .fold(0u128, |key, &count| (key << 3) | u128::from(count))
}

fn tile_counts_of(hand: &HandCounts) -> TileCounts {
    let mut counts = TileCounts::new();
    for tile in TileType::all() {
        for _ in 0..hand[tile.index()] {
            counts.add(tile);
        }
    }
    counts
}

/// 公開情報とテンパイ条件に整合する、リーチ者の隠れ手牌状態を exact に数える prototype。
///
/// 対象は門前を保っているリーチ者1人で、暗槓による固定面子だけを許す。打牌後の concealed hand
/// 枚数は固定面子数 `m` から `13 - 3 * m` として扱い、13枚固定にしない。
///
/// 数えるのは「公開牌の残枚数と矛盾せず」「テンパイしていて」「対象牌が構造上の和了牌で」
/// 「フリテン等でロン不能になっていない」隠れ手牌状態の重み総和で、放銃確率ではない。
/// 待ち形ごとの固定 weight や behavioral prior は持たない。
///
/// 候補生成は対象牌固有の構造から行い、全牌種総当たりも物理牌 subset 総当たりもしない。
/// テンパイ判定と待ち牌集合は既存の shanten / acceptance を source of truth とする。
/// 同じ `TileCounts` は decomposition 数によらず1回だけ加算する。
///
/// 待ち判定結果は target 間で共有する cache に載るため、同じ player の複数 target を順に評価する
/// 場合は同じ instance を使い回す。
pub struct ReachedHiddenHandStates<'a> {
    context: &'a GameContext,
    fixed_melds: FixedMeldCount,
    concealed_hand_len: u8,
    remaining: HandCounts,
    unron_mask: u64,
    complete_melds: Vec<[TileType; 3]>,
    evaluated: HashMap<u128, EvaluatedState>,
    generation: u32,
}

impl<'a> ReachedHiddenHandStates<'a> {
    /// 対象リーチ者の公開情報から prototype を組み立てる。
    ///
    /// リーチしていない player や副露を持つ player など、リーチの前提と矛盾する入力は推測で
    /// 補完せず [`HiddenHandStateUnsupported`] を返す。
    pub fn new(
        player: usize,
        context: &'a GameContext,
    ) -> Result<Self, HiddenHandStateUnsupported> {
        let melds = context
            .melds_of(player)
            .ok_or(HiddenHandStateUnsupported::UnknownPlayer)?;
        if !context.is_reached(player) {
            return Err(HiddenHandStateUnsupported::NotReached);
        }
        if melds.iter().any(Meld::is_open) {
            return Err(HiddenHandStateUnsupported::OpenMeld);
        }
        let fixed_melds =
            fixed_meld_count(melds).ok_or(HiddenHandStateUnsupported::TooManyMelds)?;

        let mut remaining = [0u8; TileType::COUNT];
        let mut unron_mask = 0u64;
        for tile in TileType::all() {
            remaining[tile.index()] = remaining_tile_copies(tile, context);
            if is_genbutsu_for(tile, player, context) {
                unron_mask |= tile_mask(tile);
            }
        }

        let complete_melds = feasible_complete_melds(&remaining);

        Ok(Self {
            context,
            fixed_melds,
            concealed_hand_len: 13 - 3 * fixed_melds.get(),
            remaining,
            unron_mask,
            complete_melds,
            evaluated: HashMap::new(),
            generation: 0,
        })
    }

    /// 対象リーチ者の固定面子数。
    pub fn fixed_meld_count(&self) -> FixedMeldCount {
        self.fixed_melds
    }

    /// 打牌後の concealed hand 枚数 (`13 - 3 * 固定面子数`)。
    pub fn concealed_hand_len(&self) -> u8 {
        self.concealed_hand_len
    }

    /// 対象 player 自身の河、またはリーチ後に通った牌としてロン不能な牌種か。
    pub fn is_unron_capable_tile(&self, tile: TileType) -> bool {
        self.unron_mask & tile_mask(tile) != 0
    }

    /// 評価対象の `GameContext`。
    pub fn context(&self) -> &GameContext {
        self.context
    }

    /// 待ち判定 cache に載っている隠れ手牌状態の数。計測用。
    pub fn evaluated_state_count(&self) -> usize {
        self.evaluated.len()
    }

    /// 対象牌で実際にロンできる隠れ手牌状態の重みを exact に数える。
    pub fn ron_capable_state_weight(&mut self, target: TileType) -> RonCapableStateWeight {
        let mut total = RonCapableStateWeight::default();
        if self.is_unron_capable_tile(target) {
            return total;
        }

        if self.evaluated.len() >= EVALUATED_STATE_CAPACITY {
            self.evaluated.clear();
        }
        self.generation += 1;
        let mut hand = [0u8; TileType::COUNT];
        self.collect_standard(&mut hand, target, &mut total);
        if !self.fixed_melds.has_melds() {
            self.collect_chiitoitsu(&mut hand, target, &mut total);
            self.collect_kokushi(&mut hand, target, &mut total);
        }
        total
    }

    fn collect_standard(
        &mut self,
        hand: &mut HandCounts,
        target: TileType,
        total: &mut RonCapableStateWeight,
    ) {
        let concealed_melds = 4 - self.fixed_melds.get();

        if self.try_add(hand, &[target]) {
            self.collect_meld_multisets(hand, concealed_melds, 0, target, total);
            remove(hand, &[target]);
        }

        if concealed_melds == 0 {
            return;
        }

        for group in incomplete_groups(target) {
            if !self.try_add(hand, &group) {
                continue;
            }
            for head in TileType::all() {
                if self.try_add(hand, &[head, head]) {
                    self.collect_meld_multisets(hand, concealed_melds - 1, 0, target, total);
                    remove(hand, &[head, head]);
                }
            }
            remove(hand, &group);
        }
    }

    // 完成面子の多重集合を index 非減少順で列挙し、並び順違いを別候補にしない。
    fn collect_meld_multisets(
        &mut self,
        hand: &mut HandCounts,
        melds_left: u8,
        start: usize,
        target: TileType,
        total: &mut RonCapableStateWeight,
    ) {
        if melds_left == 0 {
            self.record(hand, target, total);
            return;
        }
        for index in start..self.complete_melds.len() {
            let meld = self.complete_melds[index];
            if self.try_add(hand, &meld) {
                self.collect_meld_multisets(hand, melds_left - 1, index, target, total);
                remove(hand, &meld);
            }
        }
    }

    fn collect_chiitoitsu(
        &mut self,
        hand: &mut HandCounts,
        target: TileType,
        total: &mut RonCapableStateWeight,
    ) {
        if !self.try_add(hand, &[target]) {
            return;
        }
        self.collect_chiitoitsu_pairs(hand, 6, 0, target, total);
        remove(hand, &[target]);
    }

    fn collect_chiitoitsu_pairs(
        &mut self,
        hand: &mut HandCounts,
        pairs_left: usize,
        start: usize,
        target: TileType,
        total: &mut RonCapableStateWeight,
    ) {
        if pairs_left == 0 {
            self.record(hand, target, total);
            return;
        }
        let last = TileType::COUNT - pairs_left;
        for index in start..=last {
            let tile = TileType::new(index as u8).expect("index is a valid tile type");
            if tile == target || self.remaining[index] < 2 {
                continue;
            }
            hand[index] += 2;
            self.collect_chiitoitsu_pairs(hand, pairs_left - 1, index + 1, target, total);
            hand[index] -= 2;
        }
    }

    fn collect_kokushi(
        &mut self,
        hand: &mut HandCounts,
        target: TileType,
        total: &mut RonCapableStateWeight,
    ) {
        if !target.is_yaochu() {
            return;
        }
        let yaochu: Vec<TileType> = TileType::all().filter(|tile| tile.is_yaochu()).collect();

        if self.try_add(hand, &yaochu) {
            self.record(hand, target, total);
            remove(hand, &yaochu);
        }

        let others: Vec<TileType> = yaochu.into_iter().filter(|&tile| tile != target).collect();
        if !self.try_add(hand, &others) {
            return;
        }
        for &extra in &others {
            if self.try_add(hand, &[extra]) {
                self.record(hand, target, total);
                remove(hand, &[extra]);
            }
        }
        remove(hand, &others);
    }

    fn try_add(&self, hand: &mut HandCounts, tiles: &[TileType]) -> bool {
        for (position, &tile) in tiles.iter().enumerate() {
            let index = tile.index();
            if hand[index] >= self.remaining[index] {
                remove(hand, &tiles[..position]);
                return false;
            }
            hand[index] += 1;
        }
        true
    }

    fn record(&mut self, hand: &HandCounts, target: TileType, total: &mut RonCapableStateWeight) {
        let generation = self.generation;
        let key = state_key(hand);
        let evaluated = match self.evaluated.get_mut(&key) {
            Some(evaluated) => {
                if evaluated.counted_generation == generation {
                    return;
                }
                evaluated.counted_generation = generation;
                *evaluated
            }
            None => {
                let evaluated = EvaluatedState {
                    waits: self.structural_waits(hand),
                    counted_generation: generation,
                };
                self.evaluated.insert(key, evaluated);
                evaluated
            }
        };

        if evaluated.waits & tile_mask(target) == 0 || evaluated.waits & self.unron_mask != 0 {
            return;
        }
        total.weight += hand_weight(&self.remaining, hand);
        total.states += 1;
    }

    // テンパイ判定と待ち牌集合は既存 helper を source of truth とする。
    fn structural_waits(&self, hand: &HandCounts) -> u64 {
        let counts = tile_counts_of(hand);
        if calculate_shanten_with_fixed_melds(&counts, self.fixed_melds).min() != 0 {
            return 0;
        }
        structural_acceptance_tile_types_with_fixed_melds(&counts, self.fixed_melds)
            .into_iter()
            .fold(0u64, |mask, tile| mask | tile_mask(tile))
    }
}

fn remove(hand: &mut HandCounts, tiles: &[TileType]) {
    for &tile in tiles {
        hand[tile.index()] -= 1;
    }
}

fn hand_weight(remaining: &HandCounts, hand: &HandCounts) -> u128 {
    let mut weight = 1u128;
    for index in 0..TileType::COUNT {
        let count = hand[index];
        if count > 0 {
            weight *= BINOMIAL[usize::from(remaining[index])][usize::from(count)];
        }
    }
    weight
}

// 残枚数から物理的に成立し得る完成面子だけを列挙する。
fn feasible_complete_melds(remaining: &HandCounts) -> Vec<[TileType; 3]> {
    let mut melds = Vec::new();
    for tile in TileType::all() {
        if let Some(sequence) = tile.sequence()
            && sequence.iter().all(|member| remaining[member.index()] >= 1)
        {
            melds.push(sequence);
        }
        if remaining[tile.index()] >= 3 {
            melds.push([tile; 3]);
        }
    }
    melds
}

// 対象牌を完成牌とする未完成ブロック。順子経路は既存 `sequence_wait_routes` を使う。
fn incomplete_groups(target: TileType) -> Vec<[TileType; 2]> {
    let mut groups: Vec<[TileType; 2]> = sequence_wait_routes(target)
        .into_iter()
        .map(|route| route.required_tiles)
        .collect();
    groups.push([target, target]);
    groups
}

/// 対象牌で実際にロンできる隠れ手牌状態の重みを、単発評価用に求める。
///
/// 同じ player の複数 target を評価する場合は [`ReachedHiddenHandStates`] を使い回すほうが、
/// 待ち判定 cache を共有できる。
pub fn ron_capable_hidden_hand_weight(
    target: TileType,
    player: usize,
    context: &GameContext,
) -> Result<RonCapableStateWeight, HiddenHandStateUnsupported> {
    Ok(ReachedHiddenHandStates::new(player, context)?.ron_capable_state_weight(target))
}
