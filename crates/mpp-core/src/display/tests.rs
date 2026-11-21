use proptest::prelude::*;

use super::*;
use crate::expr::Expr;
use crate::number::Number;

fn text(n: &Number, f: &NumberFormat) -> String {
    format_number(n, f).0
}

fn latex(n: &Number, f: &NumberFormat) -> String {
    format_number(n, f).1
}

fn fl(x: f64) -> Number {
    Number::float(x)
}

fn ratio(p: i64, q: i64) -> Number {
    Number::rational(p, q).unwrap()
}

fn sf(n: u32) -> NumberFormat {
    NumberFormat {
        precision: Precision::Sig(n),
        ..NumberFormat::default()
    }
}

fn dp(n: u32) -> NumberFormat {
    NumberFormat {
        precision: Precision::Places(n),
        ..NumberFormat::default()
    }
}

fn decimal() -> NumberFormat {
    NumberFormat {
        style: Style::Decimal,
        ..NumberFormat::default()
    }
}

#[test]
fn defaults_match_the_plain_printer() {
    let d = NumberFormat::default();
    for x in [0.5, -2.25, 1234.5678, 1e-9, 3.0] {
        let plain = fl(x).to_string().replace('e', " × 10^");
        let ours = text(&fl(x), &d);
        if x.abs() < 1e-6 {
            assert!(ours.contains('×') && plain.contains('×'), "{ours}");
        } else {
            assert_eq!(ours, fl(x).to_string());
        }
    }
    assert_eq!(text(&ratio(7, 2), &d), "7/2");
    assert_eq!(latex(&ratio(-7, 2), &d), "-\\frac{7}{2}");
}

#[test]
fn significant_figures_keep_zeros() {
    assert_eq!(text(&fl(3.7654321), &sf(4)), "3.765");
    assert_eq!(text(&fl(2.5), &sf(4)), "2.500");
    assert_eq!(text(&fl(1234.5678), &sf(2)), "1200");
    assert_eq!(text(&fl(0.000123456), &sf(3)), "0.000123");
    assert_eq!(text(&fl(-9.999), &sf(3)), "-10.0");
}

#[test]
fn decimal_places() {
    assert_eq!(text(&fl(3.76543), &dp(2)), "3.77");
    assert_eq!(text(&fl(2.0), &dp(3)), "2.000");
    assert_eq!(text(&fl(-0.001), &dp(2)), "0.00");
    assert_eq!(text(&fl(1234.5), &dp(0)), "1235");
    assert_eq!(text(&fl(0.0), &dp(2)), "0.00");
    assert_eq!(text(&fl(-2.555), &dp(1)), "-2.6");
}

#[test]
fn exact_and_decimal() {
    assert_eq!(text(&ratio(1, 3), &NumberFormat::default()), "1/3");
    assert_eq!(text(&ratio(1, 3), &decimal()), "0.3333333333");
    assert_eq!(
        text(
            &ratio(1, 3),
            &NumberFormat {
                style: Style::Decimal,
                precision: Precision::Sig(4),
                ..NumberFormat::default()
            }
        ),
        "0.3333"
    );
    assert_eq!(text(&Number::int(7), &decimal()), "7");
    assert_eq!(
        text(
            &Number::int(7),
            &NumberFormat {
                precision: Precision::Places(2),
                ..decimal()
            }
        ),
        "7.00"
    );
    assert_eq!(text(&ratio(-1, 8), &decimal()), "-0.125");
}

#[test]
fn scientific_notation() {
    let sci = NumberFormat::default().scientific(4, 3);
    assert_eq!(text(&fl(6.02e23), &sci), "6.02 × 10²³");
    assert_eq!(latex(&fl(6.02e23), &sci), "6.02 \\times 10^{23}");
    assert_eq!(text(&fl(0.00012), &sci), "1.2 × 10⁻⁴");
    assert_eq!(text(&fl(1234.0), &sci), "1234");
    assert_eq!(text(&fl(12345.0), &sci), "1.2345 × 10⁴");
    assert_eq!(text(&fl(-12345.0), &sci), "-1.2345 × 10⁴");
    // Exact integers keep all their digits.
    let big = Number::from_bigint("602000000000000000000000".parse().unwrap());
    assert_eq!(text(&big, &sci), "6.02 × 10²³");
    assert_eq!(
        text(&big, &NumberFormat::default()),
        "602000000000000000000000"
    );
    // With a precision, the mantissa follows it.
    let sci3 = NumberFormat {
        precision: Precision::Sig(3),
        ..sci
    };
    assert_eq!(text(&fl(6.0e23), &sci3), "6.00 × 10²³");
    let sci_dp = NumberFormat {
        precision: Precision::Places(1),
        ..sci
    };
    assert_eq!(text(&fl(6.0221e23), &sci_dp), "6.0 × 10²³");
}

#[test]
fn engineering_notation() {
    let eng = NumberFormat::default().engineering(4, 3);
    assert_eq!(text(&fl(12345.0), &eng), "12.345 × 10³");
    assert_eq!(text(&fl(6.02e23), &eng), "602 × 10²¹");
    assert_eq!(text(&fl(0.00012), &eng), "120 × 10⁻⁶");
    assert_eq!(latex(&fl(0.00012), &eng), "120 \\times 10^{-6}");
    assert_eq!(text(&fl(2.0e9), &eng), "2 × 10⁹");
}

#[test]
fn automatic_thresholds_match_the_old_printer() {
    let d = NumberFormat::default();
    assert_eq!(text(&fl(1e20), &d), "100000000000000000000");
    assert_eq!(text(&fl(1e21), &d), "1 × 10²¹");
    assert_eq!(text(&fl(1e-7), &d), "0.0000001");
    assert_eq!(text(&fl(1e-8), &d), "1 × 10⁻⁸");
}

#[test]
fn mixed_fractions() {
    let mixed = NumberFormat {
        fractions: Fractions::Mixed,
        ..NumberFormat::default()
    };
    assert_eq!(text(&ratio(7, 2), &mixed), "3 1/2");
    assert_eq!(latex(&ratio(7, 2), &mixed), "3\\frac{1}{2}");
    assert_eq!(text(&ratio(-7, 2), &mixed), "-3 1/2");
    assert_eq!(text(&ratio(1, 2), &mixed), "1/2");
    assert_eq!(text(&Number::int(4), &mixed), "4");
}

#[test]
fn complex_forms() {
    let z = Number::complex_float(3.0, 4.0);
    let rect = NumberFormat::default();
    assert_eq!(text(&z, &rect), "3 + 4*i");
    assert_eq!(latex(&z, &rect), "3 + 4 i");
    let polar = NumberFormat {
        complex: ComplexForm::Polar,
        ..rect
    };
    assert_eq!(text(&z, &polar), "5∠0.927295218");
    assert_eq!(latex(&z, &polar), "5 e^{0.927295218 i}");
    let polar_deg = NumberFormat {
        angle: AngleUnit::Degrees,
        precision: Precision::Places(2),
        ..polar
    };
    assert_eq!(text(&z, &polar_deg), "5.00∠53.13°");
    assert_eq!(latex(&z, &polar_deg), "5.00\\angle 53.13^\\circ");
    let i = Number::i();
    assert_eq!(text(&i, &polar_deg), "1.00∠90.00°");
    assert_eq!(text(&Number::complex_float(0.0, -2.0), &rect), "-2*i");
    let br = |p: i64, q: i64| num_rational::BigRational::new(p.into(), q.into());
    let exact = Number::complex(br(1, 1), br(-1, 1));
    assert_eq!(text(&exact, &rect), "1 - i");
    assert_eq!(text(&exact, &decimal()), "1 - i");
    assert_eq!(
        text(&Number::complex(br(1, 2), br(1, 4)), &decimal()),
        "0.5 + 0.25*i"
    );
}

#[test]
fn active_format_changes_the_printers() {
    let e = Expr::rational(7, 2) + Expr::sym("x");
    let mixed = NumberFormat {
        fractions: Fractions::Mixed,
        ..NumberFormat::default()
    };
    assert_eq!(e.to_string(), "x + 7/2");
    let shown = with_format(&mixed, || e.to_string());
    assert_eq!(shown, "x + 3 1/2");
    assert_eq!(e.to_string(), "x + 7/2");
    assert!(active().is_none());
    let tiny = Expr::new(crate::ExprNode::Num(fl(6.02e23)));
    let s = NumberFormat::default().scientific(4, 3);
    assert_eq!(with_format(&s, || tiny.to_latex()), "6.02 \\times 10^{23}");
    assert_eq!(with_format(&s, || tiny.to_string()), "6.02 × 10²³");
}

#[test]
fn format_nests_and_restores() {
    let a = sf(3);
    let b = sf(5);
    with_format(&a, || {
        with_format(&b, || assert_eq!(active(), Some(b)));
        assert_eq!(active(), Some(a));
    });
    assert_eq!(active(), None);
}

#[test]
fn degrees_of_angles() {
    let half_pi = Expr::pi() / Expr::int(2);
    assert_eq!(to_degrees(&half_pi).unwrap().to_string(), "90");
    let sixth = Expr::pi() / Expr::int(6);
    assert_eq!(to_degrees(&sixth).unwrap().to_string(), "30");
    assert!(to_degrees(&Expr::sym("x")).is_none());
    let one = to_degrees(&Expr::int(1)).unwrap();
    assert!(one.to_string().starts_with("57.29577951"), "{one}");
}

#[test]
fn approximate_answers_follow_the_format() {
    let z = Complex64::new(std::f64::consts::PI, 0.0);
    assert_eq!(format_approx(z, 10), None);
    assert_eq!(
        with_format(&sf(4), || format_approx(z, 10)).as_deref(),
        Some("3.142")
    );
    assert_eq!(
        with_format(&NumberFormat::default(), || format_approx(z, 10)).as_deref(),
        Some("3.141592654")
    );
    assert_eq!(
        with_format(&dp(3), || format_approx(Complex64::new(2.0, 1e-20), 10)).as_deref(),
        Some("2.000")
    );
}

/// Counts the digits of a plain decimal without leading zeros.
fn significant_digits(s: &str) -> usize {
    let digits: String = s.chars().filter(char::is_ascii_digit).collect();
    digits.trim_start_matches('0').len()
}

proptest! {
    #[test]
    fn sig_figs_are_correct(x in 1e-5f64..1e15, n in 1u32..12) {
        let s = text(&fl(x), &sf(n));
        // Round trip within the stated precision.
        let back: f64 = s.parse().unwrap();
        prop_assert!(((back - x) / x).abs() <= 0.5 * 10f64.powi(1 - n as i32) * 1.0000001, "{s} vs {x}");
        // Exactly n digits unless trailing integer zeros were padded.
        let digits = significant_digits(&s);
        if s.contains('.') {
            prop_assert_eq!(digits, n as usize, "{}", s);
        } else {
            prop_assert!(digits >= n as usize || x >= 10f64.powi(n as i32 - 1), "{}", s);
        }
    }

    #[test]
    fn decimal_places_are_correct(x in -1e9f64..1e9, p in 0u32..10) {
        let s = text(&fl(x), &dp(p));
        let frac = s.split_once('.').map_or(0, |(_, f)| f.len());
        // A float shows at most FLOAT_DIGITS significant digits, so big values get fewer
        // places than asked for rather than digits that are not known.
        let lead = if x == 0.0 { 1 } else { x.abs().log10().floor() as i64 + 1 };
        let want = (p as i64).min((super::FLOAT_DIGITS as i64 - lead).max(0));
        prop_assert_eq!(frac as i64, want, "{}", s);
        let back: f64 = s.parse().unwrap();
        prop_assert!((back - x).abs() <= 0.5 * 10f64.powi(-(want as i32)) * 1.000001 + x.abs() * 1e-15, "{s} vs {x}");
    }

    #[test]
    fn scientific_round_trips(m in 1.0f64..10.0, e in -30i32..30, n in 2u32..10) {
        let x = m * 10f64.powi(e);
        let f = NumberFormat { precision: Precision::Sig(n), ..NumberFormat::default().scientific(0, 0) };
        let s = text(&fl(x), &f);
        let (mant, exp) = s.split_once(" × 10").unwrap();
        prop_assert_eq!(significant_digits(mant), n as usize, "{}", s);
        let exp: String = exp.chars().map(|c| match c {
            '⁻' => '-', '⁰' => '0', '¹' => '1', '²' => '2', '³' => '3', '⁴' => '4',
            '⁵' => '5', '⁶' => '6', '⁷' => '7', '⁸' => '8', '⁹' => '9', o => o }).collect();
        let back: f64 = format!("{mant}e{exp}").parse().unwrap();
        prop_assert!(((back - x) / x).abs() <= 0.5 * 10f64.powi(1 - n as i32) * 1.0001, "{s} vs {x}");
        // LaTeX carries the same digits.
        let l = latex(&fl(x), &f);
        prop_assert!(l.contains("\\times 10^{"), "{}", l);
    }

    #[test]
    fn engineering_exponents_are_multiples_of_three(m in 1.0f64..10.0, e in -30i32..30) {
        let x = m * 10f64.powi(e);
        let f = NumberFormat::default().engineering(0, 0);
        let l = latex(&fl(x), &f);
        let exp: i32 = l.split("^{").nth(1).unwrap().trim_end_matches('}').parse().unwrap();
        prop_assert_eq!(exp.rem_euclid(3), 0, "{}", l);
        let (mant, _) = l.split_once(" \\times").unwrap();
        let mv: f64 = mant.parse().unwrap();
        prop_assert!((1.0..1000.0).contains(&mv), "{}", l);
    }

    #[test]
    fn exact_rationals_round_trip(p in -10_000i64..10_000, q in 1i64..1000) {
        let n = ratio(p, q);
        let mixed = NumberFormat { fractions: Fractions::Mixed, ..NumberFormat::default() };
        let s = text(&n, &mixed);
        // Re-read "w n/d" and compare.
        let neg = s.starts_with('-');
        let body = s.trim_start_matches('-');
        let (whole, frac) = match body.split_once(' ') {
            Some((w, f)) => (w.parse::<i64>().unwrap(), f),
            None => (0, body),
        };
        let value = match frac.split_once('/') {
            Some((a, b)) => whole as f64 + a.parse::<f64>().unwrap() / b.parse::<f64>().unwrap(),
            None => frac.parse::<f64>().unwrap(),
        };
        let signed = if neg { -value } else { value };
        prop_assert!((signed - p as f64 / q as f64).abs() < 1e-9, "{s}");
    }
}
