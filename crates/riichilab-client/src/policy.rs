use crate::protocol::{MjaiAction, MjaiPossibleAction};
use crate::state::ValidationState;

pub fn build_validation_response(
    state: &ValidationState,
    request_id: u64,
    possible_actions: &[MjaiPossibleAction],
) -> Option<MjaiAction> {
    if let Some(last_tsumo) = state.last_tsumo()
        && possible_actions
            .iter()
            .any(|a| matches!(a, MjaiPossibleAction::Dahai { pai, .. } if pai == last_tsumo))
    {
        return Some(MjaiAction::Dahai {
            actor: state.actor_or_default(),
            pai: last_tsumo.to_string(),
            tsumogiri: Some(true),
            request_id: Some(request_id),
        });
    }

    if possible_actions
        .iter()
        .any(|a| matches!(a, MjaiPossibleAction::None))
    {
        return Some(MjaiAction::None {
            request_id: Some(request_id),
        });
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state_with_tsumo(seat_id: u8, pai: &str) -> ValidationState {
        let mut state = ValidationState::new();
        state.on_start_game(seat_id);
        state.on_tsumo(seat_id, pai.to_string());
        state
    }

    #[test]
    fn discards_last_tsumo_when_possible() {
        let state = state_with_tsumo(0, "6p");
        let possible_actions = vec![MjaiPossibleAction::Dahai {
            pai: "6p".to_string(),
            tsumogiri: None,
        }];
        let response = build_validation_response(&state, 42, &possible_actions).unwrap();
        assert_eq!(
            serde_json::to_string(&response).unwrap(),
            r#"{"type":"dahai","actor":0,"pai":"6p","tsumogiri":true,"request_id":42}"#
        );
    }

    #[test]
    fn does_not_discard_when_last_tsumo_is_not_a_possible_dahai() {
        let state = state_with_tsumo(0, "6p");
        let possible_actions = vec![MjaiPossibleAction::Dahai {
            pai: "1m".to_string(),
            tsumogiri: None,
        }];
        assert_eq!(
            build_validation_response(&state, 42, &possible_actions),
            None
        );
    }

    #[test]
    fn returns_none_action_on_claim_opportunity() {
        let state = state_with_tsumo(0, "6p");
        let possible_actions = vec![
            MjaiPossibleAction::Pon {
                pai: "E".to_string(),
                consumed: vec!["E".to_string(), "E".to_string()],
            },
            MjaiPossibleAction::None,
        ];
        let response = build_validation_response(&state, 43, &possible_actions).unwrap();
        assert_eq!(
            serde_json::to_string(&response).unwrap(),
            r#"{"type":"none","request_id":43}"#
        );
    }

    #[test]
    fn falls_back_when_claim_opportunity_has_no_none() {
        let state = ValidationState::new();
        let possible_actions = vec![MjaiPossibleAction::Pon {
            pai: "E".to_string(),
            consumed: vec!["E".to_string(), "E".to_string()],
        }];
        assert_eq!(
            build_validation_response(&state, 43, &possible_actions),
            None
        );
    }

    #[test]
    fn falls_back_without_last_tsumo_and_without_none() {
        let state = ValidationState::new();
        let possible_actions = vec![MjaiPossibleAction::Dahai {
            pai: "1m".to_string(),
            tsumogiri: None,
        }];
        assert_eq!(
            build_validation_response(&state, 1, &possible_actions),
            None
        );
    }

    #[test]
    fn returns_none_action_even_when_seat_id_is_unset() {
        let state = ValidationState::new();
        let possible_actions = vec![MjaiPossibleAction::None];
        let response = build_validation_response(&state, 5, &possible_actions).unwrap();
        assert_eq!(
            response,
            MjaiAction::None {
                request_id: Some(5)
            }
        );
    }

    #[test]
    fn always_echoes_request_id() {
        let state = state_with_tsumo(2, "5s");
        let possible_actions = vec![MjaiPossibleAction::Dahai {
            pai: "5s".to_string(),
            tsumogiri: None,
        }];
        for request_id in [0u64, 7, u64::MAX] {
            let response = build_validation_response(&state, request_id, &possible_actions);
            match response {
                Some(MjaiAction::Dahai {
                    request_id: echoed, ..
                }) => assert_eq!(echoed, Some(request_id)),
                other => panic!("unexpected response: {other:?}"),
            }
        }
        let possible_actions = vec![MjaiPossibleAction::None];
        let state = ValidationState::new();
        for request_id in [0u64, 7, u64::MAX] {
            let response = build_validation_response(&state, request_id, &possible_actions);
            assert_eq!(
                response,
                Some(MjaiAction::None {
                    request_id: Some(request_id)
                })
            );
        }
    }

    #[test]
    fn prefers_tsumogiri_over_none() {
        let state = state_with_tsumo(1, "6p");
        let possible_actions = vec![
            MjaiPossibleAction::Dahai {
                pai: "6p".to_string(),
                tsumogiri: None,
            },
            MjaiPossibleAction::None,
        ];
        let response = build_validation_response(&state, 9, &possible_actions).unwrap();
        assert_eq!(
            response,
            MjaiAction::Dahai {
                actor: 1,
                pai: "6p".to_string(),
                tsumogiri: Some(true),
                request_id: Some(9),
            }
        );
    }

    #[test]
    fn empty_possible_actions_falls_back() {
        let state = state_with_tsumo(0, "6p");
        assert_eq!(build_validation_response(&state, 1, &[]), None);
    }
}
