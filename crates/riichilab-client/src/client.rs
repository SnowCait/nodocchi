use bot_core::{Agent, GameContext};
use futures_util::{SinkExt, StreamExt};
use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::http::HeaderValue;
use tokio_tungstenite::tungstenite::http::header::AUTHORIZATION;
use tokio_tungstenite::tungstenite::{Error as WsError, Message};
use tracing::{debug, error, info, warn};

use crate::config::ClientConfig;
use crate::convert::{legal_action_to_mjai_action, possible_actions_to_legal_actions};
use crate::observation::ObservationPayload;
use crate::protocol::{
    ActionAckStatus, MjaiAction, MjaiEvent, MjaiPossibleAction, parse_server_event,
};
use crate::state::ValidationState;

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
    let legal_actions = possible_actions_to_legal_actions(possible_actions);
    if legal_actions.is_empty() {
        warn!(request_id, "no convertible legal actions; falling back");
    }
    let chosen = agent.act(&GameContext::default(), &legal_actions);
    info!(
        request_id,
        legal_actions = legal_actions.len(),
        chosen = ?chosen,
        "request_action"
    );
    legal_action_to_mjai_action(&chosen, actor, request_id)
}

pub async fn run_validation_client<A, P>(
    config: ClientConfig,
    agent: &mut A,
    policy: P,
) -> Result<(), ClientError>
where
    A: Agent,
    P: Fn(&ValidationState, u64, &[MjaiPossibleAction]) -> Option<MjaiAction>,
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
                        let response = policy(&state, request_id, &possible_actions)
                            .inspect(|response| {
                                debug!(
                                    request_id,
                                    response = ?response,
                                    "policy selected response"
                                );
                            })
                            .unwrap_or_else(|| {
                                build_response_for_request(
                                    state.actor_or_default(),
                                    request_id,
                                    &possible_actions,
                                    &observation,
                                    agent,
                                )
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
    use bot_core::AlwaysLegalAgent;

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
        let mut agent = AlwaysLegalAgent;
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
        let mut agent = AlwaysLegalAgent;
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
        let mut agent = AlwaysLegalAgent;
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
        let mut agent = AlwaysLegalAgent;
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
        let mut agent = AlwaysLegalAgent;
        let observation = ObservationPayload::new("dummy-base64");
        let response = build_response_for_request(0, 9, &[], &observation, &mut agent);
        assert_eq!(
            response,
            MjaiAction::None {
                request_id: Some(9)
            }
        );
    }
}
