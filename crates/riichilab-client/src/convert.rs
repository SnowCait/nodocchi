use bot_core::LegalAction;
use bot_logic::{TileId, TileType};

use crate::protocol::{MjaiAction, MjaiPossibleAction};

pub fn possible_action_to_legal_action(action: &MjaiPossibleAction) -> Option<LegalAction> {
    match action {
        MjaiPossibleAction::Dahai { pai, .. } => {
            temporary_tile_id_from_mjai_pai(pai).map(|tile| LegalAction::Dahai { tile })
        }
        MjaiPossibleAction::Reach => Some(LegalAction::Reach),
        MjaiPossibleAction::Hora => Some(LegalAction::Hora),
        MjaiPossibleAction::Ryukyoku => Some(LegalAction::Ryukyoku),
        MjaiPossibleAction::None => Some(LegalAction::None),
        MjaiPossibleAction::Chi { .. }
        | MjaiPossibleAction::Pon { .. }
        | MjaiPossibleAction::Daiminkan { .. }
        | MjaiPossibleAction::Ankan { .. }
        | MjaiPossibleAction::Kakan { .. } => None,
    }
}

pub fn legal_action_to_mjai_action(action: &LegalAction, actor: u8, request_id: u64) -> MjaiAction {
    match action {
        LegalAction::Dahai { tile } => MjaiAction::Dahai {
            actor,
            pai: tile.to_mjai_string(),
            tsumogiri: None,
            request_id: Some(request_id),
        },
        LegalAction::Reach => MjaiAction::Reach {
            actor,
            request_id: Some(request_id),
        },
        LegalAction::Hora => MjaiAction::Hora {
            actor,
            target: None,
            pai: None,
            request_id: Some(request_id),
        },
        LegalAction::Ryukyoku => MjaiAction::Ryukyoku {
            request_id: Some(request_id),
        },
        LegalAction::None => MjaiAction::None {
            request_id: Some(request_id),
        },
    }
}

pub fn possible_actions_to_legal_actions(actions: &[MjaiPossibleAction]) -> Vec<LegalAction> {
    actions
        .iter()
        .filter_map(possible_action_to_legal_action)
        .collect()
}

// TileRegistry実装までの一時措置。mjai牌文字列からは牌種しか分からないため、
// 赤5は固定ID、黒5は非赤の先頭ID、それ以外はその牌種の先頭IDを仮割当する。
pub(crate) fn temporary_tile_id_from_mjai_pai(pai: &str) -> Option<TileId> {
    match pai {
        "5mr" => return TileId::new(16),
        "5pr" => return TileId::new(52),
        "5sr" => return TileId::new(88),
        _ => {}
    }
    let tile_type = TileType::from_mjai_type_str(pai).ok()?;
    let base = tile_type.raw() * 4;
    let offset = u8::from(matches!(base, 16 | 52 | 88));
    TileId::new(base + offset)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::MjaiEvent;
    use bot_core::{Agent, GameContext, NormalAgent};

    fn tile(value: u8) -> TileId {
        TileId::new(value).unwrap()
    }

    #[test]
    fn dahai_converts_with_temporary_tile_id() {
        for (pai, expected) in [
            ("5mr", 16),
            ("5pr", 52),
            ("5sr", 88),
            ("5m", 17),
            ("5p", 53),
            ("5s", 89),
            ("1m", 0),
            ("9s", 104),
            ("C", 132),
        ] {
            let action = MjaiPossibleAction::Dahai {
                pai: pai.to_string(),
                tsumogiri: None,
            };
            assert_eq!(
                possible_action_to_legal_action(&action),
                Some(LegalAction::Dahai {
                    tile: tile(expected)
                }),
                "pai: {pai}"
            );
        }
    }

    #[test]
    fn dahai_with_invalid_pai_converts_to_none() {
        let action = MjaiPossibleAction::Dahai {
            pai: "invalid".to_string(),
            tsumogiri: None,
        };
        assert_eq!(possible_action_to_legal_action(&action), None);
    }

    #[test]
    fn non_dahai_actions_convert_directly() {
        for (action, expected) in [
            (MjaiPossibleAction::Reach, LegalAction::Reach),
            (MjaiPossibleAction::Hora, LegalAction::Hora),
            (MjaiPossibleAction::Ryukyoku, LegalAction::Ryukyoku),
            (MjaiPossibleAction::None, LegalAction::None),
        ] {
            assert_eq!(possible_action_to_legal_action(&action), Some(expected));
        }
    }

    #[test]
    fn claim_actions_convert_to_no_legal_action() {
        for action in [
            MjaiPossibleAction::Chi {
                pai: "5m".to_string(),
                consumed: vec!["4m".to_string(), "6m".to_string()],
            },
            MjaiPossibleAction::Pon {
                pai: "E".to_string(),
                consumed: vec!["E".to_string(), "E".to_string()],
            },
            MjaiPossibleAction::Daiminkan {
                pai: "9s".to_string(),
                consumed: vec!["9s".to_string(), "9s".to_string(), "9s".to_string()],
            },
            MjaiPossibleAction::Ankan {
                consumed: vec![
                    "1s".to_string(),
                    "1s".to_string(),
                    "1s".to_string(),
                    "1s".to_string(),
                ],
            },
            MjaiPossibleAction::Kakan {
                pai: "P".to_string(),
                consumed: vec!["P".to_string(), "P".to_string(), "P".to_string()],
            },
        ] {
            assert_eq!(possible_action_to_legal_action(&action), None, "{action:?}");
        }
    }

    #[test]
    fn possible_actions_skip_unconvertible_ones() {
        let actions = vec![
            MjaiPossibleAction::None,
            MjaiPossibleAction::Dahai {
                pai: "invalid".to_string(),
                tsumogiri: None,
            },
            MjaiPossibleAction::Dahai {
                pai: "1m".to_string(),
                tsumogiri: None,
            },
        ];
        assert_eq!(
            possible_actions_to_legal_actions(&actions),
            vec![LegalAction::None, LegalAction::Dahai { tile: tile(0) }]
        );
    }

    #[test]
    fn legal_action_to_mjai_action_echoes_request_id() {
        for action in [
            LegalAction::Dahai { tile: tile(16) },
            LegalAction::Reach,
            LegalAction::Hora,
            LegalAction::Ryukyoku,
            LegalAction::None,
        ] {
            let mjai = legal_action_to_mjai_action(&action, 0, 42);
            let json = serde_json::to_value(&mjai).unwrap();
            assert_eq!(json["request_id"], 42, "action: {action:?}");
        }
    }

    #[test]
    fn legal_action_to_mjai_action_sets_actor() {
        assert_eq!(
            legal_action_to_mjai_action(&LegalAction::Dahai { tile: tile(16) }, 3, 1),
            MjaiAction::Dahai {
                actor: 3,
                pai: "5mr".to_string(),
                tsumogiri: None,
                request_id: Some(1),
            }
        );
        assert_eq!(
            legal_action_to_mjai_action(&LegalAction::Reach, 2, 1),
            MjaiAction::Reach {
                actor: 2,
                request_id: Some(1),
            }
        );
        assert_eq!(
            legal_action_to_mjai_action(&LegalAction::Hora, 1, 1),
            MjaiAction::Hora {
                actor: 1,
                target: None,
                pai: None,
                request_id: Some(1),
            }
        );
    }

    #[test]
    fn official_request_action_flows_to_hora_response() {
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
        let MjaiEvent::RequestAction {
            request_id,
            possible_actions,
            ..
        } = event
        else {
            panic!("expected request_action");
        };
        let legal_actions = possible_actions_to_legal_actions(&possible_actions);
        assert_eq!(
            legal_actions,
            vec![
                LegalAction::Dahai { tile: tile(0) },
                LegalAction::Dahai { tile: tile(8) },
                LegalAction::Reach,
                LegalAction::Hora,
                LegalAction::None,
            ]
        );
        let mut agent = NormalAgent;
        let chosen = agent.act(&GameContext::default(), &legal_actions);
        assert_eq!(chosen, LegalAction::Hora);
        let response = legal_action_to_mjai_action(&chosen, 0, request_id);
        assert_eq!(
            serde_json::to_string(&response).unwrap(),
            r#"{"type":"hora","actor":0,"request_id":42}"#
        );
    }

    #[test]
    fn claim_request_action_flows_to_none_response() {
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
        let MjaiEvent::RequestAction {
            request_id,
            possible_actions,
            ..
        } = event
        else {
            panic!("expected request_action");
        };
        let legal_actions = possible_actions_to_legal_actions(&possible_actions);
        assert_eq!(legal_actions, vec![LegalAction::None]);
        let mut agent = NormalAgent;
        let chosen = agent.act(&GameContext::default(), &legal_actions);
        assert_eq!(chosen, LegalAction::None);
        let response = legal_action_to_mjai_action(&chosen, 0, request_id);
        assert_eq!(
            serde_json::to_string(&response).unwrap(),
            r#"{"type":"none","request_id":44}"#
        );
    }

    #[test]
    fn request_action_flows_to_dahai_response() {
        let json = r#"{
            "type": "request_action",
            "request_id": 43,
            "possible_actions": [
                {"type": "none"},
                {"type": "dahai", "pai": "5mr"}
            ],
            "observation": "dummy-base64"
        }"#;
        let event: MjaiEvent = serde_json::from_str(json).unwrap();
        let MjaiEvent::RequestAction {
            request_id,
            possible_actions,
            ..
        } = event
        else {
            panic!("expected request_action");
        };
        let legal_actions = possible_actions_to_legal_actions(&possible_actions);
        assert_eq!(
            legal_actions,
            vec![LegalAction::None, LegalAction::Dahai { tile: tile(16) }]
        );
        let mut agent = NormalAgent;
        let chosen = agent.act(&GameContext::default(), &legal_actions);
        assert_eq!(chosen, LegalAction::Dahai { tile: tile(16) });
        let response = legal_action_to_mjai_action(&chosen, 0, request_id);
        assert_eq!(
            serde_json::to_string(&response).unwrap(),
            r#"{"type":"dahai","actor":0,"pai":"5mr","request_id":43}"#
        );
    }
}
