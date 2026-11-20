//! Slider drags must feel instant: rebuild times (strict in release builds).

use std::collections::BTreeMap;
use std::time::Instant;

use mpp_eval::Session;
use mpp_plot::{figures, Camera, Figure, PlotTheme};

fn build(src: &str) -> Figure {
    let mut session = Session::new();
    let ev = session.run_str(src);
    figures(ev)
        .into_iter()
        .next()
        .and_then(|f| f.figure.ok())
        .expect("a figure")
}

/// Best of several runs, in milliseconds.
fn best_ms(mut f: impl FnMut()) -> f64 {
    (0..7)
        .map(|_| {
            let t = Instant::now();
            f();
            t.elapsed().as_secs_f64() * 1000.0
        })
        .fold(f64::INFINITY, f64::min)
}

const LIMIT_SCALE: f64 = if cfg!(debug_assertions) { 40.0 } else { 1.0 };

#[test]
fn three_curves_rebuild_in_under_20_ms() {
    let src = "$$ a := 1, \\; a \\in [0, 5] $$\n\n\
               Figure: $\\sin(a x) + 0.3 \\cos(3 x)$, $x^2 / 10 - a$, $\\tan(x / a)$, $x \\in [-6, 6]$.\n";
    let mut fig = build(src);
    assert_eq!(fig.spec().layers.len(), 3);
    let mut v = BTreeMap::new();
    let mut a = 1.0;
    let ms = best_ms(|| {
        a += 0.37;
        v.insert("a".to_string(), 1.0 + a % 4.0);
        fig.rebuild(&v);
    });
    eprintln!("3 curves rebuild: {ms:.2} ms");
    assert!(ms < 20.0 * LIMIT_SCALE, "{ms} ms");
}

#[test]
fn a_60_by_60_surface_rebuilds_in_under_30_ms() {
    let src = "$$ a := 1, \\; a \\in [0, 3] $$\n\n\
               Figure: $z = \\sin(a x) \\cos(y) + x y / 10$, $x, y \\in [-3, 3]$.\n";
    let mut fig = build(src);
    let mut v = BTreeMap::new();
    let mut a = 0.0;
    let ms = best_ms(|| {
        a += 0.41;
        v.insert("a".to_string(), 0.5 + a % 2.0);
        fig.rebuild(&v);
    });
    eprintln!("60x60 surface rebuild: {ms:.2} ms");
    assert!(ms < 30.0 * LIMIT_SCALE, "{ms} ms");
}

#[test]
fn drawing_a_surface_scene_is_quick_enough_to_orbit() {
    let fig = build("Figure: $z = \\sin x \\cos y$, $x, y \\in [-3, 3]$.\n");
    let mut cam = Camera::default();
    let theme = PlotTheme::light();
    let ms = best_ms(|| {
        cam.orbit(3.0, 0.5);
        let scene = fig.scene_3d(&cam, 640.0, 440.0, &theme);
        assert!(scene.items.len() > 7000);
    });
    eprintln!("orbit frame: {ms:.2} ms");
    assert!(ms < 40.0 * LIMIT_SCALE, "{ms} ms");
}

#[test]
fn an_inequality_rebuilds_quickly() {
    let src = "$$ a := 1, \\; a \\in [0, 3] $$\n\nFigure: $y > a x^2 - 1$, $x, y \\in [-3, 3]$.\n";
    let mut fig = build(src);
    let mut v = BTreeMap::new();
    let mut a = 0.0;
    let ms = best_ms(|| {
        a += 0.3;
        v.insert("a".to_string(), a % 3.0);
        fig.rebuild(&v);
    });
    eprintln!("inequality rebuild: {ms:.2} ms");
    assert!(ms < 60.0 * LIMIT_SCALE, "{ms} ms");
}
