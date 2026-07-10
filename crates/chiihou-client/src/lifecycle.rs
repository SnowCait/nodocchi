use std::fmt;
use std::str::FromStr;

use nostr_sdk::{FromBech32, PublicKey};
use thiserror::Error;

pub(crate) const CHIIHOU_PLAYER_COUNT: usize = 4;

#[derive(Debug, Error, Clone, PartialEq, Eq)]
#[error("invalid chiihou wind: {0:?}")]
pub struct ChiihouWindParseError(String);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ChiihouWind {
    East,
    South,
    West,
    North,
}

impl FromStr for ChiihouWind {
    type Err = ChiihouWindParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "東" => Ok(Self::East),
            "南" => Ok(Self::South),
            "西" => Ok(Self::West),
            "北" => Ok(Self::North),
            _ => Err(ChiihouWindParseError(s.to_string())),
        }
    }
}

impl fmt::Display for ChiihouWind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            Self::East => "東",
            Self::South => "南",
            Self::West => "西",
            Self::North => "北",
        };
        write!(f, "{s}")
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChiihouPlayerScore {
    pub player: PublicKey,
    pub score: i32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChiihouLifecycleNotification {
    GameStart {
        seat: ChiihouWind,
        players: Vec<PublicKey>,
    },
    KyokuStart {
        round_wind: ChiihouWind,
        dealer: PublicKey,
        honba: u32,
        kyotaku_points: u32,
    },
    KyokuEnd,
    GameEnd {
        scores: Vec<ChiihouPlayerScore>,
    },
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum ChiihouLifecycleError {
    #[error("missing seat in gamestart")]
    MissingSeat,
    #[error("invalid seat: {0:?}")]
    InvalidSeat(String),
    #[error("invalid player count: {0}")]
    InvalidPlayerCount(usize),
    #[error("duplicate player public key")]
    DuplicatePlayer,
    #[error("invalid player public key")]
    InvalidPublicKey,
    #[error("missing round wind in kyokustart")]
    MissingRoundWind,
    #[error("invalid round wind: {0:?}")]
    InvalidRoundWind(String),
    #[error("missing dealer in kyokustart")]
    MissingDealer,
    #[error("missing honba in kyokustart")]
    MissingHonba,
    #[error("invalid honba: {0:?}")]
    InvalidHonba(String),
    #[error("missing kyotaku points in kyokustart")]
    MissingKyotakuPoints,
    #[error("invalid kyotaku points: {0:?}")]
    InvalidKyotakuPoints(String),
    #[error("invalid score pair count in gameend: {0}")]
    InvalidScorePairCount(usize),
    #[error("missing score in gameend")]
    MissingScore,
    #[error("invalid score: {0:?}")]
    InvalidScore(String),
    #[error("unexpected payload after command")]
    UnexpectedPayload,
}

pub fn parse_chiihou_lifecycle_notification(
    content: &str,
) -> Result<Option<ChiihouLifecycleNotification>, ChiihouLifecycleError> {
    let mut tokens = content
        .split_whitespace()
        .skip_while(|token| *token != "NOTIFY")
        .skip(1);
    let Some(command) = tokens.next() else {
        return Ok(None);
    };
    match command {
        "gamestart" => parse_gamestart(tokens).map(Some),
        "kyokustart" => parse_kyokustart(tokens).map(Some),
        "kyokuend" => parse_kyokuend(tokens).map(Some),
        "gameend" => parse_gameend(tokens).map(Some),
        _ => Ok(None),
    }
}

fn parse_gamestart<'a>(
    mut tokens: impl Iterator<Item = &'a str>,
) -> Result<ChiihouLifecycleNotification, ChiihouLifecycleError> {
    let Some(seat_token) = tokens.next() else {
        return Err(ChiihouLifecycleError::MissingSeat);
    };
    let seat = seat_token
        .parse()
        .map_err(|_| ChiihouLifecycleError::InvalidSeat(seat_token.to_string()))?;
    let players = tokens
        .map(parse_player_pubkey)
        .collect::<Result<Vec<_>, _>>()?;
    if players.len() != CHIIHOU_PLAYER_COUNT {
        return Err(ChiihouLifecycleError::InvalidPlayerCount(players.len()));
    }
    ensure_unique_players(&players)?;
    Ok(ChiihouLifecycleNotification::GameStart { seat, players })
}

fn parse_kyokustart<'a>(
    mut tokens: impl Iterator<Item = &'a str>,
) -> Result<ChiihouLifecycleNotification, ChiihouLifecycleError> {
    let Some(wind_token) = tokens.next() else {
        return Err(ChiihouLifecycleError::MissingRoundWind);
    };
    let round_wind = wind_token
        .parse()
        .map_err(|_| ChiihouLifecycleError::InvalidRoundWind(wind_token.to_string()))?;
    let Some(dealer_token) = tokens.next() else {
        return Err(ChiihouLifecycleError::MissingDealer);
    };
    let dealer = parse_player_pubkey(dealer_token)?;
    let Some(honba_token) = tokens.next() else {
        return Err(ChiihouLifecycleError::MissingHonba);
    };
    let honba = parse_u32(honba_token)
        .ok_or_else(|| ChiihouLifecycleError::InvalidHonba(honba_token.to_string()))?;
    let Some(kyotaku_token) = tokens.next() else {
        return Err(ChiihouLifecycleError::MissingKyotakuPoints);
    };
    let kyotaku_points = parse_u32(kyotaku_token)
        .ok_or_else(|| ChiihouLifecycleError::InvalidKyotakuPoints(kyotaku_token.to_string()))?;
    ensure_no_remaining_tokens(tokens)?;
    Ok(ChiihouLifecycleNotification::KyokuStart {
        round_wind,
        dealer,
        honba,
        kyotaku_points,
    })
}

fn parse_kyokuend<'a>(
    tokens: impl Iterator<Item = &'a str>,
) -> Result<ChiihouLifecycleNotification, ChiihouLifecycleError> {
    ensure_no_remaining_tokens(tokens)?;
    Ok(ChiihouLifecycleNotification::KyokuEnd)
}

fn parse_gameend<'a>(
    mut tokens: impl Iterator<Item = &'a str>,
) -> Result<ChiihouLifecycleNotification, ChiihouLifecycleError> {
    let mut scores = Vec::new();
    while let Some(player_token) = tokens.next() {
        let player = parse_player_pubkey(player_token)?;
        let Some(score_token) = tokens.next() else {
            return Err(ChiihouLifecycleError::MissingScore);
        };
        let score = parse_score(score_token)
            .ok_or_else(|| ChiihouLifecycleError::InvalidScore(score_token.to_string()))?;
        scores.push(ChiihouPlayerScore { player, score });
    }
    if scores.len() != CHIIHOU_PLAYER_COUNT {
        return Err(ChiihouLifecycleError::InvalidScorePairCount(scores.len()));
    }
    let players: Vec<PublicKey> = scores.iter().map(|score| score.player).collect();
    ensure_unique_players(&players)?;
    Ok(ChiihouLifecycleNotification::GameEnd { scores })
}

fn parse_player_pubkey(token: &str) -> Result<PublicKey, ChiihouLifecycleError> {
    player_pubkey_from_token(token).ok_or(ChiihouLifecycleError::InvalidPublicKey)
}

pub(crate) fn player_pubkey_from_token(token: &str) -> Option<PublicKey> {
    token
        .strip_prefix("nostr:")
        .and_then(|npub| PublicKey::from_bech32(npub).ok())
}

fn ensure_unique_players(players: &[PublicKey]) -> Result<(), ChiihouLifecycleError> {
    for (i, player) in players.iter().enumerate() {
        if players[..i].contains(player) {
            return Err(ChiihouLifecycleError::DuplicatePlayer);
        }
    }
    Ok(())
}

fn ensure_no_remaining_tokens<'a>(
    mut tokens: impl Iterator<Item = &'a str>,
) -> Result<(), ChiihouLifecycleError> {
    if tokens.next().is_some() {
        return Err(ChiihouLifecycleError::UnexpectedPayload);
    }
    Ok(())
}

pub(crate) fn parse_u32(token: &str) -> Option<u32> {
    if !token.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    token.parse().ok()
}

fn parse_score(token: &str) -> Option<i32> {
    let digits = token.strip_prefix('-').unwrap_or(token);
    if digits.is_empty() || !digits.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    token.parse().ok()
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

    fn gamestart_content(seat: &str) -> String {
        format!(
            "{} #gamestart NOTIFY gamestart {seat} {}",
            npub_token(1),
            players_prefix()
        )
    }

    fn kyokustart_content(round_wind: &str, dealer: &str, honba: &str, kyotaku: &str) -> String {
        format!(
            "{} #kyokustart NOTIFY kyokustart {round_wind} {dealer} {honba} {kyotaku}",
            players_prefix()
        )
    }

    fn gameend_content(pairs: &str) -> String {
        format!("{} NOTIFY gameend {pairs}", players_prefix())
    }

    fn gameend_pairs(scores: &[i32]) -> String {
        scores
            .iter()
            .enumerate()
            .map(|(i, score)| format!("{} {score}", npub_token(i as u64 + 1)))
            .collect::<Vec<_>>()
            .join(" ")
    }

    #[test]
    fn parses_gamestart() {
        assert_eq!(
            parse_chiihou_lifecycle_notification(&gamestart_content("南")).unwrap(),
            Some(ChiihouLifecycleNotification::GameStart {
                seat: ChiihouWind::South,
                players: (1..=4).map(player_pubkey).collect(),
            })
        );
    }

    #[test]
    fn parses_all_seats() {
        for (seat, wind) in [
            ("東", ChiihouWind::East),
            ("南", ChiihouWind::South),
            ("西", ChiihouWind::West),
            ("北", ChiihouWind::North),
        ] {
            let parsed = parse_chiihou_lifecycle_notification(&gamestart_content(seat)).unwrap();
            assert!(
                matches!(
                    parsed,
                    Some(ChiihouLifecycleNotification::GameStart { seat, .. }) if seat == wind
                ),
                "seat: {seat}"
            );
        }
    }

    #[test]
    fn gamestart_keeps_four_player_pubkeys_in_order() {
        let parsed = parse_chiihou_lifecycle_notification(&gamestart_content("東")).unwrap();
        let Some(ChiihouLifecycleNotification::GameStart { players, .. }) = parsed else {
            panic!("expected gamestart, got: {parsed:?}");
        };
        assert_eq!(players, (1..=4).map(player_pubkey).collect::<Vec<_>>());
    }

    #[test]
    fn parses_kyokustart() {
        let content = kyokustart_content("南", &npub_token(2), "1", "2000");
        assert_eq!(
            parse_chiihou_lifecycle_notification(&content).unwrap(),
            Some(ChiihouLifecycleNotification::KyokuStart {
                round_wind: ChiihouWind::South,
                dealer: player_pubkey(2),
                honba: 1,
                kyotaku_points: 2000,
            })
        );
    }

    #[test]
    fn parses_all_round_winds() {
        for (token, wind) in [
            ("東", ChiihouWind::East),
            ("南", ChiihouWind::South),
            ("西", ChiihouWind::West),
            ("北", ChiihouWind::North),
        ] {
            let content = kyokustart_content(token, &npub_token(1), "0", "0");
            let parsed = parse_chiihou_lifecycle_notification(&content).unwrap();
            assert!(
                matches!(
                    parsed,
                    Some(ChiihouLifecycleNotification::KyokuStart { round_wind, .. })
                        if round_wind == wind
                ),
                "round wind: {token}"
            );
        }
    }

    #[test]
    fn parses_kyokuend() {
        let content = format!("{} NOTIFY kyokuend", players_prefix());
        assert_eq!(
            parse_chiihou_lifecycle_notification(&content).unwrap(),
            Some(ChiihouLifecycleNotification::KyokuEnd)
        );
    }

    #[test]
    fn parses_gameend_with_signed_scores() {
        let content = gameend_content(&gameend_pairs(&[45000, 30000, 26000, -1000]));
        assert_eq!(
            parse_chiihou_lifecycle_notification(&content).unwrap(),
            Some(ChiihouLifecycleNotification::GameEnd {
                scores: vec![
                    ChiihouPlayerScore {
                        player: player_pubkey(1),
                        score: 45000,
                    },
                    ChiihouPlayerScore {
                        player: player_pubkey(2),
                        score: 30000,
                    },
                    ChiihouPlayerScore {
                        player: player_pubkey(3),
                        score: 26000,
                    },
                    ChiihouPlayerScore {
                        player: player_pubkey(4),
                        score: -1000,
                    },
                ],
            })
        );
    }

    #[test]
    fn content_without_notify_is_none() {
        for content in ["", "こんにちは", "gamestart", "nostr:npub1ai000 join"] {
            assert_eq!(
                parse_chiihou_lifecycle_notification(content).unwrap(),
                None,
                "content: {content:?}"
            );
        }
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
        assert_eq!(
            parse_chiihou_lifecycle_notification(&sutehai).unwrap(),
            None
        );
        assert_eq!(parse_chiihou_lifecycle_notification(&naku).unwrap(), None);
    }

    #[test]
    fn unsupported_notify_commands_are_none() {
        for command in [
            "point", "haipai", "dora", "tsumo", "sutehai", "say", "open", "agari", "ryukyoku",
        ] {
            let content = format!("{} NOTIFY {command} payload", players_prefix());
            assert_eq!(
                parse_chiihou_lifecycle_notification(&content).unwrap(),
                None,
                "command: {command}"
            );
        }
    }

    #[test]
    fn notify_without_command_is_none() {
        let content = format!("{} NOTIFY", players_prefix());
        assert_eq!(
            parse_chiihou_lifecycle_notification(&content).unwrap(),
            None
        );
    }

    #[test]
    fn gamestart_with_invalid_seat_is_error() {
        assert_eq!(
            parse_chiihou_lifecycle_notification(&gamestart_content("X")),
            Err(ChiihouLifecycleError::InvalidSeat("X".to_string()))
        );
    }

    #[test]
    fn gamestart_without_seat_is_error() {
        assert_eq!(
            parse_chiihou_lifecycle_notification("NOTIFY gamestart"),
            Err(ChiihouLifecycleError::MissingSeat)
        );
    }

    #[test]
    fn gamestart_with_missing_players_is_error() {
        let content = format!(
            "NOTIFY gamestart 東 {} {} {}",
            npub_token(1),
            npub_token(2),
            npub_token(3)
        );
        assert_eq!(
            parse_chiihou_lifecycle_notification(&content),
            Err(ChiihouLifecycleError::InvalidPlayerCount(3))
        );
    }

    #[test]
    fn gamestart_with_five_players_is_error() {
        let content = format!("{} {}", gamestart_content("東"), npub_token(5));
        assert_eq!(
            parse_chiihou_lifecycle_notification(&content),
            Err(ChiihouLifecycleError::InvalidPlayerCount(5))
        );
    }

    #[test]
    fn gamestart_with_duplicate_players_is_error() {
        let content = format!(
            "NOTIFY gamestart 東 {} {} {} {}",
            npub_token(1),
            npub_token(2),
            npub_token(2),
            npub_token(3)
        );
        assert_eq!(
            parse_chiihou_lifecycle_notification(&content),
            Err(ChiihouLifecycleError::DuplicatePlayer)
        );
    }

    #[test]
    fn gamestart_with_invalid_npub_is_error() {
        let content = format!(
            "NOTIFY gamestart 東 nostr:npub1invalid {} {} {}",
            npub_token(2),
            npub_token(3),
            npub_token(4)
        );
        assert_eq!(
            parse_chiihou_lifecycle_notification(&content),
            Err(ChiihouLifecycleError::InvalidPublicKey)
        );
    }

    #[test]
    fn player_without_nostr_prefix_is_error() {
        let content = format!(
            "NOTIFY gamestart 東 {} {} {} {}",
            npub(1),
            npub_token(2),
            npub_token(3),
            npub_token(4)
        );
        assert_eq!(
            parse_chiihou_lifecycle_notification(&content),
            Err(ChiihouLifecycleError::InvalidPublicKey)
        );
    }

    #[test]
    fn nprofile_player_is_error() {
        let relay = RelayUrl::parse("wss://hint.example.com/").unwrap();
        let nprofile = Nip19Profile::new(player_pubkey(1), [relay])
            .to_bech32()
            .unwrap();
        let content = format!(
            "NOTIFY gamestart 東 nostr:{nprofile} {} {} {}",
            npub_token(2),
            npub_token(3),
            npub_token(4)
        );
        assert_eq!(
            parse_chiihou_lifecycle_notification(&content),
            Err(ChiihouLifecycleError::InvalidPublicKey)
        );
    }

    #[test]
    fn hex_player_is_error() {
        let content = format!(
            "NOTIFY gamestart 東 nostr:{} {} {} {}",
            player_pubkey(1).to_hex(),
            npub_token(2),
            npub_token(3),
            npub_token(4)
        );
        assert_eq!(
            parse_chiihou_lifecycle_notification(&content),
            Err(ChiihouLifecycleError::InvalidPublicKey)
        );
    }

    #[test]
    fn nsec_player_is_error() {
        let nsec = test_keys(1).secret_key().to_bech32().unwrap();
        let content = format!(
            "NOTIFY gamestart 東 nostr:{nsec} {} {} {}",
            npub_token(2),
            npub_token(3),
            npub_token(4)
        );
        assert_eq!(
            parse_chiihou_lifecycle_notification(&content),
            Err(ChiihouLifecycleError::InvalidPublicKey)
        );
    }

    #[test]
    fn kyokustart_with_invalid_round_wind_is_error() {
        let content = kyokustart_content("X", &npub_token(1), "0", "0");
        assert_eq!(
            parse_chiihou_lifecycle_notification(&content),
            Err(ChiihouLifecycleError::InvalidRoundWind("X".to_string()))
        );
    }

    #[test]
    fn kyokustart_with_invalid_dealer_is_error() {
        let content = kyokustart_content("東", &npub(1), "0", "0");
        assert_eq!(
            parse_chiihou_lifecycle_notification(&content),
            Err(ChiihouLifecycleError::InvalidPublicKey)
        );
    }

    #[test]
    fn kyokustart_with_missing_fields_is_error() {
        for (content, expected) in [
            (
                "NOTIFY kyokustart".to_string(),
                ChiihouLifecycleError::MissingRoundWind,
            ),
            (
                "NOTIFY kyokustart 東".to_string(),
                ChiihouLifecycleError::MissingDealer,
            ),
            (
                format!("NOTIFY kyokustart 東 {}", npub_token(1)),
                ChiihouLifecycleError::MissingHonba,
            ),
            (
                format!("NOTIFY kyokustart 東 {} 0", npub_token(1)),
                ChiihouLifecycleError::MissingKyotakuPoints,
            ),
        ] {
            assert_eq!(
                parse_chiihou_lifecycle_notification(&content),
                Err(expected),
                "content: {content:?}"
            );
        }
    }

    #[test]
    fn kyokustart_with_invalid_honba_is_error() {
        for honba in ["x", "-1", "+1", "1.5"] {
            let content = kyokustart_content("東", &npub_token(1), honba, "0");
            assert_eq!(
                parse_chiihou_lifecycle_notification(&content),
                Err(ChiihouLifecycleError::InvalidHonba(honba.to_string())),
                "honba: {honba:?}"
            );
        }
    }

    #[test]
    fn kyokustart_with_invalid_kyotaku_points_is_error() {
        for kyotaku in ["x", "-1000", "+1000", "0.5"] {
            let content = kyokustart_content("東", &npub_token(1), "0", kyotaku);
            assert_eq!(
                parse_chiihou_lifecycle_notification(&content),
                Err(ChiihouLifecycleError::InvalidKyotakuPoints(
                    kyotaku.to_string()
                )),
                "kyotaku: {kyotaku:?}"
            );
        }
    }

    #[test]
    fn gameend_with_missing_pair_is_error() {
        let content = gameend_content(&gameend_pairs(&[45000, 30000, 26000]));
        assert_eq!(
            parse_chiihou_lifecycle_notification(&content),
            Err(ChiihouLifecycleError::InvalidScorePairCount(3))
        );
    }

    #[test]
    fn gameend_with_extra_pair_is_error() {
        let content = gameend_content(&gameend_pairs(&[45000, 30000, 26000, -1000, 0]));
        assert_eq!(
            parse_chiihou_lifecycle_notification(&content),
            Err(ChiihouLifecycleError::InvalidScorePairCount(5))
        );
    }

    #[test]
    fn gameend_with_duplicate_player_is_error() {
        let pairs = format!(
            "{} 45000 {} 30000 {} 26000 {} -1000",
            npub_token(1),
            npub_token(2),
            npub_token(2),
            npub_token(3)
        );
        assert_eq!(
            parse_chiihou_lifecycle_notification(&gameend_content(&pairs)),
            Err(ChiihouLifecycleError::DuplicatePlayer)
        );
    }

    #[test]
    fn gameend_with_invalid_score_is_error() {
        for score in ["not-a-score", "+100", "25000.5"] {
            let pairs = format!(
                "{} {score} {} 30000 {} 26000 {} -1000",
                npub_token(1),
                npub_token(2),
                npub_token(3),
                npub_token(4)
            );
            assert_eq!(
                parse_chiihou_lifecycle_notification(&gameend_content(&pairs)),
                Err(ChiihouLifecycleError::InvalidScore(score.to_string())),
                "score: {score:?}"
            );
        }
    }

    #[test]
    fn gameend_with_dangling_player_is_error() {
        let pairs = format!(
            "{} {}",
            gameend_pairs(&[45000, 30000, 26000]),
            npub_token(4)
        );
        assert_eq!(
            parse_chiihou_lifecycle_notification(&gameend_content(&pairs)),
            Err(ChiihouLifecycleError::MissingScore)
        );
    }

    #[test]
    fn kyokuend_with_extra_payload_is_error() {
        let content = format!("{} NOTIFY kyokuend extra", players_prefix());
        assert_eq!(
            parse_chiihou_lifecycle_notification(&content),
            Err(ChiihouLifecycleError::UnexpectedPayload)
        );
    }

    #[test]
    fn kyokustart_with_extra_payload_is_error() {
        let content = format!(
            "{} extra",
            kyokustart_content("東", &npub_token(1), "0", "0")
        );
        assert_eq!(
            parse_chiihou_lifecycle_notification(&content),
            Err(ChiihouLifecycleError::UnexpectedPayload)
        );
    }

    #[test]
    fn wind_parse_roundtrips() {
        for s in ["東", "南", "西", "北"] {
            let wind: ChiihouWind = s.parse().unwrap();
            assert_eq!(wind.to_string(), s);
        }
    }

    #[test]
    fn wind_parse_rejects_unknown_strings() {
        for s in ["", "北北", "east", "1z", "e"] {
            assert_eq!(
                s.parse::<ChiihouWind>(),
                Err(ChiihouWindParseError(s.to_string())),
                "input: {s:?}"
            );
        }
    }
}
