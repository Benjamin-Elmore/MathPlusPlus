//! The edit history of an open file: Edit -> History lists the steps (as the
//! typeset block each one touched) and any step can be restored. Ctrl+Z and
//! Ctrl+Y stay with the editor, which already groups typing and restores the
//! caret; this keeps the longer, browsable trail beside it.

use std::time::{Duration, Instant};

/// Typing within this long of the last change joins the same step.
const JOIN: Duration = Duration::from_millis(1200);
/// The most steps kept per file.
const LIMIT: usize = 200;

/// One restorable state of a file.
#[derive(Debug, Clone)]
pub struct Step {
    /// The whole text at that moment.
    pub text: String,
    /// Where it was made.
    pub when: Instant,
    /// Where in the text the change was (the first byte that differs).
    pub offset: usize,
}

/// The steps of one file, oldest first. The first is the file as opened.
#[derive(Debug, Clone)]
pub struct Timeline {
    steps: Vec<Step>,
    last_rev: u64,
}

/// The first byte at which two texts differ (on a character boundary).
pub fn first_difference(a: &str, b: &str) -> usize {
    let mut at = 0;
    for ((i, x), y) in a.char_indices().zip(b.chars()) {
        if x != y {
            return i;
        }
        at = i + x.len_utf8();
    }
    at
}

impl Timeline {
    /// Starts a timeline with the text a file has now.
    pub fn new(text: &str, rev: u64, now: Instant) -> Self {
        Timeline {
            steps: vec![Step {
                text: text.to_string(),
                when: now,
                offset: 0,
            }],
            last_rev: rev,
        }
    }

    /// Notes the file's state. Returns true when a step was added or extended.
    pub fn observe(&mut self, rev: u64, text: &str, now: Instant) -> bool {
        if rev == self.last_rev {
            return false;
        }
        self.last_rev = rev;
        let many = self.steps.len() > 1;
        let Some(last) = self.steps.last_mut() else {
            return false;
        };
        if last.text == text {
            return false;
        }
        let offset = first_difference(&last.text, text);
        let join = many && now.duration_since(last.when) < JOIN;
        if join {
            last.text = text.to_string();
            last.when = now;
            return true;
        }
        self.steps.push(Step {
            text: text.to_string(),
            when: now,
            offset,
        });
        if self.steps.len() > LIMIT {
            self.steps.remove(0);
        }
        true
    }

    /// The steps, oldest first.
    pub fn steps(&self) -> &[Step] {
        &self.steps
    }

    /// The text of a step.
    pub fn text_at(&self, i: usize) -> Option<&str> {
        self.steps.get(i).map(|s| s.text.as_str())
    }
}

/// "just now", "12 s ago", "3 min ago", "2 h ago".
pub fn ago(then: Instant, now: Instant) -> String {
    let s = now.duration_since(then).as_secs();
    match s {
        0..=4 => "just now".into(),
        5..=59 => format!("{s} s ago"),
        60..=3599 => format!("{} min ago", s / 60),
        _ => format!("{} h ago", s / 3600),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quick_typing_is_one_step_and_a_pause_starts_another() {
        let t0 = Instant::now();
        let mut tl = Timeline::new("a", 0, t0);
        assert!(tl.observe(1, "ab", t0 + Duration::from_millis(100)));
        assert!(tl.observe(2, "abc", t0 + Duration::from_millis(300)));
        assert_eq!(tl.steps().len(), 2, "joined into one step");
        assert!(tl.observe(3, "abcd", t0 + Duration::from_secs(5)));
        assert_eq!(tl.steps().len(), 3);
        assert_eq!(tl.text_at(1), Some("abc"));
        assert!(!tl.observe(3, "abcd", t0 + Duration::from_secs(6)));
    }

    #[test]
    fn the_difference_is_found_on_a_character_boundary() {
        assert_eq!(first_difference("abc", "abd"), 2);
        assert_eq!(first_difference("a\u{3b1}b", "a\u{3b1}c"), 3);
        assert_eq!(first_difference("abc", "abcd"), 3);
        assert_eq!(first_difference("", "x"), 0);
    }

    #[test]
    fn ages_read_in_words() {
        let n = Instant::now();
        assert_eq!(ago(n, n), "just now");
        assert_eq!(ago(n, n + Duration::from_secs(90)), "1 min ago");
    }
}
