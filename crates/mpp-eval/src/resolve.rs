//! Resolution: putting the current values of names into a lowered expression.
//!
//! `:=` variables are stored as expressions, so a name resolves to its expression
//! with *its* names resolved in turn (that is how `E := ½ m v²` follows later
//! changes of `m`). User functions are expanded by substituting the arguments
//! into the body; sequences are computed term by term. Piecewise expressions pick
//! their branch as soon as the conditions are decided, which lets recursive
//! functions with a base case terminate. Holders are handed to
//! [`crate::holders::evaluate_holder`] once their parts are resolved.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::sync::Arc;

use mpp_core::calc::Recurrence;
use mpp_core::simplify::canonical;
use mpp_core::{EmptyEnv, Expr, ExprNode, LogicOp, Name, RelOp};

use crate::calculus;
use crate::error::EvalError;
use crate::holders::{evaluate_holder, Ctx};
use crate::value::{Binding, Sequence, VarKind};

/// How the resolver finds names.
pub trait Lookup {
    /// The binding of `name` at this point of the document.
    fn lookup(&self, name: &str) -> Option<Arc<Binding>>;
}

/// Deepest nesting of user-function calls.
const MAX_CALL_DEPTH: usize = 1000;
/// Most nodes visited for one statement.
const MAX_WORK: usize = 400_000;
/// Most terms of a sequence computed for one statement.
const MAX_SEQUENCE_TERMS: i64 = 10_000;
/// Largest intermediate result kept from a call or sequence term.
const MAX_SIZE: usize = 50_000;

type Result<T> = std::result::Result<T, EvalError>;

/// Resolves expressions for one statement.
pub struct Resolver<'a> {
    scope: &'a dyn Lookup,
    ctx: &'a Ctx<'a>,
    /// Where-clause values; they shadow the environment.
    locals: BTreeMap<String, Expr>,
    /// Names bound inside the expression (indices, integration variables, parameters).
    masked: Vec<Name>,
    /// Variables being expanded, to catch circular definitions.
    expanding: Vec<String>,
    /// Functions being expanded.
    calls: Vec<String>,
    memo: HashMap<(String, Vec<Expr>), Expr>,
    terms: HashMap<String, BTreeMap<i64, Expr>>,
    /// Recurrences compiled by the core, by sequence name (`None` when the rule is not
    /// one it takes, so terms are computed here instead).
    recurrences: HashMap<String, Option<Recurrence>>,
    computing: Vec<String>,
    touched: BTreeSet<String>,
    work: usize,
}

impl<'a> Resolver<'a> {
    /// A resolver over `scope` with where-clause values `locals`.
    pub fn new(scope: &'a dyn Lookup, ctx: &'a Ctx<'a>, locals: BTreeMap<String, Expr>) -> Self {
        Resolver {
            scope,
            ctx,
            locals,
            masked: Vec::new(),
            expanding: Vec::new(),
            calls: Vec::new(),
            memo: HashMap::new(),
            terms: HashMap::new(),
            recurrences: HashMap::new(),
            computing: Vec::new(),
            touched: BTreeSet::new(),
            work: 0,
        }
    }

    /// Keeps `names` symbolic (for example a figure's plotting variables).
    pub fn mask(&mut self, names: impl IntoIterator<Item = String>) {
        self.masked
            .extend(names.into_iter().map(|n| Name::from(n.as_str())));
    }

    /// Every name whose binding was used, directly or through other definitions.
    pub fn touched(&self) -> &BTreeSet<String> {
        &self.touched
    }

    /// Resolves and simplifies `e`.
    pub fn run(&mut self, e: &Expr) -> Result<Expr> {
        Ok(self.resolve(e)?.simplify())
    }

    fn is_masked(&self, name: &str) -> bool {
        self.masked.iter().any(|m| &**m == name)
    }

    fn with_mask<T>(&mut self, names: &[Name], f: impl FnOnce(&mut Self) -> T) -> T {
        let len = self.masked.len();
        self.masked.extend(names.iter().cloned());
        let out = f(self);
        self.masked.truncate(len);
        out
    }

    /// Runs `f` with no masked names (for the body of a stored definition, whose
    /// names refer to the environment, not to the expression being resolved).
    fn unmasked<T>(&mut self, f: impl FnOnce(&mut Self) -> T) -> T {
        let saved = std::mem::take(&mut self.masked);
        let out = f(self);
        self.masked = saved;
        out
    }

    fn tick(&mut self) -> Result<()> {
        self.work += 1;
        if self.work > MAX_WORK {
            Err(EvalError::TooMuchWork)
        } else {
            Ok(())
        }
    }

    fn holder(&self, e: Expr) -> Result<Expr> {
        evaluate_holder(&e, self.ctx).map_err(EvalError::Math)
    }

    fn resolve(&mut self, e: &Expr) -> Result<Expr> {
        self.tick()?;
        match e.node() {
            ExprNode::Num(_) | ExprNode::Const(_) | ExprNode::Bool(_) => Ok(e.clone()),
            ExprNode::Sym(s) => self.symbol(e, s),
            ExprNode::Apply(name, args) => self.apply(name, args),
            ExprNode::Piecewise { cases, otherwise } => self.piecewise(cases, otherwise.as_ref()),
            ExprNode::Derivative { expr, vars } => {
                let names: Vec<Name> = vars.iter().map(|(n, _)| n.clone()).collect();
                let body = self.with_mask(&names, |r| r.resolve(expr))?;
                self.holder(Expr::new(ExprNode::Derivative {
                    expr: body,
                    vars: vars.clone(),
                }))
            }
            ExprNode::Integral { .. } => {
                let parts = self.integral_parts(e)?;
                self.holder(parts)
            }
            ExprNode::BigOp {
                kind,
                expr,
                var,
                lower,
                upper,
            } => {
                let lower = self.resolve(lower)?.simplify();
                let upper = self.resolve(upper)?.simplify();
                let body = self.with_mask(std::slice::from_ref(var), |r| r.resolve(expr))?;
                self.holder(Expr::new(ExprNode::BigOp {
                    kind: *kind,
                    expr: body,
                    var: var.clone(),
                    lower,
                    upper,
                }))
            }
            ExprNode::Limit {
                expr,
                var,
                point,
                dir,
            } => {
                let point = self.resolve(point)?;
                let body = self.with_mask(std::slice::from_ref(var), |r| r.resolve(expr))?;
                self.holder(Expr::new(ExprNode::Limit {
                    expr: body,
                    var: var.clone(),
                    point,
                    dir: *dir,
                }))
            }
            _ => {
                let mut failure = None;
                let node = e.map_children(&mut |c| match self.resolve(c) {
                    Ok(v) => v,
                    Err(err) => {
                        failure.get_or_insert(err);
                        c.clone()
                    }
                });
                match failure {
                    Some(err) => Err(err),
                    None => Ok(canonical(node)),
                }
            }
        }
    }

    /// Resolves the parts of an integral without evaluating it. A nested integral
    /// (`∫_0^1 ∫_0^x f dy dx`) is resolved as a whole, so the calculus evaluator sees
    /// every level at once and can integrate it as an iterated integral.
    fn integral_parts(&mut self, e: &Expr) -> Result<Expr> {
        let ExprNode::Integral { expr, var, bounds } = e.node() else {
            return self.resolve(e);
        };
        self.tick()?;
        let bounds = match bounds {
            Some((a, b)) => Some((self.resolve(a)?, self.resolve(b)?)),
            None => None,
        };
        let body = self.with_mask(std::slice::from_ref(var), |r| {
            if matches!(expr.node(), ExprNode::Integral { .. }) {
                r.integral_parts(expr)
            } else {
                r.resolve(expr)
            }
        })?;
        Ok(Expr::new(ExprNode::Integral {
            expr: body,
            var: var.clone(),
            bounds,
        }))
    }

    fn symbol(&mut self, e: &Expr, name: &str) -> Result<Expr> {
        if self.is_masked(name) {
            return Ok(e.clone());
        }
        if let Some(v) = self.locals.get(name) {
            return Ok(v.clone());
        }
        let Some(binding) = self.scope.lookup(name) else {
            return Ok(e.clone());
        };
        self.touched.insert(name.to_string());
        match &*binding {
            Binding::Variable {
                kind: VarKind::Constant,
                value,
                ..
            } => Ok(value.clone()),
            Binding::Variable { value, .. } => {
                if let Some(pos) = self.expanding.iter().position(|n| n == name) {
                    let mut chain = self.expanding[pos..].to_vec();
                    chain.push(name.to_string());
                    return Err(EvalError::Circular(chain));
                }
                self.expanding.push(name.to_string());
                let out = self.unmasked(|r| r.resolve(value));
                self.expanding.pop();
                out
            }
            Binding::Function { .. } | Binding::Sequence { .. } | Binding::Imported { .. } => {
                Ok(e.clone())
            }
        }
    }

    fn apply(&mut self, name: &Name, args: &[Expr]) -> Result<Expr> {
        let bound = calculus::bound_names(name, args);
        let args = self.with_mask(&bound, |r| {
            args.iter()
                .map(|a| r.resolve(a))
                .collect::<Result<Vec<_>>>()
        })?;
        let raw = || Expr::new(ExprNode::Apply(name.clone(), args.clone()));
        if self.is_masked(name) || self.locals.contains_key(&**name) {
            return Ok(raw());
        }
        let binding = self.scope.lookup(name);
        if binding.is_some() {
            self.touched.insert(name.to_string());
        }
        match binding.as_deref() {
            Some(Binding::Function { params, body, .. }) => {
                self.call(name, params, body, args.clone())
            }
            Some(Binding::Sequence { seq, .. }) => {
                let [index] = args.as_slice() else {
                    return Err(EvalError::Arity {
                        name: name.to_string(),
                        expected: 1,
                        got: args.len(),
                    });
                };
                match index.simplify().as_number().and_then(|n| n.as_i64()) {
                    Some(k) => self.term(name, seq, k),
                    None => Ok(raw()),
                }
            }
            _ => self.holder(raw()),
        }
    }

    fn call(
        &mut self,
        name: &str,
        params: &[String],
        body: &Expr,
        args: Vec<Expr>,
    ) -> Result<Expr> {
        if params.len() != args.len() {
            return Err(EvalError::Arity {
                name: name.to_string(),
                expected: params.len(),
                got: args.len(),
            });
        }
        let constant = args.iter().all(Expr::is_constant);
        if self.calls.iter().any(|c| c == name) && !constant {
            // A recursive call with symbolic arguments stays as it is.
            return Ok(Expr::apply(name, args));
        }
        let key = (name.to_string(), args.clone());
        if constant {
            if let Some(v) = self.memo.get(&key) {
                return Ok(v.clone());
            }
        }
        if self.calls.len() >= MAX_CALL_DEPTH {
            return Err(EvalError::TooDeep(name.to_string()));
        }
        // Names in the body that the call site binds (a sum index, say) refer to
        // the environment: resolve them before the arguments go in.
        let clashing: Vec<Name> = body
            .free_symbols()
            .into_iter()
            .filter(|s| !params.iter().any(|p| p == &**s) && self.is_masked(s))
            .collect();
        let mut captured: Vec<(String, Expr)> = Vec::with_capacity(clashing.len());
        for s in clashing {
            let value = self.unmasked(|r| r.resolve(&Expr::sym(&s)))?;
            captured.push((s.to_string(), value));
        }
        let mut body = body.clone();
        if !captured.is_empty() {
            let pairs: Vec<(&str, Expr)> = captured
                .iter()
                .map(|(n, v)| (n.as_str(), v.clone()))
                .collect();
            body = body.subs_all(&pairs);
        }
        let pairs: Vec<(&str, Expr)> = params.iter().map(String::as_str).zip(args).collect();
        let body = body.subs_all(&pairs);
        self.calls.push(name.to_string());
        let out = self.resolve(&body);
        self.calls.pop();
        let out = out?;
        if out.size() > MAX_SIZE {
            return Err(EvalError::Math(mpp_core::MathError::TooLarge(format!(
                "the result of {name}"
            ))));
        }
        if constant {
            self.memo.insert(key, out.clone());
        }
        Ok(out)
    }

    /// The term `name_k` of a sequence, computed upwards from its first initial term.
    fn term(&mut self, name: &str, seq: &Sequence, k: i64) -> Result<Expr> {
        if let Some(v) = self.terms.get(name).and_then(|m| m.get(&k)) {
            return Ok(v.clone());
        }
        if let Some(init) = seq.initials.get(&k) {
            let v = self.unmasked(|r| r.resolve(init))?.simplify();
            self.terms
                .entry(name.to_string())
                .or_default()
                .insert(k, v.clone());
            return Ok(v);
        }
        if let Some(v) = self.core_term(name, seq, k)? {
            self.terms
                .entry(name.to_string())
                .or_default()
                .insert(k, v.clone());
            return Ok(v);
        }
        let Some(body) = &seq.body else {
            return Err(EvalError::Invalid(format!("{name}_{k} is not defined")));
        };
        let Some(&first) = seq.initials.keys().next() else {
            return Err(EvalError::Invalid(format!(
                "the sequence {name} has no initial term such as {name}_1 := …"
            )));
        };
        if k < first {
            return Err(EvalError::Invalid(format!(
                "{name}_{k} comes before the first term {name}_{first}"
            )));
        }
        if k - first > MAX_SEQUENCE_TERMS {
            return Err(EvalError::TooDeep(name.to_string()));
        }
        if self.computing.iter().any(|c| c == name) {
            return Err(EvalError::Invalid(format!(
                "{name}_{k} is needed before it is known"
            )));
        }
        self.computing.push(name.to_string());
        let out = self.terms_up_to(name, seq, body, first, k);
        self.computing.pop();
        out
    }

    /// The term `name_k` from the core's [`Recurrence`], when the rule compiles to one.
    fn core_term(&mut self, name: &str, seq: &Sequence, k: i64) -> Result<Option<Expr>> {
        if !self.recurrences.contains_key(name) {
            let compiled = self.compile_recurrence(name, seq);
            self.recurrences.insert(name.to_string(), compiled);
        }
        let Some(Some(rec)) = self.recurrences.get(name) else {
            return Ok(None);
        };
        if k < rec.start() {
            return Ok(None);
        }
        if k - rec.start() > MAX_SEQUENCE_TERMS {
            return Err(EvalError::TooDeep(name.to_string()));
        }
        let value = rec.nth(k).map_err(EvalError::Math)?;
        if self.ctx.logging() {
            let note = format!(
                "{name}_{{{}+{}}} = {} from {name}_{} = {}",
                seq.index,
                seq.shift,
                rec.rule(),
                rec.start(),
                rec.initial()
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
                    .join(", ")
            );
            self.ctx.log(
                "compute the terms of the recurrence",
                &Expr::apply(name, vec![Expr::int(k)]),
                &value,
                Some(note),
            );
        }
        Ok(Some(value))
    }

    /// The recurrence of `name` for the core, with its rule resolved against the
    /// environment (the index and the sequence itself stay symbolic).
    fn compile_recurrence(&mut self, name: &str, seq: &Sequence) -> Option<Recurrence> {
        let body = seq.body.as_ref()?;
        let (&first, _) = seq.initials.iter().next()?;
        let mut initial = Vec::with_capacity(seq.initials.len());
        for (offset, (&j, init)) in seq.initials.iter().enumerate() {
            if j != first.checked_add(i64::try_from(offset).ok()?)? {
                return None;
            }
            initial.push(self.unmasked(|r| r.resolve(init)).ok()?.simplify());
        }
        let keep = [Name::from(seq.index.as_str()), Name::from(name)];
        let rule = self
            .unmasked(|r| r.with_mask(&keep, |r| r.resolve(body)))
            .ok()?;
        Recurrence::with_shift(name, &seq.index, first, initial, &rule, seq.shift).ok()
    }

    fn terms_up_to(
        &mut self,
        name: &str,
        seq: &Sequence,
        body: &Expr,
        first: i64,
        k: i64,
    ) -> Result<Expr> {
        let mut last = Expr::undefined();
        for j in first..=k {
            if let Some(v) = self.terms.get(name).and_then(|m| m.get(&j)) {
                last = v.clone();
                continue;
            }
            let v = match seq.initials.get(&j) {
                Some(init) => self.unmasked(|r| r.resolve(init))?,
                None => {
                    let n = Expr::int(j.saturating_sub(seq.shift));
                    let step = body.subs(&seq.index, &n);
                    self.unmasked(|r| r.resolve(&step))?
                }
            }
            .simplify();
            if v.size() > MAX_SIZE {
                return Err(EvalError::Math(mpp_core::MathError::TooLarge(format!(
                    "{name}_{j}"
                ))));
            }
            self.terms
                .entry(name.to_string())
                .or_default()
                .insert(j, v.clone());
            last = v;
        }
        Ok(last)
    }

    fn piecewise(&mut self, cases: &[(Expr, Expr)], otherwise: Option<&Expr>) -> Result<Expr> {
        let mut kept = Vec::new();
        let mut fallback = None;
        for (value, cond) in cases {
            let cond = self.resolve(cond)?.simplify();
            match decide(&cond) {
                Some(true) if kept.is_empty() => return self.resolve(value),
                Some(true) => {
                    fallback = Some(self.resolve(value)?);
                    break;
                }
                Some(false) => {}
                None => kept.push((self.resolve(value)?, cond)),
            }
        }
        if fallback.is_none() {
            if let Some(o) = otherwise {
                if kept.is_empty() {
                    return self.resolve(o);
                }
                fallback = Some(self.resolve(o)?);
            }
        }
        if kept.is_empty() {
            return Err(EvalError::Invalid(
                "no case of the piecewise definition applies here".into(),
            ));
        }
        Ok(Expr::new(ExprNode::Piecewise {
            cases: kept,
            otherwise: fallback,
        }))
    }
}

/// Decides a condition whose sides are known numbers.
pub fn decide(cond: &Expr) -> Option<bool> {
    match cond.node() {
        ExprNode::Bool(b) => Some(*b),
        ExprNode::Relation(op, a, b) => {
            let diff = (a - b).simplify();
            if let Some(n) = diff.as_number().filter(|n| n.is_exact() && n.is_real()) {
                let sign = n.sign()?;
                use std::cmp::Ordering::{Equal, Greater, Less};
                return Some(match op {
                    RelOp::Eq | RelOp::Approx => sign == Equal,
                    RelOp::Ne => sign != Equal,
                    RelOp::Lt => sign == Less,
                    RelOp::Le => sign != Greater,
                    RelOp::Gt => sign == Greater,
                    RelOp::Ge => sign != Less,
                });
            }
            if !(a.is_constant() && b.is_constant()) {
                return None;
            }
            let x = a.eval_f64(&EmptyEnv).ok()?;
            let y = b.eval_f64(&EmptyEnv).ok()?;
            let close = (x - y).abs() <= 1e-12 * x.abs().max(y.abs()).max(1.0);
            Some(match op {
                RelOp::Eq | RelOp::Approx => close,
                RelOp::Ne => !close,
                RelOp::Lt => x < y && !close,
                RelOp::Le => x < y || close,
                RelOp::Gt => x > y && !close,
                RelOp::Ge => x > y || close,
            })
        }
        ExprNode::Logic(op, args) => {
            let vals: Vec<Option<bool>> = args.iter().map(decide).collect();
            match op {
                LogicOp::Not => vals.first().copied().flatten().map(|b| !b),
                LogicOp::And => {
                    if vals.contains(&Some(false)) {
                        Some(false)
                    } else if vals.iter().all(Option::is_some) {
                        Some(true)
                    } else {
                        None
                    }
                }
                LogicOp::Or => {
                    if vals.contains(&Some(true)) {
                        Some(true)
                    } else if vals.iter().all(Option::is_some) {
                        Some(false)
                    } else {
                        None
                    }
                }
            }
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::holders::Holders;
    use crate::value::Origin;
    use mpp_syntax::document::BlockId;
    use mpp_syntax::Span;

    #[derive(Default)]
    struct Map(HashMap<String, Arc<Binding>>);

    impl Lookup for Map {
        fn lookup(&self, name: &str) -> Option<Arc<Binding>> {
            self.0.get(name).cloned()
        }
    }

    fn origin() -> Origin {
        Origin::local(BlockId(0), Span::point(0))
    }

    impl Map {
        fn var(mut self, name: &str, value: Expr) -> Self {
            self.0.insert(
                name.into(),
                Arc::new(Binding::Variable {
                    kind: VarKind::Variable,
                    value,
                    origin: origin(),
                }),
            );
            self
        }
        fn func(mut self, name: &str, params: &[&str], body: Expr) -> Self {
            self.0.insert(
                name.into(),
                Arc::new(Binding::Function {
                    params: params.iter().map(|p| p.to_string()).collect(),
                    body,
                    origin: origin(),
                }),
            );
            self
        }
    }

    fn run(scope: &Map, e: &Expr) -> Result<Expr> {
        let holders = Holders::new();
        let ctx = Ctx::new(&holders);
        Resolver::new(scope, &ctx, BTreeMap::new()).run(e)
    }

    #[test]
    fn formulas_follow_their_names() {
        let m = Expr::sym("m");
        let scope = Map::default()
            .var("m", Expr::int(3))
            .var("E", Expr::int(2) * m.clone());
        assert_eq!(run(&scope, &Expr::sym("E")).unwrap(), Expr::int(6));
        let cyc = Map::default()
            .var("a", Expr::sym("b"))
            .var("b", Expr::sym("a"));
        assert!(matches!(
            run(&cyc, &Expr::sym("a")),
            Err(EvalError::Circular(_))
        ));
    }

    #[test]
    fn recursive_functions_with_a_base_case() {
        let n = Expr::sym("n");
        let body = Expr::new(ExprNode::Piecewise {
            cases: vec![(
                Expr::one(),
                Expr::relation(RelOp::Le, n.clone(), Expr::one()),
            )],
            otherwise: Some(n.clone() * Expr::apply("f", vec![n.clone() - Expr::one()])),
        });
        let scope = Map::default().func("f", &["n"], body);
        assert_eq!(
            run(&scope, &Expr::apply("f", vec![Expr::int(5)])).unwrap(),
            Expr::int(120)
        );
        // Symbolic arguments do not loop.
        assert!(run(&scope, &Expr::apply("f", vec![Expr::sym("k")])).is_ok());
    }

    #[test]
    fn sum_index_does_not_capture() {
        let scope = Map::default().var("n", Expr::int(10)).func(
            "f",
            &["x"],
            Expr::sym("x") + Expr::sym("n"),
        );
        let sum = Expr::new(ExprNode::BigOp {
            kind: mpp_core::BigOpKind::Sum,
            expr: Expr::apply("f", vec![Expr::sym("n")]),
            var: "n".into(),
            lower: Expr::one(),
            upper: Expr::int(3),
        });
        // (1 + 10) + (2 + 10) + (3 + 10)
        assert_eq!(run(&scope, &sum).unwrap(), Expr::int(36));
    }

    #[test]
    fn decisions() {
        let rel = |op, a: i64, b: i64| Expr::relation(op, Expr::int(a), Expr::int(b));
        assert_eq!(decide(&rel(RelOp::Lt, 1, 2)), Some(true));
        assert_eq!(decide(&rel(RelOp::Ge, 1, 2)), Some(false));
        assert_eq!(
            decide(&Expr::relation(RelOp::Lt, Expr::sym("x"), Expr::one())),
            None
        );
        assert_eq!(
            decide(&Expr::relation(RelOp::Gt, Expr::pi(), Expr::int(3))),
            Some(true)
        );
    }
}
