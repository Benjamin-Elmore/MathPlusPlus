//! The maths helpers and calculus operators as holders, one module per subject:
//! [`analysis`] (curve sketching: `crit`, `extrema`, helper sentences), [`multivar`]
//! (multivariable and vector calculus), [`applications`] (areas, volumes, work …),
//! [`numerics`] (quadrature, Newton, Euler, RK4, Taylor bounds), [`convergence`] (series
//! tests), [`ode`] (`dsolve` and the PDE helpers) and [`transforms`] (the Laplace
//! operators). [`operator_args`] reads the calculus operators' arguments by their shape.
//!
//! Two holders are registered: [`evaluate`] for the analysis and multivariable helpers and
//! [`evaluate_operators`] for the calculus operators.

use mpp_core::{Expr, Func, MathError, MathResult};

use crate::holders::Ctx;

mod analysis;
mod applications;
mod convergence;
mod multivar;
mod numerics;
mod ode;
mod operator_args;

/// True for a word that selects a method or variant of a calculus operator (`left`,
/// `shell`, `simpson` ...), the words the helpers accept as options.
pub fn is_option_word(word: &str) -> bool {
    operator_args::KEYWORDS.contains(&word)
}
mod transforms;

pub use analysis::{
    bound_names, denominators_key, evaluate, is_helper, sentence_holder, sentence_op,
    unwrap_where_defined, where_defined, WHERE_DEFINED,
};
pub use ode::ode_statement;
pub(crate) use ode::{is_differential_equation_or_system, FUNCTION_MARKER};

/// Evaluates the calculus operators (integral applications, numerical methods, series,
/// differential equations, the Laplace operators) and the special functions `erf`, `Si`
/// and `Ci` typed as `\operatorname{erf}(x)`; `None` for anything else.
pub fn evaluate_operators(e: &Expr, ctx: &Ctx<'_>) -> Option<MathResult<Expr>> {
    if let Some(kept) = ode::unknown_function_derivative(e) {
        return Some(Ok(kept));
    }
    let call = operator_args::Call::read(e)?;
    if let Some(f) = special_function(&call.name) {
        return Some(match call.args.as_slice() {
            [a] => Expr::func(f, vec![a.clone()]),
            _ => Err(MathError::WrongArity {
                function: call.name.clone(),
                expected: "1".into(),
                got: call.args.len(),
            }),
        });
    }
    let result = match call.name.as_str() {
        "area" | "areabetween" => applications::area(&call, ctx),
        "volume" | "vol" => applications::volume(&call, ctx),
        "arclength" | "arc" => applications::arc_length(&call, ctx),
        "surfacearea" | "surface" => applications::surface(&call, ctx),
        "average" | "avg" | "averagevalue" => {
            applications::single_integral(&call, ctx, applications::Single::Average)
        }
        "work" => applications::single_integral(&call, ctx, applications::Single::Work),
        "centroid" => applications::centroid(&call, ctx),
        "centreofmass" | "centerofmass" | "com" => {
            applications::single_integral(&call, ctx, applications::Single::Rod)
        }
        "accumulate" | "accumulation" => applications::accumulate(&call, ctx),
        "riemann" | "leftsum" | "rightsum" | "midsum" | "midpoint" | "trapezoid"
        | "trapezoidal" | "simpson" => numerics::quadrature(&call, ctx),
        "newton" => numerics::newton(&call, ctx),
        "euler" | "rk4" | "rungekutta" => numerics::stepper(&call, ctx),
        "taylorbound" | "remainder" => numerics::taylor_bound(&call, ctx),
        "termsneeded" | "taylorterms" => numerics::taylor_terms(&call, ctx),
        "tests" | "convtests" | "convergencetests" => convergence::tests(&call, ctx),
        "radius" | "roc" | "radiusofconvergence" => convergence::power_series(&call, ctx, true),
        "ioc" | "intervalofconvergence" => convergence::power_series(&call, ctx, false),
        "dsolve" | "odesolve" | "ode" => ode::dsolve(&call, ctx),
        "solve"
            if call
                .args
                .first()
                .is_some_and(ode::is_differential_equation_or_system) =>
        {
            ode::dsolve(&call, ctx)
        }
        "laplace" => transforms::laplace(&call, ctx, false),
        "invlaplace" | "ilaplace" | "inverselaplace" => transforms::laplace(&call, ctx, true),
        "heat" | "heatequation" => ode::pde(&call, ctx, ode::Pde::Heat),
        "wave" | "waveequation" => ode::pde(&call, ctx, ode::Pde::Wave),
        "laplacerect" | "laplaceequation" => ode::pde(&call, ctx, ode::Pde::Laplace),
        _ => return None,
    };
    Some(result)
}

fn special_function(name: &str) -> Option<Func> {
    match name {
        "erf" => Some(Func::Erf),
        "si" => Some(Func::Si),
        "ci" => Some(Func::Ci),
        _ => None,
    }
}
