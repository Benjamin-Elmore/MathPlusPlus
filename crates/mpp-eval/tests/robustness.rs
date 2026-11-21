//! Performance and robustness regressions: inputs that used to take exponential time,
//! freeze the evaluator or give a wrong answer. Time limits are generous so they hold in
//! debug builds; the tight targets are checked by the release-only tests at the end.

use std::sync::Arc;
use std::time::{Duration, Instant};

use mpp_eval::{Evaluated, NoImports, Session};

fn run(src: &str) -> Evaluated {
    Session::new()
        .with_importer(Arc::new(NoImports))
        .run_str(src)
        .clone()
}

fn errors(ev: &Evaluated) -> Vec<String> {
    ev.errors().iter().map(|d| d.message.clone()).collect()
}

/// Runs `src` and fails when it takes longer than `limit`.
fn timed(src: &str, limit: Duration) -> Evaluated {
    let start = Instant::now();
    let ev = run(src);
    let took = start.elapsed();
    assert!(took < limit, "took {took:?} (limit {limit:?})");
    ev
}

/// The exact answer of the first query (no errors allowed).
fn answer_of(ev: &Evaluated) -> String {
    let errs = errors(ev);
    assert!(errs.is_empty(), "{errs:?}");
    let (_, line) = ev.queries().next().expect("a query");
    line.plain.clone().unwrap_or_default()
}

fn tower(n: usize, integrand: &str, var: &str) -> String {
    let mut s = String::from("$$ ");
    for _ in 0..n {
        s.push_str(r"\int_0^1 ");
    }
    s.push_str(integrand);
    for _ in 0..n {
        s.push_str(&format!(r" \, d{var}"));
    }
    s.push_str(" = ? $$");
    s
}

#[test]
fn nested_integrals_of_a_constant_inner_value_take_linear_time() {
    // Ten levels used to spin for minutes (about 8x per level).
    for n in [6, 10, 50] {
        let ev = timed(&tower(n, "x", "x"), Duration::from_secs(20));
        assert_eq!(answer_of(&ev), "1/2", "n = {n}");
    }
}

#[test]
fn deep_nested_integrals_without_a_closed_form_stop_with_a_clear_message() {
    let src = r"$$ \int_0^1 \int_0^1 \int_0^1 \int_0^1 \int_0^1 \int_0^1 \int_0^1 \sin(x y z u v w t) \, dx \, dy \, dz \, du \, dv \, dw \, dt = ? $$";
    let ev = timed(src, Duration::from_secs(30));
    let errs = errors(&ev);
    // Either a numerical answer or a clear "too large" message, never a hang.
    if !errs.is_empty() {
        assert!(
            errs.iter()
                .any(|e| e.contains("too large") || e.contains("nested")),
            "{errs:?}"
        );
    }
}

#[test]
fn a_byte_order_mark_before_the_first_block_is_ignored() {
    // Notepad writes one; it used to hide the first `$$` block.
    let ev = run("\u{FEFF}$$ 1 + 1 = ? $$\n");
    assert_eq!(answer_of(&ev), "2");
}

#[test]
fn old_mac_line_endings_are_read() {
    let ev = run("# T\r\r$$ 1 + 2 = ? $$\r");
    assert_eq!(answer_of(&ev), "3");
}

#[test]
fn defined_names_are_read_without_evaluating() {
    let src = r"Let $\theta := 2$.

$$ f(x) := x^2 $$

$$ a_0 := 1 $$

$$ a_{n+1} := 2 a_n $$

$$ \operatorname{factor}(2^{256} + 1) = ? $$
";
    let start = Instant::now();
    let names = mpp_eval::defined_names(src);
    assert!(start.elapsed() < Duration::from_secs(1));
    assert_eq!(names, ["a", "f", "θ"]);
}

#[test]
fn names_in_let_sentences_keep_their_subscripts() {
    let names = mpp_eval::defined_names(r"Let $\vartheta := 0.5$ and $v_0 := 20$.");
    assert_eq!(names, ["v_0", "ϑ"]);
}

/// A CSV file of `rows` measurements `h (cm), m (kg)` with decimals, and its folder.
fn big_csv(rows: usize) -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("temp dir");
    let mut text = String::from("h (cm), m (kg)\n");
    for i in 0..rows {
        let h = 150.0 + (i % 500) as f64 * 0.1 + 0.005;
        let m = 0.9 * h - 85.0 + ((i * 7) % 13) as f64 * 0.01;
        text.push_str(&format!("{h:.3}, {m:.3}\n"));
    }
    std::fs::write(dir.path().join("d.csv"), text).expect("write csv");
    dir
}

fn session_in(dir: &std::path::Path) -> Session {
    let mut s = Session::new().with_importer(Arc::new(mpp_eval::FileImporter::new()));
    s.set_file(dir.join("doc.mpp"));
    s
}

const STATS: &str = r"Data: $D$ from [[d.csv]].

Some prose.

$$ \operatorname{corr}(h, m) \approx ? $$

$$ \sum_i m_i = ? $$

$$ \bar{h} = ? $$
";

fn check_stats(ev: &Evaluated) {
    let errs = errors(ev);
    assert!(errs.is_empty(), "{errs:?}");
    let answers: Vec<String> = ev
        .queries()
        .map(|(_, l)| l.plain_answer().unwrap_or_default())
        .collect();
    assert_eq!(answers.len(), 3, "{answers:?}");
    assert!(answers[0].contains("0.99"), "{answers:?}");
    // Measured decimals stay decimals, never huge fractions.
    assert!(!answers[2].contains('/'), "{answers:?}");
}

#[test]
fn large_data_files_load_quickly_and_are_not_reloaded_on_every_edit() {
    let rows = if cfg!(debug_assertions) {
        20_000
    } else {
        100_000
    };
    let dir = big_csv(rows);
    let mut s = session_in(dir.path());
    let start = Instant::now();
    let first = s.run_str(STATS).clone();
    let load = start.elapsed();
    check_stats(&first);
    // A prose edit re-runs the Data line, but the unchanged file is not parsed again
    // and the statistics are not recomputed.
    let edited = STATS.replace("Some prose.", "Some more prose.");
    let start = Instant::now();
    let second = s.update(&edited).clone();
    let edit = start.elapsed();
    check_stats(&second);
    let limit = if cfg!(debug_assertions) { 10 } else { 1 };
    assert!(edit < Duration::from_secs(limit), "edit took {edit:?}");
    if !cfg!(debug_assertions) {
        assert!(load < Duration::from_secs(5), "first run took {load:?}");
    }
}

#[test]
fn a_long_column_is_summarised_not_printed() {
    let dir = big_csv(1_000);
    let mut s = session_in(dir.path());
    let ev = s.run_str("Data: $D$ from [[d.csv]].\n").clone();
    let shown: Vec<String> = ev
        .blocks
        .iter()
        .flat_map(|b| b.values.iter())
        .filter_map(|l| l.plain.clone())
        .collect();
    assert!(shown.iter().any(|t| t == "1000 values in cm"), "{shown:?}");
}

fn plain_answer(src: &str) -> String {
    let ev = run(src);
    let errs = errors(&ev);
    assert!(errs.is_empty(), "{src}: {errs:?}");
    let (_, line) = ev.queries().next().expect("a query");
    line.plain_answer().unwrap_or_default()
}

#[test]
fn mutually_recursive_functions_never_give_a_wrong_answer() {
    // `f(1)` used to be `g + 1`: `g(x)` was read as `g · x` because `g` came later.
    let ev = run("$$ f(x) := g(x) + 1 $$\n\n$$ g(x) := f(x) + 1 $$\n\n$$ f(1) = ? $$\n");
    let errs = errors(&ev);
    assert!(
        errs.iter()
            .any(|e| e.contains("too deeply") || e.contains("circular")),
        "{errs:?}"
    );
    // A forward use that terminates works.
    let ev = run("$$ f(x) := g(x) + 1 $$\n\n$$ g(x) := 2 x $$\n\n$$ f(3) = ? $$\n");
    assert_eq!(answer_of(&ev), "7");
}

#[test]
fn huge_values_are_shown_in_scientific_form_not_as_infinity() {
    let fact = plain_answer("$$ 100000! = ? $$");
    assert!(
        fact.contains("2.8242294")
            && (fact.contains("456573") || fact.contains("⁴⁵⁶⁵⁷³"))
            && !fact.contains('∞'),
        "{fact}"
    );
    let pow = plain_answer("$$ 2^{1000000} = ? $$");
    assert!(
        (pow.contains("301029") || pow.contains("³⁰¹⁰²⁹")) && !pow.contains('∞'),
        "{pow}"
    );
    let ln = plain_answer(r"$$ \ln(1000!) \approx ? $$");
    assert!(ln.contains("5912.12817"), "{ln}");
    let m = plain_answer(r"$$ \operatorname{mod}(2^{1000000}, 7) = ? $$");
    assert!(m.ends_with('2'), "{m}");
}

#[test]
fn recurrences_run_to_their_term_limit_with_a_clear_message_past_it() {
    // Terms past 10 000 used to fail with "`a` calls itself too deeply".
    let ev = timed(
        "$$ a_0 := 1 $$\n\n$$ a_{n+1} := a_n + 1 $$\n\n$$ a_{50000} = ? $$\n",
        Duration::from_secs(30),
    );
    assert_eq!(answer_of(&ev), "50001");
    // Fibonacci terms with thousands of digits stay quick (big-integer steps).
    let ev = timed(
        "$$ a_0 := 1 $$\n\n$$ a_1 := 1 $$\n\n$$ a_{n+1} := a_n + a_{n-1} $$\n\n$$ a_{30000} = ? $$\n",
        Duration::from_secs(30),
    );
    assert!(answer_of(&ev).len() > 6000);
    let ev = run("$$ a_0 := 1 $$\n\n$$ a_{n+1} := a_n + 1 $$\n\n$$ a_{1000000} = ? $$\n");
    let errs = errors(&ev);
    assert!(
        errs.iter().any(|e| e.contains("100000-term limit")),
        "{errs:?}"
    );
}

#[test]
fn a_product_too_long_to_compute_is_called_a_product() {
    let ev = run(r"$$ \prod_{k=1}^{1000000000} (1 + \frac{1}{k^2}) \approx ? $$");
    let errs = errors(&ev);
    assert!(errs.iter().any(|e| e.contains("this product")), "{errs:?}");
}

#[test]
fn user_functions_recurse_thousands_of_levels_with_a_clear_limit_past_that() {
    let def =
        r"$$ f(n) := \begin{cases} 0 & n \le 0 \\ f(n-1) + 1 & \text{otherwise} \end{cases} $$";
    let ev = timed(
        &format!("{def}\n\n$$ f(9000) = ? $$\n"),
        Duration::from_secs(60),
    );
    assert_eq!(answer_of(&ev), "9000");
    let ev = run(&format!("{def}\n\n$$ f(20000) = ? $$\n"));
    let errs = errors(&ev);
    assert!(
        errs.iter()
            .any(|e| e.contains("more than 10000 nested calls")),
        "{errs:?}"
    );
}

#[test]
fn queries_on_a_data_file_that_did_not_load_say_so_instead_of_echoing() {
    let dir = tempfile::tempdir().expect("temp dir");
    let doc = r"Data: $D$ from [[missing.csv]].

$$ \bar{h} = ? $$

$$ \operatorname{median}(m) = ? $$

$$ \operatorname{var}(m) = ? $$

$$ p(u) := \operatorname{linreg}(h, m) $$

$$ \frac{d}{dx} x^2 = ? $$
";
    let ev = session_in(dir.path()).run_str(doc).clone();
    let errs = errors(&ev);
    assert_eq!(errs.len(), 5, "{errs:?}");
    assert!(errs[0].contains("missing.csv"), "{errs:?}");
    for e in &errs[1..] {
        assert!(
            e.contains("no data, because missing.csv did not load"),
            "{errs:?}"
        );
    }
    // Maths that does not use the data is answered as usual.
    let last = ev.queries().last().expect("query").1;
    assert_eq!(last.plain.as_deref(), Some("2*x"));
    // Once the file is there, the same queries are answered.
    std::fs::write(dir.path().join("missing.csv"), "h, m\n1, 2\n3, 5\n").expect("write");
    let ev = session_in(dir.path()).run_str(doc).clone();
    assert!(errors(&ev).is_empty(), "{:?}", errors(&ev));
}

#[test]
fn a_bad_byte_is_flagged_and_the_rest_of_the_file_still_runs() {
    let dir = tempfile::tempdir().expect("temp dir");
    let mut bytes = b"$$ x := 3 $$\n\nSome prose with a Latin-1 degree sign: 20".to_vec();
    bytes.push(0xB0);
    bytes.extend_from_slice(b"C.\n\n$$ x + 1 = ? $$\n");
    let path = dir.path().join("doc.mpp");
    std::fs::write(&path, &bytes).expect("write");
    let mut s = Session::new().with_importer(Arc::new(mpp_eval::FileImporter::new()));
    let ev = s
        .run_file(&path)
        .expect("a bad byte no longer refuses the file")
        .clone();
    let (_, line) = ev.queries().next().expect("a query");
    assert_eq!(line.plain.as_deref(), Some("4"));
    let warnings: Vec<_> = ev
        .all_diagnostics()
        .into_iter()
        .filter(|d| d.message.contains("not valid UTF-8"))
        .collect();
    assert_eq!(warnings.len(), 1, "{:?}", ev.all_diagnostics());
    let at = warnings[0].span.start;
    assert_eq!(&ev.source()[at..warnings[0].span.end], "\u{FFFD}");
    assert!(ev.source()[..at].ends_with("sign: 20"));

    // An imported file with a bad byte loads too, next to a data file.
    let mut lib = b"$$ y := 2 $$\n\n% caf".to_vec();
    lib.push(0xE9);
    lib.push(b'\n');
    std::fs::write(dir.path().join("lib.mpp"), &lib).expect("write");
    let mut csv = b"t (s), v\n".to_vec();
    csv.extend_from_slice(b"1, 2\n2, 4\n");
    std::fs::write(dir.path().join("d.csv"), &csv).expect("write");
    let main = dir.path().join("main.mpp");
    std::fs::write(
        &main,
        "![[lib.mpp]]\n\nData: $D$ from [[d.csv]].\n\n$$ y + \\bar{v} = ? $$\n",
    )
    .expect("write");
    let ev = s.run_file(&main).expect("run").clone();
    let (_, line) = ev.queries().next().expect("a query");
    assert_eq!(line.plain.as_deref(), Some("5"));
}
