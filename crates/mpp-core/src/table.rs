//! Labelled tables as values: a curve-sketch summary, extrema with their kinds, the two
//! sides of a theorem check.
//!
//! A table is the expression `table((label₁, value₁), (label₂, value₂), …)`, each label a
//! symbol holding the row's words. It flows through the evaluator like any other value,
//! and [`rows`] reads it back so a renderer can lay it out as a two-column table.

use std::fmt::Write as _;

use crate::expr::{Expr, ExprNode};

/// The `Apply` name of a table.
pub const TABLE: &str = "table";

/// A table with one row per `(label, value)`.
pub fn table(rows: Vec<(String, Expr)>) -> Expr {
    Expr::apply(
        TABLE,
        rows.into_iter()
            .map(|(label, value)| Expr::new(ExprNode::Tuple(vec![Expr::sym(&label), value])))
            .collect(),
    )
}

/// The rows of a table, or `None` when `e` is not one.
pub fn rows(e: &Expr) -> Option<Vec<(String, Expr)>> {
    let ExprNode::Apply(name, items) = e.node() else {
        return None;
    };
    if &**name != TABLE {
        return None;
    }
    items
        .iter()
        .map(|row| match row.node() {
            ExprNode::Tuple(pair) => match pair.as_slice() {
                [label, value] => label.as_sym().map(|l| (l.to_string(), value.clone())),
                _ => None,
            },
            _ => None,
        })
        .collect()
}

/// The value in the row labelled `label`.
pub fn row(e: &Expr, label: &str) -> Option<Expr> {
    rows(e)?
        .into_iter()
        .find(|(l, _)| l == label)
        .map(|(_, v)| v)
}

/// Plain text: one `label: value` line per row.
pub fn to_text(rows: &[(String, Expr)]) -> String {
    let mut out = String::new();
    for (i, (label, value)) in rows.iter().enumerate() {
        if i > 0 {
            out.push('\n');
        }
        let _ = write!(out, "{label}: {}", show_plain(value));
    }
    out
}

/// Unicode text: [`to_text`] with each value as [`Expr::to_text`] reads it.
pub fn to_unicode(rows: &[(String, Expr)]) -> String {
    let mut out = String::new();
    for (i, (label, value)) in rows.iter().enumerate() {
        if i > 0 {
            out.push('\n');
        }
        let shown = match value.node() {
            ExprNode::Set(items) if items.is_empty() => "none".into(),
            ExprNode::Bool(_) => show_plain(value),
            _ => value.to_text(),
        };
        let _ = write!(out, "{label}: {shown}");
    }
    out
}

/// LaTeX: a two-column `array` with the labels as text.
pub fn to_latex(rows: &[(String, Expr)]) -> String {
    let body: Vec<String> = rows
        .iter()
        .map(|(label, value)| format!("\\text{{{}}} & {}", escape(label), show_latex(value)))
        .collect();
    format!(
        "\\begin{{array}}{{ll}} {} \\end{{array}}",
        body.join(" \\\\ ")
    )
}

/// A row value as text: words for "none" and the truth values.
fn show_plain(value: &Expr) -> String {
    match value.node() {
        ExprNode::Set(items) if items.is_empty() => "none".into(),
        ExprNode::Bool(true) => "yes".into(),
        ExprNode::Bool(false) => "no".into(),
        _ => value.to_string(),
    }
}

fn show_latex(value: &Expr) -> String {
    match value.node() {
        ExprNode::Set(items) if items.is_empty() => "\\text{none}".into(),
        ExprNode::Bool(true) => "\\text{yes}".into(),
        ExprNode::Bool(false) => "\\text{no}".into(),
        _ => value.to_latex(),
    }
}

fn escape(s: &str) -> String {
    s.chars()
        .flat_map(|c| match c {
            '{' | '}' | '_' | '&' | '%' | '$' | '#' => vec!['\\', c],
            _ => vec![c],
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_and_text() {
        let t = table(vec![
            ("domain".into(), Expr::int(1)),
            ("critical points".into(), Expr::new(ExprNode::Set(vec![]))),
            ("continuous".into(), Expr::new(ExprNode::Bool(true))),
        ]);
        let r = rows(&t).unwrap();
        assert_eq!(r.len(), 3);
        assert_eq!(row(&t, "domain"), Some(Expr::int(1)));
        assert_eq!(
            to_text(&r),
            "domain: 1\ncritical points: none\ncontinuous: yes"
        );
        assert!(to_latex(&r).starts_with("\\begin{array}{ll} \\text{domain} & 1"));
        assert!(rows(&Expr::int(3)).is_none());
    }
}
