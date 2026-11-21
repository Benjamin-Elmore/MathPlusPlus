//! Mapping precision and classification of problems.

use super::*;
use crate::doc::Doc;
use crate::theme::Mode;

/// The problems of a text after its evaluation, laid out at a fixed width.
fn problems_of(src: &str) -> (Vec<Problem>, std::sync::Arc<crate::doc::PageCache>) {
    let mut d = Doc::with_text(src, None);
    d.settle();
    let pc = d.page(900.0, 19.0, Mode::Light);
    (pc.problems.clone(), pc)
}

#[test]
fn the_guide_example_counts() {
    let src = "# Problems on purpose\n\nLet $\\vartheta := 0.3$ and $m := 2$.\n\n$$ \\frac{1}{} = ? $$\n\n$$ \\sin \\theta \\approx ? $$\n\n$$ 2 (x + 1 $$\n\n$$ \\Gamma(x + 1) \\overset{?}{=} x \\Gamma(x) $$\n\n$$ \\int_0^1 e^{-x^2} \\, dx \\approx ? $$\n\n$$ \\operatorname{erf}(x) + \\operatorname{erfc}(x) \\overset{?}{=} 1 $$\n";
    let (ps, _) = problems_of(src);
    let c = Counts::of(&ps);
    // The N9 guide expected 3 errors, 3 warnings and 1 note; the evaluator now works out
    // the line that was the note, so both identities are numeric checks and none is left.
    assert_eq!((c.errors, c.warnings, c.infos), (3, 4, 0), "{ps:#?}");
    assert_eq!(c.summary(), "3 errors, 4 warnings");
}

/// The page box of the node with exactly this source text.
fn node_rect(pc: &crate::doc::PageCache, src: &str, text: &str, nth: usize) -> Rect {
    let start = src
        .match_indices(text)
        .nth(nth)
        .map(|(i, _)| i)
        .expect("text in source");
    map::node_exact(&pc.page, Span::new(start, start + text.len())).expect("node box")
}

fn close(a: Rect, b: Rect) -> bool {
    (a.min - b.min).length() < 0.5 && (a.max - b.max).length() < 0.5
}

#[test]
fn an_undefined_name_in_a_subscript_underlines_only_the_subscript() {
    let src = "$$ a^{\\beta} + 1 \\approx ? $$\n";
    let (ps, pc) = problems_of(src);
    let p = ps
        .iter()
        .find(|p| matches!(&p.kind, Kind::Undefined { name, .. } if name == "β"))
        .expect("β is undefined");
    let beta = node_rect(&pc, src, "\\beta", 0);
    assert_eq!(p.rects.len(), 1);
    assert!(close(p.rects[0], beta), "{:?} vs {:?}", p.rects[0], beta);
    let a = node_rect(&pc, src, "a", 0);
    assert!(
        beta.width() < a.width() * 1.5,
        "the superscript box is small"
    );
}

#[test]
fn each_undefined_name_gets_its_own_underline() {
    let src = "$$ x_{\\theta} + \\theta \\approx ? $$\n";
    let (ps, pc) = problems_of(src);
    let names: Vec<&str> = ps
        .iter()
        .filter_map(|p| match &p.kind {
            Kind::Undefined { name, .. } => Some(name.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(names, ["x_θ", "θ"]);
    let theta = ps
        .iter()
        .find(|p| matches!(&p.kind, Kind::Undefined { name, .. } if name == "θ"))
        .expect("θ");
    // The lone θ, not the θ inside the subscript of x_θ.
    let lone = node_rect(&pc, src, "\\theta", 1);
    assert!(close(theta.rect, lone), "{:?} vs {:?}", theta.rect, lone);
}

#[test]
fn an_empty_box_underlines_the_box() {
    let src = "$$ \\frac{1}{} = ? $$\n";
    let (ps, pc) = problems_of(src);
    let p = ps
        .iter()
        .find(|p| matches!(p.kind, Kind::EmptyBox { .. }))
        .expect("empty box");
    let (slot, label) = map::slot_at(&pc.page, p.span).expect("slot");
    assert!(close(p.rect, slot));
    assert_eq!(label.as_deref(), Some("denominator"));
    assert!(p.message.contains("denominator"), "{}", p.message);
}

#[test]
fn an_empty_box_inside_a_template_box_is_found() {
    // An empty root inside the integrand of an integral.
    let src = "$$ \\int_{0}^{1} {\\sqrt{}} \\, dx $$\n";
    let (ps, _) = problems_of(src);
    let p = ps
        .iter()
        .find(|p| matches!(p.kind, Kind::EmptyBox { .. }))
        .expect("empty box");
    assert!(p.message.contains("radicand"), "{}", p.message);
}

#[test]
fn a_missing_bracket_underlines_the_opening_bracket() {
    let src = "$$ (a + b $$\n";
    let (ps, pc) = problems_of(src);
    let p = ps
        .iter()
        .find(|p| matches!(p.kind, Kind::MissingBracket { .. }))
        .expect("missing bracket");
    let group = node_rect(&pc, src, "(a + b", 0);
    let inner = node_rect(&pc, src, "a + b", 0);
    assert_eq!(p.rects.len(), 1);
    let r = p.rects[0];
    assert!((r.left() - group.left()).abs() < 0.5);
    assert!(
        r.right() <= inner.left() + 0.5,
        "only the bracket: {r:?} {inner:?}"
    );
    match &p.kind {
        Kind::MissingBracket { close, insert, .. } => {
            assert_eq!(close, ")");
            assert_eq!(&src[..*insert], "$$ (a + b");
        }
        _ => unreachable!(),
    }
}

#[test]
fn the_inner_of_two_open_brackets_is_the_one_reported() {
    let src = "$$ [a + (b $$\n";
    let (ps, pc) = problems_of(src);
    let paren = ps
        .iter()
        .find(|p| matches!(&p.kind, Kind::MissingBracket { close, .. } if close == ")"))
        .expect(")");
    let g = node_rect(&pc, src, "(b", 0);
    assert!((paren.rect.left() - g.left()).abs() < 0.5);
}

#[test]
fn no_message_shows_source_text_or_line_numbers() {
    let src = "# T\n\n$$ \\frac{a}{b $$\n\n$$ \\sqrt{} = ? $$\n\n$$ (x $$\n\n$$ q \\approx ? $$\n";
    let (ps, _) = problems_of(src);
    assert!(ps.len() >= 3, "{ps:?}");
    for p in &ps {
        for bad in ["\\", "$$", "`", "line ", "column", "{", "}"] {
            assert!(!p.message.contains(bad), "{bad:?} in {:?}", p.message);
        }
    }
}

#[test]
fn a_numeric_check_is_a_warning() {
    let src = "$$ \\Gamma(x + 1) \\overset{?}{=} x \\Gamma(x) $$\n";
    let (ps, _) = problems_of(src);
    if let Some(p) = ps
        .iter()
        .find(|p| matches!(p.kind, Kind::NumericCheck { .. }))
    {
        assert_eq!(p.level, Level::Warning);
    }
}

#[test]
fn an_unused_definition_is_a_warning_on_its_name() {
    let src = "$$ a := 2 $$\n\n$$ b := 3 $$\n\n$$ a + 1 = ? $$\n";
    let (ps, pc) = problems_of(src);
    let p = ps
        .iter()
        .find(|p| matches!(&p.kind, Kind::Unused { name } if name == "b"))
        .expect("b is unused");
    assert_eq!(p.level, Level::Warning);
    let b = node_rect(&pc, src, "b", 0);
    assert!(close(p.rect, b), "{:?} vs {:?}", p.rect, b);
    assert!(!ps
        .iter()
        .any(|p| matches!(&p.kind, Kind::Unused { name } if name == "a")));
}

#[test]
fn a_library_file_has_no_unused_warnings() {
    let (ps, _) = problems_of("$$ g := 9.81 $$\n\n$$ c := 3 $$\n");
    assert!(ps.is_empty(), "{ps:?}");
}

#[test]
fn the_template_being_filled_in_is_quiet() {
    let src = "$$ \\frac{1}{} = ? $$\n";
    let doc = mpp_syntax::document::parse_document(src);
    let mk = |quiet| Inputs {
        src,
        doc: &doc,
        eval: None,
        eval_fresh: false,
        background: None,
        quiet,
        proof: &[],
    };
    assert!(!gather(&mk(None)).is_empty());
    assert!(gather(&mk(Some(3..13))).is_empty());
}

#[test]
fn problems_of_unchanged_blocks_survive_an_edit_elsewhere() {
    let src = "$$ q \\approx ? $$\n\n$$ 1 + 1 = ? $$\n";
    let mut d = Doc::with_text(src, None);
    d.settle();
    let before = d.page(900.0, 19.0, Mode::Light).problems.clone();
    assert!(before
        .iter()
        .any(|p| matches!(p.kind, Kind::Undefined { .. })));
    // Typing in the second block (no evaluation yet): the first keeps its mark.
    let at = d.editor.text().find("1 + 1").expect("second block");
    d.editor.move_caret(at, false);
    d.editor.insert_text("2 + ");
    let after = d.page(900.0, 19.0, Mode::Light).problems.clone();
    assert!(after
        .iter()
        .any(|p| matches!(p.kind, Kind::Undefined { .. })));
}

#[test]
fn counts_summarise() {
    let c = Counts {
        errors: 1,
        warnings: 2,
        infos: 3,
    };
    assert_eq!(c.summary(), "1 error, 2 warnings");
    assert_eq!(c.worst(), Some(Level::Error));
    assert_eq!(Counts::default().summary(), "No problems");
}

/// Every message the IDE shows for the example files and the guides (problem text,
/// "Expected", "How to fix" and each quick fix) is words and typeset maths: no source
/// characters appear in the words.
#[test]
fn no_message_from_any_example_shows_source_text() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    fn walk(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
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
    let mut files: Vec<std::path::PathBuf> = vec![];
    for dir in ["examples", "docs", "work/pipeline-tests"] {
        walk(&root.join(dir), &mut files);
    }
    files.sort();
    assert!(files.len() > 20, "found {files:?}");
    let mut bad = vec![];
    let mut seen = 0;
    for path in &files {
        let mut d = Doc::open(path).expect("readable");
        d.settle();
        let pc = d.page(900.0, 19.0, Mode::Light);
        let src = d.editor.text().to_string();
        let doc = mpp_syntax::document::parse_document(&src);
        for p in &pc.problems {
            let mut lines: Vec<&Rich> = vec![&p.rich];
            lines.extend(p.expected.iter());
            lines.extend(p.fix.iter());
            let fixes = fixes::quick_fixes(p, &src, &doc);
            lines.extend(fixes.iter().map(|f| &f.title));
            for r in lines {
                seen += 1;
                for seg in &r.0 {
                    if let Seg::Text(t) | Seg::Italic(t) = seg {
                        if t.contains(['$', '\\', '^', '_', '{', '}']) {
                            bad.push(format!("{}: {t:?} in {:?}", path.display(), r.plain()));
                        }
                    }
                }
            }
        }
    }
    assert!(seen > 10, "only {seen} messages");
    assert!(bad.is_empty(), "{}", bad.join("\n"));
}

#[test]
fn inline_maths_in_messages_is_typeset() {
    let r = rich::from_message(
        r"the answer depends on whether the exponent is −1; add an assumption such as Assume $n \neq -1$. to pick a case",
    );
    assert!(r.0.contains(&Seg::Math(r"n \neq -1".into())), "{r:?}");
    let r = rich::from_message(r"ln(e^{y}) = y only for real y; `Assume $y \in \mathbb{R}$.`");
    assert!(
        r.0.iter()
            .any(|s| matches!(s, Seg::Math(m) if m.contains("mathbb"))),
        "{r:?}"
    );
}

#[test]
fn text_maths_words_in_notes_are_typeset() {
    let r = rich::from_message(
        "the series is exact up to x^9; Σ 1/k^2 converges (|aₖ|^(1/k) tends to 1)",
    );
    let maths: Vec<&str> =
        r.0.iter()
            .filter_map(|s| match s {
                Seg::Math(m) => Some(m.as_str()),
                _ => None,
            })
            .collect();
    assert_eq!(maths, ["x^{9}", "1/k^{2}", "|aₖ|^{1/k}"], "{r:?}");
    assert!(r.plain().contains("(|aₖ|"), "{}", r.plain());
}

/// Letters side by side that spell no defined name get a warning with both quick
/// fixes; once the name is defined (by those letters on the left of `:=`) they don't.
#[test]
fn a_spelled_word_that_is_not_a_name_is_pointed_out() {
    let src = "$$ speed + 1 = ? $$\n";
    let (ps, _) = problems_of(src);
    let p = ps
        .iter()
        .find(|p| matches!(&p.kind, Kind::LetterRun { word } if word == "speed"))
        .expect("a warning on speed");
    assert_eq!(p.level, Level::Warning);
    let doc = mpp_syntax::document::parse_document(src);
    let fixes = fixes::quick_fixes(p, src, &doc);
    let edits: Vec<String> = fixes
        .iter()
        .filter_map(|f| match &f.action {
            fixes::FixAction::Replace { with, .. } => Some(with.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(edits, [r"\mathrm{speed}", "s p e e d"]);
    let (ps, _) = problems_of("$$ speed := 3 $$\n\n$$ speed \\cdot 2 = ? $$\n");
    assert!(
        !ps.iter().any(|p| matches!(p.kind, Kind::LetterRun { .. })),
        "{ps:#?}"
    );
}
