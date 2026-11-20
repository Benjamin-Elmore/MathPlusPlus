//! A small infix reader for the tools' unit tests, so textbook problems read as written:
//! `p("x^3 - 3*x")`, `p("sqrt(4 - x^2)")`, `p("ln(x)/x")`. Implicit products are not
//! supported; write `*`.

use crate::expr::Expr;
use crate::functions::{func, Func};

/// Parses `src`; panics on malformed test input.
pub fn p(src: &str) -> Expr {
    let toks = lex(src);
    let mut r = Reader { toks, i: 0 };
    let e = r.sum();
    assert!(r.i == r.toks.len(), "trailing input in {src:?}");
    e
}

#[derive(Clone, Debug, PartialEq)]
enum Tok {
    Num(String),
    Name(String),
    Op(char),
}

fn lex(src: &str) -> Vec<Tok> {
    let chars: Vec<char> = src.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c.is_whitespace() {
            i += 1;
        } else if c.is_ascii_digit() || c == '.' {
            let start = i;
            while i < chars.len() && (chars[i].is_ascii_digit() || chars[i] == '.') {
                i += 1;
            }
            out.push(Tok::Num(chars[start..i].iter().collect()));
        } else if c.is_alphabetic() || c == '_' {
            let start = i;
            while i < chars.len() && (chars[i].is_alphanumeric() || chars[i] == '_') {
                i += 1;
            }
            out.push(Tok::Name(chars[start..i].iter().collect()));
        } else {
            out.push(Tok::Op(c));
            i += 1;
        }
    }
    out
}

struct Reader {
    toks: Vec<Tok>,
    i: usize,
}

impl Reader {
    fn peek(&self) -> Option<&Tok> {
        self.toks.get(self.i)
    }

    fn eat(&mut self, c: char) -> bool {
        if self.peek() == Some(&Tok::Op(c)) {
            self.i += 1;
            true
        } else {
            false
        }
    }

    fn sum(&mut self) -> Expr {
        let mut acc = self.product();
        loop {
            if self.eat('+') {
                acc = acc + self.product();
            } else if self.eat('-') {
                acc = acc - self.product();
            } else {
                return acc;
            }
        }
    }

    fn product(&mut self) -> Expr {
        let mut acc = self.unary();
        loop {
            if self.eat('*') {
                acc = acc * self.unary();
            } else if self.eat('/') {
                acc = acc / self.unary();
            } else {
                return acc;
            }
        }
    }

    fn unary(&mut self) -> Expr {
        if self.eat('-') {
            return -self.unary();
        }
        self.power()
    }

    fn power(&mut self) -> Expr {
        let base = self.atom();
        if self.eat('^') {
            let exp = self.unary();
            return base.pow(exp);
        }
        base
    }

    fn atom(&mut self) -> Expr {
        match self.toks.get(self.i).cloned() {
            Some(Tok::Num(n)) => {
                self.i += 1;
                if let Some((a, b)) = n.split_once('.') {
                    let den = 10_i64.pow(u32::try_from(b.len()).unwrap_or(0));
                    let num: i64 = format!("{a}{b}").parse().unwrap_or(0);
                    Expr::rational(num, den)
                } else {
                    Expr::int(n.parse().unwrap_or(0))
                }
            }
            Some(Tok::Name(name)) => {
                self.i += 1;
                if self.eat('(') {
                    let mut args = vec![self.sum()];
                    while self.eat(',') {
                        args.push(self.sum());
                    }
                    assert!(self.eat(')'), "missing ) after {name}(");
                    return call(&name, args);
                }
                match name.as_str() {
                    "pi" => Expr::pi(),
                    "e" => Expr::e(),
                    "oo" => Expr::infinity(),
                    _ => Expr::sym(&name),
                }
            }
            Some(Tok::Op('(')) => {
                self.i += 1;
                let e = self.sum();
                assert!(self.eat(')'), "missing )");
                e
            }
            other => panic!("unexpected {other:?}"),
        }
    }
}

fn call(name: &str, mut args: Vec<Expr>) -> Expr {
    match name {
        "sqrt" => args.remove(0).sqrt(),
        "exp" => Expr::e().pow(args.remove(0)),
        _ => match Func::from_name(name) {
            Some(f) => func(f, args),
            None => Expr::apply(name, args),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_textbook_input() {
        assert_eq!(p("x^3 - 3*x").to_string(), "x^3 - 3*x");
        assert_eq!(p("sqrt(4 - x^2)").to_string(), "sqrt(-x^2 + 4)");
        assert_eq!(p("ln(x)/x").to_string(), "ln(x)/x");
        assert_eq!(p("-x^2").to_string(), "-x^2");
        assert_eq!(p("0.5*x").to_string(), "x/2");
    }
}
