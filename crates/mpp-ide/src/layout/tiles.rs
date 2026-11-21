//! The tile layout: which panels exist and how they are arranged. These are
//! plain functions over an `egui_tiles` tree, so they are tested without a
//! window.

use egui_tiles::{Container, Linear, LinearDir, Tile, TileId, Tiles, Tree};
use serde::{Deserialize, Serialize};

use super::docref;
use crate::settings::SideView;

/// What a tile shows.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Pane {
    /// A file's equation view. `view` tells two views of one file apart.
    Equation {
        #[serde(with = "docref")]
        doc: u64,
        view: u32,
    },
    /// One figure of a file, pinned in its own tile.
    Graph {
        #[serde(with = "docref")]
        doc: u64,
        figure: usize,
    },
    Explorer,
    Outline,
    Search,
    Problems,
    Variables,
    Steps,
    Library,
}

impl Pane {
    /// The document this pane shows, if any.
    pub fn doc(&self) -> Option<u64> {
        match self {
            Pane::Equation { doc, .. } | Pane::Graph { doc, .. } => Some(*doc),
            _ => None,
        }
    }

    /// True for the panels that live in the side bar.
    pub fn is_side(&self) -> bool {
        matches!(
            self,
            Pane::Explorer | Pane::Search | Pane::Outline | Pane::Variables
        )
    }

    /// The activity-bar view this panel belongs to, if any.
    pub fn side_view(&self) -> Option<SideView> {
        match self {
            Pane::Explorer => Some(SideView::Explorer),
            Pane::Search => Some(SideView::Search),
            Pane::Outline => Some(SideView::Outline),
            Pane::Variables => Some(SideView::Variables),
            _ => None,
        }
    }

    /// The panel of an activity-bar view.
    pub fn of_side(view: SideView) -> Pane {
        match view {
            SideView::Explorer => Pane::Explorer,
            SideView::Search => Pane::Search,
            SideView::Outline => Pane::Outline,
            SideView::Variables => Pane::Variables,
        }
    }

    /// The name shown in the View menu and on a tab.
    pub fn name(&self) -> &'static str {
        match self {
            Pane::Equation { .. } => "Equation view",
            Pane::Graph { .. } => "Graph",
            Pane::Explorer => "Explorer",
            Pane::Outline => "Outline",
            Pane::Search => "Search",
            Pane::Problems => "Problems",
            Pane::Variables => "Definitions",
            Pane::Steps => "Steps",
            Pane::Library => "Standard library",
        }
    }
}

/// The panels that can be opened and closed from the View menu.
pub const MENU_PANES: [Pane; 7] = [
    Pane::Explorer,
    Pane::Search,
    Pane::Outline,
    Pane::Variables,
    Pane::Problems,
    Pane::Steps,
    Pane::Library,
];

/// A ready-made arrangement.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Preset {
    Writing,
    Graphing,
    Review,
}

impl Preset {
    pub const ALL: [Preset; 3] = [Preset::Writing, Preset::Graphing, Preset::Review];

    pub fn name(self) -> &'static str {
        match self {
            Preset::Writing => "Writing",
            Preset::Graphing => "Graphing",
            Preset::Review => "Review",
        }
    }
}

/// The direction of a split or a focus move.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dir {
    Left,
    Right,
    Up,
    Down,
}

fn new_tree(root: TileId, tiles: Tiles<Pane>) -> Tree<Pane> {
    Tree::new(egui::Id::unique("mpp-tiles"), root, tiles)
}

fn group(tiles: &mut Tiles<Pane>, panes: Vec<Pane>) -> TileId {
    let ids: Vec<TileId> = panes.into_iter().map(|p| tiles.insert_pane(p)).collect();
    tiles.insert_tab_tile(ids)
}

fn binary(tiles: &mut Tiles<Pane>, dir: LinearDir, a: TileId, b: TileId, frac: f32) -> TileId {
    tiles.insert_container(Linear::new_binary(dir, [a, b], frac))
}

/// Builds a preset for the open documents (`active` is shown first).
pub fn build(preset: Preset, docs: &[u64], active: Option<u64>) -> Tree<Pane> {
    let mut tiles = Tiles::default();
    let eq: Vec<Pane> = docs
        .iter()
        .map(|d| Pane::Equation { doc: *d, view: 0 })
        .collect();
    let editor = group(&mut tiles, eq);
    if let Some(a) = active {
        set_active_doc(&mut tiles, editor, a);
    }
    let graph_doc = active.or_else(|| docs.first().copied());
    let root = match preset {
        Preset::Writing => {
            let side = group(
                &mut tiles,
                vec![Pane::Explorer, Pane::Search, Pane::Outline, Pane::Variables],
            );
            binary(&mut tiles, LinearDir::Horizontal, side, editor, 0.24)
        }
        Preset::Graphing => {
            let side = group(&mut tiles, vec![Pane::Explorer, Pane::Variables]);
            let right = match graph_doc {
                Some(doc) => group(&mut tiles, vec![Pane::Graph { doc, figure: 0 }]),
                None => group(&mut tiles, vec![Pane::Steps]),
            };
            let mid = binary(&mut tiles, LinearDir::Horizontal, editor, right, 0.6);
            binary(&mut tiles, LinearDir::Horizontal, side, mid, 0.24)
        }
        Preset::Review => {
            let side = group(&mut tiles, vec![Pane::Explorer, Pane::Search]);
            let bottom = group(&mut tiles, vec![Pane::Problems, Pane::Outline]);
            let mid = binary(&mut tiles, LinearDir::Vertical, editor, bottom, 0.72);
            binary(&mut tiles, LinearDir::Horizontal, side, mid, 0.24)
        }
    };
    let mut tree = new_tree(root, tiles);
    // Shown tab groups start on their first panel.
    tree.simplify(&super::simplification());
    tree
}

fn set_active_doc(tiles: &mut Tiles<Pane>, group_id: TileId, doc: u64) {
    let target = tiles.iter().find_map(|(id, t)| match t {
        Tile::Pane(Pane::Equation { doc: d, .. }) if *d == doc => Some(*id),
        _ => None,
    });
    if let (Some(t), Some(Tile::Container(Container::Tabs(tabs)))) =
        (target, tiles.get_mut(group_id))
    {
        tabs.set_active(t);
    }
}

/// The tile holding a pane, if it is in the tree.
pub fn find(tree: &Tree<Pane>, pane: &Pane) -> Option<TileId> {
    tree.tiles.find_pane(pane)
}

/// The first tile whose pane satisfies `pred`.
pub fn find_by(tree: &Tree<Pane>, pred: impl Fn(&Pane) -> bool) -> Option<TileId> {
    tree.tiles.iter().find_map(|(id, t)| match t {
        Tile::Pane(p) if pred(p) => Some(*id),
        _ => None,
    })
}

/// The pane of a tile.
pub fn pane_of(tree: &Tree<Pane>, id: TileId) -> Option<&Pane> {
    tree.tiles.get_pane(&id)
}

/// The tab group that new files open into: the group of `prefer`, of any
/// equation view, or an empty group.
pub fn editor_group(tree: &Tree<Pane>, prefer: Option<TileId>) -> Option<TileId> {
    let of = |id: TileId| tree.tiles.parent_of(id);
    prefer
        .filter(|p| matches!(pane_of(tree, *p), Some(Pane::Equation { .. })))
        .and_then(of)
        .or_else(|| find_by(tree, |p| matches!(p, Pane::Equation { .. })).and_then(of))
        .or_else(|| {
            tree.tiles.iter().find_map(|(id, t)| match t {
                Tile::Container(Container::Tabs(tabs)) if tabs.children.is_empty() => Some(*id),
                _ => None,
            })
        })
}

/// True for a tab group that holds only side panels (the side column, or a
/// side panel dragged out on its own). Such a group has no tab strip: the
/// activity bar picks its panel.
pub fn is_side_only(tiles: &Tiles<Pane>, group: TileId) -> bool {
    match tiles.get(group) {
        Some(Tile::Container(Container::Tabs(t))) => {
            !t.children.is_empty()
                && t.children
                    .iter()
                    .all(|c| tiles.get_pane(c).is_some_and(Pane::is_side))
        }
        _ => false,
    }
}

/// The side-only groups of a tree.
pub fn side_only_groups(tree: &Tree<Pane>) -> Vec<TileId> {
    tree.tiles
        .tile_ids()
        .filter(|id| is_side_only(&tree.tiles, *id))
        .collect()
}

/// The side-only group holding `pane`, if the pane is in one.
pub fn side_group_of(tree: &Tree<Pane>, pane: &Pane) -> Option<TileId> {
    let g = tree.tiles.parent_of(find(tree, pane)?)?;
    is_side_only(&tree.tiles, g).then_some(g)
}

/// The side column: the leftmost group holding only side panels (as last
/// laid out), else the explorer's side-only group, else any side-only group.
/// A side panel dragged out among the files makes a group of its own, which
/// is never the side column unless it is further left.
pub fn side_group(tree: &Tree<Pane>) -> Option<TileId> {
    let groups = side_only_groups(tree);
    // A hidden side-only group is the column Ctrl+B hid (it has no rect).
    if let Some(g) = groups.iter().find(|g| !tree.tiles.is_visible(**g)) {
        return Some(*g);
    }
    let leftmost = groups
        .iter()
        .filter_map(|g| tree.tiles.rect(*g).map(|r| (r.left(), *g)))
        .min_by(|a, b| a.0.total_cmp(&b.0))
        .map(|(_, g)| g);
    leftmost
        .or_else(|| side_group_of(tree, &Pane::Explorer))
        .or_else(|| groups.into_iter().next())
}

/// The activity-bar view shown in the side column.
pub fn side_column_view(tree: &Tree<Pane>) -> Option<SideView> {
    let g = side_group(tree)?;
    match tree.tiles.get(g) {
        Some(Tile::Container(Container::Tabs(t))) => t
            .active
            .filter(|a| t.children.contains(a))
            .or_else(|| t.children.first().copied())
            .and_then(|a| tree.tiles.get_pane(&a))
            .and_then(Pane::side_view),
        _ => None,
    }
}

/// True when `view`'s panel is in the side column.
pub fn in_side_column(tree: &Tree<Pane>, view: SideView) -> bool {
    let col = side_group(tree);
    col.is_some() && find(tree, &Pane::of_side(view)).and_then(|id| tree.tiles.parent_of(id)) == col
}

/// Puts the tree in step with the activity bar: the side column is shown or
/// hidden, and when `view`'s panel is in it, that panel is the one shown. A
/// panel that was dragged out among the files is left where it is; only a
/// missing panel is added back, and only when it should be shown.
pub fn sync_side(tree: &mut Tree<Pane>, view: SideView, visible: bool) {
    let pane = Pane::of_side(view);
    let Some(id) = find(tree, &pane) else {
        if visible {
            open_pane(tree, pane);
        } else if let Some(col) = side_group(tree) {
            tree.tiles.set_visible(col, false);
        }
        return;
    };
    let Some(col) = side_group(tree) else {
        return;
    };
    if !visible {
        if tree.tiles.is_visible(col) {
            tree.tiles.set_visible(col, false);
        }
        return;
    }
    if tree.tiles.parent_of(id) == Some(col) {
        let shown = matches!(
            tree.tiles.get(col),
            Some(Tile::Container(Container::Tabs(t))) if t.active == Some(id)
        ) && tree.tiles.is_visible(col);
        if !shown {
            activate(tree, id);
        }
    } else if !tree.tiles.is_visible(col) {
        show_tile(tree, col);
    }
}

/// Makes a tile, and every group above it, visible.
pub fn show_tile(tree: &mut Tree<Pane>, id: TileId) {
    let mut cur = Some(id);
    while let Some(c) = cur {
        tree.tiles.set_visible(c, true);
        cur = tree.tiles.parent_of(c);
    }
}

/// Selects the tab of a pane in its group (and the groups above it).
pub fn activate(tree: &mut Tree<Pane>, id: TileId) {
    tree.make_active(|t, _| t == id);
    show_tile(tree, id);
}

/// Adds a tile into a group as its last tab and selects it.
fn add_to_group(tree: &mut Tree<Pane>, group_id: TileId, tile: TileId) {
    tree.move_tile_to_container(tile, group_id, usize::MAX, false);
    if let Some(Tile::Container(Container::Tabs(t))) = tree.tiles.get_mut(group_id) {
        t.set_active(tile);
    }
}

/// Puts `new_group` beside `group_id`, splitting in `dir`.
pub fn split_beside(tree: &mut Tree<Pane>, group_id: TileId, new_group: TileId, dir: Dir) {
    let (linear_dir, after) = match dir {
        Dir::Left => (LinearDir::Horizontal, false),
        Dir::Right => (LinearDir::Horizontal, true),
        Dir::Up => (LinearDir::Vertical, false),
        Dir::Down => (LinearDir::Vertical, true),
    };
    let parent = tree.tiles.parent_of(group_id);
    if let Some(p) = parent {
        if let Some(Tile::Container(Container::Linear(l))) = tree.tiles.get(p) {
            if l.dir == linear_dir {
                let at = l.children.iter().position(|c| *c == group_id).unwrap_or(0);
                tree.move_tile_to_container(new_group, p, at + usize::from(after), false);
                return;
            }
        }
    }
    let children = if after {
        vec![group_id, new_group]
    } else {
        vec![new_group, group_id]
    };
    let lin = tree
        .tiles
        .insert_container(Linear::new(linear_dir, children));
    match parent {
        Some(p) => {
            if let Some(Tile::Container(c)) = tree.tiles.get_mut(p) {
                let _ = c.replace_child(group_id, lin);
            }
        }
        None => tree.root = Some(lin),
    }
}

/// Opens a second view of the focused equation view (or a copy of a graph)
/// beside it. Returns the new tile.
pub fn split_pane(tree: &mut Tree<Pane>, focused: TileId, dir: Dir, view: u32) -> Option<TileId> {
    let pane = match pane_of(tree, focused)? {
        Pane::Equation { doc, .. } => Pane::Equation { doc: *doc, view },
        Pane::Graph { doc, figure } => Pane::Graph {
            doc: *doc,
            figure: *figure,
        },
        _ => return None,
    };
    let group_id = tree.tiles.parent_of(focused)?;
    let new_pane = tree.tiles.insert_pane(pane);
    let new_group = tree.tiles.insert_tab_tile(vec![new_pane]);
    split_beside(tree, group_id, new_group, dir);
    Some(new_pane)
}

/// Opens a panel (or brings it to the front). Returns its tile.
pub fn open_pane(tree: &mut Tree<Pane>, pane: Pane) -> TileId {
    if let Some(id) = find(tree, &pane) {
        activate(tree, id);
        return id;
    }
    let id = tree.tiles.insert_pane(pane.clone());
    let target = if pane.is_side() {
        side_group(tree)
    } else {
        find_by(tree, |p| {
            matches!(p, Pane::Problems | Pane::Steps | Pane::Library)
        })
        .and_then(|t| tree.tiles.parent_of(t))
    };
    match target {
        Some(g) => add_to_group(tree, g, id),
        None => {
            let new_group = tree.tiles.insert_tab_tile(vec![id]);
            let anchor = editor_group(tree, None).or(tree.root);
            match anchor {
                Some(a) => {
                    let dir = if pane.is_side() { Dir::Left } else { Dir::Down };
                    split_beside(tree, a, new_group, dir);
                }
                None => tree.root = Some(new_group),
            }
        }
    }
    activate(tree, id);
    id
}

/// Closes a panel by removing its tile.
pub fn close_pane(tree: &mut Tree<Pane>, id: TileId) {
    let _ = tree.remove_recursively(id);
}

/// Brings the tree in line with the open documents: panes for closed files go,
/// and every file gets an equation view. Returns true when something changed.
pub fn sync_docs(tree: &mut Tree<Pane>, docs: &[u64], focused: Option<TileId>) -> bool {
    let mut changed = false;
    let stale: Vec<TileId> = tree
        .tiles
        .iter()
        .filter_map(|(id, t)| match t {
            Tile::Pane(p) => p.doc().filter(|d| !docs.contains(d)).map(|_| *id),
            _ => None,
        })
        .collect();
    for id in stale {
        let _ = tree.remove_recursively(id);
        changed = true;
    }
    for d in docs {
        let has = find_by(
            tree,
            |p| matches!(p, Pane::Equation { doc, .. } if doc == d),
        )
        .is_some();
        if has {
            continue;
        }
        // Find the group first: the new pane is not in any group yet, and
        // looking for "an equation view" must not find it.
        let group = editor_group(tree, focused);
        let id = tree.tiles.insert_pane(Pane::Equation { doc: *d, view: 0 });
        match group {
            Some(g) => add_to_group(tree, g, id),
            None => {
                let new_group = tree.tiles.insert_tab_tile(vec![id]);
                match tree.root {
                    Some(r) => split_beside(tree, r, new_group, Dir::Right),
                    None => tree.root = Some(new_group),
                }
            }
        }
        changed = true;
    }
    changed
}

/// Makes the equation view of `doc` the shown tab. Returns false when it has none.
pub fn show_doc(tree: &mut Tree<Pane>, doc: u64, prefer: Option<TileId>) -> bool {
    // Prefer the view that is already focused.
    let id = prefer
        .filter(|p| matches!(pane_of(tree, *p), Some(Pane::Equation { doc: d, .. }) if *d == doc))
        .or_else(|| {
            find_by(
                tree,
                |p| matches!(p, Pane::Equation { doc: d, .. } if *d == doc),
            )
        });
    match id {
        Some(id) => {
            activate(tree, id);
            true
        }
        None => false,
    }
}

/// The documents of the equation views in the tab group of `tile`, in tab order.
pub fn group_docs(tree: &Tree<Pane>, tile: TileId) -> Vec<u64> {
    let Some(g) = tree.tiles.parent_of(tile) else {
        return vec![];
    };
    match tree.tiles.get(g) {
        Some(Tile::Container(Container::Tabs(t))) => t
            .children
            .iter()
            .filter_map(|c| match tree.tiles.get_pane(c) {
                Some(Pane::Equation { doc, .. }) => Some(*doc),
                _ => None,
            })
            .collect(),
        _ => vec![],
    }
}

/// The tile one step from `from` in `dir`, among the panels now on screen.
pub fn neighbour(tree: &Tree<Pane>, from: TileId, dir: Dir) -> Option<TileId> {
    let r = tree.tiles.rect(from)?;
    let mut best: Option<(f32, TileId)> = None;
    for id in tree.active_tiles() {
        if id == from || !matches!(tree.tiles.get(id), Some(Tile::Pane(_))) {
            continue;
        }
        let Some(c) = tree.tiles.rect(id) else {
            continue;
        };
        let (gap, overlap) = match dir {
            Dir::Right => (c.left() - r.right(), overlap(r.y_range(), c.y_range())),
            Dir::Left => (r.left() - c.right(), overlap(r.y_range(), c.y_range())),
            Dir::Down => (c.top() - r.bottom(), overlap(r.x_range(), c.x_range())),
            Dir::Up => (r.top() - c.bottom(), overlap(r.x_range(), c.x_range())),
        };
        if gap < -2.0 || overlap <= 0.0 {
            continue;
        }
        let score = gap.max(0.0) - overlap * 0.01;
        if best.is_none_or(|(s, _)| score < s) {
            best = Some((score, id));
        }
    }
    best.map(|(_, id)| id)
}

fn overlap(a: egui::Rangef, b: egui::Rangef) -> f32 {
    (a.max.min(b.max) - a.min.max(b.min)).max(0.0)
}

/// Writes a tree as JSON, turning document numbers into keys (`docs` pairs
/// each open document's number with its key).
pub fn to_json(tree: &Tree<Pane>, docs: Vec<(u64, String)>) -> String {
    docref::with_table(docs, || serde_json::to_string(tree).unwrap_or_default())
}

/// Reads a tree back; panes whose file is not open come back as number 0 and
/// are dropped by [`sync_docs`].
pub fn from_json(json: &str, docs: Vec<(u64, String)>) -> Option<Tree<Pane>> {
    docref::with_table(docs, || serde_json::from_str::<Tree<Pane>>(json).ok())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn count(tree: &Tree<Pane>, pred: impl Fn(&Pane) -> bool) -> usize {
        tree.tiles
            .iter()
            .filter(|(_, t)| matches!(t, Tile::Pane(p) if pred(p)))
            .count()
    }

    #[test]
    fn presets_hold_the_expected_panels() {
        let w = build(Preset::Writing, &[1, 2], Some(2));
        assert_eq!(count(&w, |p| matches!(p, Pane::Equation { .. })), 2);
        assert!(find(&w, &Pane::Explorer).is_some() && find(&w, &Pane::Outline).is_some());
        let g = build(Preset::Graphing, &[1], Some(1));
        assert!(find(&g, &Pane::Graph { doc: 1, figure: 0 }).is_some());
        assert!(find(&g, &Pane::Variables).is_some());
        let r = build(Preset::Review, &[1], None);
        assert!(find(&r, &Pane::Problems).is_some());
    }

    #[test]
    fn splitting_gives_a_second_view_of_the_same_file() {
        let mut t = build(Preset::Writing, &[7], Some(7));
        let eq = find_by(&t, |p| matches!(p, Pane::Equation { .. })).expect("eq");
        let new = split_pane(&mut t, eq, Dir::Right, 1).expect("split");
        assert_eq!(count(&t, |p| matches!(p, Pane::Equation { doc: 7, .. })), 2);
        assert_ne!(
            t.tiles.parent_of(new),
            t.tiles.parent_of(eq),
            "its own group"
        );
        let new2 = split_pane(&mut t, eq, Dir::Down, 2).expect("split down");
        assert_eq!(count(&t, |p| matches!(p, Pane::Equation { doc: 7, .. })), 3);
        assert!(t.tiles.get(new2).is_some());
    }

    #[test]
    fn closed_files_lose_their_views_and_new_files_get_one() {
        let mut t = build(Preset::Writing, &[1, 2], Some(1));
        assert!(sync_docs(&mut t, &[2, 3], None));
        assert!(find_by(&t, |p| matches!(p, Pane::Equation { doc: 1, .. })).is_none());
        assert!(find_by(&t, |p| matches!(p, Pane::Equation { doc: 3, .. })).is_some());
        assert!(!sync_docs(&mut t, &[2, 3], None), "nothing more to do");
    }

    #[test]
    fn panels_close_and_reopen() {
        let mut t = build(Preset::Writing, &[1], Some(1));
        let id = find(&t, &Pane::Outline).expect("outline");
        close_pane(&mut t, id);
        assert!(find(&t, &Pane::Outline).is_none());
        open_pane(&mut t, Pane::Outline);
        assert!(find(&t, &Pane::Outline).is_some());
        open_pane(&mut t, Pane::Problems);
        assert!(
            find(&t, &Pane::Problems).is_some(),
            "a bottom group appears"
        );
    }

    #[test]
    fn a_saved_layout_comes_back_with_the_same_files() {
        let t = build(Preset::Graphing, &[5, 6], Some(6));
        let key = |d: u64| format!("file{d}.mpp");
        let json = to_json(&t, vec![(5, key(5)), (6, key(6))]);
        assert!(json.contains("file5.mpp"));
        // Next run the same files have new numbers.
        let back = from_json(&json, vec![(50, key(5)), (60, key(6))]).expect("loads");
        assert!(find_by(&back, |p| matches!(p, Pane::Equation { doc: 50, .. })).is_some());
        assert!(find_by(&back, |p| matches!(p, Pane::Graph { doc: 60, .. })).is_some());
        // A file that is gone comes back as 0 and is dropped.
        let mut gone = from_json(&json, vec![(60, key(6))]).expect("loads");
        sync_docs(&mut gone, &[60], None);
        assert!(find_by(&gone, |p| matches!(p, Pane::Equation { doc: 0, .. })).is_none());
    }
}
