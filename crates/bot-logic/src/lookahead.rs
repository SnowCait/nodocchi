//! 打牌候補ごとの2手先評価。
//!
//! 「現在打牌 → その打牌後に実際にツモり得る牌を1枚ツモった仮想手牌 → 既存打牌評価による次の
//! 最良打牌」を構造化して返す pure な基盤。向聴・受け入れ・一向聴形分類・文脈反映・打牌比較は
//! すべて既存実装を呼び出し、2手先専用の計算器は持たない。
//!
//! # 仮想ツモ対象
//!
//! 現在打牌後の状態から、見え牌を反映して残枚数が 1 枚以上ある牌を [`DrawTransition`] で分類
//! する。向聴数を下げる牌 ([`DrawTransition::Progress`]) は既存の受け入れ
//! ([`EffectiveAcceptance`]) そのもので、向聴数を維持する牌
//! ([`DrawTransition::SameShanten`]) は受け入れに含めない lookahead 専用の候補として別に列挙
//! する。分類はどちらも既存 shanten calculator の結果だけで決まり、向聴数が悪化する牌は対象外。
//! 受け入れの semantics (「1枚加えると向聴数が下がる牌」) は変えないため、same-shanten の牌が
//! [`EffectiveAcceptance`] へ混ざることはない。
//!
//! # 仮想ツモ牌の物理牌
//!
//! 仮想ツモ対象は 34 種の [`TileType`] 単位なので、仮想ツモ牌の物理牌 ([`TileId`]) は1つに
//! 決まらない。赤5のある牌種でその赤5がまだ見えていない場合だけ、赤と黒の2つの物理牌 variant が
//! あり得る。分割は最終和了牌と同じ共有 helper ([`physical_tile_variants`]) が持ち、牌種単位の
//! 残枚数を赤 / 黒へ分ける。
//!
//! 赤5と黒5では和了手の打点が変わるため、variant ごとに既存打牌評価と既存比較をそのまま通す。
//! 将来打点を使う枝では2手目の最良打牌そのものが変わり得る。同じく same-shanten の枝では仮想
//! ツモ牌をそのまま切っても向聴数が変わらないため、赤5を切るかどうかを見る既存の軸まで比較が
//! 進み、最良打牌が赤 / 黒で変わり得る。向聴数を下げる枝で打点を使わない場合だけは、仮想ツモ牌を
//! 2手目にそのまま切ると向聴が戻って Shanten 軸で必ず負けるため、赤5かどうかは最良打牌を変えず、
//! 残枚数の内訳だけが赤 / 黒へ分かれる。仮想ツモ牌以外の牌は現在の手牌の物理牌をそのまま
//! 引き継ぐため、どの variant も通常打牌評価と同じ文脈で評価できる。
//!
//! - 副露済み面子数・見え牌・場風・自風・ドラ表示牌: すべて通常打牌評価と同じ値を反映する
//! - 役牌 (`discarded_value_honor_count`): 牌種と場風・自風だけで決まるので必ず反映する
//! - 通常ドラ / 赤ドラ: 仮想ツモ牌も物理牌が決まるので通常打牌評価と同じ経路で反映する
//! - 1手目に切る物理牌: 通常打牌評価が合法 Dahai に合わせて確定した牌をそのまま除去する
//!
//! # 将来打点
//!
//! 2手目の打牌後がテンパイになる枝については、上位層が渡す評価器
//! ([`ProspectiveTenpaiValuator`]) でそのテンパイの確定打点を求め、2手目の打牌候補の比較と
//! 現在打牌の集計の両方に使う。Reach / Damaten policy も点数計算もこの module は持たず、
//! 評価器を渡されない場合は打点を一切使わない既存の比較になる。same-shanten の枝は2手目を
//! 切ってもテンパイにならないため、打点を持たない。
//!
//! # 打牌選択が使う集計値
//!
//! 打牌選択が使う前方集計値 ([`forward_metrics`]) のうち、残枚数を重みにした既存の集計値
//! ([`WeightedForwardMetric`]) は従来どおり向聴数を下げる枝だけを集計する。same-shanten の枝は
//! 2手目の打牌後も向聴数が変わらず、既存 [`ForwardMetrics`] の集計条件 (次打牌後の向聴数が1つ
//! 下がっていること) を満たさないため、そこへは寄与しない。same-shanten 側の同じ規則の集計値は
//! [`same_shanten_forward_metric_for_candidate`] から観測できる。
//!
//! Progress と SameShanten を1つの尺度へ統合するのは self-tsumo continuation
//! ([`ForwardMetrics::expected_self_tsumo_value`]) の方で、深さの違う枝を「その経路を引く確率 ×
//! テンパイ到達後の期待ツモ支払い」へ揃える。確率も期待支払いも [`crate::self_tsumo`] の
//! 閉形式そのままで、この module は係数も threshold も固定 horizon も持たない。集計に必要な
//! ツモ打点は上位層の評価器 ([`ProspectiveTsumoValuator`]) が返し、残り自摸機会は
//! [`LookaheadInputs::with_own_future_draws`] が受け取る。どちらか一方でも欠ける局面では新しい
//! 集計値を持たない。
//!
//! # same-shanten の枝の先にあるテンパイ
//!
//! same-shanten の枝は2手目の打牌後もまだ同じ向聴数なので、その枝がどれだけ強いテンパイへ
//! 到達するかは2手目までの評価では見えない。現在打牌後が1向聴の候補に限り、same-shanten の枝を
//! もう1段だけ進めた
//!
//! ```text
//! 現在打牌 → same-shanten ツモ → 2手目の最良打牌 (まだ1向聴)
//!          → その1向聴が持つ既存受け入れのツモ → 3手目の最良打牌 → テンパイ
//! ```
//!
//! を [`SameShantenDownstreamDiagnostic`] として構築できる。3手目へ進むツモ牌は2手目の打牌評価が
//! 持つ受け入れ ([`DiscardEvaluation::acceptance_after_discard`]) そのもので、テンパイへ進む牌の
//! 判定をこの module が作り直すことはない。2手目・3手目の打牌評価・比較・物理牌 variant・将来
//! 打点はどれも1段目と同じ helper を通る。
//!
//! 集計値 ([`DiscardLookaheadDiagnostic::same_shanten_downstream_value`]) は
//!
//! ```text
//! Σ(same-shanten ツモの物理牌 variant 残枚数
//!   × Σ(3手目へ進むツモの物理牌 variant 残枚数
//!       × Σ(最終和了牌の物理牌 variant 残枚数 × 支払い合計)))
//! ```
//!
//! で、平均へ正規化しない生の重み付き合計。枝の深さが違うため既存の
//! [`WeightedForwardMetric::prospective_value`] とは scale が違い、打牌選択はこの値を使わない。
//!
//! 探索は2手目の評価よりさらに重いため、詳細診断へ無条件に含めるかどうかは
//! [`LookaheadInputs::with_same_shanten_downstream`] で明示的に指定する。self-tsumo continuation
//! を集計する局面では、指定が無くても比較対象の候補 ([`forward_target_mask`]) だけ同じ枝を進める。
//! 選択専用経路と詳細診断がどちらも同じ枝集合を進めるため、詳細診断の有無で打牌選択の結果は
//! 変わらない。

use crate::acceptance::{
    DrawableTile, EffectiveAcceptance, EffectiveAcceptanceTile,
    same_shanten_draws_with_fixed_melds_and_seen, unknown_tile_count,
};
use crate::discard::{
    CandidateSeen, DecorationContext, DiscardEvaluation, ShapePenaltyMode, decorate_evaluations,
    evaluate_discards_with_seen, split_discarded_tile,
};
use crate::furiten::TENPAI_SHANTEN;
use crate::iishanten::IishantenShape;
use crate::selection::{
    ForwardMetricAccumulator, ForwardMetrics, TenpaiWaitMetric, WeightedForwardMetric,
    best_discard_selection_index_with_forward_metrics, forward_target_mask,
    requires_forward_metrics,
};
use crate::self_tsumo::{SelfTsumoFacts, SelfTsumoPath, TenpaiTsumoValue};
use crate::shanten::{EffectiveShanten, FixedMeldCount};
use crate::tile::{PhysicalTileVariant, TileId, TileType, physical_tile_variants, seen_red_fives};
use crate::tile_counts::TileCounts;

/// same-shanten の枝をテンパイまで追う対象の向聴数。
const IISHANTEN_SHANTEN: i8 = TENPAI_SHANTEN + 1;

/// 2手目の打牌後に出来上がる仮想テンパイ1つ分の入力。
///
/// 待ちも残枚数も既存の打牌評価が持つ受け入れそのもので、この module は待ちを計算し直さない。
pub struct ProspectiveTenpai<'a> {
    /// 2手目を切った後の concealed 物理牌。副露牌は含まない。
    pub concealed_tiles: &'a [TileId],
    /// テンパイ形の受け入れ。待ち牌種と残枚数の source of truth。
    pub acceptance: &'a EffectiveAcceptance,
    /// 1手目と2手目に切った物理牌。テンパイ時点で見え牌になる牌として渡す。
    pub discarded_tiles: &'a [TileId],
}

/// 仮想テンパイの確定打点を求める外部評価器。
///
/// bot-logic は Reach / Damaten policy も局面型も持たないため、実装は上位層が渡す。返り値は
/// Σ(最終和了牌の物理牌 variant 残枚数 × 支払い合計) で、平均へ正規化しない。打点を確定できない
/// 場合は 0 点にせず `None` を返す。
pub trait ProspectiveTenpaiValuator {
    fn tenpai_value(&self, tenpai: &ProspectiveTenpai<'_>) -> Option<u64>;
}

/// 仮想テンパイを自分のツモ和了だけで評価する外部評価器。
///
/// [`ProspectiveTenpaiValuator`] はロン baseline の確定打点を返すのに対し、こちらは
/// [`crate::self_tsumo`] の確率模型が使う「ツモ和了できる待ちの残枚数とツモ打点」を返す。
/// Reach / Damaten policy も点数計算も bot-logic は持たないため、実装は上位層が渡す。
///
/// ツモ baseline で役が無い待ちは和了できないので、成功する待ちにも打点にも含めない。打点を
/// 確定できない待ちが混じる場合は 0 点にせず `None` を返す。
pub trait ProspectiveTsumoValuator {
    fn tenpai_tsumo_value(&self, tenpai: &ProspectiveTenpai<'_>) -> Option<TenpaiTsumoValue>;
}

/// 仮想ツモ牌が現在打牌後の向聴数をどう変えるか。
///
/// 分類は既存 shanten calculator の結果だけで決まる。向聴数が悪化する牌はどちらにも入らず、
/// 仮想ツモの対象にしない。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DrawTransition {
    /// 向聴数が下がる牌。既存の受け入れ ([`EffectiveAcceptance`]) そのもの。
    Progress,
    /// 向聴数を維持する牌。受け入れには含めない lookahead 専用の仮想ツモ候補。
    SameShanten,
}

/// 現在の打牌候補1件について、その打牌後にツモり得る牌を1枚ツモった仮想手牌を既存打牌評価へ
/// かけた2手先評価。
///
/// pure なデータであり、押し引き・鳴き・リーチ判断のどれにも使用しない。打牌選択が使うのは
/// [`DiscardLookaheadDiagnostic::weighted_forward_metric`] が返す集計値だけである。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscardLookaheadDiagnostic {
    /// 現在の打牌候補の牌種。
    pub discard: TileType,
    /// 現在打牌後の仮想ツモ牌ごとの2手先評価。
    ///
    /// 先に向聴数を下げる牌 ([`DrawTransition::Progress`]) が現在打牌後の受け入れと同じ順序・
    /// 同じ対象牌で並び、その後ろに向聴数を維持する牌 ([`DrawTransition::SameShanten`]) が
    /// 牌種順で並ぶ。牌種はどちらの分類にも重複して現れない。
    pub draws: Vec<DrawLookaheadDiagnostic>,
}

impl DiscardLookaheadDiagnostic {
    pub fn draw(&self, tile: TileType) -> Option<&DrawLookaheadDiagnostic> {
        self.draws.iter().find(|draw| draw.draw == tile)
    }

    /// 指定した分類の仮想ツモ枝だけを並べる。
    pub fn draws_with(
        &self,
        transition: DrawTransition,
    ) -> impl Iterator<Item = &DrawLookaheadDiagnostic> {
        self.draws
            .iter()
            .filter(move |draw| draw.transition == transition)
    }

    /// 構築済みの枝から same-shanten 手変わりの前方集計値を求める。
    ///
    /// 集計規則も accumulator も既存の前方集計値と同じで、対象を same-shanten の枝に、集計する
    /// 2手目打牌後の向聴数をその枝のツモ後向聴数 (= 現在打牌後の向聴数) に限定するだけ。新しい
    /// 係数も threshold も持たない。打牌選択はこの値を使わない。
    pub fn same_shanten_forward_metric(&self) -> WeightedForwardMetric {
        accumulate_same_shanten_draws(self.draws_with(DrawTransition::SameShanten), |variant| {
            variant.prospective_value
        })
    }

    /// 構築済みの枝から same-shanten 手変わりの先にある将来テンパイの重み付き打点を求める。
    ///
    /// 集計規則も未確定の扱いも既存 accumulator そのままで、重みに使う打点が「2手目打牌後の
    /// テンパイの確定打点」ではなく「2手目打牌後の1向聴からさらに1段進めたテンパイの重み付き
    /// 打点」([`SameShantenDownstreamDiagnostic::weighted_value`]) になるだけ。
    ///
    /// [`LookaheadInputs::with_same_shanten_downstream`] を指定せずに構築した診断では、
    /// 集計対象の枝が先の枝を持たないため `None` になる。表示のために探索し直さない。
    ///
    /// 打牌選択はこの値を使わない。枝の深さが違うため既存の
    /// [`WeightedForwardMetric::prospective_value`] とは scale が違い、統合 policy は未決定。
    pub fn same_shanten_downstream_value(&self) -> Option<u64> {
        accumulate_same_shanten_draws(
            self.draws_with(DrawTransition::SameShanten),
            DrawVariantLookaheadDiagnostic::downstream_value,
        )
        .prospective_value
    }

    /// 構築済みの枝から self-tsumo continuation の期待支払いを集計する。
    ///
    /// 対象は
    ///
    /// ```text
    /// A. 1回目のツモが向聴数を下げる → 2手目の最良打牌 → テンパイ
    /// B. 1回目のツモが向聴数を維持する → 2手目の最良打牌 → 1向聴
    ///    → 2回目のツモが向聴数を下げる → 3手目の最良打牌 → テンパイ
    /// ```
    ///
    /// の2種類で、値は Σ(その経路を引く確率 × テンパイ到達後の期待ツモ支払い)
    /// [[`crate::self_tsumo::SELF_TSUMO_VALUE_SCALE`]]。確率も期待支払いも
    /// [`crate::self_tsumo`] の閉形式そのままで、固定 horizon も係数も持たない。
    ///
    /// 2回続けて向聴数を維持する枝は今回の探索範囲外で、寄与 0 になる (未確定ではない)。
    /// テンパイへ到達した枝のツモ打点を1つでも確定できない場合と、手変わりの枝の先を探索して
    /// いない場合は 0 点へ潰さず `None`。
    pub fn expected_self_tsumo_value(&self, facts: SelfTsumoFacts) -> Option<u64> {
        let mut total = 0u64;
        for draw in &self.draws {
            for variant in &draw.variants {
                let value = match draw.transition {
                    DrawTransition::Progress => terminal_self_tsumo_value(variant, facts, || {
                        SelfTsumoPath::immediate(variant.remaining, facts.unknown_tiles)
                    })?,
                    DrawTransition::SameShanten => same_shanten_self_tsumo_value(variant, facts)?,
                };
                total = total.saturating_add(value);
            }
        }
        Some(total)
    }

    /// 構築済みの枝から打牌選択用の weighted tenpai wait を集計する。
    ///
    /// 集計規則は選択専用経路 ([`forward_metrics`]) と共有するため、詳細診断を構築した場合に
    /// 同じ枝を2回評価しなくてよい。
    pub fn tenpai_wait_metric(&self) -> TenpaiWaitMetric {
        self.weighted_forward_metric(TENPAI_SHANTEN)
    }

    /// 集計対象は向聴数を下げる枝だけで、選択専用経路 ([`forward_metrics`]) と同じ枝集合になる。
    /// same-shanten の枝は2手目の打牌後も向聴数が変わらず `required_next_shanten` を満たさない
    /// ため元から寄与しないが、詳細診断の有無で選択結果が変わらないことを枝集合そのもので
    /// 保証する。
    pub fn weighted_forward_metric(&self, required_next_shanten: i8) -> WeightedForwardMetric {
        accumulate_draws(
            self.draws_with(DrawTransition::Progress),
            |_| required_next_shanten,
            |variant| variant.prospective_value,
        )
    }
}

/// 現在打牌後の仮想ツモ牌1牌種分の2手先評価。
///
/// [`DrawTransition::Progress`] の枝では `draw` / `remaining` / `shanten_after_draw` は現在
/// 打牌の [`DiscardEvaluation`] が持つ受け入れの値そのもので、診断のために再計算しない。
/// [`DrawTransition::SameShanten`] の枝も残枚数の数え方とツモ後向聴数の求め方は受け入れと同じ
/// 列挙を共有する。`remaining` は牌種ごとの残枚数を生データのまま保持し、期待値や加重平均へ
/// 潰さない。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DrawLookaheadDiagnostic {
    pub draw: TileType,
    /// 牌種単位の残枚数。`variants` の残枚数の合計と一致する。
    pub remaining: u8,
    pub shanten_after_draw: EffectiveShanten,
    /// この牌をツモった場合に現在打牌後の向聴数がどう変わるか。
    pub transition: DrawTransition,
    /// 仮想ツモ牌の物理牌ごとの2手先評価。赤5と黒5のどちらもあり得る牌種では2件になる。
    pub variants: Vec<DrawVariantLookaheadDiagnostic>,
}

impl DrawLookaheadDiagnostic {
    pub fn variant(&self, drawn_tile: TileId) -> Option<&DrawVariantLookaheadDiagnostic> {
        self.variants
            .iter()
            .find(|variant| variant.drawn_tile == drawn_tile)
    }
}

/// 仮想ツモ牌の物理牌1つ分の2手先評価。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DrawVariantLookaheadDiagnostic {
    /// 仮想的にツモった物理牌。
    pub drawn_tile: TileId,
    /// この variant の残枚数。牌種単位の残枚数を赤 / 黒へ分けたもの。
    pub remaining: u8,
    /// 仮想ツモ後の手牌に既存打牌評価と既存比較順を適用した最良打牌。打牌候補が1件も無い場合
    /// だけ `None`。数値のコピーではなく評価そのものを保持する。
    ///
    /// 副露済み面子数・見え牌・場風・自風・ドラ表示牌は現在の打牌評価と同じ値を反映する。
    pub next_discard: Option<DiscardEvaluation>,
    /// `next_discard` 後のテンパイの確定打点。Σ(最終和了牌 variant 残枚数 × 支払い合計)。
    ///
    /// テンパイでない枝・評価器を渡されなかった場合・打点を確定できない場合は `None`。
    /// 2手目の打牌候補の比較にもこの値を使うため、選択と診断で別々の打点を持たない。
    pub prospective_value: Option<u64>,
    /// `next_discard` 後のテンパイをツモ和了だけで評価した continuation の材料。
    ///
    /// テンパイでない枝・ツモ評価器を渡されなかった場合・ツモ打点を確定できない場合は `None`。
    /// ロン baseline の [`Self::prospective_value`] とは別の baseline で、流用しない。
    pub tsumo_continuation: Option<TenpaiTsumoValue>,
    /// `next_discard` 後がまだ同じ向聴数の枝を、テンパイまでもう1段進めた探索結果。
    ///
    /// [`LookaheadInputs::with_same_shanten_downstream`] を指定した same-shanten の枝だけが
    /// 持つ。探索しなかった枝は `None` で、打点を確定できなかったこと
    /// ([`SameShantenDownstreamDiagnostic::weighted_value`] が `None`) とは区別する。
    pub downstream: Option<SameShantenDownstreamDiagnostic>,
}

impl DrawVariantLookaheadDiagnostic {
    /// 先の枝の重み付き打点。探索しなかった枝と確定できない枝はどちらも `None`。
    ///
    /// どちらも「この枝の打点を確定できない」という同じ意味になり、既存 accumulator が候補
    /// 全体を `None` にする。確定しない打点を 0 点として集計しない。
    pub fn downstream_value(&self) -> Option<u64> {
        self.downstream.as_ref()?.weighted_value()
    }

    pub fn next_discard_tile(&self) -> Option<TileType> {
        self.next_discard.as_ref().map(|next| next.discard)
    }

    pub fn next_min_shanten(&self) -> Option<i8> {
        self.next_discard
            .as_ref()
            .map(DiscardEvaluation::min_shanten_after_discard)
    }

    pub fn next_acceptance_total_remaining(&self) -> Option<u8> {
        self.next_discard
            .as_ref()
            .map(DiscardEvaluation::acceptance_total_remaining)
    }

    pub fn next_acceptance_type_count(&self) -> Option<usize> {
        self.next_discard
            .as_ref()
            .map(DiscardEvaluation::acceptance_type_count)
    }

    pub fn next_standard_iishanten_shape(&self) -> Option<IishantenShape> {
        self.next_discard
            .as_ref()
            .map(|next| next.standard_iishanten_shape_after_discard)
    }
}

/// same-shanten の枝の2手目打牌後 (まだ同じ向聴数) から、テンパイまでもう1段進めた探索結果。
///
/// `draws` は2手目の打牌評価が持つ既存受け入れ
/// ([`DiscardEvaluation::acceptance_after_discard`]) そのもので、テンパイへ進む牌をここで判定
/// し直さない。各枝の `next_discard` は既存打牌評価と既存比較順が選んだ3手目の最良打牌で、
/// `prospective_value` はその打牌後のテンパイの確定打点。
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SameShantenDownstreamDiagnostic {
    pub draws: Vec<DrawLookaheadDiagnostic>,
}

impl SameShantenDownstreamDiagnostic {
    pub fn draw(&self, tile: TileType) -> Option<&DrawLookaheadDiagnostic> {
        self.draws.iter().find(|draw| draw.draw == tile)
    }

    /// Σ(3手目へ進むツモの物理牌 variant 残枚数 × 最終テンパイの確定打点)。
    ///
    /// 集計規則も未確定の扱いも既存 accumulator そのままで、テンパイにならない枝は集計対象に
    /// ならず、集計対象の枝が1つでも打点を確定できなければ `None` になる。
    pub fn weighted_value(&self) -> Option<u64> {
        accumulate_draws(
            self.draws.iter(),
            |_| TENPAI_SHANTEN,
            |variant| variant.prospective_value,
        )
        .prospective_value
    }
}

/// 全打牌候補分の2手先診断。
///
/// `candidates` は入力の打牌候補評価と同じ順序・同じ件数で、selected 候補だけでなく runner-up を
/// 含む全候補に対応する。
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct LookaheadDiagnostic {
    pub candidates: Vec<DiscardLookaheadDiagnostic>,
}

impl LookaheadDiagnostic {
    pub fn candidate(&self, discard: TileType) -> Option<&DiscardLookaheadDiagnostic> {
        self.candidates
            .iter()
            .find(|candidate| candidate.discard == discard)
    }
}

/// 2手先評価の入力。通常打牌評価が使う値と同じものだけを持ち、上位層の局面型には依存しない。
///
/// `tiles` は打牌前の全手牌 (物理牌)、`fixed_meld_count` / `dora_indicators` / `round_wind` /
/// `seat_wind` は現在の打牌評価に使ったものと同じ値を渡す。
pub struct LookaheadInputs<'a> {
    fixed_meld_count: FixedMeldCount,
    dora_indicators: &'a [TileId],
    round_wind: Option<TileType>,
    seat_wind: Option<TileType>,
    valuator: Option<&'a dyn ProspectiveTenpaiValuator>,
    tsumo_valuator: Option<&'a dyn ProspectiveTsumoValuator>,
    // 現在打牌後に自分へ残っている自摸機会。exact な fact を持たない経路では `None`。
    own_future_draws: Option<u32>,
    same_shanten_downstream: bool,
    // 1手目の打牌前の手牌。深い枝の状態と同じ型で持ち、段ごとに別の組み立てをしない。
    root: HandState,
}

impl<'a> LookaheadInputs<'a> {
    pub fn new(
        tiles: &'a [TileId],
        fixed_meld_count: FixedMeldCount,
        dora_indicators: &'a [TileId],
        round_wind: Option<TileType>,
        seat_wind: Option<TileType>,
    ) -> Self {
        Self {
            fixed_meld_count,
            dora_indicators,
            round_wind,
            seat_wind,
            valuator: None,
            tsumo_valuator: None,
            own_future_draws: None,
            same_shanten_downstream: false,
            root: HandState {
                counts: TileCounts::from_tiles(tiles.iter().copied()),
                tiles: tiles.to_vec(),
                seen: CandidateSeen::hand_only(),
                red_five_seen: seen_red_fives(tiles.iter().copied()),
                discarded: Vec::new(),
            },
        }
    }

    /// 手牌以外に見えている牌も反映する。
    ///
    /// 2手目の残枚数は「1手目の打牌前に見えていた牌 + 1手目に切った牌1枚」を seen として求める。
    /// 1手目に切った牌は2手目時点で見え牌なので、山に残っている牌として数え直さない。
    /// visible tiles は自分の手牌を含むため、既存の残枚数計算と同じ手牌差し引きで二重計上を
    /// 防ぐ。仮想ツモ牌を赤 / 黒へ分ける判定にも同じ見え牌を使う。
    pub fn with_visible_tiles(mut self, visible_tiles: &[TileId]) -> Self {
        if visible_tiles.is_empty() {
            return self;
        }
        self.root.seen = CandidateSeen::from_visible_tiles(&self.root.counts, visible_tiles);
        self.root.red_five_seen =
            seen_red_fives(self.root.tiles.iter().chain(visible_tiles).copied());
        self
    }

    /// 2手目の打牌後のテンパイに将来打点を付ける評価器を設定する。
    ///
    /// 渡さない場合、2手目の打牌比較も現在打牌の集計も打点を一切使わない。
    pub fn with_prospective_valuator(
        mut self,
        valuator: &'a dyn ProspectiveTenpaiValuator,
    ) -> Self {
        self.valuator = Some(valuator);
        self
    }

    /// テンパイをツモ和了だけで評価する評価器を設定する。
    ///
    /// 残り自摸機会 ([`Self::with_own_future_draws`]) と両方揃った場合だけ、1向聴候補の
    /// self-tsumo continuation ([`DiscardLookaheadDiagnostic::expected_self_tsumo_value`]) を
    /// 集計する。片方でも欠ける局面では新しい軸を持たず、既存の集計値だけになる。
    pub fn with_tsumo_valuator(mut self, valuator: &'a dyn ProspectiveTsumoValuator) -> Self {
        self.tsumo_valuator = Some(valuator);
        self
    }

    /// 現在打牌後に自分へ残っている自摸機会を設定する。
    ///
    /// 山の残枚数から導いた exact な fact だけを渡す。推測した巡目や河の枚数から作った近似値を
    /// 渡さないこと。
    pub fn with_own_future_draws(mut self, own_future_draws: u32) -> Self {
        self.own_future_draws = Some(own_future_draws);
        self
    }

    /// self-tsumo continuation を集計するための事実。材料が揃わない局面では `None`。
    ///
    /// 未確認牌の総数は現在の手牌と見え牌から求めた値で、打牌候補によらず共通になる。打牌で
    /// 手牌から河へ移る1枚はどちらの経路でも見えているため、候補ごとに変わらない。
    pub fn self_tsumo_facts(&self) -> Option<SelfTsumoFacts> {
        self.tsumo_valuator?;
        Some(SelfTsumoFacts {
            unknown_tiles: unknown_tile_count(&self.root.counts, self.root.seen.base()),
            own_future_draws: self.own_future_draws?,
        })
    }

    /// same-shanten の枝を、テンパイまでもう1段進めた詳細診断
    /// ([`DrawVariantLookaheadDiagnostic::downstream`]) を構築する。
    ///
    /// 対象は現在打牌後が1向聴の候補の same-shanten の枝だけ。2手目までの評価より探索が
    /// 深くなるため、詳細診断が必要な経路だけで指定する。この探索の結果は打牌選択にも2手目
    /// `next_discard` の選択にも使わないため、指定しても選択結果は変わらない。
    ///
    /// 選択済みの1候補について集計値だけが必要な場合は詳細診断を構築せず、
    /// [`same_shanten_downstream_value_for_candidate`] を使う。
    pub fn with_same_shanten_downstream(mut self) -> Self {
        self.same_shanten_downstream = true;
        self
    }
}

/// 全打牌候補の2手先診断を構築する。
///
/// 現在打牌後の受け入れは `evaluations` が持つ値をそのまま使い、診断のために再計算しない。
///
/// 2手目の打牌評価・受け入れ・向聴・一向聴形分類・文脈反映・比較は既存の打牌評価経路をそのまま
/// 呼び出す。2手先専用の shanten / acceptance / comparator / shape evaluator は持たない。
pub fn diagnose_lookahead(
    inputs: &LookaheadInputs,
    evaluations: &[DiscardEvaluation],
) -> LookaheadDiagnostic {
    let targets = self_tsumo_target_mask(inputs, evaluations);
    LookaheadDiagnostic {
        candidates: evaluations
            .iter()
            .zip(targets)
            .map(|(evaluation, target)| {
                search_candidate(
                    inputs,
                    evaluation,
                    &diagnostic_scopes(inputs, evaluation, target),
                )
            })
            .collect(),
    }
}

/// 打牌選択用の前方集計値を求める。
///
/// 戻り値は `evaluations` と同じ順序・同じ件数で、前方評価を計算しなかった候補は
/// [`ForwardMetrics::default`]。計算対象は最善向聴数が1以上で、それを維持する候補が複数ある
/// 場合の最善候補だけ。1向聴では weighted tenpai wait と打点込みの集計値、2向聴以上では
/// weighted next acceptance を返す。
///
/// 枝の評価は詳細診断 ([`diagnose_lookahead`]) と同じ helper を共有し、選択用に
/// [`LookaheadDiagnostic`] を構築しない。
pub fn forward_metrics(
    inputs: &LookaheadInputs,
    evaluations: &[DiscardEvaluation],
) -> Vec<ForwardMetrics> {
    if !requires_forward_metrics(evaluations) {
        return vec![ForwardMetrics::default(); evaluations.len()];
    }

    let best_shanten = best_shanten(evaluations);
    let targets = forward_target_mask(evaluations);
    evaluations
        .iter()
        .zip(targets)
        .map(|(evaluation, target)| {
            if !target {
                return ForwardMetrics::default();
            }
            let candidate =
                search_candidate(inputs, evaluation, &selection_scopes(inputs, evaluation));
            forward_metrics_from_candidate(inputs, evaluation, &candidate, best_shanten)
        })
        .collect()
}

/// 構築済みの2手先診断から打牌選択用の前方集計値を求める。
///
/// 詳細診断を作る経路で、同じ「現在打牌 × 受け入れ牌 × 次打牌評価」を2回計算しないための入口。
/// 対象候補の条件と集計規則は選択専用経路と同じなので、詳細診断の有無で選択結果は変わらない。
///
/// `lookahead` は `evaluations` から構築したものを渡す。候補の順序・牌種が対応しない場合は
/// 推測せず [`ForwardMetrics::default`] にする。
pub fn forward_metrics_from_lookahead(
    inputs: &LookaheadInputs,
    evaluations: &[DiscardEvaluation],
    lookahead: &LookaheadDiagnostic,
) -> Vec<ForwardMetrics> {
    if !requires_forward_metrics(evaluations) || lookahead.candidates.len() != evaluations.len() {
        return vec![ForwardMetrics::default(); evaluations.len()];
    }

    let best_shanten = best_shanten(evaluations);
    let targets = forward_target_mask(evaluations);
    evaluations
        .iter()
        .zip(lookahead.candidates.iter())
        .zip(targets)
        .map(|((evaluation, candidate), target)| {
            if !target || candidate.discard != evaluation.discard {
                return ForwardMetrics::default();
            }
            forward_metrics_from_candidate(inputs, evaluation, candidate, best_shanten)
        })
        .collect()
}

/// 打牌候補1件だけの前方集計値を求める。
///
/// 枝の評価も集計規則も [`forward_metrics`] と同じで、対象を渡された1候補に限定するだけ。
/// 候補集合の中から既に選び終えた1件について集計値が必要な経路のための入口で、
/// [`LookaheadDiagnostic`] は構築しない。
///
/// 集計値を入れる枠 (テンパイ待ち / 次の受け入れ) は渡された候補自身の打牌後向聴数で決める。
/// 候補集合の最善候補についてはその向聴数が最善向聴数と一致するため、[`forward_metrics`] が
/// 同じ候補へ返す値と一致する。
pub fn forward_metrics_for_candidate(
    inputs: &LookaheadInputs,
    evaluation: &DiscardEvaluation,
) -> ForwardMetrics {
    let candidate = search_candidate(inputs, evaluation, &selection_scopes(inputs, evaluation));
    forward_metrics_from_candidate(
        inputs,
        evaluation,
        &candidate,
        evaluation.min_shanten_after_discard(),
    )
}

/// 打牌候補1件について、same-shanten 手変わりの前方集計値を求める。
///
/// 枝の評価も集計規則も詳細診断 ([`diagnose_lookahead`]) と同じ helper を共有し、
/// [`LookaheadDiagnostic`] を構築しない。
///
/// 現在の打牌選択はこの値を使わない。Progress と SameShanten は「テンパイまでの距離が違う枝の
/// 受け入れ」という意味の異なる量なので、1つの scalar へ混ぜるには比較 policy の決定が必要で、
/// この module は係数も threshold も持たない。
pub fn same_shanten_forward_metric_for_candidate(
    inputs: &LookaheadInputs,
    evaluation: &DiscardEvaluation,
) -> WeightedForwardMetric {
    search_candidate(inputs, evaluation, SAME_SHANTEN_ONLY).same_shanten_forward_metric()
}

/// 打牌候補1件について、same-shanten 手変わりの先にある将来テンパイの重み付き打点を求める。
///
/// 枝の評価も集計規則も詳細診断 ([`diagnose_lookahead`]) と同じ helper を共有し、
/// [`LookaheadDiagnostic`] を構築しない。詳細診断を
/// [`LookaheadInputs::with_same_shanten_downstream`] 付きで構築済みの経路は、同じ枝を2回
/// 評価しないよう [`DiscardLookaheadDiagnostic::same_shanten_downstream_value`] を使う。
///
/// 値は
///
/// ```text
/// Σ(same-shanten ツモの物理牌 variant 残枚数
///   × Σ(3手目へ進むツモの物理牌 variant 残枚数
///       × Σ(最終和了牌の物理牌 variant 残枚数 × 支払い合計)))
/// ```
///
/// で、平均へ正規化しない生の重み付き合計。打点を確定できない枝が混じる場合は 0 点へ潰さず
/// `None` になる。
///
/// 対象は現在打牌後が1向聴の候補だけで、それ以外は探索せず `None`。現在の打牌選択はこの値を
/// 使わない。既存 [`WeightedForwardMetric::prospective_value`] とは枝の深さが違うため raw scale
/// も違い、直接比較できない。統合する係数も threshold もこの module は持たない。
pub fn same_shanten_downstream_value_for_candidate(
    inputs: &LookaheadInputs,
    evaluation: &DiscardEvaluation,
) -> Option<u64> {
    if !explores_downstream(evaluation) {
        return None;
    }
    search_candidate(inputs, evaluation, SAME_SHANTEN_DOWNSTREAM).same_shanten_downstream_value()
}

// same-shanten の枝をテンパイまで追う対象かどうか。今回の対象は現在打牌後が1向聴の候補だけで、
// 2向聴以上を任意深度へ一般化しない。
fn explores_downstream(evaluation: &DiscardEvaluation) -> bool {
    evaluation.min_shanten_after_discard() == IISHANTEN_SHANTEN
}

pub fn tenpai_wait_metrics_from_lookahead(
    inputs: &LookaheadInputs,
    evaluations: &[DiscardEvaluation],
    lookahead: &LookaheadDiagnostic,
) -> Vec<Option<TenpaiWaitMetric>> {
    forward_metrics_from_lookahead(inputs, evaluations, lookahead)
        .into_iter()
        .map(|metric| metric.tenpai_wait)
        .collect()
}

fn best_shanten(evaluations: &[DiscardEvaluation]) -> i8 {
    evaluations
        .iter()
        .map(DiscardEvaluation::min_shanten_after_discard)
        .min()
        .unwrap_or(i8::MAX)
}

// 探索済みの枝から打牌選択用の前方集計値を組み立てる。選択専用経路と詳細診断経路はこの1本を
// 共有し、集計規則を複製しない。
fn forward_metrics_from_candidate(
    inputs: &LookaheadInputs,
    evaluation: &DiscardEvaluation,
    candidate: &DiscardLookaheadDiagnostic,
    best_shanten: i8,
) -> ForwardMetrics {
    forward_metrics_for_shanten(
        best_shanten,
        candidate.weighted_forward_metric(best_shanten - 1),
        self_tsumo_value_for_candidate(inputs, evaluation, candidate),
    )
}

// 現在打牌後が1向聴の候補だけが持つ self-tsumo continuation。材料が揃わない局面と1向聴以外は
// 集計しない。
fn self_tsumo_value_for_candidate(
    inputs: &LookaheadInputs,
    evaluation: &DiscardEvaluation,
    candidate: &DiscardLookaheadDiagnostic,
) -> Option<u64> {
    if !explores_downstream(evaluation) {
        return None;
    }
    candidate.expected_self_tsumo_value(inputs.self_tsumo_facts()?)
}

// 集計値を現在打牌後の向聴数に応じた枠へ入れる。打点込みの集計値は向聴数に依らず持ち回る。
fn forward_metrics_for_shanten(
    best_shanten: i8,
    metric: WeightedForwardMetric,
    expected_self_tsumo_value: Option<u64>,
) -> ForwardMetrics {
    let tenpai_wait = (best_shanten == TENPAI_SHANTEN + 1).then_some(metric);
    ForwardMetrics {
        tenpai_wait,
        next_acceptance: (tenpai_wait.is_none()).then_some(metric),
        prospective_value: metric.prospective_value,
        expected_self_tsumo_value,
    }
}

// 詳細診断が進める枝。深さと対象の違いはこの指定だけが持ち、探索そのものは選択専用集計と
// 共有する。
//
// self-tsumo continuation を集計する局面では、詳細診断を要求されていなくても手変わりの枝の
// 先まで進める。詳細診断の有無で選択に使う値が変わらないようにするための条件で、選択専用経路
// ([`selection_scopes`]) と同じ枝集合になる。
fn diagnostic_scopes(
    inputs: &LookaheadInputs,
    evaluation: &DiscardEvaluation,
    self_tsumo_target: bool,
) -> [DrawScope; 2] {
    [
        DrawScope::Progress,
        DrawScope::SameShanten {
            downstream: (inputs.same_shanten_downstream && explores_downstream(evaluation))
                || self_tsumo_target,
        },
    ]
}

// 打牌選択用の集計が進める枝。self-tsumo continuation を集計する候補だけ、手変わりの枝を
// テンパイまで進める。Progress と SameShanten を別々に探索せず、1回の探索で両方の集計値を
// 求める。
fn selection_scopes(inputs: &LookaheadInputs, evaluation: &DiscardEvaluation) -> Vec<DrawScope> {
    if self_tsumo_target(inputs, evaluation) {
        return vec![
            DrawScope::Progress,
            DrawScope::SameShanten { downstream: true },
        ];
    }
    PROGRESS_ONLY.to_vec()
}

// この候補で self-tsumo continuation を集計するか。材料が揃った局面の1向聴候補だけが対象。
fn self_tsumo_target(inputs: &LookaheadInputs, evaluation: &DiscardEvaluation) -> bool {
    inputs.self_tsumo_facts().is_some() && explores_downstream(evaluation)
}

// self-tsumo continuation を集計する候補の mask。
//
// 詳細診断も選択専用集計と同じ絞り込み ([`forward_target_mask`]) を共有し、比較対象にならない
// 候補まで手変わりの枝を深く探索しない。全合法候補を無条件に深く探索すると、選択が使わない
// 枝まで「打牌候補 × 手変わり × 次打牌 × 受け入れ × 次打牌」を回すことになる。
fn self_tsumo_target_mask(
    inputs: &LookaheadInputs,
    evaluations: &[DiscardEvaluation],
) -> Vec<bool> {
    if inputs.self_tsumo_facts().is_none() || !requires_forward_metrics(evaluations) {
        return vec![false; evaluations.len()];
    }

    forward_target_mask(evaluations)
        .into_iter()
        .zip(evaluations)
        .map(|(target, evaluation)| target && self_tsumo_target(inputs, evaluation))
        .collect()
}

// same-shanten の枝を既存 accumulator で集計する。集計対象の2手目打牌後の向聴数は枝ごとの
// ツモ後向聴数そのもので、向聴数を下げる枝の集計と同じ規則になる。重みに使う打点だけを
// 呼び出し側が決める。
fn accumulate_same_shanten_draws<'a>(
    draws: impl Iterator<Item = &'a DrawLookaheadDiagnostic>,
    value: impl Fn(&DrawVariantLookaheadDiagnostic) -> Option<u64>,
) -> WeightedForwardMetric {
    accumulate_draws(draws, |draw| draw.shanten_after_draw.min(), value)
}

// 枝を既存 accumulator へ流し込む唯一の経路。集計対象の判定も、確定しない打点を 0 点として
// 扱わない規則も accumulator が持ち、呼び出し側は対象の枝・集計する向聴数・重みに使う打点だけを
// 決める。詳細診断から集計する経路と選択専用経路が必ず同じ値になるのはこの共有による。
fn accumulate_draws<'a>(
    draws: impl Iterator<Item = &'a DrawLookaheadDiagnostic>,
    required_next_shanten: impl Fn(&DrawLookaheadDiagnostic) -> i8,
    value: impl Fn(&DrawVariantLookaheadDiagnostic) -> Option<u64>,
) -> WeightedForwardMetric {
    let mut accumulator = ForwardMetricAccumulator::new();
    for draw in draws {
        let required = required_next_shanten(draw);
        for variant in &draw.variants {
            accumulator.accumulate(
                variant.remaining,
                required,
                variant.next_discard.as_ref(),
                value(variant),
            );
        }
    }
    accumulator.finish()
}

/// 探索1段で進める仮想ツモ枝の指定。
///
/// 現在扱う深さと対象の違いはこの指定だけが持ち、状態遷移・打牌評価・比較・物理牌 variant は
/// どの段でも同じ primitive を通る。任意深度の再帰へは一般化しない。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DrawScope {
    /// 向聴数を下げる枝。対象牌・残枚数・ツモ後向聴数は打牌評価が持つ受け入れそのもの。
    Progress,
    /// 向聴数を維持する枝。`downstream` を指定した枝だけ、2手目の打牌後の状態から
    /// [`DrawScope::Progress`] をもう1段進める。
    SameShanten { downstream: bool },
}

impl DrawScope {
    // この指定で進める枝の分類。
    fn transition(self) -> DrawTransition {
        match self {
            Self::Progress => DrawTransition::Progress,
            Self::SameShanten { .. } => DrawTransition::SameShanten,
        }
    }

    // この指定の枝を、2手目の打牌後からさらに1段進めるか。
    fn continues_downstream(self) -> bool {
        matches!(self, Self::SameShanten { downstream: true })
    }
}

// 向聴数を下げる枝だけを1段進める指定。選択専用の集計と、same-shanten の枝の先の段が共有する。
const PROGRESS_ONLY: &[DrawScope] = &[DrawScope::Progress];

// 向聴数を維持する枝だけを1段進める指定。
const SAME_SHANTEN_ONLY: &[DrawScope] = &[DrawScope::SameShanten { downstream: false }];

// 向聴数を維持する枝を、その先のテンパイまで進める指定。
const SAME_SHANTEN_DOWNSTREAM: &[DrawScope] = &[DrawScope::SameShanten { downstream: true }];

// 現在の打牌候補1件を、指定した枝だけ探索する。詳細診断も選択専用の集計も same-shanten の
// 観測値もこの1本を通り、用途ごとに違うのは「どの枝を進めるか」と「結果をどう集計するか」だけ。
// 入力は変更せず、打牌後の手牌を copy で作る。
fn search_candidate(
    inputs: &LookaheadInputs,
    evaluation: &DiscardEvaluation,
    scopes: &[DrawScope],
) -> DiscardLookaheadDiagnostic {
    DiscardLookaheadDiagnostic {
        discard: evaluation.discard,
        draws: search(inputs, &inputs.root, evaluation, scopes).unwrap_or_default(),
    }
}

// 探索の1段。ある状態とその打牌から、指定した分類の仮想ツモ牌ごとに「ツモ → 既存打牌評価 →
// 既存比較 → 次段の状態」を進める。1手目も same-shanten の枝の先も同じ入口を通り、段ごとに
// 別の状態遷移を書かない。打牌候補の牌種を手牌から除けない場合だけ `None`。
fn search(
    inputs: &LookaheadInputs,
    state: &HandState,
    evaluation: &DiscardEvaluation,
    scopes: &[DrawScope],
) -> Option<Vec<DrawLookaheadDiagnostic>> {
    let branch = CandidateBranch::new(state, evaluation)?;
    Some(
        scopes
            .iter()
            .flat_map(|&scope| branch.draws(inputs, evaluation, scope))
            .collect(),
    )
}

// 仮想手牌1つ分の探索状態。1手目の打牌前も、仮想ツモを経た各段も同じ型で持ち、段ごとに別の
// 組み立てをしない。
//
// `seen` は手牌以外に見えている牌なので、手牌に加えた仮想ツモ牌は含まない。`discarded` は
// ここまでに切った物理牌で、将来テンパイ時点の自分の河として既存フリテン基盤へ渡される。
#[derive(Debug, Clone)]
struct HandState {
    counts: TileCounts,
    // 物理牌一覧。切る牌の赤 / 黒とドラ枚数を確定するために持つ。
    tiles: Vec<TileId>,
    seen: CandidateSeen,
    red_five_seen: [bool; TileType::COUNT],
    discarded: Vec<TileId>,
}

// 打牌候補1件について、その打牌後の牌を仮想ツモした次段の評価に必要な状態。
// 仮想ツモ牌ごとに作り直さず、詳細診断と選択用集計で共有する。
struct CandidateBranch {
    after_discard: TileCounts,
    next_tiles: Vec<TileId>,
    seen: CandidateSeen,
    // 打牌後の仮想ツモ候補を列挙するときの見え牌。打牌評価が受け入れを求めたときと同じ値で、
    // どちらの分類の枝も同じ基準の残枚数になる。
    draw_seen: [u8; TileType::COUNT],
    red_five_seen: [bool; TileType::COUNT],
    // 打牌前までの河へこの打牌を足したもの。次段の仮想手牌がそのまま引き継ぐ。
    discarded: Vec<TileId>,
}

impl CandidateBranch {
    // 打牌候補の牌種を手牌から除けない場合だけ `None`。
    fn new(state: &HandState, evaluation: &DiscardEvaluation) -> Option<Self> {
        let mut after_discard = state.counts;
        after_discard.remove(evaluation.discard).ok()?;

        // 実際に切られる物理牌を次段の物理牌一覧から外す。赤5と黒5の両方を持ち片方だけが合法な
        // 局面でも、打牌評価が確定した物理牌をそのまま引き継ぐ。
        let (discarded, next_tiles) = match split_discarded_tile(state.tiles.clone(), evaluation) {
            Some((discarded, remaining)) => (Some(discarded), remaining),
            None => (None, state.tiles.clone()),
        };

        Some(Self {
            after_discard,
            next_tiles,
            // 切った牌は次段では見え牌になる。
            seen: state.seen.after_discard(evaluation.discard),
            draw_seen: state.seen.additional_seen(evaluation.discard),
            red_five_seen: state.red_five_seen,
            discarded: state.discarded.iter().copied().chain(discarded).collect(),
        })
    }

    // 指定した分類の仮想ツモ枝を列挙して1段進める。
    //
    // 向聴数を下げる牌は打牌評価が持つ受け入れそのもので、対象牌も残枚数もツモ後向聴数も2手先
    // 評価のために求め直さない。3手目へ進む枝もこの入口を共有し、テンパイへ進む牌の判定を別に
    // 作らない。向聴数を維持する牌は受け入れと同じ列挙・同じ shanten calculator で求め、条件
    // だけが「維持する」になる。どちらの分類も現在打牌の受け入れを求めたときと同じ見え牌から
    // 残枚数を数える。
    fn draws(
        &self,
        inputs: &LookaheadInputs,
        evaluation: &DiscardEvaluation,
        scope: DrawScope,
    ) -> Vec<DrawLookaheadDiagnostic> {
        match scope {
            DrawScope::Progress => evaluation
                .acceptance_after_discard
                .tiles
                .iter()
                .map(|tile| self.draw(inputs, drawable(tile), scope))
                .collect(),
            DrawScope::SameShanten { .. } => same_shanten_draws_with_fixed_melds_and_seen(
                &self.after_discard,
                inputs.fixed_meld_count,
                &self.draw_seen,
            )
            .into_iter()
            .map(|drawable: DrawableTile| self.draw(inputs, drawable, scope))
            .collect(),
        }
    }

    // 仮想ツモ牌1牌種を、赤 / 黒の物理牌 variant ごとに仮想ツモして評価する。
    //
    // 赤5と黒5では打点だけでなく次の最良打牌そのものが変わり得るため、variant ごとに既存打牌
    // 評価と既存比較を通す。分割規則は最終和了牌と共有する既存 helper が持つ。
    fn draw(
        &self,
        inputs: &LookaheadInputs,
        drawable: DrawableTile,
        scope: DrawScope,
    ) -> DrawLookaheadDiagnostic {
        let variants = physical_tile_variants(
            drawable.tile,
            drawable.remaining,
            self.red_five_seen[drawable.tile.index()],
        )
        .map(|variant| self.draw_variant(inputs, drawable.tile, variant, scope))
        .collect();

        DrawLookaheadDiagnostic {
            draw: drawable.tile,
            remaining: drawable.remaining,
            shanten_after_draw: drawable.shanten_after_draw,
            transition: scope.transition(),
            variants,
        }
    }

    // 仮想ツモ牌の物理牌1つ分。仮想ツモ後の手牌に既存打牌評価・既存文脈反映・既存比較順を通して
    // 最良打牌を求める。
    fn draw_variant(
        &self,
        inputs: &LookaheadInputs,
        draw: TileType,
        variant: PhysicalTileVariant,
        scope: DrawScope,
    ) -> DrawVariantLookaheadDiagnostic {
        let Some(state) = self.state_after_draw(draw, variant.tile) else {
            return DrawVariantLookaheadDiagnostic {
                drawn_tile: variant.tile,
                remaining: variant.remaining,
                next_discard: None,
                prospective_value: None,
                tsumo_continuation: None,
                downstream: None,
            };
        };

        let next = next_discard(inputs, &state);
        DrawVariantLookaheadDiagnostic {
            drawn_tile: variant.tile,
            remaining: variant.remaining,
            // 先の段も同じ探索 primitive を、対象の枝を変えて呼ぶだけ。
            downstream: scope
                .continues_downstream()
                .then_some(next.evaluation.as_ref())
                .flatten()
                .and_then(|evaluation| search(inputs, &state, evaluation, PROGRESS_ONLY))
                .map(|draws| SameShantenDownstreamDiagnostic { draws }),
            next_discard: next.evaluation,
            prospective_value: next.prospective_value,
            tsumo_continuation: next.tsumo_continuation,
        }
    }

    // 仮想ツモ後の手牌の状態。ツモ牌は手牌へ入るので見え牌 (`seen`) へは加えない。赤5をツモった
    // 枝ではその牌種の赤5が以降見えているので、物理牌 variant の分割へ反映する。
    fn state_after_draw(&self, draw: TileType, drawn_tile: TileId) -> Option<HandState> {
        let mut counts = self.after_discard;
        counts.try_add(draw).ok()?;

        let mut tiles = self.next_tiles.clone();
        tiles.push(drawn_tile);

        let mut red_five_seen = self.red_five_seen;
        red_five_seen[draw.index()] |= drawn_tile.is_red();

        Some(HandState {
            counts,
            tiles,
            seen: self.seen,
            red_five_seen,
            discarded: self.discarded.clone(),
        })
    }
}

// 受け入れ牌を既存の仮想ツモ列挙と同じ形へ揃える。残枚数もツモ後向聴数も受け入れそのもので、
// 2手先評価のために求め直さない。
fn drawable(tile: &EffectiveAcceptanceTile) -> DrawableTile {
    DrawableTile {
        tile: tile.tile,
        remaining: tile.remaining,
        shanten_after_draw: tile.shanten_after_draw,
    }
}

// 仮想手牌1つ分の最良打牌を既存打牌評価・既存文脈反映・既存比較順で求める。仮想ツモ牌の物理牌が
// 決まっているため、赤5も通常打牌評価と同じ経路で反映できる。テンパイへ進む候補には将来打点を
// 付け、比較の打点軸へ渡す。
fn next_discard(inputs: &LookaheadInputs, state: &HandState) -> NextDiscard {
    let mut evaluations =
        evaluate_discards_with_seen(&state.counts, inputs.fixed_meld_count, &state.seen);
    decorate_evaluations(
        &mut evaluations,
        &state.counts,
        &DecorationContext {
            tiles: &state.tiles,
            dora_indicators: inputs.dora_indicators,
            round_wind: inputs.round_wind,
            seat_wind: inputs.seat_wind,
            shape_penalty: ShapePenaltyMode::WithContext {
                round_wind: inputs.round_wind,
                seat_wind: inputs.seat_wind,
                fixed_meld_count: inputs.fixed_meld_count,
            },
            unresolved_red_tile: None,
        },
    );

    let values: Vec<_> = evaluations
        .iter()
        .map(|evaluation| prospective_value(inputs, state, evaluation))
        .collect();
    let metrics: Vec<_> = values
        .iter()
        .map(|&value| ForwardMetrics::from_prospective_value(value))
        .collect();

    let Some(index) = best_discard_selection_index_with_forward_metrics(&evaluations, &metrics)
    else {
        return NextDiscard::default();
    };
    let evaluation = evaluations.swap_remove(index);
    NextDiscard {
        // ツモ continuation は選ばれた1件だけ求める。2手目の打牌比較は既存の打点軸のままで、
        // 候補ごとにツモ点数計算を走らせない。
        tsumo_continuation: tenpai_tsumo_value(inputs, state, &evaluation),
        prospective_value: values[index],
        evaluation: Some(evaluation),
    }
}

// 仮想手牌1つ分の最良打牌と、その打牌後のテンパイの将来打点。
#[derive(Default)]
struct NextDiscard {
    evaluation: Option<DiscardEvaluation>,
    prospective_value: Option<u64>,
    tsumo_continuation: Option<TenpaiTsumoValue>,
}

// 打牌候補1件分の将来打点。テンパイへ進む候補だけが値を持つ。
//
// 将来テンパイ時点の自分の河は「現在の自分の河 + ここまでに切った牌 + この打牌」で、既存
// フリテン基盤へそのまま渡す。フリテン判定はこの module が持たない。
fn prospective_value(
    inputs: &LookaheadInputs,
    state: &HandState,
    evaluation: &DiscardEvaluation,
) -> Option<u64> {
    let valuator = inputs.valuator?;
    with_prospective_tenpai(state, evaluation, |tenpai| valuator.tenpai_value(tenpai))
}

// 打牌候補1件分のツモ continuation。テンパイへ進む候補だけが値を持つ。
//
// 仮想テンパイの組み立てはロン baseline の将来打点と同じ helper を共有し、フリテン基盤も河の
// 組み立ても別に持たない。
fn tenpai_tsumo_value(
    inputs: &LookaheadInputs,
    state: &HandState,
    evaluation: &DiscardEvaluation,
) -> Option<TenpaiTsumoValue> {
    // 残り自摸機会が分からない局面では continuation を集計できないので、点数計算も行わない。
    inputs.self_tsumo_facts()?;
    let valuator = inputs.tsumo_valuator?;
    with_prospective_tenpai(state, evaluation, |tenpai| {
        valuator.tenpai_tsumo_value(tenpai)
    })
}

// 打牌後がテンパイの候補について、将来テンパイ1件分の入力を組み立てて評価器へ渡す。
//
// 将来テンパイ時点の自分の河は「現在の自分の河 + ここまでに切った牌 + この打牌」で、既存
// フリテン基盤へそのまま渡す。フリテン判定はこの module が持たない。
fn with_prospective_tenpai<T>(
    state: &HandState,
    evaluation: &DiscardEvaluation,
    evaluate: impl FnOnce(&ProspectiveTenpai<'_>) -> Option<T>,
) -> Option<T> {
    if evaluation.min_shanten_after_discard() != TENPAI_SHANTEN {
        return None;
    }

    let (discarded, concealed_tiles) = split_discarded_tile(state.tiles.clone(), evaluation)?;
    let discarded_tiles: Vec<_> = state.discarded.iter().copied().chain([discarded]).collect();

    evaluate(&ProspectiveTenpai {
        concealed_tiles: &concealed_tiles,
        acceptance: &evaluation.acceptance_after_discard,
        discarded_tiles: &discarded_tiles,
    })
}

// terminal tenpai へ到達した枝1つ分の期待支払い。
//
// テンパイへ到達しない枝は寄与 0 で、テンパイだがツモ打点を確定できない枝だけが `None`。
fn terminal_self_tsumo_value(
    variant: &DrawVariantLookaheadDiagnostic,
    facts: SelfTsumoFacts,
    path: impl FnOnce() -> Option<SelfTsumoPath>,
) -> Option<u64> {
    let Some(next) = variant.next_discard.as_ref() else {
        return Some(0);
    };
    if next.min_shanten_after_discard() != TENPAI_SHANTEN {
        return Some(0);
    }
    let terminal = variant.tsumo_continuation?;
    Some(path()?.expected_payment(facts, terminal))
}

// 手変わりの枝1つ分の期待支払い。2手目の打牌後の1向聴からもう1段進めたテンパイだけを集計する。
//
// 2回続けて向聴数を維持する枝はこの探索が持たないため、寄与 0 になる。先の枝を探索していない
// 場合は「この経路を評価していない」ので、寄与 0 ではなく確定しない値として扱う。
fn same_shanten_self_tsumo_value(
    variant: &DrawVariantLookaheadDiagnostic,
    facts: SelfTsumoFacts,
) -> Option<u64> {
    if variant.next_discard.is_none() {
        return Some(0);
    }

    let mut total = 0u64;
    for draw in &variant.downstream.as_ref()?.draws {
        for second in &draw.variants {
            let value = terminal_self_tsumo_value(second, facts, || {
                SelfTsumoPath::via_same_shanten(
                    variant.remaining,
                    second.remaining,
                    facts.unknown_tiles,
                )
            })?;
            total = total.saturating_add(value);
        }
    }
    Some(total)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::discard::{
        DiscardComparisonReason, diagnose_discard_evaluations_with_fixed_melds_and_forward_metrics,
        evaluate_discards_from_tiles_with_fixed_melds_and_context,
        evaluate_discards_from_tiles_with_fixed_melds_and_visible_tiles,
        select_best_discard_from_tiles_with_context,
        select_best_discard_from_tiles_with_visible_tiles,
    };
    use crate::tile::count_indicated_dora;
    use std::sync::LazyLock;

    // 打牌評価までを持つ検証用の局面。全候補分の2手先診断は重いので、1枝だけを見るテストは
    // この局面から必要な枝だけを構築する。
    struct Situation {
        tiles: Vec<TileId>,
        counts: TileCounts,
        visible: Vec<TileId>,
        dora_indicators: Vec<TileId>,
        round_wind: Option<TileType>,
        seat_wind: Option<TileType>,
        fixed_meld_count: FixedMeldCount,
        evaluations: Vec<DiscardEvaluation>,
    }

    // 局面と全候補分の2手先診断を1組にした検証用の case。2手先探索は重いので、同じ局面を使う
    // 複数のテストで構築結果を共有する。
    struct Case {
        situation: Situation,
        lookahead: LookaheadDiagnostic,
    }

    fn tile(s: &str) -> TileType {
        TileType::from_mjai_type_str(s).unwrap()
    }

    fn ids(values: &[u8]) -> Vec<TileId> {
        values.iter().map(|&v| TileId::new(v).unwrap()).collect()
    }

    fn fixed(value: u8) -> FixedMeldCount {
        FixedMeldCount::new(value).unwrap()
    }

    // 門前14枚 112233m 456p 78s 11z 2z。赤5を含まない物理牌で構成する。
    fn concealed_hand() -> Vec<TileId> {
        ids(&[0, 1, 4, 5, 8, 9, 48, 53, 57, 96, 100, 108, 109, 112])
    }

    // 手牌以外に見えている牌。1m 2枚・5p 1枚・W 1枚。
    fn public_visible_tiles() -> Vec<TileId> {
        ids(&[2, 3, 55, 116])
    }

    // 2副露済みとみなした concealed 7枚 + ツモ 9p の8枚。123m 12p 55s 9p。
    fn melded_hand() -> Vec<TileId> {
        ids(&[0, 4, 8, 36, 40, 89, 90, 68])
    }

    fn hand_only_situation(
        tiles: &[TileId],
        fixed_meld_count: FixedMeldCount,
        dora_indicators: Vec<TileId>,
        round_wind: Option<TileType>,
        seat_wind: Option<TileType>,
    ) -> Situation {
        let evaluations = evaluate_discards_from_tiles_with_fixed_melds_and_context(
            tiles,
            fixed_meld_count,
            &dora_indicators,
            round_wind,
            seat_wind,
        );
        Situation {
            tiles: tiles.to_vec(),
            counts: TileCounts::from_tiles(tiles.iter().copied()),
            visible: Vec::new(),
            dora_indicators,
            round_wind,
            seat_wind,
            fixed_meld_count,
            evaluations,
        }
    }

    fn visible_situation(
        tiles: &[TileId],
        fixed_meld_count: FixedMeldCount,
        dora_indicators: Vec<TileId>,
        round_wind: Option<TileType>,
        seat_wind: Option<TileType>,
        visible: Vec<TileId>,
    ) -> Situation {
        let evaluations = evaluate_discards_from_tiles_with_fixed_melds_and_visible_tiles(
            tiles,
            fixed_meld_count,
            &dora_indicators,
            round_wind,
            seat_wind,
            &visible,
        );
        Situation {
            tiles: tiles.to_vec(),
            counts: TileCounts::from_tiles(tiles.iter().copied()),
            visible,
            dora_indicators,
            round_wind,
            seat_wind,
            fixed_meld_count,
            evaluations,
        }
    }

    // 局面が持つ見え牌をそのまま反映した2手先評価の入力。
    fn inputs(situation: &Situation) -> LookaheadInputs<'_> {
        LookaheadInputs::new(
            &situation.tiles,
            situation.fixed_meld_count,
            &situation.dora_indicators,
            situation.round_wind,
            situation.seat_wind,
        )
        .with_visible_tiles(&situation.visible)
    }

    fn diagnose(situation: &Situation, evaluations: &[DiscardEvaluation]) -> LookaheadDiagnostic {
        diagnose_lookahead(&inputs(situation), evaluations)
    }

    // 指定牌種の黒牌。赤5の曖昧さを持ち込まない検証で仮想ツモ牌を明示するために使う。
    fn black(tile_type: TileType) -> TileId {
        TileId::copies(tile_type)
            .find(|tile| !tile.is_red())
            .expect("黒牌がある")
    }

    // 全候補・全受け入れ牌の物理牌 variant を平坦に並べる。
    fn variants(
        lookahead: &LookaheadDiagnostic,
    ) -> impl Iterator<
        Item = (
            TileType,
            &DrawLookaheadDiagnostic,
            &DrawVariantLookaheadDiagnostic,
        ),
    > {
        lookahead.candidates.iter().flat_map(|candidate| {
            candidate.draws.iter().flat_map(move |draw| {
                draw.variants
                    .iter()
                    .map(move |variant| (candidate.discard, draw, variant))
            })
        })
    }

    fn full_case(situation: Situation) -> Case {
        let lookahead = diagnose(&situation, &situation.evaluations);
        Case {
            situation,
            lookahead,
        }
    }

    fn hand_only_case(
        tiles: &[TileId],
        fixed_meld_count: FixedMeldCount,
        dora_indicators: Vec<TileId>,
        round_wind: Option<TileType>,
        seat_wind: Option<TileType>,
    ) -> Case {
        full_case(hand_only_situation(
            tiles,
            fixed_meld_count,
            dora_indicators,
            round_wind,
            seat_wind,
        ))
    }

    fn visible_case(
        tiles: &[TileId],
        fixed_meld_count: FixedMeldCount,
        dora_indicators: Vec<TileId>,
        round_wind: Option<TileType>,
        seat_wind: Option<TileType>,
        visible: Vec<TileId>,
    ) -> Case {
        full_case(visible_situation(
            tiles,
            fixed_meld_count,
            dora_indicators,
            round_wind,
            seat_wind,
            visible,
        ))
    }

    // 現在打牌1つ・受け入れ牌1枚だけへ絞った2手先の最良打牌。既存の受け入れから対象の1枚だけを
    // 残した打牌評価を production の2手先診断へ渡すので、全候補分を構築した場合の同じ枝と同じ
    // 結果になる。context-specific regression が full lookahead を作らないための test 専用 helper。
    fn branch_next_discard(
        situation: &Situation,
        discard: TileType,
        drawn_tile: TileId,
    ) -> DiscardEvaluation {
        let draw = drawn_tile.tile_type();
        let mut evaluation = situation
            .evaluations
            .iter()
            .find(|evaluation| evaluation.discard == discard)
            .expect("current discard evaluation exists")
            .clone();
        evaluation
            .acceptance_after_discard
            .tiles
            .retain(|accepted| accepted.tile == draw);
        assert_eq!(
            evaluation.acceptance_after_discard.tiles.len(),
            1,
            "打牌 {discard:?} の受け入れに {draw:?} が含まれている必要がある"
        );

        diagnose(situation, std::slice::from_ref(&evaluation))
            .candidate(discard)
            .and_then(|candidate| candidate.draw(draw))
            .and_then(|draw| draw.variant(drawn_tile))
            .and_then(|variant| variant.next_discard.clone())
            .expect("next discard exists")
    }

    static CONCEALED_HAND_ONLY: LazyLock<Case> = LazyLock::new(|| {
        hand_only_case(
            &concealed_hand(),
            FixedMeldCount::NONE,
            Vec::new(),
            None,
            None,
        )
    });

    static CONCEALED_WITH_VISIBLE: LazyLock<Case> = LazyLock::new(|| {
        let mut visible = concealed_hand();
        visible.extend(public_visible_tiles());
        visible_case(
            &concealed_hand(),
            FixedMeldCount::NONE,
            Vec::new(),
            None,
            None,
            visible,
        )
    });

    #[test]
    fn covers_every_current_discard_candidate() {
        let case = &*CONCEALED_HAND_ONLY;

        assert!(case.situation.evaluations.len() > 1);
        assert_eq!(
            case.lookahead.candidates.len(),
            case.situation.evaluations.len()
        );
        for (candidate, evaluation) in case
            .lookahead
            .candidates
            .iter()
            .zip(case.situation.evaluations.iter())
        {
            assert_eq!(candidate.discard, evaluation.discard);
            assert_eq!(
                case.lookahead
                    .candidate(evaluation.discard)
                    .map(|found| found.discard),
                Some(evaluation.discard)
            );
        }
    }

    #[test]
    fn draws_reuse_the_existing_acceptance_of_the_current_discard() {
        let case = &*CONCEALED_WITH_VISIBLE;

        for (candidate, evaluation) in case
            .lookahead
            .candidates
            .iter()
            .zip(case.situation.evaluations.iter())
        {
            let acceptance = &evaluation.acceptance_after_discard.tiles;
            let progress: Vec<_> = candidate.draws_with(DrawTransition::Progress).collect();
            assert_eq!(progress.len(), acceptance.len());
            for (draw, accepted) in progress.into_iter().zip(acceptance.iter()) {
                assert_eq!(draw.draw, accepted.tile);
                assert_eq!(draw.remaining, accepted.remaining);
                assert_eq!(draw.shanten_after_draw, accepted.shanten_after_draw);
                // 物理牌 variant の残枚数の合計は牌種単位の残枚数と一致する。
                assert_eq!(
                    draw.variants
                        .iter()
                        .map(|variant| u32::from(variant.remaining))
                        .sum::<u32>(),
                    u32::from(accepted.remaining),
                );
                assert!(
                    draw.variants
                        .iter()
                        .all(|variant| variant.drawn_tile.tile_type() == accepted.tile)
                );
            }
        }
    }

    // 仮想ツモ後の手牌を物理牌一覧として組み立てる。ツモ牌は検証対象の物理牌 variant そのもの。
    fn hypothetical_tiles(
        situation: &Situation,
        discard: TileType,
        drawn_tile: TileId,
    ) -> Vec<TileId> {
        let evaluation = situation
            .evaluations
            .iter()
            .find(|evaluation| evaluation.discard == discard)
            .expect("current discard evaluation exists");
        let (_, mut tiles) = split_discarded_tile(situation.tiles.clone(), evaluation)
            .expect("実際に切る物理牌がある");
        tiles.push(drawn_tile);
        tiles
    }

    // 仮想ツモ牌を見え牌へ足した visible tiles。1手目の打牌は手牌から消えても visible に残る。
    fn hypothetical_visible(situation: &Situation, drawn_tile: TileId) -> Vec<TileId> {
        let mut visible = situation.visible.clone();
        visible.push(drawn_tile);
        visible
    }

    // 2手目の最良打牌を、既存の context-aware 評価 API だけで求める。lookahead 側の期待値を
    // テスト内で再実装しないための共通 helper。
    fn expected_next_discard(
        situation: &Situation,
        discard: TileType,
        drawn_tile: TileId,
    ) -> Option<DiscardEvaluation> {
        let tiles = hypothetical_tiles(situation, discard, drawn_tile);

        if situation.visible.is_empty() {
            select_best_discard_from_tiles_with_context(
                &tiles,
                &situation.dora_indicators,
                situation.round_wind,
                situation.seat_wind,
            )
        } else {
            select_best_discard_from_tiles_with_visible_tiles(
                &tiles,
                &situation.dora_indicators,
                situation.round_wind,
                situation.seat_wind,
                &hypothetical_visible(situation, drawn_tile),
            )
        }
    }

    // 全候補 × 全受け入れ牌で、2手先の next discard が既存 context-aware 評価と一致することを
    // 確認する。戻り値は検証した件数。
    fn assert_next_discard_matches_existing_evaluation(case: &Case) -> usize {
        assert_eq!(case.situation.fixed_meld_count, FixedMeldCount::NONE);

        let mut checked = 0;
        for (discard, draw, variant) in variants(&case.lookahead) {
            assert_eq!(
                variant.next_discard,
                expected_next_discard(&case.situation, discard, variant.drawn_tile),
                "discard {:?} draw {:?} variant {:?}",
                discard,
                draw.draw,
                variant.drawn_tile,
            );
            checked += 1;
        }
        checked
    }

    #[test]
    fn next_discard_matches_the_existing_evaluation_and_comparator() {
        assert!(assert_next_discard_matches_existing_evaluation(&CONCEALED_WITH_VISIBLE) > 0);
    }

    // ---- context-aware な next discard ----

    // 役牌・通常ドラ検証用の門前14枚 123m456m789m 1p 55p S W。
    //
    // 孤立字牌 S と W だけが打牌候補として同格になり、場風・自風やドラ表示牌が無ければ
    // 比較は StableOrder まで落ちる。赤5は含まない。
    fn honor_choice_hand() -> Vec<TileId> {
        ids(&[0, 4, 8, 12, 17, 20, 24, 28, 32, 36, 53, 54, 112, 116])
    }

    fn honor_choice_situation(
        dora_indicators: Vec<TileId>,
        round_wind: Option<TileType>,
        seat_wind: Option<TileType>,
    ) -> Situation {
        let hand = honor_choice_hand();
        let mut visible = hand.clone();
        visible.extend(dora_indicators.iter().copied());
        visible_situation(
            &hand,
            FixedMeldCount::NONE,
            dora_indicators,
            round_wind,
            seat_wind,
            visible,
        )
    }

    // 東場南家。S が自風の役牌、W は無関係な客風。
    static VALUE_HONOR_SITUATION: LazyLock<Situation> =
        LazyLock::new(|| honor_choice_situation(Vec::new(), Some(tile("E")), Some(tile("S"))));

    // ドラ表示牌 E → ドラは S。赤5とは無関係に牌種だけで決まる通常ドラ。
    static NORMAL_DORA_SITUATION: LazyLock<Situation> =
        LazyLock::new(|| honor_choice_situation(ids(&[108]), None, None));

    // 場風・自風もドラ表示牌も持たない対照局面。役牌 / 通常ドラの両方の比較に使う。
    static HONOR_CHOICE_FREE_SITUATION: LazyLock<Situation> =
        LazyLock::new(|| honor_choice_situation(Vec::new(), None, None));

    // context の有無で next discard が変わる枝。1p を切って 5p をツモると
    // 123m456m789m 555p S W になり、2手目は孤立字牌 S と W のどちらを切るかだけになる。
    // 場風・自風もドラ表示牌も無ければ S を切り、S が自風の役牌になる場でも S がドラになる場でも
    // S を残して W へ変わる。
    fn honor_choice_branch() -> (TileType, TileId) {
        (tile("1p"), black(tile("5p")))
    }

    #[test]
    fn next_discard_reflects_value_honor_context() {
        let situation = &*VALUE_HONOR_SITUATION;
        let (discard, drawn_tile) = honor_choice_branch();

        let next = branch_next_discard(situation, discard, drawn_tile);
        assert_eq!(
            Some(&next),
            expected_next_discard(situation, discard, drawn_tile).as_ref(),
        );

        // 役牌は牌種と場風・自風だけで決まるので、赤5の曖昧さとは無関係に必ず反映される。
        assert_eq!(
            next.discarded_value_honor_count,
            u8::from(
                next.discard
                    .is_value_honor(situation.round_wind, situation.seat_wind)
            ),
        );
    }

    #[test]
    fn value_honor_context_changes_the_next_discard() {
        // context-free では S を切る枝が、役牌保護によって W へ変わることを固定する。
        let (discard, drawn_tile) = honor_choice_branch();

        let context_free = branch_next_discard(&HONOR_CHOICE_FREE_SITUATION, discard, drawn_tile);
        let with_context = branch_next_discard(&VALUE_HONOR_SITUATION, discard, drawn_tile);

        assert_ne!(
            with_context.discard, context_free.discard,
            "役牌 context が next discard を変える枝である必要がある"
        );
        assert!(
            context_free.discard.is_value_honor(
                VALUE_HONOR_SITUATION.round_wind,
                VALUE_HONOR_SITUATION.seat_wind
            ),
            "役牌 context で守られる牌が context-free では切られる枝である必要がある"
        );
    }

    #[test]
    fn next_discard_reflects_normal_dora_context() {
        let situation = &*NORMAL_DORA_SITUATION;
        let (discard, drawn_tile) = honor_choice_branch();

        let next = branch_next_discard(situation, discard, drawn_tile);
        assert_eq!(
            Some(&next),
            expected_next_discard(situation, discard, drawn_tile).as_ref(),
        );

        // 通常ドラは牌種から決まるので、仮想ツモ牌を切る候補でも 0 に潰さない。
        assert_eq!(
            next.discarded_dora_count,
            count_indicated_dora(next.discard, &situation.dora_indicators)
                + u8::from(next.discards_red_five),
        );
    }

    #[test]
    fn normal_dora_context_changes_the_next_discard() {
        // context-free では S を切る枝が、通常ドラ保護によって W へ変わることを固定する。
        let (discard, drawn_tile) = honor_choice_branch();

        let context_free = branch_next_discard(&HONOR_CHOICE_FREE_SITUATION, discard, drawn_tile);
        let with_context = branch_next_discard(&NORMAL_DORA_SITUATION, discard, drawn_tile);

        assert_ne!(
            with_context.discard, context_free.discard,
            "通常ドラ context が next discard を変える枝である必要がある"
        );
        assert_eq!(
            count_indicated_dora(context_free.discard, &NORMAL_DORA_SITUATION.dora_indicators),
            1,
            "通常ドラ context で守られる牌が context-free では切られる枝である必要がある"
        );
    }

    // 仮想ツモ牌をそのまま2手目に切る局面を含む case。
    //
    // 門前14枚 112233m 456p 78s EE S で S を切ると 9s が受け入れになり、9s を引いた後の最良打牌は
    // その 9s になる。ドラ表示牌 8s でドラは 9s なので、仮想ツモ牌を切る候補でも通常ドラが
    // 反映されることを確認できる。
    static DRAWN_TILE_DISCARD_CASE: LazyLock<Case> = LazyLock::new(|| {
        let hand = concealed_hand();
        let dora_indicators = ids(&[101]);
        let mut visible = hand.clone();
        visible.extend(dora_indicators.iter().copied());
        visible_case(
            &hand,
            FixedMeldCount::NONE,
            dora_indicators,
            None,
            None,
            visible,
        )
    });

    #[test]
    fn drawn_tile_discard_reflects_the_physical_tile() {
        // 仮想ツモ牌を2手目に切る候補では、牌種から決まる通常ドラも物理牌から決まる赤ドラも
        // 通常打牌評価と同じ経路で反映する。
        let case = &*DRAWN_TILE_DISCARD_CASE;

        let mut checked = 0;
        for (_, draw, variant) in variants(&case.lookahead) {
            let next = variant.next_discard.as_ref().expect("next discard exists");
            if next.discard != draw.draw {
                continue;
            }
            assert_eq!(
                next.discarded_dora_count,
                count_indicated_dora(next.discard, &case.situation.dora_indicators)
                    + u8::from(next.discards_red_five),
            );
            checked += 1;
        }
        assert!(checked > 0, "仮想ツモ牌を2手目に切る候補が必要");
    }

    #[test]
    fn drawn_dora_tile_discard_keeps_the_indicated_dora_count() {
        // ドラそのものを仮想ツモしてそのまま切る候補で、通常ドラを 0 に潰していないことを固定する。
        let case = &*DRAWN_TILE_DISCARD_CASE;
        let dora = tile("9s");

        let hit = variants(&case.lookahead)
            .filter(|(_, draw, _)| draw.draw == dora)
            .filter_map(|(_, _, variant)| variant.next_discard.as_ref())
            .find(|next| next.discard == dora)
            .expect("ドラを仮想ツモしてそのまま切る候補が必要");

        assert_eq!(hit.discarded_dora_count, 1);
        assert!(!hit.discards_red_five);
    }

    // ---- seen の扱い ----

    // 2手目の受け入れ残枚数が、期待する seen 集合から計算した値と一致することを全候補で確認する。
    //
    // `public_visible` は手牌以外に見えている枚数、`counts_candidate_discard` は2手目の打牌候補を
    // seen に数えるかどうか。1手目の打牌はどちらの経路でも seen に数える。
    // 戻り値は「1手目に切った牌が2手目の受け入れに現れた回数」で、山への復活検証が効いた件数。
    fn assert_lookahead_remaining(
        case: &Case,
        public_visible: &[(TileType, u8)],
        counts_candidate_discard: bool,
    ) -> usize {
        let public_visible_count = |tile: TileType| -> u8 {
            public_visible
                .iter()
                .find(|(seen, _)| *seen == tile)
                .map(|(_, count)| *count)
                .unwrap_or(0)
        };

        let mut first_discard_hits = 0;
        for (discard, draw, variant) in variants(&case.lookahead) {
            let next = variant.next_discard.as_ref().expect("next discard exists");

            let mut after_next = case.situation.counts;
            after_next.remove(discard).unwrap();
            after_next.try_add(draw.draw).unwrap();
            after_next.remove(next.discard).unwrap();

            for accepted in &next.acceptance_after_discard.tiles {
                let seen = after_next.count(accepted.tile)
                    + public_visible_count(accepted.tile)
                    + u8::from(accepted.tile == discard)
                    + u8::from(counts_candidate_discard && accepted.tile == next.discard);
                assert_eq!(
                    accepted.remaining,
                    4u8.saturating_sub(seen),
                    "discard {discard:?} draw {:?} next {:?} tile {:?}",
                    draw.draw,
                    next.discard,
                    accepted.tile,
                );
                if accepted.tile == discard {
                    first_discard_hits += 1;
                }
            }
        }
        first_discard_hits
    }

    #[test]
    fn first_discard_stays_seen_without_visible_tiles() {
        // visible tiles が無い経路では2手目の打牌候補を seen に数えない既存 semantics を保ちつつ、
        // 1手目に切った牌だけは見え牌として残す。
        let hits = assert_lookahead_remaining(&CONCEALED_HAND_ONLY, &[], false);
        assert!(hits > 0, "1手目の打牌が2手目の受け入れに現れる局面が必要");
    }

    #[test]
    fn first_discard_stays_seen_with_visible_tiles() {
        let public_visible = [(tile("1m"), 2), (tile("5p"), 1), (tile("W"), 1)];
        let hits = assert_lookahead_remaining(&CONCEALED_WITH_VISIBLE, &public_visible, true);
        assert!(hits > 0, "1手目の打牌が2手目の受け入れに現れる局面が必要");
    }

    #[test]
    fn does_not_double_count_the_own_hand_in_visible_tiles() {
        // visible tiles が自分の手牌そのものだけなら、手牌以外に見えている牌は無い扱いになる。
        let hand = melded_hand();
        let case = visible_case(&hand, fixed(2), Vec::new(), None, None, hand.clone());

        let hits = assert_lookahead_remaining(&case, &[], true);
        assert!(hits > 0, "1手目の打牌が2手目の受け入れに現れる局面が必要");
    }

    #[test]
    fn empty_visible_tiles_match_the_fixed_meld_entry() {
        let case = hand_only_case(&melded_hand(), fixed(2), Vec::new(), None, None);

        assert_eq!(
            diagnose_lookahead(
                &LookaheadInputs::new(&case.situation.tiles, fixed(2), &[], None, None)
                    .with_visible_tiles(&[]),
                &case.situation.evaluations,
            ),
            case.lookahead,
        );
    }

    #[test]
    fn fixed_melds_keep_the_existing_effective_shanten() {
        let case = hand_only_case(&melded_hand(), fixed(2), Vec::new(), None, None);

        let mut checked = 0;
        for (_, draw, variant) in variants(&case.lookahead) {
            // 副露手では七対子・国士を復活させず、通常形のみの effective shanten になる。
            assert_eq!(draw.shanten_after_draw.concealed(), None);
            let next = variant.next_discard.as_ref().expect("next discard exists");
            assert_eq!(next.shanten_after_discard.concealed(), None);
            assert_eq!(
                next.standard_iishanten_shape_after_discard,
                IishantenShape::Unknown
            );
            for accepted in &next.acceptance_after_discard.tiles {
                assert_eq!(accepted.shanten_after_draw.concealed(), None);
            }
            checked += 1;
        }
        assert!(checked > 0);
    }

    // ---- 打牌選択用の weighted tenpai wait ----

    // 選択用集計のうち、既存 weighted tenpai wait だけを取り出す。
    fn metrics(situation: &Situation) -> Vec<Option<TenpaiWaitMetric>> {
        forward_metrics(&inputs(situation), &situation.evaluations)
            .into_iter()
            .map(|metric| metric.tenpai_wait)
            .collect()
    }

    // 集計 helper を使わずに Σ(受け入れ残枚数 × テンパイ後の和了牌残枚数) を組み立てる。
    // 期待値を診断の生の値から作ることで、集計規則そのものを固定する。
    fn expected_metric(candidate: &DiscardLookaheadDiagnostic) -> TenpaiWaitMetric {
        let mut expected = TenpaiWaitMetric::default();
        for variant in candidate.draws.iter().flat_map(|draw| draw.variants.iter()) {
            let Some(next) = variant.next_discard.as_ref() else {
                continue;
            };
            if next.min_shanten_after_discard() != TENPAI_SHANTEN {
                continue;
            }
            expected.weighted_remaining +=
                u32::from(variant.remaining) * u32::from(next.acceptance_total_remaining());
            expected.weighted_type_count +=
                u32::from(variant.remaining) * next.acceptance_type_count() as u32;
        }
        expected
    }

    // 1向聴を維持する打牌候補が複数ある門前14枚 12m 68m 444p 5p 789p 567s。
    // 打 5p は受け入れが最も広く、打 1m / 2m は 45p の両面を残してテンパイ後の待ちが広くなる。
    fn iishanten_wait_hand() -> Vec<TileId> {
        ids(&[0, 4, 20, 28, 48, 49, 50, 53, 60, 64, 68, 89, 92, 96])
    }

    static IISHANTEN_WAIT_CASE: LazyLock<Case> = LazyLock::new(|| {
        hand_only_case(
            &iishanten_wait_hand(),
            FixedMeldCount::NONE,
            Vec::new(),
            None,
            None,
        )
    });

    // 手牌以外に 3p 3枚・6p 3枚が見えている同じ局面。テンパイ後の待ちが実際に減る。
    static IISHANTEN_WAIT_WITH_VISIBLE: LazyLock<Case> = LazyLock::new(|| {
        let hand = iishanten_wait_hand();
        let mut visible = hand.clone();
        visible.extend(ids(&[44, 45, 46, 56, 57, 58]));
        visible_case(&hand, FixedMeldCount::NONE, Vec::new(), None, None, visible)
    });

    #[test]
    fn weighted_wait_is_computed_for_every_iishanten_candidate() {
        let case = &*IISHANTEN_WAIT_CASE;
        let metrics = metrics(&case.situation);

        assert_eq!(metrics.len(), case.situation.evaluations.len());
        let mut iishanten = 0;
        for (evaluation, metric) in case.situation.evaluations.iter().zip(metrics.iter()) {
            if evaluation.min_shanten_after_discard() == 1 {
                assert!(metric.is_some(), "1向聴候補 {:?}", evaluation.discard);
                iishanten += 1;
            } else {
                // 最善向聴を維持しない候補は前方評価の対象外なので None のままにする。
                assert_eq!(*metric, None, "非1向聴候補 {:?}", evaluation.discard);
            }
        }
        assert!(iishanten > 1, "1向聴候補が複数ある局面が必要");
    }

    #[test]
    fn a_single_candidate_metric_matches_the_full_comparison() {
        // 1候補だけの入口は全候補経路と同じ集計値を返す。別の計算器を持たない。
        let case = &*IISHANTEN_WAIT_CASE;
        let all = forward_metrics(&inputs(&case.situation), &case.situation.evaluations);

        let mut checked = 0;
        for (evaluation, metric) in case.situation.evaluations.iter().zip(all.iter()) {
            if metric.tenpai_wait.is_none() {
                continue;
            }
            assert_eq!(
                forward_metrics_for_candidate(&inputs(&case.situation), evaluation),
                *metric,
                "{:?}",
                evaluation.discard
            );
            checked += 1;
        }
        assert!(checked > 1, "前方評価の対象候補が複数ある局面が必要");
    }

    #[test]
    fn weighted_wait_aggregates_the_branch_evaluations() {
        let case = &*IISHANTEN_WAIT_CASE;
        let metrics = metrics(&case.situation);

        let mut checked = 0;
        for (candidate, metric) in case.lookahead.candidates.iter().zip(metrics.iter()) {
            let Some(metric) = metric else {
                continue;
            };
            assert_eq!(
                *metric,
                expected_metric(candidate),
                "{:?}",
                candidate.discard
            );
            checked += 1;
        }
        assert!(checked > 1);
    }

    #[test]
    fn weighted_wait_matches_the_detailed_lookahead() {
        // 詳細診断から集計しても選択専用経路と同じ値になる。同じ枝を2回計算する必要はない。
        let case = &*IISHANTEN_WAIT_CASE;

        assert_eq!(
            tenpai_wait_metrics_from_lookahead(
                &inputs(&case.situation),
                &case.situation.evaluations,
                &case.lookahead,
            ),
            metrics(&case.situation),
        );
    }

    #[test]
    fn weighted_wait_prefers_the_wider_tenpai_over_the_wider_acceptance() {
        // 受け入れが最も広い打牌より、テンパイ後の待ちが広い打牌の方が weighted wait が大きい。
        let case = &*IISHANTEN_WAIT_CASE;
        let metrics = metrics(&case.situation);

        let metric_of = |discard: TileType| {
            case.situation
                .evaluations
                .iter()
                .position(|evaluation| evaluation.discard == discard)
                .and_then(|index| metrics[index])
                .expect("1向聴候補の集計値がある")
        };
        let acceptance_of = |discard: TileType| {
            case.situation
                .evaluations
                .iter()
                .find(|evaluation| evaluation.discard == discard)
                .map(DiscardEvaluation::acceptance_total_remaining)
                .expect("打牌候補がある")
        };

        assert!(acceptance_of(tile("5p")) > acceptance_of(tile("1m")));
        assert!(
            metric_of(tile("1m")).weighted_remaining > metric_of(tile("5p")).weighted_remaining
        );
    }

    #[test]
    fn tenpai_hands_do_not_compute_the_weighted_wait() {
        // 最善向聴数がテンパイの局面では前方評価を計算しない。
        let situation = hand_only_situation(
            &ids(&[0, 4, 8, 12, 17, 20, 24, 28, 32, 36, 40, 89, 90, 68]),
            FixedMeldCount::NONE,
            Vec::new(),
            None,
            None,
        );
        assert_eq!(
            situation
                .evaluations
                .iter()
                .map(DiscardEvaluation::min_shanten_after_discard)
                .min(),
            Some(0)
        );

        assert!(metrics(&situation).iter().all(Option::is_none));
    }

    #[test]
    fn multi_shanten_hands_do_not_compute_the_weighted_wait() {
        // 2向聴・3向聴以上でも前方評価そのものは行うが、その結果は weighted next acceptance へ
        // 入る。weighted tenpai wait は1向聴限定なので、どの候補も持たない。
        for hand in [
            ids(&[0, 4, 8, 12, 17, 20, 48, 53, 72, 76, 108, 112, 116, 120]),
            ids(&[0, 8, 20, 28, 48, 56, 68, 76, 88, 100, 108, 116, 124, 132]),
        ] {
            let situation =
                hand_only_situation(&hand, FixedMeldCount::NONE, Vec::new(), None, None);
            let best = situation
                .evaluations
                .iter()
                .map(DiscardEvaluation::min_shanten_after_discard)
                .min()
                .expect("打牌候補がある");
            assert!(best >= 2, "2向聴以上の局面が必要");

            assert!(metrics(&situation).iter().all(Option::is_none));
        }
    }

    #[test]
    fn a_single_iishanten_candidate_does_not_compute_the_weighted_wait() {
        // 1向聴を維持する候補が1件だけなら Shanten 比較で決着するので前方評価は不要。
        let case = &*IISHANTEN_WAIT_CASE;
        let single: Vec<_> = case
            .situation
            .evaluations
            .iter()
            .filter(|evaluation| evaluation.min_shanten_after_discard() != 1)
            .cloned()
            .chain(
                case.situation
                    .evaluations
                    .iter()
                    .find(|evaluation| evaluation.min_shanten_after_discard() == 1)
                    .cloned(),
            )
            .collect();

        let metrics = forward_metrics(
            &LookaheadInputs::new(&case.situation.tiles, FixedMeldCount::NONE, &[], None, None),
            &single,
        );
        assert!(metrics.iter().all(|metric| metric.tenpai_wait.is_none()));
    }

    #[test]
    fn visible_tiles_reduce_the_weighted_wait() {
        // テンパイ後の待ち牌が他家に見えている分だけ、weighted wait が実際に減る。
        let hand_only = metrics(&IISHANTEN_WAIT_CASE.situation);
        let with_visible = metrics(&IISHANTEN_WAIT_WITH_VISIBLE.situation);

        let mut reduced = 0;
        for (without, with) in hand_only.iter().zip(with_visible.iter()) {
            let (Some(without), Some(with)) = (without, with) else {
                continue;
            };
            assert!(with.weighted_remaining <= without.weighted_remaining);
            if with.weighted_remaining < without.weighted_remaining {
                reduced += 1;
            }
        }
        assert!(reduced > 0, "見え牌で待ちが減る候補が必要");
    }

    #[test]
    fn dead_wait_tenpai_branches_contribute_zero() {
        // 待ちがすべて見えているテンパイへ進む枝は寄与 0。計算していない None とは区別する。
        //
        // 123m456m789m 99s E S + ツモ W。E を切って S を引くと 3面子 + 99s + SS + W になり、
        // 2手目は W を切って 9s / S のシャンポン待ちテンパイになる。9s と S を残り全部見え牌に
        // しておくと、そのテンパイの和了牌は1枚も残らない。
        let hand = ids(&[0, 4, 8, 12, 17, 20, 24, 28, 32, 104, 105, 108, 112]);
        let mut tiles = hand.clone();
        tiles.push(ids(&[116])[0]);
        let mut visible = tiles.clone();
        visible.extend(ids(&[106, 107, 113, 114]));
        let case = visible_case(
            &tiles,
            FixedMeldCount::NONE,
            Vec::new(),
            None,
            None,
            visible,
        );
        let metrics = metrics(&case.situation);

        let mut dead = 0;
        for (candidate, metric) in case.lookahead.candidates.iter().zip(metrics.iter()) {
            let Some(metric) = metric else {
                continue;
            };
            for variant in candidate.draws.iter().flat_map(|draw| draw.variants.iter()) {
                let Some(next) = variant.next_discard.as_ref() else {
                    continue;
                };
                if next.min_shanten_after_discard() == TENPAI_SHANTEN
                    && next.acceptance_total_remaining() == 0
                {
                    dead += 1;
                }
            }
            assert_eq!(*metric, expected_metric(candidate));
        }
        assert!(dead > 0, "死にテンへ進む枝がある局面が必要");
    }

    #[test]
    fn discarded_tiles_are_not_returned_to_the_wall() {
        // 集計対象の局面でも、1手目・2手目に切った牌を山へ戻さない既存 seen 扱いを維持する。
        // 残枚数の検証は既存の枝と同じ helper を使い、集計はその残枚数から組み立てる。
        let _ = assert_lookahead_remaining(&IISHANTEN_WAIT_CASE, &[], false);
        let _ = assert_lookahead_remaining(
            &IISHANTEN_WAIT_WITH_VISIBLE,
            &[(tile("3p"), 3), (tile("6p"), 3)],
            true,
        );
    }

    #[test]
    fn fixed_melds_keep_the_effective_shanten_semantics_in_the_weighted_wait() {
        // 副露済み手牌でも既存 EffectiveShanten のまま集計し、詳細診断と同じ値になる。
        let hand = ids(&[0, 4, 8, 36, 40, 60, 64, 89]);
        let case = hand_only_case(&hand, fixed(2), Vec::new(), None, None);

        let metrics = metrics(&case.situation);
        assert!(metrics.iter().any(Option::is_some), "1向聴候補が必要");
        assert_eq!(
            metrics,
            tenpai_wait_metrics_from_lookahead(
                &inputs(&case.situation),
                &case.situation.evaluations,
                &case.lookahead,
            ),
        );

        for (candidate, metric) in case.lookahead.candidates.iter().zip(metrics.iter()) {
            if metric.is_none() {
                continue;
            }
            for draw in &candidate.draws {
                assert_eq!(draw.shanten_after_draw.concealed(), None);
                for variant in &draw.variants {
                    let next = variant.next_discard.as_ref().expect("next discard exists");
                    assert_eq!(next.shanten_after_discard.concealed(), None);
                }
            }
        }
    }

    #[test]
    fn red_five_handling_matches_the_detailed_lookahead() {
        // 赤5を含む物理牌でも、選択用集計は詳細診断と同じ枝評価を共有する。
        let mut hand = iishanten_wait_hand();
        // 黒5s を赤5s へ置き換える。
        let position = hand.iter().position(|tile| *tile == ids(&[89])[0]).unwrap();
        hand[position] = ids(&[88])[0];
        let case = hand_only_case(&hand, FixedMeldCount::NONE, Vec::new(), None, None);

        assert!(hand.iter().any(|tile| tile.is_red()));
        let metrics = metrics(&case.situation);
        assert!(metrics.iter().any(Option::is_some));
        assert_eq!(
            metrics,
            tenpai_wait_metrics_from_lookahead(
                &inputs(&case.situation),
                &case.situation.evaluations,
                &case.lookahead,
            ),
        );
    }

    #[test]
    fn empty_visible_tiles_match_the_fixed_meld_weighted_wait() {
        let case = &*IISHANTEN_WAIT_CASE;

        assert_eq!(
            forward_metrics(
                &LookaheadInputs::new(&case.situation.tiles, FixedMeldCount::NONE, &[], None, None)
                    .with_visible_tiles(&[]),
                &case.situation.evaluations,
            )
            .into_iter()
            .map(|metric| metric.tenpai_wait)
            .collect::<Vec<_>>(),
            metrics(&case.situation),
        );
    }

    #[test]
    fn weighted_wait_from_a_mismatched_lookahead_is_absent() {
        // 候補集合と対応しない診断を渡された場合は推測せず None にする。
        let case = &*IISHANTEN_WAIT_CASE;

        assert!(
            tenpai_wait_metrics_from_lookahead(
                &inputs(&case.situation),
                &case.situation.evaluations,
                &LookaheadDiagnostic::default(),
            )
            .iter()
            .all(Option::is_none)
        );
    }

    #[test]
    fn accessors_read_the_stored_next_evaluation() {
        let draw = &CONCEALED_HAND_ONLY.lookahead.candidates[0].draws[0];
        let variant = &draw.variants[0];
        let next = variant.next_discard.as_ref().unwrap();

        assert_eq!(draw.variant(variant.drawn_tile), Some(variant));
        assert_eq!(variant.next_discard_tile(), Some(next.discard));
        assert_eq!(
            variant.next_min_shanten(),
            Some(next.min_shanten_after_discard())
        );
        assert_eq!(
            variant.next_acceptance_total_remaining(),
            Some(next.acceptance_total_remaining())
        );
        assert_eq!(
            variant.next_acceptance_type_count(),
            Some(next.acceptance_type_count())
        );
        assert_eq!(
            variant.next_standard_iishanten_shape(),
            Some(next.standard_iishanten_shape_after_discard)
        );
    }

    #[test]
    fn does_not_modify_the_input_tiles() {
        let tiles = melded_hand();
        let before = tiles.clone();
        let evaluations = evaluate_discards_from_tiles_with_fixed_melds_and_context(
            &tiles,
            fixed(2),
            &[],
            None,
            None,
        );
        let _ = diagnose_lookahead(
            &LookaheadInputs::new(&tiles, fixed(2), &[], None, None),
            &evaluations,
        );
        assert_eq!(tiles, before);
    }

    // ---- 2向聴以上の仮想ツモ牌の物理牌 variant ----

    // 2345m 1p 3p 7p 2s 7s 999s CC の2向聴。打 7p / 打 2s が最善向聴を維持し、どちらの枝でも
    // 赤5が未見の 5m / 5p / 5s を仮想ツモする。5m は手牌に黒5m があるので残枚数 3、5p / 5s は
    // 手牌にも見え牌にも無いので残枚数 4 になる。
    fn two_shanten_red_five_hand() -> Vec<TileId> {
        ids(&[4, 11, 13, 17, 39, 45, 62, 79, 97, 104, 106, 107, 133, 135])
    }

    static TWO_SHANTEN_RED_FIVE_CASE: LazyLock<Case> = LazyLock::new(|| {
        let hand = two_shanten_red_five_hand();
        let visible = hand.clone();
        visible_case(&hand, FixedMeldCount::NONE, Vec::new(), None, None, visible)
    });

    fn best_shanten(situation: &Situation) -> i8 {
        situation
            .evaluations
            .iter()
            .map(DiscardEvaluation::min_shanten_after_discard)
            .min()
            .expect("打牌候補がある")
    }

    #[test]
    fn a_two_or_more_shanten_draw_splits_into_physical_variants() {
        // 2向聴以上でも仮想ツモ牌を赤5 / 黒5の物理牌 variant へ分ける。分割規則は最終和了牌と
        // 共有する既存 helper が source of truth で、テスト側で赤黒の枚数を数え直さない。
        let case = &*TWO_SHANTEN_RED_FIVE_CASE;
        assert!(best_shanten(&case.situation) >= 2, "2向聴以上の局面が必要");

        let red_five_seen = seen_red_fives(case.situation.visible.iter().copied());
        let mut split_draws = 0;
        for candidate in &case.lookahead.candidates {
            for draw in &candidate.draws {
                let expected: Vec<_> = physical_tile_variants(
                    draw.draw,
                    draw.remaining,
                    red_five_seen[draw.draw.index()],
                )
                .map(|variant| (variant.tile, variant.remaining))
                .collect();
                let actual: Vec<_> = draw
                    .variants
                    .iter()
                    .map(|variant| (variant.drawn_tile, variant.remaining))
                    .collect();
                assert_eq!(
                    actual, expected,
                    "discard {:?} draw {:?}",
                    candidate.discard, draw.draw
                );

                // 物理牌 variant の残枚数の合計は牌種単位の残枚数と一致する。
                assert_eq!(
                    draw.variants
                        .iter()
                        .map(|variant| u32::from(variant.remaining))
                        .sum::<u32>(),
                    u32::from(draw.remaining),
                );

                let [red, black] = draw.variants.as_slice() else {
                    continue;
                };
                assert!(red.drawn_tile.is_red());
                assert!(!black.drawn_tile.is_red());
                assert_eq!(red.remaining, 1);
                assert_eq!(black.remaining, draw.remaining - 1);
                split_draws += 1;
            }
        }
        assert!(split_draws > 0, "赤5が未見の受け入れがある局面が必要");
    }

    #[test]
    fn red_and_black_variants_share_the_next_discard_without_a_prospective_value() {
        // 2向聴以上では将来打点を使わないため、赤5 / 黒5の違いは既存 comparator の結果を変えない。
        //
        // 仮想ツモ牌を2手目にそのまま切ると向聴が戻るので Shanten 軸で必ず負け、赤5かどうかが
        // 効く Dora / RedFive 軸まで落ちない。したがって物理牌 variant が変えるのは残枚数の
        // 内訳だけで、weighted next acceptance の合計は牌種単位で数えた場合と一致する。
        let case = &*TWO_SHANTEN_RED_FIVE_CASE;

        let mut checked = 0;
        for candidate in &case.lookahead.candidates {
            // 向聴数を維持する枝では仮想ツモ牌をそのまま切っても向聴が戻らないため、赤5を切るか
            // どうかを見る既存の軸まで比較が進み得る。この不変条件は向聴数を下げる枝のもの。
            for draw in candidate.draws_with(DrawTransition::Progress) {
                let [red, black] = draw.variants.as_slice() else {
                    continue;
                };
                assert_eq!(
                    red.next_discard, black.next_discard,
                    "discard {:?} draw {:?}",
                    candidate.discard, draw.draw
                );
                assert_eq!(red.prospective_value, None);
                assert_eq!(black.prospective_value, None);
                checked += 1;
            }
        }
        assert!(checked > 0, "赤 / 黒へ分かれる受け入れがある局面が必要");
    }

    #[test]
    fn weighted_next_acceptance_aggregates_the_physical_variants() {
        // 2手目の結果は物理牌 variant の残枚数で重み付けして集約する。平均や確率へ正規化しない。
        let case = &*TWO_SHANTEN_RED_FIVE_CASE;
        let required_next_shanten = best_shanten(&case.situation) - 1;
        let metrics = forward_metrics(&inputs(&case.situation), &case.situation.evaluations);

        let mut checked = 0;
        for (candidate, metric) in case.lookahead.candidates.iter().zip(metrics.iter()) {
            let Some(next_acceptance) = metric.next_acceptance else {
                continue;
            };

            let mut weighted_remaining = 0u32;
            let mut weighted_type_count = 0u32;
            for variant in candidate.draws.iter().flat_map(|draw| draw.variants.iter()) {
                let Some(next) = variant.next_discard.as_ref() else {
                    continue;
                };
                if next.min_shanten_after_discard() != required_next_shanten {
                    continue;
                }
                weighted_remaining +=
                    u32::from(variant.remaining) * u32::from(next.acceptance_total_remaining());
                weighted_type_count +=
                    u32::from(variant.remaining) * next.acceptance_type_count() as u32;
            }

            assert_eq!(next_acceptance.weighted_remaining, weighted_remaining);
            assert_eq!(next_acceptance.weighted_type_count, weighted_type_count);
            // 2向聴以上では weighted tenpai wait の枠は使わない。
            assert_eq!(metric.tenpai_wait, None);
            checked += 1;
        }
        assert!(checked > 1, "前方評価の対象候補が複数必要");
    }

    #[test]
    fn two_or_more_shanten_decides_by_the_weighted_next_acceptance_only() {
        // 2向聴以上では打点込みの集計値を持たず、既存 weighted next acceptance だけで決着する。
        let case = &*TWO_SHANTEN_RED_FIVE_CASE;
        let metrics = forward_metrics(&inputs(&case.situation), &case.situation.evaluations);

        assert!(
            metrics
                .iter()
                .all(|metric| metric.prospective_value.is_none())
        );
        assert!(
            metrics
                .iter()
                .any(|metric| metric.next_acceptance.is_some())
        );

        let diagnostic = diagnose_discard_evaluations_with_fixed_melds_and_forward_metrics(
            &case.situation.counts,
            case.situation.fixed_meld_count,
            &case.situation.evaluations,
            &metrics,
        );
        let candidate = |discard: TileType| {
            diagnostic
                .candidates
                .iter()
                .find(|candidate| candidate.evaluation.discard == discard)
                .expect("打牌候補がある")
        };

        assert_eq!(
            diagnostic
                .selected
                .as_ref()
                .map(|selected| selected.discard),
            Some(tile("2s")),
        );
        assert_eq!(
            candidate(tile("7p")).comparison_reason,
            DiscardComparisonReason::WeightedNextAcceptanceRemaining,
        );
        assert!(
            diagnostic
                .candidates
                .iter()
                .all(|candidate| candidate.prospective_value.is_none())
        );
    }

    // ---- same-shanten の手変わり ----

    // 門前14枚 13m 68m 456789p 5s 9s EE。打 9s で1向聴 (受け入れ 2m / 7m の8枚) になり、そこから
    // 4m をツモっても1向聴のままだが、5s を切ると 13m の嵌張が 34m の両面へ変わって受け入れが
    // 12枚 / 3種類へ広がる。赤5は含まず、5p は黒1枚だけ持つ。
    fn same_shanten_hand() -> Vec<TileId> {
        ids(&[0, 8, 20, 28, 48, 53, 56, 60, 64, 68, 89, 104, 108, 109])
    }

    static SAME_SHANTEN_CASE: LazyLock<Case> = LazyLock::new(|| {
        let hand = same_shanten_hand();
        let visible = hand.clone();
        visible_case(&hand, FixedMeldCount::NONE, Vec::new(), None, None, visible)
    });

    // 4m が4枚とも見えている同じ局面。
    static SAME_SHANTEN_WITH_SEEN_DRAW: LazyLock<Case> = LazyLock::new(|| {
        let hand = same_shanten_hand();
        let mut visible = hand.clone();
        visible.extend(ids(&[12, 13, 14, 15]));
        visible_case(&hand, FixedMeldCount::NONE, Vec::new(), None, None, visible)
    });

    fn evaluation_of(case: &Case, discard: TileType) -> &DiscardEvaluation {
        case.situation
            .evaluations
            .iter()
            .find(|evaluation| evaluation.discard == discard)
            .expect("打牌候補がある")
    }

    #[test]
    fn draws_are_classified_by_the_existing_shanten_result() {
        // 仮想ツモ対象は「向聴数を下げる牌」と「維持する牌」だけで、悪化する牌は含まない。
        let case = &*SAME_SHANTEN_CASE;

        let mut same_shanten = 0;
        for (candidate, evaluation) in case
            .lookahead
            .candidates
            .iter()
            .zip(case.situation.evaluations.iter())
        {
            let current = evaluation.min_shanten_after_discard();
            let accepted = evaluation.acceptance_after_discard.tile_types();
            for draw in &candidate.draws {
                assert!(
                    draw.remaining > 0,
                    "{:?} {:?}",
                    candidate.discard,
                    draw.draw
                );
                match draw.transition {
                    DrawTransition::Progress => {
                        assert!(draw.shanten_after_draw.min() < current);
                        assert!(accepted.contains(&draw.draw));
                    }
                    DrawTransition::SameShanten => {
                        assert_eq!(draw.shanten_after_draw.min(), current);
                        // 向聴数を維持する牌を既存の受け入れへ混ぜない。
                        assert!(
                            !accepted.contains(&draw.draw),
                            "{:?} {:?}",
                            candidate.discard,
                            draw.draw
                        );
                        same_shanten += 1;
                    }
                }
            }

            let tiles: Vec<_> = candidate.draws.iter().map(|draw| draw.draw).collect();
            let mut unique = tiles.clone();
            unique.sort_by_key(|tile| tile.index());
            unique.dedup();
            assert_eq!(unique.len(), tiles.len(), "{:?}", candidate.discard);
        }
        assert!(same_shanten > 0, "向聴数を維持する仮想ツモがある局面が必要");
    }

    #[test]
    fn a_same_shanten_draw_reaches_a_wider_next_acceptance() {
        let case = &*SAME_SHANTEN_CASE;
        let discard = tile("9s");
        let evaluation = evaluation_of(case, discard);
        assert_eq!(evaluation.min_shanten_after_discard(), 1);
        assert_eq!(evaluation.acceptance_total_remaining(), 8);
        assert_eq!(evaluation.acceptance_type_count(), 2);

        let draw = case
            .lookahead
            .candidate(discard)
            .and_then(|candidate| candidate.draw(tile("4m")))
            .expect("4m の枝がある");
        assert_eq!(draw.transition, DrawTransition::SameShanten);
        assert_eq!(draw.shanten_after_draw.min(), 1);
        assert_eq!(draw.remaining, 4);

        let [variant] = draw.variants.as_slice() else {
            panic!("赤5の無い牌種は物理牌 variant が1件");
        };
        let next = variant.next_discard.as_ref().expect("next discard exists");
        assert_eq!(next.discard, tile("5s"));
        // 2手目を切ってもまだ1向聴で、受け入れだけが広がる。
        assert_eq!(next.min_shanten_after_discard(), 1);
        assert_eq!(next.acceptance_total_remaining(), 12);
        assert_eq!(next.acceptance_type_count(), 3);
        assert!(next.acceptance_total_remaining() > evaluation.acceptance_total_remaining());
        assert!(next.acceptance_type_count() > evaluation.acceptance_type_count());
    }

    #[test]
    fn same_shanten_next_discard_matches_the_existing_discard_selection() {
        // 向聴数を維持する枝でも、2手目は同じ仮想手牌を既存打牌選択へ渡した結果と一致する。
        let case = &*SAME_SHANTEN_CASE;

        let mut checked = 0;
        for (discard, draw, variant) in variants(&case.lookahead) {
            if draw.transition != DrawTransition::SameShanten {
                continue;
            }
            assert_eq!(
                variant.next_discard,
                expected_next_discard(&case.situation, discard, variant.drawn_tile),
                "discard {:?} draw {:?} variant {:?}",
                discard,
                draw.draw,
                variant.drawn_tile,
            );
            checked += 1;
        }
        assert!(checked > 0, "向聴数を維持する仮想ツモがある局面が必要");
    }

    #[test]
    fn seen_tiles_drop_out_of_the_draws() {
        // 見え牌で残枚数が 0 になった牌は仮想ツモの対象にしない。
        let seen_draw = tile("4m");
        assert!(
            SAME_SHANTEN_CASE
                .lookahead
                .candidate(tile("9s"))
                .and_then(|candidate| candidate.draw(seen_draw))
                .is_some()
        );

        for candidate in &SAME_SHANTEN_WITH_SEEN_DRAW.lookahead.candidates {
            assert!(
                candidate.draw(seen_draw).is_none(),
                "discard {:?}",
                candidate.discard
            );
            assert!(candidate.draws.iter().all(|draw| draw.remaining > 0));
        }
    }

    #[test]
    fn a_same_shanten_five_draw_splits_into_physical_variants() {
        // 赤5 / 黒5の分割は向聴数を維持する枝でも既存 helper が source of truth。
        let case = &*SAME_SHANTEN_CASE;
        let red_five_seen = seen_red_fives(case.situation.visible.iter().copied());

        let mut split_draws = 0;
        for candidate in &case.lookahead.candidates {
            for draw in candidate.draws_with(DrawTransition::SameShanten) {
                let expected: Vec<_> = physical_tile_variants(
                    draw.draw,
                    draw.remaining,
                    red_five_seen[draw.draw.index()],
                )
                .map(|variant| (variant.tile, variant.remaining))
                .collect();
                let actual: Vec<_> = draw
                    .variants
                    .iter()
                    .map(|variant| (variant.drawn_tile, variant.remaining))
                    .collect();
                assert_eq!(
                    actual, expected,
                    "discard {:?} draw {:?}",
                    candidate.discard, draw.draw
                );
                split_draws += usize::from(draw.variants.len() == 2);
            }
        }
        assert!(split_draws > 0, "赤5が未見の仮想ツモがある局面が必要");

        // 手牌に黒5pを1枚持つので 5p は残り3枚。赤1枚 / 黒2枚へ分かれる。
        let five_pin = case
            .lookahead
            .candidate(tile("9s"))
            .and_then(|candidate| candidate.draw(tile("5p")))
            .expect("5p の枝がある");
        assert_eq!(five_pin.transition, DrawTransition::SameShanten);
        assert_eq!(five_pin.remaining, 3);
        assert_eq!(
            five_pin
                .variants
                .iter()
                .map(|variant| (variant.drawn_tile.is_red(), variant.remaining))
                .collect::<Vec<_>>(),
            vec![(true, 1), (false, 2)],
        );
    }

    #[test]
    fn same_shanten_draws_do_not_change_the_forward_metrics() {
        // 向聴数を維持する枝は既存の前方集計値へ寄与しない。詳細診断から集計しても同じ値になる。
        let case = &*SAME_SHANTEN_CASE;
        let metrics = forward_metrics(&inputs(&case.situation), &case.situation.evaluations);
        assert_eq!(
            forward_metrics_from_lookahead(
                &inputs(&case.situation),
                &case.situation.evaluations,
                &case.lookahead,
            ),
            metrics,
        );

        let mut checked = 0;
        for (candidate, evaluation) in case
            .lookahead
            .candidates
            .iter()
            .zip(case.situation.evaluations.iter())
        {
            for variant in candidate
                .draws_with(DrawTransition::SameShanten)
                .flat_map(|draw| draw.variants.iter())
            {
                let next = variant.next_discard.as_ref().expect("next discard exists");
                // 2手目を切っても向聴数が変わらないので、既存集計の条件を満たさない。
                assert_eq!(
                    next.min_shanten_after_discard(),
                    evaluation.min_shanten_after_discard()
                );
                // まだテンパイではないので将来打点も持たない。
                assert_eq!(variant.prospective_value, None);
                checked += 1;
            }
        }
        assert!(checked > 0);
    }

    #[test]
    fn the_same_shanten_metric_shares_the_branch_evaluation() {
        // scalar 経路と詳細診断は同じ枝評価・同じ accumulator を共有する。
        let case = &*SAME_SHANTEN_CASE;

        let mut checked = 0;
        for (candidate, evaluation) in case
            .lookahead
            .candidates
            .iter()
            .zip(case.situation.evaluations.iter())
        {
            let metric = candidate.same_shanten_forward_metric();
            assert_eq!(
                same_shanten_forward_metric_for_candidate(&inputs(&case.situation), evaluation),
                metric,
                "{:?}",
                candidate.discard
            );

            let mut expected = WeightedForwardMetric::default();
            for variant in candidate
                .draws_with(DrawTransition::SameShanten)
                .flat_map(|draw| draw.variants.iter())
            {
                let next = variant.next_discard.as_ref().expect("next discard exists");
                expected.weighted_remaining +=
                    u32::from(variant.remaining) * u32::from(next.acceptance_total_remaining());
                expected.weighted_type_count +=
                    u32::from(variant.remaining) * next.acceptance_type_count() as u32;
            }
            assert_eq!(metric.weighted_remaining, expected.weighted_remaining);
            assert_eq!(metric.weighted_type_count, expected.weighted_type_count);
            // テンパイへ届かない枝なので打点込みの集計値は持たない。
            assert_eq!(metric.prospective_value, None);
            checked += usize::from(metric.weighted_remaining > 0);
        }
        assert!(checked > 0);
    }

    #[test]
    fn shanten_stays_ahead_of_the_same_shanten_hand_change() {
        // same-shanten 手変わりの集計値が大きくても、打牌後1向聴より2向聴を優先しない。
        let case = &*SAME_SHANTEN_CASE;
        let metric_of = |discard: TileType| {
            case.lookahead
                .candidate(discard)
                .map(DiscardLookaheadDiagnostic::same_shanten_forward_metric)
                .expect("打牌候補がある")
        };

        let iishanten = tile("9s");
        let two_shanten = tile("1m");
        assert_eq!(
            evaluation_of(case, iishanten).min_shanten_after_discard(),
            1
        );
        assert_eq!(
            evaluation_of(case, two_shanten).min_shanten_after_discard(),
            2
        );
        assert!(
            metric_of(two_shanten).weighted_remaining > metric_of(iishanten).weighted_remaining,
            "2向聴側の手変わりが大きい局面が必要"
        );

        let metrics = forward_metrics(&inputs(&case.situation), &case.situation.evaluations);
        let selected = best_discard_selection_index_with_forward_metrics(
            &case.situation.evaluations,
            &metrics,
        )
        .expect("打牌を選べる");
        assert_eq!(
            case.situation.evaluations[selected].min_shanten_after_discard(),
            1
        );
    }

    // ---- same-shanten の枝の先にあるテンパイ ----

    // 将来打点の代わりに、テンパイ形の受け入れ残枚数をそのまま返す検証用の評価器。
    //
    // 打点そのものは bot-logic の責務ではないので、深い枝の重み付けだけを見るために点数計算を
    // 持たない評価器を使う。
    struct AcceptanceRemainingValuator;

    impl ProspectiveTenpaiValuator for AcceptanceRemainingValuator {
        fn tenpai_value(&self, tenpai: &ProspectiveTenpai<'_>) -> Option<u64> {
            Some(u64::from(tenpai.acceptance.total_remaining()))
        }
    }

    // 指定した牌を待ちに含むテンパイだけ打点を確定できない検証用の評価器。
    struct UnknownWaitValuator {
        unknown_wait: TileType,
    }

    impl ProspectiveTenpaiValuator for UnknownWaitValuator {
        fn tenpai_value(&self, tenpai: &ProspectiveTenpai<'_>) -> Option<u64> {
            tenpai
                .acceptance
                .tiles
                .iter()
                .all(|wait| wait.tile != self.unknown_wait)
                .then_some(1)
        }
    }

    static ACCEPTANCE_REMAINING_VALUATOR: AcceptanceRemainingValuator = AcceptanceRemainingValuator;

    fn evaluation_in(situation: &Situation, discard: TileType) -> &DiscardEvaluation {
        situation
            .evaluations
            .iter()
            .find(|evaluation| evaluation.discard == discard)
            .expect("打牌候補がある")
    }

    // 打牌候補1件だけの、same-shanten の枝をテンパイまで追った2手先診断。全候補分の深い探索を
    // 構築せずに1候補の枝を見るための test 専用 helper。
    fn downstream_candidate(
        situation: &Situation,
        discard: TileType,
        valuator: &dyn ProspectiveTenpaiValuator,
    ) -> DiscardLookaheadDiagnostic {
        diagnose_lookahead(
            &inputs(situation)
                .with_prospective_valuator(valuator)
                .with_same_shanten_downstream(),
            std::slice::from_ref(evaluation_in(situation, discard)),
        )
        .candidates
        .pop()
        .expect("打牌候補の診断がある")
    }

    // same-shanten の枝をテンパイまで追った全候補分の診断。将来打点は持たないので、3手目の
    // 最良打牌は既存打牌選択と同じ比較で決まる。
    static SAME_SHANTEN_DOWNSTREAM_CASE: LazyLock<Case> = LazyLock::new(|| {
        let hand = same_shanten_hand();
        let visible = hand.clone();
        let situation =
            visible_situation(&hand, FixedMeldCount::NONE, Vec::new(), None, None, visible);
        let lookahead = diagnose_lookahead(
            &inputs(&situation).with_same_shanten_downstream(),
            &situation.evaluations,
        );
        Case {
            situation,
            lookahead,
        }
    });

    // 打 9s の枝を将来打点付きでテンパイまで追った診断。重み付けの積算だけを見るので、候補は
    // 1件に絞る。
    static SAME_SHANTEN_DOWNSTREAM_VALUE: LazyLock<DiscardLookaheadDiagnostic> =
        LazyLock::new(|| {
            downstream_candidate(
                &SAME_SHANTEN_DOWNSTREAM_CASE.situation,
                tile("9s"),
                &ACCEPTANCE_REMAINING_VALUATOR,
            )
        });

    #[test]
    fn a_same_shanten_branch_reaches_a_tenpai_downstream() {
        // 1向聴 → same-shanten ツモ → 2手目 (まだ1向聴) → 受け入れのツモ → 3手目 → テンパイ。
        let case = &*SAME_SHANTEN_DOWNSTREAM_CASE;
        let candidate = case
            .lookahead
            .candidate(tile("9s"))
            .expect("打牌候補の診断がある");

        let draw = candidate.draw(tile("4m")).expect("4m の枝がある");
        assert_eq!(draw.transition, DrawTransition::SameShanten);
        let [variant] = draw.variants.as_slice() else {
            panic!("赤5の無い牌種は物理牌 variant が1件");
        };
        let next = variant.next_discard.as_ref().expect("2手目がある");
        assert_eq!(next.discard, tile("5s"));
        assert_eq!(next.min_shanten_after_discard(), 1);

        let downstream = variant.downstream.as_ref().expect("先の枝がある");
        // 3手目へ進むツモ牌は2手目の打牌評価が持つ既存受け入れそのもので、判定を作り直さない。
        assert_eq!(
            downstream
                .draws
                .iter()
                .map(|draw| (draw.draw, draw.remaining, draw.shanten_after_draw))
                .collect::<Vec<_>>(),
            next.acceptance_after_discard
                .tiles
                .iter()
                .map(|accepted| (
                    accepted.tile,
                    accepted.remaining,
                    accepted.shanten_after_draw
                ))
                .collect::<Vec<_>>(),
        );

        let mut tenpai_waits = 0;
        for draw in &downstream.draws {
            assert_eq!(draw.transition, DrawTransition::Progress);
            for variant in &draw.variants {
                let third = variant.next_discard.as_ref().expect("3手目がある");
                assert_eq!(third.min_shanten_after_discard(), TENPAI_SHANTEN);
                tenpai_waits += third.acceptance_after_discard.tiles.len();
            }
        }
        assert!(
            tenpai_waits > 0,
            "最終待ちを持つテンパイへ到達する必要がある"
        );
    }

    #[test]
    fn the_downstream_discard_matches_the_existing_discard_selection() {
        // 3手目の最良打牌は、同じ仮想手牌を既存打牌選択 API へ渡した結果と一致する。
        let case = &*SAME_SHANTEN_DOWNSTREAM_CASE;

        let mut checked = 0;
        for (discard, draw, variant) in variants(&case.lookahead) {
            let Some(downstream) = variant.downstream.as_ref() else {
                continue;
            };
            assert_eq!(draw.transition, DrawTransition::SameShanten);
            let next = variant.next_discard.as_ref().expect("2手目がある");

            // 3手目を評価する仮想手牌と見え牌。1手目の打牌は元の手牌として visible に残り、
            // 仮想ツモ牌だけが新しく見え牌になる。
            let mut tiles = hypothetical_tiles(&case.situation, discard, variant.drawn_tile);
            let (_, remaining) =
                split_discarded_tile(tiles.clone(), next).expect("2手目の物理牌がある");
            tiles = remaining;
            let mut visible = hypothetical_visible(&case.situation, variant.drawn_tile);

            for downstream_draw in &downstream.draws {
                for downstream_variant in &downstream_draw.variants {
                    let mut third_tiles = tiles.clone();
                    third_tiles.push(downstream_variant.drawn_tile);
                    visible.push(downstream_variant.drawn_tile);

                    assert_eq!(
                        downstream_variant.next_discard,
                        select_best_discard_from_tiles_with_visible_tiles(
                            &third_tiles,
                            &case.situation.dora_indicators,
                            case.situation.round_wind,
                            case.situation.seat_wind,
                            &visible,
                        ),
                        "discard {:?} draw {:?} next {:?} downstream draw {:?}",
                        discard,
                        variant.drawn_tile,
                        next.discard,
                        downstream_variant.drawn_tile,
                    );

                    visible.pop();
                    checked += 1;
                }
            }
        }
        assert!(
            checked > 0,
            "先の枝を持つ same-shanten の枝がある局面が必要"
        );
    }

    #[test]
    fn the_downstream_remaining_counts_every_tile_seen_so_far() {
        // 1手目の打牌・2手目の打牌・仮想ツモ牌はどれも見え牌になり、先の枝の残枚数へ反映される。
        let case = &*SAME_SHANTEN_DOWNSTREAM_CASE;

        let mut checked = 0;
        for (discard, _, variant) in variants(&case.lookahead) {
            let Some(downstream) = variant.downstream.as_ref() else {
                continue;
            };
            // 1手目の打牌も2手目の打牌も元の手牌の物理牌なので visible に含まれる。仮想ツモ牌
            // だけが新しく見えた牌になる。
            let seen =
                TileCounts::from_tiles(hypothetical_visible(&case.situation, variant.drawn_tile));
            for downstream_draw in &downstream.draws {
                assert_eq!(
                    downstream_draw.remaining,
                    4 - seen.count(downstream_draw.draw),
                    "discard {:?} draw {:?} downstream draw {:?}",
                    discard,
                    variant.drawn_tile,
                    downstream_draw.draw,
                );
                checked += 1;
            }
        }
        assert!(checked > 0);
    }

    #[test]
    fn the_downstream_value_multiplies_every_physical_variant_remaining() {
        // same-shanten ツモ・3手目へ進むツモ・最終テンパイの打点がすべて積算される。
        let candidate = &*SAME_SHANTEN_DOWNSTREAM_VALUE;

        let mut expected = 0u64;
        let mut split_draws = 0;
        for draw in candidate.draws_with(DrawTransition::SameShanten) {
            assert_eq!(
                draw.variants
                    .iter()
                    .map(|variant| u32::from(variant.remaining))
                    .sum::<u32>(),
                u32::from(draw.remaining),
            );
            split_draws += usize::from(draw.variants.len() == 2);

            for variant in &draw.variants {
                let downstream = variant.downstream.as_ref().expect("先の枝がある");
                let mut inner = 0u64;
                for downstream_draw in &downstream.draws {
                    for downstream_variant in &downstream_draw.variants {
                        inner += u64::from(downstream_variant.remaining)
                            * downstream_variant
                                .prospective_value
                                .expect("最終テンパイの打点がある");
                    }
                }
                assert_eq!(downstream.weighted_value(), Some(inner));
                expected += u64::from(variant.remaining) * inner;
            }
        }

        assert!(expected > 0);
        assert!(split_draws > 0, "赤5 / 黒5へ分かれる枝がある局面が必要");
        assert_eq!(candidate.same_shanten_downstream_value(), Some(expected));
    }

    #[test]
    fn a_red_five_downstream_branch_keeps_its_own_remaining() {
        // 赤5と黒5は牌種単位へ潰さず、物理牌 variant ごとに先の枝まで別々に評価する。
        let candidate = &*SAME_SHANTEN_DOWNSTREAM_VALUE;

        // 手牌に黒5pを1枚持つので 5p は残り3枚。赤1枚 / 黒2枚へ分かれる。
        let draw = candidate.draw(tile("5p")).expect("5p の枝がある");
        assert_eq!(draw.transition, DrawTransition::SameShanten);
        assert_eq!(draw.remaining, 3);

        let [red, black] = draw.variants.as_slice() else {
            panic!("赤5が未見の牌種は物理牌 variant が2件");
        };
        assert!(red.drawn_tile.is_red());
        assert!(!black.drawn_tile.is_red());
        assert_eq!((red.remaining, black.remaining), (1, 2));
        assert!(red.downstream_value().is_some());
        assert!(black.downstream_value().is_some());
    }

    #[test]
    fn the_downstream_scalar_path_matches_the_detailed_diagnostic() {
        // scalar 経路と詳細診断は同じ枝評価・同じ accumulator を共有する。
        let situation = &SAME_SHANTEN_DOWNSTREAM_CASE.situation;
        let discard = tile("9s");
        let inputs = inputs(situation).with_prospective_valuator(&ACCEPTANCE_REMAINING_VALUATOR);

        assert_eq!(
            same_shanten_downstream_value_for_candidate(&inputs, evaluation_in(situation, discard)),
            SAME_SHANTEN_DOWNSTREAM_VALUE.same_shanten_downstream_value(),
        );
    }

    #[test]
    fn an_unknown_downstream_tenpai_keeps_the_value_unknown() {
        // 打点を確定できない枝が1つでもあれば、0点へ潰さず全体を unknown にする。
        let situation = &SAME_SHANTEN_DOWNSTREAM_CASE.situation;
        let discard = tile("9s");
        let known = &*SAME_SHANTEN_DOWNSTREAM_VALUE;
        assert!(known.same_shanten_downstream_value().is_some());

        // 到達したテンパイの待ちに必ず現れる牌を1つ選ぶと、その枝だけが確定しなくなる。
        let unknown_wait = known
            .draws_with(DrawTransition::SameShanten)
            .flat_map(|draw| draw.variants.iter())
            .filter_map(|variant| variant.downstream.as_ref())
            .flat_map(|downstream| downstream.draws.iter())
            .flat_map(|draw| draw.variants.iter())
            .filter_map(|variant| variant.next_discard.as_ref())
            .flat_map(|next| next.acceptance_after_discard.tiles.iter())
            .map(|wait| wait.tile)
            .next()
            .expect("最終待ちがある");

        let valuator = UnknownWaitValuator { unknown_wait };
        let candidate = downstream_candidate(situation, discard, &valuator);
        assert_eq!(candidate.same_shanten_downstream_value(), None);
        assert_eq!(
            same_shanten_downstream_value_for_candidate(
                &inputs(situation).with_prospective_valuator(&valuator),
                evaluation_in(situation, discard),
            ),
            None,
        );
    }

    #[test]
    fn only_iishanten_candidates_are_followed_to_a_tenpai() {
        // 今回の対象は現在打牌後が1向聴の候補だけで、2向聴以上は探索しない。
        let case = &*SAME_SHANTEN_DOWNSTREAM_CASE;

        let mut iishanten = 0;
        for (candidate, evaluation) in case
            .lookahead
            .candidates
            .iter()
            .zip(case.situation.evaluations.iter())
        {
            let followed = candidate
                .draws_with(DrawTransition::SameShanten)
                .flat_map(|draw| draw.variants.iter())
                .any(|variant| variant.downstream.is_some());
            assert_eq!(
                followed,
                evaluation.min_shanten_after_discard() == 1,
                "{:?}",
                candidate.discard
            );
            // 向聴数を下げる枝は今回の対象ではない。
            assert!(
                candidate
                    .draws_with(DrawTransition::Progress)
                    .flat_map(|draw| draw.variants.iter())
                    .all(|variant| variant.downstream.is_none())
            );
            iishanten += usize::from(followed);
        }
        assert!(iishanten > 0);

        let two_shanten = evaluation_in(&case.situation, tile("1m"));
        assert_eq!(two_shanten.min_shanten_after_discard(), 2);
        assert_eq!(
            same_shanten_downstream_value_for_candidate(
                &inputs(&case.situation).with_prospective_valuator(&ACCEPTANCE_REMAINING_VALUATOR),
                two_shanten,
            ),
            None,
        );
    }

    #[test]
    fn following_the_same_shanten_branch_keeps_the_selection_unchanged() {
        // 深い枝を追っても打牌選択に使う集計値も選択結果も変わらない。
        let plain = &*SAME_SHANTEN_CASE;
        let followed = &*SAME_SHANTEN_DOWNSTREAM_CASE;
        assert_eq!(plain.situation.evaluations, followed.situation.evaluations);

        let metrics = forward_metrics_from_lookahead(
            &inputs(&plain.situation),
            &plain.situation.evaluations,
            &plain.lookahead,
        );
        assert_eq!(
            forward_metrics_from_lookahead(
                &inputs(&followed.situation),
                &followed.situation.evaluations,
                &followed.lookahead,
            ),
            metrics,
        );
        assert_eq!(
            best_discard_selection_index_with_forward_metrics(
                &followed.situation.evaluations,
                &metrics,
            ),
            best_discard_selection_index_with_forward_metrics(
                &plain.situation.evaluations,
                &metrics,
            ),
        );

        for (candidate, plain_candidate) in followed
            .lookahead
            .candidates
            .iter()
            .zip(plain.lookahead.candidates.iter())
        {
            // 既存 same-shanten 集計値も、深い枝を追ったかどうかで変わらない。
            assert_eq!(
                candidate.same_shanten_forward_metric(),
                plain_candidate.same_shanten_forward_metric(),
                "{:?}",
                candidate.discard
            );
            assert_eq!(
                candidate.weighted_forward_metric(TENPAI_SHANTEN),
                plain_candidate.weighted_forward_metric(TENPAI_SHANTEN),
            );
        }
    }

    // ---- self-tsumo continuation ----

    // テンパイ形の待ちをそのままツモ和了できる待ちとし、1枚あたり固定打点を持つ検証用の評価器。
    //
    // 点数計算は bot-logic の責務ではないので、経路の確率と深さの組み立てだけを見る。
    struct FixedTsumoValuator {
        payment: u64,
    }

    impl ProspectiveTsumoValuator for FixedTsumoValuator {
        fn tenpai_tsumo_value(&self, tenpai: &ProspectiveTenpai<'_>) -> Option<TenpaiTsumoValue> {
            let winning_remaining = u32::from(tenpai.acceptance.total_remaining());
            Some(TenpaiTsumoValue {
                winning_remaining,
                weighted_total: u64::from(winning_remaining) * self.payment,
            })
        }
    }

    // 指定した牌を待ちに含むテンパイだけツモ打点を確定できない検証用の評価器。
    struct UnknownWaitTsumoValuator {
        unknown_wait: TileType,
    }

    impl ProspectiveTsumoValuator for UnknownWaitTsumoValuator {
        fn tenpai_tsumo_value(&self, tenpai: &ProspectiveTenpai<'_>) -> Option<TenpaiTsumoValue> {
            tenpai
                .acceptance
                .tiles
                .iter()
                .all(|wait| wait.tile != self.unknown_wait)
                .then(|| TenpaiTsumoValue {
                    winning_remaining: u32::from(tenpai.acceptance.total_remaining()),
                    weighted_total: u64::from(tenpai.acceptance.total_remaining()) * 3900,
                })
        }
    }

    static FIXED_TSUMO_VALUATOR: FixedTsumoValuator = FixedTsumoValuator { payment: 3900 };

    // 検証用の残り自摸機会。局面から導く値ではなく、確率の組み立てだけを固定するための入力。
    const TEST_OWN_FUTURE_DRAWS: u32 = 10;

    fn self_tsumo_inputs<'a>(
        situation: &'a Situation,
        valuator: &'a dyn ProspectiveTsumoValuator,
    ) -> LookaheadInputs<'a> {
        inputs(situation)
            .with_tsumo_valuator(valuator)
            .with_own_future_draws(TEST_OWN_FUTURE_DRAWS)
    }

    // 構築済みの枝を辿って期待支払いを組み立て直す。集計対象の経路と深さが実装と一致することを
    // 確認するためのもので、確率そのものは self_tsumo module の単体テストが固定する。
    fn expected_paths_value(
        candidate: &DiscardLookaheadDiagnostic,
        facts: SelfTsumoFacts,
    ) -> Option<u64> {
        let terminal = |variant: &DrawVariantLookaheadDiagnostic, path: Option<SelfTsumoPath>| {
            let next = variant.next_discard.as_ref()?;
            (next.min_shanten_after_discard() == TENPAI_SHANTEN).then(|| {
                path.unwrap()
                    .expected_payment(facts, variant.tsumo_continuation.unwrap())
            })
        };

        let mut total = 0u64;
        for draw in &candidate.draws {
            for variant in &draw.variants {
                match draw.transition {
                    DrawTransition::Progress => {
                        let path = SelfTsumoPath::immediate(variant.remaining, facts.unknown_tiles);
                        total += terminal(variant, path).unwrap_or(0);
                    }
                    DrawTransition::SameShanten => {
                        for second_draw in &variant.downstream.as_ref()?.draws {
                            for second in &second_draw.variants {
                                let path = SelfTsumoPath::via_same_shanten(
                                    variant.remaining,
                                    second.remaining,
                                    facts.unknown_tiles,
                                );
                                total += terminal(second, path).unwrap_or(0);
                            }
                        }
                    }
                }
            }
        }
        Some(total)
    }

    #[test]
    fn the_unknown_tile_count_is_the_same_for_every_candidate() {
        // 打牌で手牌から河へ移る1枚はどちらでも見えているので、未確認牌の総数は候補で変わらない。
        let case = &*SAME_SHANTEN_CASE;
        let inputs = self_tsumo_inputs(&case.situation, &FIXED_TSUMO_VALUATOR);
        let facts = inputs.self_tsumo_facts().expect("材料が揃っている");

        // 手牌14枚だけが見えている局面なので、未確認牌は 136 - 14 枚。
        assert_eq!(facts.unknown_tiles, 122);
        assert_eq!(facts.own_future_draws, TEST_OWN_FUTURE_DRAWS);
    }

    #[test]
    fn the_expected_self_tsumo_value_sums_the_immediate_and_hand_change_paths() {
        let case = &*SAME_SHANTEN_CASE;
        let inputs = self_tsumo_inputs(&case.situation, &FIXED_TSUMO_VALUATOR);
        let facts = inputs.self_tsumo_facts().expect("材料が揃っている");
        let evaluation = evaluation_of(case, tile("9s"));
        let candidate = search_candidate(
            &inputs,
            evaluation,
            &[
                DrawScope::Progress,
                DrawScope::SameShanten { downstream: true },
            ],
        );

        let value = candidate
            .expected_self_tsumo_value(facts)
            .expect("ツモ打点を確定できる");
        assert_eq!(Some(value), expected_paths_value(&candidate, facts));
        assert!(value > 0);
    }

    #[test]
    fn the_hand_change_paths_are_added_on_the_same_scale_as_the_immediate_ones() {
        // すぐテンパイする経路と一度手変わりする経路が同じ尺度で足し合わされる。手変わりの枝を
        // 進めた集計値は、向聴数を下げる枝だけの集計値より必ず大きい。
        let case = &*SAME_SHANTEN_CASE;
        let inputs = self_tsumo_inputs(&case.situation, &FIXED_TSUMO_VALUATOR);
        let facts = inputs.self_tsumo_facts().expect("材料が揃っている");
        let evaluation = evaluation_of(case, tile("9s"));

        let immediate = search_candidate(&inputs, evaluation, PROGRESS_ONLY)
            .expected_self_tsumo_value(facts)
            .expect("ツモ打点を確定できる");
        let with_hand_change = search_candidate(
            &inputs,
            evaluation,
            &[
                DrawScope::Progress,
                DrawScope::SameShanten { downstream: true },
            ],
        )
        .expected_self_tsumo_value(facts)
        .expect("ツモ打点を確定できる");

        assert!(
            with_hand_change > immediate,
            "with_hand_change: {with_hand_change}, immediate: {immediate}"
        );
    }

    #[test]
    fn a_hand_change_path_needs_the_branch_beyond_it() {
        // 手変わりの枝の先を探索していない診断は、寄与 0 ではなく確定しない値になる。
        let case = &*SAME_SHANTEN_CASE;
        let inputs = self_tsumo_inputs(&case.situation, &FIXED_TSUMO_VALUATOR);
        let facts = inputs.self_tsumo_facts().expect("材料が揃っている");
        let evaluation = evaluation_of(case, tile("9s"));
        let shallow = search_candidate(
            &inputs,
            evaluation,
            &[
                DrawScope::Progress,
                DrawScope::SameShanten { downstream: false },
            ],
        );

        assert_eq!(shallow.expected_self_tsumo_value(facts), None);
    }

    #[test]
    fn an_unknown_tsumo_value_makes_the_whole_candidate_unknown() {
        let case = &*SAME_SHANTEN_CASE;
        let valuator = UnknownWaitTsumoValuator {
            unknown_wait: tile("2m"),
        };
        let inputs = self_tsumo_inputs(&case.situation, &valuator);
        let facts = inputs.self_tsumo_facts().expect("材料が揃っている");
        let evaluation = evaluation_of(case, tile("9s"));
        let candidate = search_candidate(
            &inputs,
            evaluation,
            &[
                DrawScope::Progress,
                DrawScope::SameShanten { downstream: true },
            ],
        );

        assert_eq!(candidate.expected_self_tsumo_value(facts), None);
    }

    #[test]
    fn the_facts_need_both_the_valuator_and_the_remaining_draws() {
        let case = &*SAME_SHANTEN_CASE;
        assert_eq!(inputs(&case.situation).self_tsumo_facts(), None);
        assert_eq!(
            inputs(&case.situation)
                .with_own_future_draws(TEST_OWN_FUTURE_DRAWS)
                .self_tsumo_facts(),
            None
        );
        assert_eq!(
            inputs(&case.situation)
                .with_tsumo_valuator(&FIXED_TSUMO_VALUATOR)
                .self_tsumo_facts(),
            None
        );
    }

    #[test]
    fn the_selection_metric_and_the_detailed_diagnostic_agree() {
        // 詳細診断の有無で、打牌選択が使う self-tsumo continuation が変わらない。
        let case = &*SAME_SHANTEN_CASE;
        let inputs = self_tsumo_inputs(&case.situation, &FIXED_TSUMO_VALUATOR);
        let evaluations = &case.situation.evaluations;

        let selection = forward_metrics(&inputs, evaluations);
        let lookahead = diagnose_lookahead(&inputs, evaluations);
        let from_diagnostic = forward_metrics_from_lookahead(&inputs, evaluations, &lookahead);

        assert_eq!(selection, from_diagnostic);
        assert!(
            selection
                .iter()
                .any(|metric| metric.expected_self_tsumo_value.is_some())
        );
    }

    #[test]
    fn without_the_remaining_draws_the_new_axis_is_unavailable() {
        // 山の残枚数が分からない局面では新しい軸を持たず、既存の集計値だけになる。
        let case = &*SAME_SHANTEN_CASE;
        let evaluations = &case.situation.evaluations;
        let plain = forward_metrics(
            &inputs(&case.situation).with_tsumo_valuator(&FIXED_TSUMO_VALUATOR),
            evaluations,
        );

        assert!(
            plain
                .iter()
                .all(|metric| metric.expected_self_tsumo_value.is_none())
        );
    }

    #[test]
    fn only_the_comparison_targets_search_the_hand_change_branch() {
        // 比較対象にならない候補まで手変わりの枝を深く探索しない。
        let case = &*SAME_SHANTEN_CASE;
        let inputs = self_tsumo_inputs(&case.situation, &FIXED_TSUMO_VALUATOR);
        let evaluations = &case.situation.evaluations;
        let lookahead = diagnose_lookahead(&inputs, evaluations);
        let targets = forward_target_mask(evaluations);

        for ((candidate, evaluation), target) in
            lookahead.candidates.iter().zip(evaluations).zip(targets)
        {
            let searched = candidate
                .draws_with(DrawTransition::SameShanten)
                .any(|draw| {
                    draw.variants
                        .iter()
                        .any(|variant| variant.downstream.is_some())
                });
            assert_eq!(
                searched,
                target && evaluation.min_shanten_after_discard() == IISHANTEN_SHANTEN,
                "{:?}",
                candidate.discard
            );
        }
    }

    #[test]
    fn a_two_shanten_candidate_set_keeps_its_existing_metrics() {
        // 2向聴以上では新しい軸を持たず、既存の集計値も変わらない。
        let case = &*TWO_SHANTEN_RED_FIVE_CASE;
        let evaluations = &case.situation.evaluations;
        let with_axis = forward_metrics(
            &self_tsumo_inputs(&case.situation, &FIXED_TSUMO_VALUATOR),
            evaluations,
        );
        let without_axis = forward_metrics(&inputs(&case.situation), evaluations);

        assert_eq!(with_axis, without_axis);
        assert!(
            with_axis
                .iter()
                .all(|metric| metric.expected_self_tsumo_value.is_none())
        );
    }
}
