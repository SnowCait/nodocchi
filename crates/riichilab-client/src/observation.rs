use riichienv_core::observation::Observation;

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
        })
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ObservationError {
    #[error("failed to decode observation: {0}")]
    Decode(String),
}

#[derive(Debug, Clone)]
pub struct DecodedObservation {
    pub player_id: u8,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture_base64(player_id: u8) -> String {
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
            None,
        );
        observation.serialize_to_base64().unwrap()
    }

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
        let payload = ObservationPayload::new(fixture_base64(2));
        let decoded = payload.decode_4p().unwrap();
        assert_eq!(decoded.player_id, 2);
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
}
