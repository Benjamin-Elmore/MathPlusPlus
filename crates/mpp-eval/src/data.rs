//! Data tables: columns of measurements from a `.mpp` table or a CSV file, the
//! statistics on them, and the figure data (scatter, fitted curve, histogram, box plot)
//! the plot crate draws.
//!
//! A table whose header row names variables (`| $t$ (s) | $x$ (m) |`) defines each
//! column as a vector of numbers, with the unit in brackets or parentheses after the
//! name; `Data: $T$ from [[measurements.csv]].` does the same for a CSV file (and names
//! the whole table `T`, as a matrix). Columns are ordinary values: `\bar{x}` is the
//! mean, `x_i` an entry, `\sum_i x_i` runs over every entry, and every statistics,
//! regression and distribution operator takes them.
//!
//! The figure structs at the end are the contract with `mpp-plot` (see
//! `work/lanes/N5.mpp`): plain numbers, ready to draw.

use mpp_core::domains::regression::{self, Fit};
use mpp_core::{Expr, ExprNode, MathError, MathResult, Number};

use crate::units_wire;

/// One column of a table.
#[derive(Debug, Clone, PartialEq)]
pub struct Column {
    /// The variable it defines (a name key such as `x`, `v_0`, `θ`).
    pub name: String,
    /// The unit written in the header, if any (`s`, `m/s`).
    pub unit: Option<String>,
    /// The entries, exact where the text was an exact decimal.
    pub values: Vec<Expr>,
}

impl Column {
    /// The column as a value: a tuple of numbers, or of quantities with the unit.
    pub fn to_expr(&self) -> MathResult<Expr> {
        let items = match &self.unit {
            None => self.values.clone(),
            Some(u) => self
                .values
                .iter()
                .map(|v| units_wire::quantity_expr(v.clone(), u))
                .collect::<MathResult<Vec<_>>>()?,
        };
        Ok(Expr::new(ExprNode::Tuple(items)))
    }
}

/// A table of columns of equal length.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct DataTable {
    pub columns: Vec<Column>,
}

impl DataTable {
    /// Number of rows.
    pub fn rows(&self) -> usize {
        self.columns.first().map_or(0, |c| c.values.len())
    }

    /// The whole table as a matrix (rows of entries, without units).
    pub fn to_matrix(&self) -> Option<Expr> {
        let rows: Vec<Vec<Expr>> = (0..self.rows())
            .map(|r| self.columns.iter().map(|c| c.values[r].clone()).collect())
            .collect();
        Expr::matrix(rows)
    }
}

/// Most rows read from one table.
pub const MAX_ROWS: usize = 100_000;

/// Splits a header cell such as `t (s)`, `x [m]` or `v_0` into the name text and unit.
pub fn split_header(cell: &str) -> (String, Option<String>) {
    let cell = cell.trim();
    for (open, close) in [('(', ')'), ('[', ']')] {
        if let (Some(a), true) = (cell.find(open), cell.ends_with(close)) {
            let name = cell[..a].trim().to_string();
            let unit = cell[a + 1..cell.len() - 1].trim().to_string();
            if !name.is_empty() && !unit.is_empty() {
                return (name, Some(unit));
            }
        }
    }
    (cell.to_string(), None)
}

/// A number written in a table cell (`1.5`, `-2`, `3e-4`, `1/3`).
pub fn parse_cell(text: &str) -> Option<Expr> {
    let t = text.trim().trim_start_matches('+');
    if let Some((a, b)) = t.split_once('/') {
        let num = Number::parse_decimal(a.trim())?;
        let den = Number::parse_decimal(b.trim())?;
        let q = (Expr::num(num) / Expr::num(den)).simplify();
        return q.is_number().then_some(q);
    }
    let t = t.replace('−', "-");
    Number::parse_decimal(&t).map(Expr::num)
}

/// The name key of a header such as `t`, `v_0`, `theta`, `\theta`, `x_{max}`.
pub fn header_key(text: &str) -> Option<String> {
    let src = text.trim().trim_matches('$').trim();
    if src.is_empty() {
        return None;
    }
    let (node, diags) = mpp_syntax::math::parse_expr(src, 0);
    if !diags.is_empty() {
        return None;
    }
    crate::names::node_key(&node)
}

/// Reads a CSV file: a header row of names (with optional units) and rows of numbers.
/// Commas, semicolons or tabs separate the fields; blank lines are skipped.
pub fn parse_csv(text: &str) -> Result<DataTable, String> {
    let mut lines = text
        .lines()
        .enumerate()
        .map(|(i, l)| (i + 1, l.trim_end_matches('\r')))
        .filter(|(_, l)| !l.trim().is_empty());
    let Some((_, header)) = lines.next() else {
        return Err("the file is empty".into());
    };
    let sep = [',', ';', '\t']
        .into_iter()
        .max_by_key(|c| header.matches(*c).count())
        .unwrap_or(',');
    let names = split_fields(header, sep);
    let mut columns = Vec::with_capacity(names.len());
    for (i, h) in names.iter().enumerate() {
        let (name_text, unit) = split_header(h);
        let name = header_key(&name_text).ok_or_else(|| {
            format!(
                "column {} of the header, `{h}`, is not a name such as x, v_0 or theta",
                i + 1
            )
        })?;
        if let Some(u) = &unit {
            mpp_core::units::parse_unit(u)
                .map_err(|e| format!("the unit of column `{name}`: {e}"))?;
        }
        if columns.iter().any(|c: &Column| c.name == name) {
            return Err(format!("two columns are named `{name}`"));
        }
        columns.push(Column {
            name,
            unit,
            values: Vec::new(),
        });
    }
    for (line_no, line) in lines {
        let fields = split_fields(line, sep);
        if fields.len() != columns.len() {
            return Err(format!(
                "line {line_no} has {} values, but the header names {} columns",
                fields.len(),
                columns.len()
            ));
        }
        for (c, f) in columns.iter_mut().zip(&fields) {
            let v = parse_cell(f)
                .ok_or_else(|| format!("line {line_no}: `{}` is not a number", f.trim()))?;
            c.values.push(v);
        }
        if columns.first().is_some_and(|c| c.values.len() > MAX_ROWS) {
            return Err(format!("the file has more than {MAX_ROWS} rows"));
        }
    }
    if columns.first().is_none_or(|c| c.values.is_empty()) {
        return Err("the file has a header but no rows".into());
    }
    Ok(DataTable { columns })
}

/// Fields of one CSV line; a field in double quotes may hold the separator.
fn split_fields(line: &str, sep: char) -> Vec<String> {
    let mut out = Vec::new();
    let mut field = String::new();
    let mut quoted = false;
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '"' if quoted && chars.peek() == Some(&'"') => {
                field.push('"');
                chars.next();
            }
            '"' => quoted = !quoted,
            c if c == sep && !quoted => out.push(std::mem::take(&mut field)),
            c => field.push(c),
        }
    }
    out.push(field);
    out.into_iter().map(|f| f.trim().to_string()).collect()
}

// ---- statistics on columns -----------------------------------------------------------

/// The plain values of a column given as a value (a tuple of numbers or quantities):
/// the numbers in one unit, and that unit.
pub fn plain_values(e: &Expr) -> MathResult<(Vec<f64>, Option<String>)> {
    let ExprNode::Tuple(items) = e.node() else {
        return Err(MathError::NotANumber(format!("{e} as a list of data")));
    };
    let mut unit: Option<mpp_core::Unit> = None;
    let mut out = Vec::with_capacity(items.len());
    for item in items {
        let q = match units_wire::fold(item)? {
            units_wire::Folded::Known { q, .. } => q,
            units_wire::Folded::Unknown => {
                return Err(MathError::NotANumber(format!("{item} as a data value")))
            }
        };
        let q = match &unit {
            None => {
                unit = Some(q.unit.clone());
                q
            }
            Some(u) => q.convert_to(u)?,
        };
        let v = mpp_core::domains::util::numeric(&q.value)
            .ok_or_else(|| MathError::NotANumber(format!("{} as a data value", q.value)))?;
        out.push(v);
    }
    Ok((out, unit.filter(|u| !u.is_one()).map(|u| u.text())))
}

// ---- figure data (the contract with mpp-plot) -------------------------------------------

/// Points to draw as dots.
#[derive(Debug, Clone, PartialEq)]
pub struct Scatter {
    pub points: Vec<(f64, f64)>,
    pub x_unit: Option<String>,
    pub y_unit: Option<String>,
}

/// A fitted curve sampled over the data's range, with its formula and R².
#[derive(Debug, Clone, PartialEq)]
pub struct FittedCurve {
    pub points: Vec<(f64, f64)>,
    pub formula: Expr,
    pub r_squared: Option<f64>,
}

/// Bars of a histogram: `[lo, hi)` (the last bar closed) and the count.
#[derive(Debug, Clone, PartialEq)]
pub struct Histogram {
    pub bins: Vec<(f64, f64, usize)>,
    pub unit: Option<String>,
}

/// A box plot: whiskers at the furthest values within 1.5 IQR of the box, and the
/// values beyond them as outliers. Quartiles by the median-of-halves method.
#[derive(Debug, Clone, PartialEq)]
pub struct BoxPlot {
    pub low: f64,
    pub q1: f64,
    pub median: f64,
    pub q3: f64,
    pub high: f64,
    pub outliers: Vec<f64>,
    pub unit: Option<String>,
}

/// The dots of `ys` against `xs`.
pub fn scatter(xs: &Expr, ys: &Expr) -> MathResult<Scatter> {
    let (x, xu) = plain_values(xs)?;
    let (y, yu) = plain_values(ys)?;
    if x.len() != y.len() {
        return Err(MathError::Domain {
            function: "a scatter plot".into(),
            detail: format!("{} x values but {} y values", x.len(), y.len()),
        });
    }
    Ok(Scatter {
        points: x.into_iter().zip(y).collect(),
        x_unit: xu,
        y_unit: yu,
    })
}

/// `samples` points of the fit over `[lo, hi]`.
pub fn fitted_curve(fit: &Fit, var: &str, lo: f64, hi: f64, samples: usize) -> FittedCurve {
    let n = samples.max(2);
    let mut points = Vec::with_capacity(n);
    for i in 0..n {
        let x = lo + (hi - lo) * i as f64 / (n - 1) as f64;
        let y = fit
            .model
            .subs(var, &Expr::float(x))
            .eval_f64(&mpp_core::EmptyEnv)
            .ok()
            .filter(|v| v.is_finite());
        if let Some(y) = y {
            points.push((x, y));
        }
    }
    FittedCurve {
        points,
        formula: fit.model.clone(),
        r_squared: fit
            .r_squared
            .as_ref()
            .and_then(mpp_core::domains::util::numeric),
    }
}

/// A histogram with `bins` bars (Sturges' rule, ⌈log₂ n⌉ + 1, when `None`).
pub fn histogram(data: &Expr, bins: Option<usize>) -> MathResult<Histogram> {
    let (v, unit) = plain_values(data)?;
    if v.is_empty() {
        return Err(MathError::Undefined("a histogram of no data".into()));
    }
    let k = bins
        .unwrap_or_else(|| (v.len() as f64).log2().ceil() as usize + 1)
        .clamp(1, 1000);
    let lo = v.iter().copied().fold(f64::INFINITY, f64::min);
    let hi = v.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    let width = if hi > lo { (hi - lo) / k as f64 } else { 1.0 };
    let mut counts = vec![0usize; k];
    for x in &v {
        let i = (((x - lo) / width).floor() as usize).min(k - 1);
        counts[i] += 1;
    }
    Ok(Histogram {
        bins: counts
            .into_iter()
            .enumerate()
            .map(|(i, c)| (lo + width * i as f64, lo + width * (i + 1) as f64, c))
            .collect(),
        unit,
    })
}

/// The five-number summary with 1.5 IQR whiskers.
pub fn box_plot(data: &Expr) -> MathResult<BoxPlot> {
    let (v, unit) = plain_values(data)?;
    let exprs: Vec<Expr> = v.iter().map(|&x| Expr::float(x)).collect();
    let (q1, q2, q3) = regression::quartiles(&exprs)?;
    let num = |e: &Expr| {
        mpp_core::domains::util::numeric(e)
            .ok_or_else(|| MathError::NotANumber(format!("{e} as a quartile")))
    };
    let (q1, median, q3) = (num(&q1)?, num(&q2)?, num(&q3)?);
    let iqr = q3 - q1;
    let (lo_fence, hi_fence) = (q1 - 1.5 * iqr, q3 + 1.5 * iqr);
    let inside: Vec<f64> = v
        .iter()
        .copied()
        .filter(|x| (lo_fence..=hi_fence).contains(x))
        .collect();
    let outliers = v
        .iter()
        .copied()
        .filter(|x| !(lo_fence..=hi_fence).contains(x))
        .collect();
    Ok(BoxPlot {
        low: inside.iter().copied().fold(f64::INFINITY, f64::min),
        q1,
        median,
        q3,
        high: inside.iter().copied().fold(f64::NEG_INFINITY, f64::max),
        outliers,
        unit,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn csv_files() {
        let t = parse_csv("t (s), x [m]\n0, 0\n1, 4.9\n2, 19.6\n").unwrap();
        assert_eq!(t.rows(), 3);
        assert_eq!(t.columns[0].name, "t");
        assert_eq!(t.columns[0].unit.as_deref(), Some("s"));
        assert_eq!(t.columns[1].values[1], Expr::rational(49, 10));
        let t = parse_csv("theta;v_0\n1;2\n").unwrap();
        assert_eq!(t.columns[0].name, "θ");
        assert_eq!(t.columns[1].name, "v_0");
        assert!(parse_csv("x,y\n1,2\n3\n").unwrap_err().contains("line 3"));
        assert!(parse_csv("x,y\n1,abc\n")
            .unwrap_err()
            .contains("not a number"));
        assert!(parse_csv("x,x\n1,2\n").is_err());
        assert!(parse_csv("1+,y\n1,2\n").is_err());
        assert!(parse_csv("").is_err());
        assert_eq!(split_fields("\"a,b\",c", ','), vec!["a,b", "c"]);
    }

    #[test]
    fn figures() {
        let xs = Expr::new(ExprNode::Tuple((1..=8).map(Expr::int).collect()));
        let h = histogram(&xs, Some(4)).unwrap();
        assert_eq!(h.bins.iter().map(|b| b.2).sum::<usize>(), 8);
        let b = box_plot(&Expr::new(ExprNode::Tuple(
            [1, 2, 3, 4, 5, 6, 7, 100]
                .iter()
                .map(|&k| Expr::int(k))
                .collect(),
        )))
        .unwrap();
        assert_eq!(b.outliers, vec![100.0]);
        assert_eq!(b.median, 4.5);
        let s = scatter(&xs, &xs).unwrap();
        assert_eq!(s.points.len(), 8);
        let fit = regression::poly_fit(
            &(1..=4).map(Expr::int).collect::<Vec<_>>(),
            &[3, 5, 7, 9]
                .iter()
                .map(|&k| Expr::int(k))
                .collect::<Vec<_>>(),
            1,
            "x",
        )
        .unwrap();
        let c = fitted_curve(&fit, "x", 0.0, 1.0, 3);
        assert_eq!(c.points, vec![(0.0, 1.0), (0.5, 2.0), (1.0, 3.0)]);
        assert_eq!(c.r_squared, Some(1.0));
    }
}
