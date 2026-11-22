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

/// A file with words and no definitions.
const NOTES: &str = "Notes.\n\n";

/// R8: an answer never holds a name defined nowhere without saying so: in a
/// file with no definitions when it only repeats the question, and in a file
/// that defines numbers whenever a name in it has no value. Pure algebra in a
/// file that defines nothing stays quiet (x + x = 2x).
#[test]
fn an_answer_with_an_undefined_name_says_so() {
    let h = typed_after(NOTES, &[Type("x + x = ?"), Pause]);
    let (copy, _) = slots(&h).pop().expect("a slot");
    assert_eq!(copy, "2*x", "algebra keeps its answer");
    for (text, line) in [
        (NOTES, "w + 1 = ?"),
        ("$$ v = 20 $$\n\n", "w + v = ?"),
        ("$$ v = 20 $$\n\n", "w + v ="),
    ] {
        let h = typed_after(text, &[Type(line), Pause]);
        let (copy, _) = slots(&h).pop().expect("a slot");
        assert!(copy.contains("w is not defined"), "{line}: {copy}");
    }
    let h = typed_after("$$ v = 20 $$\n\n", &[Type("diff(x^2, x) ="), Pause]);
    let (copy, _) = slots(&h).pop().expect("a slot");
    assert_eq!(copy, "2*x", "calculus keeps its answer");
}

/// E8: `%digits 4` applies from its line down, to every answer after it, and
/// an exact answer stays exact: only its decimal takes the digits.
#[test]
fn digits_apply_from_the_directive_down_and_keep_exact_forms() {
    let h = typed_after(
        GV,
        &[
            Type("%digits 4"),
            ENTER,
            Type("v/3 ="),
            ENTER,
            Type("v/7 ="),
            ENTER,
            Type("g/7 ="),
            ENTER,
            Type("sqrt(2)*v ="),
            Pause,
        ],
    );
    let s = Seen::of(&h);
    let all = s.visible.join(" | ");
    for want in ["6.667", "2.857", "1.401", "28.28"] {
        assert!(all.contains(want), "no {want}: {all} ({:?})", text_of(&h));
    }
    for unwanted in ["6.6666", "2.8571", "1.4014", "28.284", "√2.000", "20.00√"] {
        assert!(!all.contains(unwanted), "{unwanted}: {all}");
    }
    // The exact form is still there.
    assert!(all.contains("20√2") || all.contains("20 √2"), "{all}");
    // The directive line shows as written, a setting, not a sentence.
    assert!(s.visible.iter().any(|v| v.starts_with("%digits")), "{all}");
}

/// The engineer's definitions, as maths lines.
const DEFS: &str = concat!(
    "$$ g = 9.81 $$\n\n",
    "$$ v = 20 $$\n\n",
    r"$$ \theta = \pi/6 $$",
    "\n\n",
    r"$$ R = v^2 \sin(2\theta)/g $$",
    "\n\n"
);

/// S-N6: "R about 35.3" is an about-check, as "R approx 35.3" is.
#[test]
fn about_is_an_about_check() {
    let h = typed_after(DEFS, &[Type("R about 35.3"), ENTER, Pause]);
    let ticks = slots(&h).iter().filter(|(_, t)| t.contains('✓')).count();
    assert_eq!(ticks, 1, "{:?} ({:?})", slots(&h), text_of(&h));
}

/// T-N5: the hover of an about-check says how close counts.
#[test]
fn an_about_check_says_its_tolerance() {
    let h = typed_after(DEFS, &[Type(r"R \approx 35"), ENTER, Pause]);
    let tips = slots(&h);
    let (_, tip) = tips.iter().find(|(_, t)| t.contains('✓')).expect("a tick");
    assert!(tip.contains("±0.5"), "how close counts: {tip}");
    assert!(tip.contains("35.3"), "the value it was compared at: {tip}");
}

/// E5: an answer with units that is not a whole number shows its decimal.
#[test]
fn a_unit_answer_shows_its_decimal() {
    let h = typed_after("$$ q = 3 m/s $$\n\n", &[Type("q/2 ="), Pause]);
    let last = last_line(&h);
    assert!(last.contains("1.5"), "{last} ({:?})", text_of(&h));
}

/// S11, E17: hovering the `=` of a definition explains it, as hovering its
/// value does; a click there still goes to the page.
#[test]
fn the_equals_of_a_definition_has_a_hover_text() {
    let h = typed_after(DEFS, &[Pause]);
    let doc = h.state().doc().expect("doc");
    let pc = doc.page_cache().expect("page");
    let mut seen = 0;
    for b in pc
        .page
        .blocks
        .iter()
        .filter(|b| b.tag == mpp_ide::page::BlockTag::Math)
    {
        for p in &b.dl.items {
            let mpp_render::Primitive::Glyph { ch: '=', x, y, .. } = p else {
                continue;
            };
            let at = egui::Pos2::new(*x + 3.0, b.top + *y - 4.0);
            let link = pc
                .page
                .link_at(at)
                .unwrap_or_else(|| panic!("no hover text at {at:?}"));
            assert_eq!(link.link.target, LinkTarget::Tip);
            let tip = link.link.tip.clone().unwrap_or_default();
            assert!(tip.contains("defin"), "{tip}");
            seen += 1;
            break;
        }
    }
    assert_eq!(seen, 4, "every definition's = explains itself");
}

const UNKNOWN_COMMAND: &str = "Words.\n\n$$ \\sinn(2) + 1 = $$";

/// T-N4, E-N6: a typo of a function names the function in plain words, and an
/// unknown command is not called an empty box to type in.
#[test]
fn typos_of_functions_say_did_you_mean() {
    let mut h = typed_after(UNKNOWN_COMMAND, &[Pause]);
    let doc = h.state().doc().expect("doc");
    let pc = doc.page_cache().expect("page");
    let span = UNKNOWN_COMMAND.find("$$").expect("maths")..UNKNOWN_COMMAND.len();
    let hint = mpp_ide::insert::fill_in_hint(&doc.editor, &pc.page, &span);
    assert_eq!(hint, None, "an unknown command is not an empty box");
    let p: Vec<String> = h
        .state_mut()
        .doc_mut()
        .expect("doc")
        .problems()
        .into_iter()
        .map(|p| p.message)
        .collect();
    assert!(p.iter().any(|m| m.contains("Did you mean")), "{p:?}");
    let h = typed_after(DEFS, &[Type("roundd(R, 1) ="), Pause]);
    let (copy, _) = slots(&h).pop().expect("a slot");
    assert!(
        copy.contains("Did you mean") && copy.contains("round?"),
        "{copy}"
    );
}
