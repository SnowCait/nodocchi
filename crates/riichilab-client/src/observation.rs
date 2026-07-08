use bot_core::GameContext;
use bot_logic::TileId;
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
        let hand_tiles = observation
            .hands
            .get(usize::from(observation.player_id))
            .map(|hand| {
                hand.iter()
                    .filter_map(|&raw| u8::try_from(raw).ok())
                    .filter_map(temporary_tile_id_from_observation_tile)
                    .collect()
            })
            .unwrap_or_default();
        let dora_indicators = observation
            .dora_indicators
            .iter()
            .filter_map(|&raw| u8::try_from(raw).ok())
            .filter_map(temporary_tile_id_from_observation_tile)
            .collect();
        Ok(DecodedObservation {
            player_id: observation.player_id,
            drawn_tile: observation
                .drawn_tile
                .and_then(temporary_tile_id_from_observation_tile),
            hand_tiles,
            dora_indicators,
        })
    }
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
}

pub(crate) fn game_context_from_decoded_observation(decoded: &DecodedObservation) -> GameContext {
    GameContext::from_parts_with_dora(
        decoded.drawn_tile,
        decoded.hand_tiles.clone(),
        decoded.dora_indicators.clone(),
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
            };
            let context = game_context_from_decoded_observation(&decoded);
            assert!(context.dora_indicators().is_empty());
        }
    }
}
