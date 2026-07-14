use bot_logic::{TileId, TileType};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LegalAction {
    Dahai { tile: TileId },
    Chi { tile: TileId, consumed: Vec<TileId> },
    Pon { tile: TileId, consumed: Vec<TileId> },
    Daiminkan { tile: TileId, consumed: Vec<TileId> },
    Ankan { consumed: Vec<TileId> },
    Kakan { tile: TileId, consumed: Vec<TileId> },
    Reach,
    Hora,
    Ryukyoku,
    None,
}

// 指定牌種の合法 Dahai から、実際に切る物理牌を選ぶ共通方針。黒牌を赤牌より優先する。
//
// - 指定牌種の黒牌 Dahai が1件以上ある: 最初の黒牌 Dahai を返す
// - 黒牌がなく赤牌 Dahai がある: 最初の赤牌 Dahai を返す
// - 指定牌種の合法 Dahai がない: None
//
// 牌種の選択自体は呼び出し側で先に決める。ここでは同じ牌種の物理牌だけを黒牌へ正規化する。
// 赤5以外の牌では合法 action の元順序どおり最初の Dahai を返す。
pub(crate) fn preferred_dahai_action_for_type(
    legal_actions: &[LegalAction],
    tile_type: TileType,
) -> Option<&LegalAction> {
    let mut red_fallback = None;
    for action in legal_actions {
        let LegalAction::Dahai { tile } = action else {
            continue;
        };
        if tile.tile_type() != tile_type {
            continue;
        }
        if tile.is_red() {
            red_fallback.get_or_insert(action);
        } else {
            return Some(action);
        }
    }
    red_fallback
}

// すでに牌種を決めた Dahai について、同じ牌種の合法 Dahai から黒牌を優先し直す。
//
// `chosen` と同じ牌種の合法 Dahai に黒牌があればそれを返し、なければ `chosen`(赤牌)を維持する。
// Dahai 以外の action や、合法 Dahai が見つからない場合は `chosen` をそのまま返す。
// 牌種は変えないため、他牌種との選択順には影響しない。
pub(crate) fn prefer_black_five_for_action<'a>(
    legal_actions: &'a [LegalAction],
    chosen: &'a LegalAction,
) -> &'a LegalAction {
    let LegalAction::Dahai { tile } = chosen else {
        return chosen;
    };
    preferred_dahai_action_for_type(legal_actions, tile.tile_type()).unwrap_or(chosen)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tile(value: u8) -> TileId {
        TileId::new(value).unwrap()
    }

    fn dahai(value: u8) -> LegalAction {
        LegalAction::Dahai { tile: tile(value) }
    }

    fn tile_type_of(value: u8) -> TileType {
        tile(value).tile_type()
    }

    #[test]
    fn constructs_meld_and_kan_variants() {
        assert_eq!(
            LegalAction::Chi {
                tile: tile(17),
                consumed: vec![tile(12), tile(20)],
            },
            LegalAction::Chi {
                tile: tile(17),
                consumed: vec![tile(12), tile(20)],
            }
        );
        assert_eq!(
            LegalAction::Pon {
                tile: tile(108),
                consumed: vec![tile(109), tile(110)],
            },
            LegalAction::Pon {
                tile: tile(108),
                consumed: vec![tile(109), tile(110)],
            }
        );
        assert_eq!(
            LegalAction::Daiminkan {
                tile: tile(104),
                consumed: vec![tile(105), tile(106), tile(107)],
            },
            LegalAction::Daiminkan {
                tile: tile(104),
                consumed: vec![tile(105), tile(106), tile(107)],
            }
        );
        assert_eq!(
            LegalAction::Ankan {
                consumed: vec![tile(72), tile(73), tile(74), tile(75)],
            },
            LegalAction::Ankan {
                consumed: vec![tile(72), tile(73), tile(74), tile(75)],
            }
        );
        assert_eq!(
            LegalAction::Kakan {
                tile: tile(124),
                consumed: vec![tile(125), tile(126), tile(127)],
            },
            LegalAction::Kakan {
                tile: tile(124),
                consumed: vec![tile(125), tile(126), tile(127)],
            }
        );
    }

    // 赤5 (16=5m, 52=5p, 88=5s) と黒5 (17=5m, 53=5p, 89=5s) の各色で黒優先を確認する。
    #[test]
    fn preferred_dahai_action_prefers_black_five_over_red() {
        for (red, black) in [(16, 17), (52, 53), (88, 89)] {
            let tile_type = tile_type_of(red);
            // [赤5, 黒5] → 黒5
            let actions = vec![dahai(red), dahai(black)];
            assert_eq!(
                preferred_dahai_action_for_type(&actions, tile_type),
                Some(&dahai(black))
            );
            // [黒5, 赤5] → 黒5
            let actions = vec![dahai(black), dahai(red)];
            assert_eq!(
                preferred_dahai_action_for_type(&actions, tile_type),
                Some(&dahai(black))
            );
            // [赤5のみ] → 赤5
            let actions = vec![dahai(red)];
            assert_eq!(
                preferred_dahai_action_for_type(&actions, tile_type),
                Some(&dahai(red))
            );
            // [黒5のみ] → 黒5
            let actions = vec![dahai(black)];
            assert_eq!(
                preferred_dahai_action_for_type(&actions, tile_type),
                Some(&dahai(black))
            );
        }
    }

    #[test]
    fn preferred_dahai_action_none_for_absent_type() {
        let actions = vec![dahai(0), dahai(4)];
        assert_eq!(
            preferred_dahai_action_for_type(&actions, tile_type_of(40)),
            None
        );
    }

    #[test]
    fn preferred_dahai_action_keeps_first_for_non_red_type() {
        // 赤5以外の牌では、最初の合法物理牌を維持する。1m(0) と 1m(1) の両方が合法なら先頭。
        let actions = vec![dahai(0), dahai(1)];
        assert_eq!(
            preferred_dahai_action_for_type(&actions, tile_type_of(0)),
            Some(&dahai(0))
        );
    }

    #[test]
    fn preferred_dahai_action_ignores_non_dahai() {
        let actions = vec![
            LegalAction::Reach,
            LegalAction::Pon {
                tile: tile(16),
                consumed: vec![tile(17), tile(18)],
            },
            dahai(16),
            dahai(17),
        ];
        assert_eq!(
            preferred_dahai_action_for_type(&actions, tile_type_of(16)),
            Some(&dahai(17))
        );
    }

    #[test]
    fn prefer_black_five_for_action_normalizes_within_same_type() {
        // 選択済み action が赤5でも、同牌種に黒5があれば黒5へ正規化する。
        let actions = vec![dahai(16), dahai(17)];
        assert_eq!(
            prefer_black_five_for_action(&actions, &actions[0]),
            &dahai(17)
        );
    }

    #[test]
    fn prefer_black_five_for_action_keeps_red_when_no_black() {
        let actions = vec![dahai(16)];
        assert_eq!(
            prefer_black_five_for_action(&actions, &actions[0]),
            &dahai(16)
        );
    }

    #[test]
    fn prefer_black_five_for_action_does_not_change_tile_type() {
        // 黒5優先で他牌種(1p)へ切り替えない。選択済み 1p はそのまま。
        let actions = vec![dahai(16), dahai(36), dahai(17)];
        let chosen = dahai(36);
        assert_eq!(prefer_black_five_for_action(&actions, &chosen), &dahai(36));
    }

    #[test]
    fn prefer_black_five_for_action_passes_through_non_dahai() {
        let actions = vec![LegalAction::Reach];
        assert_eq!(
            prefer_black_five_for_action(&actions, &LegalAction::Reach),
            &LegalAction::Reach
        );
    }
}
