//! The check helper (C6): `\overset{?}{=}`, `≟` and `?=` prove identities symbolically
//! (trigonometric ones included), fall back to numeric spot checks with a counterexample,
//! and accept antiderivatives that differ by a constant. Also the quick calculator.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use mpp_eval::{calc_line, Evaluated, LineIndex, NoImports, Session, Verdict};

fn run(src: &str) -> Evaluated {
    Session::new()
        .with_importer(Arc::new(NoImports))
        .run_str(src)
        .clone()
}

fn verdict(src: &str) -> Verdict {
    let ev = run(src);
    let errs: Vec<String> = ev.errors().iter().map(|d| d.message.clone()).collect();
    assert!(errs.is_empty(), "{src}: {errs:?}");
    let (_, line) = ev.queries().next().expect("a check");
    line.verdict.clone().expect("a verdict")
}

fn examples_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples")
}

fn render(src: &str) -> String {
    let ev = run(src);
    let index = LineIndex::new(src);
    let mut out = String::new();
    for (block, line) in ev.queries() {
        let text = src.get(line.span.start..line.span.end).unwrap_or("").trim();
        let _ = writeln!(out, "{}: {text}", index.line_of(line.span.start));
        if let Some(a) = line.plain_answer() {
            let _ = writeln!(out, "    {a}");
        }
        for d in &block.diagnostics {
            if d.span.start >= line.span.start && d.span.end <= line.span.end {
                let _ = writeln!(out, "    note: {}", d.message);
            }
        }
    }
    out
}

#[test]
fn checks_example() {
    let dir = examples_dir();
    let src = std::fs::read_to_string(dir.join("checks.mpp"))
        .expect("checks.mpp")
        .replace("\r\n", "\n");
    let got = render(&src);
    let path = dir.join("checks.expected.txt");
    if std::env::var_os("MPP_BLESS").is_some() {
        std::fs::write(&path, &got).expect("write expected");
        return;
    }
    let expected = std::fs::read_to_string(&path)
        .expect("checks.expected.txt")
        .replace("\r\n", "\n");
    assert!(
        got == expected,
        "checks output changed\n--- got ---\n{got}\n--- expected ---\n{expected}"
    );
}

#[test]
fn trigonometric_identities_are_proved_symbolically() {
    for src in [
        r"$$ \sin^2 x + \cos^2 x \overset{?}{=} 1 $$",
        r"$$ \sin 2x \overset{?}{=} 2 \sin x \cos x $$",
        r"$$ \cos 2x \overset{?}{=} 2\cos^2 x - 1 $$",
        r"$$ \cos 3x \overset{?}{=} 4 \cos^3 x - 3 \cos x $$",
        r"$$ 1 + \cot^2 x \overset{?}{=} \csc^2 x $$",
        r"$$ \cos(x - y) \overset{?}{=} \cos x \cos y + \sin x \sin y $$",
    ] {
        assert_eq!(verdict(src), Verdict::Proved, "{src}");
    }
}

#[test]
fn all_three_spellings_check() {
    for src in [
        r"$$ x + x \overset{?}{=} 2x $$",
        "$$ x + x ≟ 2x $$",
        "$$ x + x ?= 2x $$",
    ] {
        assert_eq!(verdict(src).passed(), Some(true), "{src}");
    }
}

#[test]
fn a_false_identity_has_a_counterexample() {
    let v = verdict(r"$$ (x + 1)^2 \overset{?}{=} x^2 + 1 $$");
    assert_eq!(v.mark(), "✗");
    let Verdict::Fails { at, .. } = &v else {
        panic!("{v:?}");
    };
    assert_eq!(at.len(), 1);
    assert_eq!(at[0].0, "x");
    // √(x²) = x fails for negative x.
    assert_eq!(
        verdict(r"$$ \sqrt{x^2} \overset{?}{=} x $$").passed(),
        Some(false)
    );
}

#[test]
fn antiderivatives_may_differ_by_a_constant() {
    let v = verdict(r"$$ \int \sin x \cos x \, dx \overset{?}{=} \frac{\sin^2 x}{2} $$");
    assert_eq!(v.passed(), Some(true));
    let v = verdict(r"$$ \int \sin x \cos x \, dx \overset{?}{=} -\frac{\cos^2 x}{2} $$");
    assert_eq!(v.passed(), Some(true));
    // A wrong antiderivative is still wrong.
    let v = verdict(r"$$ \int \sin x \, dx \overset{?}{=} \cos x $$");
    assert_eq!(v.passed(), Some(false));
}

#[test]
fn checks_never_auto_evaluate() {
    let ev = run(r"$$ \int_0^1 x \, dx \overset{?}{=} \frac{1}{2} $$");
    assert!(ev.auto_results().next().is_none());
}

#[test]
fn trig_answers_are_tidied() {
    let ev = run(r"$$ \frac{\partial(r \cos\theta, r \sin\theta)}{\partial(r, \theta)} = ? $$");
    let (_, line) = ev.queries().next().expect("a query");
    assert_eq!(line.plain.as_deref(), Some("r"));
}

#[test]
fn quick_calculator() {
    assert_eq!(calc_line(r"\int_0^1 x^2 dx").as_deref(), Ok("1/3"));
    assert_eq!(
        calc_line(r"\sum_{n=1}^{\infty} \frac{1}{n^2}").as_deref(),
        Ok("π²/6 ≈ 1.644934067")
    );
    assert_eq!(
        calc_line(r"y'' + y = 0, \quad y(0) = 1, y'(0) = 0").as_deref(),
        Ok("y(x) = cos(x)")
    );
    assert_eq!(
        calc_line(r"\tan^2 x + 1 ≟ \sec^2 x").as_deref(),
        Ok("✓ (proved symbolically)")
    );
    let err = calc_line(r"\int_0^{} x \, dx").unwrap_err();
    assert!(err.to_string().contains("upper bound"), "{err}");
}

#[test]
fn checks_for_all_values_of_a_domain() {
    // Holds over the domain, though not for every real x.
    let v = verdict(r"$$ x^2 - 1 \ge 0 \text{ for all } x \ge 1 \overset{?}{=} \text{true} $$");
    assert_eq!(v.passed(), Some(true), "{}", v.describe());
    // Fails far out in the domain: the counterexample is reported.
    let v = verdict(r"$$ 10 - x > 0 \text{ for all } x \ge 0 \overset{?}{=} \text{true} $$");
    assert_eq!(v.passed(), Some(false), "{}", v.describe());
    assert!(v.describe().contains("x = "), "{}", v.describe());
    // A bounded domain.
    let v =
        verdict(r"$$ \sin x \ge 0 \text{ for all } 0 \le x \le 3 \overset{?}{=} \text{true} $$");
    assert_eq!(v.passed(), Some(true), "{}", v.describe());
    let v =
        verdict(r"$$ \sin x \ge 0 \text{ for all } 0 \le x \le 4 \overset{?}{=} \text{true} $$");
    assert_eq!(v.passed(), Some(false), "{}", v.describe());
}

#[test]
fn checks_for_all_times_with_units() {
    let src = r"Let $m := 0.5\,\mathrm{kg}$, $k := 20\,\mathrm{N/m}$, $c := 0.4\,\mathrm{N\,s/m}$ and $x_0 := 0.1\,\mathrm m$.

$$ m\ddot x + c\dot x + kx = 0, \quad x(0) = x_0, \ \dot x(0) = 0 $$

$$ E(t) := \tfrac12 m \dot x^2 + \tfrac12 k x^2 $$
$$ \frac{dE}{dt} \le 0 \ \text{ for all } t \ge 0 \overset{?}{=} \text{true} $$
$$ \frac{dE}{dt} \ge 0 \ \text{ for all } t \ge 0 \overset{?}{=} \text{true} $$";
    let ev = run(src);
    let errs: Vec<String> = ev.errors().iter().map(|d| d.message.clone()).collect();
    assert!(errs.is_empty(), "{errs:?}");
    let verdicts: Vec<Option<bool>> = ev
        .queries()
        .map(|(_, l)| l.verdict.as_ref().and_then(Verdict::passed))
        .collect();
    assert_eq!(verdicts, vec![Some(true), Some(false)]);
}
