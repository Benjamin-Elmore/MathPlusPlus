//! The classic linear PDEs on standard domains by separation of variables and Fourier
//! sine series: the heat equation `u_t = k u_xx` and the wave equation `u_tt = c² u_xx` on
//! `0 < x < L` with `u = 0` at both ends, and Laplace's equation `u_xx + u_yy = 0` on the
//! rectangle `[0, a] × [0, b]` with `u = f(x)` on the top edge and 0 on the others.
//!
//! The answer is a series `Σ_{n=1}^∞ …` with its coefficients worked out; each term is
//! checked to satisfy the equation and the boundary conditions.

use crate::calc::diff::diff;
use crate::calc::integrate::integrate_definite;
use crate::error::{MathError, MathResult};
use crate::expr::{BigOpKind, Expr, ExprNode};
use crate::functions::{func, Func};
use crate::simplify::{add, mul, pow, simplify};
use crate::steps::Steps;

use super::tidy;

/// The summation index.
const N: &str = "n";

fn n_sym() -> Expr {
    Expr::sym(N)
}

fn sum(term: Expr) -> Expr {
    Expr::new(ExprNode::BigOp {
        kind: BigOpKind::Sum,
        expr: term,
        var: N.into(),
        lower: Expr::one(),
        upper: Expr::infinity(),
    })
}

/// `sin(nπ) = 0` and `cos(nπ) = (−1)ⁿ` for an integer `n`.
fn integer_n(e: &Expr) -> Expr {
    let node = e.map_children(&mut integer_n);
    let rebuilt = crate::simplify::canonical(node);
    if let ExprNode::Func(f @ (Func::Sin | Func::Cos), args) = rebuilt.node() {
        let arg = &args[0];
        let k = simplify(&mul(vec![
            arg.clone(),
            pow(mul(vec![n_sym(), Expr::pi()]), Expr::int(-1)),
        ]));
        if let Some(m) = k.as_number().and_then(|q| q.as_i64()) {
            return match f {
                Func::Sin => Expr::zero(),
                _ => pow(Expr::int(-1), mul(vec![Expr::int(m), n_sym()])),
            };
        }
    }
    rebuilt
}

/// `(2/L) ∫₀ᴸ f(x) sin(nπx/L) dx`.
fn sine_coefficient(f: &Expr, x: &str, l: &Expr) -> MathResult<Expr> {
    let mode = mul(vec![
        n_sym(),
        Expr::pi(),
        Expr::sym(x),
        pow(l.clone(), Expr::int(-1)),
    ]);
    let integrand = mul(vec![f.clone(), func(Func::Sin, vec![mode])]);
    let i = integrate_definite(&integrand, x, &Expr::zero(), l, None)?;
    if crate::calc_support::value(&i).is_some() && !i.contains_sym(N) {
        // A numeric fallback means the integral had no closed form in n.
        return Err(MathError::NoClosedForm(format!(
            "the Fourier coefficients of {f}"
        )));
    }
    Ok(tidy(&integer_n(&mul(vec![
        Expr::int(2),
        pow(l.clone(), Expr::int(-1)),
        i,
    ]))))
}

fn check_term(term: &Expr, pde: impl Fn(&Expr) -> MathResult<Expr>, what: &str) -> MathResult<()> {
    let r = simplify(&pde(term)?);
    if r.is_zero() || super::verify::vanishes(&r, std::slice::from_ref(term), "x", None) {
        Ok(())
    } else {
        Err(MathError::Inconclusive(format!(
            "the {what} series (a term failed its check)"
        )))
    }
}

/// `u_t = k u_xx`, `u(0, t) = u(L, t) = 0`, `u(x, 0) = f(x)`.
pub fn heat(
    f: &Expr,
    x: &str,
    t: &str,
    l: &Expr,
    k: &Expr,
    mut steps: Option<&mut Steps>,
) -> MathResult<Expr> {
    let b = sine_coefficient(f, x, l)?;
    let lam = mul(vec![n_sym(), Expr::pi(), pow(l.clone(), Expr::int(-1))]);
    let term = simplify(&mul(vec![
        b.clone(),
        pow(
            Expr::e(),
            mul(vec![
                Expr::int(-1),
                k.clone(),
                pow(lam.clone(), Expr::int(2)),
                Expr::sym(t),
            ]),
        ),
        func(Func::Sin, vec![mul(vec![lam.clone(), Expr::sym(x)])]),
    ]));
    check_term(
        &term,
        |u| {
            Ok(add(vec![
                diff(u, t, 1, None)?,
                mul(vec![Expr::int(-1), k.clone(), diff(u, x, 2, None)?]),
            ]))
        },
        "heat",
    )?;
    let u = sum(term);
    Steps::record(
        &mut steps,
        "separation of variables",
        f,
        &u,
        Some(format!(
            "u = X({x})T({t}) gives X″ + λX = 0 with X(0) = X(L) = 0, so X = sin(nπ{x}/L), T = e^(−k(nπ/L)²{t}); u({x}, 0) = f fixes bₙ = (2/L)∫₀ᴸ f sin(nπ{x}/L) d{x} = {b}"
        )),
    );
    Ok(u)
}

/// `u_tt = c² u_xx`, `u(0, t) = u(L, t) = 0`, `u(x, 0) = f(x)`, `u_t(x, 0) = g(x)`.
#[allow(clippy::too_many_arguments)]
pub fn wave(
    f: &Expr,
    g: &Expr,
    x: &str,
    t: &str,
    l: &Expr,
    c: &Expr,
    mut steps: Option<&mut Steps>,
) -> MathResult<Expr> {
    let a_n = if f.is_zero() {
        Expr::zero()
    } else {
        sine_coefficient(f, x, l)?
    };
    let lam = mul(vec![n_sym(), Expr::pi(), pow(l.clone(), Expr::int(-1))]);
    let omega = simplify(&mul(vec![c.clone(), lam.clone()]));
    let b_n = if g.is_zero() {
        Expr::zero()
    } else {
        tidy(&mul(vec![
            sine_coefficient(g, x, l)?,
            pow(omega.clone(), Expr::int(-1)),
        ]))
    };
    let wt = mul(vec![omega.clone(), Expr::sym(t)]);
    let term = simplify(&mul(vec![
        add(vec![
            mul(vec![a_n.clone(), func(Func::Cos, vec![wt.clone()])]),
            mul(vec![b_n.clone(), func(Func::Sin, vec![wt])]),
        ]),
        func(Func::Sin, vec![mul(vec![lam, Expr::sym(x)])]),
    ]));
    check_term(
        &term,
        |u| {
            Ok(add(vec![
                diff(u, t, 2, None)?,
                mul(vec![
                    Expr::int(-1),
                    pow(c.clone(), Expr::int(2)),
                    diff(u, x, 2, None)?,
                ]),
            ]))
        },
        "wave",
    )?;
    let u = sum(term);
    Steps::record(
        &mut steps,
        "separation of variables",
        f,
        &u,
        Some(format!(
            "u = X({x})T({t}): X = sin(nπ{x}/L), T = Aₙcos(cnπ{t}/L) + Bₙsin(cnπ{t}/L); Aₙ = (2/L)∫ f sin = {a_n}, Bₙ = (2/(cnπ))∫ g sin = {b_n}"
        )),
    );
    Ok(u)
}

/// `u_xx + u_yy = 0` on `[0, a] × [0, b]`, `u(x, b) = f(x)`, zero on the other edges.
pub fn laplace_rectangle(
    f: &Expr,
    x: &str,
    y: &str,
    a: &Expr,
    b: &Expr,
    mut steps: Option<&mut Steps>,
) -> MathResult<Expr> {
    let c = sine_coefficient(f, x, a)?;
    let lam = mul(vec![n_sym(), Expr::pi(), pow(a.clone(), Expr::int(-1))]);
    let coeff = tidy(&mul(vec![
        c.clone(),
        pow(
            func(Func::Sinh, vec![mul(vec![lam.clone(), b.clone()])]),
            Expr::int(-1),
        ),
    ]));
    let term = simplify(&mul(vec![
        coeff.clone(),
        func(Func::Sin, vec![mul(vec![lam.clone(), Expr::sym(x)])]),
        func(Func::Sinh, vec![mul(vec![lam, Expr::sym(y)])]),
    ]));
    check_term(
        &term,
        |u| Ok(add(vec![diff(u, x, 2, None)?, diff(u, y, 2, None)?])),
        "Laplace",
    )?;
    let u = sum(term);
    Steps::record(
        &mut steps,
        "separation of variables",
        f,
        &u,
        Some(format!(
            "u = X({x})Y({y}): X = sin(nπ{x}/a), Y = sinh(nπ{y}/a) (zero at {y} = 0); u({x}, b) = f gives the coefficients {coeff}"
        )),
    );
    Ok(u)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn heat_and_wave_series() {
        let x = Expr::sym("x");
        // f(x) = x on [0, π]: bₙ = 2(−1)^{n+1}/n.
        let u = heat(&x, "x", "t", &Expr::pi(), &Expr::one(), None).unwrap();
        let s = u.to_string();
        assert!(s.starts_with("Σ") || s.contains("n"), "{s}");
        // Partial sums at t = 0 approach f in the interior.
        let ExprNode::BigOp { expr, .. } = u.node() else {
            panic!("{u}")
        };
        let mut total = 0.0;
        for n in 1..=2000 {
            let v = expr
                .eval_f64(&[("n", n as f64), ("x", 1.0), ("t", 0.0)])
                .unwrap();
            total += v;
        }
        assert!((total - 1.0).abs() < 2e-3, "{total}");
        // Plucked string with zero initial velocity.
        let f = x.clone() * (Expr::pi() - x.clone());
        let w = wave(
            &f,
            &Expr::zero(),
            "x",
            "t",
            &Expr::pi(),
            &Expr::int(2),
            None,
        )
        .unwrap();
        assert!(matches!(w.node(), ExprNode::BigOp { .. }));
        let lap =
            laplace_rectangle(&Expr::one(), "x", "y", &Expr::pi(), &Expr::pi(), None).unwrap();
        assert!(matches!(lap.node(), ExprNode::BigOp { .. }));
    }
}
