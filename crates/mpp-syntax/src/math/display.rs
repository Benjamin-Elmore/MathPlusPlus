//! How a named operator is shown on the page. The file keeps the short operator name
//! (`\operatorname{crit}`, `\mathcal{L}` read as `laplace`); readers see textbook
//! notation: ℒ{f}, "Critical points of f on [0, 4]", "Taylor bound", "RK4".
//!
//! The IDE, the HTML and PDF export (both drawn by the renderer) and the TeX export all
//! read this one table, so a helper is named the same way everywhere.

/// One part of a [`OpDisplay::Phrase`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Piece {
    /// Upright words.
    Text(&'static str),
    /// The operand(s), without brackets.
    Args,
    /// The subscript at full size, after the given words; skipped when absent.
    Sub(&'static str),
    /// The superscript at full size, after the given words; skipped when absent.
    Sup(&'static str),
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
}

use Piece::{Args, Sub, Sup, Text};

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
    ("taylorbound", OpDisplay::Words("Taylor bound")),
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
    ("area", OpDisplay::Words("Area between curves")),
    ("volume", OpDisplay::Words("Volume of revolution")),
    ("arclength", OpDisplay::Words("Arc length")),
    ("surfacearea", OpDisplay::Words("Surface area")),
    ("average", OpDisplay::Words("Average value")),
    ("work", OpDisplay::Words("Work")),
    ("centroid", OpDisplay::Words("Centroid")),
    ("centreofmass", OpDisplay::Words("Centre of mass")),
    ("accumulate", OpDisplay::Words("Accumulation")),
    ("riemann", OpDisplay::Words("Riemann sum")),
    ("trapezoid", OpDisplay::Words("Trapezoid rule")),
    ("simpson", OpDisplay::Words("Simpson's rule")),
    ("midpoint", OpDisplay::Words("Midpoint rule")),
    ("montecarlo", OpDisplay::Words("Monte Carlo estimate")),
    // Numerical methods.
    ("newton", OpDisplay::Words("Newton's method")),
    ("euler", OpDisplay::Words("Euler's method")),
    ("rk4", OpDisplay::Words("RK4")),
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
    ("directional", OpDisplay::Words("Directional derivative")),
    ("dirderiv", OpDisplay::Words("Directional derivative")),
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
    ("changevars", OpDisplay::Words("Change of variables")),
    ("green", OpDisplay::Words("Green's theorem")),
    ("stokes", OpDisplay::Words("Stokes' theorem")),
    ("gauss", OpDisplay::Words("Divergence theorem")),
    ("divthm", OpDisplay::Words("Divergence theorem")),
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
    ("eig", OpDisplay::Words("Eigenvalues")),
    ("eigenvals", OpDisplay::Words("Eigenvalues")),
];

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
        OpDisplay::Words(w) | OpDisplay::Phrase(_, w) => w,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

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
