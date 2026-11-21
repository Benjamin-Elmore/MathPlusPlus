//! Recent files, remembered between runs through eframe's storage.

use std::path::PathBuf;

const MAX_RECENT: usize = 10;

/// The most recently used files, newest first.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Recent {
    paths: Vec<PathBuf>,
}

impl Recent {
    /// Reads the list from its stored form (one path per line).
    pub fn from_stored(s: &str) -> Self {
        Recent {
            paths: s
                .lines()
                .filter(|l| !l.trim().is_empty())
                .map(PathBuf::from)
                .take(MAX_RECENT)
                .collect(),
        }
    }

    /// The stored form.
    pub fn to_stored(&self) -> String {
        self.paths
            .iter()
            .map(|p| p.to_string_lossy().into_owned())
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// Moves a file to the front.
    pub fn touch(&mut self, path: &std::path::Path) {
        self.paths.retain(|p| p != path);
        self.paths.insert(0, path.to_path_buf());
        self.paths.truncate(MAX_RECENT);
    }

    /// The files, newest first.
    pub fn paths(&self) -> &[PathBuf] {
        &self.paths
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn newest_first_without_duplicates() {
        let mut r = Recent::default();
        r.touch(std::path::Path::new("a.mpp"));
        r.touch(std::path::Path::new("b.mpp"));
        r.touch(std::path::Path::new("a.mpp"));
        assert_eq!(r.paths().len(), 2);
        assert_eq!(r.paths()[0], PathBuf::from("a.mpp"));
        assert_eq!(Recent::from_stored(&r.to_stored()), r);
    }
}
