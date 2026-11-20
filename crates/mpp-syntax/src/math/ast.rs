//! The maths syntax tree.
//!
//! The tree keeps presentation detail (`\frac` against `/`, which brackets, which
//! matrix environment, explicit against implicit products) because the typesetter
//! needs it. `mpp-eval` lowers it into the meaning-only core tree.

use crate::span::Span;
use crate::symbols::Font;

/// A parsed maths span: one or more statement lines joined by `\\`.
#[derive(Debug, Clone, PartialEq)]
pub struct MathAst {
    pub lines: Vec<Line>,
    /// `\label{name}`, if the span has one.
    pub label: Option<Label>,
    /// True when the lines sat inside an alignment environment (`aligned`, `align*` …).
    pub aligned: bool,
    pub span: Span,
}

/// An equation label from `\label{…}`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Label {
    pub name: String,
    pub span: Span,
}

/// One statement line with its clauses.
#[derive(Debug, Clone, PartialEq)]
pub struct Line {
    pub statement: Statement,
    pub clauses: Vec<Clause>,
    /// Marked `%noeval`: never auto-evaluated.
    pub noeval: bool,
    /// Byte offsets of the `&` alignment points on this line.
    pub align: Vec<usize>,
    pub span: Span,
}

/// The core of a statement line.
#[derive(Debug, Clone, PartialEq)]
pub enum Statement {
    /// `head := body` or `head ≡ body`.
    Definition {
        head: DefHead,
        kind: DefKind,
        body: Node,
    },
    /// `expr = ?`, `expr ≈ ?` or `expr = ?!`.
    Query {
        expr: Node,
        mode: QueryMode,
        steps: bool,
    },
    /// `lhs \overset{?}{=} rhs`.
    Check { lhs: Node, rhs: Node },
    /// A bare expression or a chain of relations (`a < b \le c`).
    Relation(Node),
    /// `a, f, g` (used by `Using … from`).
    NameList(Vec<Node>),
    /// `x, y \in [a, b]`: a plotting range or slider.
    Range(Range),
    /// Nothing on this line.
    Empty,
}

/// `:=` against `≡`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DefKind {
    /// `:=`, a changeable variable or function.
    Assign,
    /// `≡`, a constant.
    Const,
}

/// `= ?` against `≈ ?`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QueryMode {
    Exact,
    Approx,
}

/// The left side of a definition.
#[derive(Debug, Clone, PartialEq)]
pub enum DefHead {
    /// `a`, `v_0`, `\theta`.
    Name(Node),
    /// `f(x, y)`.
    Function { name: Node, params: Vec<Node> },
    /// `a_{n+1}`: `name` is the bare `a`, `index` is `n`, `shift` is `1`.
    Recurrence {
        name: Node,
        index: Node,
        shift: Node,
    },
    /// `a_1`: an initial term. When no recurrence for `a` exists the evaluator
    /// reads it as the plain variable `a_1` instead.
    Initial { name: Node, index: Node },
    /// Anything else; a diagnostic has been reported.
    Invalid(Node),
}

/// `x, y \in S`.
#[derive(Debug, Clone, PartialEq)]
pub struct Range {
    pub names: Vec<Node>,
    pub set: Node,
    pub span: Span,
}

/// A clause after a comma.
#[derive(Debug, Clone, PartialEq)]
pub enum Clause {
    /// `r = 3`: holds only for this statement.
    Where { name: Node, value: Node, span: Span },
    /// `a \in [0, 5]`.
    Range(Range),
    /// Any other relation chain, e.g. `x > 0`.
    Condition(Node),
}

impl Clause {
    /// Source span of the clause.
    pub fn span(&self) -> Span {
        match self {
            Clause::Where { span, .. } => *span,
            Clause::Range(r) => r.span,
            Clause::Condition(n) => n.span,
        }
    }
}

/// An expression node.
#[derive(Debug, Clone, PartialEq)]
pub struct Node {
    pub kind: NodeKind,
    pub span: Span,
}

impl Node {
    /// Makes a node.
    pub fn new(kind: NodeKind, span: Span) -> Self {
        Node { kind, span }
    }

    /// An error placeholder at `span`.
    pub fn error(span: Span) -> Self {
        Node::new(NodeKind::Error, span)
    }

    /// The node with any presentation-only group brackets removed.
    pub fn ungrouped(&self) -> &Node {
        match &self.kind {
            NodeKind::Group { inner, .. } => inner.ungrouped(),
            _ => self,
        }
    }

    /// The name, when this node is a plain name (or `e` / `i` used as a name).
    pub fn as_name(&self) -> Option<Name> {
        match &self.kind {
            NodeKind::Name(n) => Some(n.clone()),
            NodeKind::Constant(c) => c.letter().map(|l| Name::simple(l.to_string())),
            NodeKind::Group { inner, .. } => inner.as_name(),
            _ => None,
        }
    }

    /// True when the node is a name (see [`Node::as_name`]).
    pub fn is_name(&self) -> bool {
        self.as_name().is_some()
    }

    /// True for an error placeholder anywhere at the top of this node.
    pub fn is_error(&self) -> bool {
        matches!(self.kind, NodeKind::Error)
    }
}

/// Every kind of expression.
#[derive(Debug, Clone, PartialEq)]
pub enum NodeKind {
    /// A number, keeping its source text (`2.50`, `1e-3`).
    Number(String),
    Name(Name),
    Constant(Constant),
    /// A symbol with no maths meaning of its own (`∀`, `∃`, `∴`, `°`).
    Symbol(&'static str),
    /// `\ldots`, `\cdots`, `…`.
    Dots(Dots),
    /// `\text{…}`.
    Text(String),
    /// The empty left side of an aligned line that starts with a relation (`&= …`).
    Continuation,
    /// Placeholder where parsing failed; a diagnostic points here.
    Error,
    /// `f(x, y)`: a call when `f` is a function, otherwise a product.
    Call {
        callee: Box<Node>,
        args: Vec<Node>,
        sized: bool,
    },
    /// A known function (`\sin`, `\ln`, `\max` …) applied with or without brackets.
    FuncApp(Box<FuncApp>),
    Unary {
        op: UnaryOp,
        operand: Box<Node>,
    },
    Binary {
        op: BinOp,
        lhs: Box<Node>,
        rhs: Box<Node>,
    },
    /// `\frac{a}{b}`, `a/b`, `a ÷ b`.
    Frac {
        num: Box<Node>,
        den: Box<Node>,
        style: FracStyle,
    },
    Pow {
        base: Box<Node>,
        exp: Box<Node>,
    },
    /// A subscript on something that is not a name (names keep theirs in [`Name::sub`]).
    Subscript {
        base: Box<Node>,
        sub: Box<Node>,
    },
    /// `f'`, `f''`.
    Prime {
        base: Box<Node>,
        count: u8,
    },
    /// `n!` (count 1) or `n!!` (count 2).
    Factorial {
        operand: Box<Node>,
        count: u8,
    },
    /// `|x|`, `‖v‖`, `⌊x⌋`, `⌈x⌉`.
    Fenced {
        fence: Fence,
        inner: Box<Node>,
        style: FenceStyle,
    },
    /// `\sqrt{x}`, `\sqrt[n]{x}`, `√x`, `sqrt(x)`, `root(n, x)`.
    Sqrt {
        index: Option<Box<Node>>,
        radicand: Box<Node>,
        style: SqrtStyle,
    },
    /// `\binom{n}{k}`, `binom(n, k)`.
    Binom {
        n: Box<Node>,
        k: Box<Node>,
        style: CallStyle,
    },
    Matrix {
        env: MatrixEnv,
        rows: Vec<Vec<Node>>,
    },
    Cases {
        rows: Vec<CaseRow>,
    },
    /// `\{1, 2\}`, `\emptyset`.
    Set {
        elems: Vec<Node>,
        style: SetStyle,
    },
    /// `[a, b]`, `[a, b)`, `(a, b]`.
    Interval {
        lo: Box<Node>,
        hi: Box<Node>,
        lo_closed: bool,
        hi_closed: bool,
        sized: bool,
    },
    /// `(a, b)`, `\langle u, v \rangle`, `[1, 2, 3]`. A pair in round brackets is
    /// read as an open interval where a set is expected.
    Tuple {
        elems: Vec<Node>,
        bracket: Bracket,
        sized: bool,
    },
    /// Brackets around one expression; presentation only.
    Group {
        inner: Box<Node>,
        bracket: Bracket,
        sized: bool,
    },
    /// `\sum`, `\prod`.
    BigOp(Box<BigOp>),
    Integral(Box<Integral>),
    Limit(Box<Limit>),
    Derivative(Box<Derivative>),
    /// `\nabla f`, `\nabla \cdot F`, `\nabla \times F`, `\nabla^2 f`.
    Nabla {
        op: NablaOp,
        body: Option<Box<Node>>,
    },
    /// `\operatorname{name}_{sub}^{sup} args`.
    NamedOp(Box<NamedOp>),
    /// An accent over something that is not a single name (`\overline{z + w}`).
    Accented {
        accent: Accent,
        inner: Box<Node>,
    },
    /// A font applied to something that is not a plain name.
    Styled {
        font: Font,
        inner: Box<Node>,
    },
    /// `a = b`, `0 < x \le 1`, `x \in S`.
    Relation {
        first: Box<Node>,
        rest: Vec<(RelOp, Node)>,
    },
}

/// A name: a letter, a Greek letter, or a longer upright name, with decorations.
#[derive(Debug, Clone, PartialEq)]
pub struct Name {
    /// The letter as Unicode (`x`, `θ`), or a longer name (`max`, `crit`).
    pub base: String,
    pub sub: Option<Box<Node>>,
    pub accent: Option<Accent>,
    pub font: Option<Font>,
}

impl Name {
    /// A bare name with no decorations.
    pub fn simple(base: impl Into<String>) -> Self {
        Name {
            base: base.into(),
            sub: None,
            accent: None,
            font: None,
        }
    }
}

/// Built-in constants.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Constant {
    Pi,
    /// `e`; also a name when a definition or index binds it.
    E,
    /// `i`; also a name when a definition or index binds it (`\sum_{i=1}`).
    I,
    Infinity,
}

impl Constant {
    /// The letter for `e` and `i`, which may be rebound as names.
    pub fn letter(self) -> Option<char> {
        match self {
            Constant::E => Some('e'),
            Constant::I => Some('i'),
            _ => None,
        }
    }
}

/// Kinds of ellipsis.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dots {
    Low,
    Centre,
    Vertical,
    Diagonal,
}

/// A known function application.
#[derive(Debug, Clone, PartialEq)]
pub struct FuncApp {
    /// Name from the symbol table (`sin`, `log`, `max`).
    pub func: &'static str,
    /// `\log_2`, `\max_{x \in S}`.
    pub sub: Option<Node>,
    /// `\sin^2 x` squares the result.
    pub power: Option<Node>,
    /// `\sin^{-1} x`: the inverse function.
    pub inverse: bool,
    pub args: Vec<Node>,
    /// Written with brackets: `\sin(x)` against `\sin x`.
    pub bracketed: bool,
}

/// Prefix operators.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnaryOp {
    Neg,
    Plus,
    PlusMinus,
    MinusPlus,
    Not,
}

/// How a product was written.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MulStyle {
    /// `2x`, `x y`.
    Implicit,
    /// `*`.
    Star,
    /// `\cdot`, `·`.
    Cdot,
}

/// Binary operators.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinOp {
    Add,
    Sub,
    PlusMinus,
    MinusPlus,
    Mul(MulStyle),
    /// `\times`: a product of numbers, a cross product of vectors.
    Times,
    Compose,
    Tensor,
    DirectSum,
    Union,
    Intersect,
    SetMinus,
    /// `a \bmod n`: the remainder of `a` on division by `n`.
    Mod,
    And,
    Or,
}

/// `\frac` and friends against the slash.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FracStyle {
    Frac,
    TFrac,
    DFrac,
    Slash,
    /// `÷`.
    Obelus,
}

/// The four fences.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fence {
    Abs,
    Norm,
    Floor,
    Ceil,
}

/// How a fence was written.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FenceStyle {
    /// `|x|`, `‖v‖`, `⌊x⌋`, `\lfloor x \rfloor`.
    Delimiters,
    /// `\left| x \right|`.
    Sized,
    /// `\abs{x}`, `\norm{x}`.
    Command,
    /// `abs(x)`, `floor(x)`.
    Word,
}

/// How a root was written.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SqrtStyle {
    Command,
    /// `√x`, `∛x`.
    Radical,
    /// `sqrt(x)`, `root(n, x)`.
    Word,
}

/// LaTeX command against plain call syntax.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CallStyle {
    Command,
    Word,
}

/// Matrix environments.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MatrixEnv {
    Matrix,
    PMatrix,
    BMatrix,
    /// `Bmatrix`, curly braces.
    BraceMatrix,
    /// `vmatrix`: a determinant.
    VMatrix,
    /// `Vmatrix`: double bars.
    DoubleVMatrix,
    SmallMatrix,
}

impl MatrixEnv {
    /// The environment for a `\begin{…}` name.
    pub fn from_env(name: &str) -> Option<Self> {
        Some(match name {
            "matrix" => MatrixEnv::Matrix,
            "pmatrix" => MatrixEnv::PMatrix,
            "bmatrix" => MatrixEnv::BMatrix,
            "Bmatrix" => MatrixEnv::BraceMatrix,
            "vmatrix" => MatrixEnv::VMatrix,
            "Vmatrix" => MatrixEnv::DoubleVMatrix,
            "smallmatrix" => MatrixEnv::SmallMatrix,
            _ => return None,
        })
    }
}

/// One row of `\begin{cases}`.
#[derive(Debug, Clone, PartialEq)]
pub struct CaseRow {
    pub value: Node,
    pub condition: CaseCondition,
}

/// The condition column of a case row.
#[derive(Debug, Clone, PartialEq)]
pub enum CaseCondition {
    When(Node),
    Otherwise,
    /// No `&` column on this row.
    Missing,
}

/// How a set literal was written.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SetStyle {
    /// `\{ … \}`.
    Escaped,
    /// `{1, 2}` in the plain form.
    Braces,
    /// `\emptyset`, `∅`.
    EmptySymbol,
}

/// Bracket kinds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Bracket {
    Paren,
    Square,
    /// `\{ \}`.
    Brace,
    Angle,
    /// A TeX group `{ … }`, invisible when typeset.
    TexGroup,
    /// `\left.` with an invisible delimiter.
    None,
}

/// `\sum`, `\prod`, `\coprod`.
#[derive(Debug, Clone, PartialEq)]
pub struct BigOp {
    pub op: BigOpKind,
    /// The index in `\sum_{n=1}`.
    pub index: Option<Node>,
    /// The start in `\sum_{n=1}`.
    pub from: Option<Node>,
    /// The upper limit.
    pub to: Option<Node>,
    /// A lower script that is not `name = start` (`\sum_{x \in S}`, `\sum_k`'s `k` goes in `index`).
    pub over: Option<Node>,
    pub body: Node,
}

/// Which big operator.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BigOpKind {
    Sum,
    Prod,
    Coprod,
}

/// An integral.
#[derive(Debug, Clone, PartialEq)]
pub struct Integral {
    pub kind: IntKind,
    /// Lower bound, or the region when there is no upper bound (`\iint_D`).
    pub lower: Option<Node>,
    pub upper: Option<Node>,
    pub body: Node,
    /// The variables of the differentials, in order (`dx dy` gives `[x, y]`).
    pub diffs: Vec<Node>,
    /// The differential is dotted with the body (`\oint \mathbf F \cdot d\mathbf r`).
    pub dot: bool,
    /// Written `\int \frac{dx}{x}`: the differential sat in the numerator, which is now `1`.
    pub diff_in_numerator: bool,
}

/// ∫ ∬ ∭ ∮ ∯.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IntKind {
    Single,
    Double,
    Triple,
    Contour,
    Surface,
}

impl IntKind {
    /// How many differentials the sign normally takes.
    pub fn arity(self) -> usize {
        match self {
            IntKind::Single | IntKind::Contour => 1,
            IntKind::Double | IntKind::Surface => 2,
            IntKind::Triple => 3,
        }
    }
}

/// `\lim_{x \to a^±} body`.
#[derive(Debug, Clone, PartialEq)]
pub struct Limit {
    pub var: Node,
    pub target: Node,
    pub side: Option<Side>,
    pub body: Node,
}

/// One-sided limits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    Left,
    Right,
}

/// A derivative in any notation.
#[derive(Debug, Clone, PartialEq)]
pub struct Derivative {
    pub partial: bool,
    /// Total order (`d^2` in `\frac{d^2}{dx^2}`); `None` means first order.
    pub order: Option<Node>,
    /// Variables with their own powers (`\partial x^2 \partial y`).
    pub vars: Vec<DerivVar>,
    /// What is differentiated; `None` for a bare operator with nothing after it.
    pub of: Option<Node>,
    pub notation: DerivNotation,
}

/// One variable in a derivative's denominator.
#[derive(Debug, Clone, PartialEq)]
pub struct DerivVar {
    pub var: Node,
    pub power: Option<Node>,
}

/// How the derivative was written.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DerivNotation {
    /// `\frac{d}{dx} f` or `d/dx f`: an operator applied to what follows.
    Operator { frac: bool },
    /// `\frac{dy}{dx}` or `dy/dx`: the whole quotient.
    Whole { frac: bool },
    /// `\partial_x f`, `∂ₓ f`.
    PartialSub,
    /// `D_x f`.
    DSub,
    /// `\dot x` (1), `\ddot x` (2): with respect to time.
    Dot(u8),
    /// `\partial f` with no variable (boundary or differential).
    Bare,
}

/// The four ∇ operators.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NablaOp {
    Grad,
    Div,
    Curl,
    Laplacian,
}

/// `\operatorname{name}_{sub}^{sup} args`.
#[derive(Debug, Clone, PartialEq)]
pub struct NamedOp {
    pub name: String,
    pub sub: Option<Node>,
    pub sup: Option<Node>,
    pub args: Vec<Node>,
    pub bracketed: bool,
}

/// Accents.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Accent {
    Hat,
    WideHat,
    Bar,
    Overline,
    Vec,
    Dot,
    DDot,
    Tilde,
    WideTilde,
}

impl Accent {
    /// The accent for a symbol-table name (`hat`, `vec` …).
    pub fn from_name(name: &str) -> Option<Self> {
        Some(match name {
            "hat" => Accent::Hat,
            "widehat" => Accent::WideHat,
            "bar" => Accent::Bar,
            "overline" => Accent::Overline,
            "vec" => Accent::Vec,
            "dot" => Accent::Dot,
            "ddot" => Accent::DDot,
            "tilde" => Accent::Tilde,
            "widetilde" => Accent::WideTilde,
            _ => return None,
        })
    }

    /// The accent for a Unicode combining mark.
    pub fn from_combining(c: char) -> Option<Self> {
        Some(match c {
            '\u{0302}' => Accent::Hat,
            '\u{0304}' => Accent::Bar,
            '\u{0305}' => Accent::Overline,
            '\u{20D7}' => Accent::Vec,
            '\u{0307}' => Accent::Dot,
            '\u{0308}' => Accent::DDot,
            '\u{0303}' => Accent::Tilde,
            _ => return None,
        })
    }

    /// Lower-case name used in debug output.
    pub fn name(self) -> &'static str {
        match self {
            Accent::Hat => "hat",
            Accent::WideHat => "widehat",
            Accent::Bar => "bar",
            Accent::Overline => "overline",
            Accent::Vec => "vec",
            Accent::Dot => "dot",
            Accent::DDot => "ddot",
            Accent::Tilde => "tilde",
            Accent::WideTilde => "widetilde",
        }
    }
}

/// Relation operators.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RelOp {
    Eq,
    Ne,
    Lt,
    Gt,
    Le,
    Ge,
    Approx,
    Equiv,
    Sim,
    Simeq,
    Cong,
    Propto,
    Ll,
    Gg,
    In,
    NotIn,
    Ni,
    Subset,
    SubsetEq,
    Supset,
    SupsetEq,
    Implies,
    Iff,
    /// `\to`, `->`.
    To,
    MapsTo,
    /// Any other relation or arrow, by its Unicode symbol.
    Other(&'static str),
}

impl RelOp {
    /// The Unicode symbol.
    pub fn symbol(self) -> &'static str {
        match self {
            RelOp::Eq => "=",
            RelOp::Ne => "≠",
            RelOp::Lt => "<",
            RelOp::Gt => ">",
            RelOp::Le => "≤",
            RelOp::Ge => "≥",
            RelOp::Approx => "≈",
            RelOp::Equiv => "≡",
            RelOp::Sim => "∼",
            RelOp::Simeq => "≃",
            RelOp::Cong => "≅",
            RelOp::Propto => "∝",
            RelOp::Ll => "≪",
            RelOp::Gg => "≫",
            RelOp::In => "∈",
            RelOp::NotIn => "∉",
            RelOp::Ni => "∋",
            RelOp::Subset => "⊂",
            RelOp::SubsetEq => "⊆",
            RelOp::Supset => "⊃",
            RelOp::SupsetEq => "⊇",
            RelOp::Implies => "⇒",
            RelOp::Iff => "⇔",
            RelOp::To => "→",
            RelOp::MapsTo => "↦",
            RelOp::Other(s) => s,
        }
    }

    /// The relation for a Unicode symbol (as stored in the symbol table).
    pub fn from_symbol(s: &'static str) -> Self {
        match s {
            "=" => RelOp::Eq,
            "≠" => RelOp::Ne,
            "<" => RelOp::Lt,
            ">" => RelOp::Gt,
            "≤" => RelOp::Le,
            "≥" => RelOp::Ge,
            "≈" => RelOp::Approx,
            "≡" => RelOp::Equiv,
            "∼" => RelOp::Sim,
            "≃" => RelOp::Simeq,
            "≅" => RelOp::Cong,
            "∝" => RelOp::Propto,
            "≪" => RelOp::Ll,
            "≫" => RelOp::Gg,
            "∈" => RelOp::In,
            "∉" => RelOp::NotIn,
            "∋" => RelOp::Ni,
            "⊂" => RelOp::Subset,
            "⊆" => RelOp::SubsetEq,
            "⊃" => RelOp::Supset,
            "⊇" => RelOp::SupsetEq,
            "⇒" | "⟹" => RelOp::Implies,
            "⇔" | "⟺" => RelOp::Iff,
            "→" | "⟶" => RelOp::To,
            "↦" | "⟼" => RelOp::MapsTo,
            other => RelOp::Other(other),
        }
    }
}
