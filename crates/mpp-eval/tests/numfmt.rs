//! Number display directives end to end, and showing a result under another format.

use std::sync::Arc;

use mpp_eval::numfmt::{fewer_digits, more_digits, toggle_exact_decimal};
use mpp_eval::{Evaluated, NoImports, NumberFormat, Session};

fn run(src: &str) -> Evaluated {
    Session::new()
        .with_importer(Arc::new(NoImports))
        .run_str(src)
        .clone()
}

/// The text of each query line.
fn texts(src: &str) -> Vec<String> {
    let ev = run(src);
    let errs: Vec<_> = ev
        .errors()
        .iter()
        .map(|d| mpp_core::note::plain(&d.message))
        .collect();
    assert!(errs.is_empty(), "{src}: {errs:?}");
    ev.queries()
        .map(|(_, l)| l.plain.clone().unwrap_or_else(|| "<none>".into()))
        .collect()
}

fn latexes(src: &str) -> Vec<String> {
    run(src)
        .queries()
        .map(|(_, l)| l.latex.clone().unwrap_or_default())
        .collect()
}

fn approxes(src: &str) -> Vec<String> {
    run(src)
        .queries()
        .map(|(_, l)| l.approx.clone().unwrap_or_default())
        .collect()
}

fn one(src: &str) -> String {
    let t = texts(src);
    assert_eq!(t.len(), 1, "{t:?}");
    t[0].clone()
}

#[test]
fn defaults_are_unchanged() {
    assert_eq!(one(r"$$ \frac{7}{2} + 0 = ? $$"), "7/2");
    assert_eq!(approxes(r"$$ \sqrt{2} = ? $$"), vec!["1.414213562"]);
    assert_eq!(
        approxes(r"$$ \frac{1}{3} \approx ? $$"),
        vec!["0.3333333333"]
    );
}

#[test]
fn significant_figures() {
    assert_eq!(approxes("%sf 4\n$$ \\pi \\approx ? $$"), vec!["3.142"]);
    assert_eq!(approxes("%sf 3\n$$ \\sqrt{2} = ? $$"), vec!["1.41"]);
    assert_eq!(approxes("%sf 3\n$$ 100 \\sqrt{2} = ? $$"), vec!["141"]);
}

#[test]
fn decimal_places() {
    assert_eq!(approxes("%dp 2\n$$ \\pi \\approx ? $$"), vec!["3.14"]);
    assert_eq!(approxes("%dp 0\n$$ \\pi \\approx ? $$"), vec!["3"]);
    assert_eq!(
        approxes("%dp 4\n$$ \\frac{1}{3} \\approx ? $$"),
        vec!["0.3333"]
    );
    assert_eq!(one("%dp 2\n%decimal\n$$ \\frac{1}{3} = ? $$"), "0.33");
    assert_eq!(one("%dp 3\n%decimal\n$$ 2 + 0 = ? $$"), "2.000");
}

#[test]
fn exact_and_decimal() {
    assert_eq!(one("%decimal\n$$ \\frac{7}{2} = ? $$"), "3.5");
    assert_eq!(one("%decimal\n$$ \\frac{1}{3} = ? $$"), "0.3333333333");
    assert_eq!(one("%decimal\n$$ \\sqrt{2} = ? $$"), "1.414213562");
    assert_eq!(
        latexes("%decimal\n%sf 4\n$$ \\frac{22}{7} = ? $$"),
        vec!["3.143"]
    );
    assert_eq!(one("%decimal\n%exact\n$$ \\frac{7}{2} = ? $$"), "7/2");
}

#[test]
fn scientific_notation() {
    let src = "%sci\n$$ 6.02 \\times 10^{23} + 0 = ? $$";
    assert_eq!(one(src), "6.02 × 10²³");
    assert_eq!(latexes(src), vec!["6.02 \\times 10^{23}"]);
    assert!(!one(src).contains('e'), "no e-notation for users");
    assert_eq!(one("%sci\n$$ 12345 + 0 = ? $$"), "1.2345 × 10⁴");
    assert_eq!(one("%sci\n$$ 999 + 0 = ? $$"), "999");
    assert_eq!(one("%sci 6\n$$ 12345 + 0 = ? $$"), "12345");
    assert_eq!(one("%sci 6\n$$ 1234567 + 0 = ? $$"), "1.234567 × 10⁶");
    assert_eq!(
        one("%sci\n%sf 3\n$$ 6.0221 \\times 10^{23} + 0 = ? $$"),
        "6.02 × 10²³"
    );
}

#[test]
fn engineering_notation() {
    assert_eq!(
        one("%eng\n$$ 123456789 \\cdot 1000 = ? $$"),
        "123.456789 × 10⁹"
    );
    assert_eq!(
        latexes("%eng\n$$ 123456789 \\cdot 1000 = ? $$"),
        vec!["123.456789 \\times 10^{9}"]
    );
    assert_eq!(one("%eng\n$$ 12345 + 0 = ? $$"), "12.345 × 10³");
}

#[test]
fn plain_resets_notation_and_digits() {
    assert_eq!(one("%sci\n%sf 3\n%plain\n$$ 12345 + 0 = ? $$"), "12345");
}

#[test]
fn mixed_and_improper_fractions() {
    assert_eq!(one("%mixed\n$$ \\frac{7}{2} + 0 = ? $$"), "3 1/2");
    assert_eq!(
        latexes("%mixed\n$$ \\frac{7}{2} + 0 = ? $$"),
        vec!["3\\frac{1}{2}"]
    );
    assert_eq!(one("%mixed\n%improper\n$$ \\frac{7}{2} + 0 = ? $$"), "7/2");
    assert_eq!(one("%mixed\n$$ \\frac{1}{2} + 0 = ? $$"), "1/2");
}

#[test]
fn degrees_and_radians() {
    assert_eq!(one("%deg\n$$ \\arcsin(1/2) = ? $$"), "30°");
    assert_eq!(latexes("%deg\n$$ \\arcsin(1/2) = ? $$"), vec!["30^\\circ"]);
    assert_eq!(one("%deg\n$$ \\arctan(1) \\approx ? $$"), "45°");
    assert_eq!(one("%deg\n$$ \\arccos(0) = ? $$"), "90°");
    assert!(one("%deg\n$$ \\arcsin(0.3) = ? $$").starts_with("17.4576031"));
    assert_eq!(one("%deg\n%rad\n$$ \\arcsin(1/2) = ? $$"), "π/6");
    assert_eq!(one("$$ \\arcsin(1/2) = ? $$"), "π/6");
    // Only angle results change; other numbers and the input meaning do not.
    assert_eq!(one("%deg\n$$ \\frac{\\pi}{6} + 0 = ? $$"), "π/6");
    assert_eq!(one("%deg\n$$ \\sin(30^\\circ) = ? $$"), "1/2");
}

#[test]
fn bare_numbers_in_trig_stay_radians() {
    // `%deg` is about display: \sin 30 is the sine of 30 radians either way.
    let plain = approxes("$$ \\sin(30) \\approx ? $$");
    let deg = approxes("%deg\n$$ \\sin(30) \\approx ? $$");
    assert_eq!(plain, deg);
    assert!(plain[0].starts_with("-0.98803162"), "{plain:?}");
}

#[test]
fn complex_polar_and_rectangular() {
    assert_eq!(one("$$ (3 + 4i) + 0 = ? $$"), "3 + 4*i");
    assert_eq!(one("%polar\n$$ (3 + 4i) + 0 = ? $$"), "5∠0.927295218");
    assert_eq!(
        latexes("%polar\n$$ (3 + 4i) + 0 = ? $$"),
        vec!["5 e^{0.927295218 i}"]
    );
    assert_eq!(
        one("%polar\n%deg\n%dp 2\n$$ (3 + 4i) + 0 = ? $$"),
        "5.00∠53.13°"
    );
    assert_eq!(one("%polar\n%rect\n$$ (3 + 4i) + 0 = ? $$"), "3 + 4*i");
    assert_eq!(
        one("%decimal\n%sf 3\n$$ (1/2 + i/4) + 0 = ? $$"),
        "0.500 + 0.250*i"
    );
}

#[test]
fn directives_apply_from_their_line_on() {
    let src = "$$ \\pi \\approx ? $$\n\n%sf 3\n\n$$ \\pi \\approx ? $$\n\n%sf 6\n\n$$ \\pi \\approx ? $$\n";
    assert_eq!(approxes(src), vec!["3.141592654", "3.14", "3.14159"]);
}

#[test]
fn block_directive_applies_to_the_next_block_only() {
    let src = "%block dp 2\n$$ \\pi \\approx ? $$\n\n$$ \\pi \\approx ? $$\n";
    assert_eq!(approxes(src), vec!["3.14", "3.141592654"]);
    // A file-wide setting plus a block override on top.
    let src = "%sf 8\n\n%block sf 3\n$$ \\pi \\approx ? $$\n\n$$ \\pi \\approx ? $$\n";
    assert_eq!(approxes(src), vec!["3.14", "3.1415927"]);
}

#[test]
fn comments_are_not_directives() {
    let src = "% sf 3 is just a note\n$$ \\pi \\approx ? $$\n";
    assert!(run(src).errors().is_empty());
    assert_eq!(approxes(src), vec!["3.141592654"]);
}

#[test]
fn malformed_directives_are_errors_in_place() {
    for (src, needle) in [
        ("%sf\n$$ 1 + 1 = ? $$", "needs a number"),
        ("%sf x\n$$ 1 + 1 = ? $$", "whole number"),
        ("%dp 99\n$$ 1 + 1 = ? $$", "from 0 to 50"),
        ("%exact now\n$$ 1 + 1 = ? $$", "takes no arguments"),
        ("%block\n$$ 1 + 1 = ? $$", "needs an option"),
    ] {
        let ev = run(src);
        let errs: Vec<String> = ev
            .errors()
            .iter()
            .map(|d| mpp_core::note::plain(&d.message))
            .collect();
        assert!(
            errs.iter().any(|m| m.contains(needle)),
            "{src}: {errs:?} should mention {needle}"
        );
        // The rest of the file still runs.
        assert_eq!(ev.queries().count(), 1);
    }
}

#[test]
fn units_follow_the_same_rules() {
    let src = "$$ v := 100\\,\\mathrm{km/h} $$\n\n$$ v \\operatorname{in} \\mathrm{m/s} = ? $$\n";
    assert_eq!(one(src), "250/9 m/s");
    assert_eq!(one(&format!("%decimal\n%sf 4\n{src}")), "27.78 m/s");
    let big = "%sci\n$$ 6.02 \\times 10^{23}\\,\\mathrm{kg} + 0\\,\\mathrm{kg} = ? $$";
    assert_eq!(one(big), "6.02 × 10²³ kg");
    assert_eq!(latexes(big), vec!["6.02 \\times 10^{23}\\,\\mathrm{kg}"]);
}

#[test]
fn session_default_format_is_the_files_starting_point() {
    let mut s = Session::new().with_importer(Arc::new(NoImports));
    s.set_number_format(NumberFormat {
        precision: mpp_core::display::Precision::Sig(5),
        ..NumberFormat::default()
    });
    let ev = s
        .run_str("$$ \\pi \\approx ? $$\n\n%sf 2\n\n$$ \\pi \\approx ? $$\n")
        .clone();
    let a: Vec<_> = ev
        .queries()
        .map(|(_, l)| l.approx.clone().unwrap())
        .collect();
    assert_eq!(a, vec!["3.1416", "3.1"]);
}

#[test]
fn reformat_switches_without_changing_the_file() {
    let ev = run("$$ \\frac{22}{7} + 0 = ? $$\n\n$$ \\sqrt{2} = ? $$\n");
    let lines: Vec<_> = ev.queries().map(|(_, l)| l.clone()).collect();
    let base = NumberFormat::default();

    // exact <-> decimal
    let dec = toggle_exact_decimal(&base);
    assert_eq!(lines[0].reformat(&dec).0.as_deref(), Some("3.142857143"));
    assert_eq!(lines[0].reformat(&base).0.as_deref(), Some("22/7"));
    assert_eq!(lines[0].reformat(&dec).1.as_deref(), Some("3.142857143"));

    // more / fewer digits
    let fewer = fewer_digits(&fewer_digits(&dec));
    assert_eq!(lines[0].reformat(&fewer).0.as_deref(), Some("3.1428571"));
    let more = more_digits(&dec);
    assert_eq!(lines[0].reformat(&more).0.as_deref(), Some("3.1428571429"));
    assert_eq!(lines[1].reformat(&more).0.as_deref(), Some("1.4142135624"));
    assert_eq!(
        lines[1].reformat_all(&base).2.as_deref(),
        Some("1.414213562")
    );

    // Reformatting is a pure query: the line itself did not change.
    assert_eq!(lines[0].plain.as_deref(), Some("22/7"));

    // A result already shown under a directive can still be switched back.
    let ev = run("%sf 3\n%decimal\n$$ \\frac{22}{7} + 0 = ? $$\n");
    let line = ev.queries().next().unwrap().1.clone();
    assert_eq!(line.plain.as_deref(), Some("3.14"));
    assert_eq!(line.reformat(&base).0.as_deref(), Some("22/7"));
    let mut line = line;
    line.set_format(&base);
    assert_eq!(line.plain.as_deref(), Some("22/7"));
    assert_eq!(line.format(), Some(&base));
}

#[test]
fn keep_text_gives_what_goes_in_the_file() {
    let ev = run("%sci\n$$ 6.02 \\times 10^{23} + 0 = ? $$\n");
    let line = ev.queries().next().unwrap().1;
    assert_eq!(line.keep_text().as_deref(), Some("= 6.02 \\times 10^{23}"));
    let ev = run("$$ \\sqrt{2} = ? $$\n");
    let line = ev.queries().next().unwrap().1;
    assert_eq!(line.keep_text().as_deref(), Some("= sqrt(2) ≈ 1.414213562"));
}

#[test]
fn custom_presentations_are_left_alone() {
    // A set is shown by its own presenter; display options never garble it.
    let plain = texts("$$ \\{1, 2, 3\\} \\cup \\{3, 4\\} = ? $$");
    let fancy = texts("%mixed\n%sci\n$$ \\{1, 2, 3\\} \\cup \\{3, 4\\} = ? $$");
    assert_eq!(plain, fancy);
}

#[test]
fn applying_a_format_never_changes_values() {
    let src = "%decimal\n%sf 3\n$$ a := \\frac{1}{3} $$\n\n$$ a + \\frac{1}{6} = ? $$\n";
    let ev = run(src);
    let line = ev.queries().next().unwrap().1;
    assert_eq!(line.plain.as_deref(), Some("0.500"));
    assert_eq!(line.value.as_ref().unwrap().expr().to_string(), "1/2");
}
