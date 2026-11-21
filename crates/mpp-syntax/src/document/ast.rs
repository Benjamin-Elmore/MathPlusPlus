//! The document syntax tree: blocks, inlines and the sentence forms.

use crate::math::{Clause, MathAst, Node, Range};
use crate::span::{Diagnostic, Span};

/// A parsed `.mpp` file.
#[derive(Debug, Clone, PartialEq)]
pub struct Document {
    pub blocks: Vec<Block>,
    /// Every problem found in the document layer and in the maths inside it.
    pub diagnostics: Vec<Diagnostic>,
}

/// A stable identifier for a block: its content hash, made unique among blocks
/// with identical text by the order they appear in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct BlockId(pub u64);

/// One block of the document.
#[derive(Debug, Clone, PartialEq)]
pub struct Block {
    pub kind: BlockKind,
    /// Source range of the whole block.
    pub span: Span,
    /// Hash of the block's source text (and of anything that changes how it
    /// runs, such as `%noeval`). Equal text gives an equal hash anywhere in a file.
    pub hash: u64,
    /// Stable across edits elsewhere in the file; see [`BlockId`].
    pub id: BlockId,
}

/// What a block is.
#[derive(Debug, Clone, PartialEq)]
pub enum BlockKind {
    Heading {
        level: u8,
        content: Vec<Inline>,
    },
    Paragraph(Vec<Inline>),
    List(List),
    Table(Table),
    /// `>` lines; one entry per paragraph.
    Quote(Vec<Vec<Inline>>),
    Verbatim(Verbatim),
    /// `---`
    Rule,
    DisplayMath(DisplayMath),
    Figure(Figure),
    /// `![[file.mpp]]`
    EmbedImport {
        path: String,
        path_span: Span,
    },
    /// `Using $a, b$ [as $c, d$] from [[file.mpp]].`
    Using(Using),
    /// `Find the domain of $f$ …`
    Helper(Helper),
    /// `Analyse $f$ on $[0, 1]$.`
    Analyse(Analyse),
    /// `Solve $x^2 = 4$ for $x$.`
    Solve(Solve),
    /// `Assume $x > 0$.`, `Assume $n \in \mathbb{Z}$, $a \neq 0$.`
    Assume(Assume),
    /// `Data: $T$ from [[measurements.csv]].`
    Data(DataImport),
    /// `Let $a = 3$.` and prose that starts the same way.
    Let(Let),
    FootnoteDef {
        name: String,
        content: Vec<Inline>,
    },
    Directive(Directive),
    Theorem(Theorem),
}

/// A list; items may hold nested lists.
#[derive(Debug, Clone, PartialEq)]
pub struct List {
    pub ordered: bool,
    pub items: Vec<ListItem>,
}

/// One list item.
#[derive(Debug, Clone, PartialEq)]
pub struct ListItem {
    /// The written number of an ordered item.
    pub number: Option<u32>,
    pub content: Vec<Inline>,
    /// Lists nested under this item by deeper indentation.
    pub children: Vec<List>,
    pub span: Span,
}

/// Column alignment from the rule row of a table.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Align {
    Default,
    Left,
    Right,
    Center,
}

/// A table cell.
pub type Cell = Vec<Inline>;

/// A table: a header row, the alignment row and body rows.
#[derive(Debug, Clone, PartialEq)]
pub struct Table {
    pub header: Vec<Cell>,
    pub aligns: Vec<Align>,
    pub rows: Vec<Vec<Cell>>,
}

/// A fenced block shown as it is and never evaluated.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Verbatim {
    /// Text after the opening fence (`rust`, …).
    pub info: String,
    pub text: String,
    pub text_span: Span,
    /// False when the closing fence is missing.
    pub closed: bool,
}

/// A `$$ … $$` block.
#[derive(Debug, Clone, PartialEq)]
pub struct DisplayMath {
    pub ast: MathAst,
    /// The text between the `$$` delimiters.
    pub raw: Span,
    /// The equation number: the written `(n)` after the block, or the next
    /// automatic number for a block with a `\label`.
    pub number: Option<u32>,
    /// The `\label{…}` name.
    pub label: Option<String>,
    /// Marked by a `%noeval` line before it: never auto-evaluated.
    pub noeval: bool,
    /// False when the closing `$$` is missing.
    pub closed: bool,
}

/// A `Figure:` line.
#[derive(Debug, Clone, PartialEq)]
pub struct Figure {
    pub items: Vec<FigureItem>,
    /// The text after the first `. `; empty when there is none.
    pub caption: Vec<Inline>,
}

/// One comma-separated item of a figure.
#[derive(Debug, Clone, PartialEq)]
pub struct FigureItem {
    pub kind: FigureItemKind,
    pub span: Span,
}

/// What a figure item is.
#[derive(Debug, Clone, PartialEq)]
pub enum FigureItemKind {
    /// A curve, surface or equation.
    Expr(MathAst),
    /// `x \in [0, R]`, `x, y \in [-2, 2]`.
    Range(Range),
    /// `polar`, `tangent at $x = 1$`, …
    Keyword {
        keyword: FigureKeyword,
        arg: Option<Clause>,
    },
}

macro_rules! figure_keywords {
    ($($variant:ident => $text:literal),+ $(,)?) => {
        /// The figure keywords of grammar §4.
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
        pub enum FigureKeyword { $($variant),+ }

        impl FigureKeyword {
            /// Every keyword, in the order they are tried.
            pub const ALL: &'static [FigureKeyword] = &[$(FigureKeyword::$variant),+];

            /// The keyword as written.
            pub fn as_str(self) -> &'static str {
                match self { $(FigureKeyword::$variant => $text),+ }
            }
        }
    };
}

figure_keywords! {
    Polar => "polar",
    Parametric => "parametric",
    Implicit => "implicit",
    Points => "points",
    Contour => "contour",
    SlopeField => "slope field",
    VectorField => "vector field",
    GradientField => "gradient field",
    TangentPlaneAt => "tangent plane at",
    TangentAt => "tangent at",
    RiemannLeft => "riemann left",
    RiemannRight => "riemann right",
    RiemannMid => "riemann mid",
    RiemannTrapezoid => "riemann trapezoid",
    AreaBetween => "area between",
    RevolveAbout => "revolve about",
    Taylor => "taylor",
    NewtonFrom => "newton from",
    Scatter => "scatter",
    Histogram => "histogram",
    BoxPlot => "box plot",
}

/// `Using $names$ [as $aliases$] from [[path]].`
#[derive(Debug, Clone, PartialEq)]
pub struct Using {
    pub names: Vec<Node>,
    pub aliases: Option<Vec<Node>>,
    pub path: String,
    pub path_span: Span,
}

/// The verb of a helper sentence.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HelperVerb {
    Find,
    Compute,
    Determine,
}

/// The analysis a helper sentence asks for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HelperName {
    Domain,
    Range,
    Intercepts,
    CriticalPoints,
    Extrema,
    LocalExtrema,
    AbsoluteExtrema,
    IntervalsOfIncrease,
    Concavity,
    InflectionPoints,
    Asymptotes,
    TangentLine,
    NormalLine,
    Linearisation,
}

/// `Find the domain of $f$ on $[0, 1]$ at $x = 2$.`
#[derive(Debug, Clone, PartialEq)]
pub struct Helper {
    pub verb: HelperVerb,
    pub helper: HelperName,
    pub target: Node,
    pub interval: Option<Node>,
    pub at: Option<Clause>,
}

/// `Analyse $f$ [on $interval$].`
#[derive(Debug, Clone, PartialEq)]
pub struct Analyse {
    pub target: Node,
    pub interval: Option<Node>,
}

/// `Solve $equation$ [for $names$].`
#[derive(Debug, Clone, PartialEq)]
pub struct Solve {
    pub equation: Node,
    pub unknowns: Option<Vec<Node>>,
}

/// `Data: $T$ from [[file.csv]].`: a table of measurements read from a CSV file.
#[derive(Debug, Clone, PartialEq)]
pub struct DataImport {
    /// The table's name.
    pub name: Node,
    /// The path as written (relative to the document).
    pub path: String,
    pub path_span: Span,
}

/// `Assume $cond$ {, $cond$} [and $cond$].`: facts about names, in force from here on.
#[derive(Debug, Clone, PartialEq)]
pub struct Assume {
    pub conditions: Vec<Node>,
}

/// A paragraph that starts `Let $…$`: the maths in it defines variables.
#[derive(Debug, Clone, PartialEq)]
pub struct Let {
    pub content: Vec<Inline>,
}

impl Let {
    /// The maths spans of the sentence, in order.
    pub fn math(&self) -> impl Iterator<Item = &InlineMath> {
        self.content.iter().filter_map(|i| match &i.kind {
            InlineKind::Math(m) => Some(m),
            _ => None,
        })
    }
}

/// A `%` line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Directive {
    pub kind: DirectiveKind,
    /// The text after the `%`, trimmed.
    pub text: String,
}

/// What a `%` line does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DirectiveKind {
    /// `%noeval`: the next maths block is never auto-evaluated.
    NoEval,
    /// A number-display directive such as `%sf 4` or `%block dp 2`; read it with
    /// [`crate::document::display::parse_display_directive`].
    Display,
    /// Any other `%` line.
    Comment,
}

/// The kinds of theorem-style block.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TheoremKind {
    Definition,
    Theorem,
    Lemma,
    Corollary,
    Proposition,
    Example,
    Remark,
    Proof,
}

impl TheoremKind {
    /// Every kind, in the order they are tried.
    pub const ALL: &'static [TheoremKind] = &[
        TheoremKind::Definition,
        TheoremKind::Theorem,
        TheoremKind::Lemma,
        TheoremKind::Corollary,
        TheoremKind::Proposition,
        TheoremKind::Example,
        TheoremKind::Remark,
        TheoremKind::Proof,
    ];

    /// The word as written (`Theorem`).
    pub fn as_str(self) -> &'static str {
        match self {
            TheoremKind::Definition => "Definition",
            TheoremKind::Theorem => "Theorem",
            TheoremKind::Lemma => "Lemma",
            TheoremKind::Corollary => "Corollary",
            TheoremKind::Proposition => "Proposition",
            TheoremKind::Example => "Example",
            TheoremKind::Remark => "Remark",
            TheoremKind::Proof => "Proof",
        }
    }
}

/// `**Theorem.** …`, with the blocks that follow it inside the statement
/// (a proof runs on to its `∎`).
#[derive(Debug, Clone, PartialEq)]
pub struct Theorem {
    pub kind: TheoremKind,
    pub body: Vec<Block>,
    /// For a proof: whether it ends at `∎` or `\qed`. Always true otherwise.
    pub closed: bool,
}

/// A piece of running text.
#[derive(Debug, Clone, PartialEq)]
pub struct Inline {
    pub kind: InlineKind,
    pub span: Span,
}

/// What an inline is.
#[derive(Debug, Clone, PartialEq)]
pub enum InlineKind {
    Text(String),
    /// A line break inside a paragraph.
    SoftBreak,
    Emphasis(Vec<Inline>),
    Strong(Vec<Inline>),
    Math(InlineMath),
    /// `` `code` ``
    Code(String),
    WikiLink {
        path: String,
        alias: Option<String>,
    },
    Link {
        text: Vec<Inline>,
        url: String,
    },
    FootnoteRef(String),
    /// `\eqref{label}`; `number` is filled in once the label is known.
    EqRef {
        label: String,
        number: Option<u32>,
    },
    /// `\$`, `\*`, … : the escaped character.
    Escape(char),
}

/// `$ … $` inside text.
#[derive(Debug, Clone, PartialEq)]
pub struct InlineMath {
    pub ast: MathAst,
    /// The text between the `$` delimiters.
    pub raw: Span,
}
