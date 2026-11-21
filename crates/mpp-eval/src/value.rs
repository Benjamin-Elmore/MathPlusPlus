//! What names are bound to, and the values results carry.

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;

use mpp_core::{Expr, ExprNode};
use mpp_syntax::document::BlockId;
use mpp_syntax::Span;

/// Where a binding was made.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Origin {
    /// The block holding the definition.
    pub block: BlockId,
    /// The statement line of the definition.
    pub span: Span,
    /// The file the definition came from, as shown to the user, when it was
    /// imported (`None` for this document).
    pub file: Option<Arc<str>>,
    /// The resolved path of that file, for go-to-definition (`span` is a byte
    /// range in it).
    pub path: Option<Arc<Path>>,
    /// The 1-based line of the definition in that file.
    pub line: Option<usize>,
    /// The number as typed when the definition is a decimal (`g \equiv 9.81`), so it
    /// is shown as `9.81` rather than `981/100`.
    pub decimal: Option<Arc<str>>,
    /// The definition is worked out from a decimal the user typed (`9.81`, or a
    /// name defined from one), so answers that use it are shown as decimals.
    pub decimal_input: bool,
}

impl Origin {
    /// A definition in this document.
    pub fn local(block: BlockId, span: Span) -> Self {
        Origin {
            block,
            span,
            file: None,
            path: None,
            line: None,
            decimal: None,
            decimal_input: false,
        }
    }

    /// Same place, ignoring the byte span (which moves when text above is edited).
    fn same(&self, other: &Origin) -> bool {
        self.block == other.block && self.file == other.file
    }
}

/// `:=` against `≡`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VarKind {
    /// `:=`: may be redefined; stores the expression and recomputes from it.
    Variable,
    /// `≡`: a fixed value that cannot be redefined.
    Constant,
}

/// A sequence given by a recurrence `a_{n+1} := …` and initial terms `a_1 := 1`.
#[derive(Debug, Clone, PartialEq)]
pub struct Sequence {
    /// The index name of the recurrence (`n`).
    pub index: String,
    /// The shift on the left side (`1` in `a_{n+1}`).
    pub shift: i64,
    /// The right side of the recurrence, once it is defined.
    pub body: Option<Expr>,
    /// Initial terms by index.
    pub initials: BTreeMap<i64, Expr>,
}

/// What a name is bound to.
#[derive(Debug, Clone)]
pub enum Binding {
    /// `a := 3`, `E := ½ m v²`, `g ≡ 9.81`. For `:=` the value is the expression as
    /// written (it recomputes from its names whenever it is used); for `≡` it is
    /// the value at the point of definition.
    Variable {
        kind: VarKind,
        value: Expr,
        origin: Origin,
    },
    /// `f(x, y) := …`.
    Function {
        params: Vec<String>,
        body: Expr,
        origin: Origin,
    },
    /// `a_{n+1} := …` with its initial terms.
    Sequence { seq: Sequence, origin: Origin },
    /// A name an import declares but could not supply (the import failed).
    Imported { from: String, origin: Origin },
}

impl Binding {
    /// Where the binding was made.
    pub fn origin(&self) -> &Origin {
        match self {
            Binding::Variable { origin, .. }
            | Binding::Function { origin, .. }
            | Binding::Sequence { origin, .. }
            | Binding::Imported { origin, .. } => origin,
        }
    }

    /// Mutable access to the origin.
    pub fn origin_mut(&mut self) -> &mut Origin {
        match self {
            Binding::Variable { origin, .. }
            | Binding::Function { origin, .. }
            | Binding::Sequence { origin, .. }
            | Binding::Imported { origin, .. } => origin,
        }
    }

    /// True for a `≡` constant.
    pub fn is_constant(&self) -> bool {
        matches!(
            self,
            Binding::Variable {
                kind: VarKind::Constant,
                ..
            }
        )
    }

    /// The parameters, when this is a user function.
    pub fn params(&self) -> Option<&[String]> {
        match self {
            Binding::Function { params, .. } => Some(params),
            _ => None,
        }
    }

    /// The same meaning, ignoring byte positions. Used to decide whether a block
    /// that read this binding needs to run again.
    pub fn same(&self, other: &Binding) -> bool {
        match (self, other) {
            (
                Binding::Variable {
                    kind: k1,
                    value: v1,
                    origin: o1,
                },
                Binding::Variable {
                    kind: k2,
                    value: v2,
                    origin: o2,
                },
            ) => k1 == k2 && v1 == v2 && o1.same(o2),
            (
                Binding::Function {
                    params: p1,
                    body: b1,
                    origin: o1,
                },
                Binding::Function {
                    params: p2,
                    body: b2,
                    origin: o2,
                },
            ) => p1 == p2 && b1 == b2 && o1.same(o2),
            (
                Binding::Sequence {
                    seq: s1,
                    origin: o1,
                },
                Binding::Sequence {
                    seq: s2,
                    origin: o2,
                },
            ) => s1 == s2 && o1.same(o2),
            (
                Binding::Imported {
                    from: f1,
                    origin: o1,
                },
                Binding::Imported {
                    from: f2,
                    origin: o2,
                },
            ) => f1 == f2 && o1.same(o2),
            _ => false,
        }
    }

    /// A short description for panels and hovers (`constant`, `function of x, y` …).
    pub fn describe(&self) -> String {
        match self {
            Binding::Variable {
                kind: VarKind::Constant,
                value,
                ..
            } => format!("constant {value}"),
            Binding::Variable { value, .. } => format!("variable {value}"),
            Binding::Function { params, body, .. } => {
                format!("function ({}) ↦ {body}", params.join(", "))
            }
            Binding::Sequence { seq, .. } => {
                format!(
                    "sequence in {} with {} initial terms",
                    seq.index,
                    seq.initials.len()
                )
            }
            Binding::Imported { from, .. } => format!("imported from {from} (not loaded)"),
        }
    }
}

/// Compares two optional bindings with [`Binding::same`].
pub(crate) fn same_binding(a: Option<&Arc<Binding>>, b: Option<&Arc<Binding>>) -> bool {
    match (a, b) {
        (None, None) => true,
        (Some(a), Some(b)) => Arc::ptr_eq(a, b) || a.same(b),
        _ => false,
    }
}

/// A result value, classified by what it is.
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    /// An ordinary expression (number, symbolic expression, unevaluated holder).
    Expr(Expr),
    /// A user function.
    Function { params: Vec<String>, body: Expr },
    /// A matrix.
    Matrix(Expr),
    /// A truth value.
    Bool(bool),
    /// A finite set or an interval.
    Set(Expr),
    /// A tuple, vector or point.
    Tuple(Expr),
    /// An equation or inequality.
    Relation(Expr),
    /// A labelled table (`table((label, value), …)`), such as a curve-sketch summary;
    /// [`mpp_core::table::rows`] reads its rows.
    Table(Expr),
}

impl Value {
    /// Classifies an expression.
    pub fn from_expr(e: Expr) -> Value {
        if mpp_core::table::rows(&e).is_some() {
            return Value::Table(e);
        }
        match e.node() {
            ExprNode::Matrix { .. } => Value::Matrix(e),
            ExprNode::Bool(b) => Value::Bool(*b),
            ExprNode::Set(_) | ExprNode::Interval { .. } => Value::Set(e),
            ExprNode::Tuple(_) => Value::Tuple(e),
            ExprNode::Relation(..) | ExprNode::Logic(..) => Value::Relation(e),
            _ => Value::Expr(e),
        }
    }

    /// The expression behind the value (the body for a function).
    pub fn expr(&self) -> Expr {
        match self {
            Value::Expr(e)
            | Value::Matrix(e)
            | Value::Set(e)
            | Value::Tuple(e)
            | Value::Relation(e)
            | Value::Table(e) => e.clone(),
            Value::Function { body, .. } => body.clone(),
            Value::Bool(b) => Expr::new(ExprNode::Bool(*b)),
        }
    }
}

/// The names in scope at one point of the document.
#[derive(Debug, Clone, Default)]
pub struct Env {
    map: BTreeMap<String, Arc<Binding>>,
}

impl Env {
    /// The binding of `name`.
    pub fn get(&self, name: &str) -> Option<&Arc<Binding>> {
        self.map.get(name)
    }

    /// Binds `name`, replacing any earlier binding.
    pub fn insert(&mut self, name: String, binding: Arc<Binding>) {
        self.map.insert(name, binding);
    }

    /// Every binding, sorted by name.
    pub fn iter(&self) -> impl Iterator<Item = (&String, &Arc<Binding>)> {
        self.map.iter()
    }

    /// The bindings as a map.
    pub fn into_map(self) -> BTreeMap<String, Arc<Binding>> {
        self.map
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn var(value: i64, span: Span) -> Binding {
        Binding::Variable {
            kind: VarKind::Variable,
            value: Expr::int(value),
            origin: Origin::local(BlockId(1), span),
        }
    }

    #[test]
    fn same_ignores_spans_only() {
        let a = var(1, Span::new(0, 3));
        assert!(a.same(&var(1, Span::new(10, 13))));
        assert!(!a.same(&var(2, Span::new(0, 3))));
        assert!(same_binding(None, None));
        assert!(!same_binding(Some(&Arc::new(a)), None));
    }

    #[test]
    fn values_are_classified() {
        assert!(matches!(
            Value::from_expr(Expr::new(ExprNode::Bool(true))),
            Value::Bool(true)
        ));
        assert!(matches!(Value::from_expr(Expr::int(2)), Value::Expr(_)));
    }
}
