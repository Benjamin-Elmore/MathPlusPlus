//! Script 5: the escape hatch (U19, Design rule 4). Every maths template of the
//! Insert catalogue is put in through the real menu (Ctrl+Space, search, the
//! arrows, Enter), and left with each way out: Tab, Right, an operator or a
//! Space after a script, Esc and Enter. After leaving, what is typed goes
//! outside the template.

use egui::{Event, Key, Modifiers};
use mpp_ide::insert::catalogue::Body;
use mpp_ide::insert::{catalogue, menu, Template};

use super::runner::{app_with, harness, press, text_of, type_text, H};

/// The text a template goes into: a prose line, then an empty line to put it on.
const START: &str = "Work it out.\n\n";

/// The maths templates (the ones with boxes to get stuck in).
fn templates() -> Vec<&'static Template> {
    catalogue()
        .iter()
        .filter(|t| matches!(t.body, Body::Math(_)) && !t.cite_picker)
        .collect()
}

fn in_template(h: &H) -> bool {
    h.state().doc().expect("doc").editor.in_template()
}

/// True for a box written as an exponent or index of something (`x^{…}`,
/// `e^{…}`, `{}_{…}`), not as the limits of a big operator (`\int_{…}`,
/// `\sum^{…}`), where `+` and `=` belong inside (`n=1`, `a+1`).
fn is_script_box(text: &str, start: usize) -> bool {
    let before = &text[..start];
    let Some(mut base) = before
        .strip_suffix("^{")
        .or_else(|| before.strip_suffix("_{"))
    else {
        return false;
    };
    // Step back over the other scripts of the same base (`\int_{0}^{`).
    while base.ends_with('}') {
        let mut depth = 0;
        let mut open = None;
        for (i, c) in base.char_indices().rev() {
            match c {
                '}' => depth += 1,
                '{' => {
                    depth -= 1;
                    if depth == 0 {
                        open = Some(i);
                        break;
                    }
                }
                _ => {}
            }
        }
        let Some(open) = open else { break };
        match base[..open]
            .strip_suffix('^')
            .or_else(|| base[..open].strip_suffix('_'))
        {
            Some(b) => base = b,
            None => break,
        }
    }
    let word = base.trim_end_matches(|c: char| c.is_ascii_alphabetic());
    !(word.len() < base.len() && word.ends_with('\\'))
}

/// Puts `t` in at the caret the way a person does: Ctrl+Space, its name, the
/// arrow keys to it, Enter (and Enter again for a size picker).
fn insert(h: &mut H, t: &Template) {
    press(h, Modifiers::COMMAND, Key::Space);
    h.run_steps(2);
    assert!(h.state().insert.menu.open, "Ctrl+Space opens the menu");
    let query = t.name.to_lowercase();
    for c in query.chars() {
        h.event(Event::Text(c.to_string()));
        h.step();
    }
    let recent = h.state().insert.settings.recent.clone();
    let items = menu::flat(&menu::sections(&query, &recent));
    let at = items
        .iter()
        .position(|i| i.id == t.id)
        .unwrap_or_else(|| panic!("{} is not listed for {query:?}", t.id));
    for _ in 0..at {
        press(h, Modifiers::NONE, Key::ArrowDown);
    }
    press(h, Modifiers::NONE, Key::Enter);
    if h.state().insert.menu.picker.is_some() {
        press(h, Modifiers::NONE, Key::Enter);
    }
    h.run_steps(2);
    assert!(!h.state().insert.menu.open, "{}: the menu closed", t.id);
    assert!(in_template(h), "{}: the caret is in a box", t.id);
}

/// A fresh page with `t` just put in.
fn with(t: &Template) -> H {
    let mut h = harness(app_with(START));
    h.run_steps(3);
    insert(&mut h, t);
    h
}

/// Types `c` and checks it landed outside every box of the template.
fn typed_outside(h: &mut H, c: &str, id: &str, how: &str) {
    assert!(!in_template(h), "{id}: still in the template after {how}");
    let before = text_of(h);
    let caret = h.state().doc().expect("doc").editor.caret();
    type_text(h, c);
    let after = text_of(h);
    assert!(
        after.len() > before.len() && after[caret..].starts_with(c),
        "{id}: after {how}, {c:?} did not go where the caret was: {before:?} -> {after:?}"
    );
    let ed = &h.state().doc().expect("doc").editor;
    for s in ed.sessions() {
        for b in &s.boxes {
            let inside = after.get(b.range.clone()).unwrap_or("");
            assert!(
                !(b.range.start <= caret && caret < b.range.end) || !inside.contains(c),
                "{id}: after {how}, {c:?} went into the box {:?}",
                b.label
            );
        }
    }
}

/// Presses `key` until the caret is out of the template (at most `limit` times).
fn press_until_out(h: &mut H, key: Key, limit: usize) -> usize {
    for n in 0..limit {
        if !in_template(h) {
            return n;
        }
        press(h, Modifiers::NONE, key);
    }
    limit
}

#[test]
fn every_template_has_a_way_out_with_every_exit_key() {
    let all = templates();
    assert!(all.len() > 30, "only {} templates", all.len());
    let mut exits = 0;
    let mut stuck_right = vec![];
    for t in all {
        let id = t.id.as_str();
        // Tab: box to box, then out.
        let mut h = with(t);
        let boxes = {
            let ed = &h.state().doc().expect("doc").editor;
            ed.session_at(ed.caret()).expect("session").tab.len()
        };
        let n = press_until_out(&mut h, Key::Tab, boxes + 2);
        assert!(
            n <= boxes + 1,
            "{id}: Tab took {n} presses for {boxes} boxes"
        );
        typed_outside(&mut h, "+", id, "Tab");
        // Right: through the boxes, one level out at the end of each.
        let mut h = with(t);
        let limit = text_of(&h).len();
        press_until_out(&mut h, Key::ArrowRight, limit);
        if in_template(&h) {
            stuck_right.push(id.to_string());
        } else {
            typed_outside(&mut h, "+", id, "Right");
        }
        // Esc: one press leaves the template.
        let mut h = with(t);
        press(&mut h, Modifiers::NONE, Key::Escape);
        typed_outside(&mut h, "+", id, "Esc");
        // Enter: a new maths line after the structure.
        let mut h = with(t);
        let lines = text_of(&h).matches("\\\\").count();
        press(&mut h, Modifiers::NONE, Key::Enter);
        assert!(!in_template(&h), "{id}: still in the template after Enter");
        let after = text_of(&h);
        assert_eq!(
            after.matches("\\\\").count(),
            lines + 1,
            "{id}: Enter did not start a new maths line: {after:?}"
        );
        typed_outside(&mut h, "y", id, "Enter");
        // An operator or a Space after a script typed in the first box: `x^2`
        // then `+` puts the `+` after the exponent, still in the box.
        for key in ["+", " "] {
            let mut h = with(t);
            type_text(&mut h, &format!("x^2{key}y"));
            let after = text_of(&h);
            assert!(
                after.contains(&format!("x^{{2}}{key}y")),
                "{id}: {key:?} after x^2 did not leave the exponent: {after:?}"
            );
        }
        exits += 6;
    }
    assert!(exits > 180, "{exits} exits");
    // Known: see `right_leaves_the_helper_templates`.
    assert!(
        stuck_right.iter().all(|id| id.starts_with("helper.")),
        "Right never left: {stuck_right:?}"
    );
}

#[test]
fn right_leaves_the_helper_templates() {
    let mut stuck = vec![];
    for t in templates()
        .into_iter()
        .filter(|t| t.id.starts_with("helper."))
    {
        let mut h = with(t);
        let limit = text_of(&h).len();
        press_until_out(&mut h, Key::ArrowRight, limit);
        if in_template(&h) {
            stuck.push(t.id.clone());
        }
    }
    assert!(stuck.is_empty(), "Right never left: {stuck:?}");
}

/// A fresh page with `t` in it and the caret Tabbed (with `mods`) to the
/// script box at Tab position `pos`, if that gets there.
fn tab_to_script(t: &Template, pos: usize, mods: Modifiers) -> Option<H> {
    let mut h = with(t);
    for _ in 0..8 {
        let at = {
            let ed = &h.state().doc().expect("doc").editor;
            let s = ed.session_at(ed.caret())?;
            s.box_at(ed.caret())
                .filter(|&b| is_script_box(ed.text(), s.boxes[b].range.start))
                .and_then(|b| s.tab_pos(b))
        };
        if at == Some(pos) {
            return Some(h);
        }
        press(&mut h, mods, Key::Tab);
    }
    None
}

#[test]
#[ignore = "U27, kept by design in U28 (2c): the operator/Space exit only applies to a typed `^`/`_`; a template's own exponent or index box (the order of d^n/dx^n, a power's exponent) keeps `+` inside it, as in n+1"]
fn an_operator_or_a_space_leaves_every_script_box() {
    // Every box of a template that is an exponent or an index of something:
    // a digit typed in it, then the key; the key goes after the box.
    let mut seen = 0;
    for t in templates() {
        let probe = with(t);
        let scripts: Vec<usize> = {
            let ed = &probe.state().doc().expect("doc").editor;
            let s = ed.session_at(ed.caret()).expect("session");
            let text = ed.text();
            s.tab
                .iter()
                .enumerate()
                .filter(|(_, &i)| is_script_box(text, s.boxes[i].range.start))
                .map(|(pos, _)| pos)
                .collect()
        };
        for pos in scripts {
            for key in ["+", " ", "="] {
                // Tab (or Shift+Tab, for a box before the first one) to it.
                let mut h = [Modifiers::NONE, Modifiers::SHIFT]
                    .into_iter()
                    .find_map(|m| tab_to_script(t, pos, m))
                    .unwrap_or_else(|| panic!("{}: Tab never reached script box {pos}", t.id));
                type_text(&mut h, "2");
                let (id, range) = {
                    let ed = &h.state().doc().expect("doc").editor;
                    let s = ed.session_at(ed.caret()).expect("in the script");
                    let b = s.box_at(ed.caret()).expect("in a box");
                    (s.id, s.boxes[b].range.clone())
                };
                type_text(&mut h, key);
                let text = text_of(&h);
                let ed = &h.state().doc().expect("doc").editor;
                let now = ed
                    .session(id)
                    .map(|s| {
                        let b = s
                            .boxes
                            .iter()
                            .find(|b| b.range.start == range.start)
                            .expect("the box");
                        text[b.range.clone()].to_string()
                    })
                    .unwrap_or_else(|| text[range.clone()].to_string());
                assert_eq!(
                    now, "2",
                    "{}: {key:?} after a script went into it: {text:?}",
                    t.id
                );
                seen += 1;
            }
        }
    }
    assert!(seen > 0, "no template has a script box");
}

/// The label of the box holding the caret (the selection's start, when a
/// default is selected), if it is in one.
fn box_label(h: &H) -> Option<String> {
    let ed = &h.state().doc().expect("doc").editor;
    let at = ed.selection().map_or(ed.caret(), |r| r.start);
    let s = ed.session_at(at)?;
    s.box_at(at).map(|b| s.boxes[b].label.clone())
}

/// S6 (U28): Tab visits every box of the sum, its index and both limits too,
/// and then leaves it.
#[test]
fn tab_visits_the_bounds_of_a_sum() {
    let t = templates()
        .into_iter()
        .find(|t| t.name.eq_ignore_ascii_case("sum"))
        .expect("the sum template");
    let mut h = with(t);
    let mut seen = vec![box_label(&h).expect("in a box")];
    for _ in 0..6 {
        press(&mut h, Modifiers::NONE, Key::Tab);
        match box_label(&h) {
            Some(l) => seen.push(l),
            None => break,
        }
    }
    for want in ["index", "start", "end", "term"] {
        assert!(
            seen.iter().any(|l| l == want),
            "Tab never visited {want}: {seen:?}"
        );
    }
    assert!(!in_template(&h), "Tab left the sum: {seen:?}");
}
