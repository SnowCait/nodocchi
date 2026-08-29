//! `High` [`OpenHandThreatLevel`](crate::open_hand_threat::OpenHandThreatLevel) の非リーチ副露相手に対する防御 safety の source of truth。
//!
//! 判定は既存 Defense の pure helper をそのまま共有し、字牌の見え枚数・壁・スジ・役牌価値を
//! 別実装しない。リーチ者向けの `*_for_all_reached` と違うのは対象 player 集合の決め方と、
//! 「現物相当」の根拠だけ。
//!
//! ロン安全の根拠は対象 player 自身の河と、その player の手牌が最後に変化してから他家から
//! 切られて通った一時安全牌。`post_reach_passed_tiles` はリーチ固有なので流用しない。
//!
//! 防御 fallback の action 選択は [`select_open_hand_defense_fallback_action_with_kind`] が
//! source of truth で、[`OpenHandDefenseDiagnostic`] はその結果を写すだけにする。

use crate::action::{LegalAction, prefer_black_five_for_action};
use crate::context::GameContext;
use crate::defense::{
    HonorSafetyRank, OpponentHonorValue, SuitedSafetyEvidence, SuitedSafetyRank, SujiSafetyRank,
    WallRank, honor_dahai_actions_by_safety_with, honor_safety_rank, is_discarded_by_all_players,
    is_discarded_by_player, opponent_honor_value_for_players, suited_dahai_actions_by_safety_with,
    suited_safety_evidence_for_players, suji_safety_rank_for, suji_safety_rank_for_players,
};
use crate::open_hand_threat::{OpenHandThreatAssessment, classify_open_hand_threats};
use crate::threat::{PlayerThreatFacts, player_threat_facts_from_context};
use bot_logic::TileType;

/// [`OpenHandThreatLevel::High`](crate::open_hand_threat::OpenHandThreatLevel::High) と分類された席を防御の target として集める pure helper。
///
/// 分類そのものは行わず、渡された classification をそのまま source of truth にする。配列の
/// index が席番号で、戻り値は席順。[`OpenHandThreatLevel::Present`](crate::open_hand_threat::OpenHandThreatLevel::Present)
/// は今回の target にしない。
///
/// 自分の席・リーチ済みの席・`player_id` 不明の席は
/// [`OpenHandThreatAssessment::NotApplicable`] なので、level を持たず target にもならない。
pub fn high_open_hand_threat_players(assessments: &[OpenHandThreatAssessment; 4]) -> Vec<usize> {
    assessments
        .iter()
        .enumerate()
        .filter(|(_, assessment)| assessment.is_high())
        .map(|(player, _)| player)
        .collect()
}

/// 全4席分の facts から target を集める adapter。分類は [`classify_open_hand_threats`] が行う。
pub fn high_open_hand_threat_players_from_facts(facts: &[PlayerThreatFacts; 4]) -> Vec<usize> {
    high_open_hand_threat_players(&classify_open_hand_threats(facts))
}

/// `GameContext` から target を集める adapter。facts の構築も分類も既存経路を共有する。
pub fn high_open_hand_threat_players_from_context(context: &GameContext) -> Vec<usize> {
    high_open_hand_threat_players_from_facts(&player_threat_facts_from_context(context))
}

/// 非リーチ副露 target にこの牌でロンされないと言えるか判定する source of truth。
pub fn is_ron_safe_for_open_hand_target(
    tile: TileType,
    player: usize,
    context: &GameContext,
) -> bool {
    is_discarded_by_player(tile, player, context) || context.is_temporary_passed(tile, player)
}

/// 全 target 自身の河にある牌か判定する。target が0人なら `false`。
///
/// 判定は [`is_discarded_by_player`] の集約 ([`is_discarded_by_all_players`]) で、
/// `post_reach_passed_tiles` は見ない。他家が切っただけで通った牌も恒常的な安全牌にしない。
pub fn is_discarded_by_all_open_hand_threats(
    tile: TileType,
    targets: &[usize],
    context: &GameContext,
) -> bool {
    is_discarded_by_all_players(tile, targets, context)
}

/// 全 OpenHand target にロンされないか。target が0人なら `false`。
pub fn is_ron_safe_for_all_open_hand_targets(
    tile: TileType,
    targets: &[usize],
    context: &GameContext,
) -> bool {
    !targets.is_empty()
        && targets
            .iter()
            .all(|&player| is_ron_safe_for_open_hand_target(tile, player, context))
}

// 対象牌でまだロンされ得る target。target ごとに評価が変わる safety はこの集合だけを集約する。
//
// 対象牌が本人の河または現在有効な一時通過牌にある target はロンできないため、その target の
// 評価が全体の安全度を悪化させないよう除外する。`post_reach_passed_tiles` は使わない。
//
// 全 target が対象牌にロンできない場合は空になる。空集合は「安全と確定した」ではなく
// 「target ごとの評価が無い」で、その場合の安全根拠は
// [`OpenHandDefenseCategory::SafeAgainstAllTargets`] が表す。
fn ron_capable_targets(tile: TileType, targets: &[usize], context: &GameContext) -> Vec<usize> {
    targets
        .iter()
        .copied()
        .filter(|&player| !is_ron_safe_for_open_hand_target(tile, player, context))
        .collect()
}

/// target に対する役牌価値のうち最も危険な評価。数牌は対象外で `None`。
///
/// 対象牌にロンできない target は集約対象から除外する ([`ron_capable_targets`])。target が
/// いない場合、全 target にロンされない場合、情報不足で
/// 誰の分も確定できない場合は `None` (unknown)。情報不足を `GuestWind` と推測しない。
///
/// 集約は既存 Defense と同じ [`opponent_honor_value_for_players`]。
pub fn opponent_honor_value_for_open_hand_threats(
    tile: TileType,
    targets: &[usize],
    context: &GameContext,
) -> Option<OpponentHonorValue> {
    opponent_honor_value_for_players(tile, &ron_capable_targets(tile, targets, context), context)
}

/// target の河に対するスジ安全度。数牌なら `Some`、字牌なら `None`。
///
/// 対象牌にロンできない target は役牌価値と同じく集約対象から除外する
/// ([`ron_capable_targets`])。その target の河にスジが無いことを
/// 全体の危険度に持ち込まない。
///
/// 残った target の [`suji_safety_rank_for`] の最小値(最も危険な評価)を採る。target が0人の
/// 場合と全 target にロンされない場合は `NoSuji` で、スジがあるとは扱わない。判定も集約も既存
/// Defense の [`suji_safety_rank_for_players`] と共有する。
pub fn suji_safety_rank_for_open_hand_threats(
    tile: TileType,
    targets: &[usize],
    context: &GameContext,
) -> Option<SujiSafetyRank> {
    suji_safety_rank_for_players(tile, &ron_capable_targets(tile, targets, context), context)
}

/// target に対する数牌の防御 evidence。字牌は対象外で `None`。
///
/// 壁とスジをここで組み立てず、まだロンされ得る target ([`ron_capable_targets`]) を決めて
/// 既存 Defense の [`suited_safety_evidence_for_players`] へ渡すだけにする。evidence の意味は
/// リーチ向けと同じ。
pub fn suited_safety_evidence_for_open_hand_threats(
    tile: TileType,
    targets: &[usize],
    context: &GameContext,
) -> Option<SuitedSafetyEvidence> {
    suited_safety_evidence_for_players(tile, &ron_capable_targets(tile, targets, context), context)
}

/// target に対する数牌の安全度を壁 / スジから分類する。字牌は対象外で `None`。
///
/// [`suited_safety_evidence_for_open_hand_threats`] の evidence を
/// [`SuitedSafetyEvidence::legacy_rank`] で潰す薄い wrapper。
pub fn suited_safety_rank_for_open_hand_threats(
    tile: TileType,
    targets: &[usize],
    context: &GameContext,
) -> Option<SuitedSafetyRank> {
    suited_safety_evidence_for_open_hand_threats(tile, targets, context)
        .map(SuitedSafetyEvidence::legacy_rank)
}

/// target に対する防御候補の大分類。
///
/// 優先順位は既存 Defense ([`DefenseFallbackKind`](crate::defense::DefenseFallbackKind)) に
/// 合わせて `SafeAgainstAllTargets` → `HonorSafety` → `SuitedSafety`。
///
/// 第一分類を `Genbutsu` と呼ばないのは、リーチ固有の `post_reach_passed_tiles` と、手牌変化まで
/// だけ有効な一時通過牌の意味と寿命を混ぜないため。
///
/// 現時点では診断専用で、この順位で action を選ぶことはしない。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpenHandDefenseCategory {
    /// 本人の河または現在有効な一時通過牌により、全 target にロンされない。
    SafeAgainstAllTargets,
    HonorSafety(HonorSafetyRank),
    SuitedSafety(SuitedSafetyRank),
}

/// 牌1つ分の防御候補の大分類を求める pure helper。
///
/// 字牌でも数牌でもない牌は無いため、実際に `None` になるのは分類できない場合だけ。
pub fn open_hand_defense_category(
    tile: TileType,
    targets: &[usize],
    context: &GameContext,
) -> Option<OpenHandDefenseCategory> {
    if is_ron_safe_for_all_open_hand_targets(tile, targets, context) {
        return Some(OpenHandDefenseCategory::SafeAgainstAllTargets);
    }
    if let Some(rank) = honor_safety_rank(tile, context) {
        return Some(OpenHandDefenseCategory::HonorSafety(rank));
    }
    suited_safety_rank_for_open_hand_threats(tile, targets, context)
        .map(OpenHandDefenseCategory::SuitedSafety)
}

/// 合法 Dahai のうち、全 target にロンされない牌を元の順序を保って抽出する。
///
/// 根拠は [`is_ron_safe_for_all_open_hand_targets`]。本人の河と現在有効な一時通過牌を含み、
/// target が0人なら空。
pub fn safe_against_all_targets_dahai_actions<'a>(
    legal_actions: &'a [LegalAction],
    targets: &[usize],
    context: &GameContext,
) -> Vec<&'a LegalAction> {
    legal_actions
        .iter()
        .filter(|action| match action {
            LegalAction::Dahai { tile } => {
                is_ron_safe_for_all_open_hand_targets(tile.tile_type(), targets, context)
            }
            _ => false,
        })
        .collect()
}

/// 合法 Dahai のうち字牌のみを、target に対する安全度順に並べる。
///
/// 並べ替えは既存 Defense の [`honor_dahai_actions_by_safety_with`] と共有し、副露相手用の
/// sorting を別に持たない。役牌価値だけを
/// [`opponent_honor_value_for_open_hand_threats`] へ差し替える。
pub fn open_hand_honor_dahai_actions_by_safety<'a>(
    legal_actions: &'a [LegalAction],
    targets: &[usize],
    context: &GameContext,
) -> Vec<(&'a LegalAction, HonorSafetyRank)> {
    honor_dahai_actions_by_safety_with(legal_actions, context, |tile| {
        opponent_honor_value_for_open_hand_threats(tile, targets, context)
    })
}

/// 合法 Dahai のうち数牌のみを、target に対する安全度順に並べる。
///
/// 並べ替えは既存 Defense の [`suited_dahai_actions_by_safety_with`] と共有し、安全度だけを
/// [`suited_safety_rank_for_open_hand_threats`] へ差し替える。
pub fn open_hand_suited_dahai_actions_by_safety<'a>(
    legal_actions: &'a [LegalAction],
    targets: &[usize],
    context: &GameContext,
) -> Vec<(&'a LegalAction, SuitedSafetyRank)> {
    suited_dahai_actions_by_safety_with(legal_actions, |tile| {
        suited_safety_rank_for_open_hand_threats(tile, targets, context)
    })
}

/// High OpenHandThreat 相手に対する防御 fallback を優先順位付きで選ぶ production selector。
///
/// [`OpenHandDefenseCategory`] の並びどおり、全 target へのロン安全 → 字牌 safety → 数牌 safety の
/// 順に評価し、選ばれた大分類を添えて返す。target が0人なら `None`。
///
/// - `SafeAgainstAllTargets`: 全 target にロンされない牌。同順位では合法 Dahai の元順序を保つ。
/// - `HonorSafety`: 見え枚数の安全度 → 役牌価値 → 元の順序。既存リーチ Defense と同じ ranking。
/// - `SuitedSafety`: 壁 / スジを統合した安全度順。既存リーチ Defense と同じく
///   [`SuitedSafetyRank::NoSafety`] は fallback として選ばない。
///
/// いずれも牌種を決めたあと、その牌種内では [`prefer_black_five_for_action`] で黒5を優先する。
/// 牌種選択・大分類・安全度 rank は変えず、物理牌だけを黒牌へ正規化する。
///
/// リーチ者向けの防御 fallback ([`select_defense_fallback_action_with_kind`](crate::defense::select_defense_fallback_action_with_kind))
/// とは別経路で、両者の safety を1つに集約することはしない。リーチ者がいる局面でこちらを使うか
/// どうかは呼び出し側の責務。
pub fn select_open_hand_defense_fallback_action_with_kind<'a>(
    context: &GameContext,
    legal_actions: &'a [LegalAction],
    targets: &[usize],
) -> Option<(&'a LegalAction, OpenHandDefenseCategory)> {
    if targets.is_empty() {
        return None;
    }

    if let Some(action) = safe_against_all_targets_dahai_actions(legal_actions, targets, context)
        .into_iter()
        .next()
    {
        let action = prefer_black_five_for_action(legal_actions, action);
        return Some((action, OpenHandDefenseCategory::SafeAgainstAllTargets));
    }

    if let Some((action, rank)) =
        open_hand_honor_dahai_actions_by_safety(legal_actions, targets, context)
            .into_iter()
            .next()
    {
        let action = prefer_black_five_for_action(legal_actions, action);
        return Some((action, OpenHandDefenseCategory::HonorSafety(rank)));
    }

    if let Some((action, rank)) =
        open_hand_suited_dahai_actions_by_safety(legal_actions, targets, context)
            .into_iter()
            .find(|(_, rank)| *rank != SuitedSafetyRank::NoSafety)
    {
        let action = prefer_black_five_for_action(legal_actions, action);
        return Some((action, OpenHandDefenseCategory::SuitedSafety(rank)));
    }

    None
}

/// 防御 fallback の action だけを返す薄い wrapper。
pub fn select_open_hand_defense_fallback_action<'a>(
    context: &GameContext,
    legal_actions: &'a [LegalAction],
    targets: &[usize],
) -> Option<&'a LegalAction> {
    select_open_hand_defense_fallback_action_with_kind(context, legal_actions, targets)
        .map(|(action, _)| action)
}

/// target 1人に対する safety。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OpenHandDefenseTargetSafety {
    /// target の席。
    pub player: usize,
    /// この target 自身の河に同じ牌種があるか ([`is_discarded_by_player`])。
    /// `post_reach_passed_tiles` は含まない。
    pub discarded_by_target: bool,
    /// 本人の河または現在有効な一時通過牌により、この target にロンされないか。
    pub ron_safe: bool,
    /// この target の河に対する [`suji_safety_rank_for`]。字牌では `None`。
    ///
    /// この target 単独の評価そのもので、`discarded_by_target` による除外は行わない。集約側
    /// ([`OpenHandDefenseCandidateDiagnostic::suji_safety_rank`]) は
    /// `discarded_by_target` が `true` の target を外すため、両者の値は一致しないことがある。
    pub suji_safety_rank: Option<SujiSafetyRank>,
}

/// 合法 Dahai 1件ごとの、High OpenHandThreat 相手に対する防御評価。
///
/// production で使う pure な safety helper の結果をそのまま持つ解析用データで、表示のために
/// safety を計算し直さない。これ自体が action 選択を行うこともない。選択の source of truth は
/// [`select_open_hand_defense_fallback_action_with_kind`] であり、`selected` はその結果を写した
/// もの。
///
/// 数牌では `wall_rank` / `suji_safety_rank` / `suited_safety_rank` が `Some`、字牌では
/// `honor_safety_rank` が `Some` になり、無関係なフィールドは `None`。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenHandDefenseCandidateDiagnostic {
    /// 対象の合法 Dahai。物理牌(赤5 / 黒5)の区別を保持する。
    pub action: LegalAction,
    /// `action` の牌種。
    pub tile: TileType,
    /// この候補が OpenHand 防御 fallback として実際に選ばれたか。
    pub selected: bool,
    /// target ごとの safety。席順で、target が0人なら空。
    pub targets: Vec<OpenHandDefenseTargetSafety>,
    /// 全 target 自身の河にあるか。target が0人なら `false`。
    pub discarded_by_all_targets: bool,
    /// 全 target に対して本人の河または一時通過牌によりロン安全か。
    pub ron_safe_for_all_targets: bool,
    pub honor_safety_rank: Option<HonorSafetyRank>,
    /// target に対する [`opponent_honor_value_for_open_hand_threats`] の結果。数牌では `None`。
    pub opponent_honor_value: Option<OpponentHonorValue>,
    /// target に対する [`suited_safety_evidence_for_open_hand_threats`] の結果そのもの。
    ///
    /// 壁とスジを潰さずに持つので、`suited_safety_rank` が壁由来の `OneChance` / `NoChance` に
    /// なっている場合でも、同時にスジが成立していたかどうかを確認できる。字牌では `None`。
    pub suited_safety_evidence: Option<SuitedSafetyEvidence>,
    pub wall_rank: Option<WallRank>,
    /// target に対する [`suji_safety_rank_for_open_hand_threats`] の結果そのもの。
    ///
    /// 壁と統合する前の純粋なスジ評価なので、`suited_safety_rank` が壁由来の
    /// `OneChance` / `NoChance` になっている場合でも `HalfSuji` と `NoSuji` を区別できる。
    /// 対象牌が自身の河にある target は集約対象から外れるため、`targets` の
    /// `suji_safety_rank` をそのまま最小値へ潰した値とは一致しないことがある。
    pub suji_safety_rank: Option<SujiSafetyRank>,
    pub suited_safety_rank: Option<SuitedSafetyRank>,
    /// [`open_hand_defense_category`] による大分類。
    pub category: Option<OpenHandDefenseCategory>,
}

impl OpenHandDefenseCandidateDiagnostic {
    /// 合法 Dahai 1件から防御評価を構築する pure helper。Dahai 以外の action では `None`。
    ///
    /// `selected` は production selector の結果をそのまま渡す。ここで選び直さない。
    pub fn for_dahai_action(
        context: &GameContext,
        action: &LegalAction,
        targets: &[usize],
        selected: bool,
    ) -> Option<Self> {
        let LegalAction::Dahai { tile } = action else {
            return None;
        };
        let tile_type = tile.tile_type();
        let evidence = suited_safety_evidence_for_open_hand_threats(tile_type, targets, context);

        Some(Self {
            action: action.clone(),
            tile: tile_type,
            selected,
            targets: targets
                .iter()
                .map(|&player| OpenHandDefenseTargetSafety {
                    player,
                    discarded_by_target: is_discarded_by_player(tile_type, player, context),
                    ron_safe: is_ron_safe_for_open_hand_target(tile_type, player, context),
                    suji_safety_rank: suji_safety_rank_for(tile_type, player, context),
                })
                .collect(),
            discarded_by_all_targets: is_discarded_by_all_open_hand_threats(
                tile_type, targets, context,
            ),
            ron_safe_for_all_targets: is_ron_safe_for_all_open_hand_targets(
                tile_type, targets, context,
            ),
            honor_safety_rank: honor_safety_rank(tile_type, context),
            opponent_honor_value: opponent_honor_value_for_open_hand_threats(
                tile_type, targets, context,
            ),
            suited_safety_evidence: evidence,
            wall_rank: evidence.map(|evidence| evidence.wall_rank),
            suji_safety_rank: evidence.map(|evidence| evidence.suji_rank),
            suited_safety_rank: evidence.map(SuitedSafetyEvidence::legacy_rank),
            category: open_hand_defense_category(tile_type, targets, context),
        })
    }

    /// 合法 action のうち Dahai だけを、元の順序を保って防御評価へ変換する。
    ///
    /// `selected_action` は OpenHand 防御 fallback として実際に選ばれた action。一致する候補の
    /// `selected` だけが `true` になる。
    pub fn for_legal_actions(
        context: &GameContext,
        legal_actions: &[LegalAction],
        targets: &[usize],
        selected_action: Option<&LegalAction>,
    ) -> Vec<Self> {
        legal_actions
            .iter()
            .filter_map(|action| {
                Self::for_dahai_action(context, action, targets, selected_action == Some(action))
            })
            .collect()
    }
}

/// 採用された OpenHand 防御 fallback の内訳。
///
/// [`select_open_hand_defense_fallback_action_with_kind`] の結果をそのまま写したもので、
/// 診断側で選び直さない。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenHandDefenseSelectionDiagnostic {
    /// 実際に選ばれた合法 Dahai。物理牌(赤5 / 黒5)の区別を保持する。
    pub selected_action: LegalAction,
    /// その action が選ばれた大分類。
    pub selected_category: OpenHandDefenseCategory,
}

/// High OpenHandThreat 相手に対する防御 safety の構造化診断。
///
/// `targets` が空の局面は「OpenHand Defense target なし」で、候補評価も作らない。target がいない
/// ことを safety の値で表さないための区別。
///
/// `selected` は OpenHand 防御 fallback を実際に採用した場合だけ `Some` になる。採用しなかった
/// 局面(押し引きが `Fold` ではない、安全牌候補が無い、リーチ者がいるので既存 Defense を使った
/// など)では `None` で、候補評価だけが残る。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenHandDefenseDiagnostic {
    /// [`OpenHandThreatLevel::High`](crate::open_hand_threat::OpenHandThreatLevel::High) の target 席。席順。
    pub targets: Vec<usize>,
    /// 採用された OpenHand 防御 fallback。採用しなかった場合は `None`。
    pub selected: Option<OpenHandDefenseSelectionDiagnostic>,
    /// target が1人以上いる場合の全合法 Dahai の防御評価。target が0人なら空。
    pub candidates: Vec<OpenHandDefenseCandidateDiagnostic>,
}

impl OpenHandDefenseDiagnostic {
    /// 構築済みの classification と選択結果から診断を作る pure helper。
    ///
    /// 分類を作り直さないので、`Player threats` が持つ classification と target が必ず一致する。
    /// `selected` には [`select_open_hand_defense_fallback_action_with_kind`] の戻り値をそのまま
    /// 渡す。ここで防御 fallback を選び直さない。
    pub fn from_assessments(
        context: &GameContext,
        legal_actions: &[LegalAction],
        assessments: &[OpenHandThreatAssessment; 4],
        selected: Option<(&LegalAction, OpenHandDefenseCategory)>,
    ) -> Self {
        let targets = high_open_hand_threat_players(assessments);
        let candidates = if targets.is_empty() {
            Vec::new()
        } else {
            OpenHandDefenseCandidateDiagnostic::for_legal_actions(
                context,
                legal_actions,
                &targets,
                selected.map(|(action, _)| action),
            )
        };
        Self {
            targets,
            selected: selected.map(|(action, category)| OpenHandDefenseSelectionDiagnostic {
                selected_action: action.clone(),
                selected_category: category,
            }),
            candidates,
        }
    }

    /// `GameContext` から診断を作る adapter。分類は [`classify_open_hand_threats`] が行う。
    pub fn from_context(
        context: &GameContext,
        legal_actions: &[LegalAction],
        selected: Option<(&LegalAction, OpenHandDefenseCategory)>,
    ) -> Self {
        Self::from_assessments(
            context,
            legal_actions,
            &classify_open_hand_threats(&player_threat_facts_from_context(context)),
            selected,
        )
    }

    /// High OpenHandThreat の相手がいるか。
    pub fn has_target(&self) -> bool {
        !self.targets.is_empty()
    }

    /// 採用された OpenHand 防御 fallback の大分類。採用しなかった場合は `None`。
    pub fn selected_category(&self) -> Option<OpenHandDefenseCategory> {
        self.selected
            .as_ref()
            .map(|selection| selection.selected_category)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::defense::{suited_safety_evidence_for_players, wall_rank};
    use crate::meld::{Meld, MeldKind};
    use crate::open_hand_threat::{
        OpenHandThreatDecision, OpenHandThreatExclusion, OpenHandThreatLevel,
    };
    use crate::threat::player_threat_facts_from_context;
    use bot_logic::TileId;

    // 自分は player 0、親は player 1、場風は東で固定する。
    const SELF_PLAYER: usize = 0;
    const DEALER_PLAYER: usize = 1;

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

    // 牌種のみを指定した Chi。ドラも役牌も含まない。
    fn chi() -> Meld {
        Meld::new(MeldKind::Chi, tiles("1s 2s 3s"), Some(tile("1s")))
    }

    // 白の Pon。風情報が無くても確定役牌。
    fn value_pon() -> Meld {
        Meld::new(
            MeldKind::Pon,
            (0..3).map(|copy| tile_copy("P", copy)).collect(),
            Some(tile("P")),
        )
    }

    fn ankan() -> Meld {
        Meld::new(
            MeldKind::Ankan,
            (0..4).map(|copy| tile_copy("F", copy)).collect(),
            None,
        )
    }

    // 副露を count 個持つ席を作る。High 条件の「3副露以上」を満たすかどうかを count で決める。
    fn open_melds(count: usize) -> Vec<Meld> {
        (0..count).map(|_| chi()).collect()
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
    }

    impl ContextSpec {
        // 自分 player 0 / 親 player 1 / 場風 東 の既定。副露も河も無い。
        fn new() -> Self {
            Self {
                player_id: Some(SELF_PLAYER as u8),
                oya: Some(DEALER_PLAYER as u8),
                round_wind: Some(tile_type("E")),
                ..Self::default()
            }
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
        }
    }

    fn assessments(context: &GameContext) -> [OpenHandThreatAssessment; 4] {
        classify_open_hand_threats(&player_threat_facts_from_context(context))
    }

    fn targets(context: &GameContext) -> Vec<usize> {
        high_open_hand_threat_players_from_context(context)
    }

    // 3副露の非リーチ他家を player 3 に持つ局面。player 3 だけが High target になる。
    fn single_target_context() -> GameContext {
        ContextSpec::new().melds_of(3, open_melds(3)).build()
    }

    // 3副露の非リーチ他家を player 2 と player 3 に持つ局面。
    fn two_target_context() -> GameContext {
        ContextSpec::new()
            .melds_of(2, open_melds(3))
            .melds_of(3, open_melds(3))
            .build()
    }

    // ---- target の決定 ----

    #[test]
    fn a_high_open_hand_threat_is_a_target() {
        let context = single_target_context();
        assert_eq!(targets(&context), vec![3]);
        assert_eq!(
            high_open_hand_threat_players(&assessments(&context)),
            vec![3]
        );
    }

    #[test]
    fn a_present_open_hand_threat_is_not_a_target() {
        // 1副露だけの相手は Present で、今回の防御 target にしない。
        let context = ContextSpec::new().melds_of(3, open_melds(1)).build();

        assert_eq!(
            assessments(&context)[3].level(),
            Some(OpenHandThreatLevel::Present)
        );
        assert!(targets(&context).is_empty());
    }

    #[test]
    fn a_reached_player_is_not_a_target() {
        // 副露しているリーチ者は OpenHandThreat の対象外なので防御 target にもならない。
        let context = ContextSpec::new()
            .melds_of(3, open_melds(3))
            .reached(3)
            .build();

        assert_eq!(
            assessments(&context)[3],
            OpenHandThreatAssessment::NotApplicable(OpenHandThreatExclusion::Reached)
        );
        assert!(targets(&context).is_empty());
    }

    #[test]
    fn the_self_seat_is_not_a_target() {
        let context = ContextSpec::new()
            .melds_of(SELF_PLAYER, open_melds(3))
            .build();

        assert_eq!(
            assessments(&context)[SELF_PLAYER],
            OpenHandThreatAssessment::NotApplicable(OpenHandThreatExclusion::SelfSeat)
        );
        assert!(targets(&context).is_empty());
    }

    #[test]
    fn an_unknown_player_id_makes_no_target() {
        // player_id 不明の席を他家と推測して防御 target にしない。
        let mut spec = ContextSpec::new().melds_of(3, open_melds(3));
        spec.player_id = None;
        let context = spec.build();

        for player in 0..4 {
            assert_eq!(
                assessments(&context)[player].exclusion(),
                Some(OpenHandThreatExclusion::UnknownSeat),
                "player {player}"
            );
        }
        assert!(targets(&context).is_empty());
    }

    #[test]
    fn an_ankan_only_player_is_not_a_target() {
        // 暗槓は open meld ではないので OpenHandThreat が None になり、target にならない。
        let context = ContextSpec::new().melds_of(3, vec![ankan()]).build();

        assert_eq!(
            assessments(&context)[3],
            OpenHandThreatAssessment::Classified(OpenHandThreatDecision {
                level: OpenHandThreatLevel::None,
                reason: crate::open_hand_threat::OpenHandThreatReason::NoOpenMeld,
            })
        );
        assert!(targets(&context).is_empty());
    }

    #[test]
    fn every_high_player_becomes_a_target_in_seat_order() {
        let context = ContextSpec::new()
            .melds_of(1, open_melds(3))
            .melds_of(2, open_melds(1))
            .melds_of(3, open_melds(3))
            .build();

        assert_eq!(targets(&context), vec![1, 3]);
    }

    #[test]
    fn the_targets_match_the_shared_classification() {
        // 防御側で分類し直さず、classification をそのまま source of truth にする。
        let context = two_target_context();
        let facts = player_threat_facts_from_context(&context);

        assert_eq!(
            high_open_hand_threat_players_from_facts(&facts),
            targets(&context)
        );
        assert_eq!(
            high_open_hand_threat_players(&classify_open_hand_threats(&facts)),
            targets(&context)
        );
    }

    // ---- 本人の河 (現物相当) ----

    #[test]
    fn a_tile_in_the_targets_river_is_river_safe() {
        let context = ContextSpec::new()
            .melds_of(3, open_melds(3))
            .discards_of(3, "4s")
            .build();
        let targets = targets(&context);

        assert!(is_discarded_by_player(tile_type("4s"), 3, &context));
        assert!(is_discarded_by_all_open_hand_threats(
            tile_type("4s"),
            &targets,
            &context
        ));
    }

    #[test]
    fn a_tile_only_in_another_players_river_is_not_river_safe() {
        // 他家が切っただけの牌は、対象 player 自身の河ではないので安全根拠にしない。
        let context = ContextSpec::new()
            .melds_of(3, open_melds(3))
            .discards_of(2, "4s")
            .build();
        let targets = targets(&context);

        assert_eq!(targets, vec![3]);
        assert!(!is_discarded_by_player(tile_type("4s"), 3, &context));
        assert!(!is_discarded_by_all_open_hand_threats(
            tile_type("4s"),
            &targets,
            &context
        ));
    }

    #[test]
    fn a_post_reach_passed_tile_is_not_river_safe_for_an_open_hand_target() {
        // post_reach_passed_tiles はリーチ固有の情報で、非リーチ副露相手には流用しない。
        let context = ContextSpec::new()
            .melds_of(3, open_melds(3))
            .post_reach_passed(3, "4s")
            .build();
        let targets = targets(&context);

        assert!(context.is_post_reach_passed(tile_type("4s"), 3));
        assert!(!is_discarded_by_player(tile_type("4s"), 3, &context));
        assert!(!is_discarded_by_all_open_hand_threats(
            tile_type("4s"),
            &targets,
            &context
        ));
    }

    #[test]
    fn a_tile_in_every_targets_river_is_river_safe_for_all() {
        let context = ContextSpec::new()
            .melds_of(2, open_melds(3))
            .melds_of(3, open_melds(3))
            .discards_of(2, "4s 5s")
            .discards_of(3, "4s 6s")
            .build();
        let targets = targets(&context);

        assert_eq!(targets, vec![2, 3]);
        assert!(is_discarded_by_all_open_hand_threats(
            tile_type("4s"),
            &targets,
            &context
        ));
        // 片方の河にしか無い牌は全 target には通らない。
        assert!(!is_discarded_by_all_open_hand_threats(
            tile_type("5s"),
            &targets,
            &context
        ));
        assert!(!is_discarded_by_all_open_hand_threats(
            tile_type("6s"),
            &targets,
            &context
        ));
    }

    #[test]
    fn river_safe_for_all_is_false_without_targets() {
        // target が0人の局面を「全員に通る」と扱わない。
        let context = ContextSpec::new().discards_of(3, "4s").build();

        assert!(targets(&context).is_empty());
        assert!(!is_discarded_by_all_open_hand_threats(
            tile_type("4s"),
            &targets(&context),
            &context
        ));
    }

    #[test]
    fn a_red_five_is_river_safe_as_the_same_tile_type() {
        // 河の黒5と赤5は同じ牌種として扱う。
        let mut spec = ContextSpec::new().melds_of(3, open_melds(3));
        spec.discards[3] = vec![tile_copy("5s", 1)];
        let context = spec.build();

        assert!(is_discarded_by_all_open_hand_threats(
            tile_type("5s"),
            &targets(&context),
            &context
        ));
    }

    // ---- 字牌 safety ----

    #[test]
    fn the_honor_safety_rank_matches_the_existing_defense_helper() {
        let context = ContextSpec::new()
            .melds_of(3, open_melds(3))
            .visible(vec![tile_copy("N", 0), tile_copy("N", 1)])
            .build();
        let candidate = candidate(&context, "N");

        assert_eq!(
            candidate.honor_safety_rank,
            Some(HonorSafetyRank::TwoVisible)
        );
        assert_eq!(
            candidate.honor_safety_rank,
            honor_safety_rank(tile_type("N"), &context)
        );
    }

    #[test]
    fn the_most_dangerous_opponent_honor_value_of_the_targets_wins() {
        // 場風 東。player 1 は親で自風も東なのでダブ東、player 3 の自風は北。
        let context = ContextSpec::new()
            .melds_of(DEALER_PLAYER, open_melds(3))
            .melds_of(3, open_melds(3))
            .build();
        let targets = targets(&context);

        assert_eq!(targets, vec![DEALER_PLAYER, 3]);
        assert_eq!(
            opponent_honor_value_for_open_hand_threats(tile_type("E"), &targets, &context),
            Some(OpponentHonorValue::DoubleWind)
        );
        assert_eq!(
            opponent_honor_value_for_open_hand_threats(tile_type("E"), &[3], &context),
            Some(OpponentHonorValue::SingleValueHonor)
        );
    }

    #[test]
    fn a_target_with_the_tile_in_its_river_is_excluded_from_the_honor_value() {
        // 自身の河にある牌ではロンされないので、その target は役牌価値の集約対象から外す。
        // 東は player 1 にとってダブ東、player 3 にとっては場風だけの役牌。
        let context = ContextSpec::new()
            .melds_of(DEALER_PLAYER, open_melds(3))
            .melds_of(3, open_melds(3))
            .discards_of(DEALER_PLAYER, "E")
            .build();

        assert_eq!(
            opponent_honor_value_for_open_hand_threats(
                tile_type("E"),
                &targets(&context),
                &context
            ),
            Some(OpponentHonorValue::SingleValueHonor)
        );
    }

    #[test]
    fn a_wind_that_is_neither_the_round_wind_nor_a_seat_wind_is_a_guest_wind() {
        // 場風 東、player 3 の自風は西。南は player 3 にとって客風。
        let context = ContextSpec::new().melds_of(3, open_melds(3)).build();

        assert_eq!(context.seat_wind_of(3), Some(tile_type("W")));
        assert_eq!(
            opponent_honor_value_for_open_hand_threats(
                tile_type("S"),
                &targets(&context),
                &context
            ),
            Some(OpponentHonorValue::GuestWind)
        );
    }

    #[test]
    fn an_unknown_wind_is_not_guessed_as_a_guest_wind() {
        // 場風か親が不明な風牌は推測せず unknown のままにする。
        let mut spec = ContextSpec::new().melds_of(3, open_melds(3));
        spec.round_wind = None;
        let no_round_wind = spec.build();
        assert_eq!(
            opponent_honor_value_for_open_hand_threats(
                tile_type("N"),
                &targets(&no_round_wind),
                &no_round_wind
            ),
            None
        );

        let mut spec = ContextSpec::new().melds_of(3, open_melds(3));
        spec.oya = None;
        let no_oya = spec.build();
        assert_eq!(
            opponent_honor_value_for_open_hand_threats(tile_type("N"), &targets(&no_oya), &no_oya),
            None
        );

        // 三元牌は風情報が無くても確定する。
        assert_eq!(
            opponent_honor_value_for_open_hand_threats(tile_type("P"), &targets(&no_oya), &no_oya),
            Some(OpponentHonorValue::SingleValueHonor)
        );
    }

    #[test]
    fn the_honor_value_is_unknown_without_targets() {
        let context = ContextSpec::new().build();

        assert_eq!(
            opponent_honor_value_for_open_hand_threats(tile_type("E"), &[], &context),
            None
        );
    }

    // ---- 数牌 safety ----

    #[test]
    fn the_suji_rank_of_a_single_target_matches_the_existing_defense_helper() {
        let context = ContextSpec::new()
            .melds_of(3, open_melds(3))
            .discards_of(3, "2m")
            .build();
        let targets = targets(&context);

        assert_eq!(
            suji_safety_rank_for_open_hand_threats(tile_type("5m"), &targets, &context),
            suji_safety_rank_for(tile_type("5m"), 3, &context)
        );
        assert_eq!(
            suji_safety_rank_for_open_hand_threats(tile_type("5m"), &targets, &context),
            Some(SujiSafetyRank::HalfSuji)
        );
    }

    #[test]
    fn the_most_dangerous_suji_rank_of_the_targets_wins() {
        // player 2 に対しては両側スジ、player 3 に対しては片スジ。集約は最も危険な HalfSuji。
        let context = ContextSpec::new()
            .melds_of(2, open_melds(3))
            .melds_of(3, open_melds(3))
            .discards_of(2, "2m 8m")
            .discards_of(3, "2m")
            .build();
        let targets = targets(&context);

        assert_eq!(
            suji_safety_rank_for(tile_type("5m"), 2, &context),
            Some(SujiSafetyRank::Suji)
        );
        assert_eq!(
            suji_safety_rank_for(tile_type("5m"), 3, &context),
            Some(SujiSafetyRank::HalfSuji)
        );
        assert_eq!(
            suji_safety_rank_for_open_hand_threats(tile_type("5m"), &targets, &context),
            Some(SujiSafetyRank::HalfSuji)
        );
    }

    #[test]
    fn a_target_with_the_tile_in_its_river_does_not_lower_the_suji_rank() {
        // player 2 は 5m を河に切っているのでフリテンでロンできない。その無スジを集約に
        // 持ち込まず、ロンされ得る player 3 の両側スジがそのまま全体の評価になる。
        let context = ContextSpec::new()
            .melds_of(2, open_melds(3))
            .melds_of(3, open_melds(3))
            .discards_of(2, "5m")
            .discards_of(3, "2m 8m")
            .build();
        let targets = targets(&context);

        assert_eq!(targets, vec![2, 3]);
        assert!(is_discarded_by_player(tile_type("5m"), 2, &context));
        assert_eq!(
            suji_safety_rank_for(tile_type("5m"), 2, &context),
            Some(SujiSafetyRank::NoSuji)
        );
        assert_eq!(
            suji_safety_rank_for(tile_type("5m"), 3, &context),
            Some(SujiSafetyRank::Suji)
        );

        assert_eq!(
            suji_safety_rank_for_open_hand_threats(tile_type("5m"), &targets, &context),
            Some(SujiSafetyRank::Suji)
        );
        assert_eq!(
            suited_safety_rank_for_open_hand_threats(tile_type("5m"), &targets, &context),
            Some(SuitedSafetyRank::Suji)
        );
    }

    #[test]
    fn a_remaining_target_without_a_suji_still_lowers_the_suji_rank() {
        // player 2 は 5m を河に切っているが、player 3 にはまだロンされ得るので無スジのまま。
        let context = ContextSpec::new()
            .melds_of(2, open_melds(3))
            .melds_of(3, open_melds(3))
            .discards_of(2, "5m")
            .build();
        let targets = targets(&context);

        assert_eq!(targets, vec![2, 3]);
        assert!(!is_discarded_by_player(tile_type("5m"), 3, &context));
        assert_eq!(
            suji_safety_rank_for_open_hand_threats(tile_type("5m"), &targets, &context),
            Some(SujiSafetyRank::NoSuji)
        );
        assert_eq!(
            suited_safety_rank_for_open_hand_threats(tile_type("5m"), &targets, &context),
            Some(SuitedSafetyRank::NoSafety)
        );
    }

    #[test]
    fn a_tile_in_every_targets_river_keeps_the_first_category() {
        // 全 target が河に切っている牌は集約対象が0人になるが、安全根拠は本人の河の分類が持つ。
        let context = ContextSpec::new()
            .melds_of(2, open_melds(3))
            .melds_of(3, open_melds(3))
            .discards_of(2, "5m")
            .discards_of(3, "5m")
            .build();
        let targets = targets(&context);

        assert!(is_discarded_by_all_open_hand_threats(
            tile_type("5m"),
            &targets,
            &context
        ));
        assert_eq!(
            open_hand_defense_category(tile_type("5m"), &targets, &context),
            Some(OpenHandDefenseCategory::SafeAgainstAllTargets)
        );
        // 集約対象が0人でも「スジがある」とは扱わない。
        assert_eq!(
            suji_safety_rank_for_open_hand_threats(tile_type("5m"), &targets, &context),
            Some(SujiSafetyRank::NoSuji)
        );
        assert_eq!(
            suited_safety_rank_for_open_hand_threats(tile_type("5m"), &targets, &context),
            Some(SuitedSafetyRank::NoSafety)
        );
    }

    #[test]
    fn a_temporary_passed_tile_is_safe_without_being_in_the_targets_river() {
        let mut temporary_passed: [Vec<TileType>; 4] = Default::default();
        temporary_passed[3].push(tile_type("9m"));
        let context = single_target_context().with_temporary_passed_tiles(Some(temporary_passed));
        let targets = targets(&context);
        let diagnostic = OpenHandDefenseCandidateDiagnostic::for_dahai_action(
            &context,
            &dahai("9m"),
            &targets,
            false,
        )
        .unwrap();

        assert!(!diagnostic.discarded_by_all_targets);
        assert!(diagnostic.ron_safe_for_all_targets);
        assert_eq!(
            diagnostic.category,
            Some(OpenHandDefenseCategory::SafeAgainstAllTargets)
        );
    }

    #[test]
    fn a_post_reach_passed_tile_does_not_exclude_a_target_from_the_aggregate() {
        // post_reach_passed_tiles は非リーチ副露相手の除外根拠にしない。player 2 は 5m を
        // 河に切っていないので、その無スジも役牌価値もそのまま集約へ入る。
        let context = ContextSpec::new()
            .melds_of(2, open_melds(3))
            .melds_of(3, open_melds(3))
            .discards_of(3, "2m 8m")
            .post_reach_passed(2, "5m E")
            .build();
        let targets = targets(&context);

        assert!(context.is_post_reach_passed(tile_type("5m"), 2));
        assert!(!is_discarded_by_player(tile_type("5m"), 2, &context));
        assert_eq!(
            suji_safety_rank_for_open_hand_threats(tile_type("5m"), &targets, &context),
            Some(SujiSafetyRank::NoSuji)
        );
        assert_eq!(
            suited_safety_rank_for_open_hand_threats(tile_type("5m"), &targets, &context),
            Some(SuitedSafetyRank::NoSafety)
        );
        // 役牌価値も同じ除外規則を共有する。
        assert_eq!(
            opponent_honor_value_for_open_hand_threats(tile_type("E"), &targets, &context),
            Some(OpponentHonorValue::SingleValueHonor)
        );
    }

    #[test]
    fn the_suji_rank_without_targets_is_no_suji() {
        let context = ContextSpec::new().discards_of(3, "2m 8m").build();

        assert_eq!(
            suji_safety_rank_for_open_hand_threats(tile_type("5m"), &[], &context),
            Some(SujiSafetyRank::NoSuji)
        );
        assert_eq!(
            suji_safety_rank_for_open_hand_threats(tile_type("N"), &[], &context),
            None
        );
    }

    #[test]
    fn the_wall_rank_matches_the_existing_defense_helper() {
        // 6m が4枚見えていれば 8m の順子待ち経路は残らない。壁は見え牌由来で target に依らない。
        let context = ContextSpec::new()
            .melds_of(3, open_melds(3))
            .visible((0..4).map(|copy| tile_copy("6m", copy)).collect())
            .build();
        let candidate = candidate(&context, "8m");

        assert_eq!(candidate.wall_rank, Some(WallRank::NoChance));
        assert_eq!(
            candidate.wall_rank,
            Some(wall_rank(tile_type("8m"), &context))
        );
        assert_eq!(
            candidate.suited_safety_rank,
            Some(SuitedSafetyRank::NoChance)
        );
    }

    #[test]
    fn the_suited_safety_rank_prefers_the_wall_over_the_suji() {
        let context = ContextSpec::new()
            .melds_of(3, open_melds(3))
            .discards_of(3, "2m 8m")
            .build();
        let targets = targets(&context);

        assert_eq!(
            suited_safety_rank_for_open_hand_threats(tile_type("5m"), &targets, &context),
            Some(SuitedSafetyRank::Suji)
        );
        assert_eq!(
            suited_safety_rank_for_open_hand_threats(tile_type("N"), &targets, &context),
            None
        );
    }

    #[test]
    fn the_suited_safety_evidence_keeps_both_the_wall_and_the_suji() {
        // player3 の河 2m 8m で 5m は完全スジ。壁を OneChance にしてもスジ根拠は失われない。
        let context = ContextSpec::new()
            .melds_of(3, open_melds(3))
            .discards_of(3, "2m 8m")
            .visible(
                (0..3)
                    .map(|copy| tile_copy("3m", copy))
                    .chain((0..4).map(|copy| tile_copy("6m", copy)))
                    .collect(),
            )
            .build();
        let targets = targets(&context);
        let five_man = tile_type("5m");
        let evidence = suited_safety_evidence_for_open_hand_threats(five_man, &targets, &context)
            .expect("数牌の evidence");

        assert_eq!(evidence.wall_rank, WallRank::OneChance);
        assert_eq!(evidence.suji_rank, SujiSafetyRank::Suji);
        // 責務は target の絞り込みだけで、evidence の意味は共有 helper と同じ。
        assert_eq!(
            Some(evidence),
            suited_safety_evidence_for_players(five_man, &[3], &context)
        );
        assert_eq!(
            suited_safety_rank_for_open_hand_threats(five_man, &targets, &context),
            Some(evidence.legacy_rank())
        );
        assert_eq!(
            suited_safety_evidence_for_open_hand_threats(tile_type("N"), &targets, &context),
            None
        );
    }

    #[test]
    fn the_suited_safety_evidence_excludes_ron_safe_targets() {
        // 5m が本人の河にある player2 は集約対象から外れ、player3 の片スジだけが残る。
        let context = ContextSpec::new()
            .melds_of(2, open_melds(3))
            .melds_of(3, open_melds(3))
            .discards_of(2, "5m 2m 8m")
            .discards_of(3, "2m")
            .build();
        let targets = targets(&context);
        let five_man = tile_type("5m");

        assert_eq!(
            suited_safety_evidence_for_open_hand_threats(five_man, &targets, &context),
            suited_safety_evidence_for_players(five_man, &[3], &context)
        );
        assert_eq!(
            suited_safety_evidence_for_open_hand_threats(five_man, &targets, &context)
                .map(|evidence| evidence.suji_rank),
            Some(SujiSafetyRank::HalfSuji)
        );
    }

    // ---- 候補診断 ----

    fn dahai(mjai: &str) -> LegalAction {
        LegalAction::Dahai { tile: tile(mjai) }
    }

    fn candidate(context: &GameContext, mjai: &str) -> OpenHandDefenseCandidateDiagnostic {
        OpenHandDefenseCandidateDiagnostic::for_dahai_action(
            context,
            &dahai(mjai),
            &high_open_hand_threat_players_from_context(context),
            false,
        )
        .expect("Dahai の候補診断")
    }

    #[test]
    fn a_candidate_carries_the_per_target_safety() {
        let context = ContextSpec::new()
            .melds_of(2, open_melds(3))
            .melds_of(3, open_melds(3))
            .discards_of(2, "5m 2m 8m")
            .discards_of(3, "2m")
            .build();
        let candidate = candidate(&context, "5m");

        assert_eq!(
            candidate.targets,
            vec![
                OpenHandDefenseTargetSafety {
                    player: 2,
                    discarded_by_target: true,
                    ron_safe: true,
                    suji_safety_rank: Some(SujiSafetyRank::Suji),
                },
                OpenHandDefenseTargetSafety {
                    player: 3,
                    discarded_by_target: false,
                    ron_safe: false,
                    suji_safety_rank: Some(SujiSafetyRank::HalfSuji),
                },
            ]
        );
        assert!(!candidate.discarded_by_all_targets);
        assert_eq!(candidate.suji_safety_rank, Some(SujiSafetyRank::HalfSuji));
        assert_eq!(
            candidate.suited_safety_evidence,
            Some(SuitedSafetyEvidence {
                wall_rank: WallRank::NoWall,
                suji_rank: SujiSafetyRank::HalfSuji,
            })
        );
        assert_eq!(
            candidate.suited_safety_rank,
            Some(SuitedSafetyRank::HalfSuji)
        );
        assert_eq!(
            candidate.category,
            Some(OpenHandDefenseCategory::SuitedSafety(
                SuitedSafetyRank::HalfSuji
            ))
        );
    }

    #[test]
    fn a_candidate_reports_the_same_values_as_the_pure_helpers() {
        // 表示のために safety を計算し直さず、production の pure helper の値をそのまま載せる。
        let context = ContextSpec::new()
            .melds_of(2, open_melds(3))
            .melds_of(3, open_melds(3))
            .discards_of(2, "2m")
            .visible(vec![tile_copy("N", 0)])
            .build();
        let targets = targets(&context);

        for mjai in ["5m", "N"] {
            let candidate = candidate(&context, mjai);
            let tile = tile_type(mjai);

            assert_eq!(
                candidate.discarded_by_all_targets,
                is_discarded_by_all_open_hand_threats(tile, &targets, &context),
                "{mjai}"
            );
            assert_eq!(
                candidate.honor_safety_rank,
                honor_safety_rank(tile, &context),
                "{mjai}"
            );
            assert_eq!(
                candidate.opponent_honor_value,
                opponent_honor_value_for_open_hand_threats(tile, &targets, &context),
                "{mjai}"
            );
            assert_eq!(
                candidate.suji_safety_rank,
                suji_safety_rank_for_open_hand_threats(tile, &targets, &context),
                "{mjai}"
            );
            assert_eq!(
                candidate.suited_safety_rank,
                suited_safety_rank_for_open_hand_threats(tile, &targets, &context),
                "{mjai}"
            );
            assert_eq!(
                candidate.category,
                open_hand_defense_category(tile, &targets, &context),
                "{mjai}"
            );
        }
    }

    #[test]
    fn the_category_prefers_the_targets_river_over_the_honor_and_suited_safety() {
        let context = ContextSpec::new()
            .melds_of(3, open_melds(3))
            .discards_of(3, "N 5m")
            .build();
        let targets = targets(&context);

        assert_eq!(
            open_hand_defense_category(tile_type("N"), &targets, &context),
            Some(OpenHandDefenseCategory::SafeAgainstAllTargets)
        );
        assert_eq!(
            open_hand_defense_category(tile_type("5m"), &targets, &context),
            Some(OpenHandDefenseCategory::SafeAgainstAllTargets)
        );
        assert_eq!(
            open_hand_defense_category(tile_type("W"), &targets, &context),
            Some(OpenHandDefenseCategory::HonorSafety(
                HonorSafetyRank::NoVisible
            ))
        );
        assert_eq!(
            open_hand_defense_category(tile_type("3m"), &targets, &context),
            Some(OpenHandDefenseCategory::SuitedSafety(
                SuitedSafetyRank::NoSafety
            ))
        );
    }

    // ---- 局面診断 ----

    #[test]
    fn the_diagnostic_keeps_the_legal_dahai_order_and_skips_other_actions() {
        let context = single_target_context();
        let legal_actions = vec![
            LegalAction::Reach,
            dahai("N"),
            dahai("5m"),
            LegalAction::Hora,
        ];
        let diagnostic = OpenHandDefenseDiagnostic::from_context(&context, &legal_actions, None);

        assert!(diagnostic.has_target());
        assert_eq!(diagnostic.targets, vec![3]);
        assert!(diagnostic.selected.is_none());
        assert!(
            diagnostic
                .candidates
                .iter()
                .all(|candidate| !candidate.selected)
        );
        assert_eq!(
            diagnostic
                .candidates
                .iter()
                .map(|candidate| candidate.tile)
                .collect::<Vec<TileType>>(),
            vec![tile_type("N"), tile_type("5m")]
        );
    }

    #[test]
    fn the_diagnostic_has_no_candidates_without_targets() {
        // Present しかいない局面を「target なし」と分かる形にする。
        let context = ContextSpec::new().melds_of(3, open_melds(1)).build();
        let diagnostic = OpenHandDefenseDiagnostic::from_context(&context, &[dahai("N")], None);

        assert!(!diagnostic.has_target());
        assert!(diagnostic.targets.is_empty());
        assert!(diagnostic.candidates.is_empty());
    }

    #[test]
    fn the_diagnostic_shares_the_given_classification() {
        let context = two_target_context();
        let legal_actions = vec![dahai("N")];

        assert_eq!(
            OpenHandDefenseDiagnostic::from_assessments(
                &context,
                &legal_actions,
                &assessments(&context),
                None
            ),
            OpenHandDefenseDiagnostic::from_context(&context, &legal_actions, None)
        );
    }

    #[test]
    fn one_value_honor_in_two_melds_is_not_a_target_from_the_classification() {
        // 通常役牌1翻だけの2副露は Present。Defense は classification を共有し、独自に
        // 旧 High 条件を再実装しない。
        let context = ContextSpec::new()
            .melds_of(3, vec![value_pon(), chi()])
            .build();

        assert_eq!(
            assessments(&context)[3].level(),
            Some(OpenHandThreatLevel::Present)
        );
        assert!(targets(&context).is_empty());
    }

    // ---- 防御 fallback の選択 ----

    fn fallback(
        context: &GameContext,
        legal_actions: &[LegalAction],
    ) -> Option<(LegalAction, OpenHandDefenseCategory)> {
        select_open_hand_defense_fallback_action_with_kind(
            context,
            legal_actions,
            &targets(context),
        )
        .map(|(action, category)| (action.clone(), category))
    }

    #[test]
    fn the_fallback_is_none_without_targets() {
        // Present しかいない局面では OpenHand 防御 fallback を選ばない。
        let context = ContextSpec::new()
            .melds_of(3, open_melds(1))
            .discards_of(3, "N")
            .build();

        assert!(targets(&context).is_empty());
        assert_eq!(fallback(&context, &[dahai("N"), dahai("5m")]), None);
    }

    #[test]
    fn the_fallback_prefers_a_tile_in_every_targets_river() {
        // 第一分類は全 target へのロン安全。字牌 safety より優先し、同順位では元順序を保つ。
        let context = ContextSpec::new()
            .melds_of(2, open_melds(3))
            .melds_of(3, open_melds(3))
            .discards_of(2, "4s N")
            .discards_of(3, "4s N")
            .build();

        assert_eq!(
            fallback(&context, &[dahai("5m"), dahai("4s"), dahai("N")]),
            Some((dahai("4s"), OpenHandDefenseCategory::SafeAgainstAllTargets))
        );
        assert_eq!(
            fallback(&context, &[dahai("5m"), dahai("N"), dahai("4s")]),
            Some((dahai("N"), OpenHandDefenseCategory::SafeAgainstAllTargets))
        );
    }

    #[test]
    fn a_tile_in_only_one_targets_river_is_not_the_first_category() {
        // 片方の target の河にしか無い牌は第一分類にしない。
        let context = ContextSpec::new()
            .melds_of(2, open_melds(3))
            .melds_of(3, open_melds(3))
            .discards_of(2, "4s")
            .build();

        assert_eq!(
            fallback(&context, &[dahai("4s"), dahai("N")]),
            Some((
                dahai("N"),
                OpenHandDefenseCategory::HonorSafety(HonorSafetyRank::NoVisible)
            ))
        );
    }

    #[test]
    fn the_fallback_uses_the_honor_safety_without_a_river_safe_tile() {
        // 字牌は見え枚数の安全度順。既存リーチ Defense と同じ4段階を使う。
        let context = ContextSpec::new()
            .melds_of(3, open_melds(3))
            .visible(vec![tile_copy("N", 0), tile_copy("N", 1)])
            .build();

        assert_eq!(
            fallback(&context, &[dahai("5m"), dahai("W"), dahai("N")]),
            Some((
                dahai("N"),
                OpenHandDefenseCategory::HonorSafety(HonorSafetyRank::TwoVisible)
            ))
        );
    }

    #[test]
    fn the_fallback_breaks_an_honor_tie_by_the_opponent_honor_value() {
        // 場風 東、player 3 の自風は西。見え枚数が同じなら客風の南を先に切る。
        let context = ContextSpec::new().melds_of(3, open_melds(3)).build();

        assert_eq!(context.seat_wind_of(3), Some(tile_type("W")));
        assert_eq!(
            opponent_honor_value_for_open_hand_threats(
                tile_type("S"),
                &targets(&context),
                &context
            ),
            Some(OpponentHonorValue::GuestWind)
        );
        assert_eq!(
            opponent_honor_value_for_open_hand_threats(
                tile_type("P"),
                &targets(&context),
                &context
            ),
            Some(OpponentHonorValue::SingleValueHonor)
        );

        assert_eq!(
            fallback(&context, &[dahai("P"), dahai("S")]),
            Some((
                dahai("S"),
                OpenHandDefenseCategory::HonorSafety(HonorSafetyRank::NoVisible)
            ))
        );
    }

    #[test]
    fn an_unknown_honor_value_is_not_ordered_as_a_guest_wind() {
        // 場風が不明な風牌は unknown のまま。客風と推測して役牌より先に切らない。
        let mut spec = ContextSpec::new().melds_of(3, open_melds(3));
        spec.round_wind = None;
        let context = spec.build();

        assert_eq!(
            opponent_honor_value_for_open_hand_threats(
                tile_type("S"),
                &targets(&context),
                &context
            ),
            None
        );
        assert_eq!(
            fallback(&context, &[dahai("P"), dahai("S")]),
            Some((
                dahai("P"),
                OpenHandDefenseCategory::HonorSafety(HonorSafetyRank::NoVisible)
            ))
        );
    }

    // 数牌の安全度を段階的に作る局面。
    //
    // - 1s: 2s が4枚見えているので NoChance
    // - 9s: 8s が3枚見えているので OneChance
    // - 5m: player 3 の河の 2m / 8m から両側スジ
    // - 6p: player 3 の河の 3p から片スジ
    // - 4m: 壁もスジも無い NoSafety
    fn suited_safety_context() -> GameContext {
        let mut visible: Vec<TileId> = (0..4).map(|copy| tile_copy("2s", copy)).collect();
        visible.extend((0..3).map(|copy| tile_copy("8s", copy)));

        ContextSpec::new()
            .melds_of(3, open_melds(3))
            .discards_of(3, "2m 8m 3p")
            .visible(visible)
            .build()
    }

    #[test]
    fn the_fallback_prefers_the_safest_suited_tile() {
        // 字牌も本人の河も無い場合は数牌 safety の順。既存リーチ Defense と同じ rank 順を使う。
        let context = suited_safety_context();
        let expectations = [
            ("1s", SuitedSafetyRank::NoChance),
            ("9s", SuitedSafetyRank::OneChance),
            ("5m", SuitedSafetyRank::Suji),
            ("6p", SuitedSafetyRank::HalfSuji),
        ];

        // 先頭の候補から順に外すと、次に安全な候補が選ばれる。無スジの 4m を先頭に置いても
        // 合法 action の順序より安全度が優先される。
        for index in 0..expectations.len() {
            let mut legal_actions = vec![dahai("4m")];
            legal_actions.extend(expectations[index..].iter().map(|(mjai, _)| dahai(mjai)));

            let (mjai, rank) = expectations[index];
            assert_eq!(
                fallback(&context, &legal_actions),
                Some((dahai(mjai), OpenHandDefenseCategory::SuitedSafety(rank))),
                "{mjai}"
            );
        }
    }

    #[test]
    fn a_suited_no_safety_candidate_is_not_selected() {
        // NoSafety しか無い場合は OpenHand 防御 fallback を選ばない。既存リーチ Defense と同じ。
        let context = ContextSpec::new().melds_of(3, open_melds(3)).build();

        for mjai in ["4m", "5p"] {
            assert_eq!(
                suited_safety_rank_for_open_hand_threats(
                    tile_type(mjai),
                    &targets(&context),
                    &context
                ),
                Some(SuitedSafetyRank::NoSafety),
                "{mjai}"
            );
        }
        assert_eq!(fallback(&context, &[dahai("4m"), dahai("5p")]), None);
    }

    #[test]
    fn a_post_reach_passed_tile_is_not_selected_as_river_safe() {
        // post_reach_passed はリーチ者専用の情報。第一分類の根拠にしない。
        let context = ContextSpec::new()
            .melds_of(3, open_melds(3))
            .post_reach_passed(3, "4s")
            .build();

        assert!(context.is_post_reach_passed(tile_type("4s"), 3));
        assert_eq!(
            fallback(&context, &[dahai("4s"), dahai("N")]),
            Some((
                dahai("N"),
                OpenHandDefenseCategory::HonorSafety(HonorSafetyRank::NoVisible)
            ))
        );
    }

    #[test]
    fn the_fallback_prefers_the_black_five_of_the_same_tile_type() {
        // 牌種を決めたあと、同じ牌種内では黒5を優先する既存 semantics を保つ。
        let river_safe = ContextSpec::new()
            .melds_of(3, open_melds(3))
            .discards_of(3, "5s")
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
            fallback(&river_safe, &[red_five.clone(), black_five.clone()]),
            Some((
                black_five.clone(),
                OpenHandDefenseCategory::SafeAgainstAllTargets
            ))
        );

        // 数牌 safety 経由でも同じ。5m は player 3 の河の 2m / 8m から両側スジ。
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
                OpenHandDefenseCategory::SuitedSafety(SuitedSafetyRank::Suji)
            ))
        );
    }

    #[test]
    fn the_fallback_skips_actions_other_than_dahai() {
        let context = ContextSpec::new().melds_of(3, open_melds(3)).build();
        let legal_actions = vec![LegalAction::Reach, dahai("N"), LegalAction::Hora];

        assert_eq!(
            fallback(&context, &legal_actions),
            Some((
                dahai("N"),
                OpenHandDefenseCategory::HonorSafety(HonorSafetyRank::NoVisible)
            ))
        );
    }

    #[test]
    fn the_selected_action_matches_its_category_helper() {
        // selector が返す大分類は、その牌の open_hand_defense_category と一致する。
        let context = suited_safety_context();
        let legal_actions = vec![dahai("4m"), dahai("6p"), dahai("5m"), dahai("1s")];
        let (action, category) = fallback(&context, &legal_actions).expect("防御 fallback");
        let LegalAction::Dahai { tile } = action else {
            panic!("Dahai を選ぶ");
        };

        assert_eq!(
            Some(category),
            open_hand_defense_category(tile.tile_type(), &targets(&context), &context)
        );
    }

    #[test]
    fn the_action_only_wrapper_matches_the_selector() {
        let context = suited_safety_context();
        let legal_actions = vec![dahai("4m"), dahai("1s")];
        let targets = targets(&context);

        assert_eq!(
            select_open_hand_defense_fallback_action(&context, &legal_actions, &targets),
            select_open_hand_defense_fallback_action_with_kind(&context, &legal_actions, &targets)
                .map(|(action, _)| action)
        );
    }

    #[test]
    fn the_diagnostic_reflects_the_selected_fallback() {
        // 診断は production selector の結果を写すだけで、選び直さない。
        let context = suited_safety_context();
        let legal_actions = vec![dahai("4m"), dahai("1s")];
        let selected = select_open_hand_defense_fallback_action_with_kind(
            &context,
            &legal_actions,
            &targets(&context),
        );
        let diagnostic =
            OpenHandDefenseDiagnostic::from_context(&context, &legal_actions, selected);

        assert_eq!(
            diagnostic.selected,
            Some(OpenHandDefenseSelectionDiagnostic {
                selected_action: dahai("1s"),
                selected_category: OpenHandDefenseCategory::SuitedSafety(
                    SuitedSafetyRank::NoChance
                ),
            })
        );
        assert_eq!(
            diagnostic.selected_category(),
            Some(OpenHandDefenseCategory::SuitedSafety(
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
}
