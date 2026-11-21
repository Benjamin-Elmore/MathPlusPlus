//! Exports of the documents from the language-feel review (kept in
//! `mpp-eval/tests/fixtures/language-feel`): what a newcomer's worksheet and note
//! must look like in HTML and LaTeX.

use std::path::{Path, PathBuf};

use mpp_export::{export_path, ExportOptions, Format};

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../mpp-eval/tests/fixtures/language-feel")
        .join(name)
}

fn export(path: &Path, format: Format, opts: &ExportOptions) -> String {
    String::from_utf8(export_path(path, format, opts).unwrap().main).unwrap()
}

/// Writes `src` to a scratch file and exports it.
fn export_src(tag: &str, src: &str, format: Format, opts: &ExportOptions) -> String {
    let dir = std::env::temp_dir().join(format!("mpp-feel-{tag}-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("doc.mpp");
    std::fs::write(&file, src).unwrap();
    let out = export(&file, format, opts);
    let _ = std::fs::remove_dir_all(&dir);
    out
}

#[test]
fn html_export_shows_the_steps_and_the_reason_a_question_failed() {
    let calc = fixture("calc.mpp");
    let steps = ExportOptions {
        include_steps: true,
        ..ExportOptions::default()
    };
    let with = export(&calc, Format::Html, &steps);
    assert!(with.contains("product rule"), "no working in the HTML");
    assert!(with.contains("class=\"eq-error\""), "no error note");
    assert!(
        with.contains("diverges"),
        "the divergent series has no reason"
    );
    let without = export(&calc, Format::Html, &ExportOptions::default());
    assert!(!without.contains("product rule"));
    assert!(without.contains("diverges"));
}

#[test]
fn a_proof_ends_with_one_mark_in_html_and_tex() {
    let note = fixture("note.mpp");
    let html = export(&note, Format::Html, &ExportOptions::default());
    // Two proofs, one mark each, even though both typed their own ∎.
    assert_eq!(html.matches('∎').count(), 2, "{html}");
    let tex = export(&note, Format::Tex, &ExportOptions::default());
    assert!(!tex.contains("blacksquare"), "{tex}");
}

#[test]
fn second_level_headings_number_from_one() {
    let note = fixture("note.mpp");
    let html = export(&note, Format::Html, &ExportOptions::default());
    assert!(
        html.contains("<span class=\"sec-num\">1</span> Introduction"),
        "{html}"
    );
    assert!(!html.contains("sec-num\">0"));
    let tex = export(&note, Format::Tex, &ExportOptions::default());
    assert!(tex.contains("\\section{Introduction}"), "{tex}");
    // A single `#` over `##` sections is still the title.
    let src = "%number sections\n\n# My notes\n\n## First\n\nText.\n\n## Second\n\nMore.\n";
    let html = export_src("h1", src, Format::Html, &ExportOptions::default());
    assert!(
        html.contains("<span class=\"sec-num\">2</span> Second"),
        "{html}"
    );
}

#[test]
fn a_references_list_is_a_real_bibliography() {
    let note = fixture("note.mpp");
    let tex = export(&note, Format::Tex, &ExportOptions::default());
    assert!(tex.contains("\\begin{thebibliography}"), "{tex}");
    assert!(tex.contains("\\bibitem{hardy} G. H. Hardy"), "{tex}");
    assert!(tex.contains("\\bibitem{knuth}"), "{tex}");
    assert!(!tex.contains("\\item \\cite"), "{tex}");
    let html = export(&note, Format::Html, &ExportOptions::default());
    // Every citation link has a target.
    assert!(html.contains("id=\"ref-hardy\""), "{html}");
    assert!(html.contains("id=\"ref-knuth\""), "{html}");
}

/// An exact query whose answer is only the question again (`ζ(3) = ζ(3)`) shows the
/// decimal instead, with `≈`, in HTML and in LaTeX; a real closed form keeps `=`.
#[test]
fn an_unsimplified_special_value_exports_as_its_decimal() {
    let src = "$$ \\zeta(3) = ? $$\n\n$$ J_0(1) = ? $$\n\n$$ \\zeta(2) = ? $$\n";
    let opts = ExportOptions::default();
    let tex = export_src("tautology", src, Format::Tex, &opts);
    assert!(tex.contains(r"\zeta(3) \approx 1.202056903"), "{tex}");
    assert!(tex.contains(r"J_0(1) \approx 0.7651976866"), "{tex}");
    assert!(!tex.contains(r"\zeta\left(3\right)"), "{tex}");
    assert!(tex.contains(r"\zeta(2) = \frac{\pi^{2}}{6}"), "{tex}");
    let html = export_src("tautology", src, Format::Html, &opts);
    assert!(
        html.contains("≈1.202056903"),
        "no decimal for ζ(3) in the HTML"
    );
    assert!(
        html.contains("≈0.7651976866"),
        "no decimal for J₀(1) in the HTML"
    );
    assert!(!html.contains("(3)=𝜁(3)"), "ζ(3) = ζ(3) in the HTML");
}
