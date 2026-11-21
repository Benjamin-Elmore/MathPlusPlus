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

/// The error messages of a document.
fn errors(src: &str) -> Vec<String> {
    run(src)
        .errors()
        .iter()
        .map(|d| d.message.clone())
        .collect()
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
fn slowly_converging_sums_give_confirmed_digits() {
    let rows = [
        (
            r"$$ \sum_{n=1}^{\infty} \frac{1}{n^{1.0001}} \approx ? $$",
            "10000.57722",
        ),
        (
            r"$$ \sum_{n=1}^{\infty} \frac{1}{n^{1.01}} \approx ? $$",
            "100.5779433",
        ),
        (
            r"$$ \sum_{n=1}^{\infty} \frac{1}{n^{1.1}} \approx ? $$",
            "10.58444846",
        ),
        (
            r"$$ \sum_{n=1}^{\infty} \frac{1}{n^{11/10}} \approx ? $$",
            "10.58444846",
        ),
        (
            r"$$ \sum_{n=1}^{\infty} \frac{1}{\sqrt{n^3}} \approx ? $$",
            "2.612375349",
        ),
        (
            r"$$ \sum_{n=1}^{\infty} \frac{\cos(n\pi)}{n} \approx ? $$",
            "-0.6931471806",
        ),
        (
            r"$$ \sum_{n=2}^{\infty} \frac{1}{n^{3/2}} \approx ? $$",
            "1.612375349",
        ),
        (
            r"$$ \sum_{n=1}^{\infty} \frac{2}{(2n+1)^{3/2}} \approx ? $$",
            "1.377522373",
        ),
    ];
    for (src, want) in rows {
        assert_eq!(approx(src), want, "{src}");
    }
}

#[test]
fn a_sum_too_slow_to_confirm_is_refused_not_guessed() {
    // Σ 1/(n (ln n)^1.1) = 10.82119596…: shown right, or refused, never wrong digits.
    let src = r"$$ \sum_{n=2}^{\infty} \frac{1}{n (\ln n)^{1.1}} \approx ? $$";
    let ev = run(src);
    let (_, line) = ev.queries().next().expect("a query");
    match &line.approx {
        Some(a) => assert_eq!(a, "10.82119596", "{src}"),
        None => assert!(
            errors(src).iter().any(|e| e.contains("too slowly")),
            "{:?}",
            errors(src)
        ),
    }
}

#[test]
fn improper_integrals_of_bounded_integrands_are_computed() {
    // x/(e^x − 1) → 1 at 0 and overflows harmlessly to 0 far out.
    let rows = [
        (
            r"$$ \int_0^{\infty} \frac{x}{e^x - 1} \, dx \approx ? $$",
            "1.644934067",
        ),
        (
            r"$$ \int_0^{\infty} \frac{x^3}{e^x - 1} \, dx \approx ? $$",
            "6.493939402",
        ),
        (
            r"$$ \int_0^{\infty} \frac{x}{e^x + 1} \, dx \approx ? $$",
            "0.8224670334",
        ),
        (
            r"$$ \int_1^{\infty} \frac{x}{e^x - 1} \, dx \approx ? $$",
            "0.8674294327",
        ),
    ];
    for (src, want) in rows {
        assert_eq!(approx(src), want, "{src}");
    }
}

#[test]
fn a_numeric_integral_below_its_error_is_zero_not_noise() {
    assert_eq!(
        approx(r"$$ \int_0^{\infty} \frac{\ln x}{1 + x^2} \, dx \approx ? $$"),
        "0"
    );
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

/// Every printed digit of a 30-place answer is right: special functions, integrals,
/// sums and powers with tiny bases all go through the high-precision evaluator (or show
/// only the digits they know). References: mpmath at 60 digits, rounded half up.
#[test]
fn thirty_places_are_all_correct() {
    let rows = [
        (r"\pi", "3.141592653589793238462643383280"),
        (r"e", "2.718281828459045235360287471353"),
        (r"\sqrt{2}", "1.414213562373095048801688724210"),
        (r"\ln 2", "0.693147180559945309417232121458"),
        (r"\sin(1)", "0.841470984807896506652502321630"),
        (r"\cos(1)", "0.540302305868139717400936607443"),
        (r"\tan(1)", "1.557407724654902230506974807458"),
        (r"\Gamma(1/3)", "2.678938534707747633655692940975"),
        (r"\Gamma(1/4)", "3.625609908221908311930685155868"),
        (r"\zeta(3)", "1.202056903159594285399738161511"),
        (r"\zeta(3/2)", "2.612375348685488343348567567924"),
        (r"\zeta(1.1)", "10.584448464950809826386400791736"),
        (r"\zeta(5)", "1.036927755143369926331365486457"),
        (r"\operatorname{erf}(1)", "0.842700792949714869341220635083"),
        (
            r"\operatorname{erf}(1/2)",
            "0.520499877813046537682746653892",
        ),
        (
            r"\operatorname{erfc}(2)",
            "0.004677734981047265837930743633",
        ),
        (r"\psi(1/3)", "-3.132033780020806322996419074287"),
        (r"\psi(5/7)", "-1.180181440339498754844677720446"),
        (r"J_0(1)", "0.765197686557966551449717526103"),
        (r"J_1(2)", "0.576724807756873387202448242269"),
        (
            r"\int_0^1 e^{-x^2} \, dx",
            "0.746824132812427025399467436132",
        ),
        (
            r"e^{\pi \sqrt{163}}",
            "262537412640768743.999999999999250072597198185689",
        ),
        (r"\sinh(1)", "1.175201193643801456882381850596"),
        (r"\cosh(2)", "3.762195691083631459562213477774"),
        (r"\tanh(1/2)", "0.462117157260009758502318483644"),
        (
            r"\operatorname{asinh}(1)",
            "0.881373587019543025232609324980",
        ),
        (r"\ln 10", "2.302585092994045684017991454684"),
        (r"2^{\sqrt{2}}", "2.665144142690225188650297249873"),
        (r"\sqrt{2}^{\sqrt{2}}", "1.632526919438152844773495381025"),
        (r"\arcsin(1/3)", "0.339836909454121937096392513392"),
        (r"\arccos(1/3)", "1.230959417340774682134929178248"),
        (r"\arctan(1/3)", "0.321750554396642193401404614359"),
        (r"\operatorname{Si}(1)", "0.946083070367183014941353313823"),
        (r"\operatorname{Ci}(1)", "0.337403922900968134662646203889"),
        (
            r"\sum_{n=1}^{\infty} \frac{1}{n^2}",
            "1.644934066848226436472415166646",
        ),
        (
            r"\sum_{n=1}^{\infty} \frac{1}{n^{1.5}}",
            "2.612375348685488343348567567924",
        ),
        (r"\cos(1000!)", "0.231645130794935182503707040329"),
        (
            r"(1 + 10^{-20})^{10^{20}}",
            "2.718281828459045235346696062210",
        ),
        (
            r"(1 - 10^{-20})^{10^{20}}",
            "0.367879441171442321593684372956",
        ),
        (
            r"10^{20} \ln(1 + 10^{-20})",
            "0.999999999999999999995000000000",
        ),
        (r"\Gamma(3/10)", "2.991568987687590628312516515905"),
        (r"e^{e}", "15.154262241479264189760430272630"),
    ];
    assert!(rows.len() >= 40);
    for (expr, want) in rows {
        let src = format!("%dp 30\n$$ {expr} \\approx ? $$");
        assert_eq!(approx(&src), want, "{expr}");
    }
}

#[test]
fn powers_of_bases_near_one_keep_their_digits() {
    let rows = [
        (r"$$ (1 + 10^{-16})^{10^{16}} \approx ? $$", "2.718281828"),
        (r"$$ (1 + 10^{-20})^{10^{20}} \approx ? $$", "2.718281828"),
        (r"$$ (1 - 10^{-20})^{10^{20}} \approx ? $$", "0.3678794412"),
        (
            r"$$ (1 + 10^{-20})^{2 \cdot 10^{20}} \approx ? $$",
            "7.389056099",
        ),
        (
            r"$$ \exp(10^{20} \ln(1 + 10^{-20})) \approx ? $$",
            "2.718281828",
        ),
    ];
    for (src, want) in rows {
        assert_eq!(approx(src), want, "{src}");
    }
    let v = verdict(r"$$ (1 + 10^{-17})^{10^{17}} \overset{?}{=} 1 $$");
    assert_eq!(v.mark(), "✗", "{}", v.describe());
}

/// A special-function value is compared to all its digits: a decimal wrong from the
/// 16th digit on is ✗, never "proved".
#[test]
fn special_function_values_are_not_matched_by_wrong_decimals() {
    let rows = [
        r"$$ \zeta(3) \overset{?}{=} 1.202056903159593348462408357591 $$",
        r"$$ \zeta(3/2) \overset{?}{=} 2.612375348685489484523714054376 $$",
        r"$$ \zeta(1.5) \overset{?}{=} 2.612375348685489484523714054376 $$",
        r"$$ J_0(1) \overset{?}{=} 0.765197686557966494369509291573 $$",
        r"$$ \operatorname{erf}(1) \overset{?}{=} 0.842700792949715005164534886717 $$",
        r"$$ \int_0^1 e^{-x^2} \, dx \overset{?}{=} 0.746824132812427099459284818295 $$",
        r"$$ \psi(1/3) \overset{?}{=} -3.132033780020815338929196514073 $$",
    ];
    for src in rows {
        let v = verdict(src);
        assert_eq!(v.mark(), "✗", "{src}: {}", v.describe());
    }
    // Cancellation against a 30-digit value leaves the true tiny difference.
    assert_eq!(
        approx(r"$$ \zeta(3) - 1.202056903159594285399738161511 \approx ? $$"),
        "4.49990765 × 10⁻³¹"
    );
}

/// A value only known as a float (a numeric integral) never shows more digits than it
/// carries, however many places are asked for.
#[test]
fn float_values_show_only_known_digits() {
    let shown = approx("%dp 30\n$$ \\int_0^1 x^x \\, dx \\approx ? $$");
    let digits = shown.chars().filter(char::is_ascii_digit).count();
    assert!(shown.starts_with("0.78343051"), "{shown}");
    assert!(digits <= 15, "{shown}");
}

/// Huge angles are reduced with enough digits of π, quickly; angles too big to reduce
/// are refused with a reason, never answered 0 and never left running.
#[test]
fn huge_angles_are_reduced_or_refused_quickly() {
    let start = std::time::Instant::now();
    let rows = [
        (r"$$ \cos(1000!) \approx ? $$", "0.2316451308"),
        (r"$$ \sin(10^{1000}) \approx ? $$", "0.6533597982"),
        (r"$$ \sin(2^{100000}) \approx ? $$", "-0.3971856031"),
    ];
    for (src, want) in rows {
        assert_eq!(approx(src), want, "{src}");
    }
    for src in [
        r"$$ \cos(30000!) \approx ? $$",
        r"$$ \cos(100000!) \approx ? $$",
        r"$$ \sin(2^{1000000}) \approx ? $$",
    ] {
        let errs = errors(src);
        assert!(
            errs.iter().any(|e| e.contains("too large")),
            "{src}: {errs:?}"
        );
    }
    assert!(start.elapsed().as_secs() < 60, "took {:?}", start.elapsed());
}
