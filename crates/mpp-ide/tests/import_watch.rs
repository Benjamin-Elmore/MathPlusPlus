//! An open file picks up changes made to the files it imports.

use std::time::{Duration, Instant};

use mpp_ide::IdeApp;

fn answer(app: &IdeApp) -> Option<String> {
    let doc = app.doc()?;
    let ev = doc.eval.as_ref()?;
    ev.queries().next().and_then(|q| q.1.plain.clone())
}

#[test]
fn a_changed_imported_file_updates_the_open_file_without_typing() {
    let dir = std::env::temp_dir().join(format!("mpp-import-watch-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("dir");
    let lib = dir.join("lib.mpp");
    let main = dir.join("main.mpp");
    std::fs::write(&lib, "$$ a := 10 $$\n").expect("lib");
    std::fs::write(&main, "![[lib.mpp]]\n\n$$ a + 1 = ? $$\n").expect("main");

    let mut app = IdeApp::empty();
    app.open_file(&main, false).expect("open");
    app.doc_mut().expect("doc").settle();
    assert_eq!(answer(&app).as_deref(), Some("11"));

    // Saved from another tab (here, straight to disk), with a different length.
    std::fs::write(&lib, "$$ a := 200 $$\n").expect("rewrite");
    let start = Instant::now();
    while answer(&app).as_deref() != Some("201") && start.elapsed() < Duration::from_secs(6) {
        app.doc_mut().expect("doc").pump();
        std::thread::sleep(Duration::from_millis(50));
    }
    assert_eq!(answer(&app).as_deref(), Some("201"));
    let _ = std::fs::remove_dir_all(&dir);
}
