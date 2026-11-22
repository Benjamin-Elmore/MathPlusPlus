//! Every item of the Insert menu: templates with boxes, Greek letters, symbols,
//! helpers and graphs, with the controls each template offers.
//!
//! Templates are written in the snippet language of [`crate::editor::template`].
//! The `.mpp` text they produce is what the file holds; the user only ever sees
//! it typeset.

use std::sync::OnceLock;

use mpp_syntax::symbols::{SymbolKind, SYMBOLS};

use crate::editor::Knobs;

/// The menu's groups, in menu order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Group {
    Calculus,
    Sums,
    Algebra,
    Linear,
    Greek,
    Symbols,
    Helpers,
    Graphs,
    Units,
    /// Notes for the reader (formatted comments).
    Text,
    /// The paper features: title block, theorem-like blocks, references.
    Document,
}

impl Group {
    pub const ALL: [Group; 11] = [
        Group::Calculus,
        Group::Sums,
        Group::Algebra,
        Group::Linear,
        Group::Greek,
        Group::Symbols,
        Group::Helpers,
        Group::Graphs,
        Group::Units,
        Group::Text,
        Group::Document,
    ];

    /// The heading shown in the menu.
    pub fn title(self) -> &'static str {
        match self {
            Group::Calculus => "Calculus",
            Group::Sums => "Sums and products",
            Group::Algebra => "Algebra",
            Group::Linear => "Linear algebra",
            Group::Greek => "Greek",
            Group::Symbols => "Symbols",
            Group::Helpers => "Helpers",
            Group::Graphs => "Graphs",
            Group::Units => "Units",
            Group::Text => "Text",
            Group::Document => "Document",
        }
    }
}

/// What the builders know about where the template goes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuildCx {
    /// The likely variable (`x` unless the current function uses another).
    pub var: String,
    /// Fourier transforms use `k` instead of `ω` (a setting).
    pub fourier_k: bool,
}

impl Default for BuildCx {
    fn default() -> Self {
        BuildCx {
            var: "x".into(),
            fourier_k: false,
        }
    }
}

/// How an item becomes text.
#[derive(Debug, Clone, Copy)]
pub enum Body {
    /// A maths template built from its knobs.
    Math(fn(Knobs, &BuildCx) -> String),
    /// A symbol or letter, inserted as it is (maths text).
    Symbol(&'static str),
    /// A `Figure:` line (a block of its own) built from its knobs.
    Figure(fn(Knobs, &BuildCx) -> String),
    /// A block of prose put in as it is, in a new block (a note, a theorem).
    /// `⟨⟩` marks where the caret goes; without it the caret goes to the end.
    Block(&'static str),
}

/// One item of the Insert menu.
#[derive(Debug, Clone)]
pub struct Template {
    /// Stable id (used for "recently used").
    pub id: String,
    pub groups: Vec<Group>,
    /// The name shown in the menu.
    pub name: String,
    /// A small typeset picture for the menu (LaTeX).
    pub icon: String,
    /// Extra words the search matches (`integral`, `antiderivative` …).
    pub keywords: String,
    pub body: Body,
    pub knobs: Knobs,
    /// Shows the rows × columns picker before inserting.
    pub grid_picker: bool,
    /// Shows the citation picker (the document's reference keys) instead of
    /// inserting a template.
    pub cite_picker: bool,
}

impl Template {
    /// The snippet for these knobs.
    pub fn snippet_source(&self, knobs: Knobs, cx: &BuildCx) -> String {
        match self.body {
            Body::Math(f) | Body::Figure(f) => f(knobs, cx),
            Body::Symbol(s) | Body::Block(s) => s.to_string(),
        }
    }

    /// True for a prose block (Text and Document groups).
    pub fn is_text(&self) -> bool {
        matches!(self.body, Body::Block(_))
    }

    /// True for a `Figure:` line.
    pub fn is_figure(&self) -> bool {
        matches!(self.body, Body::Figure(_))
    }

    /// True for a single symbol or letter.
    pub fn is_symbol(&self) -> bool {
        matches!(self.body, Body::Symbol(_))
    }

    /// True when the search text matches the name, id or keywords.
    pub fn matches(&self, query: &str) -> bool {
        let q = query.trim().to_lowercase();
        if q.is_empty() {
            return true;
        }
        let hay = format!(
            "{} {} {} {}",
            self.name.to_lowercase(),
            self.id.to_lowercase(),
            self.keywords.to_lowercase(),
            self.groups
                .iter()
                .map(|g| g.title().to_lowercase())
                .collect::<Vec<_>>()
                .join(" ")
        );
        q.split_whitespace().all(|w| hay.contains(w))
    }
}

/// Every item, in menu order.
pub fn catalogue() -> &'static [Template] {
    static ALL: OnceLock<Vec<Template>> = OnceLock::new();
    ALL.get_or_init(build_catalogue)
}

/// The item with this id.
pub fn find(id: &str) -> Option<&'static Template> {
    catalogue().iter().find(|t| t.id == id)
}

fn sub(s: &str, cx: &BuildCx) -> String {
    s.replace("$v", &cx.var)
}

macro_rules! fixed {
    ($s:expr) => {{
        fn f(_: Knobs, cx: &BuildCx) -> String {
            sub($s, cx)
        }
        f as fn(Knobs, &BuildCx) -> String
    }};
}

#[allow(clippy::too_many_arguments)]
fn item(
    id: &str,
    groups: &[Group],
    name: &str,
    icon: &str,
    keywords: &str,
    body: Body,
    knobs: Knobs,
) -> Template {
    Template {
        id: id.into(),
        groups: groups.to_vec(),
        name: name.into(),
        icon: icon.into(),
        keywords: keywords.into(),
        body,
        knobs,
        grid_picker: false,
        cite_picker: false,
    }
}

fn k() -> Knobs {
    Knobs::default()
}

// ----- builders with knobs -----

/// ∬ and ∭: region mode (mode 0) or bounds mode (mode 1). For ∭ the variant
/// picks the coordinates: 0 Cartesian, 1 cylindrical, 2 spherical.
fn multiple(knobs: Knobs, triple: bool) -> String {
    let (sign, element) = if triple {
        ("iiint", "V")
    } else {
        ("iint", "A")
    };
    let coords = if triple { knobs.variant.min(2) } else { 0 };
    let (factor, vars): (&str, [&str; 3]) = match coords {
        1 => (r"r \, ", ["z", "r", r"\theta"]),
        2 => (r"\rho^{2} \sin\phi \, ", [r"\rho", r"\phi", r"\theta"]),
        _ => ("", ["z", "y", "x"]),
    };
    if knobs.mode == 0 {
        let diff = if coords == 0 {
            format!(r"d⟨~element={element}⟩")
        } else {
            format!(r"{factor}d{} \, d{} \, d{}", vars[0], vars[1], vars[2])
        };
        return format!(r"\{sign}_{{⟨region⟩}} {{⟨*integrand⟩}} \, {diff}");
    }
    let names: &[&str] = if triple {
        &["outer", "middle", "inner"]
    } else {
        &["outer", "inner"]
    };
    let vars: Vec<&str> = if triple {
        vars.to_vec()
    } else {
        vec!["y", "x"]
    };
    // Bounds, outermost first; differentials innermost first.
    let mut s = String::new();
    for n in names {
        s.push_str(&format!(r"\int_{{⟨{n} lower⟩}}^{{⟨{n} upper⟩}} "));
    }
    s.push_str(r"{⟨*integrand⟩} \, ");
    if triple {
        s.push_str(factor);
    }
    let inner_first: Vec<&str> = names.iter().rev().copied().collect();
    let diffs: Vec<String> = inner_first
        .iter()
        .zip(vars.iter())
        .map(|(n, v)| format!(r"d⟨~{n} variable={v}⟩"))
        .collect();
    s.push_str(&diffs.join(r" \, "));
    s
}

fn double(knobs: Knobs, _: &BuildCx) -> String {
    multiple(knobs, false)
}

fn triple(knobs: Knobs, _: &BuildCx) -> String {
    multiple(knobs, true)
}

/// ∇, ∇·, ∇×, ∇²: the variant picks the coordinates.
fn nabla_vars(knobs: Knobs) -> &'static str {
    match knobs.variant {
        1 => r"r, \theta, z",
        2 => r"\rho, \theta, \phi",
        _ => "x, y, z",
    }
}

fn grad(knobs: Knobs, _: &BuildCx) -> String {
    let v = nabla_vars(knobs);
    format!(r"\nabla_{{⟨~variables={v}@1⟩}} {{⟨*field@0⟩}}")
}

fn div(knobs: Knobs, _: &BuildCx) -> String {
    let v = nabla_vars(knobs);
    format!(r"\nabla_{{⟨~variables={v}@1⟩}} \cdot {{⟨*field@0⟩}}")
}

fn curl(knobs: Knobs, _: &BuildCx) -> String {
    let v = nabla_vars(knobs);
    format!(r"\nabla_{{⟨~variables={v}@1⟩}} \times {{⟨*field@0⟩}}")
}

fn laplacian(knobs: Knobs, _: &BuildCx) -> String {
    let v = nabla_vars(knobs);
    format!(r"\nabla^{{2}}_{{⟨~variables={v}@1⟩}} {{⟨*field@0⟩}}")
}

/// lim: the variant picks the side (0 both, 1 from above, 2 from below).
fn limit(knobs: Knobs, cx: &BuildCx) -> String {
    let side = match knobs.variant {
        1 => "^{+}",
        2 => "^{-}",
        _ => "",
    };
    format!(
        r"\lim_{{⟨~variable={v}⟩ \to {{⟨value⟩}}{side}}} {{⟨*expression⟩}}",
        v = cx.var
    )
}

/// ∂: one row per variable (mixed partials).
fn partial(knobs: Knobs, cx: &BuildCx) -> String {
    let n = knobs.rows.clamp(1, 6) as usize;
    let defaults = [cx.var.as_str(), "y", "z", "x", "y", "z"];
    let vars: Vec<String> = (0..n)
        .map(|i| {
            format!(
                r"\partial ⟨~variable={}#variable{i}@{}⟩",
                defaults[i],
                i + 1
            )
        })
        .collect();
    let top = if n == 1 {
        r"\partial".to_string()
    } else {
        format!(r"\partial^{{{n}}}")
    };
    format!(
        r"\frac{{{top}}}{{{}}} {{⟨*function@0⟩}}",
        vars.join(r" \, ")
    )
}

/// ODE: the rows are initial conditions.
fn ode(knobs: Knobs, _: &BuildCx) -> String {
    let mut s =
        r"\operatorname{solve}_{⟨~unknown function=y@1#unknown⟩(⟨~variable=x@2⟩)} \left( {⟨*equation@0⟩} \right)"
            .to_string();
    for i in 0..knobs.rows.min(6) as usize {
        let primes = "'".repeat(i);
        s.push_str(&format!(
            r", \quad ⟨^unknown=y⟩{primes}({{⟨point#point{i}⟩}}) = {{⟨value#value{i}⟩}}"
        ));
    }
    s
}

/// A system of ODEs: rows are equations, columns are conditions.
fn ode_system(knobs: Knobs, _: &BuildCx) -> String {
    let n = knobs.rows.clamp(1, 6) as usize;
    let eqs: Vec<String> = (0..n)
        .map(|i| {
            let star = if i == 0 { "*" } else { "" };
            format!(r"{{⟨{star}equation#equation{i}@{i}⟩}}")
        })
        .collect();
    let mut s = format!(
        r"\operatorname{{solve}}_{{⟨~unknowns=x, y@{n}⟩}} \left( \begin{{cases}} {} \end{{cases}} \right)",
        eqs.join(r" \\ ")
    );
    for j in 0..knobs.cols.min(6) as usize {
        s.push_str(&format!(
            r", \quad {{⟨condition#condition{j}@{}⟩}}",
            n + 1 + j
        ));
    }
    s
}

fn fourier(_: Knobs, cx: &BuildCx) -> String {
    let w = if cx.fourier_k { "k" } else { r"\omega" };
    format!(
        r"\mathcal{{F}}_{{{{⟨variable={v}@1⟩}} \to {{⟨frequency={w}@2⟩}}}}\left\{{ {{⟨*function@0⟩}} \right\}}",
        v = cx.var
    )
}

fn fourier_inv(_: Knobs, cx: &BuildCx) -> String {
    let w = if cx.fourier_k { "k" } else { r"\omega" };
    format!(
        r"\mathcal{{F}}^{{-1}}_{{{{⟨frequency={w}@1⟩}} \to {{⟨variable={v}@2⟩}}}}\left\{{ {{⟨*function@0⟩}} \right\}}",
        v = cx.var
    )
}

/// log: mode 0 natural (ln), 1 base 10, 2 any base.
fn log(knobs: Knobs, _: &BuildCx) -> String {
    match knobs.mode {
        1 => r"\log_{10}\left( {⟨*argument⟩} \right)".into(),
        2 => r"\log_{⟨base⟩}\left( {⟨*argument⟩} \right)".into(),
        _ => r"\ln\left( {⟨*argument⟩} \right)".into(),
    }
}

/// Piecewise: `rows` value/condition rows, plus an "otherwise" row when `flag`.
fn piecewise(knobs: Knobs, _: &BuildCx) -> String {
    let n = knobs.rows.clamp(1, 12) as usize;
    let mut rows: Vec<String> = (0..n)
        .map(|i| {
            let star = if i == 0 { "*" } else { "" };
            format!(r"{{⟨{star}value#value{i}⟩}} & {{⟨condition#condition{i}⟩}}")
        })
        .collect();
    if knobs.flag {
        rows.push(r"{⟨value#otherwise⟩} & \text{otherwise}".into());
    }
    format!(r"\begin{{cases}} {} \end{{cases}}", rows.join(r" \\ "))
}

/// A matrix (or a column vector) with one empty box per entry.
fn matrix(knobs: Knobs, _: &BuildCx) -> String {
    let (r, c) = (knobs.rows.clamp(1, 12), knobs.cols.clamp(1, 12));
    let rows: Vec<String> = (1..=r)
        .map(|i| {
            (1..=c)
                .map(|j| format!(r"{{⟨#a{i},{j}⟩}}"))
                .collect::<Vec<_>>()
                .join(" & ")
        })
        .collect();
    format!(r"\begin{{pmatrix}} {} \end{{pmatrix}}", rows.join(r" \\ "))
}

/// A function definition with `rows` arguments.
fn function_def(knobs: Knobs, cx: &BuildCx) -> String {
    let n = knobs.rows.clamp(1, 6) as usize;
    let defaults = [cx.var.as_str(), "y", "z", "t", "u", "v"];
    let args: Vec<String> = (0..n)
        .map(|i| format!("⟨~argument={}#argument{i}⟩", defaults[i]))
        .collect();
    format!(r"⟨~name=f⟩({}) := {{⟨*body⟩}}", args.join(", "))
}

/// Lagrange multipliers: one row per constraint.
fn lagrange(knobs: Knobs, _: &BuildCx) -> String {
    let n = knobs.rows.clamp(1, 6) as usize;
    let cons: Vec<String> = (0..n)
        .map(|i| format!(r"{{⟨constraint#constraint{i}⟩}}"))
        .collect();
    format!(
        r"\operatorname{{lagrange}}\left( {{⟨*function⟩}}, {} \right)",
        cons.join(", ")
    )
}

/// Graphs: rows are curves (or surfaces), columns are sliders.
fn graph_sliders(knobs: Knobs) -> String {
    (0..knobs.cols.min(6) as usize)
        .map(|j| {
            let name = ["a", "b", "c", "k", "m", "n"][j];
            format!(
                r", $⟨~slider name={name}#slider{j}⟩ \in [{{⟨min=0#min{j}⟩}}, {{⟨max=1#max{j}⟩}}]$"
            )
        })
        .collect()
}

fn graph_curves(knobs: Knobs) -> String {
    (0..knobs.rows.clamp(1, 6) as usize)
        .map(|i| {
            let star = if i == 0 { "*" } else { "" };
            format!(r"${{⟨{star}expression#expression{i}⟩}}$")
        })
        .collect::<Vec<_>>()
        .join(", ")
}

fn range(var: &str) -> String {
    format!(r"${var} \in [{{⟨from=-10#{var}from⟩}}, {{⟨to=10#{var}to⟩}}]$")
}

fn graph_2d(knobs: Knobs, _: &BuildCx) -> String {
    format!(
        "Figure: {}, {}{}.",
        graph_curves(knobs),
        range("x"),
        graph_sliders(knobs)
    )
}

fn graph_3d(knobs: Knobs, _: &BuildCx) -> String {
    format!(
        "Figure: {}, {}, {}{}.",
        graph_curves(knobs),
        range("x"),
        range("y"),
        graph_sliders(knobs)
    )
}

fn graph_param(knobs: Knobs, _: &BuildCx) -> String {
    format!(
        r"Figure: parametric $({{⟨*x(t)⟩}}, {{⟨y(t)⟩}})$, {}{}.",
        range("t"),
        graph_sliders(knobs)
    )
}

fn graph_slope(knobs: Knobs, _: &BuildCx) -> String {
    format!(
        r"Figure: slope field ${{⟨*equation⟩}}$, {}, {}{}.",
        range("x"),
        range("y"),
        graph_sliders(knobs)
    )
}

// ----- graph helpers: one figure keyword each -----

fn graph_h_tangent(_: Knobs, _: &BuildCx) -> String {
    let mut s = String::from(r"Figure: ${⟨*function⟩}$, tangent at $x = {⟨*point#point⟩}$");
    s.push_str(", ");
    s.push_str(&range("x"));
    s.push('.');
    s
}

fn graph_h_riemann(_: Knobs, _: &BuildCx) -> String {
    let mut s = String::from(r"Figure: ${⟨*function⟩}$, riemann mid $n = {⟨n=8#n⟩}$");
    s.push_str(", ");
    s.push_str(&range("x"));
    s.push('.');
    s
}

fn graph_h_trapezoid(_: Knobs, _: &BuildCx) -> String {
    let mut s = String::from(r"Figure: ${⟨*function⟩}$, riemann trapezoid $n = {⟨n=8#n⟩}$");
    s.push_str(", ");
    s.push_str(&range("x"));
    s.push('.');
    s
}

fn graph_h_area(_: Knobs, _: &BuildCx) -> String {
    let mut s = String::from(r"Figure: ${⟨*top#top⟩}$, ${⟨*bottom#bottom⟩}$, area between");
    s.push_str(", ");
    s.push_str(&range("x"));
    s.push('.');
    s
}

fn graph_h_revolve(_: Knobs, _: &BuildCx) -> String {
    let mut s = String::from(r"Figure: ${⟨*function⟩}$, revolve about $y = {⟨axis=0#axis⟩}$");
    s.push_str(", ");
    s.push_str(&range("x"));
    s.push('.');
    s
}

fn graph_h_vector(_: Knobs, _: &BuildCx) -> String {
    let mut s = String::from(r"Figure: vector field $({⟨*P#P⟩}, {⟨*Q#Q⟩})$");
    s.push_str(", ");
    s.push_str(&range("x"));
    s.push_str(", ");
    s.push_str(&range("y"));
    s.push('.');
    s
}

fn graph_h_gradient(_: Knobs, _: &BuildCx) -> String {
    let mut s = String::from(r"Figure: gradient field ${⟨*function⟩}$");
    s.push_str(", ");
    s.push_str(&range("x"));
    s.push_str(", ");
    s.push_str(&range("y"));
    s.push('.');
    s
}

fn graph_h_contour(_: Knobs, _: &BuildCx) -> String {
    let mut s = String::from(r"Figure: ${⟨*function⟩}$, contour");
    s.push_str(", ");
    s.push_str(&range("x"));
    s.push_str(", ");
    s.push_str(&range("y"));
    s.push('.');
    s
}

fn graph_h_tplane(_: Knobs, _: &BuildCx) -> String {
    let mut s =
        String::from(r"Figure: $z = {⟨*function⟩}$, tangent plane at $({⟨*x0#x0⟩}, {⟨*y0#y0⟩})$");
    s.push_str(", ");
    s.push_str(&range("x"));
    s.push_str(", ");
    s.push_str(&range("y"));
    s.push('.');
    s
}

fn graph_h_taylor(_: Knobs, _: &BuildCx) -> String {
    let mut s = String::from(r"Figure: ${⟨*function⟩}$, taylor ${⟨degree=5#n⟩}$");
    s.push_str(", ");
    s.push_str(&range("x"));
    s.push('.');
    s
}

fn graph_h_newton(_: Knobs, _: &BuildCx) -> String {
    let mut s = String::from(r"Figure: ${⟨*function⟩}$, newton from $x_0 = {⟨*start#start⟩}$");
    s.push_str(", ");
    s.push_str(&range("x"));
    s.push('.');
    s
}

fn graph_h_scatter(_: Knobs, _: &BuildCx) -> String {
    let mut s = String::from(r"Figure: scatter ${⟨*x column#xcol⟩}, {⟨*y column#ycol⟩}$");
    s.push('.');
    s
}

fn graph_h_histogram(_: Knobs, _: &BuildCx) -> String {
    let mut s = String::from(r"Figure: histogram ${⟨*column#col⟩}$");
    s.push('.');
    s
}

fn graph_h_boxplot(_: Knobs, _: &BuildCx) -> String {
    let mut s = String::from(r"Figure: box plot ${⟨*column#col⟩}$");
    s.push('.');
    s
}

// ----- helpers (C2–C4) -----

/// Where a helper argument goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Pos {
    /// Inside the brackets (or the operand when it is the only one).
    Main,
    /// Under the operator name (`\operatorname{crit}_{[0, 4]}`).
    Sub,
    /// Over it.
    Sup,
}

/// A helper argument: label, position, default, required.
type Arg = (&'static str, Pos, &'static str, bool);

/// The helper's operator form, with every argument a box (in the listed Tab
/// order). Optional arguments under or over the name are dropped when empty.
fn helper_snippet(op: &str, args: &[Arg]) -> String {
    let boxed = |i: usize, a: &Arg| {
        let (label, _, default, required) = *a;
        let opt = if required { "" } else { "?" };
        let star = if i == 0 { "*" } else { "" };
        let def = if default.is_empty() {
            String::new()
        } else {
            format!("={default}")
        };
        format!("{{⟨{opt}{star}{label}{def}@{i}⟩}}")
    };
    let mut s = format!(r"\operatorname{{{op}}}");
    let scripts: Vec<(usize, &Arg)> = args
        .iter()
        .enumerate()
        .filter(|(_, a)| a.1 != Pos::Main)
        .collect();
    let all_optional = !scripts.is_empty() && scripts.iter().all(|(_, a)| !a.3);
    if all_optional {
        s.push('⟦');
    }
    for (i, a) in &scripts {
        let mark = if a.1 == Pos::Sub { '_' } else { '^' };
        s.push(mark);
        s.push_str(&boxed(*i, a));
    }
    if all_optional {
        s.push('⟧');
    }
    let main: Vec<String> = args
        .iter()
        .enumerate()
        .filter(|(_, a)| a.1 == Pos::Main)
        .map(|(i, a)| boxed(i, a))
        .collect();
    if main.len() == 1 {
        s.push(' ');
        s.push_str(&main[0]);
    } else {
        s.push_str(&format!(r"\left( {} \right)", main.join(", ")));
    }
    s
}

const F: Arg = ("function", Pos::Main, "", true);

/// id, menu name, operator, keywords, arguments.
type HelperSpec = (
    &'static str,
    &'static str,
    &'static str,
    &'static str,
    &'static [Arg],
);

const HELPERS: &[HelperSpec] = &[
    ("domain", "Domain", "domain", "where defined", &[F]),
    ("range", "Range", "range", "values", &[F]),
    (
        "intercepts",
        "Intercepts",
        "intercepts",
        "zeros roots axis",
        &[F],
    ),
    (
        "crit",
        "Critical points",
        "crit",
        "stationary derivative zero",
        &[F, ("interval", Pos::Sub, "", false)],
    ),
    (
        "extrema",
        "Extrema",
        "extrema",
        "maximum minimum max min local absolute",
        &[F, ("interval", Pos::Sub, "", false)],
    ),
    (
        "increasing",
        "Increasing and decreasing",
        "increasing",
        "monotonic intervals of increase decrease",
        &[F, ("interval", Pos::Sub, "", false)],
    ),
    (
        "concavity",
        "Concavity",
        "concavity",
        "concave up down",
        &[F, ("interval", Pos::Sub, "", false)],
    ),
    (
        "inflection",
        "Inflection points",
        "inflection",
        "second derivative",
        &[F],
    ),
    (
        "asymptotes",
        "Asymptotes",
        "asymptotes",
        "vertical horizontal slant oblique",
        &[F],
    ),
    (
        "tangent",
        "Tangent line",
        "tangent",
        "slope line at point",
        &[F, ("point", Pos::Sub, "", true)],
    ),
    (
        "normal",
        "Normal line",
        "normal",
        "perpendicular line at point",
        &[F, ("point", Pos::Sub, "", true)],
    ),
    (
        "linearise",
        "Linearisation",
        "linearise",
        "linearization linear approximation L(x)",
        &[F, ("point", Pos::Sub, "", true)],
    ),
    (
        "mvt",
        "Mean value theorem",
        "mvt",
        "mean value theorem check",
        &[F, ("interval", Pos::Sub, "", true)],
    ),
    (
        "ivt",
        "Intermediate value theorem",
        "ivt",
        "intermediate value theorem check",
        &[F, ("interval", Pos::Sub, "", true)],
    ),
    (
        "analyse",
        "Analyse a function",
        "analyse",
        "analyze curve sketch summary table",
        &[F, ("interval", Pos::Sub, "", false)],
    ),
    (
        "area",
        "Area between curves",
        "area",
        "region between two curves",
        &[
            ("top curve", Pos::Main, "", true),
            ("bottom curve", Pos::Main, "", true),
            ("from", Pos::Sub, "", false),
            ("to", Pos::Sup, "", false),
        ],
    ),
    (
        "volume",
        "Volume of revolution",
        "volume",
        "solid revolve disk washer shell",
        &[
            ("function", Pos::Main, "", true),
            ("axis", Pos::Main, "y = 0", true),
            ("method", Pos::Main, r"\text{auto}", true),
            ("from", Pos::Sub, "", true),
            ("to", Pos::Sup, "", true),
        ],
    ),
    (
        "arclength",
        "Arc length",
        "arclength",
        "length of a curve",
        &[F, ("from", Pos::Sub, "", true), ("to", Pos::Sup, "", true)],
    ),
    (
        "surfacearea",
        "Surface area of revolution",
        "surfacearea",
        "surface revolve",
        &[
            F,
            ("axis", Pos::Main, "y = 0", true),
            ("from", Pos::Sub, "", true),
            ("to", Pos::Sup, "", true),
        ],
    ),
    (
        "average",
        "Average value",
        "average",
        "mean value of a function",
        &[F, ("from", Pos::Sub, "", true), ("to", Pos::Sup, "", true)],
    ),
    (
        "work",
        "Work",
        "work",
        "force distance physics",
        &[
            ("force", Pos::Main, "", true),
            ("from", Pos::Sub, "", true),
            ("to", Pos::Sup, "", true),
        ],
    ),
    (
        "centroid",
        "Centre of mass",
        "centroid",
        "center of mass centroid moment",
        &[
            ("density", Pos::Main, "", true),
            ("from", Pos::Sub, "", true),
            ("to", Pos::Sup, "", true),
        ],
    ),
    (
        "accumulate",
        "Accumulation function",
        "accumulate",
        "F(x) integral from a to x",
        &[F, ("start", Pos::Sub, "", true)],
    ),
    (
        "riemann",
        "Riemann sum",
        "riemann",
        "left right midpoint trapezoid simpson numerical integration",
        &[
            ("function", Pos::Main, "", true),
            ("from", Pos::Sub, "", true),
            ("to", Pos::Sup, "", true),
            ("n", Pos::Main, "4", true),
            ("rule", Pos::Main, r"\text{mid}", true),
        ],
    ),
    (
        "newton",
        "Newton's method",
        "newton",
        "root finding iteration",
        &[
            ("function", Pos::Main, "", true),
            ("starting x", Pos::Main, "", true),
            ("iterations", Pos::Main, "5", true),
        ],
    ),
    (
        "euler",
        "Euler's method",
        "euler",
        "numerical ode step",
        &[
            ("equation", Pos::Main, "", true),
            ("initial condition", Pos::Main, "", true),
            ("step", Pos::Main, "0.1", true),
            ("steps", Pos::Main, "10", true),
        ],
    ),
    (
        "rk4",
        "Runge–Kutta (RK4)",
        "rk4",
        "runge kutta numerical ode",
        &[
            ("equation", Pos::Main, "", true),
            ("initial condition", Pos::Main, "", true),
            ("step", Pos::Main, "0.1", true),
            ("steps", Pos::Main, "10", true),
        ],
    ),
    (
        "taylorbound",
        "Taylor error bound",
        "taylorbound",
        "lagrange remainder error terms",
        &[
            F,
            ("centre", Pos::Main, "0", true),
            ("order", Pos::Main, "5", true),
            ("at", Pos::Main, "", true),
        ],
    ),
    (
        "converges",
        "Convergence test",
        "converges",
        "series ratio root comparison test",
        &[("series", Pos::Main, "", true)],
    ),
    (
        "radius",
        "Radius of convergence",
        "radius",
        "interval of convergence power series",
        &[("series", Pos::Main, "", true)],
    ),
    (
        "directional",
        "Directional derivative",
        "directional",
        "gradient direction",
        &[
            F,
            ("point", Pos::Main, "", true),
            ("direction", Pos::Main, "", true),
        ],
    ),
    (
        "tangentplane",
        "Tangent plane",
        "tangentplane",
        "plane surface point",
        &[F, ("point", Pos::Sub, "", true)],
    ),
    (
        "linearapprox",
        "Linear approximation (2 variables)",
        "linearapprox",
        "linearisation two variables",
        &[F, ("point", Pos::Sub, "", true)],
    ),
    (
        "classify",
        "Second-derivative test",
        "classify",
        "saddle points extrema hessian",
        &[F],
    ),
    (
        "changevars",
        "Change of variables",
        "changevars",
        "substitution jacobian polar",
        &[
            ("integral", Pos::Main, "", true),
            ("substitution", Pos::Main, "", true),
        ],
    ),
    (
        "green",
        "Green's theorem",
        "green",
        "check both sides curve",
        &[
            ("field", Pos::Main, "", true),
            ("curve", Pos::Main, "C", true),
        ],
    ),
    (
        "stokes",
        "Stokes' theorem",
        "stokes",
        "check both sides surface",
        &[
            ("field", Pos::Main, "", true),
            ("surface", Pos::Main, "S", true),
        ],
    ),
    (
        "gauss",
        "Divergence theorem",
        "gauss",
        "check both sides solid flux",
        &[
            ("field", Pos::Main, "", true),
            ("solid", Pos::Main, "E", true),
        ],
    ),
    (
        "implicit",
        "Implicit differentiation",
        "implicit",
        "dy/dx relation",
        &[
            ("relation", Pos::Main, "", true),
            ("variable", Pos::Sub, "x", true),
        ],
    ),
    (
        "relatedrates",
        "Related rates",
        "relatedrates",
        "d/dt relation time",
        &[
            ("relation", Pos::Main, "", true),
            ("time variable", Pos::Sub, "t", true),
        ],
    ),
];

fn helper_body(op: &str) -> fn(Knobs, &BuildCx) -> String {
    // One builder per helper: look the spec up by operator at build time.
    fn build(op: &str) -> String {
        HELPERS
            .iter()
            .find(|h| h.2 == op)
            .map(|h| helper_snippet(h.2, h.4))
            .unwrap_or_default()
    }
    macro_rules! table {
        ($($name:literal),* $(,)?) => {
            match op {
                $($name => {
                    fn f(_: Knobs, _: &BuildCx) -> String { build($name) }
                    f
                })*
                _ => {
                    fn f(_: Knobs, _: &BuildCx) -> String { String::new() }
                    f
                }
            }
        };
    }
    table!(
        "domain",
        "range",
        "intercepts",
        "crit",
        "extrema",
        "increasing",
        "concavity",
        "inflection",
        "asymptotes",
        "tangent",
        "normal",
        "linearise",
        "mvt",
        "ivt",
        "analyse",
        "area",
        "volume",
        "arclength",
        "surfacearea",
        "average",
        "work",
        "centroid",
        "accumulate",
        "riemann",
        "newton",
        "euler",
        "rk4",
        "taylorbound",
        "converges",
        "radius",
        "directional",
        "tangentplane",
        "linearapprox",
        "classify",
        "changevars",
        "green",
        "stokes",
        "gauss",
        "implicit",
        "relatedrates",
    )
}

fn build_catalogue() -> Vec<Template> {
    use Group::*;
    let math = Body::Math;
    let mut v = vec![
        // ----- calculus -----
        item(
            "int.indef",
            &[Calculus],
            "Integral (indefinite)",
            r"\int f \, dx",
            "antiderivative integral",
            math(fixed!(r"\int {⟨*integrand⟩} \, d⟨~variable=$v⟩")),
            k(),
        ),
        item(
            "int.def",
            &[Calculus],
            "Integral (definite)",
            r"\int_a^b f \, dx",
            "integral bounds limits area",
            math(fixed!(
                r"\int⟦_{⟨lower⟩}^{⟨upper⟩}⟧ {⟨*integrand⟩} \, d⟨~variable=$v⟩"
            )),
            k(),
        ),
        item(
            "int.improper",
            &[Calculus],
            "Improper integral",
            r"\int_0^\infty f \, dx",
            "infinity integral",
            math(fixed!(
                r"\int_{⟨lower⟩}^{⟨upper⟩} {⟨*integrand⟩} \, d⟨~variable=$v⟩"
            )),
            k(),
        ),
        item(
            "int.double",
            &[Calculus],
            "Double integral",
            r"\iint_D f \, dA",
            "double integral area region iint",
            math(double),
            k(),
        ),
        item(
            "int.triple",
            &[Calculus],
            "Triple integral",
            r"\iiint_E f \, dV",
            "triple integral volume region iiint cylindrical spherical",
            math(triple),
            k(),
        ),
        item(
            "int.line",
            &[Calculus],
            "Line integral",
            r"\oint_C \mathbf{F} \cdot d\mathbf{r}",
            "closed contour circulation oint",
            math(fixed!(
                r"\oint_{⟨curve=C⟩} {⟨*field⟩} \cdot d⟨~differential=\mathbf{r}⟩"
            )),
            k(),
        ),
        item(
            "int.surface",
            &[Calculus],
            "Surface integral",
            r"\oiint_S \mathbf{F} \cdot d\mathbf{S}",
            "closed surface flux oiint",
            math(fixed!(
                r"\oiint_{⟨surface=S⟩} {⟨*field⟩} \cdot d⟨~differential=\mathbf{S}⟩"
            )),
            k(),
        ),
        item(
            "deriv",
            &[Calculus],
            "Derivative",
            r"\frac{d}{dx} f",
            "derivative differentiate d/dx",
            math(fixed!(
                r"\frac{d}{d⟨~variable=$v@1⟩} {⟨*function@0⟩}⟦, \quad ⟨^variable=$v⟩ = {⟨at@2⟩}⟧"
            )),
            k(),
        ),
        item(
            "deriv.n",
            &[Calculus],
            "Higher derivative",
            r"\frac{d^n}{dx^n} f",
            "second derivative nth order",
            math(fixed!(
                r"\frac{d^{⟨order=2@0⟩}}{d⟨~variable=$v@2⟩^{⟨^order=2⟩}} {⟨*function@1⟩}⟦, \quad ⟨^variable=$v⟩ = {⟨at@3⟩}⟧"
            )),
            k(),
        ),
        item(
            "partial",
            &[Calculus],
            "Partial derivative",
            r"\frac{\partial}{\partial x} f",
            "partial mixed",
            math(partial),
            Knobs { rows: 1, ..k() },
        ),
        item(
            "grad",
            &[Calculus],
            "Gradient",
            r"\nabla f",
            "nabla gradient grad",
            math(grad),
            k(),
        ),
        item(
            "div",
            &[Calculus],
            "Divergence",
            r"\nabla \cdot \mathbf{F}",
            "nabla divergence div",
            math(div),
            k(),
        ),
        item(
            "curl",
            &[Calculus],
            "Curl",
            r"\nabla \times \mathbf{F}",
            "nabla curl rotation",
            math(curl),
            k(),
        ),
        item(
            "laplacian",
            &[Calculus],
            "Laplacian",
            r"\nabla^2 f",
            "nabla laplacian laplace operator",
            math(laplacian),
            k(),
        ),
        item(
            "lim",
            &[Calculus],
            "Limit",
            r"\lim_{x \to a} f",
            "limit approaches",
            math(limit),
            k(),
        ),
        item(
            "lim.side",
            &[Calculus],
            "Limit (one-sided)",
            r"\lim_{x \to a^{+}} f",
            "one sided limit right left",
            math(limit),
            Knobs { variant: 1, ..k() },
        ),
        item(
            "taylor",
            &[Calculus, Sums],
            "Taylor series",
            r"\operatorname{series}_{x=a} f",
            "taylor maclaurin power series expansion",
            math(fixed!(
                r"\operatorname{series}_{{⟨variable=$v@3⟩}={⟨centre=0@1⟩}}^{{⟨order=5@2⟩}} {⟨*function@0⟩}"
            )),
            k(),
        ),
        item(
            "ode",
            &[Calculus],
            "Differential equation",
            r"y' = f(x, y)",
            "ode differential equation solve",
            math(ode),
            k(),
        ),
        item(
            "ode.ivp",
            &[Calculus],
            "Differential equation with initial conditions",
            r"y(0) = y_0",
            "ode initial value problem ivp conditions",
            math(ode),
            Knobs { rows: 1, ..k() },
        ),
        item(
            "ode.system",
            &[Calculus],
            "System of differential equations",
            r"\mathbf{x}' = A\mathbf{x}",
            "ode system linear",
            math(ode_system),
            Knobs { rows: 2, ..k() },
        ),
        item(
            "laplace",
            &[Calculus],
            "Laplace transform",
            r"\mathcal{L}\{f\}",
            "laplace transform",
            math(fixed!(
                r"\mathcal{L}_{{⟨time=t@1⟩} \to {⟨transform=s@2⟩}}\left\{ {⟨*function@0⟩} \right\}"
            )),
            k(),
        ),
        item(
            "laplace.inv",
            &[Calculus],
            "Inverse Laplace transform",
            r"\mathcal{L}^{-1}\{F\}",
            "inverse laplace transform",
            math(fixed!(
                r"\mathcal{L}^{-1}_{{⟨transform=s@1⟩} \to {⟨time=t@2⟩}}\left\{ {⟨*function@0⟩} \right\}"
            )),
            k(),
        ),
        item(
            "fourier",
            &[Calculus],
            "Fourier transform",
            r"\mathcal{F}\{f\}",
            "fourier transform frequency",
            math(fourier),
            k(),
        ),
        item(
            "fourier.inv",
            &[Calculus],
            "Inverse Fourier transform",
            r"\mathcal{F}^{-1}\{F\}",
            "inverse fourier transform",
            math(fourier_inv),
            k(),
        ),
        item(
            "jacobian",
            &[Calculus],
            "Jacobian",
            r"\frac{\partial(u, v)}{\partial(x, y)}",
            "jacobian determinant change of variables",
            math(fixed!(r"\frac{\partial(⟨~outputs⟩)}{\partial(⟨~inputs⟩)}")),
            k(),
        ),
        // ----- sums and products -----
        item(
            "sum",
            &[Sums],
            "Sum",
            r"\sum_{n=1}^{\infty} a_n",
            "sum sigma series summation",
            math(fixed!(
                r"\sum_{⟨~index=n⟩={⟨start=1⟩}}^{⟨end=\infty⟩} {⟨*term⟩}"
            )),
            k(),
        ),
        item(
            "prod",
            &[Sums],
            "Product",
            r"\prod_{n=1}^{\infty} a_n",
            "product pi",
            math(fixed!(
                r"\prod_{⟨~index=n⟩={⟨start=1⟩}}^{⟨end=\infty⟩} {⟨*term⟩}"
            )),
            k(),
        ),
        // ----- algebra -----
        item(
            "frac",
            &[Algebra],
            "Fraction",
            r"\frac{a}{b}",
            "fraction divide over",
            math(fixed!(r"\frac{⟨*numerator⟩}{⟨denominator⟩}")),
            k(),
        ),
        item(
            "sqrt",
            &[Algebra],
            "Square root",
            r"\sqrt{x}",
            "square root radical",
            math(fixed!(r"\sqrt{⟨*radicand⟩}")),
            k(),
        ),
        item(
            "root",
            &[Algebra],
            "nth root",
            r"\sqrt[n]{x}",
            "nth root cube root radical",
            math(fixed!(r"\sqrt⟦[⟨index⟩]⟧{⟨radicand⟩}")),
            k(),
        ),
        item(
            "pow",
            &[Algebra],
            "Power",
            r"x^n",
            "power exponent superscript squared",
            math(fixed!(r"{⟨*base⟩}^{⟨exponent⟩}")),
            k(),
        ),
        item(
            "subscript",
            &[Algebra],
            "Subscript",
            r"x_n",
            "subscript index",
            math(fixed!(r"{⟨*base⟩}_{⟨subscript⟩}")),
            k(),
        ),
        item(
            "ln",
            &[Algebra],
            "Natural logarithm",
            r"\ln x",
            "log logarithm natural ln",
            math(log),
            k(),
        ),
        item(
            "log10",
            &[Algebra],
            "Logarithm base 10",
            r"\log_{10} x",
            "log logarithm common",
            math(log),
            Knobs { mode: 1, ..k() },
        ),
        item(
            "logb",
            &[Algebra],
            "Logarithm (any base)",
            r"\log_b x",
            "log logarithm base",
            math(log),
            Knobs { mode: 2, ..k() },
        ),
        item(
            "abs",
            &[Algebra],
            "Absolute value",
            r"|x|",
            "absolute value modulus",
            math(fixed!(r"\left| {⟨*argument⟩} \right|")),
            k(),
        ),
        item(
            "floor",
            &[Algebra],
            "Floor",
            r"\lfloor x \rfloor",
            "floor round down",
            math(fixed!(r"\left\lfloor {⟨*argument⟩} \right\rfloor")),
            k(),
        ),
        item(
            "ceil",
            &[Algebra],
            "Ceiling",
            r"\lceil x \rceil",
            "ceiling round up",
            math(fixed!(r"\left\lceil {⟨*argument⟩} \right\rceil")),
            k(),
        ),
        item(
            "factorial",
            &[Algebra],
            "Factorial",
            r"n!",
            "factorial",
            math(fixed!(r"{⟨*n⟩}!")),
            k(),
        ),
        item(
            "binom",
            &[Algebra],
            "Binomial coefficient",
            r"\binom{n}{k}",
            "binomial choose combination",
            math(fixed!(r"\binom{⟨*n⟩}{⟨k⟩}")),
            k(),
        ),
        item(
            "cases",
            &[Algebra],
            "Piecewise",
            r"\begin{cases} a & x < 0 \\ b & x \ge 0 \end{cases}",
            "piecewise cases conditions otherwise",
            math(piecewise),
            Knobs {
                rows: 1,
                flag: true,
                ..k()
            },
        ),
        item(
            "fn",
            &[Algebra],
            "Function definition",
            r"f(x) := x^2",
            "function define definition",
            math(function_def),
            Knobs { rows: 1, ..k() },
        ),
        // ----- linear algebra -----
        Template {
            grid_picker: true,
            ..item(
                "matrix",
                &[Linear],
                "Matrix",
                r"\begin{pmatrix} a & b \\ c & d \end{pmatrix}",
                "matrix grid rows columns",
                math(matrix),
                Knobs {
                    rows: 2,
                    cols: 2,
                    ..k()
                },
            )
        },
        item(
            "vector",
            &[Linear],
            "Vector",
            r"\begin{pmatrix} x \\ y \\ z \end{pmatrix}",
            "vector column matrix",
            math(matrix),
            Knobs {
                rows: 3,
                cols: 1,
                ..k()
            },
        ),
        item(
            "det",
            &[Linear],
            "Determinant",
            r"\det A",
            "determinant det matrix",
            math(fixed!(r"\det\left( {⟨*matrix⟩} \right)")),
            k(),
        ),
        item(
            "inverse",
            &[Linear],
            "Inverse",
            r"A^{-1}",
            "inverse matrix",
            math(fixed!(r"{⟨*matrix⟩}^{-1}")),
            k(),
        ),
        item(
            "transpose",
            &[Linear],
            "Transpose",
            r"A^{T}",
            "transpose matrix",
            math(fixed!(r"{⟨*matrix⟩}^{T}")),
            k(),
        ),
        item(
            "eigen",
            &[Linear],
            "Eigenvalues",
            r"\operatorname{eig} A",
            "eigenvalues eigenvectors eigen matrix",
            math(fixed!(r"\operatorname{eig}\left( {⟨*matrix⟩} \right)")),
            k(),
        ),
    ];
    // ----- helpers -----
    for (id, name, op, keywords, _) in HELPERS {
        let mut t = item(
            &format!("helper.{id}"),
            &[Helpers],
            name,
            &format!(r"\operatorname{{{op}}} f"),
            keywords,
            Body::Math(helper_body(op)),
            k(),
        );
        if *id == "lagrange" {
            t.body = Body::Math(lagrange);
        }
        v.push(t);
    }
    v.push(item(
        "helper.lagrange",
        &[Helpers],
        "Lagrange multipliers",
        r"\operatorname{lagrange} f",
        "constrained optimisation optimization constraint",
        Body::Math(lagrange),
        Knobs { rows: 1, ..k() },
    ));
    // ----- graphs -----
    v.extend([
        item(
            "graph.2d",
            &[Graphs],
            "Graph (2D)",
            r"y = f(x)",
            "plot graph curve figure 2d",
            Body::Figure(graph_2d),
            Knobs { rows: 1, ..k() },
        ),
        item(
            "graph.3d",
            &[Graphs],
            "Surface (3D)",
            r"z = f(x, y)",
            "plot graph surface figure 3d",
            Body::Figure(graph_3d),
            Knobs { rows: 1, ..k() },
        ),
        item(
            "graph.param",
            &[Graphs],
            "Parametric curve",
            r"(x(t), y(t))",
            "plot graph parametric figure",
            Body::Figure(graph_param),
            k(),
        ),
        item(
            "graph.slope",
            &[Graphs],
            "Slope field",
            r"y' = f(x, y)",
            "plot direction field ode figure",
            Body::Figure(graph_slope),
            k(),
        ),
        item(
            "graph.tangent",
            &[Graphs],
            "Tangent line",
            r"y - y_0 = m(x - x_0)",
            "tangent line derivative graph figure",
            Body::Figure(graph_h_tangent),
            k(),
        ),
        item(
            "graph.riemann",
            &[Graphs],
            "Riemann sum",
            r"\sum f(x_i)\,\Delta x",
            "riemann sum rectangles midpoint area graph figure",
            Body::Figure(graph_h_riemann),
            k(),
        ),
        item(
            "graph.trapezoid",
            &[Graphs],
            "Trapezoid rule",
            r"\tfrac{\Delta x}{2}(y_0 + y_1)",
            "riemann trapezoid area graph figure",
            Body::Figure(graph_h_trapezoid),
            k(),
        ),
        item(
            "graph.area",
            &[Graphs],
            "Area between curves",
            r"\int (f - g)\,dx",
            "area between curves shade graph figure",
            Body::Figure(graph_h_area),
            k(),
        ),
        item(
            "graph.revolve",
            &[Graphs],
            "Solid of revolution",
            r"V = \pi \int f^2\,dx",
            "revolve solid volume disk shell graph figure",
            Body::Figure(graph_h_revolve),
            k(),
        ),
        item(
            "graph.vector",
            &[Graphs],
            "Vector field",
            r"\mathbf{F} = (P, Q)",
            "vector field arrows graph figure",
            Body::Figure(graph_h_vector),
            k(),
        ),
        item(
            "graph.gradient",
            &[Graphs],
            "Gradient field",
            r"\nabla f",
            "gradient field arrows graph figure",
            Body::Figure(graph_h_gradient),
            k(),
        ),
        item(
            "graph.contour",
            &[Graphs],
            "Contour plot",
            r"f(x, y) = c",
            "contour level curves graph figure",
            Body::Figure(graph_h_contour),
            k(),
        ),
        item(
            "graph.tplane",
            &[Graphs],
            "Tangent plane",
            r"z = f(x_0, y_0) + \ldots",
            "tangent plane surface 3d graph figure",
            Body::Figure(graph_h_tplane),
            k(),
        ),
        item(
            "graph.taylor",
            &[Graphs],
            "Taylor polynomials",
            r"\sum \tfrac{f^{(k)}(a)}{k!} x^k",
            "taylor series polynomial graph figure",
            Body::Figure(graph_h_taylor),
            k(),
        ),
        item(
            "graph.newton",
            &[Graphs],
            "Newton's method",
            r"x_{n+1} = x_n - \tfrac{f(x_n)}{f'(x_n)}",
            "newton method root graph figure",
            Body::Figure(graph_h_newton),
            k(),
        ),
        item(
            "graph.scatter",
            &[Graphs],
            "Scatter plot",
            r"(x_i, y_i)",
            "scatter data points plot figure",
            Body::Figure(graph_h_scatter),
            k(),
        ),
        item(
            "graph.histogram",
            &[Graphs],
            "Histogram",
            r"\text{frequency}",
            "histogram data bins plot figure",
            Body::Figure(graph_h_histogram),
            k(),
        ),
        item(
            "graph.boxplot",
            &[Graphs],
            "Box plot",
            r"Q_1, Q_2, Q_3",
            "box plot whisker data quartiles figure",
            Body::Figure(graph_h_boxplot),
            k(),
        ),
    ]);
    // ----- Text and Document: notes and the paper features -----
    let text_items: [(&str, Group, &str, &str, &str, &'static str); 14] = [
        (
            "text.heading",
            Group::Text,
            "Heading",
            r"\text{H}",
            "heading title section header #",
            "# ",
        ),
        (
            "text.list",
            Group::Text,
            "List",
            r"\bullet",
            "list bullet item items dash numbered -",
            "- ",
        ),
        (
            "text.comment",
            Group::Text,
            "Comment",
            r"\text{note}",
            "comment note remark aside % hidden",
            "% ",
        ),
        (
            "doc.title",
            Group::Document,
            "Title block",
            r"\text{Title}",
            "title author date heading paper",
            "Title: ⟨⟩\nAuthor: \nDate: ",
        ),
        (
            "doc.abstract",
            Group::Document,
            "Abstract",
            r"\text{Abstract}",
            "abstract summary paper",
            "Abstract: ",
        ),
        (
            "doc.theorem",
            Group::Document,
            "Theorem",
            r"\text{Theorem}",
            "theorem statement",
            "**Theorem.** ",
        ),
        (
            "doc.lemma",
            Group::Document,
            "Lemma",
            r"\text{Lemma}",
            "lemma",
            "**Lemma.** ",
        ),
        (
            "doc.conjecture",
            Group::Document,
            "Conjecture",
            r"\text{Conjecture}",
            "conjecture guess open problem proof agent",
            "**Conjecture.** ",
        ),
        (
            "doc.definition",
            Group::Document,
            "Definition",
            r"\text{Definition}",
            "definition define",
            "**Definition.** ",
        ),
        (
            "doc.example",
            Group::Document,
            "Example",
            r"\text{Example}",
            "example worked",
            "**Example.** ",
        ),
        (
            "doc.proof",
            Group::Document,
            "Proof",
            r"\text{Proof}",
            "proof qed",
            "**Proof.** ⟨⟩ ∎",
        ),
        (
            "doc.remark",
            Group::Document,
            "Remark",
            r"\text{Remark}",
            "remark note",
            "**Remark.** ",
        ),
        (
            "doc.references",
            Group::Document,
            "Reference list",
            r"\text{References}",
            "references bibliography citation cite",
            "References\n[@⟨⟩key] Author | Title | Year",
        ),
        (
            "doc.footnote",
            Group::Document,
            "Footnote",
            r"\text{Footnote}",
            "footnote note",
            "[^1]: ",
        ),
    ];
    v.push(Template {
        cite_picker: true,
        ..item(
            "doc.citation",
            &[Group::Document],
            "Citation",
            r"\text{[1]}",
            "citation cite reference source key",
            Body::Block("[@⟨⟩key]"),
            k(),
        )
    });
    // A maths line: the view makes the caret's line maths (U12).
    v.push(item(
        "text.mathsline",
        &[Group::Text],
        "Maths line",
        r"\sum x",
        "maths math line equation formula display block",
        Body::Block("$$ ⟨⟩ $$"),
        k(),
    ));
    for (id, group, name, icon, words, body) in text_items {
        v.push(item(
            id,
            &[group],
            name,
            icon,
            words,
            Body::Block(body),
            k(),
        ));
    }
    // ----- Units: a quantity's unit after its number, and conversions -----
    for (id, name, latex, words) in UNITS {
        v.push(item(
            &format!("unit.{id}"),
            &[Group::Units],
            name,
            latex,
            &format!("unit {words}"),
            Body::Symbol(latex),
            k(),
        ));
    }
    v.push(item(
        "unit.convert",
        &[Group::Units],
        "Convert to another unit",
        r"\operatorname{in}\,\mathrm{}",
        "unit convert in change units",
        Body::Symbol(r"\operatorname{in}\,\mathrm{}"),
        k(),
    ));
    // ----- Greek and symbols, from the symbol table -----
    let mut seen: Vec<&str> = vec![];
    for s in SYMBOLS {
        if seen.contains(&s.unicode) || s.unicode.is_empty() {
            continue;
        }
        let group = match s.kind {
            SymbolKind::Letter if is_greek(s.unicode) => Greek,
            SymbolKind::Relation
            | SymbolKind::Set
            | SymbolKind::Logic
            | SymbolKind::Arrow
            | SymbolKind::Operator => Symbols,
            SymbolKind::Calculus | SymbolKind::Misc if s.unicode.chars().count() == 1 => Symbols,
            _ => continue,
        };
        seen.push(s.unicode);
        v.push(item(
            &format!("sym.{}", s.name),
            &[group],
            s.name,
            s.unicode,
            &format!("{} {}", s.unicode, kind_word(s.kind)),
            Body::Symbol(s.unicode),
            k(),
        ));
    }
    v
}

/// The units of the Insert menu: id, name, what is inserted after a number, search words.
const UNITS: [(&str, &str, &str, &str); 26] = [
    ("m", "Metre", r"\,\mathrm{m}", "length metre meter"),
    ("km", "Kilometre", r"\,\mathrm{km}", "length kilometre"),
    ("cm", "Centimetre", r"\,\mathrm{cm}", "length centimetre"),
    ("mm", "Millimetre", r"\,\mathrm{mm}", "length millimetre"),
    ("s", "Second", r"\,\mathrm{s}", "time second"),
    ("min", "Minute", r"\,\mathrm{min}", "time minute"),
    ("h", "Hour", r"\,\mathrm{h}", "time hour"),
    ("kg", "Kilogram", r"\,\mathrm{kg}", "mass kilogram"),
    ("g", "Gram", r"\,\mathrm{g}", "mass gram"),
    (
        "ms",
        "Metres per second",
        r"\,\mathrm{m/s}",
        "speed velocity",
    ),
    (
        "kmh",
        "Kilometres per hour",
        r"\,\mathrm{km/h}",
        "speed velocity",
    ),
    (
        "ms2",
        "Metres per second squared",
        r"\,\mathrm{m/s^2}",
        "acceleration",
    ),
    ("N", "Newton", r"\,\mathrm{N}", "force newton"),
    ("J", "Joule", r"\,\mathrm{J}", "energy work joule"),
    ("W", "Watt", r"\,\mathrm{W}", "power watt"),
    ("Pa", "Pascal", r"\,\mathrm{Pa}", "pressure pascal"),
    (
        "kPa",
        "Kilopascal",
        r"\,\mathrm{kPa}",
        "pressure kilopascal",
    ),
    ("V", "Volt", r"\,\mathrm{V}", "voltage volt"),
    ("A", "Ampere", r"\,\mathrm{A}", "current ampere"),
    ("ohm", "Ohm", r"\,\Omega", "resistance ohm"),
    ("Hz", "Hertz", r"\,\mathrm{Hz}", "frequency hertz"),
    ("K", "Kelvin", r"\,\mathrm{K}", "temperature kelvin"),
    (
        "degC",
        "Degree Celsius",
        r"\,{}^\circ\mathrm{C}",
        "temperature celsius",
    ),
    ("L", "Litre", r"\,\mathrm{L}", "volume litre liter"),
    ("mol", "Mole", r"\,\mathrm{mol}", "amount mole"),
    ("rad", "Radian", r"\,\mathrm{rad}", "angle radian"),
];

fn is_greek(s: &str) -> bool {
    s.chars()
        .next()
        .is_some_and(|c| ('\u{0391}'..='\u{03F6}').contains(&c))
}

fn kind_word(k: SymbolKind) -> &'static str {
    match k {
        SymbolKind::Letter => "greek letter",
        SymbolKind::Relation => "relation",
        SymbolKind::Set => "set",
        SymbolKind::Logic => "logic",
        SymbolKind::Arrow => "arrow",
        SymbolKind::Operator => "operator",
        SymbolKind::Calculus => "calculus",
        _ => "symbol",
    }
}

/// A button on a template's little toolbar.
#[derive(Debug, Clone, PartialEq)]
pub struct Control {
    /// Shown on the button (plain text; maths symbols are Unicode).
    pub label: String,
    pub tip: String,
    pub action: Action,
    /// Drawn pressed (the current mode).
    pub active: bool,
}

/// What a control does.
#[derive(Debug, Clone, PartialEq)]
pub enum Action {
    /// Rebuilds the template with new knobs, keeping what was typed, except in
    /// the boxes listed (they get the new template's defaults).
    Rebuild { knobs: Knobs, reset: Vec<String> },
    /// Puts text in a box: the one the caret is in when it is one of `keys`,
    /// otherwise the first of `keys`.
    Fill { keys: Vec<String>, text: String },
    /// Swaps the text of pairs of boxes (the order of integration).
    Swap(Vec<(String, String)>),
}

fn rebuild(label: &str, tip: &str, knobs: Knobs, active: bool) -> Control {
    Control {
        label: label.into(),
        tip: tip.into(),
        action: Action::Rebuild {
            knobs,
            reset: vec![],
        },
        active,
    }
}

fn fill(label: &str, tip: &str, keys: &[&str], text: &str) -> Control {
    Control {
        label: label.into(),
        tip: tip.into(),
        action: Action::Fill {
            keys: keys.iter().map(|s| s.to_string()).collect(),
            text: text.into(),
        },
        active: false,
    }
}

fn plus_minus(
    out: &mut Vec<Control>,
    what: &str,
    knobs: Knobs,
    get: fn(&Knobs) -> u8,
    set: fn(&mut Knobs, u8),
    min: u8,
    max: u8,
) {
    let n = get(&knobs);
    if n < max {
        let mut k = knobs;
        set(&mut k, n + 1);
        out.push(rebuild(
            &format!("+ {what}"),
            &format!("Add a {what}"),
            k,
            false,
        ));
    }
    if n > min {
        let mut k = knobs;
        set(&mut k, n - 1);
        out.push(rebuild(
            &format!("\u{2212} {what}"),
            &format!("Remove the last {what}"),
            k,
            false,
        ));
    }
}

/// Draws pressed the fill buttons whose text is already in their box (the
/// current rule of a Riemann sum, the current axis and method of a volume), from
/// the template's box contents (see [`crate::editor::Session::contents`]).
pub fn mark_current(controls: &mut [Control], contents: &[(String, String)]) {
    for c in controls {
        if let Action::Fill { keys, text } = &c.action {
            let Some(k) = keys.first() else {
                continue;
            };
            let key = format!("{k}#0");
            if contents
                .iter()
                .any(|(kk, t)| *kk == key && !t.is_empty() && t.trim() == text.trim())
            {
                c.active = true;
            }
        }
    }
}

/// The controls a template shows while the caret is in it.
pub fn controls(template: &str, knobs: Knobs) -> Vec<Control> {
    let mut out = vec![];
    let infinity = |out: &mut Vec<Control>, keys: &[&str]| {
        out.push(fill("\u{2212}\u{221e}", "Minus infinity", keys, r"-\infty"));
        out.push(fill(
            "+\u{221e}",
            "Infinity",
            &[keys[keys.len() - 1]],
            r"\infty",
        ));
    };
    match template {
        "int.improper" => infinity(&mut out, &["lower", "upper"]),
        "int.def" => infinity(&mut out, &["lower", "upper"]),
        "int.double" | "int.triple" => {
            let triple = template == "int.triple";
            let mut region = knobs;
            region.mode = 0;
            let mut bounds = knobs;
            bounds.mode = 1;
            out.push(rebuild(
                "Region",
                "Integrate over a region (dA / dV)",
                region,
                knobs.mode == 0,
            ));
            out.push(rebuild(
                "Bounds",
                "Integrate between bounds, one pair per variable",
                bounds,
                knobs.mode == 1,
            ));
            if knobs.mode == 1 {
                let mut pairs = vec![
                    ("outer lower".to_string(), "inner lower".to_string()),
                    ("outer upper".to_string(), "inner upper".to_string()),
                    ("outer variable".to_string(), "inner variable".to_string()),
                ];
                if !triple {
                    pairs.truncate(3);
                }
                out.push(Control {
                    label: "Swap order".into(),
                    tip: "Swap the order of integration".into(),
                    action: Action::Swap(pairs),
                    active: false,
                });
            }
            if triple {
                for (i, (name, tip)) in [
                    ("Cartesian", "dV = dz dy dx"),
                    ("Cylindrical", "dV = r dz dr d\u{3b8}"),
                    (
                        "Spherical",
                        "dV = \u{3c1}\u{b2} sin \u{3c6} d\u{3c1} d\u{3c6} d\u{3b8}",
                    ),
                ]
                .into_iter()
                .enumerate()
                {
                    let mut kk = knobs;
                    kk.variant = i as u8;
                    out.push(Control {
                        label: name.into(),
                        tip: tip.into(),
                        action: Action::Rebuild {
                            knobs: kk,
                            reset: vec![
                                "outer variable".into(),
                                "middle variable".into(),
                                "inner variable".into(),
                            ],
                        },
                        active: knobs.variant == i as u8,
                    });
                }
            }
        }
        "lim" | "lim.side" => {
            for (i, (name, tip)) in [
                ("Both sides", "x \u{2192} a"),
                ("From above", "x \u{2192} a from above"),
                ("From below", "x \u{2192} a from below"),
            ]
            .into_iter()
            .enumerate()
            {
                let mut kk = knobs;
                kk.variant = i as u8;
                out.push(rebuild(name, tip, kk, knobs.variant == i as u8));
            }
            out.push(fill(
                "\u{2212}\u{221e}",
                "Minus infinity",
                &["value"],
                r"-\infty",
            ));
            out.push(fill("+\u{221e}", "Infinity", &["value"], r"\infty"));
        }
        "grad" | "div" | "curl" | "laplacian" => {
            for (i, name) in ["Cartesian", "Cylindrical", "Spherical"]
                .into_iter()
                .enumerate()
            {
                let mut kk = knobs;
                kk.variant = i as u8;
                out.push(Control {
                    label: name.into(),
                    tip: "Coordinate system".into(),
                    action: Action::Rebuild {
                        knobs: kk,
                        reset: vec!["variables".into()],
                    },
                    active: knobs.variant == i as u8,
                });
            }
        }
        "partial" => plus_minus(
            &mut out,
            "variable",
            knobs,
            |k| k.rows.max(1),
            |k, n| k.rows = n,
            1,
            6,
        ),
        "ode" | "ode.ivp" => plus_minus(
            &mut out,
            "condition",
            knobs,
            |k| k.rows,
            |k, n| k.rows = n,
            0,
            6,
        ),
        "ode.system" => {
            plus_minus(
                &mut out,
                "equation",
                knobs,
                |k| k.rows.max(1),
                |k, n| k.rows = n,
                1,
                6,
            );
            plus_minus(
                &mut out,
                "condition",
                knobs,
                |k| k.cols,
                |k, n| k.cols = n,
                0,
                6,
            );
        }
        "ln" | "log10" | "logb" => {
            for (i, name) in ["Base e (ln)", "Base 10", "Any base"]
                .into_iter()
                .enumerate()
            {
                let mut kk = knobs;
                kk.mode = i as u8;
                out.push(rebuild(name, "Logarithm base", kk, knobs.mode == i as u8));
            }
        }
        "cases" => {
            plus_minus(
                &mut out,
                "row",
                knobs,
                |k| k.rows.max(1),
                |k, n| k.rows = n,
                1,
                12,
            );
            let mut kk = knobs;
            kk.flag = !knobs.flag;
            out.push(rebuild(
                "Otherwise",
                "An \u{201c}otherwise\u{201d} row",
                kk,
                knobs.flag,
            ));
        }
        "fn" => plus_minus(
            &mut out,
            "argument",
            knobs,
            |k| k.rows.max(1),
            |k, n| k.rows = n,
            1,
            6,
        ),
        "matrix" | "vector" => {
            plus_minus(
                &mut out,
                "row",
                knobs,
                |k| k.rows.max(1),
                |k, n| k.rows = n,
                1,
                12,
            );
            plus_minus(
                &mut out,
                "column",
                knobs,
                |k| k.cols.max(1),
                |k, n| k.cols = n,
                1,
                12,
            );
        }
        "helper.lagrange" => plus_minus(
            &mut out,
            "constraint",
            knobs,
            |k| k.rows.max(1),
            |k, n| k.rows = n,
            1,
            6,
        ),
        "helper.volume" | "helper.surfacearea" => {
            out.push(fill(
                "x-axis",
                "Revolve about the x-axis",
                &["axis"],
                "y = 0",
            ));
            out.push(fill(
                "y-axis",
                "Revolve about the y-axis",
                &["axis"],
                "x = 0",
            ));
            if template == "helper.volume" {
                for (m, label, tip) in [
                    ("disk", "Discs", "Slice into discs"),
                    ("washer", "Washers", "Slice into washers"),
                    ("shell", "Shells", "Use cylindrical shells"),
                    ("auto", "Automatic", "Choose the method automatically"),
                ] {
                    out.push(fill(label, tip, &["method"], &format!(r"\text{{{m}}}")));
                }
            }
        }
        "helper.riemann" => {
            for (m, label, tip) in [
                ("left", "Left", "Left endpoints"),
                ("right", "Right", "Right endpoints"),
                ("mid", "Midpoint", "Midpoints"),
                ("trapezoid", "Trapezoid", "Trapezoid rule"),
                ("simpson", "Simpson", "Simpson's rule"),
            ] {
                out.push(fill(label, tip, &["rule"], &format!(r"\text{{{m}}}")));
            }
        }
        "graph.2d" | "graph.3d" | "graph.param" | "graph.slope" => {
            if template == "graph.2d" || template == "graph.3d" {
                plus_minus(
                    &mut out,
                    "curve",
                    knobs,
                    |k| k.rows.max(1),
                    |k, n| k.rows = n,
                    1,
                    6,
                );
            }
            plus_minus(
                &mut out,
                "slider",
                knobs,
                |k| k.cols,
                |k, n| k.cols = n,
                0,
                6,
            );
        }
        _ => {}
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::editor::parse_snippet;

    #[test]
    fn every_template_snippet_parses_with_braced_boxes() {
        let cx = BuildCx::default();
        for t in catalogue() {
            let src = t.snippet_source(t.knobs, &cx);
            assert!(!src.is_empty(), "{} builds nothing", t.id);
            if t.is_symbol() || t.is_text() {
                continue;
            }
            let s = parse_snippet(&src).unwrap_or_else(|e| panic!("{}: {e:?}", t.id));
            for b in &s.boxes {
                if b.bare || b.mirror.is_some() {
                    continue;
                }
                let before = s.text[..b.range.start].chars().next_back();
                let after = s.text[b.range.end..].chars().next();
                assert!(
                    matches!(
                        (before, after),
                        (Some('{'), Some('}')) | (Some('['), Some(']'))
                    ),
                    "{}: box {} is not inside braces in {}",
                    t.id,
                    b.label,
                    s.text
                );
            }
        }
    }

    #[test]
    fn menu_pictures_are_whole_formulas() {
        for t in catalogue().iter().filter(|t| !t.is_symbol()) {
            let (_, diags) = mpp_syntax::math::parse_math(&t.icon, 0);
            assert!(diags.is_empty(), "{}: {} has {diags:?}", t.id, t.icon);
            let dl = mpp_render::layout_latex(&t.icon, &mpp_render::LayoutOptions::inline(15.0));
            assert!(dl.slots().is_empty(), "{}: empty box in {}", t.id, t.icon);
        }
    }

    #[test]
    fn ids_are_unique_and_every_group_has_items() {
        let mut ids: Vec<&str> = catalogue().iter().map(|t| t.id.as_str()).collect();
        let n = ids.len();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), n, "duplicate ids");
        for g in Group::ALL {
            assert!(
                catalogue().iter().any(|t| t.groups.contains(&g)),
                "{} is empty",
                g.title()
            );
        }
        let greek = catalogue()
            .iter()
            .filter(|t| t.groups.contains(&Group::Greek))
            .count();
        assert!(
            greek >= 40,
            "every Greek letter, upper and lower case: {greek}"
        );
    }

    #[test]
    fn search_matches_names_and_keywords() {
        let t = find("int.def").expect("definite integral");
        assert!(t.matches("integral"));
        assert!(t.matches("Calculus"));
        assert!(t.matches("def int"));
        assert!(!t.matches("matrix"));
        assert!(find("sym.alpha").is_some_and(|t| t.matches("alpha")));
        // Searching "matrix" finds everything made of or from a matrix.
        for id in ["vector", "det", "eigen", "inverse", "transpose"] {
            assert!(
                find(id).is_some_and(|t| t.matches("matrix")),
                "{id} is found by matrix"
            );
        }
    }

    #[test]
    fn controls_change_the_shape() {
        let m = find("matrix").expect("matrix");
        let c = controls("matrix", m.knobs);
        assert!(c.iter().any(|c| c.label == "+ row"));
        assert!(c.iter().any(|c| c.label == "\u{2212} column"));
        let d = controls("int.double", Knobs::default());
        assert!(d.iter().any(|c| c.label == "Bounds"));
        let b = controls(
            "int.double",
            Knobs {
                mode: 1,
                ..Knobs::default()
            },
        );
        assert!(b.iter().any(|c| matches!(c.action, Action::Swap(_))));
    }
}
