//! The `mpp` binary: run, check, calc, render, export and ide.

use std::path::PathBuf;
use std::process::Command;

fn mpp() -> Command {
    Command::new(env!("CARGO_BIN_EXE_mpp"))
}

fn example(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples")
        .join(name)
}

fn text(out: &[u8]) -> String {
    String::from_utf8_lossy(out).to_string()
}

#[test]
fn run_prints_typeset_answers() {
    let out = mpp()
        .arg("run")
        .arg(example("integrals.mpp"))
        .output()
        .unwrap();
    assert!(out.status.success());
    let s = text(&out.stdout);
    assert!(s.contains("∫₀¹ x² dx = 1/3"), "{s}");
    assert!(!s.contains('\\'), "raw LaTeX in the output: {s}");
}

#[test]
fn run_fails_on_errors_and_shows_unicode_maths() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("bad.mpp");
    std::fs::write(
        &file,
        "$$ \\frac{1}{0} = ? $$\n\n$$ 3\\,\\mathrm{m} + 2\\,\\mathrm{s} = ? $$\n",
    )
    .unwrap();
    let out = mpp().arg("run").arg(&file).output().unwrap();
    assert!(!out.status.success());
    let s = text(&out.stdout);
    assert!(s.contains('✗'), "{s}");
    assert!(!s.contains("\\frac"), "{s}");
}

#[test]
fn check_reports_file_line_and_exits_non_zero() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("bad.mpp");
    std::fs::write(&file, "# Title\n\n$$ x := \\frac{1}{ $$\n").unwrap();
    let out = mpp().arg("check").arg(&file).output().unwrap();
    assert!(!out.status.success());
    let err = text(&out.stderr);
    assert!(err.contains("error:"), "{err}");
    assert!(err.contains("bad.mpp:"), "{err}");
    assert!(!err.contains(".."), "a raw span leaked: {err}");
}

#[test]
fn check_passes_a_good_file() {
    let out = mpp()
        .arg("check")
        .arg(example("algebra.mpp"))
        .output()
        .unwrap();
    assert!(out.status.success());
    assert!(text(&out.stdout).contains("ok"));
}

#[test]
fn calc_answers_a_question() {
    let out = mpp().arg("calc").arg(r"\int_0^1 x^2 dx").output().unwrap();
    assert!(out.status.success());
    assert_eq!(text(&out.stdout).trim(), "∫₀¹ x² dx = 1/3");
}

#[test]
fn calc_reports_errors() {
    let out = mpp()
        .arg("calc")
        .arg(r"3\,\mathrm{m} + 2\,\mathrm{s}")
        .output()
        .unwrap();
    assert!(!out.status.success());
    assert!(text(&out.stderr).contains("error:"));
}

#[test]
fn render_writes_a_page_with_maths_and_graphs() {
    let dir = tempfile::tempdir().unwrap();
    let out_file = dir.path().join("page.html");
    let status = mpp()
        .arg("render")
        .arg(example("graphs-2d.mpp"))
        .arg("-o")
        .arg(&out_file)
        .status()
        .unwrap();
    assert!(status.success());
    let html = std::fs::read_to_string(&out_file).unwrap();
    assert!(html.starts_with("<!DOCTYPE html>"));
    assert!(html.contains("<svg"));
    assert!(html.matches("<figure").count() >= 16, "graphs missing");
}

#[test]
fn render_fills_in_results() {
    let dir = tempfile::tempdir().unwrap();
    let out_file = dir.path().join("page.html");
    assert!(mpp()
        .arg("render")
        .arg(example("integrals.mpp"))
        .arg("-o")
        .arg(&out_file)
        .status()
        .unwrap()
        .success());
    let html = std::fs::read_to_string(&out_file).unwrap();
    // The answer of the first definite integral is typeset in the page's result colour.
    assert!(html.contains("var(--mpp-result"), "no results in the page");
}

#[test]
fn export_writes_all_three_formats() {
    let dir = tempfile::tempdir().unwrap();
    for (fmt, magic) in [
        ("pdf", "%PDF-"),
        ("tex", "\\documentclass"),
        ("html", "<!DOCTYPE"),
    ] {
        let out_file = dir.path().join(format!("out.{fmt}"));
        let status = mpp()
            .args(["export"])
            .arg(example("algebra.mpp"))
            .args(["--to", fmt, "-o"])
            .arg(&out_file)
            .status()
            .unwrap();
        assert!(status.success(), "{fmt}");
        let bytes = std::fs::read(&out_file).unwrap();
        assert!(
            String::from_utf8_lossy(&bytes[..magic.len().min(bytes.len())]).starts_with(magic)
                || String::from_utf8_lossy(&bytes).contains(magic),
            "{fmt}"
        );
    }
}

#[test]
fn export_rejects_an_unknown_format() {
    let out = mpp()
        .arg("export")
        .arg(example("algebra.mpp"))
        .args(["--to", "docx"])
        .output()
        .unwrap();
    assert!(!out.status.success());
}

#[test]
fn ide_is_a_command() {
    // Starting the editor would open a window, so only its help is checked here;
    // the editor itself is tested in mpp-ide.
    let out = mpp().arg("ide").arg("--help").output().unwrap();
    assert!(out.status.success());
    assert!(text(&out.stdout).contains("editor"));
}

fn docs(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../docs")
        .join(name)
}

#[test]
fn the_guide_has_no_errors_and_exports() {
    let out = mpp().arg("check").arg(docs("guide.mpp")).output().unwrap();
    assert!(out.status.success(), "{}", text(&out.stderr));
    let dir = tempfile::tempdir().unwrap();
    let pdf = dir.path().join("guide.pdf");
    assert!(mpp()
        .arg("export")
        .arg(docs("guide.mpp"))
        .args(["--to", "pdf", "-o"])
        .arg(&pdf)
        .status()
        .unwrap()
        .success());
    assert!(std::fs::read(&pdf).unwrap().starts_with(b"%PDF-"));
}

#[test]
fn run_shows_steps_when_asked() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("steps.mpp");
    std::fs::write(&file, "$$ \\int x e^x \\, dx = ?! $$\n").unwrap();
    let plain = text(&mpp().arg("run").arg(&file).output().unwrap().stdout);
    let with = text(
        &mpp()
            .arg("run")
            .arg(&file)
            .arg("--steps")
            .output()
            .unwrap()
            .stdout,
    );
    assert!(with.lines().count() > plain.lines().count(), "{with}");
    assert!(with.contains("integration by parts"));
}

#[test]
fn an_exact_query_without_a_closed_form_shows_its_number() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("numeric.mpp");
    std::fs::write(
        &file,
        concat!(
            r"$$ \zeta(3) = ? $$",
            "\n\n",
            r"$$ \operatorname{erf}(\tfrac12) = ? $$",
            "\n\n",
            r"$$ \zeta(2) = ? $$",
            "\n"
        ),
    )
    .unwrap();
    let out = mpp().arg("run").arg(&file).output().unwrap();
    let s = text(&out.stdout);
    assert!(out.status.success(), "{s}");
    // No "(no answer)", and the number is marked as approximate.
    assert!(s.contains("ζ(3) ≈ 1.202056903"), "{s}");
    assert!(s.contains("erf(1/2) ≈ 0.5204998778"), "{s}");
    // A closed form stays exact.
    assert!(s.contains("ζ(2) = π"), "{s}");
}

#[test]
fn run_marks_relations_and_uses_an_arrow_for_equation_answers() {
    let dir = std::env::temp_dir().join(format!("mpp-cli-arrow-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("a.mpp");
    std::fs::write(
        &file,
        r"$$ 3\,\mathrm{m} < 5\,\mathrm{m} = ? $$

$$ \operatorname{dsolve}(y' = -2y, y, t) = ? $$
",
    )
    .unwrap();
    let out = mpp().arg("run").arg(&file).output().unwrap();
    let _ = std::fs::remove_dir_all(&dir);
    let s = text(&out.stdout);
    assert!(s.contains("3 m < 5 m = true ✓"), "{s}");
    assert!(s.contains("→ y(t)"), "{s}");
    assert!(!s.contains("= y(t)"), "{s}");
}

#[test]
fn check_warns_about_a_mistyped_function_name() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("typo.mpp");
    std::fs::write(&file, "$$ sinn(2) = ? $$\n\n$$ lnn(2) = ? $$\n").unwrap();
    let out = mpp().arg("check").arg(&file).output().unwrap();
    let err = text(&out.stderr) + &text(&out.stdout);
    assert!(err.contains("warning"), "{err}");
    assert!(err.contains("Did you mean the function `sin`?"), "{err}");
    assert!(err.contains("Did you mean the function `ln`?"), "{err}");
}

#[test]
fn check_leaves_option_words_and_defined_names_alone() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("words.mpp");
    std::fs::write(
        &file,
        "$$ speed := 3 $$\n\n$$ speed + 1 = ? $$\n\n$$ \\operatorname{riemann}_{left}(x^2, [0, 1], 4) = ? $$\n",
    )
    .unwrap();
    let out = mpp().arg("check").arg(&file).output().unwrap();
    let err = text(&out.stderr) + &text(&out.stdout);
    assert!(!err.contains("single letters"), "{err}");
}
