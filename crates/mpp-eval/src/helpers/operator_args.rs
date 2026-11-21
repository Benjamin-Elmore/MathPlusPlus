//! How the calculus operators read their arguments. Each operator is written
//! `\operatorname{name}(arguments)`; scripts may carry an interval, a method or a variant
//! (`\operatorname{riemann}_{left}(x^2, [0, 1], 4)`,
//! `\operatorname{volume}_{shell}(x - x^2, [0, 1], x = 0)`). Arguments are recognised by
//! their shape: intervals `[a, b]` are bounds, `x = c` / `y = c` are axes (or starting
//! values), a lone name is the variable, words such as `shell` or `left` choose a method,
//! numbers are counts and points, and everything else is a function. The words are
//! recognised whether they arrive as one name (`\mathrm{shell}`) or as the product of their
//! letters (`shell` typed plainly).

use mpp_core::{Expr, ExprNode, MathError, MathResult, RelOp, Steps};

use crate::holders::Ctx;

/// Words that select a method or variant, recognised in scripts and arguments.
pub(super) const KEYWORDS: &[&str] = &[
    "left",
    "right",
    "mid",
    "midpoint",
    "trapezoid",
    "trapezoidal",
    "simpson",
    "disk",
    "disc",
    "washer",
    "shell",
    "shells",
    "laplace",
    "series",
    "frobenius",
    "numeric",
    "auto",
];

/// An operator call with its scripts.
pub(super) struct Call {
    /// Lower-case name.
    pub(super) name: String,
    pub(super) sub: Option<Expr>,
    pub(super) sup: Option<Expr>,
    pub(super) args: Vec<Expr>,
}

impl Call {
    pub(super) fn read(e: &Expr) -> Option<Call> {
        let ExprNode::Apply(name, args) = e.node() else {
            return None;
        };
        if &**name == "operatorname" {
            let [which, sub, sup, body] = args.as_slice() else {
                return None;
            };
            let ExprNode::Tuple(items) = body.node() else {
                return None;
            };
            let name = which.as_sym()?.to_lowercase();
            let script = |s: &Expr| (!is_empty_tuple(s)).then(|| s.clone());
            return Some(Call {
                name,
                sub: script(sub),
                sup: script(sup),
                args: items.clone(),
            });
        }
        Some(Call {
            name: name.to_lowercase(),
            sub: None,
            sup: None,
            args: args.clone(),
        })
    }

    /// The arguments and scripts, sorted by shape.
    pub(super) fn parts(&self) -> Parts {
        let mut p = Parts::default();
        for a in &self.args {
            p.add(a);
        }
        for s in self.sub.iter().chain(self.sup.iter()) {
            match s.as_sym() {
                Some(name) if keyword(s).is_none() => p.names.push(name.to_string()),
                _ => p.add(s),
            }
        }
        p
    }
}

pub(super) fn is_empty_tuple(e: &Expr) -> bool {
    matches!(e.node(), ExprNode::Tuple(items) if items.is_empty())
}

/// Arguments sorted by shape.
#[derive(Default)]
pub(super) struct Parts {
    pub(super) intervals: Vec<(Expr, Expr)>,
    pub(super) relations: Vec<(Expr, Expr)>,
    pub(super) names: Vec<String>,
    pub(super) words: Vec<String>,
    pub(super) numbers: Vec<Expr>,
    pub(super) tuples: Vec<Vec<Expr>>,
    pub(super) functions: Vec<Expr>,
}

impl Parts {
    fn add(&mut self, a: &Expr) {
        if let Some(w) = keyword(a) {
            self.words.push(w);
            return;
        }
        match a.node() {
            ExprNode::Interval { lo, hi, .. } => self.intervals.push((lo.clone(), hi.clone())),
            ExprNode::Relation(RelOp::Eq, l, r) => self.relations.push((l.clone(), r.clone())),
            // A lone name before any function is itself a function (`area(x, x^2)`); after
            // one it names the variable (`area(x^2, x, [0, 1])`).
            ExprNode::Sym(_) if self.functions.is_empty() && self.tuples.is_empty() => {
                self.functions.push(a.clone())
            }
            ExprNode::Sym(s) => self.names.push(s.to_string()),
            ExprNode::Tuple(items) => self.tuples.push(items.clone()),
            _ if a.is_constant() => self.numbers.push(a.clone()),
            _ => self.functions.push(a.clone()),
        }
    }

    pub(super) fn interval(&self, op: &str) -> MathResult<(Expr, Expr)> {
        self.intervals
            .first()
            .cloned()
            .ok_or_else(|| missing(op, "an interval [a, b]"))
    }

    /// The variable: a lone name given, else the only free symbol of the functions, else
    /// `x`.
    pub(super) fn variable(&self, op: &str) -> MathResult<String> {
        if let Some(n) = self.names.first() {
            return Ok(n.clone());
        }
        let mut free = std::collections::BTreeSet::new();
        for f in &self.functions {
            free.extend(f.free_symbols().iter().map(|s| s.to_string()));
        }
        match free.len() {
            0 => Ok("x".into()),
            1 => Ok(free.into_iter().next().unwrap_or_else(|| "x".into())),
            _ if free.contains("x") => Ok("x".into()),
            _ => Err(missing(
                op,
                "the variable (give it as an argument, such as x)",
            )),
        }
    }

    pub(super) fn word(&self, choices: &[&str]) -> Option<String> {
        self.words
            .iter()
            .find(|w| choices.contains(&w.as_str()))
            .cloned()
    }
}

pub(super) fn missing(op: &str, what: &str) -> MathError {
    MathError::Domain {
        function: op.to_string(),
        detail: format!("it needs {what}"),
    }
}

/// A method word, typed as one name or as the product of its letters.
pub(super) fn keyword(e: &Expr) -> Option<String> {
    if let Some(s) = e.as_sym() {
        let lower = s.to_lowercase();
        return KEYWORDS.contains(&lower.as_str()).then_some(lower);
    }
    let mut letters = Vec::new();
    collect_letters(e, &mut letters)?;
    if letters.len() < 3 {
        return None;
    }
    letters.sort_unstable();
    KEYWORDS
        .iter()
        .find(|k| {
            let mut w: Vec<char> = k.chars().collect();
            w.sort_unstable();
            w == letters
        })
        .map(|k| k.to_string())
}

pub(super) fn collect_letters(e: &Expr, out: &mut Vec<char>) -> Option<()> {
    match e.node() {
        ExprNode::Sym(s) => {
            let mut chars = s.chars();
            let c = chars.next()?;
            if chars.next().is_some() || !c.is_ascii_alphabetic() {
                return None;
            }
            out.push(c.to_ascii_lowercase());
            Some(())
        }
        // `e` and `i` arrive as Euler's number and the imaginary unit.
        ExprNode::Const(mpp_core::Constant::E) => {
            out.push('e');
            Some(())
        }
        ExprNode::Num(n) if *n == mpp_core::Number::i() => {
            out.push('i');
            Some(())
        }
        ExprNode::Mul(fs) => fs.iter().try_for_each(|f| collect_letters(f, out)),
        ExprNode::Pow(b, x) => {
            let n = x.as_number()?.as_i64()?;
            if !(1..=4).contains(&n) {
                return None;
            }
            for _ in 0..n {
                collect_letters(b, out)?;
            }
            Some(())
        }
        _ => None,
    }
}

/// Runs a core routine with a step log when the statement asks for steps.
pub(super) fn logged<T>(
    ctx: &Ctx<'_>,
    f: impl FnOnce(Option<&mut Steps>) -> MathResult<T>,
) -> MathResult<T> {
    if !ctx.logging() {
        return f(None);
    }
    let mut local = Steps::new();
    let out = f(Some(&mut local));
    ctx.append(local);
    out
}

pub(super) fn has_float(e: &Expr) -> bool {
    match e.node() {
        ExprNode::Num(n) => !n.is_exact(),
        _ => e.children().into_iter().any(has_float),
    }
}

pub(super) fn note_numeric(ctx: &Ctx<'_>, value: &Expr, what: &str) {
    if !ctx.numeric && has_float(value) {
        ctx.note(format!(
            "no closed form was found for {what}, so the value shown is numerical (≈)"
        ));
    }
}

pub(super) fn to_f64(e: &Expr, what: &str) -> MathResult<f64> {
    e.eval_f64(&mpp_core::EmptyEnv)
        .ok()
        .filter(|v| v.is_finite())
        .ok_or_else(|| MathError::NotANumber(format!("{what} ({e})")))
}

pub(super) fn to_count(e: &Expr, what: &str) -> MathResult<usize> {
    e.as_number()
        .and_then(|n| n.as_i64())
        .and_then(|n| usize::try_from(n).ok())
        .filter(|n| *n > 0)
        .ok_or_else(|| MathError::Domain {
            function: what.into(),
            detail: format!("{e} is not a positive whole number"),
        })
}
