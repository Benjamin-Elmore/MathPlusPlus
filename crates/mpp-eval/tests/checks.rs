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

/// A check over a symbolic variable passes only when it holds for every value: random
/// points never land on the integers or on 0, where piecewise functions change rule,
/// so those are tried on purpose and a sampled piecewise identity is never ✓.
#[test]
fn false_identities_never_pass_by_sampling() {
    for src in [
        r"$$ \lfloor x \rfloor + \lfloor -x \rfloor \overset{?}{=} -1 $$",
        r"$$ \lceil x \rceil - \lfloor x \rfloor \overset{?}{=} 1 $$",
        r"$$ \lceil x \rceil \overset{?}{=} \lfloor x \rfloor + 1 $$",
        r"$$ \lfloor x + y \rfloor \overset{?}{=} \lfloor x \rfloor + \lfloor y \rfloor $$",
        r"$$ \lfloor 3x \rfloor \overset{?}{=} 3 \lfloor x \rfloor $$",
        r"$$ \lfloor x \rfloor^2 \overset{?}{=} \lfloor x^2 \rfloor $$",
        r"$$ \operatorname{sgn}(x)^2 \overset{?}{=} 1 $$",
        r"$$ \operatorname{sgn}(x y) \overset{?}{=} \operatorname{sgn}(x) $$",
        r"$$ |x| \overset{?}{=} x $$",
        r"$$ |x + 1| \overset{?}{=} |x| + 1 $$",
        r"$$ \frac{|x|}{x} \overset{?}{=} 1 $$",
        r"$$ \max(x, 5) \overset{?}{=} 5 $$",
        r"$$ \min(x, -x) \overset{?}{=} -x $$",
        r"$$ \sqrt{x^2} \overset{?}{=} x $$",
        r"$$ (x^a)^b \overset{?}{=} x^{a b} $$",
        r"$$ \ln(x y) \overset{?}{=} \ln x + \ln y $$",
        r"$$ \ln(x^2) \overset{?}{=} 2 \ln x $$",
        r"$$ \sqrt{x y} \overset{?}{=} \sqrt{x} \sqrt{y} $$",
        r"$$ \arcsin(\sin x) \overset{?}{=} x $$",
        r"$$ \arctan x + \arctan \frac{1}{x} \overset{?}{=} \frac{\pi}{2} $$",
        r"$$ \sqrt{x^2 - 16} \overset{?}{=} \sqrt{x - 4} \sqrt{x + 4} $$",
        r"$$ x^2 > 0 \overset{?}{=} \text{true} $$",
        r"$$ (x - 1)^2 > 0 \overset{?}{=} \text{true} $$",
        r"$$ x^2 + y^2 > 0 \overset{?}{=} \text{true} $$",
    ] {
        let v = verdict(src);
        assert_eq!(v.passed(), Some(false), "{src}: {v:?}");
    }
    // The reviewer's case: the counterexample is x = 0.
    let v = verdict(r"$$ \lfloor x \rfloor + \lfloor -x \rfloor \overset{?}{=} -1 $$");
    let Verdict::Fails { at, lhs, rhs } = &v else {
        panic!("{v:?}");
    };
    assert_eq!(at, &[("x".to_string(), "0".to_string())]);
    assert_eq!((lhs.as_str(), rhs.as_str()), ("0", "-1"));
}

/// True identities with `| |` and `sgn` are proved by cases on the signs of the
/// variables; a true-looking piecewise identity that cannot be proved stays undecided.
#[test]
fn piecewise_identities_are_proved_or_left_undecided() {
    for src in [
        r"$$ |x|^2 \overset{?}{=} x^2 $$",
        r"$$ |x y| \overset{?}{=} |x| |y| $$",
        r"$$ \operatorname{sgn}(x) |x| \overset{?}{=} x $$",
        r"$$ \frac{x}{|x|} \overset{?}{=} \operatorname{sgn}(x) $$",
    ] {
        assert_eq!(verdict(src), Verdict::Proved, "{src}");
    }
    let v = verdict(r"$$ \lfloor x \rfloor \le x \overset{?}{=} \text{true} $$");
    assert_eq!(v.passed(), None, "{v:?}");
    assert_eq!(v.mark(), "?");
}

/// d/dx ⌊x⌋ = 0 only away from the integers, and the answer says so.
#[test]
fn derivative_of_floor_notes_the_integers() {
    let ev = run(r"$$ \frac{d}{dx} \lfloor x \rfloor = ? $$");
    let (block, line) = ev.queries().next().expect("a query");
    assert_eq!(line.plain_answer().as_deref(), Some("= 0"));
    assert!(
        block
            .diagnostics
            .iter()
            .any(|d| d.message.contains("except there") && d.message.contains("integers")),
        "{:?}",
        block.diagnostics
    );
}

/// A displayed `=` between two closed sides is a statement that can be decided: a false
/// one is an error like a failed check, and one with a typed decimal is a warning.
#[test]
fn false_numeric_statements_are_reported() {
    let ev = run(r"$$ 2^{10} = 1000 $$

$$ 2^{10} = 1024 $$

$$ \pi = 3.14159 $$
");
    let errors: Vec<String> = ev.errors().iter().map(|d| d.message.clone()).collect();
    assert_eq!(errors.len(), 1, "{errors:?}");
    assert!(errors[0].contains("this statement is false"), "{errors:?}");
    assert!(errors[0].contains("1024") && errors[0].contains("1000"));
    let warnings: Vec<&str> = ev
        .all_diagnostics()
        .into_iter()
        .filter(|d| d.severity == mpp_syntax::Severity::Warning)
        .map(|d| d.message.as_str())
        .collect();
    assert!(
        warnings.iter().any(|w| w.contains("write ≈")),
        "{warnings:?}"
    );
    // Statements with unknowns are equations, not claims to decide.
    assert!(run("$$ x^2 = 4 $$\n").errors().is_empty());
}

/// Congruences asked as queries are answered true or false, and a fraction modulo m
/// uses the inverse of its denominator.
#[test]
fn congruences_and_inverses_modulo_m() {
    let answer = |src: &str| {
        let ev = run(src);
        let errs: Vec<String> = ev.errors().iter().map(|d| d.message.clone()).collect();
        assert!(errs.is_empty(), "{src}: {errs:?}");
        let (_, line) = ev.queries().next().expect("a query");
        line.text.clone().unwrap_or_default()
    };
    assert_eq!(answer(r"$$ 17 \equiv 2 \pmod{5} = ? $$"), "true ✓");
    assert!(answer(r"$$ 17 \equiv 3 \pmod{5} = ? $$").starts_with("false ✗"));
    assert_eq!(answer(r"$$ 1 < 2 = ? $$"), "true ✓");
    assert_eq!(answer(r"$$ 3^{-1} \bmod 7 = ? $$"), "5");
}

/// An interval whose ends are the wrong way round is empty.
#[test]
fn reversed_interval_is_empty() {
    let ev = run("$$ [3, 1] = ? $$\n\n$$ [2, 2] = ? $$\n");
    let texts: Vec<String> = ev
        .queries()
        .map(|(_, l)| l.text.clone().unwrap_or_default())
        .collect();
    assert_eq!(texts, ["∅", "{2}"]);
    assert!(ev
        .all_diagnostics()
        .iter()
        .any(|d| d.message.contains("holds no numbers")));
}

/// A long display format on a value only known in floating point says so instead of
/// silently showing fewer digits; a ζ value has all of them.
#[test]
fn long_formats_mark_floating_point_values() {
    let ev = run(r"%dp 30

$$ \sum_{n=1}^{\infty} \frac{1}{n^3} \approx ? $$

$$ \sum_{n=1}^{\infty} \frac{1}{n^2 + 1} \approx ? $$
");
    let shown: Vec<String> = ev
        .queries()
        .map(|(_, l)| l.approx.clone().unwrap_or_default())
        .collect();
    assert_eq!(shown[0], "1.202056903159594285399738161511");
    assert!(shown[1].ends_with('…'), "{shown:?}");
    assert!(ev.all_diagnostics().iter().any(|d| d
        .message
        .contains("significant digits of this value are known")));
}

/// A fit's R² and what a helper's value means are information, not warnings.
#[test]
fn descriptive_notes_are_info() {
    let ev = run(r"| $u$ | $v$ |
|---|---|
| 1 | 2 |
| 2 | 4.1 |
| 3 | 5.9 |
| 10 | 30 |

$$ \operatorname{linreg}(u, v) \approx ? $$

$$ \operatorname{range}(u) = ? $$
");
    let notes: Vec<_> = ev
        .all_diagnostics()
        .into_iter()
        .filter(|d| d.message.contains("R² = ") || d.message.contains("the range of data"))
        .collect();
    assert_eq!(notes.len(), 2, "{notes:?}");
    assert!(notes
        .iter()
        .all(|d| d.severity == mpp_syntax::Severity::Info));
}
