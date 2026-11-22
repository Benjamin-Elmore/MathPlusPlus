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

fn hint(h: &H) -> Option<String> {
    h.state()
        .doc()
        .and_then(|d| d.editor.hint().map(str::to_string))
}

fn in_math(h: &H) -> bool {
    let doc = h.state().doc().expect("doc");
    doc.page_cache()
        .expect("page")
        .page
        .in_math(doc.editor.caret())
}

/// T7, E12, S8 and P3: a sentence that ends in maths and `=` keeps its words.
/// A one-time hint offers Ctrl+M, which makes just the maths at its end inline
/// maths; the sentence goes on after it.
#[test]
fn ctrl_m_makes_the_maths_at_the_end_of_a_sentence_inline() {
    let mut h = fresh();
    type_text(&mut h, "Half of v is v/2 =");
    assert_eq!(hint(&h).as_deref(), Some("Make \"v/2 =\" maths? Ctrl+M"));
    assert_eq!(
        text_of(&h),
        "Half of v is v/2 =",
        "nothing converts by itself"
    );
    press(&mut h, Modifiers::COMMAND, Key::M);
    assert_eq!(text_of(&h), "Half of v is $v/2 =$");
    assert!(!in_math(&h), "the caret is back in the sentence");
    type_text(&mut h, " m/s.");
    assert_eq!(text_of(&h), "Half of v is $v/2 =$ m/s.");
}

/// Ctrl+M or Alt+= on a selection in a sentence wraps just the selection.
#[test]
fn a_selection_in_a_sentence_becomes_inline_maths() {
    for mods in [Modifiers::COMMAND, Modifiers::ALT] {
        let mut h = fresh();
        type_text(&mut h, "The value x+1 is big");
        for _ in 0.." is big".len() {
            key(&mut h, Key::ArrowLeft);
        }
        for _ in 0.."x+1".len() {
            press(&mut h, Modifiers::SHIFT, Key::ArrowLeft);
        }
        let k = if mods == Modifiers::ALT {
            Key::Equals
        } else {
            Key::M
        };
        press(&mut h, mods, k);
        assert_eq!(text_of(&h), "The value $x+1$ is big", "with {mods:?}");
    }
}

/// S8: Alt+= with no selection in a sentence starts an empty inline maths at
/// the caret (not the whole sentence); Esc leaves it into the sentence.
#[test]
fn alt_equals_in_a_sentence_starts_inline_maths_and_esc_leaves_it() {
    let mut h = fresh();
    type_text(&mut h, "Half of v is ");
    press(&mut h, Modifiers::ALT, Key::Equals);
    assert!(in_math(&h), "the caret is in the new inline maths");
    type_text(&mut h, "v");
    assert_eq!(text_of(&h), "Half of v is $v$");
    type_text(&mut h, "/2");
    // Esc leaves the denominator's box, then the inline maths (rule 4).
    for _ in 0..2 {
        if in_math(&h) {
            key(&mut h, Key::Escape);
        }
    }
    assert!(!in_math(&h), "Esc left the inline maths");
    type_text(&mut h, " m/s.");
    let text = text_of(&h);
    assert!(
        text.starts_with("Half of v is $") && text.ends_with("$ m/s."),
        "{text:?}"
    );
    assert_eq!(text.matches('$').count(), 2, "{text:?}");
}

/// T5: `\[ … \]` typed by a LaTeX user is display maths: the editor writes it
/// as a maths line on the closing `\]`, or on Enter after `\[`.
#[test]
fn bracket_display_maths_becomes_a_maths_line() {
    let mut h = fresh();
    type_text(&mut h, r"\[ x = 1 + 2 \]");
    assert_eq!(text_of(&h), "$$ x = 1 + 2 $$");
    assert_eq!(Seen::of(&h).caret, text_of(&h).len());
    assert_eq!(Seen::of(&h).maths, 1, "a maths line on the page");
    // `\[` alone, then Enter: an empty maths line with the caret in it.
    let mut h = fresh();
    type_text(&mut h, r"\[");
    enter(&mut h);
    assert_eq!(text_of(&h), "$$  $$");
    assert!(in_math(&h));
    type_text(&mut h, "y = 3");
    assert_eq!(text_of(&h), "$$ y = 3 $$");
    // `\[` and maths, then Enter: the maths line, and a new line under it.
    let mut h = fresh();
    type_text(&mut h, r"\[ v = 20");
    enter(&mut h);
    assert_eq!(text_of(&h), "$$ v = 20 $$\n\n");
    assert!(!in_math(&h));
    // In the middle of a sentence: the maths gets a line of its own.
    let mut h = fresh();
    type_text(&mut h, r"So \[ a + b \] and more");
    assert_eq!(text_of(&h), "So\n\n$$ a + b $$\n\nand more");
}

/// P13 and S16: a line typed in code style that becomes maths by itself is
/// converted as typing in maths would (a built-up fraction, a power, θ), in
/// the same undo step: the first Ctrl+Z gives back the line as typed.
#[test]
fn a_code_style_line_is_built_up_when_it_becomes_maths() {
    for (typed, want) in [
        ("v/2 =", r"$$ \frac{v}{2} = $$"),
        ("1/n**2 =", r"$$ \frac{1}{n^{2}} = $$"),
    ] {
        let mut h = fresh();
        type_text(&mut h, typed);
        enter(&mut h);
        assert_eq!(text_of(&h), format!("{want}\n\n"), "typed {typed:?}");
        press(&mut h, Modifiers::COMMAND, Key::Z);
        assert_eq!(text_of(&h), format!("{typed}\n\n"), "undo after {typed:?}");
    }
    let mut h = fresh();
    type_text(&mut h, "R = v**2*sin(2*theta)/g");
    enter(&mut h);
    let text = text_of(&h);
    assert!(
        text.starts_with(r"$$ R = v^{2}·\frac{") && text.contains("{g}") && text.contains('θ'),
        "{text:?}"
    );
    assert!(!text.contains("**"), "{text:?}");
}

/// E18, S12 and P12: F1 opens the cheat sheet with a plain-words "Start here"
/// part first; Esc in it closes the sheet, and typing goes to the page again.
#[test]
fn esc_closes_the_cheat_sheet_and_typing_goes_on() {
    use egui_kittest::kittest::Queryable;
    let mut h = super::runner::harness(super::runner::app_with("Some words."));
    h.run_steps(3);
    press(&mut h, Modifiers::NONE, Key::F1);
    h.run_steps(3);
    assert!(h.state().ws.side_visible, "F1 shows the cheat sheet");
    assert!(
        h.query_all_by_label_contains("type the top, then /, then the bottom")
            .next()
            .is_some(),
        "the sheet starts with plain words"
    );
    press(&mut h, Modifiers::NONE, Key::Escape);
    h.run_steps(3);
    assert!(!h.state().ws.side_visible, "Esc closed the cheat sheet");
    type_text(&mut h, " More");
    assert_eq!(text_of(&h), "Some words. More");
}

/// S14: Heading and List are in the Insert menu (Ctrl+Space).
#[test]
fn heading_and_list_are_in_the_insert_menu() {
    use egui::Event;
    use mpp_ide::insert::menu;
    for (query, id, typed, want) in [
        ("heading", "text.heading", "Results", "# Results"),
        ("list", "text.list", "first", "- first"),
    ] {
        let mut h = fresh();
        press(&mut h, Modifiers::COMMAND, Key::Space);
        h.run_steps(2);
        for c in query.chars() {
            h.event(Event::Text(c.to_string()));
            h.step();
        }
        let recent = h.state().insert.settings.recent.clone();
        let items = menu::flat(&menu::sections(query, &recent));
        let at = items
            .iter()
            .position(|i| i.id == id)
            .unwrap_or_else(|| panic!("{id} is not in the Insert menu"));
        for _ in 0..at {
            key(&mut h, Key::ArrowDown);
        }
        enter(&mut h);
        h.run_steps(2);
        type_text(&mut h, typed);
        assert_eq!(text_of(&h).trim_end(), want, "{query}");
    }
}

/// S13: the note beside a sum with an empty box says what to type in plain
/// words.
#[test]
fn the_empty_box_hint_is_in_plain_words() {
    let src = r"$$ \sum_{n=1}^{10} {} $$";
    let h = super::runner::harness(super::runner::app_with(src));
    let st = h.state();
    let doc = st.doc().expect("doc");
    let page = &doc.page_cache().expect("page").page;
    let hint = mpp_ide::insert::fill_in_hint(&doc.editor, page, &(0..src.len()));
    if let Some(hint) = hint {
        assert!(!hint.contains("fill in"), "{hint}");
        assert!(hint.starts_with("type "), "{hint}");
    }
    assert_eq!(mpp_ide::insert::plain_label("term"), "what is summed");
    assert_eq!(mpp_ide::insert::plain_label(""), "in the empty boxes");
    assert_eq!(mpp_ide::insert::plain_label("numerator"), "the top");
}

/// T15: the first `:=` typed says what it means.
#[test]
fn the_first_colon_equals_says_what_it_means() {
    let mut h = fresh();
    type_text(&mut h, "=");
    type_text(&mut h, "x :=");
    assert_eq!(hint(&h).as_deref(), Some(mpp_ide::hints::DEFINES));
}

/// P10: while a prose line reads as maths, a faint cue says it will become
/// maths; a sentence gets none.
#[test]
fn a_line_that_will_become_maths_shows_a_cue() {
    let mut h = fresh();
    type_text(&mut h, "g = 9.81");
    assert!(mpp_ide::page::mode::maths_cue(
        h.state().doc().expect("doc")
    ));
    let mut h = fresh();
    type_text(&mut h, "A ball is thrown.");
    assert!(!mpp_ide::page::mode::maths_cue(
        h.state().doc().expect("doc")
    ));
}

/// T12: after `\frac{}{}` typed in full, the status bar says where the next
/// character goes.
#[test]
fn the_status_path_says_where_the_next_character_goes() {
    // The empty-braces habit: `\frac{}{}`, then a digit.
    let mut h = fresh();
    type_text(&mut h, r"$\frac{}{}");
    h.run_steps(2);
    let at = h.state().doc().expect("doc").editor.caret();
    let text = text_of(&h);
    let status = mpp_ide::page::caret_path::status_text(h.state().doc().expect("doc"));
    let inside = text[at..].trim_start().starts_with('}');
    assert_eq!(
        status.contains("denominator"),
        inside,
        "the status says {status:?} with the caret at {at} in {text:?}"
    );
    // The next character goes where the status said.
    type_text(&mut h, "1");
    let after = text_of(&h);
    assert_eq!(
        after.contains("{1}"),
        inside,
        "{status:?}, then 1 went into {after:?}"
    );
}
