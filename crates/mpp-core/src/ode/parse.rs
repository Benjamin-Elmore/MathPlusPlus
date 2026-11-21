//! Reading a differential equation: which function is unknown, the independent variable,
//! the order, and the equation as `F(x, y, y′, …, y⁽ⁿ⁾) = 0` with each derivative replaced
//! by a placeholder symbol (`y`, `y′`, `y″`, …) so that ordinary algebra applies.

use std::collections::BTreeSet;

use crate::error::{MathError, MathResult};
use crate::expr::{Expr, ExprNode, RelOp};
use crate::simplify::{add, canonical, mul, simplify};

/// Highest order handled.
pub const MAX_ORDER: u32 = 12;

/// An ordinary differential equation in placeholder form.
#[derive(Clone, Debug, PartialEq)]
pub struct Ode {
    /// The independent variable.
    pub x: String,
    /// The unknown function's name.
    pub y: String,
    /// The order.
    pub order: u32,
    /// `F` with `F = 0`, in the placeholders [`Ode::d`].
    pub residual: Expr,
    /// The equation as written (for messages and steps).
    pub original: Expr,
}

impl Ode {
    /// The placeholder for the `k`-th derivative of `y` (`y`, `y′`, `y″`, `y‴`, `y⁽⁴⁾`, …).
    pub fn d(&self, k: u32) -> Expr {
        Expr::sym(&placeholder(&self.y, k))
    }

    /// The placeholder's name.
    pub fn d_name(&self, k: u32) -> String {
        placeholder(&self.y, k)
    }

    /// Builds an equation directly from a residual in placeholder form.
    pub fn from_residual(residual: Expr, x: &str, y: &str) -> Ode {
        let order = (0..=MAX_ORDER)
            .rev()
            .find(|k| residual.contains_sym(&placeholder(y, *k)))
            .unwrap_or(0);
        Ode {
            x: x.into(),
            y: y.into(),
            order,
            original: Expr::equation(residual.clone(), Expr::zero()),
            residual,
        }
    }

    /// The residual with `y⁽ᵏ⁾ → values[k]`.
    pub fn substitute(&self, values: &[Expr]) -> Expr {
        let names: Vec<String> = (0..values.len() as u32).map(|k| self.d_name(k)).collect();
        let pairs: Vec<(&str, Expr)> = names
            .iter()
            .map(String::as_str)
            .zip(values.iter().cloned())
            .collect();
        self.residual.subs_all(&pairs)
    }

    /// True when every placeholder appears linearly and never multiplied by another.
    pub fn is_linear(&self) -> bool {
        self.linear_coefficients().is_some()
    }

    /// `(a₀, …, aₙ, g)` with `Σ aₖ y⁽ᵏ⁾ = g` when the equation is linear.
    pub fn linear_coefficients(&self) -> Option<(Vec<Expr>, Expr)> {
        let mut rest = crate::expand::expand(&self.residual).unwrap_or(self.residual.clone());
        let mut coeffs = vec![Expr::zero(); self.order as usize + 1];
        for k in (0..=self.order).rev() {
            let name = self.d_name(k);
            let c = crate::poly::as_poly(&rest, &name)?;
            match c.len() {
                0 | 1 => {}
                2 => {
                    coeffs[k as usize] = simplify(&c[1]);
                    rest = c[0].clone();
                }
                _ => return None,
            }
        }
        let placeholders: Vec<String> = (0..=self.order).map(|k| self.d_name(k)).collect();
        if coeffs
            .iter()
            .chain(std::iter::once(&rest))
            .any(|c| placeholders.iter().any(|p| c.contains_sym(p)))
        {
            return None;
        }
        Some((coeffs, simplify(&mul(vec![Expr::int(-1), rest]))))
    }
}

/// The placeholder name for `y⁽ᵏ⁾`.
pub fn placeholder(y: &str, k: u32) -> String {
    match k {
        0 => y.to_string(),
        1 => format!("{y}′"),
        2 => format!("{y}″"),
        3 => format!("{y}‴"),
        _ => {
            const SUP: [char; 10] = ['⁰', '¹', '²', '³', '⁴', '⁵', '⁶', '⁷', '⁸', '⁹'];
            let digits: String = k
                .to_string()
                .chars()
                .filter_map(|c| c.to_digit(10).map(|d| SUP[d as usize]))
                .collect();
            format!("{y}⁽{digits}⁾")
        }
    }
}

/// The function names and variables of the derivatives in `e`: `(y, x, order)`.
fn derivatives(e: &Expr, out: &mut Vec<(String, String, u32)>) {
    if let ExprNode::Derivative { expr, vars } = e.node() {
        let mut inner = expr;
        let mut total: u32 = vars.iter().map(|(_, k)| *k).sum();
        let mut names: BTreeSet<String> = vars.iter().map(|(v, _)| v.to_string()).collect();
        while let ExprNode::Derivative { expr, vars } = inner.node() {
            total += vars.iter().map(|(_, k)| *k).sum::<u32>();
            names.extend(vars.iter().map(|(v, _)| v.to_string()));
            inner = expr;
        }
        if let (Some(f), [x]) = (
            function_name(inner),
            names.into_iter().collect::<Vec<_>>().as_slice(),
        ) {
            out.push((f, x.clone(), total));
        }
    }
    for c in e.children() {
        derivatives(c, out);
    }
}

/// `y` for `y` or `y(x)`.
fn function_name(e: &Expr) -> Option<String> {
    match e.node() {
        ExprNode::Sym(s) => Some(s.to_string()),
        ExprNode::Apply(f, args) if args.len() == 1 && args[0].as_sym().is_some() => {
            Some(f.to_string())
        }
        _ => None,
    }
}

/// Names written with primes, such as the symbol `y'` or the call `y''(x)`.
fn primed(e: &Expr, out: &mut Vec<(String, u32)>) {
    let read = |s: &str| -> Option<(String, u32)> {
        let base = s.trim_end_matches(['\'', '′', '″', '‴']);
        if base.is_empty() || base.len() == s.len() {
            return None;
        }
        let k: u32 = s[base.len()..]
            .chars()
            .map(|c| match c {
                '″' => 2,
                '‴' => 3,
                _ => 1,
            })
            .sum();
        Some((base.to_string(), k))
    };
    match e.node() {
        ExprNode::Sym(s) => out.extend(read(s)),
        ExprNode::Apply(f, args) if args.len() == 1 => out.extend(read(f)),
        _ => {}
    }
    for c in e.children() {
        primed(c, out);
    }
}

/// True when `e` mentions a derivative of some function (so it is a differential equation).
pub fn is_differential_equation(e: &Expr) -> bool {
    let mut d = Vec::new();
    derivatives(e, &mut d);
    let mut p = Vec::new();
    primed(e, &mut p);
    !d.is_empty() || !p.is_empty()
}

/// Reads `eq` (an equation, or an expression meaning `= 0`) as an ODE in the unknown `y`
/// of `x`. Both are found from the derivatives when not given.
pub fn parse(eq: &Expr, y: Option<&str>, x: Option<&str>) -> MathResult<Ode> {
    let mut found = Vec::new();
    derivatives(eq, &mut found);
    let mut primes = Vec::new();
    primed(eq, &mut primes);
    let y = match y {
        Some(y) => y.to_string(),
        None => {
            let mut names: BTreeSet<String> = found.iter().map(|(f, _, _)| f.clone()).collect();
            names.extend(primes.iter().map(|(f, _)| f.clone()));
            match names.len() {
                0 => {
                    return Err(MathError::Domain {
                        function: "dsolve".into(),
                        detail: format!("{eq} has no derivative, so it is not a differential equation"),
                    })
                }
                1 => names.into_iter().next().unwrap_or_default(),
                _ => {
                    return Err(MathError::Unsupported(format!(
                        "one equation in several unknown functions ({}); write a system as a tuple of equations",
                        names.into_iter().collect::<Vec<_>>().join(", ")
                    )))
                }
            }
        }
    };
    let x = match x {
        Some(x) => x.to_string(),
        None => {
            let vars: BTreeSet<String> = found
                .iter()
                .filter(|(f, _, _)| *f == y)
                .map(|(_, v, _)| v.clone())
                .collect();
            match vars.len() {
                0 => guess_variable(eq, &y),
                1 => vars.into_iter().next().unwrap_or_else(|| "x".into()),
                _ => {
                    return Err(MathError::Unsupported(
                        "partial derivatives in several variables (a PDE); see the heat, wave and Laplace helpers"
                            .into(),
                    ))
                }
            }
        }
    };
    if x == y {
        return Err(MathError::Domain {
            function: "dsolve".into(),
            detail: "the unknown function and the variable have the same name".into(),
        });
    }
    let n = normalise(eq, &x, &y)?;
    let residual = match n.node() {
        ExprNode::Relation(RelOp::Eq, l, r) => {
            simplify(&add(vec![l.clone(), mul(vec![Expr::int(-1), r.clone()])]))
        }
        ExprNode::Relation(..) => {
            return Err(MathError::Unsupported("differential inequalities".into()))
        }
        _ => simplify(&n),
    };
    // Primes are read as d/dx; when the equation is written in t (and has no x), the
    // textbook convention is that the primes mean d/dt.
    let x = if x == "x" && !residual.contains_sym("x") && residual.contains_sym("t") && y != "t" {
        "t".to_string()
    } else {
        x
    };
    let order = (0..=MAX_ORDER)
        .rev()
        .find(|k| residual.contains_sym(&placeholder(&y, *k)) && *k > 0)
        .ok_or_else(|| MathError::Domain {
            function: "dsolve".into(),
            detail: format!("{eq} has no derivative of {y}"),
        })?;
    Ok(Ode {
        x,
        y,
        order,
        residual,
        original: eq.clone(),
    })
}

/// The variable when only primes were written: `t` if it appears, else `x`.
fn guess_variable(eq: &Expr, y: &str) -> String {
    let free = eq.free_symbols();
    for cand in ["x", "t", "s", "r", "θ"] {
        if cand != y && free.iter().any(|s| &**s == cand) {
            return cand.into();
        }
    }
    if y == "x" {
        "t".into()
    } else {
        "x".into()
    }
}

/// Replaces `y(x)` by `y` and every derivative of `y` by its placeholder.
fn normalise(e: &Expr, x: &str, y: &str) -> MathResult<Expr> {
    match e.node() {
        ExprNode::Derivative { expr, vars } => {
            let inner = normalise(expr, x, y)?;
            if vars.iter().any(|(v, _)| &**v != x) {
                if inner.contains_sym(y) {
                    return Err(MathError::Unsupported(
                        "partial derivatives (a PDE); see the heat, wave and Laplace helpers"
                            .into(),
                    ));
                }
                return Ok(e.clone());
            }
            let k: u32 = vars.iter().map(|(_, k)| *k).sum();
            if let Some(j) = order_of(&inner, y) {
                return Ok(Expr::sym(&placeholder(y, j + k)));
            }
            // A derivative of an expression in y: differentiate it with placeholders.
            Ok(total_derivative(&inner, x, y, k))
        }
        ExprNode::Apply(f, args) if args.len() == 1 => {
            let a = normalise(&args[0], x, y)?;
            if &**f == y && a.is_sym(x) {
                return Ok(Expr::sym(y));
            }
            let base = f.trim_end_matches(['\'', '′', '″', '‴']);
            if base == y && base.len() < f.len() && a.is_sym(x) {
                let k = prime_count(&f[base.len()..]);
                return Ok(Expr::sym(&placeholder(y, k)));
            }
            Ok(canonical(ExprNode::Apply(f.clone(), vec![a])))
        }
        ExprNode::Sym(s) => {
            let base = s.trim_end_matches(['\'', '′', '″', '‴']);
            if base == y && base.len() < s.len() {
                return Ok(Expr::sym(&placeholder(y, prime_count(&s[base.len()..]))));
            }
            Ok(e.clone())
        }
        _ => {
            let mut failure = None;
            let node = e.map_children(&mut |c| match normalise(c, x, y) {
                Ok(v) => v,
                Err(err) => {
                    failure.get_or_insert(err);
                    c.clone()
                }
            });
            match failure {
                Some(err) => Err(err),
                None => Ok(canonical(node)),
            }
        }
    }
}

fn prime_count(s: &str) -> u32 {
    s.chars()
        .map(|c| match c {
            '″' => 2,
            '‴' => 3,
            _ => 1,
        })
        .sum()
}

/// `Some(j)` when `e` is the placeholder of `y⁽ʲ⁾`.
fn order_of(e: &Expr, y: &str) -> Option<u32> {
    let s = e.as_sym()?;
    (0..=MAX_ORDER).find(|k| placeholder(y, *k) == s)
}

/// `dᵏ/dxᵏ e` where `e` contains placeholders: the chain rule through each `y⁽ʲ⁾`.
fn total_derivative(e: &Expr, x: &str, y: &str, k: u32) -> Expr {
    let mut cur = e.clone();
    for _ in 0..k {
        let mut terms = Vec::new();
        if let Ok(d) = crate::calc::diff::diff(&cur, x, 1, None) {
            terms.push(d);
        }
        for j in 0..MAX_ORDER {
            let name = placeholder(y, j);
            if !cur.contains_sym(&name) {
                continue;
            }
            if let Ok(d) = crate::calc::diff::diff(&cur, &name, 1, None) {
                terms.push(mul(vec![d, Expr::sym(&placeholder(y, j + 1))]));
            }
        }
        cur = simplify(&add(terms));
    }
    cur
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::expr::Name;

    fn d(e: Expr, x: &str, k: u32) -> Expr {
        Expr::new(ExprNode::Derivative {
            expr: e,
            vars: vec![(Name::from(x), k)],
        })
    }

    #[test]
    fn reads_orders_and_names() {
        let y = Expr::sym("y");
        let x = Expr::sym("x");
        let eq = Expr::equation(d(y.clone(), "x", 2) + y.clone(), x.clone());
        let ode = parse(&eq, None, None).unwrap();
        assert_eq!((ode.y.as_str(), ode.x.as_str(), ode.order), ("y", "x", 2));
        assert_eq!(ode.residual.to_string(), "-x + y + y″");
        let lin = ode.linear_coefficients().unwrap();
        assert_eq!(lin.1, x);
        // y(t) written as a call, with t as the variable.
        let yt = Expr::apply("y", vec![Expr::sym("t")]);
        let eq = Expr::equation(d(yt.clone(), "t", 1), yt);
        let ode = parse(&eq, None, None).unwrap();
        assert_eq!((ode.x.as_str(), ode.order), ("t", 1));
        // Primes typed as names.
        let eq = Expr::equation(Expr::sym("y''") + Expr::sym("y"), Expr::zero());
        let ode = parse(&eq, None, None).unwrap();
        assert_eq!(ode.order, 2);
        assert!(!is_differential_equation(&(x.clone() + y)));
    }
}
