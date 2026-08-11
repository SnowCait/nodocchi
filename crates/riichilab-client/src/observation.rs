use bot_core::{GameContext, Meld, MeldKind};
use bot_logic::{TileId, TileType};
use riichienv_core::observation::Observation;
use riichienv_core::types::{Meld as ObservationMeld, MeldType as ObservationMeldType};

use crate::convert::temporary_tile_id_from_observation_tile;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObservationPayload {
    base64: String,
}

impl ObservationPayload {
    pub fn new(base64: impl Into<String>) -> Self {
        Self {
            base64: base64.into(),
        }
    }

    pub fn as_base64(&self) -> &str {
        &self.base64
    }

    pub fn decode_4p(&self) -> Result<DecodedObservation, ObservationError> {
        let observation = Observation::deserialize_from_base64(&self.base64)
            .map_err(|e| ObservationError::Decode(e.to_string()))?;

        // RiichiEnv (riichienv-core 0.4.8) の Observation.hands はツモ牌込みであり、
        // 同じ物理牌 ID が drawn_tile にも保持される。raw 物理牌 ID のまま取得し、
        // hand_tiles 用と visible_tiles 用で別々に扱う。
        let raw_hand: Vec<u32> = observation
            .hands
            .get(usize::from(observation.player_id))
            .cloned()
            .unwrap_or_default();
        let raw_drawn_tile = observation.drawn_tile;

        // ツモ牌を raw 物理牌 ID で 1 枚だけ分離する。赤5と通常5、同牌種の別個体を
        // 区別するため、temporary 変換前の raw ID で比較する。
        let mut concealed_hand_raw = raw_hand.clone();
        let removed_drawn_tile = remove_drawn_tile_once(&mut concealed_hand_raw, raw_drawn_tile);

        let hand_tiles = decode_observation_tiles(&concealed_hand_raw);
        let drawn_tile = raw_drawn_tile.and_then(temporary_tile_id_from_observation_tile);
        let dora_indicators = decode_observation_tiles(&observation.dora_indicators);
        // discards は防御・現物判定用の player ごとの河、visible_tiles は枚数補正用。用途を分ける。
        let discards = decode_discards(&observation);
        let melds = decode_melds(&observation);

        // visible_tiles には自分から見えている現在の手牌全体をツモ牌込みで 1 回だけ含める。
        // 上流形式では raw_hand がツモ牌を含むため drawn_tile を足さない。ツモ牌が hand に
        // 含まれない互換形式（分離できなかった場合）のみ drawn_tile を 1 枚足す。
        let mut visible_tiles = decode_observation_tiles(&raw_hand);
        if !removed_drawn_tile && let Some(drawn_tile) = drawn_tile {
            visible_tiles.push(drawn_tile);
        }
        visible_tiles.extend(dora_indicators.iter().copied());
        visible_tiles.extend(discards.iter().flatten().copied());
        for player_melds in &melds {
            for meld in player_melds {
                visible_tiles.extend(meld_visible_tiles(meld));
            }
        }

        Ok(DecodedObservation {
            player_id: observation.player_id,
            drawn_tile,
            hand_tiles,
            dora_indicators,
            round_wind: TileType::wind_from_seat_index(observation.round_wind),
            seat_wind: seat_wind_from(observation.player_id, observation.oya),
            visible_tiles,
            oya: observation.oya,
            discards,
            reached: observation.riichi_declared,
            melds,
        })
    }
}

// raw 物理牌 ID 列を temporary TileId へ変換する。不正な牌 ID は従来どおり除外する。
fn decode_observation_tiles(raw_tiles: &[u32]) -> Vec<TileId> {
    raw_tiles
        .iter()
        .filter_map(|&raw| u8::try_from(raw).ok())
        .filter_map(temporary_tile_id_from_observation_tile)
        .collect()
}

// ツモ牌と一致する raw 物理牌 ID を hand から 1 枚だけ除去する。
//
// - drawn_tile == None: hand を変更せず false
// - 同じ raw 物理牌 ID が hand にある: 最初の 1 枚だけ除去し true
// - 同じ raw 物理牌 ID が hand にない: hand を変更せず false
//
// 赤5と通常5、同牌種の別個体を区別するため、temporary 変換前の raw ID で比較する。
fn remove_drawn_tile_once(hand: &mut Vec<u32>, drawn_tile: Option<u8>) -> bool {
    let Some(drawn_tile) = drawn_tile.map(u32::from) else {
        return false;
    };
    let Some(index) = hand.iter().position(|&tile| tile == drawn_tile) else {
        return false;
    };
    hand.remove(index);
    true
}

fn decode_discards(observation: &Observation) -> [Vec<TileId>; 4] {
    let mut discards: [Vec<TileId>; 4] = Default::default();
    for (player, player_discards) in observation.discards.iter().enumerate() {
        discards[player] = decode_observation_tiles(player_discards);
    }
    discards
}

fn decode_melds(observation: &Observation) -> [Vec<Meld>; 4] {
    let mut melds: [Vec<Meld>; 4] = Default::default();
    for (player, player_melds) in observation.melds.iter().enumerate() {
        melds[player] = player_melds.iter().map(decode_meld).collect();
    }
    melds
}

fn decode_meld(meld: &ObservationMeld) -> Meld {
    Meld::new(
        meld_kind_from_observation_meld_type(meld.meld_type),
        decode_observation_tiles(&meld.tiles_as_u32()),
        meld.called_tile
            .and_then(temporary_tile_id_from_observation_tile),
    )
}

fn meld_kind_from_observation_meld_type(meld_type: ObservationMeldType) -> MeldKind {
    match meld_type {
        ObservationMeldType::Chi => MeldKind::Chi,
        ObservationMeldType::Pon => MeldKind::Pon,
        ObservationMeldType::Daiminkan => MeldKind::Daiminkan,
        ObservationMeldType::Ankan => MeldKind::Ankan,
        ObservationMeldType::Kakan => MeldKind::Kakan,
    }
}

fn seat_wind_from(player_id: u8, oya: u8) -> Option<TileType> {
    if player_id >= 4 || oya >= 4 {
        return None;
    }

    let index = (player_id + 4 - oya) % 4;
    TileType::wind_from_seat_index(index)
}

// called tile は河に残っており二重計上になるため、consumed 牌だけを見えている牌として扱う。
// ankan は called tile を持たないため 4 枚すべて、kakan は元 pon の called tile を除いた牌を数える。
fn meld_visible_tiles(meld: &Meld) -> Vec<TileId> {
    let mut called_tile = meld.called_tile();
    let mut tiles = Vec::new();
    for &tile in meld.tiles() {
        if called_tile == Some(tile) {
            called_tile = None;
            continue;
        }
        tiles.push(tile);
    }
    tiles
}

#[derive(Debug, thiserror::Error)]
pub enum ObservationError {
    #[error("failed to decode observation: {0}")]
    Decode(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecodedObservation {
    pub player_id: u8,
    pub drawn_tile: Option<TileId>,
    /// RiichiEnv の Observation.hands はツモ牌込みである。
    /// hand_tiles は drawn_tile を raw 物理牌 ID で 1 枚分離した状態で保持する。
    /// hand_tiles + drawn_tile が実際の自摸後手牌になる。
    pub hand_tiles: Vec<TileId>,
    pub dora_indicators: Vec<TileId>,
    pub round_wind: Option<TileType>,
    pub seat_wind: Option<TileType>,
    pub visible_tiles: Vec<TileId>,
    pub oya: u8,
    pub discards: [Vec<TileId>; 4],
    pub reached: [bool; 4],
    pub melds: [Vec<Meld>; 4],
}

pub(crate) fn game_context_from_decoded_observation(decoded: &DecodedObservation) -> GameContext {
    GameContext::from_parts_with_melds(
        decoded.drawn_tile,
        decoded.hand_tiles.clone(),
        decoded.dora_indicators.clone(),
        decoded.round_wind,
        decoded.seat_wind,
        decoded.visible_tiles.clone(),
        Some(decoded.player_id),
        Some(decoded.oya),
        decoded.discards.clone(),
        decoded.reached,
        decoded.melds.clone(),
    )
}

#[cfg(test)]
pub(crate) fn fixture_base64(player_id: u8, drawn_tile: Option<u8>, hand: Vec<u8>) -> String {
    fixture_base64_with_dora(player_id, drawn_tile, hand, vec![])
}

#[cfg(test)]
pub(crate) fn fixture_base64_with_dora(
    player_id: u8,
    drawn_tile: Option<u8>,
    hand: Vec<u8>,
    dora_indicators: Vec<u8>,
) -> String {
    fixture_base64_with_winds(player_id, drawn_tile, hand, dora_indicators, 0, 0)
}

#[cfg(test)]
pub(crate) fn fixture_base64_with_winds(
    player_id: u8,
    drawn_tile: Option<u8>,
    hand: Vec<u8>,
    dora_indicators: Vec<u8>,
    round_wind: u8,
    oya: u8,
) -> String {
    let mut hands: [Vec<u8>; 4] = Default::default();
    hands[usize::from(player_id)] = hand;
    let observation = Observation::new(
        player_id,
        hands,
        Default::default(),
        Default::default(),
        dora_indicators,
        [25000; 4],
        [false; 4],
        vec![],
        vec![],
        0,
        0,
        round_wind,
        oya,
        0,
        vec![],
        false,
        [None; 4],
        [None; 4],
        None,
        drawn_tile,
    );
    observation.serialize_to_base64().unwrap()
}

#[cfg(test)]
pub(crate) fn fixture_base64_with_discards(
    player_id: u8,
    drawn_tile: Option<u8>,
    hand: Vec<u8>,
    dora_indicators: Vec<u8>,
    discards: [Vec<u8>; 4],
) -> String {
    let mut hands: [Vec<u8>; 4] = Default::default();
    hands[usize::from(player_id)] = hand;
    let observation = Observation::new(
        player_id,
        hands,
        Default::default(),
        discards,
        dora_indicators,
        [25000; 4],
        [false; 4],
        vec![],
        vec![],
        0,
        0,
        0,
        0,
        0,
        vec![],
        false,
        [None; 4],
        [None; 4],
        None,
        drawn_tile,
    );
    observation.serialize_to_base64().unwrap()
}

#[cfg(test)]
#[allow(clippy::too_many_arguments)]
pub(crate) fn fixture_base64_with_table_state(
    player_id: u8,
    hand: Vec<u8>,
    discards: [Vec<u8>; 4],
    riichi_declared: [bool; 4],
    round_wind: u8,
    oya: u8,
) -> String {
    let mut hands: [Vec<u8>; 4] = Default::default();
    hands[usize::from(player_id)] = hand;
    let observation = Observation::new(
        player_id,
        hands,
        Default::default(),
        discards,
        vec![],
        [25000; 4],
        riichi_declared,
        vec![],
        vec![],
        0,
        0,
        round_wind,
        oya,
        0,
        vec![],
        false,
        [None; 4],
        [None; 4],
        None,
        None,
    );
    observation.serialize_to_base64().unwrap()
}

#[cfg(test)]
#[allow(clippy::too_many_arguments)]
pub(crate) fn fixture_base64_with_melds(
    player_id: u8,
    drawn_tile: Option<u8>,
    hand: Vec<u8>,
    dora_indicators: Vec<u8>,
    discards: [Vec<u8>; 4],
    melds: [Vec<ObservationMeld>; 4],
) -> String {
    let mut hands: [Vec<u8>; 4] = Default::default();
    hands[usize::from(player_id)] = hand;
    let observation = Observation::new(
        player_id,
        hands,
        melds,
        discards,
        dora_indicators,
        [25000; 4],
        [false; 4],
        vec![],
        vec![],
        0,
        0,
        0,
        0,
        0,
        vec![],
        false,
        [None; 4],
        [None; 4],
        None,
        drawn_tile,
    );
    observation.serialize_to_base64().unwrap()
}

#[cfg(test)]
mod tests {
    use super::*;
    use riichienv_core::types::MeldType;

    fn meld(meld_type: MeldType, tiles: Vec<u8>, called_tile: Option<u8>) -> ObservationMeld {
        ObservationMeld::new(meld_type, tiles, called_tile.is_some(), -1, called_tile)
    }

    fn visible_tiles_of(
        meld_type: MeldType,
        tiles: Vec<u8>,
        called_tile: Option<u8>,
    ) -> Vec<TileId> {
        meld_visible_tiles(&decode_meld(&meld(meld_type, tiles, called_tile)))
    }

    #[test]
    fn new_keeps_base64_string() {
        assert_eq!(ObservationPayload::new("abc").as_base64(), "abc");
    }

    #[test]
    fn clone_and_equality() {
        let payload = ObservationPayload::new("abc");
        assert_eq!(payload.clone(), payload);
        assert_ne!(payload, ObservationPayload::new("xyz"));
    }

    #[test]
    fn decode_4p_roundtrip_returns_player_id() {
        let payload = ObservationPayload::new(fixture_base64(2, None, vec![]));
        let decoded = payload.decode_4p().unwrap();
        assert_eq!(decoded.player_id, 2);
    }

    #[test]
    fn decode_4p_without_drawn_tile_returns_none() {
        let payload = ObservationPayload::new(fixture_base64(0, None, vec![]));
        let decoded = payload.decode_4p().unwrap();
        assert_eq!(decoded.drawn_tile, None);
    }

    #[test]
    fn decode_4p_returns_drawn_tile() {
        let payload = ObservationPayload::new(fixture_base64(0, Some(56), vec![]));
        let decoded = payload.decode_4p().unwrap();
        assert_eq!(decoded.drawn_tile, TileId::new(56));
    }

    #[test]
    fn decode_4p_normalizes_drawn_tile_to_temporary_tile_id() {
        for (raw, expected) in [(59, 56), (16, 16), (19, 17)] {
            let payload = ObservationPayload::new(fixture_base64(0, Some(raw), vec![]));
            let decoded = payload.decode_4p().unwrap();
            assert_eq!(decoded.drawn_tile, TileId::new(expected), "raw: {raw}");
        }
    }

    #[test]
    fn decode_4p_out_of_range_drawn_tile_becomes_none() {
        let payload = ObservationPayload::new(fixture_base64(0, Some(200), vec![]));
        let decoded = payload.decode_4p().unwrap();
        assert_eq!(decoded.drawn_tile, None);
    }

    #[test]
    fn decode_4p_returns_hand_tiles() {
        let payload = ObservationPayload::new(fixture_base64(1, None, vec![0, 16, 104]));
        let decoded = payload.decode_4p().unwrap();
        assert_eq!(
            decoded.hand_tiles,
            vec![
                TileId::new(0).unwrap(),
                TileId::new(16).unwrap(),
                TileId::new(104).unwrap(),
            ]
        );
    }

    #[test]
    fn decode_4p_with_empty_hand_returns_empty_hand_tiles() {
        let payload = ObservationPayload::new(fixture_base64(0, None, vec![]));
        let decoded = payload.decode_4p().unwrap();
        assert!(decoded.hand_tiles.is_empty());
    }

    #[test]
    fn decode_4p_normalizes_hand_tiles_to_temporary_tile_id() {
        let payload = ObservationPayload::new(fixture_base64(0, None, vec![59, 16, 19]));
        let decoded = payload.decode_4p().unwrap();
        assert_eq!(
            decoded.hand_tiles,
            vec![
                TileId::new(56).unwrap(),
                TileId::new(16).unwrap(),
                TileId::new(17).unwrap(),
            ]
        );
    }

    #[test]
    fn decode_4p_skips_out_of_range_hand_tiles() {
        let payload = ObservationPayload::new(fixture_base64(0, None, vec![0, 200, 136, 104]));
        let decoded = payload.decode_4p().unwrap();
        assert_eq!(
            decoded.hand_tiles,
            vec![TileId::new(0).unwrap(), TileId::new(104).unwrap()]
        );
    }

    #[test]
    fn decode_4p_returns_both_drawn_tile_and_hand_tiles() {
        let payload = ObservationPayload::new(fixture_base64(0, Some(59), vec![0, 16]));
        let decoded = payload.decode_4p().unwrap();
        assert_eq!(decoded.drawn_tile, TileId::new(56));
        assert_eq!(
            decoded.hand_tiles,
            vec![TileId::new(0).unwrap(), TileId::new(16).unwrap()]
        );
    }

    #[test]
    fn decode_4p_without_dora_indicators_returns_empty() {
        let payload = ObservationPayload::new(fixture_base64(0, None, vec![]));
        let decoded = payload.decode_4p().unwrap();
        assert!(decoded.dora_indicators.is_empty());
    }

    #[test]
    fn decode_4p_returns_dora_indicators() {
        let payload =
            ObservationPayload::new(fixture_base64_with_dora(0, None, vec![], vec![0, 104]));
        let decoded = payload.decode_4p().unwrap();
        assert_eq!(
            decoded.dora_indicators,
            vec![TileId::new(0).unwrap(), TileId::new(104).unwrap()]
        );
    }

    #[test]
    fn decode_4p_normalizes_dora_indicators_to_temporary_tile_id() {
        let payload =
            ObservationPayload::new(fixture_base64_with_dora(0, None, vec![], vec![59, 16, 19]));
        let decoded = payload.decode_4p().unwrap();
        assert_eq!(
            decoded.dora_indicators,
            vec![
                TileId::new(56).unwrap(),
                TileId::new(16).unwrap(),
                TileId::new(17).unwrap(),
            ]
        );
    }

    #[test]
    fn decode_4p_skips_out_of_range_dora_indicators() {
        let payload = ObservationPayload::new(fixture_base64_with_dora(
            0,
            None,
            vec![],
            vec![0, 200, 136, 104],
        ));
        let decoded = payload.decode_4p().unwrap();
        assert_eq!(
            decoded.dora_indicators,
            vec![TileId::new(0).unwrap(), TileId::new(104).unwrap()]
        );
    }

    #[test]
    fn decode_4p_rejects_invalid_base64() {
        let payload = ObservationPayload::new("not-valid-base64!!");
        assert!(matches!(
            payload.decode_4p(),
            Err(ObservationError::Decode(_))
        ));
    }

    #[test]
    fn decode_4p_rejects_non_observation_json() {
        let payload = ObservationPayload::new("eyJmb28iOjF9");
        assert!(matches!(
            payload.decode_4p(),
            Err(ObservationError::Decode(_))
        ));
    }

    fn wind(value: u8) -> TileType {
        TileType::new(value).unwrap()
    }

    #[test]
    fn decode_4p_returns_round_wind_from_observation() {
        for (round_wind, expected) in [(0, 27), (1, 28), (2, 29), (3, 30)] {
            let payload = ObservationPayload::new(fixture_base64_with_winds(
                0,
                None,
                vec![],
                vec![],
                round_wind,
                0,
            ));
            let decoded = payload.decode_4p().unwrap();
            assert_eq!(
                decoded.round_wind,
                Some(wind(expected)),
                "round_wind: {round_wind}"
            );
        }
    }

    #[test]
    fn decode_4p_seat_wind_is_east_when_player_is_oya() {
        let payload =
            ObservationPayload::new(fixture_base64_with_winds(2, None, vec![], vec![], 0, 2));
        let decoded = payload.decode_4p().unwrap();
        assert_eq!(decoded.seat_wind, Some(wind(27)));
    }

    #[test]
    fn decode_4p_seat_wind_is_south_for_oya_shimocha() {
        let payload =
            ObservationPayload::new(fixture_base64_with_winds(2, None, vec![], vec![], 0, 1));
        let decoded = payload.decode_4p().unwrap();
        assert_eq!(decoded.seat_wind, Some(wind(28)));
    }

    #[test]
    fn decode_4p_seat_wind_is_west_for_oya_toimen() {
        let payload =
            ObservationPayload::new(fixture_base64_with_winds(3, None, vec![], vec![], 0, 1));
        let decoded = payload.decode_4p().unwrap();
        assert_eq!(decoded.seat_wind, Some(wind(29)));
    }

    #[test]
    fn decode_4p_seat_wind_is_north_for_oya_kamicha() {
        let payload =
            ObservationPayload::new(fixture_base64_with_winds(0, None, vec![], vec![], 0, 1));
        let decoded = payload.decode_4p().unwrap();
        assert_eq!(decoded.seat_wind, Some(wind(30)));
    }

    #[test]
    fn seat_wind_from_covers_all_seats() {
        assert_eq!(seat_wind_from(0, 0), Some(wind(27)));
        assert_eq!(seat_wind_from(1, 0), Some(wind(28)));
        assert_eq!(seat_wind_from(2, 0), Some(wind(29)));
        assert_eq!(seat_wind_from(3, 0), Some(wind(30)));
        assert_eq!(seat_wind_from(0, 3), Some(wind(28)));
        assert_eq!(seat_wind_from(1, 3), Some(wind(29)));
    }

    #[test]
    fn seat_wind_from_rejects_out_of_range_inputs() {
        assert_eq!(seat_wind_from(4, 0), None);
        assert_eq!(seat_wind_from(0, 4), None);
        assert_eq!(seat_wind_from(255, 0), None);
        assert_eq!(seat_wind_from(0, 255), None);
    }

    #[test]
    fn decode_4p_default_fixture_has_east_winds() {
        let payload = ObservationPayload::new(fixture_base64(0, None, vec![]));
        let decoded = payload.decode_4p().unwrap();
        assert_eq!(decoded.round_wind, Some(wind(27)));
        assert_eq!(decoded.seat_wind, Some(wind(27)));
    }

    #[test]
    fn decode_4p_visible_tiles_include_hand_tiles() {
        let payload = ObservationPayload::new(fixture_base64(1, None, vec![0, 16, 104]));
        let decoded = payload.decode_4p().unwrap();
        for tile in [
            TileId::new(0).unwrap(),
            TileId::new(16).unwrap(),
            TileId::new(104).unwrap(),
        ] {
            assert!(decoded.visible_tiles.contains(&tile), "missing {tile:?}");
        }
    }

    #[test]
    fn decode_4p_visible_tiles_include_drawn_tile_via_hand() {
        let payload = ObservationPayload::new(fixture_base64(0, Some(16), vec![0, 16, 104]));
        let decoded = payload.decode_4p().unwrap();
        assert_eq!(decoded.drawn_tile, TileId::new(16));
        assert!(decoded.visible_tiles.contains(&TileId::new(16).unwrap()));
    }

    #[test]
    fn decode_4p_visible_tiles_do_not_double_count_drawn_tile() {
        let payload = ObservationPayload::new(fixture_base64(0, Some(16), vec![0, 16, 104]));
        let decoded = payload.decode_4p().unwrap();
        let count = decoded
            .visible_tiles
            .iter()
            .filter(|&&tile| tile == TileId::new(16).unwrap())
            .count();
        assert_eq!(count, 1);
    }

    #[test]
    fn decode_4p_visible_tiles_include_dora_indicators() {
        let payload =
            ObservationPayload::new(fixture_base64_with_dora(0, None, vec![], vec![4, 20]));
        let decoded = payload.decode_4p().unwrap();
        for tile in [TileId::new(4).unwrap(), TileId::new(20).unwrap()] {
            assert!(decoded.visible_tiles.contains(&tile), "missing {tile:?}");
        }
    }

    #[test]
    fn decode_4p_visible_tiles_include_discards_of_all_players() {
        let discards = [vec![0], vec![16], vec![104], vec![132]];
        let payload = ObservationPayload::new(fixture_base64_with_discards(
            0,
            None,
            vec![],
            vec![],
            discards,
        ));
        let decoded = payload.decode_4p().unwrap();
        for tile in [
            TileId::new(0).unwrap(),
            TileId::new(16).unwrap(),
            TileId::new(104).unwrap(),
            TileId::new(132).unwrap(),
        ] {
            assert!(decoded.visible_tiles.contains(&tile), "missing {tile:?}");
        }
    }

    #[test]
    fn decode_4p_visible_tiles_normalize_discards_to_temporary_tile_id() {
        let discards = [vec![59], vec![19], vec![], vec![]];
        let payload = ObservationPayload::new(fixture_base64_with_discards(
            0,
            None,
            vec![],
            vec![],
            discards,
        ));
        let decoded = payload.decode_4p().unwrap();
        assert!(decoded.visible_tiles.contains(&TileId::new(56).unwrap()));
        assert!(decoded.visible_tiles.contains(&TileId::new(17).unwrap()));
    }

    #[test]
    fn decode_4p_visible_tiles_skip_out_of_range_tiles() {
        let discards = [vec![0, 200, 136], vec![], vec![], vec![]];
        let payload = ObservationPayload::new(fixture_base64_with_discards(
            0,
            None,
            vec![0, 200, 136, 104],
            vec![0, 200],
            discards,
        ));
        let decoded = payload.decode_4p().unwrap();
        assert!(!decoded.visible_tiles.iter().any(|tile| tile.raw() >= 136));
    }

    #[test]
    fn decode_4p_visible_tiles_preserve_duplicate_count_across_sources() {
        let discards = [vec![1], vec![2], vec![3], vec![]];
        let payload = ObservationPayload::new(fixture_base64_with_discards(
            0,
            None,
            vec![0],
            vec![],
            discards,
        ));
        let decoded = payload.decode_4p().unwrap();
        let count = decoded
            .visible_tiles
            .iter()
            .filter(|&&tile| tile == TileId::new(0).unwrap())
            .count();
        assert_eq!(count, 4);
    }

    #[test]
    fn decode_4p_visible_tiles_empty_when_nothing_visible() {
        let payload = ObservationPayload::new(fixture_base64(0, None, vec![]));
        let decoded = payload.decode_4p().unwrap();
        assert!(decoded.visible_tiles.is_empty());
    }

    fn count_visible(decoded: &DecodedObservation, tile: TileId) -> usize {
        decoded.visible_tiles.iter().filter(|&&t| t == tile).count()
    }

    #[test]
    fn meld_visible_tiles_pon_returns_consumed_without_called_tile() {
        let tiles = visible_tiles_of(MeldType::Pon, vec![2, 0, 1], Some(2));
        assert_eq!(
            tiles,
            vec![TileId::new(0).unwrap(), TileId::new(0).unwrap()]
        );
    }

    #[test]
    fn meld_visible_tiles_ankan_returns_all_four() {
        let tiles = visible_tiles_of(MeldType::Ankan, vec![0, 1, 2, 3], None);
        assert_eq!(tiles, vec![TileId::new(0).unwrap(); 4]);
    }

    #[test]
    fn meld_visible_tiles_kakan_excludes_original_called_tile() {
        let tiles = visible_tiles_of(MeldType::Kakan, vec![2, 0, 1, 3], Some(2));
        assert_eq!(tiles, vec![TileId::new(0).unwrap(); 3]);
    }

    #[test]
    fn meld_visible_tiles_skips_out_of_range_tiles() {
        let tiles = visible_tiles_of(MeldType::Ankan, vec![0, 200, 136, 3], None);
        assert_eq!(
            tiles,
            vec![TileId::new(0).unwrap(), TileId::new(0).unwrap()]
        );
    }

    #[test]
    fn decode_4p_visible_tiles_include_chi_consumed() {
        let mut melds: [Vec<ObservationMeld>; 4] = Default::default();
        melds[1] = vec![meld(MeldType::Chi, vec![8, 4, 12], Some(8))];
        let payload = ObservationPayload::new(fixture_base64_with_melds(
            0,
            None,
            vec![],
            vec![],
            Default::default(),
            melds,
        ));
        let decoded = payload.decode_4p().unwrap();
        assert!(decoded.visible_tiles.contains(&TileId::new(4).unwrap()));
        assert!(decoded.visible_tiles.contains(&TileId::new(12).unwrap()));
    }

    #[test]
    fn decode_4p_visible_tiles_include_pon_consumed() {
        let mut melds: [Vec<ObservationMeld>; 4] = Default::default();
        melds[2] = vec![meld(MeldType::Pon, vec![2, 0, 1], Some(2))];
        let payload = ObservationPayload::new(fixture_base64_with_melds(
            0,
            None,
            vec![],
            vec![],
            Default::default(),
            melds,
        ));
        let decoded = payload.decode_4p().unwrap();
        assert_eq!(count_visible(&decoded, TileId::new(0).unwrap()), 2);
    }

    #[test]
    fn decode_4p_visible_tiles_include_daiminkan_consumed() {
        let mut melds: [Vec<ObservationMeld>; 4] = Default::default();
        melds[3] = vec![meld(
            MeldType::Daiminkan,
            vec![104, 105, 106, 107],
            Some(104),
        )];
        let payload = ObservationPayload::new(fixture_base64_with_melds(
            0,
            None,
            vec![],
            vec![],
            Default::default(),
            melds,
        ));
        let decoded = payload.decode_4p().unwrap();
        assert_eq!(count_visible(&decoded, TileId::new(104).unwrap()), 3);
    }

    #[test]
    fn decode_4p_visible_tiles_include_ankan_all_four() {
        let mut melds: [Vec<ObservationMeld>; 4] = Default::default();
        melds[0] = vec![meld(MeldType::Ankan, vec![72, 73, 74, 75], None)];
        let payload = ObservationPayload::new(fixture_base64_with_melds(
            0,
            None,
            vec![],
            vec![],
            Default::default(),
            melds,
        ));
        let decoded = payload.decode_4p().unwrap();
        assert_eq!(count_visible(&decoded, TileId::new(72).unwrap()), 4);
    }

    #[test]
    fn decode_4p_visible_tiles_do_not_double_count_called_tile_with_discard() {
        // 鳴かれた牌は discarder の河に残るため、meld 側では called tile を数えない。
        let mut discards: [Vec<u8>; 4] = Default::default();
        discards[1] = vec![2];
        let mut melds: [Vec<ObservationMeld>; 4] = Default::default();
        melds[2] = vec![meld(MeldType::Pon, vec![2, 0, 1], Some(2))];
        let payload = ObservationPayload::new(fixture_base64_with_melds(
            0,
            None,
            vec![],
            vec![],
            discards,
            melds,
        ));
        let decoded = payload.decode_4p().unwrap();
        // 河の 1 枚 + consumed 2 枚 = 3 枚。called tile を meld 側でも数えると 4 枚になってしまう。
        assert_eq!(count_visible(&decoded, TileId::new(0).unwrap()), 3);
    }

    #[test]
    fn decode_4p_visible_tiles_skip_out_of_range_meld_tiles() {
        let mut melds: [Vec<ObservationMeld>; 4] = Default::default();
        melds[0] = vec![meld(MeldType::Ankan, vec![0, 200, 136, 3], None)];
        let payload = ObservationPayload::new(fixture_base64_with_melds(
            0,
            None,
            vec![],
            vec![],
            Default::default(),
            melds,
        ));
        let decoded = payload.decode_4p().unwrap();
        assert!(!decoded.visible_tiles.iter().any(|tile| tile.raw() >= 136));
        assert_eq!(count_visible(&decoded, TileId::new(0).unwrap()), 2);
    }

    #[test]
    fn decode_4p_visible_tiles_include_melds_of_all_players() {
        let mut melds: [Vec<ObservationMeld>; 4] = Default::default();
        melds[0] = vec![meld(MeldType::Ankan, vec![0, 1, 2, 3], None)];
        melds[3] = vec![meld(MeldType::Pon, vec![110, 108, 109], Some(110))];
        let payload = ObservationPayload::new(fixture_base64_with_melds(
            0,
            None,
            vec![],
            vec![],
            Default::default(),
            melds,
        ));
        let decoded = payload.decode_4p().unwrap();
        assert_eq!(count_visible(&decoded, TileId::new(0).unwrap()), 4);
        assert_eq!(count_visible(&decoded, TileId::new(108).unwrap()), 2);
    }

    #[test]
    fn decode_4p_visible_tiles_without_melds_match_previous_sources() {
        let discards = [vec![0], vec![16], vec![], vec![]];
        let with_helper = ObservationPayload::new(fixture_base64_with_melds(
            0,
            None,
            vec![104],
            vec![4],
            discards.clone(),
            Default::default(),
        ))
        .decode_4p()
        .unwrap();
        let without_helper = ObservationPayload::new(fixture_base64_with_discards(
            0,
            None,
            vec![104],
            vec![4],
            discards,
        ))
        .decode_4p()
        .unwrap();
        assert_eq!(with_helper.visible_tiles, without_helper.visible_tiles);
    }

    fn decoded_with_melds(melds: [Vec<ObservationMeld>; 4]) -> DecodedObservation {
        ObservationPayload::new(fixture_base64_with_melds(
            0,
            None,
            vec![],
            vec![],
            Default::default(),
            melds,
        ))
        .decode_4p()
        .unwrap()
    }

    fn decoded_with_meld_of(player: usize, meld: ObservationMeld) -> DecodedObservation {
        let mut melds: [Vec<ObservationMeld>; 4] = Default::default();
        melds[player] = vec![meld];
        decoded_with_melds(melds)
    }

    #[test]
    fn decode_4p_maps_every_meld_type_to_the_shared_meld_kind() {
        for (meld_type, tiles, called_tile, expected) in [
            (MeldType::Chi, vec![8, 4, 12], Some(8), MeldKind::Chi),
            (MeldType::Pon, vec![2, 0, 1], Some(2), MeldKind::Pon),
            (
                MeldType::Daiminkan,
                vec![3, 0, 1, 2],
                Some(3),
                MeldKind::Daiminkan,
            ),
            (MeldType::Ankan, vec![0, 1, 2, 3], None, MeldKind::Ankan),
            (MeldType::Kakan, vec![2, 0, 1, 3], Some(2), MeldKind::Kakan),
        ] {
            let decoded = decoded_with_meld_of(1, meld(meld_type, tiles, called_tile));
            assert_eq!(decoded.melds[1].len(), 1, "meld type: {meld_type:?}");
            assert_eq!(
                decoded.melds[1][0].kind(),
                expected,
                "meld type: {meld_type:?}"
            );
        }
    }

    #[test]
    fn decode_4p_keeps_meld_tiles_and_called_tile() {
        let decoded = decoded_with_meld_of(2, meld(MeldType::Chi, vec![8, 4, 12], Some(8)));
        let meld = &decoded.melds[2][0];
        assert_eq!(
            meld.tiles(),
            [
                TileId::new(8).unwrap(),
                TileId::new(4).unwrap(),
                TileId::new(12).unwrap(),
            ]
        );
        assert_eq!(meld.called_tile(), TileId::new(8));
        assert!(meld.is_open());
    }

    #[test]
    fn decode_4p_ankan_has_no_called_tile_and_is_not_open() {
        let decoded = decoded_with_meld_of(0, meld(MeldType::Ankan, vec![72, 73, 74, 75], None));
        let meld = &decoded.melds[0][0];
        assert_eq!(meld.kind(), MeldKind::Ankan);
        assert_eq!(meld.called_tile(), None);
        assert!(!meld.is_open());
    }

    #[test]
    fn decode_4p_kakan_stays_a_single_meld() {
        let decoded = decoded_with_meld_of(3, meld(MeldType::Kakan, vec![2, 0, 1, 3], Some(2)));
        assert_eq!(decoded.melds[3].len(), 1);
        assert_eq!(decoded.melds[3][0].kind(), MeldKind::Kakan);
        assert_eq!(decoded.melds[3][0].tiles().len(), 4);
    }

    #[test]
    fn decode_4p_without_melds_has_empty_melds() {
        let decoded = decoded_with_melds(Default::default());
        assert!(decoded.melds.iter().all(|melds| melds.is_empty()));
    }

    #[test]
    fn decode_4p_keeps_melds_per_player() {
        let mut melds: [Vec<ObservationMeld>; 4] = Default::default();
        melds[0] = vec![meld(MeldType::Ankan, vec![0, 1, 2, 3], None)];
        melds[3] = vec![
            meld(MeldType::Pon, vec![110, 108, 109], Some(110)),
            meld(MeldType::Chi, vec![8, 4, 12], Some(8)),
        ];
        let decoded = decoded_with_melds(melds);
        assert_eq!(decoded.melds[0].len(), 1);
        assert!(decoded.melds[1].is_empty());
        assert!(decoded.melds[2].is_empty());
        assert_eq!(decoded.melds[3].len(), 2);
    }

    #[test]
    fn own_pon_makes_context_fixed_meld_count_one() {
        let decoded = decoded_with_meld_of(0, meld(MeldType::Pon, vec![110, 108, 109], Some(110)));
        let context = game_context_from_decoded_observation(&decoded);
        assert_eq!(context.player_id(), Some(0));
        assert_eq!(context.own_melds().map(<[_]>::len), Some(1));
        assert_eq!(
            context
                .own_fixed_meld_count()
                .map(bot_logic::FixedMeldCount::get),
            Some(1)
        );
    }

    #[test]
    fn opponent_melds_do_not_change_context_fixed_meld_count() {
        let decoded = decoded_with_meld_of(1, meld(MeldType::Pon, vec![110, 108, 109], Some(110)));
        let context = game_context_from_decoded_observation(&decoded);
        assert_eq!(context.melds_of(1).map(<[_]>::len), Some(1));
        assert_eq!(
            context
                .own_fixed_meld_count()
                .map(bot_logic::FixedMeldCount::get),
            Some(0)
        );
    }

    #[test]
    fn meld_tiles_are_not_counted_twice_in_visible_tiles() {
        let decoded = decoded_with_meld_of(1, meld(MeldType::Pon, vec![110, 108, 109], Some(110)));
        let context = game_context_from_decoded_observation(&decoded);
        assert_eq!(count_visible(&decoded, TileId::new(108).unwrap()), 2);
        assert_eq!(
            context
                .visible_tiles()
                .iter()
                .filter(|&&tile| tile == TileId::new(108).unwrap())
                .count(),
            2
        );
        assert_eq!(context.melds_of(1).unwrap()[0].tiles().len(), 3);
    }

    #[test]
    fn decode_4p_returns_oya() {
        let payload = ObservationPayload::new(fixture_base64_with_table_state(
            0,
            vec![],
            Default::default(),
            [false; 4],
            0,
            2,
        ));
        let decoded = payload.decode_4p().unwrap();
        assert_eq!(decoded.oya, 2);
    }

    #[test]
    fn decode_4p_returns_discards_per_player() {
        let discards = [vec![0], vec![16], vec![], vec![104, 132]];
        let payload = ObservationPayload::new(fixture_base64_with_table_state(
            0,
            vec![],
            discards,
            [false; 4],
            0,
            0,
        ));
        let decoded = payload.decode_4p().unwrap();
        assert_eq!(decoded.discards[0], vec![TileId::new(0).unwrap()]);
        assert_eq!(decoded.discards[1], vec![TileId::new(16).unwrap()]);
        assert!(decoded.discards[2].is_empty());
        assert_eq!(
            decoded.discards[3],
            vec![TileId::new(104).unwrap(), TileId::new(132).unwrap()]
        );
    }

    #[test]
    fn decode_4p_normalizes_discards_to_temporary_tile_id() {
        let discards = [vec![59], vec![19], vec![], vec![]];
        let payload = ObservationPayload::new(fixture_base64_with_table_state(
            0,
            vec![],
            discards,
            [false; 4],
            0,
            0,
        ));
        let decoded = payload.decode_4p().unwrap();
        assert_eq!(decoded.discards[0], vec![TileId::new(56).unwrap()]);
        assert_eq!(decoded.discards[1], vec![TileId::new(17).unwrap()]);
    }

    #[test]
    fn decode_4p_skips_out_of_range_discards() {
        let discards = [vec![0, 200, 136, 104], vec![], vec![], vec![]];
        let payload = ObservationPayload::new(fixture_base64_with_table_state(
            0,
            vec![],
            discards,
            [false; 4],
            0,
            0,
        ));
        let decoded = payload.decode_4p().unwrap();
        assert_eq!(
            decoded.discards[0],
            vec![TileId::new(0).unwrap(), TileId::new(104).unwrap()]
        );
    }

    #[test]
    fn decode_4p_returns_reached_flags() {
        let payload = ObservationPayload::new(fixture_base64_with_table_state(
            0,
            vec![],
            Default::default(),
            [false, true, false, true],
            0,
            0,
        ));
        let decoded = payload.decode_4p().unwrap();
        assert_eq!(decoded.reached, [false, true, false, true]);
    }

    #[test]
    fn decode_4p_discards_still_feed_visible_tiles() {
        let discards = [vec![0], vec![16], vec![104], vec![132]];
        let payload = ObservationPayload::new(fixture_base64_with_table_state(
            0,
            vec![],
            discards,
            [false; 4],
            0,
            0,
        ));
        let decoded = payload.decode_4p().unwrap();
        for tile in [
            TileId::new(0).unwrap(),
            TileId::new(16).unwrap(),
            TileId::new(104).unwrap(),
            TileId::new(132).unwrap(),
        ] {
            assert!(decoded.visible_tiles.contains(&tile), "missing {tile:?}");
        }
    }

    mod drawn_tile_separation {
        use super::*;
        use bot_logic::evaluate_discards_from_tiles_with_context;

        fn count(tiles: &[TileId], tile: TileId) -> usize {
            tiles.iter().filter(|&&t| t == tile).count()
        }

        // 1. 上流形式の基本ケース: raw hand 14 枚のうち drawn_tile と一致する物理牌を 1 枚分離する。
        #[test]
        fn upstream_hand_separates_drawn_tile() {
            let hand = vec![0, 4, 8, 12, 16, 36, 40, 44, 48, 72, 76, 104, 120, 108];
            assert_eq!(hand.len(), 14);
            let payload = ObservationPayload::new(fixture_base64(0, Some(108), hand));
            let decoded = payload.decode_4p().unwrap();
            assert_eq!(decoded.hand_tiles.len(), 13);
            assert!(decoded.drawn_tile.is_some());
            assert_eq!(decoded.hand_tiles.len() + 1, 14);

            let context = game_context_from_decoded_observation(&decoded);
            assert_eq!(context.hand_tiles().len(), 13);
            assert!(context.drawn_tile().is_some());
        }

        // 2. 同牌種の別物理牌 ID を複数含む hand から、ツモった 1 枚だけを枚数として分離する。
        #[test]
        fn removes_only_one_physical_tile() {
            // 5m の物理牌 ID A=17, B=18。drawn は B=18。
            let payload = ObservationPayload::new(fixture_base64(0, Some(18), vec![17, 18]));
            let decoded = payload.decode_4p().unwrap();
            // 正規化後は A も B も temporary 17 になるが、枚数は 1 枚だけ減る。
            assert_eq!(decoded.hand_tiles.len(), 1);
            assert_eq!(count(&decoded.hand_tiles, TileId::new(17).unwrap()), 1);
            assert_eq!(decoded.drawn_tile, TileId::new(17));
        }

        // 3. 赤5ツモ: 赤5m を分離し、通常5m は hand に残す。
        #[test]
        fn red_five_tsumo_keeps_normal_five_in_hand() {
            // 16 = 赤5m, 17 = 通常5m, 4 = 2m。drawn は赤5m=16。
            let payload = ObservationPayload::new(fixture_base64(0, Some(16), vec![16, 17, 4]));
            let decoded = payload.decode_4p().unwrap();
            // 通常5m は残り、赤5m は分離される。
            assert!(decoded.hand_tiles.contains(&TileId::new(17).unwrap()));
            assert!(!decoded.hand_tiles.contains(&TileId::new(16).unwrap()));
            assert_eq!(decoded.drawn_tile, TileId::new(16));
            // visible_tiles には赤5m と通常5m を各 1 枚含む。
            assert_eq!(count(&decoded.visible_tiles, TileId::new(16).unwrap()), 1);
            assert_eq!(count(&decoded.visible_tiles, TileId::new(17).unwrap()), 1);
        }

        // 4. 通常5ツモ: 通常5m だけを分離し、赤5m は hand に残す。
        #[test]
        fn normal_five_tsumo_keeps_red_five_in_hand() {
            // 16 = 赤5m, 17 = 通常5m。drawn は通常5m=17。
            let payload = ObservationPayload::new(fixture_base64(0, Some(17), vec![16, 17]));
            let decoded = payload.decode_4p().unwrap();
            assert!(decoded.hand_tiles.contains(&TileId::new(16).unwrap()));
            assert!(!decoded.hand_tiles.contains(&TileId::new(17).unwrap()));
            assert_eq!(decoded.drawn_tile, TileId::new(17));
        }

        // 5. 同牌種だが raw 物理牌 ID が一致しない互換形式: 除去せず drawn を維持する。
        #[test]
        fn same_type_but_different_raw_id_is_not_removed() {
            // hand は通常5m の物理牌 A=17。drawn は通常5m の物理牌 B=18 (hand に存在しない)。
            let payload = ObservationPayload::new(fixture_base64(0, Some(18), vec![17]));
            let decoded = payload.decode_4p().unwrap();
            // A を除去しない。
            assert_eq!(decoded.hand_tiles.len(), 1);
            assert_eq!(decoded.drawn_tile, TileId::new(17));
            // visible_tiles には A と B の 2 枚分を含む。
            assert_eq!(count(&decoded.visible_tiles, TileId::new(17).unwrap()), 2);
        }

        // 6. drawn tile を含まない互換形式: 13 枚の hand を維持し、drawn を別に保持する。
        #[test]
        fn compat_hand_without_drawn_tile_keeps_thirteen() {
            let hand = vec![0, 4, 8, 12, 16, 36, 40, 44, 48, 72, 76, 104, 108];
            assert_eq!(hand.len(), 13);
            // drawn=132 (C) は hand に存在しない。
            let payload = ObservationPayload::new(fixture_base64(0, Some(132), hand));
            let decoded = payload.decode_4p().unwrap();
            assert_eq!(decoded.hand_tiles.len(), 13);
            assert_eq!(decoded.drawn_tile, TileId::new(132));
            let context = game_context_from_decoded_observation(&decoded);
            let after_tsumo =
                context.hand_tiles().len() + usize::from(context.drawn_tile().is_some());
            assert_eq!(after_tsumo, 14);
            // visible_tiles に drawn tile を 1 枚追加する。
            assert_eq!(count(&decoded.visible_tiles, TileId::new(132).unwrap()), 1);
        }

        // 7. drawn tile なし: hand も visible_tiles も既存挙動を維持する。
        #[test]
        fn no_drawn_tile_keeps_hand_and_visible() {
            let payload = ObservationPayload::new(fixture_base64(0, None, vec![0, 4]));
            let decoded = payload.decode_4p().unwrap();
            assert_eq!(
                decoded.hand_tiles,
                vec![TileId::new(0).unwrap(), TileId::new(4).unwrap()]
            );
            assert_eq!(decoded.drawn_tile, None);
            assert_eq!(
                decoded.visible_tiles,
                vec![TileId::new(0).unwrap(), TileId::new(4).unwrap()]
            );
        }

        // 8. visible_tiles の一回計上: 上流形式・互換形式ともツモ牌をちょうど 1 枚含む。
        #[test]
        fn visible_tiles_count_drawn_tile_once_upstream() {
            // raw hand 14 枚がツモ牌 108 を含む。dora / 河 / 副露なし。
            let hand = vec![0, 4, 8, 12, 16, 36, 40, 44, 48, 72, 76, 104, 120, 108];
            let payload = ObservationPayload::new(fixture_base64(0, Some(108), hand));
            let decoded = payload.decode_4p().unwrap();
            assert_eq!(decoded.visible_tiles.len(), 14);
            assert_eq!(count(&decoded.visible_tiles, TileId::new(108).unwrap()), 1);
        }

        #[test]
        fn visible_tiles_count_drawn_tile_once_compat() {
            // raw hand 13 枚にツモ牌 132 を含まない。
            let hand = vec![0, 4, 8, 12, 16, 36, 40, 44, 48, 72, 76, 104, 108];
            let payload = ObservationPayload::new(fixture_base64(0, Some(132), hand));
            let decoded = payload.decode_4p().unwrap();
            assert_eq!(decoded.visible_tiles.len(), 14);
            assert_eq!(count(&decoded.visible_tiles, TileId::new(132).unwrap()), 1);
        }

        // 9. 実際に確認された回帰牌姿。ツモ牌 N を二重計上すると偽の七対子テンパイになる。
        #[test]
        fn regression_north_tsumo_is_not_false_chiitoitsu_tenpai() {
            // 2m 3m 4m 4m 5m 5m 4p 4p 5p 2s 2s 9s 9s N の 14 枚。drawn は hand 内の N=120。
            let hand = vec![4, 8, 12, 13, 17, 18, 48, 49, 53, 76, 77, 104, 105, 120];
            assert_eq!(hand.len(), 14);
            let payload = ObservationPayload::new(fixture_base64(0, Some(120), hand));
            let decoded = payload.decode_4p().unwrap();
            assert_eq!(decoded.hand_tiles.len(), 13);
            assert_eq!(decoded.drawn_tile, TileId::new(120));
            // ツモ牌の N は hand_tiles に含まれない。
            let north = TileId::new(120).unwrap();
            assert_eq!(count(&decoded.hand_tiles, north), 0);

            let context = game_context_from_decoded_observation(&decoded);
            let tiles: Vec<TileId> = context
                .hand_tiles()
                .iter()
                .copied()
                .chain(context.drawn_tile())
                .collect();
            assert_eq!(tiles.len(), 14);
            // 評価牌内の N は 1 枚。
            assert_eq!(count(&tiles, north), 1);

            let evaluations = evaluate_discards_from_tiles_with_context(
                &tiles,
                context.dora_indicators(),
                context.round_wind(),
                context.seat_wind(),
            );
            let five_pin = evaluations
                .iter()
                .find(|evaluation| evaluation.discard.to_mjai_string() == "5p")
                .expect("5p 切り候補が存在する");
            let chiitoitsu = five_pin
                .shanten_after_discard
                .concealed()
                .expect("門前手なので七対子向聴数を持つ")
                .chiitoitsu;
            assert_eq!(chiitoitsu, 1);
            // 15 枚評価による偽の七対子テンパイ (0 向聴) を防止する。
            assert_ne!(chiitoitsu, 0);
        }

        // 10. helper は渡された clone だけを操作し、元の raw hand を変更しない。
        #[test]
        fn remove_drawn_tile_once_only_mutates_passed_clone() {
            let raw_hand: Vec<u32> = vec![17, 18, 4];
            let mut concealed = raw_hand.clone();
            let removed = remove_drawn_tile_once(&mut concealed, Some(18));
            assert!(removed);
            assert_eq!(concealed, vec![17, 4]);
            // 元の raw hand は変更されない。
            assert_eq!(raw_hand, vec![17, 18, 4]);
        }

        #[test]
        fn remove_drawn_tile_once_returns_false_without_drawn_tile() {
            let mut hand: Vec<u32> = vec![17, 18];
            assert!(!remove_drawn_tile_once(&mut hand, None));
            assert_eq!(hand, vec![17, 18]);
        }

        #[test]
        fn remove_drawn_tile_once_returns_false_when_absent() {
            let mut hand: Vec<u32> = vec![17];
            assert!(!remove_drawn_tile_once(&mut hand, Some(18)));
            assert_eq!(hand, vec![17]);
        }
    }

    mod game_context_helper {
        use super::*;

        fn decoded(
            player_id: u8,
            drawn_tile: Option<TileId>,
            hand_tiles: Vec<TileId>,
        ) -> DecodedObservation {
            DecodedObservation {
                player_id,
                drawn_tile,
                hand_tiles,
                dora_indicators: vec![],
                round_wind: None,
                seat_wind: None,
                visible_tiles: vec![],
                oya: 0,
                discards: Default::default(),
                reached: [false; 4],
                melds: Default::default(),
            }
        }

        #[test]
        fn drawn_tile_becomes_context_drawn_tile() {
            let tile = TileId::new(56).unwrap();
            let context = game_context_from_decoded_observation(&decoded(0, Some(tile), vec![]));
            assert_eq!(context.drawn_tile(), Some(tile));
        }

        #[test]
        fn no_drawn_tile_becomes_none_drawn_tile() {
            let context = game_context_from_decoded_observation(&decoded(3, None, vec![]));
            assert_eq!(context.drawn_tile(), None);
        }

        #[test]
        fn drawn_tile_and_hand_tiles_become_context_parts() {
            let tile = TileId::new(56).unwrap();
            let hand_tiles = vec![TileId::new(0).unwrap(), TileId::new(16).unwrap()];
            let context =
                game_context_from_decoded_observation(&decoded(0, Some(tile), hand_tiles.clone()));
            assert_eq!(context.drawn_tile(), Some(tile));
            assert_eq!(context.hand_tiles(), hand_tiles.as_slice());
        }

        #[test]
        fn hand_tiles_are_kept_without_drawn_tile() {
            let hand_tiles = vec![TileId::new(104).unwrap()];
            let context =
                game_context_from_decoded_observation(&decoded(1, None, hand_tiles.clone()));
            assert_eq!(context.drawn_tile(), None);
            assert_eq!(context.hand_tiles(), hand_tiles.as_slice());
        }

        #[test]
        fn dora_indicators_are_passed_to_context() {
            let dora_indicators = vec![TileId::new(4).unwrap(), TileId::new(20).unwrap()];
            let mut d = decoded(2, None, vec![]);
            d.dora_indicators = dora_indicators.clone();
            let context = game_context_from_decoded_observation(&d);
            assert_eq!(context.dora_indicators(), dora_indicators.as_slice());
        }

        #[test]
        fn empty_dora_indicators_become_empty_context_dora() {
            let context = game_context_from_decoded_observation(&decoded(0, None, vec![]));
            assert!(context.dora_indicators().is_empty());
        }

        #[test]
        fn winds_are_passed_to_context() {
            let mut d = decoded(0, None, vec![]);
            d.round_wind = Some(wind(27));
            d.seat_wind = Some(wind(28));
            let context = game_context_from_decoded_observation(&d);
            assert_eq!(context.round_wind(), Some(wind(27)));
            assert_eq!(context.seat_wind(), Some(wind(28)));
        }

        #[test]
        fn absent_winds_become_none_in_context() {
            let context = game_context_from_decoded_observation(&decoded(0, None, vec![]));
            assert_eq!(context.round_wind(), None);
            assert_eq!(context.seat_wind(), None);
        }

        #[test]
        fn visible_tiles_are_passed_to_context() {
            let visible_tiles = vec![
                TileId::new(0).unwrap(),
                TileId::new(16).unwrap(),
                TileId::new(16).unwrap(),
            ];
            let mut d = decoded(0, None, vec![]);
            d.visible_tiles = visible_tiles.clone();
            let context = game_context_from_decoded_observation(&d);
            assert_eq!(context.visible_tiles(), visible_tiles.as_slice());
        }

        #[test]
        fn empty_visible_tiles_become_empty_context_visible_tiles() {
            let context = game_context_from_decoded_observation(&decoded(0, None, vec![]));
            assert!(context.visible_tiles().is_empty());
        }

        #[test]
        fn player_id_is_passed_to_context() {
            let context = game_context_from_decoded_observation(&decoded(2, None, vec![]));
            assert_eq!(context.player_id(), Some(2));
        }

        #[test]
        fn oya_is_passed_to_context() {
            let mut d = decoded(0, None, vec![]);
            d.oya = 3;
            let context = game_context_from_decoded_observation(&d);
            assert_eq!(context.oya(), Some(3));
        }

        #[test]
        fn discards_are_passed_to_context() {
            let discards = [
                vec![TileId::new(0).unwrap()],
                vec![TileId::new(16).unwrap()],
                vec![],
                vec![TileId::new(104).unwrap()],
            ];
            let mut d = decoded(0, None, vec![]);
            d.discards = discards.clone();
            let context = game_context_from_decoded_observation(&d);
            assert_eq!(context.discards(), &discards);
        }

        #[test]
        fn reached_is_passed_to_context() {
            let mut d = decoded(0, None, vec![]);
            d.reached = [false, true, false, true];
            let context = game_context_from_decoded_observation(&d);
            assert_eq!(context.reached(), &[false, true, false, true]);
        }

        #[test]
        fn decoded_meld_tiles_reach_context_visible_tiles() {
            let mut melds: [Vec<ObservationMeld>; 4] = Default::default();
            melds[1] = vec![meld(MeldType::Pon, vec![2, 0, 1], Some(2))];
            let decoded = ObservationPayload::new(fixture_base64_with_melds(
                0,
                None,
                vec![],
                vec![],
                Default::default(),
                melds,
            ))
            .decode_4p()
            .unwrap();
            let context = game_context_from_decoded_observation(&decoded);
            let count = context
                .visible_tiles()
                .iter()
                .filter(|&&tile| tile == TileId::new(0).unwrap())
                .count();
            assert_eq!(count, 2);
        }

        #[test]
        fn shanten_agent_uses_visible_tiles_from_decoded_observation() {
            use bot_core::{Agent, LegalAction, ShantenAgent};

            let hand_values = [0u8, 4, 8, 12, 17, 20, 24, 28, 32, 48, 53, 56, 36];
            let hand: Vec<TileId> = hand_values
                .iter()
                .map(|&value| TileId::new(value).unwrap())
                .collect();
            let mut visible_tiles = hand.clone();
            visible_tiles.extend(
                [68u8, 69, 70, 71]
                    .iter()
                    .map(|&value| TileId::new(value).unwrap()),
            );
            let mut d = decoded(0, TileId::new(68), hand);
            d.visible_tiles = visible_tiles;
            let context = game_context_from_decoded_observation(&d);
            assert!(!context.visible_tiles().is_empty());

            let actions: Vec<LegalAction> = hand_values
                .iter()
                .chain(std::iter::once(&68u8))
                .map(|&value| LegalAction::Dahai {
                    tile: TileId::new(value).unwrap(),
                })
                .collect();

            let mut agent = ShantenAgent;
            let LegalAction::Dahai { tile } = agent.act(&context, &actions) else {
                panic!("expected dahai");
            };
            assert_eq!(tile.tile_type().to_mjai_string(), "9p");
        }
    }
}
