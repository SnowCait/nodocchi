use crate::action::{LegalAction, prefer_black_five_for_action};
use crate::agent::Agent;
use crate::context::GameContext;
use crate::defense::{
    DefenseDecisionDiagnostic, DefenseFallbackKind, log_defense_fallback_decision,
    select_defense_fallback_action_with_kind,
};
use crate::discard_selection::{
    DiscardActionSelection, select_best_one_step_discard_evaluation_with_fixed_meld_count,
    select_discard_action_with_diagnostic, select_discard_action_with_evaluation,
    selected_discard_tenpai_wait_availability,
};
use crate::push_pull::{
    PushPullDecision, PushPullInputs, PushPullMode, decide_push_pull, log_push_pull_decision,
    push_pull_inputs_from_context_with_evaluation,
};
use crate::threat::{PlayerThreatDiagnostic, diagnose_player_threats};
use bot_logic::{
    DiscardDecisionDiagnostic, DiscardEvaluation, DiscardFuritenDiagnostic, FixedMeldCount,
    LookaheadDiagnostic, PermanentFuriten, TenpaiWaitAvailability, TileCounts, TileId, TileType,
    calculate_shanten_with_fixed_melds,
};

const AGENT_DECISION_LOG_TARGET: &str = "bot_core::agent_decision";

// 補正後の待ち枚数がこの枚数以上ならリーチする。
const REACH_MIN_REMAINING: u8 = 4;

// リーチを検討する打牌後の向聴数。
const REACH_TENPAI_SHANTEN: i8 = 0;

// 限定 Pon を検討する現在の向聴数。今回は 1向聴 → テンパイ だけを対象にする。
const PON_CURRENT_SHANTEN: i8 = 1;

// Pon 対象牌として concealed hand に必要な枚数。対子からの Pon だけを扱い、暗刻は崩さない。
const PON_TARGET_HAND_COUNT: usize = 2;

// Pon の consumed 枚数。
const PON_CONSUMED_TILE_COUNT: usize = 2;

/// 最終 action がどの経路で選ばれたかを表す診断。プロトコル非依存。
///
/// `ShantenAgent::act()` が実際に通った経路そのものであり、診断用の別判断ロジックではない。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentActionSource {
    Hora,
    Ryukyoku,
    Pon,
    Reach,
    NormalDiscard,
    DefenseFallback(DefenseFallbackKind),
    LegalDahaiFallback,
    None,
}

impl AgentActionSource {
    // 防御 kind を分離して扱えるよう、source ラベルは kind を含めない固定名にする。
    fn label(&self) -> &'static str {
        match self {
            AgentActionSource::Hora => "Hora",
            AgentActionSource::Ryukyoku => "Ryukyoku",
            AgentActionSource::Pon => "Pon",
            AgentActionSource::Reach => "Reach",
            AgentActionSource::NormalDiscard => "NormalDiscard",
            AgentActionSource::DefenseFallback(_) => "DefenseFallback",
            AgentActionSource::LegalDahaiFallback => "LegalDahaiFallback",
            AgentActionSource::None => "None",
        }
    }

    /// 防御 fallback 経路で選ばれた場合のその種別。他の経路では `None`。
    pub fn defense_kind(&self) -> Option<DefenseFallbackKind> {
        match self {
            AgentActionSource::DefenseFallback(kind) => Some(*kind),
            _ => None,
        }
    }
}

/// 限定 Pon を採用した / しなかった理由。
///
/// `EligibleTenpai` 以外はすべて「今回は Pon しない」理由であり、最初に落ちた条件を1つだけ
/// 表す。判定順は [`PonCandidateDiagnostic`] のフィールドが埋まる順と一致する。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PonDecisionReason {
    /// 全条件を満たし、Pon 後に生きた待ちのテンパイになる。
    EligibleTenpai,
    /// 他家にリーチ者がいる。今回の Pon は押し引きへ通さない。
    OpponentReached,
    /// reaction context に `drawn_tile` があり局面として不整合。14枚扱いで判断しない。
    UnexpectedDrawnTile,
    /// 対象牌が自分にとって確実な役牌ではない。風牌の情報不足もここに含む。
    NotValueHonor,
    /// 対象牌の concealed hand 内枚数がちょうど2枚ではない。
    TargetCountNotTwo,
    /// consumed が2枚でない・手牌に無い・物理牌が重複しているなどで除去できない。
    InvalidConsumed,
    /// 自分の副露済み面子数が不明。0副露と推測しない。
    FixedMeldCountUnknown,
    /// Pon 後の副露済み面子数が上限を超える。
    FixedMeldCountOverflow,
    /// 現在の effective shanten が1向聴ではない。
    CurrentShantenNotOne,
    /// Pon 後の手牌から打牌候補を評価できない。
    NoPostPonDiscard,
    /// Pon 後の最良打牌でもテンパイにならない。
    PostPonNotTenpai,
    /// Pon 後はテンパイだが、待ち牌がすべて見えている。
    NoLiveAcceptance,
}

/// 合法 `LegalAction::Pon` 1件ごとの判断内訳。
///
/// 各フィールドは判定が実際にそこまで進んだ場合だけ `Some` になり、進まなかった判定は推測せず
/// `None` のままにする。`post_pon_discard` は本番の打牌評価 helper が返した評価そのもの。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PonCandidateDiagnostic {
    pub action: LegalAction,
    pub target: TileType,
    /// `TileType::is_value_honor(round_wind, seat_wind)` の結果。
    pub value_honor: bool,
    pub current_fixed_meld_count: Option<FixedMeldCount>,
    /// `calculate_shanten_with_fixed_melds()` で求めた現在の effective shanten。
    pub current_shanten: Option<i8>,
    pub post_pon_fixed_meld_count: Option<FixedMeldCount>,
    /// Pon 後の最良打牌評価。
    pub post_pon_discard: Option<DiscardEvaluation>,
    pub eligible: bool,
    pub selected: bool,
    pub reason: PonDecisionReason,
}

impl PonCandidateDiagnostic {
    pub fn post_pon_shanten(&self) -> Option<i8> {
        self.post_pon_discard
            .as_ref()
            .map(DiscardEvaluation::min_shanten_after_discard)
    }

    pub fn post_pon_acceptance_total_remaining(&self) -> Option<u8> {
        self.post_pon_discard
            .as_ref()
            .map(DiscardEvaluation::acceptance_total_remaining)
    }

    pub fn post_pon_acceptance_type_count(&self) -> Option<usize> {
        self.post_pon_discard
            .as_ref()
            .map(DiscardEvaluation::acceptance_type_count)
    }
}

/// 限定 Pon 判断の構造化診断。
///
/// `selected` は `ShantenAgent::act()` が実際に採用した Pon そのもので、診断用の別判断ロジック
/// は持たない。採用が無い場合の `reason` は最初の候補が落ちた理由。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PonDecisionDiagnostic {
    pub selected: Option<LegalAction>,
    pub reason: PonDecisionReason,
    pub candidates: Vec<PonCandidateDiagnostic>,
}

/// リーチを採用した / しなかった理由。
///
/// `Eligible` 以外はすべて「今回はリーチしない」理由であり、最初に落ちた条件を1つだけ表す。
/// 判定順は [`ReachDecisionDiagnostic`] のフィールドが埋まる順と一致する。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReachDecisionReason {
    /// 全条件を満たし、選んだ打牌後が生きた待ちを十分に持つテンパイになる。
    Eligible,
    /// 合法 action に [`LegalAction::Reach`] が無い。
    NoLegalReach,
    /// 通常打牌 selection が打牌を選べていない。手牌や合法 Dahai が無い局面。
    NoSelectedDiscard,
    /// 選んだ打牌の後がテンパイではない。
    NotTenpai,
    /// テンパイだが、ツモ和了できる待ちの残枚数が threshold 未満。
    InsufficientLiveWait,
}

/// リーチ判断の構造化診断。
///
/// 契約:
///
/// - 判断材料は通常打牌 selection が選んだ打牌 ([`DiscardActionSelection`]) の評価だけで、リーチ
///   専用に向聴・受け入れ・待ち・フリテンを計算し直さない。`shanten_after_discard` は
///   [`DiscardEvaluation::min_shanten_after_discard`]、`tenpai_wait` のツモ側は
///   [`DiscardEvaluation::acceptance_after_discard`] そのもの。
/// - `selected` は `ShantenAgent::act()` が実際に採用したリーチそのもので、診断用の別判断
///   ロジックは持たない。
/// - 判定が進まなかった項目は推測せず `None` のままにする。
///
/// `tenpai_wait` の恒常フリテン・ロン可否は事実として持つだけで、今回のリーチ判断には使わない。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReachDecisionDiagnostic {
    /// 通常打牌 selection が選んだ合法 Dahai。押し引き入力と同じ selection の action。
    pub selected_discard: Option<LegalAction>,
    /// `selected_discard` を切った後の向聴数。打牌を選べていない場合は `None`。
    pub shanten_after_discard: Option<i8>,
    /// `selected_discard` を切った後がテンパイの場合の待ちとロン可否。
    ///
    /// 構造上のアガリ牌種・生きた待ち・ツモ残枚数と種類数・恒常フリテン・自分の河と重複した
    /// 待ち牌を持つ。テンパイにならない場合とリーチを検討しなかった場合は `None`。
    pub tenpai_wait: Option<TenpaiWaitAvailability>,
    /// 採用したリーチ。採用しなかった場合は `None`。
    pub selected: Option<LegalAction>,
    pub reason: ReachDecisionReason,
}

impl ReachDecisionDiagnostic {
    /// リーチすべきと判断したか。`selected` が `Some` であることと同値。
    pub fn should_reach(&self) -> bool {
        self.selected.is_some()
    }

    /// 打牌後テンパイの恒常フリテン状態。テンパイにならない場合は `None`。
    pub fn permanent_furiten(&self) -> Option<PermanentFuriten> {
        self.tenpai_wait
            .as_ref()
            .map(TenpaiWaitAvailability::permanent_furiten)
    }

    /// 恒常フリテンの観点からロンできるか。テンパイにならない場合と判断できない場合は `None`。
    pub fn can_ron(&self) -> Option<bool> {
        self.tenpai_wait
            .as_ref()
            .and_then(TenpaiWaitAvailability::can_ron)
    }

    /// ツモ和了できる待ちの残枚数。テンパイにならない場合は `None`。
    ///
    /// 選ばれた [`DiscardEvaluation::acceptance_total_remaining`] と常に一致する。
    pub fn tsumo_remaining(&self) -> Option<u8> {
        self.tenpai_wait.as_ref().map(|wait| wait.tsumo_remaining)
    }

    /// ツモ和了できる待ちの種類数。テンパイにならない場合は `None`。
    ///
    /// 選ばれた [`DiscardEvaluation::acceptance_type_count`] と常に一致する。
    pub fn tsumo_type_count(&self) -> Option<usize> {
        self.tenpai_wait.as_ref().map(|wait| wait.tsumo_type_count)
    }

    /// 自分の河と重複した待ち牌。テンパイにならない場合は空。
    pub fn discarded_waits(&self) -> &[TileType] {
        self.tenpai_wait
            .as_ref()
            .map_or(&[], TenpaiWaitAvailability::discarded_waits)
    }
}

/// `ShantenAgent` が下した最終判断と、その選択経路・ログ用文脈をまとめた内部表現。
///
/// ログのためだけに判断ロジックを再実行しないよう、action 選択の過程で得た情報を保持する。
/// `push_pull` / `push_pull_inputs` / `normal_discard` は Hora / Ryukyoku / Pon の早期 return では
/// `None`。`pon` は合法 Pon が1件も無い局面では `None`。`reach` はリーチを検討する Push mode
/// 以外では `None`。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AgentDecision {
    action: LegalAction,
    source: AgentActionSource,
    push_pull_inputs: Option<PushPullInputs>,
    push_pull: Option<PushPullDecision>,
    normal_discard: Option<LegalAction>,
    reach: Option<ReachDecisionDiagnostic>,
    pon: Option<PonDecisionDiagnostic>,
}

/// `ShantenAgent` の判断過程を外部の解析ツールから辿るための構造化診断。
///
/// 契約:
///
/// - `selected_action` / `selected_source` は `ShantenAgent::act()` と**同じ selection logic** の
///   結果である。診断専用の別判断ロジックは持たない。
///   常に `selected_action == ShantenAgent::act(context, legal_actions)` が成り立つ。
/// - 追加診断情報(候補ごとの形の内訳、全防御候補評価など)は解析用途であり、action 選択には
///   影響しない。
/// - 実際に実行されなかった判断は `None` で、推測して埋めない。Hora / Ryukyoku / 限定 Pon で
///   早期終了した場合は `normal_discard` / `normal_discard_action` / `push_pull_inputs` /
///   `push_pull_decision` / `reach` / `defense` がすべて `None`。
///
/// tracing ログとは独立した pure なデータであり、ログをパースして構築することはない。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShantenDecisionDiagnostic {
    /// 最終的に選んだ action。`ShantenAgent::act()` の結果と一致する。
    pub selected_action: LegalAction,
    /// 最終 action をどの経路で選んだか。
    pub selected_source: AgentActionSource,
    /// 通常打牌評価が選んだ合法 Dahai。最終 action が別経路でも、比較用に保持する。
    pub normal_discard_action: Option<LegalAction>,
    /// 通常打牌評価を行った場合の全合法候補診断。合法 Dahai が無い場合は
    /// `selected == None` かつ `candidates` が空の診断になる。
    pub normal_discard: Option<DiscardDecisionDiagnostic>,
    /// 通常打牌評価を行った場合の全合法候補の恒常フリテン診断。`normal_discard` と同じ候補集合・
    /// 同じ順序で、候補ごとに「その打牌でテンパイになる場合の待ち・ツモ和了の残枚数と種類数・
    /// ロン可否・自分の河と重複した待ち牌」を持つ。
    ///
    /// 判定に使う自分の河は「context の自分の河 + その打牌」で、他家の河や見え牌は使わない。
    /// `player_id` が無く自分の河を特定できない場合は非フリテンと断定せず
    /// [`PermanentFuriten::Unknown`](bot_logic::PermanentFuriten::Unknown) になる。
    /// 打牌選択・押し引き・リーチ判断のどれにも使わない解析専用の情報。
    pub normal_discard_furiten: Option<Vec<DiscardFuritenDiagnostic>>,
    /// 通常打牌評価を行った場合の全合法候補の詳細な2手先診断。`normal_discard` と同じ候補集合・
    /// 同じ順序で、selected 候補だけでなく runner-up を含む全候補に対応する。
    ///
    /// 構築の有無は `selected_action` / `selected_source` / `normal_discard_action` /
    /// `push_pull_decision` / `defense` / `pon` のどれも変えない。構築した場合は打牌選択が使う
    /// 1向聴の weighted tenpai wait もこの枝評価から集計するが、集計対象と集計規則は選択専用
    /// 経路と同じなので結果は一致する。`act()` の経路では構築しない。
    pub normal_discard_lookahead: Option<LookaheadDiagnostic>,
    /// 押し引き判定に使った入力。`push_pull_inputs_from_context_with_evaluation()` の実結果。
    pub push_pull_inputs: Option<PushPullInputs>,
    /// 押し引き判定の結果。`decide_push_pull()` の実結果。
    pub push_pull_decision: Option<PushPullDecision>,
    /// リーチを検討した場合の判断内訳。リーチを検討する Push mode 以外では `None`。
    ///
    /// 採用しなかった場合も、通常打牌 selection が選んだ打牌・打牌後の向聴・待ち・恒常フリテンと
    /// 理由を保持する。`act()` と同じ helper の実結果で、診断用の別判断ロジックは持たない。
    pub reach: Option<ReachDecisionDiagnostic>,
    /// 防御 fallback を検討した場合の診断。採用されなかった場合も候補評価を保持する。
    pub defense: Option<DefenseDecisionDiagnostic>,
    /// 限定 Pon を検討した場合の診断。合法 Pon が1件も無ければ `None`。採用しなかった場合も
    /// 候補ごとの理由を保持する。
    pub pon: Option<PonDecisionDiagnostic>,
    pub own_fixed_meld_count: Option<FixedMeldCount>,
    /// 全4席分の脅威診断。`context` から読み取れる副露・リーチ・親・ドラの観測事実だけを持つ。
    ///
    /// `player_id` が不明でも席を除外せず常に4席分あり、自分か他家かは各診断の `is_self` /
    /// `is_opponent()` が unknown で表す。危険度の判断は含まず、現時点では押し引き・防御・
    /// 打牌選択のどれにも影響しない解析専用の情報。
    pub player_threats: [PlayerThreatDiagnostic; 4],
}

impl ShantenDecisionDiagnostic {
    /// 最終 action が防御 fallback 由来の場合のその種別。他の経路では `None`。
    pub fn defense_fallback_kind(&self) -> Option<DefenseFallbackKind> {
        self.selected_source.defense_kind()
    }
}

/// 診断で追加構築する解析情報の指定。
///
/// 追加情報の有無は選択結果を変えない。既定 ([`DiagnosticOptions::default`]) では、既存診断だけを
/// 構築して重い追加探索を行わない。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct DiagnosticOptions {
    /// 通常打牌候補の2手先診断 ([`ShantenDecisionDiagnostic::normal_discard_lookahead`]) を
    /// 構築するかどうか。
    ///
    /// 2手先は「打牌候補 × 受け入れ牌 × 次打牌候補」の探索になり既存診断よりさらに重いため、
    /// 既定では構築しない。有効にしても選択結果は変わらない。
    pub lookahead: bool,
}

impl DiagnosticOptions {
    /// 既存診断のみ。2手先診断は構築しない。
    pub const NONE: Self = Self { lookahead: false };
    /// 2手先診断まで構築する。
    pub const WITH_LOOKAHEAD: Self = Self { lookahead: true };
}

/// `ShantenAgent::act()` と同じ判断を行い、その過程を構造化診断として返す。
///
/// [`ShantenAgent::diagnose`] の別名。契約は [`ShantenDecisionDiagnostic`] を参照。
pub fn diagnose_shanten_decision(
    context: &GameContext,
    legal_actions: &[LegalAction],
) -> ShantenDecisionDiagnostic {
    ShantenAgent::diagnose(context, legal_actions)
}

/// 追加診断を指定して `ShantenAgent::act()` と同じ判断を行う。
///
/// [`ShantenAgent::diagnose_with_options`] の別名。
pub fn diagnose_shanten_decision_with_options(
    context: &GameContext,
    legal_actions: &[LegalAction],
    options: DiagnosticOptions,
) -> ShantenDecisionDiagnostic {
    ShantenAgent::diagnose_with_options(context, legal_actions, options)
}

// 解析専用の追加診断を集める内部収集器。
//
// `enabled == false` の通常 act() 経路では、候補ごとの形の内訳や全防御候補評価といった
// action 選択に不要な情報を一切構築しない。selection logic 自体は enabled にかかわらず共通。
#[derive(Debug, Default)]
struct DecisionDiagnostics {
    enabled: bool,
    options: DiagnosticOptions,
    normal_discard: Option<DiscardDecisionDiagnostic>,
    normal_discard_furiten: Option<Vec<DiscardFuritenDiagnostic>>,
    normal_discard_lookahead: Option<LookaheadDiagnostic>,
    defense: Option<DefenseDecisionDiagnostic>,
}

impl DecisionDiagnostics {
    fn disabled() -> Self {
        Self::default()
    }

    fn enabled_with(options: DiagnosticOptions) -> Self {
        Self {
            enabled: true,
            options,
            ..Self::default()
        }
    }

    #[cfg(test)]
    fn enabled() -> Self {
        Self::enabled_with(DiagnosticOptions::NONE)
    }
}

#[derive(Debug, Default)]
pub struct ShantenAgent;

impl ShantenAgent {
    /// `act()` と同じ判断を行い、その過程を構造化診断として返す。
    ///
    /// 判断経路は `act()` と共通の内部 helper を通るため、
    /// `diagnose(...).selected_action == ShantenAgent::act(...)` が常に成り立つ。診断用の別判断
    /// ロジックは持たない。契約の詳細は [`ShantenDecisionDiagnostic`] を参照。
    ///
    /// 解析専用の追加情報(候補ごとの形の内訳、全合法 Dahai の防御候補評価など)はこの経路
    /// でのみ構築する。通常の `act()` では計算しない。
    pub fn diagnose(
        context: &GameContext,
        legal_actions: &[LegalAction],
    ) -> ShantenDecisionDiagnostic {
        Self::diagnose_with_options(context, legal_actions, DiagnosticOptions::NONE)
    }

    /// 追加診断を指定して `act()` と同じ判断を行い、その過程を構造化診断として返す。
    ///
    /// `options` は解析専用の追加情報を構築するかどうかだけを決め、選択結果には影響しない。
    /// `diagnose_with_options(...).selected_action == ShantenAgent::act(...)` は `options` に
    /// かかわらず常に成り立つ。
    pub fn diagnose_with_options(
        context: &GameContext,
        legal_actions: &[LegalAction],
        options: DiagnosticOptions,
    ) -> ShantenDecisionDiagnostic {
        let mut diagnostics = DecisionDiagnostics::enabled_with(options);
        let decision =
            ShantenAgent.decide_with_diagnostics(context, legal_actions, &mut diagnostics);
        log_agent_decision(&decision);

        ShantenDecisionDiagnostic {
            selected_action: decision.action,
            selected_source: decision.source,
            normal_discard_action: decision.normal_discard,
            normal_discard: diagnostics.normal_discard,
            normal_discard_furiten: diagnostics.normal_discard_furiten,
            normal_discard_lookahead: diagnostics.normal_discard_lookahead,
            push_pull_inputs: decision.push_pull_inputs,
            push_pull_decision: decision.push_pull,
            reach: decision.reach,
            defense: diagnostics.defense,
            pon: decision.pon,
            own_fixed_meld_count: context.own_fixed_meld_count(),
            player_threats: diagnose_player_threats(context),
        }
    }

    // 最終 action と選択経路を1回で決める内部 helper。act() はこの結果を返し、
    // 共通箇所で agent decision ログを1件だけ出す。
    pub(crate) fn decide(&self, ctx: &GameContext, legal_actions: &[LegalAction]) -> AgentDecision {
        self.decide_with_diagnostics(ctx, legal_actions, &mut DecisionDiagnostics::disabled())
    }

    // 判断経路の本体。act() と構造化診断はこの1本を共有し、diagnostics が有効な場合だけ
    // 解析専用の追加情報を収集する。追加情報の収集は action 選択に影響しない。
    fn decide_with_diagnostics(
        &self,
        ctx: &GameContext,
        legal_actions: &[LegalAction],
        diagnostics: &mut DecisionDiagnostics,
    ) -> AgentDecision {
        if let Some(action) = legal_actions
            .iter()
            .find(|a| matches!(a, LegalAction::Hora))
        {
            return AgentDecision {
                action: action.clone(),
                source: AgentActionSource::Hora,
                push_pull_inputs: None,
                push_pull: None,
                normal_discard: None,
                reach: None,
                pon: None,
            };
        }

        if let Some(action) = legal_actions
            .iter()
            .find(|a| matches!(a, LegalAction::Ryukyoku))
        {
            return AgentDecision {
                action: action.clone(),
                source: AgentActionSource::Ryukyoku,
                push_pull_inputs: None,
                push_pull: None,
                normal_discard: None,
                reach: None,
                pon: None,
            };
        }

        // 限定 Pon。和了・流局より後、通常打牌 / 押し引き / 防御より前に検討する。
        let pon = evaluate_pon_decision(ctx, legal_actions);
        if let Some(action) = pon.as_ref().and_then(|pon| pon.selected.clone()) {
            return AgentDecision {
                action,
                source: AgentActionSource::Pon,
                push_pull_inputs: None,
                push_pull: None,
                normal_discard: None,
                reach: None,
                pon,
            };
        }

        // 通常打牌の evaluation と action を一度だけ取得し、その evaluation を
        // 押し引き入力にも共有して二重計算を避ける。
        let discard_selection = self.select_normal_discard(ctx, legal_actions, diagnostics);
        let inputs = push_pull_inputs_from_context_with_evaluation(
            ctx,
            discard_selection.evaluation.as_ref(),
        );
        let push_pull = decide_push_pull(&inputs);
        log_push_pull_decision(&push_pull, &inputs, discard_selection.action.as_ref());

        let normal_discard = discard_selection.action.clone();
        let mut reach = None;

        if let Some((action, source)) = self.select_action_for_push_pull_mode(
            push_pull.mode,
            ctx,
            legal_actions,
            &discard_selection,
            &mut reach,
            diagnostics,
        ) {
            return AgentDecision {
                action,
                source,
                push_pull_inputs: Some(inputs),
                push_pull: Some(push_pull),
                normal_discard,
                reach,
                pon,
            };
        }

        if let Some(first_dahai) = legal_actions
            .iter()
            .find(|a| matches!(a, LegalAction::Dahai { .. }))
        {
            // 最初の合法 Dahai の牌種を選んだうえで、その牌種内では黒牌を優先する。
            // 別牌種との順番は変えないため、先頭牌種はそのまま維持される。
            let action = prefer_black_five_for_action(legal_actions, first_dahai);
            return AgentDecision {
                action: action.clone(),
                source: AgentActionSource::LegalDahaiFallback,
                push_pull_inputs: Some(inputs),
                push_pull: Some(push_pull),
                normal_discard,
                reach,
                pon,
            };
        }

        let action = legal_actions
            .iter()
            .find(|a| matches!(a, LegalAction::None))
            .cloned()
            .unwrap_or(LegalAction::None);
        AgentDecision {
            action,
            source: AgentActionSource::None,
            push_pull_inputs: Some(inputs),
            push_pull: Some(push_pull),
            normal_discard,
            reach,
            pon,
        }
    }

    // 通常打牌選択。選択結果は診断の有無で変わらず、診断が有効な場合だけ全合法候補の
    // 構造化診断と2手先診断を追加で受け取る。2手先探索は診断が無効な act() 経路には入らない。
    fn select_normal_discard(
        &self,
        ctx: &GameContext,
        legal_actions: &[LegalAction],
        diagnostics: &mut DecisionDiagnostics,
    ) -> DiscardActionSelection {
        if !diagnostics.enabled {
            return select_discard_action_with_evaluation(ctx, legal_actions);
        }

        let selection = select_discard_action_with_diagnostic(
            ctx,
            legal_actions,
            diagnostics.options.lookahead,
        );
        diagnostics.normal_discard = Some(selection.diagnostic);
        diagnostics.normal_discard_furiten = Some(selection.furiten);
        diagnostics.normal_discard_lookahead = selection.lookahead;
        selection.selection
    }

    // 押し引きモードに応じた action 選択。候補は必要になった時点でのみ計算する。
    // 選ばれた action とともに、その選択経路を表す source を返す。
    //
    // - Push:    Reach → 通常打牌 → 防御 fallback
    // - Neutral: 通常打牌 → 防御 fallback(Reach は検討しない)
    // - Fold:    防御 fallback → 通常打牌(Reach は検討しない)
    //
    // リーチ判断と通常打牌・押し引きは同じ `discard_selection` を参照する。リーチのために打牌を
    // 選び直したり、待ちを別経路で計算し直したりしない。検討した場合の判断内訳は `reach` へ
    // 書き込み、検討しなかった Neutral / Fold では `None` のままにする。
    fn select_action_for_push_pull_mode(
        &self,
        mode: PushPullMode,
        ctx: &GameContext,
        legal_actions: &[LegalAction],
        discard_selection: &DiscardActionSelection,
        reach: &mut Option<ReachDecisionDiagnostic>,
        diagnostics: &mut DecisionDiagnostics,
    ) -> Option<(LegalAction, AgentActionSource)> {
        let normal_discard = discard_selection.action.as_ref();
        match mode {
            PushPullMode::Push => {
                let decision = reach.insert(decide_reach(ctx, legal_actions, discard_selection));
                if let Some(action) = decision.selected.clone() {
                    return Some((action, AgentActionSource::Reach));
                }
                if let Some(action) = normal_discard {
                    return Some((action.clone(), AgentActionSource::NormalDiscard));
                }
                self.select_defense_fallback(ctx, legal_actions, diagnostics)
            }
            PushPullMode::Neutral => {
                if let Some(action) = normal_discard {
                    return Some((action.clone(), AgentActionSource::NormalDiscard));
                }
                self.select_defense_fallback(ctx, legal_actions, diagnostics)
            }
            PushPullMode::Fold => {
                if let Some(result) = self.select_defense_fallback(ctx, legal_actions, diagnostics)
                {
                    return Some(result);
                }
                normal_discard
                    .cloned()
                    .map(|action| (action, AgentActionSource::NormalDiscard))
            }
        }
    }

    // 防御 fallback を採用する場合に、その理由を診断ログへ出しつつ action と種別を返す。
    //
    // 診断が有効な場合は、採用の有無にかかわらず検討した候補評価を収集する。候補評価の収集は
    // 選択結果に影響せず、選択は select_defense_fallback_action_with_kind() が source of truth。
    fn select_defense_fallback(
        &self,
        ctx: &GameContext,
        legal_actions: &[LegalAction],
        diagnostics: &mut DecisionDiagnostics,
    ) -> Option<(LegalAction, AgentActionSource)> {
        let selected = select_defense_fallback_action_with_kind(ctx, legal_actions);

        if let Some((action, kind)) = selected {
            log_defense_fallback_decision(ctx, action, kind, legal_actions);
        }

        if diagnostics.enabled {
            diagnostics.defense = Some(DefenseDecisionDiagnostic::from_selection(
                ctx,
                legal_actions,
                selected,
            ));
        }

        let (action, kind) = selected?;
        Some((action.clone(), AgentActionSource::DefenseFallback(kind)))
    }
}

// 限定 Pon の判断本体。act() と構造化診断はこの1本を共有し、診断は結果を載せるだけにする。
//
// 合法 Pon が1件も無ければ検討自体を行わず None。1件以上ある場合は候補ごとに条件を評価し、
// 最初に全条件を満たした候補を採用する。
fn evaluate_pon_decision(
    ctx: &GameContext,
    legal_actions: &[LegalAction],
) -> Option<PonDecisionDiagnostic> {
    let mut candidates: Vec<PonCandidateDiagnostic> = legal_actions
        .iter()
        .filter_map(|action| match action {
            LegalAction::Pon { tile, consumed } => {
                Some(evaluate_pon_candidate(ctx, action, *tile, consumed))
            }
            _ => None,
        })
        .collect();

    if candidates.is_empty() {
        return None;
    }

    let selected_index = candidates.iter().position(|candidate| candidate.eligible);
    if let Some(index) = selected_index {
        candidates[index].selected = true;
    }

    let reason = candidates[selected_index.unwrap_or(0)].reason;
    let selected = selected_index.map(|index| candidates[index].action.clone());

    Some(PonDecisionDiagnostic {
        selected,
        reason,
        candidates,
    })
}

fn evaluate_pon_candidate(
    ctx: &GameContext,
    action: &LegalAction,
    tile: TileId,
    consumed: &[TileId],
) -> PonCandidateDiagnostic {
    let target = tile.tile_type();
    let mut candidate = PonCandidateDiagnostic {
        action: action.clone(),
        target,
        value_honor: target.is_value_honor(ctx.round_wind(), ctx.seat_wind()),
        current_fixed_meld_count: None,
        current_shanten: None,
        post_pon_fixed_meld_count: None,
        post_pon_discard: None,
        eligible: false,
        selected: false,
        reason: PonDecisionReason::EligibleTenpai,
    };

    let reason = evaluate_pon_conditions(ctx, target, consumed, &mut candidate);
    candidate.eligible = reason == PonDecisionReason::EligibleTenpai;
    candidate.reason = reason;
    candidate
}

// Pon 成立条件を順に評価し、最初に落ちた条件を理由として返す。評価が進んだ範囲の値だけを
// candidate へ書き込み、評価しなかった項目は None のままにする。
fn evaluate_pon_conditions(
    ctx: &GameContext,
    target: TileType,
    consumed: &[TileId],
    candidate: &mut PonCandidateDiagnostic,
) -> PonDecisionReason {
    if ctx.any_opponent_reached() {
        return PonDecisionReason::OpponentReached;
    }

    // Pon は他家捨て牌への reaction なので、既存 client の reaction context に drawn_tile は無い。
    // drawn_tile がある不整合な context では、それを混ぜても無視しても正しい局面を復元できない
    // ため Pon を検討しない。
    if ctx.drawn_tile().is_some() {
        return PonDecisionReason::UnexpectedDrawnTile;
    }

    if !candidate.value_honor {
        return PonDecisionReason::NotValueHonor;
    }

    let hand_tiles = ctx.hand_tiles();
    let target_count = hand_tiles
        .iter()
        .filter(|tile| tile.tile_type() == target)
        .count();
    if target_count != PON_TARGET_HAND_COUNT {
        return PonDecisionReason::TargetCountNotTwo;
    }

    let Some(post_pon_tiles) = remove_pon_consumed_tiles(hand_tiles, target, consumed) else {
        return PonDecisionReason::InvalidConsumed;
    };

    let Some(current_fixed_meld_count) = ctx.own_fixed_meld_count() else {
        return PonDecisionReason::FixedMeldCountUnknown;
    };
    candidate.current_fixed_meld_count = Some(current_fixed_meld_count);

    let Some(post_pon_fixed_meld_count) = FixedMeldCount::new(current_fixed_meld_count.get() + 1)
    else {
        return PonDecisionReason::FixedMeldCountOverflow;
    };
    candidate.post_pon_fixed_meld_count = Some(post_pon_fixed_meld_count);

    let counts = TileCounts::from_tiles(hand_tiles.iter().copied());
    let current_shanten =
        calculate_shanten_with_fixed_melds(&counts, current_fixed_meld_count).min();
    candidate.current_shanten = Some(current_shanten);
    if current_shanten != PON_CURRENT_SHANTEN {
        return PonDecisionReason::CurrentShantenNotOne;
    }

    let Some(evaluation) = select_best_one_step_discard_evaluation_with_fixed_meld_count(
        ctx,
        &post_pon_tiles,
        post_pon_fixed_meld_count,
    ) else {
        return PonDecisionReason::NoPostPonDiscard;
    };
    let min_shanten = evaluation.min_shanten_after_discard();
    let acceptance_total_remaining = evaluation.acceptance_total_remaining();
    candidate.post_pon_discard = Some(evaluation);

    if min_shanten != 0 {
        return PonDecisionReason::PostPonNotTenpai;
    }
    if acceptance_total_remaining == 0 {
        return PonDecisionReason::NoLiveAcceptance;
    }

    PonDecisionReason::EligibleTenpai
}

// consumed の物理牌を concealed hand から1枚ずつ除去した仮想手牌を返す。
//
// 牌種単位で減らすのではなく物理牌 ID で除去するため、赤5などへ拡張しても semantics を保つ。
// 枚数が2枚でない・対象牌種でない・手牌に無い・同じ物理牌が重複している場合は None。
fn remove_pon_consumed_tiles(
    hand_tiles: &[TileId],
    target: TileType,
    consumed: &[TileId],
) -> Option<Vec<TileId>> {
    if consumed.len() != PON_CONSUMED_TILE_COUNT {
        return None;
    }

    let mut remaining = hand_tiles.to_vec();
    for tile in consumed {
        if tile.tile_type() != target {
            return None;
        }
        let position = remaining.iter().position(|held| held == tile)?;
        remaining.remove(position);
    }
    Some(remaining)
}

// action を agent decision ログ用のコンパクトな文字列へ変換する。
fn agent_action_label(action: &LegalAction) -> String {
    match action {
        LegalAction::Dahai { tile } => tile.to_mjai_string(),
        LegalAction::Pon { tile, .. } => format!("Pon {}", tile.to_mjai_string()),
        LegalAction::Reach => "Reach".to_string(),
        LegalAction::Hora => "Hora".to_string(),
        LegalAction::Ryukyoku => "Ryukyoku".to_string(),
        LegalAction::None => "None".to_string(),
        other => format!("{other:?}"),
    }
}

/// 意思決定1回につき最終 action と選択経路の DEBUG イベントを1件出す opt-in ログ。
///
/// `RUST_LOG=bot_core::agent_decision=debug` で有効化する。debug が無効な通常時は
/// ログ用の文字列変換などを一切行わない。
fn log_agent_decision(decision: &AgentDecision) {
    if !tracing::enabled!(target: AGENT_DECISION_LOG_TARGET, tracing::Level::DEBUG) {
        return;
    }

    let selected_action = agent_action_label(&decision.action);
    let normal_discard = decision
        .normal_discard
        .as_ref()
        .map(agent_action_label)
        .unwrap_or_else(|| "None".to_string());
    let push_pull_mode = match &decision.push_pull {
        Some(decision) => format!("{:?}", decision.mode),
        None => "None".to_string(),
    };
    let push_pull_reason = match &decision.push_pull {
        Some(decision) => format!("{:?}", decision.reason),
        None => "None".to_string(),
    };
    let defense_kind = match decision.source.defense_kind() {
        Some(kind) => format!("{kind:?}"),
        None => "None".to_string(),
    };
    let pon_reason = match &decision.pon {
        Some(pon) => format!("{:?}", pon.reason),
        None => "None".to_string(),
    };
    let reach_reason = match &decision.reach {
        Some(reach) => format!("{:?}", reach.reason),
        None => "None".to_string(),
    };

    tracing::debug!(
        target: AGENT_DECISION_LOG_TARGET,
        selected_action = %selected_action,
        selected_source = decision.source.label(),
        push_pull_mode = %push_pull_mode,
        push_pull_reason = %push_pull_reason,
        normal_discard = %normal_discard,
        reach_reason = %reach_reason,
        defense_kind = %defense_kind,
        pon_reason = %pon_reason,
        "agent decision",
    );
}

// リーチ判断の本体。act() と構造化診断はこの1本を共有し、診断は結果を載せるだけにする。
//
// 判断材料は通常打牌 selection が選んだ打牌の評価だけで、リーチ専用に手牌から向聴・受け入れを
// 計算し直さない。待ちと恒常フリテンも選択済みの1候補分だけを既存 pure helper から求め、全合法
// 候補分の診断や2手先探索は構築しない。
//
// 補正後の待ち枚数が明らかに少ない即リーチだけを抑制する最小判断であることは変えていない。
// TODO: 役判定・打点・押し引きを考慮したリーチ判断に置き換える。
fn decide_reach(
    ctx: &GameContext,
    legal_actions: &[LegalAction],
    selection: &DiscardActionSelection,
) -> ReachDecisionDiagnostic {
    let mut diagnostic = ReachDecisionDiagnostic {
        selected_discard: selection.action.clone(),
        shanten_after_discard: selection
            .evaluation
            .as_ref()
            .map(DiscardEvaluation::min_shanten_after_discard),
        tenpai_wait: None,
        selected: None,
        reason: ReachDecisionReason::NoLegalReach,
    };

    // 合法手にリーチが無ければ待ちもフリテンも求めない。
    let Some(reach) = legal_actions
        .iter()
        .find(|action| matches!(action, LegalAction::Reach))
        .cloned()
    else {
        return diagnostic;
    };

    // DiscardActionSelection の不変条件より、evaluation が Some なら action も Some。
    let Some(evaluation) = selection.evaluation.as_ref() else {
        diagnostic.reason = ReachDecisionReason::NoSelectedDiscard;
        return diagnostic;
    };

    if evaluation.min_shanten_after_discard() != REACH_TENPAI_SHANTEN {
        diagnostic.reason = ReachDecisionReason::NotTenpai;
        return diagnostic;
    }

    let Some(tenpai_wait) = selected_discard_tenpai_wait_availability(ctx, evaluation) else {
        diagnostic.reason = ReachDecisionReason::NotTenpai;
        return diagnostic;
    };

    // 待ち枚数は既存受け入れそのもので、visible tiles の反映も打牌評価の時点で済んでいる。
    // 恒常フリテンとロン可否は事実として tenpai_wait に持つだけで、判断には使わない。
    if tenpai_wait.tsumo_remaining >= REACH_MIN_REMAINING {
        diagnostic.reason = ReachDecisionReason::Eligible;
        diagnostic.selected = Some(reach);
    } else {
        diagnostic.reason = ReachDecisionReason::InsufficientLiveWait;
    }
    diagnostic.tenpai_wait = Some(tenpai_wait);
    diagnostic
}

impl Agent for ShantenAgent {
    fn act(&mut self, ctx: &GameContext, legal_actions: &[LegalAction]) -> LegalAction {
        let decision = self.decide(ctx, legal_actions);
        log_agent_decision(&decision);
        decision.action
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::defense::{HonorSafetyRank, select_defense_fallback_action};
    use crate::discard_selection::{select_best_normal_discard_evaluation, select_discard_action};
    use crate::push_pull::{PushPullReason, push_pull_inputs_from_context};
    use bot_logic::{
        DiscardComparisonReason, PermanentFuriten, TileId, TileType, compare_discard_evaluations,
    };

    pub(crate) fn tile(value: u8) -> TileId {
        TileId::new(value).unwrap()
    }

    pub(crate) fn dahai(value: u8) -> LegalAction {
        LegalAction::Dahai { tile: tile(value) }
    }

    #[test]
    fn picks_hora_first() {
        let mut agent = ShantenAgent;
        let ctx = GameContext::with_drawn_tile(tile(0));
        let actions = vec![dahai(0), LegalAction::Hora];
        assert_eq!(agent.act(&ctx, &actions), LegalAction::Hora);
    }

    #[test]
    fn prefers_ryukyoku_over_dahai() {
        let mut agent = ShantenAgent;
        let ctx = GameContext::with_drawn_tile(tile(0));
        let actions = vec![dahai(0), LegalAction::Ryukyoku];
        assert_eq!(agent.act(&ctx, &actions), LegalAction::Ryukyoku);
    }

    #[test]
    fn picks_dahai_by_discard_evaluation() {
        let mut agent = ShantenAgent;
        let hand_values = [0, 4, 8, 12, 17, 20, 24, 28, 32, 36, 40, 44, 89];
        let ctx = GameContext::from_parts(
            Some(tile(116)),
            hand_values.iter().map(|&value| tile(value)).collect(),
        );
        let actions: Vec<LegalAction> = hand_values
            .iter()
            .map(|&value| dahai(value))
            .chain([dahai(116)])
            .collect();

        let expected = select_discard_action(&ctx, &actions).unwrap();

        assert_eq!(agent.act(&ctx, &actions), expected);
    }

    #[test]
    fn prefers_hora_over_reach() {
        let mut agent = ShantenAgent;
        let ctx = GameContext::with_drawn_tile(tile(0));
        let actions = vec![LegalAction::Reach, LegalAction::Hora];
        assert_eq!(agent.act(&ctx, &actions), LegalAction::Hora);
    }

    #[test]
    fn prefers_ryukyoku_over_reach() {
        let mut agent = ShantenAgent;
        let ctx = GameContext::with_drawn_tile(tile(0));
        let actions = vec![LegalAction::Reach, LegalAction::Ryukyoku];
        assert_eq!(agent.act(&ctx, &actions), LegalAction::Ryukyoku);
    }

    #[test]
    fn prefers_reach_over_evaluated_dahai() {
        let mut agent = ShantenAgent;
        let ctx = tenpai_context(&[]);
        let actions = tenpai_actions();
        assert!(select_discard_action(&ctx, &actions).is_some());
        assert_eq!(agent.act(&ctx, &actions), LegalAction::Reach);
    }

    #[test]
    fn reach_is_policy_choice_not_fallback() {
        // 通常打牌を選べる局面でも、選んだ打牌後の待ちが十分ならリーチを選ぶ。
        let mut agent = ShantenAgent;
        let ctx = tenpai_context(&[]);
        let actions = tenpai_actions();

        let normal = select_discard_action(&ctx, &actions).expect("通常打牌を選べる");
        assert_eq!(agent.act(&ctx, &actions), LegalAction::Reach);
        assert_ne!(agent.act(&ctx, &actions), normal);
    }

    #[test]
    fn picks_dahai_when_reach_absent() {
        let mut agent = ShantenAgent;
        let ctx = GameContext::with_drawn_tile(tile(0));
        let actions = vec![dahai(0)];
        assert_eq!(agent.act(&ctx, &actions), dahai(0));
    }

    #[test]
    fn falls_back_to_first_dahai_without_reach() {
        let mut agent = ShantenAgent;
        let ctx = GameContext::default();
        let actions = vec![dahai(4), dahai(0)];
        assert_eq!(agent.act(&ctx, &actions), dahai(4));
    }

    #[test]
    fn picks_none_when_no_dahai() {
        let mut agent = ShantenAgent;
        let ctx = GameContext::default();
        let actions = vec![LegalAction::None];
        assert_eq!(agent.act(&ctx, &actions), LegalAction::None);
    }

    // Pon 以外の副露・カン。限定 Pon を追加した後も、これらは積極的に選ばない。
    pub(crate) fn chi_and_kan_actions() -> Vec<LegalAction> {
        vec![
            LegalAction::Chi {
                tile: tile(17),
                consumed: vec![tile(12), tile(20)],
            },
            LegalAction::Daiminkan {
                tile: tile(104),
                consumed: vec![tile(105), tile(106), tile(107)],
            },
            LegalAction::Ankan {
                consumed: vec![tile(72), tile(73), tile(74), tile(75)],
            },
            LegalAction::Kakan {
                tile: tile(124),
                consumed: vec![tile(125), tile(126), tile(127)],
            },
        ]
    }

    #[test]
    fn does_not_actively_claim_chi_or_kans() {
        let mut agent = ShantenAgent;
        let ctx = GameContext::default();
        let actions: Vec<LegalAction> = chi_and_kan_actions()
            .into_iter()
            .chain([LegalAction::None])
            .collect();
        assert_eq!(agent.act(&ctx, &actions), LegalAction::None);
    }

    #[test]
    fn does_not_claim_pon_outside_the_limited_conditions() {
        let mut agent = ShantenAgent;
        let ctx = GameContext::default();
        let actions: Vec<LegalAction> = chi_and_kan_actions()
            .into_iter()
            .chain([
                LegalAction::Pon {
                    tile: tile(108),
                    consumed: vec![tile(109), tile(110)],
                },
                LegalAction::None,
            ])
            .collect();
        assert_eq!(agent.act(&ctx, &actions), LegalAction::None);
    }

    #[test]
    fn falls_back_to_none_for_empty_actions() {
        let mut agent = ShantenAgent;
        let ctx = GameContext::default();
        assert_eq!(agent.act(&ctx, &[]), LegalAction::None);
    }

    #[test]
    fn uses_visible_tiles_for_discard_evaluation() {
        let mut agent = ShantenAgent;
        let hand_values = [0, 4, 8, 12, 17, 20, 24, 28, 32, 48, 53, 56, 36];
        let hand: Vec<_> = hand_values.iter().map(|&value| tile(value)).collect();
        let mut visible = hand.clone();
        visible.extend([68, 69, 70, 71].iter().map(|&value| tile(value)));
        let ctx = GameContext::from_parts_with_visible_tiles(
            Some(tile(68)),
            hand,
            vec![],
            None,
            None,
            visible,
        );
        let actions: Vec<LegalAction> = hand_values
            .iter()
            .map(|&value| dahai(value))
            .chain([dahai(68)])
            .collect();

        let selected = agent.act(&ctx, &actions);
        let LegalAction::Dahai { tile } = selected else {
            panic!("expected dahai");
        };
        assert_eq!(tile.tile_type().to_mjai_string(), "9p");
    }

    // 123m456m789m 34p 55s + ツモ 北。打 北 で 2p / 5p の両面テンパイになり、待ちは8枚。
    const TENPAI_HAND: [u8; 13] = [0, 4, 8, 12, 17, 20, 24, 28, 32, 44, 48, 89, 90];
    const TENPAI_DRAWN: u8 = 116;

    // 2p / 5p を3枚ずつ見せて、テンパイのまま待ち枚数だけを threshold 未満(2枚)にする見え牌。
    pub(crate) const TENPAI_SCARCE_VISIBLE: [u8; 6] = [40, 41, 42, 53, 54, 55];

    pub(crate) fn tenpai_context(extra_visible: &[u8]) -> GameContext {
        let hand: Vec<_> = TENPAI_HAND.iter().map(|&value| tile(value)).collect();
        let mut visible = hand.clone();
        visible.push(tile(TENPAI_DRAWN));
        visible.extend(extra_visible.iter().map(|&value| tile(value)));
        GameContext::from_parts_with_visible_tiles(
            Some(tile(TENPAI_DRAWN)),
            hand,
            vec![],
            None,
            None,
            visible,
        )
    }

    // 同じテンパイ手牌で visible tiles だけを空にした局面。
    fn tenpai_context_without_visible_tiles() -> GameContext {
        let hand: Vec<_> = TENPAI_HAND.iter().map(|&value| tile(value)).collect();
        GameContext::from_parts(Some(tile(TENPAI_DRAWN)), hand)
    }

    pub(crate) fn tenpai_actions() -> Vec<LegalAction> {
        TENPAI_HAND
            .iter()
            .map(|&value| dahai(value))
            .chain([dahai(TENPAI_DRAWN)])
            .chain([LegalAction::Reach])
            .collect()
    }

    // 123456789m 123p 5s + ツモ 北。打 北 で 5s 単騎テンパイになり、待ちは3枚だけ。
    //
    // 14枚をそのまま評価すると受け入れは {5s, 北} の6枚に見えるが、実際に切る打牌を決めた後の
    // 待ちは3枚しかない。旧判断と新判断で結論が変わる代表局面。
    const TANKI_TENPAI_HAND: [u8; 13] = [0, 4, 8, 12, 17, 20, 24, 28, 32, 36, 40, 44, 89];
    const TANKI_TENPAI_DRAWN: u8 = 116;

    fn tanki_tenpai_context() -> GameContext {
        let hand: Vec<_> = TANKI_TENPAI_HAND.iter().map(|&value| tile(value)).collect();
        GameContext::from_parts(Some(tile(TANKI_TENPAI_DRAWN)), hand)
    }

    fn tanki_tenpai_actions() -> Vec<LegalAction> {
        TANKI_TENPAI_HAND
            .iter()
            .map(|&value| dahai(value))
            .chain([dahai(TANKI_TENPAI_DRAWN)])
            .chain([LegalAction::Reach])
            .collect()
    }

    // act() が実際に通ったリーチ判断。診断専用に判断し直さないことを毎回確かめる。
    fn reach_diagnostic(ctx: &GameContext, actions: &[LegalAction]) -> ReachDecisionDiagnostic {
        let diagnostic = diagnose_matching_act(ctx, actions);
        let reach = diagnostic.reach.expect("リーチを検討している");
        assert_eq!(
            reach.should_reach(),
            diagnostic.selected_source == AgentActionSource::Reach
        );
        reach
    }

    #[test]
    fn reaches_when_visible_waits_are_plentiful() {
        let mut agent = ShantenAgent;
        let ctx = tenpai_context(&[]);
        assert_eq!(agent.act(&ctx, &tenpai_actions()), LegalAction::Reach);
    }

    #[test]
    fn skips_reach_when_visible_waits_are_scarce() {
        let mut agent = ShantenAgent;
        let ctx = tenpai_context(&TENPAI_SCARCE_VISIBLE);
        let selected = agent.act(&ctx, &tenpai_actions());
        assert!(matches!(selected, LegalAction::Dahai { .. }));
    }

    #[test]
    fn reaches_when_visible_tiles_empty_even_with_hand() {
        // visible tiles が空でも「空だから無条件にリーチ」ではなく、選んだ打牌の受け入れで
        // 判断する。この手牌は見え牌補正が無くても待ちが8枚あるのでリーチになる。
        let mut agent = ShantenAgent;
        let ctx = tenpai_context_without_visible_tiles();
        assert!(ctx.visible_tiles().is_empty());
        assert_eq!(agent.act(&ctx, &tenpai_actions()), LegalAction::Reach);
    }

    #[test]
    fn does_not_reach_without_hand_information() {
        // 手牌が無く通常打牌 selection が打牌を選べない局面では、リーチ専用の fallback で
        // 無条件にリーチしない。
        let mut agent = ShantenAgent;
        let ctx = GameContext::default();
        let actions = vec![LegalAction::Reach];

        assert_ne!(agent.act(&ctx, &actions), LegalAction::Reach);
        let reach = ShantenAgent::diagnose(&ctx, &actions)
            .reach
            .expect("Push mode でリーチを検討する");
        assert!(!reach.should_reach());
        assert_eq!(reach.reason, ReachDecisionReason::NoSelectedDiscard);
        assert_eq!(reach.selected_discard, None);
        assert_eq!(reach.shanten_after_discard, None);
        assert_eq!(reach.tenpai_wait, None);
    }

    // ---- リーチ判断 ----

    #[test]
    fn reach_uses_the_wait_of_the_selected_discard() {
        // 選んだ打牌後がテンパイで待ちが threshold 以上なら Push mode でリーチする。
        let ctx = tenpai_context(&[]);
        let actions = tenpai_actions();
        let reach = reach_diagnostic(&ctx, &actions);

        assert!(reach.should_reach());
        assert_eq!(reach.reason, ReachDecisionReason::Eligible);
        assert_eq!(reach.selected, Some(LegalAction::Reach));
        assert_eq!(reach.selected_discard, Some(dahai(TENPAI_DRAWN)));
        assert_eq!(reach.shanten_after_discard, Some(0));
        assert_eq!(reach.tsumo_remaining(), Some(8));
        assert_eq!(reach.tsumo_type_count(), Some(2));
    }

    #[test]
    fn does_not_reach_when_the_live_wait_is_below_the_threshold() {
        // テンパイでも待ち枚数が threshold 未満なら、リーチせず通常打牌へ進む。
        let ctx = tenpai_context(&TENPAI_SCARCE_VISIBLE);
        let actions = tenpai_actions();
        let normal = select_discard_action(&ctx, &actions).expect("通常打牌を選べる");
        let diagnostic = diagnose_matching_act(&ctx, &actions);
        let reach = diagnostic.reach.as_ref().expect("リーチを検討している");

        assert_eq!(diagnostic.selected_source, AgentActionSource::NormalDiscard);
        assert_eq!(diagnostic.selected_action, normal);
        assert!(!reach.should_reach());
        assert_eq!(reach.reason, ReachDecisionReason::InsufficientLiveWait);
        assert_eq!(reach.shanten_after_discard, Some(0));
        assert!(reach.tsumo_remaining().expect("テンパイ") < REACH_MIN_REMAINING);
    }

    #[test]
    fn does_not_reach_when_the_selected_discard_is_not_tenpai() {
        // 合法手にリーチがあっても、選んだ打牌後がテンパイでなければ選ばない。
        let hand_values = [0, 4, 8, 12, 17, 20, 24, 28, 32, 36, 40, 56, 89];
        let ctx = GameContext::from_parts(
            Some(tile(116)),
            hand_values.iter().map(|&value| tile(value)).collect(),
        );
        let actions: Vec<LegalAction> = hand_values
            .iter()
            .map(|&value| dahai(value))
            .chain([dahai(116), LegalAction::Reach])
            .collect();
        let reach = reach_diagnostic(&ctx, &actions);

        assert!(!reach.should_reach());
        assert_eq!(reach.reason, ReachDecisionReason::NotTenpai);
        assert!(reach.shanten_after_discard.expect("打牌を選べる") > 0);
        assert_eq!(reach.tenpai_wait, None);
    }

    #[test]
    fn does_not_reach_without_a_legal_reach() {
        // 合法手にリーチが無ければ、テンパイでもリーチを選ばず待ちも求めない。
        let ctx = tenpai_context(&[]);
        let actions: Vec<LegalAction> = tenpai_actions()
            .into_iter()
            .filter(|action| !matches!(action, LegalAction::Reach))
            .collect();
        let reach = reach_diagnostic(&ctx, &actions);

        assert!(!reach.should_reach());
        assert_eq!(reach.reason, ReachDecisionReason::NoLegalReach);
        assert_eq!(reach.tenpai_wait, None);
    }

    #[test]
    fn reach_wait_matches_the_selected_discard_evaluation() {
        // 待ち枚数・種類数は選ばれた打牌評価の受け入れそのもので、リーチ用に計算し直さない。
        for ctx in [
            tenpai_context(&[]),
            tenpai_context(&TENPAI_SCARCE_VISIBLE),
            tenpai_context_without_visible_tiles(),
        ] {
            let actions = tenpai_actions();
            let diagnostic = diagnose_matching_act(&ctx, &actions);
            let reach = diagnostic.reach.as_ref().expect("リーチを検討している");
            let evaluation = diagnostic
                .normal_discard
                .as_ref()
                .expect("通常打牌を評価している")
                .selected
                .as_ref()
                .expect("打牌を選べる");

            assert_eq!(
                reach.shanten_after_discard,
                Some(evaluation.min_shanten_after_discard())
            );
            assert_eq!(
                reach.tsumo_remaining(),
                Some(evaluation.acceptance_total_remaining())
            );
            assert_eq!(
                reach.tsumo_type_count(),
                Some(evaluation.acceptance_type_count())
            );
        }
    }

    #[test]
    fn reach_wait_reflects_visible_tiles_through_the_discard_evaluation() {
        // 見え牌の反映は打牌評価の時点で済んでいる。リーチ判断は補正済みの残枚数をそのまま使う。
        let plentiful = reach_diagnostic(&tenpai_context(&[]), &tenpai_actions());
        let scarce = reach_diagnostic(&tenpai_context(&TENPAI_SCARCE_VISIBLE), &tenpai_actions());

        assert_eq!(plentiful.tsumo_remaining(), Some(8));
        assert_eq!(scarce.tsumo_remaining(), Some(2));
        // 見え牌で消えるのは残枚数だけで、構造上の待ちは変わらない。
        assert_eq!(
            plentiful
                .tenpai_wait
                .as_ref()
                .map(|wait| &wait.structural_waits),
            scarce
                .tenpai_wait
                .as_ref()
                .map(|wait| &wait.structural_waits)
        );
    }

    #[test]
    fn reach_wait_without_visible_tiles_uses_the_evaluation_acceptance() {
        // visible tiles が空でもリーチ専用 fallback を使わず、打牌評価の受け入れで判断する。
        let ctx = tenpai_context_without_visible_tiles();
        let reach = reach_diagnostic(&ctx, &tenpai_actions());

        assert!(ctx.visible_tiles().is_empty());
        assert!(reach.should_reach());
        assert_eq!(reach.tsumo_remaining(), Some(8));

        // 同じく visible tiles が空でも、待ちが足りなければリーチしない。
        let tanki = reach_diagnostic(&tanki_tenpai_context(), &tanki_tenpai_actions());
        assert!(tanki_tenpai_context().visible_tiles().is_empty());
        assert!(!tanki.should_reach());
        assert_eq!(tanki.reason, ReachDecisionReason::InsufficientLiveWait);
        assert_eq!(tanki.tsumo_remaining(), Some(3));
    }

    #[test]
    fn permanent_furiten_is_reported_without_changing_the_reach_policy() {
        // 恒常フリテンのテンパイでも、待ち枚数だけで判断する今回の policy は変えない。
        let ctx = three_sided_context(&[], &[81]);
        let actions: Vec<LegalAction> = three_sided_actions()
            .into_iter()
            .chain([LegalAction::Reach])
            .collect();
        let reach = reach_diagnostic(&ctx, &actions);

        assert_eq!(reach.permanent_furiten(), Some(PermanentFuriten::Yes));
        assert_eq!(reach.can_ron(), Some(false));
        assert_eq!(reach.discarded_waits(), [tile(80).tile_type()]);
        // フリテンを理由にリーチを止めない。
        assert!(reach.should_reach());
        assert_eq!(reach.reason, ReachDecisionReason::Eligible);
    }

    #[test]
    fn an_unknown_player_id_leaves_the_reach_furiten_unknown() {
        // 自分の河を特定できない場合、非フリテンだと推測しない。
        let ctx = three_sided_context_without_player_id(&[81]);
        let actions: Vec<LegalAction> = three_sided_actions()
            .into_iter()
            .chain([LegalAction::Reach])
            .collect();
        let reach = reach_diagnostic(&ctx, &actions);

        assert_eq!(ctx.own_discards(), None);
        assert_eq!(reach.permanent_furiten(), Some(PermanentFuriten::Unknown));
        assert_eq!(reach.can_ron(), None);
        assert!(reach.discarded_waits().is_empty());
        assert!(reach.should_reach());
    }

    #[test]
    fn a_fully_visible_discarded_wait_keeps_the_reach_furiten_diagnostic() {
        // 構造上の待ちの一部が残枚数 0 でも、恒常フリテンは解除されない。
        let ctx = three_sided_context(&[82, 83], &[81]);
        let actions: Vec<LegalAction> = three_sided_actions()
            .into_iter()
            .chain([LegalAction::Reach])
            .collect();
        let reach = reach_diagnostic(&ctx, &actions);
        let tenpai = reach.tenpai_wait.as_ref().expect("テンパイになる");

        let three_sou = tile(80).tile_type();
        assert!(tenpai.structural_waits.contains(&three_sou));
        assert!(!tenpai.live_waits.contains(&three_sou));
        assert_eq!(reach.permanent_furiten(), Some(PermanentFuriten::Yes));
        assert_eq!(reach.can_ron(), Some(false));
        assert_eq!(reach.discarded_waits(), [three_sou]);
    }

    #[test]
    fn reach_and_push_pull_share_the_selected_discard_evaluation() {
        // リーチ判断と押し引きが別々の打牌評価を参照しないことを固定する。
        for (ctx, actions) in [
            (tenpai_context(&[]), tenpai_actions()),
            (tenpai_context(&TENPAI_SCARCE_VISIBLE), tenpai_actions()),
            (tanki_tenpai_context(), tanki_tenpai_actions()),
        ] {
            let diagnostic = diagnose_matching_act(&ctx, &actions);
            let reach = diagnostic.reach.as_ref().expect("リーチを検討している");
            let offense = diagnostic
                .push_pull_inputs
                .as_ref()
                .expect("押し引き入力がある")
                .offense
                .expect("攻撃評価がある");

            assert_eq!(
                reach.shanten_after_discard,
                Some(offense.min_shanten_after_discard)
            );
            assert_eq!(
                reach.tsumo_remaining(),
                Some(offense.acceptance_total_remaining)
            );
            assert_eq!(
                reach.tsumo_type_count(),
                Some(offense.acceptance_type_count)
            );
        }
    }

    #[test]
    fn reach_decision_is_shared_by_act_and_every_diagnose_entry_point() {
        for (ctx, actions) in [
            (tenpai_context(&[]), tenpai_actions()),
            (tenpai_context(&TENPAI_SCARCE_VISIBLE), tenpai_actions()),
            (tanki_tenpai_context(), tanki_tenpai_actions()),
            (three_sided_context(&[], &[81]), {
                three_sided_actions()
                    .into_iter()
                    .chain([LegalAction::Reach])
                    .collect()
            }),
        ] {
            let mut agent = ShantenAgent;
            let acted = agent.act(&ctx, &actions);
            let diagnostic = ShantenAgent::diagnose(&ctx, &actions);
            let with_lookahead = ShantenAgent::diagnose_with_options(
                &ctx,
                &actions,
                DiagnosticOptions::WITH_LOOKAHEAD,
            );

            assert_eq!(diagnostic.selected_action, acted);
            assert_eq!(with_lookahead.selected_action, acted);
            assert_eq!(with_lookahead.reach, diagnostic.reach);
        }
    }

    #[test]
    fn reach_evaluation_does_not_build_the_analysis_diagnostics() {
        // リーチ判断のために全候補フリテン診断や2手先診断を構築しない。必要なのは選択済み
        // 打牌1件分の待ちだけ。
        let ctx = tenpai_context(&[]);
        let mut diagnostics = DecisionDiagnostics::disabled();
        let decision =
            ShantenAgent.decide_with_diagnostics(&ctx, &tenpai_actions(), &mut diagnostics);

        assert_eq!(decision.action, LegalAction::Reach);
        assert!(diagnostics.normal_discard.is_none());
        assert!(diagnostics.normal_discard_furiten.is_none());
        assert!(diagnostics.normal_discard_lookahead.is_none());
        assert!(
            decision
                .reach
                .expect("リーチを検討している")
                .tenpai_wait
                .is_some()
        );
    }

    #[test]
    fn neutral_and_fold_do_not_evaluate_reach() {
        // リーチを検討するのは Push mode だけで、他のモードでは診断も残さない。
        for (ctx, actions, mode) in [
            (
                tenpai_under_reach_context(Some(1), [false, true, false, false]),
                tenpai_actions(),
                PushPullMode::Neutral,
            ),
            (
                fold_under_reach_context(),
                fold_actions(),
                PushPullMode::Fold,
            ),
        ] {
            let diagnostic = diagnose_matching_act(&ctx, &actions);
            assert_eq!(
                diagnostic.push_pull_decision.map(|decision| decision.mode),
                Some(mode)
            );
            assert_eq!(diagnostic.reach, None);
            assert_ne!(diagnostic.selected_action, LegalAction::Reach);
            assert_ne!(diagnostic.selected_source, AgentActionSource::Reach);
        }
    }

    #[test]
    fn follows_discard_selection_for_same_tile_type() {
        let mut agent = ShantenAgent;
        let ctx = GameContext::from_parts(Some(tile(16)), vec![tile(17)]);
        let actions = vec![dahai(17), dahai(16)];

        let expected = select_discard_action(&ctx, &actions).unwrap();

        assert_eq!(agent.act(&ctx, &actions), expected);
    }

    // 他家(player 1)がリーチしており、その河に 16(5m) がある局面。自分は player 0。
    pub(crate) fn opponent_reach_context(
        drawn_tile: Option<u8>,
        hand_values: &[u8],
    ) -> GameContext {
        let discards = [vec![], vec![tile(16)], vec![], vec![]];
        GameContext::from_parts_with_table_state(
            drawn_tile.map(tile),
            hand_values.iter().map(|&value| tile(value)).collect(),
            vec![],
            None,
            None,
            Vec::new(),
            Some(0),
            None,
            discards,
            [false, true, false, false],
        )
    }

    #[test]
    fn prefers_hora_over_genbutsu_fallback() {
        let mut agent = ShantenAgent;
        let ctx = opponent_reach_context(Some(0), &[]);
        let actions = vec![dahai(16), LegalAction::Hora];
        assert_eq!(agent.act(&ctx, &actions), LegalAction::Hora);
    }

    #[test]
    fn prefers_ryukyoku_over_genbutsu_fallback() {
        let mut agent = ShantenAgent;
        let ctx = opponent_reach_context(Some(0), &[]);
        let actions = vec![dahai(16), LegalAction::Ryukyoku];
        assert_eq!(agent.act(&ctx, &actions), LegalAction::Ryukyoku);
    }

    #[test]
    fn prefers_genbutsu_fallback_over_reach() {
        let mut agent = ShantenAgent;
        let ctx = opponent_reach_context(Some(0), &[]);
        let actions = vec![LegalAction::Reach, dahai(0), dahai(16)];
        assert_eq!(agent.act(&ctx, &actions), dahai(16));
    }

    #[test]
    fn neutral_prefers_normal_discard_over_genbutsu_fallback() {
        let mut agent = ShantenAgent;
        // 単独の子リーチに対する強い一向聴で Neutral。共通現物 16(5m) があっても
        // Neutral では通常打牌(浮いた 116(北))を防御 fallback より優先する。
        let hand_values = [0, 4, 8, 12, 13, 20, 24, 28, 32, 36, 40, 44, 89];
        let ctx = opponent_reach_context(Some(116), &hand_values);
        assert_eq!(
            decide_push_pull(&push_pull_inputs_from_context(&ctx)).mode,
            PushPullMode::Neutral
        );
        let actions: Vec<LegalAction> = hand_values
            .iter()
            .map(|&value| dahai(value))
            .chain([dahai(116), dahai(16)])
            .collect();
        let normal = select_discard_action(&ctx, &actions).unwrap();
        assert_eq!(agent.act(&ctx, &actions), normal);
        assert_ne!(agent.act(&ctx, &actions), dahai(16));
    }

    #[test]
    fn fold_without_common_genbutsu_falls_through_to_normal_discard() {
        let mut agent = ShantenAgent;
        // 他家リーチ中でも合法 Dahai に共通現物が無い Fold 局面。Reach は抑制し通常打牌へ進む。
        let ctx = opponent_reach_context(Some(0), &[]);
        assert_eq!(
            decide_push_pull(&push_pull_inputs_from_context(&ctx)).mode,
            PushPullMode::Fold
        );
        let actions = vec![LegalAction::Reach, dahai(0), dahai(56)];
        assert_eq!(agent.act(&ctx, &actions), dahai(0));
    }

    #[test]
    fn keeps_normal_behavior_without_opponent_reach() {
        let mut agent = ShantenAgent;
        // 他家リーチが無ければ、河の 16(5m) と同じ現物相当の 17(5m) が合法でも Reach を選ぶ。
        let ctx = tenpai_under_reach_context(None, [false; 4]);
        let actions = vec![LegalAction::Reach, dahai(TENPAI_DRAWN), dahai(17)];
        assert_eq!(agent.act(&ctx, &actions), LegalAction::Reach);
    }

    #[test]
    fn does_not_claim_melds_even_under_opponent_reach() {
        let mut agent = ShantenAgent;
        // 他家リーチ中でも副露・カンは積極選択しない。共通現物も無い局面。
        let ctx = opponent_reach_context(None, &[]);
        let actions = vec![
            LegalAction::Pon {
                tile: tile(108),
                consumed: vec![tile(109), tile(110)],
            },
            LegalAction::Ankan {
                consumed: vec![tile(72), tile(73), tile(74), tile(75)],
            },
            LegalAction::None,
        ];
        assert_eq!(agent.act(&ctx, &actions), LegalAction::None);
    }

    #[test]
    fn prefers_genbutsu_fallback_over_none() {
        let mut agent = ShantenAgent;
        let ctx = opponent_reach_context(Some(0), &[]);
        let actions = vec![dahai(16), LegalAction::None];
        assert_eq!(agent.act(&ctx, &actions), dahai(16));
    }

    // 他家(player 1)がリーチしており、その河に 16(5m) がある局面に visible_tiles を加える。
    fn opponent_reach_context_with_visible(
        drawn_tile: Option<u8>,
        hand_values: &[u8],
        visible_values: &[u8],
    ) -> GameContext {
        let discards = [vec![], vec![tile(16)], vec![], vec![]];
        GameContext::from_parts_with_table_state(
            drawn_tile.map(tile),
            hand_values.iter().map(|&value| tile(value)).collect(),
            vec![],
            None,
            None,
            visible_values.iter().map(|&value| tile(value)).collect(),
            Some(0),
            None,
            discards,
            [false, true, false, false],
        )
    }

    #[test]
    fn prefers_genbutsu_fallback_over_honor_safety_fallback() {
        let mut agent = ShantenAgent;
        // 共通現物 16(5m) と字牌 108(東) が両方合法でも、現物を優先する。
        let ctx = opponent_reach_context(Some(0), &[]);
        let actions = vec![dahai(108), dahai(16)];
        assert_eq!(agent.act(&ctx, &actions), dahai(16));
    }

    #[test]
    fn picks_safest_honor_dahai_when_no_common_genbutsu() {
        let mut agent = ShantenAgent;
        // 共通現物なし。東は2枚見え、南は0枚見え。より安全な東を切る。
        let ctx = opponent_reach_context_with_visible(Some(112), &[], &[108, 109]);
        let actions = vec![dahai(112), dahai(108)];
        assert_eq!(agent.act(&ctx, &actions), dahai(108));
    }

    #[test]
    fn prefers_honor_safety_fallback_over_reach() {
        let mut agent = ShantenAgent;
        // 共通現物なし。数牌と字牌が合法なら Reach より字牌を切る。
        let ctx = opponent_reach_context(Some(0), &[]);
        let actions = vec![LegalAction::Reach, dahai(0), dahai(108)];
        assert_eq!(agent.act(&ctx, &actions), dahai(108));
    }

    #[test]
    fn prefers_honor_safety_fallback_over_discard_evaluation() {
        let mut agent = ShantenAgent;
        // 通常評価では別牌が選ばれ得る手牌だが、共通現物がなければ字牌 108(東) を優先する。
        let hand_values = [0, 4, 8, 12, 13, 20, 24, 28, 32, 36, 40, 44, 89];
        let ctx = opponent_reach_context(Some(108), &hand_values);
        let actions: Vec<LegalAction> = hand_values
            .iter()
            .map(|&value| dahai(value))
            .chain([dahai(108)])
            .collect();
        assert_eq!(agent.act(&ctx, &actions), dahai(108));
    }

    #[test]
    fn fold_without_common_genbutsu_or_honor_falls_through_to_normal_discard() {
        let mut agent = ShantenAgent;
        // 共通現物も字牌 Dahai もなく、数牌も全て NoSafety の Fold 局面。
        // リーチ者の河は 16(5m) のみで、0(1m) / 56(6p) は無スジ・壁なしの NoSafety。
        // Reach を抑制し、防御牌が無いので通常打牌へ進む。
        let ctx = opponent_reach_context(Some(0), &[]);
        assert_eq!(
            decide_push_pull(&push_pull_inputs_from_context(&ctx)).mode,
            PushPullMode::Fold
        );
        let actions = vec![LegalAction::Reach, dahai(0), dahai(56)];
        assert_eq!(agent.act(&ctx, &actions), dahai(0));
    }

    #[test]
    fn does_not_use_honor_safety_fallback_without_opponent_reach() {
        let mut agent = ShantenAgent;
        // 他家リーチが無ければ、字牌(116 = 北)が合法でも従来の Reach を選ぶ。
        let ctx = tenpai_context(&[]);
        let actions = vec![LegalAction::Reach, dahai(TENPAI_DRAWN)];
        assert_eq!(agent.act(&ctx, &actions), LegalAction::Reach);
    }

    #[test]
    fn honor_safety_fallback_ignores_number_dahai() {
        let mut agent = ShantenAgent;
        // 数牌のみで字牌がなければ字牌 fallback は発動しない。Fold だが安全牌が無いので通常打牌へ進む。
        let ctx = opponent_reach_context(Some(0), &[]);
        assert_eq!(
            decide_push_pull(&push_pull_inputs_from_context(&ctx)).mode,
            PushPullMode::Fold
        );
        let actions = vec![LegalAction::Reach, dahai(0), dahai(56)];
        assert_eq!(agent.act(&ctx, &actions), dahai(0));
    }

    #[test]
    fn honor_safety_fallback_ignores_non_dahai_honor_actions() {
        let mut agent = ShantenAgent;
        // 字牌の Pon はあっても字牌 Dahai が無ければ fallback は発動しない。
        let ctx = opponent_reach_context(Some(0), &[]);
        let actions = vec![
            LegalAction::Pon {
                tile: tile(108),
                consumed: vec![tile(109), tile(110)],
            },
            LegalAction::None,
        ];
        assert_eq!(agent.act(&ctx, &actions), LegalAction::None);
    }

    #[test]
    fn honor_safety_fallback_preserves_order_within_same_rank() {
        let mut agent = ShantenAgent;
        // 東も南も0枚見えで同安全度なら legal_actions の元順序を保つ。
        let ctx = opponent_reach_context(Some(0), &[]);
        let actions = vec![dahai(112), dahai(108)];
        assert_eq!(agent.act(&ctx, &actions), dahai(112));
    }

    fn opponent_reach_wind_context(oya: u8, drawn_tile: Option<u8>) -> GameContext {
        let discards = [vec![], vec![tile(16)], vec![], vec![]];
        GameContext::from_parts_with_table_state(
            drawn_tile.map(tile),
            vec![],
            vec![],
            TileType::new(27),
            None,
            Vec::new(),
            Some(0),
            Some(oya),
            discards,
            [false, true, false, false],
        )
    }

    #[test]
    fn honor_safety_fallback_breaks_same_rank_ties_by_opponent_honor_value() {
        let mut agent = ShantenAgent;
        let ctx = opponent_reach_wind_context(3, Some(0));
        assert_eq!(agent.act(&ctx, &[dahai(132), dahai(120)]), dahai(120));
        assert_eq!(agent.act(&ctx, &[dahai(120), dahai(132)]), dahai(120));

        let ctx = opponent_reach_wind_context(1, Some(0));
        assert_eq!(agent.act(&ctx, &[dahai(108), dahai(132)]), dahai(132));
        assert_eq!(agent.act(&ctx, &[dahai(108), dahai(120)]), dahai(120));
    }

    #[test]
    fn honor_safety_fallback_keeps_visible_count_over_opponent_honor_value() {
        let mut agent = ShantenAgent;
        let ctx = GameContext::from_parts_with_table_state(
            Some(tile(0)),
            vec![],
            vec![],
            TileType::new(27),
            None,
            vec![tile(132), tile(133), tile(134)],
            Some(0),
            Some(3),
            [vec![], vec![tile(16)], vec![], vec![]],
            [false, true, false, false],
        );
        assert_eq!(agent.act(&ctx, &[dahai(120), dahai(135)]), dahai(135));
    }

    // 他家(player 1)がリーチしている局面。河・visible・手牌・引き牌を個別に指定する。
    fn suited_reach_context(
        drawn_tile: Option<u8>,
        hand_values: &[u8],
        visible_values: &[u8],
        reacher_discards: &[u8],
    ) -> GameContext {
        let discards = [
            vec![],
            reacher_discards.iter().map(|&value| tile(value)).collect(),
            vec![],
            vec![],
        ];
        GameContext::from_parts_with_table_state(
            drawn_tile.map(tile),
            hand_values.iter().map(|&value| tile(value)).collect(),
            vec![],
            None,
            None,
            visible_values.iter().map(|&value| tile(value)).collect(),
            Some(0),
            None,
            discards,
            [false, true, false, false],
        )
    }

    #[test]
    fn prefers_genbutsu_fallback_over_suited_safety_fallback() {
        let mut agent = ShantenAgent;
        // 共通現物 16(5m) と NoChance 数牌 4(2m) が両方合法でも、現物を優先する。
        let ctx = suited_reach_context(Some(0), &[], &[4, 5, 6, 7], &[16]);
        let actions = vec![dahai(4), dahai(16)];
        assert_eq!(agent.act(&ctx, &actions), dahai(16));
    }

    #[test]
    fn prefers_honor_safety_fallback_over_suited_safety_fallback() {
        let mut agent = ShantenAgent;
        // 共通現物なし。字牌 108(東) と NoChance 数牌 4(2m) が合法なら字牌を優先する。
        let ctx = suited_reach_context(Some(0), &[], &[4, 5, 6, 7], &[]);
        let actions = vec![dahai(108), dahai(4)];
        assert_eq!(agent.act(&ctx, &actions), dahai(108));
    }

    #[test]
    fn picks_no_chance_suited_dahai_when_no_genbutsu_or_honor() {
        let mut agent = ShantenAgent;
        // 共通現物も字牌もなし。4m を4枚見えにして経路 [3m,4m] を Blocked にし 2m を NoChance。
        // 無スジ 0(1m) より NoChance 4(2m) を選ぶ。
        let ctx = suited_reach_context(Some(0), &[], &[12, 13, 14, 15], &[]);
        let actions = vec![dahai(0), dahai(4)];
        assert_eq!(agent.act(&ctx, &actions), dahai(4));
    }

    #[test]
    fn picks_one_chance_suited_dahai_when_no_genbutsu_or_honor() {
        let mut agent = ShantenAgent;
        // 共通現物も字牌もなし。4m を3枚見えにして経路 [3m,4m] を OneChance にし 2m を OneChance。
        // 無スジ 0(1m) より OneChance 4(2m) を選ぶ。
        let ctx = suited_reach_context(Some(0), &[], &[12, 13, 14], &[]);
        let actions = vec![dahai(0), dahai(4)];
        assert_eq!(agent.act(&ctx, &actions), dahai(4));
    }

    #[test]
    fn picks_suji_suited_dahai_when_no_genbutsu_or_honor() {
        let mut agent = ShantenAgent;
        // 共通現物も字牌もなし。リーチ者の河に 12(4m) があり 0(1m) はスジ。無スジ 16(5m) より選ぶ。
        let ctx = suited_reach_context(Some(0), &[], &[], &[12]);
        let actions = vec![dahai(16), dahai(0)];
        assert_eq!(agent.act(&ctx, &actions), dahai(0));
    }

    #[test]
    fn suited_safety_fallback_follows_safety_order() {
        let mut agent = ShantenAgent;
        // 経路壁で安全度を作る。1p は 2p 4枚で NoChance、9p は 8p 3枚で OneChance、
        // 1s は 4s(84) 河でスジ(Suji)、5s は無スジ・壁なし(NoSafety)。最も安全な NoChance を選ぶ。
        let ctx = suited_reach_context(Some(0), &[], &[40, 41, 42, 43, 64, 65, 66], &[84]);
        let actions = vec![dahai(88), dahai(72), dahai(68), dahai(36)];
        assert_eq!(agent.act(&ctx, &actions), dahai(36));
    }

    #[test]
    fn fold_without_safe_suited_falls_through_to_normal_discard() {
        let mut agent = ShantenAgent;
        // Fold 局面で共通現物も字牌もなく数牌が全て NoSafety なら、防御 fallback は無い。
        // Reach は抑制し、防御牌がないことを理由に失敗させず通常打牌へ進む。
        let ctx = suited_reach_context(Some(0), &[], &[], &[]);
        assert_eq!(
            decide_push_pull(&push_pull_inputs_from_context(&ctx)).mode,
            PushPullMode::Fold
        );
        let actions = vec![LegalAction::Reach, dahai(0), dahai(4)];
        assert_eq!(agent.act(&ctx, &actions), dahai(0));
    }

    #[test]
    fn does_not_use_suited_safety_fallback_without_opponent_reach() {
        let mut agent = ShantenAgent;
        // 他家リーチが無ければ、河に 16(5m) があり 4(2m) がスジ相当でも従来の Reach を選ぶ。
        let ctx = tenpai_under_reach_context(None, [false; 4]);
        let actions = vec![LegalAction::Reach, dahai(TENPAI_DRAWN), dahai(4)];
        assert_eq!(agent.act(&ctx, &actions), LegalAction::Reach);
    }

    #[test]
    fn prefers_suited_safety_fallback_over_reach() {
        let mut agent = ShantenAgent;
        // 共通現物も字牌もなし。4m を4枚見えにして 2m を NoChance にすると Reach より優先する。
        let ctx = suited_reach_context(Some(0), &[], &[12, 13, 14, 15], &[]);
        let actions = vec![LegalAction::Reach, dahai(0), dahai(4)];
        assert_eq!(agent.act(&ctx, &actions), dahai(4));
    }

    #[test]
    fn push_prefers_normal_discard_over_suited_safety_fallback() {
        let mut agent = ShantenAgent;
        // テンパイで単独の子リーチに対しては Push。Reach が合法でなければ通常打牌へ進み、
        // 防御 fallback より通常打牌(32(9m))を優先する。
        let hand_values = [0, 4, 8, 12, 13, 20, 24, 28, 32, 36, 40, 44, 89];
        let ctx = suited_reach_context(Some(88), &hand_values, &[4, 5, 6, 7], &[]);
        assert_eq!(
            decide_push_pull(&push_pull_inputs_from_context(&ctx)).mode,
            PushPullMode::Push
        );
        let actions: Vec<LegalAction> = hand_values
            .iter()
            .map(|&value| dahai(value))
            .chain([dahai(88)])
            .collect();
        let normal = select_discard_action(&ctx, &actions).unwrap();
        assert_eq!(agent.act(&ctx, &actions), normal);
        assert_ne!(agent.act(&ctx, &actions), dahai(4));
    }

    #[test]
    fn suited_safety_fallback_ignores_non_dahai_actions() {
        let mut agent = ShantenAgent;
        // 数牌の Pon はあっても数牌 Dahai が無ければ数牌防御 fallback は発動しない。
        let ctx = suited_reach_context(Some(0), &[], &[4, 5, 6, 7], &[]);
        let actions = vec![
            LegalAction::Pon {
                tile: tile(4),
                consumed: vec![tile(5), tile(6)],
            },
            LegalAction::None,
        ];
        assert_eq!(agent.act(&ctx, &actions), LegalAction::None);
    }

    // テンパイ手牌で他家リーチを受ける局面。oya と reached を指定してモードを作り分ける。
    // visible は空にして、待ち枚数の見え牌補正が入らない状態で押し引きの分岐だけを確認する。
    fn tenpai_under_reach_context(oya: Option<u8>, reached: [bool; 4]) -> GameContext {
        let hand: Vec<_> = TENPAI_HAND.iter().map(|&value| tile(value)).collect();
        // リーチ者(player 1)の河に 5m を置き、テンパイ手牌の 5m を現物にする。
        let discards = [vec![], vec![tile(16)], vec![], vec![]];
        GameContext::from_parts_with_table_state(
            Some(tile(TENPAI_DRAWN)),
            hand,
            vec![],
            None,
            None,
            Vec::new(),
            Some(0),
            oya,
            discards,
            reached,
        )
    }

    #[test]
    fn push_tenpai_against_single_non_dealer_reaches() {
        let mut agent = ShantenAgent;
        // 単独の子リーチに対するテンパイ。decide_push_pull は Push。
        // Reach が合法でリーチ判断も Eligible なら、現物があっても Reach を選ぶ。
        let ctx = tenpai_under_reach_context(None, [false, true, false, false]);
        assert_eq!(
            decide_push_pull(&push_pull_inputs_from_context(&ctx)).mode,
            PushPullMode::Push
        );
        let actions = tenpai_actions();
        assert_eq!(
            reach_diagnostic(&ctx, &actions).reason,
            ReachDecisionReason::Eligible
        );
        assert_eq!(agent.act(&ctx, &actions), LegalAction::Reach);
    }

    #[test]
    fn neutral_tenpai_against_dealer_reach_prefers_normal_discard_over_reach() {
        let mut agent = ShantenAgent;
        // 親リーチに対するテンパイ。decide_push_pull は Neutral。
        // Reach が合法でも選ばず、暫定的にダマ相当の通常打牌を優先する。
        let ctx = tenpai_under_reach_context(Some(1), [false, true, false, false]);
        assert_eq!(
            decide_push_pull(&push_pull_inputs_from_context(&ctx)).mode,
            PushPullMode::Neutral
        );
        let actions = tenpai_actions();
        let normal = select_discard_action(&ctx, &actions).unwrap();
        let selected = agent.act(&ctx, &actions);
        assert_eq!(selected, normal);
        assert_ne!(selected, LegalAction::Reach);
    }

    #[test]
    fn neutral_tenpai_against_multiple_reach_prefers_normal_discard_over_reach() {
        let mut agent = ShantenAgent;
        // 複数リーチに対するテンパイ。decide_push_pull は Neutral。Reach は選ばない。
        let ctx = tenpai_under_reach_context(None, [false, true, true, false]);
        assert_eq!(
            decide_push_pull(&push_pull_inputs_from_context(&ctx)).mode,
            PushPullMode::Neutral
        );
        let actions = tenpai_actions();
        let normal = select_discard_action(&ctx, &actions).unwrap();
        let selected = agent.act(&ctx, &actions);
        assert_eq!(selected, normal);
        assert_ne!(selected, LegalAction::Reach);
    }

    // 2向聴以上で他家リーチを受ける Fold 局面。リーチ者の河に 5s を置き手牌の 5s を現物にする。
    const FOLD_HAND: [u8; 13] = [0, 4, 17, 20, 36, 40, 56, 60, 89, 108, 112, 120, 124];
    const FOLD_DRAWN: u8 = 16;

    fn fold_under_reach_context() -> GameContext {
        let hand: Vec<_> = FOLD_HAND.iter().map(|&value| tile(value)).collect();
        let discards = [vec![], vec![tile(89)], vec![], vec![]];
        GameContext::from_parts_with_table_state(
            Some(tile(FOLD_DRAWN)),
            hand,
            vec![],
            None,
            None,
            Vec::new(),
            Some(0),
            None,
            discards,
            [false, true, false, false],
        )
    }

    fn fold_actions() -> Vec<LegalAction> {
        FOLD_HAND
            .iter()
            .map(|&value| dahai(value))
            .chain([dahai(FOLD_DRAWN)])
            .collect()
    }

    #[test]
    fn fold_prefers_defense_fallback_over_normal_discard() {
        let mut agent = ShantenAgent;
        // 二向聴以上で他家リーチを受ける Fold 局面。防御 fallback(現物 5s)と通常打牌が異なり、
        // Fold では防御 fallback を通常打牌より優先する。
        let ctx = fold_under_reach_context();
        assert_eq!(
            decide_push_pull(&push_pull_inputs_from_context(&ctx)).mode,
            PushPullMode::Fold
        );
        let actions = fold_actions();
        let normal = select_discard_action(&ctx, &actions).unwrap();
        let defense = select_defense_fallback_action(&ctx, &actions)
            .cloned()
            .unwrap();
        assert_ne!(normal, defense);
        assert_eq!(agent.act(&ctx, &actions), defense);
        assert_eq!(defense, dahai(89));
    }

    #[test]
    fn agent_push_pull_uses_legal_candidate_evaluation_not_illegal_global_best() {
        // 全体最善(手牌の東を切ってテンパイ = shanten 0)は非合法で、合法なのはツモ切り 3p だけ。
        // 非合法な全体最善を使うと Push、合法なツモ切り(shanten 1)を使うと Neutral になる。
        // Agent は合法候補側の evaluation / mode を使う。
        let hand_values = [0, 4, 8, 12, 17, 20, 24, 28, 32, 36, 40, 89, 108];
        let drawn = 44; // 3p ツモ
        let discards = [vec![], vec![tile(16)], vec![], vec![]];
        let ctx = GameContext::from_parts_with_table_state(
            Some(tile(drawn)),
            hand_values.iter().map(|&value| tile(value)).collect(),
            vec![],
            None,
            None,
            Vec::new(),
            Some(0),
            None,
            discards,
            [false, true, false, false],
        );
        let tiles: Vec<_> = ctx
            .hand_tiles()
            .iter()
            .copied()
            .chain(ctx.drawn_tile())
            .collect();

        // 非合法な全体最善候補の mode は Push。
        let global_best = select_best_normal_discard_evaluation(&ctx, &tiles).unwrap();
        assert_eq!(global_best.min_shanten_after_discard(), 0);
        let illegal_mode = decide_push_pull(&push_pull_inputs_from_context_with_evaluation(
            &ctx,
            Some(&global_best),
        ))
        .mode;
        assert_eq!(illegal_mode, PushPullMode::Push);

        // 合法なのはツモ切り 3p だけ。Agent が使う offense は合法候補の評価に一致する。
        let actions = vec![dahai(drawn)];
        let selection = select_discard_action_with_evaluation(&ctx, &actions);
        let legal_evaluation = selection.evaluation.clone().unwrap();
        assert_eq!(legal_evaluation.min_shanten_after_discard(), 1);
        let legal_inputs =
            push_pull_inputs_from_context_with_evaluation(&ctx, selection.evaluation.as_ref());
        let legal_mode = decide_push_pull(&legal_inputs).mode;

        assert_ne!(illegal_mode, legal_mode);
        assert_eq!(legal_mode, PushPullMode::Neutral);
        assert_eq!(
            legal_inputs.offense.unwrap().min_shanten_after_discard,
            legal_evaluation.min_shanten_after_discard()
        );

        // Agent は合法候補(ツモ切り 3p)を切る。
        let mut agent = ShantenAgent;
        assert_eq!(agent.act(&ctx, &actions), dahai(drawn));
    }

    // ---- decide() の選択経路(AgentActionSource)テスト ----
    // tracing の出力文字列に依存せず、pure な診断構造(AgentDecision)を検証する。

    #[test]
    fn decide_reports_hora_source() {
        let agent = ShantenAgent;
        let ctx = opponent_reach_context(Some(0), &[]);
        let actions = vec![dahai(16), LegalAction::Hora];
        let decision = agent.decide(&ctx, &actions);
        assert_eq!(decision.action, LegalAction::Hora);
        assert_eq!(decision.source, AgentActionSource::Hora);
        assert_eq!(decision.push_pull, None);
        assert_eq!(decision.normal_discard, None);
        assert_eq!(decision.source.defense_kind(), None);
    }

    #[test]
    fn decide_reports_ryukyoku_source() {
        let agent = ShantenAgent;
        let ctx = opponent_reach_context(Some(0), &[]);
        let actions = vec![dahai(16), LegalAction::Ryukyoku];
        let decision = agent.decide(&ctx, &actions);
        assert_eq!(decision.action, LegalAction::Ryukyoku);
        assert_eq!(decision.source, AgentActionSource::Ryukyoku);
        assert_eq!(decision.push_pull, None);
        assert_eq!(decision.normal_discard, None);
    }

    #[test]
    fn decide_reports_reach_source_on_push() {
        let agent = ShantenAgent;
        // 他家リーチなし → Push。選んだ打牌後のテンパイが十分な待ちを持つなら Reach を選ぶ。
        let ctx = tenpai_context(&[]);
        let actions = tenpai_actions();
        let decision = agent.decide(&ctx, &actions);
        assert_eq!(decision.action, LegalAction::Reach);
        assert_eq!(decision.source, AgentActionSource::Reach);
        assert_eq!(decision.push_pull.map(|d| d.mode), Some(PushPullMode::Push));
        assert_eq!(
            decision.reach.map(|reach| reach.reason),
            Some(ReachDecisionReason::Eligible)
        );
    }

    #[test]
    fn decide_reports_normal_discard_source_on_push() {
        let agent = ShantenAgent;
        // 他家リーチなし → Push。Reach が無ければ通常打牌。
        let hand_values = [0, 4, 8, 12, 17, 20, 24, 28, 32, 36, 40, 44, 89];
        let ctx = GameContext::from_parts(
            Some(tile(116)),
            hand_values.iter().map(|&value| tile(value)).collect(),
        );
        let actions: Vec<LegalAction> = hand_values
            .iter()
            .map(|&value| dahai(value))
            .chain([dahai(116)])
            .collect();
        let normal = select_discard_action(&ctx, &actions).unwrap();
        let decision = agent.decide(&ctx, &actions);
        assert_eq!(decision.source, AgentActionSource::NormalDiscard);
        assert_eq!(decision.action, normal);
        assert_eq!(decision.normal_discard, Some(normal));
    }

    #[test]
    fn decide_reports_normal_discard_source_on_neutral() {
        let agent = ShantenAgent;
        // 単独の子リーチに対する強い一向聴で Neutral。共通現物があっても通常打牌を選ぶ。
        let hand_values = [0, 4, 8, 12, 13, 20, 24, 28, 32, 36, 40, 44, 89];
        let ctx = opponent_reach_context(Some(116), &hand_values);
        assert_eq!(
            decide_push_pull(&push_pull_inputs_from_context(&ctx)).mode,
            PushPullMode::Neutral
        );
        let actions: Vec<LegalAction> = hand_values
            .iter()
            .map(|&value| dahai(value))
            .chain([dahai(116), dahai(16)])
            .collect();
        let normal = select_discard_action(&ctx, &actions).unwrap();
        let decision = agent.decide(&ctx, &actions);
        assert_eq!(decision.source, AgentActionSource::NormalDiscard);
        assert_eq!(decision.action, normal);
        assert_eq!(
            decision.push_pull.map(|d| d.mode),
            Some(PushPullMode::Neutral)
        );
    }

    #[test]
    fn decide_reports_defense_fallback_genbutsu_source_on_fold() {
        let agent = ShantenAgent;
        // 二向聴以上の Fold 局面。防御 fallback(現物 5s)を採用し、通常打牌とは異なる。
        let ctx = fold_under_reach_context();
        let actions = fold_actions();
        let normal = select_discard_action(&ctx, &actions).unwrap();
        let defense = select_defense_fallback_action(&ctx, &actions)
            .cloned()
            .unwrap();
        let decision = agent.decide(&ctx, &actions);
        assert_eq!(
            decision.source,
            AgentActionSource::DefenseFallback(DefenseFallbackKind::Genbutsu)
        );
        assert_eq!(decision.action, defense);
        assert_eq!(decision.normal_discard, Some(normal.clone()));
        assert_ne!(decision.normal_discard, Some(decision.action.clone()));
        assert_ne!(normal, defense);
        assert_eq!(decision.push_pull.map(|d| d.mode), Some(PushPullMode::Fold));
        assert_eq!(
            decision.source.defense_kind(),
            Some(DefenseFallbackKind::Genbutsu)
        );
    }

    // 回帰構造: normal_discard(通常打牌)と最終 selected_action(防御 fallback)が
    // Fold 時に異なり得ることを、SuitedSafety 経路で確認する。
    // 実牌姿での防御は河・visible の正確な再現が必要なため、pure な選択経路として構築する。
    #[test]
    fn decide_reports_defense_fallback_suited_safety_source_on_fold() {
        use crate::defense::SuitedSafetyRank;

        let agent = ShantenAgent;
        // 共通現物も字牌もなし。4m を4枚見せて 2m を NoChance にする。
        // ツモ 1m(0) だけが手牌評価対象なので通常打牌は 1m、防御 fallback は NoChance の 2m。
        let ctx = suited_reach_context(Some(0), &[], &[12, 13, 14, 15], &[]);
        let actions = vec![dahai(0), dahai(4)];
        assert_eq!(
            decide_push_pull(&push_pull_inputs_from_context(&ctx)).mode,
            PushPullMode::Fold
        );
        let decision = agent.decide(&ctx, &actions);
        assert_eq!(decision.action, dahai(4));
        assert_eq!(
            decision.source,
            AgentActionSource::DefenseFallback(DefenseFallbackKind::SuitedSafety(
                SuitedSafetyRank::NoChance
            ))
        );
        assert_eq!(decision.normal_discard, Some(dahai(0)));
        assert_ne!(decision.normal_discard, Some(decision.action.clone()));
    }

    #[test]
    fn decide_falls_through_to_normal_discard_when_fold_has_no_defense() {
        let agent = ShantenAgent;
        // Fold だが共通現物・字牌・数牌 safety がいずれも無い局面。通常打牌へ進む。
        let ctx = suited_reach_context(Some(0), &[], &[], &[]);
        assert_eq!(
            decide_push_pull(&push_pull_inputs_from_context(&ctx)).mode,
            PushPullMode::Fold
        );
        let actions = vec![LegalAction::Reach, dahai(0), dahai(4)];
        let normal = select_discard_action(&ctx, &actions).unwrap();
        let decision = agent.decide(&ctx, &actions);
        assert_eq!(decision.source, AgentActionSource::NormalDiscard);
        assert_eq!(decision.action, normal);
        assert_eq!(decision.normal_discard, Some(normal));
    }

    #[test]
    fn legal_dahai_fallback_prefers_black_five() {
        // 手牌評価が作れず(手牌なし)、他家リーチも無い局面。通常打牌も防御 fallback も None で
        // LegalDahaiFallback へ落ちる。合法 Dahai [赤5m, 黒5m] なら黒5m を返す。
        let agent = ShantenAgent;
        let ctx = GameContext::default();
        let actions = vec![dahai(16), dahai(17)];
        let decision = agent.decide(&ctx, &actions);
        assert_eq!(decision.action, dahai(17));
        assert_eq!(decision.source, AgentActionSource::LegalDahaiFallback);
    }

    #[test]
    fn legal_dahai_fallback_prefers_black_five_when_reversed() {
        let agent = ShantenAgent;
        let ctx = GameContext::default();
        let actions = vec![dahai(17), dahai(16)];
        let decision = agent.decide(&ctx, &actions);
        assert_eq!(decision.action, dahai(17));
        assert_eq!(decision.source, AgentActionSource::LegalDahaiFallback);
    }

    #[test]
    fn legal_dahai_fallback_keeps_red_five_when_only_red() {
        let agent = ShantenAgent;
        let ctx = GameContext::default();
        let actions = vec![dahai(16)];
        let decision = agent.decide(&ctx, &actions);
        assert_eq!(decision.action, dahai(16));
        assert_eq!(decision.source, AgentActionSource::LegalDahaiFallback);
    }

    #[test]
    fn legal_dahai_fallback_keeps_leading_tile_type() {
        // 合法 Dahai [1p, 赤5m, 黒5m] では先頭牌種 1p を維持する。黒5優先で 5m を前へ出さない。
        let agent = ShantenAgent;
        let ctx = GameContext::default();
        let actions = vec![dahai(36), dahai(16), dahai(17)];
        let decision = agent.decide(&ctx, &actions);
        assert_eq!(decision.action, dahai(36));
        assert_eq!(decision.source, AgentActionSource::LegalDahaiFallback);
    }

    #[test]
    fn decide_reports_none_source_for_empty_actions() {
        let agent = ShantenAgent;
        let ctx = GameContext::default();
        let decision = agent.decide(&ctx, &[]);
        assert_eq!(decision.action, LegalAction::None);
        assert_eq!(decision.source, AgentActionSource::None);
    }

    // ---- 構造化診断 (ShantenAgent::diagnose) テスト ----
    // 診断は act() と同じ selection logic を通るため、最終 action は常に act() と一致する。

    // 診断の最終 action / source が act() と一致することを確認し、診断を返す共通 helper。
    fn diagnose_matching_act(
        ctx: &GameContext,
        actions: &[LegalAction],
    ) -> ShantenDecisionDiagnostic {
        let mut agent = ShantenAgent;
        let expected = agent.act(ctx, actions);
        let diagnostic = ShantenAgent::diagnose(ctx, actions);
        assert_eq!(diagnostic.selected_action, expected);
        assert_eq!(
            diagnostic.selected_source,
            agent.decide(ctx, actions).source
        );
        diagnostic
    }

    #[test]
    fn diagnose_free_function_matches_associated_function() {
        let ctx = fold_under_reach_context();
        let actions = fold_actions();
        assert_eq!(
            diagnose_shanten_decision(&ctx, &actions),
            ShantenAgent::diagnose(&ctx, &actions)
        );
    }

    #[test]
    fn diagnose_reports_hora_without_other_judgments() {
        let ctx = opponent_reach_context(Some(0), &[]);
        let actions = vec![dahai(16), LegalAction::Hora];
        let diagnostic = diagnose_matching_act(&ctx, &actions);

        assert_eq!(diagnostic.selected_action, LegalAction::Hora);
        assert_eq!(diagnostic.selected_source, AgentActionSource::Hora);
        assert_eq!(diagnostic.normal_discard, None);
        assert_eq!(diagnostic.normal_discard_action, None);
        assert_eq!(diagnostic.push_pull_inputs, None);
        assert_eq!(diagnostic.push_pull_decision, None);
        assert_eq!(diagnostic.defense, None);
        assert_eq!(diagnostic.defense_fallback_kind(), None);
    }

    #[test]
    fn diagnose_reports_ryukyoku_without_other_judgments() {
        let ctx = opponent_reach_context(Some(0), &[]);
        let actions = vec![dahai(16), LegalAction::Ryukyoku];
        let diagnostic = diagnose_matching_act(&ctx, &actions);

        assert_eq!(diagnostic.selected_action, LegalAction::Ryukyoku);
        assert_eq!(diagnostic.selected_source, AgentActionSource::Ryukyoku);
        assert_eq!(diagnostic.normal_discard, None);
        assert_eq!(diagnostic.normal_discard_action, None);
        assert_eq!(diagnostic.push_pull_inputs, None);
        assert_eq!(diagnostic.push_pull_decision, None);
        assert_eq!(diagnostic.defense, None);
    }

    #[test]
    fn diagnose_reports_reach_source() {
        // 待ち枚数が十分なテンパイで Reach が選ばれる局面。
        let ctx = tenpai_context(&[]);
        let actions = tenpai_actions();
        let diagnostic = diagnose_matching_act(&ctx, &actions);

        assert_eq!(diagnostic.selected_action, LegalAction::Reach);
        assert_eq!(diagnostic.selected_source, AgentActionSource::Reach);
        // Reach 経路でも通常打牌評価は実行済みなので、比較用の通常打牌は保持する。
        assert!(diagnostic.normal_discard.is_some());
        assert!(diagnostic.normal_discard_action.is_some());
        // リーチ判断は通常打牌 selection が選んだ打牌に基づく。
        let reach = diagnostic.reach.as_ref().expect("リーチを検討している");
        assert!(reach.should_reach());
        assert_eq!(reach.reason, ReachDecisionReason::Eligible);
        assert_eq!(
            reach.selected_discard.as_ref(),
            diagnostic.normal_discard_action.as_ref()
        );
        assert_eq!(
            diagnostic.push_pull_decision.map(|decision| decision.mode),
            Some(PushPullMode::Push)
        );
        // Reach を採用したので防御 fallback は検討していない。
        assert_eq!(diagnostic.defense, None);
    }

    #[test]
    fn diagnose_reports_normal_discard_source_with_matching_selection() {
        let hand_values = [0, 4, 8, 12, 17, 20, 24, 28, 32, 36, 40, 44, 89];
        let ctx = GameContext::from_parts(
            Some(tile(116)),
            hand_values.iter().map(|&value| tile(value)).collect(),
        );
        let actions: Vec<LegalAction> = hand_values
            .iter()
            .map(|&value| dahai(value))
            .chain([dahai(116)])
            .collect();
        let normal = select_discard_action(&ctx, &actions).unwrap();

        let diagnostic = diagnose_matching_act(&ctx, &actions);

        assert_eq!(diagnostic.selected_source, AgentActionSource::NormalDiscard);
        assert_eq!(diagnostic.selected_action, normal);
        assert_eq!(diagnostic.normal_discard_action, Some(normal.clone()));

        // 診断内の selected discard が実 action の牌種と一致する。
        let LegalAction::Dahai {
            tile: selected_tile,
        } = normal
        else {
            panic!("expected dahai");
        };
        let selected = diagnostic
            .normal_discard
            .as_ref()
            .unwrap()
            .selected
            .as_ref();
        assert_eq!(selected.map(|e| e.discard), Some(selected_tile.tile_type()));
        assert_eq!(diagnostic.defense, None);
    }

    #[test]
    fn diagnose_keeps_only_legal_discard_candidates_with_comparison_reasons() {
        // 手牌には他の候補があるが、合法 Dahai は 1m / 5s / 北 の3種だけ。
        let hand_values = [0, 4, 8, 12, 17, 20, 24, 28, 32, 36, 40, 44, 89];
        let ctx = GameContext::from_parts(
            Some(tile(116)),
            hand_values.iter().map(|&value| tile(value)).collect(),
        );
        let actions = vec![dahai(0), dahai(89), dahai(116)];

        let diagnostic = diagnose_matching_act(&ctx, &actions);
        let normal_discard = diagnostic.normal_discard.as_ref().unwrap();

        let legal_types: Vec<_> = [0u8, 89, 116]
            .iter()
            .map(|&value| tile(value).tile_type())
            .collect();
        let candidate_types: Vec<_> = normal_discard
            .candidates
            .iter()
            .map(|candidate| candidate.evaluation.discard)
            .collect();
        assert_eq!(candidate_types, legal_types);

        // 選択された候補は1件で、最終 action と牌種が一致する。
        let selected: Vec<_> = normal_discard
            .candidates
            .iter()
            .filter(|candidate| candidate.selected)
            .collect();
        assert_eq!(selected.len(), 1);
        let LegalAction::Dahai {
            tile: selected_tile,
        } = &diagnostic.selected_action
        else {
            panic!("expected dahai");
        };
        assert_eq!(selected[0].evaluation.discard, selected_tile.tile_type());
        assert_eq!(
            selected[0].comparison_reason,
            DiscardComparisonReason::StableOrder
        );

        // 非選択候補は「何の比較軸で負けたか」を持つ。
        for candidate in normal_discard
            .candidates
            .iter()
            .filter(|candidate| !candidate.selected)
        {
            assert_eq!(
                candidate.selected_is_strictly_better_than_candidate,
                compare_discard_evaluations(
                    normal_discard.selected.as_ref().unwrap(),
                    &candidate.evaluation
                )
                .candidate_is_better
            );
        }
    }

    #[test]
    fn diagnose_matches_act_at_physical_tile_level_for_black_and_red_five() {
        // 赤5m と黒5m が同一牌種として合法。黒5優先を維持し、評価も黒5mの物理牌情報に合わせる。
        let ctx = GameContext::from_parts_with_dora(None, vec![tile(16), tile(17)], vec![tile(12)]);
        let actions = vec![dahai(16), dahai(17)];

        let diagnostic = diagnose_matching_act(&ctx, &actions);
        assert_eq!(diagnostic.selected_action, dahai(17));
        assert_eq!(diagnostic.selected_source, AgentActionSource::NormalDiscard);

        let selected = diagnostic
            .normal_discard
            .as_ref()
            .unwrap()
            .selected
            .as_ref()
            .unwrap();
        assert!(!selected.discards_red_five);
        assert_eq!(selected.discarded_dora_count, 1);
    }

    #[test]
    fn diagnose_matches_act_when_only_red_five_is_legal() {
        let ctx = GameContext::from_parts_with_dora(None, vec![tile(16), tile(17)], vec![tile(12)]);
        let actions = vec![dahai(16)];

        let diagnostic = diagnose_matching_act(&ctx, &actions);
        assert_eq!(diagnostic.selected_action, dahai(16));

        let selected = diagnostic
            .normal_discard
            .as_ref()
            .unwrap()
            .selected
            .as_ref()
            .unwrap();
        assert!(selected.discards_red_five);
        assert_eq!(selected.discarded_dora_count, 2);
    }

    // 押し引き診断が実際の push-pull 結果と一致することを確認する共通 helper。
    fn assert_push_pull_diagnostic(
        ctx: &GameContext,
        actions: &[LegalAction],
        expected_mode: PushPullMode,
    ) -> ShantenDecisionDiagnostic {
        let diagnostic = diagnose_matching_act(ctx, actions);
        let inputs = diagnostic.push_pull_inputs.unwrap();
        let decision = diagnostic.push_pull_decision.unwrap();

        let selection = select_discard_action_with_evaluation(ctx, actions);
        assert_eq!(
            inputs,
            push_pull_inputs_from_context_with_evaluation(ctx, selection.evaluation.as_ref())
        );
        assert_eq!(decision, decide_push_pull(&inputs));
        assert_eq!(decision.mode, expected_mode);
        diagnostic
    }

    #[test]
    fn diagnose_holds_push_inputs_and_decision() {
        // テンパイで単独の子リーチに対する Push。
        let hand_values = [0, 4, 8, 12, 13, 20, 24, 28, 32, 36, 40, 44, 89];
        let ctx = suited_reach_context(Some(88), &hand_values, &[4, 5, 6, 7], &[]);
        let actions: Vec<LegalAction> = hand_values
            .iter()
            .map(|&value| dahai(value))
            .chain([dahai(88)])
            .collect();

        let diagnostic = assert_push_pull_diagnostic(&ctx, &actions, PushPullMode::Push);
        let inputs = diagnostic.push_pull_inputs.unwrap();
        assert_eq!(inputs.opponent_reach_count, 1);
        assert!(!inputs.dealer_reacher);
        assert!(!inputs.self_dealer);
        assert_eq!(
            inputs.offense.unwrap().min_shanten_after_discard,
            diagnostic
                .normal_discard
                .as_ref()
                .unwrap()
                .selected
                .as_ref()
                .unwrap()
                .min_shanten_after_discard()
        );
        assert_eq!(
            diagnostic.push_pull_decision.unwrap().reason,
            PushPullReason::TenpaiAgainstSingleNonDealer
        );
    }

    #[test]
    fn diagnose_holds_neutral_inputs_and_decision() {
        // 単独の子リーチに対する強い一向聴で Neutral。
        let hand_values = [0, 4, 8, 12, 13, 20, 24, 28, 32, 36, 40, 44, 89];
        let ctx = opponent_reach_context(Some(116), &hand_values);
        let actions: Vec<LegalAction> = hand_values
            .iter()
            .map(|&value| dahai(value))
            .chain([dahai(116), dahai(16)])
            .collect();

        let diagnostic = assert_push_pull_diagnostic(&ctx, &actions, PushPullMode::Neutral);
        assert_eq!(diagnostic.selected_source, AgentActionSource::NormalDiscard);
        // Neutral で通常打牌を採用したので防御 fallback は検討していない。
        assert_eq!(diagnostic.defense, None);
    }

    #[test]
    fn diagnose_holds_fold_inputs_and_decision() {
        let ctx = fold_under_reach_context();
        let actions = fold_actions();

        let diagnostic = assert_push_pull_diagnostic(&ctx, &actions, PushPullMode::Fold);
        assert_eq!(
            diagnostic.push_pull_decision.unwrap().reason,
            PushPullReason::TwoOrMoreShanten
        );
    }

    #[test]
    fn diagnose_reports_genbutsu_defense_fallback() {
        let ctx = fold_under_reach_context();
        let actions = fold_actions();
        let normal = select_discard_action(&ctx, &actions).unwrap();

        let diagnostic = diagnose_matching_act(&ctx, &actions);

        assert_eq!(
            diagnostic.selected_source,
            AgentActionSource::DefenseFallback(DefenseFallbackKind::Genbutsu)
        );
        assert_eq!(
            diagnostic.defense_fallback_kind(),
            Some(DefenseFallbackKind::Genbutsu)
        );
        assert_eq!(diagnostic.selected_action, dahai(89));
        // 通常打牌とは異なる action を選んでいる。
        assert_eq!(diagnostic.normal_discard_action, Some(normal.clone()));
        assert_ne!(diagnostic.normal_discard_action, Some(dahai(89)));

        let defense = diagnostic.defense.as_ref().unwrap();
        let selected = defense.selected.as_ref().unwrap();
        assert_eq!(selected.selected_kind, DefenseFallbackKind::Genbutsu);
        assert_eq!(selected.selected_action, "5s".to_string());
        assert!(selected.selected_genbutsu_for_all);

        // 候補診断は合法 Dahai 全件を持ち、選択された候補が最終 action と一致する。
        assert_eq!(defense.candidates.len(), actions.len());
        let selected_candidates: Vec<_> = defense
            .candidates
            .iter()
            .filter(|candidate| candidate.selected)
            .collect();
        assert_eq!(selected_candidates.len(), 1);
        assert_eq!(selected_candidates[0].action, diagnostic.selected_action);
        assert!(selected_candidates[0].genbutsu_for_all);
    }

    #[test]
    fn diagnose_reports_honor_safety_defense_candidates() {
        // 共通現物なし。東は2枚見え、南は0枚見え。より安全な東を切る。
        let ctx = opponent_reach_context_with_visible(Some(112), &[], &[108, 109]);
        let actions = vec![dahai(112), dahai(108)];

        let diagnostic = diagnose_matching_act(&ctx, &actions);

        assert_eq!(diagnostic.selected_action, dahai(108));
        assert_eq!(
            diagnostic.defense_fallback_kind(),
            Some(DefenseFallbackKind::HonorSafety(
                HonorSafetyRank::TwoVisible
            ))
        );

        let defense = diagnostic.defense.as_ref().unwrap();
        assert_eq!(
            defense
                .selected
                .as_ref()
                .unwrap()
                .selected_honor_safety_rank,
            Some(HonorSafetyRank::TwoVisible)
        );

        let south = &defense.candidates[0];
        let east = &defense.candidates[1];
        assert_eq!(south.tile, tile(112).tile_type());
        assert_eq!(south.honor_safety_rank, Some(HonorSafetyRank::NoVisible));
        assert!(!south.genbutsu_for_all);
        assert_eq!(south.wall_rank, None);
        assert_eq!(east.tile, tile(108).tile_type());
        assert_eq!(east.honor_safety_rank, Some(HonorSafetyRank::TwoVisible));
        assert!(east.selected);
    }

    #[test]
    fn diagnose_reports_suited_safety_defense_candidates() {
        use crate::defense::{SuitedSafetyRank, WallRank};

        // 共通現物も字牌もなし。4m を4枚見せて 2m を NoChance にする。
        let ctx = suited_reach_context(Some(0), &[], &[12, 13, 14, 15], &[]);
        let actions = vec![dahai(0), dahai(4)];

        let diagnostic = diagnose_matching_act(&ctx, &actions);

        assert_eq!(diagnostic.selected_action, dahai(4));
        assert_eq!(
            diagnostic.defense_fallback_kind(),
            Some(DefenseFallbackKind::SuitedSafety(
                SuitedSafetyRank::NoChance
            ))
        );

        let defense = diagnostic.defense.as_ref().unwrap();
        let one_man = &defense.candidates[0];
        let two_man = &defense.candidates[1];

        assert_eq!(one_man.tile, tile(0).tile_type());
        assert_eq!(one_man.wall_rank, Some(WallRank::NoWall));
        assert_eq!(one_man.suji_for_all_reached, Some(false));
        assert_eq!(one_man.suited_safety_rank, Some(SuitedSafetyRank::NoSafety));
        assert!(!one_man.selected);

        assert_eq!(two_man.tile, tile(4).tile_type());
        assert_eq!(two_man.wall_rank, Some(WallRank::NoChance));
        assert_eq!(two_man.suji_for_all_reached, Some(false));
        assert_eq!(two_man.suited_safety_rank, Some(SuitedSafetyRank::NoChance));
        assert!(two_man.selected);
    }

    #[test]
    fn diagnose_keeps_defense_candidates_when_fallback_is_not_adopted() {
        // Fold だが防御候補が無い局面。防御を検討した記録として候補評価だけ残る。
        let ctx = suited_reach_context(Some(0), &[], &[], &[]);
        let actions = vec![LegalAction::Reach, dahai(0), dahai(4)];

        let diagnostic = diagnose_matching_act(&ctx, &actions);

        assert_eq!(diagnostic.selected_source, AgentActionSource::NormalDiscard);
        assert_eq!(diagnostic.defense_fallback_kind(), None);

        let defense = diagnostic.defense.as_ref().unwrap();
        assert_eq!(defense.selected, None);
        assert_eq!(defense.selected_kind(), None);
        assert_eq!(defense.candidates.len(), 2);
        assert!(
            defense
                .candidates
                .iter()
                .all(|candidate| !candidate.selected)
        );
    }

    #[test]
    fn diagnose_reports_legal_dahai_fallback_source() {
        // 手牌情報が無く通常打牌も防御 fallback も選べない局面。合法 Dahai へ落ちる。
        let ctx = GameContext::default();
        let actions = vec![dahai(16), dahai(17)];

        let diagnostic = diagnose_matching_act(&ctx, &actions);

        assert_eq!(diagnostic.selected_action, dahai(17));
        assert_eq!(
            diagnostic.selected_source,
            AgentActionSource::LegalDahaiFallback
        );
        assert_eq!(diagnostic.normal_discard_action, None);
        assert!(diagnostic.push_pull_inputs.is_some());
    }

    #[test]
    fn diagnose_reports_none_source_for_empty_actions() {
        let ctx = GameContext::default();
        let diagnostic = diagnose_matching_act(&ctx, &[]);

        assert_eq!(diagnostic.selected_action, LegalAction::None);
        assert_eq!(diagnostic.selected_source, AgentActionSource::None);
        assert_eq!(diagnostic.normal_discard_action, None);
        // 通常打牌評価は実行したが合法候補が無いので、候補は空。
        let normal_discard = diagnostic.normal_discard.as_ref().unwrap();
        assert_eq!(normal_discard.selected, None);
        assert!(normal_discard.candidates.is_empty());
    }

    fn pon_meld() -> crate::meld::Meld {
        crate::meld::Meld::new(
            crate::meld::MeldKind::Pon,
            vec![tile(108), tile(109), tile(110)],
            Some(tile(108)),
        )
    }

    fn context_with_own_melds(
        player_id: Option<u8>,
        hand_values: &[u8],
        drawn_tile: Option<u8>,
        own_melds: Vec<crate::meld::Meld>,
    ) -> GameContext {
        let mut melds: [Vec<crate::meld::Meld>; 4] = Default::default();
        if let Some(player_id) = player_id {
            melds[usize::from(player_id)] = own_melds;
        }
        GameContext::from_parts_with_melds(
            drawn_tile.map(tile),
            hand_values.iter().map(|&value| tile(value)).collect(),
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

    #[test]
    fn diagnose_reports_own_fixed_meld_count() {
        let hand_values = [0, 4, 8, 12, 17, 20, 24, 28, 32, 36];
        let ctx = context_with_own_melds(Some(0), &hand_values, Some(40), vec![pon_meld()]);
        let actions: Vec<LegalAction> = hand_values
            .iter()
            .map(|&value| dahai(value))
            .chain([dahai(40)])
            .collect();
        let diagnostic = diagnose_matching_act(&ctx, &actions);

        assert_eq!(
            diagnostic.own_fixed_meld_count.map(FixedMeldCount::get),
            Some(1)
        );
    }

    #[test]
    fn diagnose_reports_no_own_fixed_meld_count_without_player_id() {
        let ctx = GameContext::default();
        let diagnostic = diagnose_matching_act(&ctx, &[]);
        assert_eq!(diagnostic.own_fixed_meld_count, None);
    }

    // 123456789m 123p 5s + ツモ N。他家 (player 1) だけが副露しており、リーチ者はいない。
    const OPPONENT_MELD_HAND: [u8; 13] = [0, 4, 8, 12, 17, 20, 24, 28, 32, 36, 40, 44, 89];
    const OPPONENT_MELD_DRAW: u8 = 120;

    fn red_five_chi() -> crate::meld::Meld {
        crate::meld::Meld::new(
            crate::meld::MeldKind::Chi,
            vec![tile(13), tile(16), tile(21)],
            Some(tile(13)),
        )
    }

    fn opponent_meld_context(
        player_id: Option<u8>,
        opponent_melds: Vec<crate::meld::Meld>,
    ) -> GameContext {
        opponent_meld_context_with_reach(player_id, opponent_melds, [false; 4])
    }

    fn opponent_meld_context_with_reach(
        player_id: Option<u8>,
        opponent_melds: Vec<crate::meld::Meld>,
        reached: [bool; 4],
    ) -> GameContext {
        let mut melds: [Vec<crate::meld::Meld>; 4] = Default::default();
        melds[1] = opponent_melds;

        GameContext::from_parts_with_melds(
            Some(tile(OPPONENT_MELD_DRAW)),
            OPPONENT_MELD_HAND
                .iter()
                .map(|&value| tile(value))
                .collect(),
            vec![],
            TileType::new(27),
            TileType::new(27),
            Vec::new(),
            player_id,
            Some(0),
            Default::default(),
            reached,
            melds,
        )
    }

    fn opponent_meld_actions() -> Vec<LegalAction> {
        OPPONENT_MELD_HAND
            .iter()
            .map(|&value| dahai(value))
            .chain([dahai(OPPONENT_MELD_DRAW)])
            .collect()
    }

    #[test]
    fn diagnose_reports_player_threats_for_every_seat() {
        let ctx = opponent_meld_context(Some(0), vec![white_dragon_pon(), red_five_chi()]);
        let diagnostic = diagnose_matching_act(&ctx, &opponent_meld_actions());

        // 表示・解析用に別実装で数え直さず、production の診断値をそのまま持つ。
        assert_eq!(diagnostic.player_threats, diagnose_player_threats(&ctx));

        let threat = &diagnostic.player_threats[1];
        assert_eq!(threat.player, 1);
        assert_eq!(threat.is_opponent(), Some(true));
        assert!(!threat.reached);
        assert_eq!(threat.is_dealer, Some(false));
        assert_eq!(threat.meld_count, 2);
        assert_eq!(threat.open_meld_count, 2);
        assert_eq!(threat.kan_count, 0);
        assert!(threat.melds[0].value_honor.unwrap().is_dragon);
        assert_eq!(threat.melds[1].red_dora_count, 1);

        assert_eq!(diagnostic.player_threats[0].is_self, Some(true));
        assert_eq!(diagnostic.player_threats[0].meld_count, 0);
    }

    #[test]
    fn diagnose_does_not_guess_the_self_seat_without_player_id() {
        let ctx = opponent_meld_context(None, vec![white_dragon_pon()]);
        let diagnostic = diagnose_matching_act(&ctx, &opponent_meld_actions());

        assert_eq!(diagnostic.player_threats.len(), 4);
        for (player, threat) in diagnostic.player_threats.iter().enumerate() {
            assert_eq!(threat.player, player);
            assert_eq!(threat.is_self, None);
            assert_eq!(threat.is_opponent(), None);
        }
        assert_eq!(diagnostic.player_threats[1].meld_count, 1);
    }

    #[test]
    fn opponent_melds_do_not_change_the_existing_decisions() {
        // 非リーチの副露相手は脅威入力に入らないので、既存の選択・押し引き・防御は変わらない。
        let actions = opponent_meld_actions();
        let with_melds = opponent_meld_context(Some(0), vec![white_dragon_pon(), red_five_chi()]);
        let without_melds = opponent_meld_context(Some(0), vec![]);

        let melded = diagnose_matching_act(&with_melds, &actions);
        let plain = diagnose_matching_act(&without_melds, &actions);

        assert_eq!(melded.selected_action, plain.selected_action);
        assert_eq!(melded.selected_source, plain.selected_source);
        assert_eq!(melded.normal_discard_action, plain.normal_discard_action);
        assert_eq!(melded.push_pull_inputs, plain.push_pull_inputs);
        assert_eq!(melded.push_pull_decision, plain.push_pull_decision);
        assert_eq!(melded.reach, plain.reach);
        assert_eq!(melded.defense, plain.defense);
        assert_eq!(melded.pon, plain.pon);
        assert_ne!(melded.player_threats, plain.player_threats);

        let decision = melded.push_pull_decision.expect("押し引きを判定している");
        assert_eq!(decision.mode, PushPullMode::Push);
        assert_eq!(
            decision.reason,
            crate::push_pull::PushPullReason::NoOpponentReach
        );
        assert_eq!(
            melded
                .push_pull_inputs
                .expect("押し引き入力がある")
                .opponent_reach_count,
            0
        );
    }

    #[test]
    fn player_threats_keep_act_and_diagnose_consistent() {
        let ctx = opponent_meld_context(Some(0), vec![white_dragon_pon(), red_five_chi()]);
        let actions = opponent_meld_actions();

        let mut agent = ShantenAgent;
        let acted = agent.act(&ctx, &actions);
        let diagnosed = ShantenAgent::diagnose(&ctx, &actions);
        let with_lookahead =
            ShantenAgent::diagnose_with_options(&ctx, &actions, DiagnosticOptions::WITH_LOOKAHEAD);

        assert_eq!(diagnosed.selected_action, acted);
        assert_eq!(with_lookahead.selected_action, acted);
        assert_eq!(with_lookahead.player_threats, diagnosed.player_threats);
    }

    #[test]
    fn player_threats_keep_reach_and_meld_facts_together() {
        let ctx = opponent_meld_context_with_reach(
            Some(0),
            vec![white_dragon_pon()],
            [false, true, false, false],
        );
        let diagnostic = diagnose_matching_act(&ctx, &opponent_meld_actions());

        let threat = &diagnostic.player_threats[1];
        assert!(threat.reached);
        assert_eq!(threat.meld_count, 1);
        assert_eq!(threat.open_meld_count, 1);
        assert_eq!(
            diagnostic
                .push_pull_inputs
                .expect("押し引き入力がある")
                .opponent_reach_count,
            1
        );
    }

    // 白ポン1組。副露の種類によらず完成済み面子1として数える。
    fn white_dragon_pon() -> crate::meld::Meld {
        crate::meld::Meld::new(
            crate::meld::MeldKind::Pon,
            vec![tile(124), tile(125), tile(126)],
            Some(tile(124)),
        )
    }

    #[test]
    fn act_uses_the_fixed_meld_aware_normal_discard() {
        // 白ポン1組 + 123456m 78p 55s + ツモ N。N を切ると副露込みの通常形テンパイ (待ち 6p / 9p)。
        let hand_values = [0u8, 4, 8, 12, 17, 20, 60, 64, 89, 90];
        let ctx =
            context_with_own_melds(Some(0), &hand_values, Some(120), vec![white_dragon_pon()]);
        let actions: Vec<LegalAction> = hand_values
            .iter()
            .map(|&value| dahai(value))
            .chain([dahai(120)])
            .collect();

        let mut agent = ShantenAgent;
        assert_eq!(agent.act(&ctx, &actions), dahai(120));

        let diagnostic = diagnose_matching_act(&ctx, &actions);
        assert_eq!(diagnostic.selected_source, AgentActionSource::NormalDiscard);
        assert_eq!(diagnostic.normal_discard_action, Some(dahai(120)));
        assert_eq!(
            diagnostic.own_fixed_meld_count.map(FixedMeldCount::get),
            Some(1)
        );

        let selected = diagnostic
            .normal_discard
            .as_ref()
            .unwrap()
            .selected
            .as_ref()
            .unwrap();
        assert_eq!(selected.min_shanten_after_discard(), 0);
        assert_eq!(selected.shanten_after_discard.standard(), 0);
        assert_eq!(selected.acceptance_total_remaining(), 8);
        let acceptance: Vec<String> = selected
            .acceptance_after_discard
            .tiles
            .iter()
            .map(|entry| entry.tile.to_mjai_string())
            .collect();
        assert_eq!(acceptance, vec!["6p".to_string(), "9p".to_string()]);

        // 同じ評価が押し引き入力へ共有される。
        let offense = diagnostic.push_pull_inputs.unwrap().offense.unwrap();
        assert_eq!(offense.min_shanten_after_discard, 0);
        assert_eq!(offense.acceptance_total_remaining, 8);
    }

    #[test]
    fn act_without_own_melds_keeps_the_concealed_evaluation() {
        // 同じ手牌でも副露が無ければ従来どおり二向聴のまま評価する。
        let hand_values = [0u8, 4, 8, 12, 17, 20, 60, 64, 89, 90];
        let ctx = context_with_own_melds(Some(0), &hand_values, Some(120), vec![]);
        let actions: Vec<LegalAction> = hand_values
            .iter()
            .map(|&value| dahai(value))
            .chain([dahai(120)])
            .collect();

        let diagnostic = diagnose_matching_act(&ctx, &actions);
        let selected = diagnostic
            .normal_discard
            .as_ref()
            .unwrap()
            .selected
            .as_ref()
            .unwrap();
        assert_eq!(selected.min_shanten_after_discard(), 2);
        assert!(selected.shanten_after_discard.concealed().is_some());
    }

    #[test]
    fn melds_do_not_change_the_selected_action() {
        let hand_values = [0, 4, 8, 12, 17, 20, 24, 28, 32, 36];
        let actions: Vec<LegalAction> = hand_values
            .iter()
            .map(|&value| dahai(value))
            .chain([dahai(40)])
            .collect();
        let without_melds = context_with_own_melds(Some(0), &hand_values, Some(40), vec![]);
        let with_melds = context_with_own_melds(Some(0), &hand_values, Some(40), vec![pon_meld()]);

        let mut agent = ShantenAgent;
        assert_eq!(
            agent.act(&with_melds, &actions),
            agent.act(&without_melds, &actions)
        );
        assert_eq!(
            ShantenAgent::diagnose(&with_melds, &actions).selected_action,
            ShantenAgent::diagnose(&without_melds, &actions).selected_action
        );
    }

    #[test]
    fn diagnose_reports_none_source_without_dahai_actions() {
        let ctx = GameContext::default();
        let actions = vec![
            LegalAction::Pon {
                tile: tile(108),
                consumed: vec![tile(109), tile(110)],
            },
            LegalAction::None,
        ];
        let diagnostic = diagnose_matching_act(&ctx, &actions);

        assert_eq!(diagnostic.selected_action, LegalAction::None);
        assert_eq!(diagnostic.selected_source, AgentActionSource::None);
    }

    // ---- 診断収集が action 選択へ影響しないことの確認 ----

    #[test]
    fn act_path_does_not_build_analysis_diagnostics() {
        // 通常の act() 経路では、解析専用の追加診断(候補ごとの形の内訳・全防御候補評価)を作らない。
        let ctx = fold_under_reach_context();
        let actions = fold_actions();

        let mut diagnostics = DecisionDiagnostics::disabled();
        let decision = ShantenAgent.decide_with_diagnostics(&ctx, &actions, &mut diagnostics);

        assert_eq!(decision.action, dahai(89));
        assert!(diagnostics.normal_discard.is_none());
        assert!(diagnostics.defense.is_none());
    }

    #[test]
    fn enabling_diagnostics_does_not_change_decision() {
        let cases: Vec<(GameContext, Vec<LegalAction>)> = vec![
            (fold_under_reach_context(), fold_actions()),
            (tenpai_context(&[]), tenpai_actions()),
            (GameContext::default(), vec![dahai(16), dahai(17)]),
            (GameContext::default(), vec![]),
            (
                opponent_reach_context(Some(0), &[]),
                vec![dahai(16), LegalAction::Hora],
            ),
            (
                suited_reach_context(Some(0), &[], &[12, 13, 14, 15], &[]),
                vec![dahai(0), dahai(4)],
            ),
            (
                dragon_pon_reaction().context(),
                dragon_pon_reaction().actions(),
            ),
        ];

        for (ctx, actions) in cases {
            let agent = ShantenAgent;
            let production = agent.decide(&ctx, &actions);
            let with_diagnostics =
                agent.decide_with_diagnostics(&ctx, &actions, &mut DecisionDiagnostics::enabled());
            assert_eq!(production, with_diagnostics);
        }
    }

    // ---- 2手先診断 (DiagnosticOptions::WITH_LOOKAHEAD) テスト ----

    // 2手先診断テスト用の小さい局面。2手先は「打牌候補 × 受け入れ牌 × 次打牌候補」の探索に
    // なるため、診断の構造と選択への非干渉を確認するのに十分な最小の手牌で回す。
    fn lookahead_context() -> GameContext {
        let hand: Vec<_> = [0u8, 4, 36, 40, 89].iter().map(|&v| tile(v)).collect();
        let drawn = tile(90);
        let mut visible = hand.clone();
        visible.push(drawn);
        visible.push(tile(1));
        GameContext::from_parts_with_visible_tiles(Some(drawn), hand, vec![], None, None, visible)
    }

    fn lookahead_actions() -> Vec<LegalAction> {
        [0u8, 4, 36, 40, 89, 90].iter().map(|&v| dahai(v)).collect()
    }

    #[test]
    fn act_path_does_not_build_lookahead() {
        // 通常の act() 経路では2手先診断を構築しない。
        let ctx = lookahead_context();
        let actions = lookahead_actions();

        let mut diagnostics = DecisionDiagnostics::disabled();
        let _ = ShantenAgent.decide_with_diagnostics(&ctx, &actions, &mut diagnostics);

        assert!(diagnostics.normal_discard_lookahead.is_none());
    }

    #[test]
    fn diagnose_does_not_build_lookahead_by_default() {
        // 既定の診断でも2手先は構築しない。構築するのは明示的に要求した場合だけ。
        let ctx = lookahead_context();
        let actions = lookahead_actions();

        assert!(
            ShantenAgent::diagnose(&ctx, &actions)
                .normal_discard_lookahead
                .is_none()
        );
        assert!(
            ShantenAgent::diagnose_with_options(&ctx, &actions, DiagnosticOptions::NONE)
                .normal_discard_lookahead
                .is_none()
        );
    }

    #[test]
    fn lookahead_does_not_change_the_selected_action() {
        let ctx = lookahead_context();
        let actions = lookahead_actions();

        let mut agent = ShantenAgent;
        let expected = agent.act(&ctx, &actions);
        let without = ShantenAgent::diagnose(&ctx, &actions);
        let with =
            ShantenAgent::diagnose_with_options(&ctx, &actions, DiagnosticOptions::WITH_LOOKAHEAD);

        assert_eq!(with.selected_action, expected);
        assert!(
            with.normal_discard_lookahead.is_some(),
            "2手先診断が構築されていない"
        );
        // 2手先以外の診断はすべて既定の診断と一致する。
        assert_eq!(
            ShantenDecisionDiagnostic {
                normal_discard_lookahead: None,
                ..with
            },
            without
        );
    }

    #[test]
    fn lookahead_covers_every_normal_discard_candidate() {
        let ctx = lookahead_context();
        let actions = lookahead_actions();
        let diagnostic =
            ShantenAgent::diagnose_with_options(&ctx, &actions, DiagnosticOptions::WITH_LOOKAHEAD);

        let normal_discard = diagnostic.normal_discard.expect("normal discard evaluated");
        let lookahead = diagnostic
            .normal_discard_lookahead
            .expect("lookahead built");

        assert!(normal_discard.candidates.len() > 1);
        assert_eq!(lookahead.candidates.len(), normal_discard.candidates.len());
        for (candidate_lookahead, candidate) in lookahead
            .candidates
            .iter()
            .zip(normal_discard.candidates.iter())
        {
            assert_eq!(candidate_lookahead.discard, candidate.evaluation.discard);
            // 現在打牌後の受け入れをそのまま引き継ぐので、対象牌と残枚数が一致する。
            let acceptance = &candidate.evaluation.acceptance_after_discard.tiles;
            assert_eq!(candidate_lookahead.draws.len(), acceptance.len());
            for (draw, accepted) in candidate_lookahead.draws.iter().zip(acceptance.iter()) {
                assert_eq!(draw.draw, accepted.tile);
                assert_eq!(draw.remaining, accepted.remaining);
            }
        }
    }

    #[test]
    fn lookahead_free_function_matches_associated_function() {
        let ctx = lookahead_context();
        let actions = lookahead_actions();
        assert_eq!(
            diagnose_shanten_decision_with_options(
                &ctx,
                &actions,
                DiagnosticOptions::WITH_LOOKAHEAD
            ),
            ShantenAgent::diagnose_with_options(&ctx, &actions, DiagnosticOptions::WITH_LOOKAHEAD)
        );
    }

    // ---- 1向聴の weighted tenpai wait ----

    // 12m 68m 444p 5p 789p 567s の門前14枚 (打牌選択側と同じ fixture)。
    //
    // 打 5p は受け入れが最も広く、打 1m は 45p の両面を残してテンパイ後の待ちが広くなる。
    // 合法 Dahai をこの2候補だけに絞り、新しい比較軸で選択が決まる局面にする。
    use crate::discard_selection::tests::iishanten_wait_context;

    fn iishanten_wait_actions() -> Vec<LegalAction> {
        vec![dahai(0), dahai(53)]
    }

    #[test]
    fn weighted_tenpai_wait_keeps_act_and_diagnose_consistent() {
        // act() / diagnose() / diagnose_with_options(WITH_LOOKAHEAD) の選択が一致する。
        let ctx = iishanten_wait_context();
        let actions = iishanten_wait_actions();

        let mut agent = ShantenAgent;
        let acted = agent.act(&ctx, &actions);
        let diagnosed = ShantenAgent::diagnose(&ctx, &actions);
        let with_lookahead =
            ShantenAgent::diagnose_with_options(&ctx, &actions, DiagnosticOptions::WITH_LOOKAHEAD);

        assert_eq!(diagnosed.selected_action, acted);
        assert_eq!(with_lookahead.selected_action, acted);
        assert_eq!(with_lookahead.normal_discard, diagnosed.normal_discard);

        // 新しい比較軸で選択が決まっている局面であることを固定する。
        let normal_discard = diagnosed.normal_discard.as_ref().expect("evaluated");
        let runner_up = normal_discard
            .candidates
            .iter()
            .find(|candidate| !candidate.selected)
            .expect("runner-up exists");
        assert_eq!(
            runner_up.comparison_reason,
            bot_logic::DiscardComparisonReason::WeightedTenpaiWaitRemaining
        );
        assert_eq!(acted, dahai(0));
    }

    #[test]
    fn push_pull_shares_the_selected_normal_discard() {
        // 押し引きへ渡る攻撃評価は、weighted tenpai wait で選ばれた通常打牌評価そのもの。
        let ctx = iishanten_wait_context();
        let actions = iishanten_wait_actions();

        let diagnostic = ShantenAgent::diagnose(&ctx, &actions);
        let selected = diagnostic
            .normal_discard
            .as_ref()
            .and_then(|normal_discard| normal_discard.selected.as_ref())
            .expect("selected evaluation exists");
        let offense = diagnostic
            .push_pull_inputs
            .as_ref()
            .and_then(|inputs| inputs.offense.as_ref())
            .expect("offense state exists");

        assert_eq!(
            offense.min_shanten_after_discard,
            selected.min_shanten_after_discard()
        );
        assert_eq!(
            offense.acceptance_total_remaining,
            selected.acceptance_total_remaining()
        );
        assert_eq!(
            offense.acceptance_type_count,
            selected.acceptance_type_count()
        );

        // 受け入れの多い runner-up (1手評価だけなら選ばれる候補) の評価は渡っていない。
        let runner_up = diagnostic
            .normal_discard
            .as_ref()
            .expect("evaluated")
            .candidates
            .iter()
            .find(|candidate| !candidate.selected)
            .expect("runner-up exists");
        assert!(
            runner_up.evaluation.acceptance_total_remaining() > offense.acceptance_total_remaining
        );
    }

    // ---- 限定 Pon (AgentActionSource::Pon) テスト ----

    // 他家(player 1)が捨てた牌への Pon reaction 局面を組み立てる。
    // 既定は東場東家・リーチ者なし・副露なし・ツモ牌なしで、検証したい条件だけ差し替える。
    #[derive(Debug, Clone)]
    pub(crate) struct PonReaction {
        hand: Vec<u8>,
        target: u8,
        consumed: Vec<u8>,
        round_wind: Option<u8>,
        seat_wind: Option<u8>,
        reached: [bool; 4],
        extra_visible: Vec<u8>,
        own_melds: Vec<crate::meld::Meld>,
        drawn_tile: Option<u8>,
        player_id: Option<u8>,
    }

    impl PonReaction {
        pub(crate) fn new(hand: &[u8], target: u8, consumed: &[u8]) -> Self {
            Self {
                hand: hand.to_vec(),
                target,
                consumed: consumed.to_vec(),
                round_wind: Some(27),
                seat_wind: Some(27),
                reached: [false; 4],
                extra_visible: Vec::new(),
                own_melds: Vec::new(),
                drawn_tile: None,
                player_id: Some(0),
            }
        }

        fn with_winds(mut self, round_wind: Option<u8>, seat_wind: Option<u8>) -> Self {
            self.round_wind = round_wind;
            self.seat_wind = seat_wind;
            self
        }

        fn with_reached(mut self, reached: [bool; 4]) -> Self {
            self.reached = reached;
            self
        }

        fn with_extra_visible(mut self, extra_visible: &[u8]) -> Self {
            self.extra_visible = extra_visible.to_vec();
            self
        }

        fn with_own_melds(mut self, own_melds: Vec<crate::meld::Meld>) -> Self {
            self.own_melds = own_melds;
            self
        }

        fn with_drawn_tile(mut self, drawn_tile: u8) -> Self {
            self.drawn_tile = Some(drawn_tile);
            self
        }

        fn without_player_id(mut self) -> Self {
            self.player_id = None;
            self
        }

        fn with_consumed(mut self, consumed: &[u8]) -> Self {
            self.consumed = consumed.to_vec();
            self
        }

        pub(crate) fn context(&self) -> GameContext {
            let hand: Vec<TileId> = self.hand.iter().map(|&value| tile(value)).collect();
            let discards = [vec![], vec![tile(self.target)], vec![], vec![]];

            let mut visible = hand.clone();
            visible.extend(self.drawn_tile.map(tile));
            visible.push(tile(self.target));
            visible.extend(self.own_melds.iter().flat_map(|meld| meld.tiles().to_vec()));
            visible.extend(self.extra_visible.iter().map(|&value| tile(value)));

            let mut melds: [Vec<crate::meld::Meld>; 4] = Default::default();
            melds[0] = self.own_melds.clone();

            GameContext::from_parts_with_melds(
                self.drawn_tile.map(tile),
                hand,
                vec![],
                self.round_wind.and_then(TileType::new),
                self.seat_wind.and_then(TileType::new),
                visible,
                self.player_id,
                Some(0),
                discards,
                self.reached,
                melds,
            )
        }

        pub(crate) fn pon(&self) -> LegalAction {
            LegalAction::Pon {
                tile: tile(self.target),
                consumed: self.consumed.iter().map(|&value| tile(value)).collect(),
            }
        }

        pub(crate) fn actions(&self) -> Vec<LegalAction> {
            vec![self.pon(), LegalAction::None]
        }
    }

    fn honor_pon_meld(first: u8) -> crate::meld::Meld {
        crate::meld::Meld::new(
            crate::meld::MeldKind::Pon,
            vec![tile(first), tile(first + 1), tile(first + 2)],
            Some(tile(first)),
        )
    }

    // 123456m 55p 78s N PP。P(白) の対子を持つ一向聴で、PP を Pon して N を切るとテンパイ。
    const PON_HAND: [u8; 13] = [0, 4, 8, 12, 17, 20, 53, 54, 96, 100, 120, 124, 125];
    const PON_TARGET: u8 = 126;
    const PON_CONSUMED: [u8; 2] = [124, 125];

    fn dragon_pon_reaction() -> PonReaction {
        PonReaction::new(&PON_HAND, PON_TARGET, &PON_CONSUMED)
    }

    // 診断の Pon candidate は1件で、その理由と最終 action が期待どおりであることを確認する。
    fn assert_single_pon_candidate(
        reaction: &PonReaction,
        expected_action: &LegalAction,
        expected_reason: PonDecisionReason,
    ) -> PonCandidateDiagnostic {
        let ctx = reaction.context();
        let actions = reaction.actions();
        let diagnostic = diagnose_matching_act(&ctx, &actions);

        assert_eq!(&diagnostic.selected_action, expected_action);

        let pon = diagnostic.pon.as_ref().unwrap();
        assert_eq!(pon.reason, expected_reason);
        assert_eq!(pon.candidates.len(), 1);

        let candidate = pon.candidates[0].clone();
        assert_eq!(candidate.reason, expected_reason);
        assert_eq!(
            candidate.eligible,
            expected_reason == PonDecisionReason::EligibleTenpai
        );
        assert_eq!(candidate.selected, candidate.eligible);
        assert_eq!(
            pon.selected.as_ref(),
            candidate.eligible.then_some(&candidate.action)
        );
        candidate
    }

    fn assert_pon_is_declined(reaction: &PonReaction, expected_reason: PonDecisionReason) {
        let candidate = assert_single_pon_candidate(reaction, &LegalAction::None, expected_reason);
        assert!(!candidate.eligible);
    }

    #[test]
    fn pons_value_honor_pair_that_reaches_a_live_tenpai() {
        let reaction = dragon_pon_reaction();
        let candidate = assert_single_pon_candidate(
            &reaction,
            &reaction.pon(),
            PonDecisionReason::EligibleTenpai,
        );

        assert_eq!(candidate.target, tile(PON_TARGET).tile_type());
        assert!(candidate.value_honor);
        assert_eq!(candidate.current_shanten, Some(1));
        assert_eq!(
            candidate.current_fixed_meld_count.map(FixedMeldCount::get),
            Some(0)
        );
        assert_eq!(
            candidate.post_pon_fixed_meld_count.map(FixedMeldCount::get),
            Some(1)
        );
        assert_eq!(candidate.post_pon_shanten(), Some(0));
        assert_eq!(candidate.post_pon_acceptance_total_remaining(), Some(8));
        assert_eq!(candidate.post_pon_acceptance_type_count(), Some(2));

        let evaluation = candidate.post_pon_discard.as_ref().unwrap();
        assert_eq!(evaluation.discard.to_mjai_string(), "N");
        let acceptance: Vec<(String, u8)> = evaluation
            .acceptance_after_discard
            .tiles
            .iter()
            .map(|entry| (entry.tile.to_mjai_string(), entry.remaining))
            .collect();
        assert_eq!(
            acceptance,
            vec![("6s".to_string(), 4), ("9s".to_string(), 4)]
        );
    }

    #[test]
    fn pon_source_is_reported_for_the_selected_pon() {
        let reaction = dragon_pon_reaction();
        let ctx = reaction.context();
        let actions = reaction.actions();

        let mut agent = ShantenAgent;
        assert_eq!(agent.act(&ctx, &actions), reaction.pon());

        let decision = ShantenAgent.decide(&ctx, &actions);
        assert_eq!(decision.source, AgentActionSource::Pon);
        // Pon は通常打牌・押し引き・防御より前で確定するので、それらは評価していない。
        assert_eq!(decision.push_pull, None);
        assert_eq!(decision.push_pull_inputs, None);
        assert_eq!(decision.normal_discard, None);

        let diagnostic = ShantenAgent::diagnose(&ctx, &actions);
        assert_eq!(diagnostic.selected_source, AgentActionSource::Pon);
        assert_eq!(diagnostic.normal_discard, None);
        assert_eq!(diagnostic.push_pull_decision, None);
        assert_eq!(diagnostic.defense, None);
    }

    #[test]
    fn post_pon_evaluation_matches_the_shared_discard_helper() {
        // 診断が持つ Pon 後の打牌評価は、本番の打牌評価 helper の結果そのものである。
        let reaction = dragon_pon_reaction();
        let ctx = reaction.context();
        let candidate = assert_single_pon_candidate(
            &reaction,
            &reaction.pon(),
            PonDecisionReason::EligibleTenpai,
        );

        let post_pon_tiles: Vec<TileId> = PON_HAND
            .iter()
            .filter(|value| !PON_CONSUMED.contains(value))
            .map(|&value| tile(value))
            .collect();
        let expected = select_best_one_step_discard_evaluation_with_fixed_meld_count(
            &ctx,
            &post_pon_tiles,
            FixedMeldCount::new(1).unwrap(),
        );

        assert_eq!(candidate.post_pon_discard, expected);
        assert!(expected.is_some());
    }

    #[test]
    fn pons_round_wind_pair() {
        // 東場・南家。場風の東は既存 helper で役牌と判定される。
        let reaction = PonReaction::new(
            &[0, 4, 8, 12, 17, 20, 53, 54, 96, 100, 120, 108, 109],
            110,
            &[108, 109],
        )
        .with_winds(Some(27), Some(28));

        assert!(
            tile(110)
                .tile_type()
                .is_value_honor(TileType::new(27), TileType::new(28))
        );

        let candidate = assert_single_pon_candidate(
            &reaction,
            &reaction.pon(),
            PonDecisionReason::EligibleTenpai,
        );
        assert!(candidate.value_honor);
        assert_eq!(candidate.current_shanten, Some(1));
        assert_eq!(candidate.post_pon_shanten(), Some(0));
        assert_eq!(candidate.post_pon_acceptance_total_remaining(), Some(8));
    }

    #[test]
    fn does_not_pon_guest_wind_pair() {
        // 東場・南家の西は場風でも自風でもないので役なしテンパイを作らない。
        let reaction = PonReaction::new(
            &[0, 4, 8, 12, 17, 20, 53, 54, 96, 100, 120, 116, 117],
            118,
            &[116, 117],
        )
        .with_winds(Some(27), Some(28));

        assert!(
            !tile(118)
                .tile_type()
                .is_value_honor(TileType::new(27), TileType::new(28))
        );

        assert_pon_is_declined(&reaction, PonDecisionReason::NotValueHonor);
    }

    #[test]
    fn does_not_pon_wind_pair_without_wind_information() {
        // 場風・自風が不明な東は役牌と確定できないため、自風だろうと推測しない。
        let reaction = PonReaction::new(
            &[0, 4, 8, 12, 17, 20, 53, 54, 96, 100, 120, 108, 109],
            110,
            &[108, 109],
        )
        .with_winds(None, None);

        assert_pon_is_declined(&reaction, PonDecisionReason::NotValueHonor);
    }

    #[test]
    fn pons_dragon_pair_without_wind_information() {
        // 三元牌は場風・自風に依存しないので、風の情報が無くても役牌として扱う。
        let reaction = dragon_pon_reaction().with_winds(None, None);
        let candidate = assert_single_pon_candidate(
            &reaction,
            &reaction.pon(),
            PonDecisionReason::EligibleTenpai,
        );
        assert!(candidate.value_honor);
    }

    #[test]
    fn does_not_pon_from_two_shanten() {
        // 123456m 55p 1s 9s N PP。役牌の対子でも2向聴からは鳴かない。
        let reaction = PonReaction::new(
            &[0, 4, 8, 12, 17, 20, 53, 54, 72, 104, 120, 124, 125],
            PON_TARGET,
            &PON_CONSUMED,
        );

        let candidate = assert_single_pon_candidate(
            &reaction,
            &LegalAction::None,
            PonDecisionReason::CurrentShantenNotOne,
        );
        assert_eq!(candidate.current_shanten, Some(2));
        // 1向聴判定で落ちるので、Pon 後の打牌評価は行わない。
        assert_eq!(candidate.post_pon_discard, None);
    }

    #[test]
    fn does_not_pon_while_already_tenpai() {
        // 123456789m 55p PP。テンパイ維持の比較は今回の対象外。
        let reaction = PonReaction::new(
            &[0, 4, 8, 12, 17, 20, 24, 28, 32, 53, 54, 124, 125],
            PON_TARGET,
            &PON_CONSUMED,
        );

        let candidate = assert_single_pon_candidate(
            &reaction,
            &LegalAction::None,
            PonDecisionReason::CurrentShantenNotOne,
        );
        assert_eq!(candidate.current_shanten, Some(0));
    }

    #[test]
    fn does_not_pon_when_the_best_post_pon_discard_is_still_one_shanten() {
        // 12345678m 1p 45p PP。Pon してもテンパイにならない一向聴。
        let reaction = PonReaction::new(
            &[0, 4, 8, 12, 17, 20, 24, 28, 36, 48, 53, 124, 125],
            PON_TARGET,
            &PON_CONSUMED,
        );

        let candidate = assert_single_pon_candidate(
            &reaction,
            &LegalAction::None,
            PonDecisionReason::PostPonNotTenpai,
        );
        assert_eq!(candidate.current_shanten, Some(1));
        assert_eq!(candidate.post_pon_shanten(), Some(1));
    }

    #[test]
    fn does_not_pon_without_live_acceptance() {
        // 6s / 9s が場に4枚ずつ見えている枯れ待ち。形の上ではテンパイでも鳴かない。
        let reaction =
            dragon_pon_reaction().with_extra_visible(&[92, 93, 94, 95, 104, 105, 106, 107]);

        let candidate = assert_single_pon_candidate(
            &reaction,
            &LegalAction::None,
            PonDecisionReason::NoLiveAcceptance,
        );
        assert_eq!(candidate.post_pon_shanten(), Some(0));
        assert_eq!(candidate.post_pon_acceptance_total_remaining(), Some(0));
    }

    #[test]
    fn does_not_pon_under_opponent_reach() {
        let reaction = dragon_pon_reaction().with_reached([false, true, false, false]);

        let candidate = assert_single_pon_candidate(
            &reaction,
            &LegalAction::None,
            PonDecisionReason::OpponentReached,
        );
        // リーチ者がいる時点で以降の条件は評価しない。
        assert_eq!(candidate.current_shanten, None);
        assert_eq!(candidate.post_pon_discard, None);
    }

    #[test]
    fn does_not_pon_from_a_triplet() {
        // 123456m 55p 78s PPP。既に暗刻として完成している構造は崩さない。
        let reaction = PonReaction::new(
            &[0, 4, 8, 12, 17, 20, 53, 54, 96, 100, 124, 125, 126],
            127,
            &PON_CONSUMED,
        );

        let candidate = assert_single_pon_candidate(
            &reaction,
            &LegalAction::None,
            PonDecisionReason::TargetCountNotTwo,
        );
        assert!(candidate.value_honor);
    }

    #[test]
    fn does_not_pon_without_a_known_fixed_meld_count() {
        // player_id が無く自分の副露数が確定できない局面。0副露だろうと推測しない。
        let reaction = dragon_pon_reaction().without_player_id();
        assert_eq!(reaction.context().own_fixed_meld_count(), None);

        let candidate = assert_single_pon_candidate(
            &reaction,
            &LegalAction::None,
            PonDecisionReason::FixedMeldCountUnknown,
        );
        assert_eq!(candidate.current_fixed_meld_count, None);
        assert_eq!(candidate.post_pon_fixed_meld_count, None);
    }

    #[test]
    fn pons_with_an_existing_meld() {
        // 東ポン1組 + 123m 55p 78s N PP。副露済み1組から2組へ増やしてテンパイする。
        let reaction = PonReaction::new(
            &[0, 4, 8, 53, 54, 96, 100, 120, 124, 125],
            PON_TARGET,
            &PON_CONSUMED,
        )
        .with_own_melds(vec![pon_meld()]);

        let candidate = assert_single_pon_candidate(
            &reaction,
            &reaction.pon(),
            PonDecisionReason::EligibleTenpai,
        );
        assert_eq!(
            candidate.current_fixed_meld_count.map(FixedMeldCount::get),
            Some(1)
        );
        assert_eq!(
            candidate.post_pon_fixed_meld_count.map(FixedMeldCount::get),
            Some(2)
        );
        assert_eq!(candidate.current_shanten, Some(1));
        assert_eq!(candidate.post_pon_shanten(), Some(0));
        assert_eq!(candidate.post_pon_acceptance_total_remaining(), Some(8));
        assert_eq!(
            candidate
                .post_pon_discard
                .as_ref()
                .unwrap()
                .discard
                .to_mjai_string(),
            "N"
        );
    }

    #[test]
    fn does_not_pon_when_the_post_pon_fixed_meld_count_would_overflow() {
        // 副露済み4組からの Pon は成立しない。silent clamp せず理由として報告する。
        let melds: Vec<crate::meld::Meld> = [108, 112, 116, 120]
            .iter()
            .map(|&first| honor_pon_meld(first))
            .collect();
        let reaction =
            PonReaction::new(&[124, 125], PON_TARGET, &PON_CONSUMED).with_own_melds(melds);
        assert_eq!(
            reaction
                .context()
                .own_fixed_meld_count()
                .map(FixedMeldCount::get),
            Some(4)
        );

        let candidate = assert_single_pon_candidate(
            &reaction,
            &LegalAction::None,
            PonDecisionReason::FixedMeldCountOverflow,
        );
        assert_eq!(
            candidate.current_fixed_meld_count.map(FixedMeldCount::get),
            Some(4)
        );
        assert_eq!(candidate.post_pon_fixed_meld_count, None);
    }

    #[test]
    fn does_not_pon_in_a_reaction_context_that_has_a_drawn_tile() {
        // reaction 局面に drawn_tile がある不整合な context では、14枚扱いで判断しない。
        let reaction = dragon_pon_reaction().with_drawn_tile(132);

        assert_pon_is_declined(&reaction, PonDecisionReason::UnexpectedDrawnTile);
    }

    #[test]
    fn does_not_pon_with_inconsistent_consumed_tiles() {
        for consumed in [
            // 手牌に無い物理牌
            vec![124, 127],
            // 同じ物理牌の重複
            vec![124, 124],
            // 枚数不足
            vec![124],
            // 枚数過多
            vec![124, 125, 126],
            // 対象牌種ではない
            vec![124, 120],
        ] {
            let reaction = dragon_pon_reaction().with_consumed(&consumed);
            let candidate = assert_single_pon_candidate(
                &reaction,
                &LegalAction::None,
                PonDecisionReason::InvalidConsumed,
            );
            assert_eq!(candidate.current_shanten, None, "{consumed:?}");
        }
    }

    #[test]
    fn prefers_hora_over_an_eligible_pon() {
        let mut agent = ShantenAgent;
        let reaction = dragon_pon_reaction();
        let ctx = reaction.context();

        for actions in [
            vec![LegalAction::Hora, reaction.pon(), LegalAction::None],
            vec![reaction.pon(), LegalAction::Hora, LegalAction::None],
        ] {
            assert_eq!(agent.act(&ctx, &actions), LegalAction::Hora);
            let diagnostic = ShantenAgent::diagnose(&ctx, &actions);
            assert_eq!(diagnostic.selected_source, AgentActionSource::Hora);
            // Hora で早期終了するので Pon は検討していない。
            assert_eq!(diagnostic.pon, None);
        }
    }

    #[test]
    fn prefers_ryukyoku_over_an_eligible_pon() {
        let mut agent = ShantenAgent;
        let reaction = dragon_pon_reaction();
        let ctx = reaction.context();
        let actions = vec![reaction.pon(), LegalAction::Ryukyoku, LegalAction::None];

        assert_eq!(agent.act(&ctx, &actions), LegalAction::Ryukyoku);
        let diagnostic = ShantenAgent::diagnose(&ctx, &actions);
        assert_eq!(diagnostic.selected_source, AgentActionSource::Ryukyoku);
        assert_eq!(diagnostic.pon, None);
    }

    #[test]
    fn does_not_claim_chi_or_kans_in_an_eligible_pon_context() {
        // Pon が成立する局面でも、Chi / Daiminkan / Ankan / Kakan は選ばない。
        let mut agent = ShantenAgent;
        let ctx = dragon_pon_reaction().context();
        let actions: Vec<LegalAction> = chi_and_kan_actions()
            .into_iter()
            .chain([LegalAction::None])
            .collect();

        assert_eq!(agent.act(&ctx, &actions), LegalAction::None);
        assert_eq!(ShantenAgent::diagnose(&ctx, &actions).pon, None);
    }

    #[test]
    fn pon_diagnostic_is_absent_without_a_legal_pon() {
        let ctx = dragon_pon_reaction().context();
        let actions = vec![LegalAction::None];
        assert_eq!(ShantenAgent::diagnose(&ctx, &actions).pon, None);
    }

    #[test]
    fn eligible_pon_keeps_the_first_candidate_when_several_are_legal() {
        // 合法 Pon が複数ある場合は、成立条件を満たす最初の候補を採用する。
        let reaction = dragon_pon_reaction();
        let ctx = reaction.context();
        let declined = LegalAction::Pon {
            tile: tile(PON_TARGET),
            consumed: vec![tile(124), tile(120)],
        };
        let actions = vec![declined.clone(), reaction.pon(), LegalAction::None];

        let diagnostic = diagnose_matching_act(&ctx, &actions);
        assert_eq!(diagnostic.selected_action, reaction.pon());

        let pon = diagnostic.pon.as_ref().unwrap();
        assert_eq!(pon.reason, PonDecisionReason::EligibleTenpai);
        assert_eq!(pon.candidates.len(), 2);
        assert_eq!(pon.candidates[0].action, declined);
        assert_eq!(pon.candidates[0].reason, PonDecisionReason::InvalidConsumed);
        assert!(!pon.candidates[0].selected);
        assert!(pon.candidates[1].selected);
    }

    // ---- 恒常フリテン診断 ----

    // 123m456m789m 123p 5s + ツモ 9s。打 9s で 5s 単騎テンパイ、打 1m では1向聴に落ちる。
    fn furiten_hand() -> Vec<TileId> {
        [0u8, 4, 8, 12, 17, 20, 24, 28, 32, 36, 40, 44, 89]
            .iter()
            .map(|&value| tile(value))
            .collect()
    }

    const FURITEN_DRAWN: u8 = 104;
    const FURITEN_WAIT: u8 = 90;

    fn furiten_actions() -> Vec<LegalAction> {
        vec![dahai(FURITEN_DRAWN), dahai(0)]
    }

    fn furiten_context(player_id: Option<u8>, discards: [Vec<TileId>; 4]) -> GameContext {
        let hand = furiten_hand();
        let drawn = tile(FURITEN_DRAWN);
        let mut visible = hand.clone();
        visible.push(drawn);
        for river in &discards {
            visible.extend(river.iter().copied());
        }

        GameContext::from_parts_with_table_state(
            Some(drawn),
            hand,
            vec![],
            None,
            None,
            visible,
            player_id,
            Some(0),
            discards,
            [false; 4],
        )
    }

    fn furiten_of(
        diagnostic: &ShantenDecisionDiagnostic,
        discard: TileType,
    ) -> DiscardFuritenDiagnostic {
        diagnostic
            .normal_discard_furiten
            .as_ref()
            .expect("恒常フリテン診断がある")
            .iter()
            .find(|furiten| furiten.discard == discard)
            .expect("打牌候補がある")
            .clone()
    }

    #[test]
    fn own_river_makes_the_reached_tenpai_permanently_furiten() {
        let ctx = furiten_context(
            Some(0),
            [vec![tile(FURITEN_WAIT), tile(108)], vec![], vec![], vec![]],
        );
        let diagnostic = diagnose_matching_act(&ctx, &furiten_actions());

        let furiten = furiten_of(&diagnostic, tile(FURITEN_DRAWN).tile_type());
        let tenpai = furiten.tenpai.as_ref().expect("テンパイになる");
        assert_eq!(
            tenpai.structural_waits,
            vec![tile(FURITEN_WAIT).tile_type()]
        );
        assert_eq!(tenpai.live_waits, tenpai.structural_waits);
        assert_eq!(furiten.permanent_furiten(), Some(PermanentFuriten::Yes));
        assert_eq!(furiten.discarded_waits(), [tile(FURITEN_WAIT).tile_type()]);
        assert_eq!(tenpai.can_ron(), Some(false));
        // フリテンでもツモ側は既存受け入れのまま。
        assert!(tenpai.tsumo_remaining > 0);
    }

    #[test]
    fn only_the_own_river_makes_the_tenpai_furiten() {
        // 同じ待ち牌が他家の河にあるだけではフリテンにならない。
        let ctx = furiten_context(Some(0), [vec![], vec![tile(FURITEN_WAIT)], vec![], vec![]]);
        let diagnostic = diagnose_matching_act(&ctx, &furiten_actions());

        let furiten = furiten_of(&diagnostic, tile(FURITEN_DRAWN).tile_type());
        assert_eq!(furiten.permanent_furiten(), Some(PermanentFuriten::No));
        assert!(furiten.discarded_waits().is_empty());
        assert_eq!(
            furiten.tenpai.as_ref().expect("テンパイになる").can_ron(),
            Some(true)
        );
    }

    #[test]
    fn an_unknown_player_id_leaves_the_furiten_diagnostic_unknown() {
        // player_id が無い場合、player 0 の河を自分の河と推測しない。
        let ctx = furiten_context(None, [vec![tile(FURITEN_WAIT)], vec![], vec![], vec![]]);
        let diagnostic = diagnose_matching_act(&ctx, &furiten_actions());

        assert_eq!(ctx.own_discards(), None);
        let furiten = furiten_of(&diagnostic, tile(FURITEN_DRAWN).tile_type());
        assert_eq!(furiten.permanent_furiten(), Some(PermanentFuriten::Unknown));
        assert_eq!(
            furiten.tenpai.as_ref().expect("テンパイになる").can_ron(),
            None
        );
    }

    #[test]
    fn candidates_that_do_not_reach_tenpai_have_no_furiten_diagnostic() {
        let ctx = furiten_context(Some(0), [vec![tile(FURITEN_WAIT)], vec![], vec![], vec![]]);
        let diagnostic = diagnose_matching_act(&ctx, &furiten_actions());

        let furiten = furiten_of(&diagnostic, tile(0).tile_type());
        assert!(furiten.tenpai.is_none());
        assert_eq!(furiten.permanent_furiten(), None);
        assert!(furiten.discarded_waits().is_empty());
    }

    #[test]
    fn furiten_diagnostic_covers_every_normal_discard_candidate() {
        let ctx = furiten_context(Some(0), [vec![tile(FURITEN_WAIT)], vec![], vec![], vec![]]);
        let diagnostic = diagnose_matching_act(&ctx, &furiten_actions());

        let normal_discard = diagnostic
            .normal_discard
            .as_ref()
            .expect("normal discard evaluated");
        let furiten = diagnostic
            .normal_discard_furiten
            .as_ref()
            .expect("恒常フリテン診断がある");

        assert_eq!(furiten.len(), normal_discard.candidates.len());
        for (furiten, candidate) in furiten.iter().zip(normal_discard.candidates.iter()) {
            assert_eq!(furiten.discard, candidate.evaluation.discard);
            let Some(tenpai) = furiten.tenpai.as_ref() else {
                continue;
            };
            // 待ちと残枚数は既存の受け入れそのままで、フリテンでも書き換えない。
            assert_eq!(
                tenpai.tsumo_remaining,
                candidate.evaluation.acceptance_total_remaining()
            );
            assert_eq!(
                tenpai.tsumo_type_count,
                candidate.evaluation.acceptance_type_count()
            );
        }
    }

    #[test]
    fn the_furiten_diagnostic_does_not_change_the_selected_action() {
        for player_id in [Some(0), None] {
            let ctx = furiten_context(
                player_id,
                [vec![tile(FURITEN_WAIT)], vec![], vec![], vec![]],
            );
            let actions = furiten_actions();

            let mut agent = ShantenAgent;
            let acted = agent.act(&ctx, &actions);
            let diagnostic = ShantenAgent::diagnose(&ctx, &actions);
            let with_lookahead = ShantenAgent::diagnose_with_options(
                &ctx,
                &actions,
                DiagnosticOptions::WITH_LOOKAHEAD,
            );

            assert_eq!(acted, dahai(FURITEN_DRAWN));
            assert_eq!(diagnostic.selected_action, acted);
            assert_eq!(with_lookahead.selected_action, acted);
            assert_eq!(
                with_lookahead.normal_discard_furiten,
                diagnostic.normal_discard_furiten
            );
        }
    }

    // 123m456m 3456789s + ツモ 1p。打 1p で 3s / 6s / 9s の3面待ちテンパイになる。
    // 3s は手牌に1枚だけなので、残り3枚 (81 / 82 / 83) を見え牌にできる。
    fn three_sided_hand() -> Vec<TileId> {
        [0u8, 4, 8, 12, 17, 20, 80, 84, 89, 92, 96, 100, 104]
            .iter()
            .map(|&value| tile(value))
            .collect()
    }

    fn three_sided_context(extra_visible: &[u8], own_river: &[u8]) -> GameContext {
        three_sided_context_with_player_id(Some(0), extra_visible, own_river)
    }

    // player 0 の河は同じまま、自分の席だけを特定できない局面。
    fn three_sided_context_without_player_id(own_river: &[u8]) -> GameContext {
        three_sided_context_with_player_id(None, &[], own_river)
    }

    fn three_sided_context_with_player_id(
        player_id: Option<u8>,
        extra_visible: &[u8],
        own_river: &[u8],
    ) -> GameContext {
        let hand = three_sided_hand();
        let drawn = tile(36);
        let discards = [
            own_river.iter().map(|&value| tile(value)).collect(),
            vec![],
            vec![],
            vec![],
        ];

        let mut visible = hand.clone();
        visible.push(drawn);
        visible.extend(extra_visible.iter().map(|&value| tile(value)));
        for river in &discards {
            visible.extend(river.iter().copied());
        }

        GameContext::from_parts_with_table_state(
            Some(drawn),
            hand,
            vec![],
            None,
            None,
            visible,
            player_id,
            Some(0),
            discards,
            [false; 4],
        )
    }

    fn three_sided_actions() -> Vec<LegalAction> {
        vec![dahai(36), dahai(0)]
    }

    #[test]
    fn a_fully_visible_discarded_wait_still_reports_furiten() {
        // 待ち 3s を自分が捨てていて 3s が4枚とも見えている局面。3s は既存受け入れから消えるが、
        // 恒常フリテンは解除されない。
        let ctx = three_sided_context(&[82, 83], &[81]);
        let actions = vec![dahai(36), dahai(0)];
        let diagnostic = diagnose_matching_act(&ctx, &actions);

        let furiten = furiten_of(&diagnostic, tile(36).tile_type());
        let tenpai = furiten.tenpai.as_ref().expect("テンパイになる");
        let three_sou = tile(80).tile_type();

        assert_eq!(tenpai.structural_waits.len(), 3);
        assert!(tenpai.structural_waits.contains(&three_sou));
        assert!(!tenpai.live_waits.contains(&three_sou));
        assert_eq!(furiten.permanent_furiten(), Some(PermanentFuriten::Yes));
        assert_eq!(furiten.discarded_waits(), [three_sou]);
        assert_eq!(tenpai.can_ron(), Some(false));

        // ツモ側は見え牌を反映した既存受け入れのまま。
        let evaluation = diagnostic
            .normal_discard
            .as_ref()
            .expect("normal discard evaluated")
            .candidates
            .iter()
            .find(|candidate| candidate.evaluation.discard == tile(36).tile_type())
            .map(|candidate| candidate.evaluation.clone())
            .expect("打 1p の評価がある");
        assert_eq!(
            tenpai.tsumo_remaining,
            evaluation.acceptance_total_remaining()
        );
        assert_eq!(tenpai.tsumo_type_count, evaluation.acceptance_type_count());
        assert_eq!(tenpai.tsumo_type_count, 2);
    }

    #[test]
    fn the_last_visible_copy_of_a_wait_does_not_change_the_furiten_diagnostic() {
        // 残枚数 1 → 0 の境界を跨いでも、フリテン判定・重複した待ち牌・ロン可否は変わらない。
        let actions = vec![dahai(36), dahai(0)];
        let one_left = diagnose_matching_act(&three_sided_context(&[82], &[81]), &actions);
        let none_left = diagnose_matching_act(&three_sided_context(&[82, 83], &[81]), &actions);

        let discard = tile(36).tile_type();
        let with_one_left = furiten_of(&one_left, discard);
        let with_none_left = furiten_of(&none_left, discard);

        assert_eq!(
            with_one_left.tenpai.as_ref().unwrap().live_waits.len(),
            with_none_left.tenpai.as_ref().unwrap().live_waits.len() + 1
        );
        assert_eq!(
            with_one_left.tenpai.as_ref().unwrap().furiten,
            with_none_left.tenpai.as_ref().unwrap().furiten
        );
        assert_eq!(
            with_one_left.permanent_furiten(),
            with_none_left.permanent_furiten()
        );
        assert_eq!(
            with_one_left.discarded_waits(),
            with_none_left.discarded_waits()
        );
        assert_eq!(
            with_one_left.tenpai.as_ref().unwrap().can_ron(),
            with_none_left.tenpai.as_ref().unwrap().can_ron()
        );
        assert_eq!(
            with_none_left.permanent_furiten(),
            Some(PermanentFuriten::Yes)
        );
    }

    #[test]
    fn act_path_does_not_build_the_furiten_diagnostic() {
        let ctx = furiten_context(Some(0), [vec![tile(FURITEN_WAIT)], vec![], vec![], vec![]]);

        let mut diagnostics = DecisionDiagnostics::disabled();
        let _ = ShantenAgent.decide_with_diagnostics(&ctx, &furiten_actions(), &mut diagnostics);

        assert!(diagnostics.normal_discard_furiten.is_none());
    }

    #[test]
    fn agent_action_source_labels_are_stable() {
        assert_eq!(AgentActionSource::Hora.label(), "Hora");
        assert_eq!(AgentActionSource::Ryukyoku.label(), "Ryukyoku");
        assert_eq!(AgentActionSource::Pon.label(), "Pon");
        assert_eq!(AgentActionSource::Reach.label(), "Reach");
        assert_eq!(AgentActionSource::NormalDiscard.label(), "NormalDiscard");
        assert_eq!(
            AgentActionSource::DefenseFallback(DefenseFallbackKind::Genbutsu).label(),
            "DefenseFallback"
        );
        assert_eq!(
            AgentActionSource::LegalDahaiFallback.label(),
            "LegalDahaiFallback"
        );
        assert_eq!(AgentActionSource::None.label(), "None");
    }
}
