//! Scripts 2 and 3: the same short worksheet typed by a mathematician (LaTeX
//! style) and by a coder (code style). Both must give the same answers and the
//! same maths on the page; only the saved text may differ in form.

use egui::Key;

use super::runner::{fresh, run, snapshot, Seen, Step, Watch, H};

use Step::{Pause, Type};

const ENTER: Step = Step::Key(Key::Enter);

/// A mathematician: `$` starts a maths line, LaTeX commands, `:=` definitions.
const MATHEMATICIAN: &[Step] = &[
    Type("#Worksheet"),
    ENTER,
    Type(r"$a := 3$$"),
    ENTER,
    Type(r"$\frac{a}{2} = ?$$"),
    ENTER,
    Pause,
    Type(r"$\int_{0}^{1} x^2 \, dx = ?$$"),
    ENTER,
    Type(r"$\alpha := \sqrt{16}$$"),
    ENTER,
    Type(r"$2 \alpha + a = ?$$"),
    ENTER,
    Pause,
];

/// A coder: plain lines that turn into maths when finished, `**`, `*`,
/// `sqrt(…)`, `int(…)` and `x = 5` definitions.
const CODER: &[Step] = &[
    Type("#Worksheet"),
    ENTER,
    Type("a = 3"),
    ENTER,
    Type("a/2 = ?"),
    ENTER,
    Pause,
    Type("int(x**2, x, 0, 1) = ?"),
    ENTER,
    Type("alpha = sqrt(16)"),
    ENTER,
    Type("2*alpha + a = ?"),
    ENTER,
    Pause,
];

fn typed(script: &[Step]) -> H {
    let mut h = fresh();
    let mut w = Watch::new();
    run(&mut h, script, |step, kind, s| w.frame(step, kind, s));
    h
}

/// What the evaluator read on every maths line, and what it answered: the
/// line's kind, its input as parsed, and the answer.
fn results(h: &H) -> Vec<String> {
    let doc = h.state().doc().expect("doc");
    let ev = doc.eval.as_deref().expect("evaluated");
    ev.blocks
        .iter()
        .flat_map(|b| b.values.iter())
        .map(|l| {
            format!(
                "{:?} | {} | {}",
                l.kind,
                l.input
                    .as_ref()
                    .map(ToString::to_string)
                    .unwrap_or_default(),
                l.text.clone().unwrap_or_default()
            )
        })
        .collect()
}

/// The answers to the `= ?` lines as drawn: the glyphs after their `=`.
fn drawn_answers(s: &Seen) -> Vec<String> {
    [2, 3, 5]
        .iter()
        .filter_map(|&i| {
            s.visible
                .get(i)?
                .rsplit_once('=')
                .map(|(_, a)| a.to_string())
        })
        .collect()
}

#[test]
fn a_mathematician_and_a_coder_get_the_same_worksheet() {
    let maths = typed(MATHEMATICIAN);
    let code = typed(CODER);
    // The same lines read the same way, with the same answers.
    let want = [
        "Definition { name: \"a\", kind: Variable } | 3 | 3",
        "Query { mode: Exact, steps: false } | a/2 | 3/2",
        "Query { mode: Exact, steps: false } | ∫_0^1 x^2 dx | 1/3",
        "Definition { name: \"α\", kind: Variable } | 4 | 4",
        "Query { mode: Exact, steps: false } | a + 2*α | 11",
    ];
    assert_eq!(results(&maths), want);
    assert_eq!(results(&code), want);
    let (sm, sc) = (Seen::of(&maths), Seen::of(&code));
    // Exact fractions show their decimal beside them (E5).
    assert_eq!(drawn_answers(&sm), ["32≈1.5", "13≈0.3333333333", "11"]);
    assert_eq!(drawn_answers(&sc), drawn_answers(&sm));
    // Both are a heading and five maths lines; only the saved text differs.
    assert_eq!((sm.maths, sc.maths), (5, 5));
    assert_eq!(sm.visible[0], "Worksheet");
    assert_eq!(sc.visible[0], "Worksheet");
    assert_eq!(
        sm.text,
        "# Worksheet\n\n$$ a := 3 $$\n\n$$ \\frac{a}{2} = ? $$\n\n\
         $$ \\int_{0}^{1} x^{2} \\, dx = ? $$\n\n$$ α := \\sqrt{16} $$\n\n\
         $$ 2 α + a = ? $$\n\n"
    );
    // The coder's lines became maths by themselves, so they were converted as
    // typing in maths converts them (U28, P13 and S16).
    assert_eq!(
        sc.text,
        "# Worksheet\n\n$$ a = 3 $$\n\n$$ \\frac{a}{2} = ? $$\n\n\
         $$ \\int_{0}^{1} x^{2} \\, dx = ? $$\n\n$$ α = \\sqrt{16} $$\n\n\
         $$ 2·α + a = ? $$\n\n"
    );
    let (mut maths, mut code) = (maths, code);
    snapshot(&mut maths, "replay_worksheet_latex");
    snapshot(&mut code, "replay_worksheet_code");
}

#[test]
fn a_fraction_answer_moves_nothing() {
    let mut h = fresh();
    let mut w = Watch::new();
    run(
        &mut h,
        &[
            Type("#Worksheet"),
            ENTER,
            Type("$a := 3$$"),
            ENTER,
            Type(r"$\frac{a}{2} = ?$$"),
            ENTER,
            Pause,
        ],
        |step, kind, s| w.frame(step, kind, s),
    );
}
