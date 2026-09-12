use std::fmt::Debug;
use std::time::{Duration, Instant};

use bot_logic::{
    ForwardMetricsObserver, ForwardMetricsPhase, SearchStateMemoStats, TileId, TileType,
    TwoShantenSelfTsumoObserver,
};

use crate::action::LegalAction;
use crate::call_decision::CallKind;
use crate::prospective_value::tenpai_value_memo_counter;

/// 意思決定1回を phase 別に分けた実測時間。
///
/// phase は production path の判断順にそのまま対応する。早期 return した局面では、
/// 到達しなかった phase は `Duration::ZERO` のままになる。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct DecisionPhaseDurations {
    /// Hora / Ryukyoku / 鳴きなど、通常打牌選択より前。
    pub early: Duration,
    /// 通常打牌選択の全体。
    pub normal_discard: Duration,
    /// `normal_discard` の内訳。通常打牌選択を通らなかった局面と、構造化診断つきの経路では
    /// すべて `Duration::ZERO` のままになる。
    pub normal_discard_phases: NormalDiscardPhaseDurations,
    /// 通常打牌選択より後の押し引き / Reach / 防御 / 最終 action 選択。
    pub post_discard: Duration,
    /// `early` の内訳のうち鳴き判断。合法な Chi / Pon が無い局面では、すべて
    /// `Duration::ZERO` のままになる。
    pub call: CallDecisionDurations,
}

impl DecisionPhaseDurations {
    pub fn total(&self) -> Duration {
        self.early + self.normal_discard + self.post_discard
    }
}

/// 鳴き判断1回を内部処理別に分けた実測時間。
///
/// 合計は `DecisionPhaseDurations::early` を超えない。合法な Chi / Pon が1件も無く候補評価を
/// 通らなかった局面では、すべて `Duration::ZERO` のままになる。
///
/// `total` は壁時計で、内訳はそれぞれの処理が実際に走っていた時間。Call 側の候補評価と Pass
/// 側の継続評価を別 thread で重ねた局面では、内訳の合計が `total` を超える。これは既存の
/// 候補単位の並列評価と同じ semantics で、重ねた分だけ壁時計が内訳より短くなる。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CallDecisionDurations {
    /// 鳴き判断全体の壁時計。最初の候補評価から最終候補の選択まで。
    pub total: Duration,
    /// 実際に評価した鳴き候補の合計。semantic に同一で結果を再利用した候補は評価を行わない
    /// ので含まない。
    pub candidates: Duration,
    /// 1向聴 Call / Pass 比較のために1回だけ評価する Pass 側の ExpectedSelfTsumoValue。
    /// 比較が発火しなかった局面では `Duration::ZERO` のままになる。Call 側と重ねて評価した
    /// 局面でも、この値は Pass 側の評価そのものにかかった時間で、待ち時間を含まない。
    pub pass_iishanten_self_tsumo: Duration,
}

impl CallDecisionDurations {
    /// 候補評価と Pass 評価を除いた残りの鳴き policy 処理。
    ///
    /// Call / Pass を重ねた局面では内訳の合計が壁時計を超えるため、`Duration::ZERO` になる。
    pub fn remaining(&self) -> Duration {
        self.total
            .saturating_sub(self.candidates + self.pass_iishanten_self_tsumo)
    }
}

/// production が並べた鳴き候補1件の実測。
///
/// `kind` / `tile` / `consumed` は候補になった合法 action そのもので、同じ牌の重複候補も
/// 合法 action の列挙順でそれぞれ1件ずつ並ぶ。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CallCandidateDuration {
    pub kind: CallKind,
    pub tile: TileId,
    pub consumed: Vec<TileId>,
    /// 候補1件の評価全体。`reused` の候補では評価を行わないため `Duration::ZERO`。
    pub elapsed: Duration,
    /// そのうち鳴き後の打牌選択 (前方評価を含む)。そこまで進まなかった候補と `reused` の候補
    /// では `Duration::ZERO` のままになる。
    pub post_call_discard_selection: Duration,
    /// 先に評価した semantic に同一な候補の結果をそのまま使ったか。`true` の候補では鳴き後の
    /// 打牌評価を実行していないので、実測も 0 になる。
    pub reused: bool,
}

/// 通常打牌選択1回を内部処理別に分けた実測時間。
///
/// 合計は `DecisionPhaseDurations::normal_discard` を超えない。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct NormalDiscardPhaseDurations {
    /// 合法打牌候補の生成と、向聴 / 受け入れなどの基本評価。
    pub base_evaluation: Duration,
    /// 打牌選択が使う前方集計値 (lookahead / forward metrics)。
    pub forward_metrics: Duration,
    /// `forward_metrics` の内訳。前方集計値を計算しなかった局面では、すべて `Duration::ZERO` の
    /// ままになる。
    pub forward_metrics_phases: ForwardMetricsPhaseDurations,
    /// production comparator が評価する2向聴 Progress-first と、ドラ差 gate の
    /// Full 追加評価。
    /// 対象外の局面では `Duration::ZERO` のままになる。
    pub two_shanten_self_tsumo: Duration,
    /// production comparator が評価する3向聴 Progress-only self-tsumo value。
    /// 対象外の局面では `Duration::ZERO` のままになる。
    pub three_shanten_self_tsumo: Duration,
    /// 残りの補助評価 (現在聴牌候補の待ち / 打点 / ツモ期待値) と候補比較・最終打牌の確定。
    pub selection_finalize: Duration,
}

impl NormalDiscardPhaseDurations {
    pub fn total(&self) -> Duration {
        self.base_evaluation
            + self.forward_metrics
            + self.two_shanten_self_tsumo
            + self.three_shanten_self_tsumo
            + self.selection_finalize
    }
}

/// 前方集計値1回を内部処理別に分けた実測時間。
///
/// 合計は `NormalDiscardPhaseDurations::forward_metrics` を超えない。前方集計値の入力を
/// 組み立てる時間はどの内訳にも入らない。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ForwardMetricsPhaseDurations {
    /// 仮想ツモ枝の探索。ツモ後の次打牌評価と、その枝が使う将来打点の scoring を含む。
    pub lookahead_search: Duration,
    /// 探索済みの枝からの重み付き集計 (weighted tenpai wait / weighted next acceptance)。
    pub weighted_aggregation: Duration,
    /// 探索済みの枝からの self-tsumo continuation の集計。
    pub self_tsumo_continuation: Duration,
}

impl ForwardMetricsPhaseDurations {
    pub fn total(&self) -> Duration {
        self.lookahead_search + self.weighted_aggregation + self.self_tsumo_continuation
    }
}

/// production comparator が実際に評価した2向聴 Progress / Full 候補1件の実測。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct TwoShantenSelfTsumoCandidateDuration {
    pub discard: TileType,
    pub elapsed: Duration,
}

/// production が1向聴の深い前方評価を実際に行った候補1件の実測と仕事量。
///
/// 並ぶのは深い前方評価の対象になった候補 ([`bot_logic::forward_target_mask`]) だけで、
/// 対象外の候補は前方評価そのものを通らないため1件も入らない。順序は production の候補順
/// (合法打牌の評価順) そのままで、並行に評価した局面でも thread の終了順には依らない。
///
/// `elapsed` はその候補を評価していた実時間で、phase の壁時計ではない。候補単位で並行に評価
/// する production では、候補の `elapsed` の合計が
/// [`NormalDiscardPhaseDurations::forward_metrics`] の壁時計を超える。これは既存の2向聴 Full
/// 候補や Call / Pass の重なりと同じ semantics で、重ねた分だけ壁時計が内訳より短くなる。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IishantenForwardCandidateDuration {
    pub discard: TileType,
    /// 候補1件の前方評価全体。
    pub elapsed: Duration,
    /// そのうちの内部処理別の内訳。合計は `elapsed` を超えない。
    pub phases: ForwardMetricsPhaseDurations,
    /// 候補1件の評価が使った探索内の同一 state memo の利用数。memo を持たない局面では 0。
    pub search_state_memo: SearchStateMemoStats,
    /// 候補1件の評価が引いた未来テンパイの値 memo の利用数。miss は実際に打点を評価した件数。
    pub tenpai_value_memo_hits: u64,
    pub tenpai_value_memo_misses: u64,
}

/// 計測付きで実行した意思決定の最終 action と phase 別実測時間。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TimedAgentAction {
    pub action: LegalAction,
    pub phases: DecisionPhaseDurations,
    pub(crate) two_shanten_self_tsumo_candidates: Vec<TwoShantenSelfTsumoCandidateDuration>,
    pub(crate) iishanten_forward_candidates: Vec<IishantenForwardCandidateDuration>,
    pub(crate) call_candidates: Vec<CallCandidateDuration>,
}

impl TimedAgentAction {
    /// 同じ production execution で実際に評価した `ForwardTargets` の Progress と、
    /// gate 対象 pair の Full 追加評価の打牌・実測時間。Full 対象は同じ牌種が2回現れる。
    /// 対象外の request では空。内部の計測用 representation は公開しない。
    pub fn two_shanten_self_tsumo_candidates(
        &self,
    ) -> impl ExactSizeIterator<Item = (TileType, Duration)> + '_ {
        self.two_shanten_self_tsumo_candidates
            .iter()
            .map(|candidate| (candidate.discard, candidate.elapsed))
    }

    /// 同じ production execution で1向聴の深い前方評価を実際に行った候補の実測。最善向聴数が
    /// 1向聴でない request と、前方評価を通らなかった request では空。
    pub fn iishanten_forward_candidates(&self) -> &[IishantenForwardCandidateDuration] {
        &self.iishanten_forward_candidates
    }

    /// 同じ production execution で実際に評価した鳴き候補の実測。合法な Chi / Pon が無い
    /// request では空。
    pub fn call_candidates(&self) -> &[CallCandidateDuration] {
        &self.call_candidates
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DecisionPhase {
    Early,
    NormalDiscard,
    PostDiscard,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum NormalDiscardPhase {
    BaseEvaluation,
    ForwardMetrics,
    TwoShantenSelfTsumo,
    ThreeShantenSelfTsumo,
    SelectionFinalize,
}

/// phase 別に経過時間を積み上げられる計測結果。
pub(crate) trait PhaseDurations: Default + Debug {
    type Phase: Copy + Debug;

    /// 計測開始時点の phase。
    const FIRST: Self::Phase;

    fn accumulate(&mut self, phase: Self::Phase, elapsed: Duration);
}

impl PhaseDurations for DecisionPhaseDurations {
    type Phase = DecisionPhase;

    const FIRST: Self::Phase = DecisionPhase::Early;

    fn accumulate(&mut self, phase: Self::Phase, elapsed: Duration) {
        match phase {
            DecisionPhase::Early => self.early += elapsed,
            DecisionPhase::NormalDiscard => self.normal_discard += elapsed,
            DecisionPhase::PostDiscard => self.post_discard += elapsed,
        }
    }
}

impl PhaseDurations for ForwardMetricsPhaseDurations {
    type Phase = ForwardMetricsPhase;

    const FIRST: Self::Phase = ForwardMetricsPhase::LookaheadSearch;

    fn accumulate(&mut self, phase: Self::Phase, elapsed: Duration) {
        match phase {
            ForwardMetricsPhase::LookaheadSearch => self.lookahead_search += elapsed,
            ForwardMetricsPhase::WeightedAggregation => self.weighted_aggregation += elapsed,
            ForwardMetricsPhase::SelfTsumoContinuation => self.self_tsumo_continuation += elapsed,
        }
    }
}

impl PhaseDurations for NormalDiscardPhaseDurations {
    type Phase = NormalDiscardPhase;

    const FIRST: Self::Phase = NormalDiscardPhase::BaseEvaluation;

    fn accumulate(&mut self, phase: Self::Phase, elapsed: Duration) {
        match phase {
            NormalDiscardPhase::BaseEvaluation => self.base_evaluation += elapsed,
            NormalDiscardPhase::ForwardMetrics => self.forward_metrics += elapsed,
            NormalDiscardPhase::TwoShantenSelfTsumo => self.two_shanten_self_tsumo += elapsed,
            NormalDiscardPhase::ThreeShantenSelfTsumo => self.three_shanten_self_tsumo += elapsed,
            NormalDiscardPhase::SelectionFinalize => self.selection_finalize += elapsed,
        }
    }
}

/// production path へ差し込む optional な phase 計測器。
///
/// 無効時は `Instant` を一切取得せず、判断内容にも影響しない。判断を再実行することは
/// なく、通った経路の経過時間をその場で計上するだけ。
#[derive(Debug)]
pub(crate) struct PhaseTimer<D: PhaseDurations, B: Default = ()> {
    state: Option<TimerState<D>>,
    // 可変長の内訳は duration DTO から分離する。forward 専用 timer は `()` のまま。
    breakdown: B,
}

/// 意思決定1回の可変長の内訳。scalar な duration DTO と分けて持つ。
#[derive(Debug, Default)]
pub(crate) struct DecisionBreakdown {
    normal_discard: NormalDiscardBreakdown,
    call_candidates: Vec<CallCandidateDuration>,
}

/// 通常打牌選択1回の可変長の内訳。候補単位の実測は向聴数別にそれぞれ別の列で持つ。
#[derive(Debug, Default)]
pub(crate) struct NormalDiscardBreakdown {
    two_shanten_self_tsumo_candidates: Vec<TwoShantenSelfTsumoCandidateDuration>,
    iishanten_forward_candidates: Vec<IishantenForwardCandidateDuration>,
}

pub(crate) type DecisionPhaseTimer = PhaseTimer<DecisionPhaseDurations, DecisionBreakdown>;
pub(crate) type NormalDiscardPhaseTimer =
    PhaseTimer<NormalDiscardPhaseDurations, NormalDiscardBreakdown>;

/// 1向聴の深い前方評価を、phase 別と候補別に計る optional な計測器。
///
/// 無効時は区切りの通知を受けても `Instant` を取得しない。有効な場合も通った区切りの経過時間を
/// その場で計上するだけで、対象候補も枝の探索も集計も計測の有無で変わらない。
///
/// `phases` は通った区切りの合計で、候補単位で並行に評価した局面では候補の内訳の足し合わせに
/// なる。その場合は前方集計 phase の壁時計を超え得る。
#[derive(Debug)]
pub(crate) struct ForwardMetricsPhaseTimer {
    state: Option<ForwardMetricsTimerState>,
}

#[derive(Debug)]
struct ForwardMetricsTimerState {
    phases: ForwardMetricsPhaseDurations,
    candidates: Vec<IishantenForwardCandidateDuration>,
    /// 候補単位の実測を残すか。最善向聴数が1向聴でない局面では `false` で、候補の区切りを
    /// 受けても列へ積まない。
    measures_candidates: bool,
    /// 区切りを開いたままの候補。閉じた時点で1件として `candidates` へ積む。
    current: Option<OpenForwardCandidate>,
    /// 直前の区切りからの起点。
    since: Instant,
    /// 計上先の phase。最初の区切りを待っている間は `None`。
    phase: Option<ForwardMetricsPhase>,
}

#[derive(Debug)]
struct OpenForwardCandidate {
    discard: TileType,
    started: Instant,
    phases: ForwardMetricsPhaseDurations,
    /// 候補へ入った時点の累計。候補1件分の利用数は閉じる時点との差になる。
    search_state_memo: SearchStateMemoStats,
    tenpai_value_memo: (u64, u64),
}

/// 2向聴 ExpectedSelfTsumoValue の候補別 optional 計測器。
///
/// 無効時は observer の通知を受けても `Instant` を取得しない。
#[derive(Debug)]
pub(crate) struct TwoShantenSelfTsumoTimer {
    state: Option<TwoShantenSelfTsumoTimerState>,
}

#[derive(Debug, Default)]
struct TwoShantenSelfTsumoTimerState {
    current: Option<(TileType, Instant)>,
    elapsed: Vec<TwoShantenSelfTsumoCandidateDuration>,
}

#[derive(Debug)]
struct TimerState<D: PhaseDurations> {
    /// 計上先の phase。最初の `enter()` を待っている間は `None` で、その間の経過時間はどの
    /// phase にも計上しない。
    phase: Option<D::Phase>,
    since: Instant,
    durations: D,
}

impl<D: PhaseDurations, B: Default> PhaseTimer<D, B> {
    pub(crate) fn disabled() -> Self {
        Self {
            state: None,
            breakdown: B::default(),
        }
    }

    pub(crate) fn started() -> Self {
        Self {
            state: Some(TimerState {
                phase: Some(D::FIRST),
                since: Instant::now(),
                durations: D::default(),
            }),
            breakdown: B::default(),
        }
    }

    /// 現在の phase へ経過時間を計上し、次の phase へ進める。
    pub(crate) fn enter(&mut self, phase: D::Phase) {
        if let Some(state) = self.state.as_mut() {
            state.flush();
            state.phase = Some(phase);
        }
    }

    /// 最後の phase へ経過時間を計上して結果を返す。早期 return した局面では、
    /// その時点の phase へそのまま計上される。
    pub(crate) fn finish(mut self) -> D {
        match self.state.take() {
            Some(mut state) => {
                state.flush();
                state.durations
            }
            None => D::default(),
        }
    }
}

impl DecisionPhaseTimer {
    /// 通常打牌選択の内訳を計る子計測器。外側の計測が有効な場合だけ有効にする。
    pub(crate) fn normal_discard_timer(&self) -> NormalDiscardPhaseTimer {
        match self.state {
            Some(_) => NormalDiscardPhaseTimer::started(),
            None => NormalDiscardPhaseTimer::disabled(),
        }
    }

    /// 通常打牌選択の内訳を計上する。内訳を計る経路を通らなかった場合は呼ばれず、
    /// 既定値の 0 がそのまま残る。
    pub(crate) fn record_normal_discard_phases(&mut self, durations: NormalDiscardPhaseDurations) {
        if let Some(state) = self.state.as_mut() {
            state.durations.normal_discard_phases = durations;
        }
    }

    /// 鳴き判断の内訳を計る子計測器。外側の計測が有効な場合だけ有効にする。
    ///
    /// 合法な Chi / Pon が1件も無い局面では候補の区切りを1つも通らないため、内訳は 0 の
    /// ままになる。
    pub(crate) fn call_timer(&self) -> CallDecisionTimer {
        match self.state {
            Some(_) => CallDecisionTimer::armed(),
            None => CallDecisionTimer::disabled(),
        }
    }

    /// 鳴き判断の内訳を計上する。
    pub(crate) fn record_call(
        &mut self,
        durations: CallDecisionDurations,
        candidates: Vec<CallCandidateDuration>,
    ) {
        if let Some(state) = self.state.as_mut() {
            state.durations.call = durations;
            self.breakdown.call_candidates = candidates;
        }
    }

    pub(crate) fn take_call_candidates(&mut self) -> Vec<CallCandidateDuration> {
        std::mem::take(&mut self.breakdown.call_candidates)
    }

    /// 可変長の内訳は scalar duration DTO とは別に保持する。
    pub(crate) fn record_normal_discard_breakdown(&mut self, breakdown: NormalDiscardBreakdown) {
        if self.state.is_some() {
            self.breakdown.normal_discard = breakdown;
        }
    }

    pub(crate) fn take_two_shanten_self_tsumo_candidates(
        &mut self,
    ) -> Vec<TwoShantenSelfTsumoCandidateDuration> {
        std::mem::take(
            &mut self
                .breakdown
                .normal_discard
                .two_shanten_self_tsumo_candidates,
        )
    }

    pub(crate) fn take_iishanten_forward_candidates(
        &mut self,
    ) -> Vec<IishantenForwardCandidateDuration> {
        std::mem::take(&mut self.breakdown.normal_discard.iishanten_forward_candidates)
    }
}

/// 鳴き判断へ差し込む optional な計測器。
///
/// 無効時は `Instant` を一切取得せず、判断内容にも影響しない。最初の候補評価まで全体の計測も
/// 始めないため、鳴き候補が無い局面ではすべて 0 のままになる。
#[derive(Debug)]
pub(crate) struct CallDecisionTimer {
    state: Option<CallDecisionTimerState>,
}

#[derive(Debug, Default)]
struct CallDecisionTimerState {
    since: Option<Instant>,
    durations: CallDecisionDurations,
    candidates: Vec<CallCandidateDuration>,
}

impl CallDecisionTimer {
    pub(crate) fn disabled() -> Self {
        Self { state: None }
    }

    pub(crate) fn armed() -> Self {
        Self {
            state: Some(CallDecisionTimerState::default()),
        }
    }

    /// 候補1件を計る子計測器。最初の候補で鳴き判断全体の計測も始める。
    pub(crate) fn candidate_timer(&mut self) -> CallCandidateTimer {
        match self.state.as_mut() {
            Some(state) => {
                state.since.get_or_insert_with(Instant::now);
                CallCandidateTimer::started()
            }
            None => CallCandidateTimer::disabled(),
        }
    }

    /// 候補1件の実測を、評価した合法 action と対応付けて計上する。
    pub(crate) fn record_candidate(
        &mut self,
        kind: CallKind,
        tile: TileId,
        consumed: &[TileId],
        elapsed: CallCandidateElapsed,
    ) {
        if let Some(state) = self.state.as_mut() {
            state.durations.candidates += elapsed.total;
            state.candidates.push(CallCandidateDuration {
                kind,
                tile,
                consumed: consumed.to_vec(),
                elapsed: elapsed.total,
                post_call_discard_selection: elapsed.post_call_discard_selection,
                reused: false,
            });
        }
    }

    /// 先に評価した semantic に同一な候補の結果を再利用した候補を計上する。
    ///
    /// 候補そのものは合法 action の列挙順で残すが、鳴き後の打牌評価は実行していないので実測は
    /// 0 で、`candidates` の合計にも足さない。結果を複製するだけの時間は鳴き判断全体との差分
    /// (`CallDecisionDurations::remaining()`) に残る。
    pub(crate) fn record_reused_candidate(
        &mut self,
        kind: CallKind,
        tile: TileId,
        consumed: &[TileId],
    ) {
        if let Some(state) = self.state.as_mut() {
            state.since.get_or_insert_with(Instant::now);
            state.candidates.push(CallCandidateDuration {
                kind,
                tile,
                consumed: consumed.to_vec(),
                elapsed: Duration::ZERO,
                post_call_discard_selection: Duration::ZERO,
                reused: true,
            });
        }
    }

    /// 計測が有効か。別 thread で行う評価の実測を取るかどうかの判断に使う。
    ///
    /// 無効な run では実測を取らないので、そちらの `Instant` も取得しない。
    pub(crate) fn is_armed(&self) -> bool {
        self.state.is_some()
    }

    /// 鳴き判断全体の計測を始める。既に始まっている場合は何もしない。
    ///
    /// 候補評価の手前に安価な事前判定を置く経路が、その分を全体の壁時計へ含めるための入口。
    pub(crate) fn start(&mut self) {
        if let Some(state) = self.state.as_mut() {
            state.since.get_or_insert_with(Instant::now);
        }
    }

    /// 別 thread で評価した Pass 側の実測を計上する。
    ///
    /// Call 側と重ねて評価した場合に使う。計上するのは Pass の評価そのものにかかった時間で、
    /// join を待った時間は含まない。
    pub(crate) fn record_pass_iishanten_self_tsumo(&mut self, elapsed: Duration) {
        if let Some(state) = self.state.as_mut() {
            state.durations.pass_iishanten_self_tsumo += elapsed;
        }
    }

    /// Pass 側の1向聴 ExpectedSelfTsumoValue の評価を計る。無効時は `Instant` を取得しない。
    pub(crate) fn measure_pass_iishanten_self_tsumo<T>(
        &mut self,
        evaluate: impl FnOnce() -> T,
    ) -> T {
        let Some(state) = self.state.as_mut() else {
            return evaluate();
        };
        let since = Instant::now();
        let value = evaluate();
        state.durations.pass_iishanten_self_tsumo += since.elapsed();
        value
    }

    pub(crate) fn finish(self) -> (CallDecisionDurations, Vec<CallCandidateDuration>) {
        match self.state {
            Some(mut state) => {
                if let Some(since) = state.since {
                    state.durations.total = since.elapsed();
                }
                (state.durations, state.candidates)
            }
            None => (CallDecisionDurations::default(), Vec::new()),
        }
    }
}

/// 鳴き候補1件の実測。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct CallCandidateElapsed {
    total: Duration,
    post_call_discard_selection: Duration,
}

impl CallCandidateElapsed {
    /// 同じ候補を複数回に分けて計った実測を足し合わせる。
    ///
    /// 候補1件の評価を安価な事前判定と高コストな鳴き後打牌選択に分けて行う経路が、その候補が
    /// 実際に払った時間を1件分としてまとめるための入口。間に挟まる他候補の評価や待ち時間は
    /// どちらの区間にも入らない。
    pub(crate) fn merged(self, other: Self) -> Self {
        Self {
            total: self.total + other.total,
            post_call_discard_selection: self.post_call_discard_selection
                + other.post_call_discard_selection,
        }
    }
}

/// 鳴き候補1件へ差し込む optional な計測器。
#[derive(Debug)]
pub(crate) struct CallCandidateTimer {
    state: Option<CallCandidateTimerState>,
}

#[derive(Debug)]
struct CallCandidateTimerState {
    since: Instant,
    post_call_discard_selection: Duration,
}

impl CallCandidateTimer {
    pub(crate) fn disabled() -> Self {
        Self { state: None }
    }

    pub(crate) fn started() -> Self {
        Self {
            state: Some(CallCandidateTimerState {
                since: Instant::now(),
                post_call_discard_selection: Duration::ZERO,
            }),
        }
    }

    /// 鳴き後の打牌選択を計る。無効時は `Instant` を取得しない。
    pub(crate) fn measure_post_call_discard_selection<T>(
        &mut self,
        select: impl FnOnce() -> T,
    ) -> T {
        let Some(state) = self.state.as_mut() else {
            return select();
        };
        let since = Instant::now();
        let selection = select();
        state.post_call_discard_selection += since.elapsed();
        selection
    }

    pub(crate) fn finish(self) -> CallCandidateElapsed {
        match self.state {
            Some(state) => CallCandidateElapsed {
                total: state.since.elapsed(),
                post_call_discard_selection: state.post_call_discard_selection,
            },
            None => CallCandidateElapsed::default(),
        }
    }
}

impl NormalDiscardPhaseTimer {
    /// 前方集計値の内訳を計る子計測器。外側の計測が有効な場合だけ有効にする。
    ///
    /// 前方集計値を計算しない局面では区切りを1つも通らないため、内訳は 0 のままになる。
    pub(crate) fn forward_metrics_timer(&self) -> ForwardMetricsPhaseTimer {
        match self.state {
            Some(_) => ForwardMetricsPhaseTimer::armed(),
            None => ForwardMetricsPhaseTimer::disabled(),
        }
    }

    /// 前方集計値の内訳を計上する。
    pub(crate) fn record_forward_metrics_phases(
        &mut self,
        durations: ForwardMetricsPhaseDurations,
    ) {
        if let Some(state) = self.state.as_mut() {
            state.durations.forward_metrics_phases = durations;
        }
    }

    /// 2向聴 ExpectedSelfTsumoValue の候補別計測器。外側が有効な場合だけ時計を有効にする。
    pub(crate) fn two_shanten_self_tsumo_timer(&self) -> TwoShantenSelfTsumoTimer {
        match self.state {
            Some(_) => TwoShantenSelfTsumoTimer::started(),
            None => TwoShantenSelfTsumoTimer::disabled(),
        }
    }
}

impl<D: PhaseDurations> PhaseTimer<D, NormalDiscardBreakdown> {
    /// 可変長の内訳は scalar duration DTO とは別に保持する。
    pub(crate) fn record_two_shanten_self_tsumo_candidates(
        &mut self,
        candidates: Vec<TwoShantenSelfTsumoCandidateDuration>,
    ) {
        if self.state.is_some() {
            self.breakdown.two_shanten_self_tsumo_candidates = candidates;
        }
    }

    /// 1向聴の深い前方評価を実際に行った候補の実測。production の候補順そのままで受け取る。
    pub(crate) fn record_iishanten_forward_candidates(
        &mut self,
        candidates: Vec<IishantenForwardCandidateDuration>,
    ) {
        if self.state.is_some() {
            self.breakdown.iishanten_forward_candidates = candidates;
        }
    }

    pub(crate) fn take_two_shanten_self_tsumo_candidates(
        &mut self,
    ) -> Vec<TwoShantenSelfTsumoCandidateDuration> {
        std::mem::take(&mut self.breakdown.two_shanten_self_tsumo_candidates)
    }

    pub(crate) fn take_iishanten_forward_candidates(
        &mut self,
    ) -> Vec<IishantenForwardCandidateDuration> {
        std::mem::take(&mut self.breakdown.iishanten_forward_candidates)
    }

    /// 可変長の内訳をまとめて外側の計測器へ渡す。
    pub(crate) fn take_breakdown(&mut self) -> NormalDiscardBreakdown {
        std::mem::take(&mut self.breakdown)
    }
}

impl ForwardMetricsPhaseTimer {
    pub(crate) fn disabled() -> Self {
        Self { state: None }
    }

    /// 最初の区切りまで計上を始めない計測器。区切りを1つも通らなかった経路では、どの phase も
    /// `Duration::ZERO` のままになる。
    pub(crate) fn armed() -> Self {
        Self {
            state: Some(ForwardMetricsTimerState {
                phases: ForwardMetricsPhaseDurations::default(),
                candidates: Vec::new(),
                measures_candidates: false,
                current: None,
                since: Instant::now(),
                phase: None,
            }),
        }
    }

    /// 候補単位の実測を残すかを決める。1向聴の深い前方評価だけを対象にするための入口。
    pub(crate) fn measuring_candidates(mut self, measures: bool) -> Self {
        if let Some(state) = self.state.as_mut() {
            state.measures_candidates = measures;
        }
        self
    }

    /// 候補1件分の実測を残すか。`false` の計測器へ渡す run は候補単位の `Instant` を取らない。
    pub(crate) fn measures_candidates(&self) -> bool {
        self.state
            .as_ref()
            .is_some_and(|state| state.measures_candidates)
    }

    /// 逐次の区切りでは表せない、別 thread で計り終えた候補1件分の実測を反映する。
    ///
    /// 呼ぶ順が候補順。phase の内訳はそのまま `phases` へも足し合わせるので、候補単位で並行に
    /// 評価した局面でも内訳が 0 のままにならない。
    pub(crate) fn record_candidate(&mut self, candidate: IishantenForwardCandidateDuration) {
        if let Some(state) = self.state.as_mut() {
            state.phases.lookahead_search += candidate.phases.lookahead_search;
            state.phases.weighted_aggregation += candidate.phases.weighted_aggregation;
            state.phases.self_tsumo_continuation += candidate.phases.self_tsumo_continuation;
            if state.measures_candidates {
                state.candidates.push(candidate);
            }
        }
    }

    pub(crate) fn take_candidates(&mut self) -> Vec<IishantenForwardCandidateDuration> {
        self.state
            .as_mut()
            .map_or_else(Vec::new, |state| std::mem::take(&mut state.candidates))
    }

    pub(crate) fn finish(mut self) -> ForwardMetricsPhaseDurations {
        match self.state.take() {
            Some(mut state) => {
                state.close(Instant::now());
                state.phases
            }
            None => ForwardMetricsPhaseDurations::default(),
        }
    }
}

/// 前方集計値の区切りをそのまま実測へ変える。計測が無効な場合は `Instant` を取得しない。
impl ForwardMetricsObserver for ForwardMetricsPhaseTimer {
    fn enter_phase(&mut self, phase: ForwardMetricsPhase) {
        if let Some(state) = self.state.as_mut() {
            state.flush(Instant::now());
            state.phase = Some(phase);
        }
    }

    fn enter_candidate(&mut self, discard: TileType, memo: SearchStateMemoStats) {
        if let Some(state) = self.state.as_mut() {
            let now = Instant::now();
            // 直前の候補はこの区切りで閉じる。候補1件分の利用数は、その候補へ入った時点の
            // 累計との差になる。
            state.close_with_memo(now, memo);
            state.phase = None;
            if state.measures_candidates {
                state.current = Some(OpenForwardCandidate {
                    discard,
                    started: now,
                    phases: ForwardMetricsPhaseDurations::default(),
                    search_state_memo: memo,
                    tenpai_value_memo: tenpai_value_memo_counter::counts(),
                });
            }
        }
    }

    fn exit_candidates(&mut self, memo: SearchStateMemoStats) {
        if let Some(state) = self.state.as_mut() {
            state.close_with_memo(Instant::now(), memo);
            state.phase = None;
        }
    }
}

impl ForwardMetricsTimerState {
    // 直前の区切りからの経過時間を、その phase と開いている候補の両方へ計上する。
    fn flush(&mut self, now: Instant) {
        let elapsed = now.duration_since(self.since);
        self.since = now;
        let Some(phase) = self.phase else {
            return;
        };
        self.phases.accumulate(phase, elapsed);
        if let Some(current) = self.current.as_mut() {
            current.phases.accumulate(phase, elapsed);
        }
    }

    fn close(&mut self, now: Instant) {
        let memo = self
            .current
            .as_ref()
            .map(|current| current.search_state_memo)
            .unwrap_or_default();
        self.close_with_memo(now, memo);
    }

    // 開いている候補を閉じる。仕事量は候補へ入った時点との差で、累計そのものは載せない。
    fn close_with_memo(&mut self, now: Instant, memo: SearchStateMemoStats) {
        self.flush(now);
        let Some(current) = self.current.take() else {
            return;
        };
        let (hits, misses) = tenpai_value_memo_counter::counts();
        self.candidates.push(IishantenForwardCandidateDuration {
            discard: current.discard,
            elapsed: now.duration_since(current.started),
            phases: current.phases,
            search_state_memo: memo_stats_delta(memo, current.search_state_memo),
            tenpai_value_memo_hits: hits.saturating_sub(current.tenpai_value_memo.0),
            tenpai_value_memo_misses: misses.saturating_sub(current.tenpai_value_memo.1),
        });
    }
}

// 同一 state memo の利用数の差分。field を分解して受けるため、計上が増えたら引き忘れが
// compile error になる。
pub(crate) fn memo_stats_delta(
    after: SearchStateMemoStats,
    before: SearchStateMemoStats,
) -> SearchStateMemoStats {
    let SearchStateMemoStats {
        two_shanten_hits,
        two_shanten_misses,
        iishanten_hits,
        iishanten_misses,
        next_discard_hits,
        next_discard_misses,
        same_shanten_next_discard_hits,
        same_shanten_next_discard_misses,
    } = before;
    SearchStateMemoStats {
        two_shanten_hits: after.two_shanten_hits.saturating_sub(two_shanten_hits),
        two_shanten_misses: after.two_shanten_misses.saturating_sub(two_shanten_misses),
        iishanten_hits: after.iishanten_hits.saturating_sub(iishanten_hits),
        iishanten_misses: after.iishanten_misses.saturating_sub(iishanten_misses),
        next_discard_hits: after.next_discard_hits.saturating_sub(next_discard_hits),
        next_discard_misses: after
            .next_discard_misses
            .saturating_sub(next_discard_misses),
        same_shanten_next_discard_hits: after
            .same_shanten_next_discard_hits
            .saturating_sub(same_shanten_next_discard_hits),
        same_shanten_next_discard_misses: after
            .same_shanten_next_discard_misses
            .saturating_sub(same_shanten_next_discard_misses),
    }
}

impl TwoShantenSelfTsumoTimer {
    pub(crate) fn disabled() -> Self {
        Self { state: None }
    }

    pub(crate) fn started() -> Self {
        Self {
            state: Some(TwoShantenSelfTsumoTimerState::default()),
        }
    }

    pub(crate) fn finish(mut self) -> Vec<TwoShantenSelfTsumoCandidateDuration> {
        if let Some(state) = self.state.as_mut() {
            state.flush_at(Instant::now());
        }
        self.state.map_or_else(Vec::new, |state| state.elapsed)
    }
}

impl TwoShantenSelfTsumoObserver for TwoShantenSelfTsumoTimer {
    fn enter_candidate(&mut self, discard: TileType) {
        if let Some(state) = self.state.as_mut() {
            let now = Instant::now();
            state.flush_at(now);
            state.current = Some((discard, now));
        }
    }
}

/// 逐次の候補境界では表せない、既に計測済みの候補1件分の実測を受け取る観測器。
///
/// bot-logic の [`TwoShantenSelfTsumoObserver`] は「次の候補へ入った」という境界だけを受け取る
/// ため、同時に評価した候補の内訳を表せない。並行に評価する側は worker ごとに elapsed を独立
/// して計り、join したあとに候補 index 順でこの入口から反映する。bot-logic 側の純粋な評価は
/// この trait を知らず、thread 前提にもならない。
pub(crate) trait TwoShantenFullSelfTsumoObserver: TwoShantenSelfTsumoObserver {
    /// 候補ごとの実測を受け取るか。`false` の観測器へ渡す run は `Instant` を一切取らない。
    fn measures_candidates(&self) -> bool;

    /// 直前に入った候補の区切りを、いま閉じる。並行評価へ入る前に呼び、worker の待ち時間が
    /// 直前の候補の実測へ入らないようにする。
    fn close_candidate(&mut self);

    /// 別 thread で計り終えた候補1件分の実測を反映する。呼ぶ順が候補 index 順。
    fn record_candidate(&mut self, discard: TileType, elapsed: Duration);
}

/// 計測しない経路。境界と同じく何も持たない。
impl TwoShantenFullSelfTsumoObserver for () {
    fn measures_candidates(&self) -> bool {
        false
    }

    fn close_candidate(&mut self) {}

    fn record_candidate(&mut self, _discard: TileType, _elapsed: Duration) {}
}

impl TwoShantenFullSelfTsumoObserver for TwoShantenSelfTsumoTimer {
    fn measures_candidates(&self) -> bool {
        self.state.is_some()
    }

    fn close_candidate(&mut self) {
        if let Some(state) = self.state.as_mut() {
            state.flush_at(Instant::now());
        }
    }

    fn record_candidate(&mut self, discard: TileType, elapsed: Duration) {
        if let Some(state) = self.state.as_mut() {
            state
                .elapsed
                .push(TwoShantenSelfTsumoCandidateDuration { discard, elapsed });
        }
    }
}

impl TwoShantenSelfTsumoTimerState {
    fn flush_at(&mut self, now: Instant) {
        if let Some((discard, since)) = self.current.take() {
            self.elapsed.push(TwoShantenSelfTsumoCandidateDuration {
                discard,
                elapsed: now.duration_since(since),
            });
        }
    }
}

impl<D: PhaseDurations> TimerState<D> {
    fn flush(&mut self) {
        let now = Instant::now();
        let elapsed = now.duration_since(self.since);
        self.since = now;
        if let Some(phase) = self.phase {
            self.durations.accumulate(phase, elapsed);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn public_duration_dtos_remain_copy() {
        fn assert_copy<T: Copy>() {}
        assert_copy::<DecisionPhaseDurations>();
        assert_copy::<NormalDiscardPhaseDurations>();
    }

    #[test]
    fn candidate_breakdown_is_separate_from_scalar_phases() {
        let mut decision = DecisionPhaseTimer::started();
        let mut normal = decision.normal_discard_timer();
        let mut candidates = normal.two_shanten_self_tsumo_timer();
        let discard = TileType::from_mjai_type_str("5m").unwrap();
        candidates.enter_candidate(discard);
        normal.record_two_shanten_self_tsumo_candidates(candidates.finish());

        // 1向聴の深い前方評価は、逐次評価では候補の区切りをそのまま実測へ変える。
        let forward_discard = TileType::from_mjai_type_str("3p").unwrap();
        let mut forward = normal.forward_metrics_timer().measuring_candidates(true);
        forward.enter_candidate(forward_discard, SearchStateMemoStats::default());
        forward.enter_phase(ForwardMetricsPhase::LookaheadSearch);
        forward.exit_candidates(SearchStateMemoStats::default());
        normal.record_iishanten_forward_candidates(forward.take_candidates());
        normal.record_forward_metrics_phases(forward.finish());

        decision.record_normal_discard_breakdown(normal.take_breakdown());
        decision.record_normal_discard_phases(normal.finish());
        let two_shanten_self_tsumo_candidates = decision.take_two_shanten_self_tsumo_candidates();
        let iishanten_forward_candidates = decision.take_iishanten_forward_candidates();
        let timed = TimedAgentAction {
            action: LegalAction::None,
            phases: decision.finish(),
            two_shanten_self_tsumo_candidates,
            iishanten_forward_candidates,
            call_candidates: Vec::new(),
        };
        let phases = timed.phases;
        assert_eq!(phases, timed.phases);
        assert_eq!(timed.two_shanten_self_tsumo_candidates().len(), 1);
        assert_eq!(
            timed.two_shanten_self_tsumo_candidates().next().unwrap().0,
            discard
        );
        // 向聴数別の候補列は混ざらず、それぞれの区切りだけを持つ。
        assert_eq!(timed.iishanten_forward_candidates().len(), 1);
        let forward_candidate = &timed.iishanten_forward_candidates()[0];
        assert_eq!(forward_candidate.discard, forward_discard);
        // 候補の内訳は、その候補を評価していた実時間を超えない。
        assert!(forward_candidate.phases.total() <= forward_candidate.elapsed);
        assert_eq!(
            phases.normal_discard_phases.forward_metrics_phases,
            forward_candidate.phases
        );
    }

    #[test]
    fn a_forward_timer_without_candidate_measurement_keeps_only_the_phases() {
        // 最善向聴数が1向聴でない局面では候補の区切りを受けても列へ積まない。phase 別の内訳は
        // 従来どおり計上する。
        let normal = NormalDiscardPhaseTimer::started();
        let mut forward = normal.forward_metrics_timer();
        assert!(!forward.measures_candidates());
        forward.enter_candidate(
            TileType::from_mjai_type_str("1m").unwrap(),
            SearchStateMemoStats::default(),
        );
        forward.enter_phase(ForwardMetricsPhase::WeightedAggregation);
        forward.exit_candidates(SearchStateMemoStats::default());

        assert!(forward.take_candidates().is_empty());
        let phases = forward.finish();
        assert!(phases.weighted_aggregation > Duration::ZERO);
        assert_eq!(phases.lookahead_search, Duration::ZERO);
    }

    #[test]
    fn a_disabled_forward_timer_keeps_the_parallel_candidate_measurements_out() {
        // 計測しない run へ worker 側の実測を渡しても何も残さない。
        let mut forward = ForwardMetricsPhaseTimer::disabled();
        assert!(!forward.measures_candidates());
        forward.record_candidate(IishantenForwardCandidateDuration {
            discard: TileType::from_mjai_type_str("1m").unwrap(),
            elapsed: Duration::from_millis(1),
            phases: ForwardMetricsPhaseDurations {
                lookahead_search: Duration::from_millis(1),
                ..ForwardMetricsPhaseDurations::default()
            },
            search_state_memo: SearchStateMemoStats::default(),
            tenpai_value_memo_hits: 0,
            tenpai_value_memo_misses: 0,
        });

        assert!(forward.take_candidates().is_empty());
        assert_eq!(forward.finish(), ForwardMetricsPhaseDurations::default());
    }

    #[test]
    fn the_parallel_candidate_measurements_add_up_to_the_forward_phases() {
        // 並列評価では worker 側で計り終えた内訳をそのまま足し合わせる。候補の実測の合計が
        // phase の壁時計を超えても、内訳は候補ごとの実時間のまま書き換えない。
        let normal = NormalDiscardPhaseTimer::started();
        let mut forward = normal.forward_metrics_timer().measuring_candidates(true);
        let candidates = ["1m", "2m"].map(|discard| IishantenForwardCandidateDuration {
            discard: TileType::from_mjai_type_str(discard).unwrap(),
            elapsed: Duration::from_millis(10),
            phases: ForwardMetricsPhaseDurations {
                lookahead_search: Duration::from_millis(9),
                weighted_aggregation: Duration::from_millis(1),
                self_tsumo_continuation: Duration::ZERO,
            },
            search_state_memo: SearchStateMemoStats::default(),
            tenpai_value_memo_hits: 3,
            tenpai_value_memo_misses: 1,
        });
        for candidate in candidates {
            forward.record_candidate(candidate);
        }

        assert_eq!(forward.take_candidates(), candidates.to_vec());
        let phases = forward.finish();
        assert_eq!(phases.lookahead_search, Duration::from_millis(18));
        assert_eq!(phases.weighted_aggregation, Duration::from_millis(2));
    }

    #[test]
    fn disabled_timers_do_not_keep_candidate_breakdowns() {
        let candidate = TwoShantenSelfTsumoCandidateDuration {
            discard: TileType::from_mjai_type_str("5m").unwrap(),
            elapsed: Duration::ZERO,
        };
        let mut normal = NormalDiscardPhaseTimer::disabled();
        normal.record_two_shanten_self_tsumo_candidates(vec![candidate]);
        assert!(normal.take_two_shanten_self_tsumo_candidates().is_empty());
        let mut decision = DecisionPhaseTimer::disabled();
        decision.record_normal_discard_breakdown(NormalDiscardBreakdown {
            two_shanten_self_tsumo_candidates: vec![candidate],
            iishanten_forward_candidates: Vec::new(),
        });
        assert!(decision.take_two_shanten_self_tsumo_candidates().is_empty());
    }

    #[test]
    fn an_armed_call_timer_without_candidates_measures_nothing() {
        // 合法な Chi / Pon が無い request では候補の区切りを1つも通らない。
        let (durations, candidates) = CallDecisionTimer::armed().finish();

        assert_eq!(durations, CallDecisionDurations::default());
        assert!(candidates.is_empty());
    }

    #[test]
    fn a_disabled_timer_hands_out_a_disabled_call_timer() {
        let timer = DecisionPhaseTimer::disabled();
        let mut call = timer.call_timer();
        let mut candidate = call.candidate_timer();
        let measured = candidate.measure_post_call_discard_selection(|| 1);
        call.record_candidate(
            CallKind::Pon,
            TileId::new(0).unwrap(),
            &[TileId::new(1).unwrap(), TileId::new(2).unwrap()],
            candidate.finish(),
        );
        let pass = call.measure_pass_iishanten_self_tsumo(|| 2);
        let (durations, candidates) = call.finish();

        // 計測が無効でも評価そのものは同じように通す。
        assert_eq!(measured, 1);
        assert_eq!(pass, 2);
        assert_eq!(durations, CallDecisionDurations::default());
        assert!(candidates.is_empty());
    }

    #[test]
    fn the_recorded_call_candidates_are_kept_as_the_breakdown() {
        let mut timer = DecisionPhaseTimer::started();
        let mut call = timer.call_timer();
        let tiles = [TileId::new(4).unwrap(), TileId::new(8).unwrap()];
        for _ in 0..2 {
            let candidate = call.candidate_timer();
            call.record_candidate(
                CallKind::Chi,
                TileId::new(0).unwrap(),
                &tiles,
                candidate.finish(),
            );
        }
        let (durations, candidates) = call.finish();
        timer.record_call(durations, candidates);
        let recorded = timer.take_call_candidates();
        let phases = timer.finish();

        // 重複候補も行をまとめず、記録した順にそのまま2件並ぶ。
        assert_eq!(recorded.len(), 2);
        assert!(recorded.iter().all(
            |candidate| candidate.kind == CallKind::Chi && candidate.consumed == tiles.to_vec()
        ));
        assert!(recorded.iter().all(|candidate| !candidate.reused));
        assert_eq!(phases.call.candidates, durations.candidates);
        assert_eq!(phases.call.pass_iishanten_self_tsumo, Duration::ZERO);
        assert_eq!(
            phases.call.remaining(),
            phases.call.total - durations.candidates
        );
    }

    #[test]
    fn a_reused_call_candidate_is_recorded_without_any_evaluation_time() {
        let mut timer = DecisionPhaseTimer::started();
        let mut call = timer.call_timer();
        let tiles = [TileId::new(4).unwrap(), TileId::new(8).unwrap()];
        let candidate = call.candidate_timer();
        call.record_candidate(
            CallKind::Chi,
            TileId::new(0).unwrap(),
            &tiles,
            candidate.finish(),
        );
        call.record_reused_candidate(CallKind::Chi, TileId::new(0).unwrap(), &tiles);
        let (durations, candidates) = call.finish();
        timer.record_call(durations, candidates);
        let recorded = timer.take_call_candidates();

        // 候補そのものは残すが、評価していないので実測は 0 で合計にも入らない。
        assert_eq!(recorded.len(), 2);
        assert!(!recorded[0].reused);
        assert!(recorded[1].reused);
        assert_eq!(recorded[1].elapsed, Duration::ZERO);
        assert_eq!(recorded[1].post_call_discard_selection, Duration::ZERO);
        assert_eq!(durations.candidates, recorded[0].elapsed);
    }

    #[test]
    fn the_call_breakdown_is_taken_only_once() {
        let mut timer = DecisionPhaseTimer::started();
        timer.record_call(
            CallDecisionDurations::default(),
            vec![CallCandidateDuration {
                kind: CallKind::Pon,
                tile: TileId::new(0).unwrap(),
                consumed: vec![TileId::new(1).unwrap(), TileId::new(2).unwrap()],
                elapsed: Duration::from_millis(1),
                post_call_discard_selection: Duration::from_millis(1),
                reused: false,
            }],
        );

        assert_eq!(timer.take_call_candidates().len(), 1);
        assert!(timer.take_call_candidates().is_empty());
    }

    #[test]
    fn a_disabled_timer_measures_nothing() {
        let mut timer = DecisionPhaseTimer::disabled();
        timer.enter(DecisionPhase::NormalDiscard);
        timer.enter(DecisionPhase::PostDiscard);

        assert_eq!(timer.finish(), DecisionPhaseDurations::default());
    }

    #[test]
    fn only_the_entered_phases_are_accounted() {
        let mut timer = DecisionPhaseTimer::started();
        timer.enter(DecisionPhase::NormalDiscard);
        let durations = timer.finish();

        assert_eq!(durations.post_discard, Duration::ZERO);
        assert_eq!(
            durations.total(),
            durations.early + durations.normal_discard
        );
    }

    #[test]
    fn phases_not_reached_stay_zero() {
        let durations = DecisionPhaseTimer::started().finish();

        assert_eq!(durations.normal_discard, Duration::ZERO);
        assert_eq!(durations.post_discard, Duration::ZERO);
        assert_eq!(
            durations.normal_discard_phases,
            NormalDiscardPhaseDurations::default()
        );
    }

    #[test]
    fn a_disabled_timer_hands_out_a_disabled_normal_discard_timer() {
        let timer = DecisionPhaseTimer::disabled();
        let mut normal_discard = timer.normal_discard_timer();
        normal_discard.enter(NormalDiscardPhase::ForwardMetrics);
        normal_discard.enter(NormalDiscardPhase::SelectionFinalize);

        assert_eq!(
            normal_discard.finish(),
            NormalDiscardPhaseDurations::default()
        );
    }

    #[test]
    fn a_disabled_timer_keeps_the_recorded_normal_discard_phases_at_zero() {
        let mut timer = DecisionPhaseTimer::disabled();
        timer.record_normal_discard_phases(NormalDiscardPhaseDurations {
            base_evaluation: Duration::from_millis(1),
            forward_metrics: Duration::from_millis(2),
            selection_finalize: Duration::from_millis(3),
            ..NormalDiscardPhaseDurations::default()
        });

        assert_eq!(timer.finish(), DecisionPhaseDurations::default());
    }

    #[test]
    fn an_armed_timer_measures_nothing_until_the_first_phase() {
        // 区切りを1つも通らない経路では、どの phase も 0 のままになる。
        let timer = ForwardMetricsPhaseTimer::armed();

        assert_eq!(timer.finish(), ForwardMetricsPhaseDurations::default());
    }

    #[test]
    fn an_armed_timer_accounts_only_from_the_first_phase() {
        let mut timer = ForwardMetricsPhaseTimer::armed();
        timer.enter_phase(ForwardMetricsPhase::WeightedAggregation);
        let durations = timer.finish();

        assert_eq!(durations.lookahead_search, Duration::ZERO);
        assert_eq!(durations.self_tsumo_continuation, Duration::ZERO);
        assert_eq!(durations.total(), durations.weighted_aggregation);
    }

    #[test]
    fn a_disabled_timer_hands_out_a_disabled_forward_metrics_timer() {
        let timer = NormalDiscardPhaseTimer::disabled();
        let mut forward_metrics = timer.forward_metrics_timer();
        forward_metrics.enter_phase(ForwardMetricsPhase::LookaheadSearch);
        forward_metrics.enter_phase(ForwardMetricsPhase::WeightedAggregation);

        assert_eq!(
            forward_metrics.finish(),
            ForwardMetricsPhaseDurations::default()
        );
    }

    #[test]
    fn a_disabled_timer_hands_out_a_disabled_two_shanten_timer() {
        let timer = NormalDiscardPhaseTimer::disabled();
        let mut two_shanten = timer.two_shanten_self_tsumo_timer();
        two_shanten.enter_candidate(TileType::from_mjai_type_str("1m").unwrap());

        assert!(two_shanten.finish().is_empty());
    }

    #[test]
    fn two_shanten_candidate_boundaries_are_recorded_in_order() {
        let mut timer = NormalDiscardPhaseTimer::started();
        let mut two_shanten = timer.two_shanten_self_tsumo_timer();
        let one_man = TileType::from_mjai_type_str("1m").unwrap();
        let two_man = TileType::from_mjai_type_str("2m").unwrap();
        two_shanten.enter_candidate(one_man);
        two_shanten.enter_candidate(two_man);
        let candidates = two_shanten.finish();
        timer.record_two_shanten_self_tsumo_candidates(candidates.clone());

        assert_eq!(
            candidates
                .iter()
                .map(|candidate| candidate.discard)
                .collect::<Vec<_>>(),
            vec![one_man, two_man]
        );
        assert_eq!(timer.take_two_shanten_self_tsumo_candidates(), candidates);
        assert!(timer.take_two_shanten_self_tsumo_candidates().is_empty());
    }

    #[test]
    fn a_disabled_timer_keeps_the_recorded_forward_metrics_phases_at_zero() {
        let mut timer = NormalDiscardPhaseTimer::disabled();
        timer.record_forward_metrics_phases(ForwardMetricsPhaseDurations {
            lookahead_search: Duration::from_millis(1),
            weighted_aggregation: Duration::from_millis(2),
            self_tsumo_continuation: Duration::from_millis(3),
        });

        assert_eq!(timer.finish(), NormalDiscardPhaseDurations::default());
    }

    #[test]
    fn the_recorded_forward_metrics_phases_are_kept_as_the_breakdown() {
        let mut timer = NormalDiscardPhaseTimer::started();
        let mut forward_metrics = timer.forward_metrics_timer();
        forward_metrics.enter_phase(ForwardMetricsPhase::LookaheadSearch);
        let breakdown = forward_metrics.finish();
        timer.record_forward_metrics_phases(breakdown);
        let durations = timer.finish();

        assert_eq!(durations.forward_metrics_phases, breakdown);
        assert_eq!(
            durations.forward_metrics_phases.weighted_aggregation,
            Duration::ZERO
        );
    }

    #[test]
    fn the_recorded_normal_discard_phases_are_kept_as_the_breakdown() {
        let mut timer = DecisionPhaseTimer::started();
        let mut normal_discard = timer.normal_discard_timer();
        normal_discard.enter(NormalDiscardPhase::ForwardMetrics);
        let breakdown = normal_discard.finish();
        timer.record_normal_discard_phases(breakdown);
        let durations = timer.finish();

        assert_eq!(durations.normal_discard_phases, breakdown);
        assert_eq!(
            durations.normal_discard_phases.selection_finalize,
            Duration::ZERO
        );
    }
}
