use crate::action::LegalAction;
use crate::context::GameContext;
use crate::discard_selection::select_best_discard_evaluation;
use bot_logic::{DiscardEvaluation, IishantenShape};

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
/// - 打点
/// - 点棒状況
/// - 局・順位条件
///
/// 待ち形については Complete 一向聴だけを限定的に考慮する。一般的な良形・愚形評価は未対応で、
/// `Headless` / `Kuttsuki` / `Weak` に固定順位や押し引き差は付けない。
/// また、自分が親の場合の一向聴を限定的に考慮する。正確な打点・点棒・順位条件は未対応。
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
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PushPullOffenseState {
    pub min_shanten_after_discard: i8,
    pub acceptance_total_remaining: u8,
    pub acceptance_type_count: usize,
    pub standard_iishanten_shape_after_discard: IishantenShape,
}

/// 押し引き判定に使用する入力データ。
///
/// - `opponent_reach_count`: 自分を除くリーチ者数。
/// - `dealer_reacher`: 他家リーチ者に親が含まれるか。親情報がない場合は false。
/// - `self_dealer`: 自分が親か。`player_id` または `oya` が不明なら false。
/// - `offense`: 攻撃評価を構築できない場合は `None`。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PushPullInputs {
    pub opponent_reach_count: u8,
    pub dealer_reacher: bool,
    pub self_dealer: bool,
    pub offense: Option<PushPullOffenseState>,
}

/// 押し引き判定がどの条件で下されたかを表す理由。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PushPullReason {
    NoOpponentReach,
    MissingOffenseEvaluation,
    TenpaiAgainstSingleNonDealer,
    TenpaiUnderHighPressure,
    StrongIishantenAgainstSingleNonDealer,
    CompleteIishantenAgainstSingleNonDealer,
    DealerIishantenAgainstSingleNonDealer,
    IishantenUnderHighPressure,
    TwoOrMoreShanten,
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

/// `GameContext` から押し引き判定の入力を構築する。
///
/// リーチ情報は `context.reached_opponents()` を使用する。`player_id == None` の場合は
/// `reached_opponents()` の仕様どおり、リーチフラグが立っている全席を対象にする。
/// 攻撃評価は既存の best discard 評価を再利用し、手牌とツモ牌が空なら `offense == None`。
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
        select_best_discard_evaluation(context, &tiles)
    };

    push_pull_inputs_from_context_with_evaluation(context, evaluation.as_ref())
}

/// すでに計算済みの `DiscardEvaluation` を利用して押し引き入力を構築する crate-private helper。
///
/// リーチ者数と親リーチ判定は `push_pull_inputs_from_context()` と同じロジックを共有する。
/// offense は渡された evaluation から構築し、新しい向聴数・受け入れ計算は行わない。
/// evaluation が `None` なら offense も `None`。
pub(crate) fn push_pull_inputs_from_context_with_evaluation(
    context: &GameContext,
    evaluation: Option<&DiscardEvaluation>,
) -> PushPullInputs {
    let reached_opponents = context.reached_opponents();
    let opponent_reach_count = reached_opponents.len() as u8;
    let dealer_reacher = context
        .oya()
        .is_some_and(|oya| reached_opponents.contains(&(oya as usize)));
    let self_dealer = match (context.player_id(), context.oya()) {
        (Some(player_id), Some(oya)) => player_id == oya,
        _ => false,
    };

    let offense = evaluation.map(|evaluation| PushPullOffenseState {
        min_shanten_after_discard: evaluation.min_shanten_after_discard(),
        acceptance_total_remaining: evaluation.acceptance_total_remaining(),
        acceptance_type_count: evaluation.acceptance_type_count(),
        standard_iishanten_shape_after_discard: evaluation.standard_iishanten_shape_after_discard,
    });

    PushPullInputs {
        opponent_reach_count,
        dealer_reacher,
        self_dealer,
        offense,
    }
}

/// 押し引きを判定する pure な暫定 helper。
///
/// これは最初の保守的な土台であり、以下を考慮していない。
///
/// - 打点
/// - 点棒状況
/// - 局・順位条件
///
/// 待ち形については Complete 一向聴だけを限定的に考慮する。一般的な良形・愚形評価は未対応で、
/// `Headless` / `Kuttsuki` / `Weak` に固定順位や押し引き差は付けない。
/// また、自分が親の場合の一向聴を限定的に考慮する。正確な打点・点棒・順位条件は未対応。
/// また、暫定 threshold は実戦の regression test に基づいて将来調整する。
/// この判定結果は `ShantenAgent` の action 選択に反映される。
///
/// - `Push`: Reach → 通常打牌 → 防御 fallback
/// - `Neutral`: 通常打牌 → 防御 fallback(Reach は抑制)
/// - `Fold`: 防御 fallback → 通常打牌(Reach は抑制)
pub fn decide_push_pull(inputs: &PushPullInputs) -> PushPullDecision {
    // 1. 他家リーチがなければ攻撃評価の有無にかかわらず押す。
    if inputs.opponent_reach_count == 0 {
        return PushPullDecision {
            mode: PushPullMode::Push,
            reason: PushPullReason::NoOpponentReach,
        };
    }

    // 2. 攻撃評価が無ければ、情報不足を理由に強制 Fold にはせず Neutral に留める。
    let Some(offense) = inputs.offense else {
        return PushPullDecision {
            mode: PushPullMode::Neutral,
            reason: PushPullReason::MissingOffenseEvaluation,
        };
    };

    let single_non_dealer = inputs.opponent_reach_count == 1 && !inputs.dealer_reacher;

    // 3. テンパイ相当(向聴 <= 0)。
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

    // 4. 一向聴。単独の子リーチかつ受け入れが暫定 threshold 以上の場合だけ強い一向聴。
    if offense.min_shanten_after_discard == 1 {
        // 4-1. 既存の強い一向聴 threshold。形にかかわらず従来どおり。
        let strong = single_non_dealer
            && offense.acceptance_total_remaining >= STRONG_IISHANTEN_MIN_REMAINING
            && offense.acceptance_type_count >= STRONG_IISHANTEN_MIN_TYPES;
        if strong {
            return PushPullDecision {
                mode: PushPullMode::Neutral,
                reason: PushPullReason::StrongIishantenAgainstSingleNonDealer,
            };
        }

        // 4-2. 強い一向聴 threshold には届かないが、形が Complete で限定 threshold を満たす場合だけ Neutral。
        let complete = single_non_dealer
            && offense.standard_iishanten_shape_after_discard == IishantenShape::Complete
            && offense.acceptance_total_remaining >= COMPLETE_IISHANTEN_MIN_REMAINING
            && offense.acceptance_type_count >= COMPLETE_IISHANTEN_MIN_TYPES;
        if complete {
            return PushPullDecision {
                mode: PushPullMode::Neutral,
                reason: PushPullReason::CompleteIishantenAgainstSingleNonDealer,
            };
        }

        // 4-3. 自分が親のときだけ、形を限定せずに限定 threshold を満たす場合だけ Neutral。
        let dealer = single_non_dealer
            && inputs.self_dealer
            && offense.acceptance_total_remaining >= DEALER_IISHANTEN_MIN_REMAINING
            && offense.acceptance_type_count >= DEALER_IISHANTEN_MIN_TYPES;
        if dealer {
            return PushPullDecision {
                mode: PushPullMode::Neutral,
                reason: PushPullReason::DealerIishantenAgainstSingleNonDealer,
            };
        }

        return PushPullDecision {
            mode: PushPullMode::Fold,
            reason: PushPullReason::IishantenUnderHighPressure,
        };
    }

    // 5. 二向聴以上。
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
        offense_min_shanten_after_discard = ?inputs.offense.map(|offense| offense.min_shanten_after_discard),
        offense_acceptance_total_remaining = ?inputs.offense.map(|offense| offense.acceptance_total_remaining),
        offense_acceptance_type_count = ?inputs.offense.map(|offense| offense.acceptance_type_count),
        offense_iishanten_shape_after_discard = ?inputs.offense.map(|offense| offense.standard_iishanten_shape_after_discard),
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
        PushPullOffenseState {
            min_shanten_after_discard: shanten,
            acceptance_total_remaining: remaining,
            acceptance_type_count: types,
            standard_iishanten_shape_after_discard: shape,
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
        PushPullInputs {
            opponent_reach_count,
            dealer_reacher,
            self_dealer,
            offense,
        }
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
        use crate::discard_selection::select_best_discard_evaluation;

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
        let evaluation = select_best_discard_evaluation(&context, &tiles);

        let shared = push_pull_inputs_from_context_with_evaluation(&context, evaluation.as_ref());
        let public = push_pull_inputs_from_context(&context);
        assert_eq!(shared, public);
        assert!(shared.offense.is_some());
    }

    #[test]
    fn with_evaluation_transcribes_iishanten_shape() {
        use crate::discard_selection::select_best_discard_evaluation;

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
        let mut evaluation =
            select_best_discard_evaluation(&context, &tiles).expect("evaluation should exist");

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
        use crate::discard_selection::select_best_discard_evaluation;

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
        let evaluation = select_best_discard_evaluation(&context, &tiles);
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
}
