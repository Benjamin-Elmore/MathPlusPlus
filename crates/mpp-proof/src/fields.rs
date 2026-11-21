//! Checking claims about abstract fields (vector calculus) on a random concrete instance.
//!
//! A claim such as `∂_t u_λ = λ³ (∂_t u)(λx, λ²t)` is about every solution `u`. The CAS
//! cannot work with an unknown `u`, so the checker builds one: random smooth fields
//! (sums of sines and polynomials) that satisfy the theorem's hypotheses (a
//! divergence-free `u` is a curl; a force that appears once in an equation is solved
//! from it), evaluates both sides exactly with the CAS's differentiation, and compares
//! them at random points. Agreement is evidence, not proof, so a claim that passes is ≈,
//! never ✓. A disagreement is ✗ only when every hypothesis could be imposed; otherwise
//! the claim is left unverified.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use mpp_core::{Expr, MathResult};
use mpp_syntax::math::{BinOp, Fence, MulStyle, NablaOp, Name, Node, NodeKind, UnaryOp};
use mpp_syntax::symbols::Font;
use num_complex::Complex64;

use crate::model::ClaimOutcome;
use crate::notation::{additive_terms, text, walk};

/// Coordinates of the physical space and of a similarity variable.
const X: [&str; 3] = ["x1", "x2", "x3"];
const Y: [&str; 3] = ["y1", "y2", "y3"];
const T: &str = "t";

/// The value of a subexpression on the instance.
#[derive(Debug, Clone)]
enum V {
    Scalar(Expr),
    Vector([Expr; 3]),
    /// `a · ∇`: the directional derivative along `a`.
    Op([Expr; 3]),
    /// A bare `∇`, waiting for a dot.
    Nabla,
    /// `(a, b)`: the arguments of a composition.
    Point(Vec<V>),
}

type R<T> = Result<T, String>;

fn err<T>(msg: impl Into<String>) -> R<T> {
    Err(msg.into())
}

fn m<T>(r: MathResult<T>) -> R<T> {
    r.map_err(|e| e.to_string())
}

/// Where an abstract field lives.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Space {
    /// Over `(x, t)`.
    X,
    /// Over the similarity variable `y` alone.
    Y,
}

/// A field declared by a call such as `u(x, t)` or `U(y)`.
#[derive(Debug, Clone)]
struct Field {
    vector: bool,
    space: Space,
}

/// The fields, definitions and hypotheses of one theorem.
#[derive(Debug, Clone, Default)]
pub(crate) struct FieldScope {
    /// Abstract fields by key.
    fields: BTreeMap<String, Field>,
    /// Definitions by key: parameter keys (empty for a plain name) and the body.
    defs: BTreeMap<String, (Vec<String>, Node)>,
    /// Relations assumed true (from equations the theorem refers to).
    hypotheses: Vec<Node>,
    /// The key of the coordinate vector (`\mathbf x`) and of time (`t`).
    coord: String,
    time: String,
    /// The key of the similarity variable (`\mathbf y`), when one is defined.
    sim: Option<String>,
}

/// The key of a name: its letter, `!b` when bold, and its subscript as written.
fn key(n: &Name, src: &str) -> String {
    let mut k = n.base.clone();
    if n.font == Some(Font::Bold) {
        k.push_str("!b");
    }
    if let Some(sub) = &n.sub {
        k.push('_');
        k.push_str(text(src, sub.span).trim());
    }
    k
}

fn name_key(node: &Node, src: &str) -> Option<String> {
    match &node.ungrouped().kind {
        NodeKind::Name(n) => Some(key(n, src)),
        // `i` and `e` are names where a sum or subscript binds them.
        NodeKind::Constant(c) => c.letter().map(|l| l.to_string()),
        NodeKind::Styled {
            font: Font::Bold,
            inner,
        } => match &inner.ungrouped().kind {
            NodeKind::Name(n) => {
                let mut n = n.clone();
                n.font = Some(Font::Bold);
                Some(key(&n, src))
            }
            _ => None,
        },
        _ => None,
    }
}

fn is_bold_key(k: &str) -> bool {
    k.split('_').next().is_some_and(|b| b.ends_with("!b"))
}

impl FieldScope {
    /// An empty scope with the usual coordinates `\mathbf x` and `t`.
    pub(crate) fn new() -> Self {
        FieldScope {
            coord: "x!b".into(),
            time: "t".into(),
            ..Default::default()
        }
    }

    /// True when the scope knows any field.
    pub(crate) fn has_fields(&self) -> bool {
        !self.fields.is_empty()
    }

    /// Reads declarations (`u(x, t)`, `U(y)`) and definitions (`s = T - t`,
    /// `u_λ(x, t) = λ u(λx, λ²t)`, `ω = ∇ × u`) from maths in a statement or setup
    /// paragraph. Later definitions replace earlier ones.
    pub(crate) fn read(&mut self, node: &Node, src: &str) {
        self.declare(node, src);
        // Definitions: `name = body` or `name(params) = body`.
        if let NodeKind::Relation { first, rest } = &node.ungrouped().kind {
            if let [(mpp_syntax::math::RelOp::Eq, body)] = rest.as_slice() {
                self.define(first, body, src);
            }
        }
    }

    /// Reads declarations only: a call of a name on the coordinates (`u(x, t)`) or on a
    /// similarity variable (`U(y)`).
    pub(crate) fn declare(&mut self, node: &Node, src: &str) {
        walk(node, &mut |n| {
            if let NodeKind::Call { callee, args, .. } = &n.kind {
                let Some(k) = name_key(callee, src) else {
                    return;
                };
                let arg_keys: Vec<Option<String>> = args.iter().map(|a| name_key(a, src)).collect();
                let space = match arg_keys.as_slice() {
                    [Some(a), Some(b)] if *a == self.coord && *b == self.time => Some(Space::X),
                    [Some(a)] if *a != self.coord && is_bold_key(a) => {
                        self.sim = Some(a.clone());
                        Some(Space::Y)
                    }
                    _ => None,
                };
                if let Some(space) = space {
                    if !self.defs.contains_key(&k) {
                        self.fields.insert(
                            k.clone(),
                            Field {
                                vector: is_bold_key(&k),
                                space,
                            },
                        );
                    }
                }
            }
        });
    }

    /// Records `head = body` as a definition when the head is a name (or a call of a
    /// name on plain names) other than a coordinate.
    pub(crate) fn define(&mut self, head: &Node, body: &Node, src: &str) {
        let (k, params) = match &head.ungrouped().kind {
            NodeKind::Call { callee, args, .. } => {
                let Some(k) = name_key(callee, src) else {
                    return;
                };
                let params: Option<Vec<String>> = args.iter().map(|a| name_key(a, src)).collect();
                match params {
                    Some(p) => (k, p),
                    None => return,
                }
            }
            _ => match name_key(head, src) {
                Some(k) => (k, Vec::new()),
                None => return,
            },
        };
        if k == self.coord || k == self.time {
            return;
        }
        self.fields.remove(&k);
        self.defs.insert(k, (params, body.clone()));
    }

    /// True when `side` is a name, or a call of a name, that the scope does not know (not
    /// a field, a definition or a coordinate): an equation with such a side defines it.
    fn names_new(&self, side: &Node, src: &str) -> bool {
        let callee = match &side.ungrouped().kind {
            NodeKind::Call { callee, .. } => callee.as_ref(),
            _ => side,
        };
        match name_key(callee, src) {
            Some(k) => {
                !self.fields.contains_key(&k)
                    && !self.defs.contains_key(&k)
                    && k != self.coord
                    && k != self.time
            }
            None => false,
        }
    }

    /// Adds a hypothesis (a relation assumed true).
    pub(crate) fn assume(&mut self, relation: &Node) {
        self.hypotheses.push(relation.clone());
    }

    /// True when `node` talks about a field: it names one (or a definition built from
    /// one), or uses `∇` or a partial derivative.
    pub(crate) fn mentions(&self, node: &Node, src: &str) -> bool {
        if !self.has_fields() {
            return false;
        }
        let mut hit = false;
        walk(node, &mut |n| match &n.kind {
            NodeKind::Nabla { .. } => hit = true,
            NodeKind::Derivative(d) if d.partial => hit = true,
            NodeKind::Name(name) => {
                let k = key(name, src);
                if self.fields.contains_key(&k)
                    || self
                        .defs
                        .get(&k)
                        .is_some_and(|(_, body)| self.mentions(body, src))
                {
                    hit = true;
                }
                // `u_j`: a component of the field `\mathbf u`.
                let mut bold = name.clone();
                bold.font = Some(Font::Bold);
                bold.sub = None;
                if name.sub.is_some() && self.fields.contains_key(&key(&bold, src)) {
                    hit = true;
                }
            }
            _ => {}
        });
        hit
    }

    /// Checks `lhs = rhs` on a random instance of the fields.
    pub(crate) fn check(&self, lhs: &Node, rhs: &Node, src: &str) -> ClaimOutcome {
        let inst = match Instance::build(self, src) {
            Ok(i) => i,
            Err(e) => return ClaimOutcome::Unverified(format!("no instance could be built: {e}")),
        };
        let l = inst.eval(lhs, &HashMap::new());
        let r = inst.eval(rhs, &HashMap::new());
        let (l, r) = match (l, r) {
            (Ok(l), Ok(r)) => (l, r),
            (Err(e), _) | (_, Err(e)) => {
                return ClaimOutcome::Unverified(format!(
                    "the field notation was not understood: {e}"
                ))
            }
        };
        let diff = match inst.difference(l, r) {
            Ok(d) => d,
            Err(e) => return ClaimOutcome::Unverified(e),
        };
        inst.compare(&diff)
    }
}

/// A random concrete instance of the scope's fields.
struct Instance<'a> {
    scope: &'a FieldScope,
    src: &'a str,
    /// Values of the abstract fields.
    fields: BTreeMap<String, V>,
    /// Hypotheses that could not be imposed.
    skipped: usize,
}

/// A small deterministic generator.
struct Rng(u64);

impl Rng {
    fn unit(&mut self) -> f64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 >> 11) as f64 / (1u64 << 53) as f64
    }

    fn coef(&mut self) -> Expr {
        // A small nonzero rational: ±k/4, k in 1..=8.
        let k = 1 + (self.unit() * 8.0) as i64;
        let k = if self.unit() < 0.5 { -k } else { k };
        Expr::rational(k, 4)
    }
}

fn sym(n: &str) -> Expr {
    Expr::sym(n)
}

fn sin(e: Expr) -> R<Expr> {
    m(Expr::call("sin", vec![e]))
}

impl<'a> Instance<'a> {
    fn build(scope: &'a FieldScope, src: &'a str) -> R<Self> {
        let mut rng = Rng(0x9E37_79B9_7F4A_7C15);
        let mut inst = Instance {
            scope,
            src,
            fields: BTreeMap::new(),
            skipped: 0,
        };
        // Divergence-free constraints and equations that can be solved for a field.
        let mut divergence_free = BTreeSet::new();
        let mut solve_for: Vec<(String, &Node)> = Vec::new();
        for h in &scope.hypotheses {
            let NodeKind::Relation { first, rest } = &h.ungrouped().kind else {
                continue;
            };
            let [(mpp_syntax::math::RelOp::Eq, rhs)] = rest.as_slice() else {
                continue;
            };
            if let (
                NodeKind::Nabla {
                    op: NablaOp::Div,
                    body: Some(b),
                },
                true,
            ) = (&first.ungrouped().kind, is_zero(rhs))
            {
                if let Some(k) = name_key(b, src) {
                    if scope.fields.contains_key(&k) {
                        divergence_free.insert(k);
                        continue;
                    }
                }
                inst.skipped += 1;
                continue;
            }
            // `u(x, 0) = u_0(x)` only names something new (`u_0`): nothing to impose.
            if [first.as_ref(), rhs]
                .iter()
                .any(|side| scope.names_new(side, src))
            {
                continue;
            }
            match inst.solvable_field(first, rhs) {
                Some(k) => solve_for.push((k, h)),
                None => inst.skipped += 1,
            }
        }
        let solved: BTreeSet<String> = solve_for.iter().map(|(k, _)| k.clone()).collect();
        for (k, f) in &scope.fields {
            if solved.contains(k) {
                continue;
            }
            let coords: &[&str; 3] = if f.space == Space::X { &X } else { &Y };
            let time = f.space == Space::X;
            let v = if f.vector && divergence_free.contains(k) {
                let a = [
                    random_scalar(&mut rng, coords, time)?,
                    random_scalar(&mut rng, coords, time)?,
                    random_scalar(&mut rng, coords, time)?,
                ];
                V::Vector(curl(&a, coords)?)
            } else if f.vector {
                V::Vector([
                    random_scalar(&mut rng, coords, time)?,
                    random_scalar(&mut rng, coords, time)?,
                    random_scalar(&mut rng, coords, time)?,
                ])
            } else {
                V::Scalar(random_scalar(&mut rng, coords, time)?)
            };
            inst.fields.insert(k.clone(), v);
        }
        // Solve each equation for its field: the field is minus the residual with it set
        // to zero, divided by its sign.
        for (k, h) in solve_for {
            let NodeKind::Relation { first, rest } = &h.ungrouped().kind else {
                continue;
            };
            let Some((_, rhs)) = rest.first() else {
                continue;
            };
            let vector = scope.fields.get(&k).is_some_and(|f| f.vector);
            let zero = if vector {
                V::Vector([Expr::zero(), Expr::zero(), Expr::zero()])
            } else {
                V::Scalar(Expr::zero())
            };
            inst.fields.insert(k.clone(), zero);
            let sign = inst.sign_of(&k, first, rhs).unwrap_or(1.0);
            let l = inst.eval(first, &HashMap::new())?;
            let r = inst.eval(rhs, &HashMap::new())?;
            let residual = sub(l, r)?;
            let value = scale(residual, &Expr::float(-1.0 / sign))?;
            inst.fields.insert(k, value);
        }
        Ok(inst)
    }

    /// The abstract field that appears exactly once, as a bare term of `lhs = rhs`.
    fn solvable_field(&self, lhs: &Node, rhs: &Node) -> Option<String> {
        let mut found = Vec::new();
        for side in [lhs, rhs] {
            for t in additive_terms(side.ungrouped()) {
                if let Some(k) = name_key(t.node, self.src) {
                    if self.scope.fields.contains_key(&k) {
                        found.push(k);
                    }
                }
            }
        }
        let k = found.first()?.clone();
        let mut count = 0;
        for side in [lhs, rhs] {
            walk(side, &mut |n| {
                if name_key(n, self.src).as_deref() == Some(k.as_str()) {
                    count += 1;
                }
            });
        }
        (count == 1).then_some(k)
    }

    /// +1 when the field is a positive term of the left side (or negative on the right).
    fn sign_of(&self, k: &str, lhs: &Node, rhs: &Node) -> Option<f64> {
        for (side, flip) in [(lhs, 1.0), (rhs, -1.0)] {
            for t in additive_terms(side.ungrouped()) {
                if name_key(t.node, self.src).as_deref() == Some(k) {
                    return Some(if t.negative { -flip } else { flip });
                }
            }
        }
        None
    }

    /// Evaluates a node with index variables bound in `env`.
    fn eval(&self, node: &Node, env: &HashMap<String, i64>) -> R<V> {
        let src = self.src;
        match &node.kind {
            NodeKind::Number(t) => {
                let x: f64 = t.trim().parse().map_err(|_| format!("the number {t}"))?;
                Ok(V::Scalar(number(x)))
            }
            NodeKind::Constant(c)
                if c.letter().is_some_and(|l| env.contains_key(&l.to_string())) =>
            {
                let l = c.letter().map(|l| l.to_string()).unwrap_or_default();
                Ok(V::Scalar(Expr::int(env.get(&l).copied().unwrap_or(0))))
            }
            NodeKind::Constant(c) => Ok(V::Scalar(match c {
                mpp_syntax::math::Constant::Pi => Expr::pi(),
                mpp_syntax::math::Constant::E => Expr::e(),
                mpp_syntax::math::Constant::I => return err("the imaginary unit"),
                mpp_syntax::math::Constant::Infinity => return err("infinity"),
            })),
            NodeKind::Name(n) => self.name(n, env),
            NodeKind::Styled { .. } => match name_key(node, src) {
                Some(k) => self.by_key(&k, env),
                None => err("a styled expression"),
            },
            NodeKind::Group { inner, .. } => self.eval(inner, env),
            NodeKind::Tuple { elems, .. } => Ok(V::Point(
                elems.iter().map(|e| self.eval(e, env)).collect::<R<_>>()?,
            )),
            NodeKind::Call { callee, args, .. } => self.call(callee, args, env),
            NodeKind::Unary { op, operand } => {
                let v = self.eval(operand, env)?;
                match op {
                    UnaryOp::Neg => scale(v, &Expr::int(-1)),
                    UnaryOp::Plus => Ok(v),
                    _ => err("a ± sign"),
                }
            }
            NodeKind::Binary { op, lhs, rhs } => {
                let a = self.eval(lhs, env)?;
                let b = self.eval(rhs, env)?;
                match op {
                    BinOp::Add => add(a, b),
                    BinOp::Sub => sub(a, b),
                    BinOp::Mul(MulStyle::Cdot) => self.dot(a, b),
                    BinOp::Mul(_) => self.product(a, b),
                    BinOp::Times => match (&a, &b) {
                        (V::Vector(x), V::Vector(y)) => Ok(V::Vector(cross(x, y))),
                        _ => self.product(a, b),
                    },
                    _ => err("this operator"),
                }
            }
            NodeKind::Frac { num, den, .. } => {
                let a = self.eval(num, env)?;
                let V::Scalar(d) = self.eval(den, env)? else {
                    return err("a vector in a denominator");
                };
                scale(a, &(Expr::one() / d))
            }
            NodeKind::Pow { base, exp } => {
                // `|u|^2` is `u · u`.
                if let NodeKind::Fenced {
                    fence: Fence::Abs | Fence::Norm,
                    inner,
                    ..
                } = &base.ungrouped().kind
                {
                    if is_two(exp) {
                        if let V::Vector(v) = self.eval(inner, env)? {
                            return Ok(V::Scalar(dot(&v, &v)));
                        }
                    }
                }
                let V::Scalar(b) = self.eval(base, env)? else {
                    return err("a power of a vector");
                };
                let V::Scalar(e) = self.eval(exp, env)? else {
                    return err("a vector exponent");
                };
                Ok(V::Scalar(b.pow(e)))
            }
            NodeKind::Sqrt {
                index: None,
                radicand,
                ..
            } => {
                let V::Scalar(b) = self.eval(radicand, env)? else {
                    return err("the root of a vector");
                };
                Ok(V::Scalar(b.sqrt()))
            }
            NodeKind::Fenced {
                fence: Fence::Abs | Fence::Norm,
                inner,
                ..
            } => match self.eval(inner, env)? {
                V::Vector(v) => Ok(V::Scalar(dot(&v, &v).sqrt())),
                V::Scalar(s) => Ok(V::Scalar(s.clone() * s).map_scalar(|e| e.sqrt())),
                _ => err("an absolute value"),
            },
            NodeKind::Nabla { op, body } => {
                let Some(body) = body else {
                    return Ok(V::Nabla);
                };
                let v = self.eval(body, env)?;
                self.nabla(*op, v)
            }
            NodeKind::Derivative(d) if d.partial => {
                let [var] = d.vars.as_slice() else {
                    return err("a mixed partial derivative");
                };
                let Some(of) = &d.of else {
                    return err("a partial derivative of nothing");
                };
                // `∂_i u_j ∂_j u_i`: the derivative takes the first factor when the rest
                // holds a derivative of its own.
                if let NodeKind::Binary {
                    op: BinOp::Mul(MulStyle::Implicit),
                    lhs,
                    rhs,
                } = &of.kind
                {
                    if has_derivative(rhs) {
                        let first = self.partial(&var.var, lhs, env)?;
                        let rest = self.eval(rhs, env)?;
                        return self.product(first, rest);
                    }
                }
                self.partial(&var.var, of, env)
            }
            NodeKind::BigOp(b) => self.sum(b, env),
            _ => err(format!("`{}`", text(src, node.span).trim())),
        }
    }

    fn name(&self, n: &Name, env: &HashMap<String, i64>) -> R<V> {
        let k = key(n, self.src);
        if let Some(i) = env.get(&n.base) {
            if n.sub.is_none() {
                return Ok(V::Scalar(Expr::int(*i)));
            }
        }
        // `u_j`: a component of the vector field `\mathbf u`.
        if let Some(sub) = &n.sub {
            let mut bold = n.clone();
            bold.font = Some(Font::Bold);
            bold.sub = None;
            let bk = key(&bold, self.src);
            let idx = match &sub.ungrouped().kind {
                NodeKind::Name(s) if s.sub.is_none() => env.get(&s.base).copied(),
                NodeKind::Constant(c) => c.letter().and_then(|l| env.get(&l.to_string()).copied()),
                NodeKind::Number(t) => t.trim().parse::<i64>().ok(),
                _ => None,
            };
            if let Some(i) = idx {
                if self.scope.fields.contains_key(&bk) || self.scope.defs.contains_key(&bk) {
                    if let V::Vector(v) = self.by_key(&bk, env)? {
                        let c = usize::try_from(i - 1).ok().and_then(|i| v.get(i).cloned());
                        return c
                            .map(V::Scalar)
                            .ok_or_else(|| "a component out of range".into());
                    }
                }
            }
        }
        self.by_key(&k, env)
    }

    fn by_key(&self, k: &str, env: &HashMap<String, i64>) -> R<V> {
        if k == self.scope.coord {
            return Ok(V::Vector([sym(X[0]), sym(X[1]), sym(X[2])]));
        }
        if k == self.scope.time {
            return Ok(V::Scalar(sym(T)));
        }
        if let Some(v) = self.fields.get(k) {
            return Ok(v.clone());
        }
        if let Some((params, body)) = self.scope.defs.get(k) {
            if params.is_empty() {
                return self.eval(body, env);
            }
            // A function of the coordinates, used bare: its value at (x, t).
            return self.eval(body, env);
        }
        // Anything else is a parameter (ν, λ, T): a fixed positive number.
        Ok(V::Scalar(Expr::float(stable_param(k))))
    }

    fn call(&self, callee: &Node, args: &[Node], env: &HashMap<String, i64>) -> R<V> {
        let Some(k) = name_key(callee, self.src) else {
            let f = self.eval(callee, env)?;
            let a = self.eval_args(args, env)?;
            return self.product(f, a);
        };
        let is_field = self.scope.fields.contains_key(&k)
            || self.scope.defs.get(&k).is_some_and(|(p, _)| !p.is_empty())
            || k == self.scope.sim.clone().unwrap_or_default();
        let value = self.by_key(&k, env)?;
        let point = self.eval_args(args, env)?;
        if !is_field {
            return self.product(value, point);
        }
        // Calling at the coordinates themselves changes nothing.
        let arg_keys: Vec<Option<String>> = args.iter().map(|a| name_key(a, self.src)).collect();
        if let Some((params, _)) = self.scope.defs.get(&k) {
            if arg_keys
                .iter()
                .zip(params)
                .all(|(a, p)| a.as_deref() == Some(p))
            {
                return Ok(value);
            }
        }
        let space = self.scope.fields.get(&k).map_or(Space::X, |f| f.space);
        if !point_fits(&point, space) {
            // `u(∇ · ω)`: a product written with brackets.
            return self.product(value, point);
        }
        compose(value, point, space)
    }

    fn eval_args(&self, args: &[Node], env: &HashMap<String, i64>) -> R<V> {
        if let [one] = args {
            self.eval(one, env)
        } else {
            Ok(V::Point(
                args.iter().map(|a| self.eval(a, env)).collect::<R<_>>()?,
            ))
        }
    }

    fn dot(&self, a: V, b: V) -> R<V> {
        match (a, b) {
            (V::Vector(x), V::Vector(y)) => Ok(V::Scalar(dot(&x, &y))),
            (V::Vector(x), V::Nabla) => Ok(V::Op(x)),
            (a, b) => self.product(a, b),
        }
    }

    fn product(&self, a: V, b: V) -> R<V> {
        match (a, b) {
            (V::Op(o), v) => self.apply(&o, v),
            (V::Scalar(s), v) | (v, V::Scalar(s)) if !matches!(v, V::Point(_)) => scale(v, &s),
            (v, V::Point(p)) if point_fits(&V::Point(p.clone()), Space::X) => {
                compose(v, V::Point(p), Space::X)
            }
            (V::Vector(_), V::Vector(_)) => err("a product of two vectors (write · or ×)"),
            _ => err("this product"),
        }
    }

    /// `(a · ∇) v`, differentiating in the space `v` lives in.
    fn apply(&self, a: &[Expr; 3], v: V) -> R<V> {
        let coords = self.space_of(&v);
        let v = if coords == Space::X { self.to_x(v)? } else { v };
        let names: &[&str; 3] = if coords == Space::X { &X } else { &Y };
        let d = |e: &Expr| -> R<Expr> {
            let mut acc = Expr::zero();
            for (ai, c) in a.iter().zip(names) {
                acc = acc + ai.clone() * m(e.diff(c, 1))?;
            }
            Ok(acc)
        };
        match v {
            V::Scalar(s) => Ok(V::Scalar(d(&s)?)),
            V::Vector([p, q, r]) => Ok(V::Vector([d(&p)?, d(&q)?, d(&r)?])),
            _ => err("a directional derivative of this"),
        }
    }

    fn nabla(&self, op: NablaOp, v: V) -> R<V> {
        let space = self.space_of(&v);
        let v = if space == Space::X { self.to_x(v)? } else { v };
        let c: &[&str; 3] = if space == Space::X { &X } else { &Y };
        let d = |e: &Expr, i: usize| -> R<Expr> { m(e.diff(c[i], 1)) };
        match (op, v) {
            (NablaOp::Grad, V::Scalar(s)) => Ok(V::Vector([d(&s, 0)?, d(&s, 1)?, d(&s, 2)?])),
            (NablaOp::Div, V::Vector(v)) => {
                Ok(V::Scalar(d(&v[0], 0)? + d(&v[1], 1)? + d(&v[2], 2)?))
            }
            (NablaOp::Curl, V::Vector(v)) => Ok(V::Vector(curl(&v, c)?)),
            (NablaOp::Laplacian, V::Scalar(s)) => Ok(V::Scalar(laplacian(&s, c)?)),
            (NablaOp::Laplacian, V::Vector([p, q, r])) => Ok(V::Vector([
                laplacian(&p, c)?,
                laplacian(&q, c)?,
                laplacian(&r, c)?,
            ])),
            _ => err("∇ applied to the wrong kind of value"),
        }
    }

    fn partial(&self, var: &Node, of: &Node, env: &HashMap<String, i64>) -> R<V> {
        let v = self.eval(of, env)?;
        let wrt: String = match name_key(var, self.src) {
            Some(k) if k == self.scope.time => T.to_string(),
            Some(k) => match env.get(&k) {
                Some(i) => {
                    let space = self.space_of(&v);
                    let c: &[&str; 3] = if space == Space::X { &X } else { &Y };
                    let idx = usize::try_from(*i - 1).map_err(|_| "an index below 1")?;
                    c.get(idx).ok_or("an index above 3")?.to_string()
                }
                None => return err(format!("a derivative in `{k}`")),
            },
            None => return err("a derivative in this variable"),
        };
        let v = if wrt == T || X.contains(&wrt.as_str()) {
            self.to_x(v)?
        } else {
            v
        };
        map_v(v, |e| m(e.diff(&wrt, 1)))
    }

    fn sum(&self, b: &mpp_syntax::math::BigOp, env: &HashMap<String, i64>) -> R<V> {
        if b.op != mpp_syntax::math::BigOpKind::Sum {
            return err("a product over an index");
        }
        let mut indices: Vec<String> = Vec::new();
        let mut from = 1i64;
        if let Some(i) = &b.index {
            indices.extend(name_key(i, self.src));
        }
        if let Some(over) = &b.over {
            walk(over, &mut |n| match &n.kind {
                NodeKind::Name(name) => indices.push(name.base.clone()),
                NodeKind::Constant(c) => indices.extend(c.letter().map(|l| l.to_string())),
                _ => {}
            });
            walk(over, &mut |n| {
                if let NodeKind::Number(t) = &n.kind {
                    if let Ok(v) = t.trim().parse::<i64>() {
                        from = v;
                    }
                }
            });
        }
        if let Some(f) = &b.from {
            if let NodeKind::Number(t) = &f.kind {
                from = t.trim().parse().unwrap_or(1);
            }
        }
        let to = match &b.to {
            Some(t) => match &t.kind {
                NodeKind::Number(t) => t.trim().parse().unwrap_or(3),
                _ => return err("a sum with a symbolic bound"),
            },
            None => 3,
        };
        if indices.is_empty() || indices.len() > 3 || to - from > 5 {
            return err("this sum");
        }
        let mut total: Option<V> = None;
        let mut idx = vec![from; indices.len()];
        loop {
            let mut env2 = env.clone();
            for (n, v) in indices.iter().zip(&idx) {
                env2.insert(n.clone(), *v);
            }
            let term = self.eval(&b.body, &env2)?;
            total = Some(match total {
                None => term,
                Some(t) => add(t, term)?,
            });
            // Next index tuple.
            let mut pos = indices.len();
            loop {
                if pos == 0 {
                    return total.ok_or_else(|| "an empty sum".into());
                }
                pos -= 1;
                if let Some(slot) = idx.get_mut(pos) {
                    if *slot < to {
                        *slot += 1;
                        break;
                    }
                    *slot = from;
                }
            }
        }
    }

    /// Which coordinates a value depends on: `Y` when it holds only `y` symbols.
    fn space_of(&self, v: &V) -> Space {
        let mut has_x = false;
        let mut has_y = false;
        for_each_expr(v, &mut |e| {
            for s in e.free_symbols() {
                if X.contains(&&*s) || &*s == T {
                    has_x = true;
                }
                if Y.contains(&&*s) {
                    has_y = true;
                }
            }
        });
        if has_y && !has_x {
            Space::Y
        } else {
            Space::X
        }
    }

    /// Rewrites `y` in terms of `x` and `t` through the similarity variable's definition.
    fn to_x(&self, v: V) -> R<V> {
        let mut any_y = false;
        for_each_expr(&v, &mut |e| {
            if e.free_symbols().iter().any(|s| Y.contains(&&**s)) {
                any_y = true;
            }
        });
        if !any_y {
            return Ok(v);
        }
        let Some(sim) = &self.scope.sim else {
            return err("a similarity variable without a definition");
        };
        let Some((_, body)) = self.scope.defs.get(sim) else {
            return err("a similarity variable without a definition");
        };
        let V::Vector(y) = self.eval(body, &HashMap::new())? else {
            return err("a similarity variable that is not a vector");
        };
        let pairs: Vec<(&str, Expr)> = Y.iter().copied().zip(y.iter().cloned()).collect();
        map_v(v, |e| Ok(e.subs_all(&pairs)))
    }

    /// The two sides' difference, in the coordinates `x`, `t`.
    fn difference(&self, l: V, r: V) -> R<V> {
        let l = self.to_x(l)?;
        let r = self.to_x(r)?;
        match (&l, &r) {
            (V::Vector(_), V::Scalar(z)) if is_zero_expr(z) => Ok(l),
            (V::Scalar(z), V::Vector(_)) if is_zero_expr(z) => Ok(r),
            _ => sub(l, r),
        }
    }

    /// Compares the difference with zero at random points.
    fn compare(&self, diff: &V) -> ClaimOutcome {
        let mut exprs = Vec::new();
        for_each_expr(diff, &mut |e| exprs.push(e.clone()));
        let mut rng = Rng(0xD1B5_4A32_D192_ED03);
        let mut points = 0;
        for _ in 0..6 {
            let mut env: HashMap<String, Complex64> = HashMap::new();
            for c in X {
                env.insert(c.to_string(), Complex64::new(-1.0 + 2.0 * rng.unit(), 0.0));
            }
            env.insert(T.to_string(), Complex64::new(0.05 + 0.25 * rng.unit(), 0.0));
            let mut worst = 0.0f64;
            let mut ok = true;
            for e in &exprs {
                match e.eval_complex(&env) {
                    Ok(z) if z.re.is_finite() && z.im.is_finite() => worst = worst.max(z.norm()),
                    _ => ok = false,
                }
            }
            if !ok {
                continue;
            }
            if worst > 1e-7 {
                let at: Vec<String> = ["x1", "x2", "x3", "t"]
                    .iter()
                    .map(|c| format!("{c} = {:.4}", env.get(*c).map_or(0.0, |z| z.re)))
                    .collect();
                let msg = format!(
                    "on a random instance the sides differ by {worst:.3e} at {}",
                    at.join(", ")
                );
                return if self.skipped == 0 {
                    ClaimOutcome::Failed(msg)
                } else {
                    ClaimOutcome::Unverified(format!(
                        "{msg}; {} hypothesis(es) could not be imposed on the instance",
                        self.skipped
                    ))
                };
            }
            points += 1;
        }
        if points >= 3 {
            ClaimOutcome::Numeric(format!(
                "holds on a random concrete instance of the fields at {points} points"
            ))
        } else {
            ClaimOutcome::Unverified("the instance could not be evaluated at enough points".into())
        }
    }
}

/// A parameter's value, fixed by its name so every evaluation agrees.
fn stable_param(k: &str) -> f64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in k.bytes() {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    0.6 + (h % 1000) as f64 / 1000.0 * 1.2
}

trait MapScalar {
    fn map_scalar(self, f: impl Fn(Expr) -> Expr) -> V;
}

impl MapScalar for V {
    fn map_scalar(self, f: impl Fn(Expr) -> Expr) -> V {
        match self {
            V::Scalar(s) => V::Scalar(f(s)),
            other => other,
        }
    }
}

fn number(x: f64) -> Expr {
    if x.fract() == 0.0 && x.abs() < 1e12 {
        Expr::int(x as i64)
    } else {
        Expr::float(x)
    }
}

fn is_zero(n: &Node) -> bool {
    matches!(&n.ungrouped().kind, NodeKind::Number(t) if t.trim().parse::<f64>() == Ok(0.0))
        || matches!(&n.ungrouped().kind, NodeKind::Name(name) if name.base == "0")
}

fn is_two(n: &Node) -> bool {
    matches!(&n.ungrouped().kind, NodeKind::Number(t) if t.trim() == "2")
}

fn is_zero_expr(e: &Expr) -> bool {
    e.is_zero()
}

fn has_derivative(n: &Node) -> bool {
    let mut found = false;
    walk(n, &mut |x| {
        if matches!(x.kind, NodeKind::Derivative(_)) {
            found = true;
        }
    });
    found
}

fn random_scalar(rng: &mut Rng, c: &[&str; 3], time: bool) -> R<Expr> {
    let mut acc = Expr::zero();
    for _ in 0..2 {
        let mut arg = Expr::zero();
        for v in c {
            arg = arg + rng.coef() * sym(v);
        }
        if time {
            arg = arg + rng.coef() * sym(T);
        }
        arg = arg + rng.coef();
        acc = acc + rng.coef() * sin(arg)?;
    }
    let i = (rng.unit() * 3.0) as usize % 3;
    let j = (rng.unit() * 3.0) as usize % 3;
    let mut poly = rng.coef() * sym(c[i]) * sym(c[j]);
    if time {
        poly = poly * (Expr::one() + rng.coef() * sym(T));
    }
    Ok(acc + poly)
}

fn dot(a: &[Expr; 3], b: &[Expr; 3]) -> Expr {
    a[0].clone() * b[0].clone() + a[1].clone() * b[1].clone() + a[2].clone() * b[2].clone()
}

fn cross(a: &[Expr; 3], b: &[Expr; 3]) -> [Expr; 3] {
    [
        a[1].clone() * b[2].clone() - a[2].clone() * b[1].clone(),
        a[2].clone() * b[0].clone() - a[0].clone() * b[2].clone(),
        a[0].clone() * b[1].clone() - a[1].clone() * b[0].clone(),
    ]
}

fn curl(v: &[Expr; 3], c: &[&str; 3]) -> R<[Expr; 3]> {
    let d = |e: &Expr, i: usize| -> R<Expr> { m(e.diff(c[i], 1)) };
    Ok([
        d(&v[2], 1)? - d(&v[1], 2)?,
        d(&v[0], 2)? - d(&v[2], 0)?,
        d(&v[1], 0)? - d(&v[0], 1)?,
    ])
}

fn laplacian(s: &Expr, c: &[&str; 3]) -> R<Expr> {
    let mut acc = Expr::zero();
    for v in c {
        acc = acc + m(s.diff(v, 2))?;
    }
    Ok(acc)
}

fn for_each_expr(v: &V, f: &mut dyn FnMut(&Expr)) {
    match v {
        V::Scalar(s) => f(s),
        V::Vector(x) | V::Op(x) => x.iter().for_each(f),
        V::Point(p) => p.iter().for_each(|q| for_each_expr(q, f)),
        V::Nabla => {}
    }
}

fn map_v(v: V, f: impl Fn(&Expr) -> R<Expr>) -> R<V> {
    map_dyn(v, &f)
}

fn map_dyn(v: V, f: &dyn Fn(&Expr) -> R<Expr>) -> R<V> {
    Ok(match v {
        V::Scalar(s) => V::Scalar(f(&s)?),
        V::Vector([a, b, c]) => V::Vector([f(&a)?, f(&b)?, f(&c)?]),
        V::Op([a, b, c]) => V::Op([f(&a)?, f(&b)?, f(&c)?]),
        V::Point(p) => V::Point(p.into_iter().map(|q| map_dyn(q, f)).collect::<R<_>>()?),
        V::Nabla => V::Nabla,
    })
}

fn add(a: V, b: V) -> R<V> {
    match (a, b) {
        (V::Scalar(x), V::Scalar(y)) => Ok(V::Scalar(x + y)),
        (V::Vector([a, b, c]), V::Vector([d, e, f])) => Ok(V::Vector([a + d, b + e, c + f])),
        (V::Vector(v), V::Scalar(z)) | (V::Scalar(z), V::Vector(v)) if z.is_zero() => {
            Ok(V::Vector(v))
        }
        _ => err("a sum of a scalar and a vector"),
    }
}

fn sub(a: V, b: V) -> R<V> {
    add(a, scale(b, &Expr::int(-1))?)
}

fn scale(v: V, s: &Expr) -> R<V> {
    map_v(v, |e| Ok(e.clone() * s.clone()))
}

/// True when `point` has the shape of a point of `space`: `(vector, scalar)` for `X`,
/// one vector for `Y`.
fn point_fits(point: &V, space: Space) -> bool {
    match (space, point) {
        (Space::X, V::Point(p)) => matches!(p.as_slice(), [V::Vector(_), V::Scalar(_)]),
        (Space::Y, V::Vector(_)) => true,
        (Space::Y, V::Point(p)) => matches!(p.as_slice(), [V::Vector(_)]),
        _ => false,
    }
}

/// The value at another point: `v(a, b)` substitutes the coordinates.
fn compose(v: V, point: V, space: Space) -> R<V> {
    let args = match point {
        V::Point(p) => p,
        other => vec![other],
    };
    let mut pairs: Vec<(String, Expr)> = Vec::new();
    match (space, args.as_slice()) {
        (Space::X, [V::Vector(a), V::Scalar(b)]) => {
            for (c, e) in X.iter().zip(a.iter()) {
                pairs.push((c.to_string(), e.clone()));
            }
            pairs.push((T.to_string(), b.clone()));
        }
        (Space::Y, [V::Vector(a)]) => {
            for (c, e) in Y.iter().zip(a.iter()) {
                pairs.push((c.to_string(), e.clone()));
            }
        }
        _ => return err("a field evaluated at a point of the wrong shape"),
    }
    // Substitute through fresh names first, so `x → λx, t → λ²t` happens at once.
    let fresh: Vec<(String, Expr)> = pairs
        .iter()
        .enumerate()
        .map(|(i, (c, _))| (c.clone(), sym(&format!("tmp{i}"))))
        .collect();
    let back: Vec<(String, Expr)> = pairs
        .iter()
        .enumerate()
        .map(|(i, (_, e))| (format!("tmp{i}"), e.clone()))
        .collect();
    let first: Vec<(&str, Expr)> = fresh.iter().map(|(a, b)| (a.as_str(), b.clone())).collect();
    let second: Vec<(&str, Expr)> = back.iter().map(|(a, b)| (a.as_str(), b.clone())).collect();
    map_v(v, |e| Ok(e.subs_all(&first).subs_all(&second)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use mpp_syntax::math::{parse_math, Statement};

    fn rel(src: &str) -> Node {
        let (ast, _) = parse_math(src, 0);
        match &ast.lines[0].statement {
            Statement::Relation(n) => n.clone(),
            other => panic!("not a relation: {other:?}"),
        }
    }

    fn sides(n: &Node) -> (Node, Node) {
        match &n.kind {
            NodeKind::Relation { first, rest } => ((**first).clone(), rest[0].1.clone()),
            _ => panic!("not a relation"),
        }
    }

    fn scope() -> (FieldScope, String) {
        let decl = "\\mathbf u(\\mathbf x, t) + p(\\mathbf x, t) + \\mathbf f(\\mathbf x, t) = 0";
        let ns = "\\partial_t \\mathbf u + (\\mathbf u \\cdot \\nabla)\\mathbf u = \\nu \\nabla^2 \\mathbf u - \\nabla p + \\mathbf f, \\qquad \\nabla \\cdot \\mathbf u = 0";
        let src = format!("{decl}\n{ns}\n");
        let mut s = FieldScope::new();
        let (a, _) = parse_math(decl, 0);
        if let Statement::Relation(n) = &a.lines[0].statement {
            s.read(n, &src);
        }
        let (b, _) = parse_math(ns, decl.len() + 1);
        if let Statement::Relation(n) = &b.lines[0].statement {
            s.assume(n);
        }
        for c in &b.lines[0].clauses {
            if let mpp_syntax::math::Clause::Condition(n) = c {
                s.assume(n);
            }
        }
        (s, src)
    }

    #[test]
    fn vector_identities_hold_and_wrong_ones_fail() {
        let (s, base) = scope();
        let claim = "(\\mathbf u \\cdot \\nabla)\\mathbf u = \\nabla\\big(\\tfrac12 |\\mathbf u|^2\\big) - \\mathbf u \\times (\\nabla \\times \\mathbf u)";
        let src = format!("{base}{claim}");
        let (ast, _) = parse_math(claim, base.len());
        let Statement::Relation(n) = &ast.lines[0].statement else {
            panic!("relation");
        };
        let (l, r) = sides(n);
        let out = s.check(&l, &r, &src);
        assert!(matches!(out, ClaimOutcome::Numeric(_)), "{out:?}");
        let wrong =
            "(\\mathbf u \\cdot \\nabla)\\mathbf u = \\nabla\\big(\\tfrac12 |\\mathbf u|^2\\big)";
        let src = format!("{base}{wrong}");
        let (ast, _) = parse_math(wrong, base.len());
        let Statement::Relation(n) = &ast.lines[0].statement else {
            panic!("relation");
        };
        let (l, r) = sides(n);
        assert!(matches!(s.check(&l, &r, &src), ClaimOutcome::Failed(_)));
    }

    #[test]
    fn hypotheses_shape_the_instance() {
        let (s, base) = scope();
        // ∇·u = 0 holds because u was built as a curl.
        let claim = "\\nabla \\cdot \\mathbf u = 0";
        let src = format!("{base}{claim}");
        let (ast, _) = parse_math(claim, base.len());
        let Statement::Relation(n) = &ast.lines[0].statement else {
            panic!("relation");
        };
        let (l, r) = sides(n);
        assert!(matches!(s.check(&l, &r, &src), ClaimOutcome::Numeric(_)));
        let _ = rel("a = b");
    }
}
