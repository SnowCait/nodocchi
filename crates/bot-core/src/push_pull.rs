use crate::action::LegalAction;
use crate::context::GameContext;
use crate::discard_selection::{
    concealed_tiles_after_discard, select_best_normal_discard_evaluation,
    selected_discard_tenpai_wait_availability, selected_iishanten_forward_metrics_from_context,
};
use crate::offense_value::{TenpaiOffenseValue, evaluate_tenpai_offense_value};
use crate::open_hand_threat::{
    OpenHandThreatAssessment, classify_open_hand_threats, has_high_open_hand_threat,
};
use crate::threat::{
    PlayerThreatFacts, fixed_meld_value_facts, has_reached_dealer,
    player_threat_facts_from_context, reached_opponent_count,
};
use bot_logic::{
    DiscardEvaluation, ForwardMetrics, IishantenShape, PermanentFuriten, TenpaiWaitAvailability,
    TileCounts, TileType, count_dora,
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
/// - 待ち形(両面・カンチャン・単騎など)
/// - 相手ごとの放銃率
/// - 点棒状況
/// - 局・順位条件
///
/// 正確な打点は [`PushPullOffenseState::tenpai_offense_value_after_discard`] として持ち、
/// threat ありの非フリテンテンパイでだけ判定へ反映する。それ以外の局面では打点を判定に
/// 使わない。打牌後の受け入れ・一向聴形・簡易打点 proxy は
/// `PushPullInputs` とログに保持するが、現在の判定には使わない。本格的な一向聴押し引きを
/// 実装するときの解析材料として残している。
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
/// 打点関連フィールドは、打牌後の自分の手牌全体 (concealed hand + 確認できている fixed meld) から
/// 求める簡易 proxy であり、正確な翻数・打点ではない。fixed meld は Ankan を含む全 fixed meld が
/// 対象で、`player_id` が不明で自分の fixed meld を特定できない場合は推測せず数えない。一般役・
/// 符・点数計算はまだ含めない。向聴数・受け入れは fixed meld を考慮した `DiscardEvaluation` の値を
/// そのまま受け取る。
///
/// `decide_push_pull()` が現在参照するのは `min_shanten_after_discard` /
/// `tenpai_wait_after_discard` / `tenpai_offense_value_after_discard` だけで、受け入れ・
/// 一向聴形・簡易打点 proxy・1向聴の前方集計値は診断・ログ用に保持する。簡易打点 proxy は
/// exact 打点とは別物で、一向聴以上の解析材料として残している。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PushPullOffenseState {
    pub min_shanten_after_discard: i8,
    pub acceptance_total_remaining: u8,
    pub acceptance_type_count: usize,
    pub standard_iishanten_shape_after_discard: IishantenShape,

    /// 打牌後にテンパイになる場合の待ちと恒常フリテンの事実。テンパイにならない打牌や、
    /// 待ちを構築できない場合は `None`。
    pub tenpai_wait_after_discard: Option<PushPullTenpaiWaitFacts>,

    /// 打牌後テンパイを攻撃継続した場合の攻撃モードと確定打点。
    ///
    /// 評価するのは「打牌後がテンパイで、恒常フリテンでない」場合だけで、それ以外は `None` に
    /// なる。exact 打点を使う policy がその条件に限られるため、使わない局面では点数計算その
    /// ものを行わない。相手の threat には依存しない。
    ///
    /// 評価した結果として打点を確定できなかった場合は `Some` のまま
    /// [`OffenseValue::Unknown`](crate::offense_value::OffenseValue::Unknown) になる。
    /// 「評価していない」と「評価したが確定しない」は区別する。
    pub tenpai_offense_value_after_discard: Option<TenpaiOffenseValue>,

    /// 打牌後の自分の手牌全体 (concealed hand + 確認できている fixed meld) のドラの総数。
    /// 表示牌ドラと赤ドラを含む。同じ牌を示す表示牌が複数あれば重複分も数え、赤5が表示牌ドラでも
    /// あれば両方数える。Kan は物理牌4枚をそのまま数える。
    pub dora_count_after_discard: u8,
    /// 打牌後の自分の手牌全体 (concealed hand + 確認できている fixed meld) の赤ドラ(赤5)の枚数。
    /// `dora_count_after_discard` の内数であり、合計 proxy へ別途加算しない。
    pub red_dora_count_after_discard: u8,
    /// 打牌後の自分の手牌全体で確認できる役牌の翻 proxy。concealed hand の役牌刻子・槓子候補と、
    /// 自分の fixed meld の確定役牌翻の合計。
    /// 三元牌は1、場風・自風は各1、連風牌(場風かつ自風)は2。concealed hand 側は同じ牌が4枚あっても
    /// 刻子・槓子候補1組として一度だけ数える。Chi は字牌を含まないため役牌翻を持たない。
    /// 場風・自風が不明な風牌は数えない(三元牌は風情報が無くても数える)。
    pub value_honor_han_proxy_after_discard: u8,

    /// 打牌後が1向聴の場合に、通常打牌選択が使った前方集計値。1向聴でなければ `None`。
    ///
    /// 通常打牌選択が持っている値をそのまま転記するだけで、押し引き側で2手先探索も打点集計も
    /// 行わない。1向聴の押し引きはまだこの値を判断に使わず、threshold を決めるための観測値
    /// として診断・ログにだけ出す。
    ///
    /// [`ForwardMetrics::tenpai_wait`] は将来テンパイの待ちを1手目の残枚数で重み付けした
    /// 合計、[`ForwardMetrics::prospective_value`] はその枝を確定打点で重み付けした合計。
    /// 打点を確定できない枝がある場合と、集計対象の枝が1つも無い場合は打点込みの値が `None`
    /// になる。確定しない打点を 0 点として扱わない。
    pub iishanten_forward_metrics: Option<ForwardMetrics>,
}

impl PushPullOffenseState {
    /// 簡易打点 proxy。ドラ総数と役牌翻 proxy の合計で、正確な翻数・打点ではない。
    /// 赤ドラ数は `dora_count_after_discard` に既に含まれるため別途加算しない。
    pub fn simple_value_proxy_after_discard(&self) -> u8 {
        self.dora_count_after_discard
            .saturating_add(self.value_honor_han_proxy_after_discard)
    }

    /// 攻撃継続時の確定打点の残枚数加重合計 [点]。
    ///
    /// 打点を評価していない場合と、評価しても確定しなかった場合はどちらも `None`。既存
    /// [`OffenseValue::Known`](crate::offense_value::OffenseValue::Known) の値をそのまま読み、
    /// 押し引き側で集計し直さない。
    pub fn tenpai_offense_weighted_total(&self) -> Option<u64> {
        self.tenpai_offense_value_after_discard?
            .value
            .weighted_total()
    }

    /// 明確な threat に対して押すために要求する打牌後テンパイの条件。
    ///
    /// 打牌後がテンパイにならない場合と、恒常フリテンを判定できない場合は `None` になり、
    /// 強いテンパイと推測しない。
    ///
    /// - 非フリテン + 攻撃打点を確定できた: [`StrongTenpaiRequirement::WeightedTotal`]。
    ///   要求する残枚数加重合計は [`tenpai_push_weighted_total_min`] が決める。
    /// - 非フリテン + 攻撃打点を確定できない: 従来どおり [`STRONG_TENPAI_MIN_REMAINING`] 枚
    /// - 恒常フリテン: ロンできずツモ依存になるため [`FURITEN_STRONG_TENPAI_MIN_REMAINING`] 枚
    ///
    /// 親リーチ判定は [`PushPullInputs::dealer_reacher`] が source of truth で、ここでは
    /// 受け取った値をそのまま使う。
    pub fn strong_tenpai_requirement(
        &self,
        dealer_reacher: bool,
    ) -> Option<StrongTenpaiRequirement> {
        match self.tenpai_wait_after_discard?.permanent_furiten {
            PermanentFuriten::No => Some(match self.tenpai_offense_weighted_total() {
                Some(_) => StrongTenpaiRequirement::WeightedTotal(tenpai_push_weighted_total_min(
                    dealer_reacher,
                )),
                None => StrongTenpaiRequirement::LiveWait(STRONG_TENPAI_MIN_REMAINING),
            }),
            PermanentFuriten::Yes => Some(StrongTenpaiRequirement::LiveWait(
                FURITEN_STRONG_TENPAI_MIN_REMAINING,
            )),
            PermanentFuriten::Unknown => None,
        }
    }
}

/// 明確な threat に対して押せる「強いテンパイ」に要求する条件。
///
/// 非フリテンで攻撃打点を確定できたかどうかで、要求する量そのものが変わる。どちらも inclusive。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StrongTenpaiRequirement {
    /// 攻撃打点の残枚数加重合計 [点] の下限。待ち枚数と打点の両方を含む1つの値で判定する。
    WeightedTotal(u64),
    /// 待ち枚数の下限。攻撃打点を使えない場合の fallback。
    LiveWait(u8),
}

/// 明確な threat に対して exact 打点で押すために要求する残枚数加重合計 [点]。inclusive。
///
/// 他家リーチ者に親が含まれるかだけで決まる。自分が親かどうかでは変えない。`dealer_reacher` は
/// [`PushPullInputs::dealer_reacher`] が source of truth で、ここで親リーチを判定し直さない。
fn tenpai_push_weighted_total_min(dealer_reacher: bool) -> u64 {
    if dealer_reacher {
        DEALER_REACH_TENPAI_PUSH_WEIGHTED_TOTAL_MIN
    } else {
        TENPAI_PUSH_WEIGHTED_TOTAL_MIN
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

/// 打牌後の自分の手牌全体の簡易打点 proxy の内訳。`PushPullOffenseState` の各フィールドへ転記する前段の計算値。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
struct OffenseValueProxyBreakdown {
    dora_count: u8,
    red_dora_count: u8,
    value_honor_han_proxy: u8,
}

impl OffenseValueProxyBreakdown {
    /// concealed hand 側と fixed meld 側の寄与を一度ずつだけ合計する。
    fn combined_with(self, other: Self) -> Self {
        Self {
            dora_count: self.dora_count.saturating_add(other.dora_count),
            red_dora_count: self.red_dora_count.saturating_add(other.red_dora_count),
            value_honor_han_proxy: self
                .value_honor_han_proxy
                .saturating_add(other.value_honor_han_proxy),
        }
    }
}

/// 打牌後の concealed hand 内で確認できる役牌刻子・槓子候補の翻 proxy。
///
/// 三元牌は常に1。風牌は `round_wind` / `seat_wind` と一致した分だけ数え、連風牌は2。
/// 風情報が不明な風牌は数えない。fixed meld 側の役牌翻はここでは扱わず、既存の
/// [`fixed_meld_value_facts`] から求める。
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

/// 補正済み評価と `GameContext` から、打牌後の自分の手牌全体の簡易打点 proxy の内訳を一度だけ計算する。
///
/// 打牌後の concealed hand の寄与と、自分の fixed meld の寄与を一度ずつだけ合計する。fixed meld は
/// `concealed_tiles_after_discard` の牌集合に含まれないため、`visible_tiles` などから数え直して
/// 二重計上しない。
fn offense_value_proxy_after_discard(
    context: &GameContext,
    evaluation: &DiscardEvaluation,
) -> OffenseValueProxyBreakdown {
    concealed_value_proxy_after_discard(context, evaluation)
        .combined_with(own_fixed_meld_value_proxy(context))
}

/// 自分の fixed meld の簡易打点 proxy の内訳。
///
/// ドラ・赤ドラ・役牌の判定は既存 [`fixed_meld_value_facts`] に一本化し、押し引き側で `MeldKind` や
/// 場風・自風を判定し直さない。Ankan は公開副露ではないが自分の手牌価値の一部なので、相手の
/// OpenHandThreat とは違い全 fixed meld を対象にする。
///
/// `player_id` が不明で自分の fixed meld を特定できない場合は、player 0 を自分と仮定するような補完を
/// せず、確認できない fixed meld の打点を加算しない。
fn own_fixed_meld_value_proxy(context: &GameContext) -> OffenseValueProxyBreakdown {
    let Some(melds) = context.own_melds() else {
        return OffenseValueProxyBreakdown::default();
    };

    let facts = fixed_meld_value_facts(
        melds,
        context.dora_indicators(),
        context.round_wind(),
        context.seat_wind(),
    );

    OffenseValueProxyBreakdown {
        dora_count: facts.dora_count,
        red_dora_count: facts.red_dora_count,
        value_honor_han_proxy: u8::try_from(facts.value_honor_melds.confirmed_han())
            .unwrap_or(u8::MAX),
    }
}

/// 補正済み評価と `GameContext` から、打牌後の concealed hand の簡易打点 proxy の内訳を計算する。
///
/// 実際に切られる物理牌カテゴリ(赤5・通常5)と一致するよう、`concealed_tiles_after_discard` で
/// 物理牌を1枚除いた打牌後の concealed hand へ処理を一元化する。ドラ総数・赤ドラ数・役牌翻 proxy を同じ牌集合から求める。
///
/// 通常の `ShantenAgent` 経路では補正済み評価と合法 action の物理牌情報が一致する不変条件があるため、
/// 一致する物理牌は必ず見つかる。それでも見つからない場合は panic せず、契約違反を `debug_assert` で
/// 検出しつつ release ではデフォルト値(計算不能)を返す。
fn concealed_value_proxy_after_discard(
    context: &GameContext,
    evaluation: &DiscardEvaluation,
) -> OffenseValueProxyBreakdown {
    let Some(tiles) = concealed_tiles_after_discard(context, evaluation) else {
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
/// 現在の暫定 policy は `opponent_reach_count` を threat の有無にだけ使い、`dealer_reacher` は
/// exact 打点で押すときの threshold にだけ使う ([`tenpai_push_weighted_total_min`])。複数リーチでも
/// 親が1人でも含まれていれば親リーチ扱いで、リーチ者数そのものでは境界を変えない。`self_dealer`
/// は判定に使わず、診断・ログ用の事実として保持する。
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

    /// High OpenHandThreat の対象がすべて「1副露かつ河12枚以上」だけで High になった相手か。
    ///
    /// High target の特定には分類済みの `open_hand_threats` を使い、その target の意味は対応する
    /// `player_threats` の観測 facts で確認する。diagnostic reason には依存せず、High 条件そのものも
    /// ここでは分類し直さない。High target がいない場合は `false`。
    pub fn has_only_late_one_meld_high_open_hand_threats(&self) -> bool {
        let mut has_high_target = false;

        for (facts, assessment) in self.player_threats.iter().zip(&self.open_hand_threats) {
            if !assessment.is_high() {
                continue;
            }

            has_high_target = true;
            if facts.open_meld_count != 1 || facts.discard_count < 12 {
                return false;
            }
        }

        has_high_target
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
    /// 終盤の1副露だけが High target の局面で、通常打牌後がテンパイなので押す。
    TenpaiAgainstLateOneMeldHighOpenHand,
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

// テンパイ相当とみなす打牌後の向聴数。和了形 (-1) も含めるため以下で比較する。
const TENPAI_SHANTEN: i8 = 0;

// 明確な threat に対して押せる「強いテンパイ」の暫定 threshold。実戦の regression test に基づき
// 将来調整する。リーチするかどうかを決める REACH_MIN_REMAINING とは別物で、こちらは threat に
// 対して押すかどうかの threshold。攻撃打点を確定できない場合の fallback として使う。
const STRONG_TENPAI_MIN_REMAINING: u8 = 6;

// 恒常フリテンのテンパイはロンできずツモ依存になるため、非フリテンより2枚多く要求する。
const FURITEN_STRONG_TENPAI_MIN_REMAINING: u8 = 8;

// 攻撃打点を確定できた非フリテンのテンパイで要求する残枚数加重合計 [点]。inclusive。
// 加重合計は待ち枚数と打点の両方を含むので、平均打点と待ち枚数を段階的に見る必要は無い。
// 旧 policy の代表的な境界 (3900 × 4枚 / 5200 × 3枚) をそのまま連続的な threshold へ置き換えた値。
const TENPAI_PUSH_WEIGHTED_TOTAL_MIN: u64 = 15_600;

// 他家リーチ者に親が含まれる場合に要求する残枚数加重合計 [点]。inclusive。
// 放銃時の失点が大きいので基本 threshold の 1.5 倍を要求する。リーチ者が複数いても、親が1人でも
// 含まれていればこちらを使う。自分が親かどうかでは変えない。
const DEALER_REACH_TENPAI_PUSH_WEIGHTED_TOTAL_MIN: u64 = 23_400;

/// `GameContext` から押し引き判定の入力を構築する。
///
/// リーチ情報は `GameContext` から構築した脅威 facts
/// ([`player_threat_facts_from_context`]) を source of truth にする。`player_id == None` の
/// 場合は `GameContext::reached_opponents()` と同じ仕様で、リーチフラグが立っている全席を
/// 対象にする。
///
/// 攻撃評価は既存の通常打牌 best 評価 ([`select_best_normal_discard_evaluation`]) を再利用する。
/// 比較 semantics は `ShantenAgent` の通常打牌選択と同じで、1向聴限定の weighted tenpai wait を
/// 含む。打牌候補の絞り込みには `legal_actions` を使わないので、対象は手牌から切れる全打牌候補に
/// なる。手牌とツモ牌が空なら `offense == None`。
///
/// `legal_actions` は攻撃打点を求めるときの合法 Reach 判定にだけ使う。Reach 可否を別経路で
/// 推測し直さないため、合法 action を持たない呼び出し元は空スライスを渡し、その場合は
/// 「リーチできない局面」として扱う。
pub fn push_pull_inputs_from_context(
    context: &GameContext,
    legal_actions: &[LegalAction],
) -> PushPullInputs {
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

    push_pull_inputs_from_context_with_evaluation(context, evaluation.as_ref(), legal_actions)
}

/// すでに計算済みの `DiscardEvaluation` を利用して押し引き入力を構築する crate-private helper。
///
/// 脅威 facts をまだ構築していない入口用。すでに構築済みなら
/// [`push_pull_inputs_from_threat_facts`] へ渡して二重構築を避ける。
///
/// この入口は通常打牌選択の結果を保持しないため、1向聴の前方集計値は選んだ1候補についてだけ
/// 既存の前方評価基盤から求め直す ([`selected_iishanten_forward_metrics_from_context`])。通常の
/// `act()` 経路は選択が計算済みの値をそのまま渡すので、こちらは通らない。
pub(crate) fn push_pull_inputs_from_context_with_evaluation(
    context: &GameContext,
    evaluation: Option<&DiscardEvaluation>,
    legal_actions: &[LegalAction],
) -> PushPullInputs {
    let iishanten_forward_metrics = evaluation.and_then(|evaluation| {
        selected_iishanten_forward_metrics_from_context(context, evaluation)
    });

    push_pull_inputs_from_threat_facts(
        context,
        player_threat_facts_from_context(context),
        evaluation,
        iishanten_forward_metrics,
        legal_actions,
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
///
/// 攻撃継続時の確定打点は、その打点を使う policy が成立する局面
/// ([`should_evaluate_tenpai_offense_value`]) でだけ評価する。合法 Reach の有無は
/// `legal_actions` を source of truth にし、Reach 可否を別経路で推測し直さない。
///
/// `iishanten_forward_metrics` は通常打牌選択が同じ `evaluation` について観測した1向聴の前方
/// 集計値で、押し引き側では転記するだけ。2手先探索も打点集計もここでは行わない。1向聴でない
/// 打牌では `None` を渡す。
pub(crate) fn push_pull_inputs_from_threat_facts(
    context: &GameContext,
    player_threats: [PlayerThreatFacts; 4],
    evaluation: Option<&DiscardEvaluation>,
    iishanten_forward_metrics: Option<ForwardMetrics>,
    legal_actions: &[LegalAction],
) -> PushPullInputs {
    let opponent_reach_count = reached_opponent_count(&player_threats);
    let dealer_reacher = has_reached_dealer(&player_threats);
    let self_dealer = match (context.player_id(), context.oya()) {
        (Some(player_id), Some(oya)) => player_id == oya,
        _ => false,
    };

    let offense = evaluation.map(|evaluation| {
        let value_proxy = offense_value_proxy_after_discard(context, evaluation);
        let tenpai_wait = selected_discard_tenpai_wait_availability(context, evaluation);
        let tenpai_wait_facts = tenpai_wait
            .as_ref()
            .map(PushPullTenpaiWaitFacts::from_availability);
        let tenpai_offense_value = tenpai_wait
            .as_ref()
            .filter(|_| {
                should_evaluate_tenpai_offense_value(
                    evaluation.min_shanten_after_discard(),
                    tenpai_wait_facts,
                )
            })
            .map(|tenpai_wait| {
                evaluate_tenpai_offense_value(context, evaluation, tenpai_wait, legal_actions)
            });

        PushPullOffenseState {
            min_shanten_after_discard: evaluation.min_shanten_after_discard(),
            acceptance_total_remaining: evaluation.acceptance_total_remaining(),
            acceptance_type_count: evaluation.acceptance_type_count(),
            standard_iishanten_shape_after_discard: evaluation
                .standard_iishanten_shape_after_discard,
            tenpai_wait_after_discard: tenpai_wait_facts,
            tenpai_offense_value_after_discard: tenpai_offense_value,
            dora_count_after_discard: value_proxy.dora_count,
            red_dora_count_after_discard: value_proxy.red_dora_count,
            value_honor_han_proxy_after_discard: value_proxy.value_honor_han_proxy,
            iishanten_forward_metrics,
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

/// 攻撃継続時の確定打点を評価する局面か。
///
/// exact 打点を使う policy が「打牌後がテンパイで、恒常フリテンでない」場合だけなので、それ以外
/// では点数計算そのものを行わない。恒常フリテンのテンパイの挙動は今回変えないため、その打点は
/// 求めても使い道が無い。
///
/// 判定材料は自分の手牌から求めた事実だけで、相手の threat には依存しない。offense は threat の
/// 有無で変わらないという既存の性質を保つ。
fn should_evaluate_tenpai_offense_value(
    min_shanten_after_discard: i8,
    wait: Option<PushPullTenpaiWaitFacts>,
) -> bool {
    min_shanten_after_discard <= TENPAI_SHANTEN
        && wait.is_some_and(|wait| wait.permanent_furiten == PermanentFuriten::No)
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

/// 明確な threat に対して押せる「強いテンパイ」の条件。
///
/// 待ち枚数と恒常フリテンは、選択済み打牌の既存 [`TenpaiWaitAvailability`] から転記した事実を
/// そのまま使う。攻撃打点の残枚数加重合計も既存 [`OffenseValue`](crate::offense_value::OffenseValue)
/// の値をそのまま読む。押し引き側で待ち・残枚数・打点を数え直さない。要求する条件は
/// [`PushPullOffenseState::strong_tenpai_requirement`] が1か所で決める。
fn is_strong_tenpai(offense: &PushPullOffenseState, dealer_reacher: bool) -> bool {
    let Some(wait) = offense.tenpai_wait_after_discard else {
        return false;
    };

    match offense.strong_tenpai_requirement(dealer_reacher) {
        Some(StrongTenpaiRequirement::WeightedTotal(min_total)) => offense
            .tenpai_offense_weighted_total()
            .is_some_and(|total| total >= min_total),
        Some(StrongTenpaiRequirement::LiveWait(min_remaining)) => {
            wait.tsumo_remaining >= min_remaining
        }
        None => false,
    }
}

/// 押し引きを判定する pure な暫定 helper。
///
/// 明確な threat が無ければ従来どおり通常の攻撃判断 (`Push`) を続ける。明確な threat がある
/// 場合は、原則として打牌後が強いテンパイのときだけ押し、それ以外は降りる。ただし他家リーチが
/// なく、High target がすべて1副露かつ河12枚以上なら、通常打牌後がテンパイであれば強さを問わず
/// 押す。
///
/// | 自分の状態 | mode |
/// | --- | --- |
/// | 攻撃評価を作れない | `Fold` |
/// | 強いテンパイ | `Push` |
/// | 強いと確認できないテンパイ | `Fold` (終盤1副露 High だけなら `Push`) |
/// | 一向聴 | `Fold` |
/// | 二向聴以上 | `Fold` |
///
/// 明確な threat は「他家リーチが1人以上」「High OpenHandThreat が1人以上」「その複合」の3種類。
/// 終盤1副露 High だけの例外は High OpenHandThreat 単独に限り、Riichi / Combined や2副露以上を
/// 含む High には適用しない。`Present` の副露相手は threat に数えない。
///
/// 情報不足 (攻撃評価なし / テンパイなのに待ちを構築できない / 恒常フリテンが判定不能) の場合は
/// 原則として攻撃継続を推測せず `Fold` にする。ただし終盤1副露 High だけを相手にしたテンパイの
/// 例外では、`min_shanten_after_discard <= 0` を根拠に押すため、待ち情報や恒常フリテンから強い
/// テンパイと確認できなくても `Push` になり得る。原則の局面で `Neutral` にすると通常打牌が防御
/// fallback より優先され、実質的に押してしまうため。
///
/// 強いテンパイの境界は打牌後テンパイの恒常フリテンと攻撃打点で決まる
/// ([`PushPullOffenseState::strong_tenpai_requirement`])。非フリテンで攻撃継続時の打点を
/// 確定できる場合は、待ち枚数と打点の両方を含む残枚数加重合計だけで判定し、確定できない場合と
/// 恒常フリテンでは従来の待ち枚数だけの policy を維持する。
///
/// これは説明可能な暫定 policy であり、以下はまだ考慮していない。
///
/// - 一向聴での本格的な押し引き
/// - 恒常フリテンのテンパイでの正確な打点
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

    // 3. テンパイ相当(向聴 <= 0)。強いテンパイ、または終盤1副露 High だけなら押す。
    if offense.min_shanten_after_discard <= TENPAI_SHANTEN {
        let (mode, reason) = if is_strong_tenpai(&offense, inputs.dealer_reacher) {
            (PushPullMode::Push, reasons.strong_tenpai)
        } else if threat == ThreatKind::HighOpenHand
            && inputs.has_only_late_one_meld_high_open_hand_threats()
        {
            (
                PushPullMode::Push,
                PushPullReason::TenpaiAgainstLateOneMeldHighOpenHand,
            )
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
        offense_tenpai_mode = ?inputs.offense.and_then(|offense| offense.tenpai_offense_value_after_discard).map(|value| value.mode),
        offense_tenpai_value_known = ?inputs.offense.and_then(|offense| offense.tenpai_offense_value_after_discard).map(|value| value.value.is_known()),
        offense_tenpai_average_value = ?inputs.offense.and_then(|offense| offense.tenpai_offense_value_after_discard).and_then(|value| value.value.average_total()),
        offense_tenpai_weighted_total = ?inputs.offense.and_then(|offense| offense.tenpai_offense_weighted_total()),
        offense_strong_tenpai_requirement = ?inputs.offense.and_then(|offense| offense.strong_tenpai_requirement(inputs.dealer_reacher)),
        offense_dora_count_after_discard = ?inputs.offense.map(|offense| offense.dora_count_after_discard),
        offense_red_dora_count_after_discard = ?inputs.offense.map(|offense| offense.red_dora_count_after_discard),
        offense_value_honor_han_proxy_after_discard = ?inputs.offense.map(|offense| offense.value_honor_han_proxy_after_discard),
        offense_simple_value_proxy_after_discard = ?inputs.offense.map(|offense| offense.simple_value_proxy_after_discard()),
        offense_iishanten_prospective_value = ?inputs.offense.and_then(|offense| offense.iishanten_forward_metrics).and_then(|metrics| metrics.prospective_value),
        offense_iishanten_weighted_tenpai_wait_remaining = ?inputs.offense.and_then(|offense| offense.iishanten_forward_metrics).and_then(|metrics| metrics.tenpai_wait).map(|wait| wait.weighted_remaining),
        offense_iishanten_weighted_tenpai_wait_type_count = ?inputs.offense.and_then(|offense| offense.iishanten_forward_metrics).and_then(|metrics| metrics.tenpai_wait).map(|wait| wait.weighted_type_count),
        normal_discard = ?normal_discard,
        "push-pull decision",
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::meld::{Meld, MeldKind};
    use crate::offense_value::{OffenseValue, TenpaiOffenseMode};
    use bot_logic::{TenpaiWaitMetric, TileId, TileType};

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

    // 攻撃打点を確定した非フリテンの打牌後テンパイ。全 variant が同じ支払い合計になる待ちとして
    // 加重合計を組み立てる。
    fn valued_tenpai_offense(tsumo_remaining: u8, total: u32) -> PushPullOffenseState {
        PushPullOffenseState {
            tenpai_offense_value_after_discard: Some(TenpaiOffenseValue {
                mode: TenpaiOffenseMode::Reach,
                value: OffenseValue::Known {
                    weighted_total: u64::from(total) * u64::from(tsumo_remaining),
                    total_remaining: u32::from(tsumo_remaining),
                },
            }),
            ..tenpai_offense(tsumo_remaining, PermanentFuriten::No)
        }
    }

    // 残枚数加重合計だけを指定した非フリテンの打牌後テンパイ。待ち枚数の内訳は判定に使わないので、
    // 加重合計の境界だけを固定したいときに使う。
    fn weighted_total_tenpai_offense(weighted_total: u64) -> PushPullOffenseState {
        PushPullOffenseState {
            tenpai_offense_value_after_discard: Some(TenpaiOffenseValue {
                mode: TenpaiOffenseMode::Reach,
                value: OffenseValue::Known {
                    weighted_total,
                    total_remaining: 4,
                },
            }),
            ..tenpai_offense(4, PermanentFuriten::No)
        }
    }

    // 攻撃打点を評価したが確定しなかった打牌後テンパイ。
    fn unknown_value_tenpai_offense(
        tsumo_remaining: u8,
        furiten: PermanentFuriten,
    ) -> PushPullOffenseState {
        PushPullOffenseState {
            tenpai_offense_value_after_discard: Some(TenpaiOffenseValue {
                mode: TenpaiOffenseMode::Reach,
                value: OffenseValue::Unknown,
            }),
            ..tenpai_offense(tsumo_remaining, furiten)
        }
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
            tenpai_offense_value_after_discard: None,
            dora_count_after_discard: dora,
            red_dora_count_after_discard: red_dora,
            value_honor_han_proxy_after_discard: value_honor_han,
            iishanten_forward_metrics: None,
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
        assert!(is_strong_tenpai(
            &tenpai_offense(6, PermanentFuriten::No),
            false
        ));
        assert!(!is_strong_tenpai(
            &tenpai_offense(5, PermanentFuriten::No),
            false
        ));

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

    // ---- 攻撃打点を確定できたテンパイの押し引き ----

    #[test]
    fn the_tenpai_push_boundary_is_the_weighted_total() {
        // 非フリテンで打点を確定できたテンパイは、待ち枚数と打点の両方を含む残枚数加重合計だけで
        // 判定する。旧 policy の代表的な境界 (3900 × 4枚 / 5200 × 3枚) がそのまま境界になる。
        let cases = [
            (2000, 8, PushPullMode::Push),
            (3900, 4, PushPullMode::Push),
            (5200, 3, PushPullMode::Push),
            (7700, 2, PushPullMode::Fold),
            (8000, 2, PushPullMode::Push),
            (12000, 1, PushPullMode::Fold),
            (16000, 1, PushPullMode::Push),
        ];

        for (total, tsumo_remaining, mode) in cases {
            let offense = valued_tenpai_offense(tsumo_remaining, total);
            assert_eq!(
                offense.tenpai_offense_weighted_total(),
                Some(u64::from(total) * u64::from(tsumo_remaining)),
                "{total} x {tsumo_remaining}"
            );
            assert_eq!(
                offense.strong_tenpai_requirement(false),
                Some(StrongTenpaiRequirement::WeightedTotal(
                    TENPAI_PUSH_WEIGHTED_TOTAL_MIN
                )),
                "{total} x {tsumo_remaining}"
            );

            let reason = match mode {
                PushPullMode::Push => PushPullReason::StrongTenpaiAgainstReach,
                _ => PushPullReason::WeakTenpaiAgainstReach,
            };
            assert_decision(&inputs(1, false, Some(offense)), mode, reason);
        }
    }

    #[test]
    fn the_tenpai_push_weighted_total_threshold_is_inclusive() {
        // 15,600 ちょうどは押し側。加重合計は平均へ割り算せずそのまま threshold と比較する。
        let exact = weighted_total_tenpai_offense(TENPAI_PUSH_WEIGHTED_TOTAL_MIN);
        let below = weighted_total_tenpai_offense(TENPAI_PUSH_WEIGHTED_TOTAL_MIN - 1);

        assert!(is_strong_tenpai(&exact, false));
        assert!(!is_strong_tenpai(&below, false));

        assert_decision(
            &inputs(1, false, Some(exact)),
            PushPullMode::Push,
            PushPullReason::StrongTenpaiAgainstReach,
        );
        assert_decision(
            &inputs(1, false, Some(below)),
            PushPullMode::Fold,
            PushPullReason::WeakTenpaiAgainstReach,
        );
    }

    #[test]
    fn the_live_wait_count_alone_does_not_decide_a_valued_tenpai() {
        // 待ち枚数が多くても打点が足りなければ押さず、待ちが1枚でも打点が足りれば押す。
        // 旧 policy の 3枚 / 4枚 threshold は使わない。
        let many_waits = valued_tenpai_offense(8, 1000);
        assert_eq!(many_waits.tenpai_offense_weighted_total(), Some(8000));
        assert!(!is_strong_tenpai(&many_waits, false));

        let single_wait = valued_tenpai_offense(1, 16000);
        assert!(is_strong_tenpai(&single_wait, false));
    }

    #[test]
    fn a_dealer_reach_requires_one_and_a_half_times_the_weighted_total() {
        // 他家リーチ者に親が含まれる場合だけ 1.5 倍の threshold を使う。
        let cases = [
            (DEALER_REACH_TENPAI_PUSH_WEIGHTED_TOTAL_MIN - 1, false),
            (DEALER_REACH_TENPAI_PUSH_WEIGHTED_TOTAL_MIN, true),
        ];

        for (weighted_total, pushes) in cases {
            let offense = weighted_total_tenpai_offense(weighted_total);
            assert_eq!(
                offense.strong_tenpai_requirement(true),
                Some(StrongTenpaiRequirement::WeightedTotal(
                    DEALER_REACH_TENPAI_PUSH_WEIGHTED_TOTAL_MIN
                ))
            );
            assert_eq!(is_strong_tenpai(&offense, true), pushes, "{weighted_total}");

            let (mode, reason) = if pushes {
                (PushPullMode::Push, PushPullReason::StrongTenpaiAgainstReach)
            } else {
                (PushPullMode::Fold, PushPullReason::WeakTenpaiAgainstReach)
            };
            assert_decision(&inputs(1, true, Some(offense)), mode, reason);
        }
    }

    #[test]
    fn a_dealer_reach_folds_what_a_non_dealer_reach_pushes() {
        // 同じ手でも親リーチかどうかで結論が変わる代表例。
        let cases = [
            (8000, 2, PushPullMode::Fold),
            (12000, 2, PushPullMode::Push),
            (16000, 1, PushPullMode::Fold),
            (16000, 2, PushPullMode::Push),
            (32000, 1, PushPullMode::Push),
        ];

        for (total, tsumo_remaining, mode) in cases {
            let offense = valued_tenpai_offense(tsumo_remaining, total);
            let reason = match mode {
                PushPullMode::Push => PushPullReason::StrongTenpaiAgainstReach,
                _ => PushPullReason::WeakTenpaiAgainstReach,
            };
            assert_decision(&inputs(1, true, Some(offense)), mode, reason);
        }
    }

    #[test]
    fn a_non_dealer_reach_keeps_the_base_weighted_total_threshold() {
        // 子リーチだけなら 15,600。リーチ者数では threshold を変えない。
        let offense = weighted_total_tenpai_offense(TENPAI_PUSH_WEIGHTED_TOTAL_MIN);

        for reach_count in [1, 2, 3] {
            assert_decision(
                &inputs(reach_count, false, Some(offense)),
                PushPullMode::Push,
                PushPullReason::StrongTenpaiAgainstReach,
            );
        }
    }

    #[test]
    fn multiple_reaches_use_the_dealer_threshold_when_a_dealer_is_among_them() {
        // 複数リーチでも親が1人でも含まれていれば 23,400 を要求する。
        let offense =
            weighted_total_tenpai_offense(DEALER_REACH_TENPAI_PUSH_WEIGHTED_TOTAL_MIN - 1);

        for reach_count in [2, 3] {
            assert_decision(
                &inputs(reach_count, true, Some(offense)),
                PushPullMode::Fold,
                PushPullReason::WeakTenpaiAgainstReach,
            );
            assert_decision(
                &inputs(reach_count, false, Some(offense)),
                PushPullMode::Push,
                PushPullReason::StrongTenpaiAgainstReach,
            );
        }
    }

    #[test]
    fn a_self_dealer_does_not_change_the_weighted_total_threshold() {
        // 1.5 倍にするのは相手に親リーチ者がいる場合だけで、自分が親かどうかでは変えない。
        let offense = weighted_total_tenpai_offense(TENPAI_PUSH_WEIGHTED_TOTAL_MIN);

        for self_dealer in [false, true] {
            assert_decision(
                &inputs_with_dealer(1, false, self_dealer, Some(offense)),
                PushPullMode::Push,
                PushPullReason::StrongTenpaiAgainstReach,
            );
        }
    }

    #[test]
    fn an_unconfirmed_offense_value_keeps_the_six_live_wait_boundary() {
        // 打点を評価していない場合と、評価しても確定しない場合はどちらも既存 policy のまま。
        // 親リーチでも 6枚 fallback は 1.5 倍しない。
        for offense in [
            tenpai_offense(STRONG_TENPAI_MIN_REMAINING, PermanentFuriten::No),
            unknown_value_tenpai_offense(STRONG_TENPAI_MIN_REMAINING, PermanentFuriten::No),
        ] {
            assert_eq!(offense.tenpai_offense_weighted_total(), None);
            for dealer_reacher in [false, true] {
                assert_eq!(
                    offense.strong_tenpai_requirement(dealer_reacher),
                    Some(StrongTenpaiRequirement::LiveWait(
                        STRONG_TENPAI_MIN_REMAINING
                    ))
                );
                assert!(is_strong_tenpai(&offense, dealer_reacher));
            }
        }

        for offense in [
            tenpai_offense(STRONG_TENPAI_MIN_REMAINING - 1, PermanentFuriten::No),
            unknown_value_tenpai_offense(STRONG_TENPAI_MIN_REMAINING - 1, PermanentFuriten::No),
        ] {
            for dealer_reacher in [false, true] {
                assert!(!is_strong_tenpai(&offense, dealer_reacher));
                assert_decision(
                    &inputs(1, dealer_reacher, Some(offense)),
                    PushPullMode::Fold,
                    PushPullReason::WeakTenpaiAgainstReach,
                );
            }
        }
    }

    #[test]
    fn a_permanent_furiten_tenpai_ignores_the_offense_value() {
        // 恒常フリテンには残枚数加重合計 policy を適用せず、現行の8枚 policy を維持する。
        // 加重合計が threshold を大きく超える手でも、待ちが足りなければ押さない。
        for total in [8000u32, 5200, 1000] {
            let strong = PushPullOffenseState {
                tenpai_offense_value_after_discard: Some(TenpaiOffenseValue {
                    mode: TenpaiOffenseMode::Reach,
                    value: OffenseValue::Known {
                        weighted_total: u64::from(total) * 8,
                        total_remaining: 8,
                    },
                }),
                ..tenpai_offense(FURITEN_STRONG_TENPAI_MIN_REMAINING, PermanentFuriten::Yes)
            };
            let weak = PushPullOffenseState {
                tenpai_wait_after_discard: tenpai_offense(
                    FURITEN_STRONG_TENPAI_MIN_REMAINING - 1,
                    PermanentFuriten::Yes,
                )
                .tenpai_wait_after_discard,
                ..strong
            };

            for dealer_reacher in [false, true] {
                assert_eq!(
                    strong.strong_tenpai_requirement(dealer_reacher),
                    Some(StrongTenpaiRequirement::LiveWait(
                        FURITEN_STRONG_TENPAI_MIN_REMAINING
                    ))
                );
                assert!(is_strong_tenpai(&strong, dealer_reacher));
                assert!(!is_strong_tenpai(&weak, dealer_reacher));
            }
        }
    }

    #[test]
    fn a_permanent_furiten_tenpai_is_never_pushed_by_the_weighted_total_alone() {
        // 加重合計だけを見れば押せる値でも、恒常フリテンなら7枚待ちでは押さない。
        let offense = PushPullOffenseState {
            tenpai_offense_value_after_discard: Some(TenpaiOffenseValue {
                mode: TenpaiOffenseMode::Reach,
                value: OffenseValue::Known {
                    weighted_total: DEALER_REACH_TENPAI_PUSH_WEIGHTED_TOTAL_MIN * 10,
                    total_remaining: 7,
                },
            }),
            ..tenpai_offense(
                FURITEN_STRONG_TENPAI_MIN_REMAINING - 1,
                PermanentFuriten::Yes,
            )
        };

        assert!(!is_strong_tenpai(&offense, false));
        assert_decision(
            &inputs(1, false, Some(offense)),
            PushPullMode::Fold,
            PushPullReason::WeakTenpaiAgainstReach,
        );
    }

    #[test]
    fn an_unknown_furiten_tenpai_stays_weak_with_any_offense_value() {
        // フリテンを判定できない場合は、打点が確定しても強いテンパイと推測しない。
        let offense = PushPullOffenseState {
            tenpai_offense_value_after_discard: Some(TenpaiOffenseValue {
                mode: TenpaiOffenseMode::Reach,
                value: OffenseValue::Known {
                    weighted_total: 8000 * 20,
                    total_remaining: 20,
                },
            }),
            ..tenpai_offense(20, PermanentFuriten::Unknown)
        };

        for dealer_reacher in [false, true] {
            assert_eq!(offense.strong_tenpai_requirement(dealer_reacher), None);
            assert!(!is_strong_tenpai(&offense, dealer_reacher));
            assert_decision(
                &inputs(1, dealer_reacher, Some(offense)),
                PushPullMode::Fold,
                PushPullReason::WeakTenpaiAgainstReach,
            );
        }
    }

    #[test]
    fn no_threat_pushes_regardless_of_the_offense_value() {
        // threat が無ければ従来どおり押す。打点も待ち枚数も見ない。
        for offense in [
            valued_tenpai_offense(1, 1000),
            valued_tenpai_offense(3, 8000),
            unknown_value_tenpai_offense(1, PermanentFuriten::Yes),
        ] {
            assert_decision(
                &inputs(0, false, Some(offense)),
                PushPullMode::Push,
                PushPullReason::NoThreat,
            );
        }
    }

    #[test]
    fn the_furiten_strong_tenpai_boundary_is_eight_live_waits() {
        // 恒常フリテンはロンできずツモ依存になるため、非フリテンより2枚多く要求する。
        assert!(is_strong_tenpai(
            &tenpai_offense(8, PermanentFuriten::Yes),
            false
        ));
        assert!(!is_strong_tenpai(
            &tenpai_offense(7, PermanentFuriten::Yes),
            false
        ));

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
            assert!(!is_strong_tenpai(&offense, false));
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
        assert!(!is_strong_tenpai(&offense, false));

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
        assert!(!is_strong_tenpai(&offense, false));

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
    fn the_iishanten_forward_metrics_do_not_change_any_branch() {
        // 1向聴の前方集計値は観測用に保持するだけで、押し引きの判断には一切使わない。
        // 打点込みの集計値がどれだけ高くても threat があれば Fold のまま。
        let metrics = |prospective_value: Option<u64>| {
            Some(ForwardMetrics {
                tenpai_wait: Some(TenpaiWaitMetric {
                    weighted_remaining: u32::MAX,
                    weighted_type_count: u32::MAX,
                    prospective_value,
                }),
                next_acceptance: None,
                prospective_value,
                expected_self_tsumo_value: prospective_value,
            })
        };

        for forward in [metrics(Some(u64::MAX)), metrics(None), None] {
            let offense = PushPullOffenseState {
                iishanten_forward_metrics: forward,
                ..offense_with_shape(1, 8, 2, IishantenShape::Complete)
            };

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

            // threat が無ければ従来どおり Push。集計値で Neutral へ振り分けたりしない。
            assert_decision(
                &inputs(0, false, Some(offense)),
                PushPullMode::Push,
                PushPullReason::NoThreat,
            );
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
        let inputs = push_pull_inputs_from_context(&context, &[]);
        assert_eq!(inputs.opponent_reach_count, 1);
    }

    #[test]
    fn opponent_reach_count_without_player_id_counts_all() {
        let context = table_state_context(None, vec![], None, None, [true, false, true, false]);
        let inputs = push_pull_inputs_from_context(&context, &[]);
        assert_eq!(inputs.opponent_reach_count, 2);
    }

    #[test]
    fn dealer_reacher_true_when_oya_is_opponent_reacher() {
        let context =
            table_state_context(None, vec![], Some(0), Some(1), [false, true, false, false]);
        let inputs = push_pull_inputs_from_context(&context, &[]);
        assert!(inputs.dealer_reacher);
    }

    #[test]
    fn dealer_reacher_false_when_self_is_oya() {
        let context =
            table_state_context(None, vec![], Some(0), Some(0), [true, true, false, false]);
        let inputs = push_pull_inputs_from_context(&context, &[]);
        assert!(!inputs.dealer_reacher);
    }

    #[test]
    fn dealer_reacher_false_without_oya() {
        let context = table_state_context(None, vec![], Some(0), None, [false, true, false, false]);
        let inputs = push_pull_inputs_from_context(&context, &[]);
        assert!(!inputs.dealer_reacher);
    }

    #[test]
    fn offense_is_none_without_tiles() {
        let context = table_state_context(None, vec![], Some(0), None, [false, true, false, false]);
        let inputs = push_pull_inputs_from_context(&context, &[]);
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

        let inputs = push_pull_inputs_from_context(&context, &[]);
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

        let inputs = push_pull_inputs_from_context(&context, &[]);
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

        let shared =
            push_pull_inputs_from_context_with_evaluation(&context, evaluation.as_ref(), &[]);
        let public = push_pull_inputs_from_context(&context, &[]);
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

        let public = push_pull_inputs_from_context(&context, &[]);
        assert_eq!(
            public,
            push_pull_inputs_from_context_with_evaluation(&context, normal.as_ref(), &[]),
        );
        assert_ne!(
            public,
            push_pull_inputs_from_context_with_evaluation(&context, one_step.as_ref(), &[]),
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
            let inputs =
                push_pull_inputs_from_context_with_evaluation(&context, Some(&evaluation), &[]);
            let offense = inputs.offense.expect("offense should be present");
            assert_eq!(offense.standard_iishanten_shape_after_discard, shape);
        }
    }

    #[test]
    fn with_evaluation_none_yields_no_offense() {
        let context = table_state_context(None, vec![], Some(0), None, [false, true, false, false]);
        let inputs = push_pull_inputs_from_context_with_evaluation(&context, None, &[]);
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

        let inputs =
            push_pull_inputs_from_context_with_evaluation(&context, evaluation.as_ref(), &[]);

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

        let _ = push_pull_inputs_from_context(&context, &[]);

        assert_eq!(context, before);
    }

    #[test]
    fn self_dealer_true_when_player_is_oya() {
        let context =
            table_state_context(None, vec![], Some(1), Some(1), [false, false, false, false]);
        let inputs = push_pull_inputs_from_context(&context, &[]);
        assert!(inputs.self_dealer);
    }

    #[test]
    fn self_dealer_false_when_player_is_not_oya() {
        let context =
            table_state_context(None, vec![], Some(1), Some(2), [false, false, false, false]);
        let inputs = push_pull_inputs_from_context(&context, &[]);
        assert!(!inputs.self_dealer);
    }

    #[test]
    fn self_dealer_false_without_player_id() {
        let context =
            table_state_context(None, vec![], None, Some(1), [false, false, false, false]);
        let inputs = push_pull_inputs_from_context(&context, &[]);
        assert!(!inputs.self_dealer);
    }

    #[test]
    fn self_dealer_false_without_oya() {
        let context =
            table_state_context(None, vec![], Some(1), None, [false, false, false, false]);
        let inputs = push_pull_inputs_from_context(&context, &[]);
        assert!(!inputs.self_dealer);
    }

    #[test]
    fn self_dealer_and_dealer_reacher_are_distinct() {
        // 自分が親で子1人がリーチ。
        let dealer_self =
            table_state_context(None, vec![], Some(0), Some(0), [false, true, false, false]);
        let inputs = push_pull_inputs_from_context(&dealer_self, &[]);
        assert!(inputs.self_dealer);
        assert!(!inputs.dealer_reacher);
        assert_eq!(inputs.opponent_reach_count, 1);

        // 自分が子で親がリーチ。
        let dealer_reach =
            table_state_context(None, vec![], Some(0), Some(1), [false, true, false, false]);
        let inputs = push_pull_inputs_from_context(&dealer_reach, &[]);
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

    // ---- 実局面の攻撃打点が押し引き入力へ届くこと ----

    struct ExactValueTileSource {
        used: [bool; TileId::COUNT],
    }

    impl ExactValueTileSource {
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

    // 攻撃打点を確定できる局面で、単独の子リーチを受けた押し引き入力を組み立てる。場風・自風・
    // 履歴依存フリテンを既知にして、ダマでロンできる非フリテンのテンパイにする。
    // 攻撃打点の評価条件のうち、局面ごとに変えたい部分。既定は「未リーチで Reach が合法、
    // 履歴依存フリテンなし」。
    #[derive(Clone, Copy)]
    struct ExactValueSetup {
        reach_legal: bool,
        self_reached: bool,
        /// 親の席。既定はリーチしている player 1 ではない席で、子リーチの局面になる。
        oya: u8,
        history_furiten: bot_logic::HistoryFuritenFacts,
    }

    impl Default for ExactValueSetup {
        fn default() -> Self {
            Self {
                reach_legal: true,
                self_reached: false,
                oya: 3,
                history_furiten: bot_logic::HistoryFuritenFacts {
                    same_turn: Some(false),
                    riichi_missed_win: Some(false),
                },
            }
        }
    }

    fn exact_value_inputs(
        hand: &[&str],
        drawn: &str,
        dora_indicators: &[&str],
        extra_visible: &[&str],
    ) -> PushPullInputs {
        exact_value_inputs_with(
            hand,
            drawn,
            dora_indicators,
            extra_visible,
            ExactValueSetup::default(),
        )
    }

    fn exact_value_inputs_with(
        hand: &[&str],
        drawn: &str,
        dora_indicators: &[&str],
        extra_visible: &[&str],
        setup: ExactValueSetup,
    ) -> PushPullInputs {
        let mut source = ExactValueTileSource::new();
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
            .chain(setup.reach_legal.then_some(LegalAction::Reach))
            .collect();

        let context = GameContext::from_parts_with_table_state(
            Some(drawn_tile),
            hand_tiles,
            dora_indicators,
            TileType::from_mjai_type_str("E").ok(),
            crate::context::seat_wind_for_player(0, setup.oya),
            visible,
            Some(0),
            Some(setup.oya),
            Default::default(),
            [setup.self_reached, true, false, false],
        )
        .with_history_furiten_facts(setup.history_furiten);

        push_pull_inputs_from_context(&context, &actions)
    }

    // 平和 + 断幺 + ドラ2 の 6s 待ち。ダマ 7700 なのでダマのまま押す高打点テンパイ。
    const HIGH_VALUE_HAND: [&str; 13] = [
        "2m", "3m", "4m", "6m", "7m", "8m", "2p", "2p", "3s", "4s", "5s", "4s", "5s",
    ];

    // 3p 嵌張の役なしテンパイ。リーチのみ 1300 点の低打点テンパイになる。
    const LOW_VALUE_HAND: [&str; 13] = [
        "2m", "3m", "4m", "4m", "5m", "6m", "6m", "7m", "8m", "2p", "4p", "9s", "9s",
    ];

    #[test]
    fn a_high_value_tenpai_pushes_with_three_live_waits_in_a_real_hand() {
        // ダマ 7700 の 6s 待ちを3枚まで減らした局面。待ち枚数だけの6枚には届かないが、
        // 加重合計 7700 × 3 = 23,100 が 15,600 以上なので押す。
        let inputs = exact_value_inputs(&HIGH_VALUE_HAND, "N", &["1p"], &["3s", "3s", "3s", "6s"]);
        let offense = inputs.offense.expect("攻撃評価がある");
        let wait = offense
            .tenpai_wait_after_discard
            .expect("テンパイの待ち facts がある");

        assert_eq!(wait.tsumo_remaining, 3);
        assert_eq!(wait.permanent_furiten, PermanentFuriten::No);
        assert_eq!(wait.can_ron, Some(true));
        let value = offense
            .tenpai_offense_value_after_discard
            .expect("攻撃打点を評価している");
        assert_eq!(value.mode, TenpaiOffenseMode::Damaten);
        assert_eq!(value.value.average_total(), Some(7700));
        assert_eq!(offense.tenpai_offense_weighted_total(), Some(23_100));
        assert!(!inputs.dealer_reacher);
        assert_eq!(
            offense.strong_tenpai_requirement(inputs.dealer_reacher),
            Some(StrongTenpaiRequirement::WeightedTotal(
                TENPAI_PUSH_WEIGHTED_TOTAL_MIN
            ))
        );

        assert_decision(
            &inputs,
            PushPullMode::Push,
            PushPullReason::StrongTenpaiAgainstReach,
        );
    }

    #[test]
    fn a_high_value_tenpai_folds_with_two_live_waits_in_a_real_hand() {
        // 同じ高打点でも残り2枚なら 7700 × 2 = 15,400 で 15,600 に届かず押さない。
        let inputs = exact_value_inputs(
            &HIGH_VALUE_HAND,
            "N",
            &["1p"],
            &["3s", "3s", "3s", "6s", "6s"],
        );
        let offense = inputs.offense.expect("攻撃評価がある");

        assert_eq!(
            offense
                .tenpai_wait_after_discard
                .expect("テンパイの待ち facts がある")
                .tsumo_remaining,
            2
        );
        assert_eq!(offense.tenpai_offense_weighted_total(), Some(15_400));

        assert_decision(
            &inputs,
            PushPullMode::Fold,
            PushPullReason::WeakTenpaiAgainstReach,
        );
    }

    #[test]
    fn a_dealer_reach_folds_the_same_high_value_three_wait_tenpai() {
        // 同じ 7700 × 3 = 23,100 のテンパイでも、親リーチなら 23,400 に届かず押さない。
        // 親リーチ判定は threat facts から入力へ届いた `dealer_reacher` をそのまま使う。
        let inputs = exact_value_inputs_with(
            &HIGH_VALUE_HAND,
            "N",
            &["1p"],
            &["3s", "3s", "3s", "6s"],
            ExactValueSetup {
                oya: 1,
                ..Default::default()
            },
        );
        let offense = inputs.offense.expect("攻撃評価がある");

        assert!(inputs.dealer_reacher);
        assert_eq!(offense.tenpai_offense_weighted_total(), Some(23_100));
        assert_eq!(
            offense.strong_tenpai_requirement(inputs.dealer_reacher),
            Some(StrongTenpaiRequirement::WeightedTotal(
                DEALER_REACH_TENPAI_PUSH_WEIGHTED_TOTAL_MIN
            ))
        );

        assert_decision(
            &inputs,
            PushPullMode::Fold,
            PushPullReason::WeakTenpaiAgainstReach,
        );
    }

    #[test]
    fn a_low_value_tenpai_folds_with_four_live_waits_in_a_real_hand() {
        // ダマでは役が無いのでリーチ込みで評価する。リーチのみ 1300 点は4枚待ちでも
        // 1300 × 4 = 5200 にしかならず、旧 policy の4枚 threshold と違って押さない。
        let inputs = exact_value_inputs(&LOW_VALUE_HAND, "N", &[], &[]);
        let offense = inputs.offense.expect("攻撃評価がある");
        let value = offense
            .tenpai_offense_value_after_discard
            .expect("攻撃打点を評価している");

        assert_eq!(
            offense
                .tenpai_wait_after_discard
                .expect("テンパイの待ち facts がある")
                .tsumo_remaining,
            4
        );
        assert_eq!(value.mode, TenpaiOffenseMode::Reach);
        assert_eq!(value.value.average_total(), Some(1300));
        assert_eq!(offense.tenpai_offense_weighted_total(), Some(5_200));

        assert_decision(
            &inputs,
            PushPullMode::Fold,
            PushPullReason::WeakTenpaiAgainstReach,
        );
    }

    #[test]
    fn a_low_value_tenpai_folds_with_three_live_waits_in_a_real_hand() {
        // 同じ低打点で残り3枚ならさらに届かない。
        let inputs = exact_value_inputs(&LOW_VALUE_HAND, "N", &[], &["3p"]);
        let offense = inputs.offense.expect("攻撃評価がある");

        assert_eq!(
            offense
                .tenpai_wait_after_discard
                .expect("テンパイの待ち facts がある")
                .tsumo_remaining,
            3
        );
        assert_eq!(offense.tenpai_offense_weighted_total(), Some(3_900));

        assert_decision(
            &inputs,
            PushPullMode::Fold,
            PushPullReason::WeakTenpaiAgainstReach,
        );
    }

    // 平和 + 断幺 + ドラ1 の 6s 待ち。ダマ 3900 だがリーチ込みなら 7700 で、リーチ1翻の有無で
    // 5200 の境界をまたぐ。
    const REACH_CROSSES_THRESHOLD_HAND: [&str; 13] = [
        "2m", "3m", "4m", "6m", "7m", "8m", "2p", "2p", "3s", "4s", "5s", "4s", "5s",
    ];

    #[test]
    fn an_already_reached_tenpai_is_valued_with_the_reach_han() {
        // 既にリーチしている手には Reach action が出ないが、それはダマ手だからではない。
        // ダマ換算の 3900 ではなくリーチ込みの 7700 で評価し、3枚待ちから押す。
        let inputs = exact_value_inputs_with(
            &REACH_CROSSES_THRESHOLD_HAND,
            "N",
            &["1m"],
            &["3s", "3s", "3s", "6s"],
            ExactValueSetup {
                reach_legal: false,
                self_reached: true,
                ..Default::default()
            },
        );
        let offense = inputs.offense.expect("攻撃評価がある");
        let wait = offense
            .tenpai_wait_after_discard
            .expect("テンパイの待ち facts がある");
        let value = offense
            .tenpai_offense_value_after_discard
            .expect("攻撃打点を評価している");

        assert_eq!(wait.tsumo_remaining, 3);
        assert_eq!(wait.permanent_furiten, PermanentFuriten::No);

        assert_eq!(value.mode, TenpaiOffenseMode::Reach);
        assert_eq!(value.value.average_total(), Some(7700));
        assert_eq!(offense.tenpai_offense_weighted_total(), Some(23_100));

        assert_decision(
            &inputs,
            PushPullMode::Push,
            PushPullReason::StrongTenpaiAgainstReach,
        );
    }

    #[test]
    fn an_illegal_reach_before_declaring_stays_damaten() {
        // 同じ手でもまだリーチしていなければダマ手なので、ダマ 3900 のまま評価する。
        // 5200 未満なので3枚待ちでは押さない。
        let inputs = exact_value_inputs_with(
            &REACH_CROSSES_THRESHOLD_HAND,
            "N",
            &["1m"],
            &["3s", "3s", "3s", "6s"],
            ExactValueSetup {
                reach_legal: false,
                ..Default::default()
            },
        );
        let offense = inputs.offense.expect("攻撃評価がある");
        let value = offense
            .tenpai_offense_value_after_discard
            .expect("攻撃打点を評価している");

        assert_eq!(value.mode, TenpaiOffenseMode::Damaten);
        assert_eq!(value.value.average_total(), Some(3900));
        assert_eq!(offense.tenpai_offense_weighted_total(), Some(11_700));

        assert_decision(
            &inputs,
            PushPullMode::Fold,
            PushPullReason::WeakTenpaiAgainstReach,
        );
    }

    #[test]
    fn a_damaten_tenpai_that_cannot_ron_falls_back_to_the_six_live_wait_boundary() {
        // 恒常フリテンではないが、リーチ後の見逃しでロンできないテンパイ。ダマ打点はロン和了を
        // 前提にした値なので、7700 と確定できる手でも攻撃打点としては使わず既存 policy へ落ちる。
        for extra_visible in [
            // 6s 待ち4枚。
            vec!["3s", "3s", "3s"],
            // 6s 待ち3枚。
            vec!["3s", "3s", "3s", "6s"],
        ] {
            let inputs = exact_value_inputs_with(
                &HIGH_VALUE_HAND,
                "N",
                &["1p"],
                &extra_visible,
                ExactValueSetup {
                    reach_legal: false,
                    history_furiten: bot_logic::HistoryFuritenFacts {
                        same_turn: Some(false),
                        riichi_missed_win: Some(true),
                    },
                    ..Default::default()
                },
            );
            let offense = inputs.offense.expect("攻撃評価がある");
            let wait = offense
                .tenpai_wait_after_discard
                .expect("テンパイの待ち facts がある");
            let value = offense
                .tenpai_offense_value_after_discard
                .expect("攻撃打点を評価している");

            assert_eq!(wait.permanent_furiten, PermanentFuriten::No);
            assert_eq!(wait.can_ron, Some(false));
            assert!((3..=4).contains(&wait.tsumo_remaining));

            assert_eq!(value.mode, TenpaiOffenseMode::Damaten);
            assert_eq!(value.value, OffenseValue::Unknown);
            assert_eq!(offense.tenpai_offense_weighted_total(), None);
            assert_eq!(
                offense.strong_tenpai_requirement(inputs.dealer_reacher),
                Some(StrongTenpaiRequirement::LiveWait(
                    STRONG_TENPAI_MIN_REMAINING
                ))
            );

            assert_decision(
                &inputs,
                PushPullMode::Fold,
                PushPullReason::WeakTenpaiAgainstReach,
            );
        }
    }

    #[test]
    fn a_damaten_tenpai_with_unknown_ron_falls_back_to_the_six_live_wait_boundary() {
        // ロン可否が unknown な局面でも、ロンできると推測してダマ打点を使わない。
        let inputs = exact_value_inputs_with(
            &HIGH_VALUE_HAND,
            "N",
            &["1p"],
            &["3s", "3s", "3s"],
            ExactValueSetup {
                reach_legal: false,
                history_furiten: bot_logic::HistoryFuritenFacts {
                    same_turn: None,
                    riichi_missed_win: None,
                },
                ..Default::default()
            },
        );
        let offense = inputs.offense.expect("攻撃評価がある");
        let wait = offense
            .tenpai_wait_after_discard
            .expect("テンパイの待ち facts がある");

        assert_eq!(wait.permanent_furiten, PermanentFuriten::No);
        assert_eq!(wait.can_ron, None);
        assert_eq!(
            offense
                .tenpai_offense_value_after_discard
                .expect("攻撃打点を評価している")
                .value,
            OffenseValue::Unknown
        );
        assert_eq!(
            offense.strong_tenpai_requirement(inputs.dealer_reacher),
            Some(StrongTenpaiRequirement::LiveWait(
                STRONG_TENPAI_MIN_REMAINING
            ))
        );

        assert_decision(
            &inputs,
            PushPullMode::Fold,
            PushPullReason::WeakTenpaiAgainstReach,
        );
    }

    #[test]
    fn a_tenpai_without_a_scoring_context_falls_back_to_the_six_live_wait_boundary() {
        // 場風・自風が不明で打点を確定できない局面は、従来どおり6枚が境界になる。
        let mut source = ExactValueTileSource::new();
        let hand_tiles = source.tiles(&LOW_VALUE_HAND);
        let drawn_tile = source.tile("N");
        let actions: Vec<LegalAction> = hand_tiles
            .iter()
            .chain([&drawn_tile])
            .map(|&tile| LegalAction::Dahai { tile })
            .chain([LegalAction::Reach])
            .collect();
        let context = GameContext::from_parts_with_table_state(
            Some(drawn_tile),
            hand_tiles,
            vec![],
            None,
            None,
            Vec::new(),
            Some(0),
            Some(3),
            Default::default(),
            [false, true, false, false],
        )
        .with_history_furiten_facts(bot_logic::HistoryFuritenFacts {
            same_turn: Some(false),
            riichi_missed_win: Some(false),
        });

        let inputs = push_pull_inputs_from_context(&context, &actions);
        let offense = inputs.offense.expect("攻撃評価がある");
        let value = offense
            .tenpai_offense_value_after_discard
            .expect("攻撃打点を評価している");

        assert_eq!(value.value, OffenseValue::Unknown);
        assert_eq!(offense.tenpai_offense_weighted_total(), None);
        assert_eq!(
            offense.strong_tenpai_requirement(inputs.dealer_reacher),
            Some(StrongTenpaiRequirement::LiveWait(
                STRONG_TENPAI_MIN_REMAINING
            ))
        );

        // 待ちは4枚。打点が確定していれば押せる枚数だが、確定しないので降りる。
        assert_eq!(
            offense
                .tenpai_wait_after_discard
                .expect("テンパイの待ち facts がある")
                .tsumo_remaining,
            4
        );
        assert_decision(
            &inputs,
            PushPullMode::Fold,
            PushPullReason::WeakTenpaiAgainstReach,
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
        offense_state_from_normal_discard_with_actions(context, &[])
    }

    fn offense_state_from_normal_discard_with_actions(
        context: &GameContext,
        legal_actions: &[LegalAction],
    ) -> PushPullOffenseState {
        let tiles: Vec<TileId> = context
            .hand_tiles()
            .iter()
            .copied()
            .chain(context.drawn_tile())
            .collect();
        let evaluation = select_best_normal_discard_evaluation(context, &tiles).unwrap();
        push_pull_inputs_from_context_with_evaluation(context, Some(&evaluation), legal_actions)
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
        // 履歴依存フリテンが unknown な context なので、総合ロン可否も unknown のまま。
        assert_eq!(wait.can_ron, None);

        let inputs = inputs_with_dealer(1, false, false, Some(offense));
        let decision = decide_push_pull(&inputs);
        assert_eq!(decision.mode, PushPullMode::Push);
        assert_eq!(decision.reason, PushPullReason::StrongTenpaiAgainstReach);
    }

    #[test]
    fn own_fixed_meld_value_reaches_the_offense_state() {
        // 白ポン1組。副露の役牌翻が offense state の打点 proxy へ届き、押し引きの判断は変わらない。
        let with_meld = offense_state_from_normal_discard(&one_meld_context(vec![one_meld_pon()]));
        assert_eq!(with_meld.value_honor_han_proxy_after_discard, 1);
        assert_eq!(with_meld.dora_count_after_discard, 0);
        assert_eq!(with_meld.red_dora_count_after_discard, 0);
        assert_eq!(with_meld.simple_value_proxy_after_discard(), 1);

        let decision = decide_push_pull(&inputs_with_dealer(1, false, false, Some(with_meld)));
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

    // ---- 自分の fixed meld が簡易打点 proxy へ反映されること ----

    fn meld(kind: MeldKind, tiles: &[u8]) -> Meld {
        let tiles = ids(tiles);
        let called_tile = kind.is_open().then(|| tiles[0]);
        Meld::new(kind, tiles, called_tile)
    }

    fn melds_at(player: usize, melds: Vec<Meld>) -> [Vec<Meld>; 4] {
        let mut all: [Vec<Meld>; 4] = Default::default();
        all[player] = melds;
        all
    }

    // 打牌する 9s と、ドラ・役牌にならない filler だけの concealed hand。`extra` で打点要素を足す。
    fn plain_hand_with(extra: &[u8]) -> Vec<TileId> {
        let mut hand = ids(extra);
        hand.extend(ids(&[0, 4, 20, 24, 36, 40, 44, 56, 60, 64]));
        hand.push(tile(104));
        hand
    }

    // fixed meld を持つ proxy 用 context。fixed meld の物理牌は実局面と同じく visible tiles にも入れ、
    // そこから二重に数えないことを同時に固定する。
    fn proxy_meld_context(
        hand: Vec<TileId>,
        dora: Vec<TileId>,
        round_wind: Option<TileType>,
        seat_wind: Option<TileType>,
        player_id: Option<u8>,
        melds: [Vec<Meld>; 4],
    ) -> GameContext {
        let visible_tiles: Vec<TileId> = melds
            .iter()
            .flatten()
            .flat_map(|meld| meld.tiles().to_vec())
            .collect();

        GameContext::from_parts_with_melds(
            None,
            hand,
            dora,
            round_wind,
            seat_wind,
            visible_tiles,
            player_id,
            None,
            Default::default(),
            [false; 4],
            melds,
        )
    }

    #[test]
    fn proxy_without_fixed_melds_counts_the_concealed_hand_only() {
        // ドラ表示牌 3m、concealed に 4m 1枚・赤5p・白刻子。fixed meld が無ければ従来どおり。
        let hand = plain_hand_with(&[12, 52, 124, 125, 126]);
        let context = proxy_meld_context(hand, ids(&[8]), None, None, Some(0), Default::default());
        let evaluation = proxy_evaluation(nine_s(), false);

        let concealed = concealed_value_proxy_after_discard(&context, &evaluation);
        assert_eq!(concealed.dora_count, 2);
        assert_eq!(concealed.red_dora_count, 1);
        assert_eq!(concealed.value_honor_han_proxy, 1);
        assert_eq!(
            offense_value_proxy_after_discard(&context, &evaluation),
            concealed
        );
    }

    #[test]
    fn proxy_counts_indicator_dora_in_a_fixed_meld() {
        // ドラ表示牌 3m。concealed の 4m 1枚に、自分の 4m ポン3枚が加算される。
        let context = proxy_meld_context(
            plain_hand_with(&[15]),
            ids(&[8]),
            None,
            None,
            Some(0),
            melds_at(0, vec![meld(MeldKind::Pon, &[12, 13, 14])]),
        );
        let evaluation = proxy_evaluation(nine_s(), false);

        let concealed = concealed_value_proxy_after_discard(&context, &evaluation);
        let proxy = offense_value_proxy_after_discard(&context, &evaluation);
        assert_eq!(concealed.dora_count, 1);
        assert_eq!(proxy.dora_count, 4);
        assert_eq!(proxy.red_dora_count, 0);
        assert_eq!(proxy.value_honor_han_proxy, 0);
    }

    #[test]
    fn proxy_counts_red_dora_in_a_fixed_meld_without_double_counting() {
        // 赤5m を含むチー。ドラ総数へ1、赤ドラへ1で、簡易 proxy では赤ドラを再加算しない。
        let context = proxy_meld_context(
            plain_hand_with(&[]),
            vec![],
            None,
            None,
            Some(0),
            melds_at(0, vec![meld(MeldKind::Chi, &[12, 16, 21])]),
        );
        let evaluation = proxy_evaluation(nine_s(), false);
        let proxy = offense_value_proxy_after_discard(&context, &evaluation);

        assert_eq!(proxy.dora_count, 1);
        assert_eq!(proxy.red_dora_count, 1);
        assert_eq!(proxy.value_honor_han_proxy, 0);

        let offense =
            push_pull_inputs_from_context_with_evaluation(&context, Some(&evaluation), &[])
                .offense
                .expect("offense should be present");
        assert_eq!(offense.dora_count_after_discard, 1);
        assert_eq!(offense.red_dora_count_after_discard, 1);
        assert_eq!(offense.simple_value_proxy_after_discard(), 1);
    }

    #[test]
    fn proxy_counts_a_dragon_pon_as_one_han() {
        let context = proxy_meld_context(
            plain_hand_with(&[]),
            vec![],
            None,
            None,
            Some(0),
            melds_at(0, vec![meld(MeldKind::Pon, &[124, 125, 126])]),
        );
        let proxy = offense_value_proxy_after_discard(&context, &proxy_evaluation(nine_s(), false));

        assert_eq!(proxy.value_honor_han_proxy, 1);
        assert_eq!(proxy.dora_count, 0);
    }

    #[test]
    fn proxy_counts_a_double_wind_pon_as_two_han() {
        // 東場・東家の東ポン。自風は `oya` からではなく既知の `GameContext::seat_wind()` から取る。
        let context = proxy_meld_context(
            plain_hand_with(&[]),
            vec![],
            Some(wind(27)),
            Some(wind(27)),
            Some(0),
            melds_at(0, vec![meld(MeldKind::Pon, &[108, 109, 110])]),
        );
        let proxy = offense_value_proxy_after_discard(&context, &proxy_evaluation(nine_s(), false));

        assert_eq!(proxy.value_honor_han_proxy, 2);
        assert_eq!(
            player_threat_facts_from_context(&context)[0].seat_wind,
            None
        );
    }

    #[test]
    fn proxy_counts_every_tile_of_an_ankan_and_keeps_it_out_of_open_meld_facts() {
        // ドラ表示牌 3m の 4m 暗槓。物理牌4枚ぶん数え、相手向けの open meld facts は変えない。
        let context = proxy_meld_context(
            plain_hand_with(&[]),
            ids(&[8]),
            None,
            None,
            Some(0),
            melds_at(0, vec![meld(MeldKind::Ankan, &[12, 13, 14, 15])]),
        );
        let proxy = offense_value_proxy_after_discard(&context, &proxy_evaluation(nine_s(), false));
        assert_eq!(proxy.dora_count, 4);

        let facts = player_threat_facts_from_context(&context)[0];
        assert_eq!(facts.meld_dora_count, 4);
        assert_eq!(facts.open_meld_dora_count, 0);
        assert_eq!(facts.open_visible_han_proxy(), 0);
    }

    #[test]
    fn proxy_counts_an_ankan_value_honor_but_open_facts_do_not() {
        let context = proxy_meld_context(
            plain_hand_with(&[]),
            vec![],
            None,
            None,
            Some(0),
            melds_at(0, vec![meld(MeldKind::Ankan, &[124, 125, 126, 127])]),
        );
        let proxy = offense_value_proxy_after_discard(&context, &proxy_evaluation(nine_s(), false));
        assert_eq!(proxy.value_honor_han_proxy, 1);

        let facts = player_threat_facts_from_context(&context)[0];
        assert_eq!(facts.value_honor_melds.confirmed_han(), 1);
        assert_eq!(facts.open_value_honor_melds.confirmed_han(), 0);
    }

    #[test]
    fn proxy_adds_concealed_and_fixed_meld_contributions_once() {
        // concealed: 4m 1枚 + 赤5p + 白刻子。fixed meld: 4m ポン + 赤5m を含むチー + 東ポン。
        // 場風 E・自風 S なので東ポンは1翻。
        let context = proxy_meld_context(
            plain_hand_with(&[15, 52, 124, 125, 126]),
            ids(&[8]),
            Some(wind(27)),
            Some(wind(28)),
            Some(0),
            melds_at(
                0,
                vec![
                    meld(MeldKind::Pon, &[12, 13, 14]),
                    meld(MeldKind::Chi, &[16, 21, 25]),
                    meld(MeldKind::Pon, &[108, 109, 110]),
                ],
            ),
        );
        let evaluation = proxy_evaluation(nine_s(), false);

        let concealed = concealed_value_proxy_after_discard(&context, &evaluation);
        assert_eq!(concealed.dora_count, 2);
        assert_eq!(concealed.red_dora_count, 1);
        assert_eq!(concealed.value_honor_han_proxy, 1);

        let proxy = offense_value_proxy_after_discard(&context, &evaluation);
        assert_eq!(proxy.dora_count, 2 + 3 + 1);
        assert_eq!(proxy.red_dora_count, 1 + 1);
        assert_eq!(proxy.value_honor_han_proxy, 1 + 1);
    }

    #[test]
    fn proxy_does_not_guess_an_unknown_wind_of_a_fixed_meld() {
        // 東ポン。場風・自風とも不明なら数えず、場風だけ確定していればその1翻だけ数える。
        let melds = || melds_at(0, vec![meld(MeldKind::Pon, &[108, 109, 110])]);
        let evaluation = proxy_evaluation(nine_s(), false);

        let unknown =
            proxy_meld_context(plain_hand_with(&[]), vec![], None, None, Some(0), melds());
        assert_eq!(
            offense_value_proxy_after_discard(&unknown, &evaluation).value_honor_han_proxy,
            0
        );

        let round_wind_only = proxy_meld_context(
            plain_hand_with(&[]),
            vec![],
            Some(wind(27)),
            None,
            Some(0),
            melds(),
        );
        assert_eq!(
            offense_value_proxy_after_discard(&round_wind_only, &evaluation).value_honor_han_proxy,
            1
        );
    }

    #[test]
    fn proxy_ignores_melds_when_the_own_seat_is_unknown() {
        // `player_id` が不明なら player 0 を自分と仮定せず、確認できない fixed meld を数えない。
        let evaluation = proxy_evaluation(nine_s(), false);
        let unknown_seat = proxy_meld_context(
            plain_hand_with(&[]),
            ids(&[8]),
            None,
            None,
            None,
            melds_at(
                0,
                vec![
                    meld(MeldKind::Pon, &[12, 13, 14]),
                    meld(MeldKind::Pon, &[124, 125, 126]),
                ],
            ),
        );
        let proxy = offense_value_proxy_after_discard(&unknown_seat, &evaluation);
        assert_eq!(proxy.dora_count, 0);
        assert_eq!(proxy.value_honor_han_proxy, 0);

        // 自分の席が分かっていても、他家の fixed meld は自分の打点にならない。
        let opponent_meld = proxy_meld_context(
            plain_hand_with(&[]),
            ids(&[8]),
            None,
            None,
            Some(0),
            melds_at(
                1,
                vec![
                    meld(MeldKind::Pon, &[12, 13, 14]),
                    meld(MeldKind::Pon, &[124, 125, 126]),
                ],
            ),
        );
        let proxy = offense_value_proxy_after_discard(&opponent_meld, &evaluation);
        assert_eq!(proxy.dora_count, 0);
        assert_eq!(proxy.value_honor_han_proxy, 0);
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

        let inputs =
            push_pull_inputs_from_context_with_evaluation(&context, Some(&evaluation), &[]);
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

        let public = push_pull_inputs_from_context(&context, &[]);
        let shared =
            push_pull_inputs_from_context_with_evaluation(&context, evaluation.as_ref(), &[]);
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
        let _ = push_pull_inputs_from_context_with_evaluation(&context, Some(&evaluation), &[]);

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
        let inputs = push_pull_inputs_from_context(&context, &[]);

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
            push_pull_inputs_from_threat_facts(&context, facts, None, None, &[]),
            push_pull_inputs_from_context_with_evaluation(&context, None, &[])
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
                    let inputs = push_pull_inputs_from_context(&context, &[]);
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

    // player 1 が1副露かつ河12枚で High になる facts。
    fn late_one_meld_high_facts() -> [PlayerThreatFacts; 4] {
        let mut facts = open_meld_facts_of(1, 1, [false; 4], Some(0));
        facts[1].discard_count = 12;
        facts
    }

    fn late_one_meld_high_inputs(offense: Option<PushPullOffenseState>) -> PushPullInputs {
        inputs_with_threats(0, false, false, offense, late_one_meld_high_facts())
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
    fn tenpai_against_only_a_late_one_meld_high_open_hand_pushes() {
        let weak = late_one_meld_high_inputs(Some(tenpai_offense(3, PermanentFuriten::No)));
        assert!(weak.has_only_late_one_meld_high_open_hand_threats());
        assert_high_open_hand_decision(
            &weak,
            PushPullMode::Push,
            PushPullReason::TenpaiAgainstLateOneMeldHighOpenHand,
        );

        // strong tenpai は専用例外ではなく既存 reason のまま押す。
        assert_high_open_hand_decision(
            &late_one_meld_high_inputs(Some(strong_tenpai_offense())),
            PushPullMode::Push,
            PushPullReason::StrongTenpaiAgainstHighOpenHand,
        );
    }

    #[test]
    fn the_late_one_meld_exception_does_not_depend_on_the_offense_value() {
        // 打点を確定しても確定しなくても、終盤1副露 High だけならテンパイで押す例外は変わらない。
        for offense in [
            weighted_total_tenpai_offense(TENPAI_PUSH_WEIGHTED_TOTAL_MIN - 1),
            unknown_value_tenpai_offense(2, PermanentFuriten::No),
            tenpai_offense(2, PermanentFuriten::Unknown),
        ] {
            let inputs = late_one_meld_high_inputs(Some(offense));
            assert!(!is_strong_tenpai(&offense, false));
            assert_high_open_hand_decision(
                &inputs,
                PushPullMode::Push,
                PushPullReason::TenpaiAgainstLateOneMeldHighOpenHand,
            );
        }
    }

    #[test]
    fn non_tenpai_against_a_late_one_meld_high_open_hand_still_folds() {
        assert_high_open_hand_decision(
            &late_one_meld_high_inputs(Some(offense(1, 8, 3))),
            PushPullMode::Fold,
            PushPullReason::IishantenAgainstHighOpenHand,
        );
        assert_high_open_hand_decision(
            &late_one_meld_high_inputs(Some(offense(2, 20, 4))),
            PushPullMode::Fold,
            PushPullReason::TwoOrMoreShantenAgainstHighOpenHand,
        );
    }

    #[test]
    fn weak_tenpai_against_a_two_meld_high_open_hand_still_folds() {
        let mut facts = open_meld_facts_of(1, 2, [false; 4], Some(0));
        facts[1].discard_count = 9;
        let inputs = inputs_with_threats(0, false, false, Some(weak_tenpai_offense()), facts);

        assert!(!inputs.has_only_late_one_meld_high_open_hand_threats());
        assert_high_open_hand_decision(
            &inputs,
            PushPullMode::Fold,
            PushPullReason::WeakTenpaiAgainstHighOpenHand,
        );
    }

    #[test]
    fn every_high_target_must_be_a_late_one_meld_open_hand() {
        let mut all_late = late_one_meld_high_facts();
        let mut second_late = open_meld_facts_of(2, 1, [false; 4], Some(0));
        second_late[2].discard_count = 13;
        all_late[2] = second_late[2];
        let all_late_inputs =
            inputs_with_threats(0, false, false, Some(weak_tenpai_offense()), all_late);
        assert!(all_late_inputs.has_only_late_one_meld_high_open_hand_threats());
        assert_high_open_hand_decision(
            &all_late_inputs,
            PushPullMode::Push,
            PushPullReason::TenpaiAgainstLateOneMeldHighOpenHand,
        );

        let mut mixed = late_one_meld_high_facts();
        let mut two_meld = open_meld_facts_of(2, 2, [false; 4], Some(0));
        two_meld[2].discard_count = 9;
        mixed[2] = two_meld[2];
        let mixed_inputs = inputs_with_threats(0, false, false, Some(weak_tenpai_offense()), mixed);
        assert!(!mixed_inputs.has_only_late_one_meld_high_open_hand_threats());
        assert_high_open_hand_decision(
            &mixed_inputs,
            PushPullMode::Fold,
            PushPullReason::WeakTenpaiAgainstHighOpenHand,
        );
    }

    #[test]
    fn a_reach_keeps_the_strong_tenpai_threshold_with_a_late_one_meld_high() {
        let mut facts = late_one_meld_high_facts();
        facts[2].reached = true;
        let inputs = inputs_with_threats(1, false, false, Some(weak_tenpai_offense()), facts);

        assert!(inputs.has_combined_threat());
        assert!(inputs.has_only_late_one_meld_high_open_hand_threats());
        assert_decision(
            &inputs,
            PushPullMode::Fold,
            PushPullReason::WeakTenpaiAgainstCombinedThreat,
        );
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
    fn a_high_open_hand_threat_alone_uses_the_base_weighted_total_threshold() {
        // High OpenHandThreat 単独はリーチ者がいないので、親リーチの 1.5 倍は適用しない。
        let exact = weighted_total_tenpai_offense(TENPAI_PUSH_WEIGHTED_TOTAL_MIN);
        let below = weighted_total_tenpai_offense(TENPAI_PUSH_WEIGHTED_TOTAL_MIN - 1);

        let inputs = high_open_hand_inputs(Some(exact));
        assert_eq!(inputs.opponent_reach_count, 0);
        assert!(!inputs.dealer_reacher);
        assert_high_open_hand_decision(
            &inputs,
            PushPullMode::Push,
            PushPullReason::StrongTenpaiAgainstHighOpenHand,
        );
        assert_high_open_hand_decision(
            &high_open_hand_inputs(Some(below)),
            PushPullMode::Fold,
            PushPullReason::WeakTenpaiAgainstHighOpenHand,
        );
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
        let inputs = push_pull_inputs_from_context(&context, &[]);

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
