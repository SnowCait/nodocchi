use std::cmp::Ordering;

use bot_logic::TileType;
use bot_logic::tile::TileTypeSet;

use crate::action::LegalAction;
use crate::context::GameContext;
use crate::defense::{RonRiskEvidence, dahai_ron_risk_evidence_for_player};
use crate::push_pull::PushPullInputs;

const IISHANTEN: i8 = 1;

/// 他家リーチ者がちょうど1人 (Combined threat ではない) の局面で、production の通常打牌後が
/// ちょうど1向聴のときに求める exact ron-risk の観測値。
///
/// 将来の1向聴 Push/Fold policy を検討するための観測値で、現在の [`decide_push_pull`] も action
/// 選択もこの値を読まない。exact model は Reach Defense と同じ
/// [`CompressedHiddenHandStates`](crate::defense::CompressedHiddenHandStates) と
/// [`RonRiskEvidence`] をそのまま使い、別の risk model を持たない。`R/T` は実放銃確率ではなく
/// structural risk evidence で、source of truth は整数の `R` / `T`。
///
/// [`decide_push_pull`]: crate::push_pull::decide_push_pull
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IishantenReachRonRisk {
    /// 評価したリーチ者の席。
    pub reacher: usize,
    /// production が選んだ通常打牌の牌種。赤5 / 黒5 は同じ牌種として扱う。
    pub selected_discard: TileType,
    /// exact model で評価できた場合の summary。unsupported state・`T == 0`・model invariant との
    /// 矛盾・exact ratio comparison 不能のいずれかなら `None` (unavailable) で、スジや壁などから
    /// 値を補完しない。
    pub exact: Option<IishantenReachRonRiskSummary>,
}

/// 打牌後1向聴を維持する合法打牌候補の exact `R/T` の compact summary。
///
/// 候補の単位は牌種で、赤5 / 黒5 は1候補として同じ evidence を共有する。比率の比較はすべて
/// [`RonRiskEvidence::compare_ratio`] で行い、浮動小数点を使わない。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IishantenReachRonRiskSummary {
    /// production が選んだ通常打牌の evidence。
    pub selected: RonRiskEvidence,
    /// 1向聴候補の中で `R/T` が最小の evidence。同率の候補はどれも同じ比率を表す。
    pub minimum: RonRiskEvidence,
    /// 選択打牌の risk 順位 (1 始まり)。`R/T` が選択打牌より厳密に小さい候補の数 + 1 で、同率の
    /// 候補は同順位になる。合法 action の並び順を risk 差として扱わない。
    pub selected_rank: usize,
    /// exact 評価できた1向聴候補の牌種数。選択打牌を含む。
    pub candidate_count: usize,
    /// 選択打牌が最小 `R/T` と同率か。
    pub selected_is_minimum: bool,
}

impl IishantenReachRonRisk {
    pub fn is_exact_available(&self) -> bool {
        self.exact.is_some()
    }
}

/// 単独リーチ × 打牌後1向聴の局面だけ、1向聴候補の exact `R/T` summary を求める。
///
/// 対象外の局面 (NoThreat・OpenHandThreat 単独・Combined threat・複数リーチ・テンパイ・2向聴以上・
/// 通常打牌を選んでいない) では exact model を構築せず `None` を返す。対象局面では logging の
/// 有無にかかわらず評価する。
///
/// リーチ者1人につき既存 [`dahai_ron_risk_evidence_for_player`] を1回だけ呼び、
/// `CompressedHiddenHandStates` の構築と共通の `T` の計算も1回で済ませる。`iishanten_discards` は
/// 通常打牌選択が評価した合法打牌候補のうち、打牌後がちょうど1向聴になる牌種。
pub(crate) fn iishanten_reach_ron_risk(
    context: &GameContext,
    inputs: &PushPullInputs,
    selected_normal_discard: Option<&LegalAction>,
    iishanten_discards: TileTypeSet,
    legal_actions: &[LegalAction],
) -> Option<IishantenReachRonRisk> {
    if !inputs.is_single_reach_threat() || inputs.offense?.min_shanten_after_discard != IISHANTEN {
        return None;
    }
    let Some(LegalAction::Dahai { tile }) = selected_normal_discard else {
        return None;
    };
    let reacher = inputs
        .player_threats
        .iter()
        .find(|facts| facts.is_reached_opponent())?
        .player;
    let selected_discard = tile.tile_type();

    Some(IishantenReachRonRisk {
        reacher,
        selected_discard,
        exact: exact_summary(
            context,
            reacher,
            selected_discard,
            iishanten_discards,
            legal_actions,
        ),
    })
}

fn exact_summary(
    context: &GameContext,
    reacher: usize,
    selected_discard: TileType,
    iishanten_discards: TileTypeSet,
    legal_actions: &[LegalAction],
) -> Option<IishantenReachRonRiskSummary> {
    if !iishanten_discards.contains(selected_discard) {
        return None;
    }
    let candidates: Vec<LegalAction> = legal_actions
        .iter()
        .filter(|action| {
            matches!(action, LegalAction::Dahai { tile } if iishanten_discards.contains(tile.tile_type()))
        })
        .cloned()
        .collect();
    let evidence = dahai_ron_risk_evidence_for_player(reacher, context, &candidates)?;
    if evidence.len() != candidates.len() {
        return None;
    }

    let mut seen = TileTypeSet::new();
    let mut by_tile_type = Vec::with_capacity(candidates.len());
    for (action, evidence) in candidates.iter().zip(evidence) {
        let LegalAction::Dahai { tile } = action else {
            return None;
        };
        if !seen.contains(tile.tile_type()) {
            seen.insert(tile.tile_type());
            by_tile_type.push((tile.tile_type(), evidence.evidence));
        }
    }
    summarize_iishanten_ron_risk(selected_discard, &by_tile_type)
}

/// 牌種ごとの exact evidence から、選択打牌の順位と最小 `R/T` を exact ratio comparison で求める。
///
/// 1組でも比較不能なら値を推測せず `None` を返す。
pub(crate) fn summarize_iishanten_ron_risk(
    selected_discard: TileType,
    candidates: &[(TileType, RonRiskEvidence)],
) -> Option<IishantenReachRonRiskSummary> {
    let selected = candidates
        .iter()
        .find(|(tile_type, _)| *tile_type == selected_discard)?
        .1;

    let mut minimum = selected;
    let mut strictly_safer = 0;
    for (_, evidence) in candidates {
        if evidence.compare_ratio(&selected)? == Ordering::Less {
            strictly_safer += 1;
        }
        if evidence.compare_ratio(&minimum)? == Ordering::Less {
            minimum = *evidence;
        }
    }

    Some(IishantenReachRonRiskSummary {
        selected,
        minimum,
        selected_rank: strictly_safer + 1,
        candidate_count: candidates.len(),
        selected_is_minimum: selected.compare_ratio(&minimum)? == Ordering::Equal,
    })
}

#[cfg(test)]
mod tests;
