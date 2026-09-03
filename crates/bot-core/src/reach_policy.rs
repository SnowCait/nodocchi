//! 攻撃を継続するテンパイで Reach / Damaten のどちらを選ぶかを決める policy 層。
//!
//! リーチ判断そのもの ([`crate::agents::ShantenAgent`] の Reach action 選択) と、押し引きが
//! 攻撃打点を求めるときの攻撃モード判定は、同じ結論でなければならない。両者が同じ条件を別々に
//! 書くと片方だけがずれるため、条件は [`decide_reach_reason`] 1本だけが持つ。Reach action 判断
//! だけに適用する categorical rule も同じく1本の helper が持ち、条件を呼び出し側へ散らさない。
//!
//! この層は待ち・フリテン・ダマ打点を計算しない。既にそれぞれの source of truth から求めた
//! 結論 (合法 Reach の有無・[`DamatenValueVerdict`]・生きた待ちの残枚数) を受け取り、そこから
//! 理由を1つ選ぶだけの pure helper になっている。
//!
//! # categorical rule
//!
//! ダマ打点 threshold とは別に、既存 scoring が named 役満と確定した恒常フリテン聴牌だけを
//! 対象にする categorical rule ([`selects_named_yakuman_damaten`]) を持つ。ダマ打点の大小では
//! なく「ロンできない聴牌のツモ和了が名前の付いた役満で確定している」という別の事実に基づく
//! ため、[`ReachDecisionReason::HighValueDamaten`] へ統合せず専用の理由で区別する。
//!
//! この rule を適用するのは production の Reach action 判断だけで、押し引きの攻撃モードと将来
//! テンパイの selection value は従来どおり [`decide_reach_reason`] の結論のまま変えない。役満
//! 判定そのものもこの層は持たず、既存 scoring が named 役満と確定したかどうかという事実だけを
//! 受け取る。
//!
//! # リーチの合法性
//!
//! 「リーチを宣言できる局面か」という条件も [`is_reach_legal`] 1本だけが持つ。合法手を組み立て
//! る scenario と、将来テンパイの Reach / Damaten 判断を再現する経路が同じ条件を別々に書かない
//! ようにするための共有 helper で、条件を満たすかどうかの材料 ([`ReachLegalityFacts`]) は
//! 呼び出し側がそれぞれの局面から集める。
//!
//! # リーチの timing
//!
//! [`decide_reach_reason`] が Reach を選んだ後に、そのリーチを**今回の request で宣言するか**を
//! 決めるのが [`ReachTimingDecision`] の層である。base policy とは別概念で、
//!
//! ```text
//! ReachDecisionReason   Reach か Damaten かという base policy
//! ReachTimingDecision   base policy が Reach の場合に、今回宣言するか見送るか
//! ```
//!
//! という関係になる。base policy がダマを選んだ聴牌では timing 判断そのものを行わず、
//! [`ReachTimingDecision`] に Damaten は含まれない。
//!
//! [`ReachTimingDecision::DeferReach`] は「今回はリーチを宣言せず、通常打牌 selection が選んだ
//! Dahai を行う」だけの意味で、次の局面では状態を持たずに通常 policy を評価し直す。「必ず1巡
//! 待ってからリーチする」という production state ではない。判断材料に使う `1巡 defer` は
//! counterfactual の評価 horizon であって、production action の約束ではない。
//!
//! production で timing を評価するのは、恒常フリテンが確定した聴牌
//! ([`evaluates_reach_timing`]) と、structural gate をすべて満たす非フリテン悪形の暫定 heuristic
//! ([`evaluates_non_furiten_bad_wait_reach_timing`]) だけである。比較はどちらも既存 self-tsumo
//! metric の大小だけを見る。この層も待ち・打点・確率を計算しない。

use bot_logic::PermanentFuriten;

use crate::damaten_value::DamatenValueVerdict;
use crate::defense::{SujiSafetyRank, WallRank};

/// リーチ宣言に必要な持ち点 [点]。inclusive。
pub const REACH_MIN_SCORE: i32 = 1000;

/// リーチ宣言に必要な山の残りツモ牌数。inclusive。
pub const REACH_MIN_REMAINING_TILES: u32 = 4;

/// リーチが合法かを決める局面の材料。
///
/// unknown な材料は `None` で表す。分からない材料からリーチ不可と推測しないため、`None` は
/// その条件を満たすものとして扱う。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReachLegalityFacts {
    /// 門前か。自分の副露が分からない場合は `None`。
    pub menzen: Option<bool>,
    /// 既にリーチしているか。自分の席を特定できない場合は `None`。
    pub already_reached: Option<bool>,
    /// 自分の現在持ち点 [点]。
    pub score: Option<i32>,
    /// 山の残りツモ牌数。
    pub remaining_tiles: Option<u32>,
    /// その打牌の後がテンパイか。
    pub tenpai_after_discard: bool,
}

/// 局面がリーチ宣言の条件を満たすか。
///
/// RiichiEnv の4麻 semantics に合わせ、門前・未リーチ・打牌後テンパイ・持ち点・残りツモ牌数の
/// 全てを満たす場合だけ合法とする。unknown な材料はリーチ不可と推測せず、明示的に不可能と
/// 分かる場合だけ `false` にする。
pub fn is_reach_legal(facts: ReachLegalityFacts) -> bool {
    facts.menzen.unwrap_or(true)
        && !facts.already_reached.unwrap_or(false)
        && facts.tenpai_after_discard
        && facts.score.is_none_or(|score| score >= REACH_MIN_SCORE)
        && facts
            .remaining_tiles
            .is_none_or(|remaining| remaining >= REACH_MIN_REMAINING_TILES)
}

// 補正後の待ち枚数がこの枚数以上ならリーチする。生牌の単騎は3枚なので、待ち枚数だけを理由に
// 抑制するのは3枚未満に限る。
pub const REACH_MIN_REMAINING: u8 = 3;

/// リーチを採用した / しなかった理由。
///
/// `Eligible*` 以外はすべて「今回はリーチしない」理由であり、最初に落ちた条件を1つだけ表す。
/// 判定順は [`ReachDecisionDiagnostic`](crate::reach_decision::ReachDecisionDiagnostic) の
/// フィールドが埋まる順と一致する。
///
/// ダマ打点による判断 ([`DamatenValueVerdict`]) を適用した経路と、ダマ打点を使わない既存判断の
/// 経路は別の理由として区別する。既存判断へ落ちるのはフリテン・ロン可否 unknown・ダマ打点を
/// 確定できない場合で、そこでは非フリテンだともダマ打点が十分だとも推測しない。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReachDecisionReason {
    /// ダマ打点を使わない既存判断で、選んだ打牌後が生きた待ちを十分に持つテンパイになる。
    Eligible,
    /// ダマでは役が無い待ちがあるためリーチする。
    EligibleNoDamatenYaku,
    /// ダマ打点が threshold 未満の待ちがあるためリーチする。
    EligibleLowValue,
    /// 全ての生きた待ちがダマで役ありかつ threshold 以上なのでダマにする。
    HighValueDamaten,
    /// 恒常フリテンで、全ての生きた Tsumo physical variant が named 役満と確定したのでダマに
    /// する。
    ///
    /// ダマ打点 threshold ([`Self::HighValueDamaten`]) とは別の categorical rule で、判断材料も
    /// 既存 scoring の役満判定だけになる。数え役満・一部だけ named 役満・scoring unknown は
    /// 含まない。
    NamedYakumanDamaten,
    /// 合法 action に [`LegalAction::Reach`](crate::action::LegalAction::Reach) が無い。
    NoLegalReach,
    /// 通常打牌 selection が打牌を選べていない。手牌や合法 Dahai が無い局面。
    NoSelectedDiscard,
    /// 選んだ打牌の後がテンパイではない。
    NotTenpai,
    /// テンパイだが、生きた待ちが1枚も無い。
    NoLiveWait,
    /// ダマ打点を使わない既存判断で、ツモ和了できる待ちの残枚数が threshold 未満。
    InsufficientLiveWait,
}

impl ReachDecisionReason {
    /// この理由がリーチを選ぶものか。
    ///
    /// 攻撃を継続する場合の攻撃モードもこの述語で決まる。リーチを選ばない理由はすべて、
    /// 攻撃継続時にダマのまま進むことを意味する。
    pub fn selects_reach(self) -> bool {
        matches!(
            self,
            Self::Eligible | Self::EligibleNoDamatenYaku | Self::EligibleLowValue
        )
    }
}

/// テンパイ手を攻撃継続する場合に、リーチするかダマのままにするかを決める。
///
/// `reach_legal` は合法 action に Reach があるか。合法 Reach が無ければダマ以外に選択肢が無い
/// ため、他の材料を見ずに [`ReachDecisionReason::NoLegalReach`] になる。
///
/// `damaten_verdict` はダマ打点から畳んだ結論。ダマ打点を評価しなかった場合 (ダマでロンできない・
/// ロン可否が unknown・打牌後の手牌を組み立てられない) は `None` で、その場合は非フリテンだとも
/// ダマ打点が十分だとも推測せず、待ち枚数だけを見る既存判断になる。
///
/// `tsumo_remaining` は生きた待ちの残枚数。ダマ打点で結論が出る場合、待ち枚数の threshold
/// ([`REACH_MIN_REMAINING`]) を先に適用してリーチを抑制しない。したがって1～2枚待ちでもダマが
/// 安ければリーチする。
pub fn decide_reach_reason(
    reach_legal: bool,
    damaten_verdict: Option<DamatenValueVerdict>,
    tsumo_remaining: u8,
) -> ReachDecisionReason {
    if !reach_legal {
        return ReachDecisionReason::NoLegalReach;
    }

    match damaten_verdict {
        Some(DamatenValueVerdict::NoLiveWait) => ReachDecisionReason::NoLiveWait,
        Some(DamatenValueVerdict::NoYaku) => ReachDecisionReason::EligibleNoDamatenYaku,
        Some(DamatenValueVerdict::BelowThreshold) => ReachDecisionReason::EligibleLowValue,
        Some(DamatenValueVerdict::AboveThreshold) => ReachDecisionReason::HighValueDamaten,
        Some(DamatenValueVerdict::Indeterminate) | None => {
            if tsumo_remaining >= REACH_MIN_REMAINING {
                ReachDecisionReason::Eligible
            } else {
                ReachDecisionReason::InsufficientLiveWait
            }
        }
    }
}

/// named 役満の categorical rule を評価する局面か。
///
/// 恒常フリテンが確定していて、生きた待ちが残っている聴牌だけを対象にする。恒常フリテンは既存の
/// [`PermanentFuriten`] だけが source of truth で、[`PermanentFuriten::Unknown`] を恒常フリテン
/// だと推測しない。対象外の局面では役満判定そのものを行わない。
pub fn evaluates_named_yakuman_damaten(
    permanent_furiten: PermanentFuriten,
    tsumo_remaining: u8,
) -> bool {
    permanent_furiten == PermanentFuriten::Yes && tsumo_remaining > 0
}

/// 恒常フリテンの named 役満聴牌でリーチを宣言しない categorical rule。
///
/// ロンできない聴牌のツモ和了が既存 scoring 上 named 役満で確定しているという事実だけで
/// [`ReachDecisionReason::NamedYakumanDamaten`] を選ぶ。ダマ打点の大小を見る
/// [`ReachDecisionReason::HighValueDamaten`] とは別の判断で、打点 threshold も待ち枚数
/// threshold も持たない。
///
/// 対象は [`evaluates_named_yakuman_damaten`] を満たす局面だけで、非恒常フリテンの named 役満は
/// 従来どおりダマ打点 threshold の結論になる。
///
/// `named_yakuman_established` は「生きた Tsumo physical variant がすべて named 役満だと既存
/// scoring 上確定した」かどうかで、呼び出し側の scoring がその1つの事実へ畳んで渡す。一部の
/// variant だけ役満・数え役満・scoring unknown・生きた variant なし・そもそも評価していない場合は
/// どれも `false` で、この層は役満を判定し直さない。
pub fn selects_named_yakuman_damaten(
    permanent_furiten: PermanentFuriten,
    tsumo_remaining: u8,
    named_yakuman_established: bool,
) -> bool {
    evaluates_named_yakuman_damaten(permanent_furiten, tsumo_remaining) && named_yakuman_established
}

/// base policy がリーチを選んだ聴牌で、そのリーチを今回宣言するかどうか。
///
/// base policy ([`decide_reach_reason`]) の Reach / Damaten とは別の層で、Damaten はこの enum に
/// 含まれない。base policy がダマを選んだ聴牌では timing 判断そのものを行わない。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReachTimingDecision {
    /// 今回の request でリーチを宣言する。
    ReachNow,
    /// 今回の request ではリーチを宣言せず、通常打牌 selection が選んだ Dahai を行う。
    ///
    /// 「必ず1巡待ってからリーチする」という production state ではない。次の局面では状態を
    /// 記憶せず、通常 policy をその局面から評価し直す。
    DeferReach,
}

/// timing 判断の理由。最初に落ちた条件を1つだけ表す。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReachTimingReason {
    /// 恒常フリテンが確定していないので timing evaluation の対象外。base policy のままリーチする。
    NotPermanentFuriten,
    /// timing evaluation の gate は通ったが、self-tsumo 比較のどちらかを確定できなかった。
    ///
    /// 比較不能を 0 点と混同せず、base policy のままリーチする。
    SelfTsumoComparisonUnknown,
    /// 恒常フリテン聴牌の self-tsumo 比較が確定した。決定はその大小そのもの。
    PermanentFuritenSelfTsumo,
    /// 非フリテン悪形の暫定 heuristic の structural gate を満たさないため対象外。
    ///
    /// Ron probability を推定した結果ではなく、base policy のままリーチする。
    NonFuritenBadWaitHeuristicNotEligible,
    /// 非フリテン悪形の暫定 heuristic が対象にした self-tsumo 比較。決定はその大小そのもの。
    ///
    /// 非現物・壁なし・無スジは公開情報上の安全根拠が無いという structural gate にだけ使い、
    /// ロン確率や数値係数には変換しない。
    NonFuritenBadWaitHeuristic,
}

/// timing 判断とその材料。
///
/// `reach_now` / `defer_forced_reach` は評価した場合だけ持つ既存 self-tsumo 比較の値そのもので、
/// この層が確率模型も点数計算も持たない。評価対象外の局面ではどちらも `None`。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReachTimingDiagnostic {
    pub decision: ReachTimingDecision,
    pub reason: ReachTimingReason,
    /// 今すぐリーチして現在の待ちのまま残り自摸機会を使い切る期待ツモ支払い。
    pub reach_now: Option<u64>,
    /// 1巡 defer し、次のテンパイで合法ならリーチする counterfactual の期待ツモ支払い。
    pub defer_forced_reach: Option<u64>,
}

impl ReachTimingDiagnostic {
    /// timing evaluation の対象ではなかった局面の結果。
    pub fn not_evaluated() -> Self {
        Self {
            decision: ReachTimingDecision::ReachNow,
            reason: ReachTimingReason::NotPermanentFuriten,
            reach_now: None,
            defer_forced_reach: None,
        }
    }

    /// 非フリテン悪形の暫定 heuristic の対象ではなかった局面の結果。
    pub fn non_furiten_heuristic_not_evaluated() -> Self {
        Self {
            decision: ReachTimingDecision::ReachNow,
            reason: ReachTimingReason::NonFuritenBadWaitHeuristicNotEligible,
            reach_now: None,
            defer_forced_reach: None,
        }
    }

    /// 今回はリーチを宣言しないか。
    pub fn defers_reach(&self) -> bool {
        matches!(self.decision, ReachTimingDecision::DeferReach)
    }

    /// この timing 判断が選んだ側の self-tsumo value。
    ///
    /// [`ReachTimingDecision::ReachNow`] なら `reach now`、
    /// [`ReachTimingDecision::DeferReach`] なら `defer → forced Reach` の値そのもの。比較不能
    /// ([`ReachTimingReason::SelfTsumoComparisonUnknown`]) で base policy のリーチを維持した
    /// 場合は、確定しない比較を 0 点にせず `None`。
    pub fn self_tsumo_value(&self) -> Option<u64> {
        if self.reason == ReachTimingReason::SelfTsumoComparisonUnknown {
            return None;
        }
        match self.decision {
            ReachTimingDecision::ReachNow => self.reach_now,
            ReachTimingDecision::DeferReach => self.defer_forced_reach,
        }
    }
}

/// 恒常フリテン聴牌の timing evaluation を行う局面か。
///
/// 恒常フリテンは既存の [`PermanentFuriten`] だけが source of truth で、
/// [`PermanentFuriten::Unknown`] を恒常フリテンだと推測しない。非フリテン聴牌のうち暫定
/// structural heuristic の対象だけは、別の [`evaluates_non_furiten_bad_wait_reach_timing`] で
/// 判定する。非フリテン全般を self-tsumo 比較へ入れるものではない。
pub fn evaluates_reach_timing(permanent_furiten: PermanentFuriten, tsumo_remaining: u8) -> bool {
    permanent_furiten == PermanentFuriten::Yes && tsumo_remaining > 0
}

/// 非フリテン悪形へ timing evaluation を接続する暫定 heuristic の structural facts。
///
/// 待ち・残枚数・ロン可否は選択済み打牌後の既存値、public safety は Reach 宣言牌を河へ置いた
/// 状態へ既存 Defense helper を適用した値、external threats は既存 classification の件数を
/// 受け取る。この型は確率も safety の数値係数も持たない。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NonFuritenBadWaitTimingFacts {
    pub permanent_furiten: PermanentFuriten,
    pub can_ron: Option<bool>,
    pub live_wait_type_count: usize,
    pub live_copies: u8,
    /// 既存 [`bot_logic::TileType::is_yaochu`] を反転した判定そのもの。
    pub wait_is_non_yaochu: bool,
    pub reach_genbutsu: Option<bool>,
    pub wall_rank: Option<WallRank>,
    pub suji_rank: Option<SujiSafetyRank>,
    pub reached_opponent_count: usize,
    pub high_open_hand_target_count: usize,
}

/// 非フリテン悪形の暫定 heuristic が selected wait を評価対象にするか。
///
/// 非現物・壁なし・無スジは「ロンされやすい」という確率的意味ではなく、Reach 後の public
/// safety に既存の現物・壁・スジによる安全根拠が無いことだけを表す structural gate。
pub fn evaluates_non_furiten_bad_wait_reach_timing(facts: NonFuritenBadWaitTimingFacts) -> bool {
    facts.permanent_furiten == PermanentFuriten::No
        && facts.can_ron == Some(true)
        && facts.live_wait_type_count == 1
        && (1..=3).contains(&facts.live_copies)
        && facts.wait_is_non_yaochu
        && facts.reach_genbutsu == Some(false)
        && facts.wall_rank == Some(WallRank::NoWall)
        && facts.suji_rank == Some(SujiSafetyRank::NoSuji)
        && facts.reached_opponent_count == 0
        && facts.high_open_hand_target_count == 0
}

/// 恒常フリテン聴牌の self-tsumo 比較から timing を決める。
///
/// 比べるのは既存 self-tsumo metric の大小だけで、点差・割合・待ち枚数の threshold は持たない。
/// 同値は [`ReachTimingDecision::ReachNow`]。
///
/// どちらかを確定できない場合は 0 点として扱わず、比較不能として base policy のリーチを維持する。
pub fn decide_permanent_furiten_reach_timing(
    reach_now: Option<u64>,
    defer_forced_reach: Option<u64>,
) -> ReachTimingDiagnostic {
    decide_self_tsumo_reach_timing(
        ReachTimingReason::PermanentFuritenSelfTsumo,
        reach_now,
        defer_forced_reach,
    )
}

/// 非フリテン悪形の暫定 heuristic で既存 self-tsumo 比較から timing を決める。
///
/// 比べるのは大小だけで、同値は ReachNow。点差・割合 threshold や Ron probability は持たない。
pub fn decide_non_furiten_bad_wait_reach_timing(
    reach_now: Option<u64>,
    defer_forced_reach: Option<u64>,
) -> ReachTimingDiagnostic {
    decide_self_tsumo_reach_timing(
        ReachTimingReason::NonFuritenBadWaitHeuristic,
        reach_now,
        defer_forced_reach,
    )
}

fn decide_self_tsumo_reach_timing(
    evaluated_reason: ReachTimingReason,
    reach_now: Option<u64>,
    defer_forced_reach: Option<u64>,
) -> ReachTimingDiagnostic {
    let (decision, reason) = match (reach_now, defer_forced_reach) {
        (Some(now), Some(defer)) if defer > now => {
            (ReachTimingDecision::DeferReach, evaluated_reason)
        }
        (Some(_), Some(_)) => (ReachTimingDecision::ReachNow, evaluated_reason),
        _ => (
            ReachTimingDecision::ReachNow,
            ReachTimingReason::SelfTsumoComparisonUnknown,
        ),
    };

    ReachTimingDiagnostic {
        decision,
        reason,
        reach_now,
        defer_forced_reach,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn legality_facts() -> ReachLegalityFacts {
        ReachLegalityFacts {
            menzen: Some(true),
            already_reached: Some(false),
            score: Some(25_000),
            remaining_tiles: Some(70),
            tenpai_after_discard: true,
        }
    }

    #[test]
    fn every_reach_condition_must_hold() {
        assert!(is_reach_legal(legality_facts()));

        for facts in [
            ReachLegalityFacts {
                menzen: Some(false),
                ..legality_facts()
            },
            ReachLegalityFacts {
                already_reached: Some(true),
                ..legality_facts()
            },
            ReachLegalityFacts {
                score: Some(REACH_MIN_SCORE - 100),
                ..legality_facts()
            },
            ReachLegalityFacts {
                remaining_tiles: Some(REACH_MIN_REMAINING_TILES - 1),
                ..legality_facts()
            },
            ReachLegalityFacts {
                tenpai_after_discard: false,
                ..legality_facts()
            },
        ] {
            assert!(!is_reach_legal(facts), "{facts:?}");
        }
    }

    #[test]
    fn unknown_facts_do_not_conclude_that_reach_is_illegal() {
        assert!(is_reach_legal(ReachLegalityFacts {
            menzen: None,
            already_reached: None,
            score: None,
            remaining_tiles: None,
            tenpai_after_discard: true,
        }));
    }

    #[test]
    fn an_illegal_reach_never_selects_reach() {
        for verdict in [
            None,
            Some(DamatenValueVerdict::NoYaku),
            Some(DamatenValueVerdict::BelowThreshold),
            Some(DamatenValueVerdict::AboveThreshold),
        ] {
            let reason = decide_reach_reason(false, verdict, 20);
            assert_eq!(reason, ReachDecisionReason::NoLegalReach);
            assert!(!reason.selects_reach());
        }
    }

    #[test]
    fn a_conclusive_damaten_verdict_ignores_the_live_wait_threshold() {
        for remaining in [0, 1, REACH_MIN_REMAINING, 20] {
            assert_eq!(
                decide_reach_reason(true, Some(DamatenValueVerdict::NoYaku), remaining),
                ReachDecisionReason::EligibleNoDamatenYaku
            );
            assert_eq!(
                decide_reach_reason(true, Some(DamatenValueVerdict::BelowThreshold), remaining),
                ReachDecisionReason::EligibleLowValue
            );
            assert_eq!(
                decide_reach_reason(true, Some(DamatenValueVerdict::AboveThreshold), remaining),
                ReachDecisionReason::HighValueDamaten
            );
            assert_eq!(
                decide_reach_reason(true, Some(DamatenValueVerdict::NoLiveWait), remaining),
                ReachDecisionReason::NoLiveWait
            );
        }
    }

    #[test]
    fn an_indeterminate_damaten_value_falls_back_to_the_live_wait_threshold() {
        for verdict in [None, Some(DamatenValueVerdict::Indeterminate)] {
            assert_eq!(
                decide_reach_reason(true, verdict, REACH_MIN_REMAINING),
                ReachDecisionReason::Eligible
            );
            assert_eq!(
                decide_reach_reason(true, verdict, REACH_MIN_REMAINING - 1),
                ReachDecisionReason::InsufficientLiveWait
            );
        }
    }

    #[test]
    fn only_a_confirmed_permanent_furiten_evaluates_the_timing() {
        assert!(evaluates_reach_timing(PermanentFuriten::Yes, 1));

        // 恒常フリテンが確定していない局面と、生きた待ちが1枚も無い聴牌は対象外。unknown を
        // 恒常フリテンだと推測しない。
        for facts in [
            (PermanentFuriten::No, 8),
            (PermanentFuriten::Unknown, 8),
            (PermanentFuriten::Yes, 0),
        ] {
            assert!(!evaluates_reach_timing(facts.0, facts.1), "{facts:?}");
        }
    }

    fn non_furiten_bad_wait_facts() -> NonFuritenBadWaitTimingFacts {
        NonFuritenBadWaitTimingFacts {
            permanent_furiten: PermanentFuriten::No,
            can_ron: Some(true),
            live_wait_type_count: 1,
            live_copies: 3,
            wait_is_non_yaochu: true,
            reach_genbutsu: Some(false),
            wall_rank: Some(WallRank::NoWall),
            suji_rank: Some(SujiSafetyRank::NoSuji),
            reached_opponent_count: 0,
            high_open_hand_target_count: 0,
        }
    }

    #[test]
    fn only_the_limited_non_furiten_bad_wait_evaluates_the_timing() {
        assert!(evaluates_non_furiten_bad_wait_reach_timing(
            non_furiten_bad_wait_facts()
        ));

        // 各 structural gate を1つずつ外す。壁・スジは既存 enum の生 facts を直接見る。
        for facts in [
            NonFuritenBadWaitTimingFacts {
                permanent_furiten: PermanentFuriten::Yes,
                ..non_furiten_bad_wait_facts()
            },
            NonFuritenBadWaitTimingFacts {
                permanent_furiten: PermanentFuriten::Unknown,
                ..non_furiten_bad_wait_facts()
            },
            NonFuritenBadWaitTimingFacts {
                can_ron: Some(false),
                ..non_furiten_bad_wait_facts()
            },
            NonFuritenBadWaitTimingFacts {
                can_ron: None,
                ..non_furiten_bad_wait_facts()
            },
            NonFuritenBadWaitTimingFacts {
                live_wait_type_count: 2,
                ..non_furiten_bad_wait_facts()
            },
            NonFuritenBadWaitTimingFacts {
                live_copies: 0,
                ..non_furiten_bad_wait_facts()
            },
            NonFuritenBadWaitTimingFacts {
                live_copies: 4,
                ..non_furiten_bad_wait_facts()
            },
            NonFuritenBadWaitTimingFacts {
                wait_is_non_yaochu: false,
                ..non_furiten_bad_wait_facts()
            },
            NonFuritenBadWaitTimingFacts {
                reached_opponent_count: 1,
                ..non_furiten_bad_wait_facts()
            },
            NonFuritenBadWaitTimingFacts {
                high_open_hand_target_count: 1,
                ..non_furiten_bad_wait_facts()
            },
        ] {
            assert!(
                !evaluates_non_furiten_bad_wait_reach_timing(facts),
                "{facts:?}"
            );
        }
    }

    #[test]
    fn the_non_furiten_bad_wait_requires_raw_no_wall_no_suji_non_genbutsu_evidence() {
        let eligible = non_furiten_bad_wait_facts();
        assert_eq!(eligible.reach_genbutsu, Some(false));
        assert_eq!(eligible.wall_rank, Some(WallRank::NoWall));
        assert_eq!(eligible.suji_rank, Some(SujiSafetyRank::NoSuji));
        assert!(evaluates_non_furiten_bad_wait_reach_timing(eligible));

        for facts in [
            NonFuritenBadWaitTimingFacts {
                reach_genbutsu: Some(true),
                ..eligible
            },
            NonFuritenBadWaitTimingFacts {
                wall_rank: Some(WallRank::OneChance),
                ..eligible
            },
            NonFuritenBadWaitTimingFacts {
                wall_rank: Some(WallRank::NoChance),
                ..eligible
            },
            NonFuritenBadWaitTimingFacts {
                suji_rank: Some(SujiSafetyRank::HalfSuji),
                ..eligible
            },
            NonFuritenBadWaitTimingFacts {
                suji_rank: Some(SujiSafetyRank::Suji),
                ..eligible
            },
            NonFuritenBadWaitTimingFacts {
                reach_genbutsu: None,
                wall_rank: None,
                suji_rank: None,
                ..eligible
            },
        ] {
            assert!(
                !evaluates_non_furiten_bad_wait_reach_timing(facts),
                "{facts:?}"
            );
        }
    }

    #[test]
    fn only_a_strictly_higher_defer_value_defers_the_reach() {
        let defer = decide_permanent_furiten_reach_timing(Some(100), Some(101));
        assert_eq!(defer.decision, ReachTimingDecision::DeferReach);
        assert_eq!(defer.reason, ReachTimingReason::PermanentFuritenSelfTsumo);
        assert!(defer.defers_reach());

        // 同値は ReachNow。点差や割合の threshold は持たない。
        for (reach_now, defer_forced_reach) in [(100, 100), (100, 99), (100, 0)] {
            let timing =
                decide_permanent_furiten_reach_timing(Some(reach_now), Some(defer_forced_reach));
            assert_eq!(timing.decision, ReachTimingDecision::ReachNow);
            assert_eq!(timing.reason, ReachTimingReason::PermanentFuritenSelfTsumo);
            assert!(!timing.defers_reach());
        }
    }

    #[test]
    fn an_unresolved_comparison_never_defers_the_reach() {
        // 確定できない値を 0 点として扱わない。比較不能なら base policy のリーチを維持する。
        for values in [(None, Some(100)), (Some(100), None), (None, None)] {
            let timing = decide_permanent_furiten_reach_timing(values.0, values.1);
            assert_eq!(timing.decision, ReachTimingDecision::ReachNow);
            assert_eq!(timing.reason, ReachTimingReason::SelfTsumoComparisonUnknown);
            assert_eq!(timing.reach_now, values.0);
            assert_eq!(timing.defer_forced_reach, values.1);
        }
    }

    #[test]
    fn the_non_furiten_bad_wait_heuristic_uses_only_strict_ordering() {
        let defer = decide_non_furiten_bad_wait_reach_timing(Some(100), Some(101));
        assert_eq!(defer.decision, ReachTimingDecision::DeferReach);
        assert_eq!(defer.reason, ReachTimingReason::NonFuritenBadWaitHeuristic);

        // 同値と defer 劣位は ReachNow。比率や点差 threshold は持たない。
        for values in [(Some(100), Some(100)), (Some(100), Some(99))] {
            let timing = decide_non_furiten_bad_wait_reach_timing(values.0, values.1);
            assert_eq!(timing.decision, ReachTimingDecision::ReachNow);
            assert_eq!(timing.reason, ReachTimingReason::NonFuritenBadWaitHeuristic);
        }

        // unknown は 0 と扱わず、base Reach を維持する。
        for values in [(None, Some(100)), (Some(100), None), (None, None)] {
            let timing = decide_non_furiten_bad_wait_reach_timing(values.0, values.1);
            assert_eq!(timing.decision, ReachTimingDecision::ReachNow);
            assert_eq!(timing.reason, ReachTimingReason::SelfTsumoComparisonUnknown);
        }
    }

    #[test]
    fn a_timing_that_was_not_evaluated_keeps_the_base_reach() {
        let timing = ReachTimingDiagnostic::not_evaluated();

        assert_eq!(timing.decision, ReachTimingDecision::ReachNow);
        assert_eq!(timing.reason, ReachTimingReason::NotPermanentFuriten);
        assert_eq!(timing.reach_now, None);
        assert_eq!(timing.defer_forced_reach, None);
        assert!(!timing.defers_reach());
    }

    #[test]
    fn only_a_confirmed_permanent_furiten_named_yakuman_becomes_a_categorical_damaten() {
        assert!(evaluates_named_yakuman_damaten(PermanentFuriten::Yes, 1));
        assert!(selects_named_yakuman_damaten(
            PermanentFuriten::Yes,
            1,
            true
        ));

        // 恒常フリテンが確定していない聴牌と、生きた待ちが1枚も無い聴牌は対象外。unknown を
        // 恒常フリテンだと推測しない。
        for facts in [
            (PermanentFuriten::No, 8),
            (PermanentFuriten::Unknown, 8),
            (PermanentFuriten::Yes, 0),
        ] {
            assert!(
                !evaluates_named_yakuman_damaten(facts.0, facts.1),
                "{facts:?}"
            );
            assert!(
                !selects_named_yakuman_damaten(facts.0, facts.1, true),
                "{facts:?}"
            );
        }
    }

    #[test]
    fn an_unestablished_named_yakuman_keeps_the_existing_base_policy() {
        // 一部だけ役満・数え役満・scoring unknown はどれも確定しない扱いで、categorical rule へ
        // 入れない。恒常フリテンでも既存判断のままになる。
        assert!(!selects_named_yakuman_damaten(
            PermanentFuriten::Yes,
            8,
            false
        ));
        assert_eq!(
            decide_reach_reason(true, None, REACH_MIN_REMAINING),
            ReachDecisionReason::Eligible
        );
    }

    #[test]
    fn only_the_eligible_reasons_select_reach() {
        for reason in [
            ReachDecisionReason::Eligible,
            ReachDecisionReason::EligibleNoDamatenYaku,
            ReachDecisionReason::EligibleLowValue,
        ] {
            assert!(reason.selects_reach());
        }

        for reason in [
            ReachDecisionReason::HighValueDamaten,
            ReachDecisionReason::NamedYakumanDamaten,
            ReachDecisionReason::NoLegalReach,
            ReachDecisionReason::NoSelectedDiscard,
            ReachDecisionReason::NotTenpai,
            ReachDecisionReason::NoLiveWait,
            ReachDecisionReason::InsufficientLiveWait,
        ] {
            assert!(!reason.selects_reach());
        }
    }
}
