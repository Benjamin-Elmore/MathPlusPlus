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

/// The caret is drawn, below every maths box, on the line where the text goes.
fn caret_on_its_line(h: &H) {
    let s = Seen::of(h);
    let at = s.caret_at.expect("the caret is drawn");
    let stop = s.stop_at.expect("the caret has a stop");
    assert!(
        (stop.x - at.x).abs() < 3.0 && (stop.y - at.y).abs() < 3.0,
        "the caret was drawn at {at:?} but text goes at {stop:?} in {:?}",
        s.text
    );
    let focused = h.state().doc().is_some_and(|d| d.caret_screen.is_some());
    assert!(focused, "the page lost the keyboard");
}

/// The page draws `words` (the page's text leaves the spaces out).
fn shows(h: &H, words: &str) -> bool {
    let want: String = words.split_whitespace().collect();
    Seen::of(h).visible.iter().any(|v| v.contains(&want))
}

/// The page holds the keyboard: a typed key lands in the text.
fn keys_land(h: &mut H, s: &str) {
    let before = text_of(h);
    type_text(h, s);
    let after = text_of(h);
    assert!(
        after.contains(s) && after.len() > before.len(),
        "{s:?} was lost: {before:?} -> {after:?}"
    );
}

/// T2: `$x = 2$`, Esc, then words: the words go into prose under the maths,
/// and the caret is drawn where they go.
#[test]
fn esc_after_maths_lands_in_prose() {
    let mut h = fresh();
    type_text(&mut h, "$x = 2$");
    press(&mut h, Modifiers::NONE, Key::Escape);
    keys_land(&mut h, "then some words");
    assert_eq!(text_of(&h), "$$ x = 2 $$\n\nthen some words");
    assert!(!in_math(&h));
    caret_on_its_line(&h);
    assert!(shows(&h, "then some words"));
}

/// S1: Esc from inside a maths box lands on a new prose line under the box;
/// what is typed shows, and the caret is drawn there.
#[test]
fn esc_inside_a_maths_box_lands_on_a_line_under_it() {
    let mut h = fresh();
    type_text(&mut h, "A ball.");
    enter(&mut h);
    press(&mut h, Modifiers::ALT, Key::Equals);
    type_text(&mut h, "g = 9.81");
    enter(&mut h);
    type_text(&mut h, "v = 20");
    assert!(in_math(&h));
    let box_bottom = Seen::of(&h).caret_at.expect("caret").y;
    press(&mut h, Modifiers::NONE, Key::Escape);
    assert!(!in_math(&h), "Esc left the maths");
    keys_land(&mut h, "Now the integral");
    let text = text_of(&h);
    assert!(
        text.ends_with("$$\n\nNow the integral"),
        "the words are on a line of their own: {text:?}"
    );
    caret_on_its_line(&h);
    assert!(Seen::of(&h).caret_at.expect("caret").y > box_bottom + 5.0);
    assert!(shows(&h, "Now the integral"));
}

/// T3: Right at the end of the maths leaves it: the words go into prose.
#[test]
fn right_at_the_end_of_maths_leaves_it() {
    for typed in ["$x = 2$", "$x = 2"] {
        let mut h = fresh();
        type_text(&mut h, typed);
        press(&mut h, Modifiers::NONE, Key::ArrowRight);
        assert!(!in_math(&h), "{typed}: Right left the maths");
        keys_land(&mut h, " and words");
        assert_eq!(text_of(&h), "$$ x = 2 $$\n\n and words", "after {typed}");
        assert!(shows(&h, "and words"));
        caret_on_its_line(&h);
    }
}
