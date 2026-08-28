//! テンパイ到達後に自分のツモで和了する期待支払いを閉形式で求める pure な確率模型。
//!
//! 1向聴の打牌比較で「すぐテンパイする枝」と「一度手変わりしてから進む枝」を同じ尺度へ揃える
//! ための計算だけを持つ。向聴・受け入れ・待ち・点数計算はどれも既存 layer が source of truth で、
//! この module は残枚数と自摸回数から確率を組み立てるだけになっている。
//!
//! # 評価する範囲
//!
//! 求めるのは局収支 EV ではなく self-tsumo-only offense continuation value で、
//!
//! ```text
//! その経路を実際に引く確率 × テンパイ到達後に残り自摸機会内でツモ和了する期待支払い
//! ```
//!
//! だけを含む。ロン和了・他家の和了・放銃・鳴き・将来の槓・ダマ後の手変わり・本場・供託・
//! 点棒状況はどれも含めない。
//!
//! # テンパイ後の continuation
//!
//! テンパイ到達後は毎巡の全ツモを再帰探索せず、超幾何分布の閉形式で求める。terminal tenpai
//! 時点で
//!
//! ```text
//! U = 自分から見て未確認の物理牌
//! W = ツモ和了できる live physical winning variant の残枚数合計
//! n = そこから流局までに残っている自分の自摸機会
//! ```
//!
//! とすると、`n` 回以内に少なくとも1枚 winning variant を引く確率は
//!
//! ```text
//! P_hit = 1 - C(U - W, n) / C(U, n)
//! ```
//!
//! になる。`W == 0` と `n == 0` は 0、`n > U - W` は 1、`n > U` は `n = U` として扱う。
//!
//! # 固定小数点
//!
//! `U <= 136` の小さい組合せしか扱わないので、確率も期待支払いも浮動小数点を使わず u128 の
//! 固定小数点で求める。`C(U - W, n) / C(U, n)` は階乗を展開せず1手ずつ約分するため、桁溢れも
//! ゼロ除算も起きない。

/// ツモ和了確率の固定小数点スケール。`TSUMO_PROBABILITY_SCALE` が確率 1 を表す。
pub const TSUMO_PROBABILITY_SCALE: u64 = 1_000_000_000_000;

/// 期待支払いの固定小数点スケール。`SELF_TSUMO_VALUE_SCALE` が 1 点を表す。
pub const SELF_TSUMO_VALUE_SCALE: u64 = 1_000_000;

/// terminal tenpai 到達時点の、自分の自摸機会に関する事実。
///
/// どちらも現在打牌後の値で、仮想ツモを1回進めるごとに1ずつ減る。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SelfTsumoFacts {
    /// 現在打牌後に自分から見て未確認の物理牌の総数 `U0`。山の残枚数ではない。
    pub unknown_tiles: u32,
    /// 現在打牌後に自分へ残っている自摸機会。
    pub own_future_draws: u32,
}

/// terminal tenpai 1件分の、ツモ和了できる待ちの残枚数と打点。
///
/// `winning_remaining` と `weighted_total` はどちらも物理牌 variant 単位の集計で、赤5と黒5を
/// 牌種へ潰す前の値。ツモ baseline で役が無い variant は和了できないので、どちらにも含めない。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct TenpaiTsumoValue {
    /// `W`。ツモ和了できる live physical winning variant の残枚数合計。
    pub winning_remaining: u32,
    /// `Σ(variant の残枚数 × その variant でツモ和了した場合の支払い合計)`。
    pub weighted_total: u64,
}

impl TenpaiTsumoValue {
    /// このテンパイから `own_draws` 回以内にツモ和了する期待支払い [[`SELF_TSUMO_VALUE_SCALE`]]。
    ///
    /// `P_hit × weighted_total / W` で、和了できる待ちが1枚も無ければ 0。
    pub fn expected_payment(self, unknown: u32, own_draws: u32) -> u64 {
        if self.winning_remaining == 0 {
            return 0;
        }
        let hit = tsumo_hit_probability(unknown, self.winning_remaining, own_draws);
        let numerator =
            u128::from(SELF_TSUMO_VALUE_SCALE) * u128::from(hit) * u128::from(self.weighted_total);
        let denominator = u128::from(TSUMO_PROBABILITY_SCALE) * u128::from(self.winning_remaining);
        u64::try_from(numerator / denominator).unwrap_or(u64::MAX)
    }
}

/// terminal tenpai へ至る経路1本分の確率。
///
/// 分子は経路上で引く物理牌 variant の残枚数の積、分母は自分が1枚確認するごとに1減る unknown
/// pool の積。相手3人の自摸で分母を機械的に減らすことはせず、自分に割り当てられる将来の自摸
/// 位置を unknown physical tiles のランダム配置として扱う。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SelfTsumoPath {
    numerator: u64,
    denominator: u64,
    /// この経路で使った自分の自摸回数 `d`。
    own_draws: u32,
}

impl SelfTsumoPath {
    /// 現在打牌後の1回目の自摸で `remaining` 枚の物理牌 variant を引く経路。
    pub fn immediate(remaining: u8, unknown_tiles: u32) -> Option<Self> {
        (unknown_tiles >= 1).then_some(Self {
            numerator: u64::from(remaining),
            denominator: u64::from(unknown_tiles),
            own_draws: 1,
        })
    }

    /// 1回目に手変わりの物理牌 variant を、2回目に向聴数を下げる物理牌 variant を引く経路。
    ///
    /// 2回目の残枚数は1回目のツモを手牌へ加えた後の値で、分母は `U0 × (U0 - 1)`。
    pub fn via_same_shanten(first: u8, second: u8, unknown_tiles: u32) -> Option<Self> {
        (unknown_tiles >= 2).then_some(Self {
            numerator: u64::from(first) * u64::from(second),
            denominator: u64::from(unknown_tiles) * u64::from(unknown_tiles - 1),
            own_draws: 2,
        })
    }

    /// この経路を実際に引く確率 [[`TSUMO_PROBABILITY_SCALE`]]。診断表示用。
    pub fn probability(self) -> u64 {
        let scaled = u128::from(TSUMO_PROBABILITY_SCALE) * u128::from(self.numerator)
            / u128::from(self.denominator);
        u64::try_from(scaled).unwrap_or(TSUMO_PROBABILITY_SCALE)
    }

    pub fn own_draws(self) -> u32 {
        self.own_draws
    }

    /// この経路の terminal tenpai 時点で自分から見て未確認の物理牌。
    pub fn terminal_unknown_tiles(self, facts: SelfTsumoFacts) -> u32 {
        facts.unknown_tiles.saturating_sub(self.own_draws)
    }

    /// この経路の terminal tenpai 時点で自分へ残っている自摸機会。
    pub fn terminal_own_future_draws(self, facts: SelfTsumoFacts) -> u32 {
        facts.own_future_draws.saturating_sub(self.own_draws)
    }

    /// この経路の期待支払い [[`SELF_TSUMO_VALUE_SCALE`]]。
    ///
    /// `経路確率 × terminal continuation` を1回の除算へ畳み、途中で丸めない。
    pub fn expected_payment(self, facts: SelfTsumoFacts, terminal: TenpaiTsumoValue) -> u64 {
        if terminal.winning_remaining == 0 {
            return 0;
        }
        let hit = tsumo_hit_probability(
            self.terminal_unknown_tiles(facts),
            terminal.winning_remaining,
            self.terminal_own_future_draws(facts),
        );
        let numerator = u128::from(SELF_TSUMO_VALUE_SCALE)
            * u128::from(self.numerator)
            * u128::from(hit)
            * u128::from(terminal.weighted_total);
        let denominator = u128::from(self.denominator)
            * u128::from(TSUMO_PROBABILITY_SCALE)
            * u128::from(terminal.winning_remaining);
        u64::try_from(numerator / denominator).unwrap_or(u64::MAX)
    }
}

/// `unknown` 枚の未確認牌から `own_draws` 回引く間に、`winning` 枚のうち少なくとも1枚を引く確率
/// [[`TSUMO_PROBABILITY_SCALE`]]。
///
/// `1 - C(unknown - winning, own_draws) / C(unknown, own_draws)` を階乗を展開せずに求める。
/// 和了牌が無い場合と自摸機会が無い場合は 0、外し切れない場合 (`own_draws > unknown - winning`)
/// は 1 になる。`own_draws > unknown` は `own_draws = unknown` として扱う。
pub fn tsumo_hit_probability(unknown: u32, winning: u32, own_draws: u32) -> u64 {
    if unknown == 0 || winning == 0 || own_draws == 0 {
        return 0;
    }

    let winning = winning.min(unknown);
    let miss_pool = unknown - winning;
    let own_draws = own_draws.min(unknown);
    if own_draws > miss_pool {
        return TSUMO_PROBABILITY_SCALE;
    }

    let mut miss = u128::from(TSUMO_PROBABILITY_SCALE);
    for drawn in 0..own_draws {
        miss = miss * u128::from(miss_pool - drawn) / u128::from(unknown - drawn);
    }
    TSUMO_PROBABILITY_SCALE.saturating_sub(u64::try_from(miss).unwrap_or(TSUMO_PROBABILITY_SCALE))
}

#[cfg(test)]
mod tests {
    use super::*;

    // 確率 1 を分母 `denominator` の分数と比べる。固定小数点の丸めだけを許す。
    fn assert_probability(actual: u64, numerator: u64, denominator: u64) {
        let expected =
            u128::from(TSUMO_PROBABILITY_SCALE) * u128::from(numerator) / u128::from(denominator);
        let expected = u64::try_from(expected).expect("固定小数点へ収まる");
        assert!(
            actual.abs_diff(expected) <= 1,
            "actual: {actual}, expected: {expected}"
        );
    }

    #[test]
    fn no_winning_tile_never_hits() {
        assert_eq!(tsumo_hit_probability(100, 0, 10), 0);
    }

    #[test]
    fn no_remaining_draw_never_hits() {
        assert_eq!(tsumo_hit_probability(100, 4, 0), 0);
    }

    #[test]
    fn a_single_draw_hits_with_the_wait_ratio() {
        assert_probability(tsumo_hit_probability(4, 1, 1), 1, 4);
        assert_probability(tsumo_hit_probability(100, 4, 1), 4, 100);
    }

    #[test]
    fn drawing_more_than_the_missing_tiles_always_hits() {
        // U - W = 3 なので、4回引けば必ず当たる。
        assert_eq!(tsumo_hit_probability(4, 1, 4), TSUMO_PROBABILITY_SCALE);
        assert_eq!(tsumo_hit_probability(4, 1, 100), TSUMO_PROBABILITY_SCALE);
    }

    #[test]
    fn every_tile_winning_always_hits() {
        assert_eq!(tsumo_hit_probability(4, 4, 1), TSUMO_PROBABILITY_SCALE);
        assert_eq!(tsumo_hit_probability(136, 136, 1), TSUMO_PROBABILITY_SCALE);
    }

    #[test]
    fn the_closed_form_matches_the_hypergeometric_complement() {
        // 1 - C(U - W, n) / C(U, n) を素直に展開した値と一致する。
        assert_probability(tsumo_hit_probability(4, 1, 2), 1, 2);
        assert_probability(tsumo_hit_probability(4, 1, 3), 3, 4);
        // 1 - (96 × 95) / (100 × 99) = 1 - 9120 / 9900
        assert_probability(tsumo_hit_probability(100, 4, 2), 780, 9900);
    }

    #[test]
    fn the_boundary_of_the_tile_count_does_not_overflow() {
        // 全 unknown 牌を引く経路でも桁溢れもゼロ除算も起きない。
        assert_eq!(tsumo_hit_probability(136, 1, 136), TSUMO_PROBABILITY_SCALE);
        assert_probability(tsumo_hit_probability(136, 1, 135), 135, 136);
        assert_eq!(tsumo_hit_probability(0, 0, 10), 0);
        assert!(tsumo_hit_probability(136, 4, 18) < TSUMO_PROBABILITY_SCALE);
    }

    #[test]
    fn more_remaining_draws_never_lower_the_probability() {
        let probabilities: Vec<_> = (0..=20)
            .map(|draws| tsumo_hit_probability(122, 4, draws))
            .collect();
        for pair in probabilities.windows(2) {
            assert!(pair[0] <= pair[1]);
        }
    }

    #[test]
    fn an_earlier_tenpai_keeps_more_draws_and_is_worth_more() {
        // 同じ待ち・同じツモ打点なら、残っている自摸機会が多いほど continuation が高い。
        let terminal = TenpaiTsumoValue {
            winning_remaining: 4,
            weighted_total: 4 * 3900,
        };
        let early = terminal.expected_payment(100, 9);
        let late = terminal.expected_payment(100, 8);
        assert!(early > late, "early: {early}, late: {late}");
    }

    #[test]
    fn a_wider_wait_is_worth_more_at_the_same_payment() {
        let narrow = TenpaiTsumoValue {
            winning_remaining: 4,
            weighted_total: 4 * 3900,
        };
        let wide = TenpaiTsumoValue {
            winning_remaining: 6,
            weighted_total: 6 * 3900,
        };
        assert!(wide.expected_payment(100, 8) > narrow.expected_payment(100, 8));
    }

    #[test]
    fn a_higher_hand_value_is_worth_more_at_the_same_wait() {
        let cheap = TenpaiTsumoValue {
            winning_remaining: 4,
            weighted_total: 4 * 1300,
        };
        let expensive = TenpaiTsumoValue {
            winning_remaining: 4,
            weighted_total: 4 * 7700,
        };
        assert!(expensive.expected_payment(100, 8) > cheap.expected_payment(100, 8));
    }

    #[test]
    fn a_dead_wait_is_worth_nothing() {
        let dead = TenpaiTsumoValue {
            winning_remaining: 0,
            weighted_total: 0,
        };
        assert_eq!(dead.expected_payment(100, 8), 0);
    }

    #[test]
    fn the_expected_payment_is_the_hit_probability_times_the_average_payment() {
        let terminal = TenpaiTsumoValue {
            winning_remaining: 4,
            // 赤5を引いた場合だけ打点が上がる待ち。variant ごとの重み付き合計そのもの。
            weighted_total: 3 * 3900 + 5200,
        };
        let hit = tsumo_hit_probability(100, 4, 3);
        let expected = u128::from(SELF_TSUMO_VALUE_SCALE)
            * u128::from(hit)
            * u128::from(terminal.weighted_total)
            / (u128::from(TSUMO_PROBABILITY_SCALE) * 4);
        assert_eq!(u128::from(terminal.expected_payment(100, 3)), expected,);
    }

    #[test]
    fn a_path_probability_is_the_product_over_the_shrinking_unknown_pool() {
        let immediate = SelfTsumoPath::immediate(4, 100).expect("経路を作れる");
        assert_probability(immediate.probability(), 4, 100);
        assert_eq!(immediate.own_draws(), 1);

        let via = SelfTsumoPath::via_same_shanten(4, 6, 100).expect("経路を作れる");
        assert_probability(via.probability(), 4 * 6, 100 * 99);
        assert_eq!(via.own_draws(), 2);
    }

    #[test]
    fn a_path_consumes_one_unknown_tile_and_one_draw_per_step() {
        let facts = SelfTsumoFacts {
            unknown_tiles: 100,
            own_future_draws: 10,
        };
        let immediate = SelfTsumoPath::immediate(4, 100).expect("経路を作れる");
        assert_eq!(immediate.terminal_unknown_tiles(facts), 99);
        assert_eq!(immediate.terminal_own_future_draws(facts), 9);

        let via = SelfTsumoPath::via_same_shanten(4, 4, 100).expect("経路を作れる");
        assert_eq!(via.terminal_unknown_tiles(facts), 98);
        assert_eq!(via.terminal_own_future_draws(facts), 8);
    }

    #[test]
    fn a_path_without_an_unknown_pool_has_no_probability() {
        assert_eq!(SelfTsumoPath::immediate(4, 0), None);
        assert_eq!(SelfTsumoPath::via_same_shanten(4, 4, 1), None);
    }

    #[test]
    fn the_remaining_draws_never_go_below_zero() {
        let facts = SelfTsumoFacts {
            unknown_tiles: 4,
            own_future_draws: 1,
        };
        let via = SelfTsumoPath::via_same_shanten(2, 2, 4).expect("経路を作れる");
        assert_eq!(via.terminal_own_future_draws(facts), 0);
        assert_eq!(
            via.expected_payment(
                facts,
                TenpaiTsumoValue {
                    winning_remaining: 2,
                    weighted_total: 2 * 3900,
                }
            ),
            0
        );
    }
}
