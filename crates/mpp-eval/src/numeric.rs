//! Numeric answers for `≈ ?`: 10 significant digits, complex values supported.

use mpp_core::number::{format_f64, Number};
use mpp_core::{EmptyEnv, Expr, ExprNode, MathError};
use num_complex::Complex64;

use crate::error::EvalError;

/// Significant digits in a numeric answer.
pub const DIGITS: usize = 10;

/// Formats a complex value, dropping a negligible imaginary part.
pub fn format_complex(z: Complex64, digits: usize) -> String {
    if let Some(shown) = mpp_core::display::format_approx(z, digits) {
        return shown;
    }
    if z.im.abs() <= 1e-12 * z.re.abs().max(1.0) {
        format_f64(z.re, digits)
    } else {
        Number::complex_float(z.re, z.im).to_decimal(digits)
    }
}

/// The value of a closed expression as decimal text.
pub fn approximate(e: &Expr) -> Result<String, EvalError> {
    let free: Vec<String> = e.free_symbols().iter().map(|s| s.to_string()).collect();
    if !free.is_empty() {
        return Err(EvalError::Unbound(free));
    }
    approx_closed(e)
}

fn approx_closed(e: &Expr) -> Result<String, EvalError> {
    match e.node() {
        ExprNode::Matrix { cols, data, .. } => {
            let cells = data
                .iter()
                .map(approx_closed)
                .collect::<Result<Vec<_>, _>>()?;
            let rows: Vec<String> = cells
                .chunks((*cols).max(1))
                .map(|r| format!("[{}]", r.join(", ")))
                .collect();
            Ok(format!("[{}]", rows.join(", ")))
        }
        ExprNode::Tuple(items) | ExprNode::Set(items) => {
            let parts = items
                .iter()
                .map(approx_closed)
                .collect::<Result<Vec<_>, _>>()?;
            let (open, close) = if matches!(e.node(), ExprNode::Set(_)) {
                ("{", "}")
            } else {
                ("(", ")")
            };
            Ok(format!("{open}{}{close}", parts.join(", ")))
        }
        ExprNode::Bool(b) => Ok(b.to_string()),
        _ => match scalar(e) {
            Ok(z) if z.re.is_finite() && z.im.is_finite() => Ok(format_complex(z, DIGITS)),
            // Beyond the range of a float (`100000!`, `2^{1000000}`, `ln(1000!)`): work
            // with the logarithm instead of answering ∞.
            other => match huge::magnitude(e) {
                Some(v) => Ok(v.format(DIGITS)),
                None => other.map(|z| format_complex(z, DIGITS)),
            },
        },
    }
}

/// Real values too large (or small) for a float, carried as a sign and a base-10
/// logarithm, with lnΓ by Stirling's series for factorials.
pub mod huge {
    use mpp_core::{Constant, Expr, ExprNode, Func, Number};
    use num_bigint::BigInt;
    use num_traits::{Signed, ToPrimitive, Zero};
    use std::f64::consts::LN_10;

    /// `sign · 10^log10` (sign 0 for zero).
    #[derive(Clone, Copy, Debug, PartialEq)]
    pub struct Big {
        pub sign: f64,
        pub log10: f64,
    }

    impl Big {
        fn plain(x: f64) -> Option<Big> {
            if !x.is_finite() {
                return None;
            }
            Some(if x == 0.0 {
                Big {
                    sign: 0.0,
                    log10: f64::NEG_INFINITY,
                }
            } else {
                Big {
                    sign: x.signum(),
                    log10: x.abs().log10(),
                }
            })
        }

        /// The value as a float, when it fits.
        pub fn to_f64(self) -> Option<f64> {
            if self.sign == 0.0 {
                return Some(0.0);
            }
            let x = self.sign * 10f64.powf(self.log10);
            (x.is_finite() && x != 0.0).then_some(x)
        }

        /// `2.824229408e456573`: `digits` significant digits and a power of ten.
        pub fn format(self, digits: usize) -> String {
            if let Some(x) = self.to_f64() {
                return mpp_core::number::format_f64(x, digits);
            }
            let sign = if self.sign < 0.0 { "-" } else { "" };
            let mut exp = self.log10.floor();
            if exp.abs() >= 1e15 {
                // Even the exponent is too long to write out: `10^(9.956570552e101)`.
                let shown = mpp_core::number::format_f64(self.log10, digits);
                return format!("{sign}10^({shown})");
            }
            // The digits a float logarithm can vouch for: about 15 in all, less those
            // spent on the exponent.
            let exp_digits = (exp.abs().max(1.0).log10().floor() as usize) + 1;
            let digits = digits.min(15usize.saturating_sub(exp_digits)).clamp(1, 15);
            let mut mantissa = 10f64.powf(self.log10 - exp);
            let scale = 10f64.powi(i32::try_from(digits - 1).unwrap_or(9));
            mantissa = (mantissa * scale).round() / scale;
            if mantissa >= 10.0 {
                mantissa /= 10.0;
                exp += 1.0;
            }
            let text = format!("{mantissa:.*}", digits - 1);
            let text = if text.contains('.') {
                text.trim_end_matches('0').trim_end_matches('.').to_string()
            } else {
                text
            };
            format!("{sign}{text}e{exp}")
        }
    }

    /// log10 of a positive big integer.
    fn log10_int(n: &BigInt) -> f64 {
        let bits = n.bits();
        if bits <= 1000 {
            return n.to_f64().map_or(f64::NAN, f64::log10);
        }
        let shift = bits - 64;
        let top = (n >> shift).to_f64().unwrap_or(f64::NAN);
        top.log10() + (shift as f64) * std::f64::consts::LOG10_2
    }

    /// ln Γ(x) for x > 0 (Stirling's series after shifting x past 10).
    pub fn ln_gamma(x: f64) -> f64 {
        if x <= 0.0 || !x.is_finite() {
            return f64::NAN;
        }
        let mut shift = 0.0;
        let mut x = x;
        while x < 10.0 {
            shift += x.ln();
            x += 1.0;
        }
        let x2 = x * x;
        let series = 1.0 / (12.0 * x) - 1.0 / (360.0 * x * x2) + 1.0 / (1260.0 * x * x2 * x2)
            - 1.0 / (1680.0 * x * x2 * x2 * x2);
        (x - 0.5) * x.ln() - x + 0.5 * (2.0 * std::f64::consts::PI).ln() + series - shift
    }

    fn real(e: &Expr) -> Option<f64> {
        magnitude(e)?.to_f64()
    }

    /// The value of a closed real expression in [`Big`] form, or `None` when it is not
    /// one this knows (complex values, unknown functions).
    pub fn magnitude(e: &Expr) -> Option<Big> {
        match e.node() {
            ExprNode::Num(Number::Rational(q)) => {
                if q.is_zero() {
                    return Big::plain(0.0);
                }
                let sign = if q.is_negative() { -1.0 } else { 1.0 };
                let l = log10_int(&q.numer().abs()) - log10_int(q.denom());
                l.is_finite().then_some(Big { sign, log10: l })
            }
            ExprNode::Num(Number::Float(x)) => Big::plain(*x),
            ExprNode::Num(_) => None,
            ExprNode::Const(Constant::Pi) => Big::plain(std::f64::consts::PI),
            ExprNode::Const(Constant::E) => Big::plain(std::f64::consts::E),
            ExprNode::Const(_) => None,
            ExprNode::Mul(fs) => {
                let mut out = Big {
                    sign: 1.0,
                    log10: 0.0,
                };
                for f in fs {
                    let v = magnitude(f)?;
                    if v.sign == 0.0 {
                        return Big::plain(0.0);
                    }
                    out.sign *= v.sign;
                    out.log10 += v.log10;
                }
                Some(out)
            }
            ExprNode::Add(ts) => {
                let vs = ts.iter().map(magnitude).collect::<Option<Vec<Big>>>()?;
                let top = vs
                    .iter()
                    .filter(|v| v.sign != 0.0)
                    .map(|v| v.log10)
                    .fold(f64::NEG_INFINITY, f64::max);
                if !top.is_finite() {
                    return Big::plain(0.0);
                }
                let rel: f64 = vs
                    .iter()
                    .filter(|v| v.sign != 0.0)
                    .map(|v| v.sign * 10f64.powf(v.log10 - top))
                    .sum();
                if rel == 0.0 || !rel.is_finite() {
                    return None;
                }
                Some(Big {
                    sign: rel.signum(),
                    log10: top + rel.abs().log10(),
                })
            }
            ExprNode::Pow(base, exp) => {
                let k = real(exp)?;
                let b = magnitude(base)?;
                if b.sign == 0.0 {
                    return (k > 0.0).then(|| Big::plain(0.0)).flatten();
                }
                let sign = if b.sign > 0.0 {
                    1.0
                } else if k.fract() == 0.0 {
                    if (k / 2.0).fract() == 0.0 {
                        1.0
                    } else {
                        -1.0
                    }
                } else {
                    return None;
                };
                let l = k * b.log10;
                l.is_finite().then_some(Big { sign, log10: l })
            }
            ExprNode::Func(f, args) => match (f, args.as_slice()) {
                (Func::Factorial, [n]) => {
                    let n = real(n)?;
                    (n >= 0.0).then(|| Big {
                        sign: 1.0,
                        log10: ln_gamma(n + 1.0) / LN_10,
                    })
                }
                (Func::Gamma, [x]) => {
                    let x = real(x)?;
                    (x > 0.0).then(|| Big {
                        sign: 1.0,
                        log10: ln_gamma(x) / LN_10,
                    })
                }
                (Func::Ln, [x]) => {
                    let v = magnitude(x)?;
                    (v.sign > 0.0)
                        .then(|| Big::plain(v.log10 * LN_10))
                        .flatten()
                }
                (Func::Abs, [x]) => {
                    let v = magnitude(x)?;
                    Some(Big {
                        sign: v.sign.abs(),
                        log10: v.log10,
                    })
                }
                _ => Big::plain(e.eval_f64(&mpp_core::EmptyEnv).ok()?),
            },
            _ => Big::plain(e.eval_f64(&mpp_core::EmptyEnv).ok()?),
        }
    }
}

/// The complex value of a closed scalar expression.
pub fn scalar(e: &Expr) -> Result<Complex64, EvalError> {
    match e.eval_f64(&EmptyEnv) {
        Ok(x) => Ok(Complex64::new(x, 0.0)),
        Err(real_err) => match e.eval_complex(&EmptyEnv) {
            Ok(z) if z.re.is_finite() && z.im.is_finite() => Ok(z),
            Ok(_) => Err(EvalError::Math(MathError::Undefined("the value".into()))),
            Err(MathError::Domain { .. }) | Err(MathError::Undefined(_)) => {
                Err(EvalError::Math(real_err))
            }
            Err(e) => Err(EvalError::Math(e)),
        },
    }
}

/// True for an exact rational (the answer is already a plain number).
pub fn is_exact_rational(e: &Expr) -> bool {
    e.as_number()
        .is_some_and(|n| n.as_rational().is_some() || !n.is_exact())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats() {
        assert_eq!(approximate(&Expr::pi()).unwrap(), "3.141592654");
        assert_eq!(approximate(&Expr::int(-4).sqrt()).unwrap(), "2*i");
        assert_eq!(
            approximate(&Expr::sym("x")),
            Err(EvalError::Unbound(vec!["x".into()]))
        );
        assert!(is_exact_rational(&Expr::rational(1, 3)));
        assert!(!is_exact_rational(&Expr::int(2).sqrt()));
    }
}
