use crate::action::LegalAction;
use crate::context::GameContext;
use bot_logic::TileType;

use super::hard_safety::is_genbutsu_for_all_reached;
use super::honor::{
    HonorSafetyRank, OpponentHonorValue, honor_safety_rank, opponent_honor_value_for_reached,
};
use super::suited::{
    SuitedSafetyEvidence, SuitedSafetyRank, suited_safety_evidence_for_all_reached,
};
use super::suji::{SujiSafetyRank, is_suji_for_all_reached};
use super::wall::WallRank;
use super::{
    DahaiRonRiskEvidence, DefenseFallbackEvaluation, DefenseFallbackKind, RonRiskEvidence,
    single_reach_dahai_actions_by_ron_risk,
};

const LOG_TARGET: &str = "bot_core::defense";

/// 防御 fallback がどの理由で選ばれたかを表す診断データ。
///
/// tracing の出力文字列に依存せずテストできるよう、ログへ渡す値を pure に構築する。
/// 数牌なら壁 / スジ / 数牌 safety を、字牌なら字牌 safety を持ち、無関係なフィールドは `None`。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DefenseFallbackDiagnostic {
    pub selected_action: String,
    pub selected_kind: DefenseFallbackKind,
    pub opponent_reach_count: u8,
    pub selected_genbutsu_for_all: bool,
    pub selected_honor_safety_rank: Option<HonorSafetyRank>,
    /// 現物のリーチ者を除いた全リーチ者に対する [`opponent_honor_value_for_reached`] の結果。
    ///
    /// 同じ `selected_honor_safety_rank` の字牌どうしの tie-break に使った値。数牌では `None`。
    pub selected_opponent_honor_value: Option<OpponentHonorValue>,
    /// 現物ではない全リーチ者に対する [`suited_safety_evidence_for_all_reached`] の結果そのもの。
    ///
    /// 壁とスジを潰さずに持つので、`selected_suited_safety_rank` が壁由来の
    /// `OneChance` / `NoChance` になっている場合でも、同時にスジが成立していたかどうかを
    /// 確認できる。字牌では `None`。
    pub selected_suited_safety_evidence: Option<SuitedSafetyEvidence>,
    pub selected_wall_rank: Option<WallRank>,
    /// 現物ではない、まだロンされ得る全リーチ者に対して完全なスジなら `true`。
    /// そのうち一人でも片スジ / 無スジなら `false`。集約対象が空の場合も `false`。片スジと
    /// 無スジの区別は `selected_suji_safety_rank_for_all_reached` で分かる。
    pub selected_suji_for_all_reached: Option<bool>,
    /// 現物ではない全リーチ者に対する [`suji_safety_rank_for_all_reached`](super::suji_safety_rank_for_all_reached) の結果そのもの。
    ///
    /// 壁と統合する前の純粋なスジ評価なので、`selected_suited_safety_rank` が壁由来の
    /// `OneChance` / `NoChance` になっている場合でも `HalfSuji` と `NoSuji` を区別できる。
    pub selected_suji_safety_rank_for_all_reached: Option<SujiSafetyRank>,
    pub selected_suited_safety_rank: Option<SuitedSafetyRank>,
    /// 単独リーチ exact model が利用可能な場合の、選択牌に対する `R/T` evidence。
    pub selected_ron_risk_evidence: Option<RonRiskEvidence>,
}

impl DefenseFallbackDiagnostic {
    /// 選択された防御 fallback の action と種別から診断データを構築する pure helper。
    ///
    /// 数牌に対しては `suited_safety_evidence_for_all_reached` と `is_suji_for_all_reached` を、
    /// 字牌に対しては `honor_safety_rank` を計算する。壁 / スジ / 数牌 safety は evidence から
    /// 取り出し、診断のために別計算しない。Dahai 以外の action では牌由来の値は空。
    pub fn from_selection(
        context: &GameContext,
        action: &LegalAction,
        kind: DefenseFallbackKind,
    ) -> Self {
        let ron_risk_evidence =
            single_reach_ron_risk_evidence(context, std::slice::from_ref(action));
        Self::from_selection_with_ron_risk(
            context,
            action,
            kind,
            ron_risk_for_action(ron_risk_evidence.as_deref(), action),
        )
    }

    fn from_selection_with_ron_risk(
        context: &GameContext,
        action: &LegalAction,
        kind: DefenseFallbackKind,
        ron_risk_evidence: Option<RonRiskEvidence>,
    ) -> Self {
        let tile_type = match action {
            LegalAction::Dahai { tile } => Some(tile.tile_type()),
            _ => None,
        };
        let selected_action = match action {
            LegalAction::Dahai { tile } => tile.to_mjai_string(),
            other => format!("{other:?}"),
        };
        let evidence =
            tile_type.and_then(|tile| suited_safety_evidence_for_all_reached(tile, context));
        let suited_tile = tile_type.filter(|tile| !tile.is_honor());

        Self {
            selected_action,
            selected_kind: kind,
            opponent_reach_count: context.reached_opponents().len() as u8,
            selected_genbutsu_for_all: tile_type
                .is_some_and(|tile| is_genbutsu_for_all_reached(tile, context)),
            selected_honor_safety_rank: tile_type.and_then(|tile| honor_safety_rank(tile, context)),
            selected_opponent_honor_value: tile_type
                .and_then(|tile| opponent_honor_value_for_reached(tile, context)),
            selected_suited_safety_evidence: evidence,
            selected_wall_rank: evidence.map(|evidence| evidence.wall_rank),
            selected_suji_for_all_reached: suited_tile
                .map(|tile| is_suji_for_all_reached(tile, context)),
            selected_suji_safety_rank_for_all_reached: evidence.map(|evidence| evidence.suji_rank),
            selected_suited_safety_rank: evidence.map(SuitedSafetyEvidence::legacy_rank),
            selected_ron_risk_evidence: ron_risk_evidence,
        }
    }
}

/// 合法 Dahai 1件ごとの防御候補評価。
///
/// 防御 fallback の優先順位判断に使う値だけを pure に保持する解析用データで、これ自体が
/// action 選択を行うことはない。選択の source of truth は
/// [`select_defense_fallback_action_with_kind`](super::select_defense_fallback_action_with_kind) であり、`selected` はその結果を写したもの。
///
/// 数牌では `wall_rank` / `suji_for_all_reached` / `suited_safety_rank` が `Some`、字牌では
/// `honor_safety_rank` が `Some` になり、無関係なフィールドは `None`。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DefenseCandidateDiagnostic {
    /// 対象の合法 Dahai。物理牌(赤5 / 黒5)の区別を保持する。
    pub action: LegalAction,
    /// `action` の牌種。
    pub tile: TileType,
    /// この候補が防御 fallback として選ばれたか。
    pub selected: bool,
    pub genbutsu_for_all: bool,
    pub honor_safety_rank: Option<HonorSafetyRank>,
    /// 現物のリーチ者を除いた全リーチ者に対する [`opponent_honor_value_for_reached`] の結果。
    ///
    /// 同じ `honor_safety_rank` の字牌どうしの tie-break に使う値。数牌では `None`。
    pub opponent_honor_value: Option<OpponentHonorValue>,
    /// 現物ではない全リーチ者に対する [`suited_safety_evidence_for_all_reached`] の結果そのもの。
    ///
    /// 壁とスジを潰さずに持つので、`suited_safety_rank` が壁由来の `OneChance` / `NoChance` に
    /// なっている場合でも、同時にスジが成立していたかどうかを確認できる。字牌では `None`。
    pub suited_safety_evidence: Option<SuitedSafetyEvidence>,
    pub wall_rank: Option<WallRank>,
    /// 現物ではない、まだロンされ得る全リーチ者に対して完全なスジなら `true`。
    /// そのうち一人でも片スジ / 無スジなら `false`。集約対象が空の場合も `false`。片スジと
    /// 無スジの区別は `suji_safety_rank_for_all_reached` で分かる。
    pub suji_for_all_reached: Option<bool>,
    /// 現物ではない全リーチ者に対する [`suji_safety_rank_for_all_reached`](super::suji_safety_rank_for_all_reached) の結果そのもの。
    ///
    /// 壁と統合する前の純粋なスジ評価なので、`suited_safety_rank` が壁由来の
    /// `OneChance` / `NoChance` になっている場合でも `HalfSuji` と `NoSuji` を区別できる。
    pub suji_safety_rank_for_all_reached: Option<SujiSafetyRank>,
    pub suited_safety_rank: Option<SuitedSafetyRank>,
    /// 単独リーチ exact model が利用可能な場合の、この候補に対する `R/T` evidence。
    pub ron_risk_evidence: Option<RonRiskEvidence>,
}

impl DefenseCandidateDiagnostic {
    /// 合法 Dahai 1件から防御候補評価を構築する pure helper。Dahai 以外の action では `None`。
    pub fn for_dahai_action(
        context: &GameContext,
        action: &LegalAction,
        selected: bool,
    ) -> Option<Self> {
        let ron_risk_evidence =
            single_reach_ron_risk_evidence(context, std::slice::from_ref(action));
        Self::for_dahai_action_with_ron_risk(
            context,
            action,
            selected,
            ron_risk_for_action(ron_risk_evidence.as_deref(), action),
        )
    }

    fn for_dahai_action_with_ron_risk(
        context: &GameContext,
        action: &LegalAction,
        selected: bool,
        ron_risk_evidence: Option<RonRiskEvidence>,
    ) -> Option<Self> {
        let LegalAction::Dahai { tile } = action else {
            return None;
        };
        let tile_type = tile.tile_type();
        let evidence = suited_safety_evidence_for_all_reached(tile_type, context);
        let suited_tile = (!tile_type.is_honor()).then_some(tile_type);

        Some(Self {
            action: action.clone(),
            tile: tile_type,
            selected,
            genbutsu_for_all: is_genbutsu_for_all_reached(tile_type, context),
            honor_safety_rank: honor_safety_rank(tile_type, context),
            opponent_honor_value: opponent_honor_value_for_reached(tile_type, context),
            suited_safety_evidence: evidence,
            wall_rank: evidence.map(|evidence| evidence.wall_rank),
            suji_for_all_reached: suited_tile.map(|tile| is_suji_for_all_reached(tile, context)),
            suji_safety_rank_for_all_reached: evidence.map(|evidence| evidence.suji_rank),
            suited_safety_rank: evidence.map(SuitedSafetyEvidence::legacy_rank),
            ron_risk_evidence,
        })
    }

    /// 合法 action のうち Dahai だけを、元の順序を保って防御候補評価へ変換する。
    ///
    /// `selected_action` は防御 fallback として実際に選ばれた action。一致する候補の `selected`
    /// だけが `true` になる。
    pub fn for_legal_actions(
        context: &GameContext,
        legal_actions: &[LegalAction],
        selected_action: Option<&LegalAction>,
    ) -> Vec<Self> {
        let ron_risk_evidence = single_reach_ron_risk_evidence(context, legal_actions);
        Self::for_legal_actions_with_ron_risk(
            context,
            legal_actions,
            selected_action,
            ron_risk_evidence.as_deref(),
        )
    }

    fn for_legal_actions_with_ron_risk(
        context: &GameContext,
        legal_actions: &[LegalAction],
        selected_action: Option<&LegalAction>,
        ron_risk_evidence: Option<&[DahaiRonRiskEvidence<'_>]>,
    ) -> Vec<Self> {
        legal_actions
            .iter()
            .filter_map(|action| {
                Self::for_dahai_action_with_ron_risk(
                    context,
                    action,
                    selected_action == Some(action),
                    ron_risk_for_action(ron_risk_evidence, action),
                )
            })
            .collect()
    }
}

/// 防御 fallback を検討した局面の構造化診断。
///
/// `selected` は防御 fallback を採用した場合の既存診断で、検討したが候補が無かった場合は `None`。
/// `candidates` は採否にかかわらず全合法 Dahai の防御評価を保持する解析用データで、
/// 「なぜその牌を切ったか」を後から追跡するために使う。
///
/// 防御選択ロジックは再実装しない。採用結果は [`select_defense_fallback_action_with_kind`](super::select_defense_fallback_action_with_kind) の
/// 結果をそのまま写す。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DefenseDecisionDiagnostic {
    pub selected: Option<DefenseFallbackDiagnostic>,
    pub candidates: Vec<DefenseCandidateDiagnostic>,
}

impl DefenseDecisionDiagnostic {
    /// 実際の防御 fallback 選択結果と合法 action から診断データを構築する pure helper。
    ///
    /// `selected` には [`select_defense_fallback_action_with_kind`](super::select_defense_fallback_action_with_kind) の戻り値をそのまま渡す。
    pub fn from_selection(
        context: &GameContext,
        legal_actions: &[LegalAction],
        selected: Option<(&LegalAction, DefenseFallbackKind)>,
    ) -> Self {
        let ron_risk_evidence = single_reach_ron_risk_evidence(context, legal_actions);
        Self::from_parts(
            context,
            legal_actions,
            selected,
            ron_risk_evidence.as_deref(),
        )
    }

    pub(crate) fn from_evaluation(
        context: &GameContext,
        legal_actions: &[LegalAction],
        evaluation: &DefenseFallbackEvaluation<'_>,
    ) -> Self {
        Self::from_parts(
            context,
            legal_actions,
            evaluation.selected,
            evaluation.ron_risk_evidence.as_deref(),
        )
    }

    fn from_parts(
        context: &GameContext,
        legal_actions: &[LegalAction],
        selected: Option<(&LegalAction, DefenseFallbackKind)>,
        ron_risk_evidence: Option<&[DahaiRonRiskEvidence<'_>]>,
    ) -> Self {
        Self {
            selected: selected.map(|(action, kind)| {
                DefenseFallbackDiagnostic::from_selection_with_ron_risk(
                    context,
                    action,
                    kind,
                    ron_risk_for_action(ron_risk_evidence, action),
                )
            }),
            candidates: DefenseCandidateDiagnostic::for_legal_actions_with_ron_risk(
                context,
                legal_actions,
                selected.map(|(action, _)| action),
                ron_risk_evidence,
            ),
        }
    }

    /// 採用された防御 fallback の種別。検討したが候補が無かった場合は `None`。
    pub fn selected_kind(&self) -> Option<DefenseFallbackKind> {
        self.selected
            .as_ref()
            .map(|diagnostic| diagnostic.selected_kind)
    }
}

fn single_reach_ron_risk_evidence<'a>(
    context: &GameContext,
    legal_actions: &'a [LegalAction],
) -> Option<Vec<DahaiRonRiskEvidence<'a>>> {
    let reached = context.reached_opponents();
    let &[player] = reached.as_slice() else {
        return None;
    };
    single_reach_dahai_actions_by_ron_risk(player, context, legal_actions)
}

fn ron_risk_for_action(
    evidence: Option<&[DahaiRonRiskEvidence<'_>]>,
    action: &LegalAction,
) -> Option<RonRiskEvidence> {
    let LegalAction::Dahai { tile } = action else {
        return None;
    };
    evidence?
        .iter()
        .find(|candidate| {
            matches!(candidate.action, LegalAction::Dahai { tile: candidate_tile } if candidate_tile.tile_type() == tile.tile_type())
        })
        .map(|candidate| candidate.evidence)
}

/// 防御 fallback を実際に採用したとき DEBUG イベントを1件出す opt-in ログ。
///
/// `RUST_LOG=bot_core::defense=debug` で有効化する。debug が無効な通常時は診断値や文字列を
/// 一切構築しない。TRACE が有効なら、合法 Dahai ごとの防御評価も追加で記録する。
///
/// 出力値は pure な診断データ (`DefenseFallbackDiagnostic` / `DefenseCandidateDiagnostic`) から
/// 作る。ログを解析して診断データを作る向きにはしない。
pub fn log_defense_fallback_decision(
    context: &GameContext,
    action: &LegalAction,
    kind: DefenseFallbackKind,
    legal_actions: &[LegalAction],
) {
    if !tracing::enabled!(target: LOG_TARGET, tracing::Level::DEBUG) {
        return;
    }

    let ron_risk_evidence = single_reach_ron_risk_evidence(context, legal_actions);
    log_defense_fallback_decision_with_ron_risk(
        context,
        action,
        kind,
        legal_actions,
        ron_risk_evidence.as_deref(),
    );
}

pub(crate) fn log_defense_fallback_evaluation(
    context: &GameContext,
    evaluation: &DefenseFallbackEvaluation<'_>,
    legal_actions: &[LegalAction],
) {
    if !tracing::enabled!(target: LOG_TARGET, tracing::Level::DEBUG) {
        return;
    }
    let Some((action, kind)) = evaluation.selected else {
        return;
    };
    log_defense_fallback_decision_with_ron_risk(
        context,
        action,
        kind,
        legal_actions,
        evaluation.ron_risk_evidence.as_deref(),
    );
}

fn log_defense_fallback_decision_with_ron_risk(
    context: &GameContext,
    action: &LegalAction,
    kind: DefenseFallbackKind,
    legal_actions: &[LegalAction],
    ron_risk_evidence: Option<&[DahaiRonRiskEvidence<'_>]>,
) {
    let diagnostic = DefenseFallbackDiagnostic::from_selection_with_ron_risk(
        context,
        action,
        kind,
        ron_risk_for_action(ron_risk_evidence, action),
    );
    tracing::debug!(
        target: LOG_TARGET,
        selected_action = %diagnostic.selected_action,
        selected_kind = ?diagnostic.selected_kind,
        opponent_reach_count = diagnostic.opponent_reach_count,
        selected_genbutsu_for_all = diagnostic.selected_genbutsu_for_all,
        selected_honor_safety_rank = ?diagnostic.selected_honor_safety_rank,
        selected_opponent_honor_value = ?diagnostic.selected_opponent_honor_value,
        selected_wall_rank = ?diagnostic.selected_wall_rank,
        selected_suji_for_all_reached = ?diagnostic.selected_suji_for_all_reached,
        selected_suji_safety_rank = ?diagnostic.selected_suji_safety_rank_for_all_reached,
        selected_suited_safety_rank = ?diagnostic.selected_suited_safety_rank,
        selected_ron_capable_weight = diagnostic.selected_ron_risk_evidence.map(|evidence| evidence.ron_capable_weight),
        selected_tenpai_weight = diagnostic.selected_ron_risk_evidence.map(|evidence| evidence.tenpai_weight),
        "defense fallback decision",
    );

    if tracing::enabled!(target: LOG_TARGET, tracing::Level::TRACE) {
        for candidate in DefenseCandidateDiagnostic::for_legal_actions_with_ron_risk(
            context,
            legal_actions,
            Some(action),
            ron_risk_evidence,
        ) {
            log_defense_fallback_candidate(&candidate);
        }
    }
}

// 合法 Dahai ごとの防御候補評価を TRACE で1件記録する。値は pure な診断データから取り出す。
fn log_defense_fallback_candidate(candidate: &DefenseCandidateDiagnostic) {
    let tile = match &candidate.action {
        LegalAction::Dahai { tile } => tile.to_mjai_string(),
        other => format!("{other:?}"),
    };

    tracing::trace!(
        target: LOG_TARGET,
        tile = %tile,
        genbutsu_for_all = candidate.genbutsu_for_all,
        honor_safety_rank = ?candidate.honor_safety_rank,
        opponent_honor_value = ?candidate.opponent_honor_value,
        wall_rank = ?candidate.wall_rank,
        suji_for_all_reached = ?candidate.suji_for_all_reached,
        suji_safety_rank = ?candidate.suji_safety_rank_for_all_reached,
        suited_safety_rank = ?candidate.suited_safety_rank,
        ron_capable_weight = candidate.ron_risk_evidence.map(|evidence| evidence.ron_capable_weight),
        tenpai_weight = candidate.ron_risk_evidence.map(|evidence| evidence.tenpai_weight),
        "defense fallback candidate",
    );
}
