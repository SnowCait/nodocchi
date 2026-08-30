use crate::context::GameContext;
use crate::meld::{Meld, MeldKind};
use bot_logic::{TileId, TileType};

pub(super) fn tile(value: u8) -> TileId {
    TileId::new(value).unwrap()
}

pub(super) fn table_state_context(
    player_id: Option<u8>,
    oya: Option<u8>,
    discards: [Vec<TileId>; 4],
    reached: [bool; 4],
) -> GameContext {
    GameContext::from_parts_with_table_state(
        None,
        vec![],
        vec![],
        None,
        None,
        Vec::new(),
        player_id,
        oya,
        discards,
        reached,
    )
}

pub(super) fn post_reach_context(
    player_id: Option<u8>,
    discards: [Vec<TileId>; 4],
    reached: [bool; 4],
    post_reach_passed: [Vec<TileType>; 4],
) -> GameContext {
    table_state_context(player_id, None, discards, reached)
        .with_post_reach_passed_tiles(post_reach_passed)
}

pub(super) fn tile_type(mjai: &str) -> TileType {
    TileType::from_mjai_type_str(mjai).unwrap()
}

pub(super) fn discarded(mjai: &str) -> TileId {
    TileId::new(tile_type(mjai).raw() * 4).unwrap()
}

pub(super) fn held(mjai: &str) -> TileId {
    TileId::new(tile_type(mjai).raw() * 4 + 1).unwrap()
}

pub(super) fn visible_context(visible_tiles: Vec<TileId>) -> GameContext {
    GameContext::from_parts_with_visible_tiles(None, vec![], vec![], None, None, visible_tiles)
}

pub(super) fn honor(value: u8) -> TileType {
    TileType::new(value).unwrap()
}

pub(super) const EAST: u8 = 27;
pub(super) const SOUTH: u8 = 28;
pub(super) const WEST: u8 = 29;
pub(super) const NORTH: u8 = 30;
pub(super) const HAKU: u8 = 31;
pub(super) const HATSU: u8 = 32;
pub(super) const CHUN: u8 = 33;

pub(super) fn honor_value_context(
    round_wind: Option<TileType>,
    oya: Option<u8>,
    reached: [bool; 4],
    discards: [Vec<TileId>; 4],
    visible_tiles: Vec<TileId>,
) -> GameContext {
    GameContext::from_parts_with_table_state(
        None,
        vec![],
        vec![],
        round_wind,
        None,
        visible_tiles,
        Some(0),
        oya,
        discards,
        reached,
    )
}

pub(super) fn single_reacher_honor_context(oya: u8) -> GameContext {
    honor_value_context(
        Some(honor(EAST)),
        Some(oya),
        [false, true, false, false],
        Default::default(),
        vec![],
    )
}

pub(super) fn suited_context(
    visible_tiles: Vec<TileId>,
    discards: [Vec<TileId>; 4],
    reached: [bool; 4],
) -> GameContext {
    GameContext::from_parts_with_table_state(
        None,
        vec![],
        vec![],
        None,
        None,
        visible_tiles,
        Some(0),
        None,
        discards,
        reached,
    )
}

// exact model を一人だけ unavailable にして、複数リーチの legacy fallback 自体を検証する。
pub(super) fn legacy_suited_context(
    visible_tiles: Vec<TileId>,
    discards: [Vec<TileId>; 4],
    reached: [bool; 4],
) -> GameContext {
    let mut melds: [Vec<Meld>; 4] = Default::default();
    let unavailable_player = reached
        .iter()
        .enumerate()
        .find(|(player, reached)| *player != 0 && **reached)
        .map(|(player, _)| player)
        .expect("at least one reached opponent");
    let pon_tiles: Vec<_> = TileId::copies(tile_type("9p")).take(3).collect();
    melds[unavailable_player] = vec![Meld::new(
        MeldKind::Pon,
        pon_tiles.clone(),
        Some(pon_tiles[0]),
    )];
    GameContext::from_parts_with_melds(
        None,
        vec![],
        vec![],
        None,
        None,
        visible_tiles,
        Some(0),
        None,
        discards,
        reached,
        melds,
    )
}

// 二人のリーチ者について、2m は全員にスジ・1m は一人にだけスジになる状況を作る。
// player1 の河: 4m(1m スジ根拠) と 5m(2m スジ根拠)。player2 の河: 5m のみ。
pub(super) fn all_reached_partial_suji_context(visible_tiles: Vec<TileId>) -> GameContext {
    legacy_suited_context(
        visible_tiles,
        [vec![], vec![tile(12), tile(16)], vec![tile(17)], vec![]],
        [false, true, true, false],
    )
}

// 片スジ回帰局面。リーチ者(1)の河は 1p(36) と 4s(84)。
// 手牌 444p147m258p123s7s + ツモ 9m を visible にしても壁は両方 NoWall なので、
// 4p(1p だけスジ)と 7s(4s でスジ)の差はスジ分類だけで決まる。
pub(super) fn half_suji_regression_context() -> GameContext {
    let hand = vec![
        tile(48),
        tile(49),
        tile(50),
        tile(0),
        tile(12),
        tile(24),
        tile(40),
        tile(52),
        tile(64),
        tile(72),
        tile(76),
        tile(80),
        tile(96),
        tile(32),
    ];
    suited_context(
        hand,
        [vec![], vec![tile(36), tile(84)], vec![], vec![]],
        [false, true, false, false],
    )
}

pub(super) fn multiple_reach_half_suji_regression_context() -> GameContext {
    let hand = vec![
        tile(48),
        tile(49),
        tile(50),
        tile(0),
        tile(12),
        tile(24),
        tile(40),
        tile(52),
        tile(64),
        tile(72),
        tile(76),
        tile(80),
        tile(96),
        tile(32),
    ];
    legacy_suited_context(
        hand,
        [
            vec![],
            vec![tile(36), tile(84)],
            vec![tile(37), tile(85)],
            vec![],
        ],
        [false, true, true, false],
    )
}
