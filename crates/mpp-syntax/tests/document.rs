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

#[test]
fn a_display_after_a_list_number_or_prose_is_its_own_block() {
    let src = "4. $$ x = ? $$\n\nPartial fractions: $$ y = ? $$\n\nIt costs $$5 = ? here.\n";
    let doc = parse_document(src);
    let kinds: Vec<&str> = doc
        .blocks
        .iter()
        .map(|b| match b.kind {
            BlockKind::List(_) => "list",
            BlockKind::DisplayMath(_) => "display",
            BlockKind::Paragraph(_) => "paragraph",
            _ => "other",
        })
        .collect();
    assert_eq!(
        kinds,
        ["list", "display", "paragraph", "display", "paragraph"],
        "{}",
        dump(&doc)
    );
}

#[test]
fn display_marks_inside_code_spans_stay_prose() {
    let src = "The maths sits between `$ x $` and `$$ x $$` in a file.

See ``$$ a $$`` too.
";
    let doc = parse_document(src);
    assert!(doc.diagnostics.is_empty(), "{:?}", doc.diagnostics);
    assert!(
        doc.blocks
            .iter()
            .all(|b| matches!(b.kind, BlockKind::Paragraph(_))),
        "{}",
        dump(&doc)
    );
}

#[test]
fn a_letter_after_a_number_is_a_variable_where_it_is_one() {
    // g is defined before, t is the function's parameter, the figure's range or a
    // letter the same line uses: each is the variable, set in italic.
    let src = concat!(
        r"$$ g \equiv 9.81\,\mathrm{m/s^2} $$",
        "\n\n",
        r"$$ H := \frac{v^2}{2 g} $$",
        "\n\n",
        r"$$ u(t) := e^{-20 t} \cos(20 t) $$",
        "\n\n",
        r"$$ \int_0^1 2 t \, dt = ? $$",
        "\n\n",
        r"Figure: $e^{-3 t}$, $t \in [0, 1]$.",
        "\n\n",
        r"Let $m = 2\,\mathrm{kg}$.",
        "\n\n",
        "$$ F := 3 m $$\n",
    );
    let out = outline(src);
    assert!(
        !out.contains("quantity g") && !out.contains("quantity t") && !out.contains("quantity m"),
        "{out}"
    );
    // A letter nothing binds stays a unit, and a thin space always makes one.
    let out = outline(concat!(
        "$$ 5 m + 2 m = ? $$\n\n",
        r"$$ u(t) := 20\,t $$",
        "\n"
    ));
    assert_eq!(out.matches("quantity m").count(), 2, "{out}");
    assert!(out.contains("quantity t"), "{out}");
}

/// A word defined with `velocity := 3` is one name wherever it is written, so it is
/// set the same way (upright) at its definition and at every use.
#[test]
fn a_defined_word_is_one_name_where_it_is_used() {
    use mpp_syntax::math::{NodeKind, Statement};
    let src = "$$ velocity := 3 $$\n\n$$ 2 velocity^2 + xy = ? $$\n";
    let doc = parse_document(src);
    let BlockKind::DisplayMath(m) = &doc.blocks[1].kind else {
        panic!("{}", dump(&doc));
    };
    let Statement::Query { expr, .. } = &m.ast.lines[0].statement else {
        panic!("{}", dump(&doc));
    };
    let mut names = vec![];
    fn walk(n: &mpp_syntax::math::Node, out: &mut Vec<String>) {
        if let NodeKind::Name(name) = &n.kind {
            out.push(name.base.clone());
        }
        mpp_syntax::math::visit::children(n, &mut |c| walk(c, out));
    }
    walk(expr, &mut names);
    assert!(names.contains(&"velocity".to_string()), "{names:?}");
    // Letters that spell no defined name still multiply.
    assert!(names.contains(&"x".to_string()) && names.contains(&"y".to_string()));
    assert!(!names.contains(&"v".to_string()), "{names:?}");
}

/// The inline maths spans of a one-paragraph document, as source text.
fn inline_maths(src: &str) -> Vec<String> {
    use mpp_syntax::document::InlineKind;
    let doc = parse_document(src);
    let mut out = Vec::new();
    for b in &doc.blocks {
        if let BlockKind::Paragraph(items) = &b.kind {
            for i in items {
                if let InlineKind::Math(m) = &i.kind {
                    out.push(src[m.raw.start..m.raw.end].to_string());
                }
            }
        }
    }
    out
}

#[test]
fn money_never_starts_maths() {
    // A dollar sign straight before a digit, or followed by a space, is text.
    for src in [
        "It costs $5 today.\n",
        "Between $5 and $10 each.\n",
        "From $5-$10.\n",
        "A $ sign on its own.\n",
        "Ends with a dollar $\n",
    ] {
        let doc = parse_document(src);
        assert!(inline_maths(src).is_empty(), "{src:?} has maths");
        assert!(
            !doc.errors().any(|d| d.message.contains("never closed")),
            "{src:?} reports an unclosed dollar"
        );
    }
    // Maths that starts with a digit still works when its `$` closes it tightly.
    assert_eq!(inline_maths("About $0$ to order 4.\n"), vec!["0"]);
    assert_eq!(inline_maths("Here $1 + 1 = ?$ ok.\n"), vec!["1 + 1 = ?"]);
    assert_eq!(inline_maths("So $x$ costs $5.\n"), vec!["x"]);
    // A `$` before a letter still opens maths (and says when it never closes).
    let doc = parse_document("Take $x + 1 and stop.\n");
    assert!(doc.errors().any(|d| d.message.contains("never closed")));
}

/// The display blocks of `src`, with the source of each one's maths.
fn displays(src: &str) -> Vec<String> {
    let doc = parse_document(src);
    doc.blocks
        .iter()
        .filter_map(|b| match &b.kind {
            BlockKind::DisplayMath(d) => Some(src[d.raw.start..d.raw.end].trim().to_string()),
            _ => None,
        })
        .collect()
}

#[test]
fn bracket_display_maths_is_read_like_dollars() {
    // Pasted LaTeX: `\[ … \]` on one line, or over several, is display maths.
    assert_eq!(displays("\\[ x = 1 + 2 \\]\n"), vec!["x = 1 + 2"]);
    assert_eq!(
        displays("Some words.\n\n\\[\nE = m c^2\n\\]\n\nMore words.\n"),
        vec!["E = m c^2"]
    );
    assert_eq!(displays("Text.\n\\[ y = 3 \\]\n"), vec!["y = 3"]);
    let doc = parse_document("\\[ x = 1 + 2 \\]\n");
    assert!(doc.diagnostics.is_empty(), "{}", dump(&doc));
    // An unclosed `\[` (still being typed) is text.
    assert!(displays("\\[ x = 1\n").is_empty());
    assert!(displays("\\[ x = 1\n\nNext \\] here\n").is_empty());
}
