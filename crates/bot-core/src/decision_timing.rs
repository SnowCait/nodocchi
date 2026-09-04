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
    /// 通常打牌選択より後の押し引き / Reach / 防御 / 最終 action 選択。
    pub post_discard: Duration,
}

impl DecisionPhaseDurations {
    pub fn total(&self) -> Duration {
        self.early + self.normal_discard + self.post_discard
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

/// production path へ差し込む optional な phase 計測器。
///
/// 無効時は `Instant` を一切取得せず、判断内容にも影響しない。判断を再実行することは
/// なく、通った経路の経過時間をその場で計上するだけ。
#[derive(Debug, Default)]
pub(crate) struct DecisionPhaseTimer {
    state: Option<TimerState>,
}

#[derive(Debug)]
struct TimerState {
    phase: DecisionPhase,
    since: Instant,
    durations: DecisionPhaseDurations,
}

impl DecisionPhaseTimer {
    pub(crate) fn disabled() -> Self {
        Self::default()
    }

    pub(crate) fn started() -> Self {
        Self {
            state: Some(TimerState {
                phase: DecisionPhase::Early,
                since: Instant::now(),
                durations: DecisionPhaseDurations::default(),
            }),
        }
    }

    /// 現在の phase へ経過時間を計上し、次の phase へ進める。
    pub(crate) fn enter(&mut self, phase: DecisionPhase) {
        if let Some(state) = self.state.as_mut() {
            state.flush();
            state.phase = phase;
        }
    }

    /// 最後の phase へ経過時間を計上して結果を返す。早期 return した局面では、
    /// その時点の phase へそのまま計上される。
    pub(crate) fn finish(mut self) -> DecisionPhaseDurations {
        match self.state.as_mut() {
            Some(state) => {
                state.flush();
                state.durations
            }
            None => DecisionPhaseDurations::default(),
        }
    }
}

impl TimerState {
    fn flush(&mut self) {
        let now = Instant::now();
        let elapsed = now.duration_since(self.since);
        self.since = now;
        match self.phase {
            DecisionPhase::Early => self.durations.early += elapsed,
            DecisionPhase::NormalDiscard => self.durations.normal_discard += elapsed,
            DecisionPhase::PostDiscard => self.durations.post_discard += elapsed,
        }
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
    }
}
