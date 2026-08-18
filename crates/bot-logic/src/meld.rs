use crate::shanten::FixedMeldCount;
use crate::tile::{TileId, TileType};

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

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum MeldShape {
    Sequence { start: TileType },
    Triplet { tile: TileType },
    Kan { tile: TileType },
}

impl MeldShape {
    pub fn is_sequence(self) -> bool {
        matches!(self, Self::Sequence { .. })
    }

    pub fn is_triplet_like(self) -> bool {
        matches!(self, Self::Triplet { .. } | Self::Kan { .. })
    }

    pub fn is_kan(self) -> bool {
        matches!(self, Self::Kan { .. })
    }

    pub fn sequence_start(self) -> Option<TileType> {
        match self {
            Self::Sequence { start } => Some(start),
            Self::Triplet { .. } | Self::Kan { .. } => None,
        }
    }

    pub fn triplet_tile_type(self) -> Option<TileType> {
        match self {
            Self::Triplet { tile } | Self::Kan { tile } => Some(tile),
            Self::Sequence { .. } => None,
        }
    }

    pub fn tile_types(self) -> Option<[TileType; 3]> {
        match self {
            Self::Sequence { start } => start.sequence(),
            Self::Triplet { tile } | Self::Kan { tile } => Some([tile; 3]),
        }
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

    pub fn shape(&self) -> Option<MeldShape> {
        match self.kind {
            MeldKind::Chi => sequence_shape(&self.tiles),
            MeldKind::Pon => {
                triplet_tile_type(&self.tiles, 3).map(|tile| MeldShape::Triplet { tile })
            }
            MeldKind::Daiminkan | MeldKind::Ankan | MeldKind::Kakan => {
                triplet_tile_type(&self.tiles, 4).map(|tile| MeldShape::Kan { tile })
            }
        }
    }
}

fn sequence_shape(tiles: &[TileId]) -> Option<MeldShape> {
    let [first, second, third] = tiles else {
        return None;
    };
    let mut tile_types = [first.tile_type(), second.tile_type(), third.tile_type()];
    tile_types.sort_unstable();
    let start = tile_types[0];
    (start.sequence()? == tile_types).then_some(MeldShape::Sequence { start })
}

fn triplet_tile_type(tiles: &[TileId], expected_len: usize) -> Option<TileType> {
    if tiles.len() != expected_len {
        return None;
    }
    let (first, rest) = tiles.split_first()?;
    let tile = first.tile_type();
    rest.iter()
        .all(|other| other.tile_type() == tile)
        .then_some(tile)
}

pub fn is_menzen(melds: &[Meld]) -> bool {
    !melds.iter().any(Meld::is_open)
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

    fn tile_type(s: &str) -> TileType {
        TileType::from_mjai_type_str(s).unwrap()
    }

    struct TileIdSource {
        used: [u8; TileType::COUNT],
    }

    impl TileIdSource {
        fn new() -> Self {
            Self {
                used: [0; TileType::COUNT],
            }
        }

        fn meld(&mut self, kind: MeldKind, strings: &[&str]) -> Meld {
            let tiles: Vec<TileId> = strings.iter().map(|s| self.tile(s)).collect();
            let called_tile = kind.is_open().then(|| tiles[0]);
            Meld::new(kind, tiles, called_tile)
        }

        fn tile(&mut self, s: &str) -> TileId {
            let tile_type = tile_type(s);
            let copy = &mut self.used[tile_type.index()];
            let id = TileId::new(tile_type.raw() * 4 + *copy).unwrap();
            *copy += 1;
            id
        }
    }

    #[test]
    fn chi_shape_is_a_sequence() {
        let mut source = TileIdSource::new();
        let meld = source.meld(MeldKind::Chi, &["3p", "1p", "2p"]);

        assert_eq!(
            meld.shape(),
            Some(MeldShape::Sequence {
                start: tile_type("1p")
            })
        );
        assert!(meld.shape().unwrap().is_sequence());
        assert_eq!(
            meld.shape().unwrap().sequence_start(),
            Some(tile_type("1p"))
        );
        assert_eq!(meld.shape().unwrap().triplet_tile_type(), None);
        assert_eq!(
            meld.shape().unwrap().tile_types(),
            Some([tile_type("1p"), tile_type("2p"), tile_type("3p")])
        );
    }

    #[test]
    fn pon_shape_is_a_triplet() {
        let mut source = TileIdSource::new();
        let meld = source.meld(MeldKind::Pon, &["E", "E", "E"]);

        assert_eq!(
            meld.shape(),
            Some(MeldShape::Triplet {
                tile: tile_type("E")
            })
        );
        let shape = meld.shape().unwrap();
        assert!(shape.is_triplet_like());
        assert!(!shape.is_kan());
        assert_eq!(shape.triplet_tile_type(), Some(tile_type("E")));
        assert_eq!(shape.sequence_start(), None);
        assert_eq!(shape.tile_types(), Some([tile_type("E"); 3]));
    }

    #[test]
    fn kan_shapes_are_triplet_like_kans() {
        for kind in [MeldKind::Daiminkan, MeldKind::Ankan, MeldKind::Kakan] {
            let mut source = TileIdSource::new();
            let meld = source.meld(kind, &["5s", "5s", "5s", "5s"]);
            let shape = meld.shape().expect("kind: {kind:?}");

            assert_eq!(
                shape,
                MeldShape::Kan {
                    tile: tile_type("5s")
                },
                "kind: {kind:?}"
            );
            assert!(shape.is_kan(), "kind: {kind:?}");
            assert!(shape.is_triplet_like(), "kind: {kind:?}");
            assert!(!shape.is_sequence(), "kind: {kind:?}");
            assert_eq!(shape.triplet_tile_type(), Some(tile_type("5s")));
        }
    }

    #[test]
    fn malformed_melds_have_no_shape() {
        let mut source = TileIdSource::new();

        assert_eq!(
            source.meld(MeldKind::Chi, &["1m", "1m", "1m"]).shape(),
            None
        );
        assert_eq!(
            source.meld(MeldKind::Chi, &["1m", "2m", "4m"]).shape(),
            None
        );
        assert_eq!(
            source.meld(MeldKind::Chi, &["9m", "1p", "2p"]).shape(),
            None
        );
        assert_eq!(source.meld(MeldKind::Chi, &["E", "S", "W"]).shape(), None);
        assert_eq!(source.meld(MeldKind::Chi, &["1s", "2s"]).shape(), None);
        assert_eq!(
            source.meld(MeldKind::Pon, &["1m", "2m", "3m"]).shape(),
            None
        );
        assert_eq!(source.meld(MeldKind::Pon, &["9p", "9p"]).shape(), None);
        assert_eq!(
            source
                .meld(MeldKind::Ankan, &["1s", "1s", "1s", "2s"])
                .shape(),
            None
        );
        assert_eq!(
            source
                .meld(MeldKind::Daiminkan, &["3s", "3s", "3s"])
                .shape(),
            None
        );
    }

    #[test]
    fn menzen_survives_ankan_only() {
        let mut source = TileIdSource::new();
        let ankan = source.meld(MeldKind::Ankan, &["1m", "1m", "1m", "1m"]);

        assert!(is_menzen(&[]));
        assert!(is_menzen(std::slice::from_ref(&ankan)));
    }

    #[test]
    fn menzen_is_broken_by_every_open_meld() {
        for (kind, tiles) in [
            (MeldKind::Chi, vec!["1p", "2p", "3p"]),
            (MeldKind::Pon, vec!["E", "E", "E"]),
            (MeldKind::Daiminkan, vec!["5s", "5s", "5s", "5s"]),
            (MeldKind::Kakan, vec!["9m", "9m", "9m", "9m"]),
        ] {
            let mut source = TileIdSource::new();
            let melds = vec![
                source.meld(MeldKind::Ankan, &["2m", "2m", "2m", "2m"]),
                source.meld(kind, &tiles),
            ];

            assert!(!is_menzen(&melds), "kind: {kind:?}");
        }
    }
}
