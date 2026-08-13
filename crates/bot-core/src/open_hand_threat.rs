//! 非リーチ副露相手の暫定 threat classification。
//!
//! 観測 facts ([`PlayerThreatFacts`]) だけを入力にした pure な判定で、`GameContext` を
//! 解析し直さない。押し引き・防御の policy はここには持たない。

use crate::threat::PlayerThreatFacts;

/// 非リーチ副露相手の暫定的な危険度。
///
/// 正確なテンパイ確率・放銃率・推定打点ではなく、観測できた副露・ドラ・役牌・局進行だけから
/// 決める暫定 heuristic。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpenHandThreatLevel {
    /// open meld が無い。Ankan だけの場合もここ。
    None,
    /// open meld はあるが、`High` の条件は満たさない。
    Present,
    /// 暫定 heuristic の警戒条件を満たす。
    High,
}

/// [`OpenHandThreatLevel`] をその値にした条件。
///
/// 複数の `High` 条件を同時に満たす場合は、[`classify_open_hand_threat`] が固定の優先順位で
/// 1つだけ選ぶ。level 自体はどの条件を満たしても `High` なので、この順位は診断表示のためだけの
/// ものになる。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpenHandThreatReason {
    /// open meld が無い。
    NoOpenMeld,
    /// open meld はあるが `High` 条件をどれも満たさない。
    OpenMeldPresent,
    /// 3副露以上。
    ThreeOrMoreOpenMelds,
    /// 2副露以上かつ確定役牌の副露がある。
    TwoOrMoreWithValueHonor,
    /// 2副露以上かつ open meld 内のドラが2枚以上。
    TwoOrMoreWithDora,
    /// 親が2副露以上。
    DealerWithTwoOrMoreOpenMelds,
    /// 2副露以上かつ河が9枚以上。
    TwoOrMoreOpenMeldsFromNineDiscards,
    /// 1副露以上かつ河が12枚以上。
    OpenMeldFromTwelveDiscards,
}

/// 非リーチ他家として分類した結果。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OpenHandThreatDecision {
    pub level: OpenHandThreatLevel,
    pub reason: OpenHandThreatReason,
}

/// `OpenHandThreat` の対象外である理由。
///
/// 対象外は level を持たない。特に `UnknownSeat` を「危険度なし」と確定させないため、
/// [`OpenHandThreatLevel::None`] とは別の状態にする。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpenHandThreatExclusion {
    /// 自分の席。
    SelfSeat,
    /// リーチ済み。リーチ者の threat は既存のリーチ情報が source of truth。
    Reached,
    /// `player_id` 不明で自分の席かどうかを確定できない。他家と推測しない。
    UnknownSeat,
}

/// [`classify_open_hand_threat`] の結果。
///
/// 対象外の席を `Classified(None)` に潰さず、明示的に [`Self::NotApplicable`] で表す。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpenHandThreatAssessment {
    Classified(OpenHandThreatDecision),
    NotApplicable(OpenHandThreatExclusion),
}

impl OpenHandThreatAssessment {
    /// 分類できた場合の判定。対象外の席では `None`。
    pub fn decision(self) -> Option<OpenHandThreatDecision> {
        match self {
            Self::Classified(decision) => Some(decision),
            Self::NotApplicable(_) => None,
        }
    }

    /// 分類できた場合の level。対象外の席では `None` (unknown) で、
    /// [`OpenHandThreatLevel::None`] と区別する。
    pub fn level(self) -> Option<OpenHandThreatLevel> {
        self.decision().map(|decision| decision.level)
    }

    /// 分類できた場合の reason。対象外の席では `None`。
    pub fn reason(self) -> Option<OpenHandThreatReason> {
        self.decision().map(|decision| decision.reason)
    }

    /// 対象外の場合のその理由。分類できた席では `None`。
    pub fn exclusion(self) -> Option<OpenHandThreatExclusion> {
        match self {
            Self::Classified(_) => None,
            Self::NotApplicable(exclusion) => Some(exclusion),
        }
    }
}

// 局進行・打点に関係なく High とする open meld 数。
const THREE_OPEN_MELDS: usize = 3;
// 役牌・ドラ・親・中盤後半の各条件で High とする open meld 数。
const TWO_OPEN_MELDS: usize = 2;
// Present とみなす最小の open meld 数。局進行条件でも同じ最小値を使う。
const ONE_OPEN_MELD: usize = 1;
// 2副露以上と組み合わせて High とする確定役牌の副露数。
const HIGH_VALUE_HONOR_MELDS: usize = 1;
// 2副露以上と組み合わせて High とする open meld 内のドラ枚数。
const HIGH_OPEN_MELD_DORA_COUNT: u8 = 2;
// 2副露以上を強く警戒し始める河の枚数。
const MID_ROUND_DISCARD_COUNT: usize = 9;
// 1副露でも強く警戒し始める河の枚数。
const LATE_ROUND_DISCARD_COUNT: usize = 12;

/// 観測 facts から非リーチ副露相手の暫定 threat を分類する pure helper。
///
/// これは暫定 heuristic であり、正確なテンパイ確率・放銃率・推定打点を表さない。以下のいずれかを
/// 満たすと [`OpenHandThreatLevel::High`] になる。
///
/// - `open_meld_count >= 3`
/// - `open_meld_count >= 2` かつ `open_value_honor_melds.confirmed >= 1`
/// - `open_meld_count >= 2` かつ `open_meld_dora_count >= 2`
/// - `is_dealer == Some(true)` かつ `open_meld_count >= 2`
/// - `open_meld_count >= 2` かつ `discard_count >= 9`
/// - `open_meld_count >= 1` かつ `discard_count >= 12`
///
/// どれも満たさず `open_meld_count >= 1` なら [`OpenHandThreatLevel::Present`]、
/// `open_meld_count == 0` なら [`OpenHandThreatLevel::None`]。Ankan は `open_meld_count` にも
/// open meld 限定のドラ・役牌 facts にも入らないため、暗槓だけの相手は `None` になる。
///
/// 自分の席・リーチ済みの席・`player_id` 不明の席は対象外
/// ([`OpenHandThreatAssessment::NotApplicable`]) で、危険度なしとは区別する。
pub fn classify_open_hand_threat(facts: PlayerThreatFacts) -> OpenHandThreatAssessment {
    if let Some(exclusion) = exclusion_of(facts) {
        return OpenHandThreatAssessment::NotApplicable(exclusion);
    }

    let decision = match high_reason(facts) {
        Some(reason) => OpenHandThreatDecision {
            level: OpenHandThreatLevel::High,
            reason,
        },
        None if facts.open_meld_count >= ONE_OPEN_MELD => OpenHandThreatDecision {
            level: OpenHandThreatLevel::Present,
            reason: OpenHandThreatReason::OpenMeldPresent,
        },
        None => OpenHandThreatDecision {
            level: OpenHandThreatLevel::None,
            reason: OpenHandThreatReason::NoOpenMeld,
        },
    };

    OpenHandThreatAssessment::Classified(decision)
}

/// 全4席分の facts をまとめて分類する helper。
pub fn classify_open_hand_threats(facts: &[PlayerThreatFacts; 4]) -> [OpenHandThreatAssessment; 4] {
    std::array::from_fn(|player| classify_open_hand_threat(facts[player]))
}

// 対象外の席とその理由。自分のリーチは自分の席として、席が不明なリーチ者はリーチ者として扱い、
// OpenHandThreat とリーチ由来の threat を二重適用しない。
fn exclusion_of(facts: PlayerThreatFacts) -> Option<OpenHandThreatExclusion> {
    if facts.is_self == Some(true) {
        return Some(OpenHandThreatExclusion::SelfSeat);
    }
    if facts.reached {
        return Some(OpenHandThreatExclusion::Reached);
    }
    if facts.is_self.is_none() {
        return Some(OpenHandThreatExclusion::UnknownSeat);
    }
    None
}

// 満たした High 条件のうち、優先順位が最も高いものの reason。
fn high_reason(facts: PlayerThreatFacts) -> Option<OpenHandThreatReason> {
    high_conditions(facts)
        .into_iter()
        .find_map(|(matched, reason)| matched.then_some(reason))
}

// High 条件と診断 reason の対応。並びは reason の優先順位で、どの条件も level は High なので、
// 並び順は level の判定を変えない。
fn high_conditions(facts: PlayerThreatFacts) -> [(bool, OpenHandThreatReason); 6] {
    let open_melds = facts.open_meld_count;
    [
        (
            open_melds >= THREE_OPEN_MELDS,
            OpenHandThreatReason::ThreeOrMoreOpenMelds,
        ),
        (
            open_melds >= TWO_OPEN_MELDS
                && facts.open_value_honor_melds.confirmed >= HIGH_VALUE_HONOR_MELDS,
            OpenHandThreatReason::TwoOrMoreWithValueHonor,
        ),
        (
            open_melds >= TWO_OPEN_MELDS && facts.open_meld_dora_count >= HIGH_OPEN_MELD_DORA_COUNT,
            OpenHandThreatReason::TwoOrMoreWithDora,
        ),
        (
            facts.is_dealer == Some(true) && open_melds >= TWO_OPEN_MELDS,
            OpenHandThreatReason::DealerWithTwoOrMoreOpenMelds,
        ),
        (
            open_melds >= TWO_OPEN_MELDS && facts.discard_count >= MID_ROUND_DISCARD_COUNT,
            OpenHandThreatReason::TwoOrMoreOpenMeldsFromNineDiscards,
        ),
        (
            open_melds >= ONE_OPEN_MELD && facts.discard_count >= LATE_ROUND_DISCARD_COUNT,
            OpenHandThreatReason::OpenMeldFromTwelveDiscards,
        ),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context::GameContext;
    use crate::meld::{Meld, MeldKind};
    use crate::threat::{MeldKindCounts, ValueHonorMeldCounts, player_threat_facts_from_context};
    use bot_logic::{TileId, TileType};

    const EAST: u8 = 27;
    const HAKU: u8 = 31;

    fn tile(value: u8) -> TileId {
        TileId::new(value).unwrap()
    }

    fn honor(value: u8) -> TileType {
        TileType::new(value).unwrap()
    }

    // 副露もリーチも無い他家の facts。ここから必要な観測事実だけを足して条件を作る。
    fn opponent_facts() -> PlayerThreatFacts {
        PlayerThreatFacts {
            player: 3,
            is_self: Some(false),
            is_dealer: Some(false),
            reached: false,
            seat_wind: None,
            discard_count: 0,
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
        }
    }

    // Chi だけを `count` 個持つ他家の facts。ドラも役牌も含まない。
    fn open_melds(count: usize) -> PlayerThreatFacts {
        PlayerThreatFacts {
            meld_count: count,
            open_meld_count: count,
            meld_kinds: MeldKindCounts {
                chi: count,
                ..MeldKindCounts::default()
            },
            ..opponent_facts()
        }
    }

    fn with_discards(facts: PlayerThreatFacts, discard_count: usize) -> PlayerThreatFacts {
        PlayerThreatFacts {
            discard_count,
            ..facts
        }
    }

    // 確定役牌の副露を1つ足す。fixed meld 全体と open meld 限定の両方に数える。
    fn with_value_honor(facts: PlayerThreatFacts) -> PlayerThreatFacts {
        let counts = ValueHonorMeldCounts {
            dragon: 1,
            confirmed: 1,
            ..ValueHonorMeldCounts::default()
        };
        PlayerThreatFacts {
            value_honor_melds: counts,
            open_value_honor_melds: counts,
            ..facts
        }
    }

    // open meld 内のドラを足す。
    fn with_open_dora(facts: PlayerThreatFacts, dora: u8) -> PlayerThreatFacts {
        PlayerThreatFacts {
            meld_dora_count: dora,
            open_meld_dora_count: dora,
            ..facts
        }
    }

    fn as_dealer(facts: PlayerThreatFacts) -> PlayerThreatFacts {
        PlayerThreatFacts {
            is_dealer: Some(true),
            ..facts
        }
    }

    fn classified(
        level: OpenHandThreatLevel,
        reason: OpenHandThreatReason,
    ) -> OpenHandThreatAssessment {
        OpenHandThreatAssessment::Classified(OpenHandThreatDecision { level, reason })
    }

    fn assert_classified(
        facts: PlayerThreatFacts,
        level: OpenHandThreatLevel,
        reason: OpenHandThreatReason,
    ) {
        assert_eq!(
            classify_open_hand_threat(facts),
            classified(level, reason),
            "{facts:?}"
        );
    }

    // ---- 基本 ----

    #[test]
    fn no_meld_is_none() {
        assert_classified(
            opponent_facts(),
            OpenHandThreatLevel::None,
            OpenHandThreatReason::NoOpenMeld,
        );
    }

    #[test]
    fn single_open_meld_before_the_late_round_is_present() {
        for discard_count in [0, 1, 8, 11] {
            assert_classified(
                with_discards(open_melds(1), discard_count),
                OpenHandThreatLevel::Present,
                OpenHandThreatReason::OpenMeldPresent,
            );
        }
    }

    #[test]
    fn single_value_honor_meld_is_present() {
        // 役牌副露1つだけでは High にしない。
        assert_classified(
            with_discards(with_value_honor(open_melds(1)), 11),
            OpenHandThreatLevel::Present,
            OpenHandThreatReason::OpenMeldPresent,
        );
    }

    #[test]
    fn single_meld_with_two_open_dora_is_present() {
        assert_classified(
            with_discards(with_open_dora(open_melds(1), 2), 11),
            OpenHandThreatLevel::Present,
            OpenHandThreatReason::OpenMeldPresent,
        );
    }

    #[test]
    fn two_plain_melds_of_a_child_are_present() {
        // 子・役牌なし・open dora 1以下・河8枚以下。
        assert_classified(
            with_discards(with_open_dora(open_melds(2), 1), 8),
            OpenHandThreatLevel::Present,
            OpenHandThreatReason::OpenMeldPresent,
        );
    }

    // ---- High 条件 ----

    #[test]
    fn three_open_melds_are_high() {
        for count in [3, 4] {
            assert_classified(
                open_melds(count),
                OpenHandThreatLevel::High,
                OpenHandThreatReason::ThreeOrMoreOpenMelds,
            );
        }
    }

    #[test]
    fn two_melds_with_a_confirmed_value_honor_are_high() {
        assert_classified(
            with_value_honor(open_melds(2)),
            OpenHandThreatLevel::High,
            OpenHandThreatReason::TwoOrMoreWithValueHonor,
        );
    }

    #[test]
    fn two_melds_with_an_unconfirmed_wind_are_present() {
        // 場風・自風が不明な風牌の副露は役牌と確定していないので High 条件を満たさない。
        let facts = open_melds(2);
        let counts = ValueHonorMeldCounts {
            unconfirmed_wind: 1,
            ..ValueHonorMeldCounts::default()
        };
        let facts = PlayerThreatFacts {
            value_honor_melds: counts,
            open_value_honor_melds: counts,
            ..facts
        };

        assert_classified(
            facts,
            OpenHandThreatLevel::Present,
            OpenHandThreatReason::OpenMeldPresent,
        );
    }

    #[test]
    fn two_melds_with_two_open_dora_are_high() {
        assert_classified(
            with_open_dora(open_melds(2), 2),
            OpenHandThreatLevel::High,
            OpenHandThreatReason::TwoOrMoreWithDora,
        );
    }

    #[test]
    fn two_melds_with_one_open_dora_are_present() {
        assert_classified(
            with_open_dora(open_melds(2), 1),
            OpenHandThreatLevel::Present,
            OpenHandThreatReason::OpenMeldPresent,
        );
    }

    #[test]
    fn a_dealer_with_two_melds_is_high() {
        assert_classified(
            as_dealer(open_melds(2)),
            OpenHandThreatLevel::High,
            OpenHandThreatReason::DealerWithTwoOrMoreOpenMelds,
        );
    }

    #[test]
    fn a_dealer_with_one_meld_is_present() {
        assert_classified(
            as_dealer(open_melds(1)),
            OpenHandThreatLevel::Present,
            OpenHandThreatReason::OpenMeldPresent,
        );
    }

    #[test]
    fn an_unknown_dealer_seat_is_not_treated_as_a_dealer() {
        let facts = PlayerThreatFacts {
            is_dealer: None,
            ..open_melds(2)
        };
        assert_classified(
            facts,
            OpenHandThreatLevel::Present,
            OpenHandThreatReason::OpenMeldPresent,
        );
    }

    // ---- 局進行 threshold ----

    #[test]
    fn two_melds_at_eight_discards_are_present() {
        assert_classified(
            with_discards(open_melds(2), 8),
            OpenHandThreatLevel::Present,
            OpenHandThreatReason::OpenMeldPresent,
        );
    }

    #[test]
    fn two_melds_at_nine_discards_are_high() {
        assert_classified(
            with_discards(open_melds(2), 9),
            OpenHandThreatLevel::High,
            OpenHandThreatReason::TwoOrMoreOpenMeldsFromNineDiscards,
        );
    }

    #[test]
    fn one_meld_at_eleven_discards_is_present() {
        assert_classified(
            with_discards(open_melds(1), 11),
            OpenHandThreatLevel::Present,
            OpenHandThreatReason::OpenMeldPresent,
        );
    }

    #[test]
    fn one_meld_at_twelve_discards_is_high() {
        assert_classified(
            with_discards(open_melds(1), 12),
            OpenHandThreatLevel::High,
            OpenHandThreatReason::OpenMeldFromTwelveDiscards,
        );
    }

    #[test]
    fn a_long_river_without_an_open_meld_is_none() {
        assert_classified(
            with_discards(open_melds(0), 18),
            OpenHandThreatLevel::None,
            OpenHandThreatReason::NoOpenMeld,
        );
    }

    // ---- reason の優先順位 ----

    #[test]
    fn two_melds_at_twelve_discards_report_the_nine_discard_reason() {
        assert_classified(
            with_discards(open_melds(2), 12),
            OpenHandThreatLevel::High,
            OpenHandThreatReason::TwoOrMoreOpenMeldsFromNineDiscards,
        );
    }

    #[test]
    fn the_highest_priority_reason_wins() {
        // すべての High 条件を同時に満たす facts から、条件を1つずつ外して優先順位を固定する。
        let all = with_discards(
            as_dealer(with_open_dora(with_value_honor(open_melds(3)), 2)),
            12,
        );
        let expected = [
            OpenHandThreatReason::ThreeOrMoreOpenMelds,
            OpenHandThreatReason::TwoOrMoreWithValueHonor,
            OpenHandThreatReason::TwoOrMoreWithDora,
            OpenHandThreatReason::DealerWithTwoOrMoreOpenMelds,
            OpenHandThreatReason::TwoOrMoreOpenMeldsFromNineDiscards,
            OpenHandThreatReason::OpenMeldFromTwelveDiscards,
        ];

        let mut facts = all;
        assert_classified(facts, OpenHandThreatLevel::High, expected[0]);

        facts = PlayerThreatFacts {
            meld_count: 2,
            open_meld_count: 2,
            ..facts
        };
        assert_classified(facts, OpenHandThreatLevel::High, expected[1]);

        facts = PlayerThreatFacts {
            value_honor_melds: ValueHonorMeldCounts::default(),
            open_value_honor_melds: ValueHonorMeldCounts::default(),
            ..facts
        };
        assert_classified(facts, OpenHandThreatLevel::High, expected[2]);

        facts = with_open_dora(facts, 0);
        assert_classified(facts, OpenHandThreatLevel::High, expected[3]);

        facts = PlayerThreatFacts {
            is_dealer: Some(false),
            ..facts
        };
        assert_classified(facts, OpenHandThreatLevel::High, expected[4]);

        facts = PlayerThreatFacts {
            meld_count: 1,
            open_meld_count: 1,
            ..facts
        };
        assert_classified(facts, OpenHandThreatLevel::High, expected[5]);
    }

    #[test]
    fn every_high_condition_alone_is_high() {
        // level は満たした条件の優先順位に依らず High になる。
        let conditions = [
            open_melds(3),
            with_value_honor(open_melds(2)),
            with_open_dora(open_melds(2), 2),
            as_dealer(open_melds(2)),
            with_discards(open_melds(2), 9),
            with_discards(open_melds(1), 12),
        ];

        for facts in conditions {
            assert_eq!(
                classify_open_hand_threat(facts).level(),
                Some(OpenHandThreatLevel::High),
                "{facts:?}"
            );
        }
    }

    // ---- 対象外 ----

    #[test]
    fn the_self_seat_is_not_applicable() {
        let facts = PlayerThreatFacts {
            is_self: Some(true),
            ..open_melds(3)
        };
        let assessment = classify_open_hand_threat(facts);

        assert_eq!(
            assessment,
            OpenHandThreatAssessment::NotApplicable(OpenHandThreatExclusion::SelfSeat)
        );
        assert_eq!(assessment.level(), None);
        assert_eq!(assessment.reason(), None);
    }

    #[test]
    fn an_unknown_seat_stays_unknown() {
        // player_id 不明の席を他家と推測して Present / High にしない。危険度なしにも確定させない。
        for facts in [open_melds(0), open_melds(1), open_melds(3)] {
            let facts = PlayerThreatFacts {
                is_self: None,
                ..facts
            };
            let assessment = classify_open_hand_threat(facts);

            assert_eq!(
                assessment,
                OpenHandThreatAssessment::NotApplicable(OpenHandThreatExclusion::UnknownSeat),
                "{facts:?}"
            );
            assert_eq!(assessment.level(), None, "{facts:?}");
        }
    }

    #[test]
    fn a_reached_player_is_not_applicable() {
        // リーチ者の threat は既存のリーチ情報が source of truth で、二重適用しない。
        let facts = PlayerThreatFacts {
            reached: true,
            ..open_melds(3)
        };
        let assessment = classify_open_hand_threat(facts);

        assert_eq!(
            assessment,
            OpenHandThreatAssessment::NotApplicable(OpenHandThreatExclusion::Reached)
        );
        assert_eq!(assessment.level(), None);
        assert_eq!(
            assessment.exclusion(),
            Some(OpenHandThreatExclusion::Reached)
        );
    }

    #[test]
    fn a_reached_seat_with_an_unknown_player_id_is_reported_as_reached() {
        // 席が不明なリーチ者も、既存のリーチ semantics と同じくリーチ者として対象外にする。
        let facts = PlayerThreatFacts {
            is_self: None,
            reached: true,
            ..open_melds(1)
        };

        assert_eq!(
            classify_open_hand_threat(facts),
            OpenHandThreatAssessment::NotApplicable(OpenHandThreatExclusion::Reached)
        );
    }

    #[test]
    fn the_self_seat_takes_priority_over_the_reach_exclusion() {
        let facts = PlayerThreatFacts {
            is_self: Some(true),
            reached: true,
            ..open_melds(1)
        };

        assert_eq!(
            classify_open_hand_threat(facts),
            OpenHandThreatAssessment::NotApplicable(OpenHandThreatExclusion::SelfSeat)
        );
    }

    #[test]
    fn classifying_every_seat_keeps_the_seat_order() {
        let context = context_with(vec![chi()], vec![], 0);
        let facts = player_threat_facts_from_context(&context);
        let expected: [OpenHandThreatAssessment; 4] =
            std::array::from_fn(|player| classify_open_hand_threat(facts[player]));

        assert_eq!(classify_open_hand_threats(&facts), expected);
    }

    // ---- Ankan (GameContext 経由) ----

    // 1m2m3m の Chi。ドラも役牌も含まない。
    fn chi() -> Meld {
        Meld::new(
            MeldKind::Chi,
            vec![tile(0), tile(4), tile(8)],
            Some(tile(0)),
        )
    }

    // 5m の暗槓。4m 表示なので赤5を含めてドラ5枚になる。
    fn dora_ankan() -> Meld {
        Meld::new(
            MeldKind::Ankan,
            vec![tile(16), tile(17), tile(18), tile(19)],
            None,
        )
    }

    // 白の暗槓。確定役牌。
    fn value_honor_ankan() -> Meld {
        Meld::new(
            MeldKind::Ankan,
            (0..4).map(|copy| tile(HAKU * 4 + copy)).collect(),
            None,
        )
    }

    // 河の牌種は数えないので、副露牌と重ならない物理牌を指定枚数だけ並べる。
    fn river(count: usize) -> Vec<TileId> {
        (0..count).map(|index| tile(60 + index as u8)).collect()
    }

    // 自分は player 0 で親も player 0。player 3 が melds と河を持つ子の他家になる。
    fn context_with(
        melds: Vec<Meld>,
        dora_indicators: Vec<TileId>,
        discard_count: usize,
    ) -> GameContext {
        context_with_reach(melds, dora_indicators, discard_count, false)
    }

    fn context_with_reach(
        melds: Vec<Meld>,
        dora_indicators: Vec<TileId>,
        discard_count: usize,
        reached: bool,
    ) -> GameContext {
        GameContext::from_parts_with_melds(
            None,
            vec![],
            dora_indicators,
            Some(honor(EAST)),
            None,
            Vec::new(),
            Some(0),
            Some(0),
            [vec![], vec![], vec![], river(discard_count)],
            [false, false, false, reached],
            [vec![], vec![], vec![], melds],
        )
    }

    fn assess(context: &GameContext, player: usize) -> OpenHandThreatAssessment {
        classify_open_hand_threat(player_threat_facts_from_context(context)[player])
    }

    #[test]
    fn an_ankan_is_not_an_open_meld() {
        let context = context_with(vec![value_honor_ankan()], vec![], 0);
        let facts = player_threat_facts_from_context(&context)[3];

        assert_eq!(facts.meld_count, 1);
        assert_eq!(facts.open_meld_count, 0);
        assert_eq!(
            assess(&context, 3),
            classified(OpenHandThreatLevel::None, OpenHandThreatReason::NoOpenMeld)
        );
    }

    #[test]
    fn an_ankan_with_dora_is_still_none() {
        let context = context_with(vec![dora_ankan()], vec![tile(12)], 0);
        let facts = player_threat_facts_from_context(&context)[3];

        assert!(facts.meld_dora_count >= 2);
        assert_eq!(facts.open_meld_dora_count, 0);
        assert_eq!(
            assess(&context, 3),
            classified(OpenHandThreatLevel::None, OpenHandThreatReason::NoOpenMeld)
        );
    }

    #[test]
    fn a_value_honor_ankan_is_still_none() {
        let context = context_with(vec![value_honor_ankan()], vec![], 0);
        let facts = player_threat_facts_from_context(&context)[3];

        assert_eq!(facts.value_honor_melds.confirmed, 1);
        assert_eq!(facts.open_value_honor_melds.confirmed, 0);
        assert_eq!(
            assess(&context, 3),
            classified(OpenHandThreatLevel::None, OpenHandThreatReason::NoOpenMeld)
        );
    }

    #[test]
    fn an_ankan_with_a_chi_is_one_open_meld_before_the_late_round() {
        let context = context_with(vec![dora_ankan(), chi()], vec![tile(12)], 11);
        let facts = player_threat_facts_from_context(&context)[3];

        assert_eq!(facts.meld_count, 2);
        assert_eq!(facts.open_meld_count, 1);
        assert_eq!(facts.discard_count, 11);
        // Ankan のドラを数えていれば「2副露 + ドラ2」で High になってしまう組み合わせ。
        assert!(facts.meld_dora_count >= 2);
        assert_eq!(facts.open_meld_dora_count, 0);
        assert_eq!(
            assess(&context, 3),
            classified(
                OpenHandThreatLevel::Present,
                OpenHandThreatReason::OpenMeldPresent
            )
        );
    }

    #[test]
    fn an_ankan_with_a_chi_at_twelve_discards_is_high_only_from_the_late_round() {
        let context = context_with(vec![value_honor_ankan(), chi()], vec![], 12);
        let facts = player_threat_facts_from_context(&context)[3];

        assert_eq!(facts.open_meld_count, 1);
        assert_eq!(facts.open_value_honor_melds.confirmed, 0);
        assert_eq!(
            assess(&context, 3),
            classified(
                OpenHandThreatLevel::High,
                OpenHandThreatReason::OpenMeldFromTwelveDiscards
            )
        );
    }

    #[test]
    fn the_open_meld_count_drives_the_classification_from_the_context() {
        // 同じ2副露でも、片方が Ankan なら open meld は1つ分しか数えない。
        let two_open = context_with(vec![chi(), chi()], vec![], 9);
        let one_open = context_with(vec![value_honor_ankan(), chi()], vec![], 9);

        assert_eq!(
            assess(&two_open, 3),
            classified(
                OpenHandThreatLevel::High,
                OpenHandThreatReason::TwoOrMoreOpenMeldsFromNineDiscards
            )
        );
        assert_eq!(
            assess(&one_open, 3),
            classified(
                OpenHandThreatLevel::Present,
                OpenHandThreatReason::OpenMeldPresent
            )
        );
    }

    #[test]
    fn the_self_seat_from_the_context_is_not_applicable() {
        let context = context_with(vec![chi(), chi(), chi()], vec![], 0);

        assert_eq!(
            assess(&context, 0),
            OpenHandThreatAssessment::NotApplicable(OpenHandThreatExclusion::SelfSeat)
        );
    }

    #[test]
    fn a_reached_player_from_the_context_is_not_applicable() {
        // 副露しているリーチ者でも OpenHandThreat の対象にしない。
        let context = context_with_reach(vec![chi(), chi(), chi()], vec![], 12, true);
        let facts = player_threat_facts_from_context(&context)[3];

        assert!(facts.reached);
        assert_eq!(facts.open_meld_count, 3);
        assert_eq!(
            assess(&context, 3),
            OpenHandThreatAssessment::NotApplicable(OpenHandThreatExclusion::Reached)
        );
    }
}
