use std::cmp::Ordering;

use crate::action::LegalAction;
use crate::context::GameContext;
use bot_logic::TileType;

use super::{
    CompressedHiddenHandStates, CompressedStructuralTenpaiHiddenHandStates, RonRiskEvidence,
};

/// 1候補牌について、指定 target 1人から得た exact `R/T` evidence。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlayerRonRiskEvidence {
    pub player: usize,
    pub evidence: RonRiskEvidence,
}

/// 合法 Dahai 1件について、全 target を個別評価した exact risk vector。
///
/// 単独リーチも `player_evidence` が1要素の risk vector として同じ表現で扱う。同じ `TileType`
/// の赤5 / 黒5は同じ evidence を共有する。`player_evidence` は player id 順で保持する。
/// production comparator はこの順序を意味に使わず、各 `R/T` を exact に比較して worst-first へ
/// 並べてから辞書順比較する。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DahaiRonRiskVector<'a> {
    pub action: &'a LegalAction,
    pub player_evidence: Vec<PlayerRonRiskEvidence>,
}

pub(crate) fn player_ron_risk_evidence_for_action<'a>(
    vectors: Option<&'a [DahaiRonRiskVector<'_>]>,
    action: &LegalAction,
) -> Option<&'a [PlayerRonRiskEvidence]> {
    let LegalAction::Dahai { tile } = action else {
        return None;
    };
    vectors?
        .iter()
        .find(|candidate| {
            matches!(candidate.action, LegalAction::Dahai { tile: candidate_tile } if candidate_tile.tile_type() == tile.tile_type())
        })
        .map(|candidate| candidate.player_evidence.as_slice())
}

/// 指定したリーチ者について、全合法 Dahai を exact `R/T` evidence へ変換する。
///
/// 戻り値は `legal_actions` 中の Dahai と同じ順序で並ぶ。`CompressedHiddenHandStates` は1回だけ
/// 構築し、同じ `TileType` も1回だけ評価する。exact model が unsupported、`T == 0`、または model
/// invariant と矛盾する場合は `None` を返す。
pub(super) fn dahai_ron_risk_evidence_for_player(
    player: usize,
    context: &GameContext,
    legal_actions: &[LegalAction],
) -> Option<Vec<PlayerRonRiskEvidence>> {
    let mut states = CompressedHiddenHandStates::new(player, context).ok()?;
    let tenpai_weight = states.tenpai_state_weight().weight;
    collect_dahai_ron_risk_evidence(player, legal_actions, tenpai_weight, |tile| {
        Some(states.ron_risk_evidence(tile))
    })
}

/// 指定した非リーチ副露 target について、全合法 Dahai を exact `R/T` evidence へ変換する。
fn open_hand_dahai_ron_risk_evidence_for_player(
    player: usize,
    context: &GameContext,
    legal_actions: &[LegalAction],
) -> Option<Vec<PlayerRonRiskEvidence>> {
    let mut states = CompressedStructuralTenpaiHiddenHandStates::new(player, context).ok()?;
    let tenpai_weight = states.tenpai_state_weight().weight;
    collect_dahai_ron_risk_evidence(player, legal_actions, tenpai_weight, |tile| {
        states.ron_risk_evidence(tile).ok()
    })
}

fn collect_dahai_ron_risk_evidence(
    player: usize,
    legal_actions: &[LegalAction],
    tenpai_weight: u128,
    mut evidence_for_tile: impl FnMut(TileType) -> Option<RonRiskEvidence>,
) -> Option<Vec<PlayerRonRiskEvidence>> {
    if tenpai_weight == 0 {
        return None;
    }

    let mut by_tile = [None; TileType::COUNT];
    let mut evaluated = Vec::new();
    for action in legal_actions {
        let LegalAction::Dahai { tile } = action else {
            continue;
        };
        let tile_type = tile.tile_type();
        let evidence = match by_tile[tile_type.index()] {
            Some(evidence) => evidence,
            None => {
                let evidence = evidence_for_tile(tile_type)?;
                // All targets share this player's denominator. Treat an internal invariant
                // mismatch or an unavailable exact ratio as unavailable instead of clamping it.
                if evidence.tenpai_weight != tenpai_weight
                    || evidence.ron_capable_weight > tenpai_weight
                    || evidence.compare_ratio(&evidence) != Some(Ordering::Equal)
                {
                    return None;
                }
                by_tile[tile_type.index()] = Some(evidence);
                evidence
            }
        };
        evaluated.push(PlayerRonRiskEvidence { player, evidence });
    }
    Some(evaluated)
}

/// 全リーチ者を既存 single-player exact model で個別評価する。
///
/// player ごとに `CompressedHiddenHandStates` を1回だけ構築する。1人でも unavailable なら exact
/// と legacy を混在させず、局面全体を unavailable として `None` を返す。
pub(crate) fn reached_opponents_dahai_actions_by_ron_risk<'a>(
    context: &GameContext,
    legal_actions: &'a [LegalAction],
) -> Option<Vec<DahaiRonRiskVector<'a>>> {
    let reached = context.reached_opponents();
    dahai_actions_by_ron_risk(&reached, legal_actions, |player| {
        dahai_ron_risk_evidence_for_player(player, context, legal_actions)
    })
}

/// 全 OpenHand target を conditional-tenpai exact model で個別評価する。
///
/// 1人でも model unavailable なら exact と heuristic を混在させず `None` を返す。
pub(crate) fn open_hand_targets_dahai_actions_by_ron_risk<'a>(
    context: &GameContext,
    legal_actions: &'a [LegalAction],
    targets: &[usize],
) -> Option<Vec<DahaiRonRiskVector<'a>>> {
    dahai_actions_by_ron_risk(targets, legal_actions, |player| {
        open_hand_dahai_ron_risk_evidence_for_player(player, context, legal_actions)
    })
}

/// Riichi target と High OpenHand target を、それぞれ既存の exact model で評価して同じ
/// [`DahaiRonRiskVector`] へ積む。
///
/// target kind の分類は呼び出し側の責務とし、ここでは席一覧だけを受け取る。どちらかの一覧が空、
/// または1人でも model unavailable なら partial vector を返さず `None` にする。
pub(crate) fn combined_targets_dahai_actions_by_ron_risk<'a>(
    context: &GameContext,
    legal_actions: &'a [LegalAction],
    riichi_targets: &[usize],
    open_hand_targets: &[usize],
) -> Option<Vec<DahaiRonRiskVector<'a>>> {
    if riichi_targets.is_empty() || open_hand_targets.is_empty() {
        return None;
    }

    let mut vectors = dahai_actions_by_ron_risk(riichi_targets, legal_actions, |player| {
        dahai_ron_risk_evidence_for_player(player, context, legal_actions)
    })?;
    append_players_to_ron_risk_vectors(&mut vectors, open_hand_targets, |player| {
        open_hand_dahai_ron_risk_evidence_for_player(player, context, legal_actions)
    })?;

    // The two input lists are grouped by kind, while diagnostics promise seat order.
    for vector in &mut vectors {
        vector
            .player_evidence
            .sort_by_key(|evidence| evidence.player);
    }
    Some(vectors)
}

fn dahai_actions_by_ron_risk<'a>(
    targets: &[usize],
    legal_actions: &'a [LegalAction],
    mut evidence_for_player: impl FnMut(usize) -> Option<Vec<PlayerRonRiskEvidence>>,
) -> Option<Vec<DahaiRonRiskVector<'a>>> {
    if targets.is_empty() {
        return None;
    }

    let mut vectors: Vec<_> = legal_actions
        .iter()
        .filter(|action| matches!(action, LegalAction::Dahai { .. }))
        .map(|action| DahaiRonRiskVector {
            action,
            player_evidence: Vec::with_capacity(targets.len()),
        })
        .collect();

    append_players_to_ron_risk_vectors(&mut vectors, targets, &mut evidence_for_player)?;

    Some(vectors)
}

fn append_players_to_ron_risk_vectors(
    vectors: &mut [DahaiRonRiskVector<'_>],
    targets: &[usize],
    mut evidence_for_player: impl FnMut(usize) -> Option<Vec<PlayerRonRiskEvidence>>,
) -> Option<()> {
    for &player in targets {
        let evaluated = evidence_for_player(player)?;
        if evaluated.len() != vectors.len() {
            return None;
        }
        for (vector, evidence) in vectors.iter_mut().zip(evaluated) {
            vector.player_evidence.push(evidence);
        }
    }
    Some(())
}

/// 2候補の opponent risk vector を worst-first の辞書順で exact 比較する。
///
/// `Less` は `left` の minimax risk vector が小さく、より安全であることを表す。比率の比較には
/// [`RonRiskEvidence::compare_ratio`] だけを使い、1つでも unavailable なら `None` を返す。
pub fn compare_lexicographic_minimax_ron_risk(
    left: &[PlayerRonRiskEvidence],
    right: &[PlayerRonRiskEvidence],
) -> Option<Ordering> {
    if left.is_empty() || left.len() != right.len() {
        return None;
    }
    if player_mask(left)? != player_mask(right)? {
        return None;
    }

    let left = worst_first(left)?;
    let right = worst_first(right)?;
    for (left, right) in left.into_iter().zip(right) {
        let ordering = left.evidence.compare_ratio(&right.evidence)?;
        if ordering != Ordering::Equal {
            return Some(ordering);
        }
    }
    Some(Ordering::Equal)
}

fn player_mask(evidence: &[PlayerRonRiskEvidence]) -> Option<u8> {
    let mut mask = 0u8;
    for candidate in evidence {
        if candidate.player >= 4 {
            return None;
        }
        let bit = 1u8 << candidate.player;
        if mask & bit != 0 {
            return None;
        }
        mask |= bit;
    }
    Some(mask)
}

fn worst_first(evidence: &[PlayerRonRiskEvidence]) -> Option<Vec<&PlayerRonRiskEvidence>> {
    let mut sorted: Vec<_> = evidence.iter().collect();
    for candidate in &sorted {
        if candidate.evidence.compare_ratio(&candidate.evidence) != Some(Ordering::Equal) {
            return None;
        }
    }

    // At most three opponents are compared, so a small fallible insertion sort keeps every
    // comparison exact and propagates `compare_ratio()` unavailability. Equal ratios are stabilized
    // by player id only for diagnostics.
    for index in 1..sorted.len() {
        let mut current = index;
        while current > 0 {
            let ordering = sorted[current]
                .evidence
                .compare_ratio(&sorted[current - 1].evidence)?;
            let should_swap = ordering == Ordering::Greater
                || (ordering == Ordering::Equal
                    && sorted[current].player < sorted[current - 1].player);
            if !should_swap {
                break;
            }
            sorted.swap(current, current - 1);
            current -= 1;
        }
    }
    Some(sorted)
}
