//! How a named operator is shown on the page. The file keeps the short operator name
//! (`\operatorname{crit}`, `\mathcal{L}` read as `laplace`); readers see textbook
//! notation: ℒ{f}, "Critical points of f on [0, 4]", "Taylor bound", "RK4".
//!
//! The IDE, the HTML and PDF export (both drawn by the renderer) and the TeX export all
//! read this one table, so a helper is named the same way everywhere.

/// One part of a [`OpDisplay::Phrase`] or [`OpDisplay::Sentence`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Piece {
    /// Upright words.
    Text(&'static str),
    /// The operand(s), without brackets.
    Args,
    /// The operand at this place (0 = the first).
    Arg(usize),
    /// The operand at this place when it is one of the option words, shown as the
    /// words it maps to (`mid` → "Midpoint Riemann sum of"). Anything else (an empty
    /// box while editing) is shown as maths between the two fallback texts.
    Choice(
        usize,
        &'static [(&'static str, &'static str)],
        &'static str,
        &'static str,
    ),
    /// The subscript at full size, after the given words; skipped when absent.
    Sub(&'static str),
    /// The superscript at full size, after the given words; skipped when absent.
    Sup(&'static str),
    /// The subscript and superscript as an interval `[a, b]` after the words;
    /// skipped unless both are present.
    Interval(&'static str),
    /// A maths variable in italics (`n`, `h`).
    Var(&'static str),
}

/// The words an option word maps to in a [`Piece::Choice`] table.
pub fn choice_words(table: &[(&str, &'static str)], word: &str) -> Option<&'static str> {
    table.iter().find(|(w, _)| *w == word).map(|(_, s)| *s)
}

/// The pieces to draw `name` with when it has `args` operands, if it is drawn as
/// a sentence.
pub fn sentence(name: &str, args: usize) -> Option<&'static [Piece]> {
    match operator_display(name)? {
        OpDisplay::Phrase(p, _) if args == 1 => Some(p),
        OpDisplay::Sentence(p, n, _) if args == n => Some(p),
        _ => None,
    }
}

/// The sentence for `name` as plain Unicode text, from its scripts and operands
/// already written as text (for the command line). `None` when it is not drawn
/// as a sentence with this many operands.
pub fn sentence_text(
    name: &str,
    sub: Option<&str>,
    sup: Option<&str>,
    args: &[String],
) -> Option<String> {
    let pieces = sentence(name, args.len())?;
    // The sentence fits only the argument order the Insert menu writes: no
    // interval among the arguments (the bounds are scripts) and option words
    // that are known.
    let fits = pieces.iter().all(|p| match *p {
        Arg(i) => args
            .get(i)
            .is_some_and(|a| !a.trim_start().starts_with('[')),
        Choice(i, table, _, _) => args
            .get(i)
            .is_some_and(|a| choice_words(table, a.trim()).is_some()),
        _ => true,
    });
    if !fits {
        return None;
    }
    let mut parts: Vec<String> = vec![];
    for p in pieces {
        match *p {
            Text(w) | Var(w) => parts.push(w.to_string()),
            Args => parts.push(args.join(", ")),
            Arg(i) => parts.push(args.get(i).cloned().unwrap_or_default()),
            Choice(i, table, before, after) => {
                let a = args.get(i).map(String::as_str).unwrap_or("");
                match choice_words(table, a.trim()) {
                    Some(w) => parts.push(w.to_string()),
                    None => {
                        parts.push(before.to_string());
                        parts.push(a.to_string());
                        parts.push(after.to_string());
                    }
                }
            }
            Sub(w) => {
                if let Some(s) = sub {
                    parts.push(w.to_string());
                    parts.push(s.to_string());
                }
            }
            Sup(w) => {
                if let Some(s) = sup {
                    parts.push(w.to_string());
                    parts.push(s.to_string());
                }
            }
            Interval(w) => {
                if let (Some(a), Some(b)) = (sub, sup) {
                    parts.push(w.to_string());
                    parts.push(format!("[{a}, {b}]"));
                }
            }
        }
    }
    let mut out = String::new();
    for part in parts.iter().filter(|p| !p.is_empty()) {
        if !out.is_empty() && !part.starts_with([',', ':', '?', '.']) {
            out.push(' ');
        }
        out.push_str(part);
    }
    Some(out)
}

/// How an operator is drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpDisplay {
    /// A transform: a script letter (ℒ, 𝓕), `⁻¹` when inverse, the operand in braces.
    Transform { letter: char, inverse: bool },
    /// A name in words; scripts stay scripts and the operands follow as written.
    Words(&'static str),
    /// A sentence with the single operand inside it ("Critical points of f on [0, 4]").
    /// With more than one operand the operator is drawn as [`OpDisplay::Words`] with
    /// the second field.
    Phrase(&'static [Piece], &'static str),
    /// A sentence for a helper with exactly this many operands ("Midpoint Riemann
    /// sum of f on [0, 2] with n = 4"); with any other number it is drawn as
    /// [`OpDisplay::Words`] with the last field.
    Sentence(&'static [Piece], usize, &'static str),
}

use Piece::{Arg, Args, Choice, Interval, Sub, Sup, Text, Var};

/// Riemann sum rules, as written by the Insert menu's rule buttons.
const RULES: &[(&str, &str)] = &[
    ("left", "Left Riemann sum of"),
    ("right", "Right Riemann sum of"),
    ("mid", "Midpoint Riemann sum of"),
    ("midpoint", "Midpoint Riemann sum of"),
    ("trap", "Trapezoid rule for"),
    ("trapezoid", "Trapezoid rule for"),
    ("trapezoidal", "Trapezoid rule for"),
    ("simpson", "Simpson's rule for"),
    ("Simpson", "Simpson's rule for"),
];

/// Volume of revolution methods.
const METHODS: &[(&str, &str)] = &[
    ("auto", "(method chosen automatically)"),
    ("disk", "by discs"),
    ("disc", "by discs"),
    ("washer", "by washers"),
    ("shell", "by shells"),
    ("shells", "by shells"),
];

/// Operator name → display.
const TABLE: &[(&str, OpDisplay)] = &[
    // Transforms.
    (
        "laplace",
        OpDisplay::Transform {
            letter: 'L',
            inverse: false,
        },
    ),
    (
        "invlaplace",
        OpDisplay::Transform {
            letter: 'L',
            inverse: true,
        },
    ),
    (
        "ilaplace",
        OpDisplay::Transform {
            letter: 'L',
            inverse: true,
        },
    ),
    (
        "inverselaplace",
        OpDisplay::Transform {
            letter: 'L',
            inverse: true,
        },
    ),
    (
        "fourier",
        OpDisplay::Transform {
            letter: 'F',
            inverse: false,
        },
    ),
    (
        "invfourier",
        OpDisplay::Transform {
            letter: 'F',
            inverse: true,
        },
    ),
    // Function analysis.
    (
        "domain",
        OpDisplay::Phrase(&[Text("Domain of"), Args], "Domain"),
    ),
    (
        "range",
        OpDisplay::Phrase(&[Text("Range of"), Args], "Range"),
    ),
    (
        "intercepts",
        OpDisplay::Phrase(&[Text("Intercepts of"), Args], "Intercepts"),
    ),
    (
        "crit",
        OpDisplay::Phrase(
            &[Text("Critical points of"), Args, Sub("on")],
            "Critical points",
        ),
    ),
    (
        "critical",
        OpDisplay::Phrase(
            &[Text("Critical points of"), Args, Sub("on")],
            "Critical points",
        ),
    ),
    (
        "extrema",
        OpDisplay::Phrase(&[Text("Extrema of"), Args, Sub("on")], "Extrema"),
    ),
    (
        "localextrema",
        OpDisplay::Phrase(
            &[Text("Local extrema of"), Args, Sub("on")],
            "Local extrema",
        ),
    ),
    (
        "absextrema",
        OpDisplay::Phrase(
            &[Text("Absolute extrema of"), Args, Sub("on")],
            "Absolute extrema",
        ),
    ),
    (
        "increasing",
        OpDisplay::Phrase(
            &[
                Text("Where"),
                Args,
                Text("increases and decreases"),
                Sub("on"),
            ],
            "Increasing and decreasing",
        ),
    ),
    (
        "decreasing",
        OpDisplay::Phrase(
            &[
                Text("Where"),
                Args,
                Text("increases and decreases"),
                Sub("on"),
            ],
            "Increasing and decreasing",
        ),
    ),
    (
        "monotonicity",
        OpDisplay::Phrase(
            &[
                Text("Where"),
                Args,
                Text("increases and decreases"),
                Sub("on"),
            ],
            "Increasing and decreasing",
        ),
    ),
    (
        "concavity",
        OpDisplay::Phrase(&[Text("Concavity of"), Args, Sub("on")], "Concavity"),
    ),
    (
        "inflection",
        OpDisplay::Phrase(&[Text("Inflection points of"), Args], "Inflection points"),
    ),
    (
        "asymptotes",
        OpDisplay::Phrase(&[Text("Asymptotes of"), Args], "Asymptotes"),
    ),
    (
        "tangent",
        OpDisplay::Phrase(&[Text("Tangent line to"), Args, Sub("at")], "Tangent line"),
    ),
    (
        "normal",
        OpDisplay::Phrase(&[Text("Normal line to"), Args, Sub("at")], "Normal line"),
    ),
    (
        "linearisation",
        OpDisplay::Phrase(
            &[Text("Linearisation of"), Args, Sub("at")],
            "Linearisation",
        ),
    ),
    (
        "linearization",
        OpDisplay::Phrase(
            &[Text("Linearisation of"), Args, Sub("at")],
            "Linearisation",
        ),
    ),
    (
        "linearise",
        OpDisplay::Phrase(
            &[Text("Linearisation of"), Args, Sub("at")],
            "Linearisation",
        ),
    ),
    (
        "mvt",
        OpDisplay::Phrase(
            &[Text("Mean value theorem for"), Args, Sub("on")],
            "Mean value theorem",
        ),
    ),
    (
        "ivt",
        OpDisplay::Phrase(
            &[
                Text("Intermediate value theorem for"),
                Args,
                Sub("on"),
                Sup(", value"),
            ],
            "Intermediate value theorem",
        ),
    ),
    (
        "analyse",
        OpDisplay::Phrase(&[Text("Analysis of"), Args], "Analysis"),
    ),
    (
        "analyze",
        OpDisplay::Phrase(&[Text("Analysis of"), Args], "Analysis"),
    ),
    // Series.
    (
        "series",
        OpDisplay::Phrase(
            &[Text("Taylor series of"), Args, Sub("at"), Sup(", order")],
            "Taylor series",
        ),
    ),
    (
        "taylor",
        OpDisplay::Phrase(
            &[Text("Taylor series of"), Args, Sub("at"), Sup(", order")],
            "Taylor series",
        ),
    ),
    (
        "taylorbound",
        OpDisplay::Sentence(
            &[
                Text("Error bound for the degree"),
                Arg(2),
                Text("Taylor polynomial of"),
                Arg(0),
                Text("about"),
                Arg(1),
                Text(", at"),
                Arg(3),
            ],
            4,
            "Taylor bound",
        ),
    ),
    (
        "converges",
        OpDisplay::Phrase(&[Text("Does"), Args, Text("converge?")], "Convergence test"),
    ),
    (
        "radius",
        OpDisplay::Phrase(
            &[Text("Radius of convergence of"), Args],
            "Radius of convergence",
        ),
    ),
    ("termsneeded", OpDisplay::Words("Terms needed")),
    // Integral applications.
    (
        "area",
        OpDisplay::Sentence(
            &[
                Text("Area between"),
                Arg(0),
                Text("and"),
                Arg(1),
                Interval("on"),
            ],
            2,
            "Area between curves",
        ),
    ),
    (
        "volume",
        OpDisplay::Sentence(
            &[
                Text("Volume from rotating"),
                Arg(0),
                Text("about"),
                Arg(1),
                Interval("on"),
                Choice(2, METHODS, "by the", "method"),
            ],
            3,
            "Volume of revolution",
        ),
    ),
    (
        "arclength",
        OpDisplay::Phrase(&[Text("Arc length of"), Args, Interval("on")], "Arc length"),
    ),
    (
        "surfacearea",
        OpDisplay::Sentence(
            &[
                Text("Surface area from rotating"),
                Arg(0),
                Text("about"),
                Arg(1),
                Interval("on"),
            ],
            2,
            "Surface area",
        ),
    ),
    (
        "average",
        OpDisplay::Phrase(
            &[Text("Average value of"), Args, Interval("on")],
            "Average value",
        ),
    ),
    (
        "work",
        OpDisplay::Phrase(
            &[Text("Work done by the force"), Args, Interval("over")],
            "Work",
        ),
    ),
    (
        "centroid",
        OpDisplay::Phrase(
            &[Text("Centre of mass for the density"), Args, Interval("on")],
            "Centroid",
        ),
    ),
    (
        "centreofmass",
        OpDisplay::Phrase(
            &[Text("Centre of mass for the density"), Args, Interval("on")],
            "Centre of mass",
        ),
    ),
    (
        "accumulate",
        OpDisplay::Phrase(
            &[Text("Accumulation function of"), Args, Sub("from")],
            "Accumulation",
        ),
    ),
    (
        "riemann",
        OpDisplay::Sentence(
            &[
                Choice(2, RULES, "Riemann sum by the", "rule, of"),
                Arg(0),
                Interval("on"),
                Text("with"),
                Var("n"),
                Text("="),
                Arg(1),
            ],
            3,
            "Riemann sum",
        ),
    ),
    (
        "trapezoid",
        OpDisplay::Sentence(
            &[
                Text("Trapezoid rule for"),
                Arg(0),
                Interval("on"),
                Text("with"),
                Var("n"),
                Text("="),
                Arg(1),
            ],
            2,
            "Trapezoid rule",
        ),
    ),
    (
        "simpson",
        OpDisplay::Sentence(
            &[
                Text("Simpson's rule for"),
                Arg(0),
                Interval("on"),
                Text("with"),
                Var("n"),
                Text("="),
                Arg(1),
            ],
            2,
            "Simpson's rule",
        ),
    ),
    (
        "midpoint",
        OpDisplay::Sentence(
            &[
                Text("Midpoint rule for"),
                Arg(0),
                Interval("on"),
                Text("with"),
                Var("n"),
                Text("="),
                Arg(1),
            ],
            2,
            "Midpoint rule",
        ),
    ),
    ("montecarlo", OpDisplay::Words("Monte Carlo estimate")),
    // Numerical methods.
    (
        "newton",
        OpDisplay::Sentence(
            &[
                Text("Newton's method for"),
                Arg(0),
                Text("starting at"),
                Arg(1),
                Text(","),
                Arg(2),
                Text("iterations"),
            ],
            3,
            "Newton's method",
        ),
    ),
    (
        "euler",
        OpDisplay::Sentence(
            &[
                Text("Euler's method for"),
                Arg(0),
                Text("with"),
                Arg(1),
                Text(", step"),
                Var("h"),
                Text("="),
                Arg(2),
                Text(","),
                Arg(3),
                Text("steps"),
            ],
            4,
            "Euler's method",
        ),
    ),
    (
        "rk4",
        OpDisplay::Sentence(
            &[
                Text("RK4 solution of"),
                Arg(0),
                Text("with"),
                Arg(1),
                Text(", step"),
                Var("h"),
                Text("="),
                Arg(2),
                Text(","),
                Arg(3),
                Text("steps"),
            ],
            4,
            "RK4",
        ),
    ),
    ("rk4table", OpDisplay::Words("RK4 table")),
    ("fixedpoint", OpDisplay::Words("Fixed-point iteration")),
    // Equations.
    (
        "solve",
        OpDisplay::Phrase(&[Text("Solve"), Sub("for"), Text(":"), Args], "Solve"),
    ),
    (
        "dsolve",
        OpDisplay::Phrase(&[Text("Solve"), Sub("for"), Text(":"), Args], "Solve"),
    ),
    (
        "nsolve",
        OpDisplay::Phrase(
            &[Text("Solve numerically"), Sub("for"), Text(":"), Args],
            "Solve numerically",
        ),
    ),
    ("lsolve", OpDisplay::Words("Solve by Laplace transform")),
    // Several variables.
    ("directional", DIRECTIONAL),
    ("dirderiv", DIRECTIONAL),
    (
        "tangentplane",
        OpDisplay::Phrase(
            &[Text("Tangent plane to"), Args, Sub("at")],
            "Tangent plane",
        ),
    ),
    (
        "linearapprox",
        OpDisplay::Phrase(
            &[Text("Linear approximation of"), Args, Sub("at")],
            "Linear approximation",
        ),
    ),
    (
        "classify",
        OpDisplay::Phrase(
            &[Text("Classify the critical points of"), Args],
            "Second-derivative test",
        ),
    ),
    (
        "changevars",
        OpDisplay::Sentence(
            &[Text("Change of variables in"), Arg(0), Text("with"), Arg(1)],
            2,
            "Change of variables",
        ),
    ),
    (
        "green",
        OpDisplay::Sentence(
            &[Text("Green's theorem for"), Arg(0), Text("around"), Arg(1)],
            2,
            "Green's theorem",
        ),
    ),
    (
        "stokes",
        OpDisplay::Sentence(
            &[Text("Stokes' theorem for"), Arg(0), Text("over"), Arg(1)],
            2,
            "Stokes' theorem",
        ),
    ),
    ("gauss", DIVERGENCE),
    ("divthm", DIVERGENCE),
    ("lineintegral", OpDisplay::Words("Line integral")),
    ("flux", OpDisplay::Words("Flux")),
    (
        "implicit",
        OpDisplay::Phrase(
            &[Text("Implicit derivative of"), Args, Sub("with respect to")],
            "Implicit differentiation",
        ),
    ),
    (
        "relatedrates",
        OpDisplay::Phrase(
            &[Text("Related rates for"), Args, Sub("in")],
            "Related rates",
        ),
    ),
    ("lagrange", OpDisplay::Words("Lagrange multipliers")),
    // Linear algebra.
    // Number theory.
    (
        "contfrac",
        OpDisplay::Phrase(&[Text("Continued fraction of"), Args], "Continued fraction"),
    ),
    (
        "convergents",
        OpDisplay::Sentence(
            &[Text("The first"), Arg(1), Text("convergents of"), Arg(0)],
            2,
            "Convergents",
        ),
    ),
    (
        "eig",
        OpDisplay::Phrase(&[Text("Eigenvalues of"), Args], "Eigenvalues"),
    ),
    (
        "eigenvals",
        OpDisplay::Phrase(&[Text("Eigenvalues of"), Args], "Eigenvalues"),
    ),
];

const DIRECTIONAL: OpDisplay = OpDisplay::Sentence(
    &[
        Text("Directional derivative of"),
        Arg(0),
        Text("at"),
        Arg(1),
        Text("in the direction of"),
        Arg(2),
    ],
    3,
    "Directional derivative",
);

const DIVERGENCE: OpDisplay = OpDisplay::Sentence(
    &[Text("Divergence theorem for"), Arg(0), Text("over"), Arg(1)],
    2,
    "Divergence theorem",
);

/// How a `Figure:` keyword reads on the page and in exports: "riemann mid" is shown
/// as "midpoint rule," so the line reads "f, midpoint rule, n = 8". The words start
/// in lower case unless they are a name; capitalise them at the start of a line
/// with [`capitalised`].
pub fn figure_keyword_words(k: crate::document::FigureKeyword) -> &'static str {
    use crate::document::FigureKeyword as K;
    match k {
        K::Polar => "polar curve",
        K::Parametric => "parametric curve",
        K::Implicit => "implicit curve",
        K::Points => "points",
        K::Contour => "contour plot",
        K::SlopeField => "slope field of",
        K::VectorField => "vector field",
        K::GradientField => "gradient field of",
        K::TangentPlaneAt => "with its tangent plane at",
        K::TangentAt => "with its tangent line at",
        K::RiemannLeft => "left Riemann sum,",
        K::RiemannRight => "right Riemann sum,",
        K::RiemannMid => "midpoint rule,",
        K::RiemannTrapezoid => "trapezoid rule,",
        K::AreaBetween => "and the area between them",
        K::RevolveAbout => "rotated about",
        K::Taylor => "Taylor polynomial of degree",
        K::NewtonFrom => "Newton's method from",
        K::Scatter => "scatter plot of",
        K::Histogram => "histogram of",
        K::BoxPlot => "box plot of",
    }
}

/// `s` with its first letter in upper case.
pub fn capitalised(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        Some(f) => f.to_uppercase().chain(c).collect(),
        None => String::new(),
    }
}

/// How the operator `name` is shown, when it is one of the helpers or transforms.
pub fn operator_display(name: &str) -> Option<OpDisplay> {
    TABLE.iter().find(|(n, _)| *n == name).map(|(_, d)| *d)
}

/// The operator's name in words ("Critical points", "Laplace transform"), for menus,
/// messages and the TeX export.
pub fn operator_words(name: &str) -> Option<&'static str> {
    Some(match operator_display(name)? {
        OpDisplay::Transform {
            letter: 'L',
            inverse: false,
        } => "Laplace transform",
        OpDisplay::Transform {
            letter: 'L',
            inverse: true,
        } => "Inverse Laplace transform",
        OpDisplay::Transform { inverse: false, .. } => "Fourier transform",
        OpDisplay::Transform { inverse: true, .. } => "Inverse Fourier transform",
        OpDisplay::Words(w) | OpDisplay::Phrase(_, w) | OpDisplay::Sentence(_, _, w) => w,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn helpers_with_options_read_as_textbook_sentences() {
        let s = |name: &str, sub: Option<&str>, sup: Option<&str>, args: &[&str]| {
            let args: Vec<String> = args.iter().map(|a| a.to_string()).collect();
            sentence_text(name, sub, sup, &args)
        };
        assert_eq!(
            s("riemann", Some("0"), Some("2"), &["f", "4", "mid"]).as_deref(),
            Some("Midpoint Riemann sum of f on [0, 2] with n = 4")
        );
        assert_eq!(
            s("volume", Some("0"), Some("1"), &["f", "y = 0", "auto"]).as_deref(),
            Some(
                "Volume from rotating f about y = 0 on [0, 1] \
                 (method chosen automatically)"
            )
        );
        assert_eq!(
            s("rk4", None, None, &["y′ = y", "y(0) = 1", "0.1", "10"]).as_deref(),
            Some("RK4 solution of y′ = y with y(0) = 1, step h = 0.1, 10 steps")
        );
        // Another argument order (an unknown option word, an interval among the
        // arguments) is not this sentence.
        assert_eq!(s("riemann", None, None, &["f", "[0, 1]", "4"]), None);
        assert_eq!(s("area", None, None, &["sin x", "[0, 2π]"]), None);
        // Another number of operands is not a sentence.
        assert_eq!(s("riemann", None, None, &["f"]), None);
        assert_eq!(operator_words("riemann"), Some("Riemann sum"));
    }

    #[test]
    fn every_helper_has_words_without_code_characters() {
        for (name, _) in TABLE {
            let words = operator_words(name).unwrap();
            assert!(
                !words.contains(['_', '\\', '{', '}', '(']) && words != *name,
                "{name} → {words}"
            );
        }
    }
}
