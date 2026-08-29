use super::common::*;
use crate::defense::wall::sequence_wait_routes;
use crate::defense::*;
use bot_logic::TileType;

// 6p(tile 56-59)を対象に、経路構成牌 4p/5p/7p/8p の見え枚数で壁を作る helper。
// 4p: 48-51, 5p: 52-55, 6p: 56-59, 7p: 60-63, 8p: 64-67。

#[test]
fn wall_rank_no_wall_when_own_count_high_but_routes_open() {
    // 対象牌自身(6p)を3枚見えていても、経路 4p/5p/7p/8p に壁がなければ NoWall。
    // 対象牌自身の見え枚数を壁判定に使わないことの回帰テスト。
    let six_pin = tile(56).tile_type();
    assert_eq!(
        wall_rank(
            six_pin,
            &visible_context(vec![tile(56), tile(57), tile(58)])
        ),
        WallRank::NoWall
    );
}

#[test]
fn wall_rank_no_wall_when_own_count_four_but_routes_open() {
    // 対象牌自身(6p)を4枚見えていても、経路に壁がなければ NoWall。人工的な pure test。
    let six_pin = tile(56).tile_type();
    assert_eq!(
        wall_rank(
            six_pin,
            &visible_context(vec![tile(56), tile(57), tile(58), tile(59)])
        ),
        WallRank::NoWall
    );
}

#[test]
fn wall_rank_no_chance_when_both_routes_blocked() {
    // 5p を4枚・7p を4枚見え。経路 [4p,5p] と [7p,8p] が両方 Blocked なので NoChance。
    let six_pin = tile(56).tile_type();
    let visible = vec![
        tile(52),
        tile(53),
        tile(54),
        tile(55),
        tile(60),
        tile(61),
        tile(62),
        tile(63),
    ];
    assert_eq!(
        wall_rank(six_pin, &visible_context(visible)),
        WallRank::NoChance
    );
}

#[test]
fn wall_rank_no_wall_when_one_route_blocked_and_other_open() {
    // 5p を4枚見え(経路 [4p,5p] は Blocked)だが、7p/8p は見えず経路 [7p,8p] は Open。NoWall。
    let six_pin = tile(56).tile_type();
    let visible = vec![tile(52), tile(53), tile(54), tile(55)];
    assert_eq!(
        wall_rank(six_pin, &visible_context(visible)),
        WallRank::NoWall
    );
}

#[test]
fn wall_rank_one_chance_when_blocked_and_one_chance() {
    // 5p を4枚見え(Blocked)、7p を3枚見え(OneChance)。Open が無く OneChance が残るので OneChance。
    let six_pin = tile(56).tile_type();
    let visible = vec![
        tile(52),
        tile(53),
        tile(54),
        tile(55),
        tile(60),
        tile(61),
        tile(62),
    ];
    assert_eq!(
        wall_rank(six_pin, &visible_context(visible)),
        WallRank::OneChance
    );
}

#[test]
fn wall_rank_no_wall_when_one_chance_and_open() {
    // 5p を3枚見え(経路 [4p,5p] は OneChance)、7p/8p は見えず経路 [7p,8p] は Open。NoWall。
    let six_pin = tile(56).tile_type();
    let visible = vec![tile(52), tile(53), tile(54)];
    assert_eq!(
        wall_rank(six_pin, &visible_context(visible)),
        WallRank::NoWall
    );
}

#[test]
fn wall_rank_no_wall_for_honor() {
    // 字牌は経路を持たないので、何枚見えていても NoWall。
    let east = tile(108).tile_type();
    assert_eq!(
        wall_rank(
            east,
            &visible_context(vec![tile(108), tile(109), tile(110), tile(111)])
        ),
        WallRank::NoWall
    );
}

#[test]
fn wall_rank_terminal_uses_only_in_range_route() {
    // 1p は経路 [2p,3p] のみ、9p は経路 [7p,8p] のみを評価し、範囲外へは進まない。
    // 1p: 2p(40-43)を4枚見えで NoChance。9p: 8p(64-67)を4枚見えで NoChance。
    let one_pin = tile(36).tile_type();
    let nine_pin = tile(68).tile_type();
    assert_eq!(
        wall_rank(
            one_pin,
            &visible_context(vec![tile(40), tile(41), tile(42), tile(43)])
        ),
        WallRank::NoChance
    );
    assert_eq!(
        wall_rank(
            nine_pin,
            &visible_context(vec![tile(64), tile(65), tile(66), tile(67)])
        ),
        WallRank::NoChance
    );
}

#[test]
fn sequence_wait_routes_stay_in_suit_and_in_range() {
    // 端牌は経路1本、中張牌は2本。suit をまたがず 1〜9 の範囲内に収まる。
    let one_pin = tile(36).tile_type();
    assert_eq!(sequence_wait_routes(one_pin).len(), 1);
    let nine_pin = tile(68).tile_type();
    assert_eq!(sequence_wait_routes(nine_pin).len(), 1);
    let five_pin = tile(52).tile_type();
    assert_eq!(sequence_wait_routes(five_pin).len(), 2);
    // 字牌は経路なし。
    assert!(sequence_wait_routes(tile(108).tile_type()).is_empty());
}

#[test]
fn wall_rank_counts_red_five_in_route_as_same_type() {
    // 経路構成牌 5p の壁を赤5p(tile 52)込みの4枚で作る。赤5も通常5と同じ TileType。
    // 6p の経路 [4p,5p] が Blocked、[7p,8p] は Open なので NoWall。
    let six_pin = tile(56).tile_type();
    let visible = vec![tile(52), tile(53), tile(54), tile(55)];
    assert_eq!(
        visible_count_of(tile(53).tile_type(), &visible_context(visible.clone())),
        4
    );
    assert_eq!(
        wall_rank(six_pin, &visible_context(visible)),
        WallRank::NoWall
    );
}

#[test]
fn is_one_chance_reflects_route_one_chance() {
    // 6p の経路 [4p,5p] を Blocked、[7p,8p] を OneChance にすると is_one_chance == true。
    let six_pin = tile(56).tile_type();
    let one_chance = vec![
        tile(52),
        tile(53),
        tile(54),
        tile(55),
        tile(60),
        tile(61),
        tile(62),
    ];
    assert!(is_one_chance(six_pin, &visible_context(one_chance)));
    // 経路が Open のままなら false。
    assert!(!is_one_chance(six_pin, &visible_context(vec![])));
    // 字牌は経路を持たないので false。
    let east = tile(108).tile_type();
    assert!(!is_one_chance(
        east,
        &visible_context(vec![tile(108), tile(109), tile(110)])
    ));
}

#[test]
fn is_no_chance_reflects_route_blocked() {
    // 6p の両経路を Blocked にすると is_no_chance == true。片方 Open なら false。
    let six_pin = tile(56).tile_type();
    let no_chance = vec![
        tile(52),
        tile(53),
        tile(54),
        tile(55),
        tile(60),
        tile(61),
        tile(62),
        tile(63),
    ];
    assert!(is_no_chance(six_pin, &visible_context(no_chance)));
    assert!(!is_no_chance(
        six_pin,
        &visible_context(vec![tile(52), tile(53), tile(54), tile(55)])
    ));
    // 字牌は4枚見えでも false。
    let east = tile(108).tile_type();
    assert!(!is_no_chance(
        east,
        &visible_context(vec![tile(108), tile(109), tile(110), tile(111)])
    ));
}

#[test]
fn wall_tile_types_by_rank_excludes_honors() {
    let context = visible_context(vec![]);
    let ranked = wall_tile_types_by_rank(&context);
    assert!(ranked.iter().all(|(tile, _)| !tile.is_honor()));
}

#[test]
fn wall_tile_types_by_rank_returns_number_tiles_in_all_order() {
    let context = visible_context(vec![]);
    let ranked = wall_tile_types_by_rank(&context);
    let expected: Vec<(TileType, WallRank)> = TileType::all()
        .filter(|tile| !tile.is_honor())
        .map(|tile| (tile, WallRank::NoWall))
        .collect();
    assert_eq!(ranked, expected);
    // 数牌は27種。
    assert_eq!(ranked.len(), 27);
}

#[test]
fn wall_tile_types_by_rank_includes_no_wall_entries() {
    // 2m を4枚見え。経路 [2m,3m] が Blocked になる 1m だけ NoChance、他は NoWall。
    let context = visible_context(vec![tile(4), tile(5), tile(6), tile(7)]);
    let ranked = wall_tile_types_by_rank(&context);
    let one_man = tile(0).tile_type();
    assert_eq!(
        ranked
            .iter()
            .find(|(tile, _)| *tile == one_man)
            .map(|(_, rank)| *rank),
        Some(WallRank::NoChance)
    );
    assert!(
        ranked
            .iter()
            .any(|(tile, rank)| *tile != one_man && *rank == WallRank::NoWall)
    );
    assert_eq!(ranked.len(), 27);
}
