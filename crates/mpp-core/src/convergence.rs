//! Convergence tests reported one by one (divergence, geometric, p-series, comparison,
//! limit comparison, ratio, root, integral, alternating series, telescoping) and the radius
//! and interval of convergence of a power series, with both endpoints checked.

use std::cmp::Ordering;
use std::fmt;

use crate::calc::limit::limit;
use crate::calc::sum::alternating_split;
use crate::calc::sum::convergence::{
    bounded_comparison, divergence_test, eventual_sign, eventually_decreasing, integral_test,
    limit_comparison, p_series, ratio_test, rational_comparison, root_test, telescoping,
    term_ratio,
};
use crate::calc::sum::{convergence as decide, ConvergenceReport};
use crate::error::{MathError, MathResult};
use crate::expr::{Expr, ExprNode, LimitDir};
use crate::functions::{func, Func};
use crate::poly::as_poly;
use crate::simplify::{add, mul, pow, simplify};
use crate::steps::Steps;

use crate::calc_support::value;

/// What one named test says about a series.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TestOutcome {
    /// The test's name.
    pub test: &'static str,
    /// `Some(true)` converges, `Some(false)` diverges, `None` when the test does not decide
    /// (it is inconclusive or does not apply).
    pub verdict: Option<bool>,
    /// Why.
    pub detail: String,
}

impl fmt::Display for TestOutcome {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let v = match self.verdict {
            Some(true) => "converges",
            Some(false) => "diverges",
            None => "no conclusion",
        };
        write!(f, "{}: {v} ({})", self.test, self.detail)
    }
}

fn outcome(test: &'static str, r: Option<ConvergenceReport>, missing: &str) -> TestOutcome {
    match r {
        Some(r) => TestOutcome {
            test,
            verdict: r.converges,
            detail: r.detail,
        },
        None => TestOutcome {
            test,
            verdict: None,
            detail: missing.to_string(),
        },
    }
}

/// Runs every named test on `Σ term` (index `k`) and returns each outcome, plus the overall
/// verdict. The verdict is `None` when no test decides; if two tests ever disagree the
/// verdict is also `None` (never a wrong answer).
pub fn series_tests(
    term: &Expr,
    k: &str,
    steps: Option<&mut Steps>,
) -> MathResult<(Option<bool>, Vec<TestOutcome>)> {
    let t = simplify(term);
    if t.free_symbols().iter().any(|s| &**s != k) {
        return Err(MathError::Unsupported(
            "convergence tests on a term with other unknowns".into(),
        ));
    }
    let (alternating, mag) = match alternating_split(&t, k) {
        Some((_, m)) => (true, m),
        None => (false, t.clone()),
    };
    let sign = eventual_sign(&mag, k);
    let pos = match sign {
        Some(Ordering::Less) => simplify(&mul(vec![Expr::int(-1), mag.clone()])),
        _ => mag.clone(),
    };
    let positive = sign.is_some();
    let mut out = Vec::new();
    // Divergence (nth-term) test.
    out.push(match divergence_test(&mag, k) {
        Some(r) => outcome("divergence test", Some(r), ""),
        None => TestOutcome {
            test: "divergence test",
            verdict: None,
            detail: "the terms tend to 0, so this test is inconclusive".into(),
        },
    });
    // Geometric series.
    let geometric = geometric_test(&t, k);
    out.push(outcome(
        "geometric series test",
        geometric,
        "the terms are not a constant times rⁿ",
    ));
    let only_positive = "it needs terms of one sign";
    let absolute = |r: Option<ConvergenceReport>| -> Option<ConvergenceReport> {
        // For an alternating series these tests are about Σ|aₖ|: convergence carries over,
        // divergence of Σ|aₖ| says nothing about Σaₖ.
        match r {
            Some(r) if alternating && r.converges == Some(false) => Some(ConvergenceReport {
                converges: None,
                detail: format!(
                    "{}, so Σ|aₖ| diverges and the test says nothing about the alternating series",
                    r.detail
                ),
                ..r
            }),
            Some(r) if alternating && r.converges == Some(true) => Some(ConvergenceReport {
                detail: format!("{} (absolute convergence)", r.detail),
                ..r
            }),
            other => other,
        }
    };
    out.push(if positive {
        outcome(
            "p-series test",
            absolute(p_series(&pos, k)),
            "the terms are not c/kᵖ",
        )
    } else {
        outcome("p-series test", None, only_positive)
    });
    out.push(if positive {
        outcome(
            "comparison test",
            absolute(rational_comparison(&pos, k).or_else(|| bounded_comparison(&t, k))),
            "no simple series to compare with was found",
        )
    } else {
        outcome("comparison test", None, only_positive)
    });
    out.push(if positive {
        outcome(
            "limit comparison test",
            absolute(limit_comparison(&pos, k)),
            "no p-series with a finite non-zero limit ratio was found",
        )
    } else {
        outcome("limit comparison test", None, only_positive)
    });
    out.push(outcome(
        "ratio test",
        absolute(ratio_test(&pos, k)),
        "the ratio |aₖ₊₁/aₖ| tends to 1 (or has no limit), so the test is inconclusive",
    ));
    out.push(if positive {
        outcome(
            "root test",
            absolute(root_test(&pos, k)),
            "|aₖ|^(1/k) tends to 1 or the test does not apply (no k in an exponent)",
        )
    } else {
        outcome("root test", None, only_positive)
    });
    out.push(if positive {
        outcome(
            "integral test",
            absolute(integral_test(&pos, k)),
            "the terms are not eventually decreasing or ∫ f has no closed form",
        )
    } else {
        outcome("integral test", None, only_positive)
    });
    out.push(if alternating {
        if positive && eventually_decreasing(&mag, k) && divergence_test(&mag, k).is_none() {
            TestOutcome {
                test: "alternating series test",
                verdict: Some(true),
                detail: format!("the signs alternate and |aₖ| = {pos} decreases to 0"),
            }
        } else {
            TestOutcome {
                test: "alternating series test",
                verdict: None,
                detail: "|aₖ| does not decrease to 0".into(),
            }
        }
    } else {
        TestOutcome {
            test: "alternating series test",
            verdict: None,
            detail: "the series does not alternate".into(),
        }
    });
    out.push(outcome(
        "telescoping series",
        telescoping(&t, k),
        "the terms are not a difference g(k) − g(k + m)",
    ));
    let decided: Vec<bool> = out.iter().filter_map(|o| o.verdict).collect();
    let verdict = match decided.first() {
        Some(&v) if decided.iter().all(|&w| w == v) => Some(v),
        _ => None,
    };
    if let Some(log) = steps {
        let series = Expr::new(ExprNode::BigOp {
            kind: crate::expr::BigOpKind::Sum,
            expr: t.clone(),
            var: k.into(),
            lower: Expr::one(),
            upper: Expr::infinity(),
        });
        for o in &out {
            log.push(o.test, series.clone(), series.clone(), Some(o.to_string()));
        }
    }
    Ok((verdict, out))
}

/// The radius and interval of convergence of a power series.
#[derive(Clone, Debug, PartialEq)]
pub struct PowerSeriesInterval {
    /// The centre `a` of `Σ cₖ (x − a)^(mk)`.
    pub centre: Expr,
    /// The radius (`∞` for every `x`, `0` for the centre only).
    pub radius: Expr,
    /// The interval of convergence (endpoints included when the series converges there).
    pub interval: Expr,
    /// What happens at each endpoint, with the test that decided it.
    pub endpoints: Vec<(Expr, ConvergenceReport)>,
}

/// The radius and interval of convergence of `Σ_k term(k, x)` (a power series in `x`):
/// the ratio test (then the root test) gives `L(x) = A·|x − a|ᵐ`, so the series converges
/// for `|x − a| < A^(−1/m)`; the two endpoints are tested separately.
pub fn power_series_interval(
    term: &Expr,
    k: &str,
    x: &str,
    mut steps: Option<&mut Steps>,
) -> MathResult<PowerSeriesInterval> {
    let t = simplify(term);
    if !t.contains_sym(x) {
        return Err(MathError::Domain {
            function: "interval of convergence".into(),
            detail: format!("the term does not contain {x}"),
        });
    }
    let (how, l) = match ratio_limit(&t, k) {
        Some(l) => ("ratio test", l),
        None => (
            "root test",
            root_limit(&t, k).ok_or_else(|| {
                MathError::Unsupported(format!(
                    "the radius of convergence of Σ {t}: neither the ratio nor the root test gave a limit"
                ))
            })?,
        ),
    };
    // L(x) is A·(x − a)^m (or its absolute value), 0 or ∞.
    let l_poly = strip_abs(&l);
    let (centre, radius) = if l_poly.is_zero() {
        (guess_centre(&t, x), Expr::infinity())
    } else if l.is_infinite() || is_infinite_multiple(&l_poly) {
        (guess_centre(&t, x), Expr::zero())
    } else {
        let c = as_poly(&l_poly, x).ok_or_else(|| {
            MathError::Unsupported(format!(
                "the radius of convergence: the limit {l} is not a power of (x − a)"
            ))
        })?;
        let m = c.len() - 1;
        if m == 0 {
            return Err(MathError::Unsupported(format!(
                "the radius of convergence: the limit {l} does not depend on {x}"
            )));
        }
        let lead = simplify(&c[m]);
        let a = simplify(&mul(vec![
            Expr::int(-1),
            c[m - 1].clone(),
            pow(mul(vec![Expr::int(m as i64), lead.clone()]), Expr::int(-1)),
        ]));
        // Check L = lead·(x − a)^m exactly.
        let rebuilt = mul(vec![
            lead.clone(),
            pow(
                add(vec![Expr::sym(x), mul(vec![Expr::int(-1), a.clone()])]),
                Expr::int(m as i64),
            ),
        ]);
        let diff = simplify(&add(vec![
            l_poly.clone(),
            mul(vec![Expr::int(-1), rebuilt]),
        ]));
        let diff = diff.expand().unwrap_or(diff);
        if !simplify(&diff).is_zero() {
            return Err(MathError::Unsupported(format!(
                "the radius of convergence: the limit {l} is not of the form A·(x − a)ᵐ"
            )));
        }
        let abs_lead = func(Func::Abs, vec![lead]);
        let r = simplify(&pow(abs_lead, Expr::rational(-1, m as i64)));
        (a, r)
    };
    Steps::record(
        &mut steps,
        how,
        &t,
        &l,
        Some(format!(
            "lim_{{{k}→∞}} of the {} is {l}; the series converges when this is below 1, so the centre is {x} = {centre} and the radius is R = {radius}",
            if how == "ratio test" { "ratio |aₖ₊₁/aₖ|" } else { "root |aₖ|^(1/k)" }
        )),
    );
    let mut endpoints = Vec::new();
    let interval = if radius.is_infinite() {
        Expr::new(ExprNode::Interval {
            lo: Expr::neg_infinity(),
            hi: Expr::infinity(),
            lo_closed: false,
            hi_closed: false,
        })
    } else if radius.is_zero() {
        Expr::new(ExprNode::Set(vec![centre.clone()]))
    } else {
        let lo = simplify(&add(vec![
            centre.clone(),
            mul(vec![Expr::int(-1), radius.clone()]),
        ]));
        let hi = simplify(&add(vec![centre.clone(), radius.clone()]));
        let mut closed = Vec::new();
        for end in [&lo, &hi] {
            let at_end = split_negative_bases(&simplify(&t.subs(x, end)), k);
            let report = decide(&at_end, k, &Expr::zero());
            Steps::record(
                &mut steps,
                "check the endpoint",
                &at_end,
                &at_end,
                Some(format!("at {x} = {end}: {report}")),
            );
            match report.converges {
                Some(c) => closed.push(c),
                None => {
                    return Err(MathError::Undetermined(format!(
                        "whether the series converges at the endpoint {x} = {end} ({})",
                        report.detail
                    )))
                }
            }
            endpoints.push((end.clone(), report));
        }
        Expr::new(ExprNode::Interval {
            lo,
            hi,
            lo_closed: closed[0],
            hi_closed: closed[1],
        })
    };
    Ok(PowerSeriesInterval {
        centre,
        radius,
        interval,
        endpoints,
    })
}

/// A geometric series: the ratio of consecutive terms does not depend on `k`.
fn geometric_test(t: &Expr, k: &str) -> Option<ConvergenceReport> {
    let r = simplify(&term_ratio(t, k));
    if r.contains_sym(k) {
        return None;
    }
    let v = value(&r)?.abs();
    let converges = v < 1.0;
    Some(ConvergenceReport {
        converges: Some(converges),
        test: "geometric series test".into(),
        detail: format!(
            "the common ratio is r = {r} and |r| {} 1",
            if converges { "<" } else { "≥" }
        ),
    })
}

/// `lim |aₖ₊₁/aₖ|` with `x` as a parameter.
fn ratio_limit(t: &Expr, k: &str) -> Option<Expr> {
    let r = term_ratio(t, k);
    let l = limit_in_k(&r, k)?;
    if l.is_undefined() {
        return None;
    }
    Some(simplify(&func(Func::Abs, vec![l])))
}

fn root_limit(t: &Expr, k: &str) -> Option<Expr> {
    let abs = func(Func::Abs, vec![t.clone()]);
    let root = pow(abs, pow(Expr::sym(k), Expr::int(-1)));
    let l = limit_in_k(&root, k)?;
    (!l.is_undefined()).then_some(l)
}

/// `lim_{k→∞} e`, taking factors free of `k` (such as powers of `x − a`) outside first.
fn limit_in_k(e: &Expr, k: &str) -> Option<Expr> {
    let e = simplify(e);
    let factors = match e.node() {
        ExprNode::Mul(fs) => fs.clone(),
        _ => vec![e.clone()],
    };
    let (outside, inside): (Vec<Expr>, Vec<Expr>) =
        factors.into_iter().partition(|f| !f.contains_sym(k));
    let inner = mul(inside);
    let l = if inner.contains_sym(k) {
        limit(&inner, k, &Expr::infinity(), LimitDir::Both, None).ok()?
    } else {
        inner
    };
    let mut all = outside;
    all.push(l);
    Some(simplify(&mul(all)))
}

/// `(−c)^(…k…) → (−1)^(…k…) · c^(…k…)` so that powers of the same base cancel.
fn split_negative_bases(e: &Expr, k: &str) -> Expr {
    let node = e.map_children(&mut |c| split_negative_bases(c, k));
    let rebuilt = crate::simplify::canonical(node);
    if let ExprNode::Pow(b, x) = rebuilt.node() {
        if x.contains_sym(k) {
            if let Some(n) = b.as_number() {
                if n.is_real() && n.is_negative() && !n.is_minus_one() {
                    let pos = simplify(&mul(vec![Expr::int(-1), b.clone()]));
                    return simplify(&mul(vec![
                        pow(Expr::int(-1), x.clone()),
                        pow(pos, x.clone()),
                    ]));
                }
            }
        }
    }
    rebuilt
}

/// `|u|·c → u·c` (signs do not matter for `|L| < 1` once the form is known).
fn strip_abs(e: &Expr) -> Expr {
    let node = e.map_children(&mut strip_abs);
    let rebuilt = crate::simplify::canonical(node);
    match rebuilt.node() {
        ExprNode::Func(Func::Abs, args) if args.len() == 1 => args[0].clone(),
        _ => rebuilt,
    }
}

fn is_infinite_multiple(e: &Expr) -> bool {
    e.is_infinite() || matches!(e.node(), ExprNode::Mul(fs) if fs.iter().any(Expr::is_infinite))
}

/// The centre when the radius is 0 or ∞: the `a` of a factor `(x − a)^…` in the term.
fn guess_centre(t: &Expr, x: &str) -> Expr {
    fn find(e: &Expr, x: &str) -> Option<Expr> {
        if let ExprNode::Pow(b, _) = e.node() {
            if let Some(c) = as_poly(b, x) {
                if c.len() == 2 && c[1].is_one() {
                    return Some(simplify(&mul(vec![Expr::int(-1), c[0].clone()])));
                }
            }
        }
        e.children().into_iter().find_map(|c| find(c, x))
    }
    find(t, x).unwrap_or_else(Expr::zero)
}

/// True when the value is a finite real number (used by tests and the eval wiring).
pub fn is_finite_value(e: &Expr) -> bool {
    value(e).is_some()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn k() -> Expr {
        Expr::sym("k")
    }
    fn x() -> Expr {
        Expr::sym("x")
    }
    fn n(v: i64) -> Expr {
        Expr::int(v)
    }
    fn verdict_of(o: &[TestOutcome], name: &str) -> Option<bool> {
        o.iter().find(|t| t.test == name).and_then(|t| t.verdict)
    }

    #[test]
    fn named_tests() {
        // Σ 1/k²: p-series, comparison, integral test all say converges.
        let (v, o) = series_tests(&k().pow(-2), "k", None).unwrap();
        assert_eq!(v, Some(true));
        assert_eq!(verdict_of(&o, "p-series test"), Some(true));
        assert_eq!(verdict_of(&o, "integral test"), Some(true));
        assert_eq!(verdict_of(&o, "ratio test"), None);
        // Σ 1/k: diverges.
        let (v, o) = series_tests(&k().recip(), "k", None).unwrap();
        assert_eq!(v, Some(false));
        assert_eq!(verdict_of(&o, "p-series test"), Some(false));
        // Σ (−1)^k / k: alternating series test; absolute tests say nothing.
        let alt = n(-1).pow(k()) / k();
        let (v, o) = series_tests(&alt, "k", None).unwrap();
        assert_eq!(v, Some(true));
        assert_eq!(verdict_of(&o, "alternating series test"), Some(true));
        assert_eq!(verdict_of(&o, "p-series test"), None);
        // Σ 2^k / k!: ratio test.
        let fact = func(Func::Factorial, vec![k()]);
        let (v, o) = series_tests(&(n(2).pow(k()) / fact), "k", None).unwrap();
        assert_eq!(v, Some(true));
        assert_eq!(verdict_of(&o, "ratio test"), Some(true));
        // Σ k/(k + 1): divergence test.
        let (v, o) = series_tests(&(k() / (k() + n(1))), "k", None).unwrap();
        assert_eq!(v, Some(false));
        assert_eq!(verdict_of(&o, "divergence test"), Some(false));
        // Σ (k/(2k+1))^k: root test.
        let (v, o) = series_tests(&(k() / (n(2) * k() + n(1))).pow(k()), "k", None).unwrap();
        assert_eq!(v, Some(true));
        assert_eq!(verdict_of(&o, "root test"), Some(true));
    }

    #[test]
    fn intervals_of_convergence() {
        // Σ (x − 2)^k / (k 3^k): centre 2, radius 3, [−1, 5).
        let t = (x() - n(2)).pow(k()) / (k() * n(3).pow(k()));
        let r = power_series_interval(&t, "k", "x", None).unwrap();
        assert_eq!(r.centre, n(2));
        assert_eq!(r.radius, n(3));
        assert_eq!(r.interval.to_string(), "[-1, 5)");
        // Σ x^k / k!: everywhere.
        let t = x().pow(k()) / func(Func::Factorial, vec![k()]);
        let r = power_series_interval(&t, "k", "x", None).unwrap();
        assert!(r.radius.is_infinite());
        // Σ k! x^k: only at 0.
        let t = func(Func::Factorial, vec![k()]) * x().pow(k());
        let r = power_series_interval(&t, "k", "x", None).unwrap();
        assert!(r.radius.is_zero());
        // Σ x^k / k²: [−1, 1].
        let t = x().pow(k()) / k().pow(2);
        let r = power_series_interval(&t, "k", "x", None).unwrap();
        assert_eq!(r.interval.to_string(), "[-1, 1]");
        // Σ (−1)^k x^(2k) / 4^k: radius 2, open interval (geometric at both ends).
        let t = n(-1).pow(k()) * x().pow(n(2) * k()) / n(4).pow(k());
        let r = power_series_interval(&t, "k", "x", None).unwrap();
        assert_eq!(r.radius, n(2));
        assert_eq!(r.interval.to_string(), "(-2, 2)");
    }
}
