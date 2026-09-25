//! リーチ者と `Caution` / `Danger`
//! [`OpenHandThreatLevel`](crate::open_hand_threat::OpenHandThreatLevel) の非リーチ相手が同時に
//! いる複合 threat 局面の防御 safety の source of truth。
//!
//! 判定は既存 Defense / OpenHand Defense の pure helper をそのまま共有し、字牌の見え枚数・壁・
//! スジ・役牌価値を別実装しない。違うのは target 集合の作り方と、「その player にロンされない」
//! 根拠が target の種類ごとに変わる点だけ。
//!
//! - [`ThreatDefenseTargetKind::Riichi`]：本人の河と `post_reach_passed_tiles` の両方
//!   ([`is_genbutsu_for`])。
//! - [`ThreatDefenseTargetKind::HighOpenHand`]：本人の河または現在有効な一時通過牌
//!   ([`is_ron_safe_for_open_hand_target`])。`post_reach_passed_tiles` は流用しない。
//! - same-hand passed：`HighOpenHand` target の hard-safe とは区別する独立 evidence。
//!
//! 防御 fallback の action 選択は [`select_combined_threat_defense_fallback_action_with_kind`] が
//! source of truth。hard-safe → same-hand passed → 全 target の exact `R/T` → exact unavailable
//! 時だけ legacy safety の順で選び、[`CombinedDefenseDiagnostic`] は同じ evaluation の結果と
//! evidence を写すだけにする。

use crate::action::{DahaiCandidateOrdering, LegalAction, prefer_black_five_for_action};
use crate::context::GameContext;
use crate::defense::{
    DahaiRonRiskVector, HonorSafetyRank, OpponentHonorValue, PlayerRonRiskEvidence,
    SuitedSafetyEvidence, SuitedSafetyRank, SujiSafetyRank, WallRank,
    combined_targets_dahai_actions_by_ron_risk, honor_dahai_actions_by_safety_with,
    honor_safety_rank, is_genbutsu_for, opponent_honor_value_for_players,
    player_ron_risk_evidence_for_action, select_lexicographic_minimax_action,
    sort_by_lexicographic_minimax, suited_dahai_actions_by_safety_with,
    suited_safety_evidence_for_players, suji_safety_rank_for, suji_safety_rank_for_players,
};
use crate::open_hand_defense::{
    actionable_open_hand_threat_players, is_ron_safe_for_open_hand_target,
};
use crate::open_hand_threat::{OpenHandThreatAssessment, classify_open_hand_threats};
use crate::threat::{PlayerThreatFacts, player_threat_facts_from_context};
use bot_logic::TileType;

/// 防御 target の種類。「その player にロンされない」根拠がこの種類で変わる。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThreatDefenseTargetKind {
    /// 他家リーチ者。現物 ([`is_genbutsu_for`]) がロン安全の根拠。
    Riichi,
    /// `Caution` / `Danger` の非リーチ相手。本人の河または現在有効な一時通過牌がロン安全の根拠。
    ///
    /// 診断表示に出る名前なので、level 分割後も従来の名前のまま両方の level に使う。
    HighOpenHand,
}

/// 防御 target 1人分。席と種類だけを持つ軽量な値。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ThreatDefenseTarget {
    pub player: usize,
    pub kind: ThreatDefenseTargetKind,
}

impl ThreatDefenseTarget {
    pub fn riichi(player: usize) -> Self {
        Self {
            player,
            kind: ThreatDefenseTargetKind::Riichi,
        }
    }

    pub fn high_open_hand(player: usize) -> Self {
        Self {
            player,
            kind: ThreatDefenseTargetKind::HighOpenHand,
        }
    }
}

/// 複合 threat 局面の防御 target を席順で集める pure helper。
///
/// リーチ者は [`PlayerThreatFacts::is_reached_opponent`]、`Caution` / `Danger` の副露相手は渡された
/// classification ([`actionable_open_hand_threat_players`]) をそのまま source of truth にする。どちらも
/// ここで分類し直さない。リーチ済みの席は OpenHandThreat の対象外なので、1つの席が両方の target
/// になることはない。
///
/// 戻り値が空でないのは「リーチ者が1人以上」かつ「`Caution` / `Danger` の副露相手が1人以上」の複合 threat
/// 局面だけ。どちらか一方しかいない局面は既存の Riichi Defense / OpenHand Defense が担当するため、
/// ここでは target を作らない。
pub fn combined_threat_defense_targets(
    player_threats: &[PlayerThreatFacts; 4],
    assessments: &[OpenHandThreatAssessment; 4],
) -> Vec<ThreatDefenseTarget> {
    let targets = threat_defense_targets(player_threats, assessments);
    let has_riichi = targets
        .iter()
        .any(|target| target.kind == ThreatDefenseTargetKind::Riichi);
    let has_open_hand = targets
        .iter()
        .any(|target| target.kind == ThreatDefenseTargetKind::HighOpenHand);
    if has_riichi && has_open_hand {
        targets
    } else {
        Vec::new()
    }
}

/// 現在の threat target を種類付きで席順に集める pure helper。
///
/// リーチ者は [`PlayerThreatFacts::is_reached_opponent`]、`Caution` / `Danger` の非リーチ相手は渡された
/// classification ([`actionable_open_hand_threat_players`]) をそのまま source of truth にする。どちらも
/// ここで分類し直さない。したがって [`ThreatDefenseTargetKind::HighOpenHand`] の target は公開副露
/// がある相手に限らず、暗槓だけで `Caution` / `Danger` になった相手も含む。リーチ済みの席は OpenHandThreat の
/// 対象外なので、1つの席が両方の target になることはない。
///
/// [`combined_threat_defense_targets`] と違い、リーチ者だけ・`Caution` / `Danger` の非リーチ相手だけの局面でも
/// その target を返す。target ごとの hard-safe 判定 ([`is_ron_safe_for_target`] /
/// [`is_safe_against_all_threats`]) を threat 構成によらず1つの概念として共有したい呼び出し元の
/// ための入口で、防御 fallback の action 選択そのものは従来どおり threat 構成ごとの入口が担当する。
///
/// 明確な threat がいない局面では空になる。
pub fn threat_defense_targets(
    player_threats: &[PlayerThreatFacts; 4],
    assessments: &[OpenHandThreatAssessment; 4],
) -> Vec<ThreatDefenseTarget> {
    let mut targets: Vec<ThreatDefenseTarget> = player_threats
        .iter()
        .filter(|facts| facts.is_reached_opponent())
        .map(|facts| ThreatDefenseTarget::riichi(facts.player))
        .chain(
            actionable_open_hand_threat_players(assessments)
                .into_iter()
                .map(ThreatDefenseTarget::high_open_hand),
        )
        .collect();
    targets.sort_by_key(|target| target.player);
    targets
}

/// 全4席分の facts から target を集める adapter。分類は [`classify_open_hand_threats`] が行う。
pub fn combined_threat_defense_targets_from_facts(
    facts: &[PlayerThreatFacts; 4],
) -> Vec<ThreatDefenseTarget> {
    combined_threat_defense_targets(facts, &classify_open_hand_threats(facts))
}

/// `GameContext` から target を集める adapter。facts の構築も分類も既存経路を共有する。
pub fn combined_threat_defense_targets_from_context(
    context: &GameContext,
) -> Vec<ThreatDefenseTarget> {
    combined_threat_defense_targets_from_facts(&player_threat_facts_from_context(context))
}

/// その target にこの牌でロンされないと言えるか判定する pure helper。
///
/// 根拠は target の種類ごとに変わる。リーチ者は現物 ([`is_genbutsu_for`]) で、本人の河と
/// `post_reach_passed_tiles` の両方を使う。`Caution` / `Danger` の副露相手は本人の河または現在有効な一時通過牌
/// ([`is_ron_safe_for_open_hand_target`]) を使い、`post_reach_passed_tiles` は使わない。
///
/// この判定を selector や診断へ散らさず、target 種類の分岐はここに1つだけ置く。
pub fn is_ron_safe_for_target(
    tile: TileType,
    target: ThreatDefenseTarget,
    context: &GameContext,
) -> bool {
    match target.kind {
        ThreatDefenseTargetKind::Riichi => is_genbutsu_for(tile, target.player, context),
        ThreatDefenseTargetKind::HighOpenHand => {
            is_ron_safe_for_open_hand_target(tile, target.player, context)
        }
    }
}

/// target に対する same-hand passed evidence。
///
/// `HighOpenHand` にだけ適用し、hard-safe の [`is_ron_safe_for_target`] には含めない。
pub fn is_same_hand_passed_for_target(
    tile: TileType,
    target: ThreatDefenseTarget,
    context: &GameContext,
) -> bool {
    target.kind == ThreatDefenseTargetKind::HighOpenHand
        && context.is_same_hand_passed(tile, target.player)
}

/// 全 target に対してロン安全か判定する。target が0人なら `false`。
///
/// リーチ者の現物と副露相手本人の河が混ざった集合なので、リーチ者向けの現物
/// ([`is_genbutsu_for_all_reached`](crate::defense::is_genbutsu_for_all_reached)) とは別物。
pub fn is_safe_against_all_threats(
    tile: TileType,
    targets: &[ThreatDefenseTarget],
    context: &GameContext,
) -> bool {
    !targets.is_empty()
        && targets
            .iter()
            .all(|&target| is_ron_safe_for_target(tile, target, context))
}

// Wall / OneChance / Suji / Honor の heuristic で評価する target の席。
//
// hard-safe の target と same-hand passed evidence がある target は、それより弱い heuristic の
// 集約から除外する。
//
// 全 target が hard-safe または same-hand passed の場合は空になる。空集合は「hard-safe」と
// いう意味ではなく、強い根拠は [`CombinedDefenseCategory`] の別 category が表す。
fn heuristic_target_players(
    tile: TileType,
    targets: &[ThreatDefenseTarget],
    context: &GameContext,
) -> Vec<usize> {
    targets
        .iter()
        .filter(|&&target| {
            !is_ron_safe_for_target(tile, target, context)
                && !is_same_hand_passed_for_target(tile, target, context)
        })
        .map(|target| target.player)
        .collect()
}

/// 全 target が hard-safe または same-hand passed で覆われ、少なくとも1人には
/// same-hand passed が必要かを判定する。target が0人なら `false`。
pub fn has_same_hand_passed_for_all_threats(
    tile: TileType,
    targets: &[ThreatDefenseTarget],
    context: &GameContext,
) -> bool {
    !targets.is_empty()
        && targets.iter().all(|&target| {
            is_ron_safe_for_target(tile, target, context)
                || is_same_hand_passed_for_target(tile, target, context)
        })
        && targets.iter().any(|&target| {
            !is_ron_safe_for_target(tile, target, context)
                && is_same_hand_passed_for_target(tile, target, context)
        })
}

/// target に対する役牌価値のうち最も危険な評価。数牌は対象外で `None`。
///
/// heuristic で評価する target ([`heuristic_target_players`]) だけを集約する。target がいない場合、
/// 全 target がロン不能な場合、情報不足で誰の分も確定できない場合は `None` (unknown)。情報不足を
/// `GuestWind` と推測しない。
///
/// 集約も player ごとの評価も既存 Defense の [`opponent_honor_value_for_players`] と共有する。
pub fn opponent_honor_value_for_combined_threats(
    tile: TileType,
    targets: &[ThreatDefenseTarget],
    context: &GameContext,
) -> Option<OpponentHonorValue> {
    opponent_honor_value_for_players(
        tile,
        &heuristic_target_players(tile, targets, context),
        context,
    )
}

/// target の河に対するスジ安全度。数牌なら `Some`、字牌なら `None`。
///
/// まだロンされ得る target だけを集約し、各 player の [`suji_safety_rank_for`] の最小値
/// (最も危険な評価) を採る。target が0人の場合と全 target がロン不能な場合は `NoSuji` で、
/// スジがあるとは扱わない。判定も集約も既存 Defense の [`suji_safety_rank_for_players`] と共有する。
pub fn suji_safety_rank_for_combined_threats(
    tile: TileType,
    targets: &[ThreatDefenseTarget],
    context: &GameContext,
) -> Option<SujiSafetyRank> {
    suji_safety_rank_for_players(
        tile,
        &heuristic_target_players(tile, targets, context),
        context,
    )
}

/// target に対する数牌の防御 evidence。字牌は対象外で `None`。
///
/// 壁とスジをここで組み立てず、heuristic で評価する target ([`heuristic_target_players`]) を
/// 決めて既存 Defense の [`suited_safety_evidence_for_players`] へ渡すだけにする。evidence の
/// 意味はリーチ / OpenHand と同じ。
pub fn suited_safety_evidence_for_combined_threats(
    tile: TileType,
    targets: &[ThreatDefenseTarget],
    context: &GameContext,
) -> Option<SuitedSafetyEvidence> {
    suited_safety_evidence_for_players(
        tile,
        &heuristic_target_players(tile, targets, context),
        context,
    )
}

/// target に対する数牌の安全度を壁 / スジから分類する。字牌は対象外で `None`。
///
/// [`suited_safety_evidence_for_combined_threats`] の evidence を
/// [`SuitedSafetyEvidence::legacy_rank`] で潰す薄い wrapper。
pub fn suited_safety_rank_for_combined_threats(
    tile: TileType,
    targets: &[ThreatDefenseTarget],
    context: &GameContext,
) -> Option<SuitedSafetyRank> {
    suited_safety_evidence_for_combined_threats(tile, targets, context)
        .map(SuitedSafetyEvidence::legacy_rank)
}

/// 複合 threat に対する防御候補の大分類。
///
/// 優先順位は `SafeAgainstAllThreats` → `SameHandPassed` → `ExactRonRisk` → `HonorSafety` →
/// `SuitedSafety`。exact が1 target でも unavailable の場合だけ legacy safety を使う。
///
/// 第一分類を `Genbutsu` と呼ばないのは、リーチ者の現物と、副露相手本人の河または現在有効な
/// 一時通過牌という根拠の違う安全牌が混ざった集合だから。既存の
/// [`DefenseFallbackKind::Genbutsu`](crate::defense::DefenseFallbackKind)
/// や [`OpenHandDefenseCategory::SafeAgainstAllTargets`](crate::open_hand_defense::OpenHandDefenseCategory)
/// へは押し込まない。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CombinedDefenseCategory {
    SafeAgainstAllThreats,
    /// 全 target が hard-safe または same-hand passed で覆われる。
    SameHandPassed,
    /// 全 target の exact `R/T` を worst-first lexicographic minimax で比較する。
    ExactRonRisk,
    HonorSafety(HonorSafetyRank),
    SuitedSafety(SuitedSafetyRank),
}

/// 牌1つ分の hard-safe / same-hand passed / legacy safety の大分類を求める pure helper。
///
/// exact model は評価しないため `ExactRonRisk` は返さない。production の exact category と
/// evidence は [`evaluate_combined_threat_defense_fallback_action_with_kind`] が決める。
pub fn combined_defense_category(
    tile: TileType,
    targets: &[ThreatDefenseTarget],
    context: &GameContext,
) -> Option<CombinedDefenseCategory> {
    if is_safe_against_all_threats(tile, targets, context) {
        return Some(CombinedDefenseCategory::SafeAgainstAllThreats);
    }
    if has_same_hand_passed_for_all_threats(tile, targets, context) {
        return Some(CombinedDefenseCategory::SameHandPassed);
    }
    if let Some(rank) = honor_safety_rank(tile, context) {
        return Some(CombinedDefenseCategory::HonorSafety(rank));
    }
    suited_safety_rank_for_combined_threats(tile, targets, context)
        .map(CombinedDefenseCategory::SuitedSafety)
}

/// 合法 Dahai のうち、全 target に対してロン安全な牌を元の順序を保って抽出する。
///
/// 根拠は [`is_safe_against_all_threats`] だけで、target が0人なら空。
pub fn safe_against_all_threats_dahai_actions<'a>(
    legal_actions: &'a [LegalAction],
    targets: &[ThreatDefenseTarget],
    context: &GameContext,
) -> Vec<&'a LegalAction> {
    legal_actions
        .iter()
        .filter(|action| match action {
            LegalAction::Dahai { tile } => {
                is_safe_against_all_threats(tile.tile_type(), targets, context)
            }
            _ => false,
        })
        .collect()
}

/// 合法 Dahai のうち、全 target が hard-safe または same-hand passed で覆われる牌を、
/// hard-safe な target 数の多い順に並べる。
///
/// hard-safe 数が同じ候補は、合法 Dahai の元順序を維持する。
pub fn same_hand_passed_combined_dahai_actions<'a>(
    legal_actions: &'a [LegalAction],
    targets: &[ThreatDefenseTarget],
    context: &GameContext,
) -> Vec<&'a LegalAction> {
    let mut actions: Vec<&LegalAction> = legal_actions
        .iter()
        .filter(|action| match action {
            LegalAction::Dahai { tile } => {
                has_same_hand_passed_for_all_threats(tile.tile_type(), targets, context)
            }
            _ => false,
        })
        .collect();
    actions.sort_by_key(|action| {
        let LegalAction::Dahai { tile } = action else {
            unreachable!("filtered to Dahai actions")
        };
        std::cmp::Reverse(
            targets
                .iter()
                .filter(|&&target| is_ron_safe_for_target(tile.tile_type(), target, context))
                .count(),
        )
    });
    actions
}

/// 合法 Dahai のうち字牌のみを、target に対する安全度順に並べる。
///
/// 並べ替えは既存 Defense の [`honor_dahai_actions_by_safety_with`] と共有し、複合 threat 用の
/// sorting を別に持たない。役牌価値だけを [`opponent_honor_value_for_combined_threats`] へ
/// 差し替える。
pub fn combined_honor_dahai_actions_by_safety<'a>(
    legal_actions: &'a [LegalAction],
    targets: &[ThreatDefenseTarget],
    context: &GameContext,
) -> Vec<(&'a LegalAction, HonorSafetyRank)> {
    honor_dahai_actions_by_safety_with(legal_actions, context, |tile| {
        opponent_honor_value_for_combined_threats(tile, targets, context)
    })
}

/// 合法 Dahai のうち数牌のみを、target に対する安全度順に並べる。
///
/// 並べ替えは既存 Defense の [`suited_dahai_actions_by_safety_with`] と共有し、安全度だけを
/// [`suited_safety_rank_for_combined_threats`] へ差し替える。
pub fn combined_suited_dahai_actions_by_safety<'a>(
    legal_actions: &'a [LegalAction],
    targets: &[ThreatDefenseTarget],
    context: &GameContext,
) -> Vec<(&'a LegalAction, SuitedSafetyRank)> {
    suited_dahai_actions_by_safety_with(legal_actions, |tile| {
        suited_safety_rank_for_combined_threats(tile, targets, context)
    })
}

/// 複合 threat に対する防御 fallback を優先順位付きで選ぶ production selector。
///
/// [`CombinedDefenseCategory`] の並びどおり、全 target へのロン安全 → same-hand passed → exact
/// `R/T` → 字牌 safety → 数牌 safety の順に評価し、選ばれた大分類を添えて返す。target が0人
/// なら `None`。
///
/// - `SafeAgainstAllThreats`: 全 target にロンされない牌。同順位では合法 Dahai の元順序を保つ。
/// - `SameHandPassed`: 全 target が hard-safe または same-hand passed で覆われる牌。hard-safe な
///   target 数が多い候補を優先し、同数なら元順序を保つ。
/// - `ExactRonRisk`: target ごとの `R/T` を risk の高い順に並べた lexicographic minimax。
///   exact tie は元順序を保つ。1 target でも unavailable なら局面全体を legacy へ戻す。
/// - `HonorSafety`: 見え枚数の安全度 → 役牌価値 → 元の順序。既存 Defense と同じ ranking。
/// - `SuitedSafety`: 壁 / スジを統合した安全度順。既存 Defense と同じく
///   [`SuitedSafetyRank::NoSafety`] は fallback として選ばない。
///
/// いずれも牌種を決めたあと、その牌種内では [`prefer_black_five_for_action`] で黒5を優先する。
/// 牌種選択・大分類・安全度 rank は変えず、物理牌だけを黒牌へ正規化する。
pub fn select_combined_threat_defense_fallback_action_with_kind<'a>(
    context: &GameContext,
    legal_actions: &'a [LegalAction],
    targets: &[ThreatDefenseTarget],
) -> Option<(&'a LegalAction, CombinedDefenseCategory)> {
    evaluate_combined_threat_defense_fallback_action_with_kind(context, legal_actions, targets)
        .selected
}

#[derive(Debug)]
pub(crate) struct CombinedDefenseEvaluation<'a> {
    pub(crate) selected: Option<(&'a LegalAction, CombinedDefenseCategory)>,
    pub(crate) ron_risk_vectors: Option<Vec<DahaiRonRiskVector<'a>>>,
}

pub(crate) fn evaluate_combined_threat_defense_fallback_action_with_kind<'a>(
    context: &GameContext,
    legal_actions: &'a [LegalAction],
    targets: &[ThreatDefenseTarget],
) -> CombinedDefenseEvaluation<'a> {
    if targets.is_empty() {
        return CombinedDefenseEvaluation {
            selected: None,
            ron_risk_vectors: None,
        };
    }

    if let Some(action) = safe_against_all_threats_dahai_actions(legal_actions, targets, context)
        .into_iter()
        .next()
    {
        let action = prefer_black_five_for_action(legal_actions, action);
        return CombinedDefenseEvaluation {
            selected: Some((action, CombinedDefenseCategory::SafeAgainstAllThreats)),
            ron_risk_vectors: None,
        };
    }

    if let Some(action) = same_hand_passed_combined_dahai_actions(legal_actions, targets, context)
        .into_iter()
        .next()
    {
        let action = prefer_black_five_for_action(legal_actions, action);
        return CombinedDefenseEvaluation {
            selected: Some((action, CombinedDefenseCategory::SameHandPassed)),
            ron_risk_vectors: None,
        };
    }

    let ron_risk_vectors = combined_targets_ron_risk_vectors(context, legal_actions, targets);
    if let Some(vectors) = ron_risk_vectors.as_ref() {
        match select_lexicographic_minimax_action(vectors) {
            Ok(Some(chosen)) => {
                let action = prefer_black_five_for_action(legal_actions, chosen);
                return CombinedDefenseEvaluation {
                    selected: Some((action, CombinedDefenseCategory::ExactRonRisk)),
                    ron_risk_vectors,
                };
            }
            Ok(None) => {}
            Err(()) => {
                return CombinedDefenseEvaluation {
                    selected: select_legacy_combined_threat_defense_fallback_action_with_kind(
                        context,
                        legal_actions,
                        targets,
                    ),
                    ron_risk_vectors: None,
                };
            }
        }
    }

    CombinedDefenseEvaluation {
        selected: select_legacy_combined_threat_defense_fallback_action_with_kind(
            context,
            legal_actions,
            targets,
        ),
        ron_risk_vectors,
    }
}

// target kind の分類はこの module の責務。ron-risk 層へは席一覧だけを渡し、その一覧ごとに
// 対応する既存 evaluator を使う。production selection と診断用の追加収集がこの1実装を共有する。
fn combined_targets_ron_risk_vectors<'a>(
    context: &GameContext,
    legal_actions: &'a [LegalAction],
    targets: &[ThreatDefenseTarget],
) -> Option<Vec<DahaiRonRiskVector<'a>>> {
    let riichi_targets: Vec<_> = targets
        .iter()
        .filter(|target| target.kind == ThreatDefenseTargetKind::Riichi)
        .map(|target| target.player)
        .collect();
    let open_hand_targets: Vec<_> = targets
        .iter()
        .filter(|target| target.kind == ThreatDefenseTargetKind::HighOpenHand)
        .map(|target| target.player)
        .collect();
    combined_targets_dahai_actions_by_ron_risk(
        context,
        legal_actions,
        &riichi_targets,
        &open_hand_targets,
    )
}

/// production selection を変えずに、診断用の candidate exact evidence まで揃える。
///
/// [`evaluate_combined_threat_defense_fallback_action_with_kind`] は hard-safe / same-hand passed
/// で selection が確定すると exact model を走らせず `ron_risk_vectors` を `None` のまま早期
/// return する。その場合だけ、選択後に同じ exact model を診断用として追加収集する。選択打牌・
/// category precedence・段の順序・早期 return の性能特性はいずれも変えない。
///
/// exact 比較まで進んだ局面では再収集しない。production が exact を試して unavailable だった
/// ことと、hard-safe で早期 return して exact を試していないことを混同しない。
pub(crate) fn collect_combined_candidate_ron_risk_evidence<'a>(
    context: &GameContext,
    legal_actions: &'a [LegalAction],
    targets: &[ThreatDefenseTarget],
    evaluation: &mut CombinedDefenseEvaluation<'a>,
) {
    if !settled_before_the_exact_comparison(evaluation.selected) {
        return;
    }
    evaluation.ron_risk_vectors =
        combined_targets_ron_risk_vectors(context, legal_actions, targets);
}

// exact 比較より前の段で selection が確定したか。この場合だけ既存 evaluator は exact model を
// 走らせていない。
fn settled_before_the_exact_comparison(
    selected: Option<(&LegalAction, CombinedDefenseCategory)>,
) -> bool {
    matches!(
        selected,
        Some((
            _,
            CombinedDefenseCategory::SafeAgainstAllThreats
                | CombinedDefenseCategory::SameHandPassed
        ))
    )
}

// 順序は ranking と共有する extend_with_legacy_combined_defense_candidates で作り、そこから
// 既存 selector が採用し得る先頭候補を取る。
fn select_legacy_combined_threat_defense_fallback_action_with_kind<'a>(
    context: &GameContext,
    legal_actions: &'a [LegalAction],
    targets: &[ThreatDefenseTarget],
) -> Option<(&'a LegalAction, CombinedDefenseCategory)> {
    let mut ordering = DahaiCandidateOrdering::new(legal_actions);
    extend_with_legacy_combined_defense_candidates(&mut ordering, context, legal_actions, targets);
    ordering
        .into_ordered()
        .into_iter()
        .find(|&(_, category)| is_selectable_combined_defense_category(category))
}

// exact model が unavailable な場合の従来 heuristic 順序。既存 selector と同じく字牌を数牌より
// 先に置き、各列の並べ方も既存 helper そのままにする。NoSafety の数牌は既存 selector の対象外
// なので、順位だけ与えて末尾へ置く。
fn extend_with_legacy_combined_defense_candidates<'a>(
    ordering: &mut DahaiCandidateOrdering<'a, CombinedDefenseCategory>,
    context: &GameContext,
    legal_actions: &'a [LegalAction],
    targets: &[ThreatDefenseTarget],
) {
    for (action, rank) in combined_honor_dahai_actions_by_safety(legal_actions, targets, context) {
        ordering.push(action, CombinedDefenseCategory::HonorSafety(rank));
    }

    let suited = combined_suited_dahai_actions_by_safety(legal_actions, targets, context);
    for &(action, rank) in suited
        .iter()
        .filter(|&&(_, rank)| rank != SuitedSafetyRank::NoSafety)
    {
        ordering.push(action, CombinedDefenseCategory::SuitedSafety(rank));
    }
    for (action, rank) in suited {
        ordering.push(action, CombinedDefenseCategory::SuitedSafety(rank));
    }
}

/// production selector が防御 fallback として採用し得る大分類か。
///
/// 既存 selector は数牌 safety の [`SuitedSafetyRank::NoSafety`] を採用しないので、ranking では
/// 末尾の候補として残したうえで選択対象から外す。
fn is_selectable_combined_defense_category(category: CombinedDefenseCategory) -> bool {
    category != CombinedDefenseCategory::SuitedSafety(SuitedSafetyRank::NoSafety)
}

/// 複合 threat 防御候補を production の優先順位どおりに並べる。
///
/// 段の順序と各段の並べ方は [`evaluate_combined_threat_defense_fallback_action_with_kind`] と同じ
/// 既存 helper を使い、forced fold 用の comparator を持たない。`ron_risk_vectors` には production
/// evaluation が実際に構築した exact evidence をそのまま渡す。`None` の場合、または exact 比較が
/// 1件でも不能な場合は production と同じく従来 heuristic の順序へ落ちる。
pub(crate) fn ordered_combined_defense_candidates<'a>(
    context: &GameContext,
    legal_actions: &'a [LegalAction],
    targets: &[ThreatDefenseTarget],
    ron_risk_vectors: Option<&[DahaiRonRiskVector<'a>]>,
) -> Vec<(&'a LegalAction, CombinedDefenseCategory)> {
    let mut ordering = DahaiCandidateOrdering::new(legal_actions);
    if targets.is_empty() {
        return ordering.into_ordered();
    }

    for action in safe_against_all_threats_dahai_actions(legal_actions, targets, context) {
        ordering.push(action, CombinedDefenseCategory::SafeAgainstAllThreats);
    }
    for action in same_hand_passed_combined_dahai_actions(legal_actions, targets, context) {
        ordering.push(action, CombinedDefenseCategory::SameHandPassed);
    }

    match ron_risk_vectors.map(sort_by_lexicographic_minimax) {
        Some(Ok(sorted)) => {
            for vector in sorted {
                ordering.push(vector.action, CombinedDefenseCategory::ExactRonRisk);
            }
        }
        Some(Err(())) | None => extend_with_legacy_combined_defense_candidates(
            &mut ordering,
            context,
            legal_actions,
            targets,
        ),
    }

    ordering.into_ordered()
}

/// 防御 fallback の action だけを返す薄い wrapper。
pub fn select_combined_threat_defense_fallback_action<'a>(
    context: &GameContext,
    legal_actions: &'a [LegalAction],
    targets: &[ThreatDefenseTarget],
) -> Option<&'a LegalAction> {
    select_combined_threat_defense_fallback_action_with_kind(context, legal_actions, targets)
        .map(|(action, _)| action)
}

/// target 1人に対する safety。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CombinedDefenseTargetSafety {
    /// target の席と種類。
    pub target: ThreatDefenseTarget,
    /// この target にこの牌でロンされないか ([`is_ron_safe_for_target`])。根拠は種類ごとに違い、
    /// リーチ者は現物、`Caution` / `Danger` の副露相手は本人の河または現在有効な一時通過牌。
    pub ron_safe: bool,
    /// `HighOpenHand` target の concealed hand が最後に変化して以降に通ったか。
    pub same_hand_passed: bool,
    /// この target の河に対する [`suji_safety_rank_for`]。字牌では `None`。
    ///
    /// この target 単独の評価そのもので、`ron_safe` による除外は行わない。集約側
    /// ([`CombinedDefenseCandidateDiagnostic::suji_safety_rank`]) は `ron_safe` が `true` の
    /// target を外すため、両者の値は一致しないことがある。
    pub suji_safety_rank: Option<SujiSafetyRank>,
}

impl CombinedDefenseTargetSafety {
    pub fn player(&self) -> usize {
        self.target.player
    }

    pub fn kind(&self) -> ThreatDefenseTargetKind {
        self.target.kind
    }
}

/// 合法 Dahai 1件ごとの、複合 threat に対する防御評価。
///
/// production で使う pure な safety helper の結果をそのまま持つ解析用データで、表示のために
/// safety を計算し直さない。これ自体が action 選択を行うこともない。選択の source of truth は
/// [`select_combined_threat_defense_fallback_action_with_kind`] であり、`selected` はその結果を
/// 写したもの。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CombinedDefenseCandidateDiagnostic {
    /// 対象の合法 Dahai。物理牌(赤5 / 黒5)の区別を保持する。
    pub action: LegalAction,
    /// `action` の牌種。
    pub tile: TileType,
    /// この候補が複合 threat 用の防御 fallback として実際に選ばれたか。
    pub selected: bool,
    /// target ごとの safety。席順で、target が0人なら空。
    pub targets: Vec<CombinedDefenseTargetSafety>,
    /// 全 target に対してロン安全か。target が0人なら `false`。
    pub safe_against_all_threats: bool,
    /// 全 target が hard-safe または same-hand passed で覆われるか。
    pub same_hand_passed_for_all_threats: bool,
    /// exact comparison が利用可能な場合の、target ごとの `R/T` evidence。
    pub player_ron_risk_evidence: Option<Vec<PlayerRonRiskEvidence>>,
    pub honor_safety_rank: Option<HonorSafetyRank>,
    /// [`opponent_honor_value_for_combined_threats`] の結果。数牌では `None`。
    pub opponent_honor_value: Option<OpponentHonorValue>,
    /// [`suited_safety_evidence_for_combined_threats`] の結果そのもの。
    ///
    /// 壁とスジを潰さずに持つので、`suited_safety_rank` が壁由来の `OneChance` / `NoChance` に
    /// なっている場合でも、同時にスジが成立していたかどうかを確認できる。字牌では `None`。
    pub suited_safety_evidence: Option<SuitedSafetyEvidence>,
    pub wall_rank: Option<WallRank>,
    /// [`suji_safety_rank_for_combined_threats`] の結果そのもの。
    ///
    /// 壁と統合する前の純粋なスジ評価なので、`suited_safety_rank` が壁由来の
    /// `OneChance` / `NoChance` になっている場合でも `HalfSuji` と `NoSuji` を区別できる。
    /// ロン不能な target は集約対象から外れるため、`targets` の `suji_safety_rank` をそのまま
    /// 最小値へ潰した値とは一致しないことがある。
    pub suji_safety_rank: Option<SujiSafetyRank>,
    pub suited_safety_rank: Option<SuitedSafetyRank>,
    /// exact evidence があれば `ExactRonRisk`、なければ [`combined_defense_category`] の大分類。
    pub category: Option<CombinedDefenseCategory>,
}

impl CombinedDefenseCandidateDiagnostic {
    /// 合法 Dahai 1件から防御評価を構築する pure helper。Dahai 以外の action では `None`。
    ///
    /// `selected` は production selector の結果をそのまま渡す。ここで選び直さない。
    pub fn for_dahai_action(
        context: &GameContext,
        action: &LegalAction,
        targets: &[ThreatDefenseTarget],
        selected: bool,
    ) -> Option<Self> {
        Self::for_dahai_action_with_ron_risk_vector(context, action, targets, selected, None)
    }

    fn for_dahai_action_with_ron_risk_vector(
        context: &GameContext,
        action: &LegalAction,
        targets: &[ThreatDefenseTarget],
        selected: bool,
        player_ron_risk_evidence: Option<&[PlayerRonRiskEvidence]>,
    ) -> Option<Self> {
        let LegalAction::Dahai { tile } = action else {
            return None;
        };
        let tile_type = tile.tile_type();
        let evidence = suited_safety_evidence_for_combined_threats(tile_type, targets, context);

        Some(Self {
            action: action.clone(),
            tile: tile_type,
            selected,
            targets: targets
                .iter()
                .map(|&target| CombinedDefenseTargetSafety {
                    target,
                    ron_safe: is_ron_safe_for_target(tile_type, target, context),
                    same_hand_passed: is_same_hand_passed_for_target(tile_type, target, context),
                    suji_safety_rank: suji_safety_rank_for(tile_type, target.player, context),
                })
                .collect(),
            safe_against_all_threats: is_safe_against_all_threats(tile_type, targets, context),
            same_hand_passed_for_all_threats: has_same_hand_passed_for_all_threats(
                tile_type, targets, context,
            ),
            player_ron_risk_evidence: player_ron_risk_evidence.map(<[_]>::to_vec),
            honor_safety_rank: honor_safety_rank(tile_type, context),
            opponent_honor_value: opponent_honor_value_for_combined_threats(
                tile_type, targets, context,
            ),
            suited_safety_evidence: evidence,
            wall_rank: evidence.map(|evidence| evidence.wall_rank),
            suji_safety_rank: evidence.map(|evidence| evidence.suji_rank),
            suited_safety_rank: evidence.map(SuitedSafetyEvidence::legacy_rank),
            category: if player_ron_risk_evidence.is_some() {
                Some(CombinedDefenseCategory::ExactRonRisk)
            } else {
                combined_defense_category(tile_type, targets, context)
            },
        })
    }

    /// 合法 action のうち Dahai だけを、元の順序を保って防御評価へ変換する。
    ///
    /// `selected_action` は複合 threat 用の防御 fallback として実際に選ばれた action。一致する
    /// 候補の `selected` だけが `true` になる。
    pub fn for_legal_actions(
        context: &GameContext,
        legal_actions: &[LegalAction],
        targets: &[ThreatDefenseTarget],
        selected_action: Option<&LegalAction>,
    ) -> Vec<Self> {
        Self::for_legal_actions_with_ron_risk_vectors(
            context,
            legal_actions,
            targets,
            selected_action,
            None,
        )
    }

    fn for_legal_actions_with_ron_risk_vectors(
        context: &GameContext,
        legal_actions: &[LegalAction],
        targets: &[ThreatDefenseTarget],
        selected_action: Option<&LegalAction>,
        ron_risk_vectors: Option<&[DahaiRonRiskVector<'_>]>,
    ) -> Vec<Self> {
        legal_actions
            .iter()
            .filter_map(|action| {
                Self::for_dahai_action_with_ron_risk_vector(
                    context,
                    action,
                    targets,
                    selected_action == Some(action),
                    player_ron_risk_evidence_for_action(ron_risk_vectors, action),
                )
            })
            .collect()
    }
}

/// 採用された複合 threat 用の防御 fallback の内訳。
///
/// [`select_combined_threat_defense_fallback_action_with_kind`] の結果をそのまま写したもので、
/// 診断側で選び直さない。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CombinedDefenseSelectionDiagnostic {
    /// 実際に選ばれた合法 Dahai。物理牌(赤5 / 黒5)の区別を保持する。
    pub selected_action: LegalAction,
    /// その action が選ばれた大分類。
    pub selected_category: CombinedDefenseCategory,
    /// exact comparison を使った場合の、選択牌に対する target ごとの `R/T` evidence。
    pub selected_player_ron_risk_evidence: Option<Vec<PlayerRonRiskEvidence>>,
}

/// 複合 threat に対する防御 safety の構造化診断。
///
/// `targets` が空の局面は「複合 threat ではない」で、候補評価も作らない。リーチ者だけ / `Caution` / `Danger` の
/// 副露相手だけの局面は既存の `Defense` / `OpenHand defense` が担当する。
///
/// `selected` は複合 threat 用の防御 fallback を実際に採用した場合だけ `Some` になる。採用しな
/// かった局面(押し引きが `Fold` ではない、安全牌候補が無いなど)では `None` で、候補評価だけが
/// 残る。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CombinedDefenseDiagnostic {
    /// 複合 threat の target。席順で、複合 threat でなければ空。
    pub targets: Vec<ThreatDefenseTarget>,
    /// 採用された防御 fallback。採用しなかった場合は `None`。
    pub selected: Option<CombinedDefenseSelectionDiagnostic>,
    /// target が1人以上いる場合の全合法 Dahai の防御評価。target が0人なら空。
    pub candidates: Vec<CombinedDefenseCandidateDiagnostic>,
}

impl CombinedDefenseDiagnostic {
    /// 構築済みの facts / classification と選択結果から診断を作る pure helper。
    ///
    /// target を作り直さないので、押し引きが参照した threat と診断の target が必ず一致する。
    /// `selected` には [`select_combined_threat_defense_fallback_action_with_kind`] の戻り値を
    /// そのまま渡す。ここで防御 fallback や exact evidence を計算し直さない。production が
    /// exact を使った診断は、保持済み evidence を受け取る `from_evaluation` 経路で構築する。
    pub fn from_threats(
        context: &GameContext,
        legal_actions: &[LegalAction],
        player_threats: &[PlayerThreatFacts; 4],
        assessments: &[OpenHandThreatAssessment; 4],
        selected: Option<(&LegalAction, CombinedDefenseCategory)>,
    ) -> Self {
        let targets = combined_threat_defense_targets(player_threats, assessments);
        Self::from_parts(context, legal_actions, targets, selected, None)
    }

    pub(crate) fn from_evaluation(
        context: &GameContext,
        legal_actions: &[LegalAction],
        player_threats: &[PlayerThreatFacts; 4],
        assessments: &[OpenHandThreatAssessment; 4],
        evaluation: &CombinedDefenseEvaluation<'_>,
    ) -> Self {
        Self::from_parts(
            context,
            legal_actions,
            combined_threat_defense_targets(player_threats, assessments),
            evaluation.selected,
            evaluation.ron_risk_vectors.as_deref(),
        )
    }

    fn from_parts(
        context: &GameContext,
        legal_actions: &[LegalAction],
        targets: Vec<ThreatDefenseTarget>,
        selected: Option<(&LegalAction, CombinedDefenseCategory)>,
        ron_risk_vectors: Option<&[DahaiRonRiskVector<'_>]>,
    ) -> Self {
        let candidates = if targets.is_empty() {
            Vec::new()
        } else {
            CombinedDefenseCandidateDiagnostic::for_legal_actions_with_ron_risk_vectors(
                context,
                legal_actions,
                &targets,
                selected.map(|(action, _)| action),
                ron_risk_vectors,
            )
        };
        Self {
            targets,
            selected: selected.map(|(action, category)| CombinedDefenseSelectionDiagnostic {
                selected_action: action.clone(),
                selected_category: category,
                selected_player_ron_risk_evidence: player_ron_risk_evidence_for_action(
                    ron_risk_vectors,
                    action,
                )
                .map(<[_]>::to_vec),
            }),
            candidates,
        }
    }

    /// `GameContext` から診断を作る adapter。facts の構築も分類も既存経路を共有する。
    pub fn from_context(
        context: &GameContext,
        legal_actions: &[LegalAction],
        selected: Option<(&LegalAction, CombinedDefenseCategory)>,
    ) -> Self {
        let facts = player_threat_facts_from_context(context);
        Self::from_threats(
            context,
            legal_actions,
            &facts,
            &classify_open_hand_threats(&facts),
            selected,
        )
    }

    /// 複合 threat の局面か。target が1人以上いることと同値。
    pub fn has_target(&self) -> bool {
        !self.targets.is_empty()
    }

    /// 採用された防御 fallback の大分類。採用しなかった場合は `None`。
    pub fn selected_category(&self) -> Option<CombinedDefenseCategory> {
        self.selected
            .as_ref()
            .map(|selection| selection.selected_category)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context::TableStateFacts;
    use crate::defense::is_discarded_by_player;
    use crate::defense::{
        RonRiskEvidence, compare_lexicographic_minimax_ron_risk, is_genbutsu_for_all_reached,
        open_hand_targets_dahai_actions_by_ron_risk, reached_opponents_dahai_actions_by_ron_risk,
        suited_safety_evidence_for_players, suited_safety_rank_for_all_reached, wall_rank,
    };
    use crate::meld::{Meld, MeldKind};
    use crate::open_hand_threat::OpenHandThreatLevel;
    use bot_logic::TileId;

    // 自分は player 0、親は player 1、場風は東で固定する。player 1 がリーチ、player 3 が3副露の
    // actionable OpenHandThreat という複合 threat を既定の形にする。
    const SELF_PLAYER: usize = 0;
    const RIICHI_TARGET: usize = 1;
    const OTHER_PLAYER: usize = 2;
    const OPEN_HAND_TARGET: usize = 3;

    fn tile_type(mjai: &str) -> TileType {
        TileType::from_mjai_type_str(mjai).unwrap()
    }

    // 牌種から物理牌を作る。copy で同じ牌種の別の物理牌になる。
    fn tile_copy(mjai: &str, copy: u8) -> TileId {
        TileId::new(tile_type(mjai).raw() * 4 + copy).unwrap()
    }

    fn tile(mjai: &str) -> TileId {
        tile_copy(mjai, 0)
    }

    fn tiles(mjai: &str) -> Vec<TileId> {
        mjai.split_whitespace().map(tile).collect()
    }

    fn dahai(mjai: &str) -> LegalAction {
        LegalAction::Dahai { tile: tile(mjai) }
    }

    // 牌種のみを指定した Chi。ドラも役牌も含まない。
    fn chi() -> Meld {
        Meld::new(MeldKind::Chi, tiles("1s 2s 3s"), Some(tile("1s")))
    }

    // 副露を count 個持つ席を作る。Caution / Danger 条件の「3副露以上」を満たすかどうかを count で決める。
    fn open_melds(count: usize) -> Vec<Meld> {
        (0..count).map(|_| chi()).collect()
    }

    fn chi_from(mjai: &str) -> Meld {
        let tiles = tiles(mjai);
        Meld::new(MeldKind::Chi, tiles.clone(), Some(tiles[0]))
    }

    fn exact_open_melds() -> Vec<Meld> {
        let value_pon = Meld::new(
            MeldKind::Pon,
            (0..3).map(|copy| tile_copy("P", copy)).collect(),
            Some(tile("P")),
        );
        vec![
            value_pon,
            chi_from("1p 2p 3p"),
            chi_from("4p 5p 6p"),
            chi_from("7p 8p 9p"),
        ]
    }

    fn riichi_ankans() -> Vec<Meld> {
        ["1s", "2s", "3s", "4s"]
            .into_iter()
            .map(|mjai| {
                Meld::new(
                    MeldKind::Ankan,
                    (0..4).map(|copy| tile_copy(mjai, copy)).collect(),
                    None,
                )
            })
            .collect()
    }

    fn visible_with_remaining(pool: &[(&str, u8)]) -> Vec<TileId> {
        let mut remaining = [0u8; TileType::COUNT];
        for &(mjai, copies) in pool {
            remaining[tile_type(mjai).index()] = copies;
        }
        TileType::all()
            .flat_map(|tile| TileId::copies(tile).take(usize::from(4 - remaining[tile.index()])))
            .collect()
    }

    fn exact_combined_context(pool: &[(&str, u8)]) -> ContextSpec {
        ContextSpec::new()
            .reached(RIICHI_TARGET)
            .melds_of(RIICHI_TARGET, riichi_ankans())
            .melds_of(OPEN_HAND_TARGET, exact_open_melds())
            .visible(visible_with_remaining(pool))
            .known_empty_temporary_passed()
            .remaining_tiles(1)
    }

    #[derive(Debug, Clone, Default)]
    struct ContextSpec {
        player_id: Option<u8>,
        oya: Option<u8>,
        round_wind: Option<TileType>,
        discards: [Vec<TileId>; 4],
        reached: [bool; 4],
        melds: [Vec<Meld>; 4],
        visible_tiles: Vec<TileId>,
        post_reach_passed: [Vec<TileType>; 4],
        temporary_passed: Option<[Vec<TileType>; 4]>,
        same_hand_passed: Option<[Vec<TileType>; 4]>,
        remaining_tiles: Option<u32>,
    }

    impl ContextSpec {
        // 自分 player 0 / 親 player 1 / 場風 東 の既定。副露も河もリーチも無い。
        fn new() -> Self {
            Self {
                player_id: Some(SELF_PLAYER as u8),
                oya: Some(RIICHI_TARGET as u8),
                round_wind: Some(tile_type("E")),
                ..Self::default()
            }
        }

        // player 1 がリーチ、player 3 が3副露の複合 threat。
        fn combined() -> Self {
            Self::new()
                .reached(RIICHI_TARGET)
                .melds_of(OPEN_HAND_TARGET, open_melds(3))
        }

        fn melds_of(mut self, player: usize, melds: Vec<Meld>) -> Self {
            self.melds[player] = melds;
            self
        }

        fn discards_of(mut self, player: usize, mjai: &str) -> Self {
            self.discards[player] = tiles(mjai);
            self
        }

        fn reached(mut self, player: usize) -> Self {
            self.reached[player] = true;
            self
        }

        fn visible(mut self, tiles: Vec<TileId>) -> Self {
            self.visible_tiles = tiles;
            self
        }

        fn post_reach_passed(mut self, player: usize, mjai: &str) -> Self {
            self.post_reach_passed[player] = mjai
                .split_whitespace()
                .map(tile_type)
                .collect::<Vec<TileType>>();
            self
        }

        fn temporary_passed(mut self, player: usize, mjai: &str) -> Self {
            let passed = self
                .temporary_passed
                .get_or_insert_with(|| std::array::from_fn(|_| Vec::new()));
            passed[player] = mjai.split_whitespace().map(tile_type).collect();
            self
        }

        fn same_hand_passed(mut self, player: usize, mjai: &str) -> Self {
            let passed = self
                .same_hand_passed
                .get_or_insert_with(|| std::array::from_fn(|_| Vec::new()));
            passed[player] = mjai.split_whitespace().map(tile_type).collect();
            self
        }

        fn known_empty_temporary_passed(mut self) -> Self {
            self.temporary_passed = Some(Default::default());
            self
        }

        fn remaining_tiles(mut self, remaining_tiles: u32) -> Self {
            self.remaining_tiles = Some(remaining_tiles);
            self
        }

        fn build(self) -> GameContext {
            GameContext::from_parts_with_melds(
                None,
                vec![],
                vec![],
                self.round_wind,
                None,
                self.visible_tiles,
                self.player_id,
                self.oya,
                self.discards,
                self.reached,
                self.melds,
            )
            .with_post_reach_passed_tiles(self.post_reach_passed)
            .with_temporary_passed_tiles(self.temporary_passed)
            .with_same_hand_passed_tiles(self.same_hand_passed)
            .with_table_state_facts(TableStateFacts {
                remaining_tiles: self.remaining_tiles,
                ..TableStateFacts::default()
            })
        }
    }

    fn targets(context: &GameContext) -> Vec<ThreatDefenseTarget> {
        combined_threat_defense_targets_from_context(context)
    }

    fn fallback(
        context: &GameContext,
        legal_actions: &[LegalAction],
    ) -> Option<(LegalAction, CombinedDefenseCategory)> {
        select_combined_threat_defense_fallback_action_with_kind(
            context,
            legal_actions,
            &targets(context),
        )
        .map(|(action, category)| (action.clone(), category))
    }

    // production evaluation が実際に構築する exact evidence と同じものを ordering へ渡す。
    fn ordered<'a>(
        context: &GameContext,
        legal_actions: &'a [LegalAction],
    ) -> Vec<(&'a LegalAction, CombinedDefenseCategory)> {
        let targets = targets(context);
        let vectors = evaluate_combined_threat_defense_fallback_action_with_kind(
            context,
            legal_actions,
            &targets,
        )
        .ron_risk_vectors;
        ordered_combined_defense_candidates(context, legal_actions, &targets, vectors.as_deref())
    }

    // ordering の先頭が既存 selector の選択と一致することを確認する。ranking は rank 1 だけを
    // 別扱いせず、ordering 全体が production selector と同じ並びになっている。
    fn assert_ordering_leads_with_the_selection(
        context: &GameContext,
        legal_actions: &[LegalAction],
    ) {
        let ordered = ordered(context, legal_actions);
        let selectable = ordered
            .iter()
            .find(|&&(_, category)| is_selectable_combined_defense_category(category))
            .map(|&(action, category)| (action.clone(), category));

        assert_eq!(selectable, fallback(context, legal_actions));
    }

    #[test]
    fn the_ordering_leads_with_the_production_selection() {
        // hard-safe / same-hand passed / exact / legacy のどの段で決まる局面でも一致する。
        let hard_safe = exact_combined_context(&[("1m", 1), ("2m", 2)])
            .discards_of(RIICHI_TARGET, "2m")
            .discards_of(OPEN_HAND_TARGET, "2m")
            .build();
        let same_hand_passed = exact_combined_context(&[("1m", 1), ("2m", 2)])
            .discards_of(RIICHI_TARGET, "2m")
            .same_hand_passed(OPEN_HAND_TARGET, "2m")
            .build();
        let exact = exact_combined_context(&[("1m", 1), ("2m", 2)]).build();
        for context in [hard_safe, same_hand_passed, exact] {
            assert_ordering_leads_with_the_selection(&context, &[dahai("1m"), dahai("2m")]);
        }

        assert_ordering_leads_with_the_selection(
            &suited_safety_context(),
            &[dahai("4m"), dahai("1s")],
        );

        // NoSafety しか無い局面では既存 selector が None を返し、ranking では順位を持つ。
        let no_safety = ContextSpec::combined().build();
        let legal_actions = vec![dahai("4m"), dahai("5p")];
        assert_eq!(fallback(&no_safety, &legal_actions), None);
        assert_eq!(ordered(&no_safety, &legal_actions).len(), 2);
        assert_ordering_leads_with_the_selection(&no_safety, &legal_actions);
    }

    // 診断用の追加収集を通した evaluation。production selection は変わらない。
    fn evaluation_with_candidate_evidence<'a>(
        context: &GameContext,
        legal_actions: &'a [LegalAction],
    ) -> CombinedDefenseEvaluation<'a> {
        let targets = targets(context);
        let mut evaluation = evaluate_combined_threat_defense_fallback_action_with_kind(
            context,
            legal_actions,
            &targets,
        );
        collect_combined_candidate_ron_risk_evidence(
            context,
            legal_actions,
            &targets,
            &mut evaluation,
        );
        evaluation
    }

    #[test]
    fn the_candidate_evidence_is_collected_after_a_hard_safe_selection() {
        let context = exact_combined_context(&[("1m", 1), ("2m", 2)])
            .discards_of(RIICHI_TARGET, "2m")
            .discards_of(OPEN_HAND_TARGET, "2m")
            .build();
        let legal_actions = vec![dahai("1m"), dahai("2m")];

        // production API は従来どおり hard-safe で早期 return し、exact model を走らせない。
        let production = evaluate_combined_threat_defense_fallback_action_with_kind(
            &context,
            &legal_actions,
            &targets(&context),
        );
        assert_eq!(
            production.selected,
            Some((
                &legal_actions[1],
                CombinedDefenseCategory::SafeAgainstAllThreats
            ))
        );
        assert!(production.ron_risk_vectors.is_none());

        // 診断用の追加収集では、選択を変えずに全 target の candidate exact evidence が揃う。
        let evaluation = evaluation_with_candidate_evidence(&context, &legal_actions);
        assert_eq!(evaluation.selected, production.selected);
        let vectors = evaluation
            .ron_risk_vectors
            .as_deref()
            .expect("診断用に exact evidence を収集する");
        assert!(vectors.iter().all(|vector| {
            vector
                .player_evidence
                .iter()
                .map(|evidence| evidence.player)
                .eq([RIICHI_TARGET, OPEN_HAND_TARGET])
        }));

        // hard-safe の優先順位は変えず、その下の候補だけが exact evidence で並ぶ。
        assert_eq!(
            ordered_combined_defense_candidates(
                &context,
                &legal_actions,
                &targets(&context),
                evaluation.ron_risk_vectors.as_deref(),
            ),
            vec![
                (
                    &legal_actions[1],
                    CombinedDefenseCategory::SafeAgainstAllThreats
                ),
                (&legal_actions[0], CombinedDefenseCategory::ExactRonRisk),
            ]
        );
    }

    #[test]
    fn the_candidate_evidence_is_collected_after_a_same_hand_passed_selection() {
        let context = exact_combined_context(&[("1m", 1), ("2m", 2)])
            .discards_of(RIICHI_TARGET, "2m")
            .same_hand_passed(OPEN_HAND_TARGET, "2m")
            .build();
        let legal_actions = vec![dahai("1m"), dahai("2m")];

        let production = evaluate_combined_threat_defense_fallback_action_with_kind(
            &context,
            &legal_actions,
            &targets(&context),
        );
        assert_eq!(
            production.selected,
            Some((&legal_actions[1], CombinedDefenseCategory::SameHandPassed))
        );
        assert!(production.ron_risk_vectors.is_none());

        let evaluation = evaluation_with_candidate_evidence(&context, &legal_actions);
        assert_eq!(evaluation.selected, production.selected);
        assert!(evaluation.ron_risk_vectors.is_some());
    }

    #[test]
    fn the_candidate_evidence_is_not_recollected_after_an_exact_comparison() {
        // exact 比較まで進んだ局面では追加収集しない。試して unavailable だった場合も同じ。
        let exact = exact_combined_context(&[("1m", 1), ("2m", 2)]).build();
        let legal_actions = vec![dahai("1m"), dahai("2m")];
        let production = evaluate_combined_threat_defense_fallback_action_with_kind(
            &exact,
            &legal_actions,
            &targets(&exact),
        );
        let evaluation = evaluation_with_candidate_evidence(&exact, &legal_actions);
        assert_eq!(evaluation.selected, production.selected);
        assert_eq!(evaluation.ron_risk_vectors, production.ron_risk_vectors);

        let legacy = suited_safety_context();
        let legal_actions = vec![dahai("4m"), dahai("1s")];
        let production = evaluate_combined_threat_defense_fallback_action_with_kind(
            &legacy,
            &legal_actions,
            &targets(&legacy),
        );
        assert!(matches!(
            production.selected,
            Some((_, CombinedDefenseCategory::SuitedSafety(_)))
        ));
        let evaluation = evaluation_with_candidate_evidence(&legacy, &legal_actions);
        assert_eq!(evaluation.selected, production.selected);
        assert_eq!(evaluation.ron_risk_vectors, production.ron_risk_vectors);
    }

    #[test]
    fn the_exact_tier_orders_multiple_targets_by_the_lexicographic_minimax() {
        let context = exact_combined_context(&[("1m", 1), ("2m", 2), ("3m", 3)]).build();
        let legal_actions = vec![dahai("3m"), dahai("1m"), dahai("2m")];
        let evaluation = evaluate_combined_threat_defense_fallback_action_with_kind(
            &context,
            &legal_actions,
            &targets(&context),
        );
        let vectors = evaluation
            .ron_risk_vectors
            .as_deref()
            .expect("全 target の exact model が使える");
        assert!(
            vectors
                .iter()
                .all(|vector| vector.player_evidence.len() == 2)
        );

        let mut expected: Vec<_> = vectors.iter().collect();
        expected.sort_by(|left, right| {
            compare_lexicographic_minimax_ron_risk(&left.player_evidence, &right.player_evidence)
                .expect("exact 比較できる")
        });

        // 複数 target でも既存の worst-first lexicographic minimax の順序そのまま。
        assert_eq!(
            ordered(&context, &legal_actions)
                .into_iter()
                .map(|(action, _)| action)
                .collect::<Vec<_>>(),
            expected
                .into_iter()
                .map(|vector| vector.action)
                .collect::<Vec<_>>()
        );
        assert_ordering_leads_with_the_selection(&context, &legal_actions);
    }

    #[test]
    fn the_ordering_places_hard_safe_before_same_hand_passed_and_exact() {
        let context = exact_combined_context(&[("1m", 1), ("2m", 2), ("3m", 3)])
            .discards_of(RIICHI_TARGET, "2m 3m")
            .discards_of(OPEN_HAND_TARGET, "2m")
            .same_hand_passed(OPEN_HAND_TARGET, "3m")
            .build();
        let legal_actions = vec![dahai("1m"), dahai("2m"), dahai("3m")];

        assert_eq!(
            ordered(&context, &legal_actions)
                .into_iter()
                .map(|(action, category)| (action.clone(), category))
                .collect::<Vec<_>>(),
            vec![
                (dahai("2m"), CombinedDefenseCategory::SafeAgainstAllThreats),
                (dahai("3m"), CombinedDefenseCategory::SameHandPassed),
                (
                    dahai("1m"),
                    CombinedDefenseCategory::SuitedSafety(SuitedSafetyRank::NoSafety)
                ),
            ]
        );
        assert_ordering_leads_with_the_selection(&context, &legal_actions);
    }

    // ---- target の決定 ----

    // 河12枚。1副露と組み合わせると Caution になる。
    const TWELVE_DISCARDS: &str = "1m 2m 3m 4m 5m 6m 7m 8m 9m 1p 2p 3p";

    fn assert_the_open_hand_level(context: &GameContext, level: OpenHandThreatLevel) {
        let facts = player_threat_facts_from_context(context);
        assert_eq!(
            classify_open_hand_threats(&facts)[OPEN_HAND_TARGET].level(),
            Some(level)
        );
    }

    #[test]
    fn a_riichi_and_a_danger_open_hand_become_targets_in_seat_order() {
        let context = ContextSpec::combined().build();
        assert_the_open_hand_level(&context, OpenHandThreatLevel::Danger);

        assert_eq!(
            targets(&context),
            vec![
                ThreatDefenseTarget::riichi(RIICHI_TARGET),
                ThreatDefenseTarget::high_open_hand(OPEN_HAND_TARGET),
            ]
        );
    }

    #[test]
    fn a_riichi_and_a_caution_open_hand_become_targets_in_seat_order() {
        // 局進行だけで Caution になった相手も、従来の High と同じく複合 threat の target にする。
        let context = ContextSpec::new()
            .reached(RIICHI_TARGET)
            .melds_of(OPEN_HAND_TARGET, open_melds(1))
            .discards_of(OPEN_HAND_TARGET, TWELVE_DISCARDS)
            .build();
        assert_the_open_hand_level(&context, OpenHandThreatLevel::Caution);

        let expected = vec![
            ThreatDefenseTarget::riichi(RIICHI_TARGET),
            ThreatDefenseTarget::high_open_hand(OPEN_HAND_TARGET),
        ];
        let facts = player_threat_facts_from_context(&context);
        assert_eq!(targets(&context), expected);
        assert_eq!(
            threat_defense_targets(&facts, &classify_open_hand_threats(&facts)),
            expected
        );
    }

    #[test]
    fn a_caution_open_hand_alone_is_a_shared_threat_target() {
        let context = ContextSpec::new()
            .melds_of(OPEN_HAND_TARGET, open_melds(1))
            .discards_of(OPEN_HAND_TARGET, TWELVE_DISCARDS)
            .build();
        assert_the_open_hand_level(&context, OpenHandThreatLevel::Caution);
        let facts = player_threat_facts_from_context(&context);

        assert!(targets(&context).is_empty());
        assert_eq!(
            threat_defense_targets(&facts, &classify_open_hand_threats(&facts)),
            vec![ThreatDefenseTarget::high_open_hand(OPEN_HAND_TARGET)]
        );
    }

    #[test]
    fn a_riichi_without_an_actionable_open_hand_has_no_combined_target() {
        // リーチ者だけの局面は既存のリーチ者向け防御が担当する。
        let context = ContextSpec::new().reached(RIICHI_TARGET).build();

        assert_eq!(context.reached_opponents(), vec![RIICHI_TARGET]);
        assert!(targets(&context).is_empty());
    }

    #[test]
    fn an_actionable_open_hand_without_a_riichi_has_no_combined_target() {
        // Caution / Danger の副露相手だけの局面は既存の OpenHand 防御が担当する。
        let context = ContextSpec::new()
            .melds_of(OPEN_HAND_TARGET, open_melds(3))
            .build();
        let facts = player_threat_facts_from_context(&context);

        assert_eq!(
            actionable_open_hand_threat_players(&classify_open_hand_threats(&facts)),
            vec![OPEN_HAND_TARGET]
        );
        assert!(targets(&context).is_empty());
    }

    #[test]
    fn a_present_open_hand_is_not_a_combined_target() {
        // 1副露だけの相手は Present なので、リーチ者がいても複合 threat にしない。
        let context = ContextSpec::new()
            .reached(RIICHI_TARGET)
            .melds_of(OPEN_HAND_TARGET, open_melds(1))
            .build();
        let facts = player_threat_facts_from_context(&context);

        assert_eq!(
            classify_open_hand_threats(&facts)[OPEN_HAND_TARGET].level(),
            Some(OpenHandThreatLevel::Present)
        );
        assert!(targets(&context).is_empty());
    }

    #[test]
    fn a_reached_player_with_open_melds_is_only_a_riichi_target() {
        // リーチ済みの席は OpenHandThreat の対象外。1つの席が両方の target になることはない。
        let context = ContextSpec::combined()
            .melds_of(RIICHI_TARGET, open_melds(3))
            .build();

        assert_eq!(
            targets(&context),
            vec![
                ThreatDefenseTarget::riichi(RIICHI_TARGET),
                ThreatDefenseTarget::high_open_hand(OPEN_HAND_TARGET),
            ]
        );
    }

    #[test]
    fn an_unknown_player_id_makes_no_combined_target() {
        // 席が不明な相手を Caution / Danger と推測しないので、複合 threat にもならない。
        let mut spec = ContextSpec::combined();
        spec.player_id = None;
        let context = spec.build();

        assert!(!context.reached_opponents().is_empty());
        assert!(targets(&context).is_empty());
    }

    #[test]
    fn the_targets_match_the_shared_threat_sources() {
        // target 側でリーチも Caution / Danger も判定し直さない。
        let context = ContextSpec::combined().build();
        let facts = player_threat_facts_from_context(&context);

        assert_eq!(
            combined_threat_defense_targets(&facts, &classify_open_hand_threats(&facts)),
            targets(&context)
        );
        assert_eq!(
            combined_threat_defense_targets_from_facts(&facts),
            targets(&context)
        );
    }

    #[test]
    fn the_shared_threat_targets_cover_each_threat_alone() {
        // 複合 threat 用の target は従来どおり両方いる局面だけだが、共有 helper は
        // リーチだけ・Caution / Danger の非リーチ相手だけの局面でもその target を返す。
        let facts_of = |context: &GameContext| player_threat_facts_from_context(context);

        let reach_only = ContextSpec::new().reached(RIICHI_TARGET).build();
        let facts = facts_of(&reach_only);
        assert!(combined_threat_defense_targets_from_facts(&facts).is_empty());
        assert_eq!(
            threat_defense_targets(&facts, &classify_open_hand_threats(&facts)),
            vec![ThreatDefenseTarget::riichi(RIICHI_TARGET)]
        );

        let open_hand_only = ContextSpec::new()
            .melds_of(OPEN_HAND_TARGET, open_melds(3))
            .build();
        let facts = facts_of(&open_hand_only);
        assert!(combined_threat_defense_targets_from_facts(&facts).is_empty());
        assert_eq!(
            threat_defense_targets(&facts, &classify_open_hand_threats(&facts)),
            vec![ThreatDefenseTarget::high_open_hand(OPEN_HAND_TARGET)]
        );

        // 複合 threat では従来の target と同じものを返す。
        let combined = ContextSpec::combined().build();
        let facts = facts_of(&combined);
        assert_eq!(
            threat_defense_targets(&facts, &classify_open_hand_threats(&facts)),
            combined_threat_defense_targets_from_facts(&facts)
        );
        assert!(!combined_threat_defense_targets_from_facts(&facts).is_empty());

        // threat がいなければ空。
        let no_threat = ContextSpec::new().build();
        let facts = facts_of(&no_threat);
        assert!(threat_defense_targets(&facts, &classify_open_hand_threats(&facts)).is_empty());
    }

    // ---- target ごとのロン安全の根拠 ----

    #[test]
    fn a_tile_in_every_targets_river_is_safe_against_all_threats() {
        let context = ContextSpec::combined()
            .discards_of(RIICHI_TARGET, "4s")
            .discards_of(OPEN_HAND_TARGET, "4s")
            .build();
        let targets = targets(&context);

        for &target in &targets {
            assert!(
                is_ron_safe_for_target(tile_type("4s"), target, &context),
                "{target:?}"
            );
        }
        assert!(is_safe_against_all_threats(
            tile_type("4s"),
            &targets,
            &context
        ));
        assert_eq!(
            combined_defense_category(tile_type("4s"), &targets, &context),
            Some(CombinedDefenseCategory::SafeAgainstAllThreats)
        );
    }

    #[test]
    fn a_post_reach_passed_tile_is_ron_safe_only_for_the_riichi_target() {
        // リーチ者は現物 (本人の河 + post_reach_passed)、Caution / Danger の副露相手は本人の河だけ。
        let context = ContextSpec::combined()
            .post_reach_passed(RIICHI_TARGET, "4s")
            .discards_of(OPEN_HAND_TARGET, "4s")
            .build();
        let targets = targets(&context);

        assert!(!is_discarded_by_player(
            tile_type("4s"),
            RIICHI_TARGET,
            &context
        ));
        assert!(is_ron_safe_for_target(
            tile_type("4s"),
            ThreatDefenseTarget::riichi(RIICHI_TARGET),
            &context
        ));
        assert!(is_safe_against_all_threats(
            tile_type("4s"),
            &targets,
            &context
        ));
    }

    #[test]
    fn a_post_reach_passed_tile_is_not_ron_safe_for_the_open_hand_target() {
        // 他家が切って通っただけの牌は、副露相手にとっての安全根拠にしない。
        let context = ContextSpec::combined()
            .post_reach_passed(RIICHI_TARGET, "4s")
            .post_reach_passed(OPEN_HAND_TARGET, "4s")
            .discards_of(OTHER_PLAYER, "4s")
            .build();
        let targets = targets(&context);

        assert!(context.is_post_reach_passed(tile_type("4s"), OPEN_HAND_TARGET));
        assert!(is_ron_safe_for_target(
            tile_type("4s"),
            ThreatDefenseTarget::riichi(RIICHI_TARGET),
            &context
        ));
        assert!(!is_ron_safe_for_target(
            tile_type("4s"),
            ThreatDefenseTarget::high_open_hand(OPEN_HAND_TARGET),
            &context
        ));
        assert!(!is_safe_against_all_threats(
            tile_type("4s"),
            &targets,
            &context
        ));
        assert_ne!(
            combined_defense_category(tile_type("4s"), &targets, &context),
            Some(CombinedDefenseCategory::SafeAgainstAllThreats)
        );
    }

    #[test]
    fn a_current_temporary_passed_tile_is_safe_for_the_open_hand_target() {
        let context = ContextSpec::combined()
            .post_reach_passed(RIICHI_TARGET, "9m")
            .temporary_passed(OPEN_HAND_TARGET, "9m")
            .build();
        let targets = targets(&context);

        assert!(is_ron_safe_for_target(
            tile_type("9m"),
            ThreatDefenseTarget::riichi(RIICHI_TARGET),
            &context
        ));
        assert!(is_ron_safe_for_target(
            tile_type("9m"),
            ThreatDefenseTarget::high_open_hand(OPEN_HAND_TARGET),
            &context
        ));
        assert_eq!(
            combined_defense_category(tile_type("9m"), &targets, &context),
            Some(CombinedDefenseCategory::SafeAgainstAllThreats)
        );
    }

    #[test]
    fn same_hand_passed_is_not_hard_safe_for_the_open_hand_target() {
        let context = ContextSpec::combined()
            .post_reach_passed(RIICHI_TARGET, "2s")
            .same_hand_passed(OPEN_HAND_TARGET, "2s")
            .build();
        let targets = targets(&context);
        let open_hand = ThreatDefenseTarget::high_open_hand(OPEN_HAND_TARGET);

        assert!(is_same_hand_passed_for_target(
            tile_type("2s"),
            open_hand,
            &context
        ));
        assert!(!is_ron_safe_for_target(
            tile_type("2s"),
            open_hand,
            &context
        ));
        assert!(!is_safe_against_all_threats(
            tile_type("2s"),
            &targets,
            &context
        ));
        assert_eq!(
            combined_defense_category(tile_type("2s"), &targets, &context),
            Some(CombinedDefenseCategory::SameHandPassed)
        );
    }

    #[test]
    fn the_first_category_is_not_the_existing_genbutsu() {
        // リーチ現物と副露相手本人の河が混ざった集合なので、リーチ者向けの現物とは別物。
        let context = ContextSpec::combined()
            .post_reach_passed(RIICHI_TARGET, "4s")
            .discards_of(OPEN_HAND_TARGET, "4s")
            .discards_of(OTHER_PLAYER, "5p")
            .build();
        let targets = targets(&context);

        // 5p はリーチ者にとって現物ではないので、既存の現物判定では安全にならない。
        assert!(is_genbutsu_for_all_reached(tile_type("4s"), &context));
        assert!(!is_genbutsu_for_all_reached(tile_type("5p"), &context));
        assert!(is_safe_against_all_threats(
            tile_type("4s"),
            &targets,
            &context
        ));
        assert!(!is_safe_against_all_threats(
            tile_type("5p"),
            &targets,
            &context
        ));
    }

    // ---- ロン可能な target だけの集約 ----

    #[test]
    fn a_genbutsu_riichi_target_is_excluded_from_the_aggregated_suji() {
        // 5m はリーチ者の現物。その両側スジは集約に持ち込まず、副露相手の無スジが残る。
        let context = ContextSpec::combined()
            .discards_of(RIICHI_TARGET, "5m 2m 8m")
            .discards_of(OPEN_HAND_TARGET, "9p")
            .build();
        let targets = targets(&context);

        assert_eq!(
            suji_safety_rank_for(tile_type("5m"), RIICHI_TARGET, &context),
            Some(SujiSafetyRank::Suji)
        );
        assert_eq!(
            suji_safety_rank_for(tile_type("5m"), OPEN_HAND_TARGET, &context),
            Some(SujiSafetyRank::NoSuji)
        );
        assert_eq!(
            suji_safety_rank_for_combined_threats(tile_type("5m"), &targets, &context),
            Some(SujiSafetyRank::NoSuji)
        );
        assert_eq!(
            suited_safety_rank_for_combined_threats(tile_type("5m"), &targets, &context),
            Some(SuitedSafetyRank::NoSafety)
        );
    }

    #[test]
    fn a_river_safe_open_hand_target_is_excluded_from_the_aggregated_suji() {
        // 5s は副露相手の河にあるのでロンされない。リーチ者に対する両側スジだけが残る。
        let context = ContextSpec::combined()
            .discards_of(RIICHI_TARGET, "2s 8s")
            .discards_of(OPEN_HAND_TARGET, "5s")
            .build();
        let targets = targets(&context);

        assert!(!is_safe_against_all_threats(
            tile_type("5s"),
            &targets,
            &context
        ));
        assert_eq!(
            suji_safety_rank_for(tile_type("5s"), OPEN_HAND_TARGET, &context),
            Some(SujiSafetyRank::NoSuji)
        );
        assert_eq!(
            suji_safety_rank_for_combined_threats(tile_type("5s"), &targets, &context),
            Some(SujiSafetyRank::Suji)
        );
        assert_eq!(
            suited_safety_rank_for_combined_threats(tile_type("5s"), &targets, &context),
            Some(SuitedSafetyRank::Suji)
        );
        assert_eq!(
            combined_defense_category(tile_type("5s"), &targets, &context),
            Some(CombinedDefenseCategory::SuitedSafety(
                SuitedSafetyRank::Suji
            ))
        );
    }

    #[test]
    fn the_most_dangerous_rank_of_the_ron_capable_targets_is_used() {
        // どちらの target にもロンされ得る場合は最も危険な rank を採る。
        let context = ContextSpec::combined()
            .discards_of(RIICHI_TARGET, "2s 8s")
            .build();
        let targets = targets(&context);

        assert_eq!(
            suji_safety_rank_for_combined_threats(tile_type("5s"), &targets, &context),
            Some(SujiSafetyRank::NoSuji)
        );
    }

    #[test]
    fn only_the_ron_capable_targets_decide_the_opponent_honor_value() {
        // 東は親の player 1 にとってダブ東、player 3 にとっては場風だけ。
        let base = ContextSpec::combined();
        let both = base.clone().build();
        assert_eq!(
            opponent_honor_value_for_combined_threats(tile_type("E"), &targets(&both), &both),
            Some(OpponentHonorValue::DoubleWind)
        );

        // 東がリーチ者の現物になると、そのダブ東は集約に残らない。
        let riichi_safe = base.clone().discards_of(RIICHI_TARGET, "E").build();
        assert_eq!(
            opponent_honor_value_for_combined_threats(
                tile_type("E"),
                &targets(&riichi_safe),
                &riichi_safe
            ),
            Some(OpponentHonorValue::SingleValueHonor)
        );

        // 逆に副露相手の河にあれば、リーチ者のダブ東だけが残る。
        let open_hand_safe = base.discards_of(OPEN_HAND_TARGET, "E").build();
        assert_eq!(
            opponent_honor_value_for_combined_threats(
                tile_type("E"),
                &targets(&open_hand_safe),
                &open_hand_safe
            ),
            Some(OpponentHonorValue::DoubleWind)
        );
    }

    #[test]
    fn every_target_being_ron_safe_leaves_the_first_category_as_the_only_basis() {
        // 全 target にロンされない牌は、集約が空になっても安全根拠を第一分類が表す。
        let context = ContextSpec::combined()
            .discards_of(RIICHI_TARGET, "4s")
            .discards_of(OPEN_HAND_TARGET, "4s")
            .build();
        let targets = targets(&context);

        assert!(is_safe_against_all_threats(
            tile_type("4s"),
            &targets,
            &context
        ));
        // 集約対象が0人でもスジがあるとは扱わない。
        assert_eq!(
            suji_safety_rank_for_combined_threats(tile_type("4s"), &targets, &context),
            Some(SujiSafetyRank::NoSuji)
        );
        assert_eq!(
            suited_safety_rank_for_combined_threats(tile_type("4s"), &targets, &context),
            Some(SuitedSafetyRank::NoSafety)
        );
        assert_eq!(
            opponent_honor_value_for_combined_threats(tile_type("4s"), &targets, &context),
            None
        );
        assert_eq!(
            combined_defense_category(tile_type("4s"), &targets, &context),
            Some(CombinedDefenseCategory::SafeAgainstAllThreats)
        );
    }

    #[test]
    fn the_wall_is_shared_with_the_existing_defense() {
        // 壁は見え牌由来で target に依らないので、既存 helper をそのまま共有する。
        let context = ContextSpec::combined()
            .visible((0..4).map(|copy| tile_copy("2s", copy)).collect())
            .build();
        let targets = targets(&context);

        assert_eq!(wall_rank(tile_type("1s"), &context), WallRank::NoChance);
        assert_eq!(
            suited_safety_rank_for_combined_threats(tile_type("1s"), &targets, &context),
            Some(SuitedSafetyRank::NoChance)
        );
    }

    #[test]
    fn the_suited_safety_evidence_is_shared_with_the_existing_defense() {
        // 壁とスジを別々に組み立てず、ロンされ得る target を決めて共有 helper へ渡すだけ。
        let context = ContextSpec::combined()
            .discards_of(RIICHI_TARGET, "2m 8m")
            .discards_of(OPEN_HAND_TARGET, "2m 8m")
            .visible(
                (0..3)
                    .map(|copy| tile_copy("3m", copy))
                    .chain((0..4).map(|copy| tile_copy("6m", copy)))
                    .collect(),
            )
            .build();
        let targets = targets(&context);
        let five_man = tile_type("5m");
        let evidence = suited_safety_evidence_for_combined_threats(five_man, &targets, &context)
            .expect("数牌の evidence");

        assert_eq!(evidence.wall_rank, WallRank::OneChance);
        assert_eq!(evidence.suji_rank, SujiSafetyRank::Suji);
        assert_eq!(
            Some(evidence),
            suited_safety_evidence_for_players(
                five_man,
                &[RIICHI_TARGET, OPEN_HAND_TARGET],
                &context
            )
        );
        assert_eq!(
            suited_safety_rank_for_combined_threats(five_man, &targets, &context),
            Some(evidence.legacy_rank())
        );
        assert_eq!(
            suited_safety_evidence_for_combined_threats(tile_type("N"), &targets, &context),
            None
        );
    }

    // ---- exact ron-risk vector ----

    #[test]
    fn riichi_and_actionable_open_hand_share_one_exact_risk_vector() {
        let context = exact_combined_context(&[("1m", 1), ("2m", 2)]).build();
        let legal_actions = vec![dahai("2m"), dahai("1m")];
        let evaluation = evaluate_combined_threat_defense_fallback_action_with_kind(
            &context,
            &legal_actions,
            &targets(&context),
        );

        assert_eq!(
            evaluation.selected,
            Some((&legal_actions[1], CombinedDefenseCategory::ExactRonRisk))
        );
        let vectors = evaluation.ron_risk_vectors.as_deref().unwrap();
        for action in &legal_actions {
            assert_eq!(
                player_ron_risk_evidence_for_action(Some(vectors), action)
                    .unwrap()
                    .iter()
                    .map(|evidence| evidence.player)
                    .collect::<Vec<_>>(),
                vec![RIICHI_TARGET, OPEN_HAND_TARGET]
            );
        }

        let facts = player_threat_facts_from_context(&context);
        let diagnostic = CombinedDefenseDiagnostic::from_evaluation(
            &context,
            &legal_actions,
            &facts,
            &classify_open_hand_threats(&facts),
            &evaluation,
        );
        assert_eq!(
            diagnostic.selected_category(),
            Some(CombinedDefenseCategory::ExactRonRisk)
        );
        assert!(diagnostic.candidates.iter().all(|candidate| {
            candidate.category == Some(CombinedDefenseCategory::ExactRonRisk)
                && candidate
                    .player_ron_risk_evidence
                    .as_ref()
                    .is_some_and(|evidence| evidence.len() == 2)
        }));
        assert_eq!(
            diagnostic
                .selected
                .as_ref()
                .unwrap()
                .selected_player_ron_risk_evidence
                .as_ref()
                .unwrap()
                .len(),
            2
        );
    }

    #[test]
    fn mixed_threat_ratios_use_worst_first_lexicographic_minimax() {
        // A=[1/10, 9/10], B=[5/10, 5/10]。target kind による weight を入れず B が安全。
        let candidate_a = [
            PlayerRonRiskEvidence {
                player: RIICHI_TARGET,
                evidence: RonRiskEvidence {
                    ron_capable_weight: 1,
                    tenpai_weight: 10,
                },
            },
            PlayerRonRiskEvidence {
                player: OPEN_HAND_TARGET,
                evidence: RonRiskEvidence {
                    ron_capable_weight: 9,
                    tenpai_weight: 10,
                },
            },
        ];
        let candidate_b = [
            PlayerRonRiskEvidence {
                player: RIICHI_TARGET,
                evidence: RonRiskEvidence {
                    ron_capable_weight: 5,
                    tenpai_weight: 10,
                },
            },
            PlayerRonRiskEvidence {
                player: OPEN_HAND_TARGET,
                evidence: RonRiskEvidence {
                    ron_capable_weight: 5,
                    tenpai_weight: 10,
                },
            },
        ];
        assert_eq!(
            compare_lexicographic_minimax_ron_risk(&candidate_b, &candidate_a),
            Some(std::cmp::Ordering::Less)
        );

        let context = exact_combined_context(&[("1m", 3), ("2m", 2)])
            .discards_of(RIICHI_TARGET, "1m")
            .discards_of(OPEN_HAND_TARGET, "2m")
            .build();
        let legal_actions = vec![dahai("1m"), dahai("2m")];
        let evaluation = evaluate_combined_threat_defense_fallback_action_with_kind(
            &context,
            &legal_actions,
            &targets(&context),
        );
        let vectors = evaluation.ron_risk_vectors.as_deref().unwrap();
        let first = player_ron_risk_evidence_for_action(Some(vectors), &legal_actions[0]).unwrap();
        let second = player_ron_risk_evidence_for_action(Some(vectors), &legal_actions[1]).unwrap();

        assert_eq!(first[0].evidence.ron_capable_weight, 0);
        assert!(first[1].evidence.ron_capable_weight > second[0].evidence.ron_capable_weight);
        assert_eq!(second[1].evidence.ron_capable_weight, 0);
        assert_eq!(
            compare_lexicographic_minimax_ron_risk(second, first),
            Some(std::cmp::Ordering::Less)
        );
        assert_eq!(
            evaluation.selected,
            Some((&legal_actions[1], CombinedDefenseCategory::ExactRonRisk))
        );
    }

    #[test]
    fn hard_safe_precedes_exact_without_collecting_a_vector() {
        let context = exact_combined_context(&[("1m", 1), ("2m", 2)])
            .discards_of(RIICHI_TARGET, "2m")
            .discards_of(OPEN_HAND_TARGET, "2m")
            .build();
        let legal_actions = vec![dahai("1m"), dahai("2m")];
        let evaluation = evaluate_combined_threat_defense_fallback_action_with_kind(
            &context,
            &legal_actions,
            &targets(&context),
        );

        assert_eq!(
            evaluation.selected,
            Some((
                &legal_actions[1],
                CombinedDefenseCategory::SafeAgainstAllThreats
            ))
        );
        assert!(evaluation.ron_risk_vectors.is_none());
    }

    #[test]
    fn open_hand_same_hand_passed_with_riichi_hard_safe_precedes_exact() {
        let context = exact_combined_context(&[("1m", 1), ("2m", 2)])
            .discards_of(RIICHI_TARGET, "2m")
            .same_hand_passed(OPEN_HAND_TARGET, "2m")
            .build();
        let legal_actions = vec![dahai("1m"), dahai("2m")];
        let evaluation = evaluate_combined_threat_defense_fallback_action_with_kind(
            &context,
            &legal_actions,
            &targets(&context),
        );

        assert_eq!(
            evaluation.selected,
            Some((&legal_actions[1], CombinedDefenseCategory::SameHandPassed))
        );
        assert!(evaluation.ron_risk_vectors.is_none());

        let vectors = combined_targets_dahai_actions_by_ron_risk(
            &context,
            &legal_actions,
            &[RIICHI_TARGET],
            &[OPEN_HAND_TARGET],
        )
        .unwrap();
        let evidence =
            player_ron_risk_evidence_for_action(Some(&vectors), &legal_actions[1]).unwrap();
        assert!(
            evidence
                .iter()
                .find(|evidence| evidence.player == OPEN_HAND_TARGET)
                .unwrap()
                .evidence
                .ron_capable_weight
                > 0,
            "same-hand passed must not turn exact R into zero"
        );
    }

    #[test]
    fn riichi_same_hand_passed_does_not_cover_that_target() {
        let context = exact_combined_context(&[("1m", 1), ("2m", 2)])
            .same_hand_passed(RIICHI_TARGET, "2m")
            .same_hand_passed(OPEN_HAND_TARGET, "2m")
            .build();
        let targets = targets(&context);

        assert!(!is_same_hand_passed_for_target(
            tile_type("2m"),
            ThreatDefenseTarget::riichi(RIICHI_TARGET),
            &context
        ));
        assert!(!has_same_hand_passed_for_all_threats(
            tile_type("2m"),
            &targets,
            &context
        ));
        assert_eq!(
            fallback(&context, &[dahai("2m")]).unwrap().1,
            CombinedDefenseCategory::ExactRonRisk
        );
    }

    #[test]
    fn one_unavailable_target_returns_the_whole_position_to_legacy() {
        let mut invalid_open_melds = exact_open_melds();
        invalid_open_melds.push(chi_from("1s 2s 3s"));
        let context = exact_combined_context(&[("1m", 1), ("E", 1)])
            .melds_of(OPEN_HAND_TARGET, invalid_open_melds)
            .build();
        let legal_actions = vec![dahai("1m"), dahai("E")];
        let evaluation = evaluate_combined_threat_defense_fallback_action_with_kind(
            &context,
            &legal_actions,
            &targets(&context),
        );

        assert!(
            reached_opponents_dahai_actions_by_ron_risk(&context, &legal_actions).is_some(),
            "Riichi exact remains available"
        );
        assert!(
            open_hand_targets_dahai_actions_by_ron_risk(
                &context,
                &legal_actions,
                &[OPEN_HAND_TARGET]
            )
            .is_none(),
            "OpenHand exact is unavailable"
        );
        assert!(evaluation.ron_risk_vectors.is_none());
        assert_eq!(
            evaluation.selected,
            Some((
                &legal_actions[1],
                CombinedDefenseCategory::HonorSafety(HonorSafetyRank::ThreeOrMoreVisible)
            ))
        );
    }

    #[test]
    fn exact_ratio_tie_keeps_legal_order() {
        let context = exact_combined_context(&[("1m", 1), ("2m", 1)]).build();
        let legal_actions = vec![dahai("2m"), dahai("1m")];

        assert_eq!(
            fallback(&context, &legal_actions),
            Some((
                legal_actions[0].clone(),
                CombinedDefenseCategory::ExactRonRisk
            ))
        );
    }

    #[test]
    fn exact_selection_preserves_black_five_preference() {
        let context = exact_combined_context(&[("5m", 1)]).build();
        let red_five = LegalAction::Dahai {
            tile: tile_copy("5m", 0),
        };
        let black_five = LegalAction::Dahai {
            tile: tile_copy("5m", 1),
        };

        assert_eq!(
            fallback(&context, &[red_five, black_five.clone()]),
            Some((black_five, CombinedDefenseCategory::ExactRonRisk))
        );
    }

    #[test]
    fn combined_evidence_matches_each_standalone_exact_evaluator() {
        let context = exact_combined_context(&[("1m", 1), ("2m", 2)]).build();
        let legal_actions = vec![dahai("2m"), dahai("1m")];
        let combined = combined_targets_dahai_actions_by_ron_risk(
            &context,
            &legal_actions,
            &[RIICHI_TARGET],
            &[OPEN_HAND_TARGET],
        )
        .unwrap();
        let riichi = reached_opponents_dahai_actions_by_ron_risk(&context, &legal_actions).unwrap();
        let open_hand = open_hand_targets_dahai_actions_by_ron_risk(
            &context,
            &legal_actions,
            &[OPEN_HAND_TARGET],
        )
        .unwrap();

        for action in &legal_actions {
            let mixed = player_ron_risk_evidence_for_action(Some(&combined), action).unwrap();
            assert_eq!(
                mixed[0],
                player_ron_risk_evidence_for_action(Some(&riichi), action).unwrap()[0]
            );
            assert_eq!(
                mixed[1],
                player_ron_risk_evidence_for_action(Some(&open_hand), action).unwrap()[0]
            );
        }
    }

    // ---- 防御 fallback の選択 ----

    #[test]
    fn the_fallback_is_none_without_targets() {
        // リーチ者だけの局面ではこの fallback を選ばない。
        let context = ContextSpec::new()
            .reached(RIICHI_TARGET)
            .discards_of(RIICHI_TARGET, "N")
            .build();

        assert!(targets(&context).is_empty());
        assert_eq!(fallback(&context, &[dahai("N"), dahai("5m")]), None);
    }

    #[test]
    fn the_fallback_prefers_a_tile_safe_against_all_threats() {
        // 第一分類は全 threat へのロン安全。字牌 safety より優先し、同順位では元順序を保つ。
        let context = ContextSpec::combined()
            .post_reach_passed(RIICHI_TARGET, "4s N")
            .discards_of(OPEN_HAND_TARGET, "4s N")
            .build();

        assert_eq!(
            fallback(&context, &[dahai("5m"), dahai("4s"), dahai("N")]),
            Some((dahai("4s"), CombinedDefenseCategory::SafeAgainstAllThreats))
        );
        assert_eq!(
            fallback(&context, &[dahai("5m"), dahai("N"), dahai("4s")]),
            Some((dahai("N"), CombinedDefenseCategory::SafeAgainstAllThreats))
        );
    }

    #[test]
    fn same_hand_passed_two_sou_beats_one_chance_nine_man_regression() {
        let context = ContextSpec::combined()
            .post_reach_passed(RIICHI_TARGET, "2s")
            .same_hand_passed(OPEN_HAND_TARGET, "2s")
            .visible((0..3).map(|copy| tile_copy("8m", copy)).collect())
            .build();

        assert!(is_ron_safe_for_target(
            tile_type("2s"),
            ThreatDefenseTarget::riichi(RIICHI_TARGET),
            &context
        ));
        assert_eq!(wall_rank(tile_type("9m"), &context), WallRank::OneChance);
        assert_eq!(
            fallback(&context, &[dahai("9m"), dahai("2s")]),
            Some((dahai("2s"), CombinedDefenseCategory::SameHandPassed))
        );
    }

    #[test]
    fn same_hand_passed_prefers_more_hard_safe_targets() {
        let context = ContextSpec::combined()
            .melds_of(OTHER_PLAYER, open_melds(3))
            .post_reach_passed(RIICHI_TARGET, "2s 3s")
            .same_hand_passed(OTHER_PLAYER, "2s 3s")
            .same_hand_passed(OPEN_HAND_TARGET, "2s 3s")
            .discards_of(OTHER_PLAYER, "3s")
            .build();
        let targets = targets(&context);

        for tile in [tile_type("2s"), tile_type("3s")] {
            assert_eq!(
                combined_defense_category(tile, &targets, &context),
                Some(CombinedDefenseCategory::SameHandPassed)
            );
        }
        assert_eq!(
            fallback(&context, &[dahai("2s"), dahai("3s")]),
            Some((dahai("3s"), CombinedDefenseCategory::SameHandPassed))
        );
    }

    #[test]
    fn same_hand_passed_hard_safe_count_tie_keeps_legal_order() {
        let context = ContextSpec::combined()
            .melds_of(OTHER_PLAYER, open_melds(3))
            .post_reach_passed(RIICHI_TARGET, "2s 3s")
            .same_hand_passed(OTHER_PLAYER, "2s 3s")
            .same_hand_passed(OPEN_HAND_TARGET, "2s 3s")
            .build();

        assert_eq!(
            fallback(&context, &[dahai("3s"), dahai("2s")]),
            Some((dahai("3s"), CombinedDefenseCategory::SameHandPassed))
        );
    }

    #[test]
    fn a_tile_safe_against_only_one_target_is_not_the_first_category() {
        let context = ContextSpec::combined()
            .discards_of(RIICHI_TARGET, "4s")
            .build();

        assert_eq!(
            fallback(&context, &[dahai("4s"), dahai("N")]),
            Some((
                dahai("N"),
                CombinedDefenseCategory::HonorSafety(HonorSafetyRank::NoVisible)
            ))
        );
    }

    #[test]
    fn the_fallback_uses_the_honor_safety_rank_of_the_existing_defense() {
        // 字牌は既存 Defense と同じ見え枚数の4段階。
        let context = ContextSpec::combined()
            .visible(vec![tile_copy("N", 0), tile_copy("N", 1)])
            .build();

        assert_eq!(
            honor_safety_rank(tile_type("N"), &context),
            Some(HonorSafetyRank::TwoVisible)
        );
        assert_eq!(
            honor_safety_rank(tile_type("W"), &context),
            Some(HonorSafetyRank::NoVisible)
        );
        assert_eq!(
            fallback(&context, &[dahai("5m"), dahai("W"), dahai("N")]),
            Some((
                dahai("N"),
                CombinedDefenseCategory::HonorSafety(HonorSafetyRank::TwoVisible)
            ))
        );
    }

    #[test]
    fn the_fallback_breaks_an_honor_tie_by_the_opponent_honor_value() {
        // 場風 東、player 1 の自風は東、player 3 の自風は西。見え枚数が同じなら客風の南を先に切る。
        let context = ContextSpec::combined().build();
        let targets = targets(&context);

        assert_eq!(
            opponent_honor_value_for_combined_threats(tile_type("S"), &targets, &context),
            Some(OpponentHonorValue::GuestWind)
        );
        assert_eq!(
            opponent_honor_value_for_combined_threats(tile_type("E"), &targets, &context),
            Some(OpponentHonorValue::DoubleWind)
        );
        assert_eq!(
            fallback(&context, &[dahai("E"), dahai("S")]),
            Some((
                dahai("S"),
                CombinedDefenseCategory::HonorSafety(HonorSafetyRank::NoVisible)
            ))
        );
    }

    #[test]
    fn an_unknown_honor_value_is_not_ordered_as_a_guest_wind() {
        let mut spec = ContextSpec::combined();
        spec.round_wind = None;
        spec.oya = None;
        let context = spec.build();
        let targets = targets(&context);

        assert_eq!(
            opponent_honor_value_for_combined_threats(tile_type("S"), &targets, &context),
            None
        );
        assert_eq!(
            fallback(&context, &[dahai("P"), dahai("S")]),
            Some((
                dahai("P"),
                CombinedDefenseCategory::HonorSafety(HonorSafetyRank::NoVisible)
            ))
        );
    }

    // 数牌の安全度を段階的に作る局面。スジは両 target の河から求める。
    //
    // - 1s: 2s が4枚見えているので NoChance
    // - 9s: 8s が3枚見えているので OneChance
    // - 5m: 2m / 8m が両 target の河にあるので両側スジ
    // - 6p: 3p が両 target の河にあるので片スジ
    // - 4m: 壁もスジも無い NoSafety
    fn suited_safety_context() -> GameContext {
        let mut visible: Vec<TileId> = (0..4).map(|copy| tile_copy("2s", copy)).collect();
        visible.extend((0..3).map(|copy| tile_copy("8s", copy)));

        ContextSpec::combined()
            .discards_of(RIICHI_TARGET, "2m 8m 3p")
            .discards_of(OPEN_HAND_TARGET, "2m 8m 3p")
            .visible(visible)
            .build()
    }

    #[test]
    fn the_fallback_prefers_the_safest_suited_tile() {
        // 数牌の順位は既存 Defense と同じ NoChance → OneChance → Suji → HalfSuji。
        let context = suited_safety_context();
        let expectations = [
            ("1s", SuitedSafetyRank::NoChance),
            ("9s", SuitedSafetyRank::OneChance),
            ("5m", SuitedSafetyRank::Suji),
            ("6p", SuitedSafetyRank::HalfSuji),
        ];

        for index in 0..expectations.len() {
            let mut legal_actions = vec![dahai("4m")];
            legal_actions.extend(expectations[index..].iter().map(|(mjai, _)| dahai(mjai)));

            let (mjai, rank) = expectations[index];
            assert_eq!(
                fallback(&context, &legal_actions),
                Some((dahai(mjai), CombinedDefenseCategory::SuitedSafety(rank))),
                "{mjai}"
            );
        }
    }

    #[test]
    fn a_suited_no_safety_candidate_is_not_selected() {
        // NoSafety しか無い場合は複合 threat 用の fallback を選ばない。既存 Defense と同じ。
        let context = ContextSpec::combined().build();
        let targets = targets(&context);

        for mjai in ["4m", "5p"] {
            assert_eq!(
                suited_safety_rank_for_combined_threats(tile_type(mjai), &targets, &context),
                Some(SuitedSafetyRank::NoSafety),
                "{mjai}"
            );
        }
        assert_eq!(fallback(&context, &[dahai("4m"), dahai("5p")]), None);
    }

    #[test]
    fn the_fallback_prefers_the_black_five_of_the_same_tile_type() {
        // 牌種を決めたあと、同じ牌種内では黒5を優先する既存 semantics を保つ。
        let safe = ContextSpec::combined()
            .discards_of(RIICHI_TARGET, "5s")
            .discards_of(OPEN_HAND_TARGET, "5s")
            .build();
        let red_five = LegalAction::Dahai {
            tile: tile_copy("5s", 0),
        };
        let black_five = LegalAction::Dahai {
            tile: tile_copy("5s", 1),
        };
        assert!(tile_copy("5s", 0).is_red());
        assert!(!tile_copy("5s", 1).is_red());

        assert_eq!(
            fallback(&safe, &[red_five, black_five.clone()]),
            Some((black_five, CombinedDefenseCategory::SafeAgainstAllThreats))
        );

        // 数牌 safety 経由でも同じ。
        let suji = suited_safety_context();
        let red_five_man = LegalAction::Dahai {
            tile: tile_copy("5m", 0),
        };
        let black_five_man = LegalAction::Dahai {
            tile: tile_copy("5m", 1),
        };

        assert_eq!(
            fallback(&suji, &[red_five_man, black_five_man.clone()]),
            Some((
                black_five_man,
                CombinedDefenseCategory::SuitedSafety(SuitedSafetyRank::Suji)
            ))
        );
    }

    #[test]
    fn the_selected_action_matches_its_category_helper() {
        let context = suited_safety_context();
        let legal_actions = vec![dahai("4m"), dahai("6p"), dahai("5m"), dahai("1s")];
        let (action, category) = fallback(&context, &legal_actions).expect("防御 fallback");
        let LegalAction::Dahai { tile } = action else {
            panic!("Dahai を選ぶ");
        };

        assert_eq!(
            Some(category),
            combined_defense_category(tile.tile_type(), &targets(&context), &context)
        );
    }

    #[test]
    fn the_action_only_wrapper_matches_the_selector() {
        let context = suited_safety_context();
        let legal_actions = vec![dahai("4m"), dahai("1s")];
        let targets = targets(&context);

        assert_eq!(
            select_combined_threat_defense_fallback_action(&context, &legal_actions, &targets),
            select_combined_threat_defense_fallback_action_with_kind(
                &context,
                &legal_actions,
                &targets
            )
            .map(|(action, _)| action)
        );
    }

    #[test]
    fn the_combined_safety_can_differ_from_the_riichi_only_safety() {
        // リーチ者だけを見ると安全でも、副露相手に対して危険なら複合 threat では選ばない。
        let context = ContextSpec::combined()
            .discards_of(RIICHI_TARGET, "2m 8m")
            .build();
        let targets = targets(&context);

        assert_eq!(
            suited_safety_rank_for_all_reached(tile_type("5m"), &context),
            Some(SuitedSafetyRank::Suji)
        );
        assert_eq!(
            suited_safety_rank_for_combined_threats(tile_type("5m"), &targets, &context),
            Some(SuitedSafetyRank::NoSafety)
        );
    }

    // ---- 構造化診断 ----

    #[test]
    fn the_diagnostic_reflects_the_selected_fallback() {
        // 診断は production selector の結果を写すだけで、選び直さない。
        let context = suited_safety_context();
        let legal_actions = vec![dahai("4m"), dahai("1s")];
        let selected = select_combined_threat_defense_fallback_action_with_kind(
            &context,
            &legal_actions,
            &targets(&context),
        );
        let diagnostic =
            CombinedDefenseDiagnostic::from_context(&context, &legal_actions, selected);

        assert!(diagnostic.has_target());
        assert_eq!(
            diagnostic.selected,
            Some(CombinedDefenseSelectionDiagnostic {
                selected_action: dahai("1s"),
                selected_category: CombinedDefenseCategory::SuitedSafety(
                    SuitedSafetyRank::NoChance
                ),
                selected_player_ron_risk_evidence: None,
            })
        );
        assert_eq!(
            diagnostic.selected_category(),
            Some(CombinedDefenseCategory::SuitedSafety(
                SuitedSafetyRank::NoChance
            ))
        );
        assert_eq!(
            diagnostic
                .candidates
                .iter()
                .map(|candidate| (candidate.tile, candidate.selected))
                .collect::<Vec<(TileType, bool)>>(),
            vec![(tile_type("4m"), false), (tile_type("1s"), true)]
        );
    }

    #[test]
    fn the_diagnostic_reports_each_targets_ron_safety() {
        let context = ContextSpec::combined()
            .post_reach_passed(RIICHI_TARGET, "4s")
            .discards_of(RIICHI_TARGET, "2s 8s")
            .build();
        let legal_actions = vec![dahai("4s"), dahai("5s")];
        let diagnostic = CombinedDefenseDiagnostic::from_context(&context, &legal_actions, None);

        let four_sou = &diagnostic.candidates[0];
        assert_eq!(
            four_sou
                .targets
                .iter()
                .map(|target| (target.player(), target.kind(), target.ron_safe))
                .collect::<Vec<(usize, ThreatDefenseTargetKind, bool)>>(),
            vec![
                (RIICHI_TARGET, ThreatDefenseTargetKind::Riichi, true),
                (
                    OPEN_HAND_TARGET,
                    ThreatDefenseTargetKind::HighOpenHand,
                    false
                ),
            ]
        );
        assert!(!four_sou.safe_against_all_threats);

        // target 単独の suji は除外前の値そのもの。集約はロン可能な target だけを見る。
        let five_sou = &diagnostic.candidates[1];
        assert_eq!(
            five_sou
                .targets
                .iter()
                .map(|target| target.suji_safety_rank)
                .collect::<Vec<Option<SujiSafetyRank>>>(),
            vec![Some(SujiSafetyRank::Suji), Some(SujiSafetyRank::NoSuji)]
        );
        assert_eq!(five_sou.suji_safety_rank, Some(SujiSafetyRank::NoSuji));
    }

    #[test]
    fn the_diagnostic_has_no_candidate_without_a_combined_threat() {
        // 複合 threat でない局面は target も候補も持たない。
        let context = ContextSpec::new().reached(RIICHI_TARGET).build();
        let diagnostic = CombinedDefenseDiagnostic::from_context(&context, &[dahai("4s")], None);

        assert!(!diagnostic.has_target());
        assert!(diagnostic.targets.is_empty());
        assert!(diagnostic.candidates.is_empty());
        assert_eq!(diagnostic.selected_category(), None);
    }
}
