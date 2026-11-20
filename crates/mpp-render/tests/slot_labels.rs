//! Labels for empty slots given by offset override the layout's own hints.

use mpp_render::{layout_latex, HitKind, LayoutOptions};

fn labels(src: &str, opts: &LayoutOptions) -> Vec<(usize, String)> {
    let dl = layout_latex(src, opts);
    let mut v: Vec<(usize, String)> = dl
        .hits
        .iter()
        .filter(|h| h.kind == HitKind::Slot)
        .map(|h| (h.span.start, h.label.clone().unwrap_or_default()))
        .collect();
    v.sort();
    v
}

#[test]
fn named_slots_replace_the_generic_hints() {
    let src = r"\operatorname{area}\left( {}, {} \right)";
    let plain = labels(src, &LayoutOptions::display(20.0));
    assert_eq!(plain.len(), 2);
    let a = plain[0].0;
    let b = plain[1].0;
    let opts = LayoutOptions {
        slot_labels: vec![(a, "top curve".into()), (b, "bottom curve".into())],
        ..LayoutOptions::display(20.0)
    };
    assert_eq!(
        labels(src, &opts),
        vec![
            (a, "top curve".to_string()),
            (b, "bottom curve".to_string())
        ]
    );
    // The label is drawn: its glyphs are on the page.
    let dl = layout_latex(src, &opts);
    assert!(dl.text().contains("topcurve"), "{}", dl.text());
}

#[test]
fn an_empty_name_hides_the_hint() {
    let src = r"\frac{}{}";
    let plain = labels(src, &LayoutOptions::display(20.0));
    assert_eq!(plain[0].1, "numerator");
    let opts = LayoutOptions {
        slot_labels: vec![(plain[0].0, String::new())],
        ..LayoutOptions::display(20.0)
    };
    assert_eq!(labels(src, &opts)[0].1, "");
}
