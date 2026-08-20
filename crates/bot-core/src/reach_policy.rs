//! 攻撃を継続するテンパイで Reach / Damaten のどちらを選ぶかを決める policy 層。
//!
//! リーチ判断そのもの ([`crate::agents::ShantenAgent`] の Reach action 選択) と、押し引きが
//! 攻撃打点を求めるときの攻撃モード判定は、同じ結論でなければならない。両者が同じ条件を別々に
//! 書くと片方だけがずれるため、条件は [`decide_reach_reason`] 1本だけが持つ。
//!
//! この層は待ち・フリテン・ダマ打点を計算しない。既にそれぞれの source of truth から求めた
//! 結論 (合法 Reach の有無・[`DamatenValueVerdict`]・生きた待ちの残枚数) を受け取り、そこから
//! 理由を1つ選ぶだけの pure helper になっている。

use crate::damaten_value::DamatenValueVerdict;

// 補正後の待ち枚数がこの枚数以上ならリーチする。生牌の単騎は3枚なので、待ち枚数だけを理由に
// 抑制するのは3枚未満に限る。
pub const REACH_MIN_REMAINING: u8 = 3;

/// リーチを採用した / しなかった理由。
///
/// `Eligible*` 以外はすべて「今回はリーチしない」理由であり、最初に落ちた条件を1つだけ表す。
/// 判定順は [`ReachDecisionDiagnostic`](crate::agents::ReachDecisionDiagnostic) のフィールドが
/// 埋まる順と一致する。
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

#[cfg(test)]
mod tests {
    use super::*;

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
