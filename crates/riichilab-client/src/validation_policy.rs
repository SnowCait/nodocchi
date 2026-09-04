use bot_core::{Agent, GameContext, MenzenAgent, NormalAgent, ShantenAgent, TsumogiriAgent};

use crate::convert::{
    checked_legal_action_to_mjai_action, possible_actions_to_legal_actions,
    temporary_tile_id_from_mjai_pai,
};
use crate::protocol::{MjaiAction, MjaiPossibleAction};
use crate::state::ValidationState;

pub type ResponsePolicy =
    fn(&ValidationState, &GameContext, u64, &[MjaiPossibleAction]) -> Option<MjaiAction>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AgentKind {
    Tsumogiri,
    Shanten,
    Menzen,
    #[default]
    Normal,
}

impl AgentKind {
    pub fn from_env() -> Result<Self, AgentKindError> {
        match std::env::var("MAHJONG_AGENT") {
            Ok(value) => value.parse(),
            Err(std::env::VarError::NotPresent) => Ok(Self::default()),
            Err(std::env::VarError::NotUnicode(_)) => Err(AgentKindError::NotUnicode),
        }
    }

    pub fn response_policy(self) -> ResponsePolicy {
        match self {
            Self::Tsumogiri => build_tsumogiri_response_with_context,
            Self::Shanten => build_shanten_response_with_context,
            Self::Menzen => build_menzen_response_with_context,
            Self::Normal => build_normal_response_with_context,
        }
    }
}

impl std::str::FromStr for AgentKind {
    type Err = AgentKindError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim().to_ascii_lowercase().as_str() {
            "" | "normal" => Ok(Self::Normal),
            "tsumogiri" | "tsumo-giri" => Ok(Self::Tsumogiri),
            "shanten" => Ok(Self::Shanten),
            "menzen" => Ok(Self::Menzen),
            other => Err(AgentKindError::Unknown(other.to_string())),
        }
    }
}

impl std::fmt::Display for AgentKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Tsumogiri => write!(f, "tsumogiri"),
            Self::Shanten => write!(f, "shanten"),
            Self::Menzen => write!(f, "menzen"),
            Self::Normal => write!(f, "normal"),
        }
    }
}

#[derive(Debug, thiserror::Error, Clone, PartialEq, Eq)]
pub enum AgentKindError {
    #[error("MAHJONG_AGENT is not valid unicode")]
    NotUnicode,

    #[error("unknown MAHJONG_AGENT: {0}")]
    Unknown(String),
}

pub fn build_tsumogiri_response(
    state: &ValidationState,
    request_id: u64,
    possible_actions: &[MjaiPossibleAction],
) -> Option<MjaiAction> {
    let context = game_context_from_validation_state(state);
    build_tsumogiri_response_with_context(state, &context, request_id, possible_actions)
}

pub(crate) fn build_tsumogiri_response_with_context(
    state: &ValidationState,
    context: &GameContext,
    request_id: u64,
    possible_actions: &[MjaiPossibleAction],
) -> Option<MjaiAction> {
    let legal_actions = possible_actions_to_legal_actions(possible_actions);

    let mut agent = TsumogiriAgent;
    let chosen = agent.act(context, &legal_actions);

    checked_legal_action_to_mjai_action(
        &chosen,
        state.actor_or_default(),
        request_id,
        possible_actions,
        context,
    )
}

pub fn build_normal_response(
    state: &ValidationState,
    request_id: u64,
    possible_actions: &[MjaiPossibleAction],
) -> Option<MjaiAction> {
    let context = game_context_from_validation_state(state);
    build_normal_response_with_context(state, &context, request_id, possible_actions)
}

pub(crate) fn build_normal_response_with_context(
    state: &ValidationState,
    context: &GameContext,
    request_id: u64,
    possible_actions: &[MjaiPossibleAction],
) -> Option<MjaiAction> {
    let legal_actions = possible_actions_to_legal_actions(possible_actions);

    let mut agent = NormalAgent;
    let chosen = agent.act(context, &legal_actions);

    checked_legal_action_to_mjai_action(
        &chosen,
        state.actor_or_default(),
        request_id,
        possible_actions,
        context,
    )
}

pub fn build_shanten_response(
    state: &ValidationState,
    request_id: u64,
    possible_actions: &[MjaiPossibleAction],
) -> Option<MjaiAction> {
    let context = game_context_from_validation_state(state);
    build_shanten_response_with_context(state, &context, request_id, possible_actions)
}

pub(crate) fn build_shanten_response_with_context(
    state: &ValidationState,
    context: &GameContext,
    request_id: u64,
    possible_actions: &[MjaiPossibleAction],
) -> Option<MjaiAction> {
    let legal_actions = possible_actions_to_legal_actions(possible_actions);

    let mut agent = ShantenAgent;
    let chosen = agent.act(context, &legal_actions);

    checked_legal_action_to_mjai_action(
        &chosen,
        state.actor_or_default(),
        request_id,
        possible_actions,
        context,
    )
}

pub fn build_menzen_response(
    state: &ValidationState,
    request_id: u64,
    possible_actions: &[MjaiPossibleAction],
) -> Option<MjaiAction> {
    let context = game_context_from_validation_state(state);
    build_menzen_response_with_context(state, &context, request_id, possible_actions)
}

pub(crate) fn build_menzen_response_with_context(
    state: &ValidationState,
    context: &GameContext,
    request_id: u64,
    possible_actions: &[MjaiPossibleAction],
) -> Option<MjaiAction> {
    let legal_actions = possible_actions_to_legal_actions(possible_actions);

    let mut agent = MenzenAgent::default();
    let chosen = agent.act(context, &legal_actions);

    checked_legal_action_to_mjai_action(
        &chosen,
        state.actor_or_default(),
        request_id,
        possible_actions,
        context,
    )
}

pub(crate) fn game_context_from_validation_state(state: &ValidationState) -> GameContext {
    state
        .last_tsumo()
        .and_then(temporary_tile_id_from_mjai_pai)
        .map(GameContext::with_drawn_tile)
        .unwrap_or_default()
        .with_reaction_source_player(state.reaction_source_player())
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

    mod agent_kind {
        use super::*;

        #[test]
        fn default_is_normal() {
            assert_eq!(AgentKind::default(), AgentKind::Normal);
        }

        #[test]
        fn parses_normal() {
            assert_eq!("normal".parse::<AgentKind>().unwrap(), AgentKind::Normal);
        }

        #[test]
        fn parses_empty_string_as_normal() {
            assert_eq!("".parse::<AgentKind>().unwrap(), AgentKind::Normal);
        }

        #[test]
        fn parses_tsumogiri() {
            assert_eq!(
                "tsumogiri".parse::<AgentKind>().unwrap(),
                AgentKind::Tsumogiri
            );
        }

        #[test]
        fn parses_tsumogiri_with_hyphen() {
            assert_eq!(
                "tsumo-giri".parse::<AgentKind>().unwrap(),
                AgentKind::Tsumogiri
            );
        }

        #[test]
        fn parses_shanten() {
            assert_eq!("shanten".parse::<AgentKind>().unwrap(), AgentKind::Shanten);
        }

        #[test]
        fn parses_menzen() {
            assert_eq!("menzen".parse::<AgentKind>().unwrap(), AgentKind::Menzen);
        }

        #[test]
        fn parses_mixed_case() {
            assert_eq!("Normal".parse::<AgentKind>().unwrap(), AgentKind::Normal);
            assert_eq!(
                "TsumoGiri".parse::<AgentKind>().unwrap(),
                AgentKind::Tsumogiri
            );
            assert_eq!("Shanten".parse::<AgentKind>().unwrap(), AgentKind::Shanten);
            assert_eq!("Menzen".parse::<AgentKind>().unwrap(), AgentKind::Menzen);
        }

        #[test]
        fn parses_with_surrounding_whitespace() {
            assert_eq!(
                " tsumogiri ".parse::<AgentKind>().unwrap(),
                AgentKind::Tsumogiri
            );
            assert_eq!(
                " Shanten ".parse::<AgentKind>().unwrap(),
                AgentKind::Shanten
            );
        }

        #[test]
        fn unknown_value_is_error() {
            assert_eq!(
                "nodocchi".parse::<AgentKind>(),
                Err(AgentKindError::Unknown("nodocchi".to_string()))
            );
        }

        #[test]
        fn display_matches_env_values() {
            assert_eq!(AgentKind::Normal.to_string(), "normal");
            assert_eq!(AgentKind::Tsumogiri.to_string(), "tsumogiri");
            assert_eq!(AgentKind::Shanten.to_string(), "shanten");
            assert_eq!(AgentKind::Menzen.to_string(), "menzen");
        }
    }

    mod context_helper {
        use super::*;
        use bot_logic::TileId;

        #[test]
        fn last_tsumo_becomes_drawn_tile() {
            let state = state_with_tsumo(0, "6p");
            assert_eq!(
                game_context_from_validation_state(&state).drawn_tile(),
                TileId::new(56)
            );
        }

        #[test]
        fn red_five_becomes_red_tile_id() {
            let state = state_with_tsumo(0, "5mr");
            assert_eq!(
                game_context_from_validation_state(&state).drawn_tile(),
                TileId::new(16)
            );
        }

        #[test]
        fn no_last_tsumo_has_no_drawn_tile() {
            let state = ValidationState::new();
            assert_eq!(
                game_context_from_validation_state(&state).drawn_tile(),
                None
            );
        }

        #[test]
        fn invalid_pai_has_no_drawn_tile() {
            let state = state_with_tsumo(0, "invalid");
            assert_eq!(
                game_context_from_validation_state(&state).drawn_tile(),
                None
            );
        }

        #[test]
        fn hidden_pai_has_no_drawn_tile() {
            let state = state_with_tsumo(0, "?");
            assert_eq!(
                game_context_from_validation_state(&state).drawn_tile(),
                None
            );
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
        fn prefers_tsumogiri_over_none() {
            let state = state_with_tsumo(1, "6p");
            let possible_actions = vec![possible_dahai("6p"), MjaiPossibleAction::None];
            let response = build_tsumogiri_response(&state, 9, &possible_actions).unwrap();
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
        fn does_not_discard_when_last_tsumo_is_not_a_possible_dahai() {
            let state = state_with_tsumo(0, "6p");
            let possible_actions = vec![possible_dahai("1m")];
            assert_eq!(
                build_tsumogiri_response(&state, 42, &possible_actions),
                None
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
        fn returns_none_action_even_when_seat_id_is_unset() {
            let state = ValidationState::new();
            let possible_actions = vec![MjaiPossibleAction::None];
            let response = build_tsumogiri_response(&state, 5, &possible_actions).unwrap();
            assert_eq!(
                response,
                MjaiAction::None {
                    request_id: Some(5)
                }
            );
        }

        #[test]
        fn does_not_pick_dahai_without_last_tsumo() {
            let mut state = ValidationState::new();
            state.on_start_game(0);
            let possible_actions = vec![possible_dahai("1m"), MjaiPossibleAction::None];
            let response = build_tsumogiri_response(&state, 8, &possible_actions).unwrap();
            assert_eq!(
                response,
                MjaiAction::None {
                    request_id: Some(8)
                }
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
        fn falls_back_without_last_tsumo_and_without_none() {
            let state = ValidationState::new();
            let possible_actions = vec![possible_dahai("1m")];
            assert_eq!(build_tsumogiri_response(&state, 1, &possible_actions), None);
        }

        #[test]
        fn empty_possible_actions_falls_back() {
            let state = state_with_tsumo(0, "6p");
            assert_eq!(build_tsumogiri_response(&state, 1, &[]), None);
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
        fn discards_last_tsumo_with_tsumogiri_flag() {
            let state = state_with_tsumo(0, "6p");
            let possible_actions = vec![possible_dahai("1m"), possible_dahai("6p")];
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
        }

        #[test]
        fn discards_first_dahai_without_last_tsumo() {
            let mut state = ValidationState::new();
            state.on_start_game(0);
            let possible_actions = vec![possible_dahai("1m"), possible_dahai("9s")];
            let response = build_normal_response(&state, 28, &possible_actions).unwrap();
            assert_eq!(
                response,
                MjaiAction::Dahai {
                    actor: 0,
                    pai: "1m".to_string(),
                    tsumogiri: None,
                    request_id: Some(28),
                }
            );
        }

        #[test]
        fn discards_first_dahai_when_last_tsumo_does_not_match() {
            let state = state_with_tsumo(0, "6p");
            let possible_actions = vec![possible_dahai("1m"), possible_dahai("9s")];
            let response = build_normal_response(&state, 29, &possible_actions).unwrap();
            assert_eq!(
                response,
                MjaiAction::Dahai {
                    actor: 0,
                    pai: "1m".to_string(),
                    tsumogiri: None,
                    request_id: Some(29),
                }
            );
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

    mod shanten_policy {
        use super::*;
        use bot_logic::TileId;

        fn tile(value: u8) -> TileId {
            TileId::new(value).unwrap()
        }

        #[test]
        fn discards_drawn_tile_from_context() {
            let state = state_with_tsumo(0, "1m");
            let context = GameContext::with_drawn_tile(tile(0));
            let possible_actions = vec![possible_dahai("1m")];
            let response =
                build_shanten_response_with_context(&state, &context, 42, &possible_actions);
            assert_eq!(
                response,
                Some(MjaiAction::Dahai {
                    actor: 0,
                    pai: "1m".to_string(),
                    tsumogiri: Some(true),
                    request_id: Some(42),
                })
            );
        }

        #[test]
        fn hora_takes_priority_over_dahai() {
            let state = state_with_tsumo(1, "6p");
            let context = GameContext::with_drawn_tile(tile(56));
            let possible_actions = vec![possible_dahai("6p"), MjaiPossibleAction::Hora];
            let response =
                build_shanten_response_with_context(&state, &context, 80, &possible_actions);
            assert_eq!(
                response,
                Some(MjaiAction::Hora {
                    actor: 1,
                    target: None,
                    pai: None,
                    request_id: Some(80),
                })
            );
        }

        #[test]
        fn ryukyoku_takes_priority_over_dahai() {
            let state = state_with_tsumo(0, "6p");
            let context = GameContext::with_drawn_tile(tile(56));
            let possible_actions = vec![possible_dahai("6p"), MjaiPossibleAction::Ryukyoku];
            let response =
                build_shanten_response_with_context(&state, &context, 81, &possible_actions);
            assert_eq!(
                response,
                Some(MjaiAction::Ryukyoku {
                    request_id: Some(81),
                })
            );
        }

        #[test]
        fn builds_response_with_hand_tiles_in_context() {
            let mut state = ValidationState::new();
            state.on_start_game(0);
            let context =
                GameContext::from_parts(Some(tile(56)), vec![tile(0), tile(16), tile(56)]);
            let possible_actions = vec![possible_dahai("1m"), possible_dahai("6p")];
            let response =
                build_shanten_response_with_context(&state, &context, 82, &possible_actions);
            assert!(matches!(response, Some(MjaiAction::Dahai { .. })));
        }

        #[test]
        fn does_not_return_actions_missing_from_possible_actions() {
            let state = state_with_tsumo(0, "6p");
            let context = game_context_from_validation_state(&state);
            let possible_actions = vec![possible_pon()];
            assert_eq!(
                build_shanten_response_with_context(&state, &context, 83, &possible_actions),
                None
            );
            assert_eq!(
                build_shanten_response_with_context(&state, &context, 83, &[]),
                None
            );
        }

        #[test]
        fn passes_with_none_action_on_claim_opportunity() {
            let state = ValidationState::new();
            let context = GameContext::default();
            let possible_actions = vec![possible_pon(), MjaiPossibleAction::None];
            let response =
                build_shanten_response_with_context(&state, &context, 84, &possible_actions);
            assert_eq!(
                response,
                Some(MjaiAction::None {
                    request_id: Some(84),
                })
            );
        }

        #[test]
        fn compat_builder_uses_state_derived_context() {
            let state = state_with_tsumo(0, "6p");
            let context = game_context_from_validation_state(&state);
            let possible_actions = vec![possible_dahai("1m"), possible_dahai("6p")];
            assert_eq!(
                build_shanten_response(&state, 85, &possible_actions),
                build_shanten_response_with_context(&state, &context, 85, &possible_actions)
            );
        }
    }

    mod menzen_policy {
        use super::*;

        #[test]
        fn passes_on_pon_opportunity() {
            let state = ValidationState::new();
            let possible_actions = vec![possible_pon(), MjaiPossibleAction::None];
            let response = build_menzen_response(&state, 90, &possible_actions);
            assert_eq!(
                response,
                Some(MjaiAction::None {
                    request_id: Some(90),
                })
            );
        }

        #[test]
        fn hora_survives_pon_filtering() {
            let state = state_with_tsumo(1, "6p");
            let possible_actions = vec![
                possible_pon(),
                MjaiPossibleAction::Hora,
                MjaiPossibleAction::None,
            ];
            let response = build_menzen_response(&state, 91, &possible_actions);
            assert_eq!(
                response,
                Some(MjaiAction::Hora {
                    actor: 1,
                    target: None,
                    pai: None,
                    request_id: Some(91),
                })
            );
        }

        #[test]
        fn matches_shanten_policy_without_meld_actions() {
            let state = state_with_tsumo(0, "6p");
            for possible_actions in [
                vec![possible_dahai("1m"), possible_dahai("6p")],
                vec![MjaiPossibleAction::Reach, possible_dahai("6p")],
                vec![MjaiPossibleAction::Hora, possible_dahai("6p")],
            ] {
                assert_eq!(
                    build_menzen_response(&state, 92, &possible_actions),
                    build_shanten_response(&state, 92, &possible_actions)
                );
            }
        }
    }

    mod policy_with_context {
        use super::*;
        use bot_logic::TileId;

        fn tile(value: u8) -> TileId {
            TileId::new(value).unwrap()
        }

        #[test]
        fn normal_marks_tsumogiri_from_context_drawn_tile() {
            let mut state = ValidationState::new();
            state.on_start_game(0);
            let context = GameContext::with_drawn_tile(tile(56));
            let possible_actions = vec![possible_dahai("1m"), possible_dahai("6p")];
            let response =
                build_normal_response_with_context(&state, &context, 60, &possible_actions)
                    .unwrap();
            assert_eq!(
                response,
                MjaiAction::Dahai {
                    actor: 0,
                    pai: "6p".to_string(),
                    tsumogiri: Some(true),
                    request_id: Some(60),
                }
            );
        }

        #[test]
        fn tsumogiri_discards_from_context_without_last_tsumo() {
            let mut state = ValidationState::new();
            state.on_start_game(1);
            assert_eq!(state.last_tsumo(), None);
            let context = GameContext::with_drawn_tile(tile(56));
            let possible_actions = vec![possible_dahai("6p"), MjaiPossibleAction::None];
            let response =
                build_tsumogiri_response_with_context(&state, &context, 61, &possible_actions)
                    .unwrap();
            assert_eq!(
                response,
                MjaiAction::Dahai {
                    actor: 1,
                    pai: "6p".to_string(),
                    tsumogiri: Some(true),
                    request_id: Some(61),
                }
            );
        }

        #[test]
        fn tsumogiri_without_context_and_state_does_not_pick_dahai() {
            let mut state = ValidationState::new();
            state.on_start_game(0);
            let context = GameContext::default();
            let possible_actions = vec![possible_dahai("1m"), MjaiPossibleAction::None];
            let response =
                build_tsumogiri_response_with_context(&state, &context, 62, &possible_actions)
                    .unwrap();
            assert_eq!(
                response,
                MjaiAction::None {
                    request_id: Some(62)
                }
            );
        }

        #[test]
        fn context_takes_precedence_over_state_last_tsumo() {
            let state = state_with_tsumo(0, "1m");
            let context = GameContext::with_drawn_tile(tile(56));
            let possible_actions = vec![possible_dahai("1m"), possible_dahai("6p")];
            let response =
                build_tsumogiri_response_with_context(&state, &context, 63, &possible_actions)
                    .unwrap();
            assert_eq!(
                response,
                MjaiAction::Dahai {
                    actor: 0,
                    pai: "6p".to_string(),
                    tsumogiri: Some(true),
                    request_id: Some(63),
                }
            );
        }

        #[test]
        fn normal_keeps_priority_with_hand_tiles() {
            let mut state = ValidationState::new();
            state.on_start_game(0);
            let context =
                GameContext::from_parts(Some(tile(56)), vec![tile(0), tile(16), tile(56)]);
            let possible_actions = vec![
                possible_dahai("1m"),
                possible_dahai("6p"),
                MjaiPossibleAction::Reach,
                MjaiPossibleAction::Hora,
            ];
            let response =
                build_normal_response_with_context(&state, &context, 70, &possible_actions)
                    .unwrap();
            assert_eq!(
                response,
                MjaiAction::Hora {
                    actor: 0,
                    target: None,
                    pai: None,
                    request_id: Some(70),
                }
            );
        }

        #[test]
        fn normal_dahai_ignores_hand_tiles() {
            let mut state = ValidationState::new();
            state.on_start_game(0);
            let context =
                GameContext::from_parts(Some(tile(56)), vec![tile(0), tile(16), tile(56)]);
            let possible_actions = vec![possible_dahai("1m"), possible_dahai("6p")];
            let response =
                build_normal_response_with_context(&state, &context, 71, &possible_actions)
                    .unwrap();
            assert_eq!(
                response,
                MjaiAction::Dahai {
                    actor: 0,
                    pai: "6p".to_string(),
                    tsumogiri: Some(true),
                    request_id: Some(71),
                }
            );
        }

        #[test]
        fn tsumogiri_keeps_drawn_tile_basis_with_hand_tiles() {
            let mut state = ValidationState::new();
            state.on_start_game(1);
            let context =
                GameContext::from_parts(Some(tile(56)), vec![tile(0), tile(16), tile(56)]);
            let possible_actions = vec![
                possible_dahai("1m"),
                possible_dahai("6p"),
                MjaiPossibleAction::None,
            ];
            let response =
                build_tsumogiri_response_with_context(&state, &context, 72, &possible_actions)
                    .unwrap();
            assert_eq!(
                response,
                MjaiAction::Dahai {
                    actor: 1,
                    pai: "6p".to_string(),
                    tsumogiri: Some(true),
                    request_id: Some(72),
                }
            );
        }

        #[test]
        fn tsumogiri_with_hand_tiles_but_no_drawn_tile_does_not_pick_dahai() {
            let mut state = ValidationState::new();
            state.on_start_game(0);
            let context = GameContext::with_hand_tiles(vec![tile(0), tile(16)]);
            let possible_actions = vec![possible_dahai("1m"), MjaiPossibleAction::None];
            let response =
                build_tsumogiri_response_with_context(&state, &context, 73, &possible_actions)
                    .unwrap();
            assert_eq!(
                response,
                MjaiAction::None {
                    request_id: Some(73)
                }
            );
        }

        #[test]
        fn compat_builders_use_state_derived_context() {
            let state = state_with_tsumo(0, "6p");
            let context = game_context_from_validation_state(&state);
            let possible_actions = vec![possible_dahai("1m"), possible_dahai("6p")];
            assert_eq!(
                build_tsumogiri_response(&state, 64, &possible_actions),
                build_tsumogiri_response_with_context(&state, &context, 64, &possible_actions)
            );
            assert_eq!(
                build_normal_response(&state, 64, &possible_actions),
                build_normal_response_with_context(&state, &context, 64, &possible_actions)
            );
        }
    }

    mod response_policy_dispatch {
        use super::*;

        #[test]
        fn tsumogiri_kind_uses_tsumogiri_policy() {
            let policy = AgentKind::Tsumogiri.response_policy();
            let state = state_with_tsumo(0, "6p");
            let context = game_context_from_validation_state(&state);
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
                    policy(&state, &context, 30, &possible_actions),
                    build_tsumogiri_response(&state, 30, &possible_actions)
                );
            }
        }

        #[test]
        fn shanten_kind_uses_shanten_policy() {
            let policy = AgentKind::Shanten.response_policy();
            let state = state_with_tsumo(0, "6p");
            let context = game_context_from_validation_state(&state);
            for possible_actions in [
                vec![
                    MjaiPossibleAction::Hora,
                    possible_dahai("6p"),
                    MjaiPossibleAction::None,
                ],
                vec![MjaiPossibleAction::Ryukyoku, possible_dahai("6p")],
                vec![possible_pon(), MjaiPossibleAction::None],
                vec![possible_pon()],
            ] {
                assert_eq!(
                    policy(&state, &context, 32, &possible_actions),
                    build_shanten_response(&state, 32, &possible_actions)
                );
            }
        }

        #[test]
        fn menzen_kind_uses_menzen_policy() {
            let policy = AgentKind::Menzen.response_policy();
            let state = state_with_tsumo(0, "6p");
            let context = game_context_from_validation_state(&state);
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
                    policy(&state, &context, 33, &possible_actions),
                    build_menzen_response(&state, 33, &possible_actions)
                );
            }
        }

        #[test]
        fn normal_kind_uses_normal_policy() {
            let policy = AgentKind::Normal.response_policy();
            let state = state_with_tsumo(0, "6p");
            let context = game_context_from_validation_state(&state);
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
                    policy(&state, &context, 31, &possible_actions),
                    build_normal_response(&state, 31, &possible_actions)
                );
            }
        }
    }
}
