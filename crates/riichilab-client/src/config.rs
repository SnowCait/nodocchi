use crate::agent::{AgentKind, AgentKindError};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClientConfig {
    pub endpoint: String,
    pub token: String,
    pub agent_kind: AgentKind,
}

impl ClientConfig {
    pub const DEFAULT_VALIDATE_ENDPOINT: &'static str = "wss://game.riichi.dev/ws/validate";

    pub fn from_parts(token: String, endpoint: Option<String>, agent_kind: AgentKind) -> Self {
        Self {
            endpoint: endpoint.unwrap_or_else(|| Self::DEFAULT_VALIDATE_ENDPOINT.to_string()),
            token,
            agent_kind,
        }
    }

    pub fn from_env() -> Result<Self, ConfigError> {
        let token =
            std::env::var("RIICHILAB_BOT_TOKEN").map_err(|_| ConfigError::MissingBotToken)?;
        let endpoint = std::env::var("RIICHILAB_ENDPOINT").ok();
        let agent_kind = AgentKind::from_env()?;
        Ok(Self::from_parts(token, endpoint, agent_kind))
    }
}

#[derive(Debug, thiserror::Error, Clone, PartialEq, Eq)]
pub enum ConfigError {
    #[error("RIICHILAB_BOT_TOKEN is not set")]
    MissingBotToken,

    #[error(transparent)]
    AgentKind(#[from] AgentKindError),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_parts_uses_default_endpoint_when_unset() {
        let config = ClientConfig::from_parts("token".to_string(), None, AgentKind::default());
        assert_eq!(config.endpoint, ClientConfig::DEFAULT_VALIDATE_ENDPOINT);
        assert_eq!(config.token, "token");
        assert_eq!(config.agent_kind, AgentKind::Normal);
    }

    #[test]
    fn from_parts_uses_endpoint_override() {
        let config = ClientConfig::from_parts(
            "token".to_string(),
            Some("wss://example.com/ws".to_string()),
            AgentKind::default(),
        );
        assert_eq!(config.endpoint, "wss://example.com/ws");
        assert_eq!(config.token, "token");
    }

    #[test]
    fn from_parts_defaults_to_normal_agent_when_env_value_is_absent() {
        let agent_kind = AgentKind::default();
        let config = ClientConfig::from_parts("token".to_string(), None, agent_kind);
        assert_eq!(config.agent_kind, AgentKind::Normal);
    }

    #[test]
    fn from_parts_keeps_tsumogiri_agent() {
        let agent_kind = "tsumogiri".parse::<AgentKind>().unwrap();
        let config = ClientConfig::from_parts("token".to_string(), None, agent_kind);
        assert_eq!(config.agent_kind, AgentKind::Tsumogiri);
    }

    #[test]
    fn from_parts_keeps_normal_agent() {
        let agent_kind = "normal".parse::<AgentKind>().unwrap();
        let config = ClientConfig::from_parts("token".to_string(), None, agent_kind);
        assert_eq!(config.agent_kind, AgentKind::Normal);
    }

    #[test]
    fn unknown_agent_value_maps_to_agent_kind_config_error() {
        let error: ConfigError = "unknown-agent".parse::<AgentKind>().unwrap_err().into();
        assert_eq!(
            error,
            ConfigError::AgentKind(AgentKindError::Unknown("unknown-agent".to_string()))
        );
    }
}
