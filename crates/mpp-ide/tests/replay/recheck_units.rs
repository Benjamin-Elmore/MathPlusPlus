//! Fix loop 3 of the persona panel (U28): checks with units, the line after a
//! directive, and unit conversion, typed through the real frame loop.

use egui::Key;

use super::runner::{fresh, run, text_of, Seen, Step, Watch, H};

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
