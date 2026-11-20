fn main() {
    let path = std::env::args().nth(1).unwrap_or_default();
    let src = std::fs::read_to_string(&path).unwrap_or_default();
    let c = mpp_proof::check_source(&src);
    for r in &c.results {
        for p in &r.proofs {
            for s in &p.steps {
                for cl in &s.claims {
                    println!(
                        "{} step {:?}: {} => {:?}",
                        r.name, s.number, cl.source, cl.outcome
                    );
                }
            }
        }
    }
}
