fn main() {
    for path in std::env::args().skip(1) {
        let src = std::fs::read_to_string(&path).unwrap_or_default();
        let mut s = mpp_eval::Session::new();
        let t = std::time::Instant::now();
        let ev = s.run_str(&src);
        println!("=== {path} ({:?})", t.elapsed());
        for b in &ev.blocks {
            for l in &b.values {
                if matches!(
                    l.kind,
                    mpp_eval::LineKind::Query { .. } | mpp_eval::LineKind::Check
                ) || l.auto
                {
                    println!(
                        "  {:?} -> {:?}",
                        src.get(l.span.start..l.span.end).unwrap_or(""),
                        l.answer()
                    );
                }
            }
            for d in &b.diagnostics {
                println!(
                    "  DIAG {:?}: {}",
                    src.get(d.span.start..d.span.end)
                        .unwrap_or("")
                        .chars()
                        .take(60)
                        .collect::<String>(),
                    d.message
                );
            }
        }
        for d in &ev.diagnostics {
            println!("  DOC-DIAG {}", d.message);
        }
    }
}
