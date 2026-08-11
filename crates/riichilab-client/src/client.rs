use std::time::Instant;

use bot_core::{Agent, GameContext};
use futures_util::{SinkExt, StreamExt};
use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::http::HeaderValue;
use tokio_tungstenite::tungstenite::http::header::AUTHORIZATION;
use tokio_tungstenite::tungstenite::{Error as WsError, Message};
use tracing::{debug, error, info, warn};

use crate::config::ClientConfig;
use crate::convert::{
    checked_legal_action_to_mjai_action, fallback_mjai_action_from_possible_actions,
    possible_actions_to_legal_actions,
};
use crate::observation::{ObservationPayload, game_context_from_decoded_observation};
use crate::protocol::{
    ActionAckStatus, MjaiAction, MjaiEvent, MjaiPossibleAction, mjai_action_type,
    parse_server_event, request_time_budget,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClientExitCondition {
    EndGame,
    ValidationResult,
}

pub(crate) fn should_finish_after_event(
    exit_condition: ClientExitCondition,
    event: &MjaiEvent,
) -> bool {
    matches!(
        (exit_condition, event),
        (ClientExitCondition::EndGame, MjaiEvent::EndGame { .. })
            | (
                ClientExitCondition::ValidationResult,
                MjaiEvent::ValidationResult { .. }
            )
    )
}

pub fn build_response_for_request<A: Agent>(
    actor: u8,
    request_id: u64,
    possible_actions: &[MjaiPossibleAction],
    observation: &ObservationPayload,
    agent: &mut A,
) -> Option<MjaiAction> {
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
}

// response 構築の流れ:
//   1. possible_actions を LegalAction に変換
//   2. Agent が LegalAction を選ぶ
//   3. checked_legal_action_to_mjai_action() で response 化を試す
//   4. 失敗したら fallback_mjai_action_from_possible_actions() を試す
//   5. それでも response が作れない場合は None を返す（呼び出し側で返信しない）
//
// possible_actions に無い none を fallback として送らないため、fallback も必ず
// possible_actions に基づいて選ぶ。合法 response が作れない場合は None を返し、
// chombo risk のある response を送るより server default に任せる。
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
    // Agent の選択は agent decision の DEBUG で追跡でき、実際に送信した action は
    // `action sent` の INFO で1件だけ記録する。ここは選択過程の DEBUG に留める。
    debug!(
        request_id,
        legal_actions = legal_actions.len(),
        chosen = ?chosen,
        "request_action"
    );

    if let Some(response) =
        checked_legal_action_to_mjai_action(&chosen, actor, request_id, possible_actions, context)
    {
        return Some(response);
    }

    warn!(
        request_id,
        chosen = ?chosen,
        possible_actions = possible_actions.len(),
        "checked conversion failed; selecting protocol-safe fallback"
    );

    match fallback_mjai_action_from_possible_actions(actor, request_id, possible_actions, context) {
        Some(fallback) => {
            warn!(
                request_id,
                chosen = ?chosen,
                fallback_type = mjai_action_type(&fallback),
                fallback_reason = "checked conversion failed",
                possible_actions = possible_actions.len(),
                "sending fallback response"
            );
            Some(fallback)
        }
        None => {
            error!(
                request_id,
                chosen = ?chosen,
                possible_actions = possible_actions.len(),
                "no legal fallback response; not replying and relying on server default"
            );
            None
        }
    }
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

/// 送信 action から `action sent` INFO ログ用のフィールドを抽出する pure helper。
///
/// `action_type` は `mjai_action_type` と一致し、Dahai のときだけ `tile` / `tsumogiri` を持つ。
/// `actor` は Ryukyoku / None を除く全 action に存在する。`request_id` は送信する `MjaiAction`
/// 自身の値で、受信した RequestAction の id とは区別する。`tile` は `action` を借用するため
/// clone を伴わない。tracing への依存なしにテストできる。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SentActionLogFields<'a> {
    pub request_id: Option<u64>,
    pub actor: Option<u8>,
    pub action_type: &'static str,
    pub tile: Option<&'a str>,
    pub tsumogiri: Option<bool>,
}

pub(crate) fn sent_action_log_fields(action: &MjaiAction) -> SentActionLogFields<'_> {
    let action_type = mjai_action_type(action);
    match action {
        MjaiAction::Dahai {
            actor,
            pai,
            tsumogiri,
            request_id,
        } => SentActionLogFields {
            request_id: *request_id,
            actor: Some(*actor),
            action_type,
            tile: Some(pai.as_str()),
            tsumogiri: *tsumogiri,
        },
        MjaiAction::Reach { actor, request_id } => SentActionLogFields {
            request_id: *request_id,
            actor: Some(*actor),
            action_type,
            tile: None,
            tsumogiri: None,
        },
        MjaiAction::Hora {
            actor, request_id, ..
        }
        | MjaiAction::Chi {
            actor, request_id, ..
        }
        | MjaiAction::Pon {
            actor, request_id, ..
        }
        | MjaiAction::Daiminkan {
            actor, request_id, ..
        }
        | MjaiAction::Ankan {
            actor, request_id, ..
        }
        | MjaiAction::Kakan {
            actor, request_id, ..
        } => SentActionLogFields {
            request_id: *request_id,
            actor: Some(*actor),
            action_type,
            tile: None,
            tsumogiri: None,
        },
        MjaiAction::Ryukyoku { request_id } | MjaiAction::None { request_id } => {
            SentActionLogFields {
                request_id: *request_id,
                actor: None,
                action_type,
                tile: None,
                tsumogiri: None,
            }
        }
    }
}

pub async fn run_riichilab_client<A, P>(
    config: ClientConfig,
    agent: &mut A,
    policy: P,
    exit_condition: ClientExitCondition,
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
                let finish = should_finish_after_event(exit_condition, &event);
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
                        time,
                        possible_actions,
                        observation,
                    } => {
                        let budget = request_time_budget(time.as_ref());
                        debug!(
                            request_id,
                            possible_actions = possible_actions.len(),
                            grace_ms = ?budget.grace_ms,
                            bank_ms = ?budget.bank_ms,
                            deadline_ms = ?budget.deadline_ms,
                            "request_action received"
                        );
                        if state.seat_id().is_none() {
                            warn!("actor not set before request_action; falling back to 0");
                        }
                        let request_start = Instant::now();
                        let observation = ObservationPayload::new(observation);
                        let context = context_for_request(&observation, &state, request_id);
                        let context_ms = request_start.elapsed().as_millis() as u64;

                        let policy_start = Instant::now();
                        let response = policy(&state, &context, request_id, &possible_actions)
                            .inspect(|response| {
                                debug!(
                                    request_id,
                                    response = ?response,
                                    "policy selected response"
                                );
                            })
                            .or_else(|| {
                                build_response_for_request_with_context(
                                    state.actor_or_default(),
                                    request_id,
                                    &possible_actions,
                                    &context,
                                    agent,
                                )
                            });
                        let policy_ms = policy_start.elapsed().as_millis() as u64;

                        // 合法 response が構築できない場合は、possible_actions に無い none を
                        // 無条件送信して chombo を招くより、返信せず server default に任せる。
                        // timeout 単体は chombo ではなく server default 適用になる。
                        let Some(response) = response else {
                            error!(
                                request_id,
                                possible_actions = possible_actions.len(),
                                policy_ms,
                                "no legal response constructed; not replying to rely on server default"
                            );
                            continue;
                        };
                        let response_type = mjai_action_type(&response);

                        let serialize_start = Instant::now();
                        let json = serde_json::to_string(&response)?;
                        let serialize_ms = serialize_start.elapsed().as_millis() as u64;

                        debug!(
                            request_id,
                            possible_actions = possible_actions.len(),
                            response_type,
                            response = %json,
                            "sending response"
                        );

                        let send_start = Instant::now();
                        ws_stream.send(Message::Text(json.into())).await?;
                        let send_ms = send_start.elapsed().as_millis() as u64;

                        // INFO 無効時は診断値を一切構築しないよう、有効時だけ helper を呼ぶ。
                        // `enabled!` と `info!` は target 未指定で同じ module path を使う。
                        if tracing::enabled!(tracing::Level::INFO) {
                            let sent = sent_action_log_fields(&response);
                            info!(
                                request_id = ?sent.request_id,
                                request_action_id = request_id,
                                actor = ?sent.actor,
                                action_type = sent.action_type,
                                tile = ?sent.tile,
                                tsumogiri = ?sent.tsumogiri,
                                "action sent"
                            );
                        }

                        let total_ms = request_start.elapsed().as_millis() as u64;

                        let over_deadline = budget
                            .deadline_ms
                            .is_some_and(|deadline| total_ms >= deadline);
                        let over_grace = budget.grace_ms.is_some_and(|grace| total_ms >= grace);
                        let over_fallback = budget.grace_ms.is_none()
                            && budget.deadline_ms.is_none()
                            && total_ms >= 1000;

                        if over_deadline {
                            error!(
                                request_id,
                                response_type,
                                context_ms,
                                policy_ms,
                                serialize_ms,
                                send_ms,
                                total_ms,
                                grace_ms = ?budget.grace_ms,
                                deadline_ms = ?budget.deadline_ms,
                                "request_action response exceeded deadline"
                            );
                        } else if over_grace || over_fallback {
                            warn!(
                                request_id,
                                response_type,
                                context_ms,
                                policy_ms,
                                serialize_ms,
                                send_ms,
                                total_ms,
                                grace_ms = ?budget.grace_ms,
                                deadline_ms = ?budget.deadline_ms,
                                "slow request_action response"
                            );
                        } else {
                            debug!(
                                request_id,
                                response_type,
                                context_ms,
                                policy_ms,
                                serialize_ms,
                                send_ms,
                                total_ms,
                                grace_ms = ?budget.grace_ms,
                                deadline_ms = ?budget.deadline_ms,
                                "request_action response timing"
                            );
                        }
                    }
                    MjaiEvent::ActionAck {
                        request_id,
                        status,
                        elapsed_ms,
                        bank_consumed_ms,
                        bank_ms,
                        message,
                        reason,
                        action,
                        attempted,
                        legal_types,
                    } => match status {
                        ActionAckStatus::Accepted => {
                            info!(
                                request_id,
                                status = ?status,
                                elapsed_ms = ?elapsed_ms,
                                bank_consumed_ms = ?bank_consumed_ms,
                                bank_ms = ?bank_ms,
                                "action_ack accepted"
                            );
                        }
                        ActionAckStatus::Defaulted => {
                            warn!(
                                request_id,
                                status = ?status,
                                elapsed_ms = ?elapsed_ms,
                                bank_consumed_ms = ?bank_consumed_ms,
                                bank_ms = ?bank_ms,
                                action = ?action,
                                message = ?message,
                                "action_ack defaulted; server substituted default action after timeout"
                            );
                        }
                        ActionAckStatus::Stale => {
                            warn!(
                                request_id,
                                status = ?status,
                                elapsed_ms = ?elapsed_ms,
                                bank_ms = ?bank_ms,
                                message = ?message,
                                "action_ack stale; reply was late or for an old request_id"
                            );
                        }
                        ActionAckStatus::Rejected => {
                            error!(
                                request_id,
                                status = ?status,
                                reason = ?reason,
                                message = ?message,
                                attempted = ?attempted,
                                legal_types = ?legal_types,
                                "action_ack rejected; chombo risk"
                            );
                        }
                        ActionAckStatus::Unparseable => {
                            error!(
                                request_id,
                                status = ?status,
                                reason = ?reason,
                                message = ?message,
                                "action_ack unparseable; chombo risk"
                            );
                        }
                    },
                    MjaiEvent::EndGame { scores } => {
                        info!(scores = ?scores, "end_game");
                    }
                    MjaiEvent::ValidationResult { passed, reason } => {
                        if passed {
                            info!(reason = ?reason, "validation_result: passed");
                        } else {
                            error!(reason = ?reason, "validation_result: failed");
                        }
                    }
                }
                if finish {
                    info!(exit_condition = ?exit_condition, "exit condition met; finishing client");
                    if let Err(e) = ws_stream.send(Message::Close(None)).await {
                        debug!(error = %e, "failed to send close frame");
                    }
                    break;
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
    use bot_core::{LegalAction, NormalAgent, ShantenAgent, TsumogiriAgent};
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
    fn sent_action_log_fields_extracts_dahai() {
        let action = MjaiAction::Dahai {
            actor: 1,
            pai: "4p".to_string(),
            tsumogiri: Some(false),
            request_id: Some(425),
        };
        let fields = sent_action_log_fields(&action);
        assert_eq!(fields.request_id, Some(425));
        assert_eq!(fields.actor, Some(1));
        assert_eq!(fields.action_type, "dahai");
        assert_eq!(fields.tile, Some("4p"));
        assert_eq!(fields.tsumogiri, Some(false));
    }

    #[test]
    fn sent_action_log_fields_uses_action_request_id_not_received_id() {
        // 受信 RequestAction の id が 100 でも、送信 MjaiAction 自身の request_id(200)を返す。
        // `action sent` の主 request_id が送信 action 由来であることを保証する。
        let received_request_id: u64 = 100;
        let action = MjaiAction::Dahai {
            actor: 1,
            pai: "4p".to_string(),
            tsumogiri: Some(false),
            request_id: Some(200),
        };
        let fields = sent_action_log_fields(&action);
        assert_eq!(fields.request_id, Some(200));
        assert_ne!(fields.request_id, Some(received_request_id));
    }

    #[test]
    fn sent_action_log_fields_extracts_reach() {
        let action = MjaiAction::Reach {
            actor: 2,
            request_id: Some(7),
        };
        let fields = sent_action_log_fields(&action);
        assert_eq!(fields.request_id, Some(7));
        assert_eq!(fields.actor, Some(2));
        assert_eq!(fields.action_type, "reach");
        assert_eq!(fields.tile, None);
        assert_eq!(fields.tsumogiri, None);
    }

    #[test]
    fn sent_action_log_fields_extracts_hora() {
        let action = MjaiAction::Hora {
            actor: 3,
            target: Some(1),
            pai: Some("3m".to_string()),
            request_id: Some(42),
        };
        let fields = sent_action_log_fields(&action);
        assert_eq!(fields.request_id, Some(42));
        assert_eq!(fields.actor, Some(3));
        assert_eq!(fields.action_type, "hora");
        assert_eq!(fields.tile, None);
        assert_eq!(fields.tsumogiri, None);
    }

    #[test]
    fn sent_action_log_fields_extracts_ryukyoku() {
        let action = MjaiAction::Ryukyoku {
            request_id: Some(9),
        };
        let fields = sent_action_log_fields(&action);
        assert_eq!(fields.request_id, Some(9));
        assert_eq!(fields.actor, None);
        assert_eq!(fields.action_type, "ryukyoku");
        assert_eq!(fields.tile, None);
        assert_eq!(fields.tsumogiri, None);
    }

    #[test]
    fn sent_action_log_fields_extracts_none() {
        let action = MjaiAction::None {
            request_id: Some(11),
        };
        let fields = sent_action_log_fields(&action);
        assert_eq!(fields.request_id, Some(11));
        assert_eq!(fields.actor, None);
        assert_eq!(fields.action_type, "none");
        assert_eq!(fields.tile, None);
        assert_eq!(fields.tsumogiri, None);
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
            Some(MjaiAction::Hora {
                actor: 1,
                target: None,
                pai: None,
                request_id: Some(42),
            })
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
            build_response_for_request(0, 43, &possible_actions, &observation, &mut agent).unwrap();
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
            Some(MjaiAction::None {
                request_id: Some(7)
            })
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
            Some(MjaiAction::Reach {
                actor: 3,
                request_id: Some(1),
            })
        );
    }

    #[test]
    fn empty_possible_actions_do_not_construct_a_response() {
        // possible_actions が空だと合法 response は構築できない。
        // possible_actions に無い none を無条件送信せず、None を返して返信しない。
        let mut agent = NormalAgent;
        let observation = ObservationPayload::new("dummy-base64");
        let response = build_response_for_request(0, 9, &[], &observation, &mut agent);
        assert_eq!(response, None);
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
            Some(MjaiAction::Dahai {
                actor: 0,
                pai: "6p".to_string(),
                tsumogiri: Some(true),
                request_id: Some(90),
            })
        );
    }

    #[test]
    fn builds_dahai_with_hand_tiles_from_observation() {
        let observation = ObservationPayload::new(fixture_base64(0, Some(59), vec![0, 16, 104]));
        let possible_actions = vec![possible_dahai("1m"), possible_dahai("6p")];
        let mut agent = ShantenAgent;
        let response =
            build_response_for_request(0, 91, &possible_actions, &observation, &mut agent);
        assert!(matches!(response, Some(MjaiAction::Dahai { .. })));
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
    fn ranked_finishes_after_end_game() {
        let event = MjaiEvent::EndGame { scores: vec![] };
        assert!(should_finish_after_event(
            ClientExitCondition::EndGame,
            &event
        ));
    }

    #[test]
    fn validate_does_not_finish_after_end_game() {
        let event = MjaiEvent::EndGame {
            scores: vec![35000, 25000, 20000, 20000],
        };
        assert!(!should_finish_after_event(
            ClientExitCondition::ValidationResult,
            &event
        ));
    }

    #[test]
    fn validate_finishes_after_validation_result() {
        let event = MjaiEvent::ValidationResult {
            passed: true,
            reason: None,
        };
        assert!(should_finish_after_event(
            ClientExitCondition::ValidationResult,
            &event
        ));
    }

    #[test]
    fn ranked_does_not_finish_after_validation_result() {
        let event = MjaiEvent::ValidationResult {
            passed: false,
            reason: Some("disconnected".to_string()),
        };
        assert!(!should_finish_after_event(
            ClientExitCondition::EndGame,
            &event
        ));
    }

    #[test]
    fn other_events_never_finish() {
        let event = MjaiEvent::EndKyoku {
            raw: serde_json::json!({"type": "end_kyoku"}),
        };
        assert!(!should_finish_after_event(
            ClientExitCondition::EndGame,
            &event
        ));
        assert!(!should_finish_after_event(
            ClientExitCondition::ValidationResult,
            &event
        ));
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

    // fallback safety 検証用に、意図的に副露・カンを選ぶテスト専用 Agent。
    // 実運用 Agent の選択方針は変えない。
    struct ClaimingAgent;

    impl Agent for ClaimingAgent {
        fn act(&mut self, _ctx: &GameContext, legal_actions: &[LegalAction]) -> LegalAction {
            legal_actions
                .iter()
                .find(|a| {
                    matches!(
                        a,
                        LegalAction::Chi { .. }
                            | LegalAction::Pon { .. }
                            | LegalAction::Daiminkan { .. }
                            | LegalAction::Ankan { .. }
                            | LegalAction::Kakan { .. }
                    )
                })
                .cloned()
                .unwrap_or(LegalAction::None)
        }
    }

    // possible_actions に無い Pon を選ぶ、fallback 経路検証用のテスト専用 Agent。
    struct MismatchedPonAgent;

    impl Agent for MismatchedPonAgent {
        fn act(&mut self, _ctx: &GameContext, _legal_actions: &[LegalAction]) -> LegalAction {
            LegalAction::Pon {
                tile: TileId::new(104).unwrap(),
                consumed: vec![TileId::new(104).unwrap(), TileId::new(104).unwrap()],
            }
        }
    }

    fn possible_kakan() -> MjaiPossibleAction {
        MjaiPossibleAction::Kakan {
            pai: "P".to_string(),
            consumed: vec!["P".to_string(), "P".to_string(), "P".to_string()],
        }
    }

    fn possible_chi() -> MjaiPossibleAction {
        MjaiPossibleAction::Chi {
            pai: "3m".to_string(),
            consumed: vec!["1m".to_string(), "2m".to_string()],
        }
    }

    fn possible_daiminkan() -> MjaiPossibleAction {
        MjaiPossibleAction::Daiminkan {
            pai: "E".to_string(),
            consumed: vec!["E".to_string(), "E".to_string(), "E".to_string()],
        }
    }

    #[test]
    fn chi_choice_builds_chi_via_checked_conversion() {
        let possible_actions = vec![possible_chi(), MjaiPossibleAction::None];
        let mut agent = ClaimingAgent;
        let response = build_response_for_request_with_context(
            0,
            94,
            &possible_actions,
            &GameContext::default(),
            &mut agent,
        );
        assert_eq!(
            response,
            Some(MjaiAction::Chi {
                actor: 0,
                pai: "3m".to_string(),
                consumed: vec!["1m".to_string(), "2m".to_string()],
                request_id: Some(94),
            })
        );
        assert_eq!(
            serde_json::to_string(&response.unwrap()).unwrap(),
            r#"{"type":"chi","actor":0,"pai":"3m","consumed":["1m","2m"],"request_id":94}"#
        );
    }

    #[test]
    fn pon_choice_builds_pon_via_checked_conversion() {
        let possible_actions = vec![possible_pon(), MjaiPossibleAction::None];
        let mut agent = ClaimingAgent;
        let response = build_response_for_request_with_context(
            1,
            95,
            &possible_actions,
            &GameContext::default(),
            &mut agent,
        );
        assert_eq!(
            response,
            Some(MjaiAction::Pon {
                actor: 1,
                pai: "E".to_string(),
                consumed: vec!["E".to_string(), "E".to_string()],
                request_id: Some(95),
            })
        );
        assert_eq!(
            serde_json::to_string(&response.unwrap()).unwrap(),
            r#"{"type":"pon","actor":1,"pai":"E","consumed":["E","E"],"request_id":95}"#
        );
    }

    #[test]
    fn daiminkan_choice_builds_daiminkan_via_checked_conversion() {
        let possible_actions = vec![possible_daiminkan(), MjaiPossibleAction::None];
        let mut agent = ClaimingAgent;
        let response = build_response_for_request_with_context(
            2,
            96,
            &possible_actions,
            &GameContext::default(),
            &mut agent,
        );
        assert_eq!(
            response,
            Some(MjaiAction::Daiminkan {
                actor: 2,
                pai: "E".to_string(),
                consumed: vec!["E".to_string(), "E".to_string(), "E".to_string()],
                request_id: Some(96),
            })
        );
        assert_eq!(
            serde_json::to_string(&response.unwrap()).unwrap(),
            r#"{"type":"daiminkan","actor":2,"pai":"E","consumed":["E","E","E"],"request_id":96}"#
        );
    }

    #[test]
    fn mismatched_pon_choice_falls_back_to_none_when_none_is_possible() {
        // possible_actions に一致する Pon が無ければ checked conversion は失敗し、
        // None が possible なら none fallback になる。
        let possible_actions = vec![possible_pon(), MjaiPossibleAction::None];
        let mut agent = MismatchedPonAgent;
        let response = build_response_for_request_with_context(
            0,
            97,
            &possible_actions,
            &GameContext::default(),
            &mut agent,
        );
        assert_eq!(
            response,
            Some(MjaiAction::None {
                request_id: Some(97),
            })
        );
    }

    #[test]
    fn mismatched_pon_choice_does_not_return_none_without_possible_none() {
        // possible_actions に None が無い場合、無条件 none は返さない。
        let possible_actions = vec![possible_pon()];
        let mut agent = MismatchedPonAgent;
        let response = build_response_for_request_with_context(
            0,
            98,
            &possible_actions,
            &GameContext::default(),
            &mut agent,
        );
        assert_eq!(response, None);
    }

    #[test]
    fn shanten_agent_still_passes_on_claims() {
        // 送信できるようになっても、ShantenAgent の鳴き判断は変えない。
        for claim in [possible_chi(), possible_pon(), possible_daiminkan()] {
            let possible_actions = vec![claim.clone(), MjaiPossibleAction::None];
            let mut agent = ShantenAgent;
            let response = build_response_for_request_with_context(
                0,
                99,
                &possible_actions,
                &GameContext::default(),
                &mut agent,
            );
            assert_eq!(
                response,
                Some(MjaiAction::None {
                    request_id: Some(99),
                }),
                "claim: {claim:?}"
            );
        }
    }

    #[test]
    fn shanten_agent_still_prefers_hora_over_claims() {
        let possible_actions = vec![
            MjaiPossibleAction::Hora,
            possible_pon(),
            MjaiPossibleAction::None,
        ];
        let mut agent = ShantenAgent;
        let response = build_response_for_request_with_context(
            0,
            100,
            &possible_actions,
            &GameContext::default(),
            &mut agent,
        );
        assert_eq!(
            response,
            Some(MjaiAction::Hora {
                actor: 0,
                target: None,
                pai: None,
                request_id: Some(100),
            })
        );
    }

    #[test]
    fn kakan_choice_builds_kakan_via_checked_conversion() {
        // 対応する possible_actions があれば checked conversion で kakan response になる。
        let possible_actions = vec![possible_kakan()];
        let mut agent = ClaimingAgent;
        let response = build_response_for_request_with_context(
            2,
            96,
            &possible_actions,
            &GameContext::default(),
            &mut agent,
        );
        assert_eq!(
            response,
            Some(MjaiAction::Kakan {
                actor: 2,
                pai: "P".to_string(),
                consumed: vec!["P".to_string(), "P".to_string(), "P".to_string()],
                request_id: Some(96),
            })
        );
    }

    #[test]
    fn mismatched_pon_choice_falls_back_to_dahai_when_none_absent() {
        // None が無く Dahai が possible なら、fallback は dahai (ツモ切り一致で tsumogiri: true)。
        let possible_actions = vec![possible_pon(), possible_dahai("6p")];
        let context = GameContext::with_drawn_tile(TileId::new(56).unwrap());
        let mut agent = MismatchedPonAgent;
        let response =
            build_response_for_request_with_context(1, 97, &possible_actions, &context, &mut agent);
        assert_eq!(
            response,
            Some(MjaiAction::Dahai {
                actor: 1,
                pai: "6p".to_string(),
                tsumogiri: Some(true),
                request_id: Some(97),
            })
        );
    }
}
