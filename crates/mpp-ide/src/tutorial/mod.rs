//! The tutorial project: a short set of lessons, shipped inside the program,
//! written to the app-data folder and opened on the first launch and from
//! Help -> Tutorial.

pub mod lessons;

use std::path::{Path, PathBuf};

use crate::app::IdeApp;

/// Where the tutorial project lives: inside the settings folder when there is
/// one, otherwise in the system temp folder.
pub fn tutorial_dir(data: Option<&Path>) -> PathBuf {
    data.map_or_else(
        || std::env::temp_dir().join("mathpp-tutorial"),
        |d| d.join("tutorial"),
    )
}

/// Writes the lessons that are missing (a lesson the user already edited is
/// left alone). Returns the folder.
pub fn install(dir: &Path) -> std::io::Result<PathBuf> {
    std::fs::create_dir_all(dir)?;
    for (name, text) in lessons::FILES {
        let p = dir.join(name);
        if !p.exists() {
            std::fs::write(p, text)?;
        }
    }
    Ok(dir.to_path_buf())
}

impl IdeApp {
    /// Opens the tutorial project and its first lesson.
    pub fn open_tutorial(&mut self, ctx: Option<&egui::Context>) {
        let dir = tutorial_dir(self.ws.settings_dir.as_deref());
        match install(&dir) {
            Ok(dir) => {
                self.open_folder(&dir, ctx);
                if let Some((first, _)) = lessons::FILES.first() {
                    if let Err(m) = self.open_file(&dir.join(first), false) {
                        self.message = Some(m);
                    }
                }
                self.ws.settings.tutorial_seen = true;
            }
            Err(e) => self.message = Some(format!("Could not set up the tutorial: {e}")),
        }
    }

    /// Start-up: restores the last session, offers recovered files, and shows
    /// the tutorial the very first time.
    pub fn start_up(&mut self, data: &Path, ctx: Option<&egui::Context>) {
        let first = self.restore_session(data, ctx);
        self.find_recovery();
        if first {
            self.open_tutorial(ctx);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn install_writes_every_lesson_and_keeps_edits() {
        let dir = std::env::temp_dir().join(format!("mpp-tut-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        install(&dir).expect("install");
        for (name, _) in lessons::FILES {
            assert!(dir.join(name).is_file(), "{name}");
        }
        std::fs::write(dir.join("constants.mpp"), "mine").expect("w");
        install(&dir).expect("again");
        assert_eq!(
            std::fs::read_to_string(dir.join("constants.mpp")).expect("r"),
            "mine"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
