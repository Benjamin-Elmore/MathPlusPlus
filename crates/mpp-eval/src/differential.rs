//! How a total differential is shown: `df = 2xy\,dx + x^2\,dy`, one term per
//! variable in the order of the function's parameters, each coefficient followed by its
//! differential (not `2*dx*x*y + dy*x^2`, the canonical order of the product).

use mpp_core::{Expr, ExprNode};

/// The variables of a total differential as lowered (`f_x dx + f_y dy`, see
/// `Lowerer::total_differential`): for each term, the `v` of its `d/dv` whose `dv`
/// multiplies it. `None` when `input` is not one.
fn variables(input: &Expr) -> Option<Vec<String>> {
    let terms: Vec<&Expr> = match input.node() {
        ExprNode::Add(ts) => ts.iter().collect(),
        _ => vec![input],
    };
    let mut vars = Vec::with_capacity(terms.len());
    for t in terms {
        let ExprNode::Mul(fs) = t.node() else {
            return None;
        };
        let var = fs.iter().find_map(|f| match f.node() {
            ExprNode::Derivative { vars, .. } if vars.len() == 1 && vars[0].1 == 1 => {
                Some(vars[0].0.to_string())
            }
            _ => None,
        })?;
        let dv = format!("d{var}");
        if !fs.iter().any(|f| f.is_sym(&dv)) {
            return None;
        }
        vars.push(var);
    }
    (!vars.is_empty()).then_some(vars)
}

/// Text and LaTeX for the value of a total differential, or `None` when `input` is not
/// one (or the value is not linear in the differentials).
pub fn present(input: &Expr, value: &Expr) -> Option<(String, String)> {
    let vars = variables(input)?;
    let mut text = String::new();
    let mut latex = String::new();
    let mut rest = value.clone();
    for v in &vars {
        let dv = format!("d{v}");
        // The value is linear in each differential, so its coefficient is ∂value/∂dv.
        let coef = value.diff(&dv, 1).ok()?.simplify();
        if coef
            .free_symbols()
            .iter()
            .any(|s| s.starts_with('d') && vars.iter().any(|w| **s == format!("d{w}")))
        {
            return None;
        }
        rest = (rest - coef.clone() * Expr::sym(&dv)).simplify();
        if coef.is_zero() {
            continue;
        }
        let (neg, mag) = match coef.as_number() {
            Some(n) if n.is_negative() => (true, (-coef.clone()).simplify()),
            _ => match split_negative(&coef) {
                Some(m) => (true, m),
                None => (false, coef.clone()),
            },
        };
        let sep = match (text.is_empty(), neg) {
            (true, false) => "",
            (true, true) => "-",
            (false, false) => " + ",
            (false, true) => " - ",
        };
        let one = mag.is_one();
        let wrap = matches!(mag.node(), ExprNode::Add(_));
        let (t, l) = (mag.to_string(), mag.to_latex());
        let (t, l) = if wrap {
            (format!("({t})"), format!("\\left({l}\\right)"))
        } else {
            (t, l)
        };
        text.push_str(sep);
        latex.push_str(sep);
        if one {
            text.push_str(&dv);
            latex.push_str(&format!("\\,d{v}"));
        } else {
            text.push_str(&format!("{t} {dv}"));
            latex.push_str(&format!("{l}\\,d{v}"));
        }
    }
    if !rest.is_zero() {
        return None;
    }
    if text.is_empty() {
        return Some(("0".into(), "0".into()));
    }
    Some((text, latex))
}

/// `−a·b` as `a·b` when the product's leading number is negative.
fn split_negative(e: &Expr) -> Option<Expr> {
    let ExprNode::Mul(fs) = e.node() else {
        return None;
    };
    let lead = fs.first()?.as_number()?;
    if !lead.is_negative() {
        return None;
    }
    Some((-e.clone()).simplify())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn total(vars: &[&str]) -> Expr {
        let call = Expr::apply("f", vars.iter().map(|v| Expr::sym(v)).collect());
        mpp_core::add(
            vars.iter()
                .map(|v| {
                    Expr::new(ExprNode::Derivative {
                        expr: call.clone(),
                        vars: vec![((*v).into(), 1)],
                    }) * Expr::sym(&format!("d{v}"))
                })
                .collect(),
        )
        .simplify()
    }

    #[test]
    fn coefficients_come_first_in_parameter_order() {
        let (x, y) = (Expr::sym("x"), Expr::sym("y"));
        let input = total(&["x", "y"]);
        let value =
            Expr::int(2) * x.clone() * y.clone() * Expr::sym("dx") + x.pow(2) * Expr::sym("dy");
        let (text, latex) = present(&input, &value.simplify()).unwrap();
        assert_eq!(text, "2*x*y dx + x^2 dy");
        assert_eq!(latex, "2 x y\\,dx + x^{2}\\,dy");
        let value = -(y.clone() * Expr::sym("dx")) + Expr::sym("dy");
        let (text, _) = present(&input, &value.simplify()).unwrap();
        assert_eq!(text, "-y dx + dy");
        assert!(present(&x, &x).is_none());
    }
}
