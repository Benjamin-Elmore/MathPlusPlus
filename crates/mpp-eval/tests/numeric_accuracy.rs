//! Numeric accuracy: checks decide exact differences exactly, `≈` answers are worked out
//! in high precision (no `∞` for large values, no lost digits from cancellation), exact
//! decimals round exactly, and infinite sums show only confirmed digits. Each row is an
//! input with its exact expected answer (references from mpmath at 50+ digits).

use std::sync::Arc;

use mpp_eval::{Evaluated, NoImports, Session};

fn run(src: &str) -> Evaluated {
    Session::new()
        .with_importer(Arc::new(NoImports))
        .run_str(src)
        .clone()
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
