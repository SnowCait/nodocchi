use nostr_sdk::PublicKey;
use thiserror::Error;

use crate::lifecycle::{parse_u32, player_pubkey_from_token};
use crate::protocol::{ChiihouCompactPaiParseError, ChiihouPai, parse_compact_chiihou_pais};

pub(crate) const CHIIHOU_HAIPAI_TILE_COUNT: usize = 13;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChiihouSayAction {
    Richi,
    Tsumo,
    Ron,
    Pon,
    Chi,
    Kan,
    Tenpai,
    Noten,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChiihouTableNotification {
    Haipai {
        player: PublicKey,
        hand: Vec<ChiihouPai>,
    },
    Dora {
        indicator: ChiihouPai,
    },
    Tsumo {
        player: PublicKey,
        remaining_tiles: u32,
        tile: ChiihouPai,
    },
    Sutehai {
        player: PublicKey,
        tile: ChiihouPai,
    },
    Say {
        player: PublicKey,
        action: ChiihouSayAction,
    },
    Open {
        player: PublicKey,
        tiles: Vec<ChiihouPai>,
    },
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum ChiihouTableNotificationError {
    #[error("missing player")]
    MissingPlayer,
    #[error("invalid player public key")]
    InvalidPublicKey,
    #[error("missing hand in haipai")]
    MissingHand,
    #[error("invalid hand: {0}")]
    InvalidHand(#[from] ChiihouCompactPaiParseError),
    #[error("invalid hand tile count: {0}")]
    InvalidHandTileCount(usize),
    #[error("missing pai")]
    MissingPai,
    #[error("invalid pai: {0:?}")]
    InvalidPai(String),
    #[error("missing remaining tiles in tsumo")]
    MissingRemainingTiles,
    #[error("invalid remaining tiles: {0:?}")]
    InvalidRemainingTiles(String),
    #[error("missing open tiles")]
    MissingOpenTiles,
    #[error("invalid open tiles: {0}")]
    InvalidOpenTiles(ChiihouCompactPaiParseError),
    #[error("missing say action")]
    MissingAction,
    #[error("unknown say action: {0:?}")]
    UnknownAction(String),
    #[error("unexpected payload after command")]
    UnexpectedPayload,
}

pub fn parse_chiihou_table_notification(
    content: &str,
) -> Result<Option<ChiihouTableNotification>, ChiihouTableNotificationError> {
    let Some(line) = content
        .lines()
        .find(|line| line.split_whitespace().any(|token| token == "NOTIFY"))
    else {
        return Ok(None);
    };
    let mut tokens = line
        .split_whitespace()
        .skip_while(|token| *token != "NOTIFY")
        .skip(1);
    let Some(command) = tokens.next() else {
        return Ok(None);
    };
    match command {
        "haipai" => parse_haipai(tokens).map(Some),
        "dora" => parse_dora(tokens).map(Some),
        "tsumo" => parse_tsumo(tokens).map(Some),
        "sutehai" => parse_sutehai(tokens).map(Some),
        "say" => parse_say(tokens).map(Some),
        "open" => parse_open(tokens).map(Some),
        _ => Ok(None),
    }
}

fn parse_haipai<'a>(
    mut tokens: impl Iterator<Item = &'a str>,
) -> Result<ChiihouTableNotification, ChiihouTableNotificationError> {
    let player = parse_player(tokens.next())?;
    let Some(hand_token) = tokens.next() else {
        return Err(ChiihouTableNotificationError::MissingHand);
    };
    let hand = parse_compact_chiihou_pais(hand_token)?;
    if hand.len() != CHIIHOU_HAIPAI_TILE_COUNT {
        return Err(ChiihouTableNotificationError::InvalidHandTileCount(
            hand.len(),
        ));
    }
    ensure_no_remaining_tokens(tokens)?;
    Ok(ChiihouTableNotification::Haipai { player, hand })
}

fn parse_dora<'a>(
    mut tokens: impl Iterator<Item = &'a str>,
) -> Result<ChiihouTableNotification, ChiihouTableNotificationError> {
    let indicator = parse_pai(tokens.next())?;
    ensure_no_remaining_tokens(tokens)?;
    Ok(ChiihouTableNotification::Dora { indicator })
}

fn parse_tsumo<'a>(
    mut tokens: impl Iterator<Item = &'a str>,
) -> Result<ChiihouTableNotification, ChiihouTableNotificationError> {
    let player = parse_player(tokens.next())?;
    let Some(remaining_token) = tokens.next() else {
        return Err(ChiihouTableNotificationError::MissingRemainingTiles);
    };
    let remaining_tiles = parse_u32(remaining_token).ok_or_else(|| {
        ChiihouTableNotificationError::InvalidRemainingTiles(remaining_token.to_string())
    })?;
    let tile = parse_pai(tokens.next())?;
    ensure_no_remaining_tokens(tokens)?;
    Ok(ChiihouTableNotification::Tsumo {
        player,
        remaining_tiles,
        tile,
    })
}

fn parse_sutehai<'a>(
    mut tokens: impl Iterator<Item = &'a str>,
) -> Result<ChiihouTableNotification, ChiihouTableNotificationError> {
    let player = parse_player(tokens.next())?;
    let tile = parse_pai(tokens.next())?;
    ensure_no_remaining_tokens(tokens)?;
    Ok(ChiihouTableNotification::Sutehai { player, tile })
}

fn parse_say<'a>(
    mut tokens: impl Iterator<Item = &'a str>,
) -> Result<ChiihouTableNotification, ChiihouTableNotificationError> {
    let player = parse_player(tokens.next())?;
    let Some(action_token) = tokens.next() else {
        return Err(ChiihouTableNotificationError::MissingAction);
    };
    let action = match action_token {
        "richi" => ChiihouSayAction::Richi,
        "tsumo" => ChiihouSayAction::Tsumo,
        "ron" => ChiihouSayAction::Ron,
        "pon" => ChiihouSayAction::Pon,
        "chi" => ChiihouSayAction::Chi,
        "kan" => ChiihouSayAction::Kan,
        "tenpai" => ChiihouSayAction::Tenpai,
        "noten" => ChiihouSayAction::Noten,
        _ => {
            return Err(ChiihouTableNotificationError::UnknownAction(
                action_token.to_string(),
            ));
        }
    };
    ensure_no_remaining_tokens(tokens)?;
    Ok(ChiihouTableNotification::Say { player, action })
}

fn parse_open<'a>(
    mut tokens: impl Iterator<Item = &'a str>,
) -> Result<ChiihouTableNotification, ChiihouTableNotificationError> {
    let player = parse_player(tokens.next())?;
    let Some(tiles_token) = tokens.next() else {
        return Err(ChiihouTableNotificationError::MissingOpenTiles);
    };
    let tiles = parse_compact_chiihou_pais(tiles_token)
        .map_err(ChiihouTableNotificationError::InvalidOpenTiles)?;
    ensure_no_remaining_tokens(tokens)?;
    Ok(ChiihouTableNotification::Open { player, tiles })
}

fn parse_player(token: Option<&str>) -> Result<PublicKey, ChiihouTableNotificationError> {
    let Some(token) = token else {
        return Err(ChiihouTableNotificationError::MissingPlayer);
    };
    player_pubkey_from_token(token).ok_or(ChiihouTableNotificationError::InvalidPublicKey)
}

fn parse_pai(token: Option<&str>) -> Result<ChiihouPai, ChiihouTableNotificationError> {
    let Some(token) = token else {
        return Err(ChiihouTableNotificationError::MissingPai);
    };
    token
        .parse()
        .map_err(|_| ChiihouTableNotificationError::InvalidPai(token.to_string()))
}

fn ensure_no_remaining_tokens<'a>(
    mut tokens: impl Iterator<Item = &'a str>,
) -> Result<(), ChiihouTableNotificationError> {
    if tokens.next().is_some() {
        return Err(ChiihouTableNotificationError::UnexpectedPayload);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use nostr_sdk::nips::nip19::Nip19Profile;
    use nostr_sdk::{Keys, RelayUrl, ToBech32};

    // テスト専用の秘密鍵から鍵を導出する。実際の運用で使用してはならない。
    fn test_keys(index: u64) -> Keys {
        Keys::parse(&format!("{index:064x}")).unwrap()
    }

    fn player_pubkey(index: u64) -> PublicKey {
        test_keys(index).public_key()
    }

    fn npub(index: u64) -> String {
        player_pubkey(index).to_bech32().unwrap()
    }

    fn npub_token(index: u64) -> String {
        format!("nostr:{}", npub(index))
    }

    fn players_prefix() -> String {
        (1..=4).map(npub_token).collect::<Vec<_>>().join(" ")
    }

    fn pai(s: &str) -> ChiihouPai {
        s.parse().unwrap()
    }

    fn pais(items: &[&str]) -> Vec<ChiihouPai> {
        items.iter().map(|s| pai(s)).collect()
    }

    const HAIPAI_HAND: &str = "1m2m3m4m5m6m7p8p9p1s1s2z2z";

    fn haipai_hand() -> Vec<ChiihouPai> {
        parse_compact_chiihou_pais(HAIPAI_HAND).unwrap()
    }

    fn haipai_content(player: &str, hand: &str) -> String {
        format!(
            "{} NOTIFY haipai {player} {hand}\n\n:mahjong_m1::mahjong_m2::mahjong_m3:",
            npub_token(1)
        )
    }

    #[test]
    fn parses_haipai_with_player_and_thirteen_tiles() {
        let content = haipai_content(&npub_token(1), HAIPAI_HAND);
        assert_eq!(
            parse_chiihou_table_notification(&content).unwrap(),
            Some(ChiihouTableNotification::Haipai {
                player: player_pubkey(1),
                hand: haipai_hand(),
            })
        );
    }

    #[test]
    fn haipai_ignores_trailing_emoji_lines() {
        let content = format!(
            "{} NOTIFY haipai {} {HAIPAI_HAND}\n\n:mahjong_m1: :mahjong_m2: NOTIFYではない行",
            npub_token(1),
            npub_token(1)
        );
        assert!(matches!(
            parse_chiihou_table_notification(&content).unwrap(),
            Some(ChiihouTableNotification::Haipai { .. })
        ));
    }

    #[test]
    fn haipai_with_twelve_tiles_is_error() {
        let content = haipai_content(&npub_token(1), "1m2m3m4m5m6m7p8p9p1s1s2z");
        assert_eq!(
            parse_chiihou_table_notification(&content),
            Err(ChiihouTableNotificationError::InvalidHandTileCount(12))
        );
    }

    #[test]
    fn haipai_with_fourteen_tiles_is_error() {
        let content = haipai_content(&npub_token(1), "1m2m3m4m5m6m7p8p9p1s1s2z2z3z");
        assert_eq!(
            parse_chiihou_table_notification(&content),
            Err(ChiihouTableNotificationError::InvalidHandTileCount(14))
        );
    }

    #[test]
    fn haipai_with_invalid_npub_is_error() {
        for player in ["nostr:npub1invalid", &npub(1)] {
            let content = haipai_content(player, HAIPAI_HAND);
            assert_eq!(
                parse_chiihou_table_notification(&content),
                Err(ChiihouTableNotificationError::InvalidPublicKey),
                "player: {player:?}"
            );
        }
    }

    #[test]
    fn haipai_with_nprofile_hex_or_nsec_is_error() {
        let relay = RelayUrl::parse("wss://hint.example.com/").unwrap();
        let nprofile = Nip19Profile::new(player_pubkey(1), [relay])
            .to_bech32()
            .unwrap();
        let hex = player_pubkey(1).to_hex();
        let nsec = test_keys(1).secret_key().to_bech32().unwrap();
        for player in [nprofile, hex, nsec] {
            let content = haipai_content(&format!("nostr:{player}"), HAIPAI_HAND);
            assert_eq!(
                parse_chiihou_table_notification(&content),
                Err(ChiihouTableNotificationError::InvalidPublicKey)
            );
        }
    }

    #[test]
    fn haipai_with_invalid_hand_is_error() {
        let content = haipai_content(&npub_token(1), "1m2m3m4m5m6m7p8p9p1s1s2z0z");
        assert_eq!(
            parse_chiihou_table_notification(&content),
            Err(ChiihouTableNotificationError::InvalidHand(
                ChiihouCompactPaiParseError::InvalidPai("0z".to_string())
            ))
        );
    }

    #[test]
    fn haipai_with_odd_length_hand_is_error() {
        let content = haipai_content(&npub_token(1), "1m2m3");
        assert_eq!(
            parse_chiihou_table_notification(&content),
            Err(ChiihouTableNotificationError::InvalidHand(
                ChiihouCompactPaiParseError::OddLength(5)
            ))
        );
    }

    #[test]
    fn haipai_without_player_or_hand_is_error() {
        assert_eq!(
            parse_chiihou_table_notification("NOTIFY haipai"),
            Err(ChiihouTableNotificationError::MissingPlayer)
        );
        assert_eq!(
            parse_chiihou_table_notification(&format!("NOTIFY haipai {}", npub_token(1))),
            Err(ChiihouTableNotificationError::MissingHand)
        );
    }

    #[test]
    fn haipai_with_extra_payload_on_notify_line_is_error() {
        let content = format!(
            "{} NOTIFY haipai {} {HAIPAI_HAND} extra",
            npub_token(1),
            npub_token(1)
        );
        assert_eq!(
            parse_chiihou_table_notification(&content),
            Err(ChiihouTableNotificationError::UnexpectedPayload)
        );
    }

    #[test]
    fn parses_dora() {
        let content = format!("{} NOTIFY dora 5p", players_prefix());
        assert_eq!(
            parse_chiihou_table_notification(&content).unwrap(),
            Some(ChiihouTableNotification::Dora {
                indicator: pai("5p"),
            })
        );
    }

    #[test]
    fn dora_with_invalid_pai_is_error() {
        for token in ["0m", "8z", "５p", "east"] {
            let content = format!("{} NOTIFY dora {token}", players_prefix());
            assert_eq!(
                parse_chiihou_table_notification(&content),
                Err(ChiihouTableNotificationError::InvalidPai(token.to_string())),
                "token: {token:?}"
            );
        }
    }

    #[test]
    fn dora_without_pai_is_error() {
        let content = format!("{} NOTIFY dora", players_prefix());
        assert_eq!(
            parse_chiihou_table_notification(&content),
            Err(ChiihouTableNotificationError::MissingPai)
        );
    }

    #[test]
    fn dora_with_extra_payload_is_error() {
        let content = format!("{} NOTIFY dora 5p 6p", players_prefix());
        assert_eq!(
            parse_chiihou_table_notification(&content),
            Err(ChiihouTableNotificationError::UnexpectedPayload)
        );
    }

    fn tsumo_content(player: &str, remaining: &str, tile: &str) -> String {
        format!("{player} NOTIFY tsumo {player} {remaining} {tile}")
    }

    #[test]
    fn parses_tsumo() {
        let content = tsumo_content(&npub_token(2), "69", "7z");
        assert_eq!(
            parse_chiihou_table_notification(&content).unwrap(),
            Some(ChiihouTableNotification::Tsumo {
                player: player_pubkey(2),
                remaining_tiles: 69,
                tile: pai("7z"),
            })
        );
    }

    #[test]
    fn tsumo_with_zero_remaining_tiles_is_ok() {
        let content = tsumo_content(&npub_token(2), "0", "1m");
        assert_eq!(
            parse_chiihou_table_notification(&content).unwrap(),
            Some(ChiihouTableNotification::Tsumo {
                player: player_pubkey(2),
                remaining_tiles: 0,
                tile: pai("1m"),
            })
        );
    }

    #[test]
    fn tsumo_with_invalid_remaining_tiles_is_error() {
        for remaining in ["-1", "+1", "1.5", "x", "１"] {
            let content = tsumo_content(&npub_token(2), remaining, "1m");
            assert_eq!(
                parse_chiihou_table_notification(&content),
                Err(ChiihouTableNotificationError::InvalidRemainingTiles(
                    remaining.to_string()
                )),
                "remaining: {remaining:?}"
            );
        }
    }

    #[test]
    fn tsumo_with_invalid_npub_is_error() {
        let content = format!("NOTIFY tsumo {} 69 7z", npub(2));
        assert_eq!(
            parse_chiihou_table_notification(&content),
            Err(ChiihouTableNotificationError::InvalidPublicKey)
        );
    }

    #[test]
    fn tsumo_with_invalid_pai_is_error() {
        let content = tsumo_content(&npub_token(2), "69", "0z");
        assert_eq!(
            parse_chiihou_table_notification(&content),
            Err(ChiihouTableNotificationError::InvalidPai("0z".to_string()))
        );
    }

    #[test]
    fn tsumo_with_missing_fields_is_error() {
        assert_eq!(
            parse_chiihou_table_notification("NOTIFY tsumo"),
            Err(ChiihouTableNotificationError::MissingPlayer)
        );
        assert_eq!(
            parse_chiihou_table_notification(&format!("NOTIFY tsumo {}", npub_token(2))),
            Err(ChiihouTableNotificationError::MissingRemainingTiles)
        );
        assert_eq!(
            parse_chiihou_table_notification(&format!("NOTIFY tsumo {} 69", npub_token(2))),
            Err(ChiihouTableNotificationError::MissingPai)
        );
    }

    #[test]
    fn tsumo_with_extra_payload_is_error() {
        let content = format!("{} extra", tsumo_content(&npub_token(2), "69", "7z"));
        assert_eq!(
            parse_chiihou_table_notification(&content),
            Err(ChiihouTableNotificationError::UnexpectedPayload)
        );
    }

    #[test]
    fn parses_sutehai() {
        let content = format!("{} NOTIFY sutehai {} 7z", players_prefix(), npub_token(3));
        assert_eq!(
            parse_chiihou_table_notification(&content).unwrap(),
            Some(ChiihouTableNotification::Sutehai {
                player: player_pubkey(3),
                tile: pai("7z"),
            })
        );
    }

    #[test]
    fn sutehai_with_invalid_npub_is_error() {
        let content = format!("{} NOTIFY sutehai {} 7z", players_prefix(), npub(3));
        assert_eq!(
            parse_chiihou_table_notification(&content),
            Err(ChiihouTableNotificationError::InvalidPublicKey)
        );
    }

    #[test]
    fn sutehai_with_invalid_pai_is_error() {
        let content = format!("{} NOTIFY sutehai {} 9z", players_prefix(), npub_token(3));
        assert_eq!(
            parse_chiihou_table_notification(&content),
            Err(ChiihouTableNotificationError::InvalidPai("9z".to_string()))
        );
    }

    #[test]
    fn sutehai_with_extra_payload_is_error() {
        let content = format!(
            "{} NOTIFY sutehai {} 7z 8p",
            players_prefix(),
            npub_token(3)
        );
        assert_eq!(
            parse_chiihou_table_notification(&content),
            Err(ChiihouTableNotificationError::UnexpectedPayload)
        );
    }

    fn say_content(player: &str, action: &str) -> String {
        format!("{} NOTIFY say {player} {action}", players_prefix())
    }

    #[test]
    fn parses_say_richi() {
        let content = say_content(&npub_token(2), "richi");
        assert_eq!(
            parse_chiihou_table_notification(&content).unwrap(),
            Some(ChiihouTableNotification::Say {
                player: player_pubkey(2),
                action: ChiihouSayAction::Richi,
            })
        );
    }

    #[test]
    fn parses_all_say_actions() {
        for (token, action) in [
            ("tsumo", ChiihouSayAction::Tsumo),
            ("ron", ChiihouSayAction::Ron),
            ("pon", ChiihouSayAction::Pon),
            ("chi", ChiihouSayAction::Chi),
            ("kan", ChiihouSayAction::Kan),
            ("tenpai", ChiihouSayAction::Tenpai),
            ("noten", ChiihouSayAction::Noten),
        ] {
            let content = say_content(&npub_token(4), token);
            assert_eq!(
                parse_chiihou_table_notification(&content).unwrap(),
                Some(ChiihouTableNotification::Say {
                    player: player_pubkey(4),
                    action,
                }),
                "action: {token}"
            );
        }
    }

    #[test]
    fn say_with_unknown_action_is_error() {
        for token in ["riichi", "kakan", "RICHI", ""] {
            let content = say_content(&npub_token(2), token);
            let expected = if token.is_empty() {
                ChiihouTableNotificationError::MissingAction
            } else {
                ChiihouTableNotificationError::UnknownAction(token.to_string())
            };
            assert_eq!(
                parse_chiihou_table_notification(&content),
                Err(expected),
                "action: {token:?}"
            );
        }
    }

    #[test]
    fn say_with_invalid_npub_is_error() {
        let content = say_content(&npub(2), "richi");
        assert_eq!(
            parse_chiihou_table_notification(&content),
            Err(ChiihouTableNotificationError::InvalidPublicKey)
        );
    }

    #[test]
    fn say_with_extra_payload_is_error() {
        let content = format!("{} now", say_content(&npub_token(2), "richi"));
        assert_eq!(
            parse_chiihou_table_notification(&content),
            Err(ChiihouTableNotificationError::UnexpectedPayload)
        );
    }

    fn open_content(player: &str, tiles: &str) -> String {
        format!("{} NOTIFY open {player} {tiles}", players_prefix())
    }

    #[test]
    fn parses_open_pon() {
        let content = open_content(&npub_token(2), "5z5z5z");
        assert_eq!(
            parse_chiihou_table_notification(&content).unwrap(),
            Some(ChiihouTableNotification::Open {
                player: player_pubkey(2),
                tiles: vec![pai("5z"); 3],
            })
        );
    }

    #[test]
    fn parses_open_chi() {
        let content = open_content(&npub_token(3), "1m2m3m");
        assert_eq!(
            parse_chiihou_table_notification(&content).unwrap(),
            Some(ChiihouTableNotification::Open {
                player: player_pubkey(3),
                tiles: pais(&["1m", "2m", "3m"]),
            })
        );
    }

    #[test]
    fn parses_open_kan() {
        let content = open_content(&npub_token(4), "5z5z5z5z");
        assert_eq!(
            parse_chiihou_table_notification(&content).unwrap(),
            Some(ChiihouTableNotification::Open {
                player: player_pubkey(4),
                tiles: vec![pai("5z"); 4],
            })
        );
    }

    #[test]
    fn parses_open_kakan() {
        let content = open_content(&npub_token(2), "5z");
        assert_eq!(
            parse_chiihou_table_notification(&content).unwrap(),
            Some(ChiihouTableNotification::Open {
                player: player_pubkey(2),
                tiles: vec![pai("5z")],
            })
        );
    }

    #[test]
    fn open_without_player_or_tiles_is_error() {
        assert_eq!(
            parse_chiihou_table_notification("NOTIFY open"),
            Err(ChiihouTableNotificationError::MissingPlayer)
        );
        assert_eq!(
            parse_chiihou_table_notification(&format!("NOTIFY open {}", npub_token(2))),
            Err(ChiihouTableNotificationError::MissingOpenTiles)
        );
    }

    #[test]
    fn open_with_invalid_npub_is_error() {
        let content = open_content(&npub(2), "5z5z5z");
        assert_eq!(
            parse_chiihou_table_notification(&content),
            Err(ChiihouTableNotificationError::InvalidPublicKey)
        );
    }

    #[test]
    fn open_with_invalid_tiles_is_error() {
        for (tiles, expected) in [
            (
                "5z5z0z",
                ChiihouCompactPaiParseError::InvalidPai("0z".to_string()),
            ),
            ("1m2m3", ChiihouCompactPaiParseError::OddLength(5)),
            ("１m2m", ChiihouCompactPaiParseError::NotAscii),
        ] {
            let content = open_content(&npub_token(2), tiles);
            assert_eq!(
                parse_chiihou_table_notification(&content),
                Err(ChiihouTableNotificationError::InvalidOpenTiles(expected)),
                "tiles: {tiles:?}"
            );
        }
    }

    #[test]
    fn open_with_extra_payload_is_error() {
        let content = format!("{} 4m", open_content(&npub_token(2), "1m2m3m"));
        assert_eq!(
            parse_chiihou_table_notification(&content),
            Err(ChiihouTableNotificationError::UnexpectedPayload)
        );
    }

    #[test]
    fn get_requests_are_none() {
        let sutehai = format!(
            ":mahjong_m1::mahjong_m2::mahjong_m3: :mahjong_east:\n{} GET sutehai?",
            npub_token(1)
        );
        let naku = format!(
            ":mahjong_m1::mahjong_m2::mahjong_m3: :mahjong_m4:\n{} GET naku? ron",
            npub_token(1)
        );
        assert_eq!(parse_chiihou_table_notification(&sutehai).unwrap(), None);
        assert_eq!(parse_chiihou_table_notification(&naku).unwrap(), None);
    }

    #[test]
    fn unsupported_notify_commands_are_none() {
        for command in [
            "point",
            "agari",
            "ryukyoku",
            "kyokuend",
            "gameend",
            "gamestart",
            "kyokustart",
        ] {
            let content = format!("{} NOTIFY {command} payload", players_prefix());
            assert_eq!(
                parse_chiihou_table_notification(&content).unwrap(),
                None,
                "command: {command}"
            );
        }
    }

    #[test]
    fn content_without_notify_is_none() {
        for content in ["", "こんにちは", "haipai", "nostr:npub1ai000 join"] {
            assert_eq!(
                parse_chiihou_table_notification(content).unwrap(),
                None,
                "content: {content:?}"
            );
        }
    }

    #[test]
    fn notify_without_command_is_none() {
        let content = format!("{} NOTIFY", players_prefix());
        assert_eq!(parse_chiihou_table_notification(&content).unwrap(), None);
    }
}
