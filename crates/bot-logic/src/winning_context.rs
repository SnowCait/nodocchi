use crate::tile::TileType;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum WinMethod {
    Ron,
    Tsumo,
}

impl WinMethod {
    pub fn is_ron(self) -> bool {
        matches!(self, Self::Ron)
    }

    pub fn is_tsumo(self) -> bool {
        matches!(self, Self::Tsumo)
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub enum RiichiStatus {
    #[default]
    Unknown,
    NotDeclared,
    Riichi,
    DoubleRiichi,
}

impl RiichiStatus {
    pub fn is_declared(self) -> Option<bool> {
        match self {
            Self::Unknown => None,
            Self::NotDeclared => Some(false),
            Self::Riichi | Self::DoubleRiichi => Some(true),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WinningContext {
    win_method: WinMethod,
    round_wind: Option<TileType>,
    seat_wind: Option<TileType>,
    riichi: RiichiStatus,
    ippatsu: Option<bool>,
    rinshan: Option<bool>,
    chankan: Option<bool>,
    remaining_live_tiles: Option<u32>,
}

impl WinningContext {
    pub fn new(win_method: WinMethod) -> Self {
        Self {
            win_method,
            round_wind: None,
            seat_wind: None,
            riichi: RiichiStatus::Unknown,
            ippatsu: None,
            rinshan: None,
            chankan: None,
            remaining_live_tiles: None,
        }
    }

    pub fn with_round_wind(mut self, round_wind: Option<TileType>) -> Self {
        self.round_wind = round_wind;
        self
    }

    pub fn with_seat_wind(mut self, seat_wind: Option<TileType>) -> Self {
        self.seat_wind = seat_wind;
        self
    }

    pub fn with_riichi(mut self, riichi: RiichiStatus) -> Self {
        self.riichi = riichi;
        self
    }

    pub fn with_ippatsu(mut self, ippatsu: Option<bool>) -> Self {
        self.ippatsu = ippatsu;
        self
    }

    pub fn with_rinshan(mut self, rinshan: Option<bool>) -> Self {
        self.rinshan = rinshan;
        self
    }

    pub fn with_chankan(mut self, chankan: Option<bool>) -> Self {
        self.chankan = chankan;
        self
    }

    pub fn with_remaining_live_tiles(mut self, remaining_live_tiles: Option<u32>) -> Self {
        self.remaining_live_tiles = remaining_live_tiles;
        self
    }

    pub fn win_method(self) -> WinMethod {
        self.win_method
    }

    pub fn round_wind(self) -> Option<TileType> {
        self.round_wind
    }

    pub fn seat_wind(self) -> Option<TileType> {
        self.seat_wind
    }

    pub fn riichi(self) -> RiichiStatus {
        self.riichi
    }

    pub fn ippatsu(self) -> Option<bool> {
        self.ippatsu
    }

    pub fn rinshan(self) -> Option<bool> {
        self.rinshan
    }

    pub fn chankan(self) -> Option<bool> {
        self.chankan
    }

    pub fn remaining_live_tiles(self) -> Option<u32> {
        self.remaining_live_tiles
    }

    pub fn is_last_live_tile(self) -> bool {
        self.remaining_live_tiles == Some(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tile_type(s: &str) -> TileType {
        TileType::from_mjai_type_str(s).unwrap()
    }

    #[test]
    fn new_context_knows_only_the_win_method() {
        let context = WinningContext::new(WinMethod::Tsumo);

        assert_eq!(context.win_method(), WinMethod::Tsumo);
        assert!(context.win_method().is_tsumo());
        assert!(!context.win_method().is_ron());
        assert_eq!(context.round_wind(), None);
        assert_eq!(context.seat_wind(), None);
        assert_eq!(context.riichi(), RiichiStatus::Unknown);
        assert_eq!(context.ippatsu(), None);
        assert_eq!(context.rinshan(), None);
        assert_eq!(context.chankan(), None);
        assert_eq!(context.remaining_live_tiles(), None);
        assert!(!context.is_last_live_tile());
    }

    #[test]
    fn builders_keep_every_given_fact() {
        let context = WinningContext::new(WinMethod::Ron)
            .with_round_wind(Some(tile_type("E")))
            .with_seat_wind(Some(tile_type("S")))
            .with_riichi(RiichiStatus::DoubleRiichi)
            .with_ippatsu(Some(true))
            .with_rinshan(Some(false))
            .with_chankan(Some(true))
            .with_remaining_live_tiles(Some(7));

        assert_eq!(context.win_method(), WinMethod::Ron);
        assert_eq!(context.round_wind(), Some(tile_type("E")));
        assert_eq!(context.seat_wind(), Some(tile_type("S")));
        assert_eq!(context.riichi(), RiichiStatus::DoubleRiichi);
        assert_eq!(context.ippatsu(), Some(true));
        assert_eq!(context.rinshan(), Some(false));
        assert_eq!(context.chankan(), Some(true));
        assert_eq!(context.remaining_live_tiles(), Some(7));
    }

    #[test]
    fn riichi_status_keeps_tri_state_semantics() {
        assert_eq!(RiichiStatus::Unknown.is_declared(), None);
        assert_eq!(RiichiStatus::NotDeclared.is_declared(), Some(false));
        assert_eq!(RiichiStatus::Riichi.is_declared(), Some(true));
        assert_eq!(RiichiStatus::DoubleRiichi.is_declared(), Some(true));
    }

    #[test]
    fn riichi_status_defaults_to_unknown() {
        assert_eq!(RiichiStatus::default(), RiichiStatus::Unknown);
    }

    #[test]
    fn last_live_tile_distinguishes_unknown_from_zero() {
        let context = WinningContext::new(WinMethod::Tsumo);

        assert!(!context.with_remaining_live_tiles(None).is_last_live_tile());
        assert!(
            !context
                .with_remaining_live_tiles(Some(1))
                .is_last_live_tile()
        );
        assert!(
            context
                .with_remaining_live_tiles(Some(0))
                .is_last_live_tile()
        );
    }
}
