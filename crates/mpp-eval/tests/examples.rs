//! The worked examples in `examples/` against their expected answers: each line of a
//! `*.expected.txt` is `what was asked → the answer`, in document order, and an error
//! counts as the answer of the line it is on.

use std::path::Path;
use std::sync::Arc;

use mpp_eval::{FileImporter, LineKind, QueryMode, Session};

/// The answers of a document in order: query answers, with errors in place.
fn answers_of(path: &Path) -> Vec<String> {
    let src = std::fs::read_to_string(path).unwrap_or_else(|e| panic!("{path:?}: {e}"));
    let mut session = Session::new().with_importer(Arc::new(FileImporter::new()));
    session.set_file(path);
    let ev = session.run_str(&src).clone();
    let errors = ev.errors();
    let mut items: Vec<(usize, String)> = Vec::new();
    let mut used = vec![false; errors.len()];
    for (_, line) in ev.queries() {
        // The first error that starts on this line.
        let inside = errors
            .iter()
            .position(|d| d.span.start >= line.span.start && d.span.start < line.span.end);
        let text = match inside {
            Some(i) => {
                used[i] = true;
                mpp_core::note::plain(&errors[i].message)
            }
            None => {
                let approx_query = matches!(
                    line.kind,
                    LineKind::Query {
                        mode: QueryMode::Approx,
                        ..
                    }
                );
                let shown = if approx_query {
                    line.approx.clone().or_else(|| line.plain.clone())
                } else {
                    line.plain.clone()
                };
                shown.unwrap_or_else(|| "<none>".into())
            }
        };
        items.push((line.span.start, text));
    }
    for (i, d) in errors.iter().enumerate() {
        if !used[i] {
            items.push((d.span.start, mpp_core::note::plain(&d.message)));
        }
    }
    items.sort_by_key(|(at, _)| *at);
    items.into_iter().map(|(_, t)| t).collect()
}

fn check(name: &str) {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples");
    let doc = root.join(format!("{name}.mpp"));
    let expected = std::fs::read_to_string(root.join(format!("{name}.expected.txt")))
        .unwrap_or_else(|e| panic!("{name}.expected.txt: {e}"));
    let want: Vec<&str> = expected
        .lines()
        .filter_map(|l| l.rsplit_once('→').map(|(_, r)| r.trim()))
        .collect();
    assert_eq!(answers_of(&doc), want, "{name}.mpp");
}

#[test]
fn units_example() {
    check("units");
}

#[test]
fn assumptions_example() {
    check("assumptions");
}

#[test]
fn piecewise_sets_example() {
    check("piecewise-sets");
}

#[test]
fn statistics_example() {
    check("statistics");
}

#[test]
fn number_display_example() {
    check("number-display");
}

#[test]
fn comments_example() {
    check("comments");
}

#[test]
fn paper_features_example() {
    check("paper-features");
}
