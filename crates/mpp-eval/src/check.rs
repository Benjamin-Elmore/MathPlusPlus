//! Checks (`\overset{?}{=}`): a symbolic proof first, then a numeric spot check.

use std::collections::HashMap;

use mpp_core::number::format_f64;
use mpp_core::{Expr, ExprNode};
use num_complex::Complex64;

use crate::holders::contains_holder;
use crate::numeric::{format_complex, DIGITS};

/// Points tried in the numeric spot check.
const ATTEMPTS: usize = 48;
/// Points that must agree before the check counts as passed.
const WANTED: usize = 12;

/// The outcome of a check.
#[derive(Debug, Clone, PartialEq)]
pub enum Verdict {
    /// The difference of the two sides simplifies to zero.
    Proved,
    /// The two sides agree at every random point tried.
    Holds { points: usize },
    /// The two sides differ at this point.
    Fails {
        /// Values of the free names, as decimal text.
        at: Vec<(String, String)>,
        lhs: String,
        rhs: String,
    },
    /// Neither a proof nor a numeric check was possible.
    Unknown(String),
}

impl Verdict {
    /// `Some(true)` for ✓, `Some(false)` for ✗, `None` when undecided.
    pub fn passed(&self) -> Option<bool> {
        match self {
            Verdict::Proved | Verdict::Holds { .. } => Some(true),
            Verdict::Fails { .. } => Some(false),
            Verdict::Unknown(_) => None,
        }
    }

    /// ✓, ✗ or ?.
    pub fn mark(&self) -> &'static str {
        match self.passed() {
            Some(true) => "✓",
            Some(false) => "✗",
            None => "?",
        }
    }

    /// A one-line description.
    pub fn describe(&self) -> String {
        match self {
            Verdict::Proved => "✓ (proved symbolically)".into(),
            Verdict::Holds { points } => format!("✓ (agrees at {points} random points)"),
            Verdict::Fails { at, lhs, rhs } => {
                let point: Vec<String> = at.iter().map(|(n, v)| format!("{n} = {v}")).collect();
                if point.is_empty() {
                    format!("✗ (left side {lhs}, right side {rhs})")
                } else {
                    format!(
                        "✗ (at {}: left side {lhs}, right side {rhs})",
                        point.join(", ")
                    )
                }
            }
            Verdict::Unknown(why) => format!("? ({why})"),
        }
    }
}

/// A small deterministic generator, so checks give the same answer every run.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> f64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 >> 11) as f64 / (1u64 << 53) as f64
    }
}

fn is_comparable(e: &Expr) -> bool {
    !matches!(
        e.node(),
        ExprNode::Relation(..)
            | ExprNode::Logic(..)
            | ExprNode::Bool(_)
            | ExprNode::Matrix { .. }
            | ExprNode::Set(_)
            | ExprNode::Interval { .. }
            | ExprNode::Tuple(_)
    )
}

/// Checks whether `lhs = rhs` holds.
pub fn check(lhs: &Expr, rhs: &Expr) -> Verdict {
    if lhs == rhs {
        return Verdict::Proved;
    }
    if !is_comparable(lhs) || !is_comparable(rhs) {
        return Verdict::Unknown("only numbers and expressions can be compared".into());
    }
    let diff = (lhs - rhs).simplify();
    if diff.is_zero() || diff.expand().is_ok_and(|d| d.is_zero()) {
        return Verdict::Proved;
    }
    // Trigonometric identities: sin²x + cos²x = 1, double angles, tan² + 1 = sec² ….
    if mpp_core::trig::identity_zero(&diff) {
        return Verdict::Proved;
    }
    if contains_holder(lhs) || contains_holder(rhs) {
        return Verdict::Unknown(
            "a side holds a calculus expression that is not evaluated yet".into(),
        );
    }
    spot_check(lhs, rhs)
}

/// Checks an antiderivative: `∫ f dx` and an answer agree when they differ by a constant.
/// `vars` are the integration variables, `constants` the names of the constants of
/// integration (set to 0). `None` when they do not agree this way either.
pub fn check_up_to_constant(
    lhs: &Expr,
    rhs: &Expr,
    vars: &[String],
    constants: &[String],
) -> Option<Verdict> {
    if vars.is_empty() || !is_comparable(lhs) || !is_comparable(rhs) {
        return None;
    }
    let mut d = (lhs - rhs).simplify();
    for c in constants {
        d = d.subs(c, &Expr::zero());
    }
    let mut numeric = None;
    for v in vars {
        let dd = mpp_core::calc::diff(&d, v, 1, None).ok()?.simplify();
        let proved = dd.is_zero()
            || dd.expand().is_ok_and(|e| e.is_zero())
            || mpp_core::trig::identity_zero(&dd);
        if !proved {
            match spot_check(&dd, &Expr::zero()) {
                Verdict::Holds { points } => numeric = Some(points),
                _ => return None,
            }
        }
    }
    Some(match numeric {
        Some(points) => Verdict::Holds { points },
        None => Verdict::Proved,
    })
}

/// The integration variables of the indefinite integrals in `e`.
pub fn indefinite_variables(e: &Expr) -> Vec<String> {
    let mut out = Vec::new();
    fn walk(e: &Expr, out: &mut Vec<String>) {
        if let ExprNode::Integral {
            var, bounds: None, ..
        } = e.node()
        {
            if !out.iter().any(|v| v == &**var) {
                out.push(var.to_string());
            }
        }
        for c in e.children() {
            walk(c, out);
        }
    }
    walk(e, &mut out);
    out
}

fn spot_check(lhs: &Expr, rhs: &Expr) -> Verdict {
    let mut names: Vec<String> = lhs
        .free_symbols()
        .union(&rhs.free_symbols())
        .map(|s| s.to_string())
        .collect();
    names.sort();
    let mut rng = Rng(0x9E37_79B9_7F4A_7C15);
    let mut agreed = 0;
    let attempts = if names.is_empty() { 1 } else { ATTEMPTS };
    for attempt in 0..attempts {
        let env: HashMap<String, Complex64> = names
            .iter()
            .map(|n| {
                let u = rng.next();
                // Alternate between the whole range and positive values, so
                // square roots and logarithms get points inside their domain.
                let x = if attempt % 2 == 0 {
                    -3.0 + 6.0 * u
                } else {
                    0.1 + 2.9 * u
                };
                (n.clone(), Complex64::new(x, 0.0))
            })
            .collect();
        let (Ok(l), Ok(r)) = (lhs.eval_complex(&env), rhs.eval_complex(&env)) else {
            continue;
        };
        if !(l.re.is_finite() && l.im.is_finite() && r.re.is_finite() && r.im.is_finite()) {
            continue;
        }
        let scale = l.norm().max(r.norm()).max(1.0);
        if (l - r).norm() <= 1e-8 * scale {
            agreed += 1;
            if agreed >= WANTED {
                break;
            }
        } else {
            let at = names
                .iter()
                .map(|n| {
                    let v = env.get(n).map_or(0.0, |z| z.re);
                    (n.clone(), format_f64(v, 6))
                })
                .collect();
            return Verdict::Fails {
                at,
                lhs: format_complex(l, DIGITS),
                rhs: format_complex(r, DIGITS),
            };
        }
    }
    if agreed == 0 {
        Verdict::Unknown("neither side could be evaluated".into())
    } else {
        Verdict::Holds { points: agreed }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn verdicts() {
        let x = Expr::sym("x");
        let sin2 = Expr::call("sin", vec![x.clone()]).unwrap().pow(2);
        let cos2 = Expr::call("cos", vec![x.clone()]).unwrap().pow(2);
        assert_eq!(check(&(sin2 + cos2), &Expr::one()).passed(), Some(true));
        let sq = (x.clone() + Expr::one()).pow(2);
        let wrong = x.pow(2) + Expr::one();
        let v = check(&sq, &wrong);
        assert_eq!(v.passed(), Some(false));
        assert!(v.describe().contains("at x = "));
        assert_eq!(check(&Expr::int(2), &Expr::int(3)).mark(), "✗");
    }
}
