//! The paper features and notes in the equation view, driven through the real app
//! with egui_kittest: the title block, numbered theorems, citations that jump to
//! their entry, footnote and equation links, formatted comments (styled, folded,
//! hidden, commented out with Ctrl+/, inserted from the Insert menu), the outline,
//! the unknown-key error, and File → Export…. Screenshots go to
//! `target/ide-snapshots/`.

use std::path::{Path, PathBuf};

use egui::{Event, Key, Modifiers, Pos2, Vec2};
use egui_kittest::Harness;
use mpp_ide::explorer::outline::OutlineKind;
use mpp_ide::page::LinkTarget;
use mpp_ide::{theme, IdeApp};

const SIZE: Vec2 = Vec2::new(1280.0, 900.0);

const PAPER: &str = "Title: Projectile motion on a slope
Author: Ben Elmore
Date: 20 November 2026
Abstract: We find how far a projectile launched at speed $v_0$ travels, following [@halliday14] and [@knuth84].

%number sections

# Range on flat ground

Let $g \\equiv 9.81$ and $v_0 \\equiv 20$. The range is given in \\eqref{range}.

$$ R(\\theta) := \\frac{v_0^2 \\sin 2\\theta}{g} \\label{range} $$

% Check the units: $v_0$ in m/s and $g$ in m/s², so $R$ comes out in metres.

**Theorem.** For a fixed launch speed the range is largest when $\\theta = \\pi/4$.

**Proof.** By \\eqref{range} the range grows with $\\sin 2\\theta$, which is at most $1$.[^max] ∎

[^max]: Strictly, for $0 \\le \\theta \\le \\pi/2$.

References
[@knuth84] Knuth, D. E. | The TeXbook | 1984
[@halliday14] Halliday, D.; Resnick, R. | Fundamentals of Physics | 2014
";

fn harness(app: IdeApp) -> Harness<'static, IdeApp> {
    Harness::builder()
        .with_size(SIZE)
        .with_max_steps(40)
        .with_context_setup(theme::install_fonts)
        .build_ui_state(|ui, app: &mut IdeApp| app.ui(ui), app)
}

fn run(h: &mut Harness<'static, IdeApp>) {
    h.run_steps(3);
}

fn settle(h: &mut Harness<'static, IdeApp>) {
    run(h);
    h.state_mut().settle();
    run(h);
}

fn temp_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("mpp-paper-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("dir");
    dir
}

fn open(text: &str, tag: &str) -> Harness<'static, IdeApp> {
    let dir = temp_dir(tag);
    let file = dir.join("paper.mpp");
    std::fs::write(&file, text).expect("write");
    let mut app = IdeApp::empty();
    app.set_theme_choice(theme::ThemeChoice::Light);
    app.open_file(&file, false).expect("open");
    let mut h = harness(app);
    settle(&mut h);
    // Once the side bar has its width, the page is laid out for the final column.
    run(&mut h);
    h
}

fn doc_text(h: &Harness<'static, IdeApp>) -> String {
    h.state()
        .doc()
        .map(|d| d.editor.text().to_string())
        .unwrap_or_default()
}

fn save_png(h: &mut Harness<'static, IdeApp>, name: &str) {
    static RENDER: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let _gpu = RENDER
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let img = h.render().expect("render");
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/ide-snapshots");
    std::fs::create_dir_all(&dir).expect("dir");
    img.save(dir.join(name)).expect("png");
}

fn click(h: &mut Harness<'static, IdeApp>, p: Pos2) {
    h.event(Event::PointerMoved(p));
    h.event(Event::PointerButton {
        pos: p,
        button: egui::PointerButton::Primary,
        pressed: true,
        modifiers: Modifiers::NONE,
    });
    h.event(Event::PointerButton {
        pos: p,
        button: egui::PointerButton::Primary,
        pressed: false,
        modifiers: Modifiers::NONE,
    });
    run(h);
}

/// The links of the laid-out page with their screen centres.
fn links(h: &Harness<'static, IdeApp>) -> Vec<(Pos2, LinkTarget, Option<String>)> {
    let origin = h.state().diag.origin;
    let pc = h
        .state()
        .doc()
        .and_then(|d| d.page_cache().cloned())
        .expect("page");
    pc.page
        .links
        .iter()
        .map(|l| {
            (
                origin + l.rect.center().to_vec2(),
                l.link.target.clone(),
                l.link.tip.clone(),
            )
        })
        .collect()
}

fn page_text(h: &mut Harness<'static, IdeApp>) -> String {
    h.state_mut().visible_text(1000.0)
}

#[test]
fn a_paper_renders_with_title_block_numbers_and_citations() {
    let mut h = open(PAPER, "render");
    let text = page_text(&mut h);
    for want in [
        "Projectile",
        "BenElmore",
        "Abstract",
        "1RangeonflatGround",
        "Theorem1.1.",
        "Proof.",
        "References",
        "[1]",
        "[2]",
        "Halliday",
        "∎",
    ] {
        let squeezed: String = text.chars().filter(|c| !c.is_whitespace()).collect();
        assert!(
            squeezed.to_lowercase().contains(&want.to_lowercase()),
            "{want:?} is on the page: {text}"
        );
    }
    // The file's own markup never shows.
    for raw in [
        "Title:", "[@", "%number", "**", "[^max]", "\\eqref", "% Check",
    ] {
        assert!(!text.contains(raw), "raw {raw:?} is visible: {text}");
    }
    // Numbered in order of first citation: Halliday is [1].
    let ls = links(&h);
    let hall = ls
        .iter()
        .find(|(_, _, tip)| tip.as_deref().is_some_and(|t| t.contains("Halliday")))
        .expect("a citation of Halliday");
    assert!(matches!(hall.1, LinkTarget::Jump(_)));
    save_png(&mut h, "paper-document.png");
}

#[test]
fn clicking_a_citation_or_an_equation_reference_jumps_to_it() {
    let mut h = open(PAPER, "jump");
    let src = doc_text(&h);
    let entry = src.find("[@knuth84] Knuth").unwrap();
    let (p, target, _) = links(&h)
        .into_iter()
        .find(|(_, t, tip)| {
            matches!(t, LinkTarget::Jump(s) if s.start == entry)
                && tip.as_deref().is_some_and(|t| t.contains("Knuth"))
        })
        .expect("a citation link to the Knuth entry");
    click(&mut h, p);
    let caret = h.state().doc().unwrap().editor.caret();
    assert_eq!(caret, entry, "{target:?}");

    let eq = src.find("$$ R(").unwrap();
    let (p, _, _) = links(&h)
        .into_iter()
        .find(|(_, t, _)| matches!(t, LinkTarget::Jump(s) if s.start == eq))
        .expect("an \\eqref link");
    click(&mut h, p);
    let caret = h.state().doc().unwrap().editor.caret();
    assert_eq!(caret, eq);
}

#[test]
fn footnote_marks_show_the_note_on_hover() {
    let mut h = open(PAPER, "footnote");
    let src = doc_text(&h);
    let def = src.find("[^max]:").unwrap();
    let (p, _, tip) = links(&h)
        .into_iter()
        .find(|(_, t, _)| matches!(t, LinkTarget::Jump(s) if s.start == def))
        .expect("a footnote link");
    assert!(tip.unwrap().contains("Strictly"));
    h.event(Event::PointerMoved(p));
    run(&mut h);
    save_png(&mut h, "paper-footnote-hover.png");
}

#[test]
fn the_outline_lists_title_theorems_and_references() {
    let mut h = open(PAPER, "outline");
    let items = h.state_mut().outline_items();
    assert_eq!(items[0].kind, OutlineKind::Title);
    let latex: Vec<&str> = items.iter().map(|i| i.latex.as_str()).collect();
    assert!(latex.iter().any(|l| l.contains("Theorem 1.1")), "{latex:?}");
    assert!(items.iter().any(|i| i.kind == OutlineKind::References));
    // Notes are listed while View → Comments is on.
    assert!(items.iter().any(|i| i.kind == OutlineKind::Note));
    h.state_mut().show_comments = false;
    run(&mut h);
    let items = h.state_mut().outline_items();
    assert!(!items.iter().any(|i| i.kind == OutlineKind::Note));
}

#[test]
fn an_unknown_citation_key_is_an_error_in_place() {
    let text = "As shown in [@nobody].\n\nReferences\n[@knuth84] Knuth | The TeXbook | 1984\n";
    let mut h = open(text, "unknown");
    let problems = h.state_mut().doc_mut().unwrap().problems();
    assert!(
        problems.iter().any(|p| p.message.contains("nobody")),
        "{:?}",
        problems.iter().map(|p| &p.message).collect::<Vec<_>>()
    );
    assert!(page_text(&mut h).contains('?'));
}

const NOTES: &str = "# Notes

A car covers $d := 120$ in $t := 1.5$.

% The answer is in km/h when $d$ is in km and $t$ in hours.

$$ \\frac{d}{t} = ? $$

$$ 2 d = ? $$
";

fn note_blocks(h: &Harness<'static, IdeApp>) -> usize {
    let note = mpp_render::Theme::LIGHT.note_background;
    let pc = h
        .state()
        .doc()
        .and_then(|d| d.page_cache().cloned())
        .unwrap();
    pc.page
        .blocks
        .iter()
        .filter(|b| {
            b.dl.items
                .iter()
                .any(|p| matches!(p, mpp_render::Primitive::Rule { color, .. } if *color == note))
        })
        .count()
}

#[test]
fn comments_are_notes_that_fold_and_hide() {
    let mut h = open(NOTES, "notes");
    assert_eq!(note_blocks(&h), 1, "the comment is a tinted note");
    let text: String = page_text(&mut h).split_whitespace().collect();
    assert!(text.contains("Theansweris"), "{text}");
    assert!(!text.contains('%'), "{text}");
    save_png(&mut h, "notes-open.png");

    // Fold it from its chevron: one line remains. (Lay the page out at the
    // view's width again: reading its text above used another width.)
    run(&mut h);
    let (p, _, _) = links(&h)
        .into_iter()
        .find(|(_, t, _)| matches!(t, LinkTarget::Fold(_)))
        .expect("a fold chevron");
    click(&mut h, p);
    assert_eq!(h.state().doc().unwrap().show().folded.len(), 1);
    let text = page_text(&mut h);
    assert!(
        text.contains('\u{2026}'),
        "folded to its first words: {text}"
    );
    let (_, _, tip) = links(&h)
        .into_iter()
        .find(|(_, t, _)| matches!(t, LinkTarget::Fold(_)))
        .expect("the folded note");
    assert!(tip.unwrap().contains("hours"), "hover shows the whole note");
    save_png(&mut h, "notes-folded.png");

    // View → Comments off: no note at all.
    h.state_mut().show_comments = false;
    run(&mut h);
    assert_eq!(note_blocks(&h), 0);
    assert!(!page_text(&mut h).contains("answer"));
}

#[test]
fn ctrl_slash_comments_a_maths_block_out_and_back() {
    let mut h = open(NOTES, "ctrl-slash");
    let src = doc_text(&h);
    let at = src.find("2 d = ?").unwrap();
    h.state_mut()
        .doc_mut()
        .unwrap()
        .editor
        .move_caret(at, false);
    run(&mut h);
    h.key_press_modifiers(Modifiers::COMMAND, Key::Slash);
    settle(&mut h);
    let text = doc_text(&h);
    assert!(text.contains("%{\n$$ 2 d = ? $$\n%}"), "{text}");
    // Commented out: not evaluated, so it has no answer.
    let ev = h.state().doc().unwrap().eval.clone().unwrap();
    assert_eq!(ev.queries().count(), 1, "only the other query runs");
    assert_eq!(note_blocks(&h), 2, "the maths shows greyed in a note");
    save_png(&mut h, "notes-commented-out-maths.png");
    // And back.
    let at = doc_text(&h).find("2 d = ?").unwrap();
    h.state_mut()
        .doc_mut()
        .unwrap()
        .editor
        .move_caret(at, false);
    h.key_press_modifiers(Modifiers::COMMAND, Key::Slash);
    settle(&mut h);
    assert_eq!(doc_text(&h), NOTES);
}

#[test]
fn insert_comment_starts_a_note() {
    let mut h = open(NOTES, "insert");
    let src = doc_text(&h);
    let at = src.find("A car").unwrap() + 3;
    let t = mpp_ide::insert::find("text.comment").expect("the Comment item");
    assert!(t.groups.contains(&mpp_ide::insert::catalogue::Group::Text));
    {
        let app = h.state_mut();
        let doc = app.doc_mut().unwrap();
        doc.editor.move_caret(at, false);
        let page = doc.page(1000.0, 18.0, theme::Mode::Light);
        let cx = mpp_ide::insert::BuildCx::default();
        mpp_ide::insert::insert(&mut doc.editor, &page.page, t, t.knobs, &cx);
        doc.touched();
    }
    run(&mut h);
    h.event(Event::Text("first thoughts".into()));
    run(&mut h);
    let text = doc_text(&h);
    assert!(text.contains("\n\n% first thoughts\n"), "{text}");
    // Enter continues the note; Enter on an empty note line ends it.
    h.key_press(Key::Enter);
    run(&mut h);
    h.key_press(Key::Enter);
    run(&mut h);
    let text = doc_text(&h);
    assert!(text.contains("% first thoughts\n\n"), "{text}");
    settle(&mut h);
    assert_eq!(note_blocks(&h), 2);
}

#[test]
fn the_export_dialog_writes_pdf_latex_and_html() {
    let mut h = open(PAPER, "export");
    h.state_mut().export.open = true;
    h.state_mut().export.options.include_comments = true;
    run(&mut h);
    save_png(&mut h, "export-dialog.png");
    let dir = temp_dir("export-out");
    for (format, name) in [
        (mpp_export::Format::Pdf, "paper.pdf"),
        (mpp_export::Format::Tex, "paper.tex"),
        (mpp_export::Format::Html, "paper.html"),
    ] {
        h.state_mut().export.format = format;
        let path = dir.join(name);
        h.state_mut().export_to(&path).expect("export");
        let bytes = std::fs::read(&path).expect("written");
        match format {
            mpp_export::Format::Pdf => assert!(bytes.starts_with(b"%PDF-")),
            mpp_export::Format::Tex => {
                let t = String::from_utf8_lossy(&bytes);
                assert!(
                    t.contains("\\maketitle") && t.contains("\\cite{halliday14}"),
                    "{t}"
                );
            }
            mpp_export::Format::Html => {
                let t = String::from_utf8_lossy(&bytes);
                assert!(t.contains("class=\"note\""), "comments included");
                assert!(t.contains("title-block"));
            }
        }
    }
}
