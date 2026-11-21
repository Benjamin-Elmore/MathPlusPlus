//! Nothing a user types may panic the plotter.

use mpp_core::{Expr, ExprNode, Func};
use mpp_eval::Session;
use mpp_plot::{figures, to_svg, Camera, Figure, FigureSpec, Geometry, Layer, PlotTheme};
use proptest::prelude::*;

fn atom() -> impl Strategy<Value = String> {
    prop_oneof![
        Just("x".to_string()),
        Just("y".to_string()),
        Just("t".to_string()),
        Just("\\pi".to_string()),
        (-30i32..30).prop_map(|n| n.to_string()),
        (-300i32..300).prop_map(|n| format!("{}.{}", n / 10, n.abs() % 10)),
        Just("10^{300}".to_string()),
        Just("0".to_string()),
    ]
}

fn expr() -> impl Strategy<Value = String> {
    atom().prop_recursive(4, 24, 3, |inner| {
        prop_oneof![
            (inner.clone(), inner.clone()).prop_map(|(a, b)| format!("({a}) + ({b})")),
            (inner.clone(), inner.clone()).prop_map(|(a, b)| format!("({a}) - ({b})")),
            (inner.clone(), inner.clone()).prop_map(|(a, b)| format!("({a}) \\cdot ({b})")),
            (inner.clone(), inner.clone()).prop_map(|(a, b)| format!("\\frac{{{a}}}{{{b}}}")),
            (inner.clone(), inner.clone()).prop_map(|(a, b)| format!("({a})^{{{b}}}")),
            inner.clone().prop_map(|a| format!("\\sin({a})")),
            inner.clone().prop_map(|a| format!("\\ln({a})")),
            inner.clone().prop_map(|a| format!("\\sqrt{{{a}}}")),
            inner.clone().prop_map(|a| format!("\\tan({a})")),
            inner.clone().prop_map(|a| format!("|{a}|")),
            (inner.clone(), inner).prop_map(|(a, b)| format!("({a}, {b})")),
        ]
    })
}

fn relation() -> impl Strategy<Value = &'static str> {
    prop_oneof![Just("="), Just("<"), Just("\\le"), Just(">"), Just("\\ge")]
}

fn draw_everything(fig: &Figure) {
    for theme in [PlotTheme::light(), PlotTheme::dark()] {
        let scene = if fig.is_3d() {
            fig.scene_3d(&Camera::default(), 300.0, 220.0, &theme)
        } else {
            fig.scene_2d(300.0, 220.0, &theme)
        };
        let svg = to_svg(&scene);
        assert!(svg.ends_with("</svg>\n"));
    }
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 48, ..ProptestConfig::default() })]

    #[test]
    fn random_curves_never_panic(e in expr(), lo in -50i32..50, hi in -50i32..50) {
        let src = format!("Figure: ${e}$, $x \\in [{lo}, {hi}]$.\n");
        let mut session = Session::new();
        let ev = session.run_str(&src);
        for f in figures(ev) {
            if let Ok(fig) = f.figure {
                draw_everything(&fig);
            }
        }
    }

    #[test]
    fn random_equations_never_panic(a in expr(), b in expr(), rel in relation()) {
        let src = format!("Figure: ${a} {rel} {b}$, $x, y \\in [-4, 4]$.\n");
        let mut session = Session::new();
        let ev = session.run_str(&src);
        for f in figures(ev) {
            if let Ok(fig) = f.figure {
                draw_everything(&fig);
            }
        }
    }

    #[test]
    fn random_surfaces_never_panic(e in expr(), lo in -9i32..9, hi in -9i32..9) {
        let src = format!("Figure: $z = {e}$, $x, y \\in [{lo}, {hi}]$.\n");
        let mut session = Session::new();
        let ev = session.run_str(&src);
        for f in figures(ev) {
            if let Ok(fig) = f.figure {
                draw_everything(&fig);
            }
        }
    }

    #[test]
    fn random_ranges_in_specs_never_panic(
        lo in prop_oneof![Just(f64::NAN), Just(f64::INFINITY), Just(f64::NEG_INFINITY), -1e300f64..1e300, -10.0f64..10.0],
        hi in prop_oneof![Just(f64::NAN), Just(f64::INFINITY), Just(f64::NEG_INFINITY), -1e300f64..1e300, -10.0f64..10.0],
    ) {
        let x = Expr::sym("x");
        let y = Expr::new(ExprNode::Func(Func::Sin, vec![x.clone()]));
        let mut spec = FigureSpec::default();
        spec.layers.push(Layer::new(Geometry::Explicit { y, var: "x".into(), dep: "y".into() }));
        spec.layers.push(Layer::new(Geometry::Implicit {
            f: mpp_core::add(vec![mpp_core::pow(x.clone(), Expr::int(2)), mpp_core::pow(Expr::sym("y"), Expr::int(2)), Expr::int(-1)]),
            vars: ["x".into(), "y".into()],
        }));
        spec.layers.push(Layer::new(Geometry::Polar { r: x.clone(), var: "x".into() }));
        spec.set_range("x", lo, hi);
        spec.set_range("y", hi, lo);
        if let Ok(fig) = Figure::new(spec) {
            draw_everything(&fig);
        }
        let mut spec3 = FigureSpec::default();
        spec3.layers.push(Layer::new(Geometry::Surface {
            z: mpp_core::mul(vec![x.clone(), Expr::sym("y")]),
            vars: ["x".into(), "y".into()],
        }));
        spec3.set_range("x", lo, hi);
        spec3.set_range("y", lo, hi);
        if let Ok(fig) = Figure::new(spec3) {
            draw_everything(&fig);
        }
    }

    #[test]
    fn random_cameras_never_panic(dx in -1e6f32..1e6, dy in -1e6f32..1e6, z in -10.0f64..1e6, px in -1e6f32..1e6) {
        let mut fig_spec = FigureSpec::default();
        fig_spec.layers.push(Layer::new(Geometry::Surface {
            z: mpp_core::add(vec![Expr::sym("x"), Expr::sym("y")]),
            vars: ["x".into(), "y".into()],
        }));
        let Ok(fig) = Figure::new(fig_spec) else { return Ok(()); };
        let mut cam = Camera::default();
        cam.orbit(dx, dy);
        cam.pan_by(px, dy);
        cam.zoom_by(z);
        let scene = fig.scene_3d(&cam, 200.0, 200.0, &PlotTheme::dark());
        let _ = to_svg(&scene);
    }
}

#[test]
fn degenerate_documents_do_not_panic() {
    let docs = [
        "Figure:\n",
        "Figure: $$.\n",
        "Figure: $x \\in [1, 1]$.\n",
        "Figure: $x$, $x \\in [3, -3]$.\n",
        "Figure: $\\frac{1}{0}$.\n",
        "Figure: $y$.\n",
        "Figure: polar.\n",
        "Figure: $1 + \\cos \\theta$, polar, $\\theta \\in [0, 0]$.\n",
        "Figure: $x < y$.\n",
        "Figure: $x^2 + y^2 + z^2 = 1$.\n",
        "Figure: $z = 1 / (x^2 + y^2)$, $x, y \\in [-1, 1]$.\n",
        "Figure: $(\\cos t, \\sin t, \\tan t)$, $t \\in [0, 7]$.\n",
        "Figure: $\\sqrt{-1 - x^2}$.\n",
        "Figure: $x$, points $(1)$.\n",
        "Figure: $x$, points $(1, \\infty), (\\sqrt{-1}, 2)$.\n",
        "Figure: $\\sin x$, slope field.\n",
    ];
    for doc in docs {
        let mut session = Session::new();
        let ev = session.run_str(doc);
        for f in figures(ev) {
            if let Ok(fig) = f.figure {
                draw_everything(&fig);
            }
        }
    }
}
