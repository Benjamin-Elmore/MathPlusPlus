//! Math++ core: exact numbers, the expression tree, simplification, polynomials and
//! elementary functions. Calculus, solving and the `Domain` registry build on this crate.

#![forbid(unsafe_code)]

pub mod calc;
pub mod display;
pub mod domains;
pub mod error;
pub mod eval;
pub mod expand;
pub mod expr;
pub mod factor;
pub mod functions;
pub mod number;
pub mod poly;
pub mod printer;
pub mod simplify;
pub mod steps;
pub mod subs;
pub mod tools;
pub mod tools_b;
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
