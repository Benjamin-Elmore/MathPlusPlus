//! The pipeline tests: every document in `work/pipeline-tests/` is checked and run like
//! `mpp check` and `mpp run`, and compared with the expectations stated in its own header
//! (see `work/pipeline-tests/index.mpp`).
//!
//! A document passes when it has no errors and every expected result appears in its
//! answers. Proof documents also expect proof statuses (N15), which this branch does not
//! have, so they can only be known failures here. Documents that fail are listed in
//! `KNOWN_FAILURES` with the reason; M16 requires that list to be empty. A document
//! that passes while listed is itself a failure, so the list cannot go stale.

use std::path::{Path, PathBuf};

use mpp_cli::{run_file, run_report};

struct Case {
    file: &'static str,
    /// Results the document must show, as text found in its `mpp run` output.
    expect: &'static [&'static str],
    /// The document also expects proof statuses from the N15 checker.
    proof: bool,
}

const CASES: &[Case] = &[
    case("navier-stokes.mpp", &[], true),
    case("historic/euclid-primes.mpp", &[], true),
    case("historic/sqrt2-irrational.mpp", &[], true),
    case("historic/pythagoras.mpp", &[], true),
    case("historic/gauss-sum.mpp", &["5050"], true),
    case("historic/fermat-little.mpp", &[], true),
    case("historic/cauchy-schwarz.mpp", &[], true),
    case("historic/euler-basel.mpp", &[], true),
    case("historic/archimedes-parabola.mpp", &[], true),
    case("historic/euler-identity.mpp", &[], true),
    case("historic/cantor-diagonal.mpp", &[], true),
    case("historic/turing-halting.mpp", &[], true),
    case("labs/01-projectile-drag.mpp", &["40.77", "34.07"], false),
    case("labs/02-pendulum.mpp", &["2.006", "2.041"], false),
    case("labs/03-damped-spring.mpp", &["0.0632", "0.9954"], false),
    case("labs/04-rc-circuit.mpp", &["3.161"], false),
    case("labs/05-radioactive-decay.mpp", &["0.2983", "7575"], false),
    case("labs/06-reaction-kinetics.mpp", &["1.926"], false),
    case("labs/07-boyles-law.mpp", &["99.2", "0.0400"], false),
    case("labs/08-titration.mpp", &["1.477", "11.959"], false),
    case("labs/09-predator-prey.mpp", &["2.75"], false),
    case("labs/10-heat-rod.mpp", &["702.3"], false),
    case("labs/11-monte-carlo-pi.mpp", &["0.0049"], false),
];

const fn case(file: &'static str, expect: &'static [&'static str], proof: bool) -> Case {
    Case {
        file,
        expect,
        proof,
    }
}

/// Documents that do not pass yet, and why. M16 needs this list empty.
const KNOWN_FAILURES: &[(&str, &str)] = &[
    ("navier-stokes.mpp", "needs N15 proof statuses; also `∇` operators, integrals over a region, sums over a set and the `[@key]` references file are not supported yet"),
    ("historic/euclid-primes.mpp", "needs N15 proof statuses"),
    ("historic/sqrt2-irrational.mpp", "needs N15 proof statuses"),
    ("historic/pythagoras.mpp", "needs N15 proof statuses"),
    ("historic/gauss-sum.mpp", "needs N15 proof statuses; `…` (ellipsis) is not supported yet"),
    ("historic/fermat-little.mpp", "needs N15 proof statuses"),
    ("historic/cauchy-schwarz.mpp", "needs N15 proof statuses"),
    ("historic/euler-basel.mpp", "needs N15 proof statuses"),
    ("historic/archimedes-parabola.mpp", "needs N15 proof statuses"),
    ("historic/euler-identity.mpp", "needs N15 proof statuses"),
    ("historic/cantor-diagonal.mpp", "needs N15 proof statuses"),
    ("historic/turing-halting.mpp", "needs N15 proof statuses"),
    ("labs/01-projectile-drag.mpp", "`≈ ?` queries in prose and a figure sentence with a trailing clause are rejected (mpp-syntax, mpp-eval); unit-valued `:=` lines"),
    ("labs/02-pendulum.mpp", "`≈ ?` queries in prose, a figure sentence with a trailing clause; elliptic-integral or ODE helpers (C-lane)"),
    ("labs/03-damped-spring.mpp", "`for all t` condition after a query, figure keywords with units in ranges (`t ∈ [0, 10 s]`)"),
    ("labs/04-rc-circuit.mpp", "`solve_t(…) ≈ ?`, fit results with names like `τ_fit`, figure items"),
    ("labs/06-reaction-kinetics.mpp", "bracket names `[A]_0` as definitions, composite subscripts `E_{a,fit}`"),
    ("labs/07-boyles-law.mpp", "multi-result `a ≈ ? b ≈ ?` lines, regression results in prose"),
    ("labs/08-titration.mpp", "`:=` with a leading operator, `\\mathrm{pH}` as a function head"),
    ("labs/09-predator-prey.mpp", "`V` called with a vector, numeric maxima over an interval, figure sentences"),
    ("labs/10-heat-rod.mpp", "PDE function `u(x, t)` as an unknown function"),
    ("labs/11-monte-carlo-pi.mpp", "`\\hat{\\pi}` as a definition head, random sampling with a seed"),
];

/// True when the report shows the expected result: for a number written with `d`
/// decimals (`0.2983`), some number in the report that rounds to it (`0.2982924364`),
/// as the labs state their results to that precision; any other text must appear as
/// written.
fn shows(report: &str, needle: &str) -> bool {
    let Ok(want) = needle.parse::<f64>() else {
        return report.contains(needle);
    };
    let decimals = needle.split_once('.').map_or(0, |(_, d)| d.len());
    let half_ulp = 0.5 * 10f64.powi(-(decimals as i32));
    numbers(report).any(|x| (x - want).abs() <= half_ulp * (1.0 + 1e-9))
}

/// The decimal numbers written in `text` (`3.160602794`, `7574.647984`, `1e-3`).
fn numbers(text: &str) -> impl Iterator<Item = f64> + '_ {
    text.split(|c: char| !(c.is_ascii_digit() || c == '.' || c == 'e' || c == '-'))
        .filter_map(|w| {
            let w = w.trim_matches(|c: char| c == '.' || c == 'e' || c == '-');
            w.parse::<f64>().ok()
        })
}

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../work/pipeline-tests")
}

/// What running a document gave: `Ok(())` when it meets its expectations.
fn outcome(c: &Case) -> Result<(), String> {
    let path = root().join(c.file);
    let ev = run_file(&path)?;
    let mut problems = vec![];
    let errors = ev.errors();
    if !errors.is_empty() {
        problems.push(format!(
            "{} error(s), first: {}",
            errors.len(),
            errors[0].message
        ));
    }
    let report = run_report(&ev);
    for needle in c.expect {
        if !shows(&report, needle) {
            problems.push(format!("missing result {needle}"));
        }
    }
    if c.proof {
        problems.push("proof statuses need the N15 checker".into());
    }
    if problems.is_empty() {
        Ok(())
    } else {
        Err(problems.join("; "))
    }
}

#[test]
fn every_pipeline_document_is_listed() {
    let mut on_disk = vec![];
    for dir in [root(), root().join("historic"), root().join("labs")] {
        for e in std::fs::read_dir(&dir).expect("pipeline dir") {
            let p = e.expect("entry").path();
            if p.extension().is_some_and(|x| x == "mpp") && p.file_name().unwrap() != "index.mpp" {
                let rel = p
                    .strip_prefix(root())
                    .unwrap()
                    .to_string_lossy()
                    .replace('\\', "/");
                on_disk.push(rel);
            }
        }
    }
    on_disk.sort();
    let mut listed: Vec<String> = CASES.iter().map(|c| c.file.to_string()).collect();
    listed.sort();
    assert_eq!(on_disk, listed, "work/pipeline-tests changed: update CASES");
}

#[test]
fn pipeline_documents_pass_or_are_known_failures() {
    let mut passed = 0;
    let mut known = 0;
    let mut bad = vec![];
    for c in CASES {
        let listed = KNOWN_FAILURES.iter().find(|(f, _)| *f == c.file);
        match (outcome(c), listed) {
            (Ok(()), None) => passed += 1,
            (Ok(()), Some(_)) => bad.push(format!(
                "{}: passes now, remove it from KNOWN_FAILURES",
                c.file
            )),
            (Err(_), Some((_, why))) => {
                known += 1;
                println!("known failure: {} ({why})", c.file);
            }
            (Err(e), None) => bad.push(format!("{}: {e}", c.file)),
        }
    }
    println!(
        "pipeline: {passed} passed, {known} known failures, {} total",
        CASES.len()
    );
    assert!(bad.is_empty(), "{}", bad.join("\n"));
}
