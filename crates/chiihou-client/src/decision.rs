use bot_core::{Agent, GameContext, LegalAction};
use bot_logic::{TileCounts, TileId, calculate_shanten};

use crate::convert::{
    chiihou_pai_from_tile_id, temporary_tile_id_from_chiihou_pai, tile_type_from_chiihou_pai,
    tile_type_from_chiihou_wind,
};
use crate::lifecycle::CHIIHOU_PLAYER_COUNT;
use crate::match_state::ChiihouTableSnapshot;
use crate::protocol::{ChiihouNakuAction, ChiihouPai, ChiihouRequest};
use crate::reply::{
    build_naku_no_reply_content, build_naku_ron_reply_content, build_sutehai_reply_content,
    build_sutehai_tsumo_reply_content,
};

const MENZEN_HAND_TILE_COUNT: usize = 13;

#[derive(Debug, thiserror::Error, Clone, PartialEq, Eq)]
pub enum SutehaiDecisionError {
    #[error("request is not GET sutehai?")]
    NotSutehaiRequest,
    #[error("no legal dahai candidates")]
    NoLegalDahai,
    #[error("invalid hand tile count: {0}")]
    InvalidHandTileCount(usize),
    #[error("too many tiles of the same kind: {0}")]
    TooManySameTiles(ChiihouPai),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChiihouSutehaiDecision {
    Tsumo,
    Dahai(ChiihouPai),
}

#[derive(Debug, thiserror::Error, Clone, PartialEq, Eq)]
pub enum NakuDecisionError {
    #[error("request is not GET naku?")]
    NotNakuRequest,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChiihouNakuDecision {
    Ron,
    No,
}

pub fn game_context_from_sutehai_request(
    hand: &[ChiihouPai],
    drawn: Option<ChiihouPai>,
) -> GameContext {
    let hand_tiles = hand
        .iter()
        .map(|&pai| temporary_tile_id_from_chiihou_pai(pai))
        .collect();
    let drawn_tile = drawn.map(temporary_tile_id_from_chiihou_pai);
    GameContext::from_parts(drawn_tile, hand_tiles)
}

fn game_context_from_request_with_state(
    hand: &[ChiihouPai],
    drawn: Option<ChiihouPai>,
    state: &ChiihouTableSnapshot,
) -> GameContext {
    let hand_tiles: Vec<TileId> = hand
        .iter()
        .map(|&pai| temporary_tile_id_from_chiihou_pai(pai))
        .collect();
    let drawn_tile = drawn.map(temporary_tile_id_from_chiihou_pai);
    let dora_indicators: Vec<TileId> = state
        .dora_indicators
        .iter()
        .map(|&pai| temporary_tile_id_from_chiihou_pai(pai))
        .collect();
    let mut discards: [Vec<TileId>; CHIIHOU_PLAYER_COUNT] = Default::default();
    for (river, state_river) in discards.iter_mut().zip(&state.discards) {
        *river = state_river
            .iter()
            .map(|&pai| temporary_tile_id_from_chiihou_pai(pai))
            .collect();
    }
    let mut visible_tiles = hand_tiles.clone();
    visible_tiles.extend(drawn_tile);
    visible_tiles.extend(dora_indicators.iter().copied());
    visible_tiles.extend(discards.iter().flatten().copied());
    GameContext::from_parts_with_table_state(
        drawn_tile,
        hand_tiles,
        dora_indicators,
        state.round_wind.map(tile_type_from_chiihou_wind),
        state.seat_wind.map(tile_type_from_chiihou_wind),
        visible_tiles,
        state.player_id,
        state.oya,
        discards,
        state.reached,
    )
}

pub fn game_context_from_sutehai_request_with_state(
    hand: &[ChiihouPai],
    drawn: Option<ChiihouPai>,
    state: &ChiihouTableSnapshot,
) -> GameContext {
    game_context_from_request_with_state(hand, drawn, state)
}

pub fn game_context_from_naku_request_with_state(
    hand: &[ChiihouPai],
    state: &ChiihouTableSnapshot,
) -> GameContext {
    game_context_from_request_with_state(hand, None, state)
}

pub fn legal_dahai_actions_from_sutehai_request(
    hand: &[ChiihouPai],
    drawn: Option<ChiihouPai>,
) -> Vec<LegalAction> {
    let mut actions = Vec::new();
    for &pai in hand.iter().chain(drawn.as_ref()) {
        let action = LegalAction::Dahai {
            tile: temporary_tile_id_from_chiihou_pai(pai),
        };
        if !actions.contains(&action) {
            actions.push(action);
        }
    }
    actions
}

pub fn is_complete_menzen_tsumo_hand(
    hand: &[ChiihouPai],
    drawn: ChiihouPai,
) -> Result<bool, SutehaiDecisionError> {
    if hand.len() != MENZEN_HAND_TILE_COUNT {
        return Err(SutehaiDecisionError::InvalidHandTileCount(hand.len()));
    }
    let mut counts = TileCounts::new();
    for &pai in hand.iter().chain(std::iter::once(&drawn)) {
        counts
            .try_add(tile_type_from_chiihou_pai(pai))
            .map_err(|_| SutehaiDecisionError::TooManySameTiles(pai))?;
    }
    Ok(calculate_shanten(&counts).min() == -1)
}

pub fn legal_actions_from_sutehai_request(
    hand: &[ChiihouPai],
    drawn: Option<ChiihouPai>,
) -> Result<Vec<LegalAction>, SutehaiDecisionError> {
    let mut actions = Vec::new();
    if let Some(drawn_pai) = drawn
        && hand.len() == MENZEN_HAND_TILE_COUNT
        && is_complete_menzen_tsumo_hand(hand, drawn_pai)?
    {
        actions.push(LegalAction::Hora);
    }
    actions.extend(legal_dahai_actions_from_sutehai_request(hand, drawn));
    Ok(actions)
}

pub fn chiihou_pai_from_dahai_action(action: &LegalAction) -> Option<ChiihouPai> {
    match action {
        LegalAction::Dahai { tile } => Some(chiihou_pai_from_tile_id(*tile)),
        _ => None,
    }
}

pub fn choose_sutehai_pai<A: Agent>(
    request: &ChiihouRequest,
    agent: &mut A,
) -> Result<ChiihouPai, SutehaiDecisionError> {
    let ChiihouRequest::Sutehai { hand, drawn } = request else {
        return Err(SutehaiDecisionError::NotSutehaiRequest);
    };
    let context = game_context_from_sutehai_request(hand, *drawn);
    choose_sutehai_pai_with_context(hand, *drawn, context, agent)
}

pub fn choose_sutehai_pai_with_state<A: Agent>(
    request: &ChiihouRequest,
    state: &ChiihouTableSnapshot,
    agent: &mut A,
) -> Result<ChiihouPai, SutehaiDecisionError> {
    let ChiihouRequest::Sutehai { hand, drawn } = request else {
        return Err(SutehaiDecisionError::NotSutehaiRequest);
    };
    let context = game_context_from_sutehai_request_with_state(hand, *drawn, state);
    choose_sutehai_pai_with_context(hand, *drawn, context, agent)
}

fn choose_sutehai_pai_with_context<A: Agent>(
    hand: &[ChiihouPai],
    drawn: Option<ChiihouPai>,
    context: GameContext,
    agent: &mut A,
) -> Result<ChiihouPai, SutehaiDecisionError> {
    let legal_actions = legal_dahai_actions_from_sutehai_request(hand, drawn);
    let Some(fallback) = legal_actions
        .first()
        .and_then(chiihou_pai_from_dahai_action)
    else {
        return Err(SutehaiDecisionError::NoLegalDahai);
    };
    let chosen = agent.act(&context, &legal_actions);
    if legal_actions.contains(&chosen) {
        return Ok(chiihou_pai_from_dahai_action(&chosen).unwrap_or(fallback));
    }
    Ok(fallback)
}

pub fn choose_sutehai_decision<A: Agent>(
    request: &ChiihouRequest,
    agent: &mut A,
) -> Result<ChiihouSutehaiDecision, SutehaiDecisionError> {
    let ChiihouRequest::Sutehai { hand, drawn } = request else {
        return Err(SutehaiDecisionError::NotSutehaiRequest);
    };
    let context = game_context_from_sutehai_request(hand, *drawn);
    choose_sutehai_decision_with_context(hand, *drawn, context, agent)
}

pub fn choose_sutehai_decision_with_state<A: Agent>(
    request: &ChiihouRequest,
    state: &ChiihouTableSnapshot,
    agent: &mut A,
) -> Result<ChiihouSutehaiDecision, SutehaiDecisionError> {
    let ChiihouRequest::Sutehai { hand, drawn } = request else {
        return Err(SutehaiDecisionError::NotSutehaiRequest);
    };
    let context = game_context_from_sutehai_request_with_state(hand, *drawn, state);
    choose_sutehai_decision_with_context(hand, *drawn, context, agent)
}

fn choose_sutehai_decision_with_context<A: Agent>(
    hand: &[ChiihouPai],
    drawn: Option<ChiihouPai>,
    context: GameContext,
    agent: &mut A,
) -> Result<ChiihouSutehaiDecision, SutehaiDecisionError> {
    let legal_actions = legal_actions_from_sutehai_request(hand, drawn)?;
    let Some(fallback) = legal_actions.iter().find_map(chiihou_pai_from_dahai_action) else {
        return Err(SutehaiDecisionError::NoLegalDahai);
    };
    let chosen = agent.act(&context, &legal_actions);
    if !legal_actions.contains(&chosen) {
        return Ok(ChiihouSutehaiDecision::Dahai(fallback));
    }
    if chosen == LegalAction::Hora {
        return Ok(ChiihouSutehaiDecision::Tsumo);
    }
    Ok(ChiihouSutehaiDecision::Dahai(
        chiihou_pai_from_dahai_action(&chosen).unwrap_or(fallback),
    ))
}

pub fn build_sutehai_reply_for_request<A: Agent>(
    server_npub: &str,
    request: &ChiihouRequest,
    agent: &mut A,
) -> Result<String, SutehaiDecisionError> {
    let decision = choose_sutehai_decision(request, agent)?;
    Ok(build_sutehai_reply_for_decision(server_npub, decision))
}

pub fn build_sutehai_reply_for_request_with_state<A: Agent>(
    server_npub: &str,
    request: &ChiihouRequest,
    state: &ChiihouTableSnapshot,
    agent: &mut A,
) -> Result<String, SutehaiDecisionError> {
    let decision = choose_sutehai_decision_with_state(request, state, agent)?;
    Ok(build_sutehai_reply_for_decision(server_npub, decision))
}

fn build_sutehai_reply_for_decision(server_npub: &str, decision: ChiihouSutehaiDecision) -> String {
    match decision {
        ChiihouSutehaiDecision::Tsumo => build_sutehai_tsumo_reply_content(server_npub),
        ChiihouSutehaiDecision::Dahai(pai) => build_sutehai_reply_content(server_npub, pai),
    }
}

pub fn game_context_from_naku_request(hand: &[ChiihouPai]) -> GameContext {
    GameContext::with_hand_tiles(
        hand.iter()
            .map(|&pai| temporary_tile_id_from_chiihou_pai(pai))
            .collect(),
    )
}

pub fn legal_actions_from_naku_actions(actions: &[ChiihouNakuAction]) -> Vec<LegalAction> {
    if actions.contains(&ChiihouNakuAction::Ron) {
        vec![LegalAction::Hora, LegalAction::None]
    } else {
        vec![LegalAction::None]
    }
}

pub fn choose_naku_decision<A: Agent>(
    request: &ChiihouRequest,
    agent: &mut A,
) -> Result<ChiihouNakuDecision, NakuDecisionError> {
    let ChiihouRequest::Naku { hand, actions, .. } = request else {
        return Err(NakuDecisionError::NotNakuRequest);
    };
    let context = game_context_from_naku_request(hand);
    choose_naku_decision_with_context(actions, context, agent)
}

pub fn choose_naku_decision_with_state<A: Agent>(
    request: &ChiihouRequest,
    state: &ChiihouTableSnapshot,
    agent: &mut A,
) -> Result<ChiihouNakuDecision, NakuDecisionError> {
    let ChiihouRequest::Naku { hand, actions, .. } = request else {
        return Err(NakuDecisionError::NotNakuRequest);
    };
    let context = game_context_from_naku_request_with_state(hand, state);
    choose_naku_decision_with_context(actions, context, agent)
}

fn choose_naku_decision_with_context<A: Agent>(
    actions: &[ChiihouNakuAction],
    context: GameContext,
    agent: &mut A,
) -> Result<ChiihouNakuDecision, NakuDecisionError> {
    let legal_actions = legal_actions_from_naku_actions(actions);
    let chosen = agent.act(&context, &legal_actions);
    if chosen == LegalAction::Hora && legal_actions.contains(&LegalAction::Hora) {
        return Ok(ChiihouNakuDecision::Ron);
    }
    Ok(ChiihouNakuDecision::No)
}

pub fn build_naku_reply_for_request<A: Agent>(
    server_npub: &str,
    request: &ChiihouRequest,
    agent: &mut A,
) -> Result<String, NakuDecisionError> {
    match choose_naku_decision(request, agent)? {
        ChiihouNakuDecision::Ron => Ok(build_naku_ron_reply_content(server_npub)),
        ChiihouNakuDecision::No => Ok(build_naku_no_reply_content(server_npub)),
    }
}

pub fn build_naku_reply_for_request_with_state<A: Agent>(
    server_npub: &str,
    request: &ChiihouRequest,
    state: &ChiihouTableSnapshot,
    agent: &mut A,
) -> Result<String, NakuDecisionError> {
    match choose_naku_decision_with_state(request, state, agent)? {
        ChiihouNakuDecision::Ron => Ok(build_naku_ron_reply_content(server_npub)),
        ChiihouNakuDecision::No => Ok(build_naku_no_reply_content(server_npub)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::ChiihouNakuAction;
    use bot_core::ShantenAgent;
    use bot_logic::TileId;

    fn pai(s: &str) -> ChiihouPai {
        s.parse().unwrap()
    }

    fn dahai(s: &str) -> LegalAction {
        LegalAction::Dahai {
            tile: temporary_tile_id_from_chiihou_pai(pai(s)),
        }
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

    struct CountingHoraAgent {
        calls: usize,
    }

    impl Agent for CountingHoraAgent {
        fn act(&mut self, _ctx: &GameContext, _legal_actions: &[LegalAction]) -> LegalAction {
            self.calls += 1;
            LegalAction::Hora
        }
    }

    fn pais(names: &[&str]) -> Vec<ChiihouPai> {
        names.iter().map(|name| pai(name)).collect()
    }

    fn complete_standard_hand() -> Vec<ChiihouPai> {
        pais(&[
            "1m", "2m", "3m", "4m", "5m", "6m", "7m", "8m", "9m", "1p", "2p", "3p", "5p",
        ])
    }

    fn complete_chiitoitsu_hand() -> Vec<ChiihouPai> {
        pais(&[
            "1m", "1m", "4m", "4m", "2p", "2p", "5p", "5p", "3s", "3s", "6s", "6s", "7z",
        ])
    }

    fn complete_kokushi_hand() -> Vec<ChiihouPai> {
        pais(&[
            "1m", "9m", "1p", "9p", "1s", "9s", "1z", "2z", "3z", "4z", "5z", "6z", "7z",
        ])
    }

    fn tenpai_hand() -> Vec<ChiihouPai> {
        pais(&[
            "1m", "2m", "3m", "4m", "5m", "6m", "7m", "8m", "9m", "1p", "2p", "3p", "1z",
        ])
    }

    fn iishanten_hand() -> Vec<ChiihouPai> {
        pais(&[
            "1m", "2m", "3m", "4m", "5m", "6m", "7m", "8m", "9m", "1p", "2p", "1z", "2z",
        ])
    }

    fn five_same_tiles_hand() -> Vec<ChiihouPai> {
        pais(&[
            "1m", "1m", "1m", "1m", "2m", "3m", "4m", "5m", "6m", "7m", "8m", "9m", "1p",
        ])
    }

    fn complete_sutehai_request() -> ChiihouRequest {
        ChiihouRequest::Sutehai {
            hand: complete_standard_hand(),
            drawn: Some(pai("5p")),
        }
    }

    #[test]
    fn game_context_holds_hand_tiles() {
        let context = game_context_from_sutehai_request(&[pai("1m"), pai("5p")], None);
        assert_eq!(
            context.hand_tiles(),
            &[
                temporary_tile_id_from_chiihou_pai(pai("1m")),
                temporary_tile_id_from_chiihou_pai(pai("5p")),
            ]
        );
    }

    #[test]
    fn game_context_holds_drawn_tile() {
        let context = game_context_from_sutehai_request(&[pai("1m")], Some(pai("7z")));
        assert_eq!(
            context.drawn_tile(),
            Some(temporary_tile_id_from_chiihou_pai(pai("7z")))
        );
    }

    #[test]
    fn game_context_without_drawn_has_no_drawn_tile() {
        let context = game_context_from_sutehai_request(&[pai("1m")], None);
        assert_eq!(context.drawn_tile(), None);
    }

    #[test]
    fn legal_dahai_actions_from_hand_only() {
        assert_eq!(
            legal_dahai_actions_from_sutehai_request(&[pai("1m"), pai("5p")], None),
            vec![dahai("1m"), dahai("5p")]
        );
    }

    #[test]
    fn legal_dahai_actions_include_drawn() {
        assert_eq!(
            legal_dahai_actions_from_sutehai_request(&[pai("1m")], Some(pai("7z"))),
            vec![dahai("1m"), dahai("7z")]
        );
    }

    #[test]
    fn legal_dahai_actions_deduplicate_same_pai() {
        assert_eq!(
            legal_dahai_actions_from_sutehai_request(
                &[pai("1m"), pai("1m"), pai("5p")],
                Some(pai("1m"))
            ),
            vec![dahai("1m"), dahai("5p")]
        );
    }

    #[test]
    fn legal_dahai_actions_keep_hand_then_drawn_order() {
        assert_eq!(
            legal_dahai_actions_from_sutehai_request(
                &[pai("5p"), pai("1m"), pai("9s")],
                Some(pai("3z"))
            ),
            vec![dahai("5p"), dahai("1m"), dahai("9s"), dahai("3z")]
        );
    }

    #[test]
    fn dahai_action_converts_back_to_chiihou_pai() {
        assert_eq!(chiihou_pai_from_dahai_action(&dahai("5p")), Some(pai("5p")));
        assert_eq!(chiihou_pai_from_dahai_action(&dahai("7z")), Some(pai("7z")));
    }

    #[test]
    fn non_dahai_actions_convert_to_none() {
        assert_eq!(chiihou_pai_from_dahai_action(&LegalAction::None), None);
        assert_eq!(chiihou_pai_from_dahai_action(&LegalAction::Reach), None);
    }

    #[test]
    fn choose_sutehai_pai_uses_agent_choice() {
        let request = ChiihouRequest::Sutehai {
            hand: vec![pai("1m"), pai("5p")],
            drawn: None,
        };
        assert_eq!(
            choose_sutehai_pai(&request, &mut PickSecondAgent),
            Ok(pai("5p"))
        );
    }

    #[test]
    fn choose_sutehai_pai_falls_back_when_agent_returns_none() {
        let request = ChiihouRequest::Sutehai {
            hand: vec![pai("1m"), pai("5p")],
            drawn: None,
        };
        assert_eq!(
            choose_sutehai_pai(&request, &mut FixedActionAgent(LegalAction::None)),
            Ok(pai("1m"))
        );
    }

    #[test]
    fn choose_sutehai_pai_falls_back_when_agent_returns_reach() {
        let request = ChiihouRequest::Sutehai {
            hand: vec![pai("1m"), pai("5p")],
            drawn: None,
        };
        assert_eq!(
            choose_sutehai_pai(&request, &mut FixedActionAgent(LegalAction::Reach)),
            Ok(pai("1m"))
        );
    }

    #[test]
    fn choose_sutehai_pai_falls_back_when_agent_returns_illegal_dahai() {
        let request = ChiihouRequest::Sutehai {
            hand: vec![pai("1m"), pai("5p")],
            drawn: None,
        };
        let illegal = LegalAction::Dahai {
            tile: TileId::new(135).unwrap(),
        };
        assert_eq!(
            choose_sutehai_pai(&request, &mut FixedActionAgent(illegal)),
            Ok(pai("1m"))
        );
    }

    #[test]
    fn choose_sutehai_pai_rejects_naku_request() {
        let request = ChiihouRequest::Naku {
            hand: vec![pai("1m"), pai("2m")],
            target: pai("3m"),
            actions: vec![ChiihouNakuAction::Ron],
        };
        assert_eq!(
            choose_sutehai_pai(&request, &mut PickSecondAgent),
            Err(SutehaiDecisionError::NotSutehaiRequest)
        );
    }

    #[test]
    fn choose_sutehai_pai_without_candidates_is_error() {
        let request = ChiihouRequest::Sutehai {
            hand: vec![],
            drawn: None,
        };
        assert_eq!(
            choose_sutehai_pai(&request, &mut PickSecondAgent),
            Err(SutehaiDecisionError::NoLegalDahai)
        );
    }

    #[test]
    fn builds_sutehai_reply_for_request() {
        let request = ChiihouRequest::Sutehai {
            hand: vec![pai("1m"), pai("5p")],
            drawn: None,
        };
        assert_eq!(
            build_sutehai_reply_for_request("npub1server", &request, &mut PickSecondAgent),
            Ok("nostr:npub1server sutehai? sutehai 5p".to_string())
        );
    }

    #[test]
    fn complete_standard_hand_is_complete() {
        assert_eq!(
            is_complete_menzen_tsumo_hand(&complete_standard_hand(), pai("5p")),
            Ok(true)
        );
    }

    #[test]
    fn complete_chiitoitsu_hand_is_complete() {
        assert_eq!(
            is_complete_menzen_tsumo_hand(&complete_chiitoitsu_hand(), pai("7z")),
            Ok(true)
        );
    }

    #[test]
    fn complete_kokushi_hand_is_complete() {
        assert_eq!(
            is_complete_menzen_tsumo_hand(&complete_kokushi_hand(), pai("1m")),
            Ok(true)
        );
    }

    #[test]
    fn tenpai_hand_is_not_complete() {
        assert_eq!(
            is_complete_menzen_tsumo_hand(&tenpai_hand(), pai("2z")),
            Ok(false)
        );
    }

    #[test]
    fn iishanten_hand_is_not_complete() {
        assert_eq!(
            is_complete_menzen_tsumo_hand(&iishanten_hand(), pai("3z")),
            Ok(false)
        );
    }

    #[test]
    fn invalid_hand_tile_count_is_error() {
        for hand in [&complete_standard_hand()[..12], &[][..]] {
            assert_eq!(
                is_complete_menzen_tsumo_hand(hand, pai("5p")),
                Err(SutehaiDecisionError::InvalidHandTileCount(hand.len()))
            );
        }
        let mut fourteen = complete_standard_hand();
        fourteen.push(pai("5p"));
        assert_eq!(
            is_complete_menzen_tsumo_hand(&fourteen, pai("5p")),
            Err(SutehaiDecisionError::InvalidHandTileCount(14))
        );
    }

    #[test]
    fn five_same_tiles_are_rejected() {
        assert_eq!(
            is_complete_menzen_tsumo_hand(&five_same_tiles_hand(), pai("1m")),
            Err(SutehaiDecisionError::TooManySameTiles(pai("1m")))
        );
    }

    #[test]
    fn legal_actions_include_hora_for_complete_hand() {
        let actions =
            legal_actions_from_sutehai_request(&complete_standard_hand(), Some(pai("5p"))).unwrap();
        assert_eq!(actions.first(), Some(&LegalAction::Hora));
        assert!(actions.contains(&dahai("1m")));
        assert!(actions.contains(&dahai("5p")));
    }

    #[test]
    fn legal_actions_include_hora_for_complete_chiitoitsu_hand() {
        let actions =
            legal_actions_from_sutehai_request(&complete_chiitoitsu_hand(), Some(pai("7z")))
                .unwrap();
        assert_eq!(actions.first(), Some(&LegalAction::Hora));
    }

    #[test]
    fn legal_actions_include_hora_for_complete_kokushi_hand() {
        let actions =
            legal_actions_from_sutehai_request(&complete_kokushi_hand(), Some(pai("1m"))).unwrap();
        assert_eq!(actions.first(), Some(&LegalAction::Hora));
    }

    #[test]
    fn legal_actions_have_no_hora_for_tenpai_hand() {
        let actions = legal_actions_from_sutehai_request(&tenpai_hand(), Some(pai("2z"))).unwrap();
        assert!(!actions.contains(&LegalAction::Hora));
        assert!(
            actions
                .iter()
                .all(|action| matches!(action, LegalAction::Dahai { .. }))
        );
    }

    #[test]
    fn legal_actions_have_no_hora_for_iishanten_hand() {
        let actions =
            legal_actions_from_sutehai_request(&iishanten_hand(), Some(pai("3z"))).unwrap();
        assert!(!actions.contains(&LegalAction::Hora));
    }

    #[test]
    fn legal_actions_have_no_hora_without_drawn() {
        let mut fourteen = complete_standard_hand();
        fourteen.push(pai("5p"));
        let actions = legal_actions_from_sutehai_request(&fourteen, None).unwrap();
        assert!(!actions.contains(&LegalAction::Hora));
    }

    #[test]
    fn legal_actions_have_no_hora_for_short_hand_with_drawn() {
        let actions =
            legal_actions_from_sutehai_request(&[pai("1m"), pai("2m"), pai("3m")], Some(pai("1z")))
                .unwrap();
        assert_eq!(
            actions,
            vec![dahai("1m"), dahai("2m"), dahai("3m"), dahai("1z")]
        );
    }

    #[test]
    fn legal_actions_reject_five_same_tiles() {
        assert_eq!(
            legal_actions_from_sutehai_request(&five_same_tiles_hand(), Some(pai("1m"))),
            Err(SutehaiDecisionError::TooManySameTiles(pai("1m")))
        );
    }

    #[test]
    fn legal_actions_deduplicate_dahai_for_complete_hand() {
        let actions =
            legal_actions_from_sutehai_request(&complete_chiitoitsu_hand(), Some(pai("7z")))
                .unwrap();
        let dahai_actions: Vec<&LegalAction> = actions
            .iter()
            .filter(|action| matches!(action, LegalAction::Dahai { .. }))
            .collect();
        assert_eq!(dahai_actions.len(), 7);
    }

    #[test]
    fn choose_sutehai_decision_tsumos_when_agent_picks_hora() {
        assert_eq!(
            choose_sutehai_decision(
                &complete_sutehai_request(),
                &mut FixedActionAgent(LegalAction::Hora)
            ),
            Ok(ChiihouSutehaiDecision::Tsumo)
        );
    }

    #[test]
    fn choose_sutehai_decision_uses_legal_dahai_choice_for_complete_hand() {
        assert_eq!(
            choose_sutehai_decision(
                &complete_sutehai_request(),
                &mut FixedActionAgent(dahai("9m"))
            ),
            Ok(ChiihouSutehaiDecision::Dahai(pai("9m")))
        );
    }

    #[test]
    fn choose_sutehai_decision_falls_back_to_first_dahai_for_none() {
        assert_eq!(
            choose_sutehai_decision(
                &complete_sutehai_request(),
                &mut FixedActionAgent(LegalAction::None)
            ),
            Ok(ChiihouSutehaiDecision::Dahai(pai("1m")))
        );
    }

    #[test]
    fn choose_sutehai_decision_falls_back_to_first_dahai_for_reach() {
        assert_eq!(
            choose_sutehai_decision(
                &complete_sutehai_request(),
                &mut FixedActionAgent(LegalAction::Reach)
            ),
            Ok(ChiihouSutehaiDecision::Dahai(pai("1m")))
        );
    }

    #[test]
    fn choose_sutehai_decision_falls_back_when_hora_is_not_legal() {
        let request = ChiihouRequest::Sutehai {
            hand: tenpai_hand(),
            drawn: Some(pai("2z")),
        };
        assert_eq!(
            choose_sutehai_decision(&request, &mut FixedActionAgent(LegalAction::Hora)),
            Ok(ChiihouSutehaiDecision::Dahai(pai("1m")))
        );
    }

    #[test]
    fn choose_sutehai_decision_falls_back_for_illegal_dahai() {
        assert_eq!(
            choose_sutehai_decision(
                &complete_sutehai_request(),
                &mut FixedActionAgent(dahai("7z"))
            ),
            Ok(ChiihouSutehaiDecision::Dahai(pai("1m")))
        );
    }

    #[test]
    fn choose_sutehai_decision_rejects_naku_request() {
        let request = ChiihouRequest::Naku {
            hand: vec![pai("1m"), pai("2m")],
            target: pai("3m"),
            actions: vec![ChiihouNakuAction::Ron],
        };
        assert_eq!(
            choose_sutehai_decision(&request, &mut FixedActionAgent(LegalAction::Hora)),
            Err(SutehaiDecisionError::NotSutehaiRequest)
        );
    }

    #[test]
    fn choose_sutehai_decision_without_candidates_is_error() {
        let request = ChiihouRequest::Sutehai {
            hand: vec![],
            drawn: None,
        };
        assert_eq!(
            choose_sutehai_decision(&request, &mut FixedActionAgent(LegalAction::Hora)),
            Err(SutehaiDecisionError::NoLegalDahai)
        );
    }

    #[test]
    fn choose_sutehai_decision_calls_agent_once() {
        let mut agent = CountingHoraAgent { calls: 0 };
        assert_eq!(
            choose_sutehai_decision(&complete_sutehai_request(), &mut agent),
            Ok(ChiihouSutehaiDecision::Tsumo)
        );
        assert_eq!(agent.calls, 1);
    }

    #[test]
    fn normal_agent_tsumos_complete_hand() {
        let mut agent = bot_core::NormalAgent;
        assert_eq!(
            choose_sutehai_decision(&complete_sutehai_request(), &mut agent),
            Ok(ChiihouSutehaiDecision::Tsumo)
        );
    }

    #[test]
    fn shanten_agent_tsumos_complete_hand() {
        let mut agent = ShantenAgent;
        assert_eq!(
            choose_sutehai_decision(&complete_sutehai_request(), &mut agent),
            Ok(ChiihouSutehaiDecision::Tsumo)
        );
    }

    #[test]
    fn tsumogiri_agent_discards_drawn_tile_even_for_complete_hand() {
        let mut agent = bot_core::TsumogiriAgent;
        assert_eq!(
            choose_sutehai_decision(&complete_sutehai_request(), &mut agent),
            Ok(ChiihouSutehaiDecision::Dahai(pai("5p")))
        );
    }

    #[test]
    fn builds_tsumo_reply_when_agent_picks_hora() {
        assert_eq!(
            build_sutehai_reply_for_request(
                "npub1server",
                &complete_sutehai_request(),
                &mut FixedActionAgent(LegalAction::Hora)
            ),
            Ok("nostr:npub1server sutehai? tsumo".to_string())
        );
    }

    #[test]
    fn builds_dahai_reply_for_complete_hand_when_agent_discards() {
        assert_eq!(
            build_sutehai_reply_for_request(
                "npub1server",
                &complete_sutehai_request(),
                &mut bot_core::TsumogiriAgent
            ),
            Ok("nostr:npub1server sutehai? sutehai 5p".to_string())
        );
    }

    fn naku_request(actions: Vec<ChiihouNakuAction>) -> ChiihouRequest {
        ChiihouRequest::Naku {
            hand: vec![pai("1m"), pai("2m"), pai("3m")],
            target: pai("4m"),
            actions,
        }
    }

    #[test]
    fn game_context_from_naku_request_holds_hand_without_drawn() {
        let context = game_context_from_naku_request(&[pai("1m"), pai("5p")]);
        assert_eq!(
            context.hand_tiles(),
            &[
                temporary_tile_id_from_chiihou_pai(pai("1m")),
                temporary_tile_id_from_chiihou_pai(pai("5p")),
            ]
        );
        assert_eq!(context.drawn_tile(), None);
    }

    #[test]
    fn legal_actions_with_ron_are_hora_and_none() {
        assert_eq!(
            legal_actions_from_naku_actions(&[ChiihouNakuAction::Ron]),
            vec![LegalAction::Hora, LegalAction::None]
        );
        assert_eq!(
            legal_actions_from_naku_actions(&[
                ChiihouNakuAction::Pon,
                ChiihouNakuAction::Ron,
                ChiihouNakuAction::Chi,
            ]),
            vec![LegalAction::Hora, LegalAction::None]
        );
    }

    #[test]
    fn legal_actions_without_ron_are_none_only() {
        assert_eq!(
            legal_actions_from_naku_actions(&[
                ChiihouNakuAction::Kan,
                ChiihouNakuAction::Pon,
                ChiihouNakuAction::Chi,
            ]),
            vec![LegalAction::None]
        );
        assert_eq!(
            legal_actions_from_naku_actions(&[]),
            vec![LegalAction::None]
        );
    }

    #[test]
    fn choose_naku_decision_rons_when_agent_picks_hora() {
        assert_eq!(
            choose_naku_decision(
                &naku_request(vec![ChiihouNakuAction::Ron]),
                &mut FixedActionAgent(LegalAction::Hora)
            ),
            Ok(ChiihouNakuDecision::Ron)
        );
    }

    #[test]
    fn choose_naku_decision_declines_hora_without_ron_offer() {
        assert_eq!(
            choose_naku_decision(
                &naku_request(vec![ChiihouNakuAction::Pon, ChiihouNakuAction::Chi]),
                &mut FixedActionAgent(LegalAction::Hora)
            ),
            Ok(ChiihouNakuDecision::No)
        );
    }

    #[test]
    fn choose_naku_decision_declines_when_agent_picks_none() {
        assert_eq!(
            choose_naku_decision(
                &naku_request(vec![ChiihouNakuAction::Ron]),
                &mut FixedActionAgent(LegalAction::None)
            ),
            Ok(ChiihouNakuDecision::No)
        );
    }

    #[test]
    fn choose_naku_decision_falls_back_to_no_for_illegal_actions() {
        let illegal_actions = [
            LegalAction::Dahai {
                tile: temporary_tile_id_from_chiihou_pai(pai("4m")),
            },
            LegalAction::Reach,
            LegalAction::Chi {
                tile: temporary_tile_id_from_chiihou_pai(pai("4m")),
                consumed: vec![],
            },
            LegalAction::Pon {
                tile: temporary_tile_id_from_chiihou_pai(pai("4m")),
                consumed: vec![],
            },
            LegalAction::Daiminkan {
                tile: temporary_tile_id_from_chiihou_pai(pai("4m")),
                consumed: vec![],
            },
            LegalAction::Ankan { consumed: vec![] },
            LegalAction::Kakan {
                tile: temporary_tile_id_from_chiihou_pai(pai("4m")),
                consumed: vec![],
            },
            LegalAction::Ryukyoku,
        ];
        for action in illegal_actions {
            assert_eq!(
                choose_naku_decision(
                    &naku_request(vec![ChiihouNakuAction::Ron]),
                    &mut FixedActionAgent(action.clone())
                ),
                Ok(ChiihouNakuDecision::No),
                "action: {action:?}"
            );
        }
    }

    #[test]
    fn choose_naku_decision_rejects_sutehai_request() {
        let request = ChiihouRequest::Sutehai {
            hand: vec![pai("1m")],
            drawn: None,
        };
        assert_eq!(
            choose_naku_decision(&request, &mut FixedActionAgent(LegalAction::Hora)),
            Err(NakuDecisionError::NotNakuRequest)
        );
    }

    #[test]
    fn normal_agent_rons_when_ron_is_offered() {
        let mut agent = bot_core::NormalAgent;
        assert_eq!(
            choose_naku_decision(&naku_request(vec![ChiihouNakuAction::Ron]), &mut agent),
            Ok(ChiihouNakuDecision::Ron)
        );
    }

    #[test]
    fn shanten_agent_rons_when_ron_is_offered() {
        let mut agent = ShantenAgent;
        assert_eq!(
            choose_naku_decision(&naku_request(vec![ChiihouNakuAction::Ron]), &mut agent),
            Ok(ChiihouNakuDecision::Ron)
        );
    }

    #[test]
    fn tsumogiri_agent_declines_ron() {
        let mut agent = bot_core::TsumogiriAgent;
        assert_eq!(
            choose_naku_decision(&naku_request(vec![ChiihouNakuAction::Ron]), &mut agent),
            Ok(ChiihouNakuDecision::No)
        );
    }

    #[test]
    fn builds_naku_ron_reply_for_request() {
        assert_eq!(
            build_naku_reply_for_request(
                "npub1server",
                &naku_request(vec![ChiihouNakuAction::Ron]),
                &mut FixedActionAgent(LegalAction::Hora)
            ),
            Ok("nostr:npub1server naku? ron".to_string())
        );
    }

    #[test]
    fn builds_naku_no_reply_for_request() {
        assert_eq!(
            build_naku_reply_for_request(
                "npub1server",
                &naku_request(vec![ChiihouNakuAction::Kan]),
                &mut FixedActionAgent(LegalAction::Hora)
            ),
            Ok("nostr:npub1server naku? no".to_string())
        );
    }

    fn snapshot_for_tests() -> ChiihouTableSnapshot {
        use crate::lifecycle::ChiihouWind;
        let mut discards: [Vec<ChiihouPai>; 4] = Default::default();
        discards[1] = vec![pai("7z"), pai("1z")];
        discards[3] = vec![pai("9s")];
        ChiihouTableSnapshot {
            dora_indicators: vec![pai("5p"), pai("5p")],
            round_wind: Some(ChiihouWind::South),
            seat_wind: Some(ChiihouWind::West),
            player_id: Some(0),
            oya: Some(2),
            discards,
            reached: [false, true, false, false],
        }
    }

    fn tile_of(s: &str) -> TileId {
        temporary_tile_id_from_chiihou_pai(pai(s))
    }

    #[test]
    fn sutehai_context_with_state_uses_request_hand_and_drawn() {
        let context = game_context_from_sutehai_request_with_state(
            &[pai("1m"), pai("5p")],
            Some(pai("7z")),
            &snapshot_for_tests(),
        );
        assert_eq!(context.hand_tiles(), &[tile_of("1m"), tile_of("5p")]);
        assert_eq!(context.drawn_tile(), Some(tile_of("7z")));
    }

    #[test]
    fn context_with_state_sets_table_fields() {
        let context =
            game_context_from_sutehai_request_with_state(&[pai("1m")], None, &snapshot_for_tests());
        assert_eq!(context.dora_indicators(), &[tile_of("5p"), tile_of("5p")]);
        assert_eq!(
            context.round_wind(),
            Some(tile_type_from_chiihou_wind(
                crate::lifecycle::ChiihouWind::South
            ))
        );
        assert_eq!(
            context.seat_wind(),
            Some(tile_type_from_chiihou_wind(
                crate::lifecycle::ChiihouWind::West
            ))
        );
        assert_eq!(context.player_id(), Some(0));
        assert_eq!(context.oya(), Some(2));
        assert_eq!(context.discards()[1], vec![tile_of("7z"), tile_of("1z")]);
        assert_eq!(context.discards()[3], vec![tile_of("9s")]);
        assert!(context.discards()[0].is_empty());
        assert_eq!(context.reached(), &[false, true, false, false]);
    }

    #[test]
    fn context_with_state_visible_tiles_hold_hand_drawn_dora_and_discards() {
        let context = game_context_from_sutehai_request_with_state(
            &[pai("1m"), pai("5p")],
            Some(pai("2m")),
            &snapshot_for_tests(),
        );
        assert_eq!(
            context.visible_tiles(),
            &[
                tile_of("1m"),
                tile_of("5p"),
                tile_of("2m"),
                tile_of("5p"),
                tile_of("5p"),
                tile_of("7z"),
                tile_of("1z"),
                tile_of("9s"),
            ]
        );
    }

    #[test]
    fn naku_context_with_state_has_no_drawn_tile() {
        let context = game_context_from_naku_request_with_state(
            &[pai("1m"), pai("5p")],
            &snapshot_for_tests(),
        );
        assert_eq!(context.drawn_tile(), None);
        assert_eq!(context.hand_tiles(), &[tile_of("1m"), tile_of("5p")]);
        assert_eq!(context.reached(), &[false, true, false, false]);
    }

    #[test]
    fn context_with_empty_state_matches_request_only_visibility() {
        let context = game_context_from_sutehai_request_with_state(
            &[pai("1m")],
            Some(pai("2m")),
            &ChiihouTableSnapshot::default(),
        );
        assert_eq!(context.visible_tiles(), &[tile_of("1m"), tile_of("2m")]);
        assert!(context.dora_indicators().is_empty());
        assert_eq!(context.round_wind(), None);
        assert_eq!(context.seat_wind(), None);
        assert_eq!(context.player_id(), None);
        assert_eq!(context.oya(), None);
    }

    #[test]
    fn stateless_context_is_unchanged_by_state_helpers() {
        let context = game_context_from_sutehai_request(&[pai("1m")], Some(pai("2m")));
        assert!(context.visible_tiles().is_empty());
        assert!(context.dora_indicators().is_empty());
    }

    #[test]
    fn choose_sutehai_pai_with_state_uses_agent_choice() {
        let request = ChiihouRequest::Sutehai {
            hand: vec![pai("1m"), pai("5p")],
            drawn: None,
        };
        assert_eq!(
            choose_sutehai_pai_with_state(&request, &snapshot_for_tests(), &mut PickSecondAgent),
            Ok(pai("5p"))
        );
    }

    #[test]
    fn builds_sutehai_reply_with_state() {
        let request = ChiihouRequest::Sutehai {
            hand: vec![pai("1m"), pai("5p")],
            drawn: None,
        };
        assert_eq!(
            build_sutehai_reply_for_request_with_state(
                "npub1server",
                &request,
                &snapshot_for_tests(),
                &mut PickSecondAgent
            ),
            Ok("nostr:npub1server sutehai? sutehai 5p".to_string())
        );
    }

    #[test]
    fn builds_naku_reply_with_state() {
        assert_eq!(
            build_naku_reply_for_request_with_state(
                "npub1server",
                &naku_request(vec![ChiihouNakuAction::Ron]),
                &snapshot_for_tests(),
                &mut FixedActionAgent(LegalAction::Hora)
            ),
            Ok("nostr:npub1server naku? ron".to_string())
        );
    }

    struct RecordingHoraAgent {
        context: Option<GameContext>,
    }

    impl Agent for RecordingHoraAgent {
        fn act(&mut self, ctx: &GameContext, _legal_actions: &[LegalAction]) -> LegalAction {
            self.context = Some(ctx.clone());
            LegalAction::Hora
        }
    }

    #[test]
    fn choose_sutehai_decision_with_state_tsumos_when_agent_picks_hora() {
        assert_eq!(
            choose_sutehai_decision_with_state(
                &complete_sutehai_request(),
                &snapshot_for_tests(),
                &mut FixedActionAgent(LegalAction::Hora)
            ),
            Ok(ChiihouSutehaiDecision::Tsumo)
        );
    }

    #[test]
    fn tsumo_decision_with_state_passes_table_state_to_agent() {
        let mut agent = RecordingHoraAgent { context: None };
        assert_eq!(
            choose_sutehai_decision_with_state(
                &complete_sutehai_request(),
                &snapshot_for_tests(),
                &mut agent
            ),
            Ok(ChiihouSutehaiDecision::Tsumo)
        );
        let context = agent.context.unwrap();
        let hand_tiles: Vec<TileId> = complete_standard_hand()
            .iter()
            .map(|&pai| temporary_tile_id_from_chiihou_pai(pai))
            .collect();
        assert_eq!(context.hand_tiles(), &hand_tiles[..]);
        assert_eq!(context.drawn_tile(), Some(tile_of("5p")));
        assert_eq!(context.dora_indicators(), &[tile_of("5p"), tile_of("5p")]);
        assert_eq!(
            context.round_wind(),
            Some(tile_type_from_chiihou_wind(
                crate::lifecycle::ChiihouWind::South
            ))
        );
        assert_eq!(
            context.seat_wind(),
            Some(tile_type_from_chiihou_wind(
                crate::lifecycle::ChiihouWind::West
            ))
        );
        assert_eq!(context.player_id(), Some(0));
        assert_eq!(context.oya(), Some(2));
        assert_eq!(context.discards()[1], vec![tile_of("7z"), tile_of("1z")]);
        assert_eq!(context.discards()[3], vec![tile_of("9s")]);
        assert_eq!(context.reached(), &[false, true, false, false]);
        let mut visible = hand_tiles;
        visible.push(tile_of("5p"));
        visible.extend([tile_of("5p"), tile_of("5p")]);
        visible.extend([tile_of("7z"), tile_of("1z"), tile_of("9s")]);
        assert_eq!(context.visible_tiles(), &visible[..]);
    }

    #[test]
    fn builds_tsumo_reply_with_state() {
        assert_eq!(
            build_sutehai_reply_for_request_with_state(
                "npub1server",
                &complete_sutehai_request(),
                &snapshot_for_tests(),
                &mut FixedActionAgent(LegalAction::Hora)
            ),
            Ok("nostr:npub1server sutehai? tsumo".to_string())
        );
    }

    #[test]
    fn builds_sutehai_reply_with_shanten_agent() {
        let mut agent = ShantenAgent;
        let request = ChiihouRequest::Sutehai {
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
                pai("1z"),
            ],
            drawn: Some(pai("2z")),
        };
        let reply = build_sutehai_reply_for_request("npub1server", &request, &mut agent).unwrap();
        assert!(reply.starts_with("nostr:npub1server sutehai? sutehai "));
    }
}
