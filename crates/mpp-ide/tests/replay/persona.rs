//! Fix round 1 of the persona panel (U28): what the teacher, the engineer, the
//! student and the Python programmer typed through the real app, and lost work
//! or broke the file. Each test is their keystrokes, through the real frame loop.

use egui::{Key, Modifiers};
use mpp_syntax::document::{parse_document, BlockKind};

use super::runner::{fresh, press, text_of, type_text, Seen, H};

fn enter(h: &mut H) {
    press(h, Modifiers::NONE, Key::Enter);
}

/// The maths blocks of the text, and that every `$` belongs to one.
fn maths_blocks(text: &str) -> usize {
    parse_document(text)
        .blocks
        .iter()
        .filter(|b| matches!(b.kind, BlockKind::DisplayMath(_)))
        .count()
}

fn in_math(h: &H) -> bool {
    let doc = h.state().doc().expect("doc");
    doc.page_cache()
        .expect("page")
        .page
        .in_math(doc.editor.caret())
}

/// T1: a closing `$` steps over the whole closing `$$` and leaves the maths;
/// Enter then gives a clean new line, with no stray `$` left behind.
#[test]
fn a_typed_closing_dollar_leaves_the_maths() {
    let mut h = fresh();
    type_text(&mut h, "$g = 9.81$");
    assert_eq!(text_of(&h), "$$ g = 9.81 $$");
    assert_eq!(
        Seen::of(&h).caret,
        text_of(&h).len(),
        "the caret is after the maths"
    );
    assert!(!in_math(&h), "the caret left the maths");
    enter(&mut h);
    type_text(&mut h, "$v = 20$");
    enter(&mut h);
    let text = text_of(&h);
    assert_eq!(text, "$$ g = 9.81 $$\n\n$$ v = 20 $$\n\n");
    assert_eq!(maths_blocks(&text), 2);
    // `$$` typed by hand at the end closes it too, with no `$` left over.
    let mut h = fresh();
    type_text(&mut h, "$R := 35.31$$");
    assert_eq!(text_of(&h), "$$ R := 35.31 $$");
    enter(&mut h);
    type_text(&mut h, "$$ E = ? $$");
    assert_eq!(text_of(&h), "$$ R := 35.31 $$\n\n$$ E = ? $$");
}
