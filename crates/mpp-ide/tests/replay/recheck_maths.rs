//! Fix loop 2 of the persona panel (U28), maths lane: what the re-check found
//! in answers, definitions and checks, typed through the real frame loop.

use egui::Key;

use super::runner::{app_with, fresh, harness, run, text_of, Seen, Step, Watch, H};
use mpp_ide::page::LinkTarget;

use Step::{Pause, Type};

const ENTER: Step = Step::Key(Key::Enter);

/// A file holding `text`, drawn, with the person's keys after it.
fn typed_after(text: &str, steps: &[Step]) -> H {
    let mut h = harness(app_with(text));
    h.run_steps(2);
    let mut w = Watch::new();
    run(&mut h, steps, |step, kind, s| w.frame(step, kind, s));
    h
}

/// Keys typed into a new, empty file.
fn typed(steps: &[Step]) -> H {
    let mut h = fresh();
    let mut w = Watch::new();
    run(&mut h, steps, |step, kind, s| w.frame(step, kind, s));
    h
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

fn last_line(h: &H) -> String {
    Seen::of(h).visible.last().cloned().unwrap_or_default()
}

const GV: &str = "$$ g = 9.81 $$\n\n$$ v = 20 $$\n\n";

/// E-N2: a `/` typed straight after an exponent ends the exponent and makes a
/// fraction of the term before it: v²/g, never v^(2/g).
#[test]
fn a_slash_after_an_exponent_divides_the_power() {
    for line in ["v^2/g =", "v**2/g =", "v^2 /g =", "v^2/ g ="] {
        let h = typed_after(GV, &[Type(line), Pause]);
        let last = last_line(&h);
        assert!(last.contains("40.77"), "{line}: {last} ({:?})", text_of(&h));
    }
    // A power in the denominator still works.
    for line in ["1/n^2 =", "1/n**2 ="] {
        let h = typed_after("$$ n = 2 $$\n\n", &[Type(line), Pause]);
        let last = last_line(&h);
        assert!(last.contains("0.25"), "{line}: {last} ({:?})", text_of(&h));
    }
}

/// E-N4: a number with units after it is still a definition.
#[test]
fn a_number_with_compound_units_defines() {
    let h = typed(&[
        Type("g = 9.81 m/s^2"),
        ENTER,
        Type("v = 20 m/s"),
        ENTER,
        Type("v*2 ="),
        Pause,
    ]);
    let last = last_line(&h);
    assert!(last.contains("40"), "{last} ({:?})", text_of(&h));
    assert!(last.contains("m/s") || last.contains("m s"), "{last}");
    let h = typed(&[
        Type("g = 9.81 m/s^2"),
        ENTER,
        Type("v = 20 m/s"),
        ENTER,
        Type("v^2/g ="),
        Pause,
    ]);
    let last = last_line(&h);
    assert!(last.contains("40.77"), "{last} ({:?})", text_of(&h));
}

/// S9: a name already defined above, written with `=` and a number, is a
/// check (U17). `R = … =` with R new defines R and shows its value, so the
/// `R = 35.3` under it (here in the same maths box) checks it.
#[test]
fn a_defined_name_equal_to_a_number_is_a_check() {
    let h = typed(&[
        Type("$$ g = 9.81"),
        ENTER,
        Type("v = 20"),
        ENTER,
        Type("theta = pi/6"),
        ENTER,
        Type("R = v^2 sin(2theta)/g ="),
        ENTER,
        Type("R = 35.3"),
        Pause,
    ]);
    let tips = slots(&h);
    let (_, tip) = tips.last().expect("a slot");
    assert!(tip.contains('✓'), "{tips:?} ({:?})", text_of(&h));
    let s = Seen::of(&h);
    assert!(s.visible.concat().contains("35.31"), "{:?}", s.visible);
    // The value of R is still shown on its own line.
    assert!(
        tips.iter()
            .any(|(copy, tip)| copy.contains("35.31") && !tip.contains("check")),
        "{tips:?}"
    );
}
