//! `mpp run` and the exports never show internal representations: every example and
//! pipeline document is run, and each answer (with its working) is scanned for the
//! markers of [`mpp_eval::internal_marker`] (`unit:m()`, core node names, `*` between
//! factors, hidden keys). A document with units on names is also exported to LaTeX
//! and HTML and scanned the same way.

use std::path::{Path, PathBuf};

use mpp_cli::{run_file, run_lines};
use mpp_eval::internal_marker;
use mpp_export::{export_path, ExportOptions, Format};

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

#[test]
fn no_answer_shows_internal_text() {
    let mut bad = vec![];
    for path in documents() {
        let Ok(ev) = run_file(&path) else {
            continue;
        };
        for line in run_lines(&ev) {
            for text in std::iter::once(&line.answer).chain(line.steps.iter()) {
                if let Some(m) = internal_marker(text) {
                    bad.push(format!("{}:{}: {m:?} in {text}", path.display(), line.line));
                }
            }
        }
    }
    assert!(
        bad.is_empty(),
        "internal text in answers:\n{}",
        bad.join("\n")
    );
}

const UNITS: &str = r"Title: Units with names

$$ v := 3\,\mathrm{m/s} $$
$$ d := v t = ? $$
$$ F := m \cdot 9.81\,\mathrm{m/s^2} = ? $$
$$ x(t) := 2\,\mathrm{m} \cdot \sin(\omega t) $$
$$ x(t) = ? $$
$$ (5\,\mathrm{km}) \operatorname{in} \mathrm{m} = ? $$

Figure: $x(t)$ for $t \in [0, 10]$, with $\omega = 2$.
";

#[test]
fn units_on_names_are_typeset_in_run_output_and_exports() {
    let dir = tempfile::tempdir().unwrap();
    let doc = dir.path().join("units.mpp");
    std::fs::write(&doc, UNITS).unwrap();
    let ev = run_file(&doc).unwrap();
    let answers: Vec<String> = run_lines(&ev).into_iter().map(|l| l.answer).collect();
    assert!(answers.iter().any(|a| a == "3t m/s"), "{answers:?}");
    assert!(answers.iter().any(|a| a == "2 sin(tω) m"), "{answers:?}");
    for a in &answers {
        assert!(internal_marker(a).is_none(), "{a}");
    }
    for (format, ext) in [(Format::Tex, "tex"), (Format::Html, "html")] {
        let out = export_path(&doc, format, &ExportOptions::default()).unwrap();
        let text = String::from_utf8_lossy(&out.main).to_string();
        for marker in ["unit:", "unit as", "unit convert", "zzunit"] {
            assert!(!text.contains(marker), "{marker} in the {ext} export");
        }
        if format == Format::Tex {
            assert!(text.contains("m/s"), "{text}");
        }
    }
}
