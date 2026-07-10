pub mod cli;
pub mod command;
pub mod config;
mod controller;
pub mod convert;
pub mod decision;
pub mod event;
pub mod handler;
pub mod lifecycle;
pub mod match_state;
pub mod nostr_adapter;
pub mod protocol;
pub mod reply;
pub mod runtime;
pub mod secret;
pub mod status;
pub mod table_notification;
pub mod tags;

pub use cli::{
    ChiihouAgentKind, ChiihouAgentKindParseError, ChiihouCliArgs, ChiihouCliError,
    ChiihouServerNpubError, ChiihouStartupConfigError, USAGE, build_cli_nostr_config,
    resolve_server_npub, validate_server_npub,
};
pub use command::{
    ChiihouCommandError, build_chiihou_startup_command_content, build_chiihou_startup_command_tags,
    sign_chiihou_next_command, sign_chiihou_startup_command,
};
pub use config::{
    CHIIHOU_SERVER_NPUB, ChiihouChannel, ChiihouChannelParseError, ChiihouConfigError,
    ChiihouNostrConfig, DEFAULT_RELAY_URLS, HANCHAN_CHANNEL_ID, TONPUU_CHANNEL_ID,
};
pub use convert::{
    chiihou_pai_from_tile_id, emoji_shortcode_to_chiihou_pai, extract_chiihou_pais_from_emoji_text,
    temporary_tile_id_from_chiihou_pai, tile_type_from_chiihou_pai, tile_type_from_chiihou_wind,
};
pub use decision::{
    ChiihouNakuDecision, NakuDecisionError, SutehaiDecisionError, build_naku_reply_for_request,
    build_naku_reply_for_request_with_state, build_sutehai_reply_for_request,
    build_sutehai_reply_for_request_with_state, chiihou_pai_from_dahai_action,
    choose_naku_decision, choose_naku_decision_with_state, choose_sutehai_pai,
    choose_sutehai_pai_with_state, game_context_from_naku_request,
    game_context_from_naku_request_with_state, game_context_from_sutehai_request,
    game_context_from_sutehai_request_with_state, legal_actions_from_naku_actions,
    legal_dahai_actions_from_sutehai_request,
};
pub use event::{
    CHIIHOU_BITCHAT_MESSAGE_KIND, CHIIHOU_BITCHAT_TELEPORT_TAG, CHIIHOU_CHANNEL_MESSAGE_KIND,
    ChiihouEventConfig, ChiihouEventError, ChiihouIncomingEvent, ChiihouOutgoingReply,
    SeenEventIds, build_reply_for_event, build_reply_for_event_with_state,
    build_reply_tags_for_event, event_channel_id, event_is_from_server, event_targets_ai,
    is_chiihou_request_kind, process_incoming_event, should_handle_event,
};
pub use handler::{
    ChiihouHandlerError, ChiihouHandlerResult, build_reply_for_request,
    build_reply_for_request_with_state, handle_chiihou_content, handle_chiihou_content_with_state,
    reply_content_for_chiihou_content, reply_content_for_chiihou_content_with_state,
};
pub use lifecycle::{
    ChiihouLifecycleError, ChiihouLifecycleNotification, ChiihouPlayerScore, ChiihouWind,
    ChiihouWindParseError, parse_chiihou_lifecycle_notification,
};
pub use match_state::{
    ChiihouMatchPhase, ChiihouMatchState, ChiihouTableSnapshot, ChiihouTableStateError,
};
pub use nostr_adapter::{
    ChiihouNostrAdapterError, incoming_event_from_nostr, nostr_tags_from_strings,
    sign_outgoing_event, sign_outgoing_reply,
};
pub use protocol::{
    ChiihouCompactPaiParseError, ChiihouNakuAction, ChiihouPai, ChiihouPaiParseError,
    ChiihouProtocolError, ChiihouRequest, ChiihouSuit, parse_chiihou_request,
    parse_compact_chiihou_pais,
};
pub use reply::{
    build_naku_no_reply_content, build_naku_ron_reply_content, build_sutehai_reply_content,
};
pub use secret::{CHIIHOU_NSEC_ENV, ChiihouSecretError, load_chiihou_nsec, validate_chiihou_nsec};
pub use table_notification::{
    ChiihouSayAction, ChiihouTableNotification, ChiihouTableNotificationError,
    parse_chiihou_table_notification,
};

pub use runtime::{
    ChiihouRuntimeError, build_chiihou_request_filter, connect_chiihou_client,
    process_and_sign_nostr_event, publish_chiihou_event, publish_chiihou_reply, run_chiihou_client,
    run_chiihou_client_auto_enter, subscribe_chiihou_requests,
};
pub use status::{
    CHIIHOU_STATUS_FETCH_TIMEOUT, CHIIHOU_STATUS_KIND, ChiihouStartupCommand, ChiihouStatusError,
    ChiihouTableStatus, build_chiihou_status_filter, fetch_chiihou_table_status,
    parse_chiihou_table_status, startup_command_for_status,
};
pub use tags::{build_reply_tags, has_tag_value, root_channel_id};
