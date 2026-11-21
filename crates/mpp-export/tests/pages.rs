//! Renders the pages of the PDF to PNG (set MPP_PAGES_DIR) and checks the pages are drawn.

use std::path::{Path, PathBuf};

use mpp_export::{load, pdf_pages_svg, EvalProvider, ExportOptions};

fn example(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples")
        .join(name)
}

fn pages(name: &str, opts: &ExportOptions) -> Vec<String> {
    let loaded = load(&example(name)).expect("load");
    let p = EvalProvider::new(
        &loaded.evaluated,
        loaded.importer.clone(),
        loaded.base_dir.clone(),
        opts.dark,
        opts.include_results,
    );
    pdf_pages_svg(&loaded.evaluated, &p, opts)
}

fn to_png(svg: &str) -> Vec<u8> {
    let mut db = resvg::usvg::fontdb::Database::new();
    for f in mpp_render::FontId::ALL {
        db.load_font_data(f.bytes().to_vec());
    }
    let options = resvg::usvg::Options {
        fontdb: std::sync::Arc::new(db),
        ..Default::default()
    };
    let tree = resvg::usvg::Tree::from_str(svg, &options).expect("parse page");
    let scale = 1.6;
    let w = (tree.size().width() * scale) as u32;
    let h = (tree.size().height() * scale) as u32;
    let mut pm = resvg::tiny_skia::Pixmap::new(w, h).unwrap();
    pm.fill(resvg::tiny_skia::Color::WHITE);
    resvg::render(
        &tree,
        resvg::tiny_skia::Transform::from_scale(scale, scale),
        &mut pm.as_mut(),
    );
    pm.encode_png().unwrap()
}

#[test]
fn render_pages_for_looking_at() {
    let Ok(dir) = std::env::var("MPP_PAGES_DIR") else {
        return;
    };
    let names = std::env::var("MPP_PAGES").unwrap_or_else(|_| "units.mpp".into());
    for name in names.split(',') {
        for dark in [false, true] {
            let opts = ExportOptions {
                dark,
                include_comments: true,
                ..ExportOptions::default()
            };
            for (i, svg) in pages(name, &opts).iter().enumerate() {
                let file = format!(
                    "{dir}/{}{}-{}.png",
                    name.trim_end_matches(".mpp"),
                    if dark { "-dark" } else { "" },
                    i + 1
                );
                std::fs::write(file, to_png(svg)).unwrap();
            }
        }
    }
}
