//! A thin layer over the evaluator: runs a few lines of maths after some definitions and
//! hands back each line's verdict or value. The checker never evaluates maths itself;
//! it writes the claims as a tiny `.mpp` document so every CAS feature applies.

use std::sync::Arc;

use mpp_core::{Expr, ExprNode};
use mpp_eval::{NoImports, Session, Verdict};
use num_complex::Complex64;

/// What one line of maths came to.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum LineOut {
    /// A check (`\overset{?}{=}`) and its verdict.
    Verdict(Verdict),
    /// A query (`= ?`) and its exact value.
    Value(Expr),
    /// The line could not be worked out.
    Error(String),
}

impl LineOut {
    /// The value as a complex number, when it is one.
    pub(crate) fn number(&self) -> Option<Complex64> {
        match self {
            LineOut::Value(e) => {
                let z = e.eval_complex(&mpp_core::EmptyEnv).ok()?;
                (z.re.is_finite() && z.im.is_finite()).then_some(z)
            }
            _ => None,
        }
    }

    /// The value as a real number (imaginary part below 1e-9 of the size).
    pub(crate) fn real(&self) -> Option<f64> {
        let z = self.number()?;
        (z.im.abs() <= 1e-9 * z.re.abs().max(1.0)).then_some(z.re)
    }
}

/// A definition written into the scratch document: `name := body` (both LaTeX).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Def {
    pub name: String,
    pub body: String,
}

impl Def {
    pub(crate) fn new(name: impl Into<String>, body: impl Into<String>) -> Self {
        Def {
            name: name.into(),
            body: body.into(),
        }
    }
}

/// Runs `lines` (each one statement line of maths) after `defs`, and returns one
/// [`LineOut`] per line, in order.
pub(crate) fn run(defs: &[Def], lines: &[String]) -> Vec<LineOut> {
    let mut src = String::new();
    for d in defs {
        src.push_str(&format!("$$ {} := {} $$\n\n", d.name, d.body));
    }
    for l in lines {
        src.push_str(&format!("$$ {l} $$\n\n"));
    }
    let mut session = Session::new().with_importer(Arc::new(NoImports));
    session.set_auto_eval(false);
    let ev = session.run_str(&src);
    let blocks = &ev.blocks;
    let mut out = Vec::with_capacity(lines.len());
    for k in 0..lines.len() {
        let Some(block) = blocks.get(defs.len() + k) else {
            out.push(LineOut::Error("the line was not read".into()));
            continue;
        };
        if let Some(d) = block
            .diagnostics
            .iter()
            .find(|d| d.severity == mpp_syntax::Severity::Error)
        {
            out.push(LineOut::Error(d.message.clone()));
            continue;
        }
        let Some(line) = block.values.first() else {
            out.push(LineOut::Error("the line holds no maths".into()));
            continue;
        };
        if let Some(v) = &line.verdict {
            out.push(LineOut::Verdict(v.clone()));
        } else if let Some(v) = &line.value {
            out.push(LineOut::Value(v.expr()));
        } else {
            out.push(LineOut::Error(
                line.hint.clone().unwrap_or_else(|| "no answer".into()),
            ));
        }
    }
    out
}

/// Runs one line.
pub(crate) fn run_one(defs: &[Def], line: String) -> LineOut {
    run(defs, &[line])
        .into_iter()
        .next()
        .unwrap_or_else(|| LineOut::Error("no answer".into()))
}

/// The single element of a one-element set (a solver answer), if that is what `e` is.
pub(crate) fn single_solution(e: &Expr) -> Option<Expr> {
    match e.node() {
        ExprNode::Set(items) if items.len() == 1 => items.first().cloned(),
        ExprNode::Relation(mpp_core::RelOp::Eq, _, rhs) => Some(rhs.clone()),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn checks_and_values() {
        let defs = vec![Def::new("p", "\\sqrt{2} q")];
        let out = run(
            &defs,
            &[
                "p^2 \\overset{?}{=} 2 q^2".to_string(),
                "\\gcd(4, 6) = ?".to_string(),
            ],
        );
        assert!(matches!(&out[0], LineOut::Verdict(v) if v.passed() == Some(true)));
        assert_eq!(out[1].real(), Some(2.0));
    }

    #[test]
    fn solver_answer() {
        let out = run_one(
            &[],
            "\\operatorname{solve}_{r} \\left(\\sqrt{2} = r/s\\right) = ?".into(),
        );
        let LineOut::Value(v) = out else {
            panic!("expected a value");
        };
        assert!(single_solution(&v).is_some());
    }
}
