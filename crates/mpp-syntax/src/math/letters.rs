//! Runs of letters written side by side (`velocity`).
//!
//! The grammar reads unknown letters next to each other as a product, as a maths
//! paper does (`xy` is `x·y`). A run that spells a name the document defines
//! (`velocity := 3`) is that name instead; the evaluator and the IDE find such runs
//! here.

use super::ast::{BinOp, Constant, MulStyle, Node, NodeKind};
use crate::Span;

/// The factors of an implicit product (`2xy` → `2`, `x`, `y`), left to right. Any
/// other node is its own single factor.
pub fn implicit_factors(node: &Node) -> Vec<&Node> {
    fn walk<'a>(n: &'a Node, out: &mut Vec<&'a Node>) {
        match &n.kind {
            NodeKind::Binary {
                op: BinOp::Mul(MulStyle::Implicit),
                lhs,
                rhs,
            } => {
                walk(lhs, out);
                walk(rhs, out);
            }
            _ => out.push(n),
        }
    }
    let mut out = vec![];
    walk(node, &mut out);
    out
}

/// The letter of a plain one-letter name (`x`; not `x_0`, `\hat x`, `\mathbf{x}`).
pub fn plain_letter(node: &Node) -> Option<char> {
    // `e` and `i` read as the constants are letters of a word too (`velocity`).
    let one = node.span.end == node.span.start + 1;
    let n = match &node.kind {
        NodeKind::Constant(Constant::E) if one => return Some('e'),
        NodeKind::Constant(Constant::I) if one => return Some('i'),
        NodeKind::Name(n) => n,
        _ => return None,
    };
    let mut cs = n.base.chars();
    let (Some(c), None) = (cs.next(), cs.next()) else {
        return None;
    };
    (c.is_ascii_alphabetic() && n.sub.is_none() && n.accent.is_none() && n.font.is_none())
        .then_some(c)
}

/// Letters written with nothing between them, at least two: factors `start..end`
/// spell `word`. The last factor may be a power of a letter (`velocity^2`), when
/// `powered` is set.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LetterRun {
    pub start: usize,
    pub end: usize,
    pub word: String,
    /// The source of the letters (without the exponent of a powered last letter).
    pub span: Span,
    pub powered: bool,
}

/// The letter a factor contributes to a run, its span, and whether it is a power.
fn run_letter(n: &Node) -> Option<(char, Span, bool)> {
    if let Some(c) = plain_letter(n) {
        return Some((c, n.span, false));
    }
    if let NodeKind::Pow { base, .. } = &n.kind {
        return plain_letter(base).map(|c| (c, base.span, true));
    }
    None
}

/// The maximal runs of adjacent letters among `factors`.
pub fn letter_runs(factors: &[&Node]) -> Vec<LetterRun> {
    let mut out = vec![];
    let mut i = 0;
    while i < factors.len() {
        let Some((c, span, powered)) = run_letter(factors[i]) else {
            i += 1;
            continue;
        };
        let mut word = c.to_string();
        let mut last = span;
        let mut end = i + 1;
        let mut was_power = powered;
        while !was_power && end < factors.len() {
            match run_letter(factors[end]) {
                Some((c, s, p)) if s.start == last.end => {
                    word.push(c);
                    last = s;
                    was_power = p;
                    end += 1;
                }
                _ => break,
            }
        }
        if end - i >= 2 {
            out.push(LetterRun {
                start: i,
                end,
                word,
                span: span.join(last),
                powered: was_power,
            });
        }
        i = end;
    }
    out
}

/// The word a node spells when it is nothing but adjacent letters (`velocity`).
pub fn spelled_word(node: &Node) -> Option<String> {
    let factors = implicit_factors(node);
    let runs = letter_runs(&factors);
    match runs.as_slice() {
        [run] if run.start == 0 && run.end == factors.len() && !run.powered => {
            Some(run.word.clone())
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::math::parse_expr;

    #[test]
    fn adjacent_letters_spell_a_word() {
        let (n, _) = parse_expr("velocity", 0);
        assert_eq!(spelled_word(&n).as_deref(), Some("velocity"));
        let (n, _) = parse_expr("v e l", 0);
        assert_eq!(spelled_word(&n), None);
        let (n, _) = parse_expr("2velocity^2", 0);
        let f = implicit_factors(&n);
        let runs = letter_runs(&f);
        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0].word, "velocity");
        assert!(runs[0].powered);
    }
}
