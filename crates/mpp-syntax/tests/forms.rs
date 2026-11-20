//! Every construct of grammar §2 in each input form it has (LaTeX first, then
//! plain, then Unicode). All forms of a case must give the same tree once
//! presentation is ignored; the snapshot records the full trees.

mod common;

use common::{report, Case};

macro_rules! cases {
    ($($name:literal => [$($form:literal),+ $(,)?]),+ $(,)?) => {
        &[$(Case { name: $name, forms: &[$($form),+] }),+]
    };
}

#[test]
fn statements() {
    let cases: &[Case] = cases![
        "definition :=" => [r"a := 3", r"a ≔ 3"],
        "function definition" => [r"f(x, y) := x^2 + y^2", r"f(x, y) := x^2 + y^2", r"f(x, y) ≔ x² + y²"],
        "constant ≡" => [r"g \equiv 9.81", r"g equiv 9.81", r"g ≡ 9.81"],
        "recurrence" => [r"a_{n+1} := 2 a_n", r"a_(n+1) := 2 a_n", r"aₙ₊₁ ≔ 2aₙ"],
        "parameter i is a name in the body" => [r"f(i) := 2i + 1", r"f(i) ≔ 2i + 1"],
        "initial term" => [r"a_1 := 1", r"a_1 := 1", r"a₁ ≔ 1"],
        "query exact" => [r"\int_0^1 x^2 \, dx = ?", r"int_0^1 x^2 dx = ?", r"∫₀¹ x² dx = ?"],
        "query approx" => [r"\sqrt{2} \approx ?", r"sqrt(2) approx ?", r"√2 ≈ ?"],
        "query with steps" => [r"\frac{d}{dx} x^3 = ?!", r"d/dx x^3 = ?!", r"d/dx x³ = ?!"],
        "check" => [
            r"\sin^2 x + \cos^2 x \overset{?}{=} 1",
            r"sin^2 x + cos^2 x ?= 1",
            r"sin² x + cos² x ≟ 1",
        ],
        "relation chain" => [r"0 < x \le 1", r"0 < x <= 1", r"0 < x ≤ 1"],
        "where clause" => [r"A = \pi r^2, \quad r = 3", r"A = pi r^2, r = 3", r"A = πr², r = 3"],
        "range clause" => [r"a := 2, \; a \in [0, 5]", r"a := 2, a in [0, 5]", r"a ≔ 2, a ∈ [0, 5]"],
        "condition clause" => [r"f(x) = \frac{1}{x}, x > 0", r"f(x) = 1/x, x > 0"],
        "range statement" => [r"x, y \in [-2, 2]", r"x, y in [-2, 2]", r"x, y ∈ [−2, 2]"],
        "open-interval range" => [r"t \in \left(0, \infty\right)", r"t in (0, inf)", r"t ∈ (0, ∞)"],
        "name list" => [r"\theta, R(\theta)", r"theta, R(theta)", r"θ, R(θ)"],
        "noeval" => [r"x^2 + 1 %noeval"],
        "label" => [r"E = mc^2 \label{eq:energy}"],
        "aligned lines" => [
            r"\begin{aligned} f(x) &= (x+1)^2 \\ &= x^2 + 2x + 1 \end{aligned}",
            r"f(x) &= (x+1)^2 \\ &= x^2 + 2x + 1",
        ],
        "two statements" => [r"a := 1 \\ b := a + 1"],
        "trailing full stop" => [r"y = x^2.", r"y = x^2"],
    ];
    insta::assert_snapshot!(report(cases));
}

#[test]
fn operators_and_precedence() {
    let cases: &[Case] = cases![
        "not equal" => [r"a \ne b", r"a != b", r"a ≠ b"],
        "greater or equal" => [r"a \geq b", r"a >= b", r"a ≥ b"],
        "approximately" => [r"a \approx b", r"a approx b", r"a ≈ b"],
        "similar" => [r"a \sim b", r"a sim b", r"a ∼ b"],
        "proportional" => [r"a \propto b", r"a propto b", r"a ∝ b"],
        "much less" => [r"a \ll b", r"a ll b", r"a ≪ b"],
        "congruent" => [r"a \cong b", r"a cong b", r"a ≅ b"],
        "not in" => [r"x \notin A", r"x notin A", r"x ∉ A"],
        "subset" => [r"A \subseteq B \subset C", r"A subseteq B subset C", r"A ⊆ B ⊂ C"],
        "implies" => [r"p \implies q", r"p => q", r"p ⇒ q"],
        "iff" => [r"p \iff q", r"p <=> q", r"p ⇔ q"],
        "tends to" => [r"x \to 0", r"x -> 0", r"x → 0"],
        "colon map" => [r"f: \mathbb{R} \to \mathbb{R}", r"f: mathbb R -> mathbb R", r"f: ℝ → ℝ"],
        "logic" => [r"\neg p \land q \lor r", r"neg p land q lor r", r"¬p ∧ q ∨ r"],
        "additive" => [r"a + b - c \pm d \mp e", r"a + b - c pm d mp e", r"a + b − c ± d ∓ e"],
        "set operators" => [r"A \cup B \cap C \setminus D", r"A cup B cap C setminus D", r"A ∪ B ∩ C ∖ D"],
        "multiplicative" => [r"a \cdot b \times c \div d / e", r"a * b times c div d / e", r"a · b × c ÷ d / e"],
        "composition" => [r"f \circ g + A \otimes B \oplus C", r"f circ g + A otimes B oplus C", r"f ∘ g + A ⊗ B ⊕ C"],
        "implicit product" => [r"2x y", r"2xy", r"2x y"],
        "implicit product with pi" => [r"2\pi r", r"2pi r", r"2πr"],
        "unary minus" => [r"-x^2", r"-x^2", r"−x²"],
        "sign after operator" => [r"a \cdot -b + -c", r"a * -b + -c"],
        "precedence" => [r"a + b c^2 d", r"a + b c^2 d", r"a + b c² d"],
        "power is right associative" => [r"a^{b^c}", r"a^b^c"],
        "subscript expression" => [r"x_{n+1}", r"x_(n+1)", r"xₙ₊₁"],
        "subscript digit" => [r"v_0 + v_{12}", r"v_0 + v_12", r"v₀ + v₁₂"],
        "subscript label" => [r"x_{max} + T_{in}", r"x_max + T_{in}"],
        "subscript list" => [r"a_{i,j}"],
        "superscripts" => [r"x^{2} + y^{-1}", r"x^2 + y^-1", r"x² + y⁻¹"],
        "primes" => [r"f'(x) + f''(x) + g^\prime", r"f'(x) + f''(x) + g'", r"f′(x) + f″(x) + g′"],
        "factorial" => [r"n! + (2n)!! + 3!"],
    ];
    insta::assert_snapshot!(report(cases));
}

#[test]
fn atoms() {
    let cases: &[Case] = cases![
        "numbers" => [r"3.14 + 2.5e-3 + 42"],
        "constants" => [r"\pi + e + i + \infty", r"pi + e + i + inf", r"π + e + i + ∞"],
        "euler identity" => [r"e^{i\pi} + 1 = 0", r"e^(i pi) + 1 = 0"],
        "greek names" => [r"\alpha + \Omega + \varepsilon", r"alpha + Omega + varepsilon", r"α + Ω + ε"],
        "letter runs split" => [r"x y z", r"xyz"],
        "call" => [r"f(x, y)", r"f(x, y)"],
        "nested call" => [r"g(f(x))"],
        "call with subscript name" => [r"v_{0}(t)", r"v_0(t)", r"v₀(t)"],
        "groups" => [r"\left( a + b \right) c", r"(a + b) c", r"(a + b)c"],
        "square group" => [r"[a + b] c", r"\left[ a + b \right] c"],
        "absolute value" => [r"\left| x - 1 \right|", r"|x - 1|", r"abs(x - 1)"],
        "absolute value forms" => [r"\lvert x \rvert", r"\abs{x}", r"|x|"],
        "nested bars" => [r"||x| - 1|", r"\left| |x| - 1 \right|"],
        "bars side by side" => [r"|a| |b|", r"|a||b|"],
        "norm" => [r"\| v \|", r"norm(v)", r"‖v‖"],
        "sized norm" => [r"\left\| v \right\|", r"\lVert v \rVert"],
        "floor and ceiling" => [r"\lfloor x \rfloor + \lceil y \rceil", r"floor(x) + ceil(y)", r"⌊x⌋ + ⌈y⌉"],
        "fraction" => [r"\frac{a+b}{c}", r"(a+b)/c"],
        "fraction styles" => [r"\tfrac12 + \dfrac{1}{x}", r"1/2 + 1/x", r"1 ÷ 2 + 1 ÷ x"],
        "fraction of function" => [r"\frac{\sin x}{x}", r"sin(x)/x", r"sin x/x"],
        "square root" => [r"\sqrt{x+1}", r"sqrt(x+1)", r"√(x+1)"],
        "nth root" => [r"\sqrt[3]{x}", r"root(3, x)", r"∛x"],
        "binomial" => [r"\binom{n}{k}", r"binom(n, k)"],
        "set literal" => [r"\{1, 2, 3\}", r"{1, 2, 3}"],
        "empty set" => [r"\emptyset", r"emptyset", r"∅"],
        "closed interval" => [r"[0, 1]", r"\left[ 0, 1 \right]"],
        "half-open intervals" => [r"[0, 1) \cup (1, 2]", r"\left[0, 1\right) \cup \left(1, 2\right]"],
        "tuple" => [r"(1, 2, 3)", r"\left(1, 2, 3\right)"],
        "angle tuple" => [r"\langle u, v \rangle", r"langle u, v rangle", r"⟨u, v⟩"],
        "list" => [r"[1, 2, 3]"],
        "accents" => [
            r"\hat{x} + \bar{y} + \vec{v} + \tilde{z}",
            r"hat x + bar y + vec v + tilde z",
            "x\u{302} + ȳ + v\u{20D7} + z\u{303}",
        ],
        "accent over expression" => [r"\overline{z + w}"],
        "fonts" => [
            r"\mathbf{F} + \mathbb{R} + \mathcal{L} + \mathfrak{g}",
            r"mathbf F + mathbb R + mathcal L + mathfrak g",
            r"𝐅 + ℝ + ℒ + 𝔤",
        ],
        "number sets" => [r"\mathbb{N} \subset \mathbb{Z} \subset \mathbb{Q}", r"ℕ ⊂ ℤ ⊂ ℚ"],
        "upright names" => [r"\mathrm{Re} + \operatorname{Var}"],
        "text" => [r"\text{speed} = 3"],
        "ellipsis" => [r"1 + 2 + \cdots + n", r"1 + 2 + ... + n", r"1 + 2 + … + n"],
        "quantifier" => [r"\forall x \in S", r"forall x in S", r"∀x ∈ S"],
        "matrix" => [r"\begin{pmatrix} 1 & 2 \\ 3 & 4 \end{pmatrix}"],
        "bracket matrix" => [r"\begin{bmatrix} a & b \\ c & d \\ \end{bmatrix}"],
        "determinant" => [r"\begin{vmatrix} a & b \\ c & d \end{vmatrix}"],
        "plain matrix" => [r"\begin{matrix} x \\ y \end{matrix}"],
        "cases" => [
            r"f(x) := \begin{cases} x & x > 0 \\ -x & \text{otherwise} \end{cases}",
            r"f(x) := \begin{cases} x, & \text{if } x > 0 \\ -x & otherwise \end{cases}",
        ],
    ];
    insta::assert_snapshot!(report(cases));
}

#[test]
fn functions() {
    let cases: &[Case] = cases![
        "argument without brackets" => [r"\sin 2x", r"sin 2x"],
        "argument stops at next function" => [r"\sin x \cos x", r"sin x cos x"],
        "argument stops at plus" => [r"\sin x + 1", r"sin x + 1"],
        "bracketed argument" => [r"\sin(x + 1)", r"sin(x + 1)", r"\sin\left(x + 1\right)"],
        "power on function" => [r"\sin^2 x", r"sin^2 x", r"sin² x"],
        "power after brackets" => [r"\sin(x)^2", r"sin(x)^2"],
        "inverse function" => [r"\sin^{-1} x", r"sin^-1 x", r"sin⁻¹ x"],
        "log base" => [r"\log_{10} x", r"log_10 x", r"log₁₀ x"],
        "several arguments" => [r"\max(a, b)", r"max(a, b)"],
        "function of absolute value" => [r"\ln|x|", r"ln|x|"],
        "exponential" => [r"e^{x} + \exp(x)", r"e^x + exp(x)"],
        "nested functions" => [r"\sin \ln x", r"sin ln x"],
        "trig in fraction" => [
            r"\frac{g x^2}{2 v_0^2 \cos^2\theta}",
            r"(g x^2)/(2 v_0^2 cos^2 theta)",
        ],
    ];
    insta::assert_snapshot!(report(cases));
}

#[test]
fn big_operators() {
    let cases: &[Case] = cases![
        "sum" => [r"\sum_{n=1}^{\infty} \frac{1}{n^2}", r"sum_(n=1)^inf 1/n^2", r"∑_(n=1)^∞ 1/n²"],
        "sum takes the term" => [r"\sum_{k=0}^{n} k + 1", r"sum_(k=0)^n k + 1"],
        "sum over a set" => [r"\sum_{x \in S} x", r"sum_(x in S) x", r"∑_(x ∈ S) x"],
        "sum with bare index" => [r"\sum_k a_k b_k", r"sum_k a_k b_k"],
        "index i is a name in the body" => [r"\sum_{i=1}^{n} i^2", r"sum_(i=1)^n i^2", r"∑_(i=1)^n i²"],
        "nested sums" => [r"\sum_{n=0}^{N-1} \sum_{m=0}^{n} a_{nm}"],
        "product" => [r"\prod_{i=1}^{n} i", r"prod_(i=1)^n i", r"∏_(i=1)^n i"],
        "definite integral" => [r"\int_0^1 x^2 + 1 \, dx", r"int_0^1 x^2 + 1 dx", r"∫₀¹ x² + 1 dx"],
        "indefinite integral" => [r"\int \sin x \, dx", r"int sin x dx", r"∫ sin x dx"],
        "upright d" => [r"\int_0^1 f(x) \, \mathrm{d}x", r"int_0^1 f(x) dx"],
        "double integral over region" => [r"\iint_D f \, dA", r"iint_D f dA", r"∬_D f dA"],
        "region as condition" => [
            r"\iint_{x^2 + y^2 \le 1} f \, dA",
            r"iint_(x^2 + y^2 <= 1) f dA",
            r"∬_(x² + y² ≤ 1) f dA",
        ],
        "iterated integral" => [r"\int_0^1 \int_0^x xy \, dy \, dx", r"int_0^1 int_0^x x y dy dx"],
        "double integral two differentials" => [r"\iint_R f \, dx \, dy", r"∬_R f dx dy"],
        "triple integral" => [r"\iiint_V \rho \, dV", r"iiint_V rho dV", r"∭_V ρ dV"],
        "line integral" => [
            r"\oint_C \mathbf{F} \cdot d\mathbf{r}",
            r"oint_C mathbf F cdot d mathbf r",
            r"∮_C 𝐅 · d𝐫",
        ],
        "surface integral" => [r"\oiint_S \mathbf{F} \cdot d\mathbf{S}", r"∯_S 𝐅 · d𝐒"],
        "differential first" => [r"\int dx \, f(x)"],
        "differential in numerator" => [r"\int \frac{dx}{x}"],
        "limit" => [r"\lim_{x \to 0} \frac{\sin x}{x}", r"lim_(x->0) sin(x)/x", r"lim_(x→0) sin(x)/x"],
        "limit from the right" => [
            r"\lim_{x \to 0^+} \frac{1}{x}",
            r"lim_(x->0^+) 1/x",
            r"lim_(x->0+) 1/x",
            r"lim_(x→0⁺) 1/x",
        ],
        "limit from the left" => [r"\lim_{x \to 0^{-}} \frac{1}{x}", r"lim_(x->0^-) 1/x", r"lim_(x→0⁻) 1/x"],
        "limit at infinity" => [
            r"\lim_{n \to \infty} \left(1 + \frac{1}{n}\right)^n",
            r"lim_(n->inf) (1 + 1/n)^n",
            r"lim_(n→∞) (1 + 1/n)ⁿ",
        ],
        "named operator" => [r"\operatorname{crit} f = ?"],
        "named operator on interval" => [r"\operatorname{extrema}_{[0,4]} f = ?"],
        "named operator at point" => [r"\operatorname{tangent}_{x=1} f = ?"],
        "solve" => [r"\operatorname{solve}_x (x^2 = 4) = ?"],
        "series" => [r"\operatorname{series}_{x=0}^{5} e^x = ?"],
        "mod" => [r"a \equiv b \pmod{n}"],
    ];
    insta::assert_snapshot!(report(cases));
}

#[test]
fn derivatives() {
    let cases: &[Case] = cases![
        "operator" => [r"\frac{d}{dx} x^2", r"d/dx x^2", r"d/dx x²"],
        "operator on a bracket" => [r"\frac{d}{dt}\left( x^2 \right)", r"d/dt (x^2)"],
        "whole quotient" => [r"\frac{dy}{dx}", r"dy/dx"],
        "second order" => [r"\frac{d^2 y}{dx^2}", r"d^2y/dx^2", r"d²y/dx²"],
        "partial" => [
            r"\frac{\partial f}{\partial x}",
            r"partial f/partial x",
            r"∂f/∂x",
            r"\partial_x f",
        ],
        "mixed partial" => [r"\frac{\partial^2 f}{\partial x \partial y}", r"∂²f/∂x∂y"],
        "partial subscripts" => [r"\partial_{xy} f + \partial_x^2 f", r"∂_(x, y) f + ∂ₓ² f"],
        "partial operator" => [r"\frac{\partial}{\partial t} u", r"∂/∂t u"],
        "jacobian" => [r"\frac{\partial(x, y)}{\partial(u, v)}", r"∂(x, y)/∂(u, v)"],
        "dot notation" => [r"\dot{x} + \ddot{x}", r"dot x + ddot x", r"ẋ + ẍ"],
        "D operator" => [r"D_x f"],
        "D as a plain name" => [r"D_x = 3"],
        "upright d" => [r"\frac{\mathrm{d}y}{\mathrm{d}x}", r"dy/dx"],
        "gradient" => [r"\nabla f", r"nabla f", r"∇f"],
        "divergence" => [r"\nabla \cdot \mathbf{F}", r"nabla cdot mathbf F", r"∇·𝐅"],
        "curl" => [r"\nabla \times \mathbf{F}", r"nabla times mathbf F", r"∇×𝐅"],
        "laplacian" => [r"\nabla^2 u", r"nabla^2 u", r"∇²u"],
        "directional derivative" => [r"\nabla f \cdot \mathbf{u}", r"∇f · 𝐮"],
        "ode" => [r"y'' + 4y' + 4y = 0", r"y′′ + 4y′ + 4y = 0", r"y″ + 4y′ + 4y = 0"],
        "initial condition" => [r"y(0) = 1, y'(0) = 0"],
    ];
    insta::assert_snapshot!(report(cases));
}
