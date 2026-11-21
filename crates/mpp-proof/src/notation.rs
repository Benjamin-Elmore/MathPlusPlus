//! Reading claims out of maths: the relations in a span, the names they use, and the
//! notation the CAS cannot take as written (ellipses, indexed families, `\pmod`).

use std::collections::BTreeSet;

use mpp_syntax::math::visit::children;
use mpp_syntax::math::{BinOp, Node, NodeKind, RelOp, UnaryOp};
use mpp_syntax::Span;

/// The source text under `span` (empty when out of range).
pub(crate) fn text(src: &str, span: Span) -> &str {
    src.get(span.start..span.end).unwrap_or("")
}

/// Calls `f` on `node` and every node below it.
pub(crate) fn walk<'n>(node: &'n Node, f: &mut dyn FnMut(&'n Node)) {
    f(node);
    children(node, &mut |c| walk(c, f));
}

/// The names `node` uses, as written (`p`, `p_i`, `\theta`, `\mathbf u`), without the
/// indices that a sum binds.
pub(crate) fn names(node: &Node, src: &str) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    let mut bound = BTreeSet::new();
    walk(node, &mut |n| match &n.kind {
        NodeKind::Name(_) => {
            out.insert(node_text(n, src).trim().to_string());
        }
        NodeKind::BigOp(b) => {
            if let Some(i) = &b.index {
                bound.insert(node_text(i, src).trim().to_string());
            }
            if let Some(over) = &b.over {
                // `\sum_{i,j}` and `\sum_{k \ge 1}` bind their names too.
                walk(over, &mut |m| {
                    if let NodeKind::Name(_) = &m.kind {
                        bound.insert(node_text(m, src).trim().to_string());
                    }
                });
            }
        }
        _ => {}
    });
    // A name that is only a subscript's letter (`i` in `p_i`) is counted with its base.
    out.retain(|n| !bound.contains(n));
    let whole: Vec<String> = out.iter().filter(|n| n.contains('_')).cloned().collect();
    out.retain(|n| {
        n.contains('_')
            || !whole.iter().any(|w| {
                w.split('_')
                    .nth(1)
                    .is_some_and(|s| s.trim_matches(['{', '}']) == n)
            })
    });
    out
}

/// True when the node holds an ellipsis (`\dots`, `\cdots`).
pub(crate) fn has_dots(node: &Node) -> bool {
    let mut found = false;
    walk(node, &mut |n| {
        if matches!(n.kind, NodeKind::Dots(_)) {
            found = true;
        }
    });
    found
}

/// True when a sum or product runs over an indexed family (`\sum_i a_i^2`, `\sum a_i
/// b_i`): the CAS reads `a_i` as one fixed symbol, so such sums cannot be computed yet.
pub(crate) fn has_indexed_family(node: &Node, src: &str) -> bool {
    let mut found = false;
    walk(node, &mut |n| {
        let NodeKind::BigOp(b) = &n.kind else {
            return;
        };
        let mut index_names = BTreeSet::new();
        if let Some(i) = &b.index {
            index_names.insert(text(src, i.span).trim().to_string());
        }
        if let Some(over) = &b.over {
            walk(over, &mut |m| {
                if let NodeKind::Name(_) = &m.kind {
                    index_names.insert(text(src, m.span).trim().to_string());
                }
            });
        }
        walk(&b.body, &mut |m| {
            if let NodeKind::Name(name) = &m.kind {
                if let Some(sub) = &name.sub {
                    let s = text(src, sub.span).trim().to_string();
                    if index_names.is_empty() || index_names.contains(&s) {
                        found = true;
                    }
                }
            }
        });
    });
    found
}

/// The relation operators the checker can test.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ClaimOp {
    Eq,
    Lt,
    Le,
    Gt,
    Ge,
    /// `≡ (mod n)`.
    Congruent,
    /// `a ∣ b`.
    Divides,
}

impl ClaimOp {
    fn from_rel(op: RelOp) -> Option<ClaimOp> {
        Some(match op {
            RelOp::Eq => ClaimOp::Eq,
            RelOp::Lt => ClaimOp::Lt,
            RelOp::Le => ClaimOp::Le,
            RelOp::Gt => ClaimOp::Gt,
            RelOp::Ge => ClaimOp::Ge,
            RelOp::Equiv => ClaimOp::Congruent,
            RelOp::Other(s) if s == "∣" || s == "|" => ClaimOp::Divides,
            _ => return None,
        })
    }
}

/// One link of a relation chain: `lhs op rhs`.
#[derive(Debug, Clone)]
pub(crate) struct Link {
    pub lhs: Node,
    pub op: ClaimOp,
    pub rhs: Node,
    /// The modulus of a congruence (from a `\pmod` anywhere in the chain), as LaTeX.
    pub modulus: Option<String>,
    pub span: Span,
}

/// The testable links of a relation chain (`a = b = c` gives `a = b` and `b = c`).
/// Links with an operator the checker does not test (`∈`, `≠`, `→`) are left out.
pub(crate) fn links(node: &Node, src: &str) -> Vec<Link> {
    let NodeKind::Relation { first, rest } = &node.ungrouped().kind else {
        return Vec::new();
    };
    let chain_text = text(src, node.span);
    let modulus = pmod_of(chain_text).map(|(_, m)| m);
    let mut out = Vec::new();
    let mut prev: &Node = first;
    for (op, next) in rest {
        if let Some(op) = ClaimOp::from_rel(*op) {
            out.push(Link {
                lhs: prev.clone(),
                op,
                rhs: next.clone(),
                modulus: modulus.clone(),
                span: prev.span.join(next.span),
            });
        }
        prev = next;
    }
    out
}

/// Splits `… \pmod{n}` (or `\pmod n`, `\bmod`) off the end of `text`: the text before it
/// and the modulus.
pub(crate) fn pmod_of(text: &str) -> Option<(String, String)> {
    let at = text.rfind("\\pmod")?;
    let before = text[..at].trim_end().to_string();
    let rest = text[at + "\\pmod".len()..].trim_start();
    let modulus = if let Some(inner) = rest.strip_prefix('{') {
        let end = inner.find('}')?;
        inner[..end].trim().to_string()
    } else {
        rest.split(|c: char| c.is_whitespace() || c == ')' || c == '$')
            .next()
            .unwrap_or("")
            .to_string()
    };
    (!modulus.is_empty()).then_some((before, modulus))
}

/// The source of `node`, extended over closing braces its span stops short of (a
/// script's span ends before the `}` of `x^{n+1}`).
pub(crate) fn node_text(node: &Node, src: &str) -> String {
    let mut end = node.span.end;
    let t = text(src, node.span);
    let mut open = 0i64;
    let mut escaped = false;
    for c in t.chars() {
        if escaped {
            escaped = false;
            continue;
        }
        match c {
            '\\' => escaped = true,
            '{' => open += 1,
            '}' => open -= 1,
            _ => {}
        }
    }
    let bytes = src.as_bytes();
    while open > 0 {
        let mut j = end;
        while bytes.get(j).is_some_and(|b| *b == b' ') {
            j += 1;
        }
        if bytes.get(j) == Some(&b'}') {
            end = j + 1;
            open -= 1;
        } else {
            break;
        }
    }
    text(src, Span::new(node.span.start, end)).to_string()
}

/// The source of `node` with any `\pmod` removed.
pub(crate) fn side_text(node: &Node, src: &str) -> String {
    let t = node_text(node, src);
    let t = t.as_str();
    let t = match pmod_of(t) {
        Some((before, _)) => before,
        None => t.to_string(),
    };
    normalise(&t)
}

/// Rewrites notation the CAS reads differently: `\sum_{k \ge 1}` becomes
/// `\sum_{k=1}^{\infty}` (a sum over a set is not supported yet; this one is a range).
pub(crate) fn normalise(t: &str) -> String {
    let mut out = String::with_capacity(t.len());
    let mut rest = t;
    while let Some(at) = rest.find("\\sum_{") {
        out.push_str(&rest[..at + "\\sum_{".len()]);
        let after = &rest[at + "\\sum_{".len()..];
        let Some(close) = after.find('}') else {
            rest = after;
            continue;
        };
        let inner = &after[..close];
        let rewritten = ["\\geq", "\\ge", "≥"].iter().find_map(|op| {
            let (var, from) = inner.split_once(op)?;
            let (var, from) = (var.trim(), from.trim());
            (!var.is_empty() && !from.is_empty() && !var.contains(['=', ',']))
                .then(|| format!("{var}={from}"))
        });
        match rewritten {
            Some(r) => {
                out.push_str(&r);
                out.push('}');
                rest = &after[close + 1..];
                if !rest.trim_start().starts_with('^') {
                    out.push_str("^{\\infty}");
                }
            }
            None => {
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

/// A term of an additive chain, with its sign.
#[derive(Debug, Clone)]
pub(crate) struct Term<'n> {
    pub negative: bool,
    pub node: &'n Node,
}

/// Flattens `a + b - c + …` into its terms.
pub(crate) fn additive_terms(node: &Node) -> Vec<Term<'_>> {
    fn go<'n>(node: &'n Node, negative: bool, out: &mut Vec<Term<'n>>) {
        match &node.kind {
            NodeKind::Binary {
                op: BinOp::Add,
                lhs,
                rhs,
            } => {
                go(lhs, negative, out);
                go(rhs, negative, out);
            }
            NodeKind::Binary {
                op: BinOp::Sub,
                lhs,
                rhs,
            } => {
                go(lhs, negative, out);
                go(rhs, !negative, out);
            }
            NodeKind::Unary {
                op: UnaryOp::Neg,
                operand,
            } if out.is_empty() => go(operand, !negative, out),
            _ => out.push(Term { negative, node }),
        }
    }
    let mut out = Vec::new();
    go(node, false, &mut out);
    out
}

/// A sum written with an ellipsis whose terms form an arithmetic progression:
/// `1 + 2 + \dots + n`, `n + (n-1) + \dots + 1`. Returns the first, second and last terms.
pub(crate) fn arithmetic_ellipsis(node: &Node) -> Option<(&Node, &Node, &Node)> {
    let terms = additive_terms(node.ungrouped());
    let dots = terms
        .iter()
        .position(|t| matches!(t.node.kind, NodeKind::Dots(_)))?;
    if dots != 2 || terms.len() != 4 || terms.iter().any(|t| t.negative) {
        return None;
    }
    let first = terms.first()?.node;
    let second = terms.get(1)?.node;
    let last = terms.get(3)?.node;
    Some((first, second, last))
}

/// A series written with a trailing ellipsis, `1 - \frac{x^2}{6} + \dots`: the part
/// before the ellipsis.
pub(crate) fn truncated_series(node: &Node) -> Option<&Node> {
    match &node.ungrouped().kind {
        NodeKind::Binary {
            op: BinOp::Add | BinOp::Sub,
            lhs,
            rhs,
        } if matches!(rhs.kind, NodeKind::Dots(_)) && !has_dots(lhs) => Some(lhs),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mpp_syntax::math::{parse_expr, parse_math, Statement};

    fn expr(src: &str) -> Node {
        parse_expr(src, 0).0
    }

    #[test]
    fn names_skip_bound_indices() {
        let src = "\\sum_{k=1}^{n} k + p_i + \\theta";
        let n = names(&expr(src), src);
        let v: Vec<&str> = n.iter().map(String::as_str).collect();
        assert_eq!(v, vec!["\\theta", "n", "p_i"]);
    }

    #[test]
    fn pmod_and_sums() {
        assert_eq!(
            pmod_of("a^p + 1 \\pmod p"),
            Some(("a^p + 1".to_string(), "p".to_string()))
        );
        assert_eq!(
            pmod_of("1 \\pmod{13}"),
            Some(("1".to_string(), "13".to_string()))
        );
        assert_eq!(
            normalise("\\sum_{k \\ge 1} \\frac{1}{k^2}"),
            "\\sum_{k=1}^{\\infty} \\frac{1}{k^2}"
        );
        assert_eq!(normalise("\\sum_{k=0}^{n} k"), "\\sum_{k=0}^{n} k");
    }

    #[test]
    fn indexed_families_and_ellipses() {
        let src = "\\sum_{i=1}^{n} a_i^2";
        assert!(has_indexed_family(&expr(src), src));
        let src = "\\sum_{k=1}^{n} k^2";
        assert!(!has_indexed_family(&expr(src), src));
        let e = expr("1 + 2 + \\dots + n");
        assert!(arithmetic_ellipsis(&e).is_some());
        let e = expr("1 - \\frac{x^2}{6} + \\frac{x^4}{120} - \\cdots");
        assert!(truncated_series(&e).is_some());
    }

    #[test]
    fn chain_links() {
        let src = "(a+1)^p = \\sum_{k=0}^{p} \\binom{p}{k} a^k \\equiv a^p + 1 \\pmod p";
        let (ast, _) = parse_math(src, 0);
        let Statement::Relation(node) = &ast.lines[0].statement else {
            panic!("expected a relation");
        };
        let ls = links(node, src);
        assert_eq!(ls.len(), 2);
        assert_eq!(ls[1].op, ClaimOp::Congruent);
        assert_eq!(ls[1].modulus.as_deref(), Some("p"));
        assert_eq!(side_text(&ls[1].rhs, src), "a^p + 1");
    }
}
