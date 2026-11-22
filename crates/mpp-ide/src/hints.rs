//! One-time hints. Each new behaviour of the editor (a line turned into maths,
//! a conversion, a heading from `#`, a number kept as a number) explains itself
//! the first time it fires and says how to undo it. Which hints were shown is
//! kept with the settings, so it survives restarts; Help -> Hints lists them
//! and resets them.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use crate::app::IdeApp;

/// Every hint the editor can show.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Hint {
    /// `#` and a letter at the start of a line made a heading.
    Heading,
    /// `$`, `=` or `:=` on an empty line started a maths line.
    StartMaths,
    /// A finished prose line was turned into maths by itself.
    AutoMaths,
    /// What was typed in maths (`x**2`, `a/b`, `alpha`) was converted.
    Conversion,
    /// `1.3` stayed a number instead of becoming a list.
    NumberNotList,
    /// A sentence ends with maths and `=`: Ctrl+M makes just that maths.
    InlineMaths,
    /// `:=` was typed: what it means.
    Defines,
}

/// What the `:=` hint says (T15).
pub const DEFINES: &str = "≔ means \"is defined as\": x := 5 gives x the value 5.";

impl Hint {
    pub const ALL: [Hint; 7] = [
        Hint::Heading,
        Hint::StartMaths,
        Hint::AutoMaths,
        Hint::Conversion,
        Hint::NumberNotList,
        Hint::InlineMaths,
        Hint::Defines,
    ];

    /// The name stored in the settings.
    pub fn key(self) -> &'static str {
        match self {
            Hint::Heading => "heading",
            Hint::StartMaths => "start-maths",
            Hint::AutoMaths => "auto-maths",
            Hint::Conversion => "conversion",
            Hint::NumberNotList => "number-not-list",
            Hint::InlineMaths => "inline-maths",
            Hint::Defines => "defines",
        }
    }

    /// The name shown in Help -> Hints.
    pub fn title(self) -> &'static str {
        match self {
            Hint::Heading => "Heading from #",
            Hint::StartMaths => "Maths line from $ or =",
            Hint::AutoMaths => "Line turned into maths",
            Hint::Conversion => "Typing converted",
            Hint::NumberNotList => "Number kept as a number",
            Hint::InlineMaths => "Maths in a sentence",
            Hint::Defines => "What := means",
        }
    }

    /// What the hint says, and how to undo what it describes.
    pub fn about(self) -> &'static str {
        match self {
            Hint::Heading => "Typing # and a letter makes a heading. Ctrl+Z keeps the # as text.",
            Hint::StartMaths => {
                "Typing $, = or := on an empty line starts a maths line. Ctrl+Z gives back what you typed."
            }
            Hint::AutoMaths => {
                "A finished line that reads as maths becomes a maths line. Ctrl+Z keeps it as text."
            }
            Hint::Conversion => {
                "Shorthand such as x**2, a/b or alpha converts as you type. Ctrl+Z gives back exactly what you typed."
            }
            Hint::NumberNotList => {
                "A number like 1.3 is never a list. For a list, type 1. and then a space."
            }
            Hint::Defines => DEFINES,
            Hint::InlineMaths => {
                "A sentence ending in maths and = (half of v is v/2 =) can get an answer: Ctrl+M makes just that part maths. Select words and press Ctrl+M or Alt+= to make them maths."
            }
        }
    }
}

/// Which hints were shown already.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct HintBook {
    seen: BTreeSet<String>,
}

impl HintBook {
    pub fn is_seen(&self, h: Hint) -> bool {
        self.seen.contains(h.key())
    }

    pub fn mark(&mut self, h: Hint) {
        self.seen.insert(h.key().to_string());
    }

    /// Makes every hint show again.
    pub fn reset(&mut self) {
        self.seen.clear();
    }

    pub fn seen_count(&self) -> usize {
        Hint::ALL.iter().filter(|h| self.is_seen(**h)).count()
    }

    fn absorb(&mut self, other: &HintBook) {
        self.seen.extend(other.seen.iter().cloned());
    }
}

impl IdeApp {
    /// Start of a frame: every open file's editor gets the settings' hint book.
    pub(crate) fn hints_in(&mut self) {
        let book = &self.ws.settings.hints;
        for d in &mut self.docs {
            if d.editor.hints != *book {
                d.editor.hints = book.clone();
            }
        }
    }

    /// End of a frame: hints the editors showed are kept in the settings.
    pub(crate) fn hints_out(&mut self) {
        let mut book = self.ws.settings.hints.clone();
        for d in &self.docs {
            book.absorb(&d.editor.hints);
        }
        if book != self.ws.settings.hints {
            self.ws.settings.hints = book;
        }
    }

    /// Help -> Hints: every hint with whether it was shown, and a reset.
    pub(crate) fn hints_menu(&mut self, ui: &mut egui::Ui) {
        ui.menu_button("Hints", |ui| {
            for h in Hint::ALL {
                let mark = if self.ws.settings.hints.is_seen(h) {
                    "\u{2713} shown"
                } else {
                    "not shown yet"
                };
                ui.label(format!("{} ({mark})", h.title()))
                    .on_hover_text(h.about());
            }
            ui.separator();
            if ui.button("Reset all hints").clicked() {
                self.ws.settings.hints.reset();
                for d in &mut self.docs {
                    d.editor.hints.reset();
                }
                ui.close();
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_are_unique_and_the_book_round_trips() {
        let keys: BTreeSet<_> = Hint::ALL.iter().map(|h| h.key()).collect();
        assert_eq!(keys.len(), Hint::ALL.len());
        let mut b = HintBook::default();
        b.mark(Hint::Conversion);
        let json = serde_json::to_string(&b).expect("json");
        let back: HintBook = serde_json::from_str(&json).expect("back");
        assert!(back.is_seen(Hint::Conversion) && !back.is_seen(Hint::Heading));
        assert_eq!(back.seen_count(), 1);
    }
}
