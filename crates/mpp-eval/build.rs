//! Embeds the standard library: every `.mpp` file in the repository's `std/` folder
//! becomes an entry of `FILES` (import path, text) in `$OUT_DIR/stdlib_files.rs`,
//! which `src/stdlib.rs` includes. Files are listed in name order so builds are
//! reproducible.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

fn main() {
    let manifest = std::env::var("CARGO_MANIFEST_DIR").unwrap_or_else(|_| ".".into());
    let std_dir = Path::new(&manifest).join("..").join("..").join("std");
    println!("cargo:rerun-if-changed={}", std_dir.display());

    let mut files: Vec<PathBuf> = std::fs::read_dir(&std_dir)
        .map(|entries| {
            entries
                .filter_map(Result::ok)
                .map(|e| e.path())
                .filter(|p| p.extension().is_some_and(|x| x == "mpp"))
                .collect()
        })
        .unwrap_or_default();
    files.sort();

    let mut out = String::from(
        "/// The embedded standard library: (import path, text), in name order.\n\
         pub(crate) const FILES: &[(&str, &str)] = &[\n",
    );
    for path in &files {
        println!("cargo:rerun-if-changed={}", path.display());
        let Some(name) = path.file_name().map(|n| n.to_string_lossy().into_owned()) else {
            continue;
        };
        let _ = writeln!(
            out,
            "    ({:?}, include_str!({:?})),",
            format!("std/{name}"),
            path.to_string_lossy()
        );
    }
    out.push_str("];\n");

    let out_dir = std::env::var("OUT_DIR").unwrap_or_else(|_| ".".into());
    let target = Path::new(&out_dir).join("stdlib_files.rs");
    if let Err(e) = std::fs::write(&target, out) {
        println!("cargo:warning=could not write {}: {e}", target.display());
    }
}
