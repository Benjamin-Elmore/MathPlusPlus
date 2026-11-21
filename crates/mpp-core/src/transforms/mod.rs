//! Integral transforms: the Laplace transform with its inverse and the transform method for
//! constant-coefficient equations ([`laplace`]), and the Fourier transform ([`fourier`]).
//! Each transform has exactly one engine, used by every operator and solver.

pub mod fourier;
pub mod laplace;
