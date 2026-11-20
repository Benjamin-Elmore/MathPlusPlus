//! Building a [`FigureSpec`] from a `Figure:` block and the evaluated session.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use mpp_core::{Expr, ExprNode, LogicOp, RelOp};
use mpp_eval::lower::{LowerScope, Lowerer};
use mpp_eval::names::node_key;
use mpp_eval::{Binding, Evaluated, RangeKind, VarKind};
use mpp_syntax::document::{
    Block, BlockId, BlockKind, FigureItemKind, FigureKeyword, Inline, InlineKind,
};
use mpp_syntax::math::{parse_name_list, Node, Statement};
use mpp_syntax::Span;

use crate::compile::{eval_const, Defs};
use crate::error::PlotError;
use crate::figure::Figure;
use crate::spec::{FigureSpec, Geometry, Label, Layer, Slider};
use crate::view2d::name_latex;

/// One `Figure:` block of a document and what became of it.
#[derive(Debug)]
pub struct DocFigure {
    pub block: BlockId,
    pub span: Span,
    pub figure: Result<Figure, PlotError>,
}

/// Builds every figure in an evaluated document.
pub fn figures(ev: &Evaluated) -> Vec<DocFigure> {
    ev.document
        .blocks
        .iter()
        .filter(|b| matches!(b.kind, BlockKind::Figure(_)))
        .map(|b| DocFigure {
            block: b.id,
            span: b.span,
            figure: figure_for_block(ev, b),
        })
        .collect()
}

/// Builds the figure of one block.
pub fn figure_for_block(ev: &Evaluated, block: &Block) -> Result<Figure, PlotError> {
    let BlockKind::Figure(_) = &block.kind else {
        return Err(PlotError::Item("this block is not a figure".into()));
    };
    Figure::new(spec_for_block(ev, block)?)
}

/// The specification of a `Figure:` block (before sampling).
pub fn spec_for_block(ev: &Evaluated, block: &Block) -> Result<FigureSpec, PlotError> {
    let BlockKind::Figure(fig) = &block.kind else {
        return Err(PlotError::Item("this block is not a figure".into()));
    };
    let scope = BindingScope(&ev.bindings);
    let defs = Defs::from_bindings(&ev.bindings);
    let doc_sliders = document_sliders(ev, &defs);
    let slider_names: BTreeSet<String> = doc_sliders.iter().map(|s| s.name.clone()).collect();

    let mut b = Builder {
        scope: &scope,
        defs: &defs,
        spec: FigureSpec::default(),
        slider_names,
        keep: BTreeSet::new(),
        flags: Flags::default(),
    };
    b.spec.caption = Some(inline_text(ev, &fig.caption)).filter(|c| !c.is_empty());

    // Pass 1: ranges and keywords decide how the expressions are read.
    let mut points_args: Vec<Span> = Vec::new();
    for item in &fig.items {
        match &item.kind {
            FigureItemKind::Range(r) => {
                for n in r.names.iter().filter_map(node_key) {
                    b.keep.insert(n.clone());
                    if let Some(set) = b.lower(&r.set) {
                        match interval_ends(&set) {
                            Some(ends) => {
                                b.spec.ranges.insert(n, ends);
                            }
                            None => b.warn("a plotting range must be an interval such as `[0, 5]`"),
                        }
                    }
                }
            }
            FigureItemKind::Keyword { keyword, arg } => match keyword {
                FigureKeyword::Polar => b.flags.polar = true,
                FigureKeyword::Parametric => b.flags.parametric = true,
                FigureKeyword::Implicit => b.flags.implicit = true,
                FigureKeyword::Contour => b.flags.contour = true,
                FigureKeyword::Points => {
                    if arg.is_some() {
                        points_args.push(item.span);
                    }
                }
                other => b.warn(&format!("`{}` is not drawn yet", other.as_str())),
            },
            FigureItemKind::Expr(_) => {}
        }
    }
    b.add_standard_names();

    // Pass 2: expressions.
    for item in &fig.items {
        if let FigureItemKind::Expr(ast) = &item.kind {
            for line in &ast.lines {
                let node = match &line.statement {
                    Statement::Relation(n) | Statement::Query { expr: n, .. } => n,
                    Statement::Empty | Statement::Range(_) => continue,
                    _ => {
                        b.warn("only expressions, equations and inequalities can be drawn");
                        continue;
                    }
                };
                if let Some(e) = b.lower(node) {
                    b.add_expr(e);
                }
            }
        }
    }
    for span in points_args {
        let Some(text) = ev.source().get(span.start..span.end) else {
            continue;
        };
        let (Some(open), Some(close)) = (text.find('$'), text.rfind('$')) else {
            continue;
        };
        if close <= open {
            continue;
        }
        let base = span.start + open + 1;
        let (nodes, _) = parse_name_list(&text[open + 1..close], base);
        let lowered: Vec<Expr> = nodes.iter().filter_map(|n| b.lower(n)).collect();
        if !lowered.is_empty() {
            b.add_points(&Expr::new(ExprNode::Set(lowered)));
        }
    }

    let mut spec = b.spec;
    // Only the sliders this figure uses.
    let mut used: BTreeSet<String> = BTreeSet::new();
    for l in &spec.layers {
        for e in l.geometry.exprs() {
            used.extend(e.free_symbols().iter().map(|s| s.to_string()));
        }
    }
    for (lo, hi) in spec.ranges.values() {
        used.extend(lo.free_symbols().iter().map(|s| s.to_string()));
        used.extend(hi.free_symbols().iter().map(|s| s.to_string()));
    }
    spec.sliders = doc_sliders
        .into_iter()
        .filter(|s| used.contains(&s.name))
        .collect();
    if spec.layers.is_empty() {
        return Err(PlotError::Item(
            spec.warnings
                .first()
                .cloned()
                .unwrap_or_else(|| "the figure has nothing to draw".into()),
        ));
    }
    Ok(spec)
}

/// Sliders declared anywhere in the document (`a := 2, \; a \in [0, 5]`).
pub fn document_sliders(ev: &Evaluated, defs: &Defs) -> Vec<Slider> {
    let mut out: Vec<Slider> = Vec::new();
    let none = BTreeMap::new();
    for block in &ev.blocks {
        for r in block.ranges.iter().filter(|r| r.kind == RangeKind::Slider) {
            let Some((lo, hi)) = interval_ends(&r.set) else {
                continue;
            };
            let (Ok(min), Ok(max)) = (eval_const(&lo, &none), eval_const(&hi, &none)) else {
                continue;
            };
            if !(min.is_finite() && max.is_finite() && min < max) {
                continue;
            }
            for name in &r.names {
                let value = defs
                    .var(name)
                    .map(|v| defs.inline(v, &BTreeSet::new()))
                    .and_then(|v| eval_const(&v, &none).ok())
                    .filter(|v| v.is_finite())
                    .unwrap_or((min + max) / 2.0)
                    .clamp(min, max);
                out.retain(|s| &s.name != name);
                out.push(Slider {
                    name: name.clone(),
                    min,
                    max,
                    value,
                });
            }
        }
    }
    out
}

fn interval_ends(e: &Expr) -> Option<(Expr, Expr)> {
    match e.node() {
        ExprNode::Interval { lo, hi, .. } => Some((lo.clone(), hi.clone())),
        _ => None,
    }
}

fn inline_text(ev: &Evaluated, inlines: &[Inline]) -> String {
    let mut out = String::new();
    for i in inlines {
        match &i.kind {
            InlineKind::Text(t) | InlineKind::Code(t) => out.push_str(t),
            InlineKind::SoftBreak => out.push(' '),
            InlineKind::Emphasis(c) | InlineKind::Strong(c) => out.push_str(&inline_text(ev, c)),
            InlineKind::Link { text, .. } => out.push_str(&inline_text(ev, text)),
            InlineKind::Math(m) => {
                if let Some(s) = ev.source().get(m.raw.start..m.raw.end) {
                    out.push_str(s.trim());
                }
            }
            InlineKind::Escape(c) => out.push(*c),
            InlineKind::WikiLink { path, alias } => {
                out.push_str(alias.as_deref().unwrap_or(path));
            }
            InlineKind::FootnoteRef(_) | InlineKind::EqRef { .. } => {}
        }
    }
    out.trim().to_string()
}

/// Scope for lowering: the session's final bindings.
struct BindingScope<'a>(&'a BTreeMap<String, Arc<Binding>>);

impl LowerScope for BindingScope<'_> {
    fn binding(&self, name: &str) -> Option<Arc<Binding>> {
        self.0.get(name).cloned()
    }

    fn is_sequence(&self, _base: &str) -> bool {
        false
    }
}

#[derive(Default)]
struct Flags {
    polar: bool,
    parametric: bool,
    implicit: bool,
    contour: bool,
}

struct Builder<'a> {
    scope: &'a BindingScope<'a>,
    defs: &'a Defs,
    spec: FigureSpec,
    slider_names: BTreeSet<String>,
    /// Names that stay symbols when inlining: plot variables and sliders.
    keep: BTreeSet<String>,
    flags: Flags,
}

/// Axis and parameter names that are plot variables unless the document makes them constants.
const STANDARD_NAMES: [&str; 9] = ["x", "y", "z", "t", "u", "v", "θ", "φ", "theta"];

/// Order in which names are assigned to axes.
const PREFERRED: [&str; 12] = [
    "x", "y", "z", "u", "v", "r", "θ", "φ", "theta", "s", "t", "w",
];

fn order_vars(mut v: Vec<String>) -> Vec<String> {
    v.sort_by_key(|n| {
        (
            PREFERRED.iter().position(|p| p == n).unwrap_or(100),
            n.clone(),
        )
    });
    v
}

impl Builder<'_> {
    fn warn(&mut self, msg: &str) {
        self.spec.warnings.push(msg.to_string());
    }

    fn lower(&mut self, node: &Node) -> Option<Expr> {
        let locals: Vec<String> = self
            .keep
            .iter()
            .cloned()
            .chain(STANDARD_NAMES.iter().map(|s| (*s).to_string()))
            .collect();
        match Lowerer::new(self.scope).with_locals(locals).lower(node) {
            Ok(e) => Some(e),
            Err(l) => {
                self.warn(&l.error.to_string());
                None
            }
        }
    }

    fn add_standard_names(&mut self) {
        for n in STANDARD_NAMES {
            let constant = self.scope.binding(n).is_some_and(|b| {
                matches!(
                    &*b,
                    Binding::Variable {
                        kind: VarKind::Constant,
                        ..
                    }
                )
            });
            if !constant {
                self.keep.insert(n.to_string());
            }
        }
        self.keep.extend(self.slider_names.iter().cloned());
    }

    fn inline(&self, e: &Expr) -> Expr {
        self.defs.inline(e, &self.keep)
    }

    /// Free plot variables of an inlined expression (sliders excluded).
    fn free(&self, e: &Expr) -> Vec<String> {
        order_vars(
            e.free_symbols()
                .iter()
                .map(|s| s.to_string())
                .filter(|s| !self.slider_names.contains(s))
                .collect(),
        )
    }

    fn push(&mut self, geometry: Geometry, label: Option<Label>) {
        self.spec.layers.push(Layer { geometry, label });
    }

    fn add_expr(&mut self, lowered: Expr) {
        let label = Label::of(&lowered);
        let call_name = match lowered.node() {
            ExprNode::Apply(n, _) => Some(n.to_string()),
            _ => None,
        };
        let e = self.inline(&lowered);
        match e.node().clone() {
            ExprNode::Relation(op, l, r) => match op {
                RelOp::Eq => self.add_equation(&l, &r, label),
                RelOp::Lt | RelOp::Le | RelOp::Gt | RelOp::Ge => self.add_region(&e, label),
                RelOp::Ne | RelOp::Approx => {
                    self.warn("only `=`, `<`, `≤`, `>` and `≥` can be drawn");
                }
            },
            ExprNode::Logic(..) => self.add_region(&e, label),
            ExprNode::Tuple(items) => self.add_tuple(&items, label),
            ExprNode::Bool(_)
            | ExprNode::Matrix { .. }
            | ExprNode::Set(_)
            | ExprNode::Interval { .. } => {
                self.warn("this value cannot be drawn as a curve");
            }
            _ => self.add_scalar(&e, label, call_name),
        }
    }

    /// A bare expression: `f(x)`, `r(θ)` with `polar`, or `f(x, y)` as a surface.
    fn add_scalar(&mut self, e: &Expr, label: Label, call_name: Option<String>) {
        let free = self.free(e);
        if self.flags.polar {
            let var = free.first().cloned().unwrap_or_else(|| "θ".into());
            self.push(Geometry::Polar { r: e.clone(), var }, Some(label));
            return;
        }
        if self.flags.implicit {
            self.add_implicit(e.clone(), &free, label);
            return;
        }
        match free.len() {
            0 | 1 => {
                let var = free.first().cloned().unwrap_or_else(|| "x".into());
                let dep = call_name
                    .filter(|n| n.chars().count() == 1)
                    .unwrap_or_else(|| "y".into());
                self.push(
                    Geometry::Explicit {
                        y: e.clone(),
                        var,
                        dep,
                    },
                    Some(label),
                );
            }
            2 => {
                let vars = [free[0].clone(), free[1].clone()];
                let g = if self.flags.contour {
                    Geometry::Contour {
                        f: e.clone(),
                        vars,
                        levels: 10,
                    }
                } else {
                    Geometry::Surface { z: e.clone(), vars }
                };
                self.push(g, Some(label));
            }
            _ => self.warn(&format!(
                "`{}` has more than two free variables ({})",
                label.plain,
                free.join(", ")
            )),
        }
    }

    fn add_equation(&mut self, l: &Expr, r: &Expr, label: Label) {
        let (lf, rf) = (self.free(l), self.free(r));
        if self.flags.implicit {
            let f = mpp_core::add(vec![
                l.clone(),
                mpp_core::mul(vec![Expr::int(-1), r.clone()]),
            ]);
            let free = self.free(&f);
            self.add_implicit(f, &free, label);
            return;
        }
        // `name = expression` where the name does not occur on the other side.
        let lone = |side: &Expr, other_free: &[String]| -> Option<String> {
            side.as_sym()
                .filter(|s| !other_free.iter().any(|f| f == s) && !self.slider_names.contains(*s))
                .map(str::to_string)
        };
        let solved = lone(l, &rf)
            .map(|name| (name, r.clone(), rf.clone()))
            .or_else(|| lone(r, &lf).map(|name| (name, l.clone(), lf.clone())));
        if let Some((name, rhs, rhs_free)) = solved {
            if self.flags.polar || (name == "r" && !self.keep.contains("r")) {
                let var = rhs_free.first().cloned().unwrap_or_else(|| "θ".into());
                self.push(Geometry::Polar { r: rhs, var }, Some(label));
                return;
            }
            match rhs_free.len() {
                0 | 1 => {
                    let var = rhs_free.first().cloned();
                    if name == "x" && var.as_deref() != Some("x") {
                        self.push(
                            Geometry::ExplicitX {
                                x: rhs,
                                var: var.unwrap_or_else(|| "y".into()),
                            },
                            Some(label),
                        );
                    } else {
                        self.push(
                            Geometry::Explicit {
                                y: rhs,
                                var: var.unwrap_or_else(|| "x".into()),
                                dep: name,
                            },
                            Some(label),
                        );
                    }
                }
                2 => {
                    let vars = [rhs_free[0].clone(), rhs_free[1].clone()];
                    let g = if self.flags.contour {
                        Geometry::Contour {
                            f: rhs,
                            vars,
                            levels: 10,
                        }
                    } else {
                        Geometry::Surface { z: rhs, vars }
                    };
                    self.push(g, Some(label));
                }
                _ => self.warn(&format!("`{}` has too many free variables", label.plain)),
            }
            return;
        }
        let f = mpp_core::add(vec![
            l.clone(),
            mpp_core::mul(vec![Expr::int(-1), r.clone()]),
        ]);
        let free = self.free(&f);
        self.add_implicit(f, &free, label);
    }

    fn add_implicit(&mut self, f: Expr, free: &[String], label: Label) {
        match free.len() {
            0..=2 => {
                let mut vars: Vec<String> = free.to_vec();
                for fill in ["x", "y"] {
                    if vars.len() < 2 && !vars.iter().any(|v| v == fill) {
                        vars.push(fill.to_string());
                    }
                }
                let vars = order_vars(vars);
                self.push(
                    Geometry::Implicit {
                        f,
                        vars: [vars[0].clone(), vars[1].clone()],
                    },
                    Some(label),
                );
            }
            _ => self.warn("implicit surfaces in three variables are not drawn yet"),
        }
    }

    /// Inequalities (and their combinations) shade the region where they hold.
    fn add_region(&mut self, e: &Expr, label: Label) {
        let free = self.free(e);
        if free.len() > 2 {
            self.warn("a shaded region can use at most two variables");
            return;
        }
        let mut vars: Vec<String> = free;
        for fill in ["x", "y"] {
            if vars.len() < 2 && !vars.iter().any(|v| v == fill) {
                vars.push(fill.to_string());
            }
        }
        let vars = order_vars(vars);
        let mut boundary = Vec::new();
        collect_boundaries(e, &mut boundary);
        self.push(
            Geometry::Region {
                cond: e.clone(),
                boundary,
                vars: [vars[0].clone(), vars[1].clone()],
            },
            Some(label),
        );
    }

    fn add_tuple(&mut self, items: &[Expr], label: Label) {
        let free: BTreeSet<String> = items.iter().flat_map(|e| self.free(e)).collect();
        let free = order_vars(free.into_iter().collect());
        match (items.len(), free.len()) {
            (2, 0) => self.add_points(&Expr::new(ExprNode::Tuple(items.to_vec()))),
            (2, _) => {
                let var = free.first().cloned().unwrap_or_else(|| "t".into());
                if free.len() > 1 {
                    self.warn("a 2D parametric curve has one parameter");
                    return;
                }
                self.push(
                    Geometry::Parametric {
                        x: items[0].clone(),
                        y: items[1].clone(),
                        var,
                    },
                    Some(label),
                );
            }
            (3, 0) => self.warn("a single 3D point is not drawn"),
            (3, 1) => self.push(
                Geometry::Curve3 {
                    x: items[0].clone(),
                    y: items[1].clone(),
                    z: items[2].clone(),
                    var: free[0].clone(),
                },
                Some(label),
            ),
            (3, 2) => self.push(
                Geometry::ParamSurface {
                    x: items[0].clone(),
                    y: items[1].clone(),
                    z: items[2].clone(),
                    vars: [free[0].clone(), free[1].clone()],
                },
                Some(label),
            ),
            _ => self.warn("a point or curve needs two or three coordinates"),
        }
    }

    /// `points $(1, 2), (3, 4)$` or a lone tuple of numbers.
    fn add_points(&mut self, e: &Expr) {
        let mut pts: Vec<(Expr, Expr)> = Vec::new();
        collect_points(&self.inline(e), &mut pts);
        if pts.is_empty() {
            self.warn("`points` needs coordinates such as `(1, 2)`");
            return;
        }
        // All marked points share one layer, so they share one colour.
        if let Some(Layer {
            geometry: Geometry::Points(existing),
            ..
        }) = self.spec.layers.last_mut()
        {
            existing.extend(pts);
            return;
        }
        self.push(Geometry::Points(pts), None);
    }
}

fn collect_points(e: &Expr, out: &mut Vec<(Expr, Expr)>) {
    match e.node() {
        ExprNode::Tuple(v)
            if v.len() == 2 && v.iter().all(|c| !matches!(c.node(), ExprNode::Tuple(_))) =>
        {
            out.push((v[0].clone(), v[1].clone()));
        }
        ExprNode::Tuple(v) | ExprNode::Set(v) => {
            for c in v {
                collect_points(c, out);
            }
        }
        ExprNode::Matrix { cols: 2, data, .. } => {
            for row in data.chunks(2) {
                if let [a, b] = row {
                    out.push((a.clone(), b.clone()));
                }
            }
        }
        _ => {}
    }
}

/// The `lhs − rhs` of every inequality in a condition, with whether it is strict.
fn collect_boundaries(e: &Expr, out: &mut Vec<(Expr, bool)>) {
    match e.node() {
        ExprNode::Relation(op, l, r) => {
            let strict = match op {
                RelOp::Lt | RelOp::Gt => true,
                RelOp::Le | RelOp::Ge => false,
                _ => return,
            };
            let f = mpp_core::add(vec![
                l.clone(),
                mpp_core::mul(vec![Expr::int(-1), r.clone()]),
            ]);
            out.push((f, strict));
        }
        ExprNode::Logic(LogicOp::And | LogicOp::Or, args) => {
            for a in args {
                collect_boundaries(a, out);
            }
        }
        _ => {}
    }
}

/// A figure's label for an axis name, as LaTeX.
pub fn axis_latex(name: &str) -> String {
    name_latex(name)
}
