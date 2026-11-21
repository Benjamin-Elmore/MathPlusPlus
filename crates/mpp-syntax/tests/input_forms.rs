//! Every row of the input forms table (`mpp_syntax::forms`): each way of typing
//! it, and what the editor writes for it, reads cleanly, and (unless the row
//! says the notation differs) as the same tree as the LaTeX form.

use mpp_syntax::forms::forms;
use mpp_syntax::math::{parse_math, sexpr};

fn tree(src: &str) -> Result<String, String> {
    let (ast, diags) = parse_math(src, 0);
    if diags.is_empty() {
        Ok(sexpr::ast(&ast, sexpr::Mode::Semantic))
    } else {
        Err(diags
            .iter()
            .map(|d| d.to_string())
            .collect::<Vec<_>>()
            .join("; "))
    }
}

#[test]
fn every_form_of_every_row_parses_to_the_same_tree() {
    let mut failures = vec![];
    for f in forms() {
        let want = match tree(f.latex) {
            Ok(t) => t,
            Err(e) => {
                failures.push(format!("{}: LaTeX `{}`: {e}", f.id, f.latex));
                continue;
            }
        };
        let mut all = vec![
            ("code", f.code),
            ("unicode", f.unicode),
            ("unicodemath", f.unicodemath),
            ("writes", f.writes),
            ("picture", f.picture),
        ];
        all.extend(f.also.iter().map(|a| ("also", *a)));
        for (what, src) in all {
            match tree(src) {
                Ok(t) if !f.same_tree || t == want => {}
                Ok(t) => failures.push(format!(
                    "{}: {what} `{src}` gives {t}, LaTeX gives {want}",
                    f.id
                )),
                Err(e) => failures.push(format!("{}: {what} `{src}`: {e}", f.id)),
            }
        }
    }
    assert!(failures.is_empty(), "\n{}", failures.join("\n"));
}

#[test]
fn call_forms_need_the_right_shape() {
    // A sum of a tuple stays a sum of a tuple; a spaced bracket is not a call.
    assert_eq!(tree("sum(f, n)").unwrap(), "(sum (tuple paren f n))");
    assert!(tree("int (f, x, 0, 1)").is_err());
    assert_eq!(
        tree("sum(n^2, n, 1, 10)").unwrap(),
        "(sum :index n :from 1 :to 10 (^ n 2))"
    );
    assert_eq!(
        tree("diff(x^3, x, 2)").unwrap(),
        tree(r"\frac{d^{2}}{dx^{2}} x^3").unwrap()
    );
    assert!(tree("diff(f)").is_err());
    assert_eq!(tree("2**3**2").unwrap(), tree("2^{3^{2}}").unwrap());
}
