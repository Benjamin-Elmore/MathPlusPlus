//! Fix round 1 of the persona panel (U28): what the teacher, the engineer, the
//! student and the Python programmer typed through the real app, and lost work
//! or broke the file. Each test is their keystrokes, through the real frame loop.

use egui::{Key, Modifiers};
use mpp_syntax::document::{parse_document, BlockKind};

use super::runner::{fresh, press, text_of, type_text, Seen, Step, H};

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

/// Types `s` and pauses until the answers are on screen.
fn answered(h: &mut H, s: &'static str) -> Vec<String> {
    super::runner::run(h, &[Step::Type(s), Step::Pause], |_, _, _| {});
    Seen::of(h).answers
}

/// T4 and R3: the `^2` exponent the editor braced closes on the user's `}`
/// together with the group the user opened, so no stray `}` is left and the
/// sum has an answer.
#[test]
fn a_typed_brace_closes_the_users_own_group() {
    let mut h = fresh();
    type_text(&mut h, r"$\frac{1}{n^2} + 1");
    assert_eq!(text_of(&h), r"$$ \frac{1}{n^{2}} + 1 $$");
    let mut h = fresh();
    let answers = answered(&mut h, r"$\sum_{n=1}^{10} \frac{1}{n^2} =$");
    assert_eq!(text_of(&h), r"$$ \sum_{n=1}^{10} \frac{1}{n^{2}} = $$");
    assert!(!answers.is_empty(), "the sum has an answer");
    // Braces the user typed in full still close one at a time.
    let mut h = fresh();
    type_text(&mut h, r"$x^{2} + \frac{a}{b} + 1");
    assert_eq!(text_of(&h), r"$$ x^{2} + \frac{a}{b} + 1 $$");
}

/// R2, R4 and S5: a fraction made from `/` (linear typing, rule 7) ends its
/// denominator on Space, an operator, a comma, `)` or the closing `$`, with the
/// usual precedence. A `\frac{…}{…}` typed in full keeps what is typed inside.
#[test]
fn a_slash_fraction_ends_like_written_maths() {
    for (typed, file) in [
        (r"$$ \theta := \pi/6 $$", r"$$ θ := \frac{\pi}{6} $$"),
        (r"$\theta := \pi/6$", r"$$ θ := \frac{\pi}{6} $$"),
        (r"$\theta := \pi/6", r"$$ θ := \frac{\pi}{6} $$"),
        ("$q := 1/2 + 3", r"$$ q := \frac{1}{2} + 3 $$"),
        ("$a = 1/2-3", r"$$ a = \frac{1}{2}-3 $$"),
        (
            "$sum(1/n^2, n, 1, 10) =",
            r"$$ \sum_{n=1}^{10} \frac{1}{n^{2}} = $$",
        ),
        ("$1/(2+3) = ", r"$$ \frac{1}{(2+3)} =  $$"),
        (r"$\frac{1}{2+3}", r"$$ \frac{1}{2+3} $$"),
    ] {
        let mut h = fresh();
        type_text(&mut h, typed);
        assert_eq!(text_of(&h), file, "typed {typed:?}");
    }
}

/// T8 and S17: the fraction `/` makes is its own undo step: the first Ctrl+Z
/// gives back exactly what was typed.
#[test]
fn the_first_undo_gives_back_the_slash() {
    for (typed, back) in [
        (r"$\pi/6", r"$$ \pi/6 $$"),
        ("$x/2", "$$ x/2 $$"),
        ("$x/25", "$$ x/25 $$"),
    ] {
        let mut h = fresh();
        type_text(&mut h, typed);
        press(&mut h, Modifiers::COMMAND, Key::Z);
        assert_eq!(text_of(&h), back, "typed {typed:?}");
        assert_eq!(
            Seen::of(&h).caret,
            back.len() - 3,
            "the caret is after what was typed"
        );
    }
}

/// Clicks where the caret stop for `offset` is drawn.
fn click_offset(h: &mut H, offset: usize) {
    let st = h.state();
    let doc = st.doc().expect("doc");
    let stop = doc
        .page_cache()
        .expect("page")
        .page
        .caret_stop_on(offset, mpp_ide::page::Side::Before)
        .expect("a stop");
    let at = st.diag.origin + egui::vec2(stop.x, (stop.top + stop.bottom) / 2.0);
    super::runner::click_at(h, at);
}

/// E1: a click after `20` in `v = 20`, then Backspace twice, takes one
/// character each time; `v = ` stays.
#[test]
fn backspace_in_maths_takes_one_character() {
    let src = "$$ v = 20 $$\n\n$$ w = v $$\n\n";
    let mut h = super::runner::harness(super::runner::app_with(src));
    h.run_steps(3);
    click_offset(&mut h, 9);
    assert_eq!(Seen::of(&h).caret, 9, "the click put the caret after 20");
    press(&mut h, Modifiers::NONE, Key::Backspace);
    assert_eq!(text_of(&h), "$$ v = 2 $$\n\n$$ w = v $$\n\n");
    press(&mut h, Modifiers::NONE, Key::Backspace);
    assert_eq!(text_of(&h), "$$ v =  $$\n\n$$ w = v $$\n\n");
    type_text(&mut h, "25");
    assert_eq!(text_of(&h), "$$ v = 25 $$\n\n$$ w = v $$\n\n");
}

/// E16: Backspace after a fraction takes the last character of its
/// denominator, not the whole `/6`.
#[test]
fn backspace_after_a_fraction_takes_one_character() {
    let src = concat!(r"$$ θ = \frac{\pi}{6} $$", "\n\n");
    let mut h = super::runner::harness(super::runner::app_with(src));
    h.run_steps(3);
    let end = src.find(" $$\n").expect("end");
    click_offset(&mut h, end);
    press(&mut h, Modifiers::NONE, Key::Backspace);
    assert_eq!(text_of(&h), concat!(r"$$ θ = \frac{\pi}{} $$", "\n\n"));
    type_text(&mut h, "4");
    assert_eq!(text_of(&h), concat!(r"$$ θ = \frac{\pi}{4} $$", "\n\n"));
}

/// Where the caret stop for `offset` is drawn on screen.
fn stop_x(h: &H, offset: usize) -> f32 {
    let st = h.state();
    let doc = st.doc().expect("doc");
    let stop = doc
        .page_cache()
        .expect("page")
        .page
        .caret_stop_on(offset, mpp_ide::page::Side::Before)
        .expect("a stop");
    st.diag.origin.x + stop.x
}

/// E2: after Backspace takes the value of `v = 20`, the caret is drawn after
/// the `=`, where the next character goes, not left of it.
#[test]
fn the_caret_is_drawn_where_the_next_character_goes_in_a_definition() {
    let src = "$$ v = 20 $$\n\n$$ w = v $$\n\n";
    let mut h = super::runner::harness(super::runner::app_with(src));
    h.run_steps(3);
    click_offset(&mut h, 9);
    press(&mut h, Modifiers::NONE, Key::Backspace);
    press(&mut h, Modifiers::NONE, Key::Backspace);
    h.run_steps(2);
    assert_eq!(text_of(&h), "$$ v =  $$\n\n$$ w = v $$\n\n");
    let drawn = Seen::of(&h).caret_at.expect("caret").x;
    let after_v = stop_x(&h, 4);
    assert!(
        drawn > after_v + 8.0,
        "the caret was drawn at x={drawn}, beside the v (x={after_v}), not after the ="
    );
    type_text(&mut h, "3");
    h.run_steps(2);
    let three = stop_x(&h, 7);
    assert!(
        (drawn - three).abs() < 6.0,
        "the caret was drawn at x={drawn} but the 3 went at x={three}"
    );
}

/// S2: Backspace only deletes just before the caret. From the empty line under
/// a maths box it joins the lines, then goes into the box; the prose above is
/// never touched.
#[test]
fn backspace_under_a_box_never_reaches_earlier_lines() {
    let src = "A ball at speed v and angle theta.\n\n$$ x = 12 $$\n\n";
    let mut h = super::runner::harness(super::runner::app_with(src));
    h.run_steps(3);
    for _ in 0..3 {
        press(&mut h, Modifiers::NONE, Key::Backspace);
        assert!(
            text_of(&h).starts_with("A ball at speed v and angle theta.\n\n$$ x = 1"),
            "Backspace reached text before the caret: {:?}",
            text_of(&h)
        );
    }
    assert_eq!(
        text_of(&h),
        "A ball at speed v and angle theta.\n\n$$ x = 1 $$"
    );
    caret_on_its_line(&h);
}

/// S3: a click on the empty page under the last maths box puts the caret on a
/// new line under it, not in the box's fraction.
#[test]
fn a_click_under_the_last_box_starts_a_new_line() {
    let src = r"$$ θ := \frac{\pi}{6} $$";
    let mut h = super::runner::harness(super::runner::app_with(src));
    h.run_steps(3);
    let s = Seen::of(&h);
    let (_, top) = s.tops[0];
    let x = s.caret_at.expect("caret").x - 20.0;
    super::runner::click_at(&mut h, egui::pos2(x, top + 160.0));
    assert!(!in_math(&h), "the click went into the maths");
    keys_land(&mut h, "Next");
    assert_eq!(text_of(&h), format!("{src}\n\nNext"));
    caret_on_its_line(&h);
}
