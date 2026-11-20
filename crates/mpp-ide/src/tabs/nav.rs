//! Back and forward through the places you jumped between (Alt+Left, Alt+Right).

use std::path::PathBuf;

/// A place: a file and a position in it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Place {
    pub path: PathBuf,
    pub offset: usize,
}

const LIMIT: usize = 100;

/// The jump history.
#[derive(Debug, Clone, Default)]
pub struct NavHistory {
    places: Vec<Place>,
    at: usize,
}

impl NavHistory {
    /// Notes a place. Anything ahead of the current spot is dropped, as in a browser.
    pub fn record(&mut self, place: Place) {
        if let Some(cur) = self.places.get(self.at) {
            if *cur == place {
                return;
            }
            // The same file with the caret nearby replaces the entry.
            if cur.path == place.path && cur.offset.abs_diff(place.offset) < 3 {
                self.places[self.at] = place;
                return;
            }
            self.places.truncate(self.at + 1);
        }
        self.places.push(place);
        if self.places.len() > LIMIT {
            self.places.remove(0);
        }
        self.at = self.places.len() - 1;
    }

    /// True when there is somewhere to go back to.
    pub fn can_back(&self) -> bool {
        self.at > 0
    }

    /// True when there is somewhere to go forward to.
    pub fn can_forward(&self) -> bool {
        self.at + 1 < self.places.len()
    }

    /// Steps back and returns the place.
    pub fn back(&mut self) -> Option<Place> {
        if self.can_back() {
            self.at -= 1;
            self.places.get(self.at).cloned()
        } else {
            None
        }
    }

    /// Steps forward and returns the place.
    pub fn forward(&mut self) -> Option<Place> {
        if self.can_forward() {
            self.at += 1;
            self.places.get(self.at).cloned()
        } else {
            None
        }
    }

    /// Forgets every entry for a file (it was deleted).
    pub fn forget(&mut self, path: &std::path::Path) {
        self.places.retain(|p| p.path != path);
        self.at = self.at.min(self.places.len().saturating_sub(1));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(f: &str, o: usize) -> Place {
        Place {
            path: PathBuf::from(f),
            offset: o,
        }
    }

    #[test]
    fn back_and_forward_walk_the_trail() {
        let mut h = NavHistory::default();
        h.record(p("a", 0));
        h.record(p("b", 5));
        h.record(p("c", 9));
        assert_eq!(h.back(), Some(p("b", 5)));
        assert_eq!(h.back(), Some(p("a", 0)));
        assert_eq!(h.back(), None);
        assert_eq!(h.forward(), Some(p("b", 5)));
        h.record(p("d", 1));
        assert!(!h.can_forward(), "a new jump drops the forward trail");
        assert_eq!(h.back(), Some(p("b", 5)));
    }
}
