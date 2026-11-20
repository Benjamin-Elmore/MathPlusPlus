//! Series solutions of linear equations: power series about an ordinary point (the first
//! coefficients free, the rest from the recurrence the equation gives), and Frobenius
//! series `xʳ Σ cₖ xᵏ` about a regular singular point of a second-order equation.
//!
//! The answer is a truncated series; its residual vanishes to the order shown.

use num_traits::ToPrimitive;

use crate::calc::series::series as taylor;
use crate::domains::solve::poly::roots;
use crate::error::{MathError, MathResult};
use crate::expr::Expr;
use crate::simplify::{add, mul, pow, simplify};
use crate::steps::Steps;

use super::parse::Ode;
use super::{linsys, tidy, Candidate, Condition, Form};
use crate::tools_b::util::value;

/// Terms computed (the series is exact below `x^TERMS`).
pub const TERMS: u32 = 8;

fn neg(e: Expr) -> Expr {
    mul(vec![Expr::int(-1), e])
}

/// The coefficients of `e` in powers of `x − a` up to `order` (Taylor, so any analytic
/// coefficient works).
fn coeffs(e: &Expr, x: &str, a: &Expr, order: u32) -> Option<Vec<Expr>> {
    if e.is_zero() {
        return Some(vec![Expr::zero(); order as usize]);
    }
    let s = taylor(e, x, a, order, None).ok()?;
    if s.valuation < 0 {
        return None;
    }
    Some((0..order as i64).map(|k| s.coeff(k)).collect())
}

/// A power-series (or Frobenius) solution of a linear equation.
pub(crate) fn solve(
    ode: &Ode,
    conditions: &[Condition],
    consts: &[String],
    steps: &mut Steps,
) -> MathResult<Candidate> {
    let (a, g) = ode.linear_coefficients().ok_or_else(|| {
        MathError::Unsupported("a series solution of a non-linear equation".into())
    })?;
    let x = ode.x.as_str();
    let centre = conditions.first().map_or_else(Expr::zero, |c| c.at.clone());
    // Every condition must be at the centre (it fixes a first coefficient); others would
    // be ignored by a truncated series, so they are refused.
    if conditions
        .iter()
        .any(|c| c.at != centre || c.order >= ode.order)
    {
        return Err(MathError::Unsupported(
            "a series solution with conditions at different points".into(),
        ));
    }
    let lead_at = simplify(&a[a.len() - 1].subs(x, &centre));
    let ordinary = value(&lead_at).is_some_and(|v| v.abs() > 1e-12);
    if ordinary {
        ordinary_point(ode, &a, &g, &centre, conditions, consts, steps)
    } else if ode.order == 2 && g.is_zero() && centre.is_zero() {
        frobenius(ode, &a, consts, steps)
    } else {
        Err(MathError::Unsupported(format!(
            "a series solution about the singular point {x} = {centre}"
        )))
    }
}

#[allow(clippy::needless_range_loop)]
fn ordinary_point(
    ode: &Ode,
    a: &[Expr],
    g: &Expr,
    centre: &Expr,
    conditions: &[Condition],
    consts: &[String],
    steps: &mut Steps,
) -> MathResult<Candidate> {
    let x = ode.x.as_str();
    let n = ode.order as usize;
    let total = TERMS as usize + n;
    let fail = || MathError::Unsupported("a power-series solution here".into());
    let ac: Vec<Vec<Expr>> = a
        .iter()
        .map(|ak| coeffs(ak, x, centre, total as u32))
        .collect::<Option<_>>()
        .ok_or_else(fail)?;
    let gc = coeffs(g, x, centre, total as u32).ok_or_else(fail)?;
    // c₀ … c_{n−1}: the conditions' values when given at the centre, else constants.
    let mut c: Vec<Expr> = Vec::with_capacity(total + n);
    let mut fact = Expr::one();
    for k in 0..n {
        if k > 0 {
            fact = simplify(&mul(vec![fact, Expr::int(k as i64)]));
        }
        let given = conditions
            .iter()
            .find(|cd| cd.order == k as u32 && cd.at == *centre)
            .map(|cd| {
                simplify(&mul(vec![
                    cd.value.clone(),
                    pow(fact.clone(), Expr::int(-1)),
                ]))
            });
        c.push(given.unwrap_or_else(|| Expr::sym(&consts[k])));
    }
    // The coefficient of (x − a)^m in L[y] − g fixes c_{m+n}.
    for m in 0..TERMS as usize {
        let unknown = "\u{1}c";
        let mut trial = c.clone();
        trial.push(Expr::sym(unknown));
        let mut terms = vec![neg(gc[m].clone())];
        for (k, ack) in ac.iter().enumerate() {
            // y⁽ᵏ⁾ has coefficient (j+k)!/j! c_{j+k} at power j.
            for j in 0..=m {
                let idx = j + k;
                if idx >= trial.len() {
                    continue;
                }
                let mut f = 1i64;
                for q in (j + 1)..=idx {
                    f *= q as i64;
                }
                terms.push(mul(vec![
                    ack[m - j].clone(),
                    Expr::int(f),
                    trial[idx].clone(),
                ]));
            }
        }
        let eq = simplify(&add(terms));
        let (row, rhs) = linsys::linear_form(&eq, &[unknown.to_string()]).ok_or_else(fail)?;
        let v = linsys::solve(vec![row], vec![rhs]).ok_or_else(fail)?;
        c.push(tidy(&v[0]));
    }
    let base = simplify(&add(vec![Expr::sym(x), neg(centre.clone())]));
    let terms = tidy(&add(c
        .iter()
        .enumerate()
        .map(|(k, ck)| mul(vec![ck.clone(), pow(base.clone(), Expr::int(k as i64))]))
        .collect()));
    let order = c.len() as u32;
    steps.push(
        "power series",
        ode.original.clone(),
        Expr::equation(Expr::sym(&ode.y), terms.clone()),
        Some(format!(
            "{x} = {centre} is an ordinary point; put {y} = Σ cₖ({base})ᵏ and match powers: each power fixes the next coefficient, giving {y} = {terms} + O(({base})^{order})",
            y = ode.y
        )),
    );
    let mut cand = Candidate {
        form: Form::Series {
            terms,
            centre: centre.clone(),
            order,
        },
        method: "power series",
        notes: vec![format!(
            "the series is exact up to {}",
            pow(base.clone(), Expr::int(i64::from(order) - 1))
        )],
        steps: Steps::new(),
    };
    cand.notes.dedup();
    Ok(cand)
}

/// `x² y″ + x p(x) y′ + q(x) y = 0` about `x = 0`.
fn frobenius(ode: &Ode, a: &[Expr], consts: &[String], steps: &mut Steps) -> MathResult<Candidate> {
    let x = ode.x.as_str();
    let xs = Expr::sym(x);
    let fail = || {
        MathError::Unsupported(
            "a Frobenius series here (the point is not a regular singular point)".into(),
        )
    };
    // p = x a₁/a₂, q = x² a₀/a₂ must be analytic at 0.
    let p = simplify(&mul(vec![
        xs.clone(),
        a[1].clone(),
        pow(a[2].clone(), Expr::int(-1)),
    ]));
    let q = simplify(&mul(vec![
        pow(xs.clone(), Expr::int(2)),
        a[0].clone(),
        pow(a[2].clone(), Expr::int(-1)),
    ]));
    let total = TERMS + 1;
    let pc = coeffs(&p, x, &Expr::zero(), total).ok_or_else(fail)?;
    let qc = coeffs(&q, x, &Expr::zero(), total).ok_or_else(fail)?;
    // Indicial equation r(r − 1) + p₀ r + q₀ = 0.
    let indicial = vec![
        qc[0].clone(),
        simplify(&add(vec![pc[0].clone(), Expr::int(-1)])),
        Expr::one(),
    ];
    let rs = roots(&indicial)?;
    let mut real: Vec<(Expr, f64)> = rs
        .into_iter()
        .filter_map(|r| {
            let z = r.value.eval_complex(&crate::eval::EmptyEnv).ok()?;
            (z.im.abs() < 1e-12).then(|| (r.value.clone(), z.re))
        })
        .collect();
    if real.is_empty() {
        return Err(MathError::Unsupported(
            "a Frobenius series with complex indicial roots".into(),
        ));
    }
    real.sort_by(|u, v| v.1.total_cmp(&u.1));
    real.dedup_by(|u, v| (u.1 - v.1).abs() < 1e-12);
    let mut solutions = Vec::new();
    let integer_gap = real.len() == 2 && {
        let gap = real[0].1 - real[1].1;
        (gap - gap.round()).abs() < 1e-12
    };
    let usable = if integer_gap { &real[..1] } else { &real[..] };
    for (r, _) in usable {
        // cₖ [ (r+k)(r+k−1) + p₀(r+k) + q₀ ] = −Σ_{j<k} cⱼ [ (r+j) p_{k−j} + q_{k−j} ]
        let mut c = vec![Expr::one()];
        for k in 1..TERMS as usize {
            let rk = simplify(&add(vec![r.clone(), Expr::int(k as i64)]));
            let f_k = simplify(&add(vec![
                mul(vec![rk.clone(), add(vec![rk.clone(), Expr::int(-1)])]),
                mul(vec![pc[0].clone(), rk.clone()]),
                qc[0].clone(),
            ]));
            if value(&f_k).is_some_and(|v| v.abs() < 1e-12) {
                return Err(fail());
            }
            let mut sum = Vec::new();
            for (j, cj) in c.iter().enumerate() {
                let rj = add(vec![r.clone(), Expr::int(j as i64)]);
                sum.push(mul(vec![
                    cj.clone(),
                    add(vec![mul(vec![rj, pc[k - j].clone()]), qc[k - j].clone()]),
                ]));
            }
            c.push(tidy(&mul(vec![neg(add(sum)), pow(f_k, Expr::int(-1))])));
        }
        let poly = add(c
            .iter()
            .enumerate()
            .map(|(k, ck)| mul(vec![ck.clone(), pow(xs.clone(), Expr::int(k as i64))]))
            .collect());
        solutions.push(tidy(&mul(vec![pow(xs.clone(), r.clone()), poly])));
    }
    let general = simplify(&add(solutions
        .iter()
        .zip(consts)
        .map(|(s, c)| mul(vec![Expr::sym(c), s.clone()]))
        .collect()));
    let roots_text: Vec<String> = real.iter().map(|(r, _)| r.to_string()).collect();
    steps.push(
        "Frobenius series",
        ode.original.clone(),
        Expr::equation(Expr::sym(&ode.y), general.clone()),
        Some(format!(
            "{x} = 0 is a regular singular point (p = {p}, q = {q} are analytic); the indicial equation r(r − 1) + p₀r + q₀ = 0 has roots {}; each gives {y} = {x}^r Σ cₖ{x}ᵏ with the recurrence from matching powers",
            roots_text.join(", "),
            y = ode.y
        )),
    );
    let mut notes = vec![format!("the series are exact up to {x}^{}", TERMS - 1)];
    if integer_gap {
        notes.push("the indicial roots differ by an integer, so only the first Frobenius solution is given (the second one has a logarithm term)".into());
    }
    let _ = TERMS.to_u32();
    Ok(Candidate {
        form: Form::Series {
            terms: general,
            centre: Expr::zero(),
            order: TERMS,
        },
        method: "Frobenius series",
        notes,
        steps: Steps::new(),
    })
}
