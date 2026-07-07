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

pub type TimeControl = serde_json::Value;

#[derive(Debug, Clone, PartialEq, serde::Deserialize)]
#[serde(tag = "type")]
pub enum MjaiEvent {
    #[serde(rename = "start_game")]
    StartGame { id: u8 },

    #[serde(rename = "request_action")]
    RequestAction {
        request_id: u64,
        #[serde(default)]
        time: Option<TimeControl>,
        possible_actions: Vec<MjaiAction>,
        observation: String,
    },

    #[serde(rename = "action_ack")]
    ActionAck {
        request_id: u64,
        status: ActionAckStatus,
        #[serde(default)]
        elapsed_ms: Option<u64>,
        #[serde(default)]
        bank_consumed_ms: Option<u64>,
        #[serde(default)]
        bank_ms: Option<u64>,
        #[serde(default)]
        message: Option<String>,
    },

    #[serde(rename = "end_game")]
    EndGame {
        #[serde(default)]
        scores: Vec<i32>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActionAckStatus {
    Accepted,
    Rejected,
    Unparseable,
    Stale,
    Defaulted,
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

    #[test]
    fn start_game_parses() {
        let event: MjaiEvent = serde_json::from_str(r#"{"type":"start_game","id":2}"#).unwrap();
        assert_eq!(event, MjaiEvent::StartGame { id: 2 });
    }

    #[test]
    fn request_action_parses_without_time() {
        let json = r#"{
            "type": "request_action",
            "request_id": 42,
            "possible_actions": [
                {"type": "none"},
                {"type": "dahai", "actor": 0, "pai": "5mr"}
            ],
            "observation": "dummy-base64"
        }"#;
        let event: MjaiEvent = serde_json::from_str(json).unwrap();
        assert_eq!(
            event,
            MjaiEvent::RequestAction {
                request_id: 42,
                time: None,
                possible_actions: vec![
                    MjaiAction::None { request_id: None },
                    MjaiAction::Dahai {
                        actor: 0,
                        pai: "5mr".to_string(),
                        tsumogiri: None,
                        request_id: None,
                    },
                ],
                observation: "dummy-base64".to_string(),
            }
        );
    }

    #[test]
    fn request_action_parses_with_time() {
        let json = r#"{
            "type": "request_action",
            "request_id": 1,
            "time": {"budget_ms": 5000, "bank_ms": 10000},
            "possible_actions": [{"type": "none"}],
            "observation": "obs"
        }"#;
        let event: MjaiEvent = serde_json::from_str(json).unwrap();
        match event {
            MjaiEvent::RequestAction {
                request_id, time, ..
            } => {
                assert_eq!(request_id, 1);
                let time = time.unwrap();
                assert_eq!(time["budget_ms"], 5000);
                assert_eq!(time["bank_ms"], 10000);
            }
            other => panic!("unexpected event: {other:?}"),
        }
    }

    #[test]
    fn action_ack_parses_with_all_fields() {
        let json = r#"{
            "type": "action_ack",
            "request_id": 42,
            "status": "accepted",
            "elapsed_ms": 120,
            "bank_consumed_ms": 0,
            "bank_ms": 10000,
            "message": "ok"
        }"#;
        let event: MjaiEvent = serde_json::from_str(json).unwrap();
        assert_eq!(
            event,
            MjaiEvent::ActionAck {
                request_id: 42,
                status: ActionAckStatus::Accepted,
                elapsed_ms: Some(120),
                bank_consumed_ms: Some(0),
                bank_ms: Some(10000),
                message: Some("ok".to_string()),
            }
        );
    }

    #[test]
    fn action_ack_parses_without_optional_fields() {
        let json = r#"{"type":"action_ack","request_id":7,"status":"rejected"}"#;
        let event: MjaiEvent = serde_json::from_str(json).unwrap();
        assert_eq!(
            event,
            MjaiEvent::ActionAck {
                request_id: 7,
                status: ActionAckStatus::Rejected,
                elapsed_ms: None,
                bank_consumed_ms: None,
                bank_ms: None,
                message: None,
            }
        );
    }

    #[test]
    fn action_ack_status_parses_all_variants() {
        for (json, expected) in [
            (r#""accepted""#, ActionAckStatus::Accepted),
            (r#""rejected""#, ActionAckStatus::Rejected),
            (r#""unparseable""#, ActionAckStatus::Unparseable),
            (r#""stale""#, ActionAckStatus::Stale),
            (r#""defaulted""#, ActionAckStatus::Defaulted),
        ] {
            let status: ActionAckStatus = serde_json::from_str(json).unwrap();
            assert_eq!(status, expected);
        }
    }

    #[test]
    fn action_ack_status_rejects_unknown() {
        assert!(serde_json::from_str::<ActionAckStatus>(r#""unknown""#).is_err());
    }

    #[test]
    fn end_game_parses_with_and_without_scores() {
        let event: MjaiEvent =
            serde_json::from_str(r#"{"type":"end_game","scores":[35000,25000,20000,20000]}"#)
                .unwrap();
        assert_eq!(
            event,
            MjaiEvent::EndGame {
                scores: vec![35000, 25000, 20000, 20000],
            }
        );
        let event: MjaiEvent = serde_json::from_str(r#"{"type":"end_game"}"#).unwrap();
        assert_eq!(event, MjaiEvent::EndGame { scores: vec![] });
    }
}
