use std::cmp::Ordering;
use std::time::{Duration, Instant};

use bot_logic::{
    CompletedHandAnalysis, HandSettlementError, HandValueError, HandValueOutcome, Meld,
    RiichiStatus, TileId, TileType, WinMethod, WinningContext, analyze_completed_hand,
    evaluate_hand_value, evaluate_ron_deal_in_payment, seen_red_fives,
};

use crate::action::LegalAction;
use crate::context::GameContext;
use crate::meld::fixed_meld_count;

use super::hidden_hand_states::{HiddenHandStateUnsupported, ReachedHiddenHandStates};
use super::wait_candidates::remaining_tile_copies;
use super::{CompressedHiddenHandStates, RonRiskEvidence};

type HandCounts = [u8; TileType::COUNT];

const MAX_TILE_COPIES: u8 = 4;

// リーチ者の隠れ手牌枚数の上限。固定面子1つごとに3枚減る。
const CONCEALED_HAND_LEN: u8 = 13;

// 牌種ごとの残枚数 (最大4枚) から k 枚を選ぶ組み合わせ数。
const TILE_COPY_COMBINATIONS: [[u128; 5]; 5] = [
    [1, 0, 0, 0, 0],
    [1, 1, 0, 0, 0],
    [1, 2, 1, 0, 0],
    [1, 3, 3, 1, 0],
    [1, 4, 6, 4, 1],
];

/// 単独リーチ相手への structural expected deal-in loss を確定できない理由。
///
/// 推測値を返さないための区別で、「期待損失が0」という結論とは別物。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StructuralExpectedDealInLossUnavailable {
    /// 既存 exact hidden-hand model がその player を扱えない。
    HiddenHandModel(HiddenHandStateUnsupported),
    /// exact `T(p)` が0で、確率測度として使えない。
    ZeroTenpaiWeight,
    /// 場風が不明。
    UnknownRoundWind,
    /// 対象リーチ者の自風が不明。
    UnknownSeatWind,
    /// 山の残りツモ可能枚数が不明で、河底が確定しない。
    UnknownRemainingTiles,
    /// 対象リーチ者のリーチが通常立直かダブル立直か確定しない。
    UnknownDoubleRiichi,
    /// 対象リーチ者の一発成立が確定しない。
    UnknownIppatsu,
    /// 裏ドラ表示牌を配れる未知牌が足りず、裏ドラの平均を取れない。
    ZeroUraArrangementWeight,
    /// scoring layer が完成手を評価できない。
    HandValue(HandValueError),
    /// リーチ者の和了形に scoring candidate が無い。
    NoScoringCandidate,
    /// bonus 翻を確定できず、scoring layer が打点を確定しない。
    IndeterminateBonusHan,
    /// 本場などの settlement 事実が足りず、支払点を確定できない。
    Settlement(HandSettlementError),
    /// `R/T` の state space と数え直した weight が一致しない。
    InconsistentModel,
    /// 整数 evidence が `u128` に収まらない。
    Overflow,
}

/// 単独リーチ相手への structural expected deal-in loss の整数 evidence。
///
/// 期待値は
///
/// ```text
/// expected loss = loss_weighted_sum / (tenpai_weight * ura_arrangement_weight)
/// ```
///
/// で、分子・分母ともに整数のまま保持する。浮動小数点へ変換するのは表示のときだけで、比較や
/// 集計はこの整数 evidence を source of truth にする。
///
/// これは empirical な放銃損失ではなく、既存 `R/T` と同じ combinatorial hidden-hand model 上の
/// 期待値である。`tenpai_weight` は
/// [`RonRiskEvidence::tenpai_weight`](super::RonRiskEvidence::tenpai_weight) そのものなので、
/// `R/T` と並べて読める。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct StructuralExpectedDealInLossEvidence {
    /// `Σ_H Σ_U w(H) * w(U) * ロン支払点` の整数分子 [点 × weight]。
    pub loss_weighted_sum: u128,
    /// exact `T(p)`。既存 `R/T` の分母と同じ値。
    pub tenpai_weight: u128,
    /// 裏ドラ表示牌 slot の全配置の weight 総和。裏ドラ slot が無い局面では `1`。
    pub ura_arrangement_weight: u128,
}

impl StructuralExpectedDealInLossEvidence {
    /// 期待値の整数分母 `tenpai_weight * ura_arrangement_weight`。
    ///
    /// `u128` に収まらない場合は飽和させず `None`。
    pub fn weight_denominator(self) -> Option<u128> {
        self.tenpai_weight
            .checked_mul(self.ura_arrangement_weight)
            .filter(|denominator| *denominator != 0)
    }

    /// 表示専用の期待損失 [点]。比較・集計には使わない。
    pub fn expected_loss(self) -> Option<f64> {
        let denominator = self.weight_denominator()?;
        Some(self.loss_weighted_sum as f64 / denominator as f64)
    }

    /// 2つの期待損失を浮動小数点なしで比較する。
    ///
    /// 分母が0、または cross multiplication が `u128` に収まらない場合は推測せず `None`。
    pub fn compare_expected_loss(self, other: &Self) -> Option<Ordering> {
        let left = self
            .loss_weighted_sum
            .checked_mul(other.weight_denominator()?)?;
        let right = other
            .loss_weighted_sum
            .checked_mul(self.weight_denominator()?)?;
        Some(left.cmp(&right))
    }
}

/// expected loss を求めるためにかかった数え上げ規模。値そのものには影響しない。
///
/// `ron_capable_states` は既存 `R` の数え上げが加算した state 数、`red_five_variants` はその
/// state を赤5込みの物理配置へ分解した件数、`scoring_evaluations` は既存 scoring layer を呼んだ
/// 回数で、裏ドラ配置ごとに1回数える。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct StructuralExpectedDealInLossMetrics {
    pub ron_capable_states: u64,
    pub red_five_variants: u64,
    pub scoring_evaluations: u64,
    pub elapsed: Duration,
}

/// 単独リーチ相手への structural expected deal-in loss を exact に求める。
///
/// `discard` は今から自分が切る物理牌で、その1枚は
/// [`GameContext::visible_tiles`] に反映済みであることを前提にする。赤5かどうかは
/// `discard` の物理牌 identity をそのまま scoring へ渡す。
///
/// 確率測度は既存 `R/T` と同じ physical combination weight だけで、牌譜統計・相手の打牌傾向・
/// 経験的な放銃率や打点分布は使わない。数え上げる state space も weight も
/// [`ReachedHiddenHandStates`] のものそのままで、期待損失のために別の hidden-hand prior を
/// 作らない。
///
/// 打点は既存 hand-value / payment / settlement layer だけで求める。含めるのは相手のリーチ・
/// 場風・相手の自風・通常ドラ・赤ドラ・裏ドラ・ロン和了時の役と符、そして本場による追加支払い
/// で、供託は「この打牌で追加で失う点」ではないので含めない。
///
/// これは diagnostics-only の reference enumeration で、production の `act()` からは呼ばない。
/// 完成手を1状態ずつ評価するため、実局面では数百万 state 規模の評価になる。
pub fn structural_expected_deal_in_loss(
    player: usize,
    discard: TileId,
    context: &GameContext,
) -> Result<StructuralExpectedDealInLossEvidence, StructuralExpectedDealInLossUnavailable> {
    structural_expected_deal_in_loss_with_metrics(
        player,
        discard,
        context,
        &mut StructuralExpectedDealInLossMetrics::default(),
    )
}

/// 数え上げ規模も受け取る [`structural_expected_deal_in_loss`]。
pub fn structural_expected_deal_in_loss_with_metrics(
    player: usize,
    discard: TileId,
    context: &GameContext,
    metrics: &mut StructuralExpectedDealInLossMetrics,
) -> Result<StructuralExpectedDealInLossEvidence, StructuralExpectedDealInLossUnavailable> {
    use StructuralExpectedDealInLossUnavailable as Unavailable;

    let start = Instant::now();
    let target = discard.tile_type();
    let ron_risk = structural_ron_risk_evidence(player, context, target)?;
    let ura_arrangement_weight =
        ura_arrangement_weight(concealed_hand_len(player, context)?, context)?;

    // 現物のようにロン可能 state が無い場合、期待損失は scoring 事実に依らず exact に0なので、
    // 打点を確定できるかどうかを問わない。
    if ron_risk.ron_capable_weight == 0 {
        metrics.elapsed = start.elapsed();
        return Ok(StructuralExpectedDealInLossEvidence {
            loss_weighted_sum: 0,
            tenpai_weight: ron_risk.tenpai_weight,
            ura_arrangement_weight,
        });
    }

    let mut states =
        ReachedHiddenHandStates::new(player, context).map_err(Unavailable::HiddenHandModel)?;
    let mut evaluator = RonLossEvaluator::new(player, discard, context)?;

    let mut loss_weighted_sum: Result<u128, Unavailable> = Ok(0);
    let enumerated = states.visit_ron_capable_states(target, &mut |hand, weight| {
        let Ok(accumulated) = loss_weighted_sum else {
            return;
        };
        loss_weighted_sum = evaluator
            .state_loss_weighted_sum(hand, weight, metrics)
            .and_then(|state_sum| {
                accumulated
                    .checked_add(state_sum)
                    .ok_or(Unavailable::Overflow)
            });
    });
    metrics.ron_capable_states = enumerated.states;
    metrics.elapsed = start.elapsed();

    // 期待損失は既存 `R/T` と同じ state space の上の値でなければならないので、数え直した `R` が
    // production evidence と食い違う場合は値を返さない。
    if enumerated.weight != ron_risk.ron_capable_weight {
        return Err(Unavailable::InconsistentModel);
    }

    Ok(StructuralExpectedDealInLossEvidence {
        loss_weighted_sum: loss_weighted_sum?,
        tenpai_weight: ron_risk.tenpai_weight,
        ura_arrangement_weight,
    })
}

/// 期待損失の分母に使う exact `R/T` evidence。production comparator と同じ compressed model から
/// 取る。
fn structural_ron_risk_evidence(
    player: usize,
    context: &GameContext,
    target: TileType,
) -> Result<RonRiskEvidence, StructuralExpectedDealInLossUnavailable> {
    use StructuralExpectedDealInLossUnavailable as Unavailable;

    let mut states =
        CompressedHiddenHandStates::new(player, context).map_err(Unavailable::HiddenHandModel)?;
    let evidence = states.ron_risk_evidence(target);
    if evidence.tenpai_weight == 0 {
        return Err(Unavailable::ZeroTenpaiWeight);
    }
    Ok(evidence)
}

/// 既存 exact model と同じ、打牌後の concealed hand 枚数 (`13 - 3 * 固定面子数`)。
fn concealed_hand_len(
    player: usize,
    context: &GameContext,
) -> Result<u8, StructuralExpectedDealInLossUnavailable> {
    use StructuralExpectedDealInLossUnavailable as Unavailable;

    let melds = context
        .melds_of(player)
        .ok_or(Unavailable::HiddenHandModel(
            HiddenHandStateUnsupported::UnknownPlayer,
        ))?;
    let fixed = fixed_meld_count(melds).ok_or(Unavailable::HiddenHandModel(
        HiddenHandStateUnsupported::TooManyMelds,
    ))?;
    Ok(CONCEALED_HAND_LEN - 3 * fixed.get())
}

/// 裏ドラ表示牌 slot の全配置の weight 総和。
///
/// リーチ者の隠れ手牌を仮定したあとに残る未知牌の枚数は state に依らず
/// `Σ remaining[t] - concealed_hand_len` で一定なので、この総和も state に依らず一定になる。
fn ura_arrangement_weight(
    concealed_hand_len: u8,
    context: &GameContext,
) -> Result<u128, StructuralExpectedDealInLossUnavailable> {
    use StructuralExpectedDealInLossUnavailable as Unavailable;

    let unseen: u32 = TileType::all()
        .map(|tile| u32::from(remaining_tile_copies(tile, context)))
        .sum();
    let pool = unseen.saturating_sub(u32::from(concealed_hand_len));
    let slots = context.dora_indicators().len() as u32;
    combinations(pool, slots)
        .filter(|weight| *weight != 0)
        .ok_or(Unavailable::ZeroUraArrangementWeight)
}

// 1状態の「支払点 × physical weight」を既存 scoring layer だけで求める評価器。
//
// 赤5を含む物理配置と裏ドラ表示牌の配置は、どちらも physical combination weight で加重平均する。
// 固定係数や平均翻数は持たない。
struct RonLossEvaluator<'a> {
    fixed_melds: &'a [Meld],
    dora_indicators: &'a [TileId],
    winning_context: WinningContext,
    discard: TileId,
    honba: Option<u32>,
    remaining: HandCounts,
    red_five_seen: [bool; TileType::COUNT],
    red_five_tile_types: Vec<TileType>,
    ura_slots: usize,
    hand_ids: Vec<TileId>,
    ura_ids: Vec<TileId>,
}

impl<'a> RonLossEvaluator<'a> {
    fn new(
        player: usize,
        discard: TileId,
        context: &'a GameContext,
    ) -> Result<Self, StructuralExpectedDealInLossUnavailable> {
        use StructuralExpectedDealInLossUnavailable as Unavailable;

        let fixed_melds = context
            .melds_of(player)
            .ok_or(Unavailable::HiddenHandModel(
                HiddenHandStateUnsupported::UnknownPlayer,
            ))?;
        let mut remaining = [0u8; TileType::COUNT];
        for tile in TileType::all() {
            remaining[tile.index()] = remaining_tile_copies(tile, context);
        }

        Ok(Self {
            fixed_melds,
            dora_indicators: context.dora_indicators(),
            winning_context: ron_winning_context(player, context)?,
            discard,
            honba: context.honba(),
            remaining,
            red_five_seen: seen_red_fives(context.visible_tiles().iter().copied()),
            red_five_tile_types: TileType::all()
                .filter(|tile| TileId::copies(*tile).any(TileId::is_red))
                .collect(),
            ura_slots: context.dora_indicators().len(),
            hand_ids: Vec::with_capacity(usize::from(CONCEALED_HAND_LEN) + 1),
            ura_ids: Vec::with_capacity(context.dora_indicators().len()),
        })
    }

    // 隠れ手牌1状態分の「支払点 × physical weight」。
    //
    // state の weight を赤5を含む物理配置へ分解し、それぞれで既存 scoring を通す。分解した
    // weight の合計は元の weight と一致しなければならない。
    fn state_loss_weighted_sum(
        &mut self,
        hand: &HandCounts,
        weight: u128,
        metrics: &mut StructuralExpectedDealInLossMetrics,
    ) -> Result<u128, StructuralExpectedDealInLossUnavailable> {
        use StructuralExpectedDealInLossUnavailable as Unavailable;

        let base_weight = self.non_red_capable_weight(hand);
        let mut total = 0u128;
        let mut split_weight = 0u128;
        for mask in 0..(1u32 << self.red_five_tile_types.len()) {
            let Some(variant_weight) = self.red_variant_weight(hand, mask, base_weight) else {
                continue;
            };
            split_weight = split_weight
                .checked_add(variant_weight)
                .ok_or(Unavailable::Overflow)?;
            if variant_weight == 0 {
                continue;
            }
            metrics.red_five_variants += 1;
            total = total
                .checked_add(self.red_variant_loss_weighted_sum(
                    hand,
                    mask,
                    variant_weight,
                    metrics,
                )?)
                .ok_or(Unavailable::Overflow)?;
        }

        // 赤5の分解は元の physical weight をそのまま分けたものなので、合計が変わってはいけない。
        if split_weight != weight {
            return Err(Unavailable::InconsistentModel);
        }
        Ok(total)
    }

    // 赤5を持ち得ない牌種だけの physical combination weight。
    fn non_red_capable_weight(&self, hand: &HandCounts) -> u128 {
        let mut weight = 1u128;
        for tile in TileType::all() {
            let count = hand[tile.index()];
            if count == 0 || self.red_five_tile_types.contains(&tile) {
                continue;
            }
            weight *= tile_copy_combinations(self.remaining[tile.index()], count);
        }
        weight
    }

    // 赤5の割り当て1通りの physical combination weight。割り当て自体が成立しない mask は `None`。
    //
    // 赤5がまだ見えていない牌種では、残枚数のうち1枚が赤5なので「赤5を含む配置」と「黒5だけの
    // 配置」へ `C(remaining - 1, count - 1)` と `C(remaining - 1, count)` で分かれる。赤5が既に
    // 見えている牌種は黒5だけの配置しかない。2つを足すと元の `C(remaining, count)` に戻る。
    fn red_variant_weight(&self, hand: &HandCounts, mask: u32, base_weight: u128) -> Option<u128> {
        let mut weight = base_weight;
        for (index, tile) in self.red_five_tile_types.iter().enumerate() {
            let count = hand[tile.index()];
            let remaining = self.remaining[tile.index()];
            let holds_red = mask & (1 << index) != 0;
            if holds_red && count == 0 {
                return None;
            }
            let seen = self.red_five_seen[tile.index()];
            weight *= match (holds_red, seen) {
                (true, true) => 0,
                (true, false) => tile_copy_combinations(remaining.saturating_sub(1), count - 1),
                (false, true) => tile_copy_combinations(remaining, count),
                (false, false) => tile_copy_combinations(remaining.saturating_sub(1), count),
            };
            if weight == 0 {
                return Some(0);
            }
        }
        Some(weight)
    }

    // 赤5の割り当てを固定した物理配置1通り分の「支払点 × physical weight」。
    //
    // 裏ドラ表示牌 slot の配置をすべて数え上げ、physical combination weight で加重する。
    fn red_variant_loss_weighted_sum(
        &mut self,
        hand: &HandCounts,
        red_five_mask: u32,
        variant_weight: u128,
        metrics: &mut StructuralExpectedDealInLossMetrics,
    ) -> Result<u128, StructuralExpectedDealInLossUnavailable> {
        use StructuralExpectedDealInLossUnavailable as Unavailable;

        self.hand_ids.clear();
        for tile in TileType::all() {
            let count = hand[tile.index()];
            if count == 0 {
                continue;
            }
            let holds_red = self.holds_red_five(tile, red_five_mask);
            for id in concealed_copies(tile, count, holds_red) {
                self.hand_ids.push(id);
            }
        }
        self.hand_ids.push(self.discard);

        let analysis = analyze_completed_hand(&self.hand_ids, self.fixed_melds)
            .map_err(|_| Unavailable::InconsistentModel)?;

        let mut pool = self.remaining;
        for tile in TileType::all() {
            pool[tile.index()] = pool[tile.index()].saturating_sub(hand[tile.index()]);
        }

        let mut weighted_payment = 0u128;
        let mut ura = UraArrangements {
            pool,
            slots: self.ura_slots,
            ids: std::mem::take(&mut self.ura_ids),
        };
        let result = ura.for_each(&mut |indicators, ura_weight| {
            metrics.scoring_evaluations += 1;
            let payment = self.ron_deal_in_payment(&analysis, indicators)?;
            let contribution = ura_weight
                .checked_mul(u128::from(payment))
                .ok_or(Unavailable::Overflow)?;
            weighted_payment = weighted_payment
                .checked_add(contribution)
                .ok_or(Unavailable::Overflow)?;
            Ok(())
        });
        self.ura_ids = ura.ids;
        result?;

        variant_weight
            .checked_mul(weighted_payment)
            .ok_or(Unavailable::Overflow)
    }

    // この割り当てで対象牌種の赤5を隠れ手牌へ入れるか。
    fn holds_red_five(&self, tile: TileType, red_five_mask: u32) -> bool {
        self.red_five_tile_types
            .iter()
            .position(|candidate| *candidate == tile)
            .is_some_and(|index| red_five_mask & (1 << index) != 0)
    }

    // 裏ドラ表示牌を固定した完成手1通りの、放銃者が失う点数。
    fn ron_deal_in_payment(
        &self,
        analysis: &CompletedHandAnalysis,
        ura_dora_indicators: &[TileId],
    ) -> Result<u32, StructuralExpectedDealInLossUnavailable> {
        use StructuralExpectedDealInLossUnavailable as Unavailable;

        let outcome = evaluate_hand_value(
            analysis,
            self.winning_context,
            self.discard.tile_type(),
            self.dora_indicators,
            Some(ura_dora_indicators),
        )
        .map_err(Unavailable::HandValue)?;
        let hand_value = match outcome {
            HandValueOutcome::Known(hand_value) => hand_value,
            HandValueOutcome::NoCandidate => return Err(Unavailable::NoScoringCandidate),
            HandValueOutcome::IndeterminateBonusHan => {
                return Err(Unavailable::IndeterminateBonusHan);
            }
        };
        evaluate_ron_deal_in_payment(&hand_value, self.honba).map_err(Unavailable::Settlement)
    }
}

/// リーチ者がこの打牌でロンした場合の和了状況。
///
/// 場風・自風・リーチの種別・一発・河底は履歴と table state を source of truth にし、確定
/// できない事実を補完しない。嶺上開花と槍槓は「通常の打牌でロンされる」評価そのものから
/// `false` が確定するので、観測事実として渡す。
fn ron_winning_context(
    player: usize,
    context: &GameContext,
) -> Result<WinningContext, StructuralExpectedDealInLossUnavailable> {
    use StructuralExpectedDealInLossUnavailable as Unavailable;

    let round_wind = context.round_wind().ok_or(Unavailable::UnknownRoundWind)?;
    let seat_wind = context
        .seat_wind_of(player)
        .ok_or(Unavailable::UnknownSeatWind)?;
    let remaining_live_tiles = context
        .remaining_tiles()
        .ok_or(Unavailable::UnknownRemainingTiles)?;
    let riichi = match context.declared_double_riichi_of(player) {
        Some(true) => RiichiStatus::DoubleRiichi,
        Some(false) => RiichiStatus::Riichi,
        None => return Err(Unavailable::UnknownDoubleRiichi),
    };
    let ippatsu = context
        .ippatsu_of(player)
        .ok_or(Unavailable::UnknownIppatsu)?;

    Ok(WinningContext::new(WinMethod::Ron)
        .with_round_wind(Some(round_wind))
        .with_seat_wind(Some(seat_wind))
        .with_riichi(riichi)
        .with_ippatsu(Some(ippatsu))
        .with_rinshan(Some(false))
        .with_chankan(Some(false))
        .with_remaining_live_tiles(Some(remaining_live_tiles)))
}

// 裏ドラ表示牌 slot の配置1通りを受け取る観測。代表の物理牌列とその配置の weight。
type UraArrangementVisit<'v> =
    &'v mut dyn FnMut(&[TileId], u128) -> Result<(), StructuralExpectedDealInLossUnavailable>;

// 裏ドラ表示牌 slot の配置を、物理牌の組み合わせ数で加重しながら数え上げる。
//
// 打点が変わるのは表示牌の牌種だけなので、牌種の多重集合ごとに1回評価し、その多重集合を作る
// 物理牌の組み合わせ数 `Π C(pool[t], m[t])` を weight にする。表示牌の赤 / 黒は通常ドラの判定に
// 影響しないので、代表の物理牌を渡す。多重集合の weight の総和は
// `C(Σ pool, slots)` になり、これが期待値の裏ドラ側の分母になる。
struct UraArrangements {
    pool: HandCounts,
    slots: usize,
    ids: Vec<TileId>,
}

impl UraArrangements {
    fn for_each(
        &mut self,
        visit: UraArrangementVisit<'_>,
    ) -> Result<(), StructuralExpectedDealInLossUnavailable> {
        self.ids.clear();
        self.recurse(0, self.slots, 1, visit)
    }

    fn recurse(
        &mut self,
        start: usize,
        slots: usize,
        weight: u128,
        visit: UraArrangementVisit<'_>,
    ) -> Result<(), StructuralExpectedDealInLossUnavailable> {
        if slots == 0 {
            return visit(&self.ids, weight);
        }
        for index in start..TileType::COUNT {
            let available = usize::from(self.pool[index]).min(slots);
            let tile = TileType::new(index as u8).expect("index is a valid tile type");
            for count in 1..=available {
                let combinations = tile_copy_combinations(self.pool[index], count as u8);
                for copy in 0..count {
                    self.ids.push(indicator_copy(tile, copy as u8));
                }
                self.recurse(index + 1, slots - count, weight * combinations, visit)?;
                self.ids.truncate(self.ids.len() - count);
            }
        }
        Ok(())
    }
}

// 隠れ手牌へ割り当てる物理牌。赤5を含む場合だけ赤牌を使い、残りは黒牌を順に使う。
//
// 同じ赤 / 黒の物理牌はどれを選んでも打点が同じなので、identity ではなく赤かどうかだけを
// 固定する。
fn concealed_copies(tile: TileType, count: u8, holds_red: bool) -> impl Iterator<Item = TileId> {
    let red = TileId::copies(tile)
        .find(|id| id.is_red())
        .filter(|_| holds_red);
    let black_count = count - u8::from(red.is_some());
    red.into_iter().chain(
        TileId::copies(tile)
            .filter(|id| !id.is_red())
            .take(usize::from(black_count)),
    )
}

// 裏ドラ表示牌の代表物理牌。通常ドラの判定は牌種だけを見るので、赤5でない物理牌を使う。
fn indicator_copy(tile: TileType, copy: u8) -> TileId {
    TileId::copies(tile)
        .filter(|id| !id.is_red())
        .nth(usize::from(copy))
        .or_else(|| TileId::copies(tile).next())
        .expect("every tile type has physical copies")
}

fn tile_copy_combinations(remaining: u8, count: u8) -> u128 {
    if remaining > MAX_TILE_COPIES || count > MAX_TILE_COPIES {
        return 0;
    }
    TILE_COPY_COMBINATIONS[usize::from(remaining)][usize::from(count)]
}

// `C(n, k)` を飽和させずに求める。収まらない場合は `None`。
fn combinations(n: u32, k: u32) -> Option<u128> {
    if k > n {
        return Some(0);
    }
    let k = k.min(n - k);
    let mut value = 1u128;
    for step in 0..k {
        value = value.checked_mul(u128::from(n - step))?;
        value /= u128::from(step + 1);
    }
    Some(value)
}

/// 通常打牌 selector が選んだ打牌について、単独リーチ相手への structural expected deal-in loss を
/// 既存 `R/T` と並べて持つ diagnostics-only の観測値。
///
/// `ron_risk` は production comparator が使う exact evidence そのもので、`expected_loss` はその
/// `T(p)` を分母に共有する。どちらも combinatorial hidden-hand model 上の値で、empirical な
/// 放銃率でも実際の期待失点でもない。
///
/// この診断は production の Push/Pull にも Defense selection にも接続しておらず、構築の有無で
/// 選択結果は変わらない。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StructuralExpectedDealInLossDiagnostic {
    /// 評価した打牌の物理牌。赤5かどうかもそのまま保持する。
    pub discard: TileId,
    /// 唯一のリーチ者の席。
    pub player: usize,
    /// 既存 exact `R/T` evidence。exact model が使えない局面では `None`。
    pub ron_risk: Option<RonRiskEvidence>,
    /// 同じ state space 上の期待放銃損失。確定できない場合は理由をそのまま保持する。
    pub expected_loss:
        Result<StructuralExpectedDealInLossEvidence, StructuralExpectedDealInLossUnavailable>,
    /// 数え上げ規模。値には影響しない。
    pub metrics: StructuralExpectedDealInLossMetrics,
}

/// 単独リーチ局面の1打牌について diagnostics-only の期待放銃損失を構築する。
///
/// 対象は他家リーチがちょうど1人の局面の通常 Dahai 1候補だけで、それ以外では `None` を返す。
/// 複数リーチの合成も、非リーチ相手の推定も行わない。
pub fn diagnose_structural_expected_deal_in_loss(
    context: &GameContext,
    action: &LegalAction,
) -> Option<StructuralExpectedDealInLossDiagnostic> {
    let LegalAction::Dahai { tile } = action else {
        return None;
    };
    let [player] = context.reached_opponents()[..] else {
        return None;
    };

    let mut metrics = StructuralExpectedDealInLossMetrics::default();
    let expected_loss =
        structural_expected_deal_in_loss_with_metrics(player, *tile, context, &mut metrics);
    Some(StructuralExpectedDealInLossDiagnostic {
        discard: *tile,
        player,
        ron_risk: CompressedHiddenHandStates::new(player, context)
            .ok()
            .map(|mut states| states.ron_risk_evidence(tile.tile_type())),
        expected_loss,
        metrics,
    })
}
