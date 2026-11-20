//! Quick fixes: the 💡 menu and `Ctrl+.`.
//!
//! A fix changes the `.mpp` text behind the scenes (define a name, import it,
//! close a bracket, use a similar name) or does something for the user (go to
//! an empty box, open an imported file). Its title is words and typeset maths.

use std::path::PathBuf;

use mpp_syntax::document::{BlockKind, Document};
use mpp_syntax::Span;

use super::rich::Rich;
use super::{Kind, Problem};
use crate::editor::template::{parse_snippet, Knobs};
use crate::editor::Editor;

/// What a fix does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FixAction {
    /// Replaces a span of the text.
    Replace { span: Span, with: String },
    /// Several replacements made as one change (one undo step).
    Edits(Vec<(Span, String)>),
    /// Adds a definition `name := □` as a new maths block at `at`, with the
    /// caret in the empty box.
    Define { at: usize, name: String },
    /// Moves the caret (to an empty box).
    MoveCaret(usize),
    /// Opens a file.
    OpenFile(PathBuf),
}

/// One entry of the quick-fix menu.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QuickFix {
    pub title: Rich,
    pub action: FixAction,
}

/// What applying a fix asks of the app.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Applied {
    /// The text changed (or the caret moved).
    Done,
    /// The app should open this file.
    Open(PathBuf),
}

/// LaTeX spelling of a name the evaluator keys in Unicode (`ϑ` → `\vartheta`).
pub fn name_latex(key: &str) -> String {
    let mut out = String::new();
    let mut chars = key.chars().peekable();
    while let Some(c) = chars.next() {
        match mpp_syntax::symbols::lookup_char(c) {
            Some(s) if !c.is_ascii() && s.latex.starts_with('\\') => {
                out.push_str(s.latex);
                if chars.peek().is_some_and(|n| n.is_ascii_alphabetic()) {
                    out.push(' ');
                }
            }
            _ => out.push(c),
        }
    }
    out
}

/// Where a new definition of a name used in the block at `offset` goes: just
/// before that block.
fn define_at(doc: &Document, offset: usize) -> usize {
    let mut at = None;
    doc.walk(&mut |b| {
        if at.is_none() && b.span.start <= offset && offset <= b.span.end {
            at = Some(b.span.start);
        }
    });
    at.unwrap_or(0)
}

/// The edit that imports `name` (written `latex`) from `file`: added to an
/// existing `Using … from [[file]]` sentence, or a new sentence after the
/// first heading (at the top when there is none).
pub fn import_edit(src: &str, doc: &Document, latex: &str, file: &str) -> (Span, String) {
    let mut existing = None;
    let mut first_heading_end = None;
    doc.walk(&mut |b| match &b.kind {
        BlockKind::Using(u) if u.path == file && existing.is_none() => {
            existing = u.names.last().map(|n| n.span.end);
        }
        BlockKind::Heading { .. } if first_heading_end.is_none() && b.span.start == 0 => {
            first_heading_end = Some(b.span.end);
        }
        _ => {}
    });
    if let Some(end) = existing {
        return (Span::point(end), format!(", {latex}"));
    }
    let line = format!("Using ${latex}$ from [[{file}]].");
    match first_heading_end {
        Some(end) => {
            let rest = src.get(end..).unwrap_or("");
            let skip = rest.len() - rest.trim_start_matches('\n').len();
            (Span::point(end + skip), format!("{line}\n\n"))
        }
        None => (Span::point(0), format!("{line}\n\n")),
    }
}

/// The fixes for a problem, best first.
pub fn quick_fixes(p: &Problem, src: &str, doc: &Document) -> Vec<QuickFix> {
    let mut out = vec![];
    match &p.kind {
        Kind::Undefined {
            name,
            at,
            similar,
            defined_in,
            similar_elsewhere,
        } => {
            let span = at.unwrap_or(p.span);
            let written = src
                .get(span.start..span.end)
                .map(str::to_string)
                .unwrap_or_else(|| name_latex(name));
            for s in similar.iter().take(3) {
                out.push(QuickFix {
                    title: Rich::text("Use ").m(s.clone()).t(" instead"),
                    action: FixAction::Replace {
                        span,
                        with: name_latex(s),
                    },
                });
            }
            out.push(QuickFix {
                title: Rich::text("Define ").m(name.clone()),
                action: FixAction::Define {
                    at: define_at(doc, span.start),
                    name: written.clone(),
                },
            });
            for (s, f) in similar_elsewhere.iter().take(2) {
                let latex = name_latex(s);
                let import = import_edit(src, doc, &latex, f);
                out.push(QuickFix {
                    title: Rich::text("Use ").m(s.clone()).t(" from ").i(f.clone()),
                    action: FixAction::Edits(vec![(span, latex), import]),
                });
            }
            for f in defined_in.iter().take(3) {
                let (span, with) = import_edit(src, doc, &written, f);
                out.push(QuickFix {
                    title: Rich::text("Import ")
                        .m(name.clone())
                        .t(" from ")
                        .i(f.clone()),
                    action: FixAction::Replace { span, with },
                });
            }
        }
        Kind::MissingBracket { close, insert, .. } => {
            out.push(QuickFix {
                title: if close == "}" {
                    Rich::text("Close the bracket")
                } else {
                    Rich::text(format!("Close the bracket with {close}"))
                },
                action: FixAction::Replace {
                    span: Span::point(*insert),
                    with: close.clone(),
                },
            });
        }
        Kind::EmptyBox { .. } => out.push(QuickFix {
            title: Rich::text("Go to the empty box"),
            action: FixAction::MoveCaret(p.span.start),
        }),
        Kind::Import {
            path: Some(path),
            file,
            ..
        } => out.push(QuickFix {
            title: Rich::text("Open ").i(file.clone()),
            action: FixAction::OpenFile(path.clone()),
        }),
        Kind::Import {
            missing: true,
            file,
            similar,
            ..
        } => {
            // Replace the path inside the `[[…]]` of the import line.
            let line = src.get(p.span.start..p.span.end).unwrap_or("");
            if let Some(i) = line.find(file.as_str()) {
                let start = p.span.start + i;
                for s in similar {
                    out.push(QuickFix {
                        title: Rich::text("Use ").i(s.clone()),
                        action: FixAction::Replace {
                            span: Span::new(start, start + file.len()),
                            with: s.clone(),
                        },
                    });
                }
            }
        }
        // Converting units arrives with the units item: the parser cannot
        // read units on this branch, so no mismatch is reported yet.
        _ => {}
    }
    out
}

/// Several non-overlapping replacements as one replacement of the range that
/// covers them all.
pub fn combine(src: &str, edits: &[(Span, String)]) -> Option<(Span, String)> {
    let mut v: Vec<&(Span, String)> = edits.iter().collect();
    v.sort_by_key(|(s, _)| (s.start, s.end));
    let start = v.first()?.0.start;
    let end = v.iter().map(|(s, _)| s.end).max()?;
    let mut out = String::new();
    let mut at = start;
    for (s, with) in v {
        if s.start < at || s.end > src.len() {
            return None;
        }
        out.push_str(src.get(at..s.start)?);
        out.push_str(with);
        at = s.end;
    }
    out.push_str(src.get(at..end)?);
    Some((Span::new(start, end), out))
}

/// Applies a fix to the text (one undo step).
pub fn apply(editor: &mut Editor, fix: &QuickFix) -> Applied {
    match &fix.action {
        FixAction::Replace { span, with } => {
            editor.replace_node(*span, with);
            Applied::Done
        }
        FixAction::Edits(edits) => {
            if let Some((span, with)) = combine(editor.text(), edits) {
                editor.replace_node(span, &with);
            }
            Applied::Done
        }
        FixAction::Define { at, name } => {
            let snippet = format!("{name} := {{\u{27e8}*value\u{27e9}}}");
            match parse_snippet(&snippet) {
                Ok(s) => {
                    editor.insert_template(
                        *at..*at,
                        "$$ ",
                        &s,
                        " $$\n\n",
                        "define",
                        Knobs::default(),
                    );
                }
                Err(_) => {
                    editor.replace_node(Span::point(*at), &format!("$$ {name} := 1 $$\n\n"));
                }
            }
            Applied::Done
        }
        FixAction::MoveCaret(o) => {
            editor.move_caret(*o, false);
            Applied::Done
        }
        FixAction::OpenFile(p) => Applied::Open(p.clone()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mpp_syntax::document::parse_document;

    #[test]
    fn names_are_written_in_latex() {
        assert_eq!(name_latex("ϑ"), "\\vartheta");
        assert_eq!(name_latex("x"), "x");
    }

    #[test]
    fn importing_adds_to_an_existing_sentence_or_a_new_one() {
        let src = "# Title\n\nUsing $a$ from [[lib.mpp]].\n\n$$ b + a $$\n";
        let doc = parse_document(src);
        let (span, with) = import_edit(src, &doc, "b", "lib.mpp");
        let mut out = src.to_string();
        out.replace_range(span.start..span.end, &with);
        assert!(out.contains("Using $a, b$ from [[lib.mpp]]."), "{out}");

        let (span, with) = import_edit(src, &doc, "\\theta", "launch.mpp");
        let mut out = src.to_string();
        out.replace_range(span.start..span.end, &with);
        assert!(
            out.starts_with("# Title\n\nUsing $\\theta$ from [[launch.mpp]].\n\nUsing $a$"),
            "{out}"
        );
    }
}
