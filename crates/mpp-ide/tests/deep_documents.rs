//! Maths nested 256 levels deep or 10 000 operators long runs end to end (parse,
//! evaluate, lay out the page) on a thread with the stack the apps' main threads
//! have, with the right answers and no stack overflow; deeper input is an error.

use mpp_eval::Session;
use mpp_ide::page::{build_page, PageInput};
use mpp_render::{IntegralLimits, Theme};
use mpp_syntax::document::parse_document;

/// The apps' main-thread stack (set by their build scripts).
const MAIN_STACK: usize = 256 << 20;

/// The answers and errors of `src`, and the number of glyphs on its page.
fn run(src: String) -> (Vec<String>, Vec<String>, usize) {
    std::thread::Builder::new()
        .stack_size(MAIN_STACK)
        .spawn(move || {
            let doc = parse_document(&src);
            let ev = Session::new().run_str(&src).clone();
            let page = build_page(&PageInput {
                show: &mpp_ide::page::ShowState::default(),
                src: &src,
                doc: &doc,
                eval: Some(&ev),
                theme: Theme::LIGHT,
                size: 18.0,
                width: 900.0,
                integral_limits: IntegralLimits::AboveBelow,
                slot_labels: &[],
                extra_refs: &mpp_ide::page::ExtraRefs::default(),
                proof: &mpp_ide::proof::ProofView::default(),
            });
            let glyphs = page
                .blocks
                .iter()
                .map(|b| b.dl.text().chars().count())
                .sum();
            let answers = ev
                .queries()
                .filter_map(|(_, l)| l.plain.clone().or_else(|| l.text.clone()))
                .collect();
            let errors = ev.errors().iter().map(|d| d.message.clone()).collect();
            (answers, errors, glyphs)
        })
        .expect("spawn")
        .join()
        .expect("no stack overflow")
}

#[test]
fn deep_brackets_run_end_to_end() {
    let src = format!("$$ {}1{} + 1 = ? $$\n", "(".repeat(256), ")".repeat(256));
    let (answers, errors, glyphs) = run(src);
    assert!(errors.is_empty(), "{errors:?}");
    assert_eq!(answers, ["2"]);
    assert!(glyphs > 0);
}

#[test]
fn ten_thousand_terms_run_end_to_end() {
    let src = format!("$$ {}1 = ? $$\n", "1 + ".repeat(9_999));
    let (answers, errors, _) = run(src);
    assert!(errors.is_empty(), "{errors:?}");
    assert_eq!(answers, ["10000"]);
}

#[test]
fn past_the_limits_is_an_error() {
    let (_, errors, _) = run(format!(
        "$$ {}1{} = ? $$\n",
        "(".repeat(300),
        ")".repeat(300)
    ));
    assert!(
        errors.iter().any(|e| e.contains("nested too deeply")),
        "{errors:?}"
    );
    let (_, errors, _) = run(format!("$$ {}1 = ? $$\n", "1 + ".repeat(12_000)));
    assert!(errors.iter().any(|e| e.contains("too long")), "{errors:?}");
}
