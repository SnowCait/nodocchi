use bot_logic::TileType;
use thiserror::Error;

const HONOR_MJAI_TOKENS: [&str; 7] = ["E", "S", "W", "N", "P", "F", "C"];

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum TileInputError {
    #[error("token {token:?}: number {number:?} has no suit, expected one of m/p/s/z")]
    MissingSuit { token: String, number: String },

    #[error("token {token:?}: suit {suit:?} has no number")]
    MissingNumber { token: String, suit: char },

    #[error("token {token:?}: unknown suit {suit:?}, expected one of m/p/s/z")]
    UnknownSuit { token: String, suit: char },

    #[error("token {token:?}: {number}z is not an honor tile, expected 1z..7z")]
    InvalidHonor { token: String, number: char },

    #[error("token {token:?}: {number}{suit} is not a tile")]
    InvalidNumber {
        token: String,
        number: char,
        suit: char,
    },

    #[error(
        "token {token:?}: 10{suit} is ambiguous, 0 means the red five so write it as \"1{suit} 0{suit}\""
    )]
    AmbiguousTen { token: String, suit: char },

    #[error("token {token:?}: red five 0{suit} appears more than once")]
    DuplicateRedFive { token: String, suit: char },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LogicalTile {
    pub tile_type: TileType,
    pub red: bool,
}

impl LogicalTile {
    pub fn black(tile_type: TileType) -> Self {
        Self {
            tile_type,
            red: false,
        }
    }

    pub fn red(tile_type: TileType) -> Self {
        Self {
            tile_type,
            red: true,
        }
    }

    pub fn to_mjai_string(self) -> String {
        let mut text = self.tile_type.to_mjai_string();
        if self.red {
            text.push('r');
        }
        text
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MpszSuit {
    Man,
    Pin,
    Sou,
    Honor,
}

impl MpszSuit {
    fn parse(suit: char) -> Option<Self> {
        match suit {
            'm' => Some(Self::Man),
            'p' => Some(Self::Pin),
            's' => Some(Self::Sou),
            'z' => Some(Self::Honor),
            _ => None,
        }
    }

    fn label(self) -> char {
        match self {
            Self::Man => 'm',
            Self::Pin => 'p',
            Self::Sou => 's',
            Self::Honor => 'z',
        }
    }
}

pub fn parse_tiles(input: &str) -> Result<Vec<LogicalTile>, TileInputError> {
    let mut tiles = Vec::new();
    for token in input.split_whitespace() {
        parse_token(token, &mut tiles)?;
    }
    Ok(tiles)
}

fn parse_token(token: &str, tiles: &mut Vec<LogicalTile>) -> Result<(), TileInputError> {
    if let Some(tile) = parse_mjai_token(token) {
        tiles.push(tile);
        return Ok(());
    }
    parse_mpsz_token(token, tiles)
}

fn parse_mjai_token(token: &str) -> Option<LogicalTile> {
    let tile_type = TileType::from_mjai_type_str(token).ok()?;
    Some(LogicalTile {
        tile_type,
        red: token.ends_with('r'),
    })
}

fn parse_mpsz_token(token: &str, tiles: &mut Vec<LogicalTile>) -> Result<(), TileInputError> {
    let mut numbers = String::new();
    let mut red_suits: Vec<char> = Vec::new();

    for character in token.chars() {
        if character.is_ascii_digit() {
            numbers.push(character);
            continue;
        }

        let suit = MpszSuit::parse(character).ok_or_else(|| TileInputError::UnknownSuit {
            token: token.to_string(),
            suit: character,
        })?;
        if numbers.is_empty() {
            return Err(TileInputError::MissingNumber {
                token: token.to_string(),
                suit: character,
            });
        }

        expand_numbers(token, &numbers, suit, &mut red_suits, tiles)?;
        numbers.clear();
    }

    if numbers.is_empty() {
        Ok(())
    } else {
        Err(TileInputError::MissingSuit {
            token: token.to_string(),
            number: numbers,
        })
    }
}

fn expand_numbers(
    token: &str,
    numbers: &str,
    suit: MpszSuit,
    red_suits: &mut Vec<char>,
    tiles: &mut Vec<LogicalTile>,
) -> Result<(), TileInputError> {
    let numbers: Vec<char> = numbers.chars().collect();

    for (index, &number) in numbers.iter().enumerate() {
        let tile = match suit {
            MpszSuit::Honor => honor_tile(token, number)?,
            _ if number == '0' => {
                let previous = index.checked_sub(1).and_then(|index| numbers.get(index));
                if previous == Some(&'1') {
                    return Err(TileInputError::AmbiguousTen {
                        token: token.to_string(),
                        suit: suit.label(),
                    });
                }
                if red_suits.contains(&suit.label()) {
                    return Err(TileInputError::DuplicateRedFive {
                        token: token.to_string(),
                        suit: suit.label(),
                    });
                }
                red_suits.push(suit.label());
                LogicalTile::red(suited_tile_type(token, '5', suit)?)
            }
            _ => LogicalTile::black(suited_tile_type(token, number, suit)?),
        };
        tiles.push(tile);
    }

    Ok(())
}

fn honor_tile(token: &str, number: char) -> Result<LogicalTile, TileInputError> {
    let invalid = || TileInputError::InvalidHonor {
        token: token.to_string(),
        number,
    };
    let index = number.to_digit(10).ok_or_else(invalid)? as usize;
    let mjai = HONOR_MJAI_TOKENS
        .get(index.wrapping_sub(1))
        .ok_or_else(invalid)?;
    let tile_type = TileType::from_mjai_type_str(mjai).map_err(|_| invalid())?;
    Ok(LogicalTile::black(tile_type))
}

fn suited_tile_type(token: &str, number: char, suit: MpszSuit) -> Result<TileType, TileInputError> {
    let mjai = format!("{number}{}", suit.label());
    TileType::from_mjai_type_str(&mjai).map_err(|_| TileInputError::InvalidNumber {
        token: token.to_string(),
        number,
        suit: suit.label(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parsed(input: &str) -> Vec<String> {
        parse_tiles(input)
            .unwrap()
            .into_iter()
            .map(|tile| tile.to_mjai_string())
            .collect()
    }

    #[test]
    fn parses_compressed_man_tiles() {
        assert_eq!(parsed("123m"), ["1m", "2m", "3m"]);
    }

    #[test]
    fn parses_compressed_pin_tiles_with_red_five() {
        assert_eq!(parsed("405p"), ["4p", "5pr", "5p"]);
    }

    #[test]
    fn parses_compressed_honor_tiles() {
        assert_eq!(parsed("1234567z"), ["E", "S", "W", "N", "P", "F", "C"]);
    }

    #[test]
    fn parses_compressed_multi_suit_token() {
        assert_eq!(
            parsed("123m456p789s1234z"),
            [
                "1m", "2m", "3m", "4p", "5p", "6p", "7s", "8s", "9s", "E", "S", "W", "N"
            ]
        );
    }

    #[test]
    fn parses_space_separated_tokens() {
        assert_eq!(
            parsed("234m 455p 789s 1234z"),
            [
                "2m", "3m", "4m", "4p", "5p", "5p", "7s", "8s", "9s", "E", "S", "W", "N"
            ]
        );
    }

    #[test]
    fn parses_mixed_mpsz_and_mjai_tokens() {
        assert_eq!(
            parsed("234m 5pr 67p E"),
            ["2m", "3m", "4m", "5pr", "6p", "7p", "E"]
        );
    }

    #[test]
    fn keeps_input_order() {
        assert_eq!(parsed("9s 1m E 5s"), ["9s", "1m", "E", "5s"]);
    }

    #[test]
    fn parses_mpsz_red_fives() {
        assert_eq!(parsed("0m"), ["5mr"]);
        assert_eq!(parsed("0p"), ["5pr"]);
        assert_eq!(parsed("0s"), ["5sr"]);
    }

    #[test]
    fn mpsz_red_five_equals_mjai_red_five() {
        for (mpsz, mjai) in [("0m", "5mr"), ("0p", "5pr"), ("0s", "5sr")] {
            assert_eq!(parse_tiles(mpsz).unwrap(), parse_tiles(mjai).unwrap());
        }
    }

    #[test]
    fn red_five_and_black_five_are_different_logical_tiles() {
        let red = parse_tiles("0m").unwrap();
        let black = parse_tiles("5m").unwrap();
        assert_ne!(red, black);
        assert_eq!(red[0].tile_type, black[0].tile_type);
        assert!(red[0].red);
        assert!(!black[0].red);
    }

    #[test]
    fn mpsz_red_five_expands_inside_a_run() {
        assert_eq!(parsed("406m"), ["4m", "5mr", "6m"]);
    }

    #[test]
    fn parses_mjai_honor_tokens() {
        assert_eq!(parsed("E S W N P F C"), ["E", "S", "W", "N", "P", "F", "C"]);
    }

    #[test]
    fn empty_input_parses_to_no_tiles() {
        assert!(parse_tiles("").unwrap().is_empty());
        assert!(parse_tiles("   ").unwrap().is_empty());
    }

    #[test]
    fn rejects_number_without_suit() {
        assert_eq!(
            parse_tiles("123"),
            Err(TileInputError::MissingSuit {
                token: "123".to_string(),
                number: "123".to_string(),
            })
        );
    }

    #[test]
    fn rejects_unknown_suit() {
        assert_eq!(
            parse_tiles("123x"),
            Err(TileInputError::UnknownSuit {
                token: "123x".to_string(),
                suit: 'x',
            })
        );
    }

    #[test]
    fn rejects_out_of_range_honor() {
        assert_eq!(
            parse_tiles("8z"),
            Err(TileInputError::InvalidHonor {
                token: "8z".to_string(),
                number: '8',
            })
        );
    }

    #[test]
    fn rejects_zero_honor() {
        assert_eq!(
            parse_tiles("0z"),
            Err(TileInputError::InvalidHonor {
                token: "0z".to_string(),
                number: '0',
            })
        );
    }

    #[test]
    fn rejects_repeated_red_five_in_token() {
        assert_eq!(
            parse_tiles("00m"),
            Err(TileInputError::DuplicateRedFive {
                token: "00m".to_string(),
                suit: 'm',
            })
        );
        assert!(parse_tiles("0m0m").is_err());
    }

    #[test]
    fn rejects_red_marker_without_suit() {
        assert_eq!(
            parse_tiles("5r"),
            Err(TileInputError::UnknownSuit {
                token: "5r".to_string(),
                suit: 'r',
            })
        );
    }

    #[test]
    fn rejects_ambiguous_ten() {
        assert_eq!(
            parse_tiles("10m"),
            Err(TileInputError::AmbiguousTen {
                token: "10m".to_string(),
                suit: 'm',
            })
        );
        assert!(parse_tiles("10p").is_err());
        assert!(parse_tiles("10s").is_err());
    }

    #[test]
    fn rejects_suit_without_number() {
        assert_eq!(
            parse_tiles("m123"),
            Err(TileInputError::MissingNumber {
                token: "m123".to_string(),
                suit: 'm',
            })
        );
    }

    #[test]
    fn rejects_invalid_red_marker_on_other_numbers() {
        assert!(parse_tiles("4mr").is_err());
    }

    #[test]
    fn error_reports_offending_token_only() {
        let error = parse_tiles("123m 8z 456p").unwrap_err();
        assert_eq!(
            error,
            TileInputError::InvalidHonor {
                token: "8z".to_string(),
                number: '8',
            }
        );
    }
}
