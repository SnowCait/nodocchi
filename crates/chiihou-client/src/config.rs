use std::str::FromStr;

use nostr_sdk::{Keys, PublicKey, ToBech32};

use crate::event::ChiihouEventConfig;

pub const DEFAULT_RELAY_URLS: [&str; 2] = ["wss://relay.nostr.wirednet.jp/", "wss://yabu.me/"];

pub const HANCHAN_CHANNEL_ID: &str =
    "c8d5c2709a5670d6f621ac8020ac3e4fc3057a4961a15319f7c0818309407723";

pub const TONPUU_CHANNEL_ID: &str =
    "06ddcb27b27f667d6487b5128625f25cb2148cf87bff0502aaffe5ca705dc626";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChiihouChannel {
    Hanchan,
    Tonpuu,
}

impl ChiihouChannel {
    pub fn channel_id(self) -> &'static str {
        match self {
            Self::Hanchan => HANCHAN_CHANNEL_ID,
            Self::Tonpuu => TONPUU_CHANNEL_ID,
        }
    }
}

#[derive(Debug, thiserror::Error, Clone, PartialEq, Eq)]
#[error("invalid chiihou channel: {0}")]
pub struct ChiihouChannelParseError(String);

impl FromStr for ChiihouChannel {
    type Err = ChiihouChannelParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if s.eq_ignore_ascii_case("hanchan") {
            Ok(Self::Hanchan)
        } else if s.eq_ignore_ascii_case("tonpuu") {
            Ok(Self::Tonpuu)
        } else {
            Err(ChiihouChannelParseError(s.to_string()))
        }
    }
}

#[derive(Debug, thiserror::Error, Clone, Copy, PartialEq, Eq)]
pub enum ChiihouConfigError {
    #[error("invalid AI secret key")]
    InvalidAiSecretKey,

    #[error("invalid server public key")]
    InvalidServerPublicKey,
}

pub struct ChiihouNostrConfig {
    keys: Keys,
    event_config: ChiihouEventConfig,
    relay_urls: Vec<String>,
}

impl ChiihouNostrConfig {
    pub fn new(
        ai_secret_key: &str,
        server_public_key: &str,
        channel: ChiihouChannel,
    ) -> Result<Self, ChiihouConfigError> {
        Self::with_relays(
            ai_secret_key,
            server_public_key,
            channel,
            DEFAULT_RELAY_URLS
                .iter()
                .map(|url| url.to_string())
                .collect(),
        )
    }

    pub fn with_relays(
        ai_secret_key: &str,
        server_public_key: &str,
        channel: ChiihouChannel,
        relay_urls: Vec<String>,
    ) -> Result<Self, ChiihouConfigError> {
        let keys =
            Keys::parse(ai_secret_key).map_err(|_| ChiihouConfigError::InvalidAiSecretKey)?;
        let server_public_key = PublicKey::parse(server_public_key)
            .map_err(|_| ChiihouConfigError::InvalidServerPublicKey)?;
        let server_npub = server_public_key
            .to_bech32()
            .map_err(|_| ChiihouConfigError::InvalidServerPublicKey)?;
        let event_config = ChiihouEventConfig {
            ai_pubkey_hex: keys.public_key().to_hex(),
            server_pubkey_hex: server_public_key.to_hex(),
            server_npub,
            channel_ids: vec![channel.channel_id().to_string()],
        };
        Ok(Self {
            keys,
            event_config,
            relay_urls,
        })
    }

    pub fn keys(&self) -> &Keys {
        &self.keys
    }

    pub fn event_config(&self) -> &ChiihouEventConfig {
        &self.event_config
    }

    pub fn relay_urls(&self) -> &[String] {
        &self.relay_urls
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // テスト専用の秘密鍵。実際の運用で使用してはならない。
    const TEST_AI_SECRET_KEY_HEX: &str =
        "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

    // テスト専用の秘密鍵。server 公開鍵の導出のみに使用する。
    const TEST_SERVER_SECRET_KEY_HEX: &str =
        "fedcba9876543210fedcba9876543210fedcba9876543210fedcba9876543210";

    fn server_keys() -> Keys {
        Keys::parse(TEST_SERVER_SECRET_KEY_HEX).unwrap()
    }

    fn server_pubkey_hex() -> String {
        server_keys().public_key().to_hex()
    }

    fn server_npub() -> String {
        server_keys().public_key().to_bech32().unwrap()
    }

    #[test]
    fn parses_hex_secret_key() {
        let config = ChiihouNostrConfig::new(
            TEST_AI_SECRET_KEY_HEX,
            &server_pubkey_hex(),
            ChiihouChannel::Hanchan,
        )
        .unwrap();
        let expected = Keys::parse(TEST_AI_SECRET_KEY_HEX).unwrap();
        assert_eq!(
            config.keys().public_key().to_hex(),
            expected.public_key().to_hex()
        );
    }

    #[test]
    fn parses_nsec_secret_key() {
        let keys = Keys::parse(TEST_AI_SECRET_KEY_HEX).unwrap();
        let nsec = keys.secret_key().to_bech32().unwrap();
        let config =
            ChiihouNostrConfig::new(&nsec, &server_pubkey_hex(), ChiihouChannel::Hanchan).unwrap();
        assert_eq!(
            config.keys().public_key().to_hex(),
            keys.public_key().to_hex()
        );
    }

    #[test]
    fn derives_ai_pubkey_from_secret_key() {
        let config = ChiihouNostrConfig::new(
            TEST_AI_SECRET_KEY_HEX,
            &server_pubkey_hex(),
            ChiihouChannel::Hanchan,
        )
        .unwrap();
        let expected = Keys::parse(TEST_AI_SECRET_KEY_HEX).unwrap();
        assert_eq!(
            config.event_config().ai_pubkey_hex,
            expected.public_key().to_hex()
        );
    }

    #[test]
    fn accepts_server_hex_pubkey() {
        let config = ChiihouNostrConfig::new(
            TEST_AI_SECRET_KEY_HEX,
            &server_pubkey_hex(),
            ChiihouChannel::Hanchan,
        )
        .unwrap();
        assert_eq!(config.event_config().server_pubkey_hex, server_pubkey_hex());
        assert_eq!(config.event_config().server_npub, server_npub());
    }

    #[test]
    fn accepts_server_npub() {
        let config = ChiihouNostrConfig::new(
            TEST_AI_SECRET_KEY_HEX,
            &server_npub(),
            ChiihouChannel::Hanchan,
        )
        .unwrap();
        assert_eq!(config.event_config().server_pubkey_hex, server_pubkey_hex());
        assert_eq!(config.event_config().server_npub, server_npub());
    }

    #[test]
    fn server_hex_and_npub_produce_same_config() {
        let from_hex = ChiihouNostrConfig::new(
            TEST_AI_SECRET_KEY_HEX,
            &server_pubkey_hex(),
            ChiihouChannel::Hanchan,
        )
        .unwrap();
        let from_npub = ChiihouNostrConfig::new(
            TEST_AI_SECRET_KEY_HEX,
            &server_npub(),
            ChiihouChannel::Hanchan,
        )
        .unwrap();
        assert_eq!(from_hex.event_config(), from_npub.event_config());
    }

    #[test]
    fn hanchan_channel_id_is_fixed() {
        assert_eq!(ChiihouChannel::Hanchan.channel_id(), HANCHAN_CHANNEL_ID);
        let config = ChiihouNostrConfig::new(
            TEST_AI_SECRET_KEY_HEX,
            &server_pubkey_hex(),
            ChiihouChannel::Hanchan,
        )
        .unwrap();
        assert_eq!(
            config.event_config().channel_ids,
            vec![HANCHAN_CHANNEL_ID.to_string()]
        );
    }

    #[test]
    fn tonpuu_channel_id_is_fixed() {
        assert_eq!(ChiihouChannel::Tonpuu.channel_id(), TONPUU_CHANNEL_ID);
        let config = ChiihouNostrConfig::new(
            TEST_AI_SECRET_KEY_HEX,
            &server_pubkey_hex(),
            ChiihouChannel::Tonpuu,
        )
        .unwrap();
        assert_eq!(
            config.event_config().channel_ids,
            vec![TONPUU_CHANNEL_ID.to_string()]
        );
    }

    #[test]
    fn channel_from_str_accepts_known_names() {
        assert_eq!(
            "hanchan".parse::<ChiihouChannel>(),
            Ok(ChiihouChannel::Hanchan)
        );
        assert_eq!(
            "tonpuu".parse::<ChiihouChannel>(),
            Ok(ChiihouChannel::Tonpuu)
        );
    }

    #[test]
    fn channel_from_str_ignores_ascii_case() {
        assert_eq!(
            "Hanchan".parse::<ChiihouChannel>(),
            Ok(ChiihouChannel::Hanchan)
        );
        assert_eq!(
            "TONPUU".parse::<ChiihouChannel>(),
            Ok(ChiihouChannel::Tonpuu)
        );
    }

    #[test]
    fn channel_from_str_rejects_unknown_name() {
        assert_eq!(
            "sanma".parse::<ChiihouChannel>(),
            Err(ChiihouChannelParseError("sanma".to_string()))
        );
    }

    #[test]
    fn new_uses_default_relays() {
        let config = ChiihouNostrConfig::new(
            TEST_AI_SECRET_KEY_HEX,
            &server_pubkey_hex(),
            ChiihouChannel::Hanchan,
        )
        .unwrap();
        assert_eq!(
            config.relay_urls(),
            &[
                "wss://relay.nostr.wirednet.jp/".to_string(),
                "wss://yabu.me/".to_string(),
            ]
        );
    }

    #[test]
    fn with_relays_keeps_given_relays() {
        let config = ChiihouNostrConfig::with_relays(
            TEST_AI_SECRET_KEY_HEX,
            &server_pubkey_hex(),
            ChiihouChannel::Hanchan,
            vec!["wss://example.com/".to_string()],
        )
        .unwrap();
        assert_eq!(config.relay_urls(), &["wss://example.com/".to_string()]);
    }

    #[test]
    fn rejects_invalid_ai_secret_key() {
        let result = ChiihouNostrConfig::new(
            "not-a-secret-key",
            &server_pubkey_hex(),
            ChiihouChannel::Hanchan,
        );
        assert!(matches!(
            result,
            Err(ChiihouConfigError::InvalidAiSecretKey)
        ));
    }

    #[test]
    fn rejects_invalid_server_public_key() {
        let result = ChiihouNostrConfig::new(
            TEST_AI_SECRET_KEY_HEX,
            "not-a-public-key",
            ChiihouChannel::Hanchan,
        );
        assert!(matches!(
            result,
            Err(ChiihouConfigError::InvalidServerPublicKey)
        ));
    }
}
