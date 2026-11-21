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

/// Control sequences that LaTeX, amsmath, amssymb, amsthm, mathtools, graphicx,
/// booktabs, xcolor, hyperref and geometry define (the packages the preamble loads).
const STANDARD_COMMANDS: &[&str] = &[
    // Greek.
    "alpha",
    "beta",
    "gamma",
    "delta",
    "epsilon",
    "varepsilon",
    "zeta",
    "eta",
    "theta",
    "vartheta",
    "iota",
    "kappa",
    "lambda",
    "mu",
    "nu",
    "xi",
    "pi",
    "varpi",
    "rho",
    "varrho",
    "sigma",
    "varsigma",
    "tau",
    "upsilon",
    "phi",
    "varphi",
    "chi",
    "psi",
    "omega",
    "Gamma",
    "Delta",
    "Theta",
    "Lambda",
    "Xi",
    "Pi",
    "Sigma",
    "Upsilon",
    "Phi",
    "Psi",
    "Omega",
    // Operators, relations and symbols.
    "pm",
    "mp",
    "times",
    "div",
    "cdot",
    "circ",
    "otimes",
    "oplus",
    "ast",
    "star",
    "bullet",
    "leq",
    "le",
    "geq",
    "ge",
    "neq",
    "ne",
    "approx",
    "equiv",
    "sim",
    "simeq",
    "cong",
    "propto",
    "ll",
    "gg",
    "prec",
    "succ",
    "perp",
    "parallel",
    "mid",
    "in",
    "notin",
    "ni",
    "subset",
    "subseteq",
    "supset",
    "supseteq",
    "cup",
    "cap",
    "setminus",
    "emptyset",
    "varnothing",
    "forall",
    "exists",
    "nexists",
    "neg",
    "lnot",
    "land",
    "wedge",
    "lor",
    "vee",
    "implies",
    "Rightarrow",
    "iff",
    "Leftrightarrow",
    "therefore",
    "because",
    "partial",
    "nabla",
    "int",
    "iint",
    "iiint",
    "oint",
    "sum",
    "prod",
    "coprod",
    "infty",
    "prime",
    "ell",
    "hbar",
    "Re",
    "Im",
    "aleph",
    "to",
    "rightarrow",
    "gets",
    "leftarrow",
    "leftrightarrow",
    "Leftarrow",
    "mapsto",
    "uparrow",
    "downarrow",
    "longrightarrow",
    "Longrightarrow",
    "longleftarrow",
    "Longleftarrow",
    "longleftrightarrow",
    "Longleftrightarrow",
    "longmapsto",
    "hat",
    "widehat",
    "bar",
    "overline",
    "vec",
    "dot",
    "ddot",
    "tilde",
    "widetilde",
    "mathbb",
    "mathcal",
    "mathbf",
    "mathfrak",
    "mathrm",
    "mathit",
    "mathsf",
    "boldsymbol",
    "langle",
    "rangle",
    "lfloor",
    "rfloor",
    "lceil",
    "rceil",
    "vert",
    "Vert",
    "ldots",
    "dots",
    "cdots",
    "vdots",
    "ddots",
    "angle",
    "triangle",
    "qedsymbol",
    "checkmark",
    "square",
    "blacksquare",
    "sin",
    "cos",
    "tan",
    "sec",
    "csc",
    "cot",
    "arcsin",
    "arccos",
    "arctan",
    "sinh",
    "cosh",
    "tanh",
    "coth",
    "ln",
    "log",
    "exp",
    "sqrt",
    "det",
    "dim",
    "ker",
    "gcd",
    "max",
    "min",
    "sup",
    "inf",
    "lim",
    "liminf",
    "limsup",
    "deg",
    "arg",
    "Pr",
    "operatorname",
    "frac",
    "tfrac",
    "dfrac",
    "binom",
    "overset",
    "underset",
    "stackrel",
    "left",
    "right",
    "big",
    "Big",
    "bigg",
    "Bigg",
    "bigl",
    "bigr",
    "middle",
    "quad",
    "qquad",
    "text",
    "textrm",
    "mathop",
    "mkern",
    "nolimits",
    "limits",
    "ensuremath",
    "coloneqq",
    "eqqcolon",
    "colon",
    "ldotp",
    "cdotp",
    "not",
    "lvert",
    "rvert",
    "lVert",
    "rVert",
    "tag",
    "label",
    "eqref",
    "ref",
    "displaystyle",
    "textstyle",
    "scriptstyle",
    "underbrace",
    "overbrace",
    "boxed",
    "pmod",
    "bmod",
    "mod",
    "substack",
    "cfrac",
    "smallsetminus",
    "leqslant",
    "geqslant",
    "lesssim",
    "gtrsim",
    "vartriangle",
    "top",
    "bot",
    "wp",
    "Box",
    "Diamond",
    "dagger",
    "ddagger",
    "S",
    "P",
    "backslash",
    "lbrace",
    "rbrace",
    "hline",
    "phantom",
    "hphantom",
    "vphantom",
    // Text and document structure.
    "documentclass",
    "usepackage",
    "begin",
    "end",
    "title",
    "author",
    "date",
    "maketitle",
    "section",
    "subsection",
    "subsubsection",
    "paragraph",
    "item",
    "emph",
    "textit",
    "textbf",
    "texttt",
    "textsc",
    "textbackslash",
    "footnote",
    "cite",
    "bibitem",
    "url",
    "href",
    "includegraphics",
    "caption",
    "centering",
    "linewidth",
    "textwidth",
    "newtheorem",
    "theoremstyle",
    "setlength",
    "parindent",
    "parskip",
    "toprule",
    "midrule",
    "bottomrule",
    "cmidrule",
    "addlinespace",
    "small",
    "footnotesize",
    "large",
    "Large",
    "normalsize",
    "color",
    "textcolor",
    "pagecolor",
    "medskip",
    "smallskip",
    "bigskip",
    "hrule",
    "par",
    "newline",
    "noindent",
    "hfill",
    "vspace",
    "hspace",
    "setcounter",
    "providecommand",
    "newcommand",
    "renewcommand",
    "qed",
    "hyperref",
    "today",
    "LaTeX",
    "TeX",
    "ldots",
    "thebibliography",
    "textendash",
    "textemdash",
    "dag",
    "copyright",
    "pounds",
    "euro",
];

/// The control sequences (`\name`) used in `tex`, outside verbatim blocks.
fn control_sequences(tex: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut verbatim = false;
    for line in tex.lines() {
        if line.contains("\\begin{verbatim}") {
            verbatim = true;
        }
        if line.contains("\\end{verbatim}") {
            verbatim = false;
            continue;
        }
        if verbatim {
            continue;
        }
        let b = line.as_bytes();
        let mut k = 0;
        while k < b.len() {
            if b[k] == b'\\' {
                let start = k + 1;
                let mut j = start;
                while j < b.len() && b[j].is_ascii_alphabetic() {
                    j += 1;
                }
                if j > start {
                    out.push(line[start..j].to_string());
                    k = j;
                } else {
                    k = start + 1;
                }
            } else {
                k += 1;
            }
        }
    }
    out
}

/// The commands a document defines itself (`\providecommand{\x}`, `\newcommand{\x}`).
fn defined_commands(tex: &str) -> Vec<String> {
    let mut out = Vec::new();
    for how in ["\\providecommand{\\", "\\newcommand{\\"] {
        let mut rest = tex;
        while let Some(i) = rest.find(how) {
            rest = &rest[i + how.len()..];
            let name: String = rest.chars().take_while(char::is_ascii_alphabetic).collect();
            out.push(name);
        }
    }
    out
}

#[test]
fn every_command_in_an_exported_tex_file_is_defined() {
    // `\xmark` and `\degree` used to reach pdflatex undefined.
    let mut files: Vec<(String, PathBuf)> = examples();
    let worksheet = std::env::temp_dir().join(format!("mpp-commands-{}", std::process::id()));
    std::fs::create_dir_all(&worksheet).unwrap();
    let extra = worksheet.join("symbols.mpp");
    std::fs::write(
        &extra,
        concat!(
            "Let $\\theta = 35\\degree$.\n\n",
            "$$ \\frac{d}{dx} \\sin^2 x \\overset{?}{=} \\cos 2x $$\n\n",
            "$$ \\oiint \\omicron \\, dA $$\n",
        ),
    )
    .unwrap();
    files.push(("symbols".into(), extra));
    let mut problems = Vec::new();
    for (name, path) in &files {
        let out = export_path(path, Format::Tex, &ExportOptions::default()).unwrap();
        let tex = String::from_utf8(out.main).unwrap();
        let defined = defined_commands(&tex);
        for cs in control_sequences(&tex) {
            if !STANDARD_COMMANDS.contains(&cs.as_str()) && !defined.contains(&cs) {
                problems.push(format!("{name}: \\{cs}"));
            }
        }
    }
    problems.sort();
    problems.dedup();
    let _ = std::fs::remove_dir_all(&worksheet);
    assert!(
        problems.is_empty(),
        "undefined control sequences:\n{}",
        problems.join("\n")
    );
}

#[test]
fn consecutive_numbered_items_stay_in_one_list() {
    // Each item used to get its own `enumerate`, so every problem was numbered 1.
    let dir = std::env::temp_dir().join(format!("mpp-enum-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let f = dir.join("items.mpp");
    std::fs::write(
        &f,
        concat!(
            "1. First: $$ 1 + 1 = ? $$\n\n",
            "2. Solve $x^2 - 5x + 6 = 0$ for $x$.\n\n",
            "3. Third: $$ \\sin(5\\,\\mathrm{m}) = ? $$\n\n",
            "## Next part\n\n",
            "4. Fourth: $$ 2 + 2 = ? $$\n",
        ),
    )
    .unwrap();
    let out = export_path(&f, Format::Tex, &ExportOptions::default()).unwrap();
    let tex = String::from_utf8(out.main).unwrap();
    let body = &tex[tex.find("\\begin{document}").unwrap()..];
    // Items 1–3 in one list; after the heading, a new list that starts at 4.
    assert_eq!(body.matches("\\begin{enumerate}").count(), 2, "{body}");
    assert_eq!(body.matches("\\item").count(), 4, "{body}");
    assert!(body.contains("\\setcounter{enumi}{3}"), "{body}");
    // The Solve sentence shows its answer, and a failed query its reason.
    assert!(body.contains("x = 2"), "{body}");
    assert!(
        body.contains("$\\times$ sin needs a dimensionless"),
        "{body}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn accented_letters_and_reference_lists_read_well() {
    let dir = std::env::temp_dir().join(format!("mpp-refs-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let f = dir.join("refs.mpp");
    std::fs::write(
        &f,
        "By L'Hôpital's rule [@spivak].\n\n## References\n\n- [@spivak] Spivak, M. | Calculus | 1967\n",
    )
    .unwrap();
    // TeX used to write `L'H?pital`.
    let tex = export_path(&f, Format::Tex, &ExportOptions::default()).unwrap();
    let tex = String::from_utf8(tex.main).unwrap();
    assert!(tex.contains("L'Hôpital"), "{tex}");
    // The HTML reference list used to have a bullet before each `[1]`.
    let html = export_path(&f, Format::Html, &ExportOptions::default()).unwrap();
    let html = String::from_utf8(html.main).unwrap();
    assert!(html.contains("<ol class=\"refs\">"), "{html}");
    let _ = std::fs::remove_dir_all(&dir);
}
