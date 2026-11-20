//! Watches the project folder so the tree follows changes made elsewhere.

use std::path::Path;
use std::sync::mpsc::{channel, Receiver};

use notify::{RecursiveMode, Watcher as _};

/// A live watch on a folder.
pub struct FolderWatch {
    _watcher: notify::RecommendedWatcher,
    rx: Receiver<()>,
}

impl std::fmt::Debug for FolderWatch {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("FolderWatch")
    }
}

impl FolderWatch {
    /// Starts watching `root` (and everything below it). `ctx` is woken when
    /// something changes. `None` when the system refuses.
    pub fn new(root: &Path, ctx: Option<egui::Context>) -> Option<Self> {
        let (tx, rx) = channel();
        let mut watcher = notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
            if res.is_ok() {
                let _ = tx.send(());
                if let Some(c) = &ctx {
                    c.request_repaint();
                }
            }
        })
        .ok()?;
        watcher.watch(root, RecursiveMode::Recursive).ok()?;
        Some(FolderWatch {
            _watcher: watcher,
            rx,
        })
    }

    /// True when something changed since the last call.
    pub fn changed(&self) -> bool {
        let mut any = false;
        while self.rx.try_recv().is_ok() {
            any = true;
        }
        any
    }
}
