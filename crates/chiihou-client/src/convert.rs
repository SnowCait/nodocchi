use bot_logic::{TileId, TileType};

use crate::protocol::{ChiihouPai, ChiihouSuit};

pub fn tile_type_from_chiihou_pai(pai: ChiihouPai) -> TileType {
    let base = match pai.suit() {
        ChiihouSuit::Man => 0,
        ChiihouSuit::Pin => 9,
        ChiihouSuit::Sou => 18,
        ChiihouSuit::Zi => 27,
    };
    TileType::new(base + pai.number() - 1).expect("valid chiihou pai maps to a tile type")
}

pub fn temporary_tile_id_from_chiihou_pai(pai: ChiihouPai) -> TileId {
    let base = tile_type_from_chiihou_pai(pai).raw() * 4;
    let offset = u8::from(matches!(base, 16 | 52 | 88));
    TileId::new(base + offset).expect("valid chiihou pai maps to a tile id")
}

pub fn chiihou_pai_from_tile_id(tile: TileId) -> ChiihouPai {
    let raw = tile.tile_type().raw();
    let (number, suit) = match raw {
        0..=8 => (raw + 1, ChiihouSuit::Man),
        9..=17 => (raw - 8, ChiihouSuit::Pin),
        18..=26 => (raw - 17, ChiihouSuit::Sou),
        _ => (raw - 26, ChiihouSuit::Zi),
    };
    ChiihouPai::new(number, suit).expect("valid tile id maps to a chiihou pai")
}

pub fn emoji_shortcode_to_chiihou_pai(shortcode: &str) -> Option<ChiihouPai> {
    let name = shortcode.strip_prefix(':').unwrap_or(shortcode);
    let name = name.strip_suffix(':').unwrap_or(name);
    shortcode_name_to_chiihou_pai(name)
}

pub fn extract_chiihou_pais_from_emoji_text(text: &str) -> Vec<ChiihouPai> {
    let mut pais = Vec::new();
    let mut rest = text;
    while let Some(start) = rest.find(':') {
        let after = &rest[start + 1..];
        let Some(end) = after.find(':') else {
            break;
        };
        if let Some(pai) = shortcode_name_to_chiihou_pai(&after[..end]) {
            pais.push(pai);
            rest = &after[end + 1..];
        } else {
            rest = &after[end..];
        }
    }
    pais
}

fn shortcode_name_to_chiihou_pai(name: &str) -> Option<ChiihouPai> {
    let honor_number = match name {
        "mahjong_east" => Some(1),
        "mahjong_south" => Some(2),
        "mahjong_west" => Some(3),
        "mahjong_north" => Some(4),
        "mahjong_white" => Some(5),
        "mahjong_green" => Some(6),
        "mahjong_red" => Some(7),
        _ => None,
    };
    if let Some(number) = honor_number {
        return ChiihouPai::new(number, ChiihouSuit::Zi);
    }
    let rest = name.strip_prefix("mahjong_")?;
    let [suit, number @ b'1'..=b'9'] = rest.as_bytes() else {
        return None;
    };
    let suit = match suit {
        b'm' => ChiihouSuit::Man,
        b'p' => ChiihouSuit::Pin,
        b's' => ChiihouSuit::Sou,
        _ => return None,
    };
    ChiihouPai::new(number - b'0', suit)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pai(s: &str) -> ChiihouPai {
        s.parse().unwrap()
    }

    fn tile(value: u8) -> TileId {
        TileId::new(value).unwrap()
    }

    #[test]
    fn honor_pais_convert_to_mjai_honor_tile_types() {
        for (chiihou, mjai) in [
            ("1z", "E"),
            ("2z", "S"),
            ("3z", "W"),
            ("4z", "N"),
            ("5z", "P"),
            ("6z", "F"),
            ("7z", "C"),
        ] {
            assert_eq!(
                tile_type_from_chiihou_pai(pai(chiihou)),
                TileType::from_mjai_type_str(mjai).unwrap(),
                "chiihou: {chiihou}"
            );
        }
    }

    #[test]
    fn number_pais_convert_to_mjai_tile_types() {
        for s in ["1m", "9m", "1p", "9p", "1s", "9s"] {
            assert_eq!(
                tile_type_from_chiihou_pai(pai(s)),
                TileType::from_mjai_type_str(s).unwrap(),
                "chiihou: {s}"
            );
        }
    }

    #[test]
    fn converts_to_temporary_tile_ids() {
        for (s, expected) in [
            ("1m", 0),
            ("5m", 17),
            ("9m", 32),
            ("5p", 53),
            ("5s", 89),
            ("1z", 108),
            ("2z", 112),
            ("3z", 116),
            ("4z", 120),
            ("5z", 124),
            ("6z", 128),
            ("7z", 132),
        ] {
            assert_eq!(
                temporary_tile_id_from_chiihou_pai(pai(s)),
                tile(expected),
                "chiihou: {s}"
            );
        }
    }

    #[test]
    fn converts_back_from_tile_ids() {
        for (value, expected) in [
            (0, "1m"),
            (17, "5m"),
            (53, "5p"),
            (89, "5s"),
            (108, "1z"),
            (124, "5z"),
            (132, "7z"),
            (135, "7z"),
        ] {
            assert_eq!(
                chiihou_pai_from_tile_id(tile(value)),
                pai(expected),
                "tile: {value}"
            );
        }
    }

    #[test]
    fn red_five_tile_ids_convert_to_plain_fives() {
        assert_eq!(chiihou_pai_from_tile_id(tile(16)), pai("5m"));
        assert_eq!(chiihou_pai_from_tile_id(tile(52)), pai("5p"));
        assert_eq!(chiihou_pai_from_tile_id(tile(88)), pai("5s"));
    }

    #[test]
    fn tile_id_roundtrips_through_chiihou_pai() {
        for value in 0..136 {
            let converted = chiihou_pai_from_tile_id(tile(value));
            assert_eq!(
                temporary_tile_id_from_chiihou_pai(converted).tile_type(),
                tile(value).tile_type(),
                "tile: {value}"
            );
        }
    }

    #[test]
    fn emoji_shortcodes_convert_to_number_pais() {
        for suit in ['m', 'p', 's'] {
            for number in 1..=9 {
                let shortcode = format!(":mahjong_{suit}{number}:");
                let expected = pai(&format!("{number}{suit}"));
                assert_eq!(
                    emoji_shortcode_to_chiihou_pai(&shortcode),
                    Some(expected),
                    "shortcode: {shortcode}"
                );
            }
        }
    }

    #[test]
    fn emoji_shortcodes_convert_to_honor_pais() {
        for (shortcode, expected) in [
            (":mahjong_east:", "1z"),
            (":mahjong_south:", "2z"),
            (":mahjong_west:", "3z"),
            (":mahjong_north:", "4z"),
            (":mahjong_white:", "5z"),
            (":mahjong_green:", "6z"),
            (":mahjong_red:", "7z"),
        ] {
            assert_eq!(
                emoji_shortcode_to_chiihou_pai(shortcode),
                Some(pai(expected)),
                "shortcode: {shortcode}"
            );
        }
    }

    #[test]
    fn emoji_shortcode_accepts_bare_name() {
        assert_eq!(
            emoji_shortcode_to_chiihou_pai("mahjong_m1"),
            Some(pai("1m"))
        );
        assert_eq!(
            emoji_shortcode_to_chiihou_pai("mahjong_east"),
            Some(pai("1z"))
        );
    }

    #[test]
    fn emoji_shortcode_rejects_unknown_names() {
        for shortcode in [
            ":mahjong_m0:",
            ":mahjong_z1:",
            ":mahjong_m10:",
            ":mahjong_blue:",
            ":smile:",
            "",
            "::",
        ] {
            assert_eq!(
                emoji_shortcode_to_chiihou_pai(shortcode),
                None,
                "shortcode: {shortcode:?}"
            );
        }
    }

    #[test]
    fn extracts_pais_from_emoji_text_in_order() {
        let text = ":mahjong_m1::mahjong_m2::mahjong_m3: :mahjong_east:";
        assert_eq!(
            extract_chiihou_pais_from_emoji_text(text),
            vec![pai("1m"), pai("2m"), pai("3m"), pai("1z")]
        );
    }

    #[test]
    fn extracts_pais_skipping_unknown_shortcodes() {
        let text = "hand :smile: :mahjong_p5: and :mahjong_red:!";
        assert_eq!(
            extract_chiihou_pais_from_emoji_text(text),
            vec![pai("5p"), pai("7z")]
        );
    }

    #[test]
    fn extracts_nothing_from_plain_text() {
        assert_eq!(extract_chiihou_pais_from_emoji_text("GET sutehai?"), vec![]);
        assert_eq!(
            extract_chiihou_pais_from_emoji_text("nostr:npub1ai000 GET sutehai?"),
            vec![]
        );
    }
}
