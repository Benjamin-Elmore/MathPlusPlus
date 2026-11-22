//! Letters side by side that spell no defined name (`velocity`, `sinn(2)`).
//!
//! The grammar keeps reading them as a product (`xy` is `x·y`), but a word such as
//! `velocity` was more likely meant as one name, and `sinn(2)` as `sin(2)`, so the
//! evaluator warns, naming the function when one is close. The IDE and the command
//! line both get the warning from here.

use mpp_syntax::math::letters::{implicit_factors, letter_runs, plain_letter};
use mpp_syntax::math::{visit, DefHead, Line, Node, NodeKind, Statement};
use mpp_syntax::Span;

/// One run of letters read as a product that was probably meant as a word.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LetterRunHit {
    pub span: Span,
    /// The letters as typed (`sinn`).
    pub word: String,
    /// The function the word is close to (`sin`), when there is one.
    pub function: Option<String>,
    /// The word is called like a function (`integrate(x^2, x, 0, 3)`).
    pub called: bool,
}

/// The functions a mistyped word is compared with.
const FUNCTIONS: [&str; 22] = [
    "sin", "cos", "tan", "sec", "csc", "cot", "sinh", "cosh", "tanh", "coth", "arcsin", "arccos",
    "arctan", "exp", "log", "ln", "sqrt", "round", "floor", "ceil", "abs", "diff",
];

/// The function `word` is one edit away from, or a function followed by a letter or
/// two (`sinn`, `lnn`, `coss`, `sqr`, `expp`).
pub fn close_function(word: &str) -> Option<&'static str> {
    FUNCTIONS.iter().copied().find(|f| {
        let extra = word.chars().count().saturating_sub(f.chars().count());
        edit_distance(word, f) == 1 || (word.starts_with(f) && (1..=2).contains(&extra))
    })
}

fn edit_distance(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.iter().enumerate() {
        let mut cur = vec![i + 1];
        for (j, cb) in b.iter().enumerate() {
            let sub = prev[j] + usize::from(ca != cb);
            cur.push(sub.min(prev[j + 1] + 1).min(cur[j] + 1));
        }
        prev = cur;
    }
    prev[b.len()]
}

/// The message of the warning for `hit`.
pub fn message(hit: &LetterRunHit) -> String {
    let spaced: Vec<String> = hit.word.chars().map(String::from).collect();
    let ask = match &hit.function {
        Some(f) => format!("Did you mean the function `{f}`?"),
        None => format!("Did you mean the name `{}`?", hit.word),
    };
    format!(
        "`{}` is read as the product {} of single letters. {ask}",
        hit.word,
        spaced.join("\u{b7}")
    )
}

/// The error for a word called like a function that no function has as its name
/// (`integrate(x^2, x, 0, 3)`): what it is, and the form to write instead.
pub fn unknown_function_message(hit: &LetterRunHit) -> String {
    let w = hit.word.as_str();
    let fix = match w {
        "integrate" | "integral" | "integ" | "quad" => {
            "Did you mean int(f, x, a, b)? (SymPy writes integrate(f, (x, a, b)).)".to_string()
        }
        "derivative" | "derive" | "deriv" | "differentiate" => "Did you mean diff(f, x)?".into(),
        "summation" | "series" => "Did you mean sum(f, n, a, b)?".into(),
        "product" => "Did you mean prod(f, n, a, b)?".into(),
        "print" | "show" | "display" | "disp" | "echo" => {
            "To see a value, type its name and = (R =).".into()
        }
        "range" => "In Python range(a, b) stops before b: write sum(f, n, a, b - 1).".into(),
        "power" | "pow" => "Write x^n (or x**n).".into(),
        _ => match &hit.function {
            Some(f) => format!("Did you mean {f}?"),
            None => "Define it first (f(x) := …) or check the spelling.".into(),
        },
    };
    format!("`{w}` is not a known function. {fix}")
}

/// The word and function named by a warning from `message`.
pub fn parse_message(msg: &str) -> Option<(String, Option<String>)> {
    let rest = msg.strip_prefix('`')?;
    let (word, tail) = rest.split_once('`')?;
    if !tail.starts_with(" is read as the product ") {
        return None;
    }
    let function = tail
        .split_once("Did you mean the function `")
        .and_then(|(_, f)| f.split_once('`'))
        .map(|(f, _)| f.to_string());
    Some((word.to_string(), function))
}

/// The runs of letters in `line` that are read as a product but spell no name for
/// which `is_known` is true.
pub fn hits(line: &Line, is_known: &dyn Fn(&str) -> bool) -> Vec<LetterRunHit> {
    fn walk(n: &Node, is_known: &dyn Fn(&str) -> bool, in_op: bool, out: &mut Vec<LetterRunHit>) {
        let in_op = in_op || matches!(n.kind, NodeKind::NamedOp(_));
        let factors = implicit_factors(n);
        if factors.len() > 1 {
            for r in letter_runs(&factors) {
                // The words a helper takes as options (`left`, `shell`) are not products.
                let option = in_op && crate::helpers::is_option_word(&r.word);
                if !is_known(&r.word) && !option {
                    // `sinn(2)`: the last letter is the name of a call; it belongs
                    // to the word the reader typed.
                    let (mut word, mut span) = (r.word.clone(), r.span);
                    // `integrate(…)`: letters straight before brackets (the last one is
                    // `e`, a constant, so no call is parsed) are called too.
                    let mut called = factors.get(r.end).is_some_and(|f| {
                        f.span.start == r.span.end
                            && matches!(f.kind, NodeKind::Tuple { .. } | NodeKind::Group { .. })
                    });
                    if let Some(NodeKind::Call { callee, .. }) = factors.get(r.end).map(|f| &f.kind)
                    {
                        if let Some(c) = plain_letter(callee) {
                            if callee.span.start == r.span.end && !r.powered {
                                word.push(c);
                                span = span.join(callee.span);
                                called = true;
                            }
                        }
                    }
                    let function = close_function(&word);
                    // Three letters make a word; a typo of a function next to
                    // brackets is flagged whatever its length.
                    let long = word.chars().count() >= 3;
                    if (long || (called && function.is_some())) && !is_known(&word) {
                        out.push(LetterRunHit {
                            span,
                            word,
                            function: function.map(str::to_string),
                            called,
                        });
                    }
                }
            }
            for f in factors {
                visit::children(f, &mut |c| walk(c, is_known, in_op, out));
            }
        } else {
            visit::children(n, &mut |c| walk(c, is_known, in_op, out));
        }
    }
    let mut roots: Vec<&Node> = vec![];
    match &line.statement {
        Statement::Definition {
            head: DefHead::Function { .. } | DefHead::Name(_),
            body,
            ..
        } => roots.push(body),
        Statement::Query { expr, .. } => roots.push(expr),
        Statement::Check { lhs, rhs } => {
            roots.push(lhs);
            roots.push(rhs);
        }
        Statement::Relation(n) => roots.push(n),
        _ => {}
    }
    let mut out = vec![];
    for r in roots {
        if matches!(r.kind, NodeKind::Error) {
            continue;
        }
        walk(r, is_known, false, &mut out);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// E-N6: a typo of `round` (and the other helpers) names it.
    #[test]
    fn typos_of_helpers_name_the_helper() {
        assert_eq!(close_function("roundd"), Some("round"));
        assert_eq!(close_function("rond"), Some("round"));
        assert_eq!(close_function("sinn"), Some("sin"));
        assert_eq!(close_function("flor"), Some("floor"));
        assert_eq!(close_function("velocity"), None);
    }
}
