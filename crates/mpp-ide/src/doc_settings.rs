//! The document switches under View → Document: "Number sections" and "Citation
//! style". They are file directives (`%number sections`, `%cite author-year`), not
//! IDE state, so the file reads the same everywhere it is opened; both are off by
//! default and a plain worksheet never needs them.
//!
//! Also the entries the Insert → Citation picker offers: the document's own
//! `References` entries, typeset.

use mpp_syntax::document::{parse_document, BlockKind, CiteStyle, Document, Reference};

const NUMBER_SECTIONS: &str = "%number sections";
const AUTHOR_YEAR: &str = "%cite author-year";

/// The lines of `src` with their end-of-line kept.
fn lines(src: &str) -> impl Iterator<Item = &str> {
    src.split_inclusive('\n')
}

fn is_number_sections(line: &str) -> bool {
    line.trim() == NUMBER_SECTIONS
}

fn is_cite(line: &str) -> bool {
    line.trim()
        .strip_prefix("%cite")
        .is_some_and(|rest| rest.is_empty() || rest.starts_with(char::is_whitespace))
}

/// Whether the file numbers its sections (`%number sections`).
pub fn number_sections(src: &str) -> bool {
    lines(src).any(is_number_sections)
}

/// The file's citation style (`%cite author-year`, numbered otherwise).
pub fn cite_style(src: &str) -> CiteStyle {
    parse_document(src).cite_style()
}

/// `src` without the lines `drop` matches, and with `add` as a new first line.
fn rewrite(src: &str, drop: impl Fn(&str) -> bool, add: Option<&str>) -> String {
    let newline = if src.contains("\r\n") { "\r\n" } else { "\n" };
    let mut out = String::with_capacity(src.len() + 32);
    if let Some(a) = add {
        out.push_str(a);
        out.push_str(newline);
    }
    for l in lines(src) {
        if !drop(l) {
            out.push_str(l);
        }
    }
    out
}

/// `src` with `%number sections` written (`on`) or removed.
pub fn set_number_sections(src: &str, on: bool) -> String {
    if number_sections(src) == on {
        return src.to_string();
    }
    rewrite(src, is_number_sections, on.then_some(NUMBER_SECTIONS))
}

/// `src` with the citation style set: `%cite author-year` for author–year, no
/// directive for the numbered default.
pub fn set_cite_style(src: &str, style: CiteStyle) -> String {
    if cite_style(src) == style && (style == CiteStyle::AuthorYear || !lines(src).any(is_cite)) {
        return src.to_string();
    }
    let add = (style == CiteStyle::AuthorYear).then_some(AUTHOR_YEAR);
    rewrite(src, is_cite, add)
}

/// One entry the citation picker offers.
#[derive(Debug, Clone, PartialEq)]
pub struct CiteChoice {
    pub key: String,
    /// The entry typeset: author, title in italics, year.
    pub latex: String,
}

/// Plain words for a `\text{…}` argument (characters LaTeX would read as markup
/// are left out).
fn clean(s: &str) -> String {
    s.chars()
        .filter(|c| !matches!(c, '{' | '}' | '\\' | '$' | '^' | '_' | '%' | '&' | '#'))
        .collect()
}

fn choice(r: &Reference) -> CiteChoice {
    let mut latex = format!(
        "\\text{{{}}}\\;\\textit{{{}}}",
        clean(&r.author),
        clean(&r.title)
    );
    if let Some(y) = &r.year {
        latex.push_str(&format!("\\;\\text{{({})}}", clean(y)));
    }
    CiteChoice {
        key: r.key.clone(),
        latex,
    }
}

/// The document's own reference entries, in the order they are written.
pub fn cite_choices(doc: &Document) -> Vec<CiteChoice> {
    let mut out = Vec::new();
    doc.walk(&mut |b| {
        if let BlockKind::References(refs) = &b.kind {
            out.extend(refs.entries.iter().map(choice));
        }
    });
    out
}

/// The text a chosen key inserts: `[@key]`.
pub fn citation(key: &str) -> String {
    format!("[@{key}]")
}

/// Replaces the editor's text with `new` as one undoable edit, keeping the caret on
/// the same text (the directives change only the top of the file).
pub fn apply(editor: &mut crate::editor::Editor, new: &str) {
    let old = editor.text().to_string();
    if old == new {
        return;
    }
    let caret = editor.caret();
    let delta = new.len() as isize - old.len() as isize;
    editor.replace_node(mpp_syntax::Span::new(0, old.len()), new);
    let moved = (caret as isize + delta).clamp(0, new.len() as isize) as usize;
    editor.move_caret(moved, false);
}

impl crate::IdeApp {
    /// View → Document: the file's "Number sections" and "Citation style" switches.
    pub(crate) fn document_menu(&mut self, ui: &mut egui::Ui) {
        let Some(doc) = self.doc_mut() else {
            return;
        };
        ui.menu_button("Document", |ui| {
            let src = doc.editor.text().to_string();
            let mut numbered = number_sections(&src);
            if ui.checkbox(&mut numbered, "Number sections").changed() {
                apply(&mut doc.editor, &set_number_sections(&src, numbered));
                doc.touched();
            }
            ui.separator();
            ui.label("Citation style");
            let style = cite_style(&src);
            for (s, label) in [
                (CiteStyle::Numbered, "Numbered  [1]"),
                (CiteStyle::AuthorYear, "Author\u{2013}year  (Knuth 1984)"),
            ] {
                if ui.radio(style == s, label).clicked() && style != s {
                    let now = doc.editor.text().to_string();
                    apply(&mut doc.editor, &set_cite_style(&now, s));
                    doc.touched();
                }
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DOC: &str = "Title: Motion\n\n# Range\n\nSee [@knuth].\n\nReferences\n[@knuth] Knuth, D. | The TeXbook | 1984\n[@lamport] Lamport, L. | LaTeX | 1994\n";

    #[test]
    fn both_switches_are_off_by_default() {
        assert!(!number_sections(DOC));
        assert_eq!(cite_style(DOC), CiteStyle::Numbered);
    }

    #[test]
    fn number_sections_writes_and_removes_its_directive() {
        let on = set_number_sections(DOC, true);
        assert!(on.starts_with("%number sections\n"));
        assert!(number_sections(&on));
        assert!(parse_document(&on).blocks.iter().any(|b| matches!(
            &b.kind,
            BlockKind::Directive(d) if d.kind == mpp_syntax::document::DirectiveKind::NumberSections
        )));
        // Writing it twice changes nothing; turning it off gives the file back.
        assert_eq!(set_number_sections(&on, true), on);
        assert_eq!(set_number_sections(&on, false), DOC);
    }

    #[test]
    fn citation_style_round_trips() {
        let ay = set_cite_style(DOC, CiteStyle::AuthorYear);
        assert_eq!(cite_style(&ay), CiteStyle::AuthorYear);
        assert_eq!(set_cite_style(&ay, CiteStyle::Numbered), DOC);
        // An explicit `%cite numbered` is removed (numbered is the default).
        let explicit = format!("%cite numbered\n{DOC}");
        assert_eq!(set_cite_style(&explicit, CiteStyle::Numbered), DOC);
        // Windows line ends are kept.
        let crlf = DOC.replace('\n', "\r\n");
        assert!(set_number_sections(&crlf, true).starts_with("%number sections\r\n"));
    }

    #[test]
    fn switching_is_one_undoable_edit() {
        let mut ed = crate::editor::Editor::new(DOC);
        ed.move_caret(DOC.len(), false);
        apply(&mut ed, &set_number_sections(DOC, true));
        assert!(number_sections(ed.text()));
        assert_eq!(ed.caret(), ed.text().len());
        assert!(ed.undo());
        assert_eq!(ed.text(), DOC);
    }

    #[test]
    fn the_picker_offers_the_references() {
        let doc = parse_document(DOC);
        let c = cite_choices(&doc);
        assert_eq!(c.len(), 2);
        assert_eq!(c[0].key, "knuth");
        assert!(c[0].latex.contains("TeXbook") && c[0].latex.contains("1984"));
        assert_eq!(citation("knuth"), "[@knuth]");
        assert!(cite_choices(&parse_document("Just $x = 1$.")).is_empty());
    }
}
