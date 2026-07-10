use nostr_sdk::Event;

use crate::config::ChiihouNostrConfig;
use crate::event::CHIIHOU_CHANNEL_MESSAGE_KIND;
use crate::nostr_adapter::{ChiihouNostrAdapterError, sign_outgoing_event};
use crate::status::ChiihouStartupCommand;

const CHIIHOU_NEXT_COMMAND: &str = "next";

pub fn build_chiihou_startup_command_content(
    server_npub: &str,
    command: ChiihouStartupCommand,
) -> String {
    build_chiihou_command_content(server_npub, &command.to_string())
}

fn build_chiihou_command_content(server_npub: &str, command: &str) -> String {
    format!("nostr:{server_npub} {command}")
}

pub fn build_chiihou_startup_command_tags(
    channel_id: &str,
    server_pubkey_hex: &str,
) -> Vec<Vec<String>> {
    vec![
        vec![
            "e".to_string(),
            channel_id.to_string(),
            String::new(),
            "root".to_string(),
        ],
        vec!["p".to_string(), server_pubkey_hex.to_string()],
    ]
}

#[derive(Debug, thiserror::Error)]
pub enum ChiihouCommandError {
    #[error("chiihou channel is not configured")]
    MissingChannel,

    #[error(transparent)]
    Adapter(#[from] ChiihouNostrAdapterError),
}

pub fn sign_chiihou_startup_command(
    command: ChiihouStartupCommand,
    config: &ChiihouNostrConfig,
) -> Result<Event, ChiihouCommandError> {
    sign_chiihou_command(&command.to_string(), config)
}

pub fn sign_chiihou_next_command(
    config: &ChiihouNostrConfig,
) -> Result<Event, ChiihouCommandError> {
    sign_chiihou_command(CHIIHOU_NEXT_COMMAND, config)
}

fn sign_chiihou_command(
    command: &str,
    config: &ChiihouNostrConfig,
) -> Result<Event, ChiihouCommandError> {
    let event_config = config.event_config();
    let channel_id = event_config
        .channel_ids
        .first()
        .ok_or(ChiihouCommandError::MissingChannel)?;
    let content = build_chiihou_command_content(&event_config.server_npub, command);
    let tags = build_chiihou_startup_command_tags(channel_id, &event_config.server_pubkey_hex);
    let event = sign_outgoing_event(
        u64::from(CHIIHOU_CHANNEL_MESSAGE_KIND),
        &tags,
        &content,
        config.keys(),
    )?;
    Ok(event)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{CHIIHOU_SERVER_NPUB, ChiihouChannel, HANCHAN_CHANNEL_ID};
    use crate::nostr_adapter::incoming_event_from_nostr;
    use nostr_sdk::{Keys, Kind, ToBech32};

    // テスト専用の秘密鍵。実際の運用で使用してはならない。
    const TEST_AI_SECRET_KEY_HEX: &str =
        "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

    // テスト専用の秘密鍵。server 公開鍵の導出のみに使用する。
    const TEST_SERVER_SECRET_KEY_HEX: &str =
        "fedcba9876543210fedcba9876543210fedcba9876543210fedcba9876543210";

    fn ai_keys() -> Keys {
        Keys::parse(TEST_AI_SECRET_KEY_HEX).unwrap()
    }

    fn server_keys() -> Keys {
        Keys::parse(TEST_SERVER_SECRET_KEY_HEX).unwrap()
    }

    fn server_npub() -> String {
        server_keys().public_key().to_bech32().unwrap()
    }

    fn config() -> ChiihouNostrConfig {
        ChiihouNostrConfig::new(
            TEST_AI_SECRET_KEY_HEX,
            &server_keys().public_key().to_hex(),
            ChiihouChannel::Hanchan,
        )
        .unwrap()
    }

    fn tag_values(tags: &[Vec<String>], tag_name: &str) -> Vec<Vec<String>> {
        tags.iter()
            .filter(|tag| tag.first().is_some_and(|name| name == tag_name))
            .cloned()
            .collect()
    }

    #[test]
    fn gamestart_content_targets_override_server() {
        let npub = server_npub();
        assert_eq!(
            build_chiihou_startup_command_content(&npub, ChiihouStartupCommand::Gamestart),
            format!("nostr:{npub} gamestart")
        );
    }

    #[test]
    fn join_content_targets_override_server() {
        let npub = server_npub();
        assert_eq!(
            build_chiihou_startup_command_content(&npub, ChiihouStartupCommand::Join),
            format!("nostr:{npub} join")
        );
    }

    #[test]
    fn gamestart_content_targets_default_server() {
        assert_eq!(
            build_chiihou_startup_command_content(
                CHIIHOU_SERVER_NPUB,
                ChiihouStartupCommand::Gamestart
            ),
            format!("nostr:{CHIIHOU_SERVER_NPUB} gamestart")
        );
    }

    #[test]
    fn join_content_targets_default_server() {
        assert_eq!(
            build_chiihou_startup_command_content(CHIIHOU_SERVER_NPUB, ChiihouStartupCommand::Join),
            format!("nostr:{CHIIHOU_SERVER_NPUB} join")
        );
    }

    #[test]
    fn command_tags_match_expected_shape() {
        assert_eq!(
            build_chiihou_startup_command_tags("channel_id", "server_pubkey_hex"),
            vec![
                vec![
                    "e".to_string(),
                    "channel_id".to_string(),
                    String::new(),
                    "root".to_string()
                ],
                vec!["p".to_string(), "server_pubkey_hex".to_string()],
            ]
        );
    }

    #[test]
    fn signed_command_has_kind_42() {
        let event =
            sign_chiihou_startup_command(ChiihouStartupCommand::Gamestart, &config()).unwrap();
        assert_eq!(event.kind, Kind::from_u16(42));
    }

    #[test]
    fn signed_command_has_expected_content() {
        let event = sign_chiihou_startup_command(ChiihouStartupCommand::Join, &config()).unwrap();
        assert_eq!(event.content, format!("nostr:{} join", server_npub()));
    }

    #[test]
    fn signed_command_uses_ai_pubkey_and_verifies() {
        let event =
            sign_chiihou_startup_command(ChiihouStartupCommand::Gamestart, &config()).unwrap();
        assert_eq!(event.pubkey, ai_keys().public_key());
        assert!(event.verify().is_ok());
    }

    #[test]
    fn signed_command_has_root_e_tag_and_server_p_tag_only() {
        let event =
            sign_chiihou_startup_command(ChiihouStartupCommand::Gamestart, &config()).unwrap();
        let tags = incoming_event_from_nostr(&event).tags;
        assert_eq!(
            tags,
            vec![
                vec![
                    "e".to_string(),
                    HANCHAN_CHANNEL_ID.to_string(),
                    String::new(),
                    "root".to_string()
                ],
                vec!["p".to_string(), server_keys().public_key().to_hex()],
            ]
        );
    }

    #[test]
    fn signed_command_has_no_reply_e_tag() {
        let event = sign_chiihou_startup_command(ChiihouStartupCommand::Join, &config()).unwrap();
        let tags = incoming_event_from_nostr(&event).tags;
        let e_tags = tag_values(&tags, "e");
        assert_eq!(e_tags.len(), 1);
        assert!(
            e_tags
                .iter()
                .all(|tag| tag.get(3).is_some_and(|marker| marker == "root"))
        );
    }

    #[test]
    fn signed_command_has_no_ai_p_tag() {
        let event = sign_chiihou_startup_command(ChiihouStartupCommand::Join, &config()).unwrap();
        let tags = incoming_event_from_nostr(&event).tags;
        let p_tags = tag_values(&tags, "p");
        assert_eq!(
            p_tags,
            vec![vec!["p".to_string(), server_keys().public_key().to_hex()]]
        );
    }

    #[test]
    fn signed_command_has_no_bitchat_tags() {
        let event =
            sign_chiihou_startup_command(ChiihouStartupCommand::Gamestart, &config()).unwrap();
        let tags = incoming_event_from_nostr(&event).tags;
        assert!(tag_values(&tags, "g").is_empty());
        assert!(tag_values(&tags, "n").is_empty());
        assert!(tag_values(&tags, "t").is_empty());
    }

    #[test]
    fn signed_next_command_has_expected_content() {
        let event = sign_chiihou_next_command(&config()).unwrap();
        assert_eq!(event.content, format!("nostr:{} next", server_npub()));
    }

    #[test]
    fn signed_next_command_has_kind_42() {
        let event = sign_chiihou_next_command(&config()).unwrap();
        assert_eq!(event.kind, Kind::from_u16(42));
    }

    #[test]
    fn signed_next_command_uses_ai_pubkey_and_verifies() {
        let event = sign_chiihou_next_command(&config()).unwrap();
        assert_eq!(event.pubkey, ai_keys().public_key());
        assert!(event.verify().is_ok());
    }

    #[test]
    fn signed_next_command_has_single_root_e_tag_and_no_reply_e_tag() {
        let event = sign_chiihou_next_command(&config()).unwrap();
        let tags = incoming_event_from_nostr(&event).tags;
        let e_tags = tag_values(&tags, "e");
        assert_eq!(
            e_tags,
            vec![vec![
                "e".to_string(),
                HANCHAN_CHANNEL_ID.to_string(),
                String::new(),
                "root".to_string()
            ]]
        );
    }

    #[test]
    fn signed_next_command_has_single_server_p_tag_and_no_ai_p_tag() {
        let event = sign_chiihou_next_command(&config()).unwrap();
        let tags = incoming_event_from_nostr(&event).tags;
        assert_eq!(
            tag_values(&tags, "p"),
            vec![vec!["p".to_string(), server_keys().public_key().to_hex()]]
        );
    }

    #[test]
    fn signed_next_command_has_no_bitchat_tags() {
        let event = sign_chiihou_next_command(&config()).unwrap();
        let tags = incoming_event_from_nostr(&event).tags;
        assert!(tag_values(&tags, "g").is_empty());
        assert!(tag_values(&tags, "n").is_empty());
        assert!(tag_values(&tags, "t").is_empty());
    }

    #[test]
    fn sign_next_command_requires_channel() {
        let mut config = config();
        config.replace_channel_ids_for_tests(Vec::new());
        assert!(matches!(
            sign_chiihou_next_command(&config),
            Err(ChiihouCommandError::MissingChannel)
        ));
    }

    #[test]
    fn sign_command_requires_channel() {
        let mut config = config();
        config.replace_channel_ids_for_tests(Vec::new());
        assert!(matches!(
            sign_chiihou_startup_command(ChiihouStartupCommand::Gamestart, &config),
            Err(ChiihouCommandError::MissingChannel)
        ));
    }
}
