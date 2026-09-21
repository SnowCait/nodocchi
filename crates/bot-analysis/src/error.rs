use thiserror::Error;

use crate::input::TileInputError;
use crate::tiles::TileAllocationError;

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum ScenarioBuildError {
    #[error("invalid tile string in {field} ({input:?}): {source}")]
    TileInput {
        field: String,
        input: String,
        #[source]
        source: TileInputError,
    },

    #[error("cannot place tiles of {field} ({input:?}): {source}")]
    TileAllocation {
        field: String,
        input: String,
        #[source]
        source: TileAllocationError,
    },

    #[error("scenario tiles are inconsistent: {source}")]
    PhysicalTiles {
        #[source]
        source: TileAllocationError,
    },

    #[error("draw ({input:?}) must be a single tile, but expands to {count} tiles")]
    MultipleDrawTiles { input: String, count: usize },

    #[error("{field} ({input:?}) must be a single tile, but expands to {count} tiles")]
    NotSingleTile {
        field: String,
        input: String,
        count: usize,
    },

    #[error("{field} ({input:?}) must be a wind tile: E, S, W or N")]
    NotWind { field: String, input: String },

    #[error("{field} must be 0..=3, but is {value}")]
    SeatOutOfRange { field: String, value: u8 },

    #[error(
        "seat_wind {explicit} conflicts with {derived} derived from player_id {player_id} and oya {oya}"
    )]
    SeatWindConflict {
        explicit: String,
        derived: String,
        player_id: u8,
        oya: u8,
    },

    #[error("reached must have 4 elements, but has {count}")]
    ReachedLength { count: usize },

    #[error("discards must have 4 elements, but has {count}")]
    DiscardsLength { count: usize },

    #[error("reach_discard_indices must have 4 elements, but has {count}")]
    ReachDiscardIndicesLength { count: usize },

    #[error("reach_discard_indices[{player}] must be 1..={discard_count}, but is {index}")]
    ReachDiscardIndexOutOfRange {
        player: usize,
        index: u32,
        discard_count: usize,
    },

    #[error("reach_discard_indices[{player}] needs reached[{player}] to be true")]
    ReachDiscardIndexWithoutReach { player: usize },

    #[error("riichi_situation.{field} must have 4 elements, but has {count}")]
    RiichiSituationLength { field: &'static str, count: usize },

    #[error("riichi_situation.{field}[{player}] needs reached[{player}] to be true")]
    RiichiSituationWithoutReach { field: &'static str, player: usize },

    #[error("post_reach_passed must have 4 elements, but has {count}")]
    PostReachPassedLength { count: usize },

    #[error("temporary_passed must have 4 elements, but has {count}")]
    TemporaryPassedLength { count: usize },

    #[error("melds must have 4 elements, but has {count}")]
    MeldsLength { count: usize },

    #[error("scores must have 4 elements, but has {count}")]
    ScoresLength { count: usize },

    #[error("kyoku must be 1..=4, but is {value}")]
    KyokuOutOfRange { value: u8 },

    #[error("{field} ({kind}) must have {expected} tiles, but has {count}")]
    MeldTileCount {
        field: String,
        kind: String,
        expected: usize,
        count: usize,
    },

    #[error("{field} ({input:?}) is not a {kind}")]
    MeldShape {
        field: String,
        kind: String,
        input: String,
    },

    #[error("{field} ({kind}) needs called_tile")]
    MeldCalledTileMissing { field: String, kind: String },

    #[error("{field} ({kind}) must not have called_tile {tile}")]
    MeldCalledTileNotAllowed {
        field: String,
        kind: String,
        tile: String,
    },

    #[error("{field} called_tile {tile} is not in the meld tiles")]
    MeldCalledTileNotInMeld { field: String, tile: String },

    #[error("{field} called_tile {tile} is not in any discards")]
    MeldCalledTileNotDiscarded { field: String, tile: String },

    #[error("legal_dahai {tile} is not in hand or draw")]
    LegalDahaiNotHeld { tile: String },

    #[error("legal_dahai {tile} does not match the held {held}")]
    LegalDahaiRedMismatch { tile: String, held: String },

    #[error("legal_dahai {tile} appears more than once")]
    LegalDahaiDuplicate { tile: String },

    #[error("{field} needs player_id to tell whose discard is called")]
    LegalPonWithoutPlayerId { field: String },

    #[error("{field} from_player must not be the player_id {player_id} itself")]
    LegalPonFromOwnDiscard { field: String, player_id: u8 },

    #[error("{field} consumed must have {expected} tiles, but has {count}")]
    LegalPonConsumedCount {
        field: String,
        expected: usize,
        count: usize,
    },

    #[error("{field} consumed ({consumed:?}) must have the same tile type as {tile}")]
    LegalPonTileType {
        field: String,
        tile: String,
        consumed: String,
    },

    #[error("{field} needs a discard of player {from_player}, but it has none")]
    LegalPonNoDiscard { field: String, from_player: u8 },

    #[error("{field} tile {tile} is not the last discard {discarded} of player {from_player}")]
    LegalPonTargetMismatch {
        field: String,
        tile: String,
        discarded: String,
        from_player: u8,
    },

    #[error("{field} consumed {tile} is not in hand")]
    LegalPonConsumedNotHeld { field: String, tile: String },

    #[error("{field} consumed must have {expected} tiles, but has {count}")]
    LegalAnkanConsumedCount {
        field: String,
        expected: usize,
        count: usize,
    },

    #[error("{field} consumed ({consumed:?}) must be four tiles of the same tile type")]
    LegalAnkanTileType { field: String, consumed: String },

    #[error("{field} consumed {tile} is not in hand or the drawn tile")]
    LegalAnkanConsumedNotHeld { field: String, tile: String },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tile_input_error_message_has_field_and_input() {
        let error = ScenarioBuildError::TileInput {
            field: "hand".to_string(),
            input: "123x".to_string(),
            source: TileInputError::UnknownSuit {
                token: "123x".to_string(),
                suit: 'x',
            },
        };
        let message = error.to_string();
        assert!(message.contains("hand"), "{message}");
        assert!(message.contains("123x"), "{message}");
    }
}
