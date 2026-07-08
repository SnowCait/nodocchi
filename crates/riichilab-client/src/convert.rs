use bot_core::{GameContext, LegalAction};
use bot_logic::{TileId, TileType};

use crate::protocol::{MjaiAction, MjaiPossibleAction};

pub fn possible_action_to_legal_action(action: &MjaiPossibleAction) -> Option<LegalAction> {
    match action {
        MjaiPossibleAction::Dahai { pai, .. } => {
            temporary_tile_id_from_mjai_pai(pai).map(|tile| LegalAction::Dahai { tile })
        }
        MjaiPossibleAction::Chi { pai, consumed } => {
            let tile = temporary_tile_id_from_mjai_pai(pai)?;
            let consumed = temporary_tile_ids_from_mjai_pais(consumed)?;
            Some(LegalAction::Chi { tile, consumed })
        }
        MjaiPossibleAction::Pon { pai, consumed } => {
            let tile = temporary_tile_id_from_mjai_pai(pai)?;
            let consumed = temporary_tile_ids_from_mjai_pais(consumed)?;
            Some(LegalAction::Pon { tile, consumed })
        }
        MjaiPossibleAction::Daiminkan { pai, consumed } => {
            let tile = temporary_tile_id_from_mjai_pai(pai)?;
            let consumed = temporary_tile_ids_from_mjai_pais(consumed)?;
            Some(LegalAction::Daiminkan { tile, consumed })
        }
        MjaiPossibleAction::Ankan { consumed } => {
            let consumed = temporary_tile_ids_from_mjai_pais(consumed)?;
            Some(LegalAction::Ankan { consumed })
        }
        MjaiPossibleAction::Kakan { pai, consumed } => {
            let tile = temporary_tile_id_from_mjai_pai(pai)?;
            let consumed = temporary_tile_ids_from_mjai_pais(consumed)?;
            Some(LegalAction::Kakan { tile, consumed })
        }
        MjaiPossibleAction::Reach => Some(LegalAction::Reach),
        MjaiPossibleAction::Hora => Some(LegalAction::Hora),
        MjaiPossibleAction::Ryukyoku => Some(LegalAction::Ryukyoku),
        MjaiPossibleAction::None => Some(LegalAction::None),
    }
}

pub fn legal_action_to_mjai_action(action: &LegalAction, actor: u8, request_id: u64) -> MjaiAction {
    match action {
        LegalAction::Dahai { tile } => MjaiAction::Dahai {
            actor,
            pai: tile.to_mjai_string(),
            tsumogiri: None,
            request_id: Some(request_id),
        },
        // RiichiLab では reach 宣言と dahai は別 request/response として扱われる。
        // そのため Reach action は打牌牌姿を持たず、後続の request_action で dahai を返す。
        LegalAction::Reach => MjaiAction::Reach {
            actor,
            request_id: Some(request_id),
        },
        LegalAction::Hora => MjaiAction::Hora {
            actor,
            target: None,
            pai: None,
            request_id: Some(request_id),
        },
        LegalAction::Ryukyoku => MjaiAction::Ryukyoku {
            request_id: Some(request_id),
        },
        // 副露・カン response は possible_actions の元 action と照合して構築する必要があるため、
        // possible_actions を参照しないこの単純変換では扱わず None を返す。
        // 実際の副露・カン response は checked_legal_action_to_mjai_action() で構築する。
        LegalAction::Chi { .. }
        | LegalAction::Pon { .. }
        | LegalAction::Daiminkan { .. }
        | LegalAction::Ankan { .. }
        | LegalAction::Kakan { .. }
        | LegalAction::None => MjaiAction::None {
            request_id: Some(request_id),
        },
    }
}

pub fn checked_legal_action_to_mjai_action(
    chosen: &LegalAction,
    actor: u8,
    request_id: u64,
    possible_actions: &[MjaiPossibleAction],
    context: &GameContext,
) -> Option<MjaiAction> {
    match chosen {
        LegalAction::Hora => possible_actions
            .iter()
            .any(|a| matches!(a, MjaiPossibleAction::Hora))
            .then_some(MjaiAction::Hora {
                actor,
                target: None,
                pai: None,
                request_id: Some(request_id),
            }),
        LegalAction::Ryukyoku => possible_actions
            .iter()
            .any(|a| matches!(a, MjaiPossibleAction::Ryukyoku))
            .then_some(MjaiAction::Ryukyoku {
                request_id: Some(request_id),
            }),
        LegalAction::Reach => possible_actions
            .iter()
            .any(|a| matches!(a, MjaiPossibleAction::Reach))
            .then_some(MjaiAction::Reach {
                actor,
                request_id: Some(request_id),
            }),
        LegalAction::None => possible_actions
            .iter()
            .any(|a| matches!(a, MjaiPossibleAction::None))
            .then_some(MjaiAction::None {
                request_id: Some(request_id),
            }),
        LegalAction::Dahai { tile } => possible_actions.iter().find_map(|a| {
            let MjaiPossibleAction::Dahai { pai, .. } = a else {
                return None;
            };
            (temporary_tile_id_from_mjai_pai(pai) == Some(*tile)).then(|| MjaiAction::Dahai {
                actor,
                pai: pai.clone(),
                tsumogiri: (context.drawn_tile() == Some(*tile)).then_some(true),
                request_id: Some(request_id),
            })
        }),
        // ankan response は actor / consumed のみで target を要さないため、
        // 対応する possible_actions の元 consumed 文字列を再利用して安全に構築できる。
        LegalAction::Ankan { consumed } => possible_actions.iter().find_map(|a| {
            let MjaiPossibleAction::Ankan {
                consumed: server_consumed,
            } = a
            else {
                return None;
            };
            mjai_pais_match_tile_ids(server_consumed, consumed).then(|| MjaiAction::Ankan {
                actor,
                consumed: server_consumed.clone(),
                request_id: Some(request_id),
            })
        }),
        // kakan response は actor / pai / consumed のみで target を要さないため、
        // 対応する possible_actions の元文字列を再利用して安全に構築できる。
        LegalAction::Kakan { tile, consumed } => possible_actions.iter().find_map(|a| {
            let MjaiPossibleAction::Kakan {
                pai,
                consumed: server_consumed,
            } = a
            else {
                return None;
            };
            (temporary_tile_id_from_mjai_pai(pai) == Some(*tile)
                && mjai_pais_match_tile_ids(server_consumed, consumed))
            .then(|| MjaiAction::Kakan {
                actor,
                pai: pai.clone(),
                consumed: server_consumed.clone(),
                request_id: Some(request_id),
            })
        }),
        // RiichiLab の Bot-to-Server response では chi / pon / daiminkan に target が必須だが、
        // request_action.possible_actions は target を含まない（MjaiPossibleAction にも target field がない）。
        // target を安全に得られないため、不完全な response で chombo を招かないよう response を返さない。
        LegalAction::Chi { .. } | LegalAction::Pon { .. } | LegalAction::Daiminkan { .. } => None,
    }
}

fn temporary_tile_ids_from_mjai_pais(pais: &[String]) -> Option<Vec<TileId>> {
    pais.iter()
        .map(|pai| temporary_tile_id_from_mjai_pai(pai))
        .collect()
}

fn mjai_pais_match_tile_ids(pais: &[String], tiles: &[TileId]) -> bool {
    let Some(mut converted) = temporary_tile_ids_from_mjai_pais(pais) else {
        return false;
    };
    let mut tiles = tiles.to_vec();
    converted.sort_unstable();
    tiles.sort_unstable();
    converted == tiles
}

pub fn possible_actions_to_legal_actions(actions: &[MjaiPossibleAction]) -> Vec<LegalAction> {
    actions
        .iter()
        .filter_map(possible_action_to_legal_action)
        .collect()
}

// TileRegistry実装までの一時措置。mjai牌文字列からは牌種しか分からないため、
// 赤5は固定ID、黒5は非赤の先頭ID、それ以外はその牌種の先頭IDを仮割当する。
pub(crate) fn temporary_tile_id_from_mjai_pai(pai: &str) -> Option<TileId> {
    match pai {
        "5mr" => return TileId::new(16),
        "5pr" => return TileId::new(52),
        "5sr" => return TileId::new(88),
        _ => {}
    }
    let tile_type = TileType::from_mjai_type_str(pai).ok()?;
    let base = tile_type.raw() * 4;
    let offset = u8::from(matches!(base, 16 | 52 | 88));
    TileId::new(base + offset)
}

pub(crate) fn temporary_tile_id_from_observation_tile(raw: u8) -> Option<TileId> {
    let tile = TileId::new(raw)?;
    if tile.is_red() {
        return Some(tile);
    }
    let base = tile.tile_type().raw() * 4;
    let offset = u8::from(matches!(base, 16 | 52 | 88));
    TileId::new(base + offset)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::MjaiEvent;
    use bot_core::{Agent, GameContext, NormalAgent};

    fn tile(value: u8) -> TileId {
        TileId::new(value).unwrap()
    }

    fn possible_dahai(pai: &str) -> MjaiPossibleAction {
        MjaiPossibleAction::Dahai {
            pai: pai.to_string(),
            tsumogiri: None,
        }
    }

    fn possible_pon() -> MjaiPossibleAction {
        MjaiPossibleAction::Pon {
            pai: "E".to_string(),
            consumed: vec!["E".to_string(), "E".to_string()],
        }
    }

    #[test]
    fn dahai_converts_with_temporary_tile_id() {
        for (pai, expected) in [
            ("5mr", 16),
            ("5pr", 52),
            ("5sr", 88),
            ("5m", 17),
            ("5p", 53),
            ("5s", 89),
            ("1m", 0),
            ("9s", 104),
            ("C", 132),
        ] {
            let action = MjaiPossibleAction::Dahai {
                pai: pai.to_string(),
                tsumogiri: None,
            };
            assert_eq!(
                possible_action_to_legal_action(&action),
                Some(LegalAction::Dahai {
                    tile: tile(expected)
                }),
                "pai: {pai}"
            );
        }
    }

    #[test]
    fn observation_tile_normalizes_to_temporary_tile_id() {
        for (raw, expected) in [
            (0, 0),
            (3, 0),
            (16, 16),
            (17, 17),
            (19, 17),
            (52, 52),
            (55, 53),
            (88, 88),
            (91, 89),
            (56, 56),
            (59, 56),
            (104, 104),
            (132, 132),
            (135, 132),
        ] {
            assert_eq!(
                temporary_tile_id_from_observation_tile(raw),
                TileId::new(expected),
                "raw: {raw}"
            );
        }
    }

    #[test]
    fn observation_tile_out_of_range_is_none() {
        assert_eq!(temporary_tile_id_from_observation_tile(136), None);
        assert_eq!(temporary_tile_id_from_observation_tile(255), None);
    }

    #[test]
    fn dahai_with_invalid_pai_converts_to_none() {
        let action = MjaiPossibleAction::Dahai {
            pai: "invalid".to_string(),
            tsumogiri: None,
        };
        assert_eq!(possible_action_to_legal_action(&action), None);
    }

    #[test]
    fn non_dahai_actions_convert_directly() {
        for (action, expected) in [
            (MjaiPossibleAction::Reach, LegalAction::Reach),
            (MjaiPossibleAction::Hora, LegalAction::Hora),
            (MjaiPossibleAction::Ryukyoku, LegalAction::Ryukyoku),
            (MjaiPossibleAction::None, LegalAction::None),
        ] {
            assert_eq!(possible_action_to_legal_action(&action), Some(expected));
        }
    }

    #[test]
    fn chi_converts_to_legal_chi() {
        let action = MjaiPossibleAction::Chi {
            pai: "5m".to_string(),
            consumed: vec!["4m".to_string(), "6m".to_string()],
        };
        assert_eq!(
            possible_action_to_legal_action(&action),
            Some(LegalAction::Chi {
                tile: tile(17),
                consumed: vec![tile(12), tile(20)],
            })
        );
    }

    #[test]
    fn pon_converts_to_legal_pon() {
        let action = MjaiPossibleAction::Pon {
            pai: "E".to_string(),
            consumed: vec!["E".to_string(), "E".to_string()],
        };
        assert_eq!(
            possible_action_to_legal_action(&action),
            Some(LegalAction::Pon {
                tile: tile(108),
                consumed: vec![tile(108), tile(108)],
            })
        );
    }

    #[test]
    fn daiminkan_converts_to_legal_daiminkan() {
        let action = MjaiPossibleAction::Daiminkan {
            pai: "9s".to_string(),
            consumed: vec!["9s".to_string(), "9s".to_string(), "9s".to_string()],
        };
        assert_eq!(
            possible_action_to_legal_action(&action),
            Some(LegalAction::Daiminkan {
                tile: tile(104),
                consumed: vec![tile(104), tile(104), tile(104)],
            })
        );
    }

    #[test]
    fn ankan_converts_to_legal_ankan() {
        let action = MjaiPossibleAction::Ankan {
            consumed: vec![
                "1s".to_string(),
                "1s".to_string(),
                "1s".to_string(),
                "1s".to_string(),
            ],
        };
        assert_eq!(
            possible_action_to_legal_action(&action),
            Some(LegalAction::Ankan {
                consumed: vec![tile(72), tile(72), tile(72), tile(72)],
            })
        );
    }

    #[test]
    fn kakan_converts_to_legal_kakan() {
        let action = MjaiPossibleAction::Kakan {
            pai: "P".to_string(),
            consumed: vec!["P".to_string(), "P".to_string(), "P".to_string()],
        };
        assert_eq!(
            possible_action_to_legal_action(&action),
            Some(LegalAction::Kakan {
                tile: tile(124),
                consumed: vec![tile(124), tile(124), tile(124)],
            })
        );
    }

    #[test]
    fn chi_with_red_five_preserves_red_tile_id() {
        let action = MjaiPossibleAction::Chi {
            pai: "5mr".to_string(),
            consumed: vec!["4m".to_string(), "6m".to_string()],
        };
        assert_eq!(
            possible_action_to_legal_action(&action),
            Some(LegalAction::Chi {
                tile: tile(16),
                consumed: vec![tile(12), tile(20)],
            })
        );
    }

    #[test]
    fn chi_with_invalid_pai_converts_to_none() {
        let action = MjaiPossibleAction::Chi {
            pai: "invalid".to_string(),
            consumed: vec!["4m".to_string(), "6m".to_string()],
        };
        assert_eq!(possible_action_to_legal_action(&action), None);
    }

    #[test]
    fn chi_with_invalid_consumed_converts_to_none() {
        let action = MjaiPossibleAction::Chi {
            pai: "5m".to_string(),
            consumed: vec!["4m".to_string(), "invalid".to_string()],
        };
        assert_eq!(possible_action_to_legal_action(&action), None);
    }

    #[test]
    fn ankan_with_invalid_consumed_converts_to_none() {
        let action = MjaiPossibleAction::Ankan {
            consumed: vec![
                "1s".to_string(),
                "1s".to_string(),
                "1s".to_string(),
                "invalid".to_string(),
            ],
        };
        assert_eq!(possible_action_to_legal_action(&action), None);
    }

    #[test]
    fn claim_actions_are_not_dropped() {
        let actions = vec![
            MjaiPossibleAction::Chi {
                pai: "5m".to_string(),
                consumed: vec!["4m".to_string(), "6m".to_string()],
            },
            MjaiPossibleAction::Pon {
                pai: "E".to_string(),
                consumed: vec!["E".to_string(), "E".to_string()],
            },
            MjaiPossibleAction::Daiminkan {
                pai: "9s".to_string(),
                consumed: vec!["9s".to_string(), "9s".to_string(), "9s".to_string()],
            },
            MjaiPossibleAction::Ankan {
                consumed: vec![
                    "1s".to_string(),
                    "1s".to_string(),
                    "1s".to_string(),
                    "1s".to_string(),
                ],
            },
            MjaiPossibleAction::Kakan {
                pai: "P".to_string(),
                consumed: vec!["P".to_string(), "P".to_string(), "P".to_string()],
            },
            MjaiPossibleAction::None,
        ];
        let legal_actions = possible_actions_to_legal_actions(&actions);
        assert_eq!(legal_actions.len(), 6);
        assert!(matches!(legal_actions[0], LegalAction::Chi { .. }));
        assert!(matches!(legal_actions[1], LegalAction::Pon { .. }));
        assert!(matches!(legal_actions[2], LegalAction::Daiminkan { .. }));
        assert!(matches!(legal_actions[3], LegalAction::Ankan { .. }));
        assert!(matches!(legal_actions[4], LegalAction::Kakan { .. }));
        assert_eq!(legal_actions[5], LegalAction::None);
    }

    #[test]
    fn possible_actions_skip_unconvertible_ones() {
        let actions = vec![
            MjaiPossibleAction::None,
            MjaiPossibleAction::Dahai {
                pai: "invalid".to_string(),
                tsumogiri: None,
            },
            MjaiPossibleAction::Dahai {
                pai: "1m".to_string(),
                tsumogiri: None,
            },
        ];
        assert_eq!(
            possible_actions_to_legal_actions(&actions),
            vec![LegalAction::None, LegalAction::Dahai { tile: tile(0) }]
        );
    }

    #[test]
    fn legal_action_to_mjai_action_echoes_request_id() {
        for action in [
            LegalAction::Dahai { tile: tile(16) },
            LegalAction::Reach,
            LegalAction::Hora,
            LegalAction::Ryukyoku,
            LegalAction::None,
        ] {
            let mjai = legal_action_to_mjai_action(&action, 0, 42);
            let json = serde_json::to_value(&mjai).unwrap();
            assert_eq!(json["request_id"], 42, "action: {action:?}");
        }
    }

    #[test]
    fn reach_response_excludes_discard_fields() {
        let mjai = legal_action_to_mjai_action(&LegalAction::Reach, 0, 42);
        let json = serde_json::to_value(&mjai).unwrap();
        assert_eq!(json["type"], "reach");
        assert_eq!(json["actor"], 0);
        assert_eq!(json["request_id"], 42);
        assert!(json.get("pai").is_none());
        assert!(json.get("tsumogiri").is_none());
    }

    #[test]
    fn reach_serializes_without_discard_information() {
        let mjai = legal_action_to_mjai_action(&LegalAction::Reach, 0, 42);
        assert_eq!(
            serde_json::to_string(&mjai).unwrap(),
            r#"{"type":"reach","actor":0,"request_id":42}"#
        );
    }

    #[test]
    fn legal_action_to_mjai_action_sets_actor() {
        assert_eq!(
            legal_action_to_mjai_action(&LegalAction::Dahai { tile: tile(16) }, 3, 1),
            MjaiAction::Dahai {
                actor: 3,
                pai: "5mr".to_string(),
                tsumogiri: None,
                request_id: Some(1),
            }
        );
        assert_eq!(
            legal_action_to_mjai_action(&LegalAction::Reach, 2, 1),
            MjaiAction::Reach {
                actor: 2,
                request_id: Some(1),
            }
        );
        assert_eq!(
            legal_action_to_mjai_action(&LegalAction::Hora, 1, 1),
            MjaiAction::Hora {
                actor: 1,
                target: None,
                pai: None,
                request_id: Some(1),
            }
        );
    }

    mod checked_conversion {
        use super::*;

        #[test]
        fn hora_is_returned_only_when_possible() {
            let context = GameContext::default();
            assert_eq!(
                checked_legal_action_to_mjai_action(
                    &LegalAction::Hora,
                    1,
                    50,
                    &[MjaiPossibleAction::Hora],
                    &context,
                ),
                Some(MjaiAction::Hora {
                    actor: 1,
                    target: None,
                    pai: None,
                    request_id: Some(50),
                })
            );
            assert_eq!(
                checked_legal_action_to_mjai_action(
                    &LegalAction::Hora,
                    1,
                    50,
                    &[MjaiPossibleAction::None],
                    &context,
                ),
                None
            );
        }

        #[test]
        fn ryukyoku_is_returned_only_when_possible() {
            let context = GameContext::default();
            assert_eq!(
                checked_legal_action_to_mjai_action(
                    &LegalAction::Ryukyoku,
                    0,
                    51,
                    &[MjaiPossibleAction::Ryukyoku],
                    &context,
                ),
                Some(MjaiAction::Ryukyoku {
                    request_id: Some(51),
                })
            );
            assert_eq!(
                checked_legal_action_to_mjai_action(
                    &LegalAction::Ryukyoku,
                    0,
                    51,
                    &[MjaiPossibleAction::None],
                    &context,
                ),
                None
            );
        }

        #[test]
        fn reach_is_returned_only_when_possible() {
            let context = GameContext::default();
            assert_eq!(
                checked_legal_action_to_mjai_action(
                    &LegalAction::Reach,
                    2,
                    52,
                    &[MjaiPossibleAction::Reach],
                    &context,
                ),
                Some(MjaiAction::Reach {
                    actor: 2,
                    request_id: Some(52),
                })
            );
            assert_eq!(
                checked_legal_action_to_mjai_action(
                    &LegalAction::Reach,
                    2,
                    52,
                    &[MjaiPossibleAction::None],
                    &context,
                ),
                None
            );
        }

        #[test]
        fn none_is_returned_only_when_possible() {
            let context = GameContext::default();
            assert_eq!(
                checked_legal_action_to_mjai_action(
                    &LegalAction::None,
                    0,
                    53,
                    &[MjaiPossibleAction::None],
                    &context,
                ),
                Some(MjaiAction::None {
                    request_id: Some(53),
                })
            );
            assert_eq!(
                checked_legal_action_to_mjai_action(
                    &LegalAction::None,
                    0,
                    53,
                    &[possible_pon()],
                    &context,
                ),
                None
            );
        }

        #[test]
        fn dahai_is_returned_only_when_matching_dahai_is_possible() {
            let context = GameContext::default();
            let chosen = LegalAction::Dahai { tile: tile(0) };
            assert_eq!(
                checked_legal_action_to_mjai_action(
                    &chosen,
                    0,
                    54,
                    &[possible_dahai("1m")],
                    &context,
                ),
                Some(MjaiAction::Dahai {
                    actor: 0,
                    pai: "1m".to_string(),
                    tsumogiri: None,
                    request_id: Some(54),
                })
            );
            assert_eq!(
                checked_legal_action_to_mjai_action(
                    &chosen,
                    0,
                    54,
                    &[possible_dahai("9s")],
                    &context,
                ),
                None
            );
        }

        #[test]
        fn dahai_matching_drawn_tile_is_tsumogiri() {
            let context = GameContext::with_drawn_tile(tile(56));
            let chosen = LegalAction::Dahai { tile: tile(56) };
            assert_eq!(
                checked_legal_action_to_mjai_action(
                    &chosen,
                    2,
                    55,
                    &[possible_dahai("6p")],
                    &context,
                ),
                Some(MjaiAction::Dahai {
                    actor: 2,
                    pai: "6p".to_string(),
                    tsumogiri: Some(true),
                    request_id: Some(55),
                })
            );
        }

        #[test]
        fn dahai_not_matching_drawn_tile_is_not_tsumogiri() {
            let context = GameContext::with_drawn_tile(tile(56));
            let chosen = LegalAction::Dahai { tile: tile(0) };
            assert_eq!(
                checked_legal_action_to_mjai_action(
                    &chosen,
                    2,
                    56,
                    &[possible_dahai("1m")],
                    &context,
                ),
                Some(MjaiAction::Dahai {
                    actor: 2,
                    pai: "1m".to_string(),
                    tsumogiri: None,
                    request_id: Some(56),
                })
            );
        }

        #[test]
        fn echoes_request_id_for_all_actions() {
            let context = GameContext::default();
            let possible_actions = vec![
                possible_dahai("1m"),
                MjaiPossibleAction::Reach,
                MjaiPossibleAction::Hora,
                MjaiPossibleAction::Ryukyoku,
                MjaiPossibleAction::None,
            ];
            for chosen in [
                LegalAction::Dahai { tile: tile(0) },
                LegalAction::Reach,
                LegalAction::Hora,
                LegalAction::Ryukyoku,
                LegalAction::None,
            ] {
                for request_id in [0u64, 7, u64::MAX] {
                    let response = checked_legal_action_to_mjai_action(
                        &chosen,
                        0,
                        request_id,
                        &possible_actions,
                        &context,
                    )
                    .unwrap();
                    let json = serde_json::to_value(&response).unwrap();
                    assert_eq!(json["request_id"], request_id, "action: {chosen:?}");
                }
            }
        }

        fn possible_ankan() -> MjaiPossibleAction {
            MjaiPossibleAction::Ankan {
                consumed: vec![
                    "1s".to_string(),
                    "1s".to_string(),
                    "1s".to_string(),
                    "1s".to_string(),
                ],
            }
        }

        fn legal_ankan() -> LegalAction {
            LegalAction::Ankan {
                consumed: vec![tile(72), tile(72), tile(72), tile(72)],
            }
        }

        fn possible_kakan() -> MjaiPossibleAction {
            MjaiPossibleAction::Kakan {
                pai: "P".to_string(),
                consumed: vec!["P".to_string(), "P".to_string(), "P".to_string()],
            }
        }

        fn legal_kakan() -> LegalAction {
            LegalAction::Kakan {
                tile: tile(124),
                consumed: vec![tile(124), tile(124), tile(124)],
            }
        }

        #[test]
        fn ankan_is_returned_only_when_possible() {
            let context = GameContext::default();
            assert_eq!(
                checked_legal_action_to_mjai_action(
                    &legal_ankan(),
                    1,
                    60,
                    &[possible_ankan()],
                    &context,
                ),
                Some(MjaiAction::Ankan {
                    actor: 1,
                    consumed: vec![
                        "1s".to_string(),
                        "1s".to_string(),
                        "1s".to_string(),
                        "1s".to_string(),
                    ],
                    request_id: Some(60),
                })
            );
            assert_eq!(
                checked_legal_action_to_mjai_action(
                    &legal_ankan(),
                    1,
                    60,
                    &[MjaiPossibleAction::None],
                    &context,
                ),
                None
            );
        }

        #[test]
        fn ankan_response_excludes_pai() {
            let context = GameContext::default();
            let response = checked_legal_action_to_mjai_action(
                &legal_ankan(),
                0,
                61,
                &[possible_ankan()],
                &context,
            )
            .unwrap();
            let json = serde_json::to_value(&response).unwrap();
            assert_eq!(json["type"], "ankan");
            assert_eq!(json["actor"], 0);
            assert_eq!(json["request_id"], 61);
            assert!(json.get("pai").is_none());
            assert_eq!(
                serde_json::to_string(&response).unwrap(),
                r#"{"type":"ankan","actor":0,"consumed":["1s","1s","1s","1s"],"request_id":61}"#
            );
        }

        #[test]
        fn kakan_is_returned_only_when_possible() {
            let context = GameContext::default();
            assert_eq!(
                checked_legal_action_to_mjai_action(
                    &legal_kakan(),
                    2,
                    62,
                    &[possible_kakan()],
                    &context,
                ),
                Some(MjaiAction::Kakan {
                    actor: 2,
                    pai: "P".to_string(),
                    consumed: vec!["P".to_string(), "P".to_string(), "P".to_string()],
                    request_id: Some(62),
                })
            );
            assert_eq!(
                checked_legal_action_to_mjai_action(
                    &legal_kakan(),
                    2,
                    62,
                    &[MjaiPossibleAction::None],
                    &context,
                ),
                None
            );
        }

        #[test]
        fn kakan_response_includes_pai() {
            let context = GameContext::default();
            let response = checked_legal_action_to_mjai_action(
                &legal_kakan(),
                0,
                63,
                &[possible_kakan()],
                &context,
            )
            .unwrap();
            assert_eq!(
                serde_json::to_string(&response).unwrap(),
                r#"{"type":"kakan","actor":0,"pai":"P","consumed":["P","P","P"],"request_id":63}"#
            );
        }

        // chi / pon / daiminkan は target を安全に得られないため response を返さない。
        #[test]
        fn chi_pon_daiminkan_are_not_returned_even_when_possible() {
            let context = GameContext::default();

            let chi_chosen = LegalAction::Chi {
                tile: tile(17),
                consumed: vec![tile(12), tile(20)],
            };
            let chi_possible = MjaiPossibleAction::Chi {
                pai: "5m".to_string(),
                consumed: vec!["4m".to_string(), "6m".to_string()],
            };
            assert_eq!(
                checked_legal_action_to_mjai_action(&chi_chosen, 0, 64, &[chi_possible], &context,),
                None
            );

            let pon_chosen = LegalAction::Pon {
                tile: tile(108),
                consumed: vec![tile(108), tile(108)],
            };
            assert_eq!(
                checked_legal_action_to_mjai_action(
                    &pon_chosen,
                    0,
                    64,
                    &[possible_pon()],
                    &context,
                ),
                None
            );

            let daiminkan_chosen = LegalAction::Daiminkan {
                tile: tile(104),
                consumed: vec![tile(104), tile(104), tile(104)],
            };
            let daiminkan_possible = MjaiPossibleAction::Daiminkan {
                pai: "9s".to_string(),
                consumed: vec!["9s".to_string(), "9s".to_string(), "9s".to_string()],
            };
            assert_eq!(
                checked_legal_action_to_mjai_action(
                    &daiminkan_chosen,
                    0,
                    64,
                    &[daiminkan_possible],
                    &context,
                ),
                None
            );
        }

        #[test]
        fn kan_not_in_possible_actions_returns_none() {
            let context = GameContext::default();
            assert_eq!(
                checked_legal_action_to_mjai_action(
                    &legal_ankan(),
                    0,
                    65,
                    &[possible_kakan()],
                    &context,
                ),
                None
            );
            assert_eq!(
                checked_legal_action_to_mjai_action(
                    &legal_kakan(),
                    0,
                    65,
                    &[possible_ankan()],
                    &context,
                ),
                None
            );
        }
    }

    #[test]
    fn official_request_action_flows_to_hora_response() {
        let json = r#"{
            "type": "request_action",
            "request_id": 42,
            "possible_actions": [
                {"type": "dahai", "pai": "1m"},
                {"type": "dahai", "pai": "3m"},
                {"type": "reach"},
                {"type": "hora"},
                {"type": "none"}
            ],
            "observation": "dummy-base64"
        }"#;
        let event: MjaiEvent = serde_json::from_str(json).unwrap();
        let MjaiEvent::RequestAction {
            request_id,
            possible_actions,
            ..
        } = event
        else {
            panic!("expected request_action");
        };
        let legal_actions = possible_actions_to_legal_actions(&possible_actions);
        assert_eq!(
            legal_actions,
            vec![
                LegalAction::Dahai { tile: tile(0) },
                LegalAction::Dahai { tile: tile(8) },
                LegalAction::Reach,
                LegalAction::Hora,
                LegalAction::None,
            ]
        );
        let mut agent = NormalAgent;
        let chosen = agent.act(&GameContext::default(), &legal_actions);
        assert_eq!(chosen, LegalAction::Hora);
        let response = legal_action_to_mjai_action(&chosen, 0, request_id);
        assert_eq!(
            serde_json::to_string(&response).unwrap(),
            r#"{"type":"hora","actor":0,"request_id":42}"#
        );
    }

    #[test]
    fn claim_request_action_flows_to_none_response() {
        let json = r#"{
            "type": "request_action",
            "request_id": 44,
            "possible_actions": [
                {"type":"pon","pai":"E","consumed":["E","E"]},
                {"type":"none"}
            ],
            "observation": "dummy-base64"
        }"#;
        let event: MjaiEvent = serde_json::from_str(json).unwrap();
        let MjaiEvent::RequestAction {
            request_id,
            possible_actions,
            ..
        } = event
        else {
            panic!("expected request_action");
        };
        let legal_actions = possible_actions_to_legal_actions(&possible_actions);
        // 副露 action は情報を落とさず LegalAction として保持される。
        assert_eq!(
            legal_actions,
            vec![
                LegalAction::Pon {
                    tile: tile(108),
                    consumed: vec![tile(108), tile(108)],
                },
                LegalAction::None,
            ]
        );
        // ただし現状の Agent は副露を積極選択せず None を返す。
        let context = GameContext::default();
        let mut agent = NormalAgent;
        let chosen = agent.act(&context, &legal_actions);
        assert_eq!(chosen, LegalAction::None);
        let response =
            checked_legal_action_to_mjai_action(&chosen, 0, request_id, &possible_actions, &context);
        assert_eq!(
            serde_json::to_string(&response.unwrap()).unwrap(),
            r#"{"type":"none","request_id":44}"#
        );
    }

    #[test]
    fn request_action_flows_to_dahai_response() {
        let json = r#"{
            "type": "request_action",
            "request_id": 43,
            "possible_actions": [
                {"type": "none"},
                {"type": "dahai", "pai": "5mr"}
            ],
            "observation": "dummy-base64"
        }"#;
        let event: MjaiEvent = serde_json::from_str(json).unwrap();
        let MjaiEvent::RequestAction {
            request_id,
            possible_actions,
            ..
        } = event
        else {
            panic!("expected request_action");
        };
        let legal_actions = possible_actions_to_legal_actions(&possible_actions);
        assert_eq!(
            legal_actions,
            vec![LegalAction::None, LegalAction::Dahai { tile: tile(16) }]
        );
        let mut agent = NormalAgent;
        let chosen = agent.act(&GameContext::default(), &legal_actions);
        assert_eq!(chosen, LegalAction::Dahai { tile: tile(16) });
        let response = legal_action_to_mjai_action(&chosen, 0, request_id);
        assert_eq!(
            serde_json::to_string(&response).unwrap(),
            r#"{"type":"dahai","actor":0,"pai":"5mr","request_id":43}"#
        );
    }
}
