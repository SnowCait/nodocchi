//! 非リーチ副露相手の観測事実を段階的に比較するための scenario corpus と、その回帰テスト。
//!
//! 各 fixture は同じ自分の攻撃状態に対して相手の副露だけを変えたもので、現行の
//! `PlayerThreatFacts` / `OpenHandThreat` / 押し引き / 通常打牌 selected を並べて比較するための
//! 固定局面。corpus 側で threat score や副露評価を再実装せず、production が構築した facts と
//! classification をそのまま確認する。
//!
//! `decide_push_pull()` は `High` の副露相手だけを threat として扱うため、`None` / `Present` の
//! fixture は従来どおり `NoThreat` → `Push`、`High` の fixture は自分の攻撃状態で分かれる。
//! 強いテンパイの自分なら `StrongTenpaiAgainstHighOpenHand` → `Push`、一向聴の自分なら受け入れの
//! 強さにかかわらず `IishantenAgainstHighOpenHand` → `Fold`、二向聴の自分なら
//! `TwoOrMoreShantenAgainstHighOpenHand` → `Fold` になることを固定する。

use bot_core::{
    Agent, DiagnosticOptions, LegalAction, MeldKindCounts, OpenHandThreatAssessment,
    OpenHandThreatDecision, OpenHandThreatExclusion, OpenHandThreatLevel, OpenHandThreatReason,
    PlayerThreatFacts, PushPullDecision, PushPullInputs, PushPullMode, PushPullOffenseState,
    PushPullReason, ShantenAgent, ShantenDecisionDiagnostic, SuitedSafetyRank, SujiSafetyRank,
    ValueHonorMeldCounts, WallRank, classify_open_hand_threat, honor_safety_rank,
    is_discarded_by_all_open_hand_threats, opponent_honor_value_for_open_hand_threats,
    suited_safety_rank_for_open_hand_threats, suji_safety_rank_for,
    suji_safety_rank_for_open_hand_threats, wall_rank,
};
use bot_logic::{TileId, TileType};

use crate::scenario::{Scenario, ScenarioSpec};

const BASELINE: &str = include_str!("../scenarios/open_hand_baseline.json");
const CHI: &str = include_str!("../scenarios/open_hand_chi.json");
const VALUE_PON: &str = include_str!("../scenarios/open_hand_value_pon.json");
const TWO_MELDS: &str = include_str!("../scenarios/open_hand_two_melds.json");
const VALUE_PON_AND_CHI: &str = include_str!("../scenarios/open_hand_value_pon_and_chi.json");
const DORA_MELDS: &str = include_str!("../scenarios/open_hand_dora_melds.json");
const TWO_MELDS_NINE_DISCARDS: &str =
    include_str!("../scenarios/open_hand_two_melds_nine_discards.json");
const CHI_TWELVE_DISCARDS: &str = include_str!("../scenarios/open_hand_chi_twelve_discards.json");
const THREE_MELDS: &str = include_str!("../scenarios/open_hand_three_melds.json");
const THREE_MELDS_VALUE_DORA: &str =
    include_str!("../scenarios/open_hand_three_melds_value_dora.json");
const DEALER_VALUE_PON: &str = include_str!("../scenarios/open_hand_dealer_value_pon.json");
const ANKAN: &str = include_str!("../scenarios/open_hand_ankan.json");
const WEAK_BASELINE: &str = include_str!("../scenarios/open_hand_weak_baseline.json");
const WEAK_VALUE_PON: &str = include_str!("../scenarios/open_hand_weak_value_pon.json");
const WEAK_THREE_MELDS_VALUE_DORA: &str =
    include_str!("../scenarios/open_hand_weak_three_melds_value_dora.json");
const IISHANTEN_BASELINE: &str = include_str!("../scenarios/open_hand_iishanten_baseline.json");
const IISHANTEN_THREE_MELDS: &str =
    include_str!("../scenarios/open_hand_iishanten_three_melds.json");
const WEAK_IISHANTEN_BASELINE: &str =
    include_str!("../scenarios/open_hand_weak_iishanten_baseline.json");
const WEAK_IISHANTEN_THREE_MELDS: &str =
    include_str!("../scenarios/open_hand_weak_iishanten_three_melds.json");

// 自分の席は player 0、親は player 1 に固定する。
const SELF_PLAYER: usize = 0;
const DEALER_PLAYER: usize = 1;

// 全 fixture で共通の河。player 2 だけが 5 枚切っており、副露を持つ席の河は空。
const RIVER_PLAYER: usize = 2;
const RIVER_DISCARD_COUNT: usize = 5;

/// corpus が持つ2種類の自分側局面。副露 threat と自分の攻撃力を後から組み合わせるための軸で、
/// 同じ hand を共有する fixture 同士は通常打牌・offense が一致する。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SelfHand {
    /// 打 N で非フリテンの強いテンパイ (待ち 6 枚以上) になる局面。
    Tenpai,
    /// 打 N で受け入れの広い一向聴 (受け入れ 8 枚以上・2 種類以上) に留まる局面。
    StrongIishanten,
    /// 打 1p で受け入れの狭い一向聴 (受け入れ 7 枚 / 2 種類) に留まる局面。受け入れ牌をほぼ
    /// 見え牌にして固定してある。
    WeakIishanten,
    /// 打 S で二向聴に留まる局面。
    TwoShanten,
}

/// 副露を持つ席に期待する観測事実と、そこから production が導く暫定 classification。
/// `PlayerThreatFacts` / `OpenHandThreatDecision` の値をそのまま比較するためだけの表で、
/// corpus 側で危険度を計算し直さない。
#[derive(Debug, Clone, Copy)]
struct ExpectedOpenHand {
    player: usize,
    discard_count: usize,
    meld_count: usize,
    open_meld_count: usize,
    kan_count: usize,
    meld_kinds: MeldKindCounts,
    meld_dora_count: u8,
    meld_red_dora_count: u8,
    value_honor_melds: ValueHonorMeldCounts,
    open_meld_dora_count: u8,
    open_meld_red_dora_count: u8,
    open_value_honor_melds: ValueHonorMeldCounts,
    threat: OpenHandThreatDecision,
}

#[derive(Debug, Clone, Copy)]
struct CorpusScenario {
    name: &'static str,
    json: &'static str,
    self_hand: SelfHand,
    /// 副露を持つ席。副露なしの基準局面は `None`。
    melded: Option<ExpectedOpenHand>,
}

fn chi_counts(chi: usize) -> MeldKindCounts {
    MeldKindCounts {
        chi,
        ..MeldKindCounts::default()
    }
}

fn chi_and_pon_counts(chi: usize, pon: usize) -> MeldKindCounts {
    MeldKindCounts {
        chi,
        pon,
        ..MeldKindCounts::default()
    }
}

// 三元牌の刻子。風情報が無くても役牌と確定する。
fn dragon_meld() -> ValueHonorMeldCounts {
    ValueHonorMeldCounts {
        dragon: 1,
        confirmed: 1,
        ..ValueHonorMeldCounts::default()
    }
}

fn no_open_meld() -> OpenHandThreatDecision {
    OpenHandThreatDecision {
        level: OpenHandThreatLevel::None,
        reason: OpenHandThreatReason::NoOpenMeld,
    }
}

fn present() -> OpenHandThreatDecision {
    OpenHandThreatDecision {
        level: OpenHandThreatLevel::Present,
        reason: OpenHandThreatReason::OpenMeldPresent,
    }
}

fn high(reason: OpenHandThreatReason) -> OpenHandThreatDecision {
    OpenHandThreatDecision {
        level: OpenHandThreatLevel::High,
        reason,
    }
}

// 役牌もドラも含まない Chi 3組を持つ子。3副露なので High になる。
fn three_plain_chi() -> ExpectedOpenHand {
    ExpectedOpenHand {
        player: 3,
        discard_count: 0,
        meld_count: 3,
        open_meld_count: 3,
        kan_count: 0,
        meld_kinds: chi_counts(3),
        meld_dora_count: 0,
        meld_red_dora_count: 0,
        value_honor_melds: ValueHonorMeldCounts::default(),
        open_meld_dora_count: 0,
        open_meld_red_dora_count: 0,
        open_value_honor_melds: ValueHonorMeldCounts::default(),
        threat: high(OpenHandThreatReason::ThreeOrMoreOpenMelds),
    }
}

fn corpus() -> Vec<CorpusScenario> {
    vec![
        CorpusScenario {
            name: "open_hand_baseline",
            json: BASELINE,
            self_hand: SelfHand::Tenpai,
            melded: None,
        },
        CorpusScenario {
            name: "open_hand_chi",
            json: CHI,
            self_hand: SelfHand::Tenpai,
            melded: Some(ExpectedOpenHand {
                player: 3,
                discard_count: 0,
                meld_count: 1,
                open_meld_count: 1,
                kan_count: 0,
                meld_kinds: chi_counts(1),
                meld_dora_count: 0,
                meld_red_dora_count: 0,
                value_honor_melds: ValueHonorMeldCounts::default(),
                open_meld_dora_count: 0,
                open_meld_red_dora_count: 0,
                open_value_honor_melds: ValueHonorMeldCounts::default(),
                threat: present(),
            }),
        },
        CorpusScenario {
            name: "open_hand_value_pon",
            json: VALUE_PON,
            self_hand: SelfHand::Tenpai,
            melded: Some(ExpectedOpenHand {
                player: 3,
                discard_count: 0,
                meld_count: 1,
                open_meld_count: 1,
                kan_count: 0,
                meld_kinds: chi_and_pon_counts(0, 1),
                meld_dora_count: 0,
                meld_red_dora_count: 0,
                value_honor_melds: dragon_meld(),
                open_meld_dora_count: 0,
                open_meld_red_dora_count: 0,
                open_value_honor_melds: dragon_meld(),
                threat: present(),
            }),
        },
        CorpusScenario {
            name: "open_hand_two_melds",
            json: TWO_MELDS,
            self_hand: SelfHand::Tenpai,
            melded: Some(ExpectedOpenHand {
                player: 3,
                discard_count: 0,
                meld_count: 2,
                open_meld_count: 2,
                kan_count: 0,
                meld_kinds: chi_counts(2),
                meld_dora_count: 0,
                meld_red_dora_count: 0,
                value_honor_melds: ValueHonorMeldCounts::default(),
                open_meld_dora_count: 0,
                open_meld_red_dora_count: 0,
                open_value_honor_melds: ValueHonorMeldCounts::default(),
                threat: present(),
            }),
        },
        CorpusScenario {
            name: "open_hand_value_pon_and_chi",
            json: VALUE_PON_AND_CHI,
            self_hand: SelfHand::Tenpai,
            melded: Some(ExpectedOpenHand {
                player: 3,
                discard_count: 0,
                meld_count: 2,
                open_meld_count: 2,
                kan_count: 0,
                meld_kinds: chi_and_pon_counts(1, 1),
                meld_dora_count: 0,
                meld_red_dora_count: 0,
                value_honor_melds: dragon_meld(),
                open_meld_dora_count: 0,
                open_meld_red_dora_count: 0,
                open_value_honor_melds: dragon_meld(),
                threat: present(),
            }),
        },
        CorpusScenario {
            name: "open_hand_dora_melds",
            json: DORA_MELDS,
            self_hand: SelfHand::Tenpai,
            melded: Some(ExpectedOpenHand {
                player: 3,
                discard_count: 0,
                meld_count: 2,
                open_meld_count: 2,
                kan_count: 0,
                meld_kinds: chi_counts(2),
                meld_dora_count: 2,
                meld_red_dora_count: 1,
                value_honor_melds: ValueHonorMeldCounts::default(),
                open_meld_dora_count: 2,
                open_meld_red_dora_count: 1,
                open_value_honor_melds: ValueHonorMeldCounts::default(),
                threat: high(OpenHandThreatReason::TwoOrMoreWithVisibleHan),
            }),
        },
        CorpusScenario {
            name: "open_hand_two_melds_nine_discards",
            json: TWO_MELDS_NINE_DISCARDS,
            self_hand: SelfHand::Tenpai,
            melded: Some(ExpectedOpenHand {
                player: 3,
                discard_count: 9,
                meld_count: 2,
                open_meld_count: 2,
                kan_count: 0,
                meld_kinds: chi_counts(2),
                meld_dora_count: 0,
                meld_red_dora_count: 0,
                value_honor_melds: ValueHonorMeldCounts::default(),
                open_meld_dora_count: 0,
                open_meld_red_dora_count: 0,
                open_value_honor_melds: ValueHonorMeldCounts::default(),
                // 役牌もドラも無い2副露でも、河が9枚まで進むと High になる。
                threat: high(OpenHandThreatReason::TwoOrMoreOpenMeldsFromNineDiscards),
            }),
        },
        CorpusScenario {
            name: "open_hand_chi_twelve_discards",
            json: CHI_TWELVE_DISCARDS,
            self_hand: SelfHand::Tenpai,
            melded: Some(ExpectedOpenHand {
                player: 3,
                discard_count: 12,
                meld_count: 1,
                open_meld_count: 1,
                kan_count: 0,
                meld_kinds: chi_counts(1),
                meld_dora_count: 0,
                meld_red_dora_count: 0,
                value_honor_melds: ValueHonorMeldCounts::default(),
                open_meld_dora_count: 0,
                open_meld_red_dora_count: 0,
                open_value_honor_melds: ValueHonorMeldCounts::default(),
                threat: high(OpenHandThreatReason::OpenMeldFromTwelveDiscards),
            }),
        },
        CorpusScenario {
            name: "open_hand_three_melds",
            json: THREE_MELDS,
            self_hand: SelfHand::Tenpai,
            melded: Some(ExpectedOpenHand {
                player: 3,
                discard_count: 0,
                meld_count: 3,
                open_meld_count: 3,
                kan_count: 0,
                meld_kinds: chi_counts(3),
                meld_dora_count: 0,
                meld_red_dora_count: 0,
                value_honor_melds: ValueHonorMeldCounts::default(),
                open_meld_dora_count: 0,
                open_meld_red_dora_count: 0,
                open_value_honor_melds: ValueHonorMeldCounts::default(),
                threat: high(OpenHandThreatReason::ThreeOrMoreOpenMelds),
            }),
        },
        CorpusScenario {
            name: "open_hand_three_melds_value_dora",
            json: THREE_MELDS_VALUE_DORA,
            self_hand: SelfHand::Tenpai,
            melded: Some(ExpectedOpenHand {
                player: 3,
                discard_count: 0,
                meld_count: 3,
                open_meld_count: 3,
                kan_count: 0,
                meld_kinds: chi_and_pon_counts(2, 1),
                meld_dora_count: 2,
                meld_red_dora_count: 1,
                value_honor_melds: dragon_meld(),
                open_meld_dora_count: 2,
                open_meld_red_dora_count: 1,
                open_value_honor_melds: dragon_meld(),
                // 役牌・ドラの条件も満たすが、優先順位により3副露の reason になる。
                threat: high(OpenHandThreatReason::ThreeOrMoreOpenMelds),
            }),
        },
        CorpusScenario {
            name: "open_hand_dealer_value_pon",
            json: DEALER_VALUE_PON,
            self_hand: SelfHand::Tenpai,
            melded: Some(ExpectedOpenHand {
                player: DEALER_PLAYER,
                discard_count: 0,
                meld_count: 1,
                open_meld_count: 1,
                kan_count: 0,
                meld_kinds: chi_and_pon_counts(0, 1),
                meld_dora_count: 0,
                meld_red_dora_count: 0,
                value_honor_melds: dragon_meld(),
                open_meld_dora_count: 0,
                open_meld_red_dora_count: 0,
                open_value_honor_melds: dragon_meld(),
                // 親でも1副露では High にしない。
                threat: present(),
            }),
        },
        CorpusScenario {
            name: "open_hand_ankan",
            json: ANKAN,
            self_hand: SelfHand::Tenpai,
            melded: Some(ExpectedOpenHand {
                player: 3,
                discard_count: 0,
                meld_count: 1,
                open_meld_count: 0,
                kan_count: 1,
                meld_kinds: MeldKindCounts {
                    ankan: 1,
                    ..MeldKindCounts::default()
                },
                meld_dora_count: 0,
                meld_red_dora_count: 0,
                value_honor_melds: ValueHonorMeldCounts::default(),
                open_meld_dora_count: 0,
                open_meld_red_dora_count: 0,
                open_value_honor_melds: ValueHonorMeldCounts::default(),
                threat: no_open_meld(),
            }),
        },
        CorpusScenario {
            name: "open_hand_iishanten_baseline",
            json: IISHANTEN_BASELINE,
            self_hand: SelfHand::StrongIishanten,
            melded: None,
        },
        CorpusScenario {
            name: "open_hand_iishanten_three_melds",
            json: IISHANTEN_THREE_MELDS,
            self_hand: SelfHand::StrongIishanten,
            melded: Some(three_plain_chi()),
        },
        CorpusScenario {
            name: "open_hand_weak_iishanten_baseline",
            json: WEAK_IISHANTEN_BASELINE,
            self_hand: SelfHand::WeakIishanten,
            melded: None,
        },
        CorpusScenario {
            name: "open_hand_weak_iishanten_three_melds",
            json: WEAK_IISHANTEN_THREE_MELDS,
            self_hand: SelfHand::WeakIishanten,
            melded: Some(three_plain_chi()),
        },
        CorpusScenario {
            name: "open_hand_weak_baseline",
            json: WEAK_BASELINE,
            self_hand: SelfHand::TwoShanten,
            melded: None,
        },
        CorpusScenario {
            name: "open_hand_weak_value_pon",
            json: WEAK_VALUE_PON,
            self_hand: SelfHand::TwoShanten,
            melded: Some(ExpectedOpenHand {
                player: 3,
                discard_count: 0,
                meld_count: 1,
                open_meld_count: 1,
                kan_count: 0,
                meld_kinds: chi_and_pon_counts(0, 1),
                meld_dora_count: 0,
                meld_red_dora_count: 0,
                value_honor_melds: dragon_meld(),
                open_meld_dora_count: 0,
                open_meld_red_dora_count: 0,
                open_value_honor_melds: dragon_meld(),
                threat: present(),
            }),
        },
        CorpusScenario {
            name: "open_hand_weak_three_melds_value_dora",
            json: WEAK_THREE_MELDS_VALUE_DORA,
            self_hand: SelfHand::TwoShanten,
            melded: Some(ExpectedOpenHand {
                player: 3,
                discard_count: 0,
                meld_count: 3,
                open_meld_count: 3,
                kan_count: 0,
                meld_kinds: chi_and_pon_counts(2, 1),
                meld_dora_count: 2,
                meld_red_dora_count: 1,
                value_honor_melds: dragon_meld(),
                open_meld_dora_count: 2,
                open_meld_red_dora_count: 1,
                open_value_honor_melds: dragon_meld(),
                threat: high(OpenHandThreatReason::ThreeOrMoreOpenMelds),
            }),
        },
    ]
}

fn resolve(entry: &CorpusScenario) -> Scenario {
    let spec: ScenarioSpec =
        serde_json::from_str(entry.json).unwrap_or_else(|error| panic!("{}: {error}", entry.name));
    Scenario::resolve(&spec).unwrap_or_else(|error| panic!("{}: {error}", entry.name))
}

fn diagnose(scenario: &Scenario) -> ShantenDecisionDiagnostic {
    ShantenAgent::diagnose(&scenario.context, &scenario.legal_actions)
}

// 副露を持たない席に期待する観測事実。河だけは player 2 が 5 枚持つ。
fn no_melds(player: usize) -> ExpectedOpenHand {
    ExpectedOpenHand {
        player,
        discard_count: if player == RIVER_PLAYER {
            RIVER_DISCARD_COUNT
        } else {
            0
        },
        meld_count: 0,
        open_meld_count: 0,
        kan_count: 0,
        meld_kinds: MeldKindCounts::default(),
        meld_dora_count: 0,
        meld_red_dora_count: 0,
        value_honor_melds: ValueHonorMeldCounts::default(),
        open_meld_dora_count: 0,
        open_meld_red_dora_count: 0,
        open_value_honor_melds: ValueHonorMeldCounts::default(),
        threat: no_open_meld(),
    }
}

// その席に期待する観測事実。副露を持たない席は共通の `no_melds` になる。
fn expected_of(entry: &CorpusScenario, player: usize) -> ExpectedOpenHand {
    match entry.melded {
        Some(melded) if melded.player == player => melded,
        _ => no_melds(player),
    }
}

fn assert_open_hand_facts(name: &str, expected: ExpectedOpenHand, facts: PlayerThreatFacts) {
    let player = expected.player;
    assert_eq!(facts.player, player, "{name} player {player}");
    assert_eq!(
        facts.discard_count, expected.discard_count,
        "{name} player {player} discard_count"
    );
    assert_eq!(
        facts.meld_count, expected.meld_count,
        "{name} player {player} meld_count"
    );
    assert_eq!(
        facts.open_meld_count, expected.open_meld_count,
        "{name} player {player} open_meld_count"
    );
    assert_eq!(
        facts.kan_count, expected.kan_count,
        "{name} player {player} kan_count"
    );
    assert_eq!(
        facts.meld_kinds, expected.meld_kinds,
        "{name} player {player} meld_kinds"
    );
    assert_eq!(
        facts.meld_dora_count, expected.meld_dora_count,
        "{name} player {player} meld_dora_count"
    );
    assert_eq!(
        facts.meld_red_dora_count, expected.meld_red_dora_count,
        "{name} player {player} meld_red_dora_count"
    );
    assert_eq!(
        facts.value_honor_melds, expected.value_honor_melds,
        "{name} player {player} value_honor_melds"
    );
    assert_eq!(
        facts.open_meld_dora_count, expected.open_meld_dora_count,
        "{name} player {player} open_meld_dora_count"
    );
    assert_eq!(
        facts.open_meld_red_dora_count, expected.open_meld_red_dora_count,
        "{name} player {player} open_meld_red_dora_count"
    );
    assert_eq!(
        facts.open_value_honor_melds, expected.open_value_honor_melds,
        "{name} player {player} open_value_honor_melds"
    );
}

// 見え牌の牌種別枚数。副露が visible tiles をどう増やしたかを牌種で比較するために使う。
fn visible_tile_counts(scenario: &Scenario) -> [u8; 34] {
    let mut counts = [0u8; 34];
    for tile in scenario.context.visible_tiles() {
        counts[usize::from(tile.tile_type().raw())] += 1;
    }
    counts
}

// baseline と比べて増えた見え牌の牌種。
fn added_visible_tile_types(baseline: &Scenario, variant: &Scenario) -> Vec<TileType> {
    let base = visible_tile_counts(baseline);
    let variant = visible_tile_counts(variant);
    (0..34)
        .filter(|&raw| variant[raw] > base[raw])
        .filter_map(|raw| TileType::new(raw as u8))
        .collect()
}

// 通常打牌候補すべての受け入れ牌種。打牌選択が使った評価の受け入れをそのまま集める。
fn acceptance_tile_types(diagnostic: &ShantenDecisionDiagnostic) -> Vec<TileType> {
    let mut types: Vec<TileType> = diagnostic
        .normal_discard
        .as_ref()
        .expect("通常打牌が評価されている")
        .candidates
        .iter()
        .flat_map(|candidate| {
            candidate
                .evaluation
                .acceptance_after_discard
                .tiles
                .iter()
                .map(|tile| tile.tile)
        })
        .collect();
    types.sort_unstable_by_key(|tile| tile.raw());
    types.dedup();
    types
}

fn hand_tiles(scenario: &Scenario) -> Vec<TileId> {
    scenario.context.hand_tiles().to_vec()
}

// その fixture が High の副露相手を持つか。
fn has_high_threat(entry: &CorpusScenario) -> bool {
    entry
        .melded
        .is_some_and(|melded| melded.threat.level == OpenHandThreatLevel::High)
}

// その fixture に期待する押し引き。High の副露相手がいる場合だけ新しい policy の対象になる。
fn expected_push_pull(entry: &CorpusScenario) -> (PushPullMode, PushPullReason) {
    if !has_high_threat(entry) {
        return (PushPullMode::Push, PushPullReason::NoThreat);
    }
    match entry.self_hand {
        SelfHand::Tenpai => (
            PushPullMode::Push,
            PushPullReason::StrongTenpaiAgainstHighOpenHand,
        ),
        // 一向聴は受け入れの強さにかかわらず Fold。
        SelfHand::StrongIishanten | SelfHand::WeakIishanten => (
            PushPullMode::Fold,
            PushPullReason::IishantenAgainstHighOpenHand,
        ),
        SelfHand::TwoShanten => (
            PushPullMode::Fold,
            PushPullReason::TwoOrMoreShantenAgainstHighOpenHand,
        ),
    }
}

// その fixture で OpenHand 防御 target になるべき席。High の副露相手だけが対象。
fn expected_targets(entry: &CorpusScenario) -> Vec<usize> {
    entry
        .melded
        .filter(|melded| melded.threat.level == OpenHandThreatLevel::High)
        .map(|melded| vec![melded.player])
        .unwrap_or_default()
}

// 全 fixture の自分側局面。同じ hand を共有する fixture 同士は通常打牌・offense が一致する。
const SELF_HANDS: [SelfHand; 4] = [
    SelfHand::Tenpai,
    SelfHand::StrongIishanten,
    SelfHand::WeakIishanten,
    SelfHand::TwoShanten,
];

// fixture を1回だけ解決・診断した結果。同じ Scenario / 診断を意味ごとの assertion で共有し、
// 同じ局面を何度も production の decision path へ通さない。
struct Evaluated {
    entry: CorpusScenario,
    scenario: Scenario,
    diagnostic: ShantenDecisionDiagnostic,
}

impl Evaluated {
    fn new(entry: CorpusScenario) -> Self {
        let scenario = resolve(&entry);
        let diagnostic = diagnose(&scenario);
        Self {
            entry,
            scenario,
            diagnostic,
        }
    }

    fn name(&self) -> &'static str {
        self.entry.name
    }

    fn push_pull_inputs(&self) -> PushPullInputs {
        self.diagnostic
            .push_pull_inputs
            .unwrap_or_else(|| panic!("{}: 押し引き入力がある", self.name()))
    }

    fn push_pull_decision(&self) -> PushPullDecision {
        self.diagnostic
            .push_pull_decision
            .unwrap_or_else(|| panic!("{}: 押し引き判断がある", self.name()))
    }

    fn offense(&self) -> PushPullOffenseState {
        self.push_pull_inputs()
            .offense
            .unwrap_or_else(|| panic!("{}: offense がある", self.name()))
    }
}

// corpus 全 fixture を1回ずつ評価した集合。
struct EvaluatedCorpus {
    scenarios: Vec<Evaluated>,
}

impl EvaluatedCorpus {
    fn evaluate() -> Self {
        Self {
            scenarios: corpus().into_iter().map(Evaluated::new).collect(),
        }
    }

    fn iter(&self) -> impl Iterator<Item = &Evaluated> {
        self.scenarios.iter()
    }

    fn find(&self, name: &str) -> &Evaluated {
        self.iter()
            .find(|evaluated| evaluated.name() == name)
            .unwrap_or_else(|| panic!("{name} scenario"))
    }

    fn select(&self, predicate: impl Fn(&CorpusScenario) -> bool) -> Vec<&Evaluated> {
        self.iter()
            .filter(|evaluated| predicate(&evaluated.entry))
            .collect()
    }

    fn group(&self, self_hand: SelfHand) -> Vec<&Evaluated> {
        self.select(|entry| entry.self_hand == self_hand)
    }
}

fn assert_no_opponent_reach(evaluated: &Evaluated) {
    let name = evaluated.name();
    let inputs = evaluated.push_pull_inputs();

    assert_eq!(inputs.opponent_reach_count, 0, "{name}");
    assert!(!inputs.dealer_reacher, "{name}");
    assert!(!inputs.self_dealer, "{name}");
    assert_eq!(
        inputs.has_high_open_hand_threat(),
        has_high_threat(&evaluated.entry),
        "{name}"
    );
}

fn assert_the_expected_push_pull(evaluated: &Evaluated) {
    let name = evaluated.name();
    let decision = evaluated.push_pull_decision();
    let (mode, reason) = expected_push_pull(&evaluated.entry);

    assert_eq!(decision.mode, mode, "{name}");
    assert_eq!(decision.reason, reason, "{name}");
}

fn assert_the_expected_player_threat_facts(evaluated: &Evaluated) {
    let name = evaluated.name();

    for player in 0..4 {
        let facts = evaluated.diagnostic.player_threats[player].facts;
        assert!(!facts.reached, "{name} player {player}");
        assert_eq!(
            facts.is_self,
            Some(player == SELF_PLAYER),
            "{name} player {player}"
        );
        assert_eq!(
            facts.is_dealer,
            Some(player == DEALER_PLAYER),
            "{name} player {player}"
        );

        assert_open_hand_facts(name, expected_of(&evaluated.entry, player), facts);
        assert_eq!(
            facts.discard_count,
            evaluated
                .scenario
                .context
                .discards_of(player)
                .unwrap()
                .len(),
            "{name} player {player} discard_count"
        );
    }
}

// 副露 facts から production が導く暫定 classification を fixture ごとに固定する。
// 自分の席は対象外で、リーチ者はこの corpus にいない。
fn assert_the_expected_open_hand_threat(evaluated: &Evaluated) {
    let name = evaluated.name();

    for player in 0..4 {
        let threat = &evaluated.diagnostic.player_threats[player];
        let expected = if player == SELF_PLAYER {
            OpenHandThreatAssessment::NotApplicable(OpenHandThreatExclusion::SelfSeat)
        } else {
            OpenHandThreatAssessment::Classified(expected_of(&evaluated.entry, player).threat)
        };

        assert_eq!(threat.open_hand_threat, expected, "{name} player {player}");
        // 表示・診断は分類し直さず、同じ facts から求めた結果を共有する。
        assert_eq!(
            threat.open_hand_threat,
            classify_open_hand_threat(threat.facts),
            "{name} player {player}"
        );
    }
}

// 表示・押し引き・診断が同じ軽量 facts を共有していることを固定する。
fn assert_the_threat_facts_are_shared_with_push_pull(evaluated: &Evaluated) {
    let name = evaluated.name();
    let inputs = evaluated.push_pull_inputs();

    for player in 0..4 {
        assert_eq!(
            inputs.player_threats[player], evaluated.diagnostic.player_threats[player].facts,
            "{name} player {player}"
        );
    }
}

// target は Player threats の classification と同じ source of truth から選ぶ。
fn assert_the_high_threats_are_the_defense_targets(evaluated: &Evaluated) {
    let name = evaluated.name();

    assert_eq!(
        evaluated.diagnostic.open_hand_defense.targets,
        expected_targets(&evaluated.entry),
        "{name}"
    );
    for &player in &evaluated.diagnostic.open_hand_defense.targets {
        assert_eq!(
            evaluated.diagnostic.player_threats[player]
                .open_hand_threat
                .level(),
            Some(OpenHandThreatLevel::High),
            "{name} player {player}"
        );
    }
}

// Present に留まる fixture は行動を変えない。副露なしの基準局面と同じ判断になる。
fn assert_the_present_threat_keeps_the_push_pull(evaluated: &Evaluated) {
    let name = evaluated.name();
    let decision = evaluated.push_pull_decision();

    assert_eq!(decision.mode, PushPullMode::Push, "{name}");
    assert_eq!(decision.reason, PushPullReason::NoThreat, "{name}");
    assert!(
        !evaluated.diagnostic.open_hand_defense.has_target(),
        "{name}"
    );
}

// High + 一向聴 / 二向聴以上の Fold では、通常打牌より OpenHand 防御 fallback を優先する。
fn assert_the_folding_scenario_selects_the_defense_fallback(evaluated: &Evaluated) {
    let name = evaluated.name();
    let diagnostic = &evaluated.diagnostic;

    let category = diagnostic
        .open_hand_defense_category()
        .unwrap_or_else(|| panic!("{name}: OpenHand 防御 fallback を採用している"));
    let selection = diagnostic
        .open_hand_defense
        .selected
        .as_ref()
        .unwrap_or_else(|| panic!("{name}: 診断に採用結果が載る"));

    assert_eq!(selection.selected_category, category, "{name}");
    assert_eq!(
        selection.selected_action, diagnostic.selected_action,
        "{name}"
    );
    // リーチ者がいないので、リーチ者向けの防御 fallback は検討自体が起きない。
    assert!(diagnostic.defense.is_none(), "{name}");
    assert_eq!(diagnostic.defense_fallback_kind(), None, "{name}");
}

// High でも強いテンパイで Push なら、安全牌を通常打牌より優先しない。
fn assert_the_pushing_scenario_keeps_the_normal_discard(evaluated: &Evaluated) {
    let name = evaluated.name();
    let diagnostic = &evaluated.diagnostic;

    assert_eq!(diagnostic.open_hand_defense_category(), None, "{name}");
    assert_eq!(diagnostic.open_hand_defense.selected, None, "{name}");
    assert_eq!(
        Some(diagnostic.selected_action.clone()),
        diagnostic.normal_discard_action,
        "{name}"
    );
}

// Present / None しか存在しない局面は「OpenHand Defense target なし」で、候補も持たない。
fn assert_no_defense_target(evaluated: &Evaluated) {
    let name = evaluated.name();

    assert!(
        !evaluated.diagnostic.open_hand_defense.has_target(),
        "{name}"
    );
    assert!(
        evaluated.diagnostic.open_hand_defense.candidates.is_empty(),
        "{name}"
    );
}

// 候補は合法 Dahai と同じ順序で、値は production の pure helper の結果そのもの。
fn assert_the_defense_safety_of_every_legal_dahai(evaluated: &Evaluated, targets: &[usize]) {
    let name = evaluated.name();
    let context = &evaluated.scenario.context;

    assert_eq!(
        evaluated
            .diagnostic
            .open_hand_defense
            .candidates
            .iter()
            .map(|candidate| candidate.action.clone())
            .collect::<Vec<LegalAction>>(),
        evaluated.scenario.legal_actions,
        "{name}"
    );

    for candidate in &evaluated.diagnostic.open_hand_defense.candidates {
        let tile = candidate.tile;
        let label = format!("{name} {}", tile.to_mjai_string());

        assert_eq!(
            candidate
                .targets
                .iter()
                .map(|target| target.player)
                .collect::<Vec<usize>>(),
            targets,
            "{label}"
        );
        assert_eq!(
            candidate.discarded_by_all_targets,
            is_discarded_by_all_open_hand_threats(tile, targets, context),
            "{label}"
        );
        assert_eq!(
            candidate.honor_safety_rank,
            honor_safety_rank(tile, context),
            "{label}"
        );
        assert_eq!(
            candidate.opponent_honor_value,
            opponent_honor_value_for_open_hand_threats(tile, targets, context),
            "{label}"
        );
        assert_eq!(
            candidate.wall_rank,
            (!tile.is_honor()).then(|| wall_rank(tile, context)),
            "{label}"
        );
        assert_eq!(
            candidate.suji_safety_rank,
            suji_safety_rank_for_open_hand_threats(tile, targets, context),
            "{label}"
        );
        assert_eq!(
            candidate.suited_safety_rank,
            suited_safety_rank_for_open_hand_threats(tile, targets, context),
            "{label}"
        );

        for target in &candidate.targets {
            assert_eq!(
                target.suji_safety_rank,
                suji_safety_rank_for(tile, target.player, context),
                "{label} target {}",
                target.player
            );
        }
    }
}

// 河を持つのは player 2 と副露を持つ席だけ。他家が切っただけの牌を安全根拠にしない。
fn assert_no_other_players_river_is_river_safe(evaluated: &Evaluated) {
    let name = evaluated.name();

    for candidate in &evaluated.diagnostic.open_hand_defense.candidates {
        for target in &candidate.targets {
            let river_has_tile = evaluated
                .scenario
                .context
                .discards_of(target.player)
                .expect("target の河")
                .iter()
                .any(|tile| tile.tile_type() == candidate.tile);
            assert_eq!(
                target.discarded_by_target,
                river_has_tile,
                "{name} {} target {}",
                candidate.tile.to_mjai_string(),
                target.player
            );
        }
    }
}

// 2副露 + 9捨て / 1副露 + 12捨て / 役牌入り2副露 / 3副露 の代表局面で target を固定する。
fn assert_the_representative_high_scenarios_fix_the_defense_target(corpus: &EvaluatedCorpus) {
    let representatives = [
        (
            "open_hand_two_melds_nine_discards",
            OpenHandThreatReason::TwoOrMoreOpenMeldsFromNineDiscards,
        ),
        (
            "open_hand_chi_twelve_discards",
            OpenHandThreatReason::OpenMeldFromTwelveDiscards,
        ),
        (
            "open_hand_dora_melds",
            OpenHandThreatReason::TwoOrMoreWithVisibleHan,
        ),
        (
            "open_hand_three_melds",
            OpenHandThreatReason::ThreeOrMoreOpenMelds,
        ),
    ];

    for (name, reason) in representatives {
        let diagnostic = &corpus.find(name).diagnostic;

        assert_eq!(diagnostic.open_hand_defense.targets, vec![3], "{name}");
        assert_eq!(
            diagnostic.player_threats[3].open_hand_threat.reason(),
            Some(reason),
            "{name}"
        );
        assert!(diagnostic.open_hand_defense.has_target(), "{name}");
    }
}

// 相手の河の 7m が3枚 + 自分の手牌の 7m で 8m の順子待ち経路が塞がる。壁は既存 helper の値。
fn assert_the_nine_discard_scenario_shares_the_existing_wall_rank(evaluated: &Evaluated) {
    let eight_man = TileType::from_mjai_type_str("8m").unwrap();

    let candidate = evaluated
        .diagnostic
        .open_hand_defense
        .candidates
        .iter()
        .find(|candidate| candidate.tile == eight_man)
        .expect("8m candidate");

    assert_eq!(
        candidate.wall_rank,
        Some(wall_rank(eight_man, &evaluated.scenario.context))
    );
    assert_eq!(candidate.wall_rank, Some(WallRank::NoChance));
    // 壁はスジより優先する。相手の河に 5m は無いのでスジは無い。
    assert_eq!(candidate.suji_safety_rank, Some(SujiSafetyRank::NoSuji));
    assert_eq!(
        candidate.suited_safety_rank,
        Some(SuitedSafetyRank::NoChance)
    );
}

fn assert_the_ankan_scenario_is_a_fixed_meld_but_not_an_open_meld(evaluated: &Evaluated) {
    let facts = evaluated.diagnostic.player_threats[3].facts;

    assert_eq!(facts.meld_count, 1);
    assert_eq!(facts.open_meld_count, 0);
    assert_eq!(facts.kan_count, 1);
    assert_eq!(facts.value_honor_melds, ValueHonorMeldCounts::default());
    assert_eq!(facts.open_meld_dora_count, 0);
    assert_eq!(
        facts.open_value_honor_melds,
        ValueHonorMeldCounts::default()
    );
    // 暗槓だけの相手は open hand の威圧材料を持たない。
    assert_eq!(
        classify_open_hand_threat(facts),
        OpenHandThreatAssessment::Classified(no_open_meld())
    );
}

// 同じ自分の局面を共有する fixture 同士は、相手の副露 facts だけが違う。手牌・ツモ・
// 合法 Dahai が同一なので、通常打牌の候補比較も offense も一致する。
fn assert_the_group_shares_the_normal_discard_and_offense(group: &[&Evaluated]) {
    let baseline = group[0];
    let baseline_offense = baseline.push_pull_inputs().offense;

    for evaluated in group.iter().skip(1) {
        let name = evaluated.name();

        assert_eq!(
            hand_tiles(&evaluated.scenario),
            hand_tiles(&baseline.scenario),
            "{name}: 手牌を共有する"
        );
        assert_eq!(
            evaluated.scenario.legal_actions, baseline.scenario.legal_actions,
            "{name}: 合法手を共有する"
        );
        assert_eq!(
            evaluated.diagnostic.normal_discard, baseline.diagnostic.normal_discard,
            "{name}: 通常打牌の候補比較が一致する"
        );
        assert_eq!(
            evaluated.diagnostic.normal_discard_action, baseline.diagnostic.normal_discard_action,
            "{name}: 通常打牌 selected が一致する"
        );
        // 最終 action は押し引きが同じ fixture 同士でだけ一致する。High の副露相手がいて
        // Fold になる fixture は、通常打牌より OpenHand 防御 fallback を優先する。
        if expected_push_pull(&evaluated.entry) == expected_push_pull(&baseline.entry) {
            assert_eq!(
                evaluated.diagnostic.selected_action, baseline.diagnostic.selected_action,
                "{name}: 最終 action が一致する"
            );
        }
        assert_eq!(
            evaluated.push_pull_inputs().offense,
            baseline_offense,
            "{name}: offense が一致する"
        );
    }
}

// 副露牌は visible tiles に加わるため、受け入れ枚数が変わり得る。この corpus は
// 「相手の副露牌が自分の受け入れ牌種と重ならない」局面に揃えてあり、そのおかげで
// acceptance が一致する。visible tiles を無視して offense が同じだと仮定しない。
fn assert_open_melds_add_visible_tiles_outside_the_acceptance(group: &[&Evaluated]) {
    let baseline = group[0];
    let acceptance = acceptance_tile_types(&baseline.diagnostic);

    for evaluated in group.iter().skip(1) {
        let name = evaluated.name();
        let added = added_visible_tile_types(&baseline.scenario, &evaluated.scenario);

        assert!(!added.is_empty(), "{name}: 副露牌が見え牌に加わる");
        for tile in &added {
            assert!(
                !acceptance.contains(tile),
                "{name}: 増えた見え牌 {} が受け入れ牌種に含まれる",
                tile.to_mjai_string()
            );
        }
        assert!(
            evaluated.scenario.context.visible_tiles().len()
                > baseline.scenario.context.visible_tiles().len(),
            "{name}: 見え牌は baseline より増える"
        );
    }
}

// 同じ役牌 Pon を子 (player 3) と親 (player 1) が持つ対照ケース。牌の内訳が同じなので
// 見え牌も一致し、facts の差は副露を持つ席と親かどうかだけになる。
fn assert_the_dealer_pair_differs_only_in_the_dealer_seat(child: &Evaluated, dealer: &Evaluated) {
    assert_eq!(
        visible_tile_counts(&child.scenario),
        visible_tile_counts(&dealer.scenario)
    );

    let child_facts = child.diagnostic.player_threats[3].facts;
    let dealer_facts = dealer.diagnostic.player_threats[DEALER_PLAYER].facts;

    assert_eq!(child_facts.is_dealer, Some(false));
    assert_eq!(dealer_facts.is_dealer, Some(true));
    assert_eq!(child_facts.open_meld_count, dealer_facts.open_meld_count);
    assert_eq!(
        child_facts.value_honor_melds,
        dealer_facts.value_honor_melds
    );
    // 親の非リーチ副露でも押し引きは変わらない。
    assert!(!dealer.push_pull_inputs().dealer_reacher);
}

// 押し引きの分岐ごとに1つずつ自分の局面を持つ。一向聴の2つは受け入れの広さが違うが、
// 現在の policy ではどちらも Fold になる。
fn assert_the_self_hands_cover_the_push_pull_branches(corpus: &EvaluatedCorpus) {
    let tenpai = corpus.group(SelfHand::Tenpai)[0].offense();
    let strong = corpus.group(SelfHand::StrongIishanten)[0].offense();
    let weak = corpus.group(SelfHand::WeakIishanten)[0].offense();
    let two_shanten = corpus.group(SelfHand::TwoShanten)[0].offense();

    assert_eq!(tenpai.min_shanten_after_discard, 0);
    assert_eq!(strong.min_shanten_after_discard, 1);
    assert_eq!(weak.min_shanten_after_discard, 1);
    assert_eq!(two_shanten.min_shanten_after_discard, 2);

    // テンパイの fixture は非フリテンで待ちが広く、強いテンパイになる。
    let wait = tenpai
        .tenpai_wait_after_discard
        .expect("テンパイの待ち facts がある");
    assert_eq!(wait.permanent_furiten, bot_logic::PermanentFuriten::No);
    assert!(wait.tsumo_remaining >= 6);
    // 履歴依存フリテンを指定していない fixture なので、総合ロン可否は unknown のままになる。
    // 押し引きの強いテンパイ判定は恒常フリテンを見るため、この unknown では変わらない。
    assert_eq!(wait.can_ron, None);

    assert!(strong.acceptance_total_remaining >= 8);
    assert!(strong.acceptance_type_count >= 2);
    assert_eq!(weak.acceptance_total_remaining, 7);
    assert_eq!(weak.acceptance_type_count, 2);
    // 一向聴では待ち facts を持たない。
    assert_eq!(strong.tenpai_wait_after_discard, None);
    assert_eq!(weak.tenpai_wait_after_discard, None);

    assert_ne!(
        tenpai.acceptance_total_remaining,
        two_shanten.acceptance_total_remaining
    );
    assert_ne!(
        tenpai.acceptance_type_count,
        two_shanten.acceptance_type_count
    );
}

// 診断を無効にした production 経路の選択と、同じ局面の診断の選択が一致する。
fn assert_the_selected_action_matches_act(evaluated: &Evaluated) {
    let mut agent = ShantenAgent;
    let acted = agent.act(
        &evaluated.scenario.context,
        &evaluated.scenario.legal_actions,
    );

    assert_eq!(
        evaluated.diagnostic.selected_action,
        acted,
        "{}",
        evaluated.name()
    );
}

#[test]
fn the_corpus_fixes_the_open_hand_threat_facts_and_decisions() {
    // fixture ごとの評価は resolve 1回 + diagnose 1回だけで、意味ごとの assertion は同じ
    // Scenario / 診断を共有する。
    let corpus = EvaluatedCorpus::evaluate();

    for evaluated in corpus.iter() {
        assert_no_opponent_reach(evaluated);
        assert_the_expected_push_pull(evaluated);
        assert_the_expected_player_threat_facts(evaluated);
        assert_the_expected_open_hand_threat(evaluated);
        assert_the_threat_facts_are_shared_with_push_pull(evaluated);
        assert_the_high_threats_are_the_defense_targets(evaluated);
        assert_the_selected_action_matches_act(evaluated);

        let targets = expected_targets(&evaluated.entry);
        if targets.is_empty() {
            assert_no_defense_target(evaluated);
        } else {
            assert_the_defense_safety_of_every_legal_dahai(evaluated, &targets);
            assert_no_other_players_river_is_river_safe(evaluated);
        }
    }

    // High になる fixture だけが threat の対象。強いテンパイなら押し、一向聴以下なら降りる。
    let high_scenarios = corpus.select(has_high_threat);
    assert!(!high_scenarios.is_empty());
    for evaluated in &high_scenarios {
        assert_the_expected_push_pull(evaluated);
    }

    let present_scenarios = corpus.select(|entry| {
        entry
            .melded
            .is_some_and(|melded| melded.threat.level == OpenHandThreatLevel::Present)
    });
    assert!(!present_scenarios.is_empty());
    for evaluated in &present_scenarios {
        assert_the_present_threat_keeps_the_push_pull(evaluated);
    }

    let folding_scenarios =
        corpus.select(|entry| expected_push_pull(entry).0 == PushPullMode::Fold);
    assert!(!folding_scenarios.is_empty());
    for evaluated in &folding_scenarios {
        assert_the_folding_scenario_selects_the_defense_fallback(evaluated);
    }

    let pushing_scenarios = corpus.select(|entry| {
        has_high_threat(entry) && expected_push_pull(entry).0 != PushPullMode::Fold
    });
    assert!(!pushing_scenarios.is_empty());
    for evaluated in &pushing_scenarios {
        assert_the_pushing_scenario_keeps_the_normal_discard(evaluated);
    }

    for self_hand in SELF_HANDS {
        let group = corpus.group(self_hand);
        assert_the_group_shares_the_normal_discard_and_offense(&group);
        assert_open_melds_add_visible_tiles_outside_the_acceptance(&group);
    }

    assert_the_representative_high_scenarios_fix_the_defense_target(&corpus);
    assert_the_nine_discard_scenario_shares_the_existing_wall_rank(
        corpus.find("open_hand_two_melds_nine_discards"),
    );
    assert_the_ankan_scenario_is_a_fixed_meld_but_not_an_open_meld(corpus.find("open_hand_ankan"));
    assert_the_dealer_pair_differs_only_in_the_dealer_seat(
        corpus.find("open_hand_value_pon"),
        corpus.find("open_hand_dealer_value_pon"),
    );
    assert_the_self_hands_cover_the_push_pull_branches(&corpus);
}

// 2手先診断は「打牌候補 × 受け入れ牌 × 次打牌候補」の重い探索なので、自分の局面ごとに
// 副露なしと3副露の代表 fixture だけで一致を確認する。
const LOOKAHEAD_SCENARIOS: [&str; 6] = [
    "open_hand_baseline",
    "open_hand_three_melds_value_dora",
    "open_hand_iishanten_three_melds",
    "open_hand_weak_iishanten_three_melds",
    "open_hand_weak_baseline",
    "open_hand_weak_three_melds_value_dora",
];

#[test]
fn the_lookahead_scenarios_select_the_same_action_in_act_and_diagnose() {
    for name in LOOKAHEAD_SCENARIOS {
        let entry = corpus()
            .into_iter()
            .find(|entry| entry.name == name)
            .unwrap_or_else(|| panic!("{name} scenario"));
        let scenario = resolve(&entry);
        let mut agent = ShantenAgent;

        let acted = agent.act(&scenario.context, &scenario.legal_actions);
        let with_lookahead = ShantenAgent::diagnose_with_options(
            &scenario.context,
            &scenario.legal_actions,
            DiagnosticOptions::WITH_LOOKAHEAD,
        );

        assert_eq!(with_lookahead.selected_action, acted, "{name}");
    }
}
