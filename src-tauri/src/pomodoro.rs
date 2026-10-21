use serde::{Deserialize, Serialize};
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct PomodoroConfig {
    pub focus_min: u64,
    pub short_break_min: u64,
    pub long_break_min: u64,
    /// A long break replaces the short one after this many focus sessions.
    pub long_break_every: u32,
    /// Start the next phase automatically instead of waiting for the user.
    pub auto_start: bool,
}

impl Default for PomodoroConfig {
    fn default() -> Self {
        Self { focus_min: 25, short_break_min: 5, long_break_min: 15, long_break_every: 4, auto_start: false }
    }
}

/// Allowed lengths in minutes (inclusive). The settings UI uses the same bounds.
pub const FOCUS_MIN_RANGE: (u64, u64) = (1, 180);
pub const SHORT_BREAK_MIN_RANGE: (u64, u64) = (1, 60);
pub const LONG_BREAK_MIN_RANGE: (u64, u64) = (1, 90);
pub const LONG_BREAK_EVERY_MAX: u32 = 12;

impl PomodoroConfig {
    /// Keeps every length inside its bounds, whatever a hand-edited config file or the UI sends.
    pub fn clamped(self) -> Self {
        let c = |v: u64, (lo, hi): (u64, u64)| v.clamp(lo, hi);
        Self {
            focus_min: c(self.focus_min, FOCUS_MIN_RANGE),
            short_break_min: c(self.short_break_min, SHORT_BREAK_MIN_RANGE),
            long_break_min: c(self.long_break_min, LONG_BREAK_MIN_RANGE),
            long_break_every: self.long_break_every.min(LONG_BREAK_EVERY_MAX),
            auto_start: self.auto_start,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    Focus,
    ShortBreak,
    LongBreak,
}

#[derive(Debug, Serialize)]
pub struct Snapshot {
    pub phase: Phase,
    pub running: bool,
    pub remaining_secs: u64,
    pub total_secs: u64,
    /// Focus sessions finished since the app started.
    pub completed_focus: u32,
    /// Phase that ended during this call, so the UI can alert once.
    pub finished: Option<Phase>,
    /// The configured lengths in minutes (they apply from the next phase if the current one had started).
    pub focus_min: u64,
    pub short_break_min: u64,
    pub long_break_min: u64,
}

/// Timer state machine. Time is passed in rather than read, so it is testable
/// and keeps counting correctly while the window is hidden or reloaded.
pub struct Pomodoro {
    cfg: PomodoroConfig,
    phase: Phase,
    completed_focus: u32,
    remaining: Duration,
    deadline: Option<Instant>,
    /// Length of the current phase, fixed when it began (a settings change doesn't stretch a started phase).
    total: Duration,
}

impl Pomodoro {
    pub fn new(cfg: PomodoroConfig) -> Self {
        let cfg = cfg.clamped();
        let total = Self::length(&cfg, Phase::Focus);
        Self { cfg, phase: Phase::Focus, completed_focus: 0, remaining: total, deadline: None, total }
    }

    fn length(cfg: &PomodoroConfig, phase: Phase) -> Duration {
        let min = match phase {
            Phase::Focus => cfg.focus_min,
            Phase::ShortBreak => cfg.short_break_min,
            Phase::LongBreak => cfg.long_break_min,
        };
        Duration::from_secs(min.max(1) * 60)
    }

    /// True while the current phase hasn't been started (or was reset). Under a second of use counts as
    /// untouched, matching the whole seconds the UI shows.
    fn untouched(&self) -> bool {
        self.deadline.is_none() && self.total.saturating_sub(self.remaining) < Duration::from_secs(1)
    }

    /// New lengths apply right away to an untouched timer. A started phase (running or paused)
    /// keeps its time; the new lengths take over from the next phase, or on Reset.
    pub fn set_config(&mut self, cfg: PomodoroConfig) {
        let untouched = self.untouched();
        self.cfg = cfg.clamped();
        if untouched {
            self.reset();
        }
    }

    pub fn start(&mut self, now: Instant) {
        if self.deadline.is_none() {
            self.deadline = Some(now + self.remaining);
        }
    }

    pub fn pause(&mut self, now: Instant) {
        if let Some(d) = self.deadline.take() {
            self.remaining = d.saturating_duration_since(now);
        }
    }

    /// Restart the current phase, stopped.
    pub fn reset(&mut self) {
        self.deadline = None;
        self.total = Self::length(&self.cfg, self.phase);
        self.remaining = self.total;
    }

    /// Jump to the next phase without crediting a finished focus session.
    pub fn skip(&mut self, now: Instant) {
        let was_running = self.deadline.is_some();
        self.enter(self.next_phase(false), now, was_running);
    }

    fn next_phase(&self, credit_focus: bool) -> Phase {
        match self.phase {
            Phase::Focus => {
                let done = self.completed_focus + u32::from(credit_focus);
                if self.cfg.long_break_every > 0 && done > 0 && done % self.cfg.long_break_every == 0 {
                    Phase::LongBreak
                } else {
                    Phase::ShortBreak
                }
            }
            _ => Phase::Focus,
        }
    }

    fn enter(&mut self, phase: Phase, now: Instant, run: bool) {
        self.phase = phase;
        self.total = Self::length(&self.cfg, phase);
        self.remaining = self.total;
        self.deadline = run.then(|| now + self.remaining);
    }

    /// Advance the clock; returns the phase that just ended, if any.
    pub fn tick(&mut self, now: Instant) -> Option<Phase> {
        let deadline = self.deadline?;
        if now < deadline {
            return None;
        }
        let ended = self.phase;
        let next = self.next_phase(true);
        if ended == Phase::Focus {
            self.completed_focus += 1;
        }
        self.enter(next, now, self.cfg.auto_start);
        Some(ended)
    }

    pub fn snapshot(&mut self, now: Instant) -> Snapshot {
        let finished = self.tick(now);
        let remaining = match self.deadline {
            Some(d) => d.saturating_duration_since(now),
            None => self.remaining,
        };
        Snapshot {
            phase: self.phase,
            running: self.deadline.is_some(),
            remaining_secs: remaining.as_secs_f64().ceil() as u64,
            total_secs: self.total.as_secs(),
            completed_focus: self.completed_focus,
            finished,
            focus_min: self.cfg.focus_min,
            short_break_min: self.cfg.short_break_min,
            long_break_min: self.cfg.long_break_min,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MIN: Duration = Duration::from_secs(60);

    fn timer() -> Pomodoro {
        Pomodoro::new(PomodoroConfig::default())
    }

    #[test]
    fn starts_idle_on_a_full_focus_session() {
        let mut p = timer();
        let s = p.snapshot(Instant::now());
        assert_eq!((s.phase, s.running, s.remaining_secs), (Phase::Focus, false, 25 * 60));
    }

    #[test]
    fn counts_down_while_running() {
        let (mut p, t0) = (timer(), Instant::now());
        p.start(t0);
        let s = p.snapshot(t0 + 10 * MIN);
        assert!(s.running);
        assert_eq!(s.remaining_secs, 15 * 60);
    }

    #[test]
    fn pause_freezes_and_resume_continues() {
        let (mut p, t0) = (timer(), Instant::now());
        p.start(t0);
        p.pause(t0 + 5 * MIN);
        assert_eq!(p.snapshot(t0 + 60 * MIN).remaining_secs, 20 * 60);
        p.start(t0 + 60 * MIN);
        assert_eq!(p.snapshot(t0 + 70 * MIN).remaining_secs, 10 * 60);
    }

    #[test]
    fn focus_finishing_moves_to_short_break_and_stops() {
        let (mut p, t0) = (timer(), Instant::now());
        p.start(t0);
        let s = p.snapshot(t0 + 25 * MIN);
        assert_eq!(s.finished, Some(Phase::Focus));
        assert_eq!((s.phase, s.running, s.completed_focus), (Phase::ShortBreak, false, 1));
        assert_eq!(s.remaining_secs, 5 * 60);
        // The alert fires once only.
        assert_eq!(p.snapshot(t0 + 26 * MIN).finished, None);
    }

    #[test]
    fn fourth_focus_session_earns_a_long_break() {
        let (mut p, mut now) = (timer(), Instant::now());
        for i in 1..=4 {
            p.start(now);
            now += 25 * MIN;
            let s = p.snapshot(now);
            if i < 4 {
                assert_eq!(s.phase, Phase::ShortBreak);
                p.start(now);
                now += 5 * MIN;
                assert_eq!(p.snapshot(now).phase, Phase::Focus);
            } else {
                assert_eq!((s.phase, s.remaining_secs), (Phase::LongBreak, 15 * 60));
            }
        }
    }

    #[test]
    fn auto_start_keeps_running_into_the_next_phase() {
        let mut p = Pomodoro::new(PomodoroConfig { auto_start: true, ..Default::default() });
        let t0 = Instant::now();
        p.start(t0);
        let s = p.snapshot(t0 + 25 * MIN);
        assert!(s.running);
        assert_eq!(s.phase, Phase::ShortBreak);
    }

    #[test]
    fn skip_does_not_credit_a_session() {
        let (mut p, t0) = (timer(), Instant::now());
        p.skip(t0);
        let s = p.snapshot(t0);
        assert_eq!((s.phase, s.completed_focus), (Phase::ShortBreak, 0));
    }

    #[test]
    fn skip_while_running_stays_running() {
        let (mut p, t0) = (timer(), Instant::now());
        p.start(t0);
        p.skip(t0 + MIN);
        assert!(p.snapshot(t0 + MIN).running);
    }

    #[test]
    fn reset_restores_full_phase_and_stops() {
        let (mut p, t0) = (timer(), Instant::now());
        p.start(t0);
        p.reset();
        let s = p.snapshot(t0 + 3 * MIN);
        assert_eq!((s.running, s.remaining_secs), (false, 25 * 60));
    }

    #[test]
    fn zero_minute_config_is_clamped() {
        let mut p = Pomodoro::new(PomodoroConfig { focus_min: 0, ..Default::default() });
        assert_eq!(p.snapshot(Instant::now()).remaining_secs, 60);
    }

    #[test]
    fn out_of_range_lengths_are_clamped() {
        let wild = PomodoroConfig { focus_min: 0, short_break_min: 999, long_break_min: 0, long_break_every: 99, auto_start: true };
        let c = wild.clamped();
        assert_eq!((c.focus_min, c.short_break_min, c.long_break_min), (1, 60, 1));
        assert_eq!((c.long_break_every, c.auto_start), (LONG_BREAK_EVERY_MAX, true));
        let huge = PomodoroConfig { focus_min: u64::MAX, ..Default::default() };
        let mut p = Pomodoro::new(huge);
        assert_eq!(p.snapshot(Instant::now()).remaining_secs, 180 * 60);
        // In-range values pass through untouched.
        let d = PomodoroConfig::default().clamped();
        assert_eq!((d.focus_min, d.short_break_min, d.long_break_min, d.long_break_every), (25, 5, 15, 4));
    }

    #[test]
    fn new_lengths_apply_at_once_to_an_untouched_timer() {
        let mut p = timer();
        p.set_config(PomodoroConfig { focus_min: 50, short_break_min: 10, ..Default::default() });
        let s = p.snapshot(Instant::now());
        assert_eq!((s.running, s.remaining_secs, s.total_secs), (false, 50 * 60, 50 * 60));
        assert_eq!((s.focus_min, s.short_break_min, s.long_break_min), (50, 10, 15));
    }

    #[test]
    fn new_lengths_apply_at_once_after_a_skip_or_reset() {
        let (mut p, t0) = (timer(), Instant::now());
        p.skip(t0); // idle on a fresh short break
        p.set_config(PomodoroConfig { short_break_min: 7, ..Default::default() });
        assert_eq!(p.snapshot(t0).remaining_secs, 7 * 60);
        p.start(t0);
        p.reset();
        p.set_config(PomodoroConfig { short_break_min: 9, ..Default::default() });
        assert_eq!(p.snapshot(t0).remaining_secs, 9 * 60);
    }

    #[test]
    fn a_running_phase_keeps_its_time_and_new_lengths_start_next_phase() {
        let (mut p, t0) = (timer(), Instant::now());
        p.start(t0);
        p.set_config(PomodoroConfig { focus_min: 50, short_break_min: 10, ..Default::default() });
        let s = p.snapshot(t0 + 10 * MIN);
        assert_eq!((s.running, s.remaining_secs, s.total_secs), (true, 15 * 60, 25 * 60));
        assert_eq!(s.focus_min, 50); // the UI can show what comes next
        let s = p.snapshot(t0 + 25 * MIN);
        assert_eq!((s.phase, s.remaining_secs, s.total_secs), (Phase::ShortBreak, 10 * 60, 10 * 60));
    }

    #[test]
    fn a_paused_phase_keeps_its_time_until_reset() {
        let (mut p, t0) = (timer(), Instant::now());
        p.start(t0);
        p.pause(t0 + 5 * MIN);
        p.set_config(PomodoroConfig { focus_min: 10, ..Default::default() });
        let s = p.snapshot(t0 + 6 * MIN);
        assert_eq!((s.running, s.remaining_secs, s.total_secs), (false, 20 * 60, 25 * 60));
        p.reset();
        assert_eq!(p.snapshot(t0 + 6 * MIN).remaining_secs, 10 * 60);
    }

    #[test]
    fn long_break_every_zero_means_never() {
        let mut p = Pomodoro::new(PomodoroConfig { long_break_every: 0, ..Default::default() });
        let t0 = Instant::now();
        p.start(t0);
        assert_eq!(p.snapshot(t0 + 25 * MIN).phase, Phase::ShortBreak);
    }
}
