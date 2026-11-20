//! Canonical constructors and the simplifier.
//!
//! `add`, `mul` and `pow` assume their inputs are already canonical and return canonical
//! results; `simplify` rebuilds a whole tree through them.

use std::collections::BTreeMap;
use std::ops;

use num_bigint::BigInt;
use num_rational::BigRational;
use num_traits::{One, Signed, ToPrimitive, Zero};

use crate::eval;
use crate::expr::{Constant, Expr, ExprNode, LogicOp};
use crate::functions::{self, Func};
use crate::number::{rational_root_parts, Number};

/// Recursion guard for constructors that may call each other.
const MAX_DEPTH: u32 = 16;

/// Splits a term into its numeric coefficient and the rest: `3·x·y → (3, x·y)`.
pub fn split_coeff(term: &Expr) -> (Number, Expr) {
    match term.node() {
        ExprNode::Num(n) => (n.clone(), Expr::one()),
        ExprNode::Mul(fs) => match fs.first().and_then(Expr::as_number) {
            Some(c) => {
                let rest = if fs.len() == 2 {
                    fs[1].clone()
                } else {
                    Expr::new(ExprNode::Mul(fs[1..].to_vec()))
                };
                (c.clone(), rest)
            }
            None => (Number::one(), term.clone()),
        },
        _ => (Number::one(), term.clone()),
    }
}

/// Puts a coefficient in front of a canonical non-numeric term.
pub fn with_coeff(c: Number, rest: Expr) -> Expr {
    if rest.is_one() {
        return Expr::num(c);
    }
    if c.is_one() && c.is_exact() {
        return rest;
    }
    if c.is_zero() {
        return Expr::num(c);
    }
    match rest.node() {
        ExprNode::Mul(fs) => {
            let mut v = Vec::with_capacity(fs.len() + 1);
            v.push(Expr::num(c));
            v.extend(fs.iter().cloned());
            Expr::new(ExprNode::Mul(v))
        }
        _ => Expr::new(ExprNode::Mul(vec![Expr::num(c), rest])),
    }
}

/// Splits a factor into base and exponent: `x^3 → (x, 3)`, `x → (x, 1)`.
pub fn split_pow(factor: &Expr) -> (Expr, Expr) {
    match factor.node() {
        ExprNode::Pow(b, e) => (b.clone(), e.clone()),
        _ => (factor.clone(), Expr::one()),
    }
}

/// Approximate value of a constant expression (no symbols), if it has one.
fn numeric_value(e: &Expr) -> Option<Number> {
    if !e.is_constant() {
        return None;
    }
    let z = eval::eval_complex_with(e, &eval::EmptyEnv).ok()?;
    if z.re.is_finite() && z.im.is_finite() {
        Some(Number::complex_float(z.re, z.im))
    } else {
        None
    }
}

/// Canonical sum.
pub fn add(terms: Vec<Expr>) -> Expr {
    let mut flat = Vec::with_capacity(terms.len());
    for t in terms {
        flatten_add(t, &mut flat);
    }
    let mut constant = Number::zero();
    let mut pos_inf = false;
    let mut neg_inf = false;
    let mut groups: BTreeMap<Expr, Number> = BTreeMap::new();
    for t in flat {
        match t.node() {
            ExprNode::Num(n) => constant = constant.add(n),
            ExprNode::Const(Constant::Undefined) => return Expr::undefined(),
            ExprNode::Const(Constant::Infinity) => pos_inf = true,
            _ => {
                let (c, rest) = split_coeff(&t);
                if matches!(rest.node(), ExprNode::Const(Constant::Infinity)) && c.is_real() {
                    if c.is_positive() {
                        pos_inf = true;
                    } else if c.is_negative() {
                        neg_inf = true;
                    }
                    continue;
                }
                match groups.get_mut(&rest) {
                    Some(existing) => *existing = existing.add(&c),
                    None => {
                        groups.insert(rest, c);
                    }
                }
            }
        }
    }
    if pos_inf && neg_inf {
        return Expr::undefined();
    }
    let mut out = Vec::with_capacity(groups.len() + 1);
    for (rest, c) in groups {
        if c.is_zero() {
            continue;
        }
        if !c.is_exact() {
            if let Some(v) = numeric_value(&rest) {
                constant = constant.add(&c.mul(&v));
                continue;
            }
        }
        out.push(with_coeff(c, rest));
    }
    if !constant.is_exact() {
        // Approximate numbers absorb exact constant terms such as π or √2.
        let mut kept = Vec::with_capacity(out.len());
        for t in out {
            match numeric_value(&t) {
                Some(v) => constant = constant.add(&v),
                None => kept.push(t),
            }
        }
        out = kept;
    }
    if pos_inf || neg_inf {
        out.push(if pos_inf {
            Expr::infinity()
        } else {
            Expr::neg_infinity()
        });
    } else if !constant.is_zero() || out.is_empty() {
        out.insert(0, Expr::num(constant));
    }
    match out.len() {
        0 => Expr::zero(),
        1 => out.pop().unwrap_or_else(Expr::zero),
        _ => Expr::new(ExprNode::Add(out)),
    }
}

fn flatten_add(t: Expr, out: &mut Vec<Expr>) {
    match t.node() {
        ExprNode::Add(v) => {
            for c in v {
                flatten_add(c.clone(), out);
            }
        }
        // A number times a sum is spread out inside sums: 2(x + 1) + x = 3x + 2.
        ExprNode::Mul(fs) if fs.len() == 2 && fs[0].is_number() => match fs[1].node() {
            ExprNode::Add(inner) => {
                for c in inner {
                    flatten_add(mul(vec![fs[0].clone(), c.clone()]), out);
                }
            }
            _ => out.push(t),
        },
        _ => out.push(t),
    }
}

/// Canonical product.
pub fn mul(factors: Vec<Expr>) -> Expr {
    mul_depth(factors, 0)
}

fn mul_depth(factors: Vec<Expr>, depth: u32) -> Expr {
    let mut flat = Vec::with_capacity(factors.len());
    for f in factors {
        flatten_mul(f, &mut flat);
    }
    let mut coeff = Number::one();
    let mut infinite = false;
    let mut groups: BTreeMap<Expr, Vec<Expr>> = BTreeMap::new();
    for f in flat {
        match f.node() {
            ExprNode::Num(n) => coeff = coeff.mul(n),
            ExprNode::Const(Constant::Undefined) => return Expr::undefined(),
            ExprNode::Const(Constant::Infinity) => infinite = true,
            ExprNode::Const(Constant::I) => coeff = coeff.mul(&Number::i()),
            _ => {
                let (b, e) = split_pow(&f);
                groups.entry(b).or_default().push(e);
            }
        }
    }
    if coeff.is_zero() {
        return if infinite {
            Expr::undefined()
        } else {
            Expr::num(coeff)
        };
    }
    let mut out = Vec::with_capacity(groups.len() + 1);
    let mut rerun = false;
    for (base, mut exps) in groups {
        let p = if exps.len() == 1 {
            let e = exps.pop().unwrap_or_else(Expr::one);
            if e.is_one() {
                base
            } else {
                Expr::new(ExprNode::Pow(base, e))
            }
        } else {
            pow_depth(base, add(exps), depth + 1)
        };
        match p.node() {
            ExprNode::Num(n) => coeff = coeff.mul(n),
            ExprNode::Mul(_) if depth < MAX_DEPTH => {
                rerun = true;
                out.push(p);
            }
            ExprNode::Const(Constant::Undefined) => return Expr::undefined(),
            _ => out.push(p),
        }
    }
    if rerun {
        out.push(Expr::num(coeff));
        return mul_depth(out, depth + 1);
    }
    if coeff.is_zero() {
        return Expr::num(coeff);
    }
    if infinite {
        if coeff.is_real() {
            coeff = if coeff.is_positive() {
                Number::one()
            } else {
                Number::int(-1)
            };
        }
        out.push(Expr::infinity());
    }
    if !coeff.is_exact() && !infinite {
        let mut kept = Vec::with_capacity(out.len());
        for f in out {
            match numeric_value(&f) {
                Some(v) => coeff = coeff.mul(&v),
                None => kept.push(f),
            }
        }
        out = kept;
    }
    out.sort();
    if out.is_empty() {
        return Expr::num(coeff);
    }
    if coeff.is_one() && coeff.is_exact() {
        if out.len() == 1 {
            return out.pop().unwrap_or_else(Expr::one);
        }
        return Expr::new(ExprNode::Mul(out));
    }
    out.insert(0, Expr::num(coeff));
    Expr::new(ExprNode::Mul(out))
}

fn flatten_mul(f: Expr, out: &mut Vec<Expr>) {
    match f.node() {
        ExprNode::Mul(v) => {
            for c in v {
                flatten_mul(c.clone(), out);
            }
        }
        _ => out.push(f),
    }
}

/// Canonical power.
pub fn pow(base: Expr, exp: Expr) -> Expr {
    pow_depth(base, exp, 0)
}

fn raw_pow(base: Expr, exp: Expr) -> Expr {
    Expr::new(ExprNode::Pow(base, exp))
}

fn pow_depth(base: Expr, exp: Expr, depth: u32) -> Expr {
    if base.is_undefined() || exp.is_undefined() {
        return Expr::undefined();
    }
    if let Some(n) = exp.as_number() {
        if n.is_zero() {
            return if n.is_exact() {
                Expr::one()
            } else {
                Expr::float(1.0)
            };
        }
        if n.is_one() && n.is_exact() {
            return base;
        }
    }
    if depth > MAX_DEPTH {
        return raw_pow(base, exp);
    }
    match base.node() {
        ExprNode::Num(b) => {
            if b.is_one() && b.is_exact() {
                return Expr::one();
            }
            if let Some(n) = exp.as_number() {
                return pow_numbers(b, n);
            }
            if !b.is_exact() {
                if let Some(v) = numeric_value(&exp) {
                    return Expr::num(b.pow_approx(&v));
                }
            }
            if b.is_zero() && exp.is_infinite() {
                return if is_neg_infinity(&exp) {
                    Expr::infinity()
                } else {
                    Expr::zero()
                };
            }
            raw_pow(base, exp)
        }
        ExprNode::Const(Constant::I) => pow_depth(Expr::i(), exp, depth + 1),
        ExprNode::Const(Constant::Infinity) => match exp.as_number().and_then(Number::sign) {
            Some(std::cmp::Ordering::Greater) => Expr::infinity(),
            Some(std::cmp::Ordering::Less) => Expr::zero(),
            _ => raw_pow(base, exp),
        },
        ExprNode::Const(Constant::E) => pow_e(exp, depth),
        ExprNode::Pow(a, c) => {
            let exp_is_int = exp.as_number().is_some_and(Number::is_integer);
            let inner_ok = c
                .as_number()
                .and_then(Number::as_rational)
                .is_some_and(|q| q > &-BigRational::one() && q <= &BigRational::one());
            if exp_is_int || inner_ok {
                pow_depth(a.clone(), mul(vec![c.clone(), exp]), depth + 1)
            } else {
                raw_pow(base, exp)
            }
        }
        ExprNode::Mul(fs) => {
            if exp.as_number().is_some_and(Number::is_integer) {
                let parts = fs
                    .iter()
                    .map(|f| pow_depth(f.clone(), exp.clone(), depth + 1))
                    .collect();
                return mul_depth(parts, depth + 1);
            }
            // (c·x)^a = c^a · x^a for a positive real coefficient c.
            match fs.first().and_then(Expr::as_number) {
                Some(c) if c.is_positive() && !(c.is_one() && c.is_exact()) => {
                    let rest = if fs.len() == 2 {
                        fs[1].clone()
                    } else {
                        Expr::new(ExprNode::Mul(fs[1..].to_vec()))
                    };
                    mul_depth(
                        vec![
                            pow_depth(Expr::num(c.clone()), exp.clone(), depth + 1),
                            pow_depth(rest, exp, depth + 1),
                        ],
                        depth + 1,
                    )
                }
                _ => raw_pow(base, exp),
            }
        }
        _ => {
            if let Some(n) = exp.as_number() {
                if !n.is_exact() {
                    if let Some(b) = numeric_value(&base) {
                        return Expr::num(b.pow_approx(n));
                    }
                }
            }
            raw_pow(base, exp)
        }
    }
}

fn is_neg_infinity(e: &Expr) -> bool {
    e.is_infinite() && !matches!(e.node(), ExprNode::Const(Constant::Infinity))
}

/// `e^exp` rules: `e^(ln x) = x`, `e^(c ln x) = x^c`, Euler's formula at multiples of `iπ/2`.
fn pow_e(exp: Expr, depth: u32) -> Expr {
    let base = Expr::e();
    if let ExprNode::Func(Func::Ln, args) = exp.node() {
        if let Some(x) = args.first() {
            return x.clone();
        }
    }
    if matches!(exp.node(), ExprNode::Const(Constant::Infinity)) {
        return Expr::infinity();
    }
    if is_neg_infinity(&exp) {
        return Expr::zero();
    }
    if let Some(n) = exp.as_number() {
        if !n.is_exact() {
            let z = n.to_complex64().exp();
            return Expr::num(Number::complex_float(z.re, z.im));
        }
    }
    if let ExprNode::Mul(fs) = exp.node() {
        if let [coeff, second] = fs.as_slice() {
            let Some(c) = coeff.as_number() else {
                return raw_pow(base, exp);
            };
            if let ExprNode::Func(Func::Ln, args) = second.node() {
                if let (Some(x), true) = (args.first(), c.is_real()) {
                    return pow_depth(x.clone(), coeff.clone(), depth + 1);
                }
            }
            // e^(k·iπ) for k a multiple of 1/2.
            if matches!(second.node(), ExprNode::Const(Constant::Pi)) {
                if let Number::Complex(re, im) = c {
                    if re.is_zero() {
                        let two_k = im * BigRational::from_integer(BigInt::from(2));
                        if two_k.is_integer() {
                            let r = two_k.to_integer().mod_floor_4();
                            return match r {
                                0 => Expr::one(),
                                1 => Expr::i(),
                                2 => Expr::int(-1),
                                _ => Expr::num(Number::i().neg()),
                            };
                        }
                    }
                }
            }
        }
    }
    raw_pow(base, exp)
}

trait ModFloor4 {
    fn mod_floor_4(&self) -> u8;
}

impl ModFloor4 for BigInt {
    fn mod_floor_4(&self) -> u8 {
        use num_integer::Integer;
        self.mod_floor(&BigInt::from(4)).to_u8().unwrap_or(0)
    }
}

/// Number to a number power: exact where possible (`(4/9)^(1/2) = 2/3`, `8^(1/2) = 2√2`).
fn pow_numbers(b: &Number, n: &Number) -> Expr {
    let raw = || raw_pow(Expr::num(b.clone()), Expr::num(n.clone()));
    if !b.is_exact() || !n.is_exact() {
        return Expr::num(b.pow_approx(n));
    }
    if let Some(k) = n.as_integer() {
        return match b.pow_int(&k) {
            Ok(v) => Expr::num(v),
            Err(crate::MathError::DivisionByZero) => Expr::undefined(),
            Err(_) => raw(),
        };
    }
    let (Some(q), Some(r)) = (n.as_rational(), b.as_rational()) else {
        return raw();
    };
    if r.is_zero() {
        return if q.is_positive() {
            Expr::zero()
        } else {
            Expr::undefined()
        };
    }
    let Some(den) = q.denom().to_u32().filter(|d| *d <= 1000) else {
        return raw();
    };
    // r^(p/den) = (r^p)^(1/den)
    let powered = match Number::Rational(r.clone()).pow_int(q.numer()) {
        Ok(Number::Rational(v)) => v,
        _ => return raw(),
    };
    let negative = powered.is_negative();
    if negative && den % 2 == 0 && den != 2 {
        return raw();
    }
    let (out, inside, index) = rational_root_parts(&powered.abs(), den);
    let mut coeff = Number::Rational(out);
    if negative {
        coeff = if den % 2 == 1 {
            coeff.neg()
        } else {
            coeff.mul(&Number::i())
        };
    }
    if inside.is_one() {
        return Expr::num(coeff);
    }
    let radical = raw_pow(
        Expr::bigint(inside),
        Expr::ratio(BigRational::new(BigInt::one(), BigInt::from(index))),
    );
    with_coeff(coeff, radical)
}

/// Simplifies a whole tree to canonical form.
pub fn simplify(e: &Expr) -> Expr {
    let once = simplify_once(e);
    let twice = simplify_once(&once);
    if twice == once {
        once
    } else {
        simplify_once(&twice)
    }
}

fn simplify_once(e: &Expr) -> Expr {
    let node = e.map_children(&mut simplify_once);
    canonical(node)
}

/// Builds a canonical expression from a node whose children are already canonical.
pub fn canonical(node: ExprNode) -> Expr {
    match node {
        ExprNode::Add(v) => add(v),
        ExprNode::Mul(v) => mul(v),
        ExprNode::Pow(b, e) => pow(b, e),
        ExprNode::Func(f, args) => functions::func(f, args),
        ExprNode::Const(Constant::I) => Expr::i(),
        ExprNode::Logic(op, args) => logic(op, args),
        other => Expr::new(other),
    }
}

fn logic(op: LogicOp, args: Vec<Expr>) -> Expr {
    let as_bool = |e: &Expr| match e.node() {
        ExprNode::Bool(b) => Some(*b),
        _ => None,
    };
    match op {
        LogicOp::Not => match args.first().and_then(as_bool) {
            Some(b) if args.len() == 1 => Expr::new(ExprNode::Bool(!b)),
            _ => Expr::new(ExprNode::Logic(op, args)),
        },
        LogicOp::And | LogicOp::Or => {
            let absorbing = op == LogicOp::Or;
            let mut kept = Vec::new();
            for a in args {
                match as_bool(&a) {
                    Some(b) if b == absorbing => return Expr::new(ExprNode::Bool(absorbing)),
                    Some(_) => {}
                    None => kept.push(a),
                }
            }
            match kept.len() {
                0 => Expr::new(ExprNode::Bool(!absorbing)),
                1 => kept
                    .pop()
                    .unwrap_or_else(|| Expr::new(ExprNode::Bool(!absorbing))),
                _ => Expr::new(ExprNode::Logic(op, kept)),
            }
        }
    }
}

impl Expr {
    /// Canonical form of this expression.
    pub fn simplify(&self) -> Expr {
        simplify(self)
    }

    /// `self ^ exp`, canonical.
    pub fn pow(&self, exp: impl Into<Expr>) -> Expr {
        pow(self.clone(), exp.into())
    }

    /// Square root as the power `1/2`.
    pub fn sqrt(&self) -> Expr {
        pow(self.clone(), Expr::rational(1, 2))
    }

    /// `n`-th root as the power `1/n`.
    pub fn root(&self, n: impl Into<Expr>) -> Expr {
        pow(self.clone(), pow(n.into(), Expr::int(-1)))
    }

    /// Reciprocal `1/self`.
    pub fn recip(&self) -> Expr {
        pow(self.clone(), Expr::int(-1))
    }
}

impl ops::Add for Expr {
    type Output = Expr;
    fn add(self, rhs: Expr) -> Expr {
        add(vec![self, rhs])
    }
}

impl ops::Add for &Expr {
    type Output = Expr;
    fn add(self, rhs: &Expr) -> Expr {
        add(vec![self.clone(), rhs.clone()])
    }
}

impl ops::Sub for Expr {
    type Output = Expr;
    fn sub(self, rhs: Expr) -> Expr {
        add(vec![self, -rhs])
    }
}

impl ops::Sub for &Expr {
    type Output = Expr;
    fn sub(self, rhs: &Expr) -> Expr {
        add(vec![self.clone(), -rhs.clone()])
    }
}

impl ops::Mul for Expr {
    type Output = Expr;
    fn mul(self, rhs: Expr) -> Expr {
        mul(vec![self, rhs])
    }
}

impl ops::Mul for &Expr {
    type Output = Expr;
    fn mul(self, rhs: &Expr) -> Expr {
        mul(vec![self.clone(), rhs.clone()])
    }
}

impl ops::Div for Expr {
    type Output = Expr;
    fn div(self, rhs: Expr) -> Expr {
        mul(vec![self, rhs.recip()])
    }
}

impl ops::Div for &Expr {
    type Output = Expr;
    fn div(self, rhs: &Expr) -> Expr {
        mul(vec![self.clone(), rhs.recip()])
    }
}

impl ops::Neg for Expr {
    type Output = Expr;
    fn neg(self) -> Expr {
        mul(vec![Expr::int(-1), self])
    }
}

impl ops::Neg for &Expr {
    type Output = Expr;
    fn neg(self) -> Expr {
        mul(vec![Expr::int(-1), self.clone()])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn x() -> Expr {
        Expr::sym("x")
    }
    fn y() -> Expr {
        Expr::sym("y")
    }
    fn n(k: i64) -> Expr {
        Expr::int(k)
    }

    #[test]
    fn collects_like_terms() {
        assert_eq!(n(2) * x() + n(3) * x(), n(5) * x());
        assert_eq!(x() - x(), n(0));
        assert_eq!((x() + n(1)) + (x() - n(1)), n(2) * x());
        assert_eq!(n(2) * (x() + n(1)) - n(2) * x(), n(2));
        assert_eq!((x() * y() + y() * x()).to_string(), "2*x*y");
    }

    #[test]
    fn combines_powers() {
        assert_eq!(x() * x().pow(2), x().pow(3));
        assert_eq!(x() / x(), n(1));
        assert_eq!(x().pow(0), n(1));
        assert_eq!(x().pow(1), x());
        assert_eq!(n(0) * x(), n(0));
        assert_eq!(x().sqrt() * x().sqrt(), x());
        assert_eq!(x().pow(2).pow(3), x().pow(6));
        // (x^2)^(1/2) is |x| for real x, so it must not become x.
        assert_ne!(x().pow(2).sqrt(), x());
    }

    #[test]
    fn exact_roots() {
        assert_eq!(Expr::rational(4, 9).sqrt(), Expr::rational(2, 3));
        assert_eq!(n(8).sqrt(), n(2) * n(2).sqrt());
        assert_eq!(n(8).sqrt().to_string(), "2*sqrt(2)");
        assert_eq!(n(-8).root(3), n(-2));
        assert_eq!(n(-4).sqrt(), n(2) * Expr::i());
        assert_eq!(n(2).sqrt() * n(2).sqrt(), n(2));
        assert_eq!(
            Expr::rational(1, 2).sqrt(),
            Expr::rational(1, 2) * n(2).sqrt()
        );
        assert_eq!(n(16).pow(Expr::rational(3, 4)), n(8));
        assert_eq!(n(0).recip(), Expr::undefined());
    }

    #[test]
    fn numeric_folding() {
        assert_eq!(n(2) + n(3) * n(4), n(14));
        assert_eq!(n(1) / n(3) + n(1) / n(6), Expr::rational(1, 2));
        assert_eq!(n(2).pow(10), n(1024));
        assert_eq!(Expr::i() * Expr::i(), n(-1));
        assert_eq!(Expr::float(0.5) + n(1), Expr::float(1.5));
    }

    #[test]
    fn infinity_rules() {
        assert_eq!(Expr::infinity() + n(1), Expr::infinity());
        assert_eq!(Expr::infinity() - Expr::infinity(), Expr::undefined());
        assert_eq!(n(-3) * Expr::infinity(), Expr::neg_infinity());
        assert_eq!(Expr::e().pow(Expr::neg_infinity()), n(0));
        assert_eq!(n(0) * Expr::infinity(), Expr::undefined());
    }

    #[test]
    fn euler() {
        assert_eq!(Expr::e().pow(Expr::i() * Expr::pi()), n(-1));
        assert_eq!(
            Expr::e().pow(Expr::rational(1, 2) * Expr::i() * Expr::pi()),
            Expr::i()
        );
    }

    #[test]
    fn distributes_powers_over_products() {
        assert_eq!((n(2) * x()).pow(2), n(4) * x().pow(2));
        assert_eq!((n(4) * x()).sqrt(), n(2) * x().sqrt());
        assert_eq!((-x()).pow(2), x().pow(2));
    }

    #[test]
    fn simplify_rebuilds_raw_trees() {
        let raw = Expr::new(ExprNode::Add(vec![
            Expr::new(ExprNode::Mul(vec![n(2), x()])),
            Expr::new(ExprNode::Mul(vec![x(), n(3)])),
            Expr::new(ExprNode::Pow(x(), n(0))),
        ]));
        assert_eq!(raw.simplify(), n(5) * x() + n(1));
        let i = Expr::constant(Constant::I);
        let raw = Expr::new(ExprNode::Mul(vec![i.clone(), i]));
        assert_eq!(raw.simplify(), n(-1));
    }

    #[test]
    fn logic_folds() {
        let t = Expr::new(ExprNode::Bool(true));
        let f = Expr::new(ExprNode::Bool(false));
        let raw = Expr::new(ExprNode::Logic(LogicOp::And, vec![t.clone(), x()]));
        assert_eq!(raw.simplify(), x());
        let raw = Expr::new(ExprNode::Logic(LogicOp::Or, vec![t.clone(), x()]));
        assert_eq!(raw.simplify(), t);
        let raw = Expr::new(ExprNode::Logic(LogicOp::Not, vec![f]));
        assert_eq!(raw.simplify(), Expr::new(ExprNode::Bool(true)));
    }
}
