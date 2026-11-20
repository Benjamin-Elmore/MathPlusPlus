//! Every example exported to PDF, LaTeX and HTML. The PDFs are parsed back, the LaTeX
//! and HTML files are compared with snapshots (`MPP_BLESS=1` rewrites them), and the
//! LaTeX is compiled with pdflatex when it is installed.

use std::path::{Path, PathBuf};

use mpp_export::{export_path, ExportOptions, Format, PageSize};

fn examples_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples")
}

fn snapshots_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/snapshots")
}

fn examples() -> Vec<(String, PathBuf)> {
    let mut out = vec![];
    for entry in std::fs::read_dir(examples_dir()).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().is_some_and(|e| e == "mpp") {
            let name = path.file_stem().unwrap().to_string_lossy().to_string();
            out.push((name, path));
        } else if path.join("main.mpp").exists() {
            let name = path.file_name().unwrap().to_string_lossy().to_string();
            out.push((name, path.join("main.mpp")));
        }
    }
    out.sort();
    out
}

fn normalise(s: &str) -> String {
    s.replace("\r\n", "\n")
}

/// The HTML without the embedded font data, which is the same in every page.
fn strip_fonts(html: &str) -> String {
    let mut out = String::new();
    let mut rest = html;
    while let Some(i) = rest.find("url(data:") {
        out.push_str(&rest[..i]);
        out.push_str("url(data:…)");
        match rest[i..].find(')') {
            Some(j) => rest = &rest[i + j + 1..],
            None => rest = "",
        }
    }
    out.push_str(rest);
    normalise(&out)
}

fn snapshot(name: &str, ext: &str, got: &str) {
    let path = snapshots_dir().join(format!("{name}.{ext}"));
    if std::env::var("MPP_BLESS").is_ok() {
        std::fs::create_dir_all(snapshots_dir()).unwrap();
        std::fs::write(&path, got).unwrap();
        return;
    }
    let want = std::fs::read_to_string(&path)
        .unwrap_or_else(|_| panic!("{} missing: run with MPP_BLESS=1", path.display()));
    assert_eq!(
        normalise(&want),
        got,
        "{name}.{ext} changed (MPP_BLESS=1 to accept)"
    );
}

#[test]
fn every_example_exports_to_a_readable_pdf() {
    for (name, path) in examples() {
        for page in [PageSize::A4, PageSize::Letter] {
            let opts = ExportOptions {
                page,
                ..ExportOptions::default()
            };
            let out =
                export_path(&path, Format::Pdf, &opts).unwrap_or_else(|e| panic!("{name}: {e}"));
            assert!(out.main.starts_with(b"%PDF-"), "{name}");
            let doc = lopdf::Document::load_mem(&out.main)
                .unwrap_or_else(|e| panic!("{name}: the PDF does not parse: {e}"));
            let pages = doc.get_pages();
            assert!(!pages.is_empty(), "{name}: no pages");
            let (w, h) = page.points();
            for (_, id) in pages {
                let dict = doc.get_dictionary(id).unwrap();
                let media: Vec<f32> = dict
                    .get(b"MediaBox")
                    .unwrap()
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|o| o.as_float().unwrap())
                    .collect();
                assert!(
                    (media[2] - w).abs() < 0.5 && (media[3] - h).abs() < 0.5,
                    "{name}"
                );
            }
        }
    }
}

#[test]
fn dark_and_option_variants_export() {
    let path = examples_dir().join("paper-features.mpp");
    for opts in [
        ExportOptions {
            dark: true,
            ..ExportOptions::default()
        },
        ExportOptions {
            include_results: false,
            include_comments: true,
            include_steps: true,
            ..ExportOptions::default()
        },
    ] {
        for format in [Format::Pdf, Format::Tex, Format::Html] {
            let out = export_path(&path, format, &opts).unwrap();
            assert!(!out.main.is_empty());
        }
    }
}

#[test]
fn results_are_included_by_default_and_can_be_left_out() {
    let path = examples_dir().join("integrals.mpp");
    let with = export_path(&path, Format::Tex, &ExportOptions::default()).unwrap();
    let with = String::from_utf8(with.main).unwrap();
    assert!(
        with.contains("\\int_0^1 x^2 \\, dx = \\frac{1}{3}"),
        "{with}"
    );
    let without = export_path(
        &path,
        Format::Tex,
        &ExportOptions {
            include_results: false,
            ..ExportOptions::default()
        },
    )
    .unwrap();
    let without = String::from_utf8(without.main).unwrap();
    assert!(without.contains("\\int_0^1 x^2 \\, dx = ?"), "{without}");
}

#[test]
fn steps_appear_when_asked_for() {
    let dir = std::env::temp_dir().join(format!("mpp-steps-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("steps.mpp");
    std::fs::write(&file, "$$ \\int x e^x \\, dx = ?! $$\n").unwrap();
    let plain = export_path(&file, Format::Tex, &ExportOptions::default()).unwrap();
    let with = export_path(
        &file,
        Format::Tex,
        &ExportOptions {
            include_steps: true,
            ..ExportOptions::default()
        },
    )
    .unwrap();
    assert!(with.main.len() > plain.main.len(), "no working shown");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn tex_snapshots() {
    for (name, path) in examples() {
        let out = export_path(&path, Format::Tex, &ExportOptions::default()).unwrap();
        let tex = String::from_utf8(out.main).unwrap();
        assert!(tex.contains("\\begin{document}") && tex.contains("\\end{document}"));
        snapshot(&name, "tex", &normalise(&tex));
        for (file, bytes) in &out.files {
            assert!(
                file.ends_with(".png") && bytes.starts_with(b"\x89PNG"),
                "{name}: {file}"
            );
            assert!(tex.contains(file), "{name}: {file} is not used");
        }
    }
}

#[test]
fn html_snapshots() {
    for (name, path) in examples() {
        let out = export_path(&path, Format::Html, &ExportOptions::default()).unwrap();
        let html = String::from_utf8(out.main).unwrap();
        assert!(html.starts_with("<!DOCTYPE html>"));
        assert!(
            !html.contains("<link ") && !html.contains("src=\"http"),
            "{name}: not self-contained"
        );
        snapshot(&name, "html", &strip_fonts(&html));
    }
}

#[test]
fn tex_compiles_when_pdflatex_is_installed() {
    let have = std::process::Command::new("pdflatex")
        .arg("--version")
        .output()
        .is_ok();
    if !have {
        eprintln!("skipped: pdflatex is not installed");
        return;
    }
    let dir = std::env::temp_dir().join(format!("mpp-tex-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    for (name, path) in examples() {
        let out = export_path(&path, Format::Tex, &ExportOptions::default()).unwrap();
        let tex = dir.join(format!("{name}.tex"));
        std::fs::write(&tex, &out.main).unwrap();
        for (file, bytes) in &out.files {
            std::fs::write(dir.join(file), bytes).unwrap();
        }
        let status = std::process::Command::new("pdflatex")
            .args(["-interaction=nonstopmode", "-halt-on-error"])
            .arg(format!("{name}.tex"))
            .current_dir(&dir)
            .output()
            .unwrap();
        assert!(
            status.status.success(),
            "{name}.tex does not compile:\n{}",
            String::from_utf8_lossy(&status.stdout)
        );
    }
    let _ = std::fs::remove_dir_all(&dir);
}
