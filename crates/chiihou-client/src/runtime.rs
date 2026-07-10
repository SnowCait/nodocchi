use std::collections::{HashMap, HashSet};

use bot_core::Agent;
use nostr_sdk::async_utility::tokio::sync::broadcast::error::RecvError;
use nostr_sdk::{
    Client, Event, EventId, Filter, Kind, RelayPoolNotification, RelayUrl, SubscriptionId,
    Timestamp,
};

use crate::config::ChiihouNostrConfig;
use crate::event::{
    CHIIHOU_BITCHAT_MESSAGE_KIND, CHIIHOU_CHANNEL_MESSAGE_KIND, ChiihouEventError, SeenEventIds,
    process_incoming_event,
};
use crate::nostr_adapter::{
    ChiihouNostrAdapterError, incoming_event_from_nostr, sign_outgoing_reply,
};

#[derive(Debug, thiserror::Error)]
pub enum ChiihouRuntimeError {
    #[error("invalid chiihou channel ID: {0}")]
    InvalidChannelId(String),

    #[error("failed to add relay {relay_url}: {message}")]
    AddRelay { relay_url: String, message: String },

    #[error("failed to subscribe to chiihou requests: {0}")]
    Subscribe(String),

    #[error("failed to process chiihou event: {0}")]
    Event(#[from] ChiihouEventError),

    #[error("failed to convert or sign chiihou reply: {0}")]
    Adapter(#[from] ChiihouNostrAdapterError),

    #[error("failed to publish chiihou reply: {0}")]
    Publish(String),
}

pub fn build_chiihou_request_filter(
    config: &ChiihouNostrConfig,
    since: Timestamp,
) -> Result<Filter, ChiihouRuntimeError> {
    let channel_ids = config
        .event_config()
        .channel_ids
        .iter()
        .map(|channel_id| {
            EventId::from_hex(channel_id)
                .map_err(|_| ChiihouRuntimeError::InvalidChannelId(channel_id.clone()))
        })
        .collect::<Result<Vec<_>, _>>()?;

    Ok(Filter::new()
        .kinds([
            Kind::from_u16(CHIIHOU_CHANNEL_MESSAGE_KIND),
            Kind::from_u16(CHIIHOU_BITCHAT_MESSAGE_KIND),
        ])
        .pubkey(config.keys().public_key())
        .events(channel_ids)
        .since(since))
}

pub fn process_and_sign_nostr_event<A: Agent>(
    event: &Event,
    config: &ChiihouNostrConfig,
    seen: &mut SeenEventIds,
    agent: &mut A,
) -> Result<Option<Event>, ChiihouRuntimeError> {
    let incoming = incoming_event_from_nostr(event);
    let Some(reply) = process_incoming_event(&incoming, config.event_config(), seen, agent)? else {
        return Ok(None);
    };
    let signed = sign_outgoing_reply(&reply, config.keys())?;
    Ok(Some(signed))
}

pub async fn connect_chiihou_client(
    config: &ChiihouNostrConfig,
) -> Result<Client, ChiihouRuntimeError> {
    let client = Client::default();

    for relay_url in config.relay_urls() {
        client
            .add_relay(relay_url.as_str())
            .await
            .map_err(|error| ChiihouRuntimeError::AddRelay {
                relay_url: relay_url.clone(),
                message: error.to_string(),
            })?;
    }

    client.connect().await;

    Ok(client)
}

fn ensure_any_relay_succeeded(
    operation: &str,
    success: &HashSet<RelayUrl>,
    failed: &HashMap<RelayUrl, String>,
) -> Result<(), String> {
    if success.is_empty() {
        return Err(format!("{operation} failed on all relays: {failed:?}"));
    }
    Ok(())
}

pub async fn subscribe_chiihou_requests(
    client: &Client,
    config: &ChiihouNostrConfig,
    since: Timestamp,
) -> Result<SubscriptionId, ChiihouRuntimeError> {
    let filter = build_chiihou_request_filter(config, since)?;

    let output = client
        .subscribe(filter, None)
        .await
        .map_err(|error| ChiihouRuntimeError::Subscribe(error.to_string()))?;

    ensure_any_relay_succeeded("subscription", &output.success, &output.failed)
        .map_err(ChiihouRuntimeError::Subscribe)?;

    if !output.failed.is_empty() {
        tracing::warn!(
            failed_relays = ?output.failed,
            "chiihou subscription failed on some relays"
        );
    }

    Ok(output.val)
}

pub async fn publish_chiihou_reply(
    client: &Client,
    event: &Event,
) -> Result<(), ChiihouRuntimeError> {
    let output = client
        .send_event(event)
        .await
        .map_err(|error| ChiihouRuntimeError::Publish(error.to_string()))?;

    ensure_any_relay_succeeded("reply publish", &output.success, &output.failed)
        .map_err(ChiihouRuntimeError::Publish)?;

    if !output.failed.is_empty() {
        tracing::warn!(
            event_id = %event.id,
            failed_relays = ?output.failed,
            "chiihou reply publish failed on some relays"
        );
    }

    Ok(())
}

pub async fn run_chiihou_client<A: Agent>(
    config: &ChiihouNostrConfig,
    agent: &mut A,
) -> Result<(), ChiihouRuntimeError> {
    let since = Timestamp::now();
    let client = connect_chiihou_client(config).await?;

    let mut notifications = client.notifications();

    let subscription_id = subscribe_chiihou_requests(&client, config, since).await?;

    let mut seen = SeenEventIds::new();

    loop {
        match notifications.recv().await {
            Ok(RelayPoolNotification::Event {
                subscription_id: received_subscription_id,
                event,
                ..
            }) if received_subscription_id == subscription_id => {
                match process_and_sign_nostr_event(&event, config, &mut seen, agent) {
                    Ok(Some(reply)) => {
                        publish_chiihou_reply(&client, &reply).await?;
                    }
                    Ok(None) => {}
                    Err(error) => {
                        tracing::warn!(
                            event_id = %event.id,
                            error = %error,
                            "failed to process chiihou event"
                        );
                    }
                }
            }
            Ok(RelayPoolNotification::Shutdown) | Err(RecvError::Closed) => break,
            Ok(_) => {}
            Err(RecvError::Lagged(skipped)) => {
                tracing::warn!(skipped, "chiihou notification receiver lagged");
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{ChiihouChannel, HANCHAN_CHANNEL_ID, TONPUU_CHANNEL_ID};
    use crate::nostr_adapter::nostr_tags_from_strings;
    use bot_core::{GameContext, LegalAction};
    use nostr_sdk::{Alphabet, EventBuilder, Keys, SingleLetterTag, ToBech32};

    // テスト専用の秘密鍵。実際の運用で使用してはならない。
    const TEST_AI_SECRET_KEY_HEX: &str =
        "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

    // テスト専用の秘密鍵。server 側 event の生成のみに使用する。
    const TEST_SERVER_SECRET_KEY_HEX: &str =
        "fedcba9876543210fedcba9876543210fedcba9876543210fedcba9876543210";

    struct PickSecondAgent;

    impl Agent for PickSecondAgent {
        fn act(&mut self, _ctx: &GameContext, legal_actions: &[LegalAction]) -> LegalAction {
            legal_actions.get(1).cloned().unwrap_or(LegalAction::None)
        }
    }

    fn ai_keys() -> Keys {
        Keys::parse(TEST_AI_SECRET_KEY_HEX).unwrap()
    }

    fn server_keys() -> Keys {
        Keys::parse(TEST_SERVER_SECRET_KEY_HEX).unwrap()
    }

    fn config(channel: ChiihouChannel) -> ChiihouNostrConfig {
        ChiihouNostrConfig::new(
            TEST_AI_SECRET_KEY_HEX,
            &server_keys().public_key().to_hex(),
            channel,
        )
        .unwrap()
    }

    fn string_tag(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| value.to_string()).collect()
    }

    fn request_tags() -> Vec<Vec<String>> {
        vec![
            string_tag(&["e", HANCHAN_CHANNEL_ID, "", "root"]),
            string_tag(&["p", &ai_keys().public_key().to_hex()]),
        ]
    }

    fn build_request_event(kind: u16, content: &str, tags: &[Vec<String>], keys: &Keys) -> Event {
        EventBuilder::new(Kind::from_u16(kind), content)
            .tags(nostr_tags_from_strings(tags).unwrap())
            .allow_self_tagging()
            .sign_with_keys(keys)
            .unwrap()
    }

    fn sutehai_content() -> String {
        let ai_npub = ai_keys().public_key().to_bech32().unwrap();
        format!(
            "\
:mahjong_m1::mahjong_m2::mahjong_m3: :mahjong_east:
nostr:{ai_npub} GET sutehai?"
        )
    }

    fn naku_content() -> String {
        let ai_npub = ai_keys().public_key().to_bech32().unwrap();
        format!(
            "\
:mahjong_m1::mahjong_m2::mahjong_m3: :mahjong_m4:
nostr:{ai_npub} GET naku? ron pon chi"
        )
    }

    fn tag_values(tags: &[Vec<String>], tag_name: &str) -> Vec<Vec<String>> {
        tags.iter()
            .filter(|tag| tag.first().is_some_and(|name| name == tag_name))
            .cloned()
            .collect()
    }

    #[test]
    fn filter_has_kinds_42_and_20000() {
        let since = Timestamp::from(1_760_000_000u64);
        let filter = build_chiihou_request_filter(&config(ChiihouChannel::Hanchan), since).unwrap();
        let kinds = filter.kinds.unwrap();
        assert_eq!(kinds.len(), 2);
        assert!(kinds.contains(&Kind::from_u16(42)));
        assert!(kinds.contains(&Kind::from_u16(20000)));
    }

    #[test]
    fn filter_has_ai_pubkey_in_p_tag() {
        let since = Timestamp::from(1_760_000_000u64);
        let filter = build_chiihou_request_filter(&config(ChiihouChannel::Hanchan), since).unwrap();
        let p_values = filter
            .generic_tags
            .get(&SingleLetterTag::lowercase(Alphabet::P))
            .unwrap();
        assert_eq!(p_values.len(), 1);
        assert!(p_values.contains(&ai_keys().public_key().to_hex()));
    }

    #[test]
    fn filter_has_hanchan_channel_in_e_tag() {
        let since = Timestamp::from(1_760_000_000u64);
        let filter = build_chiihou_request_filter(&config(ChiihouChannel::Hanchan), since).unwrap();
        let e_values = filter
            .generic_tags
            .get(&SingleLetterTag::lowercase(Alphabet::E))
            .unwrap();
        assert_eq!(e_values.len(), 1);
        assert!(e_values.contains(HANCHAN_CHANNEL_ID));
    }

    #[test]
    fn filter_has_tonpuu_channel_in_e_tag() {
        let since = Timestamp::from(1_760_000_000u64);
        let filter = build_chiihou_request_filter(&config(ChiihouChannel::Tonpuu), since).unwrap();
        let e_values = filter
            .generic_tags
            .get(&SingleLetterTag::lowercase(Alphabet::E))
            .unwrap();
        assert_eq!(e_values.len(), 1);
        assert!(e_values.contains(TONPUU_CHANNEL_ID));
    }

    #[test]
    fn filter_keeps_given_since() {
        let since = Timestamp::from(1_760_000_000u64);
        let filter = build_chiihou_request_filter(&config(ChiihouChannel::Hanchan), since).unwrap();
        assert_eq!(filter.since, Some(since));
    }

    #[test]
    fn filter_has_no_authors() {
        let since = Timestamp::from(1_760_000_000u64);
        let filter = build_chiihou_request_filter(&config(ChiihouChannel::Hanchan), since).unwrap();
        assert!(filter.authors.is_none());
    }

    #[test]
    fn filter_rejects_invalid_channel_id() {
        let mut config = config(ChiihouChannel::Hanchan);
        config.replace_channel_ids_for_tests(vec!["not-a-channel-id".to_string()]);
        let since = Timestamp::from(1_760_000_000u64);
        let result = build_chiihou_request_filter(&config, since);
        assert!(matches!(
            result,
            Err(ChiihouRuntimeError::InvalidChannelId(channel_id))
                if channel_id == "not-a-channel-id"
        ));
    }

    #[test]
    fn processes_and_signs_kind_42_sutehai_request() {
        let config = config(ChiihouChannel::Hanchan);
        let mut seen = SeenEventIds::new();
        let event = build_request_event(42, &sutehai_content(), &request_tags(), &server_keys());
        let reply = process_and_sign_nostr_event(&event, &config, &mut seen, &mut PickSecondAgent)
            .unwrap()
            .unwrap();
        assert_eq!(reply.kind, Kind::from_u16(42));
        assert_eq!(reply.pubkey, ai_keys().public_key());
        assert!(reply.verify().is_ok());
        let server_npub = server_keys().public_key().to_bech32().unwrap();
        assert_eq!(
            reply.content,
            format!("nostr:{server_npub} sutehai? sutehai 2m")
        );
    }

    #[test]
    fn processes_and_signs_kind_20000_sutehai_request() {
        let config = config(ChiihouChannel::Hanchan);
        let mut seen = SeenEventIds::new();
        let mut tags = request_tags();
        tags.push(string_tag(&["g", "xn76"]));
        tags.push(string_tag(&["n", "Server Bot"]));
        let event = build_request_event(20000, &sutehai_content(), &tags, &server_keys());
        let reply = process_and_sign_nostr_event(&event, &config, &mut seen, &mut PickSecondAgent)
            .unwrap()
            .unwrap();
        assert_eq!(reply.kind, Kind::from_u16(20000));
        assert_eq!(reply.pubkey, ai_keys().public_key());
        assert!(reply.verify().is_ok());
    }

    #[test]
    fn signed_kind_20000_reply_keeps_bitchat_tags() {
        let config = config(ChiihouChannel::Hanchan);
        let mut seen = SeenEventIds::new();
        let mut tags = request_tags();
        tags.push(string_tag(&["g", "xn76"]));
        tags.push(string_tag(&["g", "xn77"]));
        tags.push(string_tag(&["n", "Server Bot"]));
        tags.push(string_tag(&["t", "mahjong"]));
        let event = build_request_event(20000, &sutehai_content(), &tags, &server_keys());
        let reply = process_and_sign_nostr_event(&event, &config, &mut seen, &mut PickSecondAgent)
            .unwrap()
            .unwrap();
        let reply_tags = incoming_event_from_nostr(&reply).tags;
        assert_eq!(
            tag_values(&reply_tags, "g"),
            vec![string_tag(&["g", "xn76"]), string_tag(&["g", "xn77"])]
        );
        assert!(tag_values(&reply_tags, "n").is_empty());
        assert_eq!(
            tag_values(&reply_tags, "t"),
            vec![string_tag(&["t", "teleport"])]
        );
    }

    #[test]
    fn signed_kind_20000_reply_has_no_nickname_tag() {
        let config = config(ChiihouChannel::Hanchan);
        let mut seen = SeenEventIds::new();
        let mut tags = request_tags();
        tags.push(string_tag(&["g", "xn76"]));
        tags.push(string_tag(&["n", "Server Bot"]));
        let event = build_request_event(20000, &sutehai_content(), &tags, &server_keys());
        let reply = process_and_sign_nostr_event(&event, &config, &mut seen, &mut PickSecondAgent)
            .unwrap()
            .unwrap();
        assert!(reply.verify().is_ok());
        let reply_tags = incoming_event_from_nostr(&reply).tags;
        assert!(tag_values(&reply_tags, "n").is_empty());
        assert_eq!(
            tag_values(&reply_tags, "g"),
            vec![string_tag(&["g", "xn76"])]
        );
        assert_eq!(
            tag_values(&reply_tags, "t"),
            vec![string_tag(&["t", "teleport"])]
        );
    }

    #[test]
    fn signed_kind_42_reply_has_no_bitchat_tags() {
        let config = config(ChiihouChannel::Hanchan);
        let mut seen = SeenEventIds::new();
        let event = build_request_event(42, &sutehai_content(), &request_tags(), &server_keys());
        let reply = process_and_sign_nostr_event(&event, &config, &mut seen, &mut PickSecondAgent)
            .unwrap()
            .unwrap();
        let reply_tags = incoming_event_from_nostr(&reply).tags;
        assert!(tag_values(&reply_tags, "g").is_empty());
        assert!(tag_values(&reply_tags, "n").is_empty());
        assert!(tag_values(&reply_tags, "t").is_empty());
    }

    #[test]
    fn naku_request_gets_naku_no_reply() {
        let config = config(ChiihouChannel::Hanchan);
        let mut seen = SeenEventIds::new();
        let event = build_request_event(42, &naku_content(), &request_tags(), &server_keys());
        let reply = process_and_sign_nostr_event(&event, &config, &mut seen, &mut PickSecondAgent)
            .unwrap()
            .unwrap();
        let server_npub = server_keys().public_key().to_bech32().unwrap();
        assert_eq!(reply.content, format!("nostr:{server_npub} naku? no"));
    }

    #[test]
    fn ignores_request_from_invalid_sender() {
        let config = config(ChiihouChannel::Hanchan);
        let mut seen = SeenEventIds::new();
        let other_keys = Keys::generate();
        let event = build_request_event(42, &sutehai_content(), &request_tags(), &other_keys);
        let result = process_and_sign_nostr_event(&event, &config, &mut seen, &mut PickSecondAgent);
        assert!(matches!(result, Ok(None)));
    }

    #[test]
    fn ignores_request_without_ai_p_tag() {
        let config = config(ChiihouChannel::Hanchan);
        let mut seen = SeenEventIds::new();
        let tags = vec![string_tag(&["e", HANCHAN_CHANNEL_ID, "", "root"])];
        let event = build_request_event(42, &sutehai_content(), &tags, &server_keys());
        let result = process_and_sign_nostr_event(&event, &config, &mut seen, &mut PickSecondAgent);
        assert!(matches!(result, Ok(None)));
    }

    #[test]
    fn ignores_request_for_unknown_channel() {
        let config = config(ChiihouChannel::Hanchan);
        let mut seen = SeenEventIds::new();
        let tags = vec![
            string_tag(&["e", TONPUU_CHANNEL_ID, "", "root"]),
            string_tag(&["p", &ai_keys().public_key().to_hex()]),
        ];
        let event = build_request_event(42, &sutehai_content(), &tags, &server_keys());
        let result = process_and_sign_nostr_event(&event, &config, &mut seen, &mut PickSecondAgent);
        assert!(matches!(result, Ok(None)));
    }

    #[test]
    fn processes_duplicate_event_only_once() {
        let config = config(ChiihouChannel::Hanchan);
        let mut seen = SeenEventIds::new();
        let event = build_request_event(42, &sutehai_content(), &request_tags(), &server_keys());
        let first = process_and_sign_nostr_event(&event, &config, &mut seen, &mut PickSecondAgent);
        assert!(matches!(first, Ok(Some(_))));
        let second = process_and_sign_nostr_event(&event, &config, &mut seen, &mut PickSecondAgent);
        assert!(matches!(second, Ok(None)));
    }

    #[test]
    fn ignores_unrelated_content() {
        let config = config(ChiihouChannel::Hanchan);
        let mut seen = SeenEventIds::new();
        let ai_npub = ai_keys().public_key().to_bech32().unwrap();
        let content = format!("nostr:{ai_npub} join");
        let event = build_request_event(42, &content, &request_tags(), &server_keys());
        let result = process_and_sign_nostr_event(&event, &config, &mut seen, &mut PickSecondAgent);
        assert!(matches!(result, Ok(None)));
    }

    fn relay_url(url: &str) -> RelayUrl {
        RelayUrl::parse(url).unwrap()
    }

    #[test]
    fn ensure_any_relay_succeeded_is_error_without_success() {
        let success = HashSet::new();
        let failed = HashMap::from([(
            relay_url("wss://relay1.example.com"),
            "connection refused".to_string(),
        )]);
        let result = ensure_any_relay_succeeded("subscription", &success, &failed);
        let message = result.unwrap_err();
        assert!(message.contains("subscription failed on all relays"));
        assert!(message.contains("relay1.example.com"));
    }

    #[test]
    fn ensure_any_relay_succeeded_is_ok_with_all_success() {
        let success = HashSet::from([relay_url("wss://relay1.example.com")]);
        let failed = HashMap::new();
        assert!(ensure_any_relay_succeeded("subscription", &success, &failed).is_ok());
    }

    #[test]
    fn ensure_any_relay_succeeded_is_ok_with_partial_failure() {
        let success = HashSet::from([relay_url("wss://relay1.example.com")]);
        let failed = HashMap::from([(
            relay_url("wss://relay2.example.com"),
            "connection refused".to_string(),
        )]);
        assert!(ensure_any_relay_succeeded("reply publish", &success, &failed).is_ok());
    }

    #[test]
    fn signed_kind_20000_reply_drops_g_tag_without_value() {
        let config = config(ChiihouChannel::Hanchan);
        let mut seen = SeenEventIds::new();
        let mut tags = request_tags();
        tags.push(string_tag(&["g"]));
        tags.push(string_tag(&["g", "xn76"]));
        let event = build_request_event(20000, &sutehai_content(), &tags, &server_keys());
        let reply = process_and_sign_nostr_event(&event, &config, &mut seen, &mut PickSecondAgent)
            .unwrap()
            .unwrap();
        assert!(reply.verify().is_ok());
        let reply_tags = incoming_event_from_nostr(&reply).tags;
        assert_eq!(
            tag_values(&reply_tags, "g"),
            vec![string_tag(&["g", "xn76"])]
        );
    }

    #[test]
    fn malformed_request_is_event_error() {
        let config = config(ChiihouChannel::Hanchan);
        let mut seen = SeenEventIds::new();
        let ai_npub = ai_keys().public_key().to_bech32().unwrap();
        let content = format!(
            "\
:mahjong_m1::mahjong_m2: :mahjong_m3::mahjong_m4:
nostr:{ai_npub} GET sutehai?"
        );
        let event = build_request_event(42, &content, &request_tags(), &server_keys());
        let result = process_and_sign_nostr_event(&event, &config, &mut seen, &mut PickSecondAgent);
        assert!(matches!(result, Err(ChiihouRuntimeError::Event(_))));
    }
}
