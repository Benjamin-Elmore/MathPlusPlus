//! Units end to end: quantities in documents, dimension checks pointing at the term that
//! doesn't fit, conversions, units through calculus, and the example worksheet.

use std::sync::Arc;

use mpp_eval::{Evaluated, LineKind, NoImports, Session};

fn run(src: &str) -> Evaluated {
    Session::new()
        .with_importer(Arc::new(NoImports))
        .run_str(src)
        .clone()
}

fn errors(ev: &Evaluated) -> Vec<String> {
    ev.errors()
        .iter()
        .map(|d| mpp_core::note::plain(&d.message))
        .collect()
}

fn answers(src: &str) -> Vec<String> {
    let ev = run(src);
    let errs = errors(&ev);
    assert!(errs.is_empty(), "{src}: unexpected errors {errs:?}");
    ev.queries()
        .map(|(_, l)| l.plain.clone().unwrap_or_else(|| "<none>".into()))
        .collect()
}

fn answer(src: &str) -> String {
    let a = answers(src);
    assert_eq!(a.len(), 1, "{src}: {a:?}");
    a[0].clone()
}

/// The one error of `src` and the source text it points at.
fn failure(src: &str) -> (String, String) {
    let ev = run(src);
    let errs = ev.errors();
    assert_eq!(errs.len(), 1, "{src}: {:?}", errors(&ev));
    let d = errs[0];
    (d.message.clone(), src[d.span.start..d.span.end].to_string())
}

#[test]
fn the_example_worksheet_gives_its_expected_answers() {
    let src = include_str!("../../../examples/units.mpp");
    let expected = include_str!("../../../examples/units.expected.txt");
    let ev = run(src);
    let mut got: Vec<String> = Vec::new();
    for (_, line) in ev.queries() {
        let start = line.span.start;
        // The error of a failing line, or its answer.
        let error = ev
            .errors()
            .into_iter()
            .find(|d| d.span.start >= start && d.span.end <= line.span.end)
            .map(|d| mpp_core::note::plain(&d.message));
        got.push(match error {
            Some(e) => e,
            None => line.plain.clone().unwrap_or_default(),
        });
    }
    let want: Vec<&str> = expected
        .lines()
        .filter_map(|l| l.split_once('→').map(|(_, r)| r.trim()))
        .collect();
    assert_eq!(got, want);
}

#[test]
fn quantities_and_simplified_units() {
    assert_eq!(
        answer(r"$$ 70\,\mathrm{kg} \cdot 9.81\,\mathrm{m/s^2} = ? $$"),
        "686.7 N"
    );
    assert_eq!(answer("$$ 3 m/s \\cdot 4 s = ? $$"), "12 m");
    assert_eq!(answer("$$ 2 kg \\cdot 3 m/s^2 = ? $$"), "6 N");
    assert_eq!(answer("$$ 10 N \\cdot 2 m = ? $$"), "20 J");
    assert_eq!(answer("$$ 100 J / 4 s = ? $$"), "25 W");
    assert_eq!(answer("$$ 1 km + 500 m = ? $$"), "3/2 km");
    assert_eq!(answer("$$ (3 m)^2 = ? $$"), "9 m^2");
    assert_eq!(answer(r"$$ \sqrt{16\,\mathrm{m^2}} = ? $$"), "4 m");
    assert_eq!(answer("$$ 6 m / 2 m = ? $$"), "3");
    assert_eq!(
        answer(r"$$ 20\,{}^\circ\mathrm{C} + 5\,\mathrm{K} = ? $$"),
        "25 °C"
    );
    assert_eq!(
        answer(r"$$ 1.5 \times 10^{3}\,\mathrm{m} \operatorname{in} \mathrm{km} = ? $$"),
        "1.5 km"
    );
    assert_eq!(answer(r"$$ \cos(60^\circ) = ? $$"), "1/2");
    assert_eq!(
        answer(r"$$ 3\,\mathrm{m} < 2\,\mathrm{km} = ? $$"),
        "true ✓"
    );
}

#[test]
fn conversions() {
    assert_eq!(
        answer(r"$$ 100\,\mathrm{km/h} \operatorname{in} \mathrm{m/s} = ? $$"),
        "250/9 m/s"
    );
    // The prose form: a bare `in` and a plain unit.
    assert_eq!(answer("$$ v := 36 km/h $$\n$$ v in m/s = ? $$"), "10 m/s");
    assert_eq!(
        answer(r"$$ 1\,\mathrm{N\,m} \operatorname{in} \mathrm{N\,m} = ? $$"),
        "1 N·m"
    );
    assert_eq!(
        answer(r"$$ 0\,{}^\circ\mathrm{C} \operatorname{in} \mathrm{K} = ? $$"),
        "5463/20 K"
    );
    assert_eq!(
        answer(r"$$ 1\,\mathrm{mi} \operatorname{in} \mathrm{km} = ? $$"),
        "25146/15625 km"
    );
    let src = r"$$ 5\,\mathrm{m} \operatorname{in} \mathrm{s} = ? $$";
    assert_eq!(failure(src).0, "can't convert m to s");
    // E11: `to`, `->` and the `∈`/`→` that typing `in`/`to` in maths makes,
    // before a unit, convert too.
    for word in ["in", "∈", "to", "->", "→"] {
        let src = format!("$$ v = 20 m/s $$\n\n$$ v {word} km/h = $$");
        assert_eq!(answer(&src), "72 km/h", "{word}");
    }
    // `x in S` is still set membership, and `x -> 0` an arrow.
    assert_eq!(answer(r"$$ 3 \in \{1, 2, 3\} = ? $$"), "true");
    assert_eq!(answer(r"$$ \lim_{x -> 0} \frac{\sin x}{x} = ? $$"), "1");
    let approx = run(r"$$ 100\,\mathrm{km/h} \operatorname{in} \mathrm{m/s} \approx ? $$");
    let line = approx.queries().next().and_then(|(_, l)| l.approx.clone());
    assert_eq!(line.as_deref(), Some("27.77777778 m/s"));
}

#[test]
fn dimension_errors_point_at_the_term() {
    let (msg, at) = failure("$$ t := 12 s $$\n$$ 3 m + t = ? $$");
    assert_eq!((msg.as_str(), at.as_str()), ("can't add m and s", "t"));
    let (msg, at) = failure(r"$$ 1 + 2 + 3\,\mathrm{kg} \cdot 4 = ? $$");
    assert_eq!(msg, "can't add a plain number and kg");
    assert_eq!(at, r"3\,\mathrm{kg} \cdot 4");
    let (msg, at) = failure(r"$$ 2 \sin(3\,\mathrm{m}) = ? $$");
    assert_eq!(msg, "sin needs a dimensionless argument, but got m");
    assert_eq!(at, r"\sin(3\,\mathrm{m})");
    let (msg, _) = failure(r"$$ 3\,\mathrm{m} < 2\,\mathrm{kg} = ? $$");
    assert_eq!(msg, "can't compare m and kg");
    let (msg, _) = failure(r"$$ 3\,\mathrm{m} \operatorname{in} \mathrm{furlong} = ? $$");
    assert!(msg.contains("expected a unit after `in`"), "{msg}");
}

#[test]
fn letters_are_units_only_after_numbers() {
    // `m` the mass and `g` the gravity are still names.
    let src = r"$$ m := 70\,\mathrm{kg} $$
$$ g := 9.81\,\mathrm{m/s^2} $$
$$ m g = ? $$
$$ 2 m = ? $$
";
    assert_eq!(answers(src), vec!["686.7 N", "140 kg"]);
    // Parameters, indices and integration variables stay names after a number.
    assert_eq!(answer("$$ f(t) := 3 t^2 $$\n$$ f(2) = ? $$"), "12");
    assert_eq!(answer(r"$$ \int_0^1 3 t^2 \, dt = ? $$"), "1");
    assert_eq!(answer("$$ 2x + 3 = ? $$"), "2*x + 3");
    assert_eq!(answer(r"$$ \sum_{k=1}^{3} 2 k = ? $$"), "12");
    let e = answers("$$ m := 2 $$\n$$ v := 3 $$\n$$ E := \\tfrac12 m v^2 $$\n$$ E = ? $$\n");
    assert_eq!(e, vec!["9"]);
}

#[test]
fn units_through_calculus() {
    // A height in metres falling over time in seconds: the speed comes out in m/s.
    let src = r"$$ h(t) := 20\,\mathrm{m} - \frac{1}{2} \cdot 9.8\,\mathrm{m/s^2} \cdot t^2 $$
$$ h(2\,\mathrm{s}) = ? $$
$$ h'(2\,\mathrm{s}) = ? $$
$$ \int_{0\,\mathrm{s}}^{10\,\mathrm{s}} 3\,\mathrm{m/s} \, dt = ? $$
$$ \int_{0}^{2\,\mathrm{s}} 4\,\mathrm{m/s^2} \cdot t \, dt = ? $$
";
    assert_eq!(answers(src), vec!["2/5 m", "-98/5 m/s", "30 m", "8 m"]);
    let (msg, _) = failure(r"$$ \int_{0\,\mathrm{s}}^{2\,\mathrm{m}} 3 \, dt = ? $$");
    assert!(msg.contains("bounds of this integral"), "{msg}");
}

#[test]
fn definitions_show_their_units() {
    let ev = run("$$ v := 36 km/h $$\n$$ a := 9.81 m/s^2 $$\n");
    let shown: Vec<String> = ev
        .blocks
        .iter()
        .flat_map(|b| b.values.iter())
        .filter(|l| matches!(l.kind, LineKind::Definition { .. }))
        .filter_map(|l| l.plain.clone())
        .collect();
    // The decimal as typed, not 981/100.
    assert_eq!(shown, vec!["36 km/h", "9.81 m/s^2"]);
}

#[test]
fn the_parser_and_the_core_agree_on_unit_names() {
    use mpp_core::units::is_unit;
    use mpp_syntax::units::is_unit_name;
    let prefixes = [
        "", "Q", "R", "Y", "Z", "E", "P", "T", "G", "M", "k", "h", "da", "d", "c", "m", "µ", "u",
        "n", "p", "f", "a", "z", "y", "r", "q",
    ];
    let mut names: Vec<String> = Vec::new();
    for s in mpp_core::units::table::symbols() {
        for p in prefixes {
            names.push(format!("{p}{s}"));
        }
    }
    names.extend(
        [
            "ohm", "deg", "degC", "hr", "sec", "lbs", "x", "kmin", "furlong", "dam", "l",
        ]
        .iter()
        .map(|s| s.to_string()),
    );
    for n in names {
        assert_eq!(is_unit_name(&n), is_unit(&n), "`{n}`");
    }
}

fn plain_answers(src: &str) -> Vec<String> {
    run(src)
        .queries()
        .map(|(_, l)| l.plain.clone().unwrap_or_default())
        .collect()
}

#[test]
fn a_single_solve_root_is_a_value() {
    let src = r"$$ n(V) := 2\,\mathrm{mol/L} \cdot V $$

$$ V_e := \operatorname{solve}_{V} \big(n(V) = 1\,\mathrm{mol}\big) $$

$$ 2 V_e = ? $$";
    assert_eq!(answer(src), "1 L");
    // Two roots stay a set: there is no single value to compute with.
    let src = r"$$ r := \operatorname{solve}_x (x^2 = 4) $$

$$ r = ? $$";
    assert_eq!(plain_answers(src), vec!["{-2, 2}".to_string()]);
}

#[test]
fn piecewise_conditions_compare_quantities() {
    let src = r"$$ f(V) := \begin{cases} 1 & V < 25\,\mathrm{mL} \\ 2 & V = 25\,\mathrm{mL} \\ 3 & V > 25\,\mathrm{mL} \end{cases} $$

$$ f(0) = ? \qquad f(12.5\,\mathrm{mL}) = ? \qquad f(0.025\,\mathrm{L}) = ? \qquad f(1\,\mathrm{L}) = ? $$";
    assert_eq!(answers(src), vec!["1", "1", "2", "3"]);
    // Different kinds are never compared: the case stays undecided.
    let src = r"$$ g(t) := \begin{cases} 1 & t < 2\,\mathrm{s} \\ 0 & t \ge 2\,\mathrm{s} \end{cases} $$

$$ g(3\,\mathrm{m}) = ? $$";
    let shown = plain_answers(src);
    assert!(shown.iter().all(|p| p != "1" && p != "0"), "{shown:?}");
}

#[test]
fn the_log_of_a_concentration_is_of_its_value_in_mol_per_litre() {
    let src = r"$$ c := 0.001\,\mathrm{mol/L} $$

$$ -\log_{10} c = ? \qquad -\log_{10}(1\,\mathrm{mmol/mL}) = ? $$";
    assert_eq!(answers(src), vec!["3", "0"]);
    assert!(run(src)
        .all_diagnostics()
        .iter()
        .any(|d| d.message.contains("value in mol/L")));
    // Any other dimension is still an error.
    let (msg, _) = failure(r"$$ \ln(2\,\mathrm{m}) = ? $$");
    assert!(msg.contains("dimensionless"), "{msg}");
}

#[test]
fn differential_equations_with_units() {
    // A damped spring: m ẍ + c ẋ + k x = 0 with SI coefficients.
    let src = r"Let $m := 0.5\,\mathrm{kg}$, $k := 20\,\mathrm{N/m}$, $c := 0.4\,\mathrm{N\,s/m}$ and $x_0 := 10\,\mathrm{cm}$.

$$ m\ddot x + c\dot x + kx = 0, \quad x(0) = x_0, \ \dot x(0) = 0 $$

$$ x(0) = ? \qquad x(1\,\mathrm s) \approx ? \qquad x(500\,\mathrm{ms}) \approx ? $$";
    let ev = run(src);
    assert!(errors(&ev).is_empty(), "{:?}", errors(&ev));
    let approx: Vec<String> = ev
        .queries()
        .map(|(_, l)| l.approx.clone().or(l.plain.clone()).unwrap_or_default())
        .collect();
    // x(t) = x₀ e^{−0.4t}(cos ω_d t + (0.4/ω_d) sin ω_d t), ω_d = √39.84 rad/s, in the
    // centimetres x₀ is written in.
    let wd = 39.84f64.sqrt();
    let x = |t: f64| 10.0 * (-0.4 * t).exp() * ((wd * t).cos() + 0.4 / wd * (wd * t).sin());
    assert_eq!(approx[0], "10 cm", "{approx:?}");
    for (shown, t) in approx[1..].iter().zip([1.0, 0.5]) {
        let v: f64 = shown.trim_end_matches(" cm").parse().unwrap_or(f64::NAN);
        assert!((v - x(t)).abs() < 1e-8, "{shown} vs {}", x(t));
        assert!(shown.ends_with(" cm"), "{shown}");
    }
}

#[test]
fn uncoupled_systems_with_units_and_dotted_conditions() {
    let src = r"Let $g \equiv 9.81\,\mathrm{m/s^2}$, $v_0 := 20\,\mathrm{m/s}$ and $k := 0.1\,\mathrm{s^{-1}}$.

$$ \ddot x = -k\dot x, \quad \ddot y = -g - k\dot y, \quad x(0) = 0,\ y(0) = 0,\ \dot x(0) = v_0,\ \dot y(0) = v_0 $$

$$ x(1\,\mathrm s) \approx ? \qquad y(1\,\mathrm s) \approx ? \qquad \dot y(0) = ? $$";
    let ev = run(src);
    assert!(errors(&ev).is_empty(), "{:?}", errors(&ev));
    let shown: Vec<String> = ev
        .queries()
        .map(|(_, l)| l.approx.clone().or(l.plain.clone()).unwrap_or_default())
        .collect();
    let e = (-0.1f64).exp();
    let x1 = 200.0 * (1.0 - e);
    let y1 = (20.0 + 98.1) * 10.0 * (1.0 - e) - 98.1;
    for (s, want) in shown.iter().zip([x1, y1]) {
        let v: f64 = s.trim_end_matches(" m").parse().unwrap_or(f64::NAN);
        assert!((v - want).abs() < 1e-8, "{s} vs {want}");
    }
    assert_eq!(shown[2], "20 m/s");
}

#[test]
fn units_that_do_not_fit_a_differential_equation_are_not_solved() {
    // ẋ = −k x with k in metres: no unit for t makes the two sides agree.
    let src = r"Let $k := 2\,\mathrm m$.

$$ \dot x = -k x, \quad x(0) = 1 $$

$$ x(1) = ? $$";
    let ev = run(src);
    assert!(
        ev.queries().all(|(_, l)| l.plain.is_none()),
        "no value may be given"
    );
}

#[test]
fn solving_with_units_keeps_the_conditions() {
    // The landing time of a projectile with drag: y(t) = 0 at t = 0 and once later; the
    // condition t > 0 must keep only the later root (t = 0 is found exactly).
    let src = r"Let $g \equiv 9.81\,\mathrm{m/s^2}$, $v_0 := 20\,\mathrm{m/s}$ and $k := 0.1\,\mathrm{s^{-1}}$.

$$ \ddot y = -g - k\dot y, \quad y(0) = 0,\ \dot y(0) = v_0 $$

$$ T := \operatorname{solve}_t (y(t) = 0, t > 0) \approx ? $$";
    let ev = run(src);
    assert!(errors(&ev).is_empty(), "{:?}", errors(&ev));
    let shown: Vec<String> = ev
        .queries()
        .map(|(_, l)| l.approx.clone().unwrap_or_default())
        .collect();
    let t: f64 = shown[0].trim_end_matches(" s").parse().unwrap_or(f64::NAN);
    // y(T) = (v₀ + g/k)(1 − e^{−kT})/k − (g/k) T = 0.
    let y = |t: f64| (20.0 + 98.1) * (1.0 - (-0.1 * t).exp()) / 0.1 - 98.1 * t;
    assert!(y(t).abs() < 1e-6 && t > 1.0, "{shown:?}");
}
