use std::fmt::Debug;
use std::time::{Duration, Instant};

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
    /// 残りの補助評価 (現在聴牌候補の待ち / 打点 / ツモ期待値) と候補比較・最終打牌の確定。
    pub selection_finalize: Duration,
}

impl NormalDiscardPhaseDurations {
    pub fn total(&self) -> Duration {
        self.base_evaluation + self.forward_metrics + self.selection_finalize
    }
}

/// 計測付きで実行した意思決定の最終 action と phase 別実測時間。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TimedAgentAction {
    pub action: LegalAction,
    pub phases: DecisionPhaseDurations,
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

impl PhaseDurations for NormalDiscardPhaseDurations {
    type Phase = NormalDiscardPhase;

    const FIRST: Self::Phase = NormalDiscardPhase::BaseEvaluation;

    fn accumulate(&mut self, phase: Self::Phase, elapsed: Duration) {
        match phase {
            NormalDiscardPhase::BaseEvaluation => self.base_evaluation += elapsed,
            NormalDiscardPhase::ForwardMetrics => self.forward_metrics += elapsed,
            NormalDiscardPhase::SelectionFinalize => self.selection_finalize += elapsed,
        }
    }
}

/// production path へ差し込む optional な phase 計測器。
///
/// 無効時は `Instant` を一切取得せず、判断内容にも影響しない。判断を再実行することは
/// なく、通った経路の経過時間をその場で計上するだけ。
#[derive(Debug)]
pub(crate) struct PhaseTimer<D: PhaseDurations> {
    state: Option<TimerState<D>>,
}

pub(crate) type DecisionPhaseTimer = PhaseTimer<DecisionPhaseDurations>;
pub(crate) type NormalDiscardPhaseTimer = PhaseTimer<NormalDiscardPhaseDurations>;

#[derive(Debug)]
struct TimerState<D: PhaseDurations> {
    phase: D::Phase,
    since: Instant,
    durations: D,
}

impl<D: PhaseDurations> PhaseTimer<D> {
    pub(crate) fn disabled() -> Self {
        Self { state: None }
    }

    pub(crate) fn started() -> Self {
        Self {
            state: Some(TimerState {
                phase: D::FIRST,
                since: Instant::now(),
                durations: D::default(),
            }),
        }
    }

    /// 現在の phase へ経過時間を計上し、次の phase へ進める。
    pub(crate) fn enter(&mut self, phase: D::Phase) {
        if let Some(state) = self.state.as_mut() {
            state.flush();
            state.phase = phase;
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

impl<D: PhaseDurations> TimerState<D> {
    fn flush(&mut self) {
        let now = Instant::now();
        let elapsed = now.duration_since(self.since);
        self.since = now;
        self.durations.accumulate(self.phase, elapsed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
        });

        assert_eq!(timer.finish(), DecisionPhaseDurations::default());
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
