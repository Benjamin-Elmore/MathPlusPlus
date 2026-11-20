//! Calculus tools, second set: applications of integrals, numerical methods with error
//! estimates, named convergence tests and power-series intervals, and the differential
//! equations solver.
//!
//! Every routine returns exact answers where the CAS finds them and marks numerical ones
//! (approximate numbers). Nothing doubtful is returned as an answer: when a check fails the
//! result is an error that says what could not be decided. Each routine can log its working
//! into a [`Steps`](crate::steps::Steps) log, naming the method used.

pub mod applications;
pub mod convergence;
pub mod numerics;
pub mod ode;
mod util;

pub use applications::{
    accumulation, arc_length, arc_length_parametric, area_between, average_value,
    centre_of_mass_rod, centroid, surface_area, volume, work, Axis, VolumeMethod,
};
pub use convergence::{power_series_interval, series_tests, PowerSeriesInterval, TestOutcome};
pub use numerics::{
    euler, newton, quadrature_rule, rk4, taylor_bound, taylor_terms_needed, Iterate, OdeTable,
    Quadrature, QuadratureRule, TaylorBound,
};
