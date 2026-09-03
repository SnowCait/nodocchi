use crate::action::{LegalAction, preferred_dahai_action_for_type};
use crate::context::GameContext;
use crate::current_tenpai_continuation::{
    CurrentTenpaiContinuationDiagnostic, CurrentTenpaiContinuationInputs,
    diagnose_current_tenpai_continuation,
};
use crate::damaten_value::tenpai_completed_hands_after_discard;
use crate::offense_value::{
    TenpaiOffenseEvaluation, TenpaiOffenseMode, TenpaiOffenseValue,
    evaluate_tenpai_offense_with_hands,
};
use crate::prospective_value::{
    ProductionProspectiveValuator, ProspectiveLookaheadDiagnostic,
    evaluate_prospective_lookahead_value,
};
use crate::reach_policy::{
    ReachTimingDiagnostic, decide_permanent_furiten_reach_timing, evaluates_reach_timing,
};
use crate::tenpai_continuation::{
    TenpaiContinuationDiagnostic, TenpaiContinuationInputs, TenpaiSelfTsumoComparison,
    diagnose_tenpai_continuation, tenpai_candidate_self_tsumo_comparison,
};
use crate::tenpai_scoring::tenpai_tsumo_value_from_hands;
use bot_logic::{
    CurrentTenpaiMetrics, DiscardCandidateDiagnostic, DiscardDecisionDiagnostic, DiscardEvaluation,
    DiscardFuritenDiagnostic, EffectiveAcceptanceTile, EffectiveShanten, FixedMeldCount,
    ForwardMetrics, LookaheadDiagnostic, LookaheadInputs, OwnDiscards, SelfTsumoFacts,
    TenpaiWaitAvailability, TileCounts, TileId, TileType, best_discard_selection_index,
    best_discard_selection_index_with_metrics, current_tenpai_continuation_targets,
    diagnose_discard_evaluations_with_metrics, diagnose_discard_furiten, diagnose_lookahead,
    discard_tenpai_wait_availability, evaluate_discards_from_tiles_with_fixed_melds_and_context,
    evaluate_discards_from_tiles_with_fixed_melds_and_visible_tiles, forward_metrics,
    forward_metrics_for_candidate, forward_metrics_from_lookahead, split_discarded_tile,
    tsumo_hit_probability,
};

const LOG_TARGET: &str = "bot_core::discard_selection";

/// 1向聴の向聴数。押し引きへ渡す前方集計値の対象。
const IISHANTEN_SHANTEN: i8 = 1;
/// 現在聴牌の offense value 比較を適用する向聴数。
const TENPAI_SHANTEN: i8 = 0;

/// 通常打牌選択の内部結果。
///
/// - `evaluation`: 合法 Dahai 候補の中の最善 `DiscardEvaluation`。合法候補が無ければ `None`。
/// - `action`: `evaluation` に対応する合法 Dahai。
/// - `iishanten_forward_metrics`: 選んだ打牌が1向聴の場合の前方集計値。
/// - `tenpai_wait` / `tenpai_offense_value` / `tenpai_reach_timing`: 複数の現在聴牌候補を比較した
///   場合の選択済み評価。
///
/// `evaluation` と `action` は常に同時に `Some` / `None` になり、`Some` のときは牌種が一致する。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DiscardActionSelection {
    pub evaluation: Option<DiscardEvaluation>,
    pub action: Option<LegalAction>,
    /// 選んだ打牌が1向聴の場合の前方集計値。1向聴でなければ `None`。
    ///
    /// 打牌選択には影響しない観測値で、押し引きの offense state と診断へそのまま渡す。値の
    /// 出どころは選択に使った前方集計値そのもので、押し引き側で集計し直さない
    /// ([`selected_iishanten_forward_metrics`])。
    pub iishanten_forward_metrics: Option<ForwardMetrics>,
    /// 複数の現在聴牌候補を比較した場合に、選ばれた候補について計算済みの待ち。
    pub tenpai_wait: Option<TenpaiWaitAvailability>,
    /// 複数の現在聴牌候補を比較した場合に、選ばれた候補について計算済みの offense value。
    pub tenpai_offense_value: Option<TenpaiOffenseValue>,
    /// offense mode の決定時に計算済みなら、そのダマ打点診断。
    pub damaten_value: Option<crate::damaten_value::DamatenValueDiagnostic>,
    /// 候補比較が継続 timing を評価した場合に、選ばれた候補について計算済みの Reach timing。
    ///
    /// 候補比較の対象外だった場合 (現在聴牌候補が1件・cohort が新しい継続軸の対象外・継続評価を
    /// 走らせていない) は `None` で、後段のリーチ判断が選択済み1候補について従来どおり評価する。
    /// 値は既存 [`decide_permanent_furiten_reach_timing`] の結論そのもので、この field のために
    /// timing policy を複製しない。
    pub tenpai_reach_timing: Option<ReachTimingDiagnostic>,
}

/// 2手先診断をどこまで構築するか。
///
/// どの段階でも打牌選択の結果は変わらない。深い段ほど探索が重くなるため、必要な経路が明示的に
/// 指定する。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum LookaheadDiagnosticScope {
    /// 2手先診断を構築しない。
    #[default]
    None,
    /// 全合法候補の2手先診断を構築する。
    Lookahead,
    /// 2手先診断に加えて、1向聴候補の same-shanten の枝をテンパイまでもう1段追う。
    ///
    /// 「same-shanten ツモ → 2手目 → 受け入れのツモ → 3手目 → テンパイ」まで探索するため
    /// [`Self::Lookahead`] よりさらに重い。打牌選択にも押し引きにも使わない観測値。
    SameShantenDownstream,
}

impl LookaheadDiagnosticScope {
    fn builds_lookahead(self) -> bool {
        !matches!(self, Self::None)
    }

    fn builds_same_shanten_downstream(self) -> bool {
        matches!(self, Self::SameShantenDownstream)
    }
}

/// 通常打牌選択の結果と、その選択に使った全合法候補の構造化診断。
///
/// `selection` は `select_discard_action_with_evaluation()` と同じ helper で導出するため、
/// 診断を付けても選択結果は変わらない。`diagnostic` / `lookahead` は解析専用の追加情報。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DiscardActionSelectionWithDiagnostic {
    pub selection: DiscardActionSelection,
    pub diagnostic: DiscardDecisionDiagnostic,
    /// 全合法候補のフリテン診断。`diagnostic` と同じ候補集合・同じ順序。
    ///
    /// 打牌選択には一切使わない解析専用の情報で、選択結果を変えない。
    pub furiten: Vec<DiscardFuritenDiagnostic>,
    /// 全合法候補の詳細な2手先診断。要求された場合だけ構築する。
    ///
    /// 構築した場合は、選択に使う weighted forward metric もこの枝評価から集計して同じ枝を
    /// 2回計算しない。集計対象と集計規則は選択専用経路と同じなので、詳細診断の有無で選択結果は
    /// 変わらない。
    pub lookahead: Option<LookaheadDiagnostic>,
    /// `lookahead` の各枝が選んだ2手目打牌の先にあるテンパイの将来打点。`lookahead` を構築した
    /// 場合だけ持ち、同じ候補集合・同じ順序になる。
    ///
    /// 打牌選択にも2手目 `next_discard` の選択にも使わない解析専用の情報で、選択結果を変えない。
    pub lookahead_value: Option<ProspectiveLookaheadDiagnostic>,
    /// `lookahead` / `lookahead_value` の枝から絞り込んだ、現在聴牌候補をダマで継続した場合の
    /// 次の1巡。
    ///
    /// 2手先診断を構築し、かつ自分が未リーチと確定している局面だけ持つ。枝も打点も既存診断が
    /// 構築済みのものを絞り込むだけで、この診断のために探索も点数計算もやり直さない。
    ///
    /// 打牌選択にも押し引きにもリーチ判断にも使わない解析専用の情報で、選択結果を変えない。
    pub tenpai_continuation: Option<TenpaiContinuationDiagnostic>,
    /// 恒常フリテンが確定した現在聴牌 cohort の、`reach now` / `defer → forced Reach` 観測。
    ///
    /// 2手先診断を要求した場合だけ持つ。対象は AllPermanentFuriten cohort かつ base offense
    /// mode がリーチの候補だけで、構築済み `tenpai_continuation` の self-tsumo 比較を再利用する。
    ///
    /// 打牌選択にもリーチ判断にもリーチ timing にも使わない解析専用の情報で、選択結果を
    /// 変えない。
    pub current_tenpai_continuation: Option<CurrentTenpaiContinuationDiagnostic>,
    /// self-tsumo continuation の集計に使った事実。材料が揃わない局面では `None`。
    ///
    /// 選択が実際に使った値そのもので、診断のために求め直さない。
    pub self_tsumo_facts: Option<SelfTsumoFacts>,
}

// 合法 Dahai へ絞り込み・物理牌補正済みの打牌候補評価集合と、その評価対象の物理牌一覧。
// 本番選択・構造化診断・tracing ログはすべてこの集合を共有する。
struct LegalDiscardEvaluations {
    tiles: Vec<TileId>,
    evaluations: Vec<DiscardEvaluation>,
}

// 打牌選択に使う前方集計値。`evaluations` と同じ順序・同じ件数で、前方評価を
// 計算しなかった候補は `None`。本番選択・構造化診断・tracing ログはこの1組を共有し、同じ枝を
// 二重に評価しない。
type SelectionForwardMetrics = Vec<ForwardMetrics>;

/// 現在聴牌候補1件について、selection のために一度だけ求めた既存 wait / offense evaluation。
#[derive(Debug, Clone, PartialEq, Eq, Default)]
struct CurrentTenpaiCandidateEvaluation {
    wait: Option<TenpaiWaitAvailability>,
    offense: Option<TenpaiOffenseEvaluation>,
    expected_self_tsumo_value: Option<u64>,
    /// `expected_self_tsumo_value` と同じ terminal tenpai から求めたツモ和了確率。診断専用。
    self_tsumo_hit_probability: Option<u64>,
    /// 恒常フリテン確定 cohort の候補だけについて、既存 Reach timing policy を適用した結果。
    ///
    /// 対象外の cohort・対象外の候補では継続評価そのものを走らせないので `None`。
    continuation_timing: Option<ReachTimingDiagnostic>,
}

type CurrentTenpaiCandidateEvaluations = Vec<CurrentTenpaiCandidateEvaluation>;

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
    let tenpai_wait = selection_forward_metrics(context, &legal.tiles, &legal.evaluations);
    let current_tenpai = current_tenpai_candidate_evaluations(
        context,
        &legal.tiles,
        &legal.evaluations,
        legal_actions,
    );

    if tracing::enabled!(target: LOG_TARGET, tracing::Level::DEBUG) {
        log_discard_diagnostic(
            context,
            &legal.tiles,
            &diagnose_legal_evaluations(context, &legal, &tenpai_wait, &current_tenpai),
        );
    }

    selection_from_legal_evaluations(
        context,
        &legal,
        &tenpai_wait,
        &current_tenpai,
        legal_actions,
    )
}

/// `select_discard_action_with_evaluation()` と同じ選択結果に、全合法候補の構造化診断を添えて返す。
///
/// 合法候補の絞り込み・物理牌補正・最善選択はすべて通常経路と同じ helper を通すため、選択結果は
/// `select_discard_action_with_evaluation()` と一致する。`diagnostic` / `lookahead` は解析専用の
/// 追加情報で、候補ごとの形の内訳や2手先評価など通常経路では計算しない値を含むため、診断が必要な
/// 経路からのみ呼ぶ。
///
/// `scope` は2手先診断をどこまで構築するかどうか。2手先は
/// 「打牌候補 × 受け入れ牌 × 次打牌候補」の探索になり通常診断よりさらに重いため、明示的に
/// 要求された場合だけ構築する。構築の有無は選択結果を変えない。
pub(crate) fn select_discard_action_with_diagnostic(
    context: &GameContext,
    legal_actions: &[LegalAction],
    scope: LookaheadDiagnosticScope,
) -> DiscardActionSelectionWithDiagnostic {
    let legal = legal_discard_evaluations(context, legal_actions);

    // 2手先診断を構築する場合は、その枝評価から選択用の前方集計値も求める。同じ
    // 「現在打牌 × 受け入れ牌 × 次打牌評価」を2回計算しない。
    let valuator = ProductionProspectiveValuator::new(context);
    let inputs = lookahead_inputs(context, &legal.tiles, &valuator, scope);
    let lookahead = scope
        .builds_lookahead()
        .then(|| diagnose_lookahead(&inputs, &legal.evaluations));
    let tenpai_wait = match lookahead.as_ref() {
        Some(lookahead) => forward_metrics_from_lookahead(&inputs, &legal.evaluations, lookahead),
        None => forward_metrics(&inputs, &legal.evaluations),
    };
    // 将来打点は構築済みの2手先診断の枝をそのまま評価対象にする。枝の探索も打牌比較もやり直さない。
    let lookahead_value = lookahead.as_ref().map(|lookahead| {
        evaluate_prospective_lookahead_value(context, &legal.tiles, &legal.evaluations, lookahead)
    });

    // 現在聴牌のダマ継続も構築済みの枝の絞り込みだけで、追加の探索は行わない。self-tsumo 比較の
    // ための点数計算も、打牌選択が使ったものと同じ評価器・同じ事実をそのまま渡す。現在局面の
    // リーチ可否は production のリーチ判断と同じく実際の合法手を source of truth にする。
    let tenpai_continuation =
        lookahead
            .as_ref()
            .zip(lookahead_value.as_ref())
            .and_then(|(lookahead, value)| {
                diagnose_tenpai_continuation(&TenpaiContinuationInputs {
                    context,
                    tiles: &legal.tiles,
                    valuator: &valuator,
                    reach_legal: legal_actions
                        .iter()
                        .any(|action| matches!(action, LegalAction::Reach)),
                    self_tsumo_facts: inputs.self_tsumo_facts(),
                    evaluations: &legal.evaluations,
                    lookahead,
                    value,
                })
            });

    // 選択が使う継続 timing も、構築済みの全候補継続診断から取り出す。同じ候補の2手先評価を
    // 選択用にもう一度走らせない。診断を構築していない場合だけ通常経路と同じ1候補評価を通る。
    let current_tenpai = current_tenpai_candidate_evaluations_with_continuation(
        context,
        &legal.tiles,
        &legal.evaluations,
        legal_actions,
        tenpai_continuation.as_ref(),
    );

    let diagnostic = diagnose_legal_evaluations(context, &legal, &tenpai_wait, &current_tenpai);

    if tracing::enabled!(target: LOG_TARGET, tracing::Level::DEBUG) {
        log_discard_diagnostic(context, &legal.tiles, &diagnostic);
    }

    // 恒常フリテン確定 cohort の継続 timing は構築済みの2手先継続診断から self-tsumo 比較を
    // 取り出し、このために探索・将来打点・待ち・scoring を再実行しない。cohort 分類も base
    // offense mode も、打牌選択が既に求めた値そのものを渡す。
    let current_tenpai_continuation = tenpai_continuation.as_ref().map(|tenpai_continuation| {
        diagnose_current_tenpai_continuation(&CurrentTenpaiContinuationInputs {
            evaluations: &legal.evaluations,
            metrics: &current_tenpai_metrics(&current_tenpai),
            offense_modes: &current_tenpai_offense_modes(&current_tenpai),
            tenpai_continuation,
        })
    });

    DiscardActionSelectionWithDiagnostic {
        selection: selection_from_legal_evaluations(
            context,
            &legal,
            &tenpai_wait,
            &current_tenpai,
            legal_actions,
        ),
        diagnostic,
        furiten: furiten_from_legal_evaluations(context, &legal, &current_tenpai),
        lookahead,
        lookahead_value,
        tenpai_continuation,
        current_tenpai_continuation,
        self_tsumo_facts: inputs.self_tsumo_facts(),
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
//
// 選んだ打牌が1向聴の場合は、押し引きが観測する前方集計値も同時に決める。選択で使った
// `tenpai_wait` をそのまま再利用するので、比較に使った値と押し引きへ渡る値は同じになる。
fn selection_from_legal_evaluations(
    context: &GameContext,
    legal: &LegalDiscardEvaluations,
    tenpai_wait: &[ForwardMetrics],
    current_tenpai: &[CurrentTenpaiCandidateEvaluation],
    legal_actions: &[LegalAction],
) -> DiscardActionSelection {
    let current_tenpai_metrics = current_tenpai_metrics(current_tenpai);
    let selected = best_discard_selection_index_with_metrics(
        &legal.evaluations,
        tenpai_wait,
        &current_tenpai_metrics,
    );
    let evaluation = selected.map(|index| legal.evaluations[index].clone());
    let action = evaluation
        .as_ref()
        .and_then(|evaluation| legal_dahai_for_evaluation(evaluation, legal_actions));
    let iishanten_forward_metrics = selected.and_then(|index| {
        selected_iishanten_forward_metrics(
            context,
            &legal.tiles,
            &legal.evaluations[index],
            tenpai_wait.get(index).copied().unwrap_or_default(),
        )
    });
    let selected_tenpai = selected.and_then(|index| current_tenpai.get(index));

    DiscardActionSelection {
        evaluation,
        action,
        iishanten_forward_metrics,
        tenpai_wait: selected_tenpai.and_then(|value| value.wait.clone()),
        tenpai_offense_value: selected_tenpai
            .and_then(|value| value.offense.as_ref())
            .map(|value| value.offense),
        damaten_value: selected_tenpai
            .and_then(|value| value.offense.as_ref())
            .and_then(|value| value.damaten_value.clone()),
        // 候補比較が使った timing をそのまま転記する。後段のリーチ判断は同じ候補について
        // 継続評価をやり直さない。
        tenpai_reach_timing: selected_tenpai.and_then(|value| value.continuation_timing),
    }
}

/// 最善向聴が現在聴牌で、競合する合法候補が複数ある場合だけ既存 wait / offense evaluator を
/// 各候補へ適用する。1向聴・2向聴以上・聴牌候補1件の production behavior と計算量は変えない。
///
/// 恒常フリテンが全候補で確定し、全候補の base offense mode がリーチの cohort についてだけ、
/// 続けて既存の1候補継続評価を通した timing 込みの self-tsumo value も求める
/// ([`attach_current_tenpai_continuation`])。それ以外の cohort では継続評価を構築しないので、
/// 従来の計算量のまま変わらない。
fn current_tenpai_candidate_evaluations(
    context: &GameContext,
    tiles: &[TileId],
    evaluations: &[DiscardEvaluation],
    legal_actions: &[LegalAction],
) -> CurrentTenpaiCandidateEvaluations {
    current_tenpai_candidate_evaluations_with_continuation(
        context,
        tiles,
        evaluations,
        legal_actions,
        None,
    )
}

/// 2手先継続診断を構築済みの経路のための現在聴牌候補評価。
///
/// `tenpai_continuation` を渡した場合、継続 timing の材料は構築済みの全候補継続診断から取り出す
/// だけで、同じ候補の2手先評価を選択用にもう一度走らせない。渡さない場合は選択専用経路と同じく
/// 対象候補についてだけ既存の1候補継続 helper を通す。どちらの経路も対象の決め方も timing policy
/// も同じで、選択結果は診断の有無で変わらない。
fn current_tenpai_candidate_evaluations_with_continuation(
    context: &GameContext,
    tiles: &[TileId],
    evaluations: &[DiscardEvaluation],
    legal_actions: &[LegalAction],
    tenpai_continuation: Option<&TenpaiContinuationDiagnostic>,
) -> CurrentTenpaiCandidateEvaluations {
    let mut candidates =
        base_current_tenpai_candidate_evaluations(context, tiles, evaluations, legal_actions);
    attach_current_tenpai_continuation(
        context,
        evaluations,
        &mut candidates,
        legal_actions
            .iter()
            .any(|action| matches!(action, LegalAction::Reach)),
        match tenpai_continuation {
            Some(continuation) => CurrentTenpaiContinuationSource::Diagnostic(continuation),
            None => CurrentTenpaiContinuationSource::Candidate(context),
        },
    );
    candidates
}

// 現在聴牌候補ごとの既存 wait / offense / self-tsumo 評価。継続 timing はまだ持たない。
fn base_current_tenpai_candidate_evaluations(
    context: &GameContext,
    tiles: &[TileId],
    evaluations: &[DiscardEvaluation],
    legal_actions: &[LegalAction],
) -> CurrentTenpaiCandidateEvaluations {
    let best_shanten = evaluations
        .iter()
        .map(DiscardEvaluation::min_shanten_after_discard)
        .min();
    let target_count = evaluations
        .iter()
        .filter(|evaluation| evaluation.min_shanten_after_discard() == TENPAI_SHANTEN)
        .count();
    if best_shanten != Some(TENPAI_SHANTEN) || target_count <= 1 {
        return vec![CurrentTenpaiCandidateEvaluation::default(); evaluations.len()];
    }

    // 現在聴牌候補も1向聴の既存 self-tsumo 軸と同じ exact facts を使う。ここでは候補選択前の
    // base Reach / Damaten mode だけを使い、選択済み候補に対する Reach timing は評価しない。
    let valuator = ProductionProspectiveValuator::new(context);
    let self_tsumo_facts =
        lookahead_inputs(context, tiles, &valuator, LookaheadDiagnosticScope::None)
            .self_tsumo_facts();

    evaluations
        .iter()
        .map(|evaluation| {
            if evaluation.min_shanten_after_discard() != TENPAI_SHANTEN {
                return CurrentTenpaiCandidateEvaluation::default();
            }
            let wait = selected_discard_tenpai_wait_availability(context, evaluation);
            let hands = wait
                .as_ref()
                .and_then(|wait| tenpai_completed_hands_after_discard(context, evaluation, wait));
            let offense = wait.as_ref().map(|wait| {
                evaluate_tenpai_offense_with_hands(context, wait, legal_actions, hands.as_ref())
            });
            // 期待支払いとツモ和了確率は同じ terminal tenpai から一度に求め、点数計算も待ちも
            // 二重に評価しない。
            let self_tsumo = offense
                .as_ref()
                .zip(self_tsumo_facts)
                .and_then(|(offense, facts)| {
                    let hands = hands.as_ref()?;
                    let terminal =
                        tenpai_tsumo_value_from_hands(context, hands, offense.offense.mode)?;
                    Some((
                        terminal.expected_payment(facts.unknown_tiles, facts.own_future_draws),
                        tsumo_hit_probability(
                            facts.unknown_tiles,
                            terminal.winning_remaining,
                            facts.own_future_draws,
                        ),
                    ))
                });
            CurrentTenpaiCandidateEvaluation {
                wait,
                offense,
                expected_self_tsumo_value: self_tsumo.map(|(value, _)| value),
                self_tsumo_hit_probability: self_tsumo.map(|(_, probability)| probability),
                continuation_timing: None,
            }
        })
        .collect()
}

/// 継続 self-tsumo 比較の取得元。
///
/// どちらも同じ [`TenpaiSelfTsumoComparison`] を返し、対象の決め方も timing policy も共有する。
/// 継続の探索・将来打点・待ち・scoring はどちらの経路でも既存基盤そのままで、比較のために評価を
/// 追加しない。
enum CurrentTenpaiContinuationSource<'a> {
    /// 選択専用経路。対象候補についてだけ既存の1候補継続 helper を通す。
    Candidate(&'a GameContext),
    /// 2手先継続診断を構築済みの経路。全候補分の既存診断からそのまま取り出す。
    Diagnostic(&'a TenpaiContinuationDiagnostic),
}

impl CurrentTenpaiContinuationSource<'_> {
    fn self_tsumo(
        &self,
        evaluation: &DiscardEvaluation,
        reach_legal: bool,
    ) -> Option<TenpaiSelfTsumoComparison> {
        match self {
            Self::Candidate(context) => {
                tenpai_candidate_self_tsumo_comparison(context, evaluation, reach_legal)
            }
            Self::Diagnostic(continuation) => continuation
                .candidate(evaluation.discard)
                .map(|candidate| candidate.self_tsumo),
        }
    }
}

/// 恒常フリテン確定 cohort の候補について、既存 Reach timing policy を適用した結果を付ける。
///
/// 対象は「cohort が AllPermanentFuriten かつ cohort の全候補の base offense mode がリーチ」の
/// 候補だけで、cohort 判定は打牌選択と同じ [`current_tenpai_continuation_targets`] が source of
/// truth。さらに既存 [`evaluates_reach_timing`] の適用条件も満たす候補に限ることで、選択済み
/// 候補へ後段で適用される production の Reach timing と同じ意味の値になる。
///
/// 継続比較そのものは `source` が持つ既存評価で、この経路は探索も打点計算も待ち計算も持たない。
/// timing は取得元に依らず既存 [`decide_permanent_furiten_reach_timing`] だけが決める。対象外の
/// cohort では継続比較を1件も取得しない。
fn attach_current_tenpai_continuation(
    context: &GameContext,
    evaluations: &[DiscardEvaluation],
    candidates: &mut CurrentTenpaiCandidateEvaluations,
    reach_legal: bool,
    source: CurrentTenpaiContinuationSource,
) {
    // 残り自摸機会が確定しない局面では self-tsumo 確率模型の材料が揃わず、継続比較のどの値も
    // 確定しない。同じ材料から決まる cohort の expected self-tsumo value も確定しないので、
    // 評価しても必ず既存軸へ落ちる。exact fact だけで安く分かるので2手先評価の前に外す。
    if !reach_legal || own_future_draws(context).is_none() {
        return;
    }

    let metrics = current_tenpai_metrics(candidates);
    let base_reach: Vec<bool> = current_tenpai_offense_modes(candidates)
        .into_iter()
        .map(|mode| mode == Some(TenpaiOffenseMode::Reach))
        .collect();
    for (index, target) in current_tenpai_continuation_targets(evaluations, &metrics, &base_reach)
        .into_iter()
        .enumerate()
    {
        if !target {
            continue;
        }
        let candidate = &mut candidates[index];
        let evaluates_timing = candidate.wait.as_ref().is_some_and(|wait| {
            evaluates_reach_timing(wait.permanent_furiten(), wait.tsumo_remaining)
        });
        if !evaluates_timing {
            continue;
        }

        // ここへ来るのは合法手にリーチがあった経路だけ。リーチ可否は決め直さない。
        let comparison = source.self_tsumo(&evaluations[index], reach_legal);
        candidate.continuation_timing = Some(decide_permanent_furiten_reach_timing(
            comparison.and_then(|comparison| comparison.reach_now),
            comparison.and_then(|comparison| comparison.defer_forced_reach()),
        ));
    }
}

// 候補ごとの既存 base offense mode。現在聴牌の評価対象外は `None`。
fn current_tenpai_offense_modes(
    evaluations: &[CurrentTenpaiCandidateEvaluation],
) -> Vec<Option<TenpaiOffenseMode>> {
    evaluations
        .iter()
        .map(|evaluation| evaluation.offense.as_ref().map(|value| value.offense.mode))
        .collect()
}

fn current_tenpai_metrics(
    evaluations: &[CurrentTenpaiCandidateEvaluation],
) -> Vec<CurrentTenpaiMetrics> {
    evaluations
        .iter()
        .map(|evaluation| CurrentTenpaiMetrics {
            permanent_furiten: evaluation
                .wait
                .as_ref()
                .map(TenpaiWaitAvailability::permanent_furiten),
            offense_weighted_total: evaluation
                .offense
                .as_ref()
                .and_then(|value| value.offense.value.weighted_total()),
            expected_self_tsumo_value: evaluation.expected_self_tsumo_value,
            self_tsumo_hit_probability: evaluation.self_tsumo_hit_probability,
            // 既存 timing policy が選んだ側の値そのもの。比較不能を 0 点で補わない。
            continuation_self_tsumo_value: evaluation
                .continuation_timing
                .as_ref()
                .and_then(ReachTimingDiagnostic::self_tsumo_value),
        })
        .collect()
}

/// 選んだ打牌1件について、押し引きが観測する1向聴の前方集計値を返す。
///
/// 選んだ打牌が1向聴でなければ `None`。2向聴以上の前方集計値は押し引きの判断にも診断にも
/// 使わないため、ここでは持ち回らない。
///
/// `selected` は選択が使った前方集計値。1向聴の前方比較を行った候補集合では最善候補にも必ず
/// 値が入っているので、その場合は再計算せずそのまま返す。前方比較が不要だった場合 (合法候補が
/// 1件など) だけ、選んだ1候補について既存の前方評価基盤 ([`bot_logic::forward_metrics_for_candidate`])
/// から求める。押し引き側で lookahead も打点集計も持たず、全候補分の詳細診断も構築しない。
fn selected_iishanten_forward_metrics(
    context: &GameContext,
    tiles: &[TileId],
    evaluation: &DiscardEvaluation,
    selected: ForwardMetrics,
) -> Option<ForwardMetrics> {
    if evaluation.min_shanten_after_discard() != IISHANTEN_SHANTEN {
        return None;
    }
    if selected.tenpai_wait.is_some() {
        return Some(selected);
    }

    let valuator = ProductionProspectiveValuator::new(context);
    Some(forward_metrics_for_candidate(
        &lookahead_inputs(context, tiles, &valuator, LookaheadDiagnosticScope::None),
        evaluation,
    ))
}

/// 選択結果を保持していない経路のための、選んだ打牌1件分の1向聴前方集計値。
///
/// 選択が使った値を持ち回れる経路 (`select_discard_action_with_evaluation` →
/// [`DiscardActionSelection::iishanten_forward_metrics`]) はそちらを再利用し、この入口は使わない。
/// `GameContext` と打牌評価だけから押し引き入力を組み立てる経路
/// (`push_pull_inputs_from_context`) 専用で、選んだ1候補についてだけ求める。
///
/// `evaluation` は同じ `context` の手牌から求めた評価であること。
pub(crate) fn selected_iishanten_forward_metrics_from_context(
    context: &GameContext,
    evaluation: &DiscardEvaluation,
) -> Option<ForwardMetrics> {
    let tiles: Vec<_> = context
        .hand_tiles()
        .iter()
        .copied()
        .chain(context.drawn_tile())
        .collect();

    selected_iishanten_forward_metrics(context, &tiles, evaluation, ForwardMetrics::default())
}

// 最善向聴を維持する複数候補について、打牌選択用の前方集計値を求める。
//
// 対象の絞り込み (最善向聴数が1以上 かつ それを維持する候補が複数) は bot-logic 側の入口が
// 行うため、テンパイ・候補1件では前方探索が走らない。現在打牌後の受け入れは既存評価
// (evaluations) が持つ値をそのまま入力にするため、現在の1手評価を再計算しない。
// 物理牌・副露済み面子数・visible tiles・ドラ表示牌・場風・自風は本番評価と同じ値を渡す。
// GameContext 自体は渡さず、bot-logic が必要とする値と将来打点の評価器だけを渡す。
fn selection_forward_metrics(
    context: &GameContext,
    tiles: &[TileId],
    evaluations: &[DiscardEvaluation],
) -> SelectionForwardMetrics {
    let valuator = ProductionProspectiveValuator::new(context);
    forward_metrics(
        &lookahead_inputs(context, tiles, &valuator, LookaheadDiagnosticScope::None),
        evaluations,
    )
}

// 2手先評価の入力。本番選択・詳細診断・将来打点はこの1本を共有し、同じ枝を別々の入力で
// 評価しない。
//
// self-tsumo continuation の材料 (ツモ評価器と残り自摸機会) も同じ入口で渡す。残り自摸機会は
// 山の残枚数という exact fact からしか作らず、取得できない局面では渡さない。その場合 bot-logic
// 側で新しい軸を持たず、既存の比較へそのまま落ちる。
pub(crate) fn lookahead_inputs<'a>(
    context: &'a GameContext,
    tiles: &'a [TileId],
    valuator: &'a ProductionProspectiveValuator<'a>,
    scope: LookaheadDiagnosticScope,
) -> LookaheadInputs<'a> {
    let inputs = LookaheadInputs::new(
        tiles,
        evaluation_fixed_meld_count(context),
        context.dora_indicators(),
        context.round_wind(),
        context.seat_wind(),
    )
    .with_visible_tiles(context.visible_tiles())
    .with_prospective_valuator(valuator)
    .with_tsumo_valuator(valuator);
    let inputs = match own_future_draws(context) {
        Some(draws) => inputs.with_own_future_draws(draws),
        None => inputs,
    };
    if scope.builds_same_shanten_downstream() {
        inputs.with_same_shanten_downstream()
    } else {
        inputs
    }
}

/// 現在打牌後に自分へ残っている自摸機会。山の残枚数が unknown な局面では `None`。
///
/// 未来の鳴き・槓を予測しない4人麻雀のモデルでは、現在打牌後に残っている山の枚数を4人で
/// 順に分けるので `floor(remaining_tiles / 4)` になる。`remaining_tiles` は自分がツモった後の
/// 山の残枚数なので、ここで1枚引き直さない。
///
/// 巡目や河の枚数からの推測はしない。exact な fact が無い局面では新しい軸を使わない。
pub(crate) fn own_future_draws(context: &GameContext) -> Option<u32> {
    Some(context.remaining_tiles()? / 4)
}

// 絞り込み済みの合法候補集合から既存の診断を構築する。診断と tracing ログはこの結果を共有する。
// block context の副露補正が本番評価とずれないよう、診断にも同じ副露済み面子数を渡す。
// 前方集計値は選択で使ったものをそのまま渡し、診断のために再計算しない。
fn diagnose_legal_evaluations(
    context: &GameContext,
    legal: &LegalDiscardEvaluations,
    tenpai_wait: &[ForwardMetrics],
    current_tenpai: &[CurrentTenpaiCandidateEvaluation],
) -> DiscardDecisionDiagnostic {
    let counts = TileCounts::from_tiles(legal.tiles.iter().copied());
    diagnose_discard_evaluations_with_metrics(
        &counts,
        evaluation_fixed_meld_count(context),
        &legal.evaluations,
        tenpai_wait,
        &current_tenpai_metrics(current_tenpai),
    )
}

// 絞り込み済みの合法候補集合からフリテン診断を構築する。現在聴牌の比較で待ちを計算済みの
// 候補はその値を転記し、診断表示のために同じ wait evaluation をやり直さない。
//
// ツモ側は既存の打牌評価が持つ受け入れをそのまま使い、恒常フリテン判定に使う構造上のアガリ牌種と
// 「context の自分の河 + その打牌」は bot-logic の pure helper 側で組み立てる。副露済み面子数は
// 本番評価と同じ値を渡す。player_id が無く自分の河を特定できない場合は player 0 などを推測せず
// Unknown として扱う。診断専用の情報で、打牌選択には使わない。
//
// 履歴依存フリテンは選択済み1件の経路 (selected_discard_tenpai_wait_availability) と同じ
// 「打牌後」へ補正した値を渡し、候補ごとに評価時点がずれないようにする。
fn furiten_from_legal_evaluations(
    context: &GameContext,
    legal: &LegalDiscardEvaluations,
    current_tenpai: &[CurrentTenpaiCandidateEvaluation],
) -> Vec<DiscardFuritenDiagnostic> {
    let counts = TileCounts::from_tiles(legal.tiles.iter().copied());
    if current_tenpai
        .iter()
        .all(|evaluation| evaluation.wait.is_none())
    {
        return diagnose_discard_furiten(
            &counts,
            evaluation_fixed_meld_count(context),
            &legal.evaluations,
            &OwnDiscards::from_optional_river(context.own_discards()),
            context.history_furiten_after_own_discard(),
        );
    }

    let own_discards = OwnDiscards::from_optional_river(context.own_discards());
    let history_furiten = context.history_furiten_after_own_discard();
    let fixed_meld_count = evaluation_fixed_meld_count(context);
    legal
        .evaluations
        .iter()
        .enumerate()
        .map(|(index, evaluation)| DiscardFuritenDiagnostic {
            discard: evaluation.discard,
            tenpai: current_tenpai
                .get(index)
                .and_then(|evaluation| evaluation.wait.clone())
                .or_else(|| {
                    discard_tenpai_wait_availability(
                        &counts,
                        fixed_meld_count,
                        evaluation,
                        &own_discards,
                        history_furiten,
                    )
                }),
        })
        .collect()
}

/// 通常打牌選択が選んだ打牌1件について、その打牌後のテンパイの待ちとロン可否を返す。
///
/// 全合法候補分のフリテン診断 (`furiten_from_legal_evaluations`) と同じ pure helper へ同じ
/// 入力を渡し、対象を選択済みの1件だけに絞る。ツモ側は渡された打牌評価が持つ受け入れをそのまま
/// 使い、向聴・受け入れ・残枚数・待ちを再計算しない。その打牌でテンパイにならない場合は `None`。
///
/// 履歴依存フリテンも全候補診断と同じく「その打牌を切り終えた時点」へ補正した値を渡す
/// (`GameContext::history_furiten_after_own_discard`)。返り値の `can_ron()` は恒常フリテンと
/// 履歴依存フリテンを合わせた総合値になる。
///
/// `evaluation` は同じ `context` の手牌から求めた評価であること。リーチ判断のように選択済みの
/// 1候補だけが必要な経路が、全候補分の診断を構築せずに待ちとフリテンを共有するために使う。
pub(crate) fn selected_discard_tenpai_wait_availability(
    context: &GameContext,
    evaluation: &DiscardEvaluation,
) -> Option<TenpaiWaitAvailability> {
    let counts = TileCounts::from_tiles(
        context
            .hand_tiles()
            .iter()
            .copied()
            .chain(context.drawn_tile()),
    );
    discard_tenpai_wait_availability(
        &counts,
        evaluation_fixed_meld_count(context),
        evaluation,
        &OwnDiscards::from_optional_river(context.own_discards()),
        context.history_furiten_after_own_discard(),
    )
}

/// 打牌候補1件について、その打牌を1枚だけ除いた打牌後 concealed hand の物理牌一覧を返す。
///
/// 手牌とツモ牌を結合した物理牌一覧から、`evaluation.discard` の牌種かつ
/// `evaluation.discards_red_five` の赤フラグと一致する物理牌を1枚だけ除く。赤5と通常5では打点も
/// 押し引きの評価も変わるため、牌種だけでなく赤フラグも一致させる。一致する物理牌が無ければ、
/// 別の牌で代用せず `None`。
///
/// 打牌後の手牌を必要とする経路 (押し引きの打点 proxy・ダマ打点) はこの1本を共有し、同じ組み立てを
/// 複製しない。除去は1枚だけで、残りの牌の並びには意味を持たせない。
pub(crate) fn concealed_tiles_after_discard(
    context: &GameContext,
    evaluation: &DiscardEvaluation,
) -> Option<Vec<TileId>> {
    let tiles: Vec<TileId> = context
        .hand_tiles()
        .iter()
        .copied()
        .chain(context.drawn_tile())
        .collect();

    split_discarded_tile(tiles, evaluation).map(|(_, remaining)| remaining)
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
pub(crate) fn evaluation_fixed_meld_count(context: &GameContext) -> FixedMeldCount {
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

// 1手評価だけの既存比較順で最善評価を選ぶ。完全同値では先に現れた候補を維持する。
//
// 前方集計値を渡さないため、1向聴限定の weighted tenpai wait は適用しない。通常打牌選択が使う
// 比較は selection_from_legal_evaluations() /
// select_best_normal_discard_evaluation() 側にあり、こちらは意図的に1手比較だけを行う。
fn select_best_one_step_evaluation(
    evaluations: &[DiscardEvaluation],
) -> Option<&DiscardEvaluation> {
    best_discard_selection_index(evaluations, &[]).map(|index| &evaluations[index])
}

/// 合法 Dahai を受け取らない経路のための、通常打牌としての best 評価。
///
/// 比較 semantics は合法 Dahai 付きの通常打牌選択 (`select_discard_action_with_evaluation`) と
/// 同じで、1向聴限定の weighted tenpai wait と現在聴牌の offense weighted total を含む。
/// `legal_actions` は現在聴牌候補の既存 Reach / Damaten policy に渡す。違いは対象候補だけで、
/// こちらは合法 Dahai による絞り込みと物理牌補正を行わず、手牌から切れる全打牌候補を対象にする。
///
/// 押し引き入力の単独構築 (`push_pull_inputs_from_context`) のように、`GameContext` だけから
/// 「通常打牌なら何を切るか」を求める経路で使う。鳴き後シミュレーションのような1手評価には
/// [`select_best_one_step_discard_evaluation_with_fixed_meld_count`] を使い、こちらは使わない。
pub(crate) fn select_best_normal_discard_evaluation(
    context: &GameContext,
    tiles: &[TileId],
    legal_actions: &[LegalAction],
) -> Option<DiscardEvaluation> {
    let evaluations = evaluate_discard_candidates(context, tiles);
    let tenpai_wait = selection_forward_metrics(context, tiles, &evaluations);
    let current_tenpai =
        current_tenpai_candidate_evaluations(context, tiles, &evaluations, legal_actions);

    best_discard_selection_index_with_metrics(
        &evaluations,
        &tenpai_wait,
        &current_tenpai_metrics(&current_tenpai),
    )
    .map(|index| evaluations[index].clone())
}

/// 副露済み面子数と切れない牌種を明示した1手評価だけの best 評価。
///
/// 候補評価そのものは通常経路と同じ helper を共有するが、比較は既存の
/// [`bot_logic::compare_discard_evaluations`] 相当の1手比較だけで、1向聴限定の weighted tenpai
/// wait は**意図的に使わない**。鳴き判断の「鳴いた後に生きた待ちのテンパイになるか」という
/// シミュレーション用の入口であり、通常打牌 selection の semantics とは切り離す。
///
/// `forbidden_discards` は鳴いた直後に切れない牌種
/// ([`forbidden_discards_after_call`](crate::kuikae::forbidden_discards_after_call))。合法手の
/// 制約なので、比較する前に候補から取り除く。打牌候補の評価は牌種ごとに1件なので、除外も牌種
/// 単位になり、赤5と黒5の一方だけが残ることはない。実際に切る物理牌の赤黒 preference は残った
/// 候補の既存 semantics のままで変わらない。
pub(crate) fn select_best_one_step_discard_evaluation_with_fixed_meld_count(
    context: &GameContext,
    tiles: &[TileId],
    fixed_meld_count: FixedMeldCount,
    forbidden_discards: &[TileType],
) -> Option<DiscardEvaluation> {
    let mut evaluations =
        evaluate_discard_candidates_with_fixed_meld_count(context, tiles, fixed_meld_count);
    evaluations.retain(|evaluation| !forbidden_discards.contains(&evaluation.discard));
    select_best_one_step_evaluation(&evaluations).cloned()
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
    let selected_tenpai_wait = diagnostic
        .candidates
        .iter()
        .find(|candidate| candidate.selected)
        .and_then(|candidate| candidate.tenpai_wait);
    let selected_current_tenpai_offense_weighted_total = diagnostic
        .candidates
        .iter()
        .find(|candidate| candidate.selected)
        .and_then(|candidate| candidate.current_tenpai_offense_weighted_total);
    let selected_current_tenpai_expected_self_tsumo_value = diagnostic
        .candidates
        .iter()
        .find(|candidate| candidate.selected)
        .and_then(|candidate| candidate.current_tenpai_expected_self_tsumo_value);

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
        normal_weighted_tenpai_wait_remaining = ?selected_tenpai_wait
            .map(|metric| metric.weighted_remaining),
        normal_weighted_tenpai_wait_type_count = ?selected_tenpai_wait
            .map(|metric| metric.weighted_type_count),
        normal_current_tenpai_offense_weighted_total =
            ?selected_current_tenpai_offense_weighted_total,
        normal_current_tenpai_expected_self_tsumo_value =
            ?selected_current_tenpai_expected_self_tsumo_value,
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
        weighted_tenpai_wait_remaining = ?candidate
            .tenpai_wait
            .map(|metric| metric.weighted_remaining),
        weighted_tenpai_wait_type_count = ?candidate
            .tenpai_wait
            .map(|metric| metric.weighted_type_count),
        weighted_next_acceptance_remaining = ?candidate
            .next_acceptance
            .map(|metric| metric.weighted_remaining),
        weighted_next_acceptance_type_count = ?candidate
            .next_acceptance
            .map(|metric| metric.weighted_type_count),
        current_tenpai_offense_weighted_total =
            ?candidate.current_tenpai_offense_weighted_total,
        current_tenpai_expected_self_tsumo_value =
            ?candidate.current_tenpai_expected_self_tsumo_value,
        current_tenpai_continuation_self_tsumo_value =
            ?candidate.current_tenpai_continuation_self_tsumo_value,
        current_tenpai_self_tsumo_hit_probability =
            ?candidate.current_tenpai_self_tsumo_hit_probability,
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
pub(crate) mod tests {
    use super::*;
    use crate::context::TableStateFacts;
    use crate::push_pull::{PushPullOffenseState, push_pull_inputs_from_threat_facts};
    use crate::reach_policy::ReachTimingDecision;
    use crate::threat::player_threat_facts_from_context;
    use bot_logic::{
        HistoryFuritenFacts, PermanentFuriten, TileId,
        best_discard_selection_index_with_forward_metrics,
    };

    fn tile(value: u8) -> TileId {
        TileId::new(value).unwrap()
    }

    fn dahai(value: u8) -> LegalAction {
        LegalAction::Dahai { tile: tile(value) }
    }

    #[test]
    fn the_selection_does_not_own_the_completed_hands() {
        // 通常打牌 selection は production の act() でも構築される。完成手 (TenpaiCompletedHands)
        // は待ちごとの解析を丸ごと所有する重い値なので、診断でしか使わない値を持たせない。
        // 診断用の field が増えるとこの構築が壊れる。
        let selection = DiscardActionSelection {
            evaluation: None,
            action: None,
            iishanten_forward_metrics: None,
            tenpai_wait: None,
            tenpai_offense_value: None,
            damaten_value: None,
            tenpai_reach_timing: None,
        };

        assert_eq!(selection.action, None);
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

    fn current_tenpai_regression_context() -> (GameContext, Vec<LegalAction>) {
        current_tenpai_regression_context_with_facts(Default::default(), None, Some(0))
    }

    fn current_tenpai_regression_context_with_facts(
        discards: [Vec<TileId>; 4],
        remaining_tiles: Option<u32>,
        player_id: Option<u8>,
    ) -> (GameContext, Vec<LegalAction>) {
        // 34599m235p345567s。CLI regression と同じ14枚に、自席・河・履歴フリテンを既知として
        // 与え、既存 Reach / Damaten policy と scoring を確定できる局面にする。
        let hand = vec![
            tile(8),
            tile(12),
            tile(17),
            tile(32),
            tile(33),
            tile(40),
            tile(44),
            tile(53),
            tile(80),
            tile(84),
            tile(89),
            tile(90),
            tile(92),
            tile(96),
        ];
        let visible = hand
            .iter()
            .copied()
            .chain(discards.iter().flatten().copied())
            .collect();
        let actions = hand
            .iter()
            .copied()
            .map(|tile| LegalAction::Dahai { tile })
            .chain([LegalAction::Reach])
            .collect();
        let context = GameContext::from_parts_with_table_state(
            None,
            hand,
            vec![],
            TileType::from_mjai_type_str("E").ok(),
            TileType::from_mjai_type_str("N").ok(),
            visible,
            player_id,
            Some(1),
            discards,
            [false; 4],
        )
        .with_table_state_facts(TableStateFacts {
            remaining_tiles,
            ..Default::default()
        })
        .with_history_furiten_facts(HistoryFuritenFacts {
            same_turn: Some(false),
            riichi_missed_win: Some(false),
        });
        (context, actions)
    }

    #[test]
    fn current_tenpai_selection_uses_the_existing_weighted_offense_value() {
        let (context, actions) = current_tenpai_regression_context();
        let legal = legal_discard_evaluations(&context, &actions);
        let current = current_tenpai_candidate_evaluations(
            &context,
            &legal.tiles,
            &legal.evaluations,
            &actions,
        );
        let candidate = |name: &str| {
            let index = legal
                .evaluations
                .iter()
                .position(|evaluation| evaluation.discard.to_mjai_string() == name)
                .expect("candidate exists");
            (&legal.evaluations[index], &current[index])
        };

        let (two_p, two_p_value) = candidate("2p");
        let two_p_wait = two_p_value.wait.as_ref().expect("2p wait");
        let two_p_offense = two_p_value.offense.as_ref().expect("2p offense").offense;
        assert_eq!(two_p.acceptance_total_remaining(), 4);
        assert_eq!(
            two_p_wait.live_waits,
            vec![TileType::from_mjai_type_str("4p").unwrap()]
        );
        assert_eq!(
            two_p_offense.mode,
            crate::offense_value::TenpaiOffenseMode::Reach
        );
        assert_eq!(two_p_offense.value.weighted_total(), Some(20_800));
        assert_eq!(two_p_offense.value.average_total(), Some(5_200));

        let (five_p, five_p_value) = candidate("5p");
        let five_p_wait = five_p_value.wait.as_ref().expect("5p wait");
        let five_p_offense = five_p_value.offense.as_ref().expect("5p offense").offense;
        assert_eq!(five_p.acceptance_total_remaining(), 8);
        assert_eq!(
            five_p_wait.live_waits,
            vec![
                TileType::from_mjai_type_str("1p").unwrap(),
                TileType::from_mjai_type_str("4p").unwrap(),
            ]
        );
        assert_eq!(
            five_p_offense.mode,
            crate::offense_value::TenpaiOffenseMode::Reach
        );
        assert_eq!(five_p_offense.value.weighted_total(), Some(16_000));
        assert_eq!(five_p_offense.value.average_total(), Some(2_000));

        let result = select_discard_action_with_diagnostic(
            &context,
            &actions,
            LookaheadDiagnosticScope::None,
        );
        assert_eq!(
            result
                .selection
                .evaluation
                .as_ref()
                .map(|value| value.discard),
            Some(TileType::from_mjai_type_str("2p").unwrap())
        );
        assert_eq!(result.selection.tenpai_wait, Some(two_p_wait.clone()));
        assert_eq!(result.selection.tenpai_offense_value, Some(two_p_offense));
        let five_p_diagnostic = result
            .diagnostic
            .candidates
            .iter()
            .find(|candidate| candidate.evaluation.discard.to_mjai_string() == "5p")
            .expect("5p diagnostic");
        assert_eq!(
            five_p_diagnostic.comparison_reason,
            bot_logic::DiscardComparisonReason::CurrentTenpaiOffenseWeightedTotal
        );
        // 全候補が非フリテンの cohort では継続評価も self-tsumo 軸も使わない。
        assert!(
            current
                .iter()
                .all(|value| value.continuation_timing.is_none())
        );
        assert!(result.diagnostic.candidates.iter().all(|candidate| {
            candidate.current_tenpai_expected_self_tsumo_value.is_none()
                && candidate
                    .current_tenpai_continuation_self_tsumo_value
                    .is_none()
        }));
    }

    // 全候補が恒常フリテンで、全候補の base offense mode がリーチになる局面。
    fn all_permanent_furiten_context() -> (GameContext, Vec<LegalAction>) {
        current_tenpai_regression_context_with_facts(
            [vec![tile(36), tile(48)], vec![], vec![], vec![]],
            Some(70),
            Some(0),
        )
    }

    #[test]
    fn permanent_furiten_current_tenpai_candidates_use_the_continuation_self_tsumo_value() {
        let (context, actions) = all_permanent_furiten_context();
        let legal = legal_discard_evaluations(&context, &actions);
        let current = current_tenpai_candidate_evaluations(
            &context,
            &legal.tiles,
            &legal.evaluations,
            &actions,
        );
        let targets: Vec<_> = legal
            .evaluations
            .iter()
            .zip(&current)
            .filter(|(evaluation, _)| evaluation.min_shanten_after_discard() == TENPAI_SHANTEN)
            .collect();
        assert_eq!(targets.len(), 2);
        assert!(targets.iter().all(|(_, value)| {
            value
                .wait
                .as_ref()
                .map(TenpaiWaitAvailability::permanent_furiten)
                == Some(PermanentFuriten::Yes)
                && value.offense.as_ref().map(|offense| offense.offense.mode)
                    == Some(TenpaiOffenseMode::Reach)
                && value.expected_self_tsumo_value.is_some()
        }));
        // 候補値は既存 timing policy が選んだ側の self-tsumo value そのもの。
        assert!(targets.iter().all(|(_, value)| {
            let timing = value
                .continuation_timing
                .expect("継続 timing を評価している");
            timing.self_tsumo_value()
                == match timing.decision {
                    ReachTimingDecision::ReachNow => timing.reach_now,
                    ReachTimingDecision::DeferReach => timing.defer_forced_reach,
                }
        }));

        let result = select_discard_action_with_diagnostic(
            &context,
            &actions,
            LookaheadDiagnosticScope::None,
        );
        let candidates: Vec<_> = result
            .diagnostic
            .candidates
            .iter()
            .filter(|candidate| candidate.evaluation.min_shanten_after_discard() == TENPAI_SHANTEN)
            .collect();
        assert_eq!(candidates.len(), 2);
        // 新しい軸を使う cohort では、現在の待ちのままの self-tsumo 軸は使わない。
        assert!(candidates.iter().all(|candidate| {
            candidate.current_tenpai_offense_weighted_total.is_none()
                && candidate.current_tenpai_expected_self_tsumo_value.is_none()
                && candidate
                    .current_tenpai_continuation_self_tsumo_value
                    .is_some()
        }));
        let winner = candidates
            .iter()
            .find(|candidate| candidate.selected)
            .expect("selected current tenpai candidate");
        let loser = candidates
            .iter()
            .find(|candidate| !candidate.selected)
            .expect("runner-up current tenpai candidate");
        assert_eq!(winner.evaluation.discard.to_mjai_string(), "2p");
        assert!(
            winner.current_tenpai_continuation_self_tsumo_value
                > loser.current_tenpai_continuation_self_tsumo_value
        );
        assert_eq!(
            loser.comparison_reason,
            bot_logic::DiscardComparisonReason::CurrentTenpaiContinuationSelfTsumoValue
        );
    }

    #[test]
    fn base_damaten_current_tenpai_candidates_keep_the_expected_self_tsumo_axis() {
        // 合法手にリーチが無ければ base offense mode はダマ。リーチ timing の比較対象ではない
        // ので継続評価そのものを構築せず、現在の待ちのままの self-tsumo 軸を維持する。
        let (context, actions) = all_permanent_furiten_context();
        let actions: Vec<_> = actions
            .into_iter()
            .filter(|action| !matches!(action, LegalAction::Reach))
            .collect();
        let legal = legal_discard_evaluations(&context, &actions);
        let current = current_tenpai_candidate_evaluations(
            &context,
            &legal.tiles,
            &legal.evaluations,
            &actions,
        );
        assert!(current.iter().all(|value| {
            value.continuation_timing.is_none()
                && value.offense.as_ref().map(|offense| offense.offense.mode)
                    != Some(TenpaiOffenseMode::Reach)
        }));

        let result = select_discard_action_with_diagnostic(
            &context,
            &actions,
            LookaheadDiagnosticScope::None,
        );
        let candidates: Vec<_> = result
            .diagnostic
            .candidates
            .iter()
            .filter(|candidate| candidate.evaluation.min_shanten_after_discard() == TENPAI_SHANTEN)
            .collect();
        assert_eq!(candidates.len(), 2);
        assert!(candidates.iter().all(|candidate| {
            candidate
                .current_tenpai_continuation_self_tsumo_value
                .is_none()
                && candidate.current_tenpai_expected_self_tsumo_value.is_some()
        }));
        let loser = candidates
            .iter()
            .find(|candidate| !candidate.selected)
            .expect("runner-up current tenpai candidate");
        assert_eq!(
            loser.comparison_reason,
            bot_logic::DiscardComparisonReason::CurrentTenpaiExpectedSelfTsumoValue
        );
    }

    #[test]
    fn the_selected_candidate_reach_timing_reuses_the_comparator_continuation() {
        // 候補比較が評価した継続 timing は選択結果へそのまま乗り、後段の Reach timing は同じ
        // 候補の2手先評価をやり直さずその値を使う。
        let (context, actions) = all_permanent_furiten_context();
        let legal = legal_discard_evaluations(&context, &actions);
        let current = current_tenpai_candidate_evaluations(
            &context,
            &legal.tiles,
            &legal.evaluations,
            &actions,
        );
        let selected = best_discard_selection_index_with_metrics(
            &legal.evaluations,
            &selection_forward_metrics(&context, &legal.tiles, &legal.evaluations),
            &current_tenpai_metrics(&current),
        )
        .expect("selected candidate");
        let comparator_timing = current[selected]
            .continuation_timing
            .expect("選択済み候補の継続 timing");

        let selection = select_discard_action_with_evaluation(&context, &actions);
        assert_eq!(selection.tenpai_reach_timing, Some(comparator_timing));

        let diagnostic = crate::agents::ShantenAgent::diagnose_with_options(
            &context,
            &actions,
            crate::agents::DiagnosticOptions::NONE,
        );
        let reach = diagnostic.reach.expect("リーチ判断がある");

        assert_eq!(reach.timing, Some(comparator_timing));
        assert_eq!(
            comparator_timing.reason,
            crate::reach_policy::ReachTimingReason::PermanentFuritenSelfTsumo
        );
    }

    #[test]
    fn a_single_current_tenpai_candidate_falls_back_to_the_selected_candidate_timing() {
        // 現在聴牌候補が1件だけなら候補比較そのものが不要なので、継続評価も走らない。後段の
        // Reach timing は従来どおり選択済み1候補について評価する。
        let (context, actions) = all_permanent_furiten_context();
        let selected_discard = select_discard_action(&context, &actions).expect("打牌を選んでいる");
        let actions: Vec<_> = actions
            .into_iter()
            .filter(|action| matches!(action, LegalAction::Reach) || action == &selected_discard)
            .collect();

        let selection = select_discard_action_with_evaluation(&context, &actions);
        assert_eq!(selection.action, Some(selected_discard));
        assert_eq!(selection.tenpai_reach_timing, None);

        let diagnostic = crate::agents::ShantenAgent::diagnose_with_options(
            &context,
            &actions,
            crate::agents::DiagnosticOptions::NONE,
        );
        let timing = diagnostic
            .reach
            .expect("リーチ判断がある")
            .timing
            .expect("base policy がリーチを選んでいる");

        assert_eq!(
            timing.reason,
            crate::reach_policy::ReachTimingReason::PermanentFuritenSelfTsumo
        );
        assert!(timing.reach_now.is_some(), "{timing:?}");
        assert!(timing.defer_forced_reach.is_some(), "{timing:?}");
    }

    #[test]
    fn mixed_current_tenpai_candidates_compare_only_expected_self_tsumo_value() {
        // 打5pの1p/4p待ちだけが1pの恒常フリテン。打2pの4p待ちは非フリテンのまま。
        let (context, actions) = current_tenpai_regression_context_with_facts(
            [vec![tile(36)], vec![], vec![], vec![]],
            Some(70),
            Some(0),
        );
        let legal = legal_discard_evaluations(&context, &actions);
        let current = current_tenpai_candidate_evaluations(
            &context,
            &legal.tiles,
            &legal.evaluations,
            &actions,
        );
        let value = |discard: &str| {
            let index = legal
                .evaluations
                .iter()
                .position(|evaluation| evaluation.discard.to_mjai_string() == discard)
                .expect("candidate exists");
            &current[index]
        };
        assert_eq!(
            value("2p")
                .wait
                .as_ref()
                .map(TenpaiWaitAvailability::permanent_furiten),
            Some(PermanentFuriten::No)
        );
        assert_eq!(
            value("5p")
                .wait
                .as_ref()
                .map(TenpaiWaitAvailability::permanent_furiten),
            Some(PermanentFuriten::Yes)
        );

        let result = select_discard_action_with_diagnostic(
            &context,
            &actions,
            LookaheadDiagnosticScope::None,
        );
        let candidates: Vec<_> = result
            .diagnostic
            .candidates
            .iter()
            .filter(|candidate| candidate.evaluation.min_shanten_after_discard() == TENPAI_SHANTEN)
            .collect();
        // 恒常フリテンと非フリテンが混ざる cohort では継続評価そのものを構築しない。
        assert!(
            current
                .iter()
                .all(|value| value.continuation_timing.is_none())
        );
        assert!(candidates.iter().all(|candidate| {
            candidate.current_tenpai_offense_weighted_total.is_none()
                && candidate.current_tenpai_expected_self_tsumo_value.is_some()
                && candidate
                    .current_tenpai_continuation_self_tsumo_value
                    .is_none()
        }));
        let winner = candidates
            .iter()
            .find(|candidate| candidate.selected)
            .expect("selected current tenpai candidate");
        let loser = candidates
            .iter()
            .find(|candidate| !candidate.selected)
            .expect("runner-up current tenpai candidate");
        assert_eq!(winner.evaluation.discard.to_mjai_string(), "2p");
        assert!(
            winner.current_tenpai_expected_self_tsumo_value
                > loser.current_tenpai_expected_self_tsumo_value
        );
        assert_eq!(
            loser.comparison_reason,
            bot_logic::DiscardComparisonReason::CurrentTenpaiExpectedSelfTsumoValue
        );
    }

    #[test]
    fn unknown_current_tenpai_self_tsumo_value_disables_the_axis_for_the_cohort() {
        let (context, actions) = current_tenpai_regression_context_with_facts(
            [vec![tile(36)], vec![], vec![], vec![]],
            None,
            Some(0),
        );
        // 残り自摸機会が確定しないと継続比較のどの値も確定しないので、継続評価そのものを
        // 走らせない。
        let legal = legal_discard_evaluations(&context, &actions);
        let current = current_tenpai_candidate_evaluations(
            &context,
            &legal.tiles,
            &legal.evaluations,
            &actions,
        );
        assert!(
            current
                .iter()
                .all(|value| value.continuation_timing.is_none())
        );

        let result = select_discard_action_with_diagnostic(
            &context,
            &actions,
            LookaheadDiagnosticScope::None,
        );
        let candidates: Vec<_> = result
            .diagnostic
            .candidates
            .iter()
            .filter(|candidate| candidate.evaluation.min_shanten_after_discard() == TENPAI_SHANTEN)
            .collect();
        assert!(candidates.iter().all(|candidate| {
            candidate.current_tenpai_offense_weighted_total.is_none()
                && candidate.current_tenpai_expected_self_tsumo_value.is_none()
                && candidate
                    .current_tenpai_continuation_self_tsumo_value
                    .is_none()
                && candidate.comparison_reason
                    != bot_logic::DiscardComparisonReason::CurrentTenpaiExpectedSelfTsumoValue
        }));
    }

    #[test]
    fn unknown_permanent_furiten_does_not_enable_the_current_tenpai_self_tsumo_axis() {
        let (context, actions) =
            current_tenpai_regression_context_with_facts(Default::default(), Some(70), None);
        let legal = legal_discard_evaluations(&context, &actions);
        let current = current_tenpai_candidate_evaluations(
            &context,
            &legal.tiles,
            &legal.evaluations,
            &actions,
        );
        assert!(
            current
                .iter()
                .filter_map(|candidate| candidate.wait.as_ref())
                .all(|wait| wait.permanent_furiten() == PermanentFuriten::Unknown)
        );

        let result = select_discard_action_with_diagnostic(
            &context,
            &actions,
            LookaheadDiagnosticScope::None,
        );
        assert!(result.diagnostic.candidates.iter().all(|candidate| {
            candidate.current_tenpai_expected_self_tsumo_value.is_none()
                && candidate.comparison_reason
                    != bot_logic::DiscardComparisonReason::CurrentTenpaiExpectedSelfTsumoValue
        }));
    }

    #[test]
    fn current_tenpai_hit_probability_shares_the_expected_value_terminal() {
        let (context, actions) = current_tenpai_regression_context_with_facts(
            [vec![tile(36)], vec![], vec![], vec![]],
            Some(70),
            Some(0),
        );
        let legal = legal_discard_evaluations(&context, &actions);
        let current = current_tenpai_candidate_evaluations(
            &context,
            &legal.tiles,
            &legal.evaluations,
            &actions,
        );
        let index = legal
            .evaluations
            .iter()
            .position(|evaluation| evaluation.discard.to_mjai_string() == "2p")
            .expect("2p candidate");
        let evaluation = &legal.evaluations[index];
        let candidate = &current[index];
        let wait = candidate.wait.as_ref().expect("tenpai wait");
        let mode = candidate.offense.as_ref().expect("offense").offense.mode;
        let facts = lookahead_inputs(
            &context,
            &legal.tiles,
            &ProductionProspectiveValuator::new(&context),
            LookaheadDiagnosticScope::None,
        )
        .self_tsumo_facts()
        .expect("self-tsumo facts");
        let hands = tenpai_completed_hands_after_discard(&context, evaluation, wait)
            .expect("completed hands");
        let terminal =
            tenpai_tsumo_value_from_hands(&context, &hands, mode).expect("tsumo scoring");

        assert_eq!(
            candidate.expected_self_tsumo_value,
            Some(terminal.expected_payment(facts.unknown_tiles, facts.own_future_draws))
        );
        assert_eq!(
            candidate.self_tsumo_hit_probability,
            Some(tsumo_hit_probability(
                facts.unknown_tiles,
                terminal.winning_remaining,
                facts.own_future_draws,
            ))
        );

        let result = select_discard_action_with_diagnostic(
            &context,
            &actions,
            LookaheadDiagnosticScope::None,
        );
        let diagnostic = result
            .diagnostic
            .candidates
            .iter()
            .find(|diagnostic| diagnostic.evaluation.discard.to_mjai_string() == "2p")
            .expect("2p diagnostic");
        assert_eq!(
            diagnostic.current_tenpai_self_tsumo_hit_probability,
            candidate.self_tsumo_hit_probability
        );
    }

    #[test]
    fn the_ron_axis_cohort_still_observes_the_current_tenpai_hit_probability() {
        // 非フリテン cohort は既存 Ron 軸のままで、期待支払いは軸解決で落ちる。診断専用の
        // 確率だけは落とさず、打点軸で選んだ候補の和了確率も観測できる。
        let (context, actions) =
            current_tenpai_regression_context_with_facts(Default::default(), Some(70), Some(0));
        let result = select_discard_action_with_diagnostic(
            &context,
            &actions,
            LookaheadDiagnosticScope::None,
        );
        let candidates: Vec<_> = result
            .diagnostic
            .candidates
            .iter()
            .filter(|candidate| candidate.evaluation.min_shanten_after_discard() == TENPAI_SHANTEN)
            .collect();
        assert_eq!(candidates.len(), 2);
        assert!(candidates.iter().all(|candidate| {
            candidate.current_tenpai_offense_weighted_total.is_some()
                && candidate.current_tenpai_expected_self_tsumo_value.is_none()
                && candidate
                    .current_tenpai_self_tsumo_hit_probability
                    .is_some()
        }));
        assert!(candidates.iter().any(|candidate| {
            candidate.comparison_reason
                == bot_logic::DiscardComparisonReason::CurrentTenpaiOffenseWeightedTotal
        }));
    }

    #[test]
    fn unknown_self_tsumo_facts_leave_the_current_tenpai_hit_probability_unknown() {
        let (context, actions) = current_tenpai_regression_context_with_facts(
            [vec![tile(36)], vec![], vec![], vec![]],
            None,
            Some(0),
        );
        let legal = legal_discard_evaluations(&context, &actions);
        let current = current_tenpai_candidate_evaluations(
            &context,
            &legal.tiles,
            &legal.evaluations,
            &actions,
        );
        assert!(
            current
                .iter()
                .all(|candidate| candidate.self_tsumo_hit_probability.is_none())
        );

        let result = select_discard_action_with_diagnostic(
            &context,
            &actions,
            LookaheadDiagnosticScope::None,
        );
        assert!(result.diagnostic.candidates.iter().all(|candidate| {
            candidate
                .current_tenpai_self_tsumo_hit_probability
                .is_none()
        }));
    }

    #[test]
    fn current_tenpai_self_tsumo_uses_the_production_base_mode() {
        let (context, actions) = current_tenpai_regression_context_with_facts(
            [vec![tile(36)], vec![], vec![], vec![]],
            Some(70),
            Some(0),
        );
        let legal = legal_discard_evaluations(&context, &actions);
        let current = current_tenpai_candidate_evaluations(
            &context,
            &legal.tiles,
            &legal.evaluations,
            &actions,
        );
        let index = legal
            .evaluations
            .iter()
            .position(|evaluation| evaluation.discard.to_mjai_string() == "2p")
            .expect("2p candidate");
        let evaluation = &legal.evaluations[index];
        let candidate = &current[index];
        let wait = candidate.wait.as_ref().expect("tenpai wait");
        let mode = candidate.offense.as_ref().expect("offense").offense.mode;
        assert_eq!(mode, crate::offense_value::TenpaiOffenseMode::Reach);
        let facts = lookahead_inputs(
            &context,
            &legal.tiles,
            &ProductionProspectiveValuator::new(&context),
            LookaheadDiagnosticScope::None,
        )
        .self_tsumo_facts()
        .expect("self-tsumo facts");
        let hands = tenpai_completed_hands_after_discard(&context, evaluation, wait)
            .expect("completed hands");
        let expected = |mode| {
            tenpai_tsumo_value_from_hands(&context, &hands, mode)
                .map(|value| value.expected_payment(facts.unknown_tiles, facts.own_future_draws))
        };

        assert_eq!(candidate.expected_self_tsumo_value, expected(mode));
        assert_ne!(
            candidate.expected_self_tsumo_value,
            expected(crate::offense_value::TenpaiOffenseMode::Damaten)
        );

        // Reach が合法でない同じ局面は production base policy が Damaten を選ぶ。候補比較は
        // Reach timing を呼ばず、この base mode を Tsumo baseline へそのまま反映する。
        let damaten_actions: Vec<_> = actions
            .iter()
            .filter(|action| !matches!(action, LegalAction::Reach))
            .cloned()
            .collect();
        let damaten_legal = legal_discard_evaluations(&context, &damaten_actions);
        let damaten_current = current_tenpai_candidate_evaluations(
            &context,
            &damaten_legal.tiles,
            &damaten_legal.evaluations,
            &damaten_actions,
        );
        let damaten_index = damaten_legal
            .evaluations
            .iter()
            .position(|evaluation| evaluation.discard.to_mjai_string() == "2p")
            .expect("2p candidate");
        let damaten_candidate = &damaten_current[damaten_index];
        assert_eq!(
            damaten_candidate
                .offense
                .as_ref()
                .expect("offense")
                .offense
                .mode,
            crate::offense_value::TenpaiOffenseMode::Damaten
        );
        assert_eq!(
            damaten_candidate.expected_self_tsumo_value,
            expected(crate::offense_value::TenpaiOffenseMode::Damaten)
        );
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

    // 合法候補集合と前方集計値から診断と選択を作り、診断の selected と選択結果が一致することを
    // 確認する。本番経路と同じ helper だけを通す。
    fn assert_diagnostic_selection_matches(context: &GameContext, actions: &[LegalAction]) {
        let legal = legal_discard_evaluations(context, actions);
        let tenpai_wait = selection_forward_metrics(context, &legal.tiles, &legal.evaluations);
        let current_tenpai = current_tenpai_candidate_evaluations(
            context,
            &legal.tiles,
            &legal.evaluations,
            actions,
        );

        let diagnostic = diagnose_legal_evaluations(context, &legal, &tenpai_wait, &current_tenpai);
        let selection = selection_from_legal_evaluations(
            context,
            &legal,
            &tenpai_wait,
            &current_tenpai,
            actions,
        );

        assert_eq!(diagnostic.selected, selection.evaluation);
        assert!(diagnostic.selected.is_some());
    }

    #[test]
    fn diagnostic_selection_matches_best_on_legal_candidates() {
        // 診断の selected と通常経路の選択結果が、同じ合法候補一覧に対して一致することを
        // 確認する。グローバル subscriber に依存しない。
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

        assert_diagnostic_selection_matches(&context, &actions);
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

        assert_diagnostic_selection_matches(&context, &actions);
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
        let expected = select_best_normal_discard_evaluation(&context, &tiles, &actions);

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
        let global_best = select_best_one_step_evaluation(&all).unwrap().discard;

        // 全体最善(W=116)を除外し、他の牌種だけを合法にする。
        let actions: Vec<LegalAction> = hand_values.iter().map(|&value| dahai(value)).collect();
        assert!(legal_dahai_tile_for_type(global_best, &actions).is_none());

        let expected_best = select_best_one_step_evaluation(&retain_legal_dahai_evaluations(
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
        let global_best =
            select_best_one_step_evaluation(&evaluate_discard_candidates(&context, &tiles))
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

    // ---- 1向聴の weighted tenpai wait ----

    // 12m 68m 444p 5p 789p 567s の門前14枚。
    //
    // 打 5p は 12m の辺張と 68m の嵌張を残して受け入れが最も広く、打 1m は 45p の両面を残して
    // テンパイ後の待ちが広くなる。1手評価だけなら受け入れの多い 5p が選ばれる。
    pub(crate) const IISHANTEN_WAIT_TILES: [u8; 14] =
        [0, 4, 20, 28, 48, 49, 50, 53, 60, 64, 68, 89, 92, 96];

    pub(crate) fn iishanten_wait_context() -> GameContext {
        let tiles: Vec<_> = IISHANTEN_WAIT_TILES
            .iter()
            .map(|&value| tile(value))
            .collect();
        let (hand, drawn) = tiles.split_at(IISHANTEN_WAIT_TILES.len() - 1);
        GameContext::from_parts_with_visible_tiles(
            Some(drawn[0]),
            hand.to_vec(),
            vec![],
            None,
            None,
            tiles.clone(),
        )
    }

    // 手牌とツモ牌の物理牌一覧。合法 Dahai を受け取らない入口の検証で使う。
    pub(crate) fn iishanten_wait_tiles() -> Vec<TileId> {
        IISHANTEN_WAIT_TILES
            .iter()
            .map(|&value| tile(value))
            .collect()
    }

    // 1手評価だけで選ぶ best。通常打牌 selection との違いを固定するための検証用 helper で、
    // 副露済み面子数は本番評価と同じ値を使う。
    pub(crate) fn one_step_best_evaluation(
        context: &GameContext,
        tiles: &[TileId],
    ) -> Option<DiscardEvaluation> {
        select_best_one_step_discard_evaluation_with_fixed_meld_count(
            context,
            tiles,
            evaluation_fixed_meld_count(context),
            &[],
        )
    }

    // 検証対象の2候補だけを合法にする。1向聴を維持する候補が複数あるので前方評価は走る。
    fn iishanten_wait_actions() -> Vec<LegalAction> {
        vec![dahai(0), dahai(53)]
    }

    #[test]
    fn standalone_normal_discard_evaluation_uses_the_weighted_tenpai_wait() {
        // 合法 Dahai を制限せず、手牌から切れる全打牌候補を対象にした1向聴局面。
        let context = iishanten_wait_context();
        let tiles = iishanten_wait_tiles();

        let one_step = one_step_best_evaluation(&context, &tiles).expect("1手評価の best");
        let normal =
            select_best_normal_discard_evaluation(&context, &tiles, &[]).expect("通常打牌の best");

        assert_eq!(one_step.min_shanten_after_discard(), 1);
        assert_eq!(normal.min_shanten_after_discard(), 1);
        // 1手比較だけなら受け入れの多い候補、weighted wait 込みなら別候補が勝つ局面である。
        assert_ne!(normal.discard, one_step.discard);
        assert!(one_step.acceptance_total_remaining() > normal.acceptance_total_remaining());
    }

    #[test]
    fn standalone_normal_discard_evaluation_matches_the_legal_selection() {
        // 全打牌候補が合法な局面では、合法 Dahai 付きの通常打牌選択と同じ評価になる。
        let context = iishanten_wait_context();
        let actions: Vec<LegalAction> = IISHANTEN_WAIT_TILES
            .iter()
            .map(|&value| dahai(value))
            .collect();

        assert_eq!(
            select_discard_action_with_evaluation(&context, &actions).evaluation,
            select_best_normal_discard_evaluation(&context, &iishanten_wait_tiles(), &actions,),
        );
    }

    #[test]
    fn weighted_tenpai_wait_outranks_the_current_acceptance() {
        let context = iishanten_wait_context();
        let actions = iishanten_wait_actions();

        let legal = legal_discard_evaluations(&context, &actions);
        // 1手評価だけなら受け入れの多い候補が選ばれる局面である。
        let one_step = select_best_one_step_evaluation(&legal.evaluations)
            .unwrap()
            .clone();

        let selection = select_discard_action_with_evaluation(&context, &actions);
        let selected = selection.evaluation.as_ref().unwrap();

        assert_ne!(selected.discard, one_step.discard);
        assert!(one_step.acceptance_total_remaining() > selected.acceptance_total_remaining());
        assert_eq!(selection.action, Some(dahai(0)));
        assert_evaluation_action_types_match(&selection);
    }

    #[test]
    fn diagnostic_reports_the_weighted_tenpai_wait_of_each_candidate() {
        let context = iishanten_wait_context();
        let actions = iishanten_wait_actions();

        let with_diagnostic = select_discard_action_with_diagnostic(
            &context,
            &actions,
            LookaheadDiagnosticScope::None,
        );
        let candidates = &with_diagnostic.diagnostic.candidates;
        let selected = candidates
            .iter()
            .find(|candidate| candidate.selected)
            .unwrap();
        let runner_up = candidates
            .iter()
            .find(|candidate| !candidate.selected)
            .unwrap();

        assert_eq!(
            runner_up.comparison_reason,
            bot_logic::DiscardComparisonReason::WeightedTenpaiWaitRemaining
        );
        assert!(
            selected.tenpai_wait.unwrap().weighted_remaining
                > runner_up.tenpai_wait.unwrap().weighted_remaining
        );
    }

    #[test]
    fn lookahead_diagnostic_shares_the_weighted_tenpai_wait() {
        // 詳細2手先診断の有無で選択も診断も変わらない。同じ枝を2回計算しない経路を固定する。
        let context = iishanten_wait_context();
        let actions = iishanten_wait_actions();

        let without = select_discard_action_with_diagnostic(
            &context,
            &actions,
            LookaheadDiagnosticScope::None,
        );
        let with = select_discard_action_with_diagnostic(
            &context,
            &actions,
            LookaheadDiagnosticScope::Lookahead,
        );

        assert_eq!(without.selection, with.selection);
        assert_eq!(without.diagnostic, with.diagnostic);
        assert!(without.lookahead.is_none());
        assert!(with.lookahead.is_some());
        assert!(without.lookahead_value.is_none());
        assert!(with.lookahead_value.is_some());
    }

    #[test]
    fn prospective_value_does_not_change_the_discard_selection() {
        // 将来打点は解析専用の追加情報で、本番選択も候補比較も変えない。
        let context = iishanten_wait_context();
        let actions = iishanten_wait_actions();

        let production = select_discard_action_with_evaluation(&context, &actions);
        let with_value = select_discard_action_with_diagnostic(
            &context,
            &actions,
            LookaheadDiagnosticScope::Lookahead,
        );
        let without_value = select_discard_action_with_diagnostic(
            &context,
            &actions,
            LookaheadDiagnosticScope::None,
        );

        assert_eq!(with_value.selection, production);
        for (with, without) in with_value
            .diagnostic
            .candidates
            .iter()
            .zip(without_value.diagnostic.candidates.iter())
        {
            assert_eq!(with.evaluation.discard, without.evaluation.discard);
            assert_eq!(with.selected, without.selected);
            assert_eq!(with.comparison_reason, without.comparison_reason);
            assert_eq!(with.tenpai_wait, without.tenpai_wait);
        }

        // 1向聴の比較軸そのものが変わっていないことも固定する。
        let runner_up = with_value
            .diagnostic
            .candidates
            .iter()
            .find(|candidate| !candidate.selected)
            .expect("runner-up がある");
        assert_eq!(
            runner_up.comparison_reason,
            bot_logic::DiscardComparisonReason::WeightedTenpaiWaitRemaining
        );
    }

    #[test]
    fn prospective_value_keeps_the_next_discard_of_each_branch() {
        // 将来打点は既存 lookahead が既存比較順で選んだ2手目打牌をそのまま対象にする。
        let context = iishanten_wait_context();
        let actions = iishanten_wait_actions();

        let selection = select_discard_action_with_diagnostic(
            &context,
            &actions,
            LookaheadDiagnosticScope::Lookahead,
        );
        let lookahead = selection.lookahead.expect("2手先診断が構築されている");
        let value = selection.lookahead_value.expect("将来打点が構築されている");

        assert_eq!(value.candidates.len(), lookahead.candidates.len());
        for (candidate, values) in lookahead.candidates.iter().zip(value.candidates.iter()) {
            assert_eq!(candidate.discard, values.discard);
            assert_eq!(candidate.draws.len(), values.draws.len());
            for (draw, draw_value) in candidate.draws.iter().zip(values.draws.iter()) {
                assert_eq!(draw.draw, draw_value.draw);
                assert_eq!(draw.remaining, draw_value.remaining);
                assert_eq!(draw.variants.len(), draw_value.variants.len());
                for (variant, variant_value) in draw.variants.iter().zip(draw_value.variants.iter())
                {
                    assert_eq!(variant.drawn_tile, variant_value.drawn_tile);
                    assert_eq!(variant.remaining, variant_value.remaining);
                    assert_eq!(variant.next_discard_tile(), variant_value.next_discard);
                    // 診断は選択が使った値をそのまま持ち、打点を求め直さない。
                    assert_eq!(variant.prospective_value, variant_value.selection_value);
                }
            }
        }
    }

    #[test]
    fn non_iishanten_candidates_have_no_weighted_tenpai_wait() {
        // テンパイでは前方評価そのものを行わず、2向聴以上では weighted next acceptance のための
        // 前方評価を行う。どちらも1向聴限定の weighted tenpai wait は持たないので、意味の無い 0
        // ではなく None にする。
        let hands: [(&[u8], i8); 2] = [
            // 123m 456m 789m 12p 55s + ツモ 9p。最善はテンパイ。
            (&[0, 4, 8, 12, 17, 20, 24, 28, 32, 36, 40, 89, 90, 68], 0),
            // 13m 5m 8m 24p 6p 7p 13s 4s 55z 7z。最善は2向聴以上。
            (
                &[0, 8, 20, 28, 40, 48, 56, 60, 72, 80, 84, 112, 113, 132],
                2,
            ),
        ];

        for (values, expected_best) in hands {
            let tiles: Vec<_> = values.iter().map(|&value| tile(value)).collect();
            let (hand, drawn) = tiles.split_at(values.len() - 1);
            let context = GameContext::from_parts_with_visible_tiles(
                Some(drawn[0]),
                hand.to_vec(),
                vec![],
                None,
                None,
                tiles.clone(),
            );
            let actions: Vec<LegalAction> = values.iter().map(|&value| dahai(value)).collect();

            let with_diagnostic = select_discard_action_with_diagnostic(
                &context,
                &actions,
                LookaheadDiagnosticScope::None,
            );
            let best = with_diagnostic
                .diagnostic
                .candidates
                .iter()
                .map(|candidate| candidate.evaluation.min_shanten_after_discard())
                .min()
                .unwrap();
            if expected_best == 0 {
                assert_eq!(best, 0);
            } else {
                assert!(best >= expected_best, "{best}");
            }

            assert!(
                with_diagnostic
                    .diagnostic
                    .candidates
                    .iter()
                    .all(|candidate| candidate.tenpai_wait.is_none())
            );
        }
    }

    // ---- 2向聴以上の weighted next acceptance ----

    // 78m 4467p 446s WW FFF。打6sと打7pはいずれも2向聴、現在受け入れは34枚/11種で同値。
    // 既存1手比較では安定順の打6sだが、1手進んだ後の受け入れ加重合計は打7pが広い。
    const RYANSHANTEN_FORWARD_TILES: [u8; 14] =
        [116, 128, 92, 84, 85, 48, 60, 49, 56, 24, 129, 130, 28, 117];

    fn ryanshanten_forward_context() -> GameContext {
        let tiles: Vec<_> = RYANSHANTEN_FORWARD_TILES
            .iter()
            .map(|&value| tile(value))
            .collect();
        let (hand, drawn) = tiles.split_at(13);
        GameContext::from_parts_with_visible_tiles(
            Some(drawn[0]),
            hand.to_vec(),
            vec![],
            None,
            None,
            tiles.clone(),
        )
    }

    fn ryanshanten_forward_actions() -> Vec<LegalAction> {
        vec![dahai(92), dahai(60)]
    }

    fn ryanshanten_all_actions() -> Vec<LegalAction> {
        RYANSHANTEN_FORWARD_TILES
            .iter()
            .map(|&value| dahai(value))
            .collect()
    }

    #[test]
    fn weighted_next_acceptance_changes_a_real_hand_selection() {
        let context = ryanshanten_forward_context();
        let actions = ryanshanten_forward_actions();
        let legal = legal_discard_evaluations(&context, &actions);
        let one_step = select_best_one_step_evaluation(&legal.evaluations).unwrap();
        let selection = select_discard_action_with_evaluation(&context, &actions);
        let selected = selection.evaluation.as_ref().unwrap();

        assert_eq!(one_step.discard, tile(92).tile_type());
        assert_eq!(selected.discard, tile(60).tile_type());
        assert_eq!(selected.min_shanten_after_discard(), 2);
        assert!(one_step.acceptance_total_remaining() >= selected.acceptance_total_remaining());
    }

    #[test]
    fn weighted_next_acceptance_diagnostic_reuses_lookahead_and_keeps_selection_consistent() {
        let context = ryanshanten_forward_context();
        let actions = ryanshanten_forward_actions();
        let normal = select_discard_action_with_evaluation(&context, &actions);
        let without = select_discard_action_with_diagnostic(
            &context,
            &actions,
            LookaheadDiagnosticScope::None,
        );
        let with = select_discard_action_with_diagnostic(
            &context,
            &actions,
            LookaheadDiagnosticScope::Lookahead,
        );

        assert_eq!(normal, without.selection);
        assert_eq!(without.selection, with.selection);
        assert_eq!(without.diagnostic, with.diagnostic);
        let selected = without
            .diagnostic
            .candidates
            .iter()
            .find(|candidate| candidate.selected)
            .unwrap();
        let runner_up = without
            .diagnostic
            .candidates
            .iter()
            .find(|candidate| !candidate.selected)
            .unwrap();
        assert_eq!(
            runner_up.comparison_reason,
            bot_logic::DiscardComparisonReason::WeightedNextAcceptanceRemaining
        );
        assert!(
            selected.next_acceptance.unwrap().weighted_remaining
                > runner_up.next_acceptance.unwrap().weighted_remaining
        );
        assert_eq!(selected.tenpai_wait, None);
        assert!(with.lookahead.is_some());
    }

    #[test]
    fn weighted_next_acceptance_improves_selection_with_all_legal_discards() {
        let context = ryanshanten_forward_context();
        let actions = ryanshanten_all_actions();
        let legal = legal_discard_evaluations(&context, &actions);
        let one_step = select_best_one_step_evaluation(&legal.evaluations).unwrap();
        let normal = select_discard_action_with_evaluation(&context, &actions);
        let without = select_discard_action_with_diagnostic(
            &context,
            &actions,
            LookaheadDiagnosticScope::None,
        );
        let with = select_discard_action_with_diagnostic(
            &context,
            &actions,
            LookaheadDiagnosticScope::Lookahead,
        );

        assert_eq!(legal.evaluations.len(), 9);
        assert_eq!(one_step.discard, tile(92).tile_type());
        assert_eq!(normal.action, Some(dahai(60)));
        assert_eq!(normal, without.selection);
        assert_eq!(without.selection, with.selection);
        assert_eq!(without.diagnostic, with.diagnostic);

        let selected = without
            .diagnostic
            .candidates
            .iter()
            .find(|candidate| candidate.selected)
            .unwrap();
        let runner_up = without
            .diagnostic
            .candidates
            .iter()
            .find(|candidate| candidate.evaluation.discard == one_step.discard)
            .unwrap();
        assert_eq!(selected.evaluation.discard, tile(60).tile_type());
        assert_eq!(runner_up.evaluation.discard, tile(92).tile_type());
        assert_eq!(
            runner_up.comparison_reason,
            bot_logic::DiscardComparisonReason::WeightedNextAcceptanceRemaining
        );
        assert_eq!(selected.next_acceptance.unwrap().weighted_remaining, 428);
        assert_eq!(selected.next_acceptance.unwrap().weighted_type_count, 138);
        assert_eq!(runner_up.next_acceptance.unwrap().weighted_remaining, 396);
        assert_eq!(runner_up.next_acceptance.unwrap().weighted_type_count, 128);
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

        let with_diagnostic = select_discard_action_with_diagnostic(
            &context,
            &actions,
            LookaheadDiagnosticScope::None,
        );
        assert_eq!(
            with_diagnostic.selection,
            select_discard_action_with_evaluation(&context, &actions)
        );
        assert_eq!(
            with_diagnostic.diagnostic.selected,
            with_diagnostic.selection.evaluation
        );
    }

    // 赤5m(16) と通常5m(17) を持つ14枚。5m 以外はすべて完成ブロックで、1手目に 5m を切って
    // E を引くと、2手目の最良打牌は残った 5m になる。
    const RED_FIVE_LOOKAHEAD_TILES: [u8; 14] =
        [16, 17, 40, 44, 48, 57, 61, 65, 76, 80, 84, 89, 90, 108];

    // 2手目に仮想ツモする E。物理牌は手牌の E(108) とは別の未使用コピーを使う。
    const RED_FIVE_LOOKAHEAD_DRAW: u8 = 109;

    fn red_five_lookahead_context() -> GameContext {
        let tiles: Vec<_> = RED_FIVE_LOOKAHEAD_TILES
            .iter()
            .map(|&value| tile(value))
            .collect();
        let (hand, drawn) = tiles.split_at(RED_FIVE_LOOKAHEAD_TILES.len() - 1);
        GameContext::from_parts_with_visible_tiles(
            Some(drawn[0]),
            hand.to_vec(),
            vec![],
            None,
            None,
            tiles.clone(),
        )
    }

    // 5m の片方の物理牌だけを合法にした Dahai 一覧。lookahead は「打牌候補 × 受け入れ牌 ×
    // 次打牌候補」の探索になるため、検証に必要な 5m 候補だけへ絞る。
    fn red_five_lookahead_actions(legal_five: u8) -> Vec<LegalAction> {
        vec![dahai(legal_five)]
    }

    // 1手目に実際の合法 Dahai を切った後の物理手牌から、既存の context-aware 評価で2手目の
    // 最良打牌を求める。テスト側で打牌評価を再実装しないための期待値。
    fn expected_next_discard_after(discarded: u8) -> Option<bot_logic::DiscardEvaluation> {
        let mut tiles: Vec<_> = RED_FIVE_LOOKAHEAD_TILES
            .iter()
            .filter(|&&value| value != discarded)
            .map(|&value| tile(value))
            .collect();
        tiles.push(tile(RED_FIVE_LOOKAHEAD_DRAW));

        let mut visible: Vec<_> = RED_FIVE_LOOKAHEAD_TILES
            .iter()
            .map(|&value| tile(value))
            .collect();
        visible.push(tile(RED_FIVE_LOOKAHEAD_DRAW));

        bot_logic::select_best_discard_from_tiles_with_visible_tiles(
            &tiles,
            &[],
            None,
            None,
            &visible,
        )
    }

    // 5m 候補の lookahead から、E を仮想ツモした場合の2手目評価を取り出す。
    fn lookahead_next_discard_for_five(legal_five: u8) -> bot_logic::DiscardEvaluation {
        let context = red_five_lookahead_context();
        let actions = red_five_lookahead_actions(legal_five);
        let with_diagnostic = select_discard_action_with_diagnostic(
            &context,
            &actions,
            LookaheadDiagnosticScope::Lookahead,
        );

        with_diagnostic
            .lookahead
            .expect("lookahead built on request")
            .candidate(tile(16).tile_type())
            .expect("5m candidate exists")
            .draw(tile(108).tile_type())
            .expect("E draw exists")
            .variants
            .first()
            .expect("E draw variant exists")
            .next_discard
            .clone()
            .expect("next discard exists")
    }

    #[test]
    fn lookahead_discards_the_red_five_when_only_the_red_five_is_legal() {
        // 赤5mだけが合法なら、1手目で赤5mが除かれ通常5mが残る。2手目評価は通常5mが残った
        // 物理手牌を起点とした既存 context-aware 評価と一致する。
        let next = lookahead_next_discard_for_five(16);

        assert_eq!(next.discard, tile(16).tile_type());
        assert!(!next.discards_red_five);
        assert_eq!(next.discarded_dora_count, 0);
        assert_eq!(Some(next), expected_next_discard_after(16));
    }

    #[test]
    fn lookahead_discards_the_black_five_when_only_the_black_five_is_legal() {
        // 通常5mだけが合法なら、1手目で通常5mが除かれ赤5mが残る。
        let next = lookahead_next_discard_for_five(17);

        assert_eq!(next.discard, tile(16).tile_type());
        assert!(next.discards_red_five);
        assert_eq!(next.discarded_dora_count, 1);
        assert_eq!(Some(next), expected_next_discard_after(17));
    }

    #[test]
    fn lookahead_prefers_the_black_five_when_both_fives_are_legal() {
        // 両方合法なら既存の黒牌優先方針どおり通常5mを切り、赤5mが残る。
        let context = red_five_lookahead_context();
        let actions = vec![dahai(16), dahai(17)];
        let with_diagnostic = select_discard_action_with_diagnostic(
            &context,
            &actions,
            LookaheadDiagnosticScope::Lookahead,
        );

        let next = with_diagnostic
            .lookahead
            .expect("lookahead built on request")
            .candidate(tile(16).tile_type())
            .expect("5m candidate exists")
            .draw(tile(108).tile_type())
            .expect("E draw exists")
            .variants
            .first()
            .expect("E draw variant exists")
            .next_discard
            .clone()
            .expect("next discard exists");

        assert!(next.discards_red_five);
        assert_eq!(Some(next), expected_next_discard_after(17));
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

        let without = select_discard_action_with_diagnostic(
            &context,
            &actions,
            LookaheadDiagnosticScope::None,
        );
        let with = select_discard_action_with_diagnostic(
            &context,
            &actions,
            LookaheadDiagnosticScope::Lookahead,
        );

        assert!(without.lookahead.is_none());
        assert_eq!(without.selection, with.selection);
        assert_eq!(without.diagnostic, with.diagnostic);

        let lookahead = with.lookahead.expect("lookahead built on request");
        assert!(with.diagnostic.candidates.len() > 1);
        assert_eq!(lookahead.candidates.len(), with.diagnostic.candidates.len());
        for (candidate_lookahead, candidate) in lookahead
            .candidates
            .iter()
            .zip(with.diagnostic.candidates.iter())
        {
            assert_eq!(candidate_lookahead.discard, candidate.evaluation.discard);
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

        let with_diagnostic = select_discard_action_with_diagnostic(
            &context,
            &actions,
            LookaheadDiagnosticScope::None,
        );
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

        let with_diagnostic = select_discard_action_with_diagnostic(
            &context,
            &actions,
            LookaheadDiagnosticScope::None,
        );
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

        let with_diagnostic = select_discard_action_with_diagnostic(
            &context,
            &actions,
            LookaheadDiagnosticScope::None,
        );
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

        let with_diagnostic = select_discard_action_with_diagnostic(
            &context,
            &actions,
            LookaheadDiagnosticScope::None,
        );
        assert_eq!(with_diagnostic.selection.action, None);
        assert_eq!(with_diagnostic.selection.evaluation, None);
        assert_eq!(with_diagnostic.diagnostic.selected, None);
        assert!(with_diagnostic.diagnostic.candidates.is_empty());
    }

    // ---- 打牌後 concealed hand ----

    // 打牌後の手牌の組み立てが読むのは discard 牌種と discards_red_five だけ。他はダミー。
    fn discard_evaluation(discard: TileType, discards_red_five: bool) -> DiscardEvaluation {
        let shanten = EffectiveShanten::Concealed(bot_logic::Shanten {
            standard: 1,
            chiitoitsu: 6,
            kokushi: 13,
        });
        DiscardEvaluation {
            discard,
            count_before_discard: 1,
            shanten_after_discard: shanten,
            acceptance_after_discard: bot_logic::Acceptance {
                current: shanten,
                tiles: Vec::new(),
            },
            shape_penalty: 0,
            floating_tile_value: 0,
            discarded_dora_count: 0,
            discarded_value_honor_count: 0,
            discards_red_five,
            discards_isolated_tile: false,
            standard_iishanten_shape_after_discard: bot_logic::IishantenShape::Unknown,
        }
    }

    #[test]
    fn concealed_tiles_after_discard_removes_one_physical_tile_from_hand_and_draw() {
        // 手牌とツモ牌を合わせた物理牌から、切る1枚だけを除く。
        let context = GameContext::from_parts(Some(tile(104)), vec![tile(0), tile(4), tile(5)]);
        let evaluation = discard_evaluation(tile(4).tile_type(), false);

        let tiles = concealed_tiles_after_discard(&context, &evaluation).expect("一致する物理牌");

        // 同じ牌種を2枚持っていても除くのは1枚だけ。
        let two_man = tile(4).tile_type();
        assert_eq!(tiles.len(), 3);
        assert_eq!(
            tiles
                .iter()
                .filter(|tile| tile.tile_type() == two_man)
                .count(),
            1
        );
        assert!(tiles.contains(&tile(0)));
        assert!(tiles.contains(&tile(104)));
    }

    #[test]
    fn concealed_tiles_after_discard_can_remove_the_drawn_tile() {
        let context = GameContext::from_parts(Some(tile(104)), vec![tile(0), tile(4)]);
        let evaluation = discard_evaluation(tile(104).tile_type(), false);

        let tiles = concealed_tiles_after_discard(&context, &evaluation).expect("一致する物理牌");

        assert_eq!(tiles, vec![tile(0), tile(4)]);
    }

    #[test]
    fn concealed_tiles_after_discard_distinguishes_red_and_black_fives() {
        // 赤5と通常5は同じ牌種なので、赤フラグまで一致させないと切る牌を取り違える。
        let context = GameContext::from_parts(None, vec![tile(16), tile(17), tile(0)]);
        let five = tile(16).tile_type();

        let discards_red = concealed_tiles_after_discard(&context, &discard_evaluation(five, true))
            .expect("赤5が手牌にある");
        assert!(!discards_red.contains(&tile(16)));
        assert!(discards_red.contains(&tile(17)));

        let discards_black =
            concealed_tiles_after_discard(&context, &discard_evaluation(five, false))
                .expect("通常5が手牌にある");
        assert!(discards_black.contains(&tile(16)));
        assert!(!discards_black.contains(&tile(17)));
    }

    #[test]
    fn concealed_tiles_after_discard_is_none_without_a_matching_physical_tile() {
        // 一致する物理牌が無ければ別の牌で代用せず None にする。
        let black_five_only = GameContext::from_parts(None, vec![tile(17), tile(0)]);
        assert_eq!(
            concealed_tiles_after_discard(
                &black_five_only,
                &discard_evaluation(tile(16).tile_type(), true)
            ),
            None
        );

        let red_five_only = GameContext::from_parts(None, vec![tile(16), tile(0)]);
        assert_eq!(
            concealed_tiles_after_discard(
                &red_five_only,
                &discard_evaluation(tile(16).tile_type(), false)
            ),
            None
        );

        assert_eq!(
            concealed_tiles_after_discard(
                &black_five_only,
                &discard_evaluation(tile(104).tile_type(), false)
            ),
            None
        );

        assert_eq!(
            concealed_tiles_after_discard(
                &GameContext::default(),
                &discard_evaluation(tile(0).tile_type(), false)
            ),
            None
        );
    }

    // ---- 1向聴 selection への将来打点の接続 ----

    // mjai 表記の14枚とドラ表示牌から、将来打点を確定できる局面を作る。
    //
    // 場風・自風・自分の席を既知にし、手牌とドラ表示牌をそのまま見え牌として渡す。合法 Dahai は
    // 手牌の全牌にする。
    fn value_context(hand: &[&str; 14], dora_indicator: &str) -> (GameContext, Vec<LegalAction>) {
        value_context_with_winds(hand, dora_indicator, true)
    }

    // `winds == false` では場風・自風を渡さず、点数計算の入力が足りない局面にする。将来打点を
    // どの枝でも確定できないため、既存 #175 と同じく打点込みの集計値だけが `None` になる。
    fn value_context_with_winds(
        hand: &[&str; 14],
        dora_indicator: &str,
        winds: bool,
    ) -> (GameContext, Vec<LegalAction>) {
        let mut used: Vec<TileId> = Vec::new();
        let mut take = |mjai: &str| {
            let red = mjai.ends_with('r');
            let tile_type =
                TileType::from_mjai_type_str(mjai.trim_end_matches('r')).expect("牌種として読める");
            let tile = TileId::copies(tile_type)
                .find(|tile| tile.is_red() == red && !used.contains(tile))
                .expect("未使用の物理牌がある");
            used.push(tile);
            tile
        };

        let tiles: Vec<TileId> = hand.iter().map(|mjai| take(mjai)).collect();
        let dora_indicators = vec![take(dora_indicator)];
        let (hand_tiles, drawn) = tiles.split_at(tiles.len() - 1);
        let visible: Vec<TileId> = tiles
            .iter()
            .chain(dora_indicators.iter())
            .copied()
            .collect();

        let context = GameContext::from_parts_with_table_state(
            Some(drawn[0]),
            hand_tiles.to_vec(),
            dora_indicators,
            winds.then(|| TileType::from_mjai_type_str("E").expect("牌種として読める")),
            winds.then(|| TileType::from_mjai_type_str("S").expect("牌種として読める")),
            visible,
            Some(0),
            Some(3),
            Default::default(),
            [false; 4],
        )
        // 履歴依存フリテンを既知にして、未来テンパイのロン可否まで確定できる局面にする。
        .with_history_furiten_facts(HistoryFuritenFacts {
            same_turn: Some(false),
            riichi_missed_win: Some(false),
        });
        let actions = tiles
            .iter()
            .map(|&tile| LegalAction::Dahai { tile })
            .collect();
        (context, actions)
    }

    // 打点込みの集計値を外した前方集計値。既存 weighted wait 以降だけで比較した場合を再現する。
    fn without_prospective_value(metrics: &[ForwardMetrics]) -> Vec<ForwardMetrics> {
        metrics
            .iter()
            .map(|metric| ForwardMetrics {
                prospective_value: None,
                ..*metric
            })
            .collect()
    }

    // 打点込みの評価器を渡さない2手先評価。既存比較順だけで2手目を選んだ場合を再現する。
    fn lookahead_without_valuator(
        context: &GameContext,
        tiles: &[TileId],
        evaluations: &[DiscardEvaluation],
    ) -> LookaheadDiagnostic {
        diagnose_lookahead(
            &LookaheadInputs::new(
                tiles,
                evaluation_fixed_meld_count(context),
                context.dora_indicators(),
                context.round_wind(),
                context.seat_wind(),
            )
            .with_visible_tiles(context.visible_tiles()),
            evaluations,
        )
    }

    fn discard_of(evaluations: &[DiscardEvaluation], index: usize) -> String {
        evaluations[index].discard.to_mjai_string()
    }

    fn metric_of(
        evaluations: &[DiscardEvaluation],
        metrics: &[ForwardMetrics],
        discard: &str,
    ) -> ForwardMetrics {
        let index = evaluations
            .iter()
            .position(|evaluation| evaluation.discard.to_mjai_string() == discard)
            .expect("打牌候補がある");
        metrics[index]
    }

    // 123m 1p1p 2m2m 3m 7m 8m 9m 4p 5p 5p 6p 8p 相当の1向聴。ドラ表示牌 4p でドラは 5p。
    //
    // 打 5p の方が受け入れ後のテンパイ待ちは広いが、ドラ 5p を切ってしまうので将来打点が下がる。
    const VALUE_OVER_WAIT_HAND: [&str; 14] = [
        "4p", "2m", "5p", "7m", "8p", "9m", "1p", "1p", "3m", "1m", "6p", "5p", "2m", "8m",
    ];

    #[test]
    fn the_prospective_value_outranks_the_weighted_wait_in_the_selection() {
        // 待ち枚数が狭くても将来打点が高い打牌を選ぶ。
        let (context, actions) = value_context(&VALUE_OVER_WAIT_HAND, "4p");
        let legal = legal_discard_evaluations(&context, &actions);
        let metrics = selection_forward_metrics(&context, &legal.tiles, &legal.evaluations);

        let high_value = metric_of(&legal.evaluations, &metrics, "8p");
        let wide_wait = metric_of(&legal.evaluations, &metrics, "5p");
        assert!(high_value.prospective_value > wide_wait.prospective_value);
        assert!(
            high_value
                .tenpai_wait
                .expect("1向聴候補")
                .weighted_remaining
                < wide_wait.tenpai_wait.expect("1向聴候補").weighted_remaining,
            "待ち枚数では 5p が勝つ局面である"
        );

        let selected =
            best_discard_selection_index_with_forward_metrics(&legal.evaluations, &metrics)
                .expect("最善候補がある");
        assert_eq!(discard_of(&legal.evaluations, selected), "8p");

        // 打点を外すと従来どおり待ち枚数の広い 5p が選ばれる。
        let without = best_discard_selection_index_with_forward_metrics(
            &legal.evaluations,
            &without_prospective_value(&metrics),
        )
        .expect("最善候補がある");
        assert_eq!(discard_of(&legal.evaluations, without), "5p");

        assert_eq!(
            select_discard_action(&context, &actions),
            Some(LegalAction::Dahai {
                tile: legal_dahai_tile_for_type(legal.evaluations[selected].discard, &actions)
                    .expect("合法 Dahai がある")
            })
        );
    }

    // 11m 2m 33m 7m 8m 9m 2p 3p 4p 7p 9p E 相当の1向聴。ドラ表示牌 2m でドラは 3m。
    //
    // 打 E から 1m を引いた枝は、2m を切るか 3m を切るかで最終打点が変わる。
    const SECOND_DISCARD_HAND: [&str; 14] = [
        "3p", "4p", "9p", "1m", "8m", "1m", "2p", "3m", "7m", "3m", "2m", "9m", "E", "7p",
    ];

    #[test]
    fn the_second_discard_is_chosen_with_the_prospective_value() {
        // 2手目の最良打牌そのものが将来打点で変わる。
        let (context, actions) = value_context(&SECOND_DISCARD_HAND, "2m");
        let legal = legal_discard_evaluations(&context, &actions);
        let valuator = ProductionProspectiveValuator::new(&context);
        let aware = diagnose_lookahead(
            &lookahead_inputs(
                &context,
                &legal.tiles,
                &valuator,
                LookaheadDiagnosticScope::None,
            ),
            &legal.evaluations,
        );
        let plain = lookahead_without_valuator(&context, &legal.tiles, &legal.evaluations);

        let branch = |lookahead: &LookaheadDiagnostic| {
            lookahead
                .candidate(TileType::from_mjai_type_str("E").unwrap())
                .expect("打 E の候補がある")
                .draw(TileType::from_mjai_type_str("1m").unwrap())
                .expect("1m の受け入れがある")
                .variants
                .first()
                .expect("物理牌 variant がある")
                .clone()
        };

        let aware_branch = branch(&aware);
        let plain_branch = branch(&plain);
        assert_eq!(
            aware_branch.next_discard_tile().map(|t| t.to_mjai_string()),
            Some("2m".to_string()),
            "打点込みならドラの 3m を残す"
        );
        assert_eq!(
            plain_branch.next_discard_tile().map(|t| t.to_mjai_string()),
            Some("3m".to_string()),
            "打点を見なければ 3m を切る"
        );
        assert!(aware_branch.prospective_value.is_some());
        assert_eq!(plain_branch.prospective_value, None);
    }

    // 2m 3m 4m 4m 6m 6m 7m 8m 1p 2p 4p 4p 5p 6p 8p 相当の1向聴。ドラ表示牌 5m でドラは 6m。
    //
    // 打 8p の枝で2手目の最良打牌が打点込みだと変わり、その結果 1手目の weighted wait も変わる。
    const SECOND_DISCARD_FLIP_HAND: [&str; 14] = [
        "6m", "8p", "6p", "6m", "4p", "3m", "4m", "5p", "2p", "1p", "4p", "2m", "8m", "7m",
    ];

    #[test]
    fn the_value_aware_second_discard_changes_the_first_discard() {
        // 2手目の変更が1手目の集計値へ伝わり、現在打牌の選択そのものを変える。
        let (context, actions) = value_context(&SECOND_DISCARD_FLIP_HAND, "5m");
        let legal = legal_discard_evaluations(&context, &actions);
        let valuator = ProductionProspectiveValuator::new(&context);
        let aware = diagnose_lookahead(
            &lookahead_inputs(
                &context,
                &legal.tiles,
                &valuator,
                LookaheadDiagnosticScope::None,
            ),
            &legal.evaluations,
        );
        let plain = lookahead_without_valuator(&context, &legal.tiles, &legal.evaluations);

        // 打点込みで選んだ2手目から集計した weighted wait と、従来の2手目から集計した値は違う。
        let aware_metrics = without_prospective_value(&forward_metrics_from_lookahead(
            &lookahead_inputs(
                &context,
                &legal.tiles,
                &valuator,
                LookaheadDiagnosticScope::None,
            ),
            &legal.evaluations,
            &aware,
        ));
        let plain_metrics = without_prospective_value(&forward_metrics_from_lookahead(
            &lookahead_inputs(
                &context,
                &legal.tiles,
                &valuator,
                LookaheadDiagnosticScope::None,
            ),
            &legal.evaluations,
            &plain,
        ));
        assert_ne!(
            metric_of(&legal.evaluations, &aware_metrics, "8p"),
            metric_of(&legal.evaluations, &plain_metrics, "8p"),
        );

        // その差だけで現在打牌の選択が変わる。
        let aware_best =
            best_discard_selection_index_with_forward_metrics(&legal.evaluations, &aware_metrics)
                .expect("最善候補がある");
        let plain_best =
            best_discard_selection_index_with_forward_metrics(&legal.evaluations, &plain_metrics)
                .expect("最善候補がある");
        assert_eq!(discard_of(&legal.evaluations, aware_best), "6m");
        assert_eq!(discard_of(&legal.evaluations, plain_best), "8p");
    }

    // 2m 4m 6m 8m 9m 9m 1p 2p 3p 4p 6p 7p 8p 9p 相当の1向聴。ドラ表示牌 7m でドラは 8m。
    //
    // 打 2m から 5p を引く枝で、赤5p と黒5p では2手目の最良打牌が変わる。
    const RED_FIVE_BRANCH_HAND: [&str; 14] = [
        "6m", "4p", "2m", "4m", "2p", "6p", "9m", "7p", "8p", "1p", "9m", "8m", "9p", "3p",
    ];

    #[test]
    fn a_red_and_a_black_first_draw_can_choose_different_second_discards() {
        // 仮想ツモの赤5 / 黒5は打点だけでなく2手目の最良打牌そのものを変え得る。
        let (context, actions) = value_context(&RED_FIVE_BRANCH_HAND, "7m");
        let selection = select_discard_action_with_diagnostic(
            &context,
            &actions,
            LookaheadDiagnosticScope::Lookahead,
        );
        let draw = selection
            .lookahead
            .expect("2手先診断が構築されている")
            .candidate(TileType::from_mjai_type_str("2m").unwrap())
            .expect("打 2m の候補がある")
            .draw(TileType::from_mjai_type_str("5p").unwrap())
            .expect("5p の受け入れがある")
            .clone();

        // 牌種単位の残枚数を赤 / 黒へ分け、合計は元の残枚数と一致する。
        assert_eq!(draw.variants.len(), 2);
        assert_eq!(
            draw.variants
                .iter()
                .map(|variant| u32::from(variant.remaining))
                .sum::<u32>(),
            u32::from(draw.remaining),
        );

        let variant = |red: bool| {
            draw.variants
                .iter()
                .find(|variant| variant.drawn_tile.is_red() == red)
                .expect("指定した物理牌 variant がある")
        };
        assert_eq!(variant(true).remaining, 1);
        assert_eq!(variant(false).remaining, draw.remaining - 1);
        assert_ne!(
            variant(true).next_discard_tile(),
            variant(false).next_discard_tile(),
        );
        assert_ne!(
            variant(true).prospective_value,
            variant(false).prospective_value,
        );
    }

    // ---- 1向聴 selected discard の前方集計値の押し引きへの受け渡し ----

    // 通常打牌選択と押し引き入力を、本番 act() と同じ helper だけを通して組み立てる。
    fn selected_offense(
        context: &GameContext,
        actions: &[LegalAction],
    ) -> (DiscardActionSelection, PushPullOffenseState) {
        let selection = select_discard_action_with_evaluation(context, actions);
        let inputs = push_pull_inputs_from_threat_facts(
            context,
            player_threat_facts_from_context(context),
            selection.evaluation.as_ref(),
            selection.iishanten_forward_metrics,
            selection.tenpai_wait.as_ref(),
            selection.tenpai_offense_value,
            actions,
        );
        let offense = inputs.offense.expect("攻撃評価がある");
        (selection, offense)
    }

    #[test]
    fn the_selected_iishanten_forward_metrics_reach_the_push_pull_offense_state() {
        // 選んだ1向聴打牌の前方集計値が、押し引きの offense state から観測できる。
        let (context, actions) = value_context(&VALUE_OVER_WAIT_HAND, "4p");
        let (selection, offense) = selected_offense(&context, &actions);

        assert_eq!(
            selection
                .evaluation
                .as_ref()
                .expect("選択できる")
                .discard
                .to_mjai_string(),
            "8p"
        );
        assert_eq!(offense.min_shanten_after_discard, 1);

        let forward = offense
            .iishanten_forward_metrics
            .expect("1向聴の前方集計値がある");
        let wait = forward.tenpai_wait.expect("テンパイ待ちの集計値がある");
        assert!(forward.prospective_value.is_some());
        assert_eq!(forward.prospective_value, wait.prospective_value);
        assert!(wait.weighted_remaining > 0);
        assert!(wait.weighted_type_count > 0);
    }

    #[test]
    fn the_push_pull_forward_metrics_are_the_ones_the_comparator_used() {
        // 押し引きへ渡る集計値は、通常打牌の比較へ入力した集計値そのもの。二重計算しない。
        let (context, actions) = value_context(&VALUE_OVER_WAIT_HAND, "4p");
        let legal = legal_discard_evaluations(&context, &actions);
        let metrics = selection_forward_metrics(&context, &legal.tiles, &legal.evaluations);
        let selected =
            best_discard_selection_index_with_forward_metrics(&legal.evaluations, &metrics)
                .expect("最善候補がある");

        let (_, offense) = selected_offense(&context, &actions);
        assert_eq!(offense.iishanten_forward_metrics, Some(metrics[selected]));
    }

    #[test]
    fn a_single_candidate_still_reports_the_selected_forward_metrics() {
        // 候補が1件で前方比較が要らない場合でも、選んだ打牌が1向聴なら診断用の集計値は取れる。
        let (context, actions) = value_context(&VALUE_OVER_WAIT_HAND, "4p");
        let single: Vec<LegalAction> = actions
            .iter()
            .filter(|action| match action {
                LegalAction::Dahai { tile } => tile.tile_type().to_mjai_string() == "8p",
                _ => false,
            })
            .cloned()
            .collect();

        // 候補1件では選択そのものに前方評価が要らないため、selection は集計値を持たない。
        let legal = legal_discard_evaluations(&context, &single);
        assert_eq!(legal.evaluations.len(), 1);
        assert_eq!(
            selection_forward_metrics(&context, &legal.tiles, &legal.evaluations),
            vec![ForwardMetrics::default()]
        );

        // 全候補を比較した経路が同じ打牌へ求めた集計値と一致する。別の計算器を作っていない。
        let all = legal_discard_evaluations(&context, &actions);
        let expected = metric_of(
            &all.evaluations,
            &selection_forward_metrics(&context, &all.tiles, &all.evaluations),
            "8p",
        );

        let (_, offense) = selected_offense(&context, &single);
        assert_eq!(offense.min_shanten_after_discard, 1);
        assert_eq!(offense.iishanten_forward_metrics, Some(expected));
    }

    #[test]
    fn a_multi_shanten_selection_has_no_iishanten_forward_metrics() {
        // 1向聴でない打牌では前方集計値を持ち回らない。2向聴以上の押し引きは今回変えない。
        let (context, actions) = value_context(&VALUE_OVER_WAIT_HAND, "4p");
        let single: Vec<LegalAction> = actions
            .iter()
            .filter(|action| match action {
                LegalAction::Dahai { tile } => tile.tile_type().to_mjai_string() == "1m",
                _ => false,
            })
            .cloned()
            .collect();

        let (selection, offense) = selected_offense(&context, &single);
        assert!(selection.evaluation.is_some());
        assert!(offense.min_shanten_after_discard > 1);
        assert_eq!(offense.iishanten_forward_metrics, None);
    }

    #[test]
    fn an_unknown_prospective_value_stays_unknown_in_the_offense_state() {
        // 打点を確定できない枝がある場合、押し引きへ渡る打点込みの集計値も 0 ではなく `None`。
        let (context, actions) = value_context_with_winds(&VALUE_OVER_WAIT_HAND, "4p", false);
        let (_, offense) = selected_offense(&context, &actions);

        assert_eq!(offense.min_shanten_after_discard, 1);
        let forward = offense
            .iishanten_forward_metrics
            .expect("1向聴の前方集計値がある");
        assert_eq!(forward.prospective_value, None);

        // 打点を確定できなくても既存 weighted wait は求まる。
        let wait = forward.tenpai_wait.expect("テンパイ待ちの集計値がある");
        assert_eq!(wait.prospective_value, None);
        assert!(wait.weighted_remaining > 0);
    }

    #[test]
    fn tenpai_and_multi_shanten_candidates_have_no_prospective_value() {
        // テンパイでは前方評価そのものを行わず、2向聴以上では既存 weighted next acceptance の
        // ための前方評価を行う。ただし将来打点は1向聴限定なので、どちらも打点込みの集計値は
        // 持たない。
        for hand in [
            // 123m 456m 789m 12p 55s + ツモ 9p。最善はテンパイ。
            [
                "1m", "2m", "3m", "4m", "5m", "6m", "7m", "8m", "9m", "1p", "2p", "5s", "5s", "9p",
            ],
            // 13m 5m 8m 24p 6p 7p 13s 4s 55z 7z。最善は2向聴以上。
            [
                "1m", "3m", "5m", "8m", "2p", "4p", "6p", "7p", "1s", "3s", "4s", "W", "W", "C",
            ],
        ] {
            let (context, actions) = value_context(&hand, "1m");
            let legal = legal_discard_evaluations(&context, &actions);
            let metrics = selection_forward_metrics(&context, &legal.tiles, &legal.evaluations);

            assert_ne!(
                legal
                    .evaluations
                    .iter()
                    .map(DiscardEvaluation::min_shanten_after_discard)
                    .min(),
                Some(1),
            );
            assert!(
                metrics
                    .iter()
                    .all(|metric| metric.prospective_value.is_none())
            );
        }
    }

    // ---- 1向聴 selection への self-tsumo continuation の接続 ----

    // 1267m 55567p 5888s + ツモ 9s の1向聴。打 5s と打 9s は打牌後の向聴が同じで、従来は既存
    // 比較順で 5s を選んでいた。手変わりを1回挟む経路まで含めた期待ツモ支払いでは 9s の方が
    // 高い。
    const SELF_TSUMO_FLIP_HAND: [&str; 14] = [
        "1m", "2m", "6m", "7m", "5p", "5p", "5p", "6p", "7p", "5s", "8s", "8s", "8s", "9s",
    ];

    // 山の残枚数を既知にした局面。self-tsumo continuation の材料が揃う。
    fn self_tsumo_context(
        hand: &[&str; 14],
        dora_indicator: &str,
        remaining_tiles: u32,
    ) -> (GameContext, Vec<LegalAction>) {
        let (context, actions) = value_context(hand, dora_indicator);
        let context = context.with_table_state_facts(bot_core_table_state(remaining_tiles));
        (context, actions)
    }

    fn bot_core_table_state(remaining_tiles: u32) -> crate::context::TableStateFacts {
        crate::context::TableStateFacts {
            remaining_tiles: Some(remaining_tiles),
            ..Default::default()
        }
    }

    fn selected_discard(context: &GameContext, actions: &[LegalAction]) -> String {
        select_discard_action_with_evaluation(context, actions)
            .evaluation
            .expect("打牌候補がある")
            .discard
            .to_mjai_string()
    }

    #[test]
    fn the_self_tsumo_continuation_changes_the_selected_discard() {
        // 山の残枚数が分かる局面だけ新しい軸が効き、選ぶ打牌が変わる。
        let (unknown_wall, actions) = value_context(&SELF_TSUMO_FLIP_HAND, "1p");
        let (known_wall, _) = self_tsumo_context(&SELF_TSUMO_FLIP_HAND, "1p", 60);

        assert_eq!(selected_discard(&unknown_wall, &actions), "5s");
        assert_eq!(selected_discard(&known_wall, &actions), "9s");
    }

    #[test]
    fn the_new_winner_has_the_higher_expected_self_tsumo_value() {
        // 期待結果は threshold ではなく、確率 × ツモ支払いの計算結果そのものから決まる。
        let (context, actions) = self_tsumo_context(&SELF_TSUMO_FLIP_HAND, "1p", 60);
        let legal = legal_discard_evaluations(&context, &actions);
        let valuator = ProductionProspectiveValuator::new(&context);
        let metrics = forward_metrics(
            &lookahead_inputs(
                &context,
                &legal.tiles,
                &valuator,
                LookaheadDiagnosticScope::None,
            ),
            &legal.evaluations,
        );

        let value = |discard: &str| {
            metric_of(&legal.evaluations, &metrics, discard)
                .expected_self_tsumo_value
                .expect("ツモ打点を確定できる")
        };
        assert!(
            value("9s") > value("5s"),
            "9s: {}, 5s: {}",
            value("9s"),
            value("5s")
        );
    }

    #[test]
    fn the_losing_candidate_reports_the_self_tsumo_axis() {
        // 診断の比較理由も production selection と同じ軸になる。
        let (context, actions) = self_tsumo_context(&SELF_TSUMO_FLIP_HAND, "1p", 60);
        let selection = select_discard_action_with_diagnostic(
            &context,
            &actions,
            LookaheadDiagnosticScope::None,
        );
        let candidate = selection
            .diagnostic
            .candidates
            .iter()
            .find(|candidate| candidate.evaluation.discard.to_mjai_string() == "5s")
            .expect("打 5s の候補がある");

        assert!(!candidate.selected);
        assert_eq!(
            candidate.comparison_reason,
            bot_logic::DiscardComparisonReason::ExpectedSelfTsumoValue
        );
    }

    #[test]
    fn the_self_tsumo_axis_does_not_depend_on_the_candidate_order() {
        // 候補の列挙順を変えても選ぶ打牌は変わらない。
        let (context, actions) = self_tsumo_context(&SELF_TSUMO_FLIP_HAND, "1p", 60);
        let mut reversed = actions.clone();
        reversed.reverse();

        assert_eq!(
            selected_discard(&context, &actions),
            selected_discard(&context, &reversed)
        );
    }

    #[test]
    fn the_diagnostic_scope_does_not_change_the_self_tsumo_selection() {
        // 詳細診断をどこまで構築しても、打牌選択の結果も使う値も変わらない。
        let (context, actions) = self_tsumo_context(&SELF_TSUMO_FLIP_HAND, "1p", 60);
        let selections: Vec<_> = [
            LookaheadDiagnosticScope::None,
            LookaheadDiagnosticScope::Lookahead,
            LookaheadDiagnosticScope::SameShantenDownstream,
        ]
        .into_iter()
        .map(|scope| select_discard_action_with_diagnostic(&context, &actions, scope))
        .collect();

        for selection in &selections {
            assert_eq!(selection.selection.action, selections[0].selection.action);
            assert_eq!(
                selection.selection.iishanten_forward_metrics,
                selections[0].selection.iishanten_forward_metrics,
            );
        }
        assert!(
            selections[0]
                .selection
                .iishanten_forward_metrics
                .expect("1向聴の前方集計値がある")
                .expected_self_tsumo_value
                .is_some()
        );
    }

    #[test]
    fn a_two_shanten_candidate_set_is_unchanged_by_the_self_tsumo_axis() {
        // 2向聴以上では新しい軸を使わず、既存 winner も前方集計値も変わらない。
        let hand: [&str; 14] = [
            "1m", "4m", "7m", "1p", "4p", "7p", "1s", "4s", "7s", "E", "S", "W", "N", "P",
        ];
        let (unknown_wall, actions) = value_context(&hand, "1p");
        let (known_wall, _) = self_tsumo_context(&hand, "1p", 60);

        let metrics = |context: &GameContext| {
            let legal = legal_discard_evaluations(context, &actions);
            let valuator = ProductionProspectiveValuator::new(context);
            forward_metrics(
                &lookahead_inputs(
                    context,
                    &legal.tiles,
                    &valuator,
                    LookaheadDiagnosticScope::None,
                ),
                &legal.evaluations,
            )
        };

        assert_eq!(
            selected_discard(&known_wall, &actions),
            selected_discard(&unknown_wall, &actions)
        );
        assert_eq!(metrics(&known_wall), metrics(&unknown_wall));
        assert!(
            metrics(&known_wall)
                .iter()
                .all(|metric| metric.expected_self_tsumo_value.is_none())
        );
    }

    #[test]
    fn the_own_future_draws_come_from_the_remaining_wall() {
        // 残り自摸機会は山の残枚数の4分の1で、巡目や河の枚数から推測しない。
        let (context, _) = self_tsumo_context(&SELF_TSUMO_FLIP_HAND, "1p", 60);
        assert_eq!(own_future_draws(&context), Some(15));

        let (context, _) = self_tsumo_context(&SELF_TSUMO_FLIP_HAND, "1p", 3);
        assert_eq!(own_future_draws(&context), Some(0));

        let (unknown_wall, _) = value_context(&SELF_TSUMO_FLIP_HAND, "1p");
        assert_eq!(own_future_draws(&unknown_wall), None);
    }
}
