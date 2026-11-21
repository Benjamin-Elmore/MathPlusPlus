//! The standard library (`std/*.mpp`, embedded in the binary):
//! - every file runs with no errors, and every identity and worked example in it
//!   (`\overset{?}{=}`) is verified, symbolically or numerically at random points;
//! - every formula (function) has at least one worked example with its expected value;
//! - every constant matches its reference value (CODATA 2022, NIST, OEIS);
//! - every line of Ben's catalogue `work/stdlib-catalogue.mpp` maps to an entry in
//!   `std/catalogue-map.mpp`, and that entry is really in the file it names, in the
//!   form its class promises (compute, check or state);
//! - the library imports like any file, and the example document gives its answers.

use std::collections::BTreeSet;
use std::path::Path;
use std::sync::Arc;

use mpp_core::{Expr, ExprNode};
use mpp_eval::stdlib::{self, EntryKind};
use mpp_eval::{Binding, DefinitionKind, FileImporter, LineKind, QueryMode, Session};

const MAP: &str = "std/catalogue-map.mpp";

/// The library files that hold maths (everything but the catalogue map).
fn library() -> impl Iterator<Item = (&'static str, &'static str)> {
    stdlib::files().iter().copied().filter(|(p, _)| *p != MAP)
}

fn run(text: &str) -> mpp_eval::Evaluated {
    let mut session = Session::new();
    session.run_str(text).clone()
}

#[test]
fn the_library_has_every_file_of_the_plan() {
    let have: BTreeSet<&str> = stdlib::files().iter().map(|(p, _)| *p).collect();
    for name in [
        "prelude",
        "constants",
        "algebra",
        "trig",
        "hyperbolic",
        "calculus",
        "geometry",
        "linalg",
        "complex",
        "stats",
        "numbers",
        "physics",
        "finance",
        "number-theory",
        "analysis",
        "special",
        "discrete",
        "probability",
        "information",
        "algebra-structures",
        "topology-geometry",
        "applied",
        "catalogue-map",
    ] {
        let path = format!("std/{name}.mpp");
        assert!(have.contains(path.as_str()), "{path} is missing");
    }
}

#[test]
fn every_file_runs_cleanly_and_every_check_holds() {
    let mut problems = Vec::new();
    let mut checks = 0;
    for (path, text) in stdlib::files() {
        let ev = run(text);
        for d in ev.errors() {
            problems.push(format!(
                "{path}:{}: error: {}",
                ev.line_of(d.span.start),
                d.message
            ));
        }
        for (_, line) in ev.queries() {
            if line.kind != LineKind::Check {
                continue;
            }
            checks += 1;
            let ok = line.verdict.as_ref().and_then(|v| v.passed()) == Some(true);
            if !ok {
                let snippet = text.get(line.span.start..line.span.end).unwrap_or("");
                let verdict = line
                    .verdict
                    .as_ref()
                    .map_or_else(|| "no verdict".to_string(), |v| v.describe());
                problems.push(format!(
                    "{path}:{}: {snippet}  {verdict}",
                    ev.line_of(line.span.start)
                ));
            }
        }
    }
    assert!(problems.is_empty(), "\n{}", problems.join("\n"));
    assert!(checks > 400, "only {checks} checks in the library");
}

/// True when `e` applies the function `name` somewhere.
fn calls(e: &Expr, name: &str) -> bool {
    if let ExprNode::Apply(n, _) = e.node() {
        if &**n == name {
            return true;
        }
    }
    e.children().into_iter().any(|c| calls(c, name))
}

#[test]
fn every_formula_has_a_worked_example() {
    let mut missing = Vec::new();
    let mut formulas = 0;
    for (path, text) in library() {
        let ev = run(text);
        let lines: Vec<_> = ev.blocks.iter().flat_map(|b| b.values.iter()).collect();
        for line in &lines {
            let LineKind::Definition {
                name,
                kind: DefinitionKind::Function,
            } = &line.kind
            else {
                continue;
            };
            formulas += 1;
            let used = lines.iter().any(|l| {
                l.kind == LineKind::Check && l.input.as_ref().is_some_and(|e| calls(e, name))
            });
            if !used {
                missing.push(format!("{path}: {name}"));
            }
        }
    }
    assert!(
        missing.is_empty(),
        "formulas without a worked example:\n{}",
        missing.join("\n")
    );
    assert!(formulas > 150, "only {formulas} formulas in the library");
}

/// Reference values: (file, name, reference digits, source). The value of the name
/// defined in the file must match to double precision (an empty name: the value is
/// only written out in digits, in a check); for references written out to 50 digits,
/// a literal in the file must agree with every one of them.
const REFERENCES: &[(&str, &str, &str, &str)] = &[
    // Mathematical constants: OEIS, checked against mpmath to 60 digits.
    (
        "std/constants.mpp",
        "φ",
        "1.6180339887498948482045868343656381177203091798058",
        "OEIS A001622",
    ),
    (
        "std/constants.mpp",
        "γ",
        "0.57721566490153286060651209008240243104215933593992",
        "OEIS A001620",
    ),
    (
        "std/constants.mpp",
        "G",
        "0.91596559417721901505460351493238411077414937428167",
        "OEIS A006752",
    ),
    (
        "std/constants.mpp",
        "ζ_3",
        "1.2020569031595942853997381615114499907649862923405",
        "OEIS A002117",
    ),
    (
        "std/constants.mpp",
        "δ_F",
        "4.6692016091029906718532038204662016172581855774758",
        "OEIS A006890",
    ),
    (
        "std/constants.mpp",
        "α_F",
        "2.5029078750958928222839028732182157863812713767271",
        "OEIS A006891",
    ),
    (
        "std/constants.mpp",
        "",
        "0.69314718055994530941723212145817656807550013436026",
        "OEIS A002162",
    ),
    (
        "std/constants.mpp",
        "",
        "2.3025850929940456840179914546843642076011014886288",
        "OEIS A002392",
    ),
    (
        "std/constants.mpp",
        "",
        "1.4142135623730950488016887242096980785696718753769",
        "OEIS A002193",
    ),
    (
        "std/constants.mpp",
        "",
        "1.7320508075688772935274463415058723669428052538104",
        "OEIS A002194",
    ),
    (
        "std/prelude.mpp",
        "",
        "3.1415926535897932384626433832795028841971693993751",
        "OEIS A000796",
    ),
    (
        "std/prelude.mpp",
        "",
        "2.7182818284590452353602874713526624977572470936999",
        "OEIS A001113",
    ),
    // Physical constants: CODATA 2022 (NIST, May 2024); SI-exact ones are exact.
    ("std/physics.mpp", "c", "299792458", "CODATA 2022, exact"),
    (
        "std/physics.mpp",
        "h",
        "6.62607015e-34",
        "CODATA 2022, exact",
    ),
    (
        "std/physics.mpp",
        "ℏ",
        "1.054571817e-34",
        "CODATA 2022 (exact, shown truncated)",
    ),
    (
        "std/physics.mpp",
        "G",
        "6.67430e-11",
        "CODATA 2022, ±0.00015e-11",
    ),
    ("std/physics.mpp", "g_0", "9.80665", "CGPM 1901, exact"),
    (
        "std/physics.mpp",
        "k_B",
        "1.380649e-23",
        "CODATA 2022, exact",
    ),
    (
        "std/physics.mpp",
        "N_A",
        "6.02214076e23",
        "CODATA 2022, exact",
    ),
    (
        "std/physics.mpp",
        "R",
        "8.314462618",
        "CODATA 2022 (exact, shown truncated)",
    ),
    (
        "std/physics.mpp",
        "q_e",
        "1.602176634e-19",
        "CODATA 2022, exact",
    ),
    (
        "std/physics.mpp",
        "ε_0",
        "8.8541878188e-12",
        "CODATA 2022, ±0.0000000014e-12",
    ),
    (
        "std/physics.mpp",
        "μ_0",
        "1.25663706127e-6",
        "CODATA 2022, ±0.00000000020e-6",
    ),
    (
        "std/physics.mpp",
        "m_e",
        "9.1093837139e-31",
        "CODATA 2022, ±0.0000000028e-31",
    ),
    (
        "std/physics.mpp",
        "m_p",
        "1.67262192595e-27",
        "CODATA 2022, ±0.00000000052e-27",
    ),
    (
        "std/physics.mpp",
        "σ_SB",
        "5.670374419e-8",
        "CODATA 2022 (exact, shown truncated)",
    ),
    (
        "std/physics.mpp",
        "α_fs",
        "7.2973525643e-3",
        "CODATA 2022, ±0.0000000011e-3",
    ),
];

/// The plain number of a binding, with any unit factor taken as 1.
fn value_of(b: &Binding) -> Option<f64> {
    let Binding::Variable { value, .. } = b else {
        return None;
    };
    let plain = strip_units(value);
    plain.eval_f64(&mpp_core::EmptyEnv).ok()
}

fn strip_units(e: &Expr) -> Expr {
    match e.node() {
        ExprNode::Apply(name, args) if name.starts_with("unit") && args.is_empty() => Expr::one(),
        ExprNode::Apply(name, args) if name.starts_with("unit") && args.len() == 2 => {
            strip_units(&args[0])
        }
        _ => Expr::new(e.map_children(&mut |c| strip_units(c))),
    }
}

#[test]
fn constants_match_their_reference_values() {
    let mut problems = Vec::new();
    for (file, name, reference, source) in REFERENCES {
        let Some(text) = stdlib::source(file) else {
            problems.push(format!("{file} is missing"));
            continue;
        };
        if !name.is_empty() {
            let ev = run(text);
            let want: f64 = reference.parse().unwrap_or(f64::NAN);
            match ev.bindings.get(*name).and_then(|b| value_of(b)) {
                Some(got) if ((got - want) / want).abs() < 1e-9 => {}
                other => problems.push(format!(
                    "{file}: {name} = {other:?}, but {source} gives {reference}"
                )),
            }
        }
        // Long references: the digits in the file must agree with all of them.
        if reference.len() > 40 {
            let digits: String = reference.chars().filter(char::is_ascii_digit).collect();
            let file_digits = text
                .split(|c: char| !(c.is_ascii_digit() || c == '.'))
                .filter(|w| w.len() > 40)
                .map(|w| w.chars().filter(char::is_ascii_digit).collect::<String>())
                .any(|w| w.starts_with(&digits[..digits.len() - 1]));
            if !file_digits {
                problems.push(format!(
                    "{file}: no 50-digit literal for {name} agrees with {source}"
                ));
            }
        }
    }
    assert!(problems.is_empty(), "\n{}", problems.join("\n"));
}

/// The catalogue lines that need an entry: every non-blank line of the fenced list
/// except the section and subsection titles (no indent and no `=`).
fn catalogue_items() -> Vec<String> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../work/stdlib-catalogue.mpp");
    let text = std::fs::read_to_string(&path).unwrap_or_default();
    let body = text.split("```").nth(1).unwrap_or("");
    body.lines()
        .skip(1)
        .filter(|l| !l.trim().is_empty())
        .filter(|l| l.starts_with(char::is_whitespace) || l.contains('='))
        .map(|l| l.trim().to_string())
        .collect()
}

/// A table row split at unescaped `|`, with `\|` and `\*` unescaped.
fn cells(row: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut chars = row.trim().trim_start_matches('|').chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\\' if matches!(chars.peek(), Some('|' | '*' | '\\' | '[' | ']' | '#')) => {
                if let Some(n) = chars.next() {
                    cur.push(n);
                }
            }
            '|' => out.push(std::mem::take(&mut cur).trim().to_string()),
            _ => cur.push(c),
        }
    }
    if !cur.trim().is_empty() {
        out.push(cur.trim().to_string());
    }
    out
}

/// Strips the backticks around a code cell.
fn code(cell: &str) -> &str {
    cell.trim().trim_matches('`')
}

#[test]
fn every_catalogue_line_is_covered() {
    let items = catalogue_items();
    assert!(
        items.len() > 150,
        "read only {} catalogue lines",
        items.len()
    );
    let map = stdlib::source(MAP).unwrap_or("");
    let rows: Vec<Vec<String>> = map
        .lines()
        .filter(|l| l.starts_with('|') && !l.starts_with("|---") && !l.starts_with("| Catalogue"))
        .map(cells)
        .collect();

    let mut problems = Vec::new();
    let mut counts = [0usize; 3];
    for item in &items {
        let Some(row) = rows
            .iter()
            .find(|r| r.first().map(String::as_str) == Some(item))
        else {
            problems.push(format!("not in the map: {item}"));
            continue;
        };
        let [_, class, file, entry] = row.as_slice() else {
            problems.push(format!("row needs 4 cells: {item}"));
            continue;
        };
        let file = code(file);
        let entry = code(entry);
        let Some(text) = stdlib::source(file) else {
            problems.push(format!("{item}: {file} is not in the library"));
            continue;
        };
        let lines: Vec<&str> = text.lines().filter(|l| l.contains(entry)).collect();
        let ok = match class.as_str() {
            "compute" => {
                counts[0] += 1;
                lines.iter().any(|l| l.trim_start().starts_with("$$"))
            }
            "check" => {
                counts[1] += 1;
                lines.iter().any(|l| l.contains("\\overset{?}{=}"))
            }
            "state" => {
                counts[2] += 1;
                lines.iter().any(|l| l.starts_with("**Theorem.**"))
            }
            other => {
                problems.push(format!("{item}: unknown class {other}"));
                continue;
            }
        };
        if !ok {
            problems.push(format!(
                "{item}: `{entry}` is not a {class} entry in {file}"
            ));
        }
    }
    let known: BTreeSet<&String> = items.iter().collect();
    for row in &rows {
        if let Some(first) = row.first() {
            if !known.contains(first) {
                problems.push(format!(
                    "the map has a row for a line not in the catalogue: {first}"
                ));
            }
        }
    }
    assert!(problems.is_empty(), "\n{}", problems.join("\n"));
    println!(
        "catalogue: {} lines; compute {}, check {}, state {}",
        items.len(),
        counts[0],
        counts[1],
        counts[2]
    );
}

#[test]
fn the_library_imports_like_any_file() {
    let ev = run(concat!(
        "Using $\\varphi$ from [[std/constants.mpp]].\n\n",
        "$$ \\varphi^2 \\overset{?}{=} \\varphi + 1 $$\n\n",
        "![[std/geometry.mpp]]\n\n$A_{\\text{circle}}(3) = ?$\n",
    ));
    assert!(ev.errors().is_empty(), "{}", ev.report());
    let answers: Vec<String> = ev.queries().filter_map(|(_, l)| l.answer()).collect();
    assert_eq!(answers, ["✓ (proved symbolically)", "= 9*π ≈ 28.27433388"]);
    // A name the document defines itself is not overwritten by a whole-file import.
    let own = run("$$ A_{\\text{circle}}(r) := 2 r $$\n\n![[std/geometry.mpp]]\n\n$A_{\\text{circle}}(3) = ?$\n");
    let last = own.queries().last().and_then(|(_, l)| l.text.clone());
    assert_ne!(last.as_deref(), Some("9*π"), "{}", own.report());
}

#[test]
fn the_index_lists_every_file() {
    let index = stdlib::index();
    for (path, _) in library() {
        assert!(
            index.iter().any(|e| e.file == path),
            "{path} has no index entries"
        );
    }
    for kind in [
        EntryKind::Constant,
        EntryKind::Function,
        EntryKind::Identity,
        EntryKind::Theorem,
    ] {
        assert!(index.iter().any(|e| e.kind == kind), "no {kind:?} entries");
    }
    let unexplained: Vec<String> = index
        .iter()
        .filter(|e| matches!(e.kind, EntryKind::Function | EntryKind::Constant))
        .filter(|e| e.description.is_empty())
        .map(|e| format!("{}:{} {}", e.file, e.line, e.name))
        .collect();
    assert!(
        unexplained.is_empty(),
        "entries without a description:\n{}",
        unexplained.join("\n")
    );
}

#[test]
fn textbook_names_reach_the_field_functions() {
    let ev = run(concat!(
        "$\\tau(360) = ?$, $\\sigma(28) = ?$, $\\sigma_2(6) = ?$, $\\mu(30) = ?$, ",
        "$\\varphi(12) = ?$, $\\zeta(2) = ?$, $P_2(x) = ?$, $v_2(48) = ?$, ",
        "$B(\\tfrac{1}{2}, \\tfrac{1}{2}) = ?$, $K(0) = ?$, $H(2) = ?$\n\n",
        "$$ \\tau := 3 $$\n\n$\\tau(2) = ?$\n",
    ));
    assert!(ev.errors().is_empty(), "{}", ev.report());
    let answers: Vec<String> = ev.queries().filter_map(|(_, l)| l.text.clone()).collect();
    assert_eq!(
        answers,
        [
            "24",
            "56",
            "50",
            "-1",
            "4",
            "π^2/6",
            "3*x^2/2 - 1/2",
            "4",
            "π",
            "π/2",
            "1",
            "6"
        ]
    );
}

#[test]
fn seeds_make_samples_reproducible() {
    let doc = "%seed 42\n\n$\\operatorname{runif}(3) = ?$\n\n$\\operatorname{runif}(3) = ?$\n";
    let a = run(doc);
    let b = run(doc);
    let first: Vec<Option<String>> = a.queries().map(|(_, l)| l.text.clone()).collect();
    let again: Vec<Option<String>> = b.queries().map(|(_, l)| l.text.clone()).collect();
    assert_eq!(first, again);
    assert_ne!(
        first[0], first[1],
        "two calls should draw different samples"
    );
    let other = run("%seed 7\n\n$\\operatorname{runif}(3) = ?$\n");
    assert_ne!(
        other.queries().next().and_then(|(_, l)| l.text.clone()),
        first[0]
    );
    let bad = run("%seed x\n");
    assert_eq!(bad.errors().len(), 1);
    let unseeded = run("$\\operatorname{runif}(2) = ?$\n");
    assert!(unseeded.report().contains("seed is 0"));
}

/// The answers of a document in order (as `tests/examples.rs` reads them).
fn answers_of(path: &Path) -> Vec<String> {
    let src = std::fs::read_to_string(path).unwrap_or_default();
    let mut session = Session::new().with_importer(Arc::new(FileImporter::new()));
    session.set_file(path);
    let ev = session.run_str(&src).clone();
    let mut items: Vec<(usize, String)> = Vec::new();
    for (_, line) in ev.queries() {
        let approx = matches!(
            line.kind,
            LineKind::Query {
                mode: QueryMode::Approx,
                ..
            }
        );
        let text = if line.kind == LineKind::Check {
            line.verdict.as_ref().map(|v| v.mark().to_string())
        } else if approx {
            line.approx.clone().or_else(|| line.text.clone())
        } else {
            line.text.clone()
        };
        items.push((line.span.start, text.unwrap_or_else(|| "<none>".into())));
    }
    for d in ev.errors() {
        items.push((d.span.start, d.message.clone()));
    }
    items.sort_by_key(|(at, _)| *at);
    items.into_iter().map(|(_, t)| t).collect()
}

#[test]
fn standard_library_example() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples");
    let expected =
        std::fs::read_to_string(root.join("standard-library.expected.txt")).unwrap_or_default();
    let want: Vec<&str> = expected
        .lines()
        .filter_map(|l| l.rsplit_once('→').map(|(_, r)| r.trim()))
        .collect();
    assert!(!want.is_empty());
    assert_eq!(answers_of(&root.join("standard-library.mpp")), want);
}
