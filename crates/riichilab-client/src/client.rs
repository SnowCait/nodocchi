use bot_core::{Agent, GameContext};
use futures_util::{SinkExt, StreamExt};
use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::http::HeaderValue;
use tokio_tungstenite::tungstenite::http::header::AUTHORIZATION;
use tokio_tungstenite::tungstenite::{Error as WsError, Message};
use tracing::{debug, error, info, warn};

use crate::config::ClientConfig;
use crate::convert::{checked_legal_action_to_mjai_action, possible_actions_to_legal_actions};
use crate::observation::{ObservationPayload, game_context_from_decoded_observation};
use crate::protocol::{
    ActionAckStatus, MjaiAction, MjaiEvent, MjaiPossibleAction, parse_server_event,
};
use crate::state::ValidationState;
use crate::validation_policy::game_context_from_validation_state;

#[derive(Debug, thiserror::Error)]
pub enum ClientError {
    #[error("bot token is not a valid Authorization header value")]
    InvalidToken,
    #[error("websocket error: {0}")]
    WebSocket(#[from] WsError),
    #[error("failed to serialize response: {0}")]
    Serialize(#[from] serde_json::Error),
}

pub fn build_response_for_request<A: Agent>(
    actor: u8,
    request_id: u64,
    possible_actions: &[MjaiPossibleAction],
    observation: &ObservationPayload,
    agent: &mut A,
) -> MjaiAction {
    debug!(
        request_id,
        observation_len = observation.as_base64().len(),
        "building response"
    );
    let context = observation
        .decode_4p()
        .ok()
        .as_ref()
        .map(game_context_from_decoded_observation)
        .unwrap_or_default();
    build_response_for_request_with_context(actor, request_id, possible_actions, &context, agent)
        .unwrap_or(MjaiAction::None {
            request_id: Some(request_id),
        })
}

pub fn build_response_for_request_with_context<A: Agent>(
    actor: u8,
    request_id: u64,
    possible_actions: &[MjaiPossibleAction],
    context: &GameContext,
    agent: &mut A,
) -> Option<MjaiAction> {
    let legal_actions = possible_actions_to_legal_actions(possible_actions);
    if legal_actions.is_empty() {
        warn!(request_id, "no convertible legal actions; falling back");
    }
    let chosen = agent.act(context, &legal_actions);
    info!(
        request_id,
        legal_actions = legal_actions.len(),
        chosen = ?chosen,
        "request_action"
    );
    checked_legal_action_to_mjai_action(&chosen, actor, request_id, possible_actions, context)
}

pub(crate) fn context_for_request(
    observation: &ObservationPayload,
    state: &ValidationState,
    request_id: u64,
) -> GameContext {
    let decoded_observation = match observation.decode_4p() {
        Ok(decoded) => {
            debug!(
                request_id,
                player_id = decoded.player_id,
                drawn_tile = ?decoded.drawn_tile,
                "decoded observation"
            );
            Some(decoded)
        }
        Err(error) => {
            debug!(
                request_id,
                error = %error,
                "failed to decode observation; using validation state context"
            );
            None
        }
    };

    decoded_observation
        .as_ref()
        .map(game_context_from_decoded_observation)
        .unwrap_or_else(|| game_context_from_validation_state(state))
}

pub async fn run_validation_client<A, P>(
    config: ClientConfig,
    agent: &mut A,
    policy: P,
) -> Result<(), ClientError>
where
    A: Agent,
    P: Fn(&ValidationState, &GameContext, u64, &[MjaiPossibleAction]) -> Option<MjaiAction>,
{
    info!(endpoint = %config.endpoint, "connecting");
    let mut request = config.endpoint.as_str().into_client_request()?;
    let authorization = HeaderValue::from_str(&format!("Bearer {}", config.token))
        .map_err(|_| ClientError::InvalidToken)?;
    request.headers_mut().insert(AUTHORIZATION, authorization);

    let (mut ws_stream, _) = connect_async(request).await?;
    info!("connected");

    let mut state = ValidationState::new();

    while let Some(message) = ws_stream.next().await {
        let message = match message {
            Ok(message) => message,
            Err(e) => {
                error!(error = %e, "websocket error");
                return Err(e.into());
            }
        };
        match message {
            Message::Text(text) => {
                let event = match parse_server_event(text.as_str()) {
                    Ok(Some(event)) => event,
                    Ok(None) => continue,
                    Err(e) => {
                        warn!(error = %e, text = %text, "failed to parse event");
                        continue;
                    }
                };
                match event {
                    MjaiEvent::StartGame { id } => {
                        info!(actor = id, "start_game");
                        state.on_start_game(id);
                    }
                    MjaiEvent::StartKyoku {
                        kyoku, honba, oya, ..
                    } => {
                        info!(kyoku = ?kyoku, honba = ?honba, oya = ?oya, "start_kyoku");
                    }
                    MjaiEvent::Tsumo { actor, pai } => {
                        debug!(actor, pai = %pai, "tsumo");
                        state.on_tsumo(actor, pai);
                    }
                    MjaiEvent::Dahai {
                        actor,
                        pai,
                        tsumogiri,
                    } => {
                        debug!(actor, pai = %pai, tsumogiri = ?tsumogiri, "dahai");
                        state.on_dahai(actor);
                    }
                    MjaiEvent::Chi {
                        actor,
                        target,
                        pai,
                        consumed,
                    }
                    | MjaiEvent::Pon {
                        actor,
                        target,
                        pai,
                        consumed,
                    }
                    | MjaiEvent::Daiminkan {
                        actor,
                        target,
                        pai,
                        consumed,
                    } => {
                        debug!(actor, target, pai = %pai, consumed = ?consumed, "meld");
                    }
                    MjaiEvent::Ankan { actor, consumed } => {
                        debug!(actor, consumed = ?consumed, "ankan");
                    }
                    MjaiEvent::Kakan {
                        actor,
                        pai,
                        consumed,
                    } => {
                        debug!(actor, pai = %pai, consumed = ?consumed, "kakan");
                    }
                    MjaiEvent::Reach { actor } => {
                        debug!(actor, "reach");
                    }
                    MjaiEvent::Hora { actor, target, pai } => {
                        info!(actor, target = ?target, pai = ?pai, "hora");
                    }
                    MjaiEvent::Ryukyoku { reason } => {
                        info!(reason = ?reason, "ryukyoku");
                    }
                    MjaiEvent::EndKyoku { .. } => {
                        info!("end_kyoku");
                    }
                    MjaiEvent::RequestAction {
                        request_id,
                        possible_actions,
                        observation,
                        ..
                    } => {
                        if state.seat_id().is_none() {
                            warn!("actor not set before request_action; falling back to 0");
                        }
                        let observation = ObservationPayload::new(observation);
                        let context = context_for_request(&observation, &state, request_id);
                        let response = policy(&state, &context, request_id, &possible_actions)
                            .inspect(|response| {
                                debug!(
                                    request_id,
                                    response = ?response,
                                    "policy selected response"
                                );
                            })
                            .unwrap_or_else(|| {
                                build_response_for_request_with_context(
                                    state.actor_or_default(),
                                    request_id,
                                    &possible_actions,
                                    &context,
                                    agent,
                                )
                                .unwrap_or(MjaiAction::None {
                                    request_id: Some(request_id),
                                })
                            });
                        let json = serde_json::to_string(&response)?;
                        debug!(response = %json, "sending response");
                        ws_stream.send(Message::Text(json.into())).await?;
                    }
                    MjaiEvent::ActionAck {
                        request_id,
                        status,
                        message,
                        ..
                    } => {
                        if status == ActionAckStatus::Accepted {
                            info!(request_id, status = ?status, "action_ack");
                        } else {
                            warn!(request_id, status = ?status, message = ?message, "action_ack");
                        }
                    }
                    MjaiEvent::EndGame { scores } => {
                        info!(scores = ?scores, "end_game");
                    }
                    MjaiEvent::ValidationResult { passed, reason } => {
                        if passed {
                            info!(reason = ?reason, "validation_result: passed");
                        } else {
                            error!(reason = ?reason, "validation_result: failed");
                        }
                        break;
                    }
                }
            }
            Message::Close(frame) => {
                info!(frame = ?frame, "connection closed by server");
                break;
            }
            other => {
                debug!(message = ?other, "ignoring non-text message");
            }
        }
    }

    info!("client finished");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::observation::fixture_base64;
    use bot_core::{NormalAgent, ShantenAgent, TsumogiriAgent};
    use bot_logic::TileId;

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

    #[test]
    fn context_for_request_prefers_decoded_observation() {
        let observation = ObservationPayload::new(fixture_base64(0, Some(59), vec![]));
        let mut state = ValidationState::new();
        state.on_start_game(0);
        state.on_tsumo(0, "1m".to_string());
        let context = context_for_request(&observation, &state, 1);
        assert_eq!(context.drawn_tile(), TileId::new(56));
    }

    #[test]
    fn context_for_request_has_hand_tiles_from_observation() {
        let observation = ObservationPayload::new(fixture_base64(0, Some(59), vec![0, 16, 104]));
        let state = ValidationState::new();
        let context = context_for_request(&observation, &state, 1);
        assert_eq!(context.drawn_tile(), TileId::new(56));
        assert_eq!(
            context.hand_tiles(),
            &[
                TileId::new(0).unwrap(),
                TileId::new(16).unwrap(),
                TileId::new(104).unwrap(),
            ]
        );
    }

    #[test]
    fn context_for_request_falls_back_to_state_when_decode_fails() {
        let observation = ObservationPayload::new("not-valid-base64!!");
        let mut state = ValidationState::new();
        state.on_start_game(0);
        state.on_tsumo(0, "6p".to_string());
        let context = context_for_request(&observation, &state, 2);
        assert_eq!(context.drawn_tile(), TileId::new(56));
        assert!(context.hand_tiles().is_empty());
    }

    #[test]
    fn context_for_request_without_any_source_is_default() {
        let observation = ObservationPayload::new("not-valid-base64!!");
        let state = ValidationState::new();
        let context = context_for_request(&observation, &state, 3);
        assert_eq!(context, GameContext::default());
    }

    #[test]
    fn context_for_request_uses_empty_decoded_observation_over_state() {
        let observation = ObservationPayload::new(fixture_base64(0, None, vec![]));
        let mut state = ValidationState::new();
        state.on_start_game(0);
        state.on_tsumo(0, "6p".to_string());
        let context = context_for_request(&observation, &state, 4);
        assert_eq!(context.drawn_tile(), None);
    }

    #[test]
    fn context_for_request_keeps_hand_tiles_without_drawn_tile() {
        let observation = ObservationPayload::new(fixture_base64(0, None, vec![0, 104]));
        let state = ValidationState::new();
        let context = context_for_request(&observation, &state, 5);
        assert_eq!(context.drawn_tile(), None);
        assert_eq!(
            context.hand_tiles(),
            &[TileId::new(0).unwrap(), TileId::new(104).unwrap()]
        );
    }

    #[test]
    fn prefers_hora_response() {
        let possible_actions = vec![
            MjaiPossibleAction::Dahai {
                pai: "1m".to_string(),
                tsumogiri: None,
            },
            MjaiPossibleAction::Hora,
            MjaiPossibleAction::None,
        ];
        let mut agent = NormalAgent;
        let observation = ObservationPayload::new("dummy-base64");
        let response =
            build_response_for_request(1, 42, &possible_actions, &observation, &mut agent);
        assert_eq!(
            response,
            MjaiAction::Hora {
                actor: 1,
                target: None,
                pai: None,
                request_id: Some(42),
            }
        );
    }

    #[test]
    fn dahai_only_returns_dahai_response() {
        let possible_actions = vec![MjaiPossibleAction::Dahai {
            pai: "5mr".to_string(),
            tsumogiri: None,
        }];
        let mut agent = NormalAgent;
        let observation = ObservationPayload::new("dummy-base64");
        let response =
            build_response_for_request(0, 43, &possible_actions, &observation, &mut agent);
        assert_eq!(
            serde_json::to_string(&response).unwrap(),
            r#"{"type":"dahai","actor":0,"pai":"5mr","request_id":43}"#
        );
    }

    #[test]
    fn echoes_request_id() {
        let possible_actions = vec![MjaiPossibleAction::None];
        let mut agent = NormalAgent;
        let observation = ObservationPayload::new("dummy-base64");
        let response =
            build_response_for_request(0, 7, &possible_actions, &observation, &mut agent);
        assert_eq!(
            response,
            MjaiAction::None {
                request_id: Some(7)
            }
        );
    }

    #[test]
    fn sets_actor_from_argument() {
        let possible_actions = vec![MjaiPossibleAction::Reach];
        let mut agent = NormalAgent;
        let observation = ObservationPayload::new("dummy-base64");
        let response =
            build_response_for_request(3, 1, &possible_actions, &observation, &mut agent);
        assert_eq!(
            response,
            MjaiAction::Reach {
                actor: 3,
                request_id: Some(1),
            }
        );
    }

    #[test]
    fn empty_possible_actions_fall_back_to_none() {
        let mut agent = NormalAgent;
        let observation = ObservationPayload::new("dummy-base64");
        let response = build_response_for_request(0, 9, &[], &observation, &mut agent);
        assert_eq!(
            response,
            MjaiAction::None {
                request_id: Some(9)
            }
        );
    }

    #[test]
    fn uses_observation_context_for_tsumogiri() {
        let observation = ObservationPayload::new(fixture_base64(0, Some(59), vec![]));
        let possible_actions = vec![possible_dahai("1m"), possible_dahai("6p")];
        let mut agent = TsumogiriAgent;
        let response =
            build_response_for_request(0, 90, &possible_actions, &observation, &mut agent);
        assert_eq!(
            response,
            MjaiAction::Dahai {
                actor: 0,
                pai: "6p".to_string(),
                tsumogiri: Some(true),
                request_id: Some(90),
            }
        );
    }

    #[test]
    fn builds_dahai_with_hand_tiles_from_observation() {
        let observation = ObservationPayload::new(fixture_base64(0, Some(59), vec![0, 16, 104]));
        let possible_actions = vec![possible_dahai("1m"), possible_dahai("6p")];
        let mut agent = ShantenAgent;
        let response =
            build_response_for_request(0, 91, &possible_actions, &observation, &mut agent);
        assert!(matches!(response, MjaiAction::Dahai { .. }));
    }

    #[test]
    fn with_context_uses_passed_context() {
        let context = GameContext::with_drawn_tile(TileId::new(56).unwrap());
        let possible_actions = vec![possible_dahai("1m"), possible_dahai("6p")];
        let mut agent = TsumogiriAgent;
        let response =
            build_response_for_request_with_context(1, 92, &possible_actions, &context, &mut agent);
        assert_eq!(
            response,
            Some(MjaiAction::Dahai {
                actor: 1,
                pai: "6p".to_string(),
                tsumogiri: Some(true),
                request_id: Some(92),
            })
        );
    }

    #[test]
    fn with_context_returns_none_when_chosen_action_is_not_possible() {
        let possible_actions = vec![possible_pon()];
        let mut agent = TsumogiriAgent;
        let response = build_response_for_request_with_context(
            0,
            93,
            &possible_actions,
            &GameContext::default(),
            &mut agent,
        );
        assert_eq!(response, None);
    }
}
