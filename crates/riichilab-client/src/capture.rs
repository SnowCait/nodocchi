use std::borrow::Cow;
use std::fmt::Write as _;
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
use crate::protocol::{MjaiEvent, MjaiPossibleAction, TimeControl, parse_server_event_value};

pub const CAPTURE_VERSION: u32 = 1;

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

    #[error("capture record is not a session capture envelope: {0}")]
    Envelope(String),

    #[error("capture record has an unsupported version: {version}")]
    UnsupportedVersion { version: u32 },

    #[error("capture record is not a valid {REQUEST_ACTION_TYPE}: {0}")]
    Fields(String),

    #[error("capture server event is malformed: {0}")]
    ServerEvent(String),
}

/// Capture record の向き。同じ `type` が server event と client action の双方に現れるため、
/// 推測ではなく envelope の direction で区別する。
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CaptureDirection {
    Server,
    Client,
}

impl CaptureDirection {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Server => "server",
            Self::Client => "client",
        }
    }
}

pub fn init(path: Option<&Path>) -> Result<Option<(SessionCapture, WorkerGuard)>, CaptureError> {
    let Some(path) = path else {
        return Ok(None);
    };

    let (writer, guard) = NonBlockingBuilder::default()
        .lossy(true)
        .thread_name(CAPTURE_THREAD_NAME)
        .finish(open_capture_file(path)?);
    Ok(Some((SessionCapture::new(writer), guard)))
}

fn open_capture_file(path: &Path) -> Result<File, CaptureError> {
    File::options()
        .create(true)
        .write(true)
        .truncate(true)
        .open(path)
        .map_err(|source| CaptureError::OpenCaptureFile {
            path: path.to_path_buf(),
            source,
        })
}

/// client 1起動 = 1対局の双方向 session capture writer。
#[derive(Debug)]
pub struct SessionCapture {
    writer: NonBlocking,
    line: String,
}

impl SessionCapture {
    fn new(writer: NonBlocking) -> Self {
        Self {
            writer,
            line: String::new(),
        }
    }

    pub fn write_server_event(&mut self, raw_text: &str) {
        self.write_record(CaptureDirection::Server, raw_text);
    }

    pub fn write_client_action(&mut self, raw_payload: &str) {
        self.write_record(CaptureDirection::Client, raw_payload);
    }

    fn write_record(&mut self, direction: CaptureDirection, raw_text: &str) {
        let Some(event) = jsonl_event(raw_text) else {
            warn!(
                direction = direction.as_str(),
                "capture event is not a single JSON value; skipping"
            );
            return;
        };

        self.line.clear();
        push_record(&mut self.line, direction, &event);
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

/// raw payload を envelope で包んだ1行を組み立てる。raw JSON が不正なら `None`。
pub fn record_line(direction: CaptureDirection, raw_text: &str) -> Option<String> {
    let event = jsonl_event(raw_text)?;
    let mut line = String::with_capacity(event.len() + 48);
    push_record(&mut line, direction, &event);
    Some(line)
}

fn push_record(line: &mut String, direction: CaptureDirection, event: &str) {
    write!(
        line,
        r#"{{"version":{CAPTURE_VERSION},"direction":"{}","event":{event}}}"#,
        direction.as_str()
    )
    .expect("writing to a String never fails");
}

/// 受信 / 送信した raw JSON を、1行の JSONL に載せられる形へ整える。単一行ならそのまま借用し、
/// 複数行のときだけ compact へ書き直す。decode / re-serialize による情報欠落を避けるため。
fn jsonl_event(raw_text: &str) -> Option<Cow<'_, str>> {
    let trimmed = raw_text.trim();
    if trimmed.is_empty() {
        return None;
    }
    if !trimmed.contains(['\n', '\r']) {
        serde_json::from_str::<serde::de::IgnoredAny>(trimmed).ok()?;
        return Some(Cow::Borrowed(trimmed));
    }

    let value: serde_json::Value = serde_json::from_str(trimmed).ok()?;
    serde_json::to_string(&value).ok().map(Cow::Owned)
}

pub fn capture_server_event(capture: Option<&mut SessionCapture>, raw_text: &str) {
    if let Some(capture) = capture {
        capture.write_server_event(raw_text);
    }
}

pub fn capture_client_action(capture: Option<&mut SessionCapture>, raw_payload: &str) {
    if let Some(capture) = capture {
        capture.write_client_action(raw_payload);
    }
}

/// session capture の1行。offline analyzer からも再利用できる pure な parser API。
#[derive(Debug, Clone, PartialEq, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CaptureRecord {
    version: u32,
    direction: CaptureDirection,
    event: serde_json::Value,
}

impl CaptureRecord {
    pub fn from_json_line(line: &str) -> Result<Self, CaptureRecordError> {
        let value: serde_json::Value = serde_json::from_str(line)
            .map_err(|error| CaptureRecordError::Json(error.to_string()))?;
        let record: Self = serde_json::from_value(value)
            .map_err(|error| CaptureRecordError::Envelope(error.to_string()))?;

        if record.version != CAPTURE_VERSION {
            return Err(CaptureRecordError::UnsupportedVersion {
                version: record.version,
            });
        }
        Ok(record)
    }

    pub fn version(&self) -> u32 {
        self.version
    }

    pub fn direction(&self) -> CaptureDirection {
        self.direction
    }

    pub fn event(&self) -> &serde_json::Value {
        &self.event
    }

    pub fn event_type(&self) -> Option<&str> {
        self.event.get("type").and_then(serde_json::Value::as_str)
    }

    pub fn server_event(&self) -> Option<&serde_json::Value> {
        matches!(self.direction, CaptureDirection::Server).then_some(&self.event)
    }

    pub fn client_action(&self) -> Option<&serde_json::Value> {
        matches!(self.direction, CaptureDirection::Client).then_some(&self.event)
    }

    /// server が送ってきた `request_action` record だけを `CapturedRequestAction` にする。
    /// 他の record は `Ok(None)` で、client action や `action_ack` は対象外。
    pub fn request_action(&self) -> Result<Option<CapturedRequestAction>, CaptureRecordError> {
        let Some(event) = self.server_event() else {
            return Ok(None);
        };
        if self.event_type() != Some(REQUEST_ACTION_TYPE) {
            return Ok(None);
        }

        serde_json::from_value(event.clone())
            .map(Some)
            .map_err(|error| CaptureRecordError::Fields(error.to_string()))
    }

    /// server record を live client と同じ parser semantics で MJAI event にする。
    pub fn mjai_event(&self) -> Result<Option<MjaiEvent>, CaptureRecordError> {
        let Some(event) = self.server_event() else {
            return Ok(None);
        };
        parse_server_event_value(event.clone())
            .map_err(|error| CaptureRecordError::ServerEvent(error.to_string()))
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

    fn server_line(raw_text: &str) -> String {
        format!(r#"{{"version":1,"direction":"server","event":{raw_text}}}"#)
    }

    fn client_line(raw_text: &str) -> String {
        format!(r#"{{"version":1,"direction":"client","event":{raw_text}}}"#)
    }

    fn write_session(path: &Path, records: &[(CaptureDirection, String)]) -> String {
        let (mut capture, guard) = init(Some(path)).unwrap().unwrap();
        for (direction, text) in records {
            match direction {
                CaptureDirection::Server => capture.write_server_event(text),
                CaptureDirection::Client => capture.write_client_action(text),
            }
        }
        drop(capture);
        drop(guard);
        std::fs::read_to_string(path).unwrap()
    }

    fn write_server_events(path: &Path, texts: &[String]) -> String {
        let records: Vec<(CaptureDirection, String)> = texts
            .iter()
            .map(|text| (CaptureDirection::Server, text.clone()))
            .collect();
        write_session(path, &records)
    }

    #[test]
    fn init_without_path_does_not_touch_the_filesystem() {
        let path = temp_capture_path("disabled");
        let _ = std::fs::remove_file(&path);

        assert!(init(None).unwrap().is_none());

        assert!(!path.exists());
    }

    #[test]
    fn capture_helpers_without_capture_write_nothing() {
        capture_server_event(None, &request_action_text(1, "AAA"));
        capture_client_action(None, r#"{"type":"none","request_id":1}"#);
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
    fn wraps_a_server_event_in_a_capture_envelope() {
        let path = temp_capture_path("server-envelope");
        let _ = std::fs::remove_file(&path);
        let text = request_action_text(1, "AAA");

        let contents = write_server_events(&path, std::slice::from_ref(&text));
        let _ = std::fs::remove_file(&path);

        assert_eq!(contents, format!("{}\n", server_line(&text)));

        let record = CaptureRecord::from_json_line(contents.lines().next().unwrap()).unwrap();
        assert_eq!(record.version(), CAPTURE_VERSION);
        assert_eq!(record.direction(), CaptureDirection::Server);
        assert_eq!(record.event_type(), Some("request_action"));
        assert_eq!(
            record.event(),
            &serde_json::from_str::<serde_json::Value>(&text).unwrap()
        );
    }

    #[test]
    fn wraps_a_client_action_in_a_capture_envelope() {
        let path = temp_capture_path("client-envelope");
        let _ = std::fs::remove_file(&path);
        let payload = r#"{"type":"reach","actor":1,"request_id":123}"#;

        let contents = write_session(&path, &[(CaptureDirection::Client, payload.to_string())]);
        let _ = std::fs::remove_file(&path);

        assert_eq!(contents, format!("{}\n", client_line(payload)));

        let record = CaptureRecord::from_json_line(contents.lines().next().unwrap()).unwrap();
        assert_eq!(record.direction(), CaptureDirection::Client);
        assert_eq!(record.event_type(), Some("reach"));
        assert_eq!(
            record.client_action(),
            Some(&serde_json::from_str::<serde_json::Value>(payload).unwrap())
        );
        assert_eq!(record.server_event(), None);
    }

    #[test]
    fn the_same_event_type_is_distinguished_by_direction() {
        let path = temp_capture_path("direction");
        let _ = std::fs::remove_file(&path);
        let client_reach = r#"{"type":"reach","actor":1,"request_id":7}"#;
        let server_reach = r#"{"type":"reach","actor":1}"#;

        let contents = write_session(
            &path,
            &[
                (CaptureDirection::Client, client_reach.to_string()),
                (CaptureDirection::Server, server_reach.to_string()),
            ],
        );
        let _ = std::fs::remove_file(&path);

        let records: Vec<CaptureRecord> = contents
            .lines()
            .map(|line| CaptureRecord::from_json_line(line).unwrap())
            .collect();
        assert_eq!(
            records
                .iter()
                .map(CaptureRecord::direction)
                .collect::<Vec<_>>(),
            [CaptureDirection::Client, CaptureDirection::Server]
        );
        assert!(
            records
                .iter()
                .all(|record| record.event_type() == Some("reach"))
        );
        assert!(records[0].server_event().is_none());
        assert!(records[1].client_action().is_none());
    }

    #[test]
    fn keeps_the_order_the_records_were_written_in() {
        let path = temp_capture_path("order");
        let _ = std::fs::remove_file(&path);
        let request_action = request_action_text(41, "AAA");
        let client_reach = r#"{"type":"reach","actor":1,"request_id":41}"#;
        let action_ack = r#"{"type":"action_ack","request_id":41,"status":"accepted"}"#;
        let server_reach = r#"{"type":"reach","actor":1}"#;
        let server_dahai = r#"{"type":"dahai","actor":1,"pai":"1m","tsumogiri":true}"#;

        let contents = write_session(
            &path,
            &[
                (CaptureDirection::Server, request_action.clone()),
                (CaptureDirection::Client, client_reach.to_string()),
                (CaptureDirection::Server, action_ack.to_string()),
                (CaptureDirection::Server, server_reach.to_string()),
                (CaptureDirection::Server, server_dahai.to_string()),
            ],
        );
        let _ = std::fs::remove_file(&path);

        assert_eq!(
            contents.lines().collect::<Vec<_>>(),
            [
                server_line(&request_action),
                client_line(client_reach),
                server_line(action_ack),
                server_line(server_reach),
                server_line(server_dahai),
            ]
        );
    }

    #[test]
    fn captures_every_server_event_of_a_kyoku() {
        let path = temp_capture_path("all-events");
        let _ = std::fs::remove_file(&path);
        let request_action = request_action_text(30, "AAA");
        let texts = [
            r#"{"type":"start_game","id":0}"#,
            r#"{"type":"start_kyoku","kyoku":1}"#,
            r#"{"type":"tsumo","actor":0,"pai":"1m"}"#,
            r#"{"type":"dahai","actor":0,"pai":"1m","tsumogiri":true}"#,
            r#"{"type":"chi","actor":1,"target":0,"pai":"1m","consumed":["2m","3m"]}"#,
            r#"{"type":"pon","actor":2,"target":1,"pai":"5p","consumed":["5p","5p"]}"#,
            r#"{"type":"daiminkan","actor":3,"target":2,"pai":"E","consumed":["E","E","E"]}"#,
            r#"{"type":"ankan","actor":0,"consumed":["9s","9s","9s","9s"]}"#,
            r#"{"type":"kakan","actor":2,"pai":"5p","consumed":["5p","5p","5p"]}"#,
            r#"{"type":"reach","actor":1}"#,
            &request_action,
            r#"{"type":"action_ack","request_id":30,"status":"accepted"}"#,
            r#"{"type":"hora","actor":1,"target":0,"pai":"1m"}"#,
            r#"{"type":"ryukyoku","reason":"fanpai"}"#,
            r#"{"type":"end_kyoku"}"#,
            r#"{"type":"end_game","scores":[25000,25000,25000,25000]}"#,
            r#"{"type":"validation_result","passed":true}"#,
        ];

        let (mut capture, guard) = init(Some(&path)).unwrap().unwrap();
        for text in texts {
            assert!(parse_server_event(text).unwrap().is_some(), "{text}");
            capture_server_event(Some(&mut capture), text);
        }
        drop(capture);
        drop(guard);

        let contents = std::fs::read_to_string(&path).unwrap();
        let _ = std::fs::remove_file(&path);

        let lines: Vec<&str> = contents.lines().collect();
        assert_eq!(lines.len(), texts.len());
        for (line, text) in lines.iter().zip(texts) {
            assert_eq!(*line, server_line(text));
        }

        let event_types: Vec<String> = lines
            .iter()
            .map(|line| {
                CaptureRecord::from_json_line(line)
                    .unwrap()
                    .event_type()
                    .unwrap()
                    .to_string()
            })
            .collect();
        for expected in ["start_kyoku", "action_ack", "end_game", "validation_result"] {
            assert!(
                event_types.iter().any(|event_type| event_type == expected),
                "{expected}: {event_types:?}"
            );
        }
    }

    #[test]
    fn init_replaces_an_existing_capture_file() {
        let path = temp_capture_path("truncate");
        std::fs::write(
            &path,
            format!("{}\n", server_line(&request_action_text(900, "OLD"))),
        )
        .unwrap();
        let text = request_action_text(1, "AAA");

        let contents = write_server_events(&path, std::slice::from_ref(&text));
        let _ = std::fs::remove_file(&path);

        assert_eq!(contents, format!("{}\n", server_line(&text)));
        assert!(!contents.contains("OLD"), "{contents}");
    }

    #[test]
    fn re_running_init_starts_a_new_capture_session() {
        let path = temp_capture_path("new-session");
        let _ = std::fs::remove_file(&path);

        let first_session = write_server_events(
            &path,
            &[request_action_text(1, "AAA"), request_action_text(2, "BBB")],
        );
        assert_eq!(first_session.lines().count(), 2);

        let text = request_action_text(3, "CCC");
        let second_session = write_server_events(&path, std::slice::from_ref(&text));
        let _ = std::fs::remove_file(&path);

        assert_eq!(second_session, format!("{}\n", server_line(&text)));
        assert!(!second_session.contains("AAA"), "{second_session}");
        assert!(!second_session.contains("BBB"), "{second_session}");
    }

    #[test]
    fn skips_malformed_raw_json_instead_of_writing_a_broken_record() {
        let path = temp_capture_path("malformed");
        let _ = std::fs::remove_file(&path);
        let text = request_action_text(50, "AAA");

        let contents = write_session(
            &path,
            &[
                (CaptureDirection::Server, "not json".to_string()),
                (CaptureDirection::Client, "{\"type\":".to_string()),
                (CaptureDirection::Server, "   ".to_string()),
                (CaptureDirection::Server, text.clone()),
            ],
        );
        let _ = std::fs::remove_file(&path);

        assert_eq!(contents, format!("{}\n", server_line(&text)));
        for line in contents.lines() {
            assert!(CaptureRecord::from_json_line(line).is_ok(), "{line}");
        }
    }

    #[test]
    fn extracts_only_the_server_request_action_records() {
        let observation = fixture_base64(0, Some(59), vec![0, 16, 104]);
        let path = temp_capture_path("request-action");
        let _ = std::fs::remove_file(&path);

        let contents = write_session(
            &path,
            &[
                (
                    CaptureDirection::Server,
                    r#"{"type":"start_kyoku","kyoku":1}"#.to_string(),
                ),
                (
                    CaptureDirection::Server,
                    request_action_text(60, &observation),
                ),
                (
                    CaptureDirection::Client,
                    r#"{"type":"dahai","actor":0,"pai":"1m","request_id":60}"#.to_string(),
                ),
                (
                    CaptureDirection::Server,
                    r#"{"type":"action_ack","request_id":60,"status":"accepted"}"#.to_string(),
                ),
            ],
        );
        let _ = std::fs::remove_file(&path);

        let records: Vec<CapturedRequestAction> = contents
            .lines()
            .map(|line| CaptureRecord::from_json_line(line).unwrap())
            .filter_map(|record| record.request_action().unwrap())
            .collect();

        assert_eq!(records.len(), 1);
        assert_eq!(records[0].request_id, 60);
        assert_eq!(records[0].observation, observation);
        assert_eq!(records[0].observation_payload().as_base64(), observation);
        assert_eq!(
            records[0].possible_actions,
            vec![
                MjaiPossibleAction::Dahai {
                    pai: "1m".to_string(),
                    tsumogiri: Some(false),
                },
                MjaiPossibleAction::None,
            ]
        );
        assert_eq!(
            records[0].legal_actions(),
            possible_actions_to_legal_actions(&records[0].possible_actions)
        );
    }

    #[test]
    fn a_client_action_with_the_same_type_is_not_a_request_action() {
        let record =
            CaptureRecord::from_json_line(&client_line(&request_action_text(61, "AAA"))).unwrap();
        assert_eq!(record.event_type(), Some("request_action"));
        assert_eq!(record.request_action().unwrap(), None);
    }

    #[test]
    fn keeps_optional_request_action_fields() {
        let record = CaptureRecord::from_json_line(&server_line(&request_action_text(22, "AAA")))
            .unwrap()
            .request_action()
            .unwrap()
            .unwrap();
        assert_eq!(record.request_id, 22);
        assert_eq!(record.actor, None);
        assert_eq!(record.time, Some(serde_json::json!({"grace_ms": 5000})));

        let with_actor = server_line(
            r#"{"type":"request_action","request_id":23,"actor":2,"possible_actions":[],"observation":"AAA"}"#,
        );
        let record = CaptureRecord::from_json_line(&with_actor)
            .unwrap()
            .request_action()
            .unwrap()
            .unwrap();
        assert_eq!(record.actor, Some(2));
        assert_eq!(record.time, None);
    }

    #[test]
    fn reports_a_request_action_with_missing_fields() {
        let record =
            CaptureRecord::from_json_line(&server_line(r#"{"type":"request_action"}"#)).unwrap();
        let error = record.request_action().unwrap_err();
        assert!(matches!(error, CaptureRecordError::Fields(_)), "{error:?}");
    }

    #[test]
    fn captured_server_events_share_the_live_parser_semantics() {
        let malformed =
            CaptureRecord::from_json_line(&server_line(r#"{"type":"tsumo","actor":0}"#)).unwrap();
        assert!(
            matches!(
                malformed.mjai_event(),
                Err(CaptureRecordError::ServerEvent(_))
            ),
            "{:?}",
            malformed.mjai_event()
        );

        let unknown =
            CaptureRecord::from_json_line(&server_line(r#"{"type":"future_event","actor":2}"#))
                .unwrap();
        assert_eq!(unknown.mjai_event().unwrap(), None);
    }

    #[test]
    fn rejects_records_that_are_not_capture_envelopes() {
        let error = CaptureRecord::from_json_line("not json").unwrap_err();
        assert!(matches!(error, CaptureRecordError::Json(_)), "{error:?}");

        let error = CaptureRecord::from_json_line(r#"{"version":1,"event":{}}"#).unwrap_err();
        assert!(
            matches!(error, CaptureRecordError::Envelope(_)),
            "{error:?}"
        );

        let error = CaptureRecord::from_json_line(
            r#"{"version":1,"direction":"proxy","event":{"type":"reach"}}"#,
        )
        .unwrap_err();
        assert!(
            matches!(error, CaptureRecordError::Envelope(_)),
            "{error:?}"
        );

        let error = CaptureRecord::from_json_line(
            r#"{"version":2,"direction":"server","event":{"type":"reach"}}"#,
        )
        .unwrap_err();
        assert_eq!(error, CaptureRecordError::UnsupportedVersion { version: 2 });
    }

    // 旧 capture 形式は「1行そのものが request_action」だった。新 parser は fallback を持たず、
    // envelope の無い行を読まない。
    #[test]
    fn does_not_fall_back_to_the_old_raw_request_action_schema() {
        let old_line = request_action_text(70, "AAA");
        let error = CaptureRecord::from_json_line(&old_line).unwrap_err();
        assert!(
            matches!(error, CaptureRecordError::Envelope(_)),
            "{error:?}"
        );

        let old_action_line = r#"{"type":"reach","actor":1,"request_id":70}"#;
        let error = CaptureRecord::from_json_line(old_action_line).unwrap_err();
        assert!(
            matches!(error, CaptureRecordError::Envelope(_)),
            "{error:?}"
        );
    }

    #[test]
    fn record_line_keeps_single_line_json_as_is() {
        let text = request_action_text(33, "AAA");
        assert_eq!(
            record_line(CaptureDirection::Server, &format!("  {text}\n")),
            Some(server_line(&text))
        );
        assert_eq!(
            record_line(
                CaptureDirection::Client,
                r#"{"type":"none","request_id":1}"#
            ),
            Some(client_line(r#"{"type":"none","request_id":1}"#))
        );
    }

    #[test]
    fn record_line_compacts_multi_line_json() {
        let line = record_line(
            CaptureDirection::Server,
            "{\n  \"type\": \"request_action\",\n  \"request_id\": 34\n}",
        )
        .unwrap();
        assert!(!line.contains('\n'), "{line}");

        let record = CaptureRecord::from_json_line(&line).unwrap();
        assert_eq!(
            record.event(),
            &serde_json::json!({"type": "request_action", "request_id": 34})
        );
    }

    #[test]
    fn record_line_rejects_empty_and_broken_text() {
        assert_eq!(record_line(CaptureDirection::Server, "   "), None);
        assert_eq!(record_line(CaptureDirection::Server, "{\n  broken"), None);
        assert_eq!(record_line(CaptureDirection::Client, "not json"), None);
    }

    #[test]
    fn the_captured_table_state_survives_the_replay() {
        use crate::observation::fixture_base64_with_table_state_facts;

        let observation = fixture_base64_with_table_state_facts(
            1,
            Some(59),
            vec![0, 16, 104],
            [12300, 28700, 40100, 18900],
            2,
            3,
            1,
            2,
            3,
        );
        let record =
            CaptureRecord::from_json_line(&server_line(&request_action_text(36, &observation)))
                .unwrap()
                .request_action()
                .unwrap()
                .unwrap();

        let decoded = ObservationPayload::new(observation).decode_4p().unwrap();
        let context = record.game_context().unwrap();

        assert_eq!(context.table_state(), decoded.table_state);
        assert_eq!(
            context,
            game_context_from_decoded_observation(&decoded),
            "capture replay は通常 decode と同じ context を作る"
        );
        assert_eq!(context.scores(), Some([12300, 28700, 40100, 18900]));
        assert_eq!(context.honba(), Some(2));
        assert_eq!(context.kyotaku_points(), Some(3000));
        assert_eq!(context.kyoku(), Some(4));
        assert_eq!(context.remaining_tiles(), None);
    }

    #[test]
    fn decodes_the_captured_observation_with_the_client_conversion() {
        let observation = fixture_base64(0, Some(59), vec![0, 16, 104]);
        let record =
            CaptureRecord::from_json_line(&server_line(&request_action_text(35, &observation)))
                .unwrap()
                .request_action()
                .unwrap()
                .unwrap();

        let decoded = ObservationPayload::new(observation).decode_4p().unwrap();
        assert_eq!(record.decode_observation().unwrap(), decoded);
        assert_eq!(
            record.game_context().unwrap(),
            game_context_from_decoded_observation(&decoded)
        );
    }
}
