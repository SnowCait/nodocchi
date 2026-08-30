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
                // mismatch as unavailable instead of clamping it or ordering invalid evidence.
                if evidence.tenpai_weight != tenpai_weight
                    || evidence.ron_capable_weight > tenpai_weight
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
