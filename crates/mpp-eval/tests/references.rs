//! `References from [[file]]`: the evaluator reads the file and checks every
//! citation key against its entries.

use std::sync::Arc;

use mpp_eval::{FileImporter, Session};

fn run(dir: &std::path::Path, src: &str) -> Vec<String> {
    let main = dir.join("main.mpp");
    std::fs::write(&main, src).unwrap();
    let mut session = Session::new().with_importer(Arc::new(FileImporter::new()));
    let ev = session.run_file(&main).unwrap();
    ev.errors()
        .iter()
        .map(|d| mpp_core::note::plain(&d.message))
        .collect()
}

#[test]
fn keys_are_checked_against_the_references_file() {
    let dir = std::env::temp_dir().join(format!("mpp-refs-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("refs.mpp"),
        "References\n[@knuth84] Knuth, D. E. | The TeXbook | 1984\n",
    )
    .unwrap();
    let ok = run(&dir, "As in [@knuth84].\n\nReferences from [[refs.mpp]]\n");
    assert!(ok.is_empty(), "{ok:?}");
    let bad = run(
        &dir,
        "As in [@lamport94].\n\nReferences from [[refs.mpp]]\n",
    );
    assert!(
        bad.iter().any(|m| m.contains("lamport94")),
        "unknown key reported: {bad:?}"
    );
    let missing = run(&dir, "As in [@knuth84].\n\nReferences from [[nope.mpp]]\n");
    assert!(
        missing.iter().any(|m| m.contains("nope.mpp")),
        "unreadable file reported: {missing:?}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}
