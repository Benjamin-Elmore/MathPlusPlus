//! The block layer: splits the file into lines and reads each block in turn.
//!
//! Every block is recognised from its first line, and a block that goes wrong
//! (an unclosed `$$`, a code fence with no end) reports an error and stops at a
//! point the rest of the document can continue from.

use std::collections::HashMap;

use super::ast::*;
use super::figure::parse_figure;
use super::inline::parse_inlines;
use super::list::{build_lists, marker, Flat};
use super::sentence::parse_sentence;
use super::table::{parse_table, rule_aligns};
use crate::math::parse_math;
use crate::span::{Diagnostic, Span};

/// How deeply theorem bodies may nest.
const MAX_BLOCK_DEPTH: usize = 4;

/// One line of the source, without its line ending.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Line {
    pub start: usize,
    pub end: usize,
}

/// Splits `src` into lines (`\n` or `\r\n` endings).
pub(crate) fn split_lines(src: &str) -> Vec<Line> {
    let mut lines = Vec::new();
    let mut start = 0;
    for piece in src.split('\n') {
        let mut end = start + piece.len();
        if piece.ends_with('\r') {
            end -= 1;
        }
        lines.push(Line { start, end });
        start += piece.len() + 1;
    }
    // A final newline does not start another line.
    if src.ends_with('\n') {
        lines.pop();
    }
    lines
}

/// State shared by every block of one document.
pub(crate) struct Ctx<'a> {
    pub src: &'a str,
    pub diags: Vec<Diagnostic>,
    eq_counter: u32,
    pub labels: HashMap<String, u32>,
    /// The `%noeval` line waiting for a maths block.
    pending_noeval: Option<Span>,
}

impl<'a> Ctx<'a> {
    pub fn new(src: &'a str) -> Self {
        Ctx {
            src,
            diags: Vec::new(),
            eq_counter: 0,
            labels: HashMap::new(),
            pending_noeval: None,
        }
    }

    /// Reports a `%noeval` that no maths block followed.
    pub fn finish(&mut self) {
        self.dangling_noeval();
    }

    fn dangling_noeval(&mut self) {
        if let Some(span) = self.pending_noeval.take() {
            self.diags.push(Diagnostic::warning(
                span,
                "`%noeval` has no maths block right after it",
            ));
        }
    }

    fn text(&self, span: Span) -> &'a str {
        self.src.get(span.start..span.end).unwrap_or("")
    }

    fn line_text(&self, line: Line) -> &'a str {
        self.text(Span::new(line.start, line.end))
    }
}

fn block(kind: BlockKind, span: Span) -> Block {
    Block {
        kind,
        span,
        hash: 0,
        id: BlockId(0),
    }
}

/// The text of a line without leading whitespace, and where it starts.
fn trimmed<'a>(ctx: &Ctx<'a>, line: Line) -> (&'a str, usize) {
    let text = ctx.line_text(line);
    let t = text.trim_start();
    (t, line.start + (text.len() - t.len()))
}

fn is_blank(ctx: &Ctx<'_>, line: Line) -> bool {
    ctx.line_text(line).trim().is_empty()
}

/// Reads the blocks in `lines`.
pub(crate) fn parse_blocks(ctx: &mut Ctx<'_>, lines: &[Line], depth: usize) -> Vec<Block> {
    let mut blocks = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        let line = lines[i];
        if is_blank(ctx, line) {
            i += 1;
            continue;
        }
        let before = blocks.len();
        let next = block_at_line(ctx, lines, i, depth, &mut blocks);
        // Always make progress, whatever the block reader did.
        i = next.max(i + 1);

        for b in blocks.iter().skip(before) {
            match &b.kind {
                BlockKind::Directive(d) if d.kind == DirectiveKind::NoEval => {
                    ctx.dangling_noeval();
                    ctx.pending_noeval = Some(b.span);
                }
                BlockKind::Directive(_) | BlockKind::DisplayMath(_) => {}
                _ => ctx.dangling_noeval(),
            }
        }
    }
    blocks
}

/// Reads the block that starts at `lines[i]`, pushes it, and returns the
/// index of the next unread line.
fn block_at_line(
    ctx: &mut Ctx<'_>,
    lines: &[Line],
    i: usize,
    depth: usize,
    out: &mut Vec<Block>,
) -> usize {
    let line = lines[i];
    let (t, ts) = trimmed(ctx, line);

    if t.starts_with("```") {
        if let Some(next) = verbatim(ctx, lines, i, out) {
            return next;
        }
    }
    if t.starts_with('%') {
        out.push(directive(t, ts, line));
        return i + 1;
    }
    if let Some((level, offset)) = heading_level(t) {
        let content = trim_range(ctx, Span::new(ts + offset, line.end));
        let inlines = parse_inlines(ctx.src, content, &mut ctx.diags);
        out.push(block(
            BlockKind::Heading {
                level,
                content: inlines,
            },
            Span::new(ts, content.end.max(ts + offset)),
        ));
        return i + 1;
    }
    if is_rule(t) {
        out.push(block(
            BlockKind::Rule,
            Span::new(ts, ts + t.trim_end().len()),
        ));
        return i + 1;
    }
    if t.starts_with("$$") {
        return display_math(ctx, lines, i, out);
    }
    if t.starts_with("Figure:") {
        let rest_span = Span::new(ts + "Figure:".len(), line.end);
        let figure = parse_figure(ctx.src, rest_span, &mut ctx.diags);
        out.push(block(
            BlockKind::Figure(figure),
            Span::new(ts, ts + t.trim_end().len()),
        ));
        return i + 1;
    }
    if t.starts_with("![[") {
        if let Some(b) = embed(ctx, t, ts) {
            out.push(b);
            return i + 1;
        }
    }
    if let Some((name, offset)) = footnote_def(t) {
        return footnote(ctx, lines, i, name, ts + offset, ts, out);
    }
    if depth < MAX_BLOCK_DEPTH {
        if let Some((kind, prefix)) = theorem_start(t) {
            return theorem(ctx, lines, i, kind, ts, ts + prefix, depth, out);
        }
    }
    if t.starts_with('>') {
        return quote(ctx, lines, i, out);
    }
    if t.starts_with('|') && table_rule_at(ctx, lines, i + 1).is_some() {
        return table(ctx, lines, i, out);
    }
    if marker(ctx.line_text(line), line.start).is_some() {
        return list(ctx, lines, i, out);
    }
    paragraph(ctx, lines, i, out)
}

// ---- single-line recognisers --------------------------------------------

fn heading_level(t: &str) -> Option<(u8, usize)> {
    let hashes = t.bytes().take_while(|&b| b == b'#').count();
    if hashes == 0 || hashes > 6 {
        return None;
    }
    let after = &t[hashes..];
    if after.is_empty() {
        return Some((hashes as u8, hashes));
    }
    let gap = after.len() - after.trim_start_matches([' ', '\t']).len();
    (gap > 0).then_some((hashes as u8, hashes + gap))
}

fn is_rule(t: &str) -> bool {
    let t = t.trim_end();
    t.len() >= 3 && t.bytes().all(|b| b == b'-')
}

fn footnote_def(t: &str) -> Option<(&str, usize)> {
    let rest = t.strip_prefix("[^")?;
    let close = rest.find("]:")?;
    let name = &rest[..close];
    let valid = !name.is_empty()
        && name
            .chars()
            .all(|c| c.is_alphanumeric() || c == '_' || c == '-');
    if !valid {
        return None;
    }
    let after = &rest[close + 2..];
    let gap = after.len() - after.trim_start().len();
    Some((name, 2 + close + 2 + gap))
}

fn theorem_start(t: &str) -> Option<(TheoremKind, usize)> {
    let rest = t.strip_prefix("**")?;
    TheoremKind::ALL.iter().find_map(|&kind| {
        let after = rest.strip_prefix(kind.as_str())?.strip_prefix(".**")?;
        let gap = after.len() - after.trim_start().len();
        Some((kind, 2 + kind.as_str().len() + 3 + gap))
    })
}

/// The span of the table rule on `lines[k]`, if that line is one.
fn table_rule_at(ctx: &Ctx<'_>, lines: &[Line], k: usize) -> Option<Vec<Align>> {
    let line = lines.get(k)?;
    rule_aligns(ctx.src, Span::new(line.start, line.end))
}

fn trim_range(ctx: &Ctx<'_>, span: Span) -> Span {
    let s = ctx.text(span);
    let t = s.trim_start();
    let start = span.start + (s.len() - t.len());
    Span::new(start, start + t.trim_end().len())
}

/// True when `lines[k]` ends the paragraph that started above it.
fn interrupts(ctx: &Ctx<'_>, lines: &[Line], k: usize) -> bool {
    let line = lines[k];
    let (t, _) = trimmed(ctx, line);
    if t.starts_with("```")
        || t.starts_with("$$")
        || t.starts_with('%')
        || t.starts_with('>')
        || t.starts_with("Figure:")
        || t.starts_with("![[")
        || heading_level(t).is_some()
        || is_rule(t)
        || footnote_def(t).is_some()
        || theorem_start(t).is_some()
        || starts_sentence(ctx, line)
    {
        return true;
    }
    if t.starts_with('|') && table_rule_at(ctx, lines, k + 1).is_some() {
        return true;
    }
    // Only a bullet or an item numbered 1 may interrupt a paragraph.
    marker(ctx.line_text(line), line.start).is_some_and(|m| !m.ordered || m.number == Some(1))
}

// ---- blocks ----------------------------------------------------------------

fn directive(t: &str, ts: usize, line: Line) -> Block {
    let text = t[1..].trim().to_string();
    let kind = if text == "noeval" {
        DirectiveKind::NoEval
    } else {
        DirectiveKind::Comment
    };
    block(
        BlockKind::Directive(Directive { kind, text }),
        Span::new(ts, ts + t.trim_end().len().min(line.end - ts)),
    )
}

fn verbatim(ctx: &mut Ctx<'_>, lines: &[Line], i: usize, out: &mut Vec<Block>) -> Option<usize> {
    let line = lines[i];
    let (t, ts) = trimmed(ctx, line);
    let fence = t.bytes().take_while(|&b| b == b'`').count();
    let info = t[fence..].trim();
    if info.contains('`') {
        // Inline code that happens to start the line.
        return None;
    }
    let close = (i + 1..lines.len()).find(|&k| {
        let (ct, _) = trimmed(ctx, lines[k]);
        let ticks = ct.bytes().take_while(|&b| b == b'`').count();
        ticks >= fence && ct[ticks..].trim().is_empty()
    });
    let (text_span, closed, end, next) = match close {
        Some(c) => {
            let (cs, ce) = if c == i + 1 {
                (lines[c].start, lines[c].start)
            } else {
                (lines[i + 1].start, lines[c - 1].end)
            };
            (Span::new(cs, ce), true, lines[c].end, c + 1)
        }
        None => {
            ctx.diags.push(
                Diagnostic::error(Span::new(ts, ts + fence), "this code fence is never closed")
                    .expecting(["a closing ```"]),
            );
            let last = lines[lines.len() - 1];
            let cs = lines.get(i + 1).map_or(last.end, |l| l.start);
            (Span::new(cs, last.end), false, last.end, lines.len())
        }
    };
    out.push(block(
        BlockKind::Verbatim(Verbatim {
            info: info.to_string(),
            text: ctx.text(text_span).to_string(),
            text_span,
            closed,
        }),
        Span::new(ts, end),
    ));
    Some(next)
}

fn embed(ctx: &mut Ctx<'_>, t: &str, ts: usize) -> Option<Block> {
    let body = t.trim_end();
    let inner = body.strip_prefix("![[")?.strip_suffix("]]")?;
    let path = inner.trim();
    if path.is_empty() || inner.contains("]]") {
        ctx.diags.push(
            Diagnostic::error(
                Span::new(ts, ts + body.len()),
                "an import needs a file path",
            )
            .expecting(["`![[file.mpp]]`"]),
        );
        return None;
    }
    let lead = inner.len() - inner.trim_start().len();
    let path_start = ts + 3 + lead;
    Some(block(
        BlockKind::EmbedImport {
            path: path.to_string(),
            path_span: Span::new(path_start, path_start + path.len()),
        },
        Span::new(ts, ts + body.len()),
    ))
}

fn quote(ctx: &mut Ctx<'_>, lines: &[Line], i: usize, out: &mut Vec<Block>) -> usize {
    let mut k = i;
    let mut paragraphs: Vec<Vec<Inline>> = Vec::new();
    let mut current: Vec<Inline> = Vec::new();
    let mut end = lines[i].end;
    let start = trimmed(ctx, lines[i]).1;
    while k < lines.len() {
        let (t, ts) = trimmed(ctx, lines[k]);
        let Some(rest) = t.strip_prefix('>') else {
            break;
        };
        let skip = 1 + usize::from(rest.starts_with(' '));
        let content = trim_range(ctx, Span::new(ts + skip.min(t.len()), lines[k].end));
        end = lines[k].end;
        if content.is_empty() {
            if !current.is_empty() {
                paragraphs.push(std::mem::take(&mut current));
            }
        } else {
            if !current.is_empty() {
                current.push(Inline {
                    kind: InlineKind::SoftBreak,
                    span: Span::point(content.start),
                });
            }
            current.extend(parse_inlines(ctx.src, content, &mut ctx.diags));
        }
        k += 1;
    }
    if !current.is_empty() {
        paragraphs.push(current);
    }
    out.push(block(
        BlockKind::Quote(paragraphs),
        Span::new(start, end.max(start)),
    ));
    k
}

fn table(ctx: &mut Ctx<'_>, lines: &[Line], i: usize, out: &mut Vec<Block>) -> usize {
    let header = Span::new(lines[i].start, lines[i].end);
    let aligns = table_rule_at(ctx, lines, i + 1).unwrap_or_default();
    let mut body = Vec::new();
    let mut k = i + 2;
    while k < lines.len() && trimmed(ctx, lines[k]).0.starts_with('|') {
        body.push(Span::new(lines[k].start, lines[k].end));
        k += 1;
    }
    let table = parse_table(ctx.src, header, aligns, &body, &mut ctx.diags);
    let start = trimmed(ctx, lines[i]).1;
    let end = lines[k - 1].end;
    out.push(block(BlockKind::Table(table), Span::new(start, end)));
    k
}

fn list(ctx: &mut Ctx<'_>, lines: &[Line], i: usize, out: &mut Vec<Block>) -> usize {
    let mut flat: Vec<Flat> = Vec::new();
    let mut k = i;
    while k < lines.len() {
        let line = lines[k];
        if is_blank(ctx, line) {
            break;
        }
        if let Some(m) = marker(ctx.line_text(line), line.start) {
            flat.push(Flat {
                marker: m,
                end: trim_range(ctx, Span::new(m.content.min(line.end), line.end)).end,
            });
        } else if interrupts(ctx, lines, k) {
            break;
        } else if let Some(last) = flat.last_mut() {
            // A wrapped line continues the item above.
            last.end = trim_range(ctx, Span::new(line.start, line.end)).end;
        }
        k += 1;
    }
    for (list, span) in build_lists(ctx.src, &flat, &mut ctx.diags) {
        out.push(block(BlockKind::List(list), span));
    }
    k
}

fn footnote(
    ctx: &mut Ctx<'_>,
    lines: &[Line],
    i: usize,
    name: &str,
    content_start: usize,
    start: usize,
    out: &mut Vec<Block>,
) -> usize {
    let mut k = i + 1;
    while k < lines.len() && !is_blank(ctx, lines[k]) && !interrupts(ctx, lines, k) {
        k += 1;
    }
    let content = trim_range(
        ctx,
        Span::new(content_start.min(lines[k - 1].end), lines[k - 1].end),
    );
    let inlines = parse_inlines(ctx.src, content, &mut ctx.diags);
    out.push(block(
        BlockKind::FootnoteDef {
            name: name.to_string(),
            content: inlines,
        },
        Span::new(start, content.end.max(start)),
    ));
    k
}

fn paragraph(ctx: &mut Ctx<'_>, lines: &[Line], i: usize, out: &mut Vec<Block>) -> usize {
    let mut k = i + 1;
    // A sentence form on its own line is a block of its own.
    let single = starts_sentence(ctx, lines[i]);
    while !single && k < lines.len() && !is_blank(ctx, lines[k]) && !interrupts(ctx, lines, k) {
        k += 1;
    }
    let first = trimmed(ctx, lines[i]).1;
    let range = trim_range(ctx, Span::new(first, lines[k - 1].end));

    // The paragraph as one sentence form.
    let mut local = Vec::new();
    if let Some(kind) = parse_sentence(ctx.src, range, &mut local) {
        ctx.diags.extend(local);
        out.push(block(kind, range));
        return k;
    }
    let inlines = parse_inlines(ctx.src, range, &mut ctx.diags);
    out.push(block(BlockKind::Paragraph(inlines), range));
    k
}

/// True when the line is a complete `Using`, helper, `Analyse` or `Solve` sentence.
fn starts_sentence(ctx: &Ctx<'_>, line: Line) -> bool {
    let (t, ts) = trimmed(ctx, line);
    if t.is_empty() {
        return false;
    }
    let mut scratch = Vec::new();
    let span = Span::new(ts, ts + t.trim_end().len());
    !matches!(
        parse_sentence(ctx.src, span, &mut scratch),
        None | Some(BlockKind::Let(_))
    )
}

// ---- display maths ---------------------------------------------------------

/// True when the line that starts at `p` holds only whitespace.
fn blank_line_at(src: &str, p: usize, limit: usize) -> bool {
    let rest = src.get(p..limit).unwrap_or("");
    rest.split('\n').next().is_some_and(|l| l.trim().is_empty())
}

fn display_math(ctx: &mut Ctx<'_>, lines: &[Line], i: usize, out: &mut Vec<Block>) -> usize {
    let src = ctx.src;
    let bytes = src.as_bytes();
    let open = trimmed(ctx, lines[i]).1;
    let limit = lines[lines.len() - 1].end;
    let content_start = open + 2;

    let mut k = content_start;
    let mut close = None;
    let mut stop = limit;
    while k < limit {
        match bytes.get(k) {
            Some(b'\\') if bytes.get(k + 1) != Some(&b'\n') => k += 2,
            Some(b'$') if bytes.get(k + 1) == Some(&b'$') && k + 1 < limit => {
                close = Some(k);
                break;
            }
            Some(b'\n') => {
                if blank_line_at(src, k + 1, limit) {
                    stop = k;
                    break;
                }
                k += 1;
            }
            _ => k += 1,
        }
    }

    let (content_end, line_of_end, rest_start) = match close {
        Some(c) => (c, c, Some(c + 2)),
        None => (stop, stop.saturating_sub(1).max(open), None),
    };
    let content_end = content_end.max(content_start);
    let m = lines
        .partition_point(|l| l.start <= line_of_end)
        .saturating_sub(1)
        .max(i);
    let block_end = lines[m].end.max(content_end);

    let raw = Span::new(content_start, content_end);
    let closed = close.is_some();
    if !closed {
        ctx.diags.push(
            Diagnostic::error(Span::new(open, open + 2), "this `$$` is never closed")
                .expecting(["a closing `$$`"]),
        );
    }

    // `(n)` after the closing `$$`.
    let mut explicit = None;
    if let Some(rs) = rest_start {
        let rest_span = Span::new(rs.min(block_end), block_end);
        let rest = ctx.text(rest_span).trim();
        if !rest.is_empty() {
            let digits = rest
                .strip_prefix('(')
                .and_then(|r| r.strip_suffix(')'))
                .map(str::trim)
                .filter(|d| !d.is_empty() && d.bytes().all(|b| b.is_ascii_digit()));
            match digits.and_then(|d| d.parse::<u32>().ok()) {
                Some(n) => explicit = Some(n),
                None => ctx.diags.push(
                    Diagnostic::error(rest_span, "unexpected text after the closing `$$`")
                        .expecting(["an equation number such as `(1)`", "the end of the line"]),
                ),
            }
        }
    }

    let (mut ast, d) = parse_math(ctx.text(raw), raw.start);
    ctx.diags.extend(d);

    let noeval = ctx.pending_noeval.take().is_some();
    if noeval {
        for line in &mut ast.lines {
            line.noeval = true;
        }
    }

    let label = ast.label.as_ref().map(|l| l.name.clone());
    let number = match (explicit, &label) {
        (Some(n), _) => Some(n),
        (None, Some(_)) => Some(ctx.eq_counter.saturating_add(1)),
        (None, None) => None,
    };
    if let Some(n) = number {
        ctx.eq_counter = n;
    }
    if let (Some(name), Some(n)) = (&label, number) {
        if ctx.labels.contains_key(name) {
            let span = ast.label.as_ref().map_or(raw, |l| l.span);
            ctx.diags.push(Diagnostic::error(
                span,
                format!("the label `{name}` is already used by an earlier equation"),
            ));
        } else {
            ctx.labels.insert(name.clone(), n);
        }
    }

    out.push(block(
        BlockKind::DisplayMath(DisplayMath {
            ast,
            raw,
            number,
            label,
            noeval,
            closed,
        }),
        Span::new(open, block_end),
    ));
    m + 1
}

// ---- theorem blocks --------------------------------------------------------

#[allow(clippy::too_many_arguments)]
fn theorem(
    ctx: &mut Ctx<'_>,
    lines: &[Line],
    i: usize,
    kind: TheoremKind,
    start: usize,
    body_start: usize,
    depth: usize,
    out: &mut Vec<Block>,
) -> usize {
    // The first line without the `**Kind.**` prefix.
    let mut body: Vec<Line> = vec![Line {
        start: body_start.min(lines[i].end),
        end: lines[i].end,
    }];
    let mut closed = true;
    let mut k = i;
    if kind == TheoremKind::Proof {
        closed = false;
        loop {
            let text = ctx.line_text(body[body.len() - 1]);
            if text.contains('∎') || text.contains("\\qed") {
                closed = true;
                k += 1;
                break;
            }
            k += 1;
            if k >= lines.len() {
                break;
            }
            let (t, _) = trimmed(ctx, lines[k]);
            if heading_level(t).is_some() || theorem_start(t).is_some() {
                break;
            }
            body.push(lines[k]);
        }
        if !closed {
            let at = body[0].start.min(lines[i].end);
            ctx.diags.push(
                Diagnostic::warning(
                    Span::new(start, at),
                    "this proof is not closed with `∎` or `\\qed`",
                )
                .expecting(["`∎`", "`\\qed`"]),
            );
        }
    } else {
        k = i + 1;
        while k < lines.len() {
            let (t, _) = trimmed(ctx, lines[k]);
            if is_blank(ctx, lines[k]) || heading_level(t).is_some() || theorem_start(t).is_some() {
                break;
            }
            body.push(lines[k]);
            k += 1;
        }
    }
    let blocks = parse_blocks(ctx, &body, depth + 1);
    let end = body[body.len() - 1].end.max(start);
    out.push(block(
        BlockKind::Theorem(Theorem {
            kind,
            body: blocks,
            closed,
        }),
        Span::new(start, end),
    ));
    k
}
