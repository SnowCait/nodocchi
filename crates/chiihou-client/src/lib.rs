pub mod cli;
pub mod command;
pub mod config;
pub mod convert;
pub mod decision;
pub mod event;
pub mod handler;
pub mod nostr_adapter;
pub mod protocol;
pub mod reply;
pub mod runtime;
pub mod secret;
pub mod status;
pub mod tags;

pub use cli::{
    ChiihouAgentKind, ChiihouAgentKindParseError, ChiihouCliArgs, ChiihouCliError,
    ChiihouServerNpubError, ChiihouStartupConfigError, USAGE, build_cli_nostr_config,
    resolve_server_npub, validate_server_npub,
};
pub use command::{
    ChiihouCommandError, build_chiihou_startup_command_content, build_chiihou_startup_command_tags,
    sign_chiihou_startup_command,
};
pub use config::{
    CHIIHOU_SERVER_NPUB, ChiihouChannel, ChiihouChannelParseError, ChiihouConfigError,
    ChiihouNostrConfig, DEFAULT_RELAY_URLS, HANCHAN_CHANNEL_ID, TONPUU_CHANNEL_ID,
};
pub use convert::{
    chiihou_pai_from_tile_id, emoji_shortcode_to_chiihou_pai, extract_chiihou_pais_from_emoji_text,
    temporary_tile_id_from_chiihou_pai, tile_type_from_chiihou_pai,
};
pub use decision::{
    SutehaiDecisionError, build_sutehai_reply_for_request, chiihou_pai_from_dahai_action,
    choose_sutehai_pai, game_context_from_sutehai_request,
    legal_dahai_actions_from_sutehai_request,
};
pub use event::{
    CHIIHOU_BITCHAT_MESSAGE_KIND, CHIIHOU_BITCHAT_TELEPORT_TAG, CHIIHOU_CHANNEL_MESSAGE_KIND,
    ChiihouEventConfig, ChiihouEventError, ChiihouIncomingEvent, ChiihouOutgoingReply,
    SeenEventIds, build_reply_for_event, build_reply_tags_for_event, event_channel_id,
    event_is_from_server, event_targets_ai, is_chiihou_request_kind, process_incoming_event,
    should_handle_event,
};
pub use handler::{
    ChiihouHandlerError, ChiihouHandlerResult, build_reply_for_request, handle_chiihou_content,
    reply_content_for_chiihou_content,
};
pub use nostr_adapter::{
    ChiihouNostrAdapterError, incoming_event_from_nostr, nostr_tags_from_strings,
    sign_outgoing_event, sign_outgoing_reply,
};
pub use protocol::{
    ChiihouNakuAction, ChiihouPai, ChiihouPaiParseError, ChiihouProtocolError, ChiihouRequest,
    ChiihouSuit, parse_chiihou_request,
};
pub use reply::{build_naku_no_reply_content, build_sutehai_reply_content};
pub use secret::{CHIIHOU_NSEC_ENV, ChiihouSecretError, load_chiihou_nsec, validate_chiihou_nsec};

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
