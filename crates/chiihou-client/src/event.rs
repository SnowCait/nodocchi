use std::collections::HashSet;

use bot_core::Agent;

use crate::handler::{
    ChiihouHandlerError, reply_content_for_chiihou_content,
    reply_content_for_chiihou_content_with_state,
};
use crate::lifecycle::{
    ChiihouLifecycleError, ChiihouLifecycleNotification, parse_chiihou_lifecycle_notification,
};
use crate::match_state::ChiihouTableSnapshot;
use crate::table_notification::{
    ChiihouTableNotification, ChiihouTableNotificationError, parse_chiihou_table_notification,
};
use crate::tags::{build_reply_tags, has_tag_value, root_channel_id};

pub const CHIIHOU_CHANNEL_MESSAGE_KIND: u16 = 42;
pub const CHIIHOU_BITCHAT_MESSAGE_KIND: u16 = 20000;

pub const CHIIHOU_BITCHAT_TELEPORT_TAG: &str = "teleport";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChiihouIncomingEvent {
    pub id: String,
    pub pubkey: String,
    pub kind: u64,
    pub tags: Vec<Vec<String>>,
    pub content: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChiihouOutgoingReply {
    pub kind: u64,
    pub tags: Vec<Vec<String>>,
    pub content: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChiihouEventConfig {
    pub ai_pubkey_hex: String,
    pub server_pubkey_hex: String,
    pub server_npub: String,
    pub channel_ids: Vec<String>,
}

#[derive(Debug, thiserror::Error, Clone, PartialEq, Eq)]
pub enum ChiihouEventError {
    #[error("failed to handle chiihou content: {0}")]
    Handler(#[from] ChiihouHandlerError),

    #[error("failed to parse chiihou lifecycle notification: {0}")]
    Lifecycle(#[from] ChiihouLifecycleError),

    #[error("failed to parse chiihou table notification: {0}")]
    TableNotification(#[from] ChiihouTableNotificationError),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ChiihouIncomingAction {
    Ignore,
    Reply(ChiihouOutgoingReply),
    Lifecycle(ChiihouLifecycleNotification),
    TableNotification(ChiihouTableNotification),
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct SeenEventIds {
    ids: HashSet<String>,
}

impl SeenEventIds {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn contains(&self, event_id: &str) -> bool {
        self.ids.contains(event_id)
    }

    pub fn insert(&mut self, event_id: impl Into<String>) -> bool {
        self.ids.insert(event_id.into())
    }

    pub fn should_process(&mut self, event_id: &str) -> bool {
        self.insert(event_id)
    }
}

pub fn is_chiihou_request_kind(kind: u64) -> bool {
    kind == u64::from(CHIIHOU_CHANNEL_MESSAGE_KIND)
        || kind == u64::from(CHIIHOU_BITCHAT_MESSAGE_KIND)
}

pub fn event_targets_ai(event: &ChiihouIncomingEvent, ai_pubkey_hex: &str) -> bool {
    has_tag_value(&event.tags, "p", ai_pubkey_hex)
}

pub fn event_is_from_server(event: &ChiihouIncomingEvent, server_pubkey_hex: &str) -> bool {
    event.pubkey == server_pubkey_hex
}

pub fn event_channel_id<'a>(
    event: &'a ChiihouIncomingEvent,
    allowed_channel_ids: &[String],
) -> Option<&'a str> {
    root_channel_id(&event.tags).filter(|channel_id| {
        allowed_channel_ids
            .iter()
            .any(|allowed| allowed == channel_id)
    })
}

pub fn should_handle_event(
    event: &ChiihouIncomingEvent,
    config: &ChiihouEventConfig,
    seen: &mut SeenEventIds,
) -> bool {
    is_chiihou_request_kind(event.kind)
        && event_is_from_server(event, &config.server_pubkey_hex)
        && event_targets_ai(event, &config.ai_pubkey_hex)
        && event_channel_id(event, &config.channel_ids).is_some()
        && seen.should_process(&event.id)
}

pub fn build_reply_tags_for_event(
    event: &ChiihouIncomingEvent,
    config: &ChiihouEventConfig,
) -> Option<Vec<Vec<String>>> {
    let channel_id = event_channel_id(event, &config.channel_ids)?;
    let mut tags = build_reply_tags(
        &event.id,
        channel_id,
        &config.ai_pubkey_hex,
        &config.server_pubkey_hex,
    );
    if event.kind == u64::from(CHIIHOU_BITCHAT_MESSAGE_KIND) {
        tags.extend(
            event
                .tags
                .iter()
                .filter(|tag| tag.len() >= 2 && tag.first().is_some_and(|name| name == "g"))
                .cloned(),
        );
        tags.push(vec![
            "t".to_string(),
            CHIIHOU_BITCHAT_TELEPORT_TAG.to_string(),
        ]);
    }
    Some(tags)
}

pub fn build_reply_for_event<A: Agent>(
    event: &ChiihouIncomingEvent,
    config: &ChiihouEventConfig,
    agent: &mut A,
) -> Result<Option<ChiihouOutgoingReply>, ChiihouEventError> {
    let Some(content) =
        reply_content_for_chiihou_content(&config.server_npub, &event.content, agent)?
    else {
        return Ok(None);
    };
    let Some(tags) = build_reply_tags_for_event(event, config) else {
        return Ok(None);
    };
    Ok(Some(ChiihouOutgoingReply {
        kind: event.kind,
        tags,
        content,
    }))
}

pub fn build_reply_for_event_with_state<A: Agent>(
    event: &ChiihouIncomingEvent,
    config: &ChiihouEventConfig,
    state: &ChiihouTableSnapshot,
    agent: &mut A,
) -> Result<Option<ChiihouOutgoingReply>, ChiihouEventError> {
    let Some(content) = reply_content_for_chiihou_content_with_state(
        &config.server_npub,
        &event.content,
        state,
        agent,
    )?
    else {
        return Ok(None);
    };
    let Some(tags) = build_reply_tags_for_event(event, config) else {
        return Ok(None);
    };
    Ok(Some(ChiihouOutgoingReply {
        kind: event.kind,
        tags,
        content,
    }))
}

fn classify_notification(
    content: &str,
) -> Result<Option<ChiihouIncomingAction>, ChiihouEventError> {
    if let Some(notification) = parse_chiihou_lifecycle_notification(content)? {
        return Ok(Some(ChiihouIncomingAction::Lifecycle(notification)));
    }
    if let Some(notification) = parse_chiihou_table_notification(content)? {
        return Ok(Some(ChiihouIncomingAction::TableNotification(notification)));
    }
    Ok(None)
}

pub(crate) fn classify_incoming_event<A: Agent>(
    event: &ChiihouIncomingEvent,
    config: &ChiihouEventConfig,
    seen: &mut SeenEventIds,
    agent: &mut A,
) -> Result<ChiihouIncomingAction, ChiihouEventError> {
    if !should_handle_event(event, config, seen) {
        return Ok(ChiihouIncomingAction::Ignore);
    }
    if let Some(action) = classify_notification(&event.content)? {
        return Ok(action);
    }
    match build_reply_for_event(event, config, agent)? {
        Some(reply) => Ok(ChiihouIncomingAction::Reply(reply)),
        None => Ok(ChiihouIncomingAction::Ignore),
    }
}

pub(crate) fn classify_incoming_event_with_state<A: Agent>(
    event: &ChiihouIncomingEvent,
    config: &ChiihouEventConfig,
    seen: &mut SeenEventIds,
    state: &ChiihouTableSnapshot,
    agent: &mut A,
) -> Result<ChiihouIncomingAction, ChiihouEventError> {
    if !should_handle_event(event, config, seen) {
        return Ok(ChiihouIncomingAction::Ignore);
    }
    if let Some(action) = classify_notification(&event.content)? {
        return Ok(action);
    }
    match build_reply_for_event_with_state(event, config, state, agent)? {
        Some(reply) => Ok(ChiihouIncomingAction::Reply(reply)),
        None => Ok(ChiihouIncomingAction::Ignore),
    }
}

pub fn process_incoming_event<A: Agent>(
    event: &ChiihouIncomingEvent,
    config: &ChiihouEventConfig,
    seen: &mut SeenEventIds,
    agent: &mut A,
) -> Result<Option<ChiihouOutgoingReply>, ChiihouEventError> {
    match classify_incoming_event(event, config, seen, agent)? {
        ChiihouIncomingAction::Reply(reply) => Ok(Some(reply)),
        ChiihouIncomingAction::Ignore
        | ChiihouIncomingAction::Lifecycle(_)
        | ChiihouIncomingAction::TableNotification(_) => Ok(None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::handler::ChiihouHandlerError;
    use crate::protocol::ChiihouProtocolError;
    use bot_core::{GameContext, LegalAction};

    struct PickSecondAgent;

    impl Agent for PickSecondAgent {
        fn act(&mut self, _ctx: &GameContext, legal_actions: &[LegalAction]) -> LegalAction {
            legal_actions.get(1).cloned().unwrap_or(LegalAction::None)
        }
    }

    struct HoraAgent;

    impl Agent for HoraAgent {
        fn act(&mut self, _ctx: &GameContext, _legal_actions: &[LegalAction]) -> LegalAction {
            LegalAction::Hora
        }
    }

    fn config() -> ChiihouEventConfig {
        ChiihouEventConfig {
            ai_pubkey_hex: "ai_pubkey".to_string(),
            server_pubkey_hex: "server_pubkey".to_string(),
            server_npub: "npub1server".to_string(),
            channel_ids: vec!["channel_hanchan".to_string(), "channel_tonpuu".to_string()],
        }
    }

    fn tag(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| value.to_string()).collect()
    }

    fn valid_event(id: &str, content: &str) -> ChiihouIncomingEvent {
        ChiihouIncomingEvent {
            id: id.to_string(),
            pubkey: "server_pubkey".to_string(),
            kind: 42,
            tags: vec![
                tag(&["e", "channel_hanchan", "", "root"]),
                tag(&["p", "ai_pubkey"]),
            ],
            content: content.to_string(),
        }
    }

    fn sutehai_content() -> &'static str {
        "\
:mahjong_m1::mahjong_m2::mahjong_m3::mahjong_m4::mahjong_m5::mahjong_m6::mahjong_m7::mahjong_m8::mahjong_m9::mahjong_p1::mahjong_p2::mahjong_p3::mahjong_s1: :mahjong_east:
nostr:npub1ai000 GET sutehai?"
    }

    fn naku_content() -> &'static str {
        "\
:mahjong_m1::mahjong_m2::mahjong_m3: :mahjong_m4:
nostr:npub1ai000 GET naku? ron pon chi"
    }

    #[test]
    fn chiihou_request_kinds_are_42_and_20000() {
        assert!(is_chiihou_request_kind(42));
        assert!(is_chiihou_request_kind(20000));
        assert!(!is_chiihou_request_kind(30315));
        assert!(!is_chiihou_request_kind(1));
    }

    #[test]
    fn chiihou_kind_constants_match_request_kinds() {
        assert_eq!(CHIIHOU_CHANNEL_MESSAGE_KIND, 42);
        assert_eq!(CHIIHOU_BITCHAT_MESSAGE_KIND, 20000);
        assert!(is_chiihou_request_kind(u64::from(
            CHIIHOU_CHANNEL_MESSAGE_KIND
        )));
        assert!(is_chiihou_request_kind(u64::from(
            CHIIHOU_BITCHAT_MESSAGE_KIND
        )));
    }

    #[test]
    fn event_targets_ai_with_matching_p_tag() {
        let event = valid_event("event1", "");
        assert!(event_targets_ai(&event, "ai_pubkey"));
        assert!(!event_targets_ai(&event, "other_pubkey"));

        let mut event_without_p_tag = valid_event("event1", "");
        event_without_p_tag.tags = vec![tag(&["e", "channel_hanchan", "", "root"])];
        assert!(!event_targets_ai(&event_without_p_tag, "ai_pubkey"));
    }

    #[test]
    fn event_is_from_server_compares_pubkey() {
        let event = valid_event("event1", "");
        assert!(event_is_from_server(&event, "server_pubkey"));
        assert!(!event_is_from_server(&event, "other_pubkey"));
    }

    #[test]
    fn event_channel_id_returns_allowed_root_channel() {
        let event = valid_event("event1", "");
        assert_eq!(
            event_channel_id(&event, &config().channel_ids),
            Some("channel_hanchan")
        );
    }

    #[test]
    fn event_channel_id_is_none_for_unknown_channel() {
        let mut event = valid_event("event1", "");
        event.tags = vec![
            tag(&["e", "channel_unknown", "", "root"]),
            tag(&["p", "ai_pubkey"]),
        ];
        assert_eq!(event_channel_id(&event, &config().channel_ids), None);
    }

    #[test]
    fn event_channel_id_is_none_without_root_e_tag() {
        let mut event = valid_event("event1", "");
        event.tags = vec![tag(&["p", "ai_pubkey"])];
        assert_eq!(event_channel_id(&event, &config().channel_ids), None);
    }

    #[test]
    fn seen_event_ids_should_process_once() {
        let mut seen = SeenEventIds::new();
        assert!(seen.should_process("event1"));
        assert!(!seen.should_process("event1"));
        assert!(seen.should_process("event2"));
    }

    #[test]
    fn seen_event_ids_insert_and_contains() {
        let mut seen = SeenEventIds::new();
        assert!(!seen.contains("event1"));
        assert!(seen.insert("event1"));
        assert!(seen.contains("event1"));
        assert!(!seen.insert("event1"));
    }

    #[test]
    fn should_handle_event_accepts_valid_event() {
        let mut seen = SeenEventIds::new();
        let event = valid_event("event1", sutehai_content());
        assert!(should_handle_event(&event, &config(), &mut seen));
        assert!(seen.contains("event1"));
    }

    #[test]
    fn should_handle_event_accepts_kind_20000() {
        let mut seen = SeenEventIds::new();
        let mut event = valid_event("event1", sutehai_content());
        event.kind = 20000;
        assert!(should_handle_event(&event, &config(), &mut seen));
    }

    #[test]
    fn should_handle_event_rejects_wrong_kind() {
        let mut seen = SeenEventIds::new();
        let mut event = valid_event("event1", sutehai_content());
        event.kind = 1;
        assert!(!should_handle_event(&event, &config(), &mut seen));
        assert!(!seen.contains("event1"));
    }

    #[test]
    fn should_handle_event_rejects_wrong_server_pubkey() {
        let mut seen = SeenEventIds::new();
        let mut event = valid_event("event1", sutehai_content());
        event.pubkey = "other_pubkey".to_string();
        assert!(!should_handle_event(&event, &config(), &mut seen));
        assert!(!seen.contains("event1"));
    }

    #[test]
    fn should_handle_event_rejects_event_without_ai_p_tag() {
        let mut seen = SeenEventIds::new();
        let mut event = valid_event("event1", sutehai_content());
        event.tags = vec![tag(&["e", "channel_hanchan", "", "root"])];
        assert!(!should_handle_event(&event, &config(), &mut seen));
        assert!(!seen.contains("event1"));
    }

    #[test]
    fn should_handle_event_rejects_unknown_channel() {
        let mut seen = SeenEventIds::new();
        let mut event = valid_event("event1", sutehai_content());
        event.tags = vec![
            tag(&["e", "channel_unknown", "", "root"]),
            tag(&["p", "ai_pubkey"]),
        ];
        assert!(!should_handle_event(&event, &config(), &mut seen));
        assert!(!seen.contains("event1"));
    }

    #[test]
    fn should_handle_event_rejects_duplicate_event_id() {
        let mut seen = SeenEventIds::new();
        let event = valid_event("event1", sutehai_content());
        assert!(should_handle_event(&event, &config(), &mut seen));
        assert!(!should_handle_event(&event, &config(), &mut seen));
    }

    #[test]
    fn kind_42_reply_tags_have_only_nip28_tags() {
        let event = valid_event("event1", sutehai_content());
        let tags = build_reply_tags_for_event(&event, &config()).unwrap();
        assert_eq!(
            tags,
            vec![
                tag(&["e", "channel_hanchan", "", "root"]),
                tag(&["e", "event1", "", "reply", "server_pubkey"]),
                tag(&["p", "ai_pubkey"]),
                tag(&["p", "server_pubkey"]),
            ]
        );
        assert!(
            !tags
                .iter()
                .any(|tag| tag.first().is_some_and(|name| name == "g"))
        );
        assert!(
            !tags
                .iter()
                .any(|tag| tag.first().is_some_and(|name| name == "n"))
        );
        assert!(
            !tags
                .iter()
                .any(|tag| tag.first().is_some_and(|name| name == "t"))
        );
    }

    #[test]
    fn kind_42_reply_tags_ignore_incoming_g_tag() {
        let mut event = valid_event("event1", sutehai_content());
        event.tags.push(tag(&["g", "xn76"]));
        let tags = build_reply_tags_for_event(&event, &config()).unwrap();
        assert!(
            !tags
                .iter()
                .any(|tag| tag.first().is_some_and(|name| name == "g"))
        );
    }

    #[test]
    fn kind_20000_reply_tags_add_bitchat_tags() {
        let mut event = valid_event("server_event_id", sutehai_content());
        event.kind = 20000;
        event.tags = vec![
            tag(&["e", "channel_hanchan", "", "root"]),
            tag(&["p", "ai_pubkey"]),
            tag(&["g", "xn76"]),
            tag(&["n", "Server Bot"]),
        ];
        assert_eq!(
            build_reply_tags_for_event(&event, &config()).unwrap(),
            vec![
                tag(&["e", "channel_hanchan", "", "root"]),
                tag(&["e", "server_event_id", "", "reply", "server_pubkey"]),
                tag(&["p", "ai_pubkey"]),
                tag(&["p", "server_pubkey"]),
                tag(&["g", "xn76"]),
                tag(&["t", "teleport"]),
            ]
        );
    }

    #[test]
    fn kind_20000_reply_has_no_nickname_tag() {
        let mut event = valid_event("event1", sutehai_content());
        event.kind = 20000;
        event.tags.push(tag(&["g", "xn76"]));
        event.tags.push(tag(&["n", "Server Bot"]));
        let tags = build_reply_tags_for_event(&event, &config()).unwrap();
        assert!(
            !tags
                .iter()
                .any(|tag| tag.first().is_some_and(|name| name == "n"))
        );
        assert!(tags.contains(&tag(&["g", "xn76"])));
        assert!(tags.contains(&tag(&["t", "teleport"])));
    }

    #[test]
    fn kind_20000_reply_tags_do_not_copy_incoming_n_and_t_tags() {
        let mut event = valid_event("event1", sutehai_content());
        event.kind = 20000;
        event.tags.push(tag(&["n", "Server Bot"]));
        event.tags.push(tag(&["t", "mahjong"]));
        let tags = build_reply_tags_for_event(&event, &config()).unwrap();
        assert!(!tags.contains(&tag(&["n", "Server Bot"])));
        assert!(!tags.contains(&tag(&["t", "mahjong"])));
        assert_eq!(
            tags.iter()
                .filter(|t| t.first().is_some_and(|name| name == "n"))
                .count(),
            0
        );
        assert_eq!(
            tags.iter()
                .filter(|t| t.first().is_some_and(|name| name == "t"))
                .count(),
            1
        );
    }

    #[test]
    fn kind_20000_reply_tags_keep_multiple_g_tags_in_order() {
        let mut event = valid_event("event1", sutehai_content());
        event.kind = 20000;
        event.tags.push(tag(&["g", "xn76"]));
        event.tags.push(tag(&["g", "xn77"]));
        event.tags.push(tag(&["g", "xn78"]));
        let tags = build_reply_tags_for_event(&event, &config()).unwrap();
        let g_tags: Vec<&Vec<String>> = tags
            .iter()
            .filter(|t| t.first().is_some_and(|name| name == "g"))
            .collect();
        assert_eq!(
            g_tags,
            vec![
                &tag(&["g", "xn76"]),
                &tag(&["g", "xn77"]),
                &tag(&["g", "xn78"]),
            ]
        );
    }

    #[test]
    fn kind_20000_reply_does_not_copy_g_tag_without_value() {
        let mut event = valid_event("event1", sutehai_content());
        event.kind = 20000;
        event.tags.push(tag(&["g"]));
        event.tags.push(tag(&["g", "xn76"]));
        let tags = build_reply_tags_for_event(&event, &config()).unwrap();
        let g_tags: Vec<&Vec<String>> = tags
            .iter()
            .filter(|t| t.first().is_some_and(|name| name == "g"))
            .collect();
        assert_eq!(g_tags, vec![&tag(&["g", "xn76"])]);
    }

    #[test]
    fn kind_20000_reply_keeps_g_tag_with_extra_values() {
        let mut event = valid_event("event1", sutehai_content());
        event.kind = 20000;
        event.tags.push(tag(&["g", "xn76", "extra"]));
        let tags = build_reply_tags_for_event(&event, &config()).unwrap();
        assert!(tags.contains(&tag(&["g", "xn76", "extra"])));
    }

    #[test]
    fn kind_20000_reply_keeps_valid_g_tags_in_order_and_skips_bare_g_tag() {
        let mut event = valid_event("event1", sutehai_content());
        event.kind = 20000;
        event.tags.push(tag(&["g", "xn76"]));
        event.tags.push(tag(&["g"]));
        event.tags.push(tag(&["g", "xn77", "extra"]));
        event.tags.push(tag(&["g", "xn78"]));
        let tags = build_reply_tags_for_event(&event, &config()).unwrap();
        let g_tags: Vec<&Vec<String>> = tags
            .iter()
            .filter(|t| t.first().is_some_and(|name| name == "g"))
            .collect();
        assert_eq!(
            g_tags,
            vec![
                &tag(&["g", "xn76"]),
                &tag(&["g", "xn77", "extra"]),
                &tag(&["g", "xn78"]),
            ]
        );
    }

    #[test]
    fn build_reply_tags_for_event_is_none_without_allowed_channel() {
        let mut event = valid_event("event1", sutehai_content());
        event.tags = vec![
            tag(&["e", "channel_unknown", "", "root"]),
            tag(&["p", "ai_pubkey"]),
        ];
        assert_eq!(build_reply_tags_for_event(&event, &config()), None);
    }

    #[test]
    fn builds_reply_for_sutehai_event() {
        let event = valid_event("event1", sutehai_content());
        assert_eq!(
            build_reply_for_event(&event, &config(), &mut PickSecondAgent),
            Ok(Some(ChiihouOutgoingReply {
                kind: 42,
                tags: vec![
                    tag(&["e", "channel_hanchan", "", "root"]),
                    tag(&["e", "event1", "", "reply", "server_pubkey"]),
                    tag(&["p", "ai_pubkey"]),
                    tag(&["p", "server_pubkey"]),
                ],
                content: "nostr:npub1server sutehai? sutehai 2m".to_string(),
            }))
        );
    }

    fn complete_sutehai_content() -> &'static str {
        "\
:mahjong_m1::mahjong_m2::mahjong_m3::mahjong_m4::mahjong_m5::mahjong_m6::mahjong_m7::mahjong_m8::mahjong_m9::mahjong_p1::mahjong_p2::mahjong_p3::mahjong_p5: :mahjong_p5:
nostr:npub1ai000 GET sutehai?"
    }

    #[test]
    fn builds_tsumo_reply_for_complete_sutehai_event() {
        let event = valid_event("event1", complete_sutehai_content());
        assert_eq!(
            build_reply_for_event(&event, &config(), &mut HoraAgent),
            Ok(Some(ChiihouOutgoingReply {
                kind: 42,
                tags: vec![
                    tag(&["e", "channel_hanchan", "", "root"]),
                    tag(&["e", "event1", "", "reply", "server_pubkey"]),
                    tag(&["p", "ai_pubkey"]),
                    tag(&["p", "server_pubkey"]),
                ],
                content: "nostr:npub1server sutehai? tsumo".to_string(),
            }))
        );
    }

    #[test]
    fn builds_kind_20000_tsumo_reply_with_bitchat_tags() {
        let mut event = valid_event("event1", complete_sutehai_content());
        event.kind = 20000;
        event.tags.push(tag(&["g", "xn76"]));
        event.tags.push(tag(&["g"]));
        event.tags.push(tag(&["n", "Server Bot"]));
        let reply = build_reply_for_event(&event, &config(), &mut HoraAgent)
            .unwrap()
            .unwrap();
        assert_eq!(reply.kind, 20000);
        assert_eq!(reply.content, "nostr:npub1server sutehai? tsumo");
        assert!(reply.tags.contains(&tag(&["g", "xn76"])));
        assert!(!reply.tags.contains(&tag(&["g"])));
        assert!(reply.tags.contains(&tag(&["t", "teleport"])));
        assert!(
            !reply
                .tags
                .iter()
                .any(|tag| tag.first().is_some_and(|name| name == "n"))
        );
    }

    #[test]
    fn complete_sutehai_event_replies_dahai_when_agent_discards() {
        let event = valid_event("event1", complete_sutehai_content());
        let reply = build_reply_for_event(&event, &config(), &mut PickSecondAgent)
            .unwrap()
            .unwrap();
        assert_eq!(reply.content, "nostr:npub1server sutehai? sutehai 1m");
    }

    struct ReachAgent;

    impl Agent for ReachAgent {
        fn act(&mut self, _ctx: &GameContext, _legal_actions: &[LegalAction]) -> LegalAction {
            LegalAction::Reach
        }
    }

    fn richi_sutehai_content() -> &'static str {
        "\
:mahjong_m1::mahjong_m2::mahjong_m3::mahjong_m4::mahjong_m5::mahjong_m6::mahjong_m7::mahjong_m8::mahjong_m9::mahjong_p2::mahjong_p3::mahjong_s5::mahjong_s5: :mahjong_east:
nostr:npub1ai000 GET sutehai?"
    }

    fn richi_snapshot() -> ChiihouTableSnapshot {
        ChiihouTableSnapshot {
            player_id: Some(0),
            remaining_tiles: Some(30),
            ..ChiihouTableSnapshot::default()
        }
    }

    #[test]
    fn builds_kind_42_richi_reply_with_state() {
        let event = valid_event("event1", richi_sutehai_content());
        assert_eq!(
            build_reply_for_event_with_state(&event, &config(), &richi_snapshot(), &mut ReachAgent),
            Ok(Some(ChiihouOutgoingReply {
                kind: 42,
                tags: vec![
                    tag(&["e", "channel_hanchan", "", "root"]),
                    tag(&["e", "event1", "", "reply", "server_pubkey"]),
                    tag(&["p", "ai_pubkey"]),
                    tag(&["p", "server_pubkey"]),
                ],
                content: "nostr:npub1server sutehai? richi 1z".to_string(),
            }))
        );
    }

    #[test]
    fn builds_kind_20000_richi_reply_with_bitchat_tags() {
        let mut event = valid_event("event1", richi_sutehai_content());
        event.kind = 20000;
        event.tags.push(tag(&["g", "xn76"]));
        event.tags.push(tag(&["g"]));
        event.tags.push(tag(&["n", "Server Bot"]));
        let reply =
            build_reply_for_event_with_state(&event, &config(), &richi_snapshot(), &mut ReachAgent)
                .unwrap()
                .unwrap();
        assert_eq!(reply.kind, 20000);
        assert_eq!(reply.content, "nostr:npub1server sutehai? richi 1z");
        assert!(reply.tags.contains(&tag(&["g", "xn76"])));
        assert!(!reply.tags.contains(&tag(&["g"])));
        assert!(reply.tags.contains(&tag(&["t", "teleport"])));
        assert!(
            !reply
                .tags
                .iter()
                .any(|tag| tag.first().is_some_and(|name| name == "n"))
        );
    }

    #[test]
    fn richi_reply_keeps_request_kind() {
        for kind in [42u64, 20000] {
            let mut event = valid_event("event1", richi_sutehai_content());
            event.kind = kind;
            let reply = build_reply_for_event_with_state(
                &event,
                &config(),
                &richi_snapshot(),
                &mut ReachAgent,
            )
            .unwrap()
            .unwrap();
            assert_eq!(reply.kind, kind, "kind: {kind}");
        }
    }

    #[test]
    fn richi_hand_without_state_falls_back_to_dahai_reply() {
        let event = valid_event("event1", richi_sutehai_content());
        let reply = build_reply_for_event(&event, &config(), &mut ReachAgent)
            .unwrap()
            .unwrap();
        assert_eq!(reply.content, "nostr:npub1server sutehai? sutehai 1m");
    }

    #[test]
    fn builds_naku_no_reply_for_naku_event() {
        let event = valid_event("event1", naku_content());
        let reply = build_reply_for_event(&event, &config(), &mut PickSecondAgent)
            .unwrap()
            .unwrap();
        assert_eq!(reply.kind, 42);
        assert_eq!(reply.content, "nostr:npub1server naku? no");
    }

    #[test]
    fn builds_naku_ron_reply_for_naku_event() {
        let event = valid_event("event1", naku_content());
        assert_eq!(
            build_reply_for_event(&event, &config(), &mut HoraAgent),
            Ok(Some(ChiihouOutgoingReply {
                kind: 42,
                tags: vec![
                    tag(&["e", "channel_hanchan", "", "root"]),
                    tag(&["e", "event1", "", "reply", "server_pubkey"]),
                    tag(&["p", "ai_pubkey"]),
                    tag(&["p", "server_pubkey"]),
                ],
                content: "nostr:npub1server naku? ron".to_string(),
            }))
        );
    }

    #[test]
    fn builds_kind_20000_naku_ron_reply_with_bitchat_tags() {
        let mut event = valid_event("event1", naku_content());
        event.kind = 20000;
        event.tags.push(tag(&["g", "xn76"]));
        let reply = build_reply_for_event(&event, &config(), &mut HoraAgent)
            .unwrap()
            .unwrap();
        assert_eq!(reply.kind, 20000);
        assert_eq!(reply.content, "nostr:npub1server naku? ron");
        assert!(reply.tags.contains(&tag(&["g", "xn76"])));
        assert!(reply.tags.contains(&tag(&["t", "teleport"])));
        assert!(
            !reply
                .tags
                .iter()
                .any(|tag| tag.first().is_some_and(|name| name == "n"))
        );
    }

    #[test]
    fn builds_kind_20000_reply_with_bitchat_tags() {
        let mut event = valid_event("event1", sutehai_content());
        event.kind = 20000;
        event.tags.push(tag(&["g", "xn76"]));
        let reply = build_reply_for_event(&event, &config(), &mut PickSecondAgent)
            .unwrap()
            .unwrap();
        assert_eq!(reply.kind, 20000);
        assert!(reply.tags.contains(&tag(&["g", "xn76"])));
        assert!(
            !reply
                .tags
                .iter()
                .any(|tag| tag.first().is_some_and(|name| name == "n"))
        );
        assert!(reply.tags.contains(&tag(&["t", "teleport"])));
    }

    #[test]
    fn build_reply_is_none_for_unrelated_content() {
        let event = valid_event("event1", "nostr:npub1ai000 join");
        assert_eq!(
            build_reply_for_event(&event, &config(), &mut PickSecondAgent),
            Ok(None)
        );
    }

    #[test]
    fn build_reply_is_none_without_allowed_channel() {
        let mut event = valid_event("event1", sutehai_content());
        event.tags = vec![
            tag(&["e", "channel_unknown", "", "root"]),
            tag(&["p", "ai_pubkey"]),
        ];
        assert_eq!(
            build_reply_for_event(&event, &config(), &mut PickSecondAgent),
            Ok(None)
        );
    }

    #[test]
    fn processes_valid_sutehai_event() {
        let mut seen = SeenEventIds::new();
        let event = valid_event("event1", sutehai_content());
        assert_eq!(
            process_incoming_event(&event, &config(), &mut seen, &mut PickSecondAgent),
            Ok(Some(ChiihouOutgoingReply {
                kind: 42,
                tags: vec![
                    tag(&["e", "channel_hanchan", "", "root"]),
                    tag(&["e", "event1", "", "reply", "server_pubkey"]),
                    tag(&["p", "ai_pubkey"]),
                    tag(&["p", "server_pubkey"]),
                ],
                content: "nostr:npub1server sutehai? sutehai 2m".to_string(),
            }))
        );
    }

    #[test]
    fn processes_same_event_id_only_once() {
        let mut seen = SeenEventIds::new();
        let event = valid_event("event1", sutehai_content());
        assert!(
            process_incoming_event(&event, &config(), &mut seen, &mut PickSecondAgent)
                .unwrap()
                .is_some()
        );
        assert_eq!(
            process_incoming_event(&event, &config(), &mut seen, &mut PickSecondAgent),
            Ok(None)
        );
    }

    #[test]
    fn process_ignores_invalid_sender() {
        let mut seen = SeenEventIds::new();
        let mut event = valid_event("event1", sutehai_content());
        event.pubkey = "other_pubkey".to_string();
        assert_eq!(
            process_incoming_event(&event, &config(), &mut seen, &mut PickSecondAgent),
            Ok(None)
        );
    }

    #[test]
    fn process_ignores_invalid_channel() {
        let mut seen = SeenEventIds::new();
        let mut event = valid_event("event1", sutehai_content());
        event.tags = vec![
            tag(&["e", "channel_unknown", "", "root"]),
            tag(&["p", "ai_pubkey"]),
        ];
        assert_eq!(
            process_incoming_event(&event, &config(), &mut seen, &mut PickSecondAgent),
            Ok(None)
        );
    }

    #[test]
    fn process_ignores_event_without_ai_p_tag() {
        let mut seen = SeenEventIds::new();
        let mut event = valid_event("event1", sutehai_content());
        event.tags = vec![tag(&["e", "channel_hanchan", "", "root"])];
        assert_eq!(
            process_incoming_event(&event, &config(), &mut seen, &mut PickSecondAgent),
            Ok(None)
        );
    }

    fn lifecycle_players() -> Vec<nostr_sdk::PublicKey> {
        // テスト専用の秘密鍵から鍵を導出する。実際の運用で使用してはならない。
        (1..=4u64)
            .map(|index| {
                nostr_sdk::Keys::parse(&format!("{index:064x}"))
                    .unwrap()
                    .public_key()
            })
            .collect()
    }

    fn gamestart_content() -> String {
        use nostr_sdk::ToBech32;
        let players = lifecycle_players()
            .iter()
            .map(|player| format!("nostr:{}", player.to_bech32().unwrap()))
            .collect::<Vec<_>>()
            .join(" ");
        format!("NOTIFY gamestart 東 {players}")
    }

    #[test]
    fn classifies_valid_lifecycle_event() {
        let mut seen = SeenEventIds::new();
        let event = valid_event("event1", &gamestart_content());
        assert_eq!(
            classify_incoming_event(&event, &config(), &mut seen, &mut PickSecondAgent),
            Ok(ChiihouIncomingAction::Lifecycle(
                ChiihouLifecycleNotification::GameStart {
                    seat: crate::lifecycle::ChiihouWind::East,
                    players: lifecycle_players(),
                }
            ))
        );
    }

    #[test]
    fn classifies_kyokuend_event() {
        let mut seen = SeenEventIds::new();
        let event = valid_event("event1", "NOTIFY kyokuend");
        assert_eq!(
            classify_incoming_event(&event, &config(), &mut seen, &mut PickSecondAgent),
            Ok(ChiihouIncomingAction::Lifecycle(
                ChiihouLifecycleNotification::KyokuEnd
            ))
        );
    }

    #[test]
    fn classifies_sutehai_request_as_reply() {
        let mut seen = SeenEventIds::new();
        let event = valid_event("event1", sutehai_content());
        let action =
            classify_incoming_event(&event, &config(), &mut seen, &mut PickSecondAgent).unwrap();
        assert!(matches!(action, ChiihouIncomingAction::Reply(_)));
    }

    #[test]
    fn classifies_naku_request_as_reply() {
        let mut seen = SeenEventIds::new();
        let event = valid_event("event1", naku_content());
        let ChiihouIncomingAction::Reply(reply) =
            classify_incoming_event(&event, &config(), &mut seen, &mut PickSecondAgent).unwrap()
        else {
            panic!("expected reply action");
        };
        assert_eq!(reply.content, "nostr:npub1server naku? no");
    }

    #[test]
    fn classify_ignores_unsupported_notify() {
        for content in [
            "NOTIFY point payload",
            "NOTIFY agari payload",
            "NOTIFY ryukyoku payload",
        ] {
            let mut seen = SeenEventIds::new();
            let event = valid_event("event1", content);
            assert_eq!(
                classify_incoming_event(&event, &config(), &mut seen, &mut PickSecondAgent),
                Ok(ChiihouIncomingAction::Ignore),
                "content: {content:?}"
            );
        }
    }

    fn ai_npub_token() -> String {
        use nostr_sdk::ToBech32;
        format!("nostr:{}", lifecycle_players()[0].to_bech32().unwrap())
    }

    #[test]
    fn classifies_table_notification_event() {
        let mut seen = SeenEventIds::new();
        let content = format!("{} NOTIFY dora 5p", ai_npub_token());
        let event = valid_event("event1", &content);
        let action =
            classify_incoming_event(&event, &config(), &mut seen, &mut PickSecondAgent).unwrap();
        assert!(matches!(
            action,
            ChiihouIncomingAction::TableNotification(ChiihouTableNotification::Dora { .. })
        ));
    }

    #[test]
    fn classify_returns_table_notification_error_for_malformed_table_notify() {
        let mut seen = SeenEventIds::new();
        let event = valid_event("event1", "NOTIFY tsumo :mahjong_m1:");
        assert_eq!(
            classify_incoming_event(&event, &config(), &mut seen, &mut PickSecondAgent),
            Err(ChiihouEventError::TableNotification(
                ChiihouTableNotificationError::InvalidPublicKey
            ))
        );
    }

    #[test]
    fn classify_ignores_table_notification_from_other_server() {
        let mut seen = SeenEventIds::new();
        let content = format!("{} NOTIFY dora 5p", ai_npub_token());
        let mut event = valid_event("event1", &content);
        event.pubkey = "other_pubkey".to_string();
        assert_eq!(
            classify_incoming_event(&event, &config(), &mut seen, &mut PickSecondAgent),
            Ok(ChiihouIncomingAction::Ignore)
        );
    }

    #[test]
    fn classify_processes_same_table_notification_event_id_only_once() {
        let mut seen = SeenEventIds::new();
        let content = format!("{} NOTIFY dora 5p", ai_npub_token());
        let event = valid_event("event1", &content);
        assert!(matches!(
            classify_incoming_event(&event, &config(), &mut seen, &mut PickSecondAgent),
            Ok(ChiihouIncomingAction::TableNotification(_))
        ));
        assert_eq!(
            classify_incoming_event(&event, &config(), &mut seen, &mut PickSecondAgent),
            Ok(ChiihouIncomingAction::Ignore)
        );
    }

    #[test]
    fn process_returns_none_for_table_notification_event() {
        let mut seen = SeenEventIds::new();
        let content = format!("{} NOTIFY dora 5p", ai_npub_token());
        let event = valid_event("event1", &content);
        assert_eq!(
            process_incoming_event(&event, &config(), &mut seen, &mut PickSecondAgent),
            Ok(None)
        );
    }

    #[test]
    fn classify_with_state_passes_snapshot_to_agent_context() {
        use crate::match_state::ChiihouTableSnapshot;
        use crate::protocol::ChiihouPai;

        struct RecordingAgent {
            context: Option<GameContext>,
        }

        impl Agent for RecordingAgent {
            fn act(&mut self, ctx: &GameContext, legal_actions: &[LegalAction]) -> LegalAction {
                self.context = Some(ctx.clone());
                legal_actions.first().cloned().unwrap_or(LegalAction::None)
            }
        }

        let dora: ChiihouPai = "5p".parse().unwrap();
        let state = ChiihouTableSnapshot {
            dora_indicators: vec![dora],
            player_id: Some(2),
            ..ChiihouTableSnapshot::default()
        };
        let mut seen = SeenEventIds::new();
        let event = valid_event("event1", sutehai_content());
        let mut agent = RecordingAgent { context: None };
        let action =
            classify_incoming_event_with_state(&event, &config(), &mut seen, &state, &mut agent)
                .unwrap();
        assert!(matches!(action, ChiihouIncomingAction::Reply(_)));
        let context = agent.context.unwrap();
        assert_eq!(context.player_id(), Some(2));
        assert_eq!(
            context.dora_indicators(),
            &[crate::convert::temporary_tile_id_from_chiihou_pai(dora)]
        );
    }

    #[test]
    fn classify_returns_lifecycle_error_for_malformed_notify() {
        let mut seen = SeenEventIds::new();
        let event = valid_event("event1", "NOTIFY gamestart");
        assert_eq!(
            classify_incoming_event(&event, &config(), &mut seen, &mut PickSecondAgent),
            Err(ChiihouEventError::Lifecycle(
                ChiihouLifecycleError::MissingSeat
            ))
        );
    }

    #[test]
    fn classify_ignores_lifecycle_event_from_other_server() {
        let mut seen = SeenEventIds::new();
        let mut event = valid_event("event1", &gamestart_content());
        event.pubkey = "other_pubkey".to_string();
        assert_eq!(
            classify_incoming_event(&event, &config(), &mut seen, &mut PickSecondAgent),
            Ok(ChiihouIncomingAction::Ignore)
        );
    }

    #[test]
    fn classify_ignores_lifecycle_event_for_other_channel() {
        let mut seen = SeenEventIds::new();
        let mut event = valid_event("event1", &gamestart_content());
        event.tags = vec![
            tag(&["e", "channel_unknown", "", "root"]),
            tag(&["p", "ai_pubkey"]),
        ];
        assert_eq!(
            classify_incoming_event(&event, &config(), &mut seen, &mut PickSecondAgent),
            Ok(ChiihouIncomingAction::Ignore)
        );
    }

    #[test]
    fn classify_ignores_lifecycle_event_without_ai_p_tag() {
        let mut seen = SeenEventIds::new();
        let mut event = valid_event("event1", &gamestart_content());
        event.tags = vec![tag(&["e", "channel_hanchan", "", "root"])];
        assert_eq!(
            classify_incoming_event(&event, &config(), &mut seen, &mut PickSecondAgent),
            Ok(ChiihouIncomingAction::Ignore)
        );
    }

    #[test]
    fn classify_ignores_lifecycle_event_with_unsupported_kind() {
        let mut seen = SeenEventIds::new();
        let mut event = valid_event("event1", &gamestart_content());
        event.kind = 1;
        assert_eq!(
            classify_incoming_event(&event, &config(), &mut seen, &mut PickSecondAgent),
            Ok(ChiihouIncomingAction::Ignore)
        );
    }

    #[test]
    fn classify_processes_same_lifecycle_event_id_only_once() {
        let mut seen = SeenEventIds::new();
        let event = valid_event("event1", "NOTIFY kyokuend");
        assert!(matches!(
            classify_incoming_event(&event, &config(), &mut seen, &mut PickSecondAgent),
            Ok(ChiihouIncomingAction::Lifecycle(_))
        ));
        assert_eq!(
            classify_incoming_event(&event, &config(), &mut seen, &mut PickSecondAgent),
            Ok(ChiihouIncomingAction::Ignore)
        );
    }

    #[test]
    fn process_returns_none_for_valid_lifecycle_event() {
        let mut seen = SeenEventIds::new();
        let event = valid_event("event1", "NOTIFY kyokuend");
        assert_eq!(
            process_incoming_event(&event, &config(), &mut seen, &mut PickSecondAgent),
            Ok(None)
        );
    }

    #[test]
    fn invalid_hand_tile_count_sutehai_event_gets_no_reply() {
        use crate::decision::SutehaiDecisionError;
        let mut seen = SeenEventIds::new();
        let event = valid_event(
            "event1",
            "\
:mahjong_m1::mahjong_m2::mahjong_m3: :mahjong_east:
nostr:npub1ai000 GET sutehai?",
        );
        assert_eq!(
            process_incoming_event(&event, &config(), &mut seen, &mut PickSecondAgent),
            Err(ChiihouEventError::Handler(ChiihouHandlerError::Sutehai(
                SutehaiDecisionError::InvalidHandTileCount(3)
            )))
        );
    }

    #[test]
    fn process_returns_handler_error_for_unparsable_content() {
        let mut seen = SeenEventIds::new();
        let event = valid_event(
            "event1",
            "\
:mahjong_m1::mahjong_m2: :mahjong_m3::mahjong_m4:
nostr:npub1ai000 GET sutehai?",
        );
        assert_eq!(
            process_incoming_event(&event, &config(), &mut seen, &mut PickSecondAgent),
            Err(ChiihouEventError::Handler(ChiihouHandlerError::Protocol(
                ChiihouProtocolError::InvalidTileLayout
            )))
        );
    }
}
