//! Proofs on the command line: the checker's report for `mpp check`, `mpp prove`
//! (the proof agent), `mpp key` (the API key) and `mpp lean` (the Lean export).

use std::io::Write;
use std::path::{Path, PathBuf};

use mpp_proof::agent::{self, AgentConfig, KeyStore, Mode, RunStop, Task, Transport};
use mpp_syntax::document::{parse_document, parse_reference_entries, BlockKind, Document};

use crate::to_unicode;

/// Prose with `$…$` maths, the maths turned to Unicode (`$x^2$` → `x²`).
pub fn prose_unicode(s: &str) -> String {
    s.split('$')
        .enumerate()
        .map(|(i, part)| {
            if i % 2 == 1 {
                to_unicode(part)
            } else {
                part.to_string()
            }
        })
        .collect()
}

/// The entries of a document's `References from [[file]]` files.
fn extra_references(doc: &Document, base: Option<&Path>) -> Vec<mpp_syntax::document::Reference> {
    let mut refs = Vec::new();
    doc.walk(&mut |b| {
        if let BlockKind::References(r) = &b.kind {
            if let Some((path, _)) = &r.from {
                let file = base.map_or_else(|| PathBuf::from(path), |d| d.join(path));
                if let Ok(text) = mpp_eval::read_text(file) {
                    refs.extend(parse_reference_entries(&text));
                }
            }
        }
    });
    refs
}

/// The checker's report on a document.
pub fn check_proofs(src: &str, base: Option<&Path>) -> mpp_proof::DocumentCheck {
    let doc = parse_document(src);
    let refs = extra_references(&doc, base);
    mpp_proof::check_document(&doc, src, &refs)
}

/// The proof checker's report of a file: one line per theorem with its summary,
/// then one line per step with its status mark. Empty when the file has no
/// theorem-like blocks. `base` is the file's folder (for `References from [[…]]`).
pub fn proof_report(src: &str, base: Option<&Path>) -> String {
    prose_unicode(&check_proofs(src, base).report())
}

/// The outcome of `mpp prove`, for its exit code: 0 when the target ends proved,
/// 1 when not, 2 on errors.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProveOutcome {
    Proved,
    NotProved,
    Error,
}

impl ProveOutcome {
    /// The process exit code.
    pub fn code(self) -> u8 {
        match self {
            ProveOutcome::Proved => 0,
            ProveOutcome::NotProved => 1,
            ProveOutcome::Error => 2,
        }
    }
}

/// What `mpp prove` was asked.
pub struct ProveArgs<'a> {
    pub path: &'a Path,
    /// The result to prove (`Theorem 2`); `None` proves the first result in the file.
    pub target: Option<String>,
    /// Propose conjectures about this topic instead.
    pub conjecture: Option<String>,
    pub cap_usd: f64,
    /// Write the draft without asking.
    pub yes: bool,
}

/// Where `mpp prove` writes and what it reads from the user.
pub struct ProveIo<'a> {
    pub out: &'a mut dyn Write,
    pub err: &'a mut dyn Write,
    /// The answer to "Write this change?".
    pub answer: &'a mut dyn FnMut() -> String,
}

/// `mpp prove file.mpp[#name]`: runs the proof agent on a result and shows the
/// checker's verdict, the agent's words (labelled as such) and the diff. The file
/// is written only after `y` or with `--yes`. `transport` is the real API when `None`.
pub fn prove(
    args: &ProveArgs<'_>,
    store: &dyn KeyStore,
    transport: Option<&dyn Transport>,
    config: &AgentConfig,
    io: &mut ProveIo<'_>,
) -> ProveOutcome {
    if !agent::available(store) {
        let _ = writeln!(
            io.err,
            "error: the proof agent is off: set an API key first (mpp key set)"
        );
        return ProveOutcome::Error;
    }
    if !(args.cap_usd.is_finite() && args.cap_usd >= 0.01) {
        let _ = writeln!(io.err, "error: set a spend cap of at least $0.01 (--cap)");
        return ProveOutcome::Error;
    }
    let source = match mpp_eval::read_text(args.path) {
        Ok(s) => s,
        Err(e) => {
            let _ = writeln!(io.err, "error: could not read {}: {e}", args.path.display());
            return ProveOutcome::Error;
        }
    };
    let check = check_proofs(&source, args.path.parent());
    let (mode, target) = match &args.conjecture {
        Some(topic) => (Mode::Conjecture, topic.clone()),
        None => {
            let target = match (&args.target, check.results.first()) {
                (Some(t), _) => t.clone(),
                (None, Some(r)) => r.name.clone(),
                (None, None) => {
                    let _ = writeln!(io.err, "error: the file has no theorem to prove");
                    return ProveOutcome::Error;
                }
            };
            if check.result(&target).is_none() {
                let names: Vec<&str> = check.results.iter().map(|r| r.name.as_str()).collect();
                let _ = writeln!(
                    io.err,
                    "error: no result named {target} (the file has: {})",
                    names.join(", ")
                );
                return ProveOutcome::Error;
            }
            (Mode::Prove, target)
        }
    };
    let task = Task {
        path: args.path.to_path_buf(),
        source,
        target: target.clone(),
        mode,
    };
    let config = AgentConfig {
        spend_cap_usd: args.cap_usd,
        ..config.clone()
    };
    let real;
    let transport: &dyn Transport = match transport {
        Some(t) => t,
        None => match agent::ReqwestTransport::new() {
            Ok(t) => {
                real = t;
                &real
            }
            Err(e) => {
                let _ = writeln!(io.err, "error: the API could not be reached: {}", e.0);
                return ProveOutcome::Error;
            }
        },
    };
    let _ = writeln!(io.out, "Proving {target} (spend cap ${:.2})", args.cap_usd);
    let err = &mut *io.err;
    let report = agent::run(&task, &config, store, transport, &mut |m| {
        let _ = write!(err, "\r  cost {}   ", m.label());
        let _ = err.flush();
    });
    let _ = writeln!(io.err);
    let report = match report {
        Ok(r) => r,
        Err(e) => {
            let _ = writeln!(io.err, "error: {e}");
            return ProveOutcome::Error;
        }
    };
    let out = &mut *io.out;
    let _ = writeln!(out, "Cost: {}", report.cost.label());
    let stop = match &report.stop {
        RunStop::Finished => "the agent finished".to_string(),
        RunStop::SpendCap => "stopped at the spend cap".to_string(),
        RunStop::BudgetReached => "the session reached its budget".to_string(),
        RunStop::Refused(why) => format!("the model declined: {why}"),
        RunStop::Error(e) => format!("the session ended with an error: {e}"),
        RunStop::TooLong => "the run took too long".to_string(),
        RunStop::Stopped => "stopped".to_string(),
    };
    let _ = writeln!(out, "Run: {stop}");
    if !report.verdict_text.is_empty() {
        let _ = writeln!(out, "Checker: {target}: {}", report.verdict_text);
    }
    if !report.agent_says.trim().is_empty() {
        let _ = writeln!(out, "The agent says: {}", report.agent_says.trim());
    }
    if let (Some(a), Some(e)) = (&report.agent_id, &report.environment_id) {
        let _ = writeln!(out, "Agent {a}, environment {e}");
    }
    let proved = mode == Mode::Prove
        && report
            .checked
            .result(&target)
            .is_some_and(|r| r.summary.verdict.is_proved());
    let outcome = if proved {
        ProveOutcome::Proved
    } else {
        ProveOutcome::NotProved
    };
    let Some(p) = report.proposal.as_ref().filter(|p| p.changes_anything()) else {
        let _ = writeln!(out, "No change to the file was proposed.");
        return outcome;
    };
    let _ = write!(out, "{}", prose_unicode(&p.diff));
    let write = if args.yes {
        true
    } else {
        let _ = write!(out, "Write this change to {}? [y/N] ", args.path.display());
        let _ = out.flush();
        (io.answer)().trim().eq_ignore_ascii_case("y")
    };
    if !write {
        let _ = writeln!(out, "The file was not changed.");
        return outcome;
    }
    if let Err(e) = std::fs::write(args.path, p.accept()) {
        let _ = writeln!(
            io.err,
            "error: could not write {}: {e}",
            args.path.display()
        );
        return ProveOutcome::Error;
    }
    let _ = writeln!(out, "Wrote {}", args.path.display());
    outcome
}

/// Splits `file.mpp#Theorem 2` (also `#theorem-2`) into the path and the result.
pub fn split_target(arg: &str) -> (PathBuf, Option<String>) {
    match agent::parse_target(arg) {
        Some((p, name)) if p.extension().is_some_and(|e| e == "mpp") => (p, Some(name)),
        _ => (PathBuf::from(arg), None),
    }
}

/// `mpp lean file.mpp`: writes the Lean project; with `build`, runs `lake build`.
pub fn lean(path: &Path, out_dir: &Path, build: bool) -> Result<String, String> {
    let src =
        mpp_eval::read_text(path).map_err(|e| format!("could not read {}: {e}", path.display()))?;
    let doc = parse_document(&src);
    let check = check_proofs(&src, path.parent());
    let module = path.file_stem().map_or_else(
        || "document".to_string(),
        |s| s.to_string_lossy().to_string(),
    );
    let config = mpp_proof::lean::LeanConfig::default();
    let export = mpp_proof::lean::export(&doc, &src, &check, &module, &config);
    let mut msg = format!(
        "wrote a Lean project with {} statement(s) to {}\n",
        export.theorems.len(),
        out_dir.display()
    );
    if !build {
        export
            .write(out_dir)
            .map_err(|e| format!("could not write {}: {e}", out_dir.display()))?;
        return Ok(msg);
    }
    match mpp_proof::lean::build(&export, out_dir, &config) {
        Ok(o) if o.success => msg.push_str("lake build succeeded\n"),
        Ok(o) => msg.push_str(&format!(
            "lake build failed with {} error(s)\n",
            o.errors.len()
        )),
        Err(mpp_proof::lean::LakeError::NotInstalled) => {
            export
                .write(out_dir)
                .map_err(|e| format!("could not write {}: {e}", out_dir.display()))?;
            msg.push_str("Lean is not installed, so nothing was built\n");
        }
        Err(e) => return Err(e.to_string()),
    }
    Ok(msg)
}
