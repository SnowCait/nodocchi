use nostr_sdk::prelude::{Event, EventBuilder, FinalizeEvent, Keys, Kind, Tag};

use crate::event::{ChiihouIncomingEvent, ChiihouOutgoingReply};

#[derive(Debug, thiserror::Error, Clone, PartialEq, Eq)]
pub enum ChiihouNostrAdapterError {
    #[error("nostr kind is out of range: {0}")]
    KindOutOfRange(u64),

    #[error("invalid nostr tag at index {index}: {message}")]
    InvalidTag { index: usize, message: String },

    #[error("failed to sign nostr event: {0}")]
    Sign(String),
}

pub fn incoming_event_from_nostr(event: &Event) -> ChiihouIncomingEvent {
    ChiihouIncomingEvent {
        id: event.id.to_hex(),
        pubkey: event.pubkey.to_hex(),
        kind: u64::from(event.kind.as_u16()),
        tags: event
            .tags
            .iter()
            .map(|tag| tag.as_slice().to_vec())
            .collect(),
        content: event.content.clone(),
    }
}

pub fn nostr_tags_from_strings(tags: &[Vec<String>]) -> Result<Vec<Tag>, ChiihouNostrAdapterError> {
    tags.iter()
        .enumerate()
        .map(|(index, tag)| {
            Tag::parse(tag).map_err(|error| ChiihouNostrAdapterError::InvalidTag {
                index,
                message: error.to_string(),
            })
        })
        .collect()
}

pub fn sign_outgoing_event(
    kind: u64,
    tags: &[Vec<String>],
    content: &str,
    keys: &Keys,
) -> Result<Event, ChiihouNostrAdapterError> {
    let kind = u16::try_from(kind).map_err(|_| ChiihouNostrAdapterError::KindOutOfRange(kind))?;
    let tags = nostr_tags_from_strings(tags)?;
    EventBuilder::new(Kind::from_u16(kind), content)
        .tags(tags)
        .finalize(keys)
        .map_err(|error| ChiihouNostrAdapterError::Sign(error.to_string()))
}

pub fn sign_outgoing_reply(
    reply: &ChiihouOutgoingReply,
    keys: &Keys,
) -> Result<Event, ChiihouNostrAdapterError> {
    sign_outgoing_event(reply.kind, &reply.tags, &reply.content, keys)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tags::{build_reply_tags, has_tag_value};

    // テスト専用の秘密鍵。実際の運用で使用してはならない。
    const TEST_AI_SECRET_KEY_HEX: &str =
        "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

    // テスト専用の秘密鍵。server 側 event の生成のみに使用する。
    const TEST_SERVER_SECRET_KEY_HEX: &str =
        "fedcba9876543210fedcba9876543210fedcba9876543210fedcba9876543210";

    fn ai_keys() -> Keys {
        Keys::parse(TEST_AI_SECRET_KEY_HEX).unwrap()
    }

    fn server_keys() -> Keys {
        Keys::parse(TEST_SERVER_SECRET_KEY_HEX).unwrap()
    }

    fn string_tag(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| value.to_string()).collect()
    }

    fn request_tags() -> Vec<Vec<String>> {
        vec![
            string_tag(&["e", "channel0000", "", "root"]),
            string_tag(&["p", &ai_keys().public_key().to_hex()]),
        ]
    }

    fn build_nostr_event(kind: u16, content: &str, tags: &[Vec<String>]) -> Event {
        EventBuilder::new(Kind::from_u16(kind), content)
            .tags(nostr_tags_from_strings(tags).unwrap())
            .finalize(&server_keys())
            .unwrap()
    }

    fn reply_tags() -> Vec<Vec<String>> {
        build_reply_tags(
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            "channel0000",
            &ai_keys().public_key().to_hex(),
            &server_keys().public_key().to_hex(),
        )
    }

    fn reply(kind: u64) -> ChiihouOutgoingReply {
        ChiihouOutgoingReply {
            kind,
            tags: reply_tags(),
            content: "nostr:npub1server sutehai? sutehai 2m".to_string(),
        }
    }

    #[test]
    fn incoming_event_id_is_hex() {
        let event = build_nostr_event(42, "content", &request_tags());
        let incoming = incoming_event_from_nostr(&event);
        assert_eq!(incoming.id, event.id.to_hex());
        assert_eq!(incoming.id.len(), 64);
        assert!(incoming.id.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn incoming_event_pubkey_is_hex() {
        let event = build_nostr_event(42, "content", &request_tags());
        let incoming = incoming_event_from_nostr(&event);
        assert_eq!(incoming.pubkey, server_keys().public_key().to_hex());
        assert_eq!(incoming.pubkey.len(), 64);
        assert!(incoming.pubkey.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn incoming_event_keeps_kind_42() {
        let event = build_nostr_event(42, "content", &request_tags());
        assert_eq!(incoming_event_from_nostr(&event).kind, 42);
    }

    #[test]
    fn incoming_event_keeps_kind_20000() {
        let event = build_nostr_event(20000, "content", &request_tags());
        assert_eq!(incoming_event_from_nostr(&event).kind, 20000);
    }

    #[test]
    fn incoming_event_keeps_content() {
        let content = ":mahjong_m1::mahjong_m2: nostr:npub1ai000 GET sutehai?";
        let event = build_nostr_event(42, content, &request_tags());
        assert_eq!(incoming_event_from_nostr(&event).content, content);
    }

    #[test]
    fn incoming_event_keeps_tag_order_and_values() {
        let tags = vec![
            string_tag(&["e", "channel0000", "", "root"]),
            string_tag(&["e", "request0000", "", "reply", "pubkey0000"]),
            string_tag(&["p", &ai_keys().public_key().to_hex()]),
            string_tag(&["p", &server_keys().public_key().to_hex()]),
        ];
        let event = build_nostr_event(42, "content", &tags);
        assert_eq!(incoming_event_from_nostr(&event).tags, tags);
    }

    #[test]
    fn converts_e_tag() {
        let tags = vec![string_tag(&["e", "event0000"])];
        let converted = nostr_tags_from_strings(&tags).unwrap();
        assert_eq!(converted.len(), 1);
        assert_eq!(converted[0].as_slice(), &["e", "event0000"]);
    }

    #[test]
    fn converts_p_tag() {
        let tags = vec![string_tag(&["p", "pubkey0000"])];
        let converted = nostr_tags_from_strings(&tags).unwrap();
        assert_eq!(converted.len(), 1);
        assert_eq!(converted[0].as_slice(), &["p", "pubkey0000"]);
    }

    #[test]
    fn converts_reply_e_tag_with_five_values() {
        let tags = vec![string_tag(&["e", "event0000", "", "reply", "pubkey0000"])];
        let converted = nostr_tags_from_strings(&tags).unwrap();
        assert_eq!(
            converted[0].as_slice(),
            &["e", "event0000", "", "reply", "pubkey0000"]
        );
    }

    #[test]
    fn empty_tag_is_error() {
        let tags = vec![string_tag(&["e", "event0000"]), Vec::new()];
        assert!(matches!(
            nostr_tags_from_strings(&tags),
            Err(ChiihouNostrAdapterError::InvalidTag { index: 1, .. })
        ));
    }

    #[test]
    fn keeps_order_of_multiple_tags() {
        let tags = vec![
            string_tag(&["e", "channel0000", "", "root"]),
            string_tag(&["e", "request0000", "", "reply", "pubkey0000"]),
            string_tag(&["p", "pubkey0001"]),
            string_tag(&["p", "pubkey0002"]),
        ];
        let converted = nostr_tags_from_strings(&tags).unwrap();
        let restored: Vec<Vec<String>> = converted
            .iter()
            .map(|tag| tag.as_slice().to_vec())
            .collect();
        assert_eq!(restored, tags);
    }

    #[test]
    fn signed_reply_keeps_kind_42() {
        let event = sign_outgoing_reply(&reply(42), &ai_keys()).unwrap();
        assert_eq!(event.kind, Kind::from_u16(42));
    }

    #[test]
    fn signed_reply_keeps_kind_20000() {
        let event = sign_outgoing_reply(&reply(20000), &ai_keys()).unwrap();
        assert_eq!(event.kind, Kind::from_u16(20000));
    }

    fn richi_reply(kind: u64) -> ChiihouOutgoingReply {
        ChiihouOutgoingReply {
            kind,
            tags: reply_tags(),
            content: "nostr:npub1server sutehai? richi 5p".to_string(),
        }
    }

    #[test]
    fn signed_richi_reply_keeps_kind_42() {
        let event = sign_outgoing_reply(&richi_reply(42), &ai_keys()).unwrap();
        assert_eq!(event.kind, Kind::from_u16(42));
        assert_eq!(event.content, "nostr:npub1server sutehai? richi 5p");
        assert!(event.verify().is_ok());
    }

    #[test]
    fn signed_richi_reply_keeps_kind_20000() {
        let event = sign_outgoing_reply(&richi_reply(20000), &ai_keys()).unwrap();
        assert_eq!(event.kind, Kind::from_u16(20000));
        assert_eq!(event.content, "nostr:npub1server sutehai? richi 5p");
        assert!(event.verify().is_ok());
    }

    #[test]
    fn signed_richi_reply_keeps_tag_order_and_values() {
        let reply = richi_reply(42);
        let event = sign_outgoing_reply(&reply, &ai_keys()).unwrap();
        assert_eq!(incoming_event_from_nostr(&event).tags, reply.tags);
    }

    #[test]
    fn signed_reply_keeps_content() {
        let reply = reply(42);
        let event = sign_outgoing_reply(&reply, &ai_keys()).unwrap();
        assert_eq!(event.content, reply.content);
    }

    #[test]
    fn signed_reply_uses_ai_pubkey() {
        let event = sign_outgoing_reply(&reply(42), &ai_keys()).unwrap();
        assert_eq!(event.pubkey, ai_keys().public_key());
    }

    #[test]
    fn signed_reply_verifies() {
        let event = sign_outgoing_reply(&reply(42), &ai_keys()).unwrap();
        assert!(event.verify().is_ok());
    }

    #[test]
    fn signed_reply_keeps_tag_order_and_values() {
        let reply = reply(42);
        let event = sign_outgoing_reply(&reply, &ai_keys()).unwrap();
        assert_eq!(incoming_event_from_nostr(&event).tags, reply.tags);
    }

    #[test]
    fn signed_reply_keeps_root_and_reply_e_tags() {
        let event = sign_outgoing_reply(&reply(42), &ai_keys()).unwrap();
        let tags = incoming_event_from_nostr(&event).tags;
        assert!(has_tag_value(&tags, "e", "channel0000"));
        assert!(has_tag_value(
            &tags,
            "e",
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
        ));
    }

    #[test]
    fn signed_reply_keeps_ai_and_server_p_tags() {
        let event = sign_outgoing_reply(&reply(42), &ai_keys()).unwrap();
        let tags = incoming_event_from_nostr(&event).tags;
        assert!(has_tag_value(&tags, "p", &ai_keys().public_key().to_hex()));
        assert!(has_tag_value(
            &tags,
            "p",
            &server_keys().public_key().to_hex()
        ));
    }

    #[test]
    fn kind_over_u16_max_is_error() {
        let result = sign_outgoing_reply(&reply(70000), &ai_keys());
        assert_eq!(result, Err(ChiihouNostrAdapterError::KindOutOfRange(70000)));
    }

    #[test]
    fn reply_with_empty_tag_is_error() {
        let mut reply = reply(42);
        reply.tags.insert(2, Vec::new());
        assert!(matches!(
            sign_outgoing_reply(&reply, &ai_keys()),
            Err(ChiihouNostrAdapterError::InvalidTag { index: 2, .. })
        ));
    }
}
