//! The document parser never panics, and every span it reports lies inside
//! the source on character boundaries, whatever the input.

use mpp_syntax::document::{block_at, parse_document, BlockKind, Document};
use proptest::prelude::*;

/// Checks the invariants that hold for any input.
fn check(src: &str) -> Document {
    let doc = parse_document(src);
    let ok = |s: mpp_syntax::Span| {
        s.start <= s.end
            && s.end <= src.len()
            && src.is_char_boundary(s.start)
            && src.is_char_boundary(s.end)
    };
    doc.walk(&mut |b| {
        assert!(ok(b.span), "block span {:?} in {src:?}", b.span);
        if let BlockKind::DisplayMath(m) = &b.kind {
            assert!(ok(m.raw), "raw span {:?} in {src:?}", m.raw);
        }
    });
    for d in &doc.diagnostics {
        assert!(ok(d.span), "diagnostic {d} in {src:?}");
    }
    // `block_at` searches by halves: it finds what a walk over the blocks finds.
    for off in 0..=src.len() {
        let want = linear_block_at(&doc.blocks, off).map(|b| b.span);
        assert_eq!(
            block_at(&doc, off).map(|b| b.span),
            want,
            "at {off} in {src:?}"
        );
    }
    // A parse whose formulas come from the parser's memo (moved to new offsets) is
    // the same as a fresh parse: parse the text again behind a prefix and compare.
    let shifted = format!("Words.\n\n{src}");
    let again = parse_document(&shifted);
    let fresh = std::thread::spawn(move || parse_document(&shifted))
        .join()
        .expect("thread");
    assert_eq!(format!("{again:?}"), format!("{fresh:?}"), "in {src:?}");
    doc
}

/// The innermost block holding `offset`, by walking every block (what `block_at`
/// did before it searched by halves).
fn linear_block_at(
    blocks: &[mpp_syntax::document::Block],
    offset: usize,
) -> Option<&mpp_syntax::document::Block> {
    let hit = blocks
        .iter()
        .find(|b| b.span.start <= offset && offset < b.span.end)
        .or_else(|| {
            blocks
                .iter()
                .find(|b| b.span.start <= offset && offset == b.span.end)
        })?;
    if let BlockKind::Theorem(t) = &hit.kind {
        if let Some(inner) = linear_block_at(&t.body, offset) {
            return Some(inner);
        }
    }
    Some(hit)
}

const PIECES: &[&str] = &[
    "$",
    "$$",
    "[[",
    "]]",
    "[",
    "]",
    "(",
    ")",
    "*",
    "**",
    "`",
    "```",
    "\\",
    "\\\\",
    "\\eqref{a}",
    "\\label{a}",
    "|",
    "|---|",
    ":-:",
    "#",
    "##",
    "---",
    "- ",
    "1. ",
    "  ",
    "\t",
    "\n",
    "\n\n",
    "\r\n",
    ">",
    "> ",
    "%",
    "%noeval",
    "![[",
    "[^1]",
    "[^1]: ",
    "Figure: ",
    "polar ",
    ", ",
    ". ",
    "Using ",
    " from ",
    "Find the domain of ",
    "Solve ",
    "Let ",
    "**Proof.** ",
    "**Theorem.** ",
    "∎",
    "\\qed",
    "x",
    "y^2",
    "é",
    "日本",
    "∑",
    "😀",
    "\\frac{1}{2}",
    "a := 1",
    " = ?",
    "\\begin{aligned}",
    "&",
    "\\in [0, 1]",
];

fn pieces() -> impl Strategy<Value = String> {
    prop::collection::vec(prop::sample::select(PIECES), 0..40).prop_map(|v| v.concat())
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(512))]

    #[test]
    fn any_text(src in ".{0,200}") {
        check(&src);
    }

    #[test]
    fn any_text_with_newlines(src in "(.|\n){0,200}") {
        check(&src);
    }

    #[test]
    fn markup_soup(src in pieces()) {
        check(&src);
    }

    #[test]
    fn cut_and_spliced_documents(cut in 0usize..400, at in 0usize..400, ins in pieces()) {
        let base = std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/grammar.mpp"),
        )
        .unwrap();
        let mut end = cut.min(base.len());
        while !base.is_char_boundary(end) {
            end -= 1;
        }
        let mut at = at.min(end);
        while !base.is_char_boundary(at) {
            at -= 1;
        }
        let spliced = format!("{}{}{}", &base[..at], ins, &base[at..end]);
        check(&spliced);
    }
}

#[test]
fn deep_and_long_inputs() {
    for n in [50, 500, 5000] {
        check(&"*a ".repeat(n));
        check(&"**a *b ".repeat(n));
        check(&"[".repeat(n));
        check(&"[[".repeat(n));
        check(&"[a](".repeat(n));
        check(&"$".repeat(n));
        check(&"$$ ".repeat(n));
        check(&"\\".repeat(n));
        check(&"> ".repeat(n));
        check(&"| a ".repeat(n));
        check(&"  - x\n".repeat(n));
        check(
            &(0..n)
                .map(|i| format!("{}- x\n", " ".repeat(i)))
                .collect::<String>(),
        );
        check(&"**Proof.** x\n".repeat(n));
        check(&"%noeval\n".repeat(n));
        check(&"```\n".repeat(n));
    }
}

#[test]
fn whole_file_in_one_line_and_empty() {
    check("");
    check("\n");
    check("\r");
    check("   ");
    check("$$");
    check("$$\n");
    check("```");
    check("|");
    check("|\n|");
    check("Figure:");
    check("Figure: $");
    check("**Proof.**");
    check("![[");
    check("[^]: x");
    check("1.");
    check("-");
}
