//! Formatted comments (`% note`, `%{ … %}`), citations, `References`, the title
//! block and the `%cite` / `%number sections` directives.

use mpp_syntax::document::{
    citation_diagnostics, dump::dump, parse_document, parse_reference_entries, BlockKind,
    CiteStyle, DirectiveKind,
};

fn clean(src: &str) -> String {
    let doc = parse_document(src);
    assert!(
        doc.diagnostics.is_empty(),
        "unexpected diagnostics:\n{}",
        dump(&doc)
    );
    dump(&doc)
}

#[test]
fn line_comments_are_blocks_with_prose_and_maths() {
    insta::assert_snapshot!(clean(concat!(
        "% A *note* with $x^2$ maths.\n",
        "% second line of the same note\n",
        "\n",
        "$$ y = x $$\n",
        "% a note straight after the maths\n",
        "\n",
        "$$ z = 1 $$ (4) % trailing note\n",
    )));
}

#[test]
fn block_comments_hold_any_blocks() {
    insta::assert_snapshot!(clean(concat!(
        "%{ Working:\n",
        "- first $a$\n",
        "- second\n",
        "\n",
        "| a | b |\n",
        "|---|---|\n",
        "| 1 | 2 |\n",
        "$$ x := 1 \\label{gone} $$\n",
        "%}\n",
        "After.\n",
        "%{ one line %}\n",
    )));
}

#[test]
fn directives_keep_the_word_after_the_percent() {
    let doc = parse_document("%noeval\n$$ x = 1 $$\n\n%sf 4\n\n%cite author-year\n\n%number sections\n\n% sf 4 is a comment\n\n%whatever\n");
    let kinds: Vec<String> = doc
        .blocks
        .iter()
        .map(|b| match &b.kind {
            BlockKind::Directive(d) => format!("{:?}", d.kind),
            BlockKind::Comment(_) => "comment".into(),
            _ => "other".into(),
        })
        .collect();
    assert_eq!(
        kinds,
        [
            "NoEval",
            "other",
            "Display",
            "Cite(AuthorYear)",
            "NumberSections",
            "comment",
            "Comment"
        ]
    );
    assert_eq!(doc.cite_style(), CiteStyle::AuthorYear);
    assert!(matches!(
        doc.blocks[0].kind,
        BlockKind::Directive(ref d) if d.kind == DirectiveKind::NoEval
    ));
}

#[test]
fn comments_define_nothing() {
    let doc = parse_document(concat!(
        "%{\n",
        "$$ a := 1 \\label{inside} $$\n",
        "%}\n",
        "$$ b := 2 \\label{outside} $$\n",
        "See \\eqref{inside} and \\eqref{outside}.\n",
    ));
    let errors: Vec<_> = doc.errors().map(|d| d.message.clone()).collect();
    assert_eq!(errors.len(), 1, "{errors:?}");
    assert!(errors[0].contains("inside"));
    // The outside equation is still number 1: the comment took no number.
    let n = doc.blocks.iter().find_map(|b| match &b.kind {
        BlockKind::DisplayMath(m) => m.number,
        _ => None,
    });
    assert_eq!(n, Some(1));
}

#[test]
fn comment_problems_are_not_reported() {
    // An unclosed `$$` and a bad formula inside a comment are not errors.
    let doc = parse_document("% broken $x +$ and $$ never closed\n\nAfter.\n");
    assert!(doc.diagnostics.is_empty(), "{}", dump(&doc));
}

#[test]
fn an_unclosed_block_comment_is_an_error() {
    let doc = parse_document("%{ never closed\nstill comment\n");
    assert_eq!(doc.errors().count(), 1);
    match &doc.blocks[0].kind {
        BlockKind::Comment(c) => assert!(!c.closed && c.braces),
        other => panic!("{other:?}"),
    }
}

#[test]
fn comments_between_noeval_and_maths_keep_the_flag() {
    let doc = parse_document("%noeval\n% why\n$$ x = 1 $$\n");
    assert!(doc.diagnostics.is_empty(), "{}", dump(&doc));
    assert!(doc
        .blocks
        .iter()
        .any(|b| matches!(&b.kind, BlockKind::DisplayMath(m) if m.noeval)));
}

#[test]
fn title_block_and_references() {
    insta::assert_snapshot!(clean(concat!(
        "% a leading comment is fine\n",
        "Title: Projectile *motion*\n",
        "Author: Ben Elmore\n",
        "Date: 20 November 2026\n",
        "Abstract: We study $y(x)$\n",
        "on a slope.\n",
        "\n",
        "# Intro\n",
        "Following [@knuth84] and [@lee20; @knuth84], see [@lee20].\n",
        "\n",
        "References\n",
        "[@knuth84] Knuth, D. E. | The TeXbook | 1984 | https://example.org/tex\n",
        "[@lee20]: Lee, K. and Park, H. | Slopes | 2020\n",
    )));
}

#[test]
fn title_lines_only_count_at_the_top() {
    let doc = parse_document("Some text.\n\nTitle: not a title\n");
    assert!(doc
        .blocks
        .iter()
        .all(|b| !matches!(b.kind, BlockKind::Title(_))));
}

#[test]
fn unknown_citation_keys_are_errors() {
    let doc = parse_document(concat!(
        "See [@nobody] and [@known].\n\n",
        "References\n",
        "[@known] A. Person | A Book | 2001\n",
    ));
    let errs: Vec<_> = doc.errors().collect();
    assert_eq!(errs.len(), 1, "{}", dump(&doc));
    assert!(errs[0].message.contains("nobody"));
    assert!(errs[0].expected.iter().any(|e| e.contains("known")));
}

#[test]
fn citing_without_any_references_is_an_error() {
    let doc = parse_document("Text [@x].\n");
    assert_eq!(doc.errors().count(), 1);
}

#[test]
fn a_references_file_defers_the_check() {
    let src = "Text [@a] and [@b].\n\nReferences from [[refs.mpp]]\n";
    let doc = parse_document(src);
    assert!(doc.diagnostics.is_empty(), "{}", dump(&doc));
    assert!(doc.has_references_file());
    let entries = parse_reference_entries("# Refs\n[@a] X | Y | 1999\nnoise\n");
    let errs = citation_diagnostics(&doc, &entries);
    assert_eq!(errs.len(), 1);
    assert!(errs[0].message.contains("`b`"));
}

#[test]
fn duplicate_and_malformed_entries_are_errors() {
    let doc = parse_document(concat!(
        "References\n",
        "[@a] X | One | 1\n",
        "[@a] Y | Two | 2\n",
        "[@c] only an author\n",
    ));
    assert_eq!(doc.errors().count(), 2, "{}", dump(&doc));
}

#[test]
fn bracket_at_signs_that_are_not_citations_stay_text() {
    insta::assert_snapshot!(clean("Mail [@ me] or [@a b] or [@].\n"));
}

#[test]
fn every_new_form_survives_ragged_input() {
    for src in [
        "%{",
        "%}",
        "%{ %}",
        "%",
        "% ",
        "$$ x $$ %",
        "$$ x $$ (2) %",
        "References",
        "References from [[",
        "References from [[x.mpp]] trailing",
        "[@",
        "[@a",
        "[@a;",
        "Title:",
        "Abstract:",
        "Abstract:\nbody",
        "%{\n%{\n%}\n",
    ] {
        let _ = parse_document(src);
    }
}
