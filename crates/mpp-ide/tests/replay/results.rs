//! U28 fix round 2a: answers, checks and the evaluator as the persona panel met
//! them (E3–E9, S7, S9, S11, P2–P9, R7, R8, T6), typed through the real frame
//! loop.

use egui::Key;

use super::runner::{app_with, harness, run, Seen, Step, Watch, H};
use mpp_ide::page::LinkTarget;

use Step::{Pause, Type};

const ENTER: Step = Step::Key(Key::Enter);

/// The engineer's definitions (persona E), as maths lines.
const DEFS: &str = concat!(
    "$$ g = 9.81 $$\n\n",
    "$$ v = 20 $$\n\n",
    r"$$ \theta = \pi/6 $$",
    "\n\n",
    r"$$ R = v^2 \sin(2\theta)/g $$",
    "\n\n"
);

/// A file holding `text`, drawn, with the person's keys after it.
fn typed_after(text: &str, steps: &[Step]) -> H {
    let mut h = harness(app_with(text));
    h.run_steps(2);
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

/// The slot of the last line on the page.
fn last_slot(h: &H) -> (String, String) {
    slots(h).pop().expect("the last line has a slot")
}

/// The Problems list, as text.
fn problems(h: &mut H) -> Vec<String> {
    h.state_mut()
        .doc_mut()
        .expect("doc")
        .problems()
        .into_iter()
        .map(|p| p.message)
        .collect()
}

/// E4, S9, P6, R7, T6: a failed check says ✗ and the real value, never a ×
/// between empty boxes, and it is listed in Problems.
#[test]
fn a_failed_check_shows_the_real_value_and_is_a_problem() {
    let mut h = typed_after(DEFS, &[Type("R = 35.2"), ENTER, Pause]);
    let s = Seen::of(&h);
    let last = s.visible.last().expect("a line");
    assert!(!last.contains('×'), "drawn as an operator: {last}");
    assert!(last.contains("35.31194307"), "the real value: {last}");
    let (copy, tip) = last_slot(&h);
    assert_eq!(copy, "35.31194307");
    assert!(tip.contains("✗"), "{tip}");
    assert!(tip.contains("rounds"), "the rule is in the hint: {tip}");
    let p = problems(&mut h);
    assert!(
        p.iter().any(|m| m.contains("does not hold")),
        "a failed check is a problem: {p:?}"
    );
}

/// E4, P6: a check against a written number passes when the value rounds to
/// it (35.31… is 35.3 to the digits written).
#[test]
fn a_check_passes_when_the_value_rounds_to_the_number_written() {
    let mut h = typed_after(DEFS, &[Type("R = 35.3"), ENTER, Pause]);
    let (_, tip) = last_slot(&h);
    assert!(tip.contains("✓"), "{tip}");
    assert!(tip.contains("rounds to") && tip.contains("35.3"), "{tip}");
    let p = problems(&mut h);
    assert!(!p.iter().any(|m| m.contains("does not hold")), "{p:?}");
}

/// S9, T6, P4, R7: every way of writing "about" is a tolerant check, and `==`
/// is a check too.
#[test]
fn about_and_double_equals_are_checks() {
    for line in [
        r"R \approx 35.3",
        "R ≈ 35.3",
        "R ~= 35.3",
        "R ~ 35.3",
        "R approx 35.3",
        "R == 35.3",
    ] {
        let text = format!("{DEFS}$$ {line} $$");
        let h = typed_after(&text, &[Pause]);
        let (_, tip) = last_slot(&h);
        assert!(tip.contains("✓"), "{line}: {tip}");
        let text = format!("{DEFS}$$ {} $$", line.replace("35.3", "36"));
        let h = typed_after(&text, &[Pause]);
        let (copy, tip) = last_slot(&h);
        assert!(tip.contains("✗"), "{line} against 36: {tip}");
        assert_eq!(copy, "35.31194307", "{line}");
    }
}

/// S9, P4: typed the way people write "about" and coder checks, a line turns
/// into maths when finished and is checked.
#[test]
fn typed_about_and_coder_checks_become_checks() {
    for line in ["R ~ 35.3", "R == 35.3  # the range", "R approx 35.3"] {
        let h = typed_after(DEFS, &[Type(line), ENTER, Pause]);
        let ticks = slots(&h).iter().filter(|(_, t)| t.contains("✓")).count();
        assert_eq!(ticks, 1, "{line}: {:?}", slots(&h));
    }
}

/// P4, P5: comparisons give ✓ or ✗.
#[test]
fn comparisons_give_a_tick_or_a_cross() {
    for (line, holds) in [
        ("abs(R - 35.3) < 0.1", true),
        ("R < 30", false),
        ("R > 30", true),
        ("R ≤ 35", false),
        (r"R \ge 35", true),
    ] {
        let text = format!("{DEFS}$$ {line} $$");
        let h = typed_after(&text, &[Pause]);
        let (_, tip) = last_slot(&h);
        let want = if holds { "✓" } else { "✗" };
        assert!(tip.contains(want), "{line}: {tip}");
    }
}

/// E5, P9: an exact answer that is not a whole number shows its decimal beside
/// it, as definitions do.
#[test]
fn an_exact_fraction_shows_its_decimal() {
    let h = typed_after("", &[Type("sum(1/n^2, n, 1, 10) ="), Pause]);
    let s = Seen::of(&h);
    let last = s.visible.last().expect("a line");
    assert!(last.contains("1.549767731"), "no decimal: {last}");
    let (copy, _) = last_slot(&h);
    assert_eq!(
        copy, "1968329/1270080",
        "a click still copies the exact value"
    );
}

/// E7, E8: `round(x, n)` rounds, its name is upright (never italic letters), and
/// `%digits 4` (the same as `%sf 4`) sets the significant figures.
#[test]
fn round_and_digits() {
    let h = typed_after(DEFS, &[Type("round(R, 1) ="), Pause]);
    let s = Seen::of(&h);
    let last = s.visible.last().expect("a line");
    assert!(last.starts_with("round("), "the name is upright: {last}");
    assert_eq!(last_slot(&h).0, "35.3", "{last}");
    let text = format!("{DEFS}%digits 4\n\n$$ R = $$");
    let h = typed_after(&text, &[Pause]);
    assert_eq!(last_slot(&h).0, "35.31");
}

/// P2, E7: an unknown function is an error in the answer slot with the form to
/// write, never a product of its letters with g put in.
#[test]
fn an_unknown_function_is_an_error_not_an_answer() {
    let mut h = typed_after(DEFS, &[Type("integrate(x**2, x, 0, 3) ="), Pause]);
    let s = Seen::of(&h);
    let last = s.visible.last().expect("a line");
    assert!(
        last.starts_with("integrate("),
        "the name is upright: {last}"
    );
    assert!(
        !last.contains("9.81"),
        "an answer made up from the letters: {last}"
    );
    let (copy, _) = last_slot(&h);
    assert!(copy.contains("integrate is not a known function"), "{copy}");
    assert!(copy.contains("int(f, x, a, b)"), "{copy}");
    let p = problems(&mut h);
    assert!(
        p.iter().any(|m| m.contains("is not a known function")),
        "{p:?}"
    );
    let h = typed_after(DEFS, &[Type("foo(3) ="), Pause]);
    assert!(last_slot(&h).0.contains("foo is not a known function"));
}

/// R8: a name defined nowhere gives no answer but a warning, in the slot and in
/// Problems.
#[test]
fn an_undefined_name_is_said_not_echoed() {
    let mut h = typed_after(DEFS, &[Type("w + 1 = ?"), Pause]);
    let (copy, _) = last_slot(&h);
    assert!(copy.contains("w is not defined"), "{copy}");
    let p = problems(&mut h);
    assert!(p.iter().any(|m| m.contains("is not defined")), "{p:?}");
}

/// P7, P8: prose that was meant as a question gets a hint with the form to type.
#[test]
fn coder_idioms_in_prose_get_a_hint() {
    for (line, want) in [
        ("R", "type R ="),
        ("print(R)", "type R ="),
        ("integrate(x**2, (x, 0, 3))", "int(f, x, a, b)"),
        ("sum(1/n**2 for n in range(1, 11))", "sum(f, n, a, b)"),
    ] {
        let mut h = typed_after(DEFS, &[Type(line), Pause]);
        let p = problems(&mut h);
        assert!(p.iter().any(|m| m.contains(want)), "{line}: {p:?}");
    }
}

/// S7: names used by another row of the same maths box are used.
#[test]
fn names_used_in_the_same_box_are_not_unused() {
    let rows = concat!(
        "$$ g = 9.81 \\\\",
        "\nv = 20 \\\\",
        "\n\\theta = \\pi/6 \\\\",
        "\nR = v^2 \\sin(2\\theta)/g \\\\",
        "\nR = 35.3 $$\n\n"
    );
    let mut h = typed_after(rows, &[Pause]);
    let p = problems(&mut h);
    assert!(!p.iter().any(|m| m.contains("Unused")), "{p:?}");
}

/// E9 (Design rule 5): an answer with units arriving moves no line.
#[test]
fn a_unit_answer_moves_nothing() {
    let text = "$$ q = 20 m/s $$\n\n$$ q*3600/1000 = $$\n\nAfter the answer.\n\n$$ 1 + 1 = ? $$\n\nThe end.";
    let mut h = harness(app_with(text));
    h.run_steps(2);
    let before = Seen::of(&h).tops;
    run(&mut h, &[Pause], |_, _, _| {});
    let s = Seen::of(&h);
    assert!(
        s.answers.iter().any(|a| a.contains("72")),
        "{:?}",
        s.answers
    );
    for ((span, a), (_, b)) in before.iter().zip(&s.tops) {
        assert!(
            (a - b).abs() < 0.5,
            "the line at {span:?} moved from {a} to {b}"
        );
    }
}

/// E6: `~= ?` (≈ ?) gives the decimal in the answer slot, not a fraction in the
/// formula.
#[test]
fn about_equals_question_gives_the_decimal_in_the_slot() {
    let h = typed_after("", &[Type("sum(1/n^2, n, 1, 10) ~= ?"), Pause]);
    assert_eq!(last_slot(&h).0, "1.549767731");
}
