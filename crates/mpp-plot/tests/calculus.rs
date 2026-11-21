//! The visual calculus helpers draw correct geometry: strips whose sum equals the numeric
//! rule, tangents that touch the curve, regions with the right area, solution curves that
//! follow their field, and data figures that match their data.

use std::collections::BTreeMap;
use std::f64::consts::{E, PI};

use mpp_core::applications::{self, Axis};
use mpp_core::numerics::{self, QuadratureRule};
use mpp_core::{pow, Expr};
use mpp_eval::Session;
use mpp_plot::helpers::FieldKind;
use mpp_plot::{figures, Figure, LayerData, MarkerKind, PlotTheme};

fn build(src: &str) -> Figure {
    let mut session = Session::new();
    let ev = session.run_str(src);
    assert!(ev.errors().is_empty(), "{:?}", ev.errors());
    figures(ev)
        .into_iter()
        .next()
        .map(|f| f.figure.unwrap_or_else(|e| panic!("figure failed: {e}")))
        .unwrap_or_else(|| panic!("no figure in {src}"))
}

fn x_squared() -> Expr {
    pow(Expr::sym("x"), Expr::int(2))
}

fn number(e: &Expr) -> f64 {
    e.eval_f64(&mpp_core::EmptyEnv).expect("a number")
}

/// Every layer, with the parts of composites flattened.
fn flat(fig: &Figure) -> Vec<&LayerData> {
    fn walk<'a>(d: &'a LayerData, out: &mut Vec<&'a LayerData>) {
        match d {
            LayerData::Composite(parts) => parts.iter().for_each(|p| walk(p, out)),
            other => out.push(other),
        }
    }
    let mut out = Vec::new();
    fig.layers().iter().for_each(|d| walk(d, &mut out));
    out
}

fn bars_sum(fig: &Figure) -> (usize, f64) {
    flat(fig)
        .into_iter()
        .find_map(|d| match d {
            LayerData::Bars(b) => Some((
                b.len(),
                b.iter().map(|b| (b.y0 + b.y1) / 2.0 * (b.x1 - b.x0)).sum(),
            )),
            _ => None,
        })
        .expect("strips")
}

#[test]
fn riemann_strips_sum_to_the_numeric_rule() {
    let f = x_squared();
    for (word, rule) in [
        ("left", QuadratureRule::Left),
        ("right", QuadratureRule::Right),
        ("mid", QuadratureRule::Midpoint),
        ("trapezoid", QuadratureRule::Trapezoid),
    ] {
        let fig = build(&format!(
            "Figure: $x^2$, riemann {word} $n = 8$, $x \\in [0, 2]$.\n"
        ));
        let (count, sum) = bars_sum(&fig);
        assert_eq!(count, 8, "{word}");
        let q = numerics::quadrature_rule(&f, "x", &Expr::int(0), &Expr::int(2), 8, rule, None)
            .unwrap();
        assert!((sum - number(&q.value)).abs() < 1e-9, "{word}: {sum}");
        assert!(
            fig.readouts()
                .iter()
                .any(|r| r.to_lowercase().contains(word) || r.starts_with("Midpoint rule, n = 8")),
            "{word}: {:?}",
            fig.readouts()
        );
    }
}

#[test]
fn the_strip_slider_rebuilds_the_strips() {
    let mut fig = build("Figure: $x^2$, riemann left $n = 8$, $x \\in [0, 2]$.\n");
    let sliders = fig.sliders();
    let n = sliders
        .iter()
        .find(|s| s.name == "n")
        .expect("a slider for n");
    assert!(n.integer && n.value == 8.0 && n.min >= 1.0 && n.max >= 24.0);
    assert_eq!(n.step(), 1.0);
    let mut values = BTreeMap::new();
    values.insert("n".to_string(), 16.4);
    fig.rebuild(&values);
    let (count, sum) = bars_sum(&fig);
    assert_eq!(count, 16);
    let q = numerics::quadrature_rule(
        &x_squared(),
        "x",
        &Expr::int(0),
        &Expr::int(2),
        16,
        QuadratureRule::Left,
        None,
    )
    .unwrap();
    assert!((sum - number(&q.value)).abs() < 1e-9);
}

#[test]
fn a_slider_the_document_declared_is_used() {
    let fig = build(
        "$$ n := 4, \\; n \\in [1, 30] $$\n\nFigure: $x^2$, riemann right $n$, $x \\in [0, 2]$.\n",
    );
    assert_eq!(bars_sum(&fig).0, 4);
    assert_eq!(fig.sliders().iter().filter(|s| s.name == "n").count(), 1);
}

#[test]
fn the_tangent_touches_the_curve_and_moves_with_its_slider() {
    let mut fig = build("Figure: $x^3 - 2x$, tangent at $x = 1$, $x \\in [-2, 2]$.\n");
    let tangent = |fig: &Figure| {
        flat(fig)
            .into_iter()
            .find_map(|d| match d {
                LayerData::Tangent { point, slope } => Some((*point, *slope)),
                _ => None,
            })
            .expect("a tangent")
    };
    let (p, m) = tangent(&fig);
    assert_eq!(p, [1.0, -1.0]);
    assert!((m - 1.0).abs() < 1e-9);
    let mut values = BTreeMap::new();
    values.insert("x_0".to_string(), -1.0);
    fig.rebuild(&values);
    let (p, m) = tangent(&fig);
    assert!((p[0] + 1.0).abs() < 1e-12 && (p[1] - 1.0).abs() < 1e-9);
    assert!((m - 1.0).abs() < 1e-9, "3x^2 - 2 at -1");
    values.insert("x_0".to_string(), 2.0);
    fig.rebuild(&values);
    assert!((tangent(&fig).1 - 10.0).abs() < 1e-9);
    assert!(
        fig.readouts()[0].contains("y = 10x"),
        "{:?}",
        fig.readouts()
    );
}

fn shoelace(poly: &[[f64; 2]]) -> f64 {
    let n = poly.len();
    (0..n)
        .map(|i| {
            let (a, b) = (poly[i], poly[(i + 1) % n]);
            a[0] * b[1] - b[0] * a[1]
        })
        .sum::<f64>()
        .abs()
        / 2.0
}

fn shaded_area(fig: &Figure) -> f64 {
    flat(fig)
        .into_iter()
        .filter_map(|d| match d {
            LayerData::Shaded(polys) => Some(polys.iter().map(|p| shoelace(p)).sum::<f64>()),
            _ => None,
        })
        .sum()
}

#[test]
fn the_area_between_curves_matches_the_c3_value_and_finds_the_crossings() {
    let fig = build("Figure: $x$, $x^2$, area between.\n");
    let exact =
        applications::area_between(&Expr::sym("x"), Some(&x_squared()), "x", None, None).unwrap();
    assert!((number(&exact) - 1.0 / 6.0).abs() < 1e-12);
    assert!((shaded_area(&fig) - number(&exact)).abs() < 1e-4);
    let crossings: Vec<[f64; 2]> = flat(&fig)
        .into_iter()
        .filter_map(|d| match d {
            LayerData::Points(p) => Some(p.clone()),
            _ => None,
        })
        .flatten()
        .collect();
    assert_eq!(crossings.len(), 2);
    assert!(crossings[0][0].abs() < 1e-9 && (crossings[1][0] - 1.0).abs() < 1e-9);
    assert!(
        fig.readouts()[0].ends_with("0.1667"),
        "{:?}",
        fig.readouts()
    );
}

#[test]
fn shading_changes_sides_at_every_crossing() {
    let fig =
        build("Figure: $\\sin x$, $\\cos x$, area between $[0, 2\\pi]$, $x \\in [0, 2\\pi]$.\n");
    let pieces: usize = flat(&fig)
        .into_iter()
        .map(|d| match d {
            LayerData::Shaded(p) => p.len(),
            _ => 0,
        })
        .sum();
    // The curves cross at pi/4 and 5 pi/4: three pieces.
    assert_eq!(pieces, 3);
    // The total is 4 sqrt 2.
    assert!(
        (shaded_area(&fig) - 4.0 * 2f64.sqrt()).abs() < 5e-3,
        "{}",
        shaded_area(&fig)
    );
}

#[test]
fn solids_of_revolution_report_the_c3_volume() {
    // A washer: the region between y = x and y = x^2 on [0, 1] about the x axis.
    let fig = build("Figure: $x$, $x^2$, revolve about $y = 0$, $x \\in [0, 1]$.\n");
    assert!(fig.is_3d());
    let exact = applications::volume(
        &Expr::sym("x"),
        Some(&x_squared()),
        "x",
        &Expr::int(0),
        &Expr::int(1),
        &Axis::x_axis(),
        None,
        None,
    )
    .unwrap();
    // 2 pi / 15.
    assert!((number(&exact) - 2.0 * PI / 15.0).abs() < 1e-9);
    let line = &fig.readouts()[0];
    assert!(line.contains("washer") && line.contains("0.4189"), "{line}");

    // Disks and shells.
    let disk = build("Figure: $\\sqrt{x}$, revolve about $y = 0$, $x \\in [0, 4]$.\n");
    assert!(disk.readouts()[0].contains("disk") && disk.readouts()[0].contains("25.13"));
    let shell = build("Figure: $x^2$, revolve about $x = 0$, $x \\in [0, 2]$.\n");
    assert!(shell.readouts()[0].contains("shell") && shell.readouts()[0].contains("25.13"));
    // Same volume (8 pi) by shells as the disk figure above, within the sampling.
    let exact_shell = applications::volume(
        &x_squared(),
        None,
        "x",
        &Expr::int(0),
        &Expr::int(2),
        &Axis::Vertical(Expr::int(0)),
        None,
        None,
    )
    .unwrap();
    assert!((number(&exact_shell) - 8.0 * PI).abs() < 1e-9);
}

#[test]
fn a_washer_is_a_ring_between_the_two_radii() {
    let fig = build("Figure: $x$, $x^2$, revolve about $y = 0$, $x \\in [0, 1]$.\n");
    let solid = fig
        .layers()
        .iter()
        .find_map(|d| match d {
            LayerData::Solid(s) => Some(s),
            _ => None,
        })
        .expect("a solid");
    let slice = solid.parts.iter().find(|p| p.highlight).expect("a slice");
    // At x = 1/2 the radii are x^2 = 1/4 and x = 1/2.
    let radii: Vec<f64> = slice.mesh.points.iter().map(|p| p[1].hypot(p[2])).collect();
    let lo = radii.iter().copied().fold(f64::INFINITY, f64::min);
    let hi = radii.iter().copied().fold(0.0, f64::max);
    assert!(
        (lo - 0.25).abs() < 1e-9 && (hi - 0.5).abs() < 1e-9,
        "{lo} {hi}"
    );
    // The solid can be turned: the 3D scene is drawn from any camera.
    let mut cam = mpp_plot::Camera::default();
    for _ in 0..4 {
        cam.orbit(60.0, 10.0);
        let scene = fig.scene_3d(&cam, 480.0, 360.0, &PlotTheme::light());
        assert!(scene.counts().2 > 500);
    }
}

#[test]
fn a_solution_curve_follows_its_slope_field() {
    // y' = y through (0, 1) is e^x.
    let fig = build("Figure: slope field $y' = y, (0, 1)$, $x \\in [-2, 2]$, $y \\in [-1, 9]$.\n");
    let LayerData::Field {
        items,
        curves,
        kind,
        ..
    } = &fig.layers()[0]
    else {
        panic!("a field");
    };
    assert_eq!(*kind, FieldKind::Slope);
    assert!(items.len() > 200);
    // Every stroke has slope y.
    assert!(items
        .iter()
        .all(|i| (i.dir[1] - i.pos[1]).abs() < 1e-9 && i.dir[0] == 1.0));
    assert_eq!(curves.len(), 1);
    let at_one = curves[0]
        .iter()
        .min_by(|a, b| (a[0] - 1.0).abs().total_cmp(&(b[0] - 1.0).abs()))
        .unwrap();
    assert!((at_one[1] - E).abs() < 0.03, "{at_one:?}");
}

#[test]
fn the_template_slope_field_draws() {
    // The text the Insert menu writes for a slope field.
    let fig = build("Figure: slope field $y' = x - y$, $x \\in [-10, 10]$, $y \\in [-10, 10]$.\n");
    assert_eq!(fig.warnings().count(), 0);
    let scene = fig.scene_2d(500.0, 360.0, &PlotTheme::light());
    assert!(scene.counts().0 > 300);
}

#[test]
fn a_phase_portrait_circles_the_centre() {
    let fig = build("Figure: slope field $x' = y, y' = -x$, $x, y \\in [-3, 3]$.\n");
    let LayerData::Field {
        curves, directed, ..
    } = &fig.layers()[0]
    else {
        panic!("a field");
    };
    assert!(*directed && !curves.is_empty());
    for c in curves {
        let r: Vec<f64> = c.iter().map(|p| p[0].hypot(p[1])).collect();
        let (lo, hi) = r
            .iter()
            .fold((f64::INFINITY, 0.0f64), |(a, b), v| (a.min(*v), b.max(*v)));
        assert!(
            hi - lo < 0.02 * hi,
            "an orbit stays at one radius: {lo} {hi}"
        );
    }
}

#[test]
fn gradient_and_vector_fields_point_the_right_way() {
    let fig = build("Figure: gradient field $x^2 + y^2$, $x, y \\in [-2, 2]$.\n");
    let items: Vec<_> = fig
        .layers()
        .iter()
        .find_map(|d| match d {
            LayerData::Field { items, .. } => Some(items),
            _ => None,
        })
        .expect("arrows")
        .clone();
    assert!(items
        .iter()
        .all(|i| (i.dir[0] - 2.0 * i.pos[0]).abs() < 1e-5
            && (i.dir[1] - 2.0 * i.pos[1]).abs() < 1e-5));
    // Level curves go under the arrows.
    assert!(matches!(fig.layers()[0], LayerData::Contours(_)));

    let rot = build("Figure: vector field $(-y, x)$, $x, y \\in [-2, 2]$.\n");
    let LayerData::Field { items, kind, .. } = &rot.layers()[0] else {
        panic!("arrows");
    };
    assert_eq!(*kind, FieldKind::Arrow);
    assert!(items
        .iter()
        .all(|i| (i.dir[0] + i.pos[1]).abs() < 1e-9 && (i.dir[1] - i.pos[0]).abs() < 1e-9));
}

#[test]
fn level_curves_are_drawn_for_a_saddle() {
    let fig = build("Figure: $x^2 - y^2$, contour, $x, y \\in [-2, 2]$.\n");
    let LayerData::Contours(levels) = &fig.layers()[0] else {
        panic!("contours");
    };
    assert!(levels.len() >= 8 && levels.iter().all(|(_, l)| !l.is_empty()));
}

#[test]
fn the_tangent_plane_touches_the_surface_at_its_point() {
    let mut fig =
        build("Figure: $z = x^2 + y^2$, tangent plane at $(1, 1)$, $x, y \\in [-2, 2]$.\n");
    assert!(fig.is_3d());
    let plane = |fig: &Figure| {
        fig.layers()
            .iter()
            .find_map(|d| match d {
                LayerData::Solid(s) if !s.in_box => Some(s.clone()),
                _ => None,
            })
            .expect("a plane")
    };
    let s = plane(&fig);
    assert_eq!(s.dots, vec![[1.0, 1.0, 2.0]]);
    // z = 2 + 2 (x - 1) + 2 (y - 1) at every vertex.
    for p in s.parts[0].mesh.points.iter().filter(|p| p[2].is_finite()) {
        assert!((p[2] - (2.0 + 2.0 * (p[0] - 1.0) + 2.0 * (p[1] - 1.0))).abs() < 1e-9);
    }
    assert!(fig.readouts()[0].contains("z = 2 + 2(x \u{2212} 1) + 2(y \u{2212} 1)"));
    // Moving the point with its sliders moves the plane.
    let mut values = BTreeMap::new();
    values.insert("x_0".to_string(), 0.0);
    values.insert("y_0".to_string(), 0.0);
    fig.rebuild(&values);
    assert_eq!(plane(&fig).dots, vec![[0.0, 0.0, 0.0]]);
}

#[test]
fn taylor_polynomials_follow_the_degree_slider() {
    let mut fig = build("Figure: $\\sin x$, taylor $n = 5$, $x \\in [-3, 3]$.\n");
    let end = |fig: &Figure| -> f64 {
        flat(fig)
            .into_iter()
            .filter_map(|d| match d {
                LayerData::Curve(s) => Some(s),
                _ => None,
            })
            .nth(1)
            .and_then(|s| s.points().last().copied())
            .map(|p| p[1])
            .expect("the polynomial")
    };
    // T5(3) = 3 - 27/6 + 243/120.
    assert!((end(&fig) - (3.0 - 4.5 + 2.025)).abs() < 1e-9);
    let degree = fig.sliders().into_iter().find(|s| s.name == "n").unwrap();
    assert!(degree.integer && degree.value == 5.0);
    let mut values = BTreeMap::new();
    values.insert("n".to_string(), 1.0);
    fig.rebuild(&values);
    assert!((end(&fig) - 3.0).abs() < 1e-9, "T1(3) = 3");
    values.insert("n".to_string(), 9.0);
    fig.rebuild(&values);
    assert!((end(&fig) - 3f64.sin()).abs() < 5e-3);
}

#[test]
fn a_bare_number_after_taylor_is_the_degree() {
    // The Insert menu writes `taylor $5$`, shown as "Taylor polynomial of degree 5".
    let fig = build("Figure: $\\sin x$, taylor ${5}$, $x \\in [-3, 3]$.\n");
    assert!(
        fig.readouts().iter().any(|r| r.contains("degree 5")),
        "{:?}",
        fig.readouts()
    );
}

#[test]
fn newton_iterations_match_the_c3_table() {
    let fig = build("Figure: $x^2 - 2$, newton from $x_0 = 2$, $x \\in [0, 3]$.\n");
    let iterates: Vec<[f64; 2]> = flat(&fig)
        .into_iter()
        .filter_map(|d| match d {
            LayerData::Points(p) => Some(p.clone()),
            _ => None,
        })
        .flatten()
        .collect();
    let f = mpp_core::add(vec![x_squared(), Expr::int(-2)]);
    let (_, table) = numerics::newton(&f, "x", 2.0, 1e-12, None).unwrap();
    assert_eq!(iterates.len(), 5);
    for (p, row) in iterates.iter().zip(&table) {
        assert!((p[0] - row.x).abs() < 1e-12 && (p[1] - row.fx).abs() < 1e-12);
    }
    assert!((iterates[4][0] - 2f64.sqrt()).abs() < 1e-6);
}

#[test]
fn scatter_histogram_and_box_plot_match_the_data() {
    let table = "| $t$ | $x$ |
|---|---|
| 0 | 1 |
| 1 | 2 |
| 2 | 3 |
| 3 | 4 |
| 4 | 5 |
| 5 | 6 |
| 6 | 7 |
| 7 | 100 |

";
    let fig = build(&format!("{table}Figure: scatter $t, x$.\n"));
    let LayerData::Points(p) = &fig.layers()[0] else {
        panic!("points");
    };
    assert_eq!(p[1], [1.0, 2.0]);
    assert_eq!(p.len(), 8);
    assert_eq!(fig.axis_names(), ("t".to_string(), "x".to_string()));

    let hist = build(&format!("{table}Figure: histogram $x$, histogram $x$.\n"));
    let LayerData::Histogram { counts, edges } = &hist.layers()[0] else {
        panic!("bars");
    };
    assert_eq!(counts.iter().sum::<f64>(), 8.0);
    assert_eq!(edges.len(), counts.len() + 1);

    let boxes = build(&format!("{table}Figure: box plot $x$.\n"));
    let LayerData::BoxPlot(stats) = &boxes.layers()[0] else {
        panic!("a box");
    };
    assert_eq!(stats[0].median, 4.5);
    assert_eq!(stats[0].outliers, vec![100.0]);
}

#[test]
fn a_fitted_curve_is_drawn_over_the_data_range() {
    let src = "| $t$ | $x$ |\n|---|---|\n| 0 | 1 |\n| 1 | 3 |\n| 2 | 5 |\n| 3 | 7 |\n\n\
               $$ p(u) := \\operatorname{linreg}(t, x) $$\n\n\
               Figure: scatter $t, x$, $p(t)$.\n";
    let fig = build(src);
    assert_eq!(fig.layers().len(), 2);
    let LayerData::Curve(c) = &fig.layers()[0] else {
        panic!("the fit");
    };
    let xs: Vec<f64> = c.points().map(|p| p[0]).collect();
    let lo = xs.iter().copied().fold(f64::INFINITY, f64::min);
    let hi = xs.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    assert!(lo > -0.5 && lo < 0.1 && hi > 2.9 && hi < 3.5, "{lo} {hi}");
    assert!(c.points().all(|p| (p[1] - (1.0 + 2.0 * p[0])).abs() < 1e-9));
}

#[test]
fn jumps_get_open_and_closed_dots() {
    let fig = build(
        "Figure: $\\begin{cases} x & x < 1 \\\\ x + 2 & x \\ge 1 \\end{cases}$, $x \\in [-3, 4]$.\n",
    );
    let scene = fig.scene_2d(500.0, 360.0, &PlotTheme::light());
    let dots: Vec<_> = scene
        .items
        .iter()
        .filter_map(|i| match i {
            mpp_plot::Item::Marker(m) if m.kind == MarkerKind::Circle => Some(*m),
            _ => None,
        })
        .collect();
    assert_eq!(dots.len(), 2);
    assert_eq!(dots.iter().filter(|m| m.filled).count(), 1);
    assert_eq!(dots.iter().filter(|m| !m.filled).count(), 1);
    // The open dot is filled with the page colour, so it reads as a hole.
    let open = dots.iter().find(|m| !m.filled).unwrap();
    assert_eq!(open.hollow_fill, PlotTheme::light().background);
}

#[test]
fn every_helper_keyword_parses_and_draws_in_both_themes() {
    let helpers = [
        "Figure: $x^2$, tangent at $x = 1$, $x \\in [-2, 2]$.\n",
        "Figure: $x^2$, riemann trapezoid $n = 4$, $x \\in [0, 2]$.\n",
        "Figure: $x^2$, $x$, area between $[0, 1]$.\n",
        "Figure: $\\sin x$, taylor $n = 3$, $a = 1$, $x \\in [-3, 3]$.\n",
        "Figure: $x^2 - 2$, newton from $x_0 = 2$, $n = 3$.\n",
        "Figure: slope field $y' = y (1 - y), (0, 0.2)$, $x \\in [-3, 3]$, $y \\in [-1, 2]$.\n",
        "Figure: slope field $(y, -x)$, $x, y \\in [-2, 2]$.\n",
        "Figure: $y - x$, slope field, $x, y \\in [-2, 2]$.\n",
        "Figure: $(y, x)$, vector field, $x, y \\in [-2, 2]$.\n",
        "Figure: $x y$, gradient field, $x, y \\in [-2, 2]$.\n",
    ];
    for src in helpers {
        let fig = build(src);
        assert_eq!(
            fig.warnings().count(),
            0,
            "{src}: {:?}",
            fig.warnings().collect::<Vec<_>>()
        );
        for theme in [PlotTheme::light(), PlotTheme::dark()] {
            let scene = fig.scene_2d(480.0, 340.0, &theme);
            assert!(scene.items.len() > 20, "{src}");
            assert!(mpp_plot::to_svg(&scene).contains("</svg>"));
        }
    }
}

#[test]
fn helpers_without_a_curve_say_so_instead_of_panicking() {
    let mut session = Session::new();
    let ev = session.run_str("Figure: $x^2$, $y \\in [0, 1]$, tangent plane at $(1, 1)$.\n\nFigure: points $(1, 1)$, taylor $n = 2$.\n");
    for f in figures(ev) {
        match f.figure {
            Ok(fig) => assert!(fig.warnings().count() > 0),
            Err(e) => assert!(!e.to_string().is_empty()),
        }
    }
}

#[test]
fn axis_letters_are_maths_italic() {
    // x and y are the slanted letters of the maths alphabet, not upright text.
    let fig = build("Figure: $x^2$, $x \\in [-2, 2]$.\n");
    let scene = fig.scene_2d(480.0, 340.0, &PlotTheme::light());
    let names: Vec<&str> = scene
        .labels()
        .filter(|l| l.latex.is_some() && l.text.chars().count() == 1)
        .map(|l| l.text.as_str())
        .collect();
    assert!(
        names.contains(&"\u{1D465}") && names.contains(&"\u{1D466}"),
        "{names:?}"
    );
    let cube = build("Figure: $z = x^2 - y^2$, $x, y \\in [-2, 2]$.\n");
    let scene = cube.scene_3d(
        &mpp_plot::Camera::default(),
        480.0,
        340.0,
        &PlotTheme::dark(),
    );
    let names: Vec<&str> = scene
        .labels()
        .filter(|l| l.latex.is_some())
        .map(|l| l.text.as_str())
        .collect();
    for letter in ["\u{1D465}", "\u{1D466}", "\u{1D467}"] {
        assert!(names.contains(&letter), "{letter} in {names:?}");
    }
}
