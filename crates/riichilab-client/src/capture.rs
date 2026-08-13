use std::borrow::Cow;
use std::fs::File;
use std::io::Write;
use std::path::{Path, PathBuf};

use bot_core::{GameContext, LegalAction};
use tracing::warn;
use tracing_appender::non_blocking::{NonBlocking, NonBlockingBuilder, WorkerGuard};

use crate::convert::possible_actions_to_legal_actions;
use crate::observation::{
    DecodedObservation, ObservationError, ObservationPayload, game_context_from_decoded_observation,
};
use crate::protocol::{MjaiEvent, MjaiPossibleAction, TimeControl};

const REQUEST_ACTION_TYPE: &str = "request_action";
const CAPTURE_THREAD_NAME: &str = "riichilab-capture";

#[derive(Debug, thiserror::Error)]
pub enum CaptureError {
    #[error("failed to open capture file {path}: {source}")]
    OpenCaptureFile {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
}

#[derive(Debug, thiserror::Error, Clone, PartialEq, Eq)]
pub enum CaptureRecordError {
    #[error("capture record is not valid JSON: {0}")]
    Json(String),

    #[error("capture record type is not {REQUEST_ACTION_TYPE}: {0:?}")]
    UnexpectedType(Option<String>),

    #[error("capture record is not a request_action: {0}")]
    Fields(String),
}

pub fn init(
    path: Option<&Path>,
) -> Result<Option<(RequestActionCapture, WorkerGuard)>, CaptureError> {
    let Some(path) = path else {
        return Ok(None);
    };

    let (writer, guard) = NonBlockingBuilder::default()
        .lossy(true)
        .thread_name(CAPTURE_THREAD_NAME)
        .finish(open_capture_file(path)?);
    Ok(Some((RequestActionCapture::new(writer), guard)))
}

fn open_capture_file(path: &Path) -> Result<File, CaptureError> {
    File::options()
        .create(true)
        .append(true)
        .open(path)
        .map_err(|source| CaptureError::OpenCaptureFile {
            path: path.to_path_buf(),
            source,
        })
}

#[derive(Debug)]
pub struct RequestActionCapture {
    writer: NonBlocking,
    line: String,
}

impl RequestActionCapture {
    fn new(writer: NonBlocking) -> Self {
        Self {
            writer,
            line: String::new(),
        }
    }

    pub fn write_record(&mut self, raw_text: &str) {
        let Some(record) = jsonl_record(raw_text) else {
            warn!("capture record is not a single JSON value; skipping");
            return;
        };

        self.line.clear();
        self.line.push_str(&record);
        self.line.push('\n');

        let dropped_before = self.writer.error_counter().dropped_lines();
        if let Err(error) = self.writer.write_all(self.line.as_bytes()) {
            warn!(error = %error, "failed to write capture record");
            return;
        }

        let dropped = self.writer.error_counter().dropped_lines();
        if dropped > dropped_before {
            warn!(dropped, "capture record dropped by the non-blocking writer");
        }
    }
}

fn jsonl_record(raw_text: &str) -> Option<Cow<'_, str>> {
    let trimmed = raw_text.trim();
    if trimmed.is_empty() {
        return None;
    }
    if !trimmed.contains(['\n', '\r']) {
        return Some(Cow::Borrowed(trimmed));
    }

    let value: serde_json::Value = serde_json::from_str(trimmed).ok()?;
    serde_json::to_string(&value).ok().map(Cow::Owned)
}

pub fn should_capture_event(event: &MjaiEvent) -> bool {
    matches!(event, MjaiEvent::RequestAction { .. })
}

pub fn capture_server_event(
    capture: Option<&mut RequestActionCapture>,
    event: &MjaiEvent,
    raw_text: &str,
) {
    if let Some(capture) = capture
        && should_capture_event(event)
    {
        capture.write_record(raw_text);
    }
}

#[derive(Debug, Clone, PartialEq, serde::Deserialize)]
pub struct CapturedRequestAction {
    pub request_id: u64,
    #[serde(default)]
    pub actor: Option<u8>,
    #[serde(default)]
    pub time: Option<TimeControl>,
    pub possible_actions: Vec<MjaiPossibleAction>,
    pub observation: String,
}

impl CapturedRequestAction {
    pub fn from_json_line(line: &str) -> Result<Self, CaptureRecordError> {
        let value: serde_json::Value = serde_json::from_str(line)
            .map_err(|error| CaptureRecordError::Json(error.to_string()))?;

        match value.get("type").and_then(serde_json::Value::as_str) {
            Some(REQUEST_ACTION_TYPE) => {}
            other => {
                return Err(CaptureRecordError::UnexpectedType(
                    other.map(str::to_string),
                ));
            }
        }

        serde_json::from_value(value).map_err(|error| CaptureRecordError::Fields(error.to_string()))
    }

    pub fn observation_payload(&self) -> ObservationPayload {
        ObservationPayload::new(self.observation.clone())
    }

    pub fn decode_observation(&self) -> Result<DecodedObservation, ObservationError> {
        self.observation_payload().decode_4p()
    }

    pub fn game_context(&self) -> Result<GameContext, ObservationError> {
        Ok(game_context_from_decoded_observation(
            &self.decode_observation()?,
        ))
    }

    pub fn legal_actions(&self) -> Vec<LegalAction> {
        possible_actions_to_legal_actions(&self.possible_actions)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::observation::fixture_base64;
    use crate::protocol::parse_server_event;

    fn temp_capture_path(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "riichilab-client-capture-{name}-{}.jsonl",
            std::process::id()
        ))
    }

    fn request_action_text(request_id: u64, observation: &str) -> String {
        format!(
            r#"{{"type":"request_action","request_id":{request_id},"time":{{"grace_ms":5000}},"possible_actions":[{{"type":"dahai","pai":"1m","tsumogiri":false}},{{"type":"none"}}],"observation":"{observation}"}}"#
        )
    }

    fn write_records(path: &Path, texts: &[String]) -> String {
        let (mut capture, guard) = init(Some(path)).unwrap().unwrap();
        for text in texts {
            capture.write_record(text);
        }
        drop(capture);
        drop(guard);
        std::fs::read_to_string(path).unwrap()
    }

    #[test]
    fn init_without_path_does_not_touch_the_filesystem() {
        let path = temp_capture_path("disabled");
        let _ = std::fs::remove_file(&path);

        assert!(init(None).unwrap().is_none());

        assert!(!path.exists());
    }

    #[test]
    fn init_fails_for_missing_directory() {
        let path = temp_capture_path("missing-dir").join("nested.jsonl");
        let error = init(Some(&path)).unwrap_err();
        let CaptureError::OpenCaptureFile {
            path: reported_path,
            ..
        } = error;
        assert_eq!(reported_path, path);
    }

    #[test]
    fn writes_one_record_for_one_request_action() {
        let path = temp_capture_path("single");
        let _ = std::fs::remove_file(&path);
        let text = request_action_text(1, "AAA");

        let contents = write_records(&path, std::slice::from_ref(&text));
        let _ = std::fs::remove_file(&path);

        assert_eq!(contents, format!("{text}\n"));
        assert_eq!(contents.lines().count(), 1);
    }

    #[test]
    fn writes_one_line_per_request_action() {
        let path = temp_capture_path("multiple");
        let _ = std::fs::remove_file(&path);
        let texts = vec![
            request_action_text(1, "AAA"),
            request_action_text(2, "BBB"),
            request_action_text(3, "CCC"),
        ];

        let contents = write_records(&path, &texts);
        let _ = std::fs::remove_file(&path);

        let lines: Vec<&str> = contents.lines().collect();
        assert_eq!(lines.len(), 3);
        for (line, text) in lines.iter().zip(&texts) {
            assert_eq!(*line, text);
        }
    }

    #[test]
    fn each_line_parses_back_as_a_request_action_record() {
        let path = temp_capture_path("reparse");
        let _ = std::fs::remove_file(&path);
        let texts = vec![
            request_action_text(11, "AAA"),
            request_action_text(12, "BBB"),
        ];

        let contents = write_records(&path, &texts);
        let _ = std::fs::remove_file(&path);

        let records: Vec<CapturedRequestAction> = contents
            .lines()
            .map(|line| CapturedRequestAction::from_json_line(line).unwrap())
            .collect();
        assert_eq!(
            records
                .iter()
                .map(|record| record.request_id)
                .collect::<Vec<_>>(),
            [11, 12]
        );
        assert_eq!(records[0].observation, "AAA");
        assert_eq!(records[1].observation, "BBB");
    }

    #[test]
    fn observation_string_is_kept_as_received() {
        let observation = fixture_base64(0, Some(59), vec![0, 16, 104]);
        let path = temp_capture_path("observation");
        let _ = std::fs::remove_file(&path);

        let contents = write_records(&path, &[request_action_text(20, &observation)]);
        let _ = std::fs::remove_file(&path);

        let record =
            CapturedRequestAction::from_json_line(contents.lines().next().unwrap()).unwrap();
        assert_eq!(record.observation, observation);
        assert_eq!(record.observation_payload().as_base64(), observation);
    }

    #[test]
    fn possible_actions_survive_the_roundtrip() {
        let path = temp_capture_path("possible-actions");
        let _ = std::fs::remove_file(&path);

        let contents = write_records(&path, &[request_action_text(21, "AAA")]);
        let _ = std::fs::remove_file(&path);

        let record =
            CapturedRequestAction::from_json_line(contents.lines().next().unwrap()).unwrap();
        assert_eq!(
            record.possible_actions,
            vec![
                MjaiPossibleAction::Dahai {
                    pai: "1m".to_string(),
                    tsumogiri: Some(false),
                },
                MjaiPossibleAction::None,
            ]
        );
        assert_eq!(
            record.legal_actions(),
            possible_actions_to_legal_actions(&record.possible_actions)
        );
    }

    #[test]
    fn keeps_optional_request_action_fields() {
        let record =
            CapturedRequestAction::from_json_line(&request_action_text(22, "AAA")).unwrap();
        assert_eq!(record.request_id, 22);
        assert_eq!(record.actor, None);
        assert_eq!(record.time, Some(serde_json::json!({"grace_ms": 5000})),);

        let with_actor = r#"{"type":"request_action","request_id":23,"actor":2,"possible_actions":[],"observation":"AAA"}"#;
        let record = CapturedRequestAction::from_json_line(with_actor).unwrap();
        assert_eq!(record.actor, Some(2));
        assert_eq!(record.time, None);
    }

    #[test]
    fn rejects_records_that_are_not_request_actions() {
        let error =
            CapturedRequestAction::from_json_line(r#"{"type":"start_game","id":0}"#).unwrap_err();
        assert_eq!(
            error,
            CaptureRecordError::UnexpectedType(Some("start_game".to_string()))
        );

        let error = CapturedRequestAction::from_json_line(r#"{"id":0}"#).unwrap_err();
        assert_eq!(error, CaptureRecordError::UnexpectedType(None));

        let error = CapturedRequestAction::from_json_line("not json").unwrap_err();
        assert!(matches!(error, CaptureRecordError::Json(_)), "{error:?}");

        let error =
            CapturedRequestAction::from_json_line(r#"{"type":"request_action"}"#).unwrap_err();
        assert!(matches!(error, CaptureRecordError::Fields(_)), "{error:?}");
    }

    #[test]
    fn only_request_action_events_are_captured() {
        let path = temp_capture_path("event-filter");
        let _ = std::fs::remove_file(&path);
        let request_action = request_action_text(30, "AAA");
        let texts = [
            r#"{"type":"start_game","id":0}"#,
            r#"{"type":"tsumo","actor":0,"pai":"1m"}"#,
            &request_action,
            r#"{"type":"action_ack","request_id":30,"status":"accepted"}"#,
            r#"{"type":"end_game","scores":[25000,25000,25000,25000]}"#,
        ];

        let (mut capture, guard) = init(Some(&path)).unwrap().unwrap();
        for text in texts {
            let event = parse_server_event(text).unwrap().unwrap();
            capture_server_event(Some(&mut capture), &event, text);
        }
        drop(capture);
        drop(guard);

        let contents = std::fs::read_to_string(&path).unwrap();
        let _ = std::fs::remove_file(&path);

        assert_eq!(contents, format!("{request_action}\n"));
    }

    #[test]
    fn should_capture_event_only_accepts_request_action() {
        let request_action = parse_server_event(&request_action_text(31, "AAA"))
            .unwrap()
            .unwrap();
        assert!(should_capture_event(&request_action));

        for text in [
            r#"{"type":"start_game","id":0}"#,
            r#"{"type":"start_kyoku","kyoku":1}"#,
            r#"{"type":"dahai","actor":1,"pai":"1m"}"#,
            r#"{"type":"reach","actor":1}"#,
            r#"{"type":"action_ack","request_id":1,"status":"accepted"}"#,
            r#"{"type":"end_game","scores":[]}"#,
            r#"{"type":"validation_result","passed":true}"#,
        ] {
            let event = parse_server_event(text).unwrap().unwrap();
            assert!(!should_capture_event(&event), "{text}");
        }
    }

    #[test]
    fn capture_server_event_without_capture_writes_nothing() {
        let text = request_action_text(32, "AAA");
        let event = parse_server_event(&text).unwrap().unwrap();
        capture_server_event(None, &event, &text);
    }

    #[test]
    fn jsonl_record_keeps_single_line_json_as_is() {
        let text = request_action_text(33, "AAA");
        assert_eq!(jsonl_record(&text), Some(Cow::Borrowed(text.as_str())));
        assert_eq!(
            jsonl_record(&format!("  {text}\n")),
            Some(Cow::Borrowed(text.as_str()))
        );
    }

    #[test]
    fn jsonl_record_compacts_multi_line_json() {
        let record = jsonl_record("{\n  \"type\": \"request_action\",\n  \"request_id\": 34\n}")
            .unwrap()
            .into_owned();
        assert!(!record.contains('\n'), "{record}");
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&record).unwrap(),
            serde_json::json!({"type": "request_action", "request_id": 34})
        );
    }

    #[test]
    fn jsonl_record_rejects_empty_and_broken_multi_line_text() {
        assert_eq!(jsonl_record("   "), None);
        assert_eq!(jsonl_record("{\n  broken"), None);
    }

    #[test]
    fn decodes_the_captured_observation_with_the_client_conversion() {
        let observation = fixture_base64(0, Some(59), vec![0, 16, 104]);
        let record =
            CapturedRequestAction::from_json_line(&request_action_text(35, &observation)).unwrap();

        let decoded = ObservationPayload::new(observation).decode_4p().unwrap();
        assert_eq!(record.decode_observation().unwrap(), decoded);
        assert_eq!(
            record.game_context().unwrap(),
            game_context_from_decoded_observation(&decoded)
        );
    }
}
