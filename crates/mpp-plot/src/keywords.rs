//! The figure keywords that add a visual calculus helper or a data plot to a figure:
//! `tangent at`, `riemann …`, `area between`, `revolve about`, `slope field`,
//! `vector field`, `gradient field`, `tangent plane at`, `taylor`, `newton from`,
//! `scatter`, `histogram` and `box plot`. Each reads its `$…$` argument as a list and
//! builds [`Geometry`] for the curve it applies to. Numbers the reader may want to move
//! (the tangent point, the number of strips, the degree) become sliders of the figure.

use std::collections::BTreeMap;

use mpp_core::{Expr, ExprNode};
use mpp_eval::data as stats;
use mpp_eval::lower::LowerScope;
use mpp_eval::names::node_key;
use mpp_eval::Binding;
use mpp_syntax::document::FigureKeyword;
use mpp_syntax::math::{Node, NodeKind, RelOp as SynRel};

use crate::build::{Builder, STANDARD_NAMES};
use crate::compile::eval_const;
use crate::spec::{AxisLine, BoxStats, Geometry, Label, RiemannRule, Slider};

/// One entry of a keyword's argument list: `n = 8` (named), or a bare value.
pub(crate) struct Param {
    pub name: Option<String>,
    pub value: Expr,
    pub node: Node,
}

impl Param {
    fn is_interval(&self) -> bool {
        matches!(self.node.kind, NodeKind::Interval { .. })
    }

    fn is_tuple(&self) -> bool {
        matches!(self.node.kind, NodeKind::Tuple { .. })
    }

    /// A single number or named number (not an interval, a point or an equation of a line).
    fn is_scalar(&self) -> bool {
        !self.is_interval() && !self.is_tuple()
    }
}

impl Builder<'_> {
    /// Reads the argument list of a keyword. A relation `name = value` keeps its name.
    fn params(&mut self, nodes: &[Node]) -> Vec<Param> {
        let mut out = Vec::new();
        for node in nodes {
            if let NodeKind::Relation { first, rest } = &node.kind {
                if let ([(SynRel::Eq, rhs)], Some(name)) = (rest.as_slice(), node_key(first)) {
                    if let Some(value) = self.lower(rhs) {
                        out.push(Param {
                            name: Some(name),
                            value,
                            node: rhs.clone(),
                        });
                    }
                    continue;
                }
            }
            if let Some(value) = self.lower(node) {
                out.push(Param {
                    name: None,
                    value,
                    node: node.clone(),
                });
            }
        }
        out
    }

    /// The nth explicit curve `y = f(var)` of the figure, as `(f, var, dep)`.
    fn nth_explicit(&self, nth: usize) -> Option<(Expr, String, String)> {
        self.spec
            .layers
            .iter()
            .filter_map(|l| match &l.geometry {
                Geometry::Explicit { y, var, dep } => Some((y.clone(), var.clone(), dep.clone())),
                _ => None,
            })
            .nth(nth)
    }

    fn explicit_indices(&self) -> Vec<usize> {
        self.spec
            .layers
            .iter()
            .enumerate()
            .filter(|(_, l)| matches!(l.geometry, Geometry::Explicit { .. }))
            .map(|(i, _)| i)
            .collect()
    }

    fn var_range(&self, var: &str) -> Option<(f64, f64)> {
        let (a, b) = self.spec.ranges.get(var)?;
        let none = BTreeMap::new();
        let (a, b) = (eval_const(a, &none).ok()?, eval_const(b, &none).ok()?);
        (a.is_finite() && b.is_finite() && a < b).then_some((a, b))
    }

    /// A number the reader can move. A name that the document already made a slider keeps
    /// that slider; a value that uses other sliders is used as written; anything else
    /// becomes a slider of this figure, starting at the written value.
    fn param_expr(
        &mut self,
        p: Option<&Param>,
        hint: &str,
        default: f64,
        range: Option<(f64, f64)>,
        integer: bool,
    ) -> Expr {
        let name = p
            .and_then(|p| p.name.clone())
            .filter(|n| !STANDARD_NAMES.contains(&n.as_str()))
            .unwrap_or_else(|| hint.to_string());
        if self.slider_names.contains(&name) {
            return Expr::sym(&name);
        }
        let written = p.map(|p| self.inline(&p.value));
        if let Some(e) = &written {
            if !e.free_symbols().is_empty() {
                return e.clone();
            }
        }
        let mut value = written
            .and_then(|e| eval_const(&e, &BTreeMap::new()).ok())
            .filter(|v| v.is_finite())
            .unwrap_or(default);
        if integer {
            value = value.round();
        }
        let (mut lo, mut hi) = range.unwrap_or((value - 3.5, value + 3.5));
        lo = lo.min(value);
        hi = hi.max(value);
        if integer {
            lo = lo.floor();
            hi = hi.ceil();
        }
        if hi <= lo {
            hi = lo + 1.0;
        }
        let mut unique = name.clone();
        let mut k = 2;
        while self.slider_names.contains(&unique)
            || self.auto_sliders.iter().any(|s| s.name == unique)
        {
            unique = format!("{name}{k}");
            k += 1;
        }
        self.auto_sliders.push(Slider {
            name: unique.clone(),
            min: lo,
            max: hi,
            value,
            integer,
        });
        Expr::sym(&unique)
    }

    /// The first interval in an argument list, as its two ends.
    fn interval_of(&self, params: &[Param]) -> Option<(Expr, Expr)> {
        let p = params.iter().find(|p| p.is_interval())?;
        match self.inline(&p.value).node() {
            ExprNode::Interval { lo, hi, .. } => Some((lo.clone(), hi.clone())),
            _ => None,
        }
    }

    /// Adds the helper a keyword asks for. `nodes` is its `$…$` argument as a list.
    pub(crate) fn apply_keyword(&mut self, kw: FigureKeyword, nodes: &[Node]) {
        use FigureKeyword as K;
        match kw {
            K::TangentAt => self.add_tangent(nodes),
            K::RiemannLeft => self.add_riemann(RiemannRule::Left, nodes),
            K::RiemannRight => self.add_riemann(RiemannRule::Right, nodes),
            K::RiemannMid => self.add_riemann(RiemannRule::Mid, nodes),
            K::RiemannTrapezoid => self.add_riemann(RiemannRule::Trapezoid, nodes),
            K::AreaBetween => self.add_area(nodes),
            K::RevolveAbout => self.add_revolve(nodes),
            K::TangentPlaneAt => self.add_tangent_plane(nodes),
            K::Taylor => self.add_taylor(nodes),
            K::NewtonFrom => self.add_newton(nodes),
            K::SlopeField | K::VectorField | K::GradientField => self.add_field_from(kw, nodes),
            K::Scatter => self.add_scatter(nodes),
            K::Histogram => self.add_histogram(nodes),
            K::BoxPlot => self.add_box_plot(nodes),
            K::Polar | K::Parametric | K::Implicit | K::Points | K::Contour => {}
        }
    }

    fn need_curve(&mut self, kw: FigureKeyword) -> Option<(Expr, String, String)> {
        let found = self.nth_explicit(0);
        if found.is_none() {
            self.warn(&format!(
                "`{}` needs a curve `y = f(x)` in the same figure",
                kw.as_str()
            ));
        }
        found
    }

    fn add_tangent(&mut self, nodes: &[Node]) {
        let Some((f, var, _)) = self.need_curve(FigureKeyword::TangentAt) else {
            return;
        };
        let params = self.params(nodes);
        let range = self.var_range(&var);
        let default = range.map_or(0.0, |(a, b)| (a + b) / 2.0);
        let at = self.param_expr(
            params.iter().find(|p| p.is_scalar()),
            "x_0",
            default,
            range,
            false,
        );
        self.push(
            Geometry::Tangent { f, var, at },
            Some(Label::text("tangent line")),
        );
    }

    fn add_riemann(&mut self, rule: RiemannRule, nodes: &[Node]) {
        let kw = match rule {
            RiemannRule::Left => FigureKeyword::RiemannLeft,
            RiemannRule::Right => FigureKeyword::RiemannRight,
            RiemannRule::Mid => FigureKeyword::RiemannMid,
            RiemannRule::Trapezoid => FigureKeyword::RiemannTrapezoid,
        };
        let Some((f, var, _)) = self.need_curve(kw) else {
            return;
        };
        let params = self.params(nodes);
        let bounds = self.interval_of(&params);
        let given = params.iter().find(|p| p.is_scalar());
        let default = 8.0;
        let n = self.param_expr(given, "n", default, Some((1.0, 40.0)), true);
        // The slider reaches well past the starting value.
        if let Some(s) = self.auto_sliders.last_mut() {
            if s.integer && s.max < 3.0 * s.value {
                s.max = (3.0 * s.value).ceil();
            }
        }
        self.push(
            Geometry::Riemann {
                f,
                var,
                rule,
                n,
                bounds,
            },
            None,
        );
    }

    fn add_area(&mut self, nodes: &[Node]) {
        let Some((f, var, _)) = self.need_curve(FigureKeyword::AreaBetween) else {
            return;
        };
        let g = self.nth_explicit(1).map(|(g, _, _)| g);
        let params = self.params(nodes);
        let bounds = self.interval_of(&params);
        self.push(Geometry::AreaBetween { f, g, var, bounds }, None);
    }

    fn add_revolve(&mut self, nodes: &[Node]) {
        let Some((f, var, dep)) = self.need_curve(FigureKeyword::RevolveAbout) else {
            return;
        };
        let g = self.nth_explicit(1).map(|(g, _, _)| g);
        // Raw nodes: `y = 0` is a relation whose left side picks the kind of axis.
        let mut axis = AxisLine::Horizontal(Expr::int(0));
        let mut bounds = None;
        for node in nodes {
            match &node.kind {
                NodeKind::Relation { first, rest } => {
                    if let ([(SynRel::Eq, rhs)], Some(name)) = (rest.as_slice(), node_key(first)) {
                        if let Some(c) = self.lower(rhs) {
                            let c = self.inline(&c);
                            axis = if name == var {
                                AxisLine::Vertical(c)
                            } else {
                                AxisLine::Horizontal(c)
                            };
                        }
                    }
                }
                NodeKind::Interval { .. } => {
                    if let Some(e) = self.lower(node) {
                        if let ExprNode::Interval { lo, hi, .. } = self.inline(&e).node() {
                            bounds = Some((lo.clone(), hi.clone()));
                        }
                    }
                }
                _ => self.warn("`revolve about` takes an axis such as `y = 0` or `x = 2`"),
            }
        }
        let _ = dep;
        // The curves become the solid: remove them from the flat plot.
        let idx = self.explicit_indices();
        for i in idx.into_iter().take(2).rev() {
            self.spec.layers.remove(i);
        }
        self.push(
            Geometry::Revolve {
                f,
                g,
                var,
                bounds,
                axis,
            },
            None,
        );
    }

    fn add_tangent_plane(&mut self, nodes: &[Node]) {
        let found = self.spec.layers.iter().find_map(|l| match &l.geometry {
            Geometry::Surface { z, vars } => Some((z.clone(), vars.clone())),
            _ => None,
        });
        let Some((z, vars)) = found else {
            self.warn("`tangent plane at` needs a surface `z = f(x, y)` in the same figure");
            return;
        };
        let params = self.params(nodes);
        let mut coords: Vec<Expr> = Vec::new();
        for p in &params {
            if let NodeKind::Tuple { elems, .. } = &p.node.kind {
                coords = elems.iter().filter_map(|n| self.lower(n)).collect();
                break;
            }
        }
        if coords.len() != 2 {
            coords = params
                .iter()
                .filter(|p| p.is_scalar())
                .map(|p| p.value.clone())
                .collect();
        }
        let rx = self.var_range(&vars[0]);
        let ry = self.var_range(&vars[1]);
        let mid = |r: Option<(f64, f64)>| r.map_or(0.0, |(a, b)| (a + b) / 2.0);
        let mut at = Vec::new();
        for (k, hint, r) in [(0usize, "x_0", rx), (1, "y_0", ry)] {
            let written = coords.get(k).cloned().map(|value| Param {
                name: None,
                value,
                node: Node::error(mpp_syntax::Span::new(0, 0)),
            });
            at.push(self.param_expr(written.as_ref(), hint, mid(r), r, false));
        }
        let (Some(a), Some(b)) = (at.first().cloned(), at.get(1).cloned()) else {
            return;
        };
        self.push(
            Geometry::TangentPlane {
                z,
                vars,
                at: (a, b),
            },
            None,
        );
    }

    fn add_taylor(&mut self, nodes: &[Node]) {
        let Some((f, var, _)) = self.need_curve(FigureKeyword::Taylor) else {
            return;
        };
        let params = self.params(nodes);
        const DEGREE: [&str; 5] = ["n", "N", "k", "degree", "d"];
        let is_degree = |p: &Param| match &p.name {
            Some(n) => DEGREE.contains(&n.as_str()),
            None => false,
        };
        let degree = params
            .iter()
            .find(|p| is_degree(p))
            .or_else(|| params.iter().find(|p| p.name.is_none() && p.is_scalar()));
        let center = params.iter().find(|p| {
            p.is_scalar() && !is_degree(p) && !degree.is_some_and(|d| std::ptr::eq(*p, d))
        });
        let degree_expr = self.param_expr(degree, "n", 3.0, Some((0.0, 12.0)), true);
        let center_expr = match center {
            Some(c) => {
                let range = self.var_range(&var);
                self.param_expr(Some(c), "a", 0.0, range, false)
            }
            None => Expr::int(0),
        };
        self.push(
            Geometry::Taylor {
                f,
                var,
                center: center_expr,
                degree: degree_expr,
            },
            Some(Label::text("Taylor polynomial")),
        );
    }

    fn add_newton(&mut self, nodes: &[Node]) {
        let Some((f, var, _)) = self.need_curve(FigureKeyword::NewtonFrom) else {
            return;
        };
        let params = self.params(nodes);
        let steps_param = params
            .iter()
            .find(|p| matches!(p.name.as_deref(), Some("n" | "k" | "steps")));
        let start = params
            .iter()
            .find(|p| p.is_scalar() && !steps_param.is_some_and(|s| std::ptr::eq(*p, s)));
        let range = self.var_range(&var);
        let x0 = self.param_expr(start, "x_0", 1.0, range, false);
        let steps = self.param_expr(steps_param, "steps", 4.0, Some((0.0, 10.0)), true);
        self.push(Geometry::Newton { f, var, x0, steps }, None);
    }

    // ---- fields -----------------------------------------------------------------

    /// `slope field $y' = x - y$`, `slope field $x' = y, y' = -x$`, `vector field $(y, -x)$`,
    /// `gradient field $x^2 + y^2$`.
    fn add_field_from(&mut self, kw: FigureKeyword, nodes: &[Node]) {
        let mut exprs: Vec<Expr> = Vec::new();
        let mut inits: Vec<(Expr, Expr)> = Vec::new();
        for node in nodes {
            // `y(0) = 1`: a starting point of a solution curve.
            if let NodeKind::Relation { first, rest } = &node.kind {
                if let (NodeKind::Call { args, .. }, [(SynRel::Eq, rhs)]) =
                    (&first.kind, rest.as_slice())
                {
                    if let ([a], Some(b)) = (args.as_slice(), self.lower(rhs)) {
                        if let Some(a) = self.lower(a) {
                            inits.push((self.inline(&a), self.inline(&b)));
                            continue;
                        }
                    }
                }
                // `y' = f(x, y)`: remember the name of the unknown so it stays a symbol.
                if let NodeKind::Prime { base, .. } = &first.kind {
                    if let Some(dep) = node_key(base) {
                        self.keep.insert(dep);
                    }
                }
            }
            if let Some(e) = self.lower(node) {
                let e = self.inline(&e);
                // A constant pair after a slope field is a starting point.
                if kw == FigureKeyword::SlopeField {
                    if let ExprNode::Tuple(v) = e.node() {
                        if v.len() == 2 && e.free_symbols().is_empty() {
                            inits.push((v[0].clone(), v[1].clone()));
                            continue;
                        }
                    }
                }
                exprs.push(e);
            }
        }
        let label = match kw {
            FigureKeyword::SlopeField => "slope field",
            FigureKeyword::VectorField => "vector field",
            _ => "gradient field",
        };
        self.add_field_exprs(kw, exprs, inits, label);
    }

    /// Builds a field from lowered expressions (also used for `$…$, slope field`).
    pub(crate) fn add_field_exprs(
        &mut self,
        kw: FigureKeyword,
        exprs: Vec<Expr>,
        inits: Vec<(Expr, Expr)>,
        what: &str,
    ) {
        // Derivative forms: `y' = rhs` gives (unknown, rhs).
        let mut odes: Vec<(String, Expr)> = Vec::new();
        let mut plain: Vec<Expr> = Vec::new();
        for e in &exprs {
            match e.node() {
                ExprNode::Relation(mpp_core::RelOp::Eq, l, r) => match l.node() {
                    ExprNode::Derivative { expr, .. } if expr.as_sym().is_some() => {
                        odes.push((expr.as_sym().unwrap_or("y").to_string(), r.clone()));
                    }
                    _ => plain.push(e.clone()),
                },
                _ => plain.push(e.clone()),
            }
        }
        let names_of = |e: &[&Expr], skip: &[&str], b: &Builder<'_>| -> Vec<String> {
            let mut v: Vec<String> = Vec::new();
            for x in e {
                v.extend(
                    b.free(x)
                        .into_iter()
                        .filter(|n| !skip.contains(&n.as_str())),
                );
            }
            v.sort();
            v.dedup();
            v
        };
        match kw {
            FigureKeyword::SlopeField => {
                if odes.len() >= 2 {
                    let vars = [odes[0].0.clone(), odes[1].0.clone()];
                    let (dx, dy) = (odes[0].1.clone(), odes[1].1.clone());
                    self.push(
                        Geometry::PhasePortrait {
                            dx,
                            dy,
                            vars,
                            starts: inits,
                        },
                        None,
                    );
                } else if let Some((dep, rhs)) = odes.first() {
                    let others = names_of(&[rhs], &[dep.as_str()], self);
                    let indep = if others.iter().any(|n| n == "x") {
                        "x"
                    } else if others.iter().any(|n| n == "t") || dep == "x" {
                        "t"
                    } else {
                        "x"
                    };
                    self.push(
                        Geometry::SlopeField {
                            dydx: rhs.clone(),
                            vars: [indep.to_string(), dep.clone()],
                            starts: inits,
                        },
                        None,
                    );
                } else if let Some(e) = plain.first() {
                    if let ExprNode::Tuple(v) = e.node() {
                        if v.len() == 2 {
                            let vars = two_vars(self.order(names_of(&[&v[0], &v[1]], &[], self)));
                            self.push(
                                Geometry::PhasePortrait {
                                    dx: v[0].clone(),
                                    dy: v[1].clone(),
                                    vars,
                                    starts: inits,
                                },
                                None,
                            );
                            return;
                        }
                    }
                    let vars = two_vars(self.order(names_of(&[e], &[], self)));
                    self.push(
                        Geometry::SlopeField {
                            dydx: e.clone(),
                            vars,
                            starts: inits,
                        },
                        None,
                    );
                } else {
                    self.warn("`slope field` needs an equation such as `y' = x - y`");
                }
            }
            FigureKeyword::VectorField => {
                let comps: Vec<Expr> = match plain.as_slice() {
                    [one] => match one.node() {
                        ExprNode::Tuple(v) => v.clone(),
                        _ => vec![one.clone()],
                    },
                    many => many.to_vec(),
                };
                if comps.len() != 2 {
                    self.warn("`vector field` needs two components such as `(y, -x)` (3D fields are not drawn)");
                    return;
                }
                let vars = two_vars(self.order(names_of(&[&comps[0], &comps[1]], &[], self)));
                self.push(
                    Geometry::VectorField {
                        fx: comps[0].clone(),
                        fy: comps[1].clone(),
                        vars,
                    },
                    None,
                );
            }
            _ => {
                let Some(f) = plain.first() else {
                    self.warn("`gradient field` needs a function such as `x^2 + y^2`");
                    return;
                };
                let vars = two_vars(self.order(names_of(&[f], &[], self)));
                // Level curves under the arrows, as in a textbook figure.
                self.push(
                    Geometry::Contour {
                        f: f.clone(),
                        vars: vars.clone(),
                        levels: 8,
                    },
                    None,
                );
                self.push(Geometry::GradientField { f: f.clone(), vars }, None);
            }
        }
        let _ = what;
    }

    // ---- data -------------------------------------------------------------------

    /// The values of a data column named by a node.
    fn column(&mut self, node: &Node) -> Option<(String, Expr)> {
        let name = node_key(node)?;
        match self.scope.binding(&name).as_deref() {
            Some(Binding::Variable { value, .. }) if matches!(value.node(), ExprNode::Tuple(_)) => {
                Some((name, value.clone()))
            }
            _ => {
                self.warn(&format!("`{name}` is not a column of data"));
                None
            }
        }
    }

    fn add_scatter(&mut self, nodes: &[Node]) {
        let cols: Vec<(String, Expr)> = nodes.iter().filter_map(|n| self.column(n)).collect();
        let (xs, ys, names) = match cols.as_slice() {
            [(xn, x), (yn, y)] => (x.clone(), y.clone(), (xn.clone(), yn.clone())),
            [(yn, y)] => {
                let n = match y.node() {
                    ExprNode::Tuple(v) => v.len(),
                    _ => 0,
                };
                let idx = Expr::new(ExprNode::Tuple((1..=n as i64).map(Expr::int).collect()));
                (idx, y.clone(), ("i".to_string(), yn.clone()))
            }
            _ => {
                self.warn("`scatter` needs two columns, such as `scatter $t, x$`");
                return;
            }
        };
        match stats::scatter(&xs, &ys) {
            Ok(s) => {
                let pts: Vec<[f64; 2]> = s.points.iter().map(|(a, b)| [*a, *b]).collect();
                self.push(Geometry::Scatter(pts), None);
                self.spec.axis_names = Some(names);
            }
            Err(e) => self.warn(&e.to_string()),
        }
    }

    fn add_histogram(&mut self, nodes: &[Node]) {
        let Some(first) = nodes.first() else {
            self.warn("`histogram` needs a column, such as `histogram $m$`");
            return;
        };
        let Some((name, col)) = self.column(first) else {
            return;
        };
        let bins = nodes
            .get(1)
            .and_then(|n| self.lower(n))
            .and_then(|e| eval_const(&e, &BTreeMap::new()).ok())
            .filter(|v| v.is_finite() && *v >= 1.0)
            .map(|v| v as usize);
        match stats::histogram(&col, bins) {
            Ok(h) => {
                let mut edges: Vec<f64> = h.bins.iter().map(|b| b.0).collect();
                if let Some(last) = h.bins.last() {
                    edges.push(last.1);
                }
                let counts = h.bins.iter().map(|b| b.2 as f64).collect();
                self.push(Geometry::Histogram { edges, counts }, None);
                self.spec.axis_names = Some((name, "count".to_string()));
            }
            Err(e) => self.warn(&e.to_string()),
        }
    }

    fn add_box_plot(&mut self, nodes: &[Node]) {
        let mut boxes = Vec::new();
        for n in nodes {
            let Some((name, col)) = self.column(n) else {
                continue;
            };
            match stats::box_plot(&col) {
                Ok(b) => boxes.push(BoxStats {
                    label: name,
                    low: b.low,
                    q1: b.q1,
                    median: b.median,
                    q3: b.q3,
                    high: b.high,
                    outliers: b.outliers,
                }),
                Err(e) => self.warn(&e.to_string()),
            }
        }
        if boxes.is_empty() {
            self.warn("`box plot` needs a column, such as `box plot $m$`");
            return;
        }
        self.push(Geometry::BoxPlot(boxes), None);
        self.spec.axis_names = Some((String::new(), String::new()));
    }
}

/// Two plot variable names, filling in `x` and `y` when fewer are used.
fn two_vars(v: Vec<String>) -> [String; 2] {
    let mut v = v;
    for fill in ["x", "y"] {
        if v.len() < 2 && !v.iter().any(|n| n == fill) {
            v.push(fill.to_string());
        }
    }
    let mut it = v.into_iter();
    [
        it.next().unwrap_or_else(|| "x".into()),
        it.next().unwrap_or_else(|| "y".into()),
    ]
}
