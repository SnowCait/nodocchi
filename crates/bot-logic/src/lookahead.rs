//! 打牌候補ごとの2手先診断。
//!
//! 「現在打牌 → その打牌後の各受け入れ牌を1枚ツモった仮想手牌 → 既存打牌評価による次の最良打牌」
//! を構造化して返す解析専用の pure な基盤。向聴・受け入れ・一向聴形分類・文脈反映・打牌比較は
//! すべて既存実装を呼び出し、2手先専用の計算器は持たない。
//!
//! # 仮想ツモ牌の物理牌
//!
//! 受け入れ ([`EffectiveAcceptanceTile`]) は 34 種の [`TileType`] 単位なので、仮想ツモ牌の物理牌
//! ([`TileId`]) は決まらない。仮想ツモ牌以外の牌は現在の手牌の物理牌をそのまま引き継ぐため、
//! 通常打牌評価と同じ文脈で評価できる。
//!
//! - 副露済み面子数・見え牌・場風・自風・ドラ表示牌: すべて通常打牌評価と同じ値を反映する
//! - 役牌 (`discarded_value_honor_count`): 牌種と場風・自風だけで決まるので必ず反映する
//! - 通常ドラ (ドラ表示牌が示す分): 牌種だけで決まるので必ず反映する
//! - 赤5 (`discards_red_five` と赤ドラ分): 仮想ツモ牌の牌種を2手目に切る候補でだけ解決できない
//!
//! 最後の未解決分はさらに絞り込める。手牌に同種の黒牌が残っていれば通常打牌評価も黒を選ぶので
//! 結果は一致し、赤1枚しか無い場合もその赤が手牌に見えている以上、仮想ツモは黒に確定する。
//! したがって実際に未解決なのは「仮想ツモ牌と同種の牌が手牌に1枚も残っていない場合」だけで、
//! そのとき赤5扱いにはせず赤ドラ分も加算しない。赤5の確率モデルなど新しい仕様は導入しない。

use crate::acceptance::EffectiveAcceptanceTile;
use crate::discard::{
    CandidateSeen, DecorationContext, DiscardEvaluation, ShapePenaltyMode, decorate_evaluations,
    discarded_tile_id_for_type, evaluate_discards_with_seen, select_best,
};
use crate::iishanten::IishantenShape;
use crate::shanten::{EffectiveShanten, FixedMeldCount};
use crate::tile::{TileId, TileType};
use crate::tile_counts::TileCounts;

/// 現在の打牌候補1件について、その打牌後の受け入れ牌を1枚ツモった仮想手牌を既存打牌評価へ
/// かけた2手先診断。
///
/// 解析専用の pure なデータであり、打牌選択・押し引き・鳴き・リーチ判断のどれにも使用しない。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscardLookaheadDiagnostic {
    /// 現在の打牌候補の牌種。
    pub discard: TileType,
    /// 現在打牌後の受け入れ牌ごとの2手先診断。順序と対象牌は現在打牌後の受け入れと同じ。
    pub draws: Vec<DrawLookaheadDiagnostic>,
}

impl DiscardLookaheadDiagnostic {
    pub fn draw(&self, tile: TileType) -> Option<&DrawLookaheadDiagnostic> {
        self.draws.iter().find(|draw| draw.draw == tile)
    }
}

/// 現在打牌後の受け入れ牌1枚を仮想的にツモった場合の2手先診断。
///
/// `draw` / `remaining` / `shanten_after_draw` は現在打牌の [`DiscardEvaluation`] が持つ
/// 受け入れ ([`EffectiveAcceptanceTile`]) の値そのもので、診断のために再計算しない。
/// `remaining` は牌種ごとの残枚数を生データのまま保持し、期待値や加重平均へ潰さない。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DrawLookaheadDiagnostic {
    pub draw: TileType,
    pub remaining: u8,
    pub shanten_after_draw: EffectiveShanten,
    /// 仮想ツモ後の手牌に既存打牌評価と既存比較順を適用した最良打牌。打牌候補が1件も無い場合だけ
    /// `None`。数値のコピーではなく評価そのものを保持する。
    ///
    /// 副露済み面子数・見え牌・場風・自風・ドラ表示牌は現在の打牌評価と同じ値を反映する。
    /// 赤5だけはモジュールの「仮想ツモ牌の物理牌」どおり解決できない場合がある。
    pub next_discard: Option<DiscardEvaluation>,
}

impl DrawLookaheadDiagnostic {
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

// 2手先診断の評価入力。通常打牌評価が使う値と同じものだけを持ち、上位層の局面型には依存しない。
struct LookaheadInputs<'a> {
    counts: TileCounts,
    tiles: &'a [TileId],
    fixed_meld_count: FixedMeldCount,
    dora_indicators: &'a [TileId],
    round_wind: Option<TileType>,
    seat_wind: Option<TileType>,
    seen: CandidateSeen,
}

/// 副露済み面子数と場風・自風・ドラ表示牌を考慮して全打牌候補の2手先診断を構築する。
///
/// `tiles` は打牌前の全手牌（物理牌）、`evaluations` はその手牌に対する既存の打牌候補評価で、
/// `fixed_meld_count` / `dora_indicators` / `round_wind` / `seat_wind` はその評価に使ったものと
/// 同じ値を渡す。現在打牌後の受け入れは `evaluations` が持つ値をそのまま使い、診断のために
/// 再計算しない。
///
/// 2手目の打牌評価・受け入れ・向聴・一向聴形分類・文脈反映・比較は既存の打牌評価経路をそのまま
/// 呼び出す。2手先専用の shanten / acceptance / comparator / shape evaluator は持たない。
pub fn diagnose_lookahead_with_fixed_melds(
    tiles: &[TileId],
    fixed_meld_count: FixedMeldCount,
    dora_indicators: &[TileId],
    round_wind: Option<TileType>,
    seat_wind: Option<TileType>,
    evaluations: &[DiscardEvaluation],
) -> LookaheadDiagnostic {
    diagnose_lookahead(
        &LookaheadInputs {
            counts: TileCounts::from_tiles(tiles.iter().copied()),
            tiles,
            fixed_meld_count,
            dora_indicators,
            round_wind,
            seat_wind,
            seen: CandidateSeen::hand_only(),
        },
        evaluations,
    )
}

/// visible tiles も考慮して全打牌候補の2手先診断を構築する。
///
/// 2手目の残枚数は「1手目の打牌前に見えていた牌 + 1手目に切った牌1枚」を seen として求める。
/// 1手目に切った牌は2手目時点で見え牌なので、山に残っている牌として数え直さない。
/// visible tiles は自分の手牌を含むため、既存の残枚数計算と同じ手牌差し引きで二重計上を防ぐ。
///
/// `visible_tiles` が空の場合は [`diagnose_lookahead_with_fixed_melds`] と同じ seen 扱いになる。
#[allow(clippy::too_many_arguments)]
pub fn diagnose_lookahead_with_fixed_melds_and_visible_tiles(
    tiles: &[TileId],
    fixed_meld_count: FixedMeldCount,
    dora_indicators: &[TileId],
    round_wind: Option<TileType>,
    seat_wind: Option<TileType>,
    visible_tiles: &[TileId],
    evaluations: &[DiscardEvaluation],
) -> LookaheadDiagnostic {
    if visible_tiles.is_empty() {
        return diagnose_lookahead_with_fixed_melds(
            tiles,
            fixed_meld_count,
            dora_indicators,
            round_wind,
            seat_wind,
            evaluations,
        );
    }

    let counts = TileCounts::from_tiles(tiles.iter().copied());
    diagnose_lookahead(
        &LookaheadInputs {
            counts,
            tiles,
            fixed_meld_count,
            dora_indicators,
            round_wind,
            seat_wind,
            seen: CandidateSeen::from_visible_tiles(&counts, visible_tiles),
        },
        evaluations,
    )
}

fn diagnose_lookahead(
    inputs: &LookaheadInputs,
    evaluations: &[DiscardEvaluation],
) -> LookaheadDiagnostic {
    LookaheadDiagnostic {
        candidates: evaluations
            .iter()
            .map(|evaluation| lookahead_for_candidate(inputs, evaluation))
            .collect(),
    }
}

// 現在の打牌候補1件分の2手先診断。入力は変更せず、打牌後の手牌を copy で作る。
fn lookahead_for_candidate(
    inputs: &LookaheadInputs,
    evaluation: &DiscardEvaluation,
) -> DiscardLookaheadDiagnostic {
    let mut after_discard = inputs.counts;
    if after_discard.remove(evaluation.discard).is_err() {
        return DiscardLookaheadDiagnostic {
            discard: evaluation.discard,
            draws: Vec::new(),
        };
    }

    // 1手目に切った牌は2手目時点で見え牌になる。
    let next_seen = inputs.seen.after_discard(evaluation.discard);
    // 1手目に切る物理牌も通常打牌評価と同じ規則で決め、2手目の物理牌一覧から外す。
    let next_tiles = tiles_after_discard(inputs.tiles, evaluation.discard);

    let draws = evaluation
        .acceptance_after_discard
        .tiles
        .iter()
        .map(|tile| lookahead_for_draw(inputs, &after_discard, &next_tiles, &next_seen, tile))
        .collect();

    DiscardLookaheadDiagnostic {
        discard: evaluation.discard,
        draws,
    }
}

// 受け入れ牌1枚を仮想的にツモった手牌を作り、既存打牌評価・既存文脈反映・既存比較順で最良打牌を
// 求める。仮想ツモ牌は物理牌が決まらないため `next_tiles` には含めず、赤5が解決できない牌種として
// 既存の decoration へ渡す。
fn lookahead_for_draw(
    inputs: &LookaheadInputs,
    after_discard: &TileCounts,
    next_tiles: &[TileId],
    seen: &CandidateSeen,
    tile: &EffectiveAcceptanceTile,
) -> DrawLookaheadDiagnostic {
    let mut hypothetical = *after_discard;
    let next_discard = if hypothetical.try_add(tile.tile).is_ok() {
        let mut evaluations =
            evaluate_discards_with_seen(&hypothetical, inputs.fixed_meld_count, seen);
        decorate_evaluations(
            &mut evaluations,
            &hypothetical,
            &DecorationContext {
                tiles: next_tiles,
                dora_indicators: inputs.dora_indicators,
                round_wind: inputs.round_wind,
                seat_wind: inputs.seat_wind,
                shape_penalty: ShapePenaltyMode::WithContext {
                    round_wind: inputs.round_wind,
                    seat_wind: inputs.seat_wind,
                    fixed_meld_count: inputs.fixed_meld_count,
                },
                unresolved_red_tile: Some(tile.tile),
            },
        );
        select_best(evaluations)
    } else {
        None
    };

    DrawLookaheadDiagnostic {
        draw: tile.tile,
        remaining: tile.remaining,
        shanten_after_draw: tile.shanten_after_draw,
        next_discard,
    }
}

// 指定牌種を1枚切った後の物理牌一覧。切る物理牌は通常打牌評価と同じ黒牌優先で選ぶ。
fn tiles_after_discard(tiles: &[TileId], discard: TileType) -> Vec<TileId> {
    let mut remaining = tiles.to_vec();
    let Some(discarded) = discarded_tile_id_for_type(discard, tiles) else {
        return remaining;
    };
    if let Some(position) = remaining.iter().position(|tile| *tile == discarded) {
        remaining.remove(position);
    }
    remaining
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::discard::{
        evaluate_discards_from_tiles_with_fixed_melds_and_context,
        evaluate_discards_from_tiles_with_fixed_melds_and_visible_tiles,
        select_best_discard_from_tiles_with_context,
        select_best_discard_from_tiles_with_visible_tiles,
    };
    use crate::tile::count_indicated_dora;
    use std::sync::LazyLock;

    // 打牌評価と2手先診断を1組にした検証用の局面。2手先探索は重いので、同じ局面を使う複数の
    // テストで構築結果を共有する。
    struct Case {
        tiles: Vec<TileId>,
        counts: TileCounts,
        visible: Vec<TileId>,
        dora_indicators: Vec<TileId>,
        round_wind: Option<TileType>,
        seat_wind: Option<TileType>,
        fixed_meld_count: FixedMeldCount,
        evaluations: Vec<DiscardEvaluation>,
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

    fn hand_only_case(
        tiles: &[TileId],
        fixed_meld_count: FixedMeldCount,
        dora_indicators: Vec<TileId>,
        round_wind: Option<TileType>,
        seat_wind: Option<TileType>,
    ) -> Case {
        let counts = TileCounts::from_tiles(tiles.iter().copied());
        let evaluations = evaluate_discards_from_tiles_with_fixed_melds_and_context(
            tiles,
            fixed_meld_count,
            &dora_indicators,
            round_wind,
            seat_wind,
        );
        let lookahead = diagnose_lookahead_with_fixed_melds(
            tiles,
            fixed_meld_count,
            &dora_indicators,
            round_wind,
            seat_wind,
            &evaluations,
        );
        Case {
            tiles: tiles.to_vec(),
            counts,
            visible: Vec::new(),
            dora_indicators,
            round_wind,
            seat_wind,
            fixed_meld_count,
            evaluations,
            lookahead,
        }
    }

    fn visible_case(
        tiles: &[TileId],
        fixed_meld_count: FixedMeldCount,
        dora_indicators: Vec<TileId>,
        round_wind: Option<TileType>,
        seat_wind: Option<TileType>,
        visible: Vec<TileId>,
    ) -> Case {
        let counts = TileCounts::from_tiles(tiles.iter().copied());
        let evaluations = evaluate_discards_from_tiles_with_fixed_melds_and_visible_tiles(
            tiles,
            fixed_meld_count,
            &dora_indicators,
            round_wind,
            seat_wind,
            &visible,
        );
        let lookahead = diagnose_lookahead_with_fixed_melds_and_visible_tiles(
            tiles,
            fixed_meld_count,
            &dora_indicators,
            round_wind,
            seat_wind,
            &visible,
            &evaluations,
        );
        Case {
            tiles: tiles.to_vec(),
            counts,
            visible,
            dora_indicators,
            round_wind,
            seat_wind,
            fixed_meld_count,
            evaluations,
            lookahead,
        }
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

    // 指定牌種のうち、まだ使われていない物理牌を1枚返す。赤5の曖昧さを持ち込まないよう黒牌を
    // 優先する。
    fn unused_copy(tile_type: TileType, used: &[TileId]) -> TileId {
        let copies: Vec<TileId> = (0..4)
            .filter_map(|offset| TileId::new(tile_type.raw() * 4 + offset))
            .filter(|id| !used.contains(id))
            .collect();
        copies
            .iter()
            .copied()
            .find(|id| !id.is_red())
            .or_else(|| copies.first().copied())
            .expect("an unused physical copy exists")
    }

    #[test]
    fn covers_every_current_discard_candidate() {
        let case = &*CONCEALED_HAND_ONLY;

        assert!(case.evaluations.len() > 1);
        assert_eq!(case.lookahead.candidates.len(), case.evaluations.len());
        for (candidate, evaluation) in case
            .lookahead
            .candidates
            .iter()
            .zip(case.evaluations.iter())
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
            .zip(case.evaluations.iter())
        {
            let acceptance = &evaluation.acceptance_after_discard.tiles;
            assert_eq!(candidate.draws.len(), acceptance.len());
            for (draw, accepted) in candidate.draws.iter().zip(acceptance.iter()) {
                assert_eq!(draw.draw, accepted.tile);
                assert_eq!(draw.remaining, accepted.remaining);
                assert_eq!(draw.shanten_after_draw, accepted.shanten_after_draw);
            }
        }
    }

    // 仮想ツモ後の手牌を物理牌一覧として組み立てる。ツモ牌には未使用の黒牌を割り当てるため、
    // 既存の context-aware 評価 API へそのまま渡せる。
    fn hypothetical_tiles(case: &Case, discard: TileType, draw: TileType) -> Vec<TileId> {
        let mut tiles = tiles_after_discard(&case.tiles, discard);
        let mut used = tiles.clone();
        used.extend(case.visible.iter().copied());
        used.extend(case.dora_indicators.iter().copied());
        tiles.push(unused_copy(draw, &used));
        tiles
    }

    // 仮想ツモ牌を見え牌へ足した visible tiles。1手目の打牌は手牌から消えても visible に残る。
    fn hypothetical_visible(case: &Case, drawn_tile: TileId) -> Vec<TileId> {
        let mut visible = case.visible.clone();
        visible.push(drawn_tile);
        visible
    }

    // 2手目の最良打牌を、既存の context-aware 評価 API だけで求める。lookahead 側の期待値を
    // テスト内で再実装しないための共通 helper。
    fn expected_next_discard(
        case: &Case,
        discard: TileType,
        draw: TileType,
    ) -> Option<DiscardEvaluation> {
        let tiles = hypothetical_tiles(case, discard, draw);
        let drawn_tile = *tiles.last().expect("the drawn tile was pushed last");
        assert!(!drawn_tile.is_red(), "赤5の曖昧さが無い局面で検証する");

        if case.visible.is_empty() {
            select_best_discard_from_tiles_with_context(
                &tiles,
                &case.dora_indicators,
                case.round_wind,
                case.seat_wind,
            )
        } else {
            select_best_discard_from_tiles_with_visible_tiles(
                &tiles,
                &case.dora_indicators,
                case.round_wind,
                case.seat_wind,
                &hypothetical_visible(case, drawn_tile),
            )
        }
    }

    // 全候補 × 全受け入れ牌で、2手先の next discard が既存 context-aware 評価と一致することを
    // 確認する。戻り値は検証した件数。
    fn assert_next_discard_matches_existing_evaluation(case: &Case) -> usize {
        assert_eq!(case.fixed_meld_count, FixedMeldCount::NONE);

        let mut checked = 0;
        for candidate in &case.lookahead.candidates {
            for draw in &candidate.draws {
                assert_eq!(
                    draw.next_discard,
                    expected_next_discard(case, candidate.discard, draw.draw),
                    "discard {:?} draw {:?}",
                    candidate.discard,
                    draw.draw,
                );
                checked += 1;
            }
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

    fn honor_choice_case(
        dora_indicators: Vec<TileId>,
        round_wind: Option<TileType>,
        seat_wind: Option<TileType>,
    ) -> Case {
        let hand = honor_choice_hand();
        let mut visible = hand.clone();
        visible.extend(dora_indicators.iter().copied());
        visible_case(
            &hand,
            FixedMeldCount::NONE,
            dora_indicators,
            round_wind,
            seat_wind,
            visible,
        )
    }

    // 東場南家。S が自風の役牌、W は無関係な客風。
    static VALUE_HONOR_CASE: LazyLock<Case> =
        LazyLock::new(|| honor_choice_case(Vec::new(), Some(tile("E")), Some(tile("S"))));

    // ドラ表示牌 E → ドラは S。赤5とは無関係に牌種だけで決まる通常ドラ。
    static NORMAL_DORA_CASE: LazyLock<Case> =
        LazyLock::new(|| honor_choice_case(ids(&[108]), None, None));

    // 場風・自風もドラ表示牌も持たない対照 case。役牌 / 通常ドラの両方の比較に使う。
    static HONOR_CHOICE_FREE_CASE: LazyLock<Case> =
        LazyLock::new(|| honor_choice_case(Vec::new(), None, None));

    // 2つの診断で next discard の牌種が異なる (candidate, draw) の件数を数える。
    fn count_next_discard_differences(left: &Case, right: &Case) -> usize {
        let mut differences = 0;
        for candidate in &left.lookahead.candidates {
            let Some(other) = right.lookahead.candidate(candidate.discard) else {
                continue;
            };
            for draw in &candidate.draws {
                let Some(other_draw) = other.draw(draw.draw) else {
                    continue;
                };
                if draw.next_discard_tile() != other_draw.next_discard_tile() {
                    differences += 1;
                }
            }
        }
        differences
    }

    #[test]
    fn next_discard_reflects_value_honor_context() {
        let case = &*VALUE_HONOR_CASE;
        // 役牌は牌種と場風・自風だけで決まるので、赤5の曖昧さとは無関係に必ず反映される。
        assert!(assert_next_discard_matches_existing_evaluation(case) > 0);

        for candidate in &case.lookahead.candidates {
            for draw in &candidate.draws {
                let next = draw.next_discard.as_ref().expect("next discard exists");
                assert_eq!(
                    next.discarded_value_honor_count,
                    u8::from(next.discard.is_value_honor(case.round_wind, case.seat_wind)),
                );
            }
        }
    }

    #[test]
    fn value_honor_context_changes_the_next_discard() {
        // context-free では決着しない候補が、役牌保護によって別の牌になることを固定する。
        let differences =
            count_next_discard_differences(&VALUE_HONOR_CASE, &HONOR_CHOICE_FREE_CASE);
        assert!(
            differences > 0,
            "役牌 context が next discard を変える局面である必要がある"
        );
    }

    #[test]
    fn next_discard_reflects_normal_dora_context() {
        let case = &*NORMAL_DORA_CASE;
        assert!(assert_next_discard_matches_existing_evaluation(case) > 0);

        // 通常ドラは牌種から決まるので、仮想ツモ牌を切る候補でも 0 に潰さない。
        for candidate in &case.lookahead.candidates {
            for draw in &candidate.draws {
                let next = draw.next_discard.as_ref().expect("next discard exists");
                assert_eq!(
                    next.discarded_dora_count,
                    count_indicated_dora(next.discard, &case.dora_indicators)
                        + u8::from(next.discards_red_five),
                );
            }
        }
    }

    #[test]
    fn normal_dora_context_changes_the_next_discard() {
        let differences =
            count_next_discard_differences(&NORMAL_DORA_CASE, &HONOR_CHOICE_FREE_CASE);
        assert!(
            differences > 0,
            "通常ドラ context が next discard を変える局面である必要がある"
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
    fn drawn_tile_type_keeps_indicated_dora_without_resolving_red_five() {
        // 仮想ツモ牌を2手目に切る候補でも、牌種から確定する通常ドラは反映し、赤5扱いにはしない。
        let case = &*DRAWN_TILE_DISCARD_CASE;
        assert!(assert_next_discard_matches_existing_evaluation(case) > 0);

        let mut checked = 0;
        for candidate in &case.lookahead.candidates {
            for draw in &candidate.draws {
                let next = draw.next_discard.as_ref().expect("next discard exists");
                if next.discard != draw.draw {
                    continue;
                }
                assert!(!next.discards_red_five);
                assert_eq!(
                    next.discarded_dora_count,
                    count_indicated_dora(next.discard, &case.dora_indicators),
                );
                checked += 1;
            }
        }
        assert!(checked > 0, "仮想ツモ牌を2手目に切る候補が必要");
    }

    #[test]
    fn drawn_dora_tile_discard_keeps_the_indicated_dora_count() {
        // ドラそのものを仮想ツモしてそのまま切る候補で、通常ドラを 0 に潰していないことを固定する。
        let case = &*DRAWN_TILE_DISCARD_CASE;
        let dora = tile("9s");

        let hit = case
            .lookahead
            .candidates
            .iter()
            .flat_map(|candidate| candidate.draws.iter())
            .filter(|draw| draw.draw == dora)
            .filter_map(|draw| draw.next_discard.as_ref())
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
        for candidate in &case.lookahead.candidates {
            for draw in &candidate.draws {
                let next = draw.next_discard.as_ref().expect("next discard exists");

                let mut after_next = case.counts;
                after_next.remove(candidate.discard).unwrap();
                after_next.try_add(draw.draw).unwrap();
                after_next.remove(next.discard).unwrap();

                for accepted in &next.acceptance_after_discard.tiles {
                    let seen = after_next.count(accepted.tile)
                        + public_visible_count(accepted.tile)
                        + u8::from(accepted.tile == candidate.discard)
                        + u8::from(counts_candidate_discard && accepted.tile == next.discard);
                    assert_eq!(
                        accepted.remaining,
                        4u8.saturating_sub(seen),
                        "discard {:?} draw {:?} next {:?} tile {:?}",
                        candidate.discard,
                        draw.draw,
                        next.discard,
                        accepted.tile,
                    );
                    if accepted.tile == candidate.discard {
                        first_discard_hits += 1;
                    }
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
            diagnose_lookahead_with_fixed_melds_and_visible_tiles(
                &case.tiles,
                fixed(2),
                &[],
                None,
                None,
                &[],
                &case.evaluations,
            ),
            case.lookahead,
        );
    }

    #[test]
    fn fixed_melds_keep_the_existing_effective_shanten() {
        let case = hand_only_case(&melded_hand(), fixed(2), Vec::new(), None, None);

        let mut checked = 0;
        for candidate in &case.lookahead.candidates {
            for draw in &candidate.draws {
                // 副露手では七対子・国士を復活させず、通常形のみの effective shanten になる。
                assert_eq!(draw.shanten_after_draw.concealed(), None);
                let next = draw.next_discard.as_ref().expect("next discard exists");
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
        }
        assert!(checked > 0);
    }

    #[test]
    fn accessors_read_the_stored_next_evaluation() {
        let draw = &CONCEALED_HAND_ONLY.lookahead.candidates[0].draws[0];
        let next = draw.next_discard.as_ref().unwrap();

        assert_eq!(draw.next_discard_tile(), Some(next.discard));
        assert_eq!(
            draw.next_min_shanten(),
            Some(next.min_shanten_after_discard())
        );
        assert_eq!(
            draw.next_acceptance_total_remaining(),
            Some(next.acceptance_total_remaining())
        );
        assert_eq!(
            draw.next_acceptance_type_count(),
            Some(next.acceptance_type_count())
        );
        assert_eq!(
            draw.next_standard_iishanten_shape(),
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
        let _ =
            diagnose_lookahead_with_fixed_melds(&tiles, fixed(2), &[], None, None, &evaluations);
        assert_eq!(tiles, before);
    }
}
