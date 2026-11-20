fn main() {
    let path = std::env::args().nth(1).unwrap_or_default();
    let src = std::fs::read_to_string(&path).unwrap_or_default();
    let doc = mpp_syntax::document::parse_document(&src);
    let check = mpp_proof::check_document(&doc, &src, &[]);
    let stem = std::path::Path::new(&path)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("doc");
    let ex = mpp_proof::lean::export(
        &doc,
        &src,
        &check,
        stem,
        &mpp_proof::lean::LeanConfig::default(),
    );
    println!("{}", ex.source());
    for t in &ex.theorems {
        println!(
            "-- {}: lean={:?} latex={:?} warnings={:?}",
            t.name, t.alignment.lean, t.alignment.lean_latex, t.alignment.warnings
        );
    }
}
