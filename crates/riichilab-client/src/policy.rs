use crate::agent::AgentKind;
use crate::protocol::{MjaiAction, MjaiPossibleAction};
use crate::state::ValidationState;

pub fn build_agent_response(
    agent_kind: AgentKind,
    state: &ValidationState,
    request_id: u64,
    possible_actions: &[MjaiPossibleAction],
) -> Option<MjaiAction> {
    match agent_kind {
        AgentKind::Tsumogiri => build_tsumogiri_response(state, request_id, possible_actions),
        AgentKind::Normal => build_normal_response(state, request_id, possible_actions),
    }
}

pub fn build_tsumogiri_response(
    state: &ValidationState,
    request_id: u64,
    possible_actions: &[MjaiPossibleAction],
) -> Option<MjaiAction> {
    tsumogiri_dahai_response(state, request_id, possible_actions)
        .or_else(|| none_response(request_id, possible_actions))
}

pub fn build_normal_response(
    state: &ValidationState,
    request_id: u64,
    possible_actions: &[MjaiPossibleAction],
) -> Option<MjaiAction> {
    if possible_actions
        .iter()
        .any(|a| matches!(a, MjaiPossibleAction::Hora))
    {
        return Some(MjaiAction::Hora {
            actor: state.actor_or_default(),
            target: None,
            pai: None,
            request_id: Some(request_id),
        });
    }

    if possible_actions
        .iter()
        .any(|a| matches!(a, MjaiPossibleAction::Ryukyoku))
    {
        return Some(MjaiAction::Ryukyoku {
            request_id: Some(request_id),
        });
    }

    if possible_actions
        .iter()
        .any(|a| matches!(a, MjaiPossibleAction::Reach))
    {
        return Some(MjaiAction::Reach {
            actor: state.actor_or_default(),
            request_id: Some(request_id),
        });
    }

    tsumogiri_dahai_response(state, request_id, possible_actions)
        .or_else(|| none_response(request_id, possible_actions))
}

pub fn build_validation_response(
    state: &ValidationState,
    request_id: u64,
    possible_actions: &[MjaiPossibleAction],
) -> Option<MjaiAction> {
    build_tsumogiri_response(state, request_id, possible_actions)
}

fn tsumogiri_dahai_response(
    state: &ValidationState,
    request_id: u64,
    possible_actions: &[MjaiPossibleAction],
) -> Option<MjaiAction> {
    let last_tsumo = state.last_tsumo()?;
    possible_actions
        .iter()
        .any(|a| matches!(a, MjaiPossibleAction::Dahai { pai, .. } if pai == last_tsumo))
        .then(|| MjaiAction::Dahai {
            actor: state.actor_or_default(),
            pai: last_tsumo.to_string(),
            tsumogiri: Some(true),
            request_id: Some(request_id),
        })
}

fn none_response(request_id: u64, possible_actions: &[MjaiPossibleAction]) -> Option<MjaiAction> {
    possible_actions
        .iter()
        .any(|a| matches!(a, MjaiPossibleAction::None))
        .then_some(MjaiAction::None {
            request_id: Some(request_id),
        })
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

    fn possible_dahai(pai: &str) -> MjaiPossibleAction {
        MjaiPossibleAction::Dahai {
            pai: pai.to_string(),
            tsumogiri: None,
        }
    }

    fn possible_pon() -> MjaiPossibleAction {
        MjaiPossibleAction::Pon {
            pai: "E".to_string(),
            consumed: vec!["E".to_string(), "E".to_string()],
        }
    }

    mod tsumogiri_policy {
        use super::*;

        #[test]
        fn discards_last_tsumo_when_possible() {
            let state = state_with_tsumo(0, "6p");
            let possible_actions = vec![possible_dahai("6p")];
            let response = build_tsumogiri_response(&state, 42, &possible_actions).unwrap();
            assert_eq!(
                serde_json::to_string(&response).unwrap(),
                r#"{"type":"dahai","actor":0,"pai":"6p","tsumogiri":true,"request_id":42}"#
            );
        }

        #[test]
        fn prefers_tsumogiri_over_hora() {
            let state = state_with_tsumo(1, "6p");
            let possible_actions = vec![
                MjaiPossibleAction::Hora,
                possible_dahai("6p"),
                MjaiPossibleAction::None,
            ];
            let response = build_tsumogiri_response(&state, 10, &possible_actions).unwrap();
            assert_eq!(
                response,
                MjaiAction::Dahai {
                    actor: 1,
                    pai: "6p".to_string(),
                    tsumogiri: Some(true),
                    request_id: Some(10),
                }
            );
        }

        #[test]
        fn prefers_tsumogiri_over_reach() {
            let state = state_with_tsumo(2, "5s");
            let possible_actions = vec![MjaiPossibleAction::Reach, possible_dahai("5s")];
            let response = build_tsumogiri_response(&state, 11, &possible_actions).unwrap();
            assert_eq!(
                response,
                MjaiAction::Dahai {
                    actor: 2,
                    pai: "5s".to_string(),
                    tsumogiri: Some(true),
                    request_id: Some(11),
                }
            );
        }

        #[test]
        fn returns_none_action_on_claim_opportunity_without_last_tsumo() {
            let state = ValidationState::new();
            let possible_actions = vec![possible_pon(), MjaiPossibleAction::None];
            let response = build_tsumogiri_response(&state, 43, &possible_actions).unwrap();
            assert_eq!(
                serde_json::to_string(&response).unwrap(),
                r#"{"type":"none","request_id":43}"#
            );
        }

        #[test]
        fn falls_back_when_none_is_not_possible() {
            let state = ValidationState::new();
            let possible_actions = vec![possible_pon()];
            assert_eq!(
                build_tsumogiri_response(&state, 43, &possible_actions),
                None
            );
        }

        #[test]
        fn always_echoes_request_id() {
            let state = state_with_tsumo(2, "5s");
            let possible_actions = vec![possible_dahai("5s")];
            for request_id in [0u64, 7, u64::MAX] {
                let response = build_tsumogiri_response(&state, request_id, &possible_actions);
                match response {
                    Some(MjaiAction::Dahai {
                        request_id: echoed, ..
                    }) => assert_eq!(echoed, Some(request_id)),
                    other => panic!("unexpected response: {other:?}"),
                }
            }
            let state = ValidationState::new();
            let possible_actions = vec![MjaiPossibleAction::None];
            for request_id in [0u64, 7, u64::MAX] {
                let response = build_tsumogiri_response(&state, request_id, &possible_actions);
                assert_eq!(
                    response,
                    Some(MjaiAction::None {
                        request_id: Some(request_id)
                    })
                );
            }
        }
    }

    mod normal_policy {
        use super::*;

        #[test]
        fn hora_is_selected_when_possible() {
            let state = state_with_tsumo(1, "6p");
            let possible_actions = vec![MjaiPossibleAction::Hora, MjaiPossibleAction::None];
            let response = build_normal_response(&state, 20, &possible_actions).unwrap();
            assert_eq!(
                response,
                MjaiAction::Hora {
                    actor: 1,
                    target: None,
                    pai: None,
                    request_id: Some(20),
                }
            );
        }

        #[test]
        fn ryukyoku_is_selected_when_possible() {
            let state = state_with_tsumo(0, "6p");
            let possible_actions = vec![MjaiPossibleAction::Ryukyoku, possible_dahai("6p")];
            let response = build_normal_response(&state, 21, &possible_actions).unwrap();
            assert_eq!(
                response,
                MjaiAction::Ryukyoku {
                    request_id: Some(21),
                }
            );
        }

        #[test]
        fn reach_is_selected_when_possible() {
            let state = state_with_tsumo(3, "6p");
            let possible_actions = vec![MjaiPossibleAction::Reach, possible_dahai("6p")];
            let response = build_normal_response(&state, 22, &possible_actions).unwrap();
            assert_eq!(
                response,
                MjaiAction::Reach {
                    actor: 3,
                    request_id: Some(22),
                }
            );
        }

        #[test]
        fn hora_takes_priority_over_reach_and_dahai() {
            let state = state_with_tsumo(0, "6p");
            let possible_actions = vec![
                possible_dahai("6p"),
                MjaiPossibleAction::Reach,
                MjaiPossibleAction::Hora,
            ];
            let response = build_normal_response(&state, 23, &possible_actions).unwrap();
            assert_eq!(
                response,
                MjaiAction::Hora {
                    actor: 0,
                    target: None,
                    pai: None,
                    request_id: Some(23),
                }
            );
        }

        #[test]
        fn reach_takes_priority_over_dahai() {
            let state = state_with_tsumo(0, "6p");
            let possible_actions = vec![possible_dahai("6p"), MjaiPossibleAction::Reach];
            let response = build_normal_response(&state, 24, &possible_actions).unwrap();
            assert_eq!(
                response,
                MjaiAction::Reach {
                    actor: 0,
                    request_id: Some(24),
                }
            );
        }

        #[test]
        fn discards_only_when_last_tsumo_matches_possible_dahai() {
            let state = state_with_tsumo(0, "6p");
            let possible_actions = vec![possible_dahai("6p")];
            let response = build_normal_response(&state, 25, &possible_actions).unwrap();
            assert_eq!(
                response,
                MjaiAction::Dahai {
                    actor: 0,
                    pai: "6p".to_string(),
                    tsumogiri: Some(true),
                    request_id: Some(25),
                }
            );

            let possible_actions = vec![possible_dahai("1m")];
            assert_eq!(build_normal_response(&state, 25, &possible_actions), None);
        }

        #[test]
        fn passes_with_none_action_on_claim_opportunity() {
            let state = ValidationState::new();
            let possible_actions = vec![possible_pon(), MjaiPossibleAction::None];
            let response = build_normal_response(&state, 26, &possible_actions).unwrap();
            assert_eq!(
                serde_json::to_string(&response).unwrap(),
                r#"{"type":"none","request_id":26}"#
            );
        }

        #[test]
        fn does_not_return_actions_missing_from_possible_actions() {
            let state = state_with_tsumo(0, "6p");
            let possible_actions = vec![possible_pon()];
            assert_eq!(build_normal_response(&state, 27, &possible_actions), None);
            assert_eq!(build_normal_response(&state, 27, &[]), None);
        }

        #[test]
        fn always_echoes_request_id() {
            let state = state_with_tsumo(0, "6p");
            let possible_actions = vec![
                MjaiPossibleAction::Hora,
                MjaiPossibleAction::Ryukyoku,
                MjaiPossibleAction::Reach,
                possible_dahai("6p"),
                MjaiPossibleAction::None,
            ];
            for request_id in [0u64, 7, u64::MAX] {
                let response = build_normal_response(&state, request_id, &possible_actions);
                match response {
                    Some(MjaiAction::Hora {
                        request_id: echoed, ..
                    }) => assert_eq!(echoed, Some(request_id)),
                    other => panic!("unexpected response: {other:?}"),
                }
            }
        }
    }

    mod agent_dispatch {
        use super::*;

        #[test]
        fn tsumogiri_kind_matches_tsumogiri_policy() {
            let state = state_with_tsumo(0, "6p");
            for possible_actions in [
                vec![
                    MjaiPossibleAction::Hora,
                    possible_dahai("6p"),
                    MjaiPossibleAction::None,
                ],
                vec![possible_pon(), MjaiPossibleAction::None],
                vec![possible_pon()],
            ] {
                assert_eq!(
                    build_agent_response(AgentKind::Tsumogiri, &state, 30, &possible_actions),
                    build_tsumogiri_response(&state, 30, &possible_actions)
                );
            }
        }

        #[test]
        fn normal_kind_matches_normal_policy() {
            let state = state_with_tsumo(0, "6p");
            for possible_actions in [
                vec![
                    MjaiPossibleAction::Hora,
                    possible_dahai("6p"),
                    MjaiPossibleAction::None,
                ],
                vec![MjaiPossibleAction::Reach, possible_dahai("6p")],
                vec![possible_pon(), MjaiPossibleAction::None],
                vec![possible_pon()],
            ] {
                assert_eq!(
                    build_agent_response(AgentKind::Normal, &state, 31, &possible_actions),
                    build_normal_response(&state, 31, &possible_actions)
                );
            }
        }
    }

    mod validation_response_compatibility {
        use super::*;

        #[test]
        fn discards_last_tsumo_when_possible() {
            let state = state_with_tsumo(0, "6p");
            let possible_actions = vec![possible_dahai("6p")];
            let response = build_validation_response(&state, 42, &possible_actions).unwrap();
            assert_eq!(
                serde_json::to_string(&response).unwrap(),
                r#"{"type":"dahai","actor":0,"pai":"6p","tsumogiri":true,"request_id":42}"#
            );
        }

        #[test]
        fn does_not_discard_when_last_tsumo_is_not_a_possible_dahai() {
            let state = state_with_tsumo(0, "6p");
            let possible_actions = vec![possible_dahai("1m")];
            assert_eq!(
                build_validation_response(&state, 42, &possible_actions),
                None
            );
        }

        #[test]
        fn returns_none_action_on_claim_opportunity() {
            let state = state_with_tsumo(0, "6p");
            let possible_actions = vec![possible_pon(), MjaiPossibleAction::None];
            let response = build_validation_response(&state, 43, &possible_actions).unwrap();
            assert_eq!(
                serde_json::to_string(&response).unwrap(),
                r#"{"type":"none","request_id":43}"#
            );
        }

        #[test]
        fn falls_back_when_claim_opportunity_has_no_none() {
            let state = ValidationState::new();
            let possible_actions = vec![possible_pon()];
            assert_eq!(
                build_validation_response(&state, 43, &possible_actions),
                None
            );
        }

        #[test]
        fn falls_back_without_last_tsumo_and_without_none() {
            let state = ValidationState::new();
            let possible_actions = vec![possible_dahai("1m")];
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
        fn prefers_tsumogiri_over_none() {
            let state = state_with_tsumo(1, "6p");
            let possible_actions = vec![possible_dahai("6p"), MjaiPossibleAction::None];
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
}
