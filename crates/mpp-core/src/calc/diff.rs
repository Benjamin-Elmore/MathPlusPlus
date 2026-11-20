//! Symbolic differentiation. Every rule that fires records a step, so the steps are the
//! working that produced the answer.

use std::sync::Arc;

use crate::error::MathResult;
use crate::expr::{Constant, Expr, ExprNode};
use crate::functions::{func, Func};
use crate::number::Number;
use crate::printer::display_order;
use crate::simplify::{add, mul, pow, simplify};
use crate::steps::Steps;

/// The `order`-th derivative of `expr` with respect to `var`, simplified.
///
/// Unknown (user) functions and operators without a rule stay as unevaluated
/// `Derivative` nodes, so `d/dx f(x)` is `f′(x)`.
pub fn diff(expr: &Expr, var: &str, order: u32, steps: Option<&mut Steps>) -> MathResult<Expr> {
    let mut log = steps;
    let mut current = simplify(expr);
    for _ in 0..order {
        current = d(&current, var, &mut log)?;
    }
    Ok(current)
}

/// The unevaluated derivative `d/dvar expr` (orders add up when nested).
pub fn derivative_holder(e: &Expr, var: &str) -> Expr {
    if let ExprNode::Derivative { expr, vars } = e.node() {
        let mut vars = vars.clone();
        match vars.last_mut() {
            Some((v, k)) if &**v == var => *k += 1,
            _ => vars.push((Arc::from(var), 1)),
        }
        return Expr::new(ExprNode::Derivative {
            expr: expr.clone(),
            vars,
        });
    }
    Expr::new(ExprNode::Derivative {
        expr: e.clone(),
        vars: vec![(Arc::from(var), 1)],
    })
}

type Log<'a, 'b> = &'a mut Option<&'b mut Steps>;

fn step(log: Log<'_, '_>, rule: &str, before: Expr, after: Expr, note: Option<String>) {
    Steps::record(log, rule, &before, &after, note);
}

fn raw_mul(v: Vec<Expr>) -> Expr {
    Expr::new(ExprNode::Mul(v))
}

fn raw_add(v: Vec<Expr>) -> Expr {
    Expr::new(ExprNode::Add(v))
}

fn d(e: &Expr, var: &str, log: Log<'_, '_>) -> MathResult<Expr> {
    let h = || derivative_holder(e, var);
    if !e.contains_sym(var) {
        step(log, "constant rule", h(), Expr::zero(), None);
        return Ok(Expr::zero());
    }
    match e.node() {
        ExprNode::Sym(_) => {
            step(log, "power rule", h(), Expr::one(), None);
            Ok(Expr::one())
        }
        ExprNode::Add(terms) => {
            let terms = display_order(terms);
            let split = raw_add(terms.iter().map(|t| derivative_holder(t, var)).collect());
            step(log, "sum rule", h(), split, None);
            let parts = terms
                .iter()
                .map(|t| d(t, var, log))
                .collect::<MathResult<Vec<_>>>()?;
            Ok(add(parts))
        }
        ExprNode::Mul(factors) => d_mul(e, factors, var, log),
        ExprNode::Pow(b, x) => d_pow(e, b, x, var, log),
        ExprNode::Func(f, args) => d_func(e, *f, args, var, log),
        ExprNode::Relation(op, a, b) => Ok(Expr::relation(*op, d(a, var, log)?, d(b, var, log)?)),
        ExprNode::Matrix { rows, cols, data } => Ok(Expr::new(ExprNode::Matrix {
            rows: *rows,
            cols: *cols,
            data: data
                .iter()
                .map(|x| d(x, var, log))
                .collect::<MathResult<_>>()?,
        })),
        ExprNode::Tuple(v) => Ok(Expr::new(ExprNode::Tuple(
            v.iter()
                .map(|x| d(x, var, log))
                .collect::<MathResult<_>>()?,
        ))),
        ExprNode::Piecewise { cases, otherwise } => {
            let mut new_cases = Vec::with_capacity(cases.len());
            for (v, c) in cases {
                new_cases.push((d(v, var, log)?, c.clone()));
            }
            let otherwise = match otherwise {
                Some(o) => Some(d(o, var, log)?),
                None => None,
            };
            Ok(Expr::new(ExprNode::Piecewise {
                cases: new_cases,
                otherwise,
            }))
        }
        // User functions, nested derivatives, integrals, sums, limits: unevaluated.
        _ => Ok(h()),
    }
}

fn d_mul(e: &Expr, factors: &[Expr], var: &str, log: Log<'_, '_>) -> MathResult<Expr> {
    let h = derivative_holder(e, var);
    let (consts, vars): (Vec<Expr>, Vec<Expr>) =
        factors.iter().cloned().partition(|f| !f.contains_sym(var));
    if !consts.is_empty() {
        let c = mul(consts);
        let rest = mul(vars);
        step(
            log,
            "constant multiple",
            h,
            raw_mul(vec![c.clone(), derivative_holder(&rest, var)]),
            None,
        );
        let dr = d(&rest, var, log)?;
        return Ok(mul(vec![c, dr]));
    }
    // Factors with a negative numeric exponent form the denominator.
    let mut num = Vec::new();
    let mut den = Vec::new();
    for f in &vars {
        match f.node() {
            ExprNode::Pow(b, x) if x.as_number().is_some_and(Number::is_negative) => {
                den.push(pow(b.clone(), -x.clone()));
            }
            _ => num.push(f.clone()),
        }
    }
    if !num.is_empty() && !den.is_empty() {
        let n = mul(num);
        let dd = mul(den);
        let shown = raw_mul(vec![
            raw_add(vec![
                raw_mul(vec![derivative_holder(&n, var), dd.clone()]),
                raw_mul(vec![Expr::int(-1), n.clone(), derivative_holder(&dd, var)]),
            ]),
            Expr::new(ExprNode::Pow(dd.clone(), Expr::int(-2))),
        ]);
        step(log, "quotient rule", h, shown, None);
        let dn = d(&n, var, log)?;
        let ddd = d(&dd, var, log)?;
        let top = add(vec![
            mul(vec![dn, dd.clone()]),
            mul(vec![Expr::int(-1), n, ddd]),
        ]);
        return Ok(mul(vec![top, pow(dd, Expr::int(-2))]));
    }
    let shown = raw_add(
        (0..vars.len())
            .map(|i| {
                let mut v = Vec::with_capacity(vars.len());
                for (j, f) in vars.iter().enumerate() {
                    v.push(if i == j {
                        derivative_holder(f, var)
                    } else {
                        f.clone()
                    });
                }
                raw_mul(v)
            })
            .collect(),
    );
    step(log, "product rule", h, shown, None);
    let mut terms = Vec::with_capacity(vars.len());
    for i in 0..vars.len() {
        let di = d(&vars[i], var, log)?;
        let mut v = vec![di];
        v.extend(
            vars.iter()
                .enumerate()
                .filter(|(j, _)| *j != i)
                .map(|(_, f)| f.clone()),
        );
        terms.push(mul(v));
    }
    Ok(add(terms))
}

fn d_pow(e: &Expr, b: &Expr, x: &Expr, var: &str, log: Log<'_, '_>) -> MathResult<Expr> {
    let h = derivative_holder(e, var);
    let (b_var, x_var) = (b.contains_sym(var), x.contains_sym(var));
    if !x_var {
        let outer = mul(vec![
            x.clone(),
            pow(b.clone(), add(vec![x.clone(), Expr::int(-1)])),
        ]);
        if b.is_sym(var) {
            step(log, "power rule", h, outer.clone(), None);
            return Ok(outer);
        }
        step(
            log,
            "chain rule",
            h,
            raw_mul(vec![outer.clone(), derivative_holder(b, var)]),
            Some(format!("power rule on u^{x} with u = {b}")),
        );
        let db = d(b, var, log)?;
        return Ok(mul(vec![outer, db]));
    }
    if !b_var {
        let is_e = matches!(b.node(), ExprNode::Const(Constant::E));
        let outer = if is_e {
            e.clone()
        } else {
            mul(vec![e.clone(), func(Func::Ln, vec![b.clone()])])
        };
        let rule = if is_e {
            "derivative of e^x"
        } else {
            "exponential rule"
        };
        if x.is_sym(var) {
            step(log, rule, h, outer.clone(), None);
            return Ok(outer);
        }
        step(
            log,
            "chain rule",
            h,
            raw_mul(vec![outer.clone(), derivative_holder(x, var)]),
            Some(format!("{rule} with u = {x}")),
        );
        let dx = d(x, var, log)?;
        return Ok(mul(vec![outer, dx]));
    }
    // f^g = e^(g ln f)
    let inner = mul(vec![x.clone(), func(Func::Ln, vec![b.clone()])]);
    step(
        log,
        "logarithmic differentiation",
        h,
        raw_mul(vec![e.clone(), derivative_holder(&inner, var)]),
        Some(format!("{e} = e^({inner})")),
    );
    let di = d(&inner, var, log)?;
    Ok(mul(vec![e.clone(), di]))
}

/// `F′(u)` for a one-argument function `F`, if there is a rule.
fn outer_derivative(f: Func, u: &Expr) -> Option<Expr> {
    let one = |g: Func| func(g, vec![u.clone()]);
    let neg = |x: Expr| mul(vec![Expr::int(-1), x]);
    let u2 = || pow(u.clone(), Expr::int(2));
    let half = Expr::rational(-1, 2);
    Some(match f {
        Func::Sin => one(Func::Cos),
        Func::Cos => neg(one(Func::Sin)),
        Func::Tan => pow(one(Func::Sec), Expr::int(2)),
        Func::Sec => mul(vec![one(Func::Sec), one(Func::Tan)]),
        Func::Csc => neg(mul(vec![one(Func::Csc), one(Func::Cot)])),
        Func::Cot => neg(pow(one(Func::Csc), Expr::int(2))),
        Func::Asin => pow(add(vec![Expr::one(), neg(u2())]), half),
        Func::Acos => neg(pow(add(vec![Expr::one(), neg(u2())]), half)),
        Func::Atan => pow(add(vec![Expr::one(), u2()]), Expr::int(-1)),
        Func::Sinh => one(Func::Cosh),
        Func::Cosh => one(Func::Sinh),
        Func::Tanh => pow(one(Func::Cosh), Expr::int(-2)),
        Func::Asinh => pow(add(vec![u2(), Expr::one()]), half),
        Func::Acosh => pow(add(vec![u2(), Expr::int(-1)]), half),
        Func::Atanh => pow(add(vec![Expr::one(), neg(u2())]), Expr::int(-1)),
        Func::Ln => pow(u.clone(), Expr::int(-1)),
        Func::Abs => one(Func::Sign),
        Func::Sign | Func::Floor | Func::Ceil => Expr::zero(),
        Func::Erf => mul(vec![
            Expr::int(2),
            pow(Expr::pi(), Expr::rational(-1, 2)),
            pow(Expr::e(), neg(u2())),
        ]),
        Func::Si => mul(vec![one(Func::Sin), pow(u.clone(), Expr::int(-1))]),
        Func::Ci => mul(vec![one(Func::Cos), pow(u.clone(), Expr::int(-1))]),
        Func::Gamma => mul(vec![one(Func::Gamma), one(Func::Digamma)]),
        Func::Factorial => {
            let up = add(vec![u.clone(), Expr::one()]);
            mul(vec![one(Func::Factorial), func(Func::Digamma, vec![up])])
        }
        _ => return None,
    })
}

fn d_func(e: &Expr, f: Func, args: &[Expr], var: &str, log: Log<'_, '_>) -> MathResult<Expr> {
    let h = derivative_holder(e, var);
    match (f, args) {
        (Func::Log, [x, b]) => {
            // log_b(x) = ln(x) / ln(b)
            let rewritten = mul(vec![
                func(Func::Ln, vec![x.clone()]),
                pow(func(Func::Ln, vec![b.clone()]), Expr::int(-1)),
            ]);
            step(
                log,
                "change of base",
                h,
                derivative_holder(&rewritten, var),
                None,
            );
            d(&rewritten, var, log)
        }
        (Func::Atan2, [y, x]) => {
            // d atan2(y, x) = (x y′ − y x′) / (x² + y²)
            let shown = raw_mul(vec![
                raw_add(vec![
                    raw_mul(vec![x.clone(), derivative_holder(y, var)]),
                    raw_mul(vec![Expr::int(-1), y.clone(), derivative_holder(x, var)]),
                ]),
                pow(
                    add(vec![
                        pow(x.clone(), Expr::int(2)),
                        pow(y.clone(), Expr::int(2)),
                    ]),
                    Expr::int(-1),
                ),
            ]);
            step(log, "derivative of atan2", h, shown, None);
            let dy = d(y, var, log)?;
            let dx = d(x, var, log)?;
            let top = add(vec![
                mul(vec![x.clone(), dy]),
                mul(vec![Expr::int(-1), y.clone(), dx]),
            ]);
            Ok(mul(vec![
                top,
                pow(
                    add(vec![
                        pow(x.clone(), Expr::int(2)),
                        pow(y.clone(), Expr::int(2)),
                    ]),
                    Expr::int(-1),
                ),
            ]))
        }
        (_, [u]) => {
            let Some(outer) = outer_derivative(f, u) else {
                return Ok(h);
            };
            if u.is_sym(var) {
                step(
                    log,
                    &format!("derivative of {}", f.name()),
                    h,
                    outer.clone(),
                    None,
                );
                return Ok(outer);
            }
            step(
                log,
                "chain rule",
                h,
                raw_mul(vec![outer.clone(), derivative_holder(u, var)]),
                Some(format!("derivative of {} with u = {u}", f.name())),
            );
            let du = d(u, var, log)?;
            Ok(mul(vec![outer, du]))
        }
        _ => Ok(h),
    }
}

impl Expr {
    /// The `order`-th derivative with respect to `var` (no steps).
    pub fn diff(&self, var: &str, order: u32) -> MathResult<Expr> {
        diff(self, var, order, None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn x() -> Expr {
        Expr::sym("x")
    }
    fn n(k: i64) -> Expr {
        Expr::int(k)
    }
    fn f(name: &str, a: Expr) -> Expr {
        Expr::call(name, vec![a]).unwrap()
    }
    fn dx(e: &Expr) -> Expr {
        diff(e, "x", 1, None).unwrap()
    }

    #[test]
    fn basic_rules() {
        assert_eq!(dx(&n(7)), n(0));
        assert_eq!(dx(&x()), n(1));
        assert_eq!(dx(&x().pow(5)), n(5) * x().pow(4));
        assert_eq!(dx(&(n(3) * x().pow(2) + n(5))), n(6) * x());
        assert_eq!(dx(&x().sqrt()).to_string(), "1/(2*sqrt(x))");
        assert_eq!(dx(&x().recip()), -x().pow(-2));
        assert_eq!(dx(&(Expr::sym("a") * x())), Expr::sym("a"));
    }

    #[test]
    fn product_quotient_chain() {
        let e = x().pow(2) * f("sin", x());
        assert_eq!(dx(&e).to_string(), "x^2*cos(x) + 2*x*sin(x)");
        let q = x() / (x() + n(1));
        assert_eq!(dx(&q).to_string(), "1/(x + 1)^2");
        let c = f("sin", x().pow(2));
        assert_eq!(dx(&c), n(2) * x() * f("cos", x().pow(2)));
        let nested = f("exp", f("sin", n(3) * x()));
        assert_eq!(
            dx(&nested),
            n(3) * f("cos", n(3) * x()) * f("exp", f("sin", n(3) * x()))
        );
        assert_eq!(dx(&(x() + n(1)).pow(3)), n(3) * (x() + n(1)).pow(2));
    }

    #[test]
    fn every_function() {
        let u = x();
        let cases: Vec<(&str, Expr)> = vec![
            ("sin", f("cos", u.clone())),
            ("cos", -f("sin", u.clone())),
            ("tan", f("sec", u.clone()).pow(2)),
            ("sec", f("sec", u.clone()) * f("tan", u.clone())),
            ("csc", -(f("csc", u.clone()) * f("cot", u.clone()))),
            ("cot", -f("csc", u.clone()).pow(2)),
            ("asin", (n(1) - x().pow(2)).pow(Expr::rational(-1, 2))),
            ("acos", -(n(1) - x().pow(2)).pow(Expr::rational(-1, 2))),
            ("atan", (n(1) + x().pow(2)).recip()),
            ("sinh", f("cosh", u.clone())),
            ("cosh", f("sinh", u.clone())),
            ("tanh", f("cosh", u.clone()).pow(-2)),
            ("asinh", (x().pow(2) + n(1)).pow(Expr::rational(-1, 2))),
            ("acosh", (x().pow(2) - n(1)).pow(Expr::rational(-1, 2))),
            ("atanh", (n(1) - x().pow(2)).recip()),
            ("ln", x().recip()),
            ("exp", f("exp", u.clone())),
            ("abs", f("sign", u.clone())),
            ("floor", n(0)),
            ("Gamma", f("Gamma", u.clone()) * f("digamma", u.clone())),
        ];
        for (name, expected) in cases {
            assert_eq!(dx(&f(name, x())), expected, "d/dx {name}(x)");
        }
        let lg = Expr::call("log", vec![x(), n(2)]).unwrap();
        assert_eq!(dx(&lg), (x() * f("ln", n(2))).recip());
        assert_eq!(dx(&n(2).pow(x())), n(2).pow(x()) * f("ln", n(2)));
        let at = Expr::call("atan2", vec![x(), n(1)]).unwrap();
        assert_eq!(dx(&at), (x().pow(2) + n(1)).recip());
    }

    #[test]
    fn general_power() {
        let e = x().pow(x());
        assert_eq!(dx(&e), x().pow(x()) * (f("ln", x()) + n(1)));
    }

    #[test]
    fn user_functions_stay_unevaluated() {
        let g = Expr::apply("f", vec![x()]);
        let d1 = dx(&g);
        assert_eq!(d1.to_string(), "d/dx(f(x))");
        let d2 = diff(&g, "x", 2, None).unwrap();
        assert_eq!(d2.to_string(), "d^2/dx^2(f(x))");
        assert_eq!(dx(&(n(3) * g.clone())), n(3) * d1);
    }

    #[test]
    fn higher_orders_and_partials() {
        assert_eq!(diff(&x().pow(4), "x", 3, None).unwrap(), n(24) * x());
        assert_eq!(diff(&f("sin", x()), "x", 4, None).unwrap(), f("sin", x()));
        let y = Expr::sym("y");
        let e = x().pow(2) * y.clone() + y.pow(3);
        assert_eq!(
            diff(&e, "y", 1, None).unwrap(),
            x().pow(2) + n(3) * y.pow(2)
        );
        assert_eq!(diff(&e, "x", 0, None).unwrap(), e);
    }

    #[test]
    fn steps_record_the_rules() {
        let mut steps = Steps::new();
        let e = x().pow(2) * f("sin", x());
        let result = diff(&e, "x", 1, Some(&mut steps)).unwrap();
        let rules: Vec<&str> = steps.iter().map(|s| s.rule.as_str()).collect();
        assert_eq!(
            rules,
            vec!["product rule", "power rule", "derivative of sin"]
        );
        assert_eq!(
            steps.iter().next().map(|s| s.before.clone()),
            Some(derivative_holder(&e, "x"))
        );
        assert_eq!(result, dx(&e));

        let mut steps = Steps::new();
        let nested = f("sin", f("cos", x().pow(2)));
        diff(&nested, "x", 1, Some(&mut steps)).unwrap();
        let rules: Vec<&str> = steps.iter().map(|s| s.rule.as_str()).collect();
        assert_eq!(rules, vec!["chain rule", "chain rule", "power rule"]);
        assert!(steps.iter().next().and_then(|s| s.note.clone()).is_some());

        let mut steps = Steps::new();
        diff(&(n(3) * x().pow(2) + n(5)), "x", 1, Some(&mut steps)).unwrap();
        let rules: Vec<&str> = steps.iter().map(|s| s.rule.as_str()).collect();
        assert_eq!(
            rules,
            vec![
                "sum rule",
                "constant multiple",
                "power rule",
                "constant rule"
            ]
        );

        let mut steps = Steps::new();
        diff(&(x() / (x() + n(1))), "x", 1, Some(&mut steps)).unwrap();
        assert_eq!(
            steps.iter().next().map(|s| s.rule.as_str()),
            Some("quotient rule")
        );
    }
}
