//! The document parser: one snapshot per block and inline kind, the example
//! file from the work queue, the repository's own `.mpp` files and error recovery.

use mpp_syntax::document::{block_at, dump::dump, parse_document, BlockKind, DirectiveKind};

/// Parses `src` and returns its outline.
fn outline(src: &str) -> String {
    dump(&parse_document(src))
}

/// Parses `src`, which must have no diagnostics, and returns its outline.
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
fn headings_and_rules() {
    insta::assert_snapshot!(clean(
        "# One\n## Two *em*\n###### Six\n####### seven hashes\n#nospace\n\n---\n-----\n"
    ));
}

#[test]
fn paragraphs_and_inlines() {
    insta::assert_snapshot!(clean(concat!(
        "Plain *emphasis*, **strong**, **strong with *nested* em** and `code`.\n",
        "A second line with $x^2 + 1$ maths, a [[other.mpp]] link, [[a.mpp|an alias]],\n",
        "a [link](https://example.com/a_b), a note[^1] and \\eqref stays.\n",
        "Escapes: \\$ \\* \\[ \\] \\\\ \\| \\# \\% and a lone * star, 2 * 3 * 4.\n",
    )));
}

#[test]
fn lists() {
    insta::assert_snapshot!(clean(concat!(
        "- one\n",
        "- two with a\n  wrapped line\n",
        "  - nested a\n",
        "  - nested b\n",
        "    1. deep ordered\n",
        "    2. second\n",
        "- three\n",
        "\n",
        "3. starts at three\n",
        "4. four\n",
        "* star item\n",
    )));
}

#[test]
fn tables() {
    insta::assert_snapshot!(clean(concat!(
        "| Name | Value | Mid | Right |\n",
        "|---|:--|:-:|--:|\n",
        "| $a \\mid b$ | *x* | `p|q` | 1 \\| 2 |\n",
        "| last | 1 | 2 | 3 |\n",
    )));
}

#[test]
fn table_errors() {
    insta::assert_snapshot!(outline(
        "| a | b |\n|---|\n| 1 |\n\nNot a table: |x| is the absolute value.\n"
    ));
}

#[test]
fn quotes_and_footnotes() {
    insta::assert_snapshot!(clean(concat!(
        "> A quote with $x$\n",
        "> over two lines.\n",
        ">\n",
        "> Second paragraph.\n",
        "\n",
        "[^1]: The note, with *emphasis*.\n",
    )));
}

#[test]
fn verbatim_fences() {
    insta::assert_snapshot!(clean(concat!(
        "```rust\n",
        "let x = $$ not maths $$;\n",
        "# not a heading\n",
        "```\n",
        "\n",
        "```\n",
        "```\n",
    )));
}

#[test]
fn display_math() {
    insta::assert_snapshot!(clean(concat!(
        "$$ y(x) := x^2 $$\n",
        "\n",
        "$$ a = 1 \\label{eq:a} $$\n",
        "\n",
        "$$\n",
        "\\begin{aligned}\n",
        "b &:= 2 \\\\\n",
        "c &:= 3\n",
        "\\end{aligned} \\label{eq:b}\n",
        "$$\n",
        "\n",
        "$$ d = 4 $$ (7)\n",
        "\n",
        "$$ e = 5 \\label{eq:e} $$\n",
        "\n",
        "See \\eqref{eq:a}, \\eqref{eq:b} and \\eqref{eq:e}.\n",
    )));
}

#[test]
fn noeval_directive() {
    let doc = parse_document("% a comment\n%noeval\n$$ 1 + 1 = ? $$\n\n$$ 2 + 2 = ? $$\n");
    insta::assert_snapshot!(dump(&doc));
    let flags: Vec<bool> = doc
        .blocks
        .iter()
        .filter_map(|b| match &b.kind {
            BlockKind::DisplayMath(m) => Some(m.noeval && m.ast.lines.iter().all(|l| l.noeval)),
            _ => None,
        })
        .collect();
    assert_eq!(flags, [true, false]);
    assert!(matches!(
        &doc.blocks[1].kind,
        BlockKind::Directive(d) if d.kind == DirectiveKind::NoEval
    ));
}

#[test]
fn figures() {
    insta::assert_snapshot!(clean(concat!(
        "Figure: $y(x)$, $x \\in [0, R]$.\n",
        "Figure: $z = x^2 - y^2$, $x, y \\in [-2, 2]$. A saddle, with *caption*.\n",
        "Figure: polar $r = 1 + \\cos\\theta$, $\\theta \\in [0, 2\\pi]$.\n",
        "Figure: $f(x)$, tangent at $x = 1$, riemann left, slope field.\n",
    )));
}

#[test]
fn imports() {
    insta::assert_snapshot!(clean("![[constants.mpp]]\n\n  ![[ sub/dir.mpp ]]\n"));
}

#[test]
fn sentence_forms() {
    insta::assert_snapshot!(clean(concat!(
        "Using $\\theta, R(\\theta)$ from [[launch.mpp]].\n\n",
        "using $a, b$ as $c, d$ from [[x.mpp]]\n\n",
        "Find the domain of $f(x) = \\sqrt{x}$.\n\n",
        "Compute the tangent line of $x^2$ at $x = 1$.\n\n",
        "determine the extrema of $x^3 - 3x$ on $[0, 4]$.\n\n",
        "Find the linearization of $\\sin x$ on $[0, 1]$ at $x = 0$.\n\n",
        "Analyse $x^2 e^{-x}$ on $[0, 5]$.\n\n",
        "Analyze $\\ln x$.\n\n",
        "Solve $x^2 = 4$ for $x$.\n\n",
        "Solve $x^2 - y = 0$.\n\n",
        "Let $a = 3$.\n\n",
        "Let $g \\equiv 9.81$ be gravity and $v_0 := 20$ the launch speed.\n",
    )));
}

#[test]
fn sentence_lines_split() {
    insta::assert_snapshot!(clean(
        "Using $a$ from [[x.mpp]].\nFind the range of $f$.\nSolve $f = 0$.\n"
    ));
}

#[test]
fn prose_that_looks_like_a_sentence() {
    insta::assert_snapshot!(clean(
        "Using $x$ we get the answer.\n\nLet us begin.\n\nFind the domain of nothing.\n\nSolve it.\n"
    ));
}

#[test]
fn theorems_and_proofs() {
    insta::assert_snapshot!(clean(concat!(
        "**Definition.** A *group* is a set $G$ with an operation.\n\n",
        "**Theorem.** For all $x$:\n",
        "$$ x = x \\label{eq:id} $$\n\n",
        "**Lemma.** Small.\n",
        "**Corollary.** Immediate.\n",
        "**Proposition.** p\n\n**Example.** e\n\n**Remark.** r\n\n",
        "**Proof.** First paragraph, uses \\eqref{eq:id}.\n\n",
        "Second paragraph with display\n",
        "$$ 1 = 1 $$\n\n",
        "Done. $\\qed$\n\n",
        "After the proof.\n\n",
        "**Proof.** Short. ∎\n",
        "Next paragraph is outside.\n",
    )));
}

#[test]
fn unclosed_proof_stops_at_heading() {
    insta::assert_snapshot!(outline("**Proof.** Never ends.\n\nMore.\n\n# Heading\n"));
}

// ---- the example from the work queue ----------------------------------------

const EXAMPLE: &str = "# Projectile motion
Let $g \\equiv 9.81$ be gravity and $v_0 := 20$ the launch speed.
Using $\\theta, R(\\theta)$ from [[launch.mpp]].
![[constants.mpp]]

$$ y(x) := x \\tan\\theta - \\frac{g x^2}{2 v_0^2 \\cos^2\\theta} $$
$$ E := \\tfrac12 m v_0^2 $$

$$ \\frac{d}{dx} y(x) = ? $$
$$ \\int_0^{R} y(x)\\,dx \\approx ? $$
$$ \\sum_{n=1}^{\\infty} \\frac{1}{n^2} = ? $$
$$ \\lim_{x\\to 0} \\frac{\\sin x}{x} = ? $$

Figure: $y(x)$, $x \\in [0, R]$.
Figure: $z = x^2 - y^2$, $x, y \\in [-2, 2]$.
";

#[test]
fn work_queue_example() {
    insta::assert_snapshot!(clean(EXAMPLE));
}

// ---- spans, ids, lookups -----------------------------------------------------

#[test]
fn maths_spans_index_the_whole_file() {
    let src = "Intro line\n\nText $a + $ more\n\n$$ x +\n$$\n";
    let doc = parse_document(src);
    assert!(doc.diagnostics.len() >= 2, "{}", dump(&doc));
    let inline_err = doc.diagnostics.iter().find(|d| d.span.start < 28).unwrap();
    assert!(src[inline_err.span.start..].starts_with("$ more") || inline_err.span.start >= 18);
    for d in &doc.diagnostics {
        assert!(d.span.end <= src.len(), "{d}");
    }
}

#[test]
fn block_at_finds_innermost() {
    let src = "# Title\n\n**Proof.** Text\n\n$$ a = 1 $$\n\nBye ∎\n\nAfter.\n";
    let doc = parse_document(src);
    let at = |needle: &str| {
        let off = src.find(needle).unwrap();
        block_at(&doc, off).map(|b| std::mem::discriminant(&b.kind))
    };
    let disc = |k: BlockKind| std::mem::discriminant(&k);
    assert_eq!(
        at("Title"),
        Some(disc(BlockKind::Heading {
            level: 1,
            content: vec![]
        }))
    );
    assert_eq!(
        at("a = 1"),
        Some(disc(BlockKind::DisplayMath(
            match &parse_document("$$ a $$").blocks[0].kind {
                BlockKind::DisplayMath(m) => m.clone(),
                _ => unreachable!(),
            }
        )))
    );
    assert!(matches!(
        block_at(&doc, src.find("Text").unwrap()).map(|b| &b.kind),
        Some(BlockKind::Paragraph(_))
    ));
    assert!(block_at(&doc, 10_000).is_none());
}

#[test]
fn ids_are_content_hashes_and_survive_edits_elsewhere() {
    let a = parse_document("# T\n\n$$ x := 1 $$\n\nProse.\n\n$$ x := 1 $$\n");
    let b = parse_document(
        "# Changed title\n\nNew paragraph first.\n\n$$ x := 1 $$\n\nProse.\n\n$$ x := 1 $$\n",
    );
    let math = |d: &mpp_syntax::document::Document| -> Vec<(u64, u64)> {
        d.blocks
            .iter()
            .filter(|b| matches!(b.kind, BlockKind::DisplayMath(_)))
            .map(|b| (b.hash, b.id.0))
            .collect()
    };
    let (ma, mb) = (math(&a), math(&b));
    assert_eq!(ma[0].0, ma[1].0, "same text, same hash");
    assert_ne!(ma[0].1, ma[1].1, "same text, different ids");
    assert_eq!(ma, mb, "unchanged blocks keep hash and id");
    let prose = |d: &mpp_syntax::document::Document| {
        d.blocks
            .iter()
            .find(|b| matches!(&b.kind, BlockKind::Paragraph(_)) && b.span.len() == 6)
            .map(|b| b.id)
    };
    assert_eq!(prose(&a), prose(&b));
}

#[test]
fn noeval_changes_the_hash() {
    let plain = parse_document("$$ x $$\n");
    let marked = parse_document("%noeval\n$$ x $$\n");
    assert_ne!(plain.blocks[0].hash, marked.blocks[1].hash);
}

// ---- error recovery ----------------------------------------------------------

#[test]
fn unclosed_display_math_closes_at_blank_line() {
    let doc = parse_document("$$ a = 1\n\nNext paragraph.\n");
    insta::assert_snapshot!(dump(&doc));
    assert!(doc.errors().any(|d| d.message.contains("never closed")));
}

#[test]
fn unclosed_things() {
    insta::assert_snapshot!(outline(concat!(
        "Dollar $ never closed, [[wiki never closed, `code never closed, **strong never.\n\n",
        "```\nfence never closed\n\n",
        "$$ last",
    )));
}

#[test]
fn bad_equation_references() {
    let doc =
        parse_document("See \\eqref{nope}.\n\n$$ a = 1 \\label{x} $$\n\n$$ b = 2 \\label{x} $$\n");
    insta::assert_snapshot!(dump(&doc));
}

#[test]
fn bad_maths_does_not_stop_the_document() {
    let doc = parse_document("Text $\\frac{1}$ more.\n\n$$ ( $$\n\n# Still here\n");
    insta::assert_snapshot!(dump(&doc));
    assert!(doc
        .blocks
        .iter()
        .any(|b| matches!(&b.kind, BlockKind::Heading { .. })));
}

#[test]
fn junk_after_display_math_and_figures() {
    insta::assert_snapshot!(outline(concat!(
        "$$ a $$ trailing words\n\n",
        "Figure: banana\n\n",
        "Figure: $y$ and more\n\n",
        "![[]]\n\n",
        "%noeval\nNot maths.\n",
    )));
}

#[test]
fn line_endings_and_unicode() {
    let crlf = outline("# T\r\n\r\nLine one\r\nline two $x$\r\n\r\n$$ a $$\r\n");
    let lf = outline("# T\n\nLine one\nline two $x$\n\n$$ a $$\n");
    insta::assert_snapshot!(lf);
    // Only the spans differ (each `\r` shifts the offsets).
    let strip = |s: &str| {
        s.lines()
            .map(|l| {
                let l = l.split_once("] ").map_or(l, |(_, r)| r);
                l.split(" raw=").next().unwrap_or(l).to_string()
            })
            .collect::<Vec<_>>()
    };
    assert_eq!(strip(&crlf), strip(&lf));
    clean("Ünïcödé ∑ α «quoted» 日本語 $\\alpha$ ∎\n\n| α | β |\n|---|---|\n| ∫ | ∂ |\n");
}

// ---- every .mpp file in the repository -----------------------------------------

#[test]
fn repository_files_parse_without_panics() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    for rel in [
        "README.mpp",
        "docs/grammar.mpp",
        "work/PLAN.mpp",
        "work/RUN-STATE.mpp",
    ] {
        let src = std::fs::read_to_string(root.join(rel)).unwrap();
        let doc = parse_document(&src);
        assert!(!doc.blocks.is_empty(), "{rel}");
        let mut count = 0;
        doc.walk(&mut |b| {
            count += 1;
            assert!(b.span.end <= src.len(), "{rel}: {:?}", b.span);
        });
        assert!(count > 0);
        // Every block must start at or after the one before it.
        assert!(doc
            .blocks
            .windows(2)
            .all(|w| w[0].span.start <= w[1].span.start));
        for d in &doc.diagnostics {
            assert!(d.span.end <= src.len(), "{rel}: {d}");
        }
    }
}
