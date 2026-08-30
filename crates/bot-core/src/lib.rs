pub mod action;
pub mod agent;
pub mod agents;
pub mod call_decision;
pub mod combined_defense;
pub mod context;
pub mod damaten_value;
pub mod defense;
pub mod discard_selection;
pub mod kuikae;
pub mod meld;
pub mod offense_value;
pub mod open_hand_defense;
pub mod open_hand_threat;
pub mod prospective_value;
pub mod push_pull;
pub mod reach_policy;
pub mod threat;

pub use action::LegalAction;
pub use agent::Agent;
pub use agents::{
    AgentActionSource, DiagnosticOptions, MenzenAgent, NormalAgent, ReachDecisionDiagnostic,
    ReachDecisionReason, ShantenAgent, ShantenDecisionDiagnostic, TsumogiriAgent,
    diagnose_shanten_decision, diagnose_shanten_decision_with_options,
};
pub use call_decision::{
    CALL_CURRENT_SHANTEN, CALL_MIN_LIVE_WAIT_REMAINING, CALL_TENPAI_SHANTEN,
    CallCandidateDiagnostic, CallDecisionDiagnostic, CallDecisionReason, CallKind, CallWaitYaku,
    CallWaitYakuDiagnostic,
};
pub use combined_defense::{
    CombinedDefenseCandidateDiagnostic, CombinedDefenseCategory, CombinedDefenseDiagnostic,
    CombinedDefenseSelectionDiagnostic, CombinedDefenseTargetSafety, ThreatDefenseTarget,
    ThreatDefenseTargetKind, combined_defense_category, combined_honor_dahai_actions_by_safety,
    combined_suited_dahai_actions_by_safety, combined_threat_defense_targets,
    combined_threat_defense_targets_from_context, combined_threat_defense_targets_from_facts,
    is_ron_safe_for_target, is_safe_against_all_threats, opponent_honor_value_for_combined_threats,
    safe_against_all_threats_dahai_actions, select_combined_threat_defense_fallback_action,
    select_combined_threat_defense_fallback_action_with_kind,
    suited_safety_evidence_for_combined_threats, suited_safety_rank_for_combined_threats,
    suji_safety_rank_for_combined_threats,
};
pub use context::{GameContext, TableStateFacts, seat_wind_for_player};
pub use damaten_value::{
    DAMATEN_MIN_TOTAL, DamatenValue, DamatenValueDiagnostic, DamatenValueVerdict, DamatenWaitValue,
    DamatenWinningTileValue, damaten_baseline_context,
};
pub use defense::{
    CompressedHiddenHandStateMetrics, CompressedHiddenHandStates, DahaiRonRiskEvidence,
    DahaiRonRiskVector, DefenseCandidateDiagnostic, DefenseDecisionDiagnostic,
    DefenseFallbackDiagnostic, DefenseFallbackKind, HiddenHandStateMetrics,
    HiddenHandStateUnsupported, HonorSafetyRank, OpponentHonorValue, PlayerRonRiskEvidence,
    ReachedHiddenHandStates, RonCapableStateWeight, RonRiskEvidence, SequenceWaitRoute,
    SequenceWaitShape, SuitedSafetyEvidence, SuitedSafetyRank, SujiSafetyRank, TenpaiStateWeight,
    WallRank, compare_lexicographic_minimax_ron_risk, compressed_ron_capable_hidden_hand_weight,
    compressed_tenpai_hidden_hand_weight, genbutsu_dahai_actions_for_all_reached,
    honor_dahai_actions_by_safety, honor_dahai_actions_by_safety_with, honor_safety_rank,
    is_discarded_by_all_players, is_discarded_by_player, is_genbutsu_for,
    is_genbutsu_for_all_reached, is_no_chance, is_one_chance, is_suji_for, is_suji_for_all_reached,
    is_suji_for_any_reached, log_defense_fallback_decision, opponent_honor_value_for,
    opponent_honor_value_for_players, opponent_honor_value_for_reached,
    reached_player_dahai_actions_by_ron_risk, remaining_tile_copies,
    ron_capable_hidden_hand_weight, select_defense_fallback_action,
    select_defense_fallback_action_with_kind, select_genbutsu_fallback_action,
    select_honor_safety_fallback_action, select_suited_safety_fallback_action,
    sequence_route_remaining_combinations, sequence_route_remaining_combinations_for_player,
    sequence_wait_routes, shanpon_remaining_combinations,
    shanpon_remaining_combinations_for_player, single_reach_dahai_actions_by_ron_risk,
    suited_dahai_actions_by_safety, suited_dahai_actions_by_safety_with,
    suited_safety_evidence_for_all_reached, suited_safety_evidence_for_any_reached,
    suited_safety_evidence_for_players, suited_safety_outweighs_honor,
    suited_safety_rank_for_all_reached, suited_safety_rank_for_any_reached,
    suited_safety_rank_for_players, suji_dahai_actions_by_safety, suji_safety_rank_for,
    suji_safety_rank_for_all_reached, suji_safety_rank_for_any_reached,
    suji_safety_rank_for_players, tanki_remaining_candidates,
    tanki_remaining_candidates_for_player, visible_count_of, wall_rank, wall_tile_types_by_rank,
};
pub use discard_selection::select_discard_action;
pub use kuikae::forbidden_discards_after_call;
pub use meld::{Meld, MeldKind, fixed_meld_count};
pub use offense_value::{
    OffenseValue, TenpaiOffenseMode, TenpaiOffenseValue, reach_baseline_context,
};
pub use open_hand_defense::{
    OpenHandDefenseCandidateDiagnostic, OpenHandDefenseCategory, OpenHandDefenseDiagnostic,
    OpenHandDefenseSelectionDiagnostic, OpenHandDefenseTargetSafety, high_open_hand_threat_players,
    high_open_hand_threat_players_from_context, high_open_hand_threat_players_from_facts,
    is_discarded_by_all_open_hand_threats, is_ron_safe_for_all_open_hand_targets,
    is_ron_safe_for_open_hand_target, open_hand_defense_category,
    open_hand_honor_dahai_actions_by_safety, open_hand_suited_dahai_actions_by_safety,
    opponent_honor_value_for_open_hand_threats, safe_against_all_targets_dahai_actions,
    select_open_hand_defense_fallback_action, select_open_hand_defense_fallback_action_with_kind,
    suited_safety_evidence_for_open_hand_threats, suited_safety_rank_for_open_hand_threats,
    suji_safety_rank_for_open_hand_threats,
};
pub use open_hand_threat::{
    OpenHandThreatAssessment, OpenHandThreatDecision, OpenHandThreatExclusion, OpenHandThreatLevel,
    OpenHandThreatReason, classify_open_hand_threat, classify_open_hand_threats,
    has_high_open_hand_threat,
};
pub use prospective_value::{
    ProspectiveBaselineValue, ProspectiveDiscardValue, ProspectiveDrawValue,
    ProspectiveDrawVariantValue, ProspectiveLookaheadDiagnostic, ProspectiveOutcome,
    ProspectiveTenpaiValue, ProspectiveUnavailable, ProspectiveUnknownReason, ProspectiveValue,
    ProspectiveWaitValue, ProspectiveWinningTileValue,
};
pub use push_pull::{
    PushPullDecision, PushPullInputs, PushPullMode, PushPullOffenseState, PushPullReason,
    PushPullTenpaiWaitFacts, StrongTenpaiRequirement, decide_push_pull,
    push_pull_inputs_from_context,
};
pub use reach_policy::{
    REACH_MIN_REMAINING, REACH_MIN_REMAINING_TILES, REACH_MIN_SCORE, ReachLegalityFacts,
    decide_reach_reason, is_reach_legal,
};
pub use threat::{
    FixedMeldValueFacts, MeldKindCounts, MeldThreatDiagnostic, MeldThreatFacts,
    PlayerThreatDiagnostic, PlayerThreatFacts, PlayerThreatInputs, ValueHonorMeldCounts,
    ValueHonorMeldFacts, diagnose_meld_threat, diagnose_player_threat,
    diagnose_player_threat_with_facts, diagnose_player_threats, diagnose_player_threats_with_facts,
    fixed_meld_value_facts, has_reached_dealer, meld_threat_facts, player_threat_facts,
    player_threat_facts_from_context, player_threat_inputs, reached_opponent_count,
};
