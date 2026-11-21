//! Math++ core: exact numbers, the expression tree, simplification, polynomials and
//! elementary functions. Calculus, solving and the `Domain` registry build on this crate.
//!
//! The textbook calculus tools each live in one module named for their subject:
//! [`analysis`] (curve sketching), [`applications`] (areas, volumes, work …),
//! [`convergence`] (named series tests, intervals of convergence), [`numerics`]
//! (quadrature, Newton, Euler and RK4 with error estimates), [`ode`] (the
//! differential-equations solver), [`multivar`] (multivariable and vector calculus),
//! [`transforms`] (Laplace and Fourier), [`step_tree`] ("show the working") and
//! [`table`] (labelled tables as values).

#![forbid(unsafe_code)]

pub mod analysis;
pub mod applications;
pub mod budget;
pub mod calc;
pub(crate) mod calc_support;
pub mod convergence;
pub mod display;
pub mod domains;
pub mod error;
pub mod eval;
pub mod expand;
pub mod expr;
pub mod factor;
pub mod family;
pub mod functions;
pub mod hp;
pub mod multivar;
pub mod note;
pub mod number;
pub mod numerics;
pub mod ode;
pub mod poly;
pub mod printer;
pub mod simplify;
pub mod step_tree;
pub mod steps;
pub mod subs;
pub mod table;
#[cfg(test)]
pub(crate) mod testkit;
pub mod transforms;
pub mod trig;
pub mod units;

pub use domains::{Ctx, Domain, Registry};
pub use error::{MathError, MathResult};
pub use eval::{EmptyEnv, Env};
pub use expand::expand;
pub use expr::{BigOpKind, Constant, Expr, ExprNode, LimitDir, LogicOp, Name, RelOp};
pub use factor::factor;
pub use functions::Func;
pub use number::Number;
pub use poly::{as_poly, is_polynomial_in, poly_div, poly_gcd, Poly};
pub use simplify::{add, mul, pow, simplify};
pub use steps::{Step, Steps};
pub use units::{Dimension, Quantity, Unit};
