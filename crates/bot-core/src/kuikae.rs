//! 鳴いた直後に切れない牌 (喰い替え) を求める pure rule。
//!
//! 戦術 policy ではなく合法手の制約なので、鳴き後の打牌をシミュレートする経路はこの rule を
//! 通してから打牌候補を比較する。
//!
//! # semantics
//!
//! RiichiLab が使う RiichiEnv の `kuikae_forbidden` (Tenhou / Mahjong Soul preset のどちらでも
//! 既定で有効) と同じ規則にする。
//!
//! ```text
//! Pon → 鳴いた牌と同じ牌種
//! Chi → 鳴いた牌と同じ牌種
//!       + 鳴いた牌が順子の端の場合、反対側へ1つ伸ばした牌種 (筋の喰い替え)
//! Kan → 禁止牌なし
//! ```
//!
//! 順子の中の牌を鳴いた場合 (嵌張の Chi) は、伸ばした先の牌種を禁止しない。伸ばした先が同じ
//! スートに無い場合も禁止牌は増えない。
//!
//! # 牌種単位
//!
//! 禁止は牌種単位で、赤5と黒5の一方だけを禁止することはない。実際に切る物理牌の赤黒 preference
//! やドラの数え方は打牌評価側の既存 semantics のままで、この rule は関与しない。

use bot_logic::{Meld, MeldKind, MeldShape, TileType};

/// 鳴いた直後に切れない牌種を返す。
///
/// `meld` は今回鳴いて成立した副露そのもの。鳴いた牌 (`Meld::called_tile`) を持たない副露と
/// Kan では禁止牌が無いため空になる。牌の構成が面子として不正な Chi では、伸ばした先を推測せず
/// 鳴いた牌だけを禁止する。
pub fn forbidden_discards_after_call(meld: &Meld) -> Vec<TileType> {
    let Some(called_tile) = meld.called_tile() else {
        return Vec::new();
    };
    let called = called_tile.tile_type();

    match meld.kind() {
        MeldKind::Pon => vec![called],
        MeldKind::Chi => {
            let mut forbidden = vec![called];
            forbidden.extend(chi_extended_run_tile(meld, called));
            forbidden
        }
        MeldKind::Daiminkan | MeldKind::Ankan | MeldKind::Kakan => Vec::new(),
    }
}

// 鳴いた牌が順子の端にある Chi で、反対側へ1つ伸ばした牌種。
//
// 鳴いた牌が下端なら「鳴かずに持っていた2枚 + この牌種」で別の順子が作れるため、その牌種を
// 切ると喰い替えになる。上端の場合も向きが反対になるだけで同じ。中の牌を鳴いた場合と、伸ばした
// 先が同じスートに無い場合は禁止牌が増えない。
fn chi_extended_run_tile(meld: &Meld, called: TileType) -> Option<TileType> {
    let MeldShape::Sequence { start } = meld.shape()? else {
        return None;
    };
    let [low, _, high] = start.sequence()?;

    if called == low {
        high.next_in_suit()
    } else if called == high {
        low.previous_in_suit()
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use bot_logic::TileId;

    fn tile_type(s: &str) -> TileType {
        TileType::from_mjai_type_str(s).unwrap()
    }

    fn tile_types(strings: &[&str]) -> Vec<TileType> {
        strings.iter().map(|s| tile_type(s)).collect()
    }

    // 牌種と赤フラグから物理牌を1枚選ぶ。同じ牌種を複数回使ってもよい。
    fn tile(s: &str) -> TileId {
        let red = s.ends_with('r');
        let tile_type = tile_type(s.trim_end_matches('r'));
        TileId::copies(tile_type)
            .find(|tile| tile.is_red() == red)
            .unwrap()
    }

    fn meld(kind: MeldKind, called: &str, consumed: &[&str]) -> Meld {
        let called_tile = tile(called);
        let mut tiles = vec![called_tile];
        tiles.extend(consumed.iter().map(|s| tile(s)));
        Meld::new(kind, tiles, Some(called_tile))
    }

    #[test]
    fn pon_forbids_the_called_tile_type() {
        assert_eq!(
            forbidden_discards_after_call(&meld(MeldKind::Pon, "P", &["P", "P"])),
            tile_types(&["P"])
        );
        assert_eq!(
            forbidden_discards_after_call(&meld(MeldKind::Pon, "3s", &["3s", "3s"])),
            tile_types(&["3s"])
        );
    }

    #[test]
    fn a_pon_of_a_five_forbids_the_tile_type_including_the_red_five() {
        // 黒5を鳴いても赤5を鳴いても、禁止されるのは牌種そのもの1つだけ。
        for called in ["5s", "5sr"] {
            assert_eq!(
                forbidden_discards_after_call(&meld(MeldKind::Pon, called, &["5s", "5s"])),
                tile_types(&["5s"]),
                "{called}"
            );
        }
    }

    #[test]
    fn a_lower_chi_forbids_the_called_tile_and_the_upper_flank() {
        // 3s を鳴いて 345s。4s5s は元から持っていたので、6s を切ると 456s への喰い替えになる。
        assert_eq!(
            forbidden_discards_after_call(&meld(MeldKind::Chi, "3s", &["4s", "5s"])),
            tile_types(&["3s", "6s"])
        );
        assert_eq!(
            forbidden_discards_after_call(&meld(MeldKind::Chi, "1m", &["2m", "3m"])),
            tile_types(&["1m", "4m"])
        );
    }

    #[test]
    fn an_upper_chi_forbids_the_called_tile_and_the_lower_flank() {
        // 5s を鳴いて 345s。3s4s は元から持っていたので、2s を切ると 234s への喰い替えになる。
        assert_eq!(
            forbidden_discards_after_call(&meld(MeldKind::Chi, "5s", &["3s", "4s"])),
            tile_types(&["5s", "2s"])
        );
        assert_eq!(
            forbidden_discards_after_call(&meld(MeldKind::Chi, "9p", &["7p", "8p"])),
            tile_types(&["9p", "6p"])
        );
    }

    #[test]
    fn a_middle_chi_forbids_only_the_called_tile() {
        assert_eq!(
            forbidden_discards_after_call(&meld(MeldKind::Chi, "4s", &["3s", "5s"])),
            tile_types(&["4s"])
        );
    }

    #[test]
    fn a_flank_outside_the_suit_does_not_add_a_forbidden_tile() {
        // 7s を鳴いた 789s の上端は 10s にあたるので存在しない。下端も同じ。
        assert_eq!(
            forbidden_discards_after_call(&meld(MeldKind::Chi, "7s", &["8s", "9s"])),
            tile_types(&["7s"])
        );
        assert_eq!(
            forbidden_discards_after_call(&meld(MeldKind::Chi, "3s", &["1s", "2s"])),
            tile_types(&["3s"])
        );
    }

    #[test]
    fn a_chi_flank_never_crosses_a_suit_boundary() {
        // 9m の次は 1p ではない。スートをまたいだ牌種を禁止しない。
        assert_eq!(
            forbidden_discards_after_call(&meld(MeldKind::Chi, "7m", &["8m", "9m"])),
            tile_types(&["7m"])
        );
        assert_eq!(
            forbidden_discards_after_call(&meld(MeldKind::Chi, "3p", &["1p", "2p"])),
            tile_types(&["3p"])
        );
    }

    #[test]
    fn kans_have_no_forbidden_discard() {
        for kind in [MeldKind::Daiminkan, MeldKind::Kakan] {
            assert!(
                forbidden_discards_after_call(&meld(kind, "P", &["P", "P", "P"])).is_empty(),
                "{kind:?}"
            );
        }
        let ankan = Meld::new(
            MeldKind::Ankan,
            vec![tile("1m"), tile("1m"), tile("1m"), tile("1m")],
            None,
        );
        assert!(forbidden_discards_after_call(&ankan).is_empty());
    }

    #[test]
    fn a_malformed_chi_only_forbids_the_called_tile() {
        // 面子として不正な Chi では、伸ばした先を推測しない。
        assert_eq!(
            forbidden_discards_after_call(&meld(MeldKind::Chi, "3s", &["5s", "7s"])),
            tile_types(&["3s"])
        );
    }
}
