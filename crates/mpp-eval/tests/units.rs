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
    ev.errors().iter().map(|d| d.message.clone()).collect()
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
            .map(|d| d.message.clone());
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
        "6867/10 N"
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
        "3/2 km"
    );
    assert_eq!(answer(r"$$ \cos(60^\circ) = ? $$"), "1/2");
    assert_eq!(answer(r"$$ 3\,\mathrm{m} < 2\,\mathrm{km} = ? $$"), "true");
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
    // `x in S` is still set membership.
    assert_eq!(answer(r"$$ 3 \in \{1, 2, 3\} = ? $$"), "true");
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
    assert_eq!(answers(src), vec!["6867/10 N", "140 kg"]);
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
    assert_eq!(shown, vec!["36 km/h", "981/100 m/s^2"]);
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
