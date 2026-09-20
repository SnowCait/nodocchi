use bot_analysis::ScenarioBuildError;
use riichilab_client::CaptureRecordError;
use thiserror::Error;

use crate::cli::CliError;

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum ScenarioError {
    #[error(transparent)]
    Cli(#[from] CliError),

    #[error(transparent)]
    Build(#[from] ScenarioBuildError),

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
}

impl ScenarioError {
    pub fn is_usage_error(&self) -> bool {
        matches!(self, Self::Cli(_))
    }
}

#[cfg(test)]
mod tests {
    use bot_analysis::TileInputError;

    use super::*;

    #[test]
    fn cli_errors_are_usage_errors() {
        assert!(ScenarioError::from(CliError::MissingHand).is_usage_error());
    }

    #[test]
    fn scenario_errors_are_not_usage_errors() {
        let error = ScenarioError::from(ScenarioBuildError::ReachedLength { count: 3 });
        assert!(!error.is_usage_error());
    }

    #[test]
    fn build_error_message_is_kept_as_is() {
        let build = ScenarioBuildError::TileInput {
            field: "hand".to_string(),
            input: "123x".to_string(),
            source: TileInputError::UnknownSuit {
                token: "123x".to_string(),
                suit: 'x',
            },
        };
        let message = build.to_string();
        assert_eq!(ScenarioError::from(build).to_string(), message);
    }
}
