//! The document a first launch opens, so the page is never blank.

/// A short worked page: prose, definitions, queries and a figure.
pub const WELCOME: &str = "\
# Welcome to Math++

Click anywhere on the page and type. Results fill in as you work, and Greek letters appear as you type their names: try *alpha*.

## A first problem

A ball of mass $m$ moves at speed $v$. Define the quantities

$$ m := 2 $$

$$ v := 3 $$

and its kinetic energy follows from

$$ E := \\frac{1}{2} m v^2 $$

$$ E = ? $$

## Powers and fractions

Large powers are exact:

$$ 2^{10} = ? $$

and fractions combine without rounding:

$$ \\frac{3}{4} + \\frac{5}{6} = ? $$

Figure: $y = x^2$, $x \\in [-2, 2]$. The parabola.
";
