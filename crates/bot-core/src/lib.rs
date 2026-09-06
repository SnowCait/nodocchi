pub mod action;
pub mod agent;
pub mod agents;
pub mod call_decision;
pub mod combined_defense;
pub mod context;
pub mod current_tenpai_continuation;
pub mod damaten_value;
pub mod decision_timing;
pub mod defense;
pub mod discard_selection;
mod fold_defense;
pub mod kuikae;
pub mod meld;
pub mod offense_value;
pub mod open_hand_defense;
pub mod open_hand_threat;
pub mod prospective_value;
pub mod push_pull;
pub mod reach_damaten_comparison;
pub mod reach_decision;
pub mod reach_policy;
pub mod ron_opportunity;
pub mod ryukyoku_decision;
pub mod shanten_diagnostic;
#[cfg(test)]
pub(crate) mod shanten_test_support;
pub mod tenpai_continuation;
pub mod tenpai_scoring;
pub mod threat;
pub mod two_shanten_self_tsumo_cost;

pub use action::LegalAction;
pub use agent::Agent;
pub use agents::{
    AgentActionSource, MenzenAgent, NormalAgent, ReachDecisionReason, ShantenAgent, TsumogiriAgent,
};
pub use call_decision::{
    CALL_CURRENT_SHANTEN, CALL_MIN_LIVE_WAIT_REMAINING, CALL_TENPAI_SHANTEN,
    CallCandidateDiagnostic, CallDecisionDiagnostic, CallDecisionReason,
    CallIishantenAcceptanceDiagnostic, CallIishantenComparison, CallIishantenSelfTsumoDiagnostic,
    CallKind, CallTwoShantenPassEvaluation, CallTwoShantenSelfTsumoDiagnostic, CallWaitYaku,
    CallWaitYakuDiagnostic,
};
pub use combined_defense::{
    CombinedDefenseCandidateDiagnostic, CombinedDefenseCategory, CombinedDefenseDiagnostic,
    CombinedDefenseSelectionDiagnostic, CombinedDefenseTargetSafety, ThreatDefenseTarget,
    ThreatDefenseTargetKind, combined_defense_category, combined_honor_dahai_actions_by_safety,
    combined_suited_dahai_actions_by_safety, combined_threat_defense_targets,
    combined_threat_defense_targets_from_context, combined_threat_defense_targets_from_facts,
    has_same_hand_passed_for_all_threats, is_ron_safe_for_target, is_safe_against_all_threats,
    is_same_hand_passed_for_target, opponent_honor_value_for_combined_threats,
    safe_against_all_threats_dahai_actions, same_hand_passed_combined_dahai_actions,
    select_combined_threat_defense_fallback_action,
    select_combined_threat_defense_fallback_action_with_kind,
    suited_safety_evidence_for_combined_threats, suited_safety_rank_for_combined_threats,
    suji_safety_rank_for_combined_threats,
};
pub use context::{GameContext, TableStateFacts, seat_wind_for_player};
pub use current_tenpai_continuation::{
    CurrentTenpaiContinuationCandidate, CurrentTenpaiContinuationDiagnostic,
};
pub use damaten_value::{
    DAMATEN_MIN_TOTAL, DamatenValue, DamatenValueDiagnostic, DamatenValueVerdict, DamatenWaitValue,
    DamatenWinningTileValue, damaten_baseline_context,
};
pub use decision_timing::{
    DecisionPhaseDurations, ForwardMetricsPhaseDurations, NormalDiscardPhaseDurations,
    TimedAgentAction,
};
pub use defense::{
    CompressedHiddenHandStateMetrics, CompressedHiddenHandStates, DefenseCandidateDiagnostic,
    DefenseDecisionDiagnostic, DefenseFallbackDiagnostic, DefenseFallbackKind,
    HiddenHandStateMetrics, HiddenHandStateUnsupported, HonorSafetyRank, OpponentHonorValue,
    PlayerRonRiskEvidence, ReachedHiddenHandStates, RonCapableStateWeight, RonRiskEvidence,
    SequenceWaitRoute, SequenceWaitShape, SuitedSafetyEvidence, SuitedSafetyRank, SujiSafetyRank,
    TenpaiStateWeight, WallRank, compare_lexicographic_minimax_ron_risk,
    compressed_ron_capable_hidden_hand_weight, compressed_tenpai_hidden_hand_weight,
    genbutsu_dahai_actions_for_all_reached, honor_dahai_actions_by_safety,
    honor_dahai_actions_by_safety_with, honor_safety_rank, is_discarded_by_all_players,
    is_discarded_by_player, is_genbutsu_for, is_genbutsu_for_all_reached, is_no_chance,
    is_one_chance, is_suji_for, is_suji_for_all_reached, is_suji_for_any_reached,
    log_defense_fallback_decision, opponent_honor_value_for, opponent_honor_value_for_players,
    opponent_honor_value_for_reached, remaining_tile_copies, ron_capable_hidden_hand_weight,
    select_defense_fallback_action, select_defense_fallback_action_with_kind,
    select_genbutsu_fallback_action, select_honor_safety_fallback_action,
    select_suited_safety_fallback_action, sequence_route_remaining_combinations,
    sequence_route_remaining_combinations_for_player, sequence_wait_routes,
    shanpon_remaining_combinations, shanpon_remaining_combinations_for_player,
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
    OffenseValue, ReachRonBaselineDiagnostic, ReachRonWaitValue, ReachRonWinningTileValue,
    TenpaiOffenseMode, TenpaiOffenseValue, reach_baseline_context,
};
pub use open_hand_defense::{
    OpenHandDefenseCandidateDiagnostic, OpenHandDefenseCategory, OpenHandDefenseDiagnostic,
    OpenHandDefenseSelectionDiagnostic, OpenHandDefenseTargetSafety,
    has_same_hand_passed_for_all_open_hand_targets, high_open_hand_threat_players,
    high_open_hand_threat_players_from_context, high_open_hand_threat_players_from_facts,
    is_discarded_by_all_open_hand_threats, is_ron_safe_for_all_open_hand_targets,
    is_ron_safe_for_open_hand_target, is_same_hand_passed_for_open_hand_target,
    open_hand_defense_category, open_hand_honor_dahai_actions_by_safety,
    open_hand_suited_dahai_actions_by_safety, opponent_honor_value_for_open_hand_threats,
    safe_against_all_targets_dahai_actions, same_hand_passed_open_hand_dahai_actions,
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
    ProspectiveTenpaiValue, ProspectiveUnavailable, ProspectiveWaitValue,
    ProspectiveWinningTileValue,
};
pub use push_pull::{
    PushPullDecision, PushPullInputs, PushPullMode, PushPullOffenseState, PushPullReason,
    PushPullTenpaiWaitFacts, StrongTenpaiRequirement, decide_push_pull,
    push_pull_inputs_from_context,
};
pub use reach_damaten_comparison::ReachDamatenComparisonDiagnostic;
pub use reach_decision::ReachDecisionDiagnostic;
pub use reach_policy::{
    REACH_MIN_REMAINING, REACH_MIN_REMAINING_TILES, REACH_MIN_SCORE, ReachLegalityFacts,
    ReachTimingDecision, ReachTimingDiagnostic, ReachTimingReason,
    decide_permanent_furiten_reach_timing, decide_reach_reason, evaluates_reach_timing,
    is_reach_legal,
};
pub use ron_opportunity::{
    HonorPublicSafetyEvidence, ReachPublicSafetyEvidence, RonOpportunityDiagnostic,
    RonOpportunityExternalThreats, RonOpportunityWaitDiagnostic,
};
pub use ryukyoku_decision::{
    RYUKYOKU_CONTINUE_CHIITOITSU_SHANTEN, RYUKYOKU_CONTINUE_KOKUSHI_SHANTEN,
    RYUKYOKU_CONTINUE_STANDARD_SHANTEN, RyukyokuDecisionDiagnostic, RyukyokuVerdict,
    continues_with_shanten, evaluate_ryukyoku_decision,
};
pub use shanten_diagnostic::{
    DiagnosticOptions, ShantenDecisionDiagnostic, diagnose_shanten_decision,
    diagnose_shanten_decision_with_options,
};
pub use tenpai_continuation::{
    TenpaiContinuationBranch, TenpaiContinuationCandidate, TenpaiContinuationDiagnostic,
    TenpaiSelfTsumoComparison,
};
pub use tenpai_scoring::{TenpaiVariantUnknownReason, TenpaiVariantValue};
pub use threat::{
    FixedMeldValueFacts, MeldKindCounts, MeldThreatDiagnostic, MeldThreatFacts,
    PlayerThreatDiagnostic, PlayerThreatFacts, PlayerThreatInputs, ValueHonorMeldCounts,
    ValueHonorMeldFacts, diagnose_meld_threat, diagnose_player_threat,
    diagnose_player_threat_with_facts, diagnose_player_threats, diagnose_player_threats_with_facts,
    fixed_meld_value_facts, has_reached_dealer, meld_threat_facts, player_threat_facts,
    player_threat_facts_from_context, player_threat_inputs, reached_opponent_count,
};
pub use two_shanten_self_tsumo_cost::{
    TwoShantenProgressSelfTsumoCost, TwoShantenSelfTsumoCost,
    measure_two_shanten_progress_self_tsumo, measure_two_shanten_self_tsumo,
};
