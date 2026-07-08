#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClientConfig {
    pub endpoint: String,
    pub token: String,
}

impl ClientConfig {
    pub const DEFAULT_VALIDATE_ENDPOINT: &'static str = "wss://game.riichi.dev/ws/validate";
    pub const RANKED_ENDPOINT: &'static str = "wss://game.riichi.dev/ws/ranked";

    pub fn from_parts(token: String, endpoint: Option<String>) -> Self {
        Self {
            endpoint: endpoint.unwrap_or_else(|| Self::DEFAULT_VALIDATE_ENDPOINT.to_string()),
            token,
        }
    }

    pub fn from_env_with_endpoint(endpoint: String) -> Result<Self, ConfigError> {
        let token =
            std::env::var("RIICHILAB_BOT_TOKEN").map_err(|_| ConfigError::MissingBotToken)?;
        Ok(Self::from_parts(token, Some(endpoint)))
    }
}

#[derive(Debug, thiserror::Error, Clone, PartialEq, Eq)]
pub enum ConfigError {
    #[error("RIICHILAB_BOT_TOKEN is not set")]
    MissingBotToken,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_parts_uses_default_endpoint_when_unset() {
        let config = ClientConfig::from_parts("token".to_string(), None);
        assert_eq!(config.endpoint, ClientConfig::DEFAULT_VALIDATE_ENDPOINT);
        assert_eq!(config.token, "token");
    }

    #[test]
    fn from_parts_uses_endpoint_override() {
        let config = ClientConfig::from_parts(
            "token".to_string(),
            Some("wss://example.com/ws".to_string()),
        );
        assert_eq!(config.endpoint, "wss://example.com/ws");
        assert_eq!(config.token, "token");
    }
}
