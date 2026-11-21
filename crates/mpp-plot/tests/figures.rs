//! Figures built from real documents.

use std::collections::BTreeMap;

use mpp_eval::Session;
use mpp_plot::{figures, Figure, Geometry, LayerData, PlotTheme};

fn build(src: &str) -> Vec<Figure> {
    let mut session = Session::new();
    let ev = session.run_str(src);
    figures(ev)
        .into_iter()
        .map(|f| {
            f.figure
                .unwrap_or_else(|e| panic!("figure failed: {e}\n{src}"))
        })
        .collect()
}

fn kinds(f: &Figure) -> Vec<&'static str> {
    f.spec()
        .layers
        .iter()
        .map(|l| match &l.geometry {
            Geometry::Explicit { .. } => "explicit",
            Geometry::ExplicitX { .. } => "explicit-x",
            Geometry::Parametric { .. } => "parametric",
            Geometry::Polar { .. } => "polar",
            Geometry::Implicit { .. } => "implicit",
            Geometry::Region { .. } => "region",
            Geometry::Contour { .. } => "contour",
            Geometry::Points(_) => "points",
            Geometry::Scatter(_) => "scatter",
            Geometry::Histogram { .. } => "histogram",
            Geometry::Surface { .. } => "surface",
            Geometry::ParamSurface { .. } => "param-surface",
            Geometry::Curve3 { .. } => "curve3",
            Geometry::BoxPlot(_) => "box-plot",
            _ => "helper",
        })
        .collect()
}

#[test]
fn a_function_with_a_range_and_a_caption() {
    let f = &build("Figure: $\\sin x$, $x \\in [-2\\pi, 2\\pi]$. The sine wave.\n")[0];
    assert_eq!(kinds(f), ["explicit"]);
    assert_eq!(f.caption(), Some("The sine wave."));
    let w = f.window();
    assert!((w.x0 + std::f64::consts::TAU).abs() < 1e-9);
    assert!(w.y0 < -1.0 && w.y1 > 1.0 && w.y1 < 1.5, "{w:?}");
}

#[test]
fn user_functions_and_constants_are_resolved() {
    let src = "$$ R \\equiv 3 $$\n\n$$ f(x) := R x^2 $$\n\nFigure: $f(x)$, $x \\in [0, 2]$.\n";
    let f = &build(src)[0];
    let LayerData::Curve(s) = &f.layers()[0] else {
        panic!("not a curve")
    };
    let last = s.segments[0].last().copied().unwrap();
    assert!(
        (last[0] - 2.0).abs() < 1e-9 && (last[1] - 12.0).abs() < 1e-6,
        "{last:?}"
    );
}

#[test]
fn several_curves_get_a_legend() {
    let f = &build("Figure: $\\sin x$, $\\cos x$, $x \\in [0, 7]$.\n")[0];
    assert_eq!(kinds(f), ["explicit", "explicit"]);
    assert_eq!(f.legend().len(), 2);
}

#[test]
fn sliders_drive_the_figure() {
    let src = "$$ a := 2, \\; a \\in [0, 5] $$\n\nFigure: $a x$, $x \\in [0, 1]$.\n";
    let mut f = build(src).remove(0);
    let sliders = f.sliders();
    assert_eq!(sliders.len(), 1);
    assert_eq!(
        (sliders[0].min, sliders[0].max, sliders[0].value),
        (0.0, 5.0, 2.0)
    );
    let top = |f: &Figure| match &f.layers()[0] {
        LayerData::Curve(s) => s.segments[0].last().copied().unwrap()[1],
        _ => f64::NAN,
    };
    assert!((top(&f) - 2.0).abs() < 1e-9);
    let mut v = BTreeMap::new();
    v.insert("a".to_string(), 4.0);
    f.rebuild(&v);
    assert!((top(&f) - 4.0).abs() < 1e-9);
    assert_eq!(f.sliders()[0].value, 4.0);
}

#[test]
fn equations_become_the_right_kind_of_curve() {
    let f = &build("Figure: $x^2 + y^2 = 4$.\n")[0];
    assert_eq!(kinds(f), ["implicit"]);
    let LayerData::Lines(lines) = &f.layers()[0] else {
        panic!("not lines")
    };
    assert!(!lines.is_empty());
    for p in lines.iter().flatten() {
        assert!((p[0] * p[0] + p[1] * p[1] - 4.0).abs() < 1e-3);
    }
    let w = f.window();
    assert!(w.x0 < -2.0 && w.x1 > 2.0, "{w:?}");

    assert_eq!(kinds(&build("Figure: $y = x^2$.\n")[0]), ["explicit"]);
    assert_eq!(kinds(&build("Figure: $x = y^2$.\n")[0]), ["explicit-x"]);
    assert_eq!(kinds(&build("Figure: $x = 2$.\n")[0]), ["explicit-x"]);
    assert_eq!(
        kinds(&build("Figure: $(\\cos t, \\sin t)$, $t \\in [0, 2\\pi]$.\n")[0]),
        ["parametric"]
    );
    assert_eq!(
        kinds(&build("Figure: $1 + \\cos \\theta$, polar.\n")[0]),
        ["polar"]
    );
    assert_eq!(
        kinds(&build("Figure: $r = 2 \\sin \\theta$.\n")[0]),
        ["polar"]
    );
}

#[test]
fn inequalities_shade_a_region() {
    let f = &build("Figure: $y > x^2$, $x, y \\in [-2, 2]$.\n")[0];
    assert_eq!(kinds(f), ["region"]);
    let LayerData::Region { grid, edges } = &f.layers()[0] else {
        panic!("not a region")
    };
    assert!(grid.inside.iter().any(|b| *b) && grid.inside.iter().any(|b| !*b));
    assert_eq!(edges.len(), 1);
    assert!(edges[0].1, "y > x^2 is strict, so its edge is dashed");
    // A cell above the parabola's vertex is inside.
    let (nx, ny) = (grid.nx, grid.ny);
    assert!(grid.inside[(ny - 2) * nx + nx / 2]);
    assert!(!grid.inside[nx / 2]);
}

#[test]
fn chained_inequalities() {
    let f = &build("Figure: $1 < x^2 + y^2 \\le 4$, $x, y \\in [-3, 3]$.\n")[0];
    assert_eq!(kinds(f), ["region"]);
    let LayerData::Region { edges, .. } = &f.layers()[0] else {
        panic!("not a region")
    };
    assert_eq!(edges.len(), 2);
}

#[test]
fn points_and_curves_together() {
    let f = &build("Figure: $x^2$, points $(1, 1)$, points $(2, 4)$, $x \\in [0, 3]$.\n")[0];
    assert_eq!(kinds(f), ["explicit", "points"]);
    let LayerData::Points(p) = &f.layers()[1] else {
        panic!("not points")
    };
    assert_eq!(p.len(), 2);
}

#[test]
fn surfaces_and_space_curves() {
    let f = &build("Figure: $z = x^2 - y^2$, $x, y \\in [-2, 2]$.\n")[0];
    assert_eq!(kinds(f), ["surface"]);
    assert!(f.is_3d());
    let LayerData::Surface(m) = &f.layers()[0] else {
        panic!("not a surface")
    };
    assert_eq!((m.nu, m.nv), (61, 61));
    assert_eq!(m.at(0, 0), [-2.0, -2.0, 0.0]);
    assert_eq!(
        kinds(&build("Figure: $\\sin x \\cos y$, $x, y \\in [-3, 3]$.\n")[0]),
        ["surface"]
    );
    assert_eq!(
        kinds(&build("Figure: $(\\cos t, \\sin t, t)$, $t \\in [0, 6]$.\n")[0]),
        ["curve3"]
    );
    assert_eq!(
        kinds(&build("Figure: $(\\cos u \\sin v, \\sin u \\sin v, \\cos v)$, $u \\in [0, 6.3]$, $v \\in [0, 3.15]$.\n")[0]),
        ["param-surface"]
    );
}

#[test]
fn bad_items_are_reported_not_fatal() {
    let mut session = Session::new();
    let ev = session.run_str("Figure: $x \\ne 2$.\n");
    let all = figures(ev);
    assert_eq!(all.len(), 1);
    assert!(all[0].figure.is_err());

    let f = &build("Figure: $\\sin x$, $x \\ne 2$.\n")[0];
    assert!(f.warnings().count() >= 1);
}

#[test]
fn both_themes_draw_every_kind() {
    let docs = [
        "Figure: $\\sin x$, $\\tan x$, $x \\in [-6, 6]$.\n",
        "Figure: $x^2 + y^2 = 1$.\n",
        "Figure: $y < x$, $y > -x$, $x, y \\in [-3, 3]$.\n",
        "Figure: $z = x y$, $x, y \\in [-1, 1]$.\n",
    ];
    for doc in docs {
        let f = &build(doc)[0];
        for theme in [PlotTheme::light(), PlotTheme::dark()] {
            let scene = f.scene(480.0, 320.0, &theme);
            assert!(scene.items.len() > 5, "{doc}");
            let svg = mpp_plot::to_svg(&scene);
            assert!(svg.starts_with("<svg") && svg.trim_end().ends_with("</svg>"));
        }
    }
}

#[test]
fn a_user_function_of_t_or_u_or_y_is_a_curve_not_a_surface() {
    for (def, fig) in [
        ("u(t) := t^2", "u(t)"),
        ("y(t) := t^2", "y(t)"),
        ("x(t) := t^2", "x(t)"),
        ("v(s) := s^2", "v(s)"),
    ] {
        let var = if fig.contains("(s)") { "s" } else { "t" };
        let src = format!("$$ {def} $$\n\nFigure: ${fig}$, ${var} \\in [0, 1]$.\n");
        let f = &build(&src)[0];
        assert_eq!(kinds(f), ["explicit"], "{src}");
        let LayerData::Curve(s) = &f.layers()[0] else {
            panic!("not a curve: {src}")
        };
        let last = s.segments[0].last().copied().unwrap();
        assert!(
            (last[0] - 1.0).abs() < 1e-9 && (last[1] - 1.0).abs() < 1e-6,
            "{src}: {last:?}"
        );
    }
}
