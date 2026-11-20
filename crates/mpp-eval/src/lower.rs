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
    BigOp, BigOpKind as SynBigOp, BinOp, CaseCondition, CaseRow, Constant, DerivNotation,
    Derivative, Fence, FuncApp, IntKind, Integral, Limit, MatrixEnv, NablaOp, Name, NamedOp, Node,
    NodeKind, RelOp as SynRel, Side, UnaryOp,
};
use mpp_syntax::Span;

use crate::error::{EvalError, Located};
use crate::names::{base_key, name_key, node_key};
use crate::value::Binding;

/// What lowering needs to know about the names in scope.
pub trait LowerScope {
    /// The binding of `name` at this point of the document.
    fn binding(&self, name: &str) -> Option<Arc<Binding>>;
    /// True when the document defines a recurrence for the sequence `base`.
    fn is_sequence(&self, base: &str) -> bool;
}

/// Result of lowering one node.
pub type LowerResult = Result<Expr, Located>;

/// Names the core treats as built-in functions besides [`Func`]'s own.
const EXTRA_BUILTINS: &[&str] = &[
    "exp", "sqrt", "cbrt", "root", "lg", "asec", "arcsec", "acsc", "arccsc", "acot", "arccot",
    "sech", "csch", "coth",
];

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
        }
    }

    /// Adds names bound by the whole statement.
    pub fn with_locals(mut self, names: impl IntoIterator<Item = String>) -> Self {
        self.locals.extend(names);
        self
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
            K::Text(t) => Err(fail(
                span,
                EvalError::Invalid(format!("the text “{t}” cannot be evaluated")),
            )),
            K::Continuation => Err(fail(
                span,
                EvalError::Unsupported("a line that continues the one before".into()),
            )),
            K::Error => Err(fail(span, EvalError::Silent)),
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
                let a = self.lower(lhs)?;
                let b = self.lower(rhs)?;
                Ok(binary(*op, a, b))
            }
            K::Frac { num, den, .. } => {
                let n = self.lower(num)?;
                let d = self.lower(den)?;
                Ok(n / d)
            }
            K::Pow { base, exp } => self.power(base, exp),
            K::Subscript { .. } => Err(fail(
                span,
                EvalError::Unsupported("a subscript on this expression".into()),
            )),
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
                Ok(Expr::apply(name, vec![self.lower(body)?]))
            }
            K::NamedOp(op) => self.named_op(op),
            K::Accented { .. } => Err(fail(
                span,
                EvalError::Unsupported("an accent on this expression".into()),
            )),
            K::Relation { first, rest } => self.relation(first, rest),
        }
    }

    fn name(&mut self, n: &Name, span: Span) -> LowerResult {
        if let Some(sub) = &n.sub {
            let base = base_key(n);
            if self.is_sequence(&base) {
                let index = self.lower(sub)?;
                return Ok(Expr::apply(&base, vec![index]));
            }
        }
        let key = name_key(n).ok_or_else(|| {
            fail(
                span,
                EvalError::Unsupported("a subscript this complicated on a name".into()),
            )
        })?;
        if let Some(params) = self.function_params(&key) {
            let args = params.iter().map(|p| Expr::sym(p)).collect();
            return Ok(Expr::apply(&key, args));
        }
        Ok(Expr::sym(&key))
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
        let inner = callee.ungrouped();
        if let NodeKind::Prime { base, count } = &inner.kind {
            if let Some(key) = node_key(base) {
                if let Some(params) = self.function_params(&key) {
                    return self.prime_call(&key, &params, *count, args, span);
                }
            }
        }
        if let Some(key) = node_key(inner) {
            if let Some(params) = self.function_params(&key) {
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
                return Ok(Expr::apply(&key, self.lower_all(args)?));
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
        Ok(Expr::new(ExprNode::Derivative {
            expr: body,
            vars: vec![("x".into(), u32::from(count))],
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
        Ok(b.pow(e))
    }

    fn func_app(&mut self, app: &FuncApp, span: Span) -> LowerResult {
        let args = self.lower_all(&app.args)?;
        let name = app.func;
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
        let var = self.bound_name(op.index.as_ref(), span, "sum")?;
        let (Some(from), Some(to)) = (&op.from, &op.to) else {
            return Err(fail(
                span,
                EvalError::Invalid("this sum needs a start value and an end value".into()),
            ));
        };
        let lower = self.lower(from)?;
        let upper = self.lower(to)?;
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
        if matches!(i.kind, IntKind::Contour | IntKind::Surface) {
            return Err(fail(
                span,
                EvalError::Unsupported("line and surface integrals".into()),
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
        if bounds.is_some() && vars.len() > 1 {
            return Err(fail(
                span,
                EvalError::Unsupported("bounds on a multiple integral".into()),
            ));
        }
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

    fn derivative(&mut self, d: &Derivative, span: Span) -> LowerResult {
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

    fn named_op(&mut self, op: &NamedOp) -> LowerResult {
        let args = self.lower_all(&op.args)?;
        if op.sub.is_none() && op.sup.is_none() {
            return Ok(Expr::apply(&op.name, args));
        }
        let empty = || Expr::new(ExprNode::Tuple(Vec::new()));
        let sub = match &op.sub {
            Some(s) => self.lower(s)?,
            None => empty(),
        };
        let sup = match &op.sup {
            Some(s) => self.lower(s)?,
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
