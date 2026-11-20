//! A session: runs a document and re-runs only what an edit changes.

use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use mpp_syntax::document::{block_at, parse_document, Block, BlockId, BlockKind, Document};
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
    number_format: mpp_core::display::NumberFormat,
    evaluated: Evaluated,
    /// "Show steps": every `= ?` query records its working, as if written `= ?!`.
    show_steps: bool,
    /// Auto-evaluation (C8): the toggle, the time budget and the answer cache.
    auto: AutoEval,
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
            number_format: mpp_core::display::NumberFormat::default(),
            evaluated: Evaluated::empty(),
            show_steps: false,
            auto: AutoEval::default(),
        }
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
        let doc = parse_document(src);
        let lines = LineIndex::new(src);
        let sequences = sequence_names(&doc);
        if sequences != self.sequences {
            // Whether `a_1` is an initial term depends on the whole document.
            self.cache.clear();
            self.sequences = sequences;
        }
        let inputs = Inputs {
            sequences: &self.sequences,
            holders: &self.holders,
            importer: self.importer.as_ref(),
            base_dir: self.base_dir.as_deref(),
            root_dir: self.root_dir.as_deref(),
            chain: &self.chain,
            doc_name: self.doc_name.as_deref(),
            lines: &lines,
            show_steps: self.show_steps,
            auto: &self.auto,
        };

        let pass = if own_stack {
            with_large_stack(|| run_blocks(&doc, &self.cache, &inputs))
        } else {
            run_blocks(&doc, &self.cache, &inputs)
        };
        let Pass {
            env,
            cache,
            mut blocks,
            recomputed,
        } = pass;
        self.cache = cache;

        let diagnostics = attach_parse_diagnostics(&doc, &mut blocks);
        fill_in_hints(&mut blocks);
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
fn run_blocks(doc: &Document, old: &HashMap<BlockId, Cached>, inputs: &Inputs<'_>) -> Pass {
    let mut flat: Vec<&Block> = Vec::new();
    doc.walk(&mut |b| flat.push(b));

    let mut env = Env::default();
    let mut cache = HashMap::with_capacity(flat.len());
    let mut blocks = Vec::with_capacity(flat.len());
    let mut recomputed = Vec::new();
    for block in flat {
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
                run_block(block, &mut env, inputs)
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
    }
    Pass {
        env,
        cache,
        blocks,
        recomputed,
    }
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

/// Every name with a recurrence `a_{n+1} := …` anywhere in the document.
fn sequence_names(doc: &Document) -> BTreeSet<String> {
    let mut names = BTreeSet::new();
    let mut scan = |ast: &MathAst| {
        for line in &ast.lines {
            if let Statement::Definition {
                head: DefHead::Recurrence { name, .. },
                ..
            } = &line.statement
            {
                names.extend(node_key(name));
            }
        }
    };
    doc.walk(&mut |b| for_each_math(b, &mut scan));
    names
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
