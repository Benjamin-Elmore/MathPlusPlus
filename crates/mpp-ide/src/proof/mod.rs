//! Proofs in the IDE: the checker's statuses as marks in the equation view, the
//! summary line under each theorem, the proof agent panel, the API key settings and
//! the Lean alignment view.
//!
//! Everything the checker reports is preview data: marks, squiggles and summaries
//! are drawn on the page and never written into the file. The checker runs on its
//! own thread after each edit ([`ProofWorker`]); the page uses its newest report.

pub mod marks;
pub mod ui;
mod wire;

use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::Arc;
use std::time::Duration;

use mpp_proof::{DocumentCheck, StepStatus, Verdict};
use mpp_syntax::document::{parse_document, BlockKind, Document, TheoremKind};
use mpp_syntax::Span;

pub use ui::{AgentPanel, AlignView, KeySettings, ProofUi};

/// A theorem-like block that the checker reports on (theorems, lemmas,
/// corollaries, propositions, conjectures).
pub fn is_result(kind: TheoremKind) -> bool {
    matches!(
        kind,
        TheoremKind::Theorem
            | TheoremKind::Lemma
            | TheoremKind::Corollary
            | TheoremKind::Proposition
            | TheoremKind::Conjecture
    )
}

/// How strong a summary is, for its colour: green only for "proved", amber for
/// cited or structure only, red for gaps, external results and wrong steps.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tone {
    Proved,
    Partial,
    Weak,
}

impl Tone {
    /// The tone of a verdict.
    pub fn of(v: &Verdict) -> Tone {
        match v {
            Verdict::Proved => Tone::Proved,
            Verdict::ProvedUsingCited(_) | Verdict::ProvedStructureOnly => Tone::Partial,
            _ => Tone::Weak,
        }
    }

    /// Its colour in a theme.
    pub fn color(self, theme: &mpp_render::Theme) -> mpp_render::Color {
        match self {
            Tone::Proved => theme.series[2],
            Tone::Partial => theme.warning,
            Tone::Weak => theme.error,
        }
    }
}

/// The colour of a step's mark.
pub fn status_color(s: StepStatus, theme: &mpp_render::Theme) -> mpp_render::Color {
    match s {
        StepStatus::Symbolic | StepStatus::Lean | StepStatus::Numeric => theme.series[2],
        StepStatus::Structure | StepStatus::Cited => theme.info,
        StepStatus::Gap | StepStatus::External => theme.warning,
        StepStatus::Wrong => theme.error,
    }
}

/// The summary line under one theorem.
#[derive(Debug, Clone, PartialEq)]
pub struct SummaryLine {
    /// The theorem block's span.
    pub theorem: Span,
    /// `Lemma 2`.
    pub name: String,
    pub text: String,
    pub tone: Tone,
}

/// One proof step's status, placed by its span.
#[derive(Debug, Clone, PartialEq)]
pub struct StepMark {
    pub span: Span,
    pub number: Option<u32>,
    pub status: StepStatus,
    pub detail: String,
    pub counterexample: Option<String>,
    /// Each relation checked (its LaTeX) with what became of it.
    pub claims: Vec<(String, String)>,
    /// The result whose proof this step is in.
    pub result: String,
}

/// What the page draws from the checker's report.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ProofView {
    pub summaries: Vec<SummaryLine>,
    /// Step marks (only when the report is of the text on screen).
    pub steps: Vec<StepMark>,
}

impl ProofView {
    /// Matches a report to the parsed document. Summaries are matched to theorems by
    /// their order, so they stay put while a newer report is on its way; step marks
    /// need offsets and are only given for a report of exactly this text (`fresh`).
    pub fn new(doc: &Document, check: &DocumentCheck, fresh: bool) -> ProofView {
        let mut out = ProofView::default();
        let mut k = 0;
        for b in &doc.blocks {
            let BlockKind::Theorem(t) = &b.kind else {
                continue;
            };
            if !is_result(t.kind) {
                continue;
            }
            let Some(r) = check.results.get(k) else {
                break;
            };
            k += 1;
            if r.kind != t.kind {
                continue;
            }
            if !r.proofs.is_empty() || t.kind == TheoremKind::Conjecture {
                out.summaries.push(SummaryLine {
                    theorem: b.span,
                    name: r.name.clone(),
                    text: r.summary.text.clone(),
                    tone: Tone::of(&r.summary.verdict),
                });
            }
        }
        if fresh {
            for r in &check.results {
                for p in &r.proofs {
                    for s in &p.steps {
                        out.steps.push(StepMark {
                            span: s.span,
                            number: s.number,
                            status: s.status,
                            detail: s.detail.clone(),
                            counterexample: s.counterexample.clone(),
                            claims: s
                                .claims
                                .iter()
                                .map(|c| (c.source.clone(), claim_outcome(&c.outcome)))
                                .collect(),
                            result: r.name.clone(),
                        });
                    }
                }
            }
        }
        out
    }

    /// The summary line of the theorem block at `span`.
    pub fn summary_for(&self, span: Span) -> Option<&SummaryLine> {
        self.summaries.iter().find(|s| s.theorem == span)
    }
}

fn claim_outcome(o: &mpp_proof::ClaimOutcome) -> String {
    use mpp_proof::ClaimOutcome as C;
    match o {
        C::Proved => "proved".into(),
        C::ProvedBy(rule) => format!("proved ({rule})"),
        C::Numeric(how) => format!("agrees numerically ({how})"),
        C::Failed(cx) => format!("fails: {cx}"),
        C::Unverified(why) => format!("not verified ({why})"),
        C::Informal(why) => format!("informal ({why})"),
    }
}

/// A finished check.
pub struct ProofDone {
    pub generation: u64,
    pub text: String,
    pub check: Arc<DocumentCheck>,
}

struct ProofJob {
    generation: u64,
    text: String,
    base: Option<PathBuf>,
}

/// The checker on its own thread: newer texts replace older ones that have not
/// started, and results are picked up on the next frame.
pub struct ProofWorker {
    tx: Option<Sender<ProofJob>>,
    rx: Receiver<ProofDone>,
    sent: u64,
    received: u64,
}

impl std::fmt::Debug for ProofWorker {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ProofWorker")
            .field("sent", &self.sent)
            .field("received", &self.received)
            .finish()
    }
}

impl Default for ProofWorker {
    fn default() -> Self {
        ProofWorker::new()
    }
}

/// True when a document has a theorem-like block the checker reports on.
pub fn has_results(src: &str) -> bool {
    [
        "**Theorem",
        "**Lemma",
        "**Corollary",
        "**Proposition",
        "**Conjecture",
    ]
    .iter()
    .any(|h| src.contains(h))
}

/// Checks a text: the `References from [[file]]` entries are read relative to `base`.
pub fn check_text(src: &str, base: Option<&Path>) -> DocumentCheck {
    if !has_results(src) {
        return DocumentCheck::default();
    }
    let doc = parse_document(src);
    let refs = crate::page::ExtraRefs::read(&doc, base);
    mpp_proof::check_document(&doc, src, &refs.entries)
}

impl ProofWorker {
    /// Starts the thread.
    pub fn new() -> Self {
        let (tx, jobs) = channel::<ProofJob>();
        let (done_tx, rx) = channel::<ProofDone>();
        let spawned = std::thread::Builder::new()
            .name("mpp-proof".into())
            .stack_size(32 * 1024 * 1024)
            .spawn(move || {
                while let Ok(mut job) = jobs.recv() {
                    while let Ok(newer) = jobs.try_recv() {
                        job = newer;
                    }
                    let check = Arc::new(check_text(&job.text, job.base.as_deref()));
                    let done = ProofDone {
                        generation: job.generation,
                        text: job.text,
                        check,
                    };
                    if done_tx.send(done).is_err() {
                        break;
                    }
                }
            });
        ProofWorker {
            tx: spawned.is_ok().then_some(tx),
            rx,
            sent: 0,
            received: 0,
        }
    }

    /// Asks for `text` to be checked.
    pub fn submit(&mut self, text: &str, base: Option<&Path>) {
        let Some(tx) = &self.tx else {
            return;
        };
        self.sent += 1;
        let job = ProofJob {
            generation: self.sent,
            text: text.to_string(),
            base: base.map(Path::to_path_buf),
        };
        if tx.send(job).is_err() {
            self.sent -= 1;
        }
    }

    /// The newest finished check, if one arrived.
    pub fn poll(&mut self) -> Option<ProofDone> {
        let mut latest = None;
        while let Ok(d) = self.rx.try_recv() {
            self.received = self.received.max(d.generation);
            latest = Some(d);
        }
        latest
    }

    /// Waits (up to `timeout`) for everything submitted so far.
    pub fn wait(&mut self, timeout: Duration) -> Option<ProofDone> {
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

    /// True while a submitted check has not come back.
    pub fn busy(&self) -> bool {
        self.received < self.sent
    }
}

/// The result (`Theorem 2`) whose statement or proof holds `offset`: a proof
/// belongs to the result before it.
pub fn result_at(doc: &Document, offset: usize) -> Option<String> {
    let mut count = 0usize;
    let mut current: Option<(String, Span)> = None;
    let mut found = None;
    for b in &doc.blocks {
        if let BlockKind::Theorem(t) = &b.kind {
            if is_result(t.kind) {
                count += 1;
                let n = t.number.clone().unwrap_or_else(|| count.to_string());
                current = Some((format!("{} {n}", t.kind.as_str()), b.span));
            }
            let inside = b.span.start <= offset && offset <= b.span.end;
            if inside && (is_result(t.kind) || t.kind == TheoremKind::Proof) {
                found = current.as_ref().map(|(n, _)| n.clone());
            }
        }
    }
    found
}

/// Every result of a document by name, in order.
pub fn result_names(doc: &Document) -> Vec<String> {
    let mut out = vec![];
    let mut count = 0usize;
    for b in &doc.blocks {
        if let BlockKind::Theorem(t) = &b.kind {
            if is_result(t.kind) {
                count += 1;
                let n = t.number.clone().unwrap_or_else(|| count.to_string());
                out.push(format!("{} {n}", t.kind.as_str()));
            }
        }
    }
    out
}

/// Prose with `$…$` maths as one typesettable LaTeX string (words in `\text{}`).
pub fn prose_latex(src: &str) -> String {
    let mut out = String::new();
    for (i, part) in src.split('$').enumerate() {
        if part.is_empty() {
            continue;
        }
        if i % 2 == 1 {
            out.push(' ');
            out.push_str(part.trim());
            out.push(' ');
        } else {
            let clean: String = part
                .chars()
                .map(|c| match c {
                    '\\' | '{' | '}' | '%' | '&' | '#' | '_' | '^' | '~' | '*' => ' ',
                    c => c,
                })
                .collect();
            if !clean.trim().is_empty() {
                out.push_str("\\text{");
                out.push_str(&clean);
                out.push('}');
            }
        }
    }
    out.trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    const SRC: &str = "**Lemma 1.** For every real $x$, $(x+1)^2 = x^2 + 2x + 1$.\n\n**Proof.**\n1. $(x+1)^2 = x^2 + 2x + 1$ — by algebra.\n∎\n\n**Theorem 2.** It holds.\n\n**Proof.**\n1. It holds — *(gap: not shown)*.\n∎\n";

    #[test]
    fn summaries_follow_theorems_and_marks_need_a_fresh_report() {
        let doc = parse_document(SRC);
        let check = check_text(SRC, None);
        let v = ProofView::new(&doc, &check, true);
        assert_eq!(v.summaries.len(), 2);
        assert_eq!(v.summaries[0].text, "proved");
        assert_eq!(v.summaries[0].tone, Tone::Proved);
        assert_eq!(v.summaries[1].tone, Tone::Weak);
        assert_eq!(v.steps.len(), 2);
        assert_eq!(v.steps[0].status, StepStatus::Symbolic);
        assert_eq!(v.steps[1].status, StepStatus::Gap);
        let stale = ProofView::new(&doc, &check, false);
        assert_eq!(stale.summaries.len(), 2);
        assert!(stale.steps.is_empty());
    }

    #[test]
    fn the_result_at_a_place_includes_its_proof() {
        let doc = parse_document(SRC);
        let at = SRC.find("It holds —").unwrap_or(0);
        assert_eq!(result_at(&doc, at).as_deref(), Some("Theorem 2"));
        assert_eq!(result_at(&doc, 2).as_deref(), Some("Lemma 1"));
        assert_eq!(result_names(&doc), ["Lemma 1", "Theorem 2"]);
    }

    #[test]
    fn the_worker_checks_in_the_background() {
        let mut w = ProofWorker::new();
        w.submit(SRC, None);
        let done = w.wait(Duration::from_secs(30)).expect("a report");
        assert_eq!(done.check.results.len(), 2);
        assert!(!w.busy());
    }

    #[test]
    fn prose_is_typeset_with_its_maths() {
        assert_eq!(
            prose_latex("For every $n$, $n = n$."),
            "\\text{For every } n \\text{, } n = n \\text{.}"
        );
    }
}
