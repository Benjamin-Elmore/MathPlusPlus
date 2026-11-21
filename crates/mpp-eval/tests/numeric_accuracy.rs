//! Numeric accuracy: checks decide exact differences exactly, `≈` answers are worked out
//! in high precision (no `∞` for large values, no lost digits from cancellation), exact
//! decimals round exactly, and infinite sums show only confirmed digits. Each row is an
//! input with its exact expected answer (references from mpmath at 50+ digits).

use std::sync::Arc;

use mpp_eval::{Evaluated, NoImports, Session, Verdict};

fn run(src: &str) -> Evaluated {
    Session::new()
        .with_importer(Arc::new(NoImports))
        .run_str(src)
        .clone()
}

/// The verdict of the first check.
fn verdict(src: &str) -> Verdict {
    let ev = run(src);
    let errs: Vec<String> = ev.errors().iter().map(|d| d.message.clone()).collect();
    assert!(errs.is_empty(), "{src}: {errs:?}");
    let (_, line) = ev.queries().next().expect("a check");
    line.verdict.clone().expect("a verdict")
}

/// The numeric answer of the first query (no errors allowed).
fn approx(src: &str) -> String {
    let ev = run(src);
    let errs: Vec<String> = ev.errors().iter().map(|d| d.message.clone()).collect();
    assert!(errs.is_empty(), "{src}: {errs:?}");
    let (_, line) = ev.queries().next().expect("a query");
    line.approx.clone().unwrap_or_default()
}

#[test]
fn false_equalities_with_tiny_gaps_fail() {
    let rows = [
        r"$$ x \overset{?}{=} x + 10^{-12} $$",
        r"$$ 2^{100} + 1 \overset{?}{=} 2^{100} $$",
        r"$$ 10^{20} + 1 \overset{?}{=} 10^{20} $$",
        r"$$ 10^{-20} \overset{?}{=} 0 $$",
        r"$$ \pi \overset{?}{=} 3.14159265358979 $$",
        r"$$ \zeta(3) \overset{?}{=} 1.2020569 $$",
        r"$$ \frac{1}{3} \overset{?}{=} 0.333333333333 $$",
        r"$$ \sin x \overset{?}{=} \sin x + 10^{-9} $$",
        r"$$ e^{\pi\sqrt{163}} \overset{?}{=} 262537412640768744 $$",
        r"$$ x^2 \overset{?}{=} x^2 + 10^{-15} x $$",
    ];
    for src in rows {
        let v = verdict(src);
        assert_eq!(v.mark(), "✗", "{src}: {}", v.describe());
    }
}

#[test]
fn failing_checks_show_the_digits_that_differ() {
    let v = verdict(r"$$ \pi \overset{?}{=} 3.14159265358979 $$");
    assert_eq!(
        v.describe(),
        "✗ (left side 3.14159265358979324, right side 3.14159265358979)"
    );
    let v = verdict(r"$$ 2^{100} + 1 \overset{?}{=} 2^{100} $$");
    assert_eq!(
        v.describe(),
        "✗ (left side 1267650600228229401496703205377, right side 1267650600228229401496703205376)"
    );
    let v = verdict(r"$$ e^{\pi\sqrt{163}} \overset{?}{=} 262537412640768744 $$");
    assert_eq!(
        v.describe(),
        "✗ (left side 262537412640768743.99999999999925, right side 262537412640768744)"
    );
}

#[test]
fn true_statements_still_pass_and_unproved_constants_are_approximate() {
    for src in [
        r"$$ \sin^2 x + \cos^2 x \overset{?}{=} 1 $$",
        r"$$ (x + 1)^2 \overset{?}{=} x^2 + 2x + 1 $$",
        r"$$ \frac{1}{4} \overset{?}{=} 0.25 $$",
        r"$$ \sqrt{8} \overset{?}{=} 2\sqrt{2} $$",
    ] {
        assert_eq!(verdict(src).passed(), Some(true), "{src}");
    }
    // Equal to every digit tried, but not proved: ≈, never ✓.
    let v = verdict(
        r"$$ \Gamma(\tfrac{1}{3}) \Gamma(\tfrac{2}{3}) \overset{?}{=} \frac{2\pi}{\sqrt{3}} $$",
    );
    assert!(
        matches!(v, Verdict::Proved | Verdict::Agrees { .. }),
        "{v:?}"
    );
    if let Verdict::Agrees { .. } = v {
        assert_eq!(v.mark(), "≈");
        assert!(
            v.describe().contains("checked numerically"),
            "{}",
            v.describe()
        );
    }
}

#[test]
fn approximations_beyond_the_float_range() {
    let rows = [
        (r"$$ 200! \approx ? $$", "7.886578674 × 10³⁷⁴"),
        (r"$$ e^{1000} \approx ? $$", "1.970071114 × 10⁴³⁴"),
        (r"$$ \ln(10^{400}) \approx ? $$", "921.0340372"),
        (r"$$ \Gamma(10^{6}) \approx ? $$", "8.263931688 × 10⁵⁵⁶⁵⁷⁰²"),
        (r"$$ \Gamma(200) \approx ? $$", "3.943289337 × 10³⁷²"),
        (r"$$ (10^{10})! \approx ? $$", "2.325796206 × 10⁹⁵⁶⁵⁷⁰⁵⁵¹⁸⁶"),
        (r"$$ 10^{10^{10}} \approx ? $$", "1 × 10¹⁰⁰⁰⁰⁰⁰⁰⁰⁰⁰"),
    ];
    for (src, want) in rows {
        assert_eq!(approx(src), want, "{src}");
    }
}

#[test]
fn approximations_keep_digits_through_cancellation() {
    let rows = [
        (
            r"$$ \pi - 3.14159265358979 \approx ? $$",
            "3.238462643 × 10⁻¹⁵",
        ),
        (
            r"$$ e^{\pi\sqrt{163}} - 262537412640768744 \approx ? $$",
            "-7.499274028 × 10⁻¹³",
        ),
        (r"$$ \sqrt{10^{20} + 1} - 10^{10} \approx ? $$", "5 × 10⁻¹¹"),
    ];
    for (src, want) in rows {
        assert_eq!(approx(src), want, "{src}");
    }
}

#[test]
fn decimal_places_round_the_exact_value() {
    let rows = [
        ("%dp 2\n$$ 2.675 \\approx ? $$", "2.68"),
        ("%dp 2\n$$ 1.005 \\approx ? $$", "1.01"),
        ("%dp 1\n$$ 0.25 \\approx ? $$", "0.3"),
        ("%dp 2\n$$ -2.675 \\approx ? $$", "-2.68"),
        ("%dp 3\n$$ \\pi \\approx ? $$", "3.142"),
        ("%sf 3\n$$ 2.675 \\approx ? $$", "2.68"),
    ];
    for (src, want) in rows {
        assert_eq!(approx(src), want, "{src}");
    }
}

#[test]
fn relations_between_constants_are_decided_in_high_precision() {
    let v = verdict(r"$$ \pi < 3.14159265358980 \overset{?}{=} \text{true} $$");
    assert_eq!(v.passed(), Some(true), "{}", v.describe());
    let v = verdict(r"$$ \pi > 3.14159265358980 \overset{?}{=} \text{true} $$");
    assert_eq!(v.mark(), "✗", "{}", v.describe());
    let v = verdict(
        r"$$ \left| \pi - 3.14159265358979323846264338327950288419716939937510582 \right| < 10^{-50} \overset{?}{=} \text{true} $$",
    );
    assert_eq!(v.passed(), Some(true), "{}", v.describe());
}
