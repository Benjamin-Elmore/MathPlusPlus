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
//!
//! Slow jobs run on a small pool of worker threads ([`POOL_SIZE`]) fed from a bounded
//! queue that serves the newest job first; a job whose line was edited away is dropped
//! from the queue or stopped through its cancel flag, and no job runs past
//! [`WORKER_LIMIT`]. Typing a slow expression one character at a time therefore never
//! piles up threads.

use std::collections::hash_map::DefaultHasher;
use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::hash::{Hash, Hasher};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
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

/// Most worker threads running slow jobs at once.
const POOL_SIZE: usize = 2;

/// Most jobs waiting for a worker; beyond this the oldest waiting job is dropped.
const MAX_QUEUE: usize = 8;

/// The longest any one background job may run before it is stopped.
const WORKER_LIMIT: Duration = Duration::from_secs(120);

/// How long an idle worker waits for work before its thread ends.
const WORKER_IDLE: Duration = Duration::from_secs(5);

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
    /// Jobs queued or running, with the flag that stops each.
    running: Mutex<HashMap<u64, Arc<AtomicBool>>>,
    /// Signalled whenever a queued or running job finishes or is dropped.
    finished: Condvar,
    /// Jobs waiting for a worker (newest at the back) and the live worker count.
    queue: Mutex<Pool>,
    /// Signalled when a job is queued.
    work: Condvar,
    /// The jobs the current pass asked for.
    touched: Mutex<HashSet<u64>>,
}

#[derive(Debug, Default)]
struct Pool {
    waiting: VecDeque<(u64, Job)>,
    workers: usize,
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

    /// True while some job is still queued or running.
    pub fn has_running(&self) -> bool {
        !lock(&self.running).is_empty()
    }

    /// A new pass over the document begins: forget which jobs were asked for.
    pub fn begin_pass(&self) {
        lock(&self.touched).clear();
    }

    /// The pass is over and the text changed: queued jobs nobody asked for any more
    /// are dropped and running ones are told to stop.
    pub fn sweep(&self) {
        let touched = lock(&self.touched).clone();
        let mut pool = lock(&self.queue);
        let mut running = lock(&self.running);
        let queued: HashSet<u64> = pool.waiting.iter().map(|(k, _)| *k).collect();
        pool.waiting.retain(|(k, _)| touched.contains(k));
        running.retain(|k, flag| {
            if touched.contains(k) {
                return true;
            }
            if queued.contains(k) {
                // Never started: gone now.
                return false;
            }
            // On a worker: stopped, and it removes itself when it ends.
            flag.store(true, Ordering::Relaxed);
            true
        });
        drop(running);
        drop(pool);
        self.finished.notify_all();
    }

    /// Stops every job (the session is going away).
    pub fn cancel_all(&self) {
        lock(&self.queue).waiting.clear();
        for flag in lock(&self.running).values() {
            flag.store(true, Ordering::Relaxed);
        }
        self.finished.notify_all();
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

impl std::fmt::Debug for Job {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Job").field("expr", &self.expr).finish()
    }
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

    /// Works the job out on this thread; the flag is false when it was stopped on
    /// request, so the answer says nothing about the maths and is not kept.
    fn compute(&self) -> (AutoOutcome, bool) {
        let ctx = Ctx::new(&self.holders)
            .numeric(self.numeric)
            .with_assumptions(self.assumptions.clone());
        let value = {
            let mut r = Resolver::new(&NoNames, &ctx, BTreeMap::new());
            r.run(&self.expr)
        };
        let keep = mpp_core::budget::peek_tripped() != Some(mpp_core::budget::Trip::Cancelled);
        let Collected { notes, shown, .. } = ctx.finish();
        (
            AutoOutcome {
                value: value.map_err(|e| e.to_string()),
                notes,
                shown,
            },
            keep,
        )
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
/// waiting or running after `deadline` (then a pool worker finishes it and it lands
/// in the cache).
pub(crate) fn answer(auto: &AutoEval, job: Job, deadline: Option<Instant>) -> Option<AutoOutcome> {
    let key = job.key();
    lock(&auto.cache.touched).insert(key);
    if let Some(done) = auto.cache.get(key) {
        return Some(done);
    }
    let Some(deadline) = deadline else {
        return Some(compute_here(auto, key, &job));
    };
    if !lock(&auto.cache.running).contains_key(&key) && !submit(&auto.cache, key, job.clone()) {
        // No thread could be started: work it out here.
        return Some(compute_here(auto, key, &job));
    }
    wait_for(auto, key, deadline)
}

fn compute_here(auto: &AutoEval, key: u64, job: &Job) -> AutoOutcome {
    let (out, keep) = job.compute();
    if keep {
        auto.cache.put(key, out.clone());
    }
    out
}

/// Queues `job` for the pool, newest first; false when no worker could be started.
fn submit(cache: &Arc<AutoCache>, key: u64, job: Job) -> bool {
    let mut pool = lock(&cache.queue);
    lock(&cache.running).insert(key, Arc::new(AtomicBool::new(false)));
    pool.waiting.push_back((key, job));
    while pool.waiting.len() > MAX_QUEUE {
        // The oldest waiting job is the least likely to still be on screen.
        if let Some((old, _)) = pool.waiting.pop_front() {
            lock(&cache.running).remove(&old);
        }
    }
    if pool.workers < POOL_SIZE {
        let worker_cache = cache.clone();
        let spawned = std::thread::Builder::new()
            .name("mpp-auto".into())
            .stack_size(WORKER_STACK)
            .spawn(move || worker(&worker_cache));
        if spawned.is_ok() {
            pool.workers += 1;
        } else if pool.workers == 0 {
            pool.waiting.retain(|(k, _)| *k != key);
            lock(&cache.running).remove(&key);
            return false;
        }
    }
    drop(pool);
    cache.work.notify_one();
    true
}

/// One pool worker: takes the newest waiting job until it has been idle a while.
fn worker(cache: &Arc<AutoCache>) {
    loop {
        let next = {
            let mut pool = lock(&cache.queue);
            loop {
                if let Some(item) = pool.waiting.pop_back() {
                    break Some(item);
                }
                let (guard, timeout) = match cache.work.wait_timeout(pool, WORKER_IDLE) {
                    Ok(r) => r,
                    Err(p) => p.into_inner(),
                };
                pool = guard;
                if timeout.timed_out() && pool.waiting.is_empty() {
                    pool.workers = pool.workers.saturating_sub(1);
                    break None;
                }
            }
        };
        let Some((key, job)) = next else {
            return;
        };
        let flag = lock(&cache.running).get(&key).cloned();
        if let Some(flag) = flag {
            let budget = mpp_core::budget::Budget::new(flag, Some(WORKER_LIMIT));
            let _guard = mpp_core::budget::install(budget);
            let (out, keep) = job.compute();
            if keep {
                cache.put(key, out);
            }
        }
        lock(&cache.running).remove(&key);
        cache.finished.notify_all();
    }
}

/// Waits for a queued or running job until `deadline`; `None` when it is still
/// going (or was dropped, which leaves its line pending to be asked for again).
fn wait_for(auto: &AutoEval, key: u64, deadline: Instant) -> Option<AutoOutcome> {
    let mut running = lock(&auto.cache.running);
    while running.contains_key(&key) {
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

    #[test]
    fn many_slow_jobs_never_start_more_than_the_pool() {
        let auto = AutoEval::default();
        for n in 0..40 {
            let e = Expr::int(n) + Expr::int(1000);
            let _ = answer(&auto, job(e), Some(Instant::now()));
            let pool = lock(&auto.cache.queue);
            assert!(pool.workers <= POOL_SIZE, "{} workers", pool.workers);
            assert!(pool.waiting.len() <= MAX_QUEUE);
        }
        assert!(auto.cache.wait(Duration::from_secs(60)));
    }

    #[test]
    fn a_sweep_drops_jobs_nobody_asked_for() {
        let auto = AutoEval::default();
        auto.cache.begin_pass();
        {
            // Queue a job by hand without starting a worker.
            let mut pool = lock(&auto.cache.queue);
            let j = job(Expr::int(9));
            let key = j.key();
            pool.waiting.push_back((key, j));
            lock(&auto.cache.running).insert(key, Arc::new(AtomicBool::new(false)));
        }
        assert!(auto.cache.has_running());
        auto.cache.sweep();
        assert!(!auto.cache.has_running());
        assert!(lock(&auto.cache.queue).waiting.is_empty());
    }
}
