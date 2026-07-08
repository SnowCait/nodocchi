use thiserror::Error;

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum TileParseError {
    #[error("invalid mjai tile string: {0:?}")]
    InvalidMjaiString(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Suit {
    Man,
    Pin,
    Sou,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct TileType(u8);

impl TileType {
    pub const COUNT: usize = 34;

    pub fn new(value: u8) -> Option<Self> {
        (value < 34).then_some(Self(value))
    }

    pub fn all() -> impl Iterator<Item = Self> {
        (0..34).map(Self)
    }

    pub fn raw(self) -> u8 {
        self.0
    }

    pub fn index(self) -> usize {
        self.0 as usize
    }

    pub fn is_man(self) -> bool {
        self.0 <= 8
    }

    pub fn is_pin(self) -> bool {
        (9..=17).contains(&self.0)
    }

    pub fn is_sou(self) -> bool {
        (18..=26).contains(&self.0)
    }

    pub fn is_honor(self) -> bool {
        self.0 >= 27
    }

    pub fn is_terminal(self) -> bool {
        matches!(self.number(), Some(1) | Some(9))
    }

    pub fn is_yaochu(self) -> bool {
        self.is_honor() || self.is_terminal()
    }

    pub fn number(self) -> Option<u8> {
        if self.is_honor() {
            None
        } else {
            Some(self.0 % 9 + 1)
        }
    }

    pub fn suit(self) -> Option<Suit> {
        match self.0 {
            0..=8 => Some(Suit::Man),
            9..=17 => Some(Suit::Pin),
            18..=26 => Some(Suit::Sou),
            _ => None,
        }
    }

    pub fn is_suited(self) -> bool {
        !self.is_honor()
    }

    pub fn next_in_suit(self) -> Option<TileType> {
        match self.number()? {
            1..=8 => Some(Self(self.0 + 1)),
            _ => None,
        }
    }

    pub fn second_next_in_suit(self) -> Option<TileType> {
        self.next_in_suit()?.next_in_suit()
    }

    pub fn can_start_sequence(self) -> bool {
        matches!(self.number(), Some(1..=7))
    }

    pub fn sequence(self) -> Option<[TileType; 3]> {
        Some([self, self.next_in_suit()?, self.second_next_in_suit()?])
    }

    pub fn to_mjai_string(self) -> String {
        match self.0 {
            27 => "E".to_string(),
            28 => "S".to_string(),
            29 => "W".to_string(),
            30 => "N".to_string(),
            31 => "P".to_string(),
            32 => "F".to_string(),
            33 => "C".to_string(),
            value => {
                let number = value % 9 + 1;
                let suit = match value / 9 {
                    0 => 'm',
                    1 => 'p',
                    _ => 's',
                };
                format!("{number}{suit}")
            }
        }
    }

    pub fn from_mjai_type_str(s: &str) -> Result<Self, TileParseError> {
        let honor = match s {
            "E" => Some(27),
            "S" => Some(28),
            "W" => Some(29),
            "N" => Some(30),
            "P" => Some(31),
            "F" => Some(32),
            "C" => Some(33),
            _ => None,
        };
        if let Some(value) = honor {
            return Ok(Self(value));
        }
        let (digit, suit) = match s.as_bytes() {
            [digit @ b'1'..=b'9', suit] => (*digit, *suit),
            [digit @ b'5', suit, b'r'] => (*digit, *suit),
            _ => return Err(TileParseError::InvalidMjaiString(s.to_string())),
        };
        let base = match suit {
            b'm' => 0,
            b'p' => 9,
            b's' => 18,
            _ => return Err(TileParseError::InvalidMjaiString(s.to_string())),
        };
        Ok(Self(base + (digit - b'1')))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct TileId(u8);

impl TileId {
    pub const COUNT: usize = 136;

    pub fn new(value: u8) -> Option<Self> {
        (value < 136).then_some(Self(value))
    }

    pub fn raw(self) -> u8 {
        self.0
    }

    pub fn index(self) -> usize {
        self.0 as usize
    }

    pub fn tile_type(self) -> TileType {
        TileType(self.0 / 4)
    }

    pub fn copy_index(self) -> u8 {
        self.0 % 4
    }

    pub fn is_red(self) -> bool {
        matches!(self.0, 16 | 52 | 88)
    }

    pub fn to_mjai_string(self) -> String {
        let mut s = self.tile_type().to_mjai_string();
        if self.is_red() {
            s.push('r');
        }
        s
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum VisibleTile {
    Known(TileId),
    Unknown,
}

pub fn next_dora(tile: TileType) -> TileType {
    match tile.0 {
        value @ 0..=26 => {
            let base = value / 9 * 9;
            TileType(base + (value - base + 1) % 9)
        }
        value @ 27..=30 => TileType(27 + (value - 27 + 1) % 4),
        value => TileType(31 + (value - 31 + 1) % 3),
    }
}

pub fn count_dora(tile: TileId, dora_indicators: &[TileId]) -> u8 {
    let tile_type = tile.tile_type();
    let indicated = dora_indicators
        .iter()
        .filter(|indicator| next_dora(indicator.tile_type()) == tile_type)
        .count() as u8;
    indicated + u8::from(tile.is_red())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tt(value: u8) -> TileType {
        TileType::new(value).unwrap()
    }

    fn id(value: u8) -> TileId {
        TileId::new(value).unwrap()
    }

    #[test]
    fn tile_type_new_accepts_valid_range() {
        assert!(TileType::new(0).is_some());
        assert!(TileType::new(33).is_some());
    }

    #[test]
    fn tile_type_new_rejects_out_of_range() {
        assert!(TileType::new(34).is_none());
    }

    #[test]
    fn tile_id_new_accepts_valid_range() {
        assert!(TileId::new(0).is_some());
        assert!(TileId::new(135).is_some());
    }

    #[test]
    fn tile_id_new_rejects_out_of_range() {
        assert!(TileId::new(136).is_none());
    }

    #[test]
    fn tile_id_maps_to_tile_type() {
        assert_eq!(id(0).tile_type(), tt(0));
        assert_eq!(id(16).tile_type(), tt(4));
        assert_eq!(id(52).tile_type(), tt(13));
        assert_eq!(id(88).tile_type(), tt(22));
        assert_eq!(id(135).tile_type(), tt(33));
    }

    #[test]
    fn red_five_ids() {
        assert!(id(16).is_red());
        assert!(id(52).is_red());
        assert!(id(88).is_red());
        assert!(!id(17).is_red());
    }

    #[test]
    fn copy_index_and_raw() {
        assert_eq!(id(16).copy_index(), 0);
        assert_eq!(id(19).copy_index(), 3);
        assert_eq!(id(135).raw(), 135);
        assert_eq!(id(7).index(), 7);
        assert_eq!(tt(33).raw(), 33);
        assert_eq!(tt(5).index(), 5);
    }

    #[test]
    fn tile_type_predicates() {
        assert!(tt(0).is_man());
        assert!(!tt(9).is_man());
        assert!(tt(9).is_pin());
        assert!(tt(18).is_sou());
        assert!(tt(27).is_honor());
        assert!(!tt(26).is_honor());
        assert!(tt(0).is_terminal());
        assert!(tt(8).is_terminal());
        assert!(!tt(4).is_terminal());
        assert!(!tt(27).is_terminal());
        assert!(tt(0).is_yaochu());
        assert!(tt(27).is_yaochu());
        assert!(!tt(4).is_yaochu());
    }

    #[test]
    fn number_and_suit() {
        assert_eq!(tt(0).number(), Some(1));
        assert_eq!(tt(26).number(), Some(9));
        assert_eq!(tt(27).number(), None);
        assert_eq!(tt(0).suit(), Some(Suit::Man));
        assert_eq!(tt(9).suit(), Some(Suit::Pin));
        assert_eq!(tt(18).suit(), Some(Suit::Sou));
        assert_eq!(tt(33).suit(), None);
    }

    #[test]
    fn is_suited_for_numbers_and_honors() {
        assert!(tt(0).is_suited());
        assert!(tt(8).is_suited());
        assert!(tt(9).is_suited());
        assert!(tt(17).is_suited());
        assert!(tt(18).is_suited());
        assert!(tt(26).is_suited());
        assert!(!tt(27).is_suited());
        assert!(!tt(33).is_suited());
    }

    #[test]
    fn next_in_suit_stays_within_suit() {
        assert_eq!(tt(0).next_in_suit(), Some(tt(1)));
        assert_eq!(tt(7).next_in_suit(), Some(tt(8)));
        assert_eq!(tt(8).next_in_suit(), None);
        assert_eq!(tt(9).next_in_suit(), Some(tt(10)));
        assert_eq!(tt(16).next_in_suit(), Some(tt(17)));
        assert_eq!(tt(17).next_in_suit(), None);
        assert_eq!(tt(18).next_in_suit(), Some(tt(19)));
        assert_eq!(tt(25).next_in_suit(), Some(tt(26)));
        assert_eq!(tt(26).next_in_suit(), None);
        assert_eq!(tt(27).next_in_suit(), None);
    }

    #[test]
    fn second_next_in_suit_stays_within_suit() {
        assert_eq!(tt(0).second_next_in_suit(), Some(tt(2)));
        assert_eq!(tt(6).second_next_in_suit(), Some(tt(8)));
        assert_eq!(tt(7).second_next_in_suit(), None);
        assert_eq!(tt(8).second_next_in_suit(), None);
        assert_eq!(tt(9).second_next_in_suit(), Some(tt(11)));
        assert_eq!(tt(15).second_next_in_suit(), Some(tt(17)));
        assert_eq!(tt(16).second_next_in_suit(), None);
        assert_eq!(tt(17).second_next_in_suit(), None);
        assert_eq!(tt(18).second_next_in_suit(), Some(tt(20)));
        assert_eq!(tt(24).second_next_in_suit(), Some(tt(26)));
        assert_eq!(tt(25).second_next_in_suit(), None);
        assert_eq!(tt(26).second_next_in_suit(), None);
        assert_eq!(tt(27).second_next_in_suit(), None);
    }

    #[test]
    fn can_start_sequence_for_numbers_and_honors() {
        assert!(tt(0).can_start_sequence());
        assert!(tt(6).can_start_sequence());
        assert!(!tt(7).can_start_sequence());
        assert!(!tt(8).can_start_sequence());
        assert!(tt(9).can_start_sequence());
        assert!(tt(15).can_start_sequence());
        assert!(!tt(16).can_start_sequence());
        assert!(!tt(17).can_start_sequence());
        assert!(tt(18).can_start_sequence());
        assert!(tt(24).can_start_sequence());
        assert!(!tt(25).can_start_sequence());
        assert!(!tt(26).can_start_sequence());
        assert!(!tt(27).can_start_sequence());
    }

    #[test]
    fn sequence_from_start_tile() {
        assert_eq!(tt(0).sequence(), Some([tt(0), tt(1), tt(2)]));
        assert_eq!(tt(6).sequence(), Some([tt(6), tt(7), tt(8)]));
        assert_eq!(tt(7).sequence(), None);
        assert_eq!(tt(8).sequence(), None);
        assert_eq!(tt(9).sequence(), Some([tt(9), tt(10), tt(11)]));
        assert_eq!(tt(15).sequence(), Some([tt(15), tt(16), tt(17)]));
        assert_eq!(tt(16).sequence(), None);
        assert_eq!(tt(17).sequence(), None);
        assert_eq!(tt(18).sequence(), Some([tt(18), tt(19), tt(20)]));
        assert_eq!(tt(24).sequence(), Some([tt(24), tt(25), tt(26)]));
        assert_eq!(tt(25).sequence(), None);
        assert_eq!(tt(26).sequence(), None);
        assert_eq!(tt(27).sequence(), None);
    }

    #[test]
    fn next_dora_wraps_numbers() {
        assert_eq!(next_dora(tt(8)), tt(0));
        assert_eq!(next_dora(tt(17)), tt(9));
        assert_eq!(next_dora(tt(26)), tt(18));
        assert_eq!(next_dora(tt(0)), tt(1));
    }

    #[test]
    fn next_dora_wraps_honors() {
        assert_eq!(next_dora(tt(27)), tt(28));
        assert_eq!(next_dora(tt(30)), tt(27));
        assert_eq!(next_dora(tt(31)), tt(32));
        assert_eq!(next_dora(tt(33)), tt(31));
    }

    #[test]
    fn from_mjai_type_str_parses_numbers() {
        assert_eq!(TileType::from_mjai_type_str("1m").unwrap(), tt(0));
        assert_eq!(TileType::from_mjai_type_str("9m").unwrap(), tt(8));
        assert_eq!(TileType::from_mjai_type_str("1p").unwrap(), tt(9));
        assert_eq!(TileType::from_mjai_type_str("9p").unwrap(), tt(17));
        assert_eq!(TileType::from_mjai_type_str("1s").unwrap(), tt(18));
        assert_eq!(TileType::from_mjai_type_str("9s").unwrap(), tt(26));
    }

    #[test]
    fn from_mjai_type_str_parses_honors() {
        assert_eq!(TileType::from_mjai_type_str("E").unwrap(), tt(27));
        assert_eq!(TileType::from_mjai_type_str("S").unwrap(), tt(28));
        assert_eq!(TileType::from_mjai_type_str("W").unwrap(), tt(29));
        assert_eq!(TileType::from_mjai_type_str("N").unwrap(), tt(30));
        assert_eq!(TileType::from_mjai_type_str("P").unwrap(), tt(31));
        assert_eq!(TileType::from_mjai_type_str("F").unwrap(), tt(32));
        assert_eq!(TileType::from_mjai_type_str("C").unwrap(), tt(33));
    }

    #[test]
    fn from_mjai_type_str_parses_red_fives() {
        assert_eq!(TileType::from_mjai_type_str("5mr").unwrap(), tt(4));
        assert_eq!(TileType::from_mjai_type_str("5pr").unwrap(), tt(13));
        assert_eq!(TileType::from_mjai_type_str("5sr").unwrap(), tt(22));
    }

    #[test]
    fn from_mjai_type_str_rejects_invalid() {
        assert!(TileType::from_mjai_type_str("?").is_err());
        assert!(TileType::from_mjai_type_str("").is_err());
        assert!(TileType::from_mjai_type_str("0m").is_err());
        assert!(TileType::from_mjai_type_str("10m").is_err());
        assert!(TileType::from_mjai_type_str("5x").is_err());
        assert!(TileType::from_mjai_type_str("4mr").is_err());
        assert!(TileType::from_mjai_type_str("e").is_err());
    }

    #[test]
    fn tile_type_to_mjai_string() {
        assert_eq!(tt(0).to_mjai_string(), "1m");
        assert_eq!(tt(17).to_mjai_string(), "9p");
        assert_eq!(tt(22).to_mjai_string(), "5s");
        assert_eq!(tt(27).to_mjai_string(), "E");
        assert_eq!(tt(33).to_mjai_string(), "C");
    }

    #[test]
    fn tile_id_to_mjai_string() {
        assert_eq!(id(16).to_mjai_string(), "5mr");
        assert_eq!(id(17).to_mjai_string(), "5m");
        assert_eq!(id(52).to_mjai_string(), "5pr");
        assert_eq!(id(53).to_mjai_string(), "5p");
        assert_eq!(id(88).to_mjai_string(), "5sr");
        assert_eq!(id(89).to_mjai_string(), "5s");
        assert_eq!(id(0).to_mjai_string(), "1m");
        assert_eq!(id(135).to_mjai_string(), "C");
    }

    #[test]
    fn count_dora_counts_indicators_and_red() {
        let indicator_4m = id(12);
        let another_4m = id(13);
        let indicator_9m = id(32);
        let red_5m = id(16);
        let black_5m = id(17);
        assert_eq!(count_dora(black_5m, &[indicator_4m]), 1);
        assert_eq!(count_dora(red_5m, &[indicator_4m]), 2);
        assert_eq!(count_dora(black_5m, &[indicator_4m, another_4m]), 2);
        assert_eq!(count_dora(red_5m, &[]), 1);
        assert_eq!(count_dora(id(0), &[]), 0);
        assert_eq!(count_dora(id(0), &[indicator_9m]), 1);
    }

    #[test]
    fn visible_tile_variants() {
        let known = VisibleTile::Known(id(16));
        assert_eq!(known, VisibleTile::Known(id(16)));
        assert_ne!(known, VisibleTile::Unknown);
    }

    #[test]
    fn all_yields_every_tile_type() {
        let all: Vec<_> = TileType::all().collect();
        assert_eq!(all.len(), TileType::COUNT);
        assert_eq!(all[0], tt(0));
        assert_eq!(all[33], tt(33));
    }
}
