use std::cell::Cell;
use std::collections::HashMap;
use std::time::{Duration, Instant};

use bot_logic::{
    FixedMeldCount, Meld, RiichiStatus, TileCounts, TileId, TileType, WinMethod, WinningContext,
    analyze_completed_hand, evaluate_winning_yaku, evaluate_winning_yakuman,
    fixed_melds_guarantee_yaku, is_standard_hand_complete, standard_completion_intersects,
};

use crate::context::GameContext;
use crate::meld::fixed_meld_count;
use crate::open_hand_defense::is_ron_safe_for_open_hand_target;

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
    /// OpenHand ron-capable model に必要な場風が不明。
    UnknownRoundWind,
    /// OpenHand ron-capable model に必要な対象 player の自風が不明。
    UnknownSeatWind,
    /// OpenHand ron-capable model に必要な残りツモ可能枚数が不明。
    UnknownRemainingTiles,
    /// OpenHand ron-capable model に必要な current temporary-passed 履歴が不明。
    UnknownTemporaryPassedTiles,
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

/// enumerating hidden-hand model の内訳計測値。数え上げ結果には影響しない。
///
/// 件数と時間は同じ instance で行った全 target 評価の累計。`unique_candidates` は同一 target
/// 内の重複を除いた状態数、`cache_hits` は同一 target 内の重複と target 間の再利用を合わせた
/// 件数。candidate record 内の細粒度 elapsed は全件を数える counter と違い、hot path への影響を
/// 抑えるため排他的な sampling cohort で計測する。候補数が多いほど interval を広げ、各candidate
/// で追加する timer は最大1組にする。`target_completion` は target の structural completion 判定
/// だけを含み、
/// `guaranteed_yaku_shortcuts` は実 evaluator を呼ばず固定面子の通常役で確定した completed
/// state 数。guaranteed-yaku では boolean 判定、それ以外では materialized analysis を使った
/// 件数も分けて記録する。`tile_count_constructions` と `tile_id_materializations` は hidden state
/// から各 representation を実際に構築した回数。役と named Yakuman の評価時間はそれぞれ別に
/// 記録する。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct HiddenHandStateMetrics {
    pub generated_candidates: u64,
    pub record_calls: u64,
    pub unique_candidates: u64,
    pub evaluated_states: u64,
    pub state_key_constructions: u64,
    pub cache_lookups: u64,
    pub cache_hits: u64,
    pub same_generation_duplicates: u64,
    pub cross_target_cache_reuses: u64,
    pub cache_misses: u64,
    pub cache_inserts: u64,
    /// `HashMap::insert()` 前後で capacity が実際に増加した回数。
    pub cache_capacity_grows: u64,
    /// insert 前後で capacity が実際に増加した insertion だけの elapsed。
    pub cache_growth_insertion: Duration,
    pub cache_clears: u64,
    pub cached_states: usize,
    pub tile_count_constructions: u64,
    pub tile_id_materializations: u64,
    pub hand_weight_calculations: u64,
    pub result_accumulations: u64,
    pub completion_checks: u64,
    pub furiten_states_checked: u64,
    pub furiten_states_filtered: u64,
    /// candidate tile ごとに完成判定を呼んだ回数。batch path の判定は含まない。
    pub furiten_completion_checks: u64,
    /// hidden state の candidate 集合へ一括 Standard completion 判定を行った回数。
    pub furiten_batch_checks: u64,
    /// batch 判定へ渡した、5枚目にならない candidate tile 数の累計。
    pub furiten_candidate_tiles: u64,
    pub target_completion_checks: u64,
    pub target_boolean_completion_checks: u64,
    pub target_materialized_analyses: u64,
    pub completed_states: u64,
    pub guaranteed_yaku_shortcuts: u64,
    pub yaku_evaluations: u64,
    pub yaku_successful_states: u64,
    pub yakuman_evaluations: u64,
    pub yakuman_successful_states: u64,
    pub ron_capable_weight: u128,
    pub ron_capable_states: u64,
    /// judgement elapsed を除いた candidate traversal / record の旧 residual bucket。
    pub candidate_generation: Duration,
    /// 細粒度 elapsed の sampling interval。対象外の model では `0`。
    pub candidate_timing_interval: u64,
    pub candidate_timing_samples: u64,
    pub sampled_timer_overhead_measurements: u64,
    pub sampled_timer_overhead: Duration,
    /// sampled `record()` 全体。furiten / target / yaku / Yakuman の時間も含む。
    pub sampled_record_total: Duration,
    /// sampled `record()` から上記 judgement elapsed を除いた時間。
    pub sampled_record_residual: Duration,
    /// record cohort に偶然含まれた cache growth。全 growth の実測値へ置換するため分離する。
    pub sampled_record_cache_growth: Duration,
    pub sampled_state_key_constructions: u64,
    pub sampled_state_key: Duration,
    pub sampled_cache_lookups: u64,
    pub sampled_cache_lookup: Duration,
    pub sampled_representation_constructions: u64,
    pub sampled_representation_construction: Duration,
    pub sampled_cache_inserts: u64,
    pub sampled_cache_insertion: Duration,
    pub sampled_hand_weight_calculations: u64,
    pub sampled_hand_weight: Duration,
    pub sampled_result_accumulations: u64,
    pub sampled_result_accumulation: Duration,
    pub unron_filtering: Duration,
    pub target_completion: Duration,
    pub yaku_evaluation: Duration,
    pub yakuman_evaluation: Duration,
    pub total_r_evaluation: Duration,
}

#[derive(Debug, Clone, Copy, Default)]
struct YakuQualifiedCompletion {
    structural_checked: bool,
    boolean_completion_checked: bool,
    materialized_analysis: bool,
    structurally_complete: bool,
    guaranteed_yaku_shortcut: bool,
    yaku_evaluated: bool,
    yaku_successful: bool,
    yakuman_evaluated: bool,
    yakuman_successful: bool,
    structural_completion: Duration,
    yaku_evaluation: Duration,
    yakuman_evaluation: Duration,
}

#[derive(Debug, Clone, Copy)]
struct RonQualification {
    context: WinningContext,
    guaranteed_yaku: bool,
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

// candidate ごとの clock read が measurement 自体を支配しないよう、細粒度 elapsed はこの間隔で
// sampling する。全件の処理回数は別 counter で記録する。
const LARGE_CANDIDATE_TIMING_SAMPLE_INTERVAL: u64 = 127;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CandidateTimingSample {
    None,
    Record,
    StateKey,
    CacheLookup,
    Representation,
    CacheInsertion,
    HandWeight,
    ResultAccumulation,
    TimerOverhead,
}

fn candidate_timing_sample(record_call: u64, interval: u64) -> CandidateTimingSample {
    if interval == 0 {
        return CandidateTimingSample::None;
    }
    match record_call % interval {
        0 => CandidateTimingSample::Record,
        1 => CandidateTimingSample::StateKey,
        2 => CandidateTimingSample::CacheLookup,
        3 => CandidateTimingSample::Representation,
        4 => CandidateTimingSample::CacheInsertion,
        5 => CandidateTimingSample::HandWeight,
        6 => CandidateTimingSample::ResultAccumulation,
        7 => CandidateTimingSample::TimerOverhead,
        _ => CandidateTimingSample::None,
    }
}

fn candidate_timing_interval(fixed_meld_count: FixedMeldCount) -> u64 {
    match fixed_meld_count.get() {
        2 => 31,
        3.. => 11,
        _ => LARGE_CANDIDATE_TIMING_SAMPLE_INTERVAL,
    }
}

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

fn tile_counts(hand: &HandCounts) -> TileCounts {
    TileCounts::try_from(*hand).expect("generated hand has at most four copies per tile type")
}

pub(super) fn is_standard_hand_complete_with_target(
    counts: &TileCounts,
    target: TileType,
    fixed_meld_count: FixedMeldCount,
) -> bool {
    let mut counts = *counts;
    counts.try_add(target).is_ok() && is_standard_hand_complete(&counts, fixed_meld_count)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum HiddenHandModelMode {
    Riichi,
    OpenHandStructuralTenpai,
    OpenHandRonCapable,
}

pub(super) struct HiddenHandModelInput<'a> {
    pub(super) mode: HiddenHandModelMode,
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
            HiddenHandModelMode::OpenHandStructuralTenpai
            | HiddenHandModelMode::OpenHandRonCapable => {
                if context.is_reached(player) {
                    return Err(HiddenHandStateUnsupported::Reached);
                }
                if !fixed_melds.iter().any(Meld::is_open) {
                    return Err(HiddenHandStateUnsupported::NoOpenMeld);
                }
                if mode == HiddenHandModelMode::OpenHandRonCapable
                    && context.temporary_passed_tiles_of(player).is_none()
                {
                    return Err(HiddenHandStateUnsupported::UnknownTemporaryPassedTiles);
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
            let is_unron = match mode {
                HiddenHandModelMode::Riichi => is_genbutsu_for(tile, player, context),
                HiddenHandModelMode::OpenHandStructuralTenpai => false,
                HiddenHandModelMode::OpenHandRonCapable => {
                    is_ron_safe_for_open_hand_target(tile, player, context)
                }
            };
            if is_unron {
                unron_mask |= tile_mask(tile);
                unron_tiles.push(tile);
            }
        }

        Ok(Self {
            mode,
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

fn open_hand_winning_context(
    player: usize,
    context: &GameContext,
) -> Result<WinningContext, HiddenHandStateUnsupported> {
    let round_wind = context
        .round_wind()
        .ok_or(HiddenHandStateUnsupported::UnknownRoundWind)?;
    let seat_wind = context
        .seat_wind_of(player)
        .ok_or(HiddenHandStateUnsupported::UnknownSeatWind)?;
    let remaining_live_tiles = context
        .remaining_tiles()
        .ok_or(HiddenHandStateUnsupported::UnknownRemainingTiles)?;
    Ok(WinningContext::new(WinMethod::Ron)
        .with_round_wind(Some(round_wind))
        .with_seat_wind(Some(seat_wind))
        .with_riichi(RiichiStatus::NotDeclared)
        .with_ippatsu(Some(false))
        .with_rinshan(Some(false))
        .with_chankan(Some(false))
        .with_remaining_live_tiles(Some(remaining_live_tiles)))
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
/// target の完成形判定は既存 completed-hand logic を source of truth とする。OpenHand の
/// guaranteed-yaku と furiten 判定では Standard decomposition 探索を共有する boolean helper、
/// 役評価に analysis が必要な場合は [`analyze_completed_hand`] を使う。全34牌種の受け入れを毎回
/// 作らず、対象牌と「その player に対して既にロン不能な牌種」だけを調べる。同じ
/// `TileCounts` は decomposition 数によらず1回だけ加算する。
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
    use_standard_completion_for_furiten: bool,
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
        let candidate_timing_interval = if input.mode == HiddenHandModelMode::OpenHandRonCapable {
            candidate_timing_interval(input.fixed_meld_count)
        } else {
            0
        };

        Self {
            context: input.context,
            fixed_melds: input.fixed_melds,
            fixed_meld_count: input.fixed_meld_count,
            concealed_hand_len: input.concealed_hand_len,
            remaining: input.remaining,
            unron_mask: input.unron_mask,
            unron_tiles: input.unron_tiles,
            complete_melds,
            use_standard_completion_for_furiten: input.mode
                == HiddenHandModelMode::OpenHandRonCapable,
            evaluated: HashMap::new(),
            metrics: HiddenHandStateMetrics {
                candidate_timing_interval,
                ..HiddenHandStateMetrics::default()
            },
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
            cached_states: self.evaluated.len(),
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
        !self.waits_on_unron_tile_with_ids(&mut ids, hand)
            && self.completes_hand(&mut ids, hand, target)
    }

    /// 対象牌で実際にロンできる隠れ手牌状態の重みを exact に数える。
    ///
    /// target は今から自分が捨てる牌を想定し、その物理牌は `visible_tiles` に反映済みである
    /// ことを前提にする。詳細は [`ReachedHiddenHandStates`] を参照。
    pub fn ron_capable_state_weight(&mut self, target: TileType) -> RonCapableStateWeight {
        self.completion_state_weight(target)
    }

    fn completion_state_weight(&mut self, target: TileType) -> RonCapableStateWeight {
        self.filtered_completion_state_weight(target, None)
    }

    fn yaku_qualified_state_weight(
        &mut self,
        target: TileType,
        winning_context: WinningContext,
    ) -> RonCapableStateWeight {
        let qualification = RonQualification {
            context: winning_context,
            guaranteed_yaku: fixed_melds_guarantee_yaku(self.fixed_melds, winning_context),
        };
        self.filtered_completion_state_weight(target, Some(qualification))
    }

    fn filtered_completion_state_weight(
        &mut self,
        target: TileType,
        qualification: Option<RonQualification>,
    ) -> RonCapableStateWeight {
        let total_start = Instant::now();
        let mut total = RonCapableStateWeight::default();
        if self.is_unron_capable_tile(target) {
            self.metrics.total_r_evaluation += total_start.elapsed();
            return total;
        }

        if self.evaluated.len() >= EVALUATED_STATE_CAPACITY {
            self.evaluated.clear();
            self.metrics.cache_clears += 1;
        }
        self.generation += 1;

        let before = self.judgement_elapsed();
        let mut hand = [0u8; TileType::COUNT];
        self.collect_standard(&mut hand, target, qualification, &mut total);
        if !self.fixed_meld_count.has_melds() {
            self.collect_chiitoitsu(&mut hand, target, qualification, &mut total);
            self.collect_kokushi(&mut hand, target, qualification, &mut total);
        }
        let total_elapsed = total_start.elapsed();
        self.metrics.candidate_generation +=
            total_elapsed.saturating_sub(self.judgement_elapsed() - before);
        self.metrics.total_r_evaluation += total_elapsed;

        total
    }

    fn judgement_elapsed(&self) -> Duration {
        self.metrics.unron_filtering
            + self.metrics.target_completion
            + self.metrics.yaku_evaluation
            + self.metrics.yakuman_evaluation
    }

    fn collect_standard(
        &mut self,
        hand: &mut HandCounts,
        target: TileType,
        qualification: Option<RonQualification>,
        total: &mut RonCapableStateWeight,
    ) {
        let concealed_melds = 4 - self.fixed_meld_count.get();

        if self.try_add(hand, &[target]) {
            self.collect_meld_multisets(hand, concealed_melds, 0, target, qualification, total);
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
                    self.collect_meld_multisets(
                        hand,
                        concealed_melds - 1,
                        0,
                        target,
                        qualification,
                        total,
                    );
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
        qualification: Option<RonQualification>,
        total: &mut RonCapableStateWeight,
    ) {
        if melds_left == 0 {
            self.record(hand, target, qualification, total);
            return;
        }
        for index in start..self.complete_melds.len() {
            let meld = self.complete_melds[index];
            if self.try_add(hand, &meld) {
                self.collect_meld_multisets(
                    hand,
                    melds_left - 1,
                    index,
                    target,
                    qualification,
                    total,
                );
                remove(hand, &meld);
            }
        }
    }

    fn collect_chiitoitsu(
        &mut self,
        hand: &mut HandCounts,
        target: TileType,
        qualification: Option<RonQualification>,
        total: &mut RonCapableStateWeight,
    ) {
        if !self.try_add(hand, &[target]) {
            return;
        }
        self.collect_chiitoitsu_pairs(hand, 6, 0, target, qualification, total);
        remove(hand, &[target]);
    }

    fn collect_chiitoitsu_pairs(
        &mut self,
        hand: &mut HandCounts,
        pairs_left: usize,
        start: usize,
        target: TileType,
        qualification: Option<RonQualification>,
        total: &mut RonCapableStateWeight,
    ) {
        if pairs_left == 0 {
            self.record(hand, target, qualification, total);
            return;
        }
        let last = TileType::COUNT - pairs_left;
        for index in start..=last {
            let tile = TileType::new(index as u8).expect("index is a valid tile type");
            if tile == target || self.remaining[index] < 2 {
                continue;
            }
            hand[index] += 2;
            self.collect_chiitoitsu_pairs(
                hand,
                pairs_left - 1,
                index + 1,
                target,
                qualification,
                total,
            );
            hand[index] -= 2;
        }
    }

    fn collect_kokushi(
        &mut self,
        hand: &mut HandCounts,
        target: TileType,
        qualification: Option<RonQualification>,
        total: &mut RonCapableStateWeight,
    ) {
        if !target.is_yaochu() {
            return;
        }
        let yaochu: Vec<TileType> = TileType::all().filter(|tile| tile.is_yaochu()).collect();

        if self.try_add(hand, &yaochu) {
            self.record(hand, target, qualification, total);
            remove(hand, &yaochu);
        }

        let others: Vec<TileType> = yaochu.into_iter().filter(|&tile| tile != target).collect();
        if !self.try_add(hand, &others) {
            return;
        }
        for &extra in &others {
            if self.try_add(hand, &[extra]) {
                self.record(hand, target, qualification, total);
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

    fn record(
        &mut self,
        hand: &HandCounts,
        target: TileType,
        qualification: Option<RonQualification>,
        total: &mut RonCapableStateWeight,
    ) {
        let timing_sample = candidate_timing_sample(
            self.metrics.record_calls,
            self.metrics.candidate_timing_interval,
        );
        if timing_sample == CandidateTimingSample::TimerOverhead {
            let start = Instant::now();
            self.metrics.sampled_timer_overhead_measurements += 1;
            self.metrics.sampled_timer_overhead += start.elapsed();
        }
        let record_start = (timing_sample == CandidateTimingSample::Record).then(Instant::now);
        let judgement_before = self.judgement_elapsed();
        self.metrics.record_calls += 1;
        self.metrics.generated_candidates += 1;

        self.record_inner(hand, target, qualification, total, timing_sample);

        if let Some(record_start) = record_start {
            let elapsed = record_start.elapsed();
            let judgement = self.judgement_elapsed() - judgement_before;
            self.metrics.candidate_timing_samples += 1;
            self.metrics.sampled_record_total += elapsed;
            self.metrics.sampled_record_residual += elapsed.saturating_sub(judgement);
        }
    }

    fn record_inner(
        &mut self,
        hand: &HandCounts,
        target: TileType,
        qualification: Option<RonQualification>,
        total: &mut RonCapableStateWeight,
        timing_sample: CandidateTimingSample,
    ) {
        let generation = self.generation;

        let state_key_start = (timing_sample == CandidateTimingSample::StateKey).then(Instant::now);
        let key = state_key(hand);
        self.metrics.state_key_constructions += 1;
        if let Some(state_key_start) = state_key_start {
            self.metrics.sampled_state_key_constructions += 1;
            self.metrics.sampled_state_key += state_key_start.elapsed();
        }

        let cache_lookup_start =
            (timing_sample == CandidateTimingSample::CacheLookup).then(Instant::now);
        self.metrics.cache_lookups += 1;
        let (known, same_generation_duplicate) = match self.evaluated.get_mut(&key) {
            Some(evaluated) => {
                self.metrics.cache_hits += 1;
                if evaluated.counted_generation == generation {
                    self.metrics.same_generation_duplicates += 1;
                    (Some(evaluated.waits_on_unron_tile), true)
                } else {
                    self.metrics.cross_target_cache_reuses += 1;
                    evaluated.counted_generation = generation;
                    (Some(evaluated.waits_on_unron_tile), false)
                }
            }
            None => {
                self.metrics.cache_misses += 1;
                (None, false)
            }
        };
        if let Some(cache_lookup_start) = cache_lookup_start {
            self.metrics.sampled_cache_lookups += 1;
            self.metrics.sampled_cache_lookup += cache_lookup_start.elapsed();
        }
        if same_generation_duplicate {
            return;
        }

        self.metrics.unique_candidates += 1;
        if known == Some(true) {
            self.metrics.furiten_states_filtered += 1;
            return;
        }

        let guaranteed_yaku = qualification.is_some_and(|value| value.guaranteed_yaku);
        let needs_counts = self.use_standard_completion_for_furiten
            && (guaranteed_yaku || known.is_none() && !self.unron_tiles.is_empty());
        let counts =
            needs_counts.then(|| self.construct_tile_counts_for_record(hand, timing_sample));
        let mut ids = (!self.use_standard_completion_for_furiten)
            .then(|| self.materialize_tile_ids_for_record(hand, timing_sample));
        if known.is_none() {
            let checks_before = self.completion_checks.get();
            let start = Instant::now();
            let waits_on_unron_tile = if self.use_standard_completion_for_furiten {
                counts.as_ref().is_some_and(|counts| {
                    self.metrics.furiten_candidate_tiles += self
                        .unron_tiles
                        .iter()
                        .filter(|&&tile| counts.count(tile) < MAX_TILE_COPIES)
                        .count() as u64;
                    self.waits_on_unron_tile_with_counts(counts)
                })
            } else {
                if ids.is_none() {
                    ids = Some(self.materialize_tile_ids_for_record(hand, timing_sample));
                }
                let ids = ids.as_mut().expect("initialized above");
                self.waits_on_unron_tile_with_ids(ids, hand)
            };
            self.metrics.unron_filtering += start.elapsed();
            self.metrics.furiten_states_checked += 1;
            let completion_checks = self.completion_checks.get() - checks_before;
            if self.use_standard_completion_for_furiten {
                self.metrics.furiten_batch_checks += completion_checks;
            } else {
                self.metrics.furiten_completion_checks += completion_checks;
            }
            self.metrics.evaluated_states += 1;
            let capacity_before = self.evaluated.capacity();
            // `capacity()` は lower bound なので、この条件は actual growth の判定には使わない。
            // capacity 未満なら reallocation なしで保持できる契約を使い、全insertのclock readを
            // 避けつつ、growthの可能性があるinsertだけを全件計時する。
            let reaches_reported_capacity = self.evaluated.len() == capacity_before;
            let insertion_start = (self.metrics.candidate_timing_interval != 0
                && (reaches_reported_capacity
                    || timing_sample == CandidateTimingSample::CacheInsertion))
                .then(Instant::now);
            self.evaluated.insert(
                key,
                EvaluatedState {
                    waits_on_unron_tile,
                    counted_generation: generation,
                },
            );
            let insertion_elapsed = insertion_start.map(|start| start.elapsed());
            self.metrics.cache_inserts += 1;
            let capacity_after = self.evaluated.capacity();
            let actually_grew = capacity_after > capacity_before;
            if actually_grew && self.metrics.candidate_timing_interval != 0 {
                let elapsed = insertion_elapsed.expect("a capacity-exhausting insertion is timed");
                self.metrics.cache_capacity_grows += 1;
                self.metrics.cache_growth_insertion += elapsed;
                if timing_sample == CandidateTimingSample::Record {
                    self.metrics.sampled_record_cache_growth += elapsed;
                }
            } else if timing_sample == CandidateTimingSample::CacheInsertion {
                let elapsed = insertion_elapsed.expect("sampled insertion is timed");
                self.metrics.sampled_cache_inserts += 1;
                self.metrics.sampled_cache_insertion += elapsed;
            }
            if waits_on_unron_tile {
                self.metrics.furiten_states_filtered += 1;
                return;
            }
        }

        let completes = match qualification {
            Some(qualification) => {
                let result = if qualification.guaranteed_yaku {
                    self.completes_hand_with_guaranteed_yaku(
                        counts
                            .as_ref()
                            .expect("guaranteed-yaku target uses count representation"),
                        target,
                    )
                } else {
                    if ids.is_none() {
                        ids = Some(self.materialize_tile_ids_for_record(hand, timing_sample));
                    }
                    let ids = ids.as_mut().expect("initialized above");
                    self.completes_hand_with_yaku(ids, hand, target, qualification.context)
                };
                self.metrics.target_completion += result.structural_completion;
                self.metrics.yaku_evaluation += result.yaku_evaluation;
                self.metrics.yakuman_evaluation += result.yakuman_evaluation;
                self.metrics.target_completion_checks += u64::from(result.structural_checked);
                self.metrics.target_boolean_completion_checks +=
                    u64::from(result.boolean_completion_checked);
                self.metrics.target_materialized_analyses +=
                    u64::from(result.materialized_analysis);
                self.metrics.completed_states += u64::from(result.structurally_complete);
                self.metrics.guaranteed_yaku_shortcuts +=
                    u64::from(result.guaranteed_yaku_shortcut);
                self.metrics.yaku_evaluations += u64::from(result.yaku_evaluated);
                self.metrics.yaku_successful_states += u64::from(result.yaku_successful);
                self.metrics.yakuman_evaluations += u64::from(result.yakuman_evaluated);
                self.metrics.yakuman_successful_states += u64::from(result.yakuman_successful);
                result.guaranteed_yaku_shortcut
                    || result.yaku_successful
                    || result.yakuman_successful
            }
            None => {
                if ids.is_none() {
                    ids = Some(self.materialize_tile_ids_for_record(hand, timing_sample));
                }
                let ids = ids.as_mut().expect("initialized above");
                let checks_before = self.completion_checks.get();
                let start = Instant::now();
                let completes = self.completes_hand(ids, hand, target);
                self.metrics.target_completion += start.elapsed();
                self.metrics.target_completion_checks +=
                    self.completion_checks.get() - checks_before;
                self.metrics.completed_states += u64::from(completes);
                completes
            }
        };
        if !completes {
            return;
        }

        let hand_weight_start =
            (timing_sample == CandidateTimingSample::HandWeight).then(Instant::now);
        let weight = hand_weight(&self.remaining, hand);
        self.metrics.hand_weight_calculations += 1;
        if let Some(hand_weight_start) = hand_weight_start {
            self.metrics.sampled_hand_weight_calculations += 1;
            self.metrics.sampled_hand_weight += hand_weight_start.elapsed();
        }

        let result_accumulation_start =
            (timing_sample == CandidateTimingSample::ResultAccumulation).then(Instant::now);
        self.metrics.result_accumulations += 1;
        total.weight += weight;
        total.states += 1;
        self.metrics.ron_capable_weight += weight;
        self.metrics.ron_capable_states += 1;
        if let Some(result_accumulation_start) = result_accumulation_start {
            self.metrics.sampled_result_accumulations += 1;
            self.metrics.sampled_result_accumulation += result_accumulation_start.elapsed();
        }
    }

    fn construct_tile_counts_for_record(
        &mut self,
        hand: &HandCounts,
        timing_sample: CandidateTimingSample,
    ) -> TileCounts {
        let start = (timing_sample == CandidateTimingSample::Representation).then(Instant::now);
        let counts = tile_counts(hand);
        self.metrics.tile_count_constructions += 1;
        if let Some(start) = start {
            self.metrics.sampled_representation_constructions += 1;
            self.metrics.sampled_representation_construction += start.elapsed();
        }
        counts
    }

    fn materialize_tile_ids_for_record(
        &mut self,
        hand: &HandCounts,
        timing_sample: CandidateTimingSample,
    ) -> Vec<TileId> {
        let start = (timing_sample == CandidateTimingSample::Representation).then(Instant::now);
        let ids = concealed_tile_ids(hand);
        self.metrics.tile_id_materializations += 1;
        if let Some(start) = start {
            self.metrics.sampled_representation_constructions += 1;
            self.metrics.sampled_representation_construction += start.elapsed();
        }
        ids
    }

    // 候補の別待ちが、その player に対して既にロン不能な牌種と重なるか。
    //
    // 全34牌種の受け入れを作らず、本人の河と各 mode の current passed evidence から得た
    // ロン不能牌だけを調べる。その牌でも和了形になるなら、その候補はロンできない。
    fn waits_on_unron_tile_with_ids(&self, ids: &mut Vec<TileId>, hand: &HandCounts) -> bool {
        self.unron_tiles
            .iter()
            .any(|&tile| self.completes_hand(ids, hand, tile))
    }

    fn waits_on_unron_tile_with_counts(&self, counts: &TileCounts) -> bool {
        self.completion_checks.set(self.completion_checks.get() + 1);
        standard_completion_intersects(counts, self.fixed_meld_count, &self.unron_tiles)
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

    fn completes_hand_with_guaranteed_yaku(
        &self,
        counts: &TileCounts,
        tile: TileType,
    ) -> YakuQualifiedCompletion {
        if counts.count(tile) >= MAX_TILE_COPIES {
            return YakuQualifiedCompletion::default();
        }
        self.completion_checks.set(self.completion_checks.get() + 1);
        let structural_start = Instant::now();
        let structurally_complete =
            is_standard_hand_complete_with_target(counts, tile, self.fixed_meld_count);
        YakuQualifiedCompletion {
            structural_checked: true,
            boolean_completion_checked: true,
            structurally_complete,
            guaranteed_yaku_shortcut: structurally_complete,
            structural_completion: structural_start.elapsed(),
            ..YakuQualifiedCompletion::default()
        }
    }

    fn completes_hand_with_yaku(
        &self,
        ids: &mut Vec<TileId>,
        hand: &HandCounts,
        tile: TileType,
        context: WinningContext,
    ) -> YakuQualifiedCompletion {
        let copy = hand[tile.index()];
        if copy >= MAX_TILE_COPIES {
            return YakuQualifiedCompletion::default();
        }
        self.completion_checks.set(self.completion_checks.get() + 1);

        ids.push(tile_id(tile, copy));
        let structural_start = Instant::now();
        let analysis = analyze_completed_hand(ids, self.fixed_melds);
        let structural_completion = structural_start.elapsed();
        let structurally_complete = analysis
            .as_ref()
            .is_ok_and(|analysis| analysis.is_complete());

        let mut result = YakuQualifiedCompletion {
            structural_checked: true,
            materialized_analysis: true,
            structurally_complete,
            structural_completion,
            ..YakuQualifiedCompletion::default()
        };
        if let Ok(analysis) = analysis
            && structurally_complete
        {
            result.yaku_evaluated = true;
            let yaku_start = Instant::now();
            result.yaku_successful = evaluate_winning_yaku(&analysis, context, tile)
                .iter()
                .any(|evaluation| !evaluation.is_empty());
            result.yaku_evaluation = yaku_start.elapsed();

            // Preserve the existing short-circuit order: named Yakuman is evaluated only when
            // ordinary yaku did not make this state ron-capable.
            if !result.yaku_successful {
                result.yakuman_evaluated = true;
                let yakuman_start = Instant::now();
                result.yakuman_successful = evaluate_winning_yakuman(&analysis, context, tile)
                    .iter()
                    .any(|evaluation| !evaluation.is_empty());
                result.yakuman_evaluation = yakuman_start.elapsed();
            }
        }
        ids.pop();
        result
    }
}

/// 公開副露を固定した、非リーチ相手の conditional-tenpai hidden-hand model。
///
/// 公開情報と固定副露に整合する structural tenpai state space を扱う。固定副露があるため hand
/// family は Standard に限定される。structural completion の計数に加え、必要な局面情報が既知なら
/// 役・named Yakuman・current furiten を反映した ron-capable state も数えられる。
/// テンパイ確率は扱わず、production defense からは利用しない counting foundation である。
pub struct StructuralTenpaiHiddenHandStates<'a> {
    player: usize,
    context: &'a GameContext,
    inner: ReachedHiddenHandStates<'a>,
    ron_inner: Option<ReachedHiddenHandStates<'a>>,
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
            player,
            context,
            inner: ReachedHiddenHandStates::from_input(input),
            ron_inner: None,
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

    /// 対象牌で現在ロン可能な Standard hidden-hand state weight を exact に数える。
    ///
    /// structural completion に加えて、既存 yaku / named Yakuman evaluator で役成立を確認し、
    /// target 本人の河または current temporary-passed と重なる待ちを持つ state を除外する。
    /// 必要な局面情報が unknown の場合は `0` と推測せず [`HiddenHandStateUnsupported`] を返す。
    pub fn ron_capable_state_weight(
        &mut self,
        target: TileType,
    ) -> Result<RonCapableStateWeight, HiddenHandStateUnsupported> {
        let winning_context = open_hand_winning_context(self.player, self.context)?;
        if self.ron_inner.is_none() {
            let input = HiddenHandModelInput::new(
                self.player,
                self.context,
                HiddenHandModelMode::OpenHandRonCapable,
            )?;
            self.ron_inner = Some(ReachedHiddenHandStates::from_input(input));
        }
        Ok(self
            .ron_inner
            .as_mut()
            .expect("initialized above")
            .yaku_qualified_state_weight(target, winning_context))
    }

    /// 評価対象の `GameContext`。
    pub fn context(&self) -> &GameContext {
        self.inner.context()
    }

    /// これまでの評価の内訳計測値。計測用で、数え上げ結果には影響しない。
    pub fn metrics(&self) -> HiddenHandStateMetrics {
        self.inner.metrics()
    }

    /// 遅延生成された OpenHand `R` enumerator の内訳計測値。
    ///
    /// `ron_capable_state_weight` がまだ呼ばれていない場合は `None`。
    pub fn ron_metrics(&self) -> Option<HiddenHandStateMetrics> {
        self.ron_inner
            .as_ref()
            .map(ReachedHiddenHandStates::metrics)
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
