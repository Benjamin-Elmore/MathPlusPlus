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

/// The glyph sizes of every label drawn in the slots of `src`.
fn label_glyph_sizes(src: &str, opts: &LayoutOptions) -> Vec<f32> {
    let dl = layout_latex(src, opts);
    let slots: Vec<mpp_render::Rect> = dl
        .hits
        .iter()
        .filter(|h| h.kind == HitKind::Slot)
        .map(|h| h.rect)
        .collect();
    dl.items
        .iter()
        .filter_map(|p| match p {
            mpp_render::Primitive::Glyph { x, y, size, .. }
                if slots.iter().any(|r| r.contains(*x + 0.5, *y - 0.5)) =>
            {
                Some(*size)
            }
            _ => None,
        })
        .collect()
}

#[test]
fn labels_in_script_positions_stay_legible() {
    // The bounds of a definite integral are script-size slots.
    let src = r"\int_{}^{} {} \, dx";
    let opts = LayoutOptions::display(20.0);
    let sizes = label_glyph_sizes(src, &opts);
    assert!(!sizes.is_empty());
    for s in &sizes {
        assert!(*s >= 10.0, "a label glyph is {s} px");
    }
    // At a small zoom the floor still holds in pixels.
    for s in label_glyph_sizes(src, &LayoutOptions::display(12.0)) {
        assert!(s >= 10.0 - 1e-3, "a label glyph is {s} px at 12 px");
    }
    // Every label fits inside its box, with padding.
    let dl = layout_latex(src, &opts);
    for h in dl.hits.iter().filter(|h| h.kind == HitKind::Slot) {
        assert!(h.rect.w > 0.0 && h.rect.h >= 10.0, "{:?}", h.rect);
    }
}

#[test]
fn filled_maths_is_unchanged_by_the_label_floor() {
    // A filled integral is laid out exactly as before: no slot is involved.
    let a = layout_latex(r"\int_0^1 x^2 \, dx", &LayoutOptions::display(20.0));
    let b = layout_latex(r"\int_{0}^{1} {x^2} \, dx", &LayoutOptions::display(20.0));
    assert!((a.width - b.width).abs() < 1e-3);
    assert!((a.height - b.height).abs() < 1e-3);
}

fn integral_glyph(src: &str) -> Option<(u16, f32)> {
    let dl = layout_latex(src, &LayoutOptions::display(20.0));
    dl.items.iter().find_map(|p| match p {
        mpp_render::Primitive::Glyph {
            ch: '∫',
            glyph,
            size,
            ..
        } => Some((*glyph, *size)),
        _ => None,
    })
}

#[test]
fn a_template_integral_uses_the_display_size_sign() {
    let template = integral_glyph(r"\int_{}^{} {} \, dx").expect("template ∫");
    let filled = integral_glyph(r"\int_0^1 x^2 \, dx").expect("filled ∫");
    assert_eq!(template, filled);
    let inline = layout_latex(r"\int_0^1 x \, dx", &LayoutOptions::inline(20.0));
    let text_glyph = inline.items.iter().find_map(|p| match p {
        mpp_render::Primitive::Glyph {
            ch: '∫', glyph, ..
        } => Some(*glyph),
        _ => None,
    });
    assert_ne!(
        text_glyph,
        Some(template.0),
        "display uses the large variant"
    );
}
