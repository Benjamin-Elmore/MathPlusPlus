//! The pipeline tests (M16): every document in `work/pipeline-tests/` against the
//! expectations written in its own header. This is the one harness; run it with
//! `cargo test -p mpp-cli --test pipeline -- --nocapture` (the table is printed).
//!
//! Each file is run through the parser, the evaluator and the proof checker, and every
//! expectation becomes a named check. A file passes when all its checks pass. The test
//! prints a table of every file with the reason for each failure.
//!
//! Numbers are compared at the precision each document writes them to (`0.2983` means
//! anything that rounds to it), or with the tolerance the document states.
//!
//! What fails the test in the default mode:
//! - a proof reported as more proved than its expectation (over-claiming);
//! - Navier–Stokes Theorem 6 coming back proved;
//! - a file that is not on the known-failures list failing;
//! - a file on the known-failures list passing (take it off the list).
//!
//! The known failures are files that need features still being built (each listed with
//! the reason). They are still run, never skipped. Set `MPP_PIPELINE_STRICT=1` to make
//! every failure fail the test.

use std::path::{Path, PathBuf};

use mpp_eval::{LineKind, LineResult, Session};
use mpp_proof::lean::{export, LeanConfig};
use mpp_proof::{check_document, DocumentCheck, StepStatus, Verdict};
use mpp_syntax::document::parse_document;

/// Files that fail today, and the short reason.
const KNOWN_FAILURES: &[(&str, &str)] = &[
    (
        "navier-stokes.mpp",
        "Lean export: ≈ steps on fields cannot be exported without sorry",
    ),
    (
        "historic/archimedes-parabola.mpp",
        "geometry steps the CAS cannot check",
    ),
    (
        "historic/cauchy-schwarz.mpp",
        "sums over indexed families a_i",
    ),
    (
        "historic/pythagoras.mpp",
        "area-counting premise; prose figure",
    ),
    ("historic/fermat-little.mpp", "\\overset{?}{\\equiv} checks"),
    ("labs/01-projectile-drag.mpp", "needs Ben: `T ≈ ?` has no definition, since T is introduced only by the prose \"lands when $y(T) = 0$ with $T > 0$\" (prose relations define nothing), so `R := x(T)` has no number either. Written in maths, `T := \\operatorname{solve}_t(y(t) = 0, t > 0)` gives T ≈ 2.757 s and R ≈ 34.07 m; R₀, T₀ and the drag solution x(t), y(t) (with units) are computed"),
    ("labs/02-pendulum.mpp", "needs Ben: `T ≈ ?` has no definition, since T is \"the time between successive maxima of θ(t)\" in prose only and there is no maths construct for it yet. Everything else passes: T₀ ≈ 2.006 s, T_exact ≈ 2.041 s, the series estimate, and θ(t) itself, now a numerical (RK45) solution with units (θ(T/2) = −30° to 10 digits)"),
    ("labs/03-damped-spring.mpp", "needs Ben: T_d = 2π/√39.84 s = 0.99545172… s, which rounds to 0.9955, not the expected 0.9954 (0.9954 is the value cut off after four decimals); everything else passes, including the ODE solution with units and the check dE/dt ≤ 0 for all t ≥ 0"),
    ("labs/06-reaction-kinetics.mpp", "needs Ben: a definition whose head is an expression (`\\frac{k(T_2)}{k(T_1)} := …` with where-clauses), bracket names `[A]_0` and the function `[A](t)`, and a fit of `ln k = ln A − (E_a/R)(1/T)` where E_a is already defined, with `E_{a,fit}` read as its fitted value"),
    ("labs/07-boyles-law.mpp", "needs Ben: the least-squares slope of the table is 99.30 J (within the stated 0.5 of 99.2), so the recovered amount is n_fit = 0.04006 mol, which rounds to 0.0401, not the expected 0.0400; and `R^2 ≈ ?` is the square of the gas constant R defined above, not the fit's coefficient of determination (the fit line reports R² ≈ 0.999985 itself)"),
    ("labs/11-monte-carlo-pi.mpp", "random variables are not built yet: `X_j, Y_j \\sim \\mathrm{Uniform}[0, 1]` declares nothing, so the seeded Monte Carlo sum for π̂ has no value (`\\hat\\pi` is now a name); and needs Ben: p is defined only in prose (`$p = \\pi/4$` in a sentence is not a definition), so σ and the 3σ check have no number"),
];

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("work")
        .join("pipeline-tests")
}

/// One named check of a file.
struct Check {
    what: String,
    result: Result<(), String>,
}

fn ok(what: impl Into<String>) -> Check {
    Check {
        what: what.into(),
        result: Ok(()),
    }
}

fn fail(what: impl Into<String>, why: impl Into<String>) -> Check {
    Check {
        what: what.into(),
        result: Err(why.into()),
    }
}

fn expect(what: impl Into<String>, cond: bool, why: impl Into<String>) -> Check {
    if cond {
        ok(what)
    } else {
        fail(what, why)
    }
}

/// What a result's summary must be.
#[derive(Debug, Clone, Copy)]
enum Want {
    Proved,
    Cited(usize),
    Structure,
    Gaps,
    NotProvedHere,
}

impl Want {
    fn verdict_matches(self, v: &Verdict) -> bool {
        match (self, v) {
            (Want::Proved, Verdict::Proved) => true,
            (Want::Cited(n), Verdict::ProvedUsingCited(m)) => n == *m,
            (Want::Structure, Verdict::ProvedStructureOnly) => true,
            (Want::Gaps, Verdict::Gaps(_)) => true,
            // More external results than expected is more cautious, never an over-claim.
            (Want::NotProvedHere, Verdict::NotProvedHere(_)) => true,
            _ => false,
        }
    }

    fn strength(self) -> u8 {
        match self {
            Want::Proved => Verdict::Proved.strength(),
            Want::Cited(n) => Verdict::ProvedUsingCited(n).strength(),
            Want::Structure => Verdict::ProvedStructureOnly.strength(),
            Want::Gaps => Verdict::Gaps(1).strength(),
            Want::NotProvedHere => Verdict::NotProvedHere(1).strength(),
        }
    }
}

/// Checks the summaries, and flags over-claims separately.
fn statuses(
    check: &DocumentCheck,
    wants: &[(&str, Want)],
    overclaims: &mut Vec<String>,
    file: &str,
) -> Vec<Check> {
    let mut out = Vec::new();
    for (name, want) in wants {
        let Some(r) = check.result(name) else {
            out.push(fail(
                format!("{name} status"),
                format!("{name} was not found"),
            ));
            continue;
        };
        let v = &r.summary.verdict;
        if v.strength() > want.strength() {
            overclaims.push(format!(
                "{file}: {name} reported \"{}\", expected {want:?}",
                r.summary.text
            ));
        }
        out.push(expect(
            format!("{name} status"),
            want.verdict_matches(v),
            format!("expected {want:?}, got \"{}\"", r.summary.text),
        ));
    }
    out
}

fn run_session(src: &str, path: &Path) -> Session {
    let mut s = Session::new();
    s.set_file(path);
    let _ = s.run_str(src);
    s
}

/// The statement line whose source starts with `fragment`.
fn line<'a>(s: &'a Session, fragment: &str) -> Option<&'a LineResult> {
    let ev = s.evaluated();
    let src = ev.source();
    let matching: Vec<&LineResult> = ev
        .blocks
        .iter()
        .flat_map(|b| b.values.iter())
        .filter(|l| {
            src.get(l.span.start..l.span.end)
                .is_some_and(|t| t.trim_start().starts_with(fragment))
        })
        .collect();
    // The same maths may appear in prose too; the line with an answer is the one meant,
    // and of `R_0 := … \approx ?` (a definition and its query) the query.
    matching
        .iter()
        .find(|l| matches!(l.kind, LineKind::Query { .. }) && l.answer().is_some())
        .or_else(|| matching.iter().find(|l| l.answer().is_some()))
        .or(matching.first())
        .copied()
}

/// The first diagnostic of the block holding `fragment`, for failure reasons.
fn diagnostic_near(s: &Session, fragment: &str) -> String {
    let ev = s.evaluated();
    let src = ev.source();
    let at = src.find(fragment);
    for b in &ev.blocks {
        if at.is_some_and(|a| a >= b.span.start && a <= b.span.end) {
            if let Some(d) = b.diagnostics.first() {
                return d.message.clone();
            }
        }
    }
    "no result was produced".into()
}

/// Reads a number from an answer such as `≈ 40.77 m`, `1.2097 × 10⁻⁴ yr⁻¹`, `= 5050`.
fn number_in(text: &str) -> Option<f64> {
    let t = text.replace('−', "-");
    let t = t.trim_start_matches(['≈', '=', ' ']);
    let mut num = String::new();
    let mut chars = t.chars().peekable();
    while let Some(&c) = chars.peek() {
        if c.is_ascii_digit() || c == '.' || (num.is_empty() && c == '-') {
            num.push(c);
            chars.next();
        } else {
            break;
        }
    }
    let mut x: f64 = num.parse().ok()?;
    let rest: String = chars.collect();
    // `40000/981 m` or `2*π` is an exact form, not a number.
    if rest.starts_with(['/', '*', '^', '('])
        || rest.chars().next().is_some_and(char::is_alphabetic) && !rest.starts_with(' ')
    {
        return None;
    }
    let rest = rest.trim_start();
    if let Some(exp) = rest
        .strip_prefix("× 10")
        .or_else(|| rest.strip_prefix("×10"))
    {
        let sup: String = exp
            .chars()
            .take_while(|c| "⁰¹²³⁴⁵⁶⁷⁸⁹⁻-0123456789^{}".contains(*c))
            .filter_map(|c| match c {
                '⁰' => Some('0'),
                '¹' => Some('1'),
                '²' => Some('2'),
                '³' => Some('3'),
                '⁴' => Some('4'),
                '⁵' => Some('5'),
                '⁶' => Some('6'),
                '⁷' => Some('7'),
                '⁸' => Some('8'),
                '⁹' => Some('9'),
                '⁻' | '-' => Some('-'),
                d if d.is_ascii_digit() => Some(d),
                _ => None,
            })
            .collect();
        let e: i32 = sup.parse().ok()?;
        x *= 10f64.powi(e);
    }
    Some(x)
}

fn value_of(l: &LineResult) -> Option<f64> {
    l.approx
        .as_deref()
        .and_then(single_number)
        .or_else(|| l.text.as_deref().and_then(single_number))
}

/// A number, or the one number in a one-element set (`≈ {4.605170186 s}`, the solution
/// set of an equation with a single root).
fn single_number(text: &str) -> Option<f64> {
    let t = text.trim_start_matches(['≈', '=', ' ']).trim_end();
    match t.strip_prefix('{').and_then(|r| r.strip_suffix('}')) {
        Some(inner) if !inner.contains(',') => number_in(inner),
        _ => number_in(text),
    }
}

/// The value written in `written` (`0.2983`, `1.2097e-4`, `7575`) and the tolerance
/// that precision means: half a unit in its last written decimal place, since the labs
/// state their results rounded to the digits they show.
fn written_precision(written: &str) -> Option<(f64, f64)> {
    let want: f64 = written.parse().ok()?;
    let (mantissa, exp) = match written.split_once(['e', 'E']) {
        Some((m, e)) => (m, e.parse::<i32>().ok()?),
        None => (written, 0),
    };
    let decimals = mantissa.split_once('.').map_or(0, |(_, d)| d.len()) as i32;
    Some((want, 0.5 * 10f64.powi(exp - decimals)))
}

/// `fragment ≈ written`, at the precision the document writes it to.
fn approx(s: &Session, fragment: &str, written: &str) -> Check {
    let what = format!("{fragment} ≈ {written}");
    match written_precision(written) {
        Some((want, tol)) => compare(s, what, fragment, want, tol),
        None => fail(what, "the expectation is not a number"),
    }
}

/// `fragment ≈ want ± tol`, for a tolerance the document states (or an exact value).
fn within(s: &Session, fragment: &str, want: f64, tol: f64) -> Check {
    compare(
        s,
        format!("{fragment} ≈ {want} ± {tol}"),
        fragment,
        want,
        tol,
    )
}

fn compare(s: &Session, what: String, fragment: &str, want: f64, tol: f64) -> Check {
    match line(s, fragment) {
        Some(l) => match value_of(l) {
            // A little slack so a value on the rounding boundary is not lost to floating point.
            Some(v) if (v - want).abs() <= tol * (1.0 + 1e-9) => ok(what),
            Some(v) => fail(what, format!("got {v}")),
            None => fail(
                what,
                format!(
                    "no number ({})",
                    l.answer().unwrap_or_else(|| diagnostic_near(s, fragment))
                ),
            ),
        },
        None => fail(what, diagnostic_near(s, fragment)),
    }
}

/// A check line (`\overset{?}{=}`) must pass.
fn check_passes(s: &Session, fragment: &str) -> Check {
    let what = format!("check {fragment}");
    match line(s, fragment) {
        Some(l) if l.kind == LineKind::Check => match &l.verdict {
            Some(v) if v.passed() == Some(true) => ok(what),
            Some(v) => fail(what, v.describe()),
            None => fail(what, diagnostic_near(s, fragment)),
        },
        _ => fail(what, diagnostic_near(s, fragment)),
    }
}

/// No error diagnostics from parsing or evaluation in the whole file.
fn no_errors(s: &Session) -> Check {
    let errs: Vec<String> = s
        .evaluated()
        .errors()
        .iter()
        .map(|d| d.message.clone())
        .collect();
    expect(
        "parses and runs with no errors",
        errs.is_empty(),
        format!(
            "{} error(s), first: {}",
            errs.len(),
            errs.first().cloned().unwrap_or_default()
        ),
    )
}

/// The steps of a result whose numbers are listed must be gaps; the others must not.
fn gap_steps(check: &DocumentCheck, name: &str, gaps: &[u32]) -> Check {
    let what = format!("{name}: steps {gaps:?} are gaps and nothing else");
    let Some(r) = check.result(name) else {
        return fail(what, "not found");
    };
    let Some(p) = r.proofs.first() else {
        return fail(what, "no proof");
    };
    for s in &p.steps {
        let n = s.number.unwrap_or(0);
        let is_gap = s.status == StepStatus::Gap;
        if gaps.contains(&n) != is_gap {
            return fail(what, format!("step {n} is {}", s.status.name()));
        }
    }
    ok(what)
}

/// Every step that has a relation to check is ✓ or ≈ (prose steps with nothing to
/// compute may be ◇; written gaps are covered by `gap_steps`).
fn computed_where_possible(check: &DocumentCheck, name: &str) -> Check {
    let what = format!("{name}: every step with a claim is ✓ or ≈");
    let Some(r) = check.result(name) else {
        return fail(what, "not found");
    };
    for p in &r.proofs {
        for s in &p.steps {
            if !s.claims.is_empty() && s.status != StepStatus::Gap && !s.status.is_computed() {
                return fail(
                    what,
                    format!("step {:?} is {} ({})", s.number, s.status.name(), s.detail),
                );
            }
        }
    }
    ok(what)
}

/// With the reference entry for `key` deleted, the step citing it becomes a gap.
fn deleted_citation(src: &str, key: &str, name: &str, step: u32) -> Check {
    let what = format!("without [@{key}], {name} step {step} is a gap");
    let Some(at) = src.find(&format!("- [@{key}]")) else {
        return fail(what, "the reference entry was not found");
    };
    let end = src[at..].find('\n').map_or(src.len(), |e| at + e + 1);
    let cut = format!("{}{}", &src[..at], &src[end..]);
    let c = mpp_proof::check_source(&cut);
    let status = c
        .result(name)
        .and_then(|r| r.proofs.first())
        .and_then(|p| p.steps.iter().find(|s| s.number == Some(step)))
        .map(|s| s.status);
    let summary = c.result(name).map(|r| r.summary.verdict.clone());
    expect(
        what,
        status == Some(StepStatus::Gap) && matches!(summary, Some(Verdict::Gaps(_))),
        format!("step is {status:?}, summary {summary:?}"),
    )
}

fn proof_file(rel: &str, overclaims: &mut Vec<String>) -> Vec<Check> {
    let path = root().join(rel);
    let src = match std::fs::read_to_string(&path) {
        Ok(s) => s,
        Err(e) => return vec![fail("read the file", e.to_string())],
    };
    let doc = parse_document(&src);
    let check = check_document(&doc, &src, &[]);
    let session = run_session(&src, &path);
    let s = &session;
    let mut out = Vec::new();
    match rel {
        "navier-stokes.mpp" => {
            let parse_errors: Vec<String> = doc.errors().map(|d| d.message.clone()).collect();
            out.push(expect(
                "parses with no errors",
                parse_errors.is_empty(),
                parse_errors.first().cloned().unwrap_or_default(),
            ));
            out.extend(statuses(
                &check,
                &[
                    ("Lemma 1", Want::Proved),
                    ("Lemma 2", Want::Gaps),
                    ("Lemma 3", Want::Proved),
                    ("Lemma 4", Want::Proved),
                    ("Lemma 5", Want::Proved),
                    ("Theorem 6", Want::NotProvedHere),
                ],
                overclaims,
                rel,
            ));
            for l in ["Lemma 1", "Lemma 2", "Lemma 3", "Lemma 4", "Lemma 5"] {
                out.push(computed_where_possible(&check, l));
            }
            out.push(gap_steps(&check, "Lemma 2", &[4, 5, 6, 7]));
            let t6 = check.result("Theorem 6");
            out.push(expect(
                "Theorem 6 is never proved",
                t6.is_some_and(|r| !r.summary.verdict.is_proved()),
                "Theorem 6 came back proved",
            ));
            // Lean export.
            let ex = export(&doc, &src, &check, "navier-stokes", &LeanConfig::default());
            let t6l = ex.theorem("Theorem 6");
            out.push(expect(
                "Lean: Theorem 6 exports as a statement with sorry, with alignment data",
                t6l.is_some_and(|t| {
                    t.statement_sorry && !t.alignment.mpp.is_empty() && !t.alignment.confirmed
                }) && ex.source().contains("theorem navier_stokes_theorem_6"),
                "Theorem 6 is missing from the export or was formalised without a person",
            ));
            for l in ["Lemma 3", "Lemma 4", "Lemma 5"] {
                let t = ex.theorem(l);
                let bad: Vec<u32> = t
                    .map(|t| {
                        t.steps
                            .iter()
                            .filter(|s| s.status.is_computed() && s.sorry)
                            .filter_map(|s| s.number)
                            .collect()
                    })
                    .unwrap_or_default();
                out.push(expect(
                    format!("Lean: {l} exports with no sorry on its checked steps"),
                    t.is_some() && bad.is_empty(),
                    format!(
                        "steps {bad:?} were checked numerically on a field instance (≈); vector calculus on fields has no Lean translation, so they export as sorry"
                    ),
                ));
            }
        }
        "historic/euclid-primes.mpp" => {
            out.extend(statuses(
                &check,
                &[("Theorem 1", Want::Cited(1))],
                overclaims,
                rel,
            ));
        }
        "historic/sqrt2-irrational.mpp" => {
            out.extend(statuses(
                &check,
                &[("Lemma 1", Want::Proved), ("Theorem 2", Want::Proved)],
                overclaims,
                rel,
            ));
        }
        "historic/pythagoras.mpp" => {
            out.extend(statuses(
                &check,
                &[("Theorem 1", Want::Cited(1))],
                overclaims,
                rel,
            ));
            out.push(no_errors(s));
        }
        "historic/gauss-sum.mpp" => {
            out.extend(statuses(
                &check,
                &[("Theorem 1", Want::Proved)],
                overclaims,
                rel,
            ));
            let both = check.result("Theorem 1").is_some_and(|r| {
                r.proofs.len() == 2
                    && r.proofs
                        .iter()
                        .all(|p| p.summary.verdict == Verdict::Proved)
            });
            out.push(expect(
                "both proofs are proved",
                both,
                "a proof is not proved",
            ));
            out.push(within(s, "\\sum_{k=1}^{100} k", 5050.0, 0.0));
        }
        "historic/fermat-little.mpp" => {
            out.extend(statuses(
                &check,
                &[("Lemma 1", Want::Proved), ("Theorem 2", Want::Proved)],
                overclaims,
                rel,
            ));
            out.push(check_passes(s, "3^{12}"));
        }
        "historic/cauchy-schwarz.mpp" => {
            out.extend(statuses(
                &check,
                &[("Theorem 1", Want::Proved)],
                overclaims,
                rel,
            ));
        }
        "historic/euler-basel.mpp" => {
            out.extend(statuses(
                &check,
                &[("Theorem 1", Want::Cited(1))],
                overclaims,
                rel,
            ));
            out.push(deleted_citation(&src, "weierstrass1876", "Theorem 1", 2));
            out.push(within(
                s,
                "\\sum_{k=1}^{\\infty}",
                std::f64::consts::PI.powi(2) / 6.0,
                1e-9,
            ));
            out.push(approx(s, "\\sum_{k=1}^{10^6}", "1.644933"));
        }
        "historic/archimedes-parabola.mpp" => {
            out.extend(statuses(
                &check,
                &[("Theorem 1", Want::Proved)],
                overclaims,
                rel,
            ));
        }
        "historic/euler-identity.mpp" => {
            out.extend(statuses(
                &check,
                &[
                    ("Theorem 1", Want::Cited(1)),
                    ("Corollary 2", Want::Cited(1)),
                ],
                overclaims,
                rel,
            ));
            out.push(within(s, "e^{i\\pi}", -1.0, 1e-12));
        }
        "historic/cantor-diagonal.mpp" => {
            out.extend(statuses(
                &check,
                &[("Theorem 1", Want::Cited(1))],
                overclaims,
                rel,
            ));
            out.push(deleted_citation(&src, "rudin", "Theorem 1", 2));
        }
        "historic/turing-halting.mpp" => {
            out.extend(statuses(
                &check,
                &[("Theorem 1", Want::Structure)],
                overclaims,
                rel,
            ));
            let ex = export(&doc, &src, &check, "turing-halting", &LeanConfig::default());
            let t = ex.theorem("Theorem 1");
            out.push(expect(
                "Lean: a statement and a skeleton with sorry on every step",
                t.is_some_and(|t| {
                    !t.steps.is_empty()
                        && t.steps.iter().all(|s| s.sorry)
                        && !t.alignment.mpp.is_empty()
                }),
                "the skeleton is missing",
            ));
        }
        _ => out.push(fail(
            "expectations",
            "no expectations are written for this file",
        )),
    }
    out
}

fn lab_file(rel: &str) -> Vec<Check> {
    let path = root().join(rel);
    let src = match std::fs::read_to_string(&path) {
        Ok(s) => s,
        Err(e) => return vec![fail("read the file", e.to_string())],
    };
    let session = run_session(&src, &path);
    let s = &session;
    let mut out = vec![no_errors(s)];
    // The labs state no theorems: the checker must not report any result on them.
    let check = mpp_proof::check_source(&src);
    out.push(expect(
        "the proof checker claims nothing",
        check.results.is_empty(),
        format!("{} result(s) reported", check.results.len()),
    ));
    match rel {
        "labs/01-projectile-drag.mpp" => {
            out.push(approx(s, "R_0", "40.77"));
            out.push(approx(s, "T_0", "2.883"));
            out.push(approx(s, "T \\approx", "2.757"));
            out.push(within(s, "R :=", 34.07, 0.05));
        }
        "labs/02-pendulum.mpp" => {
            out.push(approx(s, "T_0", "2.006"));
            out.push(within(s, "T \\approx", 2.041, 0.001));
            out.push(approx(s, "T_{\\text{exact}}", "2.041"));
            out.push(within(s, "T_{\\text{series}}", 2.041, 1e-4));
        }
        "labs/03-damped-spring.mpp" => {
            out.push(approx(s, "\\omega_0", "6.325"));
            out.push(approx(s, "\\zeta", "0.0632"));
            out.push(approx(s, "\\omega_d", "6.312"));
            out.push(approx(s, "T_d", "0.9954"));
            out.push(approx(s, "\\delta", "0.398"));
            out.push(check_passes(s, "\\frac{dE}{dt}"));
        }
        "labs/04-rc-circuit.mpp" => {
            out.push(within(s, "\\tau := RC", 1.0, 1e-9));
            out.push(approx(s, "V_C(\\tau)", "3.161"));
            out.push(approx(s, "\\operatorname{solve}_t", "4.605"));
            out.push(approx(s, "\\tau_{\\text{fit}}", "1.0"));
        }
        "labs/05-radioactive-decay.mpp" => {
            out.push(approx(s, "\\lambda :=", "1.2097e-4"));
            out.push(approx(s, "\\frac{N(10", "0.2983"));
            out.push(approx(s, "\\operatorname{solve}_t", "7575"));
        }
        "labs/06-reaction-kinetics.mpp" => {
            out.push(approx(s, "\\frac{k(T_2)}{k(T_1)} \\approx", "1.926"));
            out.push(approx(s, "[A](60", "0.0549"));
            out.push(approx(s, "E_{a,\\text{fit}}", "50"));
        }
        "labs/07-boyles-law.mpp" => {
            out.push(approx(s, "nRT", "99.15"));
            // The lab states the slope's tolerance: 0.5.
            out.push(within(s, "a \\approx", 99.2, 0.5));
            out.push(within(s, "b \\approx", 0.0, 1.0));
            out.push(within(s, "R^2", 0.9995, 0.0005));
            out.push(approx(s, "n_{\\text{fit}}", "0.0400"));
        }
        "labs/08-titration.mpp" => {
            out.push(approx(s, "V_{\\text{eq}}", "25.0"));
            out.push(approx(s, "\\mathrm{pH}(0)", "1.000"));
            out.push(approx(s, "\\mathrm{pH}(12.5", "1.477"));
            out.push(approx(s, "\\mathrm{pH}(25.0", "7.00"));
            out.push(approx(s, "\\mathrm{pH}(30.0", "11.959"));
        }
        "labs/09-predator-prey.mpp" => {
            let eq = line(s, "\\operatorname{solve}_{x, y}");
            let text = eq.and_then(|l| l.text.clone()).unwrap_or_default();
            out.push(expect(
                "equilibrium (4, 2.75)",
                text.contains("x = 4") && (text.contains("11/4") || text.contains("2.75")),
                format!("got {text:?}"),
            ));
            out.push(within(s, "\\max_{t", 0.0, 1e-6));
        }
        "labs/10-heat-rod.mpp" => {
            out.push(check_passes(s, "\\frac{\\partial u}{\\partial t}"));
            out.push(approx(s, "\\operatorname{solve}_t", "702.3"));
        }
        "labs/11-monte-carlo-pi.mpp" => {
            out.push(approx(s, "\\sigma :=", "0.00164"));
            out.push(check_passes(s, "|\\hat\\pi - \\pi| < 3\\sigma"));
        }
        _ => out.push(fail(
            "expectations",
            "no expectations are written for this file",
        )),
    }
    out
}

/// Every `.mpp` file under the folder except the index, as paths relative to it.
fn pipeline_files() -> Vec<String> {
    let mut out = Vec::new();
    for sub in ["", "historic", "labs"] {
        let dir = root().join(sub);
        let Ok(rd) = std::fs::read_dir(&dir) else {
            continue;
        };
        let mut names: Vec<String> = rd
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().to_string())
            .filter(|n| n.ends_with(".mpp") && n != "index.mpp")
            .collect();
        names.sort();
        for n in names {
            out.push(if sub.is_empty() {
                n
            } else {
                format!("{sub}/{n}")
            });
        }
    }
    out
}

#[test]
fn pipeline() {
    let strict = std::env::var("MPP_PIPELINE_STRICT").is_ok_and(|v| v == "1");
    let files = pipeline_files();
    assert_eq!(files.len(), 23, "the pipeline has 23 documents: {files:?}");
    let mut overclaims = Vec::new();
    let mut passed = 0;
    let mut table = String::new();
    let mut unexpected = Vec::new();
    let mut stale = Vec::new();
    for rel in &files {
        let checks = if rel.starts_with("labs/") {
            lab_file(rel)
        } else {
            proof_file(rel, &mut overclaims)
        };
        let failures: Vec<&Check> = checks.iter().filter(|c| c.result.is_err()).collect();
        let known = KNOWN_FAILURES.iter().find(|(f, _)| f == rel);
        if failures.is_empty() {
            passed += 1;
            table.push_str(&format!("PASS  {rel}\n"));
            if known.is_some() {
                table.push_str("      (now passes: take it off the known-failures list)\n");
                stale.push(rel.clone());
            }
        } else {
            table.push_str(&format!("FAIL  {rel}\n"));
            for f in &failures {
                if let Err(why) = &f.result {
                    table.push_str(&format!("      - {}: {why}\n", f.what));
                }
            }
            if known.is_none() {
                unexpected.push(rel.clone());
            }
        }
    }
    println!(
        "{table}\npipeline: {passed} of {} documents pass",
        files.len()
    );
    assert!(
        overclaims.is_empty(),
        "over-claimed proof statuses:\n{}",
        overclaims.join("\n")
    );
    assert!(
        unexpected.is_empty(),
        "files that should pass failed: {unexpected:?}\n{table}"
    );
    assert!(
        stale.is_empty(),
        "known failures that pass now (take them off KNOWN_FAILURES): {stale:?}\n{table}"
    );
    if strict {
        assert_eq!(
            passed,
            files.len(),
            "strict mode: every document must pass\n{table}"
        );
    }
}

#[test]
fn numbers_are_read_from_answers() {
    assert_eq!(number_in("≈ 40.77 m"), Some(40.77));
    assert_eq!(number_in("= 5050"), Some(5050.0));
    let x = number_in("≈ 1.2097 × 10⁻⁴ yr⁻¹").unwrap_or(0.0);
    assert!((x - 1.2097e-4).abs() < 1e-12);
    assert_eq!(number_in("= −1"), Some(-1.0));
    assert_eq!(single_number("≈ {4.605 s}"), Some(4.605));
    assert_eq!(single_number("= {1, 2}"), None);
}

#[test]
fn written_numbers_carry_their_precision() {
    assert_eq!(written_precision("7575"), Some((7575.0, 0.5)));
    let (x, tol) = written_precision("0.2983").unwrap_or_default();
    assert!((x - 0.2983).abs() < 1e-15 && (tol - 0.00005).abs() < 1e-15);
    let (x, tol) = written_precision("1.2097e-4").unwrap_or_default();
    assert!((x - 1.2097e-4).abs() < 1e-18 && (tol - 0.5e-8).abs() < 1e-20);
}
