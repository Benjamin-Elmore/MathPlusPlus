//! Axis ticks on a 1-2-5 grid, and number formatting for tick labels.

use std::f64::consts::PI;

/// A step of the form 1, 2 or 5 times a power of ten that gives about `target` ticks.
pub fn nice_step(span: f64, target: usize) -> f64 {
    if !(span.is_finite() && span > 0.0) {
        return 1.0;
    }
    let raw = span / target.max(1) as f64;
    let mag = 10f64.powf(raw.log10().floor());
    let f = raw / mag;
    let nice = if f < 1.5 {
        1.0
    } else if f < 3.5 {
        2.0
    } else if f < 7.5 {
        5.0
    } else {
        10.0
    };
    nice * mag
}

/// Tick positions inside `[lo, hi]`, multiples of a 1-2-5 step, about `target` of them.
/// Returns the step too.
pub fn ticks(lo: f64, hi: f64, target: usize) -> (Vec<f64>, f64) {
    if !(lo.is_finite() && hi.is_finite()) || hi <= lo {
        return (Vec::new(), 1.0);
    }
    let step = nice_step(hi - lo, target);
    let first = (lo / step).ceil();
    let last = (hi / step).floor();
    let count = (last - first + 1.0).max(0.0);
    if count > 200.0 {
        return (Vec::new(), step);
    }
    let mut out = Vec::with_capacity(count as usize);
    let mut k = first;
    while k <= last {
        // Multiply rather than accumulate so ticks stay exact multiples of the step.
        let v = k * step;
        out.push(if v.abs() < step * 1e-9 { 0.0 } else { v });
        k += 1.0;
    }
    (out, step)
}

/// Rounds `[lo, hi]` outwards to multiples of the tick step.
pub fn nice_bounds(lo: f64, hi: f64, target: usize) -> (f64, f64) {
    if !(lo.is_finite() && hi.is_finite()) {
        return (-1.0, 1.0);
    }
    if hi <= lo {
        let d = if lo == 0.0 { 1.0 } else { lo.abs() * 0.1 };
        return (lo - d, hi + d);
    }
    let step = nice_step(hi - lo, target);
    ((lo / step).floor() * step, (hi / step).ceil() * step)
}

/// Superscript digits for scientific notation.
fn superscript(n: i32) -> String {
    const DIGITS: [char; 10] = ['⁰', '¹', '²', '³', '⁴', '⁵', '⁶', '⁷', '⁸', '⁹'];
    let mut s = String::new();
    if n < 0 {
        s.push('⁻');
    }
    for ch in n.unsigned_abs().to_string().chars() {
        if let Some(d) = ch.to_digit(10) {
            s.push(DIGITS[d as usize]);
        }
    }
    s
}

/// Formats a tick value for a grid with spacing `step`: no more decimals than the step
/// needs, a true minus sign, and scientific notation for very large or small values.
pub fn format_tick(v: f64, step: f64) -> String {
    if !v.is_finite() {
        return String::new();
    }
    if v == 0.0 {
        return "0".into();
    }
    let a = v.abs();
    if a >= 1e6 || a < 1e-4 {
        let exp = a.log10().floor() as i32;
        let mant = v / 10f64.powi(exp);
        let m = trim_zeros(&format!("{mant:.3}"));
        let body = if m == "1" || m == "-1" {
            format!("{}10{}", if v < 0.0 { "−" } else { "" }, superscript(exp))
        } else {
            format!("{}×10{}", m.replace('-', "−"), superscript(exp))
        };
        return body;
    }
    let decimals = if step.is_finite() && step > 0.0 {
        (-step.log10().floor()).max(0.0) as usize
    } else {
        0
    }
    .min(8);
    trim_zeros(&format!("{v:.decimals$}")).replace('-', "−")
}

fn trim_zeros(s: &str) -> String {
    if s.contains('.') {
        s.trim_end_matches('0').trim_end_matches('.').to_string()
    } else {
        s.to_string()
    }
}

/// Ticks at multiples of π/2 (or π when the range is wide) with labels such as `π/2`.
/// Returns `None` when the range is not a sensible number of half-turns.
pub fn pi_ticks(lo: f64, hi: f64) -> Option<Vec<(f64, String)>> {
    if !(lo.is_finite() && hi.is_finite()) || hi <= lo {
        return None;
    }
    let span = (hi - lo) / PI;
    if !(0.9..=24.0).contains(&span) {
        return None;
    }
    let unit = if span <= 2.5 {
        0.5
    } else if span <= 10.0 {
        1.0
    } else {
        2.0
    };
    let first = (lo / PI / unit - 1e-9).ceil() as i64;
    let last = (hi / PI / unit + 1e-9).floor() as i64;
    let mut out = Vec::new();
    for k in first..=last {
        let halves = (k as f64 * unit * 2.0).round() as i64;
        out.push((k as f64 * unit * PI, pi_label(halves)));
    }
    Some(out)
}

/// The label for `halves` times π/2.
fn pi_label(halves: i64) -> String {
    if halves == 0 {
        return "0".into();
    }
    let sign = if halves < 0 { "−" } else { "" };
    let h = halves.abs();
    let body = if h % 2 == 0 {
        match h / 2 {
            1 => "π".to_string(),
            n => format!("{n}π"),
        }
    } else if h == 1 {
        "π/2".to_string()
    } else {
        format!("{h}π/2")
    };
    format!("{sign}{body}")
}

/// True when `v` is within rounding of a multiple of π/2.
pub fn is_half_pi_multiple(v: f64) -> bool {
    let k = v / (PI / 2.0);
    v.is_finite() && (k - k.round()).abs() < 1e-9
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn steps_are_one_two_five() {
        for span in [0.07, 0.3, 1.0, 3.0, 8.0, 17.0, 250.0, 9e5] {
            let step = nice_step(span, 8);
            let m = step / 10f64.powf(step.log10().floor());
            assert!(
                [1.0, 2.0, 5.0].iter().any(|k| (m - k).abs() < 1e-9) || (m - 10.0).abs() < 1e-9,
                "span {span} gave step {step}"
            );
        }
    }

    #[test]
    fn ticks_cover_range_in_order() {
        let (t, step) = ticks(-3.2, 7.9, 8);
        assert!(!t.is_empty());
        assert!(t.windows(2).all(|w| w[0] < w[1]));
        assert!(t.iter().all(|v| (-3.2..=7.9).contains(v)));
        assert_eq!(step, 1.0);
        assert_eq!(t[0], -3.0);
        assert!(t.contains(&0.0));
    }

    #[test]
    fn degenerate_ranges_do_not_panic() {
        assert!(ticks(1.0, 1.0, 8).0.is_empty());
        assert!(ticks(f64::NAN, 1.0, 8).0.is_empty());
        assert!(ticks(0.0, 1e300, 8).0.len() <= 200);
        assert_eq!(nice_bounds(3.0, 3.0, 6).0, 2.7);
    }

    #[test]
    fn tick_labels() {
        assert_eq!(format_tick(0.5, 0.5), "0.5");
        assert_eq!(format_tick(-2.0, 1.0), "−2");
        assert_eq!(format_tick(0.0, 1.0), "0");
        assert_eq!(format_tick(2e6, 1e6), "2×10⁶");
        assert_eq!(format_tick(1e-5, 1e-5), "10⁻⁵");
        assert_eq!(format_tick(0.25, 0.05), "0.25");
    }

    #[test]
    fn pi_labels() {
        let t = pi_ticks(-2.0 * PI, 2.0 * PI).unwrap();
        let labels: Vec<&str> = t.iter().map(|(_, l)| l.as_str()).collect();
        assert_eq!(labels, ["−2π", "−π", "0", "π", "2π"]);
        let t = pi_ticks(0.0, PI).unwrap();
        assert_eq!(t[1].1, "π/2");
        assert!(pi_ticks(0.0, 1.0).is_none());
    }
}
