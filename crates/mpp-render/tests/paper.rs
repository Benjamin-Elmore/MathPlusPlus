//! The optional paper features of the HTML page: formatted comments, the title
//! block, numbered sections and theorems, citations and references, footnote
//! hovers. A plain worksheet must render exactly as it did without any of them.

use std::path::PathBuf;

use mpp_render::{
    render_html, render_html_with, HtmlOptions, LineResult, NoResults, ResultProvider, Theme,
};
use mpp_syntax::document::{parse_document, Block, DisplayMath};

fn page(src: &str, opts: &HtmlOptions) -> String {
    render_html_with(&parse_document(src), &NoResults, opts)
}

fn opts() -> HtmlOptions {
    HtmlOptions {
        embed_fonts: false,
        ..HtmlOptions::default()
    }
}

fn with_comments() -> HtmlOptions {
    HtmlOptions {
        include_comments: true,
        ..opts()
    }
}

/// The page body with the style sheet gone and every drawing collapsed, so a
/// snapshot shows the structure.
fn skeleton(html: &str) -> String {
    let body = html.split("<main").nth(1).unwrap_or(html);
    let mut out = String::new();
    let mut rest = body;
    while let Some(at) = rest.find("<svg") {
        out.push_str(&rest[..at]);
        match rest[at..].find("</svg>") {
            Some(end) => {
                out.push_str("<svg/>");
                rest = &rest[at + end + "</svg>".len()..];
            }
            None => {
                rest = "";
            }
        }
    }
    out.push_str(rest);
    out
}

fn example(name: &str) -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples")
        .join(name);
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path:?}: {e}"))
}

const PLAIN: &str = "# A worksheet\n\nLet $a \\equiv 2$.\n\n$$ a^2 = ? $$\n\n**Theorem.** It holds.\n\n**Proof.** Yes. ∎\n\nA note[^n].\n\n[^n]: Small.\n";

#[test]
fn a_plain_worksheet_is_untouched() {
    let plain = page(PLAIN, &opts());
    assert_eq!(plain, page(PLAIN, &with_comments()));
    for marker in [
        "<aside",
        "<header class=\"title-block\"",
        "class=\"references\"",
        "<span class=\"sec-num\"",
        "class=\"cite",
    ] {
        assert!(!plain.contains(marker), "{marker}");
    }
    assert!(plain.contains("<h1>A worksheet</h1>"));
    assert!(plain.contains("<span class=\"thm-head\">Theorem 1.</span>"));
}

// ---- comments ---------------------------------------------------------------

const NOTES: &str = "Text.

% a *note* with $x^2$

$$ y = 1 $$
% after the maths

%{ with prose:
$$ w = 3 $$
%}

%{
$$ z = 2 \\label{gone} $$
%}

End.
";

#[test]
fn comments_are_left_out_unless_asked_for() {
    let hidden = page(NOTES, &opts());
    assert!(!hidden.contains("<aside"));
    assert!(!hidden.contains("a <em>note</em>"));
    let shown = page(NOTES, &with_comments());
    assert_eq!(shown.matches("<aside").count(), 4);
    assert!(shown.contains("<aside class=\"note\">"));
    assert!(shown.contains("<aside class=\"note off\">"));
    assert!(shown.contains("a <em>note</em> with <svg class=\"m\""));
    assert!(shown.contains("✎"));
    // A commented-out equation takes no number and no anchor.
    assert!(!shown.contains("class=\"eq-num\""));
    assert!(!shown.contains("id=\"eq-"));
}

struct NoCalls;

impl ResultProvider for NoCalls {
    fn display_results(&self, _block: &Block, _math: &DisplayMath) -> Vec<LineResult> {
        panic!("the maths in a comment is never evaluated");
    }
}

#[test]
fn comment_maths_is_never_given_results() {
    let doc = parse_document("%{\n$$ x = ? $$\n%}\n");
    let html = render_html_with(&doc, &NoCalls, &with_comments());
    assert!(html.contains("note off"));
}

#[test]
fn comments_take_nothing_from_the_page() {
    let src = "% Theorem and footnote: **Theorem.** x[^a] [@k]\n\n**Theorem.** Real.\n";
    let html = page(src, &with_comments());
    // The commented theorem is not counted: the real one is number 1.
    assert!(html.contains("Theorem 1."));
    assert!(!html.contains("class=\"footnotes\""));
}

#[test]
fn note_colours_follow_the_theme() {
    let html = page(NOTES, &with_comments());
    assert!(html.contains(&format!(
        "--mpp-note-background: {};",
        Theme::LIGHT.note_background.hex()
    )));
    assert!(html.contains(&format!(
        "--mpp-note-accent: {};",
        Theme::DARK.note_accent.hex()
    )));
    assert_ne!(Theme::LIGHT.note_background, Theme::DARK.note_background);
    assert!(html.contains("background: var(--mpp-note-background)"));
}

// ---- title block, sections, theorems ----------------------------------------

const TITLE: &str = "Title: On *ranges*\nAuthor: Ben\nAuthor: Kyle\nDate: 1 May 2027\nAbstract: A short $a$.\n\n# Intro\n\nText.\n";

#[test]
fn the_title_block_is_typeset() {
    let html = page(TITLE, &opts());
    assert!(html.contains("<title>On ranges</title>"));
    assert!(html.contains("<h1 class=\"doc-title\">On <em>ranges</em></h1>"));
    assert_eq!(html.matches("class=\"doc-author\"").count(), 2);
    assert!(html.contains("<p class=\"doc-date\">1 May 2027</p>"));
    assert!(html.contains("<p class=\"abstract-head\">Abstract</p>"));
    assert!(html.contains("<h1>Intro</h1>"));
}

const SECTIONS: &str = "%number sections\n\n# Algebra\n\n**Theorem.** One.\n\n**Lemma.** Two.\n\n## Detail\n\n**Remark.** None.\n\n# Calculus\n\n**Definition.** Three.\n\n**Proof.** Done.\n";

#[test]
fn theorems_are_numbered_per_section_on_request() {
    let html = page(SECTIONS, &opts());
    assert!(html.contains("<h1><span class=\"sec-num\">1</span> Algebra</h1>"));
    assert!(html.contains("<h2><span class=\"sec-num\">1.1</span> Detail</h2>"));
    assert!(html.contains("<h1><span class=\"sec-num\">2</span> Calculus</h1>"));
    for label in [
        "Theorem 1.1.",
        "Lemma 1.2.",
        "Definition 2.1.",
        "Remark.",
        "Proof.",
    ] {
        assert!(
            html.contains(&format!("<span class=\"thm-head\">{label}</span>")),
            "{label}"
        );
    }
    // The proof still ends with its ∎.
    assert!(html.contains("<span class=\"qed\">∎</span>"));
}

#[test]
fn a_single_hash_is_the_title_when_sections_are_numbered() {
    let html = page(
        "%number sections\n\n# Title\n\n## One\n\n**Theorem.** A.\n",
        &opts(),
    );
    assert!(html.contains("<h1>Title</h1>"));
    assert!(html.contains("<h2><span class=\"sec-num\">1</span> One</h2>"));
    assert!(html.contains("Theorem 1.1."));
}

// ---- citations and references -----------------------------------------------

const CITES: &str = "See [@b], then [@a; @b], and [@zzz].\n\nReferences\n[@a] Smith, J. and Lee, K. | Alpha | 2001\n[@b] Brown, A. | Beta *study* | 1999 | https://example.org/b\n[@c] Quine, W. | Gamma | 1950\n";

#[test]
fn citations_are_numbered_in_order_of_first_use() {
    let html = page(CITES, &opts());
    assert!(html.contains("[<a class=\"cite\" href=\"#ref-b\">1</a>]"));
    assert!(html.contains(
        "[<a class=\"cite\" href=\"#ref-a\">2</a>, <a class=\"cite\" href=\"#ref-b\">1</a>]"
    ));
    assert!(html.contains("class=\"cite-missing\""));
    // The list: cited entries as first cited, then the uncited one.
    let list = html
        .split("<ol class=\"refs numbered\">")
        .nth(1)
        .unwrap_or("");
    let b = list.find("id=\"ref-b\"").unwrap_or(usize::MAX);
    let a = list.find("id=\"ref-a\"").unwrap_or(usize::MAX);
    let c = list.find("id=\"ref-c\"").unwrap_or(usize::MAX);
    assert!(b < a && a < c, "{list}");
    assert!(list.contains("[1]</span> Brown, A. <em>Beta *study*</em>, 1999."));
    assert!(list.contains("<a href=\"https://example.org/b\">"));
}

#[test]
fn author_year_style_lists_alphabetically() {
    let html = page(&format!("%cite author-year\n\n{CITES}"), &opts());
    assert!(html.contains("(<a class=\"cite\" href=\"#ref-b\">Brown 1999</a>)"));
    assert!(html.contains("(<a class=\"cite\" href=\"#ref-a\">Smith and Lee 2001</a>; "));
    let list = html
        .split("<ol class=\"refs author-year\">")
        .nth(1)
        .unwrap_or("");
    let b = list.find("id=\"ref-b\"").unwrap_or(usize::MAX);
    let a = list.find("id=\"ref-a\"").unwrap_or(usize::MAX);
    let c = list.find("id=\"ref-c\"").unwrap_or(usize::MAX);
    assert!(b < c && c < a, "{list}");
}

struct RefsFile;

impl ResultProvider for RefsFile {
    fn references_source(&self, path: &str) -> Option<String> {
        (path == "refs.mpp").then(|| "[@a] X, Y. | From a file | 2010\n".to_string())
    }
}

#[test]
fn references_can_come_from_a_file() {
    let doc = parse_document("Cite [@a].\n\nReferences from [[refs.mpp]]\n");
    assert!(doc.diagnostics.is_empty());
    let html = render_html_with(&doc, &RefsFile, &opts());
    assert!(html.contains("From a file"));
    assert!(html.contains("[<a class=\"cite\" href=\"#ref-a\">1</a>]"));
    let without = render_html(&doc, &NoResults);
    assert!(without.contains("are not loaded"));
    assert!(without.contains("class=\"cite-missing\""));
}

#[test]
fn footnotes_show_their_note_on_hover() {
    let html = page("Claim[^1].\n\n[^1]: The *reason*.\n", &opts());
    assert!(html.contains("<sup class=\"fnref\" id=\"fnref-1\" title=\"The reason.\">"));
    assert!(html.contains("<ol class=\"footnotes\">"));
}

// ---- the examples -----------------------------------------------------------

#[test]
fn the_comments_example_page() {
    let src = example("comments.mpp");
    let doc = parse_document(&src);
    assert!(doc.diagnostics.is_empty());
    insta::assert_snapshot!(skeleton(&render_html_with(
        &doc,
        &NoResults,
        &with_comments()
    )));
    let bare = skeleton(&render_html_with(&doc, &NoResults, &opts()));
    assert!(!bare.contains("<aside"));
    assert!(!bare.contains("longer note"));
    write_page(
        "comments",
        &render_html_with(&doc, &NoResults, &with_comments()),
    );
}

#[test]
fn the_paper_features_example_page() {
    let src = example("paper-features.mpp");
    let doc = parse_document(&src);
    assert!(doc.diagnostics.is_empty(), "{:?}", doc.diagnostics);
    let html = render_html_with(&doc, &NoResults, &with_comments());
    insta::assert_snapshot!(skeleton(&html));
    write_page("paper-features", &html);
    let author_year = src.replace("%cite numbered", "%cite author-year");
    let doc = parse_document(&author_year);
    let html = render_html_with(&doc, &NoResults, &with_comments());
    insta::assert_snapshot!("paper_features_author_year", skeleton(&html));
    write_page("paper-features-author-year", &html);
}

fn write_page(name: &str, html: &str) {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("mpp-render-pages");
    let _ = std::fs::create_dir_all(&dir);
    let light = html.replacen(
        "<html lang=\"en\">",
        "<html lang=\"en\" class=\"light\">",
        1,
    );
    let _ = std::fs::write(dir.join(format!("{name}.html")), light);
    let dark = html.replacen("<html lang=\"en\">", "<html lang=\"en\" class=\"dark\">", 1);
    let _ = std::fs::write(dir.join(format!("{name}-dark.html")), dark);
}
