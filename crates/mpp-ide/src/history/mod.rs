//! Helpers for Edit -> History, which lists the editor's undo steps (as the
//! typeset block each one touched): where two states differ, and how long ago
//! a step was made.

use std::time::Instant;

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
    use std::time::Duration;

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
