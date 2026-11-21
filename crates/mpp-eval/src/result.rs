//! The results of running a document: per block, per statement line.

use std::collections::{BTreeMap, HashMap};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use mpp_core::{Expr, Steps};
use mpp_syntax::document::{BlockId, Document};
use mpp_syntax::math::QueryMode;
use mpp_syntax::{Diagnostic, Severity, Span};

use crate::check::Verdict;
use crate::graph::DepGraph;
use crate::import::FileDiagnostic;
use crate::value::{Binding, Origin, Value};

/// What a definition line defined.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DefinitionKind {
    /// `a := …`
    Variable,
    /// `g ≡ …`
    Constant,
    /// `f(x) := …`
    Function,
    /// `a_{n+1} := …`
    Recurrence,
    /// `a_1 := …` for a sequence with a recurrence.
    Initial,
}

/// What a statement line is.
#[derive(Debug, Clone, PartialEq)]
pub enum LineKind {
    Definition {
        name: String,
        kind: DefinitionKind,
    },
    /// `= ?`, `≈ ?`, `= ?!`.
    Query {
        mode: QueryMode,
        steps: bool,
    },
    /// `\overset{?}{=}`.
    Check,
    /// A bare expression or relation chain: recorded, not evaluated.
    Relation,
    /// `x \in [a, b]` on its own.
    Range,
    /// An expression item of a `Figure:` line.
    Figure,
    /// The maths of a helper sentence (`Find the critical points of $f$`).
    Helper,
}

/// The result of one statement line.
#[derive(Debug, Clone, PartialEq)]
pub struct LineResult {
    /// Source span of the line.
    pub span: Span,
    pub kind: LineKind,
    /// The lowered input (before names were substituted).
    pub input: Option<Expr>,
    /// The exact result (for a definition, its value at this point).
    pub value: Option<Value>,
    /// The value as LaTeX.
    pub latex: Option<String>,
    /// The value as Unicode text for people to read (`9π`, `2xy`, `x² + 1`).
    pub text: Option<String>,
    /// The same value in the stable ASCII form of [`std::fmt::Display`] (`9*π`,
    /// `2*x*y`, `x^2 + 1`), for tools and tests.
    pub plain: Option<String>,
    /// The numeric value, 10 significant digits.
    pub approx: Option<String>,
    /// The verdict of a check.
    pub verdict: Option<Verdict>,
    /// Conditions written after the statement (`, x > 0`).
    pub conditions: Vec<Expr>,
    /// True when auto-evaluation may show a preview result for this line.
    pub auto_eval: bool,
    /// Marked `%noeval`.
    pub noeval: bool,
    /// The working of a `= ?!` query, step by step.
    pub steps: Option<Steps>,
    /// The display format the text was last shown under (see [`crate::numfmt`]).
    pub number_format: Option<crate::numfmt::Shown>,
    /// The value is an auto-evaluation result (C8): shown in grey after the line in the
    /// view only, never written to the file (see [`Evaluated::keep_result_text`]).
    pub auto: bool,
    /// An auto-evaluation is still running (the IDE shows a spinner);
    /// [`crate::Session::refresh`] picks the answer up when it is done.
    pub pending: bool,
    /// Why no answer is shown yet, such as "fill in: upper bound" for an empty box.
    pub hint: Option<String>,
    /// The line uses a decimal the user typed (`9.81 + 1`, `g + 1` after
    /// `g \equiv 9.81`): unless the file asks for `%exact`, its answer is shown as a
    /// decimal too (10.81, not 1081/100).
    pub decimal_input: bool,
}

impl LineResult {
    /// An empty result of `kind` for the line at `span`.
    pub fn new(span: Span, kind: LineKind) -> Self {
        LineResult {
            span,
            kind,
            input: None,
            value: None,
            latex: None,
            text: None,
            plain: None,
            approx: None,
            verdict: None,
            conditions: Vec::new(),
            auto_eval: false,
            noeval: false,
            steps: None,
            number_format: None,
            auto: false,
            pending: false,
            hint: None,
            decimal_input: false,
        }
    }

    /// The working as a foldable tree (top-level steps with their sub-steps), ending at
    /// the answer; `None` when no steps were recorded.
    pub fn step_tree(&self) -> Option<mpp_core::step_tree::StepTree> {
        let steps = self.steps.as_ref()?;
        let tree = match (&self.input, &self.value) {
            (Some(q), Some(v)) => mpp_core::step_tree::StepTree::for_answer(steps, q, &v.expr()),
            _ => mpp_core::step_tree::StepTree::from_steps(steps),
        };
        Some(tree)
    }

    /// Splits the shown text into its two forms: [`LineResult::plain`] keeps the text
    /// as it was made, and [`LineResult::text`] becomes the Unicode reading of the value
    /// when the text is just the value printed (a presentation of its own, such as a
    /// quantity with units or a table, is kept as it is).
    pub fn present_text(&mut self) {
        if self.plain.is_some() {
            // Already split (a presentation that came with its own Unicode text).
            return;
        }
        self.plain = self.text.clone();
        let (Some(t), Some(v)) = (&self.text, &self.value) else {
            return;
        };
        let unicode = match v {
            Value::Function { params, body } => (*t == format!("({}) ↦ {body}", params.join(", ")))
                .then(|| format!("({}) ↦ {}", params.join(", "), body.to_text())),
            Value::Table(table) => mpp_core::table::rows(table)
                .filter(|rows| *t == mpp_core::table::to_text(rows))
                .map(|rows| mpp_core::table::to_unicode(&rows)),
            other => {
                let e = other.expr();
                (*t == e.to_string()).then(|| e.to_text())
            }
        };
        match unicode {
            Some(u) => self.text = Some(u),
            // A presentation of its own made from plain printing: never the `*` sign.
            None => self.text = Some(crate::error::readable(t)),
        }
    }

    /// The answer as [`LineResult::answer`] gives it, in the plain ASCII form.
    pub fn plain_answer(&self) -> Option<String> {
        let mut plain = self.clone();
        plain.text = self.plain.clone().or_else(|| self.text.clone());
        plain.answer()
    }

    /// Sets the value with its LaTeX and plain text.
    pub fn set_value(&mut self, value: Value) {
        match &value {
            Value::Function { params, body } => {
                self.latex = Some(format!(
                    "({}) \\mapsto {}",
                    params.join(", "),
                    body.to_latex()
                ));
                self.text = Some(format!("({}) ↦ {body}", params.join(", ")));
            }
            Value::Table(t) => {
                let rows = mpp_core::table::rows(t).unwrap_or_default();
                self.latex = Some(mpp_core::table::to_latex(&rows));
                self.text = Some(mpp_core::table::to_text(&rows));
            }
            other => {
                let e = other.expr();
                self.latex = Some(e.to_latex());
                self.text = Some(e.to_string());
            }
        }
        self.value = Some(value);
    }

    /// True when the value is built from numbers, constants, sums, products and powers
    /// only (`π/2`, `2√2`, `(1 - i)/2`): an exact closed form worth showing as it is,
    /// unlike an unevaluated call such as `erf(1/2)`.
    pub fn is_closed_form(&self) -> bool {
        fn closed(e: &Expr) -> bool {
            use mpp_core::ExprNode as N;
            match e.node() {
                // A free name or a call with no arguments, in a value that has a decimal,
                // is a named constant (`γ`).
                N::Num(_) | N::Const(_) | N::Sym(_) => true,
                N::Apply(_, args) => args.is_empty(),
                N::Add(_) | N::Mul(_) | N::Pow(..) => e.children().into_iter().all(closed),
                _ => false,
            }
        }
        match &self.value {
            Some(Value::Function { .. } | Value::Table(_)) | None => false,
            Some(v) => closed(&v.expr()),
        }
    }

    /// True when an exact query got no simplification at all (`ζ(3) = ?`, `J₀(1) = ?`):
    /// the exact text only repeats what was asked, so writing it would show a tautology
    /// (`ζ(3) = ζ(3)`). The answer is then the decimal, shown with `≈` everywhere: in
    /// `mpp run`, the editor and every export.
    pub fn shows_decimal_only(&self) -> bool {
        if self.verdict.is_some() || self.approx.is_none() {
            return false;
        }
        if !matches!(
            self.kind,
            LineKind::Helper
                | LineKind::Query {
                    mode: QueryMode::Exact,
                    ..
                }
        ) {
            return false;
        }
        let plain = self.plain.as_ref().or(self.text.as_ref());
        match (plain, &self.input) {
            (None, _) => true,
            // The lowered input is already simplified (`√8` lowers to `2√2`), so an
            // exact closed form built from numbers, constants and powers is never
            // "unchanged": `= ?` keeps it.
            (Some(t), Some(input)) => *t == input.to_string() && !self.is_closed_form(),
            (Some(_), None) => false,
        }
    }

    /// The answer as one line of text (`= 2`, `≈ 1.414213562`, `✓ …`).
    pub fn answer(&self) -> Option<String> {
        if let Some(v) = &self.verdict {
            return Some(v.describe());
        }
        match (&self.kind, &self.text, &self.approx) {
            (
                LineKind::Query {
                    mode: QueryMode::Approx,
                    ..
                },
                _,
                Some(a),
            ) => Some(format!("≈ {a}")),
            (_, _, Some(a)) if self.shows_decimal_only() => Some(format!("≈ {a}")),
            (_, Some(t), Some(a)) if t != a => Some(format!("= {t} ≈ {a}")),
            (_, Some(t), _) => Some(format!("= {t}")),
            (_, None, Some(a)) => Some(format!("≈ {a}")),
            _ => None,
        }
    }
}

/// Whether a range is a slider or a plotting range.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RangeKind {
    /// `a := 2, a \in [0, 5]`: a slider for the figures and the IDE.
    Slider,
    /// `x \in [0, R]` in a `Figure:` line.
    Plot,
}

/// A range written with `\in`.
#[derive(Debug, Clone, PartialEq)]
pub struct RangeInfo {
    pub names: Vec<String>,
    /// The set, with known names substituted (usually an interval).
    pub set: Expr,
    pub kind: RangeKind,
    pub span: Span,
}

/// An import the block asked for.
#[derive(Debug, Clone, PartialEq)]
pub struct ImportInfo {
    pub path: String,
    pub span: Span,
    /// `(name in the file, name here)`, or `None` for "import everything".
    pub names: Option<Vec<(String, String)>>,
    /// True when the importer supplied the file.
    pub loaded: bool,
    /// The resolved path of the file.
    pub file: Option<Arc<Path>>,
    /// The file and every file it imports in turn (what to watch).
    pub files: Vec<PathBuf>,
    /// Everything reported inside the imported files, in full. The import line
    /// itself only carries a summary such as "2 errors in mechanics.mpp".
    pub diagnostics: Vec<FileDiagnostic>,
    /// The result depends on the importer's context (a loop or a missing file).
    pub uncacheable: bool,
}

/// Everything one block produced.
#[derive(Debug, Clone, PartialEq)]
pub struct BlockResult {
    pub id: BlockId,
    pub span: Span,
    /// Names this block defines, in order.
    pub defines: Vec<String>,
    /// Names this block looks up from the blocks before it.
    pub uses: Vec<String>,
    /// One result per statement line.
    pub values: Vec<LineResult>,
    /// Parser and evaluation problems inside this block.
    pub diagnostics: Vec<Diagnostic>,
    /// Working for `= ?!` lines, by index into `values`.
    pub steps: Vec<(usize, Steps)>,
    pub ranges: Vec<RangeInfo>,
    pub imports: Vec<ImportInfo>,
}

impl BlockResult {
    /// An empty result for a block.
    pub fn new(id: BlockId, span: Span) -> Self {
        BlockResult {
            id,
            span,
            defines: Vec::new(),
            uses: Vec::new(),
            values: Vec::new(),
            diagnostics: Vec::new(),
            steps: Vec::new(),
            ranges: Vec::new(),
            imports: Vec::new(),
        }
    }

    /// The steps recorded for `values[index]`.
    pub fn steps_for(&self, index: usize) -> Option<&Steps> {
        self.steps.iter().find(|(i, _)| *i == index).map(|(_, s)| s)
    }

    /// True when any diagnostic is an error.
    pub fn has_errors(&self) -> bool {
        self.diagnostics
            .iter()
            .any(|d| d.severity == Severity::Error)
    }

    /// Moves every span by `delta` bytes (the block moved in the file).
    pub(crate) fn shift(&mut self, delta: isize) {
        let mv = |s: &mut Span| *s = shift_span(*s, delta);
        mv(&mut self.span);
        self.values.iter_mut().for_each(|v| mv(&mut v.span));
        self.diagnostics.iter_mut().for_each(|d| mv(&mut d.span));
        self.ranges.iter_mut().for_each(|r| mv(&mut r.span));
        self.imports.iter_mut().for_each(|i| mv(&mut i.span));
    }
}

/// `span` moved by `delta` bytes.
pub(crate) fn shift_span(span: Span, delta: isize) -> Span {
    let start = span.start.checked_add_signed(delta).unwrap_or(0);
    let end = span.end.checked_add_signed(delta).unwrap_or(start);
    Span::new(start, end)
}

/// Line numbers for byte offsets.
#[derive(Debug, Clone, Default)]
pub struct LineIndex {
    starts: Vec<usize>,
}

impl LineIndex {
    /// Indexes `src`.
    pub fn new(src: &str) -> Self {
        let mut starts = vec![0];
        starts.extend(src.match_indices('\n').map(|(i, _)| i + 1));
        LineIndex { starts }
    }

    /// The 1-based line holding byte `offset`.
    pub fn line_of(&self, offset: usize) -> usize {
        self.starts.partition_point(|&s| s <= offset).max(1)
    }
}

/// The result of running a whole document.
#[derive(Debug, Clone)]
pub struct Evaluated {
    /// The parse of the text (shared with the editor, which parsed the same text).
    pub document: Arc<Document>,
    /// One result per block, nested blocks included, in document order.
    pub blocks: Vec<BlockResult>,
    /// The blocks that ran in the last run or update (the others were reused).
    pub recomputed: Vec<BlockId>,
    /// Problems not inside any block.
    pub diagnostics: Vec<Diagnostic>,
    /// The names in scope at the end of the document.
    pub bindings: BTreeMap<String, Arc<Binding>>,
    /// Which blocks define and use which names.
    pub graph: DepGraph,
    pub(crate) source: String,
    pub(crate) lines: LineIndex,
    /// Where each block's result is in [`Evaluated::blocks`] (see [`Evaluated::block`]).
    pub(crate) index: HashMap<BlockId, usize>,
}

/// True for the evaluator's own hidden keys in the environment (the document seed,
/// a function's denominators as written, random-variable declarations …). Names
/// read from a document never contain `:`, so they never collide with these.
pub fn is_internal_name(name: &str) -> bool {
    name.contains(':')
}

impl Evaluated {
    /// The names a reader defined, in order, as the outline, the variables panel and
    /// autocomplete offer them: hidden keys are left out, and a random-variable
    /// declaration is offered under its variable's own name.
    pub fn user_names(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        for name in self.bindings.keys() {
            let shown = if crate::random::is_random_key(name) {
                name.split_once(':').map(|(_, n)| n.to_string())
            } else if is_internal_name(name) {
                None
            } else {
                Some(name.clone())
            };
            if let Some(n) = shown {
                if !out.contains(&n) {
                    out.push(n);
                }
            }
        }
        out
    }

    /// The result of an empty document.
    pub fn empty() -> Self {
        Evaluated {
            document: Arc::new(mpp_syntax::document::parse_document("")),
            blocks: Vec::new(),
            recomputed: Vec::new(),
            diagnostics: Vec::new(),
            bindings: BTreeMap::new(),
            graph: DepGraph::default(),
            source: String::new(),
            lines: LineIndex::default(),
            index: HashMap::new(),
        }
    }

    /// Where `name` was defined: the document itself, or the file it was imported
    /// from (with the byte span and line of the definition in that file). Use it
    /// for go-to-definition across files.
    pub fn definition_of(&self, name: &str) -> Option<Origin> {
        self.bindings.get(name).map(|b| b.origin().clone())
    }

    /// Every file imported by the document, directly or through other files.
    pub fn imported_files(&self) -> Vec<PathBuf> {
        let mut files: Vec<PathBuf> = Vec::new();
        for import in self.blocks.iter().flat_map(|b| b.imports.iter()) {
            for f in &import.files {
                if !files.contains(f) {
                    files.push(f.clone());
                }
            }
        }
        files
    }

    /// The source text that was run.
    pub fn source(&self) -> &str {
        &self.source
    }

    /// The 1-based line of a byte offset.
    pub fn line_of(&self, offset: usize) -> usize {
        self.lines.line_of(offset)
    }

    /// The result of the block `id`.
    pub fn block(&self, id: BlockId) -> Option<&BlockResult> {
        // The page asks for every block of a long file: look it up, and fall back to
        // a search if the list was changed since the index was made.
        match self.index.get(&id).and_then(|&i| self.blocks.get(i)) {
            Some(b) if b.id == id => Some(b),
            _ => self.blocks.iter().find(|b| b.id == id),
        }
    }

    /// Makes the index [`Evaluated::block`] reads.
    pub(crate) fn index_blocks(&mut self) {
        self.index = self
            .blocks
            .iter()
            .enumerate()
            .rev()
            .map(|(i, b)| (b.id, i))
            .collect();
    }

    /// Every diagnostic, block ones included, in source order.
    pub fn all_diagnostics(&self) -> Vec<&Diagnostic> {
        let mut all: Vec<&Diagnostic> = self
            .diagnostics
            .iter()
            .chain(self.blocks.iter().flat_map(|b| b.diagnostics.iter()))
            .collect();
        all.sort_by_key(|d| (d.span.start, d.span.end));
        all
    }

    /// The errors among [`Evaluated::all_diagnostics`].
    pub fn errors(&self) -> Vec<&Diagnostic> {
        self.all_diagnostics()
            .into_iter()
            .filter(|d| d.severity == Severity::Error)
            .collect()
    }

    /// Every query and check line with its block.
    pub fn queries(&self) -> impl Iterator<Item = (&BlockResult, &LineResult)> {
        self.blocks.iter().flat_map(|b| {
            b.values
                .iter()
                .filter(|v| matches!(v.kind, LineKind::Query { .. } | LineKind::Check))
                .map(move |v| (b, v))
        })
    }

    /// Every line with an auto-evaluation result (C8), pending or hinted line, with its
    /// block.
    pub fn auto_results(&self) -> impl Iterator<Item = (&BlockResult, &LineResult)> {
        self.blocks.iter().flat_map(|b| {
            b.values
                .iter()
                .filter(|v| v.auto || v.pending || v.hint.is_some())
                .map(move |v| (b, v))
        })
    }

    /// `Ctrl+Enter` (keep the result): the new text for line `line` of block `block`
    /// (an index into its [`BlockResult::values`]), with its auto result written into
    /// the document as ordinary maths: `\int_0^1 x^2 \, dx = \frac{1}{3}`, or for an
    /// equation `x^2 = 4 \implies x = -2 \lor x = 2`. The IDE replaces the text of the
    /// line's span with it. A line with no auto result comes back unchanged.
    pub fn keep_result_text(&self, block: BlockId, line: usize) -> String {
        let Some(lr) = self
            .blocks
            .iter()
            .find(|b| b.id == block)
            .and_then(|b| b.values.get(line))
        else {
            return String::new();
        };
        let span = lr.span;
        let original = self
            .source
            .get(span.start..span.end)
            .unwrap_or("")
            .to_string();
        if !lr.auto {
            return original;
        }
        let shown = match (&lr.latex, &lr.approx, &lr.text) {
            (_, Some(a), None) => format!(" \\approx {a}"),
            (Some(l), _, _) => {
                let l = l.replace("\\;\\text{or}\\;", "\\lor");
                let is_equation = matches!(
                    lr.input.as_ref().map(Expr::node),
                    Some(mpp_core::ExprNode::Relation(..))
                );
                if is_equation {
                    format!(" \\implies {l}")
                } else {
                    format!(" = {l}")
                }
            }
            _ => return original,
        };
        let end = self.statement_end(span).unwrap_or(span.end);
        let cut = end.saturating_sub(span.start).min(original.len());
        if !original.is_char_boundary(cut) {
            return original;
        }
        format!("{}{shown}{}", &original[..cut], &original[cut..])
    }

    /// Where the statement of the display line at `span` ends (before its clauses).
    fn statement_end(&self, span: Span) -> Option<usize> {
        use mpp_syntax::document::BlockKind;
        use mpp_syntax::math::Statement;
        let mut found = None;
        self.document.walk(&mut |b| {
            if let BlockKind::DisplayMath(m) = &b.kind {
                for line in &m.ast.lines {
                    if line.span == span {
                        if let Statement::Relation(n) = &line.statement {
                            found = Some(n.span.end);
                        }
                    }
                }
            }
        });
        found
    }

    fn snippet(&self, span: Span) -> String {
        let text = self.source.get(span.start..span.end).unwrap_or("");
        text.split_whitespace().collect::<Vec<_>>().join(" ")
    }

    /// A plain-text report: each query line with its answer, then the problems.
    pub fn report(&self) -> String {
        let mut out = String::new();
        let answered = self.blocks.iter().flat_map(|b| {
            b.values.iter().filter(|v| {
                matches!(v.kind, LineKind::Query { .. } | LineKind::Check)
                    || (v.kind == LineKind::Helper && v.value.is_some())
            })
        });
        for line in answered {
            let answer = line
                .answer()
                .unwrap_or_else(|| "(no answer)".into())
                .replace('\n', "\n        ");
            let _ = writeln!(
                out,
                "{:>4}: {}  {answer}",
                self.line_of(line.span.start),
                self.snippet(line.span)
            );
        }
        for d in self.all_diagnostics() {
            let level = match d.severity {
                Severity::Error => "error",
                Severity::Warning => "warning",
                Severity::Info => "info",
            };
            let _ = writeln!(
                out,
                "{:>4}: {level}: {}",
                self.line_of(d.span.start),
                d.message
            );
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn line_numbers() {
        let idx = LineIndex::new("a\nbc\n\nd");
        assert_eq!(idx.line_of(0), 1);
        assert_eq!(idx.line_of(2), 2);
        assert_eq!(idx.line_of(5), 3);
        assert_eq!(idx.line_of(6), 4);
        assert_eq!(idx.line_of(100), 4);
    }

    #[test]
    fn spans_shift() {
        assert_eq!(shift_span(Span::new(5, 9), -2), Span::new(3, 7));
        assert_eq!(shift_span(Span::new(1, 2), -5), Span::new(0, 0));
    }
}
