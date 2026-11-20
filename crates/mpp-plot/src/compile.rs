//! Turning document expressions into something fast to evaluate: user functions and
//! constants are inlined once, leaving only the plot variables and slider names free.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use mpp_core::{Env, Expr, ExprNode, MathError, MathResult};
use mpp_eval::{Binding, VarKind};

/// How deep user functions may call each other while inlining.
const MAX_DEPTH: u32 = 24;

/// The user's definitions at the end of the document.
#[derive(Clone, Debug, Default)]
pub struct Defs {
    vars: BTreeMap<String, Expr>,
    funcs: BTreeMap<String, (Vec<String>, Expr)>,
}

impl Defs {
    /// Collects variables, constants and functions from the evaluator's bindings.
    pub fn from_bindings(bindings: &BTreeMap<String, Arc<Binding>>) -> Defs {
        let mut d = Defs::default();
        for (name, b) in bindings {
            match b.as_ref() {
                Binding::Variable { value, kind, .. } => {
                    let _ = matches!(kind, VarKind::Variable | VarKind::Constant);
                    d.vars.insert(name.clone(), value.clone());
                }
                Binding::Function { params, body, .. } => {
                    d.funcs.insert(name.clone(), (params.clone(), body.clone()));
                }
                Binding::Sequence { .. } | Binding::Imported { .. } => {}
            }
        }
        d
    }

    /// Adds a variable (used by tests and by callers that build figures by hand).
    pub fn with_var(mut self, name: &str, value: Expr) -> Defs {
        self.vars.insert(name.to_string(), value);
        self
    }

    /// Adds a function.
    pub fn with_function(mut self, name: &str, params: &[&str], body: Expr) -> Defs {
        self.funcs.insert(
            name.to_string(),
            (params.iter().map(|p| (*p).to_string()).collect(), body),
        );
        self
    }

    /// True when `name` is a defined variable or constant.
    pub fn has_var(&self, name: &str) -> bool {
        self.vars.contains_key(name)
    }

    /// The defined expression of a variable.
    pub fn var(&self, name: &str) -> Option<&Expr> {
        self.vars.get(name)
    }

    /// Replaces defined names and user-function calls in `e` by their definitions.
    /// Names in `keep` (plot variables, sliders) stay as symbols.
    pub fn inline(&self, e: &Expr, keep: &BTreeSet<String>) -> Expr {
        self.inline_at(e, keep, 0)
    }

    fn inline_at(&self, e: &Expr, keep: &BTreeSet<String>, depth: u32) -> Expr {
        if depth > MAX_DEPTH {
            return e.clone();
        }
        match e.node() {
            ExprNode::Sym(name) => {
                if keep.contains(name.as_ref()) {
                    return e.clone();
                }
                match self.vars.get(name.as_ref()) {
                    Some(v) => self.inline_at(v, keep, depth + 1),
                    None => e.clone(),
                }
            }
            ExprNode::Apply(name, args) => {
                let args: Vec<Expr> = args
                    .iter()
                    .map(|a| self.inline_at(a, keep, depth))
                    .collect();
                match self.funcs.get(name.as_ref()) {
                    Some((params, body)) if params.len() == args.len() => {
                        let map: BTreeMap<&str, &Expr> =
                            params.iter().map(String::as_str).zip(args.iter()).collect();
                        let filled = substitute(body, &map);
                        self.inline_at(&filled, keep, depth + 1)
                    }
                    _ => Expr::new(ExprNode::Apply(name.clone(), args)),
                }
            }
            _ => {
                // Bound variables of sums and limits are not free names.
                let mut keep_here;
                let keep = match e.bound_var() {
                    Some(v) if !keep.contains(v.as_ref()) => {
                        keep_here = keep.clone();
                        keep_here.insert(v.to_string());
                        &keep_here
                    }
                    _ => keep,
                };
                Expr::new(e.map_children(&mut |c| self.inline_at(c, keep, depth)))
            }
        }
    }
}

/// Replaces symbols by expressions at the same time, without simplifying.
pub fn substitute(e: &Expr, map: &BTreeMap<&str, &Expr>) -> Expr {
    match e.node() {
        ExprNode::Sym(name) => match map.get(name.as_ref()) {
            Some(r) => (*r).clone(),
            None => e.clone(),
        },
        _ => {
            if let Some(v) = e.bound_var() {
                if map.contains_key(v.as_ref()) {
                    let mut inner = map.clone();
                    inner.remove(v.as_ref());
                    return Expr::new(e.map_children(&mut |c| substitute(c, &inner)));
                }
            }
            Expr::new(e.map_children(&mut |c| substitute(c, map)))
        }
    }
}

/// Values for the free symbols of an inlined expression: sliders plus a few plot variables.
pub struct Frame<'a> {
    /// Current slider values.
    pub fixed: &'a BTreeMap<String, f64>,
    /// The plot variables being swept.
    pub vars: &'a [(&'a str, f64)],
}

impl Env for Frame<'_> {
    fn real(&self, name: &str) -> Option<f64> {
        self.vars
            .iter()
            .find(|(n, _)| *n == name)
            .map(|(_, v)| *v)
            .or_else(|| self.fixed.get(name).copied())
    }
}

/// Evaluates `e` as a real number; `None` when it is undefined or not real there.
pub fn eval_real(e: &Expr, frame: &Frame<'_>) -> Option<f64> {
    match e.eval_f64(frame) {
        Ok(v) if v.is_finite() => Some(v),
        Ok(_) => None,
        Err(MathError::Domain { .. }) | Err(MathError::Undefined(_)) => {
            // A result that is real up to rounding (such as the root of a square).
            let z = e.eval_complex(frame).ok()?;
            (z.re.is_finite() && z.im.abs() <= 1e-9 * (1.0 + z.re.abs())).then_some(z.re)
        }
        Err(_) => None,
    }
}

/// Evaluates a truth value (an inequality or a combination) at a point.
pub fn eval_truth(cond: &Expr, frame: &Frame<'_>) -> bool {
    use mpp_core::{LogicOp, RelOp};
    match cond.node() {
        ExprNode::Bool(b) => *b,
        ExprNode::Relation(op, a, b) => match (eval_real(a, frame), eval_real(b, frame)) {
            (Some(a), Some(b)) => match op {
                RelOp::Eq => a == b,
                RelOp::Ne => a != b,
                RelOp::Lt => a < b,
                RelOp::Le => a <= b,
                RelOp::Gt => a > b,
                RelOp::Ge => a >= b,
                RelOp::Approx => (a - b).abs() <= 1e-9 * a.abs().max(b.abs()).max(1.0),
            },
            _ => false,
        },
        ExprNode::Logic(op, args) => match op {
            LogicOp::And => args.iter().all(|a| eval_truth(a, frame)),
            LogicOp::Or => args.iter().any(|a| eval_truth(a, frame)),
            LogicOp::Not => args.first().is_some_and(|a| !eval_truth(a, frame)),
        },
        _ => false,
    }
}

/// Convenience for one plot variable.
pub fn eval1(e: &Expr, fixed: &BTreeMap<String, f64>, name: &str, x: f64) -> Option<f64> {
    eval_real(
        e,
        &Frame {
            fixed,
            vars: &[(name, x)],
        },
    )
}

/// Convenience for two plot variables.
pub fn eval2(
    e: &Expr,
    fixed: &BTreeMap<String, f64>,
    names: [&str; 2],
    x: f64,
    y: f64,
) -> Option<f64> {
    eval_real(
        e,
        &Frame {
            fixed,
            vars: &[(names[0], x), (names[1], y)],
        },
    )
}

/// Evaluates a number that may use constants and sliders (for ranges).
pub fn eval_const(e: &Expr, fixed: &BTreeMap<String, f64>) -> MathResult<f64> {
    e.eval_f64(&Frame { fixed, vars: &[] })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sym(s: &str) -> Expr {
        Expr::sym(s)
    }

    #[test]
    fn inlines_functions_and_constants_but_keeps_plot_variables() {
        // f(x) := a x^2, a := 3
        let body = mpp_core::mul(vec![sym("a"), mpp_core::pow(sym("x"), Expr::int(2))]);
        let defs = Defs::default()
            .with_var("a", Expr::int(3))
            .with_function("f", &["x"], body);
        let call = Expr::apply("f", vec![mpp_core::add(vec![sym("x"), Expr::int(1)])]);
        let keep: BTreeSet<String> = ["x".to_string()].into();
        let e = defs.inline(&call, &keep);
        let fixed = BTreeMap::new();
        // 3 (x+1)^2 at x = 2 is 27.
        assert_eq!(eval1(&e, &fixed, "x", 2.0), Some(27.0));
        assert!(e.free_symbols().iter().all(|s| s.as_ref() == "x"));
    }

    #[test]
    fn sliders_stay_symbolic() {
        let defs = Defs::default().with_var("a", Expr::int(2));
        let e = mpp_core::mul(vec![sym("a"), sym("x")]);
        let keep: BTreeSet<String> = ["x".to_string(), "a".to_string()].into();
        let e = defs.inline(&e, &keep);
        let mut fixed = BTreeMap::new();
        fixed.insert("a".to_string(), 5.0);
        assert_eq!(eval1(&e, &fixed, "x", 3.0), Some(15.0));
    }

    #[test]
    fn self_recursion_terminates() {
        let body = Expr::apply("f", vec![sym("x")]);
        let defs = Defs::default().with_function("f", &["x"], body);
        let e = defs.inline(&Expr::apply("f", vec![sym("x")]), &BTreeSet::new());
        assert_eq!(eval1(&e, &BTreeMap::new(), "x", 1.0), None);
    }

    #[test]
    fn domain_errors_are_gaps() {
        let e = mpp_core::pow(sym("x"), Expr::rational(1, 2));
        let fixed = BTreeMap::new();
        assert_eq!(eval1(&e, &fixed, "x", 4.0), Some(2.0));
        assert_eq!(eval1(&e, &fixed, "x", -4.0), None);
    }

    #[test]
    fn truth_of_combined_conditions() {
        use mpp_core::{LogicOp, RelOp};
        let c = Expr::new(ExprNode::Logic(
            LogicOp::And,
            vec![
                Expr::relation(RelOp::Gt, sym("x"), Expr::int(0)),
                Expr::relation(RelOp::Lt, sym("x"), Expr::int(2)),
            ],
        ));
        let fixed = BTreeMap::new();
        let at = |x: f64| {
            eval_truth(
                &c,
                &Frame {
                    fixed: &fixed,
                    vars: &[("x", x)],
                },
            )
        };
        assert!(at(1.0) && !at(3.0) && !at(0.0));
    }
}
