//! The document model behind the equation view: the `.mpp` text, a caret and
//! an optional selection, all as byte offsets into the source.
//!
//! The view maps screen positions to offsets with the page's caret stops; this
//! module only edits text. Its small API (`insert_text`, `delete`, `move_caret`,
//! `replace_node`) is what the template and box editor builds on: every change
//! to the text goes through one splice, which keeps the boxes of inserted
//! templates (see [`template`]) in step with the text.

pub mod comment;
mod rules;
pub mod symbols;
pub mod template;

use std::ops::Range;
use std::time::Instant;

use mpp_syntax::Span;

use template::shift_range;
pub use template::{likely_variable, parse_snippet, Knobs, Session, Snippet, TBox};

/// Most undo steps kept.
const HISTORY_LIMIT: usize = 500;
/// Most bytes the undo steps may keep (older steps are dropped past this).
const HISTORY_BYTES: usize = 8 << 20;
/// At most this many templates are tracked at once (the oldest are forgotten).
const SESSION_LIMIT: usize = 64;

/// How consecutive edits group into one undo step.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EditKind {
    Typing,
    Other,
}

/// The text of an undo step. The step nearest the current state keeps its whole
/// text; every other step keeps only how it differs from its neighbour nearer the
/// current state, so a long history of small edits costs little memory.
#[derive(Debug, Clone)]
enum Stored {
    Full(String),
    /// This step's text is the neighbour's with `neighbour[start..start + inserted]`
    /// replaced by `removed`.
    Delta {
        start: usize,
        removed: String,
        inserted: usize,
    },
}

impl Stored {
    /// How `this` differs from `neighbour`.
    fn delta(this: &str, neighbour: &str) -> Stored {
        let mut start = this
            .bytes()
            .zip(neighbour.bytes())
            .take_while(|(a, b)| a == b)
            .count();
        while !this.is_char_boundary(start) {
            start -= 1;
        }
        let room = this.len().min(neighbour.len()) - start;
        let mut tail = this
            .bytes()
            .rev()
            .zip(neighbour.bytes().rev())
            .take(room)
            .take_while(|(a, b)| a == b)
            .count();
        while !this.is_char_boundary(this.len() - tail) {
            tail -= 1;
        }
        Stored::Delta {
            start,
            removed: this[start..this.len() - tail].to_string(),
            inserted: neighbour.len() - tail - start,
        }
    }

    /// The whole text, given the neighbour's.
    fn text_from(&self, neighbour: &str) -> String {
        match self {
            Stored::Full(t) => t.clone(),
            Stored::Delta {
                start,
                removed,
                inserted,
            } => {
                let mut t = String::with_capacity(neighbour.len() + removed.len());
                t.push_str(&neighbour[..*start]);
                t.push_str(removed);
                t.push_str(&neighbour[start + inserted..]);
                t
            }
        }
    }

    fn bytes(&self) -> usize {
        match self {
            Stored::Full(t) => t.len(),
            Stored::Delta { removed, .. } => removed.len() + 16,
        }
    }
}

#[derive(Debug, Clone)]
struct Snapshot {
    text: Stored,
    caret: usize,
    sessions: Vec<Session>,
    /// When this state was reached (none: as the file was opened).
    when: Option<Instant>,
}

impl Snapshot {
    fn full(&self) -> &str {
        match &self.text {
            Stored::Full(t) => t,
            Stored::Delta { .. } => unreachable!("the nearest step keeps its whole text"),
        }
    }
}

/// Pushes a step that keeps its whole text; the step below it keeps only a delta.
fn push_step(stack: &mut Vec<Snapshot>, s: Snapshot) {
    if let Some(top) = stack.last_mut() {
        if let Stored::Full(t) = &top.text {
            top.text = Stored::delta(t, s.full());
        }
    }
    stack.push(s);
}

/// Pops the nearest step (whole text); the one below gets its whole text back.
fn pop_step(stack: &mut Vec<Snapshot>) -> Option<Snapshot> {
    let s = stack.pop()?;
    if let Some(top) = stack.last_mut() {
        if let Stored::Delta { .. } = top.text {
            top.text = Stored::Full(top.text.text_from(s.full()));
        }
    }
    Some(s)
}

/// The texts of a stack, nearest step first.
fn step_texts(stack: &[Snapshot], limit: usize) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for s in stack.iter().rev().take(limit) {
        let text = match out.last() {
            Some(prev) => s.text.text_from(prev),
            None => s.full().to_string(),
        };
        out.push(text);
    }
    out
}

/// One state of the undo history, as Edit, History lists it.
#[derive(Debug, Clone)]
pub struct HistoryState {
    pub text: String,
    /// When the state was reached (none: as the file was opened).
    pub when: Option<Instant>,
}

/// Typing this character ends a word: it joins the word's undo step, and
/// the next character starts a new one.
fn ends_word(c: char) -> bool {
    c.is_whitespace() || matches!(c, ',' | ';' | '.' | ':' | '!' | '?')
}

/// The text of a document with its caret and selection.
#[derive(Debug, Clone, Default)]
pub struct Editor {
    text: String,
    caret: usize,
    anchor: Option<usize>,
    undo: Vec<Snapshot>,
    redo: Vec<Snapshot>,
    last_kind: Option<EditKind>,
    /// The last word typed was ended (a space or punctuation): the next
    /// character starts a new undo step.
    word_done: bool,
    /// When the current state was reached.
    when: Option<Instant>,
    revision: u64,
    sessions: Vec<Session>,
    next_session: u64,
    /// The template Tab just left forward, and where the caret was put.
    left: Option<(u64, usize)>,
    /// A one-time hint from the last edit, with the revision it belongs to.
    hint: Option<(String, u64)>,
    /// The heading hint was shown already.
    hinted_heading: bool,
}

impl Editor {
    /// An editor holding `text`, with the caret at the start.
    pub fn new(text: impl Into<String>) -> Self {
        Editor {
            text: text.into(),
            ..Editor::default()
        }
    }

    /// The whole text.
    pub fn text(&self) -> &str {
        &self.text
    }

    /// The caret's byte offset.
    pub fn caret(&self) -> usize {
        self.caret
    }

    /// Bumps on every change to the text.
    pub fn revision(&self) -> u64 {
        self.revision
    }

    /// The selected range, if any.
    pub fn selection(&self) -> Option<Range<usize>> {
        let a = self.anchor?;
        if a == self.caret {
            return None;
        }
        Some(a.min(self.caret)..a.max(self.caret))
    }

    /// The selected text.
    pub fn selected_text(&self) -> Option<&str> {
        self.selection().and_then(|r| self.text.get(r))
    }

    /// Replaces the whole text (a file was loaded); clears the history.
    pub fn set_text(&mut self, text: impl Into<String>) {
        self.text = text.into();
        self.caret = 0;
        self.anchor = None;
        self.undo.clear();
        self.redo.clear();
        self.sessions.clear();
        self.last_kind = None;
        self.word_done = false;
        self.when = None;
        self.revision += 1;
    }

    fn clamp(&self, offset: usize) -> usize {
        let mut o = offset.min(self.text.len());
        while o > 0 && !self.text.is_char_boundary(o) {
            o -= 1;
        }
        o
    }

    fn record(&mut self, kind: EditKind, contiguous: bool) {
        let group = kind == EditKind::Typing
            && self.last_kind == Some(EditKind::Typing)
            && contiguous
            && !self.word_done;
        self.word_done = false;
        if !group {
            push_step(
                &mut self.undo,
                Snapshot {
                    text: Stored::Full(self.text.clone()),
                    caret: self.caret,
                    sessions: self.sessions.clone(),
                    when: self.when,
                },
            );
            self.trim_history();
        }
        self.redo.clear();
        self.last_kind = Some(kind);
        self.left = None;
        self.when = Some(Instant::now());
    }

    /// The one place the text changes: replaces `range` and moves every
    /// template box, the caret and the anchor to match. A template the edit cuts
    /// across is forgotten (its text stays).
    fn splice(&mut self, range: Range<usize>, with: &str) {
        let start = self.clamp(range.start);
        let end = self.clamp(range.end).max(start);
        let edit = start..end;
        self.text.replace_range(edit.clone(), with);
        let n = with.len();
        self.caret = move_point(self.caret, &edit, n);
        self.anchor = self.anchor.map(|a| move_point(a, &edit, n));
        self.sessions.retain_mut(|s| adjust_session(s, &edit, n));
    }

    /// Moves the caret to `offset`, extending the selection when `extend`.
    pub fn move_caret(&mut self, offset: usize, extend: bool) {
        let o = self.clamp(offset);
        if extend {
            self.anchor.get_or_insert(self.caret);
        } else {
            self.anchor = None;
        }
        self.caret = o;
        self.last_kind = None;
        self.left = None;
    }

    /// Selects a range.
    pub fn select(&mut self, range: Range<usize>) {
        self.anchor = Some(self.clamp(range.start));
        self.caret = self.clamp(range.end);
        self.last_kind = None;
        self.left = None;
    }

    /// Inserts text at the caret, replacing the selection.
    pub fn insert_text(&mut self, s: &str) {
        if s.is_empty() {
            return;
        }
        let left = self.left.take();
        // Typing groups word by word: a word and the space or punctuation after
        // it are one step. A new line is a step of its own.
        let typing = !s.contains('\n');
        let contiguous = self.selection().is_none();
        if s.chars().all(ends_word) {
            // ", " after a word: the space joins the comma's step.
            self.word_done = false;
        }
        self.record(
            if typing {
                EditKind::Typing
            } else {
                EditKind::Other
            },
            contiguous,
        );
        self.word_done = typing && s.chars().next_back().is_some_and(ends_word);
        let range = self.selection().unwrap_or(self.caret..self.caret);
        let left = left.filter(|(_, at)| range == (*at..*at));
        self.splice(range.clone(), s);
        if let Some((id, at)) = left {
            self.keep_out(id, at);
        }
        self.caret = range.start + s.len();
        self.anchor = None;
        self.fixup();
        self.revision += 1;
    }

    /// Text was typed just after template `id` (at `at`), which the caret had
    /// left: the boxes ending there must not take it in.
    fn keep_out(&mut self, id: u64, at: usize) {
        let Some(si) = self.session_index(id) else {
            return;
        };
        let s = &mut self.sessions[si];
        for b in &mut s.boxes {
            if b.range.start <= at && b.range.end > at {
                b.range.end = at;
            }
        }
        if s.range.end > at {
            s.range.end = at;
        }
        for (g, _) in &mut s.groups {
            if g.end > at {
                g.end = at;
            }
        }
    }

    /// Deletes the selection, or the text between the caret and `target`
    /// (the offset the view says is the previous or next visible position).
    /// When `balanced` is set and that text holds an unmatched brace, the caret
    /// moves to `target` instead, so a structure is entered before it is taken apart.
    pub fn delete(&mut self, target: usize, balanced: bool) {
        if let Some(r) = self.selection() {
            self.record(EditKind::Other, false);
            self.splice(r.clone(), "");
            self.caret = r.start;
            self.anchor = None;
            self.fixup();
            self.revision += 1;
            return;
        }
        let t = self.clamp(target);
        if t == self.caret {
            return;
        }
        let range = t.min(self.caret)..t.max(self.caret);
        if balanced && !braces_balanced(&self.text[range.clone()]) {
            self.caret = t;
            self.last_kind = None;
            return;
        }
        self.record(EditKind::Other, false);
        self.splice(range.clone(), "");
        self.caret = range.start;
        self.fixup();
        self.revision += 1;
    }

    /// Deletes a range of the text and puts the caret where it was.
    pub fn delete_range(&mut self, range: Range<usize>) {
        let start = self.clamp(range.start);
        let end = self.clamp(range.end).max(start);
        if start == end {
            return;
        }
        self.record(EditKind::Other, false);
        self.splice(start..end, "");
        self.caret = start;
        self.anchor = None;
        self.fixup();
        self.revision += 1;
    }

    /// Replaces the source of a node (or any span) and puts the caret after it.
    pub fn replace_node(&mut self, span: Span, text: &str) {
        let start = self.clamp(span.start);
        let end = self.clamp(span.end).max(start);
        self.record(EditKind::Other, false);
        self.splice(start..end, text);
        self.caret = start + text.len();
        self.anchor = None;
        self.fixup();
        self.revision += 1;
    }

    /// Replaces a range without making an undo step of its own (live symbol
    /// replacement joins the typing that caused it); the caret keeps its place
    /// relative to the text after the range.
    pub fn replace_inline(&mut self, range: Range<usize>, with: &str) {
        let start = self.clamp(range.start);
        let end = self.clamp(range.end).max(start);
        let caret = self.caret;
        self.splice(start..end, with);
        if caret >= end {
            self.caret = caret - (end - start) + with.len();
        }
        self.anchor = None;
        self.fixup();
        self.revision += 1;
    }

    /// Drops the oldest undo steps past the step and byte limits.
    fn trim_history(&mut self) {
        let mut bytes: usize = self.undo.iter().map(|s| s.text.bytes()).sum();
        let mut drop = 0;
        while self.undo.len() - drop > 1
            && (self.undo.len() - drop > HISTORY_LIMIT || bytes > HISTORY_BYTES)
        {
            bytes -= self.undo[drop].text.bytes();
            drop += 1;
        }
        self.undo.drain(..drop);
    }

    /// Bytes the undo and redo steps keep.
    pub fn history_bytes(&self) -> usize {
        self.undo
            .iter()
            .chain(&self.redo)
            .map(|s| s.text.bytes())
            .sum()
    }

    fn restore(&mut self, s: Snapshot) -> Snapshot {
        let Stored::Full(text) = s.text else {
            unreachable!("the nearest step keeps its whole text")
        };
        let old = Snapshot {
            text: Stored::Full(std::mem::replace(&mut self.text, text)),
            caret: self.caret,
            sessions: std::mem::replace(&mut self.sessions, s.sessions),
            when: std::mem::replace(&mut self.when, s.when),
        };
        self.caret = s.caret.min(self.text.len());
        self.anchor = None;
        self.last_kind = None;
        self.word_done = false;
        self.revision += 1;
        old
    }

    /// Steps back one undo step.
    pub fn undo(&mut self) -> bool {
        let Some(s) = pop_step(&mut self.undo) else {
            return false;
        };
        let old = self.restore(s);
        push_step(&mut self.redo, old);
        true
    }

    /// Steps forward again.
    pub fn redo(&mut self) -> bool {
        let Some(s) = pop_step(&mut self.redo) else {
            return false;
        };
        let old = self.restore(s);
        push_step(&mut self.undo, old);
        true
    }

    /// Every state of the undo history, oldest first: the undo steps, the
    /// current state (at [`Editor::history_position`]), then the redo steps.
    pub fn history(&self) -> Vec<HistoryState> {
        self.history_window(usize::MAX)
            .into_iter()
            .map(|(_, s)| s)
            .collect()
    }

    /// The newest `count` states of [`Editor::history`] with their indices, oldest
    /// first (cheaper than the whole history when it is long).
    pub fn history_window(&self, count: usize) -> Vec<(usize, HistoryState)> {
        let redo = step_texts(&self.redo, usize::MAX);
        let mut newest: Vec<(usize, HistoryState)> = Vec::new();
        let here = self.undo.len();
        for (k, (text, s)) in redo
            .into_iter()
            .zip(self.redo.iter().rev())
            .enumerate()
            .rev()
        {
            newest.push((here + 1 + k, HistoryState { text, when: s.when }));
        }
        newest.push((
            here,
            HistoryState {
                text: self.text.clone(),
                when: self.when,
            },
        ));
        let room = count.saturating_sub(newest.len());
        let undo = step_texts(&self.undo, room);
        for (k, (text, s)) in undo.into_iter().zip(self.undo.iter().rev()).enumerate() {
            newest.push((here - 1 - k, HistoryState { text, when: s.when }));
        }
        newest.truncate(count);
        newest.reverse();
        newest
    }

    /// Where the current state is in [`Editor::history`].
    pub fn history_position(&self) -> usize {
        self.undo.len()
    }

    /// Undoes or redoes until state `index` of [`Editor::history`] is current
    /// (so Ctrl+Z and Ctrl+Y keep working from there).
    pub fn go_to_history(&mut self, index: usize) -> bool {
        if index > self.undo.len() + self.redo.len() {
            return false;
        }
        while self.undo.len() > index {
            self.undo();
        }
        while self.undo.len() < index {
            self.redo();
        }
        true
    }

    // ----- templates -----

    /// The templates being tracked, oldest first.
    pub fn sessions(&self) -> &[Session] {
        &self.sessions
    }

    /// The innermost template holding the offset.
    pub fn session_at(&self, offset: usize) -> Option<&Session> {
        self.sessions
            .iter()
            .filter(|s| s.holds(offset))
            .min_by_key(|s| s.range.len())
    }

    fn session_index(&self, id: u64) -> Option<usize> {
        self.sessions.iter().position(|s| s.id == id)
    }

    /// The template with this id.
    pub fn session(&self, id: u64) -> Option<&Session> {
        self.sessions.iter().find(|s| s.id == id)
    }

    /// Inserts a template in place of `replace`, with `before` and `after` around
    /// its text (the `$$ ` that makes a prose line a maths block). One undo step.
    /// The caret goes to the template's first box (its default selected).
    /// Returns the template's id.
    pub fn insert_template(
        &mut self,
        replace: Range<usize>,
        before: &str,
        snippet: &Snippet,
        after: &str,
        template: &str,
        knobs: Knobs,
    ) -> u64 {
        self.record(EditKind::Other, false);
        self.place_template(replace, before, snippet, after, template, knobs)
    }

    fn place_template(
        &mut self,
        replace: Range<usize>,
        before: &str,
        snippet: &Snippet,
        after: &str,
        template: &str,
        knobs: Knobs,
    ) -> u64 {
        let start = self.clamp(replace.start);
        let end = self.clamp(replace.end).max(start);
        let full = format!("{before}{}{after}", snippet.text);
        self.splice(start..end, &full);
        let id = self.next_session;
        self.next_session += 1;
        let mut s = Session::new(template, knobs, snippet, start + before.len());
        s.id = id;
        let primary = s.primary;
        self.sessions.push(s);
        if self.sessions.len() > SESSION_LIMIT {
            self.sessions.remove(0);
        }
        self.caret = start + full.len();
        self.anchor = None;
        self.fixup();
        if let Some(p) = primary {
            self.focus_box(id, p);
        }
        self.revision += 1;
        id
    }

    /// Puts the caret in a box: selects what is in it, so typing replaces it.
    pub fn focus_box(&mut self, id: u64, index: usize) {
        let Some(b) = self
            .session(id)
            .and_then(|s| s.boxes.get(index))
            .map(|b| b.range.clone())
        else {
            return;
        };
        if let Some(si) = self.session_index(id) {
            let s = &mut self.sessions[si];
            if s.primary != Some(index) {
                s.walked = true;
            }
        }
        if b.is_empty() {
            self.move_caret(b.start, false);
        } else {
            self.select(b);
        }
    }

    /// Tab (or Shift+Tab when `!forward`) inside a template: moves to the next
    /// box in the Tab order, which is reading order (an integral's bounds, then
    /// its integrand, then its variable); past the last box the caret leaves the
    /// template (and empty optional parts are removed). Shift+Tab walks the same
    /// order backwards. The caret starts in the primary box (the integrand), so
    /// until it has been anywhere else the first Tab from there goes to the first
    /// box of the order when an earlier box is still empty. Returns false when
    /// the caret is not in a template.
    pub fn tab(&mut self, forward: bool) -> bool {
        let caret = self.selection().map_or(self.caret, |r| r.start);
        let Some(s) = self.session_at(caret) else {
            return false;
        };
        let id = s.id;
        let cur = s.box_at(caret).and_then(|i| s.tab_pos(i));
        let start = s.primary.and_then(|i| s.tab_pos(i));
        // Boxes before the starting one that still need filling (an integral's
        // bounds; a sum's index and limits come with defaults).
        let lead_open = |p: usize| s.tab[..p].iter().any(|&i| s.boxes[i].range.is_empty());
        let next = match (cur, forward) {
            (Some(p), true) if !s.walked && Some(p) == start && lead_open(p) => Some(0),
            (Some(p), true) => Some(p + 1),
            (Some(p), false) => p.checked_sub(1),
            (None, true) => s.tab.iter().position(|&i| s.boxes[i].range.start > caret),
            (None, false) => s.tab.iter().rposition(|&i| s.boxes[i].range.end < caret),
        };
        if let (Some(p), Some(si)) = (cur, self.session_index(id)) {
            if Some(p) != start {
                self.sessions[si].walked = true;
            }
        }
        let Some(s) = self.session(id) else {
            return false;
        };
        match next.and_then(|p| s.tab.get(p).copied()) {
            Some(i) => self.focus_box(id, i),
            None => {
                self.finalize(id);
                let target =
                    self.session(id)
                        .map(|s| if forward { s.range.end } else { s.range.start });
                if let Some(t) = target {
                    self.move_caret(t, false);
                    if forward {
                        // Typing now goes after the template, not into its last box.
                        self.left = Some((id, t));
                    }
                }
            }
        }
        true
    }

    /// Removes the empty optional parts of a template (both bounds empty make an
    /// indefinite integral). Joins the current undo step.
    pub fn finalize(&mut self, id: u64) {
        loop {
            let Some(s) = self.session(id) else {
                return;
            };
            let strip = s.groups.iter().position(|(_, boxes)| {
                boxes
                    .iter()
                    .all(|&b| s.boxes.get(b).is_none_or(TBox::is_open))
            });
            let Some(g) = strip else {
                return;
            };
            let (range, boxes) = s.groups[g].clone();
            if let Some(si) = self.session_index(id) {
                let s = &mut self.sessions[si];
                s.groups.remove(g);
                remove_boxes(s, &boxes);
            }
            self.splice(range, "");
            self.fixup();
            self.revision += 1;
        }
    }

    /// Finalizes every template the caret is not in (call after the caret moved).
    pub fn finalize_outside(&mut self) {
        let caret = self.caret;
        let ids: Vec<u64> = self
            .sessions
            .iter()
            .filter(|s| !s.holds(caret) && s.range.end != caret)
            .filter(|s| {
                s.groups
                    .iter()
                    .any(|(_, b)| b.iter().all(|&i| s.boxes.get(i).is_none_or(TBox::is_open)))
            })
            .map(|s| s.id)
            .collect();
        for id in ids {
            self.finalize(id);
        }
    }

    fn box_text(&self, b: &TBox) -> &str {
        self.text.get(b.range.clone()).unwrap_or("")
    }

    /// Backspace inside a template. In an empty first box it removes the whole
    /// template when all its boxes are empty (or still hold their defaults);
    /// otherwise, at the start of a box, it moves back one box. Returns false
    /// when Backspace should do its usual thing.
    pub fn backspace_in_template(&mut self) -> bool {
        if self.selection().is_some() {
            return false;
        }
        let caret = self.caret;
        let Some(s) = self.session_at(caret) else {
            return false;
        };
        let id = s.id;
        let Some(i) = s.box_at(caret) else {
            return false;
        };
        let b = &s.boxes[i];
        if caret != b.range.start {
            return false;
        }
        let pos = s.tab_pos(i).unwrap_or(0);
        if pos == 0 {
            let all_empty = s.tab_boxes().all(|b| b.untouched(&self.text));
            if all_empty && b.range.is_empty() {
                let r = s.range.clone();
                self.remove_template(id, r);
            } else {
                let start = s.range.start;
                self.move_caret(start, false);
            }
            return true;
        }
        let prev = s.tab[pos - 1];
        let end = s.boxes[prev].range.end;
        self.move_caret(end, false);
        true
    }

    fn remove_template(&mut self, id: u64, range: Range<usize>) {
        self.record(EditKind::Other, false);
        self.sessions.retain(|s| s.id != id);
        // A template alone in a `$$ … $$` block takes the block with it.
        let (mut a, mut b) = (range.start, range.end);
        let alone = self.text.get(..a).is_some_and(|t| t.ends_with("$$ "))
            && self.text.get(b..).is_some_and(|t| t.starts_with(" $$"));
        if alone {
            a -= 3;
            b += 3;
        }
        self.splice(a..b, "");
        self.caret = a;
        self.anchor = None;
        self.fixup();
        self.revision += 1;
    }

    /// Sets the text of a box (a quick-pick button). One undo step; the caret
    /// goes after the new text.
    pub fn set_box_text(&mut self, id: u64, index: usize, with: &str) {
        let Some(r) = self
            .session(id)
            .and_then(|s| s.boxes.get(index))
            .map(|b| b.range.clone())
        else {
            return;
        };
        self.record(EditKind::Other, false);
        self.splice(r.clone(), with);
        self.caret = r.start + with.len();
        self.anchor = None;
        self.fixup();
        self.revision += 1;
    }

    /// Replaces a template with a rebuilt one (a control changed its shape),
    /// putting the caret back in the box with the same key. One undo step.
    pub fn rebuild_template(&mut self, id: u64, snippet: &Snippet, knobs: Knobs) -> Option<u64> {
        let s = self.session(id)?.clone();
        let caret_key = s
            .box_at(self.caret)
            .and_then(|i| s.boxes.get(i))
            .map(|b| b.key.clone());
        self.record(EditKind::Other, false);
        // The template and any nested inside it are rebuilt away with their text.
        self.sessions
            .retain(|o| !(s.range.start <= o.range.start && o.range.end <= s.range.end));
        let new_id = self.place_template(s.range.clone(), "", snippet, "", &s.template, knobs);
        if let Some(k) = caret_key {
            let r = self.session(new_id).and_then(|n| {
                n.boxes
                    .iter()
                    .find(|b| b.key == k && b.mirror_of.is_none())
                    .map(|b| b.range.clone())
            });
            if let Some(r) = r {
                self.move_caret(r.end, false);
            }
        }
        Some(new_id)
    }

    /// The labels of empty boxes, by the offset of their empty slot.
    pub fn slot_labels(&self) -> Vec<(usize, String)> {
        let mut out = vec![];
        for s in &self.sessions {
            for b in s.tab_boxes() {
                if b.range.is_empty() {
                    out.push((b.range.start, b.label.clone()));
                }
            }
        }
        out
    }

    /// Templates that still have an empty box: their parse errors are shown as a
    /// "fill in" hint, not as errors.
    pub fn quiet_ranges(&self) -> Vec<Range<usize>> {
        self.sessions
            .iter()
            .filter(|s| s.tab_boxes().any(|b| b.range.is_empty()))
            .map(|s| s.range.clone())
            .collect()
    }

    /// The empty required boxes (offset of the slot, label), in Tab order.
    pub fn missing_boxes(&self) -> Vec<(usize, String)> {
        let mut out = vec![];
        for s in &self.sessions {
            for b in s.tab_boxes() {
                if b.required && b.range.is_empty() {
                    out.push((b.range.start, b.label.clone()));
                }
            }
        }
        out
    }

    /// Keeps bare boxes parseable and mirrors in step after an edit.
    fn fixup(&mut self) {
        // Where the caret is, relative to its box, so it can be put back.
        let anchor = self.session_at(self.caret).and_then(|s| {
            let i = s.box_at(self.caret)?;
            Some((s.id, i, self.caret - s.boxes[i].range.start))
        });
        let ids: Vec<u64> = self.sessions.iter().map(|s| s.id).collect();
        for id in ids {
            let n = self.session(id).map_or(0, |s| s.boxes.len());
            for i in 0..n {
                self.fix_box(id, i);
            }
        }
        if let Some((id, i, rel)) = anchor {
            if let Some(r) = self
                .session(id)
                .and_then(|s| s.boxes.get(i))
                .map(|b| b.range.clone())
            {
                self.caret = (r.start + rel).min(r.end);
            }
        }
        self.caret = self.clamp(self.caret);
    }

    fn fix_box(&mut self, id: u64, i: usize) {
        let Some(s) = self.session(id) else {
            return;
        };
        let Some(b) = s.boxes.get(i).cloned() else {
            return;
        };
        if let Some(src) = b.mirror_of.and_then(|m| s.boxes.get(m)) {
            let want = match self.box_text(src) {
                "" => "{}".to_string(),
                t => t.to_string(),
            };
            if self.box_text(&b) != want {
                self.splice(b.range.clone(), &want);
            }
            return;
        }
        if !b.bare {
            return;
        }
        let empty = b.range.is_empty();
        if empty && !b.wrapped {
            let at = b.range.start;
            self.splice(at..at, "{}");
            if let Some(si) = self.session_index(id) {
                let tb = &mut self.sessions[si].boxes[i];
                tb.range = at + 1..at + 1;
                tb.wrapped = true;
            }
        } else if !empty && b.wrapped {
            let (a, z) = (b.range.start, b.range.end);
            let braced = a > 0
                && self.text.as_bytes().get(a - 1) == Some(&b'{')
                && self.text.as_bytes().get(z) == Some(&b'}');
            if braced {
                self.splice(z..z + 1, "");
                self.splice(a - 1..a, "");
            }
            if let Some(si) = self.session_index(id) {
                self.sessions[si].boxes[i].wrapped = false;
            }
        }
    }
}

/// Where an offset ends up after `edit` was replaced by `n` bytes.
fn move_point(x: usize, edit: &Range<usize>, n: usize) -> usize {
    if x >= edit.end {
        x - edit.len() + n
    } else if x > edit.start {
        edit.start + n.min(x - edit.start)
    } else {
        x
    }
}

/// Moves a session's ranges for an edit. False when the edit broke the template.
fn adjust_session(s: &mut Session, edit: &Range<usize>, n: usize) -> bool {
    for b in &mut s.boxes {
        match shift_range(&b.range, edit, n, true) {
            Some(r) => b.range = r,
            None => return false,
        }
    }
    let Some(r) = shift_range(&s.range, edit, n, false) else {
        return false;
    };
    s.range = r;
    s.groups
        .retain_mut(|(g, _)| match shift_range(g, edit, n, false) {
            Some(r) => {
                *g = r;
                true
            }
            None => false,
        });
    // A box can grow past the template's ends (typing at the end of `dx`).
    for b in &s.boxes {
        s.range.start = s.range.start.min(b.range.start);
        s.range.end = s.range.end.max(b.range.end);
    }
    for (g, boxes) in &mut s.groups {
        for &i in boxes.iter() {
            if let Some(b) = s.boxes.get(i) {
                g.start = g.start.min(b.range.start);
                g.end = g.end.max(b.range.end);
            }
        }
    }
    true
}

/// Drops boxes from a session, fixing up indices.
fn remove_boxes(s: &mut Session, gone: &[usize]) {
    let mut next = 0;
    let map: Vec<Option<usize>> = (0..s.boxes.len())
        .map(|i| {
            if gone.contains(&i) {
                None
            } else {
                next += 1;
                Some(next - 1)
            }
        })
        .collect();
    let remap = |i: usize| map.get(i).copied().flatten();
    let old = std::mem::take(&mut s.boxes);
    s.boxes = old
        .into_iter()
        .enumerate()
        .filter(|(i, _)| remap(*i).is_some())
        .map(|(_, mut b)| {
            b.mirror_of = b.mirror_of.and_then(remap);
            b
        })
        .collect();
    s.tab = s.tab.iter().filter_map(|&i| remap(i)).collect();
    s.primary = s.primary.and_then(remap);
    for (_, g) in &mut s.groups {
        *g = g.iter().filter_map(|&i| remap(i)).collect();
    }
}

fn braces_balanced(s: &str) -> bool {
    let mut depth = 0i32;
    let mut prev = '\0';
    for c in s.chars() {
        if prev != '\\' {
            match c {
                '{' => depth += 1,
                '}' => depth -= 1,
                _ => {}
            }
        }
        prev = c;
    }
    depth == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn typing_inserts_at_the_caret() {
        let mut e = Editor::new("ab");
        e.move_caret(1, false);
        e.insert_text("x");
        assert_eq!(e.text(), "axb");
        assert_eq!(e.caret(), 2);
    }

    #[test]
    fn typing_groups_into_one_undo_step() {
        let mut e = Editor::new("");
        for c in ["a", "b", "c"] {
            e.insert_text(c);
        }
        assert!(e.undo());
        assert_eq!(e.text(), "");
        assert!(e.redo());
        assert_eq!(e.text(), "abc");
    }

    #[test]
    fn typing_groups_word_by_word() {
        let mut e = Editor::new("");
        for c in "ab cd, ef".chars() {
            e.insert_text(&c.to_string());
        }
        assert!(e.undo());
        assert_eq!(e.text(), "ab cd, ");
        assert!(e.undo());
        assert_eq!(e.text(), "ab ");
        assert!(e.undo());
        assert_eq!(e.text(), "");
        assert!(!e.undo());
    }

    #[test]
    fn a_new_line_is_a_step_of_its_own() {
        let mut e = Editor::new("");
        e.insert_text("a");
        e.insert_text("\n");
        e.insert_text("b");
        assert!(e.undo());
        assert_eq!(e.text(), "a\n");
        assert!(e.undo());
        assert_eq!(e.text(), "a");
    }

    #[test]
    fn the_history_walks_the_undo_steps() {
        let mut e = Editor::new("x");
        e.replace_node(Span::new(0, 1), "y");
        e.replace_node(Span::new(0, 1), "z");
        let history = e.history();
        let texts: Vec<&str> = history.iter().map(|h| h.text.as_str()).collect();
        assert_eq!(texts, ["x", "y", "z"]);
        assert_eq!(e.history_position(), 2);
        assert!(e.go_to_history(0));
        assert_eq!(e.text(), "x");
        assert_eq!(e.history().len(), 3, "the later steps stay as redo");
        assert!(e.go_to_history(2));
        assert_eq!(e.text(), "z");
        assert!(!e.go_to_history(3));
    }

    #[test]
    fn a_long_history_of_small_edits_stays_small_and_walks_back_exactly() {
        // A 64 KB document and 600 separate edits: full copies took about 84 KB a step.
        let base: String = "$$ x_1 := 1 $$\n\n".repeat(4000);
        let mut e = Editor::new(base.clone());
        let mut texts = vec![base.clone()];
        for i in 0..600 {
            e.move_caret((i * 97) % e.text().len(), false);
            e.insert_text("\n");
            texts.push(e.text().to_string());
        }
        assert!(
            e.history_bytes() < 4 * base.len(),
            "{} bytes for a {}-byte text",
            e.history_bytes(),
            base.len()
        );
        // Only the newest HISTORY_LIMIT steps are kept; each comes back exactly.
        let kept = texts.len() - 1 - HISTORY_LIMIT;
        let history = e.history();
        assert_eq!(history.len(), HISTORY_LIMIT + 1);
        assert!(history
            .iter()
            .zip(&texts[kept..])
            .all(|(h, t)| h.text == *t));
        let window = e.history_window(3);
        assert_eq!(window.len(), 3);
        assert_eq!(window[0].0, HISTORY_LIMIT - 2);
        assert_eq!(window[2].1.text, texts[600]);
        for _ in 0..250 {
            assert!(e.undo());
        }
        assert_eq!(e.text(), texts[350]);
        for _ in 0..100 {
            assert!(e.redo());
        }
        assert_eq!(e.text(), texts[450]);
        assert!(e.go_to_history(10));
        assert_eq!(e.text(), texts[kept + 10]);
        e.insert_text("é");
        assert!(e.undo());
        assert_eq!(e.text(), texts[kept + 10]);
    }

    #[test]
    fn steps_with_multibyte_text_come_back_whole() {
        let mut e = Editor::new("αβγ");
        e.replace_node(Span::new(2, 4), "ββ");
        e.replace_node(Span::new(0, 2), "é");
        let texts: Vec<String> = e.history().into_iter().map(|h| h.text).collect();
        assert_eq!(texts, ["αβγ", "αββγ", "éββγ"]);
        assert!(e.undo() && e.undo());
        assert_eq!(e.text(), "αβγ");
    }

    #[test]
    fn delete_to_target_and_selection() {
        let mut e = Editor::new("hello");
        e.move_caret(5, false);
        e.delete(4, false);
        assert_eq!(e.text(), "hell");
        e.select(0..2);
        e.delete(0, false);
        assert_eq!(e.text(), "ll");
    }

    #[test]
    fn unbalanced_delete_enters_the_structure_first() {
        let mut e = Editor::new("\\frac{a}{b}");
        e.move_caret(11, false);
        e.delete(10, true);
        assert_eq!(e.text(), "\\frac{a}{b}");
        assert_eq!(e.caret(), 10);
        e.delete(9, true);
        assert_eq!(e.text(), "\\frac{a}{}");
    }

    #[test]
    fn replace_node_and_multibyte() {
        let mut e = Editor::new("x = α + 1");
        e.replace_node(Span::new(4, 6), "beta");
        assert_eq!(e.text(), "x = beta + 1");
        assert_eq!(e.caret(), 8);
        e.move_caret(5, false);
        assert!(e.text().is_char_boundary(e.caret()));
    }

    #[test]
    fn moves_never_split_a_character() {
        let mut e = Editor::new("αβ");
        e.move_caret(1, false);
        assert_eq!(e.caret(), 0);
    }
}
