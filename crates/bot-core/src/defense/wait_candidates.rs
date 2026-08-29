use crate::context::GameContext;
use bot_logic::TileType;

use super::hard_safety::is_discarded_by_player;
use super::visible_count_of;

/// 見え牌を除き、未知領域に残り得る対象牌の物理牌枚数を返す。
///
/// 赤5は通常5と同じ `TileType` として数える。異常入力で5枚以上見えていても0で飽和する。
pub fn remaining_tile_copies(tile: TileType, context: &GameContext) -> u8 {
    4_u8.saturating_sub(visible_count_of(tile, context))
}

/// 対象牌を Shanpon 用の対子として保持し得る、未知の物理牌組み合わせ数を返す。
///
/// `remaining_tile_copies` から2枚を選ぶ組み合わせ (`C(remaining, 2)`) であり、相手の
/// 手牌分布などを考慮した放銃確率ではない。
pub fn shanpon_remaining_combinations(tile: TileType, context: &GameContext) -> u8 {
    let remaining = remaining_tile_copies(tile, context);
    remaining * remaining.saturating_sub(1) / 2
}

/// 指定 player の河も考慮した Shanpon 用の未知牌組み合わせ数を返す。
///
/// 対象牌そのものが player 自身の河にあればロンできないため0を返す。リーチ後に通った牌など、
/// リーチ固有の hard safety はここでは扱わない。
pub fn shanpon_remaining_combinations_for_player(
    tile: TileType,
    player: usize,
    context: &GameContext,
) -> u8 {
    if is_discarded_by_player(tile, player, context) {
        0
    } else {
        shanpon_remaining_combinations(tile, context)
    }
}

/// 対象牌を Tanki で保持し得る、未知の物理牌候補数を返す。
///
/// 値は `remaining_tile_copies` そのものであり、相手の手牌分布などを考慮した放銃確率ではない。
pub fn tanki_remaining_candidates(tile: TileType, context: &GameContext) -> u8 {
    remaining_tile_copies(tile, context)
}

/// 指定 player の河も考慮した Tanki 用の未知牌候補数を返す。
///
/// 対象牌そのものが player 自身の河にあればロンできないため0を返す。リーチ後に通った牌など、
/// リーチ固有の hard safety はここでは扱わない。
pub fn tanki_remaining_candidates_for_player(
    tile: TileType,
    player: usize,
    context: &GameContext,
) -> u8 {
    if is_discarded_by_player(tile, player, context) {
        0
    } else {
        tanki_remaining_candidates(tile, context)
    }
}
