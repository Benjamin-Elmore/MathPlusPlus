//! Performance and robustness of the IDE on awkward folders and files: a slow file
//! next to the open one, a byte-order mark, a very long line, two windows sharing the
//! recovery folder.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use mpp_ide::IdeApp;

fn scratch(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("mpp-robust-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).expect("scratch folder");
    d
}

#[test]
fn a_slow_file_in_the_folder_never_holds_up_the_open_file() {
    let d = scratch("sibling");
    // Factoring this number takes about ten seconds; the folder index used to evaluate
    // every sibling file before the open file's results were shown.
    std::fs::write(
        d.join("slow.mpp"),
        "$$ \\theta := 2 $$\n\n$$ \\operatorname{factor}(2^{256} + 1) = ? $$\n",
    )
    .expect("write");
    let open = d.join("open.mpp");
    std::fs::write(&open, "$$ 1 + 1 = ? $$\n").expect("write");
    let mut app = IdeApp::empty();
    app.open_file(&open, false).expect("open");
    let start = Instant::now();
    let doc = app.doc_mut().expect("a document");
    doc.settle();
    let took = start.elapsed();
    assert!(took < Duration::from_secs(5), "settled in {took:?}");
    let ev = doc.eval.as_ref().expect("evaluated");
    assert_eq!(
        ev.queries().next().and_then(|(_, l)| l.plain.clone()),
        Some("2".to_string())
    );
    // The sibling's names are still indexed, read without evaluating it.
    let bg = doc.background().expect("background");
    assert_eq!(bg.index.files_defining("θ"), ["slow.mpp"]);
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn a_second_window_never_takes_the_first_windows_recovery_copies() {
    use mpp_ide::settings::AutosaveMode;
    let d = scratch("two-windows");
    let data = d.join("appdata");
    let f = d.join("m.mpp");
    std::fs::write(&f, "# M\n").expect("write");
    let offered = |app: &IdeApp| app.ws.autosave.dialog.as_ref().map_or(0, |x| x.files.len());
    // Window A edits and keeps a recovery copy.
    let mut a = IdeApp::empty();
    a.start_up(&data, None);
    a.open_file(&f, false).expect("open");
    a.set_autosave_mode(AutosaveMode::Off);
    {
        let doc = a.doc_mut().expect("doc");
        doc.editor.move_caret(4, false);
        doc.editor.insert_text("\n$$ precious := 1 $$\n");
        doc.touched();
    }
    a.write_recovery();
    // Window B starts while A runs: A's live edits are not a crash.
    let mut b = IdeApp::empty();
    b.start_up(&data, None);
    assert_eq!(offered(&b), 0);
    // B closes cleanly; that must not delete A's copy.
    b.clear_recovery_on_exit();
    drop(b);
    a.write_recovery();
    // A crashes (no clean exit): the next start offers its copy.
    drop(a);
    let mut c = IdeApp::empty();
    c.start_up(&data, None);
    assert_eq!(offered(&c), 1);
    let _ = std::fs::remove_dir_all(&d);
}
