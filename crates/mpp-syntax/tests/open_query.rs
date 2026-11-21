//! A line ending in `=` asks for its answer (U16).

use mpp_syntax::math::{parse_math, QueryMode, Statement};

fn statement(src: &str) -> (Statement, usize) {
    let (ast, diags) = parse_math(src, 0);
    assert_eq!(ast.lines.len(), 1, "{src}");
    (ast.lines[0].statement.clone(), diags.len())
}

#[test]
fn a_trailing_equals_is_a_question() {
    let (s, errors) = statement("1.321321312 + 5 =");
    assert_eq!(errors, 0);
    assert!(matches!(
        s,
        Statement::Query {
            mode: QueryMode::Exact,
            steps: false,
            ..
        }
    ));
    // A definition ending in `=` asks for the value it defines.
    let (s, errors) = statement("E := 2x + 5 =");
    assert_eq!(errors, 0);
    assert!(matches!(
        s,
        Statement::Definition {
            query: Some(QueryMode::Exact),
            ..
        }
    ));
    // `= ?` is unchanged; an `=` with something after it is still a relation.
    assert!(matches!(statement("x = ?").0, Statement::Query { .. }));
    assert!(matches!(statement("a = b = c").0, Statement::Relation(_)));
    // Only the end of a line asks: the first line of two is a relation chain.
    let (ast, diags) = parse_math(r"a + 1 = \\ b = 2", 0);
    assert!(diags.is_empty(), "{diags:?}");
    assert!(matches!(ast.lines[0].statement, Statement::Query { .. }));
    assert!(matches!(ast.lines[1].statement, Statement::Relation(_)));
}
