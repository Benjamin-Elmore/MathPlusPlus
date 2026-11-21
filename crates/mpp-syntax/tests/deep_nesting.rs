//! Deeply nested scripts parse in linear time and report one error, not one per level.

use std::time::{Duration, Instant};

use mpp_syntax::math::parse_math;

fn nest(open: &str, close: char, n: usize) -> String {
    let mut s = open.repeat(n);
    s.push('1');
    s.extend(std::iter::repeat_n(close, n));
    s.push_str(" = ?");
    s
}

#[test]
fn power_towers_parse_quickly_and_report_once() {
    for (open, close) in [("2^{", '}'), ("x_{", '}'), ("2^(", ')'), ("(", ')')] {
        // Two thousand levels used to take about 4 s (cubic time in the unit look-ahead).
        for n in [200, 2_000, 20_000] {
            let src = nest(open, close, n);
            let start = Instant::now();
            let (_, diags) = parse_math(&src, 0);
            let took = start.elapsed();
            assert!(took < Duration::from_secs(3), "{open} x {n}: {took:?}");
            assert!(
                diags.len() <= 1,
                "{open} x {n}: {} diagnostics",
                diags.len()
            );
        }
    }
}

#[test]
fn an_unclosed_nest_reports_one_error() {
    let src = format!("{} = ?", "(".repeat(260));
    let (_, diags) = parse_math(&src, 0);
    assert_eq!(diags.len(), 1, "{diags:?}");
}

#[test]
fn units_after_numbers_still_parse() {
    for src in ["3 m = ?", "9.81m/s^2 = ?", "2kg = ?", "5 km/h = ?"] {
        let (_, diags) = parse_math(src, 0);
        assert!(diags.is_empty(), "{src}: {diags:?}");
    }
}

#[test]
fn a_megabyte_line_is_rejected_before_it_is_tokenised() {
    // 3 million terms used to take 3.6 s and 640 MB before the same error.
    let src = format!("{}1 = ?", "1+".repeat(1_500_000));
    let start = Instant::now();
    let (_, diags) = parse_math(&src, 0);
    assert!(
        start.elapsed() < Duration::from_secs(2),
        "{:?}",
        start.elapsed()
    );
    assert_eq!(diags.len(), 1);
    assert!(diags[0].message.contains("too long"), "{diags:?}");
}
