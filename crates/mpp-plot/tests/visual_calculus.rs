//! `examples/visual-calculus.mpp`: every figure builds without warnings, the list of figures
//! matches `examples/visual-calculus.expected.txt` (set `MPP_BLESS=1` to rewrite it), and
//! the figures render to PNG in both themes under `target/plot-snapshots/visual-calculus`.
//! A few small ones are also kept as SVG snapshots.

use std::path::PathBuf;

use mpp_eval::Session;
use mpp_plot::{figures, to_png, to_svg, Camera, Figure, Geometry, PlotTheme};

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
}

fn example_figures() -> Vec<Figure> {
    let path = root().join("examples").join("visual-calculus.mpp");
    let src = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    let mut session = Session::new();
    let ev = session.run_str(&src);
    assert!(ev.errors().is_empty(), "{:?}", ev.errors());
    figures(ev)
        .into_iter()
        .enumerate()
        .map(|(i, f)| {
            f.figure
                .unwrap_or_else(|e| panic!("figure {} failed: {e}", i + 1))
        })
        .collect()
}

fn kind(g: &Geometry) -> &'static str {
    match g {
        Geometry::Explicit { .. } => "curve",
        Geometry::Contour { .. } => "contour",
        Geometry::Surface { .. } => "surface",
        Geometry::Scatter(_) => "scatter",
        Geometry::Histogram { .. } => "histogram",
        Geometry::BoxPlot(_) => "box plot",
        Geometry::Tangent { .. } => "tangent",
        Geometry::Riemann { .. } => "riemann",
        Geometry::AreaBetween { .. } => "area between",
        Geometry::Revolve { .. } => "revolve",
        Geometry::SlopeField { .. } => "slope field",
        Geometry::PhasePortrait { .. } => "phase portrait",
        Geometry::VectorField { .. } => "vector field",
        Geometry::GradientField { .. } => "gradient field",
        Geometry::TangentPlane { .. } => "tangent plane",
        Geometry::Taylor { .. } => "taylor",
        Geometry::Newton { .. } => "newton",
        _ => "other",
    }
}

/// One line per figure: its dimension, what it draws, its sliders and its read-outs.
fn describe(fig: &Figure) -> String {
    let layers: Vec<&str> = fig
        .spec()
        .layers
        .iter()
        .map(|l| kind(&l.geometry))
        .collect();
    let sliders: Vec<String> = fig
        .sliders()
        .iter()
        .map(|s| format!("{}={}", s.name, s.value))
        .collect();
    let mut line = format!(
        "{} {}",
        if fig.is_3d() { "3D" } else { "2D" },
        layers.join(" + ")
    );
    if !sliders.is_empty() {
        line.push_str(&format!(" | sliders {}", sliders.join(", ")));
    }
    if !fig.readouts().is_empty() {
        line.push_str(&format!(" | {}", fig.readouts().join("; ")));
    }
    line
}

#[test]
fn the_figure_list_matches_the_expected_file() {
    let figs = example_figures();
    for (i, f) in figs.iter().enumerate() {
        assert_eq!(
            f.warnings().count(),
            0,
            "figure {}: {:?}",
            i + 1,
            f.warnings().collect::<Vec<_>>()
        );
    }
    let got: String = figs
        .iter()
        .enumerate()
        .map(|(i, f)| format!("{}. {}\n", i + 1, describe(f)))
        .collect();
    let path = root().join("examples").join("visual-calculus.expected.txt");
    if std::env::var_os("MPP_BLESS").is_some() {
        std::fs::write(&path, &got).unwrap();
    }
    let want = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("{}: {e}", path.display()))
        .replace("\r\n", "\n");
    assert_eq!(got, want);
}

#[test]
fn every_figure_renders_in_light_and_dark() {
    let dir = root()
        .join("target")
        .join("plot-snapshots")
        .join("visual-calculus");
    let _ = std::fs::create_dir_all(&dir);
    for (i, fig) in example_figures().iter().enumerate() {
        for theme in [PlotTheme::light(), PlotTheme::dark()] {
            let scene = if fig.is_3d() {
                fig.scene_3d(&Camera::default(), 640.0, 440.0, &theme)
            } else {
                fig.scene_2d(640.0, 440.0, &theme)
            };
            let png = to_png(&scene, 1.5).unwrap_or_else(|e| panic!("figure {}: {e}", i + 1));
            assert!(png.starts_with(&[0x89, b'P', b'N', b'G']));
            std::fs::write(
                dir.join(format!("figure-{:02}-{}.png", i + 1, theme.name)),
                png,
            )
            .unwrap();
        }
    }
}

fn small(src: &str) -> Figure {
    let mut session = Session::new();
    let ev = session.run_str(src);
    figures(ev)
        .into_iter()
        .next()
        .and_then(|f| f.figure.ok())
        .expect("a figure")
}

#[test]
fn svg_snapshot_tangent_light() {
    let fig = small("Figure: $x^2$, tangent at $x = 1$, $x \\in [-2, 2]$.\n");
    insta::assert_snapshot!(to_svg(&fig.scene_2d(420.0, 320.0, &PlotTheme::light())));
}

#[test]
fn svg_snapshot_tangent_dark() {
    let fig = small("Figure: $x^2$, tangent at $x = 1$, $x \\in [-2, 2]$.\n");
    insta::assert_snapshot!(to_svg(&fig.scene_2d(420.0, 320.0, &PlotTheme::dark())));
}

#[test]
fn svg_snapshot_riemann_light() {
    let fig = small("Figure: $x^2$, riemann trapezoid $n = 4$, $x \\in [0, 2]$.\n");
    insta::assert_snapshot!(to_svg(&fig.scene_2d(420.0, 320.0, &PlotTheme::light())));
}

#[test]
fn svg_snapshot_riemann_dark() {
    let fig = small("Figure: $x^2$, riemann mid $n = 4$, $x \\in [0, 2]$.\n");
    insta::assert_snapshot!(to_svg(&fig.scene_2d(420.0, 320.0, &PlotTheme::dark())));
}

#[test]
fn svg_snapshot_area_between_light() {
    let fig = small("Figure: $x$, $x^2$, area between.\n");
    insta::assert_snapshot!(to_svg(&fig.scene_2d(420.0, 320.0, &PlotTheme::light())));
}

#[test]
fn svg_snapshot_area_between_dark() {
    let fig = small("Figure: $x$, $x^2$, area between.\n");
    insta::assert_snapshot!(to_svg(&fig.scene_2d(420.0, 320.0, &PlotTheme::dark())));
}

#[test]
fn svg_snapshot_newton_light() {
    let fig = small("Figure: $x^2 - 2$, newton from $x_0 = 3$, $n = 2$, $x \\in [0, 3.5]$.\n");
    insta::assert_snapshot!(to_svg(&fig.scene_2d(420.0, 320.0, &PlotTheme::light())));
}

#[test]
fn svg_snapshot_taylor_dark() {
    let fig = small("Figure: $\\sin x$, taylor $n = 3$, $x \\in [-4, 4]$.\n");
    insta::assert_snapshot!(to_svg(&fig.scene_2d(420.0, 320.0, &PlotTheme::dark())));
}

#[test]
fn svg_snapshot_slope_field_light() {
    let fig =
        small("Figure: slope field $y' = x - y, (0, 1)$, $x \\in [-2, 2]$, $y \\in [-2, 2]$.\n");
    insta::assert_snapshot!(to_svg(&fig.scene_2d(360.0, 280.0, &PlotTheme::light())));
}
