//! The example documents draw every figure they contain.

use std::path::PathBuf;

use mpp_eval::Session;
use mpp_plot::{figures, to_svg, Camera, PlotTheme};

fn example(name: &str) -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("examples")
        .join(name);
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

fn check(name: &str, expected: usize, three_d: bool) {
    let src = example(name);
    let mut session = Session::new();
    let ev = session.run_str(&src);
    assert!(ev.errors().is_empty(), "{name}: {:?}", ev.errors());
    let all = figures(ev);
    assert_eq!(all.len(), expected, "{name}: figure count");
    for (i, f) in all.into_iter().enumerate() {
        let fig = f
            .figure
            .unwrap_or_else(|e| panic!("{name} figure {i}: {e}"));
        assert_eq!(
            fig.warnings().count(),
            0,
            "{name} figure {i}: {:?}",
            fig.warnings().collect::<Vec<_>>()
        );
        assert_eq!(fig.is_3d(), three_d, "{name} figure {i}");
        for theme in [PlotTheme::light(), PlotTheme::dark()] {
            let scene = fig.scene(500.0, 360.0, &theme);
            assert!(scene.items.len() > 8, "{name} figure {i} drew nothing");
            let _ = to_svg(&scene);
        }
        if three_d {
            let _ = fig.scene_3d(&Camera::default(), 400.0, 300.0, &PlotTheme::light());
        }
    }
}

#[test]
fn graphs_2d_example() {
    check("graphs-2d.mpp", 16, false);
}

#[test]
fn graphs_3d_example() {
    check("graphs-3d.mpp", 7, true);
}
