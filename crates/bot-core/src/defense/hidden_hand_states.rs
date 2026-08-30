use std::cell::Cell;
use std::collections::HashMap;
use std::time::{Duration, Instant};

use bot_logic::{FixedMeldCount, Meld, TileCounts, TileId, TileType, analyze_completed_hand};

use crate::context::GameContext;
use crate::meld::fixed_meld_count;

use super::hard_safety::is_genbutsu_for;
use super::wait_candidates::remaining_tile_copies;
use super::wall::sequence_wait_routes;

/// exact hidden-hand model で隠れ手牌状態を数えられない理由。
///
/// 選択した model の前提と矛盾する入力を推測で補完しないための区別で、`0` 通りという結論とは
/// 別物。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HiddenHandStateUnsupported {
    /// player が範囲外で席を取得できない。
    UnknownPlayer,
    /// 対象 player がリーチしていない。
    NotReached,
    /// conditional-tenpai model の対象 player が既にリーチしている。
    Reached,
    /// 副露を持つ player で、リーチ者の門前前提と矛盾する。暗槓は対象外。
    OpenMeld,
    /// conditional-tenpai model の対象 player に公開副露がない。
    NoOpenMeld,
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

/// 対象牌を加えると structural completion になる隠れ手牌状態の重み。
///
/// `weight` は条件を満たす手牌状態ごとの物理牌組み合わせ数
/// `Π C(remaining[t], count[t])` の総和で、放銃確率でもロン可能 weight でもない。
/// `states` は重複排除後の手牌状態そのものの個数。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct StructuralCompletionStateWeight {
    pub weight: u128,
    pub states: u64,
}

impl From<RonCapableStateWeight> for StructuralCompletionStateWeight {
    fn from(value: RonCapableStateWeight) -> Self {
        Self {
            weight: value.weight,
            states: value.states,
        }
    }
}

/// prototype の内訳計測値。数え上げ結果には影響しない。
///
/// `candidate_generation` は候補生成と重複排除、`unron_filtering` はロン不能牌との交差判定、
/// `target_completion` は対象牌の和了判定に費やした累積時間。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct HiddenHandStateMetrics {
    pub generated_candidates: u64,
    pub evaluated_states: u64,
    pub completion_checks: u64,
    pub candidate_generation: Duration,
    pub unron_filtering: Duration,
    pub target_completion: Duration,
}

// 牌種ごとの残枚数 (最大4枚) から k 枚を選ぶ組み合わせ数。
const BINOMIAL: [[u128; 5]; 5] = [
    [1, 0, 0, 0, 0],
    [1, 1, 0, 0, 0],
    [1, 2, 1, 0, 0],
    [1, 3, 3, 1, 0],
    [1, 4, 6, 4, 1],
];

const MAX_TILE_COPIES: u8 = 4;

// 和了形の枚数。隠れ手牌 (13 - 3 * 固定面子数) に和了牌1枚を足した上限。
const COMPLETED_HAND_LEN: usize = 14;

type HandCounts = [u8; TileType::COUNT];

// target をまたいで持ち越す判定 cache の上限 entry 数。target 評価の開始時にだけ確認するので、
// 1つの target 内の重複排除は上限に関係なく完全なままになる。
const EVALUATED_STATE_CAPACITY: usize = 1 << 22;

// 重複排除とロン不能牌判定結果の cache entry。
//
// ロン不能牌との交差判定は target に依存しないので、同じ player の複数 target で使い回せる。
#[derive(Debug, Clone, Copy)]
struct EvaluatedState {
    waits_on_unron_tile: bool,
    // この手牌状態を評価済みの target 世代。
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

fn tile_id(tile: TileType, copy: u8) -> TileId {
    TileId::new(tile.raw() * MAX_TILE_COPIES + copy).expect("at most four copies per tile type")
}

// 手牌状態を物理牌 id 列にする。牌種ごとに使っていない copy index から順に割り当てる。
fn concealed_tile_ids(hand: &HandCounts) -> Vec<TileId> {
    let mut ids = Vec::with_capacity(COMPLETED_HAND_LEN);
    for tile in TileType::all() {
        for copy in 0..hand[tile.index()] {
            ids.push(tile_id(tile, copy));
        }
    }
    ids
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum HiddenHandModelMode {
    Riichi,
    OpenHandStructuralTenpai,
}

pub(super) struct HiddenHandModelInput<'a> {
    pub(super) context: &'a GameContext,
    pub(super) fixed_melds: &'a [Meld],
    pub(super) fixed_meld_count: FixedMeldCount,
    pub(super) concealed_hand_len: u8,
    pub(super) remaining: HandCounts,
    pub(super) unron_mask: u64,
    pub(super) unron_tiles: Vec<TileType>,
}

impl<'a> HiddenHandModelInput<'a> {
    pub(super) fn new(
        player: usize,
        context: &'a GameContext,
        mode: HiddenHandModelMode,
    ) -> Result<Self, HiddenHandStateUnsupported> {
        let fixed_melds = context
            .melds_of(player)
            .ok_or(HiddenHandStateUnsupported::UnknownPlayer)?;
        match mode {
            HiddenHandModelMode::Riichi => {
                if !context.is_reached(player) {
                    return Err(HiddenHandStateUnsupported::NotReached);
                }
                if fixed_melds.iter().any(Meld::is_open) {
                    return Err(HiddenHandStateUnsupported::OpenMeld);
                }
            }
            HiddenHandModelMode::OpenHandStructuralTenpai => {
                if context.is_reached(player) {
                    return Err(HiddenHandStateUnsupported::Reached);
                }
                if !fixed_melds.iter().any(Meld::is_open) {
                    return Err(HiddenHandStateUnsupported::NoOpenMeld);
                }
            }
        }
        let fixed_meld_count =
            fixed_meld_count(fixed_melds).ok_or(HiddenHandStateUnsupported::TooManyMelds)?;

        let mut remaining = [0u8; TileType::COUNT];
        let mut unron_mask = 0u64;
        let mut unron_tiles = Vec::new();
        for tile in TileType::all() {
            remaining[tile.index()] = remaining_tile_copies(tile, context);
            if mode == HiddenHandModelMode::Riichi && is_genbutsu_for(tile, player, context) {
                unron_mask |= tile_mask(tile);
                unron_tiles.push(tile);
            }
        }

        Ok(Self {
            context,
            fixed_melds,
            fixed_meld_count,
            concealed_hand_len: 13 - 3 * fixed_meld_count.get(),
            remaining,
            unron_mask,
            unron_tiles,
        })
    }
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
/// **target は今から自分が捨てる牌を想定し、その物理牌は
/// [`GameContext::visible_tiles`] に反映済みであることを前提にする。** target 自身は隠れ手牌へ
/// 配る牌ではないので `remaining[]` から消費しないが、候補が保持する target の枚数は
/// `remaining[target]` の制約を受ける。自分が持っていない牌を target に渡すと、その1枚分の
/// 見え牌が欠けた残枚数で数えることになる。
///
/// 候補生成は対象牌固有の構造から行い、全牌種総当たりも物理牌 subset 総当たりもしない。
/// 完成形判定は既存 [`analyze_completed_hand`] を source of truth とする。全34牌種の受け入れを
/// 毎回作らず、対象牌と「その player に対して既にロン不能な牌種」だけを調べる。
/// 同じ `TileCounts` は decomposition 数によらず1回だけ加算する。
///
/// ロン不能牌との交差判定は target 間で共有する cache に載るため、同じ player の複数 target を
/// 順に評価する場合は同じ instance を使い回す。
pub struct ReachedHiddenHandStates<'a> {
    context: &'a GameContext,
    fixed_melds: &'a [Meld],
    fixed_meld_count: FixedMeldCount,
    concealed_hand_len: u8,
    remaining: HandCounts,
    unron_mask: u64,
    unron_tiles: Vec<TileType>,
    complete_melds: Vec<[TileType; 3]>,
    evaluated: HashMap<u128, EvaluatedState>,
    metrics: HiddenHandStateMetrics,
    completion_checks: Cell<u64>,
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
        Ok(Self::from_input(HiddenHandModelInput::new(
            player,
            context,
            HiddenHandModelMode::Riichi,
        )?))
    }

    fn from_input(input: HiddenHandModelInput<'a>) -> Self {
        let complete_melds = feasible_complete_melds(&input.remaining);

        Self {
            context: input.context,
            fixed_melds: input.fixed_melds,
            fixed_meld_count: input.fixed_meld_count,
            concealed_hand_len: input.concealed_hand_len,
            remaining: input.remaining,
            unron_mask: input.unron_mask,
            unron_tiles: input.unron_tiles,
            complete_melds,
            evaluated: HashMap::new(),
            metrics: HiddenHandStateMetrics::default(),
            completion_checks: Cell::new(0),
            generation: 0,
        }
    }

    /// 対象リーチ者の固定面子数。
    pub fn fixed_meld_count(&self) -> FixedMeldCount {
        self.fixed_meld_count
    }

    /// 打牌後の concealed hand 枚数 (`13 - 3 * 固定面子数`)。
    pub fn concealed_hand_len(&self) -> u8 {
        self.concealed_hand_len
    }

    /// 対象 player 自身の河、またはリーチ後に通った牌としてロン不能な牌種か。
    pub fn is_unron_capable_tile(&self, tile: TileType) -> bool {
        self.unron_mask & tile_mask(tile) != 0
    }

    /// 対象 player に対して既にロン不能な牌種。フリテン判定はこの集合だけを調べる。
    pub fn unron_capable_tiles(&self) -> &[TileType] {
        &self.unron_tiles
    }

    /// 評価対象の `GameContext`。
    pub fn context(&self) -> &GameContext {
        self.context
    }

    /// 判定 cache に載っている隠れ手牌状態の数。計測用。
    pub fn evaluated_state_count(&self) -> usize {
        self.evaluated.len()
    }

    /// これまでの評価の内訳計測値。計測用で、数え上げ結果には影響しない。
    pub fn metrics(&self) -> HiddenHandStateMetrics {
        HiddenHandStateMetrics {
            completion_checks: self.completion_checks.get(),
            ..self.metrics
        }
    }

    /// 隠れ手牌1状態が対象牌でロン可能かを判定する。
    ///
    /// cache を使わない素の判定で、`ron_capable_state_weight` の hot path と同じ述語を使う。
    /// 全34牌種の受け入れを作る従来判定との cross-check 用に公開している。
    pub fn is_ron_capable_hidden_hand(&self, hand: &TileCounts, target: TileType) -> bool {
        let hand = hand.as_array();
        let mut ids = concealed_tile_ids(hand);
        !self.waits_on_unron_tile(&mut ids, hand) && self.completes_hand(&mut ids, hand, target)
    }

    /// 対象牌で実際にロンできる隠れ手牌状態の重みを exact に数える。
    ///
    /// target は今から自分が捨てる牌を想定し、その物理牌は `visible_tiles` に反映済みである
    /// ことを前提にする。詳細は [`ReachedHiddenHandStates`] を参照。
    pub fn ron_capable_state_weight(&mut self, target: TileType) -> RonCapableStateWeight {
        self.completion_state_weight(target)
    }

    fn completion_state_weight(&mut self, target: TileType) -> RonCapableStateWeight {
        let mut total = RonCapableStateWeight::default();
        if self.is_unron_capable_tile(target) {
            return total;
        }

        if self.evaluated.len() >= EVALUATED_STATE_CAPACITY {
            self.evaluated.clear();
        }
        self.generation += 1;

        let start = Instant::now();
        let before = self.judgement_elapsed();
        let mut hand = [0u8; TileType::COUNT];
        self.collect_standard(&mut hand, target, &mut total);
        if !self.fixed_meld_count.has_melds() {
            self.collect_chiitoitsu(&mut hand, target, &mut total);
            self.collect_kokushi(&mut hand, target, &mut total);
        }
        self.metrics.candidate_generation += start.elapsed() - (self.judgement_elapsed() - before);

        total
    }

    fn judgement_elapsed(&self) -> Duration {
        self.metrics.unron_filtering + self.metrics.target_completion
    }

    fn collect_standard(
        &mut self,
        hand: &mut HandCounts,
        target: TileType,
        total: &mut RonCapableStateWeight,
    ) {
        let concealed_melds = 4 - self.fixed_meld_count.get();

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
        self.metrics.generated_candidates += 1;
        let generation = self.generation;
        let key = state_key(hand);
        let known = match self.evaluated.get_mut(&key) {
            Some(evaluated) => {
                if evaluated.counted_generation == generation {
                    return;
                }
                evaluated.counted_generation = generation;
                Some(evaluated.waits_on_unron_tile)
            }
            None => None,
        };
        if known == Some(true) {
            return;
        }

        let mut ids = concealed_tile_ids(hand);
        if known.is_none() {
            let start = Instant::now();
            let waits_on_unron_tile = self.waits_on_unron_tile(&mut ids, hand);
            self.metrics.unron_filtering += start.elapsed();
            self.metrics.evaluated_states += 1;
            self.evaluated.insert(
                key,
                EvaluatedState {
                    waits_on_unron_tile,
                    counted_generation: generation,
                },
            );
            if waits_on_unron_tile {
                return;
            }
        }

        let start = Instant::now();
        let completes = self.completes_hand(&mut ids, hand, target);
        self.metrics.target_completion += start.elapsed();
        if !completes {
            return;
        }

        total.weight += hand_weight(&self.remaining, hand);
        total.states += 1;
    }

    // 候補の別待ちが、その player に対して既にロン不能な牌種と重なるか。
    //
    // 全34牌種の受け入れを作らず、本人の河とリーチ後に通った牌 (`is_genbutsu_for`) だけを調べる。
    // その牌でも和了形になるなら、その候補はその牌を見逃したことになりロンできない。
    fn waits_on_unron_tile(&self, ids: &mut Vec<TileId>, hand: &HandCounts) -> bool {
        self.unron_tiles
            .iter()
            .any(|&tile| self.completes_hand(ids, hand, tile))
    }

    // 隠れ手牌へ1枚加えた形が和了形かを既存 completed hand logic で判定する。
    //
    // 5枚目になる牌種は和了牌になり得ないので、受け入れ判定と同じく除外する。
    fn completes_hand(&self, ids: &mut Vec<TileId>, hand: &HandCounts, tile: TileType) -> bool {
        let copy = hand[tile.index()];
        if copy >= MAX_TILE_COPIES {
            return false;
        }
        self.completion_checks.set(self.completion_checks.get() + 1);
        ids.push(tile_id(tile, copy));
        let complete = analyze_completed_hand(ids, self.fixed_melds)
            .is_ok_and(|analysis| analysis.is_complete());
        ids.pop();
        complete
    }
}

/// 公開副露を固定した、非リーチ相手の conditional-tenpai hidden-hand model。
///
/// 公開情報と固定副露に整合する structural tenpai state space だけを扱う。固定副露があるため
/// hand family は Standard に限定される。役・フリテン・ロン可能性・テンパイ確率は扱わない。
/// production defense からは利用しない counting foundation である。
pub struct StructuralTenpaiHiddenHandStates<'a> {
    inner: ReachedHiddenHandStates<'a>,
}

impl<'a> StructuralTenpaiHiddenHandStates<'a> {
    /// 非リーチかつ公開副露を1つ以上持つ player の公開情報から model を組み立てる。
    pub fn new(
        player: usize,
        context: &'a GameContext,
    ) -> Result<Self, HiddenHandStateUnsupported> {
        let input = HiddenHandModelInput::new(
            player,
            context,
            HiddenHandModelMode::OpenHandStructuralTenpai,
        )?;
        Ok(Self {
            inner: ReachedHiddenHandStates::from_input(input),
        })
    }

    /// 対象 player の固定面子数。
    pub fn fixed_meld_count(&self) -> FixedMeldCount {
        self.inner.fixed_meld_count()
    }

    /// 打牌後の concealed hand 枚数 (`13 - 3 * 固定面子数`)。
    pub fn concealed_hand_len(&self) -> u8 {
        self.inner.concealed_hand_len()
    }

    /// 対象牌を加えると Standard の structural completion になる state weight を数える。
    ///
    /// target は今から自分が捨てる牌を想定し、その物理牌は `visible_tiles` に反映済みである
    /// ことを前提にする。
    pub fn target_completion_state_weight(
        &mut self,
        target: TileType,
    ) -> StructuralCompletionStateWeight {
        self.inner.completion_state_weight(target).into()
    }

    /// 評価対象の `GameContext`。
    pub fn context(&self) -> &GameContext {
        self.inner.context()
    }

    /// これまでの評価の内訳計測値。計測用で、数え上げ結果には影響しない。
    pub fn metrics(&self) -> HiddenHandStateMetrics {
        self.inner.metrics()
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
/// target の前提と数える対象は [`ReachedHiddenHandStates`] と同じ。同じ player の複数 target を
/// 評価する場合は [`ReachedHiddenHandStates`] を使い回すほうが判定 cache を共有できる。
pub fn ron_capable_hidden_hand_weight(
    target: TileType,
    player: usize,
    context: &GameContext,
) -> Result<RonCapableStateWeight, HiddenHandStateUnsupported> {
    Ok(ReachedHiddenHandStates::new(player, context)?.ron_capable_state_weight(target))
}
