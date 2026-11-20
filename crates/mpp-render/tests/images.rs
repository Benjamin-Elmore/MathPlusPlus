//! Rasterises the gallery to PNGs under `target/tmp/mpp-render-png/` for
//! eyeballing, and checks that every SVG we write parses as SVG.

mod common;

use std::path::PathBuf;

use mpp_render::{
    layout_latex, layout_math, to_svg_with, IntegralLimits, LayoutOptions, LineResult, SvgOptions,
};
use mpp_syntax::math::{parse_math, visit, Node, NodeKind, Statement};

fn save(name: &str, dl: &mpp_render::DisplayList) {
    let svg = to_svg_with(
        dl,
        &SvgOptions {
            outlines: true,
            padding: 6.0,
        },
    );
    let pix = rasterise(&svg, 1.0).expect("parses");
    let _ = pix.save_png(out_dir().join(format!("{name}.png")));
}

#[test]
fn editing_states_render_to_png() {
    let opts = LayoutOptions {
        results: vec![
            LineResult::filled(0, r"\frac{\pi^2}{6}"),
            LineResult::ghost(1, r"\frac12"),
        ],
        equation_number: Some("4".into()),
        width: Some(520.0),
        ..LayoutOptions::display(28.0)
    };
    let dl = layout_latex(
        r"\sum_{n=1}^{\infty} \frac{1}{n^2} = ? \\ \int_0^1 x \, dx",
        &opts,
    );
    save("results", &dl);

    fn blank(n: &mut Node) {
        if matches!(n.kind, NodeKind::Name(_)) {
            n.kind = NodeKind::Placeholder {
                label: String::new(),
            };
            return;
        }
        visit::children_mut(n, &mut blank);
    }
    let cases = [
        ("slots-integral", r"\int_a^b f \, dt"),
        ("slots-sum", r"\sum_{n=a}^{b} f"),
        ("slots-limit", r"\lim_{x \to a} f"),
        ("slots-fraction", r"\frac{a}{b}"),
    ];
    for (name, src) in cases {
        let (mut ast, _) = parse_math(src, 0);
        for line in &mut ast.lines {
            if let Statement::Relation(n) = &mut line.statement {
                blank(n);
            }
        }
        save(name, &layout_math(&ast, &LayoutOptions::display(28.0)));
    }
}

fn out_dir() -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("mpp-render-png");
    let _ = std::fs::create_dir_all(&dir);
    dir
}

fn rasterise(svg: &str, scale: f32) -> Option<resvg::tiny_skia::Pixmap> {
    let tree = resvg::usvg::Tree::from_str(svg, &resvg::usvg::Options::default()).ok()?;
    let size = tree.size();
    let w = (size.width() * scale).ceil().max(1.0) as u32;
    let h = (size.height() * scale).ceil().max(1.0) as u32;
    let mut pixmap = resvg::tiny_skia::Pixmap::new(w, h)?;
    pixmap.fill(resvg::tiny_skia::Color::WHITE);
    resvg::render(
        &tree,
        resvg::tiny_skia::Transform::from_scale(scale, scale),
        &mut pixmap.as_mut(),
    );
    Some(pixmap)
}

/// Stacks several formulas into one SVG sheet, one per row, left-aligned.
fn sheet(svgs: &[String], widths: &[f32], heights: &[f32]) -> String {
    let pad = 12.0;
    let width = widths.iter().copied().fold(0.0, f32::max) + 2.0 * pad;
    let mut y = pad;
    let mut body = String::new();
    for (i, s) in svgs.iter().enumerate() {
        // Drop the outer <svg …> tag and nest the content in a translated group.
        let inner = s
            .split_once('\n')
            .map(|(_, rest)| rest.trim_end().trim_end_matches("</svg>"))
            .unwrap_or("");
        body.push_str(&format!(
            "<g transform=\"translate({pad} {y})\">{inner}</g>\n"
        ));
        y += heights[i] + pad;
    }
    format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" xmlns:xlink=\"http://www.w3.org/1999/xlink\" width=\"{width}\" height=\"{y}\" viewBox=\"0 0 {width} {y}\">\n<rect width=\"100%\" height=\"100%\" fill=\"white\"/>\n{body}</svg>\n"
    )
}

#[test]
fn gallery_renders_to_png() {
    let dir = out_dir();
    let mut svgs = vec![];
    let mut widths = vec![];
    let mut heights = vec![];
    for (name, src) in common::CASES {
        let dl = layout_latex(src, &LayoutOptions::display(28.0));
        let svg = to_svg_with(
            &dl,
            &SvgOptions {
                outlines: true,
                padding: 4.0,
            },
        );
        let pix = rasterise(&svg, 1.0).unwrap_or_else(|| panic!("{name}: SVG did not parse"));
        let _ = pix.save_png(dir.join(format!("{name}.png")));
        widths.push(dl.width + 8.0);
        heights.push(dl.total_height() + 8.0);
        svgs.push(svg);
    }
    // Contact sheets of about a dozen formulas each.
    for (k, chunk) in svgs.chunks(12).enumerate() {
        let start = k * 12;
        let end = start + chunk.len();
        let s = sheet(chunk, &widths[start..end], &heights[start..end]);
        let pix = rasterise(&s, 1.0).expect("sheet parses");
        let _ = pix.save_png(dir.join(format!("sheet-{k}.png")));
    }
    // The side convention for integrals.
    let side = LayoutOptions {
        integral_limits: IntegralLimits::Side,
        ..LayoutOptions::display(28.0)
    };
    let dl = layout_latex(r"\int_0^1 x^2 \, dx", &side);
    let svg = to_svg_with(
        &dl,
        &SvgOptions {
            outlines: true,
            padding: 4.0,
        },
    );
    let pix = rasterise(&svg, 1.0).expect("parses");
    let _ = pix.save_png(dir.join("integral_side.png"));
}
