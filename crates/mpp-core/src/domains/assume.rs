//! Assumptions about symbols (`x > 0`, `n ∈ ℤ`, `a ≠ 0`) and the simplifications,
//! integrals and limits that depend on them.
//!
//! [`Assumptions`] keeps, for each symbol, an interval of the real line it lies in, the
//! points it avoids and whether it is an integer. Every fact also makes the symbol real.
//! Contradictory facts are an error ([`MathError::Contradiction`]).
//!
//! What uses them (all additive: the plain `simplify`, `integrate` and `limit` are
//! unchanged):
//! - [`simplify_with`]: `√(x²) = |x|` always (for real `x`) and `= x` when `x ≥ 0`;
//!   `|x| = x` or `−x` by sign; `ln(eˣ) = x` for real `x`; `(xᵃ)ᵇ = xᵃᵇ` for `x > 0`
//!   with real exponents, or an integer `b`; piecewise branches decided by the facts.
//!   Without the facts the safe form stays and the step log says why.
//! - [`integrate_power`]: `∫ xⁿ dx` with a symbolic `n` splits on `n = −1` unless the
//!   facts decide it.
//! - [`limit_at_infinity`] and [`improper_integral`]: limits at ±∞ whose value depends on
//!   the sign of a parameter (`e^{a x}`, `x^{1−p}`) use the facts, and give a piecewise
//!   answer over the sign cases when the facts don't decide.

use std::cmp::Ordering;
use std::collections::BTreeMap;

use crate::domains::util;
use crate::error::{MathError, MathResult};
use crate::expr::{Constant, Expr, ExprNode, LogicOp, RelOp};
use crate::functions::Func;
use crate::simplify::{add, mul};
use crate::steps::Steps;

/// One end of an interval: the value and whether it is included.
#[derive(Clone, Debug, PartialEq)]
pub struct End {
    pub value: Expr,
    pub closed: bool,
}

/// What is known about one symbol.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct Facts {
    /// Lower end (none: unbounded below).
    pub lo: Option<End>,
    /// Upper end (none: unbounded above).
    pub hi: Option<End>,
    /// Points the symbol avoids (`a ≠ 0`).
    pub excluded: Vec<Expr>,
    /// The symbol is an integer.
    pub integer: bool,
}

/// The facts in force at one point of a document.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct Assumptions {
    vars: BTreeMap<String, Facts>,
}

/// Which signs a value may have.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Signs {
    pub negative: bool,
    pub zero: bool,
    pub positive: bool,
}

impl Signs {
    const ANY: Signs = Signs {
        negative: true,
        zero: true,
        positive: true,
    };
    const POS: Signs = Signs {
        negative: false,
        zero: false,
        positive: true,
    };
    const NEG: Signs = Signs {
        negative: true,
        zero: false,
        positive: false,
    };
    const ZERO: Signs = Signs {
        negative: false,
        zero: true,
        positive: false,
    };

    fn of(o: Ordering) -> Signs {
        match o {
            Ordering::Less => Signs::NEG,
            Ordering::Equal => Signs::ZERO,
            Ordering::Greater => Signs::POS,
        }
    }

    /// True when the value is certainly ≥ 0.
    pub fn nonnegative(self) -> bool {
        !self.negative
    }

    /// True when the value is certainly ≤ 0.
    pub fn nonpositive(self) -> bool {
        !self.positive
    }

    /// True when the value is certainly > 0.
    pub fn positive(self) -> bool {
        !self.negative && !self.zero
    }

    /// True when the value is certainly < 0.
    pub fn negative(self) -> bool {
        !self.positive && !self.zero
    }

    /// True when the value is certainly ≠ 0.
    pub fn nonzero(self) -> bool {
        !self.zero
    }

    /// The one sign, when it is known.
    pub fn exact(self) -> Option<Ordering> {
        match (self.negative, self.zero, self.positive) {
            (true, false, false) => Some(Ordering::Less),
            (false, true, false) => Some(Ordering::Equal),
            (false, false, true) => Some(Ordering::Greater),
            _ => None,
        }
    }

    fn neg(self) -> Signs {
        Signs {
            negative: self.positive,
            zero: self.zero,
            positive: self.negative,
        }
    }

    fn times(self, o: Signs) -> Signs {
        let mut out = Signs {
            negative: false,
            zero: self.zero || o.zero,
            positive: false,
        };
        if (self.positive && o.positive) || (self.negative && o.negative) {
            out.positive = true;
        }
        if (self.positive && o.negative) || (self.negative && o.positive) {
            out.negative = true;
        }
        out
    }

    fn plus(self, o: Signs) -> Signs {
        if self == Signs::ZERO {
            return o;
        }
        if o == Signs::ZERO {
            return self;
        }
        let nonneg = self.nonnegative() && o.nonnegative();
        let nonpos = self.nonpositive() && o.nonpositive();
        if nonneg {
            return Signs {
                negative: false,
                zero: self.zero && o.zero,
                positive: true,
            };
        }
        if nonpos {
            return Signs {
                negative: true,
                zero: self.zero && o.zero,
                positive: false,
            };
        }
        Signs::ANY
    }
}

fn contradiction(name: &str, facts: &Facts) -> MathError {
    MathError::Contradiction(format!(
        "these assumptions can't all hold: {}",
        facts.describe(name)
    ))
}

impl Facts {
    /// The facts as a sentence about `name`: `x > 0 and x < 0`.
    pub fn describe(&self, name: &str) -> String {
        let mut parts = Vec::new();
        if let Some(lo) = &self.lo {
            let op = if lo.closed { "≥" } else { ">" };
            parts.push(format!("{name} {op} {}", lo.value));
        }
        if let Some(hi) = &self.hi {
            let op = if hi.closed { "≤" } else { "<" };
            parts.push(format!("{name} {op} {}", hi.value));
        }
        for x in &self.excluded {
            parts.push(format!("{name} ≠ {x}"));
        }
        if self.integer {
            parts.push(format!("{name} is an integer"));
        }
        parts.join(" and ")
    }
}

impl Assumptions {
    /// No facts.
    pub fn new() -> Assumptions {
        Assumptions::default()
    }

    /// True when nothing is assumed.
    pub fn is_empty(&self) -> bool {
        self.vars.is_empty()
    }

    /// The facts about `name`.
    pub fn facts(&self, name: &str) -> Option<&Facts> {
        self.vars.get(name)
    }

    /// The symbols something is known about.
    pub fn symbols(&self) -> impl Iterator<Item = &str> {
        self.vars.keys().map(String::as_str)
    }

    /// Adds every fact of `other`.
    pub fn extend(&mut self, other: &Assumptions) -> MathResult<()> {
        for (name, f) in &other.vars {
            if let Some(lo) = &f.lo {
                self.bound(name, lo.clone(), true)?;
            }
            if let Some(hi) = &f.hi {
                self.bound(name, hi.clone(), false)?;
            }
            for x in &f.excluded {
                self.exclude(name, x.clone())?;
            }
            if f.integer {
                self.integer(name)?;
            }
        }
        Ok(())
    }

    /// Adds a condition: a relation (`x > 0`, `0 < x \le 1`, `a ≠ 0`, `2x - 1 ≥ 0`),
    /// a membership (`n ∈ ℤ`, `x ∈ [0, 1]`, `x ∈ ℝ`) or a conjunction of these.
    pub fn assume(&mut self, cond: &Expr) -> MathResult<()> {
        match cond.node() {
            ExprNode::Logic(LogicOp::And, parts) => parts.iter().try_for_each(|p| self.assume(p)),
            ExprNode::Relation(op, a, b) => self.relation(*op, a, b),
            ExprNode::Apply(name, args) if &**name == "in" => match args.as_slice() {
                [x, set] => self.member(x, set),
                _ => Err(unsupported(cond)),
            },
            ExprNode::Bool(true) => Ok(()),
            ExprNode::Bool(false) => {
                Err(MathError::Contradiction("this assumption is false".into()))
            }
            _ => Err(unsupported(cond)),
        }
    }

    fn member(&mut self, x: &Expr, set: &Expr) -> MathResult<()> {
        let name = x.as_sym().ok_or_else(|| unsupported(x))?.to_string();
        match set.node() {
            ExprNode::Sym(s) => match &**s {
                "ℤ" | "Z" => self.integer(&name),
                "ℕ" | "N" => {
                    self.integer(&name)?;
                    self.bound(
                        &name,
                        End {
                            value: Expr::zero(),
                            closed: true,
                        },
                        true,
                    )
                }
                "ℝ" | "R" | "ℚ" | "Q" => {
                    self.vars.entry(name).or_default();
                    Ok(())
                }
                _ => Err(unsupported(set)),
            },
            ExprNode::Interval {
                lo,
                hi,
                lo_closed,
                hi_closed,
            } => {
                if !lo.is_infinite() {
                    self.bound(
                        &name,
                        End {
                            value: lo.clone(),
                            closed: *lo_closed,
                        },
                        true,
                    )?;
                }
                if !hi.is_infinite() {
                    self.bound(
                        &name,
                        End {
                            value: hi.clone(),
                            closed: *hi_closed,
                        },
                        false,
                    )?;
                }
                self.vars.entry(name).or_default();
                Ok(())
            }
            _ => Err(unsupported(set)),
        }
    }

    /// `a op b` where one side is linear in a single symbol and the other is constant.
    fn relation(&mut self, op: RelOp, a: &Expr, b: &Expr) -> MathResult<()> {
        let diff = (a - b).simplify();
        let syms = diff.free_symbols();
        let [name] = syms.iter().collect::<Vec<_>>()[..] else {
            return Err(unsupported(&Expr::relation(op, a.clone(), b.clone())));
        };
        let name = name.to_string();
        let poly = diff
            .as_poly(&name)
            .filter(|c| c.len() == 2 && c.iter().all(Expr::is_constant))
            .ok_or_else(|| unsupported(&Expr::relation(op, a.clone(), b.clone())))?;
        // c1·x + c0 op 0  ⇔  x op' −c0/c1, with op' flipped when c1 < 0.
        let (c0, c1) = (&poly[0], &poly[1]);
        let flip = match util::sign(c1) {
            Some(Ordering::Greater) => false,
            Some(Ordering::Less) => true,
            _ => return Err(unsupported(&Expr::relation(op, a.clone(), b.clone()))),
        };
        let point = (-c0.clone() / c1.clone()).simplify();
        let op = if flip { flipped(op) } else { op };
        match op {
            RelOp::Gt | RelOp::Ge => self.bound(
                &name,
                End {
                    value: point,
                    closed: op == RelOp::Ge,
                },
                true,
            ),
            RelOp::Lt | RelOp::Le => self.bound(
                &name,
                End {
                    value: point,
                    closed: op == RelOp::Le,
                },
                false,
            ),
            RelOp::Eq | RelOp::Approx => {
                self.bound(
                    &name,
                    End {
                        value: point.clone(),
                        closed: true,
                    },
                    true,
                )?;
                self.bound(
                    &name,
                    End {
                        value: point,
                        closed: true,
                    },
                    false,
                )
            }
            RelOp::Ne => self.exclude(&name, point),
        }
    }

    fn bound(&mut self, name: &str, end: End, lower: bool) -> MathResult<()> {
        if !end.value.is_constant() || util::numeric(&end.value).is_none() {
            return Err(MathError::Unsupported(format!(
                "an assumption with the bound {}",
                end.value
            )));
        }
        let f = self.vars.entry(name.to_string()).or_default();
        let slot = if lower { &mut f.lo } else { &mut f.hi };
        let tighter = match slot {
            None => true,
            Some(old) => match util::compare(&end.value, &old.value) {
                Some(Ordering::Greater) => lower,
                Some(Ordering::Less) => !lower,
                Some(Ordering::Equal) => old.closed && !end.closed,
                None => false,
            },
        };
        if tighter {
            *slot = Some(end);
        }
        self.check(name)
    }

    fn exclude(&mut self, name: &str, point: Expr) -> MathResult<()> {
        let f = self.vars.entry(name.to_string()).or_default();
        if !f.excluded.contains(&point) {
            f.excluded.push(point);
        }
        self.check(name)
    }

    fn integer(&mut self, name: &str) -> MathResult<()> {
        self.vars.entry(name.to_string()).or_default().integer = true;
        self.check(name)
    }

    /// Reports facts about `name` that can't all hold.
    fn check(&self, name: &str) -> MathResult<()> {
        let Some(f) = self.vars.get(name) else {
            return Ok(());
        };
        if let (Some(lo), Some(hi)) = (&f.lo, &f.hi) {
            match util::compare(&lo.value, &hi.value) {
                Some(Ordering::Greater) => return Err(contradiction(name, f)),
                Some(Ordering::Equal) => {
                    if !(lo.closed && hi.closed) || f.excluded.contains(&lo.value) {
                        return Err(contradiction(name, f));
                    }
                    if f.integer && util::integer(&lo.value).is_none() {
                        return Err(contradiction(name, f));
                    }
                }
                _ => {}
            }
            if f.integer {
                // An integer strictly inside, or on a closed end.
                let first = util::numeric(&lo.value).map(|v| {
                    if lo.closed {
                        v.ceil()
                    } else {
                        v.floor() + 1.0
                    }
                });
                let last = util::numeric(&hi.value).map(|v| {
                    if hi.closed {
                        v.floor()
                    } else {
                        v.ceil() - 1.0
                    }
                });
                if let (Some(a), Some(b)) = (first, last) {
                    if a > b {
                        return Err(contradiction(name, f));
                    }
                }
            }
        }
        Ok(())
    }

    // ---- what the facts imply ---------------------------------------------------

    /// True when `name` is known to be real (every fact makes a symbol real).
    pub fn is_real_symbol(&self, name: &str) -> bool {
        self.vars.contains_key(name)
    }

    /// The signs `e` may have, for a real `e`; `None` when `e` may not be real.
    pub fn signs(&self, e: &Expr) -> Option<Signs> {
        match e.node() {
            ExprNode::Num(n) => {
                if !n.is_real() {
                    return None;
                }
                n.sign().map(Signs::of)
            }
            ExprNode::Const(Constant::Pi | Constant::E) => Some(Signs::POS),
            ExprNode::Const(Constant::Infinity) => Some(Signs::POS),
            ExprNode::Const(_) => None,
            ExprNode::Sym(s) => {
                let f = self.vars.get(&**s)?;
                Some(self.symbol_signs(f, &Expr::zero()))
            }
            ExprNode::Add(_) if self.linear_signs(e).is_some() => self.linear_signs(e),
            ExprNode::Add(terms) => {
                let mut acc = Signs::ZERO;
                for t in terms {
                    acc = acc.plus(self.signs(t)?);
                }
                Some(acc)
            }
            ExprNode::Mul(factors) => {
                let mut acc = Signs::POS;
                for f in factors {
                    acc = acc.times(self.signs(f)?);
                }
                Some(acc)
            }
            ExprNode::Pow(b, k) => self.power_signs(b, k),
            ExprNode::Func(f, args) => self.function_signs(*f, args),
            // A unit factor (`unit:m`, the evaluator's quantities) is a positive constant.
            ExprNode::Apply(name, args) if args.is_empty() && name.starts_with("unit:") => {
                Some(Signs::POS)
            }
            _ => None,
        }
    }

    /// Signs of `c·x + d` for one symbol `x`, from where `x` lies relative to `−d/c`.
    fn linear_signs(&self, e: &Expr) -> Option<Signs> {
        let syms = e.free_symbols();
        let [name] = syms.iter().collect::<Vec<_>>()[..] else {
            return None;
        };
        let f = self.vars.get(&**name)?;
        let c = e
            .as_poly(name)
            .filter(|c| c.len() == 2 && c.iter().all(Expr::is_constant))?;
        let slope = util::sign(&c[1])?;
        let point = (-c[0].clone() / c[1].clone()).simplify();
        let rel = self.symbol_signs(f, &point);
        match slope {
            Ordering::Greater => Some(rel),
            Ordering::Less => Some(rel.neg()),
            Ordering::Equal => None,
        }
    }

    /// Signs of `x − point` for a symbol with facts `f`.
    fn symbol_signs(&self, f: &Facts, point: &Expr) -> Signs {
        let zero = point.clone();
        let mut s = Signs::ANY;
        // Lower end ≥ 0 rules out negatives; > 0 (or = 0 open) rules out zero too.
        if let Some(lo) = &f.lo {
            match util::compare(&lo.value, &zero) {
                Some(Ordering::Greater) => {
                    s.negative = false;
                    s.zero = false;
                }
                Some(Ordering::Equal) => {
                    s.negative = false;
                    if !lo.closed {
                        s.zero = false;
                    }
                }
                _ => {}
            }
        }
        if let Some(hi) = &f.hi {
            match util::compare(&hi.value, &zero) {
                Some(Ordering::Less) => {
                    s.positive = false;
                    s.zero = false;
                }
                Some(Ordering::Equal) => {
                    s.positive = false;
                    if !hi.closed {
                        s.zero = false;
                    }
                }
                _ => {}
            }
        }
        if f.excluded.iter().any(|x| util::equal(x, point)) {
            s.zero = false;
        }
        s
    }

    fn power_signs(&self, b: &Expr, k: &Expr) -> Option<Signs> {
        let kn = k.as_number();
        let integer_k = kn.and_then(|n| n.as_integer());
        if let Some(n) = integer_k {
            let bs = self.signs(b)?;
            let even = (&n % 2u32) == 0u32.into();
            let zero = bs.zero && n > 0u32.into();
            if even {
                return Some(Signs {
                    negative: false,
                    zero,
                    positive: true,
                });
            }
            return Some(Signs {
                negative: bs.negative,
                zero,
                positive: bs.positive,
            });
        }
        let bs = self.signs(b)?;
        if bs.positive() && self.is_real(k) {
            return Some(Signs::POS);
        }
        // A root of a non-negative value (k a positive fraction).
        if bs.nonnegative()
            && kn.is_some_and(|n| n.is_real() && n.sign() == Some(Ordering::Greater))
        {
            return Some(Signs {
                negative: false,
                zero: bs.zero,
                positive: true,
            });
        }
        None
    }

    fn function_signs(&self, f: Func, args: &[Expr]) -> Option<Signs> {
        let [x] = args else {
            return None;
        };
        match f {
            Func::Abs => {
                let s = self.signs(x);
                Some(Signs {
                    negative: false,
                    zero: s.is_none_or(|s| s.zero),
                    positive: true,
                })
            }
            Func::Cosh => self.is_real(x).then_some(Signs::POS),
            Func::Sign => self.signs(x),
            Func::Ln => {
                let s = self.signs(&(x - &Expr::one()).simplify())?;
                if self.signs(x)?.positive() {
                    Some(s)
                } else {
                    None
                }
            }
            Func::Atan | Func::Sinh | Func::Tanh | Func::Asinh => self.signs(x),
            _ => None,
        }
    }

    /// True when `e` is known to be real.
    pub fn is_real(&self, e: &Expr) -> bool {
        match e.node() {
            ExprNode::Num(n) => n.is_real(),
            ExprNode::Const(Constant::Pi | Constant::E) => true,
            ExprNode::Sym(s) => self.is_real_symbol(s),
            ExprNode::Add(v) | ExprNode::Mul(v) => v.iter().all(|t| self.is_real(t)),
            ExprNode::Pow(b, k) => {
                self.is_real(b)
                    && self.is_real(k)
                    && (self.is_integer(k) || self.signs(b).is_some_and(Signs::positive))
            }
            ExprNode::Func(f, args) => match f {
                Func::Sin
                | Func::Cos
                | Func::Atan
                | Func::Sinh
                | Func::Cosh
                | Func::Tanh
                | Func::Abs
                | Func::Floor
                | Func::Ceil
                | Func::Sign
                | Func::Asinh => args.iter().all(|a| self.is_real(a)),
                Func::Ln => args
                    .first()
                    .and_then(|a| self.signs(a))
                    .is_some_and(Signs::positive),
                _ => false,
            },
            _ => false,
        }
    }

    /// True when `e` is known to be an integer.
    pub fn is_integer(&self, e: &Expr) -> bool {
        match e.node() {
            ExprNode::Num(n) => n.as_integer().is_some(),
            ExprNode::Sym(s) => self.vars.get(&**s).is_some_and(|f| f.integer),
            ExprNode::Add(v) | ExprNode::Mul(v) => v.iter().all(|t| self.is_integer(t)),
            ExprNode::Pow(b, k) => {
                self.is_integer(b)
                    && self.is_integer(k)
                    && self.signs(k).is_some_and(Signs::nonnegative)
            }
            _ => false,
        }
    }

    /// Decides a condition from the facts, when they settle it.
    pub fn decide(&self, cond: &Expr) -> Option<bool> {
        match cond.node() {
            ExprNode::Bool(b) => Some(*b),
            ExprNode::Relation(op, a, b) => {
                let d = (a - b).simplify();
                if let Some(o) = util::sign(&d).filter(|_| d.is_constant()) {
                    return Some(holds(*op, Signs::of(o)));
                }
                let s = self.signs(&d)?;
                let all = |f: &dyn Fn(Ordering) -> bool| {
                    [
                        (s.negative, Ordering::Less),
                        (s.zero, Ordering::Equal),
                        (s.positive, Ordering::Greater),
                    ]
                    .iter()
                    .filter(|(possible, _)| *possible)
                    .map(|(_, o)| f(*o))
                    .collect::<Vec<bool>>()
                };
                let results = all(&|o| holds(*op, Signs::of(o)));
                if results.iter().all(|r| *r) {
                    Some(true)
                } else if results.iter().all(|r| !r) {
                    Some(false)
                } else {
                    None
                }
            }
            ExprNode::Logic(LogicOp::And, parts) => {
                let mut all = true;
                for p in parts {
                    match self.decide(p) {
                        Some(false) => return Some(false),
                        Some(true) => {}
                        None => all = false,
                    }
                }
                all.then_some(true)
            }
            ExprNode::Logic(LogicOp::Or, parts) => {
                let mut none = true;
                for p in parts {
                    match self.decide(p) {
                        Some(true) => return Some(true),
                        Some(false) => {}
                        None => none = false,
                    }
                }
                none.then_some(false)
            }
            ExprNode::Logic(LogicOp::Not, parts) => {
                parts.first().and_then(|p| self.decide(p)).map(|b| !b)
            }
            ExprNode::Apply(name, args) if &**name == "in" => match args.as_slice() {
                [x, set] if matches!(set.as_sym(), Some("ℤ" | "Z")) => {
                    self.is_integer(x).then_some(true)
                }
                [x, set] if matches!(set.as_sym(), Some("ℝ" | "R")) => {
                    self.is_real(x).then_some(true)
                }
                _ => None,
            },
            _ => None,
        }
    }
}

fn holds(op: RelOp, s: Signs) -> bool {
    match (op, s.exact()) {
        (RelOp::Eq | RelOp::Approx, Some(o)) => o == Ordering::Equal,
        (RelOp::Ne, Some(o)) => o != Ordering::Equal,
        (RelOp::Lt, Some(o)) => o == Ordering::Less,
        (RelOp::Le, Some(o)) => o != Ordering::Greater,
        (RelOp::Gt, Some(o)) => o == Ordering::Greater,
        (RelOp::Ge, Some(o)) => o != Ordering::Less,
        _ => false,
    }
}

fn flipped(op: RelOp) -> RelOp {
    match op {
        RelOp::Lt => RelOp::Gt,
        RelOp::Le => RelOp::Ge,
        RelOp::Gt => RelOp::Lt,
        RelOp::Ge => RelOp::Le,
        other => other,
    }
}

fn unsupported(e: &Expr) -> MathError {
    MathError::Unsupported(format!(
        "assuming {e} (assume a bound on one name, such as x > 0, a ≠ 0 or n ∈ ℤ)"
    ))
}

// ---- simplification with assumptions ------------------------------------------------

/// `e` simplified with what `a` knows. Rewrites that need a fact keep the safe form
/// without it, and the step log says why.
pub fn simplify_with(e: &Expr, a: &Assumptions, mut steps: Option<&mut Steps>) -> Expr {
    let out = rewrite(&e.simplify(), a, &mut steps).simplify();
    if out != e.simplify() {
        Steps::record(&mut steps, "simplify with the assumptions", e, &out, None);
    }
    out
}

fn rewrite(e: &Expr, a: &Assumptions, steps: &mut Option<&mut Steps>) -> Expr {
    let e = Expr::new(e.map_children(&mut |c| rewrite(c, a, steps)));
    match e.node() {
        ExprNode::Pow(b, k) => power(&e, b, k, a, steps),
        ExprNode::Func(Func::Abs, args) => match args.as_slice() {
            [x] => absolute(&e, x, a, steps),
            _ => e,
        },
        // sin(nπ) = 0 and cos(nπ) = (−1)ⁿ for an integer n.
        ExprNode::Func(f @ (Func::Sin | Func::Cos), args) => match args.as_slice() {
            [arg] => match integer_multiple_of_pi(arg, a) {
                Some(n) if *f == Func::Sin => {
                    let _ = n;
                    Expr::zero()
                }
                Some(n) => Expr::int(-1).pow(n),
                None => e,
            },
            _ => e,
        },
        ExprNode::Func(Func::Ln, args) => match args.as_slice() {
            [x] => match x.node() {
                ExprNode::Pow(base, y) if matches!(base.node(), ExprNode::Const(Constant::E)) => {
                    if a.is_real(y) {
                        y.clone()
                    } else {
                        note(
                            steps,
                            &e,
                            format!(
                                "ln(e^{y}) = {y} only for real {y}; add an assumption such as `Assume ${y} \\in \\mathbb{{R}}$.`"
                            ),
                        );
                        e
                    }
                }
                _ => e,
            },
            _ => e,
        },
        ExprNode::Piecewise { cases, otherwise } => {
            let mut kept = Vec::new();
            for (v, c) in cases {
                match a.decide(c) {
                    Some(true) if kept.is_empty() => return v.clone(),
                    Some(true) => {
                        return Expr::new(ExprNode::Piecewise {
                            cases: kept,
                            otherwise: Some(v.clone()),
                        })
                    }
                    Some(false) => {}
                    None => kept.push((v.clone(), c.clone())),
                }
            }
            match (kept.is_empty(), otherwise) {
                (true, Some(o)) => o.clone(),
                _ => Expr::new(ExprNode::Piecewise {
                    cases: kept,
                    otherwise: otherwise.clone(),
                }),
            }
        }
        ExprNode::Relation(..) => match a.decide(&e) {
            Some(b) if !a.is_empty() => Expr::new(ExprNode::Bool(b)),
            _ => e,
        },
        _ => e,
    }
}

/// `n` when `arg = n·π` with `n` an integer by the facts (and not a plain number,
/// which the simplifier handles itself).
fn integer_multiple_of_pi(arg: &Expr, a: &Assumptions) -> Option<Expr> {
    let ExprNode::Mul(factors) = arg.node() else {
        return None;
    };
    let pi = factors
        .iter()
        .position(|f| matches!(f.node(), ExprNode::Const(Constant::Pi)))?;
    let rest: Vec<Expr> = factors
        .iter()
        .enumerate()
        .filter(|(i, _)| *i != pi)
        .map(|(_, f)| f.clone())
        .collect();
    let n = mul(rest);
    (!n.is_constant() && a.is_integer(&n)).then_some(n)
}

fn note(steps: &mut Option<&mut Steps>, e: &Expr, why: String) {
    Steps::record(steps, "keep the safe form", e, e, Some(why));
}

/// `|x|` → `x` or `−x` when the sign of `x` is known.
fn absolute(e: &Expr, x: &Expr, a: &Assumptions, steps: &mut Option<&mut Steps>) -> Expr {
    match a.signs(x) {
        Some(s) if s.nonnegative() => x.clone(),
        Some(s) if s.nonpositive() => (-x.clone()).simplify(),
        _ => {
            if !x.is_constant() {
                note(
                    steps,
                    e,
                    format!("|{x}| stays: {x} may be negative or positive"),
                );
            }
            e.clone()
        }
    }
}

fn power(e: &Expr, b: &Expr, k: &Expr, a: &Assumptions, steps: &mut Option<&mut Steps>) -> Expr {
    // 0^k = 0 and 1^k = 1 for k > 0.
    if (util::is_zero(b) || b.is_one()) && a.signs(k).is_some_and(Signs::positive) {
        return b.clone();
    }
    let ExprNode::Pow(x, inner_k) = b.node() else {
        return e.clone();
    };
    // √(x²) = |x| (for real x), (x⁴)^(1/4) = |x| …
    let even_root = match (inner_k.as_number(), k.as_number()) {
        (Some(m), Some(r)) => {
            let prod = (Expr::num(m.clone()) * Expr::num(r.clone())).simplify();
            prod.is_one()
                && m.as_integer()
                    .is_some_and(|n| (&n % 2u32) == 0u32.into() && n > 0u32.into())
        }
        _ => false,
    };
    if even_root {
        let abs = Expr::new(ExprNode::Func(Func::Abs, vec![x.clone()]));
        if a.signs(x)
            .is_none_or(|s| !s.nonnegative() && !s.nonpositive())
            && !x.is_constant()
        {
            note(
                steps,
                e,
                format!(
                    "{e} = |{x}| because {x} may be negative; `Assume ${x} \\ge 0$.` gives {x}"
                ),
            );
        }
        return absolute(&abs, x, a, steps);
    }
    // (xᵃ)ᵇ = x^(ab) for x > 0 with real exponents, or for an integer b.
    let integer_k = a.is_integer(k);
    let positive_base = a.signs(x).is_some_and(Signs::positive);
    if integer_k || (positive_base && a.is_real(inner_k) && a.is_real(k)) {
        return x.pow(mul(vec![inner_k.clone(), k.clone()]));
    }
    if !x.is_constant() && !(inner_k.is_constant() && k.is_constant() && positive_base) {
        note(
            steps,
            e,
            format!("({x}^{inner_k})^{k} stays: combining the powers needs {x} > 0 (or an integer power)"),
        );
    }
    e.clone()
}

// ---- integrals and limits that depend on a parameter ----------------------------------

/// `∫ c·xⁿ dx` with a symbolic `n` (no other dependence on `x`): `c·x^(n+1)/(n+1)`
/// when the facts say `n ≠ −1`, `c·ln|x|` when `n = −1`, and the piecewise answer
/// over both cases otherwise. `None` when the integrand is not of that shape.
pub fn integrate_power(e: &Expr, var: &str, a: &Assumptions) -> Option<Expr> {
    let (c, n) = power_term(&e.simplify(), var)?;
    let n1 = (n.clone() + Expr::one()).simplify();
    let general = mul(vec![c.clone(), Expr::sym(var).pow(n1.clone()), n1.pow(-1)]);
    let log = mul(vec![
        c,
        Expr::func(
            Func::Ln,
            vec![Expr::new(ExprNode::Func(Func::Abs, vec![Expr::sym(var)]))],
        )
        .ok()?,
    ]);
    let ne = Expr::relation(RelOp::Ne, n.clone(), Expr::int(-1));
    Some(match a.decide(&ne) {
        Some(true) => general,
        Some(false) => log,
        None => Expr::new(ExprNode::Piecewise {
            cases: vec![(general, ne)],
            otherwise: Some(log),
        }),
    })
}

/// `c · var^n` with `c` free of `var` and `n` symbolic (free of `var`).
fn power_term(e: &Expr, var: &str) -> Option<(Expr, Expr)> {
    let is_power = |f: &Expr| -> Option<Expr> {
        match f.node() {
            ExprNode::Pow(b, n) if b.is_sym(var) && !n.contains_sym(var) && !n.is_constant() => {
                Some(n.clone())
            }
            _ => None,
        }
    };
    if let Some(n) = is_power(e) {
        return Some((Expr::one(), n));
    }
    let ExprNode::Mul(factors) = e.node() else {
        return None;
    };
    let mut n = None;
    let mut rest = Vec::new();
    for f in factors {
        match is_power(f) {
            Some(p) if n.is_none() => n = Some(p),
            _ if f.contains_sym(var) => return None,
            _ => rest.push(f.clone()),
        }
    }
    Some((mul(rest), n?))
}

/// The limit of `e` as `var → +∞` (or `−∞` with `negative`), using the signs of the
/// parameters; a piecewise answer over the sign cases of a parameter the facts don't
/// settle. `Ok(None)` when the expression is outside what this handles.
pub fn limit_at_infinity(
    e: &Expr,
    var: &str,
    negative: bool,
    a: &Assumptions,
) -> MathResult<Option<Expr>> {
    let e = if negative {
        e.subs(var, &(-Expr::sym(var)))
    } else {
        e.clone()
    };
    split_cases(a, 0, &mut |facts| match lim(&e.simplify(), var, facts) {
        Ok(l) => Outcome::Value(l.to_expr()),
        Err(Need::Sign(s)) => Outcome::Need(s),
        Err(Need::Unsupported) => Outcome::Unsupported,
    })
}

/// `∫_lo^∞ e dvar` (or from −∞) through the antiderivative and a limit that may depend
/// on the sign of a parameter. `Ok(None)` when this doesn't apply.
pub fn improper_integral(
    e: &Expr,
    var: &str,
    lo: &Expr,
    hi: &Expr,
    a: &Assumptions,
) -> MathResult<Option<Expr>> {
    let up = hi.is_infinite() && !lo.is_infinite();
    let down = lo.is_infinite() && !hi.is_infinite();
    if !(up || down) {
        return Ok(None);
    }
    let anti = match integrate_power(e, var, a) {
        Some(f) => f,
        None => match crate::calc::integrate(e, var, None) {
            Ok(f) => f,
            Err(_) => return Ok(None),
        },
    };
    let finite = if up { lo } else { hi };
    let sign_up = up && util::sign(hi) == Some(Ordering::Greater);
    let at_finite = anti.subs(var, finite).simplify();
    let result = split_cases(a, 0, &mut |facts| {
        let f = simplify_with(&anti, facts, None);
        if let Some(s) = undecided_condition(&f) {
            return Outcome::Need(s);
        }
        let at_end = match lim(&f.simplify(), var, facts) {
            Ok(l) if sign_up || down => l,
            Ok(l) => l,
            Err(Need::Sign(s)) => return Outcome::Need(s),
            Err(Need::Unsupported) => return Outcome::Unsupported,
        };
        let at_end = if down {
            // From −∞: substitute x → −x in the antiderivative.
            match lim(&f.subs(var, &(-Expr::sym(var))).simplify(), var, facts) {
                Ok(l) => l,
                Err(Need::Sign(s)) => return Outcome::Need(s),
                Err(Need::Unsupported) => return Outcome::Unsupported,
            }
        } else {
            at_end
        };
        let base = simplify_with(&at_finite, facts, None);
        let value = match at_end {
            Lim::Finite(v) => {
                if up {
                    (v - base).simplify()
                } else {
                    (base - v).simplify()
                }
            }
            Lim::PosInf => {
                if up {
                    Expr::infinity()
                } else {
                    Expr::neg_infinity()
                }
            }
            Lim::NegInf => {
                if up {
                    Expr::neg_infinity()
                } else {
                    Expr::infinity()
                }
            }
        };
        Outcome::Value(value)
    })?;
    Ok(result)
}

/// For a piecewise value whose first condition the facts don't decide, the expression
/// whose sign settles it (`n ≠ −1` gives `n + 1`).
fn undecided_condition(e: &Expr) -> Option<Expr> {
    let ExprNode::Piecewise { cases, .. } = e.node() else {
        return None;
    };
    match cases.first()?.1.node() {
        ExprNode::Relation(_, l, r) => Some((l - r).simplify()),
        _ => None,
    }
}

enum Outcome {
    Value(Expr),
    Need(Expr),
    Unsupported,
}

/// Most nested sign splits.
const MAX_SPLITS: usize = 3;

/// Runs `f` under the facts; when it needs the sign of `s`, runs it again under
/// `s > 0`, `s = 0` and `s < 0` and joins the answers into a piecewise value.
fn split_cases(
    a: &Assumptions,
    depth: usize,
    f: &mut dyn FnMut(&Assumptions) -> Outcome,
) -> MathResult<Option<Expr>> {
    match f(a) {
        Outcome::Value(v) => Ok(Some(v)),
        Outcome::Unsupported => Ok(None),
        Outcome::Need(s) => {
            if depth >= MAX_SPLITS {
                return Ok(None);
            }
            let mut cases: Vec<(Expr, Expr)> = Vec::new();
            for op in [RelOp::Gt, RelOp::Eq, RelOp::Lt] {
                let cond = Expr::relation(op, s.clone(), Expr::zero());
                let mut more = a.clone();
                match more.assume(&cond) {
                    Ok(()) => {}
                    Err(MathError::Contradiction(_)) => continue,
                    Err(_) => return Ok(None),
                }
                let Some(v) = split_cases(&more, depth + 1, f)? else {
                    return Ok(None);
                };
                let cond = readable_condition(&cond);
                match cases.iter_mut().find(|(w, _)| *w == v) {
                    Some((_, c)) => *c = merge(c, &cond),
                    None => cases.push((v, cond)),
                }
            }
            Ok(Some(match cases.len() {
                0 => return Ok(None),
                1 => cases.swap_remove(0).0,
                _ => {
                    let (last, _) = cases.pop().unwrap_or((Expr::undefined(), Expr::one()));
                    Expr::new(ExprNode::Piecewise {
                        cases,
                        otherwise: Some(last),
                    })
                }
            }))
        }
    }
}

/// `1 − p > 0` read as `p < 1` when the condition is linear in one name.
fn readable_condition(cond: &Expr) -> Expr {
    let ExprNode::Relation(op, l, r) = cond.node() else {
        return cond.clone();
    };
    let d = (l - r).simplify();
    let syms = d.free_symbols();
    let [name] = syms.iter().collect::<Vec<_>>()[..] else {
        return cond.clone();
    };
    let Some(c) = d.as_poly(name).filter(|c| c.len() == 2) else {
        return cond.clone();
    };
    let flip = util::sign(&c[1]) == Some(Ordering::Less);
    let point = (-c[0].clone() / c[1].clone()).simplify();
    let op = if flip { flipped(*op) } else { *op };
    Expr::relation(op, Expr::sym(name), point)
}

/// Joins two conditions on one name (`p > 1` with `p = 1` gives `p ≥ 1`).
fn merge(a: &Expr, b: &Expr) -> Expr {
    if let (ExprNode::Relation(o1, l1, r1), ExprNode::Relation(o2, l2, r2)) = (a.node(), b.node()) {
        if l1 == l2 && r1 == r2 {
            let joined = match (o1, o2) {
                (RelOp::Gt, RelOp::Eq) | (RelOp::Eq, RelOp::Gt) => Some(RelOp::Ge),
                (RelOp::Lt, RelOp::Eq) | (RelOp::Eq, RelOp::Lt) => Some(RelOp::Le),
                (RelOp::Gt, RelOp::Lt) | (RelOp::Lt, RelOp::Gt) => Some(RelOp::Ne),
                _ => None,
            };
            if let Some(op) = joined {
                return Expr::relation(op, l1.clone(), r1.clone());
            }
        }
    }
    Expr::new(ExprNode::Logic(LogicOp::Or, vec![a.clone(), b.clone()]))
}

/// A limit value at +∞.
#[derive(Clone, Debug, PartialEq)]
enum Lim {
    Finite(Expr),
    PosInf,
    NegInf,
}

impl Lim {
    fn to_expr(&self) -> Expr {
        match self {
            Lim::Finite(v) => v.clone(),
            Lim::PosInf => Expr::infinity(),
            Lim::NegInf => Expr::neg_infinity(),
        }
    }

    fn negate(self) -> Lim {
        match self {
            Lim::Finite(v) => Lim::Finite((-v).simplify()),
            Lim::PosInf => Lim::NegInf,
            Lim::NegInf => Lim::PosInf,
        }
    }
}

enum Need {
    /// The answer depends on the sign of this expression.
    Sign(Expr),
    Unsupported,
}

/// The sign of `s` from the facts, or a request for it.
fn sign_of(s: &Expr, a: &Assumptions) -> Result<Ordering, Need> {
    let s = s.simplify();
    if s.is_constant() {
        return util::sign(&s).ok_or(Need::Unsupported);
    }
    match a.signs(&s).and_then(Signs::exact) {
        Some(o) => Ok(o),
        None => Err(Need::Sign(s)),
    }
}

/// `lim_{var→+∞} e`.
fn lim(e: &Expr, var: &str, a: &Assumptions) -> Result<Lim, Need> {
    if !e.contains_sym(var) {
        return Ok(Lim::Finite(e.clone()));
    }
    if e.is_sym(var) {
        return Ok(Lim::PosInf);
    }
    let free_params = e.free_symbols().iter().any(|s| &**s != var);
    match e.node() {
        ExprNode::Add(terms) => {
            let mut finite = Vec::new();
            let mut inf: Option<Lim> = None;
            for t in terms {
                match lim(t, var, a)? {
                    Lim::Finite(v) => finite.push(v),
                    l => match &inf {
                        None => inf = Some(l),
                        Some(prev) if *prev == l => {}
                        Some(_) => return Err(Need::Unsupported),
                    },
                }
            }
            Ok(inf.unwrap_or_else(|| Lim::Finite(add(finite).simplify())))
        }
        ExprNode::Mul(factors) => {
            let (free, bound): (Vec<&Expr>, Vec<&Expr>) =
                factors.iter().partition(|f| !f.contains_sym(var));
            let c = mul(free.into_iter().cloned().collect());
            let mut acc = Lim::Finite(Expr::one());
            for f in bound {
                acc = times(acc, lim(f, var, a)?)?;
            }
            match acc {
                Lim::Finite(v) => Ok(Lim::Finite((c * v).simplify())),
                inf => match sign_of(&c, a)? {
                    Ordering::Greater => Ok(inf),
                    Ordering::Less => Ok(inf.negate()),
                    // c = 0: the product is 0 for every x.
                    Ordering::Equal => Ok(Lim::Finite(Expr::zero())),
                },
            }
        }
        ExprNode::Pow(b, k) if !k.contains_sym(var) => match lim(b, var, a)? {
            Lim::PosInf => match sign_of(k, a)? {
                Ordering::Greater => Ok(Lim::PosInf),
                Ordering::Less => Ok(Lim::Finite(Expr::zero())),
                Ordering::Equal => Ok(Lim::Finite(Expr::one())),
            },
            Lim::NegInf => {
                if a.is_integer(k) {
                    let n = k.as_number().and_then(|n| n.as_integer());
                    match (sign_of(k, a)?, n) {
                        (Ordering::Less, _) => Ok(Lim::Finite(Expr::zero())),
                        (Ordering::Equal, _) => Ok(Lim::Finite(Expr::one())),
                        (Ordering::Greater, Some(n)) if (&n % 2u32) == 0u32.into() => {
                            Ok(Lim::PosInf)
                        }
                        (Ordering::Greater, Some(_)) => Ok(Lim::NegInf),
                        _ => Err(Need::Unsupported),
                    }
                } else {
                    Err(Need::Unsupported)
                }
            }
            Lim::Finite(v) => Ok(Lim::Finite(v.pow(k.clone()).simplify())),
        },
        ExprNode::Pow(b, g) if !b.contains_sym(var) => {
            // c^g(x) for c > 0: through e^(g ln c).
            let ln_c = if matches!(b.node(), ExprNode::Const(Constant::E)) {
                Expr::one()
            } else {
                match sign_of(b, a)? {
                    Ordering::Greater => {
                        Expr::func(Func::Ln, vec![b.clone()]).map_err(|_| Need::Unsupported)?
                    }
                    _ => return Err(Need::Unsupported),
                }
            };
            let exponent = (g.clone() * ln_c).simplify();
            match lim(&exponent, var, a)? {
                Lim::PosInf => Ok(Lim::PosInf),
                Lim::NegInf => Ok(Lim::Finite(Expr::zero())),
                Lim::Finite(v) => Ok(Lim::Finite(Expr::e().pow(v).simplify())),
            }
        }
        ExprNode::Func(Func::Ln, args) => match args.as_slice() {
            [x] => match lim(x, var, a)? {
                Lim::PosInf => Ok(Lim::PosInf),
                Lim::Finite(v) => match sign_of(&v, a)? {
                    Ordering::Greater => Ok(Lim::Finite(
                        Expr::func(Func::Ln, vec![v]).map_err(|_| Need::Unsupported)?,
                    )),
                    Ordering::Equal => Ok(Lim::NegInf),
                    Ordering::Less => Err(Need::Unsupported),
                },
                Lim::NegInf => Err(Need::Unsupported),
            },
            _ => Err(Need::Unsupported),
        },
        ExprNode::Func(Func::Abs, args) => match args.as_slice() {
            [x] => match lim(x, var, a)? {
                Lim::PosInf | Lim::NegInf => Ok(Lim::PosInf),
                Lim::Finite(v) => Ok(Lim::Finite(
                    Expr::func(Func::Abs, vec![v]).map_err(|_| Need::Unsupported)?,
                )),
            },
            _ => Err(Need::Unsupported),
        },
        _ if !free_params => {
            // No parameters: the core's own limit.
            match crate::calc::limit(e, var, &Expr::infinity(), crate::expr::LimitDir::Both, None) {
                Ok(v) if v.is_infinite() => {
                    if util::sign(&v) == Some(Ordering::Less) {
                        Ok(Lim::NegInf)
                    } else {
                        Ok(Lim::PosInf)
                    }
                }
                Ok(v) => Ok(Lim::Finite(v)),
                Err(_) => Err(Need::Unsupported),
            }
        }
        _ => Err(Need::Unsupported),
    }
}

fn times(a: Lim, b: Lim) -> Result<Lim, Need> {
    Ok(match (a, b) {
        (Lim::Finite(x), Lim::Finite(y)) => Lim::Finite((x * y).simplify()),
        (Lim::Finite(x), inf) | (inf, Lim::Finite(x)) => match util::sign(&x) {
            Some(Ordering::Greater) => inf,
            Some(Ordering::Less) => inf.negate(),
            _ => return Err(Need::Unsupported),
        },
        (Lim::PosInf, Lim::PosInf) | (Lim::NegInf, Lim::NegInf) => Lim::PosInf,
        _ => Lim::NegInf,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn x() -> Expr {
        Expr::sym("x")
    }

    fn assume(conds: &[Expr]) -> Assumptions {
        let mut a = Assumptions::new();
        for c in conds {
            a.assume(c).unwrap();
        }
        a
    }

    fn gt(name: &str, v: i64) -> Expr {
        Expr::relation(RelOp::Gt, Expr::sym(name), Expr::int(v))
    }

    #[test]
    fn facts_and_contradictions() {
        let a = assume(&[gt("x", 0)]);
        assert!(a.signs(&x()).unwrap().positive());
        assert!(a.signs(&(x().pow(3) + Expr::int(2))).unwrap().positive());
        let mut b = Assumptions::new();
        b.assume(&gt("x", 0)).unwrap();
        let err = b
            .assume(&Expr::relation(RelOp::Lt, x(), Expr::int(0)))
            .unwrap_err();
        assert!(matches!(err, MathError::Contradiction(_)), "{err}");
        let mut c = Assumptions::new();
        c.assume(&Expr::apply("in", vec![Expr::sym("n"), Expr::sym("ℤ")]))
            .unwrap();
        c.assume(&gt("n", 0)).unwrap();
        assert!(c
            .assume(&Expr::relation(RelOp::Lt, Expr::sym("n"), Expr::int(1)))
            .is_err());
        // 2x − 1 ≥ 0 is x ≥ 1/2.
        let d = assume(&[Expr::relation(
            RelOp::Ge,
            Expr::int(2) * x() - Expr::one(),
            Expr::zero(),
        )]);
        assert!(d.signs(&x()).unwrap().positive());
        let e = assume(&[Expr::relation(RelOp::Ne, Expr::sym("a"), Expr::zero())]);
        assert!(e.signs(&Expr::sym("a")).unwrap().nonzero());
    }

    #[test]
    fn simplifications() {
        let s = |e: Expr, a: &Assumptions| simplify_with(&e, a, None).to_string();
        let none = Assumptions::new();
        let pos = assume(&[gt("x", 0)]);
        let neg = assume(&[Expr::relation(RelOp::Lt, x(), Expr::zero())]);
        assert_eq!(s(x().pow(2).sqrt(), &none), "|x|");
        assert_eq!(s(x().pow(2).sqrt(), &pos), "x");
        assert_eq!(s(x().pow(2).sqrt(), &neg), "-x");
        let abs = Expr::func(Func::Abs, vec![x()]).unwrap();
        assert_eq!(s(abs.clone(), &pos), "x");
        assert_eq!(s(abs, &none), "|x|");
        let ln = Expr::func(Func::Ln, vec![Expr::e().pow(x())]).unwrap();
        assert_eq!(s(ln.clone(), &none), "ln(e^x)");
        assert_eq!(s(ln, &pos), "x");
        let real = assume(&[
            gt("x", 0),
            Expr::apply("in", vec![Expr::sym("a"), Expr::sym("ℝ")]),
            Expr::apply("in", vec![Expr::sym("b"), Expr::sym("ℝ")]),
        ]);
        let pp = x().pow(Expr::sym("a")).pow(Expr::sym("b"));
        assert_eq!(s(pp.clone(), &none), "(x^a)^b");
        assert_eq!(s(pp, &real), "x^(a*b)");
        // A logged reason for the safe form.
        let mut steps = Steps::new();
        let _ = simplify_with(&x().pow(2).sqrt(), &none, Some(&mut steps));
        assert!(steps.0.iter().any(|st| st
            .note
            .as_deref()
            .is_some_and(|n| n.contains("may be negative"))));
    }

    #[test]
    fn power_integrals_split_on_minus_one() {
        let n = Expr::sym("n");
        let e = x().pow(n.clone());
        let split = integrate_power(&e, "x", &Assumptions::new()).unwrap();
        assert!(matches!(split.node(), ExprNode::Piecewise { .. }));
        let ne = assume(&[Expr::relation(RelOp::Ne, n.clone(), Expr::int(-1))]);
        assert_eq!(
            integrate_power(&e, "x", &ne).unwrap().to_string(),
            "x^(n + 1)/(n + 1)"
        );
        let eq = assume(&[Expr::relation(RelOp::Eq, n, Expr::int(-1))]);
        assert_eq!(
            integrate_power(&e, "x", &eq).unwrap().to_string(),
            "ln(|x|)"
        );
    }

    #[test]
    fn limits_that_depend_on_a_sign() {
        let a = Expr::sym("a");
        let e = Expr::e().pow(a.clone() * x());
        let neg = assume(&[Expr::relation(RelOp::Lt, a.clone(), Expr::zero())]);
        assert_eq!(
            limit_at_infinity(&e, "x", false, &neg).unwrap(),
            Some(Expr::zero())
        );
        let split = limit_at_infinity(&e, "x", false, &Assumptions::new())
            .unwrap()
            .unwrap();
        assert!(
            matches!(split.node(), ExprNode::Piecewise { .. }),
            "{split}"
        );
        // ∫_1^∞ x^(−p) dx = 1/(p − 1) for p > 1.
        let p = Expr::sym("p");
        let f = x().pow(-p.clone());
        let big = assume(&[gt("p", 1)]);
        let v = improper_integral(&f, "x", &Expr::one(), &Expr::infinity(), &big)
            .unwrap()
            .unwrap();
        // 1/(p − 1), in whatever form the simplifier keeps.
        assert_eq!(v.subs("p", &Expr::int(3)).simplify(), Expr::rational(1, 2));
        let small = assume(&[Expr::relation(RelOp::Le, p, Expr::one())]);
        let v = improper_integral(&f, "x", &Expr::one(), &Expr::infinity(), &small)
            .unwrap()
            .unwrap();
        assert_eq!(v, Expr::infinity());
    }
}
