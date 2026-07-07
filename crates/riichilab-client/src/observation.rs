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
        Ok(DecodedObservation {
            player_id: observation.player_id,
            drawn_tile: observation
                .drawn_tile
                .and_then(temporary_tile_id_from_observation_tile),
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
}

pub(crate) fn game_context_from_decoded_observation(decoded: &DecodedObservation) -> GameContext {
    decoded
        .drawn_tile
        .map(GameContext::with_drawn_tile)
        .unwrap_or_default()
}

#[cfg(test)]
pub(crate) fn fixture_base64(player_id: u8, drawn_tile: Option<u8>) -> String {
    let observation = Observation::new(
        player_id,
        Default::default(),
        Default::default(),
        Default::default(),
        vec![],
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
        let payload = ObservationPayload::new(fixture_base64(2, None));
        let decoded = payload.decode_4p().unwrap();
        assert_eq!(decoded.player_id, 2);
    }

    #[test]
    fn decode_4p_without_drawn_tile_returns_none() {
        let payload = ObservationPayload::new(fixture_base64(0, None));
        let decoded = payload.decode_4p().unwrap();
        assert_eq!(decoded.drawn_tile, None);
    }

    #[test]
    fn decode_4p_returns_drawn_tile() {
        let payload = ObservationPayload::new(fixture_base64(0, Some(56)));
        let decoded = payload.decode_4p().unwrap();
        assert_eq!(decoded.drawn_tile, TileId::new(56));
    }

    #[test]
    fn decode_4p_normalizes_drawn_tile_to_temporary_tile_id() {
        for (raw, expected) in [(59, 56), (16, 16), (19, 17)] {
            let payload = ObservationPayload::new(fixture_base64(0, Some(raw)));
            let decoded = payload.decode_4p().unwrap();
            assert_eq!(decoded.drawn_tile, TileId::new(expected), "raw: {raw}");
        }
    }

    #[test]
    fn decode_4p_out_of_range_drawn_tile_becomes_none() {
        let payload = ObservationPayload::new(fixture_base64(0, Some(200)));
        let decoded = payload.decode_4p().unwrap();
        assert_eq!(decoded.drawn_tile, None);
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
            };
            assert_eq!(
                game_context_from_decoded_observation(&decoded),
                GameContext::default()
            );
        }
    }
}
