//! Settings and session, kept as one small JSON file in the app-data folder
//! (never inside a project): the last folder and tabs, zoom, autosave, the
//! theme, the Insert menu's settings, and whether the tutorial has been shown.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// When files save by themselves.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum AutosaveMode {
    Off,
    /// A second after typing stops.
    #[default]
    Delay,
    /// When the window loses focus.
    Focus,
}

/// Which side-bar view is showing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum SideView {
    #[default]
    Explorer,
    Search,
    Outline,
}

/// Everything remembered between runs.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub last_folder: Option<PathBuf>,
    pub tabs: Vec<PathBuf>,
    pub active: usize,
    pub side_visible: bool,
    pub side_view: SideView,
    pub side_width: f32,
    /// Page text size in pixels (the zoom).
    pub size: f32,
    pub autosave: AutosaveMode,
    pub tutorial_seen: bool,
    /// Light, dark or follow Windows (`ThemeChoice::to_stored`).
    pub theme: String,
    /// The Insert menu's settings and recently used items (`InsertSettings::to_stored`).
    pub insert: String,
    /// Copy maths as LaTeX instead of Unicode.
    pub copy_latex: bool,
    /// Grey automatic answers (View → Auto-evaluate).
    pub auto_eval: bool,
    /// The working of every query (View → Show steps).
    pub show_steps: bool,
    /// Results as decimals by default (View → Numbers).
    pub decimals: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            last_folder: None,
            tabs: vec![],
            active: 0,
            side_visible: true,
            side_view: SideView::Explorer,
            side_width: 250.0,
            size: crate::layout::BODY_SIZE,
            autosave: AutosaveMode::Delay,
            tutorial_seen: false,
            theme: crate::theme::ThemeChoice::default().to_stored().into(),
            insert: String::new(),
            copy_latex: false,
            auto_eval: true,
            show_steps: false,
            decimals: false,
        }
    }
}

impl Settings {
    /// Reads settings from JSON; anything unreadable gives the defaults.
    pub fn from_json(s: &str) -> Settings {
        let mut v: Settings = serde_json::from_str(s).unwrap_or_default();
        v.size = v
            .size
            .clamp(crate::layout::MIN_SIZE, crate::layout::MAX_SIZE);
        if !v.side_width.is_finite() {
            v.side_width = 250.0;
        }
        v.side_width = v.side_width.clamp(140.0, 700.0);
        v
    }

    /// The JSON form.
    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).unwrap_or_default()
    }

    /// Reads `settings.json` from a folder (defaults when missing).
    pub fn load(dir: &Path) -> Settings {
        std::fs::read_to_string(dir.join("settings.json"))
            .map(|s| Settings::from_json(&s))
            .unwrap_or_default()
    }

    /// Writes `settings.json` into a folder.
    pub fn save(&self, dir: &Path) -> std::io::Result<()> {
        std::fs::create_dir_all(dir)?;
        std::fs::write(dir.join("settings.json"), self.to_json())
    }

    /// True when no settings file exists yet in `dir` (the first launch).
    pub fn is_first_launch(dir: &Path) -> bool {
        !dir.join("settings.json").exists()
    }
}

/// The app-data folder (`MPP_IDE_DATA` overrides it, which tests rely on).
pub fn app_data_dir() -> Option<PathBuf> {
    if let Some(p) = std::env::var_os("MPP_IDE_DATA") {
        return Some(PathBuf::from(p));
    }
    #[cfg(target_os = "windows")]
    let base = std::env::var_os("APPDATA").map(PathBuf::from);
    #[cfg(target_os = "macos")]
    let base =
        std::env::var_os("HOME").map(|h| PathBuf::from(h).join("Library/Application Support"));
    #[cfg(all(not(target_os = "windows"), not(target_os = "macos")))]
    let base = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")));
    base.map(|b| b.join("MathPP"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_and_survives_garbage() {
        let s = Settings {
            size: 24.0,
            last_folder: Some(PathBuf::from("/p")),
            autosave: AutosaveMode::Focus,
            ..Settings::default()
        };
        assert_eq!(Settings::from_json(&s.to_json()), s);
        assert_eq!(Settings::from_json("not json"), Settings::default());
        let wild = Settings::from_json("{\"size\": 9000, \"side_width\": -4}");
        assert_eq!(wild.size, crate::layout::MAX_SIZE);
        assert_eq!(wild.side_width, 140.0);
    }
}
