pub mod convert;
pub mod decision;
pub mod event;
pub mod handler;
pub mod protocol;
pub mod reply;
pub mod tags;

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
    ChiihouEventConfig, ChiihouEventError, ChiihouIncomingEvent, ChiihouOutgoingReply,
    SeenEventIds, build_reply_for_event, event_channel_id, event_is_from_server, event_targets_ai,
    is_chiihou_request_kind, process_incoming_event, should_handle_event,
};
pub use handler::{
    ChiihouHandlerError, ChiihouHandlerResult, build_reply_for_request, handle_chiihou_content,
    reply_content_for_chiihou_content,
};
pub use protocol::{
    ChiihouNakuAction, ChiihouPai, ChiihouPaiParseError, ChiihouProtocolError, ChiihouRequest,
    ChiihouSuit, parse_chiihou_request,
};
pub use reply::{build_naku_no_reply_content, build_sutehai_reply_content};
pub use tags::{build_reply_tags, has_tag_value, root_channel_id};
