//! forced fold evaluation が観察する防御候補の ranking。
//!
//! 基礎となる順序は既存 production defense policy そのままで、ここでは comparator を作らない。
//! 段の順序・category precedence・exact `R/T` comparator・multi-target lexicographic minimax・
//! heuristic fallback ordering・tie-break・合法 action 順はすべて既存 selector と共有する
//! `ordered_*_candidates` helper が決める。
//!
//! そのうえで forced fold だけは、手牌内の同一牌枚数を織り込んだ
//! [`effective_fold_risk`] で exact 段を並べ替える。同一牌を複数枚持つ場合、1枚目が通った事実に
//! よって次巡以降の同一牌の安全性が高まる continuation value を簡易的に近似する heuristic で、
//! defense target ごとの safety evidence の寿命を厳密にモデル化したものではない
//! ([`effective_fold_risk`] に前提を書く)。production defense policy の ordering も
//! exact `R/T` 自体も書き換えない。
//!
//! 全合法 Dahai を同じ ranked candidate として扱い、rank 1 だけを別扱いしない。
//! `ranked_candidates[0].action == selected_action` が成り立つのは、forced fold の選択が
//! この ranking の先頭そのものだからで、rank 1 用の特別処理はない。

use std::cmp::Ordering;

use crate::action::LegalAction;
use crate::combined_defense::CombinedDefenseCategory;
use crate::defense::{
    DefenseFallbackKind, PlayerRonRiskEvidence, RonRiskEvidence, worst_first_ron_risk_evidence,
};
use crate::open_hand_defense::OpenHandDefenseCategory;

use super::ForcedFoldDefenseKind;

/// forced fold ordering 上の防御候補1件。
///
/// 全候補が同じ形式で、action・forced fold rank・defense kind / category・hard-safe か・exact
/// ron-risk evidence・手牌内の同一牌枚数・heuristic evidence を表す。0-risk candidate を Summary
/// へ追加表示するために順位を付け替えないので、`rank` は常に forced fold ordering 上の順位。
///
/// `action` は牌種ごとに1件で、同じ牌種の赤5 / 黒5は既存 selector と同じ
/// `prefer_black_five_for_action` 正規化で黒5に寄せる。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ForcedFoldRankedCandidate {
    /// 対象の合法 Dahai。
    pub action: LegalAction,
    /// forced fold ordering 上の順位。1 始まり。
    pub rank: usize,
    /// production ordering がこの候補へ与えた defense family と family 内の種別。
    pub defense_kind: ForcedFoldDefenseKind,
    /// exact model が利用可能な場合の target 別 `R/T` evidence。席順で持つ。
    ///
    /// production evaluation が実際に構築した evidence をそのまま写したもので、ranking や
    /// Summary のために exact model を再計算しない。
    pub player_ron_risk_evidence: Option<Vec<PlayerRonRiskEvidence>>,
    /// この候補と同じ牌種を手牌 (自摸牌を含む) に持っている枚数。1 以上。
    ///
    /// 待ち判定上同一になる牌種単位で数えるので、赤5と黒5は同じ牌種として合算する。
    pub copies: usize,
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

    /// target が1人の場合の ForcedFold 用 ranking score。
    ///
    /// exact `R/T` を [`effective_fold_risk`] で写した順位付け用の値で、実際の放銃確率では
    /// ない。複数 target では `None` で、target 別の値は
    /// [`worst_first_effective_fold_risks`](Self::worst_first_effective_fold_risks) から取る。
    pub fn effective_fold_risk(&self) -> Option<f64> {
        let evidence = self.ron_risk_evidence()?;
        Some(effective_fold_risk(
            model_risk_ratio(evidence)?,
            self.copies,
        ))
    }

    /// target 別の ForcedFold 用 ranking score を worst-first 順で返す。
    ///
    /// 並べ替えは exact `R/T` の worst-first そのもの。`copies` は候補ごとに1つなので、
    /// [`effective_fold_risk`] は単調変換であり worst-first 順は `R/T` のものと一致する。
    pub fn worst_first_effective_fold_risks(&self) -> Option<Vec<(usize, f64)>> {
        self.worst_first_player_ron_risk_evidence()?
            .into_iter()
            .map(|evidence| {
                Some((
                    evidence.player,
                    effective_fold_risk(model_risk_ratio(evidence.evidence)?, self.copies),
                ))
            })
            .collect()
    }

    // exact `R/T` で順位が決まった段の候補か。並べ替えるのはこの段だけで、hard-safe・同巡内
    // 通過・heuristic の段は段間の precedence ごと production ordering のまま残す。
    fn reranked_by_effective_fold_risk(&self) -> bool {
        !self.is_hard_safe()
            && self.heuristic_evidence().is_none()
            && self.worst_first_effective_fold_risks().is_some()
    }
}

/// ForcedFold の順位付け用 score。実際の放銃確率ではない。
///
/// `model_risk` はその牌を今1枚切った場合の model risk (`R/T`) で、その意味も値もここでは
/// 変えない。同一牌を `copies` 枚持つ場合、1枚目が通った事実によって次巡以降の同一牌の安全性が
/// 高まる continuation value を、「`copies` 巡ぶんを1回の `model_risk` でカバーできる」とみなす
/// ことで簡易的に近似する heuristic。`copies <= 1` では `model_risk` のまま。
///
/// この近似は、通過が次巡以降へどれだけ残るかを defense target ごとに区別しない。実際の
/// safety evidence の寿命は target の種別で違う。
///
/// - Reach: 通れば `post_reach_passed` としてそのリーチ者への現物になる。リーチ者の手牌は
///   変化しないので、この safety は局中継続する。
/// - OpenHand: 非リーチ副露相手の通過情報は Reach と同じ永続的な hard-safe ではない。
///   `same_hand_passed` は「target の concealed hand が最後に変化して以降に通った」ことを前提と
///   する safety evidence で、手出し・判別できない打牌・鳴き・槓で失効する。production も
///   これを hard-safe とは扱わず、exact model の `R == 0` とも扱わない。
///
/// つまりこの score は ForcedFold ranking 用の heuristic で、上の違いを厳密にモデル化した値では
/// ない。継続価値そのものの評価方式は別途検討する (issue #329)。
pub fn effective_fold_risk(model_risk: f64, copies: usize) -> f64 {
    if copies <= 1 {
        return model_risk;
    }

    1.0 - (1.0 - model_risk).powf(1.0 / copies as f64)
}

// exact evidence の R/T を比率へ写す。分母0や model 外の値では比率を推測しない。
fn model_risk_ratio(evidence: RonRiskEvidence) -> Option<f64> {
    if evidence.tenpai_weight == 0 || evidence.ron_capable_weight > evidence.tenpai_weight {
        return None;
    }
    Some(evidence.ron_capable_weight as f64 / evidence.tenpai_weight as f64)
}

/// ForcedFold 専用の並べ替え。`rank` も並べ替え後の位置で振り直す。
///
/// 動かすのは exact model risk を持つ非 hard-safe 候補だけで、その候補が占めていた位置の中で
/// だけ入れ替える。hard-safe 候補・exact model が使えない heuristic 候補は production ordering
/// 上の位置に留まるので、段の順序と段間の precedence は変わらない。
///
/// 段内の比較は worst-first の [`effective_fold_risk`] 辞書順で、全候補が `copies == 1` なら
/// [`effective_fold_risk`] は `R/T` そのものなので production ordering と同じ並びになる。
/// 比較が引き分けた場合は stable sort が production ordering を保つ。
///
/// 対象は Reach / OpenHand / Combined いずれの exact 段でも同じで、family で分けない。target
/// 種別ごとの safety evidence の違いは [`effective_fold_risk`] の前提として扱う。
pub(super) fn rerank_by_effective_fold_risk(candidates: &mut [ForcedFoldRankedCandidate]) {
    let positions: Vec<usize> = candidates
        .iter()
        .enumerate()
        .filter(|(_, candidate)| candidate.reranked_by_effective_fold_risk())
        .map(|(index, _)| index)
        .collect();

    let mut sorted = positions.clone();
    sorted.sort_by(|&left, &right| {
        compare_effective_fold_risk(&candidates[left], &candidates[right])
    });

    let reordered: Vec<ForcedFoldRankedCandidate> = sorted
        .into_iter()
        .map(|index| candidates[index].clone())
        .collect();
    for (&position, candidate) in positions.iter().zip(reordered) {
        candidates[position] = candidate;
    }

    for (index, candidate) in candidates.iter_mut().enumerate() {
        candidate.rank = index + 1;
    }
}

// worst-first の effective fold risk を辞書順で比較する。`Less` が安全側。
fn compare_effective_fold_risk(
    left: &ForcedFoldRankedCandidate,
    right: &ForcedFoldRankedCandidate,
) -> Ordering {
    let (Some(left), Some(right)) = (
        left.worst_first_effective_fold_risks(),
        right.worst_first_effective_fold_risks(),
    ) else {
        return Ordering::Equal;
    };

    for ((_, left), (_, right)) in left.into_iter().zip(right) {
        let ordering = left.partial_cmp(&right).unwrap_or(Ordering::Equal);
        if ordering != Ordering::Equal {
            return ordering;
        }
    }
    Ordering::Equal
}
