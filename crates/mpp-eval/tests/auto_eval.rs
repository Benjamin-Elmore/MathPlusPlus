//! Auto-evaluation (C8): display maths that is a bare expression, or an equation with an
//! unknown, gets its answer in the view model only (`auto: true`), with a time budget,
//! a pending state, `%noeval`, a session toggle, "fill in" hints and `Ctrl+Enter`.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use mpp_eval::{Evaluated, LineIndex, LineKind, NoImports, Session};
use mpp_syntax::Severity;

fn session() -> Session {
    Session::new().with_importer(Arc::new(NoImports))
}

fn run(src: &str) -> Evaluated {
    session().run_str(src).clone()
}

/// The auto answer of the only auto line.
fn auto(src: &str) -> String {
    let ev = run(src);
    let errs: Vec<String> = ev
        .errors()
        .iter()
        .map(|d| mpp_core::note::plain(&d.message))
        .collect();
    assert!(errs.is_empty(), "{src}: {errs:?}");
    let lines: Vec<_> = ev.auto_results().collect();
    assert_eq!(lines.len(), 1, "{src}: {lines:?}");
    let (_, line) = lines[0];
    assert!(line.auto, "{src}: {line:?}");
    line.plain_answer().unwrap_or_default()
}

fn no_auto(src: &str) {
    let ev = run(src);
    let shown: Vec<String> = ev
        .blocks
        .iter()
        .flat_map(|b| b.values.iter())
        .filter(|l| l.auto)
        .filter_map(|l| l.plain_answer())
        .collect();
    assert!(shown.is_empty(), "{src}: {shown:?}");
}

fn examples_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples")
}

/// Each display line with its auto answer, hint or pending state.
fn render(src: &str) -> String {
    let ev = run(src);
    let index = LineIndex::new(src);
    let mut out = String::new();
    for b in &ev.blocks {
        for line in &b.values {
            if !matches!(line.kind, LineKind::Relation) || !line.auto_eval {
                continue;
            }
            let text = src.get(line.span.start..line.span.end).unwrap_or("").trim();
            let _ = writeln!(out, "{}: {text}", index.line_of(line.span.start));
            if line.auto {
                if let Some(a) = line.plain_answer() {
                    let _ = writeln!(out, "    {a}");
                }
            }
            if line.pending {
                let _ = writeln!(out, "    (pending)");
            }
            if let Some(h) = &line.hint {
                let _ = writeln!(out, "    hint: {h}");
            }
            for d in &b.diagnostics {
                if d.span.start >= line.span.start && d.span.end <= line.span.end {
                    let kind = match d.severity {
                        Severity::Error => "error",
                        Severity::Warning | Severity::Info => "note",
                    };
                    let _ = writeln!(out, "    {kind}: {}", mpp_core::note::plain(&d.message));
                }
            }
        }
    }
    out
}

#[test]
fn auto_evaluation_example() {
    let dir = examples_dir();
    let src = std::fs::read_to_string(dir.join("auto-evaluation.mpp"))
        .expect("auto-evaluation.mpp")
        .replace("\r\n", "\n");
    let got = render(&src);
    let path = dir.join("auto-evaluation.expected.txt");
    if std::env::var_os("MPP_BLESS").is_some() {
        std::fs::write(&path, &got).expect("write expected");
        return;
    }
    let expected = std::fs::read_to_string(&path)
        .expect("auto-evaluation.expected.txt")
        .replace("\r\n", "\n");
    assert!(
        got == expected,
        "auto-evaluation output changed\n--- got ---\n{got}\n--- expected ---\n{expected}"
    );
    // Nothing in the example is an error.
    let ev = run(&src);
    let errs: Vec<String> = ev
        .errors()
        .iter()
        .map(|d| mpp_core::note::plain(&d.message))
        .collect();
    assert!(errs.is_empty(), "{errs:?}");
}

#[test]
fn every_notation_gets_an_answer() {
    assert_eq!(auto(r"$$ \int_0^1 x^2 \, dx $$"), "= 1/3");
    assert_eq!(auto(r"$$ \int x \, dx $$"), "= x^2/2 + C");
    assert_eq!(
        auto(r"$$ \iint_{x^2 + y^2 \le 1} 1 \, dA $$"),
        "= π ≈ 3.141592654"
    );
    assert_eq!(
        auto(r"$$ \iiint_{x^2 + y^2 + z^2 \le 1} 1 \, dV $$"),
        "= 4*π/3 ≈ 4.188790205"
    );
    assert_eq!(
        auto(r"$$ \oint_{x^2 + y^2 = 4} (-y, x) \cdot d\mathbf{r} $$"),
        "= 8*π ≈ 25.13274123"
    );
    assert_eq!(auto(r"$$ \frac{d}{dx} \sin x $$"), "= cos(x)");
    assert_eq!(auto(r"$$ \partial_y (x^2 y^3) $$"), "= 3*x^2*y^2");
    assert_eq!(auto(r"$$ D_x x^3 $$"), "= 3*x^2");
    assert_eq!(auto(r"$$ \nabla (x y) $$"), "= (y, x)");
    assert_eq!(
        auto(r"$$ \frac{\partial(2u, 3v)}{\partial(u, v)} $$"),
        "= 6"
    );
    assert_eq!(auto(r"$$ \lim_{x \to \infty} \frac{1}{x} $$"), "= 0");
    assert_eq!(auto(r"$$ \sum_{k=1}^{4} k $$"), "= 10");
    assert_eq!(auto(r"$$ \mathcal L\{1\} $$"), "= 1/s");
    assert_eq!(
        auto(r"$$ \mathcal{L}^{-1}\{\frac{1}{s - 2}\} $$"),
        "= e^(2*t)"
    );
    assert_eq!(auto(r"$$ 2x - 6 = 0 $$"), "= x = 3");
    assert_eq!(auto(r"$$ y' = 2y, \quad y(0) = 1 $$"), "= y(x) = e^(2*x)");
    assert_eq!(
        auto(r"$$ \det \begin{pmatrix} 2 & 0 \\ 0 & 5 \end{pmatrix} $$"),
        "= 10"
    );
    // A defined name is put in.
    let src = concat!("$$ a := 3 $$", "\n\n", r"$$ a^2 + 1 $$");
    let ev = run(src);
    let (_, line) = ev.auto_results().next().expect("an auto line");
    assert_eq!(line.plain_answer().as_deref(), Some("= 10"));
}

#[test]
fn definitions_checks_and_plain_expressions_show_nothing() {
    no_auto(r"$$ a := \int_0^1 x \, dx $$");
    no_auto(r"$$ \sin^2 x + \cos^2 x \overset{?}{=} 1 $$");
    no_auto(r"$$ x^2 + 1 $$");
    no_auto(r"$$ 5 $$");
    no_auto(r"$$ v = 3 $$");
    // Two unknowns and one equation: nothing to solve.
    no_auto(r"$$ y = x^2 + 1 $$");
    // Inline maths in prose is never auto-evaluated.
    no_auto(r"The area is $\int_0^1 x \, dx$.");
}

#[test]
fn noeval_and_the_session_toggle() {
    no_auto("%noeval\n$$ \\int_0^1 x \\, dx $$\n");
    let mut s = session();
    s.set_auto_eval(false);
    assert!(!s.auto_eval());
    let ev = s.run_str(r"$$ \int_0^1 x \, dx $$").clone();
    assert!(ev.auto_results().next().is_none());
    s.set_auto_eval(true);
    let ev = s.update(r"$$ \int_0^1 x \, dx $$").clone();
    let (_, line) = ev.auto_results().next().expect("an auto line");
    assert_eq!(line.plain_answer().as_deref(), Some("= 1/2"));
    // The file is never changed: the source is what was typed.
    assert_eq!(ev.source(), r"$$ \int_0^1 x \, dx $$");
}

#[test]
fn empty_boxes_give_a_hint_and_no_answer() {
    let ev = run("$$ \\int_{0}^{} {x^2} \\, dx $$\n");
    let (_, line) = ev.auto_results().next().expect("a hinted line");
    assert!(!line.auto);
    assert_eq!(line.hint.as_deref(), Some("fill in: upper bound"));
    let ev = run("$$ \\frac{{}}{2} $$\n");
    let (_, line) = ev.auto_results().next().expect("a hinted line");
    assert_eq!(line.hint.as_deref(), Some("fill in: the empty box"));
}

#[test]
fn a_slow_block_is_left_pending_and_finished_later() {
    let src = concat!(
        r"$$ \int_0^1 x^2 \, dx $$",
        "\n\n",
        r"$$ \sum_{n=1}^{\infty} \frac{1}{n^2} $$",
        "\n"
    );
    let mut s = session();
    s.set_auto_budget(Some(Duration::ZERO));
    let first = s.run_str(src).clone();
    // With no time at all, each line is pending or (on a fast machine) already done.
    for (_, line) in first.auto_results() {
        assert!(line.pending || line.auto, "{line:?}");
    }
    assert!(s.wait_pending(Duration::from_secs(60)));
    let ev = s.refresh().clone();
    assert!(!s.has_pending());
    let answers: Vec<String> = ev
        .auto_results()
        .filter_map(|(_, l)| l.plain_answer())
        .collect();
    assert_eq!(answers, ["= 1/3", "= π^2/6 ≈ 1.644934067"]);
    // Cached: running again computes nothing new and gives the same answers at once.
    let again = s.refresh().clone();
    assert!(again.recomputed.is_empty(), "{:?}", again.recomputed);
}

#[test]
fn keep_result_writes_ordinary_maths() {
    let src = concat!(
        r"$$ \int_0^1 x^2 \, dx $$",
        "\n\n",
        r"$$ x^2 - 5x + 6 = 0 $$",
        "\n\n",
        r"$$ y'' + y = 0, \quad y(0) = 1, y'(0) = 0 $$",
        "\n"
    );
    let ev = run(src);
    let kept: Vec<String> = ev
        .blocks
        .iter()
        .flat_map(|b| {
            b.values
                .iter()
                .enumerate()
                .filter(|(_, l)| l.auto)
                .map(|(i, _)| ev.keep_result_text(b.id, i))
                .collect::<Vec<_>>()
        })
        .collect();
    assert_eq!(
        kept,
        [
            r"\int_0^1 x^2 \, dx = \frac{1}{3}",
            r"x^2 - 5x + 6 = 0 \implies x = 2 \lor x = 3",
            r"y'' + y = 0 \implies y\left(x\right) = \cos\left(x\right), \quad y(0) = 1, y'(0) = 0",
        ]
    );
    // The kept text reads back with no errors and no second answer.
    let mut edited = String::new();
    for k in &kept {
        edited.push_str("$$ ");
        edited.push_str(k);
        edited.push_str(" $$\n\n");
    }
    let ev = run(&edited);
    let errs: Vec<String> = ev
        .errors()
        .iter()
        .map(|d| mpp_core::note::plain(&d.message))
        .collect();
    assert!(errs.is_empty(), "{edited}: {errs:?}");
    assert!(ev.blocks.iter().all(|b| b.values.iter().all(|l| !l.auto)));
}

/// The only query (or check) line of `src`, with its answer as `mpp run` prints it.
fn query(src: &str) -> (mpp_eval::LineResult, String) {
    let ev = run(src);
    let lines: Vec<_> = ev.queries().map(|(_, l)| l.clone()).collect();
    assert_eq!(lines.len(), 1, "{src}: {lines:?}");
    let answer = lines[0].plain_answer().unwrap_or_default();
    (lines[0].clone(), answer)
}

#[test]
fn a_line_ending_in_equals_asks_for_its_answer() {
    // Ben's video: no `?` needed.
    let (line, answer) = query("$$ 1.321321312 + 5 = $$");
    assert!(line.open, "{line:?}");
    assert_eq!(answer, "= 6.321321312");
    let (line, answer) = query("$$ x := 5 $$\n\n$$ 2x + 5 = $$");
    assert!(line.open);
    assert_eq!(answer, "= 15");
    // `= ?`, `≈ ?` and `= ?!` keep their meaning, and are not "open".
    let (line, answer) = query("$$ 1 + 2 = ? $$");
    assert!(!line.open);
    assert_eq!(answer, "= 3");
    let (line, answer) = query(r"$$ \sqrt{2} \approx ? $$");
    assert!(!line.open);
    assert_eq!(answer, "≈ 1.414213562");
    let (line, _) = query(r"$$ \frac{d}{dx} x^2 = ?! $$");
    assert!(line.steps.is_some() && !line.open);
    // Inline maths in a sentence asks the same way.
    let (line, answer) = query("The sum $2 + 3 =$ is small.");
    assert!(line.open);
    assert_eq!(answer, "= 5");
    // A definition ending in `=` shows the value it defined.
    let src = "$$ a := 3 $$\n\n$$ b := a^2 + 1 = $$";
    let ev = run(src);
    let asked: Vec<String> = ev.queries().filter_map(|(_, l)| l.plain_answer()).collect();
    assert_eq!(asked, ["= 10"]);
    // A name with no value is an error in the slot, not a crash.
    let ev = run("$$ q + 1 = $$\n\n$$ 2 \\cdot w = $$");
    assert!(ev.errors().is_empty(), "{:?}", ev.errors());
    // Nothing in an old file means something else now: no error appears.
    let ev = run(r"$$ \int_0^1 x \, dx = ? $$");
    assert!(ev.errors().is_empty());
}

#[test]
fn a_definition_from_other_names_shows_its_value_but_a_plain_number_does_not() {
    let ev = run(
        "$$ x := 5 $$\n\n$$ E := 2x + 5 $$\n\n$$ g := 9.81 $$\n\n$$ h := -2 $$\n\n$$ k := 1/2 $$",
    );
    let defs: Vec<(String, bool, Option<String>)> = ev
        .blocks
        .iter()
        .flat_map(|b| b.values.iter())
        .filter_map(|l| match &l.kind {
            LineKind::Definition { name, .. } => {
                Some((name.clone(), l.shows_value, l.plain.clone()))
            }
            _ => None,
        })
        .collect();
    assert_eq!(
        defs,
        [
            ("x".into(), false, Some("5".into())),
            ("E".into(), true, Some("15".into())),
            ("g".into(), false, Some("9.81".into())),
            ("h".into(), false, Some("-2".into())),
            ("k".into(), false, Some("1/2".into())),
        ]
    );
    // A value with a free name is not a number: nothing to show.
    let ev = run("$$ F := m a $$");
    assert!(ev.blocks[0].values.iter().all(|l| !l.shows_value));
}
