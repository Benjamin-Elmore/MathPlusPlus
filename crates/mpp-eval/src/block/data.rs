//! Data tables in documents: a `.mpp` table whose header names variables, a CSV file
//! read with `Data: $T$ from [[file.csv]].`, and functions defined as regression fits
//! (`f(x) := \operatorname{linreg}(t, d)`).

use std::path::Path;
use std::sync::Arc;

use mpp_core::domains::regression::{self, FitKind};
use mpp_core::{Expr, ExprNode};
use mpp_syntax::document::{Cell, DataImport, InlineKind, Table};
use mpp_syntax::math::Statement;
use mpp_syntax::Span;

use super::{Locals, Run};

/// Longest data column shown entry by entry; longer ones are summarised.
const SHOWN_ENTRIES: usize = 200;
use crate::data::{self, Column};
use crate::error::{EvalError, Located};
use crate::names::node_key;
use crate::result::{DefinitionKind, ImportInfo, LineKind, LineResult};
use crate::value::{Binding, Value, VarKind};

/// Regression operators that can define a function.
const FITS: &[&str] = &["linreg", "polyreg", "expreg", "powreg", "logreg"];

impl Run<'_, '_> {
    /// A table whose header row names variables and whose cells are numbers defines one
    /// data column per header. Returns false for any other table (it stays prose).
    pub(super) fn data_table(&mut self, t: &Table) -> bool {
        let Some(heads) = t
            .header
            .iter()
            .map(|c| header_of(c, self.inputs.source))
            .collect::<Option<Vec<(String, Option<String>, Span)>>>()
        else {
            return false;
        };
        if heads.is_empty() || t.rows.is_empty() {
            return false;
        }
        let mut columns: Vec<Column> = heads
            .iter()
            .map(|(name, unit, _)| Column {
                name: name.clone(),
                unit: unit.clone(),
                values: Vec::new(),
            })
            .collect();
        for row in &t.rows {
            if row.len() != columns.len() {
                return false;
            }
            for (cell, column) in row.iter().zip(columns.iter_mut()) {
                match self.cell_value(cell) {
                    Some(v) => column.values.push(v),
                    None => return false,
                }
            }
        }
        for ((_, unit, span), column) in heads.iter().zip(&columns) {
            if let Some(u) = unit {
                if let Err(e) = mpp_core::units::parse_unit(u) {
                    self.error(*span, EvalError::Math(e));
                    return true;
                }
            }
            self.define_column(column, column.to_expr(), *span);
        }
        true
    }

    /// A number in a table cell: plain text, or maths with a constant value.
    fn cell_value(&self, cell: &Cell) -> Option<Expr> {
        let parts: Vec<&InlineKind> = cell
            .iter()
            .map(|i| &i.kind)
            .filter(|k| !matches!(k, InlineKind::Text(t) if t.trim().is_empty()))
            .collect();
        match parts.as_slice() {
            [InlineKind::Text(t)] => data::parse_cell(t),
            [InlineKind::Math(m)] => {
                let [line] = m.ast.lines.as_slice() else {
                    return None;
                };
                let Statement::Relation(node) = &line.statement else {
                    return None;
                };
                let e = self.lower(node, &[]).ok()?.simplify();
                (e.is_constant() && e.as_number().is_some()).then_some(e)
            }
            _ => None,
        }
    }

    fn define_column(&mut self, column: &Column, value: mpp_core::MathResult<Expr>, span: Span) {
        if self.blocked(&column.name, span) {
            return;
        }
        let value = match value {
            Ok(v) => v,
            Err(e) => return self.error(span, EvalError::Math(e)),
        };
        self.define(
            &column.name,
            Binding::Variable {
                kind: VarKind::Variable,
                value: value.clone(),
                origin: self.origin(span),
            },
        );
        let mut lr = LineResult::new(
            span,
            LineKind::Definition {
                name: column.name.clone(),
                kind: DefinitionKind::Variable,
            },
        );
        lr.input = Some(value.clone());
        if column.values.len() > SHOWN_ENTRIES {
            // A long column is summarised: printing 100 000 entries in three forms on
            // every edit used to take seconds.
            let n = column.values.len();
            let unit = column
                .unit
                .as_deref()
                .map_or(String::new(), |u| format!(" in {u}"));
            let summary = format!("{n} values{unit}");
            lr.latex = Some(format!("\\text{{{summary}}}"));
            lr.text = Some(summary.clone());
            lr.plain = Some(summary);
            lr.value = Some(Value::from_expr(value));
            // A presentation of its own: number formats leave it as it is.
            lr.number_format = Some(crate::numfmt::Shown {
                format: mpp_core::display::NumberFormat::default(),
                derived: false,
                angle: false,
            });
        } else if !self.present_units(&mut lr, &value, mpp_syntax::math::QueryMode::Exact, span) {
            lr.set_value(Value::from_expr(value));
        }
        self.push(lr);
    }

    /// `Data: $T$ from [[measurements.csv]].`: reads the file (relative to this
    /// document), defines its columns and the table `T` as a matrix. The file is
    /// listed with the block's imports, so it is watched and read again when it changes.
    pub(super) fn data_import(&mut self, d: &DataImport) {
        self.volatile = true;
        let Some(table_name) = node_key(&d.name) else {
            return self.error(
                d.name.span,
                EvalError::Invalid("expected a name for the table".into()),
            );
        };
        let mut info = ImportInfo {
            path: d.path.clone(),
            span: d.path_span,
            names: None,
            loaded: false,
            file: None,
            files: Vec::new(),
            diagnostics: Vec::new(),
            uncacheable: true,
        };
        let read = self
            .inputs
            .importer
            .read_data(self.inputs.base_dir, &d.path);
        let (path, text) = match read {
            Ok(found) => found,
            Err(msg) => {
                self.result.imports.push(info);
                return self.error(d.path_span, EvalError::Invalid(msg));
            }
        };
        info.loaded = true;
        info.file = Some(Arc::from(path.as_path()));
        info.files = vec![path.clone()];
        self.result.imports.push(info);
        let loaded = match data::load_csv(&text) {
            Ok(t) => t,
            Err(msg) => {
                let file = Path::new(&d.path)
                    .file_name()
                    .map_or(d.path.clone(), |f| f.to_string_lossy().into_owned());
                return self.error(d.path_span, EvalError::Invalid(format!("{file}: {msg}")));
            }
        };
        for (column, value) in loaded.table.columns.iter().zip(&loaded.values) {
            self.define_column(column, value.clone(), d.path_span);
        }
        if let Some(m) = loaded.matrix.clone() {
            if !self.blocked(&table_name, d.name.span) {
                self.define(
                    &table_name,
                    Binding::Variable {
                        kind: VarKind::Variable,
                        value: m,
                        origin: self.origin(d.name.span),
                    },
                );
            }
        }
    }

    /// `f(x) := \operatorname{linreg}(t, d)`: the fitted model as the body of `f`, in
    /// its own parameter, with R² noted. `None` when the body is not a fit.
    pub(super) fn fitted_body(
        &mut self,
        body: &Expr,
        params: &[String],
        span: Span,
    ) -> Option<Expr> {
        let ExprNode::Apply(op, args) = body.node() else {
            return None;
        };
        if !FITS.contains(&&**op) {
            return None;
        }
        let [param] = params else {
            self.error(
                span,
                EvalError::Invalid("a fitted function takes one variable".into()),
            );
            return Some(body.clone());
        };
        let resolve = |run: &Run<'_, '_>, e: &Expr| {
            run.resolve_full(e, &Locals::new(), &[], false, false).value
        };
        let values: Result<Vec<Expr>, EvalError> = args.iter().map(|a| resolve(self, a)).collect();
        let values = match values {
            Ok(v) => v,
            Err(err) => {
                self.located(Located::new(span, err));
                return Some(body.clone());
            }
        };
        // Data with units are fitted on their plain values.
        let mut values_plain = Vec::with_capacity(values.len());
        for v in values {
            match crate::units_wire::strip_list(&v) {
                Ok(Some((items, _))) => values_plain.push(Expr::new(ExprNode::Tuple(items))),
                Ok(None) => values_plain.push(v),
                Err(e) => {
                    self.error(span, EvalError::Math(e));
                    return Some(body.clone());
                }
            }
        }
        let values = values_plain;
        match fit(op, &values, param) {
            Ok(fit) => {
                if let Some(r2) = &fit.r_squared {
                    let shown = if crate::numeric::is_exact_rational(r2) {
                        r2.to_string()
                    } else {
                        crate::numeric::approximate(r2).unwrap_or_else(|_| r2.to_string())
                    };
                    self.warn(span, format!("{} fit, R² = {shown}", kind_name(&fit.kind)));
                }
                Some(fit.model)
            }
            Err(e) => {
                self.error(span, EvalError::Math(e));
                Some(body.clone())
            }
        }
    }
}

fn kind_name(k: &FitKind) -> String {
    match k {
        FitKind::Linear => "linear".into(),
        FitKind::Polynomial(d) => format!("degree-{d} polynomial"),
        FitKind::Exponential => "exponential".into(),
        FitKind::Power => "power".into(),
        FitKind::Logarithmic => "logarithmic".into(),
    }
}

/// The fit `op` of `(xs, ys[, degree])` in the variable `var`.
fn fit(op: &str, args: &[Expr], var: &str) -> mpp_core::MathResult<regression::Fit> {
    let list = |e: &Expr| -> mpp_core::MathResult<Vec<Expr>> {
        match e.node() {
            ExprNode::Tuple(items) => Ok(items.clone()),
            _ => mpp_core::domains::util::items(op, std::slice::from_ref(e)),
        }
    };
    let (xs, ys) = match args {
        [x, y, ..] => (list(x)?, list(y)?),
        _ => {
            return Err(mpp_core::MathError::WrongArity {
                function: op.into(),
                expected: "2".into(),
                got: args.len(),
            })
        }
    };
    match op {
        "linreg" => regression::poly_fit(&xs, &ys, 1, var),
        "polyreg" => {
            let d = args
                .get(2)
                .and_then(mpp_core::domains::util::integer)
                .and_then(|d| usize::try_from(d).ok())
                .ok_or_else(|| mpp_core::MathError::Domain {
                    function: op.into(),
                    detail: "give the degree as a third argument, such as polyreg(x, y, 2)".into(),
                })?;
            regression::poly_fit(&xs, &ys, d, var)
        }
        "expreg" => regression::transformed_fit(&xs, &ys, FitKind::Exponential, var),
        "powreg" => regression::transformed_fit(&xs, &ys, FitKind::Power, var),
        _ => regression::transformed_fit(&xs, &ys, FitKind::Logarithmic, var),
    }
}

/// A header cell `$t$`, `$t$ (s)` or `$x$ [m]`: the name key, the unit and the span.
/// `\mathrm{kPa}`, `\mathrm L`, `\mathrm{s^{-1}}`, `\mathrm{mol/L}` as plain unit text.
fn plain_unit(tex: &str) -> String {
    let mut s = tex
        .replace("\\mathrm", "")
        .replace("\\text", "")
        .replace("\\Omega", "Ω")
        .replace("\\mu", "µ")
        .replace("\\,", " ")
        .replace("\\ ", " ");
    s.retain(|c| c != '{' && c != '}');
    s.trim().to_string()
}

fn header_of(cell: &Cell, src: &str) -> Option<(String, Option<String>, Span)> {
    let mut name = None;
    let mut rest = String::new();
    let mut span = None;
    for inline in cell {
        match &inline.kind {
            InlineKind::Math(m) if name.is_none() => {
                let [line] = m.ast.lines.as_slice() else {
                    return None;
                };
                let Statement::Relation(node) = &line.statement else {
                    return None;
                };
                name = Some(node_key(node)?);
                span = Some(inline.span);
            }
            InlineKind::Text(t) if name.is_some() => rest.push_str(t),
            // `$V$ ($\mathrm L$)`: the unit written as maths.
            InlineKind::Math(m) if name.is_some() => {
                let tex = src.get(m.raw.start..m.raw.end)?;
                rest.push_str(&plain_unit(tex));
            }
            InlineKind::Text(t) if t.trim().is_empty() => {}
            _ => return None,
        }
    }
    let name = name?;
    let rest = rest.trim();
    let unit = if rest.is_empty() {
        None
    } else {
        let (_, unit) = data::split_header(&format!("x {rest}"));
        Some(unit?)
    };
    Some((name, unit, span?))
}
