//! What a reader sees never shows the evaluator's internal representations: every
//! example and pipeline document is evaluated and laid out as the IDE draws it, and
//! the glyphs on the page are scanned for
//! internal markers (unit factors written as `unit:m()`, core node names, `*` between
//! factors, hidden environment keys …; see [`mpp_eval::internal_marker`]).

use std::path::{Path, PathBuf};

use mpp_eval::{internal_marker, Evaluated, Session};
use mpp_ide::page::{build_page, PageInput};
use mpp_render::{IntegralLimits, Theme};
use mpp_syntax::document::{parse_document, Document};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn documents() -> Vec<PathBuf> {
    fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
        let Ok(rd) = std::fs::read_dir(dir) else {
            return;
        };
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() {
                walk(&p, out);
            } else if p.extension().is_some_and(|x| x == "mpp") {
                out.push(p);
            }
        }
    }
    let mut out = vec![];
    walk(&root().join("examples"), &mut out);
    walk(&root().join("work/pipeline-tests"), &mut out);
    out.sort();
    out
}

fn evaluate(src: &str, path: &Path) -> (Document, Evaluated) {
    let doc = parse_document(src);
    let mut s = Session::new();
    s.set_file(path);
    let ev = s.run_str(src).clone();
    (doc, ev)
}

/// Every glyph on the page, one block per line.
fn page_text(src: &str, doc: &Document, ev: &Evaluated) -> String {
    let page = build_page(&PageInput {
        show: &mpp_ide::page::ShowState::default(),
        src,
        doc,
        eval: Some(ev),
        theme: Theme::LIGHT,
        size: 18.0,
        width: 900.0,
        integral_limits: IntegralLimits::AboveBelow,
        slot_labels: &[],
        extra_refs: &mpp_ide::page::ExtraRefs::default(),
        proof: &mpp_ide::proof::ProofView::default(),
    });
    page.blocks
        .iter()
        .map(|b| b.dl.text())
        .collect::<Vec<_>>()
        .join("\n")
}

fn problems(path: &Path, src: &str) -> Vec<String> {
    let (doc, ev) = evaluate(src, path);
    let mut bad = vec![];
    for line in page_text(src, &doc, &ev).lines() {
        if let Some(m) = internal_marker(line) {
            bad.push(format!("{}: page: {m:?} in {line}", path.display()));
        }
    }
    bad
}

/// Answers with units that still hold free names, conversions and a figure of a
/// function with units: the cases that used to show `unit:m()` and `3*t`.
const UNITS: &str = r"Title: Units with names

$$ v := 3\,\mathrm{m/s} $$
$$ d := v t = ? $$
$$ F := m \cdot 9.81\,\mathrm{m/s^2} = ? $$
$$ x(t) := 2\,\mathrm{m} \cdot \sin(\omega t) $$
$$ x(t) = ? $$
$$ (5\,\mathrm{km}) \operatorname{in} \mathrm{m} = ? $$
$$ X_j \sim \mathrm{Uniform}[0, 1] $$

Figure: $x(t)$ for $t \in [0, 10]$, with $\omega = 2$.
";

#[test]
fn units_with_free_names_are_typeset() {
    let path = root().join("examples/units-fixture.mpp");
    let bad = problems(&path, UNITS);
    assert!(bad.is_empty(), "{}", bad.join("\n"));
    let (doc, ev) = evaluate(UNITS, &path);
    let text = page_text(UNITS, &doc, &ev);
    // The answers are there, with their units: 3t m/s.
    assert!(text.contains("3tm/s"), "{text}");
}

#[test]
fn no_document_shows_internal_text() {
    let mut bad = vec![];
    for path in documents() {
        let src = std::fs::read_to_string(&path).unwrap();
        bad.extend(problems(&path, &src));
    }
    assert!(bad.is_empty(), "internal text shown:\n{}", bad.join("\n"));
}
