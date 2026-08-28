use riichilab_client::CaptureRecordError;
use thiserror::Error;

use crate::cli::CliError;
use crate::input::TileInputError;
use crate::tiles::TileAllocationError;

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum ScenarioError {
    #[error(transparent)]
    Cli(#[from] CliError),

    #[error("cannot read scenario file {path:?}: {message}")]
    ReadFile { path: String, message: String },

    #[error("cannot parse scenario JSON {path:?}: {message}")]
    Json { path: String, message: String },

    #[error("cannot write {path:?}: {message}")]
    WriteFile { path: String, message: String },

    #[error("cannot parse capture file {path:?} line {line}: {source}")]
    CaptureRecord {
        path: String,
        line: usize,
        #[source]
        source: CaptureRecordError,
    },

    #[error("capture file {path:?} has no request_action record")]
    EmptyCapture { path: String },

    #[error(
        "capture file {path:?} has {count} request_action records; select one with --request-id"
    )]
    AmbiguousCapture { path: String, count: usize },

    #[error("capture file {path:?} has no request_action with request_id {request_id}")]
    CapturedRequestNotFound { path: String, request_id: u64 },

    #[error("cannot decode the observation of request_id {request_id} in {path:?}: {message}")]
    CaptureObservation {
        path: String,
        request_id: u64,
        message: String,
    },

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
}

impl ScenarioError {
    pub fn is_usage_error(&self) -> bool {
        matches!(self, Self::Cli(_))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cli_errors_are_usage_errors() {
        assert!(ScenarioError::from(CliError::MissingHand).is_usage_error());
    }

    #[test]
    fn scenario_errors_are_not_usage_errors() {
        let error = ScenarioError::ReachedLength { count: 3 };
        assert!(!error.is_usage_error());
    }

    #[test]
    fn tile_input_error_message_has_field_and_input() {
        let error = ScenarioError::TileInput {
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
