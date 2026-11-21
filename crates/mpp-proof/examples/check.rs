fn main() {
    for path in std::env::args().skip(1) {
        let src = std::fs::read_to_string(&path).unwrap();
        let t = std::time::Instant::now();
        let c = mpp_proof::check_source(&src);
        println!("=== {path} ({:?})", t.elapsed());
        print!("{}", c.report());
    }
}
