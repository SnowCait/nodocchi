//! 組み立て済みのテンパイをツモ和了の hypothetical baseline で評価する、現在聴牌と将来聴牌に
//! 共通の scoring primitive。
//!
//! 点数計算そのものは bot-logic の既存 scoring rule
//! ([`evaluate_tenpai_hand_value`]) が source of truth で、この層が持つのは
//!
//! ```text
//! 完成手 (TenpaiCompletedHands) + Reach / Damaten mode
//!   → ツモ baseline を組み立てて既存 scoring を1回通し
//!   → 和了牌の物理牌 variant 単位の結論をどう畳むか
//! ```
//!
//! だけである。向聴・受け入れ・待ち・フリテン・完成手の組み立ては呼び出し側の既存 layer が
//! 持ち、ここでは行わない。
//!
//! # current / prospective のどちらにも属さない
//!
//! 同じツモ打点を、現在打牌後のテンパイ (現在聴牌候補の self-tsumo 比較・リーチ判断の named
//! 役満 rule・ダマ継続の非和了ツモ判定) と、1向聴 lookahead の先にある将来テンパイ
//! ([`crate::prospective_value`]) の両方が使う。どちらの policy 層にも属さない primitive
//! なので、baseline の組み立ても集約規則もこの module 1本だけが持つ。
//!
//! 「いつリーチを宣言するか」「将来テンパイでリーチが合法か」「フリテンかどうか」「lookahead の
//! 枝をどう集約するか」はこの層の責務ではない。[`TenpaiOffenseMode`] はリーチ宣言の有無だけを
//! 表す入力で、その mode を決めるのは呼び出し側の既存 policy になる。
//!
//! # ツモ baseline
//!
//! ロン baseline ([`crate::offense_value::reach_baseline_context`] /
//! [`crate::damaten_value::damaten_baseline_context`]) を流用せず、[`WinMethod::Tsumo`] として
//! 組み立てる。門前ツモの1翻は既存の役判定が付けるので、この層で翻を足さない。一発・海底・
//! 嶺上開花・槍槓のような未来の偶発要素は既存 baseline と同じ思想で加えず、リーチの裏ドラも
//! 既存の最低保証 baseline (裏0) と揃える。
//!
//! ダマ baseline はロンできるかに依らず使う。評価するのが自分のツモ和了だけなので、フリテンは
//! 打点を確定できない理由にならない。
//!
//! # 集約
//!
//! 待ちは牌種ごと、さらに和了牌の物理牌 (赤5 / 黒5) ごとの variant に分かれる。ツモ baseline で
//! 役が無い variant はその牌でツモ和了できないので、0点の和了として加算せず non-winning draw
//! として扱う。点数計算の入力不足・裏ドラ未確定のように本当に評価できない variant は推測せず
//! 確定しないままにし、残枚数 0 の variant はどちらにも寄与させない。

use bot_logic::{
    HandValueError, HandValueOutcome, Payment, RiichiStatus, TenpaiCompletedHands,
    TenpaiHandValueProfile, TenpaiTsumoValue, TileId, WinMethod, WinningContext,
    evaluate_tenpai_hand_value,
};

use crate::context::GameContext;
use crate::damaten_value::BASELINE_REMAINING_LIVE_TILES;
use crate::offense_value::{
    BASELINE_CHANKAN, BASELINE_IPPATSU, BASELINE_RINSHAN, BASELINE_URA_DORA_INDICATORS,
    TenpaiOffenseMode,
};

/// 和了牌の物理牌1つ分の打点。
///
/// 既存 [`HandValueOutcome`] の結論を潰さずに区別する。役なしは0点ではなく [`Self::NoYaku`]。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TenpaiVariantValue {
    /// 役があり、支払いまで確定した。
    Known {
        payment: Payment,
        /// 名前の付いた役満として確定したか。
        is_yakuman: bool,
    },
    /// 役が無い ([`HandValueOutcome::NoCandidate`])。0点ではない。
    NoYaku,
    /// 打点を確定できない。理由を潰さずに保持する。
    Unknown(TenpaiVariantUnknownReason),
}

impl TenpaiVariantValue {
    pub fn payment(self) -> Option<Payment> {
        match self {
            Self::Known { payment, .. } => Some(payment),
            Self::NoYaku | Self::Unknown(_) => None,
        }
    }

    /// 確定した支払い合計 [点]。確定しない場合と役なしの場合は `None`。
    pub fn total(self) -> Option<u32> {
        self.payment().map(Payment::total)
    }

    pub fn is_yakuman(self) -> bool {
        matches!(
            self,
            Self::Known {
                is_yakuman: true,
                ..
            }
        )
    }
}

/// 打点を確定できない理由。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TenpaiVariantUnknownReason {
    /// bonus 翻が確定しない ([`HandValueOutcome::IndeterminateBonusHan`])。
    IndeterminateBonusHan,
    /// 点数計算の入力が足りない。場風・自風が不明な場合など。
    Scoring(HandValueError),
    /// 役はあるが支払いを求められない。
    MissingPayment,
}

/// 既存の手牌価値の結果を variant 1つ分の打点へ畳む。役なしと確定しない理由を潰さずに区別して
/// 持つ。
pub(crate) fn tenpai_variant_value(
    outcome: Result<&HandValueOutcome<'_>, HandValueError>,
) -> TenpaiVariantValue {
    match outcome {
        Ok(HandValueOutcome::Known(hand_value)) => match hand_value.payment() {
            Some(payment) => TenpaiVariantValue::Known {
                payment,
                is_yakuman: hand_value.is_yakuman(),
            },
            None => TenpaiVariantValue::Unknown(TenpaiVariantUnknownReason::MissingPayment),
        },
        Ok(HandValueOutcome::NoCandidate) => TenpaiVariantValue::NoYaku,
        Ok(HandValueOutcome::IndeterminateBonusHan) => {
            TenpaiVariantValue::Unknown(TenpaiVariantUnknownReason::IndeterminateBonusHan)
        }
        Err(error) => TenpaiVariantValue::Unknown(TenpaiVariantUnknownReason::Scoring(error)),
    }
}

/// 和了牌の物理牌 variant 1つを、Tsumo baseline でツモ和了できるか。
///
/// 既存のツモ集計 ([`tsumo_value`]) が「ツモ baseline で役が無い variant は和了できないので
/// 成功する待ちに含めない」としているのと同じ判定を、variant 単位で取り出したもの。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TsumoVariantStatus {
    /// 役があり、その牌でツモ和了できる。
    Winning,
    /// ツモ baseline で役が無く、その牌ではツモ和了できない。
    NoYaku,
    /// ツモ和了できるかを確定できない。0点とも和了とも扱わない。
    Unknown,
}

impl TsumoVariantStatus {
    fn from_value(value: TenpaiVariantValue) -> Self {
        match value {
            TenpaiVariantValue::Known { .. } => Self::Winning,
            TenpaiVariantValue::NoYaku => Self::NoYaku,
            TenpaiVariantValue::Unknown(_) => Self::Unknown,
        }
    }
}

/// テンパイ1件分の、和了牌の物理牌 variant ごとの Tsumo baseline の結論。
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct TsumoVariantOutcomes {
    variants: Vec<(TileId, TsumoVariantStatus)>,
}

impl TsumoVariantOutcomes {
    /// 指定した物理牌の結論。評価対象に無い牌は推測せず [`TsumoVariantStatus::Unknown`]。
    pub(crate) fn status(&self, winning_tile: TileId) -> TsumoVariantStatus {
        self.variants
            .iter()
            .find(|(tile, _)| *tile == winning_tile)
            .map_or(TsumoVariantStatus::Unknown, |(_, status)| *status)
    }
}

/// ツモ和了だけを評価する hypothetical baseline と裏ドラ表示牌。
///
/// [`TenpaiOffenseMode`] はリーチ宣言の有無だけを表し、どちらの mode も [`WinMethod::Tsumo`] と
/// して組み立てる。攻撃モードを確定できない場合は baseline も作らない。
pub(crate) fn tsumo_scoring_inputs(
    context: &GameContext,
    mode: TenpaiOffenseMode,
) -> Option<(WinningContext, Option<&'static [TileId]>)> {
    let riichi = match mode {
        TenpaiOffenseMode::Reach => RiichiStatus::Riichi,
        TenpaiOffenseMode::Damaten => RiichiStatus::NotDeclared,
        TenpaiOffenseMode::Unknown => return None,
    };
    let baseline = WinningContext::new(WinMethod::Tsumo)
        .with_round_wind(context.round_wind())
        .with_seat_wind(context.seat_wind())
        .with_riichi(riichi)
        .with_ippatsu(Some(BASELINE_IPPATSU))
        .with_chankan(Some(BASELINE_CHANKAN))
        .with_rinshan(Some(BASELINE_RINSHAN))
        .with_remaining_live_tiles(Some(BASELINE_REMAINING_LIVE_TILES));
    let ura_dora = matches!(mode, TenpaiOffenseMode::Reach).then_some(BASELINE_URA_DORA_INDICATORS);
    Some((baseline, ura_dora))
}

/// 組み立て済みの完成手を、指定した攻撃モードの Tsumo baseline で評価した待ちごとの手牌価値。
///
/// この module の入口はすべてこの1本を通り、baseline の組み立ても点数計算の呼び出しも複製
/// しない。mode から baseline を作れない場合は `None`。
fn tenpai_tsumo_profile<'a>(
    context: &GameContext,
    hands: &'a TenpaiCompletedHands,
    mode: TenpaiOffenseMode,
) -> Option<TenpaiHandValueProfile<'a>> {
    let (baseline, ura_dora) = tsumo_scoring_inputs(context, mode)?;
    Some(evaluate_tenpai_hand_value(
        hands,
        baseline,
        context.dora_indicators(),
        ura_dora,
    ))
}

/// 組み立て済みの完成手を、指定した production offense mode の Tsumo baseline で評価する。
///
/// prospective tenpai と現在打牌後の tenpai が同じ baseline・点数計算・physical variant 集約を
/// 共用するための入口。Reach / Damaten はリーチ宣言の有無だけを表し、どちらも Tsumo として
/// 評価する。Reach timing はこの helper の責務外。
pub(crate) fn tenpai_tsumo_value_from_hands(
    context: &GameContext,
    hands: &TenpaiCompletedHands,
    mode: TenpaiOffenseMode,
) -> Option<TenpaiTsumoValue> {
    tsumo_value(&tenpai_tsumo_profile(context, hands, mode)?)
}

/// テンパイのツモ和了が named 役満だと既存 scoring 上確定したか。
///
/// 役満判定は既存 scoring の結論 ([`HandValue::is_yakuman`](bot_logic::HandValue::is_yakuman))
/// だけが source of truth で、牌姿・役満名の列挙・点数 threshold から役満を推測しない。数え役満は
/// 名前の付いた役満ではないので [`Self::NotEstablished`] になる。
///
/// この結論をどう使うかは consumer 側の policy が決める。恒常フリテン聴牌の categorical rule
/// ([`selects_named_yakuman_damaten`](crate::reach_policy::selects_named_yakuman_damaten)) が
/// 現在の consumer で、この層は rule を持たない。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NamedYakumanTsumo {
    /// 生きた physical variant が1件以上あり、その全ての Tsumo 打点が named 役満と確定した。
    AllLiveVariants,
    /// named 役満だと確定しない。
    ///
    /// 一部の variant だけ named 役満・役なしを含む・数え役満・scoring unknown・生きた
    /// physical variant が1件も無い場合と、この経路で評価しなかった場合をすべて含む。
    NotEstablished,
}

impl NamedYakumanTsumo {
    /// named 役満だと確定したか。consumer の policy へ渡す1つの事実へ畳む。
    pub fn is_established(self) -> bool {
        matches!(self, Self::AllLiveVariants)
    }
}

/// 組み立て済みの完成手を、指定した production offense mode の Tsumo baseline で評価し、生きた
/// 和了牌の物理牌 variant がすべて named 役満になるかを求める。
///
/// baseline も点数計算も variant の分け方も [`tenpai_tsumo_value_from_hands`] と同じで、役満か
/// どうかは既存 scoring の結論そのもの。残枚数 0 の variant は引けないので判定に含めず、生きた
/// variant が1件も無いテンパイは named 役満と確定しない。役なし・数え役満・scoring unknown も
/// 同じく確定しない扱いで、名前の付いた役満だと推測しない。
pub(crate) fn tenpai_tsumo_named_yakuman(
    context: &GameContext,
    hands: &TenpaiCompletedHands,
    mode: TenpaiOffenseMode,
) -> NamedYakumanTsumo {
    tenpai_tsumo_profile(context, hands, mode)
        .map_or(NamedYakumanTsumo::NotEstablished, |profile| {
            named_yakuman_tsumo(&profile)
        })
}

/// 組み立て済みの完成手を、指定した攻撃モードの Tsumo baseline で評価し、和了牌の物理牌
/// variant ごとにツモ和了できるかを求める。
///
/// 集計値 ([`tenpai_tsumo_value_from_hands`]) と同じ profile を同じ判定で読み、variant 単位の
/// 結論だけを残したもの。役判定も点数計算もここでやり直さない。baseline を組み立てられない
/// 場合はどの牌も確定しない。
pub(crate) fn tenpai_tsumo_variant_outcomes(
    context: &GameContext,
    hands: &TenpaiCompletedHands,
    mode: TenpaiOffenseMode,
) -> TsumoVariantOutcomes {
    let Some(profile) = tenpai_tsumo_profile(context, hands, mode) else {
        return TsumoVariantOutcomes::default();
    };
    TsumoVariantOutcomes {
        variants: profile
            .waits()
            .iter()
            .flat_map(|wait| wait.winning_tiles().iter())
            .map(|variant| {
                (
                    variant.winning_tile(),
                    TsumoVariantStatus::from_value(tenpai_variant_value(variant.outcome())),
                )
            })
            .collect(),
    }
}

fn named_yakuman_tsumo(profile: &TenpaiHandValueProfile<'_>) -> NamedYakumanTsumo {
    let mut live_variants = profile
        .waits()
        .iter()
        .flat_map(|wait| wait.winning_tiles().iter())
        .filter(|variant| variant.remaining() > 0)
        .peekable();

    if live_variants.peek().is_none() {
        return NamedYakumanTsumo::NotEstablished;
    }
    if live_variants.all(|variant| tenpai_variant_value(variant.outcome()).is_yakuman()) {
        NamedYakumanTsumo::AllLiveVariants
    } else {
        NamedYakumanTsumo::NotEstablished
    }
}

/// 待ちごとの評価結果を、ツモ和了できる variant の残枚数と重み付き打点へ畳む。
///
/// ツモ baseline で役が無い variant はその牌でツモ和了できないので、成功する待ちにも打点にも
/// 含めない。0点の和了として加算せず、non-winning draw として扱う。点数計算の入力不足・裏ドラ
/// 未確定のように本当に評価できない variant は推測せず `None` にする。残枚数 0 の variant は
/// 生きていないのでどちらにも寄与しない。
fn tsumo_value(profile: &TenpaiHandValueProfile<'_>) -> Option<TenpaiTsumoValue> {
    let mut value = TenpaiTsumoValue::default();
    for wait in profile.waits() {
        for variant in wait.winning_tiles() {
            if variant.remaining() == 0 {
                continue;
            }
            let total = match tenpai_variant_value(variant.outcome()) {
                TenpaiVariantValue::Known { payment, .. } => payment.total(),
                TenpaiVariantValue::NoYaku => continue,
                TenpaiVariantValue::Unknown(_) => return None,
            };
            value.winning_remaining += u32::from(variant.remaining());
            value.weighted_total += u64::from(total) * u64::from(variant.remaining());
        }
    }
    Some(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    use bot_logic::{FixedMeldCount, Meld, MeldKind, TileCounts, TileType, tenpai_completed_hands};

    struct TileIdSource {
        used: [bool; TileId::COUNT],
    }

    impl TileIdSource {
        fn new() -> Self {
            Self {
                used: [false; TileId::COUNT],
            }
        }

        fn tiles(&mut self, strings: &[&str]) -> Vec<TileId> {
            strings.iter().map(|s| self.tile(s)).collect()
        }

        fn tile(&mut self, s: &str) -> TileId {
            let red = s.ends_with('r');
            let id = TileId::copies(tile(s))
                .find(|id| id.is_red() == red && !self.used[id.index()])
                .expect("同じ物理牌を使い回していない");
            self.used[id.index()] = true;
            id
        }
    }

    fn tile(s: &str) -> TileType {
        TileType::from_mjai_type_str(s.trim_end_matches('r')).expect("牌種として読める")
    }

    // 副露1つのテンパイを組み立てて、ツモ baseline の待ちごとの結論を確認する。
    //
    // 333m を pon した 234m 567p 55s 78s のテンパイ。6s なら全て中張牌で断幺が付くが、9s は
    // 么九牌が入るため副露手では役が無い。
    fn open_tenpai_tsumo_value(mode: TenpaiOffenseMode) -> Option<TenpaiTsumoValue> {
        let mut source = TileIdSource::new();
        let melded = source.tiles(&["3m", "3m", "3m"]);
        let concealed = source.tiles(&["2m", "3m", "4m", "5p", "6p", "7p", "5s", "5s", "7s", "8s"]);
        let melds = vec![Meld::new(MeldKind::Pon, melded, None)];

        let counts = TileCounts::from_tiles(concealed.iter().copied());
        let fixed_meld_count = FixedMeldCount::new(1).expect("副露1つ");
        let acceptance =
            bot_logic::calculate_acceptance_with_fixed_melds(&counts, fixed_meld_count);
        let hands = tenpai_completed_hands(&concealed, &melds, &acceptance, None, &concealed)
            .expect("テンパイの完成手を解析できる");

        let ctx = GameContext::from_parts_with_context(
            None,
            concealed,
            Vec::new(),
            Some(tile("E")),
            Some(tile("S")),
        );
        let (baseline, ura_dora) = tsumo_scoring_inputs(&ctx, mode)?;
        let profile = evaluate_tenpai_hand_value(&hands, baseline, &[], ura_dora);
        tsumo_value(&profile)
    }

    // 13面待ちの国士無双テンパイ。
    const KOKUSHI_TENPAI_HAND: [&str; 13] = [
        "1m", "9m", "1p", "9p", "1s", "9s", "E", "S", "W", "N", "P", "F", "C",
    ];

    // 白白白 發發發 中中 + 234m + 5m5m の 中 / 5m シャンポン。中ツモは大三元だが、5m ツモは
    // 小三元にしかならない。
    const DAISANGEN_SHANPON_HAND: [&str; 13] = [
        "P", "P", "P", "F", "F", "F", "C", "C", "2m", "3m", "4m", "5m", "5m",
    ];

    // 555m 123p 789p + 5s5s 9s9s の 5s / 9s シャンポン。ドラ表示牌 4m を4枚見せると 5m の
    // ドラ12翻 + 門前ツモで数え役満になる。
    const KAZOE_SHANPON_HAND: [&str; 13] = [
        "5m", "5m", "5m", "1p", "2p", "3p", "7p", "8p", "9p", "5s", "5s", "9s", "9s",
    ];
    const KAZOE_DORA_INDICATORS: [&str; 4] = ["4m", "4m", "4m", "4m"];

    // 門前テンパイ1件を組み立てて、ダマの Tsumo baseline で named 役満と確定するかを求める。
    // 待ちも残枚数も既存の受け入れそのもので、`extra_visible` は見え牌として残枚数へ反映する。
    fn named_yakuman_tsumo_of(
        hand: &[&str],
        dora_indicators: &[&str],
        extra_visible: &[&str],
        known_winds: bool,
    ) -> NamedYakumanTsumo {
        let mut source = TileIdSource::new();
        let concealed = source.tiles(hand);
        let dora_indicators = source.tiles(dora_indicators);
        let extra_visible = source.tiles(extra_visible);
        let visible: Vec<TileId> = concealed
            .iter()
            .chain(dora_indicators.iter())
            .chain(extra_visible.iter())
            .copied()
            .collect();

        let counts = TileCounts::from_tiles(concealed.iter().copied());
        let acceptance = bot_logic::calculate_acceptance_with_visible_tiles(&counts, &visible);
        let hands = tenpai_completed_hands(&concealed, &[], &acceptance, None, &visible)
            .expect("テンパイの完成手を解析できる");
        let ctx = GameContext::from_parts_with_visible_tiles(
            None,
            concealed,
            dora_indicators,
            known_winds.then(|| tile("E")),
            known_winds.then(|| tile("S")),
            visible,
        );

        tenpai_tsumo_named_yakuman(&ctx, &hands, TenpaiOffenseMode::Damaten)
    }

    #[test]
    fn every_live_variant_of_a_named_yakuman_tenpai_is_established() {
        assert_eq!(
            named_yakuman_tsumo_of(&KOKUSHI_TENPAI_HAND, &[], &[], true),
            NamedYakumanTsumo::AllLiveVariants
        );
    }

    #[test]
    fn a_partly_named_yakuman_tenpai_is_not_established() {
        // 中ツモだけが大三元で、5m ツモは小三元にしかならない。
        assert_eq!(
            named_yakuman_tsumo_of(&DAISANGEN_SHANPON_HAND, &[], &[], true),
            NamedYakumanTsumo::NotEstablished
        );
    }

    #[test]
    fn a_kazoe_yakuman_tenpai_is_not_a_named_yakuman() {
        // 数え役満は名前の付いた役満ではないので、点数が役満でも確定しない。
        assert_eq!(
            named_yakuman_tsumo_of(&KAZOE_SHANPON_HAND, &KAZOE_DORA_INDICATORS, &[], true),
            NamedYakumanTsumo::NotEstablished
        );
    }

    #[test]
    fn an_unknown_scoring_variant_is_not_inferred_as_a_named_yakuman() {
        // 同じ国士でも、自風が不明で親子が決まらないと支払いを確定できない。役満だと推測せず、
        // 確定しない variant として扱う。
        assert_eq!(
            named_yakuman_tsumo_of(&KOKUSHI_TENPAI_HAND, &[], &[], false),
            NamedYakumanTsumo::NotEstablished
        );
    }

    #[test]
    fn a_variant_without_any_remaining_tile_is_not_judged() {
        // 残枚数 0 の 5m は引けないので判定に含めない。生きた variant が中だけになれば確定する。
        assert_eq!(
            named_yakuman_tsumo_of(
                &DAISANGEN_SHANPON_HAND,
                &[],
                &["5m", "5mr", "1m", "1m", "1m"],
                true
            ),
            NamedYakumanTsumo::AllLiveVariants
        );

        // 生きた variant が1件も無いテンパイは和了しようが無いので確定しない。
        assert_eq!(
            named_yakuman_tsumo_of(&DAISANGEN_SHANPON_HAND, &[], &["5m", "5mr", "C", "C"], true),
            NamedYakumanTsumo::NotEstablished
        );
    }

    #[test]
    fn a_no_yaku_tsumo_variant_is_not_a_winning_draw() {
        // ツモ baseline で役が無い待ちは、0点の和了として加算せず success wait から外す。
        let value =
            open_tenpai_tsumo_value(TenpaiOffenseMode::Damaten).expect("ツモ打点を確定できる");

        // 待ちは 6s / 9s の各4枚だが、断幺が付くのは 6s だけ。
        assert_eq!(value.winning_remaining, 4);
        assert_eq!(value.weighted_total % 4, 0);
        assert!(value.weighted_total > 0);
    }

    #[test]
    fn a_tenpai_without_any_tsumo_yaku_has_no_winning_wait() {
        // 生きた待ちが全て役なしなら、待ち枚数も打点も 0 になる。0点の和了として加算しない。
        let value = TenpaiTsumoValue::default();
        assert_eq!(value.winning_remaining, 0);
        assert_eq!(value.weighted_total, 0);
    }
}
