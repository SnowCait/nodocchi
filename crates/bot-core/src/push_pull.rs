use crate::action::LegalAction;
use crate::context::GameContext;
use crate::discard_selection::select_best_normal_discard_evaluation;
use crate::open_hand_threat::{
    OpenHandThreatAssessment, classify_open_hand_threats, has_high_open_hand_threat,
};
use crate::threat::{
    PlayerThreatFacts, has_reached_dealer, player_threat_facts_from_context, reached_opponent_count,
};
use bot_logic::{DiscardEvaluation, IishantenShape, TileCounts, TileId, TileType, count_dora};

const LOG_TARGET: &str = "bot_core::push_pull";

/// 押し引きの判断結果を表すモード。
///
/// `ShantenAgent` は `Hora` / `Ryukyoku` を確認したあと、このモードに応じて
/// action の優先順位を切り替える。
///
/// - `Push`: Reach → 通常打牌 → 防御 fallback
/// - `Neutral`: 通常打牌 → 防御 fallback(Reach は抑制)
/// - `Fold`: 防御 fallback → 通常打牌(Reach は抑制)
///
/// これは暫定 heuristic であり、以下はまだ考慮していない。
///
/// - 正確な打点(翻数・符・点数計算)
/// - 点棒状況
/// - 局・順位条件
///
/// 待ち形については Complete 一向聴だけを限定的に考慮する。一般的な良形・愚形評価は未対応で、
/// `Headless` / `Kuttsuki` / `Weak` に固定順位や押し引き差は付けない。
/// また、自分が親の場合の一向聴を限定的に考慮する。正確な打点・点棒・順位条件は未対応。
///
/// 打牌後の concealed hand の簡易打点 proxy は `PushPullInputs` とログに保持し、一向聴だけを
/// 対象にした限定補正で使用する。それ以外の branch では判定に影響しない。
///
/// 相手の危険度は、リーチ者と `High` の副露相手が同時にいる複合 threat、他家リーチだけの局面、
/// 他家リーチが0人で `OpenHandThreat` だけの局面の3通りに分けて見る。複合 threat は単独リーチ
/// より強い pressure として扱い、それ以外は従来どおり片方の情報だけを使う。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PushPullMode {
    Push,
    Neutral,
    Fold,
}

/// 攻撃を継続した場合の最善候補の評価値。
///
/// 現在の手牌から既存の通常打牌選択を行った場合の、最善候補の評価値を保持する。
/// 新しい向聴数計算や受け入れ計算・一向聴形分類は行わず、既存の `DiscardEvaluation` から取得する。
///
/// 打点関連フィールドは、打牌後の concealed hand 内で確認できる牌だけから求める簡易 proxy であり、
/// 正確な翻数・打点ではない。現在の簡易 proxy は fixed meld をまだ含めないため、副露・暗槓の
/// ドラや役牌は数えない。向聴数・受け入れは fixed meld を考慮した `DiscardEvaluation` の値をそのまま
/// 受け取る。
/// `decide_push_pull()` はこの proxy の合計だけを、単独の子リーチに対する子の一向聴の
/// 限定補正で参照する。個別フィールドは診断・ログ用。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PushPullOffenseState {
    pub min_shanten_after_discard: i8,
    pub acceptance_total_remaining: u8,
    pub acceptance_type_count: usize,
    pub standard_iishanten_shape_after_discard: IishantenShape,

    /// 打牌後の concealed hand に残るドラの総数。表示牌ドラと赤ドラを含む。
    /// 同じ牌を示す表示牌が複数あれば重複分も数え、赤5が表示牌ドラでもあれば両方数える。
    pub dora_count_after_discard: u8,
    /// 打牌後の concealed hand に残る赤ドラ(赤5)の枚数。`dora_count_after_discard` の内数であり、
    /// 合計 proxy へ別途加算しない。
    pub red_dora_count_after_discard: u8,
    /// 打牌後の concealed hand 内で確認できる役牌刻子・槓子候補の翻 proxy。
    /// 三元牌刻子は1、場風刻子・自風刻子は各1、連風牌(場風かつ自風)は2。
    /// 同じ牌が4枚あっても刻子・槓子候補1組として一度だけ数える。現在の簡易 proxy は
    /// fixed meld をまだ含めないため、副露した役牌は数えない。
    /// 場風・自風が不明な風牌は数えない(三元牌は風情報が無くても数える)。
    pub value_honor_han_proxy_after_discard: u8,
}

impl PushPullOffenseState {
    /// 簡易打点 proxy。ドラ総数と役牌翻 proxy の合計で、正確な翻数・打点ではない。
    /// 赤ドラ数は `dora_count_after_discard` に既に含まれるため別途加算しない。
    pub fn simple_value_proxy_after_discard(&self) -> u8 {
        self.dora_count_after_discard
            .saturating_add(self.value_honor_han_proxy_after_discard)
    }
}

/// 打牌後の concealed hand の簡易打点 proxy の内訳。`PushPullOffenseState` の各フィールドへ転記する前段の計算値。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
struct OffenseValueProxyBreakdown {
    dora_count: u8,
    red_dora_count: u8,
    value_honor_han_proxy: u8,
}

/// 補正済み評価が指す物理牌を1枚だけ除いた、打牌後の concealed hand の物理牌一覧を返す。
///
/// 手牌とツモ牌を結合した物理牌一覧から、`evaluation.discard` の牌種かつ
/// `evaluation.discards_red_five` の赤フラグと一致する牌を1枚だけ除く。赤5と通常5の混同を避けるため、
/// 牌種だけでなく赤フラグも一致させる。一致する物理牌が無ければ `None`。
fn tiles_after_discard(
    context: &GameContext,
    evaluation: &DiscardEvaluation,
) -> Option<Vec<TileId>> {
    let mut tiles: Vec<TileId> = context
        .hand_tiles()
        .iter()
        .copied()
        .chain(context.drawn_tile())
        .collect();

    let position = tiles.iter().position(|&tile| {
        tile.tile_type() == evaluation.discard && tile.is_red() == evaluation.discards_red_five
    })?;
    tiles.remove(position);
    Some(tiles)
}

/// 打牌後の concealed hand 内で確認できる役牌刻子・槓子候補の翻 proxy。
///
/// 三元牌は常に1。風牌は `round_wind` / `seat_wind` と一致した分だけ数え、連風牌は2。
/// 風情報が不明な風牌は数えない。現在の簡易 proxy は fixed meld をまだ含めないため、
/// 副露・暗槓は数えない。
fn value_honor_triplet_han(
    tile: TileType,
    round_wind: Option<TileType>,
    seat_wind: Option<TileType>,
) -> u8 {
    let mut han = u8::from(tile.is_dragon());
    if tile.is_wind() {
        han += u8::from(round_wind == Some(tile));
        han += u8::from(seat_wind == Some(tile));
    }
    han
}

/// 補正済み評価と `GameContext` から、打牌後の concealed hand の簡易打点 proxy の内訳を一度だけ計算する。
///
/// 実際に切られる物理牌カテゴリ(赤5・通常5)と一致するよう、`tiles_after_discard` で
/// 物理牌を1枚除いた打牌後の concealed hand へ処理を一元化する。ドラ総数・赤ドラ数・役牌翻 proxy を同じ牌集合から求める。
///
/// 通常の `ShantenAgent` 経路では補正済み評価と合法 action の物理牌情報が一致する不変条件があるため、
/// 一致する物理牌は必ず見つかる。それでも見つからない場合は panic せず、契約違反を `debug_assert` で
/// 検出しつつ release ではデフォルト値(計算不能)を返す。
fn offense_value_proxy_after_discard(
    context: &GameContext,
    evaluation: &DiscardEvaluation,
) -> OffenseValueProxyBreakdown {
    let Some(tiles) = tiles_after_discard(context, evaluation) else {
        debug_assert!(
            false,
            "打牌後の concealed hand を構築できない: 補正済み評価と一致する物理牌が手牌・ツモ牌に存在しない"
        );
        return OffenseValueProxyBreakdown::default();
    };

    let dora_indicators = context.dora_indicators();
    let mut dora_count = 0u8;
    let mut red_dora_count = 0u8;
    for &tile in &tiles {
        dora_count = dora_count.saturating_add(count_dora(tile, dora_indicators));
        if tile.is_red() {
            red_dora_count = red_dora_count.saturating_add(1);
        }
    }

    let counts = TileCounts::from_tiles(tiles.iter().copied());
    let round_wind = context.round_wind();
    let seat_wind = context.seat_wind();
    let mut value_honor_han_proxy = 0u8;
    for tile in TileType::all() {
        if counts.count(tile) >= 3 {
            value_honor_han_proxy = value_honor_han_proxy
                .saturating_add(value_honor_triplet_han(tile, round_wind, seat_wind));
        }
    }

    OffenseValueProxyBreakdown {
        dora_count,
        red_dora_count,
        value_honor_han_proxy,
    }
}

/// 押し引き判定に使用する入力データ。
///
/// - `opponent_reach_count`: 自分を除くリーチ者数。
/// - `dealer_reacher`: 他家リーチ者に親が含まれるか。親情報がない場合は false。
/// - `self_dealer`: 自分が親か。`player_id` または `oya` が不明なら false。
/// - `offense`: 攻撃評価を構築できない場合は `None`。
/// - `player_threats`: 全4席分の軽量な脅威 facts。
/// - `open_hand_threats`: `player_threats` から導出した全4席分の OpenHandThreat classification。
///
/// `opponent_reach_count` / `dealer_reacher` は `player_threats` から導出する。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PushPullInputs {
    pub opponent_reach_count: u8,
    pub dealer_reacher: bool,
    pub self_dealer: bool,
    pub offense: Option<PushPullOffenseState>,
    /// 全4席分の軽量な脅威 facts。副露・リーチ・親・自風・ドラ・役牌副露の観測事実を持つ。
    ///
    /// リーチ情報の source of truth であり、副露由来の危険度は `open_hand_threats` が持つ
    /// classification が source of truth。
    pub player_threats: [PlayerThreatFacts; 4],
    /// 全4席分の非リーチ副露相手の classification。`player_threats` から
    /// [`classify_open_hand_threats`] で一度だけ導出する。
    ///
    /// 押し引きと OpenHand 防御はこの同じ classification を参照し、High 条件をそれぞれで
    /// 書き直さない。自分の席・リーチ済みの席・`player_id` 不明の席は level を持たない
    /// [`OpenHandThreatAssessment::NotApplicable`] になる。
    pub open_hand_threats: [OpenHandThreatAssessment; 4],
}

impl PushPullInputs {
    /// High OpenHandThreat の相手が1人以上いるか。
    ///
    /// 判定は [`has_high_open_hand_threat`] と共有し、押し引き側で High 条件を書き直さない。
    pub fn has_high_open_hand_threat(&self) -> bool {
        has_high_open_hand_threat(&self.open_hand_threats)
    }

    /// リーチ者と High OpenHandThreat の相手が同時にいる複合 threat の局面か。
    ///
    /// 条件は `opponent_reach_count >= 1` かつ [`Self::has_high_open_hand_threat`]。
    /// [`OpenHandThreatLevel::Present`](crate::open_hand_threat::OpenHandThreatLevel::Present) の
    /// 相手は複合 threat に含めない。High 条件は PR の classification が source of truth で、
    /// ここで書き直さない。
    pub fn has_combined_threat(&self) -> bool {
        self.opponent_reach_count >= 1 && self.has_high_open_hand_threat()
    }
}

/// 押し引き判定がどの条件で下されたかを表す理由。
///
/// `*AgainstSingleNonDealer` / `*UnderHighPressure` / `TwoOrMoreShanten` は他家リーチだけがある
/// 局面の既存 reason。`*AgainstHighOpenHand` は他家リーチが0人で High OpenHandThreat の相手が
/// いる局面の reason。`*AgainstCombinedThreat` は両者が同時にいる複合 threat 局面の reason で、
/// 3種類が混ざることはない。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PushPullReason {
    NoOpponentReach,
    MissingOffenseEvaluation,
    TenpaiAgainstSingleNonDealer,
    TenpaiUnderHighPressure,
    StrongIishantenAgainstSingleNonDealer,
    CompleteIishantenAgainstSingleNonDealer,
    DealerIishantenAgainstSingleNonDealer,
    HighValueIishantenAgainstSingleNonDealer,
    IishantenUnderHighPressure,
    TwoOrMoreShanten,
    MissingOffenseAgainstHighOpenHand,
    TenpaiAgainstHighOpenHand,
    IishantenAgainstHighOpenHand,
    TwoOrMoreShantenAgainstHighOpenHand,
    MissingOffenseAgainstCombinedThreat,
    TenpaiAgainstCombinedThreat,
    IishantenAgainstCombinedThreat,
    TwoOrMoreShantenAgainstCombinedThreat,
}

/// 押し引き判定の結果。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PushPullDecision {
    pub mode: PushPullMode,
    pub reason: PushPullReason,
}

// 強い一向聴とみなすための暫定 heuristic。実戦の regression test に基づき将来調整する。
const STRONG_IISHANTEN_MIN_REMAINING: u8 = 8;
const STRONG_IISHANTEN_MIN_TYPES: usize = 2;

// 完全一向聴だけを対象にした限定補正の暫定 threshold。強い一向聴 threshold に届かなくても、
// 形が Complete でこの受け入れを満たす場合だけ Neutral にする。
const COMPLETE_IISHANTEN_MIN_REMAINING: u8 = 6;
const COMPLETE_IISHANTEN_MIN_TYPES: usize = 2;

// 自分が親のときだけ、強い一向聴 threshold より少し押し寄りにする限定補正の暫定 threshold。
// 形は限定せず、単独の子リーチに対してこの受け入れを満たす場合だけ Neutral にする。
const DEALER_IISHANTEN_MIN_REMAINING: u8 = 7;
const DEALER_IISHANTEN_MIN_TYPES: usize = 2;

// 明確な高打点だけを対象にした限定補正の暫定 threshold。受け入れや形は限定せず、
// 単独の子リーチに対する子の一向聴で簡易打点 proxy がこの値以上の場合だけ Neutral にする。
const HIGH_VALUE_IISHANTEN_MIN_SIMPLE_VALUE_PROXY: u8 = 4;

/// `GameContext` から押し引き判定の入力を構築する。
///
/// リーチ情報は `GameContext` から構築した脅威 facts
/// ([`player_threat_facts_from_context`]) を source of truth にする。`player_id == None` の
/// 場合は `GameContext::reached_opponents()` と同じ仕様で、リーチフラグが立っている全席を
/// 対象にする。
///
/// 攻撃評価は既存の通常打牌 best 評価 ([`select_best_normal_discard_evaluation`]) を再利用する。
/// 比較 semantics は `ShantenAgent` の通常打牌選択と同じで、1向聴限定の weighted tenpai wait を
/// 含む。合法 Dahai を受け取らない入口なので、対象は手牌から切れる全打牌候補になる。
/// 手牌とツモ牌が空なら `offense == None`。
pub fn push_pull_inputs_from_context(context: &GameContext) -> PushPullInputs {
    let tiles: Vec<_> = context
        .hand_tiles()
        .iter()
        .copied()
        .chain(context.drawn_tile())
        .collect();

    let evaluation = if tiles.is_empty() {
        None
    } else {
        select_best_normal_discard_evaluation(context, &tiles)
    };

    push_pull_inputs_from_context_with_evaluation(context, evaluation.as_ref())
}

/// すでに計算済みの `DiscardEvaluation` を利用して押し引き入力を構築する crate-private helper。
///
/// 脅威 facts をまだ構築していない入口用。すでに構築済みなら
/// [`push_pull_inputs_from_threat_facts`] へ渡して二重構築を避ける。
pub(crate) fn push_pull_inputs_from_context_with_evaluation(
    context: &GameContext,
    evaluation: Option<&DiscardEvaluation>,
) -> PushPullInputs {
    push_pull_inputs_from_threat_facts(
        context,
        player_threat_facts_from_context(context),
        evaluation,
    )
}

/// 構築済みの脅威 facts と `DiscardEvaluation` から押し引き入力を構築する crate-private helper。
///
/// リーチ者数と親リーチ判定は `player_threats` だけを source of truth にする
/// ([`reached_opponent_count`] / [`has_reached_dealer`])。`GameContext` のリーチ情報を別経路で
/// 数え直さない。`player_id` が不明な場合の扱いも `GameContext::reached_opponents()` と同じで、
/// リーチフラグが立っている全席を他家リーチとして数える。
///
/// 非リーチ副露相手の classification は既存 [`classify_open_hand_threats`] を同じ facts から
/// 一度だけ導出する。押し引き側で High 条件を分類し直さない。
///
/// offense は渡された evaluation から構築し、新しい向聴数・受け入れ計算は行わない。
/// evaluation が `None` なら offense も `None`。
pub(crate) fn push_pull_inputs_from_threat_facts(
    context: &GameContext,
    player_threats: [PlayerThreatFacts; 4],
    evaluation: Option<&DiscardEvaluation>,
) -> PushPullInputs {
    let opponent_reach_count = reached_opponent_count(&player_threats);
    let dealer_reacher = has_reached_dealer(&player_threats);
    let self_dealer = match (context.player_id(), context.oya()) {
        (Some(player_id), Some(oya)) => player_id == oya,
        _ => false,
    };

    let offense = evaluation.map(|evaluation| {
        let value_proxy = offense_value_proxy_after_discard(context, evaluation);
        PushPullOffenseState {
            min_shanten_after_discard: evaluation.min_shanten_after_discard(),
            acceptance_total_remaining: evaluation.acceptance_total_remaining(),
            acceptance_type_count: evaluation.acceptance_type_count(),
            standard_iishanten_shape_after_discard: evaluation
                .standard_iishanten_shape_after_discard,
            dora_count_after_discard: value_proxy.dora_count,
            red_dora_count_after_discard: value_proxy.red_dora_count,
            value_honor_han_proxy_after_discard: value_proxy.value_honor_han_proxy,
        }
    });

    PushPullInputs {
        opponent_reach_count,
        dealer_reacher,
        self_dealer,
        offense,
        player_threats,
        open_hand_threats: classify_open_hand_threats(&player_threats),
    }
}

/// 強い一向聴の暫定条件。形は限定せず、受け入れの枚数と種類数だけを見る。
///
/// 相手の脅威条件は含まない。単独の子リーチに対する限定補正だけがこの pure helper を参照する。
fn is_strong_iishanten(offense: &PushPullOffenseState) -> bool {
    offense.acceptance_total_remaining >= STRONG_IISHANTEN_MIN_REMAINING
        && offense.acceptance_type_count >= STRONG_IISHANTEN_MIN_TYPES
}

/// 完全一向聴だけを対象にした限定条件。強い一向聴 threshold に届かない場合の補正。
fn is_complete_iishanten(offense: &PushPullOffenseState) -> bool {
    offense.standard_iishanten_shape_after_discard == IishantenShape::Complete
        && offense.acceptance_total_remaining >= COMPLETE_IISHANTEN_MIN_REMAINING
        && offense.acceptance_type_count >= COMPLETE_IISHANTEN_MIN_TYPES
}

/// 自分が親のときだけの限定条件。形は限定しない。
fn is_dealer_iishanten(inputs: &PushPullInputs, offense: &PushPullOffenseState) -> bool {
    inputs.self_dealer
        && offense.acceptance_total_remaining >= DEALER_IISHANTEN_MIN_REMAINING
        && offense.acceptance_type_count >= DEALER_IISHANTEN_MIN_TYPES
}

/// 自分が子で、簡易打点 proxy が明確に高い場合だけの限定条件。受け入れ・形は限定しない。
fn is_high_value_iishanten(inputs: &PushPullInputs, offense: &PushPullOffenseState) -> bool {
    !inputs.self_dealer
        && offense.simple_value_proxy_after_discard() >= HIGH_VALUE_IISHANTEN_MIN_SIMPLE_VALUE_PROXY
}

/// 他家リーチが0人の局面の押し引き。
///
/// High OpenHandThreat の相手がいなければ従来どおり無条件で押す。`Present` だけの相手は行動を
/// 変えない。High の相手が1人以上いる場合だけ、テンパイ / 一向聴 / 二向聴以上で押し引きを
/// 分ける。
///
/// 一向聴では、単独の子リーチに対する限定補正 (強い一向聴 / 完全一向聴 / 自分が親 /
/// 簡易高打点) を適用せず、条件によらず `Fold` にする。それらの補正はリーチ policy 側だけで
/// 維持する。
///
/// 相手のリーチは含まないので、`RiichiThreat` と `OpenHandThreat` を1つの危険度へ集約する
/// 判定はここには無い。
fn decide_against_open_hand_threats(inputs: &PushPullInputs) -> PushPullDecision {
    if !inputs.has_high_open_hand_threat() {
        return PushPullDecision {
            mode: PushPullMode::Push,
            reason: PushPullReason::NoOpponentReach,
        };
    }

    // 攻撃評価が無ければ、情報不足を理由に強制 Fold にはせず Neutral に留める。
    let Some(offense) = inputs.offense else {
        return PushPullDecision {
            mode: PushPullMode::Neutral,
            reason: PushPullReason::MissingOffenseAgainstHighOpenHand,
        };
    };

    // テンパイ相当(向聴 <= 0)。副露相手が High でも自動 Fold にはしない。
    if offense.min_shanten_after_discard <= 0 {
        return PushPullDecision {
            mode: PushPullMode::Push,
            reason: PushPullReason::TenpaiAgainstHighOpenHand,
        };
    }

    // 一向聴。受け入れ・形・親・打点にかかわらず、限定補正を適用せず降りる。
    if offense.min_shanten_after_discard == 1 {
        return PushPullDecision {
            mode: PushPullMode::Fold,
            reason: PushPullReason::IishantenAgainstHighOpenHand,
        };
    }

    PushPullDecision {
        mode: PushPullMode::Fold,
        reason: PushPullReason::TwoOrMoreShantenAgainstHighOpenHand,
    }
}

/// リーチ者と High OpenHandThreat の相手が同時にいる複合 threat 局面の押し引き。
///
/// 単独の子リーチより強い pressure として扱い、テンパイでも押さない。ただし情報不足やテンパイ
/// から即 Fold にもしない。
///
/// - 攻撃評価なし: `Neutral`
/// - テンパイ (向聴 <= 0): `Neutral`。Reach は抑制されるが通常打牌は維持する。
/// - 一向聴: `Fold`
/// - 二向聴以上: `Fold`
///
/// 一向聴では、単独の子リーチに対する限定補正 (強い一向聴 / 完全一向聴 / 自分が親 / 簡易
/// 高打点) を適用しない。それらは単独の子リーチに対する補正として維持し、複合 threat には
/// 持ち込まない。
fn decide_against_combined_threat(inputs: &PushPullInputs) -> PushPullDecision {
    // 攻撃評価が無ければ、情報不足を理由に強制 Fold にはせず Neutral に留める。
    let Some(offense) = inputs.offense else {
        return PushPullDecision {
            mode: PushPullMode::Neutral,
            reason: PushPullReason::MissingOffenseAgainstCombinedThreat,
        };
    };

    // テンパイ相当(向聴 <= 0)。単独の子リーチなら Push だが、複合 threat では押さない。
    if offense.min_shanten_after_discard <= 0 {
        return PushPullDecision {
            mode: PushPullMode::Neutral,
            reason: PushPullReason::TenpaiAgainstCombinedThreat,
        };
    }

    if offense.min_shanten_after_discard == 1 {
        return PushPullDecision {
            mode: PushPullMode::Fold,
            reason: PushPullReason::IishantenAgainstCombinedThreat,
        };
    }

    PushPullDecision {
        mode: PushPullMode::Fold,
        reason: PushPullReason::TwoOrMoreShantenAgainstCombinedThreat,
    }
}

/// 押し引きを判定する pure な暫定 helper。
///
/// これは最初の保守的な土台であり、以下を考慮していない。
///
/// - 正確な打点(翻数・符・点数計算)
/// - 点棒状況
/// - 局・順位条件
///
/// 待ち形については Complete 一向聴だけを限定的に考慮する。一般的な良形・愚形評価は未対応で、
/// `Headless` / `Kuttsuki` / `Weak` に固定順位や押し引き差は付けない。
/// また、自分が親の場合の一向聴を限定的に考慮する。正確な打点・点棒・順位条件は未対応。
/// また、暫定 threshold は実戦の regression test に基づいて将来調整する。
/// 打牌後の concealed hand の簡易打点 proxy(`PushPullOffenseState::simple_value_proxy_after_discard()`)は、
/// 単独の子リーチに対する子の一向聴だけを対象にした限定補正で参照する。テンパイ・親リーチ・
/// 複数リーチ・二向聴以上・自分が親の場合は従来どおり proxy を見ない。
/// この判定結果は `ShantenAgent` の action 選択に反映される。
///
/// 判定順は 複合 threat → リーチのみ → 非リーチ副露相手のみ。リーチ者と High の副露相手が
/// 同時にいる局面だけを先に複合 policy ([`decide_against_combined_threat`]) で判定し、リーチだけ
/// の局面と副露相手だけの局面は既存の分岐をそのまま通す。
///
/// 他家リーチがある局面では従来どおりリーチ由来の policy だけを使う。他家リーチが0人の局面
/// だけが `PushPullInputs::open_hand_threats` を見る policy の対象で、High の相手が1人以上
/// いる場合に押し引きを分ける ([`decide_against_open_hand_threats`])。`Present` の相手は行動を
/// 変えず、複合 threat にも含めない。
///
/// - `Push`: Reach → 通常打牌 → 防御 fallback
/// - `Neutral`: 通常打牌 → 防御 fallback(Reach は抑制)
/// - `Fold`: 防御 fallback → 通常打牌(Reach は抑制)
pub fn decide_push_pull(inputs: &PushPullInputs) -> PushPullDecision {
    // 1. リーチ者と High の副露相手が同時にいる複合 threat。単独リーチより強い pressure として
    //    扱うため、リーチ policy より先に判定する。
    if inputs.has_combined_threat() {
        return decide_against_combined_threat(inputs);
    }

    // 2. 他家リーチがなければ、非リーチ副露相手の classification だけで判定する。
    if inputs.opponent_reach_count == 0 {
        return decide_against_open_hand_threats(inputs);
    }

    // 3. 攻撃評価が無ければ、情報不足を理由に強制 Fold にはせず Neutral に留める。
    let Some(offense) = inputs.offense else {
        return PushPullDecision {
            mode: PushPullMode::Neutral,
            reason: PushPullReason::MissingOffenseEvaluation,
        };
    };

    let single_non_dealer = inputs.opponent_reach_count == 1 && !inputs.dealer_reacher;

    // 4. テンパイ相当(向聴 <= 0)。
    if offense.min_shanten_after_discard <= 0 {
        if single_non_dealer {
            return PushPullDecision {
                mode: PushPullMode::Push,
                reason: PushPullReason::TenpaiAgainstSingleNonDealer,
            };
        }
        return PushPullDecision {
            mode: PushPullMode::Neutral,
            reason: PushPullReason::TenpaiUnderHighPressure,
        };
    }

    // 5. 一向聴。単独の子リーチかつ受け入れが暫定 threshold 以上の場合だけ強い一向聴。
    if offense.min_shanten_after_discard == 1 {
        // 5-1. 既存の強い一向聴 threshold。形にかかわらず従来どおり。
        let strong = single_non_dealer && is_strong_iishanten(&offense);
        if strong {
            return PushPullDecision {
                mode: PushPullMode::Neutral,
                reason: PushPullReason::StrongIishantenAgainstSingleNonDealer,
            };
        }

        // 5-2. 強い一向聴 threshold には届かないが、形が Complete で限定 threshold を満たす場合だけ Neutral。
        let complete = single_non_dealer && is_complete_iishanten(&offense);
        if complete {
            return PushPullDecision {
                mode: PushPullMode::Neutral,
                reason: PushPullReason::CompleteIishantenAgainstSingleNonDealer,
            };
        }

        // 5-3. 自分が親のときだけ、形を限定せずに限定 threshold を満たす場合だけ Neutral。
        let dealer = single_non_dealer && is_dealer_iishanten(inputs, &offense);
        if dealer {
            return PushPullDecision {
                mode: PushPullMode::Neutral,
                reason: PushPullReason::DealerIishantenAgainstSingleNonDealer,
            };
        }

        // 5-4. 子の自分が単独の子リーチを受け、簡易打点 proxy が限定 threshold 以上の場合だけ Neutral。
        // 受け入れ・形は限定せず、明確な高打点だけを対象にする。Push にはしない。
        let high_value = single_non_dealer && is_high_value_iishanten(inputs, &offense);
        if high_value {
            return PushPullDecision {
                mode: PushPullMode::Neutral,
                reason: PushPullReason::HighValueIishantenAgainstSingleNonDealer,
            };
        }

        return PushPullDecision {
            mode: PushPullMode::Fold,
            reason: PushPullReason::IishantenUnderHighPressure,
        };
    }

    // 6. 二向聴以上。
    PushPullDecision {
        mode: PushPullMode::Fold,
        reason: PushPullReason::TwoOrMoreShanten,
    }
}

/// 押し引き判断1回につき DEBUG イベントを1件出す opt-in ログ。
///
/// `RUST_LOG=bot_core::push_pull=debug` で有効化する。debug が無効な通常時は
/// ログ用の文字列変換などを一切行わない。全打牌候補は
/// `bot_core::discard_selection=trace` に任せ、ここでは重複出力しない。
pub(crate) fn log_push_pull_decision(
    decision: &PushPullDecision,
    inputs: &PushPullInputs,
    normal_discard: Option<&LegalAction>,
) {
    if !tracing::enabled!(target: LOG_TARGET, tracing::Level::DEBUG) {
        return;
    }

    let normal_discard = normal_discard.map(|action| match action {
        LegalAction::Dahai { tile } => tile.to_mjai_string(),
        other => format!("{other:?}"),
    });

    tracing::debug!(
        target: LOG_TARGET,
        mode = ?decision.mode,
        reason = ?decision.reason,
        opponent_reach_count = inputs.opponent_reach_count,
        dealer_reacher = inputs.dealer_reacher,
        self_dealer = inputs.self_dealer,
        high_open_hand_threat = inputs.has_high_open_hand_threat(),
        combined_threat = inputs.has_combined_threat(),
        offense_min_shanten_after_discard = ?inputs.offense.map(|offense| offense.min_shanten_after_discard),
        offense_acceptance_total_remaining = ?inputs.offense.map(|offense| offense.acceptance_total_remaining),
        offense_acceptance_type_count = ?inputs.offense.map(|offense| offense.acceptance_type_count),
        offense_iishanten_shape_after_discard = ?inputs.offense.map(|offense| offense.standard_iishanten_shape_after_discard),
        offense_dora_count_after_discard = ?inputs.offense.map(|offense| offense.dora_count_after_discard),
        offense_red_dora_count_after_discard = ?inputs.offense.map(|offense| offense.red_dora_count_after_discard),
        offense_value_honor_han_proxy_after_discard = ?inputs.offense.map(|offense| offense.value_honor_han_proxy_after_discard),
        offense_simple_value_proxy_after_discard = ?inputs.offense.map(|offense| offense.simple_value_proxy_after_discard()),
        normal_discard = ?normal_discard,
        "push-pull decision",
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use bot_logic::{TileId, TileType};

    fn tile(value: u8) -> TileId {
        TileId::new(value).unwrap()
    }

    fn offense(shanten: i8, remaining: u8, types: usize) -> PushPullOffenseState {
        offense_with_shape(shanten, remaining, types, IishantenShape::Unknown)
    }

    fn offense_with_shape(
        shanten: i8,
        remaining: u8,
        types: usize,
        shape: IishantenShape,
    ) -> PushPullOffenseState {
        offense_with_shape_and_proxy(shanten, remaining, types, shape, 0, 0, 0)
    }

    #[allow(clippy::too_many_arguments)]
    fn offense_with_shape_and_proxy(
        shanten: i8,
        remaining: u8,
        types: usize,
        shape: IishantenShape,
        dora: u8,
        red_dora: u8,
        value_honor_han: u8,
    ) -> PushPullOffenseState {
        PushPullOffenseState {
            min_shanten_after_discard: shanten,
            acceptance_total_remaining: remaining,
            acceptance_type_count: types,
            standard_iishanten_shape_after_discard: shape,
            dora_count_after_discard: dora,
            red_dora_count_after_discard: red_dora,
            value_honor_han_proxy_after_discard: value_honor_han,
        }
    }

    fn inputs(
        opponent_reach_count: u8,
        dealer_reacher: bool,
        offense: Option<PushPullOffenseState>,
    ) -> PushPullInputs {
        inputs_with_dealer(opponent_reach_count, dealer_reacher, false, offense)
    }

    fn inputs_with_dealer(
        opponent_reach_count: u8,
        dealer_reacher: bool,
        self_dealer: bool,
        offense: Option<PushPullOffenseState>,
    ) -> PushPullInputs {
        inputs_with_threats(
            opponent_reach_count,
            dealer_reacher,
            self_dealer,
            offense,
            no_threat_facts(),
        )
    }

    fn inputs_with_threats(
        opponent_reach_count: u8,
        dealer_reacher: bool,
        self_dealer: bool,
        offense: Option<PushPullOffenseState>,
        player_threats: [PlayerThreatFacts; 4],
    ) -> PushPullInputs {
        PushPullInputs {
            opponent_reach_count,
            dealer_reacher,
            self_dealer,
            offense,
            player_threats,
            open_hand_threats: classify_open_hand_threats(&player_threats),
        }
    }

    // 副露もリーチも無い4席分の facts。
    fn no_threat_facts() -> [PlayerThreatFacts; 4] {
        player_threat_facts_from_context(&GameContext::default())
    }

    #[test]
    fn no_opponent_reach_pushes_without_offense() {
        let decision = decide_push_pull(&inputs(0, false, None));
        assert_eq!(decision.mode, PushPullMode::Push);
        assert_eq!(decision.reason, PushPullReason::NoOpponentReach);
    }

    #[test]
    fn no_opponent_reach_pushes_with_offense() {
        let decision = decide_push_pull(&inputs(0, false, Some(offense(2, 4, 2))));
        assert_eq!(decision.mode, PushPullMode::Push);
        assert_eq!(decision.reason, PushPullReason::NoOpponentReach);
    }

    #[test]
    fn missing_offense_is_neutral() {
        let decision = decide_push_pull(&inputs(1, false, None));
        assert_eq!(decision.mode, PushPullMode::Neutral);
        assert_eq!(decision.reason, PushPullReason::MissingOffenseEvaluation);
    }

    #[test]
    fn tenpai_against_single_non_dealer_pushes() {
        let decision = decide_push_pull(&inputs(1, false, Some(offense(0, 4, 1))));
        assert_eq!(decision.mode, PushPullMode::Push);
        assert_eq!(
            decision.reason,
            PushPullReason::TenpaiAgainstSingleNonDealer
        );
    }

    #[test]
    fn tenpai_against_dealer_reach_is_neutral() {
        let decision = decide_push_pull(&inputs(1, true, Some(offense(0, 4, 1))));
        assert_eq!(decision.mode, PushPullMode::Neutral);
        assert_eq!(decision.reason, PushPullReason::TenpaiUnderHighPressure);
    }

    #[test]
    fn tenpai_against_multiple_reach_is_neutral() {
        let decision = decide_push_pull(&inputs(2, false, Some(offense(0, 4, 1))));
        assert_eq!(decision.mode, PushPullMode::Neutral);
        assert_eq!(decision.reason, PushPullReason::TenpaiUnderHighPressure);
    }

    #[test]
    fn strong_iishanten_boundary_is_neutral() {
        let decision = decide_push_pull(&inputs(1, false, Some(offense(1, 8, 2))));
        assert_eq!(decision.mode, PushPullMode::Neutral);
        assert_eq!(
            decision.reason,
            PushPullReason::StrongIishantenAgainstSingleNonDealer
        );
    }

    #[test]
    fn strong_iishanten_threshold_takes_priority_over_complete_reason() {
        // 強い一向聴 threshold を満たす場合は形が Complete でも既存 reason を維持する。
        let decision = decide_push_pull(&inputs(
            1,
            false,
            Some(offense_with_shape(1, 8, 2, IishantenShape::Complete)),
        ));
        assert_eq!(decision.mode, PushPullMode::Neutral);
        assert_eq!(
            decision.reason,
            PushPullReason::StrongIishantenAgainstSingleNonDealer
        );
    }

    #[test]
    fn complete_iishanten_boundary_is_neutral() {
        let decision = decide_push_pull(&inputs(
            1,
            false,
            Some(offense_with_shape(1, 6, 2, IishantenShape::Complete)),
        ));
        assert_eq!(decision.mode, PushPullMode::Neutral);
        assert_eq!(
            decision.reason,
            PushPullReason::CompleteIishantenAgainstSingleNonDealer
        );
    }

    #[test]
    fn weak_iishanten_with_same_acceptance_folds() {
        let decision = decide_push_pull(&inputs(
            1,
            false,
            Some(offense_with_shape(1, 6, 2, IishantenShape::Weak)),
        ));
        assert_eq!(decision.mode, PushPullMode::Fold);
        assert_eq!(decision.reason, PushPullReason::IishantenUnderHighPressure);
    }

    #[test]
    fn headless_and_kuttsuki_are_not_corrected() {
        for shape in [IishantenShape::Headless, IishantenShape::Kuttsuki] {
            let decision =
                decide_push_pull(&inputs(1, false, Some(offense_with_shape(1, 6, 2, shape))));
            assert_eq!(decision.mode, PushPullMode::Fold);
            assert_eq!(decision.reason, PushPullReason::IishantenUnderHighPressure);
        }
    }

    #[test]
    fn complete_iishanten_below_remaining_threshold_folds() {
        let decision = decide_push_pull(&inputs(
            1,
            false,
            Some(offense_with_shape(1, 5, 2, IishantenShape::Complete)),
        ));
        assert_eq!(decision.mode, PushPullMode::Fold);
        assert_eq!(decision.reason, PushPullReason::IishantenUnderHighPressure);
    }

    #[test]
    fn complete_iishanten_below_type_threshold_folds() {
        let decision = decide_push_pull(&inputs(
            1,
            false,
            Some(offense_with_shape(1, 6, 1, IishantenShape::Complete)),
        ));
        assert_eq!(decision.mode, PushPullMode::Fold);
        assert_eq!(decision.reason, PushPullReason::IishantenUnderHighPressure);
    }

    #[test]
    fn complete_iishanten_against_dealer_reach_folds() {
        let decision = decide_push_pull(&inputs(
            1,
            true,
            Some(offense_with_shape(1, 6, 2, IishantenShape::Complete)),
        ));
        assert_eq!(decision.mode, PushPullMode::Fold);
        assert_eq!(decision.reason, PushPullReason::IishantenUnderHighPressure);
    }

    #[test]
    fn complete_iishanten_against_multiple_reach_folds() {
        let decision = decide_push_pull(&inputs(
            2,
            false,
            Some(offense_with_shape(1, 6, 2, IishantenShape::Complete)),
        ));
        assert_eq!(decision.mode, PushPullMode::Fold);
        assert_eq!(decision.reason, PushPullReason::IishantenUnderHighPressure);
    }

    #[test]
    fn complete_shape_does_not_change_tenpai_branch() {
        // 向聴 <= 0 なら形が Complete でも既存のテンパイ分岐を使う。
        let single = decide_push_pull(&inputs(
            1,
            false,
            Some(offense_with_shape(0, 6, 2, IishantenShape::Complete)),
        ));
        assert_eq!(single.mode, PushPullMode::Push);
        assert_eq!(single.reason, PushPullReason::TenpaiAgainstSingleNonDealer);

        let dealer = decide_push_pull(&inputs(
            1,
            true,
            Some(offense_with_shape(0, 6, 2, IishantenShape::Complete)),
        ));
        assert_eq!(dealer.mode, PushPullMode::Neutral);
        assert_eq!(dealer.reason, PushPullReason::TenpaiUnderHighPressure);
    }

    #[test]
    fn complete_shape_does_not_change_two_shanten_branch() {
        // 向聴 >= 2 なら形が Complete でも既存の TwoOrMoreShanten を維持する。
        let decision = decide_push_pull(&inputs(
            1,
            false,
            Some(offense_with_shape(2, 6, 2, IishantenShape::Complete)),
        ));
        assert_eq!(decision.mode, PushPullMode::Fold);
        assert_eq!(decision.reason, PushPullReason::TwoOrMoreShanten);
    }

    #[test]
    fn complete_shape_does_not_change_no_opponent_reach() {
        let decision = decide_push_pull(&inputs(
            0,
            false,
            Some(offense_with_shape(1, 6, 2, IishantenShape::Complete)),
        ));
        assert_eq!(decision.mode, PushPullMode::Push);
        assert_eq!(decision.reason, PushPullReason::NoOpponentReach);
    }

    #[test]
    fn iishanten_below_remaining_threshold_folds() {
        let decision = decide_push_pull(&inputs(1, false, Some(offense(1, 7, 2))));
        assert_eq!(decision.mode, PushPullMode::Fold);
        assert_eq!(decision.reason, PushPullReason::IishantenUnderHighPressure);
    }

    #[test]
    fn iishanten_below_type_threshold_folds() {
        let decision = decide_push_pull(&inputs(1, false, Some(offense(1, 8, 1))));
        assert_eq!(decision.mode, PushPullMode::Fold);
        assert_eq!(decision.reason, PushPullReason::IishantenUnderHighPressure);
    }

    #[test]
    fn iishanten_against_dealer_reach_folds_even_with_wide_acceptance() {
        let decision = decide_push_pull(&inputs(1, true, Some(offense(1, 12, 4))));
        assert_eq!(decision.mode, PushPullMode::Fold);
        assert_eq!(decision.reason, PushPullReason::IishantenUnderHighPressure);
    }

    #[test]
    fn iishanten_against_multiple_reach_folds_even_with_wide_acceptance() {
        let decision = decide_push_pull(&inputs(2, false, Some(offense(1, 12, 4))));
        assert_eq!(decision.mode, PushPullMode::Fold);
        assert_eq!(decision.reason, PushPullReason::IishantenUnderHighPressure);
    }

    #[test]
    fn two_shanten_folds() {
        let decision = decide_push_pull(&inputs(1, false, Some(offense(2, 20, 4))));
        assert_eq!(decision.mode, PushPullMode::Fold);
        assert_eq!(decision.reason, PushPullReason::TwoOrMoreShanten);
    }

    #[test]
    fn three_shanten_folds() {
        let decision = decide_push_pull(&inputs(1, false, Some(offense(3, 30, 6))));
        assert_eq!(decision.mode, PushPullMode::Fold);
        assert_eq!(decision.reason, PushPullReason::TwoOrMoreShanten);
    }

    fn table_state_context(
        drawn_tile: Option<TileId>,
        hand_tiles: Vec<TileId>,
        player_id: Option<u8>,
        oya: Option<u8>,
        reached: [bool; 4],
    ) -> GameContext {
        GameContext::from_parts_with_table_state(
            drawn_tile,
            hand_tiles,
            vec![],
            None,
            None,
            Vec::new(),
            player_id,
            oya,
            Default::default(),
            reached,
        )
    }

    #[test]
    fn opponent_reach_count_excludes_self() {
        let context = table_state_context(None, vec![], Some(0), None, [true, true, false, false]);
        let inputs = push_pull_inputs_from_context(&context);
        assert_eq!(inputs.opponent_reach_count, 1);
    }

    #[test]
    fn opponent_reach_count_without_player_id_counts_all() {
        let context = table_state_context(None, vec![], None, None, [true, false, true, false]);
        let inputs = push_pull_inputs_from_context(&context);
        assert_eq!(inputs.opponent_reach_count, 2);
    }

    #[test]
    fn dealer_reacher_true_when_oya_is_opponent_reacher() {
        let context =
            table_state_context(None, vec![], Some(0), Some(1), [false, true, false, false]);
        let inputs = push_pull_inputs_from_context(&context);
        assert!(inputs.dealer_reacher);
    }

    #[test]
    fn dealer_reacher_false_when_self_is_oya() {
        let context =
            table_state_context(None, vec![], Some(0), Some(0), [true, true, false, false]);
        let inputs = push_pull_inputs_from_context(&context);
        assert!(!inputs.dealer_reacher);
    }

    #[test]
    fn dealer_reacher_false_without_oya() {
        let context = table_state_context(None, vec![], Some(0), None, [false, true, false, false]);
        let inputs = push_pull_inputs_from_context(&context);
        assert!(!inputs.dealer_reacher);
    }

    #[test]
    fn offense_is_none_without_tiles() {
        let context = table_state_context(None, vec![], Some(0), None, [false, true, false, false]);
        let inputs = push_pull_inputs_from_context(&context);
        assert_eq!(inputs.offense, None);
    }

    #[test]
    fn offense_matches_context_selector() {
        let hand: Vec<_> = [0u8, 4, 8, 12, 17, 20, 24, 28, 32, 36, 40, 44, 89]
            .iter()
            .map(|&value| tile(value))
            .collect();
        let context = GameContext::from_parts_with_context(
            Some(tile(116)),
            hand,
            vec![tile(12)],
            Some(TileType::new(27).unwrap()),
            Some(TileType::new(28).unwrap()),
        );

        let inputs = push_pull_inputs_from_context(&context);
        let offense = inputs.offense.expect("offense should be present");

        let tiles: Vec<_> = context
            .hand_tiles()
            .iter()
            .copied()
            .chain(context.drawn_tile())
            .collect();
        let expected = bot_logic::select_best_discard_from_tiles_with_context(
            &tiles,
            context.dora_indicators(),
            context.round_wind(),
            context.seat_wind(),
        )
        .unwrap();

        assert_eq!(
            offense.min_shanten_after_discard,
            expected.min_shanten_after_discard()
        );
        assert_eq!(
            offense.acceptance_total_remaining,
            expected.acceptance_total_remaining()
        );
        assert_eq!(
            offense.acceptance_type_count,
            expected.acceptance_type_count()
        );
    }

    #[test]
    fn offense_matches_visible_tiles_selector() {
        let hand: Vec<_> = [0u8, 4, 8, 12, 17, 20, 24, 28, 32, 48, 53, 56, 36]
            .iter()
            .map(|&value| tile(value))
            .collect();
        let mut visible = hand.clone();
        visible.extend([68u8, 69, 70, 71].iter().map(|&value| tile(value)));
        let context = GameContext::from_parts_with_visible_tiles(
            Some(tile(68)),
            hand,
            vec![],
            None,
            None,
            visible,
        );

        let inputs = push_pull_inputs_from_context(&context);
        let offense = inputs.offense.expect("offense should be present");

        let tiles: Vec<_> = context
            .hand_tiles()
            .iter()
            .copied()
            .chain(context.drawn_tile())
            .collect();
        let expected = bot_logic::select_best_discard_from_tiles_with_visible_tiles(
            &tiles,
            context.dora_indicators(),
            context.round_wind(),
            context.seat_wind(),
            context.visible_tiles(),
        )
        .unwrap();

        assert_eq!(
            offense.min_shanten_after_discard,
            expected.min_shanten_after_discard()
        );
        assert_eq!(
            offense.acceptance_total_remaining,
            expected.acceptance_total_remaining()
        );
        assert_eq!(
            offense.acceptance_type_count,
            expected.acceptance_type_count()
        );
    }

    #[test]
    fn with_evaluation_matches_public_inputs() {
        use crate::discard_selection::select_best_normal_discard_evaluation;

        let hand: Vec<_> = [0u8, 4, 8, 12, 17, 20, 24, 28, 32, 36, 40, 44, 89]
            .iter()
            .map(|&value| tile(value))
            .collect();
        let context = GameContext::from_parts_with_table_state(
            Some(tile(116)),
            hand,
            vec![tile(12)],
            Some(TileType::new(27).unwrap()),
            Some(TileType::new(28).unwrap()),
            Vec::new(),
            Some(0),
            Some(1),
            Default::default(),
            [false, true, false, false],
        );

        let tiles: Vec<_> = context
            .hand_tiles()
            .iter()
            .copied()
            .chain(context.drawn_tile())
            .collect();
        let evaluation = select_best_normal_discard_evaluation(&context, &tiles);

        let shared = push_pull_inputs_from_context_with_evaluation(&context, evaluation.as_ref());
        let public = push_pull_inputs_from_context(&context);
        assert_eq!(shared, public);
        assert!(shared.offense.is_some());
    }

    #[test]
    fn public_inputs_use_the_normal_discard_selection() {
        // 単独入口の offense も、1向聴限定の weighted tenpai wait を含む通常打牌 selection から
        // 構築する。1手比較だけの best 評価とは別の候補になる局面で固定する。
        use crate::discard_selection::select_best_normal_discard_evaluation;
        use crate::discard_selection::tests::{
            iishanten_wait_context, iishanten_wait_tiles, one_step_best_evaluation,
        };

        let context = iishanten_wait_context();
        let tiles = iishanten_wait_tiles();

        let normal = select_best_normal_discard_evaluation(&context, &tiles);
        let one_step = one_step_best_evaluation(&context, &tiles);
        assert!(normal.is_some());
        assert_ne!(normal, one_step, "両者が分かれる局面である必要がある");

        let public = push_pull_inputs_from_context(&context);
        assert_eq!(
            public,
            push_pull_inputs_from_context_with_evaluation(&context, normal.as_ref()),
        );
        assert_ne!(
            public,
            push_pull_inputs_from_context_with_evaluation(&context, one_step.as_ref()),
        );
        assert!(public.offense.is_some());
    }

    #[test]
    fn with_evaluation_transcribes_iishanten_shape() {
        use crate::discard_selection::select_best_normal_discard_evaluation;

        let hand: Vec<_> = [0u8, 4, 8, 12, 17, 20, 24, 28, 32, 36, 40, 44, 89]
            .iter()
            .map(|&value| tile(value))
            .collect();
        let context = GameContext::from_parts_with_table_state(
            Some(tile(116)),
            hand,
            vec![tile(12)],
            Some(TileType::new(27).unwrap()),
            Some(TileType::new(28).unwrap()),
            Vec::new(),
            Some(0),
            Some(1),
            Default::default(),
            [false, true, false, false],
        );

        let tiles: Vec<_> = context
            .hand_tiles()
            .iter()
            .copied()
            .chain(context.drawn_tile())
            .collect();
        let mut evaluation = select_best_normal_discard_evaluation(&context, &tiles)
            .expect("evaluation should exist");

        for shape in [IishantenShape::Complete, IishantenShape::Unknown] {
            evaluation.standard_iishanten_shape_after_discard = shape;
            let inputs = push_pull_inputs_from_context_with_evaluation(&context, Some(&evaluation));
            let offense = inputs.offense.expect("offense should be present");
            assert_eq!(offense.standard_iishanten_shape_after_discard, shape);
        }
    }

    #[test]
    fn with_evaluation_none_yields_no_offense() {
        let context = table_state_context(None, vec![], Some(0), None, [false, true, false, false]);
        let inputs = push_pull_inputs_from_context_with_evaluation(&context, None);
        assert_eq!(inputs.offense, None);
        assert_eq!(inputs.opponent_reach_count, 1);
    }

    #[test]
    fn with_evaluation_keeps_reach_count_and_dealer_judgment() {
        use crate::discard_selection::select_best_normal_discard_evaluation;

        let hand: Vec<_> = [0u8, 4, 8, 12, 17, 20, 24, 28, 32, 36, 40, 44, 89]
            .iter()
            .map(|&value| tile(value))
            .collect();
        let context = GameContext::from_parts_with_table_state(
            Some(tile(116)),
            hand,
            vec![],
            None,
            None,
            Vec::new(),
            Some(0),
            Some(1),
            Default::default(),
            [false, true, true, false],
        );
        let before = context.clone();

        let tiles: Vec<_> = context
            .hand_tiles()
            .iter()
            .copied()
            .chain(context.drawn_tile())
            .collect();
        let evaluation = select_best_normal_discard_evaluation(&context, &tiles);
        let evaluation_before = evaluation.clone();

        let inputs = push_pull_inputs_from_context_with_evaluation(&context, evaluation.as_ref());

        assert_eq!(inputs.opponent_reach_count, 2);
        assert!(inputs.dealer_reacher);
        // GameContext と evaluation を変更しない。
        assert_eq!(context, before);
        assert_eq!(evaluation, evaluation_before);
    }

    #[test]
    fn does_not_mutate_context() {
        let hand: Vec<_> = [0u8, 4, 8, 12, 17, 20, 24, 28, 32, 36, 40, 44, 89]
            .iter()
            .map(|&value| tile(value))
            .collect();
        let context = GameContext::from_parts_with_table_state(
            Some(tile(116)),
            hand,
            vec![tile(12)],
            Some(TileType::new(27).unwrap()),
            Some(TileType::new(28).unwrap()),
            Vec::new(),
            Some(0),
            Some(1),
            Default::default(),
            [false, true, false, false],
        );
        let before = context.clone();

        let _ = push_pull_inputs_from_context(&context);

        assert_eq!(context, before);
    }

    #[test]
    fn self_dealer_true_when_player_is_oya() {
        let context =
            table_state_context(None, vec![], Some(1), Some(1), [false, false, false, false]);
        let inputs = push_pull_inputs_from_context(&context);
        assert!(inputs.self_dealer);
    }

    #[test]
    fn self_dealer_false_when_player_is_not_oya() {
        let context =
            table_state_context(None, vec![], Some(1), Some(2), [false, false, false, false]);
        let inputs = push_pull_inputs_from_context(&context);
        assert!(!inputs.self_dealer);
    }

    #[test]
    fn self_dealer_false_without_player_id() {
        let context =
            table_state_context(None, vec![], None, Some(1), [false, false, false, false]);
        let inputs = push_pull_inputs_from_context(&context);
        assert!(!inputs.self_dealer);
    }

    #[test]
    fn self_dealer_false_without_oya() {
        let context =
            table_state_context(None, vec![], Some(1), None, [false, false, false, false]);
        let inputs = push_pull_inputs_from_context(&context);
        assert!(!inputs.self_dealer);
    }

    #[test]
    fn self_dealer_and_dealer_reacher_are_distinct() {
        // 自分が親で子1人がリーチ。
        let dealer_self =
            table_state_context(None, vec![], Some(0), Some(0), [false, true, false, false]);
        let inputs = push_pull_inputs_from_context(&dealer_self);
        assert!(inputs.self_dealer);
        assert!(!inputs.dealer_reacher);
        assert_eq!(inputs.opponent_reach_count, 1);

        // 自分が子で親がリーチ。
        let dealer_reach =
            table_state_context(None, vec![], Some(0), Some(1), [false, true, false, false]);
        let inputs = push_pull_inputs_from_context(&dealer_reach);
        assert!(!inputs.self_dealer);
        assert!(inputs.dealer_reacher);
        assert_eq!(inputs.opponent_reach_count, 1);
    }

    #[test]
    fn dealer_iishanten_boundary_is_neutral() {
        let decision = decide_push_pull(&inputs_with_dealer(
            1,
            false,
            true,
            Some(offense_with_shape(1, 7, 2, IishantenShape::Weak)),
        ));
        assert_eq!(decision.mode, PushPullMode::Neutral);
        assert_eq!(
            decision.reason,
            PushPullReason::DealerIishantenAgainstSingleNonDealer
        );
    }

    #[test]
    fn dealer_iishanten_folds_when_self_is_child() {
        let decision = decide_push_pull(&inputs_with_dealer(
            1,
            false,
            false,
            Some(offense_with_shape(1, 7, 2, IishantenShape::Weak)),
        ));
        assert_eq!(decision.mode, PushPullMode::Fold);
        assert_eq!(decision.reason, PushPullReason::IishantenUnderHighPressure);
    }

    #[test]
    fn dealer_iishanten_below_remaining_threshold_folds() {
        let decision = decide_push_pull(&inputs_with_dealer(
            1,
            false,
            true,
            Some(offense_with_shape(1, 6, 2, IishantenShape::Weak)),
        ));
        assert_eq!(decision.mode, PushPullMode::Fold);
        assert_eq!(decision.reason, PushPullReason::IishantenUnderHighPressure);
    }

    #[test]
    fn dealer_iishanten_below_type_threshold_folds() {
        let decision = decide_push_pull(&inputs_with_dealer(
            1,
            false,
            true,
            Some(offense_with_shape(1, 7, 1, IishantenShape::Weak)),
        ));
        assert_eq!(decision.mode, PushPullMode::Fold);
        assert_eq!(decision.reason, PushPullReason::IishantenUnderHighPressure);
    }

    #[test]
    fn dealer_iishanten_keeps_strong_reason() {
        let decision = decide_push_pull(&inputs_with_dealer(
            1,
            false,
            true,
            Some(offense_with_shape(1, 8, 2, IishantenShape::Weak)),
        ));
        assert_eq!(decision.mode, PushPullMode::Neutral);
        assert_eq!(
            decision.reason,
            PushPullReason::StrongIishantenAgainstSingleNonDealer
        );
    }

    #[test]
    fn dealer_iishanten_keeps_complete_reason() {
        let decision = decide_push_pull(&inputs_with_dealer(
            1,
            false,
            true,
            Some(offense_with_shape(1, 6, 2, IishantenShape::Complete)),
        ));
        assert_eq!(decision.mode, PushPullMode::Neutral);
        assert_eq!(
            decision.reason,
            PushPullReason::CompleteIishantenAgainstSingleNonDealer
        );
    }

    #[test]
    fn dealer_iishanten_complete_at_eight_keeps_strong_reason() {
        let decision = decide_push_pull(&inputs_with_dealer(
            1,
            false,
            true,
            Some(offense_with_shape(1, 8, 2, IishantenShape::Complete)),
        ));
        assert_eq!(decision.mode, PushPullMode::Neutral);
        assert_eq!(
            decision.reason,
            PushPullReason::StrongIishantenAgainstSingleNonDealer
        );
    }

    #[test]
    fn dealer_iishanten_not_applied_to_multiple_reach() {
        let decision = decide_push_pull(&inputs_with_dealer(
            2,
            false,
            true,
            Some(offense_with_shape(1, 7, 2, IishantenShape::Weak)),
        ));
        assert_eq!(decision.mode, PushPullMode::Fold);
        assert_eq!(decision.reason, PushPullReason::IishantenUnderHighPressure);
    }

    #[test]
    fn dealer_iishanten_not_applied_to_dealer_reach() {
        // 不整合な入力(self_dealer と dealer_reacher が同時に true)でも親補正は適用しない。
        let decision = decide_push_pull(&inputs_with_dealer(
            1,
            true,
            false,
            Some(offense_with_shape(1, 7, 2, IishantenShape::Weak)),
        ));
        assert_eq!(decision.mode, PushPullMode::Fold);
        assert_eq!(decision.reason, PushPullReason::IishantenUnderHighPressure);
    }

    #[test]
    fn dealer_does_not_change_tenpai_branch() {
        let single = decide_push_pull(&inputs_with_dealer(1, false, true, Some(offense(0, 4, 1))));
        assert_eq!(single.mode, PushPullMode::Push);
        assert_eq!(single.reason, PushPullReason::TenpaiAgainstSingleNonDealer);

        let dealer = decide_push_pull(&inputs_with_dealer(1, true, false, Some(offense(0, 4, 1))));
        assert_eq!(dealer.mode, PushPullMode::Neutral);
        assert_eq!(dealer.reason, PushPullReason::TenpaiUnderHighPressure);
    }

    #[test]
    fn dealer_does_not_change_two_shanten_branch() {
        let decision =
            decide_push_pull(&inputs_with_dealer(1, false, true, Some(offense(2, 20, 4))));
        assert_eq!(decision.mode, PushPullMode::Fold);
        assert_eq!(decision.reason, PushPullReason::TwoOrMoreShanten);
    }

    #[test]
    fn dealer_does_not_change_no_opponent_reach() {
        let decision =
            decide_push_pull(&inputs_with_dealer(0, false, true, Some(offense(1, 7, 2))));
        assert_eq!(decision.mode, PushPullMode::Push);
        assert_eq!(decision.reason, PushPullReason::NoOpponentReach);
    }

    #[test]
    fn dealer_does_not_change_missing_offense() {
        let decision = decide_push_pull(&inputs_with_dealer(1, false, true, None));
        assert_eq!(decision.mode, PushPullMode::Neutral);
        assert_eq!(decision.reason, PushPullReason::MissingOffenseEvaluation);
    }

    #[test]
    fn high_value_iishanten_without_value_folds() {
        let offense = offense_with_shape_and_proxy(1, 7, 2, IishantenShape::Weak, 0, 0, 0);
        assert_eq!(offense.simple_value_proxy_after_discard(), 0);

        let decision = decide_push_pull(&inputs_with_dealer(1, false, false, Some(offense)));
        assert_eq!(decision.mode, PushPullMode::Fold);
        assert_eq!(decision.reason, PushPullReason::IishantenUnderHighPressure);
    }

    #[test]
    fn high_value_iishanten_below_threshold_folds() {
        let offense = offense_with_shape_and_proxy(1, 7, 2, IishantenShape::Weak, 3, 1, 0);
        assert_eq!(offense.simple_value_proxy_after_discard(), 3);

        let decision = decide_push_pull(&inputs_with_dealer(1, false, false, Some(offense)));
        assert_eq!(decision.mode, PushPullMode::Fold);
        assert_eq!(decision.reason, PushPullReason::IishantenUnderHighPressure);
    }

    #[test]
    fn high_value_iishanten_boundary_is_neutral() {
        let offense = offense_with_shape_and_proxy(1, 7, 2, IishantenShape::Weak, 4, 1, 0);
        assert_eq!(offense.simple_value_proxy_after_discard(), 4);

        let decision = decide_push_pull(&inputs_with_dealer(1, false, false, Some(offense)));
        assert_eq!(decision.mode, PushPullMode::Neutral);
        assert_eq!(
            decision.reason,
            PushPullReason::HighValueIishantenAgainstSingleNonDealer
        );
    }

    #[test]
    fn high_value_iishanten_above_threshold_is_neutral() {
        // ドラと役牌翻 proxy の合計でも threshold を超えれば同じ扱い。
        let offense = offense_with_shape_and_proxy(1, 7, 2, IishantenShape::Weak, 4, 1, 2);
        assert_eq!(offense.simple_value_proxy_after_discard(), 6);

        let decision = decide_push_pull(&inputs_with_dealer(1, false, false, Some(offense)));
        assert_eq!(decision.mode, PushPullMode::Neutral);
        assert_eq!(
            decision.reason,
            PushPullReason::HighValueIishantenAgainstSingleNonDealer
        );
    }

    #[test]
    fn high_value_iishanten_against_dealer_reach_folds() {
        let offense = offense_with_shape_and_proxy(1, 7, 2, IishantenShape::Weak, 4, 1, 0);
        let decision = decide_push_pull(&inputs_with_dealer(1, true, false, Some(offense)));
        assert_eq!(decision.mode, PushPullMode::Fold);
        assert_eq!(decision.reason, PushPullReason::IishantenUnderHighPressure);
    }

    #[test]
    fn high_value_iishanten_against_multiple_reach_folds() {
        let offense = offense_with_shape_and_proxy(1, 7, 2, IishantenShape::Weak, 4, 1, 0);
        for reach in [2, 3] {
            let decision =
                decide_push_pull(&inputs_with_dealer(reach, false, false, Some(offense)));
            assert_eq!(decision.mode, PushPullMode::Fold, "reach {reach}");
            assert_eq!(
                decision.reason,
                PushPullReason::IishantenUnderHighPressure,
                "reach {reach}"
            );
        }
    }

    #[test]
    fn high_value_two_or_more_shanten_folds() {
        for shanten in [2, 3] {
            let offense =
                offense_with_shape_and_proxy(shanten, 20, 4, IishantenShape::Unknown, 4, 1, 2);
            let decision = decide_push_pull(&inputs_with_dealer(1, false, false, Some(offense)));
            assert_eq!(decision.mode, PushPullMode::Fold, "shanten {shanten}");
            assert_eq!(
                decision.reason,
                PushPullReason::TwoOrMoreShanten,
                "shanten {shanten}"
            );
        }
    }

    #[test]
    fn high_value_does_not_change_tenpai_branch() {
        let offense = offense_with_shape_and_proxy(0, 4, 1, IishantenShape::Unknown, 4, 1, 2);

        let single = decide_push_pull(&inputs_with_dealer(1, false, false, Some(offense)));
        assert_eq!(single.mode, PushPullMode::Push);
        assert_eq!(single.reason, PushPullReason::TenpaiAgainstSingleNonDealer);

        let dealer_reach = decide_push_pull(&inputs_with_dealer(1, true, false, Some(offense)));
        assert_eq!(dealer_reach.mode, PushPullMode::Neutral);
        assert_eq!(dealer_reach.reason, PushPullReason::TenpaiUnderHighPressure);

        let multiple = decide_push_pull(&inputs_with_dealer(2, false, false, Some(offense)));
        assert_eq!(multiple.mode, PushPullMode::Neutral);
        assert_eq!(multiple.reason, PushPullReason::TenpaiUnderHighPressure);
    }

    #[test]
    fn high_value_does_not_change_no_opponent_reach() {
        let offense = offense_with_shape_and_proxy(1, 7, 2, IishantenShape::Weak, 4, 1, 0);
        let decision = decide_push_pull(&inputs_with_dealer(0, false, false, Some(offense)));
        assert_eq!(decision.mode, PushPullMode::Push);
        assert_eq!(decision.reason, PushPullReason::NoOpponentReach);
    }

    #[test]
    fn high_value_iishanten_is_not_applied_when_self_is_dealer() {
        // 自分が親のときは親補正だけを使う。親補正の受け入れに届かなければ高打点でも Fold。
        let offense = offense_with_shape_and_proxy(1, 6, 2, IishantenShape::Weak, 4, 1, 0);
        let decision = decide_push_pull(&inputs_with_dealer(1, false, true, Some(offense)));
        assert_eq!(decision.mode, PushPullMode::Fold);
        assert_eq!(decision.reason, PushPullReason::IishantenUnderHighPressure);
    }

    #[test]
    fn high_value_iishanten_keeps_existing_iishanten_reasons() {
        // 既存の一向聴補正が先に成立する場合は reason を変えない。
        let strong = decide_push_pull(&inputs_with_dealer(
            1,
            false,
            false,
            Some(offense_with_shape_and_proxy(
                1,
                8,
                2,
                IishantenShape::Weak,
                4,
                1,
                0,
            )),
        ));
        assert_eq!(strong.mode, PushPullMode::Neutral);
        assert_eq!(
            strong.reason,
            PushPullReason::StrongIishantenAgainstSingleNonDealer
        );

        let complete = decide_push_pull(&inputs_with_dealer(
            1,
            false,
            false,
            Some(offense_with_shape_and_proxy(
                1,
                6,
                2,
                IishantenShape::Complete,
                4,
                1,
                0,
            )),
        ));
        assert_eq!(complete.mode, PushPullMode::Neutral);
        assert_eq!(
            complete.reason,
            PushPullReason::CompleteIishantenAgainstSingleNonDealer
        );

        let dealer = decide_push_pull(&inputs_with_dealer(
            1,
            false,
            true,
            Some(offense_with_shape_and_proxy(
                1,
                7,
                2,
                IishantenShape::Weak,
                4,
                1,
                0,
            )),
        ));
        assert_eq!(dealer.mode, PushPullMode::Neutral);
        assert_eq!(
            dealer.reason,
            PushPullReason::DealerIishantenAgainstSingleNonDealer
        );
    }

    // ---- 副露済み手牌の通常打牌評価が押し引き入力へ届くこと ----

    // 白ポン1組 + 123456m 78p 55s + ツモ N。N を切ると副露込みの通常形テンパイ。
    fn one_meld_context(own_melds: Vec<crate::meld::Meld>) -> GameContext {
        let hand = [0u8, 4, 8, 12, 17, 20, 60, 64, 89, 90]
            .iter()
            .map(|&value| tile(value))
            .collect();
        let mut melds: [Vec<crate::meld::Meld>; 4] = Default::default();
        melds[0] = own_melds;

        GameContext::from_parts_with_melds(
            Some(tile(120)),
            hand,
            vec![],
            None,
            None,
            Vec::new(),
            Some(0),
            Some(3),
            Default::default(),
            [false, true, false, false],
            melds,
        )
    }

    fn one_meld_pon() -> crate::meld::Meld {
        crate::meld::Meld::new(
            crate::meld::MeldKind::Pon,
            vec![tile(124), tile(125), tile(126)],
            Some(tile(124)),
        )
    }

    fn offense_state_from_normal_discard(context: &GameContext) -> PushPullOffenseState {
        let tiles: Vec<TileId> = context
            .hand_tiles()
            .iter()
            .copied()
            .chain(context.drawn_tile())
            .collect();
        let evaluation = select_best_normal_discard_evaluation(context, &tiles).unwrap();
        push_pull_inputs_from_context_with_evaluation(context, Some(&evaluation))
            .offense
            .unwrap()
    }

    #[test]
    fn fixed_meld_aware_evaluation_reaches_the_offense_state() {
        let context = one_meld_context(vec![one_meld_pon()]);
        let offense = offense_state_from_normal_discard(&context);

        // 通常打牌評価と同じ副露込みの向聴・受け入れがそのまま届く。
        assert_eq!(offense.min_shanten_after_discard, 0);
        assert_eq!(offense.acceptance_total_remaining, 8);
        assert_eq!(offense.acceptance_type_count, 2);

        // threshold は変更していない。単独の子リーチに対するテンパイなので押す。
        let inputs = inputs_with_dealer(1, false, false, Some(offense));
        let decision = decide_push_pull(&inputs);
        assert_eq!(decision.mode, PushPullMode::Push);
        assert_eq!(
            decision.reason,
            PushPullReason::TenpaiAgainstSingleNonDealer
        );
    }

    #[test]
    fn concealed_hand_offense_state_is_unchanged() {
        // 同じ手牌でも副露が無ければ従来どおり二向聴のまま押し引きへ渡る。
        let offense = offense_state_from_normal_discard(&one_meld_context(vec![]));
        assert_eq!(offense.min_shanten_after_discard, 2);

        let decision = decide_push_pull(&inputs_with_dealer(1, false, false, Some(offense)));
        assert_eq!(decision.mode, PushPullMode::Fold);
        assert_eq!(decision.reason, PushPullReason::TwoOrMoreShanten);
    }

    // ここから打牌後の concealed hand の簡易打点 proxy のテスト。
    use bot_logic::{Acceptance, EffectiveShanten, Shanten};

    fn ids(values: &[u8]) -> Vec<TileId> {
        values.iter().map(|&value| tile(value)).collect()
    }

    fn wind(value: u8) -> TileType {
        TileType::new(value).unwrap()
    }

    // 打牌後 proxy 計算で読むのは discard 牌種と discards_red_five だけ。他フィールドはダミー。
    fn proxy_evaluation(discard: TileType, discards_red_five: bool) -> DiscardEvaluation {
        let shanten = EffectiveShanten::Concealed(Shanten {
            standard: 1,
            chiitoitsu: 6,
            kokushi: 13,
        });
        DiscardEvaluation {
            discard,
            count_before_discard: 1,
            shanten_after_discard: shanten,
            acceptance_after_discard: Acceptance {
                current: shanten,
                tiles: Vec::new(),
            },
            shape_penalty: 0,
            floating_tile_value: 0,
            discarded_dora_count: 0,
            discarded_value_honor_count: 0,
            discards_red_five,
            discards_isolated_tile: false,
            standard_iishanten_shape_after_discard: IishantenShape::Unknown,
        }
    }

    fn proxy_context(
        hand: Vec<TileId>,
        dora: Vec<TileId>,
        round_wind: Option<TileType>,
        seat_wind: Option<TileType>,
    ) -> GameContext {
        GameContext::from_parts_with_context(None, hand, dora, round_wind, seat_wind)
    }

    // 5m=type4, 5p=type13, P(白)=type31, E(東)=type27
    fn five_m() -> TileType {
        tile(16).tile_type()
    }
    fn five_p() -> TileType {
        tile(52).tile_type()
    }
    fn haku() -> TileType {
        tile(124).tile_type()
    }
    fn nine_s() -> TileType {
        tile(104).tile_type()
    }

    #[test]
    fn proxy_no_dora_no_red_no_value_honor() {
        // ドラ表示牌なし・赤なし・役牌刻子なし。
        let mut hand = ids(&[0, 4, 8, 12, 20, 24, 28, 32, 36, 40, 44, 48, 56]);
        hand.push(tile(104)); // 捨てる 9s
        let context = proxy_context(hand, vec![], None, None);
        let evaluation = proxy_evaluation(nine_s(), false);
        let proxy = offense_value_proxy_after_discard(&context, &evaluation);
        assert_eq!(proxy.dora_count, 0);
        assert_eq!(proxy.red_dora_count, 0);
        assert_eq!(proxy.value_honor_han_proxy, 0);
    }

    #[test]
    fn proxy_keeps_indicator_dora() {
        // ドラ表示牌 4p、打牌後に通常 5p が残る。
        let mut hand = ids(&[0, 4, 8, 12, 20, 24, 28, 32, 36, 40, 44, 53, 56]);
        hand.push(tile(104));
        let context = proxy_context(hand, ids(&[48]), None, None);
        let evaluation = proxy_evaluation(nine_s(), false);
        let proxy = offense_value_proxy_after_discard(&context, &evaluation);
        assert_eq!(proxy.dora_count, 1);
        assert_eq!(proxy.red_dora_count, 0);
    }

    #[test]
    fn proxy_excludes_discarded_indicator_dora() {
        // ドラ表示牌 4p、実際に通常 5p を捨てるので打牌後には含めない。
        let hand = ids(&[0, 4, 8, 12, 20, 24, 28, 32, 36, 40, 44, 56, 60, 53]);
        let context = proxy_context(hand, ids(&[48]), None, None);
        let evaluation = proxy_evaluation(five_p(), false);
        let proxy = offense_value_proxy_after_discard(&context, &evaluation);
        assert_eq!(proxy.dora_count, 0);
    }

    #[test]
    fn proxy_keeps_red_five() {
        // 打牌後に赤 5m が残り、他にドラがない。
        let mut hand = ids(&[16, 0, 4, 8, 20, 24, 28, 32, 36, 40, 44, 48, 56]);
        hand.push(tile(104));
        let context = proxy_context(hand, vec![], None, None);
        let evaluation = proxy_evaluation(nine_s(), false);
        let proxy = offense_value_proxy_after_discard(&context, &evaluation);
        assert_eq!(proxy.dora_count, 1);
        assert_eq!(proxy.red_dora_count, 1);
    }

    #[test]
    fn proxy_discards_red_five_keeps_black_five() {
        // 通常 5m と赤 5m の両方があり、赤 5m を捨てる。
        let hand = ids(&[16, 17, 0, 4, 8, 20, 24, 28, 32, 36, 40, 44, 48, 56]);
        let context = proxy_context(hand, vec![], None, None);
        let evaluation = proxy_evaluation(five_m(), true);
        let proxy = offense_value_proxy_after_discard(&context, &evaluation);
        assert_eq!(proxy.red_dora_count, 0);
    }

    #[test]
    fn proxy_discards_black_five_keeps_red_five() {
        // 通常 5m と赤 5m の両方があり、通常 5m を捨てる。
        let hand = ids(&[16, 17, 0, 4, 8, 20, 24, 28, 32, 36, 40, 44, 48, 56]);
        let context = proxy_context(hand, vec![], None, None);
        let evaluation = proxy_evaluation(five_m(), false);
        let proxy = offense_value_proxy_after_discard(&context, &evaluation);
        assert_eq!(proxy.red_dora_count, 1);
    }

    #[test]
    fn proxy_red_five_is_also_indicator_dora() {
        // 打牌後に赤 5p が残り、ドラ表示牌 4p でもある。赤ドラ分を重複加算しない。
        let mut hand = ids(&[52, 0, 4, 8, 20, 24, 28, 32, 36, 40, 44, 56, 60]);
        hand.push(tile(104));
        let context = proxy_context(hand, ids(&[48]), None, None);
        let evaluation = proxy_evaluation(nine_s(), false);
        let proxy = offense_value_proxy_after_discard(&context, &evaluation);
        assert_eq!(proxy.dora_count, 2);
        assert_eq!(proxy.red_dora_count, 1);
        // simple proxy には 2 だけ加算(赤ドラの重複加算はしない)。
        assert_eq!(
            proxy.dora_count.saturating_add(proxy.value_honor_han_proxy),
            2
        );
    }

    #[test]
    fn proxy_multiple_same_indicator_dora() {
        // ドラ表示牌 4p が2枚、打牌後に通常 5p が残る。
        let mut hand = ids(&[53, 0, 4, 8, 20, 24, 28, 32, 36, 40, 44, 56, 60]);
        hand.push(tile(104));
        let context = proxy_context(hand, ids(&[48, 49]), None, None);
        let evaluation = proxy_evaluation(nine_s(), false);
        let proxy = offense_value_proxy_after_discard(&context, &evaluation);
        assert_eq!(proxy.dora_count, 2);
    }

    #[test]
    fn proxy_dragon_triplet_without_winds() {
        // 白白白。三元牌は風情報が無くても1として数える。
        let mut hand = ids(&[124, 125, 126, 0, 4, 8, 20, 24, 28, 32, 36, 40]);
        hand.push(tile(104));
        let context = proxy_context(hand, vec![], None, None);
        let evaluation = proxy_evaluation(nine_s(), false);
        let proxy = offense_value_proxy_after_discard(&context, &evaluation);
        assert_eq!(proxy.value_honor_han_proxy, 1);
    }

    #[test]
    fn proxy_round_wind_triplet() {
        // 東東東、場風=東・自風=南。
        let mut hand = ids(&[108, 109, 110, 0, 4, 8, 20, 24, 28, 32, 36, 40]);
        hand.push(tile(104));
        let context = proxy_context(hand, vec![], Some(wind(27)), Some(wind(28)));
        let evaluation = proxy_evaluation(nine_s(), false);
        let proxy = offense_value_proxy_after_discard(&context, &evaluation);
        assert_eq!(proxy.value_honor_han_proxy, 1);
    }

    #[test]
    fn proxy_seat_wind_triplet() {
        // 西西西、場風=南・自風=西。
        let mut hand = ids(&[116, 117, 118, 0, 4, 8, 20, 24, 28, 32, 36, 40]);
        hand.push(tile(104));
        let context = proxy_context(hand, vec![], Some(wind(28)), Some(wind(29)));
        let evaluation = proxy_evaluation(nine_s(), false);
        let proxy = offense_value_proxy_after_discard(&context, &evaluation);
        assert_eq!(proxy.value_honor_han_proxy, 1);
    }

    #[test]
    fn proxy_double_wind_triplet() {
        // 東東東、場風=東・自風=東。連風牌は2。
        let mut hand = ids(&[108, 109, 110, 0, 4, 8, 20, 24, 28, 32, 36, 40]);
        hand.push(tile(104));
        let context = proxy_context(hand, vec![], Some(wind(27)), Some(wind(27)));
        let evaluation = proxy_evaluation(nine_s(), false);
        let proxy = offense_value_proxy_after_discard(&context, &evaluation);
        assert_eq!(proxy.value_honor_han_proxy, 2);
    }

    #[test]
    fn proxy_pair_is_not_counted() {
        // 白白(対子)は数えない。
        let mut hand = ids(&[124, 125, 0, 4, 8, 20, 24, 28, 32, 36, 40, 44]);
        hand.push(tile(104));
        let context = proxy_context(hand, vec![], None, None);
        let evaluation = proxy_evaluation(nine_s(), false);
        let proxy = offense_value_proxy_after_discard(&context, &evaluation);
        assert_eq!(proxy.value_honor_han_proxy, 0);
    }

    #[test]
    fn proxy_four_copies_counted_once() {
        // 白白白白でも刻子・槓子候補1組として一度だけ。
        let mut hand = ids(&[124, 125, 126, 127, 0, 4, 8, 20, 24, 28, 32]);
        hand.push(tile(104));
        let context = proxy_context(hand, vec![], None, None);
        let evaluation = proxy_evaluation(nine_s(), false);
        let proxy = offense_value_proxy_after_discard(&context, &evaluation);
        assert_eq!(proxy.value_honor_han_proxy, 1);
    }

    #[test]
    fn proxy_four_copies_discard_one_keeps_triplet() {
        // 打牌前 白白白白、白を1枚切ると打牌後は白白白。
        let hand = ids(&[124, 125, 126, 127, 0, 4, 8, 20, 24, 28, 32, 36, 40, 44]);
        let context = proxy_context(hand, vec![], None, None);
        let evaluation = proxy_evaluation(haku(), false);
        let proxy = offense_value_proxy_after_discard(&context, &evaluation);
        assert_eq!(proxy.value_honor_han_proxy, 1);
    }

    #[test]
    fn proxy_triplet_discard_one_becomes_pair() {
        // 打牌前 白白白、白を1枚切ると打牌後は白白。
        let hand = ids(&[124, 125, 126, 0, 4, 8, 20, 24, 28, 32, 36, 40, 44, 48]);
        let context = proxy_context(hand, vec![], None, None);
        let evaluation = proxy_evaluation(haku(), false);
        let proxy = offense_value_proxy_after_discard(&context, &evaluation);
        assert_eq!(proxy.value_honor_han_proxy, 0);
    }

    #[test]
    fn proxy_wind_triplet_with_unknown_winds_is_zero() {
        // 東東東、場風・自風とも不明。推測せず数えない。
        let mut hand = ids(&[108, 109, 110, 0, 4, 8, 20, 24, 28, 32, 36, 40]);
        hand.push(tile(104));
        let context = proxy_context(hand, vec![], None, None);
        let evaluation = proxy_evaluation(nine_s(), false);
        let proxy = offense_value_proxy_after_discard(&context, &evaluation);
        assert_eq!(proxy.value_honor_han_proxy, 0);
    }

    #[test]
    fn proxy_composite() {
        // 打牌後: 表示牌ドラ2枚分(赤5p+通常5p, 表示牌 4p)、赤ドラ1枚、白刻子。
        let mut hand = ids(&[52, 53, 124, 125, 126, 0, 4, 8, 20, 24, 28, 32]);
        hand.push(tile(104));
        let context = proxy_context(hand, ids(&[48]), None, None);
        let evaluation = proxy_evaluation(nine_s(), false);
        let proxy = offense_value_proxy_after_discard(&context, &evaluation);
        assert_eq!(proxy.dora_count, 3);
        assert_eq!(proxy.red_dora_count, 1);
        assert_eq!(proxy.value_honor_han_proxy, 1);
        assert_eq!(
            proxy.dora_count.saturating_add(proxy.value_honor_han_proxy),
            4
        );
    }

    // 実戦寄りの14枚 context。ドラ・場風を持たせて proxy が非ゼロになり得る。
    fn proxy_realistic_context() -> GameContext {
        let hand: Vec<_> = ids(&[0, 4, 8, 12, 17, 20, 24, 28, 32, 36, 40, 44, 108]);
        GameContext::from_parts_with_table_state(
            Some(tile(109)),
            hand,
            ids(&[12]),
            Some(wind(27)),
            Some(wind(27)),
            Vec::new(),
            Some(0),
            Some(1),
            Default::default(),
            [false, true, false, false],
        )
    }

    #[test]
    fn with_evaluation_transcribes_value_proxy() {
        let context = proxy_realistic_context();
        let tiles: Vec<_> = context
            .hand_tiles()
            .iter()
            .copied()
            .chain(context.drawn_tile())
            .collect();
        let evaluation = select_best_normal_discard_evaluation(&context, &tiles)
            .expect("evaluation should exist");
        let expected = offense_value_proxy_after_discard(&context, &evaluation);

        let inputs = push_pull_inputs_from_context_with_evaluation(&context, Some(&evaluation));
        let offense = inputs.offense.expect("offense should be present");

        assert_eq!(offense.dora_count_after_discard, expected.dora_count);
        assert_eq!(
            offense.red_dora_count_after_discard,
            expected.red_dora_count
        );
        assert_eq!(
            offense.value_honor_han_proxy_after_discard,
            expected.value_honor_han_proxy
        );
        assert_eq!(
            offense.simple_value_proxy_after_discard(),
            expected
                .dora_count
                .saturating_add(expected.value_honor_han_proxy)
        );
    }

    #[test]
    fn public_helper_matches_with_evaluation_value_proxy() {
        let context = proxy_realistic_context();
        let tiles: Vec<_> = context
            .hand_tiles()
            .iter()
            .copied()
            .chain(context.drawn_tile())
            .collect();
        let evaluation = select_best_normal_discard_evaluation(&context, &tiles);

        let public = push_pull_inputs_from_context(&context);
        let shared = push_pull_inputs_from_context_with_evaluation(&context, evaluation.as_ref());
        assert_eq!(public, shared);

        let public_offense = public.offense.expect("offense should be present");
        let shared_offense = shared.offense.expect("offense should be present");
        assert_eq!(
            public_offense.dora_count_after_discard,
            shared_offense.dora_count_after_discard
        );
        assert_eq!(
            public_offense.red_dora_count_after_discard,
            shared_offense.red_dora_count_after_discard
        );
        assert_eq!(
            public_offense.value_honor_han_proxy_after_discard,
            shared_offense.value_honor_han_proxy_after_discard
        );
        assert_eq!(
            public_offense.simple_value_proxy_after_discard(),
            shared_offense.simple_value_proxy_after_discard()
        );
    }

    #[test]
    fn value_proxy_changes_decision_only_in_high_value_iishanten_branch() {
        // 単独の子リーチ・子の自分・一向聴でだけ、proxy が判定を変える。
        let shape = IishantenShape::Weak;
        let low = offense_with_shape_and_proxy(1, 7, 2, shape, 0, 0, 0);
        let high = offense_with_shape_and_proxy(1, 7, 2, shape, 6, 1, 2);

        let low_decision = decide_push_pull(&inputs_with_dealer(1, false, false, Some(low)));
        assert_eq!(low_decision.mode, PushPullMode::Fold);
        assert_eq!(
            low_decision.reason,
            PushPullReason::IishantenUnderHighPressure
        );

        let high_decision = decide_push_pull(&inputs_with_dealer(1, false, false, Some(high)));
        assert_eq!(high_decision.mode, PushPullMode::Neutral);
        assert_eq!(
            high_decision.reason,
            PushPullReason::HighValueIishantenAgainstSingleNonDealer
        );
    }

    #[test]
    fn value_proxy_does_not_change_other_branches() {
        // 高打点補正の対象外では、同じ向聴数・受け入れ・一向聴形・親情報で proxy だけを変えても
        // 判定は変わらない。
        let cases = [
            // (opponent_reach, dealer_reacher, self_dealer, shanten, remaining, types, shape)
            (0u8, false, false, 1i8, 7u8, 2usize, IishantenShape::Weak), // 他家リーチなし
            (1, false, false, 0, 4, 1, IishantenShape::Unknown),         // テンパイ(単独子リーチ)
            (1, true, false, 0, 4, 1, IishantenShape::Unknown),          // テンパイ(親リーチ)
            (2, false, false, 0, 4, 1, IishantenShape::Unknown),         // テンパイ(複数リーチ)
            (1, false, false, 1, 8, 2, IishantenShape::Weak),            // Strong 一向聴
            (1, false, false, 1, 6, 2, IishantenShape::Complete),        // Complete 一向聴
            (1, false, true, 1, 7, 2, IishantenShape::Weak),             // Dealer 一向聴
            (1, true, false, 1, 7, 2, IishantenShape::Weak),             // 親リーチへの一向聴
            (2, false, false, 1, 7, 2, IishantenShape::Weak),            // 複数リーチへの一向聴
            (1, false, true, 1, 6, 2, IishantenShape::Weak),             // 自分が親の一向聴 Fold
            (1, false, false, 2, 20, 4, IishantenShape::Unknown),        // 二向聴以上
        ];

        for (reach, dealer_reacher, self_dealer, shanten, remaining, types, shape) in cases {
            let low = offense_with_shape_and_proxy(shanten, remaining, types, shape, 0, 0, 0);
            let high = offense_with_shape_and_proxy(shanten, remaining, types, shape, 6, 1, 2);
            let a = inputs_with_dealer(reach, dealer_reacher, self_dealer, Some(low));
            let b = inputs_with_dealer(reach, dealer_reacher, self_dealer, Some(high));
            assert_eq!(
                decide_push_pull(&a),
                decide_push_pull(&b),
                "{reach} {dealer_reacher} {self_dealer} {shanten} {remaining} {types} {shape:?}"
            );
        }
    }

    #[test]
    fn value_proxy_does_not_mutate_context_or_evaluation() {
        let context = proxy_realistic_context();
        let context_before = context.clone();
        let tiles: Vec<_> = context
            .hand_tiles()
            .iter()
            .copied()
            .chain(context.drawn_tile())
            .collect();
        let evaluation = select_best_normal_discard_evaluation(&context, &tiles)
            .expect("evaluation should exist");
        let evaluation_before = evaluation.clone();

        let _ = offense_value_proxy_after_discard(&context, &evaluation);
        let _ = push_pull_inputs_from_context_with_evaluation(&context, Some(&evaluation));

        assert_eq!(context, context_before);
        assert_eq!(evaluation, evaluation_before);
    }

    // ---- 軽量 threat facts を押し引き入力へ渡す ----

    // player 1 が白ポンだけを持ち、リーチ者がいない4席分の facts。
    fn opponent_meld_facts() -> [PlayerThreatFacts; 4] {
        player_threat_facts_from_context(&opponent_meld_context([false; 4]))
    }

    fn opponent_meld_context(reached: [bool; 4]) -> GameContext {
        let mut melds: [Vec<crate::meld::Meld>; 4] = Default::default();
        melds[1] = vec![one_meld_pon()];

        GameContext::from_parts_with_melds(
            None,
            vec![],
            vec![],
            None,
            None,
            Vec::new(),
            Some(0),
            Some(0),
            Default::default(),
            reached,
            melds,
        )
    }

    #[test]
    fn threat_facts_reach_the_push_pull_inputs() {
        let context = opponent_meld_context([false, false, false, true]);
        let inputs = push_pull_inputs_from_context(&context);

        assert_eq!(
            inputs.player_threats,
            player_threat_facts_from_context(&context)
        );
        assert_eq!(inputs.player_threats[1].open_meld_count, 1);
        assert_eq!(inputs.player_threats[1].value_honor_melds.confirmed, 1);
        assert!(inputs.player_threats[3].reached);
    }

    #[test]
    fn threat_facts_entry_point_matches_the_context_entry_point() {
        let context = opponent_meld_context([false, false, false, true]);
        let facts = player_threat_facts_from_context(&context);

        assert_eq!(
            push_pull_inputs_from_threat_facts(&context, facts, None),
            push_pull_inputs_from_context_with_evaluation(&context, None)
        );
    }

    #[test]
    fn reach_inputs_are_derived_from_the_threat_facts() {
        // player_id / oya / reached のあらゆる組み合わせで、既存のリーチ情報と一致する。
        let reach_patterns = [
            [false, false, false, false],
            [true, false, false, false],
            [false, true, false, false],
            [true, true, false, false],
            [false, true, true, false],
            [true, true, true, true],
        ];

        for player_id in [None, Some(0), Some(2)] {
            for oya in [None, Some(0), Some(1)] {
                for reached in reach_patterns {
                    let context = table_state_context(None, vec![], player_id, oya, reached);
                    let inputs = push_pull_inputs_from_context(&context);
                    let reached_opponents = context.reached_opponents();

                    assert_eq!(
                        inputs.opponent_reach_count,
                        reached_opponents.len() as u8,
                        "{player_id:?} {oya:?} {reached:?}"
                    );
                    assert_eq!(
                        inputs.dealer_reacher,
                        oya.is_some_and(|oya| reached_opponents.contains(&usize::from(oya))),
                        "{player_id:?} {oya:?} {reached:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn threat_facts_do_not_change_any_decision() {
        // 副露 facts の有無だけを変えても、mode / reason は既存のまま。
        let facts = opponent_meld_facts();
        assert_eq!(facts[1].open_meld_count, 1);
        assert_eq!(facts[1].value_honor_melds.confirmed, 1);
        assert_eq!(reached_opponent_count(&facts), 0);
        assert_ne!(facts, no_threat_facts());

        let offenses = [
            None,
            Some(offense(0, 4, 1)),
            Some(offense_with_shape(1, 8, 2, IishantenShape::Weak)),
            Some(offense_with_shape(1, 6, 2, IishantenShape::Complete)),
            Some(offense_with_shape(1, 5, 1, IishantenShape::Weak)),
            Some(offense(2, 20, 4)),
        ];
        // (opponent_reach_count, dealer_reacher, self_dealer)
        let seats = [
            (0u8, false, false),
            (1, false, false),
            (1, true, false),
            (1, false, true),
            (2, false, false),
            (2, true, false),
        ];

        for offense in offenses {
            for (reach, dealer_reacher, self_dealer) in seats {
                let plain = inputs_with_dealer(reach, dealer_reacher, self_dealer, offense);
                let melded =
                    inputs_with_threats(reach, dealer_reacher, self_dealer, offense, facts);
                assert_eq!(
                    decide_push_pull(&plain),
                    decide_push_pull(&melded),
                    "{reach} {dealer_reacher} {self_dealer} {offense:?}"
                );
            }
        }
    }

    #[test]
    fn present_open_hands_still_push() {
        // Present に留まる副露相手だけの局面は従来どおり NoOpponentReach → Push。
        let facts = opponent_meld_facts();
        assert!(!has_high_open_hand_threat(&classify_open_hand_threats(
            &facts
        )));

        for offense in [None, Some(offense(2, 20, 4))] {
            let decision = decide_push_pull(&inputs_with_threats(0, false, false, offense, facts));
            assert_eq!(decision.mode, PushPullMode::Push);
            assert_eq!(decision.reason, PushPullReason::NoOpponentReach);
        }
    }

    // ---- High OpenHandThreat に対する押し引き ----

    // ドラも役牌も含まない Chi。
    fn chi_meld() -> crate::meld::Meld {
        crate::meld::Meld::new(
            crate::meld::MeldKind::Chi,
            vec![tile(0), tile(4), tile(8)],
            Some(tile(0)),
        )
    }

    // 指定席が Chi を `count` 個持つ4席分の facts。自分は player 0 で親も player 0。
    fn open_meld_facts_of(
        player: usize,
        count: usize,
        reached: [bool; 4],
        player_id: Option<u8>,
    ) -> [PlayerThreatFacts; 4] {
        let mut melds: [Vec<crate::meld::Meld>; 4] = Default::default();
        melds[player] = (0..count).map(|_| chi_meld()).collect();

        let context = GameContext::from_parts_with_melds(
            None,
            vec![],
            vec![],
            None,
            None,
            Vec::new(),
            player_id,
            Some(0),
            Default::default(),
            reached,
            melds,
        );
        player_threat_facts_from_context(&context)
    }

    // player 1 が3副露で High になる facts。リーチ者はいない。
    fn high_open_hand_facts() -> [PlayerThreatFacts; 4] {
        open_meld_facts_of(1, 3, [false; 4], Some(0))
    }

    // High の副露相手だけがいる局面の押し引き入力。
    fn high_open_hand_inputs(offense: Option<PushPullOffenseState>) -> PushPullInputs {
        high_open_hand_inputs_with_dealer(false, offense)
    }

    fn high_open_hand_inputs_with_dealer(
        self_dealer: bool,
        offense: Option<PushPullOffenseState>,
    ) -> PushPullInputs {
        inputs_with_threats(0, false, self_dealer, offense, high_open_hand_facts())
    }

    fn assert_high_open_hand_decision(
        inputs: &PushPullInputs,
        mode: PushPullMode,
        reason: PushPullReason,
    ) {
        assert!(inputs.has_high_open_hand_threat());
        let decision = decide_push_pull(inputs);
        assert_eq!(decision.mode, mode, "{:?}", inputs.offense);
        assert_eq!(decision.reason, reason, "{:?}", inputs.offense);
    }

    #[test]
    fn no_high_open_hand_threat_keeps_pushing() {
        // 副露相手がいない、または Present しかいない局面は従来どおり。
        for facts in [
            no_threat_facts(),
            opponent_meld_facts(),
            open_meld_facts_of(1, 1, [false; 4], Some(0)),
        ] {
            let inputs = inputs_with_threats(0, false, false, Some(offense(2, 20, 4)), facts);
            assert!(!inputs.has_high_open_hand_threat());

            let decision = decide_push_pull(&inputs);
            assert_eq!(decision.mode, PushPullMode::Push);
            assert_eq!(decision.reason, PushPullReason::NoOpponentReach);
        }
    }

    #[test]
    fn missing_offense_against_a_high_open_hand_is_neutral() {
        // 情報不足を理由に強制 Fold にはしない。
        assert_high_open_hand_decision(
            &high_open_hand_inputs(None),
            PushPullMode::Neutral,
            PushPullReason::MissingOffenseAgainstHighOpenHand,
        );
    }

    #[test]
    fn tenpai_against_a_high_open_hand_pushes() {
        // High の副露相手がいてもテンパイから自動 Fold にはしない。
        for shanten in [0, -1] {
            assert_high_open_hand_decision(
                &high_open_hand_inputs(Some(offense(shanten, 4, 1))),
                PushPullMode::Push,
                PushPullReason::TenpaiAgainstHighOpenHand,
            );
        }
    }

    #[test]
    fn strong_iishanten_against_a_high_open_hand_folds() {
        // 受け入れが強い一向聴 threshold を満たしても、High の副露相手には押さない。
        for offense in [
            offense(1, 8, 2),
            offense(1, 16, 4),
            offense_with_shape(1, 7, 2, IishantenShape::Weak),
            offense_with_shape(1, 8, 1, IishantenShape::Weak),
        ] {
            assert_high_open_hand_decision(
                &high_open_hand_inputs(Some(offense)),
                PushPullMode::Fold,
                PushPullReason::IishantenAgainstHighOpenHand,
            );
        }
    }

    #[test]
    fn complete_iishanten_against_a_high_open_hand_folds() {
        // 完全一向聴の限定補正も High の副露相手には適用しない。
        for offense in [
            offense_with_shape(1, 6, 2, IishantenShape::Complete),
            offense_with_shape(1, 5, 2, IishantenShape::Complete),
            offense_with_shape(1, 6, 1, IishantenShape::Complete),
        ] {
            assert_high_open_hand_decision(
                &high_open_hand_inputs(Some(offense)),
                PushPullMode::Fold,
                PushPullReason::IishantenAgainstHighOpenHand,
            );
        }
    }

    #[test]
    fn dealer_iishanten_against_a_high_open_hand_folds() {
        // 自分が親でも一向聴なら降りる。
        for offense in [
            offense_with_shape(1, 8, 2, IishantenShape::Weak),
            offense_with_shape(1, 7, 2, IishantenShape::Weak),
            offense_with_shape(1, 6, 2, IishantenShape::Weak),
            offense_with_shape(1, 6, 2, IishantenShape::Complete),
        ] {
            assert_high_open_hand_decision(
                &high_open_hand_inputs_with_dealer(true, Some(offense)),
                PushPullMode::Fold,
                PushPullReason::IishantenAgainstHighOpenHand,
            );
        }
    }

    #[test]
    fn high_value_iishanten_against_a_high_open_hand_folds() {
        // 簡易打点 proxy が限定補正の threshold 以上でも一向聴なら降りる。
        let offense = offense_with_shape_and_proxy(1, 7, 2, IishantenShape::Weak, 4, 1, 0);
        assert_eq!(offense.simple_value_proxy_after_discard(), 4);

        assert_high_open_hand_decision(
            &high_open_hand_inputs(Some(offense)),
            PushPullMode::Fold,
            PushPullReason::IishantenAgainstHighOpenHand,
        );

        let offense = offense_with_shape_and_proxy(1, 6, 2, IishantenShape::Weak, 6, 1, 2);
        assert_high_open_hand_decision(
            &high_open_hand_inputs_with_dealer(true, Some(offense)),
            PushPullMode::Fold,
            PushPullReason::IishantenAgainstHighOpenHand,
        );
    }

    #[test]
    fn the_reported_iishanten_against_a_high_open_hand_folds() {
        // 実戦で Neutral になっていた局面の再現。3副露の High に対し、受け入れ8枚 / 3種類の
        // 完全一向聴でも押さない。
        assert_high_open_hand_decision(
            &high_open_hand_inputs(Some(offense_with_shape(1, 8, 3, IishantenShape::Complete))),
            PushPullMode::Fold,
            PushPullReason::IishantenAgainstHighOpenHand,
        );
    }

    #[test]
    fn weak_iishanten_against_a_high_open_hand_folds() {
        for shape in [
            IishantenShape::Weak,
            IishantenShape::Headless,
            IishantenShape::Kuttsuki,
            IishantenShape::Unknown,
        ] {
            assert_high_open_hand_decision(
                &high_open_hand_inputs(Some(offense_with_shape(1, 4, 1, shape))),
                PushPullMode::Fold,
                PushPullReason::IishantenAgainstHighOpenHand,
            );
        }
    }

    #[test]
    fn two_or_more_shanten_against_a_high_open_hand_folds() {
        for shanten in [2, 3] {
            assert_high_open_hand_decision(
                &high_open_hand_inputs(Some(offense(shanten, 30, 6))),
                PushPullMode::Fold,
                PushPullReason::TwoOrMoreShantenAgainstHighOpenHand,
            );
        }
    }

    #[test]
    fn the_high_open_hand_iishanten_does_not_use_the_riichi_iishanten_exceptions() {
        // 一向聴の限定補正 (強い一向聴 / 完全一向聴 / 自分が親 / 簡易高打点) は単独の子リーチ
        // 側だけに残す。High の副露相手には適用せず、条件によらず Fold にする。
        let cases = [
            (
                offense_with_shape(1, 8, 2, IishantenShape::Weak),
                false,
                PushPullMode::Neutral,
            ),
            (
                offense_with_shape(1, 7, 2, IishantenShape::Weak),
                false,
                PushPullMode::Fold,
            ),
            (
                offense_with_shape(1, 6, 2, IishantenShape::Complete),
                false,
                PushPullMode::Neutral,
            ),
            (
                offense_with_shape(1, 5, 2, IishantenShape::Complete),
                false,
                PushPullMode::Fold,
            ),
            (
                offense_with_shape(1, 7, 2, IishantenShape::Weak),
                true,
                PushPullMode::Neutral,
            ),
            (
                offense_with_shape(1, 6, 2, IishantenShape::Weak),
                true,
                PushPullMode::Fold,
            ),
            (
                offense_with_shape_and_proxy(1, 7, 2, IishantenShape::Weak, 4, 1, 0),
                false,
                PushPullMode::Neutral,
            ),
            (
                offense_with_shape_and_proxy(1, 7, 2, IishantenShape::Weak, 3, 0, 0),
                false,
                PushPullMode::Fold,
            ),
        ];

        for (offense, self_dealer, riichi_mode) in cases {
            // 単独の子リーチに対する既存の境界は変えない。
            let riichi =
                decide_push_pull(&inputs_with_dealer(1, false, self_dealer, Some(offense)));
            assert_eq!(
                riichi.mode, riichi_mode,
                "{offense:?} self_dealer={self_dealer}"
            );

            assert_high_open_hand_decision(
                &high_open_hand_inputs_with_dealer(self_dealer, Some(offense)),
                PushPullMode::Fold,
                PushPullReason::IishantenAgainstHighOpenHand,
            );
        }
    }

    #[test]
    fn a_reached_opponent_with_a_high_open_hand_uses_the_combined_policy() {
        // player 1 がリーチ、player 2 が3副露の High。押し引きは複合 threat policy になる。
        let facts = open_meld_facts_of(2, 3, [false, true, false, false], Some(0));
        assert!(facts[1].reached);
        assert_eq!(facts[2].open_meld_count, 3);

        let cases = [
            (None, PushPullReason::MissingOffenseAgainstCombinedThreat),
            (
                Some(offense(0, 4, 1)),
                PushPullReason::TenpaiAgainstCombinedThreat,
            ),
            (
                Some(offense_with_shape(1, 8, 2, IishantenShape::Weak)),
                PushPullReason::IishantenAgainstCombinedThreat,
            ),
            (
                Some(offense_with_shape(1, 6, 2, IishantenShape::Complete)),
                PushPullReason::IishantenAgainstCombinedThreat,
            ),
            (
                Some(offense(2, 20, 4)),
                PushPullReason::TwoOrMoreShantenAgainstCombinedThreat,
            ),
        ];

        for (offense, reason) in cases {
            for self_dealer in [false, true] {
                let melded = inputs_with_threats(1, false, self_dealer, offense, facts);
                assert!(melded.has_combined_threat());
                assert_eq!(
                    decide_push_pull(&melded).reason,
                    reason,
                    "{offense:?} self_dealer={self_dealer}"
                );
            }
        }
    }

    #[test]
    fn a_reached_player_is_not_a_high_open_hand_threat() {
        // 副露しているリーチ者は OpenHandThreat の対象外。リーチ由来の危険度と二重適用しない。
        let facts = open_meld_facts_of(1, 3, [false, true, false, false], Some(0));
        assert!(facts[1].reached);
        assert_eq!(facts[1].open_meld_count, 3);

        let inputs = inputs_with_threats(0, false, false, Some(offense(2, 20, 4)), facts);
        assert!(!inputs.has_high_open_hand_threat());

        let decision = decide_push_pull(&inputs);
        assert_eq!(decision.mode, PushPullMode::Push);
        assert_eq!(decision.reason, PushPullReason::NoOpponentReach);
    }

    #[test]
    fn an_unknown_player_id_is_not_guessed_as_a_high_open_hand_threat() {
        let facts = open_meld_facts_of(1, 3, [false; 4], None);
        assert_eq!(facts[1].open_meld_count, 3);
        assert_eq!(facts[1].is_self, None);

        let inputs = inputs_with_threats(0, false, false, Some(offense(2, 20, 4)), facts);
        assert!(!inputs.has_high_open_hand_threat());

        let decision = decide_push_pull(&inputs);
        assert_eq!(decision.mode, PushPullMode::Push);
        assert_eq!(decision.reason, PushPullReason::NoOpponentReach);
    }

    #[test]
    fn the_open_hand_classification_is_derived_from_the_same_facts() {
        // 押し引き入力の classification は facts から一度だけ導出する。押し引き側で分類し直さない。
        let context = opponent_meld_context([false, false, false, true]);
        let inputs = push_pull_inputs_from_context(&context);

        assert_eq!(
            inputs.open_hand_threats,
            classify_open_hand_threats(&inputs.player_threats)
        );
    }

    // ---- RiichiThreat + High OpenHandThreat の複合 threat に対する押し引き ----

    // player 1 がリーチ、player 2 が3副露で High になる facts。
    fn combined_threat_facts() -> [PlayerThreatFacts; 4] {
        open_meld_facts_of(2, 3, [false, true, false, false], Some(0))
    }

    // player 1 と player 3 がリーチ、player 2 が3副露で High になる facts。
    fn multiple_reach_combined_threat_facts() -> [PlayerThreatFacts; 4] {
        open_meld_facts_of(2, 3, [false, true, false, true], Some(0))
    }

    // 子リーチ1人 + High の副露相手1人。
    fn combined_threat_inputs(offense: Option<PushPullOffenseState>) -> PushPullInputs {
        inputs_with_threats(1, false, false, offense, combined_threat_facts())
    }

    fn assert_combined_threat_decision(
        inputs: &PushPullInputs,
        mode: PushPullMode,
        reason: PushPullReason,
    ) {
        assert!(inputs.has_combined_threat());
        let decision = decide_push_pull(inputs);
        assert_eq!(decision.mode, mode, "{:?}", inputs.offense);
        assert_eq!(decision.reason, reason, "{:?}", inputs.offense);
    }

    #[test]
    fn a_riichi_with_a_high_open_hand_is_a_combined_threat() {
        // リーチ者と High の副露相手が同時にいる場合だけ複合 threat。
        let facts = combined_threat_facts();
        assert_eq!(reached_opponent_count(&facts), 1);

        let combined = inputs_with_threats(1, false, false, Some(offense(0, 4, 1)), facts);
        assert!(combined.has_combined_threat());

        // リーチ者だけ、High の副露相手だけの局面は複合 threat にしない。
        let riichi_only = inputs(1, false, Some(offense(0, 4, 1)));
        assert!(!riichi_only.has_combined_threat());
        let open_hand_only = high_open_hand_inputs(Some(offense(0, 4, 1)));
        assert!(!open_hand_only.has_combined_threat());
    }

    #[test]
    fn a_present_open_hand_with_a_riichi_is_not_a_combined_threat() {
        // Present の副露相手は複合 threat に含めない。既存のリーチ policy のまま。
        let facts = open_meld_facts_of(2, 1, [false, true, false, false], Some(0));
        let inputs = inputs_with_threats(1, false, false, Some(offense(0, 4, 1)), facts);

        assert!(!inputs.has_high_open_hand_threat());
        assert!(!inputs.has_combined_threat());

        let decision = decide_push_pull(&inputs);
        assert_eq!(decision.mode, PushPullMode::Push);
        assert_eq!(
            decision.reason,
            PushPullReason::TenpaiAgainstSingleNonDealer
        );
    }

    #[test]
    fn missing_offense_against_a_combined_threat_is_neutral() {
        // 情報不足を理由に強制 Fold にはしない。
        assert_combined_threat_decision(
            &combined_threat_inputs(None),
            PushPullMode::Neutral,
            PushPullReason::MissingOffenseAgainstCombinedThreat,
        );
    }

    #[test]
    fn tenpai_against_a_combined_threat_is_neutral() {
        // 単独の子リーチだけなら Push だが、複合 threat では押さない。即 Fold にもしない。
        for shanten in [0, -1] {
            assert_combined_threat_decision(
                &combined_threat_inputs(Some(offense(shanten, 8, 2))),
                PushPullMode::Neutral,
                PushPullReason::TenpaiAgainstCombinedThreat,
            );
        }
    }

    #[test]
    fn tenpai_against_a_dealer_reach_with_a_high_open_hand_is_neutral() {
        assert_combined_threat_decision(
            &inputs_with_threats(
                1,
                true,
                false,
                Some(offense(0, 4, 1)),
                combined_threat_facts(),
            ),
            PushPullMode::Neutral,
            PushPullReason::TenpaiAgainstCombinedThreat,
        );
    }

    #[test]
    fn tenpai_against_multiple_reach_with_a_high_open_hand_is_neutral() {
        let facts = multiple_reach_combined_threat_facts();
        assert_eq!(reached_opponent_count(&facts), 2);

        assert_combined_threat_decision(
            &inputs_with_threats(2, false, false, Some(offense(0, 4, 1)), facts),
            PushPullMode::Neutral,
            PushPullReason::TenpaiAgainstCombinedThreat,
        );
    }

    #[test]
    fn iishanten_against_a_combined_threat_folds() {
        // 単独の子リーチや High 単独に対する一向聴の限定補正は複合 threat へ持ち込まない。
        let strong = offense(1, 8, 2);
        let complete = offense_with_shape(1, 6, 2, IishantenShape::Complete);
        let high_value = offense_with_shape_and_proxy(
            1,
            4,
            1,
            IishantenShape::Unknown,
            HIGH_VALUE_IISHANTEN_MIN_SIMPLE_VALUE_PROXY,
            0,
            0,
        );
        let weak = offense(1, 4, 1);

        for offense in [strong, complete, high_value, weak] {
            assert_combined_threat_decision(
                &combined_threat_inputs(Some(offense)),
                PushPullMode::Fold,
                PushPullReason::IishantenAgainstCombinedThreat,
            );
        }

        // 自分が親の一向聴も同じ。
        assert_combined_threat_decision(
            &inputs_with_threats(
                1,
                false,
                true,
                Some(offense(1, DEALER_IISHANTEN_MIN_REMAINING, 2)),
                combined_threat_facts(),
            ),
            PushPullMode::Fold,
            PushPullReason::IishantenAgainstCombinedThreat,
        );
    }

    #[test]
    fn two_or_more_shanten_against_a_combined_threat_folds() {
        for shanten in [2, 3] {
            assert_combined_threat_decision(
                &combined_threat_inputs(Some(offense(shanten, 30, 6))),
                PushPullMode::Fold,
                PushPullReason::TwoOrMoreShantenAgainstCombinedThreat,
            );
        }
    }

    #[test]
    fn the_combined_policy_is_decided_before_the_riichi_policy() {
        // 同じ offense でも、複合 threat は単独リーチより強い pressure として扱う。
        let offense = offense(1, 8, 2);
        let riichi_only = decide_push_pull(&inputs(1, false, Some(offense)));
        assert_eq!(riichi_only.mode, PushPullMode::Neutral);
        assert_eq!(
            riichi_only.reason,
            PushPullReason::StrongIishantenAgainstSingleNonDealer
        );

        assert_combined_threat_decision(
            &combined_threat_inputs(Some(offense)),
            PushPullMode::Fold,
            PushPullReason::IishantenAgainstCombinedThreat,
        );
    }

    #[test]
    fn the_riichi_only_and_open_hand_only_policies_are_unchanged() {
        // 複合 threat 以外の局面は既存 policy の判定結果をそのまま維持する。
        let offense = Some(offense(0, 4, 1));

        let riichi_only = decide_push_pull(&inputs(1, false, offense));
        assert_eq!(riichi_only.mode, PushPullMode::Push);
        assert_eq!(
            riichi_only.reason,
            PushPullReason::TenpaiAgainstSingleNonDealer
        );

        let open_hand_only = decide_push_pull(&high_open_hand_inputs(offense));
        assert_eq!(open_hand_only.mode, PushPullMode::Push);
        assert_eq!(
            open_hand_only.reason,
            PushPullReason::TenpaiAgainstHighOpenHand
        );

        let no_threat = decide_push_pull(&inputs(0, false, offense));
        assert_eq!(no_threat.mode, PushPullMode::Push);
        assert_eq!(no_threat.reason, PushPullReason::NoOpponentReach);
    }

    #[test]
    fn the_combined_threat_is_derived_from_the_shared_classification() {
        // 複合 threat の判定は既存のリーチ情報と classification をそのまま使う。
        let inputs = combined_threat_inputs(Some(offense(1, 8, 2)));

        assert_eq!(
            inputs.has_combined_threat(),
            inputs.opponent_reach_count >= 1
                && has_high_open_hand_threat(&inputs.open_hand_threats)
        );
    }
}
