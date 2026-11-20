//! Helpers shared by the parser tests.

#![allow(dead_code)]

use mpp_syntax::math::{parse_math, sexpr, MathAst};

/// Parses `src` and fails the test if there are any diagnostics.
pub fn parse_clean(src: &str) -> MathAst {
    let (ast, diags) = parse_math(src, 0);
    assert!(
        diags.is_empty(),
        "unexpected diagnostics for {src:?}:\n{}",
        diags
            .iter()
            .map(|d| format!("  {d} (expected {:?})", d.expected))
            .collect::<Vec<_>>()
            .join("\n")
    );
    ast
}

/// Full (presentation) S-expression of a clean parse.
pub fn full(src: &str) -> String {
    sexpr::ast(&parse_clean(src), sexpr::Mode::Full)
}

/// Semantic S-expression of a clean parse.
pub fn semantic(src: &str) -> String {
    sexpr::ast(&parse_clean(src), sexpr::Mode::Semantic)
}

/// One construct written in several input forms (LaTeX first).
pub struct Case<'a> {
    pub name: &'a str,
    pub forms: &'a [&'a str],
}

/// Checks that every form of each case gives the same tree (ignoring
/// presentation) and returns a report of the full trees for a snapshot.
pub fn report(cases: &[Case<'_>]) -> String {
    let mut out = String::new();
    for case in cases {
        let first = case.forms[0];
        let want = semantic(first);
        let first_full = full(first);
        out.push_str(&format!(
            "## {}\n{first}\n  => {}\n",
            case.name,
            indent(&first_full)
        ));
        for form in &case.forms[1..] {
            let got = semantic(form);
            assert_eq!(
                got, want,
                "{}: `{form}` does not match `{first}`",
                case.name
            );
            let form_full = full(form);
            if form_full != first_full {
                out.push_str(&format!("{form}\n  => {}\n", indent(&form_full)));
            } else {
                out.push_str(&format!("{form}\n  => (same)\n"));
            }
        }
    }
    out
}

fn indent(s: &str) -> String {
    s.replace('\n', "\n     ")
}
