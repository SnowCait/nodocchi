use std::fmt;
use std::str::FromStr;

use thiserror::Error;

use crate::convert::{
    extract_chiihou_pais_from_emoji_text, extract_concealed_chiihou_pais_from_emoji_text,
};

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum ChiihouPaiParseError {
    #[error("invalid chiihou pai string: {0:?}")]
    Invalid(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum ChiihouSuit {
    Man,
    Pin,
    Sou,
    Zi,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ChiihouPai {
    suit: ChiihouSuit,
    number: u8,
}

impl ChiihouPai {
    pub fn new(number: u8, suit: ChiihouSuit) -> Option<Self> {
        let max = match suit {
            ChiihouSuit::Man | ChiihouSuit::Pin | ChiihouSuit::Sou => 9,
            ChiihouSuit::Zi => 7,
        };
        (1..=max).contains(&number).then_some(Self { suit, number })
    }

    pub fn number(self) -> u8 {
        self.number
    }

    pub fn suit(self) -> ChiihouSuit {
        self.suit
    }
}

impl FromStr for ChiihouPai {
    type Err = ChiihouPaiParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let invalid = || ChiihouPaiParseError::Invalid(s.to_string());
        let [number @ b'1'..=b'9', suit] = s.as_bytes() else {
            return Err(invalid());
        };
        let suit = match suit {
            b'm' => ChiihouSuit::Man,
            b'p' => ChiihouSuit::Pin,
            b's' => ChiihouSuit::Sou,
            b'z' => ChiihouSuit::Zi,
            _ => return Err(invalid()),
        };
        Self::new(number - b'0', suit).ok_or_else(invalid)
    }
}

impl fmt::Display for ChiihouPai {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let suit = match self.suit {
            ChiihouSuit::Man => 'm',
            ChiihouSuit::Pin => 'p',
            ChiihouSuit::Sou => 's',
            ChiihouSuit::Zi => 'z',
        };
        write!(f, "{}{}", self.number, suit)
    }
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum ChiihouCompactPaiParseError {
    #[error("compact pai string is not ascii")]
    NotAscii,
    #[error("compact pai string has odd length: {0}")]
    OddLength(usize),
    #[error("invalid chiihou pai string: {0:?}")]
    InvalidPai(String),
}

pub fn parse_compact_chiihou_pais(s: &str) -> Result<Vec<ChiihouPai>, ChiihouCompactPaiParseError> {
    if !s.is_ascii() {
        return Err(ChiihouCompactPaiParseError::NotAscii);
    }
    if !s.len().is_multiple_of(2) {
        return Err(ChiihouCompactPaiParseError::OddLength(s.len()));
    }
    s.as_bytes()
        .as_chunks::<2>()
        .0
        .iter()
        .map(|chunk| {
            let token =
                std::str::from_utf8(chunk).map_err(|_| ChiihouCompactPaiParseError::NotAscii)?;
            token
                .parse()
                .map_err(|_| ChiihouCompactPaiParseError::InvalidPai(token.to_string()))
        })
        .collect()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChiihouNakuAction {
    Ron,
    Kan,
    Pon,
    Chi,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChiihouRequest {
    Sutehai {
        hand: Vec<ChiihouPai>,
        drawn: Option<ChiihouPai>,
    },
    Naku {
        hand: Vec<ChiihouPai>,
        target: ChiihouPai,
        actions: Vec<ChiihouNakuAction>,
    },
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum ChiihouProtocolError {
    #[error("missing command after GET")]
    MissingCommand,
    #[error("unknown GET command: {0:?}")]
    UnknownCommand(String),
    #[error("missing hand tiles")]
    MissingHand,
    #[error("invalid tile layout")]
    InvalidTileLayout,
    #[error("missing naku target tile")]
    MissingNakuTarget,
    #[error("missing naku actions")]
    MissingActions,
    #[error("unknown naku action: {0:?}")]
    UnknownAction(String),
}

pub fn parse_chiihou_request(
    content: &str,
) -> Result<Option<ChiihouRequest>, ChiihouProtocolError> {
    let lines: Vec<&str> = content.lines().collect();
    let Some(get_index) = lines
        .iter()
        .position(|line| line.split_whitespace().any(|token| token == "GET"))
    else {
        return Ok(None);
    };
    let mut tokens = lines[get_index]
        .split_whitespace()
        .skip_while(|token| *token != "GET")
        .skip(1);
    let Some(command) = tokens.next() else {
        return Err(ChiihouProtocolError::MissingCommand);
    };
    let groups = tile_groups(&lines[..get_index]);
    match command {
        "sutehai?" => parse_sutehai_request(groups).map(Some),
        "naku?" => parse_naku_request(groups, tokens).map(Some),
        _ => Err(ChiihouProtocolError::UnknownCommand(command.to_string())),
    }
}

fn tile_groups(lines: &[&str]) -> Vec<Vec<ChiihouPai>> {
    lines
        .iter()
        .flat_map(|line| line.split_whitespace())
        .filter(|token| !extract_chiihou_pais_from_emoji_text(token).is_empty())
        .map(extract_concealed_chiihou_pais_from_emoji_text)
        .collect()
}

fn parse_sutehai_request(
    groups: Vec<Vec<ChiihouPai>>,
) -> Result<ChiihouRequest, ChiihouProtocolError> {
    let mut groups = groups.into_iter();
    let Some(hand) = groups.next() else {
        return Err(ChiihouProtocolError::MissingHand);
    };
    let drawn = match groups.next() {
        None => None,
        Some(group) if group.len() == 1 => Some(group[0]),
        Some(_) => return Err(ChiihouProtocolError::InvalidTileLayout),
    };
    if groups.next().is_some() {
        return Err(ChiihouProtocolError::InvalidTileLayout);
    }
    Ok(ChiihouRequest::Sutehai { hand, drawn })
}

fn parse_naku_request<'a>(
    groups: Vec<Vec<ChiihouPai>>,
    tokens: impl Iterator<Item = &'a str>,
) -> Result<ChiihouRequest, ChiihouProtocolError> {
    let mut actions = Vec::new();
    for token in tokens {
        let action = match token {
            "ron" => ChiihouNakuAction::Ron,
            "kan" => ChiihouNakuAction::Kan,
            "pon" => ChiihouNakuAction::Pon,
            "chi" => ChiihouNakuAction::Chi,
            _ => return Err(ChiihouProtocolError::UnknownAction(token.to_string())),
        };
        if !actions.contains(&action) {
            actions.push(action);
        }
    }
    if actions.is_empty() {
        return Err(ChiihouProtocolError::MissingActions);
    }
    let mut groups = groups.into_iter();
    let Some(hand) = groups.next() else {
        return Err(ChiihouProtocolError::MissingHand);
    };
    let target = match groups.next() {
        Some(group) if group.len() == 1 => group[0],
        Some(_) => return Err(ChiihouProtocolError::InvalidTileLayout),
        None => return Err(ChiihouProtocolError::MissingNakuTarget),
    };
    if groups.next().is_some() {
        return Err(ChiihouProtocolError::InvalidTileLayout);
    }
    Ok(ChiihouRequest::Naku {
        hand,
        target,
        actions,
    })
}

pub fn chi_material_pairs(hand: &[ChiihouPai], target: ChiihouPai) -> Vec<[ChiihouPai; 2]> {
    let suit = target.suit();
    if suit == ChiihouSuit::Zi {
        return Vec::new();
    }
    let number = i16::from(target.number());
    [
        [number - 2, number - 1],
        [number - 1, number + 1],
        [number + 1, number + 2],
    ]
    .into_iter()
    .filter_map(|[first, second]| chi_material_pair(hand, suit, first, second))
    .collect()
}

fn chi_material_pair(
    hand: &[ChiihouPai],
    suit: ChiihouSuit,
    first: i16,
    second: i16,
) -> Option<[ChiihouPai; 2]> {
    let first = number_pai(suit, first)?;
    let second = number_pai(suit, second)?;
    (hand.contains(&first) && hand.contains(&second)).then_some([first, second])
}

fn number_pai(suit: ChiihouSuit, number: i16) -> Option<ChiihouPai> {
    ChiihouPai::new(u8::try_from(number).ok()?, suit)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pai(s: &str) -> ChiihouPai {
        s.parse().unwrap()
    }

    #[test]
    fn parses_all_valid_number_pais() {
        for suit in ['m', 'p', 's'] {
            for number in 1..=9 {
                let s = format!("{number}{suit}");
                let parsed: ChiihouPai = s.parse().unwrap();
                assert_eq!(parsed.to_string(), s);
            }
        }
    }

    #[test]
    fn parses_all_valid_honor_pais() {
        for number in 1..=7 {
            let s = format!("{number}z");
            let parsed: ChiihouPai = s.parse().unwrap();
            assert_eq!(parsed.to_string(), s);
        }
    }

    #[test]
    fn parse_roundtrips_representative_pais() {
        let parsed: ChiihouPai = "7z".parse().unwrap();
        assert_eq!(parsed.to_string(), "7z");
        assert_eq!(pai("1m").to_string(), "1m");
        assert_eq!(pai("9s").to_string(), "9s");
    }

    #[test]
    fn rejects_invalid_pai_strings() {
        for s in [
            "0m", "10m", "8z", "9z", "0z", "1x", "E", "5mr", "", "m", "m1", "5", "１m",
        ] {
            assert!(s.parse::<ChiihouPai>().is_err(), "input: {s:?}");
        }
    }

    #[test]
    fn new_validates_number_range() {
        assert!(ChiihouPai::new(9, ChiihouSuit::Man).is_some());
        assert!(ChiihouPai::new(7, ChiihouSuit::Zi).is_some());
        assert!(ChiihouPai::new(0, ChiihouSuit::Man).is_none());
        assert!(ChiihouPai::new(10, ChiihouSuit::Pin).is_none());
        assert!(ChiihouPai::new(8, ChiihouSuit::Zi).is_none());
    }

    #[test]
    fn number_and_suit_accessors() {
        let parsed = pai("5p");
        assert_eq!(parsed.number(), 5);
        assert_eq!(parsed.suit(), ChiihouSuit::Pin);
        let honor = pai("6z");
        assert_eq!(honor.number(), 6);
        assert_eq!(honor.suit(), ChiihouSuit::Zi);
    }

    #[test]
    fn parses_compact_thirteen_tile_hand() {
        assert_eq!(
            parse_compact_chiihou_pais("1m2m3m4p4p7s7s1z1z2z3z4z5z").unwrap(),
            vec![
                pai("1m"),
                pai("2m"),
                pai("3m"),
                pai("4p"),
                pai("4p"),
                pai("7s"),
                pai("7s"),
                pai("1z"),
                pai("1z"),
                pai("2z"),
                pai("3z"),
                pai("4z"),
                pai("5z"),
            ]
        );
    }

    #[test]
    fn parses_compact_hand_with_duplicate_tiles() {
        assert_eq!(
            parse_compact_chiihou_pais("5p5p5p5p").unwrap(),
            vec![pai("5p"); 4]
        );
    }

    #[test]
    fn compact_empty_string_is_empty() {
        assert_eq!(parse_compact_chiihou_pais("").unwrap(), vec![]);
    }

    #[test]
    fn compact_odd_length_is_error() {
        for s in ["1", "1m2", "1m2m3"] {
            assert_eq!(
                parse_compact_chiihou_pais(s),
                Err(ChiihouCompactPaiParseError::OddLength(s.len())),
                "input: {s:?}"
            );
        }
    }

    #[test]
    fn compact_invalid_pai_is_error() {
        for (s, invalid) in [("1m0m", "0m"), ("8z1m", "8z"), ("m1", "m1"), ("1m2x", "2x")] {
            assert_eq!(
                parse_compact_chiihou_pais(s),
                Err(ChiihouCompactPaiParseError::InvalidPai(invalid.to_string())),
                "input: {s:?}"
            );
        }
    }

    #[test]
    fn compact_non_ascii_is_error() {
        for s in ["１m2m", "1m東"] {
            assert_eq!(
                parse_compact_chiihou_pais(s),
                Err(ChiihouCompactPaiParseError::NotAscii),
                "input: {s:?}"
            );
        }
    }

    #[test]
    fn parses_sutehai_request_with_drawn() {
        let content = "\
:mahjong_m1::mahjong_m2::mahjong_m3: :mahjong_east:
nostr:npub1ai000 GET sutehai?";
        assert_eq!(
            parse_chiihou_request(content).unwrap(),
            Some(ChiihouRequest::Sutehai {
                hand: vec![pai("1m"), pai("2m"), pai("3m")],
                drawn: Some(pai("1z")),
            })
        );
    }

    #[test]
    fn parses_sutehai_request_without_drawn() {
        let content = "\
:mahjong_m1::mahjong_m2::mahjong_m3:
nostr:npub1ai000 GET sutehai?";
        assert_eq!(
            parse_chiihou_request(content).unwrap(),
            Some(ChiihouRequest::Sutehai {
                hand: vec![pai("1m"), pai("2m"), pai("3m")],
                drawn: None,
            })
        );
    }

    #[test]
    fn parses_naku_request_with_actions() {
        let content = "\
:mahjong_m1::mahjong_m2::mahjong_m3: :mahjong_m4:
nostr:npub1ai000 GET naku? ron pon chi";
        assert_eq!(
            parse_chiihou_request(content).unwrap(),
            Some(ChiihouRequest::Naku {
                hand: vec![pai("1m"), pai("2m"), pai("3m")],
                target: pai("4m"),
                actions: vec![
                    ChiihouNakuAction::Ron,
                    ChiihouNakuAction::Pon,
                    ChiihouNakuAction::Chi,
                ],
            })
        );
    }

    #[test]
    fn parses_naku_request_with_ron_only() {
        let content = "\
:mahjong_m1::mahjong_m2::mahjong_m3: :mahjong_m4:
nostr:npub1ai000 GET naku? ron";
        assert_eq!(
            parse_chiihou_request(content).unwrap(),
            Some(ChiihouRequest::Naku {
                hand: vec![pai("1m"), pai("2m"), pai("3m")],
                target: pai("4m"),
                actions: vec![ChiihouNakuAction::Ron],
            })
        );
    }

    #[test]
    fn parses_naku_request_actions_in_any_order() {
        let content = "\
:mahjong_m1::mahjong_m2::mahjong_m3: :mahjong_m4:
nostr:npub1ai000 GET naku? chi pon ron";
        assert_eq!(
            parse_chiihou_request(content).unwrap(),
            Some(ChiihouRequest::Naku {
                hand: vec![pai("1m"), pai("2m"), pai("3m")],
                target: pai("4m"),
                actions: vec![
                    ChiihouNakuAction::Chi,
                    ChiihouNakuAction::Pon,
                    ChiihouNakuAction::Ron,
                ],
            })
        );
    }

    #[test]
    fn parses_naku_request_deduplicating_actions() {
        let content = "\
:mahjong_m1::mahjong_m2::mahjong_m3: :mahjong_m4:
nostr:npub1ai000 GET naku? ron ron pon ron";
        assert_eq!(
            parse_chiihou_request(content).unwrap(),
            Some(ChiihouRequest::Naku {
                hand: vec![pai("1m"), pai("2m"), pai("3m")],
                target: pai("4m"),
                actions: vec![ChiihouNakuAction::Ron, ChiihouNakuAction::Pon],
            })
        );
    }

    #[test]
    fn parses_naku_request_with_kan() {
        let content = "\
:mahjong_white::mahjong_white::mahjong_white: :mahjong_white:
nostr:npub1ai000 GET naku? kan";
        assert_eq!(
            parse_chiihou_request(content).unwrap(),
            Some(ChiihouRequest::Naku {
                hand: vec![pai("5z"), pai("5z"), pai("5z")],
                target: pai("5z"),
                actions: vec![ChiihouNakuAction::Kan],
            })
        );
    }

    #[test]
    fn unrelated_content_is_none() {
        for content in [
            "",
            "gamestart",
            "こんにちは",
            ":mahjong_m1::mahjong_m2:",
            "nostr:npub1ai000 sutehai? sutehai 7z",
        ] {
            assert_eq!(
                parse_chiihou_request(content).unwrap(),
                None,
                "content: {content:?}"
            );
        }
    }

    #[test]
    fn get_without_command_is_error() {
        assert_eq!(
            parse_chiihou_request("nostr:npub1ai000 GET"),
            Err(ChiihouProtocolError::MissingCommand)
        );
    }

    #[test]
    fn unknown_get_command_is_error() {
        assert_eq!(
            parse_chiihou_request(":mahjong_m1:\nnostr:npub1ai000 GET foo?"),
            Err(ChiihouProtocolError::UnknownCommand("foo?".to_string()))
        );
    }

    #[test]
    fn sutehai_without_hand_is_error() {
        assert_eq!(
            parse_chiihou_request("nostr:npub1ai000 GET sutehai?"),
            Err(ChiihouProtocolError::MissingHand)
        );
    }

    #[test]
    fn sutehai_with_multiple_drawn_pais_is_error() {
        let content = "\
:mahjong_m1::mahjong_m2: :mahjong_m3::mahjong_m4:
nostr:npub1ai000 GET sutehai?";
        assert_eq!(
            parse_chiihou_request(content),
            Err(ChiihouProtocolError::InvalidTileLayout)
        );
    }

    #[test]
    fn naku_without_actions_is_error() {
        let content = "\
:mahjong_m1::mahjong_m2::mahjong_m3: :mahjong_m4:
nostr:npub1ai000 GET naku?";
        assert_eq!(
            parse_chiihou_request(content),
            Err(ChiihouProtocolError::MissingActions)
        );
    }

    #[test]
    fn naku_with_unknown_action_is_error() {
        let content = "\
:mahjong_m1::mahjong_m2::mahjong_m3: :mahjong_m4:
nostr:npub1ai000 GET naku? ron riichi";
        assert_eq!(
            parse_chiihou_request(content),
            Err(ChiihouProtocolError::UnknownAction("riichi".to_string()))
        );
    }

    fn pais(names: &[&str]) -> Vec<ChiihouPai> {
        names.iter().map(|name| pai(name)).collect()
    }

    fn pair(first: &str, second: &str) -> [ChiihouPai; 2] {
        [pai(first), pai(second)]
    }

    const CHI_MELD: &str = "<:mahjong_m1::mahjong_m2::mahjong_m3:>";
    const PON_MELD: &str = "<:mahjong_south::mahjong_south::mahjong_south:>";
    const DAIMINKAN_MELD: &str = "<:mahjong_white::mahjong_white::mahjong_white::mahjong_white:>";
    const ANKAN_MELD: &str = "(:mahjong_east::mahjong_east::mahjong_east::mahjong_east:)";

    #[test]
    fn sutehai_hand_without_melds_is_unchanged() {
        let content = "\
:mahjong_m2::mahjong_m4::mahjong_p7: :mahjong_m5:
nostr:npub1ai000 GET sutehai?";
        assert_eq!(
            parse_chiihou_request(content).unwrap(),
            Some(ChiihouRequest::Sutehai {
                hand: pais(&["2m", "4m", "7p"]),
                drawn: Some(pai("5m")),
            })
        );
    }

    #[test]
    fn sutehai_hand_excludes_chi_meld_tiles() {
        let content = format!(
            "\
:mahjong_m2::mahjong_m4::mahjong_p7:{CHI_MELD} :mahjong_m5:
nostr:npub1ai000 GET sutehai?"
        );
        assert_eq!(
            parse_chiihou_request(&content).unwrap(),
            Some(ChiihouRequest::Sutehai {
                hand: pais(&["2m", "4m", "7p"]),
                drawn: Some(pai("5m")),
            })
        );
    }

    #[test]
    fn sutehai_hand_excludes_ankan_tiles() {
        let content = format!(
            "\
:mahjong_m2::mahjong_m4::mahjong_p7:{ANKAN_MELD} :mahjong_m5:
nostr:npub1ai000 GET sutehai?"
        );
        assert_eq!(
            parse_chiihou_request(&content).unwrap(),
            Some(ChiihouRequest::Sutehai {
                hand: pais(&["2m", "4m", "7p"]),
                drawn: Some(pai("5m")),
            })
        );
    }

    #[test]
    fn sutehai_hand_excludes_every_meld() {
        let content = format!(
            "\
:mahjong_m2::mahjong_m4::mahjong_p7::mahjong_p9:{CHI_MELD}{PON_MELD}{DAIMINKAN_MELD}{ANKAN_MELD} :mahjong_m5:
nostr:npub1ai000 GET sutehai?"
        );
        assert_eq!(
            parse_chiihou_request(&content).unwrap(),
            Some(ChiihouRequest::Sutehai {
                hand: pais(&["2m", "4m", "7p", "9p"]),
                drawn: Some(pai("5m")),
            })
        );
    }

    #[test]
    fn sutehai_after_naku_has_meld_but_no_drawn_pai() {
        let content = format!(
            "\
:mahjong_m2::mahjong_m4:{PON_MELD}
nostr:npub1ai000 GET sutehai?"
        );
        assert_eq!(
            parse_chiihou_request(&content).unwrap(),
            Some(ChiihouRequest::Sutehai {
                hand: pais(&["2m", "4m"]),
                drawn: None,
            })
        );
    }

    #[test]
    fn naku_hand_excludes_pon_meld_tiles() {
        let content = format!(
            "\
:mahjong_m2::mahjong_m4:{PON_MELD} :mahjong_m3:
nostr:npub1ai000 GET naku? chi"
        );
        assert_eq!(
            parse_chiihou_request(&content).unwrap(),
            Some(ChiihouRequest::Naku {
                hand: pais(&["2m", "4m"]),
                target: pai("3m"),
                actions: vec![ChiihouNakuAction::Chi],
            })
        );
    }

    #[test]
    fn naku_hand_excludes_every_meld() {
        let content = format!(
            "\
:mahjong_m2::mahjong_m4:{CHI_MELD}{PON_MELD}{ANKAN_MELD} :mahjong_m3:
nostr:npub1ai000 GET naku? chi"
        );
        assert_eq!(
            parse_chiihou_request(&content).unwrap(),
            Some(ChiihouRequest::Naku {
                hand: pais(&["2m", "4m"]),
                target: pai("3m"),
                actions: vec![ChiihouNakuAction::Chi],
            })
        );
    }

    #[test]
    fn naku_target_is_kept_outside_the_hand() {
        let content = format!(
            "\
:mahjong_east::mahjong_east:{PON_MELD} :mahjong_east:
nostr:npub1ai000 GET naku? pon kan"
        );
        assert_eq!(
            parse_chiihou_request(&content).unwrap(),
            Some(ChiihouRequest::Naku {
                hand: pais(&["1z", "1z"]),
                target: pai("1z"),
                actions: vec![ChiihouNakuAction::Pon, ChiihouNakuAction::Kan],
            })
        );
    }

    #[test]
    fn chi_material_has_single_pair_for_lower_run() {
        assert_eq!(
            chi_material_pairs(&pais(&["1m", "2m", "9s", "1z"]), pai("3m")),
            vec![pair("1m", "2m")]
        );
    }

    #[test]
    fn chi_material_has_three_pairs_for_middle_pai() {
        assert_eq!(
            chi_material_pairs(&pais(&["1m", "2m", "4m", "5m"]), pai("3m")),
            vec![pair("1m", "2m"), pair("2m", "4m"), pair("4m", "5m")]
        );
    }

    #[test]
    fn chi_material_pairs_are_ascending() {
        for pairs in [
            chi_material_pairs(&pais(&["1m", "2m", "4m", "5m"]), pai("3m")),
            chi_material_pairs(&pais(&["7p", "8p"]), pai("9p")),
        ] {
            assert!(!pairs.is_empty());
            for [first, second] in pairs {
                assert_eq!(first.suit(), second.suit());
                assert!(first.number() < second.number(), "{first} {second}");
            }
        }
    }

    #[test]
    fn chi_material_clamps_lower_edge_pai() {
        assert_eq!(
            chi_material_pairs(&pais(&["2m", "3m", "4m"]), pai("1m")),
            vec![pair("2m", "3m")]
        );
    }

    #[test]
    fn chi_material_clamps_upper_edge_pai() {
        assert_eq!(
            chi_material_pairs(&pais(&["6m", "7m", "8m"]), pai("9m")),
            vec![pair("7m", "8m")]
        );
    }

    #[test]
    fn chi_material_is_empty_for_honor_pai() {
        assert!(chi_material_pairs(&pais(&["1z", "1z", "2z", "3z"]), pai("2z")).is_empty());
    }

    #[test]
    fn chi_material_is_empty_without_materials() {
        assert!(chi_material_pairs(&pais(&["1m", "5m", "9m"]), pai("3m")).is_empty());
    }

    #[test]
    fn chi_material_ignores_other_suits() {
        assert!(chi_material_pairs(&pais(&["1p", "2p", "1s", "2s"]), pai("3m")).is_empty());
    }

    #[test]
    fn chi_material_does_not_duplicate_pairs_for_duplicate_tiles() {
        assert_eq!(
            chi_material_pairs(&pais(&["1m", "1m", "2m", "2m"]), pai("3m")),
            vec![pair("1m", "2m")]
        );
    }

    #[test]
    fn naku_without_target_is_error() {
        let content = "\
:mahjong_m1::mahjong_m2::mahjong_m3:
nostr:npub1ai000 GET naku? ron";
        assert_eq!(
            parse_chiihou_request(content),
            Err(ChiihouProtocolError::MissingNakuTarget)
        );
    }
}
