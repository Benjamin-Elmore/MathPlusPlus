//! Fix loop 3 of the persona panel (U28): checks with units, the line after a
//! directive, and unit conversion, typed through the real frame loop.

use egui::Key;

use super::runner::{fresh, run, text_of, Seen, Step, Watch, H};
use mpp_ide::page::LinkTarget;

use Step::{Pause, Type};

const ENTER: Step = Step::Key(Key::Enter);

fn typed(steps: &[Step]) -> H {
    let mut h = fresh();
    let mut w = Watch::new();
    run(&mut h, steps, |step, kind, s| w.frame(step, kind, s));
    h
}

/// E-F3: a line typed straight after a `%digits 4` line becomes maths, even
/// when the same text (`R =`) was asked once already further up.
#[test]
fn the_line_after_a_directive_becomes_maths() {
    let h = typed(&[
        Type("g = 9.81"),
        ENTER,
        Type("v = 20"),
        ENTER,
        Type("R = v^2*sin(2*pi/6)/g"),
        ENTER,
        Type("R ="),
        Pause,
        ENTER,
        Type("%digits 4"),
        ENTER,
        Type("R ="),
        Pause,
        ENTER,
        Type("v/3 ="),
        Pause,
    ]);
    let text = text_of(&h);
    assert_eq!(text.matches("$$ R = $$").count(), 2, "{text:?}");
    let s = Seen::of(&h);
    let after: Vec<&String> = s
        .visible
        .iter()
        .skip_while(|v| !v.contains("digits"))
        .collect();
    assert!(after.len() >= 3, "{:?}", s.visible);
    assert!(after[1].contains("35.31"), "{after:?}");
    assert!(after[2].contains("6.667"), "{after:?}");
}

/// E11: `v in km/h =` (and `v to km/h =`), as the cheat sheet shows it, becomes
/// maths and answers in the other unit; so does a check with a prefixed unit.
#[test]
fn a_value_in_other_units() {
    for line in ["v in km/h =", "v to km/h ="] {
        let h = typed(&[Type("v = 20 m/s"), ENTER, Type(line), Pause]);
        let s = Seen::of(&h);
        let last = s.visible.last().cloned().unwrap_or_default();
        assert!(
            last.contains("72"),
            "{line}: {:?} ({:?})",
            s.visible,
            text_of(&h)
        );
        assert!(last.contains("km/h"), "{line}: {last}");
    }
    let h = typed(&[
        Type("v = 20 m/s"),
        ENTER,
        Type("v = 7200 cm/s"),
        ENTER,
        Type("x"),
        Pause,
    ]);
    assert!(
        text_of(&h).contains("$$ v = 7200 cm/s $$"),
        "{:?}",
        text_of(&h)
    );
}

/// Every answer slot on the page: what a click copies and what hovering shows.
fn slots(h: &H) -> Vec<(String, String)> {
    let doc = h.state().doc().expect("doc");
    let pc = doc.page_cache().expect("page");
    pc.page
        .links
        .iter()
        .filter_map(|l| match &l.link.target {
            LinkTarget::Answer(t) => Some((t.clone(), l.link.tip.clone().unwrap_or_default())),
            _ => None,
        })
        .collect()
}

/// The engineer's worksheet with units, then `line`, typed as they typed it.
fn range_then(line: &'static str) -> H {
    typed(&[
        Type("g = 9.81 m/s^2"),
        ENTER,
        Type("v = 20 m/s"),
        ENTER,
        Type("R = v^2*sin(2*pi/6)/g"),
        ENTER,
        Type(line),
        ENTER,
        Pause,
    ])
}

/// E-F1: checks with units get the rules of a written number, in the unit
/// written; a missing unit is a ✗ that says why. E-F2: no "is read as" warning
/// on them, so hovering the line shows the check's own reason.
#[test]
fn checks_with_units_round_and_say_why() {
    for (line, mark) in [
        ("R = 35.3 m", '✓'),
        ("R = 3530 cm", '✓'),
        ("R approx 35 m", '✓'),
        ("R = 35.4 m", '✗'),
        ("R = 35.3", '✗'),
    ] {
        let mut h = range_then(line);
        let tips = slots(&h);
        let (_, tip) = tips.last().expect("a slot");
        assert!(tip.contains(mark), "{line}: {tips:?} ({:?})", text_of(&h));
        let problems = h.state_mut().doc_mut().expect("doc").problems();
        assert!(
            !problems.iter().any(|p| p.message.contains("is read as")),
            "{line}: {:?}",
            problems.iter().map(|p| &p.message).collect::<Vec<_>>()
        );
        if mark == '✗' {
            assert!(
                problems.iter().any(|p| p.message.contains("does not hold")),
                "{line}: {problems:?}"
            );
        }
    }
    let mut h = range_then("R = 35.3");
    let tips = slots(&h);
    let (_, tip) = tips.last().expect("a slot");
    assert!(
        tip.contains("✗ because") && tip.contains("is in metres; } 35.3 \\text{ has no unit"),
        "{tip}"
    );
    let problems = h.state_mut().doc_mut().expect("doc").problems();
    assert!(
        problems
            .iter()
            .any(|p| p.message.contains("R is in metres; 35.3 has no unit")),
        "{problems:?}"
    );
}

/// E-F4: a definition with units shows its value, as a unitless one does.
#[test]
fn a_definition_with_units_shows_its_value() {
    let h = range_then("x = 1");
    let s = Seen::of(&h);
    let def = s
        .visible
        .iter()
        .find(|v| v.contains("sin"))
        .cloned()
        .unwrap_or_default();
    let plain = typed(&[
        Type("g = 9.81"),
        ENTER,
        Type("v = 20"),
        ENTER,
        Type("R = v^2*sin(2*pi/6)/g"),
        ENTER,
        Type("x = 1"),
        ENTER,
        Pause,
    ]);
    let plain_def = Seen::of(&plain).visible[2].clone();
    assert!(plain_def.contains("35.31194307"), "{plain_def}");
    assert!(def.contains("35.31"), "{:?}", s.visible);
    assert!(def.contains("35.31194307m"), "{def}");
}

/// E8: `%digits 4` gives its digits to the decimal only; a definition's exact
/// form is never padded (`20000√3/981`, not `20000√3.000/981`).
#[test]
fn digits_apply_to_the_decimal_only() {
    let h = typed(&[
        Type("%digits 4"),
        ENTER,
        Type("g = 9.81"),
        ENTER,
        Type("v = 20"),
        ENTER,
        Type("R = v^2*sin(2*pi/6)/g"),
        ENTER,
        Type("R ="),
        ENTER,
        Pause,
    ]);
    let s = Seen::of(&h);
    let def = s
        .visible
        .iter()
        .find(|v| v.contains("sin"))
        .cloned()
        .unwrap_or_default();
    assert!(def.contains("20000√3981"), "{:?}", s.visible);
    assert!(!def.contains("3.000"), "{def}");
    assert!(def.contains("≈35.31") && !def.contains("35.311"), "{def}");
}
