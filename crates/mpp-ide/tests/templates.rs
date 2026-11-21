//! Every template of the Insert menu, against the "C9 template slots" table:
//! insert it, check its boxes (count, Tab order, labels, defaults, required),
//! Tab through and fill it, then check the `.mpp` text it wrote, that the text
//! parses, that nothing raw is visible, and the answer where one exists today.

use mpp_eval::{LineKind, Session};
use mpp_ide::editor::{Editor, Knobs};
use mpp_ide::insert::{self, apply, catalogue, controls, find, Action, BuildCx};
use mpp_ide::page::{build_page, Page, PageInput};
use mpp_render::{HitBox, HitKind, IntegralLimits, Primitive, Theme};
use mpp_syntax::document::{parse_document, BlockKind};

fn page_with(ed: &Editor, limits: IntegralLimits) -> Page {
    let doc = parse_document(ed.text());
    let labels = ed.slot_labels();
    build_page(&PageInput {
        show: &mpp_ide::page::ShowState::default(),
        src: ed.text(),
        doc: &doc,
        eval: None,
        theme: Theme::LIGHT,
        size: 18.0,
        width: 900.0,
        integral_limits: limits,
        slot_labels: &labels,
        extra_refs: &mpp_ide::page::ExtraRefs::default(),
        proof: &mpp_ide::proof::ProofView::default(),
    })
}

fn page_of(ed: &Editor) -> Page {
    page_with(ed, IntegralLimits::AboveBelow)
}

fn insert_into(ed: &mut Editor, id: &str, knobs: Option<Knobs>) -> u64 {
    let t = find(id).unwrap_or_else(|| panic!("no template {id}"));
    let page = page_of(ed);
    let cx = BuildCx::default();
    insert::insert(ed, &page, t, knobs.unwrap_or(t.knobs), &cx)
        .unwrap_or_else(|| panic!("{id} made no template"))
}

/// What a reader sees: every glyph on the page.
fn visible(ed: &Editor) -> String {
    page_of(ed)
        .blocks
        .iter()
        .map(|b| b.dl.text())
        .collect::<Vec<_>>()
        .join("\n")
}

fn assert_nothing_raw(id: &str, shown: &str) {
    for bad in [
        "\\",
        "_",
        "^",
        "$",
        "frac",
        "operatorname",
        "mathcal",
        "begin",
        "end{",
        "quad",
        "left",
        "right",
        "cdot",
        "infty",
        "Figure:",
    ] {
        assert!(
            !shown.contains(bad),
            "{id}: raw {bad:?} is visible in {shown:?}"
        );
    }
}

fn slot_hits(page: &Page) -> Vec<HitBox> {
    let mut v: Vec<HitBox> = page
        .blocks
        .iter()
        .flat_map(|b| {
            b.dl.hits.iter().cloned().map(move |mut h| {
                h.rect.y += b.top;
                h
            })
        })
        .filter(|h| h.kind == HitKind::Slot)
        .collect();
    v.sort_by_key(|h| h.span.start);
    v
}

/// One row of the table: the boxes in Tab order as (label, default, required),
/// what to type in each ("" keeps the default), the text it must write, and the
/// answer to `= ?` where the evaluator already works it out.
struct Case {
    id: &'static str,
    knobs: Option<Knobs>,
    boxes: &'static [(&'static str, &'static str, bool)],
    fills: &'static [&'static str],
    text: &'static str,
    answer: Option<&'static str>,
}

const fn case(
    id: &'static str,
    boxes: &'static [(&'static str, &'static str, bool)],
    fills: &'static [&'static str],
    text: &'static str,
    answer: Option<&'static str>,
) -> Case {
    Case {
        id,
        knobs: None,
        boxes,
        fills,
        text,
        answer,
    }
}

const fn with(knobs: Knobs, c: Case) -> Case {
    Case {
        knobs: Some(knobs),
        ..c
    }
}

const fn knobs(rows: u8, cols: u8, mode: u8, variant: u8, flag: bool) -> Knobs {
    Knobs {
        rows,
        cols,
        mode,
        variant,
        flag,
    }
}

const R: bool = true;
const O: bool = false;

fn cases() -> Vec<Case> {
    vec![
        // ----- calculus -----
        case(
            "int.indef",
            &[("integrand", "", R), ("variable", "x", R)],
            &["x^2", ""],
            r"\int {x^2} \, dx",
            None,
        ),
        case(
            "int.def",
            &[
                ("lower", "", O),
                ("upper", "", O),
                ("integrand", "", R),
                ("variable", "x", R),
            ],
            &["0", "1", "x^2", ""],
            r"\int_{0}^{1} {x^2} \, dx",
            None,
        ),
        case(
            "int.def",
            &[
                ("lower", "", O),
                ("upper", "", O),
                ("integrand", "", R),
                ("variable", "x", R),
            ],
            &["", "", "x^2", "t"],
            r"\int {x^2} \, dt",
            None,
        ),
        case(
            "int.improper",
            &[
                ("lower", "", R),
                ("upper", "", R),
                ("integrand", "", R),
                ("variable", "x", R),
            ],
            &["0", r"\infty", "e^{-x}", ""],
            r"\int_{0}^{\infty} {e^{-x}} \, dx",
            None,
        ),
        case(
            "int.double",
            &[("region", "", R), ("integrand", "", R), ("element", "A", R)],
            &["D", "x y", ""],
            r"\iint_{D} {x y} \, dA",
            None,
        ),
        with(
            knobs(0, 0, 1, 0, false),
            case(
                "int.double",
                &[
                    ("outer lower", "", R),
                    ("outer upper", "", R),
                    ("inner lower", "", R),
                    ("inner upper", "", R),
                    ("integrand", "", R),
                    ("inner variable", "y", R),
                    ("outer variable", "x", R),
                ],
                &["0", "1", "0", "2", "x y", "", ""],
                r"\int_{0}^{1} \int_{0}^{2} {x y} \, dy \, dx",
                None,
            ),
        ),
        case(
            "int.triple",
            &[("region", "", R), ("integrand", "", R), ("element", "V", R)],
            &["E", "1", ""],
            r"\iiint_{E} {1} \, dV",
            None,
        ),
        with(
            knobs(0, 0, 1, 0, false),
            case(
                "int.triple",
                &[
                    ("outer lower", "", R),
                    ("outer upper", "", R),
                    ("middle lower", "", R),
                    ("middle upper", "", R),
                    ("inner lower", "", R),
                    ("inner upper", "", R),
                    ("integrand", "", R),
                    ("inner variable", "z", R),
                    ("middle variable", "y", R),
                    ("outer variable", "x", R),
                ],
                &["0", "1", "0", "1", "0", "1", "1", "", "", ""],
                r"\int_{0}^{1} \int_{0}^{1} \int_{0}^{1} {1} \, dz \, dy \, dx",
                None,
            ),
        ),
        with(
            knobs(0, 0, 1, 1, false),
            case(
                "int.triple",
                &[
                    ("outer lower", "", R),
                    ("outer upper", "", R),
                    ("middle lower", "", R),
                    ("middle upper", "", R),
                    ("inner lower", "", R),
                    ("inner upper", "", R),
                    ("integrand", "", R),
                    ("inner variable", "z", R),
                    ("middle variable", "r", R),
                    ("outer variable", r"\theta", R),
                ],
                &["0", r"2\pi", "0", "1", "0", "1", "1", "", "", ""],
                r"\int_{0}^{2\pi} \int_{0}^{1} \int_{0}^{1} {1} \, r \, dz \, dr \, d\theta",
                None,
            ),
        ),
        case(
            "int.line",
            &[
                ("curve", "C", R),
                ("field", "", R),
                ("differential", r"\mathbf{r}", R),
            ],
            &["", r"\mathbf{F}", ""],
            r"\oint_{C} {\mathbf{F}} \cdot d\mathbf{r}",
            None,
        ),
        case(
            "int.surface",
            &[
                ("surface", "S", R),
                ("field", "", R),
                ("differential", r"\mathbf{S}", R),
            ],
            &["", r"\mathbf{F}", ""],
            r"\oiint_{S} {\mathbf{F}} \cdot d\mathbf{S}",
            None,
        ),
        case(
            "deriv",
            &[("function", "", R), ("variable", "x", R), ("at", "", O)],
            &["x^3", "", ""],
            r"\frac{d}{dx} {x^3}",
            None,
        ),
        case(
            "deriv",
            &[("function", "", R), ("variable", "x", R), ("at", "", O)],
            &["t^3", "t", "2"],
            r"\frac{d}{dt} {t^3}, \quad t = {2}",
            None,
        ),
        case(
            "deriv.n",
            &[
                ("order", "2", R),
                ("function", "", R),
                ("variable", "x", R),
                ("at", "", O),
            ],
            &["3", r"\sin x", "", ""],
            r"\frac{d^{3}}{dx^{3}} {\sin x}",
            None,
        ),
        case(
            "partial",
            &[("function", "", R), ("variable", "x", R)],
            &["x^2 y", ""],
            r"\frac{\partial}{\partial x} {x^2 y}",
            None,
        ),
        with(
            knobs(2, 0, 0, 0, false),
            case(
                "partial",
                &[
                    ("function", "", R),
                    ("variable", "x", R),
                    ("variable", "y", R),
                ],
                &["x^2 y", "", ""],
                r"\frac{\partial^{2}}{\partial x \, \partial y} {x^2 y}",
                None,
            ),
        ),
        case(
            "grad",
            &[("field", "", R), ("variables", "x, y, z", R)],
            &["x^2 + y^2", ""],
            r"\nabla_{x, y, z} {x^2 + y^2}",
            None,
        ),
        case(
            "div",
            &[("field", "", R), ("variables", "x, y, z", R)],
            &[r"\mathbf{F}", ""],
            r"\nabla_{x, y, z} \cdot {\mathbf{F}}",
            None,
        ),
        case(
            "curl",
            &[("field", "", R), ("variables", "x, y, z", R)],
            &[r"\mathbf{F}", ""],
            r"\nabla_{x, y, z} \times {\mathbf{F}}",
            None,
        ),
        case(
            "laplacian",
            &[("field", "", R), ("variables", "x, y, z", R)],
            &["f", ""],
            r"\nabla^{2}_{x, y, z} {f}",
            None,
        ),
        case(
            "lim",
            &[
                ("variable", "x", R),
                ("value", "", R),
                ("expression", "", R),
            ],
            &["", "0", r"\frac{\sin x}{x}"],
            r"\lim_{x \to {0}} {\frac{\sin x}{x}}",
            None,
        ),
        case(
            "lim.side",
            &[
                ("variable", "x", R),
                ("value", "", R),
                ("expression", "", R),
            ],
            &["", "0", r"\frac{1}{x}"],
            r"\lim_{x \to {0}^{+}} {\frac{1}{x}}",
            None,
        ),
        case(
            "taylor",
            &[
                ("function", "", R),
                ("centre", "0", R),
                ("order", "5", R),
                ("variable", "x", R),
            ],
            &[r"\sin x", "", "", ""],
            r"\operatorname{series}_{{x}={0}}^{{5}} {\sin x}",
            None,
        ),
        case(
            "ode",
            &[
                ("equation", "", R),
                ("unknown function", "y", R),
                ("variable", "x", R),
            ],
            &["y' = y", "", ""],
            r"\operatorname{solve}_{y(x)} \left( {y' = y} \right)",
            None,
        ),
        case(
            "ode.ivp",
            &[
                ("equation", "", R),
                ("unknown function", "y", R),
                ("variable", "x", R),
                ("point", "", R),
                ("value", "", R),
            ],
            &["y' = y", "", "", "0", "1"],
            r"\operatorname{solve}_{y(x)} \left( {y' = y} \right), \quad y({0}) = {1}",
            None,
        ),
        case(
            "ode.system",
            &[
                ("equation", "", R),
                ("equation", "", R),
                ("unknowns", "x, y", R),
            ],
            &["x' = y", "y' = -x", ""],
            r"\operatorname{solve}_{x, y} \left( \begin{cases} {x' = y} \\ {y' = -x} \end{cases} \right)",
            None,
        ),
        case(
            "laplace",
            &[("function", "", R), ("time", "t", R), ("transform", "s", R)],
            &["e^{-t}", "", ""],
            r"\mathcal{L}_{{t} \to {s}}\left\{ {e^{-t}} \right\}",
            None,
        ),
        case(
            "laplace.inv",
            &[("function", "", R), ("transform", "s", R), ("time", "t", R)],
            &[r"\frac{1}{s}", "", ""],
            r"\mathcal{L}^{-1}_{{s} \to {t}}\left\{ {\frac{1}{s}} \right\}",
            None,
        ),
        case(
            "fourier",
            &[
                ("function", "", R),
                ("variable", "x", R),
                ("frequency", r"\omega", R),
            ],
            &["e^{-x^2}", "", ""],
            r"\mathcal{F}_{{x} \to {\omega}}\left\{ {e^{-x^2}} \right\}",
            None,
        ),
        case(
            "fourier.inv",
            &[
                ("function", "", R),
                ("frequency", r"\omega", R),
                ("variable", "x", R),
            ],
            &["1", "", ""],
            r"\mathcal{F}^{-1}_{{\omega} \to {x}}\left\{ {1} \right\}",
            None,
        ),
        case(
            "jacobian",
            &[("outputs", "", R), ("inputs", "", R)],
            &["u, v", "x, y"],
            r"\frac{\partial(u, v)}{\partial(x, y)}",
            None,
        ),
        // ----- sums and products -----
        case(
            "sum",
            &[
                ("index", "n", R),
                ("start", "1", R),
                ("end", r"\infty", R),
                ("term", "", R),
            ],
            &["", "", "10", "n"],
            r"\sum_{n={1}}^{10} {n}",
            Some("55"),
        ),
        case(
            "prod",
            &[
                ("index", "n", R),
                ("start", "1", R),
                ("end", r"\infty", R),
                ("term", "", R),
            ],
            &["k", "", "5", "k"],
            r"\prod_{k={1}}^{5} {k}",
            Some("120"),
        ),
        // ----- algebra -----
        case(
            "frac",
            &[("numerator", "", R), ("denominator", "", R)],
            &["1", "2"],
            r"\frac{1}{2}",
            Some(r"\frac{1}{2}"),
        ),
        case(
            "sqrt",
            &[("radicand", "", R)],
            &["9"],
            r"\sqrt{9}",
            Some("3"),
        ),
        case(
            "root",
            &[("index", "", O), ("radicand", "", R)],
            &["3", "8"],
            r"\sqrt[3]{8}",
            Some("2"),
        ),
        case(
            "root",
            &[("index", "", O), ("radicand", "", R)],
            &["", "9"],
            r"\sqrt{9}",
            Some("3"),
        ),
        case(
            "pow",
            &[("base", "", R), ("exponent", "", R)],
            &["2", "10"],
            r"{2}^{10}",
            Some("1024"),
        ),
        case(
            "subscript",
            &[("base", "", R), ("subscript", "", R)],
            &["x", "1"],
            r"{x}_{1}",
            None,
        ),
        case(
            "ln",
            &[("argument", "", R)],
            &["e^2"],
            r"\ln\left( {e^2} \right)",
            Some("2"),
        ),
        case(
            "log10",
            &[("argument", "", R)],
            &["100"],
            r"\log_{10}\left( {100} \right)",
            Some("2"),
        ),
        case(
            "logb",
            &[("base", "", R), ("argument", "", R)],
            &["2", "8"],
            r"\log_{2}\left( {8} \right)",
            Some("3"),
        ),
        case(
            "abs",
            &[("argument", "", R)],
            &["-3"],
            r"\left| {-3} \right|",
            Some("3"),
        ),
        case(
            "floor",
            &[("argument", "", R)],
            &["2.5"],
            r"\left\lfloor {2.5} \right\rfloor",
            Some("2"),
        ),
        case(
            "ceil",
            &[("argument", "", R)],
            &["2.5"],
            r"\left\lceil {2.5} \right\rceil",
            Some("3"),
        ),
        case("factorial", &[("n", "", R)], &["4"], r"{4}!", Some("24")),
        case(
            "binom",
            &[("n", "", R), ("k", "", R)],
            &["5", "2"],
            r"\binom{5}{2}",
            Some("10"),
        ),
        case(
            "cases",
            &[("value", "", R), ("condition", "", R), ("value", "", R)],
            &["x", r"x \ge 0", "-x"],
            r"\begin{cases} {x} & {x \ge 0} \\ {-x} & \text{otherwise} \end{cases}",
            None,
        ),
        case(
            "fn",
            &[("name", "f", R), ("argument", "x", R), ("body", "", R)],
            &["", "", "x^2"],
            r"f(x) := {x^2}",
            None,
        ),
        with(
            knobs(2, 0, 0, 0, false),
            case(
                "fn",
                &[
                    ("name", "f", R),
                    ("argument", "x", R),
                    ("argument", "y", R),
                    ("body", "", R),
                ],
                &["g", "", "", "x y"],
                r"g(x, y) := {x y}",
                None,
            ),
        ),
        // ----- linear algebra -----
        case(
            "matrix",
            &[("", "", R), ("", "", R), ("", "", R), ("", "", R)],
            &["1", "2", "3", "4"],
            r"\begin{pmatrix} {1} & {2} \\ {3} & {4} \end{pmatrix}",
            Some(r"\begin{pmatrix} 1 & 2 \\ 3 & 4 \end{pmatrix}"),
        ),
        with(
            knobs(2, 3, 0, 0, false),
            case(
                "matrix",
                &[
                    ("", "", R),
                    ("", "", R),
                    ("", "", R),
                    ("", "", R),
                    ("", "", R),
                    ("", "", R),
                ],
                &["1", "2", "3", "4", "5", "6"],
                r"\begin{pmatrix} {1} & {2} & {3} \\ {4} & {5} & {6} \end{pmatrix}",
                None,
            ),
        ),
        case(
            "vector",
            &[("", "", R), ("", "", R), ("", "", R)],
            &["1", "2", "3"],
            r"\begin{pmatrix} {1} \\ {2} \\ {3} \end{pmatrix}",
            None,
        ),
        case(
            "det",
            &[("matrix", "", R)],
            &["A"],
            r"\det\left( {A} \right)",
            None,
        ),
        case("inverse", &[("matrix", "", R)], &["A"], r"{A}^{-1}", None),
        case("transpose", &[("matrix", "", R)], &["A"], r"{A}^{T}", None),
        case(
            "eigen",
            &[("matrix", "", R)],
            &["A"],
            r"\operatorname{eig}\left( {A} \right)",
            None,
        ),
        // ----- helpers named in the table -----
        case(
            "helper.crit",
            &[("function", "", R), ("interval", "", O)],
            &["x^3 - 3x", "[0, 4]"],
            r"\operatorname{crit}_{[0, 4]} {x^3 - 3x}",
            None,
        ),
        case(
            "helper.crit",
            &[("function", "", R), ("interval", "", O)],
            &["x^3 - 3x", ""],
            r"\operatorname{crit} {x^3 - 3x}",
            None,
        ),
        case(
            "helper.tangent",
            &[("function", "", R), ("point", "", R)],
            &["x^2", "x = 1"],
            r"\operatorname{tangent}_{x = 1} {x^2}",
            None,
        ),
        case(
            "helper.area",
            &[
                ("top curve", "", R),
                ("bottom curve", "", R),
                ("from", "", O),
                ("to", "", O),
            ],
            &["x", "x^2", "", ""],
            r"\operatorname{area}\left( {x}, {x^2} \right)",
            None,
        ),
        case(
            "helper.area",
            &[
                ("top curve", "", R),
                ("bottom curve", "", R),
                ("from", "", O),
                ("to", "", O),
            ],
            &["x", "x^2", "0", "1"],
            r"\operatorname{area}_{0}^{1}\left( {x}, {x^2} \right)",
            None,
        ),
        case(
            "helper.volume",
            &[
                ("function", "", R),
                ("axis", "y = 0", R),
                ("method", r"\text{auto}", R),
                ("from", "", R),
                ("to", "", R),
            ],
            &[r"\sqrt{x}", "", "", "0", "4"],
            r"\operatorname{volume}_{0}^{4}\left( {\sqrt{x}}, {y = 0}, {\text{auto}} \right)",
            None,
        ),
        case(
            "helper.riemann",
            &[
                ("function", "", R),
                ("from", "", R),
                ("to", "", R),
                ("n", "4", R),
                ("rule", r"\text{mid}", R),
            ],
            &["x^2", "0", "2", "", ""],
            r"\operatorname{riemann}_{0}^{2}\left( {x^2}, {4}, {\text{mid}} \right)",
            None,
        ),
        case(
            "helper.newton",
            &[
                ("function", "", R),
                ("starting x", "", R),
                ("iterations", "5", R),
            ],
            &["x^2 - 2", "1", ""],
            r"\operatorname{newton}\left( {x^2 - 2}, {1}, {5} \right)",
            None,
        ),
        case(
            "helper.lagrange",
            &[("function", "", R), ("constraint", "", R)],
            &["x y", "x + y = 1"],
            r"\operatorname{lagrange}\left( {x y}, {x + y = 1} \right)",
            None,
        ),
        case(
            "helper.directional",
            &[("function", "", R), ("point", "", R), ("direction", "", R)],
            &["x y", "(1, 2)", "(3, 4)"],
            r"\operatorname{directional}\left( {x y}, {(1, 2)}, {(3, 4)} \right)",
            None,
        ),
        // ----- graphs -----
        case(
            "graph.2d",
            &[("expression", "", R), ("from", "-10", R), ("to", "10", R)],
            &["x^2", "", ""],
            r"Figure: ${x^2}$, $x \in [{-10}, {10}]$.",
            None,
        ),
        case(
            "graph.3d",
            &[
                ("expression", "", R),
                ("from", "-10", R),
                ("to", "10", R),
                ("from", "-10", R),
                ("to", "10", R),
            ],
            &["x^2 - y^2", "-2", "2", "-2", "2"],
            r"Figure: ${x^2 - y^2}$, $x \in [{-2}, {2}]$, $y \in [{-2}, {2}]$.",
            None,
        ),
        case(
            "graph.param",
            &[
                ("x(t)", "", R),
                ("y(t)", "", R),
                ("from", "-10", R),
                ("to", "10", R),
            ],
            &[r"\cos t", r"\sin t", "0", r"2\pi"],
            r"Figure: parametric $({\cos t}, {\sin t})$, $t \in [{0}, {2\pi}]$.",
            None,
        ),
        case(
            "graph.slope",
            &[
                ("equation", "", R),
                ("from", "-10", R),
                ("to", "10", R),
                ("from", "-10", R),
                ("to", "10", R),
            ],
            &["y' = x - y", "", "", "", ""],
            r"Figure: slope field ${y' = x - y}$, $x \in [{-10}, {10}]$, $y \in [{-10}, {10}]$.",
            None,
        ),
        case(
            "graph.tangent",
            &[
                ("function", "", R),
                ("point", "", R),
                ("from", "-10", R),
                ("to", "10", R),
            ],
            &[r"x^3 - 2x", r"1", r"-2", r"2"],
            r"Figure: ${x^3 - 2x}$, tangent at $x = {1}$, $x \in [{-2}, {2}]$.",
            None,
        ),
        case(
            "graph.riemann",
            &[
                ("function", "", R),
                ("n", "8", R),
                ("from", "-10", R),
                ("to", "10", R),
            ],
            &[r"x^2", r"8", r"0", r"2"],
            r"Figure: ${x^2}$, riemann mid $n = {8}$, $x \in [{0}, {2}]$.",
            None,
        ),
        case(
            "graph.trapezoid",
            &[
                ("function", "", R),
                ("n", "8", R),
                ("from", "-10", R),
                ("to", "10", R),
            ],
            &[r"x^2", r"6", r"0", r"2"],
            r"Figure: ${x^2}$, riemann trapezoid $n = {6}$, $x \in [{0}, {2}]$.",
            None,
        ),
        case(
            "graph.area",
            &[
                ("top", "", R),
                ("bottom", "", R),
                ("from", "-10", R),
                ("to", "10", R),
            ],
            &[r"x", r"x^2", r"0", r"1"],
            r"Figure: ${x}$, ${x^2}$, area between, $x \in [{0}, {1}]$.",
            None,
        ),
        case(
            "graph.revolve",
            &[
                ("function", "", R),
                ("axis", "0", R),
                ("from", "-10", R),
                ("to", "10", R),
            ],
            &[r"\sqrt{x}", r"0", r"0", r"4"],
            r"Figure: ${\sqrt{x}}$, revolve about $y = {0}$, $x \in [{0}, {4}]$.",
            None,
        ),
        case(
            "graph.vector",
            &[
                ("P", "", R),
                ("Q", "", R),
                ("from", "-10", R),
                ("to", "10", R),
                ("from", "-10", R),
                ("to", "10", R),
            ],
            &[r"-y", r"x", r"-2", r"2", r"-2", r"2"],
            r"Figure: vector field $({-y}, {x})$, $x \in [{-2}, {2}]$, $y \in [{-2}, {2}]$.",
            None,
        ),
        case(
            "graph.gradient",
            &[
                ("function", "", R),
                ("from", "-10", R),
                ("to", "10", R),
                ("from", "-10", R),
                ("to", "10", R),
            ],
            &[r"x^2 + y^2", r"-2", r"2", r"-2", r"2"],
            r"Figure: gradient field ${x^2 + y^2}$, $x \in [{-2}, {2}]$, $y \in [{-2}, {2}]$.",
            None,
        ),
        case(
            "graph.contour",
            &[
                ("function", "", R),
                ("from", "-10", R),
                ("to", "10", R),
                ("from", "-10", R),
                ("to", "10", R),
            ],
            &[r"x^2 - y^2", r"-2", r"2", r"-2", r"2"],
            r"Figure: ${x^2 - y^2}$, contour, $x \in [{-2}, {2}]$, $y \in [{-2}, {2}]$.",
            None,
        ),
        case(
            "graph.tplane",
            &[
                ("function", "", R),
                ("x0", "", R),
                ("y0", "", R),
                ("from", "-10", R),
                ("to", "10", R),
                ("from", "-10", R),
                ("to", "10", R),
            ],
            &[r"x^2 + y^2", r"1", r"1", r"-2", r"2", r"-2", r"2"],
            r"Figure: $z = {x^2 + y^2}$, tangent plane at $({1}, {1})$, $x \in [{-2}, {2}]$, $y \in [{-2}, {2}]$.",
            None,
        ),
        case(
            "graph.taylor",
            &[
                ("function", "", R),
                ("degree", "5", R),
                ("from", "-10", R),
                ("to", "10", R),
            ],
            &[r"\sin x", r"5", r"-3", r"3"],
            r"Figure: ${\sin x}$, taylor ${5}$, $x \in [{-3}, {3}]$.",
            None,
        ),
        case(
            "graph.newton",
            &[
                ("function", "", R),
                ("start", "", R),
                ("from", "-10", R),
                ("to", "10", R),
            ],
            &[r"x^2 - 2", r"2", r"0", r"3"],
            r"Figure: ${x^2 - 2}$, newton from $x_0 = {2}$, $x \in [{0}, {3}]$.",
            None,
        ),
        case(
            "graph.scatter",
            &[("x column", "", R), ("y column", "", R)],
            &[r"t", r"x"],
            r"Figure: scatter ${t}, {x}$.",
            None,
        ),
        case(
            "graph.histogram",
            &[("column", "", R)],
            &[r"x"],
            r"Figure: histogram ${x}$.",
            None,
        ),
        case(
            "graph.boxplot",
            &[("column", "", R)],
            &[r"x"],
            r"Figure: box plot ${x}$.",
            None,
        ),
    ]
}

/// The answer to `= ?` on the block (its LaTeX), after evaluating the text.
fn answer(src: &str) -> Option<String> {
    let mut s = Session::new();
    let ev = s.run_str(src).clone();
    ev.blocks
        .iter()
        .flat_map(|b| b.values.iter())
        .find(|v| matches!(v.kind, LineKind::Query { .. }))
        .and_then(|v| v.latex.clone())
}

fn run_case(c: &Case) {
    let id = c.id;
    let mut ed = Editor::new("");
    let sid = insert_into(&mut ed, id, c.knobs);
    let s = ed.session(sid).expect("session").clone();

    // Boxes: count, Tab order, labels, defaults, required.
    let got: Vec<(String, String, bool)> = s
        .tab_boxes()
        .map(|b| {
            (
                b.label.clone(),
                ed.text()[b.range.clone()].to_string(),
                b.required,
            )
        })
        .collect();
    let want: Vec<(String, String, bool)> = c
        .boxes
        .iter()
        .map(|(l, d, r)| (l.to_string(), d.to_string(), *r))
        .collect();
    assert_eq!(
        got, want,
        "{id}: boxes (label, default, required) in Tab order"
    );
    assert_eq!(c.fills.len(), c.boxes.len(), "{id}: one fill per box");

    // The caret starts in the primary box (the integrand, the term …).
    let caret = ed.selection().map_or(ed.caret(), |r| r.start);
    let primary = s.primary.expect("primary box");
    assert_eq!(
        s.box_at(caret),
        Some(primary),
        "{id}: caret starts in its first box"
    );

    // Every empty box shows its label, typeset where the maths puts it.
    let page = page_of(&ed);
    let slots = slot_hits(&page);
    let mut empty_labels: Vec<String> = s
        .boxes
        .iter()
        .filter(|b| b.mirror_of.is_none() && b.range.is_empty())
        .map(|b| b.label.clone())
        .collect();
    let mut shown_labels: Vec<String> = slots
        .iter()
        .map(|h| h.label.clone().unwrap_or_default())
        .collect();
    empty_labels.sort();
    shown_labels.sort();
    assert_eq!(
        shown_labels, empty_labels,
        "{id}: labels of the empty boxes"
    );
    assert_nothing_raw(id, &visible(&ed));

    // Tab through in order, filling each box.
    ed.focus_box(sid, s.tab[0]);
    for (k, ((label, default, _), fill)) in c.boxes.iter().zip(c.fills).enumerate() {
        let s = ed.session(sid).expect("session").clone();
        let at = ed.selection().map_or(ed.caret(), |r| r.start);
        let cur = s.box_at(at).map(|i| s.boxes[i].label.clone());
        assert_eq!(cur.as_deref(), Some(*label), "{id}: Tab stop {k}");
        // A default is pre-filled and selected, so typing replaces it.
        assert_eq!(
            ed.selected_text().unwrap_or(""),
            *default,
            "{id}: {label} selected"
        );
        if !fill.is_empty() {
            ed.insert_text(fill);
        }
        assert!(ed.tab(true), "{id}: Tab is handled in the template");
    }
    // Tab past the last box leaves the template.
    let s = ed.session(sid).expect("session").clone();
    assert_eq!(ed.caret(), s.range.end, "{id}: the caret left the template");

    let text = ed.text().to_string();
    let expected = if find(id).is_some_and(|t| t.is_figure()) {
        c.text.to_string()
    } else {
        format!("$$ {} $$", c.text)
    };
    assert_eq!(text, expected, "{id}: the .mpp text");
    let doc = parse_document(&text);
    let errors: Vec<String> = doc.errors().map(|d| d.message.clone()).collect();
    assert!(errors.is_empty(), "{id}: {text:?} parses, but: {errors:?}");
    assert!(ed.slot_labels().is_empty(), "{id}: no empty box is left");
    assert_nothing_raw(id, &visible(&ed));
    if find(id).is_some_and(|t| t.is_figure()) {
        assert!(
            matches!(
                doc.blocks.first().map(|b| &b.kind),
                Some(BlockKind::Figure(_))
            ),
            "{id}: a figure line"
        );
    }

    if let Some(a) = c.answer {
        let src = format!("$$ {} = ? $$\n", c.text);
        assert_eq!(answer(&src).as_deref(), Some(a), "{id}: the answer");
    }
}

#[test]
fn every_template_matches_the_slot_table() {
    let all = cases();
    for c in &all {
        run_case(c);
    }
    // Every template with boxes in the catalogue has a row (helpers outside the
    // table are checked by the generic test below).
    for t in catalogue() {
        if t.is_symbol() || t.is_text() || t.id.starts_with("helper.") {
            continue;
        }
        assert!(
            all.iter().any(|c| c.id == t.id),
            "{} has no row in the slot table test",
            t.id
        );
    }
}

#[test]
fn every_helper_inserts_tabs_and_parses() {
    for t in catalogue().iter().filter(|t| t.id.starts_with("helper.")) {
        let mut ed = Editor::new("");
        let sid = insert_into(&mut ed, &t.id, None);
        let s = ed.session(sid).expect("session").clone();
        assert!(!s.tab.is_empty(), "{}: has boxes", t.id);
        assert!(
            s.tab_boxes().any(|b| b.required),
            "{}: something is required",
            t.id
        );
        ed.focus_box(sid, s.tab[0]);
        for _ in 0..s.tab.len() {
            if ed.selected_text().unwrap_or("").is_empty() {
                ed.insert_text("2");
            }
            ed.tab(true);
        }
        let doc = parse_document(ed.text());
        assert!(doc.errors().next().is_none(), "{}: {:?}", t.id, ed.text());
        assert_nothing_raw(&t.id, &visible(&ed));
    }
}

#[test]
fn greek_and_symbols_insert_as_typeset_maths() {
    for t in catalogue().iter().filter(|t| t.is_symbol()) {
        // In maths: the symbol itself.
        let mut ed = Editor::new("$$ x $$");
        ed.move_caret(4, false);
        let page = page_of(&ed);
        insert::insert(&mut ed, &page, t, t.knobs, &BuildCx::default());
        assert!(ed.text().contains(t.icon.as_str()), "{}", t.id);
        // In prose: inline maths.
        let mut ed = Editor::new("Hello ");
        ed.move_caret(6, false);
        let page = page_of(&ed);
        insert::insert(&mut ed, &page, t, t.knobs, &BuildCx::default());
        assert_eq!(ed.text(), format!("Hello ${}$", t.icon), "{}", t.id);
        let shown = visible(&ed);
        assert!(!shown.contains('$'), "{}: {shown}", t.id);
    }
}

// ----- 2D layout: boxes sit where the maths puts them -----

fn glyph_box(page: &Page, ch: char) -> (f32, f32, f32) {
    page.blocks
        .iter()
        .flat_map(|b| {
            b.dl.items.iter().filter_map(move |p| match p {
                Primitive::Glyph {
                    ch: c, x, y, size, ..
                } if *c == ch => Some((*x, y + b.top, *size)),
                _ => None,
            })
        })
        .next()
        .unwrap_or_else(|| panic!("no {ch} on the page"))
}

fn slot(page: &Page, label: &str) -> HitBox {
    slot_hits(page)
        .into_iter()
        .find(|h| h.label.as_deref() == Some(label))
        .unwrap_or_else(|| panic!("no {label} box"))
}

#[test]
fn integral_bounds_sit_above_and_below_the_sign() {
    let mut ed = Editor::new("");
    insert_into(&mut ed, "int.def", None);
    let page = page_of(&ed);
    let (sx, sy, size) = glyph_box(&page, '∫');
    let lower = slot(&page, "lower");
    let upper = slot(&page, "upper");
    let integrand = slot(&page, "integrand");
    // Above and below the sign, centred on it.
    assert!(
        upper.rect.y + upper.rect.h < sy - size * 0.5,
        "upper is above"
    );
    assert!(lower.rect.y > sy, "lower is below the baseline");
    let mid = |h: &HitBox| h.rect.x + h.rect.w / 2.0;
    // (The upper bound leans right by the sign's italic correction, as in TeX.)
    assert!(
        (mid(&upper) - mid(&lower)).abs() < size * 0.6,
        "bounds are stacked"
    );
    assert!(
        lower.rect.x < sx + size && mid(&lower) > sx - size,
        "over the sign"
    );
    // Integrand to the right.
    assert!(integrand.rect.x > lower.rect.x + lower.rect.w);
    // The side setting puts them at the top-right and bottom-right.
    let side = page_with(&ed, IntegralLimits::Side);
    let (sx2, _, _) = glyph_box(&side, '∫');
    let lo = slot(&side, "lower");
    let up = slot(&side, "upper");
    assert!(lo.rect.x > sx2 && up.rect.x > sx2, "beside the sign");
    assert!(up.rect.y < lo.rect.y);
}

#[test]
fn sums_limits_fractions_roots_powers_matrices_and_cases_lay_out_in_2d() {
    // ∑: end above, n = start below, term to the right.
    let mut ed = Editor::new("");
    insert_into(&mut ed, "sum", None);
    let p = page_of(&ed);
    let (_, sy, _) = glyph_box(&p, '∑');
    let (_, iy, _) = glyph_box(&p, '∞');
    let (_, ny, _) = glyph_box(&p, '𝑛');
    assert!(iy < sy - 10.0, "end above the sign");
    assert!(ny > sy, "n = 1 below the sign");
    assert!(slot(&p, "term").rect.x > glyph_box(&p, '∑').0);

    // lim: x → □ below the word.
    let mut ed = Editor::new("");
    insert_into(&mut ed, "lim", None);
    let p = page_of(&ed);
    let (_, ly, _) = glyph_box(&p, 'm');
    assert!(slot(&p, "value").rect.y > ly, "x → value under lim");

    // Fraction: numerator over the bar, denominator under it.
    let mut ed = Editor::new("");
    insert_into(&mut ed, "frac", None);
    let p = page_of(&ed);
    let (n, d) = (slot(&p, "numerator"), slot(&p, "denominator"));
    assert!(
        n.rect.y + n.rect.h <= d.rect.y,
        "numerator over denominator"
    );

    // Root: the radicand under the radical sign; power: exponent raised.
    let mut ed = Editor::new("");
    insert_into(&mut ed, "sqrt", None);
    let p = page_of(&ed);
    assert!(slot(&p, "radicand").rect.x > glyph_box(&p, '√').0);
    let mut ed = Editor::new("");
    insert_into(&mut ed, "pow", None);
    let p = page_of(&ed);
    let (b, e) = (slot(&p, "base"), slot(&p, "exponent"));
    assert!(e.rect.y + e.rect.h < b.rect.y + b.rect.h, "exponent raised");
    assert!(e.rect.h < b.rect.h, "exponent smaller");

    // Matrix: a real grid.
    let mut ed = Editor::new("");
    insert_into(&mut ed, "matrix", Some(knobs(2, 3, 0, 0, false)));
    let p = page_of(&ed);
    let cells = slot_hits(&p);
    assert_eq!(cells.len(), 6);
    let mut ys: Vec<i32> = cells.iter().map(|h| h.rect.y.round() as i32).collect();
    let mut xs: Vec<i32> = cells.iter().map(|h| h.rect.x.round() as i32).collect();
    ys.sort_unstable();
    ys.dedup();
    xs.sort_unstable();
    xs.dedup();
    assert_eq!((ys.len(), xs.len()), (2, 3), "2 rows × 3 columns");

    // Piecewise: value/condition rows.
    let mut ed = Editor::new("");
    insert_into(&mut ed, "cases", None);
    let p = page_of(&ed);
    let v = slot_hits(&p);
    let values: Vec<&HitBox> = v
        .iter()
        .filter(|h| h.label.as_deref() == Some("value"))
        .collect();
    assert_eq!(values.len(), 2);
    assert!(values[1].rect.y > values[0].rect.y + 4.0, "rows stack");
    assert!(slot(&p, "condition").rect.x > values[0].rect.x);
}

// ----- box rules -----

#[test]
fn backspace_in_an_empty_first_box_removes_an_empty_template_only() {
    // All empty: the template (and its maths block) goes.
    let mut ed = Editor::new("");
    insert_into(&mut ed, "frac", None);
    assert!(ed.backspace_in_template());
    assert_eq!(ed.text(), "");
    // Undo brings it back in one step.
    assert!(ed.undo());
    assert_eq!(ed.text(), r"$$ \frac{}{} $$");

    // Something typed: Backspace in the empty second box moves back one box.
    let mut ed = Editor::new("");
    let id = insert_into(&mut ed, "frac", None);
    ed.insert_text("1");
    ed.tab(true);
    assert!(ed.backspace_in_template());
    assert_eq!(ed.text(), r"$$ \frac{1}{} $$");
    let s = ed.session(id).expect("session");
    assert_eq!(
        s.box_at(ed.caret()),
        Some(s.tab[0]),
        "back in the numerator"
    );
    // At the start of the (filled) first box it leaves the template, keeping it.
    ed.move_caret(s.boxes[s.tab[0]].range.start, false);
    assert!(ed.backspace_in_template());
    assert_eq!(ed.text(), r"$$ \frac{1}{} $$");

    // Defaults count as empty: in an untouched sum, clearing the index (its
    // first box) and pressing Backspace removes the whole sum.
    let mut ed = Editor::new("");
    let id = insert_into(&mut ed, "sum", None);
    let first = ed.session(id).expect("s").tab[0];
    ed.focus_box(id, first);
    assert_eq!(ed.selected_text(), Some("n"));
    let c = ed.caret();
    ed.delete(c, false);
    assert_eq!(
        ed.text(),
        r"$$ \sum_{{}={1}}^{\infty} {} $$",
        "an empty bare box gets braces"
    );
    assert!(ed.backspace_in_template());
    assert_eq!(ed.text(), "");
}

#[test]
fn shift_tab_goes_back_and_templates_nest() {
    let mut ed = Editor::new("");
    let outer = insert_into(&mut ed, "sum", None);
    // The caret is in the term: put an integral inside it.
    let inner = insert_into(&mut ed, "int.def", None);
    assert_ne!(outer, inner);
    ed.insert_text("x");
    // Shift+Tab: back to the inner template's upper bound.
    assert!(ed.tab(false));
    let s = ed.session(inner).expect("inner");
    let at = ed.caret();
    assert_eq!(
        s.box_at(at).map(|i| s.boxes[i].label.as_str()),
        Some("upper")
    );
    // Forward past the inner template, into the outer sum's term again.
    ed.tab(true);
    ed.tab(true);
    ed.tab(true);
    let o = ed.session(outer).expect("outer");
    assert_eq!(
        o.box_at(ed.caret()).map(|i| o.boxes[i].label.as_str()),
        Some("term")
    );
    // Leaving drops the inner integral's empty bounds: an indefinite integral.
    assert!(ed.text().contains(r"\int {x} \, dx"), "{}", ed.text());
    ed.tab(true);
    let doc = parse_document(ed.text());
    assert!(doc.errors().next().is_none(), "{}", ed.text());
}

/// The label of the box the caret (or selection) is in.
fn label_now(ed: &Editor, sid: u64) -> String {
    let s = ed.session(sid).expect("session");
    let at = ed.selection().map_or(ed.caret(), |r| r.start);
    s.box_at(at)
        .map(|i| s.boxes[i].label.clone())
        .unwrap_or_default()
}

/// Tab (or Shift+Tab) `n` times, giving the label after each press.
fn walk(ed: &mut Editor, sid: u64, forward: bool, n: usize) -> Vec<String> {
    (0..n)
        .map(|_| {
            assert!(ed.tab(forward));
            label_now(ed, sid)
        })
        .collect()
}

#[test]
fn shift_tab_is_exactly_tab_backwards_in_every_template() {
    let mut runs: Vec<(String, Option<Knobs>)> = catalogue()
        .iter()
        .filter(|t| !t.is_symbol() && !t.is_text())
        .map(|t| (t.id.clone(), None))
        .collect();
    for id in ["int.double", "int.triple"] {
        runs.push((id.into(), Some(knobs(0, 0, 1, 0, false))));
    }
    for (id, kn) in runs {
        let mut ed = Editor::new("");
        let sid = insert_into(&mut ed, &id, kn);
        let s = ed.session(sid).expect("session").clone();
        let order: Vec<String> = s.tab_boxes().map(|b| b.label.clone()).collect();
        let n = order.len();
        if n == 0 {
            continue;
        }
        // Forward from the first box: every box in order, then out.
        ed.focus_box(sid, s.tab[0]);
        let fwd = walk(&mut ed, sid, true, n - 1);
        assert_eq!(fwd, order[1..].to_vec(), "{id}: Tab order");
        ed.tab(true);
        let end = ed.session(sid).expect("session").range.end;
        assert_eq!(ed.caret(), end, "{id}: Tab past the last box leaves");

        // Backward from the last box: the same boxes reversed, then out.
        let mut ed = Editor::new("");
        let sid = insert_into(&mut ed, &id, kn);
        let s = ed.session(sid).expect("session").clone();
        ed.focus_box(sid, s.tab[n - 1]);
        let back = walk(&mut ed, sid, false, n - 1);
        let mut want: Vec<String> = order[..n - 1].to_vec();
        want.reverse();
        assert_eq!(back, want, "{id}: Shift+Tab is Tab reversed");
        ed.tab(false);
        let start = ed.session(sid).expect("session").range.start;
        assert_eq!(
            ed.caret(),
            start,
            "{id}: Shift+Tab before the first box leaves"
        );
    }
}

#[test]
fn integrals_tab_from_the_integrand_through_the_slot_table_order() {
    let bounds = Some(knobs(0, 0, 1, 0, false));
    for (id, kn) in [
        ("int.def", None),
        ("int.improper", None),
        ("int.double", None),
        ("int.double", bounds),
        ("int.triple", None),
        ("int.triple", bounds),
    ] {
        let mut ed = Editor::new("");
        let sid = insert_into(&mut ed, id, kn);
        let s = ed.session(sid).expect("session").clone();
        let order: Vec<String> = s.tab_boxes().map(|b| b.label.clone()).collect();
        // The caret starts in the integrand …
        assert_eq!(
            label_now(&ed, sid),
            "integrand",
            "{id}: starts in the integrand"
        );
        // … and Tab walks the table's order from its first box (bounds or
        // region, integrand, differential), then leaves.
        let got = walk(&mut ed, sid, true, order.len());
        assert_eq!(got, order, "{id}: Tab from the start");
        ed.tab(true);
        let end = ed.session(sid).expect("session").range.end;
        assert_eq!(ed.caret(), end, "{id}: then out of the template");
    }
    // Shift+Tab from the integrand goes back through the bounds, then out.
    let mut ed = Editor::new("");
    let sid = insert_into(&mut ed, "int.def", None);
    assert_eq!(walk(&mut ed, sid, false, 2), ["upper", "lower"]);
    // A sum's index and limits have defaults: Tab from the term leaves.
    let mut ed = Editor::new("");
    let sid = insert_into(&mut ed, "sum", None);
    ed.insert_text("n");
    ed.tab(true);
    let end = ed.session(sid).expect("session").range.end;
    assert_eq!(ed.caret(), end, "sum: Tab from the term leaves");
}

#[test]
fn typing_after_leaving_a_template_stays_outside_it() {
    let mut ed = Editor::new("");
    let id = insert_into(&mut ed, "int.indef", None);
    ed.insert_text("x");
    ed.tab(true);
    ed.tab(true);
    for c in [" ", "=", " ", "?"] {
        ed.insert_text(c);
    }
    assert_eq!(ed.text(), r"$$ \int {x} \, dx = ? $$");
    let s = ed.session(id).expect("session");
    let var = &s.boxes[s.tab[1]];
    assert_eq!(
        &ed.text()[var.range.clone()],
        "x",
        "the variable box keeps only x"
    );
    assert!(ed.session_at(ed.caret()).is_none(), "the caret is outside");
    // Going back into the last box and typing does extend it.
    let end = var.range.end;
    ed.move_caret(end, false);
    ed.insert_text("y");
    let s = ed.session(id).expect("session");
    assert_eq!(&ed.text()[s.boxes[s.tab[1]].range.clone()], "xy");
}

#[test]
fn the_differential_uses_the_current_functions_variable() {
    let mut ed = Editor::new("$$ f(t) := t^2 $$\n\n");
    ed.move_caret(ed.text().len(), false);
    let t = find("int.def").expect("t");
    let page = page_of(&ed);
    let cx = insert::InsertSettings::default().build_cx(ed.text(), ed.caret());
    insert::insert(&mut ed, &page, t, t.knobs, &cx);
    assert!(
        ed.text().ends_with(r"$$ \int_{}^{} {} \, dt $$"),
        "{}",
        ed.text()
    );
}

#[test]
fn inserting_into_prose_makes_a_maths_block() {
    let mut ed = Editor::new("Work out the area.");
    ed.move_caret(ed.text().len(), false);
    insert_into(&mut ed, "frac", None);
    assert_eq!(ed.text(), "Work out the area.\n\n$$ \\frac{}{} $$\n");
    let doc = parse_document(ed.text());
    assert!(matches!(doc.blocks[1].kind, BlockKind::DisplayMath(_)));
    // In the middle of a line the line is split around the block.
    let mut ed = Editor::new("Before after");
    ed.move_caret(6, false);
    insert_into(&mut ed, "sqrt", None);
    let doc = parse_document(ed.text());
    assert_eq!(doc.blocks.len(), 3, "{}", ed.text());
    // A graph is a Figure line after the block the caret is in.
    let mut ed = Editor::new("$$ x $$\n");
    ed.move_caret(4, false);
    insert_into(&mut ed, "graph.2d", None);
    let doc = parse_document(ed.text());
    assert!(
        matches!(doc.blocks[1].kind, BlockKind::Figure(_)),
        "{}",
        ed.text()
    );
}

#[test]
fn selected_maths_goes_into_the_first_box() {
    let mut ed = Editor::new("$$ x + 1 $$");
    ed.select(3..8);
    insert_into(&mut ed, "sqrt", None);
    assert_eq!(ed.text(), r"$$ \sqrt{x + 1} $$");
}

#[test]
fn a_template_insert_is_one_undo_step() {
    let mut ed = Editor::new("$$ a $$");
    ed.move_caret(4, false);
    insert_into(&mut ed, "int.def", None);
    ed.insert_text("x");
    assert!(ed.undo());
    assert!(ed.undo());
    assert_eq!(ed.text(), "$$ a $$");
    assert!(ed.redo());
    assert!(ed.text().contains(r"\int"));
    assert!(
        !ed.sessions().is_empty(),
        "the boxes come back with the redo"
    );
}

#[test]
fn typing_shortcuts_make_integrals_and_fractions() {
    let cx = BuildCx::default();
    let mut ed = Editor::new("$$  $$");
    ed.move_caret(3, false);
    for ch in "int ".chars() {
        ed.insert_text(&ch.to_string());
        insert::shortcut(&mut ed, ch, &cx);
    }
    assert_eq!(ed.text(), r"$$ \int_{}^{} {} \, dx $$");
    let mut ed = Editor::new("$$  $$");
    ed.move_caret(3, false);
    for ch in "12/".chars() {
        ed.insert_text(&ch.to_string());
        insert::shortcut(&mut ed, ch, &cx);
    }
    assert_eq!(ed.text(), r"$$ \frac{12}{} $$");
    ed.insert_text("5");
    assert_eq!(
        ed.text(),
        r"$$ \frac{12}{5} $$",
        "the caret was in the denominator"
    );
    // A word that merely ends in "int" is left alone.
    let mut ed = Editor::new("$$  $$");
    ed.move_caret(3, false);
    for ch in "point ".chars() {
        ed.insert_text(&ch.to_string());
        assert!(!insert::shortcut(&mut ed, ch, &cx));
    }
}

// ----- controls -----

fn control(id: &str, knobs: Knobs, label: &str) -> Action {
    controls(id, knobs)
        .into_iter()
        .find(|c| c.label == label)
        .unwrap_or_else(|| panic!("{id} has no {label} control"))
        .action
}

#[test]
fn matrix_rows_and_columns_can_be_added_and_removed() {
    let mut ed = Editor::new("");
    let id = insert_into(&mut ed, "matrix", None);
    ed.insert_text("1");
    let k = ed.session(id).expect("s").knobs;
    let id = apply(
        &mut ed,
        id,
        &control("matrix", k, "+ column"),
        &BuildCx::default(),
    )
    .expect("rebuilt");
    assert_eq!(
        ed.text(),
        r"$$ \begin{pmatrix} {1} & {} & {} \\ {} & {} & {} \end{pmatrix} $$"
    );
    let k = ed.session(id).expect("s").knobs;
    let id = apply(
        &mut ed,
        id,
        &control("matrix", k, "+ row"),
        &BuildCx::default(),
    )
    .expect("rebuilt");
    let k = ed.session(id).expect("s").knobs;
    assert_eq!((k.rows, k.cols), (3, 3));
    apply(
        &mut ed,
        id,
        &control("matrix", k, "\u{2212} row"),
        &BuildCx::default(),
    );
    assert!(
        ed.text().contains(r"{1} & {} & {} \\ {} & {} & {} \end"),
        "{}",
        ed.text()
    );
}

#[test]
fn double_integrals_switch_mode_and_swap_order() {
    let cx = BuildCx::default();
    let mut ed = Editor::new("");
    let id = insert_into(&mut ed, "int.double", None);
    ed.insert_text("x y");
    let k = ed.session(id).expect("s").knobs;
    let id = apply(&mut ed, id, &control("int.double", k, "Bounds"), &cx).expect("bounds");
    assert_eq!(
        ed.text(),
        r"$$ \int_{}^{} \int_{}^{} {x y} \, dy \, dx $$",
        "the integrand is kept"
    );
    let k = ed.session(id).expect("s").knobs;
    let swap = control("int.double", k, "Swap order");
    let s = ed.session(id).expect("s").clone();
    let lower = s.tab[0];
    ed.focus_box(id, lower);
    ed.insert_text("0");
    let id = apply(&mut ed, id, &swap, &cx).expect("swapped");
    assert_eq!(ed.text(), r"$$ \int_{}^{} \int_{0}^{} {x y} \, dx \, dy $$");
    let k = ed.session(id).expect("s").knobs;
    apply(&mut ed, id, &control("int.double", k, "Region"), &cx);
    assert_eq!(ed.text(), r"$$ \iint_{} {x y} \, dA $$");
}

#[test]
fn triple_integral_coordinates_set_the_volume_element() {
    let cx = BuildCx::default();
    let mut ed = Editor::new("");
    let id = insert_into(&mut ed, "int.triple", None);
    let k = ed.session(id).expect("s").knobs;
    let id = apply(&mut ed, id, &control("int.triple", k, "Cylindrical"), &cx).expect("c");
    assert_eq!(ed.text(), r"$$ \iiint_{} {} \, r \, dz \, dr \, d\theta $$");
    let k = ed.session(id).expect("s").knobs;
    apply(&mut ed, id, &control("int.triple", k, "Spherical"), &cx);
    assert_eq!(
        ed.text(),
        r"$$ \iiint_{} {} \, \rho^{2} \sin\phi \, d\rho \, d\phi \, d\theta $$"
    );
    let errors: Vec<String> = parse_document(ed.text())
        .errors()
        .map(|d| d.message.clone())
        .collect();
    assert!(
        errors
            .iter()
            .all(|m| m.contains("inside") || m.contains("after `_`")),
        "only the empty boxes: {errors:?}"
    );
}

#[test]
fn infinity_buttons_limit_sides_rows_and_conditions() {
    let cx = BuildCx::default();
    // ±∞ fill the bound the caret is in (or the matching one).
    let mut ed = Editor::new("");
    let id = insert_into(&mut ed, "int.improper", None);
    let k = Knobs::default();
    apply(
        &mut ed,
        id,
        &control("int.improper", k, "\u{2212}\u{221e}"),
        &cx,
    );
    apply(&mut ed, id, &control("int.improper", k, "+\u{221e}"), &cx);
    assert_eq!(ed.text(), r"$$ \int_{-\infty}^{\infty} {} \, dx $$");

    // The side of a limit.
    let mut ed = Editor::new("");
    let id = insert_into(&mut ed, "lim", None);
    let k = ed.session(id).expect("s").knobs;
    apply(&mut ed, id, &control("lim", k, "From below"), &cx);
    assert_eq!(ed.text(), r"$$ \lim_{x \to {}^{-}} {} $$");

    // Piecewise rows and the otherwise row.
    let mut ed = Editor::new("");
    let id = insert_into(&mut ed, "cases", None);
    ed.insert_text("1");
    let k = ed.session(id).expect("s").knobs;
    let id = apply(&mut ed, id, &control("cases", k, "+ row"), &cx).expect("row");
    assert_eq!(
        ed.text(),
        r"$$ \begin{cases} {1} & {} \\ {} & {} \\ {} & \text{otherwise} \end{cases} $$"
    );
    let k = ed.session(id).expect("s").knobs;
    apply(&mut ed, id, &control("cases", k, "Otherwise"), &cx);
    assert_eq!(
        ed.text(),
        r"$$ \begin{cases} {1} & {} \\ {} & {} \end{cases} $$"
    );

    // ODE initial conditions: add a row (y′ for the second).
    let mut ed = Editor::new("");
    let id = insert_into(&mut ed, "ode.ivp", None);
    ed.insert_text("y'' + y = 0");
    let k = ed.session(id).expect("s").knobs;
    apply(&mut ed, id, &control("ode.ivp", k, "+ condition"), &cx);
    assert_eq!(
        ed.text(),
        r"$$ \operatorname{solve}_{y(x)} \left( {y'' + y = 0} \right), \quad y({}) = {}, \quad y'({}) = {} $$"
    );

    // Renaming the unknown function renames it in the conditions too.
    let s = ed.sessions().last().expect("s").clone();
    let unknown = s.tab[1];
    ed.focus_box(s.id, unknown);
    ed.insert_text("u");
    assert!(
        ed.text().contains(r"\quad u({}) = {}, \quad u'({})"),
        "{}",
        ed.text()
    );
}

#[test]
fn empty_required_boxes_give_a_fill_in_hint_and_no_answer() {
    let mut ed = Editor::new("");
    insert_into(&mut ed, "int.def", None);
    let page = page_of(&ed);
    let b = &page.blocks[0];
    let hint = insert::fill_in_hint(&ed, &page, &(b.span.start..b.span.end));
    assert_eq!(hint.as_deref(), Some("fill in: integrand"));
    // Optional bounds are not asked for; once the integrand is there, nothing is.
    ed.insert_text("x");
    let page = page_of(&ed);
    let b = &page.blocks[0];
    assert_eq!(
        insert::fill_in_hint(&ed, &page, &(b.span.start..b.span.end)),
        None
    );
    // An improper integral needs its bounds.
    let mut ed = Editor::new("");
    insert_into(&mut ed, "int.improper", None);
    ed.insert_text("x");
    let page = page_of(&ed);
    let b = &page.blocks[0];
    assert_eq!(
        insert::fill_in_hint(&ed, &page, &(b.span.start..b.span.end)).as_deref(),
        Some("fill in: lower bound")
    );
    // No answer while a box is empty.
    assert_eq!(
        answer(&format!("{} = ? $$", ed.text().trim_end_matches(" $$"))),
        None
    );
}

#[test]
fn a_footnote_puts_its_mark_at_the_caret_and_its_note_after_the_paragraph() {
    let src = "Water boils at 100 degrees.\n\nNext paragraph.";
    let mut ed = Editor::new(src);
    ed.move_caret(src.find('.').expect("dot"), false);
    insert_into_block(&mut ed, "doc.footnote");
    assert_eq!(
        ed.text(),
        "Water boils at 100 degrees[^1].\n\n[^1]: \n\nNext paragraph."
    );
    // The caret is in the note, ready to type it.
    assert_eq!(ed.caret(), ed.text().find("[^1]: ").expect("note") + 6);
    let doc = parse_document(ed.text());
    assert!(doc.errors().next().is_none(), "{}", ed.text());
    assert!(doc
        .blocks
        .iter()
        .any(|b| matches!(b.kind, BlockKind::FootnoteDef { .. })));
    // The next one takes the next number; from an empty line the mark goes at
    // the end of the paragraph above.
    let src = format!("{}\n\nMore text\n\n", ed.text());
    let mut ed = Editor::new(&src);
    ed.move_caret(src.len(), false);
    insert_into_block(&mut ed, "doc.footnote");
    assert!(
        ed.text().contains("More text[^2]\n\n[^2]: "),
        "{}",
        ed.text()
    );
    // Inside a word, the mark goes after it.
    let mut ed = Editor::new("Some prose here.");
    ed.move_caret(7, false);
    insert_into_block(&mut ed, "doc.footnote");
    assert_eq!(ed.text(), "Some prose[^1] here.\n\n[^1]: \n");
}

fn insert_into_block(ed: &mut Editor, id: &str) {
    let t = find(id).unwrap_or_else(|| panic!("no template {id}"));
    let page = page_of(ed);
    insert::insert(ed, &page, t, t.knobs, &BuildCx::default());
}

#[test]
fn figure_lines_name_their_graphs_in_words() {
    for (id, want, gone) in [
        ("graph.riemann", "midpoint rule,", "riemann"),
        ("graph.trapezoid", "trapezoid rule,", "riemann"),
        ("graph.taylor", "Taylor polynomial of degree", "taylor"),
        ("graph.newton", "Newton's method from", "newton"),
        ("graph.param", "Parametric curve", "parametric"),
        ("graph.slope", "Slope field of", "slope field"),
        ("graph.contour", "contour plot", "xx"),
        ("graph.scatter", "Scatter plot of", "scatter"),
        ("graph.histogram", "Histogram of", "histogram"),
        ("graph.boxplot", "Box plot of", "box plot"),
    ] {
        let mut ed = Editor::new("");
        insert_into(&mut ed, id, None);
        let shown = visible(&ed);
        let squash = |s: &str| s.replace(" ", "");
        assert!(shown.contains(&squash(want)), "{id}: {shown:?}");
        assert!(!shown.contains(&squash(gone)), "{id}: {shown:?}");
    }
}

#[test]
fn the_toolbar_clears_the_words_that_start_a_figure_line() {
    for id in ["graph.param", "graph.slope"] {
        let mut ed = Editor::new("");
        let sid = insert_into(&mut ed, id, None);
        let s = ed.session(sid).expect("session").clone();
        let page = page_of(&ed);
        let span = mpp_syntax::Span::new(s.range.start, s.range.end);
        let boxes = page.rect_of_span(span).expect("rect");
        let ink = page.line_ink(span, boxes);
        // The heading words sit left of the first box and are now included.
        assert!(
            ink.left() < boxes.left() - 20.0,
            "{id}: {ink:?} vs {boxes:?}"
        );
        let bounds = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(900.0, 800.0));
        let width = 110.0;
        let (at, pivot) = mpp_ide::insert::menu::toolbar_anchor(ink, width, bounds);
        let left = if pivot == egui::Align2::RIGHT_TOP {
            at.x - width
        } else {
            at.x
        };
        let bar = egui::Rect::from_min_size(egui::pos2(left, at.y), egui::vec2(width, 60.0));
        assert!(
            !bar.intersects(ink),
            "{id}: toolbar {bar:?} covers the line {ink:?}"
        );
    }
}

#[test]
fn helper_templates_read_as_sentences_with_their_options_in_words() {
    for (id, want) in [
        ("helper.riemann", "MidpointRiemannsumof"),
        ("helper.volume", "(methodchosenautomatically)"),
        ("helper.rk4", "RK4solutionof"),
        ("helper.newton", "Newton'smethodfor"),
    ] {
        let mut ed = Editor::new("");
        insert_into(&mut ed, id, None);
        let shown = visible(&ed);
        assert!(shown.contains(want), "{id}: {shown:?}");
        for word in [",mid", ",auto", "(□", "riemann"] {
            assert!(!shown.contains(word), "{id}: {word} in {shown:?}");
        }
    }
    // The rule buttons show which rule is in use.
    let mut ed = Editor::new("");
    let sid = insert_into(&mut ed, "helper.riemann", None);
    let s = ed.session(sid).expect("session").clone();
    let mut c = mpp_ide::insert::controls(&s.template, s.knobs);
    mpp_ide::insert::mark_current(&mut c, &s.contents(ed.text()));
    let pressed: Vec<&str> = c
        .iter()
        .filter(|c| c.active)
        .map(|c| c.label.as_str())
        .collect();
    assert_eq!(pressed, ["Midpoint"]);
}
