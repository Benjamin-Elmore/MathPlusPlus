//! The real line machinery behind the analysis: exact real zeros (with a numeric fallback),
//! the points where a function may stop being defined, sample points between them, and
//! intervals built from them.

use std::cmp::Ordering;

use crate::domains::sets::Piece;
use crate::domains::solve::{self, numeric, Range, Solutions};
use crate::domains::util;
use crate::error::{MathError, MathResult};
use crate::expr::{Expr, ExprNode};
use crate::functions::{func, Func};
use crate::simplify::simplify;

/// Real zeros of an expression.
#[derive(Clone, Debug, Default)]
pub(crate) struct Zeros {
    /// Sorted, without repeats.
    pub values: Vec<Expr>,
    /// Periodic families `a + b·k` (only when no bounded interval was given).
    pub families: Vec<Expr>,
    /// The integer parameter of the families.
    pub param: Option<String>,
    /// The expression is zero everywhere.
    pub everywhere: bool,
    /// False when some values were found numerically.
    pub exact: bool,
}

impl Zeros {
    fn exact() -> Zeros {
        Zeros {
            exact: true,
            ..Zeros::default()
        }
    }

    /// True when there are periodic families.
    pub fn periodic(&self) -> bool {
        !self.families.is_empty()
    }

    /// Adds `other` (values merged and sorted).
    pub fn merge(&mut self, other: Zeros) {
        self.exact &= other.exact;
        self.everywhere |= other.everywhere;
        for v in other.values {
            push_unique(&mut self.values, v);
        }
        for f in other.families {
            if !self.families.contains(&f) {
                self.families.push(f);
            }
        }
        if self.param.is_none() {
            self.param = other.param;
        }
        sort_values(&mut self.values);
    }
}

/// The value of a constant expression as `f64` (±∞ allowed), for ordering.
pub(crate) fn num(e: &Expr) -> Option<f64> {
    if e.is_infinite() {
        return Some(if matches!(e.node(), ExprNode::Const(_)) {
            f64::INFINITY
        } else {
            f64::NEG_INFINITY
        });
    }
    util::numeric(e)
}

/// Sorts constant expressions by value.
pub(crate) fn sort_values(v: &mut [Expr]) {
    v.sort_by(|a, b| {
        num(a)
            .unwrap_or(f64::NAN)
            .total_cmp(&num(b).unwrap_or(f64::NAN))
    });
}

/// Appends `v` unless an equal value is already there.
pub(crate) fn push_unique(list: &mut Vec<Expr>, v: Expr) {
    if !list.iter().any(|o| same_value(o, &v)) {
        list.push(v);
    }
}

/// Equal constants (exactly, or to rounding for numeric ones).
pub(crate) fn same_value(a: &Expr, b: &Expr) -> bool {
    if a == b {
        return true;
    }
    match (num(a), num(b)) {
        (Some(x), Some(y)) if x.is_finite() && y.is_finite() => {
            let close = (x - y).abs() <= 1e-9 * x.abs().max(y.abs()).max(1.0);
            close && (!is_exact(a) || !is_exact(b) || util::equal(a, b))
        }
        (Some(x), Some(y)) => x == y,
        _ => false,
    }
}

/// True when `e` holds no floating-point number.
pub(crate) fn is_exact(e: &Expr) -> bool {
    match e.node() {
        ExprNode::Num(n) => n.is_exact(),
        _ => e.children().into_iter().all(is_exact),
    }
}

/// Compares two constants (exactly when possible).
pub(crate) fn cmp_values(a: &Expr, b: &Expr) -> Option<Ordering> {
    if same_value(a, b) {
        return Some(Ordering::Equal);
    }
    util::compare(a, b).or_else(|| num(a)?.partial_cmp(&num(b)?))
}

/// `f(var = x)` simplified, when it is a real finite number.
pub(crate) fn value_at(f: &Expr, var: &str, x: &Expr) -> Option<Expr> {
    let v = simplify(&f.subs(var, x));
    if v.is_undefined() || v.is_infinite() {
        return None;
    }
    num(&v).filter(|n| n.is_finite()).map(|_| v)
}

/// A derivative's value at `x`: like [`value_at`], but undefined where a `sign(u)` in it
/// has `u = 0` (the derivative of `|u|` does not exist there).
pub(crate) fn slope_at(d: &Expr, var: &str, x: &Expr) -> Option<Expr> {
    fn sign_breaks(e: &Expr, var: &str, x: &Expr) -> bool {
        if let ExprNode::Func(Func::Sign, args) = e.node() {
            if args
                .first()
                .is_some_and(|u| value_at(u, var, x).is_some_and(|v| util::is_zero(&v)))
            {
                return true;
            }
        }
        e.children().into_iter().any(|c| sign_breaks(c, var, x))
    }
    if sign_breaks(d, var, x) {
        return None;
    }
    value_at(d, var, x)
}

/// `f(x)` in `f64`, `None` where `f` is undefined or not real.
pub(crate) fn eval(f: &Expr, var: &str, x: f64) -> Option<f64> {
    numeric::eval_at(f, var, x)
}

/// A point strictly between `a` and `b` (either may be infinite), away from both.
pub(crate) fn sample_between(a: f64, b: f64) -> f64 {
    match (a.is_finite(), b.is_finite()) {
        (true, true) => a + (b - a) / 2.0,
        (false, true) => b - b.abs().max(1.0),
        (true, false) => a + a.abs().max(1.0),
        (false, false) => 0.5,
    }
}

/// An open interval `(lo, hi)`.
pub(crate) fn open(lo: Expr, hi: Expr) -> Piece {
    Piece {
        lo,
        hi,
        lo_closed: false,
        hi_closed: false,
    }
}

/// The single point `{v}` as a piece.
pub(crate) fn point(v: Expr) -> Piece {
    Piece {
        lo: v.clone(),
        hi: v,
        lo_closed: true,
        hi_closed: true,
    }
}

/// The piece as a solver range.
pub(crate) fn range_of(p: &Piece) -> Range {
    Range {
        lo: p.lo.clone(),
        hi: p.hi.clone(),
        lo_closed: p.lo_closed,
        hi_closed: p.hi_closed,
    }
}

/// True when `x` lies strictly inside `p`.
pub(crate) fn inside(p: &Piece, x: &Expr) -> bool {
    cmp_values(x, &p.lo) == Some(Ordering::Greater) && cmp_values(x, &p.hi) == Some(Ordering::Less)
}

/// True when `x` lies in `p` (closed ends included).
pub(crate) fn within(p: &Piece, x: &Expr) -> bool {
    let lo = match cmp_values(x, &p.lo) {
        Some(Ordering::Greater) => true,
        Some(Ordering::Equal) => p.lo_closed,
        _ => false,
    };
    let hi = match cmp_values(x, &p.hi) {
        Some(Ordering::Less) => true,
        Some(Ordering::Equal) => p.hi_closed,
        _ => false,
    };
    lo && hi
}

/// Real zeros of `g` in `var`, inside `range` when given.
pub(crate) fn zeros(
    g: &Expr,
    var: &str,
    range: Option<&Range>,
    notes: &mut Vec<String>,
) -> MathResult<Zeros> {
    zeros_at(g, var, range, notes, 0)
}

/// Most nested factor splits tried by [`zeros`].
const MAX_SPLIT: u32 = 3;

fn zeros_at(
    g: &Expr,
    var: &str,
    range: Option<&Range>,
    notes: &mut Vec<String>,
    depth: u32,
) -> MathResult<Zeros> {
    let g = simplify(g);
    if !g.contains_sym(var) {
        let mut z = Zeros::exact();
        z.everywhere = util::is_zero(&g);
        return Ok(z);
    }
    // Pull a factor common to every term out of a sum: x e^{-x} − 2e^{-x} = e^{-x}(x − 2).
    let g = match common_factor(&g).filter(|_| depth < MAX_SPLIT) {
        Some((c, rest)) => Expr::new(ExprNode::Mul(vec![c, rest])),
        None => g,
    };
    // A product is zero where one of its factors is (and the product is defined);
    // exponentials and other never-zero factors are dropped first.
    if let (ExprNode::Mul(factors), true) = (g.node(), depth < MAX_SPLIT) {
        let live: Vec<&Expr> = factors
            .iter()
            .filter(|f| f.contains_sym(var) && !never_zero(f))
            .collect();
        if live.len() != factors.len() || live.len() > 1 {
            let mut all = Zeros::exact();
            for f in live {
                let z = zeros_at(f, var, range, notes, depth + 1)?;
                if z.everywhere {
                    return Ok(z);
                }
                all.merge(z);
            }
            all.values.retain(|v| {
                value_at(&g, var, v).is_some_and(|y| util::is_zero(&y) || !is_exact(&y))
            });
            return Ok(all);
        }
    }
    let mut ctx = crate::domains::Ctx::new();
    let found = solve::solve(&g, var, range, &mut ctx);
    let mut z = Zeros::exact();
    match found {
        Ok(Solutions::Finite { values, exact }) => {
            z.exact = exact;
            for v in values {
                let v = simplify(&v);
                let real = num(&v).is_some_and(f64::is_finite);
                let ok = match range {
                    Some(r) => r.contains(&v),
                    None => true,
                };
                if real && ok {
                    push_unique(&mut z.values, v);
                }
            }
        }
        Ok(Solutions::Periodic { families, param }) => {
            z.families = families
                .into_iter()
                .filter(|f| {
                    let at0 = simplify(&f.subs(&param, &Expr::zero()));
                    num(&at0).is_some_and(f64::is_finite)
                })
                .collect();
            z.param = Some(param);
        }
        Ok(Solutions::All) => z.everywhere = true,
        Err(MathError::TooLarge(_)) => {
            return Err(MathError::TooLarge(format!(
                "the list of solutions of {g} = 0 (give a bounded interval)"
            )))
        }
        Err(_) => {
            let (lo, hi) = match range {
                Some(r) => (
                    num(&r.lo).unwrap_or(solve::DEFAULT_SCAN.0).max(-1e6),
                    num(&r.hi).unwrap_or(solve::DEFAULT_SCAN.1).min(1e6),
                ),
                None => solve::DEFAULT_SCAN,
            };
            let roots = numeric::roots_in(&g, var, lo, hi)?;
            z.exact = false;
            for r in roots {
                push_unique(&mut z.values, Expr::float(r));
            }
            let note = format!(
                "the solutions of {g} = 0 were found numerically (≈), searching [{lo}, {hi}]"
            );
            if !notes.contains(&note) {
                notes.push(note);
            }
        }
    }
    if !z.exact {
        let note = format!("some solutions of {g} = 0 are numerical (≈)");
        if !notes.iter().any(|n| n.contains(&format!("{g} = 0"))) {
            notes.push(note);
        }
    }
    sort_values(&mut z.values);
    Ok(z)
}

/// `(c, rest)` with `e = c·rest` when every term of the sum `e` has the factor `c`.
fn common_factor(e: &Expr) -> Option<(Expr, Expr)> {
    let ExprNode::Add(terms) = e.node() else {
        return None;
    };
    let factors_of = |t: &Expr| -> Vec<Expr> {
        match t.node() {
            ExprNode::Mul(fs) => fs.iter().filter(|f| !f.is_number()).cloned().collect(),
            _ => vec![t.clone()],
        }
    };
    let mut common = factors_of(terms.first()?);
    for t in &terms[1..] {
        let fs = factors_of(t);
        common.retain(|c| fs.contains(c));
    }
    if common.is_empty() {
        return None;
    }
    // Divide each term by the common factors (taking them off its factor list).
    let rest_terms: Vec<Expr> = terms
        .iter()
        .map(|t| {
            let mut fs: Vec<Expr> = match t.node() {
                ExprNode::Mul(fs) => fs.clone(),
                _ => vec![t.clone()],
            };
            for c in &common {
                if let Some(i) = fs.iter().position(|f| f == c) {
                    fs.remove(i);
                }
            }
            crate::simplify::mul(fs)
        })
        .collect();
    let rest = crate::simplify::add(rest_terms);
    Some((crate::simplify::mul(common), rest))
}

/// True for factors that are never zero: `e^u`, `a^u` with `a > 0`, and `(positive)^k`.
fn never_zero(f: &Expr) -> bool {
    match f.node() {
        ExprNode::Pow(b, x) => {
            (b.is_constant() && num(b).is_some_and(|v| v > 0.0) && !x.is_constant())
                || (x.as_number().is_some_and(|n| n.is_negative()) && never_zero(b))
                || never_zero(b)
        }
        ExprNode::Add(terms) => {
            // c + (even powers and squares): positive when c > 0 and every other term ≥ 0.
            let mut has_positive_const = false;
            for t in terms {
                if t.is_constant() {
                    has_positive_const |= num(t).is_some_and(|v| v > 0.0);
                } else if !is_square(t) {
                    return false;
                }
            }
            has_positive_const
        }
        _ => f.is_constant() && num(f).is_some_and(|v| v != 0.0),
    }
}

/// `u^(2k)` or a positive multiple of one.
fn is_square(t: &Expr) -> bool {
    match t.node() {
        ExprNode::Pow(_, x) => x
            .as_number()
            .and_then(|n| n.as_rational())
            .is_some_and(|r| r.is_integer() && r.numer() % 2 == 0.into() && r.numer() > &0.into()),
        ExprNode::Mul(fs) => {
            let (c, rest): (Vec<&Expr>, Vec<&Expr>) = fs.iter().partition(|f| f.is_constant());
            c.iter().all(|c| num(c).is_some_and(|v| v > 0.0))
                && rest.len() == 1
                && is_square(rest[0])
        }
        _ => false,
    }
}

/// The expressions whose zeros are where `f` may stop being defined or smooth: bases of
/// negative and fractional powers, logarithm arguments, the cosines and sines behind
/// `tan`, `sec`, `cot` and `csc`, the edges of `arcsin`/`arccos`, and `|u|`.
pub(crate) fn trouble(f: &Expr, var: &str, out: &mut Vec<Expr>) -> MathResult<()> {
    if !f.contains_sym(var) {
        return Ok(());
    }
    match f.node() {
        ExprNode::Pow(b, x) => {
            if let Some(n) = x.as_number() {
                let fractional = n.as_rational().is_some_and(|r| !r.is_integer()) || !n.is_exact();
                if n.is_negative() || fractional {
                    out.push(b.clone());
                }
            } else if b.contains_sym(var) {
                // b^g(x): defined for b > 0.
                out.push(b.clone());
            }
            trouble(b, var, out)?;
            trouble(x, var, out)
        }
        ExprNode::Func(fun, args) => {
            for a in args {
                trouble(a, var, out)?;
            }
            let Some(u) = args.first() else {
                return Ok(());
            };
            let one = Expr::one;
            match fun {
                Func::Ln | Func::Log => out.extend(args.iter().cloned()),
                Func::Tan | Func::Sec => out.push(func(Func::Cos, vec![u.clone()])),
                Func::Cot | Func::Csc => out.push(func(Func::Sin, vec![u.clone()])),
                Func::Asin | Func::Acos | Func::Atanh => {
                    out.push(u.clone() - one());
                    out.push(u.clone() + one());
                }
                Func::Acosh => out.push(u.clone() - one()),
                Func::Abs | Func::Sign => out.push(u.clone()),
                Func::Sin
                | Func::Cos
                | Func::Atan
                | Func::Sinh
                | Func::Cosh
                | Func::Tanh
                | Func::Asinh => {}
                other => {
                    return Err(MathError::Unsupported(format!(
                        "analysing a function built from {}",
                        other.name()
                    )))
                }
            }
            Ok(())
        }
        ExprNode::Add(items) | ExprNode::Mul(items) => {
            for i in items {
                trouble(i, var, out)?;
            }
            Ok(())
        }
        ExprNode::Sym(_) | ExprNode::Num(_) | ExprNode::Const(_) => Ok(()),
        _ => Err(MathError::Unsupported(format!(
            "analysing {f} (only elementary functions of one variable)"
        ))),
    }
}
