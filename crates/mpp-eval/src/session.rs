//! A session: runs a document and re-runs only what an edit changes.

use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use mpp_syntax::document::{
    block_at, citation_diagnostics, parse_document, parse_reference_entries, Block, BlockId,
    BlockKind, Document, TheoremKind,
};
use mpp_syntax::math::{DefHead, MathAst, Statement};
use mpp_syntax::Diagnostic;

use crate::auto::AutoEval;
use crate::block::{run_block, Inputs, Outcome};
use crate::files::{canonical, FileImporter};
use crate::graph::DepGraph;
use crate::holders::Holders;
use crate::import::Importer;
use crate::names::node_key;
use crate::result::{shift_span, BlockResult, Evaluated, LineIndex};
use crate::value::{same_binding, Env};

/// The longest one query may run in the command line and the calculator before it
/// stops with "stopped after N s".
pub const DEFAULT_TIME_LIMIT: Duration = Duration::from_secs(60);

/// A block's last outcome, kept so an unchanged block can be reused.
#[derive(Debug, Clone)]
struct Cached {
    outcome: Outcome,
    start: usize,
}

/// Runs `.mpp` documents. Keep one per open file: [`Session::update`] re-runs only
/// the blocks an edit changed and the blocks downstream of changed definitions.
pub struct Session {
    holders: Arc<Holders>,
    importer: Arc<dyn Importer>,
    base_dir: Option<PathBuf>,
    /// The folder the import tree started from; shown file names are relative to it.
    root_dir: Option<PathBuf>,
    /// The files being loaded, outermost first, for the import loop check.
    chain: Vec<PathBuf>,
    /// The name of this file in messages.
    doc_name: Option<Arc<str>>,
    cache: HashMap<BlockId, Cached>,
    sequences: BTreeSet<String>,
    /// Names called with an argument somewhere they are not differentiated.
    called: BTreeSet<String>,
    /// Names differentiated in an equation somewhere in the document.
    ode_unknowns: BTreeSet<String>,
    functions: BTreeSet<String>,
    /// The document has a proof, so its displayed statements are the proof checker's.
    has_proofs: bool,
    number_format: mpp_core::display::NumberFormat,
    evaluated: Evaluated,
    /// "Show steps": every `= ?` query records its working, as if written `= ?!`.
    show_steps: bool,
    /// Auto-evaluation (C8): the toggle, the time budget and the answer cache.
    auto: AutoEval,
    /// Set (from another thread) to stop the run in progress; cleared when a run starts.
    cancel: Arc<AtomicBool>,
    /// Longest a single query (one block) may run; `None` is no limit.
    time_limit: Option<Duration>,
}

/// Stops the run a [`Session`] is busy with, from any thread.
#[derive(Clone, Debug)]
pub struct CancelHandle(Arc<AtomicBool>);

impl CancelHandle {
    /// Asks the running calculation to stop at its next check. The session's next
    /// run starts afresh (the request only affects work already in progress).
    pub fn cancel(&self) {
        self.0.store(true, Ordering::Relaxed);
    }
}

impl std::fmt::Debug for Session {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Session")
            .field("holders", &self.holders)
            .field("base_dir", &self.base_dir)
            .field("cached_blocks", &self.cache.len())
            .finish()
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        self.auto.cache.cancel_all();
    }
}

impl Default for Session {
    fn default() -> Self {
        Session::new()
    }
}

impl Session {
    /// A session with the built-in holder evaluators and a file importer.
    pub fn new() -> Self {
        Session {
            holders: Arc::new(Holders::new()),
            importer: Arc::new(FileImporter::new()),
            base_dir: None,
            root_dir: None,
            chain: Vec::new(),
            doc_name: None,
            cache: HashMap::new(),
            sequences: BTreeSet::new(),
            called: BTreeSet::new(),
            ode_unknowns: BTreeSet::new(),
            functions: BTreeSet::new(),
            has_proofs: false,
            number_format: mpp_core::display::NumberFormat::default(),
            evaluated: Evaluated::empty(),
            show_steps: false,
            auto: AutoEval::default(),
            cancel: Arc::new(AtomicBool::new(false)),
            time_limit: None,
        }
    }

    /// A handle that stops the run in progress (for example when the user types).
    /// Call it *before* handing the session the newer text.
    pub fn cancel_handle(&self) -> CancelHandle {
        CancelHandle(self.cancel.clone())
    }

    /// Limits how long one query (one block) may run; a longer one stops with
    /// "stopped after N s". `None` (the default) is no limit.
    pub fn set_time_limit(&mut self, limit: Option<Duration>) {
        self.time_limit = limit;
    }

    /// The per-query time limit.
    pub fn time_limit(&self) -> Option<Duration> {
        self.time_limit
    }

    /// Turns the "Show steps" option on or off: when on, every `= ?` query records its
    /// working as if it were written `= ?!` (forgets cached results).
    pub fn set_show_steps(&mut self, on: bool) {
        if self.show_steps != on {
            self.show_steps = on;
            self.cache.clear();
        }
    }

    /// True when the "Show steps" option is on.
    pub fn show_steps(&self) -> bool {
        self.show_steps
    }

    /// Turns auto-evaluation (the grey `= …` results of display maths) on or off for
    /// the whole document; `%noeval` still switches it off for one block. The next run
    /// or update applies it.
    pub fn set_auto_eval(&mut self, on: bool) {
        if self.auto.enabled != on {
            self.auto.enabled = on;
            self.cache.clear();
        }
    }

    /// True when auto-evaluation is on (the default).
    pub fn auto_eval(&self) -> bool {
        self.auto.enabled
    }

    /// How long each block may spend on its auto results before they are left
    /// `pending` and finished on a worker thread. `None` (the default) works everything
    /// out in place; the IDE sets a budget so a slow integral never holds up typing.
    pub fn set_auto_budget(&mut self, budget: Option<std::time::Duration>) {
        self.auto.budget = budget;
    }

    /// The time budget per block for auto results.
    pub fn auto_budget(&self) -> Option<std::time::Duration> {
        self.auto.budget
    }

    /// True while some auto result is still being worked out.
    pub fn has_pending(&self) -> bool {
        self.evaluated
            .blocks
            .iter()
            .any(|b| b.values.iter().any(|l| l.pending))
    }

    /// Waits until every running auto result is done, or `timeout` passes; true when
    /// none is running any more. Call [`Session::refresh`] afterwards to show them.
    pub fn wait_pending(&self, timeout: std::time::Duration) -> bool {
        self.auto.cache.wait(timeout)
    }

    /// True when a pending auto result has finished since the last run, so
    /// [`Session::refresh`] would show something new.
    pub fn pending_ready(&self) -> bool {
        self.has_pending() && !self.auto.cache.has_running()
    }

    /// Runs the current text again, putting in the auto results that finished since
    /// the last run (only the blocks that were waiting run again).
    pub fn refresh(&mut self) -> &Evaluated {
        let src = self.evaluated.source.clone();
        self.update(&src)
    }

    /// Replaces the holder registry (and forgets cached results).
    pub fn with_holders(mut self, holders: Holders) -> Self {
        self.holders = Arc::new(holders);
        self.cache.clear();
        self.auto.cache.clear();
        self.importer.invalidate(None);
        self
    }

    /// The holder registry, to register more evaluators (forgets cached results).
    pub fn holders_mut(&mut self) -> &mut Holders {
        self.cache.clear();
        self.auto.cache.clear();
        self.importer.invalidate(None);
        Arc::make_mut(&mut self.holders)
    }

    /// Adds a field of maths whose operators documents can then call; the standard
    /// domains stay (forgets cached results).
    pub fn register_domain(&mut self, domain: Box<dyn mpp_core::Domain>) {
        self.holders_mut().register_domain(domain);
    }

    /// Replaces the whole domain registry ([`mpp_core::Registry::standard`] is the
    /// default; forgets cached results).
    pub fn with_registry(mut self, registry: mpp_core::Registry) -> Self {
        self.holders_mut().set_domains(registry);
        self
    }

    /// Replaces the importer (and forgets cached results).
    pub fn with_importer(mut self, importer: Arc<dyn Importer>) -> Self {
        self.importer = importer;
        self.cache.clear();
        self
    }

    /// The importer, to share its cache with a [`crate::FileWatcher`].
    pub fn importer(&self) -> Arc<dyn Importer> {
        self.importer.clone()
    }

    /// The folder imports are resolved against.
    pub fn set_base_dir(&mut self, dir: Option<PathBuf>) {
        self.base_dir = dir;
        self.cache.clear();
    }

    /// Says which file the next runs are of: imports resolve against its folder,
    /// an import of the file itself is reported as a loop, and messages name it.
    pub fn set_file(&mut self, path: impl AsRef<Path>) {
        let key = canonical(path.as_ref());
        self.base_dir = key.parent().map(Path::to_path_buf);
        self.root_dir = self.base_dir.clone();
        self.doc_name = key
            .file_name()
            .map(|n| Arc::from(n.to_string_lossy().as_ref()));
        self.chain = vec![key];
        self.cache.clear();
    }

    /// Forgets the cached copy of the imported file `path` because it changed on
    /// disk. The next [`Session::update`] reads it again and re-runs only the
    /// blocks that import it and the blocks that use what it defines.
    pub fn invalidate(&mut self, path: impl AsRef<Path>) {
        self.importer.invalidate(Some(path.as_ref()));
    }

    /// The display format a file starts with (the IDE's settings); `%` directives in
    /// the file change it from their line on. Takes effect at the next run.
    pub fn set_number_format(&mut self, format: mpp_core::display::NumberFormat) {
        self.number_format = format;
    }

    /// The last result.
    pub fn evaluated(&self) -> &Evaluated {
        &self.evaluated
    }

    /// Runs `src` from scratch.
    pub fn run_str(&mut self, src: &str) -> &Evaluated {
        self.cache.clear();
        self.update(src)
    }

    /// Reads and runs a file; imports are resolved against its folder.
    pub fn run_file(&mut self, path: impl AsRef<Path>) -> std::io::Result<&Evaluated> {
        let path = path.as_ref();
        let text = std::fs::read_to_string(path)?;
        self.set_file(path);
        Ok(self.run_str(&text))
    }

    /// A session that runs an imported file inside an import tree.
    pub(crate) fn for_import(
        holders: Arc<Holders>,
        importer: Arc<dyn Importer>,
        base_dir: Option<PathBuf>,
        root_dir: Option<PathBuf>,
        chain: Vec<PathBuf>,
        doc_name: Arc<str>,
    ) -> Self {
        let mut session = Session::new();
        session.holders = holders;
        session.importer = importer;
        session.base_dir = base_dir;
        session.root_dir = root_dir;
        session.chain = chain;
        session.doc_name = Some(doc_name);
        // An imported file's blocks are not shown, so nothing is auto-evaluated there.
        session.auto.enabled = false;
        session
    }

    /// Runs an imported file on the current thread (the caller already has the
    /// large evaluation stack).
    pub(crate) fn run_nested(&mut self, src: &str) -> &Evaluated {
        self.cache.clear();
        self.update_with(src, false)
    }

    /// Runs the new text of the document, reusing every block whose text is
    /// unchanged and whose inputs (the bindings it read) are unchanged.
    /// [`Evaluated::recomputed`] lists the blocks that ran.
    pub fn update(&mut self, src: &str) -> &Evaluated {
        self.update_with(src, true)
    }

    fn update_with(&mut self, src: &str, own_stack: bool) -> &Evaluated {
        let mut doc = parse_document(src);
        check_references_file(&mut doc, self.importer.as_ref(), self.base_dir.as_deref());
        let lines = LineIndex::new(src);
        let (sequences, functions) = sequence_names(&doc);
        let (called, ode_unknowns) = called_names(&doc);
        let has_proofs = has_proofs(&doc);
        if sequences != self.sequences
            || called != self.called
            || ode_unknowns != self.ode_unknowns
            || functions != self.functions
            || has_proofs != self.has_proofs
        {
            // Whether `a_1` is an initial term, and whether a differential equation
            // defines its unknown, depend on the whole document.
            self.cache.clear();
            self.sequences = sequences;
            self.called = called;
            self.ode_unknowns = ode_unknowns;
            self.functions = functions;
            self.has_proofs = has_proofs;
        }
        let inputs = Inputs {
            sequences: &self.sequences,
            called: &self.called,
            ode_unknowns: &self.ode_unknowns,
            functions: &self.functions,
            source: src,
            holders: &self.holders,
            importer: self.importer.as_ref(),
            base_dir: self.base_dir.as_deref(),
            root_dir: self.root_dir.as_deref(),
            chain: &self.chain,
            doc_name: self.doc_name.as_deref(),
            lines: &lines,
            show_steps: self.show_steps,
            auto: &self.auto,
            has_proofs: self.has_proofs,
        };

        let text_changed = own_stack && src != self.evaluated.source;
        if own_stack {
            self.auto.cache.begin_pass();
        }
        let pass = if own_stack {
            // A cancel asked for before this run began belongs to the run it
            // replaced; one asked for from here on stops this run.
            self.cancel.store(false, Ordering::Relaxed);
            let budget = mpp_core::budget::Budget::new(self.cancel.clone(), self.time_limit);
            with_large_stack(|| {
                let _budget = mpp_core::budget::install(budget.clone());
                run_blocks(&doc, &self.cache, &inputs, true)
            })
        } else {
            run_blocks(&doc, &self.cache, &inputs, false)
        };
        if text_changed {
            // Slow answers for text that is gone are not worth finishing.
            self.auto.cache.sweep();
        }
        let Pass {
            env,
            cache,
            mut blocks,
            recomputed,
        } = pass;
        self.cache = cache;

        let mut diagnostics = attach_parse_diagnostics(&doc, &mut blocks);
        fill_in_hints(&mut blocks);
        crate::unanswered::flag_unanswered(src, &lines, &mut blocks, &mut diagnostics);
        let graph = DepGraph::build(&blocks);
        let mut evaluated = Evaluated {
            document: doc,
            blocks,
            recomputed,
            diagnostics,
            bindings: env.into_map(),
            graph,
            source: src.to_string(),
            lines,
        };
        crate::numfmt::apply(&mut evaluated, &self.number_format);
        // Keep the lines as shown, so a block reused on the next edit is not formatted
        // again.
        for b in &evaluated.blocks {
            if let Some(c) = self.cache.get_mut(&b.id) {
                if c.outcome.result.values.len() == b.values.len() {
                    c.outcome.result.values.clone_from(&b.values);
                }
            }
        }
        self.evaluated = evaluated;
        &self.evaluated
    }
}

/// What one pass over the blocks produced.
struct Pass {
    env: Env,
    cache: HashMap<BlockId, Cached>,
    blocks: Vec<BlockResult>,
    recomputed: Vec<BlockId>,
}

/// Runs every block of `doc` in order, reusing cached outcomes where possible.
fn run_blocks(
    doc: &Document,
    old: &HashMap<BlockId, Cached>,
    inputs: &Inputs<'_>,
    top: bool,
) -> Pass {
    let mut flat: Vec<&Block> = Vec::new();
    doc.walk(&mut |b| flat.push(b));
    let scopes = proof_scopes(doc);

    let mut env = Env::default();
    let mut cache = HashMap::with_capacity(flat.len());
    let mut blocks = Vec::with_capacity(flat.len());
    let mut recomputed = Vec::new();
    // Names defined inside a proof belong to that proof: the names in scope before
    // it come back once its last block is done.
    let mut saved: Vec<(usize, Env)> = Vec::new();
    for (i, block) in flat.into_iter().enumerate() {
        if let Some(&(_, last)) = scopes.iter().find(|(start, _)| *start == i) {
            saved.push((last, env.clone()));
        }
        let reusable = old.get(&block.id).filter(|c| {
            !c.outcome.positional
                && !c.outcome.volatile
                && c.outcome
                    .reads
                    .iter()
                    .all(|(name, seen)| same_binding(env.get(name), seen.as_ref()))
        });
        let outcome = match reusable {
            Some(c) => {
                let mut outcome = c.outcome.clone();
                let delta = block.span.start as isize - c.start as isize;
                if delta != 0 {
                    shift_outcome(&mut outcome, delta);
                }
                for (name, binding) in &outcome.effects {
                    env.insert(name.clone(), binding.clone());
                }
                outcome
            }
            None => {
                recomputed.push(block.id);
                if top {
                    // One time limit per query, not per document.
                    mpp_core::budget::restart();
                }
                let mut outcome = run_block(block, &mut env, inputs);
                if top
                    && mpp_core::budget::take_tripped() == Some(mpp_core::budget::Trip::Cancelled)
                {
                    // A stopped block says nothing about its text: run it again next time.
                    outcome.volatile = true;
                }
                outcome
            }
        };
        blocks.push(outcome.result.clone());
        cache.insert(
            block.id,
            Cached {
                outcome,
                start: block.span.start,
            },
        );
        while saved.last().is_some_and(|(last, _)| *last == i) {
            if let Some((_, before)) = saved.pop() {
                env = before;
            }
        }
    }
    Pass {
        env,
        cache,
        blocks,
        recomputed,
    }
}

/// The proofs of a document as `(first, last)` indices into its [`Document::walk`]
/// order: the proof block itself and its last nested block.
fn proof_scopes(doc: &Document) -> Vec<(usize, usize)> {
    fn go(blocks: &[Block], next: &mut usize, out: &mut Vec<(usize, usize)>) {
        for b in blocks {
            let at = *next;
            *next += 1;
            if let BlockKind::Theorem(t) = &b.kind {
                go(&t.body, next, out);
                if t.kind == TheoremKind::Proof {
                    out.push((at, *next - 1));
                }
            }
        }
    }
    let mut out = Vec::new();
    go(&doc.blocks, &mut 0, &mut out);
    out.sort_unstable();
    out
}

/// Stack for evaluation: recursive user functions and deep expressions recurse.
const EVAL_STACK: usize = 256 << 20;

/// Runs `f` on a scoped thread with a large stack, or on this thread when no
/// thread can be started.
fn with_large_stack<T: Send>(f: impl Fn() -> T + Sync) -> T {
    std::thread::scope(|scope| {
        let worker = std::thread::Builder::new()
            .name("mpp-eval".into())
            .stack_size(EVAL_STACK)
            .spawn_scoped(scope, &f);
        match worker {
            Ok(handle) => match handle.join() {
                Ok(out) => out,
                Err(panic) => std::panic::resume_unwind(panic),
            },
            Err(_) => f(),
        }
    })
}

fn shift_outcome(outcome: &mut Outcome, delta: isize) {
    let id = outcome.result.id;
    outcome.result.shift(delta);
    for (_, binding) in &mut outcome.effects {
        if binding.origin().block == id && binding.origin().file.is_none() {
            let mut b = (**binding).clone();
            let origin = b.origin_mut();
            origin.span = shift_span(origin.span, delta);
            *binding = Arc::new(b);
        }
    }
}

/// Reads the file of a `References from [[file]]` block and checks every `[@key]`
/// against its entries (the parser cannot, since it never reads files). A file that
/// cannot be read is an error on the block.
fn check_references_file(doc: &mut Document, importer: &dyn Importer, base: Option<&Path>) {
    if !doc.has_references_file() {
        return;
    }
    let mut files = Vec::new();
    doc.walk(&mut |b| {
        if let BlockKind::References(r) = &b.kind {
            if let Some((path, span)) = &r.from {
                files.push((path.clone(), *span));
            }
        }
    });
    let mut entries = Vec::new();
    let mut extra = Vec::new();
    for (path, span) in files {
        match importer.read_data(base, &path) {
            Ok((_, text)) => entries.extend(parse_reference_entries(&text)),
            Err(why) => extra.push(Diagnostic::error(
                span,
                format!("the references in {path} could not be read: {why}"),
            )),
        }
    }
    if extra.is_empty() {
        extra.extend(citation_diagnostics(doc, &entries));
    }
    doc.diagnostics.extend(extra);
    doc.diagnostics.sort_by_key(|d| (d.span.start, d.span.end));
}

/// Puts each parser diagnostic in the innermost block it falls in; returns the rest.
fn attach_parse_diagnostics(doc: &Document, blocks: &mut [BlockResult]) -> Vec<Diagnostic> {
    let mut loose = Vec::new();
    for d in &doc.diagnostics {
        let target =
            block_at(doc, d.span.start).and_then(|b| blocks.iter_mut().find(|r| r.id == b.id));
        match target {
            Some(r) => r.diagnostics.push(d.clone()),
            None => loose.push(d.clone()),
        }
    }
    for b in blocks.iter_mut() {
        b.diagnostics.sort_by_key(|d| (d.span.start, d.span.end));
    }
    loose
}

/// "fill in: upper bound" for a display line that could not be read because a part is
/// missing (an empty template box written `{}`, or `\int_0^{} …`): the parser's
/// "expected the upper bound after `^`" names the part.
fn fill_in_hints(blocks: &mut [BlockResult]) {
    for b in blocks.iter_mut() {
        let diagnostics = &b.diagnostics;
        for line in b.values.iter_mut() {
            if !line.auto_eval || line.input.is_some() || line.hint.is_some() {
                continue;
            }
            let missing = diagnostics
                .iter()
                .filter(|d| line.span.start <= d.span.start && d.span.end <= line.span.end)
                .find_map(|d| missing_part(&d.message));
            if let Some(part) = missing {
                line.hint = Some(format!("fill in: {part}"));
            }
        }
    }
}

/// The part a parser message says is missing: "expected the upper bound after `^`" →
/// "upper bound", "expected an expression inside `{}` …" → "the empty box".
fn missing_part(message: &str) -> Option<String> {
    let rest = message.strip_prefix("expected ")?;
    if rest.starts_with("an expression inside `{}`") || rest.starts_with("an expression") {
        return Some(crate::lower::long_label(""));
    }
    let what = rest.strip_prefix("the ")?;
    let end = [" after ", " to ", " before "]
        .iter()
        .filter_map(|w| what.find(w))
        .min()
        .unwrap_or(what.len());
    let part = what[..end].trim();
    (!part.is_empty() && !part.contains('`')).then(|| part.to_string())
}

/// Every name with a recurrence `a_{n+1} := …` anywhere in the document, and every
/// name defined as a function (`f(x) := …`) anywhere in it.
fn sequence_names(doc: &Document) -> (BTreeSet<String>, BTreeSet<String>) {
    let mut names = BTreeSet::new();
    let mut functions = BTreeSet::new();
    let mut scan = |ast: &MathAst| {
        for line in &ast.lines {
            match &line.statement {
                Statement::Definition {
                    head: DefHead::Recurrence { name, .. },
                    ..
                } => names.extend(node_key(name)),
                Statement::Definition {
                    head: DefHead::Function { name, .. },
                    ..
                } => functions.extend(node_key(name)),
                _ => {}
            }
        }
    };
    doc.walk(&mut |b| for_each_math(b, &mut scan));
    (names, functions)
}

/// Names called with an argument (`N(10\,\mathrm{yr})`, `x(T)`, `V_C(\tau)`) in a line
/// or figure where the same name is not differentiated: a differential equation in
/// such a name defines it for those uses. A name only ever met inside differential
/// equations (with their conditions `y(0) = 1`) stays an unknown of each equation.
///
/// Also returns the names differentiated in a line that holds an equation: the
/// unknowns of the document's differential equations.
fn called_names(doc: &Document) -> (BTreeSet<String>, BTreeSet<String>) {
    use mpp_syntax::math::{visit, Accent, Clause, Line, Node, NodeKind};
    fn walk(n: &Node, calls: &mut Vec<String>, diffs: &mut Vec<String>) {
        match &n.kind {
            NodeKind::Call { callee, args, .. } if !args.is_empty() => {
                if let Some(k) = node_key(callee) {
                    calls.push(k);
                }
            }
            NodeKind::Prime { base, .. } => diffs.extend(node_key(base)),
            NodeKind::Derivative(d) => {
                if let Some(of) = &d.of {
                    diffs.extend(node_key(of.ungrouped()));
                }
            }
            NodeKind::Accented {
                accent: Accent::Dot | Accent::DDot,
                inner,
            } => diffs.extend(node_key(inner)),
            NodeKind::Name(name) if matches!(name.accent, Some(Accent::Dot | Accent::DDot)) => {
                diffs.push(name.base.clone());
            }
            _ => {}
        }
        visit::children(n, &mut |c| walk(c, calls, diffs));
    }
    fn has_equation(n: &Node) -> bool {
        let mut found = matches!(n.kind, NodeKind::Relation { .. });
        visit::children(n, &mut |c| found = found || has_equation(c));
        found
    }
    fn line_calls(line: &Line, out: &mut BTreeSet<String>, unknowns: &mut BTreeSet<String>) {
        let (mut calls, mut diffs) = (Vec::new(), Vec::new());
        let mut nodes: Vec<&Node> = Vec::new();
        match &line.statement {
            Statement::Definition { body, .. } => nodes.push(body),
            Statement::Query { expr, .. } | Statement::Relation(expr) => nodes.push(expr),
            Statement::Check { lhs, rhs } => {
                nodes.push(lhs);
                nodes.push(rhs);
            }
            _ => {}
        }
        for c in &line.clauses {
            if let Clause::Condition(n) | Clause::Where { value: n, .. } = c {
                nodes.push(n);
            }
        }
        let equation = nodes.iter().any(|n| has_equation(n));
        for n in nodes {
            walk(n, &mut calls, &mut diffs);
        }
        if equation {
            unknowns.extend(diffs.iter().cloned());
        } else if matches!(line.statement, Statement::Definition { .. }) {
            // `E(t) := ½ m ẋ² + ½ k x²`: a definition that differentiates x uses it
            // as a function.
            out.extend(diffs.iter().cloned());
        }
        out.extend(calls.into_iter().filter(|c| !diffs.contains(c)));
    }
    let mut names = BTreeSet::new();
    let mut unknowns = BTreeSet::new();
    let mut scan = |ast: &MathAst| {
        for line in &ast.lines {
            line_calls(line, &mut names, &mut unknowns);
        }
    };
    doc.walk(&mut |b| {
        for_each_math(b, &mut scan);
        if let BlockKind::Figure(f) = &b.kind {
            for item in &f.items {
                if let mpp_syntax::document::FigureItemKind::Expr(ast) = &item.kind {
                    scan(ast);
                }
            }
        }
    });
    (names, unknowns)
}

/// The names a document defines, read from its syntax alone: nothing is evaluated, so
/// this stays fast however slow the document's queries are. It covers `:=` and `≡`
/// definitions anywhere in maths, `Let $a = 3$.` sentences and `Data:` tables; names
/// only the evaluator creates (random variables, a table's columns) are not listed.
pub fn defined_names(src: &str) -> Vec<String> {
    use mpp_syntax::math::{NodeKind, RelOp};
    let doc = parse_document(src);
    let (sequences, _) = sequence_names(&doc);
    let mut out: Vec<String> = Vec::new();
    let heads = |ast: &MathAst, lets: bool, out: &mut Vec<String>| {
        for line in &ast.lines {
            match &line.statement {
                Statement::Definition { head, .. } => {
                    let key = match head {
                        DefHead::Name(n) => node_key(n),
                        DefHead::Function { name, .. } | DefHead::Recurrence { name, .. } => {
                            node_key(name)
                        }
                        // `v_0 := 20` is the variable `v_0` unless `v` has a recurrence.
                        DefHead::Initial { name, index } => node_key(name).and_then(|k| {
                            if sequences.contains(&k) {
                                Some(k)
                            } else {
                                crate::names::sub_text(index).map(|i| format!("{k}_{i}"))
                            }
                        }),
                        _ => None,
                    };
                    out.extend(key);
                }
                Statement::Relation(node) if lets => {
                    if let NodeKind::Relation { first, rest } = &node.kind {
                        if let [(RelOp::Eq, _)] = rest.as_slice() {
                            let key = node_key(first).or_else(|| match &first.kind {
                                NodeKind::Call { callee, .. } => node_key(callee),
                                _ => None,
                            });
                            out.extend(key);
                        }
                    }
                }
                _ => {}
            }
        }
    };
    doc.walk(&mut |b| match &b.kind {
        BlockKind::Data(d) => out.extend(node_key(&d.name)),
        BlockKind::Let(l) => {
            for m in l.math() {
                heads(&m.ast, true, &mut out);
            }
        }
        _ => for_each_math(b, &mut |ast| heads(ast, false, &mut out)),
    });
    out.retain(|n| !crate::result::is_internal_name(n));
    out.sort();
    out.dedup();
    out
}

/// Calls `f` on every maths span directly inside `block`.
fn for_each_math(block: &Block, f: &mut impl FnMut(&MathAst)) {
    use mpp_syntax::document::{Inline, InlineKind, List};
    fn inlines(items: &[Inline], f: &mut impl FnMut(&MathAst)) {
        for i in items {
            match &i.kind {
                InlineKind::Math(m) => f(&m.ast),
                InlineKind::Emphasis(inner) | InlineKind::Strong(inner) => inlines(inner, f),
                InlineKind::Link { text, .. } => inlines(text, f),
                _ => {}
            }
        }
    }
    fn list(l: &List, f: &mut impl FnMut(&MathAst)) {
        for item in &l.items {
            inlines(&item.content, f);
            for child in &item.children {
                list(child, f);
            }
        }
    }
    match &block.kind {
        BlockKind::DisplayMath(m) => f(&m.ast),
        BlockKind::Let(l) => inlines(&l.content, f),
        BlockKind::Heading { content, .. }
        | BlockKind::Paragraph(content)
        | BlockKind::FootnoteDef { content, .. } => inlines(content, f),
        BlockKind::List(l) => list(l, f),
        BlockKind::Table(t) => {
            for cell in t.header.iter().chain(t.rows.iter().flatten()) {
                inlines(cell, f);
            }
        }
        BlockKind::Quote(ps) => {
            for p in ps {
                inlines(p, f);
            }
        }
        _ => {}
    }
}

/// True when the document has a proof (a `**Proof.**` block).
fn has_proofs(doc: &mpp_syntax::document::Document) -> bool {
    let mut found = false;
    doc.walk(&mut |b| {
        if let mpp_syntax::document::BlockKind::Theorem(t) = &b.kind {
            found |= t.kind == mpp_syntax::document::TheoremKind::Proof;
        }
    });
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_is_send() {
        fn check<T: Send>() {}
        check::<Session>();
        check::<Evaluated>();
    }
}
