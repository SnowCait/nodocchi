pub mod acceptance;
pub mod discard;
pub mod hand;
pub mod iishanten;
pub mod lookahead;
pub mod shanten;
pub mod tile;
pub mod tile_counts;

pub use acceptance::{
    Acceptance, AcceptanceTile, EffectiveAcceptance, EffectiveAcceptanceTile, calculate_acceptance,
    calculate_acceptance_with_fixed_melds, calculate_acceptance_with_fixed_melds_and_visible_tiles,
    calculate_acceptance_with_visible_tiles,
};
pub use discard::{
    DiscardBlockContext, DiscardCandidateDiagnostic, DiscardComparison, DiscardComparisonReason,
    DiscardDecisionDiagnostic, DiscardEvaluation, FloatingTileValue, HandShapeSummary, PairContext,
    ShapeBreakdown, compare_discard_evaluations, diagnose_discard_evaluations,
    diagnose_discard_evaluations_with_fixed_melds, discard_block_context,
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
    shape_penalty_for_discard_with_fixed_melds_and_context,
};
pub use hand::{Hand, HandError};
pub use iishanten::{
    IishantenShape, classify_standard_iishanten_shape,
    classify_standard_iishanten_shape_after_discard,
};
pub use lookahead::{
    DiscardLookaheadDiagnostic, DrawLookaheadDiagnostic, LookaheadDiagnostic,
    diagnose_lookahead_with_fixed_melds, diagnose_lookahead_with_fixed_melds_and_visible_tiles,
};
pub use shanten::{
    EffectiveShanten, FixedMeldCount, Shanten, calculate_shanten,
    calculate_shanten_with_fixed_melds, chiitoitsu_shanten, kokushi_shanten, standard_shanten,
    standard_shanten_with_fixed_melds,
};
pub use tile::{
    Suit, TileId, TileParseError, TileType, VisibleTile, count_dora, count_indicated_dora,
    next_dora,
};
pub use tile_counts::{TileCountError, TileCounts};
