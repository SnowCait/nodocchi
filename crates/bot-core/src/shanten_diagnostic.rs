//! ShantenAgent の production decision から診断情報を収集し、最終診断を組み立てる。

use crate::action::LegalAction;
use crate::agents::{AgentActionSource, AgentDecision, ShantenAgent, log_agent_decision};
use crate::call_decision::CallDecisionDiagnostic;
use crate::combined_defense::{CombinedDefenseCategory, CombinedDefenseDiagnostic};
use crate::context::GameContext;
use crate::current_tenpai_continuation::CurrentTenpaiContinuationDiagnostic;
use crate::defense::{DefenseDecisionDiagnostic, DefenseFallbackEvaluation, DefenseFallbackKind};
use crate::discard_selection::{
    DiscardActionSelection, DiscardActionSelectionWithDiagnostic, LookaheadDiagnosticScope,
};
use crate::fold_defense::FoldDefenseEvaluation;
use crate::open_hand_defense::{OpenHandDefenseCategory, OpenHandDefenseDiagnostic};
use crate::open_hand_threat::OpenHandThreatAssessment;
use crate::prospective_value::ProspectiveLookaheadDiagnostic;
use crate::push_pull::{PushPullDecision, PushPullInputs};
use crate::reach_damaten_comparison::{
    ReachDamatenComparisonDiagnostic, ReachDamatenComparisonInputs,
    diagnose_reach_damaten_comparison,
};
use crate::reach_decision::ReachDecisionDiagnostic;
use crate::ryukyoku_decision::RyukyokuDecisionDiagnostic;
use crate::tenpai_continuation::TenpaiContinuationDiagnostic;
use crate::threat::{
    PlayerThreatDiagnostic, diagnose_player_threats_with_facts, player_threat_facts_from_context,
};
use bot_logic::{
    DiscardDecisionDiagnostic, DiscardFuritenDiagnostic, FixedMeldCount, LookaheadDiagnostic,
    SelfTsumoFacts, TenpaiCompletedHands, TwoShantenSelfTsumoDiagnostic,
};

#[cfg(test)]
mod tests;

/// `ShantenAgent` の判断過程を外部の解析ツールから辿るための構造化診断。
///
/// 契約:
///
/// - `selected_action` / `selected_source` は `ShantenAgent::act()` と**同じ selection logic** の
///   結果である。診断専用の別判断ロジックは持たない。
///   常に `selected_action == ShantenAgent::act(context, legal_actions)` が成り立つ。
/// - 追加診断情報(候補ごとの形の内訳、全防御候補評価など)は解析用途であり、action 選択には
///   影響しない。
/// - 実際に実行されなかった判断は `None` で、推測して埋めない。Hora / Ryukyoku / 鳴きで
///   早期終了した場合は `normal_discard` / `normal_discard_action` / `push_pull_inputs` /
///   `push_pull_decision` / `reach` / `defense` がすべて `None`。
/// - `ryukyoku` は `LegalAction::Ryukyoku` が合法だった局面だけ `Some`。九種九牌を宣言せず
///   続行した局面でも保持するので、続行の判断根拠をそのまま辿れる。
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
    /// 通常打牌評価を行った場合の全合法候補のフリテン診断。`normal_discard` と同じ候補集合・
    /// 同じ順序で、候補ごとに「その打牌でテンパイになる場合の待ち・ツモ和了の残枚数と種類数・
    /// ロン可否・自分の河と重複した待ち牌・ロン可否に使った打牌後の履歴依存フリテン」を持つ。
    ///
    /// 判定に使う自分の河は「context の自分の河 + その打牌」で、他家の河や見え牌は使わない。
    /// `player_id` が無く自分の河を特定できない場合は非フリテンと断定せず
    /// [`PermanentFuriten::Unknown`](bot_logic::PermanentFuriten::Unknown) になる。
    /// 打牌選択・押し引き・リーチ判断のどれにも使わない解析専用の情報。
    pub normal_discard_furiten: Option<Vec<DiscardFuritenDiagnostic>>,
    /// 現在時点 (今回の打牌の前) の履歴依存フリテンの production facts。
    ///
    /// [`bot_logic::TenpaiWaitAvailability::can_ron`] が使うのは、この facts を「選択した打牌を切った後」
    /// へ補正した値 ([`GameContext::history_furiten_after_own_discard`]) であり、同じ値とは
    /// 限らない。補正後の facts は各 [`bot_logic::TenpaiWaitAvailability::history_furiten`] が持つ。
    /// 例えば現在 `same_turn = Some(true)` でも、自分のツモを経た今回の打牌後は `Some(false)`
    /// になり、他の軸が非フリテン確定ならロンできる。
    pub history_furiten: bot_logic::HistoryFuritenFacts,
    /// 通常打牌評価を行った場合の全合法候補の詳細な2手先診断。`normal_discard` と同じ候補集合・
    /// 同じ順序で、selected 候補だけでなく runner-up を含む全候補に対応する。
    ///
    /// 構築の有無は `selected_action` / `selected_source` / `normal_discard_action` /
    /// `push_pull_decision` / `defense` / `call` のどれも変えない。構築した場合は打牌選択が使う
    /// 1向聴の weighted tenpai wait もこの枝評価から集計するが、集計対象と集計規則は選択専用
    /// 経路と同じなので結果は一致する。`act()` の経路では構築しない。
    pub normal_discard_lookahead: Option<LookaheadDiagnostic>,
    /// `normal_discard_lookahead` の各枝が選んだ2手目打牌の先にあるテンパイの将来打点。
    /// 2手先診断を構築した場合だけ持ち、同じ候補集合・同じ順序になる。
    ///
    /// 評価対象は既存 lookahead が既存 comparator で選んだ `next_discard` そのもので、打点を見て
    /// 選び直さない。テンパイ枝についてはダマ / リーチ両方の baseline で評価し、未来時点で
    /// リーチが合法かどうかもリーチするかどうかも決めない。打牌選択・押し引き・リーチ判断の
    /// どれにも使わない解析専用の情報で、構築の有無は選択結果を変えない。
    pub normal_discard_lookahead_value: Option<ProspectiveLookaheadDiagnostic>,
    /// 現在打牌後が聴牌の候補について、非和了ツモ1枚と最善打牌で再び聴牌になるダマ継続。
    /// 2手先診断を構築し、かつ自分が未リーチと確定している局面だけ持つ。
    ///
    /// 枝は既存2手先評価の same-shanten の枝そのもの、次打牌は既存 comparator が選んだもの、
    /// 打点は `normal_discard_lookahead_value` が評価済みの値そのもので、この診断のために探索も
    /// 点数計算もやり直さない。現在の和了牌を引いた枝は既存分類上 same-shanten にならないため
    /// 含まれない。待ちが変わる枝も、ツモ切りで元の待ちを維持する枝も同じ枝集合に含む。
    ///
    /// 現時点では diagnostics 専用で、打牌選択・押し引き・リーチ判断のどれにも接続していない。
    /// 構築の有無は選択結果を変えない。
    pub normal_discard_tenpai_continuation: Option<TenpaiContinuationDiagnostic>,
    /// 恒常フリテンが確定した現在聴牌 cohort の `reach now` / `defer → forced Reach` と、既存
    /// timing policy を診断のためだけに適用した結果。
    ///
    /// `normal_discard_tenpai_continuation` が持つ既存 self-tsumo 比較を候補ごとに再利用しており、
    /// この診断のために2手先評価を再実行しない。打牌選択・Reach / Damaten・production Reach
    /// timing のどれにも接続しない。
    pub normal_discard_current_tenpai_continuation: Option<CurrentTenpaiContinuationDiagnostic>,
    /// 現在打牌後が2向聴の候補を、1向聴と同じ self-tsumo 尺度へ揃えた ExpectedSelfTsumoValue。
    /// 打牌候補集合の最善向聴数が2向聴で、かつ2向聴診断を要求した場合だけ持つ。
    ///
    /// 枝は「2向聴 → (Progress / 一度だけの SameShanten) → 1向聴 → 既存の1向聴 continuation」で、
    /// 向聴・受け入れ・打牌比較・将来打点・確率はどれも既存 layer そのまま。
    ///
    /// 現時点では diagnostics 専用で、打牌選択・押し引き・リーチ判断のどれにも接続していない。
    /// 1向聴候補の `expected_self_tsumo_value` とは起点の向聴数が違うため、同じ軸として混ぜない。
    /// 構築の有無は選択結果を変えない。
    pub normal_discard_two_shanten_self_tsumo: Option<TwoShantenSelfTsumoDiagnostic>,
    /// 通常打牌評価で self-tsumo continuation の集計に使った事実。材料が揃わない局面では `None`。
    ///
    /// 選択が実際に使った値そのもので、診断のために求め直さない。詳細な2手先診断を構築した
    /// 場合だけ持つ。
    pub normal_discard_self_tsumo_facts: Option<SelfTsumoFacts>,
    /// 押し引き判定に使った入力。`push_pull_inputs_from_context_with_evaluation()` の実結果。
    pub push_pull_inputs: Option<PushPullInputs>,
    /// 押し引き判定の結果。`decide_push_pull()` の実結果。
    pub push_pull_decision: Option<PushPullDecision>,
    /// リーチを検討した場合の判断内訳。リーチを検討する Push mode 以外では `None`。
    ///
    /// 採用しなかった場合も、通常打牌 selection が選んだ打牌・打牌後の向聴・待ち・恒常フリテンと
    /// 理由を保持する。`act()` と同じ helper の実結果で、診断用の別判断ロジックは持たない。
    pub reach: Option<ReachDecisionDiagnostic>,
    /// リーチを検討した場合の Reach / Damaten 判断材料の統合診断。
    ///
    /// production の判断結果・ダマ Ron 打点・self-tsumo 比較・リーチ Ron baseline を1か所へ
    /// 並べただけの観測値で、どの値も既存診断が持っているものそのもの。self-tsumo 比較は2手先
    /// 診断を構築した場合だけ持ち、この診断のために2手先探索を追加しない。
    ///
    /// 現時点では diagnostics 専用で、Reach / Damaten の production 判断には接続していない。
    /// winner も `should_reach` の再判断も持たず、構築の有無は選択結果を変えない。
    pub reach_damaten_comparison: Option<ReachDamatenComparisonDiagnostic>,
    /// 防御 fallback を検討した場合の診断。採用されなかった場合も候補評価を保持する。
    pub defense: Option<DefenseDecisionDiagnostic>,
    /// 鳴きを検討した場合の診断。合法な Chi / Pon が1件も無ければ `None`。採用しなかった
    /// 場合も候補ごとの理由を保持する。
    pub call: Option<CallDecisionDiagnostic>,
    /// 九種九牌を検討した場合の診断。`LegalAction::Ryukyoku` が合法でない局面と、Hora で
    /// 早期終了した局面では `None`。
    ///
    /// 宣言・続行のどちらでも判断に使った3種類の向聴数を保持する。`act()` と同じ helper の
    /// 実結果で、診断用の別判断ロジックは持たない。手牌を評価できなかった場合の向聴数は
    /// `None` のままで、推測して埋めない。
    pub ryukyoku: Option<RyukyokuDecisionDiagnostic>,
    pub own_fixed_meld_count: Option<FixedMeldCount>,
    /// 全4席分の脅威診断。`context` から読み取れる副露・リーチ・親・ドラの観測事実だけを持つ。
    ///
    /// 集計値 (`facts`) は `act()` が押し引き入力へ渡したものと同じ
    /// [`PlayerThreatFacts`](crate::threat::PlayerThreatFacts) そのもので、診断のために数え直さ
    /// ない。`melds` の物理牌など表示用の詳細だけをこの経路で追加する。
    ///
    /// `player_id` が不明でも席を除外せず常に4席分あり、自分か他家かは各 facts の `is_self` /
    /// `is_opponent()` が unknown で表す。危険度の判断は含まず、現時点では押し引き・防御・
    /// 打牌選択のどれにも影響しない解析専用の情報。
    pub player_threats: [PlayerThreatDiagnostic; 4],
    /// `High` OpenHandThreat の相手に対する防御 safety の診断。
    ///
    /// target は `player_threats` が持つ classification をそのまま source of truth にして選ぶ。
    /// `High` の相手がいない局面では `targets` も `candidates` も空になる。
    ///
    /// 防御 fallback ([`Self::defense`]) がリーチ者向けなのに対し、こちらは非リーチ副露相手
    /// 向けで、現物相当の根拠に `post_reach_passed_tiles` を使わない。`selected` は `act()` が
    /// 実際に採用した OpenHand 防御 fallback で、診断側で選び直さない。採用しなかった局面では
    /// `None` になり、候補評価だけが解析用に残る。
    pub open_hand_defense: OpenHandDefenseDiagnostic,
    /// リーチ者と `High` OpenHandThreat の相手が同時にいる複合 threat 局面の防御 safety の診断。
    ///
    /// target はリーチ情報と `player_threats` が持つ classification をそのまま source of truth に
    /// して選ぶ。複合 threat ではない局面 (リーチ者だけ / `High` の相手だけ / threat なし) では
    /// `targets` も `candidates` も空になり、防御は既存の [`Self::defense`] /
    /// [`Self::open_hand_defense`] が担当する。
    ///
    /// target ごとに「ロン安全」の根拠が違い、リーチ者は現物 (本人の河 + post_reach_passed)、
    /// `High` の副露相手は本人の河と現在有効な一時通過牌を使う。`selected` は `act()` が実際に
    /// 採用した複合 threat 用の防御 fallback で、診断側で選び直さない。
    pub combined_defense: CombinedDefenseDiagnostic,
}

impl ShantenDecisionDiagnostic {
    /// 最終 action がリーチ者向けの防御 fallback 由来の場合のその種別。他の経路では `None`。
    pub fn defense_fallback_kind(&self) -> Option<DefenseFallbackKind> {
        self.selected_source.defense_kind()
    }

    /// 最終 action が非リーチ副露相手向けの防御 fallback 由来の場合のその大分類。
    /// 他の経路では `None`。
    pub fn open_hand_defense_category(&self) -> Option<OpenHandDefenseCategory> {
        self.selected_source.open_hand_defense_category()
    }

    /// 最終 action が複合 threat 向けの防御 fallback 由来の場合のその大分類。他の経路では `None`。
    pub fn combined_defense_category(&self) -> Option<CombinedDefenseCategory> {
        self.selected_source.combined_defense_category()
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
    /// 2手先診断の same-shanten の枝を、テンパイまでもう1段追うかどうか。
    ///
    /// 「same-shanten ツモ → 2手目 → 受け入れのツモ → 3手目 → テンパイ」まで探索するため
    /// `lookahead` だけの場合よりさらに重い。対象は現在打牌後が1向聴の候補だけで、`lookahead`
    /// が無効なら何も構築しない。打牌選択にも押し引きにも使わない観測値なので、有効にしても
    /// 選択結果は変わらない。
    pub same_shanten_downstream: bool,
    /// 現在打牌後が2向聴の候補の ExpectedSelfTsumoValue
    /// ([`ShantenDecisionDiagnostic::normal_discard_two_shanten_self_tsumo`]) を構築するかどうか。
    ///
    /// 「2向聴 → (Progress / 一度だけの SameShanten) → 1向聴 → 既存の1向聴 continuation」まで
    /// 探索するため、`same_shanten_downstream` よりさらに重い。対象は打牌候補集合の最善向聴数が
    /// 2向聴の場合だけで、`lookahead` が無効なら何も構築しない。打牌選択にも押し引きにも
    /// 使わない観測値なので、有効にしても選択結果は変わらない。
    pub two_shanten_self_tsumo: bool,
}

impl DiagnosticOptions {
    /// 既存診断のみ。2手先診断は構築しない。
    pub const NONE: Self = Self {
        lookahead: false,
        same_shanten_downstream: false,
        two_shanten_self_tsumo: false,
    };
    /// 2手先診断まで構築する。
    pub const WITH_LOOKAHEAD: Self = Self {
        lookahead: true,
        same_shanten_downstream: false,
        two_shanten_self_tsumo: false,
    };
    /// 2手先診断に加えて、same-shanten の枝をテンパイまで追う。
    pub const WITH_SAME_SHANTEN_DOWNSTREAM: Self = Self {
        lookahead: true,
        same_shanten_downstream: true,
        two_shanten_self_tsumo: false,
    };
    /// さらに、2向聴候補の ExpectedSelfTsumoValue も求める。
    pub const WITH_TWO_SHANTEN_SELF_TSUMO: Self = Self {
        lookahead: true,
        same_shanten_downstream: true,
        two_shanten_self_tsumo: true,
    };

    pub(crate) fn lookahead_scope(self) -> LookaheadDiagnosticScope {
        match (
            self.lookahead,
            self.same_shanten_downstream,
            self.two_shanten_self_tsumo,
        ) {
            (false, _, _) => LookaheadDiagnosticScope::None,
            (true, false, false) => LookaheadDiagnosticScope::Lookahead,
            (true, _, true) => LookaheadDiagnosticScope::TwoShantenSelfTsumo,
            (true, true, false) => LookaheadDiagnosticScope::SameShantenDownstream,
        }
    }
}

/// `ShantenAgent::act()` と同じ判断を行い、その過程を構造化診断として返す。
///
/// [`ShantenAgent::diagnose`] の別名。契約は [`ShantenDecisionDiagnostic`] を参照。
pub fn diagnose_shanten_decision(
    context: &GameContext,
    legal_actions: &[LegalAction],
) -> ShantenDecisionDiagnostic {
    diagnose_shanten_decision_with_options(context, legal_actions, DiagnosticOptions::NONE)
}

/// 追加診断を指定して `ShantenAgent::act()` と同じ判断を行う。
///
/// [`ShantenAgent::diagnose_with_options`] の別名。
pub fn diagnose_shanten_decision_with_options(
    context: &GameContext,
    legal_actions: &[LegalAction],
    options: DiagnosticOptions,
) -> ShantenDecisionDiagnostic {
    let mut diagnostics = DecisionDiagnostics::enabled_with(options);
    let decision = ShantenAgent.decide_with_diagnostics(context, legal_actions, &mut diagnostics);
    log_agent_decision(&decision);
    diagnostics.finish(context, legal_actions, decision)
}

// 解析専用の追加診断を集める内部収集器。
//
// `enabled == false` の通常 act() 経路では、候補ごとの形の内訳や全防御候補評価といった
// action 選択に不要な情報を一切構築しない。selection logic 自体は enabled にかかわらず共通。
#[derive(Debug, Default)]
pub(crate) struct DecisionDiagnostics {
    enabled: bool,
    options: DiagnosticOptions,
    pub(crate) normal_discard: Option<DiscardDecisionDiagnostic>,
    pub(crate) normal_discard_furiten: Option<Vec<DiscardFuritenDiagnostic>>,
    pub(crate) normal_discard_lookahead: Option<LookaheadDiagnostic>,
    normal_discard_lookahead_value: Option<ProspectiveLookaheadDiagnostic>,
    normal_discard_tenpai_continuation: Option<TenpaiContinuationDiagnostic>,
    normal_discard_current_tenpai_continuation: Option<CurrentTenpaiContinuationDiagnostic>,
    normal_discard_two_shanten_self_tsumo: Option<TwoShantenSelfTsumoDiagnostic>,
    normal_discard_self_tsumo_facts: Option<SelfTsumoFacts>,
    pub(crate) reach_damaten_comparison: Option<ReachDamatenComparisonDiagnostic>,
    pub(crate) defense: Option<DefenseDecisionDiagnostic>,
    open_hand_defense: Option<OpenHandDefenseDiagnostic>,
    combined_defense: Option<CombinedDefenseDiagnostic>,
}

impl DecisionDiagnostics {
    pub(crate) fn disabled() -> Self {
        Self::default()
    }

    pub(crate) fn enabled_with(options: DiagnosticOptions) -> Self {
        Self {
            enabled: true,
            options,
            ..Self::default()
        }
    }

    #[cfg(test)]
    pub(crate) fn enabled() -> Self {
        Self::enabled_with(DiagnosticOptions::NONE)
    }

    pub(crate) fn is_enabled(&self) -> bool {
        self.enabled
    }

    pub(crate) fn lookahead_scope(&self) -> LookaheadDiagnosticScope {
        self.options.lookahead_scope()
    }

    pub(crate) fn collect_normal_discard(
        &mut self,
        selection: DiscardActionSelectionWithDiagnostic,
    ) -> DiscardActionSelection {
        self.normal_discard = Some(selection.diagnostic);
        self.normal_discard_furiten = Some(selection.furiten);
        self.normal_discard_lookahead = selection.lookahead;
        self.normal_discard_lookahead_value = selection.lookahead_value;
        self.normal_discard_tenpai_continuation = selection.tenpai_continuation;
        self.normal_discard_current_tenpai_continuation = selection.current_tenpai_continuation;
        self.normal_discard_two_shanten_self_tsumo = selection.two_shanten_self_tsumo;
        self.normal_discard_self_tsumo_facts = selection.self_tsumo_facts;
        selection.selection
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn collect_reach_damaten_comparison(
        &mut self,
        context: &GameContext,
        legal_actions: &[LegalAction],
        reach: &ReachDecisionDiagnostic,
        selection: &DiscardActionSelection,
        hands: Option<TenpaiCompletedHands>,
        open_hand_threats: &[OpenHandThreatAssessment; 4],
    ) {
        if !self.enabled {
            return;
        }
        self.reach_damaten_comparison = Some(diagnose_reach_damaten_comparison(
            ReachDamatenComparisonInputs {
                context,
                reach_legal: legal_actions
                    .iter()
                    .any(|action| matches!(action, LegalAction::Reach)),
                reach,
                selection,
                hands,
                continuation: self.normal_discard_tenpai_continuation.as_ref(),
                open_hand_threats,
            },
        ));
    }

    pub(crate) fn collect_fold_defense(
        &mut self,
        context: &GameContext,
        legal_actions: &[LegalAction],
        inputs: &PushPullInputs,
        evaluation: &FoldDefenseEvaluation<'_>,
    ) {
        if !self.enabled {
            return;
        }

        match evaluation {
            FoldDefenseEvaluation::Reach(evaluation) => {
                self.defense = Some(DefenseDecisionDiagnostic::from_evaluation(
                    context,
                    legal_actions,
                    evaluation,
                ));
            }
            FoldDefenseEvaluation::OpenHand(evaluation) => {
                self.open_hand_defense = Some(OpenHandDefenseDiagnostic::from_evaluation(
                    context,
                    legal_actions,
                    &inputs.open_hand_threats,
                    evaluation,
                ));
            }
            FoldDefenseEvaluation::Combined(evaluation) => {
                self.combined_defense = Some(CombinedDefenseDiagnostic::from_evaluation(
                    context,
                    legal_actions,
                    &inputs.player_threats,
                    &inputs.open_hand_threats,
                    evaluation,
                ));
            }
        }
    }

    pub(crate) fn collect_defense(
        &mut self,
        context: &GameContext,
        legal_actions: &[LegalAction],
        evaluation: &DefenseFallbackEvaluation<'_>,
    ) {
        if self.enabled {
            self.defense = Some(DefenseDecisionDiagnostic::from_evaluation(
                context,
                legal_actions,
                evaluation,
            ));
        }
    }

    fn finish(
        mut self,
        context: &GameContext,
        legal_actions: &[LegalAction],
        decision: AgentDecision,
    ) -> ShantenDecisionDiagnostic {
        // 押し引きまで進んだ場合はそのとき使った facts をそのまま診断へ載せ、集計を作り直さない。
        // Hora / Ryukyoku / 鳴きで早期終了した場合だけ、診断のためにここで facts を作る。
        let player_threat_facts = decision.push_pull_inputs.map_or_else(
            || player_threat_facts_from_context(context),
            |inputs| inputs.player_threats,
        );
        let player_threats = diagnose_player_threats_with_facts(context, &player_threat_facts);

        // OpenHand 防御の target は診断が持つ classification をそのまま使い、分類し直さない。
        let open_hand_threats =
            std::array::from_fn(|player| player_threats[player].open_hand_threat);

        // 採用された防御 fallback は production decision が通った経路そのもの。ここで選び直さない。
        let open_hand_defense = self.open_hand_defense.take().unwrap_or_else(|| {
            OpenHandDefenseDiagnostic::from_assessments(
                context,
                legal_actions,
                &open_hand_threats,
                decision
                    .source
                    .open_hand_defense_category()
                    .map(|category| (&decision.action, category)),
            )
        });
        let combined_defense = self.combined_defense.take().unwrap_or_else(|| {
            CombinedDefenseDiagnostic::from_threats(
                context,
                legal_actions,
                &player_threat_facts,
                &open_hand_threats,
                decision
                    .source
                    .combined_defense_category()
                    .map(|category| (&decision.action, category)),
            )
        });

        ShantenDecisionDiagnostic {
            selected_action: decision.action,
            selected_source: decision.source,
            normal_discard_action: decision.normal_discard,
            normal_discard: self.normal_discard,
            normal_discard_furiten: self.normal_discard_furiten,
            history_furiten: context.history_furiten(),
            normal_discard_lookahead: self.normal_discard_lookahead,
            normal_discard_lookahead_value: self.normal_discard_lookahead_value,
            normal_discard_tenpai_continuation: self.normal_discard_tenpai_continuation,
            normal_discard_current_tenpai_continuation: self
                .normal_discard_current_tenpai_continuation,
            normal_discard_two_shanten_self_tsumo: self.normal_discard_two_shanten_self_tsumo,
            normal_discard_self_tsumo_facts: self.normal_discard_self_tsumo_facts,
            push_pull_inputs: decision.push_pull_inputs,
            push_pull_decision: decision.push_pull,
            reach: decision.reach,
            reach_damaten_comparison: self.reach_damaten_comparison,
            defense: self.defense,
            call: decision.call,
            ryukyoku: decision.ryukyoku,
            own_fixed_meld_count: context.own_fixed_meld_count(),
            player_threats,
            open_hand_defense,
            combined_defense,
        }
    }
}
