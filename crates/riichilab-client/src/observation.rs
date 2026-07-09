use bot_core::GameContext;
use bot_logic::{TileId, TileType};
use riichienv_core::observation::Observation;

use crate::convert::temporary_tile_id_from_observation_tile;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObservationPayload {
    base64: String,
}

impl ObservationPayload {
    pub fn new(base64: impl Into<String>) -> Self {
        Self {
            base64: base64.into(),
        }
    }

    pub fn as_base64(&self) -> &str {
        &self.base64
    }

    pub fn decode_4p(&self) -> Result<DecodedObservation, ObservationError> {
        let observation = Observation::deserialize_from_base64(&self.base64)
            .map_err(|e| ObservationError::Decode(e.to_string()))?;
        let hand_tiles: Vec<TileId> = observation
            .hands
            .get(usize::from(observation.player_id))
            .map(|hand| {
                hand.iter()
                    .filter_map(|&raw| u8::try_from(raw).ok())
                    .filter_map(temporary_tile_id_from_observation_tile)
                    .collect()
            })
            .unwrap_or_default();
        let dora_indicators: Vec<TileId> = observation
            .dora_indicators
            .iter()
            .filter_map(|&raw| u8::try_from(raw).ok())
            .filter_map(temporary_tile_id_from_observation_tile)
            .collect();
        let mut visible_tiles = hand_tiles.clone();
        visible_tiles.extend(dora_indicators.iter().copied());
        for player_discards in &observation.discards {
            visible_tiles.extend(
                player_discards
                    .iter()
                    .filter_map(|&raw| u8::try_from(raw).ok())
                    .filter_map(temporary_tile_id_from_observation_tile),
            );
        }
        Ok(DecodedObservation {
            player_id: observation.player_id,
            drawn_tile: observation
                .drawn_tile
                .and_then(temporary_tile_id_from_observation_tile),
            hand_tiles,
            dora_indicators,
            round_wind: TileType::wind_from_seat_index(observation.round_wind),
            seat_wind: seat_wind_from(observation.player_id, observation.oya),
            visible_tiles,
        })
    }
}

fn seat_wind_from(player_id: u8, oya: u8) -> Option<TileType> {
    if player_id >= 4 || oya >= 4 {
        return None;
    }

    let index = (player_id + 4 - oya) % 4;
    TileType::wind_from_seat_index(index)
}

#[derive(Debug, thiserror::Error)]
pub enum ObservationError {
    #[error("failed to decode observation: {0}")]
    Decode(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecodedObservation {
    pub player_id: u8,
    pub drawn_tile: Option<TileId>,
    pub hand_tiles: Vec<TileId>,
    pub dora_indicators: Vec<TileId>,
    pub round_wind: Option<TileType>,
    pub seat_wind: Option<TileType>,
    pub visible_tiles: Vec<TileId>,
}

pub(crate) fn game_context_from_decoded_observation(decoded: &DecodedObservation) -> GameContext {
    GameContext::from_parts_with_visible_tiles(
        decoded.drawn_tile,
        decoded.hand_tiles.clone(),
        decoded.dora_indicators.clone(),
        decoded.round_wind,
        decoded.seat_wind,
        decoded.visible_tiles.clone(),
    )
}

#[cfg(test)]
pub(crate) fn fixture_base64(player_id: u8, drawn_tile: Option<u8>, hand: Vec<u8>) -> String {
    fixture_base64_with_dora(player_id, drawn_tile, hand, vec![])
}

#[cfg(test)]
pub(crate) fn fixture_base64_with_dora(
    player_id: u8,
    drawn_tile: Option<u8>,
    hand: Vec<u8>,
    dora_indicators: Vec<u8>,
) -> String {
    fixture_base64_with_winds(player_id, drawn_tile, hand, dora_indicators, 0, 0)
}

#[cfg(test)]
pub(crate) fn fixture_base64_with_winds(
    player_id: u8,
    drawn_tile: Option<u8>,
    hand: Vec<u8>,
    dora_indicators: Vec<u8>,
    round_wind: u8,
    oya: u8,
) -> String {
    let mut hands: [Vec<u8>; 4] = Default::default();
    hands[usize::from(player_id)] = hand;
    let observation = Observation::new(
        player_id,
        hands,
        Default::default(),
        Default::default(),
        dora_indicators,
        [25000; 4],
        [false; 4],
        vec![],
        vec![],
        0,
        0,
        round_wind,
        oya,
        0,
        vec![],
        false,
        [None; 4],
        [None; 4],
        None,
        drawn_tile,
    );
    observation.serialize_to_base64().unwrap()
}

#[cfg(test)]
pub(crate) fn fixture_base64_with_discards(
    player_id: u8,
    drawn_tile: Option<u8>,
    hand: Vec<u8>,
    dora_indicators: Vec<u8>,
    discards: [Vec<u8>; 4],
) -> String {
    let mut hands: [Vec<u8>; 4] = Default::default();
    hands[usize::from(player_id)] = hand;
    let observation = Observation::new(
        player_id,
        hands,
        Default::default(),
        discards,
        dora_indicators,
        [25000; 4],
        [false; 4],
        vec![],
        vec![],
        0,
        0,
        0,
        0,
        0,
        vec![],
        false,
        [None; 4],
        [None; 4],
        None,
        drawn_tile,
    );
    observation.serialize_to_base64().unwrap()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_keeps_base64_string() {
        assert_eq!(ObservationPayload::new("abc").as_base64(), "abc");
    }

    #[test]
    fn clone_and_equality() {
        let payload = ObservationPayload::new("abc");
        assert_eq!(payload.clone(), payload);
        assert_ne!(payload, ObservationPayload::new("xyz"));
    }

    #[test]
    fn decode_4p_roundtrip_returns_player_id() {
        let payload = ObservationPayload::new(fixture_base64(2, None, vec![]));
        let decoded = payload.decode_4p().unwrap();
        assert_eq!(decoded.player_id, 2);
    }

    #[test]
    fn decode_4p_without_drawn_tile_returns_none() {
        let payload = ObservationPayload::new(fixture_base64(0, None, vec![]));
        let decoded = payload.decode_4p().unwrap();
        assert_eq!(decoded.drawn_tile, None);
    }

    #[test]
    fn decode_4p_returns_drawn_tile() {
        let payload = ObservationPayload::new(fixture_base64(0, Some(56), vec![]));
        let decoded = payload.decode_4p().unwrap();
        assert_eq!(decoded.drawn_tile, TileId::new(56));
    }

    #[test]
    fn decode_4p_normalizes_drawn_tile_to_temporary_tile_id() {
        for (raw, expected) in [(59, 56), (16, 16), (19, 17)] {
            let payload = ObservationPayload::new(fixture_base64(0, Some(raw), vec![]));
            let decoded = payload.decode_4p().unwrap();
            assert_eq!(decoded.drawn_tile, TileId::new(expected), "raw: {raw}");
        }
    }

    #[test]
    fn decode_4p_out_of_range_drawn_tile_becomes_none() {
        let payload = ObservationPayload::new(fixture_base64(0, Some(200), vec![]));
        let decoded = payload.decode_4p().unwrap();
        assert_eq!(decoded.drawn_tile, None);
    }

    #[test]
    fn decode_4p_returns_hand_tiles() {
        let payload = ObservationPayload::new(fixture_base64(1, None, vec![0, 16, 104]));
        let decoded = payload.decode_4p().unwrap();
        assert_eq!(
            decoded.hand_tiles,
            vec![
                TileId::new(0).unwrap(),
                TileId::new(16).unwrap(),
                TileId::new(104).unwrap(),
            ]
        );
    }

    #[test]
    fn decode_4p_with_empty_hand_returns_empty_hand_tiles() {
        let payload = ObservationPayload::new(fixture_base64(0, None, vec![]));
        let decoded = payload.decode_4p().unwrap();
        assert!(decoded.hand_tiles.is_empty());
    }

    #[test]
    fn decode_4p_normalizes_hand_tiles_to_temporary_tile_id() {
        let payload = ObservationPayload::new(fixture_base64(0, None, vec![59, 16, 19]));
        let decoded = payload.decode_4p().unwrap();
        assert_eq!(
            decoded.hand_tiles,
            vec![
                TileId::new(56).unwrap(),
                TileId::new(16).unwrap(),
                TileId::new(17).unwrap(),
            ]
        );
    }

    #[test]
    fn decode_4p_skips_out_of_range_hand_tiles() {
        let payload = ObservationPayload::new(fixture_base64(0, None, vec![0, 200, 136, 104]));
        let decoded = payload.decode_4p().unwrap();
        assert_eq!(
            decoded.hand_tiles,
            vec![TileId::new(0).unwrap(), TileId::new(104).unwrap()]
        );
    }

    #[test]
    fn decode_4p_returns_both_drawn_tile_and_hand_tiles() {
        let payload = ObservationPayload::new(fixture_base64(0, Some(59), vec![0, 16]));
        let decoded = payload.decode_4p().unwrap();
        assert_eq!(decoded.drawn_tile, TileId::new(56));
        assert_eq!(
            decoded.hand_tiles,
            vec![TileId::new(0).unwrap(), TileId::new(16).unwrap()]
        );
    }

    #[test]
    fn decode_4p_without_dora_indicators_returns_empty() {
        let payload = ObservationPayload::new(fixture_base64(0, None, vec![]));
        let decoded = payload.decode_4p().unwrap();
        assert!(decoded.dora_indicators.is_empty());
    }

    #[test]
    fn decode_4p_returns_dora_indicators() {
        let payload =
            ObservationPayload::new(fixture_base64_with_dora(0, None, vec![], vec![0, 104]));
        let decoded = payload.decode_4p().unwrap();
        assert_eq!(
            decoded.dora_indicators,
            vec![TileId::new(0).unwrap(), TileId::new(104).unwrap()]
        );
    }

    #[test]
    fn decode_4p_normalizes_dora_indicators_to_temporary_tile_id() {
        let payload =
            ObservationPayload::new(fixture_base64_with_dora(0, None, vec![], vec![59, 16, 19]));
        let decoded = payload.decode_4p().unwrap();
        assert_eq!(
            decoded.dora_indicators,
            vec![
                TileId::new(56).unwrap(),
                TileId::new(16).unwrap(),
                TileId::new(17).unwrap(),
            ]
        );
    }

    #[test]
    fn decode_4p_skips_out_of_range_dora_indicators() {
        let payload = ObservationPayload::new(fixture_base64_with_dora(
            0,
            None,
            vec![],
            vec![0, 200, 136, 104],
        ));
        let decoded = payload.decode_4p().unwrap();
        assert_eq!(
            decoded.dora_indicators,
            vec![TileId::new(0).unwrap(), TileId::new(104).unwrap()]
        );
    }

    #[test]
    fn decode_4p_rejects_invalid_base64() {
        let payload = ObservationPayload::new("not-valid-base64!!");
        assert!(matches!(
            payload.decode_4p(),
            Err(ObservationError::Decode(_))
        ));
    }

    #[test]
    fn decode_4p_rejects_non_observation_json() {
        let payload = ObservationPayload::new("eyJmb28iOjF9");
        assert!(matches!(
            payload.decode_4p(),
            Err(ObservationError::Decode(_))
        ));
    }

    fn wind(value: u8) -> TileType {
        TileType::new(value).unwrap()
    }

    #[test]
    fn decode_4p_returns_round_wind_from_observation() {
        for (round_wind, expected) in [(0, 27), (1, 28), (2, 29), (3, 30)] {
            let payload = ObservationPayload::new(fixture_base64_with_winds(
                0,
                None,
                vec![],
                vec![],
                round_wind,
                0,
            ));
            let decoded = payload.decode_4p().unwrap();
            assert_eq!(
                decoded.round_wind,
                Some(wind(expected)),
                "round_wind: {round_wind}"
            );
        }
    }

    #[test]
    fn decode_4p_seat_wind_is_east_when_player_is_oya() {
        let payload =
            ObservationPayload::new(fixture_base64_with_winds(2, None, vec![], vec![], 0, 2));
        let decoded = payload.decode_4p().unwrap();
        assert_eq!(decoded.seat_wind, Some(wind(27)));
    }

    #[test]
    fn decode_4p_seat_wind_is_south_for_oya_shimocha() {
        let payload =
            ObservationPayload::new(fixture_base64_with_winds(2, None, vec![], vec![], 0, 1));
        let decoded = payload.decode_4p().unwrap();
        assert_eq!(decoded.seat_wind, Some(wind(28)));
    }

    #[test]
    fn decode_4p_seat_wind_is_west_for_oya_toimen() {
        let payload =
            ObservationPayload::new(fixture_base64_with_winds(3, None, vec![], vec![], 0, 1));
        let decoded = payload.decode_4p().unwrap();
        assert_eq!(decoded.seat_wind, Some(wind(29)));
    }

    #[test]
    fn decode_4p_seat_wind_is_north_for_oya_kamicha() {
        let payload =
            ObservationPayload::new(fixture_base64_with_winds(0, None, vec![], vec![], 0, 1));
        let decoded = payload.decode_4p().unwrap();
        assert_eq!(decoded.seat_wind, Some(wind(30)));
    }

    #[test]
    fn seat_wind_from_covers_all_seats() {
        assert_eq!(seat_wind_from(0, 0), Some(wind(27)));
        assert_eq!(seat_wind_from(1, 0), Some(wind(28)));
        assert_eq!(seat_wind_from(2, 0), Some(wind(29)));
        assert_eq!(seat_wind_from(3, 0), Some(wind(30)));
        assert_eq!(seat_wind_from(0, 3), Some(wind(28)));
        assert_eq!(seat_wind_from(1, 3), Some(wind(29)));
    }

    #[test]
    fn seat_wind_from_rejects_out_of_range_inputs() {
        assert_eq!(seat_wind_from(4, 0), None);
        assert_eq!(seat_wind_from(0, 4), None);
        assert_eq!(seat_wind_from(255, 0), None);
        assert_eq!(seat_wind_from(0, 255), None);
    }

    #[test]
    fn decode_4p_default_fixture_has_east_winds() {
        let payload = ObservationPayload::new(fixture_base64(0, None, vec![]));
        let decoded = payload.decode_4p().unwrap();
        assert_eq!(decoded.round_wind, Some(wind(27)));
        assert_eq!(decoded.seat_wind, Some(wind(27)));
    }

    #[test]
    fn decode_4p_visible_tiles_include_hand_tiles() {
        let payload = ObservationPayload::new(fixture_base64(1, None, vec![0, 16, 104]));
        let decoded = payload.decode_4p().unwrap();
        for tile in [
            TileId::new(0).unwrap(),
            TileId::new(16).unwrap(),
            TileId::new(104).unwrap(),
        ] {
            assert!(decoded.visible_tiles.contains(&tile), "missing {tile:?}");
        }
    }

    #[test]
    fn decode_4p_visible_tiles_include_drawn_tile_via_hand() {
        let payload = ObservationPayload::new(fixture_base64(0, Some(16), vec![0, 16, 104]));
        let decoded = payload.decode_4p().unwrap();
        assert_eq!(decoded.drawn_tile, TileId::new(16));
        assert!(decoded.visible_tiles.contains(&TileId::new(16).unwrap()));
    }

    #[test]
    fn decode_4p_visible_tiles_do_not_double_count_drawn_tile() {
        let payload = ObservationPayload::new(fixture_base64(0, Some(16), vec![0, 16, 104]));
        let decoded = payload.decode_4p().unwrap();
        let count = decoded
            .visible_tiles
            .iter()
            .filter(|&&tile| tile == TileId::new(16).unwrap())
            .count();
        assert_eq!(count, 1);
    }

    #[test]
    fn decode_4p_visible_tiles_include_dora_indicators() {
        let payload =
            ObservationPayload::new(fixture_base64_with_dora(0, None, vec![], vec![4, 20]));
        let decoded = payload.decode_4p().unwrap();
        for tile in [TileId::new(4).unwrap(), TileId::new(20).unwrap()] {
            assert!(decoded.visible_tiles.contains(&tile), "missing {tile:?}");
        }
    }

    #[test]
    fn decode_4p_visible_tiles_include_discards_of_all_players() {
        let discards = [vec![0], vec![16], vec![104], vec![132]];
        let payload = ObservationPayload::new(fixture_base64_with_discards(
            0,
            None,
            vec![],
            vec![],
            discards,
        ));
        let decoded = payload.decode_4p().unwrap();
        for tile in [
            TileId::new(0).unwrap(),
            TileId::new(16).unwrap(),
            TileId::new(104).unwrap(),
            TileId::new(132).unwrap(),
        ] {
            assert!(decoded.visible_tiles.contains(&tile), "missing {tile:?}");
        }
    }

    #[test]
    fn decode_4p_visible_tiles_normalize_discards_to_temporary_tile_id() {
        let discards = [vec![59], vec![19], vec![], vec![]];
        let payload = ObservationPayload::new(fixture_base64_with_discards(
            0,
            None,
            vec![],
            vec![],
            discards,
        ));
        let decoded = payload.decode_4p().unwrap();
        assert!(decoded.visible_tiles.contains(&TileId::new(56).unwrap()));
        assert!(decoded.visible_tiles.contains(&TileId::new(17).unwrap()));
    }

    #[test]
    fn decode_4p_visible_tiles_skip_out_of_range_tiles() {
        let discards = [vec![0, 200, 136], vec![], vec![], vec![]];
        let payload = ObservationPayload::new(fixture_base64_with_discards(
            0,
            None,
            vec![0, 200, 136, 104],
            vec![0, 200],
            discards,
        ));
        let decoded = payload.decode_4p().unwrap();
        assert!(!decoded.visible_tiles.iter().any(|tile| tile.raw() >= 136));
    }

    #[test]
    fn decode_4p_visible_tiles_preserve_duplicate_count_across_sources() {
        let discards = [vec![1], vec![2], vec![3], vec![]];
        let payload = ObservationPayload::new(fixture_base64_with_discards(
            0,
            None,
            vec![0],
            vec![],
            discards,
        ));
        let decoded = payload.decode_4p().unwrap();
        let count = decoded
            .visible_tiles
            .iter()
            .filter(|&&tile| tile == TileId::new(0).unwrap())
            .count();
        assert_eq!(count, 4);
    }

    #[test]
    fn decode_4p_visible_tiles_empty_when_nothing_visible() {
        let payload = ObservationPayload::new(fixture_base64(0, None, vec![]));
        let decoded = payload.decode_4p().unwrap();
        assert!(decoded.visible_tiles.is_empty());
    }

    mod game_context_helper {
        use super::*;

        #[test]
        fn drawn_tile_becomes_context_drawn_tile() {
            let tile = TileId::new(56).unwrap();
            let decoded = DecodedObservation {
                player_id: 0,
                drawn_tile: Some(tile),
                hand_tiles: vec![],
                dora_indicators: vec![],
                round_wind: None,
                seat_wind: None,
                visible_tiles: vec![],
            };
            assert_eq!(
                game_context_from_decoded_observation(&decoded),
                GameContext::with_drawn_tile(tile)
            );
        }

        #[test]
        fn no_drawn_tile_becomes_default_context() {
            let decoded = DecodedObservation {
                player_id: 3,
                drawn_tile: None,
                hand_tiles: vec![],
                dora_indicators: vec![],
                round_wind: None,
                seat_wind: None,
                visible_tiles: vec![],
            };
            assert_eq!(
                game_context_from_decoded_observation(&decoded),
                GameContext::default()
            );
        }

        #[test]
        fn drawn_tile_and_hand_tiles_become_context_parts() {
            let tile = TileId::new(56).unwrap();
            let hand_tiles = vec![TileId::new(0).unwrap(), TileId::new(16).unwrap()];
            let decoded = DecodedObservation {
                player_id: 0,
                drawn_tile: Some(tile),
                hand_tiles: hand_tiles.clone(),
                dora_indicators: vec![],
                round_wind: None,
                seat_wind: None,
                visible_tiles: vec![],
            };
            assert_eq!(
                game_context_from_decoded_observation(&decoded),
                GameContext::from_parts(Some(tile), hand_tiles)
            );
        }

        #[test]
        fn hand_tiles_are_kept_without_drawn_tile() {
            let hand_tiles = vec![TileId::new(104).unwrap()];
            let decoded = DecodedObservation {
                player_id: 1,
                drawn_tile: None,
                hand_tiles: hand_tiles.clone(),
                dora_indicators: vec![],
                round_wind: None,
                seat_wind: None,
                visible_tiles: vec![],
            };
            let context = game_context_from_decoded_observation(&decoded);
            assert_eq!(context.drawn_tile(), None);
            assert_eq!(context.hand_tiles(), hand_tiles.as_slice());
        }

        #[test]
        fn dora_indicators_are_passed_to_context() {
            let dora_indicators = vec![TileId::new(4).unwrap(), TileId::new(20).unwrap()];
            let decoded = DecodedObservation {
                player_id: 2,
                drawn_tile: None,
                hand_tiles: vec![],
                dora_indicators: dora_indicators.clone(),
                round_wind: None,
                seat_wind: None,
                visible_tiles: vec![],
            };
            let context = game_context_from_decoded_observation(&decoded);
            assert_eq!(context.dora_indicators(), dora_indicators.as_slice());
        }

        #[test]
        fn empty_dora_indicators_become_empty_context_dora() {
            let decoded = DecodedObservation {
                player_id: 0,
                drawn_tile: None,
                hand_tiles: vec![],
                dora_indicators: vec![],
                round_wind: None,
                seat_wind: None,
                visible_tiles: vec![],
            };
            let context = game_context_from_decoded_observation(&decoded);
            assert!(context.dora_indicators().is_empty());
        }

        #[test]
        fn winds_are_passed_to_context() {
            let decoded = DecodedObservation {
                player_id: 0,
                drawn_tile: None,
                hand_tiles: vec![],
                dora_indicators: vec![],
                round_wind: Some(wind(27)),
                seat_wind: Some(wind(28)),
                visible_tiles: vec![],
            };
            let context = game_context_from_decoded_observation(&decoded);
            assert_eq!(context.round_wind(), Some(wind(27)));
            assert_eq!(context.seat_wind(), Some(wind(28)));
        }

        #[test]
        fn absent_winds_become_none_in_context() {
            let decoded = DecodedObservation {
                player_id: 0,
                drawn_tile: None,
                hand_tiles: vec![],
                dora_indicators: vec![],
                round_wind: None,
                seat_wind: None,
                visible_tiles: vec![],
            };
            let context = game_context_from_decoded_observation(&decoded);
            assert_eq!(context.round_wind(), None);
            assert_eq!(context.seat_wind(), None);
        }

        #[test]
        fn visible_tiles_are_passed_to_context() {
            let visible_tiles = vec![
                TileId::new(0).unwrap(),
                TileId::new(16).unwrap(),
                TileId::new(16).unwrap(),
            ];
            let decoded = DecodedObservation {
                player_id: 0,
                drawn_tile: None,
                hand_tiles: vec![],
                dora_indicators: vec![],
                round_wind: None,
                seat_wind: None,
                visible_tiles: visible_tiles.clone(),
            };
            let context = game_context_from_decoded_observation(&decoded);
            assert_eq!(context.visible_tiles(), visible_tiles.as_slice());
        }

        #[test]
        fn empty_visible_tiles_become_empty_context_visible_tiles() {
            let decoded = DecodedObservation {
                player_id: 0,
                drawn_tile: None,
                hand_tiles: vec![],
                dora_indicators: vec![],
                round_wind: None,
                seat_wind: None,
                visible_tiles: vec![],
            };
            let context = game_context_from_decoded_observation(&decoded);
            assert!(context.visible_tiles().is_empty());
        }

        #[test]
        fn shanten_agent_uses_visible_tiles_from_decoded_observation() {
            use bot_core::{Agent, LegalAction, ShantenAgent};

            let hand_values = [0u8, 4, 8, 12, 17, 20, 24, 28, 32, 48, 53, 56, 36];
            let hand: Vec<TileId> = hand_values
                .iter()
                .map(|&value| TileId::new(value).unwrap())
                .collect();
            let mut visible_tiles = hand.clone();
            visible_tiles.extend(
                [68u8, 69, 70, 71]
                    .iter()
                    .map(|&value| TileId::new(value).unwrap()),
            );
            let decoded = DecodedObservation {
                player_id: 0,
                drawn_tile: TileId::new(68),
                hand_tiles: hand,
                dora_indicators: vec![],
                round_wind: None,
                seat_wind: None,
                visible_tiles,
            };
            let context = game_context_from_decoded_observation(&decoded);
            assert!(!context.visible_tiles().is_empty());

            let actions: Vec<LegalAction> = hand_values
                .iter()
                .chain(std::iter::once(&68u8))
                .map(|&value| LegalAction::Dahai {
                    tile: TileId::new(value).unwrap(),
                })
                .collect();

            let mut agent = ShantenAgent;
            let LegalAction::Dahai { tile } = agent.act(&context, &actions) else {
                panic!("expected dahai");
            };
            assert_eq!(tile.tile_type().to_mjai_string(), "9p");
        }
    }
}
