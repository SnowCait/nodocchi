//! 牌種構成から決まる通常役が、置き換え前の実装 ([`super::reference`]) と完全に一致すること
//! を確かめる差分検証。
//!
//! 判定規則は reference 側にしか無く、ここは同じ完成手を両方へ渡して結果を比べるだけ。

use super::reference;
use crate::completed_hand_corpus;
use crate::tile::TileType;
use crate::yaku::{
    hand_tile_types, is_shousangen, single_suit, standard_meld_shapes, tile_composition_yaku,
    triplet_tile_types,
};

#[test]
fn the_hand_tile_types_are_the_tile_types_the_hand_holds() {
    for analysis in completed_hand_corpus::analyses() {
        let mut expected = reference::hand_tile_types(&analysis);
        expected.sort_unstable();
        expected.dedup();
        let actual: Vec<TileType> = hand_tile_types(analysis.tile_type_counts()).collect();
        assert_eq!(actual, expected);
    }
}

#[test]
fn the_tile_composition_yaku_match_the_reference() {
    for analysis in completed_hand_corpus::analyses() {
        let tiles = reference::hand_tile_types(&analysis);
        assert_eq!(
            tile_composition_yaku(analysis.tile_type_counts()),
            reference::tile_composition_yaku(&tiles),
        );
        assert_eq!(
            single_suit(analysis.tile_type_counts()),
            reference::single_suit(&tiles),
        );
    }
}

#[test]
fn the_shousangen_decision_matches_the_reference() {
    for analysis in completed_hand_corpus::analyses() {
        for standard in analysis.standard_decompositions() {
            let Some(melds) = standard_meld_shapes(standard, analysis.fixed_melds()) else {
                continue;
            };
            assert_eq!(
                is_shousangen(standard.pair(), &melds),
                reference::is_shousangen(standard.pair(), &melds),
            );
        }
    }
}

#[test]
fn the_triplet_tile_types_match_the_reference() {
    for analysis in completed_hand_corpus::analyses() {
        for standard in analysis.standard_decompositions() {
            let Some(melds) = standard_meld_shapes(standard, analysis.fixed_melds()) else {
                continue;
            };
            for predicate in [
                TileType::is_dragon as fn(TileType) -> bool,
                TileType::is_wind,
                TileType::is_honor,
                TileType::is_terminal,
            ] {
                let expected = reference::set_tiles(&melds, predicate);
                let actual = triplet_tile_types(&melds, predicate);
                assert_eq!(actual.len(), expected.len());
                for tile in TileType::all() {
                    assert_eq!(actual.contains(tile), expected.contains(&tile));
                }
            }
        }
    }
}
