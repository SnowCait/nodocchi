use bot_core::Agent;

use crate::decision::{
    NakuDecisionError, SutehaiDecisionError, build_naku_reply_for_request,
    build_naku_reply_for_request_with_state, build_sutehai_reply_for_request,
    build_sutehai_reply_for_request_with_state,
};
use crate::match_state::ChiihouTableSnapshot;
use crate::protocol::{ChiihouProtocolError, ChiihouRequest, parse_chiihou_request};

#[derive(Debug, thiserror::Error, Clone, PartialEq, Eq)]
pub enum ChiihouHandlerError {
    #[error("failed to parse chiihou request: {0}")]
    Protocol(#[from] ChiihouProtocolError),

    #[error("failed to choose sutehai: {0}")]
    Sutehai(#[from] SutehaiDecisionError),

    #[error("failed to choose naku: {0}")]
    Naku(#[from] NakuDecisionError),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChiihouHandlerResult {
    NoReply,
    ReplyContent(String),
}

pub fn build_reply_for_request<A: Agent>(
    server_npub: &str,
    request: &ChiihouRequest,
    agent: &mut A,
) -> Result<ChiihouHandlerResult, ChiihouHandlerError> {
    let content = match request {
        ChiihouRequest::Sutehai { .. } => {
            build_sutehai_reply_for_request(server_npub, request, agent)?
        }
        ChiihouRequest::Naku { .. } => build_naku_reply_for_request(server_npub, request, agent)?,
    };
    Ok(ChiihouHandlerResult::ReplyContent(content))
}

pub fn build_reply_for_request_with_state<A: Agent>(
    server_npub: &str,
    request: &ChiihouRequest,
    state: &ChiihouTableSnapshot,
    agent: &mut A,
) -> Result<ChiihouHandlerResult, ChiihouHandlerError> {
    let content = match request {
        ChiihouRequest::Sutehai { .. } => {
            build_sutehai_reply_for_request_with_state(server_npub, request, state, agent)?
        }
        ChiihouRequest::Naku { .. } => {
            build_naku_reply_for_request_with_state(server_npub, request, state, agent)?
        }
    };
    Ok(ChiihouHandlerResult::ReplyContent(content))
}

pub fn handle_chiihou_content<A: Agent>(
    server_npub: &str,
    content: &str,
    agent: &mut A,
) -> Result<ChiihouHandlerResult, ChiihouHandlerError> {
    match parse_chiihou_request(content)? {
        None => Ok(ChiihouHandlerResult::NoReply),
        Some(request) => build_reply_for_request(server_npub, &request, agent),
    }
}

pub fn handle_chiihou_content_with_state<A: Agent>(
    server_npub: &str,
    content: &str,
    state: &ChiihouTableSnapshot,
    agent: &mut A,
) -> Result<ChiihouHandlerResult, ChiihouHandlerError> {
    match parse_chiihou_request(content)? {
        None => Ok(ChiihouHandlerResult::NoReply),
        Some(request) => build_reply_for_request_with_state(server_npub, &request, state, agent),
    }
}

pub fn reply_content_for_chiihou_content_with_state<A: Agent>(
    server_npub: &str,
    content: &str,
    state: &ChiihouTableSnapshot,
    agent: &mut A,
) -> Result<Option<String>, ChiihouHandlerError> {
    match handle_chiihou_content_with_state(server_npub, content, state, agent)? {
        ChiihouHandlerResult::NoReply => Ok(None),
        ChiihouHandlerResult::ReplyContent(content) => Ok(Some(content)),
    }
}

pub fn reply_content_for_chiihou_content<A: Agent>(
    server_npub: &str,
    content: &str,
    agent: &mut A,
) -> Result<Option<String>, ChiihouHandlerError> {
    match handle_chiihou_content(server_npub, content, agent)? {
        ChiihouHandlerResult::NoReply => Ok(None),
        ChiihouHandlerResult::ReplyContent(content) => Ok(Some(content)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::{ChiihouNakuAction, ChiihouPai};
    use bot_core::{GameContext, LegalAction};

    fn pai(s: &str) -> ChiihouPai {
        s.parse().unwrap()
    }

    struct PickSecondAgent;

    impl Agent for PickSecondAgent {
        fn act(&mut self, _ctx: &GameContext, legal_actions: &[LegalAction]) -> LegalAction {
            legal_actions.get(1).cloned().unwrap_or(LegalAction::None)
        }
    }

    struct FixedActionAgent(LegalAction);

    impl Agent for FixedActionAgent {
        fn act(&mut self, _ctx: &GameContext, _legal_actions: &[LegalAction]) -> LegalAction {
            self.0.clone()
        }
    }

    fn tenpai_sutehai_request() -> ChiihouRequest {
        ChiihouRequest::Sutehai {
            hand: vec![
                pai("1m"),
                pai("2m"),
                pai("3m"),
                pai("4m"),
                pai("5m"),
                pai("6m"),
                pai("7m"),
                pai("8m"),
                pai("9m"),
                pai("1p"),
                pai("2p"),
                pai("3p"),
                pai("1s"),
            ],
            drawn: Some(pai("1z")),
        }
    }

    #[test]
    fn builds_sutehai_reply_from_sutehai_request() {
        assert_eq!(
            build_reply_for_request(
                "npub1server",
                &tenpai_sutehai_request(),
                &mut PickSecondAgent
            ),
            Ok(ChiihouHandlerResult::ReplyContent(
                "nostr:npub1server sutehai? sutehai 2m".to_string()
            ))
        );
    }

    #[test]
    fn invalid_hand_tile_count_request_is_sutehai_error() {
        let request = ChiihouRequest::Sutehai {
            hand: vec![pai("1m"), pai("2m"), pai("3m")],
            drawn: Some(pai("1z")),
        };
        assert_eq!(
            build_reply_for_request("npub1server", &request, &mut PickSecondAgent),
            Err(ChiihouHandlerError::Sutehai(
                SutehaiDecisionError::InvalidHandTileCount(3)
            ))
        );
    }

    #[test]
    fn builds_naku_no_reply_from_naku_request() {
        let request = ChiihouRequest::Naku {
            hand: vec![pai("1m"), pai("2m"), pai("3m")],
            target: pai("4m"),
            actions: vec![ChiihouNakuAction::Kan],
        };
        assert_eq!(
            build_reply_for_request("npub1server", &request, &mut PickSecondAgent),
            Ok(ChiihouHandlerResult::ReplyContent(
                "nostr:npub1server naku? no".to_string()
            ))
        );
    }

    #[test]
    fn naku_request_with_ron_replies_no_when_agent_declines() {
        let request = ChiihouRequest::Naku {
            hand: vec![pai("1m"), pai("2m"), pai("3m")],
            target: pai("4m"),
            actions: vec![
                ChiihouNakuAction::Ron,
                ChiihouNakuAction::Pon,
                ChiihouNakuAction::Chi,
            ],
        };
        assert_eq!(
            build_reply_for_request(
                "npub1server",
                &request,
                &mut FixedActionAgent(LegalAction::None)
            ),
            Ok(ChiihouHandlerResult::ReplyContent(
                "nostr:npub1server naku? no".to_string()
            ))
        );
    }

    #[test]
    fn naku_request_with_ron_replies_ron_when_agent_picks_hora() {
        let request = ChiihouRequest::Naku {
            hand: vec![pai("1m"), pai("2m"), pai("3m")],
            target: pai("4m"),
            actions: vec![ChiihouNakuAction::Ron],
        };
        assert_eq!(
            build_reply_for_request(
                "npub1server",
                &request,
                &mut FixedActionAgent(LegalAction::Hora)
            ),
            Ok(ChiihouHandlerResult::ReplyContent(
                "nostr:npub1server naku? ron".to_string()
            ))
        );
    }

    #[test]
    fn naku_request_without_ron_replies_no_even_if_agent_picks_hora() {
        let request = ChiihouRequest::Naku {
            hand: vec![pai("1m"), pai("2m"), pai("3m")],
            target: pai("4m"),
            actions: vec![ChiihouNakuAction::Pon, ChiihouNakuAction::Chi],
        };
        assert_eq!(
            build_reply_for_request(
                "npub1server",
                &request,
                &mut FixedActionAgent(LegalAction::Hora)
            ),
            Ok(ChiihouHandlerResult::ReplyContent(
                "nostr:npub1server naku? no".to_string()
            ))
        );
    }

    #[test]
    fn naku_request_falls_back_to_no_for_illegal_agent_action() {
        let request = ChiihouRequest::Naku {
            hand: vec![pai("1m"), pai("2m"), pai("3m")],
            target: pai("4m"),
            actions: vec![ChiihouNakuAction::Ron],
        };
        for action in [
            LegalAction::Dahai {
                tile: crate::convert::temporary_tile_id_from_chiihou_pai(pai("4m")),
            },
            LegalAction::Pon {
                tile: crate::convert::temporary_tile_id_from_chiihou_pai(pai("4m")),
                consumed: vec![],
            },
        ] {
            assert_eq!(
                build_reply_for_request("npub1server", &request, &mut FixedActionAgent(action)),
                Ok(ChiihouHandlerResult::ReplyContent(
                    "nostr:npub1server naku? no".to_string()
                ))
            );
        }
    }

    fn complete_sutehai_request() -> ChiihouRequest {
        ChiihouRequest::Sutehai {
            hand: vec![
                pai("1m"),
                pai("2m"),
                pai("3m"),
                pai("4m"),
                pai("5m"),
                pai("6m"),
                pai("7m"),
                pai("8m"),
                pai("9m"),
                pai("1p"),
                pai("2p"),
                pai("3p"),
                pai("5p"),
            ],
            drawn: Some(pai("5p")),
        }
    }

    #[test]
    fn builds_tsumo_reply_from_complete_sutehai_request() {
        assert_eq!(
            build_reply_for_request(
                "npub1server",
                &complete_sutehai_request(),
                &mut FixedActionAgent(LegalAction::Hora)
            ),
            Ok(ChiihouHandlerResult::ReplyContent(
                "nostr:npub1server sutehai? tsumo".to_string()
            ))
        );
    }

    #[test]
    fn builds_tsumo_reply_from_complete_sutehai_request_with_state() {
        assert_eq!(
            build_reply_for_request_with_state(
                "npub1server",
                &complete_sutehai_request(),
                &crate::match_state::ChiihouTableSnapshot::default(),
                &mut FixedActionAgent(LegalAction::Hora)
            ),
            Ok(ChiihouHandlerResult::ReplyContent(
                "nostr:npub1server sutehai? tsumo".to_string()
            ))
        );
    }

    #[test]
    fn handles_complete_sutehai_content_with_tsumo() {
        let content = "\
:mahjong_m1::mahjong_m2::mahjong_m3::mahjong_m4::mahjong_m5::mahjong_m6::mahjong_m7::mahjong_m8::mahjong_m9::mahjong_p1::mahjong_p2::mahjong_p3::mahjong_p5: :mahjong_p5:
nostr:npub1ai000 GET sutehai?";
        assert_eq!(
            handle_chiihou_content(
                "npub1server",
                content,
                &mut FixedActionAgent(LegalAction::Hora)
            ),
            Ok(ChiihouHandlerResult::ReplyContent(
                "nostr:npub1server sutehai? tsumo".to_string()
            ))
        );
    }

    #[test]
    fn handles_sutehai_content() {
        let content = "\
:mahjong_m1::mahjong_m2::mahjong_m3::mahjong_m4::mahjong_m5::mahjong_m6::mahjong_m7::mahjong_m8::mahjong_m9::mahjong_p1::mahjong_p2::mahjong_p3::mahjong_s1: :mahjong_east:
nostr:npub1ai000 GET sutehai?";
        assert_eq!(
            handle_chiihou_content("npub1server", content, &mut PickSecondAgent),
            Ok(ChiihouHandlerResult::ReplyContent(
                "nostr:npub1server sutehai? sutehai 2m".to_string()
            ))
        );
    }

    #[test]
    fn invalid_hand_tile_count_content_is_sutehai_error() {
        let content = "\
:mahjong_m1::mahjong_m2::mahjong_m3: :mahjong_east:
nostr:npub1ai000 GET sutehai?";
        assert_eq!(
            handle_chiihou_content("npub1server", content, &mut PickSecondAgent),
            Err(ChiihouHandlerError::Sutehai(
                SutehaiDecisionError::InvalidHandTileCount(3)
            ))
        );
    }

    #[test]
    fn handles_naku_content_with_no_when_agent_declines() {
        let content = "\
:mahjong_m1::mahjong_m2::mahjong_m3: :mahjong_m4:
nostr:npub1ai000 GET naku? ron pon chi";
        assert_eq!(
            handle_chiihou_content("npub1server", content, &mut PickSecondAgent),
            Ok(ChiihouHandlerResult::ReplyContent(
                "nostr:npub1server naku? no".to_string()
            ))
        );
    }

    #[test]
    fn handles_naku_content_with_ron_when_agent_picks_hora() {
        let content = "\
:mahjong_m1::mahjong_m2::mahjong_m3: :mahjong_m4:
nostr:npub1ai000 GET naku? ron pon chi";
        assert_eq!(
            handle_chiihou_content(
                "npub1server",
                content,
                &mut FixedActionAgent(LegalAction::Hora)
            ),
            Ok(ChiihouHandlerResult::ReplyContent(
                "nostr:npub1server naku? ron".to_string()
            ))
        );
    }

    #[test]
    fn unrelated_content_is_no_reply() {
        assert_eq!(
            handle_chiihou_content("npub1server", "nostr:npub1ai000 join", &mut PickSecondAgent),
            Ok(ChiihouHandlerResult::NoReply)
        );
    }

    #[test]
    fn unparsable_sutehai_content_is_protocol_error() {
        let content = "\
:mahjong_m1::mahjong_m2: :mahjong_m3::mahjong_m4:
nostr:npub1ai000 GET sutehai?";
        assert_eq!(
            handle_chiihou_content("npub1server", content, &mut PickSecondAgent),
            Err(ChiihouHandlerError::Protocol(
                ChiihouProtocolError::InvalidTileLayout
            ))
        );
    }

    #[test]
    fn sutehai_without_candidates_is_sutehai_error() {
        let request = ChiihouRequest::Sutehai {
            hand: vec![],
            drawn: None,
        };
        assert_eq!(
            build_reply_for_request("npub1server", &request, &mut PickSecondAgent),
            Err(ChiihouHandlerError::Sutehai(
                SutehaiDecisionError::NoLegalDahai
            ))
        );
    }

    #[test]
    fn reply_content_helper_returns_some_for_reply() {
        let content = "\
:mahjong_m1::mahjong_m2::mahjong_m3::mahjong_m4::mahjong_m5::mahjong_m6::mahjong_m7::mahjong_m8::mahjong_m9::mahjong_p1::mahjong_p2::mahjong_p3::mahjong_s1: :mahjong_east:
nostr:npub1ai000 GET sutehai?";
        assert_eq!(
            reply_content_for_chiihou_content("npub1server", content, &mut PickSecondAgent),
            Ok(Some("nostr:npub1server sutehai? sutehai 2m".to_string()))
        );
    }

    #[test]
    fn reply_content_helper_returns_none_for_no_reply() {
        assert_eq!(
            reply_content_for_chiihou_content(
                "npub1server",
                "nostr:npub1ai000 join",
                &mut PickSecondAgent
            ),
            Ok(None)
        );
    }
}
