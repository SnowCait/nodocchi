#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "type")]
pub enum MjaiAction {
    #[serde(rename = "dahai")]
    Dahai {
        actor: u8,
        pai: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        tsumogiri: Option<bool>,
        #[serde(skip_serializing_if = "Option::is_none")]
        request_id: Option<u64>,
    },

    #[serde(rename = "reach")]
    Reach {
        actor: u8,
        #[serde(skip_serializing_if = "Option::is_none")]
        request_id: Option<u64>,
    },

    #[serde(rename = "hora")]
    Hora {
        actor: u8,
        #[serde(skip_serializing_if = "Option::is_none")]
        target: Option<u8>,
        #[serde(skip_serializing_if = "Option::is_none")]
        pai: Option<String>,
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
    fn dahai_serializes_with_all_fields() {
        let action = MjaiAction::Dahai {
            actor: 0,
            pai: "5mr".to_string(),
            tsumogiri: Some(true),
            request_id: Some(1),
        };
        let json = serde_json::to_string(&action).unwrap();
        assert_eq!(
            json,
            r#"{"type":"dahai","actor":0,"pai":"5mr","tsumogiri":true,"request_id":1}"#
        );
    }

    #[test]
    fn dahai_omits_absent_optional_fields() {
        let action = MjaiAction::Dahai {
            actor: 3,
            pai: "1m".to_string(),
            tsumogiri: None,
            request_id: None,
        };
        let json = serde_json::to_string(&action).unwrap();
        assert_eq!(json, r#"{"type":"dahai","actor":3,"pai":"1m"}"#);
    }

    #[test]
    fn dahai_roundtrip_echoes_request_id() {
        let json = r#"{"type":"dahai","actor":1,"pai":"1m","tsumogiri":false,"request_id":42}"#;
        let action: MjaiAction = serde_json::from_str(json).unwrap();
        assert_eq!(
            action,
            MjaiAction::Dahai {
                actor: 1,
                pai: "1m".to_string(),
                tsumogiri: Some(false),
                request_id: Some(42),
            }
        );
        assert_eq!(serde_json::to_string(&action).unwrap(), json);
    }

    #[test]
    fn reach_roundtrip_with_actor() {
        let action = MjaiAction::Reach {
            actor: 2,
            request_id: Some(7),
        };
        let json = serde_json::to_string(&action).unwrap();
        assert_eq!(json, r#"{"type":"reach","actor":2,"request_id":7}"#);
        let parsed: MjaiAction = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, action);
    }

    #[test]
    fn hora_roundtrip_with_all_fields() {
        let action = MjaiAction::Hora {
            actor: 1,
            target: Some(2),
            pai: Some("3m".to_string()),
            request_id: Some(42),
        };
        let json = serde_json::to_string(&action).unwrap();
        assert_eq!(
            json,
            r#"{"type":"hora","actor":1,"target":2,"pai":"3m","request_id":42}"#
        );
        let parsed: MjaiAction = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, action);
    }

    #[test]
    fn hora_omits_absent_optional_fields() {
        let action = MjaiAction::Hora {
            actor: 0,
            target: None,
            pai: None,
            request_id: None,
        };
        let json = serde_json::to_string(&action).unwrap();
        assert_eq!(json, r#"{"type":"hora","actor":0}"#);
    }

    #[test]
    fn none_omits_request_id_when_absent() {
        let action = MjaiAction::None { request_id: None };
        let json = serde_json::to_string(&action).unwrap();
        assert_eq!(json, r#"{"type":"none"}"#);
    }

    #[test]
    fn ryukyoku_and_none_roundtrip() {
        for json in [
            r#"{"type":"ryukyoku","request_id":7}"#,
            r#"{"type":"none","request_id":7}"#,
        ] {
            let action: MjaiAction = serde_json::from_str(json).unwrap();
            assert_eq!(serde_json::to_string(&action).unwrap(), json);
        }
    }

    #[test]
    fn missing_optional_fields_deserialize_as_none() {
        let action: MjaiAction = serde_json::from_str(r#"{"type":"reach","actor":2}"#).unwrap();
        assert_eq!(
            action,
            MjaiAction::Reach {
                actor: 2,
                request_id: None,
            }
        );
        let action: MjaiAction =
            serde_json::from_str(r#"{"type":"dahai","actor":0,"pai":"5s"}"#).unwrap();
        assert_eq!(
            action,
            MjaiAction::Dahai {
                actor: 0,
                pai: "5s".to_string(),
                tsumogiri: None,
                request_id: None,
            }
        );
    }

    #[test]
    fn reach_without_actor_fails_to_parse() {
        assert!(serde_json::from_str::<MjaiAction>(r#"{"type":"reach","request_id":7}"#).is_err());
    }

    #[test]
    fn unknown_type_fails_to_parse() {
        assert!(serde_json::from_str::<MjaiAction>(r#"{"type":"unknown"}"#).is_err());
    }
}
