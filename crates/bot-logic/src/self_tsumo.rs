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
//! n = そこから残っている自分の自摸機会 (soft horizon 適用後)
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
//! # soft horizon
//!
//! `n` の起点は流局までの raw の自摸機会ではなく、[`SelfTsumoHorizon`] で短くした値。他家和了・
//! 放銃などによる局の途中終了を直接モデル化する代わりの近似で、経路確率・`P_hit`・terminal
//! scoring の式は変えず、起点の自摸機会だけを変える。
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

/// 従来の流局までの horizon とみなす巡目。`horizon_turn` がこれ以上なら soft horizon は raw の
/// 自摸機会をそのまま返す。
pub const UNTIL_RYUKYOKU_HORIZON_TURN: u32 = 18;

/// self-tsumo continuation が見る将来の自摸機会の soft horizon。
///
/// 他家和了・放銃などによる局の途中終了を直接モデル化する代わりに、self-tsumo continuation の
/// 将来 horizon を短くする近似で、「`horizon_turn` 巡目で局が終わる」モデルではない。
/// 流局までを [`UNTIL_RYUKYOKU_HORIZON_TURN`] 巡相当とみなし、その差だけ raw の自摸機会を減らす。
/// `horizon_turn` 巡目相当を過ぎても `late_min_future_draws` 回までは残すので、終盤でも近未来の
/// self-tsumo 評価は無効にならない。どの場合も raw を超えない。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SelfTsumoHorizon {
    pub horizon_turn: u32,
    pub late_min_future_draws: u32,
}

impl SelfTsumoHorizon {
    /// production の既定値。
    ///
    /// `late_min_future_draws = 2` は、1向聴から `Progress -> テンパイ -> 1回のツモ和了機会` を
    /// 最低限評価できる値。1 では最初のツモをテンパイ到達に使った時点で terminal の自摸機会が
    /// 0 になり、終盤の ExpectedSelfTsumoValue が実質的に無効になる。
    pub const PRODUCTION: Self = Self {
        horizon_turn: 12,
        late_min_future_draws: 2,
    };

    /// 流局までの自摸機会をすべて使う従来の semantics。
    pub const UNTIL_RYUKYOKU: Self = Self {
        horizon_turn: UNTIL_RYUKYOKU_HORIZON_TURN,
        late_min_future_draws: 0,
    };

    /// この horizon がすでに流局までの semantics か。
    ///
    /// `horizon_turn >= UNTIL_RYUKYOKU_HORIZON_TURN` では soft horizon の reduction が 0 になり、
    /// `late_min_future_draws` に依らず raw の自摸機会をそのまま使う。struct の一致ではなく
    /// この判定を source of truth にする。
    pub const fn is_until_ryukyoku(self) -> bool {
        self.horizon_turn >= UNTIL_RYUKYOKU_HORIZON_TURN
    }

    /// raw の残り自摸機会へこの horizon を適用した値。
    pub fn effective_future_draws(self, raw_future_draws: u32) -> u32 {
        soft_horizon_future_draws(
            raw_future_draws,
            self.horizon_turn,
            self.late_min_future_draws,
        )
    }
}

impl Default for SelfTsumoHorizon {
    fn default() -> Self {
        Self::PRODUCTION
    }
}

/// raw の残り自摸機会 (流局まで) を soft horizon で短くした自摸機会。
///
/// ```text
/// horizon_reduction = 18 - horizon_turn
/// effective = min(raw, max(raw - horizon_reduction, late_min_future_draws))
/// ```
///
/// 減算はどちらも 0 で止める。`horizon_turn >= 18` では raw のまま。
pub fn soft_horizon_future_draws(
    raw_future_draws: u32,
    horizon_turn: u32,
    late_min_future_draws: u32,
) -> u32 {
    let horizon_reduction = UNTIL_RYUKYOKU_HORIZON_TURN.saturating_sub(horizon_turn);
    raw_future_draws.min(
        raw_future_draws
            .saturating_sub(horizon_reduction)
            .max(late_min_future_draws),
    )
}

/// terminal tenpai 到達時点の、自分の自摸機会に関する事実。
///
/// どちらも現在打牌後の値で、仮想ツモを1回進めるごとに1ずつ減る。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SelfTsumoFacts {
    /// 現在打牌後に自分から見て未確認の物理牌の総数 `U0`。山の残枚数ではない。
    pub unknown_tiles: u32,
    /// 現在打牌後に自分へ残っている自摸機会。流局までの raw 値ではなく、[`SelfTsumoHorizon`]
    /// を適用した後の値。
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

    /// 1回目と2回目に手変わりの物理牌 variant を、3回目に向聴数を下げる物理牌 variant を引く
    /// 経路。
    ///
    /// 各回の残枚数はそこまでのツモを手牌へ加えた後の値で、分母は `U0 × (U0 - 1) × (U0 - 2)`。
    /// 手変わりを2回まで許す診断専用の追加深度だけが使う。
    pub fn via_same_shanten_twice(
        first: u8,
        second: u8,
        third: u8,
        unknown_tiles: u32,
    ) -> Option<Self> {
        (unknown_tiles >= 3).then_some(Self {
            numerator: u64::from(first) * u64::from(second) * u64::from(third),
            denominator: u64::from(unknown_tiles)
                * u64::from(unknown_tiles - 1)
                * u64::from(unknown_tiles - 2),
            own_draws: 3,
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

    /// この経路の terminal state を起点に求めた continuation value を、経路確率で重み付けする
    /// [[`SELF_TSUMO_VALUE_SCALE`]]。
    ///
    /// 手前の経路と、その先で別に求めた期待支払いをつなぐための合成。`value` はこの経路の
    /// terminal state ([`Self::terminal_unknown_tiles`] / [`Self::terminal_own_future_draws`])
    /// を起点に、同じ scale で求めた値を渡す。確率は [`Self::probability`] と同じ分数のまま
    /// 掛け、途中で丸めない。
    pub fn weighted_continuation(self, value: u64) -> u64 {
        let weighted =
            u128::from(value) * u128::from(self.numerator) / u128::from(self.denominator);
        u64::try_from(weighted).unwrap_or(u64::MAX)
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
    fn a_continuation_is_weighted_by_the_path_probability() {
        // 先で別に求めた期待支払いを、その経路を引く確率でそのまま重み付けする。
        let path = SelfTsumoPath::immediate(4, 100).expect("経路を作れる");
        let value = 7 * SELF_TSUMO_VALUE_SCALE;
        assert_eq!(path.weighted_continuation(value), value * 4 / 100);
        assert_eq!(path.weighted_continuation(0), 0);
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
    fn the_production_soft_horizon_shortens_raw_draws_with_a_late_minimum() {
        let horizon = SelfTsumoHorizon::PRODUCTION;
        assert_eq!(horizon, SelfTsumoHorizon::default());
        assert_eq!(
            (horizon.horizon_turn, horizon.late_min_future_draws),
            (12, 2)
        );
        for (raw, effective) in [
            (17, 11),
            (16, 10),
            (12, 6),
            (10, 4),
            (9, 3),
            (8, 2),
            (7, 2),
            (3, 2),
            (2, 2),
            (1, 1),
            (0, 0),
        ] {
            assert_eq!(horizon.effective_future_draws(raw), effective, "raw {raw}");
        }
    }

    #[test]
    fn the_until_ryukyoku_horizon_keeps_the_raw_draws() {
        for raw in 0..=20 {
            assert_eq!(
                SelfTsumoHorizon::UNTIL_RYUKYOKU.effective_future_draws(raw),
                raw
            );
            // horizon 18 は late minimum に依らず従来どおり。
            for late_min in 0..=4 {
                assert_eq!(soft_horizon_future_draws(raw, 18, late_min), raw);
            }
            assert_eq!(soft_horizon_future_draws(raw, 20, 2), raw);
        }
    }

    #[test]
    fn a_horizon_turn_of_eighteen_or_more_is_until_ryukyoku_for_any_late_minimum() {
        let horizon = |horizon_turn, late_min_future_draws| SelfTsumoHorizon {
            horizon_turn,
            late_min_future_draws,
        };
        assert!(SelfTsumoHorizon::UNTIL_RYUKYOKU.is_until_ryukyoku());
        assert!(horizon(18, 0).is_until_ryukyoku());
        assert!(horizon(18, 2).is_until_ryukyoku());
        assert!(horizon(20, 2).is_until_ryukyoku());
        assert!(!SelfTsumoHorizon::PRODUCTION.is_until_ryukyoku());
        for late_min in 0..=4 {
            assert!(!horizon(17, late_min).is_until_ryukyoku());
        }

        // 判定は raw をそのまま使う soft horizon の semantics と一致する。
        for horizon_turn in 0..=22 {
            for late_min in 0..=4 {
                let candidate = horizon(horizon_turn, late_min);
                let keeps_raw = (0..=20).all(|raw| candidate.effective_future_draws(raw) == raw);
                assert_eq!(
                    candidate.is_until_ryukyoku(),
                    keeps_raw,
                    "{horizon_turn} {late_min}"
                );
            }
        }
    }

    #[test]
    fn the_soft_horizon_never_exceeds_the_raw_draws() {
        for horizon_turn in 0..=20 {
            for late_min in 0..=5 {
                for raw in 0..=20 {
                    let effective = soft_horizon_future_draws(raw, horizon_turn, late_min);
                    assert!(effective <= raw, "{horizon_turn} {late_min} {raw}");
                    assert!(
                        effective >= raw.min(late_min),
                        "{horizon_turn} {late_min} {raw}"
                    );
                }
            }
        }
        // raw < late_min では raw のまま。
        assert_eq!(soft_horizon_future_draws(1, 12, 2), 1);
        assert_eq!(soft_horizon_future_draws(0, 12, 2), 0);
        assert_eq!(soft_horizon_future_draws(2, 12, 3), 2);
    }

    #[test]
    fn the_effective_draws_grow_with_the_horizon_turn() {
        let effective = |horizon_turn, raw| soft_horizon_future_draws(raw, horizon_turn, 2);
        assert_eq!(
            [12, 14, 16, 18].map(|turn| effective(turn, 16)),
            [10, 12, 14, 16]
        );
        assert_eq!(
            [12, 14, 16, 18].map(|turn| effective(turn, 8)),
            [2, 4, 6, 8]
        );
        assert_eq!(
            [12, 14, 16, 18].map(|turn| effective(turn, 4)),
            [2, 2, 2, 4]
        );
        assert_eq!(
            [12, 14, 16, 18].map(|turn| effective(turn, 1)),
            [1, 1, 1, 1]
        );
    }

    #[test]
    fn a_late_minimum_of_two_leaves_one_draw_after_the_progress() {
        let terminal = TenpaiTsumoValue {
            winning_remaining: 4,
            weighted_total: 4 * 3900,
        };
        let progress = SelfTsumoPath::immediate(8, 100).expect("経路を作れる");
        let facts = |horizon: SelfTsumoHorizon| SelfTsumoFacts {
            unknown_tiles: 100,
            own_future_draws: horizon.effective_future_draws(5),
        };

        let production = facts(SelfTsumoHorizon::PRODUCTION);
        assert_eq!(production.own_future_draws, 2);
        assert_eq!(progress.terminal_own_future_draws(production), 1);
        assert!(progress.expected_payment(production, terminal) > 0);

        // late minimum 1 では Progress でテンパイした時点で自摸機会が残らない。
        let late_one = facts(SelfTsumoHorizon {
            horizon_turn: 12,
            late_min_future_draws: 1,
        });
        assert_eq!(late_one.own_future_draws, 1);
        assert_eq!(progress.terminal_own_future_draws(late_one), 0);
        assert_eq!(progress.expected_payment(late_one, terminal), 0);
    }

    #[test]
    fn same_shanten_steps_consume_the_effective_draws() {
        let facts = SelfTsumoFacts {
            unknown_tiles: 100,
            own_future_draws: SelfTsumoHorizon::PRODUCTION.effective_future_draws(10),
        };
        assert_eq!(facts.own_future_draws, 4);
        let immediate = SelfTsumoPath::immediate(4, 100).expect("経路を作れる");
        let once = SelfTsumoPath::via_same_shanten(4, 4, 100).expect("経路を作れる");
        let twice = SelfTsumoPath::via_same_shanten_twice(4, 4, 4, 100).expect("経路を作れる");
        assert_eq!(
            [immediate, once, twice].map(|path| path.terminal_own_future_draws(facts)),
            [3, 2, 1]
        );

        let late = SelfTsumoFacts {
            own_future_draws: SelfTsumoHorizon::PRODUCTION.effective_future_draws(6),
            ..facts
        };
        assert_eq!(late.own_future_draws, 2);
        assert_eq!(
            [immediate, once, twice].map(|path| path.terminal_own_future_draws(late)),
            [1, 0, 0]
        );
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
