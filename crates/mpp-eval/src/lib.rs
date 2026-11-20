//! Math++ evaluator: runs a `.mpp` document top to bottom, like reading a paper.
//!
//! [`Session::run_str`] parses a document, lowers each maths node into a core
//! expression ([`lower`]), and runs the statements in order: `:=` variables and
//! functions, `≡` constants, recurrences, `= ?` / `≈ ?` / `= ?!` queries and
//! `\overset{?}{=}` checks. Every block gets a [`BlockResult`] with the names it
//! defines and uses, a [`LineResult`] per statement line and its diagnostics.
//! [`Session::update`] re-runs only the blocks an edit changed and those that read
//! a changed definition, so the IDE can evaluate as you type on a background
//! thread (`Session` is `Send`).
//!
//! Calculus notation becomes unevaluated holder nodes that the calculus phase
//! evaluates by registering a [`HolderEval`] (see [`holders`]); imports go through
//! the [`Importer`] hook (files on disk by default, with a cache, loop and clash checks).

#![forbid(unsafe_code)]

pub mod assume;
mod auto;
mod block;
pub mod calc;
pub mod calculus;
pub mod check;
pub mod data;
mod differential;
pub mod dispatch;
pub mod domains_wire;
pub mod error;
pub mod fields_wire;
pub mod files;
pub mod graph;
pub mod helpers;
pub mod helpers_b;
pub mod holders;
pub mod import;
pub mod lower;
pub mod names;
pub mod numeric;
pub mod numfmt;
pub mod resolve;
pub mod result;
mod session;
pub mod stdlib;
pub mod units_wire;
pub mod value;
#[cfg(feature = "watch")]
pub mod watch;

pub use calc::{calc_line, CalcError};
pub use check::Verdict;
pub use error::{EvalError, Located};
pub use files::{
    EmbeddedSources, FileImporter, FsSource, LayeredSource, Location, SourceProvider, Stamp,
};
pub use graph::DepGraph;
pub use holders::{evaluate_holder, Collected, Ctx, HolderEval, Holders, Shown};
pub use import::{
    Exports, FileDiagnostic, ImportContext, ImportError, ImportErrorKind, ImportRequest, Importer,
    NoImports,
};
pub use mpp_core::display::NumberFormat;
pub use mpp_syntax::math::QueryMode;
pub use result::{
    BlockResult, DefinitionKind, Evaluated, ImportInfo, LineIndex, LineKind, LineResult, RangeInfo,
    RangeKind,
};
pub use session::Session;
pub use value::{Binding, Env, Origin, Sequence, Value, VarKind};
#[cfg(feature = "watch")]
pub use watch::FileWatcher;
