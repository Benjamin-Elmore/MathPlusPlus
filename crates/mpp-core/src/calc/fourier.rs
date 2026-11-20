//! Fourier series on `[−L, L]`: the coefficients `aₙ = (1/L)∫ f cos(nπx/L)` and
//! `bₙ = (1/L)∫ f sin(nπx/L)` by definite integration, and the first `N` terms.

use std::fmt;

use crate::calc::integrate::integrate_definite;
use crate::error::{MathError, MathResult};
use crate::expr::Expr;
use crate::functions::{func, Func};
use crate::simplify::{add, mul, pow, simplify};
use crate::steps::Steps;

/// Most harmonics computed.
pub const MAX_HARMONICS: usize = 50;

/// `a₀/2 + Σ_{n=1}^{N} (aₙ cos(nπx/L) + bₙ sin(nπx/L))`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FourierSeries {
    /// `a₀` (the series starts with `a₀/2`).
    pub a0: Expr,
    /// `a₁ … a_N`.
    pub a: Vec<Expr>,
    /// `b₁ … b_N`.
    pub b: Vec<Expr>,
    /// The half-period `L`.
    pub half_period: Expr,
    /// The variable.
    pub var: String,
    /// The partial sum as one expression.
    pub terms: Expr,
}

impl fmt::Display for FourierSeries {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} + …", self.terms)
    }
}

/// The first `harmonics` terms of the Fourier series of `f` on `[−L, L]`.
pub fn fourier_series(
    f: &Expr,
    var: &str,
    half_period: &Expr,
    harmonics: usize,
    steps: Option<&mut Steps>,
) -> MathResult<FourierSeries> {
    let mut log = steps;
    if harmonics > MAX_HARMONICS {
        return Err(MathError::TooLarge(format!(
            "{harmonics} Fourier harmonics"
        )));
    }
    let l = simplify(half_period);
    if l.contains_sym(var) {
        return Err(MathError::Undefined(
            "a half-period that depends on the variable".into(),
        ));
    }
    if let Ok(v) = l.eval_f64(&crate::eval::EmptyEnv) {
        if v <= 0.0 || !v.is_finite() {
            return Err(MathError::Domain {
                function: "a Fourier series".into(),
                detail: "the half-period L must be positive and finite".into(),
            });
        }
    }
    let f = simplify(f);
    let x = Expr::sym(var);
    let mirrored = simplify(&f.subs(var, &mul(vec![Expr::int(-1), x.clone()])));
    let even = mirrored == f;
    let odd = simplify(&add(vec![mirrored, f.clone()])).is_zero();
    let lo = mul(vec![Expr::int(-1), l.clone()]);
    let inv_l = pow(l.clone(), Expr::int(-1));
    let angle = |n: usize| {
        mul(vec![
            Expr::int(n as i64),
            Expr::pi(),
            x.clone(),
            inv_l.clone(),
        ])
    };
    let coeff = |weight: Expr| -> MathResult<Expr> {
        let integrand = simplify(&mul(vec![f.clone(), weight]));
        let v = integrate_definite(&integrand, var, &lo, &l, None)?;
        Ok(simplify(&mul(vec![inv_l.clone(), v])))
    };
    let a0 = if odd {
        Expr::zero()
    } else {
        coeff(Expr::one())?
    };
    let mut a = Vec::with_capacity(harmonics);
    let mut b = Vec::with_capacity(harmonics);
    let mut parts = vec![mul(vec![Expr::rational(1, 2), a0.clone()])];
    for n in 1..=harmonics {
        let cos_n = func(Func::Cos, vec![angle(n)]);
        let sin_n = func(Func::Sin, vec![angle(n)]);
        let an = if odd {
            Expr::zero()
        } else {
            coeff(cos_n.clone())?
        };
        let bn = if even {
            Expr::zero()
        } else {
            coeff(sin_n.clone())?
        };
        parts.push(mul(vec![an.clone(), cos_n]));
        parts.push(mul(vec![bn.clone(), sin_n]));
        a.push(an);
        b.push(bn);
    }
    let terms = simplify(&add(parts));
    let symmetry = if even {
        "f is even, so every bₙ = 0"
    } else if odd {
        "f is odd, so every aₙ = 0"
    } else {
        "aₙ = (1/L)∫ f cos(nπx/L) dx, bₙ = (1/L)∫ f sin(nπx/L) dx"
    };
    Steps::record(
        &mut log,
        "Fourier coefficients",
        &f,
        &terms,
        Some(format!("on [−{l}, {l}]: {symmetry}")),
    );
    Ok(FourierSeries {
        a0,
        a,
        b,
        half_period: l,
        var: var.into(),
        terms,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn x() -> Expr {
        Expr::sym("x")
    }

    #[test]
    fn sawtooth_and_parabola() {
        // x on [−π, π]: bₙ = 2(−1)^{n+1}/n
        let s = fourier_series(&x(), "x", &Expr::pi(), 4, None).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(s.a0, Expr::zero());
        let want: Vec<Expr> = [2, -1].iter().map(|v| Expr::int(*v)).collect();
        assert_eq!(&s.b[..2], &want[..]);
        assert_eq!(s.b[2], Expr::rational(2, 3));
        assert!(s.a.iter().all(Expr::is_zero));
        // x² on [−π, π]: a₀ = 2π²/3, aₙ = 4(−1)ⁿ/n²
        let mut steps = Steps::new();
        let s = fourier_series(&x().pow(2), "x", &Expr::pi(), 3, Some(&mut steps))
            .unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(s.a0, Expr::rational(2, 3) * Expr::pi().pow(2));
        assert_eq!(s.a[0], Expr::int(-4));
        assert_eq!(s.a[1], Expr::int(1));
        assert!(s.b.iter().all(Expr::is_zero));
        assert_eq!(steps.len(), 1);
        assert!(s.to_string().ends_with("+ …"));
    }

    #[test]
    fn other_intervals_and_errors() {
        // A square wave on [−1, 1] (sign x): bₙ = 4/(nπ) for odd n.
        let sq = Expr::call("sign", vec![x()]).unwrap_or_else(|_| Expr::undefined());
        let s = fourier_series(&sq, "x", &Expr::one(), 3, None).unwrap_or_else(|e| panic!("{e}"));
        let b1 = s.b[0].eval_f64(&crate::eval::EmptyEnv).unwrap_or(f64::NAN);
        assert!((b1 - 4.0 / std::f64::consts::PI).abs() < 1e-8, "{}", s.b[0]);
        let b2 = s.b[1].eval_f64(&crate::eval::EmptyEnv).unwrap_or(f64::NAN);
        assert!(b2.abs() < 1e-8);
        assert!(fourier_series(&x(), "x", &Expr::int(-1), 2, None).is_err());
        assert!(fourier_series(&x(), "x", &Expr::one(), MAX_HARMONICS + 1, None).is_err());
    }
}
