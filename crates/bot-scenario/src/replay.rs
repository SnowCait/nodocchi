use riichilab_client::CapturedRequestAction;

use crate::error::ScenarioError;
use crate::scenario::Scenario;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CapturedScenario {
    pub path: String,
    pub request_id: u64,
    pub actor: Option<u8>,
    pub possible_action_count: usize,
    pub scenario: Scenario,
}

impl CapturedScenario {
    pub fn header(&self) -> String {
        let actor = self
            .actor
            .map(|actor| actor.to_string())
            .unwrap_or_else(|| "None".to_string());
        [
            "RiichiLab capture".to_string(),
            format!("  file: {}", self.path),
            format!("  request_id: {}", self.request_id),
            format!("  actor: {actor}"),
            format!("  possible actions: {}", self.possible_action_count),
            format!("  legal actions: {}", self.scenario.legal_actions.len()),
        ]
        .join("\n")
    }
}

pub fn load_captured_scenario(
    path: &str,
    request_id: Option<u64>,
) -> Result<CapturedScenario, ScenarioError> {
    let text = std::fs::read_to_string(path).map_err(|error| ScenarioError::ReadFile {
        path: path.to_string(),
        message: error.to_string(),
    })?;

    captured_scenario(path, select_record(path, &text, request_id)?)
}

fn select_record(
    path: &str,
    text: &str,
    request_id: Option<u64>,
) -> Result<CapturedRequestAction, ScenarioError> {
    let mut records = Vec::new();
    for (index, line) in text.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        let record = CapturedRequestAction::from_json_line(line).map_err(|source| {
            ScenarioError::CaptureRecord {
                path: path.to_string(),
                line: index + 1,
                source,
            }
        })?;
        records.push(record);
    }

    let Some(request_id) = request_id else {
        return match records.len() {
            0 => Err(ScenarioError::EmptyCapture {
                path: path.to_string(),
            }),
            1 => Ok(records.remove(0)),
            count => Err(ScenarioError::AmbiguousCapture {
                path: path.to_string(),
                count,
            }),
        };
    };

    records
        .into_iter()
        .find(|record| record.request_id == request_id)
        .ok_or(ScenarioError::CapturedRequestNotFound {
            path: path.to_string(),
            request_id,
        })
}

fn captured_scenario(
    path: &str,
    record: CapturedRequestAction,
) -> Result<CapturedScenario, ScenarioError> {
    let context = record
        .game_context()
        .map_err(|error| ScenarioError::CaptureObservation {
            path: path.to_string(),
            request_id: record.request_id,
            message: error.to_string(),
        })?;

    Ok(CapturedScenario {
        path: path.to_string(),
        request_id: record.request_id,
        actor: record.actor,
        possible_action_count: record.possible_actions.len(),
        scenario: Scenario {
            context,
            legal_actions: record.legal_actions(),
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use bot_core::{
        Agent, LegalAction, MeldKind, PushPullMode, PushPullReason, ShantenAgent,
        player_threat_facts_from_context,
    };
    use riichilab_client::observation::{
        fixture_base64, fixture_base64_with_melds, fixture_meld,
        game_context_from_decoded_observation,
    };
    use riichilab_client::{
        CaptureRecordError, MjaiPossibleAction, ObservationPayload, build_response_for_request,
        checked_legal_action_to_mjai_action, possible_actions_to_legal_actions,
    };

    const CAPTURED_HAND: [u8; 13] = [0, 4, 8, 12, 17, 20, 53, 54, 96, 100, 120, 124, 125];

    const CAPTURED_DRAWN_TILE: u8 = 59;

    const CAPTURED_DAHAI: [&str; 12] = [
        "1m", "2m", "3m", "4m", "5m", "6m", "5p", "6p", "7s", "8s", "N", "P",
    ];

    fn possible_actions_json() -> String {
        CAPTURED_DAHAI
            .iter()
            .map(|pai| format!(r#"{{"type":"dahai","pai":"{pai}","tsumogiri":false}}"#))
            .collect::<Vec<_>>()
            .join(",")
    }

    fn request_action_line(request_id: u64, observation: &str) -> String {
        format!(
            r#"{{"type":"request_action","request_id":{request_id},"possible_actions":[{}],"observation":"{observation}"}}"#,
            possible_actions_json()
        )
    }

    fn observation_base64() -> String {
        fixture_base64(0, Some(CAPTURED_DRAWN_TILE), CAPTURED_HAND.to_vec())
    }

    fn observation_base64_with_opponent_pon() -> String {
        let mut melds: [Vec<_>; 4] = Default::default();
        melds[1] = vec![
            fixture_meld(MeldKind::Pon, vec![124, 125, 126], Some(126)),
            fixture_meld(MeldKind::Chi, vec![32, 36, 40], Some(32)),
        ];
        let mut discards: [Vec<u8>; 4] = Default::default();
        discards[2] = vec![126];
        fixture_base64_with_melds(
            0,
            Some(CAPTURED_DRAWN_TILE),
            CAPTURED_HAND.to_vec(),
            vec![],
            discards,
            melds,
        )
    }

    fn write_capture(name: &str, lines: &[String]) -> String {
        let path = std::env::temp_dir().join(format!(
            "bot-scenario-replay-{name}-{}.jsonl",
            std::process::id()
        ));
        let mut text = lines.join("\n");
        text.push('\n');
        std::fs::write(&path, text).unwrap();
        path.to_str().unwrap().to_string()
    }

    #[test]
    fn replays_the_only_captured_request() {
        let observation = observation_base64();
        let path = write_capture("single", &[request_action_line(410, &observation)]);
        let captured = load_captured_scenario(&path, None).unwrap();
        let _ = std::fs::remove_file(&path);

        assert_eq!(captured.request_id, 410);
        assert_eq!(captured.possible_action_count, CAPTURED_DAHAI.len());
    }

    #[test]
    fn replay_context_matches_the_client_observation_decoding() {
        let observation = observation_base64();
        let path = write_capture("context", &[request_action_line(411, &observation)]);
        let captured = load_captured_scenario(&path, None).unwrap();
        let _ = std::fs::remove_file(&path);

        let decoded = ObservationPayload::new(observation).decode_4p().unwrap();
        assert_eq!(
            captured.scenario.context,
            game_context_from_decoded_observation(&decoded)
        );
    }

    #[test]
    fn replay_legal_actions_match_the_client_conversion() {
        let observation = observation_base64();
        let path = write_capture("legal-actions", &[request_action_line(412, &observation)]);
        let captured = load_captured_scenario(&path, None).unwrap();
        let _ = std::fs::remove_file(&path);

        let possible_actions: Vec<MjaiPossibleAction> =
            serde_json::from_str(&format!("[{}]", possible_actions_json()))
                .expect("possible actions should parse");
        assert_eq!(
            captured.scenario.legal_actions,
            possible_actions_to_legal_actions(&possible_actions)
        );
        assert!(
            captured
                .scenario
                .legal_actions
                .iter()
                .all(|action| matches!(action, LegalAction::Dahai { .. }))
        );
    }

    fn direct_scenario(observation: &str) -> Scenario {
        let decoded = ObservationPayload::new(observation.to_string())
            .decode_4p()
            .unwrap();
        let possible_actions: Vec<MjaiPossibleAction> =
            serde_json::from_str(&format!("[{}]", possible_actions_json()))
                .expect("possible actions should parse");
        Scenario {
            context: game_context_from_decoded_observation(&decoded),
            legal_actions: possible_actions_to_legal_actions(&possible_actions),
        }
    }

    #[test]
    fn replay_diagnostic_matches_a_direct_diagnose_on_the_same_context() {
        let observation = observation_base64();
        let path = write_capture("diagnose", &[request_action_line(413, &observation)]);
        let captured = load_captured_scenario(&path, None).unwrap();
        let _ = std::fs::remove_file(&path);

        let direct = direct_scenario(&observation);
        let replayed =
            ShantenAgent::diagnose(&captured.scenario.context, &captured.scenario.legal_actions);
        let expected = ShantenAgent::diagnose(&direct.context, &direct.legal_actions);

        assert_eq!(replayed.selected_action, expected.selected_action);
        assert_eq!(
            replayed.selected_action,
            ShantenAgent.act(&direct.context, &direct.legal_actions)
        );
        assert_eq!(replayed.push_pull_inputs, expected.push_pull_inputs);
        assert_eq!(replayed.push_pull_decision, expected.push_pull_decision);
        assert_eq!(replayed.player_threats, expected.player_threats);
        assert_eq!(
            replayed
                .player_threats
                .clone()
                .map(|diagnostic| diagnostic.facts),
            player_threat_facts_from_context(&direct.context)
        );
    }

    #[test]
    fn replayed_selection_matches_the_client_response_for_the_same_request() {
        let observation = observation_base64();
        let path = write_capture("client-response", &[request_action_line(421, &observation)]);
        let captured = load_captured_scenario(&path, None).unwrap();
        let _ = std::fs::remove_file(&path);

        let possible_actions: Vec<MjaiPossibleAction> =
            serde_json::from_str(&format!("[{}]", possible_actions_json()))
                .expect("possible actions should parse");
        let client_response = build_response_for_request(
            0,
            captured.request_id,
            &possible_actions,
            &ObservationPayload::new(observation),
            &mut ShantenAgent,
        );

        let diagnostic =
            ShantenAgent::diagnose(&captured.scenario.context, &captured.scenario.legal_actions);
        assert!(client_response.is_some());
        assert_eq!(
            client_response,
            checked_legal_action_to_mjai_action(
                &diagnostic.selected_action,
                0,
                captured.request_id,
                &possible_actions,
                &captured.scenario.context,
            )
        );
    }

    #[test]
    fn replay_keeps_open_melds_of_a_non_reaching_opponent() {
        let observation = observation_base64_with_opponent_pon();
        let path = write_capture("open-melds", &[request_action_line(414, &observation)]);
        let captured = load_captured_scenario(&path, None).unwrap();
        let _ = std::fs::remove_file(&path);

        let context = &captured.scenario.context;
        let diagnostic = ShantenAgent::diagnose(context, &captured.scenario.legal_actions);
        let facts = &diagnostic.player_threats[1].facts;

        assert!(!context.any_opponent_reached());
        assert!(!facts.reached);
        assert_eq!(facts.meld_count, 2);
        assert_eq!(facts.open_meld_count, 2);
        assert_eq!(facts.is_opponent(), Some(true));
        assert_eq!(facts.value_honor_melds.dragon, 1);

        let decision = diagnostic.push_pull_decision.unwrap();
        assert_eq!(decision.mode, PushPullMode::Push);
        assert_eq!(decision.reason, PushPullReason::NoOpponentReach);
    }

    #[test]
    fn selects_a_record_by_request_id() {
        let observation = observation_base64();
        let path = write_capture(
            "select",
            &[
                request_action_line(415, &observation),
                request_action_line(416, &observation),
                request_action_line(417, &observation),
            ],
        );

        let captured = load_captured_scenario(&path, Some(416)).unwrap();
        assert_eq!(captured.request_id, 416);

        let error = load_captured_scenario(&path, Some(999)).unwrap_err();
        assert_eq!(
            error,
            ScenarioError::CapturedRequestNotFound {
                path: path.clone(),
                request_id: 999,
            }
        );

        let error = load_captured_scenario(&path, None).unwrap_err();
        let _ = std::fs::remove_file(&path);
        assert_eq!(
            error,
            ScenarioError::AmbiguousCapture {
                path: path.clone(),
                count: 3,
            }
        );
    }

    #[test]
    fn reports_an_empty_capture_file() {
        let path = write_capture("empty", &[]);
        let error = load_captured_scenario(&path, None).unwrap_err();
        let _ = std::fs::remove_file(&path);
        assert_eq!(error, ScenarioError::EmptyCapture { path });
    }

    #[test]
    fn reports_a_record_that_is_not_a_request_action() {
        let observation = observation_base64();
        let path = write_capture(
            "not-request-action",
            &[
                request_action_line(418, &observation),
                r#"{"type":"action_ack","request_id":418,"status":"accepted"}"#.to_string(),
            ],
        );
        let error = load_captured_scenario(&path, Some(418)).unwrap_err();
        let _ = std::fs::remove_file(&path);

        assert_eq!(
            error,
            ScenarioError::CaptureRecord {
                path,
                line: 2,
                source: CaptureRecordError::UnexpectedType(Some("action_ack".to_string())),
            }
        );
    }

    #[test]
    fn reports_an_undecodable_observation() {
        let path = write_capture(
            "broken-observation",
            &[request_action_line(419, "not-base64!!")],
        );
        let error = load_captured_scenario(&path, None).unwrap_err();
        let _ = std::fs::remove_file(&path);

        assert!(
            matches!(&error, ScenarioError::CaptureObservation { request_id, .. } if *request_id == 419),
            "{error:?}"
        );
    }

    #[test]
    fn header_shows_the_capture_source() {
        let observation = observation_base64();
        let path = write_capture("header", &[request_action_line(420, &observation)]);
        let captured = load_captured_scenario(&path, None).unwrap();
        let _ = std::fs::remove_file(&path);

        let header = captured.header();
        assert!(header.starts_with("RiichiLab capture\n"), "{header}");
        assert!(header.contains(&format!("  file: {path}")), "{header}");
        assert!(header.contains("  request_id: 420"), "{header}");
        assert!(header.contains("  actor: None"), "{header}");
        assert!(header.contains("  possible actions: 12"), "{header}");
        assert!(header.contains("  legal actions: 12"), "{header}");
    }
}
