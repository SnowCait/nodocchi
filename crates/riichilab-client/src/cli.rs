use std::path::PathBuf;

use crate::config::ClientConfig;
use crate::validation_policy::AgentKind;

pub const USAGE: &str = "usage: riichilab-client [validate|ranked] [--agent normal|tsumogiri|shanten|menzen] [--log-file <PATH>]";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ConnectionMode {
    #[default]
    Validate,
    Ranked,
}

impl ConnectionMode {
    pub fn endpoint(self) -> &'static str {
        match self {
            Self::Validate => ClientConfig::DEFAULT_VALIDATE_ENDPOINT,
            Self::Ranked => ClientConfig::RANKED_ENDPOINT,
        }
    }
}

impl std::fmt::Display for ConnectionMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Validate => write!(f, "validate"),
            Self::Ranked => write!(f, "ranked"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CliArgs {
    pub mode: ConnectionMode,
    pub agent: Option<AgentKind>,
    pub log_file: Option<PathBuf>,
}

impl CliArgs {
    pub fn parse<I>(args: I) -> Result<Self, CliError>
    where
        I: IntoIterator<Item = String>,
    {
        let mut args = args.into_iter();
        let mut mode = None;
        let mut agent = None;
        let mut log_file = None;

        while let Some(arg) = args.next() {
            match arg.as_str() {
                "validate" if mode.is_none() => mode = Some(ConnectionMode::Validate),
                "ranked" if mode.is_none() => mode = Some(ConnectionMode::Ranked),
                "--agent" => {
                    let value = args.next().ok_or(CliError::MissingAgentValue)?;
                    agent = Some(
                        value
                            .parse::<AgentKind>()
                            .map_err(|_| CliError::UnknownAgent(value))?,
                    );
                }
                "--log-file" => {
                    let value = args.next().ok_or(CliError::MissingLogFileValue)?;
                    log_file = Some(PathBuf::from(value));
                }
                other if other.starts_with('-') => {
                    return Err(CliError::UnknownOption(other.to_string()));
                }
                other => return Err(CliError::UnknownMode(other.to_string())),
            }
        }

        Ok(Self {
            mode: mode.unwrap_or_default(),
            agent,
            log_file,
        })
    }
}

#[derive(Debug, thiserror::Error, Clone, PartialEq, Eq)]
pub enum CliError {
    #[error("unknown mode: {0}")]
    UnknownMode(String),

    #[error("unknown option: {0}")]
    UnknownOption(String),

    #[error("--agent requires a value")]
    MissingAgentValue,

    #[error("unknown agent: {0}")]
    UnknownAgent(String),

    #[error("--log-file requires a value")]
    MissingLogFileValue,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(args: &[&str]) -> Result<CliArgs, CliError> {
        CliArgs::parse(args.iter().map(|s| s.to_string()))
    }

    #[test]
    fn defaults_to_validate_mode_without_agent() {
        let args = parse(&[]).unwrap();
        assert_eq!(args.mode, ConnectionMode::Validate);
        assert_eq!(args.agent, None);
        assert_eq!(args.log_file, None);
    }

    #[test]
    fn parses_validate_mode() {
        let args = parse(&["validate"]).unwrap();
        assert_eq!(args.mode, ConnectionMode::Validate);
    }

    #[test]
    fn parses_ranked_mode() {
        let args = parse(&["ranked"]).unwrap();
        assert_eq!(args.mode, ConnectionMode::Ranked);
    }

    #[test]
    fn parses_agent_option() {
        let args = parse(&["validate", "--agent", "shanten"]).unwrap();
        assert_eq!(args.mode, ConnectionMode::Validate);
        assert_eq!(args.agent, Some(AgentKind::Shanten));
    }

    #[test]
    fn parses_agent_menzen() {
        let args = parse(&["validate", "--agent", "menzen"]).unwrap();
        assert_eq!(args.agent, Some(AgentKind::Menzen));
    }

    #[test]
    fn parses_agent_option_without_mode() {
        let args = parse(&["--agent", "tsumogiri"]).unwrap();
        assert_eq!(args.mode, ConnectionMode::Validate);
        assert_eq!(args.agent, Some(AgentKind::Tsumogiri));
    }

    #[test]
    fn parses_log_file_option() {
        let args = parse(&["--log-file", "logs/ranked.log"]).unwrap();
        assert_eq!(args.log_file, Some(PathBuf::from("logs/ranked.log")));
    }

    #[test]
    fn parses_log_file_option_with_mode_and_agent() {
        let args = parse(&[
            "ranked",
            "--agent",
            "shanten",
            "--log-file",
            "logs/ranked.log",
        ])
        .unwrap();
        assert_eq!(args.mode, ConnectionMode::Ranked);
        assert_eq!(args.agent, Some(AgentKind::Shanten));
        assert_eq!(args.log_file, Some(PathBuf::from("logs/ranked.log")));
    }

    #[test]
    fn rejects_missing_log_file_value() {
        assert_eq!(parse(&["--log-file"]), Err(CliError::MissingLogFileValue));
    }

    #[test]
    fn rejects_unknown_mode() {
        assert_eq!(
            parse(&["practice"]),
            Err(CliError::UnknownMode("practice".to_string()))
        );
    }

    #[test]
    fn rejects_duplicate_mode() {
        assert_eq!(
            parse(&["validate", "ranked"]),
            Err(CliError::UnknownMode("ranked".to_string()))
        );
    }

    #[test]
    fn rejects_unknown_option() {
        assert_eq!(
            parse(&["--endpoint"]),
            Err(CliError::UnknownOption("--endpoint".to_string()))
        );
    }

    #[test]
    fn rejects_missing_agent_value() {
        assert_eq!(parse(&["--agent"]), Err(CliError::MissingAgentValue));
    }

    #[test]
    fn rejects_unknown_agent() {
        assert_eq!(
            parse(&["--agent", "nodocchi"]),
            Err(CliError::UnknownAgent("nodocchi".to_string()))
        );
    }

    #[test]
    fn validate_mode_uses_validate_endpoint() {
        assert_eq!(
            ConnectionMode::Validate.endpoint(),
            "wss://game.riichi.dev/ws/validate"
        );
    }

    #[test]
    fn ranked_mode_uses_ranked_endpoint() {
        assert_eq!(
            ConnectionMode::Ranked.endpoint(),
            "wss://game.riichi.dev/ws/ranked"
        );
    }
}
