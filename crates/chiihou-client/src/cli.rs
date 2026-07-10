use nostr_sdk::{FromBech32, PublicKey};

use crate::config::{ChiihouChannel, ChiihouConfigError, ChiihouNostrConfig};
use crate::secret::{ChiihouSecretError, validate_chiihou_nsec};

pub const USAGE: &str = "usage: chiihou-client --server-npub <NPUB> --channel <hanchan|tonpuu> [--agent normal|tsumogiri|shanten]";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ChiihouAgentKind {
    #[default]
    Normal,
    Tsumogiri,
    Shanten,
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
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChiihouCliArgs {
    pub server_npub: String,
    pub channel: ChiihouChannel,
    pub agent: ChiihouAgentKind,
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
                other => return Err(ChiihouCliError::UnknownOption(other.to_string())),
            }
        }

        Ok(Self {
            server_npub: server_npub.ok_or(ChiihouCliError::ServerNpubRequired)?,
            channel: channel.ok_or(ChiihouCliError::ChannelRequired)?,
            agent: agent.unwrap_or_default(),
        })
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

    #[error("--server-npub is required")]
    ServerNpubRequired,

    #[error("--channel is required")]
    ChannelRequired,

    #[error("unknown channel: {0}")]
    UnknownChannel(String),

    #[error("unknown agent: {0}")]
    UnknownAgent(String),

    #[error("option specified more than once: {0}")]
    DuplicateOption(&'static str),
}

#[derive(Debug, thiserror::Error, Clone, PartialEq, Eq)]
#[error("--server-npub must contain an NIP-19 npub public key")]
pub struct ChiihouServerNpubError;

pub fn validate_server_npub(value: &str) -> Result<String, ChiihouServerNpubError> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err(ChiihouServerNpubError);
    }
    PublicKey::from_bech32(trimmed).map_err(|_| ChiihouServerNpubError)?;
    Ok(trimmed.to_string())
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
    let valid_npub = validate_server_npub(&args.server_npub)?;
    let config = ChiihouNostrConfig::new(&valid_nsec, &valid_npub, args.channel)?;
    Ok(config)
}

#[cfg(test)]
mod tests {
    use super::*;
    use nostr_sdk::{Keys, ToBech32};

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
    fn parses_required_options() {
        let args = parse(&["--server-npub", "npub1example", "--channel", "hanchan"]).unwrap();
        assert_eq!(args.server_npub, "npub1example");
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
        assert_eq!(args.server_npub, "npub1example");
        assert_eq!(args.channel, ChiihouChannel::Tonpuu);
        assert_eq!(args.agent, ChiihouAgentKind::Shanten);
    }

    #[test]
    fn parses_agent_normal() {
        let args = parse(&[
            "--server-npub",
            "npub1example",
            "--channel",
            "hanchan",
            "--agent",
            "normal",
        ])
        .unwrap();
        assert_eq!(args.agent, ChiihouAgentKind::Normal);
    }

    #[test]
    fn parses_agent_tsumogiri() {
        let args = parse(&[
            "--server-npub",
            "npub1example",
            "--channel",
            "hanchan",
            "--agent",
            "tsumogiri",
        ])
        .unwrap();
        assert_eq!(args.agent, ChiihouAgentKind::Tsumogiri);
    }

    #[test]
    fn parses_agent_tsumogiri_with_hyphen() {
        let args = parse(&[
            "--server-npub",
            "npub1example",
            "--channel",
            "hanchan",
            "--agent",
            "tsumo-giri",
        ])
        .unwrap();
        assert_eq!(args.agent, ChiihouAgentKind::Tsumogiri);
    }

    #[test]
    fn parses_agent_shanten() {
        let args = parse(&[
            "--server-npub",
            "npub1example",
            "--channel",
            "hanchan",
            "--agent",
            "shanten",
        ])
        .unwrap();
        assert_eq!(args.agent, ChiihouAgentKind::Shanten);
    }

    #[test]
    fn defaults_to_normal_agent() {
        let args = parse(&["--server-npub", "npub1example", "--channel", "hanchan"]).unwrap();
        assert_eq!(args.agent, ChiihouAgentKind::Normal);
    }

    #[test]
    fn channel_ignores_ascii_case() {
        let args = parse(&["--server-npub", "npub1example", "--channel", "HANCHAN"]).unwrap();
        assert_eq!(args.channel, ChiihouChannel::Hanchan);
        let args = parse(&["--server-npub", "npub1example", "--channel", "Tonpuu"]).unwrap();
        assert_eq!(args.channel, ChiihouChannel::Tonpuu);
    }

    #[test]
    fn agent_ignores_ascii_case() {
        let args = parse(&[
            "--server-npub",
            "npub1example",
            "--channel",
            "hanchan",
            "--agent",
            "Shanten",
        ])
        .unwrap();
        assert_eq!(args.agent, ChiihouAgentKind::Shanten);
        let args = parse(&[
            "--server-npub",
            "npub1example",
            "--channel",
            "hanchan",
            "--agent",
            "TSUMOGIRI",
        ])
        .unwrap();
        assert_eq!(args.agent, ChiihouAgentKind::Tsumogiri);
    }

    #[test]
    fn rejects_missing_server_npub_option() {
        assert_eq!(
            parse(&["--channel", "hanchan"]),
            Err(ChiihouCliError::ServerNpubRequired)
        );
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
            parse(&["--server-npub", "npub1example", "--channel"]),
            Err(ChiihouCliError::MissingChannelValue)
        );
    }

    #[test]
    fn rejects_missing_agent_value() {
        assert_eq!(
            parse(&[
                "--server-npub",
                "npub1example",
                "--channel",
                "hanchan",
                "--agent"
            ]),
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
            parse(&["--server-npub", "npub1example", "--channel", "sanma"]),
            Err(ChiihouCliError::UnknownChannel("sanma".to_string()))
        );
    }

    #[test]
    fn rejects_unknown_agent() {
        assert_eq!(
            parse(&[
                "--server-npub",
                "npub1example",
                "--channel",
                "hanchan",
                "--agent",
                "nodocchi"
            ]),
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
            parse(&[
                "--server-npub",
                "npub1example",
                "--channel",
                "hanchan",
                "--channel",
                "tonpuu"
            ]),
            Err(ChiihouCliError::DuplicateOption("--channel"))
        );
    }

    #[test]
    fn rejects_duplicate_agent() {
        assert_eq!(
            parse(&[
                "--server-npub",
                "npub1example",
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
    }

    #[test]
    fn accepts_valid_server_npub() {
        let npub = server_npub();
        assert_eq!(validate_server_npub(&npub).unwrap(), npub);
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
    fn builds_config_from_valid_inputs() {
        let args = ChiihouCliArgs {
            server_npub: server_npub(),
            channel: ChiihouChannel::Hanchan,
            agent: ChiihouAgentKind::Normal,
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
    fn build_config_rejects_hex_nsec() {
        let args = ChiihouCliArgs {
            server_npub: server_npub(),
            channel: ChiihouChannel::Hanchan,
            agent: ChiihouAgentKind::Normal,
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
            server_npub: hex,
            channel: ChiihouChannel::Hanchan,
            agent: ChiihouAgentKind::Normal,
        };
        let result = build_cli_nostr_config(&ai_nsec(), &args);
        assert!(matches!(
            result,
            Err(ChiihouStartupConfigError::ServerNpub(_))
        ));
    }
}
