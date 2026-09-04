pub mod capture;
pub mod cli;
pub mod client;
pub mod config;
pub mod convert;
pub mod logging;
pub mod observation;
pub mod protocol;
pub mod state;
pub mod tls;
pub mod validation_policy;

pub use capture::{
    CAPTURE_VERSION, CaptureDirection, CaptureError, CaptureRecord, CaptureRecordError,
    CapturedRequestAction, SessionCapture, capture_client_action, capture_server_event,
};
pub use cli::{CliArgs, CliError, ConnectionMode, USAGE};
pub use client::{
    ClientError, ClientExitCondition, build_response_for_request,
    build_response_for_request_with_context, run_riichilab_client,
};
pub use config::{ClientConfig, ConfigError};
pub use convert::{
    checked_legal_action_to_mjai_action, fallback_mjai_action_from_possible_actions,
    legal_action_to_mjai_action, possible_action_to_legal_action,
    possible_actions_to_legal_actions,
};
pub use logging::LoggingError;
pub use observation::{
    DecodedObservation, ObservationError, ObservationPayload, game_context_from_decoded_observation,
};
pub use protocol::{
    ActionAckStatus, MjaiAction, MjaiEvent, MjaiPossibleAction, RequestTimeBudget, TimeControl,
    mjai_action_type, parse_server_event, parse_server_event_value, request_time_budget,
};
pub use state::ValidationState;
pub use tls::install_default_crypto_provider;
