//! Produces and checks `examples/fields.expected.txt`: each line of
//! `examples/fields.mpp` that ends in `= ?` is evaluated here through the domain
//! registry with the same operator and arguments, so the expected output is fixed
//! before the evaluator wires these operators into documents.

use mpp_core::domains::{Ctx, Registry};
use mpp_core::{Expr, ExprNode, Func, RelOp};

fn t(v: Vec<Expr>) -> Expr {
    Expr::new(ExprNode::Tuple(v))
}

fn ints(v: &[i64]) -> Expr {
    t(v.iter().map(|&x| Expr::int(x)).collect())
}

fn mat(rows: &[&[Expr]]) -> Expr {
    Expr::matrix(rows.iter().map(|r| r.to_vec()).collect()).expect("matrix")
}

fn q(p: i64, d: i64) -> Expr {
    Expr::rational(p, d)
}

/// `(label as written in the document, operator, arguments)`.
fn cases() -> Vec<(&'static str, &'static str, Vec<Expr>)> {
    let (x, y) = (Expr::sym("x"), Expr::sym("y"));
    let (qq, p, tt) = (Expr::sym("q"), Expr::sym("p"), Expr::sym("t"));
    let golden = (Expr::one() + Expr::int(5).sqrt()) / Expr::int(2);
    let unit = |v: &Expr| t(vec![v.clone(), Expr::zero(), Expr::one()]);
    let inside = Expr::relation(RelOp::Le, x.pow(2) + y.pow(2), Expr::one());
    let k4 = t([(1, 2), (1, 3), (1, 4), (2, 3), (2, 4), (3, 4)]
        .iter()
        .map(|&(a, b)| ints(&[a, b]))
        .collect());
    let sin15 = Expr::func(Func::Sin, vec![Expr::pi() / Expr::int(12)]).expect("sin");
    vec![
        ("τ(360)", "divcount", vec![Expr::int(360)]),
        ("σ(28)", "divsum", vec![Expr::int(28)]),
        ("μ(30)", "mobius", vec![Expr::int(30)]),
        ("π(1000)", "primepi", vec![Expr::int(1000)]),
        ("|8|₂", "padicabs", vec![Expr::int(2), Expr::int(8)]),
        (
            "√7 as a continued fraction",
            "contfrac",
            vec![Expr::int(7).sqrt()],
        ),
        (
            "convergents of φ",
            "convergents",
            vec![golden, Expr::int(6)],
        ),
        ("5 in ℤ[i]", "gaussfactor", vec![Expr::int(5)]),
        (
            "x · x² in 𝔽₈",
            "gfmul",
            vec![
                Expr::int(2),
                Expr::int(3),
                ints(&[0, 1, 0]),
                ints(&[0, 0, 1]),
            ],
        ),
        ("ζ(2)", "zeta", vec![Expr::int(2)]),
        ("ζ(3)", "zeta", vec![Expr::int(3)]),
        (
            "L(1, χ₄)",
            "dirichletl",
            vec![Expr::one(), ints(&[1, 0, -1, 0])],
        ),
        ("erf(1/2)", "erf", vec![q(1, 2)]),
        ("W(1)", "lambertw", vec![Expr::one()]),
        ("W₋₁(−1/10)", "lambertw", vec![q(-1, 10), Expr::int(-1)]),
        ("J₀(1)", "besselj", vec![Expr::zero(), Expr::one()]),
        ("B(1/2, 1/2)", "beta", vec![q(1, 2), q(1, 2)]),
        ("P₂(x)", "legendrep", vec![Expr::int(2), x.clone()]),
        ("K(sin 15°)", "ellipk", vec![sin15]),
        (
            "⟨δ′, sin⟩",
            "diracpair",
            vec![
                Expr::func(Func::Sin, vec![x.clone()]).expect("sin"),
                x.clone(),
                Expr::one(),
            ],
        ),
        ("DFT of (1, 2, 3, 4)", "dft", vec![ints(&[1, 2, 3, 4])]),
        ("C₁₀", "catalan", vec![Expr::int(10)]),
        ("f^(4,3,1)", "hooklength", vec![ints(&[4, 3, 1])]),
        ("spanning trees of K₄", "spanningtrees", vec![k4]),
        (
            "R(3,3) ≤ 6",
            "ramsey",
            vec![Expr::int(3), Expr::int(3), Expr::int(6)],
        ),
        (
            "Hamming [7,4] code",
            "codeparams",
            vec![
                mat(&[
                    &[1, 0, 0, 0, 1, 1, 0].map(Expr::int),
                    &[0, 1, 0, 0, 1, 0, 1].map(Expr::int),
                    &[0, 0, 1, 0, 0, 1, 1].map(Expr::int),
                    &[0, 0, 0, 1, 1, 1, 1].map(Expr::int),
                ]),
                Expr::int(2),
            ],
        ),
        (
            "stationary π",
            "stationary",
            vec![mat(&[&[q(1, 2), q(1, 2)], &[q(1, 4), q(3, 4)]])],
        ),
        (
            "π̂ (seed 42, N = 10⁵)",
            "montecarlo",
            vec![
                Expr::int(4) * inside,
                t(vec![unit(&x), unit(&y)]),
                Expr::int(100_000),
                Expr::int(42),
            ],
        ),
        (
            "H(1/2, 1/4, 1/4)",
            "entropy",
            vec![t(vec![q(1, 2), q(1, 4), q(1, 4)])],
        ),
        ("BSC(0.11) capacity", "bsccapacity", vec![Expr::float(0.11)]),
        (
            "exp(tJ)",
            "expm",
            vec![
                mat(&[&[Expr::zero(), Expr::int(-1)], &[Expr::one(), Expr::zero()]]),
                tt,
            ],
        ),
        ("characters of S₃", "chartable", vec![Expr::sym("S3")]),
        (
            "value of [[3,1],[2,4]]",
            "gamevalue",
            vec![mat(&[
                &[Expr::int(3), Expr::one()],
                &[Expr::int(2), Expr::int(4)],
            ])],
        ),
        (
            "Hamilton for H = (p² + q²)/2",
            "hamilton",
            vec![(p.pow(2) + qq.pow(2)) / Expr::int(2), qq, p],
        ),
        (
            "put from call",
            "putfromcall",
            vec![
                Expr::int(10),
                Expr::int(100),
                Expr::int(100),
                q(1, 20),
                Expr::one(),
            ],
        ),
    ]
}

fn render() -> String {
    let reg = Registry::standard();
    let mut out = String::new();
    for (label, op, args) in cases() {
        let mut ctx = Ctx::new();
        let shown = match reg.call(op, &args, &mut ctx) {
            // An exact special value (`ζ(3)`, `J₀(1)`) is kept exact: shown by its decimal.
            Ok(v) => match v.eval_f64(&mpp_core::EmptyEnv) {
                Ok(x) if v.as_number().is_none() && matches!(v.node(), ExprNode::Apply(..)) => {
                    mpp_core::number::format_f64(x, 10)
                }
                _ => v.to_string(),
            },
            Err(e) => e.to_string(),
        };
        out.push_str(&format!("{label} = ? → {shown}\n"));
    }
    out
}

#[test]
fn fields_example_matches_expected_output() {
    let expected = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../examples/fields.expected.txt"
    ));
    let actual = render();
    let norm = |s: &str| s.replace("\r\n", "\n").trim_end().to_string();
    assert_eq!(norm(&actual), norm(expected), "actual output:\n{actual}");
}
