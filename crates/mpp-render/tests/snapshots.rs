//! SVG text snapshots of every construct. Glyphs are written as `<text>` so the
//! snapshots read and diff well; one snapshot keeps the outline form.

mod common;

use mpp_render::{
    layout_latex, layout_math, to_svg, to_svg_with, IntegralLimits, LayoutOptions, LineResult,
    MathStyle, SvgOptions,
};
use mpp_syntax::math::{parse_math, visit, NodeKind};

fn text_svg(src: &str, opts: &LayoutOptions) -> String {
    let dl = layout_latex(src, opts);
    to_svg_with(
        &dl,
        &SvgOptions {
            outlines: false,
            padding: 0.0,
        },
    )
}

#[test]
fn gallery_display() {
    for (name, src) in common::CASES {
        let svg = text_svg(src, &LayoutOptions::display(20.0));
        insta::assert_snapshot!(format!("display__{name}"), svg, src);
    }
}

#[test]
fn gallery_inline() {
    for name in [
        "sum",
        "integral",
        "limit",
        "fraction",
        "max_limits",
        "pmatrix",
    ] {
        let src = common::CASES
            .iter()
            .find(|(n, _)| *n == name)
            .map(|(_, s)| *s)
            .unwrap_or_default();
        let svg = text_svg(src, &LayoutOptions::inline(20.0));
        insta::assert_snapshot!(format!("inline__{name}"), svg, src);
    }
}

#[test]
fn integral_side_limits() {
    let opts = LayoutOptions {
        integral_limits: IntegralLimits::Side,
        ..LayoutOptions::display(20.0)
    };
    insta::assert_snapshot!(text_svg(r"\int_0^1 x^2 \, dx", &opts));
}

#[test]
fn outlines() {
    let dl = layout_latex(r"\frac{1}{x}", &LayoutOptions::display(20.0));
    insta::assert_snapshot!(to_svg(&dl));
}

#[test]
fn results_filled_and_ghost() {
    let src = r"\sum_{n=1}^{\infty} \frac{1}{n^2} = ? \\ \int_0^1 x \, dx";
    let opts = LayoutOptions {
        results: vec![
            LineResult::filled(0, r"\frac{\pi^2}{6}"),
            LineResult::ghost(1, r"\frac12"),
        ],
        ..LayoutOptions::display(20.0)
    };
    insta::assert_snapshot!(text_svg(src, &opts));
}

#[test]
fn equation_number() {
    let opts = LayoutOptions {
        equation_number: Some("3".into()),
        width: Some(400.0),
        ..LayoutOptions::display(20.0)
    };
    insta::assert_snapshot!(text_svg(r"E = m c^2", &opts));
}

#[test]
fn aligned_lines() {
    let src = r"\begin{aligned} (a + b)^2 &= (a + b)(a + b) \\ &= a^2 + 2ab + b^2 \end{aligned}";
    insta::assert_snapshot!(text_svg(src, &LayoutOptions::display(20.0)));
}

#[test]
fn template_slots() {
    let (mut ast, _) = parse_math(r"\int_a^b f \, dt", 0);
    // Turn every single-letter name into an empty template box.
    fn blank(n: &mut mpp_syntax::math::Node) {
        if matches!(n.kind, NodeKind::Name(_)) {
            n.kind = NodeKind::Placeholder {
                label: String::new(),
            };
            return;
        }
        visit::children_mut(n, &mut blank);
    }
    for line in &mut ast.lines {
        if let mpp_syntax::math::Statement::Relation(n) = &mut line.statement {
            blank(n);
        }
    }
    let dl = layout_math(&ast, &LayoutOptions::display(20.0));
    insta::assert_snapshot!(to_svg_with(
        &dl,
        &SvgOptions {
            outlines: false,
            padding: 0.0
        }
    ));
    let text = layout_math(
        &ast,
        &LayoutOptions {
            style: MathStyle::Text,
            ..LayoutOptions::default()
        },
    );
    assert!(!text.slots().is_empty());
}
