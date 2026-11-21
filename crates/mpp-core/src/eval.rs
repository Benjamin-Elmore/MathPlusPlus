//! Numeric evaluation in `f64` and `Complex64`.

use std::collections::{BTreeMap, HashMap};

use num_complex::Complex64;

use crate::error::{MathError, MathResult};
use crate::expr::{BigOpKind, Constant, Expr, ExprNode, LogicOp, RelOp};
use crate::functions::{eval_func_complex, eval_func_f64};
use crate::number::Number;

/// Most terms a finite sum or product is evaluated over.
const MAX_TERMS: i64 = 10_000_000;

/// Values for free symbols (and, optionally, user functions) during numeric evaluation.
pub trait Env {
    /// Real value of a symbol.
    fn real(&self, name: &str) -> Option<f64>;

    /// Complex value of a symbol (defaults to the real value).
    fn complex(&self, name: &str) -> Option<Complex64> {
        self.real(name).map(|x| Complex64::new(x, 0.0))
    }

    /// Calls a user function with real arguments.
    fn call_real(&self, _name: &str, _args: &[f64]) -> Option<MathResult<f64>> {
        None
    }

    /// Calls a user function with complex arguments.
    fn call_complex(&self, _name: &str, _args: &[Complex64]) -> Option<MathResult<Complex64>> {
        None
    }
}

/// An environment with no values.
pub struct EmptyEnv;

impl Env for EmptyEnv {
    fn real(&self, _name: &str) -> Option<f64> {
        None
    }
}

impl Env for HashMap<String, f64> {
    fn real(&self, name: &str) -> Option<f64> {
        self.get(name).copied()
    }
}

impl Env for HashMap<&str, f64> {
    fn real(&self, name: &str) -> Option<f64> {
        self.get(name).copied()
    }
}

impl Env for BTreeMap<String, f64> {
    fn real(&self, name: &str) -> Option<f64> {
        self.get(name).copied()
    }
}

impl Env for [(&str, f64)] {
    fn real(&self, name: &str) -> Option<f64> {
        self.iter().find(|(n, _)| *n == name).map(|(_, v)| *v)
    }
}

impl<const N: usize> Env for [(&str, f64); N] {
    fn real(&self, name: &str) -> Option<f64> {
        self.as_slice().real(name)
    }
}

impl Env for HashMap<String, Complex64> {
    fn real(&self, name: &str) -> Option<f64> {
        self.get(name).filter(|z| z.im == 0.0).map(|z| z.re)
    }
    fn complex(&self, name: &str) -> Option<Complex64> {
        self.get(name).copied()
    }
}

/// Neumaier compensated sum.
fn compensated_sum(values: impl Iterator<Item = f64>) -> f64 {
    let mut sum = 0.0;
    let mut c = 0.0;
    for v in values {
        let t = sum + v;
        if sum.abs() >= v.abs() {
            c += (sum - t) + v;
        } else {
            c += (v - t) + sum;
        }
        sum = t;
    }
    sum + c
}

/// Evaluates in real `f64` arithmetic; complex intermediate values are errors.
pub fn eval_f64_with(e: &Expr, env: &dyn Env) -> MathResult<f64> {
    let v = real(e, env)?;
    if v.is_nan() {
        return Err(MathError::Undefined("the value".into()));
    }
    Ok(v)
}

fn real(e: &Expr, env: &dyn Env) -> MathResult<f64> {
    match e.node() {
        ExprNode::Num(n) => {
            if n.is_real() {
                Ok(n.to_f64())
            } else {
                Err(MathError::Domain {
                    function: "real evaluation".into(),
                    detail: format!("{n} is a complex number"),
                })
            }
        }
        ExprNode::Sym(s) => env
            .real(s)
            .ok_or_else(|| MathError::UnboundVariable(s.to_string())),
        ExprNode::Const(c) => match c {
            Constant::Pi => Ok(std::f64::consts::PI),
            Constant::E => Ok(std::f64::consts::E),
            Constant::Infinity => Ok(f64::INFINITY),
            Constant::I => Err(MathError::Domain {
                function: "real evaluation".into(),
                detail: "i is not a real number".into(),
            }),
            Constant::Undefined => Err(MathError::Undefined("the value".into())),
        },
        ExprNode::Add(v) => {
            let vals = v
                .iter()
                .map(|t| real(t, env))
                .collect::<MathResult<Vec<_>>>()?;
            Ok(compensated_sum(vals.into_iter()))
        }
        ExprNode::Mul(v) => v.iter().try_fold(1.0, |acc, f| Ok(acc * real(f, env)?)),
        ExprNode::Pow(b, x) => {
            let bv = real(b, env)?;
            let xv = real(x, env)?;
            real_pow(bv, xv, x.as_number())
        }
        ExprNode::Func(f, args) => {
            let vals = args
                .iter()
                .map(|a| real(a, env))
                .collect::<MathResult<Vec<_>>>()?;
            eval_func_f64(*f, &vals)
        }
        ExprNode::Apply(name, args) => {
            let vals = args
                .iter()
                .map(|a| real(a, env))
                .collect::<MathResult<Vec<_>>>()?;
            env.call_real(name, &vals)
                .or_else(|| crate::domains::special::call_real(name, &vals))
                .unwrap_or_else(|| Err(MathError::UnknownFunction(name.to_string())))
        }
        ExprNode::BigOp {
            kind,
            expr,
            var,
            lower,
            upper,
        } => {
            let (lo, hi) = int_bounds(*kind, real(lower, env)?, real(upper, env)?)?;
            let mut vals = Vec::new();
            for k in lo..=hi {
                let bound = Bind {
                    inner: env,
                    name: var.as_ref(),
                    value: k as f64,
                };
                vals.push(real(expr, &bound)?);
            }
            Ok(match kind {
                BigOpKind::Sum => compensated_sum(vals.into_iter()),
                BigOpKind::Product => vals.into_iter().product(),
            })
        }
        ExprNode::Piecewise { cases, otherwise } => {
            for (value, cond) in cases {
                if truth(cond, env)? {
                    return real(value, env);
                }
            }
            match otherwise {
                Some(o) => real(o, env),
                None => Err(MathError::Undefined("the piecewise function here".into())),
            }
        }
        _ => Err(not_a_number(e)),
    }
}

fn not_a_number(e: &Expr) -> MathError {
    let what = match e.node() {
        ExprNode::Relation(..) => "a relation",
        ExprNode::Bool(_) | ExprNode::Logic(..) => "a truth value",
        ExprNode::Matrix { .. } => "a matrix",
        ExprNode::Set(_) | ExprNode::Interval { .. } => "a set",
        ExprNode::Tuple(_) => "a tuple",
        ExprNode::Derivative { .. } => "an unevaluated derivative",
        ExprNode::Integral { .. } => "an unevaluated integral",
        ExprNode::Limit { .. } => "an unevaluated limit",
        _ => "this expression",
    };
    MathError::NotANumber(what.into())
}

fn real_pow(b: f64, x: f64, exact_exp: Option<&Number>) -> MathResult<f64> {
    if b == 0.0 && x < 0.0 {
        return Err(MathError::DivisionByZero);
    }
    if b < 0.0 && x.fract() != 0.0 {
        // A real odd root of a negative number: (-8)^(1/3) = -2.
        if let Some(q) = exact_exp.and_then(Number::as_rational) {
            use num_integer::Integer;
            if q.denom().is_odd() {
                let mag = (-b).powf(x);
                return Ok(if q.numer().is_odd() { -mag } else { mag });
            }
        }
        return Err(MathError::Domain {
            function: "power".into(),
            detail: "a negative number to a fractional power is not real".into(),
        });
    }
    Ok(b.powf(x))
}

fn int_bounds(kind: BigOpKind, lo: f64, hi: f64) -> MathResult<(i64, i64)> {
    if !lo.is_finite() || !hi.is_finite() {
        return Err(MathError::Unsupported(
            "numeric evaluation of an infinite sum".into(),
        ));
    }
    let (lo, hi) = (lo.ceil() as i64, hi.floor() as i64);
    if hi.saturating_sub(lo) > MAX_TERMS {
        let what = match kind {
            BigOpKind::Sum => "this sum",
            BigOpKind::Product => "this product",
        };
        return Err(MathError::TooLarge(what.into()));
    }
    Ok((lo, hi))
}

fn truth(cond: &Expr, env: &dyn Env) -> MathResult<bool> {
    match cond.node() {
        ExprNode::Bool(b) => Ok(*b),
        ExprNode::Relation(op, a, b) => {
            let (a, b) = (real(a, env)?, real(b, env)?);
            Ok(match op {
                RelOp::Eq => a == b,
                RelOp::Ne => a != b,
                RelOp::Lt => a < b,
                RelOp::Le => a <= b,
                RelOp::Gt => a > b,
                RelOp::Ge => a >= b,
                RelOp::Approx => (a - b).abs() <= 1e-9 * a.abs().max(b.abs()).max(1.0),
            })
        }
        ExprNode::Logic(op, args) => match op {
            LogicOp::Not => Ok(!truth(
                args.first()
                    .ok_or_else(|| MathError::NotANumber("an empty condition".into()))?,
                env,
            )?),
            LogicOp::And => {
                for a in args {
                    if !truth(a, env)? {
                        return Ok(false);
                    }
                }
                Ok(true)
            }
            LogicOp::Or => {
                for a in args {
                    if truth(a, env)? {
                        return Ok(true);
                    }
                }
                Ok(false)
            }
        },
        _ => Err(MathError::NotANumber("this condition".into())),
    }
}

/// An environment with one extra binding.
struct Bind<'a> {
    inner: &'a dyn Env,
    name: &'a str,
    value: f64,
}

impl Env for Bind<'_> {
    fn real(&self, name: &str) -> Option<f64> {
        if name == self.name {
            Some(self.value)
        } else {
            self.inner.real(name)
        }
    }
    fn complex(&self, name: &str) -> Option<Complex64> {
        if name == self.name {
            Some(Complex64::new(self.value, 0.0))
        } else {
            self.inner.complex(name)
        }
    }
    fn call_real(&self, name: &str, args: &[f64]) -> Option<MathResult<f64>> {
        self.inner.call_real(name, args)
    }
    fn call_complex(&self, name: &str, args: &[Complex64]) -> Option<MathResult<Complex64>> {
        self.inner.call_complex(name, args)
    }
}

/// Evaluates in complex arithmetic (principal branches).
pub fn eval_complex_with(e: &Expr, env: &dyn Env) -> MathResult<Complex64> {
    match e.node() {
        ExprNode::Num(n) => Ok(n.to_complex64()),
        ExprNode::Sym(s) => env
            .complex(s)
            .ok_or_else(|| MathError::UnboundVariable(s.to_string())),
        ExprNode::Const(c) => match c {
            Constant::Pi => Ok(Complex64::new(std::f64::consts::PI, 0.0)),
            Constant::E => Ok(Complex64::new(std::f64::consts::E, 0.0)),
            Constant::I => Ok(Complex64::new(0.0, 1.0)),
            Constant::Infinity => Ok(Complex64::new(f64::INFINITY, 0.0)),
            Constant::Undefined => Err(MathError::Undefined("the value".into())),
        },
        ExprNode::Add(v) => {
            let vals = v
                .iter()
                .map(|t| eval_complex_with(t, env))
                .collect::<MathResult<Vec<_>>>()?;
            Ok(Complex64::new(
                compensated_sum(vals.iter().map(|z| z.re)),
                compensated_sum(vals.iter().map(|z| z.im)),
            ))
        }
        ExprNode::Mul(v) => v.iter().try_fold(Complex64::new(1.0, 0.0), |acc, f| {
            Ok(acc * eval_complex_with(f, env)?)
        }),
        ExprNode::Pow(b, x) => {
            let bv = eval_complex_with(b, env)?;
            let xv = eval_complex_with(x, env)?;
            complex_pow(bv, xv)
        }
        ExprNode::Func(f, args) => {
            let vals = args
                .iter()
                .map(|a| eval_complex_with(a, env))
                .collect::<MathResult<Vec<_>>>()?;
            eval_func_complex(*f, &vals)
        }
        ExprNode::Apply(name, args) => {
            let vals = args
                .iter()
                .map(|a| eval_complex_with(a, env))
                .collect::<MathResult<Vec<_>>>()?;
            env.call_complex(name, &vals)
                .or_else(|| crate::domains::special::call_complex(name, &vals))
                .unwrap_or_else(|| Err(MathError::UnknownFunction(name.to_string())))
        }
        ExprNode::BigOp {
            kind,
            expr,
            var,
            lower,
            upper,
        } => {
            let (lo, hi) = int_bounds(*kind, real(lower, env)?, real(upper, env)?)?;
            let mut acc = match kind {
                BigOpKind::Sum => Complex64::new(0.0, 0.0),
                BigOpKind::Product => Complex64::new(1.0, 0.0),
            };
            for k in lo..=hi {
                let bound = Bind {
                    inner: env,
                    name: var.as_ref(),
                    value: k as f64,
                };
                let v = eval_complex_with(expr, &bound)?;
                match kind {
                    BigOpKind::Sum => acc += v,
                    BigOpKind::Product => acc *= v,
                }
            }
            Ok(acc)
        }
        ExprNode::Piecewise { .. } => real(e, env).map(|x| Complex64::new(x, 0.0)),
        _ => Err(not_a_number(e)),
    }
}

fn complex_pow(b: Complex64, x: Complex64) -> MathResult<Complex64> {
    if b.re == 0.0 && b.im == 0.0 {
        if x.re > 0.0 {
            return Ok(Complex64::new(0.0, 0.0));
        }
        if x.re == 0.0 && x.im == 0.0 {
            return Ok(Complex64::new(1.0, 0.0));
        }
        return Err(MathError::DivisionByZero);
    }
    if x.im == 0.0 && x.re.fract() == 0.0 && x.re.abs() <= f64::from(i32::MAX) {
        return Ok(b.powi(x.re as i32));
    }
    if b.im == 0.0 && b.re > 0.0 && x.im == 0.0 {
        return Ok(Complex64::new(b.re.powf(x.re), 0.0));
    }
    Ok(b.powc(x))
}

impl Expr {
    /// Real value with symbols taken from `env`.
    pub fn eval_f64(&self, env: &dyn Env) -> MathResult<f64> {
        eval_f64_with(self, env)
    }

    /// Complex value with symbols taken from `env`.
    pub fn eval_complex(&self, env: &dyn Env) -> MathResult<Complex64> {
        eval_complex_with(self, env)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::functions::Func;

    #[test]
    fn evaluates_with_an_environment() {
        let x = Expr::sym("x");
        let e = x.pow(2) + Expr::int(2) * x.clone() + Expr::one();
        assert_eq!(e.eval_f64(&[("x", 3.0)]).unwrap(), 16.0);
        assert_eq!(
            e.eval_f64(&EmptyEnv),
            Err(MathError::UnboundVariable("x".into()))
        );
        let mut map = HashMap::new();
        map.insert("x".to_string(), 1.0);
        assert_eq!(e.eval_f64(&map).unwrap(), 4.0);
    }

    #[test]
    fn real_and_complex_modes() {
        let e = Expr::int(-4).sqrt();
        assert!(e.eval_f64(&EmptyEnv).is_err());
        let z = e.eval_complex(&EmptyEnv).unwrap();
        assert!((z.im - 2.0).abs() < 1e-12);
        let cube = Expr::new(ExprNode::Pow(Expr::sym("x"), Expr::rational(1, 3)));
        assert!((cube.eval_f64(&[("x", -8.0)]).unwrap() + 2.0).abs() < 1e-12);
    }

    #[test]
    fn errors_are_reported() {
        let e = Expr::new(ExprNode::Pow(Expr::sym("x"), Expr::int(-1)));
        assert_eq!(e.eval_f64(&[("x", 0.0)]), Err(MathError::DivisionByZero));
        let f = Expr::apply("f", vec![Expr::one()]);
        assert_eq!(
            f.eval_f64(&EmptyEnv),
            Err(MathError::UnknownFunction("f".into()))
        );
        let ln = Expr::new(ExprNode::Func(Func::Ln, vec![Expr::sym("x")]));
        assert!(ln.eval_f64(&[("x", -1.0)]).is_err());
    }

    #[test]
    fn finite_sums_and_piecewise() {
        let sum = Expr::new(ExprNode::BigOp {
            kind: BigOpKind::Sum,
            expr: Expr::sym("k").pow(2),
            var: "k".into(),
            lower: Expr::one(),
            upper: Expr::int(10),
        });
        assert_eq!(sum.eval_f64(&EmptyEnv).unwrap(), 385.0);
        let pw = Expr::new(ExprNode::Piecewise {
            cases: vec![(
                Expr::sym("x"),
                Expr::relation(RelOp::Ge, Expr::sym("x"), Expr::zero()),
            )],
            otherwise: Some(-Expr::sym("x")),
        });
        assert_eq!(pw.eval_f64(&[("x", -3.0)]).unwrap(), 3.0);
    }
}
