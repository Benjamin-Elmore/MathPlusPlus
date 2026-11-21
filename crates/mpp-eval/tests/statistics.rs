//! Data and statistics in documents: tables whose header names variables, CSV files
//! (read relative to the document and again when they change), the statistics and
//! regression operators on columns, distributions and confidence intervals.

use std::sync::Arc;

use mpp_eval::{Evaluated, FileImporter, NoImports, Session};
use mpp_syntax::Severity;

fn run(src: &str) -> Evaluated {
    Session::new()
        .with_importer(Arc::new(NoImports))
        .run_str(src)
        .clone()
}

fn errors(ev: &Evaluated) -> Vec<String> {
    ev.errors().iter().map(|d| d.message.clone()).collect()
}

/// Warnings and information (a fit's R² is information).
fn notes(ev: &Evaluated) -> Vec<String> {
    ev.all_diagnostics()
        .iter()
        .filter(|d| matches!(d.severity, Severity::Warning | Severity::Info))
        .map(|d| d.message.clone())
        .collect()
}

fn shown(ev: &Evaluated) -> Vec<String> {
    ev.queries()
        .map(|(_, l)| {
            l.plain
                .clone()
                .or_else(|| l.approx.clone())
                .unwrap_or_else(|| "<none>".into())
        })
        .collect()
}

fn answers(src: &str) -> Vec<String> {
    let ev = run(src);
    let errs = errors(&ev);
    assert!(errs.is_empty(), "{src}: unexpected errors {errs:?}");
    shown(&ev)
}

/// Scores of a class (a textbook data set): n = 8, mean 5, population sd 2.
const SCORES: &str = "| $s$ |
|---|
| 2 |
| 4 |
| 4 |
| 4 |
| 5 |
| 5 |
| 7 |
| 9 |
";

#[test]
fn descriptive_statistics_of_a_column() {
    let src = format!(
        r"{SCORES}
$$ \bar{{s}} = ? $$
$$ \operatorname{{median}}(s) = ? $$
$$ \operatorname{{mode}}(s) = ? $$
$$ \operatorname{{pstd}}(s) = ? $$
$$ \operatorname{{var}}(s) = ? $$
$$ \min(s) = ? $$
$$ \max(s) = ? $$
$$ \operatorname{{quartiles}}(s) = ? $$
$$ \sum_i s_i = ? $$
$$ \frac{{1}}{{8}} \sum_{{i=1}}^{{8}} (s_i - \bar{{s}})^2 = ? $$
"
    );
    assert_eq!(
        answers(&src),
        vec![
            "5",
            "9/2",
            "4",
            "2",
            "32/7",
            "2",
            "9",
            "(4, 9/2, 6)",
            "40",
            "4"
        ]
    );
    let n = notes(&run(&src));
    assert!(n
        .iter()
        .any(|m| m == "population standard deviation (divides by n)"));
    assert!(n.iter().any(|m| m == "sample variance (divides by n − 1)"));
}

#[test]
fn columns_with_units() {
    let src = r"| $t$ (s) | $d$ [m] |
|---|---|
| 0 | 0 |
| 1 | 5 |
| 2 | 20 |

$$ \bar{d} = ? $$
$$ \operatorname{var}(d) = ? $$
$$ d_3 = ? $$
$$ \max(t) = ? $$
";
    assert_eq!(answers(src), vec!["25/3 m", "325/3 m^2", "20 m", "2 s"]);
}

#[test]
fn tables_that_are_not_data_stay_prose() {
    let ev = run("| name | value |\n|---|---|\n| a | 1 |\n");
    assert!(errors(&ev).is_empty());
    assert!(ev.queries().next().is_none());
}

#[test]
fn regression_recovers_exact_data() {
    // y = 2x + 1 exactly, and y = x² − x + 3 exactly.
    let src = r"| $x$ | $y$ | $z$ |
|---|---|---|
| 0 | 1 | 3 |
| 1 | 3 | 3 |
| 2 | 5 | 5 |
| 3 | 7 | 9 |

$$ f(u) := \operatorname{linreg}(x, y) $$
$$ g(u) := \operatorname{polyreg}(x, z, 2) $$
$$ f(10) = ? $$
$$ g(4) = ? $$
$$ \operatorname{corr}(x, y) = ? $$
";
    assert_eq!(answers(src), vec!["21", "15", "1"]);
    let n = notes(&run(src));
    assert!(n.iter().any(|m| m == "linear fit, R² = 1"), "{n:?}");
    assert!(
        n.iter().any(|m| m == "degree-2 polynomial fit, R² = 1"),
        "{n:?}"
    );
}

#[test]
fn exponential_fits_recover_their_curve() {
    // y = 3·2^x.
    let src = r"| $x$ | $y$ |
|---|---|
| 0 | 3 |
| 1 | 6 |
| 2 | 12 |
| 3 | 24 |

$$ f(u) := \operatorname{expreg}(x, y) $$
$$ f(5) \approx ? $$
";
    let a = answers(src);
    let v: f64 = a[0].parse().expect("a number");
    assert!((v - 96.0).abs() < 1e-6, "{v}");
}

#[test]
fn distributions_and_intervals() {
    let a = answers(
        r"$$ \operatorname{normalcdf}(1.96) \approx ? $$
$$ \operatorname{normalcdf}(-1, 1, 0, 1) \approx ? $$
$$ \operatorname{invnorm}(0.975) \approx ? $$
$$ \operatorname{binompdf}(10, 0.5, 5) = ? $$
$$ \operatorname{poissoncdf}(3, 2) \approx ? $$
$$ \operatorname{invt}(0.975, 10) \approx ? $$
$$ \operatorname{chi2cdf}(3.841, 1) \approx ? $$
",
    );
    assert_eq!(
        a,
        vec![
            "0.9750021049",
            "0.6826894921",
            "1.959963985",
            // p was typed as the decimal 0.5, so the answer is a decimal.
            "0.24609375",
            "17/(2*e^3)",
            "2.228138852",
            "0.9499863162"
        ]
    );
    let ev = run(r"$$ \operatorname{invnorm}(1.5) \approx ? $$");
    assert_eq!(errors(&ev).len(), 1);
}

#[test]
fn csv_files_are_read_and_read_again_when_they_change() {
    let dir = tempfile::tempdir().expect("a temporary folder");
    let csv = dir.path().join("lab.csv");
    std::fs::write(&csv, "t (s), v (m/s)\n0, 1\n1, 3\n2, 5\n").expect("write");
    let doc = dir.path().join("lab.mpp");
    let src = "Data: $L$ from [[lab.csv]].\n\n$$ \\bar{v} = ? $$\n$$ L = ? $$\n";
    std::fs::write(&doc, src).expect("write");
    let mut session = Session::new().with_importer(Arc::new(FileImporter::new()));
    session.set_file(&doc);
    let first = session.run_str(src).clone();
    assert!(errors(&first).is_empty(), "{:?}", errors(&first));
    assert_eq!(shown(&first), vec!["3 m/s", "[[0, 1], [1, 3], [2, 5]]"]);
    assert!(first
        .imported_files()
        .iter()
        .any(|f| f.ends_with("lab.csv")));
    std::fs::write(&csv, "t (s), v (m/s)\n0, 10\n1, 20\n").expect("write");
    session.invalidate(&csv);
    let second = session.update(src).clone();
    assert_eq!(shown(&second)[0], "15 m/s");
    // A broken file is reported at the link, with the line that is wrong.
    std::fs::write(&csv, "t, v\n0, 1\n1\n").expect("write");
    let third = session.update(src).clone();
    let errs = errors(&third);
    assert!(
        errs.iter().any(|e| e.contains("lab.csv: line 3")),
        "{errs:?}"
    );
    let missing = session.update("Data: $M$ from [[nothere.csv]].\n").clone();
    assert_eq!(errors(&missing).len(), 1);
}
