use crate::context::GameContext;
use bot_logic::TileType;

use super::visible_count_of;

// 数牌の順子待ち経路ごとの壁 / ワンチャンス分類。見えているほど当たり筋が減る。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum WallRank {
    NoWall,
    OneChance,
    NoChance,
}

// 対象牌を和了牌とする順子待ち経路1本の壁分類。
//
// - Blocked:   経路を構成するどちらかの牌が4枚以上見えている(その順子待ちは残らない)。
// - OneChance: Blocked ではなく、どちらかの牌が3枚見えている。
// - Open:      それ以外。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SequenceRouteRank {
    Open,
    OneChance,
    Blocked,
}

// 対象牌 n を和了牌とする、同一色内の順子待ち経路を列挙する。
//
// - n >= 3: [n-2, n-1]
// - n <= 7: [n+1, n+2]
//
// 字牌は対象外で空。1〜9 の範囲外へは進まないので、端牌は経路が1本だけになる。
// 対象牌自身の見え枚数は経路に含めない。
pub(super) fn sequence_wait_routes(tile: TileType) -> Vec<[TileType; 2]> {
    let Some(number) = tile.number() else {
        return Vec::new();
    };
    // 同一色の 1 の TileType を基準にした相対計算。suit をまたがない。
    let suit_base = tile.raw() - (number - 1);
    let in_suit = |value: u8| TileType::new(suit_base + value - 1).unwrap();

    let mut routes = Vec::new();
    if number >= 3 {
        routes.push([in_suit(number - 2), in_suit(number - 1)]);
    }
    if number <= 7 {
        routes.push([in_suit(number + 1), in_suit(number + 2)]);
    }
    routes
}

// 順子待ち経路1本の壁分類。経路を構成する牌の見え枚数だけを使う。
fn sequence_route_rank(route: [TileType; 2], context: &GameContext) -> SequenceRouteRank {
    let max_visible = route
        .iter()
        .map(|&tile| visible_count_of(tile, context))
        .max()
        .unwrap_or(0);
    if max_visible >= 4 {
        SequenceRouteRank::Blocked
    } else if max_visible >= 3 {
        SequenceRouteRank::OneChance
    } else {
        SequenceRouteRank::Open
    }
}

/// 数牌の順子待ちに関する壁 / ワンチャンスを保守的に分類する。
///
/// 対象牌 `n` を和了牌とする順子待ち経路(`[n-2, n-1]` / `[n+1, n+2]`)を列挙し、各経路を
/// 構成する牌の見え枚数から集約する。**対象牌 `n` 自身の見え枚数は壁判定に使わない**。
/// 自分が `n` を暗刻で持っているだけで壁扱いされる誤分類を避けるためである。
///
/// 集約規則:
///
/// - すべての有効経路が Blocked: `NoChance`
/// - すべての有効経路が Blocked または OneChance で、少なくとも1経路が OneChance: `OneChance`
/// - Open の経路が1つでもある: `NoWall`
///
/// これは順子待ちに関する限定的な安全度でしかなく、単騎・双碰・嵌張などへの絶対的な安全は
/// 意味しない。字牌は対象外で常に `NoWall`。
pub fn wall_rank(tile: TileType, context: &GameContext) -> WallRank {
    let routes = sequence_wait_routes(tile);
    if routes.is_empty() {
        return WallRank::NoWall;
    }

    let mut has_open = false;
    let mut has_one_chance = false;
    for route in routes {
        match sequence_route_rank(route, context) {
            SequenceRouteRank::Open => has_open = true,
            SequenceRouteRank::OneChance => has_one_chance = true,
            SequenceRouteRank::Blocked => {}
        }
    }

    if has_open {
        WallRank::NoWall
    } else if has_one_chance {
        WallRank::OneChance
    } else {
        WallRank::NoChance
    }
}

// 順子待ち経路がワンチャンスの数牌か判定する。詳細は `wall_rank`。
pub fn is_one_chance(tile: TileType, context: &GameContext) -> bool {
    wall_rank(tile, context) == WallRank::OneChance
}

// 順子待ち経路がノーチャンスの数牌か判定する。詳細は `wall_rank`。
pub fn is_no_chance(tile: TileType, context: &GameContext) -> bool {
    wall_rank(tile, context) == WallRank::NoChance
}

// 数牌のみを TileType::all() の順序で壁分類と共に返す。NoWall も含める。
pub fn wall_tile_types_by_rank(context: &GameContext) -> Vec<(TileType, WallRank)> {
    TileType::all()
        .filter(|tile| !tile.is_honor())
        .map(|tile| (tile, wall_rank(tile, context)))
        .collect()
}
