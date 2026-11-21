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
    /// The caret is in the title block (its empty fields show their labels).
    title: bool,
    /// Bumped when a new proof check arrives.
    proof: u64,
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
    pub page: Arc<Page>,
    /// What the page shows from the evaluation (see [`crate::page::eval_view`]).
    view: u64,
    pub doc: Arc<Document>,
    pub problems: Vec<Problem>,
    /// Slider rows under each figure, as laid out.
    rows: Vec<(mpp_syntax::document::BlockId, usize)>,
    /// The proof statuses drawn on this page (preview only).
    pub proof: crate::proof::ProofView,
}

/// Modification time and size of a file (`None` when it cannot be read).
type FileStamp = Option<(Option<std::time::SystemTime>, u64)>;

/// How often the imported files are looked at on disk.
const IMPORT_POLL: Duration = Duration::from_secs(1);

fn stamp_of(path: &Path) -> FileStamp {
    let m = std::fs::metadata(path).ok()?;
    Some((m.modified().ok(), m.len()))
}

static NEXT_DOC_ID: AtomicU64 = AtomicU64::new(1);

/// Frees a large value (the old page, parse or evaluation of a long file) on a thread
/// of its own: freeing a page with hundreds of thousands of caret stops and thousands
/// of problems takes milliseconds that a keystroke should not wait for.
fn drop_later<T: Send + 'static>(value: T) {
    // Without a thread the value is freed here, as it would have been.
    let _ = std::thread::Builder::new()
        .name("mpp-free".into())
        .spawn(move || drop(value));
}

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
    /// The files the last evaluation imported, with how they looked when it started
    /// (checked about once a second, so a file saved from another tab shows here).
    imports: Vec<(PathBuf, FileStamp)>,
    imports_checked: Instant,
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
    /// The proof checker's thread and its newest report, with the text it checked.
    proof_worker: crate::proof::ProofWorker,
    pub proof: Option<Arc<mpp_proof::DocumentCheck>>,
    proof_text: String,
    proof_id: u64,
    /// The parse of the text at a revision, shared by the page, the outline and the
    /// proof view (a 3000-line file takes milliseconds to parse).
    parsed: std::sync::Mutex<Option<(u64, Arc<Document>)>>,
    /// The files the text links to and whether each exists, at a revision and a time
    /// (the explorer and the status bar ask every frame).
    links: std::sync::Mutex<Option<LinkMemo>>,
}

/// See [`Doc::link_targets`].
#[derive(Debug)]
struct LinkMemo {
    revision: u64,
    checked: Instant,
    targets: Arc<Vec<(PathBuf, bool)>>,
}

/// How long the existence of linked files is trusted before it is checked again.
const LINK_RECHECK: Duration = Duration::from_secs(1);

impl Doc {
    /// The parse of the current text (parsed once per revision).
    pub fn parsed(&self) -> Arc<Document> {
        let rev = self.editor.revision();
        let mut memo = self
            .parsed
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some((r, d)) = memo.as_ref() {
            if *r == rev {
                return d.clone();
            }
        }
        let d = Arc::new(parse_document(self.editor.text()));
        drop_later(memo.replace((rev, d.clone())));
        d
    }

    /// The files the text imports or links to (resolved against its folder), each with
    /// whether it exists. Kept per revision, and the existence for a second, so a
    /// file with thousands of links is not scanned on every frame.
    pub fn link_targets(&self) -> Arc<Vec<(PathBuf, bool)>> {
        let Some(dir) = self.path.as_deref().and_then(Path::parent) else {
            return Arc::default();
        };
        let rev = self.editor.revision();
        let mut memo = self
            .links
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(m) = memo.as_ref() {
            if m.revision == rev && m.checked.elapsed() < LINK_RECHECK {
                return m.targets.clone();
            }
        }
        let paths: Vec<PathBuf> = match memo.as_ref() {
            Some(m) if m.revision == rev => m.targets.iter().map(|(p, _)| p.clone()).collect(),
            _ => crate::explorer::links::targets(self.editor.text(), dir),
        };
        // After an edit, links whose file was looked up less than a second ago keep
        // that answer (a file with hundreds of links is not checked on disk again for
        // every keystroke).
        let known: std::collections::HashMap<&Path, bool> = memo
            .as_ref()
            .filter(|m| m.revision != rev && m.checked.elapsed() < LINK_RECHECK)
            .map(|m| m.targets.iter().map(|(p, e)| (p.as_path(), *e)).collect())
            .unwrap_or_default();
        let targets: Arc<Vec<(PathBuf, bool)>> = Arc::new(
            paths
                .into_iter()
                .map(|p| {
                    let exists = known
                        .get(p.as_path())
                        .copied()
                        .unwrap_or_else(|| p.is_file());
                    (p, exists)
                })
                .collect(),
        );
        let checked = memo
            .as_ref()
            .filter(|_| !known.is_empty())
            .map_or_else(Instant::now, |m| m.checked);
        drop(known);
        *memo = Some(LinkMemo {
            revision: rev,
            checked,
            targets: targets.clone(),
        });
        targets
    }

    /// The error, warning and note counts of the current text, without copying
    /// the problems (laid out at the last page size when the page is not built yet).
    pub fn problem_counts_now(&mut self) -> Counts {
        let (w, s, m) = self.last_layout;
        Counts::of(&self.page(w, s, m).problems)
    }

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
            imports: Vec::new(),
            imports_checked: Instant::now(),
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
            proof_worker: crate::proof::ProofWorker::new(),
            proof: None,
            proof_text: String::new(),
            parsed: std::sync::Mutex::new(None),
            links: std::sync::Mutex::new(None),
            proof_id: 0,
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
        let text = mpp_eval::read_text(path)?;
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
        // The parse the page made of this very text, if there is one.
        let parsed = self
            .parsed
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .as_ref()
            .filter(|(rev, _)| *rev == self.editor.revision())
            .map(|(_, d)| d.clone());
        // The imported files as they are before this run reads them.
        self.imports = self
            .eval
            .as_ref()
            .map(|ev| ev.imported_files())
            .unwrap_or_default()
            .into_iter()
            .map(|f| {
                let st = stamp_of(&f);
                (f, st)
            })
            .collect();
        self.imports_checked = Instant::now();
        self.worker
            .submit_parsed(&text, base.as_deref(), path.as_deref(), parsed);
        self.proof_worker.submit(&text, base.as_deref());
        self.submitted = Some(self.editor.revision());
    }

    fn took_proof(&mut self, done: crate::proof::ProofDone) {
        self.proof = Some(done.check);
        self.proof_text = done.text;
        self.proof_id += 1;
    }

    /// True when the proof report is of exactly the text on screen.
    pub fn proof_fresh(&self) -> bool {
        self.proof.is_some() && self.proof_text == self.editor.text()
    }

    /// What the page draws from the newest proof report.
    pub fn proof_view(&self) -> crate::proof::ProofView {
        match &self.proof {
            Some(c) => {
                let doc = self.parsed();
                crate::proof::ProofView::new(&doc, c, self.proof_fresh())
            }
            None => crate::proof::ProofView::default(),
        }
    }

    /// A number that changes whenever a new proof report arrives.
    pub fn proof_generation(&self) -> u64 {
        self.proof_id
    }

    /// Picks up finished evaluations and, once typing has paused, asks for a new
    /// one. Returns how long to wait before calling again, if work is pending.
    pub fn pump(&mut self) -> Option<Duration> {
        // Anything new that arrived is drawn in the very next frame.
        let mut arrived = false;
        if let Some(done) = self.proof_worker.poll() {
            self.took_proof(done);
            arrived = true;
        }
        if let Some(done) = self.worker.poll() {
            arrived = true;
            self.eval_text = done.text;
            drop_later(self.eval.replace(done.eval));
            self.background = Some(done.background);
            self.pending = done.pending;
            self.eval_id += 1;
            self.took_eval();
        }
        if self.submitted == Some(self.editor.revision())
            && !self.worker.busy()
            && self.imports_checked.elapsed() >= IMPORT_POLL
        {
            self.imports_checked = Instant::now();
            if self.imports.iter().any(|(f, st)| stamp_of(f) != *st) {
                // An imported file changed on disk: run the file again.
                self.submitted = None;
                self.last_edit = Instant::now() - Duration::from_millis(DEBOUNCE_MS);
            }
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
        if arrived {
            return Some(Duration::ZERO);
        }
        if self.worker.busy() || self.pending || self.proof_worker.busy() {
            return Some(Duration::from_millis(30));
        }
        (!self.imports.is_empty())
            .then(|| IMPORT_POLL.saturating_sub(self.imports_checked.elapsed()))
    }

    /// After an evaluation: the files it imported that were not looked at yet are
    /// stamped now.
    fn watch_imports(&mut self) {
        let Some(ev) = self.eval.clone() else {
            return;
        };
        let files = ev.imported_files();
        self.imports.retain(|(f, _)| files.contains(f));
        for f in files {
            if !self.imports.iter().any(|(g, _)| *g == f) {
                let st = stamp_of(&f);
                self.imports.push((f, st));
            }
        }
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
        self.watch_imports();
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
        self.worker.evaluating() || self.submitted != Some(self.editor.revision())
    }

    /// Runs the evaluation for the current text now and waits for it.
    pub fn settle(&mut self) {
        if self.submitted != Some(self.editor.revision()) {
            self.submit_now();
        }
        if let Some(done) = self.worker.wait(Duration::from_secs(30)) {
            self.eval_text = done.text;
            drop_later(self.eval.replace(done.eval));
            self.background = Some(done.background);
            self.pending = done.pending;
            self.eval_id += 1;
            self.took_eval();
        }
        if let Some(done) = self.proof_worker.wait(Duration::from_secs(60)) {
            self.took_proof(done);
        }
        // The page is laid out again for the new evaluation (its key has changed),
        // reusing the layout when the evaluation shows the same on it.
        self.spare = None;
    }

    /// True when the last page drawn shows the newest evaluation, and that
    /// evaluation is of the text on screen (what a person sees is up to date).
    pub fn page_is_current(&self) -> bool {
        self.eval_fresh()
            && self.cache.as_ref().is_some_and(|c| {
                c.key.eval_id == self.eval_id && c.key.revision == self.editor.revision()
            })
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
            title: caret_in_title(self.editor.text(), self.editor.caret()),
            show: self.show_version,
            proof: self.proof_id,
        };
        self.last_layout = (width, size, mode);
        let mut earlier: Option<Arc<PageCache>> = None;
        if self.cache.as_ref().is_some_and(|c| c.key != key) {
            let old = self.cache.take();
            let spare = self.spare.take();
            // A second view of this file asks for its own width: keep the other.
            self.cache = spare.filter(|s| s.key == key);
            // The same page for an older evaluation of the same text: if the new one
            // shows the same on the page, only its problems are found again.
            earlier = old.clone().filter(|o| {
                CacheKey {
                    eval_id: key.eval_id,
                    proof: key.proof,
                    ..o.key
                } == key
            });
            let (keep, gone): (Vec<_>, Vec<_>) = old
                .into_iter()
                .partition(|o| o.key.revision == key.revision && o.key.eval_id == key.eval_id);
            self.spare = keep.into_iter().next();
            drop_later(gone);
        }
        let editor = &self.editor;
        let eval = self.eval.as_deref();
        let fresh = self.eval.is_some() && self.eval_text == editor.text();
        let limits = self.integral_limits;
        let background = self.background.clone();
        let mut show = self.show.clone();
        show.editing_title = key.title;
        let show = &show;
        let base = self.path.as_deref().and_then(Path::parent);
        let proof = self.proof.clone();
        let proof_fresh = self.proof.is_some() && self.proof_text == editor.text();
        let parsed = if self.cache.is_none() {
            Some(
                earlier
                    .as_ref()
                    .map_or_else(|| self.parsed(), |e| e.doc.clone()),
            )
        } else {
            None
        };
        self.cache
            .get_or_insert_with(|| {
                let src = editor.text();
                let doc = parsed.unwrap_or_else(|| Arc::new(parse_document(src)));
                let proof_view = proof
                    .as_deref()
                    .map(|c| crate::proof::ProofView::new(&doc, c, proof_fresh))
                    .unwrap_or_default();
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
                    proof: &proof_view,
                };
                let view = crate::page::eval_view(&doc, eval, show);
                let inputs = diagnostics::Inputs {
                    src,
                    doc: &doc,
                    eval,
                    eval_fresh: fresh,
                    background: background.as_deref(),
                    quiet: key.quiet.map(|(a, b)| a..b),
                    proof: &proof_view.steps,
                };
                let reuse = earlier
                    .as_ref()
                    .filter(|e| e.view == view && e.proof == proof_view);
                // The problems are gathered on a thread of their own while the page is
                // laid out (in a long file each takes milliseconds), then placed on it.
                let (page, found) = std::thread::scope(|scope| {
                    let gathering = scope.spawn(|| diagnostics::gather(&inputs));
                    let page = match reuse {
                        Some(e) => e.page.clone(),
                        None => Arc::new(build_page(&input)),
                    };
                    let found = gathering
                        .join()
                        .unwrap_or_else(|_| diagnostics::gather(&inputs));
                    (page, found)
                });
                drop_later(earlier);
                let problems = diagnostics::place(found, &page, src);
                Arc::new(PageCache {
                    key,
                    page,
                    view,
                    doc,
                    problems,
                    rows: show.figure_rows.clone(),
                    proof: proof_view,
                })
            })
            .clone()
    }
}

/// True when the caret is on one of the title-block lines at the top of the file
/// (`Title:`, `Author:`, `Date:`, `Abstract:`), or just after them.
fn caret_in_title(text: &str, caret: usize) -> bool {
    let mut at = 0;
    let mut seen = false;
    for line in text.split_inclusive('\n') {
        let t = line.trim_start();
        let field = ["Title:", "Author:", "Date:", "Abstract:"]
            .iter()
            .any(|k| t.starts_with(k));
        if !field {
            break;
        }
        seen = true;
        let end = at + line.trim_end_matches(['\n', '\r']).len();
        if caret >= at && caret <= end {
            return true;
        }
        at += line.len();
    }
    seen && caret == at && at == text.len()
}

#[cfg(test)]
mod title_tests {
    use super::caret_in_title;

    #[test]
    fn the_caret_is_in_the_title_block_only_on_its_lines() {
        let t = "Title: Waves\nAuthor:\nDate:\n\nSome prose.\n";
        assert!(caret_in_title(t, 3));
        assert!(caret_in_title(t, 19));
        assert!(!caret_in_title(t, t.len() - 3));
        assert!(!caret_in_title("Some prose.", 2));
    }
}
