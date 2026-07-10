use bot_core::Agent;

use crate::decision::{SutehaiDecisionError, build_sutehai_reply_for_request};
use crate::protocol::{ChiihouProtocolError, ChiihouRequest, parse_chiihou_request};
use crate::reply::build_naku_no_reply_content;

#[derive(Debug, thiserror::Error, Clone, PartialEq, Eq)]
pub enum ChiihouHandlerError {
    #[error("failed to parse chiihou request: {0}")]
    Protocol(#[from] ChiihouProtocolError),

    #[error("failed to choose sutehai: {0}")]
    Sutehai(#[from] SutehaiDecisionError),
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
        ChiihouRequest::Naku { .. } => build_naku_no_reply_content(server_npub),
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

    #[test]
    fn builds_sutehai_reply_from_sutehai_request() {
        let request = ChiihouRequest::Sutehai {
            hand: vec![pai("1m"), pai("2m"), pai("3m")],
            drawn: Some(pai("1z")),
        };
        assert_eq!(
            build_reply_for_request("npub1server", &request, &mut PickSecondAgent),
            Ok(ChiihouHandlerResult::ReplyContent(
                "nostr:npub1server sutehai? sutehai 2m".to_string()
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
    fn naku_request_with_ron_pon_chi_still_replies_no() {
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
            build_reply_for_request("npub1server", &request, &mut PickSecondAgent),
            Ok(ChiihouHandlerResult::ReplyContent(
                "nostr:npub1server naku? no".to_string()
            ))
        );
    }

    #[test]
    fn handles_sutehai_content() {
        let content = "\
:mahjong_m1::mahjong_m2::mahjong_m3: :mahjong_east:
nostr:npub1ai000 GET sutehai?";
        assert_eq!(
            handle_chiihou_content("npub1server", content, &mut PickSecondAgent),
            Ok(ChiihouHandlerResult::ReplyContent(
                "nostr:npub1server sutehai? sutehai 2m".to_string()
            ))
        );
    }

    #[test]
    fn handles_naku_content_with_fixed_no() {
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
:mahjong_m1::mahjong_m2::mahjong_m3: :mahjong_east:
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
