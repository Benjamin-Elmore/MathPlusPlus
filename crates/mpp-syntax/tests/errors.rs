//! Errors point at the exact span, say what was expected, and the parser
//! recovers so later lines still parse.

use mpp_syntax::math::{parse_math, sexpr, Statement};
use mpp_syntax::Span;

/// Parses each input and records its diagnostics (with the source under each
/// span) and the tree it still returned.
fn diagnose(inputs: &[&str]) -> String {
    let mut out = String::new();
    for src in inputs {
        let (ast, diags) = parse_math(src, 0);
        assert!(!diags.is_empty(), "expected an error for {src:?}");
        out.push_str(&format!("## {src}\n"));
        for d in &diags {
            let under = src.get(d.span.start..d.span.end).unwrap_or("?");
            out.push_str(&format!(
                "  {}..{} {:?}: {}\n    expected: {}\n",
                d.span.start,
                d.span.end,
                under,
                d.message,
                d.expected.join(", ")
            ));
        }
        out.push_str(&format!(
            "  tree: {}\n",
            sexpr::ast(&ast, sexpr::Mode::Full).replace('\n', "\n        ")
        ));
    }
    out
}

#[test]
fn error_messages() {
    let report = diagnose(&[
        r"x^",
        r"\sum_{n=1}^ ",
        r"\frac{1}{",
        r"\frac{a}{b}}",
        r"\int_0^1 x^2",
        r"f(x,",
        r"(a + b",
        r"[1, 2",
        r"|x",
        r"\left( x",
        r"3 + * 4",
        r"#x",
        r"\foo x",
        r"\lim_{x} x",
        r"\lim x",
        r"x \to",
        r"\sum",
        // `\sin` alone names the function (prose about it); with a power it needs an argument.
        r"\sin^2",
        r"x :=",
        r"f(1) := 2",
        r"x^2 + 1 := 3",
        r"\begin{foo} x \end{foo}",
        r"\begin{pmatrix} 1 & 2",
        r"\begin{pmatrix} 1 \end{bmatrix}",
        r"root(3)",
        r"floor x",
        r"x = 3 )",
        r"(1, 2]x [",
        r"\left",
        r"x \label{a} \label{b}",
        r"\frac{}{2}",
    ]);
    insta::assert_snapshot!(report);
}

#[test]
fn recovers_at_comma_and_line_break() {
    let src = r"a := ) , b = 2 \\ c := 3";
    let (ast, diags) = parse_math(src, 0);
    assert_eq!(ast.lines.len(), 2);
    assert!(!diags.is_empty());
    assert!(matches!(
        ast.lines[1].statement,
        Statement::Definition { .. }
    ));
    assert_eq!(
        sexpr::ast(&ast, sexpr::Mode::Semantic),
        "(line (def := a (error)) (where b 2))\n(def := c 3)"
    );
}

#[test]
fn spans_use_the_base_offset() {
    let src = r"x + \alpha";
    let (ast, diags) = parse_math(src, 100);
    assert!(diags.is_empty());
    assert_eq!(ast.span, Span::new(100, 100 + src.len()));
    let Statement::Relation(node) = &ast.lines[0].statement else {
        panic!("expected an expression");
    };
    assert_eq!(node.span, Span::new(100, 110));
    let (_, diags) = parse_math("x^", 40);
    assert_eq!(diags[0].span, Span::point(42));
}

#[test]
fn error_spans_point_at_the_token() {
    let (_, diags) = parse_math(r"a + \foo + b", 7);
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].span, Span::new(11, 15));
    assert_eq!(diags[0].message, r"unknown command `\foo`");
}
