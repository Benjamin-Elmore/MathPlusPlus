//! The tutorial lessons, shipped inside the program. Each is a small `.mpp`
//! file that ends in a "Try it" box to change.

/// `(file name, text)` of every file in the tutorial project, in order.
pub const FILES: [(&str, &str); 12] = [
    ("01 Typing maths.mpp", L01),
    ("02 Greek letters.mpp", L02),
    ("03 The Insert menu.mpp", L03),
    ("04 Definitions.mpp", L04),
    ("05 Imports.mpp", L05),
    ("06 Auto-evaluation.mpp", L06),
    ("07 Steps.mpp", L07),
    ("08 Graphs and sliders.mpp", L08),
    ("09 Units.mpp", L09),
    ("10 Comments.mpp", L10),
    ("11 Export.mpp", L11),
    ("constants.mpp", CONSTANTS),
];

const L01: &str = "# Typing maths

Welcome. This page is a worksheet: you read it, and you type straight into it. There is no code to run and no separate preview.

Click at the end of the line below and type a few more characters. Everything you type turns into typeset maths as you go.

$$ \\frac{1}{2} + \\frac{1}{3} = ? $$

Type `^` after a letter to raise it to a power, and `/` to make a fraction. Press the arrow keys to move through a formula: the cursor steps into a power or a fraction and back out.

**Example.** Try it. Change the numbers in the equation below and watch the answer follow.

$$ 2 + 3 \\cdot 4 = ? $$
";

const L02: &str = "# Greek letters

Type the name of a letter and it becomes the letter: `alpha` gives \u{03b1}, `beta` gives \u{03b2}, `theta` gives \u{03b8}. The LaTeX spellings work too, so `\\alpha` is the same letter.

Names are just names. A Greek letter can hold a number like any other name.

Let $\\alpha := 3$.

$$ \\alpha^2 = ? $$

**Example.** Try it. Give \u{03b2} a value, then use it in an equation.

Let $\\beta := 4$.

$$ \\alpha + \\beta = ? $$
";

const L03: &str = "# The Insert menu

You do not need to remember how to write a fraction, an integral or a matrix. Put the cursor anywhere in the page and open the Insert menu: the Insert button at the top, right-click and choose Insert, or Ctrl+Space. Start typing to search (integral, matrix, alpha) and press Enter. Empty boxes appear where the parts go; press Tab to jump to the next box. Undo takes the whole template away in one step.

While a box is still empty it shows a grey word for what goes in it. Once you leave the template, anything still missing gets a red wavy line: hover over it to read what is wrong, press F8 to jump to the next problem, and Ctrl+. to pick a fix.

An integral has boxes for the limits and the thing being integrated:

$$ \\int_0^1 x^2 \\, dx = ? $$

**Example.** Try it. Change the upper limit from 1 to 2.

$$ \\int_0^2 x \\, dx = ? $$
";

const L04: &str = "# Definitions

Use `:=` to give a name a value. Once defined, the name can be used anywhere below it.

Let $m := 2$.

Let $v := 5$.

The energy of a moving mass is

$$ E = \\frac{1}{2} m v^2 $$

Hover over a name to see what it means, and Ctrl+click it to jump to where it was defined.

**Example.** Try it. Change the value of the mass above and look at the result.

$$ \\frac{1}{2} m v^2 = ? $$
";

const L05: &str = "# Imports

Maths gets reused. Put shared names in one file and bring them into another. This line imports everything from the file called constants in the same folder:

![[constants]]

Now the names from that file can be used here:

$$ g = ? $$

To import only some names, write a sentence like this one.

Using $g$ from [[constants]].

**Example.** Try it. Open the constants file from the Explorer, change the value of g, and come back to see the answer change. You can also drag a file from the Explorer onto the page to add an import.

$$ 2 g = ? $$
";

const L06: &str = "# Auto-evaluation

Results appear by themselves. Write a question mark where you want the answer and the answer is filled in. A grey result appears when the program can work something out without being asked.

$$ 12 \\cdot 12 = ? $$

$$ \\sqrt{144} = ? $$

If something cannot be worked out, the page underlines exactly the part that is wrong and says why. Fix it and the underline goes away.

**Example.** Try it. Change the numbers below.

$$ 7 \\cdot 8 - 6 = ? $$
";

const L07: &str = "# Steps

Sometimes the answer is not enough and you want to see how it was found. Open View, then Panels, then Steps to show the step-by-step tile beside the page. It shows the working for the equation you are in.

An equation to solve:

Solve $x^2 = 9$ for $x$.

Every tile in the window can be dragged, split and resized, so keep the steps next to the page while you work.

**Example.** Try it. Change the 9 to another number.

Solve $x^2 = 16$ for $x$.
";

const L08: &str = "# Graphs and sliders

A line that starts with Figure: draws a graph right in the page. The text after the full stop is its caption.

Figure: $y = x^2$, $x \\in [0, 3]$. A parabola.

Graphs use your definitions. Define a value that can change:

Let $k := 2$.

Figure: $y = k x$, $x \\in [0, 3]$. A straight line whose slope is k.

Open View, then Panels, then Variables. Every name defined with := is listed with a slider. Move the slider for k and the number in the page, the results and the graph all change together. Right-click a graph and choose Open in tile to keep it in its own tile while you scroll.

**Example.** Try it. Change k and watch the line.

$$ k \\cdot 3 = ? $$
";

const L09: &str = "# Units

Real problems have units. Write a number and its unit together and the program carries the unit through the working and checks that they agree, so adding metres to seconds is reported as a mistake rather than quietly accepted.

The units tool is installed with the full release of this program. Until then the page treats every number as a plain number, so keep the unit in your own words beside it.

A length in metres, written in words: five metres plus three metres.

$$ 5 + 3 = ? $$

**Example.** Try it. Work out the distance for a speed of 4 and a time of 6.

$$ 4 \\cdot 6 = ? $$
";

const L10: &str = "# Comments

A page can hold notes that are not part of the maths. A line that starts with a percent sign is a comment: it is kept in the file but never shown or evaluated.

Words around the maths are ordinary text. You can make words *emphasised* or **bold**, make lists, and add headings with the hash sign, like the title of this page.

- Notes are plain sentences.
- Maths goes between dollar signs inside a sentence, like $a + b$.

**Example.** Try it. Write a sentence of your own under this line, with a little maths in it.

$$ 1 + 1 = ? $$
";

const L11: &str = "# Export

When the page is finished you can share it. Open File and choose Export to turn the page into a document that others can read without the program. The typeset maths, the results and the graphs go into the export exactly as you see them.

You can also copy any part of the page. Copying maths gives Unicode maths that pastes cleanly into other programs; Edit, then Copy maths as LaTeX, gives LaTeX instead.

That is the end of the tutorial. Everything here can be reopened from Help, then Tutorial, at any time.

**Example.** Try it. Make a sum of your own.

$$ 10 \\cdot 10 = ? $$
";

const CONSTANTS: &str = "# Constants

Shared names for the lessons.

Let $g := 9.81$.
";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_lesson_ends_with_a_try_it_box() {
        for (name, text) in FILES {
            if name == "constants.mpp" {
                continue;
            }
            assert!(
                text.contains("**Example.** Try it"),
                "{name} has no try-it box"
            );
        }
        let names: Vec<&str> = FILES.iter().map(|(n, _)| *n).collect();
        assert!(names.contains(&"constants.mpp"));
    }
}
