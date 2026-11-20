//! One open document: its text, its evaluation and its laid-out page.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use mpp_eval::Evaluated;
use mpp_syntax::document::{parse_document, Document};
use mpp_syntax::Span;

use crate::diagnostics::{self, Problem};
use crate::editor::Editor;
use crate::eval::EvalWorker;
use crate::layout::DEBOUNCE_MS;
use crate::page::{build_page, Page, PageInput};
use crate::theme::Mode;

#[derive(Debug, Clone, Copy, PartialEq)]
struct CacheKey {
    revision: u64,
    eval_id: u64,
    width: u32,
    size: u32,
    mode: Mode,
}

/// A laid-out page with the parse it came from and its problems.
#[derive(Debug)]
pub struct PageCache {
    key: CacheKey,
    pub page: Page,
    pub doc: Document,
    pub problems: Vec<Problem>,
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
    eval_text: String,
    eval_id: u64,
    submitted: Option<u64>,
    last_edit: Instant,
    cache: Option<Arc<PageCache>>,
    /// A span to highlight (the target of go-to-definition).
    pub highlight: Option<Span>,
    /// A span to scroll into view on the next frame.
    pub scroll_to: Option<Span>,
    /// The x the caret keeps while moving up and down.
    pub want_x: Option<f32>,
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
            eval_text: String::new(),
            eval_id: 0,
            submitted: None,
            last_edit: Instant::now(),
            cache: None,
            highlight: None,
            scroll_to: None,
            want_x: None,
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

    /// The problems of the current page (empty before the first layout).
    pub fn problems(&self) -> Vec<Problem> {
        self.cache
            .as_ref()
            .map(|c| c.problems.clone())
            .unwrap_or_default()
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
        self.worker.submit(&text, base.as_deref());
        self.submitted = Some(self.editor.revision());
    }

    /// Picks up finished evaluations and, once typing has paused, asks for a new
    /// one. Returns how long to wait before calling again, if work is pending.
    pub fn pump(&mut self) -> Option<Duration> {
        if let Some(done) = self.worker.poll() {
            self.eval_text = done.text;
            self.eval = Some(done.eval);
            self.eval_id += 1;
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
        self.worker.busy().then_some(Duration::from_millis(30))
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
            self.eval_id += 1;
        }
        self.cache = None;
    }

    /// True when the evaluation is of exactly the text on screen.
    pub fn eval_fresh(&self) -> bool {
        self.eval.is_some() && self.eval_text == self.editor.text()
    }

    /// The number of errors in the current evaluation and parse.
    pub fn error_count(&self) -> usize {
        self.cache
            .as_ref()
            .map(|c| {
                c.problems
                    .iter()
                    .filter(|p| p.severity == mpp_syntax::Severity::Error)
                    .count()
            })
            .unwrap_or(0)
    }

    /// The laid-out page for a window width, rebuilt only when something changed.
    pub fn page(&mut self, width: f32, size: f32, mode: Mode) -> Arc<PageCache> {
        let key = CacheKey {
            revision: self.editor.revision(),
            eval_id: self.eval_id,
            width: width.to_bits(),
            size: size.to_bits(),
            mode,
        };
        if self.cache.as_ref().is_some_and(|c| c.key != key) {
            self.cache = None;
        }
        let editor = &self.editor;
        let eval = self.eval.as_deref();
        let fresh = self.eval.is_some() && self.eval_text == editor.text();
        self.cache
            .get_or_insert_with(|| {
                let src = editor.text();
                let doc = parse_document(src);
                let input = PageInput {
                    src,
                    doc: &doc,
                    eval,
                    theme: mode.theme(),
                    size,
                    width,
                };
                let page = build_page(&input);
                let problems = diagnostics::collect(&doc, eval, fresh, &page);
                Arc::new(PageCache {
                    key,
                    page,
                    doc,
                    problems,
                })
            })
            .clone()
    }
}
