//! Reading imported `.mpp` files from disk.

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;

use mpp_eval::{Exports, ImportRequest, Importer, Session};

/// Imports files from the file system, resolving paths against the importing
/// file's folder. Nested imports are followed up to a fixed depth, which also
/// stops import loops.
#[derive(Debug, Clone, Default)]
pub struct FsImporter {
    depth: usize,
}

const MAX_DEPTH: usize = 6;

impl Importer for FsImporter {
    fn import(&self, base: Option<&Path>, request: &ImportRequest) -> Result<Exports, String> {
        if self.depth >= MAX_DEPTH {
            return Err("these files import each other too deeply".into());
        }
        let mut path = base.unwrap_or_else(|| Path::new(".")).join(&request.path);
        if path.extension().is_none() {
            path.set_extension("mpp");
        }
        let text = std::fs::read_to_string(&path)
            .map_err(|_| format!("the file {} could not be opened", request.path))?;
        let mut session = Session::new().with_importer(Arc::new(FsImporter {
            depth: self.depth + 1,
        }));
        session.set_base_dir(path.parent().map(Path::to_path_buf));
        let ev = session.run_str(&text);
        let all = &ev.bindings;
        let bindings = match &request.names {
            None => all.clone(),
            Some(names) => {
                let mut m = BTreeMap::new();
                for (theirs, mine) in names {
                    if let Some(b) = all.get(theirs) {
                        m.insert(mine.clone(), b.clone());
                    }
                }
                m
            }
        };
        Ok(Exports {
            file: Arc::from(path.to_string_lossy().as_ref()),
            bindings,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mpp_syntax::Span;

    #[test]
    fn missing_files_give_a_message() {
        let imp = FsImporter::default();
        let req = ImportRequest {
            path: "does-not-exist.mpp".into(),
            span: Span::new(0, 0),
            names: None,
        };
        assert!(imp.import(None, &req).is_err());
    }
}
