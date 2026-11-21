//! The proof agent's custom tools. They run inside Math++ on the user's PC; the agent
//! only sees their text results.
//!
//! - `check_step`: the checker's status for a step of the current draft. The agent reads
//!   statuses; it can never set one.
//! - `evaluate`: one line of maths through the quick calculator.
//! - `search_library`: theorem headings in the document's folder that match a query.
//! - `read_file`: a `.mpp`, `.txt` or `.csv` file in the document's folder (no other path).
//! - `lean_check`: the Lean export of a result, and `lake build` when Lean is installed.
//! - `write_steps`: replaces the target's proof in the draft (or adds a conjecture). The
//!   file itself is never touched: the user sees the draft as a diff and accepts it.

use std::path::{Component, Path, PathBuf};

use serde_json::{json, Value};

use crate::checker::{check_source, result_spans};
use crate::lean::{self, LeanConfig};
use crate::model::{ClaimOutcome, DocumentCheck};

/// Status marks the agent may not write: only the checker shows them.
const STATUS_MARKS: &[char] = &['✓', '≈', '⊢', '◇', '📖', '⚠', '✗'];

/// The tool definitions sent to the API (custom tools with JSON schemas).
pub fn definitions() -> Vec<Value> {
    vec![
        tool(
            "check_step",
            "Run the Math++ proof checker on the current draft and return the status of one step (or of every step when `step` is omitted) of a result, with the reason. Statuses: ✓ symbolic, ≈ numeric, ⊢ Lean, ◇ structure only, 📖 cited, ⚠ gap or external, ✗ wrong. Only the checker sets statuses.",
            json!({
                "type": "object",
                "properties": {
                    "result": {"type": "string", "description": "The result's name, such as \"Lemma 2\" or \"Theorem 6\"."},
                    "step": {"type": "integer", "description": "The step number; omit for every step."}
                },
                "required": ["result"],
                "additionalProperties": false
            }),
        ),
        tool(
            "evaluate",
            "Work out one line of maths written as in a .mpp maths block (LaTeX): an expression, `= ?`, `≈ ?`, or a check with \\overset{?}{=}. Use it to test conjectures numerically before trying to prove them.",
            json!({
                "type": "object",
                "properties": {"maths": {"type": "string"}},
                "required": ["maths"],
                "additionalProperties": false
            }),
        ),
        tool(
            "search_library",
            "Search the theorem headings and statements of the .mpp files in the document's folder for a phrase. Returns up to 10 matches as `file: line`.",
            json!({
                "type": "object",
                "properties": {"query": {"type": "string"}},
                "required": ["query"],
                "additionalProperties": false
            }),
        ),
        tool(
            "read_file",
            "Read a .mpp, .txt or .csv file in the document's folder, by a path relative to it.",
            json!({
                "type": "object",
                "properties": {"path": {"type": "string"}},
                "required": ["path"],
                "additionalProperties": false
            }),
        ),
        tool(
            "lean_check",
            "Export a result of the current draft to Lean 4 + Mathlib and, if Lean is installed, run lake build. Returns the Lean statement, the alignment warnings and any compile errors.",
            json!({
                "type": "object",
                "properties": {"result": {"type": "string"}},
                "required": ["result"],
                "additionalProperties": false
            }),
        ),
        tool(
            "write_steps",
            "Write the proof of the target result in the draft as numbered steps, each `N. claim — by reason.` (or a note *(gap: …)*, *(cited: … [@key])*, *(external: [@key])*). Replaces the target's proof in the draft only. Or, with `conjecture`, add a Conjecture block: it is tested numerically first and refused if a counterexample is found. Never write status marks; the checker sets them. Returns the checker's statuses for the new draft.",
            json!({
                "type": "object",
                "properties": {
                    "result": {"type": "string", "description": "The target result's name."},
                    "steps": {"type": "string", "description": "The numbered steps, one per line."},
                    "conjecture": {"type": "string", "description": "A conjecture's statement as one line of maths (LaTeX), instead of a proof."}
                },
                "additionalProperties": false
            }),
        ),
    ]
}

fn tool(name: &str, description: &str, schema: Value) -> Value {
    json!({
        "type": "custom",
        "name": name,
        "description": description,
        "input_schema": schema
    })
}

/// The document being worked on: the original text and the agent's draft.
#[derive(Debug, Clone)]
pub struct Workspace {
    /// The `.mpp` file (its folder bounds `read_file` and `search_library`).
    pub path: PathBuf,
    pub original: String,
    pub draft: String,
    /// The result the agent was asked to prove.
    pub target: String,
    original_check: DocumentCheck,
    /// Every call made, for the run report.
    pub calls: Vec<ToolCall>,
}

/// One tool call and its result.
#[derive(Debug, Clone, PartialEq)]
pub struct ToolCall {
    pub name: String,
    pub input: String,
    pub output: String,
    pub is_error: bool,
}

impl Workspace {
    pub fn new(path: PathBuf, source: String, target: String) -> Self {
        let original_check = check_source(&source);
        Workspace {
            path,
            draft: source.clone(),
            original: source,
            target,
            original_check,
            calls: Vec::new(),
        }
    }

    /// The checker's report on the original document.
    pub fn original_check(&self) -> &DocumentCheck {
        &self.original_check
    }

    /// Runs a tool. The result text goes back to the agent; `true` marks an error.
    pub fn run(&mut self, name: &str, input: &Value) -> (String, bool) {
        let out = match name {
            "check_step" => self.check_step(input),
            "evaluate" => evaluate(input),
            "search_library" => self.search_library(input),
            "read_file" => self.read_file(input),
            "lean_check" => self.lean_check(input),
            "write_steps" => self.write_steps(input),
            other => Err(format!("there is no tool called `{other}`")),
        };
        let (text, err) = match out {
            Ok(t) => (t, false),
            Err(e) => (e, true),
        };
        self.calls.push(ToolCall {
            name: name.to_string(),
            input: input.to_string(),
            output: text.clone(),
            is_error: err,
        });
        (text, err)
    }

    fn folder(&self) -> PathBuf {
        self.path
            .parent()
            .map_or_else(|| PathBuf::from("."), Path::to_path_buf)
    }

    fn check_step(&self, input: &Value) -> Result<String, String> {
        let result = str_field(input, "result")?;
        let step = input.get("step").and_then(Value::as_u64);
        let check = check_source(&self.draft);
        let r = check
            .result(&result)
            .ok_or_else(|| format!("there is no result called {result}"))?;
        let mut out = format!("{}: {}\n", r.name, r.summary.text);
        for p in &r.proofs {
            for s in &p.steps {
                if step.is_some_and(|n| s.number != u32::try_from(n).ok()) {
                    continue;
                }
                out.push_str(&format!(
                    "step {}: {} {} — {}\n",
                    s.number.map_or_else(|| "-".into(), |n| n.to_string()),
                    s.status.mark(),
                    s.status.name(),
                    s.detail
                ));
            }
            for i in &p.issues {
                out.push_str(&format!("issue: {i}\n"));
            }
        }
        Ok(out)
    }

    fn search_library(&self, input: &Value) -> Result<String, String> {
        let query = str_field(input, "query")?.to_lowercase();
        if query.trim().is_empty() {
            return Err("the query is empty".into());
        }
        let mut hits = Vec::new();
        let mut files: Vec<PathBuf> = std::fs::read_dir(self.folder())
            .map_err(|e| format!("the folder could not be read: {e}"))?
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| p.extension().is_some_and(|x| x == "mpp"))
            .collect();
        files.sort();
        for f in files {
            let Ok(text) = std::fs::read_to_string(&f) else {
                continue;
            };
            for (n, line) in text.lines().enumerate() {
                let l = line.trim_start();
                let heading = l.starts_with("**Theorem")
                    || l.starts_with("**Lemma")
                    || l.starts_with("**Corollary")
                    || l.starts_with("**Proposition")
                    || l.starts_with("**Conjecture")
                    || l.starts_with("**Definition");
                if heading && l.to_lowercase().contains(&query) {
                    let name = f
                        .file_name()
                        .map(|s| s.to_string_lossy().to_string())
                        .unwrap_or_default();
                    hits.push(format!(
                        "{name}:{}: {}",
                        n + 1,
                        l.chars().take(200).collect::<String>()
                    ));
                    if hits.len() >= 10 {
                        break;
                    }
                }
            }
        }
        Ok(if hits.is_empty() {
            "no match".into()
        } else {
            hits.join("\n")
        })
    }

    fn read_file(&self, input: &Value) -> Result<String, String> {
        let rel = str_field(input, "path")?;
        let p = Path::new(&rel);
        if p.is_absolute()
            || p.components().any(|c| {
                matches!(
                    c,
                    Component::ParentDir | Component::RootDir | Component::Prefix(_)
                )
            })
        {
            return Err("only files inside the document's folder can be read".into());
        }
        let ext_ok = p
            .extension()
            .and_then(|e| e.to_str())
            .is_some_and(|e| matches!(e, "mpp" | "txt" | "csv"));
        if !ext_ok {
            return Err("only .mpp, .txt and .csv files can be read".into());
        }
        let full = self.folder().join(p);
        // A link inside the folder must not lead out of it.
        let real = std::fs::canonicalize(&full).map_err(|_| format!("there is no file {rel}"))?;
        let home = std::fs::canonicalize(self.folder())
            .map_err(|e| format!("the document's folder could not be read: {e}"))?;
        if !real.starts_with(&home) {
            return Err("only files inside the document's folder can be read".into());
        }
        let meta = std::fs::metadata(&full).map_err(|_| format!("there is no file {rel}"))?;
        if meta.len() > 200_000 {
            return Err("the file is larger than 200 kB".into());
        }
        std::fs::read_to_string(&full).map_err(|e| format!("{rel} could not be read: {e}"))
    }

    fn lean_check(&self, input: &Value) -> Result<String, String> {
        let result = str_field(input, "result")?;
        let doc = mpp_syntax::document::parse_document(&self.draft);
        let check = crate::checker::check_document(&doc, &self.draft, &[]);
        let stem = self.path.file_stem().map_or_else(
            || "document".to_string(),
            |s| s.to_string_lossy().to_string(),
        );
        let config = LeanConfig::default();
        let ex = lean::export(&doc, &self.draft, &check, &stem, &config);
        let t = ex
            .theorem(&result)
            .ok_or_else(|| format!("there is no result called {result}"))?;
        let mut out = format!(
            "Lean statement: {}\nalignment warnings: {}\n",
            t.alignment.lean.as_deref().unwrap_or("(not stated: sorry)"),
            t.alignment.warnings.join("; ")
        );
        if lean::find_lake().is_none() {
            out.push_str("Lean is not installed on this machine, so nothing was compiled.\n");
            return Ok(out);
        }
        let dir = std::env::temp_dir().join(format!("mpp-lean-agent-{}", std::process::id()));
        match lean::build(&ex, &dir, &config) {
            Ok(o) => {
                out.push_str(&format!(
                    "lake build: {}\n",
                    if o.success { "succeeded" } else { "failed" }
                ));
                for (line, msg) in o.errors.iter().take(20) {
                    out.push_str(&format!("line {line}: {msg}\n"));
                }
            }
            Err(e) => out.push_str(&format!("{e}\n")),
        }
        Ok(out)
    }

    fn write_steps(&mut self, input: &Value) -> Result<String, String> {
        if let Some(c) = input.get("conjecture").and_then(Value::as_str) {
            return self.add_conjecture(c, input.get("steps").and_then(Value::as_str));
        }
        let result = str_field(input, "result")?;
        if result != self.target {
            return Err(format!(
                "you may only write the proof of {}, the result you were asked to prove",
                self.target
            ));
        }
        let steps = str_field(input, "steps")?;
        check_written_text(&steps)?;
        let doc = mpp_syntax::document::parse_document(&self.draft);
        let spans = result_spans(&doc, &self.draft);
        let (_, statement, proof) = spans
            .iter()
            .find(|(n, _, _)| *n == result)
            .ok_or_else(|| format!("there is no result called {result}"))?;
        let block = format!("**Proof.**\n{}\n∎", steps.trim_end());
        let candidate = match proof {
            Some(p) => format!(
                "{}{}{}",
                &self.draft[..p.start],
                block,
                &self.draft[p.end..]
            ),
            None => {
                let end = statement.end;
                format!("{}\n\n{}{}", &self.draft[..end], block, &self.draft[end..])
            }
        };
        let after = check_source(&candidate);
        enforce_rules(
            &self.original_check,
            &after,
            &result,
            &self.original,
            &candidate,
        )?;
        self.draft = candidate;
        let r = after.result(&result);
        let mut out = format!(
            "draft updated. {}: {}\n",
            result,
            r.map_or_else(String::new, |r| r.summary.text.clone())
        );
        if let Some(r) = r {
            for p in r.proofs.iter().take(1) {
                for s in &p.steps {
                    out.push_str(&format!(
                        "step {}: {} {} — {}\n",
                        s.number.map_or_else(|| "-".into(), |n| n.to_string()),
                        s.status.mark(),
                        s.status.name(),
                        s.detail
                    ));
                }
            }
        }
        Ok(out)
    }

    fn add_conjecture(&mut self, statement: &str, steps: Option<&str>) -> Result<String, String> {
        check_written_text(statement)?;
        if let Some(s) = steps {
            check_written_text(s)?;
        }
        let statement = statement.trim().trim_matches('$').trim();
        // Test it numerically first: as the only claim of a one-step proof.
        // A conjecture claims its formula for every value of its names, so a point where
        // the sides differ refutes it.
        let probe = format!(
            "**Theorem 1.** ${statement}$\n\n**Proof.**\n1. For every real ${statement}$ — by algebra.\n∎\n"
        );
        let c = check_source(&probe);
        let step = c
            .results
            .first()
            .and_then(|r| r.proofs.first())
            .and_then(|p| p.steps.first());
        let tested =
            match step {
                Some(s) if s.claims.is_empty() => return Err(
                    "state the conjecture as a formula that can be tested (a relation in maths)"
                        .into(),
                ),
                Some(s) => s
                    .claims
                    .iter()
                    .map(|c| c.outcome.clone())
                    .collect::<Vec<_>>(),
                None => return Err("the conjecture could not be read".into()),
            };
        if let Some(ClaimOutcome::Failed(at)) =
            tested.iter().find(|o| matches!(o, ClaimOutcome::Failed(_)))
        {
            return Err(format!("the conjecture is false: {at}"));
        }
        if !tested.iter().all(|o| {
            matches!(
                o,
                ClaimOutcome::Proved | ClaimOutcome::ProvedBy(_) | ClaimOutcome::Numeric(_)
            )
        }) {
            return Err(
                "the conjecture could not be tested numerically; restate it so it can be".into(),
            );
        }
        let existing = check_source(&self.draft).results.len();
        let mut add = String::new();
        if !self.draft.contains("## Conjectures from the proof agent") {
            add.push_str("\n\n## Conjectures from the proof agent\n");
        }
        add.push_str(&format!(
            "\n**Conjecture {}.** ${statement}$\n",
            existing + 1
        ));
        if let Some(s) = steps {
            add.push_str(&format!("\n**Proof.**\n{}\n∎\n", s.trim_end()));
        }
        self.draft = format!("{}{}", self.draft.trim_end(), add);
        Ok(format!(
            "conjecture added; it agrees numerically ({}). Its proof, if any, is checked like any other.",
            tested
                .iter()
                .map(|o| match o {
                    ClaimOutcome::Numeric(h) => h.clone(),
                    _ => "proved symbolically".into(),
                })
                .collect::<Vec<_>>()
                .join("; ")
        ))
    }
}

fn str_field(input: &Value, name: &str) -> Result<String, String> {
    input
        .get(name)
        .and_then(Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| format!("the input needs a string field `{name}`"))
}

fn evaluate(input: &Value) -> Result<String, String> {
    let maths = str_field(input, "maths")?;
    mpp_eval::calc_line(&maths).map_err(|e| e.0)
}

/// Written text may not carry status marks or start new blocks.
fn check_written_text(t: &str) -> Result<(), String> {
    if let Some(m) = t.chars().find(|c| STATUS_MARKS.contains(c)) {
        return Err(format!(
            "do not write status marks such as {m}: only the checker sets a step's status"
        ));
    }
    for line in t.lines() {
        let l = line.trim_start();
        if l.starts_with('#')
            || l.starts_with("**")
            || l.starts_with("$$")
            || l.starts_with("References")
        {
            return Err(
                "write only numbered proof steps (no headings, theorem blocks or display maths)"
                    .into(),
            );
        }
    }
    Ok(())
}

/// The rules a draft must keep:
/// - an external, unverified result the original proof rests on stays (cite instead of
///   claiming: the agent cannot make it disappear);
/// - a proof that was not proved can only gain steps the checker verifies, cites, or
///   reads as real structure (no prose `— by algebra` steps with nothing to check);
/// - nothing outside the target's proof changes.
pub fn enforce_rules(
    before: &DocumentCheck,
    after: &DocumentCheck,
    target: &str,
    original: &str,
    candidate: &str,
) -> Result<(), String> {
    for r in &before.results {
        if r.name == target {
            continue;
        }
        let same = after
            .result(&r.name)
            .is_some_and(|a| a.summary.verdict == r.summary.verdict);
        if !same {
            return Err(format!(
                "the draft changed {}, which is not the target",
                r.name
            ));
        }
    }
    let Some(b) = before.result(target) else {
        return Err(format!("there is no result called {target}"));
    };
    let Some(a) = after.result(target) else {
        return Err(format!("the draft lost {target}"));
    };
    for key in &b.summary.external {
        if !a.summary.external.contains(key) {
            return Err(format!(
                "the proof rests on the external, unverified result [@{key}]; keep it cited — you cannot replace it with a claim"
            ));
        }
    }
    if !b.summary.verdict.is_proved() {
        let old_steps: Vec<&str> = b
            .proofs
            .first()
            .map(|p| {
                p.steps
                    .iter()
                    .map(|s| span_text(original, s.span))
                    .collect()
            })
            .unwrap_or_default();
        if let Some(p) = a.proofs.first() {
            for s in &p.steps {
                let text = span_text(candidate, s.span);
                let new = !old_steps.contains(&text);
                if new && s.prose {
                    return Err(format!(
                        "step {} is prose with nothing the checker can verify; a gap can only be closed by a step the checker checks (✓ ≈ ⊢) or a cited result",
                        s.number.map_or_else(|| "-".into(), |n| n.to_string())
                    ));
                }
            }
        }
    }
    Ok(())
}

fn span_text(src: &str, span: mpp_syntax::Span) -> &str {
    src.get(span.start..span.end).unwrap_or("").trim()
}
