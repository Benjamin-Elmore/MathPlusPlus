//! Elementary functions: exact values where known, numeric values everywhere.

use std::cmp::Ordering;
use std::f64::consts::PI;

use num_bigint::BigInt;
use num_complex::Complex64;
use num_integer::Integer;
use num_rational::BigRational;
use num_traits::{One, Signed, Zero};

use crate::error::{MathError, MathResult};
use crate::expr::{Constant, Expr, ExprNode};
use crate::number::{self, Number};
use crate::simplify::{pow, split_coeff};

mod special;

/// Built-in functions. `exp`, `sqrt` and roots are powers, so they have no variant.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Func {
    Sin,
    Cos,
    Tan,
    Sec,
    Csc,
    Cot,
    Asin,
    Acos,
    Atan,
    /// `atan2(y, x)`.
    Atan2,
    Sinh,
    Cosh,
    Tanh,
    Asinh,
    Acosh,
    Atanh,
    /// Natural logarithm.
    Ln,
    /// `log(x, base)`.
    Log,
    Abs,
    Sign,
    Floor,
    Ceil,
    Factorial,
    Gamma,
    /// The digamma function ψ = Γ′/Γ.
    Digamma,
    Min,
    Max,
    Gcd,
    Lcm,
    /// `binomial(n, k)`.
    Binomial,
    /// The error function `erf x = (2/√π) ∫₀ˣ e^(−t²) dt`.
    Erf,
    /// The sine integral `Si x = ∫₀ˣ sin t / t dt`.
    Si,
    /// The cosine integral `Ci x = γ + ln x + ∫₀ˣ (cos t − 1)/t dt` (real for `x > 0`).
    Ci,
}

/// How many arguments a function takes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Arity {
    Exactly(usize),
    AtLeast(usize),
}

impl Func {
    /// Every function, for tables and tests.
    pub const ALL: [Func; 33] = [
        Func::Sin,
        Func::Cos,
        Func::Tan,
        Func::Sec,
        Func::Csc,
        Func::Cot,
        Func::Asin,
        Func::Acos,
        Func::Atan,
        Func::Atan2,
        Func::Sinh,
        Func::Cosh,
        Func::Tanh,
        Func::Asinh,
        Func::Acosh,
        Func::Atanh,
        Func::Ln,
        Func::Log,
        Func::Abs,
        Func::Sign,
        Func::Floor,
        Func::Ceil,
        Func::Factorial,
        Func::Gamma,
        Func::Digamma,
        Func::Min,
        Func::Max,
        Func::Gcd,
        Func::Lcm,
        Func::Binomial,
        Func::Erf,
        Func::Si,
        Func::Ci,
    ];

    /// The plain name used when printing.
    pub fn name(self) -> &'static str {
        match self {
            Func::Sin => "sin",
            Func::Cos => "cos",
            Func::Tan => "tan",
            Func::Sec => "sec",
            Func::Csc => "csc",
            Func::Cot => "cot",
            Func::Asin => "asin",
            Func::Acos => "acos",
            Func::Atan => "atan",
            Func::Atan2 => "atan2",
            Func::Sinh => "sinh",
            Func::Cosh => "cosh",
            Func::Tanh => "tanh",
            Func::Asinh => "asinh",
            Func::Acosh => "acosh",
            Func::Atanh => "atanh",
            Func::Ln => "ln",
            Func::Log => "log",
            Func::Abs => "abs",
            Func::Sign => "sign",
            Func::Floor => "floor",
            Func::Ceil => "ceil",
            Func::Factorial => "factorial",
            Func::Gamma => "Gamma",
            Func::Digamma => "digamma",
            Func::Min => "min",
            Func::Max => "max",
            Func::Gcd => "gcd",
            Func::Lcm => "lcm",
            Func::Binomial => "binomial",
            Func::Erf => "erf",
            Func::Si => "Si",
            Func::Ci => "Ci",
        }
    }

    /// Looks a function up by name (`arcsin` and `asin` are the same).
    pub fn from_name(name: &str) -> Option<Func> {
        Some(match name {
            "sin" => Func::Sin,
            "cos" => Func::Cos,
            "tan" => Func::Tan,
            "sec" => Func::Sec,
            "csc" | "cosec" => Func::Csc,
            "cot" => Func::Cot,
            "asin" | "arcsin" => Func::Asin,
            "acos" | "arccos" => Func::Acos,
            "atan" | "arctan" => Func::Atan,
            "atan2" | "arctan2" => Func::Atan2,
            "sinh" => Func::Sinh,
            "cosh" => Func::Cosh,
            "tanh" => Func::Tanh,
            "asinh" | "arsinh" | "arcsinh" => Func::Asinh,
            "acosh" | "arcosh" | "arccosh" => Func::Acosh,
            "atanh" | "artanh" | "arctanh" => Func::Atanh,
            "ln" => Func::Ln,
            "log" => Func::Log,
            "abs" => Func::Abs,
            "sign" | "sgn" => Func::Sign,
            "floor" => Func::Floor,
            "ceil" => Func::Ceil,
            "factorial" => Func::Factorial,
            "Gamma" | "gamma" | "Γ" => Func::Gamma,
            "digamma" | "psi" => Func::Digamma,
            "min" => Func::Min,
            "max" => Func::Max,
            "gcd" => Func::Gcd,
            "lcm" => Func::Lcm,
            "binomial" | "binom" | "choose" => Func::Binomial,
            "erf" => Func::Erf,
            "Si" => Func::Si,
            "Ci" => Func::Ci,
            _ => return None,
        })
    }

    /// Accepted argument counts.
    pub fn arity(self) -> Arity {
        match self {
            Func::Atan2 | Func::Binomial => Arity::Exactly(2),
            Func::Log => Arity::Exactly(2),
            Func::Min | Func::Max => Arity::AtLeast(1),
            Func::Gcd | Func::Lcm => Arity::AtLeast(1),
            _ => Arity::Exactly(1),
        }
    }

    /// Checks an argument count.
    pub fn check_arity(self, got: usize) -> MathResult<()> {
        let (ok, expected) = match self.arity() {
            Arity::Exactly(n) => (got == n, n.to_string()),
            Arity::AtLeast(n) => (got >= n, format!("at least {n}")),
        };
        if ok {
            Ok(())
        } else {
            Err(MathError::WrongArity {
                function: self.name().into(),
                expected,
                got,
            })
        }
    }
}

impl Expr {
    /// Canonical application of a built-in function; arity is checked.
    pub fn func(f: Func, args: Vec<Expr>) -> MathResult<Expr> {
        f.check_arity(args.len())?;
        Ok(func(f, args))
    }

    /// Applies a function by name: built-ins (plus `exp`, `sqrt`, `root`, `log` with one
    /// argument meaning `ln`, `asec`, `acsc`, `acot`, `sech`, `csch`, `coth`), otherwise a
    /// user-function application.
    pub fn call(name: &str, args: Vec<Expr>) -> MathResult<Expr> {
        let one_arg = |args: &[Expr]| -> MathResult<Expr> {
            match args {
                [a] => Ok(a.clone()),
                _ => Err(MathError::WrongArity {
                    function: name.into(),
                    expected: "1".into(),
                    got: args.len(),
                }),
            }
        };
        let recip_of = |f: Func, a: Expr| Expr::func(f, vec![a]).map(|v| v.recip());
        match name {
            "exp" => Ok(pow(Expr::e(), one_arg(&args)?)),
            "sqrt" => Ok(one_arg(&args)?.sqrt()),
            "cbrt" => Ok(one_arg(&args)?.root(Expr::int(3))),
            "root" => match <[Expr; 2]>::try_from(args) {
                Ok([x, n]) => Ok(x.root(n)),
                Err(args) => Err(MathError::WrongArity {
                    function: "root".into(),
                    expected: "2".into(),
                    got: args.len(),
                }),
            },
            "log" if args.len() == 1 => Expr::func(Func::Ln, args),
            "lg" => Expr::func(Func::Log, vec![one_arg(&args)?, Expr::int(10)]),
            "asec" | "arcsec" => Expr::func(Func::Acos, vec![one_arg(&args)?.recip()]),
            "acsc" | "arccsc" => Expr::func(Func::Asin, vec![one_arg(&args)?.recip()]),
            "acot" | "arccot" => Expr::func(Func::Atan, vec![one_arg(&args)?.recip()]),
            "sech" => recip_of(Func::Cosh, one_arg(&args)?),
            "csch" => recip_of(Func::Sinh, one_arg(&args)?),
            "coth" => recip_of(Func::Tanh, one_arg(&args)?),
            _ => match Func::from_name(name) {
                Some(f) => Expr::func(f, args),
                None => Ok(Expr::apply(name, args)),
            },
        }
    }
}

fn raw(f: Func, args: Vec<Expr>) -> Expr {
    Expr::new(ExprNode::Func(f, args))
}

fn or_raw(f: Func, args: Vec<Expr>, value: Option<Expr>) -> Expr {
    value.unwrap_or_else(|| raw(f, args))
}

/// Canonical application of a built-in function (arguments already canonical).
pub fn func(f: Func, args: Vec<Expr>) -> Expr {
    if f.check_arity(args.len()).is_err() {
        return raw(f, args);
    }
    if args.iter().any(Expr::is_undefined) {
        return Expr::undefined();
    }
    if let Some(v) = approx_value(f, &args) {
        return v;
    }
    let value = match f {
        Func::Sin | Func::Cos | Func::Tan | Func::Sec | Func::Csc | Func::Cot => trig(f, &args[0]),
        Func::Asin | Func::Acos | Func::Atan => inverse_trig(f, &args[0]),
        Func::Atan2 => atan2(&args[0], &args[1]),
        Func::Sinh | Func::Cosh | Func::Tanh | Func::Asinh | Func::Acosh | Func::Atanh => {
            hyperbolic(f, &args[0])
        }
        Func::Ln => ln(&args[0]),
        Func::Log => log(&args[0], &args[1]),
        Func::Abs => abs(&args[0]),
        Func::Sign => sign(&args[0]),
        Func::Floor | Func::Ceil => floor_ceil(f, &args[0]),
        Func::Factorial => factorial(&args[0]),
        Func::Gamma => gamma(&args[0]),
        Func::Digamma => None,
        Func::Min | Func::Max => return min_max(f, args),
        Func::Gcd | Func::Lcm => gcd_lcm(f, &args),
        Func::Binomial => binomial(&args[0], &args[1]),
        Func::Erf | Func::Si | Func::Ci => special_integral(f, &args[0]),
    };
    or_raw(f, args, value)
}

/// Numeric value when every argument is a number and at least one is approximate.
fn approx_value(f: Func, args: &[Expr]) -> Option<Expr> {
    let nums: Vec<&Number> = args.iter().map(Expr::as_number).collect::<Option<_>>()?;
    if nums.iter().all(|n| n.is_exact()) {
        return None;
    }
    if nums.iter().all(|n| n.is_real()) {
        let vals: Vec<f64> = nums.iter().map(|n| n.to_f64()).collect();
        if let Ok(v) = eval_func_f64(f, &vals) {
            return Some(Expr::float(v));
        }
    }
    let vals: Vec<Complex64> = nums.iter().map(|n| n.to_complex64()).collect();
    eval_func_complex(f, &vals)
        .ok()
        .map(|z| Expr::num(Number::complex_float(z.re, z.im)))
}

/// Exact values of `erf`, `Si` and `Ci`: at zero, at ±∞, and odd symmetry.
fn special_integral(f: Func, u: &Expr) -> Option<Expr> {
    if u.is_zero() {
        return Some(match f {
            Func::Ci => Expr::neg_infinity(),
            _ => Expr::zero(),
        });
    }
    if u.is_infinite() {
        let positive = matches!(u.node(), ExprNode::Const(Constant::Infinity));
        let sign = Expr::int(if positive { 1 } else { -1 });
        return match f {
            Func::Erf => Some(sign),
            Func::Si => Some(crate::simplify::mul(vec![
                sign,
                Expr::rational(1, 2),
                Expr::pi(),
            ])),
            Func::Ci if positive => Some(Expr::zero()),
            _ => None,
        };
    }
    if f != Func::Ci && has_negative_sign(u) {
        let minus_u = crate::simplify::mul(vec![Expr::int(-1), u.clone()]);
        return Some(crate::simplify::mul(vec![
            Expr::int(-1),
            func(f, vec![minus_u]),
        ]));
    }
    None
}

/// `Some(c)` when `e = c·π` with `c` an exact rational.
fn pi_multiple(e: &Expr) -> Option<BigRational> {
    match e.node() {
        ExprNode::Num(n) if n.is_zero() => Some(BigRational::zero()),
        ExprNode::Const(Constant::Pi) => Some(BigRational::one()),
        ExprNode::Mul(fs)
            if fs.len() == 2 && matches!(fs[1].node(), ExprNode::Const(Constant::Pi)) =>
        {
            fs[0].as_number().and_then(Number::as_rational).cloned()
        }
        _ => None,
    }
}

/// True when `e` has a negative real coefficient (so `f(-x)` can use symmetry).
fn has_negative_sign(e: &Expr) -> bool {
    let (c, _) = split_coeff(e);
    c.is_negative()
}

fn q(p: i64, d: i64) -> BigRational {
    BigRational::new(BigInt::from(p), BigInt::from(d))
}

fn sqrt_int(n: i64) -> Expr {
    Expr::int(n).sqrt()
}

/// `sin(kπ)` for `k ∈ [0, 1/2]` with a known value.
fn sin_first_quadrant(k: &BigRational) -> Option<Expr> {
    let table: [(BigRational, Expr); 5] = [
        (q(0, 1), Expr::zero()),
        (q(1, 6), Expr::rational(1, 2)),
        (q(1, 4), Expr::rational(1, 2) * sqrt_int(2)),
        (q(1, 3), Expr::rational(1, 2) * sqrt_int(3)),
        (q(1, 2), Expr::one()),
    ];
    let extra: [(BigRational, Expr); 2] = [
        (q(1, 12), Expr::rational(1, 4) * (sqrt_int(6) - sqrt_int(2))),
        (q(5, 12), Expr::rational(1, 4) * (sqrt_int(6) + sqrt_int(2))),
    ];
    table
        .into_iter()
        .chain(extra)
        .find(|(a, _)| a == k)
        .map(|(_, v)| v)
}

/// Exact `sin(kπ)`.
fn sin_pi(k: &BigRational) -> Option<Expr> {
    let two = BigRational::from_integer(BigInt::from(2));
    // Reduce to [0, 2).
    let mut k = k - (k / &two).floor() * &two;
    let mut negate = false;
    if k >= BigRational::one() {
        k -= BigRational::one();
        negate = true;
    }
    if k > q(1, 2) {
        k = BigRational::one() - k;
    }
    let v = sin_first_quadrant(&k)?;
    Some(if negate { -v } else { v })
}

fn cos_pi(k: &BigRational) -> Option<Expr> {
    sin_pi(&(k + q(1, 2)))
}

fn trig(f: Func, x: &Expr) -> Option<Expr> {
    if let Some(k) = pi_multiple(x) {
        let (s, c) = (sin_pi(&k), cos_pi(&k));
        let ratio = |num: Option<Expr>, den: Option<Expr>| -> Option<Expr> {
            let den = den?;
            let num = num?;
            if den.is_zero() {
                return Some(Expr::undefined());
            }
            Some(num / den)
        };
        return match f {
            Func::Sin => s,
            Func::Cos => c,
            Func::Tan => ratio(s, c),
            Func::Cot => ratio(c, s),
            Func::Sec => ratio(Some(Expr::one()), c),
            Func::Csc => ratio(Some(Expr::one()), s),
            _ => None,
        };
    }
    // f(inverse f(y)) = y
    if let ExprNode::Func(g, inner) = x.node() {
        let back = matches!(
            (f, g),
            (Func::Sin, Func::Asin) | (Func::Cos, Func::Acos) | (Func::Tan, Func::Atan)
        );
        if back {
            return inner.first().cloned();
        }
    }
    if has_negative_sign(x) {
        let pos = -x.clone();
        let inner = func(f, vec![pos]);
        return Some(match f {
            Func::Cos | Func::Sec => inner,
            _ => -inner,
        });
    }
    None
}

fn inverse_trig(f: Func, x: &Expr) -> Option<Expr> {
    if let ExprNode::Const(Constant::Infinity) = x.node() {
        return (f == Func::Atan).then(|| Expr::rational(1, 2) * Expr::pi());
    }
    if f == Func::Atan && x.is_infinite() {
        return Some(Expr::rational(-1, 2) * Expr::pi());
    }
    if has_negative_sign(x) {
        let pos = -x.clone();
        let inner = func(f, vec![pos]);
        return Some(match f {
            Func::Acos => Expr::pi() - inner,
            _ => -inner,
        });
    }
    let sin_table = [
        (Expr::zero(), q(0, 1)),
        (Expr::rational(1, 2), q(1, 6)),
        (Expr::rational(1, 2) * sqrt_int(2), q(1, 4)),
        (Expr::rational(1, 2) * sqrt_int(3), q(1, 3)),
        (Expr::one(), q(1, 2)),
    ];
    let tan_table = [
        (Expr::zero(), q(0, 1)),
        (Expr::rational(1, 3) * sqrt_int(3), q(1, 6)),
        (Expr::one(), q(1, 4)),
        (sqrt_int(3), q(1, 3)),
    ];
    let lookup =
        |table: &[(Expr, BigRational)]| table.iter().find(|(v, _)| v == x).map(|(_, k)| k.clone());
    let k = match f {
        Func::Asin => lookup(&sin_table)?,
        Func::Acos => q(1, 2) - lookup(&sin_table)?,
        Func::Atan => lookup(&tan_table)?,
        _ => return None,
    };
    Some(Expr::ratio(k) * Expr::pi())
}

fn atan2(y: &Expr, x: &Expr) -> Option<Expr> {
    let yn = y.as_number().and_then(Number::as_rational)?;
    let xn = x.as_number().and_then(Number::as_rational)?;
    let half_pi = Expr::rational(1, 2) * Expr::pi();
    Some(
        match (xn.cmp(&BigRational::zero()), yn.cmp(&BigRational::zero())) {
            (Ordering::Equal, Ordering::Equal) => Expr::undefined(),
            (Ordering::Equal, Ordering::Greater) => half_pi,
            (Ordering::Equal, Ordering::Less) => -half_pi,
            (Ordering::Greater, _) => func(Func::Atan, vec![Expr::ratio(yn / xn)]),
            (Ordering::Less, Ordering::Less) => {
                func(Func::Atan, vec![Expr::ratio(yn / xn)]) - Expr::pi()
            }
            (Ordering::Less, _) => func(Func::Atan, vec![Expr::ratio(yn / xn)]) + Expr::pi(),
        },
    )
}

fn hyperbolic(f: Func, x: &Expr) -> Option<Expr> {
    if x.is_zero() {
        return match f {
            Func::Cosh => Some(Expr::one()),
            Func::Acosh => None,
            _ => Some(Expr::zero()),
        };
    }
    if f == Func::Acosh && x.is_one() {
        return Some(Expr::zero());
    }
    if let ExprNode::Func(g, inner) = x.node() {
        let back = matches!(
            (f, g),
            (Func::Sinh, Func::Asinh) | (Func::Cosh, Func::Acosh) | (Func::Tanh, Func::Atanh)
        );
        if back {
            return inner.first().cloned();
        }
    }
    if f != Func::Acosh && has_negative_sign(x) {
        let inner = func(f, vec![-x.clone()]);
        return Some(if f == Func::Cosh { inner } else { -inner });
    }
    None
}

fn ln(x: &Expr) -> Option<Expr> {
    match x.node() {
        ExprNode::Num(n) if n.is_one() => Some(Expr::zero()),
        ExprNode::Num(n) if n.is_zero() => Some(Expr::neg_infinity()),
        ExprNode::Const(Constant::E) => Some(Expr::one()),
        ExprNode::Const(Constant::Infinity) => Some(Expr::infinity()),
        ExprNode::Pow(b, e) if matches!(b.node(), ExprNode::Const(Constant::E)) => {
            // ln(e^r) = r only for real r.
            e.as_number().filter(|n| n.is_real()).map(|_| e.clone())
        }
        _ => None,
    }
}

/// Exact `log_b(x)` for positive rationals when `x = b^(p/q)` with small `q`.
fn log(x: &Expr, b: &Expr) -> Option<Expr> {
    if matches!(b.node(), ExprNode::Const(Constant::E)) {
        return Some(func(Func::Ln, vec![x.clone()]));
    }
    if x.is_one() {
        return Some(Expr::zero());
    }
    if x == b {
        return Some(Expr::one());
    }
    let xr = x.as_number().and_then(Number::as_rational)?;
    let br = b.as_number().and_then(Number::as_rational)?;
    if !xr.is_positive() || !br.is_positive() || br.is_one() {
        return None;
    }
    let ratio = number::ratio_to_f64(xr).ln() / number::ratio_to_f64(br).ln();
    if !ratio.is_finite() {
        return None;
    }
    for den in 1..=6i64 {
        let p = (ratio * den as f64).round();
        if p.abs() > 10_000.0 {
            return None;
        }
        let p = p as i64;
        let lhs = Number::Rational(xr.clone())
            .pow_int(&BigInt::from(den))
            .ok()?;
        let rhs = Number::Rational(br.clone())
            .pow_int(&BigInt::from(p))
            .ok()?;
        if lhs == rhs {
            return Some(Expr::rational(p, den));
        }
    }
    None
}

fn abs(x: &Expr) -> Option<Expr> {
    match x.node() {
        ExprNode::Num(Number::Rational(r)) => Some(Expr::ratio(r.abs())),
        ExprNode::Num(Number::Complex(a, b)) => Some(Expr::ratio(a * a + b * b).sqrt()),
        ExprNode::Const(Constant::Pi | Constant::E | Constant::Infinity) => Some(x.clone()),
        ExprNode::Func(Func::Abs, _) => Some(x.clone()),
        ExprNode::Pow(b, e) if e.as_number().is_some_and(Number::is_real) => {
            Some(pow(func(Func::Abs, vec![b.clone()]), e.clone()))
        }
        ExprNode::Mul(_) => {
            let (c, rest) = split_coeff(x);
            if c.is_one() {
                return None;
            }
            let c_abs = abs(&Expr::num(c))?;
            Some(c_abs * func(Func::Abs, vec![rest]))
        }
        _ => None,
    }
}

fn sign(x: &Expr) -> Option<Expr> {
    match x.node() {
        ExprNode::Num(n) => match n.sign() {
            Some(Ordering::Greater) => Some(Expr::one()),
            Some(Ordering::Less) => Some(Expr::int(-1)),
            Some(Ordering::Equal) => Some(Expr::zero()),
            None => Some(x.clone() / func(Func::Abs, vec![x.clone()])),
        },
        ExprNode::Const(Constant::Pi | Constant::E | Constant::Infinity) => Some(Expr::one()),
        ExprNode::Func(Func::Sign, _) => Some(x.clone()),
        ExprNode::Mul(_) => {
            let (c, rest) = split_coeff(x);
            if c.is_one() || !c.is_real() {
                return None;
            }
            Some(sign(&Expr::num(c))? * func(Func::Sign, vec![rest]))
        }
        _ => None,
    }
}

fn floor_ceil(f: Func, x: &Expr) -> Option<Expr> {
    if let Some(r) = x.as_number().and_then(Number::as_rational) {
        return Some(Expr::ratio(if f == Func::Floor {
            r.floor()
        } else {
            r.ceil()
        }));
    }
    if x.is_constant() {
        let v = x.eval_f64(&crate::eval::EmptyEnv).ok()?;
        let target = if f == Func::Floor {
            v.floor()
        } else {
            v.ceil()
        };
        // Only trust the float when it is clearly away from an integer.
        if (v - v.round()).abs() > 1e-9 * v.abs().max(1.0) && target.abs() < 1e15 {
            return Some(Expr::int(target as i64));
        }
    }
    None
}

fn factorial(x: &Expr) -> Option<Expr> {
    let r = x.as_number().and_then(Number::as_rational)?;
    if r.is_integer() {
        let n = r.to_integer();
        if n.is_negative() {
            return Some(Expr::undefined());
        }
        return number::factorial(&n).ok().map(Expr::bigint);
    }
    // Half-integers through Γ.
    if r.denom() == &BigInt::from(2) {
        return gamma(&Expr::ratio(r + BigRational::one()));
    }
    None
}

fn gamma(x: &Expr) -> Option<Expr> {
    let r = x.as_number().and_then(Number::as_rational)?;
    if r.is_integer() {
        let n = r.to_integer();
        if !n.is_positive() {
            return Some(Expr::undefined());
        }
        return number::factorial(&(n - 1)).ok().map(Expr::bigint);
    }
    if r.denom() != &BigInt::from(2) {
        return None;
    }
    // r = m + 1/2.
    let m = (r - q(1, 2)).to_integer();
    let m_abs = m.abs();
    if m_abs > BigInt::from(number::MAX_FACTORIAL / 2) {
        return None;
    }
    let two_m = number::factorial(&(&m_abs * 2)).ok()?;
    let m_fact = number::factorial(&m_abs).ok()?;
    let four_m = Number::int(4).pow_int(&m_abs).ok()?;
    let four_m = four_m.as_integer()?;
    let coeff = if m.is_negative() {
        // Γ(1/2 − n) = (−4)^n n! / (2n)! · √π
        let sign = if m_abs.is_odd() { -1 } else { 1 };
        BigRational::new(four_m * m_fact * sign, two_m)
    } else {
        // Γ(n + 1/2) = (2n)! / (4^n n!) · √π
        BigRational::new(two_m, four_m * m_fact)
    };
    Some(Expr::ratio(coeff) * Expr::pi().sqrt())
}

fn min_max(f: Func, args: Vec<Expr>) -> Expr {
    let mut flat = Vec::new();
    for a in args {
        match a.node() {
            ExprNode::Func(g, inner) if *g == f => flat.extend(inner.iter().cloned()),
            _ => flat.push(a),
        }
    }
    let mut best: Option<Number> = None;
    let mut rest: Vec<Expr> = Vec::new();
    for a in flat {
        match a.as_number().filter(|n| n.is_real()) {
            Some(n) => {
                let better = match &best {
                    None => true,
                    Some(b) => {
                        let ord = n.cmp_real(b);
                        if f == Func::Min {
                            ord == Some(Ordering::Less)
                        } else {
                            ord == Some(Ordering::Greater)
                        }
                    }
                };
                if better {
                    best = Some(n.clone());
                }
            }
            None => {
                if !rest.contains(&a) {
                    rest.push(a);
                }
            }
        }
    }
    rest.sort();
    if let Some(b) = best {
        rest.push(Expr::num(b));
    }
    match rest.len() {
        1 => rest.pop().unwrap_or_else(Expr::undefined),
        _ => raw(f, rest),
    }
}

fn gcd_lcm(f: Func, args: &[Expr]) -> Option<Expr> {
    let nums: Vec<&BigRational> = args
        .iter()
        .map(|a| a.as_number().and_then(Number::as_rational))
        .collect::<Option<_>>()?;
    let mut acc = nums.first()?.abs();
    for n in &nums[1..] {
        acc = if f == Func::Gcd {
            number::gcd_rational(&acc, n)
        } else {
            number::lcm_rational(&acc, n)
        };
    }
    Some(Expr::ratio(acc.abs()))
}

fn binomial(n: &Expr, k: &Expr) -> Option<Expr> {
    if k.is_zero() {
        return Some(Expr::one());
    }
    if k.is_one() {
        return Some(n.clone());
    }
    if n == k {
        return Some(Expr::one());
    }
    let kk = k.as_number().and_then(Number::as_integer)?;
    let nn = n.as_number().and_then(Number::as_rational)?;
    number::binomial(nn, &kk).ok().map(Expr::ratio)
}

/// Lanczos approximation of Γ for complex arguments (g = 7, n = 9).
pub fn gamma_complex(z: Complex64) -> Complex64 {
    const G: f64 = 7.0;
    const COEFFS: [f64; 9] = [
        0.999_999_999_999_809_9,
        676.520_368_121_885_1,
        -1_259.139_216_722_402_8,
        771.323_428_777_653_1,
        -176.615_029_162_140_6,
        12.507_343_278_686_905,
        -0.138_571_095_265_720_12,
        9.984_369_578_019_572e-6,
        1.505_632_735_149_311_6e-7,
    ];
    if z.re < 0.5 {
        // Reflection: Γ(z) Γ(1 − z) = π / sin(πz).
        let one = Complex64::new(1.0, 0.0);
        return Complex64::new(PI, 0.0) / ((z * PI).sin() * gamma_complex(one - z));
    }
    let z = z - 1.0;
    let mut x = Complex64::new(COEFFS[0], 0.0);
    for (i, c) in COEFFS.iter().enumerate().skip(1) {
        x += *c / (z + i as f64);
    }
    let t = z + G + 0.5;
    (2.0 * PI).sqrt() * t.powc(z + 0.5) * (-t).exp() * x
}

/// Γ for real arguments; errors at the poles.
pub fn gamma_f64(x: f64) -> MathResult<f64> {
    if x <= 0.0 && x.fract() == 0.0 {
        return Err(MathError::Domain {
            function: "Γ".into(),
            detail: "Γ has a pole at zero and the negative integers".into(),
        });
    }
    if x.fract() == 0.0 && x <= 171.0 {
        // Exact for integers in range.
        let mut acc = 1.0f64;
        let mut k = 2.0;
        while k < x {
            acc *= k;
            k += 1.0;
        }
        return Ok(acc);
    }
    if x > 171.7 {
        return Ok(f64::INFINITY);
    }
    Ok(gamma_complex(Complex64::new(x, 0.0)).re)
}

/// Digamma ψ(z) for complex arguments (reflection, recurrence, asymptotic series).
pub fn digamma_complex(z: Complex64) -> Complex64 {
    if z.re < 0.5 {
        // ψ(z) = ψ(1 − z) − π cot(πz)
        let one = Complex64::new(1.0, 0.0);
        return digamma_complex(one - z) - PI / (z * PI).tan();
    }
    let mut z = z;
    let mut acc = Complex64::new(0.0, 0.0);
    while z.re < 10.0 {
        acc -= z.inv();
        z += 1.0;
    }
    let inv = z.inv();
    let inv2 = inv * inv;
    let series = inv2
        * (1.0 / 12.0
            - inv2 * (1.0 / 120.0 - inv2 * (1.0 / 252.0 - inv2 * (1.0 / 240.0 - inv2 / 132.0))));
    acc + z.ln() - inv * 0.5 - series
}

/// Digamma for real arguments; errors at the poles.
pub fn digamma_f64(x: f64) -> MathResult<f64> {
    if x <= 0.0 && x.fract() == 0.0 {
        return Err(MathError::Domain {
            function: "ψ".into(),
            detail: "ψ has a pole at zero and the negative integers".into(),
        });
    }
    if x < 0.5 {
        return Ok(digamma_f64(1.0 - x)? - PI / (PI * x).tan());
    }
    Ok(digamma_complex(Complex64::new(x, 0.0)).re)
}

fn domain(f: Func, detail: &str) -> MathError {
    MathError::Domain {
        function: f.name().into(),
        detail: detail.into(),
    }
}

fn gcd_f64(a: f64, b: f64) -> f64 {
    let (mut a, mut b) = (a.abs(), b.abs());
    while b > 0.5 {
        let t = a % b;
        a = b;
        b = t;
    }
    a
}

/// Evaluates a built-in function in real arithmetic.
pub fn eval_func_f64(f: Func, args: &[f64]) -> MathResult<f64> {
    f.check_arity(args.len())?;
    let x = args[0];
    let v = match f {
        Func::Sin => x.sin(),
        Func::Cos => x.cos(),
        Func::Tan => {
            if x.cos() == 0.0 {
                return Err(domain(f, "tan is undefined at odd multiples of π/2"));
            }
            x.tan()
        }
        Func::Sec => 1.0 / x.cos(),
        Func::Csc => {
            if x.sin() == 0.0 {
                return Err(MathError::DivisionByZero);
            }
            1.0 / x.sin()
        }
        Func::Cot => {
            if x.sin() == 0.0 {
                return Err(MathError::DivisionByZero);
            }
            x.cos() / x.sin()
        }
        Func::Asin | Func::Acos => {
            if !(-1.0..=1.0).contains(&x) {
                return Err(domain(f, "the argument must lie in [-1, 1]"));
            }
            if f == Func::Asin {
                x.asin()
            } else {
                x.acos()
            }
        }
        Func::Atan => x.atan(),
        Func::Atan2 => {
            if x == 0.0 && args[1] == 0.0 {
                return Err(domain(f, "atan2(0, 0) has no angle"));
            }
            x.atan2(args[1])
        }
        Func::Sinh => x.sinh(),
        Func::Cosh => x.cosh(),
        Func::Tanh => x.tanh(),
        Func::Asinh => x.asinh(),
        Func::Acosh => {
            if x < 1.0 {
                return Err(domain(f, "the argument must be at least 1"));
            }
            x.acosh()
        }
        Func::Atanh => {
            if x <= -1.0 || x >= 1.0 {
                return Err(domain(f, "the argument must lie in (-1, 1)"));
            }
            x.atanh()
        }
        Func::Ln => {
            if x < 0.0 {
                return Err(domain(f, "the logarithm of a negative number is not real"));
            }
            if x == 0.0 {
                return Ok(f64::NEG_INFINITY);
            }
            x.ln()
        }
        Func::Log => {
            let b = args[1];
            if x < 0.0 || b <= 0.0 || b == 1.0 {
                return Err(domain(
                    f,
                    "needs a positive argument and a positive base other than 1",
                ));
            }
            x.ln() / b.ln()
        }
        Func::Abs => x.abs(),
        Func::Sign => {
            if x > 0.0 {
                1.0
            } else if x < 0.0 {
                -1.0
            } else {
                0.0
            }
        }
        Func::Floor => x.floor(),
        Func::Ceil => x.ceil(),
        Func::Factorial => gamma_f64(x + 1.0)?,
        Func::Gamma => gamma_f64(x)?,
        Func::Digamma => digamma_f64(x)?,
        Func::Min => args.iter().copied().fold(f64::INFINITY, f64::min),
        Func::Max => args.iter().copied().fold(f64::NEG_INFINITY, f64::max),
        Func::Gcd | Func::Lcm => {
            if args.iter().any(|a| a.fract() != 0.0) {
                return Err(domain(f, "the arguments must be integers"));
            }
            let mut acc = x.abs();
            for &a in &args[1..] {
                acc = if f == Func::Gcd {
                    gcd_f64(acc, a)
                } else if acc == 0.0 || a == 0.0 {
                    0.0
                } else {
                    (acc * a / gcd_f64(acc, a)).abs()
                };
            }
            acc
        }
        Func::Erf => special::erf(x),
        Func::Si => special::si(x),
        Func::Ci => {
            if x < 0.0 {
                return Err(domain(f, "Ci is real only for positive arguments"));
            }
            if x == 0.0 {
                return Ok(f64::NEG_INFINITY);
            }
            special::ci(x)
        }
        Func::Binomial => {
            let k = args[1];
            if k.fract() != 0.0 {
                let n = x;
                gamma_f64(n + 1.0)? / (gamma_f64(k + 1.0)? * gamma_f64(n - k + 1.0)?)
            } else if k < 0.0 {
                0.0
            } else {
                let mut acc = 1.0;
                let mut j = 0.0;
                while j < k {
                    acc *= (x - j) / (j + 1.0);
                    j += 1.0;
                }
                acc
            }
        }
    };
    Ok(v)
}

/// Evaluates a built-in function in complex arithmetic (principal branches).
pub fn eval_func_complex(f: Func, args: &[Complex64]) -> MathResult<Complex64> {
    f.check_arity(args.len())?;
    let z = args[0];
    let one = Complex64::new(1.0, 0.0);
    let real_only = |f: Func| -> MathResult<Complex64> {
        if args.iter().all(|a| a.im == 0.0) {
            let vals: Vec<f64> = args.iter().map(|a| a.re).collect();
            eval_func_f64(f, &vals).map(|v| Complex64::new(v, 0.0))
        } else {
            Err(domain(f, "the argument must be real"))
        }
    };
    let nonzero = |w: Complex64| -> MathResult<Complex64> {
        if w.re == 0.0 && w.im == 0.0 {
            Err(MathError::DivisionByZero)
        } else {
            Ok(w)
        }
    };
    Ok(match f {
        Func::Sin => z.sin(),
        Func::Cos => z.cos(),
        Func::Tan => z.tan(),
        Func::Sec => one / nonzero(z.cos())?,
        Func::Csc => one / nonzero(z.sin())?,
        Func::Cot => z.cos() / nonzero(z.sin())?,
        Func::Asin => z.asin(),
        Func::Acos => z.acos(),
        Func::Atan => z.atan(),
        Func::Sinh => z.sinh(),
        Func::Cosh => z.cosh(),
        Func::Tanh => z.tanh(),
        Func::Asinh => z.asinh(),
        Func::Acosh => z.acosh(),
        Func::Atanh => z.atanh(),
        Func::Ln => {
            if z.re == 0.0 && z.im == 0.0 {
                return Ok(Complex64::new(f64::NEG_INFINITY, 0.0));
            }
            z.ln()
        }
        Func::Log => {
            let b = nonzero(args[1])?;
            nonzero(z)?.ln() / nonzero(b.ln())?
        }
        Func::Abs => Complex64::new(z.norm(), 0.0),
        Func::Sign => {
            if z.re == 0.0 && z.im == 0.0 {
                Complex64::new(0.0, 0.0)
            } else {
                z / z.norm()
            }
        }
        Func::Gamma => {
            if z.im == 0.0 {
                Complex64::new(gamma_f64(z.re)?, 0.0)
            } else {
                gamma_complex(z)
            }
        }
        Func::Digamma => {
            if z.im == 0.0 {
                Complex64::new(digamma_f64(z.re)?, 0.0)
            } else {
                digamma_complex(z)
            }
        }
        Func::Factorial => {
            if z.im == 0.0 {
                Complex64::new(gamma_f64(z.re + 1.0)?, 0.0)
            } else {
                gamma_complex(z + 1.0)
            }
        }
        Func::Atan2
        | Func::Floor
        | Func::Ceil
        | Func::Min
        | Func::Max
        | Func::Gcd
        | Func::Lcm
        | Func::Binomial
        | Func::Erf
        | Func::Si
        | Func::Ci => real_only(f)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::eval::EmptyEnv;

    fn call(name: &str, args: Vec<Expr>) -> Expr {
        Expr::call(name, args).unwrap()
    }
    fn x() -> Expr {
        Expr::sym("x")
    }
    fn pi_over(d: i64) -> Expr {
        Expr::rational(1, d) * Expr::pi()
    }

    #[test]
    fn known_trig_values() {
        assert_eq!(call("sin", vec![pi_over(6)]), Expr::rational(1, 2));
        assert_eq!(call("cos", vec![Expr::zero()]), Expr::one());
        assert_eq!(call("cos", vec![Expr::pi()]), Expr::int(-1));
        assert_eq!(call("sin", vec![Expr::pi()]), Expr::zero());
        assert_eq!(
            call("sin", vec![Expr::rational(5, 4) * Expr::pi()]),
            Expr::rational(-1, 2) * Expr::int(2).sqrt()
        );
        assert_eq!(call("tan", vec![pi_over(4)]), Expr::one());
        assert_eq!(call("tan", vec![pi_over(3)]), Expr::int(3).sqrt());
        assert_eq!(call("tan", vec![pi_over(2)]), Expr::undefined());
        assert_eq!(call("sec", vec![pi_over(3)]), Expr::int(2));
        assert_eq!(call("cot", vec![pi_over(4)]), Expr::one());
        assert_eq!(
            call("cos", vec![Expr::rational(-2, 3) * Expr::pi()]),
            Expr::rational(-1, 2)
        );
    }

    #[test]
    fn trig_symmetry_and_inverses() {
        assert_eq!(call("sin", vec![-x()]), -call("sin", vec![x()]));
        assert_eq!(call("cos", vec![-x()]), call("cos", vec![x()]));
        assert_eq!(call("sin", vec![call("asin", vec![x()])]), x());
        assert_eq!(call("asin", vec![Expr::rational(1, 2)]), pi_over(6));
        assert_eq!(call("acos", vec![Expr::rational(1, 2)]), pi_over(3));
        assert_eq!(
            call("acos", vec![Expr::rational(-1, 2)]),
            Expr::rational(2, 3) * Expr::pi()
        );
        assert_eq!(call("atan", vec![Expr::one()]), pi_over(4));
        assert_eq!(call("atan", vec![Expr::infinity()]), pi_over(2));
        assert_eq!(
            call("asin", vec![Expr::int(-1)]),
            Expr::rational(-1, 2) * Expr::pi()
        );
        assert_eq!(call("atan2", vec![Expr::one(), Expr::zero()]), pi_over(2));
        assert_eq!(
            call("atan2", vec![Expr::one(), Expr::int(-1)]),
            Expr::rational(3, 4) * Expr::pi()
        );
    }

    #[test]
    fn logs_and_exponentials() {
        assert_eq!(call("ln", vec![Expr::one()]), Expr::zero());
        assert_eq!(call("ln", vec![Expr::e()]), Expr::one());
        assert_eq!(call("exp", vec![call("ln", vec![x()])]), x());
        assert_eq!(
            call("ln", vec![call("exp", vec![Expr::int(3)])]),
            Expr::int(3)
        );
        assert_eq!(call("exp", vec![Expr::zero()]), Expr::one());
        assert_eq!(
            call("log", vec![Expr::int(1000), Expr::int(10)]),
            Expr::int(3)
        );
        assert_eq!(
            call("log", vec![Expr::int(2), Expr::int(4)]),
            Expr::rational(1, 2)
        );
        assert_eq!(
            call("log", vec![Expr::rational(1, 8), Expr::int(2)]),
            Expr::int(-3)
        );
        assert_eq!(call("log", vec![x(), Expr::e()]), call("ln", vec![x()]));
        assert_eq!(call("ln", vec![Expr::zero()]), Expr::neg_infinity());
        assert_eq!(
            call("exp", vec![Expr::int(2) * call("ln", vec![x()])]),
            x().pow(2)
        );
    }

    #[test]
    fn roots_through_call() {
        assert_eq!(
            call("sqrt", vec![Expr::rational(4, 9)]),
            Expr::rational(2, 3)
        );
        assert_eq!(
            call("root", vec![Expr::int(27), Expr::int(3)]),
            Expr::int(3)
        );
        assert_eq!(call("cbrt", vec![Expr::int(-27)]), Expr::int(-3));
        assert!(Expr::call("root", vec![x()]).is_err());
    }

    #[test]
    fn abs_sign_floor_ceil() {
        assert_eq!(call("abs", vec![Expr::int(-3)]), Expr::int(3));
        assert_eq!(
            call("abs", vec![Expr::int(-2) * x()]),
            Expr::int(2) * call("abs", vec![x()])
        );
        assert_eq!(
            call("abs", vec![Expr::int(3) + Expr::int(4) * Expr::i()]),
            Expr::int(5)
        );
        assert_eq!(call("sign", vec![Expr::rational(-1, 2)]), Expr::int(-1));
        assert_eq!(call("floor", vec![Expr::rational(7, 2)]), Expr::int(3));
        assert_eq!(call("ceil", vec![Expr::rational(-7, 2)]), Expr::int(-3));
        assert_eq!(call("floor", vec![Expr::pi()]), Expr::int(3));
        assert_eq!(call("ceil", vec![Expr::e()]), Expr::int(3));
    }

    #[test]
    fn factorial_gamma_binomial() {
        assert_eq!(call("factorial", vec![Expr::int(5)]), Expr::int(120));
        assert_eq!(call("Gamma", vec![Expr::int(5)]), Expr::int(24));
        assert_eq!(call("Gamma", vec![Expr::rational(1, 2)]), Expr::pi().sqrt());
        assert_eq!(
            call("Gamma", vec![Expr::rational(5, 2)]),
            Expr::rational(3, 4) * Expr::pi().sqrt()
        );
        assert_eq!(
            call("Gamma", vec![Expr::rational(-1, 2)]),
            Expr::int(-2) * Expr::pi().sqrt()
        );
        assert_eq!(call("Gamma", vec![Expr::zero()]), Expr::undefined());
        assert_eq!(
            call("binomial", vec![Expr::int(10), Expr::int(3)]),
            Expr::int(120)
        );
        assert_eq!(call("binomial", vec![x(), Expr::one()]), x());
        let g = gamma_f64(0.5).unwrap();
        assert!((g - PI.sqrt()).abs() < 1e-12);
        assert!((gamma_f64(-0.5).unwrap() + 2.0 * PI.sqrt()).abs() < 1e-10);
        assert!(gamma_f64(-2.0).is_err());
        assert_eq!(gamma_f64(6.0).unwrap(), 120.0);
        // ψ(1) = −γ and ψ(1/2) = −γ − 2 ln 2.
        let euler_gamma = std::f64::consts::EULER_GAMMA;
        assert!((digamma_f64(1.0).unwrap() + euler_gamma).abs() < 1e-12);
        assert!((digamma_f64(0.5).unwrap() + euler_gamma + 2.0 * 2f64.ln()).abs() < 1e-12);
        assert!((digamma_f64(-0.5).unwrap() - 0.036_489_973_978_576_52).abs() < 1e-10);
        assert!(digamma_f64(0.0).is_err());
    }

    #[test]
    fn min_max_gcd_lcm() {
        assert_eq!(
            call("min", vec![Expr::int(3), Expr::int(1), Expr::int(2)]),
            Expr::int(1)
        );
        assert_eq!(
            call("max", vec![Expr::int(3), x(), Expr::int(5)]).to_string(),
            "max(x, 5)"
        );
        assert_eq!(
            call("gcd", vec![Expr::int(12), Expr::int(18)]),
            Expr::int(6)
        );
        assert_eq!(call("lcm", vec![Expr::int(4), Expr::int(6)]), Expr::int(12));
    }

    #[test]
    fn hyperbolic_values() {
        assert_eq!(call("sinh", vec![Expr::zero()]), Expr::zero());
        assert_eq!(call("cosh", vec![Expr::zero()]), Expr::one());
        assert_eq!(call("acosh", vec![Expr::one()]), Expr::zero());
        assert_eq!(call("cosh", vec![-x()]), call("cosh", vec![x()]));
        assert_eq!(call("tanh", vec![-x()]), -call("tanh", vec![x()]));
    }

    #[test]
    fn approximate_arguments_evaluate() {
        let v = call("sin", vec![Expr::float(0.5)]);
        assert_eq!(v, Expr::float(0.5f64.sin()));
        let v = call("ln", vec![Expr::float(-1.0)]);
        let z = v.eval_complex(&EmptyEnv).unwrap();
        assert!((z.im - PI).abs() < 1e-12);
    }

    #[test]
    fn arity_errors() {
        assert!(matches!(
            Expr::call("sin", vec![x(), x()]),
            Err(MathError::WrongArity { .. })
        ));
        assert!(Expr::call("f", vec![x()]).is_ok());
    }

    #[test]
    fn numeric_functions() {
        for f in Func::ALL {
            let n = match f.arity() {
                Arity::Exactly(n) | Arity::AtLeast(n) => n.max(1),
            };
            let args = vec![0.5; n];
            let real = eval_func_f64(f, &args);
            let cplx: Vec<Complex64> = args.iter().map(|a| Complex64::new(*a, 0.0)).collect();
            let complex = eval_func_complex(f, &cplx);
            if let (Ok(r), Ok(c)) = (real, complex) {
                assert!((r - c.re).abs() < 1e-9, "{f:?}");
            }
        }
    }
}
