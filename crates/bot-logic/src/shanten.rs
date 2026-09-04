use crate::tile_counts::TileCounts;

mod decomposition;
#[cfg(test)]
mod differential;
#[cfg(test)]
mod reference;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Shanten {
    pub standard: i8,
    pub chiitoitsu: i8,
    pub kokushi: i8,
}

impl Shanten {
    pub fn min(self) -> i8 {
        self.standard.min(self.chiitoitsu).min(self.kokushi)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct FixedMeldCount(u8);

impl FixedMeldCount {
    pub const MAX: u8 = 4;
    pub const NONE: Self = Self(0);

    pub fn new(value: u8) -> Option<Self> {
        (value <= Self::MAX).then_some(Self(value))
    }

    pub fn get(self) -> u8 {
        self.0
    }

    pub fn has_melds(self) -> bool {
        self.0 > 0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EffectiveShanten {
    Concealed(Shanten),
    Melded { standard: i8 },
}

impl EffectiveShanten {
    pub fn min(self) -> i8 {
        match self {
            Self::Concealed(shanten) => shanten.min(),
            Self::Melded { standard } => standard,
        }
    }

    pub fn standard(self) -> i8 {
        match self {
            Self::Concealed(shanten) => shanten.standard,
            Self::Melded { standard } => standard,
        }
    }

    pub fn concealed(self) -> Option<Shanten> {
        match self {
            Self::Concealed(shanten) => Some(shanten),
            Self::Melded { .. } => None,
        }
    }
}

/// 向聴数表現から最小向聴数だけを取り出す最小の抽象。
///
/// [`Acceptance`](crate::acceptance::Acceptance) のように向聴数表現を型引数に持つ構造を、門前形
/// ([`Shanten`]) と副露形 ([`EffectiveShanten`]) のどちらでも同じ helper で扱うために使う。
/// 新しい向聴計算は持たず、既存の `min()` をそのまま返す。
pub trait MinShanten: Copy {
    fn min_shanten(self) -> i8;
}

impl MinShanten for Shanten {
    fn min_shanten(self) -> i8 {
        self.min()
    }
}

impl MinShanten for EffectiveShanten {
    fn min_shanten(self) -> i8 {
        self.min()
    }
}

pub fn calculate_shanten(counts: &TileCounts) -> Shanten {
    Shanten {
        standard: standard_shanten(counts),
        chiitoitsu: chiitoitsu_shanten(counts),
        kokushi: kokushi_shanten(counts),
    }
}

pub fn calculate_shanten_with_fixed_melds(
    counts: &TileCounts,
    fixed_meld_count: FixedMeldCount,
) -> EffectiveShanten {
    if fixed_meld_count.has_melds() {
        EffectiveShanten::Melded {
            standard: standard_shanten_with_fixed_melds(counts, fixed_meld_count),
        }
    } else {
        EffectiveShanten::Concealed(calculate_shanten(counts))
    }
}

pub fn standard_shanten(counts: &TileCounts) -> i8 {
    standard_shanten_with_fixed_melds(counts, FixedMeldCount::NONE)
}

pub fn standard_shanten_with_fixed_melds(
    counts: &TileCounts,
    fixed_meld_count: FixedMeldCount,
) -> i8 {
    decomposition::standard_shanten(counts.as_array(), fixed_meld_count.get())
}

pub fn chiitoitsu_shanten(counts: &TileCounts) -> i8 {
    let pairs = counts
        .iter()
        .filter(|(_, count)| *count >= 2)
        .count()
        .min(7) as i8;

    let unique = counts
        .iter()
        .filter(|(_, count)| *count >= 1)
        .count()
        .min(7) as i8;

    6 - pairs + (7 - unique)
}

pub fn kokushi_shanten(counts: &TileCounts) -> i8 {
    let unique_yaochu = counts
        .iter()
        .filter(|(tile, count)| tile.is_yaochu() && *count >= 1)
        .count() as i8;

    let has_yaochu_pair = counts
        .iter()
        .any(|(tile, count)| tile.is_yaochu() && count >= 2);

    13 - unique_yaochu - i8::from(has_yaochu_pair)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tile::TileType;

    fn tile(s: &str) -> TileType {
        TileType::from_mjai_type_str(s).unwrap()
    }

    fn counts(strings: &[&str]) -> TileCounts {
        TileCounts::from_tile_types(strings.iter().map(|s| tile(s)))
    }

    #[test]
    fn min_shanten_matches_the_existing_min() {
        let shanten = Shanten {
            standard: 2,
            chiitoitsu: 1,
            kokushi: 5,
        };
        assert_eq!(shanten.min_shanten(), shanten.min());
        assert_eq!(
            EffectiveShanten::Concealed(shanten).min_shanten(),
            EffectiveShanten::Concealed(shanten).min()
        );
        assert_eq!(EffectiveShanten::Melded { standard: 3 }.min_shanten(), 3);
    }

    #[test]
    fn complete_hand_returns_minus_one() {
        let counts = counts(&[
            "1m", "2m", "3m", "4m", "5m", "6m", "7m", "8m", "9m", "1p", "2p", "3p", "5s", "5s",
        ]);
        assert_eq!(standard_shanten(&counts), -1);
    }

    #[test]
    fn tenpai_hand_returns_zero() {
        let counts = counts(&[
            "1m", "2m", "3m", "4m", "5m", "6m", "7m", "8m", "9m", "1p", "2p", "3p", "5s",
        ]);
        assert_eq!(standard_shanten(&counts), 0);
    }

    #[test]
    fn one_shanten_hand_returns_one() {
        let counts = counts(&[
            "1m", "2m", "3m", "4m", "5m", "6m", "7m", "8m", "9m", "1p", "5s", "E", "E",
        ]);
        assert_eq!(standard_shanten(&counts), 1);
    }

    #[test]
    fn pair_counts_as_partial_when_head_is_taken() {
        let counts = counts(&[
            "1m", "2m", "3m", "4m", "5m", "6m", "7m", "8m", "9m", "1p", "1p", "2s", "2s", "E",
        ]);
        assert_eq!(standard_shanten(&counts), 0);
    }

    #[test]
    fn complete_hand_with_triplet_and_sequences_returns_minus_one() {
        let counts = counts(&[
            "1m", "1m", "1m", "2m", "3m", "4m", "3p", "4p", "5p", "7s", "8s", "9s", "E", "E",
        ]);
        assert_eq!(standard_shanten(&counts), -1);
    }

    #[test]
    fn empty_hand_returns_eight() {
        assert_eq!(standard_shanten(&TileCounts::new()), 8);
    }

    #[test]
    fn reused_profile_tables_keep_the_same_shanten() {
        // 色ごとの要約表を呼び出し間で使い回しても、呼び出し順にかかわらず同じ向聴数を返す。
        let hands = [
            counts(&[
                "1m", "2m", "3m", "4m", "5m", "6m", "7m", "8m", "9m", "1p", "5s", "E", "E",
            ]),
            counts(&[
                "1m", "2m", "3m", "4m", "5m", "6m", "7m", "8m", "9m", "1p", "2p", "3p", "5s",
            ]),
            counts(&["1m", "3m", "5m", "7m", "9m", "E", "S", "W"]),
        ];

        let forward: Vec<_> = hands.iter().map(standard_shanten).collect();
        let backward: Vec<_> = hands.iter().rev().map(standard_shanten).collect();
        let melded: Vec<_> = hands
            .iter()
            .map(|counts| {
                standard_shanten_with_fixed_melds(counts, FixedMeldCount::new(1).unwrap())
            })
            .collect();

        assert_eq!(
            forward,
            backward.into_iter().rev().collect::<Vec<_>>(),
            "呼び出し順で結果が変わらない"
        );
        assert_eq!(
            forward,
            hands.iter().map(standard_shanten).collect::<Vec<_>>()
        );
        // 要約表は牌姿だけの表なので、副露済み面子数を足す段が門前の結果と混ざらない。
        assert_ne!(melded, forward);
        assert_eq!(
            melded,
            hands
                .iter()
                .map(|counts| standard_shanten_with_fixed_melds(
                    counts,
                    FixedMeldCount::new(1).unwrap()
                ))
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn isolated_tiles_only_returns_six() {
        let counts = counts(&["1m", "3m", "5m", "7m", "9m", "E", "S", "W"]);
        assert_eq!(standard_shanten(&counts), 6);
    }

    #[test]
    fn partial_does_not_cross_suits() {
        assert_eq!(standard_shanten(&counts(&["9m", "1p"])), 8);
        assert_eq!(standard_shanten(&counts(&["8m", "9m", "1p"])), 7);
    }

    #[test]
    fn sequence_does_not_cross_suits() {
        let counts = counts(&["8m", "9m", "1p", "E", "E"]);
        assert_eq!(standard_shanten(&counts), 6);
    }

    #[test]
    fn chiitoitsu_complete_hand_returns_minus_one() {
        let counts = counts(&[
            "1m", "1m", "2m", "2m", "3m", "3m", "4p", "4p", "5p", "5p", "6s", "6s", "E", "E",
        ]);
        assert_eq!(chiitoitsu_shanten(&counts), -1);
    }

    #[test]
    fn chiitoitsu_tenpai_hand_returns_zero() {
        let counts = counts(&[
            "1m", "1m", "2m", "2m", "3m", "3m", "4p", "4p", "5p", "5p", "6s", "6s", "E",
        ]);
        assert_eq!(chiitoitsu_shanten(&counts), 0);
    }

    #[test]
    fn chiitoitsu_one_shanten_hand_returns_one() {
        let counts = counts(&[
            "1m", "1m", "2m", "2m", "3m", "3m", "4p", "4p", "5p", "5p", "6s", "E",
        ]);
        assert_eq!(chiitoitsu_shanten(&counts), 1);
    }

    #[test]
    fn chiitoitsu_quad_counts_as_one_pair() {
        let counts = counts(&[
            "1m", "1m", "1m", "1m", "2m", "2m", "3m", "3m", "4p", "4p", "5p", "5p", "6s", "6s", "E",
        ]);
        assert_eq!(chiitoitsu_shanten(&counts), 0);
    }

    #[test]
    fn chiitoitsu_empty_hand_returns_thirteen() {
        assert_eq!(chiitoitsu_shanten(&TileCounts::new()), 13);
    }

    #[test]
    fn kokushi_complete_hand_returns_minus_one() {
        let counts = counts(&[
            "1m", "9m", "1p", "9p", "1s", "9s", "E", "S", "W", "N", "P", "F", "C", "1m",
        ]);
        assert_eq!(kokushi_shanten(&counts), -1);
    }

    #[test]
    fn kokushi_thirteen_wait_tenpai_returns_zero() {
        let counts = counts(&[
            "1m", "9m", "1p", "9p", "1s", "9s", "E", "S", "W", "N", "P", "F", "C",
        ]);
        assert_eq!(kokushi_shanten(&counts), 0);
    }

    #[test]
    fn kokushi_single_wait_tenpai_returns_zero() {
        let counts = counts(&[
            "1m", "9m", "1p", "9p", "1s", "9s", "E", "S", "W", "N", "P", "F", "1m",
        ]);
        assert_eq!(kokushi_shanten(&counts), 0);
    }

    #[test]
    fn kokushi_one_shanten_hand_returns_one() {
        let counts = counts(&[
            "1m", "9m", "1p", "9p", "1s", "9s", "E", "S", "W", "N", "P", "1m",
        ]);
        assert_eq!(kokushi_shanten(&counts), 1);
    }

    #[test]
    fn kokushi_middle_tile_does_not_count_as_yaochu() {
        let counts = counts(&[
            "1m", "9m", "1p", "9p", "1s", "9s", "E", "S", "W", "N", "P", "F", "5m",
        ]);
        assert_eq!(kokushi_shanten(&counts), 1);
    }

    #[test]
    fn kokushi_empty_hand_returns_thirteen() {
        assert_eq!(kokushi_shanten(&TileCounts::new()), 13);
    }

    #[test]
    fn shanten_min_returns_smallest_value() {
        let shanten = Shanten {
            standard: 2,
            chiitoitsu: 1,
            kokushi: 5,
        };
        assert_eq!(shanten.min(), 1);
    }

    #[test]
    fn shanten_min_returns_minus_one_when_included() {
        let shanten = Shanten {
            standard: 0,
            chiitoitsu: -1,
            kokushi: 3,
        };
        assert_eq!(shanten.min(), -1);
    }

    #[test]
    fn calculate_shanten_standard_complete_hand() {
        let counts = counts(&[
            "1m", "2m", "3m", "4m", "5m", "6m", "7m", "8m", "9m", "1p", "2p", "3p", "5s", "5s",
        ]);
        let shanten = calculate_shanten(&counts);
        assert_eq!(shanten.standard, -1);
        assert_eq!(shanten.min(), -1);
    }

    #[test]
    fn calculate_shanten_chiitoitsu_complete_hand() {
        let counts = counts(&[
            "1m", "1m", "2m", "2m", "3m", "3m", "4p", "4p", "5p", "5p", "6s", "6s", "E", "E",
        ]);
        let shanten = calculate_shanten(&counts);
        assert_eq!(shanten.chiitoitsu, -1);
        assert_eq!(shanten.min(), -1);
    }

    #[test]
    fn calculate_shanten_kokushi_complete_hand() {
        let counts = counts(&[
            "1m", "9m", "1p", "9p", "1s", "9s", "E", "S", "W", "N", "P", "F", "C", "1m",
        ]);
        let shanten = calculate_shanten(&counts);
        assert_eq!(shanten.kokushi, -1);
        assert_eq!(shanten.min(), -1);
    }

    #[test]
    fn calculate_shanten_standard_tenpai_hand() {
        let counts = counts(&[
            "1m", "2m", "3m", "4m", "5m", "6m", "7m", "8m", "9m", "1p", "2p", "3p", "5s",
        ]);
        let shanten = calculate_shanten(&counts);
        assert_eq!(shanten.standard, 0);
        assert_eq!(shanten.min(), 0);
    }

    #[test]
    fn calculate_shanten_chiitoitsu_tenpai_hand() {
        let counts = counts(&[
            "1m", "1m", "2m", "2m", "3m", "3m", "4p", "4p", "5p", "5p", "6s", "6s", "E",
        ]);
        let shanten = calculate_shanten(&counts);
        assert_eq!(shanten.chiitoitsu, 0);
        assert_eq!(shanten.min(), 0);
    }

    #[test]
    fn calculate_shanten_kokushi_tenpai_hand() {
        let counts = counts(&[
            "1m", "9m", "1p", "9p", "1s", "9s", "E", "S", "W", "N", "P", "F", "C",
        ]);
        let shanten = calculate_shanten(&counts);
        assert_eq!(shanten.kokushi, 0);
        assert_eq!(shanten.min(), 0);
    }

    #[test]
    fn calculate_shanten_empty_hand() {
        let shanten = calculate_shanten(&TileCounts::new());
        assert_eq!(shanten.standard, 8);
        assert_eq!(shanten.chiitoitsu, 13);
        assert_eq!(shanten.kokushi, 13);
        assert_eq!(shanten.min(), 8);
    }

    fn fixed(value: u8) -> FixedMeldCount {
        FixedMeldCount::new(value).unwrap()
    }

    #[test]
    fn fixed_meld_count_accepts_zero_to_four() {
        for value in 0..=FixedMeldCount::MAX {
            assert_eq!(
                FixedMeldCount::new(value).map(FixedMeldCount::get),
                Some(value)
            );
        }
        assert_eq!(FixedMeldCount::NONE.get(), 0);
        assert!(!FixedMeldCount::NONE.has_melds());
        assert!(fixed(1).has_melds());
    }

    #[test]
    fn fixed_meld_count_rejects_more_than_four_without_clamping() {
        assert_eq!(FixedMeldCount::new(5), None);
        assert_eq!(FixedMeldCount::new(u8::MAX), None);
    }

    #[test]
    fn standard_shanten_with_zero_fixed_melds_matches_concealed_api() {
        let hands = [
            counts(&[
                "1m", "2m", "3m", "4m", "5m", "6m", "7m", "8m", "9m", "1p", "2p", "3p", "5s", "5s",
            ]),
            counts(&[
                "1m", "2m", "3m", "4m", "5m", "6m", "7m", "8m", "9m", "1p", "2p", "3p", "5s",
            ]),
            counts(&[
                "1m", "2m", "3m", "4m", "5m", "6m", "7m", "8m", "9m", "1p", "5s", "E", "E",
            ]),
            counts(&[
                "1m", "1m", "2m", "2m", "3m", "3m", "4p", "4p", "5p", "5p", "6s", "6s", "E",
            ]),
            counts(&[
                "1m", "9m", "1p", "9p", "1s", "9s", "E", "S", "W", "N", "P", "F", "C",
            ]),
            counts(&["1m", "3m", "5m", "7m", "9m", "E", "S", "W"]),
            TileCounts::new(),
        ];

        for hand in hands {
            assert_eq!(
                standard_shanten_with_fixed_melds(&hand, FixedMeldCount::NONE),
                standard_shanten(&hand)
            );
            assert_eq!(
                calculate_shanten_with_fixed_melds(&hand, FixedMeldCount::NONE),
                EffectiveShanten::Concealed(calculate_shanten(&hand))
            );
            assert_eq!(
                calculate_shanten_with_fixed_melds(&hand, FixedMeldCount::NONE).min(),
                calculate_shanten(&hand).min()
            );
        }
    }

    #[test]
    fn one_fixed_meld_with_nine_tiles_and_single_tile_is_tenpai() {
        let hand = counts(&["1m", "2m", "3m", "4m", "5m", "6m", "7m", "8m", "9m", "5p"]);
        assert_eq!(standard_shanten_with_fixed_melds(&hand, fixed(1)), 0);
        assert_eq!(calculate_shanten_with_fixed_melds(&hand, fixed(1)).min(), 0);
    }

    #[test]
    fn one_fixed_meld_with_nine_tiles_and_pair_is_complete() {
        let hand = counts(&[
            "1m", "2m", "3m", "4m", "5m", "6m", "7m", "8m", "9m", "5p", "5p",
        ]);
        assert_eq!(standard_shanten_with_fixed_melds(&hand, fixed(1)), -1);
        assert_eq!(
            calculate_shanten_with_fixed_melds(&hand, fixed(1)).min(),
            -1
        );
    }

    #[test]
    fn two_fixed_melds_with_six_tiles_and_single_tile_is_tenpai() {
        let hand = counts(&["1m", "2m", "3m", "4m", "5m", "6m", "5p"]);
        assert_eq!(standard_shanten_with_fixed_melds(&hand, fixed(2)), 0);

        let mut drawn = hand;
        drawn.add(tile("5p"));
        assert_eq!(standard_shanten_with_fixed_melds(&drawn, fixed(2)), -1);
    }

    #[test]
    fn three_fixed_melds_with_three_tiles_and_single_tile_is_tenpai() {
        let hand = counts(&["1m", "2m", "3m", "5p"]);
        assert_eq!(standard_shanten_with_fixed_melds(&hand, fixed(3)), 0);

        let mut drawn = hand;
        drawn.add(tile("5p"));
        assert_eq!(standard_shanten_with_fixed_melds(&drawn, fixed(3)), -1);
    }

    #[test]
    fn four_fixed_melds_with_single_tile_is_tenpai() {
        let hand = counts(&["5p"]);
        assert_eq!(standard_shanten_with_fixed_melds(&hand, fixed(4)), 0);

        let mut drawn = hand;
        drawn.add(tile("5p"));
        assert_eq!(standard_shanten_with_fixed_melds(&drawn, fixed(4)), -1);
    }

    #[test]
    fn fixed_melds_are_not_a_tile_count_correction() {
        let hand = counts(&["1m", "2m", "3m", "5p"]);
        assert_eq!(standard_shanten_with_fixed_melds(&hand, fixed(3)), 0);
        assert_eq!(standard_shanten_with_fixed_melds(&hand, fixed(2)), 2);
        assert_eq!(standard_shanten_with_fixed_melds(&hand, fixed(1)), 4);
        assert_eq!(
            standard_shanten_with_fixed_melds(&hand, FixedMeldCount::NONE),
            6
        );
    }

    #[test]
    fn fixed_melds_do_not_use_kokushi() {
        let hand = counts(&["1m", "9m", "1p", "9p", "1s", "9s", "E", "S", "W", "N"]);
        assert_eq!(kokushi_shanten(&hand), 3);
        assert_eq!(calculate_shanten(&hand).min(), 3);

        let effective = calculate_shanten_with_fixed_melds(&hand, fixed(1));
        assert_eq!(effective.min(), 6);
        assert_eq!(effective.standard(), 6);
        assert_eq!(effective.concealed(), None);
    }

    #[test]
    fn fixed_melds_do_not_use_chiitoitsu() {
        let hand = counts(&["1m", "1m", "4m", "4m", "7m", "7m", "1p", "1p", "4p", "4p"]);
        assert_eq!(chiitoitsu_shanten(&hand), 3);

        let effective = calculate_shanten_with_fixed_melds(&hand, fixed(1));
        assert_eq!(effective.min(), 2);
        assert_eq!(
            effective.min(),
            standard_shanten_with_fixed_melds(&hand, fixed(1))
        );
        assert_eq!(effective.concealed(), None);
    }

    #[test]
    fn effective_shanten_exposes_concealed_shanten_only_without_fixed_melds() {
        let hand = counts(&[
            "1m", "1m", "2m", "2m", "3m", "3m", "4p", "4p", "5p", "5p", "6s", "6s", "E",
        ]);
        let concealed = calculate_shanten_with_fixed_melds(&hand, FixedMeldCount::NONE);
        assert_eq!(concealed.concealed(), Some(calculate_shanten(&hand)));
        assert_eq!(concealed.min(), 0);
        assert_eq!(concealed.standard(), standard_shanten(&hand));
    }
}
