use bot_logic::TileId;

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

#[cfg(test)]
mod tests {
    use super::*;

    fn tile(value: u8) -> TileId {
        TileId::new(value).unwrap()
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
}
