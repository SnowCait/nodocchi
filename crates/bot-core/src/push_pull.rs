use crate::action::LegalAction;
use crate::context::GameContext;
use crate::discard_selection::{
    select_best_normal_discard_evaluation, selected_discard_tenpai_wait_availability,
};
use crate::open_hand_threat::{
    OpenHandThreatAssessment, classify_open_hand_threats, has_high_open_hand_threat,
};
use crate::threat::{
    PlayerThreatFacts, has_reached_dealer, player_threat_facts_from_context, reached_opponent_count,
};
use bot_logic::{
    DiscardEvaluation, IishantenShape, PermanentFuriten, TenpaiWaitAvailability, TileCounts,
    TileId, TileType, count_dora,
};

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
/// 現在の暫定 policy ([`decide_push_pull`]) は `Neutral` を返さない。明確な threat が無ければ
/// `Push`、明確な threat があれば強いテンパイだけ `Push` で、それ以外は `Fold` になる。
/// `Neutral` は `ShantenAgent` の action 順序としては残してあり、本格的な一向聴押し引きを
/// 実装するときの中間モードとして使う。
///
/// これは暫定 heuristic であり、以下はまだ考慮していない。
///
/// - 正確な打点(翻数・符・点数計算)
/// - 待ち形(両面・カンチャン・単騎など)
/// - 相手ごとの放銃率
/// - 点棒状況
/// - 局・順位条件
///
/// 打牌後の受け入れ・一向聴形・簡易打点 proxy は `PushPullInputs` とログに保持するが、現在の
/// 判定には使わない。本格的な一向聴押し引きを実装するときの解析材料として残している。
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
///
/// `decide_push_pull()` が現在参照するのは `min_shanten_after_discard` と
/// `tenpai_wait_after_discard` だけで、受け入れ・一向聴形・簡易打点 proxy は診断・ログ用に
/// 保持する。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PushPullOffenseState {
    pub min_shanten_after_discard: i8,
    pub acceptance_total_remaining: u8,
    pub acceptance_type_count: usize,
    pub standard_iishanten_shape_after_discard: IishantenShape,

    /// 打牌後にテンパイになる場合の待ちと恒常フリテンの事実。テンパイにならない打牌や、
    /// 待ちを構築できない場合は `None`。
    pub tenpai_wait_after_discard: Option<PushPullTenpaiWaitFacts>,

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

/// 打牌後テンパイの待ちと恒常フリテンの scalar facts。
///
/// 既存の [`TenpaiWaitAvailability`] から転記するだけで、待ち・残枚数・恒常フリテンを押し引き
/// 側で計算し直さない。`PushPullInputs` の `Copy` を保つため、牌種の Vec は持たない。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PushPullTenpaiWaitFacts {
    /// ツモ和了できる牌の残枚数。既存受け入れそのもので、見え牌を反映済み。
    pub tsumo_remaining: u8,
    /// 実際に残っているツモ可能牌の種類数。
    pub tsumo_type_count: usize,
    /// 自分の河による恒常フリテンの状態。河を特定できなければ
    /// [`PermanentFuriten::Unknown`]。
    pub permanent_furiten: PermanentFuriten,
    /// 恒常フリテンの観点でロンできるか。判断できない場合は `None`。
    pub can_ron: Option<bool>,
}

impl PushPullTenpaiWaitFacts {
    /// 既存の待ち診断から押し引きが使う scalar facts だけを転記する。
    fn from_availability(availability: &TenpaiWaitAvailability) -> Self {
        Self {
            tsumo_remaining: availability.tsumo_remaining,
            tsumo_type_count: availability.tsumo_type_count,
            permanent_furiten: availability.permanent_furiten(),
            can_ron: availability.can_ron(),
        }
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
///
/// 現在の暫定 policy は `opponent_reach_count` を threat の有無にだけ使い、`dealer_reacher` /
/// `self_dealer` は判定に使わない。親リーチ・複数リーチ・自分が親でも境界は同じで、これらは
/// 診断・ログ用の事実として保持する。
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
/// 自分の状態 (攻撃評価なし / 強いテンパイ / 弱いテンパイ / 一向聴 / 二向聴以上) と、相手の
/// threat の種類 (`*AgainstReach` / `*AgainstHighOpenHand` / `*AgainstCombinedThreat`) の組で
/// できている。threat の種類が混ざることはない。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PushPullReason {
    /// 他家リーチも High OpenHandThreat もいない。
    NoThreat,
    MissingOffenseAgainstReach,
    StrongTenpaiAgainstReach,
    WeakTenpaiAgainstReach,
    IishantenAgainstReach,
    TwoOrMoreShantenAgainstReach,
    MissingOffenseAgainstHighOpenHand,
    StrongTenpaiAgainstHighOpenHand,
    WeakTenpaiAgainstHighOpenHand,
    IishantenAgainstHighOpenHand,
    TwoOrMoreShantenAgainstHighOpenHand,
    MissingOffenseAgainstCombinedThreat,
    StrongTenpaiAgainstCombinedThreat,
    WeakTenpaiAgainstCombinedThreat,
    IishantenAgainstCombinedThreat,
    TwoOrMoreShantenAgainstCombinedThreat,
}

/// 押し引き判定の結果。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PushPullDecision {
    pub mode: PushPullMode,
    pub reason: PushPullReason,
}

// 明確な threat に対して押せる「強いテンパイ」の暫定 threshold。実戦の regression test に基づき
// 将来調整する。リーチするかどうかを決める REACH_MIN_REMAINING とは別物で、こちらは threat に
// 対して押すかどうかの threshold。
const STRONG_TENPAI_MIN_REMAINING: u8 = 6;

// 恒常フリテンのテンパイはロンできずツモ依存になるため、非フリテンより2枚多く要求する。
const FURITEN_STRONG_TENPAI_MIN_REMAINING: u8 = 8;

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
///
/// 打牌後テンパイの待ちと恒常フリテンも、選択済み打牌の既存経路
/// ([`selected_discard_tenpai_wait_availability`]) から scalar facts だけを転記する。
/// テンパイにならない打牌では `None` になる。
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
            tenpai_wait_after_discard: selected_discard_tenpai_wait_availability(
                context, evaluation,
            )
            .as_ref()
            .map(PushPullTenpaiWaitFacts::from_availability),
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

/// 明確な threat の種類。reason の系列を選ぶためだけに使う。
///
/// 判定境界は3種類とも同じで、種類によって押し引きの条件は変わらない。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ThreatKind {
    /// 他家リーチだけがいる。
    Reach,
    /// 他家リーチが0人で、High OpenHandThreat の相手がいる。
    HighOpenHand,
    /// 他家リーチと High OpenHandThreat の相手が同時にいる。
    Combined,
}

/// threat の種類ごとの reason 一覧。自分の状態と threat の種類の組を1か所にまとめる。
struct ThreatReasons {
    missing_offense: PushPullReason,
    strong_tenpai: PushPullReason,
    weak_tenpai: PushPullReason,
    iishanten: PushPullReason,
    two_or_more_shanten: PushPullReason,
}

impl ThreatKind {
    fn reasons(self) -> ThreatReasons {
        match self {
            ThreatKind::Reach => ThreatReasons {
                missing_offense: PushPullReason::MissingOffenseAgainstReach,
                strong_tenpai: PushPullReason::StrongTenpaiAgainstReach,
                weak_tenpai: PushPullReason::WeakTenpaiAgainstReach,
                iishanten: PushPullReason::IishantenAgainstReach,
                two_or_more_shanten: PushPullReason::TwoOrMoreShantenAgainstReach,
            },
            ThreatKind::HighOpenHand => ThreatReasons {
                missing_offense: PushPullReason::MissingOffenseAgainstHighOpenHand,
                strong_tenpai: PushPullReason::StrongTenpaiAgainstHighOpenHand,
                weak_tenpai: PushPullReason::WeakTenpaiAgainstHighOpenHand,
                iishanten: PushPullReason::IishantenAgainstHighOpenHand,
                two_or_more_shanten: PushPullReason::TwoOrMoreShantenAgainstHighOpenHand,
            },
            ThreatKind::Combined => ThreatReasons {
                missing_offense: PushPullReason::MissingOffenseAgainstCombinedThreat,
                strong_tenpai: PushPullReason::StrongTenpaiAgainstCombinedThreat,
                weak_tenpai: PushPullReason::WeakTenpaiAgainstCombinedThreat,
                iishanten: PushPullReason::IishantenAgainstCombinedThreat,
                two_or_more_shanten: PushPullReason::TwoOrMoreShantenAgainstCombinedThreat,
            },
        }
    }
}

/// 明確な threat の有無と種類。`Present` の副露相手は threat に数えない。
///
/// リーチ情報も High 条件も既存の source of truth をそのまま使い、ここで分類し直さない。
fn threat_kind(inputs: &PushPullInputs) -> Option<ThreatKind> {
    match (
        inputs.opponent_reach_count >= 1,
        inputs.has_high_open_hand_threat(),
    ) {
        (true, true) => Some(ThreatKind::Combined),
        (true, false) => Some(ThreatKind::Reach),
        (false, true) => Some(ThreatKind::HighOpenHand),
        (false, false) => None,
    }
}

/// 明確な threat に対して押せる「強いテンパイ」の暫定条件。
///
/// 待ち枚数と恒常フリテンは、選択済み打牌の既存 [`TenpaiWaitAvailability`] から転記した事実を
/// そのまま使う。押し引き側で待ちや残枚数を数え直さない。
///
/// - 非フリテン: 残枚数が [`STRONG_TENPAI_MIN_REMAINING`] 以上
/// - 恒常フリテン: ロンできずツモ依存になるため [`FURITEN_STRONG_TENPAI_MIN_REMAINING`] 以上
/// - フリテン判定不能: 強いテンパイと推測しない
///
/// 待ち形・待ち種類数・打点は現時点では条件にしない。
fn is_strong_tenpai(offense: &PushPullOffenseState) -> bool {
    let Some(wait) = offense.tenpai_wait_after_discard else {
        return false;
    };

    match wait.permanent_furiten {
        PermanentFuriten::No => wait.tsumo_remaining >= STRONG_TENPAI_MIN_REMAINING,
        PermanentFuriten::Yes => wait.tsumo_remaining >= FURITEN_STRONG_TENPAI_MIN_REMAINING,
        PermanentFuriten::Unknown => false,
    }
}

/// 押し引きを判定する pure な暫定 helper。
///
/// 明確な threat が無ければ従来どおり通常の攻撃判断 (`Push`) を続ける。明確な threat がある
/// 場合は、打牌後が強いテンパイのときだけ押し、それ以外は降りる。
///
/// | 自分の状態 | mode |
/// | --- | --- |
/// | 攻撃評価を作れない | `Fold` |
/// | 強いテンパイ | `Push` |
/// | 強いと確認できないテンパイ | `Fold` |
/// | 一向聴 | `Fold` |
/// | 二向聴以上 | `Fold` |
///
/// 明確な threat は「他家リーチが1人以上」「High OpenHandThreat が1人以上」「その複合」の3種類
/// で、境界はどれも同じ。親リーチ・複数リーチでも強いテンパイなら押し、`Present` の副露相手は
/// threat に数えない。threat の種類は [`PushPullReason`] だけで区別する。
///
/// 情報不足 (攻撃評価なし / テンパイなのに待ちを構築できない / 恒常フリテンが判定不能) の場合は
/// 攻撃継続を推測せず `Fold` にする。`Neutral` にすると通常打牌が防御 fallback より優先され、
/// 実質的に押してしまうため。
///
/// これは説明可能な暫定 policy であり、以下はまだ考慮していない。
///
/// - 一向聴での本格的な押し引き
/// - 正確な打点(翻数・符・点数計算)
/// - 待ち形(両面・カンチャン・単騎など)
/// - 相手ごとの放銃率
/// - 点棒状況・局・順位条件
///
/// 暫定 threshold は実戦の regression test に基づいて将来調整する。この判定結果は
/// `ShantenAgent` の action 選択に反映される。
///
/// - `Push`: Reach → 通常打牌 → 防御 fallback
/// - `Fold`: 防御 fallback → 通常打牌(Reach は抑制)
pub fn decide_push_pull(inputs: &PushPullInputs) -> PushPullDecision {
    // 1. 明確な threat が無ければ従来どおり押す。
    let Some(threat) = threat_kind(inputs) else {
        return PushPullDecision {
            mode: PushPullMode::Push,
            reason: PushPullReason::NoThreat,
        };
    };
    let reasons = threat.reasons();

    // 2. 攻撃評価が無ければ、強いテンパイと確認できないので降りる。
    let Some(offense) = inputs.offense else {
        return PushPullDecision {
            mode: PushPullMode::Fold,
            reason: reasons.missing_offense,
        };
    };

    // 3. テンパイ相当(向聴 <= 0)。強いテンパイだけ押す。
    if offense.min_shanten_after_discard <= 0 {
        let (mode, reason) = if is_strong_tenpai(&offense) {
            (PushPullMode::Push, reasons.strong_tenpai)
        } else {
            (PushPullMode::Fold, reasons.weak_tenpai)
        };
        return PushPullDecision { mode, reason };
    }

    // 4. 一向聴。本格的な一向聴押し引きは別タスクで、現在は押さない。
    if offense.min_shanten_after_discard == 1 {
        return PushPullDecision {
            mode: PushPullMode::Fold,
            reason: reasons.iishanten,
        };
    }

    // 5. 二向聴以上。
    PushPullDecision {
        mode: PushPullMode::Fold,
        reason: reasons.two_or_more_shanten,
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
        offense_tenpai_tsumo_remaining = ?inputs.offense.and_then(|offense| offense.tenpai_wait_after_discard).map(|wait| wait.tsumo_remaining),
        offense_tenpai_tsumo_type_count = ?inputs.offense.and_then(|offense| offense.tenpai_wait_after_discard).map(|wait| wait.tsumo_type_count),
        offense_tenpai_permanent_furiten = ?inputs.offense.and_then(|offense| offense.tenpai_wait_after_discard).map(|wait| wait.permanent_furiten),
        offense_tenpai_can_ron = ?inputs.offense.and_then(|offense| offense.tenpai_wait_after_discard).and_then(|wait| wait.can_ron),
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

    // テンパイにならない打牌後の攻撃評価。テンパイ判定に使う待ち facts は持たない。
    fn offense(shanten: i8, remaining: u8, types: usize) -> PushPullOffenseState {
        offense_with_shape(shanten, remaining, types, IishantenShape::Unknown)
    }

    // 打牌後テンパイの攻撃評価。待ち枚数と恒常フリテンだけを指定する。
    fn tenpai_offense(tsumo_remaining: u8, furiten: PermanentFuriten) -> PushPullOffenseState {
        PushPullOffenseState {
            tenpai_wait_after_discard: Some(PushPullTenpaiWaitFacts {
                tsumo_remaining,
                tsumo_type_count: 1,
                permanent_furiten: furiten,
                can_ron: match furiten {
                    PermanentFuriten::No => Some(true),
                    PermanentFuriten::Yes => Some(false),
                    PermanentFuriten::Unknown => None,
                },
            }),
            ..offense(0, tsumo_remaining, 1)
        }
    }

    // 非フリテンの強いテンパイ / 弱いテンパイの境界。
    fn strong_tenpai_offense() -> PushPullOffenseState {
        tenpai_offense(STRONG_TENPAI_MIN_REMAINING, PermanentFuriten::No)
    }

    fn weak_tenpai_offense() -> PushPullOffenseState {
        tenpai_offense(STRONG_TENPAI_MIN_REMAINING - 1, PermanentFuriten::No)
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
            tenpai_wait_after_discard: None,
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

    // ---- 明確な threat に対する押し引き ----

    fn assert_decision(inputs: &PushPullInputs, mode: PushPullMode, reason: PushPullReason) {
        let decision = decide_push_pull(inputs);
        assert_eq!(decision.mode, mode, "{:?}", inputs.offense);
        assert_eq!(decision.reason, reason, "{:?}", inputs.offense);
    }

    #[test]
    fn no_threat_pushes_without_offense() {
        assert_decision(
            &inputs(0, false, None),
            PushPullMode::Push,
            PushPullReason::NoThreat,
        );
    }

    #[test]
    fn no_threat_pushes_with_any_offense() {
        // threat が無ければ従来どおり通常の攻撃判断を続ける。テンパイの強さも見ない。
        for offense in [
            offense(3, 30, 6),
            offense(2, 20, 4),
            offense(1, 12, 4),
            weak_tenpai_offense(),
            tenpai_offense(2, PermanentFuriten::Yes),
            offense(0, 4, 1),
        ] {
            assert_decision(
                &inputs(0, false, Some(offense)),
                PushPullMode::Push,
                PushPullReason::NoThreat,
            );
        }
    }

    #[test]
    fn missing_offense_against_a_reach_folds() {
        // 情報不足で強いテンパイと確認できないため、攻撃継続を推測しない。
        assert_decision(
            &inputs(1, false, None),
            PushPullMode::Fold,
            PushPullReason::MissingOffenseAgainstReach,
        );
    }

    #[test]
    fn strong_tenpai_against_a_reach_pushes() {
        for shanten in [0, -1] {
            let offense = PushPullOffenseState {
                min_shanten_after_discard: shanten,
                ..strong_tenpai_offense()
            };
            assert_decision(
                &inputs(1, false, Some(offense)),
                PushPullMode::Push,
                PushPullReason::StrongTenpaiAgainstReach,
            );
        }
    }

    #[test]
    fn weak_tenpai_against_a_reach_folds() {
        assert_decision(
            &inputs(1, false, Some(weak_tenpai_offense())),
            PushPullMode::Fold,
            PushPullReason::WeakTenpaiAgainstReach,
        );
    }

    #[test]
    fn the_strong_tenpai_boundary_is_six_live_waits() {
        // 非フリテンは残枚数 6 枚が境界。
        assert!(is_strong_tenpai(&tenpai_offense(6, PermanentFuriten::No)));
        assert!(!is_strong_tenpai(&tenpai_offense(5, PermanentFuriten::No)));

        assert_decision(
            &inputs(1, false, Some(tenpai_offense(6, PermanentFuriten::No))),
            PushPullMode::Push,
            PushPullReason::StrongTenpaiAgainstReach,
        );
        assert_decision(
            &inputs(1, false, Some(tenpai_offense(5, PermanentFuriten::No))),
            PushPullMode::Fold,
            PushPullReason::WeakTenpaiAgainstReach,
        );
    }

    #[test]
    fn the_furiten_strong_tenpai_boundary_is_eight_live_waits() {
        // 恒常フリテンはロンできずツモ依存になるため、非フリテンより2枚多く要求する。
        assert!(is_strong_tenpai(&tenpai_offense(8, PermanentFuriten::Yes)));
        assert!(!is_strong_tenpai(&tenpai_offense(7, PermanentFuriten::Yes)));

        assert_decision(
            &inputs(1, false, Some(tenpai_offense(8, PermanentFuriten::Yes))),
            PushPullMode::Push,
            PushPullReason::StrongTenpaiAgainstReach,
        );
        assert_decision(
            &inputs(1, false, Some(tenpai_offense(7, PermanentFuriten::Yes))),
            PushPullMode::Fold,
            PushPullReason::WeakTenpaiAgainstReach,
        );
    }

    #[test]
    fn an_unknown_furiten_tenpai_is_never_strong() {
        // フリテンを判定できない場合は待ち枚数が十分でも強いテンパイと推測しない。
        for remaining in [6, 8, 20] {
            let offense = tenpai_offense(remaining, PermanentFuriten::Unknown);
            assert!(!is_strong_tenpai(&offense));
            assert_decision(
                &inputs(1, false, Some(offense)),
                PushPullMode::Fold,
                PushPullReason::WeakTenpaiAgainstReach,
            );
        }
    }

    #[test]
    fn a_tenpai_without_wait_facts_is_never_strong() {
        // テンパイなのに待ちを構築できない場合も強いテンパイと確認できない。
        let offense = offense(0, 20, 5);
        assert_eq!(offense.tenpai_wait_after_discard, None);
        assert!(!is_strong_tenpai(&offense));

        assert_decision(
            &inputs(1, false, Some(offense)),
            PushPullMode::Fold,
            PushPullReason::WeakTenpaiAgainstReach,
        );
    }

    #[test]
    fn the_strong_tenpai_boundary_uses_the_wait_after_discard() {
        // 14枚状態の受け入れではなく、実際に選択された打牌後の待ちで判定する。
        let offense = PushPullOffenseState {
            acceptance_total_remaining: 20,
            acceptance_type_count: 6,
            ..tenpai_offense(3, PermanentFuriten::No)
        };
        assert!(!is_strong_tenpai(&offense));

        assert_decision(
            &inputs(1, false, Some(offense)),
            PushPullMode::Fold,
            PushPullReason::WeakTenpaiAgainstReach,
        );
    }

    #[test]
    fn a_dealer_reach_keeps_the_same_boundary() {
        // 親リーチでも強いテンパイなら押し、弱いテンパイなら降りる。
        assert_decision(
            &inputs(1, true, Some(strong_tenpai_offense())),
            PushPullMode::Push,
            PushPullReason::StrongTenpaiAgainstReach,
        );
        assert_decision(
            &inputs(1, true, Some(weak_tenpai_offense())),
            PushPullMode::Fold,
            PushPullReason::WeakTenpaiAgainstReach,
        );
    }

    #[test]
    fn multiple_reaches_keep_the_same_boundary() {
        for (reach_count, dealer_reacher) in [(2, false), (2, true), (3, false)] {
            assert_decision(
                &inputs(reach_count, dealer_reacher, Some(strong_tenpai_offense())),
                PushPullMode::Push,
                PushPullReason::StrongTenpaiAgainstReach,
            );
            assert_decision(
                &inputs(reach_count, dealer_reacher, Some(weak_tenpai_offense())),
                PushPullMode::Fold,
                PushPullReason::WeakTenpaiAgainstReach,
            );
        }
    }

    #[test]
    fn a_self_dealer_keeps_the_same_boundary() {
        // 自分が親でも境界は変えない。
        assert_decision(
            &inputs_with_dealer(1, false, true, Some(strong_tenpai_offense())),
            PushPullMode::Push,
            PushPullReason::StrongTenpaiAgainstReach,
        );
        assert_decision(
            &inputs_with_dealer(1, false, true, Some(weak_tenpai_offense())),
            PushPullMode::Fold,
            PushPullReason::WeakTenpaiAgainstReach,
        );
    }

    #[test]
    fn iishanten_against_a_reach_always_folds() {
        // 旧 strong / complete / dealer / high-value の一向聴補正はすべて撤去した。
        let cases = [
            offense_with_shape(1, 12, 4, IishantenShape::Weak),
            offense_with_shape(1, 8, 2, IishantenShape::Weak),
            offense_with_shape(1, 7, 2, IishantenShape::Weak),
            offense_with_shape(1, 6, 2, IishantenShape::Complete),
            offense_with_shape(1, 8, 2, IishantenShape::Complete),
            offense_with_shape_and_proxy(1, 7, 2, IishantenShape::Weak, 4, 1, 0),
            offense_with_shape_and_proxy(1, 6, 2, IishantenShape::Weak, 6, 1, 2),
        ];

        for offense in cases {
            for self_dealer in [false, true] {
                for (reach_count, dealer_reacher) in [(1, false), (1, true), (2, false)] {
                    assert_decision(
                        &inputs_with_dealer(
                            reach_count,
                            dealer_reacher,
                            self_dealer,
                            Some(offense),
                        ),
                        PushPullMode::Fold,
                        PushPullReason::IishantenAgainstReach,
                    );
                }
            }
        }
    }

    #[test]
    fn two_or_more_shanten_against_a_reach_folds() {
        for shanten in [2, 3] {
            assert_decision(
                &inputs(1, false, Some(offense(shanten, 30, 6))),
                PushPullMode::Fold,
                PushPullReason::TwoOrMoreShantenAgainstReach,
            );
        }
    }

    #[test]
    fn the_iishanten_shape_does_not_change_any_branch() {
        // 一向聴形は診断用に保持するだけで、押し引きには影響しない。
        for shape in [
            IishantenShape::Complete,
            IishantenShape::Weak,
            IishantenShape::Headless,
            IishantenShape::Kuttsuki,
            IishantenShape::Unknown,
        ] {
            assert_decision(
                &inputs(1, false, Some(offense_with_shape(1, 8, 2, shape))),
                PushPullMode::Fold,
                PushPullReason::IishantenAgainstReach,
            );
            assert_decision(
                &inputs(1, false, Some(offense_with_shape(2, 20, 4, shape))),
                PushPullMode::Fold,
                PushPullReason::TwoOrMoreShantenAgainstReach,
            );
            assert_decision(
                &inputs(0, false, Some(offense_with_shape(1, 8, 2, shape))),
                PushPullMode::Push,
                PushPullReason::NoThreat,
            );
        }
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
    fn the_value_proxy_does_not_change_any_branch() {
        // 簡易打点 proxy は診断用に保持するだけで、押し引きには影響しない。
        let cases = [
            (1, PushPullReason::IishantenAgainstReach),
            (2, PushPullReason::TwoOrMoreShantenAgainstReach),
        ];

        for (shanten, reason) in cases {
            for (dora, red, honor) in [(0, 0, 0), (4, 1, 0), (6, 1, 2)] {
                for self_dealer in [false, true] {
                    let offense = offense_with_shape_and_proxy(
                        shanten,
                        8,
                        2,
                        IishantenShape::Weak,
                        dora,
                        red,
                        honor,
                    );
                    assert_decision(
                        &inputs_with_dealer(1, false, self_dealer, Some(offense)),
                        PushPullMode::Fold,
                        reason,
                    );
                }
            }
        }
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

        // 待ち facts も同じ評価から届く。非フリテンで残枚数 8 枚なので強いテンパイ。
        let wait = offense
            .tenpai_wait_after_discard
            .expect("テンパイの待ち facts がある");
        assert_eq!(wait.tsumo_remaining, 8);
        assert_eq!(wait.tsumo_type_count, 2);
        assert_eq!(wait.permanent_furiten, PermanentFuriten::No);
        assert_eq!(wait.can_ron, Some(true));

        let inputs = inputs_with_dealer(1, false, false, Some(offense));
        let decision = decide_push_pull(&inputs);
        assert_eq!(decision.mode, PushPullMode::Push);
        assert_eq!(decision.reason, PushPullReason::StrongTenpaiAgainstReach);
    }

    #[test]
    fn concealed_hand_offense_state_is_unchanged() {
        // 同じ手牌でも副露が無ければ従来どおり二向聴のまま押し引きへ渡る。
        let offense = offense_state_from_normal_discard(&one_meld_context(vec![]));
        assert_eq!(offense.min_shanten_after_discard, 2);

        assert_eq!(offense.tenpai_wait_after_discard, None);

        let decision = decide_push_pull(&inputs_with_dealer(1, false, false, Some(offense)));
        assert_eq!(decision.mode, PushPullMode::Fold);
        assert_eq!(
            decision.reason,
            PushPullReason::TwoOrMoreShantenAgainstReach
        );
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
    fn the_value_proxy_never_changes_a_decision() {
        // 同じ向聴数・受け入れ・一向聴形・親情報で proxy だけを変えても判定は変わらない。
        let cases = [
            // (opponent_reach, dealer_reacher, self_dealer, shanten, remaining, types, shape)
            (0u8, false, false, 1i8, 7u8, 2usize, IishantenShape::Weak), // threat なし
            (1, false, false, 0, 4, 1, IishantenShape::Unknown),         // テンパイ(単独子リーチ)
            (1, true, false, 0, 4, 1, IishantenShape::Unknown),          // テンパイ(親リーチ)
            (2, false, false, 0, 4, 1, IishantenShape::Unknown),         // テンパイ(複数リーチ)
            (1, false, false, 1, 8, 2, IishantenShape::Weak),            // 受け入れの広い一向聴
            (1, false, false, 1, 6, 2, IishantenShape::Complete),        // 完全一向聴
            (1, false, true, 1, 7, 2, IishantenShape::Weak),             // 自分が親の一向聴
            (1, true, false, 1, 7, 2, IishantenShape::Weak),             // 親リーチへの一向聴
            (2, false, false, 1, 7, 2, IishantenShape::Weak),            // 複数リーチへの一向聴
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
        // Present に留まる副露相手は threat に数えない。従来どおり NoThreat → Push。
        let facts = opponent_meld_facts();
        assert!(!has_high_open_hand_threat(&classify_open_hand_threats(
            &facts
        )));

        for offense in [None, Some(offense(2, 20, 4)), Some(weak_tenpai_offense())] {
            let decision = decide_push_pull(&inputs_with_threats(0, false, false, offense, facts));
            assert_eq!(decision.mode, PushPullMode::Push);
            assert_eq!(decision.reason, PushPullReason::NoThreat);
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
        // 副露相手がいない、または Present しかいない局面は threat 扱いしない。
        for facts in [
            no_threat_facts(),
            opponent_meld_facts(),
            open_meld_facts_of(1, 1, [false; 4], Some(0)),
        ] {
            let inputs = inputs_with_threats(0, false, false, Some(offense(2, 20, 4)), facts);
            assert!(!inputs.has_high_open_hand_threat());

            let decision = decide_push_pull(&inputs);
            assert_eq!(decision.mode, PushPullMode::Push);
            assert_eq!(decision.reason, PushPullReason::NoThreat);
        }
    }

    #[test]
    fn missing_offense_against_a_high_open_hand_folds() {
        // 情報不足で強いテンパイと確認できないため降りる。
        assert_high_open_hand_decision(
            &high_open_hand_inputs(None),
            PushPullMode::Fold,
            PushPullReason::MissingOffenseAgainstHighOpenHand,
        );
    }

    #[test]
    fn strong_tenpai_against_a_high_open_hand_pushes() {
        for shanten in [0, -1] {
            let offense = PushPullOffenseState {
                min_shanten_after_discard: shanten,
                ..strong_tenpai_offense()
            };
            assert_high_open_hand_decision(
                &high_open_hand_inputs(Some(offense)),
                PushPullMode::Push,
                PushPullReason::StrongTenpaiAgainstHighOpenHand,
            );
        }
    }

    #[test]
    fn weak_tenpai_against_a_high_open_hand_folds() {
        // 待ち枚数不足・恒常フリテン・フリテン判定不能はどれも強いテンパイにしない。
        for offense in [
            weak_tenpai_offense(),
            tenpai_offense(7, PermanentFuriten::Yes),
            tenpai_offense(20, PermanentFuriten::Unknown),
            offense(0, 20, 5),
        ] {
            assert_high_open_hand_decision(
                &high_open_hand_inputs(Some(offense)),
                PushPullMode::Fold,
                PushPullReason::WeakTenpaiAgainstHighOpenHand,
            );
        }
    }

    #[test]
    fn the_high_open_hand_tenpai_boundary_matches_the_reach_boundary() {
        for (remaining, furiten, mode) in [
            (6, PermanentFuriten::No, PushPullMode::Push),
            (5, PermanentFuriten::No, PushPullMode::Fold),
            (8, PermanentFuriten::Yes, PushPullMode::Push),
            (7, PermanentFuriten::Yes, PushPullMode::Fold),
        ] {
            let offense = tenpai_offense(remaining, furiten);
            let reason = if mode == PushPullMode::Push {
                PushPullReason::StrongTenpaiAgainstHighOpenHand
            } else {
                PushPullReason::WeakTenpaiAgainstHighOpenHand
            };
            assert_high_open_hand_decision(&high_open_hand_inputs(Some(offense)), mode, reason);
        }
    }

    #[test]
    fn iishanten_against_a_high_open_hand_always_folds() {
        // 旧 strong / complete / dealer / high-value の一向聴補正は復活させない。
        let cases = [
            offense(1, 16, 4),
            offense_with_shape(1, 8, 2, IishantenShape::Weak),
            offense_with_shape(1, 7, 2, IishantenShape::Weak),
            offense_with_shape(1, 6, 2, IishantenShape::Complete),
            // 実戦で Neutral になっていた局面と同じ受け入れ8枚 / 3種類の完全一向聴。
            offense_with_shape(1, 8, 3, IishantenShape::Complete),
            offense_with_shape_and_proxy(1, 7, 2, IishantenShape::Weak, 4, 1, 0),
            offense_with_shape_and_proxy(1, 6, 2, IishantenShape::Weak, 6, 1, 2),
        ];

        for offense in cases {
            for self_dealer in [false, true] {
                assert_high_open_hand_decision(
                    &high_open_hand_inputs_with_dealer(self_dealer, Some(offense)),
                    PushPullMode::Fold,
                    PushPullReason::IishantenAgainstHighOpenHand,
                );
            }
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
    fn a_reached_opponent_with_a_high_open_hand_uses_the_combined_policy() {
        // player 1 がリーチ、player 2 が3副露の High。押し引きは複合 threat policy になる。
        let facts = open_meld_facts_of(2, 3, [false, true, false, false], Some(0));
        assert!(facts[1].reached);
        assert_eq!(facts[2].open_meld_count, 3);

        let cases = [
            (None, PushPullReason::MissingOffenseAgainstCombinedThreat),
            (
                Some(strong_tenpai_offense()),
                PushPullReason::StrongTenpaiAgainstCombinedThreat,
            ),
            (
                Some(weak_tenpai_offense()),
                PushPullReason::WeakTenpaiAgainstCombinedThreat,
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
        assert_eq!(decision.reason, PushPullReason::NoThreat);
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
        assert_eq!(decision.reason, PushPullReason::NoThreat);
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

        let combined = inputs_with_threats(1, false, false, Some(strong_tenpai_offense()), facts);
        assert!(combined.has_combined_threat());

        // リーチ者だけ、High の副露相手だけの局面は複合 threat にしない。
        let riichi_only = inputs(1, false, Some(strong_tenpai_offense()));
        assert!(!riichi_only.has_combined_threat());
        let open_hand_only = high_open_hand_inputs(Some(strong_tenpai_offense()));
        assert!(!open_hand_only.has_combined_threat());
    }

    #[test]
    fn a_present_open_hand_with_a_riichi_is_not_a_combined_threat() {
        // Present の副露相手は複合 threat に含めない。リーチだけの reason になる。
        let facts = open_meld_facts_of(2, 1, [false, true, false, false], Some(0));
        let inputs = inputs_with_threats(1, false, false, Some(strong_tenpai_offense()), facts);

        assert!(!inputs.has_high_open_hand_threat());
        assert!(!inputs.has_combined_threat());

        let decision = decide_push_pull(&inputs);
        assert_eq!(decision.mode, PushPullMode::Push);
        assert_eq!(decision.reason, PushPullReason::StrongTenpaiAgainstReach);
    }

    #[test]
    fn missing_offense_against_a_combined_threat_folds() {
        // 情報不足で強いテンパイと確認できないため降りる。
        assert_combined_threat_decision(
            &combined_threat_inputs(None),
            PushPullMode::Fold,
            PushPullReason::MissingOffenseAgainstCombinedThreat,
        );
    }

    #[test]
    fn strong_tenpai_against_a_combined_threat_pushes() {
        // 複合 threat でも強いテンパイなら押す。
        for shanten in [0, -1] {
            let offense = PushPullOffenseState {
                min_shanten_after_discard: shanten,
                ..strong_tenpai_offense()
            };
            assert_combined_threat_decision(
                &combined_threat_inputs(Some(offense)),
                PushPullMode::Push,
                PushPullReason::StrongTenpaiAgainstCombinedThreat,
            );
        }
    }

    #[test]
    fn weak_tenpai_against_a_combined_threat_folds() {
        for offense in [
            weak_tenpai_offense(),
            tenpai_offense(7, PermanentFuriten::Yes),
            tenpai_offense(20, PermanentFuriten::Unknown),
            offense(0, 20, 5),
        ] {
            assert_combined_threat_decision(
                &combined_threat_inputs(Some(offense)),
                PushPullMode::Fold,
                PushPullReason::WeakTenpaiAgainstCombinedThreat,
            );
        }
    }

    #[test]
    fn the_combined_threat_tenpai_boundary_matches_the_reach_boundary() {
        // 親リーチ・複数リーチとの複合でも境界は同じ。
        let cases = [
            (6, PermanentFuriten::No, PushPullMode::Push),
            (5, PermanentFuriten::No, PushPullMode::Fold),
            (8, PermanentFuriten::Yes, PushPullMode::Push),
            (7, PermanentFuriten::Yes, PushPullMode::Fold),
        ];
        let multiple = multiple_reach_combined_threat_facts();
        assert_eq!(reached_opponent_count(&multiple), 2);

        for (remaining, furiten, mode) in cases {
            let offense = Some(tenpai_offense(remaining, furiten));
            let reason = if mode == PushPullMode::Push {
                PushPullReason::StrongTenpaiAgainstCombinedThreat
            } else {
                PushPullReason::WeakTenpaiAgainstCombinedThreat
            };

            assert_combined_threat_decision(&combined_threat_inputs(offense), mode, reason);
            assert_combined_threat_decision(
                &inputs_with_threats(1, true, false, offense, combined_threat_facts()),
                mode,
                reason,
            );
            assert_combined_threat_decision(
                &inputs_with_threats(2, false, false, offense, multiple),
                mode,
                reason,
            );
        }
    }

    #[test]
    fn iishanten_against_a_combined_threat_folds() {
        let cases = [
            offense(1, 8, 2),
            offense_with_shape(1, 6, 2, IishantenShape::Complete),
            offense_with_shape_and_proxy(1, 4, 1, IishantenShape::Unknown, 6, 1, 2),
            offense(1, 4, 1),
        ];

        for offense in cases {
            for self_dealer in [false, true] {
                assert_combined_threat_decision(
                    &inputs_with_threats(
                        1,
                        false,
                        self_dealer,
                        Some(offense),
                        combined_threat_facts(),
                    ),
                    PushPullMode::Fold,
                    PushPullReason::IishantenAgainstCombinedThreat,
                );
            }
        }
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
    fn the_current_policy_never_returns_neutral() {
        // Neutral は ShantenAgent の action 順序としては残しているが、現在の policy は返さない。
        let offenses = [
            None,
            Some(strong_tenpai_offense()),
            Some(weak_tenpai_offense()),
            Some(tenpai_offense(8, PermanentFuriten::Yes)),
            Some(tenpai_offense(20, PermanentFuriten::Unknown)),
            Some(offense(0, 20, 5)),
            Some(offense(1, 16, 4)),
            Some(offense(2, 20, 4)),
        ];

        for offense in offenses {
            for inputs in [
                inputs(0, false, offense),
                inputs(1, false, offense),
                inputs(1, true, offense),
                inputs(2, false, offense),
                high_open_hand_inputs(offense),
                combined_threat_inputs(offense),
            ] {
                assert_ne!(
                    decide_push_pull(&inputs).mode,
                    PushPullMode::Neutral,
                    "{offense:?}"
                );
            }
        }
    }

    #[test]
    fn every_threat_kind_shares_the_same_boundary() {
        // リーチだけ / High だけ / 複合 のどれでも、mode の境界は同じで reason だけが変わる。
        let cases = [
            (None, PushPullMode::Fold),
            (Some(strong_tenpai_offense()), PushPullMode::Push),
            (Some(weak_tenpai_offense()), PushPullMode::Fold),
            (Some(offense(1, 16, 4)), PushPullMode::Fold),
            (Some(offense(2, 20, 4)), PushPullMode::Fold),
        ];

        for (offense, mode) in cases {
            assert_eq!(
                decide_push_pull(&inputs(1, false, offense)).mode,
                mode,
                "riichi {offense:?}"
            );
            assert_eq!(
                decide_push_pull(&high_open_hand_inputs(offense)).mode,
                mode,
                "open hand {offense:?}"
            );
            assert_eq!(
                decide_push_pull(&combined_threat_inputs(offense)).mode,
                mode,
                "combined {offense:?}"
            );
        }
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
