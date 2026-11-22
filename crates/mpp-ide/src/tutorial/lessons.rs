//! The tutorial lessons, shipped inside the program: five short pages, each a
//! small `.mpp` file that ends in a "Try it" box to change, plus the shared
//! `constants.mpp` that page 5 imports.

/// `(file name, text)` of every file in the tutorial project, in order. The
/// first run opens the first one.
pub const FILES: [(&str, &str); 6] = [
    ("1 Prose and maths.mpp", P1),
    ("2 Typing maths.mpp", P2),
    ("3 Definitions and answers.mpp", P3),
    ("4 Graphs.mpp", P4),
    ("5 Imports and the side column.mpp", P5),
    ("constants.mpp", CONSTANTS),
];

const P1: &str = r#"# 1. Prose and maths

Every line on this page is one of two kinds. Words are **prose**: plain page, a black caret. A formula is a **maths line**: it sits in its own tinted box with a ∑ mark in the left margin, and the caret changes colour inside it.

The line below is maths. Click in it and change a number.

$$ 2 + 3 \cdot 4 = ? $$

**Switching.** Any of these turns a line into maths, and any of them turns it back:
- Type `$` on an empty line (LaTeX users).
- Type `=` or `:=` on an empty line.
- Press Alt+= (as in Word) or Ctrl+M.
- Click the ∑ mark in the margin, or the word Prose or Maths in the status bar.

**Auto-detect.** When you finish a line that is clearly maths, such as `x := 5` or `1.5 + 2 =`, it turns into a maths line by itself and a hint tells you how to undo it. Two or more ordinary words keep a line as prose.

**Esc** leaves maths and puts the caret back in prose.

**Example.** Try it. Write `y := 7` on the empty line below and press Enter.

"#;

const P2: &str = r#"# 2. Typing maths

You can type a formula the LaTeX way or the code way. Both give the same typeset result, and both convert as you type, only when you press a space, an operator or a closing bracket. The first Ctrl+Z gives back exactly what you typed.

| You know LaTeX | You write code | You get |
| --- | --- | --- |
| `\frac{a}{b}` | `a/b` | a fraction |
| `x^2` | `x**2` or `x^2` | a power |
| `\sqrt{x}` | `sqrt(x)` | a root |
| `\le`, `\ge`, `\ne` | `<=`, `>=`, `!=` | ≤, ≥, ≠ |
| `\alpha` | `alpha` | α |

$$ \frac{1}{2} + \frac{1}{3} = ? $$

**Getting out of a box.** Tab goes to the next box and then out. Right arrow at the end of a box leaves one level. A space or an operator after an exponent leaves it. Esc leaves the formula. Enter starts a new maths line.

Type `\` in maths for a list of every symbol and template. Ctrl+Space opens the full Insert menu.

**Example.** Try it. Change the line below: type `sqrt(` and a number, or `\sqrt`.

$$ \sqrt{144} = ? $$
"#;

const P3: &str = r#"# 3. Definitions and answers

**Define** a name with `:=`. After that the name works anywhere below. On a maths line, `x = 5` also defines `x` when it is not defined yet, and a small mark shows it was read that way.

$$ x := 5 $$

$$ E := 2 x + 5 $$

**Answers** come by themselves, in a slot at the end of the line that is kept free, so nothing moves when one arrives:
- A line that ends with `=` is worked out: `x + 1 =`.
- `= ?` asks for an exact answer, and `≈ ?` asks for a decimal.
- A definition shows its value in grey when it is a number: `E` shows 15.

$$ x + 1 = $$

$$ \sqrt{2} \approx ? $$

When something cannot be worked out, the slot shows a red ⚠ and a short message, and a hover tells you more. If `x` is defined and you write `x = 5` again, it is a check: ✓ or ✗.

**Example.** Try it. Change the 5 in the first line to another number and watch every answer follow.

$$ 2 x = ? $$
"#;

const P4: &str = r#"# 4. Graphs

A line that starts with Figure: draws a graph right in the page. The text after the full stop is its caption.

Figure: $y = x^2$, $x \in [0, 3]$. A parabola.

Graphs use your definitions. Define a value that can change:

$$ k := 2 $$

Figure: $y = k x$, $x \in [0, 3]$. A straight line whose slope is k.

Open the Definitions icon in the side column: every name defined with := is listed, and its value can be changed there. Right-click a graph and choose Open in tile to keep it in its own tile while you scroll.

**Example.** Try it. Change k above and watch the line.

$$ k \cdot 3 = ? $$
"#;

const P5: &str = r#"# 5. Imports and the side column

**Imports.** Put shared names in one file and bring them into another. This line imports everything from the file called constants in the same folder:

![[constants]]

Now the names from that file can be used here:

$$ g = ? $$

To import only some names, write a sentence like this one.

Using $g$ from [[constants]].

You can also drag a file from the Explorer onto the page to add an import.

**The side column.** The icons at the far left each show one panel, and clicking the active icon hides the column (Ctrl+B does the same):
- Explorer: your files and folders.
- Search: find words or symbols in all files.
- Outline: the headings and blocks of this file.
- Definitions: the names defined here, with their values.
- Problems: everything to fix.
- Tutorial: these pages.

Any panel can be dragged into the main area as a tile. Help, then Hints, lists the one-time hints and can show them again.

**Example.** Try it. Open the constants file from the Explorer, change the value of g, and come back to see the answer change.

$$ 2 g = ? $$
"#;

const CONSTANTS: &str = r#"# Constants

Shared names for the lessons.

Let $g := 9.81$.
"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn five_pages_then_the_shared_constants() {
        assert_eq!(FILES.len(), 6);
        for (i, (name, text)) in FILES.iter().take(5).enumerate() {
            assert!(name.starts_with(&format!("{} ", i + 1)), "{name}");
            assert!(
                text.contains("**Example.** Try it"),
                "{name} has no try-it box"
            );
        }
        assert_eq!(FILES[5].0, "constants.mpp");
    }
}
