//! Time limits and cancellation of long calculations.

use std::sync::Arc;
use std::time::{Duration, Instant};

use mpp_eval::{NoImports, Session};

const SLOW: &str = "$$ \\operatorname{factor}(2^{128}+1) = ? $$\n";

fn errors(session: &mut Session, src: &str) -> Vec<String> {
    session
        .run_str(src)
        .errors()
        .iter()
        .map(|d| d.message.clone())
        .collect()
}

#[test]
fn a_time_limit_stops_a_long_query_with_a_clear_message() {
    let mut session = Session::new().with_importer(Arc::new(NoImports));
    session.set_time_limit(Some(Duration::from_millis(300)));
    let start = Instant::now();
    let errs = errors(
        &mut session,
        "$$ \\operatorname{factor}(2^{128}+1) = ? $$\n",
    );
    assert!(start.elapsed() < Duration::from_secs(20));
    let first = errs.first().expect("the query must fail");
    assert!(first.contains("stopped after 0.3 s"), "{first}");
}

#[test]
fn the_limit_is_per_query_not_per_document() {
    let mut session = Session::new().with_importer(Arc::new(NoImports));
    session.set_time_limit(Some(Duration::from_secs(30)));
    let src = "$$ 1 + 1 = ? $$\n\n$$ 2 + 2 = ? $$\n";
    assert!(errors(&mut session, src).is_empty());
}

#[test]
fn cancelling_stops_the_run_and_the_next_run_starts_afresh() {
    let mut session = Session::new().with_importer(Arc::new(NoImports));
    let handle = session.cancel_handle();
    let stopper = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(200));
        handle.cancel();
    });
    let start = Instant::now();
    let _ = errors(&mut session, SLOW);
    stopper.join().expect("stopper");
    assert!(start.elapsed() < Duration::from_secs(60));
    // The stop is forgotten: the same session answers the next text normally.
    let errs = errors(&mut session, "$$ 3 + 4 = ? $$\n");
    assert!(errs.is_empty(), "{errs:?}");
}

#[test]
fn a_stopped_block_is_run_again_next_time() {
    let mut session = Session::new().with_importer(Arc::new(NoImports));
    let handle = session.cancel_handle();
    let stopper = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(100));
        handle.cancel();
    });
    let _ = errors(&mut session, SLOW);
    stopper.join().expect("stopper");
    session.set_time_limit(Some(Duration::from_millis(200)));
    let ev = session.update(SLOW);
    assert!(
        !ev.recomputed.is_empty(),
        "a cancelled block must not be reused from the cache"
    );
}

#[test]
fn a_stop_asked_for_before_a_kept_run_starts_is_not_lost() {
    // The editor's worker takes a job, then an edit asks for a stop before the run
    // starts: `update` forgot that stop and ran the stale job to the end, while
    // `update_keeping_cancel` honours it (the worker resets the stop itself).
    let mut session = Session::new().with_importer(Arc::new(NoImports));
    let handle = session.cancel_handle();
    handle.cancel();
    let start = Instant::now();
    let _ = session.update_keeping_cancel(SLOW);
    assert!(
        start.elapsed() < Duration::from_secs(5),
        "{:?}",
        start.elapsed()
    );
    assert!(handle.is_cancelled());
    handle.reset();
    assert!(!handle.is_cancelled());
    let ev = session.update_keeping_cancel("$$ 3 + 4 = ? $$\n");
    assert!(ev.errors().is_empty());
}
