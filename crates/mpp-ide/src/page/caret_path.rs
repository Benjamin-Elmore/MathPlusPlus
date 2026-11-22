//! Where the caret is inside maths: the structures holding it, from the outside
//! in ("fraction › denominator"). The status bar says it (U13) and the page
//! boxes the innermost part and, more faintly, its structure (U14).

use egui::{Painter, Pos2, Rect, Stroke, StrokeKind};
use mpp_render::HitKind;
use mpp_syntax::document::{BlockKind, Document, Inline, InlineKind};
use mpp_syntax::math::{visit, MathAst, Node, NodeKind};
use mpp_syntax::Span;

use super::Page;
use crate::theme::{c32, Palette};

/// One structure around the caret and the part of it the caret is in.
#[derive(Debug, Clone, PartialEq)]
pub struct Step {
    /// The structure ("fraction", "root", "integral").
    pub what: &'static str,
    /// The part ("denominator", "radicand", "upper bound").
    pub part: &'static str,
    /// The source of the part and of the whole structure.
    pub part_span: Span,
    pub whole_span: Span,
}

/// The maths (display or inline) whose text holds `offset`.
pub fn maths_at(doc: &Document, offset: usize) -> Option<&MathAst> {
    fn inlines(items: &[Inline], offset: usize) -> Option<&MathAst> {
        for i in items {
            match &i.kind {
                InlineKind::Math(m) if m.raw.start <= offset && offset <= m.raw.end => {
                    return Some(&m.ast);
                }
                InlineKind::Emphasis(inner)
                | InlineKind::Strong(inner)
                | InlineKind::Link { text: inner, .. } => {
                    if let Some(a) = inlines(inner, offset) {
                        return Some(a);
                    }
                }
                _ => {}
            }
        }
        None
    }
    fn blocks(bs: &[mpp_syntax::document::Block], offset: usize) -> Option<&MathAst> {
        for b in bs {
            if offset < b.span.start || offset > b.span.end {
                continue;
            }
            let found = match &b.kind {
                BlockKind::DisplayMath(m) if m.raw.start <= offset && offset <= m.raw.end => {
                    Some(&m.ast)
                }
                BlockKind::Paragraph(items) => inlines(items, offset),
                BlockKind::Let(l) => inlines(&l.content, offset),
                BlockKind::Theorem(t) => blocks(&t.body, offset),
                _ => None,
            };
            if found.is_some() {
                return found;
            }
        }
        None
    }
    blocks(&doc.blocks, offset)
}

/// The structure `parent` is, and the part of it `child` is; `None` when the
/// parent is not a structure with parts (a sum of terms, a product).
fn role(parent: &Node, child: &Node) -> Option<(&'static str, &'static str)> {
    use NodeKind as K;
    let is = |n: &Node| std::ptr::eq(n, child);
    let opt = |n: &Option<Node>| n.as_ref().is_some_and(is);
    Some(match &parent.kind {
        K::Frac { num, .. } => (
            "fraction",
            if is(num) { "numerator" } else { "denominator" },
        ),
        K::Pow { exp, .. } if is(exp) => ("power", "exponent"),
        K::Subscript { sub, .. } if is(sub) => ("subscript", "index"),
        K::Sqrt { index, .. } => (
            "root",
            if index.as_deref().is_some_and(is) {
                "index"
            } else {
                "radicand"
            },
        ),
        K::Binom { n, .. } => ("binomial", if is(n) { "top" } else { "bottom" }),
        K::Fenced { .. } | K::Group { .. } => ("brackets", "inside"),
        K::Integral(i) => (
            "integral",
            if opt(&i.lower) {
                "lower bound"
            } else if opt(&i.upper) {
                "upper bound"
            } else if is(&i.body) {
                "integrand"
            } else {
                "variable"
            },
        ),
        K::BigOp(b) => (
            match b.op {
                mpp_syntax::math::BigOpKind::Sum => "sum",
                _ => "product",
            },
            if opt(&b.to) {
                "upper limit"
            } else if is(&b.body) {
                "term"
            } else {
                "lower limit"
            },
        ),
        K::Limit(l) => (
            "limit",
            if is(&l.var) {
                "variable"
            } else if is(&l.target) {
                "approaches"
            } else {
                "expression"
            },
        ),
        _ => return None,
    })
}

fn holds(n: &Node, offset: usize) -> bool {
    n.span.start <= offset && offset <= n.span.end
}

/// The structures holding `offset` in the maths of `page`, outside in.
pub fn path_at(page: &Page, doc: &Document, offset: usize) -> Vec<Step> {
    let Some(ast) = maths_at(doc, offset) else {
        return Vec::new();
    };
    // The outermost node box holding the caret is where the walk starts.
    let Some(block) = page.block_at(offset) else {
        return Vec::new();
    };
    let root = block
        .dl
        .hits
        .iter()
        .filter(|h| h.kind != HitKind::Result && h.span.start <= offset && offset <= h.span.end)
        .filter_map(|h| mpp_render::node_at_path(ast, &h.path))
        .filter(|n| ast.span.start <= n.span.start && n.span.end <= ast.span.end)
        .max_by_key(|n| n.span.end - n.span.start);
    let Some(mut node) = root else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for _ in 0..64 {
        let mut next: Option<&Node> = None;
        visit::children(node, &mut |c| {
            // The child the caret is in; at a boundary, the one it is inside.
            if holds(c, offset) && next.is_none_or(|n| n.span.end == offset) {
                next = Some(c);
            }
        });
        // Spaces typed at the end of a box (`\frac{1}{2 |}`) are in the box:
        // the part before them holds the caret while the structure does,
        // since that is where the next character goes.
        if next.is_none() && offset < node.span.end {
            let mut last: Option<&Node> = None;
            visit::children(node, &mut |c| {
                if c.span.end <= offset && last.is_none_or(|l| c.span.end >= l.span.end) {
                    last = Some(c);
                }
            });
            next = last.filter(|c| role(node, c).is_some());
        }
        let Some(c) = next else { break };
        if let Some((what, part)) = role(node, c) {
            out.push(Step {
                what,
                part,
                part_span: c.span,
                whole_span: node.span,
            });
        }
        node = c;
    }
    out
}

/// The status bar's words for where the caret is: "Prose", "Maths" or
/// "Maths › fraction › denominator".
pub fn describe(page: &Page, doc: &Document, offset: usize) -> String {
    if !page.in_math(offset) {
        return "Prose".into();
    }
    let mut s = String::from("Maths");
    for step in path_at(page, doc, offset) {
        s.push_str(" \u{203a} ");
        s.push_str(step.what);
        s.push_str(" \u{203a} ");
        s.push_str(step.part);
    }
    s
}

/// [`describe`] for the open file's caret, from its last laid-out page.
pub fn status_text(doc: &crate::doc::Doc) -> String {
    match doc.page_cache() {
        Some(pc) => describe(&pc.page, &pc.doc, doc.editor.caret()),
        None => "Prose".into(),
    }
}

/// Boxes the innermost part holding the caret, and more faintly the structure
/// it belongs to, in colours unlike the selection's.
pub fn paint_focus(
    painter: &Painter,
    page: &Page,
    doc: &Document,
    offset: usize,
    origin: Pos2,
    pal: &Palette,
) {
    let Some(step) = path_at(page, doc, offset).pop() else {
        return;
    };
    let off = origin.to_vec2();
    let rect = |s: Span| -> Option<Rect> { page.rect_of_span(s).map(|r| r.translate(off)) };
    if let Some(r) = rect(step.whole_span) {
        painter.rect_stroke(
            r.expand(3.0),
            3.0,
            Stroke::new(1.0, c32(pal.focus_parent)),
            StrokeKind::Outside,
        );
    }
    if let Some(r) = rect(step.part_span) {
        painter.rect_stroke(
            r.expand(1.5),
            2.0,
            Stroke::new(1.3, c32(pal.focus_box)),
            StrokeKind::Outside,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::page::{build_page, ExtraRefs, PageInput, ShowState};
    use crate::theme::Mode;
    use mpp_syntax::document::parse_document;

    fn at(src: &str, offset: usize) -> String {
        let doc = parse_document(src);
        let show = ShowState::default();
        let refs = ExtraRefs::default();
        let proof = crate::proof::ProofView::default();
        let page = build_page(&PageInput {
            src,
            doc: &doc,
            eval: None,
            theme: Mode::Light.theme(),
            size: 16.0,
            width: 700.0,
            integral_limits: mpp_render::IntegralLimits::AboveBelow,
            slot_labels: &[],
            show: &show,
            extra_refs: &refs,
            proof: &proof,
        });
        describe(&page, &doc, offset)
    }

    #[test]
    fn the_status_bar_says_where_the_caret_is() {
        let src = "Some words.\n\n$$ y := \\frac{1}{x + 2} $$\n";
        assert_eq!(at(src, 3), "Prose");
        let den = src.find("x + 2").unwrap() + 1;
        assert_eq!(at(src, den), "Maths \u{203a} fraction \u{203a} denominator");
        let num = src.find("{1}").unwrap() + 1;
        assert_eq!(at(src, num), "Maths \u{203a} fraction \u{203a} numerator");
        let y = src.find("y :=").unwrap();
        assert_eq!(at(src, y), "Maths");
    }

    #[test]
    fn integral_bounds_and_powers_are_named() {
        let src = "$$ \\int_0^{5} x^{2} \\, dx = ? $$\n";
        let up = src.find("{5}").unwrap() + 1;
        assert_eq!(at(src, up), "Maths \u{203a} integral \u{203a} upper bound");
        let exp = src.find("{2}").unwrap() + 1;
        assert_eq!(
            at(src, exp),
            "Maths \u{203a} integral \u{203a} integrand \u{203a} power \u{203a} exponent"
        );
    }

    #[test]
    fn spaces_at_the_end_of_a_box_are_in_the_box() {
        let src = "$$ q := \\frac{1}{2 } $$\n";
        let pos = src.find("2 }").unwrap() + 2;
        assert_eq!(at(src, pos), "Maths \u{203a} fraction \u{203a} denominator");
        let src = "$$ q := \\frac{1}{2} + 3 $$\n";
        let after = src.find("} + 3").unwrap() + 2;
        assert_eq!(at(src, after), "Maths");
    }

    #[test]
    fn inline_maths_counts_as_maths() {
        let src = "So $\\sqrt{a}$ is.\n";
        let a = src.find("{a}").unwrap() + 1;
        assert_eq!(at(src, a), "Maths \u{203a} root \u{203a} radicand");
    }
}
