//! The side column, VS Code style: the activity bar's icons are the only way to
//! switch side panels. A click shows that panel, a click on the lit icon hides
//! the column, Ctrl+B toggles it. Driven through the real frame loop with
//! pointer events at the icons' real positions.

use std::path::{Path, PathBuf};

use egui::{Event, Key, Modifiers, PointerButton, Pos2, Vec2};
use egui_kittest::kittest::{NodeT, Queryable};
use egui_kittest::Harness;
use mpp_ide::layout::tiles::{self, Pane};
use mpp_ide::settings::SideView;
use mpp_ide::{theme, IdeApp};

const SIZE: Vec2 = Vec2::new(1180.0, 760.0);

fn harness(app: IdeApp) -> Harness<'static, IdeApp> {
    Harness::builder()
        .with_size(SIZE)
        .with_max_steps(40)
        .with_context_setup(theme::install_fonts)
        .build_ui_state(|ui, app: &mut IdeApp| app.ui(ui), app)
}

fn project(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("mpp-side-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).expect("mkdir");
    std::fs::write(
        d.join("main.mpp"),
        "# Main\n\n$$ x := 5 $$\n\n$$ E := 2x + 5 $$\n",
    )
    .expect("w");
    d
}

fn app_in(d: &Path) -> IdeApp {
    let mut app = IdeApp::empty();
    app.open_folder(d, None);
    app.open_file(&d.join("main.mpp"), false).expect("open");
    app
}

/// The frames an input causes: one for the input, then only those the app
/// asks for (as the desktop event loop runs).
fn frames(h: &mut Harness<'static, IdeApp>) {
    h.step();
    idle(h);
}

/// Only the frames the app asks for (no input arrives).
fn idle(h: &mut Harness<'static, IdeApp>) {
    for _ in 0..30 {
        if !h.ctx.has_requested_repaint() {
            break;
        }
        h.step();
    }
}

fn click_at(h: &mut Harness<'static, IdeApp>, pos: Pos2) {
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
    // Move away so no tooltip covers the next check.
    h.event(Event::PointerMoved(Pos2::new(700.0, 400.0)));
    frames(h);
}

fn icon_pos(h: &Harness<'static, IdeApp>, view: SideView) -> Pos2 {
    h.get_by_label(view.label()).rect().center()
}

fn click_icon(h: &mut Harness<'static, IdeApp>, view: SideView) {
    let pos = icon_pos(h, view);
    assert!(pos.x < 46.0, "{view:?} is an activity-bar icon at {pos:?}");
    click_at(h, pos);
}

/// The side panels drawn on screen, by their headers.
fn panels_on_screen(h: &Harness<'static, IdeApp>) -> Vec<SideView> {
    SideView::ALL
        .into_iter()
        .filter(|v| h.query_by_label(&format!("{} panel", v.label())).is_some())
        .collect()
}

fn lit_icons(h: &Harness<'static, IdeApp>) -> Vec<SideView> {
    SideView::ALL
        .into_iter()
        .filter(|v| {
            h.get_by_label(v.label()).accesskit_node().toggled()
                == Some(egui::accesskit::Toggled::True)
        })
        .collect()
}

/// What the screen shows matches the expected side panel (or none).
fn assert_shows(h: &Harness<'static, IdeApp>, want: Option<SideView>, step: &str) {
    let want_list: Vec<SideView> = want.into_iter().collect();
    assert_eq!(
        panels_on_screen(h),
        want_list,
        "panel on screen after {step}"
    );
    assert_eq!(lit_icons(h), want_list, "lit icon after {step}");
    assert_eq!(h.state().ws.side_visible, want.is_some(), "after {step}");
    if let Some(v) = want {
        assert_eq!(h.state().ws.side_view, v, "after {step}");
    }
}

fn permutations(items: &[SideView]) -> Vec<Vec<SideView>> {
    if items.len() <= 1 {
        return vec![items.to_vec()];
    }
    let mut out = vec![];
    for i in 0..items.len() {
        let mut rest = items.to_vec();
        let first = rest.remove(i);
        for mut p in permutations(&rest) {
            p.insert(0, first);
            out.push(p);
        }
    }
    out
}

#[test]
fn every_icon_in_every_order_shows_its_panel_and_the_lit_icon_hides_it() {
    let d = project("orders");
    let mut h = harness(app_in(&d));
    frames(&mut h);
    assert_shows(&h, Some(SideView::Explorer), "start");
    let mut shown = Some(SideView::Explorer);
    let orders = permutations(&SideView::ALL);
    assert_eq!(orders.len(), 24);
    for order in orders {
        for v in order {
            click_icon(&mut h, v);
            // VS Code: clicking the lit icon hides the column; any other shows its panel.
            shown = if shown == Some(v) { None } else { Some(v) };
            assert_shows(&h, shown, &format!("clicking {v:?}"));
        }
    }
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn clicking_the_lit_icon_hides_the_column_and_clicking_again_brings_it_back() {
    let d = project("lit");
    let mut h = harness(app_in(&d));
    frames(&mut h);
    click_icon(&mut h, SideView::Outline);
    assert_shows(&h, Some(SideView::Outline), "Outline");
    let page_before = h.state().ws.tiles.tree.as_ref().and_then(|t| {
        let id = tiles::find_by(t, |p| matches!(p, Pane::Equation { .. }))?;
        t.tiles.rect(id)
    });
    click_icon(&mut h, SideView::Outline);
    assert_shows(&h, None, "the lit Outline icon");
    let page_after = h.state().ws.tiles.tree.as_ref().and_then(|t| {
        let id = tiles::find_by(t, |p| matches!(p, Pane::Equation { .. }))?;
        t.tiles.rect(id)
    });
    let (b, a) = (page_before.expect("rect"), page_after.expect("rect"));
    assert!(
        a.left() < b.left() - 100.0,
        "the page takes the column's room ({b:?} -> {a:?})"
    );
    click_icon(&mut h, SideView::Outline);
    assert_shows(&h, Some(SideView::Outline), "Outline again");
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn ctrl_b_toggles_the_column_and_keeps_its_panel() {
    let d = project("ctrlb");
    let mut h = harness(app_in(&d));
    frames(&mut h);
    click_icon(&mut h, SideView::Search);
    assert_shows(&h, Some(SideView::Search), "Search");
    for round in 0..3 {
        h.key_press_modifiers(Modifiers::COMMAND, Key::B);
        frames(&mut h);
        assert_shows(&h, None, &format!("Ctrl+B (round {round})"));
        h.key_press_modifiers(Modifiers::COMMAND, Key::B);
        frames(&mut h);
        assert_shows(
            &h,
            Some(SideView::Search),
            &format!("Ctrl+B again ({round})"),
        );
    }
    // Hidden by Ctrl+B, then an icon: that icon's panel shows.
    h.key_press_modifiers(Modifiers::COMMAND, Key::B);
    frames(&mut h);
    click_icon(&mut h, SideView::Variables);
    assert_shows(&h, Some(SideView::Variables), "Variables after Ctrl+B");
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn the_side_column_has_no_tab_strip_or_close_buttons() {
    let d = project("strip");
    let mut h = harness(app_in(&d));
    frames(&mut h);
    let icon_top = h.get_by_label("Explorer").rect().top();
    let header = h.get_by_label("Explorer panel").rect();
    assert!(
        header.top() - icon_top < 12.0,
        "the panel header sits at the top, no tab strip above it ({icon_top} vs {})",
        header.top()
    );
    // The other side panels are not offered as tabs in the column.
    for v in [SideView::Search, SideView::Outline, SideView::Variables] {
        let n = h.query_all_by_label(v.label()).count();
        assert_eq!(n, 1, "only the {v:?} icon carries its name");
    }
    // Side panels never get a × (even dragged in among the files).
    let st = h.state();
    let tree = st.ws.tiles.tree.as_ref().expect("tree");
    let col = tiles::side_group(tree).expect("side column");
    assert!(tiles::is_side_only(&tree.tiles, col));
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn closing_a_side_panel_from_the_view_menu_and_reopening_it_from_its_icon() {
    let d = project("close");
    let mut h = harness(app_in(&d));
    frames(&mut h);
    click_icon(&mut h, SideView::Outline);
    assert_shows(&h, Some(SideView::Outline), "Outline");
    h.state_mut().toggle_pane(Pane::Outline);
    frames(&mut h);
    assert!(!h.state_mut().has_pane(&Pane::Outline), "closed");
    assert_shows(&h, None, "closing the shown panel");
    click_icon(&mut h, SideView::Outline);
    assert_shows(&h, Some(SideView::Outline), "reopening from its icon");
    // Closing a panel that is not showing leaves the column alone.
    h.state_mut().toggle_pane(Pane::Search);
    frames(&mut h);
    assert_shows(&h, Some(SideView::Outline), "closing Search");
    click_icon(&mut h, SideView::Search);
    assert_shows(&h, Some(SideView::Search), "Search comes back");
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn a_side_panel_dragged_into_the_main_area_stays_and_ctrl_b_hides_only_the_column() {
    let d = project("drag");
    let mut h = harness(app_in(&d));
    frames(&mut h);
    click_icon(&mut h, SideView::Outline);
    let from = h.get_by_label("Outline panel").rect().center();
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
        .expect("page");
    // Drop near the right edge of the page: a new tile beside it.
    let to = Pos2::new(page.right() - 20.0, page.center().y);
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
    frames(&mut h);
    frames(&mut h);
    let (outside, column_view) = {
        let st = h.state();
        let t = st.ws.tiles.tree.as_ref().expect("tree");
        (
            !tiles::in_side_column(t, SideView::Outline),
            tiles::side_column_view(t),
        )
    };
    assert!(outside, "the Outline left the side column");
    assert!(
        h.query_by_label("Outline panel").is_some(),
        "and is still on screen"
    );
    // The lit icon follows what the column shows now.
    assert_eq!(Some(h.state().ws.side_view), column_view);
    h.key_press_modifiers(Modifiers::COMMAND, Key::B);
    frames(&mut h);
    assert!(!h.state().ws.side_visible);
    assert!(
        h.query_by_label("Outline panel").is_some(),
        "Ctrl+B hides the column, not the tile dragged among the files"
    );
    assert!(lit_icons(&h).is_empty());
    let _ = std::fs::remove_dir_all(&d);
}
