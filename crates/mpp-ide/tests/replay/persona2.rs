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

/// The index of the block holding the caret.
fn caret_block(h: &H) -> usize {
    let s = Seen::of(h);
    s.tops
        .iter()
        .position(|(sp, _)| sp.start <= s.caret && s.caret <= sp.end)
        .expect("the caret is in a block")
}

/// E15 and R15: Up and Down go line by line, never box by box inside a sum or
/// a fraction; Home and End go to the ends of the line.
#[test]
fn up_and_down_move_by_lines_and_home_end_to_the_line_ends() {
    let src = concat!(
        "Intro line.\n\n",
        r"$$ R := \sum_{n=1}^{10} \frac{1}{n^{2}} $$",
        "\n\n",
        r"$$ v := \frac{20}{3} + 1 $$",
        "\n\nlast words"
    );
    let mut h = super::runner::harness(super::runner::app_with(src));
    h.run_steps(3);
    assert_eq!(caret_block(&h), 3);
    for want in [2, 1, 0] {
        key(&mut h, Key::ArrowUp);
        assert_eq!(caret_block(&h), want, "one Up is one line");
    }
    for want in [1, 2, 3] {
        key(&mut h, Key::ArrowDown);
        assert_eq!(caret_block(&h), want, "one Down is one line");
    }
    // On the sum's line: Home to its start, End to its end.
    key(&mut h, Key::ArrowUp);
    key(&mut h, Key::ArrowUp);
    assert_eq!(caret_block(&h), 1);
    key(&mut h, Key::Home);
    let r = src.find("R :=").expect("R");
    assert_eq!(Seen::of(&h).caret, r, "Home goes to the start of the line");
    key(&mut h, Key::End);
    let end = src.find("}} $$").expect("end") + 2;
    assert_eq!(Seen::of(&h).caret, end, "End goes to the end of the line");
}

/// E15: PageUp and PageDown move by a screenful.
#[test]
fn page_up_and_down_move_by_a_screenful() {
    let src: String = (1..=80).map(|i| format!("Line number {i}.\n\n")).collect();
    let src = src.trim_end().to_string();
    let mut h = super::runner::harness(super::runner::app_with(&src));
    h.run_steps(3);
    let line = |h: &H| text_of(h)[..Seen::of(h).caret].matches("\n\n").count();
    assert_eq!(line(&h), 79);
    press(&mut h, Modifiers::NONE, Key::PageUp);
    h.run_steps(2);
    let up = line(&h);
    assert!((50..=75).contains(&up), "PageUp moved to line {up}");
    press(&mut h, Modifiers::NONE, Key::PageDown);
    h.run_steps(2);
    assert!(line(&h) > up + 5, "PageDown moved back down");
}
