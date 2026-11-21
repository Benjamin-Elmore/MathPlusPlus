//! The outline of a file: headings, definitions and figures, as typeset LaTeX.

use mpp_eval::Evaluated;
use mpp_syntax::document::{BlockKind, Document, Inline, InlineKind};
use mpp_syntax::Span;

use crate::defs;

/// What an outline entry is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutlineKind {
    Heading(u8),
    Definition,
    Figure,
}

/// One outline entry, ready to typeset.
#[derive(Debug, Clone, PartialEq)]
pub struct OutlineItem {
    pub kind: OutlineKind,
    /// What to typeset (mixes `\text{...}` and maths).
    pub latex: String,
    /// Where it is in the file (click scrolls here).
    pub span: Span,
}

/// Words and maths of some inline content as one typesettable LaTeX string.
pub fn inlines_latex(inlines: &[Inline], src: &str) -> String {
    let mut out = String::new();
    push_inlines(inlines, src, &mut out);
    out.trim().to_string()
}

fn text_piece(out: &mut String, s: &str) {
    let clean: String = s
        .chars()
        .map(|c| match c {
            '\\' | '{' | '}' | '$' | '%' | '&' | '#' | '_' | '^' | '~' => ' ',
            c => c,
        })
        .collect();
    if clean.trim().is_empty() {
        if !clean.is_empty() {
            out.push_str("\\ ");
        }
        return;
    }
    out.push_str("\\text{");
    out.push_str(&clean);
    out.push('}');
}

fn push_inlines(inlines: &[Inline], src: &str, out: &mut String) {
    for i in inlines {
        match &i.kind {
            InlineKind::Text(t) | InlineKind::Code(t) => text_piece(out, t),
            InlineKind::SoftBreak => out.push_str("\\ "),
            InlineKind::Emphasis(v) | InlineKind::Strong(v) => push_inlines(v, src, out),
            InlineKind::Math(m) => {
                if let Some(raw) = src.get(m.raw.start..m.raw.end) {
                    out.push(' ');
                    out.push_str(raw.trim());
                    out.push(' ');
                }
            }
            InlineKind::WikiLink { path, alias } => {
                text_piece(out, alias.as_deref().unwrap_or(path));
            }
            InlineKind::Link { text, .. } => push_inlines(text, src, out),
            InlineKind::Escape(c) => text_piece(out, &c.to_string()),
            InlineKind::FootnoteRef(_) | InlineKind::EqRef { .. } | InlineKind::Cite(_) => {}
        }
    }
}

/// The outline of a parsed file. Definitions need an evaluation.
pub fn outline(doc: &Document, src: &str, ev: Option<&Evaluated>) -> Vec<OutlineItem> {
    let mut items = vec![];
    let mut figure = 0;
    doc.walk(&mut |b| match &b.kind {
        BlockKind::Heading { level, content } => {
            let latex = inlines_latex(content, src);
            if !latex.is_empty() {
                items.push(OutlineItem {
                    kind: OutlineKind::Heading(*level),
                    latex,
                    span: b.span,
                });
            }
        }
        BlockKind::Figure(f) => {
            figure += 1;
            let caption = inlines_latex(&f.caption, src);
            let latex = if caption.is_empty() {
                format!("\\text{{Figure {figure}}}")
            } else {
                format!("\\text{{Figure {figure}. }} {caption}")
            };
            items.push(OutlineItem {
                kind: OutlineKind::Figure,
                latex,
                span: b.span,
            });
        }
        _ => {}
    });
    if let Some(ev) = ev {
        for d in defs::all(ev) {
            if d.file.is_none() && d.span.end <= src.len() {
                items.push(OutlineItem {
                    kind: OutlineKind::Definition,
                    latex: defs::summary_latex(&d),
                    span: d.span,
                });
            }
        }
    }
    items.sort_by_key(|i| i.span.start);
    items
}

#[cfg(test)]
mod tests {
    use super::*;
    use mpp_syntax::document::parse_document;

    #[test]
    fn headings_and_figures_are_listed_in_order() {
        let src = "# Energy $E$\n\nSome words.\n\n## Motion\n\nFigure: y = x^2 for x in [0, 1]. A parabola.\n";
        let doc = parse_document(src);
        let o = outline(&doc, src, None);
        let kinds: Vec<OutlineKind> = o.iter().map(|i| i.kind).collect();
        assert_eq!(kinds[0], OutlineKind::Heading(1));
        assert!(o[0].latex.contains("Energy") && o[0].latex.contains('E'));
        assert_eq!(kinds[1], OutlineKind::Heading(2));
        assert!(o.iter().any(|i| i.kind == OutlineKind::Figure));
        assert!(o.windows(2).all(|w| w[0].span.start <= w[1].span.start));
    }
}
