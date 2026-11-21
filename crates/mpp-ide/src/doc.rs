//! One open document: its text, its evaluation and its laid-out page.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use mpp_eval::Evaluated;
use mpp_render::IntegralLimits;
use mpp_syntax::document::{parse_document, Document};
use mpp_syntax::Span;

use crate::diagnostics::{self, Background, Counts, Problem};
use crate::editor::Editor;
use crate::eval::{EvalSettings, EvalWorker};
use crate::figure::Figures;
use crate::layout::DEBOUNCE_MS;
use crate::page::{build_page, Page, PageInput, ShowState};
use crate::theme::Mode;

#[derive(Debug, Clone, Copy, PartialEq)]
struct CacheKey {
    revision: u64,
    eval_id: u64,
    width: u32,
    size: u32,
    mode: Mode,
    limits: IntegralLimits,
    /// The template the caret is filling in (its problems are not shown yet).
    quiet: Option<(usize, usize)>,
    /// Bumped when the view's display choices change.
    show: u64,
}

impl PageCache {
    /// Slider rows the page left under a figure.
    pub fn doc_rows(&self, id: mpp_syntax::document::BlockId) -> usize {
        self.rows
            .iter()
            .find(|(b, _)| *b == id)
            .map_or(0, |(_, n)| *n)
    }
}

/// A laid-out page with the parse it came from and its problems.
#[derive(Debug)]
pub struct PageCache {
    key: CacheKey,
    pub page: Page,
    pub doc: Document,
    pub problems: Vec<Problem>,
    /// Slider rows under each figure, as laid out.
    rows: Vec<(mpp_syntax::document::BlockId, usize)>,
}

static NEXT_DOC_ID: AtomicU64 = AtomicU64::new(1);

/// An open file (or an unsaved new one).
#[derive(Debug)]
pub struct Doc {
    /// A number that stays with this document while it is open (tiles point at it).
    pub id: u64,
    pub path: Option<PathBuf>,
    pub editor: Editor,
    saved: String,
    worker: EvalWorker,
    pub eval: Option<Arc<Evaluated>>,
    background: Option<Arc<Background>>,
    eval_text: String,
    eval_id: u64,
    submitted: Option<u64>,
    last_edit: Instant,
    cache: Option<Arc<PageCache>>,
    /// The page at the other width, for a second view of this file.
    spare: Option<Arc<PageCache>>,
    /// A span to highlight (the target of go-to-definition).
    pub highlight: Option<Span>,
    /// A span to scroll into view on the next frame.
    pub scroll_to: Option<Span>,
    /// The x the caret keeps while moving up and down.
    pub want_x: Option<f32>,
    /// Integral bounds above and below the sign (default) or at its side.
    pub integral_limits: IntegralLimits,
    /// The width, size and mode of the last page laid out (problem lists of
    /// files that are not on screen use them).
    last_layout: (f32, f32, Mode),
    /// The view's display choices (number formats, unfolded steps, figure rows).
    show: ShowState,
    show_version: u64,
    /// The figures of the last evaluation, with their cameras and slider values.
    pub figures: Figures,
    eval_settings: EvalSettings,
    /// Automatic answers of the last evaluation are still being worked out.
    pending: bool,
}

impl Doc {
    /// A document with the given text.
    pub fn with_text(text: &str, path: Option<PathBuf>) -> Self {
        let mut d = Doc {
            id: NEXT_DOC_ID.fetch_add(1, Ordering::Relaxed),
            path,
            editor: Editor::new(text),
            saved: text.to_string(),
            worker: EvalWorker::new(),
            eval: None,
            background: None,
            eval_text: String::new(),
            eval_id: 0,
            submitted: None,
            last_edit: Instant::now(),
            cache: None,
            spare: None,
            highlight: None,
            scroll_to: None,
            want_x: None,
            integral_limits: IntegralLimits::AboveBelow,
            last_layout: (800.0, crate::layout::BODY_SIZE, Mode::Light),
            show: ShowState::default(),
            show_version: 0,
            figures: Figures::default(),
            eval_settings: EvalSettings::default(),
            pending: false,
        };
        d.submit_now();
        d
    }

    /// An empty unsaved document.
    pub fn blank() -> Self {
        Doc::with_text("", None)
    }

    /// Reads a file.
    pub fn open(path: &Path) -> std::io::Result<Self> {
        let text = std::fs::read_to_string(path)?;
        Ok(Doc::with_text(&text, Some(path.to_path_buf())))
    }

    /// The name shown on the tab.
    pub fn title(&self) -> String {
        self.path
            .as_deref()
            .map(crate::explorer::display_name)
            .unwrap_or_else(|| "Untitled".into())
    }

    /// True when there are changes that are not saved.
    pub fn is_dirty(&self) -> bool {
        self.editor.text() != self.saved
    }

    /// Replaces the text with what is on disk now (the file changed outside the
    /// app and there were no unsaved edits). The caret stays where it was.
    pub fn reload(&mut self, text: &str) {
        let caret = self.editor.caret();
        let end = self.editor.text().len();
        self.editor.replace_node(Span::new(0, end), text);
        self.editor.move_caret(caret, false);
        self.saved = text.to_string();
        self.touched();
    }

    /// Writes the file to its path.
    pub fn save(&mut self) -> std::io::Result<()> {
        let Some(p) = self.path.clone() else {
            return Err(std::io::Error::other("this file has no name yet"));
        };
        std::fs::write(p, self.editor.text())?;
        self.saved = self.editor.text().to_string();
        Ok(())
    }

    /// Writes the file to a new path and keeps editing that one.
    pub fn save_as(&mut self, path: &Path) -> std::io::Result<()> {
        self.path = Some(path.to_path_buf());
        self.save()?;
        self.submitted = None;
        Ok(())
    }

    fn base(&self) -> Option<&Path> {
        self.path.as_deref().and_then(Path::parent)
    }

    fn submit_now(&mut self) {
        let text = self.editor.text().to_string();
        let base = self.base().map(Path::to_path_buf);
        let path = self.path.clone();
        self.worker
            .submit_file(&text, base.as_deref(), path.as_deref());
        self.submitted = Some(self.editor.revision());
    }

    /// Picks up finished evaluations and, once typing has paused, asks for a new
    /// one. Returns how long to wait before calling again, if work is pending.
    pub fn pump(&mut self) -> Option<Duration> {
        if let Some(done) = self.worker.poll() {
            self.eval_text = done.text;
            self.eval = Some(done.eval);
            self.background = Some(done.background);
            self.pending = done.pending;
            self.eval_id += 1;
            self.took_eval();
        }
        if self.submitted != Some(self.editor.revision()) {
            let wait = Duration::from_millis(DEBOUNCE_MS);
            let since = self.last_edit.elapsed();
            if since >= wait {
                self.submit_now();
            } else {
                return Some(wait - since);
            }
        }
        (self.worker.busy() || self.pending).then_some(Duration::from_millis(30))
    }

    /// The evaluated line whose span holds `offset`: its block and index.
    pub fn line_at(&self, offset: usize) -> Option<(mpp_syntax::document::BlockId, usize)> {
        let ev = self.eval.as_ref()?;
        ev.blocks.iter().find_map(|b| {
            b.values
                .iter()
                .position(|v| v.span.start <= offset && offset <= v.span.end)
                .map(|i| (b.id, i))
        })
    }

    /// Ctrl+Enter: writes the answer shown on the line at `offset` into the file as
    /// ordinary maths (a grey automatic answer, or a result shown in a format picked
    /// with a right-click). True when the text changed.
    pub fn keep_result_at(&mut self, offset: usize) -> bool {
        if !self.eval_fresh() {
            return false;
        }
        let Some(ev) = self.eval.clone() else {
            return false;
        };
        let Some((block, i)) = self.line_at(offset) else {
            return false;
        };
        let Some(v) = ev.block(block).and_then(|b| b.values.get(i)) else {
            return false;
        };
        let original = self
            .editor
            .text()
            .get(v.span.start..v.span.end)
            .unwrap_or("")
            .to_string();
        let new = if v.auto {
            ev.keep_result_text(block, i)
        } else if let (mpp_eval::LineKind::Query { .. }, Some(fmt)) =
            (&v.kind, self.show.format_for(v.span).copied())
        {
            let (_, latex) = v.reformat(&fmt);
            match (latex, original.rfind('?')) {
                (Some(l), Some(q)) => {
                    let end = q + 1 + usize::from(original[q + 1..].starts_with('!'));
                    format!("{}{l}{}", &original[..q], &original[end..])
                }
                _ => return false,
            }
        } else {
            return false;
        };
        if new.is_empty() || new == original {
            return false;
        }
        self.editor.replace_node(v.span, &new);
        self.touched();
        true
    }

    /// The evaluator's settings (auto-evaluation, steps, number format). A change
    /// evaluates the file again.
    pub fn set_eval_settings(&mut self, settings: EvalSettings) {
        if settings != self.eval_settings {
            self.eval_settings = settings;
            self.worker.set_settings(settings);
            self.submitted = None;
            if self.show.show_steps != settings.show_steps {
                self.show_mut().show_steps = settings.show_steps;
            }
        }
    }

    /// View → Comments: show the notes, or hide them all (they take no space).
    pub fn set_show_comments(&mut self, show: bool) {
        if self.show.hide_comments == show {
            self.show_mut().hide_comments = !show;
        }
    }

    /// The evaluator's settings.
    pub fn eval_settings(&self) -> EvalSettings {
        self.eval_settings
    }

    /// True while automatic answers are still being worked out (spinners show).
    pub fn pending(&self) -> bool {
        self.pending
    }

    /// The page laid out last, if any.
    pub fn page_cache(&self) -> Option<&Arc<PageCache>> {
        self.cache.as_ref()
    }

    /// The view's display choices.
    pub fn show(&self) -> &ShowState {
        &self.show
    }

    /// Changes the view's display choices (the page is laid out again).
    pub fn show_mut(&mut self) -> &mut ShowState {
        self.show_version += 1;
        &mut self.show
    }

    /// A new evaluation arrived: rebuild the figures and their slider rows.
    fn took_eval(&mut self) {
        if let Some(ev) = self.eval.clone() {
            self.figures.sync(&ev);
            let rows = self.figures.rows();
            let threes = self.figures.three_d();
            if rows != self.show.figure_rows || threes != self.show.figure_3d {
                let show = self.show_mut();
                show.figure_rows = rows;
                show.figure_3d = threes;
            }
        }
    }

    /// Notes that the text changed just now (restarts the debounce).
    pub fn touched(&mut self) {
        self.last_edit = Instant::now();
    }

    /// True while an evaluation is running or waiting to start.
    pub fn evaluating(&self) -> bool {
        self.worker.busy() || self.submitted != Some(self.editor.revision())
    }

    /// Runs the evaluation for the current text now and waits for it.
    pub fn settle(&mut self) {
        if self.submitted != Some(self.editor.revision()) {
            self.submit_now();
        }
        if let Some(done) = self.worker.wait(Duration::from_secs(30)) {
            self.eval_text = done.text;
            self.eval = Some(done.eval);
            self.background = Some(done.background);
            self.pending = done.pending;
            self.eval_id += 1;
            self.took_eval();
        }
        self.cache = None;
        self.spare = None;
    }

    /// True when the evaluation is of exactly the text on screen.
    pub fn eval_fresh(&self) -> bool {
        self.eval.is_some() && self.eval_text == self.editor.text()
    }

    /// How long since the text was last touched.
    pub fn idle_for(&self) -> Duration {
        self.last_edit.elapsed()
    }

    /// Problems of imported files and the names other files define, from the
    /// last evaluation.
    pub fn background(&self) -> Option<&Background> {
        self.background.as_deref()
    }

    /// The problems of the current text (laid out at the last page size when
    /// the page is not built yet).
    pub fn problems(&mut self) -> Vec<Problem> {
        let (w, s, m) = self.last_layout;
        self.page(w, s, m).problems.clone()
    }

    /// The error, warning and note counts of the last laid-out page.
    pub fn problem_counts(&self) -> Counts {
        self.cache
            .as_ref()
            .map(|c| Counts::of(&c.problems))
            .unwrap_or_default()
    }

    /// The number of errors in the current evaluation and parse.
    pub fn error_count(&self) -> usize {
        self.problem_counts().errors
    }

    /// The template the caret is filling in, while it still has empty boxes.
    fn quiet_range(&self) -> Option<(usize, usize)> {
        let caret = self.editor.caret();
        let s = self.editor.session_at(caret)?;
        s.tab_boxes()
            .any(|b| b.range.is_empty())
            .then_some((s.range.start, s.range.end))
    }

    /// The laid-out page for a window width, rebuilt only when something changed.
    pub fn page(&mut self, width: f32, size: f32, mode: Mode) -> Arc<PageCache> {
        let key = CacheKey {
            revision: self.editor.revision(),
            eval_id: self.eval_id,
            width: width.to_bits(),
            size: size.to_bits(),
            mode,
            limits: self.integral_limits,
            quiet: self.quiet_range(),
            show: self.show_version,
        };
        self.last_layout = (width, size, mode);
        if self.cache.as_ref().is_some_and(|c| c.key != key) {
            let old = self.cache.take();
            let spare = self.spare.take();
            // A second view of this file asks for its own width: keep the other.
            self.cache = spare.filter(|s| s.key == key);
            self.spare =
                old.filter(|o| o.key.revision == key.revision && o.key.eval_id == key.eval_id);
        }
        let editor = &self.editor;
        let eval = self.eval.as_deref();
        let fresh = self.eval.is_some() && self.eval_text == editor.text();
        let limits = self.integral_limits;
        let background = self.background.clone();
        let show = &self.show;
        let base = self.path.as_deref().and_then(Path::parent);
        self.cache
            .get_or_insert_with(|| {
                let src = editor.text();
                let doc = parse_document(src);
                let labels = editor.slot_labels();
                let extra_refs = crate::page::ExtraRefs::read(&doc, base);
                let input = PageInput {
                    src,
                    doc: &doc,
                    eval,
                    theme: mode.theme(),
                    size,
                    width,
                    integral_limits: limits,
                    slot_labels: &labels,
                    show,
                    extra_refs: &extra_refs,
                };
                let page = build_page(&input);
                let inputs = diagnostics::Inputs {
                    src,
                    doc: &doc,
                    eval,
                    eval_fresh: fresh,
                    background: background.as_deref(),
                    quiet: key.quiet.map(|(a, b)| a..b),
                };
                let problems = diagnostics::collect(&inputs, &page);
                Arc::new(PageCache {
                    key,
                    page,
                    doc,
                    problems,
                    rows: show.figure_rows.clone(),
                })
            })
            .clone()
    }
}
