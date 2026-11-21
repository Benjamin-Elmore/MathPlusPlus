//! A new field of maths plugs into a session without touching the core or the evaluator.

use std::sync::Arc;

use mpp_core::domains::{Ctx, Domain, Value};
use mpp_core::{Expr, MathResult, Registry};
use mpp_eval::{NoImports, Session};

/// `triple(x)` answers `3x` for a number.
struct Triple;

impl Domain for Triple {
    fn name(&self) -> &'static str {
        "tripling"
    }
    fn operators(&self) -> &'static [&'static str] {
        &["triple"]
    }
    fn apply(&self, op: &str, args: &[Value], _ctx: &mut Ctx<'_>) -> Option<MathResult<Value>> {
        match (op, args) {
            ("triple", [x]) if x.as_number().is_some() => Some(Ok(Expr::int(3) * x.clone())),
            _ => None,
        }
    }
}

fn answers(session: &mut Session, src: &str) -> Vec<String> {
    let ev = session.run_str(src);
    ev.queries()
        .map(|(_, l)| l.text.clone().unwrap_or_default())
        .collect()
}

#[test]
fn a_registered_domain_answers_its_operator() {
    let mut session = Session::new().with_importer(Arc::new(NoImports));
    session.register_domain(Box::new(Triple));
    let ev = session
        .run_str(r"$$ \operatorname{triple}(14) = ? $$")
        .clone();
    assert!(ev.errors().is_empty(), "{:?}", ev.errors());
    let text: Vec<String> = ev.queries().map(|(_, l)| format!("{l:?}")).collect();
    assert!(text.iter().any(|t| t.contains("42")), "{text:?}");
}

#[test]
fn the_standard_domains_stay_when_one_is_added() {
    let mut session = Session::new().with_importer(Arc::new(NoImports));
    session.register_domain(Box::new(Triple));
    let ev = session
        .run_str(r"$$ \det\begin{pmatrix}1&2\\3&4\end{pmatrix} = ? $$")
        .clone();
    assert!(ev.errors().is_empty(), "{:?}", ev.errors());
}

#[test]
fn an_empty_registry_has_no_operators() {
    let mut session = Session::new()
        .with_importer(Arc::new(NoImports))
        .with_registry(Registry::empty());
    let _ = answers(&mut session, "$$ 1 + 1 = ? $$\n");
    assert!(!session.holders_mut().domains().handles("det"));
}
