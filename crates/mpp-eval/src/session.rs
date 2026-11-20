//! A session: runs a document and re-runs only what an edit changes.

use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use mpp_syntax::document::{block_at, parse_document, Block, BlockId, BlockKind, Document};
use mpp_syntax::math::{DefHead, MathAst, Statement};
use mpp_syntax::Diagnostic;

use crate::block::{run_block, Inputs, Outcome};
use crate::graph::DepGraph;
use crate::holders::Holders;
use crate::import::{Importer, NoImports};
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
    cache: HashMap<BlockId, Cached>,
    sequences: BTreeSet<String>,
    evaluated: Evaluated,
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
    /// A session with the built-in holder evaluators and no import support.
    pub fn new() -> Self {
        Session {
            holders: Arc::new(Holders::new()),
            importer: Arc::new(NoImports),
            base_dir: None,
            cache: HashMap::new(),
            sequences: BTreeSet::new(),
            evaluated: Evaluated::empty(),
        }
    }

    /// Replaces the holder registry (and forgets cached results).
    pub fn with_holders(mut self, holders: Holders) -> Self {
        self.holders = Arc::new(holders);
        self.cache.clear();
        self
    }

    /// The holder registry, to register more evaluators (forgets cached results).
    pub fn holders_mut(&mut self) -> &mut Holders {
        self.cache.clear();
        Arc::make_mut(&mut self.holders)
    }

    /// Replaces the importer (and forgets cached results).
    pub fn with_importer(mut self, importer: Arc<dyn Importer>) -> Self {
        self.importer = importer;
        self.cache.clear();
        self
    }

    /// The folder imports are resolved against.
    pub fn set_base_dir(&mut self, dir: Option<PathBuf>) {
        self.base_dir = dir;
        self.cache.clear();
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
        self.base_dir = path.parent().map(Path::to_path_buf);
        Ok(self.run_str(&text))
    }

    /// Runs the new text of the document, reusing every block whose text is
    /// unchanged and whose inputs (the bindings it read) are unchanged.
    /// [`Evaluated::recomputed`] lists the blocks that ran.
    pub fn update(&mut self, src: &str) -> &Evaluated {
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
            lines: &lines,
        };

        let pass = with_large_stack(|| run_blocks(&doc, &self.cache, &inputs));
        let Pass {
            env,
            cache,
            mut blocks,
            recomputed,
        } = pass;
        self.cache = cache;

        let diagnostics = attach_parse_diagnostics(&doc, &mut blocks);
        let graph = DepGraph::build(&blocks);
        self.evaluated = Evaluated {
            document: doc,
            blocks,
            recomputed,
            diagnostics,
            bindings: env.into_map(),
            graph,
            source: src.to_string(),
            lines,
        };
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
