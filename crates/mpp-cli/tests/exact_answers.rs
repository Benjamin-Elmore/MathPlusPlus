//! `mpp run` keeps exact answers exact: a `= ?` query of a constant prints its closed
//! form (`2√2`), the same as `mpp calc`, and falls back to a decimal only when the value
//! is an unevaluated call (`erf(1/2)`).

use std::sync::Arc;

use mpp_cli::run_report;
use mpp_eval::{NoImports, Session};

fn report(src: &str) -> String {
    let ev = Session::new()
        .with_importer(Arc::new(NoImports))
        .run_str(src)
        .clone();
    run_report(&ev).trim().to_string()
}

#[test]
fn constant_queries_print_their_exact_form() {
    let cases = [
        (r"$$ \sqrt{8} = ? $$", "√8 = 2√2"),
        (r"$$ \sqrt{12} + \sqrt{27} = ? $$", "√12 + √27 = 5√3"),
        (r"$$ \cos(\pi/6) = ? $$", "cos(π/6) = √3/2"),
        (r"$$ \frac{1}{1 + i} = ? $$", "1/(1 + i) = 1/2 - i/2"),
        (r"$$ \Gamma(1/2) = ? $$", "Γ(1/2) = √π"),
        (r"$$ \psi(1) = ? $$", "ψ(1) = -γ"),
        (r"$$ \frac{\pi}{2} = ? $$", "π/2 = π/2"),
    ];
    for (src, want) in cases {
        assert_eq!(report(src), want, "{src}");
    }
}

#[test]
fn unevaluated_calls_still_fall_back_to_a_decimal() {
    assert_eq!(
        report(r"$$ \operatorname{erf}(1/2) = ? $$"),
        "erf(1/2) ≈ 0.5204998778"
    );
    assert_eq!(report(r"$$ \zeta(3) = ? $$"), "ζ(3) ≈ 1.202056903");
}

#[test]
fn the_one_line_answer_of_an_unsimplified_value_is_its_decimal() {
    // The editor's report, `mpp calc` and the kept answer all read the one-line answer:
    // `= ζ(3) ≈ 1.202056903` would repeat the question, so it is the decimal alone.
    let answer = |src: &str| {
        let ev = Session::new()
            .with_importer(Arc::new(NoImports))
            .run_str(src)
            .clone();
        let (_, line) = ev.queries().next().expect("a query");
        line.answer().expect("an answer")
    };
    assert_eq!(answer(r"$$ \zeta(3) = ? $$"), "≈ 1.202056903");
    assert_eq!(answer(r"$$ J_0(1) = ? $$"), "≈ 0.7651976866");
    assert_eq!(answer(r"$$ \zeta(2) = ? $$"), "= π²/6 ≈ 1.644934067");
}

#[test]
fn an_oscillating_tail_from_far_out_is_not_rounding_noise() {
    // Doubles near 10^17 are 16 apart, so sampling sin x there gave noise
    // (≈ -6.6 × 10⁻³²); the true value is cos c/c² + 2 sin c/c³ ≈ -8.86 × 10⁻³⁵.
    assert_eq!(
        report(r"$$ \int_{10^{17}}^{\infty} \frac{\sin x}{x^2} dx = ? $$"),
        "∫_(10¹⁷)^∞ (sin x)/x² dx ≈ -8.855573283 × 10⁻³⁵"
    );
    assert_eq!(
        report(r"$$ \int_{10^{17}}^{\infty} \frac{\cos x}{x^2} dx = ? $$"),
        "∫_(10¹⁷)^∞ (cos x)/x² dx ≈ 4.645301048 × 10⁻³⁵"
    );
}

/// The query lines of `src`, one report line each.
fn reports(src: &[&str]) -> Vec<String> {
    src.iter().map(|s| report(s)).collect()
}

#[test]
fn an_integrand_that_lives_far_out_is_sampled_at_its_own_scale() {
    // The map x = a + t/(1 − t) never sampled these where they matter, and the absolute
    // tolerance accepted the noise: 1.0958 × 10⁻¹¹ for 1.5708 × 10⁻⁷ and 6.98 × 10⁻¹² for
    // 10⁻⁷. The values come from mpmath at 50 digits.
    let got = reports(&[
        r"$$ \int_{10^{7}}^{\infty} \frac{1}{x^2+\ln x} dx = ? $$",
        r"$$ \int_{0}^{\infty} \frac{1}{(x+10^{7})^2+\ln(x+1)} dx = ? $$",
        r"$$ \int_{10^{7}}^{\infty} \frac{e^{-1/x}}{x^2+\ln x} dx = ? $$",
    ]);
    assert_eq!(
        got,
        [
            "∫_(10⁷)^∞ 1/(x²+ln x) dx ≈ 0.0000001",
            "∫₀^∞ 1/((x+10⁷)²+ln(x+1)) dx ≈ 0.0000001",
            "∫_(10⁷)^∞ (e^(-1/x))/(x²+ln x) dx ≈ 9.9999995 × 10⁻⁸",
        ]
    );
}

#[test]
fn an_improper_integral_with_a_closed_form_takes_the_limit_of_it() {
    // F = ln|x| − ln(x² + 1)/2 − arctan(x)/x → 0 at ∞ once the logarithms are combined:
    // the value is arctan(10⁷)/10⁷ + ln(1 + 10⁻¹⁴)/2 ≈ 1.5707962768 × 10⁻⁷.
    assert_eq!(
        report(r"$$ \int_{10^{7}}^{\infty} \frac{\arctan x}{x^2} dx = ? $$"),
        "∫_(10⁷)^∞ (arctan x)/x² dx = atan(10000000)/10000000 - ln(10000000) + ln(100000000000001)/2"
    );
    // x eˣ cos x → 0 as x → −∞ although cos x has no limit (this used to "diverge").
    assert_eq!(
        report(r"$$ \int_{-\infty}^{0} x e^{x} \cos x dx = ? $$"),
        "∫_(-∞)⁰ x e^x cos x dx = 0"
    );
}

#[test]
fn oscillating_integrals_far_out_follow_the_expansion_in_every_direction() {
    // Each was rounding noise before (−1.135 × 10⁻¹² for −9.07 × 10⁻¹⁵, a wrong sign at
    // −10⁸, 6.6 × 10⁻³² for 8.86 × 10⁻³⁵, …). The values come from mpmath at 50 digits.
    let got = reports(&[
        r"$$ \int_{10^{7}}^{\infty} \frac{\sin x}{x^2} dx = ? $$",
        r"$$ \int_{4 \cdot 10^{7}}^{\infty} \frac{\sin x}{x^2} dx = ? $$",
        r"$$ \int_{10^{6}}^{\infty} \frac{\sin x}{x^2} dx = ? $$",
        r"$$ \int_{10^{7}}^{\infty} \frac{\cos x}{x^{3/2}} dx = ? $$",
        r"$$ \int_{10^{9}}^{10^{10}} \frac{\cos x}{x^2} dx = ? $$",
        r"$$ \int_{10^{14}}^{10^{15}} \frac{\cos x}{x^2} dx = ? $$",
        r"$$ \int_{10^{17}}^{10^{18}} \frac{\cos x}{x^2} dx = ? $$",
        r"$$ \int_{-\infty}^{-10^{17}} \frac{\sin x}{x^2} dx = ? $$",
        r"$$ \int_{-\infty}^{-10^{17}} \frac{\cos x}{x^2} dx = ? $$",
        r"$$ \int_{-\infty}^{-10^{8}} \frac{\sin x}{x^2} dx = ? $$",
        r"$$ \int_{10^{17}}^{10^{17}+1000} \frac{\sin x}{x^2} dx = ? $$",
        r"$$ \int_{10^{17}}^{2 \cdot 10^{17}} \frac{\sin x}{x^2} dx = ? $$",
        r"$$ \int_{10^{17}}^{\infty} \frac{\sin(2x+1)}{x^2} dx = ? $$",
    ]);
    assert_eq!(
        got,
        [
            "∫_(10⁷)^∞ (sin x)/x² dx ≈ -9.072703021 × 10⁻¹⁵",
            "∫_(4 · 10⁷)^∞ (sin x)/x² dx ≈ -1.029041752 × 10⁻¹⁶",
            "∫_(10⁶)^∞ (sin x)/x² dx ≈ 9.367514275 × 10⁻¹³",
            "∫_(10⁷)^∞ (cos x)/(x^(3/2)) dx ≈ -1.329889322 × 10⁻¹¹",
            "∫_(10⁹)^(10¹⁰) (cos x)/x² dx ≈ -5.50718508 × 10⁻¹⁹",
            "∫_(10¹⁴)^(10¹⁵) (cos x)/x² dx ≈ 2.179910354 × 10⁻²⁹",
            "∫_(10¹⁷)^(10¹⁸) (cos x)/x² dx ≈ 4.546004116 × 10⁻³⁵",
            "∫_(-∞)^(-10¹⁷) (sin x)/x² dx ≈ 8.855573283 × 10⁻³⁵",
            "∫_(-∞)^(-10¹⁷) (cos x)/x² dx ≈ 4.645301048 × 10⁻³⁵",
            "∫_(-∞)^(-10⁸) (sin x)/x² dx ≈ 3.633850707 × 10⁻¹⁷",
            "∫_(10¹⁷)^(10¹⁷+1000) (sin x)/x² dx ≈ -7.716488557 × 10⁻³⁵",
            "∫_(10¹⁷)^(2 · 10¹⁷) (sin x)/x² dx ≈ -1.027663219 × 10⁻³⁴",
            "∫_(10¹⁷)^∞ (sin(2x+1))/x² dx ≈ -1.925939875 × 10⁻³⁵",
        ]
    );
}

#[test]
fn other_oscillations_too_far_out_to_sample_are_refused() {
    let r = report(r"$$ \int_{-10^{17}}^{\infty} \frac{\sin x}{x^2+1} dx = ? $$");
    assert!(
        r.contains("could not determine") && r.contains("oscillates"),
        "{r}"
    );
}

#[test]
fn a_trigonometric_function_of_a_shrinking_argument_does_not_oscillate() {
    // sin(1/x) far out is smooth: computed (≈ 10⁻⁹ and so on, from mpmath), not refused
    // as oscillating.
    let got = reports(&[
        r"$$ \int_{10^{9}}^{\infty} \sin^2(1/x) dx = ? $$",
        r"$$ \int_{10^{9}}^{\infty} \frac{\sin(1/x)}{x^{3/2}} dx = ? $$",
        r"$$ \int_{10^{9}}^{\infty} \frac{\cos(1/x)}{x^2 \ln x} dx = ? $$",
    ]);
    assert_eq!(
        got,
        [
            "∫_(10⁹)^∞ sin²(1/x) dx ≈ 1 × 10⁻⁹",
            "∫_(10⁹)^∞ (sin(1/x))/(x^(3/2)) dx ≈ 2.108185107 × 10⁻¹⁴",
            "∫_(10⁹)^∞ (cos(1/x))/(x² ln x) dx ≈ 4.612369258 × 10⁻¹¹",
        ]
    );
}
