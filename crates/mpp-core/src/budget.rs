//! Cancellation and deadlines for long calculations.
//!
//! The evaluator installs a [`Budget`] on the thread that runs a query (with
//! [`install`]); the hot loops of the core (factoring, sums, integration, series, ODE
//! stepping) call [`check`] now and then and stop with a clear error once the cancel
//! flag is set or the deadline has passed. With no budget installed `check` is free
//! and never fails, so library users and tests are unaffected.

use std::cell::{Cell, RefCell};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::error::{MathError, MathResult};

/// A cancel flag and an optional time limit, shared with whoever may cancel.
#[derive(Clone, Debug)]
pub struct Budget {
    cancel: Arc<AtomicBool>,
    limit: Option<Duration>,
    deadline: Option<Instant>,
}

/// Why a calculation was stopped.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Trip {
    /// Someone asked for it to stop (a newer edit, a closed window).
    Cancelled,
    /// The time limit passed.
    TimedOut,
}

impl Budget {
    /// A budget with the given cancel flag and per-query time limit.
    pub fn new(cancel: Arc<AtomicBool>, limit: Option<Duration>) -> Budget {
        Budget {
            cancel,
            limit,
            deadline: limit.map(|l| Instant::now() + l),
        }
    }

    /// Starts the time limit again (one limit per query, not per document).
    pub fn restart(&mut self) {
        self.deadline = self.limit.map(|l| Instant::now() + l);
    }
}

thread_local! {
    static CURRENT: RefCell<Option<Budget>> = const { RefCell::new(None) };
    static TICKS: Cell<u32> = const { Cell::new(0) };
    static TRIPPED: Cell<Option<Trip>> = const { Cell::new(None) };
}

/// Keeps a budget installed on this thread; the previous one comes back on drop.
pub struct Guard {
    previous: Option<Budget>,
}

/// Installs `budget` for the current thread until the guard is dropped.
pub fn install(budget: Budget) -> Guard {
    TRIPPED.with(|t| t.set(None));
    let previous = CURRENT.with(|c| c.borrow_mut().replace(budget));
    Guard { previous }
}

impl Drop for Guard {
    fn drop(&mut self) {
        let previous = self.previous.take();
        CURRENT.with(|c| *c.borrow_mut() = previous);
    }
}

/// Restarts the time limit of the budget installed on this thread (a new query
/// begins) and forgets an earlier time-out.
pub fn restart() {
    CURRENT.with(|c| {
        if let Some(b) = c.borrow_mut().as_mut() {
            b.restart();
        }
    });
    TRIPPED.with(|t| {
        if t.get() == Some(Trip::TimedOut) {
            t.set(None);
        }
    });
}

/// What stopped the work on this thread since the budget was installed (or the last
/// [`take_tripped`]), if anything.
pub fn take_tripped() -> Option<Trip> {
    TRIPPED.with(Cell::take)
}

/// Like [`take_tripped`] but leaves the record in place.
pub fn peek_tripped() -> Option<Trip> {
    TRIPPED.with(Cell::get)
}

/// Checks the budget: `Err` once the work should stop. Cheap enough for inner loops
/// (the clock is read once every 64 calls; the cancel flag on every call).
pub fn check() -> MathResult<()> {
    CURRENT.with(|c| {
        let c = c.borrow();
        let Some(b) = c.as_ref() else {
            return Ok(());
        };
        if b.cancel.load(Ordering::Relaxed) {
            TRIPPED.with(|t| t.set(Some(Trip::Cancelled)));
            return Err(MathError::Cancelled);
        }
        if let Some(deadline) = b.deadline {
            let tick = TICKS.with(|t| {
                let n = t.get().wrapping_add(1);
                t.set(n);
                n
            });
            let already = TRIPPED.with(Cell::get) == Some(Trip::TimedOut);
            if already || tick.is_multiple_of(64) {
                let now = Instant::now();
                if already || now >= deadline {
                    TRIPPED.with(|t| t.set(Some(Trip::TimedOut)));
                    let ms = b.limit.map_or(0, |l| l.as_millis() as u64);
                    return Err(MathError::TimedOut(ms));
                }
            }
        }
        Ok(())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_budget_never_stops() {
        for _ in 0..1000 {
            assert!(check().is_ok());
        }
    }

    #[test]
    fn the_cancel_flag_stops_work() {
        let flag = Arc::new(AtomicBool::new(false));
        let _g = install(Budget::new(flag.clone(), None));
        assert!(check().is_ok());
        flag.store(true, Ordering::Relaxed);
        assert_eq!(check(), Err(MathError::Cancelled));
        assert_eq!(take_tripped(), Some(Trip::Cancelled));
    }

    #[test]
    fn the_deadline_stops_work_and_is_removed_with_the_guard() {
        {
            let _g = install(Budget::new(
                Arc::new(AtomicBool::new(false)),
                Some(Duration::from_millis(1)),
            ));
            std::thread::sleep(Duration::from_millis(5));
            let stopped = (0..200).any(|_| check().is_err());
            assert!(stopped);
            assert_eq!(take_tripped(), Some(Trip::TimedOut));
        }
        assert!(check().is_ok());
    }
}
