use crate::acceptance::EffectiveAcceptanceTile;
use crate::discard::{CandidateSeen, DiscardEvaluation, evaluate_discards_with_seen, select_best};
use crate::iishanten::IishantenShape;
use crate::shanten::{EffectiveShanten, FixedMeldCount};
use crate::tile::{TileId, TileType};
use crate::tile_counts::TileCounts;

/// 現在の打牌候補1件について、その打牌後の受け入れ牌を1枚ツモった仮想手牌を既存打牌評価へ
/// かけた2手先診断。
///
/// 解析専用の pure なデータであり、打牌選択・押し引き・鳴き・リーチ判断のどれにも使用しない。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SecondPlyDiagnostic {
    /// 現在の打牌候補の牌種。
    pub discard: TileType,
    /// 現在打牌後の受け入れ牌ごとの2手先診断。順序と対象牌は現在打牌後の受け入れと同じ。
    pub draws: Vec<SecondPlyDrawDiagnostic>,
}

impl SecondPlyDiagnostic {
    pub fn draw(&self, tile: TileType) -> Option<&SecondPlyDrawDiagnostic> {
        self.draws.iter().find(|draw| draw.draw == tile)
    }
}

/// 現在打牌後の受け入れ牌1枚を仮想的にツモった場合の2手先診断。
///
/// `draw` / `remaining` / `shanten_after_draw` は現在打牌の [`DiscardEvaluation`] が持つ
/// 受け入れ ([`EffectiveAcceptanceTile`]) の値そのもので、診断のために再計算しない。
/// `remaining` は牌種ごとの残枚数を生データのまま保持し、期待値や加重平均へ潰さない。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SecondPlyDrawDiagnostic {
    pub draw: TileType,
    pub remaining: u8,
    pub shanten_after_draw: EffectiveShanten,
    /// 仮想ツモ後14枚に既存打牌評価と既存比較順を適用した最良打牌。打牌候補が1件も無い場合だけ
    /// `None`。数値のコピーではなく評価そのものを保持する。
    pub next_discard: Option<DiscardEvaluation>,
}

impl SecondPlyDrawDiagnostic {
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
    pub candidates: Vec<SecondPlyDiagnostic>,
}

impl LookaheadDiagnostic {
    pub fn candidate(&self, discard: TileType) -> Option<&SecondPlyDiagnostic> {
        self.candidates
            .iter()
            .find(|candidate| candidate.discard == discard)
    }
}

/// 副露済み面子数を考慮して全打牌候補の2手先診断を構築する。
///
/// `counts` は打牌前の全手牌、`evaluations` はその手牌に対する既存の打牌候補評価で、
/// `fixed_meld_count` はその評価に使ったものと同じ値を渡す。現在打牌後の受け入れは
/// `evaluations` が持つ値をそのまま使い、診断のために再計算しない。
///
/// 2手目の打牌評価・受け入れ・向聴・一向聴形分類・比較は既存の打牌評価経路をそのまま呼び出す。
/// 2手先専用の shanten / acceptance / comparator / shape evaluator は持たない。
pub fn diagnose_second_ply_with_fixed_melds(
    counts: &TileCounts,
    fixed_meld_count: FixedMeldCount,
    evaluations: &[DiscardEvaluation],
) -> LookaheadDiagnostic {
    diagnose_second_ply_with_seen(
        counts,
        fixed_meld_count,
        &CandidateSeen::hand_only(),
        evaluations,
    )
}

/// 副露済み面子数と visible tiles を考慮して全打牌候補の2手先診断を構築する。
///
/// 2手目の残枚数は「1手目の打牌前に見えていた牌 + 1手目に切った牌1枚」を seen として求める。
/// 1手目に切った牌は2手目時点で見え牌なので、山に残っている牌として数え直さない。
/// visible tiles は自分の手牌を含むため、既存の残枚数計算と同じ手牌差し引きで二重計上を防ぐ。
///
/// `fixed_meld_count == FixedMeldCount::NONE` かつ `visible_tiles` が空の場合は
/// [`diagnose_second_ply_with_fixed_melds`] と同じ seen 扱いになる。
pub fn diagnose_second_ply_with_fixed_melds_and_visible_tiles(
    counts: &TileCounts,
    fixed_meld_count: FixedMeldCount,
    visible_tiles: &[TileId],
    evaluations: &[DiscardEvaluation],
) -> LookaheadDiagnostic {
    if visible_tiles.is_empty() {
        return diagnose_second_ply_with_fixed_melds(counts, fixed_meld_count, evaluations);
    }

    diagnose_second_ply_with_seen(
        counts,
        fixed_meld_count,
        &CandidateSeen::from_visible_tiles(counts, visible_tiles),
        evaluations,
    )
}

fn diagnose_second_ply_with_seen(
    counts: &TileCounts,
    fixed_meld_count: FixedMeldCount,
    seen: &CandidateSeen,
    evaluations: &[DiscardEvaluation],
) -> LookaheadDiagnostic {
    LookaheadDiagnostic {
        candidates: evaluations
            .iter()
            .map(|evaluation| second_ply_for_candidate(counts, fixed_meld_count, seen, evaluation))
            .collect(),
    }
}

// 現在の打牌候補1件分の2手先診断。入力 counts は変更せず、打牌後の手牌を copy で作る。
fn second_ply_for_candidate(
    counts: &TileCounts,
    fixed_meld_count: FixedMeldCount,
    seen: &CandidateSeen,
    evaluation: &DiscardEvaluation,
) -> SecondPlyDiagnostic {
    let mut after_discard = *counts;
    if after_discard.remove(evaluation.discard).is_err() {
        return SecondPlyDiagnostic {
            discard: evaluation.discard,
            draws: Vec::new(),
        };
    }

    // 1手目に切った牌は2手目時点で見え牌になる。
    let next_seen = seen.after_discard(evaluation.discard);
    let draws = evaluation
        .acceptance_after_discard
        .tiles
        .iter()
        .map(|tile| second_ply_for_draw(&after_discard, fixed_meld_count, &next_seen, tile))
        .collect();

    SecondPlyDiagnostic {
        discard: evaluation.discard,
        draws,
    }
}

// 受け入れ牌1枚を仮想的にツモった手牌を作り、既存打牌評価と既存比較順で最良打牌を求める。
fn second_ply_for_draw(
    after_discard: &TileCounts,
    fixed_meld_count: FixedMeldCount,
    seen: &CandidateSeen,
    tile: &EffectiveAcceptanceTile,
) -> SecondPlyDrawDiagnostic {
    let mut hypothetical = *after_discard;
    let next_discard = if hypothetical.try_add(tile.tile).is_ok() {
        select_best(evaluate_discards_with_seen(
            &hypothetical,
            fixed_meld_count,
            seen,
        ))
    } else {
        None
    };

    SecondPlyDrawDiagnostic {
        draw: tile.tile,
        remaining: tile.remaining,
        shanten_after_draw: tile.shanten_after_draw,
        next_discard,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::discard::{
        evaluate_discards_with_fixed_melds, evaluate_discards_with_fixed_melds_and_visible_tiles,
        select_best_discard_with_visible_tiles,
    };
    use std::sync::LazyLock;

    // 打牌評価と2手先診断を1組にした検証用の局面。2手先探索は重いので、同じ局面を使う複数の
    // テストで構築結果を共有する。
    struct Case {
        counts: TileCounts,
        visible: Vec<TileId>,
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

    fn hand_only_case(tiles: &[TileId], fixed_meld_count: FixedMeldCount) -> Case {
        let counts = TileCounts::from_tiles(tiles.iter().copied());
        let evaluations = evaluate_discards_with_fixed_melds(&counts, fixed_meld_count);
        let lookahead =
            diagnose_second_ply_with_fixed_melds(&counts, fixed_meld_count, &evaluations);
        Case {
            counts,
            visible: Vec::new(),
            evaluations,
            lookahead,
        }
    }

    fn visible_case(
        tiles: &[TileId],
        fixed_meld_count: FixedMeldCount,
        visible: Vec<TileId>,
    ) -> Case {
        let counts = TileCounts::from_tiles(tiles.iter().copied());
        let evaluations = evaluate_discards_with_fixed_melds_and_visible_tiles(
            &counts,
            fixed_meld_count,
            &visible,
        );
        let lookahead = diagnose_second_ply_with_fixed_melds_and_visible_tiles(
            &counts,
            fixed_meld_count,
            &visible,
            &evaluations,
        );
        Case {
            counts,
            visible,
            evaluations,
            lookahead,
        }
    }

    static CONCEALED_HAND_ONLY: LazyLock<Case> =
        LazyLock::new(|| hand_only_case(&concealed_hand(), FixedMeldCount::NONE));

    static CONCEALED_WITH_VISIBLE: LazyLock<Case> = LazyLock::new(|| {
        let mut visible = concealed_hand();
        visible.extend(public_visible_tiles());
        visible_case(&concealed_hand(), FixedMeldCount::NONE, visible)
    });

    // 指定牌種のうち、まだ使われていない物理牌を1枚返す。
    fn unused_copy(tile_type: TileType, used: &[TileId]) -> TileId {
        (0..4)
            .filter_map(|offset| TileId::new(tile_type.raw() * 4 + offset))
            .find(|id| !used.contains(id))
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
        let case = &*CONCEALED_HAND_ONLY;

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

    #[test]
    fn next_discard_matches_the_existing_evaluation_and_comparator() {
        let case = &*CONCEALED_WITH_VISIBLE;

        let mut checked = 0;
        for evaluation in &case.evaluations {
            let mut after_discard = case.counts;
            after_discard.remove(evaluation.discard).unwrap();

            for accepted in &evaluation.acceptance_after_discard.tiles {
                let mut hypothetical = after_discard;
                hypothetical.try_add(accepted.tile).unwrap();

                // 1手目の打牌は手牌から消えても visible には残り、仮想ツモ牌は手牌が増えた分だけ
                // visible にも足す。これで既存 visible tiles 経路が2手先と同じ seen になる。
                let mut next_visible = case.visible.clone();
                next_visible.push(unused_copy(accepted.tile, &next_visible));

                let expected = select_best_discard_with_visible_tiles(&hypothetical, &next_visible);
                let actual = case
                    .lookahead
                    .candidate(evaluation.discard)
                    .unwrap()
                    .draw(accepted.tile)
                    .unwrap();

                assert_eq!(actual.next_discard, expected);
                checked += 1;
            }
        }
        assert!(checked > 0);
    }

    // 2手目の受け入れ残枚数が、期待する seen 集合から計算した値と一致することを全候補で確認する。
    //
    // `public_visible` は手牌以外に見えている枚数、`counts_candidate_discard` は2手目の打牌候補を
    // seen に数えるかどうか。1手目の打牌はどちらの経路でも seen に数える。
    // 戻り値は「1手目に切った牌が2手目の受け入れに現れた回数」で、山への復活検証が効いた件数。
    fn assert_second_ply_remaining(
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
        let hits = assert_second_ply_remaining(&CONCEALED_HAND_ONLY, &[], false);
        assert!(hits > 0, "1手目の打牌が2手目の受け入れに現れる局面が必要");
    }

    #[test]
    fn first_discard_stays_seen_with_visible_tiles() {
        let public_visible = [(tile("1m"), 2), (tile("5p"), 1), (tile("W"), 1)];
        let hits = assert_second_ply_remaining(&CONCEALED_WITH_VISIBLE, &public_visible, true);
        assert!(hits > 0, "1手目の打牌が2手目の受け入れに現れる局面が必要");
    }

    #[test]
    fn does_not_double_count_the_own_hand_in_visible_tiles() {
        // visible tiles が自分の手牌そのものだけなら、手牌以外に見えている牌は無い扱いになる。
        let hand = melded_hand();
        let case = visible_case(&hand, fixed(2), hand.clone());

        let hits = assert_second_ply_remaining(&case, &[], true);
        assert!(hits > 0, "1手目の打牌が2手目の受け入れに現れる局面が必要");
    }

    #[test]
    fn empty_visible_tiles_match_the_fixed_meld_entry() {
        let case = hand_only_case(&melded_hand(), fixed(2));

        assert_eq!(
            diagnose_second_ply_with_fixed_melds_and_visible_tiles(
                &case.counts,
                fixed(2),
                &[],
                &case.evaluations,
            ),
            case.lookahead,
        );
    }

    #[test]
    fn fixed_melds_keep_the_existing_effective_shanten() {
        let case = hand_only_case(&melded_hand(), fixed(2));

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
    fn does_not_modify_the_input_counts() {
        let counts = TileCounts::from_tiles(melded_hand());
        let before = counts;
        let evaluations = evaluate_discards_with_fixed_melds(&counts, fixed(2));
        let _ = diagnose_second_ply_with_fixed_melds(&counts, fixed(2), &evaluations);
        assert_eq!(counts, before);
    }
}
