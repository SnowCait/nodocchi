use std::time::Duration;

use nostr_sdk::nips::nip19::Nip19Profile;
use nostr_sdk::{FromBech32, PublicKey};

use crate::config::{CHIIHOU_SERVER_NPUB, ChiihouChannel, ChiihouConfigError, ChiihouNostrConfig};
use crate::secret::{ChiihouSecretError, validate_chiihou_nsec};

pub const USAGE: &str = "usage: chiihou-client --channel <hanchan|tonpuu> [--agent normal|tsumogiri|shanten|menzen] [--server-npub <NPUB_OR_NPROFILE>] [--auto-next] [--response-delay-ms <MILLISECONDS>]";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ChiihouAgentKind {
    #[default]
    Normal,
    Tsumogiri,
    Shanten,
    Menzen,
}

#[derive(Debug, thiserror::Error, Clone, PartialEq, Eq)]
#[error("unknown chiihou agent: {0}")]
pub struct ChiihouAgentKindParseError(String);

impl std::str::FromStr for ChiihouAgentKind {
    type Err = ChiihouAgentKindParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim().to_ascii_lowercase().as_str() {
            "normal" => Ok(Self::Normal),
            "tsumogiri" | "tsumo-giri" => Ok(Self::Tsumogiri),
            "shanten" => Ok(Self::Shanten),
            "menzen" => Ok(Self::Menzen),
            other => Err(ChiihouAgentKindParseError(other.to_string())),
        }
    }
}

impl std::fmt::Display for ChiihouAgentKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Normal => write!(f, "normal"),
            Self::Tsumogiri => write!(f, "tsumogiri"),
            Self::Shanten => write!(f, "shanten"),
            Self::Menzen => write!(f, "menzen"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChiihouCliArgs {
    pub server_npub: Option<String>,
    pub channel: ChiihouChannel,
    pub agent: ChiihouAgentKind,
    pub auto_next: bool,
    pub response_delay_ms: u64,
}

impl ChiihouCliArgs {
    pub fn parse<I>(args: I) -> Result<Self, ChiihouCliError>
    where
        I: IntoIterator<Item = String>,
    {
        let mut args = args.into_iter();
        let mut server_npub = None;
        let mut channel = None;
        let mut agent = None;
        let mut auto_next = false;
        let mut response_delay_ms = None;

        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--server-npub" => {
                    if server_npub.is_some() {
                        return Err(ChiihouCliError::DuplicateOption("--server-npub"));
                    }
                    server_npub = Some(args.next().ok_or(ChiihouCliError::MissingServerNpubValue)?);
                }
                "--channel" => {
                    if channel.is_some() {
                        return Err(ChiihouCliError::DuplicateOption("--channel"));
                    }
                    let value = args.next().ok_or(ChiihouCliError::MissingChannelValue)?;
                    channel = Some(
                        value
                            .parse::<ChiihouChannel>()
                            .map_err(|_| ChiihouCliError::UnknownChannel(value))?,
                    );
                }
                "--agent" => {
                    if agent.is_some() {
                        return Err(ChiihouCliError::DuplicateOption("--agent"));
                    }
                    let value = args.next().ok_or(ChiihouCliError::MissingAgentValue)?;
                    agent = Some(
                        value
                            .parse::<ChiihouAgentKind>()
                            .map_err(|_| ChiihouCliError::UnknownAgent(value))?,
                    );
                }
                "--auto-next" => {
                    if auto_next {
                        return Err(ChiihouCliError::DuplicateOption("--auto-next"));
                    }
                    auto_next = true;
                }
                "--response-delay-ms" => {
                    if response_delay_ms.is_some() {
                        return Err(ChiihouCliError::DuplicateOption("--response-delay-ms"));
                    }
                    let value = args
                        .next()
                        .ok_or(ChiihouCliError::MissingResponseDelayValue)?;
                    response_delay_ms = Some(
                        value
                            .parse::<u64>()
                            .map_err(|_| ChiihouCliError::InvalidResponseDelay(value))?,
                    );
                }
                other => return Err(ChiihouCliError::UnknownOption(other.to_string())),
            }
        }

        Ok(Self {
            server_npub,
            channel: channel.ok_or(ChiihouCliError::ChannelRequired)?,
            agent: agent.unwrap_or_default(),
            auto_next,
            response_delay_ms: response_delay_ms.unwrap_or(0),
        })
    }

    pub fn response_delay(&self) -> Duration {
        Duration::from_millis(self.response_delay_ms)
    }
}

#[derive(Debug, thiserror::Error, Clone, PartialEq, Eq)]
pub enum ChiihouCliError {
    #[error("unknown option: {0}")]
    UnknownOption(String),

    #[error("--server-npub requires a value")]
    MissingServerNpubValue,

    #[error("--channel requires a value")]
    MissingChannelValue,

    #[error("--agent requires a value")]
    MissingAgentValue,

    #[error("--channel is required")]
    ChannelRequired,

    #[error("unknown channel: {0}")]
    UnknownChannel(String),

    #[error("unknown agent: {0}")]
    UnknownAgent(String),

    #[error("--response-delay-ms requires a value")]
    MissingResponseDelayValue,

    #[error("--response-delay-ms must be a non-negative integer in milliseconds: {0}")]
    InvalidResponseDelay(String),

    #[error("option specified more than once: {0}")]
    DuplicateOption(&'static str),
}

#[derive(Debug, thiserror::Error, Clone, PartialEq, Eq)]
#[error("--server-npub must contain an NIP-19 npub or nprofile")]
pub struct ChiihouServerNpubError;

pub fn validate_server_npub(value: &str) -> Result<String, ChiihouServerNpubError> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err(ChiihouServerNpubError);
    }
    if PublicKey::from_bech32(trimmed).is_err() && Nip19Profile::from_bech32(trimmed).is_err() {
        return Err(ChiihouServerNpubError);
    }
    Ok(trimmed.to_string())
}

pub fn resolve_server_npub(override_npub: Option<&str>) -> Result<String, ChiihouServerNpubError> {
    match override_npub {
        Some(value) => validate_server_npub(value),
        None => validate_server_npub(CHIIHOU_SERVER_NPUB),
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ChiihouStartupConfigError {
    #[error(transparent)]
    Secret(#[from] ChiihouSecretError),

    #[error(transparent)]
    ServerNpub(#[from] ChiihouServerNpubError),

    #[error(transparent)]
    Nostr(#[from] ChiihouConfigError),
}

pub fn build_cli_nostr_config(
    nsec: &str,
    args: &ChiihouCliArgs,
) -> Result<ChiihouNostrConfig, ChiihouStartupConfigError> {
    let valid_nsec = validate_chiihou_nsec(nsec)?;
    let server_npub = resolve_server_npub(args.server_npub.as_deref())?;
    let config = ChiihouNostrConfig::new(&valid_nsec, &server_npub, args.channel)?;
    Ok(config)
}

#[cfg(test)]
mod tests {
    use super::*;
    use nostr_sdk::{Keys, RelayUrl, ToBech32};

    // テスト専用の秘密鍵。実際の運用で使用してはならない。
    const TEST_AI_SECRET_KEY_HEX: &str =
        "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

    // テスト専用の秘密鍵。server 公開鍵の導出のみに使用する。
    const TEST_SERVER_SECRET_KEY_HEX: &str =
        "fedcba9876543210fedcba9876543210fedcba9876543210fedcba9876543210";

    fn server_npub() -> String {
        Keys::parse(TEST_SERVER_SECRET_KEY_HEX)
            .unwrap()
            .public_key()
            .to_bech32()
            .unwrap()
    }

    fn server_nprofile() -> String {
        let relay = RelayUrl::parse("wss://hint.example.com/").unwrap();
        Nip19Profile::new(
            Keys::parse(TEST_SERVER_SECRET_KEY_HEX)
                .unwrap()
                .public_key(),
            [relay],
        )
        .to_bech32()
        .unwrap()
    }

    fn ai_nsec() -> String {
        Keys::parse(TEST_AI_SECRET_KEY_HEX)
            .unwrap()
            .secret_key()
            .to_bech32()
            .unwrap()
    }

    fn parse(args: &[&str]) -> Result<ChiihouCliArgs, ChiihouCliError> {
        ChiihouCliArgs::parse(args.iter().map(|s| s.to_string()))
    }

    #[test]
    fn parses_without_server_npub() {
        let args = parse(&["--channel", "hanchan"]).unwrap();
        assert_eq!(args.server_npub, None);
        assert_eq!(args.channel, ChiihouChannel::Hanchan);
        assert_eq!(args.agent, ChiihouAgentKind::Normal);
        assert!(!args.auto_next);
    }

    #[test]
    fn auto_next_defaults_to_false() {
        let args = parse(&["--channel", "hanchan"]).unwrap();
        assert!(!args.auto_next);
        let args = parse(&["--channel", "tonpuu", "--agent", "shanten"]).unwrap();
        assert!(!args.auto_next);
    }

    #[test]
    fn parses_auto_next_flag() {
        let args = parse(&["--channel", "hanchan", "--auto-next"]).unwrap();
        assert!(args.auto_next);
        assert_eq!(args.channel, ChiihouChannel::Hanchan);
    }

    #[test]
    fn parses_auto_next_before_other_options() {
        let args = parse(&["--auto-next", "--channel", "tonpuu"]).unwrap();
        assert!(args.auto_next);
        assert_eq!(args.channel, ChiihouChannel::Tonpuu);
    }

    #[test]
    fn parses_auto_next_with_agent_in_any_order() {
        let args = parse(&["--channel", "hanchan", "--agent", "shanten", "--auto-next"]).unwrap();
        assert!(args.auto_next);
        assert_eq!(args.agent, ChiihouAgentKind::Shanten);
        let args = parse(&["--agent", "shanten", "--auto-next", "--channel", "hanchan"]).unwrap();
        assert!(args.auto_next);
        assert_eq!(args.agent, ChiihouAgentKind::Shanten);
        assert_eq!(args.channel, ChiihouChannel::Hanchan);
    }

    #[test]
    fn rejects_duplicate_auto_next() {
        assert_eq!(
            parse(&["--channel", "hanchan", "--auto-next", "--auto-next"]),
            Err(ChiihouCliError::DuplicateOption("--auto-next"))
        );
    }

    #[test]
    fn rejects_auto_next_with_inline_value() {
        assert_eq!(
            parse(&["--channel", "hanchan", "--auto-next=true"]),
            Err(ChiihouCliError::UnknownOption(
                "--auto-next=true".to_string()
            ))
        );
    }

    #[test]
    fn rejects_no_auto_next() {
        assert_eq!(
            parse(&["--channel", "hanchan", "--no-auto-next"]),
            Err(ChiihouCliError::UnknownOption("--no-auto-next".to_string()))
        );
    }

    #[test]
    fn rejects_auto_next_with_separate_value() {
        assert_eq!(
            parse(&["--channel", "hanchan", "--auto-next", "false"]),
            Err(ChiihouCliError::UnknownOption("false".to_string()))
        );
    }

    #[test]
    fn usage_mentions_auto_next() {
        assert!(USAGE.contains("--auto-next"));
    }

    #[test]
    fn response_delay_defaults_to_zero() {
        let args = parse(&["--channel", "hanchan"]).unwrap();
        assert_eq!(args.response_delay_ms, 0);
        assert!(!args.auto_next);
    }

    #[test]
    fn parses_response_delay_ms() {
        let args = parse(&["--channel", "hanchan", "--response-delay-ms", "1000"]).unwrap();
        assert_eq!(args.response_delay_ms, 1000);
    }

    #[test]
    fn parses_response_delay_before_channel() {
        let args = parse(&["--response-delay-ms", "5000", "--channel", "tonpuu"]).unwrap();
        assert_eq!(args.response_delay_ms, 5000);
        assert_eq!(args.channel, ChiihouChannel::Tonpuu);
    }

    #[test]
    fn parses_response_delay_with_auto_next() {
        let args = parse(&[
            "--channel",
            "hanchan",
            "--auto-next",
            "--response-delay-ms",
            "5000",
        ])
        .unwrap();
        assert!(args.auto_next);
        assert_eq!(args.response_delay_ms, 5000);
    }

    #[test]
    fn accepts_zero_response_delay() {
        let args = parse(&["--channel", "hanchan", "--response-delay-ms", "0"]).unwrap();
        assert_eq!(args.response_delay_ms, 0);
    }

    #[test]
    fn rejects_missing_response_delay_value() {
        assert_eq!(
            parse(&["--channel", "hanchan", "--response-delay-ms"]),
            Err(ChiihouCliError::MissingResponseDelayValue)
        );
    }

    #[test]
    fn rejects_non_integer_response_delay() {
        assert_eq!(
            parse(&["--channel", "hanchan", "--response-delay-ms", "abc"]),
            Err(ChiihouCliError::InvalidResponseDelay("abc".to_string()))
        );
    }

    #[test]
    fn rejects_negative_response_delay() {
        assert_eq!(
            parse(&["--channel", "hanchan", "--response-delay-ms", "-1"]),
            Err(ChiihouCliError::InvalidResponseDelay("-1".to_string()))
        );
    }

    #[test]
    fn rejects_fractional_response_delay() {
        assert_eq!(
            parse(&["--channel", "hanchan", "--response-delay-ms", "1.5"]),
            Err(ChiihouCliError::InvalidResponseDelay("1.5".to_string()))
        );
    }

    #[test]
    fn rejects_duplicate_response_delay() {
        assert_eq!(
            parse(&[
                "--channel",
                "hanchan",
                "--response-delay-ms",
                "100",
                "--response-delay-ms",
                "200"
            ]),
            Err(ChiihouCliError::DuplicateOption("--response-delay-ms"))
        );
    }

    #[test]
    fn rejects_response_delay_with_inline_value() {
        assert_eq!(
            parse(&["--channel", "hanchan", "--response-delay-ms=1000"]),
            Err(ChiihouCliError::UnknownOption(
                "--response-delay-ms=1000".to_string()
            ))
        );
    }

    #[test]
    fn usage_mentions_response_delay_ms() {
        assert!(USAGE.contains("--response-delay-ms <MILLISECONDS>"));
    }

    #[test]
    fn response_delay_converts_to_duration() {
        let args = parse(&["--channel", "hanchan"]).unwrap();
        assert_eq!(args.response_delay(), Duration::ZERO);

        let args = parse(&["--channel", "hanchan", "--response-delay-ms", "1000"]).unwrap();
        assert_eq!(args.response_delay(), Duration::from_millis(1000));

        let args = parse(&["--channel", "hanchan", "--response-delay-ms", "5000"]).unwrap();
        assert_eq!(args.response_delay(), Duration::from_millis(5000));
    }

    #[test]
    fn parses_with_server_npub() {
        let args = parse(&["--server-npub", "npub1example", "--channel", "hanchan"]).unwrap();
        assert_eq!(args.server_npub, Some("npub1example".to_string()));
        assert_eq!(args.channel, ChiihouChannel::Hanchan);
        assert_eq!(args.agent, ChiihouAgentKind::Normal);
    }

    #[test]
    fn parses_options_in_any_order() {
        let args = parse(&[
            "--agent",
            "shanten",
            "--channel",
            "tonpuu",
            "--server-npub",
            "npub1example",
        ])
        .unwrap();
        assert_eq!(args.server_npub, Some("npub1example".to_string()));
        assert_eq!(args.channel, ChiihouChannel::Tonpuu);
        assert_eq!(args.agent, ChiihouAgentKind::Shanten);
    }

    #[test]
    fn parses_agent_normal() {
        let args = parse(&["--channel", "hanchan", "--agent", "normal"]).unwrap();
        assert_eq!(args.agent, ChiihouAgentKind::Normal);
    }

    #[test]
    fn parses_agent_tsumogiri() {
        let args = parse(&["--channel", "hanchan", "--agent", "tsumogiri"]).unwrap();
        assert_eq!(args.agent, ChiihouAgentKind::Tsumogiri);
    }

    #[test]
    fn parses_agent_tsumogiri_with_hyphen() {
        let args = parse(&["--channel", "hanchan", "--agent", "tsumo-giri"]).unwrap();
        assert_eq!(args.agent, ChiihouAgentKind::Tsumogiri);
    }

    #[test]
    fn parses_agent_shanten() {
        let args = parse(&["--channel", "hanchan", "--agent", "shanten"]).unwrap();
        assert_eq!(args.agent, ChiihouAgentKind::Shanten);
    }

    #[test]
    fn parses_agent_menzen() {
        let args = parse(&["--channel", "hanchan", "--agent", "menzen"]).unwrap();
        assert_eq!(args.agent, ChiihouAgentKind::Menzen);
    }

    #[test]
    fn defaults_to_normal_agent() {
        let args = parse(&["--channel", "hanchan"]).unwrap();
        assert_eq!(args.agent, ChiihouAgentKind::Normal);
    }

    #[test]
    fn channel_ignores_ascii_case() {
        let args = parse(&["--channel", "HANCHAN"]).unwrap();
        assert_eq!(args.channel, ChiihouChannel::Hanchan);
        let args = parse(&["--channel", "Tonpuu"]).unwrap();
        assert_eq!(args.channel, ChiihouChannel::Tonpuu);
    }

    #[test]
    fn agent_ignores_ascii_case() {
        let args = parse(&["--channel", "hanchan", "--agent", "Shanten"]).unwrap();
        assert_eq!(args.agent, ChiihouAgentKind::Shanten);
        let args = parse(&["--channel", "hanchan", "--agent", "TSUMOGIRI"]).unwrap();
        assert_eq!(args.agent, ChiihouAgentKind::Tsumogiri);
        let args = parse(&["--channel", "hanchan", "--agent", "Menzen"]).unwrap();
        assert_eq!(args.agent, ChiihouAgentKind::Menzen);
    }

    #[test]
    fn rejects_missing_server_npub_value() {
        assert_eq!(
            parse(&["--channel", "hanchan", "--server-npub"]),
            Err(ChiihouCliError::MissingServerNpubValue)
        );
    }

    #[test]
    fn rejects_missing_channel_option() {
        assert_eq!(
            parse(&["--server-npub", "npub1example"]),
            Err(ChiihouCliError::ChannelRequired)
        );
    }

    #[test]
    fn rejects_missing_channel_value() {
        assert_eq!(
            parse(&["--channel"]),
            Err(ChiihouCliError::MissingChannelValue)
        );
    }

    #[test]
    fn rejects_missing_agent_value() {
        assert_eq!(
            parse(&["--channel", "hanchan", "--agent"]),
            Err(ChiihouCliError::MissingAgentValue)
        );
    }

    #[test]
    fn rejects_unknown_option() {
        assert_eq!(
            parse(&["--nsec", "value"]),
            Err(ChiihouCliError::UnknownOption("--nsec".to_string()))
        );
    }

    #[test]
    fn rejects_unknown_channel() {
        assert_eq!(
            parse(&["--channel", "sanma"]),
            Err(ChiihouCliError::UnknownChannel("sanma".to_string()))
        );
    }

    #[test]
    fn rejects_unknown_agent() {
        assert_eq!(
            parse(&["--channel", "hanchan", "--agent", "nodocchi"]),
            Err(ChiihouCliError::UnknownAgent("nodocchi".to_string()))
        );
    }

    #[test]
    fn rejects_positional_argument() {
        assert_eq!(
            parse(&["hanchan"]),
            Err(ChiihouCliError::UnknownOption("hanchan".to_string()))
        );
    }

    #[test]
    fn rejects_duplicate_server_npub() {
        assert_eq!(
            parse(&[
                "--server-npub",
                "npub1example",
                "--server-npub",
                "npub1other",
                "--channel",
                "hanchan"
            ]),
            Err(ChiihouCliError::DuplicateOption("--server-npub"))
        );
    }

    #[test]
    fn rejects_duplicate_channel() {
        assert_eq!(
            parse(&["--channel", "hanchan", "--channel", "tonpuu"]),
            Err(ChiihouCliError::DuplicateOption("--channel"))
        );
    }

    #[test]
    fn rejects_duplicate_agent() {
        assert_eq!(
            parse(&[
                "--channel",
                "hanchan",
                "--agent",
                "normal",
                "--agent",
                "shanten"
            ]),
            Err(ChiihouCliError::DuplicateOption("--agent"))
        );
    }

    #[test]
    fn agent_kind_default_is_normal() {
        assert_eq!(ChiihouAgentKind::default(), ChiihouAgentKind::Normal);
    }

    #[test]
    fn agent_kind_from_str_trims_whitespace() {
        assert_eq!(
            " shanten ".parse::<ChiihouAgentKind>().unwrap(),
            ChiihouAgentKind::Shanten
        );
    }

    #[test]
    fn agent_kind_display_matches_input_format() {
        assert_eq!(ChiihouAgentKind::Normal.to_string(), "normal");
        assert_eq!(ChiihouAgentKind::Tsumogiri.to_string(), "tsumogiri");
        assert_eq!(ChiihouAgentKind::Shanten.to_string(), "shanten");
        assert_eq!(ChiihouAgentKind::Menzen.to_string(), "menzen");
    }

    #[test]
    fn accepts_valid_server_npub() {
        let npub = server_npub();
        assert_eq!(validate_server_npub(&npub).unwrap(), npub);
    }

    #[test]
    fn accepts_valid_server_nprofile() {
        let nprofile = server_nprofile();
        assert_eq!(validate_server_npub(&nprofile).unwrap(), nprofile);
    }

    #[test]
    fn trims_server_npub_whitespace() {
        let npub = server_npub();
        let padded = format!("  {npub}\n");
        assert_eq!(validate_server_npub(&padded).unwrap(), npub);
    }

    #[test]
    fn rejects_hex_server_pubkey() {
        let hex = Keys::parse(TEST_SERVER_SECRET_KEY_HEX)
            .unwrap()
            .public_key()
            .to_hex();
        assert_eq!(validate_server_npub(&hex), Err(ChiihouServerNpubError));
    }

    #[test]
    fn rejects_nsec_as_server_npub() {
        assert_eq!(
            validate_server_npub(&ai_nsec()),
            Err(ChiihouServerNpubError)
        );
    }

    #[test]
    fn rejects_nostr_uri_server_npub() {
        let uri = format!("nostr:{}", server_npub());
        assert_eq!(validate_server_npub(&uri), Err(ChiihouServerNpubError));
    }

    #[test]
    fn rejects_malformed_server_npub() {
        assert_eq!(
            validate_server_npub("npub1invalid"),
            Err(ChiihouServerNpubError)
        );
    }

    #[test]
    fn rejects_empty_server_npub() {
        assert_eq!(validate_server_npub(""), Err(ChiihouServerNpubError));
        assert_eq!(validate_server_npub("   "), Err(ChiihouServerNpubError));
    }

    #[test]
    fn resolves_default_server_npub_without_override() {
        assert_eq!(resolve_server_npub(None).unwrap(), CHIIHOU_SERVER_NPUB);
    }

    #[test]
    fn resolves_override_server_npub() {
        let npub = server_npub();
        assert_eq!(resolve_server_npub(Some(&npub)).unwrap(), npub);
    }

    #[test]
    fn resolves_override_server_nprofile() {
        let nprofile = server_nprofile();
        assert_eq!(resolve_server_npub(Some(&nprofile)).unwrap(), nprofile);
    }

    #[test]
    fn rejects_invalid_override_server_npub() {
        assert_eq!(
            resolve_server_npub(Some("npub1invalid")),
            Err(ChiihouServerNpubError)
        );
    }

    #[test]
    fn rejects_hex_override_server_pubkey() {
        let hex = Keys::parse(TEST_SERVER_SECRET_KEY_HEX)
            .unwrap()
            .public_key()
            .to_hex();
        assert_eq!(resolve_server_npub(Some(&hex)), Err(ChiihouServerNpubError));
    }

    #[test]
    fn rejects_nsec_override_server_npub() {
        assert_eq!(
            resolve_server_npub(Some(&ai_nsec())),
            Err(ChiihouServerNpubError)
        );
    }

    #[test]
    fn default_server_npub_is_valid_npub() {
        assert!(PublicKey::from_bech32(CHIIHOU_SERVER_NPUB).is_ok());
    }

    #[test]
    fn builds_config_from_valid_inputs() {
        let args = ChiihouCliArgs {
            server_npub: Some(server_npub()),
            channel: ChiihouChannel::Hanchan,
            agent: ChiihouAgentKind::Normal,
            auto_next: false,
            response_delay_ms: 0,
        };
        let config = build_cli_nostr_config(&ai_nsec(), &args).unwrap();
        let expected = Keys::parse(TEST_AI_SECRET_KEY_HEX).unwrap();
        assert_eq!(
            config.keys().public_key().to_hex(),
            expected.public_key().to_hex()
        );
        assert_eq!(config.event_config().server_npub, server_npub());
    }

    #[test]
    fn builds_config_with_default_server() {
        let args = ChiihouCliArgs {
            server_npub: None,
            channel: ChiihouChannel::Hanchan,
            agent: ChiihouAgentKind::Normal,
            auto_next: false,
            response_delay_ms: 0,
        };
        let config = build_cli_nostr_config(&ai_nsec(), &args).unwrap();
        assert_eq!(config.event_config().server_npub, CHIIHOU_SERVER_NPUB);
    }

    #[test]
    fn builds_config_with_nprofile_server() {
        let args = ChiihouCliArgs {
            server_npub: Some(server_nprofile()),
            channel: ChiihouChannel::Hanchan,
            agent: ChiihouAgentKind::Normal,
            auto_next: false,
            response_delay_ms: 0,
        };
        let config = build_cli_nostr_config(&ai_nsec(), &args).unwrap();
        assert_eq!(config.event_config().server_npub, server_npub());
    }

    #[test]
    fn build_config_rejects_hex_nsec() {
        let args = ChiihouCliArgs {
            server_npub: Some(server_npub()),
            channel: ChiihouChannel::Hanchan,
            agent: ChiihouAgentKind::Normal,
            auto_next: false,
            response_delay_ms: 0,
        };
        let result = build_cli_nostr_config(TEST_AI_SECRET_KEY_HEX, &args);
        assert!(matches!(
            result,
            Err(ChiihouStartupConfigError::Secret(
                ChiihouSecretError::InvalidNsec
            ))
        ));
    }

    #[test]
    fn build_config_rejects_hex_server_pubkey() {
        let hex = Keys::parse(TEST_SERVER_SECRET_KEY_HEX)
            .unwrap()
            .public_key()
            .to_hex();
        let args = ChiihouCliArgs {
            server_npub: Some(hex),
            channel: ChiihouChannel::Hanchan,
            agent: ChiihouAgentKind::Normal,
            auto_next: false,
            response_delay_ms: 0,
        };
        let result = build_cli_nostr_config(&ai_nsec(), &args);
        assert!(matches!(
            result,
            Err(ChiihouStartupConfigError::ServerNpub(_))
        ));
    }
}
