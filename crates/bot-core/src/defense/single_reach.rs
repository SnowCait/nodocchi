use std::cmp::Ordering;

use crate::action::LegalAction;
use crate::context::GameContext;
use bot_logic::TileType;

use super::{CompressedHiddenHandStates, RonRiskEvidence};

/// 単独リーチ者に対する合法 Dahai 1件の exact structural risk evidence。
///
/// 同じ `TileType` の赤5 / 黒5は同じ evidence を共有する。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DahaiRonRiskEvidence<'a> {
    pub action: &'a LegalAction,
    pub evidence: RonRiskEvidence,
}

/// 1候補牌について、指定リーチ者1人から得た exact structural risk evidence。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlayerRonRiskEvidence {
    pub player: usize,
    pub evidence: RonRiskEvidence,
}

/// 合法 Dahai 1件について、全リーチ者を個別評価した exact risk vector。
///
/// `player_evidence` は player id 順で保持する。production comparator はこの順序を意味に使わず、
/// 各 `R/T` を exact に比較して worst-first へ並べてから辞書順比較する。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DahaiRonRiskVector<'a> {
    pub action: &'a LegalAction,
    pub player_evidence: Vec<PlayerRonRiskEvidence>,
}

/// 指定したリーチ者について、全合法 Dahai を exact `R/T` evidence へ変換する。
///
/// `CompressedHiddenHandStates` は1回だけ構築し、同じ `TileType` も1回だけ評価する。exact model
/// が unsupported、`T == 0`、または model invariant と矛盾する場合は `None` を返す。
pub fn reached_player_dahai_actions_by_ron_risk<'a>(
    player: usize,
    context: &GameContext,
    legal_actions: &'a [LegalAction],
) -> Option<Vec<DahaiRonRiskEvidence<'a>>> {
    let mut states = CompressedHiddenHandStates::new(player, context).ok()?;
    let tenpai_weight = states.tenpai_state_weight().weight;
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
                let evidence = states.ron_risk_evidence(tile_type);
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
        evaluated.push(DahaiRonRiskEvidence { action, evidence });
    }
    Some(evaluated)
}

/// 単独リーチ者について、全合法 Dahai を exact `R/T` evidence へ変換する。
///
/// `CompressedHiddenHandStates` は1回だけ構築し、同じ `TileType` も1回だけ評価する。exact model
/// が unsupported、`T == 0`、または model invariant と矛盾する場合は `None` を返す。
pub fn single_reach_dahai_actions_by_ron_risk<'a>(
    player: usize,
    context: &GameContext,
    legal_actions: &'a [LegalAction],
) -> Option<Vec<DahaiRonRiskEvidence<'a>>> {
    if context.reached_opponents().as_slice() != [player] {
        return None;
    }

    reached_player_dahai_actions_by_ron_risk(player, context, legal_actions)
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
    if reached.is_empty() {
        return None;
    }

    let mut vectors: Vec<_> = legal_actions
        .iter()
        .filter(|action| matches!(action, LegalAction::Dahai { .. }))
        .map(|action| DahaiRonRiskVector {
            action,
            player_evidence: Vec::with_capacity(reached.len()),
        })
        .collect();

    for player in reached {
        let evaluated = reached_player_dahai_actions_by_ron_risk(player, context, legal_actions)?;
        if evaluated.len() != vectors.len() {
            return None;
        }
        for (vector, candidate) in vectors.iter_mut().zip(evaluated) {
            if vector.action != candidate.action {
                return None;
            }
            vector.player_evidence.push(PlayerRonRiskEvidence {
                player,
                evidence: candidate.evidence,
            });
        }
    }

    Some(vectors)
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
