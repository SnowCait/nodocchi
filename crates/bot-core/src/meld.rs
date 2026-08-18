pub use bot_logic::meld::{Meld, MeldKind, fixed_meld_count};

#[cfg(test)]
mod tests {
    use super::*;
    use bot_logic::{FixedMeldCount, TileId};

    fn tile(value: u8) -> TileId {
        TileId::new(value).unwrap()
    }

    #[test]
    fn re_exported_meld_is_the_bot_logic_type() {
        let meld: bot_logic::Meld = Meld::new(
            MeldKind::Pon,
            vec![tile(108), tile(109), tile(110)],
            Some(tile(108)),
        );

        assert_eq!(meld.kind(), bot_logic::MeldKind::Pon);
        assert!(meld.is_open());
    }

    #[test]
    fn re_exported_fixed_meld_count_counts_melds() {
        let melds = vec![Meld::new(
            MeldKind::Ankan,
            vec![tile(108), tile(109), tile(110), tile(111)],
            None,
        )];

        assert_eq!(fixed_meld_count(&melds), FixedMeldCount::new(1));
        assert!(!melds[0].is_open());
    }
}
