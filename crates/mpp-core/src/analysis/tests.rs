//! Textb∞k curve-sketching problems with known answers.

use super::*;
use crate::testkit::p;

fn an(f: &str) -> Analysis {
    Analysis::new(&p(f), "x", None).unwrap()
}

fn closed(lo: &str, hi: &str) -> Piece {
    Piece {
        lo: p(lo),
        hi: p(hi),
        lo_closed: true,
        hi_closed: true,
    }
}

fn on(f: &str, lo: &str, hi: &str) -> Analysis {
    Analysis::new(&p(f), "x", Some(closed(lo, hi))).unwrap()
}

fn s(e: &Expr) -> String {
    e.to_string()
}

fn all(v: &[Expr]) -> Vec<String> {
    v.iter().map(s).collect()
}

#[test]
fn domains_of_textbook_functions() {
    assert_eq!(
        s(&an("sqrt(4 - x^2)").domain().unwrap().to_expr()),
        "[-2, 2]"
    );
    assert_eq!(
        s(&an("1/(x^2 - 1)").domain().unwrap().to_expr()),
        "(-∞, -1) ∪ (-1, 1) ∪ (1, ∞)"
    );
    assert_eq!(s(&an("ln(x - 2)").domain().unwrap().to_expr()), "(2, ∞)");
    assert_eq!(
        s(&an("ln(x^2 - 4)").domain().unwrap().to_expr()),
        "(-∞, -2) ∪ (2, ∞)"
    );
    assert_eq!(s(&an("x^3 - 3*x").domain().unwrap().to_expr()), "(-∞, ∞)");
    assert_eq!(
        s(&an("asin(2*x)").domain().unwrap().to_expr()),
        "[-1/2, 1/2]"
    );
    assert_eq!(
        s(&an("sqrt(x)/(x - 4)").domain().unwrap().to_expr()),
        "[0, 4) ∪ (4, ∞)"
    );
}

#[test]
fn ranges() {
    assert_eq!(s(&an("x^2 - 4*x + 7").range().unwrap().to_expr()), "[3, ∞)");
    assert_eq!(s(&an("sqrt(4 - x^2)").range().unwrap().to_expr()), "[0, 2]");
    assert_eq!(s(&an("1/x").range().unwrap().to_expr()), "(-∞, 0) ∪ (0, ∞)");
    assert_eq!(s(&an("exp(x)").range().unwrap().to_expr()), "(0, ∞)");
    assert_eq!(
        s(&an("x/(x^2 + 1)").range().unwrap().to_expr()),
        "[-1/2, 1/2]"
    );
    assert_eq!(
        s(&on("sin(x)", "0", "2*pi").range().unwrap().to_expr()),
        "[-1, 1]"
    );
}

#[test]
fn critical_points_and_first_derivative_test() {
    let c = an("x^3 - 3*x").critical_points().unwrap();
    assert_eq!(c.len(), 2);
    assert_eq!(
        (s(&c[0].x), s(&c[0].y), c[0].kind),
        ("-1".into(), "2".into(), Kind::LocalMax)
    );
    assert_eq!(
        (s(&c[1].x), s(&c[1].y), c[1].kind),
        ("1".into(), "-2".into(), Kind::LocalMin)
    );
    // x^3 has a stationary point that is not an extremum.
    let c = an("x^3").critical_points().unwrap();
    assert_eq!(c.len(), 1);
    assert_eq!(c[0].kind, Kind::Neither);
    // |x| and x^(2/3): f′ is undefined at 0, a minimum.
    let c = an("abs(x)").critical_points().unwrap();
    assert_eq!(
        (s(&c[0].x), c[0].kind, c[0].stationary),
        ("0".into(), Kind::LocalMin, false)
    );
    let c = an("x^(2/3)").critical_points().unwrap();
    assert_eq!((s(&c[0].x), c[0].kind), ("0".into(), Kind::LocalMin));
    // x e^{-x}: maximum at 1.
    let c = an("x*exp(-x)").critical_points().unwrap();
    assert_eq!((s(&c[0].x), c[0].kind), ("1".into(), Kind::LocalMax));
    assert_eq!(s(&c[0].y), s(&p("exp(-1)")));
}

#[test]
fn closed_interval_method() {
    // Stewart: f(x) = x^3 − 3x^2 + 1 on [−1/2, 4]: max 17 at 4, min −3 at 2.
    let (max, min) = on("x^3 - 3*x^2 + 1", "-1/2", "4")
        .absolute_extrema()
        .unwrap();
    assert_eq!(max.value.map(|v| s(&v)), Some("17".into()));
    assert_eq!(all(&max.at), vec!["4"]);
    assert_eq!(min.value.map(|v| s(&v)), Some("-3".into()));
    assert_eq!(all(&min.at), vec!["2"]);
    // On all of ℝ a cubic has neither.
    let (max, min) = an("x^3 - 3*x").absolute_extrema().unwrap();
    assert!(max.value.is_none() && min.value.is_none());
    // x^2 + 1 has an absolute minimum 1 and no maximum.
    let (max, min) = an("x^2 + 1").absolute_extrema().unwrap();
    assert!(max.value.is_none());
    assert_eq!(min.value.map(|v| s(&v)), Some("1".into()));
    // 1/(1 + x^2): max 1 at 0, no min (it only approaches 0).
    let (max, min) = an("1/(1 + x^2)").absolute_extrema().unwrap();
    assert_eq!(max.value.map(|v| s(&v)), Some("1".into()));
    assert!(min.value.is_none());
}

#[test]
fn increasing_decreasing_and_concavity() {
    let mut a = an("x^3 - 3*x");
    let (up, down) = a.monotonicity().unwrap();
    assert_eq!(s(&up.to_expr()), "(-∞, -1) ∪ (1, ∞)");
    assert_eq!(s(&down.to_expr()), "(-1, 1)");
    let (cu, cd) = a.concavity().unwrap();
    assert_eq!(s(&cu.to_expr()), "(0, ∞)");
    assert_eq!(s(&cd.to_expr()), "(-∞, 0)");
    assert_eq!(all(&a.inflection_points().unwrap()), vec!["(0, 0)"]);
    // x^3 increases on all of ℝ (joined across 0); x^4 is concave up everywhere.
    assert_eq!(s(&an("x^3").monotonicity().unwrap().0.to_expr()), "(-∞, ∞)");
    let mut q = an("x^4");
    assert_eq!(s(&q.concavity().unwrap().0.to_expr()), "(-∞, ∞)");
    assert!(q.inflection_points().unwrap().is_empty());
    // 1/x decreases on each side separately.
    assert_eq!(
        s(&an("1/x").monotonicity().unwrap().1.to_expr()),
        "(-∞, 0) ∪ (0, ∞)"
    );
    // x e^{-x}: inflection at (2, 2/e^2).
    let infl = an("x*exp(-x)").inflection_points().unwrap();
    assert_eq!(all(&infl), vec![format!("(2, {})", s(&p("2*exp(-2)")))]);
}

#[test]
fn intercepts() {
    let (xs, y) = an("x^2 - x - 6").intercepts().unwrap();
    assert_eq!(all(&xs), vec!["(-2, 0)", "(3, 0)"]);
    assert_eq!(y.map(|v| s(&v)), Some("(0, -6)".into()));
    let (xs, y) = an("ln(x)").intercepts().unwrap();
    assert_eq!(all(&xs), vec!["(1, 0)"]);
    assert!(y.is_none());
}

#[test]
fn asymptotes() {
    let (v, h, sl) = an("(2*x^2 + 1)/(x^2 - 1)").asymptotes().unwrap();
    assert_eq!(all(&v), vec!["x = -1", "x = 1"]);
    assert_eq!(all(&h), vec!["y = 2"]);
    assert!(sl.is_empty());
    let (v, h, sl) = an("(x^2 + 1)/(x - 1)").asymptotes().unwrap();
    assert_eq!(all(&v), vec!["x = 1"]);
    assert!(h.is_empty());
    assert_eq!(all(&sl), vec!["y = x + 1"]);
    let (v, h, _) = an("exp(x)").asymptotes().unwrap();
    assert!(v.is_empty());
    assert_eq!(all(&h), vec!["y = 0"]);
    let (v, _, _) = an("ln(x)").asymptotes().unwrap();
    assert_eq!(all(&v), vec!["x = 0"]);
}

#[test]
fn tangent_normal_linearisation() {
    let mut a = an("x^2");
    assert_eq!(s(&a.tangent(&p("1")).unwrap()), "y = 2*x - 1");
    assert_eq!(s(&a.normal(&p("1")).unwrap()), "y = -x/2 + 3/2");
    assert_eq!(s(&a.normal(&p("0")).unwrap()), "x = 0");
    let mut r = an("sqrt(x)");
    assert_eq!(s(&r.linearisation(&p("4")).unwrap()), "L(x) = x/4 + 1");
    // Vertical tangent of the cube r∞t at 0.
    assert_eq!(s(&an("x^(1/3)").tangent(&p("0")).unwrap()), "x = 0");
    // |x| has a corner at 0, and ln is undefined at −1.
    assert!(an("abs(x)").tangent(&p("0")).is_err());
    assert!(an("ln(x)").tangent(&p("-1")).is_err());
}

#[test]
fn mean_value_and_intermediate_value() {
    // f = x^3 − x on [0, 2]: average rate 3, c = 2/√3.
    let t = an("x^3 - x").mean_value(&closed("0", "2")).unwrap();
    assert_eq!(
        table::row(&t, "average rate of change").map(|v| s(&v)),
        Some("3".into())
    );
    let cs = table::row(&t, "c with f′(c) equal to it").unwrap();
    assert_eq!(s(&cs), format!("{{{}}}", s(&p("2/sqrt(3)"))));
    // 1/x on [−1, 1] is not continuous.
    let t = an("1/x").mean_value(&closed("-1", "1")).unwrap();
    assert_eq!(
        table::row(&t, "continuous on [-1, 1]"),
        Some(Expr::new(ExprNode::Bool(false)))
    );
    // IVT: x^3 + x − 1 has a r∞t in [0, 1].
    let t = an("x^3 + x - 1")
        .intermediate_value(&closed("0", "1"), &Expr::zero())
        .unwrap();
    assert_eq!(
        table::row(&t, "0 lies between f(0) and f(1)"),
        Some(Expr::new(ExprNode::Bool(true)))
    );
    let count = table::row(&t, "c with f(c) = 0").map(|v| match v.node() {
        ExprNode::Set(items) => items.len(),
        _ => 0,
    });
    assert_eq!(count, Some(1));
}

#[test]
fn summary_table_has_every_row() {
    let t = an("x^3 - 3*x").summary().unwrap();
    let labels: Vec<String> = table::rows(&t)
        .unwrap()
        .into_iter()
        .map(|(l, _)| l)
        .collect();
    for want in [
        "domain",
        "range",
        "x-intercepts",
        "critical points",
        "local maxima",
        "increasing on",
        "concave up on",
        "inflection points",
        "vertical asymptotes",
    ] {
        assert!(
            labels.iter().any(|l| l == want),
            "{want} missing: {labels:?}"
        );
    }
    assert_eq!(
        table::row(&t, "local maxima").map(|v| s(&v)),
        Some("{(-1, 2)}".into())
    );
    assert_eq!(
        table::row(&t, "range").map(|v| s(&v)),
        Some("(-∞, ∞)".into())
    );
}

#[test]
fn refuses_what_it_cannot_settle() {
    // Periodic on ℝ: asks for an interval.
    let err = an("sin(x)").critical_points().unwrap_err();
    assert!(err.to_string().contains("give an interval"), "{err}");
    // A parameter without a value.
    assert!(Analysis::new(&p("a*x^2"), "x", None).is_err());
    // On an interval the periodic function is fine.
    let c = on("sin(x)", "0", "2*pi").critical_points().unwrap();
    assert_eq!(
        c.iter().map(|c| s(&c.x)).collect::<Vec<_>>(),
        vec!["π/2", "3*π/2"]
    );
    assert_eq!(variable_of(&p("t^2"), None).unwrap(), "t");
    assert!(variable_of(&p("a*b"), None).is_err());
}
