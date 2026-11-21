//! Running one block: its statements, in order, against the environment.

mod assume;
mod auto;
mod data;
mod fit;
mod ode;
mod random;
mod seed;
mod sentences;
mod units;

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use mpp_core::{Expr, ExprNode, LogicOp, MathError, Step, Steps};
use mpp_syntax::document::{
    Block, BlockId, BlockKind, Figure, FigureItemKind, Inline, InlineKind, List, Using,
};
use mpp_syntax::math::{
    visit, Clause, DefHead, DefKind, Line, MathAst, Node, NodeKind, QueryMode, Range,
    RelOp as SynRel, Statement,
};
use mpp_syntax::{Diagnostic, Severity, Span};

use crate::check::check;
use crate::error::{EvalError, Located};
use crate::holders::{contains_holder, Collected, Ctx, Holders, Shown};
use crate::import::{Exports, ImportContext, ImportErrorKind, ImportRequest, Importer};
use crate::lower::{LowerScope, Lowerer};
use crate::names::{node_key, subscripted};
use crate::numeric::{approximate, is_exact_rational};
use crate::resolve::{Lookup, Resolver};
use crate::result::{
    BlockResult, DefinitionKind, ImportInfo, LineIndex, LineKind, LineResult, RangeInfo, RangeKind,
};
use crate::value::{Binding, Env, Origin, Sequence, Value, VarKind};

/// What every block of a run shares.
pub(crate) struct Inputs<'a> {
    /// Sequences with a recurrence anywhere in the document.
    pub sequences: &'a BTreeSet<String>,
    /// Names called with an argument where they are not differentiated (see
    /// `Run::define_ode_solution`).
    pub called: &'a BTreeSet<String>,
    /// Names differentiated in an equation somewhere in the document.
    pub ode_unknowns: &'a BTreeSet<String>,
    /// The document's text (spans index into it).
    pub source: &'a str,
    pub holders: &'a Arc<Holders>,
    pub importer: &'a dyn Importer,
    pub base_dir: Option<&'a Path>,
    pub root_dir: Option<&'a Path>,
    pub chain: &'a [PathBuf],
    pub doc_name: Option<&'a str>,
    pub lines: &'a LineIndex,
    /// The session's "Show steps" option: record the working of every `= ?` query.
    pub show_steps: bool,
    /// Auto-evaluation settings and cache.
    pub auto: &'a crate::auto::AutoEval,
}

/// Bindings a block observed before it ran, by name.
pub(crate) type Reads = Vec<(String, Option<Arc<Binding>>)>;

/// What running a block produced.
#[derive(Debug, Clone)]
pub(crate) struct Outcome {
    pub result: BlockResult,
    /// Every binding the block looked up from earlier blocks (or found missing).
    pub reads: Reads,
    /// The bindings the block made, in order.
    pub effects: Vec<(String, Arc<Binding>)>,
    /// A message names a line number elsewhere, so the block cannot be reused
    /// when other text moves.
    pub positional: bool,
    /// The block depends on something outside the document (an import).
    pub volatile: bool,
}

/// Runs `block`, updating `env`.
pub(crate) fn run_block(block: &Block, env: &mut Env, inputs: &Inputs<'_>) -> Outcome {
    let mut run = Run {
        inputs,
        env,
        block: block.id,
        reads: RefCell::new(BTreeMap::new()),
        uses: RefCell::new(BTreeSet::new()),
        result: BlockResult::new(block.id, block.span),
        effects: Vec::new(),
        positional: false,
        volatile: false,
        local_assumptions: Vec::new(),
        unknown_fns: Vec::new(),
        auto_deadline: None,
    };
    run.block(block);
    run.finish()
}

/// Where a maths span sits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Place {
    Display { noeval: bool },
    Let,
    Prose,
}

struct Run<'a, 'e> {
    inputs: &'a Inputs<'a>,
    env: &'e mut Env,
    block: BlockId,
    reads: RefCell<BTreeMap<String, Option<Arc<Binding>>>>,
    uses: RefCell<BTreeSet<String>>,
    result: BlockResult,
    effects: Vec<(String, Arc<Binding>)>,
    positional: bool,
    volatile: bool,
    /// Conditions in brackets after a query in the display block being run.
    local_assumptions: Vec<Expr>,
    /// Unknown functions of the differential equation on the line being run.
    unknown_fns: Vec<String>,
    /// When this block's auto results stop waiting (set by the first one).
    auto_deadline: Option<std::time::Instant>,
}

/// A read-only view of the environment that records what it is asked.
struct View<'v> {
    env: &'v Env,
    block: BlockId,
    reads: &'v RefCell<BTreeMap<String, Option<Arc<Binding>>>>,
    uses: &'v RefCell<BTreeSet<String>>,
    sequences: &'v BTreeSet<String>,
    ode_unknowns: &'v BTreeSet<String>,
}

impl View<'_> {
    fn get(&self, name: &str, as_use: bool) -> Option<Arc<Binding>> {
        let found = self.env.get(name).cloned();
        let own = found
            .as_ref()
            .is_some_and(|b| b.origin().block == self.block);
        if !own {
            self.reads
                .borrow_mut()
                .entry(name.to_string())
                .or_insert_with(|| found.clone());
            if as_use {
                self.uses.borrow_mut().insert(name.to_string());
            }
        }
        found
    }
}

impl LowerScope for View<'_> {
    fn binding(&self, name: &str) -> Option<Arc<Binding>> {
        self.get(name, true)
    }
    fn is_sequence(&self, base: &str) -> bool {
        self.sequences.contains(base)
    }
    fn is_ode_unknown(&self, name: &str) -> bool {
        self.ode_unknowns.contains(name)
    }
}

impl Lookup for View<'_> {
    fn lookup(&self, name: &str) -> Option<Arc<Binding>> {
        self.get(name, true)
    }
}

/// What a resolution produced.
struct Resolved {
    value: Result<Expr, EvalError>,
    steps: Option<Steps>,
    /// Remarks from the evaluators, such as a numerical fallback.
    notes: Vec<String>,
    /// Preferred presentations of values.
    shown: Vec<Shown>,
    touched: BTreeSet<String>,
}

type Locals = BTreeMap<String, Expr>;

/// Substitutes where-clause values into a stored expression.
fn with_locals(e: Expr, locals: &Locals, skip: &[String]) -> Expr {
    let pairs: Vec<(&str, Expr)> = locals
        .iter()
        .filter(|(n, _)| !skip.contains(n))
        .map(|(n, v)| (n.as_str(), v.clone()))
        .collect();
    if pairs.is_empty() {
        e
    } else {
        e.subs_all(&pairs)
    }
}

/// The number as typed when a definition's body is a plain decimal, alone or with a
/// unit (`9.81`, `9.81\,\mathrm{m/s^2}`).
fn written_decimal(body: &Node) -> Option<String> {
    let number = match &body.ungrouped().kind {
        NodeKind::Number(t) => t,
        NodeKind::Quantity { value, .. } => match &value.ungrouped().kind {
            NodeKind::Number(t) => t,
            _ => return None,
        },
        _ => return None,
    };
    (number.contains('.') && number.chars().all(|c| c.is_ascii_digit() || c == '.'))
        .then(|| number.clone())
}

/// Shows an exact value as the decimal it was typed as: `9.81 m/s^2`, not
/// `981/100 m/s^2` (the value itself stays exact).
fn show_decimal(lr: &mut LineResult, decimal: &str) {
    if let Some(t) = &lr.text {
        lr.text = Some(match t.split_once(' ') {
            Some((_, unit)) => format!("{decimal} {unit}"),
            None => decimal.to_string(),
        });
    }
    if let Some(l) = &lr.latex {
        lr.latex = Some(match l.split_once("\\,") {
            Some((_, unit)) => format!("{decimal}\\,{unit}"),
            None => decimal.to_string(),
        });
    }
}

/// The one root when `input` is a `\operatorname{solve}` of a single unknown and its
/// value is a set with exactly one element.
fn single_root(input: &Expr, value: &Expr) -> Option<Expr> {
    let ExprNode::Apply(name, args) = input.node() else {
        return None;
    };
    let [which, sub, ..] = args.as_slice() else {
        return None;
    };
    if &**name != "operatorname" || which.as_sym() != Some("solve") || sub.as_sym().is_none() {
        return None;
    }
    match value.node() {
        ExprNode::Set(items) if items.len() == 1 => items.first().cloned(),
        _ => None,
    }
}

/// The span of the first use of the name `key` inside `node`.
fn find_name(node: &Node, key: &str) -> Option<Span> {
    if node_key(node).as_deref() == Some(key) {
        return Some(node.span);
    }
    let mut found = None;
    visit::children(node, &mut |c| {
        if found.is_none() {
            found = find_name(c, key);
        }
    });
    found
}

impl Run<'_, '_> {
    fn view(&self) -> View<'_> {
        View {
            env: self.env,
            block: self.block,
            reads: &self.reads,
            uses: &self.uses,
            sequences: self.inputs.sequences,
            ode_unknowns: self.inputs.ode_unknowns,
        }
    }

    fn origin(&self, span: Span) -> Origin {
        Origin::local(self.block, span)
    }

    fn finish(self) -> Outcome {
        let mut result = self.result;
        result.uses = self.uses.into_inner().into_iter().collect();
        result
            .diagnostics
            .sort_by_key(|d| (d.span.start, d.span.end));
        Outcome {
            result,
            reads: self.reads.into_inner().into_iter().collect(),
            effects: self.effects,
            positional: self.positional,
            volatile: self.volatile,
        }
    }

    // ---- diagnostics -------------------------------------------------------

    fn located(&mut self, l: Located) {
        if let Some(d) = l.diagnostic() {
            self.result.diagnostics.push(d);
        }
    }

    fn error(&mut self, span: Span, error: EvalError) {
        self.located(Located::new(span, error));
    }

    fn warn(&mut self, span: Span, message: impl Into<String>) {
        self.result
            .diagnostics
            .push(Diagnostic::warning(span, message));
    }

    fn push(&mut self, mut line: LineResult) {
        line.present_text();
        self.result.values.push(line);
    }

    fn define(&mut self, name: &str, binding: Binding) {
        let b = Arc::new(binding);
        self.env.insert(name.to_string(), b.clone());
        self.effects.push((name.to_string(), b));
        if !self.result.defines.iter().any(|d| d == name) {
            self.result.defines.push(name.to_string());
        }
    }

    /// Keeps the denominators a function was written with (hidden, under
    /// [`crate::helpers::denominators_key`]), so the helpers see where the function as
    /// written is undefined even when its body simplified them away. Written for every
    /// function definition, so a redefinition replaces them.
    fn remember_denominators(&mut self, function: &str, denominators: Vec<Expr>, span: Span) {
        let key = crate::helpers::denominators_key(function);
        let b = Arc::new(Binding::Variable {
            kind: VarKind::Variable,
            value: Expr::new(ExprNode::Tuple(denominators)),
            origin: self.origin(span),
        });
        self.env.insert(key.clone(), b.clone());
        self.effects.push((key, b));
    }

    // ---- lowering and resolving --------------------------------------------

    fn lower(&self, node: &Node, locals: &[String]) -> Result<Expr, Located> {
        let view = self.view();
        Lowerer::new(&view)
            .with_locals(locals.iter().cloned())
            .with_unknown_functions(self.unknown_fns.iter().cloned())
            .lower(node)
            .map(|e| self.seeded(e, node.span))
    }

    fn lower_in_function(
        &self,
        node: &Node,
        name: &str,
        params: &[String],
        locals: &[String],
    ) -> Result<Expr, Located> {
        self.lower_function_body(node, name, params, locals)
            .map(|(e, _)| e)
    }

    /// Lowers a function body, with the denominators it was written with.
    fn lower_function_body(
        &self,
        node: &Node,
        name: &str,
        params: &[String],
        locals: &[String],
    ) -> Result<(Expr, Vec<Expr>), Located> {
        let view = self.view();
        let mut lowerer = Lowerer::new(&view)
            .with_locals(locals.iter().cloned())
            .defining(name, params)
            .with_unknown_functions(self.unknown_fns.iter().cloned());
        let e = lowerer.lower(node)?;
        let e = self.seeded(e, node.span);
        Ok((e, lowerer.take_denominators()))
    }

    /// Lowers the function a helper analyses (see [`Lowerer::lower_helper_target`]).
    fn lower_helper_target(&self, node: &Node) -> Result<Expr, Located> {
        let view = self.view();
        Lowerer::new(&view)
            .with_unknown_functions(self.unknown_fns.iter().cloned())
            .lower_helper_target(node)
    }

    fn resolve_full(
        &self,
        e: &Expr,
        locals: &Locals,
        mask: &[String],
        steps: bool,
        numeric: bool,
    ) -> Resolved {
        let view = self.view();
        let mut ctx = Ctx::new(self.inputs.holders)
            .numeric(numeric)
            .with_assumptions(std::sync::Arc::new(self.assumptions()));
        if steps {
            ctx = ctx.with_steps();
        }
        let (value, touched) = {
            let mut r = Resolver::new(&view, &ctx, locals.clone());
            r.mask(mask.iter().cloned());
            let value = r.run(e);
            (value, r.touched().clone())
        };
        let Collected {
            steps,
            notes,
            shown,
        } = ctx.finish();
        Resolved {
            value,
            steps,
            notes,
            shown,
            touched,
        }
    }

    /// `e` with the known values put in but no holder evaluated (for the steps).
    fn substituted(&self, e: &Expr, locals: &Locals) -> Option<Expr> {
        let view = self.view();
        let none = Holders::empty();
        let ctx = Ctx::new(&none);
        let mut r = Resolver::new(&view, &ctx, locals.clone());
        r.run(e).ok()
    }

    fn resolve(&self, e: &Expr, locals: &Locals, span: Span) -> Result<Expr, Located> {
        self.resolve_full(e, locals, &[], false, false)
            .value
            .map_err(|err| Located::new(span, err))
    }

    // ---- blocks ------------------------------------------------------------

    fn block(&mut self, block: &Block) {
        match &block.kind {
            BlockKind::DisplayMath(m) => self.display_block(&m.ast, m.noeval),
            BlockKind::Let(l) => {
                for m in l.math() {
                    self.math(&m.ast, Place::Let);
                }
            }
            BlockKind::Paragraph(content) if fit::fit_sentence(content).is_some() => {
                if let Some(ast) = fit::fit_sentence(content) {
                    self.fit(ast);
                }
            }
            BlockKind::Heading { content, .. }
            | BlockKind::Paragraph(content)
            | BlockKind::FootnoteDef { content, .. } => self.prose(content),
            BlockKind::List(list) => self.list(list),
            BlockKind::Table(t) => {
                if !self.data_table(t) {
                    for cell in t.header.iter().chain(t.rows.iter().flatten()) {
                        self.prose(cell);
                    }
                }
            }
            BlockKind::Quote(paragraphs) => {
                for p in paragraphs {
                    self.prose(p);
                }
            }
            BlockKind::Figure(f) => self.figure(f),
            BlockKind::EmbedImport { path, path_span } => self.import(ImportRequest {
                path: path.clone(),
                span: *path_span,
                names: None,
            }),
            BlockKind::Using(u) => self.using(u),
            BlockKind::Helper(h) => self.helper_sentence(
                crate::helpers::sentence_op(h.helper),
                &h.target,
                h.interval.as_ref(),
                h.at.as_ref(),
                block.span,
            ),
            BlockKind::Analyse(a) => {
                self.helper_sentence("analyse", &a.target, a.interval.as_ref(), None, block.span)
            }
            BlockKind::Solve(s) => self.solve_sentence(s),
            BlockKind::Assume(a) => self.assume_sentence(a),
            BlockKind::Data(d) => self.data_import(d),
            BlockKind::Directive(d) => self.seed_directive(d, block.span),
            // Comments are shown but never run: nothing in one is evaluated or defined.
            BlockKind::Comment(_)
            | BlockKind::Title(_)
            | BlockKind::References(_)
            | BlockKind::Theorem(_)
            | BlockKind::Rule
            | BlockKind::Verbatim(_) => {}
        }
    }

    fn list(&mut self, list: &List) {
        for item in &list.items {
            self.prose(&item.content);
            for child in &item.children {
                self.list(child);
            }
        }
    }

    fn prose(&mut self, inlines: &[Inline]) {
        for inline in inlines {
            match &inline.kind {
                InlineKind::Math(m) => self.math(&m.ast, Place::Prose),
                InlineKind::Emphasis(inner) | InlineKind::Strong(inner) => self.prose(inner),
                InlineKind::Link { text, .. } => self.prose(text),
                _ => {}
            }
        }
    }

    /// "Find the critical points of $f$ on $[0, 4]$." and "Analyse $f$.": the same
    /// holder as the operator form, answered on the sentence's line.
    fn helper_sentence(
        &mut self,
        op: &str,
        target: &Node,
        interval: Option<&Node>,
        at: Option<&Clause>,
        span: Span,
    ) {
        let mut lr = LineResult::new(span, LineKind::Helper);
        let lowered = (|| -> Result<Expr, Located> {
            let t = self.lower_helper_target(target)?;
            let i = interval.map(|n| self.lower(n, &[])).transpose()?;
            let a = match at {
                Some(Clause::Where { name, value, .. }) => Some(Expr::equation(
                    self.lower(name, &[])?,
                    self.lower(value, &[])?,
                )),
                Some(Clause::Condition(n)) => Some(self.lower(n, &[])?),
                _ => None,
            };
            Ok(crate::helpers::sentence_holder(op, t, i, a))
        })();
        let input = match lowered {
            Ok(e) => e,
            Err(l) => {
                self.located(l);
                return self.push(lr);
            }
        };
        lr.input = Some(input.clone());
        let resolved =
            self.resolve_full(&input, &Locals::new(), &[], self.inputs.show_steps, false);
        for note in &resolved.notes {
            self.warn(span, note.clone());
        }
        match resolved.value {
            Ok(v) => {
                let shown = crate::domains_wire::present_set(&v);
                lr.set_value(Value::from_expr(v));
                if let Some((text, latex)) = shown {
                    lr.text = Some(text);
                    lr.latex = Some(latex);
                }
            }
            Err(err) => self.error(target.span, err),
        }
        if let Some(log) = resolved.steps.filter(|l| !l.is_empty()) {
            lr.steps = Some(log.clone());
            self.result.steps.push((self.result.values.len(), log));
        }
        self.push(lr);
    }

    // ---- statement lines ---------------------------------------------------

    fn math(&mut self, ast: &MathAst, place: Place) {
        for line in &ast.lines {
            self.line(line, place);
        }
    }

    fn line(&mut self, line: &Line, place: Place) {
        self.unknown_fns = self.unknown_functions_of(line);
        self.statement_line(line, place);
        self.unknown_fns.clear();
    }

    fn statement_line(&mut self, line: &Line, place: Place) {
        let mut where_names: Vec<String> = Vec::new();
        let mut locals = Locals::new();
        let mut conditions: Vec<&Node> = Vec::new();
        for clause in &line.clauses {
            match clause {
                Clause::Where { name, value, .. } => {
                    let Some(key) = node_key(name) else {
                        self.error(
                            name.span,
                            EvalError::Invalid("expected a name before `=`".into()),
                        );
                        continue;
                    };
                    let v = self
                        .lower(value, &where_names)
                        .and_then(|e| self.resolve(&e, &locals, value.span));
                    match v {
                        Ok(v) => {
                            locals.insert(key.clone(), v);
                            where_names.push(key);
                        }
                        Err(l) => self.located(l),
                    }
                }
                Clause::Range(r) => self.range(r, RangeKind::Slider, &where_names),
                Clause::Condition(n) => conditions.push(n),
                // Read for the whole block before it runs (see `display_block`).
                Clause::Assumption(_) => {}
            }
        }
        let noeval = line.noeval || place == Place::Display { noeval: true };
        let scope = Scope {
            names: &where_names,
            locals: &locals,
            conditions: &conditions,
        };
        match &line.statement {
            Statement::Definition {
                head,
                kind,
                body,
                query,
            } => {
                self.definition(line, head, *kind, body, &scope);
                if let Some(mode) = query {
                    self.definition_query(line, head, body, *mode, &scope, noeval);
                }
            }
            Statement::Query { expr, mode, steps } => {
                self.query(line, expr, *mode, *steps, &scope, noeval)
            }
            Statement::Check { lhs, rhs } => self.check_line(line, lhs, rhs, &scope),
            Statement::Relation(node) => {
                if place != Place::Prose && self.random_declaration(line, place) {
                    return;
                }
                if place == Place::Let && self.let_definition(line, node, &scope) {
                    return;
                }
                let display = matches!(place, Place::Display { .. });
                self.relation(line, node, &scope, display, noeval);
            }
            Statement::Range(r) => {
                self.range(r, RangeKind::Slider, &where_names);
                self.push(LineResult::new(line.span, LineKind::Range));
            }
            Statement::NameList(_) | Statement::Empty => {}
        }
    }

    fn range(&mut self, r: &Range, kind: RangeKind, locals: &[String]) {
        let names: Vec<String> = r.names.iter().filter_map(node_key).collect();
        let set = self
            .lower(&r.set, locals)
            .and_then(|e| self.resolve(&e, &Locals::new(), r.set.span));
        match set {
            Ok(set) => self.result.ranges.push(RangeInfo {
                names,
                set,
                kind,
                span: r.span,
            }),
            Err(l) => self.located(l),
        }
    }

    /// Reports and returns true when `name` is a constant that cannot be redefined.
    fn blocked(&mut self, name: &str, span: Span) -> bool {
        let prior = self.view().get(name, false);
        match prior {
            Some(b) if b.is_constant() => {
                let line = self.inputs.lines.line_of(b.origin().span.start);
                self.positional = true;
                self.error(
                    span,
                    EvalError::ConstantRedefined {
                        name: name.to_string(),
                        line: b.origin().line.unwrap_or(line),
                        file: b.origin().file.as_ref().map(|f| f.to_string()),
                    },
                );
                true
            }
            _ => false,
        }
    }

    fn definition(&mut self, line: &Line, head: &DefHead, kind: DefKind, body: &Node, s: &Scope) {
        match head {
            DefHead::Name(n) => match node_key(n) {
                Some(key) => self.define_variable(line, &key, n.span, kind, body, s),
                None => self.error(
                    n.span,
                    EvalError::Invalid("only a name can be defined here".into()),
                ),
            },
            DefHead::Initial { name, index } => {
                let Some(base) = node_key(name) else {
                    return self.error(
                        name.span,
                        EvalError::Invalid("only a name can be defined here".into()),
                    );
                };
                if self.inputs.sequences.contains(&base) {
                    self.define_initial(line, &base, index, body, s);
                } else {
                    match subscripted(&base, index) {
                        Some(key) => self.define_variable(line, &key, name.span, kind, body, s),
                        None => self.error(
                            index.span,
                            EvalError::Unsupported("this subscript on a name".into()),
                        ),
                    }
                }
            }
            DefHead::Function { name, params } => self.define_function(line, name, params, body, s),
            DefHead::Recurrence { name, index, shift } => {
                self.define_recurrence(line, name, index, shift, body, s)
            }
            DefHead::Invalid(_) => {}
        }
    }

    fn define_variable(
        &mut self,
        line: &Line,
        key: &str,
        name_span: Span,
        kind: DefKind,
        body: &Node,
        s: &Scope,
    ) {
        if self.blocked(key, name_span) {
            return;
        }
        let lowered = match self.lower(body, s.names) {
            Ok(e) => e,
            Err(l) => return self.located(l),
        };
        let value = with_locals(lowered, s.locals, &[]);
        let has_prior = self.view().get(key, false).is_some();
        if value.contains_sym(key) && !has_prior {
            return self.error(name_span, EvalError::Circular(vec![key.to_string()]));
        }
        let resolved = self.resolve_full(&value, &Locals::new(), &[], false, false);
        let shown = match resolved.value.and_then(|v| self.finish_value(v)) {
            Ok(v) => v,
            Err(err) => return self.error(body.span, err),
        };
        // `V_eq := \operatorname{solve}_{V_b}(…)` with exactly one root: the name is that
        // root, a value later lines can compare and compute with (several roots keep
        // the set, which is no single value).
        let (shown, single_root) = match single_root(&value, &shown) {
            Some(root) => (root, true),
            None => (shown, false),
        };
        // A definition that reaches its own old value (`x := x + 1`) is a snapshot.
        let snapshot = kind == DefKind::Const || resolved.touched.contains(key) || single_root;
        let (var_kind, def_kind) = match kind {
            DefKind::Assign => (VarKind::Variable, DefinitionKind::Variable),
            DefKind::Const => (VarKind::Constant, DefinitionKind::Constant),
        };
        let stored = if snapshot {
            shown.clone()
        } else {
            value.clone()
        };
        let decimal = written_decimal(body);
        let mut origin = self.origin(line.span);
        origin.decimal = decimal.as_deref().map(Arc::from);
        self.define(
            key,
            Binding::Variable {
                kind: var_kind,
                value: stored,
                origin,
            },
        );
        let mut lr = LineResult::new(
            line.span,
            LineKind::Definition {
                name: key.to_string(),
                kind: def_kind,
            },
        );
        lr.input = Some(value);
        if self.present_units(&mut lr, &shown, QueryMode::Exact, body.span) {
            if let Some(d) = &decimal {
                show_decimal(&mut lr, d);
            }
            return self.push(lr);
        }
        if shown.is_constant() && !is_exact_rational(&shown) && !contains_holder(&shown) {
            lr.approx = approximate(&shown).ok();
        }
        lr.set_value(Value::from_expr(shown));
        if let Some(d) = &decimal {
            show_decimal(&mut lr, d);
        }
        self.push(lr);
    }

    /// The decimal a bare name was defined as (`g` for `g \equiv 9.81`), when `value`
    /// is still exactly that definition's value.
    fn typed_decimal(&self, expr: &Node, value: &Expr) -> Option<String> {
        let key = node_key(expr.ungrouped())?;
        let binding = self.view().get(&key, false)?;
        let Binding::Variable {
            value: stored,
            origin,
            ..
        } = binding.as_ref()
        else {
            return None;
        };
        let decimal = origin.decimal.as_deref()?;
        (stored.simplify() == value.simplify()).then(|| decimal.to_string())
    }

    fn param_names(&mut self, params: &[Node]) -> Option<Vec<String>> {
        let mut names: Vec<String> = Vec::new();
        for p in params {
            match node_key(p) {
                Some(k) if names.contains(&k) => {
                    self.error(
                        p.span,
                        EvalError::Invalid(format!("the parameter `{k}` appears twice")),
                    );
                    return None;
                }
                Some(k) => names.push(k),
                None => {
                    self.error(
                        p.span,
                        EvalError::Invalid("a parameter must be a name".into()),
                    );
                    return None;
                }
            }
        }
        Some(names)
    }

    fn define_function(
        &mut self,
        line: &Line,
        name: &Node,
        params: &[Node],
        body: &Node,
        s: &Scope,
    ) {
        let Some(key) = node_key(name) else {
            return self.error(
                name.span,
                EvalError::Invalid("a function name must be a name".into()),
            );
        };
        if self.blocked(&key, name.span) {
            return;
        }
        let Some(names) = self.param_names(params) else {
            return;
        };
        let mut locals: Vec<String> = s.names.to_vec();
        locals.extend(names.iter().cloned());
        let (lowered, denominators) = match self.lower_function_body(body, &key, &names, &locals) {
            Ok(e) => e,
            Err(l) => return self.located(l),
        };
        let mut body_expr = with_locals(lowered, s.locals, &names);
        if let Some(fitted) = self.fitted_body(&body_expr, &names, body.span) {
            body_expr = fitted;
        }
        if !s.conditions.is_empty() {
            let mut conds = Vec::new();
            for c in s.conditions {
                match self.lower_in_function(c, &key, &names, &locals) {
                    Ok(e) => conds.push(with_locals(e, s.locals, &names)),
                    Err(l) => return self.located(l),
                }
            }
            let cond = if conds.len() == 1 {
                conds.swap_remove(0)
            } else {
                Expr::new(ExprNode::Logic(LogicOp::And, conds))
            };
            // `f(x) := …, x ≥ 0` followed by `f(x) := …, x < 0` in the same
            // block builds one piecewise function.
            let mut cases = match self.env.get(&key).map(|b| &**b) {
                Some(Binding::Function {
                    params: p,
                    body: b,
                    origin,
                }) if *p == names && origin.block == self.block => match b.node() {
                    ExprNode::Piecewise {
                        cases,
                        otherwise: None,
                    } => cases.clone(),
                    _ => Vec::new(),
                },
                _ => Vec::new(),
            };
            cases.push((body_expr, cond));
            body_expr = Expr::new(ExprNode::Piecewise {
                cases,
                otherwise: None,
            });
        }
        self.define(
            &key,
            Binding::Function {
                params: names.clone(),
                body: body_expr.clone(),
                origin: self.origin(line.span),
            },
        );
        self.remember_denominators(&key, denominators, line.span);
        let mut lr = LineResult::new(
            line.span,
            LineKind::Definition {
                name: key,
                kind: DefinitionKind::Function,
            },
        );
        lr.input = Some(body_expr.clone());
        lr.set_value(Value::Function {
            params: names,
            body: body_expr,
        });
        self.push(lr);
    }

    fn integer(&mut self, node: &Node, what: &str) -> Option<i64> {
        let value = self
            .lower(node, &[])
            .ok()
            .and_then(|e| e.simplify().as_number().and_then(|n| n.as_i64()));
        if value.is_none() {
            self.error(
                node.span,
                EvalError::Invalid(format!("{what} must be a whole number")),
            );
        }
        value
    }

    fn define_recurrence(
        &mut self,
        line: &Line,
        name: &Node,
        index: &Node,
        shift: &Node,
        body: &Node,
        s: &Scope,
    ) {
        let (Some(base), Some(idx)) = (node_key(name), node_key(index)) else {
            return self.error(
                line.span,
                EvalError::Invalid("a recurrence is written like a_{n+1} := …".into()),
            );
        };
        if self.blocked(&base, name.span) {
            return;
        }
        let Some(shift) = self.integer(shift, "the shift in a recurrence") else {
            return;
        };
        let mut locals: Vec<String> = s.names.to_vec();
        locals.push(idx.clone());
        let lowered = match self.lower(body, &locals) {
            Ok(e) => with_locals(e, s.locals, std::slice::from_ref(&idx)),
            Err(l) => return self.located(l),
        };
        let mut seq = self.sequence_of(&base);
        seq.index = idx;
        seq.shift = shift;
        seq.body = Some(lowered.clone());
        self.define(
            &base,
            Binding::Sequence {
                seq,
                origin: self.origin(line.span),
            },
        );
        let mut lr = LineResult::new(
            line.span,
            LineKind::Definition {
                name: base,
                kind: DefinitionKind::Recurrence,
            },
        );
        lr.input = Some(lowered.clone());
        lr.set_value(Value::Expr(lowered));
        self.push(lr);
    }

    fn sequence_of(&self, base: &str) -> Sequence {
        match self.view().get(base, false).as_deref() {
            Some(Binding::Sequence { seq, .. }) => seq.clone(),
            _ => Sequence {
                index: "n".into(),
                shift: 1,
                body: None,
                initials: BTreeMap::new(),
            },
        }
    }

    fn define_initial(&mut self, line: &Line, base: &str, index: &Node, body: &Node, s: &Scope) {
        if self.blocked(base, index.span) {
            return;
        }
        let Some(k) = self.integer(index, "the index of an initial term") else {
            return;
        };
        let value = match self.lower(body, s.names) {
            Ok(e) => with_locals(e, s.locals, &[]),
            Err(l) => return self.located(l),
        };
        let shown = self.resolve(&value, &Locals::new(), body.span);
        let mut seq = self.sequence_of(base);
        seq.initials.insert(k, value.clone());
        let origin = match self.view().get(base, false).as_deref() {
            Some(Binding::Sequence { origin, .. }) if origin.block == self.block => origin.clone(),
            _ => self.origin(line.span),
        };
        self.define(base, Binding::Sequence { seq, origin });
        let mut lr = LineResult::new(
            line.span,
            LineKind::Definition {
                name: format!("{base}_{k}"),
                kind: DefinitionKind::Initial,
            },
        );
        lr.input = Some(value);
        match shown {
            Ok(v) => lr.set_value(Value::from_expr(v)),
            Err(l) => self.located(l),
        }
        self.push(lr);
    }

    /// `T_0 := 2\pi\sqrt{L/g} \approx ?`: after the definition, the value of the name
    /// just defined. The definition keeps the part before `≈ ?`; the answer belongs to
    /// the whole line.
    fn definition_query(
        &mut self,
        line: &Line,
        head: &DefHead,
        body: &Node,
        mode: QueryMode,
        s: &Scope,
        noeval: bool,
    ) {
        let name = match head {
            DefHead::Name(n) => n.clone(),
            DefHead::Initial { name, index } => {
                let key = node_key(name)
                    .filter(|b| !self.inputs.sequences.contains(b))
                    .and_then(|b| subscripted(&b, index));
                match key {
                    Some(k) => Node::new(NodeKind::Name(mpp_syntax::math::Name::simple(k)), name.span.join(index.span)),
                    None => return,
                }
            }
            _ => {
                return self.error(
                    line.span,
                    EvalError::Unsupported(
                        "asking for the value of a function as it is defined (ask for f(2) on a line of its own)".into(),
                    ),
                )
            }
        };
        if let Some(last) = self.result.values.last_mut() {
            if last.span == line.span {
                last.span = Span::new(line.span.start, body.span.end.max(line.span.start));
            }
        }
        self.query(line, &name, mode, false, s, noeval);
    }

    /// `Let $a = 3$.` and `Let $f(x) = x^2$.` define.
    fn let_definition(&mut self, line: &Line, node: &Node, s: &Scope) -> bool {
        let NodeKind::Relation { first, rest } = &node.kind else {
            return false;
        };
        let [(SynRel::Eq, rhs)] = rest.as_slice() else {
            return false;
        };
        if let Some(key) = node_key(first) {
            self.define_variable(line, &key, first.span, DefKind::Assign, rhs, s);
            return true;
        }
        if let NodeKind::Call { callee, args, .. } = &first.kind {
            if callee.is_name() && args.iter().all(Node::is_name) {
                self.define_function(line, callee, args, rhs, s);
                return true;
            }
        }
        false
    }

    fn query(
        &mut self,
        line: &Line,
        expr: &Node,
        mode: QueryMode,
        steps: bool,
        s: &Scope,
        noeval: bool,
    ) {
        let mut lr = LineResult::new(line.span, LineKind::Query { mode, steps });
        lr.noeval = noeval;
        let input = match self.lower(expr, s.names) {
            Ok(e) => e,
            Err(l) => {
                if matches!(l.error, EvalError::EmptyBox(_)) {
                    lr.hint = Some(l.error.to_string());
                }
                self.located(l);
                return self.push(lr);
            }
        };
        lr.input = Some(input.clone());
        let input = self.catalan_numbers(input, expr.span);
        // `y'' + y = 0 = ?`: a differential equation goes to the solver with the
        // line's conditions.
        let input = self.ode_input(&input, s).unwrap_or(input);
        let numeric = mode == QueryMode::Approx;
        let logging = steps || self.inputs.show_steps;
        let mut resolved = self.resolve_full(&input, s.locals, &[], logging, numeric);
        for note in &resolved.notes {
            self.warn(expr.span, note.clone());
        }
        let raw = resolved.value.clone();
        if raw
            .as_ref()
            .is_ok_and(crate::units_wire::has_log_of_concentration)
        {
            self.warn(expr.span, crate::units_wire::LOG_OF_CONCENTRATION);
        }
        let value = match raw.and_then(|v| self.finish_logged(v, resolved.steps.as_mut())) {
            Ok(v) => v,
            Err(err) => {
                let span = match err {
                    EvalError::Math(MathError::Dimension(_)) => {
                        self.dimension_span(expr, s.names, s.locals)
                    }
                    _ => expr.span,
                };
                self.error(span, err);
                return self.push(lr);
            }
        };
        if let Some(mut log) = resolved.steps {
            // The first step puts in the known values; the evaluators' own working
            // (derivative rules, integration techniques …) follows.
            let first = if log.is_empty() {
                Some(("substitute the known values and simplify", value.clone()))
            } else if resolved.touched.is_empty() {
                None
            } else {
                self.substituted(&input, s.locals)
                    .map(|e| ("substitute the known values", e))
            };
            if let Some((rule, after)) = first.filter(|(_, after)| *after != input) {
                log.0.insert(
                    0,
                    Step {
                        rule: rule.into(),
                        before: input.clone(),
                        after,
                        note: None,
                    },
                );
            }
            lr.steps = Some(log.clone());
            self.result.steps.push((self.result.values.len(), log));
        }
        let typed = (mode == QueryMode::Exact)
            .then(|| self.typed_decimal(expr, &value))
            .flatten();
        if self.present_units(&mut lr, &value, mode, expr.span) {
            if let Some(d) = &typed {
                show_decimal(&mut lr, d);
            }
            return self.push(lr);
        }
        if let Some(d) = &typed {
            lr.set_value(Value::from_expr(value));
            show_decimal(&mut lr, d);
            return self.push(lr);
        }
        if let Some((text, latex)) = crate::domains_wire::present_set(&value) {
            lr.set_value(Value::from_expr(value));
            lr.text = Some(text);
            lr.latex = Some(latex);
            return self.push(lr);
        }
        match mode {
            QueryMode::Exact => {
                if value.is_constant() && !is_exact_rational(&value) && !contains_holder(&value) {
                    lr.approx = approximate(&value).ok();
                }
            }
            QueryMode::Approx => match approximate(&value) {
                Ok(a) => lr.approx = Some(a),
                // No number, so no result at all: `sin θ ≈ ?` with θ unknown shows
                // only the error, never `sin(θ)` as if it were the answer.
                Err(EvalError::Unbound(names)) => {
                    self.unbound(expr, names);
                    return self.push(lr);
                }
                Err(EvalError::Math(MathError::NotANumber(what))) if contains_holder(&value) => {
                    self.warn(
                        expr.span,
                        format!("{what} cannot be computed yet, so it is kept as written"),
                    )
                }
                Err(err) => {
                    self.error(expr.span, err);
                    return self.push(lr);
                }
            },
        }
        let approximate_only = value.as_number().is_some_and(|n| !n.is_exact());
        if approximate_only && lr.approx.is_none() {
            lr.approx = approximate(&value).ok();
        }
        let shown = resolved.shown.iter().find(|s| s.value == value).cloned();
        let differential = crate::differential::present(&input, &value);
        lr.set_value(Value::from_expr(value));
        if let Some((text, latex)) = differential {
            lr.text = Some(text);
            lr.latex = Some(latex);
        }
        if approximate_only && lr.approx.is_some() {
            // A numerical fallback (no closed form) is shown with ≈ only.
            lr.text = None;
        }
        if let Some(s) = shown {
            lr.plain = Some(s.text.clone());
            lr.text = Some(s.unicode.unwrap_or(s.text));
            lr.latex = Some(s.latex);
        }
        self.push(lr);
    }

    /// `C_5 = ?`, `C_{10}/C_9 = ?`: when every free name of a query is a `C` with a whole
    /// number subscript and the document gives none of them (nor `C`) a value, they are
    /// the Catalan numbers. Anywhere else `C_1`, `C_2` stay names (the constants of a
    /// general solution), so nothing changes meaning silently.
    fn catalan_numbers(&mut self, input: Expr, span: Span) -> Expr {
        let free = input.free_symbols();
        if free.is_empty() || self.view().get("C", false).is_some() {
            return input;
        }
        let mut pairs = Vec::new();
        for name in free.iter() {
            let Some(n) = name.strip_prefix("C_").and_then(|d| d.parse::<i64>().ok()) else {
                return input;
            };
            if self.view().get(name, false).is_some() || !(0..=10_000).contains(&n) {
                return input;
            }
            pairs.push((name.to_string(), n));
        }
        let mut out = input;
        for (name, n) in &pairs {
            out = out.subs(name, &Expr::apply("catalan", vec![Expr::int(*n)]));
        }
        self.warn(span, "C_n is read as the n-th Catalan number".to_string());
        out
    }

    /// Reports names without values in a `≈ ?` query.
    fn unbound(&mut self, expr: &Node, names: Vec<String>) {
        let imported: Vec<(String, String)> = names
            .iter()
            .filter_map(|n| match self.env.get(n).map(|b| &**b) {
                Some(Binding::Imported { from, .. }) => Some((n.clone(), from.clone())),
                _ => None,
            })
            .collect();
        let span = names
            .first()
            .and_then(|n| find_name(expr, n))
            .unwrap_or(expr.span);
        if !imported.is_empty() && imported.len() == names.len() {
            let list: Vec<String> = imported.iter().map(|(n, _)| format!("`{n}`")).collect();
            let mut files: Vec<&str> = imported.iter().map(|(_, f)| f.as_str()).collect();
            files.dedup();
            let verb = if list.len() == 1 { "comes" } else { "come" };
            self.warn(
                span,
                format!(
                    "{} {verb} from {}, which is not loaded yet, so there is no number to give",
                    list.join(", "),
                    files.join(" and ")
                ),
            );
        } else {
            self.error(span, EvalError::Unbound(names));
        }
    }

    fn check_line(&mut self, line: &Line, lhs: &Node, rhs: &Node, s: &Scope) {
        let mut lr = LineResult::new(line.span, LineKind::Check);
        let sides = self
            .lower(lhs, s.names)
            .and_then(|l| self.lower(rhs, s.names).map(|r| (l, r)));
        let (l_in, r_in) = match sides {
            Ok(v) => v,
            Err(l) => {
                if matches!(l.error, EvalError::EmptyBox(_)) {
                    lr.hint = Some(l.error.to_string());
                }
                self.located(l);
                return self.push(lr);
            }
        };
        lr.input = Some(Expr::equation(l_in.clone(), r_in.clone()));
        let side = |run: &Self, e: &Expr| {
            run.resolve_full(e, s.locals, &[], false, false)
                .value
                .and_then(|v| run.finish_logged(v, None))
        };
        let l = side(self, &l_in);
        let r = side(self, &r_in);
        match (l, r) {
            (Ok(l), Ok(r)) => {
                let mut verdict = check(&l, &r);
                // `… \text{ for all } t \ge 0` (or `…, t \ge 0`): unless proved outright,
                // the statement is tried over the values the conditions allow.
                if verdict != crate::check::Verdict::Proved && !s.conditions.is_empty() {
                    let domain: Option<Vec<Expr>> = s
                        .conditions
                        .iter()
                        .map(|c| {
                            let e = self.lower(c, s.names).ok()?;
                            side(self, &e).ok()
                        })
                        .collect();
                    if let Some(v) =
                        domain.and_then(|d| crate::check_domain::check_on_domain(&l, &r, &d))
                    {
                        verdict = v;
                    }
                }
                // `\int f \, dx \overset{?}{=} F`: antiderivatives agree up to a constant.
                if verdict.passed() != Some(true) {
                    let mut vars = crate::check::indefinite_variables(&l_in);
                    vars.extend(crate::check::indefinite_variables(&r_in));
                    let written: BTreeSet<String> = l_in
                        .free_symbols()
                        .union(&r_in.free_symbols())
                        .map(|n| n.to_string())
                        .collect();
                    let constants: Vec<String> = l
                        .free_symbols()
                        .union(&r.free_symbols())
                        .map(|n| n.to_string())
                        .filter(|n| !written.contains(n) && n.starts_with('C'))
                        .collect();
                    if let Some(v) = crate::check::check_up_to_constant(&l, &r, &vars, &constants) {
                        self.warn(
                            line.span,
                            "the two sides agree up to a constant of integration",
                        );
                        verdict = v;
                    }
                }
                lr.verdict = Some(verdict);
            }
            (Err(err), _) => self.error(lhs.span, err),
            (_, Err(err)) => self.error(rhs.span, err),
        }
        self.push(lr);
    }

    fn relation(&mut self, line: &Line, node: &Node, s: &Scope, display: bool, noeval: bool) {
        let mut lr = LineResult::new(line.span, LineKind::Relation);
        lr.noeval = noeval;
        lr.auto_eval = display && !noeval;
        let mut lowered = None;
        match self.lower(node, s.names) {
            Ok(e) => {
                lr.input = Some(with_locals(e.clone(), s.locals, &[]));
                lowered = Some(e);
            }
            // An empty template box: no answer yet, and a hint instead of an error.
            Err(l) if matches!(l.error, EvalError::EmptyBox(_)) => {
                lr.hint = Some(l.error.to_string());
            }
            // Prose maths is only recorded; only display maths reports problems.
            Err(l) if display => self.located(l),
            Err(_) => {}
        }
        for c in s.conditions {
            if let Ok(e) = self.lower(c, s.names) {
                lr.conditions.push(e);
            }
        }
        if let (Some(e), true) = (&lowered, lr.auto_eval) {
            self.auto_evaluate(&mut lr, node, e, s);
        }
        self.push(lr);
        // After the line's own preview, so it still sees the unknowns as unknowns.
        if let (Some(e), true) = (&lowered, display && !noeval) {
            self.define_ode_solution(line, e, s);
        }
    }

    /// A partial differential equation with its boundary and initial conditions
    /// (`u_t = α u_xx, u(0, t) = u(L, t) = 0, u(x, 0) = f(x)`): when it is solved, the
    /// solution defines u for the lines below that use it.
    fn define_pde_solution(&mut self, line: &Line, e: &Expr, u: String, s: &Scope) {
        if !self.inputs.called.contains(&u) {
            return;
        }
        let resolve = |run: &Self, x: &Expr| {
            run.resolve_full(x, s.locals, &[], false, false)
                .value
                .ok()
                .map(|v| v.simplify())
        };
        let Some(eq) = resolve(self, e) else {
            return;
        };
        let conditions: Vec<Expr> = s
            .conditions
            .iter()
            .filter_map(|c| self.lower(c, s.names).ok())
            .filter_map(|c| resolve(self, &c))
            .collect();
        match crate::pde_wire::solve(&eq, &conditions, &u) {
            Ok(sol) => {
                self.warn(line.span, sol.method);
                self.define(
                    &u,
                    Binding::Function {
                        params: sol.params,
                        body: sol.body,
                        origin: self.origin(line.span),
                    },
                );
            }
            Err(err) => self.warn(line.span, format!("{u} is not defined: {err}")),
        }
    }

    /// `\frac{dN}{dt} = -\lambda N, \quad N(0) = N_0` written as a statement: when the
    /// equation (or system) and its conditions determine the unknown functions
    /// exactly, the solution defines them, so later lines can use `N(t)`, `N(10)` or
    /// solve for a time. A general solution with free constants defines nothing (a
    /// note says which conditions are missing).
    fn define_ode_solution(&mut self, line: &Line, e: &Expr, s: &Scope) {
        if self.unknown_fns.is_empty() {
            return;
        }
        if let [u] = self.unknown_fns.as_slice() {
            if crate::pde_wire::is_pde(e, u) {
                return self.define_pde_solution(line, e, u.clone(), s);
            }
        }
        let Some(call) = self.ode_input(e, s) else {
            return;
        };
        // The solution defines functions: an explicit one, or else a numerical one.
        let call = match call.node() {
            ExprNode::Apply(name, args) if &**name == "dsolve" => {
                let mut args = args.clone();
                args.push(Expr::sym(crate::helpers_b::FUNCTION_MARKER));
                Expr::apply(name, args)
            }
            _ => call,
        };
        let r = self.resolve_full(&call, s.locals, &[], false, false);
        for note in r.notes.iter().filter(|n| n.contains("numerical")) {
            self.warn(line.span, note.clone());
        }
        let Ok(value) = r.value.and_then(|v| self.finish_value(v)) else {
            return;
        };
        let solutions: Vec<Expr> = match value.node() {
            ExprNode::Tuple(items) => items.clone(),
            _ => vec![value.clone()],
        };
        let mut found = Vec::new();
        for sol in &solutions {
            let ExprNode::Relation(mpp_core::RelOp::Eq, lhs, rhs) = sol.node() else {
                return;
            };
            let ExprNode::Apply(name, args) = lhs.node() else {
                return;
            };
            let [var] = args.as_slice() else {
                return;
            };
            let (Some(t), true) = (var.as_sym(), self.unknown_fns.iter().any(|u| u == &**name))
            else {
                return;
            };
            // Only a function the document goes on to use (`N(10\,\mathrm{yr})`) is
            // defined; one met only in differential equations stays their unknown.
            if !self.inputs.called.contains(&**name) {
                return;
            }
            let free_constants: Vec<String> = rhs
                .free_symbols()
                .iter()
                .filter(|c| {
                    let c: &str = c;
                    (c == "C"
                        || c.strip_prefix("C_")
                            .is_some_and(|k| k.parse::<u32>().is_ok()))
                        && !e.contains_sym(c)
                })
                .map(|c| c.to_string())
                .collect();
            if !free_constants.is_empty() {
                self.warn(
                    line.span,
                    format!(
                        "{name} is not determined yet ({} left free): give its initial conditions to define it",
                        free_constants.join(", ")
                    ),
                );
                return;
            }
            found.push((name.to_string(), t.to_string(), rhs.clone()));
        }
        for (name, t, body) in found {
            self.define(
                &name,
                Binding::Function {
                    params: vec![t],
                    body,
                    origin: self.origin(line.span),
                },
            );
        }
    }

    // ---- figures and imports -------------------------------------------------

    fn figure(&mut self, f: &Figure) {
        let plot_names: Vec<String> = f
            .items
            .iter()
            .filter_map(|i| match &i.kind {
                FigureItemKind::Range(r) => Some(r.names.iter().filter_map(node_key)),
                _ => None,
            })
            .flatten()
            .collect();
        for item in &f.items {
            match &item.kind {
                FigureItemKind::Expr(ast) => {
                    for line in &ast.lines {
                        let node = match &line.statement {
                            Statement::Relation(n) | Statement::Query { expr: n, .. } => n,
                            _ => continue,
                        };
                        let mut lr = LineResult::new(line.span, LineKind::Figure);
                        let lowered = self.lower(node, &plot_names);
                        match lowered {
                            Ok(input) => {
                                let r = self.resolve_full(
                                    &input,
                                    &Locals::new(),
                                    &plot_names,
                                    false,
                                    false,
                                );
                                lr.input = Some(input);
                                match r.value {
                                    Ok(v) => lr.set_value(Value::from_expr(v)),
                                    Err(err) => self.error(node.span, err),
                                }
                            }
                            Err(l) => self.located(l),
                        }
                        self.push(lr);
                    }
                }
                FigureItemKind::Range(r) => self.range(r, RangeKind::Plot, &[]),
                FigureItemKind::Keyword { .. } => {}
            }
        }
    }

    fn using(&mut self, u: &Using) {
        let key_of = |n: &Node| {
            node_key(n).or_else(|| match &n.kind {
                NodeKind::Call { callee, .. } => node_key(callee),
                _ => None,
            })
        };
        let mut pairs = Vec::new();
        for (i, n) in u.names.iter().enumerate() {
            let Some(name) = key_of(n) else {
                self.error(
                    n.span,
                    EvalError::Invalid("expected a name to import".into()),
                );
                continue;
            };
            let alias = match &u.aliases {
                Some(aliases) => match aliases.get(i).and_then(key_of) {
                    Some(a) => a,
                    None => {
                        self.error(
                            n.span,
                            EvalError::Invalid(format!("`{name}` has no matching name after `as`")),
                        );
                        continue;
                    }
                },
                None => name.clone(),
            };
            pairs.push((name, alias));
        }
        self.import(ImportRequest {
            path: u.path.clone(),
            span: u.path_span,
            names: Some(pairs),
        });
    }

    fn import(&mut self, req: ImportRequest) {
        self.volatile = true;
        let mut info = ImportInfo {
            path: req.path.clone(),
            span: req.span,
            names: req.names.clone(),
            loaded: false,
            file: None,
            files: Vec::new(),
            diagnostics: Vec::new(),
            uncacheable: false,
        };
        let ctx = ImportContext {
            base: self.inputs.base_dir,
            root_dir: self.inputs.root_dir,
            chain: self.inputs.chain,
            holders: self.inputs.holders,
        };
        match self.inputs.importer.import_in(&ctx, &req) {
            Ok(exports) => {
                info.loaded = true;
                info.file = exports.path.clone();
                info.files = exports.files.clone();
                info.diagnostics = exports.diagnostics.clone();
                info.uncacheable = exports.uncacheable;
                self.import_names(&req, &exports);
            }
            Err(e) => {
                info.uncacheable = true;
                if e.kind == ImportErrorKind::NotLoaded {
                    self.warn(req.span, e.message);
                } else {
                    self.error(req.span, EvalError::Invalid(e.message));
                }
                for (_, alias) in req.names.iter().flatten() {
                    self.define(
                        alias,
                        Binding::Imported {
                            from: req.path.clone(),
                            origin: self.origin(req.span),
                        },
                    );
                }
            }
        }
        self.result.imports.push(info);
    }

    /// Binds the names an import brings in, reporting clashes and missing names.
    fn import_names(&mut self, req: &ImportRequest, exports: &Exports) {
        if exports.error_count > 0 {
            let n = exports.error_count;
            let first = exports
                .diagnostics
                .iter()
                .find(|d| d.diagnostic.severity == Severity::Error);
            let detail = match first {
                Some(d) if n == 1 => format!(": {}", d.diagnostic.message),
                _ => String::new(),
            };
            self.error(
                req.span,
                EvalError::Invalid(format!(
                    "{n} error{} in {}{detail}",
                    if n == 1 { "" } else { "s" },
                    exports.file
                )),
            );
        }
        let selected: Vec<(String, String)> = match &req.names {
            Some(pairs) => pairs.clone(),
            None => exports
                .bindings
                .keys()
                .map(|k| (k.clone(), k.clone()))
                .collect(),
        };
        for (name, alias) in selected {
            let Some(found) = exports.bindings.get(&name) else {
                self.error(
                    req.span,
                    EvalError::Invalid(format!("`{name}` is not defined in {}", exports.file)),
                );
                continue;
            };
            let mut b = (**found).clone();
            let origin = b.origin_mut();
            origin.block = self.block;
            if origin.file.is_none() {
                origin.file = Some(exports.file.clone());
                origin.path = exports.path.clone();
            }
            if self.import_clashes(req, &alias, &name, &b, exports) {
                continue;
            }
            self.define(&alias, b);
        }
    }

    /// True when `alias` is already taken by something else (and reports it);
    /// importing the same definition twice is not a clash.
    fn import_clashes(
        &mut self,
        req: &ImportRequest,
        alias: &str,
        name: &str,
        incoming: &Binding,
        exports: &Exports,
    ) -> bool {
        let Some(prior) = self.view().get(alias, false) else {
            return false;
        };
        let (new_file, old_file) = (
            exports.file.to_string(),
            prior.origin().file.as_ref().map(|f| f.to_string()),
        );
        if old_file.as_deref() == Some(new_file.as_str())
            && prior.origin().span == incoming.origin().span
        {
            return true;
        }
        let here = self.inputs.doc_name.unwrap_or("this document").to_string();
        let old = old_file.unwrap_or_else(|| format!("{here} (before this import)"));
        let fix = format!("`Using ${name}$ as ${alias}_2$ from [[{}]]`", req.path);
        self.error(
            req.span,
            EvalError::Invalid(format!(
                "`{alias}` is defined in both {old} and {new_file}; rename one, for example with {fix}"
            )),
        );
        true
    }
}

/// The where-clause names and values and the conditions of one statement line.
struct Scope<'s> {
    names: &'s [String],
    locals: &'s Locals,
    conditions: &'s [&'s Node],
}
