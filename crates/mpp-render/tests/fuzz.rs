//! Layout never panics and always gives finite boxes, whatever the parser
//! hands it; the HTML renderer survives random documents.

use mpp_render::{
    layout_math, render_html_with, to_svg, HtmlOptions, IntegralLimits, LayoutOptions, LineResult,
    MathStyle, NoResults, Primitive,
};
use mpp_syntax::document::parse_document;
use mpp_syntax::math::parse_math;
use proptest::prelude::*;

const PIECES: &[&str] = &[
    "x",
    "y",
    "2",
    "3.5",
    "d",
    "e",
    "i",
    "f",
    "n",
    "(",
    ")",
    "[",
    "]",
    "{",
    "}",
    "\\{",
    "\\}",
    "|",
    "\\|",
    "+",
    "-",
    "*",
    "/",
    "^",
    "_",
    "'",
    "!",
    ",",
    "&",
    "=",
    "<",
    ">",
    "->",
    ":=",
    "?",
    "\\\\",
    "\\frac",
    "\\dfrac",
    "\\tfrac12",
    "\\sqrt",
    "\\sqrt[3]",
    "\\sum",
    "\\prod",
    "\\int",
    "\\iint",
    "\\oint",
    "\\lim",
    "\\to",
    "\\partial",
    "\\nabla",
    "\\cdot",
    "\\times",
    "\\infty",
    "\\pi",
    "\\alpha",
    "\\theta",
    "\\sin",
    "\\log",
    "\\max",
    "\\left(",
    "\\right)",
    "\\left|",
    "\\right|",
    "\\begin{pmatrix}",
    "\\end{pmatrix}",
    "\\begin{cases}",
    "\\end{cases}",
    "\\text{if}",
    "\\hat",
    "\\vec",
    "\\overline",
    "\\mathbb",
    "\\mathbf",
    "\\binom",
    "\\operatorname{tr}",
    "\\overset{?}{=}",
    "\\approx",
    "\\equiv",
    "\\in",
    "\\le",
    "\\,",
    "\\quad",
    "\\ldots",
    "dx",
    "\\label{a}",
    "\\begin{aligned}",
    "\\end{aligned}",
    "\\lfloor",
    "\\rfloor",
    "\\langle",
    "\\rangle",
    "%noeval",
    " ",
    "∑",
    "∫",
    "√",
    "²",
    "α",
];

fn maths() -> impl Strategy<Value = String> {
    prop::collection::vec(prop::sample::select(PIECES), 0..40).prop_map(|v| v.join(" "))
}

fn check(dl: &mpp_render::DisplayList) {
    assert!(dl.width.is_finite() && dl.height.is_finite() && dl.depth.is_finite());
    assert!(dl.width >= 0.0 && dl.height >= 0.0 && dl.depth >= 0.0);
    for p in &dl.items {
        if let Primitive::Glyph { x, y, size, .. } = p {
            assert!(x.is_finite() && y.is_finite() && size.is_finite());
        }
    }
    for h in &dl.hits {
        assert!(h.rect.x.is_finite() && h.rect.y.is_finite());
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(400))]

    #[test]
    fn layout_never_panics(src in maths(), text in any::<bool>(), side in any::<bool>()) {
        let (ast, _) = parse_math(&src, 0);
        let opts = LayoutOptions {
            style: if text { MathStyle::Text } else { MathStyle::Display },
            integral_limits: if side { IntegralLimits::Side } else { IntegralLimits::AboveBelow },
            equation_number: Some("1".into()),
            width: Some(300.0),
            results: vec![LineResult::filled(0, src.clone()), LineResult::ghost(1, "x")],
            ..LayoutOptions::default()
        };
        let dl = layout_math(&ast, &opts);
        check(&dl);
        let _ = dl.caret_stops();
        let _ = dl.hit_test(dl.width / 2.0, dl.height);
        let svg = to_svg(&dl);
        prop_assert!(svg.starts_with("<svg"));
    }

    #[test]
    fn deep_nesting_is_bounded(depth in 1usize..80) {
        let src = format!("{}x{}", "x^{".repeat(depth), "}".repeat(depth));
        let (ast, _) = parse_math(&src, 0);
        check(&layout_math(&ast, &LayoutOptions::default()));
        let src = format!("{}1{}", "\\frac{1}{".repeat(depth), "}".repeat(depth));
        let (ast, _) = parse_math(&src, 0);
        check(&layout_math(&ast, &LayoutOptions::default()));
    }

    #[test]
    fn html_never_panics(lines in prop::collection::vec(
        prop::sample::select(&[
            "# Title", "## Part", "Some *text* with $x^2$.", "$$ \\int_0^1 x \\, dx = ? $$",
            "- item $a$", "1. first", "| a | b |", "|---|---|", "| $x$ | 2 |", "> quote",
            "**Theorem.** All $x$.", "**Proof.** Done. ∎", "Figure: $y = x^2$, $x \\in [0, 1]$.",
            "Using $f$ from [[a.mpp]].", "![[b.mpp]]", "Find the domain of $f$.", "Solve $x = 1$.",
            "[^n]: note", "see[^n]", "```", "code", "---", "%noeval", "", "\\eqref{e}",
            "$$ a \\label{e} $$", "[link](javascript:alert(1))", "<script>",
        ][..]),
        0..20,
    )) {
        let doc = parse_document(&lines.join("\n"));
        let html = render_html_with(&doc, &NoResults, &HtmlOptions { embed_fonts: false, ..HtmlOptions::default() });
        prop_assert!(html.starts_with("<!DOCTYPE html>"));
        prop_assert!(!html.contains("<script>"));
        prop_assert!(!html.to_ascii_lowercase().contains("href=\"javascript"));
    }
}
