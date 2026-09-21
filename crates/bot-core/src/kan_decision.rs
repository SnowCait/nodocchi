//! カン判断 policy 層。Chi / Pon の鳴き判断 ([`crate::call_decision`]) とは別の責務として持つ。
//!
//! Chi / Pon は他家打牌への reaction で「鳴く → 直後に打牌する」を1つの評価単位にできるが、
//! カンはそうならない。
//!
//! | 種別 | 契機 | 直後の手番 |
//! | --- | --- | --- |
//! | Ankan | 自分のツモ番 | 嶺上牌を引いてから打牌 |
//! | Kakan | 自分のツモ番 | 嶺上牌を引いてから打牌 (搶槓あり) |
//! | Daiminkan | 他家打牌への reaction | 嶺上牌を引いてから打牌 |
//!
//! どの種別も「カン → 未知の嶺上牌 → 打牌」なので、Chi / Pon の
//! `Call → post-call discard` 評価モデルをそのまま当てはめられない。そのため鳴き判断へ混ぜず、
//! この層を分ける。
//!
//! # 今回 production へ接続する範囲
//!
//! production で選べるのは [`KanKind::Ankan`] だけである。[`KanKind::Kakan`] と
//! [`KanKind::Daiminkan`] は候補として診断には並ぶが、理由
//! ([`KanDecisionReason::KakanNotConnected`] / [`KanDecisionReason::DaiminkanNotConnected`])
//! を付けて必ず選ばない。
//!
//! さらに暗槓も、**暗槓前後の打点を既存評価で比較できた局面だけ**を production 対象にする。
//! 速度 (向聴・受け入れ) が悪化しないことだけを根拠に暗槓することはない。
//!
//! # source of truth
//!
//! | 材料 | source of truth |
//! | --- | --- |
//! | カンの合法性 | 入力の `legal_actions` (`possible_actions` 由来) |
//! | 面子の形の検証 | [`Meld::shape`] |
//! | 副露済み面子数 | [`GameContext::own_fixed_meld_count`] / [`FixedMeldCount`] |
//! | カン後の向聴数 | [`calculate_shanten_with_fixed_melds`] |
//! | カン後の受け入れ | [`calculate_acceptance_with_fixed_melds`] / [`calculate_acceptance_with_fixed_melds_and_visible_tiles`] |
//! | カン後テンパイの待ちとロン可否 | [`tenpai_wait_availability`] |
//! | カン後テンパイの完成手 | [`tenpai_completed_hands`] |
//! | テンパイの攻撃打点と攻撃モード | [`evaluate_tenpai_offense_value`] / [`evaluate_tenpai_offense_with_hands`] |
//! | 暗槓しない場合の打牌 | production の通常打牌選択が選んだ [`DiscardEvaluation`] |
//! | 押し引き | [`decide_push_pull`](crate::push_pull::decide_push_pull) の結論 |
//!
//! 向聴・受け入れ・待ち・フリテン・役・点数をこの層で計算し直さない。打点は押し引きとリーチ
//! 判断が使うのと同じ [`TenpaiOffenseValue`] をそのまま読む。
//!
//! リーチ後に暗槓できるか (待ちが変わらないか) も含めて、合法性の判定はこの層に持たない。
//! `legal_actions` に [`LegalAction::Ankan`] が並んでいることだけを合法の根拠にする。
//!
//! # 判断する位置
//!
//! `ShantenAgent` は
//!
//! ```text
//! Hora → 九種九牌 → Chi / Pon → 押し引き → Reach → Kan → 通常打牌 → 防御 fallback
//! ```
//!
//! の順で action を決める。カンは Push mode でリーチを採用しなかった後にだけ検討するので、
//! 既存の Hora / 九種九牌 / Reach の優先順位は変わらない。押し引きの gate 自体はこの層が持ち、
//! 呼び出し側は結論 ([`PushPullMode`]) をそのまま渡す。
//!
//! # 比較する2つの state
//!
//! 暗槓する場合としない場合を、どちらも「13枚相当で次のツモを待つ state」に揃えて比べる。
//!
//! ```text
//! 暗槓しない: 14枚 → 通常打牌 → 13枚 (副露 N)        → 次のツモを待つ
//! 暗槓する  : 14枚 → 暗槓     → 10枚 (副露 N+1)      → 嶺上牌を待つ
//! ```
//!
//! `10枚 + 副露 N+1` と `13枚 + 副露 N` はどちらも `13 - 3 * 副露数` 枚の同じ大きさの手牌なので、
//! 既存の向聴・受け入れ・打点をそのまま同じ尺度で比べられる。比較の基準に使う打牌は production
//! の通常打牌選択が実際に選んだ1件そのもので、この層で打牌を選び直さない。
//!
//! # 暗槓の成立条件
//!
//! ```text
//! 合法な Ankan がある
//! AND 他家にリーチ者がいない
//! AND 既存 Push/Pull policy が Push と判定している
//! AND 自分のツモを経たと確認できる (drawn_tile がある)
//! AND 自分の副露済み面子数が分かり、暗槓後も上限内
//! AND consumed 4枚を手牌 + ツモ牌から取り除いてカンの形になる
//! AND 暗槓しない場合に選ぶ通常打牌の評価がある
//! AND 暗槓後の向聴数 == その通常打牌後の向聴数
//! AND 暗槓後の受け入れ残枚数 >= その通常打牌後の受け入れ残枚数
//! AND 暗槓後の受け入れ牌種数 >= その通常打牌後の受け入れ牌種数
//! AND 両側の攻撃打点を既存評価で確定でき、攻撃モードも一致する
//! AND 暗槓後の攻撃打点 >= その通常打牌後の攻撃打点
//! AND 成立した暗槓候補がちょうど1件
//! ```
//!
//! ## 向聴の比較
//!
//! [`Acceptance`](bot_logic::Acceptance) は「その牌を1枚加えると**現在の向聴数**が下がる牌」な
//! ので、向聴段階が違う state の受け入れ枚数・牌種数は同じ意味の値ではない。1向聴の受け入れ8枚
//! とテンパイの待ち4枚を `4 < 8` として比べない。したがって向聴の比較は3通りに分ける。
//!
//! | 暗槓後 vs 通常打牌後 | 扱い |
//! | --- | --- |
//! | 悪化 | [`KanDecisionReason::ShantenRegresses`] |
//! | 改善 | 受け入れも打点も同じ尺度で比べられないので [`KanDecisionReason::ShantenImprovedNotComparable`] |
//! | 同じ | 受け入れと打点の比較へ進む |
//!
//! 向聴が改善するのは、暗槓しない側の合法打牌が制限されていて4枚目を切れない局面などに限られる。
//! 改善そのものを暗槓の根拠にはしない。向聴が進んだ分の価値を既存 primitive で確定できないため、
//! 「向聴が改善したから暗槓する」という結論もここでは作らない。
//!
//! ## 打点の比較
//!
//! 速度が悪化しないことだけでは、暗槓によって役・待ち構成・確定打点が落ちる局面を弾けない。
//! そのため速度の比較を通った候補には、押し引き・リーチ判断が使うのと同じ攻撃打点
//! ([`TenpaiOffenseValue`]) の比較を必ず要求する。
//!
//! | 側 | 手牌 | 打点 |
//! | --- | --- | --- |
//! | 暗槓しない | 通常打牌後13枚 + 既存副露 | [`evaluate_tenpai_offense_value`] |
//! | 暗槓する | 暗槓後10枚 + 既存副露 + 今回の暗槓 | [`evaluate_tenpai_offense_with_hands`] |
//!
//! どちらも同じ hypothetical baseline (リーチ手なら [`current_reach_baseline_context`]、ダマ手
//! なら [`damaten_baseline_context`]) と同じ既知のドラ表示牌で評価し、生きた和了牌 variant の
//! 残枚数で加重した合計 ([`OffenseValue::weighted_total`](crate::offense_value::OffenseValue::weighted_total)) を比べる。押し引きが threshold 判定
//! に使うのと同じ値で、カン専用の打点評価も集約規則も持たない。
//!
//! 攻撃モード (リーチ手かダマ手か) が違う2つの値は同じ尺度ではないので、モードが一致しない
//! 場合は比較しない。
//!
//! ## 評価不能として暗槓しない局面
//!
//! 次のどれかに当たる候補は [`KanDecisionReason::ValueNotEvaluable`] にして暗槓せず、通常打牌を
//! そのまま維持する。速度非劣化だけを根拠に暗槓へ倒すことはしない。
//!
//! - どちらかの side がテンパイでない (テンパイ以外の打点を既存 primitive で比較できない)
//! - どちらかの side の攻撃モードが [`TenpaiOffenseMode::Unknown`]、またはモードが食い違う
//! - どちらかの side の攻撃打点が [`OffenseValue::Unknown`](crate::offense_value::OffenseValue::Unknown) (役なし・ロン不可・点数計算の
//!   入力不足・裏ドラ未確定)
//!
//! テンパイ以外を対象外にするのは、1向聴以降の価値尺度が
//! [`ExpectedSelfTsumoValue`](bot_logic::ForwardMetrics) 系になるためである。暗槓後の state は
//! 嶺上牌ぶん1回多くツモれて、しかも残り自摸機会 ([`own_future_draws`]) の元になる山の残枚数も
//! 変わるので、暗槓しない側と同じ horizon の値にならない。差を埋める補正を推測で置かない限り
//! 比較にならないため、今回は接続しない。テンパイの攻撃打点はロン和了1回分の確定打点で、
//! 残り自摸機会に依存しないので、この非対称性を持たない。
//!
//! ## 今回評価に含めないもの
//!
//! 暗槓には既存評価だけでは値を確定できない要素があり、係数や推定値を置かずに「評価に含め
//! ない」ままにする。含めていないものは次のとおりで、いずれも TODO として残す。
//!
//! - **新ドラ**: カンで増えるドラ表示牌の中身は未知なので、自分の打点にも他家の打点にも
//!   加算しない。他家リーチ中に暗槓しない ([`KanDecisionReason::OpponentReached`]) のは、
//!   この未知のドラがリーチ者の打点をどれだけ押し上げるかを既存評価で測れないためである。
//!   暗槓側の打点を過小評価する方向なので、比較は暗槓に不利な側へ倒れる。
//! - **嶺上牌**: 引く牌は未知なので、特定の牌を引いた後の state として評価しない。暗槓が1回
//!   分多くツモれることも評価へ足さない。
//!
//! 暗刻が暗槓になることで増える符は、既存 scoring が暗槓を含む固定面子から求めた値がそのまま
//! 打点比較へ入る。この層で符を数え直さない。
//!
//! # 複数の暗槓候補
//!
//! 同じ局面で2件以上の暗槓が成立した場合、production では**どれも選ばない**
//! ([`KanDecisionReason::MultipleEligibleCandidates`])。合法 action の列挙順は server が決める
//! ものなので、AI の tie-break に使わない。候補間を妥当に比較できる既存 comparator がまだ無い
//! ので、将来のためだけの独自 ranking も作らない。候補ごとの判断内訳は診断へそのまま残す。
//!
//! # 判断にかかるコスト
//!
//! 安価な事前判定 (種別・他家リーチ・押し引き・副露数・向聴・受け入れ) を通った候補だけが打点
//! 比較へ進む。打点比較は押し引きが threat ありのテンパイで払うのと同じ1回分の evaluation を
//! 両 side に対して行うもので、前方探索は通らない。合法なカンが1件も無い局面では候補の列挙で
//! 終わり、他家リーチ中と Push 以外の押し引きでは手牌を組み立てる前に落ちるので、通常の打牌
//! 局面へ載るコストは無い。
//!
//! TODO: 暗槓前後を ExpectedSelfTsumoValue で比較できるようにして、テンパイ以外の暗槓も
//! production へ接続する。嶺上牌ぶんの追加ツモと山の残枚数の差をどう揃えるかが未解決。
//!
//! TODO: 複数の暗槓候補を既存評価で比較できるようにする。
//!
//! TODO: Kakan の搶槓リスクと Daiminkan の reaction モデルを評価できるようにしてから、
//! [`KanKind`] の残り2種別を production へ接続する。

use std::cmp::Ordering;

use bot_logic::{
    DiscardEvaluation, EffectiveAcceptance, FixedMeldCount, Meld, MeldKind, OwnDiscards,
    TileCounts, TileId, TileType, calculate_acceptance_with_fixed_melds,
    calculate_acceptance_with_fixed_melds_and_visible_tiles, calculate_shanten_with_fixed_melds,
    structural_acceptance_tile_types_with_fixed_melds, tenpai_completed_hands,
    tenpai_wait_availability,
};

use crate::action::LegalAction;
use crate::context::GameContext;
use crate::discard_selection::selected_discard_tenpai_wait_availability;
use crate::offense_value::{
    TenpaiOffenseMode, TenpaiOffenseValue, evaluate_tenpai_offense_value,
    evaluate_tenpai_offense_with_hands,
};
use crate::push_pull::PushPullMode;

/// 暗槓が消費する物理牌の枚数。
const ANKAN_CONSUMED_TILE_COUNT: usize = 4;

/// 評価対象のカン種別。
///
/// 今回 production で選べるのは [`Self::Ankan`] だけで、残り2種別は候補として診断に並ぶだけ。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KanKind {
    Ankan,
    Kakan,
    Daiminkan,
}

impl KanKind {
    /// 対応する既存の副露種別。
    pub fn meld_kind(self) -> MeldKind {
        match self {
            Self::Ankan => MeldKind::Ankan,
            Self::Kakan => MeldKind::Kakan,
            Self::Daiminkan => MeldKind::Daiminkan,
        }
    }

    /// 今回 production で選択できる種別か。
    pub fn is_production_connected(self) -> bool {
        matches!(self, Self::Ankan)
    }

    // 今回 production 接続していない種別の理由。
    fn not_connected_reason(self) -> Option<KanDecisionReason> {
        match self {
            Self::Ankan => None,
            Self::Kakan => Some(KanDecisionReason::KakanNotConnected),
            Self::Daiminkan => Some(KanDecisionReason::DaiminkanNotConnected),
        }
    }
}

/// カンを採用した / しなかった理由。
///
/// [`Self::EligibleAnkanNoRegression`] 以外はすべて「今回はカンしない」理由であり、最初に落ちた
/// 条件を1つだけ表す。判定順は [`KanCandidateDiagnostic`] のフィールドが埋まる順と一致する。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KanDecisionReason {
    /// 全条件を満たし、既存評価で測れる向聴・受け入れ・攻撃打点のどれも暗槓で悪化しない。
    EligibleAnkanNoRegression,
    /// 加槓はまだ production へ接続していない。搶槓リスクを評価できていない。
    KakanNotConnected,
    /// 大明槓はまだ production へ接続していない。reaction としての評価モデルが無い。
    DaiminkanNotConnected,
    /// 他家にリーチ者がいる。新ドラがリーチ者の打点へ与える影響を既存評価で測れない。
    OpponentReached,
    /// 既存 Push/Pull policy が Push と判定していない。
    NotPush,
    /// 自分のツモを経たと確認できない。暗槓はツモ番の action なので、この局面では判断しない。
    NotAfterOwnDraw,
    /// 自分の副露済み面子数が不明。0副露と推測しない。
    FixedMeldCountUnknown,
    /// 暗槓後の副露済み面子数が上限を超える。
    FixedMeldCountOverflow,
    /// consumed が4枚でない・手牌に無い・カンの形にならない。
    InvalidConsumed,
    /// 比較の基準になる通常打牌評価が無く、暗槓しない場合と比べられない。
    NormalDiscardUnavailable,
    /// 暗槓後の向聴数が、暗槓しない場合の通常打牌後より悪くなる。
    ShantenRegresses,
    /// 暗槓後の向聴数が通常打牌後より進む。
    ///
    /// 受け入れも打点も向聴段階が違えば同じ意味の値ではないので、枚数の単純比較で結論しない。
    /// 向聴が進んだ分の価値も既存 primitive では確定できないため、改善そのものを暗槓の根拠にも
    /// しない。
    ShantenImprovedNotComparable,
    /// 向聴数は同じだが、暗槓後の受け入れが通常打牌後より減る。
    AcceptanceRegresses,
    /// 暗槓前後の攻撃打点を既存評価で比較できない。
    ///
    /// どちらかがテンパイでない、攻撃モードが [`TenpaiOffenseMode::Unknown`] または食い違う、
    /// どちらかの攻撃打点が [`OffenseValue::Unknown`](crate::offense_value::OffenseValue::Unknown) のいずれか。速度が悪化しないことだけを
    /// 根拠に暗槓せず、通常打牌を維持する。
    ValueNotEvaluable,
    /// 暗槓後の攻撃打点が、暗槓しない場合の通常打牌後より下がる。
    ValueRegresses,
    /// 成立した暗槓候補が2件以上ある。
    ///
    /// 合法 action の列挙順を tie-break にしないため、候補間を比較できる既存評価が無いうちは
    /// どれも選ばない。候補ごとの判断内訳は診断へそのまま残す。
    MultipleEligibleCandidates,
}

impl KanDecisionReason {
    /// カンを採用した理由か。
    pub fn is_eligible(self) -> bool {
        matches!(self, Self::EligibleAnkanNoRegression)
    }
}

/// 比較に使った13枚相当 state 1つ分の既存評価。
///
/// 値はすべて既存 layer が求めたもので、診断のために向聴も受け入れも打点も計算し直さない。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KanHandDiagnostic {
    /// 副露済み面子数を含めた effective shanten。
    pub shanten: i8,
    /// 受け入れ残枚数 [枚]。`shanten` が違う state 同士では同じ意味の値にならない。
    pub acceptance_remaining: u8,
    /// 受け入れ牌種数。`acceptance_remaining` と同じく向聴段階に依存する。
    pub acceptance_type_count: usize,
    /// テンパイの場合の攻撃モードと確定打点。押し引き・リーチ判断が使うのと同じ値。
    ///
    /// テンパイでない state と、待ちを組み立てられない state では `None`。
    pub offense: Option<TenpaiOffenseValue>,
}

impl KanHandDiagnostic {
    /// 生きた待ちの支払い合計の残枚数加重合計 [点]。確定しない場合は `None`。
    pub fn weighted_total(&self) -> Option<u64> {
        self.offense?.value.weighted_total()
    }

    /// 攻撃モード。テンパイでない場合は `None`。
    pub fn offense_mode(&self) -> Option<TenpaiOffenseMode> {
        Some(self.offense?.mode)
    }
}

/// 合法な `LegalAction::Ankan` / `LegalAction::Kakan` / `LegalAction::Daiminkan` 1件ごとの判断内訳。
///
/// 各フィールドは判定が実際にそこまで進んだ場合だけ `Some` になり、進まなかった判定は推測せず
/// `None` のままにする。評価不能で落ちた候補は `reason` がその理由を表す。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KanCandidateDiagnostic {
    pub action: LegalAction,
    pub kind: KanKind,
    /// カンの対象牌種。Ankan は consumed の牌種、Kakan / Daiminkan は対象の打牌 / 手出し牌。
    pub tile: Option<TileType>,
    pub current_fixed_meld_count: Option<FixedMeldCount>,
    pub post_kan_fixed_meld_count: Option<FixedMeldCount>,
    /// カンしない場合に採用する通常打牌と、その打牌後13枚の既存評価。
    ///
    /// production の通常打牌選択が選んだ1件そのもので、この層で選び直さない。
    pub baseline_discard: Option<TileType>,
    pub baseline: Option<KanHandDiagnostic>,
    /// 暗槓後の13枚相当 state の既存評価。
    pub post_kan: Option<KanHandDiagnostic>,
    pub eligible: bool,
    pub selected: bool,
    pub reason: KanDecisionReason,
}

impl KanCandidateDiagnostic {
    /// 暗槓後 - 暗槓しない場合の向聴数差。負なら暗槓後の方が良い。両方を評価した場合だけ `Some`。
    pub fn shanten_delta(&self) -> Option<i8> {
        Some(self.post_kan?.shanten - self.baseline?.shanten)
    }

    /// 暗槓後 - 暗槓しない場合の受け入れ残枚数差 [枚]。符号付き。
    ///
    /// 向聴段階が違う state 同士では同じ意味の値にならないので、production の判断は
    /// `shanten_delta() == Some(0)` の場合しかこの差を読まない。
    pub fn acceptance_remaining_delta(&self) -> Option<i16> {
        Some(
            i16::from(self.post_kan?.acceptance_remaining)
                - i16::from(self.baseline?.acceptance_remaining),
        )
    }

    /// 暗槓後 - 暗槓しない場合の受け入れ牌種数差。符号付き。
    ///
    /// 読み方の制約は [`Self::acceptance_remaining_delta`] と同じ。
    pub fn acceptance_type_delta(&self) -> Option<isize> {
        Some(
            self.post_kan?.acceptance_type_count as isize
                - self.baseline?.acceptance_type_count as isize,
        )
    }

    /// 暗槓後 - 暗槓しない場合の攻撃打点差 [点]。符号付き。
    ///
    /// 両側の打点を確定できた場合だけ `Some`。攻撃モードが食い違う組み合わせでも値は返るが、
    /// production の判断はモードが一致する場合しか読まない。
    pub fn weighted_total_delta(&self) -> Option<i128> {
        Some(
            i128::from(self.post_kan?.weighted_total()?)
                - i128::from(self.baseline?.weighted_total()?),
        )
    }
}

/// カン判断の構造化診断。
///
/// `selected` は `ShantenAgent::act()` が実際に採用したカンそのもので、診断用の別判断ロジック
/// は持たない。採用が無い場合の `reason` は最初の候補が落ちた理由。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KanDecisionDiagnostic {
    pub selected: Option<LegalAction>,
    pub reason: KanDecisionReason,
    pub candidates: Vec<KanCandidateDiagnostic>,
}

// カン判断の本体。act() と構造化診断はこの1本を共有し、診断は結果を載せるだけにする。
//
// 合法なカンが1件も無ければ検討自体を行わず None。1件以上ある場合は候補ごとに独立して条件を
// 評価し、成立した候補がちょうど1件の場合だけそれを選ぶ。2件以上成立した場合は合法 action の
// 列挙順を tie-break にせず、どれも選ばずに理由だけを残す。
//
// `normal_discard` は暗槓しない場合に採用する通常打牌の評価。production の通常打牌選択が選んだ
// ものをそのまま受け取り、この層で打牌を選び直さない。
pub(crate) fn evaluate_kan_decision(
    ctx: &GameContext,
    legal_actions: &[LegalAction],
    mode: PushPullMode,
    normal_discard: Option<&DiscardEvaluation>,
) -> Option<KanDecisionDiagnostic> {
    let mut candidates: Vec<KanCandidateDiagnostic> = Vec::new();
    for action in legal_actions {
        let Some((kind, called_tile, consumed)) = normalize_kan(action) else {
            continue;
        };
        let mut candidate = new_kan_candidate(action, kind, called_tile, consumed);
        let reason = evaluate_kan_candidate(
            ctx,
            legal_actions,
            kind,
            consumed,
            mode,
            normal_discard,
            &mut candidate,
        );
        candidate.eligible = reason.is_eligible();
        candidate.reason = reason;
        candidates.push(candidate);
    }

    if candidates.is_empty() {
        return None;
    }

    let eligible: Vec<usize> = candidates
        .iter()
        .enumerate()
        .filter(|(_, candidate)| candidate.eligible)
        .map(|(index, _)| index)
        .collect();

    // 採用が無い場合の理由は最初の候補が落ちた理由。ちょうど1件成立した場合だけ採用し、2件
    // 以上成立した場合は候補固有の理由ではなく複数成立そのものを理由にする。
    let (selected_index, reason) = match eligible.as_slice() {
        [] => (None, candidates[0].reason),
        [index] => (Some(*index), candidates[*index].reason),
        _ => (None, KanDecisionReason::MultipleEligibleCandidates),
    };

    if let Some(index) = selected_index {
        candidates[index].selected = true;
    }
    let selected = selected_index.map(|index| candidates[index].action.clone());

    Some(KanDecisionDiagnostic {
        selected,
        reason,
        candidates,
    })
}

// 合法 action をカンの共通表現へ正規化する。それ以外の action は対象外。
fn normalize_kan(action: &LegalAction) -> Option<(KanKind, Option<TileId>, &[TileId])> {
    match action {
        LegalAction::Ankan { consumed } => Some((KanKind::Ankan, None, consumed)),
        LegalAction::Kakan { tile, consumed } => Some((KanKind::Kakan, Some(*tile), consumed)),
        LegalAction::Daiminkan { tile, consumed } => {
            Some((KanKind::Daiminkan, Some(*tile), consumed))
        }
        _ => None,
    }
}

fn new_kan_candidate(
    action: &LegalAction,
    kind: KanKind,
    called_tile: Option<TileId>,
    consumed: &[TileId],
) -> KanCandidateDiagnostic {
    KanCandidateDiagnostic {
        action: action.clone(),
        kind,
        tile: called_tile
            .or_else(|| consumed.first().copied())
            .map(TileId::tile_type),
        current_fixed_meld_count: None,
        post_kan_fixed_meld_count: None,
        baseline_discard: None,
        baseline: None,
        post_kan: None,
        eligible: false,
        selected: false,
        reason: KanDecisionReason::EligibleAnkanNoRegression,
    }
}

// 候補1件の条件を上から順に評価し、最初に落ちた理由を返す。評価が進んだ範囲の値だけを
// candidate へ書き込み、評価しなかった項目は None のままにする。
//
// 判定順は安価な fact (種別・他家リーチ・押し引き・副露数・カンの形) から始め、向聴と受け入れ
// を通った候補だけが打点比較へ進む。
#[allow(clippy::too_many_arguments)]
fn evaluate_kan_candidate(
    ctx: &GameContext,
    legal_actions: &[LegalAction],
    kind: KanKind,
    consumed: &[TileId],
    mode: PushPullMode,
    normal_discard: Option<&DiscardEvaluation>,
    candidate: &mut KanCandidateDiagnostic,
) -> KanDecisionReason {
    if let Some(reason) = kind.not_connected_reason() {
        return reason;
    }

    if ctx.any_opponent_reached() {
        return KanDecisionReason::OpponentReached;
    }

    if mode != PushPullMode::Push {
        return KanDecisionReason::NotPush;
    }

    if !ctx.is_after_own_draw() {
        return KanDecisionReason::NotAfterOwnDraw;
    }

    let Some(current_fixed_meld_count) = ctx.own_fixed_meld_count() else {
        return KanDecisionReason::FixedMeldCountUnknown;
    };
    candidate.current_fixed_meld_count = Some(current_fixed_meld_count);

    let Some(post_kan_fixed_meld_count) = FixedMeldCount::new(current_fixed_meld_count.get() + 1)
    else {
        return KanDecisionReason::FixedMeldCountOverflow;
    };
    candidate.post_kan_fixed_meld_count = Some(post_kan_fixed_meld_count);

    let Some((meld, post_kan_tiles)) = ankan_meld_and_concealed_tiles(ctx, consumed) else {
        return KanDecisionReason::InvalidConsumed;
    };

    let Some(normal_discard) = normal_discard else {
        return KanDecisionReason::NormalDiscardUnavailable;
    };
    candidate.baseline_discard = Some(normal_discard.discard);
    let baseline = evaluate_baseline_hand(ctx, legal_actions, normal_discard);
    candidate.baseline = Some(baseline);

    let mut post_kan_melds = ctx.own_melds().unwrap_or_default().to_vec();
    post_kan_melds.push(meld);
    let post_kan = evaluate_post_kan_hand(
        ctx,
        legal_actions,
        &post_kan_tiles,
        &post_kan_melds,
        post_kan_fixed_meld_count,
    );
    candidate.post_kan = Some(post_kan);

    // 受け入れも打点も「現在の向聴数」に紐づく値なので、向聴段階が同じ場合だけ比較する。
    match post_kan.shanten.cmp(&baseline.shanten) {
        Ordering::Greater => return KanDecisionReason::ShantenRegresses,
        Ordering::Less => return KanDecisionReason::ShantenImprovedNotComparable,
        Ordering::Equal => {}
    }

    if post_kan.acceptance_remaining < baseline.acceptance_remaining
        || post_kan.acceptance_type_count < baseline.acceptance_type_count
    {
        return KanDecisionReason::AcceptanceRegresses;
    }

    compare_offense_value(baseline, post_kan)
}

// 速度が悪化しない候補について、暗槓前後の攻撃打点を比べる。
//
// 同じ尺度の値を確定できない組み合わせはすべて [`KanDecisionReason::ValueNotEvaluable`] にし、
// 速度非劣化だけを根拠に暗槓へ倒さない。
fn compare_offense_value(
    baseline: KanHandDiagnostic,
    post_kan: KanHandDiagnostic,
) -> KanDecisionReason {
    let (Some(baseline_offense), Some(post_kan_offense)) = (baseline.offense, post_kan.offense)
    else {
        return KanDecisionReason::ValueNotEvaluable;
    };

    // リーチ手とダマ手の打点は別 baseline の値なので同じ尺度で比べない。
    if baseline_offense.mode != post_kan_offense.mode
        || baseline_offense.mode == TenpaiOffenseMode::Unknown
    {
        return KanDecisionReason::ValueNotEvaluable;
    }

    let (Some(baseline_total), Some(post_kan_total)) = (
        baseline_offense.value.weighted_total(),
        post_kan_offense.value.weighted_total(),
    ) else {
        return KanDecisionReason::ValueNotEvaluable;
    };

    if post_kan_total < baseline_total {
        return KanDecisionReason::ValueRegresses;
    }

    KanDecisionReason::EligibleAnkanNoRegression
}

// 暗槓で consumed 4枚を取り除いた後の副露と concealed hand。
//
// 手牌 + ツモ牌から consumed の物理牌をちょうど1枚ずつ取り除き、取り除いた4枚がカンの形に
// なることを既存 [`Meld::shape`] で確かめる。形の規則をこの層で持たない。
fn ankan_meld_and_concealed_tiles(
    ctx: &GameContext,
    consumed: &[TileId],
) -> Option<(Meld, Vec<TileId>)> {
    if consumed.len() != ANKAN_CONSUMED_TILE_COUNT {
        return None;
    }

    let mut remaining: Vec<TileId> = ctx
        .hand_tiles()
        .iter()
        .copied()
        .chain(ctx.drawn_tile())
        .collect();
    for consumed_tile in consumed {
        let position = remaining.iter().position(|held| held == consumed_tile)?;
        remaining.remove(position);
    }

    let meld = Meld::new(MeldKind::Ankan, consumed.to_vec(), None);
    meld.shape()?;
    Some((meld, remaining))
}

// 暗槓しない場合に採用する通常打牌の、打牌後13枚の既存評価。
//
// 向聴・受け入れは production の通常打牌選択が求めた [`DiscardEvaluation`] そのもので、打点も
// 押し引き・リーチ判断と同じ helper を通す。この層で求め直す評価は持たない。
fn evaluate_baseline_hand(
    ctx: &GameContext,
    legal_actions: &[LegalAction],
    evaluation: &DiscardEvaluation,
) -> KanHandDiagnostic {
    let offense = selected_discard_tenpai_wait_availability(ctx, evaluation)
        .map(|wait| evaluate_tenpai_offense_value(ctx, evaluation, &wait, legal_actions));

    KanHandDiagnostic {
        shanten: evaluation.min_shanten_after_discard(),
        acceptance_remaining: evaluation.acceptance_total_remaining(),
        acceptance_type_count: evaluation.acceptance_type_count(),
        offense,
    }
}

// 暗槓後13枚相当 state の既存評価。見え牌の有無による経路分岐は通常打牌評価と揃える。
fn evaluate_post_kan_hand(
    ctx: &GameContext,
    legal_actions: &[LegalAction],
    tiles: &[TileId],
    melds: &[Meld],
    fixed_meld_count: FixedMeldCount,
) -> KanHandDiagnostic {
    let counts = TileCounts::from_tiles(tiles.iter().copied());
    let shanten = calculate_shanten_with_fixed_melds(&counts, fixed_meld_count).min();
    let acceptance = if ctx.visible_tiles().is_empty() {
        calculate_acceptance_with_fixed_melds(&counts, fixed_meld_count)
    } else {
        calculate_acceptance_with_fixed_melds_and_visible_tiles(
            &counts,
            fixed_meld_count,
            ctx.visible_tiles(),
        )
    };
    let offense = post_kan_tenpai_offense(
        ctx,
        legal_actions,
        tiles,
        melds,
        &counts,
        fixed_meld_count,
        &acceptance,
    );

    KanHandDiagnostic {
        shanten,
        acceptance_remaining: acceptance.total_remaining(),
        acceptance_type_count: acceptance.tiles.len(),
        offense,
    }
}

// 暗槓後13枚相当 state がテンパイの場合の攻撃打点。テンパイでない場合と、待ちや完成手を
// 組み立てられない場合は `None`。
//
// 待ちは既存のフリテン基盤、完成手は既存の [`tenpai_completed_hands`]、打点と攻撃モードは
// 押し引き・リーチ判断が使う [`evaluate_tenpai_offense_with_hands`] をそのまま通す。暗槓は
// 評価対象の副露として渡すので、符も役も既存 scoring が暗槓込みで求めた値になる。
//
// 自分の河は暗槓で変わらない。履歴依存フリテンは、暗槓の前に自分のツモを経ている事実を
// `ctx` 側の既存補正 ([`GameContext::history_furiten_after_own_discard`]) から取る。カンで
// 増えるドラ表示牌は未知なので、`ctx` の既知のドラ表示牌だけで評価する。
#[allow(clippy::too_many_arguments)]
fn post_kan_tenpai_offense(
    ctx: &GameContext,
    legal_actions: &[LegalAction],
    tiles: &[TileId],
    melds: &[Meld],
    counts: &TileCounts,
    fixed_meld_count: FixedMeldCount,
    acceptance: &EffectiveAcceptance,
) -> Option<TenpaiOffenseValue> {
    let wait = tenpai_wait_availability(
        acceptance,
        &structural_acceptance_tile_types_with_fixed_melds(counts, fixed_meld_count),
        &OwnDiscards::from_optional_river(ctx.own_discards()),
        ctx.history_furiten_after_own_discard(),
    )?;
    let hands =
        tenpai_completed_hands(tiles, melds, acceptance, Some(&wait), ctx.visible_tiles()).ok()?;

    Some(evaluate_tenpai_offense_with_hands(ctx, &wait, legal_actions, Some(&hands)).offense)
}

#[cfg(test)]
mod tests;
