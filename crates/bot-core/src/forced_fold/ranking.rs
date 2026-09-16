//! forced fold evaluation が観察する防御候補の ranking。
//!
//! 順序は既存 production defense policy そのままで、ここでは comparator を持たない。段の順序・
//! category precedence・exact `R/T` comparator・multi-target lexicographic minimax・heuristic
//! fallback ordering・tie-break・合法 action 順はすべて既存 selector と共有する
//! `ordered_*_candidates` helper が決める。
//!
//! 全合法 Dahai を同じ ranked candidate として扱い、rank 1 だけを別扱いしない。
//! `ranked_candidates[0].action == selected_action` が成り立つのは、ranking 全体が既存
//! production selector と同じ ordering だからで、rank 1 用の特別処理はない。

use crate::action::LegalAction;
use crate::combined_defense::CombinedDefenseCategory;
use crate::defense::{
    DefenseFallbackKind, PlayerRonRiskEvidence, RonRiskEvidence, worst_first_ron_risk_evidence,
};
use crate::open_hand_defense::OpenHandDefenseCategory;

use super::ForcedFoldDefenseKind;

/// production ordering 上の防御候補1件。
///
/// 全候補が同じ形式で、action・production rank・defense kind / category・hard-safe か・exact
/// ron-risk evidence・heuristic evidence を表す。0-risk candidate を Summary へ追加表示するために
/// 順位を付け替えないので、`rank` は常に production ordering 上の順位。
///
/// `action` は牌種ごとに1件で、同じ牌種の赤5 / 黒5は既存 selector と同じ
/// `prefer_black_five_for_action` 正規化で黒5に寄せる。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ForcedFoldRankedCandidate {
    /// 対象の合法 Dahai。
    pub action: LegalAction,
    /// production ordering 上の順位。1 始まり。
    pub rank: usize,
    /// production ordering がこの候補へ与えた defense family と family 内の種別。
    pub defense_kind: ForcedFoldDefenseKind,
    /// exact model が利用可能な場合の target 別 `R/T` evidence。席順で持つ。
    ///
    /// production evaluation が実際に構築した evidence をそのまま写したもので、ranking や
    /// Summary のために exact model を再計算しない。
    pub player_ron_risk_evidence: Option<Vec<PlayerRonRiskEvidence>>,
}

impl ForcedFoldRankedCandidate {
    /// 既存 policy 上、全 defense target からロンされないと確定しているか。
    ///
    /// 根拠は Reach 防御の全リーチ者共通現物、OpenHand 防御の `SafeAgainstAllTargets`、複合
    /// threat 防御の `SafeAgainstAllThreats` だけ。exact model 上の `R == 0` はここに含めない。
    pub fn is_hard_safe(&self) -> bool {
        matches!(
            self.defense_kind,
            ForcedFoldDefenseKind::Reach(DefenseFallbackKind::Genbutsu)
                | ForcedFoldDefenseKind::OpenHand(OpenHandDefenseCategory::SafeAgainstAllTargets)
                | ForcedFoldDefenseKind::Combined(CombinedDefenseCategory::SafeAgainstAllThreats)
        )
    }

    /// exact evidence が利用可能で、全 target について `ron_capable_weight == 0` か。
    ///
    /// 判定は integer evidence だけで行い、表示上の percentage は使わない。`R = 1`、`T = 50000`
    /// のように表示が `0.00%` になる候補も `R > 0` なので `false`。複数 target では全 player の
    /// `R == 0` だけを 0-risk とし、一部 target だけ `R == 0` の候補は含めない。
    pub fn has_exact_zero_ron_risk(&self) -> bool {
        self.player_ron_risk_evidence
            .as_ref()
            .is_some_and(|evidence| {
                !evidence.is_empty()
                    && evidence
                        .iter()
                        .all(|target| target.evidence.ron_capable_weight == 0)
            })
    }

    /// 既存の確定 fact / integer evidence 上、ロンされないと言える候補か。
    ///
    /// hard-safe と exact `R == 0` のどちらかが成り立つ場合だけ `true`。3枚見えの字牌のような
    /// heuristic safety はここに含めない。
    pub fn is_zero_risk(&self) -> bool {
        self.is_hard_safe() || self.has_exact_zero_ron_risk()
    }

    /// target が1人の場合の exact `R/T` evidence。
    ///
    /// 複数 target では `None` で、player ごとの evidence は
    /// [`worst_first_player_ron_risk_evidence`](Self::worst_first_player_ron_risk_evidence)
    /// から取る。
    pub fn ron_risk_evidence(&self) -> Option<RonRiskEvidence> {
        let &[evidence] = self.player_ron_risk_evidence.as_deref()? else {
            return None;
        };
        Some(evidence.evidence)
    }

    /// target 別 evidence を production comparator と同じ worst-first 順で返す。
    ///
    /// 並べ替えは既存 [`worst_first_ron_risk_evidence`] そのもので、表示用の別順序を作らない。
    pub fn worst_first_player_ron_risk_evidence(&self) -> Option<Vec<&PlayerRonRiskEvidence>> {
        worst_first_ron_risk_evidence(self.player_ron_risk_evidence.as_deref()?)
    }

    /// exact model を使わずに順位が決まった候補の heuristic 根拠。
    ///
    /// production ordering がその候補へ与えた種別そのもの。hard-safe な候補と exact `R/T` で
    /// 並んだ候補では `None`。
    pub fn heuristic_evidence(&self) -> Option<ForcedFoldDefenseKind> {
        if self.is_hard_safe() {
            return None;
        }
        match self.defense_kind {
            ForcedFoldDefenseKind::Reach(DefenseFallbackKind::ExactRonRisk)
            | ForcedFoldDefenseKind::OpenHand(OpenHandDefenseCategory::ExactRonRisk)
            | ForcedFoldDefenseKind::Combined(CombinedDefenseCategory::ExactRonRisk) => None,
            kind => Some(kind),
        }
    }
}
