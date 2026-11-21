//! SVG snapshots (insta) and PNG renders to look at.

use std::path::PathBuf;

use mpp_eval::Session;
use mpp_plot::{figures, to_png, to_svg, Camera, Figure, PlotTheme};

fn build(src: &str) -> Figure {
    let mut session = Session::new();
    let ev = session.run_str(src);
    figures(ev)
        .into_iter()
        .next()
        .map(|f| f.figure.unwrap_or_else(|e| panic!("figure failed: {e}")))
        .unwrap_or_else(|| panic!("no figure in {src}"))
}

fn out_dir() -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("target")
        .join("plot-snapshots");
    let _ = std::fs::create_dir_all(&dir);
    dir
}

const FIGURES: &[(&str, &str)] = &[
    (
        "sine-cosine",
        "Figure: $\\sin x$, $\\cos x$, $x \\in [-2\\pi, 2\\pi]$.\n",
    ),
    (
        "parabola-line",
        "Figure: $x^2 - 2$, $2 x + 1$, $x \\in [-4, 4]$.\n",
    ),
    ("tangent", "Figure: $\\tan x$, $x \\in [-2\\pi, 2\\pi]$.\n"),
    ("reciprocal", "Figure: $1 / x$, $x \\in [-4, 4]$.\n"),
    ("circle", "Figure: $x^2 + y^2 = 4$.\n"),
    (
        "ellipse-hyperbola",
        "Figure: $x^2 / 9 + y^2 / 4 = 1$, $x^2 - y^2 = 1$, $x, y \\in [-4, 4]$.\n",
    ),
    (
        "inequality",
        "Figure: $y > x^2 - 1$, $y < 3 - x$, $x, y \\in [-4, 4]$.\n",
    ),
    ("cardioid", "Figure: $1 + \\cos \\theta$, polar.\n"),
    (
        "lissajous",
        "Figure: $(\\sin 3 t, \\sin 2 t)$, $t \\in [0, 2\\pi]$.\n",
    ),
    (
        "step",
        "Figure: $\\begin{cases} x & x < 1 \\\\ x + 2 & x \\ge 1 \\end{cases}$, $x \\in [-3, 4]$.\n",
    ),
    (
        "saddle",
        "Figure: $z = x^2 - y^2$, $x, y \\in [-2, 2]$.\n",
    ),
    (
        "ripple",
        "Figure: $\\sin(\\sqrt{x^2 + y^2})$, $x, y \\in [-8, 8]$.\n",
    ),
    (
        "helix",
        "Figure: $(\\cos t, \\sin t, t / 4)$, $t \\in [0, 12]$.\n",
    ),
    (
        "sphere",
        "Figure: $(\\cos u \\sin v, \\sin u \\sin v, \\cos v)$, $u \\in [0, 6.2832]$, $v \\in [0, 3.1416]$.\n",
    ),
    (
        "contours",
        "Figure: $x^2 + y^2 / 2$, contour, $x, y \\in [-3, 3]$.\n",
    ),
];

#[test]
fn render_everything_to_png() {
    let dir = out_dir();
    for (name, src) in FIGURES {
        let fig = build(src);
        for theme in [PlotTheme::light(), PlotTheme::dark()] {
            let scene = if fig.is_3d() {
                fig.scene_3d(&Camera::default(), 640.0, 440.0, &theme)
            } else {
                fig.scene_2d(640.0, 420.0, &theme)
            };
            let png = to_png(&scene, 1.5).unwrap_or_else(|e| panic!("{name}: {e}"));
            assert!(png.starts_with(&[0x89, b'P', b'N', b'G']));
            std::fs::write(dir.join(format!("{name}-{}.png", theme.name)), png).unwrap();
        }
    }
}

#[test]
fn svg_snapshot_sine_light() {
    let fig = build(FIGURES[0].1);
    insta::assert_snapshot!(to_svg(&fig.scene_2d(480.0, 320.0, &PlotTheme::light())));
}

#[test]
fn svg_snapshot_sine_dark() {
    let fig = build(FIGURES[0].1);
    insta::assert_snapshot!(to_svg(&fig.scene_2d(480.0, 320.0, &PlotTheme::dark())));
}

#[test]
fn svg_snapshot_circle_light() {
    let fig = build(FIGURES[4].1);
    insta::assert_snapshot!(to_svg(&fig.scene_2d(360.0, 360.0, &PlotTheme::light())));
}

#[test]
fn svg_snapshot_inequality_dark() {
    let fig = build(FIGURES[6].1);
    insta::assert_snapshot!(to_svg(&fig.scene_2d(420.0, 320.0, &PlotTheme::dark())));
}

#[test]
fn svg_snapshot_step_light() {
    let fig = build(FIGURES[9].1);
    insta::assert_snapshot!(to_svg(&fig.scene_2d(420.0, 300.0, &PlotTheme::light())));
}

#[test]
fn svg_snapshot_saddle_light() {
    let fig = build(FIGURES[10].1);
    let mut small = fig.clone();
    small.set_quality(mpp_plot::Quality {
        surface_cells: 8,
        ..mpp_plot::Quality::default()
    });
    insta::assert_snapshot!(to_svg(&small.scene_3d(
        &Camera::default(),
        360.0,
        300.0,
        &PlotTheme::light()
    )));
}

#[test]
fn svg_snapshot_saddle_dark() {
    let fig = build(FIGURES[10].1);
    let mut small = fig.clone();
    small.set_quality(mpp_plot::Quality {
        surface_cells: 8,
        ..mpp_plot::Quality::default()
    });
    insta::assert_snapshot!(to_svg(&small.scene_3d(
        &Camera::default(),
        360.0,
        300.0,
        &PlotTheme::dark()
    )));
}
