//! Fix round 2c of the persona panel (U28): editing and moving around. Each
//! test is what the teacher, the engineer, the student or the Python
//! programmer typed, through the real frame loop.

use egui::{Key, Modifiers};

use super::runner::{fresh, press, text_of, type_text, Seen, H};

fn enter(h: &mut H) {
    press(h, Modifiers::NONE, Key::Enter);
}

fn key(h: &mut H, k: Key) {
    press(h, Modifiers::NONE, k);
}

/// T9, E13 and P11: Enter continues a list; a `- ` typed at the start of the
/// new item is not doubled, and the empty bullet left at the end of the file
/// goes once the caret leaves it.
#[test]
fn a_typed_bullet_on_a_continued_list_is_not_doubled() {
    let mut h = fresh();
    type_text(&mut h, "- air resistance");
    enter(&mut h);
    type_text(&mut h, "- spin");
    assert_eq!(text_of(&h), "- air resistance\n- spin");
    enter(&mut h);
    type_text(&mut h, "* wind");
    assert_eq!(text_of(&h), "- air resistance\n- spin\n- wind");
    let mut h = fresh();
    type_text(&mut h, "1. first");
    enter(&mut h);
    type_text(&mut h, "2. second");
    assert_eq!(text_of(&h), "1. first\n2. second");
    // The empty bullet Enter leaves at the end goes when the caret leaves it.
    enter(&mut h);
    assert_eq!(text_of(&h), "1. first\n2. second\n3. ");
    key(&mut h, Key::ArrowUp);
    h.run_steps(2);
    assert_eq!(text_of(&h), "1. first\n2. second");
    assert!(Seen::of(&h).caret <= text_of(&h).len());
}
