//! Fix loop 2 of the persona panel (U28), editing: what the re-check found
//! when the engineer, the student and the teacher typed again.

use egui::{Key, Modifiers};

use super::runner::{run, Seen, Step, H};

fn t(h: &mut H, s: &'static str) {
    run(h, &[Step::Type(s)], |_, _, _| {});
}

fn k(h: &mut H, key: Key) {
    run(h, &[Step::Key(key)], |_, _, _| {});
}

fn pause(h: &mut H) {
    run(h, &[Step::Pause], |_, _, _| {});
}

fn show(h: &H, what: &str) {
    let s = Seen::of(h);
    println!(
        "{what}: {:?} caret {} drawn {:?} stop {:?}",
        s.text, s.caret, s.caret_at, s.stop_at
    );
}

fn in_math(h: &H) -> bool {
    let doc = h.state().doc().expect("doc");
    doc.page_cache()
        .expect("page")
        .page
        .in_math(doc.editor.caret())
}

/// The caret is drawn where the next character goes.
fn drawn_at_stop(h: &H, what: &str) {
    let s = Seen::of(h);
    let (a, b) = (s.caret_at.expect("drawn"), s.stop_at.expect("stop"));
    assert!(
        (a.x - b.x).abs() < 3.0 && (a.y - b.y).abs() < 3.0,
        "{what}: the caret is drawn at {a:?} but the next character goes at {b:?} in {:?}",
        s.text
    );
}

fn defs(h: &mut H) {
    t(h, "Projectile range");
    k(h, Key::Enter);
    t(h, "A ball is thrown at speed v and angle theta.");
    k(h, Key::Enter);
    super::runner::press(h, Modifiers::ALT, Key::Equals);
    t(h, "g = 9.81");
    k(h, Key::Enter);
    t(h, "v = 20");
    k(h, Key::Enter);
    t(h, "theta = pi/6");
    k(h, Key::Enter);
}

/// S-N1, S-N5: Esc out of a box onto the line under it, type, Backspace it all
/// away and once more: the empty line goes and the caret ends at the end of
/// the box, in maths (drawn there); the box's `$$` is never touched; what is
/// typed then goes where the caret is drawn.
#[test]
fn backspace_on_the_empty_line_under_a_box_never_eats_its_closing() {
    let mut h = super::runner::fresh();
    defs(&mut h);
    t(&mut h, "R = v^2 sin(2 theta) / g =");
    pause(&mut h);
    show(&h, "answered");
    k(&mut h, Key::Escape);
    show(&h, "esc");
    t(&mut h, "Now the integral");
    pause(&mut h);
    for i in 0..16 {
        k(&mut h, Key::Backspace);
        show(&h, &format!("bs{i}"));
    }
    pause(&mut h);
    show(&h, "bs16");
    drawn_at_stop(&h, "after 16 Backspaces");
    let box_text = {
        let s = Seen::of(&h).text;
        s[..s.rfind("$$").expect("closing") + 2].to_string()
    };
    k(&mut h, Key::Backspace);
    show(&h, "bs17");
    drawn_at_stop(&h, "after 17 Backspaces");
    let s = Seen::of(&h);
    assert!(
        s.text.starts_with(&box_text),
        "the box is untouched: {:?}",
        s.text
    );
    assert_eq!(s.text.matches("$$").count() % 2, 0, "{:?}", s.text);
    assert!(
        s.text.ends_with("= $$"),
        "the empty line went: {:?}",
        s.text
    );
    assert!(in_math(&h), "the caret is in the maths, where it is drawn");
    t(&mut h, "Check it:");
    pause(&mut h);
    show(&h, "typed");
    drawn_at_stop(&h, "after typing");
    let s = Seen::of(&h);
    assert!(s.text.ends_with("=Check it: $$"), "{:?}", s.text);
}

/// E-N1: Enter after a pause-answer behaves as Enter after `v*3 =` with no
/// pause: a new line, then prose. Ctrl+M on that sentence makes it maths,
/// never unwraps the box above it.
#[test]
fn enter_after_a_pause_answer_starts_a_prose_line() {
    let mut h = super::runner::fresh();
    t(&mut h, "v = 20");
    k(&mut h, Key::Enter);
    t(&mut h, "v*3 =");
    pause(&mut h);
    show(&h, "paused");
    k(&mut h, Key::Enter);
    show(&h, "enter1");
    k(&mut h, Key::Enter);
    show(&h, "enter2");
    t(&mut h, "The speed is fine");
    show(&h, "typed");
    assert!(!in_math(&h), "the sentence is prose");
    let s = Seen::of(&h);
    let (head, tail) = s.text.rsplit_once("$$").expect("a box");
    assert_eq!(tail.trim_start(), "The speed is fine", "{:?}", s.text);
    assert!(
        head.ends_with("v·3 = "),
        "the box keeps its answer: {:?}",
        s.text
    );
    super::runner::press(&mut h, Modifiers::COMMAND, Key::M);
    pause(&mut h);
    show(&h, "ctrl+m");
    let s = Seen::of(&h);
    assert!(s.text.contains("$$ v = 20 $$"), "{:?}", s.text);
    assert!(
        s.answers.iter().any(|a| a.contains("60")),
        "{:?}",
        s.answers
    );
}

fn status(h: &H) -> String {
    mpp_ide::page::caret_path::status_text(h.state().doc().expect("doc"))
}

/// S-N3, S-N4, P-N1: Ctrl+M wraps the maths at the end of a sentence; the
/// caret is drawn after it, in the sentence, as the status bar says. Esc (or
/// End) then Enter starts a new line; nothing goes inside the `$…$`.
#[test]
fn after_ctrl_m_esc_or_end_then_enter_starts_a_new_line() {
    for leave in [Some(Key::Escape), Some(Key::End), None] {
        let mut h = super::runner::fresh();
        t(&mut h, "v = 20");
        k(&mut h, Key::Enter);
        pause(&mut h);
        t(&mut h, "Half again is v/2 =");
        super::runner::press(&mut h, Modifiers::COMMAND, Key::M);
        pause(&mut h);
        show(&h, "ctrl+m");
        println!("status {}", status(&h));
        drawn_at_stop(&h, "after Ctrl+M");
        assert_eq!(status(&h), "Prose");
        if let Some(key) = leave {
            k(&mut h, key);
            show(&h, &format!("{key:?}"));
        }
        k(&mut h, Key::Enter);
        t(&mut h, "#Second part");
        show(&h, "heading");
        let s = Seen::of(&h);
        assert!(
            s.text.ends_with("Half again is $v/2 =$\n\n# Second part"),
            "{leave:?}: {:?}",
            s.text
        );
    }
}

/// S-N2, E-N3, P-N2: inline maths made by Alt+= in a sentence. Enter closes
/// it and starts a new prose line (as in Word); Right and Esc at its end
/// leave it, and what is typed then goes after it in the sentence.
#[test]
fn alt_equals_inline_maths_is_left_by_enter_right_and_esc() {
    for leave in [Key::Enter, Key::ArrowRight, Key::Escape, Key::End] {
        let mut h = super::runner::fresh();
        t(&mut h, "v = 20");
        k(&mut h, Key::Enter);
        pause(&mut h);
        t(&mut h, "Third is ");
        super::runner::press(&mut h, Modifiers::ALT, Key::Equals);
        t(&mut h, "v/2 =");
        pause(&mut h);
        show(&h, "typed");
        k(&mut h, leave);
        show(&h, &format!("{leave:?}"));
        drawn_at_stop(&h, "after leaving");
        assert!(!in_math(&h), "{leave:?} left the maths");
        t(&mut h, "next");
        let s = Seen::of(&h);
        show(&h, "next");
        let tail = s.text.rsplit_once('$').expect("closing $").1;
        if leave == Key::Enter {
            assert_eq!(tail, "\n\nnext", "{:?}", s.text);
        } else {
            assert_eq!(tail.trim_start(), "next", "{leave:?}: {:?}", s.text);
        }
        // A later Alt+= leaves no stray braces.
        k(&mut h, Key::Enter);
        t(&mut h, "Fourth is ");
        super::runner::press(&mut h, Modifiers::ALT, Key::Equals);
        t(&mut h, "v/4 =");
        k(&mut h, Key::ArrowRight);
        t(&mut h, " ok");
        let s = Seen::of(&h);
        show(&h, "fourth");
        let tail = &s.text[s.text.find("Fourth").expect("typed")..];
        assert!(!tail.contains("${") && !tail.contains("{}"), "{:?}", s.text);
        assert_eq!(
            tail.matches('{').count(),
            tail.matches('}').count(),
            "{tail:?}"
        );
        assert!(s.text.ends_with("$ ok"), "{leave:?}: {:?}", s.text);
    }
}
