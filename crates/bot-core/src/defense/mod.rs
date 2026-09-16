mod compressed_hidden_hand_states;
mod diagnostic;
mod hard_safety;
mod hidden_hand_states;
mod honor;
mod ron_risk;
mod suited;
mod suji;
mod wait_candidates;
mod wall;

#[cfg(test)]
mod tests;

use crate::action::{DahaiCandidateOrdering, LegalAction, prefer_black_five_for_action};
use crate::context::GameContext;
use bot_logic::TileType;

pub use compressed_hidden_hand_states::{
    CompressedHiddenHandStateMetrics, CompressedHiddenHandStates,
    CompressedStructuralTenpaiHiddenHandStates, RonRiskEvidence, TenpaiStateWeight,
    compressed_ron_capable_hidden_hand_weight, compressed_tenpai_hidden_hand_weight,
};
pub(crate) use diagnostic::log_defense_fallback_evaluation;
pub use diagnostic::{
    DefenseCandidateDiagnostic, DefenseDecisionDiagnostic, DefenseFallbackDiagnostic,
    log_defense_fallback_decision,
};
pub use hard_safety::{
    genbutsu_dahai_actions_for_all_reached, is_discarded_by_all_players, is_discarded_by_player,
    is_genbutsu_for, is_genbutsu_for_all_reached, select_genbutsu_fallback_action,
};
pub use hidden_hand_states::{
    HiddenHandStateMetrics, HiddenHandStateUnsupported, ReachedHiddenHandStates,
    RonCapableStateWeight, StructuralCompletionStateWeight, StructuralTenpaiHiddenHandStates,
    ron_capable_hidden_hand_weight,
};
pub use honor::{
    HonorSafetyRank, OpponentHonorValue, honor_dahai_actions_by_safety,
    honor_dahai_actions_by_safety_with, honor_safety_rank, opponent_honor_value_for,
    opponent_honor_value_for_players, opponent_honor_value_for_reached,
    select_honor_safety_fallback_action,
};
pub(crate) use ron_risk::{
    DahaiRonRiskVector, combined_targets_dahai_actions_by_ron_risk,
    open_hand_targets_dahai_actions_by_ron_risk, player_ron_risk_evidence_for_action,
    reached_opponents_dahai_actions_by_ron_risk,
};
pub use ron_risk::{
    PlayerRonRiskEvidence, compare_lexicographic_minimax_ron_risk, worst_first_ron_risk_evidence,
};
pub use suited::{
    SuitedSafetyEvidence, SuitedSafetyRank, select_suited_safety_fallback_action,
    suited_dahai_actions_by_safety, suited_dahai_actions_by_safety_with,
    suited_safety_evidence_for_all_reached, suited_safety_evidence_for_any_reached,
    suited_safety_evidence_for_players, suited_safety_outweighs_honor,
    suited_safety_rank_for_all_reached, suited_safety_rank_for_any_reached,
    suited_safety_rank_for_players,
};
pub use suji::{
    SujiSafetyRank, is_suji_for, is_suji_for_all_reached, is_suji_for_any_reached,
    suji_dahai_actions_by_safety, suji_safety_rank_for, suji_safety_rank_for_all_reached,
    suji_safety_rank_for_any_reached, suji_safety_rank_for_players,
};
pub use wait_candidates::{
    remaining_tile_copies, shanpon_remaining_combinations,
    shanpon_remaining_combinations_for_player, tanki_remaining_candidates,
    tanki_remaining_candidates_for_player,
};
pub use wall::{
    SequenceWaitRoute, SequenceWaitShape, WallRank, is_no_chance, is_one_chance,
    sequence_route_remaining_combinations, sequence_route_remaining_combinations_for_player,
    sequence_wait_routes, wall_rank, wall_tile_types_by_rank,
};

// visible_tiles 中で同じ TileType の枚数を数える。赤5も通常5と同じ TileType として数える。
pub fn visible_count_of(tile: TileType, context: &GameContext) -> u8 {
    context
        .visible_tiles()
        .iter()
        .filter(|visible| visible.tile_type() == tile)
        .count() as u8
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DefenseFallbackKind {
    Genbutsu,
    ExactRonRisk,
    HonorSafety(HonorSafetyRank),
    SuitedSafety(SuitedSafetyRank),
}

#[derive(Debug)]
pub(crate) struct DefenseFallbackEvaluation<'a> {
    pub(crate) selected: Option<(&'a LegalAction, DefenseFallbackKind)>,
    pub(crate) ron_risk_vectors: Option<Vec<DahaiRonRiskVector<'a>>>,
}

// 他家リーチ中の防御 fallback を優先順位付きで選ぶ。
// 全リーチ者への共通現物を最優先にし、それ以外は各リーチ者の exact R/T を worst-first に
// 並べた risk vector の辞書順で比較する。1人でも exact model が unavailable なら局面全体を
// 従来の字牌 / 数牌 safety へ戻し、選ばれた種別を添えて返す。
//
// 現物は黒5対応済みの select_genbutsu_fallback_action をそのまま利用する。exact / legacy
// いずれも牌種を決めてから prefer_black_five_for_action で同じ5牌種の黒牌へ正規化する。
pub fn select_defense_fallback_action_with_kind<'a>(
    context: &GameContext,
    legal_actions: &'a [LegalAction],
) -> Option<(&'a LegalAction, DefenseFallbackKind)> {
    evaluate_defense_fallback_action_with_kind(context, legal_actions, false).selected
}

pub(crate) fn evaluate_defense_fallback_action_with_kind<'a>(
    context: &GameContext,
    legal_actions: &'a [LegalAction],
    collect_exact_evidence_for_genbutsu: bool,
) -> DefenseFallbackEvaluation<'a> {
    let genbutsu = select_genbutsu_fallback_action(context, legal_actions);
    if let Some(action) = genbutsu
        && !collect_exact_evidence_for_genbutsu
    {
        return DefenseFallbackEvaluation {
            selected: Some((action, DefenseFallbackKind::Genbutsu)),
            ron_risk_vectors: None,
        };
    }

    let ron_risk_vectors = reached_opponents_dahai_actions_by_ron_risk(context, legal_actions);

    if let Some(action) = genbutsu {
        return DefenseFallbackEvaluation {
            selected: Some((action, DefenseFallbackKind::Genbutsu)),
            ron_risk_vectors,
        };
    }

    if let Some(vectors) = ron_risk_vectors.as_ref() {
        match select_lexicographic_minimax_action(vectors) {
            Ok(Some(chosen)) => {
                let action = prefer_black_five_for_action(legal_actions, chosen);
                return DefenseFallbackEvaluation {
                    selected: Some((action, DefenseFallbackKind::ExactRonRisk)),
                    ron_risk_vectors,
                };
            }
            Ok(None) => {}
            Err(()) => {
                return DefenseFallbackEvaluation {
                    selected: select_legacy_defense_fallback_action_with_kind(
                        context,
                        legal_actions,
                    ),
                    ron_risk_vectors: None,
                };
            }
        }
    }

    DefenseFallbackEvaluation {
        selected: select_legacy_defense_fallback_action_with_kind(context, legal_actions),
        ron_risk_vectors: None,
    }
}

pub(crate) fn select_lexicographic_minimax_action<'a>(
    vectors: &[DahaiRonRiskVector<'a>],
) -> Result<Option<&'a LegalAction>, ()> {
    Ok(sort_by_lexicographic_minimax(vectors)?
        .first()
        .map(|vector| vector.action))
}

/// risk vector 全件を worst-first lexicographic minimax の安全な順に並べる。
///
/// 比較は [`compare_lexicographic_minimax_ron_risk`] だけを使い、同値は元順序を保つ。1件でも
/// 比較不能なら `Err(())` で、選択と ranking を同じ comparator で揃えるためどちらも exact を
/// 使わない判断へ倒す。
pub(crate) fn sort_by_lexicographic_minimax<'b, 'a>(
    vectors: &'b [DahaiRonRiskVector<'a>],
) -> Result<Vec<&'b DahaiRonRiskVector<'a>>, ()> {
    let mut sorted: Vec<_> = vectors.iter().collect();
    // 合法 Dahai は最大14件で、比較不能を潰さずに伝播させたいので fallible な挿入 sort にする。
    for index in 1..sorted.len() {
        let mut current = index;
        while current > 0 {
            let ordering = compare_lexicographic_minimax_ron_risk(
                &sorted[current].player_evidence,
                &sorted[current - 1].player_evidence,
            )
            .ok_or(())?;
            if ordering != std::cmp::Ordering::Less {
                break;
            }
            sorted.swap(current, current - 1);
            current -= 1;
        }
    }
    Ok(sorted)
}

/// リーチ者向け防御候補を production の優先順位どおりに並べる。
///
/// 段の順序と各段の並べ方は [`evaluate_defense_fallback_action_with_kind`] と同じ既存 helper を
/// 使い、forced fold 用の comparator を持たない。`ron_risk_vectors` には production evaluation が
/// 実際に構築した exact evidence をそのまま渡す。`None` の場合、または exact 比較が1件でも
/// 不能な場合は production と同じく従来 heuristic の順序へ落ちる。
///
/// 選択対象にならない [`SuitedSafetyRank::NoSafety`] の数牌も、順位を持つ候補として末尾に残す。
pub(crate) fn ordered_defense_fallback_candidates<'a>(
    context: &GameContext,
    legal_actions: &'a [LegalAction],
    ron_risk_vectors: Option<&[DahaiRonRiskVector<'a>]>,
) -> Vec<(&'a LegalAction, DefenseFallbackKind)> {
    let mut ordering = DahaiCandidateOrdering::new(legal_actions);
    for action in genbutsu_dahai_actions_for_all_reached(legal_actions, context) {
        ordering.push(action, DefenseFallbackKind::Genbutsu);
    }

    match ron_risk_vectors.map(sort_by_lexicographic_minimax) {
        Some(Ok(sorted)) => {
            for vector in sorted {
                ordering.push(vector.action, DefenseFallbackKind::ExactRonRisk);
            }
        }
        Some(Err(())) | None => {
            extend_with_legacy_defense_fallback_candidates(&mut ordering, context, legal_actions);
        }
    }

    ordering.into_ordered()
}

/// production selector が防御 fallback として採用し得る種別か。
///
/// 既存 selector は数牌 safety の [`SuitedSafetyRank::NoSafety`] を採用しないので、ranking では
/// 末尾の候補として残したうえで選択対象から外す。
fn is_selectable_defense_fallback_kind(kind: DefenseFallbackKind) -> bool {
    kind != DefenseFallbackKind::SuitedSafety(SuitedSafetyRank::NoSafety)
}

// 1人でも exact model が unavailable な場合の従来 selection。
//
// 順序は ranking と共有する extend_with_legacy_defense_fallback_candidates で作り、そこから
// 既存 selector が採用し得る先頭候補を取る。
fn select_legacy_defense_fallback_action_with_kind<'a>(
    context: &GameContext,
    legal_actions: &'a [LegalAction],
) -> Option<(&'a LegalAction, DefenseFallbackKind)> {
    let mut ordering = DahaiCandidateOrdering::new(legal_actions);
    extend_with_legacy_defense_fallback_candidates(&mut ordering, context, legal_actions);
    ordering
        .into_ordered()
        .into_iter()
        .find(|&(_, kind)| is_selectable_defense_fallback_kind(kind))
}

// exact model が unavailable な場合の従来 heuristic 順序。
//
// 字牌順は honor_dahai_actions_by_safety、数牌順は suited_dahai_actions_by_safety そのままで、
// 両者の横断比較も既存の suited_safety_outweighs_honor だけを使う。両列の先頭同士に同じ限定的
// 比較を繰り返し適用するので、先頭候補は既存 selection と一致する。NoSafety の数牌は既存
// selector の対象外なので、順位だけ与えて末尾へ置く。
fn extend_with_legacy_defense_fallback_candidates<'a>(
    ordering: &mut DahaiCandidateOrdering<'a, DefenseFallbackKind>,
    context: &GameContext,
    legal_actions: &'a [LegalAction],
) {
    if !context.any_opponent_reached() {
        return;
    }

    let mut honor = honor_dahai_actions_by_safety(legal_actions, context)
        .into_iter()
        .filter(|&(action, _)| !ordering.contains_tile_type_of(action))
        .collect::<Vec<_>>()
        .into_iter()
        .peekable();
    let suited: Vec<_> = suited_dahai_actions_by_safety(legal_actions, context)
        .into_iter()
        .filter(|&(action, _)| !ordering.contains_tile_type_of(action))
        .collect();
    let mut safe_suited = suited
        .iter()
        .copied()
        .filter(|&(_, rank)| rank != SuitedSafetyRank::NoSafety)
        .peekable();

    loop {
        let outweighed = match (honor.peek(), safe_suited.peek()) {
            (Some(&(honor_action, honor_rank)), Some(&(_, suited_rank))) => {
                let LegalAction::Dahai { tile: honor_tile } = honor_action else {
                    unreachable!("字牌候補は Dahai だけ")
                };
                suited_safety_outweighs_honor(
                    honor_rank,
                    opponent_honor_value_for_reached(honor_tile.tile_type(), context),
                    suited_rank,
                )
            }
            _ => false,
        };

        if outweighed || honor.peek().is_none() {
            match safe_suited.next() {
                Some((action, rank)) => {
                    ordering.push(action, DefenseFallbackKind::SuitedSafety(rank))
                }
                None => break,
            }
        } else {
            let Some((action, rank)) = honor.next() else {
                break;
            };
            ordering.push(action, DefenseFallbackKind::HonorSafety(rank));
        }
    }

    for (action, rank) in suited {
        ordering.push(action, DefenseFallbackKind::SuitedSafety(rank));
    }
}

// 防御 fallback の action だけを返す薄い wrapper。
pub fn select_defense_fallback_action<'a>(
    context: &GameContext,
    legal_actions: &'a [LegalAction],
) -> Option<&'a LegalAction> {
    select_defense_fallback_action_with_kind(context, legal_actions).map(|(action, _)| action)
}
