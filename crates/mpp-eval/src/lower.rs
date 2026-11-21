//! Lowering: the presentation-rich syntax tree becomes the meaning-only core tree.
//!
//! Most nodes map one to one. The decisions the parser leaves to this layer are
//! made here, against the names in scope:
//! - `f(x)` is a call when `f` is a function (user, sequence or built-in) and a
//!   product `f·x` otherwise; `a_n(x-c)^2` is `a_n·(x-c)^2` when `a_n` is not a function;
//! - `e` and `i` are the constants unless a definition, parameter, index or
//!   where-clause binds them;
//! - `a_k` is a term of the sequence `a` when the document has a recurrence for `a`.
//!
//! Calculus nodes become the core's unevaluated holders (see [`crate::holders`]).

use std::sync::Arc;

use mpp_core::{BigOpKind, Expr, ExprNode, Func, LimitDir, LogicOp, MathError, Number, RelOp};
use mpp_syntax::math::{
    Accent, BigOp, BigOpKind as SynBigOp, BinOp, CaseCondition, CaseRow, Constant, DerivNotation,
    Derivative, Fence, FuncApp, IntKind, Integral, Limit, MatrixEnv, MulStyle, NablaOp, Name,
    NamedOp, Node, NodeKind, RelOp as SynRel, Side, UnaryOp, UnitText,
};
use mpp_syntax::symbols::Font;
use mpp_syntax::Span;

use crate::error::{EvalError, Located};
use crate::names::{base_key, name_key, node_key};
use crate::value::Binding;

mod random;
mod region;

/// What lowering needs to know about the names in scope.
pub trait LowerScope {
    /// The binding of `name` at this point of the document.
    fn binding(&self, name: &str) -> Option<Arc<Binding>>;
    /// True when the document defines a recurrence for the sequence `base`.
    fn is_sequence(&self, base: &str) -> bool;
    /// True when `name` is an unknown function of a differential equation of the
    /// document, so `x(t)` elsewhere is that function at `t`, not `x · t`.
    fn is_ode_unknown(&self, _name: &str) -> bool {
        false
    }
}

/// Result of lowering one node.
pub type LowerResult = Result<Expr, Located>;

/// Names the core treats as built-in functions besides [`Func`]'s own.
const EXTRA_BUILTINS: &[&str] = &[
    "exp", "sqrt", "cbrt", "root", "lg", "asec", "arcsec", "acsc", "arccsc", "acot", "arccot",
    "sech", "csch", "coth",
];

/// The words of a "fill in: …" hint for a template box's label (`upper` → "upper bound").
pub fn long_label(label: &str) -> String {
    match label.trim() {
        "lower" => "lower bound".into(),
        "upper" => "upper bound".into(),
        "" => "the empty box".into(),
        other => other.into(),
    }
}

/// Functions written as calls of a free name: the Heaviside step `H(t − a)` and the
/// Dirac delta `δ(t − a)`, kept as the calls the Laplace tables read.
fn special_function(name: &str) -> Option<&'static str> {
    match name {
        "H" | "Heaviside" | "heaviside" => Some("H"),
        "δ" | "delta" | "Dirac" | "dirac" => Some("delta"),
        _ => None,
    }
}

/// True when `name` is a function the core knows.
pub fn is_builtin(name: &str) -> bool {
    Func::from_name(name).is_some() || EXTRA_BUILTINS.contains(&name)
}

/// Lowers syntax nodes against a scope.
pub struct Lowerer<'a> {
    scope: &'a dyn LowerScope,
    /// Names bound by the statement itself (parameters, indices, where-clauses).
    locals: Vec<String>,
    /// The function being defined, so its body can call itself.
    defining: Option<(String, Vec<String>)>,
    /// Unknown functions of a differential equation on this line (`y` in `y'' + y = 0,
    /// y(0) = 1`): `y(0)` is the value of `y` at 0, not `y·0`.
    unknown_fns: Vec<String>,
    /// Non-constant denominators met while lowering, so a helper can keep the points
    /// where the function as written is undefined (simplification may cancel them).
    denominators: Vec<Expr>,
}

fn fail(span: Span, error: EvalError) -> Located {
    Located::new(span, error)
}

fn math(span: Span) -> impl Fn(MathError) -> Located {
    move |e| Located::new(span, EvalError::Math(e))
}

impl<'a> Lowerer<'a> {
    /// A lowerer with no local names.
    pub fn new(scope: &'a dyn LowerScope) -> Self {
        Lowerer {
            scope,
            locals: Vec::new(),
            defining: None,
            unknown_fns: Vec::new(),
            denominators: Vec::new(),
        }
    }

    /// Adds names bound by the whole statement.
    pub fn with_locals(mut self, names: impl IntoIterator<Item = String>) -> Self {
        self.locals.extend(names);
        self
    }

    /// Treats `names` as the unknown functions of a differential equation: `y(0)` and
    /// `y'(0)` are values at a point, `y(x)` is `y` itself.
    pub fn with_unknown_functions(mut self, names: impl IntoIterator<Item = String>) -> Self {
        self.unknown_fns.extend(names);
        self
    }

    /// Lowers the function a helper analyses: `where defined(f, d₁, d₂, …)` when `f` was
    /// written with denominators, which simplification may have cancelled
    /// (`x²/x` lowers to `x`, but is undefined at 0).
    pub fn lower_helper_target(&mut self, node: &Node) -> LowerResult {
        let saved = std::mem::take(&mut self.denominators);
        let lowered = self.lower(node);
        let found = std::mem::replace(&mut self.denominators, saved);
        Ok(crate::helpers::where_defined(lowered?, found))
    }

    /// The denominators the user function `key` was written with, for its arguments.
    fn function_denominators(&mut self, key: &str, params: &[String], args: &[Expr]) {
        let Some(b) = self.scope.binding(&crate::helpers::denominators_key(key)) else {
            return;
        };
        let Binding::Variable { value, .. } = &*b else {
            return;
        };
        let ExprNode::Tuple(ds) = value.node() else {
            return;
        };
        for d in ds {
            let mut d = d.clone();
            for (p, a) in params.iter().zip(args) {
                d = d.subs(p, a);
            }
            if !d.is_constant() && !self.denominators.contains(&d) {
                self.denominators.push(d);
            }
        }
    }

    /// The denominators met so far (for a function definition to remember).
    pub fn take_denominators(&mut self) -> Vec<Expr> {
        std::mem::take(&mut self.denominators)
    }

    fn is_unknown_fn(&self, name: &str) -> bool {
        self.unknown_fns.iter().any(|n| n == name) && !self.is_local(name)
    }

    /// `y(a)`, `y'(a)` for an unknown function `y`: the call `y(a)` / `y′(a)` that the
    /// ODE solver reads as a condition, or `y` / `dy/dx` itself when `a` is the variable.
    fn unknown_fn_call(&mut self, key: &str, primes: u8, arg: &Node) -> LowerResult {
        let a = self.lower(arg)?;
        if let Some(v) = a.as_sym() {
            if !self.scope.binding(v).is_some_and(|b| b.params().is_none()) {
                let y = Expr::sym(key);
                return Ok(if primes == 0 {
                    y
                } else {
                    Expr::new(ExprNode::Derivative {
                        expr: y,
                        vars: vec![(v.into(), u32::from(primes))],
                    })
                });
            }
        }
        let name = format!("{key}{}", "′".repeat(usize::from(primes)));
        Ok(Expr::apply(&name, vec![a]))
    }

    /// Lets the body of the function `name` call itself.
    pub fn defining(mut self, name: &str, params: &[String]) -> Self {
        self.defining = Some((name.to_string(), params.to_vec()));
        self
    }

    fn is_local(&self, name: &str) -> bool {
        self.locals.iter().any(|l| l == name)
    }

    /// Runs `f` with `names` bound.
    fn bound<T>(&mut self, names: &[String], f: impl FnOnce(&mut Self) -> T) -> T {
        let len = self.locals.len();
        self.locals.extend(names.iter().cloned());
        let out = f(self);
        self.locals.truncate(len);
        out
    }

    /// The parameters when `name` is a user function here.
    fn function_params(&self, name: &str) -> Option<Vec<String>> {
        if self.is_local(name) {
            return None;
        }
        if let Some((def, params)) = &self.defining {
            if def == name {
                return Some(params.clone());
            }
        }
        self.scope
            .binding(name)
            .and_then(|b| b.params().map(<[String]>::to_vec))
    }

    fn is_sequence(&self, base: &str) -> bool {
        !self.is_local(base) && self.scope.is_sequence(base)
    }

    fn is_bound_letter(&self, letter: &str) -> bool {
        self.is_local(letter) || self.scope.binding(letter).is_some()
    }

    /// True when a call on `callee` applies a function (rather than multiplying).
    fn callee_is_function(&self, callee: &Node) -> bool {
        let callee = callee.ungrouped();
        if let NodeKind::Prime { base, .. } = &callee.kind {
            return node_key(base).is_some_and(|k| self.function_params(&k).is_some());
        }
        match node_key(callee) {
            Some(k) => {
                self.function_params(&k).is_some()
                    || self.is_sequence(&k)
                    || (!self.is_local(&k) && is_builtin(&k))
            }
            None => false,
        }
    }

    /// Lowers a list of nodes.
    pub fn lower_all(&mut self, nodes: &[Node]) -> Result<Vec<Expr>, Located> {
        nodes.iter().map(|n| self.lower(n)).collect()
    }

    /// Lowers one node.
    pub fn lower(&mut self, node: &Node) -> LowerResult {
        use NodeKind as K;
        let span = node.span;
        if matches!(
            node.kind,
            K::Call { .. } | K::Binary { .. } | K::NamedOp(_) | K::FuncApp(_)
        ) {
            if let Some(e) = self.random_operator(node)? {
                return Ok(e);
            }
        }
        match &node.kind {
            K::Number(text) => Number::parse_decimal(text).map(Expr::num).ok_or_else(|| {
                fail(
                    span,
                    EvalError::Invalid(format!("`{text}` is not a number")),
                )
            }),
            K::Name(n) => self.name(n, span),
            K::Constant(c) => Ok(self.constant(*c)),
            K::Symbol(s) => Err(fail(
                span,
                EvalError::Invalid(format!("`{s}` has no value of its own")),
            )),
            K::Dots(_) => Err(fail(span, EvalError::Unsupported("an ellipsis".into()))),
            // `\text{true}`, `\text{false}`: truth values (the right side of a check of
            // an inequality).
            K::Text(t) if matches!(t.trim(), "true" | "false") => {
                Ok(Expr::new(ExprNode::Bool(t.trim() == "true")))
            }
            K::Text(t) => Err(fail(
                span,
                EvalError::Invalid(format!("the text “{t}” cannot be evaluated")),
            )),
            K::Continuation => Err(fail(
                span,
                EvalError::Unsupported("a line that continues the one before".into()),
            )),
            K::Error => Err(fail(span, EvalError::Silent)),
            K::Placeholder { label } => Err(fail(span, EvalError::EmptyBox(long_label(label)))),
            K::Call { callee, args, .. } => self.call(callee, args, span),
            K::FuncApp(app) => self.func_app(app, span),
            K::Unary { op, operand } => {
                let x = self.lower(operand)?;
                Ok(match op {
                    UnaryOp::Neg => -x,
                    UnaryOp::Plus => x,
                    UnaryOp::PlusMinus => Expr::new(ExprNode::Set(vec![x.clone(), -x])),
                    UnaryOp::MinusPlus => Expr::new(ExprNode::Set(vec![-x.clone(), x])),
                    UnaryOp::Not => Expr::new(ExprNode::Logic(LogicOp::Not, vec![x])),
                })
            }
            K::Binary { op, lhs, rhs } => {
                if let Some(df) = self.total_differential(*op, lhs, rhs) {
                    return Ok(df);
                }
                if let Some(e) = self.ellipsis_chain(node)? {
                    return Ok(e);
                }
                if let Some(e) = self.spelled_names(node)? {
                    return Ok(e);
                }
                let a = self.lower(lhs)?;
                let b = self.lower(rhs)?;
                if let Some(m) = matrix_op(*op)
                    .and_then(|m| crate::domains_wire::matrix_binary(m, &a, &b, self.scope))
                {
                    return Ok(m);
                }
                Ok(binary(*op, a, b))
            }
            K::Frac { num, den, .. } => {
                let n = self.lower(num)?;
                let d = self.lower(den)?;
                if !d.is_constant() && !self.denominators.contains(&d) {
                    self.denominators.push(d.clone());
                }
                Ok(n / d)
            }
            K::Pow { base, exp } => self.power(base, exp),
            K::Subscript { base, sub } => match &base.ungrouped().kind {
                // `|x|_p`: the p-adic absolute value; `‖v‖_p`: the L^p norm.
                K::Fenced {
                    fence: Fence::Abs,
                    inner,
                    ..
                } => Ok(Expr::apply(
                    "padicabs",
                    vec![self.lower(sub)?, self.lower(inner)?],
                )),
                K::Fenced {
                    fence: Fence::Norm,
                    inner,
                    ..
                } => Ok(Expr::apply(
                    "lpnorm",
                    vec![self.lower(inner)?, self.lower(sub)?],
                )),
                _ => Err(fail(
                    span,
                    EvalError::Unsupported("a subscript on this expression".into()),
                )),
            },
            K::Prime { base, count } => self.prime(base, *count, span),
            K::Factorial { operand, count } => {
                let x = self.lower(operand)?;
                if *count == 1 {
                    Expr::func(Func::Factorial, vec![x]).map_err(math(span))
                } else {
                    Ok(Expr::apply("double_factorial", vec![x]))
                }
            }
            K::Fenced { fence, inner, .. } => {
                let x = self.lower(inner)?;
                match fence {
                    Fence::Abs => Expr::func(Func::Abs, vec![x]).map_err(math(span)),
                    Fence::Floor => Expr::func(Func::Floor, vec![x]).map_err(math(span)),
                    Fence::Ceil => Expr::func(Func::Ceil, vec![x]).map_err(math(span)),
                    Fence::Norm => Ok(Expr::apply("norm", vec![x])),
                }
            }
            K::Sqrt {
                index, radicand, ..
            } => {
                let r = self.lower(radicand)?;
                match index {
                    Some(i) => Ok(r.root(self.lower(i)?)),
                    None => Ok(r.sqrt()),
                }
            }
            K::Binom { n, k, .. } => {
                let args = vec![self.lower(n)?, self.lower(k)?];
                Expr::func(Func::Binomial, args).map_err(math(span))
            }
            K::Matrix { env, rows } => {
                let rows = rows
                    .iter()
                    .map(|r| self.lower_all(r))
                    .collect::<Result<Vec<_>, _>>()?;
                let m = Expr::matrix(rows).ok_or_else(|| {
                    fail(
                        span,
                        EvalError::Invalid("the rows of this matrix have different lengths".into()),
                    )
                })?;
                Ok(match env {
                    MatrixEnv::VMatrix => Expr::apply("det", vec![m]),
                    MatrixEnv::DoubleVMatrix => Expr::apply("norm", vec![m]),
                    _ => m,
                })
            }
            K::Cases { rows } => self.cases(rows),
            K::Set { elems, .. } => Ok(Expr::new(ExprNode::Set(self.lower_all(elems)?))),
            K::Interval {
                lo,
                hi,
                lo_closed,
                hi_closed,
                ..
            } => Ok(Expr::new(ExprNode::Interval {
                lo: self.lower(lo)?,
                hi: self.lower(hi)?,
                lo_closed: *lo_closed,
                hi_closed: *hi_closed,
            })),
            K::Tuple { elems, .. } => Ok(Expr::new(ExprNode::Tuple(self.lower_all(elems)?))),
            K::Group { inner, .. } | K::Styled { inner, .. } => self.lower(inner),
            K::BigOp(op) => self.big_op(op, span),
            K::Integral(i) => self.integral(i, span),
            K::Limit(l) => self.limit(l, span),
            K::Derivative(d) => self.derivative(d, span),
            K::Nabla { op, body } => {
                let body = body.as_ref().ok_or_else(|| {
                    fail(
                        span,
                        EvalError::Invalid("∇ needs something to act on".into()),
                    )
                })?;
                let name = match op {
                    NablaOp::Grad => "grad",
                    NablaOp::Div => "div",
                    NablaOp::Curl => "curl",
                    NablaOp::Laplacian => "laplacian",
                };
                // `∇f` for a user function differentiates by its parameters.
                if let Some(params) = node_key(body).and_then(|k| self.function_params(&k)) {
                    let vars = params.iter().map(|p| Expr::sym(p)).collect();
                    let target = self.bound(&params, |s| s.lower(body))?;
                    return Ok(Expr::apply(
                        name,
                        vec![target, Expr::new(ExprNode::Tuple(vars))],
                    ));
                }
                Ok(Expr::apply(name, vec![self.lower(body)?]))
            }
            K::NamedOp(op) => self.named_op(op),
            K::Accented { .. } => Err(fail(
                span,
                EvalError::Unsupported("an accent on this expression".into()),
            )),
            K::Relation { first, rest } => self.relation(first, rest),
            K::Quantity { value, unit } => self.quantity(value, unit),
            K::SetBuilder {
                var,
                domain,
                condition,
            } => {
                let x = self.bound_name(Some(var), span, "set")?;
                let d = match domain {
                    Some(d) => self.lower(d)?,
                    None => Expr::new(ExprNode::Tuple(Vec::new())),
                };
                let c = self.bound(std::slice::from_ref(&x), |s| s.lower(condition))?;
                Ok(Expr::apply(
                    crate::domains_wire::SET_OF,
                    vec![Expr::sym(&x), d, c],
                ))
            }
            K::Convert { expr, unit } => {
                let e = self.lower(expr)?;
                crate::units_wire::convert_expr(e, &unit.text).map_err(math(unit.span))
            }
        }
    }

    /// `9.81 m/s^2`. A plain one-letter unit that is a name in scope here (a parameter,
    /// an index or a defined variable) is that name instead: `3 t` with `f(t) := 3 t`.
    fn quantity(&mut self, value: &Node, unit: &UnitText) -> LowerResult {
        let v = self.lower(value)?;
        // `5\%` is the number 5/100: a percent is not a unit to carry along.
        if unit.text == "%" {
            return Ok(v / Expr::int(100));
        }
        if unit.plain && !unit.text.contains(['/', '·']) {
            let letters: Vec<String> = unit
                .text
                .chars()
                .take_while(char::is_ascii_alphabetic)
                .map(String::from)
                .collect();
            let named = |s: &Self, n: &str| s.is_local(n) || s.scope.binding(n).is_some();
            let as_names = !letters.is_empty() && letters.iter().all(|l| named(self, l));
            if as_names {
                // Read the "unit" again as maths: `3 t^2` is three times t squared.
                let (node, diags) = mpp_syntax::math::parse_expr(&unit.text, unit.span.start);
                if diags.is_empty() {
                    let names = self.lower(&node)?;
                    return Ok(v * names);
                }
            }
        }
        crate::units_wire::quantity_expr(v, &unit.text).map_err(math(unit.span))
    }

    fn name(&mut self, n: &Name, span: Span) -> LowerResult {
        if let Some(e) = self.data_name(n)? {
            return Ok(e);
        }
        if let Some(sub) = &n.sub {
            let base = base_key(n);
            if self.is_sequence(&base) {
                let index = self.lower(sub)?;
                return Ok(Expr::apply(&base, vec![index]));
            }
        }
        if let Some(e) = self.indexed_member(n)? {
            return Ok(e);
        }
        let key = name_key(n).ok_or_else(|| {
            fail(
                span,
                EvalError::Unsupported("a subscript this complicated on a name".into()),
            )
        })?;
        if let Some(params) = self.function_params(&key) {
            let args: Vec<Expr> = params.iter().map(|p| Expr::sym(p)).collect();
            self.function_denominators(&key, &params, &args);
            return Ok(Expr::apply(&key, args));
        }
        Ok(Expr::sym(&key))
    }

    /// True when `base` names a data column here (a variable holding a list of values).
    fn is_column(&self, base: &str) -> bool {
        !self.is_local(base)
            && matches!(
                self.scope.binding(base).as_deref(),
                Some(Binding::Variable { value, .. }) if matches!(value.node(), ExprNode::Tuple(_))
            )
    }

    /// The length of the data column `base`.
    fn column_len(&self, base: &str) -> Option<usize> {
        match self.scope.binding(base).as_deref() {
            Some(Binding::Variable { value, .. }) => match value.node() {
                ExprNode::Tuple(items) => Some(items.len()),
                _ => None,
            },
            _ => None,
        }
    }

    /// `\bar{x}` (the mean) and `x_i` (an entry) for a data column `x`, unless the
    /// decorated name is defined on its own.
    fn data_name(&mut self, n: &Name) -> Result<Option<Expr>, Located> {
        if !self.is_column(&n.base) {
            return Ok(None);
        }
        if let Some(key) = name_key(n) {
            if key != n.base && (self.is_local(&key) || self.scope.binding(&key).is_some()) {
                return Ok(None);
            }
        }
        match (&n.sub, n.accent) {
            (None, Some(Accent::Bar | Accent::Overline)) => {
                Ok(Some(Expr::apply("mean", vec![Expr::sym(&n.base)])))
            }
            (Some(sub), None) => {
                let index = self.lower(sub)?;
                Ok(Some(Expr::apply(
                    crate::domains_wire::ELEMENT,
                    vec![Expr::sym(&n.base), index],
                )))
            }
            _ => Ok(None),
        }
    }

    /// `\sum_i x_i` without bounds runs over every entry of the data column it indexes.
    /// `1 + 2 + \dots + n`, `n + (n-1) + \dots + 1` and `p_1 p_2 \cdots p_k`: a sum or
    /// product written with an ellipsis, read as the sum or product of its pattern.
    /// The terms before the dots must be an arithmetic progression (numbers that differ
    /// by a constant) or members of one indexed family with consecutive subscripts;
    /// anything else stays unsupported. `None` when `node` has no ellipsis.
    fn ellipsis_chain(&mut self, node: &Node) -> Result<Option<Expr>, Located> {
        let NodeKind::Binary { op, .. } = &node.kind else {
            return Ok(None);
        };
        let is_sum = match op {
            BinOp::Add => true,
            BinOp::Mul(_) => false,
            _ => return Ok(None),
        };
        fn flatten<'n>(n: &'n Node, sum: bool, out: &mut Vec<&'n Node>) {
            match &n.kind {
                NodeKind::Binary {
                    op: BinOp::Add,
                    lhs,
                    rhs,
                } if sum => {
                    flatten(lhs, sum, out);
                    flatten(rhs, sum, out);
                }
                NodeKind::Binary {
                    op: BinOp::Mul(_),
                    lhs,
                    rhs,
                } if !sum => {
                    flatten(lhs, sum, out);
                    flatten(rhs, sum, out);
                }
                _ => out.push(n),
            }
        }
        let mut terms = Vec::new();
        flatten(node, is_sum, &mut terms);
        let Some(at) = terms
            .iter()
            .position(|t| matches!(t.kind, NodeKind::Dots(_)))
        else {
            return Ok(None);
        };
        let unsupported = || {
            fail(
                node.span,
                EvalError::Unsupported("this pattern with an ellipsis".into()),
            )
        };
        let (before, after) = (&terms[..at], &terms[at + 1..]);
        let ([first, second, ..], [last]) = (before, after) else {
            return Err(unsupported());
        };
        let kind = if is_sum {
            BigOpKind::Sum
        } else {
            BigOpKind::Product
        };
        let var = ["j", "k", "m", "r"]
            .into_iter()
            .find(|v| !self.is_bound_letter(v))
            .ok_or_else(unsupported)?
            .to_string();
        // Members of an indexed family: p_1, p_2, …, p_k.
        let member = |t: &Node| -> Option<(String, Node)> {
            match &t.kind {
                NodeKind::Name(n) if n.accent.is_none() => {
                    n.sub.as_ref().map(|s| (base_key(n), (**s).clone()))
                }
                _ => None,
            }
        };
        if let (Some((b1, s1)), Some((b2, s2)), Some((bl, sl))) =
            (member(first), member(second), member(last))
        {
            if b1 == b2
                && b1 == bl
                && before
                    .iter()
                    .all(|t| member(t).is_some_and(|(b, _)| b == b1))
            {
                if self.scope.binding(&b1).is_some() || self.is_local(&b1) {
                    return Err(unsupported());
                }
                let (i1, i2) = (self.lower(&s1)?, self.lower(&s2)?);
                if !(i2.clone() - i1.clone()).simplify().is_one() {
                    return Err(unsupported());
                }
                let upper = self.lower(&sl)?;
                return Ok(Some(Expr::new(ExprNode::BigOp {
                    kind,
                    expr: mpp_core::family::member(&b1, Expr::sym(&var)),
                    var: var.as_str().into(),
                    lower: i1,
                    upper,
                })));
            }
        }
        // An arithmetic progression a, a + d, …, z: the terms a + j d, j = 0 … (z − a)/d.
        let lowered: Vec<Expr> = before
            .iter()
            .map(|t| self.lower(t))
            .collect::<Result<_, _>>()?;
        let a = lowered[0].clone();
        let d = (lowered[1].clone() - a.clone()).simplify();
        let constant_step = d.as_number().is_some_and(|n| n.is_exact() && !n.is_zero());
        let progression = lowered.windows(2).all(|w| {
            (w[1].clone() - w[0].clone() - d.clone())
                .simplify()
                .is_zero()
        });
        if !constant_step || !progression {
            return Err(unsupported());
        }
        let z = self.lower(last)?;
        let count = ((z - a.clone()) / d.clone()).simplify();
        // The last term must be reached: a whole, non-negative number of steps.
        if let Some(n) = count.as_number() {
            if n.as_i64().is_none_or(|k| k < 0) {
                return Err(unsupported());
            }
        }
        Ok(Some(Expr::new(ExprNode::BigOp {
            kind,
            expr: a + Expr::sym(&var) * d,
            var: var.as_str().into(),
            lower: Expr::zero(),
            upper: count,
        })))
    }

    /// The one index letter used as a subscript in `body` (`i` in `a_i b_i`), when it
    /// is free here; `None` when there is none or several.
    fn implied_index(&self, body: &Node) -> Option<String> {
        fn walk(n: &Node, out: &mut Vec<String>) {
            if let NodeKind::Name(name) = &n.kind {
                if let Some(sub) = &name.sub {
                    if let NodeKind::Name(s) = &sub.ungrouped().kind {
                        if s.sub.is_none()
                            && s.base.chars().count() == 1
                            && s.accent.is_none()
                            && !out.contains(&s.base)
                        {
                            out.push(s.base.clone());
                        }
                    }
                }
            }
            mpp_syntax::math::visit::children(n, &mut |c| walk(c, out));
        }
        let mut found = Vec::new();
        walk(body, &mut found);
        match found.as_slice() {
            [i] if !self.is_bound_letter(i) => Some(i.clone()),
            _ => None,
        }
    }

    /// `a_i` inside a sum over i, with neither `a_i` nor `a` defined: the i-th member
    /// of the family a (see [`mpp_core::family`]), so that each term of the sum is its own value
    /// (never `a_i` as one name repeated n times).
    fn indexed_member(&mut self, n: &Name) -> Result<Option<Expr>, Located> {
        let Some(sub) = &n.sub else {
            return Ok(None);
        };
        let Some(index) = node_key(sub.ungrouped()) else {
            return Ok(None);
        };
        if !self.is_local(&index) || n.accent.is_some() {
            return Ok(None);
        }
        let base = base_key(n);
        let key = name_key(n);
        if base == index
            || self.is_local(&base)
            || self.scope.binding(&base).is_some()
            || key
                .as_ref()
                .is_some_and(|k| self.scope.binding(k).is_some())
        {
            return Ok(None);
        }
        let i = self.lower(sub)?;
        Ok(Some(mpp_core::family::member(&base, i)))
    }

    fn data_bounds(&self, index: &str, body: &Node) -> Option<(Expr, Expr)> {
        let mut found = None;
        find_indexed(body, index, &mut |base| {
            if found.is_none() && self.is_column(base) {
                found = self.column_len(base);
            }
        });
        let n = found?;
        Some((Expr::one(), Expr::int(i64::try_from(n).ok()?)))
    }

    fn constant(&self, c: Constant) -> Expr {
        match c {
            Constant::Pi => Expr::pi(),
            Constant::Infinity => Expr::infinity(),
            Constant::E if self.is_bound_letter("e") => Expr::sym("e"),
            Constant::E => Expr::e(),
            Constant::I if self.is_bound_letter("i") => Expr::sym("i"),
            Constant::I => Expr::i(),
        }
    }

    fn call(&mut self, callee: &Node, args: &[Node], span: Span) -> LowerResult {
        // `\mathbf 1[A]`: the indicator, 1 where the condition holds and 0 elsewhere.
        if let (NodeKind::Name(n), [cond]) = (&callee.ungrouped().kind, args) {
            let bold_one = n.base == "𝟏" || (n.base == "1" && n.font == Some(Font::Bold));
            if bold_one && n.sub.is_none() && n.accent.is_none() {
                let cond = self.lower(cond)?;
                return Ok(Expr::new(ExprNode::Piecewise {
                    cases: vec![(Expr::one(), cond)],
                    otherwise: Some(Expr::zero()),
                }));
            }
        }
        if let Some(t) = crate::fields_wire::textbook(self.scope, &self.locals, callee, args.len())
        {
            return Ok(t.apply(self.lower_all(args)?));
        }
        let inner = callee.ungrouped();
        // `(\dot x)(0)` read as a call of the derivative: `x′(0)`.
        if let NodeKind::Derivative(d) = &inner.kind {
            if let (DerivNotation::Dot(n), Some(of)) = (d.notation, d.of.as_ref()) {
                if let Some(key) = node_key(of.ungrouped()) {
                    if let Some(params) = self.function_params(&key) {
                        return self.prime_call(&key, &params, n, args, span);
                    }
                    if let ([arg], true) = (args, self.is_unknown_fn(&key)) {
                        return self.unknown_fn_call(&key, n, arg);
                    }
                }
            }
        }
        if let NodeKind::Prime { base, count } = &inner.kind {
            if let Some(key) = node_key(base) {
                if let Some(params) = self.function_params(&key) {
                    return self.prime_call(&key, &params, *count, args, span);
                }
                // A built-in function (Γ′, ψ′) is never an unknown of a differential
                // equation; a textbook letter (W, J_0) is one when the line says so.
                let builtin = Func::from_name(&key).is_some();
                if let ([arg], true) = (args, builtin) {
                    if let Some(d) = self.known_prime(base, &key, *count, arg, span)? {
                        return Ok(d);
                    }
                }
                if let ([arg], true) = (args, self.is_unknown_fn(&key)) {
                    return self.unknown_fn_call(&key, *count, arg);
                }
                if let ([arg], false) = (args, builtin) {
                    if let Some(d) = self.known_prime(base, &key, *count, arg, span)? {
                        return Ok(d);
                    }
                }
            }
        }
        if let Some(key) = node_key(inner) {
            if self.function_params(&key).is_none() {
                if let ([arg], true) = (args, self.is_unknown_fn(&key)) {
                    return self.unknown_fn_call(&key, 0, arg);
                }
                // `u(0, t)`, `u(x, 0)`: the unknown of a partial differential equation at
                // a point, which its solver reads as a condition.
                if args.len() >= 2 && self.is_unknown_fn(&key) {
                    return Ok(Expr::apply(&key, self.lower_all(args)?));
                }
                // The Heaviside step and the Dirac delta, when the names are free.
                if let ([arg], Some(special)) = (args, special_function(&key)) {
                    if !self.is_bound_letter(&key) {
                        return Ok(Expr::apply(special, vec![self.lower(arg)?]));
                    }
                }
                // `V(x(t), y(t))` with x, y the unknowns of the document's differential
                // equations: the functions at the variable t (their derivatives stay
                // symbolic). Anything else in the brackets (`x (1 - x)`) is a product.
                if let [arg] = args {
                    let var = node_key(arg.ungrouped())
                        .filter(|v| v != &key && self.scope.binding(v).is_none() && arg.is_name());
                    if var.is_some()
                        && self.scope.is_ode_unknown(&key)
                        && self.scope.binding(&key).is_none()
                        && !self.is_local(&key)
                    {
                        return Ok(Expr::apply(&key, vec![self.lower(arg)?]));
                    }
                }
            }
            if let Some(params) = self.function_params(&key) {
                // `V(t)` for `V(x, y)` whose parameters are the unknowns of the
                // document's differential equations, now functions of t: the physics
                // shorthand for V(x(t), y(t)).
                if let ([arg], true) = (args, params.len() >= 2) {
                    let along_solution = params.iter().all(|p| {
                        !self.is_local(p)
                            && self.scope.is_ode_unknown(p)
                            && self.function_params(p).is_some_and(|q| q.len() == 1)
                    });
                    if along_solution {
                        let t = self.lower(arg)?;
                        let at: Vec<Expr> = params
                            .iter()
                            .map(|p| Expr::apply(p, vec![t.clone()]))
                            .collect();
                        return Ok(Expr::apply(&key, at));
                    }
                }
                if params.len() != args.len() {
                    return Err(fail(
                        span,
                        EvalError::Arity {
                            name: key,
                            expected: params.len(),
                            got: args.len(),
                        },
                    ));
                }
                let lowered = self.lower_all(args)?;
                self.function_denominators(&key, &params, &lowered);
                return Ok(Expr::apply(&key, lowered));
            }
            if self.is_sequence(&key) && args.len() == 1 {
                return Ok(Expr::apply(&key, self.lower_all(args)?));
            }
            if !self.is_local(&key) && is_builtin(&key) {
                let args = self.lower_all(args)?;
                return Expr::call(&key, args).map_err(math(span));
            }
            if args.len() != 1 {
                return Err(fail(callee.span, EvalError::NotAFunction(key)));
            }
        }
        match args {
            [arg] => {
                let f = self.lower(callee)?;
                Ok(f * self.lower(arg)?)
            }
            _ => Err(fail(
                callee.span,
                EvalError::Invalid(
                    "this is not a function, so it cannot take several arguments".into(),
                ),
            )),
        }
    }

    /// `f'(a)`: the derivative of the user function `f` at `a`.
    fn prime_call(
        &mut self,
        f: &str,
        params: &[String],
        count: u8,
        args: &[Node],
        span: Span,
    ) -> LowerResult {
        let [param] = params else {
            return Err(fail(
                span,
                EvalError::Unsupported("a prime on a function of several variables".into()),
            ));
        };
        let [arg] = args else {
            return Err(fail(
                span,
                EvalError::Arity {
                    name: format!("{f}'"),
                    expected: 1,
                    got: args.len(),
                },
            ));
        };
        let arg = self.lower(arg)?;
        let inner = Expr::new(ExprNode::Derivative {
            expr: Expr::apply(f, vec![Expr::sym(param)]),
            vars: vec![(param.as_str().into(), u32::from(count))],
        });
        if arg.is_sym(param) {
            return Ok(inner);
        }
        let at = Expr::equation(Expr::sym(param), arg);
        Ok(Expr::apply("subs", vec![inner, at]))
    }

    /// `Γ'(1)`, `\operatorname{erf}''(x)`, `J_0'(x)`, `W'(x)`: the derivative of a built-in
    /// or textbook function of one variable, at the argument. `None` when `key` is not
    /// one (or the document gives the name a meaning of its own).
    fn known_prime(
        &mut self,
        base: &Node,
        key: &str,
        count: u8,
        arg: &Node,
        span: Span,
    ) -> Result<Option<Expr>, Located> {
        const VAR: &str = "#prime";
        let t = Expr::sym(VAR);
        let f = if let Some(tb) = crate::fields_wire::textbook(self.scope, &self.locals, base, 1) {
            tb.apply(vec![t])
        } else if !self.is_local(key)
            && self.scope.binding(key).is_none()
            && Func::from_name(key)
                .is_some_and(|f| f.arity() == mpp_core::functions::Arity::Exactly(1))
        {
            Expr::call(key, vec![t]).map_err(math(span))?
        } else {
            return Ok(None);
        };
        let d = f.diff(VAR, u32::from(count)).map_err(math(span))?;
        let at = self.lower(arg)?;
        Ok(Some(d.subs(VAR, &at).simplify()))
    }

    /// `f'` with no argument; `y'` for a name that is not a function means d/dx.
    fn prime(&mut self, base: &Node, count: u8, span: Span) -> LowerResult {
        if let Some(key) = node_key(base) {
            if let Some(params) = self.function_params(&key) {
                let [param] = params.as_slice() else {
                    return Err(fail(
                        span,
                        EvalError::Unsupported("a prime on a function of several variables".into()),
                    ));
                };
                return Ok(Expr::new(ExprNode::Derivative {
                    expr: Expr::apply(&key, vec![Expr::sym(param)]),
                    vars: vec![(param.as_str().into(), u32::from(count))],
                }));
            }
        }
        let body = self.lower(base)?;
        // `x' = y, y' = -x`: when `x` is itself an unknown function, primes mean d/dt.
        let var = if self.is_unknown_fn("x") { "t" } else { "x" };
        Ok(Expr::new(ExprNode::Derivative {
            expr: body,
            vars: vec![(var.into(), u32::from(count))],
        }))
    }

    fn power(&mut self, base: &Node, exp: &Node) -> LowerResult {
        if let NodeKind::Call { callee, args, .. } = &base.kind {
            if let [arg] = args.as_slice() {
                if !self.callee_is_function(callee) {
                    // `a_n(x-c)^2` is `a_n · (x-c)^2`.
                    let f = self.lower(callee)?;
                    let x = self.lower(arg)?;
                    let e = self.lower(exp)?;
                    return Ok(f * x.pow(e));
                }
            }
        }
        let b = self.lower(base)?;
        let e = self.lower(exp)?;
        if let Some(m) = crate::domains_wire::matrix_power(&b, &e, self.scope)
            .or_else(|| crate::domains_wire::set_power(&b, &e))
        {
            return Ok(m);
        }
        Ok(b.pow(e))
    }

    fn func_app(&mut self, app: &FuncApp, span: Span) -> LowerResult {
        let name = app.func;
        // `\max_{t \in [0, 50]} f(t)`: the extremum over an interval.
        if let (Some(sub), "max" | "min", false) = (&app.sub, name, app.inverse) {
            if let NodeKind::Relation { first, rest } = &sub.ungrouped().kind {
                if let (Some(var), [(mpp_syntax::math::RelOp::In, set)]) =
                    (node_key(first), rest.as_slice())
                {
                    let set = self.lower(set)?;
                    let ExprNode::Interval { lo, hi, .. } = set.node() else {
                        return Err(fail(
                            sub.span,
                            EvalError::Unsupported(format!(
                                "{name} over a set that is not an interval"
                            )),
                        ));
                    };
                    let body =
                        self.bound(std::slice::from_ref(&var), |s| s.lower_all(&app.args))?;
                    let [body] = <[Expr; 1]>::try_from(body).map_err(|_| {
                        fail(
                            span,
                            EvalError::Unsupported(format!(
                                "{name} over an interval of several values"
                            )),
                        )
                    })?;
                    let op = if name == "max" {
                        crate::extremum::MAX_OVER
                    } else {
                        crate::extremum::MIN_OVER
                    };
                    let value =
                        Expr::apply(op, vec![body, Expr::sym(&var), lo.clone(), hi.clone()]);
                    return match &app.power {
                        Some(p) => Ok(value.pow(self.lower(p)?)),
                        None => Ok(value),
                    };
                }
            }
        }
        let args = self.lower_all(&app.args)?;
        if let ([one], "max" | "min", None, false) = (args.as_slice(), name, &app.sub, app.inverse)
        {
            if one.as_sym().is_some_and(|s| self.is_column(s)) {
                let op = if name == "max" {
                    crate::domains_wire::LIST_MAX
                } else {
                    crate::domains_wire::LIST_MIN
                };
                return Ok(Expr::apply(op, args));
            }
        }
        let value = if app.inverse {
            let inverse = match name {
                "sin" | "cos" | "tan" | "sec" | "csc" | "cot" | "sinh" | "cosh" | "tanh" => {
                    format!("a{name}")
                }
                _ => {
                    return Err(fail(
                        span,
                        EvalError::Unsupported(format!("the inverse of {name}")),
                    ))
                }
            };
            Expr::call(&inverse, args).map_err(math(span))?
        } else if let Some(sub) = &app.sub {
            if name != "log" {
                return Err(fail(
                    sub.span,
                    EvalError::Unsupported(format!("{name} with a subscript")),
                ));
            }
            let base = self.lower(sub)?;
            let [x] = <[Expr; 1]>::try_from(args).map_err(|a| {
                fail(
                    span,
                    EvalError::Math(MathError::WrongArity {
                        function: "log".into(),
                        expected: "1".into(),
                        got: a.len(),
                    }),
                )
            })?;
            Expr::func(Func::Log, vec![x, base]).map_err(math(span))?
        } else {
            Expr::call(name, args).map_err(math(span))?
        };
        match &app.power {
            Some(p) => Ok(value.pow(self.lower(p)?)),
            None => Ok(value),
        }
    }

    fn cases(&mut self, rows: &[CaseRow]) -> LowerResult {
        // A brace of equations with no conditions is a system: `\begin{cases} x' = y \\
        // y' = -x \end{cases}` is the tuple of its equations.
        let is_system = !rows.is_empty()
            && rows.iter().all(|r| {
                matches!(r.condition, CaseCondition::Missing)
                    && matches!(r.value.ungrouped().kind, NodeKind::Relation { .. })
            });
        if is_system {
            let eqs = rows
                .iter()
                .map(|r| self.lower(&r.value))
                .collect::<Result<Vec<_>, _>>()?;
            return Ok(Expr::new(ExprNode::Tuple(eqs)));
        }
        let mut cases = Vec::new();
        let mut otherwise = None;
        for row in rows {
            let value = self.lower(&row.value)?;
            match &row.condition {
                CaseCondition::When(c) if is_otherwise(c) => otherwise = Some(value),
                CaseCondition::When(c) => cases.push((value, self.lower(c)?)),
                CaseCondition::Otherwise => otherwise = Some(value),
                CaseCondition::Missing => {
                    return Err(fail(
                        row.value.span,
                        EvalError::Invalid("this case has no condition after `&`".into()),
                    ))
                }
            }
        }
        Ok(Expr::new(ExprNode::Piecewise { cases, otherwise }))
    }

    fn bound_name(&self, node: Option<&Node>, span: Span, what: &str) -> Result<String, Located> {
        node.and_then(node_key).ok_or_else(|| {
            fail(
                span,
                EvalError::Invalid(format!("this {what} has no variable")),
            )
        })
    }

    fn big_op(&mut self, op: &BigOp, span: Span) -> LowerResult {
        let kind = match op.op {
            SynBigOp::Sum => BigOpKind::Sum,
            SynBigOp::Prod => BigOpKind::Product,
            SynBigOp::Coprod => {
                return Err(fail(span, EvalError::Unsupported("the coproduct".into())))
            }
        };
        if let Some(over) = &op.over {
            return Err(fail(
                over.span,
                EvalError::Unsupported("a sum over a set".into()),
            ));
        }
        // `\sum a_i^2`: no index written; the subscript of an indexed family gives it,
        // over its usual range i = 1, …, n.
        let implied = op.index.is_none() && op.from.is_none() && op.to.is_none();
        let var = match (implied, self.implied_index(&op.body)) {
            (true, Some(i)) => i,
            _ => self.bound_name(op.index.as_ref(), span, "sum")?,
        };
        let (lower, upper) = match (&op.from, &op.to) {
            (Some(from), Some(to)) => (self.lower(from)?, self.lower(to)?),
            (None, None) if implied && self.data_bounds(&var, &op.body).is_none() => {
                (Expr::one(), Expr::sym("n"))
            }
            _ => match (&op.from, &op.to, self.data_bounds(&var, &op.body)) {
                (None, None, Some(bounds)) => bounds,
                _ => {
                    return Err(fail(
                        span,
                        EvalError::Invalid("this sum needs a start value and an end value".into()),
                    ))
                }
            },
        };
        let body = self.bound(std::slice::from_ref(&var), |s| s.lower(&op.body))?;
        Ok(Expr::new(ExprNode::BigOp {
            kind,
            expr: body,
            var: var.as_str().into(),
            lower,
            upper,
        }))
    }

    fn integral(&mut self, i: &Integral, span: Span) -> LowerResult {
        if let (Some(region), None) = (&i.lower, &i.upper) {
            if let Some(e) = self.region_integral(i, region)? {
                return Ok(e);
            }
        }
        if matches!(i.kind, IntKind::Contour | IntKind::Surface) {
            let what = if i.kind == IntKind::Contour {
                "∮ around a named curve"
            } else {
                "∯ over a named surface"
            };
            return Err(fail(
                span,
                EvalError::Unsupported(format!(
                    "{what} (parametrise it and integrate over the parameter instead)"
                )),
            ));
        }
        if i.form {
            return Err(fail(
                span,
                EvalError::Unsupported(
                    "a line integral of P dx + Q dy along a named curve (write the circle x² + y² = r² under the ∮, or parametrise it and integrate over the parameter)"
                        .into(),
                ),
            ));
        }
        if i.dot {
            return Err(fail(
                span,
                EvalError::Unsupported("an integral of a dot product with d𝐫".into()),
            ));
        }
        let vars = i
            .diffs
            .iter()
            .map(|d| {
                node_key(d).ok_or_else(|| {
                    fail(
                        d.span,
                        EvalError::Invalid("expected a variable after d".into()),
                    )
                })
            })
            .collect::<Result<Vec<_>, _>>()?;
        if vars.is_empty() {
            return Err(fail(
                span,
                EvalError::Invalid("this integral has no differential such as dx".into()),
            ));
        }
        let bounds = match (&i.lower, &i.upper) {
            (Some(lo), Some(hi)) => Some((self.lower(lo)?, self.lower(hi)?)),
            (None, None) => None,
            _ => {
                return Err(fail(
                    span,
                    EvalError::Unsupported("an integral over a region".into()),
                ))
            }
        };
        // `\iint_0^1 f dx dy`: the same bounds for every variable (a square or cube).
        // Variable bounds are written as nested integrals, `\int_0^1 \int_0^x f dy dx`.
        let body = self.bound(&vars, |s| s.lower(&i.body))?;
        let mut out = body;
        for var in &vars {
            out = Expr::new(ExprNode::Integral {
                expr: out,
                var: var.as_str().into(),
                bounds: bounds.clone(),
            });
        }
        Ok(out)
    }

    fn limit(&mut self, l: &Limit, span: Span) -> LowerResult {
        let var = self.bound_name(Some(&l.var), span, "limit")?;
        let point = self.lower(&l.target)?;
        let body = self.bound(std::slice::from_ref(&var), |s| s.lower(&l.body))?;
        let dir = match l.side {
            None => LimitDir::Both,
            Some(Side::Left) => LimitDir::Left,
            Some(Side::Right) => LimitDir::Right,
        };
        Ok(Expr::new(ExprNode::Limit {
            expr: body,
            var: var.as_str().into(),
            point,
            dir,
        }))
    }

    /// An implicit product in which adjacent letters spell a name the document
    /// defines (`velocity · 2` after `velocity := 3`): that name, not `v·e·l·o·c·i·t·y`.
    /// `None` when no run of letters is a defined name.
    fn spelled_names(&mut self, node: &Node) -> Result<Option<Expr>, Located> {
        use mpp_syntax::math::letters::{implicit_factors, letter_runs};
        if !matches!(
            node.kind,
            NodeKind::Binary {
                op: BinOp::Mul(MulStyle::Implicit),
                ..
            }
        ) {
            return Ok(None);
        }
        let factors = implicit_factors(node);
        let runs: Vec<_> = letter_runs(&factors)
            .into_iter()
            .filter(|r| self.is_local(&r.word) || self.scope.binding(&r.word).is_some())
            .collect();
        if runs.is_empty() {
            return Ok(None);
        }
        let mut terms = vec![];
        let mut i = 0;
        while i < factors.len() {
            if let Some(r) = runs.iter().find(|r| r.start == i) {
                let name = self.name(&Name::simple(r.word.clone()), r.span)?;
                let term = match &factors[r.end - 1].kind {
                    NodeKind::Pow { exp, .. } if r.powered => name.pow(self.lower(exp)?),
                    _ => name,
                };
                terms.push(term);
                i = r.end;
            } else {
                terms.push(self.lower(factors[i])?);
                i += 1;
            }
        }
        Ok(terms.into_iter().reduce(|a, b| a * b))
    }

    /// `df` for a user function `f(x, y)`: the total differential `f_x dx + f_y dy`, with
    /// `dx`, `dy` as names. `None` unless `d` is free and `f` is a user function.
    fn total_differential(&mut self, op: BinOp, lhs: &Node, rhs: &Node) -> Option<Expr> {
        if !matches!(op, BinOp::Mul(MulStyle::Implicit)) {
            return None;
        }
        if node_key(lhs).as_deref() != Some("d") || self.is_bound_letter("d") {
            return None;
        }
        let f = node_key(rhs)?;
        let params = self.function_params(&f)?;
        let call = Expr::apply(&f, params.iter().map(|p| Expr::sym(p)).collect());
        let terms = params
            .iter()
            .map(|p| {
                Expr::new(ExprNode::Derivative {
                    expr: call.clone(),
                    vars: vec![(p.as_str().into(), 1)],
                }) * Expr::sym(&format!("d{p}"))
            })
            .collect();
        Some(mpp_core::add(terms))
    }

    /// `\frac{\partial(u, v)}{\partial(x, y)}`: the Jacobian determinant `det(∂uᵢ/∂xⱼ)`.
    fn jacobian(&mut self, d: &Derivative) -> Result<Option<Expr>, Located> {
        let tuple = |n: &Node| match &n.ungrouped().kind {
            NodeKind::Tuple { elems, .. } => Some(elems.clone()),
            NodeKind::Fenced { inner, .. } => match &inner.ungrouped().kind {
                NodeKind::Tuple { elems, .. } => Some(elems.clone()),
                _ => None,
            },
            _ => None,
        };
        let (Some(of), [var]) = (&d.of, d.vars.as_slice()) else {
            return Ok(None);
        };
        let (Some(outputs), Some(inputs)) = (tuple(of), tuple(&var.var)) else {
            return Ok(None);
        };
        let names: Vec<String> = inputs.iter().filter_map(node_key).collect();
        if names.len() != inputs.len() || outputs.len() != names.len() {
            return Err(fail(
                var.var.span,
                EvalError::Invalid(
                    "a Jacobian ∂(u, v)/∂(x, y) needs as many functions as variables".into(),
                ),
            ));
        }
        let mut rows = Vec::with_capacity(outputs.len());
        for out in &outputs {
            let f = self.bound(&names, |s| s.lower(out))?;
            rows.push(
                names
                    .iter()
                    .map(|v| {
                        Expr::new(ExprNode::Derivative {
                            expr: f.clone(),
                            vars: vec![(v.as_str().into(), 1)],
                        })
                    })
                    .collect(),
            );
        }
        Ok(Expr::matrix(rows).map(|m| Expr::apply("det", vec![m])))
    }

    fn derivative(&mut self, d: &Derivative, span: Span) -> LowerResult {
        if d.partial {
            if let Some(j) = self.jacobian(d)? {
                return Ok(j);
            }
        }
        if matches!(d.notation, DerivNotation::Bare) {
            return Err(fail(
                span,
                EvalError::Unsupported("∂ without a variable".into()),
            ));
        }
        let of = d.of.as_ref().ok_or_else(|| {
            fail(
                span,
                EvalError::Invalid("there is nothing to differentiate".into()),
            )
        })?;
        // `\dot x(0)`: the time derivative of the function x at 0, like `x′(0)`.
        if let DerivNotation::Dot(n) = d.notation {
            if let NodeKind::Call { callee, args, .. } = &of.ungrouped().kind {
                if let Some(key) = node_key(callee.ungrouped()) {
                    if let Some(params) = self.function_params(&key) {
                        return self.prime_call(&key, &params, n, args, span);
                    }
                    if let ([arg], true) = (args.as_slice(), self.is_unknown_fn(&key)) {
                        return self.unknown_fn_call(&key, n, arg);
                    }
                }
            }
        }
        let mut vars: Vec<(String, u32)> = Vec::new();
        for v in &d.vars {
            let name = node_key(&v.var).ok_or_else(|| {
                fail(
                    v.var.span,
                    EvalError::Invalid("expected a variable after d".into()),
                )
            })?;
            let power = match &v.power {
                Some(p) => small_order(p)?,
                None => 1,
            };
            vars.push((name, power));
        }
        if let DerivNotation::Dot(n) = d.notation {
            if vars.is_empty() {
                vars.push(("t".into(), u32::from(n)));
            }
        }
        if vars.is_empty() {
            return Err(fail(
                span,
                EvalError::Invalid("this derivative has no variable".into()),
            ));
        }
        if let ([(_, power), ..], Some(order)) = (vars.as_mut_slice(), &d.order) {
            if d.vars.len() == 1 && d.vars.first().is_some_and(|v| v.power.is_none()) {
                *power = small_order(order)?;
            }
        }
        let names: Vec<String> = vars.iter().map(|(n, _)| n.clone()).collect();
        let body = self.bound(&names, |s| s.lower(of))?;
        Ok(Expr::new(ExprNode::Derivative {
            expr: body,
            vars: vars
                .into_iter()
                .map(|(n, k)| (n.as_str().into(), k))
                .collect(),
        }))
    }

    /// An operator's argument or script: a one-word `\text{…}` (`\text{shell}`,
    /// `\text{mid}`) is a method word, read as a name; anything else lowers as usual.
    fn op_argument(&mut self, node: &Node) -> LowerResult {
        if let NodeKind::Text(t) = &node.ungrouped().kind {
            let word = t.trim();
            if !word.is_empty() && word.chars().all(char::is_alphabetic) {
                return Ok(Expr::sym(word));
            }
        }
        self.lower(node)
    }

    fn named_op(&mut self, op: &NamedOp) -> LowerResult {
        // `\operatorname{solve}_{x, y}(…)`: the unknowns are plain variables inside, even
        // when the names are defined (as functions of t, say) elsewhere.
        if matches!(op.name.as_str(), "solve" | "nsolve") {
            if let Some(sub) = &op.sub {
                let names: Vec<String> = match &sub.ungrouped().kind {
                    NodeKind::Tuple { elems, .. } => elems.iter().filter_map(node_key).collect(),
                    _ => node_key(sub.ungrouped()).into_iter().collect(),
                };
                let fresh: Vec<String> = names
                    .into_iter()
                    .filter(|n| !self.is_local(n) && self.function_params(n).is_some())
                    .collect();
                if !fresh.is_empty() {
                    return self.bound(&fresh, |s| s.named_op_plain(op));
                }
            }
        }
        self.named_op_plain(op)
    }

    fn named_op_plain(&mut self, op: &NamedOp) -> LowerResult {
        let analysed = crate::helpers::is_helper(&op.name);
        let mut args = op
            .args
            .iter()
            .enumerate()
            .map(|(i, a)| {
                if analysed && i == 0 {
                    self.lower_helper_target(a)
                } else {
                    self.op_argument(a)
                }
            })
            .collect::<Result<Vec<_>, _>>()?;
        crate::domains_wire::protect_ode_args(&op.name, &mut args);
        if op.sub.is_none() && op.sup.is_none() {
            return Ok(Expr::apply(&op.name, args));
        }
        let empty = || Expr::new(ExprNode::Tuple(Vec::new()));
        let sub = match &op.sub {
            Some(s) => self.op_argument(s)?,
            None => empty(),
        };
        let sup = match &op.sup {
            Some(s) => self.op_argument(s)?,
            None => empty(),
        };
        Ok(Expr::apply(
            "operatorname",
            vec![
                Expr::sym(&op.name),
                sub,
                sup,
                Expr::new(ExprNode::Tuple(args)),
            ],
        ))
    }

    fn relation(&mut self, first: &Node, rest: &[(SynRel, Node)]) -> LowerResult {
        let mut prev = self.lower(first)?;
        let mut parts = Vec::new();
        for (op, node) in rest {
            // `a \equiv b \pmod n`: a congruence, not `b · mod(n)`.
            if *op == SynRel::Equiv {
                if let Some((side, modulus)) = mpp_syntax::math::split_pmod(node) {
                    let next = self.lower(&side)?;
                    let n = self.lower(&modulus)?;
                    parts.push(crate::congruence::relation(prev, next.clone(), n));
                    prev = next;
                    continue;
                }
            }
            let next = self.lower(node)?;
            parts.push(relation(*op, prev, next.clone()));
            prev = next;
        }
        Ok(match parts.len() {
            0 => prev,
            1 => parts.pop().unwrap_or(prev),
            _ => Expr::new(ExprNode::Logic(LogicOp::And, parts)),
        })
    }
}

/// Calls `f` with the base of every name subscripted by exactly `index` in `node`.
fn find_indexed(node: &Node, index: &str, f: &mut dyn FnMut(&str)) {
    if let NodeKind::Name(n) = &node.kind {
        if n.sub.as_deref().and_then(node_key).as_deref() == Some(index) {
            f(&n.base);
        }
    }
    mpp_syntax::math::visit::children(node, &mut |c| find_indexed(c, index, f));
}

fn is_otherwise(node: &Node) -> bool {
    match &node.kind {
        NodeKind::Text(t) => {
            let t = t.trim().to_lowercase();
            t == "otherwise" || t == "else"
        }
        _ => false,
    }
}

/// A derivative order: a small positive integer.
fn small_order(node: &Node) -> Result<u32, Located> {
    let n = match &node.ungrouped().kind {
        NodeKind::Number(t) => t.parse::<u32>().ok(),
        _ => None,
    };
    match n {
        Some(n) if (1..=64).contains(&n) => Ok(n),
        _ => Err(fail(
            node.span,
            EvalError::Unsupported(
                "a derivative order that is not a whole number from 1 to 64".into(),
            ),
        )),
    }
}

/// The matrix operation a binary operator stands for, when its sides are matrices.
fn matrix_op(op: BinOp) -> Option<crate::domains_wire::MatrixOp> {
    use crate::domains_wire::MatrixOp;
    Some(match op {
        BinOp::Mul(mpp_syntax::math::MulStyle::Cdot) => MatrixOp::Dot,
        BinOp::Mul(_) => MatrixOp::Mul,
        BinOp::Times => MatrixOp::Times,
        BinOp::Add => MatrixOp::Add,
        BinOp::Sub => MatrixOp::Sub,
        _ => return None,
    })
}

fn binary(op: BinOp, a: Expr, b: Expr) -> Expr {
    match op {
        BinOp::Add => a + b,
        BinOp::Sub => a - b,
        BinOp::Mul(_) | BinOp::Times => a * b,
        BinOp::PlusMinus => Expr::new(ExprNode::Set(vec![&a + &b, a - b])),
        BinOp::MinusPlus => Expr::new(ExprNode::Set(vec![&a - &b, a + b])),
        BinOp::And => Expr::new(ExprNode::Logic(LogicOp::And, vec![a, b])),
        BinOp::Or => Expr::new(ExprNode::Logic(LogicOp::Or, vec![a, b])),
        BinOp::Compose => Expr::apply("compose", vec![a, b]),
        BinOp::Tensor => Expr::apply("tensor", vec![a, b]),
        BinOp::DirectSum => Expr::apply("directsum", vec![a, b]),
        BinOp::Union => Expr::apply("union", vec![a, b]),
        BinOp::Intersect => Expr::apply("intersect", vec![a, b]),
        BinOp::SetMinus => Expr::apply("setminus", vec![a, b]),
        BinOp::Mod => Expr::apply("mod", vec![a, b]),
    }
}

/// A relation as a core node; operators the core lacks become `Apply` holders.
pub fn relation(op: SynRel, a: Expr, b: Expr) -> Expr {
    let core = match op {
        SynRel::Eq => Some(RelOp::Eq),
        SynRel::Ne => Some(RelOp::Ne),
        SynRel::Lt => Some(RelOp::Lt),
        SynRel::Gt => Some(RelOp::Gt),
        SynRel::Le => Some(RelOp::Le),
        SynRel::Ge => Some(RelOp::Ge),
        SynRel::Approx => Some(RelOp::Approx),
        _ => None,
    };
    if let Some(op) = core {
        return Expr::relation(op, a, b);
    }
    let name = match op {
        SynRel::In => "in",
        SynRel::NotIn => "notin",
        SynRel::Ni => "ni",
        SynRel::Subset => "subset",
        SynRel::SubsetEq => "subseteq",
        SynRel::Supset => "supset",
        SynRel::SupsetEq => "supseteq",
        SynRel::Equiv => "equiv",
        SynRel::Sim => "sim",
        SynRel::Simeq => "simeq",
        SynRel::Cong => "cong",
        SynRel::Propto => "propto",
        SynRel::Ll => "ll",
        SynRel::Gg => "gg",
        SynRel::Implies => "implies",
        SynRel::Iff => "iff",
        SynRel::To => "to",
        SynRel::MapsTo => "mapsto",
        other => other.symbol(),
    };
    Expr::apply(name, vec![a, b])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::value::{Origin, VarKind};
    use mpp_syntax::document::BlockId;
    use mpp_syntax::math::parse_expr;
    use std::collections::HashMap;

    #[derive(Default)]
    struct Scope {
        names: HashMap<String, Arc<Binding>>,
        sequences: Vec<String>,
    }

    impl LowerScope for Scope {
        fn binding(&self, name: &str) -> Option<Arc<Binding>> {
            self.names.get(name).cloned()
        }
        fn is_sequence(&self, base: &str) -> bool {
            self.sequences.iter().any(|s| s == base)
        }
    }

    fn origin() -> Origin {
        Origin::local(BlockId(0), Span::point(0))
    }

    fn with_function(name: &str, params: &[&str]) -> Scope {
        let mut s = Scope::default();
        s.names.insert(
            name.into(),
            Arc::new(Binding::Function {
                params: params.iter().map(|p| p.to_string()).collect(),
                body: Expr::sym(params[0]),
                origin: origin(),
            }),
        );
        s
    }

    fn lower(scope: &Scope, src: &str) -> LowerResult {
        let (node, diags) = parse_expr(src, 0);
        assert!(diags.is_empty(), "{diags:?}");
        Lowerer::new(scope).lower(&node)
    }

    #[test]
    fn empty_template_boxes_ask_to_be_filled_in() {
        let none = Scope::default();
        let boxed = Node::placeholder("upper", Span::new(0, 1));
        let err = Lowerer::new(&none).lower(&boxed).unwrap_err();
        assert_eq!(err.error, EvalError::EmptyBox("upper bound".into()));
        assert_eq!(err.error.to_string(), "fill in: upper bound");
        assert!(err.diagnostic().is_none());
    }

    #[test]
    fn call_against_product() {
        let none = Scope::default();
        let x = Expr::sym("x");
        let f = Expr::sym("f");
        assert_eq!(lower(&none, "f(x)").unwrap(), f * x.clone());
        let scope = with_function("f", &["t"]);
        assert_eq!(
            lower(&scope, "f(x)").unwrap(),
            Expr::apply("f", vec![x.clone()])
        );
        let err = lower(&scope, "f(x, 1)").unwrap_err();
        assert!(matches!(
            err.error,
            EvalError::Arity {
                expected: 1,
                got: 2,
                ..
            }
        ));
        let err = lower(&none, "g(x, 1)").unwrap_err();
        assert_eq!(err.error, EvalError::NotAFunction("g".into()));
    }

    #[test]
    fn power_after_a_product_call() {
        let none = Scope::default();
        let a = Expr::sym("a_n");
        let x = Expr::sym("x");
        let c = Expr::sym("c");
        assert_eq!(lower(&none, "a_n(x-c)^2").unwrap(), a * (x - c).pow(2));
    }

    #[test]
    fn e_and_i_are_constants_unless_bound() {
        let none = Scope::default();
        assert_eq!(lower(&none, "e").unwrap(), Expr::e());
        assert_eq!(lower(&none, "i").unwrap(), Expr::i());
        let mut bound = Scope::default();
        bound.names.insert(
            "e".into(),
            Arc::new(Binding::Variable {
                kind: VarKind::Variable,
                value: Expr::int(2),
                origin: origin(),
            }),
        );
        assert_eq!(lower(&bound, "e").unwrap(), Expr::sym("e"));
        let sum = lower(&none, "\\sum_{i=1}^{3} i").unwrap();
        match sum.node() {
            ExprNode::BigOp { expr, .. } => assert_eq!(expr, &Expr::sym("i")),
            other => panic!("expected a sum, got {other:?}"),
        }
    }

    #[test]
    fn sequences_and_holders() {
        let scope = Scope {
            sequences: vec!["a".into()],
            ..Scope::default()
        };
        assert_eq!(
            lower(&scope, "a_5").unwrap(),
            Expr::apply("a", vec![Expr::int(5)])
        );
        assert!(matches!(
            lower(&scope, "\\int_0^1 x^2 \\, dx").unwrap().node(),
            ExprNode::Integral {
                bounds: Some(_),
                ..
            }
        ));
        assert!(matches!(
            lower(&scope, "\\lim_{x \\to 0} x").unwrap().node(),
            ExprNode::Limit { .. }
        ));
        assert!(matches!(
            lower(&scope, "\\frac{d}{dx} x^2").unwrap().node(),
            ExprNode::Derivative { .. }
        ));
        assert_eq!(
            lower(&scope, "\\nabla f").unwrap(),
            Expr::apply("grad", vec![Expr::sym("f")])
        );
    }

    #[test]
    fn functions_and_structures() {
        let none = Scope::default();
        assert_eq!(lower(&none, "\\log_2 8").unwrap(), Expr::int(3));
        assert!(matches!(
            lower(&none, "\\begin{pmatrix} 1 & 2 \\\\ 3 & 4 \\end{pmatrix}")
                .unwrap()
                .node(),
            ExprNode::Matrix {
                rows: 2,
                cols: 2,
                ..
            }
        ));
        assert!(matches!(
            lower(
                &none,
                "\\begin{cases} x & x > 0 \\\\ -x & \\text{otherwise} \\end{cases}"
            )
            .unwrap()
            .node(),
            ExprNode::Piecewise {
                otherwise: Some(_),
                ..
            }
        ));
        assert!(matches!(
            lower(&none, "[0, 1)").unwrap().node(),
            ExprNode::Interval {
                lo_closed: true,
                hi_closed: false,
                ..
            }
        ));
        assert!(lower(&none, "\\sin^{-1} x").is_ok());
    }
}
