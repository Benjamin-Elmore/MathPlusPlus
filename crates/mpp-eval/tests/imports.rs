//! Imports between real files in temporary folders.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use mpp_eval::{
    Binding, EmbeddedSources, Evaluated, FileImporter, LayeredSource, LineResult, Session,
};
use tempfile::TempDir;

fn write(dir: &Path, name: &str, text: &str) -> PathBuf {
    let path = dir.join(name);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("create folder");
    }
    fs::write(&path, text).expect("write file");
    path
}

fn run_file(path: &Path) -> (Session, Evaluated) {
    let mut s = Session::new();
    let ev = s.run_file(path).expect("run").clone();
    (s, ev)
}

fn answers(ev: &Evaluated) -> Vec<&LineResult> {
    ev.queries().map(|(_, l)| l).collect()
}

fn exact(line: &LineResult) -> String {
    line.plain.clone().unwrap_or_else(|| "<none>".into())
}

fn errors(ev: &Evaluated) -> Vec<String> {
    ev.errors().iter().map(|d| d.message.clone()).collect()
}

fn no_errors(ev: &Evaluated) {
    assert!(errors(ev).is_empty(), "unexpected errors: {:?}", errors(ev));
}

/// Every message reported inside imported files, in full.
fn full_messages(ev: &Evaluated) -> Vec<String> {
    ev.blocks
        .iter()
        .flat_map(|b| b.imports.iter())
        .flat_map(|i| i.diagnostics.iter())
        .map(|d| d.diagnostic.message.clone())
        .collect()
}

#[test]
fn everything_import() {
    let dir = TempDir::new().expect("temp dir");
    write(
        dir.path(),
        "lib.mpp",
        "$$ a := 3 $$\n\n$$ f(x) := x + a $$\n",
    );
    let main = write(dir.path(), "main.mpp", "![[lib.mpp]]\n\n$$ f(2) = ? $$\n");
    let (_, ev) = run_file(&main);
    no_errors(&ev);
    assert_eq!(exact(answers(&ev)[0]), "5");
    assert!(ev.bindings.contains_key("a") && ev.bindings.contains_key("f"));
    assert!(ev.blocks[0].imports[0].loaded);
}

#[test]
fn selected_import_takes_only_the_listed_names() {
    let dir = TempDir::new().expect("temp dir");
    write(dir.path(), "lib.mpp", "$$ a := 3 $$\n\n$$ b := 4 $$\n");
    let main = write(
        dir.path(),
        "main.mpp",
        "Using $a$ from [[lib.mpp]].\n\n$$ a = ? $$\n",
    );
    let (_, ev) = run_file(&main);
    no_errors(&ev);
    assert!(ev.bindings.contains_key("a"));
    assert!(!ev.bindings.contains_key("b"));
    assert_eq!(exact(answers(&ev)[0]), "3");
}

#[test]
fn alias_renames_on_the_way_in() {
    let dir = TempDir::new().expect("temp dir");
    write(dir.path(), "lib.mpp", "$$ f(x) := 2 x $$\n");
    let main = write(
        dir.path(),
        "main.mpp",
        "Using $f$ as $g$ from [[lib.mpp]].\n\n$$ g(4) = ? $$\n",
    );
    let (_, ev) = run_file(&main);
    no_errors(&ev);
    assert_eq!(exact(answers(&ev)[0]), "8");
    assert!(!ev.bindings.contains_key("f"));
}

#[test]
fn paths_are_relative_to_the_importing_file() {
    let dir = TempDir::new().expect("temp dir");
    write(dir.path(), "shared/lib.mpp", "$$ k := 7 $$\n");
    write(dir.path(), "sub/mid.mpp", "![[../shared/lib.mpp]]\n");
    let main = write(
        dir.path(),
        "main.mpp",
        "Using $k$ from [[sub/mid.mpp]].\n\n$$ k = ? $$\n",
    );
    let (_, ev) = run_file(&main);
    // mid.mpp imports k but does not export it: only its own definitions are exported.
    assert!(
        errors(&ev)
            .iter()
            .any(|m| m.contains("`k` is not defined in sub/mid.mpp")),
        "{:?}",
        errors(&ev)
    );
    let main2 = write(
        dir.path(),
        "main2.mpp",
        "![[shared/lib.mpp]]\n\n$$ k = ? $$\n",
    );
    let (_, ev) = run_file(&main2);
    no_errors(&ev);
    assert_eq!(exact(answers(&ev)[0]), "7");
}

#[test]
fn nested_imports_carry_what_each_file_defines() {
    let dir = TempDir::new().expect("temp dir");
    write(dir.path(), "c.mpp", "$$ m := 5 $$\n");
    write(
        dir.path(),
        "b.mpp",
        "![[c.mpp]]\n\n$$ E := 2 m $$\n\n$$ h(x) := x m $$\n",
    );
    let a = write(
        dir.path(),
        "a.mpp",
        "Using $E, h$ from [[b.mpp]].\n\n$$ E = ? $$\n\n$$ h(3) = ? $$\n",
    );
    let (_, ev) = run_file(&a);
    no_errors(&ev);
    let a = answers(&ev);
    assert_eq!(exact(a[0]), "10");
    assert_eq!(exact(a[1]), "15");
    // The importer's own `m` does not leak into b's definitions.
    let main = write(
        dir.path(),
        "main.mpp",
        "$$ m := 100 $$\n\nUsing $E$ from [[b.mpp]].\n\n$$ E = ? $$\n",
    );
    let (_, ev) = run_file(&main);
    no_errors(&ev);
    assert_eq!(exact(answers(&ev)[0]), "10");
    assert_eq!(ev.imported_files().len(), 2);
}

#[test]
fn clash_between_two_imports_names_both_files() {
    let dir = TempDir::new().expect("temp dir");
    write(dir.path(), "one.mpp", "$$ x := 1 $$\n");
    write(dir.path(), "two.mpp", "$$ x := 2 $$\n");
    let main = write(
        dir.path(),
        "main.mpp",
        "![[one.mpp]]\n\n![[two.mpp]]\n\n$$ x = ? $$\n",
    );
    let (_, ev) = run_file(&main);
    let errs = errors(&ev);
    assert_eq!(errs.len(), 1, "{errs:?}");
    assert!(
        errs[0].contains("one.mpp") && errs[0].contains("two.mpp"),
        "{errs:?}"
    );
    assert!(errs[0].contains("`x`"), "{errs:?}");
    // The first import keeps the name.
    assert_eq!(exact(answers(&ev)[0]), "1");
}

#[test]
fn clash_with_an_earlier_local_definition() {
    let dir = TempDir::new().expect("temp dir");
    write(dir.path(), "lib.mpp", "$$ x := 1 $$\n");
    let main = write(dir.path(), "main.mpp", "$$ x := 9 $$\n\n![[lib.mpp]]\n");
    let (_, ev) = run_file(&main);
    let errs = errors(&ev);
    assert_eq!(errs.len(), 1, "{errs:?}");
    assert!(
        errs[0].contains("main.mpp") && errs[0].contains("lib.mpp"),
        "{errs:?}"
    );
}

#[test]
fn an_alias_avoids_the_clash() {
    let dir = TempDir::new().expect("temp dir");
    write(dir.path(), "one.mpp", "$$ x := 1 $$\n");
    write(dir.path(), "two.mpp", "$$ x := 2 $$\n");
    let main = write(
        dir.path(),
        "main.mpp",
        "![[one.mpp]]\n\nUsing $x$ as $y$ from [[two.mpp]].\n\n$$ x + y = ? $$\n",
    );
    let (_, ev) = run_file(&main);
    no_errors(&ev);
    assert_eq!(exact(answers(&ev)[0]), "3");
}

#[test]
fn importing_the_same_file_twice_is_not_a_clash() {
    let dir = TempDir::new().expect("temp dir");
    write(dir.path(), "lib.mpp", "$$ x := 1 $$\n");
    let main = write(
        dir.path(),
        "main.mpp",
        "![[lib.mpp]]\n\nUsing $x$ from [[lib.mpp]].\n",
    );
    let (_, ev) = run_file(&main);
    no_errors(&ev);
}

#[test]
fn import_loops_report_the_whole_chain() {
    let dir = TempDir::new().expect("temp dir");
    write(dir.path(), "a.mpp", "![[b.mpp]]\n\n$$ p := 1 $$\n");
    write(dir.path(), "b.mpp", "![[a.mpp]]\n\n$$ q := 2 $$\n");
    let a = dir.path().join("a.mpp");
    let (_, ev) = run_file(&a);
    let full = full_messages(&ev);
    assert!(
        full.iter().any(|m| m.contains("a.mpp → b.mpp → a.mpp")),
        "{full:?}"
    );
    // The import line shows it too, and the document still runs.
    assert!(
        errors(&ev).iter().any(|m| m.contains("error in b.mpp")),
        "{:?}",
        errors(&ev)
    );
    assert!(ev.bindings.contains_key("p"));
}

#[test]
fn a_file_importing_itself_is_a_loop() {
    let dir = TempDir::new().expect("temp dir");
    let a = write(dir.path(), "a.mpp", "![[a.mpp]]\n");
    let (_, ev) = run_file(&a);
    assert!(
        errors(&ev).iter().any(|m| m.contains("a.mpp → a.mpp")),
        "{:?}",
        errors(&ev)
    );
}

#[test]
fn a_loop_below_the_root_is_found_too() {
    let dir = TempDir::new().expect("temp dir");
    write(dir.path(), "b.mpp", "![[c.mpp]]\n");
    write(dir.path(), "c.mpp", "![[b.mpp]]\n");
    let main = write(dir.path(), "main.mpp", "![[b.mpp]]\n");
    let (_, ev) = run_file(&main);
    let full = full_messages(&ev);
    assert!(
        full.iter()
            .any(|m| m.contains("main.mpp → b.mpp → c.mpp → b.mpp")),
        "{full:?}"
    );
}

#[test]
fn a_missing_file_names_the_resolved_path() {
    let dir = TempDir::new().expect("temp dir");
    let main = write(dir.path(), "main.mpp", "![[nowhere/gone.mpp]]\n");
    let (_, ev) = run_file(&main);
    let errs = errors(&ev);
    assert_eq!(errs.len(), 1, "{errs:?}");
    assert!(
        errs[0].contains("gone.mpp") && errs[0].contains("nowhere"),
        "{errs:?}"
    );
    let folder = dir
        .path()
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("");
    assert!(errs[0].contains(folder), "{errs:?}");
    assert!(!ev.blocks[0].imports[0].loaded);
}

#[test]
fn a_missing_name_names_the_name_and_the_file() {
    let dir = TempDir::new().expect("temp dir");
    write(dir.path(), "lib.mpp", "$$ a := 1 $$\n");
    let main = write(
        dir.path(),
        "main.mpp",
        "Using $a, z$ from [[lib.mpp]].\n\n$$ a = ? $$\n",
    );
    let (_, ev) = run_file(&main);
    let errs = errors(&ev);
    assert_eq!(errs.len(), 1, "{errs:?}");
    assert!(
        errs[0].contains("`z`") && errs[0].contains("lib.mpp"),
        "{errs:?}"
    );
    assert_eq!(exact(answers(&ev)[0]), "1");
}

#[test]
fn errors_inside_an_import_are_summarised_on_the_import_line() {
    let dir = TempDir::new().expect("temp dir");
    write(
        dir.path(),
        "mechanics.mpp",
        "$$ a := 1 $$\n\n$$ q \\approx ? $$\n\n$$ r \\approx ? $$\n",
    );
    let main = write(
        dir.path(),
        "main.mpp",
        "![[mechanics.mpp]]\n\n$$ a = ? $$\n",
    );
    let (_, ev) = run_file(&main);
    let errs = errors(&ev);
    assert_eq!(errs, vec!["2 errors in mechanics.mpp".to_string()]);
    let info = &ev.blocks[0].imports[0];
    assert_eq!(info.diagnostics.len(), 2);
    assert_eq!(info.diagnostics[0].file.as_ref(), "mechanics.mpp");
    assert!(info.diagnostics[0].diagnostic.message.contains("no value"));
    assert_eq!(exact(answers(&ev)[0]), "1");
}

#[test]
fn constants_stay_constant_across_imports() {
    let dir = TempDir::new().expect("temp dir");
    write(
        dir.path(),
        "lib.mpp",
        "$$ g \\equiv 9.81 $$\n\n$$ f(x) := x g $$\n",
    );
    let main = write(
        dir.path(),
        "main.mpp",
        "![[lib.mpp]]\n\n$$ g := 3 $$\n\n$$ f(1) = ? $$\n",
    );
    let (_, ev) = run_file(&main);
    assert!(ev.bindings["g"].is_constant());
    let errs = errors(&ev);
    assert_eq!(errs.len(), 1, "{errs:?}");
    assert!(
        errs[0].contains("constant") && errs[0].contains("line 1 of lib.mpp"),
        "{errs:?}"
    );
}

#[test]
fn definitions_know_where_they_came_from() {
    let dir = TempDir::new().expect("temp dir");
    write(dir.path(), "lib.mpp", "# Lib\n\n$$ a := 3 $$\n");
    let main = write(dir.path(), "main.mpp", "![[lib.mpp]]\n\n$$ b := a + 1 $$\n");
    let (_, ev) = run_file(&main);
    no_errors(&ev);
    let origin = ev.definition_of("a").expect("a is defined");
    assert_eq!(origin.file.as_deref(), Some("lib.mpp"));
    assert_eq!(origin.line, Some(3));
    let path = origin.path.expect("has a path");
    assert!(path.ends_with("lib.mpp"));
    let text = fs::read_to_string(&path).expect("read lib");
    assert!(text[origin.span.start..origin.span.end].contains("a := 3"));
    let local = ev.definition_of("b").expect("b is defined");
    assert!(local.file.is_none());
    assert!(ev.definition_of("zzz").is_none());
}

#[test]
fn a_changed_file_is_read_again() {
    let dir = TempDir::new().expect("temp dir");
    let lib = write(dir.path(), "lib.mpp", "$$ a := 3 $$\n");
    let main = write(dir.path(), "main.mpp", "![[lib.mpp]]\n\n$$ a = ? $$\n");
    let (mut s, ev) = run_file(&main);
    assert_eq!(exact(answers(&ev)[0]), "3");

    // A different length is noticed from the file's size alone.
    fs::write(&lib, "$$ a := 40 $$\n").expect("rewrite");
    let src = fs::read_to_string(&main).expect("read");
    assert_eq!(exact(answers(s.update(&src))[0]), "40");

    // The same length and a very recent time are caught by the content check.
    fs::write(&lib, "$$ a := 41 $$\n").expect("rewrite");
    assert_eq!(exact(answers(s.update(&src))[0]), "41");

    // And an explicit invalidate always works.
    fs::write(&lib, "$$ a := 42 $$\n").expect("rewrite");
    s.invalidate(&lib);
    assert_eq!(exact(answers(s.update(&src))[0]), "42");
}

#[test]
fn a_file_that_appears_later_is_picked_up() {
    let dir = TempDir::new().expect("temp dir");
    let main = write(dir.path(), "main.mpp", "![[late.mpp]]\n\n$$ a = ? $$\n");
    let (mut s, ev) = run_file(&main);
    assert!(!errors(&ev).is_empty());
    write(dir.path(), "late.mpp", "$$ a := 5 $$\n");
    let src = fs::read_to_string(&main).expect("read");
    let ev = s.update(&src);
    no_errors(ev);
    assert_eq!(exact(answers(ev)[0]), "5");
}

#[test]
fn a_change_deep_in_the_tree_reaches_the_top() {
    let dir = TempDir::new().expect("temp dir");
    let c = write(dir.path(), "c.mpp", "$$ m := 5 $$\n");
    write(dir.path(), "b.mpp", "![[c.mpp]]\n\n$$ E := 2 m $$\n");
    let a = write(
        dir.path(),
        "a.mpp",
        "Using $E$ from [[b.mpp]].\n\n$$ E = ? $$\n",
    );
    let (mut s, ev) = run_file(&a);
    assert_eq!(exact(answers(&ev)[0]), "10");
    fs::write(&c, "$$ m := 70 $$\n").expect("rewrite");
    s.invalidate(&c);
    let src = fs::read_to_string(&a).expect("read");
    assert_eq!(exact(answers(s.update(&src))[0]), "140");
}

#[test]
fn update_after_an_imported_file_changes_recomputes_only_dependants() {
    let dir = TempDir::new().expect("temp dir");
    let lib = write(dir.path(), "lib.mpp", "$$ a := 3 $$\n\n$$ z := 1 $$\n");
    let src = "![[lib.mpp]]\n\n$$ b := a + 1 $$\n\n$$ c := 10 $$\n\n$$ b = ? $$\n\n$$ c = ? $$\n";
    let main = write(dir.path(), "main.mpp", src);
    let (mut s, ev) = run_file(&main);
    assert_eq!(exact(answers(&ev)[0]), "4");
    assert_eq!(exact(answers(&ev)[1]), "10");
    let ids: Vec<_> = ev.blocks.iter().map(|b| b.id).collect();

    // Nothing changed: only the import itself (which looks at the file) runs.
    let ev = s.update(src);
    assert_eq!(ev.recomputed, vec![ids[0]]);

    // `a` changes: the import, `b` and `b = ?` run; `c` and `c = ?` do not.
    fs::write(&lib, "$$ a := 30 $$\n\n$$ z := 1 $$\n").expect("rewrite");
    s.invalidate(&lib);
    let ev = s.update(src);
    assert_eq!(exact(answers(ev)[0]), "31");
    assert_eq!(exact(answers(ev)[1]), "10");
    assert_eq!(ev.recomputed, vec![ids[0], ids[1], ids[3]]);

    // Only `z` changes: nothing downstream of `a` runs again.
    fs::write(&lib, "$$ a := 30 $$\n\n$$ z := 2 $$\n").expect("rewrite");
    s.invalidate(&lib);
    let ev = s.update(src);
    assert_eq!(ev.recomputed, vec![ids[0]]);
}

#[test]
fn the_example_document_with_real_files() {
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let dir = TempDir::new().expect("temp dir");
    for name in ["launch.mpp", "constants.mpp"] {
        fs::copy(fixtures.join(name), dir.path().join(name)).expect("copy fixture");
    }
    let src = "\
# Projectile motion
Let $g \\equiv 9.81$ be gravity and $v_0 := 20$ the launch speed.
Using $\\theta, R(\\theta)$ from [[launch.mpp]].
![[constants.mpp]]

$$ y(x) := x \\tan\\theta - \\frac{g x^2}{2 v_0^2 \\cos^2\\theta} $$
$$ E := \\tfrac12 m v_0^2 $$

$$ \\frac{d}{dx} y(x) = ? $$
$$ \\int_0^{R} y(x)\\,dx \\approx ? $$
$$ \\sum_{n=1}^{\\infty} \\frac{1}{n^2} = ? $$
$$ \\lim_{x\\to 0} \\frac{\\sin x}{x} = ? $$
$$ E = ? $$

Figure: $y(x)$, $x \\in [0, R]$.
Figure: $z = x^2 - y^2$, $x, y \\in [-2, 2]$.
";
    let main = write(dir.path(), "projectile.mpp", src);
    let (_, ev) = run_file(&main);
    no_errors(&ev);
    assert!(ev.bindings["g"].is_constant());
    assert!(ev.bindings["c"].is_constant());
    assert!(matches!(*ev.bindings["R"], Binding::Function { .. }));
    assert_eq!(
        ev.definition_of("m").and_then(|o| o.file).as_deref(),
        Some("constants.mpp")
    );
    let a = answers(&ev);
    assert_eq!(a.len(), 5);
    assert_eq!(exact(a[4]), "400");
    // The range from launch.mpp (v_0 and g from this document) is the upper limit:
    // R = 40000/981 and the area under the trajectory is R²/6.
    let r: f64 = 40000.0 / 981.0;
    let area: f64 = a[1]
        .approx
        .as_deref()
        .and_then(|t| t.parse().ok())
        .expect("a number");
    assert!((area - r * r / 6.0).abs() < 1e-6, "{area}");
    assert_eq!(exact(a[0]), "-981*x/20000 + 1");
    assert_eq!(
        ev.blocks.iter().filter(|b| !b.imports.is_empty()).count(),
        2
    );
}

#[test]
fn embedded_files_are_served_before_the_file_system() {
    let embedded = EmbeddedSources::new().with("std/consts.mpp", "$$ p \\equiv 2 $$\n");
    let importer = FileImporter::with_source(Arc::new(LayeredSource::over_files(embedded)));
    let dir = TempDir::new().expect("temp dir");
    // A real file of the same name is shadowed.
    write(dir.path(), "std/consts.mpp", "$$ p \\equiv 99 $$\n");
    let main = write(
        dir.path(),
        "main.mpp",
        "![[std/consts.mpp]]\n\n$$ p = ? $$\n",
    );
    let mut s = Session::new().with_importer(Arc::new(importer));
    let ev = s.run_file(&main).expect("run").clone();
    no_errors(&ev);
    assert_eq!(exact(answers(&ev)[0]), "2");
}

#[test]
fn the_cache_is_keyed_by_canonical_path() {
    let dir = TempDir::new().expect("temp dir");
    write(dir.path(), "lib.mpp", "$$ a := 1 $$\n");
    let main = write(dir.path(), "main.mpp", "![[lib.mpp]]\n\n![[./lib.mpp]]\n");
    let importer = FileImporter::new();
    let mut s = Session::new().with_importer(Arc::new(importer.clone()));
    let ev = s.run_file(&main).expect("run").clone();
    no_errors(&ev);
    assert_eq!(importer.cached_files().len(), 1);
}

#[cfg(feature = "watch")]
#[test]
fn the_watcher_invalidates_and_reports_changes() {
    use mpp_eval::FileWatcher;
    use std::sync::mpsc;
    use std::time::Duration;

    let dir = TempDir::new().expect("temp dir");
    let lib = write(dir.path(), "lib.mpp", "$$ a := 3 $$\n");
    let main = write(dir.path(), "main.mpp", "![[lib.mpp]]\n\n$$ a = ? $$\n");
    let (mut s, ev) = run_file(&main);
    assert_eq!(exact(answers(&ev)[0]), "3");

    let (tx, rx) = mpsc::channel();
    let mut watcher = FileWatcher::new(s.importer(), move |p: &Path| {
        let _ = tx.send(p.to_path_buf());
    })
    .expect("watcher");
    watcher.watch_imports(&ev).expect("watch");
    assert_eq!(watcher.files().len(), 1);

    fs::write(&lib, "$$ a := 8 $$\n").expect("rewrite");
    rx.recv_timeout(Duration::from_secs(10))
        .expect("a change event");
    let src = fs::read_to_string(&main).expect("read");
    assert_eq!(exact(answers(s.update(&src))[0]), "8");
}

#[test]
fn an_error_under_a_diamond_of_imports_is_reported_once() {
    // Each file imports the two files below it; the bottom one has one syntax error.
    // The summaries used to be rebuilt per path, doubling time and memory per layer.
    let dir = TempDir::new().expect("temp dir");
    let layers = 20;
    write(dir.path(), &format!("d{layers}.mpp"), "$$ z := 1 + $$\n");
    write(dir.path(), &format!("e{layers}.mpp"), "$$ w := 2 $$\n");
    for i in (0..layers).rev() {
        let n = i + 1;
        let imports = format!("![[d{n}.mpp]]\n\n![[e{n}.mpp]]\n\n");
        write(
            dir.path(),
            &format!("d{i}.mpp"),
            &format!("{imports}$$ x_{{{i}}} := 1 $$\n"),
        );
        write(
            dir.path(),
            &format!("e{i}.mpp"),
            &format!("{imports}$$ y_{{{i}}} := 1 $$\n"),
        );
    }
    let start = std::time::Instant::now();
    let (_, ev) = run_file(&dir.path().join("d0.mpp"));
    assert!(
        start.elapsed() < std::time::Duration::from_secs(20),
        "{:?}",
        start.elapsed()
    );
    let errs = errors(&ev);
    assert_eq!(errs.len(), 2, "{errs:?}");
    assert!(
        errs.iter()
            .all(|m| m.starts_with("1 error in ") && m.contains("d20.mpp")),
        "{errs:?}"
    );
    let full = full_messages(&ev);
    assert!(full.len() < 200, "{} copies", full.len());
}

/// Writes a chain f0 → f1 → … → f{len}, where each file defines v_i := v_{i+1} + 1.
fn import_chain(dir: &Path, len: usize) {
    write(
        dir,
        &format!("f{len}.mpp"),
        &format!("$$ v_{{{len}}} := 0 $$\n"),
    );
    for i in 0..len {
        let n = i + 1;
        write(
            dir,
            &format!("f{i}.mpp"),
            &format!("![[f{n}.mpp]]\n\n$$ v_{{{i}}} := v_{{{n}}} + 1 $$\n"),
        );
    }
}

#[test]
fn long_import_chains_work_and_a_chain_past_the_limit_says_so_once() {
    let dir = TempDir::new().expect("temp dir");
    import_chain(dir.path(), 300);
    write(dir.path(), "top.mpp", "![[f0.mpp]]\n\n$$ v_0 = ? $$\n");
    let start = std::time::Instant::now();
    let (_, ev) = run_file(&dir.path().join("top.mpp"));
    let took = start.elapsed();
    no_errors(&ev);
    assert_eq!(exact(answers(&ev)[0]), "300");
    assert!(took < std::time::Duration::from_secs(60), "{took:?}");

    let dir = TempDir::new().expect("temp dir");
    import_chain(dir.path(), 1002);
    write(dir.path(), "top.mpp", "![[f0.mpp]]\n\n$$ v_0 = ? $$\n");
    let (_, ev) = run_file(&dir.path().join("top.mpp"));
    let errs = errors(&ev);
    assert_eq!(errs.len(), 1, "{errs:?}");
    assert!(errs[0].contains("more than 1000 files deep"), "{errs:?}");
    assert!(errs[0].len() < 200, "{errs:?}");
}
