#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AgentKind {
    Tsumogiri,
    #[default]
    Normal,
}

impl AgentKind {
    pub fn from_env() -> Result<Self, AgentKindError> {
        match std::env::var("MAHJONG_AGENT") {
            Ok(value) => value.parse(),
            Err(std::env::VarError::NotPresent) => Ok(Self::default()),
            Err(std::env::VarError::NotUnicode(_)) => Err(AgentKindError::NotUnicode),
        }
    }
}

impl std::str::FromStr for AgentKind {
    type Err = AgentKindError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim().to_ascii_lowercase().as_str() {
            "" | "normal" => Ok(Self::Normal),
            "tsumogiri" | "tsumo-giri" => Ok(Self::Tsumogiri),
            other => Err(AgentKindError::Unknown(other.to_string())),
        }
    }
}

impl std::fmt::Display for AgentKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Tsumogiri => write!(f, "tsumogiri"),
            Self::Normal => write!(f, "normal"),
        }
    }
}

#[derive(Debug, thiserror::Error, Clone, PartialEq, Eq)]
pub enum AgentKindError {
    #[error("MAHJONG_AGENT is not valid unicode")]
    NotUnicode,

    #[error("unknown MAHJONG_AGENT: {0}")]
    Unknown(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_normal() {
        assert_eq!(AgentKind::default(), AgentKind::Normal);
    }

    #[test]
    fn parses_normal() {
        assert_eq!("normal".parse::<AgentKind>().unwrap(), AgentKind::Normal);
    }

    #[test]
    fn parses_empty_string_as_normal() {
        assert_eq!("".parse::<AgentKind>().unwrap(), AgentKind::Normal);
    }

    #[test]
    fn parses_tsumogiri() {
        assert_eq!(
            "tsumogiri".parse::<AgentKind>().unwrap(),
            AgentKind::Tsumogiri
        );
    }

    #[test]
    fn parses_tsumogiri_with_hyphen() {
        assert_eq!(
            "tsumo-giri".parse::<AgentKind>().unwrap(),
            AgentKind::Tsumogiri
        );
    }

    #[test]
    fn parses_mixed_case() {
        assert_eq!("Normal".parse::<AgentKind>().unwrap(), AgentKind::Normal);
        assert_eq!(
            "TsumoGiri".parse::<AgentKind>().unwrap(),
            AgentKind::Tsumogiri
        );
    }

    #[test]
    fn parses_with_surrounding_whitespace() {
        assert_eq!(
            " tsumogiri ".parse::<AgentKind>().unwrap(),
            AgentKind::Tsumogiri
        );
    }

    #[test]
    fn unknown_value_is_error() {
        assert_eq!(
            "nodocchi".parse::<AgentKind>(),
            Err(AgentKindError::Unknown("nodocchi".to_string()))
        );
    }

    #[test]
    fn display_matches_env_values() {
        assert_eq!(AgentKind::Normal.to_string(), "normal");
        assert_eq!(AgentKind::Tsumogiri.to_string(), "tsumogiri");
    }
}
