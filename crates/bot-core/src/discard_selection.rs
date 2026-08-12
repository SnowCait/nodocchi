use crate::action::{LegalAction, preferred_dahai_action_for_type};
use crate::context::GameContext;
use bot_logic::{
    DiscardCandidateDiagnostic, DiscardDecisionDiagnostic, DiscardEvaluation,
    EffectiveAcceptanceTile, EffectiveShanten, FixedMeldCount, LookaheadDiagnostic, TileCounts,
    TileId, TileType, compare_discard_evaluations, diagnose_discard_evaluations_with_fixed_melds,
    diagnose_second_ply_with_fixed_melds, diagnose_second_ply_with_fixed_melds_and_visible_tiles,
    evaluate_discards_from_tiles_with_fixed_melds_and_context,
    evaluate_discards_from_tiles_with_fixed_melds_and_visible_tiles,
};

const LOG_TARGET: &str = "bot_core::discard_selection";

/// 通常打牌選択の内部結果。
///
/// - `evaluation`: 合法 Dahai 候補の中の最善 `DiscardEvaluation`。合法候補が無ければ `None`。
/// - `action`: `evaluation` に対応する合法 Dahai。
///
/// `evaluation` と `action` は常に同時に `Some` / `None` になり、`Some` のときは牌種が一致する。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DiscardActionSelection {
    pub evaluation: Option<DiscardEvaluation>,
    pub action: Option<LegalAction>,
}

/// 通常打牌選択の結果と、その選択に使った全合法候補の構造化診断。
///
/// `selection` は `select_discard_action_with_evaluation()` と同じ helper で導出するため、
/// 診断を付けても選択結果は変わらない。`diagnostic` / `lookahead` は解析専用の追加情報。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DiscardActionSelectionWithDiagnostic {
    pub selection: DiscardActionSelection,
    pub diagnostic: DiscardDecisionDiagnostic,
    /// 全合法候補の2手先診断。要求された場合だけ構築し、selection には一切使用しない。
    pub lookahead: Option<LookaheadDiagnostic>,
}

// 合法 Dahai へ絞り込み・物理牌補正済みの打牌候補評価集合と、その評価対象の物理牌一覧。
// 本番選択・構造化診断・tracing ログはすべてこの集合を共有する。
struct LegalDiscardEvaluations {
    tiles: Vec<TileId>,
    evaluations: Vec<DiscardEvaluation>,
}

pub fn select_discard_action(
    context: &GameContext,
    legal_actions: &[LegalAction],
) -> Option<LegalAction> {
    select_discard_action_with_evaluation(context, legal_actions).action
}

/// 合法 Dahai 候補だけから最善の `DiscardEvaluation` を選び、対応する合法 Dahai を返す。
///
/// 全打牌候補を評価したうえで、合法 Dahai に対応する牌種だけへ絞り込み、各評価の物理牌依存
/// フィールド (`discards_red_five` / `discarded_dora_count`) を実際に切られる合法 Dahai の
/// 物理牌へ合わせてから、既存比較順で最善を選ぶ。これにより evaluation は必ず実際に切れる牌の
/// 評価になり、押し引き入力にもそのまま共有できる。
///
/// 不変条件:
///
/// - 合法 Dahai 候補がある: `evaluation == Some` かつ `action == Some` で牌種が一致する
/// - 合法 Dahai 候補がない: `evaluation == None` かつ `action == None`
/// - `evaluation.discards_red_five == action の TileId.is_red()`
/// - `evaluation.discarded_dora_count == count_dora(action の TileId, dora_indicators)`
///
/// DEBUG / TRACE 診断が有効な場合も、物理牌補正後の合法候補だけを対象にする。
pub(crate) fn select_discard_action_with_evaluation(
    context: &GameContext,
    legal_actions: &[LegalAction],
) -> DiscardActionSelection {
    let legal = legal_discard_evaluations(context, legal_actions);

    if tracing::enabled!(target: LOG_TARGET, tracing::Level::DEBUG) {
        log_discard_diagnostic(
            context,
            &legal.tiles,
            &diagnose_legal_evaluations(context, &legal),
        );
    }

    selection_from_legal_evaluations(&legal, legal_actions)
}

/// `select_discard_action_with_evaluation()` と同じ選択結果に、全合法候補の構造化診断を添えて返す。
///
/// 合法候補の絞り込み・物理牌補正・最善選択はすべて通常経路と同じ helper を通すため、選択結果は
/// `select_discard_action_with_evaluation()` と一致する。`diagnostic` / `lookahead` は解析専用の
/// 追加情報で、候補ごとの形の内訳や2手先評価など通常経路では計算しない値を含むため、診断が必要な
/// 経路からのみ呼ぶ。
///
/// `with_lookahead` は2手先診断を構築するかどうか。2手先は
/// 「打牌候補 × 受け入れ牌 × 次打牌候補」の探索になり通常診断よりさらに重いため、明示的に
/// 要求された場合だけ構築する。構築の有無は選択結果を変えない。
pub(crate) fn select_discard_action_with_diagnostic(
    context: &GameContext,
    legal_actions: &[LegalAction],
    with_lookahead: bool,
) -> DiscardActionSelectionWithDiagnostic {
    let legal = legal_discard_evaluations(context, legal_actions);
    let diagnostic = diagnose_legal_evaluations(context, &legal);
    let lookahead = with_lookahead.then(|| lookahead_from_legal_evaluations(context, &legal));

    if tracing::enabled!(target: LOG_TARGET, tracing::Level::DEBUG) {
        log_discard_diagnostic(context, &legal.tiles, &diagnostic);
    }

    DiscardActionSelectionWithDiagnostic {
        selection: selection_from_legal_evaluations(&legal, legal_actions),
        diagnostic,
        lookahead,
    }
}

// 評価対象の物理牌一覧を作り、全打牌候補を評価してから合法 Dahai へ絞り込み・物理牌補正する。
fn legal_discard_evaluations(
    context: &GameContext,
    legal_actions: &[LegalAction],
) -> LegalDiscardEvaluations {
    let tiles: Vec<_> = context
        .hand_tiles()
        .iter()
        .copied()
        .chain(context.drawn_tile())
        .collect();

    let evaluations = retain_legal_dahai_evaluations(
        evaluate_discard_candidates(context, &tiles),
        legal_actions,
        context.dora_indicators(),
    );

    LegalDiscardEvaluations { tiles, evaluations }
}

// 補正済みの合法候補集合から最善評価と対応する合法 Dahai を決める。全経路共通の選択処理。
fn selection_from_legal_evaluations(
    legal: &LegalDiscardEvaluations,
    legal_actions: &[LegalAction],
) -> DiscardActionSelection {
    let evaluation = select_best_evaluation(&legal.evaluations).cloned();
    let action = evaluation
        .as_ref()
        .and_then(|evaluation| legal_dahai_for_evaluation(evaluation, legal_actions));

    DiscardActionSelection { evaluation, action }
}

// 絞り込み済みの合法候補集合から既存の診断を構築する。診断と tracing ログはこの結果を共有する。
// block context の副露補正が本番評価とずれないよう、診断にも同じ副露済み面子数を渡す。
fn diagnose_legal_evaluations(
    context: &GameContext,
    legal: &LegalDiscardEvaluations,
) -> DiscardDecisionDiagnostic {
    let counts = TileCounts::from_tiles(legal.tiles.iter().copied());
    diagnose_discard_evaluations_with_fixed_melds(
        &counts,
        evaluation_fixed_meld_count(context),
        &legal.evaluations,
    )
}

// 絞り込み済みの合法候補集合から2手先診断を構築する。解析専用で、選択には一切使用しない。
//
// 現在打牌後の受け入れは既存評価 (legal.evaluations) が持つ値をそのまま入力にするため、
// 現在の1手評価を再計算しない。副露済み面子数・visible tiles は本番評価と同じ値を渡し、
// 2手目の残枚数計算も既存 acceptance の seen 経路を共有する。
fn lookahead_from_legal_evaluations(
    context: &GameContext,
    legal: &LegalDiscardEvaluations,
) -> LookaheadDiagnostic {
    let counts = TileCounts::from_tiles(legal.tiles.iter().copied());
    let fixed_meld_count = evaluation_fixed_meld_count(context);

    if context.visible_tiles().is_empty() {
        diagnose_second_ply_with_fixed_melds(&counts, fixed_meld_count, &legal.evaluations)
    } else {
        diagnose_second_ply_with_fixed_melds_and_visible_tiles(
            &counts,
            fixed_meld_count,
            context.visible_tiles(),
            &legal.evaluations,
        )
    }
}

// 選択された牌種に一致する合法 Dahai を返す。通常牌を赤牌より優先し、なければ赤牌を返す。
fn legal_dahai_for_evaluation(
    evaluation: &DiscardEvaluation,
    legal_actions: &[LegalAction],
) -> Option<LegalAction> {
    legal_dahai_tile_for_type(evaluation.discard, legal_actions)
        .map(|tile| LegalAction::Dahai { tile })
}

// 指定牌種の合法 Dahai として実際に切られる物理牌を返す。通常牌を赤牌より優先し、なければ
// 赤牌を返す。action 選択 (legal_dahai_for_evaluation) と評価補正 (evaluation_for_legal_dahai)
// が同じ物理牌を指すよう、物理牌選択は全経路共通の preferred_dahai_action_for_type へ委譲する。
fn legal_dahai_tile_for_type(tile_type: TileType, legal_actions: &[LegalAction]) -> Option<TileId> {
    match preferred_dahai_action_for_type(legal_actions, tile_type)? {
        LegalAction::Dahai { tile } => Some(*tile),
        _ => None,
    }
}

// context に応じた全打牌候補の評価一覧を返す。通常経路と診断経路で分岐を共有する。
//
// 自分の副露済み面子数が分かる場合はその値を fixed-meld 対応評価へ渡し、副露済み手牌でも
// 完成済み面子を含めた向聴・受け入れで評価する。分からない場合は
// evaluation_fixed_meld_count() の方針どおり門前評価へフォールバックする。
fn evaluate_discard_candidates(context: &GameContext, tiles: &[TileId]) -> Vec<DiscardEvaluation> {
    evaluate_discard_candidates_with_fixed_meld_count(
        context,
        tiles,
        evaluation_fixed_meld_count(context),
    )
}

// 副露済み面子数を明示して全打牌候補を評価する。visible tiles の有無による経路分岐・評価
// ロジックは通常経路と完全に共通で、使う副露済み面子数だけが違う。
//
// 鳴きシミュレーションのように GameContext がまだ鳴く前の状態である場合に、
// context の副露済み面子数ではなく鳴いた後の値で評価するために使う。
fn evaluate_discard_candidates_with_fixed_meld_count(
    context: &GameContext,
    tiles: &[TileId],
    fixed_meld_count: FixedMeldCount,
) -> Vec<DiscardEvaluation> {
    if context.visible_tiles().is_empty() {
        evaluate_discards_from_tiles_with_fixed_melds_and_context(
            tiles,
            fixed_meld_count,
            context.dora_indicators(),
            context.round_wind(),
            context.seat_wind(),
        )
    } else {
        evaluate_discards_from_tiles_with_fixed_melds_and_visible_tiles(
            tiles,
            fixed_meld_count,
            context.dora_indicators(),
            context.round_wind(),
            context.seat_wind(),
            context.visible_tiles(),
        )
    }
}

// 打牌評価に使う副露済み面子数。
//
// `player_id` が無いなど自分の副露が確定できない場合 (`own_fixed_meld_count() == None`) は、
// player 0 の副露数などを推測せず、既存の門前評価経路と同じ `FixedMeldCount::NONE` で評価する。
// これは情報不足時の fallback であり「副露0と確定した」という診断ではない。診断が報告する
// `own_fixed_meld_count` は引き続き `None` のままにする。
fn evaluation_fixed_meld_count(context: &GameContext) -> FixedMeldCount {
    context
        .own_fixed_meld_count()
        .unwrap_or(FixedMeldCount::NONE)
}

// 合法 Dahai に対応する牌種を持つ評価候補だけを、元の順序を保って残す。
// さらに各評価の物理牌依存フィールド (discards_red_five / discarded_dora_count) を、実際に
// 切られる合法 Dahai の物理牌へ合わせる。牌種単位の向聴・受け入れ・shape_penalty 等は変更
// しない。評価一覧は牌種ごとに1件なので、同じ牌種の合法 Dahai が複数あっても評価は重複しない。
fn retain_legal_dahai_evaluations(
    evaluations: Vec<DiscardEvaluation>,
    legal_actions: &[LegalAction],
    dora_indicators: &[TileId],
) -> Vec<DiscardEvaluation> {
    evaluations
        .into_iter()
        .filter_map(|evaluation| {
            evaluation_for_legal_dahai(evaluation, legal_actions, dora_indicators)
        })
        .collect()
}

// 評価に対応する合法 Dahai が存在すれば、その物理牌へ物理牌依存フィールドを合わせた評価を返す。
// 存在しなければ None。物理牌は legal_dahai_tile_for_type と同じ通常牌優先・赤牌fallback方針で
// 選ぶため、返す評価と最終的に選ばれる action の物理牌は常に一致する。
fn evaluation_for_legal_dahai(
    mut evaluation: DiscardEvaluation,
    legal_actions: &[LegalAction],
    dora_indicators: &[TileId],
) -> Option<DiscardEvaluation> {
    let discarded_tile = legal_dahai_tile_for_type(evaluation.discard, legal_actions)?;
    evaluation.discards_red_five = discarded_tile.is_red();
    evaluation.discarded_dora_count = bot_logic::count_dora(discarded_tile, dora_indicators);
    Some(evaluation)
}

// 既存比較順 compare_discard_evaluations() で最善評価を選ぶ。完全同値では先に現れた候補を維持する。
fn select_best_evaluation(evaluations: &[DiscardEvaluation]) -> Option<&DiscardEvaluation> {
    let mut best: Option<&DiscardEvaluation> = None;
    for candidate in evaluations {
        match best {
            Some(current)
                if !compare_discard_evaluations(candidate, current).candidate_is_better => {}
            _ => best = Some(candidate),
        }
    }
    best
}

// 合法 action を受け取らない汎用の best 評価。push_pull など合法 action が無い経路で使用する。
pub(crate) fn select_best_discard_evaluation(
    context: &GameContext,
    tiles: &[TileId],
) -> Option<DiscardEvaluation> {
    select_best_discard_evaluation_with_fixed_meld_count(
        context,
        tiles,
        evaluation_fixed_meld_count(context),
    )
}

/// 副露済み面子数を明示した best 評価。候補評価・比較は通常経路と同じ helper を共有する。
///
/// `fixed_meld_count == evaluation_fixed_meld_count(context)` では
/// [`select_best_discard_evaluation`] と一致する。
pub(crate) fn select_best_discard_evaluation_with_fixed_meld_count(
    context: &GameContext,
    tiles: &[TileId],
    fixed_meld_count: FixedMeldCount,
) -> Option<DiscardEvaluation> {
    let evaluations =
        evaluate_discard_candidates_with_fixed_meld_count(context, tiles, fixed_meld_count);
    select_best_evaluation(&evaluations).cloned()
}

fn tiles_to_mjai(tiles: &[TileId]) -> String {
    tiles
        .iter()
        .map(|tile| tile.to_mjai_string())
        .collect::<Vec<_>>()
        .join(" ")
}

fn log_discard_diagnostic(
    context: &GameContext,
    tiles: &[TileId],
    diagnostic: &DiscardDecisionDiagnostic,
) {
    let Some(selected) = diagnostic.selected.as_ref() else {
        return;
    };

    let hand_tiles = tiles_to_mjai(context.hand_tiles());
    let all_tiles = tiles_to_mjai(tiles);
    let drawn_tile = context.drawn_tile().map(|tile| tile.to_mjai_string());
    let dora_indicators = tiles_to_mjai(context.dora_indicators());
    let round_wind = context.round_wind().map(|wind| wind.to_mjai_string());
    let seat_wind = context.seat_wind().map(|wind| wind.to_mjai_string());

    tracing::debug!(
        target: LOG_TARGET,
        hand_tiles = %hand_tiles,
        drawn_tile = ?drawn_tile,
        all_tiles = %all_tiles,
        dora_indicators = %dora_indicators,
        round_wind = ?round_wind,
        seat_wind = ?seat_wind,
        visible_tile_count = context.visible_tiles().len(),
        candidate_count = diagnostic.candidates.len(),
        normal_discard = %selected.discard.to_mjai_string(),
        normal_standard_shanten = selected.shanten_after_discard.standard(),
        normal_chiitoitsu_shanten = ?chiitoitsu_shanten(selected.shanten_after_discard),
        normal_kokushi_shanten = ?kokushi_shanten(selected.shanten_after_discard),
        normal_min_shanten = selected.min_shanten_after_discard(),
        normal_acceptance_total_remaining = selected.acceptance_total_remaining(),
        normal_acceptance_type_count = selected.acceptance_type_count(),
        normal_shape_penalty = selected.shape_penalty,
        normal_iishanten_shape_after_discard = ?selected.standard_iishanten_shape_after_discard,
        normal_floating_tile_value = selected.floating_tile_value,
        normal_discards_isolated_tile = selected.discards_isolated_tile,
        normal_discarded_dora_count = selected.discarded_dora_count,
        normal_discarded_value_honor_count = selected.discarded_value_honor_count,
        normal_discards_red_five = selected.discards_red_five,
        "normal discard evaluation",
    );

    if tracing::enabled!(target: LOG_TARGET, tracing::Level::TRACE) {
        for candidate in &diagnostic.candidates {
            log_discard_candidate(candidate);
        }
    }
}

// 副露済み面子がある場合、七対子・国士は完成形候補にできないため向聴数が存在しない。
// 適当な sentinel を表示せず `None` にして、意味の無い値をログへ出さない。
fn chiitoitsu_shanten(shanten: EffectiveShanten) -> Option<i8> {
    shanten.concealed().map(|shanten| shanten.chiitoitsu)
}

fn kokushi_shanten(shanten: EffectiveShanten) -> Option<i8> {
    shanten.concealed().map(|shanten| shanten.kokushi)
}

fn acceptance_tile_diagnostic(
    tile: &EffectiveAcceptanceTile,
) -> (String, u8, i8, Option<i8>, Option<i8>, i8) {
    (
        tile.tile.to_mjai_string(),
        tile.remaining,
        tile.shanten_after_draw.standard(),
        chiitoitsu_shanten(tile.shanten_after_draw),
        kokushi_shanten(tile.shanten_after_draw),
        tile.shanten_after_draw.min(),
    )
}

fn log_discard_candidate(candidate: &DiscardCandidateDiagnostic) {
    let evaluation = &candidate.evaluation;
    let acceptance_tiles = evaluation
        .acceptance_after_discard
        .tiles
        .iter()
        .map(acceptance_tile_diagnostic)
        .collect::<Vec<_>>();

    tracing::trace!(
        target: LOG_TARGET,
        discard = %evaluation.discard.to_mjai_string(),
        selected = candidate.selected,
        selected_is_strictly_better_than_candidate =
            candidate.selected_is_strictly_better_than_candidate,
        comparison_reason = ?candidate.comparison_reason,
        count_before_discard = evaluation.count_before_discard,
        standard_shanten_after_discard = evaluation.shanten_after_discard.standard(),
        chiitoitsu_shanten_after_discard = ?chiitoitsu_shanten(evaluation.shanten_after_discard),
        kokushi_shanten_after_discard = ?kokushi_shanten(evaluation.shanten_after_discard),
        min_shanten_after_discard = evaluation.min_shanten_after_discard(),
        acceptance_total_remaining = evaluation.acceptance_total_remaining(),
        acceptance_type_count = evaluation.acceptance_type_count(),
        acceptance_tiles = ?acceptance_tiles,
        shape_penalty = evaluation.shape_penalty,
        iishanten_shape_after_discard = ?evaluation.standard_iishanten_shape_after_discard,
        floating_tile_value = evaluation.floating_tile_value,
        discards_isolated_tile = evaluation.discards_isolated_tile,
        discarded_dora_count = evaluation.discarded_dora_count,
        discarded_value_honor_count = evaluation.discarded_value_honor_count,
        discards_red_five = evaluation.discards_red_five,
        shape_breakdown = ?candidate.shape_breakdown,
        pair_context = ?candidate.pair_context,
        block_context = ?candidate.block_context,
        floating_tile_value_breakdown = ?candidate.floating_tile_value_breakdown,
        "discard candidate",
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use bot_logic::TileId;

    fn tile(value: u8) -> TileId {
        TileId::new(value).unwrap()
    }

    fn dahai(value: u8) -> LegalAction {
        LegalAction::Dahai { tile: tile(value) }
    }

    #[test]
    fn returns_none_for_empty_legal_actions() {
        let context = GameContext::from_parts(Some(tile(0)), vec![tile(4)]);
        assert_eq!(select_discard_action(&context, &[]), None);
    }

    #[test]
    fn returns_none_without_dahai_action() {
        let context = GameContext::from_parts(Some(tile(0)), vec![tile(1)]);
        let actions = vec![LegalAction::Reach, LegalAction::None];
        assert_eq!(select_discard_action(&context, &actions), None);
    }

    #[test]
    fn returns_none_without_context_tiles() {
        let context = GameContext::default();
        let actions = vec![dahai(0)];
        assert_eq!(select_discard_action(&context, &actions), None);
    }

    #[test]
    fn returns_dahai_matching_best_discard() {
        let hand_values = [0, 4, 8, 12, 17, 20, 24, 28, 32, 36, 40, 44, 89];
        let context = GameContext::from_parts(
            Some(tile(116)),
            hand_values.iter().map(|&value| tile(value)).collect(),
        );
        let actions: Vec<LegalAction> = hand_values
            .iter()
            .map(|&value| dahai(value))
            .chain([dahai(116)])
            .collect();

        let selected_action = select_discard_action(&context, &actions).unwrap();

        let tiles: Vec<_> = context
            .hand_tiles()
            .iter()
            .copied()
            .chain(context.drawn_tile())
            .collect();
        let selected_type = bot_logic::select_best_discard_from_tiles(&tiles)
            .unwrap()
            .discard;

        assert!(matches!(
            selected_action,
            LegalAction::Dahai { tile } if tile.tile_type() == selected_type
        ));
    }

    #[test]
    fn evaluates_drawn_tile() {
        let context = GameContext::with_drawn_tile(tile(0));
        let actions = vec![dahai(0)];
        assert_eq!(select_discard_action(&context, &actions), Some(dahai(0)));
    }

    #[test]
    fn evaluates_hand_tiles() {
        let context = GameContext::with_hand_tiles(vec![tile(0), tile(4), tile(8)]);
        let actions = vec![dahai(0), dahai(4), dahai(8)];
        assert!(matches!(
            select_discard_action(&context, &actions),
            Some(LegalAction::Dahai { .. })
        ));
    }

    #[test]
    fn returns_first_dahai_of_same_tile_type() {
        let context = GameContext::from_parts(Some(tile(16)), vec![tile(17)]);
        let actions = vec![dahai(17), dahai(16)];
        assert_eq!(select_discard_action(&context, &actions), Some(dahai(17)));
    }

    #[test]
    fn prefers_black_five_over_red_of_selected_type() {
        let context = GameContext::from_parts(None, vec![tile(16), tile(17)]);
        let actions = vec![dahai(16), dahai(17)];
        assert_eq!(select_discard_action(&context, &actions), Some(dahai(17)));
    }

    #[test]
    fn falls_back_to_red_five_when_only_red_available() {
        let context = GameContext::from_parts(None, vec![tile(16)]);
        let actions = vec![dahai(16)];
        assert_eq!(select_discard_action(&context, &actions), Some(dahai(16)));
    }

    #[test]
    fn returns_none_without_context_tiles_even_with_dahai() {
        let context = GameContext::default();
        let actions = vec![dahai(16)];
        assert_eq!(select_discard_action(&context, &actions), None);
    }

    #[test]
    fn returns_none_when_selected_type_has_no_dahai() {
        let context = GameContext::with_hand_tiles(vec![tile(0)]);
        let actions = vec![dahai(4)];
        assert_eq!(select_discard_action(&context, &actions), None);
    }

    #[test]
    fn perfect_tie_avoids_discarding_dora() {
        // 123m 456m 789m 123p + 東(浮き) 西(浮き), ドラ表示 南 -> 西 がドラ
        let hand: Vec<_> = [0u8, 4, 8, 12, 17, 20, 24, 28, 32, 36, 40, 44, 108]
            .iter()
            .map(|&value| tile(value))
            .collect();
        let context = GameContext::from_parts_with_dora(Some(tile(116)), hand, vec![tile(112)]);
        let actions: Vec<LegalAction> = [0u8, 4, 8, 12, 17, 20, 24, 28, 32, 36, 40, 44, 108, 116]
            .iter()
            .map(|&value| dahai(value))
            .collect();

        let selected = select_discard_action(&context, &actions).unwrap();
        let LegalAction::Dahai { tile } = selected else {
            panic!("expected dahai");
        };
        assert_eq!(tile.tile_type().to_mjai_string(), "E");
    }

    #[test]
    fn discards_dora_when_it_lowers_shanten() {
        // 5m を切るとテンパイになる形。5m がドラでも向聴を優先して切る
        let hand: Vec<_> = [40u8, 44, 48, 56, 60, 64, 76, 80, 84, 108, 109, 96, 100]
            .iter()
            .map(|&value| tile(value))
            .collect();
        let context = GameContext::from_parts_with_dora(Some(tile(16)), hand, vec![tile(12)]);
        let actions: Vec<LegalAction> =
            [40u8, 44, 48, 56, 60, 64, 76, 80, 84, 108, 109, 96, 100, 16]
                .iter()
                .map(|&value| dahai(value))
                .collect();

        let selected = select_discard_action(&context, &actions).unwrap();
        let LegalAction::Dahai { tile } = selected else {
            panic!("expected dahai");
        };
        assert_eq!(tile.tile_type().to_mjai_string(), "5m");
    }

    #[test]
    fn prefers_black_five_over_red_with_dora_indicator() {
        // 赤5と通常5が併存する場合は通常5を切る
        let context =
            GameContext::from_parts_with_dora(None, vec![tile(16), tile(17)], vec![tile(12)]);
        let actions = vec![dahai(16), dahai(17)];
        assert_eq!(select_discard_action(&context, &actions), Some(dahai(17)));
    }

    #[test]
    fn empty_tiles_yield_no_action_with_dora() {
        let context = GameContext::from_parts_with_dora(None, vec![], vec![tile(12)]);
        let actions = vec![dahai(0)];
        assert_eq!(select_discard_action(&context, &actions), None);
    }

    #[test]
    fn perfect_tie_keeps_value_honor() {
        // 123m 456m 789m 123p + 中(浮き) 北(浮き)。役牌でない北を切る
        let hand: Vec<_> = [0u8, 4, 8, 12, 17, 20, 24, 28, 32, 36, 40, 44, 132]
            .iter()
            .map(|&value| tile(value))
            .collect();
        let context =
            GameContext::from_parts_with_context(Some(tile(120)), hand, vec![], None, None);
        let actions: Vec<LegalAction> = [0u8, 4, 8, 12, 17, 20, 24, 28, 32, 36, 40, 44, 132, 120]
            .iter()
            .map(|&value| dahai(value))
            .collect();

        let selected = select_discard_action(&context, &actions).unwrap();
        let LegalAction::Dahai { tile } = selected else {
            panic!("expected dahai");
        };
        assert_eq!(tile.tile_type().to_mjai_string(), "N");
    }

    #[test]
    fn round_wind_makes_wind_harder_to_discard() {
        // 東場。孤立した東(場風)と北(客風)では、役牌でない北を切る
        let hand: Vec<_> = [0u8, 4, 8, 12, 17, 20, 24, 28, 32, 36, 40, 44, 108]
            .iter()
            .map(|&value| tile(value))
            .collect();
        let context = GameContext::from_parts_with_context(
            Some(tile(120)),
            hand,
            vec![],
            Some(bot_logic::TileType::new(27).unwrap()),
            Some(bot_logic::TileType::new(28).unwrap()),
        );
        let actions: Vec<LegalAction> = [0u8, 4, 8, 12, 17, 20, 24, 28, 32, 36, 40, 44, 108, 120]
            .iter()
            .map(|&value| dahai(value))
            .collect();

        let selected = select_discard_action(&context, &actions).unwrap();
        let LegalAction::Dahai { tile } = selected else {
            panic!("expected dahai");
        };
        assert_eq!(tile.tile_type().to_mjai_string(), "N");
    }

    #[test]
    fn double_wind_kept_over_single_value_honor() {
        // 東場東家。ダブル東(場風かつ自風)と中(単役牌)では中を切る
        let hand: Vec<_> = [0u8, 4, 8, 12, 17, 20, 24, 28, 32, 36, 40, 44, 108]
            .iter()
            .map(|&value| tile(value))
            .collect();
        let context = GameContext::from_parts_with_context(
            Some(tile(132)),
            hand,
            vec![],
            Some(bot_logic::TileType::new(27).unwrap()),
            Some(bot_logic::TileType::new(27).unwrap()),
        );
        let actions: Vec<LegalAction> = [0u8, 4, 8, 12, 17, 20, 24, 28, 32, 36, 40, 44, 108, 132]
            .iter()
            .map(|&value| dahai(value))
            .collect();

        let selected = select_discard_action(&context, &actions).unwrap();
        let LegalAction::Dahai { tile } = selected else {
            panic!("expected dahai");
        };
        assert_eq!(tile.tile_type().to_mjai_string(), "C");
    }

    #[test]
    fn shanten_outranks_value_honor() {
        // 中を切るとテンパイ。中が役牌でも向聴を優先して切る
        let hand: Vec<_> = [40u8, 44, 48, 56, 60, 64, 76, 80, 84, 108, 109, 96, 100]
            .iter()
            .map(|&value| tile(value))
            .collect();
        let context = GameContext::from_parts_with_context(
            Some(tile(132)),
            hand,
            vec![],
            Some(bot_logic::TileType::new(27).unwrap()),
            Some(bot_logic::TileType::new(28).unwrap()),
        );
        let actions: Vec<LegalAction> =
            [40u8, 44, 48, 56, 60, 64, 76, 80, 84, 108, 109, 96, 100, 132]
                .iter()
                .map(|&value| dahai(value))
                .collect();

        let selected = select_discard_action(&context, &actions).unwrap();
        let LegalAction::Dahai { tile } = selected else {
            panic!("expected dahai");
        };
        assert_eq!(tile.tile_type().to_mjai_string(), "C");
    }

    #[test]
    fn dora_outranks_value_honor() {
        // 中(役牌・非ドラ)と北(客風・ドラ)。ドラを温存し中を切る
        // ドラ表示 西 -> 北 がドラ
        let hand: Vec<_> = [0u8, 4, 8, 12, 17, 20, 24, 28, 32, 36, 40, 44, 132]
            .iter()
            .map(|&value| tile(value))
            .collect();
        let context = GameContext::from_parts_with_context(
            Some(tile(120)),
            hand,
            vec![tile(116)],
            Some(bot_logic::TileType::new(27).unwrap()),
            Some(bot_logic::TileType::new(28).unwrap()),
        );
        let actions: Vec<LegalAction> = [0u8, 4, 8, 12, 17, 20, 24, 28, 32, 36, 40, 44, 132, 120]
            .iter()
            .map(|&value| dahai(value))
            .collect();

        let selected = select_discard_action(&context, &actions).unwrap();
        let LegalAction::Dahai { tile } = selected else {
            panic!("expected dahai");
        };
        assert_eq!(tile.tile_type().to_mjai_string(), "C");
    }

    fn tiles(values: &[u8]) -> Vec<TileId> {
        values.iter().map(|&value| tile(value)).collect()
    }

    #[test]
    fn uses_visible_tiles_when_present() {
        let hand = tiles(&[0, 4, 8, 12, 17, 20, 24, 28, 32, 48, 53, 56, 36]);
        let mut visible = hand.clone();
        visible.extend(tiles(&[68, 69, 70, 71]));
        let context = GameContext::from_parts_with_visible_tiles(
            Some(tile(68)),
            hand,
            vec![],
            None,
            None,
            visible,
        );
        let actions: Vec<LegalAction> = [0u8, 4, 8, 12, 17, 20, 24, 28, 32, 48, 53, 56, 36, 68]
            .iter()
            .map(|&value| dahai(value))
            .collect();

        let selected = select_discard_action(&context, &actions).unwrap();
        let LegalAction::Dahai { tile } = selected else {
            panic!("expected dahai");
        };
        assert_eq!(tile.tile_type().to_mjai_string(), "9p");
    }

    #[test]
    fn empty_visible_tiles_falls_back_to_context_path() {
        let hand = tiles(&[0, 4, 8, 12, 17, 20, 24, 28, 32, 48, 53, 56, 36]);
        let context =
            GameContext::from_parts_with_context(Some(tile(68)), hand, vec![], None, None);
        let actions: Vec<LegalAction> = [0u8, 4, 8, 12, 17, 20, 24, 28, 32, 48, 53, 56, 36, 68]
            .iter()
            .map(|&value| dahai(value))
            .collect();

        let selected = select_discard_action(&context, &actions).unwrap();
        let LegalAction::Dahai { tile } = selected else {
            panic!("expected dahai");
        };
        assert_eq!(tile.tile_type().to_mjai_string(), "1p");
    }

    #[test]
    fn diagnostic_selection_matches_best_on_legal_candidates() {
        // 診断 (diagnose_discard_evaluations) の selected と通常経路の select_best_evaluation が、
        // 同じ合法候補一覧に対して一致することを確認する。グローバル subscriber に依存しない。
        let hand_values = [0u8, 4, 8, 12, 17, 20, 24, 28, 32, 36, 40, 44, 89];
        let context = GameContext::from_parts_with_context(
            Some(tile(116)),
            hand_values.iter().map(|&value| tile(value)).collect(),
            vec![tile(12)],
            Some(bot_logic::TileType::new(27).unwrap()),
            Some(bot_logic::TileType::new(28).unwrap()),
        );
        let actions: Vec<LegalAction> = hand_values
            .iter()
            .map(|&value| dahai(value))
            .chain([dahai(116)])
            .collect();
        let tiles: Vec<_> = context
            .hand_tiles()
            .iter()
            .copied()
            .chain(context.drawn_tile())
            .collect();

        let legal = retain_legal_dahai_evaluations(
            evaluate_discard_candidates(&context, &tiles),
            &actions,
            context.dora_indicators(),
        );
        let counts = TileCounts::from_tiles(tiles.iter().copied());
        let diagnostic = diagnose_discard_evaluations_with_fixed_melds(
            &counts,
            evaluation_fixed_meld_count(&context),
            &legal,
        );

        assert_eq!(diagnostic.selected.as_ref(), select_best_evaluation(&legal));
        assert!(diagnostic.selected.is_some());
    }

    #[test]
    fn diagnostic_selection_matches_best_on_legal_candidates_with_visible_tiles() {
        let hand = tiles(&[0, 4, 8, 12, 17, 20, 24, 28, 32, 48, 53, 56, 36]);
        let mut visible = hand.clone();
        visible.extend(tiles(&[68, 69, 70, 71]));
        let context = GameContext::from_parts_with_visible_tiles(
            Some(tile(68)),
            hand,
            vec![],
            None,
            None,
            visible,
        );
        let actions: Vec<LegalAction> = [0u8, 4, 8, 12, 17, 20, 24, 28, 32, 48, 53, 56, 36, 68]
            .iter()
            .map(|&value| dahai(value))
            .collect();
        let all_tiles: Vec<_> = context
            .hand_tiles()
            .iter()
            .copied()
            .chain(context.drawn_tile())
            .collect();

        let legal = retain_legal_dahai_evaluations(
            evaluate_discard_candidates(&context, &all_tiles),
            &actions,
            context.dora_indicators(),
        );
        let counts = TileCounts::from_tiles(all_tiles.iter().copied());
        let diagnostic = diagnose_discard_evaluations_with_fixed_melds(
            &counts,
            evaluation_fixed_meld_count(&context),
            &legal,
        );

        assert_eq!(diagnostic.selected.as_ref(), select_best_evaluation(&legal));
        assert!(diagnostic.selected.is_some());
    }

    #[test]
    fn acceptance_tile_diagnostic_preserves_all_shanten_kinds() {
        use bot_logic::{AcceptanceTile, Shanten, TileType};

        let source = AcceptanceTile {
            tile: TileType::from_mjai_type_str("5mr").unwrap(),
            remaining: 3,
            shanten_after_draw: EffectiveShanten::Concealed(Shanten {
                standard: 1,
                chiitoitsu: 2,
                kokushi: 5,
            }),
        };
        let before = source;

        let (tile, remaining, standard, chiitoitsu, kokushi, min) =
            acceptance_tile_diagnostic(&source);

        assert_eq!(tile, "5m");
        assert_eq!(remaining, 3);
        assert_eq!(standard, 1);
        assert_eq!(chiitoitsu, Some(2));
        assert_eq!(kokushi, Some(5));
        assert_eq!(min, 1);
        assert_eq!(source, before);
    }

    #[test]
    fn acceptance_tile_diagnostic_omits_chiitoitsu_and_kokushi_with_fixed_melds() {
        // 副露済み面子がある場合、七対子・国士の向聴数は存在しないので sentinel を出さない。
        use bot_logic::{AcceptanceTile, TileType};

        let source = AcceptanceTile {
            tile: TileType::from_mjai_type_str("5p").unwrap(),
            remaining: 4,
            shanten_after_draw: EffectiveShanten::Melded { standard: -1 },
        };

        let (_, _, standard, chiitoitsu, kokushi, min) = acceptance_tile_diagnostic(&source);

        assert_eq!(standard, -1);
        assert_eq!(chiitoitsu, None);
        assert_eq!(kokushi, None);
        assert_eq!(min, -1);
    }

    #[test]
    fn evaluation_carries_iishanten_shape_after_discard() {
        // 完全一向聴(1m2m3m4m5m6m EE 2p3p 5s6s C)へ余分な 1s を加えた14枚。
        // 1s を切ると完全一向聴へ戻るので、候補評価が Complete を保持する。
        use bot_logic::IishantenShape;

        let hand = tiles(&[0, 4, 8, 12, 17, 20, 108, 109, 40, 44, 88, 92, 132, 72]);
        let context = GameContext::from_parts(None, hand);
        let all_tiles = context.hand_tiles().to_vec();

        let evaluations = evaluate_discard_candidates(&context, &all_tiles);
        let one_s = evaluations
            .iter()
            .find(|evaluation| evaluation.discard == tile(72).tile_type())
            .unwrap();
        assert_eq!(
            one_s.standard_iishanten_shape_after_discard,
            IishantenShape::Complete
        );
    }

    #[test]
    fn does_not_select_non_dahai_actions() {
        let context = GameContext::with_drawn_tile(tile(0));
        let actions = vec![
            LegalAction::Hora,
            LegalAction::Reach,
            LegalAction::Ryukyoku,
            LegalAction::None,
            dahai(0),
        ];
        assert_eq!(select_discard_action(&context, &actions), Some(dahai(0)));
    }

    #[test]
    fn public_action_matches_internal_helper_action() {
        let hand_values = [0, 4, 8, 12, 17, 20, 24, 28, 32, 36, 40, 44, 89];
        let context = GameContext::from_parts(
            Some(tile(116)),
            hand_values.iter().map(|&value| tile(value)).collect(),
        );
        let actions: Vec<LegalAction> = hand_values
            .iter()
            .map(|&value| dahai(value))
            .chain([dahai(116)])
            .collect();

        let selection = select_discard_action_with_evaluation(&context, &actions);
        assert_eq!(select_discard_action(&context, &actions), selection.action);
    }

    #[test]
    fn internal_helper_evaluation_matches_best_selector_when_all_legal() {
        // 全牌種が合法な場合は、合法候補への絞り込み後も汎用 best selector と一致する。
        let hand_values = [0, 4, 8, 12, 17, 20, 24, 28, 32, 36, 40, 44, 89];
        let context = GameContext::from_parts(
            Some(tile(116)),
            hand_values.iter().map(|&value| tile(value)).collect(),
        );
        let actions: Vec<LegalAction> = hand_values
            .iter()
            .map(|&value| dahai(value))
            .chain([dahai(116)])
            .collect();

        let tiles: Vec<_> = context
            .hand_tiles()
            .iter()
            .copied()
            .chain(context.drawn_tile())
            .collect();
        let expected = select_best_discard_evaluation(&context, &tiles);

        let selection = select_discard_action_with_evaluation(&context, &actions);
        assert_eq!(selection.evaluation, expected);
        assert!(selection.evaluation.is_some());
    }

    // 合法 Dahai がある選択では、evaluation と action の TileType が常に一致する。
    fn assert_evaluation_action_types_match(selection: &DiscardActionSelection) {
        let evaluation_type = selection
            .evaluation
            .as_ref()
            .map(|evaluation| evaluation.discard);
        let action_type = selection.action.as_ref().and_then(|action| match action {
            LegalAction::Dahai { tile } => Some(tile.tile_type()),
            _ => None,
        });
        assert_eq!(evaluation_type, action_type);
    }

    #[test]
    fn evaluation_and_action_tile_types_always_match() {
        let hand_values = [0, 4, 8, 12, 17, 20, 24, 28, 32, 36, 40, 44, 89];
        let context = GameContext::from_parts(
            Some(tile(116)),
            hand_values.iter().map(|&value| tile(value)).collect(),
        );
        let actions: Vec<LegalAction> = hand_values
            .iter()
            .map(|&value| dahai(value))
            .chain([dahai(116)])
            .collect();

        let selection = select_discard_action_with_evaluation(&context, &actions);
        assert!(selection.evaluation.is_some());
        assert!(selection.action.is_some());
        assert_evaluation_action_types_match(&selection);
    }

    #[test]
    fn excludes_illegal_global_best_and_picks_best_legal_candidate() {
        // 全体最善候補(浮いた W)が合法 Dahai に含まれない場合、その非合法候補は使わず、
        // 合法候補の中の最善(5s)を選ぶ。
        let hand_values = [0, 4, 8, 12, 17, 20, 24, 28, 32, 36, 40, 44, 89];
        let context = GameContext::from_parts(
            Some(tile(116)),
            hand_values.iter().map(|&value| tile(value)).collect(),
        );
        let tiles: Vec<_> = context
            .hand_tiles()
            .iter()
            .copied()
            .chain(context.drawn_tile())
            .collect();

        let all = evaluate_discard_candidates(&context, &tiles);
        let global_best = select_best_evaluation(&all).unwrap().discard;

        // 全体最善(W=116)を除外し、他の牌種だけを合法にする。
        let actions: Vec<LegalAction> = hand_values.iter().map(|&value| dahai(value)).collect();
        assert!(legal_dahai_tile_for_type(global_best, &actions).is_none());

        let expected_best = select_best_evaluation(&retain_legal_dahai_evaluations(
            evaluate_discard_candidates(&context, &tiles),
            &actions,
            context.dora_indicators(),
        ))
        .unwrap()
        .clone();

        let selection = select_discard_action_with_evaluation(&context, &actions);
        assert_eq!(selection.evaluation.as_ref(), Some(&expected_best));
        assert_ne!(selection.evaluation.as_ref().unwrap().discard, global_best);
        assert_evaluation_action_types_match(&selection);
    }

    #[test]
    fn respects_tsumogiri_constraint_when_only_drawn_tile_is_legal() {
        // 手牌には複数の打牌候補があるが、合法 Dahai はツモ牌(5s)だけ。
        // 全体最善(浮いた W)は手牌内の非合法牌なので使わず、ツモ切りの評価を返す。
        let hand_values = [0, 4, 8, 12, 17, 20, 24, 28, 32, 36, 40, 44, 116];
        let context = GameContext::from_parts(
            Some(tile(89)),
            hand_values.iter().map(|&value| tile(value)).collect(),
        );
        let tiles: Vec<_> = context
            .hand_tiles()
            .iter()
            .copied()
            .chain(context.drawn_tile())
            .collect();
        let global_best = select_best_evaluation(&evaluate_discard_candidates(&context, &tiles))
            .unwrap()
            .discard;
        assert_ne!(global_best, tile(89).tile_type());

        let actions = vec![dahai(89)];
        let selection = select_discard_action_with_evaluation(&context, &actions);
        assert_eq!(
            selection.evaluation.as_ref().unwrap().discard,
            tile(89).tile_type()
        );
        assert_eq!(selection.action, Some(dahai(89)));
        assert_evaluation_action_types_match(&selection);
    }

    #[test]
    fn single_legal_type_is_selected_regardless_of_evaluation() {
        // 合法 Dahai が 1 種類(1m)だけなら、評価上の優劣にかかわらずその牌種を選ぶ。
        let hand_values = [0, 4, 8, 12, 17, 20, 24, 28, 32, 36, 40, 44, 116];
        let context = GameContext::from_parts(
            Some(tile(89)),
            hand_values.iter().map(|&value| tile(value)).collect(),
        );
        let actions = vec![dahai(0)];
        let selection = select_discard_action_with_evaluation(&context, &actions);
        assert_eq!(
            selection.evaluation.as_ref().unwrap().discard,
            tile(0).tile_type()
        );
        assert_eq!(selection.action, Some(dahai(0)));
        assert_evaluation_action_types_match(&selection);
    }

    #[test]
    fn duplicate_same_type_dahai_does_not_duplicate_evaluations() {
        // 赤5m と通常5m の両方が合法でも、5m の評価候補は1件だけ。
        let hand = tiles(&[16, 17, 0, 4]);
        let context = GameContext::from_parts(None, hand);
        let tiles_all: Vec<_> = context.hand_tiles().to_vec();
        let actions = vec![dahai(16), dahai(17), dahai(0), dahai(4)];

        let all = evaluate_discard_candidates(&context, &tiles_all);
        let legal =
            retain_legal_dahai_evaluations(all.clone(), &actions, context.dora_indicators());

        let five_type = tile(17).tile_type();
        assert_eq!(legal.iter().filter(|e| e.discard == five_type).count(), 1);
        // 3牌種(5m,1m,2m)がすべて合法なので、絞り込みで件数は変わらない。
        assert_eq!(legal.len(), all.len());
    }

    #[test]
    fn internal_helper_prefers_black_five_over_red() {
        let context = GameContext::from_parts(None, vec![tile(16), tile(17)]);
        let actions = vec![dahai(16), dahai(17)];
        let selection = select_discard_action_with_evaluation(&context, &actions);
        assert_eq!(selection.action, Some(dahai(17)));
    }

    #[test]
    fn internal_helper_falls_back_to_red_five() {
        let context = GameContext::from_parts(None, vec![tile(16)]);
        let actions = vec![dahai(16)];
        let selection = select_discard_action_with_evaluation(&context, &actions);
        assert_eq!(selection.action, Some(dahai(16)));
    }

    #[test]
    fn reports_none_evaluation_and_action_without_legal_dahai() {
        // 合法 Dahai の牌種(1m)が無い場合、evaluation も action も None にする。
        // 以前は evaluation == Some / action == None を許容していたが、その状態は廃止する。
        let context = GameContext::with_hand_tiles(vec![tile(0)]);
        let actions = vec![dahai(4)];
        let selection = select_discard_action_with_evaluation(&context, &actions);
        assert_eq!(selection.evaluation, None);
        assert_eq!(selection.action, None);
    }

    #[test]
    fn red_five_only_legal_marks_evaluation_as_red() {
        // 赤5m(16)と通常5m(17)を所持するが、合法 Dahai は赤5mだけ。
        // 評価も赤5mの物理牌情報に合わせる。
        let context = GameContext::from_parts(None, vec![tile(16), tile(17)]);
        let actions = vec![dahai(16)];
        let selection = select_discard_action_with_evaluation(&context, &actions);

        let evaluation = selection.evaluation.as_ref().unwrap();
        assert_eq!(selection.action, Some(dahai(16)));
        assert_eq!(evaluation.discard, tile(16).tile_type());
        assert!(evaluation.discards_red_five);
        assert_eq!(evaluation.discarded_dora_count, 1);
        assert_evaluation_action_types_match(&selection);
    }

    #[test]
    fn black_five_only_legal_keeps_evaluation_non_red() {
        // 赤5mと通常5mを所持するが、合法 Dahai は通常5mだけ。赤ドラ分は含めない。
        let context = GameContext::from_parts(None, vec![tile(16), tile(17)]);
        let actions = vec![dahai(17)];
        let selection = select_discard_action_with_evaluation(&context, &actions);

        let evaluation = selection.evaluation.as_ref().unwrap();
        assert_eq!(selection.action, Some(dahai(17)));
        assert_eq!(evaluation.discard, tile(17).tile_type());
        assert!(!evaluation.discards_red_five);
        assert_eq!(evaluation.discarded_dora_count, 0);
        assert_evaluation_action_types_match(&selection);
    }

    #[test]
    fn both_fives_legal_prefers_black_five() {
        // 赤5mと通常5mの両方が合法なら通常5mを優先し、評価も通常5mに合わせる。
        let context = GameContext::from_parts(None, vec![tile(16), tile(17)]);
        let actions = vec![dahai(16), dahai(17)];
        let selection = select_discard_action_with_evaluation(&context, &actions);

        let evaluation = selection.evaluation.as_ref().unwrap();
        assert_eq!(selection.action, Some(dahai(17)));
        assert!(!evaluation.discards_red_five);
        assert_eq!(evaluation.discarded_dora_count, 0);
        assert_evaluation_action_types_match(&selection);
    }

    #[test]
    fn red_five_only_legal_counts_indicator_and_red_dora() {
        // 4m(12)をドラ表示牌にすると5mがドラ。赤5mだけが合法なら表示牌ドラ+赤ドラの2枚。
        let context =
            GameContext::from_parts_with_dora(None, vec![tile(16), tile(17)], vec![tile(12)]);
        let actions = vec![dahai(16)];
        let selection = select_discard_action_with_evaluation(&context, &actions);

        let evaluation = selection.evaluation.as_ref().unwrap();
        assert_eq!(selection.action, Some(dahai(16)));
        assert!(evaluation.discards_red_five);
        assert_eq!(evaluation.discarded_dora_count, 2);
        assert_evaluation_action_types_match(&selection);
    }

    #[test]
    fn both_fives_legal_with_dora_indicator_counts_indicator_only() {
        // 5mがドラでも両方合法なら通常5mを優先し、赤ドラ分は含めず表示牌ドラのみ。
        let context =
            GameContext::from_parts_with_dora(None, vec![tile(16), tile(17)], vec![tile(12)]);
        let actions = vec![dahai(16), dahai(17)];
        let selection = select_discard_action_with_evaluation(&context, &actions);

        let evaluation = selection.evaluation.as_ref().unwrap();
        assert_eq!(selection.action, Some(dahai(17)));
        assert!(!evaluation.discards_red_five);
        assert_eq!(evaluation.discarded_dora_count, 1);
        assert_evaluation_action_types_match(&selection);
    }

    #[test]
    fn legal_evaluations_carry_corrected_physical_fields_before_diagnostic() {
        // 診断 report へ渡す直前の評価(retain 後)が、赤5だけ合法のとき赤5の物理牌情報を持つ。
        let context =
            GameContext::from_parts_with_dora(None, vec![tile(16), tile(17)], vec![tile(12)]);
        let actions = vec![dahai(16)];
        let tiles = context.hand_tiles().to_vec();

        let legal = retain_legal_dahai_evaluations(
            evaluate_discard_candidates(&context, &tiles),
            &actions,
            context.dora_indicators(),
        );

        let five = legal
            .iter()
            .find(|evaluation| evaluation.discard == tile(16).tile_type())
            .unwrap();
        assert!(five.discards_red_five);
        assert_eq!(five.discarded_dora_count, 2);
    }

    // ---- 構造化診断付き選択 (select_discard_action_with_diagnostic) ----

    #[test]
    fn diagnostic_path_selection_matches_normal_path() {
        // 診断付き経路の選択結果は通常経路と一致する。診断は選択に影響しない。
        let hand_values = [0, 4, 8, 12, 17, 20, 24, 28, 32, 36, 40, 44, 89];
        let context = GameContext::from_parts(
            Some(tile(116)),
            hand_values.iter().map(|&value| tile(value)).collect(),
        );
        let actions: Vec<LegalAction> = hand_values
            .iter()
            .map(|&value| dahai(value))
            .chain([dahai(116)])
            .collect();

        let with_diagnostic = select_discard_action_with_diagnostic(&context, &actions, false);
        assert_eq!(
            with_diagnostic.selection,
            select_discard_action_with_evaluation(&context, &actions)
        );
        assert_eq!(
            with_diagnostic.diagnostic.selected,
            with_diagnostic.selection.evaluation
        );
    }

    #[test]
    fn lookahead_is_built_only_on_request_and_does_not_change_selection() {
        // 2手先診断は明示的に要求した場合だけ構築し、選択結果は要求の有無で変わらない。
        // 2手先は重い探索なので、小さい手牌で構造だけを確認する。
        let hand_values = [0, 4, 36, 40, 89];
        let context = GameContext::from_parts(
            Some(tile(90)),
            hand_values.iter().map(|&value| tile(value)).collect(),
        );
        let actions: Vec<LegalAction> = hand_values
            .iter()
            .map(|&value| dahai(value))
            .chain([dahai(90)])
            .collect();

        let without = select_discard_action_with_diagnostic(&context, &actions, false);
        let with = select_discard_action_with_diagnostic(&context, &actions, true);

        assert!(without.lookahead.is_none());
        assert_eq!(without.selection, with.selection);
        assert_eq!(without.diagnostic, with.diagnostic);

        let lookahead = with.lookahead.expect("lookahead built on request");
        assert!(with.diagnostic.candidates.len() > 1);
        assert_eq!(lookahead.candidates.len(), with.diagnostic.candidates.len());
        for (second_ply, candidate) in lookahead
            .candidates
            .iter()
            .zip(with.diagnostic.candidates.iter())
        {
            assert_eq!(second_ply.discard, candidate.evaluation.discard);
        }
    }

    #[test]
    fn diagnostic_candidates_contain_only_legal_dahai_types() {
        // 合法 Dahai が一部だけの局面では、診断候補も合法牌種だけに絞られる。
        let hand_values = [0, 4, 8, 12, 17, 20, 24, 28, 32, 36, 40, 44, 89];
        let context = GameContext::from_parts(
            Some(tile(116)),
            hand_values.iter().map(|&value| tile(value)).collect(),
        );
        let actions = vec![dahai(0), dahai(89), dahai(116)];

        let with_diagnostic = select_discard_action_with_diagnostic(&context, &actions, false);
        let candidate_types: Vec<_> = with_diagnostic
            .diagnostic
            .candidates
            .iter()
            .map(|candidate| candidate.evaluation.discard)
            .collect();

        assert_eq!(
            candidate_types,
            vec![
                tile(0).tile_type(),
                tile(89).tile_type(),
                tile(116).tile_type()
            ]
        );
        assert_eq!(
            with_diagnostic
                .diagnostic
                .candidates
                .iter()
                .filter(|candidate| candidate.selected)
                .count(),
            1
        );
    }

    #[test]
    fn diagnostic_candidates_carry_physical_corrected_fields() {
        // 赤5mだけが合法な局面では、診断候補の物理牌依存フィールドも赤5mへ補正済み。
        let context =
            GameContext::from_parts_with_dora(None, vec![tile(16), tile(17)], vec![tile(12)]);
        let actions = vec![dahai(16)];

        let with_diagnostic = select_discard_action_with_diagnostic(&context, &actions, false);
        let five = with_diagnostic
            .diagnostic
            .candidates
            .iter()
            .find(|candidate| candidate.evaluation.discard == tile(16).tile_type())
            .unwrap();

        assert_eq!(with_diagnostic.selection.action, Some(dahai(16)));
        assert!(five.evaluation.discards_red_five);
        assert_eq!(five.evaluation.discarded_dora_count, 2);
    }

    // ---- 副露済み手牌の通常打牌評価 ----

    use crate::meld::{Meld, MeldKind};

    // 白ポン1組。副露の種類によらず完成済み面子1として数える。
    fn white_dragon_pon() -> Meld {
        Meld::new(
            MeldKind::Pon,
            vec![tile(124), tile(125), tile(126)],
            Some(tile(124)),
        )
    }

    // 123456m 78p 55s (concealed) + ツモ N。白ポン1組を持つ player 0 の局面。
    fn one_meld_context(melds: [Vec<Meld>; 4], player_id: Option<u8>) -> GameContext {
        let hand = tiles(&[0, 4, 8, 12, 17, 20, 60, 64, 89, 90]);
        GameContext::from_parts_with_melds(
            Some(tile(120)),
            hand,
            vec![],
            None,
            None,
            Vec::new(),
            player_id,
            None,
            Default::default(),
            [false; 4],
            melds,
        )
    }

    fn one_meld_actions() -> Vec<LegalAction> {
        [0u8, 4, 8, 12, 17, 20, 60, 64, 89, 90, 120]
            .iter()
            .map(|&value| dahai(value))
            .collect()
    }

    fn acceptance_summary(evaluation: &DiscardEvaluation) -> Vec<(String, u8)> {
        evaluation
            .acceptance_after_discard
            .tiles
            .iter()
            .map(|entry| (entry.tile.to_mjai_string(), entry.remaining))
            .collect()
    }

    #[test]
    fn own_meld_makes_discard_evaluation_fixed_meld_aware() {
        let context = one_meld_context([vec![white_dragon_pon()], vec![], vec![], vec![]], Some(0));
        assert_eq!(
            context.own_fixed_meld_count().map(FixedMeldCount::get),
            Some(1)
        );

        let selection = select_discard_action_with_evaluation(&context, &one_meld_actions());
        let evaluation = selection.evaluation.as_ref().unwrap();

        assert_eq!(evaluation.discard.to_mjai_string(), "N");
        assert_eq!(evaluation.min_shanten_after_discard(), 0);
        assert_eq!(evaluation.shanten_after_discard.standard(), 0);
        assert_eq!(evaluation.shanten_after_discard.concealed(), None);
        assert_eq!(
            acceptance_summary(evaluation),
            vec![("6p".to_string(), 4), ("9p".to_string(), 4)]
        );
        assert_eq!(evaluation.acceptance_total_remaining(), 8);
        assert_eq!(selection.action, Some(dahai(120)));
    }

    #[test]
    fn opponent_melds_do_not_change_own_discard_evaluation() {
        // 他家の副露は自分の向聴数に影響しないため、門前評価のままになる。
        let context = one_meld_context([vec![], vec![white_dragon_pon()], vec![], vec![]], Some(0));
        assert_eq!(context.own_fixed_meld_count(), Some(FixedMeldCount::NONE));

        let selection = select_discard_action_with_evaluation(&context, &one_meld_actions());
        let evaluation = selection.evaluation.as_ref().unwrap();
        assert_eq!(evaluation.min_shanten_after_discard(), 2);
        assert!(evaluation.shanten_after_discard.concealed().is_some());
    }

    #[test]
    fn missing_player_id_falls_back_to_the_concealed_evaluation() {
        // player_id が無い場合は player 0 の副露数を推測せず、門前評価へフォールバックする。
        let context = one_meld_context([vec![white_dragon_pon()], vec![], vec![], vec![]], None);
        assert_eq!(context.own_fixed_meld_count(), None);
        assert_eq!(evaluation_fixed_meld_count(&context), FixedMeldCount::NONE);

        let selection = select_discard_action_with_evaluation(&context, &one_meld_actions());
        let evaluation = selection.evaluation.as_ref().unwrap();
        assert_eq!(evaluation.min_shanten_after_discard(), 2);
        assert!(evaluation.shanten_after_discard.concealed().is_some());
    }

    #[test]
    fn diagnostic_path_shares_the_fixed_meld_aware_evaluation() {
        let context = one_meld_context([vec![white_dragon_pon()], vec![], vec![], vec![]], Some(0));
        let actions = one_meld_actions();

        let with_diagnostic = select_discard_action_with_diagnostic(&context, &actions, false);
        assert_eq!(
            with_diagnostic.selection,
            select_discard_action_with_evaluation(&context, &actions)
        );

        let selected = with_diagnostic.diagnostic.selected.as_ref().unwrap();
        assert_eq!(selected.discard.to_mjai_string(), "N");
        assert_eq!(selected.min_shanten_after_discard(), 0);

        // 診断の block context も本番評価と同じ副露済み面子数で求める。
        let counts = TileCounts::from_tiles(
            context
                .hand_tiles()
                .iter()
                .copied()
                .chain(context.drawn_tile()),
        );
        for candidate in &with_diagnostic.diagnostic.candidates {
            assert_eq!(
                candidate.block_context,
                bot_logic::discard_block_context_with_fixed_melds(
                    &counts,
                    candidate.evaluation.discard,
                    FixedMeldCount::new(1).unwrap(),
                )
            );
        }
    }

    #[test]
    fn fixed_meld_evaluation_uses_visible_tiles() {
        // 他家に見えている 6p 2枚を反映しても、副露込みのテンパイ判定は維持する。
        let hand = tiles(&[0, 4, 8, 12, 17, 20, 60, 64, 89, 90]);
        let mut visible = hand.clone();
        visible.push(tile(120));
        visible.extend(tiles(&[56, 57]));
        let context = GameContext::from_parts_with_melds(
            Some(tile(120)),
            hand,
            vec![],
            None,
            None,
            visible,
            Some(0),
            None,
            Default::default(),
            [false; 4],
            [vec![white_dragon_pon()], vec![], vec![], vec![]],
        );

        let selection = select_discard_action_with_evaluation(&context, &one_meld_actions());
        let evaluation = selection.evaluation.as_ref().unwrap();

        assert_eq!(evaluation.discard.to_mjai_string(), "N");
        assert_eq!(evaluation.min_shanten_after_discard(), 0);
        assert_eq!(
            acceptance_summary(evaluation),
            vec![("6p".to_string(), 2), ("9p".to_string(), 4)]
        );
        assert_eq!(evaluation.acceptance_total_remaining(), 6);
    }

    #[test]
    fn diagnostic_is_empty_without_legal_dahai() {
        let context = GameContext::with_hand_tiles(vec![tile(0)]);
        let actions = vec![LegalAction::Reach, LegalAction::None];

        let with_diagnostic = select_discard_action_with_diagnostic(&context, &actions, false);
        assert_eq!(with_diagnostic.selection.action, None);
        assert_eq!(with_diagnostic.selection.evaluation, None);
        assert_eq!(with_diagnostic.diagnostic.selected, None);
        assert!(with_diagnostic.diagnostic.candidates.is_empty());
    }
}
