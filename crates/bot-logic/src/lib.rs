pub mod acceptance;
pub mod discard;
pub mod hand;
pub mod iishanten;
pub mod shanten;
pub mod tile;
pub mod tile_counts;

pub use acceptance::{
    Acceptance, AcceptanceTile, calculate_acceptance, calculate_acceptance_with_visible_tiles,
};
pub use discard::{
    DiscardBlockContext, DiscardCandidateDiagnostic, DiscardComparison, DiscardComparisonReason,
    DiscardDecisionDiagnostic, DiscardEvaluation, FloatingTileValue, HandShapeSummary, PairContext,
    ShapeBreakdown, compare_discard_evaluations, diagnose_discard_evaluations,
    discard_block_context, evaluate_discards, evaluate_discards_from_tiles,
    evaluate_discards_from_tiles_with_context, evaluate_discards_from_tiles_with_dora,
    evaluate_discards_from_tiles_with_visible_tiles, evaluate_discards_with_visible_tiles,
    floating_tile_value_breakdown_for_discard, floating_tile_value_for_discard, hand_shape_summary,
    pair_context_for_discard, select_best_discard, select_best_discard_from_tiles,
    select_best_discard_from_tiles_with_context, select_best_discard_from_tiles_with_dora,
    select_best_discard_from_tiles_with_visible_tiles, select_best_discard_with_visible_tiles,
    shape_breakdown_for_discard, shape_penalty_for_discard, shape_penalty_for_discard_with_context,
};
pub use hand::{Hand, HandError};
pub use iishanten::{
    IishantenShape, classify_standard_iishanten_shape,
    classify_standard_iishanten_shape_after_discard,
};
pub use shanten::{
    Shanten, calculate_shanten, chiitoitsu_shanten, kokushi_shanten, standard_shanten,
};
pub use tile::{Suit, TileId, TileParseError, TileType, VisibleTile, count_dora, next_dora};
pub use tile_counts::{TileCountError, TileCounts};
