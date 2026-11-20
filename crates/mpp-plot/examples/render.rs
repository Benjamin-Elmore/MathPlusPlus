//! Renders every figure of a `.mpp` file to PNG files.
//!
//! `cargo run -p mpp-plot --example render -- examples/graphs-2d.mpp out-dir [dark]`

use std::path::PathBuf;

use mpp_eval::Session;
use mpp_plot::{figures, to_png, PlotTheme};

fn main() {
    let mut args = std::env::args().skip(1);
    let (Some(file), Some(out)) = (args.next(), args.next()) else {
        eprintln!("usage: render <file.mpp> <out-dir> [dark]");
        return;
    };
    let theme = if args.next().as_deref() == Some("dark") {
        PlotTheme::dark()
    } else {
        PlotTheme::light()
    };
    let mut session = Session::new();
    let ev = match session.run_file(&file) {
        Ok(ev) => ev,
        Err(e) => {
            eprintln!("{file}: {e}");
            return;
        }
    };
    let out = PathBuf::from(out);
    if let Err(e) = std::fs::create_dir_all(&out) {
        eprintln!("{}: {e}", out.display());
        return;
    }
    for (i, f) in figures(ev).into_iter().enumerate() {
        match f.figure {
            Ok(fig) => {
                let scene = fig.scene(640.0, 440.0, &theme);
                match to_png(&scene, 1.5) {
                    Ok(png) => {
                        let path = out.join(format!("figure-{:02}.png", i + 1));
                        if let Err(e) = std::fs::write(&path, png) {
                            eprintln!("{}: {e}", path.display());
                        }
                    }
                    Err(e) => eprintln!("figure {}: {e}", i + 1),
                }
            }
            Err(e) => eprintln!("figure {}: {e}", i + 1),
        }
    }
}
