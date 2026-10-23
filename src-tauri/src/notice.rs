//! The native notification text for a finished Pomodoro phase.

use crate::pomodoro::{Phase, Snapshot};

fn ended(phase: Phase) -> &'static str {
    match phase {
        Phase::Focus => "Focus done",
        Phase::ShortBreak | Phase::LongBreak => "Break over",
    }
}

fn next(phase: Phase, minutes: u64) -> String {
    match phase {
        Phase::Focus => format!("{minutes} min focus"),
        Phase::ShortBreak => format!("{minutes} min break"),
        Phase::LongBreak => format!("{minutes} min long break"),
    }
}

/// `(title, body)` for the snapshot that reported `finished`, or `None` for any other snapshot.
/// `snapshot.phase` is already the phase that follows, so the body says what comes next.
pub fn phase_end(snapshot: &Snapshot) -> Option<(String, String)> {
    let finished = snapshot.finished?;
    Some((ended(finished).to_string(), next(snapshot.phase, snapshot.total_secs / 60)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pomodoro::{Pomodoro, PomodoroConfig};
    use chrono::NaiveDate;
    use std::time::{Duration, Instant};

    fn run(minutes: u64) -> (Pomodoro, Instant) {
        let mut p = Pomodoro::new(PomodoroConfig::default(), NaiveDate::from_ymd_opt(2026, 10, 21).unwrap(), 0);
        let t0 = Instant::now();
        p.start(t0);
        (p, t0 + Duration::from_secs(minutes * 60))
    }

    #[test]
    fn a_finished_focus_round_names_the_break_that_follows() {
        let (mut p, later) = run(26);
        let s = p.snapshot(later);
        assert_eq!(phase_end(&s), Some(("Focus done".into(), "5 min break".into())));
    }

    #[test]
    fn it_fires_once_not_on_every_poll() {
        let (mut p, later) = run(26);
        assert!(phase_end(&p.snapshot(later)).is_some());
        assert!(phase_end(&p.snapshot(later + Duration::from_secs(1))).is_none());
    }

    #[test]
    fn a_running_timer_has_nothing_to_announce() {
        let (mut p, _) = run(1);
        assert!(phase_end(&p.snapshot(Instant::now())).is_none());
    }
}
