//! The status bar's numbers, kept calm: "Working…" shows only once a run has
//! taken more than a second, and the definitions and problems counts change
//! only when a run finishes (never between keys).

use std::time::{Duration, Instant};

use mpp_eval::{Evaluated, LineKind};

use crate::diagnostics::Counts;

/// How long a run goes before the status bar says "Working…".
pub const WORKING_AFTER: Duration = Duration::from_secs(1);

/// What the status bar remembers between frames.
#[derive(Debug, Default)]
pub struct StatusMemo {
    /// The document the numbers belong to.
    doc: Option<u64>,
    /// When the current run started (none while no run is going).
    run_since: Option<Instant>,
    counts: Counts,
    defs: usize,
}

/// What the status bar shows this frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StatusView {
    pub working: bool,
    pub counts: Counts,
    pub defs: usize,
    /// When to draw again so "Working…" appears on time.
    pub wake: Option<Duration>,
}

impl StatusMemo {
    /// The document the numbers belong to.
    pub fn doc(&self) -> Option<u64> {
        self.doc
    }

    /// One frame. `running` is true while the evaluator works on the file;
    /// `settled` is true when no run is running or waiting; `fresh` gives the
    /// counts as they are now (asked for only when they will be used).
    pub fn update(
        &mut self,
        doc: Option<u64>,
        running: bool,
        settled: bool,
        now: Instant,
        fresh: impl FnOnce() -> (Counts, usize),
    ) -> StatusView {
        if doc != self.doc {
            // Another file: its own numbers straight away, and no old timer.
            self.doc = doc;
            self.run_since = None;
            (self.counts, self.defs) = fresh();
        } else if settled {
            (self.counts, self.defs) = fresh();
        }
        if running {
            self.run_since.get_or_insert(now);
        } else {
            self.run_since = None;
        }
        let took = self.run_since.map(|t| now.saturating_duration_since(t));
        let working = took.is_some_and(|t| t > WORKING_AFTER);
        let wake = took
            .filter(|_| !working)
            .map(|t| WORKING_AFTER.saturating_sub(t) + Duration::from_millis(10));
        StatusView {
            working,
            counts: self.counts,
            defs: self.defs,
            wake,
        }
    }
}

/// The number of names the file defines, from the evaluator's results: every
/// definition line it ran, each name counted once.
pub fn definitions(ev: &Evaluated) -> usize {
    let mut names: Vec<&str> = ev
        .blocks
        .iter()
        .flat_map(|b| b.values.iter())
        .filter_map(|v| match &v.kind {
            LineKind::Definition { name, .. } => Some(name.as_str()),
            _ => None,
        })
        .collect();
    names.sort_unstable();
    names.dedup();
    names.len()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn counts(errors: usize) -> Counts {
        Counts {
            errors,
            ..Counts::default()
        }
    }

    #[test]
    fn working_shows_only_after_a_second_of_one_run() {
        let mut m = StatusMemo::default();
        let t0 = Instant::now();
        let v = m.update(Some(1), false, true, t0, || (counts(0), 0));
        assert!(!v.working && v.wake.is_none());
        let v = m.update(Some(1), true, false, t0, || (counts(0), 0));
        assert!(!v.working, "a run that just started is quiet");
        assert!(v.wake.is_some_and(|w| w >= WORKING_AFTER));
        let v = m.update(
            Some(1),
            true,
            false,
            t0 + Duration::from_millis(900),
            || (counts(0), 0),
        );
        assert!(!v.working);
        let v = m.update(
            Some(1),
            true,
            false,
            t0 + Duration::from_millis(1100),
            || (counts(0), 0),
        );
        assert!(v.working, "a slow run says so");
        let v = m.update(
            Some(1),
            false,
            true,
            t0 + Duration::from_millis(1200),
            || (counts(0), 0),
        );
        assert!(!v.working, "and stops saying so when it ends");
    }

    #[test]
    fn short_runs_one_after_another_never_show_working() {
        let mut m = StatusMemo::default();
        let t0 = Instant::now();
        for i in 0..40u64 {
            let at = t0 + Duration::from_millis(i * 100);
            let running = i % 2 == 0;
            let v = m.update(Some(1), running, !running, at, || (counts(0), 0));
            assert!(!v.working, "step {i}");
        }
    }

    #[test]
    fn counts_change_only_when_a_run_finishes() {
        let mut m = StatusMemo::default();
        let t0 = Instant::now();
        let v = m.update(Some(1), false, true, t0, || (counts(1), 2));
        assert_eq!((v.counts, v.defs), (counts(1), 2));
        // Typing: a run is waiting; whatever the page says now is not shown.
        let v = m.update(Some(1), false, false, t0, || (counts(5), 0));
        assert_eq!((v.counts, v.defs), (counts(1), 2));
        let v = m.update(Some(1), true, false, t0, || (counts(5), 0));
        assert_eq!((v.counts, v.defs), (counts(1), 2));
        // The run finished.
        let v = m.update(Some(1), false, true, t0, || (counts(0), 3));
        assert_eq!((v.counts, v.defs), (counts(0), 3));
        // Another file shows its own numbers at once.
        let v = m.update(Some(2), true, false, t0, || (counts(4), 1));
        assert_eq!((v.counts, v.defs), (counts(4), 1));
    }
}
