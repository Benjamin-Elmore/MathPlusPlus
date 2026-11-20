//! Geometry checks: limits above and below, scripts raised and shrunk,
//! fractions centred, hit testing and carets.

use mpp_render::{
    layout_latex, layout_math, node_at_path, DisplayList, Fonts, HitKind, IntegralLimits,
    LayoutOptions, LineResult, Primitive, Theme,
};
use mpp_syntax::math::{parse_math, NodeKind};

/// A glyph's ink box in pixels: (left, top, right, bottom, size).
#[derive(Debug, Clone, Copy)]
struct Ink {
    left: f32,
    top: f32,
    right: f32,
    bottom: f32,
    baseline: f32,
    size: f32,
}

fn glyphs(dl: &DisplayList, ch: char) -> Vec<Ink> {
    dl.items
        .iter()
        .filter_map(|p| match p {
            Primitive::Glyph {
                font,
                glyph,
                ch: c,
                x,
                y,
                size,
                ..
            } if *c == ch => {
                let m = Fonts::get().metrics(*font, *glyph);
                Some(Ink {
                    left: x + m.x_min * size,
                    right: x + m.x_max * size,
                    top: y - m.y_max * size,
                    bottom: y - m.y_min * size,
                    baseline: *y,
                    size: *size,
                })
            }
            _ => None,
        })
        .collect()
}

fn one(dl: &DisplayList, ch: char) -> Ink {
    let v = glyphs(dl, ch);
    assert_eq!(v.len(), 1, "expected one {ch:?}, found {v:?}");
    v[0]
}

fn display(src: &str) -> DisplayList {
    layout_latex(src, &LayoutOptions::display(20.0))
}

#[test]
fn sum_limits_are_above_and_below() {
    let dl = display(r"\sum_{n=1}^{N} a_n");
    let sigma = one(&dl, '∑');
    let upper = one(&dl, '𝑁');
    let lower = one(&dl, '1');
    assert!(upper.bottom < sigma.top, "upper {upper:?} sigma {sigma:?}");
    assert!(lower.top > sigma.bottom, "lower {lower:?} sigma {sigma:?}");
    // Centred over the sign.
    let mid = (sigma.left + sigma.right) / 2.0;
    assert!(((upper.left + upper.right) / 2.0 - mid).abs() < 4.0);
}

#[test]
fn inline_sum_limits_are_at_the_side() {
    let dl = layout_latex(r"\sum_{n=1}^{N} a_n", &LayoutOptions::inline(20.0));
    let sigma = one(&dl, '∑');
    let upper = one(&dl, '𝑁');
    assert!(upper.left > sigma.right - 1.0);
}

#[test]
fn integral_bounds_above_and_below_by_default() {
    let dl = display(r"\int_0^1 x^2 \, dx");
    let sign = one(&dl, '∫');
    let upper = one(&dl, '1');
    let lower = one(&dl, '0');
    assert!(upper.bottom < sign.top);
    assert!(lower.top > sign.bottom);
    // The display integral is the large variant.
    assert!(sign.bottom - sign.top > 1.8 * 20.0);
}

#[test]
fn integral_bounds_at_the_side_with_the_option() {
    let opts = LayoutOptions {
        integral_limits: IntegralLimits::Side,
        ..LayoutOptions::display(20.0)
    };
    let dl = layout_latex(r"\int_0^1 x^2 \, dx", &opts);
    let sign = one(&dl, '∫');
    let upper = one(&dl, '1');
    let lower = one(&dl, '0');
    assert!(upper.left > (sign.left + sign.right) / 2.0);
    // Beside the top of the sign, not above it.
    assert!(upper.top > sign.top - 0.3 * 20.0 && upper.bottom < sign.bottom);
    assert!(lower.top > sign.top && lower.bottom > (sign.top + sign.bottom) / 2.0);
}

#[test]
fn fractions_are_centred_over_the_rule() {
    let dl = display(r"\frac{a}{b + c + d}");
    let rule = dl
        .items
        .iter()
        .find_map(|p| match p {
            Primitive::Rule { rect, .. } => Some(*rect),
            _ => None,
        })
        .expect("a fraction rule");
    let a = one(&dl, '𝑎');
    let mid = rule.x + rule.w / 2.0;
    assert!(((a.left + a.right) / 2.0 - mid).abs() < 2.0);
    assert!(a.bottom < rule.y);
    let b = one(&dl, '𝑏');
    assert!(b.top > rule.y + rule.h);
}

#[test]
fn superscripts_are_raised_and_smaller() {
    let dl = display(r"x^{y^z}");
    let x = one(&dl, '𝑥');
    let y = one(&dl, '𝑦');
    let z = one(&dl, '𝑧');
    assert!(y.baseline < x.baseline - 0.25 * 20.0);
    assert!(z.baseline < y.baseline);
    let script = y.size / x.size;
    let scriptscript = z.size / x.size;
    assert!((0.65..=0.75).contains(&script), "script {script}");
    assert!(
        (0.45..=0.6).contains(&scriptscript),
        "scriptscript {scriptscript}"
    );
}

#[test]
fn subscripts_are_lowered() {
    let dl = display(r"a_n");
    let a = one(&dl, '𝑎');
    let n = one(&dl, '𝑛');
    assert!(n.baseline > a.baseline);
    assert!(n.left >= a.right - 1.0);
}

#[test]
fn limit_variable_goes_under_lim() {
    let dl = display(r"\lim_{x \to 0} f(x)");
    let arrow = one(&dl, '→');
    let i = one(&dl, 'i');
    assert!(arrow.top > i.bottom);
}

#[test]
fn delimiters_grow_with_their_content() {
    let small = display(r"(x)");
    let big = display(r"\left( \frac{\frac{a}{b}}{\frac{c}{d}} \right)");
    let h = |dl: &DisplayList| {
        let p = glyphs(dl, '(');
        p.iter().map(|g| g.bottom).fold(f32::MIN, f32::max)
            - p.iter().map(|g| g.top).fold(f32::MAX, f32::min)
    };
    assert!(h(&big) > 2.0 * h(&small));
}

#[test]
fn very_tall_delimiters_are_assembled() {
    let rows = ["1"; 12].join(r" \\ ");
    let dl = display(&format!(r"\begin{{pmatrix}} {rows} \end{{pmatrix}}"));
    // An assembly is several pieces per side.
    assert!(glyphs(&dl, '(').len() >= 3);
    let pieces = glyphs(&dl, '(');
    let top = pieces.iter().map(|g| g.top).fold(f32::MAX, f32::min);
    let bottom = pieces.iter().map(|g| g.bottom).fold(f32::MIN, f32::max);
    let ones = glyphs(&dl, '1');
    let first = ones.iter().map(|o| o.top).fold(f32::MAX, f32::min);
    let last = ones.iter().map(|o| o.bottom).fold(f32::MIN, f32::max);
    // TeX lets a delimiter fall short by \delimitershortfall (0.5em in all).
    assert!(
        first >= top - 5.0 && last <= bottom + 5.0,
        "paren {top}..{bottom}, digits {first}..{last}, pieces {pieces:?}"
    );
}

#[test]
fn hit_test_finds_the_innermost_node() {
    let (ast, _) = parse_math(r"x^2 + y", 0);
    let dl = layout_math(&ast, &LayoutOptions::display(20.0));
    let two = one(&dl, '2');
    let hit = dl
        .hit_test((two.left + two.right) / 2.0, (two.top + two.bottom) / 2.0)
        .expect("a hit");
    let node = node_at_path(&ast, &hit.path).expect("path resolves");
    assert!(matches!(&node.kind, NodeKind::Number(s) if s == "2"));
    assert_eq!(hit.span, node.span);
    // Outside the formula there is nothing.
    assert!(dl.hit_test(-5.0, -5.0).is_none());
}

#[test]
fn carets_cover_the_source_in_order() {
    let (ast, _) = parse_math(r"a + \frac{b}{c}", 0);
    let dl = layout_math(&ast, &LayoutOptions::display(20.0));
    let stops = dl.caret_stops();
    assert!(stops.len() >= 6);
    assert!(stops.windows(2).all(|w| w[0].offset <= w[1].offset));
    let c = dl.caret_at(13).expect("a caret for c");
    let cg = one(&dl, '𝑐');
    assert!(c.x >= cg.left - 1.0 && c.top < cg.bottom);
}

#[test]
fn empty_slots_are_labelled_boxes() {
    let (ast, _) = parse_math(r"\int_{}^{} \, dx", 0);
    let dl = layout_math(&ast, &LayoutOptions::display(20.0));
    let labels: Vec<_> = dl.slots().iter().filter_map(|s| s.label.clone()).collect();
    assert_eq!(labels, ["lower", "upper"]);
    let outlines = dl
        .items
        .iter()
        .filter(|p| matches!(p, Primitive::Outline { dotted: true, .. }))
        .count();
    assert_eq!(outlines, 2);
    let sign = one(&dl, '∫');
    let slots = dl.slots();
    let lower = slots[0];
    let upper = slots[1];
    assert!(upper.rect.y + upper.rect.h < sign.top);
    assert!(lower.rect.y > sign.bottom);
    assert_eq!(lower.kind, HitKind::Slot);
}

#[test]
fn results_are_coloured_and_replace_the_question_mark() {
    let opts = LayoutOptions {
        results: vec![LineResult::filled(0, r"\frac{\pi^2}{6}")],
        ..LayoutOptions::display(20.0)
    };
    let dl = layout_latex(r"\sum_{n=1}^{\infty} \frac{1}{n^2} = ?", &opts);
    assert!(glyphs(&dl, '?').is_empty());
    let pi = dl.items.iter().find_map(|p| match p {
        Primitive::Glyph {
            ch: '𝜋', color, ..
        } => Some(*color),
        _ => None,
    });
    assert_eq!(pi, Some(Theme::LIGHT.result));
    assert!(dl.hits.iter().any(|h| h.kind == HitKind::Result));

    let ghost = LayoutOptions {
        results: vec![LineResult::ghost(0, "2")],
        ..LayoutOptions::display(20.0)
    };
    let dl = layout_latex(r"1 + 1", &ghost);
    let colours: Vec<_> = dl
        .items
        .iter()
        .filter_map(|p| match p {
            Primitive::Glyph { ch: '2', color, .. } => Some(*color),
            _ => None,
        })
        .collect();
    assert_eq!(colours, [Theme::LIGHT.auto_result]);
    assert_eq!(glyphs(&dl, '=').len(), 1);
}

#[test]
fn equation_numbers_sit_flush_right() {
    let opts = LayoutOptions {
        equation_number: Some("12".into()),
        width: Some(500.0),
        ..LayoutOptions::display(20.0)
    };
    let dl = layout_latex(r"a = b", &opts);
    assert!((dl.width - 500.0).abs() < 0.01);
    let close = one(&dl, ')');
    assert!(close.right > 490.0);
    let a = one(&dl, '𝑎');
    assert!(a.left > 200.0 && a.left < 250.0, "centred: {a:?}");
}

#[test]
fn bounding_box_contains_the_ink() {
    for src in [
        r"\sum_{n=1}^{\infty} \frac{1}{n^2}",
        r"\sqrt[3]{\frac{a}{b}}",
        r"\int_0^1 f",
    ] {
        let dl = display(src);
        let inks: Vec<Ink> = dl
            .items
            .iter()
            .filter_map(|p| match p {
                Primitive::Glyph { ch, .. } => glyphs(&dl, *ch).into_iter().next(),
                _ => None,
            })
            .collect();
        for i in inks {
            assert!(
                i.top >= -2.0 && i.bottom <= dl.total_height() + 2.0,
                "{src}: {i:?}"
            );
        }
    }
}
