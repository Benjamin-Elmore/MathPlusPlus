//! Auto-evaluation (type it, see the answer): the shared state behind the grey `= …`
//! results of display maths.
//!
//! A display block that is a bare expression, or an equation with an unknown, is worked
//! out without `= ?`. The answer lives only in the view model (the [`crate::LineResult`]
//! gets `auto: true`); the file is never changed. What is computed is a *job*: the line's
//! expression with every known name already put in, so its answer depends on nothing
//! else, and it is cached under the hash of that job. A job that does not finish within
//! the session's time budget keeps running on a worker thread; its line is `pending`
//! (the IDE shows a spinner) and the block is re-run by [`crate::Session::refresh`] once
//! the answer is in the cache, so a slow integral never holds up the other blocks.

use std::collections::hash_map::DefaultHasher;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::hash::{Hash, Hasher};
use std::sync::{mpsc, Arc, Condvar, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use mpp_core::domains::Assumptions;
use mpp_core::Expr;

use crate::holders::{Collected, Ctx, Holders, Shown};
use crate::resolve::{Lookup, Resolver};
use crate::value::Binding;

/// Most cached answers kept; the oldest half is dropped beyond this.
const MAX_CACHED: usize = 4096;

/// Stack for a worker: evaluation recurses (the same size as the session's own).
const WORKER_STACK: usize = 256 << 20;

/// The auto-evaluation settings and cache of a session.
#[derive(Debug, Clone)]
pub(crate) struct AutoEval {
    /// The session-wide toggle (`Session::set_auto_eval`).
    pub enabled: bool,
    /// Time each block may spend on its auto results before they are left pending.
    /// `None` computes everything in place.
    pub budget: Option<Duration>,
    pub cache: Arc<AutoCache>,
}

impl Default for AutoEval {
    fn default() -> Self {
        AutoEval {
            enabled: true,
            budget: None,
            cache: Arc::new(AutoCache::default()),
        }
    }
}

/// What an auto-evaluated line produced.
#[derive(Debug, Clone)]
pub(crate) struct AutoOutcome {
    /// The value, or the error message.
    pub value: Result<Expr, String>,
    pub notes: Vec<String>,
    pub shown: Vec<Shown>,
}

/// Answers by job hash, and the jobs still running.
#[derive(Debug, Default)]
pub(crate) struct AutoCache {
    done: Mutex<HashMap<u64, AutoOutcome>>,
    running: Mutex<HashSet<u64>>,
    /// Signalled whenever a running job finishes.
    finished: Condvar,
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    // A worker that panicked leaves no half-written entry behind, so the data is usable.
    m.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

impl AutoCache {
    fn get(&self, key: u64) -> Option<AutoOutcome> {
        lock(&self.done).get(&key).cloned()
    }

    fn put(&self, key: u64, outcome: AutoOutcome) {
        let mut done = lock(&self.done);
        if done.len() >= MAX_CACHED {
            let drop: Vec<u64> = done.keys().take(MAX_CACHED / 2).copied().collect();
            for k in drop {
                done.remove(&k);
            }
        }
        done.insert(key, outcome);
    }

    /// True while some job is still running.
    pub fn has_running(&self) -> bool {
        !lock(&self.running).is_empty()
    }

    /// Waits until no job is running or `timeout` passes; true when none is running.
    pub fn wait(&self, timeout: Duration) -> bool {
        let deadline = Instant::now() + timeout;
        let mut running = lock(&self.running);
        while !running.is_empty() {
            let now = Instant::now();
            if now >= deadline {
                return false;
            }
            running = match self.finished.wait_timeout(running, deadline - now) {
                Ok((guard, _)) => guard,
                Err(poisoned) => poisoned.into_inner().0,
            };
        }
        true
    }

    /// Forgets every answer (after a change that affects all of them).
    pub fn clear(&self) {
        lock(&self.done).clear();
    }
}

/// One thing to work out: an expression with every known name already in it.
#[derive(Clone)]
pub(crate) struct Job {
    pub expr: Expr,
    pub numeric: bool,
    pub holders: Arc<Holders>,
    pub assumptions: Arc<Assumptions>,
}

impl Job {
    /// The cache key: the expression, the mode and the facts in force.
    pub fn key(&self) -> u64 {
        let mut h = DefaultHasher::new();
        format!("{:?}", self.expr).hash(&mut h);
        self.numeric.hash(&mut h);
        format!("{:?}", self.assumptions).hash(&mut h);
        h.finish()
    }

    fn compute(&self) -> AutoOutcome {
        let ctx = Ctx::new(&self.holders)
            .numeric(self.numeric)
            .with_assumptions(self.assumptions.clone());
        let value = {
            let mut r = Resolver::new(&NoNames, &ctx, BTreeMap::new());
            r.run(&self.expr)
        };
        let Collected { notes, shown, .. } = ctx.finish();
        AutoOutcome {
            value: value.map_err(|e| e.to_string()),
            notes,
            shown,
        }
    }
}

/// A scope with no names: a job already has every known value put in.
struct NoNames;

impl Lookup for NoNames {
    fn lookup(&self, _name: &str) -> Option<Arc<Binding>> {
        None
    }
}

/// The answer to `job`: from the cache, computed now, or `None` while it is still
/// running after `deadline` (then it finishes on a worker thread and lands in the
/// cache).
pub(crate) fn answer(auto: &AutoEval, job: Job, deadline: Option<Instant>) -> Option<AutoOutcome> {
    let key = job.key();
    if let Some(done) = auto.cache.get(key) {
        return Some(done);
    }
    let Some(deadline) = deadline else {
        let out = job.compute();
        auto.cache.put(key, out.clone());
        return Some(out);
    };
    if lock(&auto.cache.running).contains(&key) {
        return wait_for(auto, key, deadline);
    }
    lock(&auto.cache.running).insert(key);
    let (tx, rx) = mpsc::channel();
    let cache = auto.cache.clone();
    let worker = {
        let job = job.clone();
        std::thread::Builder::new()
            .name("mpp-auto".into())
            .stack_size(WORKER_STACK)
            .spawn(move || {
                let out = job.compute();
                cache.put(key, out.clone());
                lock(&cache.running).remove(&key);
                cache.finished.notify_all();
                // The block may have moved on; then nobody is listening.
                let _ = tx.send(out);
            })
    };
    if worker.is_err() {
        // No thread could be started: work it out here.
        lock(&auto.cache.running).remove(&key);
        let out = job.compute();
        auto.cache.put(key, out.clone());
        return Some(out);
    }
    let left = deadline.saturating_duration_since(Instant::now());
    rx.recv_timeout(left).ok()
}

/// Waits for a job another block started.
fn wait_for(auto: &AutoEval, key: u64, deadline: Instant) -> Option<AutoOutcome> {
    let mut running = lock(&auto.cache.running);
    while running.contains(&key) {
        let now = Instant::now();
        if now >= deadline {
            return None;
        }
        running = match auto.cache.finished.wait_timeout(running, deadline - now) {
            Ok((guard, _)) => guard,
            Err(poisoned) => poisoned.into_inner().0,
        };
    }
    drop(running);
    auto.cache.get(key)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn job(e: Expr) -> Job {
        Job {
            expr: e,
            numeric: false,
            holders: Arc::new(Holders::new()),
            assumptions: Arc::new(Assumptions::new()),
        }
    }

    #[test]
    fn answers_are_cached_by_job() {
        let auto = AutoEval::default();
        let e = Expr::int(2) + Expr::int(3);
        let first = answer(&auto, job(e.clone()), None).expect("computed in place");
        assert_eq!(first.value, Ok(Expr::int(5)));
        assert!(auto.cache.get(job(e).key()).is_some());
    }

    #[test]
    fn a_zero_budget_leaves_the_job_running() {
        let auto = AutoEval {
            budget: Some(Duration::ZERO),
            ..AutoEval::default()
        };
        let e = Expr::int(6) * Expr::int(7);
        let past = Instant::now();
        let got = answer(&auto, job(e.clone()), Some(past));
        // Either it is pending or (on a fast machine) already done; both are fine, but
        // after waiting it must be in the cache.
        assert!(auto.cache.wait(Duration::from_secs(30)));
        let cached = auto.cache.get(job(e).key()).expect("finished");
        assert_eq!(cached.value, Ok(Expr::int(42)));
        if let Some(out) = got {
            assert_eq!(out.value, Ok(Expr::int(42)));
        }
    }
}
