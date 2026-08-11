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

    #[serde(rename = "chi")]
    Chi {
        actor: u8,
        pai: String,
        consumed: Vec<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        request_id: Option<u64>,
    },

    #[serde(rename = "pon")]
    Pon {
        actor: u8,
        pai: String,
        consumed: Vec<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        request_id: Option<u64>,
    },

    #[serde(rename = "daiminkan")]
    Daiminkan {
        actor: u8,
        pai: String,
        consumed: Vec<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        request_id: Option<u64>,
    },

    #[serde(rename = "ankan")]
    Ankan {
        actor: u8,
        consumed: Vec<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        request_id: Option<u64>,
    },

    #[serde(rename = "kakan")]
    Kakan {
        actor: u8,
        pai: String,
        consumed: Vec<String>,
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

#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize)]
#[serde(tag = "type")]
pub enum MjaiPossibleAction {
    #[serde(rename = "dahai")]
    Dahai {
        pai: String,
        #[serde(default)]
        tsumogiri: Option<bool>,
    },

    #[serde(rename = "chi")]
    Chi {
        pai: String,
        #[serde(default)]
        consumed: Vec<String>,
    },

    #[serde(rename = "pon")]
    Pon {
        pai: String,
        #[serde(default)]
        consumed: Vec<String>,
    },

    #[serde(rename = "daiminkan")]
    Daiminkan {
        pai: String,
        #[serde(default)]
        consumed: Vec<String>,
    },

    #[serde(rename = "ankan")]
    Ankan {
        #[serde(default)]
        consumed: Vec<String>,
    },

    #[serde(rename = "kakan")]
    Kakan {
        pai: String,
        #[serde(default)]
        consumed: Vec<String>,
    },

    #[serde(rename = "reach")]
    Reach,

    #[serde(rename = "hora")]
    Hora,

    #[serde(rename = "ryukyoku")]
    Ryukyoku,

    #[serde(rename = "none")]
    None,
}

pub type TimeControl = serde_json::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct RequestTimeBudget {
    pub grace_ms: Option<u64>,
    pub bank_ms: Option<u64>,
    pub deadline_ms: Option<u64>,
}

pub fn request_time_budget(time: Option<&TimeControl>) -> RequestTimeBudget {
    let Some(time) = time else {
        return RequestTimeBudget::default();
    };
    let field = |key: &str| time.get(key).and_then(serde_json::Value::as_u64);
    RequestTimeBudget {
        grace_ms: field("grace_ms"),
        bank_ms: field("bank_ms"),
        deadline_ms: field("deadline_ms"),
    }
}

pub fn mjai_action_type(action: &MjaiAction) -> &'static str {
    match action {
        MjaiAction::Dahai { .. } => "dahai",
        MjaiAction::Reach { .. } => "reach",
        MjaiAction::Hora { .. } => "hora",
        MjaiAction::Chi { .. } => "chi",
        MjaiAction::Pon { .. } => "pon",
        MjaiAction::Daiminkan { .. } => "daiminkan",
        MjaiAction::Ankan { .. } => "ankan",
        MjaiAction::Kakan { .. } => "kakan",
        MjaiAction::Ryukyoku { .. } => "ryukyoku",
        MjaiAction::None { .. } => "none",
    }
}

#[derive(Debug, Clone, PartialEq, serde::Deserialize)]
#[serde(tag = "type")]
pub enum MjaiEvent {
    #[serde(rename = "start_game")]
    StartGame { id: u8 },

    #[serde(rename = "start_kyoku")]
    StartKyoku {
        #[serde(default)]
        bakaze: Option<String>,
        #[serde(default)]
        dora_marker: Option<String>,
        #[serde(default)]
        kyoku: Option<u8>,
        #[serde(default)]
        honba: Option<u8>,
        #[serde(default)]
        kyotaku: Option<u8>,
        #[serde(default)]
        oya: Option<u8>,
        #[serde(default)]
        tehais: Vec<Vec<String>>,
    },

    #[serde(rename = "tsumo")]
    Tsumo { actor: u8, pai: String },

    #[serde(rename = "dahai")]
    Dahai {
        actor: u8,
        pai: String,
        #[serde(default)]
        tsumogiri: Option<bool>,
    },

    #[serde(rename = "chi")]
    Chi {
        actor: u8,
        target: u8,
        pai: String,
        #[serde(default)]
        consumed: Vec<String>,
    },

    #[serde(rename = "pon")]
    Pon {
        actor: u8,
        target: u8,
        pai: String,
        #[serde(default)]
        consumed: Vec<String>,
    },

    #[serde(rename = "daiminkan")]
    Daiminkan {
        actor: u8,
        target: u8,
        pai: String,
        #[serde(default)]
        consumed: Vec<String>,
    },

    #[serde(rename = "ankan")]
    Ankan {
        actor: u8,
        #[serde(default)]
        consumed: Vec<String>,
    },

    #[serde(rename = "kakan")]
    Kakan {
        actor: u8,
        pai: String,
        #[serde(default)]
        consumed: Vec<String>,
    },

    #[serde(rename = "reach")]
    Reach { actor: u8 },

    #[serde(rename = "hora")]
    Hora {
        actor: u8,
        #[serde(default)]
        target: Option<u8>,
        #[serde(default)]
        pai: Option<String>,
    },

    #[serde(rename = "ryukyoku")]
    Ryukyoku {
        #[serde(default)]
        reason: Option<String>,
    },

    #[serde(rename = "end_kyoku")]
    EndKyoku {
        #[serde(flatten)]
        raw: serde_json::Value,
    },

    #[serde(rename = "request_action")]
    RequestAction {
        request_id: u64,
        #[serde(default)]
        time: Option<TimeControl>,
        possible_actions: Vec<MjaiPossibleAction>,
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
        #[serde(default)]
        reason: Option<String>,
        #[serde(default)]
        action: Option<serde_json::Value>,
        #[serde(default)]
        attempted: Option<serde_json::Value>,
        #[serde(default)]
        legal_types: Vec<String>,
    },

    #[serde(rename = "end_game")]
    EndGame {
        #[serde(default)]
        scores: Vec<i32>,
    },

    #[serde(rename = "validation_result")]
    ValidationResult {
        #[serde(alias = "success")]
        passed: bool,
        #[serde(default, alias = "message")]
        reason: Option<String>,
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

impl ActionAckStatus {
    pub fn is_chombo(self) -> bool {
        matches!(self, Self::Rejected | Self::Unparseable)
    }

    pub fn is_timing_issue(self) -> bool {
        matches!(self, Self::Stale | Self::Defaulted)
    }
}

pub fn parse_server_event(text: &str) -> Result<Option<MjaiEvent>, serde_json::Error> {
    let value: serde_json::Value = serde_json::from_str(text)?;
    let event_type = value.get("type").and_then(|v| v.as_str());

    match event_type {
        Some(
            "start_game" | "start_kyoku" | "tsumo" | "dahai" | "chi" | "pon" | "daiminkan"
            | "ankan" | "kakan" | "reach" | "hora" | "ryukyoku" | "end_kyoku" | "request_action"
            | "action_ack" | "end_game" | "validation_result",
        ) => serde_json::from_value(value).map(Some),
        Some(other) => {
            tracing::debug!(event_type = other, "ignoring unknown server event");
            Ok(None)
        }
        None => {
            tracing::debug!("ignoring JSON message without type");
            Ok(None)
        }
    }
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

    // RiichiEnv v0.4.8 の Action::to_mjai() は type / actor / pai / consumed のみを出力し、
    // Bot-to-Server action JSON に target を含めない。
    #[test]
    fn chi_serializes_without_target() {
        let action = MjaiAction::Chi {
            actor: 0,
            pai: "3m".to_string(),
            consumed: vec!["1m".to_string(), "2m".to_string()],
            request_id: Some(64),
        };
        let json = serde_json::to_value(&action).unwrap();
        assert_eq!(
            json,
            serde_json::json!({
                "type": "chi",
                "actor": 0,
                "pai": "3m",
                "consumed": ["1m", "2m"],
                "request_id": 64,
            })
        );
        assert!(json.get("target").is_none());
        let parsed: MjaiAction = serde_json::from_value(json).unwrap();
        assert_eq!(parsed, action);
    }

    #[test]
    fn pon_serializes_without_target() {
        let action = MjaiAction::Pon {
            actor: 0,
            pai: "E".to_string(),
            consumed: vec!["E".to_string(), "E".to_string()],
            request_id: Some(65),
        };
        let json = serde_json::to_value(&action).unwrap();
        assert_eq!(
            json,
            serde_json::json!({
                "type": "pon",
                "actor": 0,
                "pai": "E",
                "consumed": ["E", "E"],
                "request_id": 65,
            })
        );
        assert!(json.get("target").is_none());
        let parsed: MjaiAction = serde_json::from_value(json).unwrap();
        assert_eq!(parsed, action);
    }

    #[test]
    fn daiminkan_serializes_without_target() {
        let action = MjaiAction::Daiminkan {
            actor: 0,
            pai: "E".to_string(),
            consumed: vec!["E".to_string(), "E".to_string(), "E".to_string()],
            request_id: Some(66),
        };
        let json = serde_json::to_value(&action).unwrap();
        assert_eq!(
            json,
            serde_json::json!({
                "type": "daiminkan",
                "actor": 0,
                "pai": "E",
                "consumed": ["E", "E", "E"],
                "request_id": 66,
            })
        );
        assert!(json.get("target").is_none());
        let parsed: MjaiAction = serde_json::from_value(json).unwrap();
        assert_eq!(parsed, action);
    }

    #[test]
    fn claim_actions_omit_request_id_when_absent() {
        let action = MjaiAction::Pon {
            actor: 2,
            pai: "5m".to_string(),
            consumed: vec!["5m".to_string(), "5mr".to_string()],
            request_id: None,
        };
        let json = serde_json::to_value(&action).unwrap();
        assert_eq!(
            json,
            serde_json::json!({
                "type": "pon",
                "actor": 2,
                "pai": "5m",
                "consumed": ["5m", "5mr"],
            })
        );
    }

    #[test]
    fn ankan_serializes_without_pai() {
        let action = MjaiAction::Ankan {
            actor: 0,
            consumed: vec![
                "1s".to_string(),
                "1s".to_string(),
                "1s".to_string(),
                "1s".to_string(),
            ],
            request_id: Some(60),
        };
        let json = serde_json::to_string(&action).unwrap();
        assert_eq!(
            json,
            r#"{"type":"ankan","actor":0,"consumed":["1s","1s","1s","1s"],"request_id":60}"#
        );
        let parsed: MjaiAction = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, action);
    }

    #[test]
    fn kakan_serializes_with_pai() {
        let action = MjaiAction::Kakan {
            actor: 1,
            pai: "P".to_string(),
            consumed: vec!["P".to_string(), "P".to_string(), "P".to_string()],
            request_id: Some(62),
        };
        let json = serde_json::to_string(&action).unwrap();
        assert_eq!(
            json,
            r#"{"type":"kakan","actor":1,"pai":"P","consumed":["P","P","P"],"request_id":62}"#
        );
        let parsed: MjaiAction = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, action);
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
    fn request_action_parses_official_example_without_actor() {
        let json = r#"{
            "type": "request_action",
            "request_id": 42,
            "possible_actions": [
                {"type": "dahai", "pai": "1m"},
                {"type": "dahai", "pai": "3m"},
                {"type": "reach"},
                {"type": "hora"},
                {"type": "none"}
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
                    MjaiPossibleAction::Dahai {
                        pai: "1m".to_string(),
                        tsumogiri: None,
                    },
                    MjaiPossibleAction::Dahai {
                        pai: "3m".to_string(),
                        tsumogiri: None,
                    },
                    MjaiPossibleAction::Reach,
                    MjaiPossibleAction::Hora,
                    MjaiPossibleAction::None,
                ],
                observation: "dummy-base64".to_string(),
            }
        );
    }

    #[test]
    fn possible_action_parses_each_type_without_actor() {
        for (json, expected) in [
            (
                r#"{"type":"dahai","pai":"5mr"}"#,
                MjaiPossibleAction::Dahai {
                    pai: "5mr".to_string(),
                    tsumogiri: None,
                },
            ),
            (
                r#"{"type":"dahai","pai":"1m","tsumogiri":true}"#,
                MjaiPossibleAction::Dahai {
                    pai: "1m".to_string(),
                    tsumogiri: Some(true),
                },
            ),
            (r#"{"type":"reach"}"#, MjaiPossibleAction::Reach),
            (r#"{"type":"hora"}"#, MjaiPossibleAction::Hora),
            (r#"{"type":"ryukyoku"}"#, MjaiPossibleAction::Ryukyoku),
            (r#"{"type":"none"}"#, MjaiPossibleAction::None),
        ] {
            let action: MjaiPossibleAction = serde_json::from_str(json).unwrap();
            assert_eq!(action, expected, "json: {json}");
        }
    }

    #[test]
    fn possible_action_parses_each_claim_type() {
        for (json, expected) in [
            (
                r#"{"type":"chi","pai":"5m","consumed":["4m","6m"]}"#,
                MjaiPossibleAction::Chi {
                    pai: "5m".to_string(),
                    consumed: vec!["4m".to_string(), "6m".to_string()],
                },
            ),
            (
                r#"{"type":"pon","pai":"E","consumed":["E","E"]}"#,
                MjaiPossibleAction::Pon {
                    pai: "E".to_string(),
                    consumed: vec!["E".to_string(), "E".to_string()],
                },
            ),
            (
                r#"{"type":"daiminkan","pai":"9s","consumed":["9s","9s","9s"]}"#,
                MjaiPossibleAction::Daiminkan {
                    pai: "9s".to_string(),
                    consumed: vec!["9s".to_string(), "9s".to_string(), "9s".to_string()],
                },
            ),
            (
                r#"{"type":"ankan","consumed":["1s","1s","1s","1s"]}"#,
                MjaiPossibleAction::Ankan {
                    consumed: vec![
                        "1s".to_string(),
                        "1s".to_string(),
                        "1s".to_string(),
                        "1s".to_string(),
                    ],
                },
            ),
            (
                r#"{"type":"kakan","pai":"P","consumed":["P","P","P"]}"#,
                MjaiPossibleAction::Kakan {
                    pai: "P".to_string(),
                    consumed: vec!["P".to_string(), "P".to_string(), "P".to_string()],
                },
            ),
        ] {
            let action: MjaiPossibleAction = serde_json::from_str(json).unwrap();
            assert_eq!(action, expected, "json: {json}");
        }
    }

    #[test]
    fn request_action_parses_with_claim_possible_actions() {
        let json = r#"{
            "type": "request_action",
            "request_id": 44,
            "possible_actions": [
                {"type":"pon","pai":"E","consumed":["E","E"]},
                {"type":"none"}
            ],
            "observation": "dummy-base64"
        }"#;
        let event: MjaiEvent = serde_json::from_str(json).unwrap();
        assert_eq!(
            event,
            MjaiEvent::RequestAction {
                request_id: 44,
                time: None,
                possible_actions: vec![
                    MjaiPossibleAction::Pon {
                        pai: "E".to_string(),
                        consumed: vec!["E".to_string(), "E".to_string()],
                    },
                    MjaiPossibleAction::None,
                ],
                observation: "dummy-base64".to_string(),
            }
        );
    }

    #[test]
    fn possible_action_rejects_unsupported_type() {
        assert!(serde_json::from_str::<MjaiPossibleAction>(r#"{"type":"unknown"}"#).is_err());
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
    fn request_time_budget_extracts_all_fields() {
        let time = serde_json::json!({
            "grace_ms": 3000,
            "bank_ms": 10000,
            "deadline_ms": 5000
        });
        let budget = request_time_budget(Some(&time));
        assert_eq!(
            budget,
            RequestTimeBudget {
                grace_ms: Some(3000),
                bank_ms: Some(10000),
                deadline_ms: Some(5000),
            }
        );
    }

    #[test]
    fn request_time_budget_returns_none_for_missing_fields() {
        let time = serde_json::json!({ "grace_ms": 3000 });
        let budget = request_time_budget(Some(&time));
        assert_eq!(
            budget,
            RequestTimeBudget {
                grace_ms: Some(3000),
                bank_ms: None,
                deadline_ms: None,
            }
        );
    }

    #[test]
    fn request_time_budget_handles_none_time() {
        assert_eq!(request_time_budget(None), RequestTimeBudget::default());
    }

    #[test]
    fn request_time_budget_ignores_non_integer_fields() {
        let time = serde_json::json!({
            "grace_ms": "fast",
            "bank_ms": -1,
            "deadline_ms": 5000
        });
        let budget = request_time_budget(Some(&time));
        assert_eq!(
            budget,
            RequestTimeBudget {
                grace_ms: None,
                bank_ms: None,
                deadline_ms: Some(5000),
            }
        );
    }

    #[test]
    fn mjai_action_type_maps_each_variant() {
        assert_eq!(
            mjai_action_type(&MjaiAction::Dahai {
                actor: 0,
                pai: "1m".to_string(),
                tsumogiri: None,
                request_id: None,
            }),
            "dahai"
        );
        assert_eq!(
            mjai_action_type(&MjaiAction::Reach {
                actor: 0,
                request_id: None,
            }),
            "reach"
        );
        assert_eq!(
            mjai_action_type(&MjaiAction::Hora {
                actor: 0,
                target: None,
                pai: None,
                request_id: None,
            }),
            "hora"
        );
        assert_eq!(
            mjai_action_type(&MjaiAction::Chi {
                actor: 0,
                pai: "3m".to_string(),
                consumed: vec!["1m".to_string(), "2m".to_string()],
                request_id: None,
            }),
            "chi"
        );
        assert_eq!(
            mjai_action_type(&MjaiAction::Pon {
                actor: 0,
                pai: "E".to_string(),
                consumed: vec!["E".to_string(), "E".to_string()],
                request_id: None,
            }),
            "pon"
        );
        assert_eq!(
            mjai_action_type(&MjaiAction::Daiminkan {
                actor: 0,
                pai: "E".to_string(),
                consumed: vec!["E".to_string(), "E".to_string(), "E".to_string()],
                request_id: None,
            }),
            "daiminkan"
        );
        assert_eq!(
            mjai_action_type(&MjaiAction::Ankan {
                actor: 0,
                consumed: vec!["1s".to_string(); 4],
                request_id: None,
            }),
            "ankan"
        );
        assert_eq!(
            mjai_action_type(&MjaiAction::Kakan {
                actor: 0,
                pai: "P".to_string(),
                consumed: vec!["P".to_string(); 3],
                request_id: None,
            }),
            "kakan"
        );
        assert_eq!(
            mjai_action_type(&MjaiAction::Ryukyoku { request_id: None }),
            "ryukyoku"
        );
        assert_eq!(
            mjai_action_type(&MjaiAction::None { request_id: None }),
            "none"
        );
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
                reason: None,
                action: None,
                attempted: None,
                legal_types: vec![],
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
                reason: None,
                action: None,
                attempted: None,
                legal_types: vec![],
            }
        );
    }

    #[test]
    fn action_ack_defaulted_parses_substituted_action() {
        let json = r#"{
            "type": "action_ack",
            "request_id": 51,
            "status": "defaulted",
            "elapsed_ms": 5000,
            "bank_consumed_ms": 3000,
            "bank_ms": 0,
            "action": {"type": "none"},
            "message": "deadline exceeded"
        }"#;
        let event: MjaiEvent = serde_json::from_str(json).unwrap();
        match event {
            MjaiEvent::ActionAck {
                request_id,
                status,
                bank_ms,
                action,
                ..
            } => {
                assert_eq!(request_id, 51);
                assert_eq!(status, ActionAckStatus::Defaulted);
                assert_eq!(bank_ms, Some(0));
                assert_eq!(action.unwrap()["type"], "none");
            }
            other => panic!("unexpected event: {other:?}"),
        }
    }

    #[test]
    fn action_ack_rejected_parses_reason_attempted_legal_types() {
        let json = r#"{
            "type": "action_ack",
            "request_id": 63,
            "status": "rejected",
            "reason": "action not in possible_actions",
            "message": "illegal",
            "attempted": {"type": "reach", "actor": 0},
            "legal_types": ["dahai", "none"]
        }"#;
        let event: MjaiEvent = serde_json::from_str(json).unwrap();
        match event {
            MjaiEvent::ActionAck {
                request_id,
                status,
                reason,
                attempted,
                legal_types,
                ..
            } => {
                assert_eq!(request_id, 63);
                assert_eq!(status, ActionAckStatus::Rejected);
                assert_eq!(reason.as_deref(), Some("action not in possible_actions"));
                assert_eq!(attempted.unwrap()["type"], "reach");
                assert_eq!(legal_types, vec!["dahai".to_string(), "none".to_string()]);
            }
            other => panic!("unexpected event: {other:?}"),
        }
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
    fn action_ack_status_is_chombo() {
        assert!(ActionAckStatus::Rejected.is_chombo());
        assert!(ActionAckStatus::Unparseable.is_chombo());
        assert!(!ActionAckStatus::Accepted.is_chombo());
        assert!(!ActionAckStatus::Stale.is_chombo());
        assert!(!ActionAckStatus::Defaulted.is_chombo());
    }

    #[test]
    fn action_ack_status_is_timing_issue() {
        assert!(ActionAckStatus::Stale.is_timing_issue());
        assert!(ActionAckStatus::Defaulted.is_timing_issue());
        assert!(!ActionAckStatus::Accepted.is_timing_issue());
        assert!(!ActionAckStatus::Rejected.is_timing_issue());
        assert!(!ActionAckStatus::Unparseable.is_timing_issue());
    }

    #[test]
    fn validation_result_parses_official_examples() {
        for (json, expected) in [
            (
                r#"{"type":"validation_result","passed":true}"#,
                MjaiEvent::ValidationResult {
                    passed: true,
                    reason: None,
                },
            ),
            (
                r#"{"type":"validation_result","passed":false,"reason":"disconnected"}"#,
                MjaiEvent::ValidationResult {
                    passed: false,
                    reason: Some("disconnected".to_string()),
                },
            ),
            (
                r#"{"type":"validation_result","passed":false,"reason":"penalized (illegal action)"}"#,
                MjaiEvent::ValidationResult {
                    passed: false,
                    reason: Some("penalized (illegal action)".to_string()),
                },
            ),
        ] {
            let event: MjaiEvent = serde_json::from_str(json).unwrap();
            assert_eq!(event, expected, "json: {json}");
        }
    }

    #[test]
    fn validation_result_parses_legacy_field_names_as_aliases() {
        let json = r#"{"type":"validation_result","success":true,"message":"ok"}"#;
        let event: MjaiEvent = serde_json::from_str(json).unwrap();
        assert_eq!(
            event,
            MjaiEvent::ValidationResult {
                passed: true,
                reason: Some("ok".to_string()),
            }
        );
    }

    #[test]
    fn validation_result_ignores_unknown_fields() {
        let json = r#"{"type":"validation_result","passed":true,"details":{"games":1}}"#;
        let event: MjaiEvent = serde_json::from_str(json).unwrap();
        assert_eq!(
            event,
            MjaiEvent::ValidationResult {
                passed: true,
                reason: None,
            }
        );
    }

    #[test]
    fn dahai_event_parses() {
        let json = r#"{"actor":0,"pai":"6p","tsumogiri":false,"type":"dahai"}"#;
        let event: MjaiEvent = serde_json::from_str(json).unwrap();
        assert_eq!(
            event,
            MjaiEvent::Dahai {
                actor: 0,
                pai: "6p".to_string(),
                tsumogiri: Some(false),
            }
        );
    }

    #[test]
    fn tsumo_event_parses() {
        let json = r#"{"type":"tsumo","actor":0,"pai":"3m"}"#;
        let event: MjaiEvent = serde_json::from_str(json).unwrap();
        assert_eq!(
            event,
            MjaiEvent::Tsumo {
                actor: 0,
                pai: "3m".to_string(),
            }
        );
    }

    #[test]
    fn start_kyoku_event_parses() {
        let json = r#"{
            "type": "start_kyoku",
            "bakaze": "E",
            "dora_marker": "2s",
            "kyoku": 1,
            "honba": 0,
            "kyotaku": 0,
            "oya": 0,
            "scores": [25000, 25000, 25000, 25000],
            "tehais": [
                ["1m","2m","3m","4m","5m","6m","7m","8m","9m","1p","2p","3p","4p"],
                ["?","?","?","?","?","?","?","?","?","?","?","?","?"],
                ["?","?","?","?","?","?","?","?","?","?","?","?","?"],
                ["?","?","?","?","?","?","?","?","?","?","?","?","?"]
            ]
        }"#;
        let event: MjaiEvent = serde_json::from_str(json).unwrap();
        match event {
            MjaiEvent::StartKyoku {
                bakaze,
                dora_marker,
                kyoku,
                honba,
                kyotaku,
                oya,
                tehais,
            } => {
                assert_eq!(bakaze, Some("E".to_string()));
                assert_eq!(dora_marker, Some("2s".to_string()));
                assert_eq!(kyoku, Some(1));
                assert_eq!(honba, Some(0));
                assert_eq!(kyotaku, Some(0));
                assert_eq!(oya, Some(0));
                assert_eq!(tehais.len(), 4);
                assert_eq!(tehais[0].len(), 13);
            }
            other => panic!("unexpected event: {other:?}"),
        }
    }

    #[test]
    fn reach_event_parses() {
        let json = r#"{"type":"reach","actor":0}"#;
        let event: MjaiEvent = serde_json::from_str(json).unwrap();
        assert_eq!(event, MjaiEvent::Reach { actor: 0 });
    }

    #[test]
    fn hora_event_parses() {
        let json = r#"{"type":"hora","actor":0,"target":1,"pai":"5m"}"#;
        let event: MjaiEvent = serde_json::from_str(json).unwrap();
        assert_eq!(
            event,
            MjaiEvent::Hora {
                actor: 0,
                target: Some(1),
                pai: Some("5m".to_string()),
            }
        );
    }

    #[test]
    fn ryukyoku_event_parses() {
        let json = r#"{"type":"ryukyoku","reason":"fanpai"}"#;
        let event: MjaiEvent = serde_json::from_str(json).unwrap();
        assert_eq!(
            event,
            MjaiEvent::Ryukyoku {
                reason: Some("fanpai".to_string()),
            }
        );
    }

    #[test]
    fn meld_events_parse() {
        let json = r#"{"type":"chi","actor":1,"target":0,"pai":"5m","consumed":["4m","6m"]}"#;
        let event: MjaiEvent = serde_json::from_str(json).unwrap();
        assert_eq!(
            event,
            MjaiEvent::Chi {
                actor: 1,
                target: 0,
                pai: "5m".to_string(),
                consumed: vec!["4m".to_string(), "6m".to_string()],
            }
        );

        let json = r#"{"type":"pon","actor":2,"target":0,"pai":"E","consumed":["E","E"]}"#;
        let event: MjaiEvent = serde_json::from_str(json).unwrap();
        assert_eq!(
            event,
            MjaiEvent::Pon {
                actor: 2,
                target: 0,
                pai: "E".to_string(),
                consumed: vec!["E".to_string(), "E".to_string()],
            }
        );

        let json =
            r#"{"type":"daiminkan","actor":3,"target":1,"pai":"9s","consumed":["9s","9s","9s"]}"#;
        let event: MjaiEvent = serde_json::from_str(json).unwrap();
        assert_eq!(
            event,
            MjaiEvent::Daiminkan {
                actor: 3,
                target: 1,
                pai: "9s".to_string(),
                consumed: vec!["9s".to_string(), "9s".to_string(), "9s".to_string()],
            }
        );

        let json = r#"{"type":"ankan","actor":0,"consumed":["1s","1s","1s","1s"]}"#;
        let event: MjaiEvent = serde_json::from_str(json).unwrap();
        assert_eq!(
            event,
            MjaiEvent::Ankan {
                actor: 0,
                consumed: vec![
                    "1s".to_string(),
                    "1s".to_string(),
                    "1s".to_string(),
                    "1s".to_string(),
                ],
            }
        );

        let json = r#"{"type":"kakan","actor":1,"pai":"P","consumed":["P","P","P"]}"#;
        let event: MjaiEvent = serde_json::from_str(json).unwrap();
        assert_eq!(
            event,
            MjaiEvent::Kakan {
                actor: 1,
                pai: "P".to_string(),
                consumed: vec!["P".to_string(), "P".to_string(), "P".to_string()],
            }
        );
    }

    #[test]
    fn end_kyoku_event_parses_with_arbitrary_payload() {
        let json = r#"{"type":"end_kyoku","scores":[25000,25000,25000,25000],"future_field":{"nested":true}}"#;
        let event: MjaiEvent = serde_json::from_str(json).unwrap();
        match event {
            MjaiEvent::EndKyoku { raw } => {
                assert_eq!(raw["scores"][0], 25000);
                assert_eq!(raw["future_field"]["nested"], true);
            }
            other => panic!("unexpected event: {other:?}"),
        }
    }

    #[test]
    fn parse_server_event_returns_known_event() {
        let event =
            parse_server_event(r#"{"actor":0,"pai":"6p","tsumogiri":false,"type":"dahai"}"#)
                .unwrap();
        assert_eq!(
            event,
            Some(MjaiEvent::Dahai {
                actor: 0,
                pai: "6p".to_string(),
                tsumogiri: Some(false),
            })
        );
    }

    #[test]
    fn parse_server_event_ignores_unknown_event_type() {
        let event = parse_server_event(r#"{"type":"future_event","foo":1}"#).unwrap();
        assert_eq!(event, None);
    }

    #[test]
    fn parse_server_event_ignores_json_without_type() {
        let event = parse_server_event(r#"{"foo":1}"#).unwrap();
        assert_eq!(event, None);
    }

    #[test]
    fn parse_server_event_fails_on_broken_json() {
        assert!(parse_server_event("not-json").is_err());
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
