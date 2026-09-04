use riichilab_client::{CaptureRecord, CapturedRequestAction, MjaiEvent, ValidationState};

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

#[derive(Debug, Clone, PartialEq)]
struct ReplayRequest {
    request_action: CapturedRequestAction,
    reaction_source_player: Option<u8>,
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
    let text = read_capture_file(path)?;
    captured_scenario_from_text(path, &text, request_id)
}

pub fn load_captured_scenarios(path: &str) -> Result<Vec<CapturedScenario>, ScenarioError> {
    let text = read_capture_file(path)?;
    captured_scenarios_from_text(path, &text)
}

fn captured_scenario_from_text(
    source: &str,
    text: &str,
    request_id: Option<u64>,
) -> Result<CapturedScenario, ScenarioError> {
    captured_scenario(
        source,
        select_record(source, parse_records(source, text)?, request_id)?,
    )
}

fn captured_scenarios_from_text(
    source: &str,
    text: &str,
) -> Result<Vec<CapturedScenario>, ScenarioError> {
    let records = parse_records(source, text)?;
    if records.is_empty() {
        return Err(ScenarioError::EmptyCapture {
            path: source.to_string(),
        });
    }

    records
        .into_iter()
        .map(|record| captured_scenario(source, record))
        .collect()
}

fn read_capture_file(path: &str) -> Result<String, ScenarioError> {
    std::fs::read_to_string(path).map_err(|error| ScenarioError::ReadFile {
        path: path.to_string(),
        message: error.to_string(),
    })
}

// session capture を順に読み、server event は live client と同じ ValidationState へ反映する。
// replay 対象は server の request_action だけだが、その時点の reaction source も一緒に保持する。
// client record と未知・不完全な server event から source を推測しない。envelope 自体が壊れて
// いる行は従来どおり error。
fn parse_records(path: &str, text: &str) -> Result<Vec<ReplayRequest>, ScenarioError> {
    let mut records = Vec::new();
    let mut state = ValidationState::new();
    for (index, line) in text.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        let capture_error = |source| ScenarioError::CaptureRecord {
            path: path.to_string(),
            line: index + 1,
            source,
        };
        let record = CaptureRecord::from_json_line(line).map_err(capture_error)?;
        if let Some(request_action) = record.request_action().map_err(capture_error)? {
            records.push(ReplayRequest {
                request_action,
                reaction_source_player: state.reaction_source_player(),
            });
        } else if let Some(event) = captured_server_event(&record) {
            state.on_event(&event);
        }
    }
    Ok(records)
}

fn captured_server_event(record: &CaptureRecord) -> Option<MjaiEvent> {
    serde_json::from_value(record.server_event()?.clone()).ok()
}

fn select_record(
    path: &str,
    mut records: Vec<ReplayRequest>,
    request_id: Option<u64>,
) -> Result<ReplayRequest, ScenarioError> {
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
        .find(|record| record.request_action.request_id == request_id)
        .ok_or(ScenarioError::CapturedRequestNotFound {
            path: path.to_string(),
            request_id,
        })
}

fn captured_scenario(path: &str, record: ReplayRequest) -> Result<CapturedScenario, ScenarioError> {
    let request_action = record.request_action;
    let context = request_action
        .game_context()
        .map_err(|error| ScenarioError::CaptureObservation {
            path: path.to_string(),
            request_id: request_action.request_id,
            message: error.to_string(),
        })?
        .with_reaction_source_player(record.reaction_source_player);

    Ok(CapturedScenario {
        path: path.to_string(),
        request_id: request_action.request_id,
        actor: request_action.actor,
        possible_action_count: request_action.possible_actions.len(),
        scenario: Scenario {
            context,
            legal_actions: request_action.legal_actions(),
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scenario::ScenarioSpec;
    use bot_core::{
        Agent, CallDecisionReason, CallIishantenComparison, LegalAction, MeldKind, PushPullMode,
        PushPullReason, ShantenAgent, player_threat_facts_from_context,
    };
    use bot_logic::{TileId, TileType};
    use riichilab_client::capture::{self, CaptureDirection};
    use riichilab_client::observation::{
        fixture_base64, fixture_base64_with_discards, fixture_base64_with_melds,
        fixture_base64_with_table_state_facts, fixture_meld, game_context_from_decoded_observation,
    };
    use riichilab_client::{
        CaptureRecordError, MjaiPossibleAction, ObservationPayload, build_response_for_request,
        checked_legal_action_to_mjai_action, possible_actions_to_legal_actions,
    };
    use tempfile::TempDir;

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

    fn request_action_event(request_id: u64, observation: &str) -> String {
        format!(
            r#"{{"type":"request_action","request_id":{request_id},"possible_actions":[{}],"observation":"{observation}"}}"#,
            possible_actions_json()
        )
    }

    fn server_line(event: &str) -> String {
        capture::record_line(CaptureDirection::Server, event).unwrap()
    }

    fn client_line(event: &str) -> String {
        capture::record_line(CaptureDirection::Client, event).unwrap()
    }

    fn request_action_line(request_id: u64, observation: &str) -> String {
        server_line(&request_action_event(request_id, observation))
    }

    fn iishanten_pon_request_action_line(request_id: u64) -> String {
        let mut discards: [Vec<u8>; 4] = Default::default();
        // 10枚の河で remaining_tiles = 60。最後の F が直前の reaction 対象。
        discards[1] = vec![112, 113, 114, 115, 116, 117, 118, 119, 120, 130];
        let observation = fixture_base64_with_discards(
            0,
            None,
            vec![4, 8, 12, 17, 20, 24, 56, 64, 76, 84, 108, 128, 129],
            vec![],
            discards,
        );
        server_line(&format!(
            r#"{{"type":"request_action","request_id":{request_id},"actor":0,"possible_actions":[{{"type":"pon","pai":"F","consumed":["F","F"]}},{{"type":"none"}}],"observation":"{observation}"}}"#
        ))
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

    const CAPTURE_SOURCE: &str = "fixture.jsonl";

    fn replay_capture(
        lines: &[String],
        request_id: Option<u64>,
    ) -> Result<CapturedScenario, ScenarioError> {
        let mut text = lines.join("\n");
        text.push('\n');
        captured_scenario_from_text(CAPTURE_SOURCE, &text, request_id)
    }

    #[test]
    fn replays_the_only_captured_request() {
        let observation = observation_base64();
        let captured = replay_capture(&[request_action_line(410, &observation)], None).unwrap();

        assert_eq!(captured.request_id, 410);
        assert_eq!(captured.possible_action_count, CAPTURED_DAHAI.len());
    }

    #[test]
    fn replay_context_matches_the_client_observation_decoding() {
        let observation = observation_base64();
        let captured = replay_capture(&[request_action_line(411, &observation)], None).unwrap();

        let decoded = ObservationPayload::new(observation).decode_4p().unwrap();
        assert_eq!(
            captured.scenario.context,
            game_context_from_decoded_observation(&decoded)
        );
        assert_eq!(
            captured.scenario.context.history_furiten(),
            bot_logic::HistoryFuritenFacts::default()
        );
    }

    #[test]
    fn replay_context_carries_the_captured_dahai_actor_as_reaction_source() {
        let observation = fixture_base64(0, None, CAPTURED_HAND.to_vec());
        let captured = replay_capture(
            &[
                server_line(r#"{"type":"dahai","actor":2,"pai":"4s"}"#),
                request_action_line(422, &observation),
            ],
            None,
        )
        .unwrap();

        assert_eq!(captured.scenario.context.reaction_source_player(), Some(2));
    }

    #[test]
    fn replay_context_has_no_reaction_source_after_a_tsumo() {
        let observation = observation_base64();
        let captured = replay_capture(
            &[
                server_line(r#"{"type":"dahai","actor":2,"pai":"4s"}"#),
                server_line(r#"{"type":"tsumo","actor":0,"pai":"6p"}"#),
                request_action_line(423, &observation),
            ],
            None,
        )
        .unwrap();

        assert_eq!(captured.scenario.context.reaction_source_player(), None);
    }

    #[test]
    fn replay_does_not_infer_a_reaction_source_from_a_legal_pon() {
        let captured = replay_capture(&[iishanten_pon_request_action_line(424)], None).unwrap();
        let diagnostic =
            ShantenAgent::diagnose(&captured.scenario.context, &captured.scenario.legal_actions);

        assert_eq!(captured.scenario.context.reaction_source_player(), None);
        assert_eq!(
            diagnostic.call.expect("call diagnostic").reason,
            CallDecisionReason::ReactionSourceUnknown
        );
    }

    #[test]
    fn replayed_iishanten_call_reaches_the_production_value_comparison() {
        let captured = replay_capture(
            &[
                server_line(r#"{"type":"dahai","actor":1,"pai":"F"}"#),
                iishanten_pon_request_action_line(425),
            ],
            None,
        )
        .unwrap();
        let diagnostic =
            ShantenAgent::diagnose(&captured.scenario.context, &captured.scenario.legal_actions);
        let call = diagnostic.call.expect("call diagnostic");
        let comparison = call.candidates[0]
            .iishanten_self_tsumo
            .expect("production comparison");

        assert_ne!(call.reason, CallDecisionReason::ReactionSourceUnknown);
        assert_eq!(comparison.reaction_source_player, Some(1));
        assert!(comparison.pass_expected_self_tsumo_value.is_some());
        assert!(comparison.call_expected_self_tsumo_value.is_some());
        assert_ne!(comparison.comparison, CallIishantenComparison::Unknown);
    }

    #[test]
    fn replay_context_keeps_the_captured_table_state() {
        let observation = fixture_base64_with_table_state_facts(
            1,
            Some(CAPTURED_DRAWN_TILE),
            CAPTURED_HAND.to_vec(),
            [12300, 28700, 40100, 18900],
            2,
            3,
            1,
            2,
            3,
        );
        let captured = replay_capture(&[request_action_line(413, &observation)], None).unwrap();

        let decoded = ObservationPayload::new(observation).decode_4p().unwrap();
        let context = &captured.scenario.context;
        assert_eq!(context.table_state(), decoded.table_state);
        assert_eq!(context.scores(), Some([12300, 28700, 40100, 18900]));
        assert_eq!(context.honba(), Some(2));
        assert_eq!(context.kyotaku_points(), Some(3000));
        assert_eq!(context.kyoku(), Some(4));
        // Observation は山の残り枚数を持たないが、見えている牌から復元できる。配牌 13枚 × 4人と
        // 王牌 14枚を除いた 70枚から、自分のツモ1枚を引いた 69枚。
        assert_eq!(context.remaining_tiles(), Some(69));
    }

    #[test]
    fn replay_shows_the_captured_table_state_in_the_diagnostics() {
        let observation = fixture_base64_with_table_state_facts(
            0,
            Some(CAPTURED_DRAWN_TILE),
            CAPTURED_HAND.to_vec(),
            [12300, 28700, 40100, 18900],
            2,
            3,
            1,
            2,
            3,
        );
        let captured = replay_capture(&[request_action_line(414, &observation)], None).unwrap();

        let diagnostic =
            ShantenAgent::diagnose(&captured.scenario.context, &captured.scenario.legal_actions);
        let output = crate::format::format_diagnostic(&captured.scenario, &diagnostic, false);

        assert!(output.contains("\n\nTable state\n"), "{output}");
        assert!(
            output.contains("  scores: 12300 / 28700 / 40100 / 18900"),
            "{output}"
        );
        assert!(output.contains("  honba: 2"), "{output}");
        assert!(output.contains("  kyotaku: 3000 points"), "{output}");
        assert!(output.contains("  kyoku: 4"), "{output}");
        assert!(output.contains("  remaining tiles: 69"), "{output}");
    }

    #[test]
    fn replay_legal_actions_match_the_client_conversion() {
        let observation = observation_base64();
        let captured = replay_capture(&[request_action_line(412, &observation)], None).unwrap();

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

    fn tile_types(tiles: &[TileId]) -> Vec<TileType> {
        tiles.iter().map(|tile| tile.tile_type()).collect()
    }

    // 打 W で 4p / 7p テンパイになる門前手。inline scenario ならリーチを自動生成する局面。
    const MENZEN_TENPAI_HAND: [u8; 13] = [0, 4, 8, 28, 29, 53, 56, 76, 80, 84, 96, 100, 104];

    const MENZEN_TENPAI_DRAWN_TILE: u8 = 116;

    const MENZEN_TENPAI_DAHAI: [&str; 13] = [
        "1m", "2m", "3m", "8m", "5p", "6p", "2s", "3s", "4s", "7s", "8s", "9s", "W",
    ];

    // capture の合法手は server の possible actions がそのまま source of truth で、局面から
    // リーチを足さない。同じ手牌の inline scenario ではリーチが自動生成される。
    #[test]
    fn replay_does_not_add_reach_to_the_captured_possible_actions() {
        let possible_actions = MENZEN_TENPAI_DAHAI
            .iter()
            .map(|pai| format!(r#"{{"type":"dahai","pai":"{pai}","tsumogiri":false}}"#))
            .collect::<Vec<_>>()
            .join(",");
        let observation = fixture_base64(
            0,
            Some(MENZEN_TENPAI_DRAWN_TILE),
            MENZEN_TENPAI_HAND.to_vec(),
        );
        let line = server_line(&format!(
            r#"{{"type":"request_action","request_id":430,"possible_actions":[{possible_actions}],"observation":"{observation}"}}"#
        ));
        let captured = replay_capture(&[line], None).unwrap();

        assert_eq!(
            captured.scenario.legal_actions.len(),
            MENZEN_TENPAI_DAHAI.len()
        );
        assert!(
            captured
                .scenario
                .legal_actions
                .iter()
                .all(|action| matches!(action, LegalAction::Dahai { .. }))
        );

        let inline = Scenario::resolve(&ScenarioSpec {
            hand: "12388m56p234789s".to_string(),
            draw: Some("W".to_string()),
            player_id: Some(0),
            ..ScenarioSpec::default()
        })
        .unwrap();
        // Observation は牌種で往復するので、物理牌ではなく牌種が一致することを確かめる。
        assert_eq!(
            tile_types(captured.scenario.context.hand_tiles()),
            tile_types(inline.context.hand_tiles())
        );
        assert_eq!(
            captured
                .scenario
                .context
                .drawn_tile()
                .map(TileId::tile_type),
            inline.context.drawn_tile().map(TileId::tile_type)
        );
        assert!(inline.legal_actions.contains(&LegalAction::Reach));
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
        let captured = replay_capture(&[request_action_line(413, &observation)], None).unwrap();

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
    fn reports_a_missing_capture_file() {
        let directory = TempDir::new().unwrap();
        let path = directory
            .path()
            .join("missing.jsonl")
            .to_str()
            .unwrap()
            .to_string();

        let error = load_captured_scenario(&path, None).unwrap_err();

        assert!(
            matches!(&error, ScenarioError::ReadFile { path: source, .. } if *source == path),
            "{error:?}"
        );
    }

    #[test]
    fn selects_a_record_of_a_capture_session_written_by_the_client() {
        let observation = observation_base64();
        let directory = TempDir::new().unwrap();
        let path = directory.path().join("capture.jsonl");

        let (mut capture, guard) = capture::init(Some(&path)).unwrap().unwrap();
        for request_id in [431, 432, 433] {
            capture.write_server_event(&request_action_event(request_id, &observation));
        }
        drop(capture);
        drop(guard);

        let path = path.to_str().unwrap().to_string();
        let captured = load_captured_scenario(&path, Some(432)).unwrap();
        let error = load_captured_scenario(&path, None).unwrap_err();

        assert_eq!(captured.request_id, 432);
        assert_eq!(captured.possible_action_count, CAPTURED_DAHAI.len());
        assert_eq!(error, ScenarioError::AmbiguousCapture { path, count: 3 });
    }

    #[test]
    fn replayed_selection_matches_the_client_response_for_the_same_request() {
        let observation = observation_base64();
        let captured = replay_capture(&[request_action_line(421, &observation)], None).unwrap();

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
        let captured = replay_capture(&[request_action_line(414, &observation)], None).unwrap();

        let context = &captured.scenario.context;
        let diagnostic = ShantenAgent::diagnose(context, &captured.scenario.legal_actions);
        let facts = &diagnostic.player_threats[1].facts;

        assert!(!context.any_opponent_reached());
        assert!(!facts.reached);
        assert_eq!(facts.meld_count, 2);
        assert_eq!(facts.open_meld_count, 2);
        assert_eq!(facts.is_opponent(), Some(true));
        assert_eq!(facts.value_honor_melds.dragon, 1);

        // 通常役牌1翻だけの2副露は Present。consumer は classification を共有し、従来の
        // High 向け Fold policy を適用しない。
        assert_eq!(facts.open_visible_han_proxy(), 1);
        assert_eq!(
            diagnostic.player_threats[1].open_hand_threat.level(),
            Some(bot_core::OpenHandThreatLevel::Present)
        );
        let decision = diagnostic.push_pull_decision.unwrap();
        assert_eq!(decision.mode, PushPullMode::Push);
        assert_eq!(decision.reason, PushPullReason::NoThreat);
    }

    #[test]
    fn selects_a_record_by_request_id() {
        let observation = observation_base64();
        let lines = [
            request_action_line(415, &observation),
            request_action_line(416, &observation),
            request_action_line(417, &observation),
        ];

        let captured = replay_capture(&lines, Some(416)).unwrap();
        assert_eq!(captured.request_id, 416);

        let error = replay_capture(&lines, Some(999)).unwrap_err();
        assert_eq!(
            error,
            ScenarioError::CapturedRequestNotFound {
                path: CAPTURE_SOURCE.to_string(),
                request_id: 999,
            }
        );

        let error = replay_capture(&lines, None).unwrap_err();
        assert_eq!(
            error,
            ScenarioError::AmbiguousCapture {
                path: CAPTURE_SOURCE.to_string(),
                count: 3,
            }
        );
    }

    #[test]
    fn reports_an_empty_capture_file() {
        let error = replay_capture(&[], None).unwrap_err();
        assert_eq!(
            error,
            ScenarioError::EmptyCapture {
                path: CAPTURE_SOURCE.to_string(),
            }
        );
    }

    // 正常な session capture は request_action の前後に server event と client action を大量に
    // 含む。server event は state へ反映するが、replay 対象として数えるのは request_action だけ。
    #[test]
    fn selects_only_the_request_action_while_processing_session_records() {
        let observation = observation_base64();
        let lines = [
            server_line(r#"{"type":"start_game","id":0}"#),
            server_line(r#"{"type":"start_kyoku","kyoku":1,"oya":0}"#),
            server_line(r#"{"type":"tsumo","actor":0,"pai":"5s"}"#),
            request_action_line(418, &observation),
            client_line(r#"{"type":"dahai","actor":0,"pai":"1m","request_id":418}"#),
            server_line(r#"{"type":"action_ack","request_id":418,"status":"accepted"}"#),
            server_line(r#"{"type":"dahai","actor":0,"pai":"1m","tsumogiri":false}"#),
            server_line(r#"{"type":"reach","actor":1}"#),
            server_line(r#"{"type":"hora","actor":1,"target":0,"pai":"1m"}"#),
            server_line(r#"{"type":"end_kyoku"}"#),
            server_line(r#"{"type":"end_game","scores":[25000,25000,25000,25000]}"#),
        ];

        let captured = replay_capture(&lines, None).unwrap();
        assert_eq!(captured.request_id, 418);
        assert_eq!(captured.possible_action_count, CAPTURED_DAHAI.len());

        let by_request_id = replay_capture(&lines, Some(418)).unwrap();
        assert_eq!(by_request_id, captured);
    }

    // client action と action_ack は同じ request_id を持つが、request selection の対象は
    // server の request_action だけ。
    #[test]
    fn does_not_count_client_actions_and_acks_as_requests() {
        let observation = observation_base64();
        let captured = replay_capture(
            &[
                request_action_line(419, &observation),
                client_line(&request_action_event(419, &observation)),
                client_line(r#"{"type":"reach","actor":0,"request_id":419}"#),
                server_line(r#"{"type":"action_ack","request_id":419,"status":"accepted"}"#),
            ],
            None,
        )
        .unwrap();

        assert_eq!(captured.request_id, 419);
    }

    #[test]
    fn reports_a_malformed_capture_envelope() {
        let observation = observation_base64();
        let error = replay_capture(
            &[
                request_action_line(420, &observation),
                r#"{"version":1,"event":{"type":"reach","actor":1}}"#.to_string(),
            ],
            Some(420),
        )
        .unwrap_err();

        assert!(
            matches!(
                &error,
                ScenarioError::CaptureRecord {
                    line: 2,
                    source: CaptureRecordError::Envelope(_),
                    ..
                }
            ),
            "{error:?}"
        );

        let error = replay_capture(&["{".to_string()], None).unwrap_err();
        assert!(
            matches!(
                &error,
                ScenarioError::CaptureRecord {
                    line: 1,
                    source: CaptureRecordError::Json(_),
                    ..
                }
            ),
            "{error:?}"
        );
    }

    // 旧 capture 形式 (1行 = request_action の raw JSON) は読まない。fallback を持たないことを
    // 固定する。
    #[test]
    fn does_not_read_the_old_raw_request_action_schema() {
        let observation = observation_base64();
        let error = replay_capture(&[request_action_event(421, &observation)], None).unwrap_err();

        assert!(
            matches!(
                &error,
                ScenarioError::CaptureRecord {
                    line: 1,
                    source: CaptureRecordError::Envelope(_),
                    ..
                }
            ),
            "{error:?}"
        );
    }

    #[test]
    fn reports_an_undecodable_observation() {
        let error = replay_capture(&[request_action_line(419, "not-base64!!")], None).unwrap_err();

        assert!(
            matches!(&error, ScenarioError::CaptureObservation { request_id, .. } if *request_id == 419),
            "{error:?}"
        );
    }

    #[test]
    fn header_shows_the_capture_source() {
        let observation = observation_base64();
        let captured = replay_capture(&[request_action_line(420, &observation)], None).unwrap();

        let header = captured.header();
        assert!(header.starts_with("RiichiLab capture\n"), "{header}");
        assert!(
            header.contains(&format!("  file: {CAPTURE_SOURCE}")),
            "{header}"
        );
        assert!(header.contains("  request_id: 420"), "{header}");
        assert!(header.contains("  actor: None"), "{header}");
        assert!(header.contains("  possible actions: 12"), "{header}");
        assert!(header.contains("  legal actions: 12"), "{header}");
    }
}
