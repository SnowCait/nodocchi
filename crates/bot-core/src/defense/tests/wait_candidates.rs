use super::common::*;
use crate::defense::*;

fn five_pin_context(visible: usize) -> GameContext {
    visible_context((52..52 + visible as u8).map(tile).collect())
}

#[test]
fn remaining_tile_copies_cover_every_valid_visible_count() {
    let five_pin = tile_type("5p");
    for (visible, expected) in [(0, 4), (1, 3), (2, 2), (3, 1), (4, 0)] {
        assert_eq!(
            remaining_tile_copies(five_pin, &five_pin_context(visible)),
            expected
        );
    }
}

#[test]
fn remaining_tile_copies_saturate_for_more_than_four_visible() {
    let context = visible_context(vec![tile(52); 5]);
    assert_eq!(remaining_tile_copies(tile_type("5p"), &context), 0);
    assert_eq!(shanpon_remaining_combinations(tile_type("5p"), &context), 0);
    assert_eq!(tanki_remaining_candidates(tile_type("5p"), &context), 0);
}

#[test]
fn shanpon_remaining_combinations_choose_two_remaining_copies() {
    let five_pin = tile_type("5p");
    for (visible, expected) in [(0, 6), (1, 3), (2, 1), (3, 0), (4, 0)] {
        assert_eq!(
            shanpon_remaining_combinations(five_pin, &five_pin_context(visible)),
            expected
        );
    }
}

#[test]
fn tanki_remaining_candidates_equal_remaining_copies() {
    let five_pin = tile_type("5p");
    for (visible, expected) in [(0, 4), (1, 3), (2, 2), (3, 1), (4, 0)] {
        assert_eq!(
            tanki_remaining_candidates(five_pin, &five_pin_context(visible)),
            expected
        );
    }
}

#[test]
fn player_river_eliminates_shanpon_and_tanki_candidates() {
    let five_pin = tile_type("5p");
    let discarded_context = suited_context(
        vec![],
        [vec![], vec![discarded("5p")], vec![], vec![]],
        [false; 4],
    );
    assert_eq!(
        shanpon_remaining_combinations_for_player(five_pin, 1, &discarded_context),
        0
    );
    assert_eq!(
        tanki_remaining_candidates_for_player(five_pin, 1, &discarded_context),
        0
    );

    let other_discard_context = suited_context(
        vec![],
        [vec![], vec![discarded("4p")], vec![], vec![]],
        [false; 4],
    );
    assert_eq!(
        shanpon_remaining_combinations_for_player(five_pin, 1, &other_discard_context),
        6
    );
    assert_eq!(
        tanki_remaining_candidates_for_player(five_pin, 1, &other_discard_context),
        4
    );
}

#[test]
fn red_five_counts_as_the_same_tile_type() {
    let context = visible_context(vec![tile(16), tile(17)]);
    let five_man = tile_type("5m");
    assert_eq!(remaining_tile_copies(five_man, &context), 2);
    assert_eq!(shanpon_remaining_combinations(five_man, &context), 1);
    assert_eq!(tanki_remaining_candidates(five_man, &context), 2);
}

#[test]
fn honor_tiles_use_the_same_raw_candidate_helpers() {
    let context = visible_context(vec![tile(108), tile(109)]);
    let east = tile_type("E");
    assert_eq!(remaining_tile_copies(east, &context), 2);
    assert_eq!(shanpon_remaining_combinations(east, &context), 1);
    assert_eq!(tanki_remaining_candidates(east, &context), 2);
}
