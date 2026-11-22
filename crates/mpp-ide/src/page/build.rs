//! Laying a parsed document out as a typeset page.

use egui::{Pos2, Rect, Vec2};
use mpp_eval::{BlockResult, Evaluated, LineKind, Value, Verdict};
use mpp_render::{
    layout_latex, CaretStop, DisplayList, FontId, HitKind, IntegralLimits, LayoutOptions,
    LineResult, MathStyle, Primitive, Rect as RRect, ResultKind, Theme,
};
use mpp_syntax::document::{
    Align, Block, BlockKind, Comment, DirectiveKind, DisplayMath, Document, Inline, InlineKind,
    InlineMath, List, References, TheoremKind, TitleField, TitleKind,
};
use mpp_syntax::math::{parse_math, QueryMode, RelOp};
use mpp_syntax::Span;

use crate::diagnostics::rich::{from_message, Rich, Seg};

use super::flow::{display_name, flow, flow_center, flow_text, glyph_for, text_width, Cx, Style};
use super::paper::{entry_text, italic_body, ExtraRefs, PaperInfo};
use super::{
    BlockTag, Canvas, Link, LinkTarget, Page, PageLink, PlacedBlock, ShowState, StepToggle,
};

/// Everything the page builder reads.
pub struct PageInput<'a> {
    pub src: &'a str,
    pub doc: &'a Document,
    /// Evaluation results (only blocks whose id matches get results).
    pub eval: Option<&'a Evaluated>,
    pub theme: Theme,
    /// Body text size in pixels.
    pub size: f32,
    /// Available width in pixels (the column is at most 70 em).
    pub width: f32,
    /// Integral bounds above and below the sign, or at its side.
    pub integral_limits: IntegralLimits,
    /// Labels for the empty boxes of templates being edited, by offset.
    pub slot_labels: &'a [(usize, String)],
    /// The view's own display choices (formats, unfolded steps, figure sliders,
    /// notes shown and folded).
    pub show: &'a ShowState,
    /// Entries of the document's `References from [[file]]` files.
    pub extra_refs: &'a ExtraRefs,
    /// The proof checker's summaries (drawn under theorems; never in the file).
    pub proof: &'a crate::proof::ProofView,
}

/// The column width for a window width: about 70 em, never wider than the window.
/// Each side keeps a margin of at least 1.75 em: the ∑ marker of a maths box and
/// the problem bars are drawn there.
pub fn column_width(avail: f32, size: f32) -> f32 {
    (avail - 2.0 * MARGIN * size).clamp(size * 10.0, size * 70.0)
}

/// The smallest margin beside the column, in ems.
pub const MARGIN: f32 = 1.75;

/// Lays the document out.
pub fn build_page(input: &PageInput<'_>) -> Page {
    super::mathcache::begin();
    let col = column_width(input.width, input.size);
    let paper = PaperInfo::scan(input.doc, input.src, input.extra_refs);
    let cx = Cx {
        src: input.src,
        theme: input.theme,
        slot_labels: input.slot_labels,
        paper: Some(&paper),
    };
    let mut page = Page {
        width: col,
        theme: Some(input.theme),
        ..Page::default()
    };
    let mut y = input.size * 0.5;
    let gap = input.size * 0.85;
    let mut prev_end: Option<usize> = None;
    let mut prev_bottom = y;
    // Where the stops of the block above begin (its last line is what counts).
    let mut prev_stops = 0;
    for b in &input.doc.blocks {
        let before = page.blocks.len();
        let stops_before = page.stops.len();
        let top = y;
        // Empty lines a person sees before this block: each is a line of its own.
        let ws_start = prev_end.unwrap_or(0);
        let blank = blank_line_stops(
            input.src,
            ws_start,
            b.span.start,
            (prev_end.is_some(), true),
            &mut y,
            input.size,
            gap,
        );
        place_block(input, &cx, b, 0.0, col, &mut y, gap, &mut page);
        if page.blocks.len() > before {
            // Spaces typed after the block above, still on its last line.
            let line_end =
                prev_end.and_then(|e| last_stop_upto(&page.stops[prev_stops..stops_before], e));
            same_line_stops(
                input.src,
                ws_start + 1,
                b.span.start,
                line_end,
                input.size,
                &mut page.stops,
            );
            page.stops.extend(blank);
            page.between.push(Span::new(ws_start, b.span.start));
            prev_end = Some(b.span.end);
            prev_stops = stops_before;
            prev_bottom = y - gap;
        } else {
            y = top;
        }
    }
    let mut end_y = prev_bottom + gap;
    match prev_end {
        Some(e) => {
            page.between.push(Span::new(e, input.src.len()));
            let line_end = last_stop_upto(&page.stops, e);
            same_line_stops(
                input.src,
                e + 1,
                input.src.len() + 1,
                line_end,
                input.size,
                &mut page.stops,
            );
            let blank = blank_line_stops(
                input.src,
                e,
                input.src.len(),
                (true, false),
                &mut end_y,
                input.size,
                gap,
            );
            page.stops.extend(blank);
        }
        None => {
            end_y = y;
            let blank = blank_line_stops(
                input.src,
                0,
                input.src.len(),
                (false, false),
                &mut end_y,
                input.size,
                gap,
            );
            page.stops.extend(blank);
        }
    }
    page.height = (end_y - gap + gap * 3.0).max(input.size * 4.0);
    page.chars.sort_by_key(|c| c.start);
    page.stops.sort_by(|a, b| {
        a.offset
            .cmp(&b.offset)
            .then(a.x.partial_cmp(&b.x).unwrap_or(std::cmp::Ordering::Equal))
    });
    page.stops.dedup_by(|a, b| {
        a.offset == b.offset && (a.x - b.x).abs() < 0.01 && (a.top - b.top).abs() < 1.0
    });
    collect_math_ranges(input.src, &input.doc.blocks, &mut page.math_ranges);
    page
}

/// The stop furthest along a block's text, at or before `end` (where the line
/// the block ends on finishes on screen).
fn last_stop_upto(stops: &[CaretStop], end: usize) -> Option<CaretStop> {
    stops
        .iter()
        .filter(|s| s.offset <= end)
        .max_by(|a, b| {
            a.offset
                .cmp(&b.offset)
                .then(
                    a.top
                        .partial_cmp(&b.top)
                        .unwrap_or(std::cmp::Ordering::Equal),
                )
                .then(a.x.partial_cmp(&b.x).unwrap_or(std::cmp::Ordering::Equal))
        })
        .copied()
}

/// Stops for spaces typed after a block's last word, still on its last line:
/// they stay on that line, after the last character, so the caret never drops
/// to the next line while someone types a space. Offsets after a line break
/// get no stop here (see [`blank_line_stops`]).
fn same_line_stops(
    src: &str,
    start: usize,
    end: usize,
    line_end: Option<CaretStop>,
    size: f32,
    out: &mut Vec<CaretStop>,
) {
    let Some(last) = line_end else {
        return;
    };
    let end = end.min(src.len() + 1);
    for o in start.max(last.offset)..end {
        if o > src.len() || !src.is_char_boundary(o) {
            continue;
        }
        let between = &src[last.offset..o];
        if between.contains('\n') {
            break;
        }
        out.push(CaretStop {
            x: last.x + between.chars().count() as f32 * size * 0.28,
            offset: o,
            ..last
        });
    }
}

/// The height of an empty line on screen: a line of body text.
pub(crate) fn blank_line_height(size: f32) -> f32 {
    size * 1.5
}

/// Caret stops for the empty lines a person sees in `src[start..end]` (see
/// [`super::blank`]), stacked from `y` down like empty paragraphs; `y` moves
/// past them. `(prev, next)`: whether there is a block before and after.
fn blank_line_stops(
    src: &str,
    start: usize,
    end: usize,
    (prev, next): (bool, bool),
    y: &mut f32,
    size: f32,
    gap: f32,
) -> Vec<CaretStop> {
    let mut out = Vec::new();
    for line in super::blank::visible_blank_lines(src, start, end, prev, next) {
        // Where the caret of a one-line paragraph sits (see `flow`).
        let (top, bottom) = (*y + size * 0.21, *y + size * 1.31);
        for (n, &o) in line.iter().enumerate() {
            out.push(CaretStop {
                x: n as f32 * size * 0.28,
                top,
                bottom,
                offset: o,
            });
        }
        *y += blank_line_height(size) + gap;
    }
    out
}

#[allow(clippy::too_many_arguments)]
fn place_block(
    input: &PageInput<'_>,
    cx: &Cx<'_>,
    b: &Block,
    x0: f32,
    w: f32,
    y: &mut f32,
    gap: f32,
    page: &mut Page,
) {
    if let BlockKind::Theorem(t) = &b.kind {
        place_theorem(input, cx, b, t, x0, w, y, gap, page);
        return;
    }
    if let BlockKind::Comment(c) = &b.kind {
        if !input.show.hide_comments {
            place_comment(input, cx, b, c, x0, w, y, gap, page);
        }
        return;
    }
    let key = prose_key(input, b, x0, w);
    let cached = key
        .as_deref()
        .and_then(|k| super::mathcache::cached_block(k, b.span.start));
    let laid = match cached {
        Some(found) => Some(found),
        None => {
            let laid = layout_block(input, cx, b, x0, w);
            if let (Some(k), Some((canvas, h, tag))) = (key, &laid) {
                super::mathcache::store_block(k, b.span.start, canvas, *h, *tag);
            }
            laid
        }
    };
    let Some((mut canvas, h, tag)) = laid else {
        return;
    };
    super::answers::inline_chips(input, b, &mut canvas);
    push_block(page, b, *y, h, canvas, tag, input);
    *y += h + gap;
}

/// The layout cache key of a plain prose block (a paragraph or `Let` sentence of text,
/// emphasis, code and inline maths): its text and everything its layout reads, with
/// offsets relative to the block. `None` for blocks whose layout depends on the rest
/// of the document (citations, footnotes, numbering, results).
fn prose_key(input: &PageInput<'_>, b: &Block, x0: f32, w: f32) -> Option<String> {
    fn plain(items: &[Inline]) -> bool {
        items.iter().all(|i| match &i.kind {
            // Links and escapes are laid out from their own text alone.
            InlineKind::Text(_)
            | InlineKind::SoftBreak
            | InlineKind::Code(_)
            | InlineKind::Math(_)
            | InlineKind::WikiLink { .. }
            | InlineKind::Escape(_) => true,
            InlineKind::Emphasis(inner)
            | InlineKind::Strong(inner)
            | InlineKind::Link { text: inner, .. } => plain(inner),
            _ => false,
        })
    }
    let content = match &b.kind {
        BlockKind::Paragraph(content) => content,
        BlockKind::Let(l) => &l.content,
        _ => return None,
    };
    if !plain(content) {
        return None;
    }
    let text = input.src.get(b.span.start..b.span.end)?;
    let labels: Vec<(usize, &String)> = input
        .slot_labels
        .iter()
        .filter(|(o, _)| b.span.start <= *o && *o <= b.span.end)
        .map(|(o, l)| (o - b.span.start, l))
        .collect();
    let tag = if matches!(b.kind, BlockKind::Let(_)) {
        'L'
    } else {
        'P'
    };
    Some(format!(
        "{tag}\u{0}{text}\u{0}{x0}/{w}/{}/{:?}/{labels:?}/{:?}",
        input.size, input.theme, input.integral_limits
    ))
}

/// A theorem-like block: a bold run-in label (`Theorem 2.1.`) before the first
/// paragraph, an italic body for theorems, lemmas, corollaries and propositions,
/// and a ∎ at the end of a proof that does not write one.
#[allow(clippy::too_many_arguments)]
fn place_theorem(
    input: &PageInput<'_>,
    cx: &Cx<'_>,
    b: &Block,
    t: &mpp_syntax::document::Theorem,
    x0: f32,
    w: f32,
    y: &mut f32,
    gap: f32,
    page: &mut Page,
) {
    let mut label = match cx.paper {
        Some(p) => p.theorem_label(t.kind),
        None => format!("{}.", t.kind.as_str()),
    };
    // A number written in the heading (`**Lemma 2.**`) is shown as written.
    if let Some(n) = &t.number {
        label = format!("{} {n}.", t.kind.as_str());
    }
    let title = t.title.clone();
    if title.is_some() {
        label = label.trim_end_matches('.').to_string();
    }
    let body_font = if italic_body(t.kind) {
        FontId::TextItalic
    } else {
        FontId::Text
    };
    let qed = t.kind == TheoremKind::Proof && !t.closed;
    let at = |o: usize| Span::new(o, o);
    let label_inline = |o: usize| Inline {
        kind: InlineKind::Strong(vec![Inline {
            kind: InlineKind::Text(format!("{label} ")),
            span: at(o),
        }]),
        span: at(o),
    };
    // `(Scaling).` after the label, in the body's font.
    let title_inlines = |o: usize| -> Vec<Inline> {
        let Some(title) = &title else {
            return vec![];
        };
        let mut v = vec![Inline {
            kind: InlineKind::Text("(".into()),
            span: at(o),
        }];
        v.extend(title.iter().cloned());
        let end = title.last().map_or(o, |i| i.span.end);
        v.push(Inline {
            kind: InlineKind::Text("). ".into()),
            span: at(end),
        });
        v
    };
    let n = t.body.len();
    for (i, inner) in t.body.iter().enumerate() {
        let last = i + 1 == n;
        if let BlockKind::Paragraph(content) = &inner.kind {
            let mut inl = Vec::with_capacity(content.len() + 2);
            if i == 0 {
                inl.push(label_inline(inner.span.start));
                inl.extend(title_inlines(inner.span.start));
            }
            inl.extend(content.iter().cloned());
            if last && qed {
                inl.push(Inline {
                    kind: InlineKind::Text("\u{2003}\u{220e}".into()),
                    span: at(inner.span.end),
                });
            }
            let mut c = Canvas::default();
            let h = flow(cx, &inl, style(input, body_font, 1.0), x0, 0.0, w, &mut c);
            push_block(page, inner, *y, h, c, BlockTag::Prose, input);
            *y += h + gap;
            continue;
        }
        if i == 0 {
            let mut c = Canvas::default();
            let mut inl = vec![label_inline(b.span.start)];
            inl.extend(title_inlines(b.span.start));
            let h = flow(
                cx,
                &inl,
                style(input, FontId::Text, 1.0),
                x0,
                0.0,
                w,
                &mut c,
            );
            push_block(page, b, *y, h, c, BlockTag::Prose, input);
            *y += h + gap * 0.3;
        }
        place_block(input, cx, inner, x0, w, y, gap, page);
        if last && qed {
            let mut c = Canvas::default();
            let inl = [Inline {
                kind: InlineKind::Text("\u{220e}".into()),
                span: at(b.span.end),
            }];
            let h = flow(
                cx,
                &inl,
                style(input, FontId::Text, 1.0),
                x0 + w - input.size,
                0.0,
                w,
                &mut c,
            );
            push_block(page, b, *y, h, c, BlockTag::Prose, input);
            *y += h + gap;
        }
    }
    if n == 0 {
        let mut c = Canvas::default();
        let mut inl = vec![label_inline(b.span.start)];
        inl.extend(title_inlines(b.span.start));
        let h = flow(
            cx,
            &inl,
            style(input, FontId::Text, 1.0),
            x0,
            0.0,
            w,
            &mut c,
        );
        push_block(page, b, *y, h, c, BlockTag::Prose, input);
        *y += h + gap;
    }
    place_proof_summary(input, cx, b, x0, w, y, gap, page);
}

/// The checker's summary under a theorem ("Checker: proved", green only for
/// proved). Preview only: it has no caret stops and is never in the file.
#[allow(clippy::too_many_arguments)]
fn place_proof_summary(
    input: &PageInput<'_>,
    cx: &Cx<'_>,
    b: &Block,
    x0: f32,
    w: f32,
    y: &mut f32,
    gap: f32,
    page: &mut Page,
) {
    let Some(line) = input.proof.summary_for(b.span) else {
        return;
    };
    let inl = [Inline {
        kind: InlineKind::Text(format!("Checker: {}", line.text)),
        span: Span::new(b.span.end, b.span.end),
    }];
    let st = Style {
        font: FontId::TextItalic,
        size: input.size * 0.9,
        color: line.tone.color(&input.theme),
    };
    let mut c = Canvas::default();
    let h = flow(cx, &inl, st, x0, 0.0, w, &mut c);
    c.stops.clear();
    c.chars.clear();
    c.hits.clear();
    let top = *y - gap * 0.55;
    push_block(page, b, top, h, c, BlockTag::Prose, input);
    *y = top + h + gap;
}

impl Canvas {
    /// The ✎ marker of a note, drawn as a filled pencil in a `size` square at
    /// `(x, y)` (the fonts have no pencil glyph).
    fn extend_pencil(&mut self, x: f32, y: f32, size: f32, color: mpp_render::Color) {
        use mpp_render::PathCmd::{Close, LineTo, MoveTo};
        let p = |u: f32, v: f32| (x + u * size, y + v * size);
        // A body slanting up to the right, then its point at the bottom left.
        let body = [p(0.30, 0.86), p(0.14, 0.70), p(0.72, 0.12), p(0.88, 0.28)];
        let tip = [p(0.14, 0.70), p(0.30, 0.86), p(0.04, 0.96)];
        for shape in [&body[..], &tip[..]] {
            let mut cmds = vec![MoveTo(shape[0].0, shape[0].1)];
            cmds.extend(shape[1..].iter().map(|&(a, b)| LineTo(a, b)));
            cmds.push(Close);
            self.items.push(Primitive::Path {
                cmds,
                color,
                stroke: None,
            });
        }
    }
}

/// A glyph primitive for one character (the note fold chevron).
fn glyph_at(
    ch: char,
    font: FontId,
    x: f32,
    y: f32,
    size: f32,
    color: mpp_render::Color,
) -> Option<Primitive> {
    let (font, glyph, _) = glyph_for(font, ch)?;
    Some(Primitive::Glyph {
        font,
        glyph,
        ch,
        x,
        y,
        size,
        color,
    })
}

/// A formatted comment as a note: a soft tinted box with a ✎ marker and a bar
/// down its edge, its blocks in smaller grey text (maths greyed, never given
/// results). Folded, it is one line: the marker, the first words and an ellipsis;
/// hovering shows the whole note.
#[allow(clippy::too_many_arguments)]
fn place_comment(
    input: &PageInput<'_>,
    cx: &Cx<'_>,
    b: &Block,
    c: &Comment,
    x0: f32,
    w: f32,
    y: &mut f32,
    gap: f32,
    page: &mut Page,
) {
    let size = input.size;
    let theme = input.theme;
    let pad = size * 0.4;
    let indent = size * 1.9;
    let small = 0.88;
    let top = *y;
    let index = page.blocks.len();
    let folded = input.show.is_folded(b.id);
    let mut bg = Canvas::default();
    let inner_h = if folded {
        let first = note_words(c, input.src, 60);
        let st = Style {
            font: FontId::TextItalic,
            size: size * small,
            color: theme.muted,
        };
        let h = flow_text(
            &format!("{first} \u{2026}"),
            Span::new(b.span.start, b.span.end),
            false,
            st,
            x0 + indent,
            pad,
            &mut bg,
        );
        h.max(size)
    } else {
        let muted = Theme {
            text: theme.muted,
            ..theme
        };
        let inner = PageInput {
            eval: None,
            theme: muted,
            size: size * small,
            ..*input
        };
        let inner_cx = Cx {
            src: cx.src,
            theme: muted,
            slot_labels: cx.slot_labels,
            paper: cx.paper,
        };
        let prev = cx.paper.map(|p| p.set_in_comment(true));
        let inner_gap = gap * 0.6;
        let mut iy = top + pad;
        for blk in &c.blocks {
            place_block(
                &inner,
                &inner_cx,
                blk,
                x0 + indent,
                w - indent - size * 1.6,
                &mut iy,
                inner_gap,
                page,
            );
        }
        if let (Some(p), Some(prev)) = (cx.paper, prev) {
            p.set_in_comment(prev);
        }
        if page.blocks.len() == index {
            // An empty note still has a line to type on.
            bg.stops.push(CaretStop {
                x: x0 + indent,
                top: pad,
                bottom: pad + size,
                offset: b.span.end,
            });
            size
        } else {
            (iy - inner_gap - top - pad).max(size)
        }
    };
    let h = inner_h + 2.0 * pad;
    let text_items = bg.items.len();
    bg.items.push(Primitive::Rule {
        rect: rrect(x0, 0.0, w, h),
        color: theme.note_background,
    });
    bg.items.push(Primitive::Rule {
        rect: rrect(x0, 0.0, 3.0, h),
        color: theme.note_accent,
    });
    let mark = size * 0.95;
    bg.extend_pencil(
        x0 + size * 0.45,
        pad + mark * 0.1,
        mark * 0.8,
        theme.note_accent,
    );
    let chevron = if folded { '\u{25b6}' } else { '\u{25bc}' };
    let cx_right = x0 + w - size * 1.2;
    bg.items.extend(glyph_at(
        chevron,
        FontId::Text,
        cx_right,
        pad + mark * 0.7,
        mark * 0.55,
        theme.note_accent,
    ));
    // Items drawn before the note's blocks: the box goes first in the list.
    let mut items = std::mem::take(&mut bg.items);
    let decorations = items.len() - text_items;
    items.rotate_right(decorations);
    bg.items = items;
    let tip = folded.then(|| comment_latex(c, input.src));
    let fold_rect = if folded {
        Rect::from_min_size(Pos2::new(x0, 0.0), Vec2::new(w, h))
    } else {
        Rect::from_min_size(
            Pos2::new(cx_right - size * 0.3, 0.0),
            Vec2::new(size * 1.5, size * 1.6),
        )
    };
    bg.links.push(PageLink {
        rect: fold_rect,
        link: Link {
            target: LinkTarget::Fold(b.id),
            tip,
        },
    });
    let dl = DisplayList {
        width: page.width,
        height: h,
        depth: 0.0,
        baseline: h,
        font_size: size,
        items: bg.items.clone(),
        hits: vec![],
    };
    let shift = Vec2::new(0.0, top);
    for s in &bg.stops {
        page.stops.push(CaretStop {
            top: s.top + top,
            bottom: s.bottom + top,
            ..*s
        });
    }
    page.links.extend(bg.links.iter().map(|l| PageLink {
        rect: l.rect.translate(shift),
        link: l.link.clone(),
    }));
    page.blocks.insert(
        index,
        PlacedBlock {
            id: b.id,
            span: b.span,
            top,
            height: h,
            dl,
            tag: BlockTag::Prose,
        },
    );
    *y = top + h + gap;
}

/// The first words of a note, as plain reading text (maths shown as written
/// between the dollars would be file text, so it is left out).
pub fn note_words(c: &Comment, src: &str, max: usize) -> String {
    let mut words = String::new();
    for blk in &c.blocks {
        let inl = match &blk.kind {
            BlockKind::Paragraph(i) | BlockKind::Heading { content: i, .. } => i.clone(),
            BlockKind::DisplayMath(_) => {
                words.push_str(" (maths)");
                continue;
            }
            _ => scan_inlines(src, blk.span),
        };
        for i in &inl {
            plain_into(&i.kind, src, &mut words);
        }
        if words.chars().count() >= max {
            break;
        }
        words.push(' ');
    }
    let words = words.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut cut: String = words.chars().take(max).collect();
    if cut.is_empty() {
        cut = "Note".into();
    }
    cut
}

fn plain_into(kind: &InlineKind, src: &str, out: &mut String) {
    match kind {
        InlineKind::Text(t) | InlineKind::Code(t) => out.push_str(t),
        InlineKind::SoftBreak => out.push(' '),
        InlineKind::Emphasis(v) | InlineKind::Strong(v) | InlineKind::Link { text: v, .. } => {
            for i in v {
                plain_into(&i.kind, src, out);
            }
        }
        // A single letter or number reads the same as text; other maths would
        // show its file text, so it becomes an ellipsis.
        InlineKind::Math(m) => match src.get(m.raw.start..m.raw.end).map(str::trim) {
            Some(t) if !t.is_empty() && t.chars().all(|c| c.is_alphanumeric() || c == '.') => {
                out.push_str(t)
            }
            _ => out.push('\u{2026}'),
        },
        InlineKind::WikiLink { path, alias } => out.push_str(alias.as_deref().unwrap_or(path)),
        InlineKind::Escape(c) => out.push(*c),
        _ => {}
    }
}

/// The whole note typeset for its hover card.
fn comment_latex(c: &Comment, src: &str) -> String {
    let mut parts = vec![];
    for blk in &c.blocks {
        match &blk.kind {
            BlockKind::Paragraph(i) | BlockKind::Heading { content: i, .. } => {
                parts.push(crate::explorer::outline::inlines_latex(i, src));
            }
            BlockKind::DisplayMath(m) => {
                if let Some(raw) = src.get(m.raw.start..m.raw.end) {
                    parts.push(raw.trim().to_string());
                }
            }
            BlockKind::List(l) => {
                for item in &l.items {
                    parts.push(format!(
                        "\\text{{\u{2022} }} {}",
                        crate::explorer::outline::inlines_latex(&item.content, src)
                    ));
                }
            }
            _ => {}
        }
    }
    parts.join("\n")
}

fn push_block(
    page: &mut Page,
    b: &Block,
    top: f32,
    h: f32,
    canvas: Canvas,
    tag: BlockTag,
    input: &PageInput<'_>,
) {
    page.chars.extend(canvas.chars.iter().copied());
    let shift = Vec2::new(0.0, top);
    page.step_toggles
        .extend(canvas.toggles.iter().map(|t| StepToggle {
            rect: t.rect.translate(shift),
            line: t.line,
        }));
    page.spinners
        .extend(canvas.spinners.iter().map(|p| *p + shift));
    page.links.extend(canvas.links.iter().map(|l| PageLink {
        rect: l.rect.translate(shift),
        link: l.link.clone(),
    }));
    page.chips
        .extend(canvas.chips.iter().map(|r| r.translate(shift)));
    for s in &canvas.stops {
        page.stops.push(CaretStop {
            x: s.x,
            top: s.top + top,
            bottom: s.bottom + top,
            offset: s.offset,
        });
    }
    let dl = DisplayList {
        width: page.width,
        height: h,
        depth: 0.0,
        baseline: h,
        font_size: input.size,
        items: canvas.items,
        hits: canvas.hits,
    };
    page.blocks.push(PlacedBlock {
        id: b.id,
        span: b.span,
        top,
        height: h,
        dl,
        tag,
    });
}

fn style(input: &PageInput<'_>, font: FontId, scale: f32) -> Style {
    Style {
        font,
        size: input.size * scale,
        color: input.theme.text,
    }
}

fn layout_block(
    input: &PageInput<'_>,
    cx: &Cx<'_>,
    b: &Block,
    x0: f32,
    w: f32,
) -> Option<(Canvas, f32, BlockTag)> {
    let mut c = Canvas::default();
    let size = input.size;
    let mut tag = BlockTag::Prose;
    let h = match &b.kind {
        BlockKind::Heading { level, content } => {
            let scale = match level {
                1 => 1.9,
                2 => 1.5,
                3 => 1.25,
                _ => 1.1,
            };
            let numbered;
            let number = cx
                .paper
                .and_then(|p| p.heading_number(*level))
                .filter(|_| !mpp_syntax::document::heading_has_number(content));
            let content = match number {
                Some(n) => {
                    let at = Span::new(b.span.start, b.span.start);
                    let mut v = vec![Inline {
                        kind: InlineKind::Text(format!("{n}\u{2003}")),
                        span: at,
                    }];
                    v.extend(content.iter().cloned());
                    numbered = v;
                    &numbered
                }
                None => content,
            };
            let st = style(input, FontId::TextBold, scale);
            let h = flow(cx, content, st, x0, 0.0, w, &mut c);
            if c.stops.is_empty() {
                // An empty heading (`#` just typed) is still a line the caret is on.
                let h = h.max(st.size * 1.5);
                c.stops.push(CaretStop {
                    x: x0,
                    top: 0.0,
                    bottom: h,
                    offset: b.span.end,
                });
                h
            } else {
                h
            }
        }
        BlockKind::Title(t) => layout_title(input, cx, t, b.span.end, x0, w, &mut c),
        BlockKind::References(r) => layout_references(input, cx, b, r, x0, w, &mut c)?,
        BlockKind::Comment(_) => return None,
        BlockKind::Paragraph(content) => flow(
            cx,
            content,
            style(input, FontId::Text, 1.0),
            x0,
            0.0,
            w,
            &mut c,
        ),
        BlockKind::Let(l) => flow(
            cx,
            &l.content,
            style(input, FontId::Text, 1.0),
            x0,
            0.0,
            w,
            &mut c,
        ),
        BlockKind::Using(_)
        | BlockKind::Helper(_)
        | BlockKind::Analyse(_)
        | BlockKind::Solve(_)
        | BlockKind::Assume(_)
        | BlockKind::Data(_) => {
            let inl = scan_inlines(input.src, b.span);
            let h = flow(
                cx,
                &inl,
                style(input, FontId::Text, 1.0),
                x0,
                0.0,
                w,
                &mut c,
            );
            // Helper sentences show what they found under the sentence.
            let found = match (&b.kind, input.eval.and_then(|ev| ev.block(b.id))) {
                (BlockKind::Helper(_) | BlockKind::Analyse(_) | BlockKind::Solve(_), Some(br)) => {
                    layout_sentence_results(input, br, x0, h + size * 0.2, w, &mut c)
                }
                _ => 0.0,
            };
            h + found
        }
        BlockKind::List(list) => layout_list(input, cx, list, x0, 0.0, w, &mut c),
        BlockKind::Table(t) => {
            let n = t.header.len().max(1);
            let col_w = w / n as f32;
            let mut y = 0.0;
            c.items.push(Primitive::Rule {
                rect: rrect(x0, 0.0, w, 1.0),
                color: input.theme.rule,
            });
            y += size * 0.3;
            let rows = std::iter::once((&t.header, true)).chain(t.rows.iter().map(|r| (r, false)));
            for (row, head) in rows {
                let mut row_h = 0.0f32;
                for (i, cell) in row.iter().enumerate() {
                    let st = style(
                        input,
                        if head { FontId::TextBold } else { FontId::Text },
                        0.95,
                    );
                    let cx0 = x0 + col_w * i as f32;
                    let mut cc = Canvas::default();
                    let ch = flow(cx, cell, st, 0.0, 0.0, col_w - size * 0.8, &mut cc);
                    let width: f32 = cc
                        .items
                        .iter()
                        .filter_map(|p| match p {
                            Primitive::Glyph { x, .. } => Some(*x),
                            _ => None,
                        })
                        .fold(0.0, f32::max);
                    let shift = match t.aligns.get(i).copied().unwrap_or(Align::Default) {
                        Align::Right => (col_w - size * 0.8 - width).max(0.0),
                        Align::Center => ((col_w - size * 0.8 - width) / 2.0).max(0.0),
                        _ => 0.0,
                    };
                    let dl = DisplayList {
                        items: cc.items,
                        hits: cc.hits,
                        ..DisplayList::default()
                    };
                    c.merge(&dl, cx0 + shift, y, false);
                    c.chars.extend(cc.chars.iter().copied());
                    c.chips.extend(
                        cc.chips
                            .iter()
                            .map(|r| r.translate(Vec2::new(cx0 + shift, y))),
                    );
                    for s in cc.stops {
                        c.stops.push(CaretStop {
                            x: s.x + cx0 + shift,
                            top: s.top + y,
                            bottom: s.bottom + y,
                            offset: s.offset,
                        });
                    }
                    row_h = row_h.max(ch);
                }
                y += row_h + size * 0.2;
                if head {
                    c.items.push(Primitive::Rule {
                        rect: rrect(x0, y, w, 0.8),
                        color: input.theme.rule,
                    });
                    y += size * 0.2;
                }
            }
            c.items.push(Primitive::Rule {
                rect: rrect(x0, y, w, 1.0),
                color: input.theme.rule,
            });
            y + size * 0.2
        }
        BlockKind::Quote(paras) => {
            let mut y = 0.0;
            for p in paras {
                let mut st = style(input, FontId::TextItalic, 1.0);
                st.color = input.theme.muted;
                y += flow(cx, p, st, x0 + size * 1.2, y, w - size * 1.2, &mut c) + size * 0.3;
            }
            c.items.push(Primitive::Rule {
                rect: rrect(x0 + size * 0.3, 0.0, 2.0, y),
                color: input.theme.muted,
            });
            y
        }
        BlockKind::Verbatim(v) => {
            let mut y = size * 0.3;
            let mut off = v.text_span.start;
            let mapped = input.src.get(v.text_span.start..v.text_span.end) == Some(v.text.as_str());
            let mut st = style(input, FontId::Text, 0.92);
            st.color = input.theme.text;
            let bg = c.items.len();
            for line in v.text.split('\n') {
                let span = Span::new(off, off + line.len());
                y += flow_text(line, span, mapped, st, x0 + size * 0.6, y, &mut c);
                off += line.len() + 1;
            }
            y += size * 0.3;
            c.items.insert(
                bg,
                Primitive::Rule {
                    rect: rrect(x0, 0.0, w, y),
                    color: input.theme.code_background,
                },
            );
            y
        }
        BlockKind::Rule => {
            c.items.push(Primitive::Rule {
                rect: rrect(x0, size * 0.4, w, 1.0),
                color: input.theme.rule,
            });
            c.stops.push(CaretStop {
                x: x0,
                top: 0.0,
                bottom: size,
                offset: b.span.start,
            });
            tag = BlockTag::Rule;
            size * 0.8
        }
        BlockKind::DisplayMath(m) => {
            tag = BlockTag::Math;
            layout_display(input, b, m, x0, w, &mut c)
        }
        BlockKind::Figure(f) => {
            let is_3d = input.show.figure_3d.contains(&b.id);
            let box_h = if is_3d { size * 16.0 } else { size * 12.0 };
            let box_w = (w * 0.8).min(size * 40.0);
            // The figure's maths (what is drawn, the ranges) as an editable line
            // above it, typeset, so its template boxes can be filled in.
            let spec = figure_spec_inlines(input.src, b, f);
            let mut top = size * 0.3;
            if !spec.is_empty() {
                let mut st = style(input, FontId::Text, 0.95);
                st.color = input.theme.muted;
                top += flow(cx, &spec, st, x0 + (w - box_w) / 2.0, 0.0, box_w, &mut c);
                top += size * 0.3;
            }
            let rect = Rect::from_min_size(
                Pos2::new(x0 + (w - box_w) / 2.0, top),
                Vec2::new(box_w, box_h),
            );
            tag = BlockTag::Figure(rect);
            c.stops.push(CaretStop {
                x: rect.left(),
                top: rect.top(),
                bottom: rect.bottom(),
                offset: b.span.start,
            });
            let mut y = rect.bottom() + size * 0.4;
            // Slider rows (drawn by the view) sit between the graph and the caption.
            y += input.show.rows_for(b.id) as f32 * slider_row_height(size);
            if !f.caption.is_empty() {
                let mut st = style(input, FontId::Text, 0.92);
                st.color = input.theme.muted;
                y += flow(cx, &f.caption, st, x0, y, w, &mut c);
            } else {
                c.stops.push(CaretStop {
                    x: rect.right(),
                    top: rect.top(),
                    bottom: rect.bottom(),
                    offset: b.span.end,
                });
            }
            y
        }
        BlockKind::EmbedImport { path, .. } => {
            let inl = [
                Inline {
                    kind: InlineKind::Text("Imports ".into()),
                    span: Span::new(b.span.start, b.span.start),
                },
                Inline {
                    kind: InlineKind::WikiLink {
                        path: path.clone(),
                        alias: Some(display_name(path)),
                    },
                    span: b.span,
                },
            ];
            let mut st = style(input, FontId::Text, 1.0);
            st.color = input.theme.muted;
            flow(cx, &inl, st, x0, 0.0, w, &mut c)
        }
        BlockKind::FootnoteDef { name, content } => {
            let label = cx
                .paper
                .and_then(|p| p.notes.get(name))
                .map_or_else(|| name.clone(), |n| n.number.to_string());
            let mut inl = vec![Inline {
                kind: InlineKind::Text(format!("{label}. ")),
                span: Span::new(b.span.start, b.span.start),
            }];
            inl.extend(content.iter().cloned());
            let at = c.stops.len();
            let h = flow(
                cx,
                &inl,
                style(input, FontId::Text, 0.9),
                x0,
                0.0,
                w,
                &mut c,
            );
            // An empty note has no text to stop at: the caret waits after its number,
            // where the note's words will go.
            let own = input.src.get(b.span.start..b.span.end).unwrap_or("");
            if let Some(i) = own.find("]:") {
                let start = b.span.start + i + 2;
                let start = start + usize::from(input.src[start..].starts_with(' '));
                if let Some(last) = c.stops[at..]
                    .iter()
                    .copied()
                    .max_by(|p, q| p.x.total_cmp(&q.x))
                {
                    if !c.stops[at..].iter().any(|s| s.offset >= start) {
                        c.stops.push(CaretStop {
                            offset: start,
                            ..last
                        });
                    }
                }
            }
            h
        }
        BlockKind::Directive(d) => {
            // Settings of the file, not text to read.
            if matches!(
                d.kind,
                DirectiveKind::NoEval | DirectiveKind::NumberSections | DirectiveKind::Cite(_)
            ) {
                return None;
            }
            // A number setting (`%digits 4`) is a quiet directive as written,
            // upright and small, never a sentence in italics (E8).
            let setting = d.kind == DirectiveKind::Display;
            let shown = if setting {
                format!("%{}", d.text)
            } else {
                d.text.clone()
            };
            let inl = [Inline {
                kind: InlineKind::Text(shown),
                span: b.span,
            }];
            let mut st = if setting {
                style(input, FontId::Text, 0.8)
            } else {
                style(input, FontId::TextItalic, 0.9)
            };
            st.color = input.theme.muted;
            flow(cx, &inl, st, x0, 0.0, w, &mut c)
        }
        BlockKind::Theorem(_) => return None,
    };
    Some((c, h, tag))
}

/// One line of the title block: the title centred and large, authors and the
/// date centred, the abstract narrower under a bold centred "Abstract".
fn layout_title(
    input: &PageInput<'_>,
    cx: &Cx<'_>,
    t: &TitleField,
    end: usize,
    x0: f32,
    w: f32,
    c: &mut Canvas,
) -> f32 {
    let size = input.size;
    if t.is_empty() && t.kind != TitleKind::Abstract {
        return layout_empty_title_field(input, cx, t.kind, end, x0, w, c);
    }
    match t.kind {
        TitleKind::Title => {
            size * 0.3
                + flow_center(
                    cx,
                    &t.content,
                    style(input, FontId::TextBold, 1.75),
                    x0,
                    size * 0.3,
                    w,
                    c,
                )
        }
        TitleKind::Author => flow_center(
            cx,
            &t.content,
            style(input, FontId::Text, 1.1),
            x0,
            0.0,
            w,
            c,
        ),
        TitleKind::Date => {
            let mut st = style(input, FontId::Text, 1.0);
            st.color = input.theme.muted;
            flow_center(cx, &t.content, st, x0, 0.0, w, c)
        }
        TitleKind::Abstract => {
            let head = [Inline {
                kind: InlineKind::Text("Abstract".into()),
                span: Span::new(0, 0),
            }];
            let mut hc = Canvas::default();
            let h1 = flow_center(
                cx,
                &head,
                style(input, FontId::TextBold, 0.95),
                x0,
                size * 0.4,
                w,
                &mut hc,
            );
            // The heading is generated: its glyphs are drawn, but it has no caret stops.
            c.items.extend(hc.items);
            let inset = (w * 0.08).min(size * 3.0);
            let h2 = flow(
                cx,
                &t.content,
                style(input, FontId::Text, 0.95),
                x0 + inset,
                size * 0.4 + h1,
                w - 2.0 * inset,
                c,
            );
            size * 0.4 + h1 + h2 + size * 0.4
        }
    }
}

/// A title-block field with nothing written yet: a grey label naming it, like an
/// empty box ("Author"), and the caret stop to type there. The file's keyword
/// (`Author:`) is never drawn, and the paper and the exports leave the field out.
fn layout_empty_title_field(
    input: &PageInput<'_>,
    cx: &Cx<'_>,
    kind: TitleKind,
    end: usize,
    x0: f32,
    w: f32,
    c: &mut Canvas,
) -> f32 {
    // Once the caret has left the title block, an empty author or date takes no
    // space: a finished page shows no words that are not content.
    if kind != TitleKind::Title && !input.show.editing_title {
        return 0.0;
    }
    let size = input.size;
    let scale = match kind {
        TitleKind::Title => 1.75,
        TitleKind::Author => 1.1,
        _ => 1.0,
    };
    let label = [Inline {
        kind: InlineKind::Text(kind.as_str().into()),
        span: Span::new(0, 0),
    }];
    let mut st = style(input, FontId::TextItalic, scale);
    st.color = input.theme.muted;
    let top = if kind == TitleKind::Title {
        size * 0.3
    } else {
        0.0
    };
    let mut lc = Canvas::default();
    let h = flow_center(cx, &label, st, x0, top, w, &mut lc);
    // Generated words: drawn, with one caret stop where the text goes.
    let left = lc
        .items
        .iter()
        .filter_map(|p| match p {
            Primitive::Glyph { x, .. } => Some(*x),
            _ => None,
        })
        .fold(f32::INFINITY, f32::min);
    c.items.extend(lc.items);
    c.stops.push(CaretStop {
        x: if left.is_finite() { left } else { x0 + w / 2.0 },
        top,
        bottom: top + h,
        offset: end,
    });
    top + h
}

/// The reference list, drawn at the first `References` block: a "References"
/// heading, then each entry (numbered `[n]` with a hanging indent, or by author
/// and year). Entries written in this file keep a caret stop at their start, so a
/// click on a citation can jump to them.
fn layout_references(
    input: &PageInput<'_>,
    cx: &Cx<'_>,
    b: &Block,
    r: &References,
    x0: f32,
    w: f32,
    c: &mut Canvas,
) -> Option<f32> {
    let paper = cx.paper?;
    let size = input.size;
    let first = paper.ref_block == Some(b.span) && !paper.in_comment();
    if !first {
        return None;
    }
    let at = Span::new(b.span.start, b.span.start);
    let head = [Inline {
        kind: InlineKind::Text("References".into()),
        span: at,
    }];
    let mut y = flow(
        cx,
        &head,
        style(input, FontId::TextBold, 1.5),
        x0,
        0.0,
        w,
        c,
    );
    y += size * 0.3;
    let mut note = style(input, FontId::TextItalic, 0.95);
    note.color = input.theme.muted;
    for path in &paper.missing_files {
        let text = format!("The references in {} are not loaded.", display_name(path));
        y += flow_text(&text, at, false, note, x0, y, c);
    }
    let list = paper.listing();
    let numbered = list.iter().any(|(n, _)| n.is_some());
    let hang = if numbered { size * 2.2 } else { size * 1.5 };
    for (n, entry) in list {
        let in_file = r.entries.iter().any(|e| e.span == entry.span)
            || b.span.start <= entry.span.start && entry.span.end <= b.span.end;
        let span = if in_file { entry.span } else { at };
        let (head, title, tail) = entry_text(entry);
        let base = style(input, FontId::Text, 0.95);
        let (x, width) = match n {
            Some(k) => {
                let label = format!("[{k}]");
                flow_text(
                    &label,
                    Span::new(span.start, span.start),
                    false,
                    base,
                    x0,
                    y,
                    c,
                );
                (x0 + hang, w - hang)
            }
            None => (x0, w),
        };
        let inl = [
            Inline {
                kind: InlineKind::Text(head),
                span: Span::new(span.start, span.start),
            },
            Inline {
                kind: InlineKind::Emphasis(vec![Inline {
                    kind: InlineKind::Text(title),
                    span: Span::new(span.start, span.start),
                }]),
                span: Span::new(span.start, span.start),
            },
            Inline {
                kind: InlineKind::Text(tail),
                span: Span::new(span.end, span.end),
            },
        ];
        y += flow(cx, &inl, base, x, y, width, c) + size * 0.25;
    }
    Some(y)
}

/// The maths of a `Figure:` line without the `Figure:` keyword and the final
/// full stop (the caption is laid out separately).
fn figure_spec_inlines(src: &str, b: &Block, f: &mpp_syntax::document::Figure) -> Vec<Inline> {
    let end = f
        .caption
        .first()
        .map_or(b.span.end, |c| c.span.start)
        .min(b.span.end);
    let mut inl = scan_inlines(src, Span::new(b.span.start, end));
    if let Some(Inline {
        kind: InlineKind::Text(t),
        span,
    }) = inl.first_mut()
    {
        if let Some(rest) = t.strip_prefix("Figure:") {
            let cut = t.len() - rest.trim_start().len();
            *t = rest.trim_start().to_string();
            span.start += cut;
        }
    }
    if let Some(Inline {
        kind: InlineKind::Text(t),
        span,
    }) = inl.last_mut()
    {
        let kept = t.trim_end().trim_end_matches('.').len();
        span.end = span.start + kept;
        t.truncate(kept);
    }
    inl.retain(|i| !matches!(&i.kind, InlineKind::Text(t) if t.is_empty()));
    figure_keywords_in_words(inl)
}

/// Shows the figure keywords of a `Figure:` line in words ("riemann mid" →
/// "midpoint rule,", "taylor" → "Taylor polynomial of degree"), from the shared
/// display table. The words stand at the keyword's place but cover no source
/// text: the keyword is the file's structure, not something to type over.
fn figure_keywords_in_words(inl: Vec<Inline>) -> Vec<Inline> {
    use mpp_syntax::document::FigureKeyword;
    use mpp_syntax::math::display::{capitalised, figure_keyword_words};
    let mut out: Vec<Inline> = vec![];
    for (n, it) in inl.into_iter().enumerate() {
        let InlineKind::Text(t) = &it.kind else {
            out.push(it);
            continue;
        };
        let (t, base) = (t.clone(), it.span.start);
        let mut plain = 0;
        let mut i = 0;
        while i < t.len() {
            let at_word = t[..i].chars().last().is_none_or(|c| !c.is_alphanumeric());
            let found = at_word
                .then(|| {
                    FigureKeyword::ALL.iter().find(|k| {
                        let kw = k.as_str();
                        t[i..].starts_with(kw)
                            && t[i + kw.len()..]
                                .chars()
                                .next()
                                .is_none_or(|c| !c.is_alphanumeric())
                    })
                })
                .flatten();
            if let Some(k) = found {
                if i > plain {
                    out.push(Inline {
                        kind: InlineKind::Text(t[plain..i].to_string()),
                        span: Span::new(base + plain, base + i),
                    });
                }
                let words = figure_keyword_words(*k);
                let first = n == 0 && t[..i].trim().is_empty();
                let words = if first {
                    capitalised(words)
                } else {
                    words.to_string()
                };
                out.push(Inline {
                    kind: InlineKind::Text(words),
                    span: Span::new(base + i, base + i),
                });
                i += k.as_str().len();
                plain = i;
            } else {
                i += t[i..].chars().next().map_or(1, char::len_utf8);
            }
        }
        if plain == 0 {
            out.push(it);
        } else if plain < t.len() {
            out.push(Inline {
                kind: InlineKind::Text(t[plain..].to_string()),
                span: Span::new(base + plain, base + t.len()),
            });
        }
    }
    out
}

fn rrect(x: f32, y: f32, w: f32, h: f32) -> RRect {
    RRect { x, y, w, h }
}

fn layout_list(
    input: &PageInput<'_>,
    cx: &Cx<'_>,
    list: &List,
    x0: f32,
    y0: f32,
    w: f32,
    c: &mut Canvas,
) -> f32 {
    let size = input.size;
    let indent = size * 1.6;
    let mut y = y0;
    for (i, item) in list.items.iter().enumerate() {
        let marker = if list.ordered {
            format!("{}.", item.number.unwrap_or(i as u32 + 1))
        } else {
            "\u{2022}".to_string()
        };
        let st = style(input, FontId::Text, 1.0);
        let mw = text_width(FontId::Text, &marker) * size;
        let mut x = x0 + indent - mw - size * 0.4;
        for ch in marker.chars() {
            if let Some((font, glyph, adv)) = glyph_for(FontId::Text, ch) {
                c.items.push(Primitive::Glyph {
                    font,
                    glyph,
                    ch,
                    x,
                    y: y + size * 1.0,
                    size,
                    color: st.color,
                });
                x += adv * size;
            }
        }
        y += flow(cx, &item.content, st, x0 + indent, y, w - indent, c);
        for child in &item.children {
            y += layout_list(input, cx, child, x0 + indent, y, w - indent, c);
        }
    }
    y - y0
}

fn layout_display(
    input: &PageInput<'_>,
    b: &Block,
    m: &DisplayMath,
    x0: f32,
    w: f32,
    c: &mut Canvas,
) -> f32 {
    let size = input.size;
    let mut extras = input
        .eval
        .and_then(|ev| ev.block(b.id))
        .map(|br| extras_for(br, m, input.show, block_delta(b, br)))
        .unwrap_or_default();
    // A line read as a product of letters it probably did not mean (`sinn(2)`)
    // gets a warning and no answer, never a nonsense one.
    if !extras.results.is_empty() {
        let known = crate::diagnostics::known_names(input.eval);
        let bad = crate::diagnostics::lines_with_letter_runs(m, &known);
        extras.results.retain(|r| !bad.contains(&r.line));
    }
    // Answers with a slot of their own (U16, U17) are drawn after the formula.
    let slots = super::answers::slots(input, b, &m.ast);
    extras
        .results
        .retain(|r| !super::answers::takes(&slots, r.line));
    extras.results.extend(
        slots
            .iter()
            .filter(|s| s.open)
            .map(|s| super::answers::placeholder(s.line)),
    );
    let opts = LayoutOptions {
        style: MathStyle::Display,
        font_size: size * 1.1,
        theme: input.theme,
        equation_number: m.number.map(|n| n.to_string()),
        width: Some(w),
        results: extras.results.clone(),
        slot_labels: input
            .slot_labels
            .iter()
            .filter(|(o, _)| m.raw.start <= *o && *o <= m.raw.end)
            .cloned()
            .collect(),
        ..LayoutOptions::default()
    }
    .with_integral_limits(input.integral_limits);
    let text = input.src.get(m.raw.start..m.raw.end).unwrap_or("");
    let dl = super::mathcache::layout(text, m.raw, &m.ast, &opts);
    // A maths line is about 1.5 lines tall whatever is typed into it: room for
    // an exponent above and an index below is kept from the start, so typing
    // `x^2` or `x_1` never changes the line's height or moves its baseline.
    let pad = size * 0.2;
    let fs = size * 1.1;
    let above = dl.height.max(fs * 0.95);
    let below = dl.depth.max(fs * 0.3);
    let top = pad + above - dl.height;
    let before = c.stops.len();
    c.merge(&dl, x0, top, true);
    // What is typed past the last stop the formula has (`v = ` before its
    // value): the caret goes after what is drawn of it (not the answer drawn
    // after it), where the next character will be, not back beside the `v` (E2).
    let content_end = m.raw.start + text.trim_end().len();
    let last = c.stops[before..].iter().copied().max_by_key(|s| s.offset);
    if let Some(last) = last.filter(|s| s.offset < content_end) {
        c.stops.push(CaretStop {
            x: (x0 + ink_right_of_formula(&dl)).max(last.x),
            offset: content_end,
            ..last
        });
    }
    if c.stops.len() == before {
        // An empty line: the caret stands on the baseline, as tall as a capital
        // letter, where the first character typed will be (not hanging below).
        let baseline = top + dl.height;
        c.stops.push(CaretStop {
            x: x0 + w / 2.0,
            top: baseline - fs * 0.75,
            bottom: baseline + fs * 0.1,
            offset: m.raw.start,
        });
    }
    if extras.pending {
        let right = ink_right(&dl);
        c.spinners.push(Pos2::new(
            x0 + (right + size * 0.9).min(w - size * 0.5),
            top + dl.total_height() / 2.0,
        ));
    }
    // An empty box already gets the editor's own "fill in" note beside the
    // equation (with the template's name for the box): one note, not two.
    let has_slot = dl.hits.iter().any(|h| h.kind == HitKind::Slot);
    let hints: Vec<&String> = extras
        .hints
        .iter()
        .filter(|h| !(has_slot && h.starts_with("fill in")))
        .collect();
    if !hints.is_empty() {
        let tex = format!(
            "\\text{{{}}}",
            hints
                .iter()
                .map(|h| h.as_str())
                .collect::<Vec<_>>()
                .join("; ")
        );
        let theme = Theme {
            text: input.theme.placeholder_label,
            ..input.theme
        };
        let hd = snippet(&tex, size * 0.85, theme);
        let x = (ink_right(&dl) + size).min(w - hd.width).max(0.0);
        c.merge(
            &hd,
            x0 + x,
            top + (dl.total_height() - hd.total_height()) / 2.0,
            false,
        );
    }
    // Answer rows under the block (lane B2), below the room kept for scripts (B1).
    let answer_rows = super::answers::place(
        input,
        &m.ast,
        m.number.is_some(),
        &slots,
        &dl,
        x0,
        top,
        w,
        c,
    );
    let mut y = pad + above + below + size * 0.35 + answer_rows;
    for note in &extras.notes {
        y += layout_note(input, note, x0, y, w, c);
    }
    for rows in &extras.tables {
        y += layout_table(input, rows, x0, y, w, c);
    }
    for (line, tree) in &extras.steps {
        y += layout_steps(input, *line, tree, x0, y, w, c);
    }
    y
}

/// A note under an equation (why it has no answer) as typeset words and maths,
/// broken into lines that fit `w`. Returns its height.
fn layout_note(
    input: &PageInput<'_>,
    note: &Rich,
    x0: f32,
    y0: f32,
    w: f32,
    c: &mut Canvas,
) -> f32 {
    let small = input.size * 0.9;
    let theme = Theme {
        text: input.theme.muted,
        ..input.theme
    };
    // A little room at the right for the overhang of italic letters.
    let room = w - small * 0.5;
    let lines = note_lines(note, |latex| snippet(latex, small, theme).width <= room);
    let mut y = y0;
    for l in &lines {
        let dl = snippet(l, small, theme);
        c.merge(&dl, x0, y, false);
        y += dl.total_height().max(small) + input.size * 0.1;
    }
    y - y0 + input.size * 0.2
}

/// The notes shown under each display maths block of a document (for tests).
#[cfg(test)]
pub(crate) fn page_notes(doc: &Document, eval: &Evaluated) -> Vec<Rich> {
    let mut out = vec![];
    doc.walk(&mut |b| {
        if let (BlockKind::DisplayMath(m), Some(br)) = (&b.kind, eval.block(b.id)) {
            let ex = extras_for(br, m, &ShowState::default(), block_delta(b, br));
            out.extend(ex.notes);
        }
    });
    out
}

/// A note's words and maths as LaTeX lines, each as long as `fits` allows: the words
/// in `\text`, each piece of maths kept whole (typeset by the maths layout, never as
/// flattened text) and moved to the next line when it does not fit.
pub(crate) fn note_lines(note: &Rich, fits: impl Fn(&str) -> bool) -> Vec<String> {
    // Each atom as LaTeX, with whether a space comes before it.
    let mut atoms: Vec<(String, bool)> = vec![];
    let mut pending_space = false;
    for seg in &note.0 {
        match seg {
            Seg::Text(t) | Seg::Italic(t) => {
                let mut space_before = pending_space || t.starts_with(' ');
                for word in t.split(' ') {
                    if word.is_empty() {
                        space_before = true;
                        continue;
                    }
                    let word = plain_text(word);
                    if !word.is_empty() {
                        atoms.push((format!("\\text{{{word}}}"), space_before));
                    }
                    space_before = true;
                }
                pending_space = t.ends_with(' ');
            }
            Seg::Math(m) => {
                atoms.push((format!("{{{m}}}"), pending_space));
                pending_space = false;
            }
        }
    }
    let join = |line: &[(String, bool)]| -> String {
        let mut out = String::new();
        for (i, (atom, spaced)) in line.iter().enumerate() {
            if i > 0 && *spaced {
                out.push_str("\\text{ }");
            }
            out.push_str(atom);
        }
        out
    };
    let mut lines: Vec<Vec<(String, bool)>> = vec![];
    let mut line: Vec<(String, bool)> = vec![];
    for atom in atoms {
        line.push(atom);
        if line.len() == 1 || fits(&join(&line)) {
            continue;
        }
        // Too wide: break before the last spaced piece, so a piece glued to the one
        // before it (a full stop or bracket after maths) moves with it.
        let cut = line.iter().rposition(|(_, spaced)| *spaced).unwrap_or(0);
        if cut > 0 {
            let rest = line.split_off(cut);
            lines.push(std::mem::replace(&mut line, rest));
        }
    }
    if !line.is_empty() {
        lines.push(line);
    }
    lines.iter().map(|l| join(l)).collect()
}

/// How far a block has moved since the evaluation `br` comes from (an edit above it
/// that is not evaluated yet).
fn block_delta(b: &Block, br: &BlockResult) -> isize {
    b.span.start as isize - br.span.start as isize
}

/// A fingerprint of everything the page shows from an evaluation (the results,
/// tables, working, spinners and hints of maths blocks and the findings of helper
/// sentences), with positions relative to their blocks: a new evaluation with the
/// same fingerprint lays the page out exactly as before.
pub(crate) fn eval_view(doc: &Document, eval: Option<&Evaluated>, show: &ShowState) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    let Some(ev) = eval else {
        return h.finish();
    };
    doc.walk(&mut |b| {
        let Some(br) = ev.block(b.id) else {
            return;
        };
        match &b.kind {
            BlockKind::DisplayMath(_) => {
                // Everything `extras_for` reads, without laying anything out: lines at
                // their place in the block, their answers, spinners, hints and working.
                let shows = |v: &mpp_eval::LineResult| {
                    v.text.is_some()
                        || v.latex.is_some()
                        || v.approx.is_some()
                        || v.hint.is_some()
                        || v.pending
                        || v.verdict.is_some()
                        || v.steps.is_some()
                };
                if !br.values.iter().any(shows) {
                    return;
                }
                b.id.hash(&mut h);
                let delta = block_delta(b, br);
                for v in br.values.iter().filter(|v| shows(v)) {
                    let at = v.span.start.saturating_add_signed(delta);
                    let end = v.span.end.saturating_add_signed(delta);
                    (
                        at.wrapping_sub(b.span.start),
                        end.wrapping_sub(b.span.start),
                    )
                        .hash(&mut h);
                    format!("{:?}", v.kind).hash(&mut h);
                    (&v.text, &v.latex, &v.approx, &v.hint).hash(&mut h);
                    (v.pending, v.auto, v.value.is_none()).hash(&mut h);
                    v.verdict.as_ref().map(Verdict::passed).hash(&mut h);
                    matches!(v.value, Some(Value::Relation(_))).hash(&mut h);
                    matches!(
                        v.input.as_ref().map(mpp_core::Expr::node),
                        Some(mpp_core::ExprNode::Relation(..))
                    )
                    .hash(&mut h);
                    if let Some(Value::Table(t)) = &v.value {
                        format!("{:?}", mpp_core::table::rows(t)).hash(&mut h);
                    }
                    let span = Span::new(at, end);
                    if let Some(f) = show.format_for(span) {
                        format!("{f:?}").hash(&mut h);
                    }
                    if v.steps.is_some() {
                        format!("{:?}", v.step_tree()).hash(&mut h);
                    }
                }
            }
            BlockKind::Helper(_) | BlockKind::Analyse(_) | BlockKind::Solve(_) => {
                b.id.hash(&mut h);
                for v in &br.values {
                    v.latex.hash(&mut h);
                    format!(
                        "{:?}",
                        v.value.as_ref().map(|v| matches!(v, Value::Table(_)))
                    )
                    .hash(&mut h);
                    if let Some(Value::Table(t)) = &v.value {
                        format!("{:?}", mpp_core::table::rows(t)).hash(&mut h);
                    }
                }
            }
            _ => {}
        }
    });
    super::answers::view_hash(doc, ev, &mut h);
    h.finish()
}

/// The right edge of the ink of a display list: boxes, glyphs and rules.
pub(crate) fn ink_right(dl: &DisplayList) -> f32 {
    let glyphs = dl.items.iter().filter_map(|p| match p {
        Primitive::Glyph { x, size, .. } => Some(x + size * 0.75),
        Primitive::Rule { rect, .. } => Some(rect.x + rect.w),
        _ => None,
    });
    dl.hits
        .iter()
        .map(|h| h.rect.x + h.rect.w)
        .chain(glyphs)
        .fold(0.0f32, f32::max)
}

/// Like [`ink_right`], but only the formula's own ink: what is drawn left of
/// the first answer in it (an answer, or its `⋯` while it is worked out).
fn ink_right_of_formula(dl: &DisplayList) -> f32 {
    let limit = dl
        .hits
        .iter()
        .filter(|h| h.kind == HitKind::Result)
        .map(|h| h.rect.x)
        .fold(f32::INFINITY, f32::min);
    let glyphs = dl.items.iter().filter_map(|p| match p {
        Primitive::Glyph { x, size, .. } if *x < limit => Some((x + size * 0.75).min(limit)),
        Primitive::Rule { rect, .. } if rect.x < limit => Some((rect.x + rect.w).min(limit)),
        _ => None,
    });
    dl.hits
        .iter()
        .filter(|h| h.kind != HitKind::Result && h.rect.x < limit)
        .map(|h| h.rect.x + h.rect.w)
        .chain(glyphs)
        .fold(0.0f32, f32::max)
}

/// The height of one slider row under a figure.
pub fn slider_row_height(size: f32) -> f32 {
    size * 1.7
}

/// What a maths block shows besides its own lines.
#[derive(Debug, Default)]
struct Extras {
    results: Vec<LineResult>,
    /// Tables of helper results (label, value LaTeX), shown under the equation.
    tables: Vec<Vec<(String, String)>>,
    /// Worked steps, by line span.
    steps: Vec<(Span, mpp_core::step_tree::StepTree)>,
    /// Some answer is still being worked out.
    pending: bool,
    /// What to fill in before there can be an answer ("fill in: upper bound").
    hints: Vec<String>,
    /// Why there is no answer (a sum that could not be settled): prose under the
    /// equation, wrapped to the page.
    notes: Vec<Rich>,
}

/// Text that can sit inside `\text{…}` without any markup showing.
fn plain_text(s: &str) -> String {
    s.chars()
        .map(|ch| match ch {
            '\\' | '{' | '}' | '$' | '_' | '^' | '`' | '#' | '%' | '&' | '~' => ' ',
            other => other,
        })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// A step's note ("derivative of sin with u = 3·x") as LaTeX: the words in roman
/// type, the maths in it typeset. A note that is LaTeX already is kept.
pub(crate) fn prose_latex(note: &str) -> String {
    prose_latex_with(note, false)
}

/// [`prose_latex`]; `strict` keeps a word that has a run of letters (or an
/// unbalanced bracket) as words, for sentences written by the evaluator.
pub(crate) fn prose_latex_with(note: &str, strict: bool) -> String {
    // Maths the evaluator marked in the note is typeset from its own LaTeX.
    if mpp_core::note::has_maths(note) {
        let mut out = String::new();
        for piece in mpp_core::note::pieces(note) {
            match piece {
                mpp_core::note::Piece::Text(t) => {
                    if t.starts_with(' ') {
                        out.push_str("\\text{ }");
                    }
                    if !t.trim().is_empty() {
                        out.push_str(&prose_latex_with(t.trim(), strict));
                    }
                    if t.ends_with(' ') && !t.trim().is_empty() {
                        out.push_str("\\text{ }");
                    }
                }
                mpp_core::note::Piece::Maths { latex, .. } => {
                    out.push_str(&format!("{{{latex}}}"));
                }
            }
        }
        return out;
    }
    if note.contains('\\') {
        return note.to_string();
    }
    let is_maths = |w: &str| -> bool {
        let mut cs = w.chars();
        let single = matches!((cs.next(), cs.next()), (Some(c), None) if c.is_alphabetic());
        if strict {
            let letters = w
                .chars()
                .collect::<Vec<_>>()
                .windows(2)
                .any(|p| p[0].is_alphabetic() && p[1].is_alphabetic());
            let open = w.chars().filter(|c| "([{".contains(*c)).count();
            let close = w.chars().filter(|c| ")]}".contains(*c)).count();
            if letters || open != close {
                return false;
            }
        }
        single
            || w.chars()
                .any(|c| c.is_ascii_digit() || "^_=+−-*/·×()[]{}<>≤≥≠|′'√∫∑∞".contains(c))
    };
    fn flush_text(out: &mut String, text: &mut String) {
        if !text.is_empty() {
            // The spaces at the ends stay inside `\text` (maths spacing commands
            // are not drawn), so "with u" never runs together as "withu".
            let lead = if text.starts_with(' ') { " " } else { "" };
            let trail = if text.ends_with(' ') { " " } else { "" };
            out.push_str(&format!("\\text{{{lead}{}{trail}}}", plain_text(text)));
            text.clear();
        }
    }
    fn flush_maths(out: &mut String, maths: &mut Vec<String>) {
        if !maths.is_empty() {
            out.push(' ');
            out.push_str(&crate::diagnostics::rich::text_maths_latex(
                &maths.join(" "),
            ));
            out.push(' ');
            maths.clear();
        }
    }
    let mut out = String::new();
    let mut text = String::new();
    let mut maths: Vec<String> = vec![];
    for w in note.split(' ') {
        // Punctuation after a formula stays with the words.
        let (core, punct) = match w.strip_suffix([',', ';', ':']) {
            Some(c) => (c, &w[c.len()..]),
            None => (w, ""),
        };
        if !core.is_empty() && is_maths(core) {
            if !text.is_empty() {
                text.push(' ');
            }
            flush_text(&mut out, &mut text);
            maths.push(core.to_string());
            if !punct.is_empty() {
                flush_maths(&mut out, &mut maths);
                text.push_str(punct);
            }
        } else {
            if !maths.is_empty() {
                flush_maths(&mut out, &mut maths);
                text.push(' ');
            } else if !text.is_empty() {
                text.push(' ');
            }
            text.push_str(w);
        }
    }
    flush_maths(&mut out, &mut maths);
    flush_text(&mut out, &mut text);
    out.trim().to_string()
}

/// The result of the line at `offset` (a result box sits at the end of its line).
pub(crate) fn result_line_at(br: &BlockResult, offset: usize) -> Option<usize> {
    br.values
        .iter()
        .position(|v| v.span.end == offset)
        .or_else(|| {
            br.values
                .iter()
                .position(|v| v.span.start <= offset && offset <= v.span.end)
        })
}

/// The results, tables, steps and spinners of a maths block, from its evaluation.
///
/// `br` may come from an evaluation of an older text, in which the block started
/// `delta` bytes earlier: its lines are matched where the block is now.
fn extras_for(br: &BlockResult, m: &DisplayMath, show: &ShowState, delta: isize) -> Extras {
    let mut ex = Extras::default();
    for v in &br.values {
        let span = Span::new(
            v.span.start.saturating_add_signed(delta),
            v.span.end.saturating_add_signed(delta),
        );
        let Some(i) = m
            .ast
            .lines
            .iter()
            .position(|l| l.span.start <= span.start && span.end <= l.span.end)
        else {
            continue;
        };
        let (text, latex, approx) = match show.format_for(span) {
            Some(f) => v.reformat_all(f),
            None => (v.text.clone(), v.latex.clone(), v.approx.clone()),
        };
        if v.pending {
            ex.pending = true;
            ex.results.push(LineResult::ghost(i, "\\cdots"));
            continue;
        }
        if let Some(Value::Table(t)) = &v.value {
            if let Some(rows) = mpp_core::table::rows(t) {
                ex.tables.push(
                    rows.iter()
                        .map(|(label, e)| (label.clone(), e.to_latex()))
                        .collect(),
                );
                // An arrow pointing at the table under the line: a lone symbol, not
                // the relation `\downarrow`, which would ask for a box on each side.
                let arrow = mpp_syntax::math::Node::new(
                    mpp_syntax::math::NodeKind::Symbol("↓"),
                    Span::point(0),
                );
                ex.results.push(LineResult {
                    line: i,
                    value: mpp_render::ResultValue::Node(arrow),
                    kind: if v.auto {
                        ResultKind::Ghost
                    } else {
                        ResultKind::Filled
                    },
                    relation: None,
                });
                continue;
            }
        }
        match &v.kind {
            LineKind::Query { mode, steps } => {
                // An exact answer that only repeats the question (`ζ(3) = ζ(3)`)
                // gives way to its decimal, as `≈ ?` would show it.
                let decimal = *mode == QueryMode::Approx || v.shows_decimal_only();
                if let (true, Some(a)) = (decimal, &approx) {
                    ex.results.push(LineResult {
                        relation: Some(RelOp::Approx),
                        ..LineResult::filled(i, a.clone())
                    });
                } else if let Some(l) = &latex {
                    let mut r = LineResult::filled(i, l.clone());
                    if let (Some(t), Some(a)) = (&text, &approx) {
                        if t != a {
                            r = LineResult::filled(i, format!("{l} \\approx {a}"));
                        }
                    }
                    // An answer that is an equation of its own (`L(x) = 9x − 26`) follows
                    // the question with an arrow: `… = L(x) = …` would say "=" twice.
                    if matches!(v.value, Some(Value::Relation(_))) {
                        r.relation = Some(RelOp::To);
                    }
                    ex.results.push(r);
                }
                let wanted = *steps || show.show_steps;
                if let (true, Some(tree)) = (wanted, v.step_tree()) {
                    if !tree.roots.is_empty() {
                        ex.steps.push((span, tree));
                    }
                }
            }
            LineKind::Check => {
                if let Some(ver) = &v.verdict {
                    let tex = match ver.passed() {
                        Some(true) => "\\checkmark",
                        Some(false) => "\\times",
                        None => "?",
                    };
                    ex.results.push(LineResult::filled(i, tex));
                }
            }
            _ if v.auto => {
                let is_equation = matches!(v.value, Some(Value::Relation(_)))
                    || matches!(
                        v.input.as_ref().map(mpp_core::Expr::node),
                        Some(mpp_core::ExprNode::Relation(..))
                    );
                if let Some(l) = &latex {
                    ex.results.push(LineResult {
                        relation: is_equation.then_some(RelOp::Implies),
                        ..LineResult::ghost(i, l.clone())
                    });
                } else if let Some(a) = &approx {
                    ex.results.push(LineResult {
                        relation: Some(RelOp::Approx),
                        ..LineResult::ghost(i, a.clone())
                    });
                }
            }
            _ => {
                if let (Some(h), None) = (&v.hint, &v.value) {
                    if h.starts_with("fill in") {
                        let h = plain_text(&mpp_core::note::plain(h));
                        if !h.is_empty() && !ex.hints.contains(&h) {
                            ex.hints.push(h);
                        }
                    } else {
                        // Words and typeset maths: the maths in the note is laid out
                        // from its LaTeX, never shown as flattened text.
                        let note = from_message(h);
                        if !note.is_empty() && !ex.notes.contains(&note) {
                            ex.notes.push(note);
                        }
                    }
                }
            }
        }
    }
    ex
}

/// A typeset snippet in inline style.
fn snippet(latex: &str, size: f32, theme: Theme) -> DisplayList {
    let opts = LayoutOptions {
        theme,
        ..LayoutOptions::inline(size)
    };
    layout_latex(latex, &opts)
}

/// A two-column table of helper results under an equation: labels in the margin
/// colour, values typeset in the result colour. Returns its height.
fn layout_table(
    input: &PageInput<'_>,
    rows: &[(String, String)],
    x0: f32,
    y0: f32,
    w: f32,
    c: &mut Canvas,
) -> f32 {
    let size = input.size;
    let small = size * 0.95;
    let label_theme = Theme {
        text: input.theme.muted,
        ..input.theme
    };
    let value_theme = Theme {
        text: input.theme.result,
        ..input.theme
    };
    let cells: Vec<(DisplayList, DisplayList)> = rows
        .iter()
        .map(|(label, value)| {
            (
                snippet(
                    &format!("\\text{{{}}}", plain_text(label)),
                    small,
                    label_theme,
                ),
                snippet(value, small, value_theme),
            )
        })
        .collect();
    let label_w = cells.iter().map(|(l, _)| l.width).fold(0.0f32, f32::max);
    let value_w = cells.iter().map(|(_, v)| v.width).fold(0.0f32, f32::max);
    let gap = size * 1.2;
    let table_w = (label_w + gap + value_w + size).min(w);
    let left = x0 + ((w - table_w) / 2.0).max(0.0);
    let mut y = y0 + size * 0.2;
    c.items.push(Primitive::Rule {
        rect: rrect(left, y, table_w, 0.8),
        color: input.theme.rule,
    });
    y += size * 0.3;
    for (label, value) in &cells {
        let row_h = label.total_height().max(value.total_height()).max(small);
        let ly = y + (row_h - label.total_height()) / 2.0;
        let vy = y + (row_h - value.total_height()) / 2.0;
        c.merge(label, left + size * 0.5, ly, false);
        c.merge(value, left + size * 0.5 + label_w + gap, vy, false);
        y += row_h + size * 0.25;
    }
    c.items.push(Primitive::Rule {
        rect: rrect(left, y, table_w, 0.8),
        color: input.theme.rule,
    });
    y + size * 0.3 - y0
}

/// The fold line of a line's working and, when unfolded, its steps. Returns the height.
fn layout_steps(
    input: &PageInput<'_>,
    line: Span,
    tree: &mpp_core::step_tree::StepTree,
    x0: f32,
    y0: f32,
    w: f32,
    c: &mut Canvas,
) -> f32 {
    let size = input.size;
    let small = size * 0.9;
    let muted = Theme {
        text: input.theme.muted,
        ..input.theme
    };
    let expanded = input.show.is_expanded(line);
    let mut rules: Vec<String> = vec![];
    for n in &tree.roots {
        let r = plain_text(&n.rule);
        if !r.is_empty() && !rules.contains(&r) {
            rules.push(r);
        }
    }
    let head = if rules.is_empty() {
        "\\text{Working}".to_string()
    } else {
        format!("\\text{{Working: {}}}", rules.join(", "))
    };
    let left = x0 + size * 1.5;
    let head_dl = snippet(&head, small, muted);
    let mut y = y0 + size * 0.15;
    let row_h = head_dl.total_height().max(small);
    // The fold arrow: right when folded, down when open.
    let (ax, ay, s) = (left - size * 0.9, y + row_h / 2.0, size * 0.28);
    let cmds = if expanded {
        vec![
            mpp_render::PathCmd::MoveTo(ax - s, ay - s * 0.6),
            mpp_render::PathCmd::LineTo(ax + s, ay - s * 0.6),
            mpp_render::PathCmd::LineTo(ax, ay + s * 0.8),
            mpp_render::PathCmd::Close,
        ]
    } else {
        vec![
            mpp_render::PathCmd::MoveTo(ax - s * 0.6, ay - s),
            mpp_render::PathCmd::LineTo(ax + s * 0.8, ay),
            mpp_render::PathCmd::LineTo(ax - s * 0.6, ay + s),
            mpp_render::PathCmd::Close,
        ]
    };
    c.items.push(Primitive::Path {
        cmds,
        color: input.theme.muted,
        stroke: None,
    });
    c.merge(
        &head_dl,
        left,
        y + (row_h - head_dl.total_height()) / 2.0,
        false,
    );
    c.toggles.push(StepToggle {
        rect: Rect::from_min_size(
            Pos2::new(ax - s * 2.0, y),
            Vec2::new(head_dl.width + (left - ax) + s * 2.0, row_h),
        ),
        line,
    });
    y += row_h + size * 0.2;
    if expanded {
        let mut stack: Vec<(usize, &mpp_core::step_tree::StepNode)> =
            tree.roots.iter().rev().map(|n| (0, n)).collect();
        while let Some((depth, n)) = stack.pop() {
            let indent = left + depth as f32 * size * 1.2;
            let avail = (w - (indent - x0)).max(size * 4.0);
            let rule = plain_text(&n.rule);
            let latex = if rule.is_empty() {
                n.latex_line()
            } else {
                format!("\\text{{{rule}: }}\\; {}", n.latex_line())
            };
            let dl = snippet(&latex, small, input.theme);
            let scale = if dl.width > avail {
                avail / dl.width
            } else {
                1.0
            };
            let dl = if scale < 1.0 {
                snippet(&latex, small * scale.max(0.6), input.theme)
            } else {
                dl
            };
            c.merge(&dl, indent, y, false);
            y += dl.total_height().max(small) + size * 0.2;
            if let Some(note) = &n.note {
                let tex = prose_latex(note);
                let nd = snippet(&tex, small * 0.9, muted);
                c.merge(&nd, indent + size, y, false);
                y += nd.total_height().max(small * 0.9) + size * 0.2;
            }
            for child in n.children.iter().rev() {
                stack.push((depth + 1, child));
            }
        }
    }
    y - y0
}

/// Results of a helper sentence (`Analyse …`, `Find the extrema of …`): tables as
/// tables, anything else as a typeset line in the result colour. Returns the height.
fn layout_sentence_results(
    input: &PageInput<'_>,
    br: &BlockResult,
    x0: f32,
    y0: f32,
    w: f32,
    c: &mut Canvas,
) -> f32 {
    let size = input.size;
    let mut y = y0;
    let value_theme = Theme {
        text: input.theme.result,
        ..input.theme
    };
    for v in &br.values {
        if let Some(Value::Table(t)) = &v.value {
            if let Some(rows) = mpp_core::table::rows(t) {
                let rows: Vec<(String, String)> = rows
                    .iter()
                    .map(|(label, e)| (label.clone(), e.to_latex()))
                    .collect();
                y += layout_table(input, &rows, x0, y, w, c);
                continue;
            }
        }
        if let Some(l) = &v.latex {
            let dl = snippet(l, size, value_theme);
            let x = x0 + ((w - dl.width) / 2.0).max(0.0);
            c.merge(&dl, x, y + size * 0.2, false);
            y += dl.total_height().max(size) + size * 0.4;
        }
    }
    y - y0
}

/// Inline content for blocks that keep only their maths: `$…$` pairs become maths
/// and `[[file]]` links; everything else is text mapped to the source.
pub fn scan_inlines(src: &str, span: Span) -> Vec<Inline> {
    let mut out = vec![];
    let Some(text) = src.get(span.start..span.end) else {
        return out;
    };
    let bytes = text.as_bytes();
    let mut i = 0;
    let mut seg_start = 0;
    let push_text = |from: usize, to: usize, out: &mut Vec<Inline>| {
        if to > from {
            push_text_with_links(&text[from..to], span.start + from, out);
        }
    };
    while i < bytes.len() {
        if bytes[i] == b'\\' {
            i += 2;
            continue;
        }
        if bytes[i] == b'$' {
            if let Some(rel) = text[i + 1..].find('$') {
                let close = i + 1 + rel;
                push_text(seg_start, i, &mut out);
                let raw = Span::new(span.start + i + 1, span.start + close);
                let (ast, _) = parse_math(&text[i + 1..close], raw.start);
                out.push(Inline {
                    kind: InlineKind::Math(InlineMath { ast, raw }),
                    span: Span::new(span.start + i, span.start + close + 1),
                });
                i = close + 1;
                seg_start = i;
                continue;
            }
        }
        i += 1;
    }
    push_text(seg_start, text.len(), &mut out);
    out
}

fn push_text_with_links(s: &str, base: usize, out: &mut Vec<Inline>) {
    let mut rest = s;
    let mut off = base;
    while let Some(a) = rest.find("[[") {
        let Some(len) = rest[a + 2..].find("]]") else {
            break;
        };
        if a > 0 {
            out.push(Inline {
                kind: InlineKind::Text(rest[..a].to_string()),
                span: Span::new(off, off + a),
            });
        }
        let path = rest[a + 2..a + 2 + len].to_string();
        out.push(Inline {
            kind: InlineKind::WikiLink { path, alias: None },
            span: Span::new(off + a, off + a + len + 4),
        });
        off += a + len + 4;
        rest = &rest[a + len + 4..];
    }
    if !rest.is_empty() {
        out.push(Inline {
            kind: InlineKind::Text(rest.to_string()),
            span: Span::new(off, off + rest.len()),
        });
    }
}

/// `$ … $` ranges in prose blocks and `$$ … $$` bodies.
fn collect_math_ranges(src: &str, blocks: &[Block], out: &mut Vec<Span>) {
    for b in blocks {
        match &b.kind {
            BlockKind::DisplayMath(m) => out.push(m.raw),
            BlockKind::Theorem(t) => collect_math_ranges(src, &t.body, out),
            BlockKind::Verbatim(_) | BlockKind::Rule | BlockKind::Directive(_) => {}
            // A paragraph already holds its inlines: no need to read them again.
            BlockKind::Paragraph(content) => inline_math_ranges(content, out),
            BlockKind::Let(l) => inline_math_ranges(&l.content, out),
            _ => {
                for inl in scan_inlines(src, b.span) {
                    if let InlineKind::Math(m) = inl.kind {
                        out.push(m.raw);
                    }
                }
            }
        }
    }
}

fn inline_math_ranges(items: &[Inline], out: &mut Vec<Span>) {
    for i in items {
        match &i.kind {
            InlineKind::Math(m) => out.push(m.raw),
            InlineKind::Emphasis(inner)
            | InlineKind::Strong(inner)
            | InlineKind::Link { text: inner, .. } => inline_math_ranges(inner, out),
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mpp_syntax::document::parse_document;

    #[test]
    fn step_notes_are_words_with_maths_in_them() {
        let tex = prose_latex("derivative of sin with u = 3·x");
        assert!(tex.starts_with(r"\text{derivative of sin with }"), "{tex}");
        assert!(tex.contains("u = 3·x"), "{tex}");
        let dl = mpp_render::layout_latex(&tex, &mpp_render::LayoutOptions::display(18.0));
        // The words are upright Latin letters, not maths italics.
        assert!(dl.text().contains("derivativeofsinwith"), "{}", dl.text());
        // A visible space between the last word and the maths ("with u", not "withu").
        let glyphs: Vec<(char, f32, f32)> = dl
            .items
            .iter()
            .filter_map(|p| match p {
                mpp_render::Primitive::Glyph { ch, x, size, .. } => Some((*ch, *x, *size)),
                _ => None,
            })
            .collect();
        let h = glyphs.iter().rposition(|g| g.0 == 'h').expect("h of with");
        let (_, hx, size) = glyphs[h];
        let (_, ux, _) = glyphs[h + 1];
        assert!(ux - hx > 0.75 * size, "gap {} at size {size}", ux - hx);
        assert_eq!(
            prose_latex("power rule on u^2 with u = x + 1"),
            r"\text{power rule on } u^{2} \text{ with } u = x + 1"
        );
    }

    /// The Insert menu's proof puts the caret between the heading and the ∎; it
    /// is drawn there, not back at the start of the block.
    #[test]
    fn the_caret_of_an_empty_proof_sits_after_its_heading() {
        let src = "# P\n\n**Proof.**  ∎";
        let page = page_for(src);
        let at = page.caret_stop(16).expect("a stop");
        let heading_end = page
            .stops
            .iter()
            .filter(|s| s.offset == 17)
            .map(|s| s.x)
            .fold(f32::MIN, f32::max);
        assert!(at.x > 20.0, "{at:?}");
        assert_eq!(at.x, heading_end, "{:?}", page.stops);
    }

    /// The result boxes of the first maths block of `src`, evaluated.
    fn results_of(src: &str) -> Vec<(String, Option<RelOp>)> {
        let mut session = mpp_eval::Session::new();
        let ev = session.update(src).clone();
        let b = ev
            .document
            .blocks
            .iter()
            .find(|b| matches!(b.kind, BlockKind::DisplayMath(_)))
            .expect("maths block");
        let BlockKind::DisplayMath(m) = &b.kind else {
            unreachable!()
        };
        let br = ev.block(b.id).expect("evaluated");
        extras_for(br, m, &ShowState::default(), 0)
            .results
            .into_iter()
            .map(|r| match r.value {
                mpp_render::ResultValue::Latex(l) => (l, r.relation),
                mpp_render::ResultValue::Node(_) => (String::new(), r.relation),
            })
            .collect()
    }

    #[test]
    fn an_unsimplified_special_value_shows_its_decimal_not_itself() {
        // `ζ(3) = ζ(3)` says nothing: the decimal takes the exact answer's place.
        let r = results_of("$$ \\zeta(3) = ? $$\n");
        assert_eq!(r, vec![("1.202056903".to_string(), Some(RelOp::Approx))]);
        let r = results_of("$$ J_0(1) = ? $$\n");
        assert_eq!(r, vec![("0.7651976866".to_string(), Some(RelOp::Approx))]);
        // A real closed form is still the exact answer, with its decimal beside it.
        let r = results_of("$$ \\zeta(2) = ? $$\n");
        assert_eq!(r.len(), 1);
        assert!(r[0].0.starts_with(r"\frac{\pi^{2}}{6}"), "{r:?}");
        assert_eq!(r[0].1, None);
    }

    fn page_for(src: &str) -> Page {
        let doc = parse_document(src);
        build_page(&PageInput {
            src,
            doc: &doc,
            eval: None,
            theme: Theme::LIGHT,
            size: 18.0,
            width: 900.0,
            integral_limits: IntegralLimits::AboveBelow,
            slot_labels: &[],
            show: &ShowState::default(),
            extra_refs: &ExtraRefs::default(),
            proof: &crate::proof::ProofView::default(),
        })
    }

    #[test]
    fn heading_markup_is_never_a_caret_position() {
        let src = "# Title\n\nbody text\n";
        let page = page_for(src);
        // `# ` occupies offsets 0 and 1; the first visible caret stop is at 2.
        let first = page.stops.iter().map(|s| s.offset).min();
        assert_eq!(first, Some(2));
        assert_eq!(page.caret_stop(0).map(|s| s.offset), Some(2));
        assert!(page
            .chars
            .iter()
            .all(|c| !src[c.start..c.end].contains('#')));
    }

    #[test]
    fn clicking_prose_maps_to_the_character_boundary() {
        let src = "Hello world\n";
        let page = page_for(src);
        let stop = page.stops.iter().find(|s| s.offset == 6).expect("stop");
        let at = Pos2::new(stop.x + 0.5, (stop.top + stop.bottom) / 2.0);
        assert_eq!(page.offset_at(at), Some(6));
    }

    #[test]
    fn maths_offsets_map_to_stops_inside_the_dollars() {
        let src = "$$ x + y $$\n";
        let page = page_for(src);
        assert_eq!(page.math_ranges.len(), 1);
        let plus = src.find('+').expect("plus");
        let stop = page.caret_stop(plus).expect("stop");
        assert!(stop.x > 0.0);
        assert!(page.in_math(plus));
        assert!(!page.in_math(src.len()));
        // Nothing but maths glyphs is drawn: no `$`.
        let shown: String = page.blocks.iter().map(|b| b.dl.text()).collect();
        assert!(!shown.contains('$'));
    }

    #[test]
    fn inline_maths_in_prose_is_in_a_maths_range() {
        let src = "Let $a$ be big.\n";
        let page = page_for(src);
        let a = src.find('a').expect("a");
        assert!(page.in_math(a));
        assert!(!page.in_math(0));
    }

    #[test]
    fn a_space_typed_at_the_end_of_a_line_keeps_the_caret_on_that_line() {
        // Ben's video: after typing "#This is " (the space before the next word)
        // the caret was drawn at the start of the line below.
        for src in [
            "# This is ",
            "This is ",
            "x := 5 ",
            "# This is  ",
            "Hello

This is ",
        ] {
            let page = page_for(src);
            let last = src.trim_end().len();
            let word = page.caret_stop(last).expect("stop after the last letter");
            let end = page.caret_stop(src.len()).expect("stop after the space");
            assert!(
                (end.top - word.top).abs() < 1.0,
                "{src:?}: the caret after the space is on another line ({} vs {})",
                end.top,
                word.top
            );
            assert!(
                end.x > word.x,
                "{src:?}: the caret after the space moved left"
            );
        }
    }

    fn top_of(page: &Page, offset: usize) -> f32 {
        page.caret_stop(offset).expect("a stop").top
    }

    #[test]
    fn a_new_empty_line_has_its_own_stop_where_its_text_will_go() {
        // Enter at the end of a line (the editor writes "\n\n"), then a letter.
        for (empty, typed) in [
            ("Hello\n\n", "Hello\n\nx"),
            ("# Title\n\n", "# Title\n\nx"),
            ("Hello\n\n\n\nWorld", "Hello\n\nx\n\nWorld"),
            ("\n\nWorld", "x\n\nWorld"),
            ("", "x"),
        ] {
            let at = empty.find("\n\n").map_or(0, |i| i + 2);
            let at = if empty.starts_with('\n') { 0 } else { at };
            let a = page_for(empty);
            let b = page_for(typed);
            let s = a.stops.iter().find(|s| s.offset == at);
            let s = s.unwrap_or_else(|| panic!("{empty:?}: no stop on the new line"));
            let x = top_of(&b, at + 1);
            assert!(
                (s.top - x).abs() < 1.0,
                "{empty:?}: the empty line's caret is at {} but its text goes at {x}",
                s.top
            );
            if let Some(first) = a.stops.iter().find(|o| o.offset < at) {
                assert!(s.top > first.top + 5.0, "{empty:?}: not on a line below");
            }
        }
    }

    #[test]
    fn empty_lines_push_the_blocks_below_them_down() {
        let one = page_for("Hello\n\nWorld");
        let two = page_for("Hello\n\n\n\nWorld");
        let w = |p: &Page, src: &str| top_of(p, src.find('W').expect("W"));
        assert!(w(&two, "Hello\n\n\n\nWorld") > w(&one, "Hello\n\nWorld") + 18.0);
    }

    #[test]
    fn the_blank_line_between_two_blocks_has_no_caret_stop() {
        let src = "Hello\n\nWorld";
        let page = page_for(src);
        assert!(page.stops.iter().all(|s| s.offset != 6), "{:?}", page.stops);
        // An offset there sticks to the line before, never to the next block.
        assert!((top_of(&page, 6) - top_of(&page, 5)).abs() < 1.0);
        // Left from the start of "World" goes to the end of "Hello".
        assert_eq!(page.offset_step(7, false), Some(5));
        assert_eq!(page.offset_step(5, true), Some(7));
    }

    use super::super::Side;

    fn page_showing(src: &str, show: &ShowState) -> Page {
        let doc = parse_document(src);
        build_page(&PageInput {
            src,
            doc: &doc,
            eval: None,
            theme: Theme::LIGHT,
            size: 18.0,
            width: 900.0,
            integral_limits: IntegralLimits::AboveBelow,
            slot_labels: &[],
            show,
            extra_refs: &ExtraRefs::default(),
            proof: &crate::proof::ProofView::default(),
        })
    }

    #[test]
    fn pin_shift_is_how_far_the_caret_line_moved() {
        // A note above the caret takes space or none (View → Comments): the
        // text is the same, only the layout changed.
        let src = "% a note above the line

Hello

World";
        let shown = page_showing(src, &ShowState::default());
        let hidden = page_showing(
            src,
            &ShowState {
                hide_comments: true,
                ..ShowState::default()
            },
        );
        let caret = src.len();
        let moved = top_of(&shown, caret) - top_of(&hidden, caret);
        assert!(moved > 5.0, "the note takes space: {moved}");
        let shift = super::super::pin::pin_shift(&hidden, &shown, src, caret).expect("a shift");
        assert!((shift - moved).abs() < 0.5, "{shift} vs {moved}");
        let back = super::super::pin::pin_shift(&shown, &hidden, src, caret).expect("a shift");
        assert!((back + moved).abs() < 0.5);
        // Typing on the pinned line keeps the pin; nothing moved.
        let typed = format!("{src}s");
        let pin = super::super::Pin::new(&shown, src, caret, Side::Before).expect("a pin");
        let s = pin.shift(&page_for(&typed), &typed).expect("still pinned");
        assert!(s.abs() < 0.5, "{s}");
        // Text changed before the line: no pin (the caret is scrolled to instead).
        let other = format!("x{src}");
        assert_eq!(pin.shift(&page_for(&other), &other), None);
    }

    #[test]
    fn the_caret_side_picks_the_line_when_an_offset_is_on_two() {
        let src = "Hello

World";
        let page = page_for(src);
        // Offset 6 (the blank line between the blocks) has no stop of its own.
        let before = page.caret_stop_on(6, Side::Before).expect("stop");
        let after = page.caret_stop_on(6, Side::After).expect("stop");
        assert!((before.top - top_of(&page, 5)).abs() < 1.0);
        assert!((after.top - top_of(&page, 7)).abs() < 1.0);
        assert!(after.top > before.top + 5.0);
    }

    /// The document from the video: a heading, prose, maths, an empty line.
    const ARROWS: &str = "# Title\n\nE = 2x + 5\n\n$$ x := 5 $$\n\n\n\nlast";

    /// Every place Right reaches from the start of `src`, then every place
    /// Left reaches back from the end.
    type Places = Vec<(usize, Side)>;

    fn walk(page: &Page) -> (Places, Places) {
        let (lo, hi) = page.offset_extent().expect("stops");
        let mut fwd = vec![(lo, Side::Before)];
        while let Some(n) = page.step_on(fwd[fwd.len() - 1].0, fwd[fwd.len() - 1].1, true) {
            fwd.push(n);
            assert!(fwd.len() < 500, "Right never reached the end");
        }
        let mut back = vec![(hi, Side::Before)];
        while let Some(n) = page.step_on(back[back.len() - 1].0, back[back.len() - 1].1, false) {
            back.push(n);
            assert!(back.len() < 500, "Left never reached the start");
        }
        assert_eq!(fwd.last().map(|s| s.0), Some(hi));
        assert_eq!(back.last().map(|s| s.0), Some(lo));
        (fwd, back)
    }

    #[test]
    fn arrows_never_stop_in_the_gap_between_blocks() {
        let src = ARROWS;
        let page = page_for(src);
        let (fwd, back) = walk(&page);
        let doc = parse_document(src);
        // A place is on a block, or on an empty line a person sees.
        let shown = |o: usize| {
            if doc
                .blocks
                .iter()
                .any(|b| b.span.start <= o && o <= b.span.end)
            {
                return true;
            }
            super::super::blank::on_visible_line(src, o)
        };
        for (o, _) in fwd.iter().chain(&back) {
            assert!(
                shown(*o),
                "the caret stopped at {o}, in a gap between blocks"
            );
        }
        // Each drawn place is visited once each way, at a different position.
        let pos = |(o, side): &(usize, Side)| page.caret_stop_on(*o, *side).expect("stop");
        for w in fwd.windows(2) {
            let (a, b) = (pos(&w[0]), pos(&w[1]));
            assert!(
                (a.x - b.x).abs() >= 0.5 || (a.top - b.top).abs() >= 1.0,
                "Right from {:?} to {:?} did not move",
                w[0],
                w[1]
            );
        }
        assert_eq!(fwd.len(), back.len(), "{fwd:?}\n{back:?}");
    }

    #[test]
    fn left_at_the_start_of_a_line_goes_to_the_end_of_the_line_above() {
        let src = ARROWS;
        let page = page_for(src);
        let e = src.find('E').expect("E");
        let title_end = src.find("\n\n").expect("end of the title");
        assert_eq!(
            page.step_on(e, Side::Before, false).map(|s| s.0),
            Some(title_end)
        );
        assert_eq!(
            page.step_on(title_end, Side::Before, true).map(|s| s.0),
            Some(e)
        );
        // From the first character of the maths to the end of the prose line.
        let five = src.find("5\n").expect("5") + 1;
        let x = src.find("x :=").expect("x");
        let first_maths = page
            .stops
            .iter()
            .filter(|s| s.offset > five)
            .map(|s| s.offset)
            .min()
            .expect("maths");
        assert!(first_maths <= x, "{first_maths} {x}");
        assert_eq!(
            page.step_on(first_maths, Side::Before, false).map(|s| s.0),
            Some(five)
        );
        // The empty line between the maths and "last" is a place of its own.
        let last = src.find("last").expect("last");
        let blank = last - 2;
        assert_eq!(
            page.step_on(last, Side::Before, false).map(|s| s.0),
            Some(blank)
        );
        let maths_end = page.step_on(blank, Side::Before, false).expect("maths").0;
        assert!(maths_end < blank - 2, "{maths_end}");
    }

    #[test]
    fn up_and_down_keep_the_column_across_lines_of_different_heights() {
        let src = "Hello world, this is prose\n\n$$ \\frac{a}{b} + c = d $$\n\nAnother line of prose here";
        let page = page_for(src);
        let start = src.find("world").expect("world") + 2;
        let want = page.caret_stop(start).expect("stop").x;
        let (down1, side) = page
            .vertical_on(start, Side::Before, false, Some(want))
            .expect("a line below");
        let mut at = (down1, side);
        // Down until the last line, never more than a few steps.
        for _ in 0..4 {
            match page.vertical_on(at.0, at.1, false, Some(want)) {
                Some(n) => at = n,
                None => break,
            }
        }
        let bottom = page.caret_stop_on(at.0, at.1).expect("stop");
        let last_line = src.find("Another").expect("last line");
        assert!(at.0 >= last_line, "ended at {} above the last line", at.0);
        assert!(
            (bottom.x - want).abs() < 12.0,
            "the column moved from {want} to {}",
            bottom.x
        );
        // And back up to where it started.
        let mut up = at;
        while let Some(n) = page.vertical_on(up.0, up.1, true, Some(want)) {
            up = n;
        }
        assert_eq!(up.0, start);
    }

    #[test]
    fn home_and_end_go_to_the_ends_of_the_line() {
        let src = ARROWS;
        let page = page_for(src);
        let x2 = src.find("2x").expect("2x");
        let (lo, hi) = page.line_extent_on(x2, Side::Before).expect("line");
        assert_eq!(lo, src.find('E').expect("E"));
        assert_eq!(hi, src.find("5\n").expect("5") + 1);
        assert_eq!(page.offset_extent(), Some((2, src.len())));
    }

    #[test]
    fn an_empty_document_still_has_a_caret_position() {
        let page = page_for("");
        assert_eq!(page.caret_stop(0).map(|s| s.offset), Some(0));
    }

    #[test]
    fn sentence_blocks_keep_their_maths() {
        let src = "Using $g$ from [[lib.mpp]].\n";
        let inl = scan_inlines(src, Span::new(0, src.len() - 1));
        assert!(inl.iter().any(|i| matches!(i.kind, InlineKind::Math(_))));
        assert!(inl
            .iter()
            .any(|i| matches!(i.kind, InlineKind::WikiLink { .. })));
    }
}
