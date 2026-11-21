//! The outline of a file: the title, headings, theorems, definitions, figures,
//! the reference list and (when shown) notes, as typeset LaTeX.

use mpp_eval::Evaluated;
use mpp_syntax::document::{BlockKind, Document, Inline, InlineKind, TitleKind};
use mpp_syntax::Span;

use crate::defs;

/// What an outline entry is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutlineKind {
    /// The `Title:` line: the top entry.
    Title,
    Heading(u8),
    /// A theorem-like block (`Theorem 2.1`, `Definition 3`, `Proof`).
    Theorem,
    Definition,
    Figure,
    References,
    /// A formatted comment (listed only while View → Comments is on).
    Note,
}

/// One outline entry, ready to typeset.
#[derive(Debug, Clone, PartialEq)]
pub struct OutlineItem {
    pub kind: OutlineKind,
    /// What to typeset (mixes `\text{...}` and maths).
    pub latex: String,
    /// Where it is in the file (click scrolls here).
    pub span: Span,
    /// A theorem's proof status from the checker ("proved", "2 gaps").
    pub status: Option<(String, crate::proof::Tone)>,
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
                let label = alias
                    .clone()
                    .unwrap_or_else(|| crate::page::display_name(path));
                text_piece(out, &label);
            }
            InlineKind::Link { text, .. } => push_inlines(text, src, out),
            InlineKind::Escape(c) => text_piece(out, &c.to_string()),
            InlineKind::FootnoteRef(_) | InlineKind::EqRef { .. } | InlineKind::Cite(_) => {}
        }
    }
}

/// The outline of a parsed file, without notes. Definitions need an evaluation.
pub fn outline(doc: &Document, src: &str, ev: Option<&Evaluated>) -> Vec<OutlineItem> {
    outline_with(doc, src, ev, false)
}

/// The outline, listing notes too when `notes` is on (View → Comments).
pub fn outline_with(
    doc: &Document,
    src: &str,
    ev: Option<&Evaluated>,
    notes: bool,
) -> Vec<OutlineItem> {
    outline_full(doc, src, ev, notes, None)
}

/// The outline with each theorem's proof status from the checker's report.
pub fn outline_full(
    doc: &Document,
    src: &str,
    ev: Option<&Evaluated>,
    notes: bool,
    proof: Option<&crate::proof::ProofView>,
) -> Vec<OutlineItem> {
    let mut items = vec![];
    let mut figure = 0;
    let paper = crate::page::PaperInfo::scan(doc, src, &crate::page::ExtraRefs::default());
    doc.walk(&mut |b| match &b.kind {
        BlockKind::Title(t) if t.kind == TitleKind::Title => {
            let latex = inlines_latex(&t.content, src);
            if !latex.is_empty() {
                items.push(OutlineItem {
                    kind: OutlineKind::Title,
                    latex,
                    span: b.span,
                    status: None,
                });
            }
        }
        BlockKind::Theorem(t) => {
            let mut label = paper.theorem_label(t.kind);
            if let Some(n) = &t.number {
                label = format!("{} {n}", t.kind.as_str());
            }
            let label = label.trim_end_matches('.');
            let mut latex = format!("\\text{{{label}}}");
            if let Some(title) = &t.title {
                latex.push_str(&format!(
                    "\\text{{ (}}{}\\text{{)}}",
                    inlines_latex(title, src)
                ));
            }
            let status = proof
                .and_then(|p| p.summary_for(b.span))
                .map(|l| (l.text.clone(), l.tone));
            items.push(OutlineItem {
                kind: OutlineKind::Theorem,
                latex,
                span: b.span,
                status,
            });
        }
        BlockKind::References(_) if paper.ref_block == Some(b.span) => {
            items.push(OutlineItem {
                kind: OutlineKind::References,
                latex: "\\text{References}".into(),
                span: b.span,
                status: None,
            });
        }
        BlockKind::Comment(c) if notes => {
            let words = crate::page::note_words(c, src, 32);
            let clean: String = words
                .chars()
                .filter(|ch| !matches!(ch, '{' | '}' | '\\' | '$'))
                .collect();
            items.push(OutlineItem {
                kind: OutlineKind::Note,
                latex: format!("\\text{{\u{270e} {clean}\u{2026}}}"),
                span: b.span,
                status: None,
            });
        }
        BlockKind::Heading { level, content } => {
            let mut latex = inlines_latex(content, src);
            let number = paper
                .heading_number(*level)
                .filter(|_| !mpp_syntax::document::heading_has_number(content));
            if let Some(n) = number {
                latex = format!("\\text{{{n}\u{2003}}}{latex}");
            }
            if !latex.is_empty() {
                items.push(OutlineItem {
                    kind: OutlineKind::Heading(*level),
                    latex,
                    span: b.span,
                    status: None,
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
                status: None,
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
                    status: None,
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
    fn paper_entries_are_listed() {
        let src = "Title: Motion\n\n%number sections\n\n# Range\n\n**Theorem.** True.\n\n**Definition.** A word.\n\n% A note here.\n\nReferences\n[@a] A | B | 2000\n";
        let doc = parse_document(src);
        let o = outline_with(&doc, src, None, true);
        let kinds: Vec<OutlineKind> = o.iter().map(|i| i.kind).collect();
        assert_eq!(kinds[0], OutlineKind::Title);
        let theorems: Vec<&str> = o
            .iter()
            .filter(|i| i.kind == OutlineKind::Theorem)
            .map(|i| i.latex.as_str())
            .collect();
        assert_eq!(theorems, ["\\text{Theorem 1.1}", "\\text{Definition 1.2}"]);
        assert!(kinds.contains(&OutlineKind::References));
        assert!(kinds.contains(&OutlineKind::Note));
        let without = outline(&doc, src, None);
        assert!(!without.iter().any(|i| i.kind == OutlineKind::Note));
    }

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
