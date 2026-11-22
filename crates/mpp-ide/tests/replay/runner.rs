//! The script runner: a person at the keyboard, one key per frame, through the
//! real frame loop (`IdeApp::ui`, which pumps the evaluator). A pause is wall-clock
//! time in which frames run only when the app asks for one, exactly like the
//! desktop event loop, so a missing repaint shows up as a failure. After every
//! frame the runner hands what the screen shows ([`Seen`]) to the script's check.

use std::sync::Mutex;
use std::time::{Duration, Instant};

use egui::{Event, Key, Modifiers, Pos2, Rect, Vec2};
use egui_kittest::kittest::Queryable;
use egui_kittest::{Harness, SnapshotOptions};
use mpp_ide::layout::tiles::{self, Pane};
use mpp_ide::page::{mode, BlockTag, LinkTarget};
use mpp_ide::{theme, IdeApp};
use mpp_syntax::document::{parse_document, BlockKind};
use mpp_syntax::Span;

pub type H = Harness<'static, IdeApp>;

pub const SIZE: Vec2 = Vec2::new(1180.0, 820.0);

pub fn harness(app: IdeApp) -> H {
    Harness::builder()
        .with_size(SIZE)
        .with_max_steps(40)
        .with_context_setup(theme::install_fonts)
        .build_ui_state(|ui, app: &mut IdeApp| app.ui(ui), app)
}

/// A new file holding `text`, with the caret at its end.
pub fn app_with(text: &str) -> IdeApp {
    let mut app = IdeApp::empty();
    app.new_file();
    let doc = app.doc_mut().expect("doc");
    doc.editor.set_text(text);
    doc.editor.move_caret(text.len(), false);
    doc.scroll_to = Some(Span::new(text.len(), text.len()));
    app
}

/// A new, empty file, drawn once.
pub fn fresh() -> H {
    let mut app = IdeApp::empty();
    app.new_file();
    let mut h = harness(app);
    h.run_steps(2);
    h
}

/// One key or piece of text, then the frame the input causes.
pub fn type_text(h: &mut H, s: &str) {
    for c in s.chars() {
        h.event(Event::Text(c.to_string()));
        h.step();
    }
}

pub fn press(h: &mut H, mods: Modifiers, key: Key) {
    h.key_press_modifiers(mods, key);
    h.step();
}

pub fn text_of(h: &H) -> String {
    h.state().doc().expect("doc").editor.text().to_string()
}

/// The person stops. Frames run only while the app asks for them, for at most `limit`.
pub fn idle(h: &mut H, limit: Duration) {
    let start = Instant::now();
    while start.elapsed() < limit {
        if !h.ctx.has_requested_repaint() {
            return;
        }
        std::thread::sleep(Duration::from_millis(10));
        h.step();
    }
}

/// Where the caret was drawn in the last frame (its middle, in screen pixels).
pub fn caret_at(h: &H) -> Pos2 {
    h.state()
        .doc()
        .and_then(|d| d.caret_screen)
        .expect("the caret is drawn")
        .center()
}

pub fn click_at(h: &mut H, pos: Pos2) {
    h.event(Event::PointerMoved(pos));
    for pressed in [true, false] {
        h.event(Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: Modifiers::NONE,
        });
        h.step();
    }
    h.step();
}

/// One thing the person does.
#[derive(Debug, Clone, Copy)]
pub enum Step {
    /// Typed one character per frame.
    Type(&'static str),
    Key(Key),
    /// Hands off the keyboard: long enough for the pause before a run, and until
    /// the answers are on screen (or the app stops asking for frames).
    Pause,
}

/// What one frame showed.
#[derive(Debug, Clone)]
pub struct Seen {
    pub text: String,
    /// The caret's byte offset, and where it was drawn (its middle, on screen).
    pub caret: usize,
    pub caret_at: Option<Pos2>,
    /// Where the caret's own stop is on the page (its middle, on screen): the
    /// place the next character goes.
    pub stop_at: Option<Pos2>,
    /// Every block: its source span and its top on screen.
    pub tops: Vec<(Span, f32)>,
    /// What the page draws, block by block.
    pub visible: Vec<String>,
    /// Every answer on the page (what a click copies).
    pub answers: Vec<String>,
    /// The page drawn is of the text in the editor (not one frame behind).
    pub drawn_now: bool,
    /// The status bar says "Working…".
    pub working: bool,
    /// Maths boxes, each with a ∑ marker (a run of maths lines shares one).
    pub bands: usize,
    /// Maths lines (blocks) on the page.
    pub maths: usize,
}

impl Seen {
    pub fn of(h: &H) -> Seen {
        let st = h.state();
        let doc = st.doc().expect("doc");
        let pc = doc.page_cache().expect("a page");
        let origin = st.diag.origin;
        let caret = doc.editor.caret();
        let stop_at = pc
            .page
            .caret_stop_on(caret, doc.caret_side)
            .map(|s| origin + Vec2::new(s.x, (s.top + s.bottom) / 2.0));
        Seen {
            text: doc.editor.text().to_string(),
            caret,
            caret_at: doc.caret_screen.map(|r| r.center()),
            stop_at,
            tops: pc
                .page
                .blocks
                .iter()
                .map(|b| (b.span, origin.y + b.top))
                .collect(),
            visible: pc.page.blocks.iter().map(|b| b.dl.text()).collect(),
            answers: answers(h),
            drawn_now: pc.revision() == doc.editor.revision(),
            working: h.query_all_by_label_contains("Working").next().is_some(),
            bands: mode::bands(&pc.page, st.size).len(),
            maths: pc
                .page
                .blocks
                .iter()
                .filter(|b| b.tag == BlockTag::Math)
                .count(),
        }
    }

    /// Lists in the parse of the text.
    pub fn lists(&self) -> usize {
        parse_document(&self.text)
            .blocks
            .iter()
            .filter(|b| matches!(b.kind, BlockKind::List(_)))
            .count()
    }
}

/// Every answer on the page: what a click on it copies.
pub fn answers(h: &H) -> Vec<String> {
    let doc = h.state().doc().expect("doc");
    let Some(pc) = doc.page_cache() else {
        return vec![];
    };
    pc.page
        .links
        .iter()
        .filter_map(|l| match &l.link.target {
            LinkTarget::Answer(t) => Some(t.clone()),
            _ => None,
        })
        .collect()
}

/// What happened in a frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrameKind {
    /// The frame a key or character caused.
    Input,
    /// A frame in a pause (no input).
    Idle,
}

/// What a script run measured.
#[derive(Debug, Default)]
pub struct Run {
    pub frames: usize,
    /// How long each input frame took (the whole `ui()`, pump included).
    pub key_frames: Vec<Duration>,
}

/// The longest a pause waits for the answers.
const PAUSE_LIMIT: Duration = Duration::from_secs(15);

/// Runs a script. `check` sees every frame: the step that caused it, the kind
/// of frame, and what the screen showed.
pub fn run(h: &mut H, steps: &[Step], mut check: impl FnMut(&Step, FrameKind, &Seen)) -> Run {
    let mut out = Run::default();
    let input = |h: &mut H, step: &Step, ev: Option<Event>, out: &mut Run| {
        match (step, ev) {
            (_, Some(ev)) => h.event(ev),
            (Step::Key(k), None) => h.key_press(*k),

            _ => {}
        }
        let t = Instant::now();
        h.step();
        out.key_frames.push(t.elapsed());
        out.frames += 1;
    };
    for step in steps {
        match step {
            Step::Type(s) => {
                for c in s.chars() {
                    input(h, step, Some(Event::Text(c.to_string())), &mut out);
                    check(step, FrameKind::Input, &Seen::of(h));
                }
            }
            Step::Key(_) => {
                input(h, step, None, &mut out);
                check(step, FrameKind::Input, &Seen::of(h));
            }
            Step::Pause => {
                // The app asks for frames now and then even when nothing changes
                // (autosave looks at dirty files twice a second), so the pause
                // ends once the answers are in and the screen has stayed the same.
                let start = Instant::now();
                let debounce = Duration::from_millis(mpp_ide::layout::DEBOUNCE_MS * 3);
                let mut same = 0;
                let mut last: Option<String> = None;
                loop {
                    std::thread::sleep(Duration::from_millis(15));
                    let done = h.state().doc().is_some_and(|d| d.page_is_current());
                    let quiet = !h.ctx.has_requested_repaint() || same >= 5;
                    if (start.elapsed() >= debounce && done && quiet)
                        || start.elapsed() > PAUSE_LIMIT
                    {
                        break;
                    }
                    if h.ctx.has_requested_repaint() {
                        h.step();
                        out.frames += 1;
                        let seen = Seen::of(h);
                        check(step, FrameKind::Idle, &seen);
                        let sig = format!(
                            "{:?}{:?}{:?}{}",
                            seen.caret_at, seen.tops, seen.answers, seen.working
                        );
                        same = if last.as_ref() == Some(&sig) {
                            same + 1
                        } else {
                            0
                        };
                        last = Some(sig);
                    }
                }
            }
        }
    }
    out
}

/// The rules every single frame of a person typing keeps: the page is of the
/// text typed, the caret is drawn where the next character goes, a pause never
/// moves it, the lines above the one being typed on stay still, and there is
/// never a list, a "Working…" or a raw `$` on screen.
pub struct Watch {
    /// The person only ever types at the end of the file.
    at_end: bool,
    /// The block holding the caret and the screen tops of the blocks above it,
    /// from the first frame on the current line.
    above: Option<(usize, Vec<(Span, f32)>)>,
    /// Where the caret was in the previous frame, and the text then.
    last: Option<(String, egui::Pos2)>,
    /// Lines above may move when an answer arrives (see
    /// `worksheets::a_fraction_answer_moves_nothing`).
    answers_may_move: bool,
}

impl Watch {
    /// For a person typing at the end of the file.
    pub fn at_end() -> Watch {
        Watch {
            at_end: true,
            above: None,
            last: None,
            answers_may_move: false,
        }
    }

    /// For a person typing anywhere.
    pub fn new() -> Watch {
        Watch {
            at_end: false,
            above: None,
            last: None,
            answers_may_move: false,
        }
    }

    /// Lets the lines above move in a pause, when answers arrive: only the
    /// frames that keys cause are held to it.
    pub fn answers_may_move(mut self) -> Watch {
        self.answers_may_move = true;
        self
    }

    /// Checks one frame.
    pub fn frame(&mut self, step: &Step, kind: FrameKind, s: &Seen) {
        let at = format!("{step:?} ({kind:?}) with {:?}", s.text);
        // The page drawn is of the text typed: every key shows in its own frame.
        assert!(s.drawn_now, "the page was a frame behind after {at}");
        // The caret is drawn on the line where the next character goes, below
        // every line that ends before it (at the end of the file, when the
        // person only ever types there).
        if self.at_end {
            assert_eq!(s.caret, s.text.len(), "the caret left the end after {at}");
        }
        let caret_at = s
            .caret_at
            .unwrap_or_else(|| panic!("the caret was not drawn after {at}"));
        let stop = s.stop_at.expect("the caret has a stop");
        assert!(
            (stop.y - caret_at.y).abs() < 3.0 && (stop.x - caret_at.x).abs() < 3.0,
            "the caret was drawn at {caret_at:?} but the next character goes at {stop:?} after {at}"
        );
        for (span, top) in &s.tops {
            if span.end < s.caret {
                assert!(
                    *top < caret_at.y,
                    "the caret was drawn above an earlier line after {at}"
                );
            }
        }
        // A pause never moves the caret.
        if let (FrameKind::Idle, Some((t, p))) = (kind, &self.last) {
            if *t == s.text {
                assert!(
                    (p.x - caret_at.x).abs() < 0.5 && (p.y - caret_at.y).abs() < 0.5,
                    "the caret moved by itself: {p:?} -> {caret_at:?} after {at}"
                );
            }
        }
        self.last = Some((s.text.clone(), caret_at));
        // Lines above the one being typed on stay where they are on screen.
        // Enter starts a new line (and may turn the finished one into maths).
        let line = s.tops.iter().filter(|(sp, _)| sp.end < s.caret).count();
        let above: Vec<_> = s
            .tops
            .iter()
            .filter(|(sp, _)| sp.end < s.caret)
            .copied()
            .collect();
        match &self.above {
            _ if self.answers_may_move && kind == FrameKind::Idle => {
                self.above = Some((line, above));
            }
            Some((l, before)) if *l == line && !matches!(step, Step::Key(Key::Enter)) => {
                for ((sa, ta), (_, tb)) in before.iter().zip(&above) {
                    assert!(
                        (ta - tb).abs() < 0.5,
                        "the line at {sa:?} moved from y={ta} to y={tb} after {at}"
                    );
                }
            }
            _ => self.above = Some((line, above)),
        }
        // Never a list, never "Working…", nothing raw on the page.
        assert_eq!(s.lists(), 0, "a list appeared after {at}");
        assert!(!s.working, "the status bar said Working… after {at}");
        for v in &s.visible {
            assert!(!v.contains('$'), "a raw $ on the page after {at}: {v}");
        }
    }
}

/// Rendering is done one test at a time (the GPU device is shared).
pub static RENDER: Mutex<()> = Mutex::new(());

/// The editor's page on screen, compared against `tests/snapshots/<name>.png`
/// with a small tolerance (`UPDATE_SNAPSHOTS=1` writes it).
pub fn snapshot(h: &mut H, name: &str) {
    let _one = RENDER
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let page = h
        .state()
        .ws
        .tiles
        .tree
        .as_ref()
        .and_then(|t| {
            let id = tiles::find_by(t, |p| matches!(p, Pane::Equation { .. }))?;
            t.tiles.rect(id)
        })
        .unwrap_or(Rect::from_min_size(Pos2::ZERO, SIZE));
    let ppp = h.ctx.pixels_per_point();
    let img = h.render().expect("render");
    let clip = Rect::from_min_size(Pos2::ZERO, SIZE).intersect(page);
    let (x, y) = ((clip.left() * ppp) as u32, (clip.top() * ppp) as u32);
    let (w, hgt) = ((clip.width() * ppp) as u32, (clip.height() * ppp) as u32);
    let img = image::imageops::crop_imm(&img, x, y, w, hgt).to_image();
    let opts = SnapshotOptions::new().threshold(2.0).max_failed_pixels(120);
    egui_kittest::try_image_snapshot_options(&img, name, &opts)
        .unwrap_or_else(|e| panic!("{name}: {e}"));
}
