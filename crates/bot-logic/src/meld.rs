use crate::shanten::FixedMeldCount;
use crate::tile::TileId;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MeldKind {
    Chi,
    Pon,
    Daiminkan,
    Ankan,
    Kakan,
}

impl MeldKind {
    pub fn is_open(self) -> bool {
        !matches!(self, Self::Ankan)
    }

    pub fn is_kan(self) -> bool {
        matches!(self, Self::Daiminkan | Self::Ankan | Self::Kakan)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Meld {
    kind: MeldKind,
    tiles: Vec<TileId>,
    called_tile: Option<TileId>,
}

impl Meld {
    pub fn new(kind: MeldKind, tiles: Vec<TileId>, called_tile: Option<TileId>) -> Self {
        Self {
            kind,
            tiles,
            called_tile,
        }
    }

    pub fn kind(&self) -> MeldKind {
        self.kind
    }

    pub fn tiles(&self) -> &[TileId] {
        &self.tiles
    }

    pub fn called_tile(&self) -> Option<TileId> {
        self.called_tile
    }

    pub fn is_open(&self) -> bool {
        self.kind.is_open()
    }
}

pub fn fixed_meld_count(melds: &[Meld]) -> Option<FixedMeldCount> {
    u8::try_from(melds.len()).ok().and_then(FixedMeldCount::new)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tile(value: u8) -> TileId {
        TileId::new(value).unwrap()
    }

    fn pon() -> Meld {
        Meld::new(
            MeldKind::Pon,
            vec![tile(108), tile(109), tile(110)],
            Some(tile(108)),
        )
    }

    #[test]
    fn open_melds_are_open() {
        for kind in [
            MeldKind::Chi,
            MeldKind::Pon,
            MeldKind::Daiminkan,
            MeldKind::Kakan,
        ] {
            assert!(kind.is_open(), "kind: {kind:?}");
        }
    }

    #[test]
    fn ankan_is_not_open() {
        assert!(!MeldKind::Ankan.is_open());
    }

    #[test]
    fn kans_are_kans() {
        for kind in [MeldKind::Daiminkan, MeldKind::Ankan, MeldKind::Kakan] {
            assert!(kind.is_kan(), "kind: {kind:?}");
        }
        assert!(!MeldKind::Chi.is_kan());
        assert!(!MeldKind::Pon.is_kan());
    }

    #[test]
    fn meld_holds_kind_tiles_and_called_tile() {
        let meld = pon();
        assert_eq!(meld.kind(), MeldKind::Pon);
        assert_eq!(meld.tiles(), [tile(108), tile(109), tile(110)]);
        assert_eq!(meld.called_tile(), Some(tile(108)));
        assert!(meld.is_open());
    }

    #[test]
    fn ankan_meld_has_no_called_tile_and_is_not_open() {
        let meld = Meld::new(
            MeldKind::Ankan,
            vec![tile(108), tile(109), tile(110), tile(111)],
            None,
        );
        assert_eq!(meld.called_tile(), None);
        assert!(!meld.is_open());
    }

    #[test]
    fn fixed_meld_count_counts_every_kind_as_one_meld() {
        for count in 0..=4usize {
            let melds: Vec<Meld> = (0..count).map(|_| pon()).collect();
            assert_eq!(
                fixed_meld_count(&melds).map(FixedMeldCount::get),
                Some(count as u8),
                "count: {count}"
            );
        }
    }

    #[test]
    fn fixed_meld_count_rejects_more_than_four_melds() {
        let melds: Vec<Meld> = (0..5).map(|_| pon()).collect();
        assert_eq!(fixed_meld_count(&melds), None);
    }

    #[test]
    fn fixed_meld_count_of_empty_melds_is_none_variant() {
        assert_eq!(fixed_meld_count(&[]), Some(FixedMeldCount::NONE));
    }
}
