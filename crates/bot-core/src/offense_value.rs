//! threat ありのテンパイで攻撃を継続した場合の確定打点を求める policy 層。
//!
//! 押し引きが「押せる打点か」を判断するための値だけを持つ。点数計算そのものは bot-logic の
//! 既存 scoring rule に任せ、ここでは「どの和了状況で評価するか」と「待ちごとの結果をどう
//! 1つの値へ畳むか」だけを持つ。
//!
//! # 攻撃モード
//!
//! 同じテンパイでも、リーチした手かダマの手かで打点が変わる。自分が既にリーチしていれば、その
//! テンパイはリーチ手として確定している。合法 action に Reach が出ないのはリーチ済みだからで、
//! ダマ手ではない。まだリーチしていない場合だけ、これからリーチするかを production のリーチ判断と
//! 同じ [`decide_reach_reason`] で決める。押し引き側で条件を書き直さず、合法 Reach の有無も
//! `legal_actions` だけを見る。
//!
//! 自分がリーチ済みかは `player_id` と `reached` から求めた
//! [`GameContext::own_reached`] が source of truth で、席を推測しない。判断できない場合は
//! 未リーチだともリーチ済みだとも推測せず、攻撃モードを [`TenpaiOffenseMode::Unknown`] にして
//! 打点も確定しない。
//!
//! # hypothetical baseline
//!
//! 打点比較のための和了状況は、現在の局面から推測するのではなく明示的な baseline として
//! 組み立てる。ダマの baseline は既存の [`damaten_baseline_context`] をそのまま使う。
//!
//! ```text
//! WinMethod              = Ron
//! RiichiStatus           = Riichi (リーチ済み / リーチ予定の手)
//! ippatsu                = false
//! chankan                = false
//! remaining_live_tiles   = 河底にならない固定値
//! round_wind / seat_wind = GameContext の既知 fact
//! 表ドラ / Kanドラ       = 現在の既知 indicator
//! 裏ドラ表示牌           = 空 (裏0)
//! ```
//!
//! 裏ドラは未来情報なので期待値を推測しない。ただし裏ドラ表示牌を未観測 (`None`) にして
//! [`HandValueOutcome::IndeterminateBonusHan`](bot_logic::HandValueOutcome::IndeterminateBonusHan)
//! にするのではなく、空の裏ドラ表示牌を明示的に渡して「裏0の最低保証打点」として確定させる。
//! 一発・河底・槍槓のような未来の偶発要素も同じ理由で加算しない。
//!
//! # 集約
//!
//! 待ちは牌種ごと、さらに和了牌の物理牌 (赤5 / 黒5) ごとの variant に分かれ、variant 1つ1つが
//! 別の打点を持つ。押し引きはそれらを残枚数で加重平均した1つの値で判断する。待ち牌種の間も
//! 赤 / 黒 variant の間も同じ残枚数 weight で集約する。
//!
//! 本場・供託は加えない。判定は純粋な [`Payment::total`] の加重平均だけで行う。生きていない
//! (残枚数0の) variant は平均へ寄与させない。生きた variant のどれか1つでも支払いを確定できない
//! 場合は、推測で平均を作らず [`OffenseValue::Unknown`] にする。役なし
//! ([`HandValueOutcome::NoCandidate`](bot_logic::HandValueOutcome::NoCandidate)) も機械的に0点として
//! 平均へ入れない。
//!
//! ダマのまま進む手の打点はロン和了を前提にした baseline なので、ダマでロンできると確定した
//! 場合しか使えない。ロン可否は既存のフリテン診断
//! ([`TenpaiWaitAvailability::can_ron`](bot_logic::TenpaiWaitAvailability::can_ron)) が source of
//! truth で、恒常フリテンだけでなく同巡内フリテン・リーチ後見逃しも統合した結論になる。どれで
//! 落ちても同じく [`OffenseValue::Unknown`] にし、ロンできないことを0点として扱わない。

use bot_logic::{
    DiscardEvaluation, HandValue, Payment, RiichiStatus, TenpaiCompletedHands,
    TenpaiHandValueProfile, TenpaiWaitAvailability, TileId, WinMethod, WinningContext,
    evaluate_tenpai_hand_value,
};

use crate::action::LegalAction;
use crate::context::GameContext;
use crate::damaten_value::{
    BASELINE_REMAINING_LIVE_TILES, damaten_baseline_context, damaten_value_from_hands,
    tenpai_completed_hands_after_discard,
};
use crate::reach_policy::decide_reach_reason;

/// 押し引きが高打点とみなす残枚数加重平均打点の下限 [点]。inclusive。
///
/// 親子で別の threshold へ換算せず、実点数の加重平均をそのまま比較する。
pub const PUSH_HIGH_VALUE_MIN_TOTAL: u32 = 5200;

// リーチ baseline に含める偶発役の有無。未来の偶発要素は加算しない。
const BASELINE_IPPATSU: bool = false;
const BASELINE_CHANKAN: bool = false;

// リーチ baseline の裏ドラ表示牌。未観測ではなく「観測済みで0枚」として渡す。
pub(crate) const BASELINE_URA_DORA_INDICATORS: &[TileId] = &[];

/// 攻撃を継続した場合の攻撃モード。
///
/// まだリーチしていない手でどちらを選ぶかは production のリーチ判断と同じ結論で、押し引き側の
/// 別判断ではない。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TenpaiOffenseMode {
    /// 既にリーチしている、またはこのテンパイを攻撃継続するならリーチする。
    Reach,
    /// まだリーチしておらず、攻撃継続してもダマのままにする。合法 Reach が無い場合も含む。
    Damaten,
    /// 攻撃モードを確定できない。自分がリーチ済みかを判断できない場合。
    Unknown,
}

/// 生きた待ちの支払い合計を残枚数で加重平均した攻撃打点。
///
/// 平均そのものは整数除算で丸めず、`weighted_total` と `total_remaining` の組のまま保持する。
/// threshold 判定は割り算をせずに `weighted_total >= threshold * total_remaining` で行う。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OffenseValue {
    Known {
        /// Σ(variant の [`Payment::total`] × variant の残枚数)。
        weighted_total: u64,
        /// Σ(variant の残枚数)。0 にはならない。
        total_remaining: u32,
    },
    /// 攻撃打点を確定できない。点数計算の入力不足・裏ドラ未確定・役なし・生きた待ちが無い場合。
    Unknown,
}

impl OffenseValue {
    /// 加重平均が `threshold` 以上か。確定しない場合は `None`。inclusive。
    pub fn meets(self, threshold: u32) -> Option<bool> {
        match self {
            Self::Known {
                weighted_total,
                total_remaining,
            } => Some(weighted_total >= u64::from(threshold) * u64::from(total_remaining)),
            Self::Unknown => None,
        }
    }

    /// 診断表示用の残枚数加重平均打点 [点]。確定しない場合は `None`。
    ///
    /// 表示のためだけに切り捨てた値で、threshold 判定には使わない。
    pub fn average_total(self) -> Option<u32> {
        match self {
            Self::Known {
                weighted_total,
                total_remaining,
            } => u32::try_from(weighted_total / u64::from(total_remaining)).ok(),
            Self::Unknown => None,
        }
    }

    pub fn is_known(self) -> bool {
        matches!(self, Self::Known { .. })
    }
}

/// 攻撃を継続した場合の攻撃モードと確定打点。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TenpaiOffenseValue {
    pub mode: TenpaiOffenseMode,
    pub value: OffenseValue,
}

/// リーチ打点比較用の hypothetical baseline を組み立てる。
///
/// 既にリーチしている手とこれからリーチする手のどちらにも使う。どちらもリーチ1翻が付く点は
/// 同じで、一発・裏ドラのような上振れを含めない最低保証打点になる。
///
/// 未来の事実を実際の事実として推測しないため、和了方法・一発・槍槓・残り山は policy が決めた
/// baseline の値にする。場風・自風だけを `context` の既知 fact から取り、不明なら不明のまま渡す。
pub fn reach_baseline_context(context: &GameContext) -> WinningContext {
    WinningContext::new(WinMethod::Ron)
        .with_round_wind(context.round_wind())
        .with_seat_wind(context.seat_wind())
        .with_riichi(RiichiStatus::Riichi)
        .with_ippatsu(Some(BASELINE_IPPATSU))
        .with_chankan(Some(BASELINE_CHANKAN))
        .with_remaining_live_tiles(Some(BASELINE_REMAINING_LIVE_TILES))
}

/// 選択済みの打牌1件について、その打牌後のテンパイを攻撃継続した場合の打点を求める。
///
/// `evaluation` は通常打牌 selection が選んだ打牌の評価で、待ち牌種と残枚数はその受け入れを
/// そのまま使う。`wait_availability` は同じ打牌から求めた既存のフリテン診断。押し引きのために
/// 向聴・受け入れ・待ち・フリテンを計算し直さない。
///
/// 攻撃モードは自分がリーチ済みかで決まり、未リーチの場合だけ production のリーチ判断と同じ
/// policy でリーチするかを決める。ダマ打点を評価する入口条件 (ダマでロンできると確定している
/// こと) も同じで、そこが満たされない場合は非フリテンだともダマ打点が十分だとも推測せず、
/// 待ち枚数だけを見る既存判断へ落ちる。
///
/// 攻撃モードを確定できない場合、打牌後の手牌を組み立てられない場合、ダマのままではロンできない
/// 場合、生きた variant の一部でも支払いを確定できない場合は [`OffenseValue::Unknown`] になる。
/// 推測で平均を作らない。
pub(crate) fn evaluate_tenpai_offense_value(
    context: &GameContext,
    evaluation: &DiscardEvaluation,
    wait_availability: &TenpaiWaitAvailability,
    legal_actions: &[LegalAction],
) -> TenpaiOffenseValue {
    // ロン可否は既存のフリテン診断が source of truth。恒常フリテン・同巡内フリテン・リーチ後
    // 見逃しを統合した結論で、押し引き側でフリテンを判定し直さない。
    let can_ron = wait_availability.can_ron() == Some(true);
    let hands = tenpai_completed_hands_after_discard(context, evaluation, wait_availability);
    let mode = offense_mode(
        context,
        wait_availability,
        legal_actions,
        hands.as_ref(),
        can_ron,
    );

    let value = scoring_inputs(context, mode, can_ron)
        .zip(hands.as_ref())
        .map_or(OffenseValue::Unknown, |((baseline, ura_dora), hands)| {
            let profile =
                evaluate_tenpai_hand_value(hands, baseline, context.dora_indicators(), ura_dora);
            offense_value(&profile)
        });

    TenpaiOffenseValue { mode, value }
}

/// 攻撃を継続した場合の攻撃モード。
///
/// 自分が既にリーチしていれば、そのテンパイはリーチ手として確定している。合法 action に Reach が
/// 出ないのはリーチ済みだからであって、ダマ手だからではない。まだリーチしていない場合だけ、
/// これからリーチするかを production のリーチ判断と同じ [`decide_reach_reason`] で決める。
///
/// 自分がリーチ済みかは [`GameContext::own_reached`] だけを source of truth にし、`reached` の
/// index を推測しない。判断できない場合は未リーチだともリーチ済みだとも推測せず
/// [`TenpaiOffenseMode::Unknown`] にする。
fn offense_mode(
    context: &GameContext,
    wait_availability: &TenpaiWaitAvailability,
    legal_actions: &[LegalAction],
    hands: Option<&TenpaiCompletedHands>,
    can_ron: bool,
) -> TenpaiOffenseMode {
    match context.own_reached() {
        None => TenpaiOffenseMode::Unknown,
        Some(true) => TenpaiOffenseMode::Reach,
        Some(false) => {
            let reach_legal = legal_actions
                .iter()
                .any(|action| matches!(action, LegalAction::Reach));

            // ダマでロンできると確定した場合だけダマ打点を評価する。既存リーチ判断と同じ入口条件。
            let damaten_verdict = can_ron
                .then(|| hands.map(|hands| damaten_value_from_hands(context, hands).verdict))
                .flatten();

            let reason = decide_reach_reason(
                reach_legal,
                damaten_verdict,
                wait_availability.tsumo_remaining,
            );
            if reason.selects_reach() {
                TenpaiOffenseMode::Reach
            } else {
                TenpaiOffenseMode::Damaten
            }
        }
    }
}

/// 攻撃モードごとの hypothetical baseline と裏ドラ表示牌。打点を確定できない場合は `None`。
///
/// ダマのまま進む手の打点はロン和了を前提にした baseline なので、ダマでロンできると確定した
/// 場合しか使えない。恒常フリテン・同巡内フリテン・リーチ後見逃しのどれで落ちても同じで、
/// ロンできない打点を「ダマの確定打点」として押し引きへ渡さない。ロンできないことを0点とも
/// 扱わず、打点を使わない既存判断へ委ねる。
fn scoring_inputs(
    context: &GameContext,
    mode: TenpaiOffenseMode,
    can_ron: bool,
) -> Option<(WinningContext, Option<&'static [TileId]>)> {
    match mode {
        TenpaiOffenseMode::Reach => Some((
            reach_baseline_context(context),
            Some(BASELINE_URA_DORA_INDICATORS),
        )),
        TenpaiOffenseMode::Damaten => can_ron.then(|| (damaten_baseline_context(context), None)),
        TenpaiOffenseMode::Unknown => None,
    }
}

/// 待ちごとの評価結果を、生きた variant の残枚数で加重平均した1つの値へ畳む。
fn offense_value(profile: &TenpaiHandValueProfile<'_>) -> OffenseValue {
    weighted_average(profile.waits().iter().flat_map(|wait| {
        wait.winning_tiles()
            .iter()
            .map(|variant| (variant_total(variant), variant.remaining()))
    }))
}

/// variant 1つ分の確定した支払い合計 [点]。確定しない場合は `None`。
///
/// 点数計算の入力不足・裏ドラ未確定・役なしはどれも「確定しない」で、0点として扱わない。
pub(crate) fn variant_total(variant: &bot_logic::WinningTileHandValue<'_>) -> Option<u32> {
    variant
        .known()
        .and_then(HandValue::payment)
        .map(Payment::total)
}

/// (支払い合計, 残枚数) の列を残枚数で加重平均する pure helper。
///
/// 残枚数0の variant は生きていないので、支払いを確定できるかにかかわらず平均へ入れない。
/// 生きた variant が1つも無い場合と、生きた variant のどれかが確定しない場合は
/// [`OffenseValue::Unknown`]。
///
/// 押し引きの攻撃打点と2手先診断の将来打点はこの1本を共有し、集約規則を複製しない。
pub(crate) fn weighted_average(variants: impl Iterator<Item = (Option<u32>, u8)>) -> OffenseValue {
    let mut weighted_total: u64 = 0;
    let mut total_remaining: u32 = 0;

    for (total, remaining) in variants {
        if remaining == 0 {
            continue;
        }
        let Some(total) = total else {
            return OffenseValue::Unknown;
        };
        weighted_total += u64::from(total) * u64::from(remaining);
        total_remaining += u32::from(remaining);
    }

    if total_remaining == 0 {
        return OffenseValue::Unknown;
    }

    OffenseValue::Known {
        weighted_total,
        total_remaining,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use bot_logic::{HistoryFuritenFacts, TileType};

    use crate::discard_selection::{
        select_discard_action_with_evaluation, selected_discard_tenpai_wait_availability,
    };

    fn average(variants: &[(Option<u32>, u8)]) -> OffenseValue {
        weighted_average(variants.iter().copied())
    }

    #[test]
    fn a_single_variant_average_is_its_own_total() {
        let value = average(&[(Some(5200), 4)]);
        assert_eq!(
            value,
            OffenseValue::Known {
                weighted_total: 20800,
                total_remaining: 4,
            }
        );
        assert_eq!(value.average_total(), Some(5200));
    }

    #[test]
    fn multiple_waits_are_weighted_by_remaining() {
        // 8000 が 1 枚、2000 が 3 枚 → (8000 + 6000) / 4 = 3500。
        let value = average(&[(Some(8000), 1), (Some(2000), 3)]);
        assert_eq!(value.average_total(), Some(3500));
        assert_eq!(value.meets(3500), Some(true));
        assert_eq!(value.meets(3501), Some(false));
    }

    #[test]
    fn red_and_black_variants_are_weighted_by_remaining() {
        // 同じ待ち牌種でも赤5 1枚は 7700、黒5 2枚は 5200。
        let value = average(&[(Some(7700), 1), (Some(5200), 2)]);
        assert_eq!(value.average_total(), Some(6033));
        assert_eq!(value.meets(PUSH_HIGH_VALUE_MIN_TOTAL), Some(true));
    }

    #[test]
    fn the_threshold_is_inclusive() {
        assert_eq!(
            average(&[(Some(5200), 3)]).meets(PUSH_HIGH_VALUE_MIN_TOTAL),
            Some(true)
        );
        assert_eq!(
            average(&[(Some(5199), 3)]).meets(PUSH_HIGH_VALUE_MIN_TOTAL),
            Some(false)
        );
    }

    #[test]
    fn the_threshold_does_not_round_the_average() {
        // 平均は 5200 をわずかに下回るが、整数除算では 5200 になってしまう組。
        let value = average(&[(Some(5200), 2), (Some(5199), 1)]);
        assert_eq!(value.average_total(), Some(5199));
        assert_eq!(value.meets(PUSH_HIGH_VALUE_MIN_TOTAL), Some(false));

        // 平均は 5200 をわずかに上回る組。
        let value = average(&[(Some(5201), 2), (Some(5200), 1)]);
        assert_eq!(value.meets(PUSH_HIGH_VALUE_MIN_TOTAL), Some(true));
    }

    #[test]
    fn a_variant_without_remaining_is_excluded() {
        // 残枚数0の variant は生きていないので、値が確定していても平均へ入れない。
        let with_dead = average(&[(Some(8000), 0), (Some(2000), 4)]);
        assert_eq!(with_dead, average(&[(Some(2000), 4)]));
        assert_eq!(with_dead.average_total(), Some(2000));

        // 残枚数0の variant は確定しなくても Unknown にしない。
        let with_dead_unknown = average(&[(None, 0), (Some(2000), 4)]);
        assert_eq!(with_dead_unknown.average_total(), Some(2000));
    }

    #[test]
    fn an_unknown_live_variant_makes_the_whole_value_unknown() {
        let value = average(&[(Some(8000), 3), (None, 1)]);
        assert_eq!(value, OffenseValue::Unknown);
        assert_eq!(value.meets(PUSH_HIGH_VALUE_MIN_TOTAL), None);
        assert_eq!(value.average_total(), None);
        assert!(!value.is_known());
    }

    #[test]
    fn no_live_variant_is_unknown() {
        assert_eq!(average(&[]), OffenseValue::Unknown);
        assert_eq!(average(&[(Some(8000), 0)]), OffenseValue::Unknown);
    }

    #[test]
    fn the_reach_baseline_fixes_the_future_facts() {
        let baseline = reach_baseline_context(&GameContext::default());

        assert_eq!(baseline.win_method(), WinMethod::Ron);
        assert_eq!(baseline.riichi(), RiichiStatus::Riichi);
        assert_eq!(baseline.ippatsu(), Some(false));
        assert_eq!(baseline.chankan(), Some(false));
        assert_eq!(baseline.rinshan(), None);
        assert!(!baseline.is_last_live_tile());
        assert_eq!(
            baseline.remaining_live_tiles(),
            Some(BASELINE_REMAINING_LIVE_TILES)
        );
        // 場風・自風が不明な局面では推測せず不明のまま渡す。
        assert_eq!(baseline.round_wind(), None);
        assert_eq!(baseline.seat_wind(), None);
    }

    #[test]
    fn the_reach_baseline_declares_an_empty_ura_dora() {
        // 裏ドラ表示牌は未観測ではなく「観測済みで0枚」。裏ドラ未確定にはしない。
        assert!(BASELINE_URA_DORA_INDICATORS.is_empty());
    }

    // ---- 実局面での攻撃打点 ----

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
            let tile_type = TileType::from_mjai_type_str(s.trim_end_matches('r')).unwrap();
            let red = s.ends_with('r');
            let id = TileId::copies(tile_type)
                .find(|id| id.is_red() == red && !self.used[id.index()])
                .expect("同じ物理牌を使い回していない");
            self.used[id.index()] = true;
            id
        }
    }

    // 攻撃打点を確定できる局面の組み立て。場風・自風・自分の河・履歴依存フリテンをすべて既知に
    // して、ダマでロンできる (`can_ron() == Some(true)`) 通常ケースを作る。自分は子 (南家)。
    struct OffenseCase {
        ctx: GameContext,
        actions: Vec<LegalAction>,
    }

    impl OffenseCase {
        // 通常打牌 selection が選んだ打牌後テンパイの攻撃打点。production と同じ経路で求める。
        fn value(&self) -> TenpaiOffenseValue {
            assert_eq!(self.can_ron(), Some(true), "ダマでロンできる局面である");
            self.value_with_actions(&self.actions)
        }

        // 合法 action を差し替えて評価する。ロン可否は確認せず、そのまま production 経路へ渡す。
        fn value_with_actions(&self, actions: &[LegalAction]) -> TenpaiOffenseValue {
            let selection = select_discard_action_with_evaluation(&self.ctx, actions);
            let evaluation = selection.evaluation.expect("通常打牌を選べる");
            let wait = selected_discard_tenpai_wait_availability(&self.ctx, &evaluation)
                .expect("打牌後がテンパイである");
            evaluate_tenpai_offense_value(&self.ctx, &evaluation, &wait, actions)
        }

        fn can_ron(&self) -> Option<bool> {
            let selection = select_discard_action_with_evaluation(&self.ctx, &self.actions);
            let evaluation = selection.evaluation.expect("通常打牌を選べる");
            selected_discard_tenpai_wait_availability(&self.ctx, &evaluation)
                .expect("打牌後がテンパイである")
                .can_ron()
        }

        // Reach を取り除いた合法 action。
        fn actions_without_reach(&self) -> Vec<LegalAction> {
            self.actions
                .iter()
                .filter(|action| !matches!(action, LegalAction::Reach))
                .cloned()
                .collect()
        }
    }

    fn offense_case(hand: &[&str], drawn: &str, dora_indicators: &[&str]) -> OffenseCase {
        offense_case_with(hand, drawn, dora_indicators, &[])
    }

    fn offense_case_with(
        hand: &[&str],
        drawn: &str,
        dora_indicators: &[&str],
        extra_visible: &[&str],
    ) -> OffenseCase {
        offense_case_with_furiten(
            hand,
            drawn,
            dora_indicators,
            extra_visible,
            HistoryFuritenFacts {
                same_turn: Some(false),
                riichi_missed_win: Some(false),
            },
        )
    }

    fn offense_case_with_furiten(
        hand: &[&str],
        drawn: &str,
        dora_indicators: &[&str],
        extra_visible: &[&str],
        history_furiten: HistoryFuritenFacts,
    ) -> OffenseCase {
        offense_case_inner(
            hand,
            drawn,
            dora_indicators,
            extra_visible,
            history_furiten,
            false,
        )
    }

    fn offense_case_inner(
        hand: &[&str],
        drawn: &str,
        dora_indicators: &[&str],
        extra_visible: &[&str],
        history_furiten: HistoryFuritenFacts,
        self_reached: bool,
    ) -> OffenseCase {
        let mut source = TileIdSource::new();
        let hand_tiles = source.tiles(hand);
        let drawn_tile = source.tile(drawn);
        let dora_indicators = source.tiles(dora_indicators);
        let extra_visible = source.tiles(extra_visible);

        let visible: Vec<TileId> = hand_tiles
            .iter()
            .chain([&drawn_tile])
            .chain(dora_indicators.iter())
            .chain(extra_visible.iter())
            .copied()
            .collect();
        let actions: Vec<LegalAction> = hand_tiles
            .iter()
            .chain([&drawn_tile])
            .map(|&tile| LegalAction::Dahai { tile })
            .chain([LegalAction::Reach])
            .collect();

        let ctx = GameContext::from_parts_with_table_state(
            Some(drawn_tile),
            hand_tiles,
            dora_indicators,
            TileType::from_mjai_type_str("E").ok(),
            TileType::from_mjai_type_str("S").ok(),
            visible,
            Some(0),
            Some(3),
            Default::default(),
            [self_reached, false, false, false],
        )
        .with_history_furiten_facts(history_furiten);

        OffenseCase { ctx, actions }
    }

    // 3p 嵌張の役なしテンパイ。9s の対子で断幺が消え、嵌張なので平和も付かない。ダマでは役が
    // 無いので、既存 policy 上リーチする手になる。
    const NO_YAKU_HAND: [&str; 13] = [
        "2m", "3m", "4m", "4m", "5m", "6m", "6m", "7m", "8m", "2p", "4p", "9s", "9s",
    ];

    // 平和 + 断幺の 3s / 6s 両面テンパイ。3s であがると一盃口が付くため待ちごとに打点が違う。
    const PINFU_TANYAO_HAND: [&str; 13] = [
        "2m", "3m", "4m", "6m", "7m", "8m", "2p", "2p", "3s", "4s", "5s", "4s", "5s",
    ];

    // 3s を全て見せて 6s の1種待ちにするための見え牌。
    const PINFU_TANYAO_SINGLE_WAIT_VISIBLE: [&str; 3] = ["3s", "3s", "3s"];

    // 断幺の 5s 単騎テンパイ。赤5と黒5で打点が変わる待ちになる。
    const RED_FIVE_TANKI_HAND: [&str; 13] = [
        "2m", "3m", "4m", "6m", "7m", "8m", "2p", "3p", "4p", "6p", "7p", "8p", "5s",
    ];

    #[test]
    fn a_no_yaku_menzen_tenpai_is_valued_with_the_reach_han() {
        // ダマでは役が無い手はリーチする手なので、ダマの NoCandidate ではなくリーチ1翻込みで
        // 評価する。役なしを0点として平均へ入れない。
        let case = offense_case(&NO_YAKU_HAND, "N", &[]);
        let value = case.value();

        assert_eq!(value.mode, TenpaiOffenseMode::Reach);
        // 3p 嵌張の残り4枚が、リーチのみ 40符1翻 = 1300 点。
        assert_eq!(
            value.value,
            OffenseValue::Known {
                weighted_total: 1300 * 4,
                total_remaining: 4,
            }
        );
        assert_eq!(value.value.average_total(), Some(1300));
    }

    #[test]
    fn the_reach_baseline_does_not_leave_the_ura_dora_indeterminate() {
        // 裏ドラ表示牌を未観測のまま渡すと裏ドラ未確定になる手でも、空の表示牌を明示的に渡す
        // ことで「裏0の最低保証打点」として確定する。
        let case = offense_case(&NO_YAKU_HAND, "N", &[]);
        let selection = select_discard_action_with_evaluation(&case.ctx, &case.actions);
        let evaluation = selection.evaluation.expect("通常打牌を選べる");
        let wait = selected_discard_tenpai_wait_availability(&case.ctx, &evaluation)
            .expect("打牌後がテンパイである");
        let hands = tenpai_completed_hands_after_discard(&case.ctx, &evaluation, &wait)
            .expect("打牌後の完成手を組み立てられる");
        let baseline = reach_baseline_context(&case.ctx);

        let indeterminate =
            evaluate_tenpai_hand_value(&hands, baseline, case.ctx.dora_indicators(), None);
        assert!(
            indeterminate.waits()[0].winning_tiles()[0]
                .outcome()
                .expect("点数計算の入力は足りている")
                .is_indeterminate()
        );

        assert!(case.value().value.is_known());
    }

    #[test]
    fn a_high_value_damaten_tenpai_keeps_the_damaten_value() {
        // 全ての待ちがダマで 7700 以上なら既存 policy 上ダマにする手なので、リーチ1翻を足さない。
        let case = offense_case_with(
            &PINFU_TANYAO_HAND,
            "N",
            &["1p"],
            &PINFU_TANYAO_SINGLE_WAIT_VISIBLE,
        );
        let value = case.value();

        assert_eq!(value.mode, TenpaiOffenseMode::Damaten);
        // 平和 + 断幺 + ドラ2 の 30符4翻 = 7700 点。リーチを足した 5翻 8000 点にはならない。
        assert_eq!(value.value.average_total(), Some(7700));
        assert_eq!(
            value.value,
            OffenseValue::Known {
                weighted_total: 7700 * 4,
                total_remaining: 4,
            }
        );
    }

    #[test]
    fn red_and_black_winning_tiles_are_weighted_by_remaining() {
        // 同じ 5s 単騎でも赤5であがると赤ドラ1翻が付く。赤 / 黒を残枚数で加重平均する。
        let case = offense_case(&RED_FIVE_TANKI_HAND, "N", &[]);
        let value = case.value();

        assert_eq!(value.mode, TenpaiOffenseMode::Reach);
        // 手牌の 5s は黒なので、残り3枚は赤1枚 + 黒2枚。
        // 黒: リーチ + 断幺の 40符2翻 = 2600 点、赤: さらに赤ドラ1翻の 40符3翻 = 5200 点。
        assert_eq!(
            value.value,
            OffenseValue::Known {
                weighted_total: 5200 + 2600 * 2,
                total_remaining: 3,
            }
        );
        assert_eq!(value.value.average_total(), Some(3466));
        assert_eq!(value.value.meets(PUSH_HIGH_VALUE_MIN_TOTAL), Some(false));
    }

    #[test]
    fn multiple_waits_are_weighted_by_their_own_remaining() {
        // 3s であがると一盃口が付き、6s より高い。待ち牌種の間も残枚数で加重平均する。
        // ダマは 3s 7700 / 6s 3900 で threshold 未満の待ちがあるため、リーチする手になる。
        let case = offense_case(&PINFU_TANYAO_HAND, "N", &["1m"]);
        let value = case.value();

        assert_eq!(value.mode, TenpaiOffenseMode::Reach);
        // 3s は手牌に1枚あるので残り3枚で、リーチ + 平和 + 断幺 + 一盃口 + ドラ1 の5翻 = 8000 点。
        // 6s は残り4枚で、一盃口が付かない 30符4翻 = 7700 点。
        assert_eq!(
            value.value,
            OffenseValue::Known {
                weighted_total: 8000 * 3 + 7700 * 4,
                total_remaining: 7,
            }
        );
        assert_eq!(value.value.average_total(), Some(7828));
        assert_eq!(value.value.meets(PUSH_HIGH_VALUE_MIN_TOTAL), Some(true));
    }

    #[test]
    fn an_incomplete_scoring_context_keeps_the_value_unknown() {
        // 場風・自風が不明で exact scoring できない局面では、推測で平均を作らず確定しない。
        let mut source = TileIdSource::new();
        let hand_tiles = source.tiles(&NO_YAKU_HAND);
        let drawn_tile = source.tile("N");
        let actions: Vec<LegalAction> = hand_tiles
            .iter()
            .chain([&drawn_tile])
            .map(|&tile| LegalAction::Dahai { tile })
            .chain([LegalAction::Reach])
            .collect();
        let ctx = GameContext::from_parts_with_table_state(
            Some(drawn_tile),
            hand_tiles,
            vec![],
            None,
            None,
            Vec::new(),
            Some(0),
            Some(3),
            Default::default(),
            [false; 4],
        )
        .with_history_furiten_facts(HistoryFuritenFacts {
            same_turn: Some(false),
            riichi_missed_win: Some(false),
        });

        let selection = select_discard_action_with_evaluation(&ctx, &actions);
        let evaluation = selection.evaluation.expect("通常打牌を選べる");
        let wait = selected_discard_tenpai_wait_availability(&ctx, &evaluation)
            .expect("打牌後がテンパイである");
        let value = evaluate_tenpai_offense_value(&ctx, &evaluation, &wait, &actions);

        assert_eq!(value.value, OffenseValue::Unknown);
        assert_eq!(value.value.meets(PUSH_HIGH_VALUE_MIN_TOTAL), None);
    }

    #[test]
    fn an_illegal_reach_values_the_tenpai_as_damaten() {
        // 合法 Reach が無ければ攻撃継続してもダマのままなので、ダマ打点で評価する。
        let case = offense_case(&NO_YAKU_HAND, "N", &[]);
        let value = case.value_with_actions(&case.actions_without_reach());

        assert_eq!(value.mode, TenpaiOffenseMode::Damaten);
        // ダマでは役が無いので打点を確定できない。役なしを0点として扱わない。
        assert_eq!(value.value, OffenseValue::Unknown);
    }

    #[test]
    fn an_illegal_reach_keeps_the_damaten_value_when_ron_is_available() {
        // 合法 Reach が無くても、ダマでロンできると確定していればダマ打点をそのまま使える。
        let case = offense_case_with(
            &PINFU_TANYAO_HAND,
            "N",
            &["1p"],
            &PINFU_TANYAO_SINGLE_WAIT_VISIBLE,
        );
        assert_eq!(case.can_ron(), Some(true));

        let value = case.value_with_actions(&case.actions_without_reach());
        assert_eq!(value.mode, TenpaiOffenseMode::Damaten);
        assert_eq!(value.value.average_total(), Some(7700));
    }

    #[test]
    fn an_already_reached_tenpai_is_valued_with_the_reach_han() {
        // 既にリーチしている手は Reach action が出ないが、ダマ手ではない。リーチ1翻を落とした
        // ダマ換算 3900 ではなく、リーチ込みの 7700 で評価する。
        let case = offense_case_inner(
            &PINFU_TANYAO_HAND,
            "N",
            &["1m"],
            &PINFU_TANYAO_SINGLE_WAIT_VISIBLE,
            HistoryFuritenFacts {
                same_turn: Some(false),
                riichi_missed_win: Some(false),
            },
            true,
        );
        assert_eq!(case.ctx.own_reached(), Some(true));

        let value = case.value_with_actions(&case.actions_without_reach());
        assert_eq!(value.mode, TenpaiOffenseMode::Reach);
        assert_eq!(value.value.average_total(), Some(7700));
        assert_eq!(value.value.meets(PUSH_HIGH_VALUE_MIN_TOTAL), Some(true));
    }

    #[test]
    fn an_illegal_reach_before_declaring_stays_damaten() {
        // 同じ手でもまだリーチしていなければダマ手なので、ダマ 3900 のまま評価する。
        let case = offense_case_with(
            &PINFU_TANYAO_HAND,
            "N",
            &["1m"],
            &PINFU_TANYAO_SINGLE_WAIT_VISIBLE,
        );
        assert_eq!(case.ctx.own_reached(), Some(false));

        let value = case.value_with_actions(&case.actions_without_reach());
        assert_eq!(value.mode, TenpaiOffenseMode::Damaten);
        assert_eq!(value.value.average_total(), Some(3900));
        assert_eq!(value.value.meets(PUSH_HIGH_VALUE_MIN_TOTAL), Some(false));
    }

    #[test]
    fn an_already_reached_tenpai_keeps_the_reach_value_without_ron() {
        // ダマ打点のロン可否 gate はダマ手だけのもの。既リーチ手はリーチ込みの baseline を
        // 使うため、リーチ後の見逃しでロンできなくても打点は確定したままになる。
        let case = offense_case_inner(
            &PINFU_TANYAO_HAND,
            "N",
            &["1m"],
            &PINFU_TANYAO_SINGLE_WAIT_VISIBLE,
            HistoryFuritenFacts {
                same_turn: Some(false),
                riichi_missed_win: Some(true),
            },
            true,
        );
        assert_eq!(case.can_ron(), Some(false));

        let value = case.value_with_actions(&case.actions_without_reach());
        assert_eq!(value.mode, TenpaiOffenseMode::Reach);
        assert_eq!(value.value.average_total(), Some(7700));
    }

    #[test]
    fn an_unknown_self_seat_keeps_the_offense_mode_unknown() {
        // player_id が不明だと自分がリーチ済みかを判断できない。player 0 を自分と仮定せず、
        // 攻撃モードも打点も確定しない。
        let mut source = TileIdSource::new();
        let hand_tiles = source.tiles(&PINFU_TANYAO_HAND);
        let drawn_tile = source.tile("N");
        let dora_indicators = source.tiles(&["1m"]);
        let extra_visible = source.tiles(&PINFU_TANYAO_SINGLE_WAIT_VISIBLE);
        let visible: Vec<TileId> = hand_tiles
            .iter()
            .chain([&drawn_tile])
            .chain(dora_indicators.iter())
            .chain(extra_visible.iter())
            .copied()
            .collect();
        let actions: Vec<LegalAction> = hand_tiles
            .iter()
            .chain([&drawn_tile])
            .map(|&tile| LegalAction::Dahai { tile })
            .collect();
        let ctx = GameContext::from_parts_with_table_state(
            Some(drawn_tile),
            hand_tiles,
            dora_indicators,
            TileType::from_mjai_type_str("E").ok(),
            TileType::from_mjai_type_str("S").ok(),
            visible,
            None,
            Some(3),
            Default::default(),
            [false; 4],
        )
        .with_history_furiten_facts(HistoryFuritenFacts {
            same_turn: Some(false),
            riichi_missed_win: Some(false),
        });
        assert_eq!(ctx.own_reached(), None);

        let case = OffenseCase { ctx, actions };
        let value = case.value_with_actions(&case.actions);
        assert_eq!(value.mode, TenpaiOffenseMode::Unknown);
        assert_eq!(value.value, OffenseValue::Unknown);
    }

    #[test]
    fn a_damaten_tenpai_that_cannot_ron_keeps_the_value_unknown() {
        // 恒常フリテンではないが、リーチ後の見逃しでロンできないテンパイ。ダマ打点は
        // ロン和了を前提にした値なので、7700 と確定できても攻撃打点としては使わない。
        let case = offense_case_with_furiten(
            &PINFU_TANYAO_HAND,
            "N",
            &["1p"],
            &PINFU_TANYAO_SINGLE_WAIT_VISIBLE,
            HistoryFuritenFacts {
                same_turn: Some(false),
                riichi_missed_win: Some(true),
            },
        );
        assert_eq!(case.can_ron(), Some(false));

        let value = case.value_with_actions(&case.actions_without_reach());
        assert_eq!(value.mode, TenpaiOffenseMode::Damaten);
        assert_eq!(value.value, OffenseValue::Unknown);
        // ロンできないことを0点として扱わない。
        assert_eq!(value.value.meets(PUSH_HIGH_VALUE_MIN_TOTAL), None);
    }

    #[test]
    fn a_damaten_tenpai_with_unknown_ron_keeps_the_value_unknown() {
        // ロン可否が unknown な局面でも、ロンできると推測してダマ打点を使わない。
        let case = offense_case_with_furiten(
            &PINFU_TANYAO_HAND,
            "N",
            &["1p"],
            &PINFU_TANYAO_SINGLE_WAIT_VISIBLE,
            HistoryFuritenFacts {
                same_turn: None,
                riichi_missed_win: None,
            },
        );
        assert_eq!(case.can_ron(), None);

        let value = case.value_with_actions(&case.actions_without_reach());
        assert_eq!(value.mode, TenpaiOffenseMode::Damaten);
        assert_eq!(value.value, OffenseValue::Unknown);
    }
}
