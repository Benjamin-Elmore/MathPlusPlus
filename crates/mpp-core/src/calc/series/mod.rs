//! Taylor, Maclaurin and Laurent series by series arithmetic: known expansions of `exp`,
//! `sin`, `cos`, `sinh`, `cosh`, `ln(1 + u)` and `(1 + u)^α` are combined through sums,
//! products, quotients (`tan = sin/cos`), powers and composition; inverse functions (`atan`,
//! `asin`, …) are integrated term by term from their derivatives. Anything else falls back to
//! derivatives at the centre. Expansions at `∞` are in powers of `1/x`.

mod ps;

#[cfg(test)]
mod tests;

use std::fmt;

use crate::calc::diff::diff;
use crate::error::{MathError, MathResult};
use crate::expr::{Constant, Expr, ExprNode};
use crate::functions::{func, Func};
use crate::simplify::{add, mul, pow, simplify, split_coeff, with_coeff};
use crate::steps::Steps;

pub(crate) use ps::Ps;

/// Largest order a series may be asked for.
pub const MAX_ORDER: u32 = 40;
/// Most derivatives taken by the fallback for functions without a known expansion.
const MAX_FALLBACK_ORDER: i64 = 16;
/// Recursion guard for the expression walk.
const MAX_DEPTH: u32 = 200;
/// Extra working order tried when cancellation eats precision.
const RETRIES: u32 = 4;

/// A truncated series `terms + O(remainder)`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Series {
    /// The polynomial (or Laurent polynomial) part, in powers of `x − a` (or `1/x` at ∞).
    pub terms: Expr,
    /// The expansion variable.
    pub var: String,
    /// The centre (`∞` for an asymptotic expansion).
    pub at: Expr,
    /// Terms of exponent below `order` are exact; the error is `O(base^order)`.
    pub order: i64,
    /// The error term as the expression `O(base^order)`.
    pub remainder: Expr,
    /// Coefficients from the lowest exponent `valuation` up to `order − 1`.
    pub coeffs: Vec<Expr>,
    /// The lowest exponent present (negative for a Laurent series).
    pub valuation: i64,
}

impl Series {
    /// The base the series is written in: `x`, `x − a` or `1/x`.
    pub fn base(&self) -> Expr {
        base_of(&self.var, &self.at)
    }

    /// The coefficient of `base^e`.
    pub fn coeff(&self, e: i64) -> Expr {
        if e < self.valuation {
            return Expr::zero();
        }
        usize::try_from(e - self.valuation)
            .ok()
            .and_then(|i| self.coeffs.get(i).cloned())
            .unwrap_or_else(Expr::zero)
    }

    /// The non-zero terms in ascending powers, each as `(is_negative, |term|)`.
    fn signed_terms(&self) -> Vec<(bool, Expr)> {
        let base = self.base();
        self.coeffs
            .iter()
            .enumerate()
            .filter(|(_, c)| !c.is_zero())
            .map(|(i, c)| {
                let e = self.valuation + i as i64;
                let term = mul(vec![c.clone(), pow(base.clone(), Expr::int(e))]);
                let (k, rest) = split_coeff(&term);
                if k.is_negative() {
                    (true, with_coeff(k.neg(), rest))
                } else {
                    (false, term)
                }
            })
            .collect()
    }

    /// Joins the terms (lowest power first) and the error term.
    fn join(&self, show: &dyn Fn(&Expr) -> String, remainder: String) -> String {
        let mut out = String::new();
        for (neg, t) in self.signed_terms() {
            let first = out.is_empty();
            if first {
                if neg {
                    out.push('-');
                }
            } else {
                out.push_str(if neg { " - " } else { " + " });
            }
            // A first-power term in `x − a` is a sum: `1 − (x − 1)`, not `1 − x − 1`.
            if (neg || !first) && matches!(t.node(), ExprNode::Add(_)) {
                out.push('(');
                out.push_str(&show(&t));
                out.push(')');
            } else {
                out.push_str(&show(&t));
            }
        }
        if out.is_empty() {
            remainder
        } else {
            format!("{out} + {remainder}")
        }
    }

    /// LaTeX for `terms + O(…)`, lowest power first.
    pub fn to_latex(&self) -> String {
        let o = format!(
            "O\\left({}\\right)",
            pow(self.base(), Expr::int(self.order)).to_latex()
        );
        self.join(&|e: &Expr| e.to_latex(), o)
    }
}

impl fmt::Display for Series {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let text = self.join(&|e: &Expr| e.to_string(), self.remainder.to_string());
        f.write_str(&text)
    }
}

fn is_infinity(e: &Expr) -> bool {
    matches!(e.node(), ExprNode::Const(Constant::Infinity))
}

fn base_of(var: &str, at: &Expr) -> Expr {
    let x = Expr::sym(var);
    if is_infinity(at) {
        pow(x, Expr::int(-1))
    } else if at.is_zero() {
        x
    } else {
        add(vec![x, mul(vec![Expr::int(-1), at.clone()])])
    }
}

/// The series of `expr` in `var` about `at` up to (not including) `base^order`.
///
/// `at` may be `∞`, giving an expansion in powers of `1/x`. A pole gives a Laurent series
/// (negative powers). Branch points such as `√x` at 0 (fractional powers) are
/// [`MathError::Unsupported`].
pub fn series(
    expr: &Expr,
    var: &str,
    at: &Expr,
    order: u32,
    steps: Option<&mut Steps>,
) -> MathResult<Series> {
    let mut log = steps;
    if order > MAX_ORDER {
        return Err(MathError::TooLarge(format!("a series of order {order}")));
    }
    let e = simplify(expr);
    let at = simplify(at);
    if at.contains_sym(var) || at.is_undefined() || (at.is_infinite() && !is_infinity(&at)) {
        return Err(MathError::Undefined("this expansion point".into()));
    }
    let want = i64::from(order);
    let mut working = want + 2;
    let mut last = None;
    for _ in 0..=RETRIES {
        let mut b = Builder::new(var, &at, working, log.is_some());
        let s = b.build(&e)?;
        if s.prec >= want {
            if let Some(l) = log.as_deref_mut() {
                l.0.extend(b.steps.0);
            }
            let out = finish(&s, var, &at, want);
            let note = if out.valuation >= 0 && !is_infinity(&at) {
                let listed: Vec<String> = out
                    .coeffs
                    .iter()
                    .enumerate()
                    .map(|(i, c)| format!("c_{} = {c}", out.valuation + i as i64))
                    .collect();
                format!(
                    "{out}; the coefficient of {} is c_k = f⁽ᵏ⁾({at})/k!: {}",
                    pow(out.base(), Expr::sym("k")),
                    listed.join(", ")
                )
            } else {
                format!("{out}")
            };
            Steps::record(&mut log, "combine the series", &e, &out.terms, Some(note));
            return Ok(out);
        }
        working += want - s.prec + 2;
        last = Some(s);
    }
    match last {
        Some(s) if s.prec > s.val => Ok(finish(&s, var, &at, s.prec)),
        _ => Err(MathError::Unsupported(format!(
            "a series of {e} to order {order} (too much cancellation)"
        ))),
    }
}

fn finish(s: &Ps, var: &str, at: &Expr, order: i64) -> Series {
    let base = base_of(var, at);
    let mut t = s.clone();
    t.prec = t.prec.min(order);
    let t = Ps::new(t.val, t.c, t.prec);
    let coeffs: Vec<Expr> = (t.val..order).map(|e| t.get(e)).collect();
    Series {
        terms: t.to_expr(&base),
        var: var.into(),
        at: at.clone(),
        order,
        remainder: Expr::apply("O", vec![pow(base, Expr::int(order))]),
        coeffs,
        valuation: t.val.min(order),
    }
}

/// The leading term `c · base^v` of `expr` about `at`, searching up to `max_order` terms.
pub(crate) fn leading_term(
    expr: &Expr,
    var: &str,
    at: &Expr,
    max_order: i64,
) -> Option<(Expr, i64)> {
    let mut n = 6;
    while n <= max_order {
        let mut b = Builder::new(var, at, n, false);
        let s = b.build(expr).ok()?;
        if let Some(c) = s.lead() {
            return Some((c.clone(), s.val));
        }
        n += 6;
    }
    None
}

/// True when an unevaluated derivative or integral is left in `e`.
fn has_unevaluated(e: &Expr) -> bool {
    matches!(
        e.node(),
        ExprNode::Derivative { .. } | ExprNode::Integral { .. } | ExprNode::Limit { .. }
    ) || e.children().iter().any(|c| has_unevaluated(c))
}

struct Builder<'a> {
    var: &'a str,
    at: Expr,
    infinite: bool,
    n: i64,
    depth: u32,
    logging: bool,
    steps: Steps,
}

impl<'a> Builder<'a> {
    fn new(var: &'a str, at: &Expr, n: i64, logging: bool) -> Builder<'a> {
        Builder {
            var,
            at: at.clone(),
            infinite: is_infinity(at),
            n,
            depth: 0,
            logging,
            steps: Steps::new(),
        }
    }

    fn note(&mut self, rule: String, before: &Expr, s: &Ps) {
        if self.logging {
            let base = base_of(self.var, &self.at);
            let after = s.to_expr(&base);
            self.steps.push(rule, before.clone(), after, None);
        }
    }

    fn build(&mut self, e: &Expr) -> MathResult<Ps> {
        self.depth += 1;
        if self.depth > MAX_DEPTH {
            return Err(MathError::TooLarge("this series expansion".into()));
        }
        let out = self.build_inner(e);
        self.depth -= 1;
        out
    }

    fn build_inner(&mut self, e: &Expr) -> MathResult<Ps> {
        let n = self.n;
        if !e.contains_sym(self.var) {
            return Ok(Ps::constant(e.clone(), n));
        }
        match e.node() {
            ExprNode::Sym(_) => Ok(if self.infinite {
                Ps::new(-1, vec![Expr::one()], n)
            } else {
                Ps::new(0, vec![self.at.clone(), Expr::one()], n)
            }),
            ExprNode::Add(ts) => {
                let mut acc = Ps::zero(n);
                for t in ts {
                    acc = acc.add(&self.build(t)?);
                }
                Ok(acc)
            }
            ExprNode::Mul(fs) => {
                // Divide last so that quotients keep their precision.
                let (den, num): (Vec<&Expr>, Vec<&Expr>) = fs.iter().partition(|f| {
                    matches!(f.node(), ExprNode::Pow(_, x)
                        if x.as_number().is_some_and(|v| v.is_negative()))
                });
                let mut acc = Ps::constant(Expr::one(), n);
                for f in num {
                    acc = acc.mul(&self.build(f)?);
                }
                for f in den {
                    let ExprNode::Pow(b, x) = f.node() else {
                        continue;
                    };
                    let inv = simplify(&mul(vec![Expr::int(-1), x.clone()]));
                    let d = self.build(&pow(b.clone(), inv))?;
                    acc = acc.div(&d)?;
                }
                Ok(acc)
            }
            ExprNode::Pow(b, x) => self.power(e, b, x),
            ExprNode::Func(f, args) => self.function(e, *f, args),
            _ => self.by_derivatives(e),
        }
    }

    fn power(&mut self, e: &Expr, b: &Expr, x: &Expr) -> MathResult<Ps> {
        if !x.contains_sym(self.var) {
            let s = self.build(b)?;
            let out = s.pow_const(x)?;
            if !x.as_number().is_some_and(|v| v.is_integer()) {
                self.note(format!("binomial series (1 + u)^{x}"), e, &out);
            }
            return Ok(out);
        }
        let exponent = if matches!(b.node(), ExprNode::Const(Constant::E)) {
            x.clone()
        } else {
            mul(vec![x.clone(), func(Func::Ln, vec![b.clone()])])
        };
        let out = self.build(&exponent)?.exp()?;
        self.note("exponential series".into(), e, &out);
        Ok(out)
    }

    fn function(&mut self, e: &Expr, f: Func, args: &[Expr]) -> MathResult<Ps> {
        if f == Func::Log {
            if let [x, b] = args {
                let num = self.build(&func(Func::Ln, vec![x.clone()]))?;
                return num.div(&self.build(&func(Func::Ln, vec![b.clone()]))?);
            }
        }
        let [u] = args else {
            return self.by_derivatives(e);
        };
        let out = match f {
            Func::Sin | Func::Cos | Func::Sinh | Func::Cosh => self.build(u)?.trig(f)?,
            Func::Tan | Func::Cot | Func::Tanh => {
                let s = self.build(u)?;
                let (sf, cf) = if f == Func::Tanh {
                    (Func::Sinh, Func::Cosh)
                } else {
                    (Func::Sin, Func::Cos)
                };
                let (si, co) = (s.trig(sf)?, s.trig(cf)?);
                if f == Func::Cot {
                    co.div(&si)?
                } else {
                    si.div(&co)?
                }
            }
            Func::Sec => self.build(u)?.trig(Func::Cos)?.recip()?,
            Func::Csc => self.build(u)?.trig(Func::Sin)?.recip()?,
            Func::Ln => self.build(u)?.ln()?,
            Func::Atan | Func::Asin | Func::Acos | Func::Asinh | Func::Atanh | Func::Acosh => {
                self.inverse(f, u)?
            }
            _ => return self.by_derivatives(e),
        };
        self.note(format!("series of {}", f.name()), e, &out);
        Ok(out)
    }

    /// `f(u) = f(u₀) + ∫ f′(u) u′ dt` for inverse functions.
    fn inverse(&mut self, f: Func, u: &Expr) -> MathResult<Ps> {
        // One more term of u, since differentiating loses one.
        self.n += 1;
        let s = self.build(u);
        self.n -= 1;
        let s = s?;
        let (u0, _) = s.split_constant()?;
        let one = Ps::constant(Expr::one(), s.prec);
        let sq = s.mul(&s);
        let half = Expr::rational(-1, 2);
        let fprime = match f {
            Func::Atan => one.add(&sq).recip()?,
            Func::Atanh => one.sub(&sq).recip()?,
            Func::Asin => one.sub(&sq).pow_const(&half)?,
            Func::Acos => one.sub(&sq).pow_const(&half)?.neg(),
            Func::Asinh => one.add(&sq).pow_const(&half)?,
            _ => sq.sub(&one).pow_const(&half)?,
        };
        let g = fprime.mul(&s.derivative());
        let mut out = g.integral(simplify(&func(f, vec![u0])))?;
        out.prec = out.prec.min(self.n);
        Ok(Ps::new(out.val, out.c, out.prec))
    }

    /// Taylor coefficients `f⁽ʲ⁾(a)/j!` straight from derivatives.
    fn by_derivatives(&mut self, e: &Expr) -> MathResult<Ps> {
        if self.infinite {
            return Err(MathError::Unsupported(format!(
                "an expansion of {e} at infinity"
            )));
        }
        let terms = self.n.clamp(1, MAX_FALLBACK_ORDER);
        let mut c = Vec::with_capacity(terms as usize);
        let mut d = e.clone();
        let mut fact = Expr::one();
        // Stops at the first derivative that cannot be evaluated; the series is then known
        // only to that order.
        for j in 0..terms {
            if j > 0 {
                match diff(&d, self.var, 1, None) {
                    Ok(next) => d = next,
                    Err(_) => break,
                }
                fact = mul(vec![fact, Expr::int(j)]);
            }
            let v = simplify(&d.subs(self.var, &self.at));
            if v.is_undefined()
                || v.is_infinite()
                || v.contains_sym(self.var)
                || has_unevaluated(&v)
            {
                break;
            }
            c.push(simplify(&mul(vec![v, pow(fact.clone(), Expr::int(-1))])));
        }
        if c.is_empty() {
            return Err(MathError::Unsupported(format!(
                "a series of {e} here (it is not analytic at the centre)"
            )));
        }
        let known = c.len() as i64;
        let out = Ps::new(0, c, known);
        self.note("Taylor coefficients from derivatives".into(), e, &out);
        Ok(out)
    }
}
