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
use mpp_core::domains::probability::random::Dist;
use mpp_core::simplify::canonical;
use mpp_core::{BigOpKind, EmptyEnv, Expr, ExprNode, LogicOp, Name, RelOp};

use crate::calculus;
use crate::error::EvalError;
use crate::holders::{evaluate_holder, Ctx};
use crate::random::{FAMILY_KEY, RV_KEY};
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
const MAX_SEQUENCE_TERMS: i64 = mpp_core::calc::recurrence::MAX_TERMS;
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
        // A long list of plain numbers or quantities (a data column) has nothing to
        // resolve; walking it entry by entry used to exhaust the work limit.
        if is_plain_data(e) {
            return Ok(e.clone());
        }
        match e.node() {
            ExprNode::Num(_) | ExprNode::Const(_) | ExprNode::Bool(_) => Ok(e.clone()),
            ExprNode::Sym(s) => self.symbol(e, s),
            ExprNode::Apply(name, args) => self.apply(name, args),
            ExprNode::Piecewise { cases, otherwise } => self.piecewise(cases, otherwise.as_ref()),
            ExprNode::Derivative { expr, vars } => {
                let names: Vec<Name> = vars.iter().map(|(n, _)| n.clone()).collect();
                let d = match self.leibniz(e, expr, vars)? {
                    Some(d) => d,
                    None => {
                        let body = self.with_mask(&names, |r| r.resolve(expr))?;
                        self.holder(Expr::new(ExprNode::Derivative {
                            expr: body,
                            vars: vars.clone(),
                        }))?
                    }
                };
                // `d/dx x^3 = ?, x = 2`: a where-clause value of the variable goes in
                // after differentiating (unless an outer binder owns the name).
                let pairs: Vec<(&str, Expr)> = names
                    .iter()
                    .filter(|n| !self.is_masked(n))
                    .filter_map(|n| self.locals.get(&**n).map(|v| (&**n, v.clone())))
                    .collect();
                if pairs.is_empty() || has_derivative(&d) {
                    Ok(d)
                } else {
                    Ok(d.subs_all(&pairs).simplify())
                }
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
                if let Some(v) = self.random_sum(*kind, expr, var, lower, upper)? {
                    return Ok(v);
                }
                let lower = self.resolve(lower)?.simplify();
                let upper = self.resolve(upper)?.simplify();
                let body = self.with_mask(std::slice::from_ref(var), |r| r.resolve(expr))?;
                if let Some(v) = self.data_big_op(*kind, &body, var, &lower, &upper)? {
                    return Ok(v);
                }
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
            ExprNode::Add(_) if self.limit_with_free_terms(e).is_some() => {
                let joined = self.limit_with_free_terms(e).unwrap_or_else(|| e.clone());
                self.ctx.note(
                    "the terms after the limit use its variable, so they are taken inside the limit",
                );
                self.resolve(&joined)
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

    /// `lim_{x→∞} √(x²+x) − x` reads as `(lim √(x²+x)) − x`, leaving `x` free outside
    /// the limit: a sum with a limit over `x` and other terms in a free `x` (one with no
    /// value) means the limit of the whole sum.
    fn limit_with_free_terms(&self, e: &Expr) -> Option<Expr> {
        let ExprNode::Add(ts) = e.node() else {
            return None;
        };
        let (i, var) = ts.iter().enumerate().find_map(|(i, t)| match t.node() {
            ExprNode::Limit { var, .. } => Some((i, var.clone())),
            _ => None,
        })?;
        if self.is_masked(&var)
            || self.locals.contains_key(&*var)
            || self.scope.lookup(&var).is_some()
        {
            return None;
        }
        let (inside, outside): (Vec<&Expr>, Vec<&Expr>) = ts
            .iter()
            .enumerate()
            .filter(|(k, _)| *k != i)
            .map(|(_, t)| t)
            .partition(|t| t.free_symbols().iter().any(|s| **s == *var));
        if inside.is_empty() {
            return None;
        }
        let ExprNode::Limit {
            expr,
            var,
            point,
            dir,
        } = ts[i].node()
        else {
            return None;
        };
        let mut body = vec![expr.clone()];
        body.extend(inside.into_iter().cloned());
        let limit = Expr::new(ExprNode::Limit {
            expr: mpp_core::add(body),
            var: var.clone(),
            point: point.clone(),
            dir: *dir,
        });
        let mut all: Vec<Expr> = outside.into_iter().cloned().collect();
        if all.is_empty() {
            return Some(limit);
        }
        all.push(limit);
        Some(Expr::new(ExprNode::Add(all)))
    }

    /// `d/dx ∫_{a(x)}^{b(x)} f(x, t) dt = f(x, b) b′ − f(x, a) a′ + ∫_a^b ∂f/∂x dt`
    /// (the fundamental theorem of calculus with the Leibniz rule), so the integral itself
    /// never has to be found: `d/dx ∫_0^x e^{t^2} dt = e^{x^2}`. `None` when `expr` is not
    /// a definite integral whose bounds or integrand depend on the variable.
    fn leibniz(&mut self, whole: &Expr, expr: &Expr, vars: &[(Name, u32)]) -> Result<Option<Expr>> {
        let [(x, n)] = vars else {
            return Ok(None);
        };
        let ExprNode::Integral {
            expr: f,
            var: t,
            bounds: Some((lo, hi)),
        } = expr.node()
        else {
            return Ok(None);
        };
        if t == x || matches!(f.node(), ExprNode::Integral { .. }) {
            return Ok(None);
        }
        let xs = std::slice::from_ref(x);
        let lo = self.with_mask(xs, |r| r.resolve(lo))?.simplify();
        let hi = self.with_mask(xs, |r| r.resolve(hi))?.simplify();
        let f = self
            .with_mask(&[x.clone(), t.clone()], |r| r.resolve(f))?
            .simplify();
        if !lo.contains_sym(x) && !hi.contains_sym(x) {
            return Ok(None);
        }
        let d = |e: &Expr| mpp_core::calc::diff(e, x, 1, None).map_err(EvalError::Math);
        let mut terms = vec![f.subs(t, &hi) * d(&hi)?, -(f.subs(t, &lo) * d(&lo)?)];
        if f.contains_sym(x) {
            // With no closed form the integral stays as written (`x e^{x²} + ∫_0^x e^{t²} dt`).
            let rest = Expr::new(ExprNode::Integral {
                expr: d(&f)?,
                var: t.clone(),
                bounds: Some((lo, hi)),
            });
            terms.push(self.holder(rest.clone()).unwrap_or(rest));
        }
        let mut out = mpp_core::add(terms).simplify();
        self.ctx.log(
            "fundamental theorem of calculus (Leibniz rule)",
            whole,
            &out,
            None,
        );
        if *n > 1 {
            out = self.holder(Expr::new(ExprNode::Derivative {
                expr: out,
                vars: vec![(x.clone(), n - 1)],
            }))?;
        }
        Ok(Some(out))
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
        if self.scope.lookup(name).is_none() {
            if let Some(v) = self.random_operator(name, args)? {
                return Ok(v);
            }
        }
        let mut bound = calculus::bound_names(name, args);
        bound.extend(crate::domains_wire::bound_names(name, args));
        bound.extend(crate::dispatch::bound_names(name, args));
        bound.extend(crate::extremum::bound_names(name, args));
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
            return Err(beyond_limit(name, k));
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
            return Err(beyond_limit(name, k));
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

/// Decides `a op b` for two quantities (`12.5 mL < 25 mL`): both sides are read as
/// known quantities of one kind and compared in SI units (a bare 0 compares with any
/// kind). Sides of different kinds (`2 s < 3 m`), with names whose units are unknown,
/// or temperatures on a scale, are not decided.
fn decide_quantities(op: RelOp, a: &Expr, b: &Expr) -> Option<bool> {
    let si = |e: &Expr| -> Option<(Expr, Option<mpp_core::units::Dimension>)> {
        let crate::units_wire::Folded::Known { q, certain, .. } =
            crate::units_wire::fold(e).ok()?
        else {
            return None;
        };
        if !certain || q.unit.offset().is_some() {
            return None;
        }
        let v = (q.value.clone() * q.unit.scale()).simplify();
        let dim = (!v.is_zero()).then(|| q.unit.dimension());
        Some((v, dim))
    };
    let (x, dx) = si(a)?;
    let (y, dy) = si(b)?;
    if let (Some(dx), Some(dy)) = (&dx, &dy) {
        if dx != dy {
            return None;
        }
    }
    decide(&Expr::relation(op, x, y))
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
            if crate::units_wire::contains_units(&diff) {
                return decide_quantities(*op, a, b);
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

// ---- random variables ------------------------------------------------------------

impl Resolver<'_> {
    /// The distribution stored under the hidden key `key`, with its parameters resolved.
    fn stored_dist(&mut self, key: &str) -> Result<Option<Dist>> {
        let Some(binding) = self.scope.lookup(key) else {
            return Ok(None);
        };
        let Binding::Variable { value, .. } = &*binding else {
            return Ok(None);
        };
        let mut failure = None;
        let dist = crate::random::dist_of(value, &mut |p| {
            self.unmasked(|r| r.resolve(p))
                .map(|v| v.simplify())
                .map_err(|e| {
                    let msg = e.to_string();
                    failure.get_or_insert(e);
                    mpp_core::MathError::NotANumber(msg)
                })
        });
        match (dist, failure) {
            (Ok(d), _) => Ok(Some(d)),
            (Err(_), Some(e)) => Err(e),
            (Err(e), None) => Err(EvalError::Math(e)),
        }
    }

    /// The families (`X_j ∼ D`) whose members `e` uses, with their distributions.
    fn families_in(&mut self, e: &Expr) -> Result<Vec<(String, Dist)>> {
        fn walk(e: &Expr, out: &mut Vec<String>) {
            if let Some((name, _)) = mpp_core::family::as_member(e) {
                if !out.iter().any(|n| n == name) {
                    out.push(name.to_string());
                }
            }
            for c in e.children() {
                walk(c, out);
            }
        }
        let mut names = Vec::new();
        walk(e, &mut names);
        let mut out = Vec::new();
        for n in names {
            if self.is_masked(&n) || self.locals.contains_key(&n) {
                continue;
            }
            if let Some(d) = self.stored_dist(&format!("{FAMILY_KEY}{n}"))? {
                self.touched.insert(n.clone());
                out.push((n, d));
            }
        }
        Ok(out)
    }

    /// The document seed (`%seed n`), or 0 with a note saying so.
    fn document_seed(&self) -> u64 {
        let seed = match self.scope.lookup(crate::fields_wire::SEED_KEY).as_deref() {
            Some(Binding::Variable { value, .. }) => value
                .as_number()
                .and_then(|n| n.as_integer())
                .and_then(|n| i64::try_from(n).ok()),
            _ => None,
        };
        match seed {
            Some(s) => s as u64,
            None => {
                self.ctx
                    .note("no `%seed` line in the document, so the seed is 0");
                0
            }
        }
    }

    /// `Σ_{j=a}^{b} f(X_j, …)` over members of random families: a seeded Monte Carlo
    /// sum (`None` when the body uses no family).
    /// `\sum_i m_i`, `\sum_{i=1}^{n} (x_i - \bar{x})^2`: a sum or product whose terms
    /// pick entries of data columns by the index, term by term. Each entry is put in
    /// directly, so a 100 000-row column costs one pass instead of copying the column
    /// into every term. `None` when the body does not index a list by the variable or the
    /// range leaves the lists.
    fn data_big_op(
        &mut self,
        kind: BigOpKind,
        body: &Expr,
        var: &Name,
        lower: &Expr,
        upper: &Expr,
    ) -> Result<Option<Expr>> {
        let int = |e: &Expr| e.as_number().and_then(|n| n.as_i64());
        let (Some(lo), Some(hi)) = (int(lower), int(upper)) else {
            return Ok(None);
        };
        let mut lists: Vec<Expr> = Vec::new();
        let template = pick_entries(body, var, &mut lists);
        if lists.is_empty() || lo < 1 || hi < lo {
            return Ok(None);
        }
        let len = |l: &Expr| match l.node() {
            ExprNode::Tuple(items) => items.len(),
            _ => 0,
        };
        if lists
            .iter()
            .any(|l| usize::try_from(hi).map_or(true, |h| h > len(l)))
        {
            return Ok(None);
        }
        let names: Vec<String> = (0..lists.len()).map(entry_name).collect();
        let mut terms = Vec::with_capacity(usize::try_from(hi - lo + 1).unwrap_or(0));
        for k in lo..=hi {
            self.tick()?;
            if k % 1024 == 0 {
                mpp_core::budget::check().map_err(EvalError::Math)?;
            }
            let at = usize::try_from(k - 1).unwrap_or(0);
            let mut pairs: Vec<(&str, Expr)> = vec![(&**var, Expr::int(k))];
            for (name, list) in names.iter().zip(&lists) {
                if let ExprNode::Tuple(items) = list.node() {
                    pairs.push((name.as_str(), items[at].clone()));
                }
            }
            terms.push(template.subs_all(&pairs));
        }
        let joined = match kind {
            BigOpKind::Sum => mpp_core::add(terms),
            BigOpKind::Product => mpp_core::mul(terms),
        };
        Ok(Some(joined.simplify()))
    }

    fn random_sum(
        &mut self,
        kind: BigOpKind,
        expr: &Expr,
        var: &Name,
        lower: &Expr,
        upper: &Expr,
    ) -> Result<Option<Expr>> {
        let families = self.with_mask(std::slice::from_ref(var), |r| r.families_in(expr))?;
        if families.is_empty() {
            return Ok(None);
        }
        let bound = |r: &mut Self, e: &Expr, which: &str| -> Result<i64> {
            let v = r.resolve(e)?.simplify();
            v.as_number()
                .and_then(|n| n.as_integer())
                .and_then(|n| i64::try_from(n).ok())
                .ok_or_else(|| {
                    EvalError::Math(mpp_core::MathError::NotANumber(format!(
                        "the {which} of a sum over random samples must be a whole number, not {v}"
                    )))
                })
        };
        let lo = bound(self, lower, "start")?;
        let hi = bound(self, upper, "end")?;
        let mut masked: Vec<Name> = vec![var.clone()];
        masked.extend(families.iter().flat_map(|(n, _)| {
            [
                Name::from(n.as_str()),
                Name::from(format!("{n}{}", mpp_core::family::MARK).as_str()),
            ]
        }));
        let body = self.with_mask(&masked, |r| r.resolve(expr))?.simplify();
        let seed = self.document_seed();
        let v = crate::random::mc_sum(&crate::random::McSum {
            body: &body,
            var: var.as_ref(),
            lo,
            hi,
            families: &families,
            seed,
            product: kind == BigOpKind::Product,
        })
        .map_err(EvalError::Math)?;
        Ok(Some(Expr::float(v)))
    }

    /// `E[g]`, `Var(g)` and `P(A)` over declared random variables, exactly.
    fn random_operator(&mut self, name: &str, args: &[Expr]) -> Result<Option<Expr>> {
        let op = match name {
            crate::random::EXPECTATION | crate::random::VARIANCE | crate::random::PROBABILITY => {
                name
            }
            _ => return Ok(None),
        };
        let [arg] = args else {
            return Ok(None);
        };
        // Plain random variables (`X ∼ D`).
        let mut rvs: Vec<(String, Dist)> = Vec::new();
        for s in arg.free_symbols() {
            if self.is_masked(&s) || self.locals.contains_key(&*s) {
                continue;
            }
            if let Some(d) = self.stored_dist(&format!("{RV_KEY}{s}"))? {
                self.touched.insert(s.to_string());
                rvs.push((s.to_string(), d));
            } else if let Some((base, _)) = s.split_once('_') {
                // `X_j` outside a sum over j: one member of the family X.
                if let Some(d) = self.stored_dist(&format!("{FAMILY_KEY}{base}"))? {
                    self.touched.insert(base.to_string());
                    rvs.push((s.to_string(), d));
                }
            }
        }
        let families = self.families_in(arg)?;
        let mut masked: Vec<Name> = rvs.iter().map(|(n, _)| Name::from(n.as_str())).collect();
        masked.extend(families.iter().flat_map(|(n, _)| {
            [
                Name::from(n.as_str()),
                Name::from(format!("{n}{}", mpp_core::family::MARK).as_str()),
            ]
        }));
        let resolved = self.with_mask(&masked, |r| r.resolve(arg))?.simplify();
        // Each family member is its own variable (members are independent).
        let mut members: Vec<(Expr, String)> = Vec::new();
        let renamed = replace_members(&resolved, &mut |e| {
            let (fam, index) = crate::random::member_of(e, &families)?;
            if let Some((_, n)) = members.iter().find(|(m, _)| m == e) {
                return Some(Expr::sym(n));
            }
            let n = format!("{fam}#{}", index.to_text());
            members.push((e.clone(), n.clone()));
            Some(Expr::sym(&n))
        });
        for (e, n) in &members {
            if let Some((fam, _)) = crate::random::member_of(e, &families) {
                if let Some((_, d)) = families.iter().find(|(f, _)| *f == fam) {
                    rvs.push((n.clone(), d.clone()));
                }
            }
        }
        use mpp_core::domains::probability::random as rv;
        let out = match op {
            crate::random::EXPECTATION => rv::expectation(&renamed, &rvs),
            crate::random::VARIANCE => rv::variance(&renamed, &rvs),
            _ => rv::probability(&renamed, &rvs),
        }
        .map_err(EvalError::Math)?;
        Ok(Some(out))
    }
}

fn replace_members(e: &Expr, f: &mut impl FnMut(&Expr) -> Option<Expr>) -> Expr {
    if let Some(r) = f(e) {
        return r;
    }
    if e.children().is_empty() {
        return e.clone();
    }
    Expr::new(e.map_children(&mut |c| replace_members(c, f)))
}

/// True when `e` still holds an unevaluated derivative somewhere.
fn has_derivative(e: &Expr) -> bool {
    matches!(e.node(), ExprNode::Derivative { .. }) || e.children().into_iter().any(has_derivative)
}

/// The error for a term past the recurrence limit.
fn beyond_limit(name: &str, k: i64) -> EvalError {
    EvalError::Invalid(format!(
        "{name}_{k} is beyond the {MAX_SEQUENCE_TERMS}-term limit for recurrences"
    ))
}

/// True for a long list of numbers and quantities (`3`, `1.5 · m`): a data column.
fn is_plain_data(e: &Expr) -> bool {
    let ExprNode::Tuple(items) = e.node() else {
        return false;
    };
    let plain = |i: &Expr| match i.node() {
        ExprNode::Num(_) => true,
        ExprNode::Mul(fs) => fs.iter().all(|f| match f.node() {
            ExprNode::Num(_) => true,
            ExprNode::Apply(name, args) => {
                args.is_empty() && name.starts_with(crate::units_wire::UNIT_PREFIX)
            }
            _ => false,
        }),
        _ => false,
    };
    items.len() >= 64 && items.iter().all(plain)
}

/// The placeholder for the entries of the `j`-th list in [`pick_entries`].
fn entry_name(j: usize) -> String {
    format!("\u{0}entry{j}")
}

/// `body` with every `element(list, var)` replaced by a placeholder name; the lists go
/// into `lists`, one per placeholder.
fn pick_entries(body: &Expr, var: &str, lists: &mut Vec<Expr>) -> Expr {
    if let ExprNode::Apply(name, args) = body.node() {
        if &**name == crate::domains_wire::ELEMENT {
            if let [list, index] = args.as_slice() {
                if matches!(list.node(), ExprNode::Tuple(_)) && index.as_sym() == Some(var) {
                    let j = match lists.iter().position(|l| l == list) {
                        Some(j) => j,
                        None => {
                            lists.push(list.clone());
                            lists.len() - 1
                        }
                    };
                    return Expr::sym(&entry_name(j));
                }
            }
        }
    }
    Expr::new(body.map_children(&mut |c| pick_entries(c, var, lists)))
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
