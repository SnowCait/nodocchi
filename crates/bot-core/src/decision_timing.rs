use std::fmt::Debug;
use std::time::{Duration, Instant};

use bot_logic::{
    ForwardMetricsObserver, ForwardMetricsPhase, TileType, TwoShantenSelfTsumoObserver,
};

use crate::action::LegalAction;

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
}

impl DecisionPhaseDurations {
    pub fn total(&self) -> Duration {
        self.early + self.normal_discard + self.post_discard
    }
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
    /// production comparator が追加で評価する2向聴 ExpectedSelfTsumoValue。
    /// 対象外の局面では `Duration::ZERO` のままになる。
    pub two_shanten_self_tsumo: Duration,
    /// 残りの補助評価 (現在聴牌候補の待ち / 打点 / ツモ期待値) と候補比較・最終打牌の確定。
    pub selection_finalize: Duration,
}

impl NormalDiscardPhaseDurations {
    pub fn total(&self) -> Duration {
        self.base_evaluation
            + self.forward_metrics
            + self.two_shanten_self_tsumo
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

/// production comparator が実際に評価した2向聴 ExpectedSelfTsumoValue 候補1件の実測。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct TwoShantenSelfTsumoCandidateDuration {
    pub discard: TileType,
    pub elapsed: Duration,
}

/// 計測付きで実行した意思決定の最終 action と phase 別実測時間。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TimedAgentAction {
    pub action: LegalAction,
    pub phases: DecisionPhaseDurations,
    pub(crate) two_shanten_self_tsumo_candidates: Vec<TwoShantenSelfTsumoCandidateDuration>,
}

impl TimedAgentAction {
    /// 同じ production execution で実際に評価した `ForwardTargets` の打牌と実測時間。
    /// 対象外の request では空。内部の計測用 representation は公開しない。
    pub fn two_shanten_self_tsumo_candidates(
        &self,
    ) -> impl ExactSizeIterator<Item = (TileType, Duration)> + '_ {
        self.two_shanten_self_tsumo_candidates
            .iter()
            .map(|candidate| (candidate.discard, candidate.elapsed))
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

pub(crate) type DecisionPhaseTimer =
    PhaseTimer<DecisionPhaseDurations, Vec<TwoShantenSelfTsumoCandidateDuration>>;
pub(crate) type NormalDiscardPhaseTimer =
    PhaseTimer<NormalDiscardPhaseDurations, Vec<TwoShantenSelfTsumoCandidateDuration>>;
pub(crate) type ForwardMetricsPhaseTimer = PhaseTimer<ForwardMetricsPhaseDurations>;

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

    /// 最初の `enter()` まで計上を始めない計測器。区切りを通らなかった経路では、どの phase も
    /// `Duration::ZERO` のままになる。
    pub(crate) fn armed() -> Self {
        Self {
            state: Some(TimerState {
                phase: None,
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

impl<D: PhaseDurations> PhaseTimer<D, Vec<TwoShantenSelfTsumoCandidateDuration>> {
    /// 可変長の内訳は scalar duration DTO とは別に保持する。
    pub(crate) fn record_two_shanten_self_tsumo_candidates(
        &mut self,
        candidates: Vec<TwoShantenSelfTsumoCandidateDuration>,
    ) {
        if self.state.is_some() {
            self.breakdown = candidates;
        }
    }

    pub(crate) fn take_two_shanten_self_tsumo_candidates(
        &mut self,
    ) -> Vec<TwoShantenSelfTsumoCandidateDuration> {
        std::mem::take(&mut self.breakdown)
    }
}

/// 前方集計値の区切りをそのまま実測へ変える。計測が無効な場合は `Instant` を取得しない。
impl ForwardMetricsObserver for ForwardMetricsPhaseTimer {
    fn enter_phase(&mut self, phase: ForwardMetricsPhase) {
        self.enter(phase);
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
        decision.record_two_shanten_self_tsumo_candidates(
            normal.take_two_shanten_self_tsumo_candidates(),
        );
        decision.record_normal_discard_phases(normal.finish());
        let two_shanten_self_tsumo_candidates = decision.take_two_shanten_self_tsumo_candidates();
        let timed = TimedAgentAction {
            action: LegalAction::None,
            phases: decision.finish(),
            two_shanten_self_tsumo_candidates,
        };
        let phases = timed.phases;
        assert_eq!(phases, timed.phases);
        assert_eq!(timed.two_shanten_self_tsumo_candidates().len(), 1);
        assert_eq!(
            timed.two_shanten_self_tsumo_candidates().next().unwrap().0,
            discard
        );
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
        decision.record_two_shanten_self_tsumo_candidates(vec![candidate]);
        assert!(decision.take_two_shanten_self_tsumo_candidates().is_empty());
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
        timer.enter(ForwardMetricsPhase::WeightedAggregation);
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
