mod compressed_hidden_hand_states;
mod diagnostic;
mod hard_safety;
mod hidden_hand_states;
mod honor;
mod single_reach;
mod suited;
mod suji;
mod wait_candidates;
mod wall;

#[cfg(test)]
mod tests;

use crate::action::{LegalAction, prefer_black_five_for_action};
use crate::context::GameContext;
use bot_logic::TileType;

pub use compressed_hidden_hand_states::{
    CompressedHiddenHandStateMetrics, CompressedHiddenHandStates, RonRiskEvidence,
    TenpaiStateWeight, compressed_ron_capable_hidden_hand_weight,
    compressed_tenpai_hidden_hand_weight,
};
pub(crate) use diagnostic::log_defense_fallback_evaluation;
pub use diagnostic::{
    DefenseCandidateDiagnostic, DefenseDecisionDiagnostic, DefenseFallbackDiagnostic,
    log_defense_fallback_decision,
};
pub use hard_safety::{
    genbutsu_dahai_actions_for_all_reached, is_discarded_by_all_players, is_discarded_by_player,
    is_genbutsu_for, is_genbutsu_for_all_reached, select_genbutsu_fallback_action,
};
pub use hidden_hand_states::{
    HiddenHandStateMetrics, HiddenHandStateUnsupported, ReachedHiddenHandStates,
    RonCapableStateWeight, ron_capable_hidden_hand_weight,
};
pub use honor::{
    HonorSafetyRank, OpponentHonorValue, honor_dahai_actions_by_safety,
    honor_dahai_actions_by_safety_with, honor_safety_rank, opponent_honor_value_for,
    opponent_honor_value_for_players, opponent_honor_value_for_reached,
    select_honor_safety_fallback_action,
};
pub use single_reach::{DahaiRonRiskEvidence, single_reach_dahai_actions_by_ron_risk};
pub use suited::{
    SuitedSafetyEvidence, SuitedSafetyRank, select_suited_safety_fallback_action,
    suited_dahai_actions_by_safety, suited_dahai_actions_by_safety_with,
    suited_safety_evidence_for_all_reached, suited_safety_evidence_for_any_reached,
    suited_safety_evidence_for_players, suited_safety_outweighs_honor,
    suited_safety_rank_for_all_reached, suited_safety_rank_for_any_reached,
    suited_safety_rank_for_players,
};
pub use suji::{
    SujiSafetyRank, is_suji_for, is_suji_for_all_reached, is_suji_for_any_reached,
    suji_dahai_actions_by_safety, suji_safety_rank_for, suji_safety_rank_for_all_reached,
    suji_safety_rank_for_any_reached, suji_safety_rank_for_players,
};
pub use wait_candidates::{
    remaining_tile_copies, shanpon_remaining_combinations,
    shanpon_remaining_combinations_for_player, tanki_remaining_candidates,
    tanki_remaining_candidates_for_player,
};
pub use wall::{
    SequenceWaitRoute, SequenceWaitShape, WallRank, is_no_chance, is_one_chance,
    sequence_route_remaining_combinations, sequence_route_remaining_combinations_for_player,
    sequence_wait_routes, wall_rank, wall_tile_types_by_rank,
};

// visible_tiles 中で同じ TileType の枚数を数える。赤5も通常5と同じ TileType として数える。
pub fn visible_count_of(tile: TileType, context: &GameContext) -> u8 {
    context
        .visible_tiles()
        .iter()
        .filter(|visible| visible.tile_type() == tile)
        .count() as u8
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DefenseFallbackKind {
    Genbutsu,
    ExactRonRisk,
    HonorSafety(HonorSafetyRank),
    SuitedSafety(SuitedSafetyRank),
}

#[derive(Debug)]
pub(crate) struct DefenseFallbackEvaluation<'a> {
    pub(crate) selected: Option<(&'a LegalAction, DefenseFallbackKind)>,
    pub(crate) ron_risk_evidence: Option<Vec<DahaiRonRiskEvidence<'a>>>,
}

// 他家リーチ中の防御 fallback を優先順位付きで選ぶ。
// 全リーチ者への共通現物を最優先にし、単独リーチでは全 Dahai を exact R の昇順、複数
// リーチでは従来の字牌 / 数牌 safety で比較して、選ばれた種別を添えて返す。
//
// 現物は黒5対応済みの select_genbutsu_fallback_action をそのまま利用する。exact / legacy
// いずれも牌種を決めてから prefer_black_five_for_action で同じ5牌種の黒牌へ正規化する。
pub fn select_defense_fallback_action_with_kind<'a>(
    context: &GameContext,
    legal_actions: &'a [LegalAction],
) -> Option<(&'a LegalAction, DefenseFallbackKind)> {
    evaluate_defense_fallback_action_with_kind(context, legal_actions, false).selected
}

pub(crate) fn evaluate_defense_fallback_action_with_kind<'a>(
    context: &GameContext,
    legal_actions: &'a [LegalAction],
    collect_exact_evidence_for_genbutsu: bool,
) -> DefenseFallbackEvaluation<'a> {
    let reached = context.reached_opponents();
    let genbutsu = select_genbutsu_fallback_action(context, legal_actions);
    if let Some(action) = genbutsu
        && (!collect_exact_evidence_for_genbutsu || reached.len() != 1)
    {
        return DefenseFallbackEvaluation {
            selected: Some((action, DefenseFallbackKind::Genbutsu)),
            ron_risk_evidence: None,
        };
    }

    let ron_risk_evidence = reached
        .first()
        .filter(|_| reached.len() == 1)
        .and_then(|&player| single_reach_dahai_actions_by_ron_risk(player, context, legal_actions));

    if let Some(action) = genbutsu {
        return DefenseFallbackEvaluation {
            selected: Some((action, DefenseFallbackKind::Genbutsu)),
            ron_risk_evidence,
        };
    }

    if let Some(evidence) = ron_risk_evidence.as_ref()
        && let Some(chosen) = evidence
            .iter()
            .min_by_key(|candidate| candidate.evidence.ron_capable_weight)
    {
        let action = prefer_black_five_for_action(legal_actions, chosen.action);
        return DefenseFallbackEvaluation {
            selected: Some((action, DefenseFallbackKind::ExactRonRisk)),
            ron_risk_evidence,
        };
    }

    DefenseFallbackEvaluation {
        selected: select_legacy_defense_fallback_action_with_kind(context, legal_actions),
        ron_risk_evidence: None,
    }
}

// 複数リーチ、または単独リーチの exact model が unavailable な場合の従来 selection。
fn select_legacy_defense_fallback_action_with_kind<'a>(
    context: &GameContext,
    legal_actions: &'a [LegalAction],
) -> Option<(&'a LegalAction, DefenseFallbackKind)> {
    if context.any_opponent_reached() {
        let honor = honor_dahai_actions_by_safety(legal_actions, context)
            .into_iter()
            .next();
        let suited = suited_dahai_actions_by_safety(legal_actions, context)
            .into_iter()
            .find(|(_, rank)| *rank != SuitedSafetyRank::NoSafety);

        if let (Some((honor_action, honor_rank)), Some((suited_action, suited_rank))) =
            (honor, suited)
            && let LegalAction::Dahai { tile: honor_tile } = honor_action
            && suited_safety_outweighs_honor(
                honor_rank,
                opponent_honor_value_for_reached(honor_tile.tile_type(), context),
                suited_rank,
            )
        {
            let action = prefer_black_five_for_action(legal_actions, suited_action);
            return Some((action, DefenseFallbackKind::SuitedSafety(suited_rank)));
        }

        if let Some((action, rank)) = honor {
            let action = prefer_black_five_for_action(legal_actions, action);
            return Some((action, DefenseFallbackKind::HonorSafety(rank)));
        }

        if let Some((action, rank)) = suited {
            let action = prefer_black_five_for_action(legal_actions, action);
            return Some((action, DefenseFallbackKind::SuitedSafety(rank)));
        }
    }

    None
}

// 防御 fallback の action だけを返す薄い wrapper。
pub fn select_defense_fallback_action<'a>(
    context: &GameContext,
    legal_actions: &'a [LegalAction],
) -> Option<&'a LegalAction> {
    select_defense_fallback_action_with_kind(context, legal_actions).map(|(action, _)| action)
}
