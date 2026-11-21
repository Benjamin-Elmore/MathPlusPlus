//! Odd arguments to the helper keywords never panic: they draw something or say why not.

use mpp_eval::Session;
use mpp_plot::{figures, Camera, PlotTheme};

const NASTY: &[&str] = &[
    "Figure: $x^2$, tangent at $1 / 0$, $x \\in [-1, 1]$.",
    "Figure: $|x|$, tangent at $x = 0$.",
    "Figure: $1 / x$, tangent at $x = 0$, $x \\in [-1, 1]$.",
    "Figure: $\\ln x$, tangent at $x = -2$.",
    "Figure: $x^2$, riemann left $n = 0$, $x \\in [0, 1]$.",
    "Figure: $x^2$, riemann mid $n = -3$, $x \\in [0, 1]$.",
    "Figure: $x^2$, riemann right $n = 1000000$, $x \\in [0, 1]$.",
    "Figure: $1 / x$, riemann left $n = 5$, $[-1, 1]$.",
    "Figure: $x^2$, riemann left $n = 4$, $[2, 1]$.",
    "Figure: $x$, $x$, area between.",
    "Figure: $x$, $x + 1$, area between.",
    "Figure: $x^2$, area between $[3, 3]$.",
    "Figure: $\\tan x$, $x$, area between, $x \\in [-4, 4]$.",
    "Figure: $x^2$, revolve about $y = 5$, $x \\in [0, 1]$.",
    "Figure: $x^2$, revolve about $x = 5$, $x \\in [0, 1]$.",
    "Figure: $x^2$, revolve about $z = 1$.",
    "Figure: $x^2$, revolve about $y = 0$, $x \\in [1, 1]$.",
    "Figure: $x^2$, revolve about $y = 0$, $x \\in [1, 0]$.",
    "Figure: $\\sqrt{x}$, revolve about $y = 0$, $x \\in [-4, 4]$.",
    "Figure: slope field $y' = 1 / x$, $x \\in [-1, 1]$, $y \\in [-1, 1]$.",
    "Figure: slope field $y' = y^2$, (0, 1), $x \\in [-1, 1]$, $y \\in [-1, 1]$.",
    "Figure: slope field $y' = \\sqrt{y}$, $x \\in [-1, 1]$, $y \\in [-1, 1]$.",
    "Figure: slope field $y'$.",
    "Figure: slope field $x' = y, y' = x, z' = 1$.",
    "Figure: slope field $(0, 0)$.",
    "Figure: slope field $y' = x - y, (a, b)$.",
    "Figure: slope field $y' = x - y$, $x \\in [1, 1]$.",
    "Figure: vector field $(x)$.",
    "Figure: vector field $(x, y, z)$.",
    "Figure: vector field $(1 / x, \\ln y)$, $x, y \\in [-1, 1]$.",
    "Figure: gradient field $\\sqrt{x}$.",
    "Figure: gradient field $|x| + |y|$, $x, y \\in [-1, 1]$.",
    "Figure: $z = x y$, tangent plane at $(100, 100)$, $x, y \\in [-1, 1]$.",
    "Figure: $z = 1 / x$, tangent plane at $(0, 0)$, $x, y \\in [-1, 1]$.",
    "Figure: $z = x y$, tangent plane at $5$.",
    "Figure: $z = x y$, tangent plane at $(1, 1, 1)$, $x, y \\in [-1, 1]$.",
    "Figure: $\\sin x$, taylor $n = 100$, $x \\in [-1, 1]$.",
    "Figure: $\\sin x$, taylor $n = -1$.",
    "Figure: $|x|$, taylor $n = 3$, $a = 0$.",
    "Figure: $\\ln x$, taylor $n = 3$, $a = 0$.",
    "Figure: $x^2$, taylor.",
    "Figure: $x^2 - 2$, newton from $x_0 = 0$.",
    "Figure: $x^2 + 1$, newton from $x_0 = 1$, $n = 30$.",
    "Figure: $\\ln x$, newton from $x_0 = -1$.",
    "Figure: $x^{1/3}$, newton from $x_0 = 1$, $n = 30$.",
    "Figure: scatter $nothing, nobody$.",
    "Figure: scatter $x$.",
    "Figure: histogram.",
    "Figure: histogram $q, 0$.",
    "Figure: box plot.",
    "Figure: box plot $q, r$.",
    "Figure: riemann left, tangent at, taylor, newton from, area between.",
    "Figure: tangent at $$, riemann left $,$, taylor $()$.",
];

#[test]
fn nothing_panics_and_everything_draws_or_explains() {
    for src in NASTY {
        let mut session = Session::new();
        let ev = session.run_str(&format!("{src}\n"));
        for f in figures(ev) {
            match f.figure {
                Ok(fig) => {
                    for theme in [PlotTheme::light(), PlotTheme::dark()] {
                        let scene = if fig.is_3d() {
                            fig.scene_3d(&Camera::default(), 320.0, 240.0, &theme)
                        } else {
                            fig.scene_2d(320.0, 240.0, &theme)
                        };
                        assert!(scene.width > 0.0, "{src}");
                        let _ = mpp_plot::to_svg(&scene);
                    }
                    let _ = fig.warnings().count();
                }
                Err(e) => assert!(!e.to_string().is_empty(), "{src}"),
            }
        }
    }
}

#[test]
fn dragging_every_slider_to_its_ends_is_safe() {
    let srcs = [
        "Figure: $x^2 - 2$, newton from $x_0 = 1$, $x \\in [-3, 3]$.",
        "Figure: $\\sin x$, taylor $n = 3$, $x \\in [-6, 6]$.",
        "Figure: $x^3$, tangent at $x = 0$, riemann left $n = 5$, $x \\in [-2, 2]$.",
        "Figure: $z = x^2 - y^2$, tangent plane at $(0, 0)$, $x, y \\in [-2, 2]$.",
    ];
    for src in srcs {
        let mut session = Session::new();
        let ev = session.run_str(&format!("{src}\n"));
        let Some(Ok(mut fig)) = figures(ev).into_iter().next().map(|f| f.figure) else {
            panic!("{src}");
        };
        for s in fig.sliders() {
            for v in [
                s.min,
                s.max,
                (s.min + s.max) / 2.0,
                f64::NAN,
                f64::INFINITY,
                -1e300,
            ] {
                let mut values = std::collections::BTreeMap::new();
                values.insert(s.name.clone(), v);
                fig.rebuild(&values);
                let scene = if fig.is_3d() {
                    fig.scene_3d(&Camera::default(), 320.0, 240.0, &PlotTheme::light())
                } else {
                    fig.scene_2d(320.0, 240.0, &PlotTheme::light())
                };
                assert!(!scene.items.is_empty(), "{src}");
            }
        }
    }
}
