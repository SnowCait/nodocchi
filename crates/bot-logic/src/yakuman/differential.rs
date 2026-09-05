//! 牌種構成から決まる役満が、置き換え前の実装 ([`super::reference`]) と完全に一致すること
//! を確かめる差分検証。
//!
//! 判定規則は reference 側にしか無く、ここは同じ完成手を両方へ渡して結果を比べるだけ。

use super::reference;
use crate::completed_hand_corpus;
use crate::tile::TileType;
use crate::yaku::{reference::hand_tile_types, standard_meld_shapes};
use crate::yakuman::{
    DAISANGEN_DRAGON_SET_COUNT, dragon_set_tiles, is_chuuren_poutou, tile_composition_yakuman,
    wind_yakuman,
};

#[test]
fn the_tile_composition_yakuman_match_the_reference() {
    for analysis in completed_hand_corpus::analyses() {
        let tiles = hand_tile_types(&analysis);
        assert_eq!(
            tile_composition_yakuman(analysis.tile_type_counts(), analysis.fixed_melds()),
            reference::tile_composition_yakuman(&tiles, analysis.fixed_melds()),
        );
        assert_eq!(
            is_chuuren_poutou(analysis.tile_type_counts(), analysis.fixed_melds()),
            reference::is_chuuren_poutou(&tiles, analysis.fixed_melds()),
        );
    }
}

#[test]
fn the_dragon_and_wind_sets_match_the_reference() {
    for analysis in completed_hand_corpus::analyses() {
        for standard in analysis.standard_decompositions() {
            let Some(melds) = standard_meld_shapes(standard, analysis.fixed_melds()) else {
                continue;
            };
            let dragons = dragon_set_tiles(&melds);
            let expected_dragons = crate::yaku::reference::set_tiles(&melds, TileType::is_dragon);
            assert_eq!(dragons.len(), expected_dragons.len());
            assert_eq!(
                dragons.len() == DAISANGEN_DRAGON_SET_COUNT,
                expected_dragons.len() == DAISANGEN_DRAGON_SET_COUNT,
            );
            assert_eq!(
                wind_yakuman(standard.pair(), &melds),
                reference::wind_yakuman(standard.pair(), &melds),
            );
        }
    }
}
