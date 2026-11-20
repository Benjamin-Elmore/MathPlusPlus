//! Helpers shared by the wider-field domains (extended number theory, special
//! functions, analysis, discrete maths, probability and information, algebra and
//! applied maths): argument readers with size caps, numeric evaluation with bound
//! variables, and small constructors for results.

use num_bigint::BigInt;
use num_complex::Complex64;
use num_traits::ToPrimitive;

use crate::domains::util::{int_arg, items, real_arg};
use crate::error::{MathError, MathResult};
use crate::eval::Env;
use crate::expr::{Expr, ExprNode, LogicOp, RelOp};
use crate::number::Number;

/// A domain error for operator `op`.
pub fn domain_err(op: &str, detail: impl Into<String>) -> MathError {
    MathError::Domain {
        function: op.into(),
        detail: detail.into(),
    }
}

/// A tuple expression.
pub fn tuple(v: Vec<Expr>) -> Expr {
    Expr::new(ExprNode::Tuple(v))
}

/// A truth value.
pub fn truth(b: bool) -> Expr {
    Expr::new(ExprNode::Bool(b))
}

/// An approximate real number.
pub fn float(x: f64) -> Expr {
    Expr::float(x)
}

/// An approximate complex number (real when the imaginary part is exactly zero).
pub fn complex(z: Complex64) -> Expr {
    if z.im == 0.0 {
        Expr::float(z.re)
    } else {
        Expr::num(Number::complex_float(z.re, z.im))
    }
}

/// A matrix of exact expressions given row by row (rows must have equal length).
pub fn matrix_expr(rows: Vec<Vec<Expr>>) -> Expr {
    let r = rows.len();
    let c = rows.first().map_or(0, Vec::len);
    Expr::new(ExprNode::Matrix {
        rows: r,
        cols: c,
        data: rows.into_iter().flatten().collect(),
    })
}

/// A non-negative integer argument no larger than `max`.
pub fn usize_arg(op: &str, e: &Expr, max: usize) -> MathResult<usize> {
    let n = int_arg(op, e)?;
    let v = n
        .to_usize()
        .ok_or_else(|| domain_err(op, format!("{e} must be a non-negative integer")))?;
    if v > max {
        return Err(MathError::TooLarge(format!(
            "{op} with {v} (the limit is {max})"
        )));
    }
    Ok(v)
}

/// A positive integer argument.
pub fn positive_int(op: &str, e: &Expr) -> MathResult<BigInt> {
    let n = int_arg(op, e)?;
    if n.sign() != num_bigint::Sign::Plus {
        return Err(domain_err(op, format!("{e} must be a positive integer")));
    }
    Ok(n)
}

/// A complex numeric argument (any constant expression).
pub fn complex_arg(op: &str, e: &Expr) -> MathResult<Complex64> {
    if !e.is_constant() {
        return Err(domain_err(op, format!("{e} is not a number")));
    }
    let z = e
        .eval_complex(&FieldEnv::empty())
        .map_err(|_| domain_err(op, format!("{e} is not a number")))?;
    if z.re.is_finite() && z.im.is_finite() {
        Ok(z)
    } else {
        Err(domain_err(op, format!("{e} is not a finite number")))
    }
}

/// The items of one list-like argument (tuple, set or vector).
pub fn list_of(op: &str, e: &Expr) -> MathResult<Vec<Expr>> {
    items(op, std::slice::from_ref(e))
}

/// A list of reals from one argument.
pub fn real_list_of(op: &str, e: &Expr) -> MathResult<Vec<f64>> {
    list_of(op, e)?.iter().map(|x| real_arg(op, x)).collect()
}

/// A list of complex numbers from one argument.
pub fn complex_list_of(op: &str, e: &Expr) -> MathResult<Vec<Complex64>> {
    list_of(op, e)?.iter().map(|x| complex_arg(op, x)).collect()
}

/// The rows of a matrix argument as expressions.
pub fn matrix_rows(op: &str, e: &Expr) -> MathResult<Vec<Vec<Expr>>> {
    match e.node() {
        ExprNode::Matrix { rows, cols, data } => Ok((0..*rows)
            .map(|r| data[r * cols..(r + 1) * cols].to_vec())
            .collect()),
        ExprNode::Tuple(rs) if rs.iter().all(|r| matches!(r.node(), ExprNode::Tuple(_))) => {
            let out: Vec<Vec<Expr>> = rs
                .iter()
                .map(|r| match r.node() {
                    ExprNode::Tuple(v) => v.clone(),
                    _ => Vec::new(),
                })
                .collect();
            let c = out.first().map_or(0, Vec::len);
            if c == 0 || out.iter().any(|r| r.len() != c) {
                return Err(domain_err(op, "the rows have different lengths"));
            }
            Ok(out)
        }
        _ => Err(domain_err(op, format!("{e} is not a matrix"))),
    }
}

/// A matrix of reals, at most `max` rows and columns.
pub fn real_matrix(op: &str, e: &Expr, max: usize) -> MathResult<Vec<Vec<f64>>> {
    let rows = matrix_rows(op, e)?;
    if rows.len() > max || rows.first().map_or(0, Vec::len) > max {
        return Err(MathError::TooLarge(format!("a matrix this size in {op}")));
    }
    rows.iter()
        .map(|r| r.iter().map(|x| real_arg(op, x)).collect())
        .collect()
}

/// The names in a variable list argument: one symbol or a tuple of symbols.
pub fn var_list(op: &str, e: &Expr) -> MathResult<Vec<String>> {
    let names = match e.node() {
        ExprNode::Tuple(v) => v.clone(),
        _ => vec![e.clone()],
    };
    names
        .iter()
        .map(|n| {
            n.as_sym()
                .map(str::to_string)
                .ok_or_else(|| domain_err(op, format!("{n} is not a variable name")))
        })
        .collect()
}

/// Numeric bindings for evaluation, with the wider-field functions available by name.
pub struct FieldEnv<'a> {
    names: &'a [String],
    values: &'a [f64],
}

impl<'a> FieldEnv<'a> {
    /// No bindings.
    pub fn empty() -> FieldEnv<'static> {
        FieldEnv {
            names: &[],
            values: &[],
        }
    }

    /// Binds `names[i]` to `values[i]`.
    pub fn new(names: &'a [String], values: &'a [f64]) -> FieldEnv<'a> {
        FieldEnv { names, values }
    }
}

impl Env for FieldEnv<'_> {
    fn real(&self, name: &str) -> Option<f64> {
        self.names
            .iter()
            .position(|n| n == name)
            .and_then(|i| self.values.get(i).copied())
    }

    fn call_real(&self, name: &str, args: &[f64]) -> Option<MathResult<f64>> {
        crate::domains::special::call_real(name, args)
    }

    fn call_complex(&self, name: &str, args: &[Complex64]) -> Option<MathResult<Complex64>> {
        crate::domains::special::call_complex(name, args)
    }
}

/// The value of `e` with `names` bound to `values`. Relations and logical
/// combinations, anywhere in `e`, count as 1 when true and 0 when false (indicators).
pub fn eval_at(e: &Expr, names: &[String], values: &[f64]) -> MathResult<f64> {
    NumericFn::new(e, names).eval(values)
}

/// An expression prepared for repeated numeric evaluation: each relation inside it is
/// replaced by a placeholder whose value (1 or 0) is worked out per point.
pub struct NumericFn {
    body: Expr,
    conds: Vec<Expr>,
    names: Vec<String>,
}

/// Prefix of the placeholder names (`#` never appears in a parsed name).
const PLACEHOLDER: &str = "#ind";

fn lift_conditions(e: &Expr, conds: &mut Vec<Expr>) -> Expr {
    match e.node() {
        ExprNode::Relation(..) | ExprNode::Logic(..) | ExprNode::Bool(_) => {
            conds.push(e.clone());
            Expr::sym(&format!("{PLACEHOLDER}{}", conds.len() - 1))
        }
        _ if e.children().is_empty() => e.clone(),
        _ => Expr::new(e.map_children(&mut |c| lift_conditions(c, conds))),
    }
}

impl NumericFn {
    /// Prepares `e` for evaluation with `names` bound in this order.
    pub fn new(e: &Expr, names: &[String]) -> NumericFn {
        let mut conds = Vec::new();
        let body = lift_conditions(e, &mut conds);
        let mut all = names.to_vec();
        all.extend((0..conds.len()).map(|k| format!("{PLACEHOLDER}{k}")));
        NumericFn {
            body,
            conds,
            names: all,
        }
    }

    /// The value at `values` (one per name given to [`NumericFn::new`]).
    pub fn eval(&self, values: &[f64]) -> MathResult<f64> {
        let n_vars = self.names.len() - self.conds.len();
        if values.len() != n_vars {
            return Err(crate::domains::util::arity(
                "evaluation",
                &n_vars.to_string(),
                values.len(),
            ));
        }
        if self.conds.is_empty() {
            return self.body.eval_f64(&FieldEnv::new(&self.names, values));
        }
        let env = FieldEnv::new(&self.names[..n_vars], values);
        let mut all = values.to_vec();
        for c in &self.conds {
            all.push(if truth_of(c, &env)? { 1.0 } else { 0.0 });
        }
        self.body.eval_f64(&FieldEnv::new(&self.names, &all))
    }
}

fn truth_of(e: &Expr, env: &FieldEnv<'_>) -> MathResult<bool> {
    match e.node() {
        ExprNode::Bool(b) => Ok(*b),
        ExprNode::Relation(op, a, b) => {
            let (a, b) = (a.eval_f64(env)?, b.eval_f64(env)?);
            Ok(match op {
                RelOp::Eq => a == b,
                RelOp::Ne => a != b,
                RelOp::Lt => a < b,
                RelOp::Le => a <= b,
                RelOp::Gt => a > b,
                RelOp::Ge => a >= b,
                RelOp::Approx => (a - b).abs() <= 1e-9 * a.abs().max(b.abs()).max(1.0),
            })
        }
        ExprNode::Logic(LogicOp::Not, args) => match args.first() {
            Some(a) => Ok(!truth_of(a, env)?),
            None => Err(MathError::NotANumber("an empty condition".into())),
        },
        ExprNode::Logic(LogicOp::And, args) => {
            for a in args {
                if !truth_of(a, env)? {
                    return Ok(false);
                }
            }
            Ok(true)
        }
        ExprNode::Logic(LogicOp::Or, args) => {
            for a in args {
                if truth_of(a, env)? {
                    return Ok(true);
                }
            }
            Ok(false)
        }
        _ => Ok(e.eval_f64(env)? != 0.0),
    }
}

/// True when `|a − b| ≤ tol·max(1, |a|, |b|)`.
pub fn close(a: f64, b: f64, tol: f64) -> bool {
    (a - b).abs() <= tol * 1f64.max(a.abs()).max(b.abs())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn indicator_evaluation() {
        let x = Expr::sym("x");
        let cond = Expr::relation(RelOp::Le, x.pow(2), Expr::one());
        let names = vec!["x".to_string()];
        assert_eq!(eval_at(&cond, &names, &[0.5]).unwrap(), 1.0);
        assert_eq!(eval_at(&cond, &names, &[1.5]).unwrap(), 0.0);
        assert_eq!(
            eval_at(&(x.clone() * Expr::int(2)), &names, &[1.5]).unwrap(),
            3.0
        );
    }

    #[test]
    fn argument_readers() {
        assert_eq!(usize_arg("f", &Expr::int(5), 10).unwrap(), 5);
        assert!(usize_arg("f", &Expr::int(50), 10).is_err());
        assert!(usize_arg("f", &Expr::int(-1), 10).is_err());
        let m = Expr::matrix(vec![vec![Expr::int(1), Expr::int(2)]]).unwrap();
        assert_eq!(real_matrix("f", &m, 4).unwrap(), vec![vec![1.0, 2.0]]);
        let vars = tuple(vec![Expr::sym("q"), Expr::sym("p")]);
        assert_eq!(var_list("f", &vars).unwrap(), vec!["q", "p"]);
    }
}
