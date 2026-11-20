//! Live evaluation on a background thread.
//!
//! The view never waits for the evaluator: edits are sent as jobs, newer jobs
//! replace older ones that have not started, and results are picked up on the
//! next frame. [`FsImporter`] lets `Using … from [[file.mpp]]` read real files and
//! the embedded standard library (`std/…`).

use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::Duration;

use mpp_eval::{Evaluated, NumberFormat, Session};

use crate::diagnostics::background::{self, Background, FileCache};

pub use mpp_eval::FileImporter as FsImporter;

/// How the evaluator runs: the View menu's toggles and the default number format.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EvalSettings {
    /// Grey answers after integrals, derivatives, limits and equations nobody asked for.
    pub auto_eval: bool,
    /// Record the working of every query (not only `= ?!` lines).
    pub show_steps: bool,
    /// The number format a file starts from (its `%` lines change it).
    pub number_format: NumberFormat,
    /// Time per block before its automatic answers are left pending (shown with a
    /// spinner and finished in the background).
    pub budget: Option<Duration>,
}

/// The time a block may spend on automatic answers before they are left pending.
pub const AUTO_BUDGET: Duration = Duration::from_millis(150);

impl Default for EvalSettings {
    fn default() -> Self {
        EvalSettings {
            auto_eval: true,
            show_steps: false,
            number_format: NumberFormat::default(),
            budget: Some(AUTO_BUDGET),
        }
    }
}

struct Job {
    settings: EvalSettings,
    generation: u64,
    text: String,
    base: Option<PathBuf>,
    /// The file being edited (left out of the project index).
    path: Option<PathBuf>,
}

/// A finished evaluation.
pub struct Done {
    pub generation: u64,
    pub text: String,
    pub eval: Arc<Evaluated>,
    /// Problems of imported files and the names other files define.
    pub background: Arc<Background>,
    /// Some automatic answers are still being worked out; a newer `Done` with the
    /// same generation follows when they are ready.
    pub pending: bool,
}

/// One evaluator thread with its own [`Session`] (so edits re-run only changed blocks).
pub struct EvalWorker {
    tx: Option<Sender<Job>>,
    rx: Receiver<Done>,
    handle: Option<JoinHandle<()>>,
    sent: u64,
    received: u64,
    settings: EvalSettings,
}

impl std::fmt::Debug for EvalWorker {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EvalWorker")
            .field("sent", &self.sent)
            .field("received", &self.received)
            .finish()
    }
}

impl EvalWorker {
    /// Starts the thread.
    pub fn new() -> Self {
        let (tx, jobs) = channel::<Job>();
        let (done_tx, rx) = channel::<Done>();
        let handle = std::thread::Builder::new()
            .name("mpp-eval".into())
            .stack_size(16 * 1024 * 1024)
            .spawn(move || {
                let mut session = Session::new().with_importer(Arc::new(FsImporter::default()));
                let mut base: Option<PathBuf> = None;
                let mut files = FileCache::default();
                let mut applied: Option<EvalSettings> = None;
                let Ok(mut job) = jobs.recv() else {
                    return;
                };
                'jobs: loop {
                    while let Ok(newer) = jobs.try_recv() {
                        job = newer;
                    }
                    if job.base != base {
                        session.set_base_dir(job.base.clone());
                        base = job.base.clone();
                    }
                    if applied != Some(job.settings) {
                        let s = job.settings;
                        session.set_auto_eval(s.auto_eval);
                        session.set_show_steps(s.show_steps);
                        session.set_number_format(s.number_format);
                        session.set_auto_budget(s.budget);
                        applied = Some(s);
                    }
                    let eval = Arc::new(session.update(&job.text).clone());
                    let extras = Arc::new(background::compute(
                        &eval,
                        job.base.as_deref(),
                        job.path.as_deref(),
                        &mut files,
                    ));
                    let done = Done {
                        generation: job.generation,
                        text: job.text.clone(),
                        eval,
                        background: extras.clone(),
                        pending: session.has_pending(),
                    };
                    if done_tx.send(done).is_err() {
                        break;
                    }
                    // Finish the automatic answers that were left pending, unless a
                    // newer edit arrives first.
                    while session.has_pending() {
                        match jobs.recv_timeout(Duration::from_millis(15)) {
                            Ok(newer) => {
                                job = newer;
                                continue 'jobs;
                            }
                            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                                if session.pending_ready() {
                                    let eval = Arc::new(session.refresh().clone());
                                    let done = Done {
                                        generation: job.generation,
                                        text: job.text.clone(),
                                        eval,
                                        background: extras.clone(),
                                        pending: session.has_pending(),
                                    };
                                    if done_tx.send(done).is_err() {
                                        break 'jobs;
                                    }
                                }
                            }
                            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break 'jobs,
                        }
                    }
                    match jobs.recv() {
                        Ok(next) => job = next,
                        Err(_) => break,
                    }
                }
            })
            .ok();
        EvalWorker {
            tx: Some(tx),
            rx,
            handle,
            sent: 0,
            received: 0,
            settings: EvalSettings::default(),
        }
    }

    /// Asks for `text` to be evaluated; imports resolve against `base`.
    pub fn submit(&mut self, text: &str, base: Option<&Path>) {
        self.submit_file(text, base, None);
    }

    /// The settings the next jobs run with.
    pub fn set_settings(&mut self, settings: EvalSettings) {
        self.settings = settings;
    }

    /// Asks for the text of the file at `path` to be evaluated; imports resolve
    /// against `base` (its folder, or the folder an unsaved file belongs to).
    pub fn submit_file(&mut self, text: &str, base: Option<&Path>, path: Option<&Path>) {
        self.sent += 1;
        let job = Job {
            settings: self.settings,
            generation: self.sent,
            text: text.to_string(),
            base: base.map(Path::to_path_buf),
            path: path.map(Path::to_path_buf),
        };
        if let Some(tx) = &self.tx {
            if tx.send(job).is_err() {
                self.sent -= 1;
            }
        }
    }

    /// The newest finished evaluation, if one arrived.
    pub fn poll(&mut self) -> Option<Done> {
        let mut latest = None;
        while let Ok(d) = self.rx.try_recv() {
            self.received = self.received.max(d.generation);
            latest = Some(d);
        }
        latest
    }

    /// Waits (up to `timeout`) for everything submitted so far.
    pub fn wait(&mut self, timeout: Duration) -> Option<Done> {
        let mut latest = self.poll();
        let deadline = std::time::Instant::now() + timeout;
        while self.received < self.sent {
            let left = deadline.saturating_duration_since(std::time::Instant::now());
            if left.is_zero() {
                break;
            }
            match self.rx.recv_timeout(left) {
                Ok(d) => {
                    self.received = self.received.max(d.generation);
                    latest = Some(d);
                }
                Err(_) => break,
            }
        }
        latest
    }

    /// True while a submitted job has not come back.
    pub fn busy(&self) -> bool {
        self.received < self.sent
    }
}

impl Default for EvalWorker {
    fn default() -> Self {
        EvalWorker::new()
    }
}

impl Drop for EvalWorker {
    fn drop(&mut self) {
        self.tx = None;
        if let Some(h) = self.handle.take() {
            // The thread ends once the job channel is closed and its last job is done.
            drop(h);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn evaluates_in_the_background() {
        let mut w = EvalWorker::new();
        w.submit("$$ 1 + 1 = ? $$", None);
        let done = w.wait(Duration::from_secs(20)).expect("a result");
        assert!(done.eval.queries().count() >= 1);
        assert!(!w.busy());
    }

    #[test]
    fn only_the_newest_job_matters() {
        let mut w = EvalWorker::new();
        for i in 0..5 {
            w.submit(&format!("$$ {i} + 1 = ? $$"), None);
        }
        let done = w.wait(Duration::from_secs(20)).expect("a result");
        assert_eq!(done.generation, 5);
    }
}
