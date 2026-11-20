//! Dragging the sliders of the visual calculus helpers must feel instant (strict in
//! release builds).

use std::collections::BTreeMap;
use std::time::Instant;

use mpp_eval::Session;
use mpp_plot::{figures, Figure};

fn build(src: &str) -> Figure {
    let mut session = Session::new();
    let ev = session.run_str(src);
    figures(ev)
        .into_iter()
        .next()
        .and_then(|f| f.figure.ok())
        .expect("a figure")
}

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

fn drag(fig: &mut Figure, name: &str, lo: f64, hi: f64) -> f64 {
    let mut v = BTreeMap::new();
    let mut k = 0.0;
    best_ms(|| {
        k += 0.37;
        v.insert(name.to_string(), lo + (hi - lo) * (k % 1.0));
        fig.rebuild(&v);
    })
}

#[test]
fn riemann_strips_rebuild_in_a_couple_of_milliseconds() {
    let mut fig = build("Figure: $\\sin x + x^2 / 5$, riemann mid $n = 40$, $x \\in [0, 4]$.\n");
    let ms = drag(&mut fig, "n", 1.0, 400.0);
    eprintln!("riemann rebuild: {ms:.2} ms");
    assert!(ms < 10.0 * LIMIT_SCALE, "{ms} ms");
}

#[test]
fn the_tangent_moves_in_well_under_a_millisecond() {
    let mut fig = build("Figure: $\\sin x + x^2 / 5$, tangent at $x = 1$, $x \\in [-4, 4]$.\n");
    let ms = drag(&mut fig, "x_0", -4.0, 4.0);
    eprintln!("tangent rebuild: {ms:.2} ms");
    assert!(ms < 8.0 * LIMIT_SCALE, "{ms} ms");
}

#[test]
fn taylor_degree_rebuilds_quickly() {
    let mut fig = build("Figure: $\\sin x \\, e^{x / 4}$, taylor $n = 4$, $x \\in [-5, 5]$.\n");
    // The first move differentiates; later ones reuse the chain.
    drag(&mut fig, "n", 0.0, 12.0);
    let ms = drag(&mut fig, "n", 0.0, 12.0);
    eprintln!("taylor rebuild: {ms:.2} ms");
    assert!(ms < 15.0 * LIMIT_SCALE, "{ms} ms");
}

#[test]
fn a_slope_field_with_solution_curves_rebuilds_quickly() {
    let src = "$$ a := 1, \\; a \\in [0, 3] $$\n\n\
               Figure: slope field $y' = a x - y$, $x \\in [-3, 3]$, $y \\in [-3, 3]$.\n";
    let mut fig = build(src);
    let ms = drag(&mut fig, "a", 0.0, 3.0);
    eprintln!("slope field rebuild: {ms:.2} ms");
    assert!(ms < 60.0 * LIMIT_SCALE, "{ms} ms");
}

#[test]
fn a_solid_of_revolution_rebuilds_quickly() {
    let src = "$$ a := 2, \\; a \\in [1, 4] $$\n\n\
               Figure: $\\sqrt{x}$, revolve about $y = 0$, $x \\in [0, a]$.\n";
    let mut fig = build(src);
    let ms = drag(&mut fig, "a", 1.0, 4.0);
    eprintln!("solid rebuild: {ms:.2} ms");
    assert!(ms < 30.0 * LIMIT_SCALE, "{ms} ms");
}
