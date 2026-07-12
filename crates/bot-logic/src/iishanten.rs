use crate::shanten::standard_shanten;
use crate::tile::TileType;
use crate::tile_counts::{TileCountError, TileCounts};
use std::collections::HashMap;

/// 門前13枚の通常形一向聴の形分類。
///
/// ここで [`IishantenShape::Complete`] は和了形ではなく「完全一向聴」を意味する。
/// 各牌を一度だけ使用する正確な手牌分解に基づき、`HandShapeSummary` のような
/// 牌の重複利用を許す概算値は使用しない。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IishantenShape {
    /// 完全一向聴。完成面子2組・雀頭1組・搭子2組・孤立牌1枚へ正確に分解できる。
    Complete,
    /// 頭なし一向聴。完成面子3組・搭子2組（雀頭なし）へ正確に分解できる。
    Headless,
    /// くっつき一向聴。完成面子3組・雀頭1組・孤立牌2枚へ正確に分解できる。
    Kuttsuki,
    /// 上記いずれの形にも正確に分解できないが、13枚かつ通常形一向聴である。
    Weak,
    /// 門前13枚の通常形一向聴ではない。
    Unknown,
}

/// 門前13枚の通常形一向聴を形分類する。
///
/// `counts.total() == 13` かつ `standard_shanten(counts) == 1` の場合だけ分類し、
/// どちらかを満たさなければ [`IishantenShape::Unknown`] を返す。分類は各牌を一度だけ
/// 使用する正確な分解で行う。複数の分解が可能な場合は
/// `Complete > Headless > Kuttsuki` の優先順位で一つに決定する。
pub fn classify_standard_iishanten_shape(counts: &TileCounts) -> IishantenShape {
    if counts.total() != 13 || standard_shanten(counts) != 1 {
        return IishantenShape::Unknown;
    }

    let mut memo = DecomposeMemo::new();
    if can_decompose(*counts, Target::COMPLETE, &mut memo) {
        IishantenShape::Complete
    } else if can_decompose(*counts, Target::HEADLESS, &mut memo) {
        IishantenShape::Headless
    } else if can_decompose(*counts, Target::KUTTSUKI, &mut memo) {
        IishantenShape::Kuttsuki
    } else {
        IishantenShape::Weak
    }
}

/// `discard` を1枚取り除いた後の門前13枚を通常形一向聴として形分類する。
///
/// 打牌前 counts しか手元に無い場合に使える pure helper。打牌後 counts を既に構築している
/// 評価生成経路では [`classify_standard_iishanten_shape`] を直接呼ぶ。契約は次の通り。
///
/// - `counts` に `discard` が1枚も含まれない場合は `None`。
/// - 1枚取り除ける場合は `Some(classify_standard_iishanten_shape(打牌後counts))`。
///   そのため打牌後が13枚の通常形一向聴でなければ `Some(IishantenShape::Unknown)` になる。
///
/// 牌は物理IDではなく `TileType` 単位で1枚だけ取り除く。入力の `counts` は変更しない。
pub fn classify_standard_iishanten_shape_after_discard(
    counts: &TileCounts,
    discard: TileType,
) -> Option<IishantenShape> {
    let mut after_discard = *counts;
    after_discard.remove(discard).ok()?;
    Some(classify_standard_iishanten_shape(&after_discard))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct Target {
    melds: u8,
    heads: u8,
    taatsu: u8,
    singles: u8,
}

impl Target {
    const COMPLETE: Self = Self {
        melds: 2,
        heads: 1,
        taatsu: 2,
        singles: 1,
    };
    const HEADLESS: Self = Self {
        melds: 3,
        heads: 0,
        taatsu: 2,
        singles: 0,
    };
    const KUTTSUKI: Self = Self {
        melds: 3,
        heads: 1,
        taatsu: 0,
        singles: 2,
    };

    fn is_empty(self) -> bool {
        self.melds == 0 && self.heads == 0 && self.taatsu == 0 && self.singles == 0
    }
}

type DecomposeMemo = HashMap<([u8; 34], Target), bool>;

fn can_decompose(counts: TileCounts, target: Target, memo: &mut DecomposeMemo) -> bool {
    if target.is_empty() {
        return counts.is_empty();
    }

    let Some(tile) = counts
        .iter()
        .find_map(|(tile, count)| (count >= 1).then_some(tile))
    else {
        return false;
    };

    let key = (*counts.as_array(), target);
    if let Some(&cached) = memo.get(&key) {
        return cached;
    }

    let mut result = false;

    if target.melds > 0 {
        let next = Target {
            melds: target.melds - 1,
            ..target
        };
        result = try_decompose(counts, tile, TileCounts::remove_triplet, next, memo)
            || try_decompose(counts, tile, TileCounts::remove_sequence, next, memo);
    }

    if !result && target.heads > 0 {
        let next = Target {
            heads: target.heads - 1,
            ..target
        };
        result = try_decompose(counts, tile, TileCounts::remove_pair, next, memo);
    }

    if !result && target.taatsu > 0 {
        let next = Target {
            taatsu: target.taatsu - 1,
            ..target
        };
        result = try_decompose(counts, tile, TileCounts::remove_pair, next, memo)
            || try_decompose(counts, tile, TileCounts::remove_adjacent_wait, next, memo)
            || try_decompose(counts, tile, TileCounts::remove_skip_wait, next, memo);
    }

    if !result && target.singles > 0 {
        let next = Target {
            singles: target.singles - 1,
            ..target
        };
        result = try_decompose(counts, tile, TileCounts::remove, next, memo);
    }

    memo.insert(key, result);
    result
}

fn try_decompose(
    counts: TileCounts,
    tile: TileType,
    remove: fn(&mut TileCounts, TileType) -> Result<(), TileCountError>,
    target: Target,
    memo: &mut DecomposeMemo,
) -> bool {
    let mut removed = counts;
    remove(&mut removed, tile).is_ok() && can_decompose(removed, target, memo)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shanten::{chiitoitsu_shanten, kokushi_shanten};

    fn counts(strings: &[&str]) -> TileCounts {
        TileCounts::from_tile_types(
            strings
                .iter()
                .map(|s| TileType::from_mjai_type_str(s).unwrap()),
        )
    }

    fn tt(string: &str) -> TileType {
        TileType::from_mjai_type_str(string).unwrap()
    }

    #[test]
    fn complete_shape() {
        let hand = counts(&[
            "1m", "2m", "3m", "4m", "5m", "6m", "E", "E", "2p", "3p", "5s", "6s", "C",
        ]);
        assert_eq!(standard_shanten(&hand), 1);
        assert_eq!(
            classify_standard_iishanten_shape(&hand),
            IishantenShape::Complete
        );
    }

    #[test]
    fn headless_shape() {
        let hand = counts(&[
            "1m", "2m", "3m", "4m", "5m", "6m", "7m", "8m", "9m", "2p", "3p", "5s", "6s",
        ]);
        assert_eq!(standard_shanten(&hand), 1);
        assert_eq!(
            classify_standard_iishanten_shape(&hand),
            IishantenShape::Headless
        );
    }

    #[test]
    fn kuttsuki_shape() {
        let hand = counts(&[
            "1m", "2m", "3m", "4m", "5m", "6m", "7m", "8m", "9m", "5p", "5p", "2s", "8s",
        ]);
        assert_eq!(standard_shanten(&hand), 1);
        assert_eq!(
            classify_standard_iishanten_shape(&hand),
            IishantenShape::Kuttsuki
        );
    }

    #[test]
    fn weak_shape_is_not_misread_as_complete() {
        // 完成面子3組・搭子1組・孤立牌2枚。連続形の候補を重複して数える
        // HandShapeSummary ではなく、正確な分解を使っていることの回帰テスト。
        let hand = counts(&[
            "1m", "2m", "3m", "4m", "5m", "6m", "7m", "8m", "9m", "2p", "3p", "5s", "E",
        ]);
        assert_eq!(standard_shanten(&hand), 1);
        assert_eq!(
            classify_standard_iishanten_shape(&hand),
            IishantenShape::Weak
        );
    }

    #[test]
    fn tenpai_is_unknown() {
        let hand = counts(&[
            "1m", "2m", "3m", "4m", "5m", "6m", "7m", "8m", "9m", "2p", "3p", "5s", "5s",
        ]);
        assert_eq!(standard_shanten(&hand), 0);
        assert_eq!(
            classify_standard_iishanten_shape(&hand),
            IishantenShape::Unknown
        );
    }

    #[test]
    fn two_shanten_is_unknown() {
        let hand = counts(&[
            "1m", "2m", "3m", "4m", "5m", "6m", "2p", "3p", "5s", "6s", "E", "S", "W",
        ]);
        assert_eq!(standard_shanten(&hand), 2);
        assert_eq!(
            classify_standard_iishanten_shape(&hand),
            IishantenShape::Unknown
        );
    }

    #[test]
    fn wrong_tile_count_is_unknown() {
        let twelve = counts(&[
            "1m", "2m", "3m", "4m", "5m", "6m", "7m", "8m", "9m", "2p", "3p", "5s",
        ]);
        assert_eq!(twelve.total(), 12);
        assert_eq!(
            classify_standard_iishanten_shape(&twelve),
            IishantenShape::Unknown
        );

        let fourteen = counts(&[
            "1m", "2m", "3m", "4m", "5m", "6m", "7m", "8m", "9m", "2p", "3p", "5s", "6s", "C",
        ]);
        assert_eq!(fourteen.total(), 14);
        assert_eq!(
            classify_standard_iishanten_shape(&fourteen),
            IishantenShape::Unknown
        );

        let empty = TileCounts::new();
        assert_eq!(empty.total(), 0);
        assert_eq!(
            classify_standard_iishanten_shape(&empty),
            IishantenShape::Unknown
        );
    }

    #[test]
    fn chiitoitsu_only_iishanten_is_unknown() {
        let hand = counts(&[
            "E", "E", "S", "S", "W", "W", "N", "N", "P", "P", "F", "C", "1m",
        ]);
        assert_eq!(chiitoitsu_shanten(&hand), 1);
        assert_ne!(standard_shanten(&hand), 1);
        assert_eq!(
            classify_standard_iishanten_shape(&hand),
            IishantenShape::Unknown
        );
    }

    #[test]
    fn kokushi_only_iishanten_is_unknown() {
        let hand = counts(&[
            "1m", "1m", "9m", "1p", "9p", "1s", "9s", "E", "S", "W", "N", "P", "5m",
        ]);
        assert_eq!(kokushi_shanten(&hand), 1);
        assert_ne!(standard_shanten(&hand), 1);
        assert_eq!(
            classify_standard_iishanten_shape(&hand),
            IishantenShape::Unknown
        );
    }

    #[test]
    fn classification_does_not_mutate_input() {
        let hand = counts(&[
            "1m", "2m", "3m", "4m", "5m", "6m", "E", "E", "2p", "3p", "5s", "6s", "C",
        ]);
        let before = hand;
        let _ = classify_standard_iishanten_shape(&hand);
        assert_eq!(hand, before);
    }

    #[test]
    fn after_discard_complete() {
        // Complete の13枚形へ捨てる14枚目(1s)を加える。1s を切ると Complete に戻る。
        let hand = counts(&[
            "1m", "2m", "3m", "4m", "5m", "6m", "E", "E", "2p", "3p", "5s", "6s", "C", "1s",
        ]);
        assert_eq!(
            classify_standard_iishanten_shape_after_discard(&hand, tt("1s")),
            Some(IishantenShape::Complete)
        );
    }

    #[test]
    fn after_discard_headless() {
        // Headless の13枚形へ捨てる14枚目(1s)を加える。1s を切ると Headless に戻る。
        let hand = counts(&[
            "1m", "2m", "3m", "4m", "5m", "6m", "7m", "8m", "9m", "2p", "3p", "5s", "6s", "1s",
        ]);
        assert_eq!(
            classify_standard_iishanten_shape_after_discard(&hand, tt("1s")),
            Some(IishantenShape::Headless)
        );
    }

    #[test]
    fn after_discard_kuttsuki() {
        // Kuttsuki の13枚形へ捨てる14枚目(9s)を加える。9s を切ると Kuttsuki に戻る。
        let hand = counts(&[
            "1m", "2m", "3m", "4m", "5m", "6m", "7m", "8m", "9m", "5p", "5p", "2s", "8s", "9s",
        ]);
        assert_eq!(
            classify_standard_iishanten_shape_after_discard(&hand, tt("9s")),
            Some(IishantenShape::Kuttsuki)
        );
    }

    #[test]
    fn after_discard_weak() {
        // Weak の13枚形へ捨てる14枚目(1s)を加える。1s を切ると Weak に戻る。
        let hand = counts(&[
            "1m", "2m", "3m", "4m", "5m", "6m", "7m", "8m", "9m", "2p", "3p", "5s", "E", "1s",
        ]);
        assert_eq!(
            classify_standard_iishanten_shape_after_discard(&hand, tt("1s")),
            Some(IishantenShape::Weak)
        );
    }

    #[test]
    fn after_discard_unknown_when_not_iishanten() {
        // テンパイ形へ余分な牌(1s)を加える。1s を切ると通常形一向聴ではなくテンパイなので Unknown。
        let hand = counts(&[
            "1m", "2m", "3m", "4m", "5m", "6m", "7m", "8m", "9m", "2p", "3p", "5s", "5s", "1s",
        ]);
        assert_eq!(
            classify_standard_iishanten_shape_after_discard(&hand, tt("1s")),
            Some(IishantenShape::Unknown)
        );
    }

    #[test]
    fn after_discard_none_when_tile_absent() {
        let hand = counts(&[
            "1m", "2m", "3m", "4m", "5m", "6m", "E", "E", "2p", "3p", "5s", "6s", "C", "1s",
        ]);
        assert_eq!(
            classify_standard_iishanten_shape_after_discard(&hand, tt("9p")),
            None
        );
    }

    #[test]
    fn after_discard_does_not_mutate_input() {
        let hand = counts(&[
            "1m", "2m", "3m", "4m", "5m", "6m", "E", "E", "2p", "3p", "5s", "6s", "C", "1s",
        ]);
        let before = hand;
        let _ = classify_standard_iishanten_shape_after_discard(&hand, tt("1s"));
        assert_eq!(hand, before);
    }

    #[test]
    fn complete_takes_priority_over_headless() {
        // 111m 456m 789m 23p 56s は Complete（刻子を雀頭+孤立牌へ分けた形）
        // と Headless（刻子をそのまま完成面子とした形）の両方に正確に分解できる。
        // 優先順位により Complete を返す。
        let hand = counts(&[
            "1m", "1m", "1m", "4m", "5m", "6m", "7m", "8m", "9m", "2p", "3p", "5s", "6s",
        ]);
        assert_eq!(standard_shanten(&hand), 1);

        let mut memo = DecomposeMemo::new();
        assert!(can_decompose(hand, Target::COMPLETE, &mut memo));
        assert!(can_decompose(hand, Target::HEADLESS, &mut memo));

        assert_eq!(
            classify_standard_iishanten_shape(&hand),
            IishantenShape::Complete
        );
    }
}
