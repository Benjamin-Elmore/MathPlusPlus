//! The expression tree: an immutable, shareable `Arc<ExprNode>`.

use std::collections::BTreeSet;
use std::fmt;
use std::sync::Arc;

use num_bigint::BigInt;
use num_rational::BigRational;

use crate::functions::Func;
use crate::number::Number;

/// A symbol or function name.
pub type Name = Arc<str>;

/// The name of the exact Euler–Mascheroni constant (see [`Expr::euler_gamma`]).
pub const EULER_GAMMA: &str = "eulergamma";

/// Named constants.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Constant {
    /// π.
    Pi,
    /// Euler's number e.
    E,
    /// The imaginary unit (simplification turns it into the exact number `i`).
    I,
    /// Positive infinity; −∞ is `−1 · ∞`.
    Infinity,
    /// A value that does not exist (`∞ − ∞`, `0/0` …).
    Undefined,
}

/// Relation operators.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum RelOp {
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    Approx,
}

/// Logical connectives.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum LogicOp {
    And,
    Or,
    Not,
}

/// Direction of a limit.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum LimitDir {
    /// Two-sided.
    Both,
    /// From below (`x → a⁻`).
    Left,
    /// From above (`x → a⁺`).
    Right,
}

/// Kind of a big operator.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum BigOpKind {
    Sum,
    Product,
}

/// One node of the tree. Variant order defines the canonical ordering.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ExprNode {
    /// A number.
    Num(Number),
    /// A free symbol.
    Sym(Name),
    /// A named constant.
    Const(Constant),
    /// A sum (canonical: flattened, like terms collected, at least two terms).
    Add(Vec<Expr>),
    /// A product (canonical: numeric coefficient first, powers combined).
    Mul(Vec<Expr>),
    /// `base ^ exponent`; roots are fractional powers.
    Pow(Expr, Expr),
    /// A built-in elementary function.
    Func(Func, Vec<Expr>),
    /// A user function or an operator the core does not know.
    Apply(Name, Vec<Expr>),
    /// `lhs op rhs`.
    Relation(RelOp, Expr, Expr),
    /// A truth value.
    Bool(bool),
    /// A logical combination.
    Logic(LogicOp, Vec<Expr>),
    /// Unevaluated derivative `∂ⁿ/∂x… expr`.
    Derivative { expr: Expr, vars: Vec<(Name, u32)> },
    /// Unevaluated integral, definite when `bounds` is set.
    Integral {
        expr: Expr,
        var: Name,
        bounds: Option<(Expr, Expr)>,
    },
    /// Unevaluated sum or product over `var = lower … upper`.
    BigOp {
        kind: BigOpKind,
        expr: Expr,
        var: Name,
        lower: Expr,
        upper: Expr,
    },
    /// Unevaluated limit.
    Limit {
        expr: Expr,
        var: Name,
        point: Expr,
        dir: LimitDir,
    },
    /// A matrix stored row by row.
    Matrix {
        rows: usize,
        cols: usize,
        data: Vec<Expr>,
    },
    /// `(value, condition)` pairs tried in order, then `otherwise`.
    Piecewise {
        cases: Vec<(Expr, Expr)>,
        otherwise: Option<Expr>,
    },
    /// A finite set.
    Set(Vec<Expr>),
    /// An interval of the real line.
    Interval {
        lo: Expr,
        hi: Expr,
        lo_closed: bool,
        hi_closed: bool,
    },
    /// An ordered tuple (also used for vectors and points).
    Tuple(Vec<Expr>),
}

/// An immutable expression; cheap to clone and safe to share between threads.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Expr(Arc<ExprNode>);

impl fmt::Debug for Expr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self, f)
    }
}

impl Expr {
    /// Wraps a node as is (no simplification).
    pub fn new(node: ExprNode) -> Expr {
        Expr(Arc::new(node))
    }

    /// The node.
    pub fn node(&self) -> &ExprNode {
        &self.0
    }

    /// A number.
    pub fn num(n: Number) -> Expr {
        Expr::new(ExprNode::Num(n))
    }

    /// An exact integer.
    pub fn int(n: i64) -> Expr {
        Expr::num(Number::int(n))
    }

    /// An exact big integer.
    pub fn bigint(n: BigInt) -> Expr {
        Expr::num(Number::from_bigint(n))
    }

    /// An exact rational `p/q` (`q = 0` gives undefined).
    pub fn rational(p: i64, q: i64) -> Expr {
        Number::rational(p, q).map_or_else(|_| Expr::undefined(), Expr::num)
    }

    /// An exact big rational.
    pub fn ratio(q: BigRational) -> Expr {
        Expr::num(Number::Rational(q))
    }

    /// An approximate real.
    pub fn float(x: f64) -> Expr {
        Expr::num(Number::float(x))
    }

    /// A symbol.
    pub fn sym(name: &str) -> Expr {
        Expr::new(ExprNode::Sym(Arc::from(name)))
    }

    /// A constant.
    pub fn constant(c: Constant) -> Expr {
        Expr::new(ExprNode::Const(c))
    }

    /// π.
    pub fn pi() -> Expr {
        Expr::constant(Constant::Pi)
    }

    /// The Euler–Mascheroni constant γ = −ψ(1) = 0.5772…, kept exact as the
    /// zero-argument application [`EULER_GAMMA`] (shown `γ`).
    pub fn euler_gamma() -> Expr {
        Expr::apply(EULER_GAMMA, Vec::new())
    }

    /// e.
    pub fn e() -> Expr {
        Expr::constant(Constant::E)
    }

    /// The imaginary unit as an exact number.
    pub fn i() -> Expr {
        Expr::num(Number::i())
    }

    /// +∞.
    pub fn infinity() -> Expr {
        Expr::constant(Constant::Infinity)
    }

    /// −∞.
    pub fn neg_infinity() -> Expr {
        Expr::new(ExprNode::Mul(vec![Expr::int(-1), Expr::infinity()]))
    }

    /// Undefined.
    pub fn undefined() -> Expr {
        Expr::constant(Constant::Undefined)
    }

    /// 0.
    pub fn zero() -> Expr {
        Expr::int(0)
    }

    /// 1.
    pub fn one() -> Expr {
        Expr::int(1)
    }

    /// A relation `lhs op rhs` (sides kept as given).
    pub fn relation(op: RelOp, lhs: Expr, rhs: Expr) -> Expr {
        Expr::new(ExprNode::Relation(op, lhs, rhs))
    }

    /// An equation `lhs = rhs`.
    pub fn equation(lhs: Expr, rhs: Expr) -> Expr {
        Expr::relation(RelOp::Eq, lhs, rhs)
    }

    /// A user or unknown function application.
    pub fn apply(name: &str, args: Vec<Expr>) -> Expr {
        Expr::new(ExprNode::Apply(Arc::from(name), args))
    }

    /// A matrix from rows; `None` when rows have different lengths or there are none.
    pub fn matrix(rows: Vec<Vec<Expr>>) -> Option<Expr> {
        let cols = rows.first()?.len();
        if cols == 0 || rows.iter().any(|r| r.len() != cols) {
            return None;
        }
        let n = rows.len();
        Some(Expr::new(ExprNode::Matrix {
            rows: n,
            cols,
            data: rows.into_iter().flatten().collect(),
        }))
    }

    /// The number, if this is one.
    pub fn as_number(&self) -> Option<&Number> {
        match self.node() {
            ExprNode::Num(n) => Some(n),
            _ => None,
        }
    }

    /// The symbol name, if this is a symbol.
    pub fn as_sym(&self) -> Option<&str> {
        match self.node() {
            ExprNode::Sym(s) => Some(s),
            _ => None,
        }
    }

    /// True for numbers.
    pub fn is_number(&self) -> bool {
        matches!(self.node(), ExprNode::Num(_))
    }

    /// True for exact or approximate zero.
    pub fn is_zero(&self) -> bool {
        self.as_number().is_some_and(Number::is_zero)
    }

    /// True for one.
    pub fn is_one(&self) -> bool {
        self.as_number().is_some_and(Number::is_one)
    }

    /// True for the symbol `name`.
    pub fn is_sym(&self, name: &str) -> bool {
        self.as_sym() == Some(name)
    }

    /// True for `Undefined`.
    pub fn is_undefined(&self) -> bool {
        matches!(self.node(), ExprNode::Const(Constant::Undefined))
    }

    /// True for +∞ or −∞.
    pub fn is_infinite(&self) -> bool {
        match self.node() {
            ExprNode::Const(Constant::Infinity) => true,
            ExprNode::Mul(fs) => {
                fs.len() == 2
                    && fs[0].as_number().is_some_and(Number::is_real)
                    && matches!(fs[1].node(), ExprNode::Const(Constant::Infinity))
            }
            _ => false,
        }
    }

    /// Direct children, in order.
    pub fn children(&self) -> Vec<&Expr> {
        match self.node() {
            ExprNode::Num(_) | ExprNode::Sym(_) | ExprNode::Const(_) | ExprNode::Bool(_) => {
                Vec::new()
            }
            ExprNode::Add(v)
            | ExprNode::Mul(v)
            | ExprNode::Func(_, v)
            | ExprNode::Apply(_, v)
            | ExprNode::Logic(_, v)
            | ExprNode::Set(v)
            | ExprNode::Tuple(v) => v.iter().collect(),
            ExprNode::Matrix { data, .. } => data.iter().collect(),
            ExprNode::Pow(a, b) | ExprNode::Relation(_, a, b) => vec![a, b],
            ExprNode::Derivative { expr, .. } => vec![expr],
            ExprNode::Integral { expr, bounds, .. } => {
                let mut v = vec![expr];
                if let Some((a, b)) = bounds {
                    v.push(a);
                    v.push(b);
                }
                v
            }
            ExprNode::BigOp {
                expr, lower, upper, ..
            } => vec![expr, lower, upper],
            ExprNode::Limit { expr, point, .. } => vec![expr, point],
            ExprNode::Piecewise { cases, otherwise } => {
                let mut v: Vec<&Expr> = cases.iter().flat_map(|(a, b)| [a, b]).collect();
                if let Some(o) = otherwise {
                    v.push(o);
                }
                v
            }
            ExprNode::Interval { lo, hi, .. } => vec![lo, hi],
        }
    }

    /// Rebuilds this node with `f` applied to every child (no simplification).
    pub fn map_children(&self, f: &mut impl FnMut(&Expr) -> Expr) -> ExprNode {
        let map = |v: &[Expr], f: &mut dyn FnMut(&Expr) -> Expr| v.iter().map(f).collect();
        match self.node() {
            n @ (ExprNode::Num(_) | ExprNode::Sym(_) | ExprNode::Const(_) | ExprNode::Bool(_)) => {
                n.clone()
            }
            ExprNode::Add(v) => ExprNode::Add(map(v, f)),
            ExprNode::Mul(v) => ExprNode::Mul(map(v, f)),
            ExprNode::Pow(a, b) => ExprNode::Pow(f(a), f(b)),
            ExprNode::Func(func, v) => ExprNode::Func(*func, map(v, f)),
            ExprNode::Apply(name, v) => ExprNode::Apply(name.clone(), map(v, f)),
            ExprNode::Relation(op, a, b) => ExprNode::Relation(*op, f(a), f(b)),
            ExprNode::Logic(op, v) => ExprNode::Logic(*op, map(v, f)),
            ExprNode::Derivative { expr, vars } => ExprNode::Derivative {
                expr: f(expr),
                vars: vars.clone(),
            },
            ExprNode::Integral { expr, var, bounds } => ExprNode::Integral {
                expr: f(expr),
                var: var.clone(),
                bounds: bounds.as_ref().map(|(a, b)| (f(a), f(b))),
            },
            ExprNode::BigOp {
                kind,
                expr,
                var,
                lower,
                upper,
            } => ExprNode::BigOp {
                kind: *kind,
                expr: f(expr),
                var: var.clone(),
                lower: f(lower),
                upper: f(upper),
            },
            ExprNode::Limit {
                expr,
                var,
                point,
                dir,
            } => ExprNode::Limit {
                expr: f(expr),
                var: var.clone(),
                point: f(point),
                dir: *dir,
            },
            ExprNode::Matrix { rows, cols, data } => ExprNode::Matrix {
                rows: *rows,
                cols: *cols,
                data: map(data, f),
            },
            ExprNode::Piecewise { cases, otherwise } => ExprNode::Piecewise {
                cases: cases.iter().map(|(a, b)| (f(a), f(b))).collect(),
                otherwise: otherwise.as_ref().map(&mut *f),
            },
            ExprNode::Set(v) => ExprNode::Set(map(v, f)),
            ExprNode::Interval {
                lo,
                hi,
                lo_closed,
                hi_closed,
            } => ExprNode::Interval {
                lo: f(lo),
                hi: f(hi),
                lo_closed: *lo_closed,
                hi_closed: *hi_closed,
            },
            ExprNode::Tuple(v) => ExprNode::Tuple(map(v, f)),
        }
    }

    /// The variable this node binds, if any (sums, limits, definite integrals).
    pub fn bound_var(&self) -> Option<&Name> {
        match self.node() {
            ExprNode::BigOp { var, .. } | ExprNode::Limit { var, .. } => Some(var),
            ExprNode::Integral {
                var,
                bounds: Some(_),
                ..
            } => Some(var),
            _ => None,
        }
    }

    /// Free symbols, sorted by name.
    pub fn free_symbols(&self) -> BTreeSet<Name> {
        let mut out = BTreeSet::new();
        self.collect_free(&mut out);
        out
    }

    fn collect_free(&self, out: &mut BTreeSet<Name>) {
        match self.node() {
            ExprNode::Sym(s) => {
                out.insert(s.clone());
            }
            ExprNode::BigOp {
                expr,
                var,
                lower,
                upper,
                ..
            } => {
                let mut inner = BTreeSet::new();
                expr.collect_free(&mut inner);
                inner.remove(var);
                out.extend(inner);
                lower.collect_free(out);
                upper.collect_free(out);
            }
            ExprNode::Limit {
                expr, var, point, ..
            } => {
                let mut inner = BTreeSet::new();
                expr.collect_free(&mut inner);
                inner.remove(var);
                out.extend(inner);
                point.collect_free(out);
            }
            ExprNode::Integral {
                expr,
                var,
                bounds: Some((a, b)),
            } => {
                let mut inner = BTreeSet::new();
                expr.collect_free(&mut inner);
                inner.remove(var);
                out.extend(inner);
                a.collect_free(out);
                b.collect_free(out);
            }
            _ => {
                for c in self.children() {
                    c.collect_free(out);
                }
            }
        }
    }

    /// True when the symbol `name` occurs free.
    pub fn contains_sym(&self, name: &str) -> bool {
        match self.node() {
            ExprNode::Sym(s) => &**s == name,
            _ if self.bound_var().is_some_and(|v| &**v == name) => {
                // Only the bounds can mention it.
                self.free_symbols().iter().any(|s| &**s == name)
            }
            _ => self.children().iter().any(|c| c.contains_sym(name)),
        }
    }

    /// True when no symbols or user functions occur (the value is a fixed number).
    pub fn is_constant(&self) -> bool {
        match self.node() {
            ExprNode::Num(_) | ExprNode::Const(_) => true,
            ExprNode::Sym(_) | ExprNode::Apply(..) => false,
            ExprNode::Add(v) | ExprNode::Mul(v) | ExprNode::Func(_, v) => {
                v.iter().all(Expr::is_constant)
            }
            ExprNode::Pow(a, b) => a.is_constant() && b.is_constant(),
            _ => false,
        }
    }

    /// Number of nodes (a size measure).
    pub fn size(&self) -> usize {
        1 + self.children().iter().map(|c| c.size()).sum::<usize>()
    }
}

impl From<i64> for Expr {
    fn from(n: i64) -> Self {
        Expr::int(n)
    }
}

impl From<Number> for Expr {
    fn from(n: Number) -> Self {
        Expr::num(n)
    }
}

impl From<&str> for Expr {
    fn from(name: &str) -> Self {
        Expr::sym(name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expr_is_send_and_sync() {
        fn check<T: Send + Sync>() {}
        check::<Expr>();
    }

    #[test]
    fn free_symbols_skip_bound_variables() {
        let n = Expr::sym("n");
        let x = Expr::sym("x");
        let sum = Expr::new(ExprNode::BigOp {
            kind: BigOpKind::Sum,
            expr: Expr::new(ExprNode::Mul(vec![n.clone(), x.clone()])),
            var: Arc::from("n"),
            lower: Expr::one(),
            upper: Expr::sym("m"),
        });
        let names: Vec<String> = sum.free_symbols().iter().map(|s| s.to_string()).collect();
        assert_eq!(names, vec!["m", "x"]);
        assert!(sum.contains_sym("x"));
        assert!(!sum.contains_sym("n"));
    }

    #[test]
    fn matrix_shape_is_checked() {
        assert!(Expr::matrix(vec![vec![Expr::one()], vec![]]).is_none());
        let m = Expr::matrix(vec![vec![Expr::one(), Expr::zero()]]).unwrap();
        assert!(matches!(
            m.node(),
            ExprNode::Matrix {
                rows: 1,
                cols: 2,
                ..
            }
        ));
    }

    #[test]
    fn infinity_detection() {
        assert!(Expr::infinity().is_infinite());
        assert!(Expr::neg_infinity().is_infinite());
        assert!(!Expr::sym("x").is_infinite());
    }
}
