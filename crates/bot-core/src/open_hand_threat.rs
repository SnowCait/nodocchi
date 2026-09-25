//! 非リーチ相手 (副露・暗槓を持つ席) の暫定 threat classification。
//!
//! 観測 facts ([`PlayerThreatFacts`]) だけを入力にした pure な判定で、`GameContext` を
//! 解析し直さない。押し引き・防御の policy はここには持たない。

use crate::threat::PlayerThreatFacts;

/// 非リーチ相手の暫定的な危険度。
///
/// 正確なテンパイ確率・放銃率・推定打点ではなく、観測できた副露・暗槓・ドラ・役牌・局進行だけ
/// から決める暫定 heuristic。公開副露が無くても暗槓だけで `Present` / `Caution` / `Danger` に
/// なり得る。
///
/// 現時点の production policy は `Caution` と `Danger` を区別せず、どちらも
/// [`OpenHandThreatAssessment::is_actionable`] として同じように扱う。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpenHandThreatLevel {
    /// fixed meld が無い。Ankan も完成面子なので、暗槓だけの相手はここには入らない。
    None,
    /// fixed meld はあるが、`Caution` / `Danger` の条件は満たさない。
    Present,
    /// 局進行だけを根拠にした警戒条件を満たし、`Danger` の条件は満たさない。
    Caution,
    /// 面子数・確定打点・親を根拠にした強い警戒条件を満たす。
    Danger,
}

/// [`OpenHandThreatLevel`] をその値にした条件。
///
/// 複数の警戒条件を同時に満たす場合は、[`classify_open_hand_threat`] が固定の優先順位で
/// 1つだけ選ぶ。`Danger` の条件は `Caution` の条件より常に優先されるので、選ばれた reason は
/// 常に level と一致する。
///
/// 各条件には `OpenMeld` 系と `FixedMeld` 系の2つの reason があり、同じ条件が公開副露だけで
/// 成立するなら `OpenMeld` 系、暗槓を含めて初めて成立するなら `FixedMeld` 系になる。暗槓込みで
/// 成立した条件を公開副露だけで成立したかのように表示しないための区別で、level は変わらない。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpenHandThreatReason {
    /// fixed meld が無い。
    NoOpenMeld,
    /// open meld はあるが警戒条件をどれも満たさない。
    OpenMeldPresent,
    /// 暗槓だけがあり警戒条件をどれも満たさない。
    FixedMeldPresent,
    /// `Danger`: 公開副露が3つ以上。
    ThreeOrMoreOpenMelds,
    /// `Danger`: 暗槓を含む完成面子が3つ以上。
    ThreeOrMoreFixedMelds,
    /// `Danger`: 公開副露が2つ以上かつ公開副露から確定する役牌翻・ドラ翻の proxy が2以上。
    TwoOrMoreWithVisibleHan,
    /// `Danger`: 暗槓を含む完成面子が2つ以上かつ、暗槓を含めて確定する役牌翻・ドラ翻の proxy が2以上。
    TwoOrMoreFixedMeldsWithVisibleHan,
    /// `Danger`: 親が公開副露を2つ以上。
    DealerWithTwoOrMoreOpenMelds,
    /// `Danger`: 親が暗槓を含む完成面子を2つ以上。
    DealerWithTwoOrMoreFixedMelds,
    /// `Caution`: 公開副露が2つ以上かつ河が9枚以上。
    TwoOrMoreOpenMeldsFromNineDiscards,
    /// `Caution`: 暗槓を含む完成面子が2つ以上かつ河が9枚以上。
    TwoOrMoreFixedMeldsFromNineDiscards,
    /// `Caution`: 公開副露が1つ以上かつ河が12枚以上。
    OpenMeldFromTwelveDiscards,
    /// `Caution`: 暗槓を含む完成面子が1つ以上かつ河が12枚以上。
    FixedMeldFromTwelveDiscards,
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

    /// production policy が反応する OpenHand threat か。[`OpenHandThreatLevel::Caution`] と
    /// [`OpenHandThreatLevel::Danger`] の両方を含む。
    ///
    /// level を持たない対象外の席は `false`。押し引きと防御はこの判定を共有し、警戒条件を
    /// それぞれで書き直さない。
    pub fn is_actionable(self) -> bool {
        matches!(
            self.level(),
            Some(OpenHandThreatLevel::Caution | OpenHandThreatLevel::Danger)
        )
    }
}

// 局進行・打点に関係なく Danger とする完成面子の数。
const THREE_MELDS: usize = 3;
// 役牌・ドラ・親の各条件で Danger、中盤の局進行条件で Caution とする完成面子の数。
const TWO_MELDS: usize = 2;
// Present とみなす最小の完成面子の数。局進行条件でも同じ最小値を使う。
const ONE_MELD: usize = 1;
// 2面子以上と組み合わせて Danger とする確定翻数 proxy。
const DANGER_VISIBLE_HAN_PROXY: usize = 2;
// 2面子以上を強く警戒し始める河の枚数。
const MID_ROUND_DISCARD_COUNT: usize = 9;
// 1面子でも強く警戒し始める河の枚数。
const LATE_ROUND_DISCARD_COUNT: usize = 12;

/// 観測 facts から非リーチ相手の暫定 threat を分類する pure helper。
///
/// これは暫定 heuristic であり、正確なテンパイ確率・放銃率・推定打点を表さない。暗槓も完成済みの
/// 面子なので、進行度の軸には公開副露と同じく1面子として数え、その中のドラ・赤ドラ・確定役牌も
/// 観測済みの打点として数える。以下のいずれかを満たすと [`OpenHandThreatLevel::Danger`] になる。
///
/// - `meld_count >= 3`
/// - `meld_count >= 2` かつ `fixed_meld_visible_han_proxy() >= 2`
/// - `is_dealer == Some(true)` かつ `meld_count >= 2`
///
/// `Danger` の条件を満たさず、以下のいずれかを満たすと [`OpenHandThreatLevel::Caution`] になる。
///
/// - `meld_count >= 2` かつ `discard_count >= 9`
/// - `meld_count >= 1` かつ `discard_count >= 12`
///
/// どれも満たさず `meld_count >= 1` なら [`OpenHandThreatLevel::Present`]、`meld_count == 0` なら
/// [`OpenHandThreatLevel::None`]。Daiminkan / Kakan は公開副露なので `meld_count` に1面子として
/// 入るだけで、暗槓として二重には数えない。unknown wind は推測して翻に加算しない。
///
/// 診断 reason は、同じ条件が公開副露だけで成立するなら `OpenMeld` 系、暗槓を含めて初めて成立
/// するなら `FixedMeld` 系になる。公開副露だけの相手の level と reason は従来のままになる。
///
/// 自分の席・リーチ済みの席・`player_id` 不明の席は対象外
/// ([`OpenHandThreatAssessment::NotApplicable`]) で、危険度なしとは区別する。
pub fn classify_open_hand_threat(facts: PlayerThreatFacts) -> OpenHandThreatAssessment {
    if let Some(exclusion) = exclusion_of(facts) {
        return OpenHandThreatAssessment::NotApplicable(exclusion);
    }

    let decision = match alert_decision(facts) {
        Some(decision) => decision,
        None if facts.meld_count >= ONE_MELD => OpenHandThreatDecision {
            level: OpenHandThreatLevel::Present,
            reason: present_reason(facts),
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

/// 分類済みの全4席から [`OpenHandThreatAssessment::is_actionable`] の席
/// (`Caution` / `Danger`) が1つ以上あるか判定する pure helper。
///
/// 分類し直さず、渡された classification をそのまま source of truth にする。
pub fn has_actionable_open_hand_threat(assessments: &[OpenHandThreatAssessment; 4]) -> bool {
    assessments
        .iter()
        .any(|assessment| assessment.is_actionable())
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

// 警戒条件の数。level の判定と reason の選択で同じ並びを共有する。
const ALERT_CONDITION_COUNT: usize = 5;

// 警戒条件ごとの level。Danger の条件を Caution の条件より前に並べ、先に成立した条件を採る
// だけで強い level が優先されるようにする。
const ALERT_LEVELS: [OpenHandThreatLevel; ALERT_CONDITION_COUNT] = [
    OpenHandThreatLevel::Danger,
    OpenHandThreatLevel::Danger,
    OpenHandThreatLevel::Danger,
    OpenHandThreatLevel::Caution,
    OpenHandThreatLevel::Caution,
];

// 警戒条件ごとの診断 reason。左が公開副露だけで成立した場合、右が暗槓を含めて成立した場合。
const ALERT_REASONS: [(OpenHandThreatReason, OpenHandThreatReason); ALERT_CONDITION_COUNT] = [
    (
        OpenHandThreatReason::ThreeOrMoreOpenMelds,
        OpenHandThreatReason::ThreeOrMoreFixedMelds,
    ),
    (
        OpenHandThreatReason::TwoOrMoreWithVisibleHan,
        OpenHandThreatReason::TwoOrMoreFixedMeldsWithVisibleHan,
    ),
    (
        OpenHandThreatReason::DealerWithTwoOrMoreOpenMelds,
        OpenHandThreatReason::DealerWithTwoOrMoreFixedMelds,
    ),
    (
        OpenHandThreatReason::TwoOrMoreOpenMeldsFromNineDiscards,
        OpenHandThreatReason::TwoOrMoreFixedMeldsFromNineDiscards,
    ),
    (
        OpenHandThreatReason::OpenMeldFromTwelveDiscards,
        OpenHandThreatReason::FixedMeldFromTwelveDiscards,
    ),
];

// 警戒条件が見る完成面子の進行度と、そこから確認できる打点 proxy の組。
//
// 同じ条件を「暗槓を含む全 fixed meld」と「公開副露だけ」の2つの軸で評価するための型で、条件の
// 閾値そのものは軸によらず共通になる。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct MeldProgress {
    melds: usize,
    visible_han: usize,
}

impl MeldProgress {
    // 暗槓を含む全 fixed meld の軸。level の判定はこちらを使う。
    fn fixed(facts: PlayerThreatFacts) -> Self {
        Self {
            melds: facts.meld_count,
            visible_han: facts.fixed_meld_visible_han_proxy(),
        }
    }

    // 公開副露だけの軸。同じ条件が公開情報だけで成立するかを見て reason を選ぶために使う。
    fn open(facts: PlayerThreatFacts) -> Self {
        Self {
            melds: facts.open_meld_count,
            visible_han: facts.open_visible_han_proxy(),
        }
    }
}

// 警戒条件を満たさない相手の reason。公開副露が1つも無い場合だけ FixedMeldPresent になる。
fn present_reason(facts: PlayerThreatFacts) -> OpenHandThreatReason {
    if facts.open_meld_count >= ONE_MELD {
        OpenHandThreatReason::OpenMeldPresent
    } else {
        OpenHandThreatReason::FixedMeldPresent
    }
}

// 満たした警戒条件のうち、優先順位が最も高いものの level と reason。
//
// level の判定は暗槓を含む軸だけで決まる。公開副露だけの軸は、その条件が公開情報だけでも成立
// するかを見て reason を選ぶためにだけ使う。
fn alert_decision(facts: PlayerThreatFacts) -> Option<OpenHandThreatDecision> {
    let fixed = alert_conditions(facts, MeldProgress::fixed(facts));
    let open = alert_conditions(facts, MeldProgress::open(facts));

    (0..ALERT_CONDITION_COUNT)
        .find(|&index| fixed[index])
        .map(|index| {
            let (open_reason, fixed_reason) = ALERT_REASONS[index];
            OpenHandThreatDecision {
                level: ALERT_LEVELS[index],
                reason: if open[index] {
                    open_reason
                } else {
                    fixed_reason
                },
            }
        })
}

// 警戒条件の成否。並びは [`ALERT_LEVELS`] / [`ALERT_REASONS`] と同じ優先順位で、Danger の条件が
// 先に並ぶ。
fn alert_conditions(
    facts: PlayerThreatFacts,
    progress: MeldProgress,
) -> [bool; ALERT_CONDITION_COUNT] {
    [
        progress.melds >= THREE_MELDS,
        progress.melds >= TWO_MELDS && progress.visible_han >= DANGER_VISIBLE_HAN_PROXY,
        facts.is_dealer == Some(true) && progress.melds >= TWO_MELDS,
        progress.melds >= TWO_MELDS && facts.discard_count >= MID_ROUND_DISCARD_COUNT,
        progress.melds >= ONE_MELD && facts.discard_count >= LATE_ROUND_DISCARD_COUNT,
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

    // Ankan だけを `count` 個持つ他家の facts。ドラも役牌も含まない。
    fn concealed_kans(count: usize) -> PlayerThreatFacts {
        PlayerThreatFacts {
            meld_count: count,
            open_meld_count: 0,
            kan_count: count,
            meld_kinds: MeldKindCounts {
                ankan: count,
                ..MeldKindCounts::default()
            },
            ..opponent_facts()
        }
    }

    // 公開副露1つと Ankan 1つを持つ他家の facts。完成面子は2つだが open meld は1つ。
    fn one_open_meld_and_one_concealed_kan() -> PlayerThreatFacts {
        PlayerThreatFacts {
            meld_count: 2,
            open_meld_count: 1,
            kan_count: 1,
            meld_kinds: MeldKindCounts {
                chi: 1,
                ankan: 1,
                ..MeldKindCounts::default()
            },
            ..opponent_facts()
        }
    }

    // 暗槓内のドラを足す。公開副露限定の facts には入れない。
    fn with_concealed_dora(facts: PlayerThreatFacts, dora: u8) -> PlayerThreatFacts {
        PlayerThreatFacts {
            meld_dora_count: facts.meld_dora_count + dora,
            ..facts
        }
    }

    // 暗槓の確定役牌 (三元牌) を1つ足す。公開副露限定の facts には入れない。
    fn with_concealed_value_honor(facts: PlayerThreatFacts) -> PlayerThreatFacts {
        let mut counts = facts.value_honor_melds;
        counts.dragon += 1;
        counts.confirmed += 1;
        PlayerThreatFacts {
            value_honor_melds: counts,
            ..facts
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

    fn with_two_value_honors(facts: PlayerThreatFacts) -> PlayerThreatFacts {
        let counts = ValueHonorMeldCounts {
            dragon: 2,
            confirmed: 2,
            ..ValueHonorMeldCounts::default()
        };
        PlayerThreatFacts {
            value_honor_melds: counts,
            open_value_honor_melds: counts,
            ..facts
        }
    }

    fn with_double_wind(facts: PlayerThreatFacts) -> PlayerThreatFacts {
        let counts = ValueHonorMeldCounts {
            round_wind: 1,
            seat_wind: 1,
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
        // 役牌副露1つだけでは Caution / Danger にしない。
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

    // ---- Danger / Caution 条件 ----

    #[test]
    fn three_open_melds_are_danger() {
        for count in [3, 4] {
            assert_classified(
                open_melds(count),
                OpenHandThreatLevel::Danger,
                OpenHandThreatReason::ThreeOrMoreOpenMelds,
            );
        }
    }

    #[test]
    fn two_melds_with_one_value_honor_are_present() {
        let facts = with_value_honor(open_melds(2));
        assert_eq!(facts.open_visible_han_proxy(), 1);
        assert_classified(
            facts,
            OpenHandThreatLevel::Present,
            OpenHandThreatReason::OpenMeldPresent,
        );
    }

    #[test]
    fn two_melds_with_one_value_honor_and_one_dora_are_danger() {
        let facts = with_open_dora(with_value_honor(open_melds(2)), 1);
        assert_eq!(facts.open_visible_han_proxy(), 2);
        assert_classified(
            facts,
            OpenHandThreatLevel::Danger,
            OpenHandThreatReason::TwoOrMoreWithVisibleHan,
        );
    }

    #[test]
    fn two_value_honor_melds_are_danger() {
        let facts = with_two_value_honors(open_melds(2));
        assert_eq!(facts.open_visible_han_proxy(), 2);
        assert_classified(
            facts,
            OpenHandThreatLevel::Danger,
            OpenHandThreatReason::TwoOrMoreWithVisibleHan,
        );
    }

    #[test]
    fn a_double_wind_counts_as_two_visible_han() {
        let facts = with_double_wind(open_melds(2));
        assert_eq!(facts.open_value_honor_melds.confirmed, 1);
        assert_eq!(facts.open_value_honor_melds.confirmed_han(), 2);
        assert_eq!(facts.open_visible_han_proxy(), 2);
        assert_classified(
            facts,
            OpenHandThreatLevel::Danger,
            OpenHandThreatReason::TwoOrMoreWithVisibleHan,
        );
    }

    #[test]
    fn two_melds_with_an_unconfirmed_wind_are_present() {
        // 場風・自風が不明な風牌の副露は役牌と確定していないので Danger 条件を満たさない。
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

        assert_eq!(facts.open_visible_han_proxy(), 0);
        assert_classified(
            facts,
            OpenHandThreatLevel::Present,
            OpenHandThreatReason::OpenMeldPresent,
        );
    }

    #[test]
    fn two_melds_with_two_open_dora_are_danger() {
        let facts = with_open_dora(open_melds(2), 2);
        assert_eq!(facts.open_visible_han_proxy(), 2);
        assert_classified(
            facts,
            OpenHandThreatLevel::Danger,
            OpenHandThreatReason::TwoOrMoreWithVisibleHan,
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
    fn a_dealer_with_two_melds_is_danger() {
        assert_classified(
            as_dealer(open_melds(2)),
            OpenHandThreatLevel::Danger,
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
    fn two_melds_at_nine_discards_are_caution() {
        assert_classified(
            with_discards(open_melds(2), 9),
            OpenHandThreatLevel::Caution,
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
    fn one_meld_at_twelve_discards_is_caution() {
        assert_classified(
            with_discards(open_melds(1), 12),
            OpenHandThreatLevel::Caution,
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
            OpenHandThreatLevel::Caution,
            OpenHandThreatReason::TwoOrMoreOpenMeldsFromNineDiscards,
        );
    }

    #[test]
    fn the_highest_priority_reason_wins() {
        // すべての警戒条件を同時に満たす facts から、条件を1つずつ外して優先順位を固定する。
        // Danger と Caution の条件を同時に満たす間は Danger の reason と level が優先される。
        let all = with_discards(
            as_dealer(with_open_dora(with_value_honor(open_melds(3)), 2)),
            12,
        );
        let expected = [
            OpenHandThreatReason::ThreeOrMoreOpenMelds,
            OpenHandThreatReason::TwoOrMoreWithVisibleHan,
            OpenHandThreatReason::DealerWithTwoOrMoreOpenMelds,
            OpenHandThreatReason::TwoOrMoreOpenMeldsFromNineDiscards,
            OpenHandThreatReason::OpenMeldFromTwelveDiscards,
        ];

        let mut facts = all;
        assert_classified(facts, OpenHandThreatLevel::Danger, expected[0]);

        facts = PlayerThreatFacts {
            meld_count: 2,
            open_meld_count: 2,
            ..facts
        };
        assert_classified(facts, OpenHandThreatLevel::Danger, expected[1]);

        facts = with_open_dora(facts, 0);
        facts = PlayerThreatFacts {
            value_honor_melds: ValueHonorMeldCounts::default(),
            open_value_honor_melds: ValueHonorMeldCounts::default(),
            ..facts
        };
        assert_classified(facts, OpenHandThreatLevel::Danger, expected[2]);

        facts = PlayerThreatFacts {
            is_dealer: Some(false),
            ..facts
        };
        assert_classified(facts, OpenHandThreatLevel::Caution, expected[3]);

        facts = PlayerThreatFacts {
            meld_count: 1,
            open_meld_count: 1,
            ..facts
        };
        assert_classified(facts, OpenHandThreatLevel::Caution, expected[4]);
    }

    #[test]
    fn every_danger_condition_alone_is_danger() {
        let conditions = [
            open_melds(3),
            with_open_dora(with_value_honor(open_melds(2)), 1),
            with_open_dora(open_melds(2), 2),
            as_dealer(open_melds(2)),
        ];

        for facts in conditions {
            assert_eq!(
                classify_open_hand_threat(facts).level(),
                Some(OpenHandThreatLevel::Danger),
                "{facts:?}"
            );
        }
    }

    #[test]
    fn every_caution_condition_alone_is_caution() {
        let conditions = [
            with_discards(open_melds(2), 9),
            with_discards(open_melds(1), 12),
            with_discards(concealed_kans(2), 9),
            with_discards(concealed_kans(1), 12),
        ];

        for facts in conditions {
            assert_eq!(
                classify_open_hand_threat(facts).level(),
                Some(OpenHandThreatLevel::Caution),
                "{facts:?}"
            );
        }
    }

    #[test]
    fn a_danger_condition_outranks_the_discard_conditions() {
        // 局進行の Caution 条件を同時に満たしても、Danger の level と reason を優先する。
        assert_classified(
            with_discards(open_melds(3), 12),
            OpenHandThreatLevel::Danger,
            OpenHandThreatReason::ThreeOrMoreOpenMelds,
        );
        assert_classified(
            with_discards(with_open_dora(open_melds(2), 2), 9),
            OpenHandThreatLevel::Danger,
            OpenHandThreatReason::TwoOrMoreWithVisibleHan,
        );
        assert_classified(
            with_discards(as_dealer(open_melds(2)), 12),
            OpenHandThreatLevel::Danger,
            OpenHandThreatReason::DealerWithTwoOrMoreOpenMelds,
        );
        assert_classified(
            with_discards(concealed_kans(3), 12),
            OpenHandThreatLevel::Danger,
            OpenHandThreatReason::ThreeOrMoreFixedMelds,
        );
    }

    // ---- Ankan (完成面子としての進行度と確認済み打点) ----

    #[test]
    fn a_single_concealed_kan_before_the_late_round_is_present() {
        // 暗槓は公開副露ではないが完成面子なので、None ではなく Present から始まる。
        for discard_count in [0, 1, 8, 11] {
            assert_classified(
                with_discards(concealed_kans(1), discard_count),
                OpenHandThreatLevel::Present,
                OpenHandThreatReason::FixedMeldPresent,
            );
        }
    }

    #[test]
    fn a_single_concealed_kan_at_twelve_discards_is_caution() {
        assert_classified(
            with_discards(concealed_kans(1), 12),
            OpenHandThreatLevel::Caution,
            OpenHandThreatReason::FixedMeldFromTwelveDiscards,
        );
    }

    #[test]
    fn two_concealed_kans_of_a_child_are_present_before_the_mid_round() {
        // 子・打点条件なし・河8枚以下なら、完成面子2つでも Present のまま。
        for discard_count in [0, 8] {
            assert_classified(
                with_discards(concealed_kans(2), discard_count),
                OpenHandThreatLevel::Present,
                OpenHandThreatReason::FixedMeldPresent,
            );
        }
    }

    #[test]
    fn two_concealed_kans_at_nine_discards_are_caution() {
        assert_classified(
            with_discards(concealed_kans(2), 9),
            OpenHandThreatLevel::Caution,
            OpenHandThreatReason::TwoOrMoreFixedMeldsFromNineDiscards,
        );
    }

    #[test]
    fn a_dealer_with_two_concealed_kans_is_danger() {
        assert_classified(
            as_dealer(concealed_kans(2)),
            OpenHandThreatLevel::Danger,
            OpenHandThreatReason::DealerWithTwoOrMoreFixedMelds,
        );
    }

    #[test]
    fn three_concealed_kans_are_danger() {
        for count in [3, 4] {
            assert_classified(
                concealed_kans(count),
                OpenHandThreatLevel::Danger,
                OpenHandThreatReason::ThreeOrMoreFixedMelds,
            );
        }
    }

    #[test]
    fn one_open_meld_and_one_concealed_kan_at_nine_discards_are_caution() {
        assert_classified(
            with_discards(one_open_meld_and_one_concealed_kan(), 9),
            OpenHandThreatLevel::Caution,
            OpenHandThreatReason::TwoOrMoreFixedMeldsFromNineDiscards,
        );
    }

    #[test]
    fn a_concealed_kan_dora_counts_toward_the_visible_han_proxy() {
        // 暗槓のドラ2枚だけで打点 proxy が2に届く。公開副露だけの proxy は0のまま。
        let facts = with_concealed_dora(one_open_meld_and_one_concealed_kan(), 2);

        assert_eq!(facts.open_visible_han_proxy(), 0);
        assert_eq!(facts.fixed_meld_visible_han_proxy(), 2);
        assert_classified(
            facts,
            OpenHandThreatLevel::Danger,
            OpenHandThreatReason::TwoOrMoreFixedMeldsWithVisibleHan,
        );
    }

    #[test]
    fn a_concealed_value_honor_kan_counts_toward_the_visible_han_proxy() {
        // 暗槓の確定役牌1翻と公開副露のドラ1翻を合わせて proxy が2になる。
        let facts =
            with_concealed_value_honor(with_open_dora(one_open_meld_and_one_concealed_kan(), 1));

        assert_eq!(facts.open_visible_han_proxy(), 1);
        assert_eq!(facts.fixed_meld_visible_han_proxy(), 2);
        assert_classified(
            facts,
            OpenHandThreatLevel::Danger,
            OpenHandThreatReason::TwoOrMoreFixedMeldsWithVisibleHan,
        );
    }

    #[test]
    fn multiple_concealed_kans_accumulate_in_the_visible_han_proxy() {
        // 暗槓が複数あってもドラ・確定役牌はそれぞれ加算される。
        let facts = with_concealed_value_honor(with_concealed_value_honor(with_concealed_dora(
            concealed_kans(2),
            1,
        )));

        assert_eq!(facts.value_honor_melds.confirmed, 2);
        assert_eq!(facts.fixed_meld_visible_han_proxy(), 3);
        assert_classified(
            facts,
            OpenHandThreatLevel::Danger,
            OpenHandThreatReason::TwoOrMoreFixedMeldsWithVisibleHan,
        );
    }

    #[test]
    fn an_open_meld_condition_keeps_its_open_reason_even_with_a_concealed_kan() {
        // 公開副露だけで既に成立している条件は、暗槓があっても従来の reason のまま。
        let facts = PlayerThreatFacts {
            meld_count: 4,
            open_meld_count: 3,
            kan_count: 1,
            meld_kinds: MeldKindCounts {
                chi: 3,
                ankan: 1,
                ..MeldKindCounts::default()
            },
            ..opponent_facts()
        };

        assert_classified(
            facts,
            OpenHandThreatLevel::Danger,
            OpenHandThreatReason::ThreeOrMoreOpenMelds,
        );
        assert_classified(
            with_discards(facts, 12),
            OpenHandThreatLevel::Danger,
            OpenHandThreatReason::ThreeOrMoreOpenMelds,
        );
    }

    // ---- actionable predicate ----

    #[test]
    fn caution_and_danger_are_actionable() {
        let caution = classify_open_hand_threat(with_discards(open_melds(1), 12));
        let danger = classify_open_hand_threat(open_melds(3));

        assert_eq!(caution.level(), Some(OpenHandThreatLevel::Caution));
        assert!(caution.is_actionable());
        assert_eq!(danger.level(), Some(OpenHandThreatLevel::Danger));
        assert!(danger.is_actionable());
    }

    #[test]
    fn present_and_none_are_not_actionable() {
        let present = classify_open_hand_threat(open_melds(1));
        let none = classify_open_hand_threat(opponent_facts());

        assert_eq!(present.level(), Some(OpenHandThreatLevel::Present));
        assert!(!present.is_actionable());
        assert_eq!(none.level(), Some(OpenHandThreatLevel::None));
        assert!(!none.is_actionable());
    }

    #[test]
    fn a_not_applicable_seat_is_not_actionable() {
        for exclusion in [
            OpenHandThreatExclusion::SelfSeat,
            OpenHandThreatExclusion::Reached,
            OpenHandThreatExclusion::UnknownSeat,
        ] {
            assert!(!OpenHandThreatAssessment::NotApplicable(exclusion).is_actionable());
        }
    }

    #[test]
    fn any_caution_or_danger_seat_makes_the_table_actionable() {
        let none = classify_open_hand_threat(opponent_facts());
        let present = classify_open_hand_threat(open_melds(1));
        let caution = classify_open_hand_threat(with_discards(open_melds(2), 9));
        let danger = classify_open_hand_threat(as_dealer(open_melds(2)));
        let self_seat = OpenHandThreatAssessment::NotApplicable(OpenHandThreatExclusion::SelfSeat);

        assert!(!has_actionable_open_hand_threat(&[
            self_seat, none, present, present
        ]));
        assert!(has_actionable_open_hand_threat(&[
            self_seat, none, present, caution
        ]));
        assert!(has_actionable_open_hand_threat(&[
            self_seat, danger, present, none
        ]));
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
        // player_id 不明の席を他家と推測して Present / Caution / Danger にしない。危険度なしにも確定させない。
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

    // 3m の大明槓。公開副露なので open meld にも入る。
    fn daiminkan() -> Meld {
        Meld::new(
            MeldKind::Daiminkan,
            (8..12).map(tile).collect(),
            Some(tile(8)),
        )
    }

    // 7m の加槓。公開副露なので open meld にも入る。
    fn kakan() -> Meld {
        Meld::new(
            MeldKind::Kakan,
            (24..28).map(tile).collect(),
            Some(tile(27)),
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
    fn an_ankan_is_a_fixed_meld_but_not_an_open_meld() {
        let context = context_with(vec![value_honor_ankan()], vec![], 0);
        let facts = player_threat_facts_from_context(&context)[3];

        assert_eq!(facts.meld_count, 1);
        assert_eq!(facts.open_meld_count, 0);
        assert_eq!(facts.open_visible_han_proxy(), 0);
        // 完成面子はあるので None ではなく Present。公開副露の reason とは区別する。
        assert_eq!(
            assess(&context, 3),
            classified(
                OpenHandThreatLevel::Present,
                OpenHandThreatReason::FixedMeldPresent
            )
        );
    }

    #[test]
    fn an_ankan_dora_is_counted_as_confirmed_value() {
        let context = context_with(vec![dora_ankan()], vec![tile(12)], 0);
        let facts = player_threat_facts_from_context(&context)[3];

        assert!(facts.meld_dora_count >= 2);
        assert_eq!(facts.open_meld_dora_count, 0);
        assert_eq!(facts.open_visible_han_proxy(), 0);
        assert!(facts.fixed_meld_visible_han_proxy() >= 2);
        // 打点は確認できても完成面子は1つなので、序盤は Present のまま。
        assert_eq!(
            assess(&context, 3),
            classified(
                OpenHandThreatLevel::Present,
                OpenHandThreatReason::FixedMeldPresent
            )
        );
    }

    #[test]
    fn a_value_honor_ankan_is_counted_as_confirmed_value() {
        let context = context_with(vec![value_honor_ankan()], vec![], 0);
        let facts = player_threat_facts_from_context(&context)[3];

        assert_eq!(facts.value_honor_melds.confirmed, 1);
        assert_eq!(facts.open_value_honor_melds.confirmed, 0);
        assert_eq!(facts.open_visible_han_proxy(), 0);
        assert_eq!(facts.fixed_meld_visible_han_proxy(), 1);
    }

    #[test]
    fn a_single_ankan_at_twelve_discards_is_caution_from_the_context() {
        let context = context_with(vec![value_honor_ankan()], vec![], 12);

        assert_eq!(
            assess(&context, 3),
            classified(
                OpenHandThreatLevel::Caution,
                OpenHandThreatReason::FixedMeldFromTwelveDiscards
            )
        );
    }

    #[test]
    fn an_ankan_with_a_chi_and_confirmed_value_is_danger_in_the_mid_round() {
        // regression: 1公開副露 + 暗槓の中盤の相手を、単なる1副露として扱わない。
        let context = context_with(vec![dora_ankan(), chi()], vec![tile(12)], 11);
        let facts = player_threat_facts_from_context(&context)[3];

        assert_eq!(facts.meld_count, 2);
        assert_eq!(facts.open_meld_count, 1);
        assert_eq!(facts.discard_count, 11);
        assert!(facts.meld_dora_count >= 2);
        assert_eq!(facts.open_meld_dora_count, 0);
        // 完成面子2つと暗槓のドラで、公開副露だけでは届かない Danger 条件を満たす。
        assert_eq!(
            assess(&context, 3),
            classified(
                OpenHandThreatLevel::Danger,
                OpenHandThreatReason::TwoOrMoreFixedMeldsWithVisibleHan
            )
        );
    }

    #[test]
    fn an_ankan_with_a_chi_at_nine_discards_is_caution() {
        let context = context_with(vec![value_honor_ankan(), chi()], vec![], 9);
        let facts = player_threat_facts_from_context(&context)[3];

        assert_eq!(facts.meld_count, 2);
        assert_eq!(facts.open_meld_count, 1);
        // 確定役牌1翻だけでは打点条件に届かず、河9枚の進行条件で Caution になる。
        assert_eq!(facts.fixed_meld_visible_han_proxy(), 1);
        assert_eq!(
            assess(&context, 3),
            classified(
                OpenHandThreatLevel::Caution,
                OpenHandThreatReason::TwoOrMoreFixedMeldsFromNineDiscards
            )
        );
    }

    #[test]
    fn an_ankan_with_a_chi_before_the_mid_round_is_present() {
        let context = context_with(vec![value_honor_ankan(), chi()], vec![], 8);

        assert_eq!(
            assess(&context, 3),
            classified(
                OpenHandThreatLevel::Present,
                OpenHandThreatReason::OpenMeldPresent
            )
        );
    }

    #[test]
    fn the_fixed_meld_count_drives_the_classification_from_the_context() {
        // 同じ2面子でも、公開副露だけで成立した条件は従来の reason のまま表示する。
        let two_open = context_with(vec![chi(), chi()], vec![], 9);
        let one_open = context_with(vec![value_honor_ankan(), chi()], vec![], 9);

        assert_eq!(
            assess(&two_open, 3),
            classified(
                OpenHandThreatLevel::Caution,
                OpenHandThreatReason::TwoOrMoreOpenMeldsFromNineDiscards
            )
        );
        assert_eq!(
            assess(&one_open, 3),
            classified(
                OpenHandThreatLevel::Caution,
                OpenHandThreatReason::TwoOrMoreFixedMeldsFromNineDiscards
            )
        );
    }

    #[test]
    fn a_daiminkan_and_a_kakan_are_counted_once_each() {
        // 公開された槓は open meld でもあるので、暗槓として二重には数えない。
        let context = context_with(vec![daiminkan(), kakan()], vec![], 9);
        let facts = player_threat_facts_from_context(&context)[3];

        assert_eq!(facts.meld_count, 2);
        assert_eq!(facts.open_meld_count, 2);
        assert_eq!(facts.kan_count, 2);
        assert_eq!(facts.meld_kinds.ankan, 0);
        assert_eq!(
            assess(&context, 3),
            classified(
                OpenHandThreatLevel::Caution,
                OpenHandThreatReason::TwoOrMoreOpenMeldsFromNineDiscards
            )
        );
    }

    #[test]
    fn a_daiminkan_and_a_kakan_with_an_ankan_are_three_fixed_melds() {
        let context = context_with(vec![daiminkan(), kakan(), value_honor_ankan()], vec![], 0);
        let facts = player_threat_facts_from_context(&context)[3];

        assert_eq!(facts.meld_count, 3);
        assert_eq!(facts.open_meld_count, 2);
        assert_eq!(facts.kan_count, 3);
        assert_eq!(
            assess(&context, 3),
            classified(
                OpenHandThreatLevel::Danger,
                OpenHandThreatReason::ThreeOrMoreFixedMelds
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
