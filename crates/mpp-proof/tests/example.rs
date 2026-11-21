//! `examples/proofs.mpp` against `examples/proofs.expected.txt` (the checker's report).
//! `MPP_UPDATE_EXPECTED=1` rewrites the expected file.

use std::path::PathBuf;

#[test]
fn proofs_example() {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples");
    let src = std::fs::read_to_string(dir.join("proofs.mpp")).expect("the example");
    let doc = mpp_syntax::document::parse_document(&src);
    assert!(
        doc.errors().next().is_none(),
        "the example parses with no errors: {:?}",
        doc.diagnostics
    );
    let report = mpp_proof::check_document(&doc, &src, &[]).report();
    let expected_path = dir.join("proofs.expected.txt");
    if std::env::var("MPP_UPDATE_EXPECTED").is_ok_and(|v| v == "1") {
        std::fs::write(&expected_path, &report).expect("write the expected file");
    }
    let expected = std::fs::read_to_string(&expected_path)
        .expect("examples/proofs.expected.txt")
        .replace("\r\n", "\n");
    assert_eq!(report, expected);
}
