#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "type")]
pub enum MjaiAction {
    #[serde(rename = "dahai")]
    Dahai {
        pai: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        request_id: Option<u64>,
    },

    #[serde(rename = "reach")]
    Reach {
        #[serde(skip_serializing_if = "Option::is_none")]
        request_id: Option<u64>,
    },

    #[serde(rename = "hora")]
    Hora {
        #[serde(skip_serializing_if = "Option::is_none")]
        request_id: Option<u64>,
    },

    #[serde(rename = "ryukyoku")]
    Ryukyoku {
        #[serde(skip_serializing_if = "Option::is_none")]
        request_id: Option<u64>,
    },

    #[serde(rename = "none")]
    None {
        #[serde(skip_serializing_if = "Option::is_none")]
        request_id: Option<u64>,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dahai_serializes_with_request_id() {
        let action = MjaiAction::Dahai {
            pai: "5mr".to_string(),
            request_id: Some(1),
        };
        let json = serde_json::to_string(&action).unwrap();
        assert_eq!(json, r#"{"type":"dahai","pai":"5mr","request_id":1}"#);
    }

    #[test]
    fn none_omits_request_id_when_absent() {
        let action = MjaiAction::None { request_id: None };
        let json = serde_json::to_string(&action).unwrap();
        assert_eq!(json, r#"{"type":"none"}"#);
    }

    #[test]
    fn dahai_roundtrip_echoes_request_id() {
        let json = r#"{"type":"dahai","pai":"1m","request_id":42}"#;
        let action: MjaiAction = serde_json::from_str(json).unwrap();
        assert_eq!(
            action,
            MjaiAction::Dahai {
                pai: "1m".to_string(),
                request_id: Some(42),
            }
        );
        assert_eq!(serde_json::to_string(&action).unwrap(), json);
    }

    #[test]
    fn simple_actions_roundtrip() {
        for json in [
            r#"{"type":"reach","request_id":7}"#,
            r#"{"type":"hora","request_id":7}"#,
            r#"{"type":"ryukyoku","request_id":7}"#,
            r#"{"type":"none","request_id":7}"#,
        ] {
            let action: MjaiAction = serde_json::from_str(json).unwrap();
            assert_eq!(serde_json::to_string(&action).unwrap(), json);
        }
    }

    #[test]
    fn missing_request_id_deserializes_as_none() {
        let action: MjaiAction = serde_json::from_str(r#"{"type":"reach"}"#).unwrap();
        assert_eq!(action, MjaiAction::Reach { request_id: None });
    }

    #[test]
    fn unknown_type_fails_to_parse() {
        assert!(serde_json::from_str::<MjaiAction>(r#"{"type":"unknown"}"#).is_err());
    }
}
