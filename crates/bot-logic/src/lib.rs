pub mod acceptance;
pub mod bonus_han;
pub mod completed_hand;
pub mod discard;
pub mod fu;
pub mod furiten;
pub mod han;
pub mod hand;
pub mod hand_settlement;
pub mod hand_value;
pub mod iishanten;
pub mod lookahead;
pub mod meld;
pub mod normal_hand_scoring;
pub mod normal_score;
pub mod payment;
pub mod scoring_selection;
pub mod selection;
pub mod self_tsumo;
pub mod shanten;
pub mod tenpai_hand_value;
pub mod tile;
pub mod tile_counts;
pub mod winning_context;
pub mod winning_tile;
pub mod winning_yaku;
pub mod winning_yakuman;
pub mod yaku;
pub mod yakuman;
pub mod yakuman_scoring;

pub use acceptance::{
    Acceptance, AcceptanceTile, EffectiveAcceptance, EffectiveAcceptanceTile, calculate_acceptance,
    calculate_acceptance_with_fixed_melds, calculate_acceptance_with_fixed_melds_and_visible_tiles,
    calculate_acceptance_with_visible_tiles, structural_acceptance_tile_types,
    structural_acceptance_tile_types_with_fixed_melds,
};
pub use bonus_han::{BonusHanBreakdown, UraDoraHan, evaluate_bonus_han};
pub use completed_hand::{
    ChiitoitsuDecomposition, CompletedHandAnalysis, CompletedHandDecomposition, CompletedHandError,
    ConcealedMeld, KokushiDecomposition, StandardDecomposition, analyze_completed_hand,
    is_standard_hand_complete, standard_completion_intersects,
};
pub use discard::{
    DiscardBlockContext, DiscardCandidateDiagnostic, DiscardComparison, DiscardComparisonReason,
    DiscardDecisionDiagnostic, DiscardEvaluation, FloatingTileValue, HandShapeSummary, PairContext,
    ShapeBreakdown, compare_discard_evaluations, diagnose_discard_evaluations,
    diagnose_discard_evaluations_with_fixed_melds,
    diagnose_discard_evaluations_with_fixed_melds_and_forward_metrics,
    diagnose_discard_evaluations_with_fixed_melds_and_tenpai_wait,
    diagnose_discard_evaluations_with_metrics, discard_block_context,
    discard_block_context_with_fixed_melds, evaluate_discards, evaluate_discards_from_tiles,
    evaluate_discards_from_tiles_with_context, evaluate_discards_from_tiles_with_dora,
    evaluate_discards_from_tiles_with_fixed_melds_and_context,
    evaluate_discards_from_tiles_with_fixed_melds_and_visible_tiles,
    evaluate_discards_from_tiles_with_visible_tiles, evaluate_discards_with_fixed_melds,
    evaluate_discards_with_fixed_melds_and_visible_tiles, evaluate_discards_with_visible_tiles,
    floating_tile_value_breakdown_for_discard, floating_tile_value_for_discard, hand_shape_summary,
    pair_context_for_discard, select_best_discard, select_best_discard_from_tiles,
    select_best_discard_from_tiles_with_context, select_best_discard_from_tiles_with_dora,
    select_best_discard_from_tiles_with_visible_tiles, select_best_discard_with_visible_tiles,
    shape_breakdown_for_discard, shape_penalty_for_discard, shape_penalty_for_discard_with_context,
    shape_penalty_for_discard_with_fixed_melds,
    shape_penalty_for_discard_with_fixed_melds_and_context, split_discarded_tile,
};
pub use fu::{FuBreakdown, FuContribution, FuKind, WinningFuEvaluation, evaluate_winning_fu};
pub use furiten::{
    DiscardFuritenDiagnostic, HistoryFuritenFacts, OwnDiscards, PermanentFuriten,
    PermanentFuritenDiagnostic, TenpaiWaitAvailability, can_ron_from_furiten,
    diagnose_discard_furiten, discard_tenpai_wait_availability, permanent_furiten_for_waits,
    tenpai_wait_availability,
};
pub use han::{WinningYakuHanEvaluation, YakuHan, evaluate_winning_yaku_han};
pub use hand::{Hand, HandError};
pub use hand_settlement::{
    HandSettlement, HandSettlementError, HonbaPayments, MissingSettlementFact,
    evaluate_hand_settlement,
};
pub use hand_value::{HandValue, HandValueError, HandValueOutcome, evaluate_hand_value};
pub use iishanten::{
    IishantenShape, classify_standard_iishanten_shape,
    classify_standard_iishanten_shape_after_discard,
};
pub use lookahead::{
    DiscardLookaheadDiagnostic, DrawLookaheadDiagnostic, DrawTransition,
    DrawVariantLookaheadDiagnostic, LookaheadDiagnostic, LookaheadInputs, ProspectiveTenpai,
    ProspectiveTenpaiValuator, ProspectiveTsumoValuator, SameShantenDownstreamDiagnostic,
    diagnose_lookahead, diagnose_lookahead_candidate, forward_metrics,
    forward_metrics_for_candidate, forward_metrics_from_lookahead,
    same_shanten_downstream_value_for_candidate, same_shanten_forward_metric_for_candidate,
    tenpai_wait_metrics_from_lookahead,
};
pub use meld::{Meld, MeldKind, MeldShape, fixed_meld_count, is_menzen};
pub use normal_hand_scoring::{
    MissingScoringFact, NormalScoringCandidate, NormalScoringError, NormalScoringState,
    evaluate_normal_hand_scoring,
};
pub use normal_score::{LimitClass, NormalScoreBase, NormalScoreError, evaluate_normal_score_base};
pub use payment::{Payment, PaymentBreakdown, PaymentError, evaluate_payment};
pub use scoring_selection::{
    BestScoringSelection, ScoringCandidateRef, select_best_scoring_candidate,
};
pub use selection::{
    CurrentTenpaiFuritenCohort, CurrentTenpaiMetrics, DiscardSelectionCandidate, ForwardMetrics,
    NextAcceptanceMetric, TenpaiWaitMetric, WeightedForwardMetric, best_discard_selection_index,
    best_discard_selection_index_with_forward_metrics, best_discard_selection_index_with_metrics,
    classify_current_tenpai_furiten_cohort, compare_discard_selection_candidates,
    current_tenpai_continuation_targets, resolve_current_tenpai_value_axis,
    resolve_prospective_value_axis,
};
pub use self_tsumo::{
    SELF_TSUMO_VALUE_SCALE, SelfTsumoFacts, SelfTsumoPath, TSUMO_PROBABILITY_SCALE,
    TenpaiTsumoValue, tsumo_hit_probability,
};
pub use shanten::{
    EffectiveShanten, FixedMeldCount, MinShanten, Shanten, calculate_shanten,
    calculate_shanten_with_fixed_melds, chiitoitsu_shanten, kokushi_shanten, standard_shanten,
    standard_shanten_with_fixed_melds,
};
pub use tenpai_hand_value::{
    TenpaiCompletedHands, TenpaiHandValueError, TenpaiHandValueProfile, TenpaiWaitCompletedHand,
    TenpaiWaitHandValue, WinningTileCompletedHand, WinningTileHandValue,
    evaluate_tenpai_hand_value, tenpai_completed_hands,
};
pub use tile::{
    Dragon, PhysicalTileVariant, Suit, TileId, TileParseError, TileType, VisibleTile, count_dora,
    count_indicated_dora, next_dora, physical_tile_variants, seen_red_fives,
};
pub use tile_counts::{TileCountError, TileCounts};
pub use winning_context::{RiichiStatus, WinMethod, WinningContext};
pub use winning_tile::{WaitType, WinningGroup, WinningTileInterpretation, interpret_winning_tile};
pub use winning_yaku::{WinningYakuEvaluation, concealed_set_count, evaluate_winning_yaku};
pub use winning_yakuman::{WinningYakumanEvaluation, evaluate_winning_yakuman};
pub use yaku::{
    StructuralYakuEvaluation, Yaku, YakuEvaluation, evaluate_structural_yaku, evaluate_yaku,
    fixed_melds_guarantee_yaku,
};
pub use yakuman::{Yakuman, YakumanEvaluation, evaluate_yakuman};
pub use yakuman_scoring::{
    YakumanContribution, YakumanScoringCandidate, YakumanScoringError, evaluate_yakuman_scoring,
};
