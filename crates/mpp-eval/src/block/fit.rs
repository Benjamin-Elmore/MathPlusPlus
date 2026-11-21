//! `Fit $P = a \cdot \dfrac{1}{V} + b$ to the table.`: least squares against the data
//! columns defined by the table above.
//!
//! The model is an equation in data columns (names whose value is a list of entries),
//! known values and unknown parameters (names with no value). Each row gives one
//! residual, left side minus right side, measured in SI units; the parameters are
//! found by Gauss–Newton steps with Levenberg–Marquardt damping, then each gets the
//! unit that makes the model consistent (tried from the units in it). The parameters
//! become constants of the document, and the line reports them with R².

use std::collections::HashMap;

use mpp_core::units::Unit;
use mpp_core::{Expr, ExprNode};
use mpp_syntax::document::{Inline, InlineKind};
use mpp_syntax::math::{MathAst, Statement};
use mpp_syntax::Span;

use super::{Locals, Run};
use crate::error::EvalError;
use crate::result::{LineKind, LineResult};
use crate::units_wire::{self, Folded};
use crate::value::{Binding, VarKind};

/// Most Gauss–Newton steps.
const MAX_STEPS: usize = 200;
/// Most parameters a fit takes.
const MAX_PARAMS: usize = 4;
/// Most unit candidates tried per parameter.
const MAX_CANDIDATES: usize = 8;

/// The maths of a `Fit $…$ to the table.` (or `Fit $…$:`) paragraph.
pub(super) fn fit_sentence(inlines: &[Inline]) -> Option<&MathAst> {
    let mut items = inlines
        .iter()
        .filter(|i| !matches!(&i.kind, InlineKind::Text(t) if t.trim().is_empty()));
    let InlineKind::Text(first) = &items.next()?.kind else {
        return None;
    };
    if first.trim() != "Fit" {
        return None;
    }
    let InlineKind::Math(m) = &items.next()?.kind else {
        return None;
    };
    let rest: String = items
        .map(|i| match &i.kind {
            InlineKind::Text(t) => t.clone(),
            _ => "\u{FFFD}".into(),
        })
        .collect();
    let rest = rest.trim();
    let ends = ["to the table.", "to the table:", "to the table", ":", "."];
    ends.contains(&rest).then_some(&m.ast)
}

impl Run<'_, '_> {
    /// Runs a fit sentence: defines the parameters and reports them.
    pub(super) fn fit(&mut self, ast: &MathAst) {
        let Some(line) = ast.lines.first() else {
            return;
        };
        let mut lr = LineResult::new(line.span, LineKind::Helper);
        match self.fit_line(line, ast.span) {
            Ok((values, report)) => {
                lr.set_value(crate::value::Value::from_expr(values));
                lr.text = Some(report.clone());
                lr.plain = Some(report);
            }
            Err(e) => self.error(line.span, e),
        }
        self.push(lr);
    }

    /// The fitted parameters as equations `(a = …, b = …)`, and the report line.
    fn fit_line(
        &mut self,
        line: &mpp_syntax::math::Line,
        span: Span,
    ) -> Result<(Expr, String), EvalError> {
        let Statement::Relation(node) = &line.statement else {
            return Err(EvalError::Invalid(
                "a fit needs the model as an equation, such as `y = a x + b`".into(),
            ));
        };
        let model = self.lower(node, &[]).map_err(|l| l.error)?;
        let ExprNode::Relation(mpp_core::RelOp::Eq, lhs, rhs) = model.node() else {
            return Err(EvalError::Invalid(
                "a fit needs the model as an equation, such as `y = a x + b`".into(),
            ));
        };
        let difference = lhs.clone() - rhs.clone();
        // Columns, known values and parameters.
        let mut columns: Vec<(String, Vec<Expr>)> = Vec::new();
        let mut params: Vec<String> = Vec::new();
        for name in difference.free_symbols().iter().map(|s| s.to_string()) {
            match self.view().get(&name, true).as_deref() {
                Some(Binding::Variable { value, .. }) => {
                    let v = self
                        .resolve(value, &Locals::new(), span)
                        .map_err(|l| l.error)?;
                    if let ExprNode::Tuple(items) = v.node() {
                        columns.push((name, items.clone()));
                    }
                }
                Some(_) => {}
                None => params.push(name),
            }
        }
        if columns.is_empty() {
            return Err(EvalError::Invalid(
                "a fit needs data columns: define them with a table first".into(),
            ));
        }
        let rows = columns[0].1.len();
        if columns.iter().any(|(_, v)| v.len() != rows) || rows == 0 {
            return Err(EvalError::Invalid(
                "the columns of a fit must have the same length".into(),
            ));
        }
        if params.is_empty() || params.len() > MAX_PARAMS {
            return Err(EvalError::Invalid(format!(
                "a fit needs 1 to {MAX_PARAMS} parameters with no value; this model has {}",
                params.len()
            )));
        }
        if rows < params.len() {
            return Err(EvalError::Invalid(
                "a fit needs at least as many rows as parameters".into(),
            ));
        }
        params.sort();
        // The difference at each row, with the known values in (columns kept apart).
        let col_names: Vec<String> = columns.iter().map(|(n, _)| n.clone()).collect();
        let mut locals = Locals::new();
        for n in col_names.iter().chain(params.iter()) {
            locals.insert(n.clone(), Expr::sym(n));
        }
        let general = self
            .resolve(&difference, &locals, span)
            .map_err(|l| l.error)?;
        let row_exprs: Vec<Expr> = (0..rows)
            .map(|i| {
                let mut e = general.clone();
                for (n, vals) in &columns {
                    e = e.subs(n, &vals[i]);
                }
                e
            })
            .collect();
        let units = param_units(&row_exprs[0], &params).ok_or_else(|| {
            EvalError::Invalid("the units of this model do not fit together".into())
        })?;
        // Plain residuals in SI units, as functions of the parameters' values.
        let mut plain = Vec::with_capacity(rows);
        for e in &row_exprs {
            let mut t = e.clone();
            for (p, u) in params.iter().zip(&units) {
                t = t.subs(p, &(Expr::sym(p) * units_wire::factor(u)));
            }
            match units_wire::fold(&t).map_err(EvalError::Math)? {
                Folded::Known { q, .. } => {
                    plain.push((q.value.clone() * q.unit.scale()).simplify())
                }
                Folded::Unknown => {
                    return Err(EvalError::Invalid(
                        "the units of this model could not be worked out".into(),
                    ))
                }
            }
        }
        let start = vec![1.0; params.len()];
        let lhs_general = self.resolve(lhs, &locals, span).map_err(|l| l.error)?;
        let (best, r2) = least_squares(&plain, &params, start, &lhs_general, &columns)?;
        let mut parts = Vec::new();
        let mut values = Vec::new();
        for ((p, u), x) in params.iter().zip(&units).zip(&best) {
            let value = (Expr::float(*x) * units_wire::factor(u)).simplify();
            values.push(Expr::equation(Expr::sym(p), value.clone()));
            let shown = match units_wire::approximate(&value).ok().flatten() {
                Some(s) => s,
                None => crate::numeric::approximate(&value).unwrap_or_else(|_| value.to_string()),
            };
            parts.push(format!("{} ≈ {shown}", Expr::sym(p).to_text()));
            self.define(
                p,
                Binding::Variable {
                    kind: VarKind::Constant,
                    value,
                    origin: self.origin(span),
                },
            );
        }
        if let Some(r2) = r2 {
            parts.push(format!("R² ≈ {}", mpp_core::number::format_f64(r2, 6)));
        }
        Ok((Expr::new(ExprNode::Tuple(values)), parts.join(", ")))
    }
}

/// A unit for each parameter that makes `row` dimensionally consistent: tried from the
/// units of the parts of the model, their reciprocals and their ratios.
fn param_units(row: &Expr, params: &[String]) -> Option<Vec<Unit>> {
    let mut found: Vec<Unit> = vec![Unit::one()];
    collect(row, &mut found);
    let base = found.clone();
    for a in &base {
        for b in &base {
            if let Ok(q) = a.div(b) {
                let q = mpp_core::units::Quantity::new(Expr::one(), q)
                    .simplified()
                    .unit;
                if !found.contains(&q) {
                    found.push(q);
                }
            }
        }
    }
    found.truncate(MAX_CANDIDATES * 4);
    let k = params.len();
    let n = found.len();
    let total = n.checked_pow(u32::try_from(k).ok()?)?;
    if total > 50_000 {
        return None;
    }
    for code in 0..total {
        let mut c = code;
        let pick: Vec<Unit> = (0..k)
            .map(|_| {
                let u = found[c % n].clone();
                c /= n;
                u
            })
            .collect();
        let mut t = row.clone();
        for (p, u) in params.iter().zip(&pick) {
            t = t.subs(p, &units_wire::factor(u));
        }
        if matches!(
            units_wire::fold(&t),
            Ok(Folded::Known { certain: true, .. })
        ) {
            return Some(pick);
        }
    }
    None
}

fn collect(e: &Expr, out: &mut Vec<Unit>) {
    if let Ok(Folded::Known { q, .. }) = units_wire::fold(e) {
        if !q.unit.is_one() {
            let u = q.simplified().unit;
            if !out.contains(&u) {
                out.push(u.clone());
            }
            if let Ok(r) = Unit::one().div(&u) {
                if !out.contains(&r) {
                    out.push(r);
                }
            }
        }
    }
    for c in e.children() {
        collect(c, out);
    }
}

/// Gauss–Newton with Levenberg–Marquardt damping on the residuals; returns the
/// parameters and R² (of the left side's values against the fitted ones).
fn least_squares(
    residuals: &[Expr],
    params: &[String],
    mut x: Vec<f64>,
    lhs: &Expr,
    columns: &[(String, Vec<Expr>)],
) -> Result<(Vec<f64>, Option<f64>), EvalError> {
    let eval = |x: &[f64]| -> Option<Vec<f64>> {
        let env: HashMap<String, f64> = params.iter().cloned().zip(x.iter().copied()).collect();
        residuals
            .iter()
            .map(|r| r.eval_f64(&env).ok().filter(|v| v.is_finite()))
            .collect()
    };
    let cost = |r: &[f64]| r.iter().map(|v| v * v).sum::<f64>();
    let mut r = eval(&x).ok_or_else(|| {
        EvalError::Invalid("the model cannot be evaluated at the start of the fit".into())
    })?;
    let mut lambda = 1e-3;
    let k = params.len();
    for _ in 0..MAX_STEPS {
        // Numerical Jacobian.
        let mut jac = vec![vec![0.0; k]; r.len()];
        for j in 0..k {
            let h = 1e-7 * x[j].abs().max(1e-7);
            let mut xp = x.clone();
            xp[j] += h;
            let Some(rp) = eval(&xp) else {
                return Err(EvalError::Invalid(
                    "the model cannot be evaluated near the fit".into(),
                ));
            };
            for (row, (a, b)) in jac.iter_mut().zip(rp.iter().zip(&r)) {
                row[j] = (a - b) / h;
            }
        }
        // (JᵀJ + λ diag) δ = −Jᵀr
        let mut a = vec![vec![0.0; k]; k];
        let mut g = vec![0.0; k];
        for (row, ri) in jac.iter().zip(&r) {
            for p in 0..k {
                g[p] -= row[p] * ri;
                for q in 0..k {
                    a[p][q] += row[p] * row[q];
                }
            }
        }
        let mut improved = false;
        for _ in 0..20 {
            let mut m = a.clone();
            for (p, row) in m.iter_mut().enumerate() {
                row[p] += lambda * a[p][p].max(1e-30);
            }
            let Some(delta) = solve_small(m, g.clone()) else {
                lambda *= 10.0;
                continue;
            };
            let xn: Vec<f64> = x.iter().zip(&delta).map(|(a, d)| a + d).collect();
            if let Some(rn) = eval(&xn) {
                if cost(&rn) <= cost(&r) {
                    let step = delta
                        .iter()
                        .zip(&x)
                        .map(|(d, v)| d.abs() / v.abs().max(1e-12))
                        .fold(0.0, f64::max);
                    x = xn;
                    r = rn;
                    lambda = (lambda / 10.0).max(1e-12);
                    improved = true;
                    if step < 1e-12 {
                        return Ok((x.clone(), r_squared(&r, lhs, columns)));
                    }
                    break;
                }
            }
            lambda *= 10.0;
        }
        if !improved {
            break;
        }
    }
    Ok((x, r_squared(&r, lhs, columns)))
}

/// 1 − SS_res/SS_tot, with the left side's values (in SI units) as the data.
fn r_squared(r: &[f64], lhs: &Expr, columns: &[(String, Vec<Expr>)]) -> Option<f64> {
    let rows = r.len();
    let mut ys = Vec::with_capacity(rows);
    for i in 0..rows {
        let mut e = lhs.clone();
        for (n, vals) in columns {
            e = e.subs(n, &vals[i]);
        }
        let v = match units_wire::fold(&e).ok()? {
            Folded::Known { q, .. } => (q.value * q.unit.scale())
                .eval_f64(&mpp_core::EmptyEnv)
                .ok()?,
            Folded::Unknown => return None,
        };
        ys.push(v);
    }
    let mean = ys.iter().sum::<f64>() / rows as f64;
    let ss_tot: f64 = ys.iter().map(|y| (y - mean).powi(2)).sum();
    let ss_res: f64 = r.iter().map(|v| v * v).sum();
    (ss_tot > 0.0).then(|| 1.0 - ss_res / ss_tot)
}

/// Solves a small dense system by Gaussian elimination with partial pivoting.
fn solve_small(mut a: Vec<Vec<f64>>, mut b: Vec<f64>) -> Option<Vec<f64>> {
    let n = b.len();
    for col in 0..n {
        let pivot = (col..n).max_by(|&i, &j| a[i][col].abs().total_cmp(&a[j][col].abs()))?;
        if a[pivot][col].abs() < 1e-300 {
            return None;
        }
        a.swap(col, pivot);
        b.swap(col, pivot);
        for row in col + 1..n {
            let f = a[row][col] / a[col][col];
            for k in col..n {
                a[row][k] -= f * a[col][k];
            }
            b[row] -= f * b[col];
        }
    }
    let mut x = vec![0.0; n];
    for row in (0..n).rev() {
        let s: f64 = (row + 1..n).map(|k| a[row][k] * x[k]).sum();
        x[row] = (b[row] - s) / a[row][row];
    }
    x.iter().all(|v| v.is_finite()).then_some(x)
}
