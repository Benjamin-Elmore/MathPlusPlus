//! Script 4: a person going round the side column in one session: every icon,
//! the lit icon, Ctrl+B, F1, and a panel dragged out among the files and back.
//! Pointer events go to the icons' and panels' real positions; frames run only
//! when the app asks for them (`side_column.rs` checks each of these alone).

use std::path::{Path, PathBuf};

use egui::{Event, Key, Modifiers, PointerButton, Pos2, Rect};
use egui_kittest::kittest::{NodeT, Queryable};
use mpp_ide::layout::tiles::{self, Pane};
use mpp_ide::settings::SideView;
use mpp_ide::IdeApp;

use super::runner::{harness, idle, text_of, type_text, H, RENDER, SIZE};

const MAIN: &str = "# Main\n\n$$ x := 5 $$\n\n$$ E := 2x + 5 $$\n";

fn project() -> PathBuf {
    // A fixed name: the Explorer shows it, and the snapshot with it.
    let d = std::env::temp_dir().join("mpp-replay-side");
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).expect("mkdir");
    std::fs::write(d.join("main.mpp"), MAIN).expect("w");
    d
}

fn app_in(d: &Path) -> IdeApp {
    let mut app = IdeApp::empty();
    app.open_folder(d, None);
    app.open_file(&d.join("main.mpp"), false).expect("open");
    app
}

/// The frame an input causes, then the ones the app asks for.
fn frames(h: &mut H) {
    h.step();
    idle(h, std::time::Duration::from_secs(2));
}

fn click_at(h: &mut H, pos: Pos2) {
    h.event(Event::PointerMoved(pos));
    h.step();
    for pressed in [true, false] {
        h.event(Event::PointerButton {
            pos,
            button: PointerButton::Primary,
            pressed,
            modifiers: Modifiers::NONE,
        });
        h.step();
    }
    h.event(Event::PointerMoved(Pos2::new(700.0, 400.0)));
    frames(h);
}

fn chord(h: &mut H, mods: Modifiers, key: Key) {
    h.key_press_modifiers(mods, key);
    frames(h);
}

fn click_icon(h: &mut H, view: SideView) {
    let pos = h.get_by_label(view.label()).rect().center();
    assert!(pos.x < 46.0, "{view:?} is an activity-bar icon at {pos:?}");
    click_at(h, pos);
}

/// What the screen shows: the side panels drawn, and the lit icons.
fn shown(h: &H) -> (Vec<SideView>, Vec<SideView>) {
    let panel = |v: SideView| match v {
        SideView::Help => "Cheat sheet panel".to_string(),
        _ => format!("{} panel", v.label()),
    };
    let panels = SideView::ALL
        .into_iter()
        .filter(|v| h.query_by_label(&panel(*v)).is_some())
        .collect();
    let lit = SideView::ALL
        .into_iter()
        .filter(|v| {
            h.get_by_label(v.label()).accesskit_node().toggled()
                == Some(egui::accesskit::Toggled::True)
        })
        .collect();
    (panels, lit)
}

fn assert_column(h: &H, want: Option<SideView>, step: &str) {
    let w: Vec<SideView> = want.into_iter().collect();
    assert_eq!(shown(h), (w.clone(), w), "after {step}");
    assert_eq!(h.state().ws.side_visible, want.is_some(), "after {step}");
}

fn page_rect(h: &H) -> Rect {
    let st = h.state();
    let t = st.ws.tiles.tree.as_ref().expect("tree");
    let id = tiles::find_by(t, |p| matches!(p, Pane::Equation { .. })).expect("page");
    t.tiles.rect(id).expect("page rect")
}

/// Presses on `from`, moves in small steps and lets go at `to`.
fn drag(h: &mut H, from: Pos2, to: Pos2) {
    h.event(Event::PointerMoved(from));
    h.step();
    h.event(Event::PointerButton {
        pos: from,
        button: PointerButton::Primary,
        pressed: true,
        modifiers: Modifiers::NONE,
    });
    h.step();
    for i in 1..=12 {
        let t = i as f32 / 12.0;
        h.event(Event::PointerMoved(from + (to - from) * t));
        h.step();
    }
    h.event(Event::PointerButton {
        pos: to,
        button: PointerButton::Primary,
        pressed: false,
        modifiers: Modifiers::NONE,
    });
    frames(h);
    frames(h);
}

fn outline_in_column(h: &H) -> bool {
    let st = h.state();
    tiles::in_side_column(st.ws.tiles.tree.as_ref().expect("tree"), SideView::Outline)
}

#[test]
fn going_round_the_side_column_in_one_session() {
    let d = project();
    let mut h = harness(app_in(&d));
    frames(&mut h);
    assert_column(&h, Some(SideView::Explorer), "start");
    // Every icon, in order: its panel shows and its icon is lit.
    for v in SideView::ALL.into_iter().skip(1) {
        click_icon(&mut h, v);
        assert_column(&h, Some(v), &format!("{v:?}"));
    }
    // The lit icon hides the column; Ctrl+B brings it back, then hides it.
    click_icon(&mut h, SideView::Help);
    assert_column(&h, None, "the lit Help icon");
    chord(&mut h, Modifiers::COMMAND, Key::B);
    assert_column(&h, Some(SideView::Help), "Ctrl+B");
    chord(&mut h, Modifiers::COMMAND, Key::B);
    assert_column(&h, None, "Ctrl+B again");
    // F1 opens the cheat sheet from anywhere.
    click_icon(&mut h, SideView::Outline);
    chord(&mut h, Modifiers::NONE, Key::F1);
    assert_column(&h, Some(SideView::Help), "F1");
    // The Outline dragged out among the files: it stays on screen there, the
    // column shows what is left in it, and Ctrl+B hides only the column.
    click_icon(&mut h, SideView::Outline);
    let from = h.get_by_label("Outline panel").rect().center();
    let page = page_rect(&h);
    drag(
        &mut h,
        from,
        Pos2::new(page.right() - 20.0, page.center().y),
    );
    assert!(!outline_in_column(&h), "the Outline left the column");
    assert!(h.query_by_label("Outline panel").is_some());
    chord(&mut h, Modifiers::COMMAND, Key::B);
    assert!(h.query_by_label("Outline panel").is_some(), "still there");
    chord(&mut h, Modifiers::COMMAND, Key::B);
    // And dragged back into the column: its icon shows it there again.
    let from = h.get_by_label("Outline panel").rect().center();
    // Dropped in the middle of the column, it joins the panels there.
    let column = {
        let st = h.state();
        let t = st.ws.tiles.tree.as_ref().expect("tree");
        tiles::side_group(t)
            .and_then(|g| t.tiles.rect(g))
            .expect("column")
    };
    drag(&mut h, from, column.center());
    assert!(outline_in_column(&h), "the Outline is back in the column");
    click_icon(&mut h, SideView::Outline);
    if !h.state().ws.side_visible {
        click_icon(&mut h, SideView::Outline);
    }
    assert_column(&h, Some(SideView::Outline), "the Outline back");
    chord(&mut h, Modifiers::COMMAND, Key::B);
    assert!(
        h.query_by_label("Outline panel").is_none(),
        "Ctrl+B hides it with the column again"
    );
    chord(&mut h, Modifiers::COMMAND, Key::B);
    // Nothing of this touched the file, and typing still goes to the page.
    assert_eq!(text_of(&h), MAIN);
    let at = page_rect(&h).center();
    click_at(&mut h, at);
    type_text(&mut h, "!");
    assert!(text_of(&h).contains('!'), "typing reaches the page");
    {
        let _one = RENDER
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let col = {
            let st = h.state();
            let t = st.ws.tiles.tree.as_ref().expect("tree");
            tiles::side_group(t)
                .and_then(|g| t.tiles.rect(g))
                .expect("the column")
        };
        let ppp = h.ctx.pixels_per_point();
        let img = h.render().expect("render");
        let r = Rect::from_min_max(Pos2::new(0.0, col.top()), col.right_bottom())
            .intersect(Rect::from_min_size(Pos2::ZERO, SIZE));
        let img = image::imageops::crop_imm(
            &img,
            (r.left() * ppp) as u32,
            (r.top() * ppp) as u32,
            (r.width() * ppp) as u32,
            (r.height() * ppp) as u32,
        )
        .to_image();
        let opts = egui_kittest::SnapshotOptions::new()
            .threshold(2.0)
            .max_failed_pixels(120);
        egui_kittest::try_image_snapshot_options(&img, "replay_side_outline", &opts)
            .unwrap_or_else(|e| panic!("{e}"));
    }
    let _ = std::fs::remove_dir_all(&d);
}
