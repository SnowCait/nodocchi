mod block_table;
mod group;

use std::cmp::Ordering;
use std::collections::HashMap;
use std::time::{Duration, Instant};

use bot_logic::{FixedMeldCount, TileType};

use crate::context::GameContext;

use super::hidden_hand_states::{
    HiddenHandModelInput, HiddenHandModelMode, HiddenHandStateMetrics, HiddenHandStateUnsupported,
    RonCapableStateWeight, StructuralCompletionStateWeight, StructuralTenpaiHiddenHandStates,
};
use group::{ChiitoitsuShape, GROUP_COUNT, GroupClass, GroupSpec, enumerate_group_classes};

/// compressed counting の内訳計測値。数え上げ結果には影響しない。
///
/// `precomputation` は player ごとに1回だけ行う群単位の状態圧縮、`tenpai_calculation` は
/// target-independent な denominator の畳み込み、`target_evaluation` は target ごとの畳み込みと
/// DP の累計時間。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CompressedHiddenHandStateMetrics {
    pub enumerated_group_vectors: u64,
    pub retained_group_classes: u64,
    pub collapsed_tenpai_classes: u64,
    pub collapsed_target_classes: u64,
    pub tenpai_dp_transitions: u64,
    pub dp_transitions: u64,
    pub block_tables: Duration,
    pub precomputation: Duration,
    pub tenpai_calculation: Duration,
    pub target_evaluation: Duration,
}

/// 公開情報と整合する structural tenpai 隠れ手牌状態の総量。
///
/// `weight` は各状態の物理牌組み合わせ数 `Π C(remaining[t], count[t])` の総和で、
/// `states` は hand family や decomposition をまたいで重複排除した `TileCounts` の個数。
/// 放銃確率ではない。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct TenpaiStateWeight {
    pub weight: u128,
    pub states: u64,
}

/// 1 target のロン可能 weight と、その player の structural tenpai weight。
///
/// exact な比率比較の材料を保持する。これは実際の放銃確率ではない。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct RonRiskEvidence {
    pub ron_capable_weight: u128,
    pub tenpai_weight: u128,
}

impl RonRiskEvidence {
    /// `self.R / self.T` と `other.R / other.T` を浮動小数点なしで比較する。
    ///
    /// synthetic / inconsistent evidence で分母が0、`R > T`、または cross multiplication が
    /// `u128` に収まらない場合は、比率を推測せず `None` を返す。
    pub fn compare_ratio(&self, other: &Self) -> Option<Ordering> {
        if self.tenpai_weight == 0
            || other.tenpai_weight == 0
            || self.ron_capable_weight > self.tenpai_weight
            || other.ron_capable_weight > other.tenpai_weight
        {
            return None;
        }

        // 136枚・concealed hand 最大13枚では各 weight <= C(136, 13) < 2^59。
        // 内部 model の evidence は R <= T なので cross product も < 2^118 となる。public fields
        // から作られた model 外の値についても panic しないよう、両辺を checked multiplication する。
        let lhs = self.ron_capable_weight.checked_mul(other.tenpai_weight)?;
        let rhs = other.ron_capable_weight.checked_mul(self.tenpai_weight)?;
        Some(lhs.cmp(&rhs))
    }
}

// target と、その player に対して既にロン不能な牌種だけへ落とした群 class。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum SingleKind {
    None,
    Target,
    Unron,
    Other,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum ChiitoitsuState {
    Broken,
    Pairs { pairs: u8, single: SingleKind },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct TargetClassKey {
    size: u8,
    complete: bool,
    complete_with_pair: bool,
    target_wait_to_complete: bool,
    unron_wait_to_complete: bool,
    target_wait_with_pair: bool,
    unron_wait_with_pair: bool,
    chiitoitsu: ChiitoitsuState,
}

#[derive(Debug, Clone, Copy)]
struct TargetClass {
    key: TargetClassKey,
    weight: u128,
    states: u64,
}

// 標準形の面子構成を群ごとに読み替えた DP 状態。
//
// 隠れ手牌は `3k+1` 枚なので、和了牌を1枚足して4面子1雀頭になる群の枚数 mod 3 は
// 「1つの群だけ1で残りが0」か「2つの群が2で残りが0」のどちらかしかない。前者は待ち牌が
// その群へ入って雀頭を作り、後者は片方が雀頭を持ちもう片方へ待ち牌が入る。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StandardPhase {
    Empty,
    Wait {
        target: bool,
        unron: bool,
    },
    HalfPair {
        with_pair: bool,
        target: bool,
        unron: bool,
    },
    Paired {
        target: bool,
        unron: bool,
    },
    Dead,
}

const PHASE_COUNT: usize = 18;
const CHIITOITSU_STATE_COUNT: usize = 29;

fn phase_index(phase: StandardPhase) -> usize {
    match phase {
        StandardPhase::Empty => 0,
        StandardPhase::Wait { target, unron } => 1 + usize::from(target) * 2 + usize::from(unron),
        StandardPhase::HalfPair {
            with_pair,
            target,
            unron,
        } => 5 + usize::from(with_pair) * 4 + usize::from(target) * 2 + usize::from(unron),
        StandardPhase::Paired { target, unron } => {
            13 + usize::from(target) * 2 + usize::from(unron)
        }
        StandardPhase::Dead => 17,
    }
}

fn phase_from_index(index: usize) -> StandardPhase {
    match index {
        0 => StandardPhase::Empty,
        1..=4 => StandardPhase::Wait {
            target: (index - 1) & 2 != 0,
            unron: (index - 1) & 1 != 0,
        },
        5..=12 => StandardPhase::HalfPair {
            with_pair: (index - 5) & 4 != 0,
            target: (index - 5) & 2 != 0,
            unron: (index - 5) & 1 != 0,
        },
        13..=16 => StandardPhase::Paired {
            target: (index - 13) & 2 != 0,
            unron: (index - 13) & 1 != 0,
        },
        _ => StandardPhase::Dead,
    }
}

fn single_kind_index(kind: SingleKind) -> usize {
    match kind {
        SingleKind::None => 0,
        SingleKind::Target => 1,
        SingleKind::Unron => 2,
        SingleKind::Other => 3,
    }
}

fn single_kind_from_index(index: usize) -> SingleKind {
    match index {
        0 => SingleKind::None,
        1 => SingleKind::Target,
        2 => SingleKind::Unron,
        _ => SingleKind::Other,
    }
}

fn chiitoitsu_index(state: ChiitoitsuState) -> usize {
    match state {
        ChiitoitsuState::Broken => 0,
        ChiitoitsuState::Pairs { pairs, single } => {
            1 + usize::from(pairs) * 4 + single_kind_index(single)
        }
    }
}

fn chiitoitsu_from_index(index: usize) -> ChiitoitsuState {
    if index == 0 {
        return ChiitoitsuState::Broken;
    }
    ChiitoitsuState::Pairs {
        pairs: ((index - 1) / 4) as u8,
        single: single_kind_from_index((index - 1) % 4),
    }
}

fn advance_phase(phase: StandardPhase, class: &TargetClassKey) -> StandardPhase {
    let remainder = class.size % 3;
    match phase {
        StandardPhase::Empty => match remainder {
            0 => {
                if class.complete {
                    StandardPhase::Empty
                } else {
                    StandardPhase::Dead
                }
            }
            1 => StandardPhase::Wait {
                target: class.target_wait_with_pair,
                unron: class.unron_wait_with_pair,
            },
            _ => StandardPhase::HalfPair {
                with_pair: class.complete_with_pair,
                target: class.target_wait_to_complete,
                unron: class.unron_wait_to_complete,
            },
        },
        StandardPhase::Wait { .. } => {
            if remainder == 0 && class.complete {
                phase
            } else {
                StandardPhase::Dead
            }
        }
        StandardPhase::HalfPair {
            with_pair,
            target,
            unron,
        } => match remainder {
            0 => {
                if class.complete {
                    phase
                } else {
                    StandardPhase::Dead
                }
            }
            2 => StandardPhase::Paired {
                target: (class.complete_with_pair && target)
                    || (with_pair && class.target_wait_to_complete),
                unron: (class.complete_with_pair && unron)
                    || (with_pair && class.unron_wait_to_complete),
            },
            _ => StandardPhase::Dead,
        },
        StandardPhase::Paired { .. } => {
            if remainder == 0 && class.complete {
                phase
            } else {
                StandardPhase::Dead
            }
        }
        StandardPhase::Dead => StandardPhase::Dead,
    }
}

fn advance_chiitoitsu(state: ChiitoitsuState, class: &TargetClassKey) -> ChiitoitsuState {
    let (
        ChiitoitsuState::Pairs { pairs, single },
        ChiitoitsuState::Pairs {
            pairs: added,
            single: kind,
        },
    ) = (state, class.chiitoitsu)
    else {
        return ChiitoitsuState::Broken;
    };
    if pairs + added > 6 {
        return ChiitoitsuState::Broken;
    }
    let single = match (single, kind) {
        (SingleKind::None, kind) => kind,
        (single, SingleKind::None) => single,
        _ => return ChiitoitsuState::Broken,
    };
    ChiitoitsuState::Pairs {
        pairs: pairs + added,
        single,
    }
}

/// PR #198 の enumerating implementation と同じ exact semantics を、完成した隠れ手牌状態を
/// 1件ずつ作らずに数える compressed counting prototype。
///
/// 対象・target の前提・残枚数・weight の定義・フリテン semantics はすべて
/// [`ReachedHiddenHandStates`](super::ReachedHiddenHandStates) と同じ。違いは数え方だけで、
/// 数百万の完成手牌を列挙する代わりに、
///
/// 1. 萬子・筒子・索子・字牌の4群それぞれで牌数ベクタを列挙し、
///    「面子だけへ分解できるか」「雀頭込みで分解できるか」「1枚足すとどちらになる牌種か」
///    「七対子の対子・単騎の内訳か」だけを残した class へ畳み込む
/// 2. 群 class を畳み込む DP で、4面子1雀頭の構成と七対子の構成を同時に組み立てる
///
/// という2段構えで数える。畳み込みの単位は牌数ベクタなので、`111222333m` のように複数の面子
/// 分解を持つ手牌でも1状態のままになり、decomposition 数が weight に混ざらない。
///
/// 群 class は target に依存しないので、同じ player の複数 target を評価する場合は同じ
/// instance を使い回すと、target ごとの追加コストは畳み込みと DP だけになる。
///
/// 国士無双は牌数ベクタが么九牌13種に固定され、標準形とも七対子とも同時に成立しないので、
/// DP へ載せず直接数える。
pub struct CompressedHiddenHandStates<'a> {
    context: &'a GameContext,
    fixed_meld_count: FixedMeldCount,
    concealed_hand_len: u8,
    remaining: [u8; TileType::COUNT],
    unron_mask: u64,
    unron_tiles: Vec<TileType>,
    specs: [GroupSpec; GROUP_COUNT],
    groups: [Vec<GroupClass>; GROUP_COUNT],
    tenpai_state_weight: TenpaiStateWeight,
    metrics: CompressedHiddenHandStateMetrics,
}

impl<'a> CompressedHiddenHandStates<'a> {
    /// 対象リーチ者の公開情報から compressed state space を組み立てる。
    ///
    /// 受け付けない入力の区別は [`ReachedHiddenHandStates`](super::ReachedHiddenHandStates) と
    /// 同じで、リーチしていない player や副露を持つ player は推測で補完しない。
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
        let mut metrics = CompressedHiddenHandStateMetrics::default();
        let table_start = Instant::now();
        let specs = GroupSpec::all();
        metrics.block_tables = table_start.elapsed();

        let start = Instant::now();
        let groups = specs.map(|spec| {
            let (classes, visited) = enumerate_group_classes(
                spec,
                &input.remaining,
                input.concealed_hand_len,
                !input.fixed_meld_count.has_melds(),
            );
            metrics.enumerated_group_vectors += visited;
            classes
        });
        metrics.precomputation = start.elapsed();
        metrics.retained_group_classes = groups.iter().map(|classes| classes.len() as u64).sum();

        let mut states = Self {
            context: input.context,
            fixed_meld_count: input.fixed_meld_count,
            concealed_hand_len: input.concealed_hand_len,
            remaining: input.remaining,
            unron_mask: input.unron_mask,
            unron_tiles: input.unron_tiles,
            specs,
            groups,
            tenpai_state_weight: TenpaiStateWeight::default(),
            metrics,
        };

        let start = Instant::now();
        states.tenpai_state_weight = states.calculate_tenpai_state_weight();
        states.metrics.tenpai_calculation = start.elapsed();

        states
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
        self.unron_mask & (1 << tile.index()) != 0
    }

    /// 対象 player に対して既にロン不能な牌種。フリテン判定はこの集合だけを見る。
    pub fn unron_capable_tiles(&self) -> &[TileType] {
        &self.unron_tiles
    }

    /// 評価対象の `GameContext`。
    pub fn context(&self) -> &GameContext {
        self.context
    }

    /// これまでの評価の内訳計測値。計測用で、数え上げ結果には影響しない。
    pub fn metrics(&self) -> CompressedHiddenHandStateMetrics {
        self.metrics
    }

    /// 公開情報と整合する全 structural tenpai 隠れ手牌状態の target-independent な総量。
    ///
    /// player 自身の河や post-reach passed tiles はここから状態を直接除外しない。また構造待ちの
    /// 現在残枚数が0でも、隠れ手牌そのものが `remaining` と整合すれば状態に含める。
    /// この値は [`Self::new`] で1回だけ計算され、target 評価では再計算しない。
    pub fn tenpai_state_weight(&self) -> TenpaiStateWeight {
        self.tenpai_state_weight
    }

    /// 対象牌の exact `R(p, x)` と、事前計算済みの exact `T(p)` をまとめて返す。
    pub fn ron_risk_evidence(&mut self, target: TileType) -> RonRiskEvidence {
        let ron_capable = self.ron_capable_state_weight(target);
        RonRiskEvidence {
            ron_capable_weight: ron_capable.weight,
            tenpai_weight: self.tenpai_state_weight.weight,
        }
    }

    /// 対象牌で実際にロンできる隠れ手牌状態の重みを exact に数える。
    ///
    /// 結果の定義は [`ReachedHiddenHandStates::ron_capable_state_weight`] と同じ。
    ///
    /// [`ReachedHiddenHandStates::ron_capable_state_weight`]:
    ///     super::ReachedHiddenHandStates::ron_capable_state_weight
    pub fn ron_capable_state_weight(&mut self, target: TileType) -> RonCapableStateWeight {
        self.completion_state_weight(target)
    }

    fn completion_state_weight(&mut self, target: TileType) -> RonCapableStateWeight {
        if self.is_unron_capable_tile(target) {
            return RonCapableStateWeight::default();
        }

        let start = Instant::now();
        let collapsed = self.collapse_groups(target);
        let (mut total, transitions) = self.fold_groups(&collapsed);
        self.metrics.dp_transitions += transitions;
        let kokushi = self.kokushi_weight(target);
        total.weight += kokushi.weight;
        total.states += kokushi.states;
        self.metrics.target_evaluation += start.elapsed();
        total
    }

    fn calculate_tenpai_state_weight(&mut self) -> TenpaiStateWeight {
        let collapsed = self.collapse_tenpai_groups();
        let (standard_or_chiitoitsu, transitions) = self.fold_groups(&collapsed);
        self.metrics.tenpai_dp_transitions += transitions;
        let kokushi = self.kokushi_tenpai_weight();
        TenpaiStateWeight {
            weight: standard_or_chiitoitsu.weight + kokushi.weight,
            states: standard_or_chiitoitsu.states + kokushi.states,
        }
    }

    // raw wait mask を「何らかの structural wait があるか」だけへ落とす。remaining に待ち牌が
    // 残っているかや、その牌がフリテンかは見ない。
    fn collapse_tenpai_groups(&mut self) -> [Vec<TargetClass>; GROUP_COUNT] {
        let mut collapsed: [Vec<TargetClass>; GROUP_COUNT] = Default::default();
        for (group, classes) in self.groups.iter().enumerate() {
            let mut folded: HashMap<TargetClassKey, (u128, u64)> = HashMap::new();
            for class in classes {
                let key = TargetClassKey {
                    size: class.key.size,
                    complete: class.key.complete,
                    complete_with_pair: class.key.complete_with_pair,
                    target_wait_to_complete: class.key.wait_to_complete != 0,
                    unron_wait_to_complete: false,
                    target_wait_with_pair: class.key.wait_to_complete_with_pair != 0,
                    unron_wait_with_pair: false,
                    chiitoitsu: match class.key.chiitoitsu {
                        ChiitoitsuShape::Broken => ChiitoitsuState::Broken,
                        ChiitoitsuShape::Pairs { pairs, single } => ChiitoitsuState::Pairs {
                            pairs,
                            single: if single.is_some() {
                                SingleKind::Target
                            } else {
                                SingleKind::None
                            },
                        },
                    },
                };
                let entry = folded.entry(key).or_insert((0, 0));
                entry.0 += u128::from(class.weight);
                entry.1 += class.states;
            }

            collapsed[group] = folded
                .into_iter()
                .map(|(key, (weight, states))| TargetClass {
                    key,
                    weight,
                    states,
                })
                .collect();
            self.metrics.collapsed_tenpai_classes += collapsed[group].len() as u64;
        }
        collapsed
    }

    // 群 class を target 依存の bool だけへ落とす。ここで初めて target とロン不能牌が効く。
    fn collapse_groups(&mut self, target: TileType) -> [Vec<TargetClass>; GROUP_COUNT] {
        let mut collapsed: [Vec<TargetClass>; GROUP_COUNT] = Default::default();
        for (group, classes) in self.groups.iter().enumerate() {
            let GroupSpec { base, tiles, .. } = self.specs[group];
            let target_local = (target.index() >= base && target.index() < base + tiles)
                .then(|| target.index() - base);
            let unron_local = ((self.unron_mask >> base) as u16) & ((1u16 << tiles) - 1);

            let mut folded: HashMap<TargetClassKey, (u128, u64)> = HashMap::new();
            for class in classes {
                let key = TargetClassKey {
                    size: class.key.size,
                    complete: class.key.complete,
                    complete_with_pair: class.key.complete_with_pair,
                    target_wait_to_complete: target_local
                        .is_some_and(|local| class.key.wait_to_complete >> local & 1 == 1),
                    unron_wait_to_complete: class.key.wait_to_complete & unron_local != 0,
                    target_wait_with_pair: target_local.is_some_and(|local| {
                        class.key.wait_to_complete_with_pair >> local & 1 == 1
                    }),
                    unron_wait_with_pair: class.key.wait_to_complete_with_pair & unron_local != 0,
                    chiitoitsu: match class.key.chiitoitsu {
                        ChiitoitsuShape::Broken => ChiitoitsuState::Broken,
                        ChiitoitsuShape::Pairs { pairs, single } => ChiitoitsuState::Pairs {
                            pairs,
                            single: match single {
                                None => SingleKind::None,
                                Some(local) if target_local == Some(usize::from(local)) => {
                                    SingleKind::Target
                                }
                                Some(local) if unron_local >> local & 1 == 1 => SingleKind::Unron,
                                Some(_) => SingleKind::Other,
                            },
                        },
                    },
                };
                let entry = folded.entry(key).or_insert((0, 0));
                entry.0 += u128::from(class.weight);
                entry.1 += class.states;
            }

            collapsed[group] = folded
                .into_iter()
                .map(|(key, (weight, states))| TargetClass {
                    key,
                    weight,
                    states,
                })
                .collect();
            self.metrics.collapsed_target_classes += collapsed[group].len() as u64;
        }
        collapsed
    }

    // 群 class を順に畳み込み、標準形と七対子の待ちを同時に組み立てる。
    fn fold_groups(
        &self,
        collapsed: &[Vec<TargetClass>; GROUP_COUNT],
    ) -> (RonCapableStateWeight, u64) {
        let hand_len = usize::from(self.concealed_hand_len);
        let width = PHASE_COUNT * CHIITOITSU_STATE_COUNT;
        let len = (hand_len + 1) * width;

        let mut current = vec![(0u128, 0u64); len];
        let initial_chiitoitsu = if self.fixed_meld_count.has_melds() {
            ChiitoitsuState::Broken
        } else {
            ChiitoitsuState::Pairs {
                pairs: 0,
                single: SingleKind::None,
            }
        };
        current[phase_index(StandardPhase::Empty) * CHIITOITSU_STATE_COUNT
            + chiitoitsu_index(initial_chiitoitsu)] = (1, 1);

        let mut transitions = 0u64;
        for classes in collapsed {
            let mut next = vec![(0u128, 0u64); len];
            for (state, &(weight, states)) in current.iter().enumerate() {
                if states == 0 {
                    continue;
                }
                let size = state / width;
                let phase = phase_from_index((state % width) / CHIITOITSU_STATE_COUNT);
                let chiitoitsu = chiitoitsu_from_index(state % CHIITOITSU_STATE_COUNT);
                for class in classes {
                    let size = size + usize::from(class.key.size);
                    if size > hand_len {
                        continue;
                    }
                    let phase = advance_phase(phase, &class.key);
                    let chiitoitsu = advance_chiitoitsu(chiitoitsu, &class.key);
                    if phase == StandardPhase::Dead && chiitoitsu == ChiitoitsuState::Broken {
                        continue;
                    }
                    transitions += 1;
                    let index = size * width
                        + phase_index(phase) * CHIITOITSU_STATE_COUNT
                        + chiitoitsu_index(chiitoitsu);
                    next[index].0 += weight * class.weight;
                    next[index].1 += states * class.states;
                }
            }
            current = next;
        }
        let mut total = RonCapableStateWeight::default();
        for phase in 0..PHASE_COUNT {
            for chiitoitsu in 0..CHIITOITSU_STATE_COUNT {
                let (weight, states) =
                    current[hand_len * width + phase * CHIITOITSU_STATE_COUNT + chiitoitsu];
                if states == 0 {
                    continue;
                }
                let (standard_target, standard_unron) = match phase_from_index(phase) {
                    StandardPhase::Wait { target, unron }
                    | StandardPhase::Paired { target, unron } => (target, unron),
                    _ => (false, false),
                };
                let (chiitoitsu_target, chiitoitsu_unron) = match chiitoitsu_from_index(chiitoitsu)
                {
                    ChiitoitsuState::Pairs { pairs: 6, single } => {
                        (single == SingleKind::Target, single == SingleKind::Unron)
                    }
                    _ => (false, false),
                };
                if standard_unron || chiitoitsu_unron {
                    continue;
                }
                if !standard_target && !chiitoitsu_target {
                    continue;
                }
                total.weight += weight;
                total.states += states;
            }
        }
        (total, transitions)
    }

    // 国士無双テンパイの隠れ手牌を直接数える。13面待ちと、target 単騎の12種+対子だけが対象。
    fn kokushi_weight(&self, target: TileType) -> RonCapableStateWeight {
        if self.fixed_meld_count.has_melds() || !target.is_yaochu() {
            return RonCapableStateWeight::default();
        }
        let yaochu: Vec<TileType> = TileType::all().filter(|tile| tile.is_yaochu()).collect();
        let mut total = RonCapableStateWeight::default();

        if yaochu
            .iter()
            .all(|&tile| self.remaining[tile.index()] >= 1 && !self.is_unron_capable_tile(tile))
        {
            total.weight += yaochu
                .iter()
                .map(|&tile| u128::from(self.remaining[tile.index()]))
                .product::<u128>();
            total.states += 1;
        }

        for &pair in &yaochu {
            if pair == target || self.remaining[pair.index()] < 2 {
                continue;
            }
            let others = yaochu
                .iter()
                .filter(|&&tile| tile != pair && tile != target);
            if others.clone().any(|&tile| self.remaining[tile.index()] < 1) {
                continue;
            }
            let pair_copies = u128::from(self.remaining[pair.index()]);
            total.weight += pair_copies * (pair_copies - 1) / 2
                * others
                    .map(|&tile| u128::from(self.remaining[tile.index()]))
                    .product::<u128>();
            total.states += 1;
        }
        total
    }

    // 国士の13面待ち1状態と、欠けた么九牌・対子牌の全組み合わせを target 非依存に数える。
    fn kokushi_tenpai_weight(&self) -> TenpaiStateWeight {
        if self.fixed_meld_count.has_melds() {
            return TenpaiStateWeight::default();
        }
        let yaochu: Vec<TileType> = TileType::all().filter(|tile| tile.is_yaochu()).collect();
        let mut total = TenpaiStateWeight::default();

        if yaochu.iter().all(|&tile| self.remaining[tile.index()] >= 1) {
            total.weight += yaochu
                .iter()
                .map(|&tile| u128::from(self.remaining[tile.index()]))
                .product::<u128>();
            total.states += 1;
        }

        for &missing in &yaochu {
            for &pair in &yaochu {
                if pair == missing || self.remaining[pair.index()] < 2 {
                    continue;
                }
                let others = yaochu
                    .iter()
                    .filter(|&&tile| tile != pair && tile != missing);
                if others.clone().any(|&tile| self.remaining[tile.index()] < 1) {
                    continue;
                }
                let pair_copies = u128::from(self.remaining[pair.index()]);
                total.weight += pair_copies * (pair_copies - 1) / 2
                    * others
                        .map(|&tile| u128::from(self.remaining[tile.index()]))
                        .product::<u128>();
                total.states += 1;
            }
        }
        total
    }
}

/// 公開副露を固定した、非リーチ相手の compressed conditional-tenpai model。
///
/// structural tenpai denominator と、対象牌を加えると Standard の structural completion になる
/// state weight を exact に数える。`T` は compressed DP で数え、`R` は yaku rule を DP へ複製せず
/// target completion 候補だけを共通 enumerator で列挙して既存 evaluator を呼ぶ。
/// current furiten を反映した [`RonRiskEvidence`] を構築できるが、テンパイ確率は扱わない。
pub struct CompressedStructuralTenpaiHiddenHandStates<'a> {
    player: usize,
    context: &'a GameContext,
    inner: CompressedHiddenHandStates<'a>,
    ron_counter: Option<StructuralTenpaiHiddenHandStates<'a>>,
}

impl<'a> CompressedStructuralTenpaiHiddenHandStates<'a> {
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
            inner: CompressedHiddenHandStates::from_input(input),
            ron_counter: None,
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

    /// 公開情報と固定副露に整合する全 Standard structural tenpai state の総量。
    pub fn tenpai_state_weight(&self) -> TenpaiStateWeight {
        self.inner.tenpai_state_weight()
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
    pub fn ron_capable_state_weight(
        &mut self,
        target: TileType,
    ) -> Result<RonCapableStateWeight, HiddenHandStateUnsupported> {
        if self.ron_counter.is_none() {
            self.ron_counter = Some(StructuralTenpaiHiddenHandStates::new(
                self.player,
                self.context,
            )?);
        }
        self.ron_counter
            .as_mut()
            .expect("initialized above")
            .ron_capable_state_weight(target)
    }

    /// OpenHand target の exact `R(p, x)` と structural tenpai `T(p)` を返す。
    ///
    /// `R` は役・named Yakuman・current furiten を反映するが、`T` は役なし state も含む PR #209
    /// の structural denominator をそのまま使う。必要な局面情報が unknown なら unavailable。
    pub fn ron_risk_evidence(
        &mut self,
        target: TileType,
    ) -> Result<RonRiskEvidence, HiddenHandStateUnsupported> {
        let ron_capable = self.ron_capable_state_weight(target)?;
        Ok(RonRiskEvidence {
            ron_capable_weight: ron_capable.weight,
            tenpai_weight: self.tenpai_state_weight().weight,
        })
    }

    /// 評価対象の `GameContext`。
    pub fn context(&self) -> &GameContext {
        self.inner.context()
    }

    /// これまでの評価の内訳計測値。計測用で、数え上げ結果には影響しない。
    pub fn metrics(&self) -> CompressedHiddenHandStateMetrics {
        self.inner.metrics()
    }

    /// 遅延生成された OpenHand `R` enumerator の内訳計測値。
    ///
    /// `ron_capable_state_weight` がまだ呼ばれていない場合は `None`。compressed `T` 側の
    /// [`Self::metrics`] と異なり、実際に `R` を数えた enumerating path の累計を返す。
    pub fn ron_metrics(&self) -> Option<HiddenHandStateMetrics> {
        self.ron_counter
            .as_ref()
            .and_then(StructuralTenpaiHiddenHandStates::ron_metrics)
    }
}

/// 対象牌で実際にロンできる隠れ手牌状態の重みを、単発評価用に compressed counting で求める。
///
/// 同じ player の複数 target を評価する場合は [`CompressedHiddenHandStates`] を使い回すほうが、
/// 群単位の状態圧縮を共有できる。
pub fn compressed_ron_capable_hidden_hand_weight(
    target: TileType,
    player: usize,
    context: &GameContext,
) -> Result<RonCapableStateWeight, HiddenHandStateUnsupported> {
    Ok(CompressedHiddenHandStates::new(player, context)?.ron_capable_state_weight(target))
}

/// 公開情報と整合する全 structural tenpai 隠れ手牌状態の総量を単発で求める。
pub fn compressed_tenpai_hidden_hand_weight(
    player: usize,
    context: &GameContext,
) -> Result<TenpaiStateWeight, HiddenHandStateUnsupported> {
    Ok(CompressedHiddenHandStates::new(player, context)?.tenpai_state_weight())
}
