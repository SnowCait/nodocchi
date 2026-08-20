//! フリテンの判定基盤。
//!
//! 待ちも向聴も既存の受け入れ ([`Acceptance`]) の判定を共有し、専用の shanten / acceptance /
//! wait 計算器は持たない。判定に使う情報は
//!
//! - 既存の受け入れ判定が返すテンパイの待ち
//! - 自分が捨てた牌種 ([`OwnDiscards`])
//! - 評価対象時点の履歴依存フリテン ([`HistoryFuritenFacts`])
//!
//! の3つだけで、`GameContext` のような上位層の局面型には依存しない。
//! 将来 [`crate::lookahead`] の枝で「既存の自分の河 + 1手目の打牌 + 2手目の打牌」を
//! 河として渡す場合も、[`OwnDiscards::with_discards`] で組み立てた値を渡すだけで同じ helper を
//! 使える。
//!
//! # 2種類の待ち
//!
//! 恒常フリテンは「現在のアガリ牌が自分の河にあるか」で決まり、そのアガリ牌が山や他家に残って
//! いるかどうかでは解除されない。一方 [`Acceptance`] は残枚数 0 の牌種を受け入れに含めないので、
//! 4枚とも見えている待ちは受け入れから消える。そこで待ちを2つに分けて扱う。
//!
//! - 構造上のアガリ牌種
//!   ([`structural_acceptance_tile_types`](crate::acceptance::structural_acceptance_tile_types)):
//!   残枚数 0 の牌種も含む。恒常フリテン判定に使う
//! - 実際に残っているツモ可能牌 (既存 [`Acceptance`]): 見え牌を反映する。`tsumo_remaining` /
//!   `tsumo_type_count` に使う
//!
//! したがって見え牌はツモ可能残枚数だけに効き、恒常フリテンの判定そのものには影響しない。
//! 残枚数 0 の牌種を受け入れから除く既存 [`Acceptance`] の semantics も変えない。
//!
//! # 恒常フリテンと履歴依存フリテン
//!
//! 自分の河と現在の待ちから計算できる恒常フリテン ([`PermanentFuriten`]) と、event 履歴が
//! 必要な同巡内フリテン・リーチ後見逃しフリテン ([`HistoryFuritenFacts`]) は別の入力として
//! 保持し、ロン可否 ([`can_ron_from_furiten`]) だけを総合値として一元的に判定する。
//! どの軸も unknown を `false` と推測しない。
//!
//! # 今回扱う範囲
//!
//! 他家の河はフリテン判定に使わない。フリテンを打牌選択の点数へ変換するヒューリスティックも
//! 持たず、事実だけを表現する。

use crate::acceptance::{Acceptance, structural_acceptance_tile_types_with_fixed_melds};
use crate::discard::DiscardEvaluation;
use crate::shanten::{FixedMeldCount, MinShanten};
use crate::tile::{TileId, TileType};
use crate::tile_counts::TileCounts;

// フリテンを判定する向聴数。テンパイ形の待ちだけを対象にする。
pub(crate) const TENPAI_SHANTEN: i8 = 0;

/// 過去の action 履歴に依存するフリテンの観測事実。
///
/// 自分の河と現在の待ちから計算する恒常フリテンとは別の入力事実として保持する。各 field は
/// `None` (取得不能), `Some(false)` (該当しないことを確認済み), `Some(true)` (該当を確認済み)
/// を区別する。恒常フリテンと合わせた総合ロン可否は [`can_ron_from_furiten`] が判定する。
///
/// 値は「いつの時点の事実か」で意味が変わる。打牌後を評価する [`TenpaiWaitAvailability`] には、
/// [`after_discard`](Self::after_discard) で評価対象時点へ補正した値を渡すこと。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct HistoryFuritenFacts {
    /// 同巡内フリテンの現在状態。
    pub same_turn: Option<bool>,
    /// リーチ後にロン可能なアガリ牌を見逃したことによる、局終了まで続くフリテンの現在状態。
    pub riichi_missed_win: Option<bool>,
}

impl HistoryFuritenFacts {
    /// 履歴依存フリテンのどれかに該当するか。判断できない場合は `None`。
    ///
    /// 1軸でも `Some(true)` なら他の軸が unknown でもフリテンだと断定できる。全軸が
    /// `Some(false)` のときだけ非フリテンだと断定し、それ以外は unknown のままにする。
    pub fn is_furiten(&self) -> Option<bool> {
        combine_furiten([self.same_turn, self.riichi_missed_win])
    }

    /// 今回の打牌が完了した時点の履歴依存フリテンへ補正する。
    ///
    /// `after_own_draw` は「今回の打牌の前に自分がツモしたと確定できるか」。自摸 → 打牌で
    /// 同巡内フリテンは必ず解除されるため、`same_turn` は元の値が `Some(true)` や unknown でも
    /// `Some(false)` と確定できる。Chi / Pon 後の打牌のように自分のツモを経たと確認できない
    /// 場合は現在の値をそのまま保ち、unknown を推測で埋めない。
    ///
    /// `riichi_missed_win` は局終了まで続くため、どちらの場合も維持する。
    pub fn after_discard(self, after_own_draw: bool) -> Self {
        if after_own_draw {
            Self {
                same_turn: Some(false),
                ..self
            }
        } else {
            self
        }
    }
}

// 各軸のフリテン確定状況を1つに畳む。1軸でもフリテン確定なら確定、全軸が非フリテン確定なら
// 非フリテン確定、それ以外は unknown。unknown を `false` と推測しない。
fn combine_furiten(axes: impl IntoIterator<Item = Option<bool>>) -> Option<bool> {
    let mut all_known_not_furiten = true;
    for axis in axes {
        match axis {
            Some(true) => return Some(true),
            Some(false) => {}
            None => all_known_not_furiten = false,
        }
    }
    all_known_not_furiten.then_some(false)
}

/// 恒常フリテン判定に使う「自分が捨てた牌」。
///
/// 赤5と黒5を同じ牌種として扱うため、物理牌ではなく牌種で保持する。自分の河が特定できない場合と
/// 「河が空だと確定した」場合を区別するため、河を特定できたかどうかを別に持つ。
///
/// これから切ることが確定している牌 ([`with_discard`](Self::with_discard)) は、河そのものが
/// 不明でも確定情報として扱う。河が不明でもその牌が待ちに含まれていればフリテンだと断定できる
/// 一方、非フリテンだと断定することはできない。
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct OwnDiscards {
    tiles: Vec<TileType>,
    river_known: bool,
}

impl OwnDiscards {
    /// 自分の河を特定できない場合。player 0 を自分と推測しない。
    pub fn unknown() -> Self {
        Self::default()
    }

    /// 自分の河が分かっている場合。赤5は同じ牌種の黒5として扱う。
    pub fn from_river(river: &[TileId]) -> Self {
        Self::from_river_tile_types(river.iter().map(|tile| tile.tile_type()))
    }

    pub fn from_river_tile_types(river: impl IntoIterator<Item = TileType>) -> Self {
        Self {
            tiles: river.into_iter().collect(),
            river_known: true,
        }
    }

    /// 自分の河を特定できない場合を `None` として受け取る。
    ///
    /// `player_id` が無い局面のように自分の河が決まらない入力を、そのまま `Unknown` へ落とす。
    pub fn from_optional_river(river: Option<&[TileId]>) -> Self {
        river.map_or_else(Self::unknown, Self::from_river)
    }

    /// これから切ることが確定している牌を1枚足した河。
    pub fn with_discard(&self, tile: TileType) -> Self {
        self.with_discards([tile])
    }

    /// これから切ることが確定している牌を足した河。
    ///
    /// 前方評価の枝のように「現在打牌 → テンパイに取るための打牌」を続けて足す用途を想定する。
    pub fn with_discards(&self, tiles: impl IntoIterator<Item = TileType>) -> Self {
        let mut extended = self.clone();
        extended.tiles.extend(tiles);
        extended
    }

    /// 自分の河そのものを特定できているか。
    pub fn is_river_known(&self) -> bool {
        self.river_known
    }

    pub fn tile_types(&self) -> &[TileType] {
        &self.tiles
    }

    /// 指定牌種を自分が既に捨てているか。河が不明で判断できない場合は `None`。
    pub fn contains(&self, tile: TileType) -> Option<bool> {
        if self.tiles.contains(&tile) {
            return Some(true);
        }
        self.river_known.then_some(false)
    }
}

/// 自分の河による恒常フリテンの状態。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PermanentFuriten {
    /// 自分の河を特定でき、現在の待ちと重複していない。
    No,
    /// 現在の待ちのうち1種類以上を自分が既に捨てている。
    Yes,
    /// 自分の河を特定できず、フリテンかどうか判断できない。非フリテンとは断定しない。
    Unknown,
}

impl PermanentFuriten {
    /// 恒常フリテンかどうか。判断できない場合は `None`。
    pub fn is_furiten(self) -> Option<bool> {
        match self {
            PermanentFuriten::Yes => Some(true),
            PermanentFuriten::No => Some(false),
            PermanentFuriten::Unknown => None,
        }
    }
}

/// 恒常フリテンの判定結果と、その根拠になった待ち牌。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PermanentFuritenDiagnostic {
    pub status: PermanentFuriten,
    /// 現在の待ちのうち自分の河と重複した牌種。`status` が [`PermanentFuriten::Yes`] のときだけ
    /// 空でない。
    pub discarded_waits: Vec<TileType>,
}

impl PermanentFuritenDiagnostic {
    /// 恒常フリテンかどうか。判断できない場合は `None`。
    pub fn is_furiten(&self) -> Option<bool> {
        self.status.is_furiten()
    }
}

/// 恒常フリテンと評価対象時点の履歴依存フリテンから総合ロン可否を判定する。
///
/// ロン可否を判定する唯一の入口で、call site ごとに組み合わせ規則を書き直さない。
///
/// | 入力 | 結果 |
/// | --- | --- |
/// | 恒常 / 同巡内 / リーチ後見逃しのどれか1軸でもフリテン確定 | `Some(false)` |
/// | 3軸すべて非フリテン確定 | `Some(true)` |
/// | それ以外 | `None` |
///
/// unknown を `false` と推測しない。一方、1軸でもフリテンが確定していれば他の軸が unknown でも
/// ロンできないと断定できる。
///
/// `history` は評価対象時点の facts であること。打牌後を評価する場合は
/// [`HistoryFuritenFacts::after_discard`] で補正してから渡す。
///
/// 役の有無など、フリテン以外の理由によるロン不可はここでは扱わない。
pub fn can_ron_from_furiten(
    permanent: PermanentFuriten,
    history: HistoryFuritenFacts,
) -> Option<bool> {
    combine_furiten([
        permanent.is_furiten(),
        history.same_turn,
        history.riichi_missed_win,
    ])
    .map(|furiten| !furiten)
}

/// テンパイ時の待ちについて、ツモ和了とロン和了それぞれの可否を表す pure な診断。
///
/// 待ちを2つの意味に分けて持つ。
///
/// - `structural_waits`: 構造上のアガリ牌種。見え牌で残枚数が 0 になった牌種も含み、恒常フリテン
///   判定はこちらを使う
/// - `live_waits` / `tsumo_remaining` / `tsumo_type_count`: 実際に残っているツモ可能牌。既存
///   [`Acceptance`] の値そのもので、見え牌を反映する
///
/// フリテンでもツモ側の値は書き換えない。フリテンで変わるのはロン可否
/// ([`can_ron`](Self::can_ron)) だけで、「ロンできないから残枚数を 0 にする」のように受け入れの
/// 意味を変えない。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TenpaiWaitAvailability {
    /// 構造上のアガリ牌種。残枚数 0 の牌種も含む。恒常フリテン判定に使う。
    pub structural_waits: Vec<TileType>,
    /// 実際に残っているツモ可能牌種。既存 [`Acceptance`] の受け入れ牌そのもの。
    pub live_waits: Vec<TileType>,
    /// ツモ和了できる牌の残枚数。既存 [`Acceptance::total_remaining`] そのもの。
    pub tsumo_remaining: u8,
    /// 実際に残っているツモ可能牌の種類数。既存 [`Acceptance`] の受け入れ牌種数そのもの。
    pub tsumo_type_count: usize,
    pub furiten: PermanentFuritenDiagnostic,
    /// [`can_ron`](Self::can_ron) の判定に実際に使う、この診断の評価対象時点の履歴依存フリテン。
    ///
    /// この診断は打牌後の状態なので、上位層が持つ現在時点 (打牌前) の facts とは異なる場合が
    /// ある。例えば現在 `same_turn = Some(true)` でも、自分のツモを経た今回の打牌後は
    /// `Some(false)` になる。
    pub history_furiten: HistoryFuritenFacts,
}

impl TenpaiWaitAvailability {
    /// 恒常フリテンと評価対象時点の履歴依存フリテンを合わせた総合ロン可否。
    ///
    /// 判定は [`can_ron_from_furiten`] に一元化し、call site ごとに計算し直さない。役の有無
    /// など、フリテン以外の理由によるロン不可はここでは扱わない。
    pub fn can_ron(&self) -> Option<bool> {
        can_ron_from_furiten(self.furiten.status, self.history_furiten)
    }

    pub fn permanent_furiten(&self) -> PermanentFuriten {
        self.furiten.status
    }

    /// `can_ron()` に使った評価対象時点の履歴依存フリテン。
    pub fn history_furiten(&self) -> HistoryFuritenFacts {
        self.history_furiten
    }

    pub fn discarded_waits(&self) -> &[TileType] {
        &self.furiten.discarded_waits
    }
}

/// 打牌候補1件について、その打牌でテンパイになる場合の待ちとロン可否。
///
/// ツモ側は打牌後の既存受け入れ ([`DiscardEvaluation::acceptance_after_discard`]) をそのまま使い、
/// 自分の河にはその打牌自身を足す。テンパイにならない打牌候補の `tenpai` は `None`。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscardFuritenDiagnostic {
    pub discard: TileType,
    pub tenpai: Option<TenpaiWaitAvailability>,
}

impl DiscardFuritenDiagnostic {
    /// テンパイになる場合の恒常フリテン状態。テンパイにならない打牌候補は `None`。
    pub fn permanent_furiten(&self) -> Option<PermanentFuriten> {
        self.tenpai
            .as_ref()
            .map(TenpaiWaitAvailability::permanent_furiten)
    }

    /// 自分の河と重複した待ち牌。テンパイにならない打牌候補では空。
    pub fn discarded_waits(&self) -> &[TileType] {
        self.tenpai
            .as_ref()
            .map_or(&[], TenpaiWaitAvailability::discarded_waits)
    }

    /// ロン可否の判定に使った、その打牌後の履歴依存フリテン。テンパイにならない打牌候補は
    /// `None`。
    pub fn history_furiten(&self) -> Option<HistoryFuritenFacts> {
        self.tenpai
            .as_ref()
            .map(TenpaiWaitAvailability::history_furiten)
    }
}

/// 構造上のアガリ牌種と自分の河から恒常フリテンを判定する。
///
/// 待ちのうち1種類でも自分が捨てていればフリテンで、ロンは待ち全体に対して不可になる。
///
/// `structural_waits` には残枚数 0 の牌種も含めること
/// ([`structural_acceptance_tile_types`](crate::acceptance::structural_acceptance_tile_types))。
/// 恒常フリテンはアガリ牌が自分の河にあるかどうかで決まり、その牌が山や他家に残っているか
/// どうかでは解除されない。
pub fn permanent_furiten_for_waits(
    structural_waits: &[TileType],
    own_discards: &OwnDiscards,
) -> PermanentFuritenDiagnostic {
    let discarded_waits: Vec<TileType> = structural_waits
        .iter()
        .copied()
        .filter(|wait| own_discards.contains(*wait) == Some(true))
        .collect();

    let status = if !discarded_waits.is_empty() {
        PermanentFuriten::Yes
    } else if own_discards.is_river_known() {
        PermanentFuriten::No
    } else {
        PermanentFuriten::Unknown
    };

    PermanentFuritenDiagnostic {
        status,
        discarded_waits,
    }
}

/// 既存の受け入れ・構造上のアガリ牌種・自分の河・履歴依存フリテンからテンパイの待ちと
/// ロン可否を求める。
///
/// テンパイ形 (最小向聴数 0) 以外では待ちが定まらないため `None`。門前形・副露形のどちらの
/// 受け入れでも同じ helper を使う。
///
/// `acceptance` はツモ側 (残枚数・種類数) の source of truth で、見え牌を反映した既存の値を
/// そのまま使う。`structural_waits` は残枚数 0 も含む構造上のアガリ牌種
/// ([`structural_acceptance_tile_types`](crate::acceptance::structural_acceptance_tile_types))
/// で、恒常フリテン判定だけに使う。
///
/// `history_furiten` はこの診断の評価対象時点へ補正済みの履歴依存フリテンを渡すこと
/// ([`HistoryFuritenFacts::after_discard`])。
pub fn tenpai_wait_availability<S: MinShanten>(
    acceptance: &Acceptance<S>,
    structural_waits: &[TileType],
    own_discards: &OwnDiscards,
    history_furiten: HistoryFuritenFacts,
) -> Option<TenpaiWaitAvailability> {
    if acceptance.current_min_shanten() != TENPAI_SHANTEN {
        return None;
    }

    let live_waits = acceptance.tile_types();
    Some(TenpaiWaitAvailability {
        structural_waits: structural_waits.to_vec(),
        tsumo_remaining: acceptance.total_remaining(),
        tsumo_type_count: live_waits.len(),
        live_waits,
        furiten: permanent_furiten_for_waits(structural_waits, own_discards),
        history_furiten,
    })
}

/// 打牌候補1件について、その打牌でテンパイになる場合の待ちとロン可否を求める。
///
/// `counts` は打牌前の手牌で、構造上のアガリ牌種はそこから打牌牌種を除いた手牌に対して求める。
/// ツモ側は既存の打牌評価が持つ受け入れをそのまま使い、再計算しない。
///
/// その打牌自身も自分の河に入るため、判定に使う河は `own_discards` へ打牌牌種を足したものになる。
///
/// `history_furiten` も同じく「その打牌が完了した時点」へ補正済みの値を渡すこと
/// ([`HistoryFuritenFacts::after_discard`])。現在時点の facts をそのまま渡すと、自摸 → 打牌で
/// 解除される同巡内フリテンが残ってしまう。
pub fn discard_tenpai_wait_availability(
    counts: &TileCounts,
    fixed_meld_count: FixedMeldCount,
    evaluation: &DiscardEvaluation,
    own_discards: &OwnDiscards,
    history_furiten: HistoryFuritenFacts,
) -> Option<TenpaiWaitAvailability> {
    if evaluation.acceptance_after_discard.current_min_shanten() != TENPAI_SHANTEN {
        return None;
    }

    let mut after_discard = *counts;
    after_discard.remove(evaluation.discard).ok()?;

    tenpai_wait_availability(
        &evaluation.acceptance_after_discard,
        &structural_acceptance_tile_types_with_fixed_melds(&after_discard, fixed_meld_count),
        &own_discards.with_discard(evaluation.discard),
        history_furiten,
    )
}

/// 全打牌候補分のフリテン診断。
///
/// 戻り値は `evaluations` と同じ順序・同じ件数。`counts` / `fixed_meld_count` は `evaluations` を
/// 求めたときと同じ打牌前の手牌・副露済み面子数を渡す。ツモ側は既存の打牌評価が持つ受け入れを
/// そのまま使い、向聴・受け入れ・残枚数を再計算しない。
///
/// `history_furiten` は打牌後へ補正済みの値を全候補へ同じく渡す。候補ごとに違うのは打牌牌種と
/// 打牌後の待ちだけで、「自分のツモを経た打牌か」は候補によらず同じだからである。選択候補と
/// 全候補診断で評価時点がずれないよう、補正は呼び出し側で1回だけ行う。
pub fn diagnose_discard_furiten(
    counts: &TileCounts,
    fixed_meld_count: FixedMeldCount,
    evaluations: &[DiscardEvaluation],
    own_discards: &OwnDiscards,
    history_furiten: HistoryFuritenFacts,
) -> Vec<DiscardFuritenDiagnostic> {
    evaluations
        .iter()
        .map(|evaluation| DiscardFuritenDiagnostic {
            discard: evaluation.discard,
            tenpai: discard_tenpai_wait_availability(
                counts,
                fixed_meld_count,
                evaluation,
                own_discards,
                history_furiten,
            ),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::acceptance::{
        calculate_acceptance, calculate_acceptance_with_fixed_melds,
        calculate_acceptance_with_fixed_melds_and_visible_tiles,
        calculate_acceptance_with_visible_tiles, structural_acceptance_tile_types,
    };
    use crate::discard::{
        evaluate_discards_from_tiles, evaluate_discards_from_tiles_with_visible_tiles,
    };

    #[test]
    fn history_furiten_defaults_to_unknown() {
        assert_eq!(
            HistoryFuritenFacts::default(),
            HistoryFuritenFacts {
                same_turn: None,
                riichi_missed_win: None,
            }
        );
    }

    // ---- 恒常フリテン + 履歴依存フリテン の総合ロン可否 ----

    fn history(same_turn: Option<bool>, riichi_missed_win: Option<bool>) -> HistoryFuritenFacts {
        HistoryFuritenFacts {
            same_turn,
            riichi_missed_win,
        }
    }

    #[test]
    fn any_confirmed_furiten_axis_makes_ron_impossible() {
        // 1軸でもフリテンが確定していれば、他の軸が unknown でもロンできないと断定できる。
        assert_eq!(
            can_ron_from_furiten(PermanentFuriten::Yes, history(None, None)),
            Some(false)
        );
        assert_eq!(
            can_ron_from_furiten(PermanentFuriten::Unknown, history(Some(true), None)),
            Some(false)
        );
        assert_eq!(
            can_ron_from_furiten(PermanentFuriten::Unknown, history(None, Some(true))),
            Some(false)
        );
    }

    #[test]
    fn ron_is_possible_only_when_every_axis_is_confirmed_not_furiten() {
        assert_eq!(
            can_ron_from_furiten(PermanentFuriten::No, history(Some(false), Some(false))),
            Some(true)
        );
    }

    #[test]
    fn an_unknown_axis_keeps_the_ron_availability_unknown() {
        // unknown を false と推測しない。フリテン確定の軸が1つも無ければ unknown のまま。
        assert_eq!(
            can_ron_from_furiten(PermanentFuriten::No, history(Some(false), None)),
            None
        );
        assert_eq!(
            can_ron_from_furiten(PermanentFuriten::No, history(None, Some(false))),
            None
        );
        assert_eq!(
            can_ron_from_furiten(PermanentFuriten::Unknown, history(Some(false), Some(false))),
            None
        );
    }

    #[test]
    fn a_confirmed_furiten_axis_wins_over_unknown_axes_in_both_directions() {
        // 「フリテン確定が1つでもあれば Some(false)」が unknown より優先されることを、恒常側と
        // 履歴側の両方向で固定する。
        assert_eq!(
            can_ron_from_furiten(PermanentFuriten::Yes, history(Some(false), Some(false))),
            Some(false)
        );
        assert_eq!(
            can_ron_from_furiten(PermanentFuriten::No, history(Some(true), None)),
            Some(false)
        );
    }

    #[test]
    fn history_furiten_is_furiten_follows_the_same_combination_rule() {
        assert_eq!(history(Some(true), None).is_furiten(), Some(true));
        assert_eq!(history(None, Some(true)).is_furiten(), Some(true));
        assert_eq!(history(Some(false), Some(false)).is_furiten(), Some(false));
        assert_eq!(history(Some(false), None).is_furiten(), None);
        assert_eq!(HistoryFuritenFacts::default().is_furiten(), None);
    }

    // ---- 評価時点の補正 ----

    #[test]
    fn an_own_draw_discard_clears_the_same_turn_furiten() {
        // 自摸 → 打牌を終えた時点では同巡内フリテンが解除される。元が true でも unknown でも
        // Some(false) と確定できる。
        for same_turn in [Some(true), Some(false), None] {
            assert_eq!(
                history(same_turn, Some(false))
                    .after_discard(true)
                    .same_turn,
                Some(false)
            );
        }
    }

    #[test]
    fn an_own_draw_discard_keeps_the_riichi_missed_win_furiten() {
        // リーチ後見逃しは局終了まで続くので、自摸後の打牌でも維持する。
        for riichi_missed_win in [Some(true), Some(false), None] {
            assert_eq!(
                history(Some(true), riichi_missed_win)
                    .after_discard(true)
                    .riichi_missed_win,
                riichi_missed_win
            );
        }
    }

    #[test]
    fn a_discard_without_a_confirmed_own_draw_keeps_the_history_facts() {
        // Chi / Pon 後の打牌や自摸を確認できない経路では、同巡内フリテンを解除しない。
        for facts in [
            history(Some(true), Some(false)),
            history(None, Some(false)),
            HistoryFuritenFacts::default(),
        ] {
            assert_eq!(facts.after_discard(false), facts);
        }
    }

    fn tile(s: &str) -> TileType {
        TileType::from_mjai_type_str(s).unwrap()
    }

    fn tiles(strings: &[&str]) -> Vec<TileType> {
        strings.iter().map(|s| tile(s)).collect()
    }

    fn counts(strings: &[&str]) -> TileCounts {
        TileCounts::from_tile_types(strings.iter().map(|s| tile(s)))
    }

    fn ids(values: &[u8]) -> Vec<TileId> {
        values.iter().map(|&v| TileId::new(v).unwrap()).collect()
    }

    fn river(strings: &[&str]) -> OwnDiscards {
        OwnDiscards::from_river_tile_types(strings.iter().map(|s| tile(s)))
    }

    fn fixed(value: u8) -> FixedMeldCount {
        FixedMeldCount::new(value).unwrap()
    }

    // 履歴依存フリテンが両軸とも非該当だと確認済みの facts。恒常フリテンだけを見る既存テストは
    // これを渡し、総合ロン可否が恒常フリテンで決まる状態にする。
    fn no_history_furiten() -> HistoryFuritenFacts {
        HistoryFuritenFacts {
            same_turn: Some(false),
            riichi_missed_win: Some(false),
        }
    }

    // 手牌と見え牌からテンパイの待ち診断を組み立てる。ツモ側は既存の受け入れ、恒常フリテン判定は
    // 既存の構造上の受け入れ牌種で、どちらも production と同じ入口から求める。
    fn availability_with_visible(
        counts: &TileCounts,
        visible: &[TileId],
        own_discards: &OwnDiscards,
    ) -> TenpaiWaitAvailability {
        tenpai_wait_availability(
            &calculate_acceptance_with_visible_tiles(counts, visible),
            &structural_acceptance_tile_types(counts),
            own_discards,
            no_history_furiten(),
        )
        .expect("テンパイ形である")
    }

    fn availability(counts: &TileCounts, own_discards: &OwnDiscards) -> TenpaiWaitAvailability {
        availability_with_visible(counts, &[], own_discards)
    }

    // 123m456m789m 123p 5s の単騎待ち。待ちは 5s だけ。
    fn tanki_tenpai() -> TileCounts {
        counts(&[
            "1m", "2m", "3m", "4m", "5m", "6m", "7m", "8m", "9m", "1p", "2p", "3p", "5s",
        ])
    }

    // 123m456m 3456789s の3面待ち。待ちは 3s / 6s / 9s。
    fn three_sided_tenpai() -> TileCounts {
        TileCounts::from_tiles(three_sided_tenpai_tiles().iter().copied())
    }

    // 3面待ちと同じ手牌の物理牌。3s は 80 の1枚だけ持ち、残り3枚 (81 / 82 / 83) を見え牌にできる。
    fn three_sided_tenpai_tiles() -> Vec<TileId> {
        ids(&[0, 4, 8, 12, 17, 20, 80, 84, 89, 92, 96, 100, 104])
    }

    #[test]
    fn tanki_wait_in_the_own_river_is_permanent_furiten() {
        let availability = availability(&tanki_tenpai(), &river(&["1m", "5s", "E"]));

        assert_eq!(availability.structural_waits, tiles(&["5s"]));
        assert_eq!(availability.permanent_furiten(), PermanentFuriten::Yes);
        assert_eq!(availability.discarded_waits(), tiles(&["5s"]));
        assert_eq!(availability.can_ron(), Some(false));
    }

    #[test]
    fn permanent_furiten_keeps_the_tsumo_side_of_the_existing_acceptance() {
        // フリテンでもツモ和了の残枚数・種類数は既存受け入れのまま。
        let counts = tanki_tenpai();
        let acceptance = calculate_acceptance(&counts);
        let availability = availability(&counts, &river(&["5s"]));

        assert_eq!(availability.tsumo_remaining, acceptance.total_remaining());
        assert_eq!(availability.tsumo_type_count, acceptance.tiles.len());
        assert_eq!(availability.live_waits, acceptance.tile_types());
        assert_eq!(availability.tsumo_remaining, 3);
    }

    #[test]
    fn one_discarded_wait_blocks_the_whole_multi_sided_wait() {
        let availability = availability(&three_sided_tenpai(), &river(&["6s"]));

        assert_eq!(availability.structural_waits, tiles(&["3s", "6s", "9s"]));
        assert_eq!(availability.tsumo_type_count, 3);
        assert_eq!(availability.discarded_waits(), tiles(&["6s"]));
        assert_eq!(availability.can_ron(), Some(false));
    }

    #[test]
    fn an_empty_own_river_is_not_furiten() {
        let availability = availability(&three_sided_tenpai(), &river(&[]));

        assert_eq!(availability.permanent_furiten(), PermanentFuriten::No);
        assert!(availability.discarded_waits().is_empty());
        assert_eq!(availability.can_ron(), Some(true));
    }

    #[test]
    fn tiles_in_the_own_river_that_are_not_waits_are_not_furiten() {
        let availability = availability(&three_sided_tenpai(), &river(&["9m", "5p", "E", "2s"]));

        assert_eq!(availability.permanent_furiten(), PermanentFuriten::No);
        assert_eq!(availability.can_ron(), Some(true));
    }

    #[test]
    fn only_the_own_river_decides_furiten() {
        // 他家の河や見え牌は判定に渡さない。同じ待ちで自分の河だけを変えると結果が変わる。
        let counts = three_sided_tenpai();

        assert_eq!(
            availability(&counts, &river(&["1m", "9p"])).permanent_furiten(),
            PermanentFuriten::No
        );
        assert_eq!(
            availability(&counts, &river(&["1m", "9p", "9s"])).permanent_furiten(),
            PermanentFuriten::Yes
        );
    }

    #[test]
    fn a_discarded_red_five_is_furiten_for_the_black_five_wait() {
        // 123m456m789m 123p 5p 待ちで、河には赤5p だけがある。
        let counts = counts(&[
            "1m", "2m", "3m", "4m", "5m", "6m", "7m", "8m", "9m", "1p", "2p", "3p", "5p",
        ]);
        let red_five_pin = TileId::new(52).unwrap();
        assert!(red_five_pin.is_red());
        assert_eq!(red_five_pin.tile_type(), tile("5p"));

        let availability = availability(&counts, &OwnDiscards::from_river(&[red_five_pin]));

        assert_eq!(availability.structural_waits, tiles(&["5p"]));
        assert_eq!(availability.permanent_furiten(), PermanentFuriten::Yes);
        assert_eq!(availability.discarded_waits(), tiles(&["5p"]));
    }

    #[test]
    fn an_unknown_own_river_is_not_reported_as_non_furiten() {
        let availability = availability(&three_sided_tenpai(), &OwnDiscards::unknown());

        assert_eq!(availability.permanent_furiten(), PermanentFuriten::Unknown);
        assert_eq!(availability.can_ron(), None);
        assert!(availability.discarded_waits().is_empty());
        // 待ち自体は既存の判定のまま分かる。
        assert_eq!(availability.structural_waits, tiles(&["3s", "6s", "9s"]));
    }

    #[test]
    fn an_unknown_river_still_reports_furiten_for_a_certain_discard() {
        // 河が不明でも、これから切る牌が待ちに含まれていればフリテンだと断定できる。
        let availability = availability(
            &three_sided_tenpai(),
            &OwnDiscards::unknown().with_discard(tile("9s")),
        );

        assert_eq!(availability.permanent_furiten(), PermanentFuriten::Yes);
        assert_eq!(availability.discarded_waits(), tiles(&["9s"]));
    }

    #[test]
    fn planned_discards_can_be_appended_for_a_lookahead_branch() {
        // 「既存の河 + 1手目の打牌 + 2手目の打牌」を河として渡せる。
        let own_discards = river(&["E"]).with_discards(tiles(&["9m", "3s"]));

        assert_eq!(
            own_discards.tile_types(),
            tiles(&["E", "9m", "3s"]).as_slice()
        );
        let availability = availability(&three_sided_tenpai(), &own_discards);
        assert_eq!(availability.discarded_waits(), tiles(&["3s"]));
        assert_eq!(availability.can_ron(), Some(false));
    }

    // ---- 見え牌と構造上の待ちの分離 ----

    #[test]
    fn visible_tiles_reduce_the_remaining_but_not_the_furiten_decision() {
        // 見え牌はツモ可能残枚数だけに効く。単騎待ちが4枚とも見えて受け入れが空になる境界でも、
        // フリテン判定は自分の河だけで決まる。
        let hand = ids(&[0, 4, 8, 12, 17, 20, 24, 28, 32, 36, 40, 44, 89]);
        let counts = TileCounts::from_tiles(hand.iter().copied());
        let mut two_seen = hand.clone();
        two_seen.extend(ids(&[90, 91]));
        let mut all_seen = hand.clone();
        all_seen.extend(ids(&[88, 90, 91]));

        let remaining_of = |visible: &[TileId]| {
            calculate_acceptance_with_visible_tiles(&counts, visible).total_remaining()
        };
        assert_eq!(remaining_of(&[]), 3);
        assert_eq!(remaining_of(&two_seen), 1);
        assert_eq!(remaining_of(&all_seen), 0);

        for own_discards in [river(&["5s"]), OwnDiscards::unknown(), river(&["1m"])] {
            let expected = availability(&counts, &own_discards);
            for visible in [two_seen.as_slice(), all_seen.as_slice()] {
                let actual = availability_with_visible(&counts, visible, &own_discards);
                assert_eq!(actual.furiten, expected.furiten);
                assert_eq!(actual.structural_waits, expected.structural_waits);
                assert_eq!(actual.can_ron(), expected.can_ron());
                assert!(actual.tsumo_remaining < expected.tsumo_remaining);
            }

            // 4枚とも見えている場合、既存受け入れは空になるが構造上の待ちは残る。
            let dead = availability_with_visible(&counts, &all_seen, &own_discards);
            assert_eq!(dead.tsumo_remaining, 0);
            assert_eq!(dead.tsumo_type_count, 0);
            assert!(dead.live_waits.is_empty());
            assert_eq!(dead.structural_waits, tiles(&["5s"]));
        }
    }

    #[test]
    fn a_fully_visible_discarded_wait_still_makes_the_hand_furiten() {
        // 構造上の待ち 3s / 6s / 9s のうち、3s を自分が捨てていて 3s が4枚とも見えている局面。
        // 3s は既存受け入れから消えるが、恒常フリテンは解除されない。
        let counts = three_sided_tenpai();
        let mut visible = three_sided_tenpai_tiles();
        visible.extend(ids(&[81, 82, 83]));
        let own_discards = OwnDiscards::from_river(&ids(&[81]));

        let availability = availability_with_visible(&counts, &visible, &own_discards);

        assert_eq!(availability.structural_waits, tiles(&["3s", "6s", "9s"]));
        assert_eq!(availability.live_waits, tiles(&["6s", "9s"]));
        assert_eq!(availability.permanent_furiten(), PermanentFuriten::Yes);
        assert_eq!(availability.can_ron(), Some(false));
        assert_eq!(availability.discarded_waits(), tiles(&["3s"]));

        // ツモ側は見え牌を反映した既存受け入れのまま。
        let acceptance = calculate_acceptance_with_visible_tiles(&counts, &visible);
        assert_eq!(availability.tsumo_remaining, acceptance.total_remaining());
        assert_eq!(availability.tsumo_type_count, acceptance.tiles.len());
        assert_eq!(availability.tsumo_remaining, 6);
        assert_eq!(availability.tsumo_type_count, 2);
    }

    #[test]
    fn the_furiten_decision_does_not_change_when_the_last_copy_becomes_visible() {
        // 同じ手牌・同じ自分の河で、待ち A の残枚数が 1 → 0 になってもフリテン判定は変わらない。
        let counts = three_sided_tenpai();
        let own_discards = OwnDiscards::from_river(&ids(&[81]));

        let mut one_left = three_sided_tenpai_tiles();
        one_left.extend(ids(&[81, 82]));
        let mut none_left = one_left.clone();
        none_left.extend(ids(&[83]));

        let with_one_left = availability_with_visible(&counts, &one_left, &own_discards);
        let with_none_left = availability_with_visible(&counts, &none_left, &own_discards);

        assert_eq!(with_one_left.live_waits, tiles(&["3s", "6s", "9s"]));
        assert_eq!(with_none_left.live_waits, tiles(&["6s", "9s"]));
        assert_ne!(
            with_one_left.tsumo_remaining,
            with_none_left.tsumo_remaining
        );

        assert_eq!(with_one_left.furiten, with_none_left.furiten);
        assert_eq!(
            with_one_left.permanent_furiten(),
            with_none_left.permanent_furiten()
        );
        assert_eq!(
            with_one_left.discarded_waits(),
            with_none_left.discarded_waits()
        );
        assert_eq!(with_one_left.can_ron(), with_none_left.can_ron());
        assert_eq!(with_none_left.permanent_furiten(), PermanentFuriten::Yes);
    }

    #[test]
    fn structural_waits_ignore_visible_tiles_entirely() {
        let counts = three_sided_tenpai();
        let mut visible = three_sided_tenpai_tiles();
        visible.extend(ids(&[81, 82, 83]));

        assert_eq!(
            structural_acceptance_tile_types(&counts),
            tiles(&["3s", "6s", "9s"])
        );
        assert_eq!(
            availability_with_visible(&counts, &visible, &river(&[])).structural_waits,
            availability(&counts, &river(&[])).structural_waits
        );
    }

    // ---- 特殊形と副露形 ----

    #[test]
    fn chiitoitsu_tenpai_uses_the_acceptance_wait() {
        // 七対子テンパイ。待ちは単騎の E だけ。
        let counts = counts(&[
            "1m", "1m", "2m", "2m", "3m", "3m", "4p", "4p", "5p", "5p", "6s", "6s", "E",
        ]);
        assert_eq!(calculate_acceptance(&counts).current.chiitoitsu, 0);

        assert_eq!(
            availability(&counts, &river(&["E"])).permanent_furiten(),
            PermanentFuriten::Yes
        );
        assert_eq!(
            availability(&counts, &river(&["S"])).permanent_furiten(),
            PermanentFuriten::No
        );
    }

    #[test]
    fn kokushi_thirteen_wait_is_furiten_when_any_yaochu_was_discarded() {
        // 国士13面待ち。1種類でも自分が捨てていればロン不可。
        let counts = counts(&[
            "1m", "9m", "1p", "9p", "1s", "9s", "E", "S", "W", "N", "P", "F", "C",
        ]);
        assert_eq!(structural_acceptance_tile_types(&counts).len(), 13);

        let furiten = availability(&counts, &river(&["C"]));
        assert_eq!(furiten.tsumo_type_count, 13);
        assert_eq!(furiten.tsumo_remaining, 39);
        assert_eq!(furiten.discarded_waits(), tiles(&["C"]));
        assert_eq!(furiten.can_ron(), Some(false));

        assert_eq!(
            availability(&counts, &river(&["2m", "5p"])).permanent_furiten(),
            PermanentFuriten::No
        );
    }

    #[test]
    fn fixed_meld_tenpai_keeps_the_effective_acceptance_semantics() {
        // 副露1つの通常形テンパイ。EffectiveAcceptance をそのままツモ側として使う。
        let counts = counts(&["1m", "2m", "3m", "4m", "5m", "6m", "7m", "8m", "9m", "5p"]);
        let acceptance = calculate_acceptance_with_fixed_melds(&counts, fixed(1));
        let structural = structural_acceptance_tile_types_with_fixed_melds(&counts, fixed(1));
        assert_eq!(acceptance.current.concealed(), None);
        assert_eq!(structural, tiles(&["5p"]));

        let furiten = tenpai_wait_availability(
            &acceptance,
            &structural,
            &river(&["5p"]),
            no_history_furiten(),
        )
        .expect("副露テンパイ形である");
        assert_eq!(furiten.structural_waits, tiles(&["5p"]));
        assert_eq!(furiten.live_waits, tiles(&["5p"]));
        assert_eq!(furiten.tsumo_remaining, acceptance.total_remaining());
        assert_eq!(furiten.can_ron(), Some(false));

        let non_furiten = tenpai_wait_availability(
            &acceptance,
            &structural,
            &river(&["1p"]),
            no_history_furiten(),
        )
        .expect("副露テンパイ形である");
        assert_eq!(non_furiten.can_ron(), Some(true));
    }

    #[test]
    fn fixed_meld_tenpai_separates_the_visible_remaining_from_the_furiten_decision() {
        let hand = ids(&[0, 4, 8, 12, 17, 20, 24, 28, 32, 53]);
        let counts = TileCounts::from_tiles(hand.iter().copied());
        let structural = structural_acceptance_tile_types_with_fixed_melds(&counts, fixed(1));

        let mut partly_visible = hand.clone();
        partly_visible.extend(ids(&[54, 55]));
        let mut fully_visible = partly_visible.clone();
        fully_visible.extend(ids(&[52]));

        let availability_of = |visible: &[TileId]| {
            tenpai_wait_availability(
                &calculate_acceptance_with_fixed_melds_and_visible_tiles(
                    &counts,
                    fixed(1),
                    visible,
                ),
                &structural,
                &river(&["5p"]),
                no_history_furiten(),
            )
            .expect("副露テンパイ形である")
        };

        let partly = availability_of(&partly_visible);
        let fully = availability_of(&fully_visible);

        assert_eq!(partly.tsumo_remaining, 1);
        assert_eq!(fully.tsumo_remaining, 0);
        assert!(fully.live_waits.is_empty());
        assert_eq!(fully.structural_waits, tiles(&["5p"]));
        assert_eq!(partly.furiten, fully.furiten);
        assert_eq!(fully.can_ron(), Some(false));
    }

    #[test]
    fn a_hand_that_is_not_tenpai_has_no_wait() {
        let counts = counts(&[
            "1m", "3m", "5m", "7m", "9m", "1p", "3p", "5p", "7p", "9p", "1s", "3s", "5s",
        ]);
        let acceptance = calculate_acceptance(&counts);
        assert!(acceptance.current.min() > 0);

        assert_eq!(
            tenpai_wait_availability(
                &acceptance,
                &structural_acceptance_tile_types(&counts),
                &river(&[]),
                no_history_furiten(),
            ),
            None
        );
    }

    // ---- 打牌候補ごとの診断 ----

    #[test]
    fn discard_furiten_counts_the_candidate_discard_itself() {
        // 123m456m789m 123p 5s5s から 5s を切ると 5s 単騎テンパイになり、その 5s が河に入る。
        let hand = ids(&[0, 4, 8, 12, 17, 20, 24, 28, 32, 36, 40, 44, 89, 90]);
        let counts = TileCounts::from_tiles(hand.iter().copied());
        let evaluations = evaluate_discards_from_tiles(&hand);
        let diagnostics = diagnose_discard_furiten(
            &counts,
            FixedMeldCount::NONE,
            &evaluations,
            &river(&[]),
            no_history_furiten(),
        );

        assert_eq!(diagnostics.len(), evaluations.len());
        let five_sou = diagnostics
            .iter()
            .find(|diagnostic| diagnostic.discard == tile("5s"))
            .expect("打 5s の候補がある");
        let tenpai = five_sou.tenpai.as_ref().expect("テンパイになる");
        assert_eq!(tenpai.structural_waits, tiles(&["5s"]));
        assert_eq!(five_sou.discarded_waits(), tiles(&["5s"]));
        assert_eq!(five_sou.permanent_furiten(), Some(PermanentFuriten::Yes));
        assert_eq!(tenpai.can_ron(), Some(false));
    }

    #[test]
    fn discard_furiten_is_absent_for_candidates_that_do_not_reach_tenpai() {
        let hand = ids(&[0, 8, 20, 28, 48, 56, 68, 76, 88, 100, 108, 116, 124, 132]);
        let counts = TileCounts::from_tiles(hand.iter().copied());
        let evaluations = evaluate_discards_from_tiles(&hand);
        let diagnostics = diagnose_discard_furiten(
            &counts,
            FixedMeldCount::NONE,
            &evaluations,
            &river(&[]),
            no_history_furiten(),
        );

        assert!(!diagnostics.is_empty());
        assert!(diagnostics.iter().all(|diagnostic| {
            diagnostic.tenpai.is_none()
                && diagnostic.permanent_furiten().is_none()
                && diagnostic.discarded_waits().is_empty()
        }));
    }

    #[test]
    fn discard_furiten_keeps_the_existing_acceptance_of_the_candidate() {
        let hand = ids(&[0, 4, 8, 12, 17, 20, 24, 28, 32, 36, 40, 44, 89, 90]);
        let counts = TileCounts::from_tiles(hand.iter().copied());
        let evaluations = evaluate_discards_from_tiles(&hand);
        let diagnostics = diagnose_discard_furiten(
            &counts,
            FixedMeldCount::NONE,
            &evaluations,
            &river(&["1m"]),
            no_history_furiten(),
        );

        for (diagnostic, evaluation) in diagnostics.iter().zip(evaluations.iter()) {
            assert_eq!(diagnostic.discard, evaluation.discard);
            let Some(tenpai) = diagnostic.tenpai.as_ref() else {
                assert_ne!(evaluation.min_shanten_after_discard(), 0);
                continue;
            };
            assert_eq!(
                tenpai.tsumo_remaining,
                evaluation.acceptance_total_remaining()
            );
            assert_eq!(tenpai.tsumo_type_count, evaluation.acceptance_type_count());
            assert_eq!(
                tenpai.live_waits,
                evaluation.acceptance_after_discard.tile_types()
            );
        }
    }

    #[test]
    fn discard_furiten_uses_structural_waits_when_a_wait_is_fully_visible() {
        // 打 1p で 3s / 6s / 9s の3面待ちテンパイ。3s は自分の河にあり4枚とも見えている。
        let mut hand = three_sided_tenpai_tiles();
        hand.push(ids(&[36])[0]);
        let counts = TileCounts::from_tiles(hand.iter().copied());
        let mut visible = hand.clone();
        visible.extend(ids(&[81, 82, 83]));

        let evaluations =
            evaluate_discards_from_tiles_with_visible_tiles(&hand, &[], None, None, &visible);
        let diagnostics = diagnose_discard_furiten(
            &counts,
            FixedMeldCount::NONE,
            &evaluations,
            &OwnDiscards::from_river(&ids(&[81])),
            no_history_furiten(),
        );

        let one_pin = diagnostics
            .iter()
            .find(|diagnostic| diagnostic.discard == tile("1p"))
            .expect("打 1p の候補がある");
        let tenpai = one_pin.tenpai.as_ref().expect("テンパイになる");
        let evaluation = evaluations
            .iter()
            .find(|evaluation| evaluation.discard == tile("1p"))
            .expect("打 1p の評価がある");

        assert_eq!(tenpai.structural_waits, tiles(&["3s", "6s", "9s"]));
        assert_eq!(tenpai.live_waits, tiles(&["6s", "9s"]));
        assert_eq!(one_pin.permanent_furiten(), Some(PermanentFuriten::Yes));
        assert_eq!(one_pin.discarded_waits(), tiles(&["3s"]));
        assert_eq!(tenpai.can_ron(), Some(false));
        assert_eq!(
            tenpai.tsumo_remaining,
            evaluation.acceptance_total_remaining()
        );
        assert_eq!(tenpai.tsumo_type_count, evaluation.acceptance_type_count());
    }

    #[test]
    fn an_unknown_river_leaves_the_discard_diagnostic_unknown() {
        let hand = ids(&[0, 4, 8, 12, 17, 20, 24, 28, 32, 36, 40, 44, 89, 108]);
        let counts = TileCounts::from_tiles(hand.iter().copied());
        let evaluations = evaluate_discards_from_tiles(&hand);
        let diagnostics = diagnose_discard_furiten(
            &counts,
            FixedMeldCount::NONE,
            &evaluations,
            &OwnDiscards::unknown(),
            no_history_furiten(),
        );

        let east = diagnostics
            .iter()
            .find(|diagnostic| diagnostic.discard == tile("E"))
            .expect("打 E の候補がある");
        assert_eq!(east.permanent_furiten(), Some(PermanentFuriten::Unknown));
        assert_eq!(
            east.tenpai.as_ref().expect("テンパイになる").can_ron(),
            None
        );
    }

    // ---- OwnDiscards ----

    #[test]
    fn own_discards_report_containment_per_tile_type() {
        let known = river(&["1m", "E"]);
        assert_eq!(known.contains(tile("1m")), Some(true));
        assert_eq!(known.contains(tile("2m")), Some(false));
        assert!(known.is_river_known());

        let unknown = OwnDiscards::unknown();
        assert_eq!(unknown.contains(tile("1m")), None);
        assert!(!unknown.is_river_known());
        assert!(unknown.tile_types().is_empty());

        let unknown_with_discard = unknown.with_discard(tile("1m"));
        assert_eq!(unknown_with_discard.contains(tile("1m")), Some(true));
        assert_eq!(unknown_with_discard.contains(tile("2m")), None);
        assert!(!unknown_with_discard.is_river_known());
    }

    #[test]
    fn optional_river_maps_none_to_unknown() {
        assert_eq!(
            OwnDiscards::from_optional_river(None),
            OwnDiscards::unknown()
        );

        let river_tiles = ids(&[0, 16]);
        assert_eq!(
            OwnDiscards::from_optional_river(Some(&river_tiles)),
            OwnDiscards::from_river(&river_tiles)
        );
        // 赤5m は黒5m と同じ牌種として保持する。
        assert_eq!(
            OwnDiscards::from_river(&river_tiles).tile_types(),
            tiles(&["1m", "5m"]).as_slice()
        );
    }
}
