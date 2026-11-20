//! Products `Π_{k=a}^{b} t(k)`: finite ones exactly (factor by factor for small concrete
//! ranges, otherwise through factorials, Γ and telescoping of rational factors), infinite ones
//! after a convergence check, exactly for rational factors (Γ at the poles, which gives
//! Wallis's product) and otherwise numerically as `exp Σ ln t`.

use num_rational::BigRational;
use num_traits::{One, Signed, ToPrimitive, Zero};

use super::antidiff::pole_groups;
use super::convergence::{convergence, eventual_sign};
use super::rational::{as_ratfun, is_nonpositive_integer};
use super::{
    check_bounds, holder, int_value, is_pos_infinity, sum, term_at, term_f64, tidy, value_f64,
};
use crate::calc::limit::limit;
use crate::error::{MathError, MathResult};
use crate::expr::{BigOpKind, Expr, ExprNode, LimitDir};
use crate::functions::{func, Func};
use crate::poly::Poly;
use crate::simplify::{add, mul, pow, simplify};
use crate::steps::Steps;

/// Most factors multiplied one by one in exact arithmetic.
pub const MAX_DIRECT_FACTORS: u64 = 1_000;

/// `Π_{index=from}^{to} term`.
///
/// `to` may be `∞`. A finite product is exact. An infinite product converges (to a non-zero
/// value) only when its factors tend to 1 and `Σ (t − 1)` converges; otherwise it is a
/// [`MathError::Divergent`].
pub fn product(
    term: &Expr,
    index: &str,
    from: &Expr,
    to: &Expr,
    steps: Option<&mut Steps>,
) -> MathResult<Expr> {
    let mut log = steps;
    let t = simplify(term);
    let (a, b) = (simplify(from), simplify(to));
    check_bounds(&t, index, &a, &b)?;
    if is_pos_infinity(&b) {
        return infinite_product(&t, index, &a, &mut log);
    }
    let shown = holder(BigOpKind::Product, &t, index, &a, &b);
    if let Some((lo, hi)) = int_value(&a).zip(int_value(&b)) {
        if hi < lo {
            Steps::record(&mut log, "empty product", &shown, &Expr::one(), None);
            return Ok(Expr::one());
        }
        let count = (&hi - &lo + 1u32).to_u64().unwrap_or(u64::MAX);
        if count <= MAX_DIRECT_FACTORS {
            let mut factors = Vec::with_capacity(count as usize);
            let mut n = lo.clone();
            while n <= hi {
                factors.push(term_at(&t, index, &n)?);
                n += 1u32;
            }
            let value = simplify(&mul(factors));
            Steps::record(
                &mut log,
                "direct multiplication",
                &shown,
                &value,
                Some(format!("{count} factors multiplied")),
            );
            return Ok(value);
        }
    }
    match finite_closed(&t, index, &a, &b, 0)? {
        Some((value, rule)) => {
            let value = tidy(&value);
            Steps::record(&mut log, rule, &shown, &value, None);
            Ok(value)
        }
        None => Err(MathError::NoClosedForm(format!("{shown}"))),
    }
}

/// Linear factors of a polynomial over Q: leading coefficient and `(root, multiplicity)`.
fn linear_factors(p: &Poly) -> Option<(BigRational, Vec<(BigRational, i64)>)> {
    let lc = p.leading();
    let mut out = Vec::new();
    let mut count = 0usize;
    for (part, mult) in p.square_free() {
        let roots = part.rational_roots()?;
        if roots.len() != part.degree().unwrap_or(0) {
            return None;
        }
        for r in roots {
            out.push((r, i64::from(mult)));
            count += mult as usize;
        }
    }
    (count == p.degree().unwrap_or(0)).then_some((lc, out))
}

/// `t = c · Π (k − rᵢ)^{eᵢ}` for a rational function with rational roots and poles.
fn rational_factors(t: &Expr, k: &str) -> Option<(BigRational, Vec<(BigRational, BigRational)>)> {
    let r = as_ratfun(t, k)?;
    let (lc_n, num) = linear_factors(&r.num)?;
    let (lc_d, den) = linear_factors(&r.den)?;
    if num.is_empty() && den.is_empty() {
        return None;
    }
    let mut roots: Vec<(BigRational, BigRational)> = Vec::new();
    for (root, e) in num {
        roots.push((root, BigRational::from_integer(e.into())));
    }
    for (root, e) in den {
        roots.push((root, BigRational::from_integer((-e).into())));
    }
    Some((lc_n / lc_d, roots))
}

fn q(e: &BigRational) -> Expr {
    Expr::ratio(e.clone())
}

/// `Γ(x + 1 − ρ … )`-style block: `Γ(u)` written as a factorial when `u` is `n + 1`.
fn gamma_of(u: Expr) -> Expr {
    func(Func::Gamma, vec![u])
}

/// `Π_{k=a}^{b} c · Π (k − rᵢ)^{eᵢ}` with integer-spaced roots telescoping.
fn rational_finite(t: &Expr, k: &str, a: &Expr, b: &Expr) -> Option<Expr> {
    let (lc, roots) = rational_factors(t, k)?;
    let count = add(vec![
        b.clone(),
        mul(vec![Expr::int(-1), a.clone()]),
        Expr::one(),
    ]);
    let mut parts = vec![pow(q(&lc), count)];
    for (rho, members) in pole_groups(&roots)? {
        let total: BigRational = members.iter().map(|(_, e)| e.clone()).sum();
        let upper = add(vec![b.clone(), Expr::one(), q(&-rho.clone())]);
        let lower = add(vec![a.clone(), q(&-rho.clone())]);
        if !total.is_zero() {
            let ratio = if rho.is_integer() {
                // Γ(b + 1 − ρ)/Γ(a − ρ) = (b − ρ)!/(a − ρ − 1)!
                let top = func(
                    Func::Factorial,
                    vec![add(vec![upper.clone(), Expr::int(-1)])],
                );
                let bottom = func(
                    Func::Factorial,
                    vec![add(vec![lower.clone(), Expr::int(-1)])],
                );
                mul(vec![top, pow(bottom, Expr::int(-1))])
            } else {
                mul(vec![
                    gamma_of(upper.clone()),
                    pow(gamma_of(lower.clone()), Expr::int(-1)),
                ])
            };
            if ratio.is_undefined() {
                return None;
            }
            parts.push(pow(ratio, q(&total)));
        }
        for (m, e) in members {
            for j in 0..m {
                let top = add(vec![upper.clone(), Expr::int(j)]);
                let bottom = add(vec![lower.clone(), Expr::int(j)]);
                if bottom.is_zero() {
                    return None;
                }
                parts.push(pow(mul(vec![top, pow(bottom, Expr::int(-1))]), q(&e)));
            }
        }
    }
    let v = simplify(&mul(parts));
    (!v.is_undefined()).then_some(v)
}

fn finite_closed(
    t: &Expr,
    k: &str,
    a: &Expr,
    b: &Expr,
    depth: u32,
) -> MathResult<Option<(Expr, &'static str)>> {
    if depth > 6 {
        return Ok(None);
    }
    let count = add(vec![
        b.clone(),
        mul(vec![Expr::int(-1), a.clone()]),
        Expr::one(),
    ]);
    if !t.contains_sym(k) {
        return Ok(Some((pow(t.clone(), count), "product of a constant")));
    }
    if let ExprNode::Pow(base, x) = t.node() {
        if !base.contains_sym(k) {
            let s = sum(x, k, a, b, None)?;
            return Ok(Some((
                pow(base.clone(), s),
                "product of powers (sum the exponents)",
            )));
        }
    }
    if let Some(v) = rational_finite(t, k, a, b) {
        return Ok(Some((v, "factorials and telescoping of rational factors")));
    }
    if let ExprNode::Pow(base, x) = t.node() {
        if x.as_number().is_some_and(|n| n.is_integer()) {
            if let Some((inner, rule)) = finite_closed(base, k, a, b, depth + 1)? {
                return Ok(Some((pow(inner, x.clone()), rule)));
            }
        }
    }
    if let ExprNode::Mul(fs) = t.node() {
        let mut parts = Vec::with_capacity(fs.len());
        for f in fs {
            match finite_closed(f, k, a, b, depth + 1)? {
                Some((v, _)) => parts.push(v),
                None => return Ok(None),
            }
        }
        return Ok(Some((mul(parts), "product of the factors separately")));
    }
    Ok(None)
}

fn infinite_product(t: &Expr, k: &str, a: &Expr, log: &mut Option<&mut Steps>) -> MathResult<Expr> {
    let shown = holder(BigOpKind::Product, t, k, a, &Expr::infinity());
    let factor_limit = limit(t, k, &Expr::infinity(), LimitDir::Both, None).ok();
    if let Some(l) = &factor_limit {
        if !l.is_one() && (l.is_infinite() || value_f64(l).is_some()) {
            return Err(MathError::Divergent(format!(
                "the product {shown} (its factors tend to {l}, not 1)"
            )));
        }
    }
    let excess = simplify(&add(vec![t.clone(), Expr::int(-1)]));
    let verdict = convergence(&excess, k, a);
    if verdict.converges == Some(false) {
        return Err(MathError::Divergent(format!(
            "the product {shown} (Σ (aₖ − 1) diverges by the {})",
            verdict.test
        )));
    }
    let note = Some(format!("Σ (aₖ − 1) {verdict}"));
    if let Some((value, rule)) = infinite_closed(t, k, a, 0)? {
        Steps::record(log, verdict.test.clone(), &shown, &shown, note);
        Steps::record(log, rule, &shown, &value, None);
        return Ok(value);
    }
    let Some(start) = int_value(a).and_then(|n| n.to_f64()) else {
        return Err(MathError::NoClosedForm(format!("{shown}")));
    };
    if verdict.converges != Some(true) || t.free_symbols().iter().any(|s| &**s != k) {
        return Err(MathError::Inconclusive(format!(
            "whether {shown} converges"
        )));
    }
    // Every factor must be positive for exp Σ ln aₖ; check the first ones and the tail.
    let first_ok = (0..200).all(|i| term_f64(t, k, start + f64::from(i)).is_some_and(|v| v > 0.0));
    if !first_ok || eventual_sign(t, k) != Some(std::cmp::Ordering::Greater) {
        return Err(MathError::NoClosedForm(format!("{shown}")));
    }
    let ln_t = func(Func::Ln, vec![t.clone()]);
    let ln_verdict = convergence(&ln_t, k, a);
    let (s, method) = super::numeric::infinite(&ln_t, k, start, &ln_verdict.test)?;
    let out = Expr::float(s.exp());
    Steps::record(log, verdict.test.clone(), &shown, &shown, note);
    Steps::record(
        log,
        "numeric product (exp of Σ ln aₖ)",
        &shown,
        &out,
        Some(method.describe().into()),
    );
    Ok(out)
}

fn infinite_closed(
    t: &Expr,
    k: &str,
    a: &Expr,
    depth: u32,
) -> MathResult<Option<(Expr, &'static str)>> {
    if depth > 6 {
        return Ok(None);
    }
    if let ExprNode::Pow(base, x) = t.node() {
        if !base.contains_sym(k) {
            let s = sum(x, k, a, &Expr::infinity(), None)?;
            return Ok(Some((
                simplify(&pow(base.clone(), s)),
                "product of powers (sum the exponents)",
            )));
        }
    }
    if let Some(v) = rational_infinite(t, k, a)? {
        return Ok(Some((v, "Γ function at the roots and poles")));
    }
    if let ExprNode::Mul(fs) = t.node() {
        let mut parts = Vec::with_capacity(fs.len());
        for f in fs {
            match infinite_closed(f, k, a, depth + 1)? {
                Some((v, _)) => parts.push(v),
                None => return Ok(None),
            }
        }
        return Ok(Some((
            simplify(&mul(parts)),
            "product of the factors separately",
        )));
    }
    Ok(None)
}

/// `Π_{k≥a} Π (k − rᵢ)^{eᵢ} = Π Γ(a − rᵢ)^{−eᵢ}` when `Σ eᵢ = 0` and `Σ eᵢ rᵢ = 0`.
fn rational_infinite(t: &Expr, k: &str, a: &Expr) -> MathResult<Option<Expr>> {
    let Some(start) = int_value(a) else {
        return Ok(None);
    };
    let Some((lc, roots)) = rational_factors(t, k) else {
        return Ok(None);
    };
    let total: BigRational = roots.iter().map(|(_, e)| e.clone()).sum();
    let moment: BigRational = roots.iter().map(|(r, e)| r * e).sum();
    if !lc.is_one() || !total.is_zero() || !moment.is_zero() {
        return Ok(None);
    }
    let start = BigRational::from_integer(start);
    let mut parts = Vec::with_capacity(roots.len());
    for (r, e) in &roots {
        let x = &start - r;
        if is_nonpositive_integer(&x) {
            if e.is_positive() {
                return Ok(Some(Expr::zero()));
            }
            return Err(MathError::DivisionByZero);
        }
        parts.push(pow(gamma_of(q(&x)), q(&-e.clone())));
    }
    let v = simplify(&mul(parts));
    Ok((!v.is_undefined()).then_some(v))
}
