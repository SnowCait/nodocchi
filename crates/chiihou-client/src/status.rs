use std::time::Duration;

use nostr_sdk::prelude::{Client, Filter, Kind, PublicKey};

use crate::config::ChiihouNostrConfig;

pub const CHIIHOU_STATUS_KIND: u16 = 30315;

pub const CHIIHOU_STATUS_FETCH_TIMEOUT: Duration = Duration::from_secs(5);

const CHIIHOU_TABLE_CAPACITY: u8 = 4;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChiihouTableStatus {
    Empty,
    Recruiting { joined: u8, capacity: u8 },
    Playing,
    WaitingNext,
    Unknown(String),
}

impl std::fmt::Display for ChiihouTableStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Empty => write!(f, "empty"),
            Self::Recruiting { joined, capacity } => write!(f, "recruiting {joined}/{capacity}"),
            Self::Playing => write!(f, "playing"),
            Self::WaitingNext => write!(f, "waiting-next"),
            Self::Unknown(_) => write!(f, "unknown"),
        }
    }
}

pub fn parse_chiihou_table_status(content: &str) -> ChiihouTableStatus {
    let trimmed = content.trim();
    if trimmed.is_empty() {
        return ChiihouTableStatus::Empty;
    }
    if trimmed == "対局中" {
        return ChiihouTableStatus::Playing;
    }
    if trimmed == "next待ち" {
        return ChiihouTableStatus::WaitingNext;
    }
    if let Some(counts) = trimmed.strip_prefix("募集中 ")
        && let Some(status) = parse_recruiting_counts(counts)
    {
        return status;
    }
    ChiihouTableStatus::Unknown(trimmed.to_string())
}

fn parse_recruiting_counts(counts: &str) -> Option<ChiihouTableStatus> {
    let (joined, capacity) = counts.split_once('/')?;
    if !is_plain_digits(joined) || !is_plain_digits(capacity) {
        return None;
    }
    let joined: u8 = joined.parse().ok()?;
    let capacity: u8 = capacity.parse().ok()?;
    if capacity != CHIIHOU_TABLE_CAPACITY || joined == 0 || joined > capacity {
        return None;
    }
    Some(ChiihouTableStatus::Recruiting { joined, capacity })
}

fn is_plain_digits(value: &str) -> bool {
    !value.is_empty() && value.chars().all(|c| c.is_ascii_digit())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChiihouStartupCommand {
    Gamestart,
    Join,
}

impl std::fmt::Display for ChiihouStartupCommand {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Gamestart => write!(f, "gamestart"),
            Self::Join => write!(f, "join"),
        }
    }
}

pub fn startup_command_for_status(status: &ChiihouTableStatus) -> Option<ChiihouStartupCommand> {
    match status {
        ChiihouTableStatus::Empty => Some(ChiihouStartupCommand::Gamestart),
        ChiihouTableStatus::Recruiting { joined, capacity } if joined < capacity => {
            Some(ChiihouStartupCommand::Join)
        }
        ChiihouTableStatus::Recruiting { .. }
        | ChiihouTableStatus::Playing
        | ChiihouTableStatus::WaitingNext
        | ChiihouTableStatus::Unknown(_) => None,
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ChiihouStatusError {
    #[error("chiihou channel is not configured")]
    MissingChannel,

    #[error("invalid chiihou server public key")]
    InvalidServerPublicKey,

    #[error("failed to fetch chiihou table status: {0}")]
    Fetch(String),
}

pub fn build_chiihou_status_filter(
    config: &ChiihouNostrConfig,
) -> Result<Filter, ChiihouStatusError> {
    let channel_id = config
        .event_config()
        .channel_ids
        .first()
        .ok_or(ChiihouStatusError::MissingChannel)?;
    let server_pubkey = PublicKey::from_hex(&config.event_config().server_pubkey_hex)
        .map_err(|_| ChiihouStatusError::InvalidServerPublicKey)?;
    Ok(Filter::new()
        .kind(Kind::from_u16(CHIIHOU_STATUS_KIND))
        .identifier(channel_id.clone())
        .author(server_pubkey)
        .limit(1))
}

pub async fn fetch_chiihou_table_status(
    client: &Client,
    config: &ChiihouNostrConfig,
    timeout: Duration,
) -> Result<ChiihouTableStatus, ChiihouStatusError> {
    let filter = build_chiihou_status_filter(config)?;
    let events = client
        .fetch_events(filter)
        .timeout(timeout)
        .await
        .map_err(|error| ChiihouStatusError::Fetch(error.to_string()))?;
    match events.first() {
        Some(event) => Ok(parse_chiihou_table_status(&event.content)),
        None => Ok(ChiihouTableStatus::Empty),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{ChiihouChannel, HANCHAN_CHANNEL_ID, TONPUU_CHANNEL_ID};
    use nostr_sdk::prelude::{Keys, SingleLetterTag};

    // テスト専用の秘密鍵。実際の運用で使用してはならない。
    const TEST_AI_SECRET_KEY_HEX: &str =
        "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

    // テスト専用の秘密鍵。server 公開鍵の導出のみに使用する。
    const TEST_SERVER_SECRET_KEY_HEX: &str =
        "fedcba9876543210fedcba9876543210fedcba9876543210fedcba9876543210";

    // テスト専用の秘密鍵。override server の公開鍵の導出のみに使用する。
    const TEST_OVERRIDE_SERVER_SECRET_KEY_HEX: &str =
        "1111111111111111111111111111111111111111111111111111111111111111";

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

    #[test]
    fn empty_content_is_empty() {
        assert_eq!(parse_chiihou_table_status(""), ChiihouTableStatus::Empty);
    }

    #[test]
    fn whitespace_content_is_empty() {
        assert_eq!(
            parse_chiihou_table_status("  \n\t "),
            ChiihouTableStatus::Empty
        );
    }

    #[test]
    fn parses_recruiting_one_of_four() {
        assert_eq!(
            parse_chiihou_table_status("募集中 1/4"),
            ChiihouTableStatus::Recruiting {
                joined: 1,
                capacity: 4
            }
        );
    }

    #[test]
    fn parses_recruiting_two_of_four() {
        assert_eq!(
            parse_chiihou_table_status("募集中 2/4"),
            ChiihouTableStatus::Recruiting {
                joined: 2,
                capacity: 4
            }
        );
    }

    #[test]
    fn parses_recruiting_three_of_four() {
        assert_eq!(
            parse_chiihou_table_status("募集中 3/4"),
            ChiihouTableStatus::Recruiting {
                joined: 3,
                capacity: 4
            }
        );
    }

    #[test]
    fn parses_recruiting_four_of_four() {
        assert_eq!(
            parse_chiihou_table_status("募集中 4/4"),
            ChiihouTableStatus::Recruiting {
                joined: 4,
                capacity: 4
            }
        );
    }

    #[test]
    fn parses_playing() {
        assert_eq!(
            parse_chiihou_table_status("対局中"),
            ChiihouTableStatus::Playing
        );
    }

    #[test]
    fn parses_waiting_next() {
        assert_eq!(
            parse_chiihou_table_status("next待ち"),
            ChiihouTableStatus::WaitingNext
        );
    }

    #[test]
    fn trims_status_content() {
        assert_eq!(
            parse_chiihou_table_status(" 対局中 \n"),
            ChiihouTableStatus::Playing
        );
        assert_eq!(
            parse_chiihou_table_status(" 募集中 2/4 "),
            ChiihouTableStatus::Recruiting {
                joined: 2,
                capacity: 4
            }
        );
    }

    #[test]
    fn malformed_recruiting_is_unknown() {
        for content in [
            "募集中",
            "募集中 x/4",
            "募集中 1/x",
            "募集中 0/4",
            "募集中 5/4",
            "募集中 1/3",
        ] {
            assert_eq!(
                parse_chiihou_table_status(content),
                ChiihouTableStatus::Unknown(content.to_string()),
                "content: {content}"
            );
        }
    }

    #[test]
    fn unknown_content_is_unknown() {
        assert_eq!(
            parse_chiihou_table_status("メンテナンス中"),
            ChiihouTableStatus::Unknown("メンテナンス中".to_string())
        );
    }

    #[test]
    fn status_display_is_stable() {
        assert_eq!(ChiihouTableStatus::Empty.to_string(), "empty");
        assert_eq!(
            ChiihouTableStatus::Recruiting {
                joined: 1,
                capacity: 4
            }
            .to_string(),
            "recruiting 1/4"
        );
        assert_eq!(ChiihouTableStatus::Playing.to_string(), "playing");
        assert_eq!(ChiihouTableStatus::WaitingNext.to_string(), "waiting-next");
        assert_eq!(
            ChiihouTableStatus::Unknown("秘密のstatus".to_string()).to_string(),
            "unknown"
        );
    }

    #[test]
    fn startup_command_display_is_stable() {
        assert_eq!(ChiihouStartupCommand::Gamestart.to_string(), "gamestart");
        assert_eq!(ChiihouStartupCommand::Join.to_string(), "join");
    }

    #[test]
    fn empty_status_starts_game() {
        assert_eq!(
            startup_command_for_status(&ChiihouTableStatus::Empty),
            Some(ChiihouStartupCommand::Gamestart)
        );
    }

    #[test]
    fn recruiting_with_open_seat_joins() {
        for joined in 1..=3 {
            assert_eq!(
                startup_command_for_status(&ChiihouTableStatus::Recruiting {
                    joined,
                    capacity: 4
                }),
                Some(ChiihouStartupCommand::Join),
                "joined: {joined}"
            );
        }
    }

    #[test]
    fn full_recruiting_sends_nothing() {
        assert_eq!(
            startup_command_for_status(&ChiihouTableStatus::Recruiting {
                joined: 4,
                capacity: 4
            }),
            None
        );
    }

    #[test]
    fn playing_sends_nothing() {
        assert_eq!(
            startup_command_for_status(&ChiihouTableStatus::Playing),
            None
        );
    }

    #[test]
    fn waiting_next_sends_nothing() {
        assert_eq!(
            startup_command_for_status(&ChiihouTableStatus::WaitingNext),
            None
        );
    }

    #[test]
    fn unknown_sends_nothing() {
        assert_eq!(
            startup_command_for_status(&ChiihouTableStatus::Unknown("???".to_string())),
            None
        );
    }

    #[test]
    fn status_filter_has_only_status_kind() {
        let filter = build_chiihou_status_filter(&config(ChiihouChannel::Hanchan)).unwrap();
        let kinds = filter.kinds.unwrap();
        assert_eq!(kinds.len(), 1);
        assert!(kinds.contains(&Kind::from_u16(30315)));
    }

    #[test]
    fn status_filter_has_only_server_author() {
        let filter = build_chiihou_status_filter(&config(ChiihouChannel::Hanchan)).unwrap();
        let authors = filter.authors.unwrap();
        assert_eq!(authors.len(), 1);
        assert!(authors.contains(&server_keys().public_key()));
    }

    #[test]
    fn status_filter_has_hanchan_channel_in_d_tag() {
        let filter = build_chiihou_status_filter(&config(ChiihouChannel::Hanchan)).unwrap();
        let d_values = filter
            .generic_tags
            .get(&SingleLetterTag::LOWERCASE_D)
            .unwrap();
        assert_eq!(d_values.len(), 1);
        assert!(d_values.contains(HANCHAN_CHANNEL_ID));
    }

    #[test]
    fn status_filter_has_tonpuu_channel_in_d_tag() {
        let filter = build_chiihou_status_filter(&config(ChiihouChannel::Tonpuu)).unwrap();
        let d_values = filter
            .generic_tags
            .get(&SingleLetterTag::LOWERCASE_D)
            .unwrap();
        assert_eq!(d_values.len(), 1);
        assert!(d_values.contains(TONPUU_CHANNEL_ID));
    }

    #[test]
    fn status_filter_limit_is_one() {
        let filter = build_chiihou_status_filter(&config(ChiihouChannel::Hanchan)).unwrap();
        assert_eq!(filter.limit, Some(1));
    }

    #[test]
    fn status_filter_has_no_p_tag() {
        let filter = build_chiihou_status_filter(&config(ChiihouChannel::Hanchan)).unwrap();
        assert!(
            !filter
                .generic_tags
                .contains_key(&SingleLetterTag::LOWERCASE_P)
        );
    }

    #[test]
    fn status_filter_has_no_e_tag() {
        let filter = build_chiihou_status_filter(&config(ChiihouChannel::Hanchan)).unwrap();
        assert!(
            !filter
                .generic_tags
                .contains_key(&SingleLetterTag::LOWERCASE_E)
        );
    }

    #[test]
    fn status_filter_has_no_since_or_until() {
        let filter = build_chiihou_status_filter(&config(ChiihouChannel::Hanchan)).unwrap();
        assert!(filter.since.is_none());
        assert!(filter.until.is_none());
    }

    #[test]
    fn status_filter_uses_override_server_author() {
        let override_keys = Keys::parse(TEST_OVERRIDE_SERVER_SECRET_KEY_HEX).unwrap();
        let config = ChiihouNostrConfig::new(
            TEST_AI_SECRET_KEY_HEX,
            &override_keys.public_key().to_hex(),
            ChiihouChannel::Hanchan,
        )
        .unwrap();
        let filter = build_chiihou_status_filter(&config).unwrap();
        let authors = filter.authors.unwrap();
        assert_eq!(authors.len(), 1);
        assert!(authors.contains(&override_keys.public_key()));
    }

    #[test]
    fn status_filter_requires_channel() {
        let mut config = config(ChiihouChannel::Hanchan);
        config.replace_channel_ids_for_tests(Vec::new());
        assert!(matches!(
            build_chiihou_status_filter(&config),
            Err(ChiihouStatusError::MissingChannel)
        ));
    }
}
