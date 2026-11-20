//! The domain registry in documents: matrices, equations and systems, number theory,
//! statistics and differential equations, written the way a worksheet writes them.

use std::sync::Arc;

use mpp_eval::{Evaluated, LineResult, NoImports, Session};

fn run(src: &str) -> Evaluated {
    Session::new()
        .with_importer(Arc::new(NoImports))
        .run_str(src)
        .clone()
}

fn errors(ev: &Evaluated) -> Vec<String> {
    ev.errors().iter().map(|d| d.message.clone()).collect()
}

fn answers(src: &str) -> Vec<String> {
    let ev = run(src);
    let errs = errors(&ev);
    assert!(errs.is_empty(), "{src}: unexpected errors {errs:?}");
    ev.queries()
        .map(|(_, l): (_, &LineResult)| l.text.clone().unwrap_or_else(|| "<none>".into()))
        .collect()
}

fn answer(src: &str) -> String {
    let a = answers(src);
    assert_eq!(a.len(), 1, "{src}: {a:?}");
    a[0].clone()
}

fn failure(src: &str) -> String {
    let errs = errors(&run(src));
    assert_eq!(errs.len(), 1, "{src}: {errs:?}");
    errs[0].clone()
}

const AB: &str = r"$$ A := \begin{pmatrix} 1 & 2 \\ 3 & 4 \end{pmatrix} $$
$$ B := \begin{pmatrix} 0 & 1 \\ 1 & 0 \end{pmatrix} $$
";

fn with_ab(query: &str) -> String {
    answer(&format!("{AB}\n$$ {query} = ? $$\n"))
}

#[test]
fn determinants_inverses_and_transposes() {
    assert_eq!(with_ab(r"\det A"), "-2");
    assert_eq!(with_ab(r"\operatorname{det}(A)"), "-2");
    assert_eq!(
        answer(r"$$ \begin{vmatrix} 2 & 1 \\ 7 & 4 \end{vmatrix} = ? $$"),
        "1"
    );
    assert_eq!(with_ab("A^{-1}"), "[[-2, 1], [3/2, -1/2]]");
    assert_eq!(with_ab("A^T"), "[[1, 3], [2, 4]]");
    assert_eq!(with_ab(r"A^{\top}"), "[[1, 3], [2, 4]]");
    assert_eq!(with_ab(r"\operatorname{rank} A"), "2");
    assert_eq!(with_ab(r"\operatorname{trace}(A)"), "5");
    assert_eq!(with_ab("A^{-1} A"), "[[1, 0], [0, 1]]");
    assert_eq!(with_ab(r"\det(A^2)"), "4");
}

#[test]
fn matrix_products_keep_their_order() {
    assert_eq!(with_ab("A B"), "[[2, 1], [4, 3]]");
    assert_eq!(with_ab("B A"), "[[3, 4], [1, 2]]");
    assert_eq!(with_ab(r"A \cdot B"), "[[2, 1], [4, 3]]");
    assert_eq!(with_ab("A A"), "[[7, 10], [15, 22]]");
    assert_eq!(with_ab("A^3"), "[[37, 54], [81, 118]]");
    assert_eq!(with_ab("2 A"), "[[2, 4], [6, 8]]");
    assert_eq!(with_ab("A + B"), "[[1, 3], [4, 4]]");
    assert_eq!(with_ab("A - B"), "[[1, 1], [2, 4]]");
    assert_eq!(with_ab("(A + B) A"), "[[10, 14], [16, 24]]");
    // Defined through other matrices.
    let src = format!("{AB}\n$$ C := A B $$\n$$ C - A B = ? $$\n$$ C B = ? $$\n");
    assert_eq!(answers(&src), vec!["[[0, 0], [0, 0]]", "[[1, 2], [3, 4]]"]);
    // A vector: products with a column, dot and cross products.
    let v = r"$$ u := \begin{pmatrix} 1 \\ 0 \\ 0 \end{pmatrix} $$
$$ w := \begin{pmatrix} 0 \\ 1 \\ 0 \end{pmatrix} $$
";
    assert_eq!(
        answer(&format!(r"{v}$$ u \times w = ? $$")),
        "[[0], [0], [1]]"
    );
    assert_eq!(answer(&format!(r"{v}$$ u \cdot w = ? $$")), "0");
    assert_eq!(
        with_ab(r"A \begin{pmatrix} 1 \\ 1 \end{pmatrix}"),
        "[[3], [7]]"
    );
}

#[test]
fn matrix_mistakes_are_errors() {
    assert!(failure(&format!("{AB}\n$$ A + 1 = ? $$")).contains("matrix and a number"));
    let src = r"$$ \begin{pmatrix} 1 & 2 \end{pmatrix} \begin{pmatrix} 1 & 2 \end{pmatrix} = ? $$";
    assert!(failure(src).contains("multiply a 1×2 matrix by a 1×2 matrix"));
    let singular = r"$$ \begin{pmatrix} 1 & 2 \\ 2 & 4 \end{pmatrix}^{-1} = ? $$";
    assert!(failure(singular).contains("singular"));
}

#[test]
fn eigenvalues_and_row_reduction() {
    let src = r"$$ M := \begin{pmatrix} 2 & 1 \\ 1 & 2 \end{pmatrix} $$
$$ \operatorname{eigen}(M) = ? $$
$$ \operatorname{rref}(M) = ? $$
";
    assert_eq!(answers(src), vec!["(1, 3)", "[[1, 0], [0, 1]]"]);
}

#[test]
fn equations_and_systems() {
    assert_eq!(
        answer(r"$$ \operatorname{solve}_x (x^2 - 4 = 0) = ? $$"),
        "{-2, 2}"
    );
    assert_eq!(
        answer(r"$$ \operatorname{solve}(x^2 - 5x + 6 = 0, x) = ? $$"),
        "{2, 3}"
    );
    assert_eq!(
        answer("Solve $x^2 - 5x + 6 = 0$ for $x$."),
        "x = 2 or x = 3"
    );
    assert_eq!(answer("Solve $2x + 1 = 7$."), "x = 3");
    assert_eq!(
        answer("Solve $x + y = 3, x - y = 1$ for $x, y$."),
        "(x = 2, y = 1)"
    );
    assert_eq!(
        answer(r"Solve $\begin{cases} 2x + y = 5 \\ x - y = 1 \end{cases}$ for $x, y$."),
        "(x = 2, y = 1)"
    );
    assert_eq!(
        answer(r"$$ \operatorname{solve}_{x, y} (x + y = 3, x - y = 1) = ? $$"),
        "(x = 2, y = 1)"
    );
    assert_eq!(answer("Solve $x^2 = -1$ for $x$."), "x = -i or x = i");
    // The unknown stays symbolic even when the document gives it a value.
    assert_eq!(
        answers("$$ x := 5 $$\n\nSolve $x^2 = 9$ for $x$.\n"),
        vec!["x = -3 or x = 3"]
    );
    // A parameter from the document goes in.
    assert_eq!(
        answers("$$ a := 4 $$\n\nSolve $x^2 = a$ for $x$.\n"),
        vec!["x = -2 or x = 2"]
    );
}

#[test]
fn number_theory() {
    assert_eq!(answer(r"$$ 17 \bmod 5 = ? $$"), "2");
    assert_eq!(answer(r"$$ \operatorname{mod}(-7, 3) = ? $$"), "2");
    assert_eq!(answer(r"$$ \gcd(12, 18) = ? $$"), "6");
    assert_eq!(answer(r"$$ \operatorname{lcm}(4, 6) = ? $$"), "12");
    assert_eq!(answer(r"$$ \operatorname{isprime}(97) = ? $$"), "true");
    assert_eq!(answer(r"$$ \operatorname{isprime}(91) = ? $$"), "false");
    assert_eq!(
        answer(r"$$ \operatorname{factor}(360) = ? $$"),
        "2^3 · 3^2 · 5"
    );
    assert_eq!(answer(r"$$ \operatorname{factor}(13) = ? $$"), "13");
    assert_eq!(
        answer(r"$$ \operatorname{factor}(x^2 - 1) = ? $$"),
        "(x - 1)*(x + 1)"
    );
    assert_eq!(answer(r"$$ \binom{6}{2} = ? $$"), "15");
}

#[test]
fn statistics() {
    assert_eq!(answer(r"$$ \operatorname{mean}(2, 4, 9) = ? $$"), "5");
    assert_eq!(
        answer(r"$$ \operatorname{median}(3, 1, 4, 1, 5) = ? $$"),
        "3"
    );
    assert_eq!(answer(r"$$ \operatorname{var}(1, 2, 3, 4) = ? $$"), "5/3");
    assert_eq!(answer(r"$$ \operatorname{pvar}(1, 2, 3, 4) = ? $$"), "5/4");
    assert_eq!(
        answer(r"$$ \operatorname{pstd}(2, 4, 4, 4, 5, 5, 7, 9) = ? $$"),
        "2"
    );
    assert_eq!(
        answer(r"$$ \operatorname{std}(2, 4, 4, 4, 5, 5, 7, 9) = ? $$"),
        answer(r"$$ \sqrt{\frac{32}{7}} = ? $$")
    );
}

#[test]
fn sets_and_membership() {
    assert_eq!(answer(r"$$ 3 \in [0, 5] = ? $$"), "true");
    assert_eq!(answer(r"$$ 7 \notin \{1, 2, 3\} = ? $$"), "true");
    assert_eq!(answer(r"$$ \{1, 2\} \cup \{2, 3\} = ? $$"), "{1, 2, 3}");
}

#[test]
fn lagrange_systems_with_gradients() {
    let a = answer(
        r"$$ \operatorname{solve}_{x, y, \lambda} \left( \nabla (x y) = \lambda \nabla (x^2 + y^2 - 1), x^2 + y^2 = 1 \right) = ? $$",
    );
    assert_eq!(
        a,
        "{(x = -sqrt(2)/2, y = -sqrt(2)/2, λ = 1/2), (x = sqrt(2)/2, y = sqrt(2)/2, λ = 1/2), \
         (x = -sqrt(2)/2, y = sqrt(2)/2, λ = -1/2), (x = sqrt(2)/2, y = -sqrt(2)/2, λ = -1/2)}"
    );
}

#[test]
fn differential_equations_use_the_full_integrator() {
    // ∫ x e^{x²} dx needs a substitution, which the basic built-in calculus lacks.
    let a = answer(r"$$ \operatorname{dsolve}(y' = x e^{x^2}, y, x) = ? $$");
    assert!(a.contains("e^(x^2)/2") && a.contains('C'), "{a}");
    let b = answer(r"$$ \operatorname{dsolve}(\frac{dy}{dx} = y, y, x, (0, 2)) = ? $$");
    assert_eq!(b, "y = 2*e^x");
}
