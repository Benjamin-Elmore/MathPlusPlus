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

/// The outline entries, the variables panel lines (as the LaTeX they are typeset
/// from) and the names autocomplete offers.
fn panels(src: &str, doc: &Document, ev: &Evaluated) -> Vec<String> {
    let mut out: Vec<String> = mpp_ide::explorer::outline::outline(doc, src, Some(ev))
        .into_iter()
        .map(|i| i.latex)
        .collect();
    for d in mpp_ide::defs::all(ev) {
        out.push(mpp_ide::defs::summary_latex(&d));
        out.push(d.name.clone());
    }
    out.extend(ev.user_names());
    out
}

#[test]
fn panels_list_only_user_definitions() {
    let mut bad = vec![];
    let mut docs: Vec<(PathBuf, String)> = documents()
        .into_iter()
        .map(|p| {
            let s = std::fs::read_to_string(&p).unwrap();
            (p, s)
        })
        .collect();
    docs.push((root().join("examples/units-fixture.mpp"), UNITS.to_string()));
    for (path, src) in &docs {
        let (doc, ev) = evaluate(src, path);
        for line in panels(src, &doc, &ev) {
            if let Some(m) = internal_marker(&line) {
                bad.push(format!("{}: {m:?} in {line}", path.display()));
            }
        }
    }
    assert!(bad.is_empty(), "internal names listed:\n{}", bad.join("\n"));
    // A random variable is listed as its declaration, under its own name; the
    // denominators a function was written with are not listed at all.
    let src = r"$$ R(g) := \frac{1}{g} $$
$$ X_j \sim \mathrm{Uniform}[0, 1] $$
$$ R(2) = ? $$
";
    let path = root().join("examples/panels-fixture.mpp");
    let (doc, ev) = evaluate(src, &path);
    let listed = panels(src, &doc, &ev);
    assert!(listed.iter().any(|l| l.contains(r"\sim")), "{listed:?}");
    assert!(ev.user_names().contains(&"X".to_string()));
    assert!(
        !listed.iter().any(|l| l.contains("denominators")),
        "{listed:?}"
    );
}

/// The Insert menu's title block before it is filled in: no `Title:`, `Author:` or
/// `Date:` keyword is drawn, only grey labels, and a filled title hides the others.
#[test]
fn an_empty_title_block_shows_no_keywords() {
    let path = root().join("examples/title-fixture.mpp");
    for src in [
        "Title: \nAuthor: \nDate: \n",
        "Title: Waves\nAuthor: \nDate: \n",
    ] {
        let (doc, ev) = evaluate(src, &path);
        let text = page_text(src, &doc, &ev);
        assert!(!text.contains(':'), "{src:?} shows {text:?}");
    }
}

/// Transforms and helpers are typeset in textbook notation, never by operator name.
#[test]
fn helpers_and_transforms_show_no_operator_names() {
    let path = root().join("examples/helpers-fixture.mpp");
    let src = r"$$ \mathcal{L}_{t \to s}\left\{ t^2 \right\} = ? $$

$$ \mathcal{F}^{-1}_{\omega \to x}\left\{ \frac{1}{1 + \omega^2} \right\} $$

$$ \operatorname{taylorbound}(e^x, 0, 3, 1) $$

$$ \operatorname{rk4}(x + y, 0, 1, 0.1, 5) $$

$$ \operatorname{mvt}_{[0, 2]} x^2 = ? $$

$$ \operatorname{series}_{x=0}^{5} \sin x = ? $$
";
    let (doc, ev) = evaluate(src, &path);
    let text = page_text(src, &doc, &ev);
    for name in ["laplace", "fourier", "taylorbound", "rk4", "mvt"] {
        assert!(!text.contains(name), "{name} in {text}");
    }
    assert!(text.contains('ℒ') && text.contains('ℱ'), "{text}");
    assert!(text.contains("Taylorseriesof"), "{text}");
}

/// A helper whose answer is a table points at it with a plain arrow: no empty box is
/// drawn on a finished page.
#[test]
fn a_table_answer_draws_no_empty_boxes() {
    let path = root().join("examples/curve-sketching.mpp");
    let src = r"$$ f(x) := x^3 - 3x $$

$$ \operatorname{extrema}_{[-2, 2]} f = ? $$
";
    let (doc, ev) = evaluate(src, &path);
    let page = build_page(&PageInput {
        show: &mpp_ide::page::ShowState::default(),
        src,
        doc: &doc,
        eval: Some(&ev),
        theme: Theme::LIGHT,
        size: 18.0,
        width: 900.0,
        integral_limits: IntegralLimits::AboveBelow,
        slot_labels: &[],
        extra_refs: &mpp_ide::page::ExtraRefs::default(),
        proof: &mpp_ide::proof::ProofView::default(),
    });
    let text = page_text(src, &doc, &ev);
    assert!(text.contains('↓'), "{text}");
    for b in &page.blocks {
        let outlines =
            b.dl.items
                .iter()
                .filter(|p| matches!(p, mpp_render::Primitive::Outline { .. }))
                .count();
        assert_eq!(outlines, 0, "{}", b.dl.text());
    }
}
