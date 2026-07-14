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

// possible_actions を参照しない単純変換（unchecked / simple conversion）。
// possible_actions と照合しないため、response を組み立てるだけで合法性は保証しない。
// runtime 経路では必ず checked_legal_action_to_mjai_action() を使うこと。
//
// 副露・カン variant を silently MjaiAction::None に潰さないよう、戻り値は Option とする。
// 変換できない action では None を返し、誤って none response が構築されたように
// 見える状態を避ける。
pub fn legal_action_to_mjai_action(
    action: &LegalAction,
    actor: u8,
    request_id: u64,
) -> Option<MjaiAction> {
    match action {
        LegalAction::Dahai { tile } => Some(MjaiAction::Dahai {
            actor,
            pai: tile.to_mjai_string(),
            tsumogiri: None,
            request_id: Some(request_id),
        }),
        // RiichiLab では reach 宣言と dahai は別 request/response として扱われる。
        // そのため Reach action は打牌牌姿を持たず、後続の request_action で dahai を返す。
        LegalAction::Reach => Some(MjaiAction::Reach {
            actor,
            request_id: Some(request_id),
        }),
        LegalAction::Hora => Some(MjaiAction::Hora {
            actor,
            target: None,
            pai: None,
            request_id: Some(request_id),
        }),
        LegalAction::Ryukyoku => Some(MjaiAction::Ryukyoku {
            request_id: Some(request_id),
        }),
        LegalAction::None => Some(MjaiAction::None {
            request_id: Some(request_id),
        }),
        // 副露・カン response は possible_actions の元 action と照合して構築する必要があるため、
        // possible_actions を参照しないこの単純変換では扱わず None を返す。
        // - Chi / Pon / Daiminkan は target を安全に得られない
        // - Ankan / Kakan は response 自体は作れるが、server が提示した元文字列と
        //   照合して構築する方が安全
        // これらの副露・カン response は checked_legal_action_to_mjai_action() に寄せる。
        LegalAction::Chi { .. }
        | LegalAction::Pon { .. }
        | LegalAction::Daiminkan { .. }
        | LegalAction::Ankan { .. }
        | LegalAction::Kakan { .. } => None,
    }
}

// possible_actions に基づき、chombo risk が低い fallback response を選ぶ。
//
// fallback 優先順位:
//   1. None      (claim opportunity では最も安全)
//   2. Dahai     (draw turn の基本 fallback。ツモ切り)
//   3. Ryukyoku
//   4. Hora
//   5. Reach
//   6. Ankan
//   7. Kakan
//   8. それ以外   (Chi / Pon / Daiminkan は target を得られないため None)
//
// possible_actions に無い action を送らないよう、必ず possible_actions を走査して
// 対応する response のみ構築する。合法な fallback が無い場合は None を返し、
// 呼び出し側で「返信しない」判断を行えるようにする。
pub fn fallback_mjai_action_from_possible_actions(
    actor: u8,
    request_id: u64,
    possible_actions: &[MjaiPossibleAction],
    context: &GameContext,
) -> Option<MjaiAction> {
    // 1. None
    if possible_actions
        .iter()
        .any(|a| matches!(a, MjaiPossibleAction::None))
    {
        return Some(MjaiAction::None {
            request_id: Some(request_id),
        });
    }

    // 2. Dahai (server が提示した pai をそのまま使う。ツモ切り一致時のみ tsumogiri: true)
    if let Some(response) = possible_actions.iter().find_map(|a| {
        let MjaiPossibleAction::Dahai { pai, .. } = a else {
            return None;
        };
        let tsumogiri = temporary_tile_id_from_mjai_pai(pai)
            .is_some_and(|tile| context.drawn_tile() == Some(tile))
            .then_some(true);
        Some(MjaiAction::Dahai {
            actor,
            pai: pai.clone(),
            tsumogiri,
            request_id: Some(request_id),
        })
    }) {
        return Some(response);
    }

    // 3. Ryukyoku
    if possible_actions
        .iter()
        .any(|a| matches!(a, MjaiPossibleAction::Ryukyoku))
    {
        return Some(MjaiAction::Ryukyoku {
            request_id: Some(request_id),
        });
    }

    // 4. Hora
    if possible_actions
        .iter()
        .any(|a| matches!(a, MjaiPossibleAction::Hora))
    {
        return Some(MjaiAction::Hora {
            actor,
            target: None,
            pai: None,
            request_id: Some(request_id),
        });
    }

    // 5. Reach
    if possible_actions
        .iter()
        .any(|a| matches!(a, MjaiPossibleAction::Reach))
    {
        return Some(MjaiAction::Reach {
            actor,
            request_id: Some(request_id),
        });
    }

    // 6. Ankan (server の元 consumed 文字列を再利用して構築)
    if let Some(response) = possible_actions.iter().find_map(|a| {
        let MjaiPossibleAction::Ankan { consumed } = a else {
            return None;
        };
        Some(MjaiAction::Ankan {
            actor,
            consumed: consumed.clone(),
            request_id: Some(request_id),
        })
    }) {
        return Some(response);
    }

    // 7. Kakan (server の元 pai / consumed 文字列を再利用して構築)
    if let Some(response) = possible_actions.iter().find_map(|a| {
        let MjaiPossibleAction::Kakan { pai, consumed } = a else {
            return None;
        };
        Some(MjaiAction::Kakan {
            actor,
            pai: pai.clone(),
            consumed: consumed.clone(),
            request_id: Some(request_id),
        })
    }) {
        return Some(response);
    }

    // 8. Chi / Pon / Daiminkan は target を安全に得られないため fallback 対象外。
    None
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
            let mjai = legal_action_to_mjai_action(&action, 0, 42).unwrap();
            let json = serde_json::to_value(&mjai).unwrap();
            assert_eq!(json["request_id"], 42, "action: {action:?}");
        }
    }

    #[test]
    fn simple_conversion_does_not_silently_map_melds_and_kans_to_none() {
        // 副露・カンは simple conversion では None を返し、
        // 見かけ上 MjaiAction::None に成功変換されない。
        for action in [
            LegalAction::Chi {
                tile: tile(17),
                consumed: vec![tile(12), tile(20)],
            },
            LegalAction::Pon {
                tile: tile(108),
                consumed: vec![tile(108), tile(108)],
            },
            LegalAction::Daiminkan {
                tile: tile(104),
                consumed: vec![tile(104), tile(104), tile(104)],
            },
            LegalAction::Ankan {
                consumed: vec![tile(72), tile(72), tile(72), tile(72)],
            },
            LegalAction::Kakan {
                tile: tile(124),
                consumed: vec![tile(124), tile(124), tile(124)],
            },
        ] {
            assert_eq!(
                legal_action_to_mjai_action(&action, 0, 42),
                None,
                "action: {action:?}"
            );
        }
    }

    #[test]
    fn simple_conversion_of_kakan_is_not_none_response() {
        // 名前から kakan response が返りそうに見えるが、
        // 実際に MjaiAction::None が構築される（見かけ上の成功変換）状態を避ける。
        let action = LegalAction::Kakan {
            tile: tile(124),
            consumed: vec![tile(124), tile(124), tile(124)],
        };
        let converted = legal_action_to_mjai_action(&action, 0, 42);
        assert_eq!(converted, None);
        assert!(!matches!(converted, Some(MjaiAction::None { .. })));
    }

    #[test]
    fn simple_conversion_of_ankan_is_not_none_response() {
        let action = LegalAction::Ankan {
            consumed: vec![tile(72), tile(72), tile(72), tile(72)],
        };
        let converted = legal_action_to_mjai_action(&action, 0, 42);
        assert_eq!(converted, None);
        assert!(!matches!(converted, Some(MjaiAction::None { .. })));
    }

    #[test]
    fn reach_response_excludes_discard_fields() {
        let mjai = legal_action_to_mjai_action(&LegalAction::Reach, 0, 42).unwrap();
        let json = serde_json::to_value(&mjai).unwrap();
        assert_eq!(json["type"], "reach");
        assert_eq!(json["actor"], 0);
        assert_eq!(json["request_id"], 42);
        assert!(json.get("pai").is_none());
        assert!(json.get("tsumogiri").is_none());
    }

    #[test]
    fn reach_serializes_without_discard_information() {
        let mjai = legal_action_to_mjai_action(&LegalAction::Reach, 0, 42).unwrap();
        assert_eq!(
            serde_json::to_string(&mjai).unwrap(),
            r#"{"type":"reach","actor":0,"request_id":42}"#
        );
    }

    #[test]
    fn legal_action_to_mjai_action_sets_actor() {
        assert_eq!(
            legal_action_to_mjai_action(&LegalAction::Dahai { tile: tile(16) }, 3, 1),
            Some(MjaiAction::Dahai {
                actor: 3,
                pai: "5mr".to_string(),
                tsumogiri: None,
                request_id: Some(1),
            })
        );
        assert_eq!(
            legal_action_to_mjai_action(&LegalAction::Reach, 2, 1),
            Some(MjaiAction::Reach {
                actor: 2,
                request_id: Some(1),
            })
        );
        assert_eq!(
            legal_action_to_mjai_action(&LegalAction::Hora, 1, 1),
            Some(MjaiAction::Hora {
                actor: 1,
                target: None,
                pai: None,
                request_id: Some(1),
            })
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
        fn dahai_black_five_maps_to_black_pai_when_both_possible() {
            // bot-core が黒5m(tile 17)を選んだら、possible に "5mr" と "5m" が両方あっても "5m" を送る。
            let context = GameContext::default();
            let chosen = LegalAction::Dahai { tile: tile(17) };
            let possible_actions = vec![possible_dahai("5mr"), possible_dahai("5m")];
            assert_eq!(
                checked_legal_action_to_mjai_action(&chosen, 0, 70, &possible_actions, &context),
                Some(MjaiAction::Dahai {
                    actor: 0,
                    pai: "5m".to_string(),
                    tsumogiri: None,
                    request_id: Some(70),
                })
            );
        }

        #[test]
        fn dahai_red_five_maps_to_red_pai_when_both_possible() {
            // bot-core が赤5m(tile 16)を選んだ場合は "5mr" を送る。黒5へ差し替えない。
            let context = GameContext::default();
            let chosen = LegalAction::Dahai { tile: tile(16) };
            let possible_actions = vec![possible_dahai("5mr"), possible_dahai("5m")];
            assert_eq!(
                checked_legal_action_to_mjai_action(&chosen, 0, 71, &possible_actions, &context),
                Some(MjaiAction::Dahai {
                    actor: 0,
                    pai: "5mr".to_string(),
                    tsumogiri: None,
                    request_id: Some(71),
                })
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

    mod fallback {
        use super::*;

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

        fn possible_kakan() -> MjaiPossibleAction {
            MjaiPossibleAction::Kakan {
                pai: "P".to_string(),
                consumed: vec!["P".to_string(), "P".to_string(), "P".to_string()],
            }
        }

        fn possible_chi() -> MjaiPossibleAction {
            MjaiPossibleAction::Chi {
                pai: "5m".to_string(),
                consumed: vec!["4m".to_string(), "6m".to_string()],
            }
        }

        fn possible_daiminkan() -> MjaiPossibleAction {
            MjaiPossibleAction::Daiminkan {
                pai: "9s".to_string(),
                consumed: vec!["9s".to_string(), "9s".to_string(), "9s".to_string()],
            }
        }

        #[test]
        fn prefers_none_when_present() {
            let context = GameContext::default();
            let possible_actions = vec![
                possible_dahai("1m"),
                possible_pon(),
                MjaiPossibleAction::None,
            ];
            assert_eq!(
                fallback_mjai_action_from_possible_actions(0, 42, &possible_actions, &context),
                Some(MjaiAction::None {
                    request_id: Some(42),
                })
            );
        }

        #[test]
        fn falls_back_to_dahai_when_none_absent() {
            let context = GameContext::default();
            let possible_actions = vec![possible_dahai("1m")];
            assert_eq!(
                fallback_mjai_action_from_possible_actions(0, 43, &possible_actions, &context),
                Some(MjaiAction::Dahai {
                    actor: 0,
                    pai: "1m".to_string(),
                    tsumogiri: None,
                    request_id: Some(43),
                })
            );
        }

        #[test]
        fn dahai_fallback_uses_server_pai() {
            let context = GameContext::default();
            let possible_actions = vec![possible_dahai("5mr")];
            let response =
                fallback_mjai_action_from_possible_actions(2, 44, &possible_actions, &context)
                    .unwrap();
            assert_eq!(
                response,
                MjaiAction::Dahai {
                    actor: 2,
                    pai: "5mr".to_string(),
                    tsumogiri: None,
                    request_id: Some(44),
                }
            );
        }

        #[test]
        fn dahai_fallback_marks_tsumogiri_for_drawn_tile() {
            let context = GameContext::with_drawn_tile(tile(56));
            let possible_actions = vec![possible_dahai("6p")];
            let response =
                fallback_mjai_action_from_possible_actions(1, 45, &possible_actions, &context)
                    .unwrap();
            assert_eq!(
                response,
                MjaiAction::Dahai {
                    actor: 1,
                    pai: "6p".to_string(),
                    tsumogiri: Some(true),
                    request_id: Some(45),
                }
            );
        }

        #[test]
        fn does_not_return_none_when_none_absent() {
            let context = GameContext::default();
            let possible_actions = vec![possible_pon()];
            assert_eq!(
                fallback_mjai_action_from_possible_actions(0, 46, &possible_actions, &context),
                None
            );
        }

        #[test]
        fn empty_possible_actions_do_not_fall_back_to_none() {
            let context = GameContext::default();
            assert_eq!(
                fallback_mjai_action_from_possible_actions(0, 47, &[], &context),
                None
            );
        }

        #[test]
        fn chi_pon_daiminkan_only_has_no_fallback() {
            let context = GameContext::default();
            let possible_actions = vec![possible_chi(), possible_pon(), possible_daiminkan()];
            assert_eq!(
                fallback_mjai_action_from_possible_actions(0, 48, &possible_actions, &context),
                None
            );
        }

        #[test]
        fn builds_ankan_fallback_from_possible_action() {
            let context = GameContext::default();
            let possible_actions = vec![possible_ankan()];
            assert_eq!(
                fallback_mjai_action_from_possible_actions(1, 49, &possible_actions, &context),
                Some(MjaiAction::Ankan {
                    actor: 1,
                    consumed: vec![
                        "1s".to_string(),
                        "1s".to_string(),
                        "1s".to_string(),
                        "1s".to_string(),
                    ],
                    request_id: Some(49),
                })
            );
        }

        #[test]
        fn builds_kakan_fallback_from_possible_action() {
            let context = GameContext::default();
            let possible_actions = vec![possible_kakan()];
            assert_eq!(
                fallback_mjai_action_from_possible_actions(2, 50, &possible_actions, &context),
                Some(MjaiAction::Kakan {
                    actor: 2,
                    pai: "P".to_string(),
                    consumed: vec!["P".to_string(), "P".to_string(), "P".to_string()],
                    request_id: Some(50),
                })
            );
        }

        #[test]
        fn dahai_is_preferred_over_kan() {
            let context = GameContext::default();
            let possible_actions = vec![possible_ankan(), possible_dahai("1m"), possible_kakan()];
            let response =
                fallback_mjai_action_from_possible_actions(0, 51, &possible_actions, &context)
                    .unwrap();
            assert!(matches!(response, MjaiAction::Dahai { .. }));
        }

        #[test]
        fn echoes_request_id() {
            let context = GameContext::default();
            let possible_actions = vec![possible_dahai("1m")];
            for request_id in [0u64, 7, u64::MAX] {
                let response = fallback_mjai_action_from_possible_actions(
                    0,
                    request_id,
                    &possible_actions,
                    &context,
                )
                .unwrap();
                let json = serde_json::to_value(&response).unwrap();
                assert_eq!(json["request_id"], request_id);
            }
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
        let response = legal_action_to_mjai_action(&chosen, 0, request_id).unwrap();
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
        let response = checked_legal_action_to_mjai_action(
            &chosen,
            0,
            request_id,
            &possible_actions,
            &context,
        );
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
        let response = legal_action_to_mjai_action(&chosen, 0, request_id).unwrap();
        assert_eq!(
            serde_json::to_string(&response).unwrap(),
            r#"{"type":"dahai","actor":0,"pai":"5mr","request_id":43}"#
        );
    }
}
