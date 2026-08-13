//! 非リーチ副露相手の観測事実を段階的に比較するための scenario corpus と、その回帰テスト。
//!
//! 各 fixture は同じ自分の攻撃状態に対して相手の副露だけを変えたもので、現行の
//! `PlayerThreatFacts` / `OpenHandThreat` / 押し引き / 通常打牌 selected を並べて比較するための
//! 固定局面。corpus 側で threat score や副露評価を再実装せず、production が構築した facts と
//! classification をそのまま確認する。`decide_push_pull()` はまだ副露 facts も OpenHandThreat も
//! 使わないため、`High` になる fixture を含めて全 fixture が `NoOpponentReach` → `Push` になる
//! ことを固定する。

use bot_core::{
    Agent, DiagnosticOptions, MeldKindCounts, OpenHandThreatAssessment, OpenHandThreatDecision,
    OpenHandThreatExclusion, OpenHandThreatLevel, OpenHandThreatReason, PlayerThreatFacts,
    PushPullMode, PushPullReason, ShantenAgent, ShantenDecisionDiagnostic, ValueHonorMeldCounts,
    classify_open_hand_threat,
};
use bot_logic::{TileId, TileType};

use crate::scenario::{Scenario, ScenarioSpec};

const BASELINE: &str = include_str!("../scenarios/open_hand_baseline.json");
const CHI: &str = include_str!("../scenarios/open_hand_chi.json");
const VALUE_PON: &str = include_str!("../scenarios/open_hand_value_pon.json");
const TWO_MELDS: &str = include_str!("../scenarios/open_hand_two_melds.json");
const VALUE_PON_AND_CHI: &str = include_str!("../scenarios/open_hand_value_pon_and_chi.json");
const DORA_MELDS: &str = include_str!("../scenarios/open_hand_dora_melds.json");
const THREE_MELDS: &str = include_str!("../scenarios/open_hand_three_melds.json");
const THREE_MELDS_VALUE_DORA: &str =
    include_str!("../scenarios/open_hand_three_melds_value_dora.json");
const DEALER_VALUE_PON: &str = include_str!("../scenarios/open_hand_dealer_value_pon.json");
const ANKAN: &str = include_str!("../scenarios/open_hand_ankan.json");
const WEAK_BASELINE: &str = include_str!("../scenarios/open_hand_weak_baseline.json");
const WEAK_VALUE_PON: &str = include_str!("../scenarios/open_hand_weak_value_pon.json");
const WEAK_THREE_MELDS_VALUE_DORA: &str =
    include_str!("../scenarios/open_hand_weak_three_melds_value_dora.json");

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
    /// 打 N でテンパイになる局面。
    Tenpai,
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
                threat: high(OpenHandThreatReason::TwoOrMoreWithValueHonor),
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
                threat: high(OpenHandThreatReason::TwoOrMoreWithDora),
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

fn group(self_hand: SelfHand) -> Vec<CorpusScenario> {
    corpus()
        .into_iter()
        .filter(|entry| entry.self_hand == self_hand)
        .collect()
}

#[test]
fn every_scenario_resolves_and_keeps_no_opponent_reach_push() {
    for entry in corpus() {
        let scenario = resolve(&entry);
        let diagnostic = diagnose(&scenario);
        let inputs = diagnostic
            .push_pull_inputs
            .unwrap_or_else(|| panic!("{}: 押し引き入力がある", entry.name));
        let decision = diagnostic
            .push_pull_decision
            .unwrap_or_else(|| panic!("{}: 押し引き判断がある", entry.name));

        assert_eq!(inputs.opponent_reach_count, 0, "{}", entry.name);
        assert!(!inputs.dealer_reacher, "{}", entry.name);
        assert!(!inputs.self_dealer, "{}", entry.name);
        assert_eq!(decision.mode, PushPullMode::Push, "{}", entry.name);
        assert_eq!(
            decision.reason,
            PushPullReason::NoOpponentReach,
            "{}",
            entry.name
        );
    }
}

#[test]
fn every_scenario_matches_the_expected_player_threat_facts() {
    for entry in corpus() {
        let scenario = resolve(&entry);
        let diagnostic = diagnose(&scenario);

        for player in 0..4 {
            let facts = diagnostic.player_threats[player].facts;
            assert!(!facts.reached, "{} player {player}", entry.name);
            assert_eq!(
                facts.is_self,
                Some(player == SELF_PLAYER),
                "{} player {player}",
                entry.name
            );
            assert_eq!(
                facts.is_dealer,
                Some(player == DEALER_PLAYER),
                "{} player {player}",
                entry.name
            );

            assert_open_hand_facts(entry.name, expected_of(&entry, player), facts);
            assert_eq!(
                facts.discard_count,
                scenario.context.discards_of(player).unwrap().len(),
                "{} player {player} discard_count",
                entry.name
            );
        }
    }
}

#[test]
fn every_scenario_matches_the_expected_open_hand_threat() {
    // 副露 facts から production が導く暫定 classification を fixture ごとに固定する。
    // 自分の席は対象外で、リーチ者はこの corpus にいない。
    for entry in corpus() {
        let scenario = resolve(&entry);
        let diagnostic = diagnose(&scenario);

        for player in 0..4 {
            let threat = &diagnostic.player_threats[player];
            let expected = if player == SELF_PLAYER {
                OpenHandThreatAssessment::NotApplicable(OpenHandThreatExclusion::SelfSeat)
            } else {
                OpenHandThreatAssessment::Classified(expected_of(&entry, player).threat)
            };

            assert_eq!(
                threat.open_hand_threat, expected,
                "{} player {player}",
                entry.name
            );
            // 表示・診断は分類し直さず、同じ facts から求めた結果を共有する。
            assert_eq!(
                threat.open_hand_threat,
                classify_open_hand_threat(threat.facts),
                "{} player {player}",
                entry.name
            );
        }
    }
}

#[test]
fn a_high_open_hand_threat_does_not_change_the_push_pull_decision() {
    // High になる fixture でも、非リーチ相手だけの局面の押し引きは NoOpponentReach → Push。
    let high: Vec<CorpusScenario> = corpus()
        .into_iter()
        .filter(|entry| {
            entry
                .melded
                .is_some_and(|melded| melded.threat.level == OpenHandThreatLevel::High)
        })
        .collect();
    assert!(!high.is_empty());

    for entry in high {
        let scenario = resolve(&entry);
        let diagnostic = diagnose(&scenario);
        let decision = diagnostic
            .push_pull_decision
            .unwrap_or_else(|| panic!("{}: 押し引き判断がある", entry.name));

        assert_eq!(decision.mode, PushPullMode::Push, "{}", entry.name);
        assert_eq!(
            decision.reason,
            PushPullReason::NoOpponentReach,
            "{}",
            entry.name
        );
    }
}

#[test]
fn every_scenario_shares_the_threat_facts_with_push_pull() {
    // 表示・押し引き・診断が同じ軽量 facts を共有していることを corpus 全体で固定する。
    for entry in corpus() {
        let scenario = resolve(&entry);
        let diagnostic = diagnose(&scenario);
        let inputs = diagnostic
            .push_pull_inputs
            .unwrap_or_else(|| panic!("{}: 押し引き入力がある", entry.name));

        for player in 0..4 {
            assert_eq!(
                inputs.player_threats[player], diagnostic.player_threats[player].facts,
                "{} player {player}",
                entry.name
            );
        }
    }
}

#[test]
fn the_ankan_scenario_is_a_fixed_meld_but_not_an_open_meld() {
    let entry = corpus()
        .into_iter()
        .find(|entry| entry.name == "open_hand_ankan")
        .expect("ankan scenario");
    let facts = diagnose(&resolve(&entry)).player_threats[3].facts;

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

#[test]
fn scenarios_with_the_same_self_hand_share_the_normal_discard_and_offense() {
    // 同じ自分の局面を共有する fixture 同士は、相手の副露 facts だけが違う。手牌・ツモ・
    // 合法 Dahai が同一なので、通常打牌の候補比較も offense も一致する。
    for self_hand in [SelfHand::Tenpai, SelfHand::TwoShanten] {
        let entries = group(self_hand);
        let baseline_entry = entries[0];
        let baseline = resolve(&baseline_entry);
        let baseline_diagnostic = diagnose(&baseline);
        let baseline_offense = baseline_diagnostic
            .push_pull_inputs
            .expect("押し引き入力がある")
            .offense;

        for entry in entries.iter().skip(1) {
            let scenario = resolve(entry);
            let diagnostic = diagnose(&scenario);

            assert_eq!(
                hand_tiles(&scenario),
                hand_tiles(&baseline),
                "{}: 手牌を共有する",
                entry.name
            );
            assert_eq!(
                scenario.legal_actions, baseline.legal_actions,
                "{}: 合法手を共有する",
                entry.name
            );
            assert_eq!(
                diagnostic.normal_discard, baseline_diagnostic.normal_discard,
                "{}: 通常打牌の候補比較が一致する",
                entry.name
            );
            assert_eq!(
                diagnostic.normal_discard_action, baseline_diagnostic.normal_discard_action,
                "{}: 通常打牌 selected が一致する",
                entry.name
            );
            assert_eq!(
                diagnostic.selected_action, baseline_diagnostic.selected_action,
                "{}: 最終 action が一致する",
                entry.name
            );
            assert_eq!(
                diagnostic
                    .push_pull_inputs
                    .expect("押し引き入力がある")
                    .offense,
                baseline_offense,
                "{}: offense が一致する",
                entry.name
            );
        }
    }
}

#[test]
fn open_melds_add_visible_tiles_outside_the_acceptance() {
    // 副露牌は visible tiles に加わるため、受け入れ枚数が変わり得る。この corpus は
    // 「相手の副露牌が自分の受け入れ牌種と重ならない」局面に揃えてあり、そのおかげで
    // acceptance が一致する。visible tiles を無視して offense が同じだと仮定しない。
    for self_hand in [SelfHand::Tenpai, SelfHand::TwoShanten] {
        let entries = group(self_hand);
        let baseline = resolve(&entries[0]);
        let acceptance = acceptance_tile_types(&diagnose(&baseline));

        for entry in entries.iter().skip(1) {
            let scenario = resolve(entry);
            let added = added_visible_tile_types(&baseline, &scenario);

            assert!(!added.is_empty(), "{}: 副露牌が見え牌に加わる", entry.name);
            for tile in &added {
                assert!(
                    !acceptance.contains(tile),
                    "{}: 増えた見え牌 {} が受け入れ牌種に含まれる",
                    entry.name,
                    tile.to_mjai_string()
                );
            }
            assert!(
                scenario.context.visible_tiles().len() > baseline.context.visible_tiles().len(),
                "{}: 見え牌は baseline より増える",
                entry.name
            );
        }
    }
}

#[test]
fn the_dealer_pair_differs_only_in_the_dealer_seat() {
    // 同じ役牌 Pon を子 (player 3) と親 (player 1) が持つ対照ケース。牌の内訳が同じなので
    // 見え牌も一致し、facts の差は副露を持つ席と親かどうかだけになる。
    let child = resolve(
        &corpus()
            .into_iter()
            .find(|entry| entry.name == "open_hand_value_pon")
            .expect("value pon scenario"),
    );
    let dealer = resolve(
        &corpus()
            .into_iter()
            .find(|entry| entry.name == "open_hand_dealer_value_pon")
            .expect("dealer value pon scenario"),
    );

    assert_eq!(visible_tile_counts(&child), visible_tile_counts(&dealer));

    let child_facts = diagnose(&child).player_threats[3].facts;
    let dealer_facts = diagnose(&dealer).player_threats[DEALER_PLAYER].facts;

    assert_eq!(child_facts.is_dealer, Some(false));
    assert_eq!(dealer_facts.is_dealer, Some(true));
    assert_eq!(child_facts.open_meld_count, dealer_facts.open_meld_count);
    assert_eq!(
        child_facts.value_honor_melds,
        dealer_facts.value_honor_melds
    );
    // 親の非リーチ副露でも押し引きは変わらない。
    assert!(!diagnose(&dealer).push_pull_inputs.unwrap().dealer_reacher);
}

#[test]
fn the_two_self_hands_have_different_offense_states() {
    let tenpai = diagnose(&resolve(&group(SelfHand::Tenpai)[0]))
        .push_pull_inputs
        .expect("押し引き入力がある")
        .offense
        .expect("offense がある");
    let two_shanten = diagnose(&resolve(&group(SelfHand::TwoShanten)[0]))
        .push_pull_inputs
        .expect("押し引き入力がある")
        .offense
        .expect("offense がある");

    assert_eq!(tenpai.min_shanten_after_discard, 0);
    assert_eq!(two_shanten.min_shanten_after_discard, 2);
    assert_ne!(
        tenpai.acceptance_total_remaining,
        two_shanten.acceptance_total_remaining
    );
    assert_ne!(
        tenpai.acceptance_type_count,
        two_shanten.acceptance_type_count
    );
}

// 2手先診断は「打牌候補 × 受け入れ牌 × 次打牌候補」の重い探索なので、自分の局面ごとに
// 副露なしと3副露の代表 fixture だけで一致を確認する。
const LOOKAHEAD_SCENARIOS: [&str; 4] = [
    "open_hand_baseline",
    "open_hand_three_melds_value_dora",
    "open_hand_weak_baseline",
    "open_hand_weak_three_melds_value_dora",
];

#[test]
fn every_scenario_selects_the_same_action_in_act_and_diagnose() {
    for entry in corpus() {
        let scenario = resolve(&entry);
        let mut agent = ShantenAgent;

        let acted = agent.act(&scenario.context, &scenario.legal_actions);
        let diagnostic = diagnose(&scenario);
        assert_eq!(diagnostic.selected_action, acted, "{}", entry.name);

        if !LOOKAHEAD_SCENARIOS.contains(&entry.name) {
            continue;
        }
        let with_lookahead = ShantenAgent::diagnose_with_options(
            &scenario.context,
            &scenario.legal_actions,
            DiagnosticOptions::WITH_LOOKAHEAD,
        );
        assert_eq!(with_lookahead.selected_action, acted, "{}", entry.name);
    }
}
