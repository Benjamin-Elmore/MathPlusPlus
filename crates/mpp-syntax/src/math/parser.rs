//! Recursive-descent maths parser with the precedence table of grammar §2.2.
//!
//! The parser never panics and always returns a tree: where something is
//! missing it reports a diagnostic at the exact span and puts an
//! [`NodeKind::Error`] node in its place, then carries on at the next `,`, `\\`
//! or the end of the span.

mod atoms;
mod calculus;
mod expr;
mod statement;

pub use statement::split_pmod;

use crate::lexer::{lex, Delim, Tok, Token};
use crate::math::ast::{Clause, MathAst, Node, NodeKind};
use crate::span::{Diagnostic, Span};

/// Deepest nesting the parser follows before reporting an error: 256 levels of
/// brackets (each level costs two, plus a few for the line around them). A level takes about 10 KB of stack in a debug
/// build (3 KB in release), so deep input is parsed on a thread of its own with
/// [`DEEP_STACK`] bytes of stack (see [`on_parse_stack`]).
const MAX_DEPTH: usize = 2 * 256 + 8;

/// Longest run of operators at one level (`a + b + …`) before reporting an error.
/// A run is built as a left-leaning tree as deep as the run is long; the programs
/// that read the trees do their deep work on large stacks (the evaluator's thread,
/// the apps' main threads).
const MAX_CHAIN: usize = 10_000;

/// Stack for parsing deep input: room for [`MAX_DEPTH`] levels with a wide margin.
const DEEP_STACK: usize = 64 << 20;

/// The nesting limit used when a parse of deep input cannot get a large stack.
const SAFE_DEPTH: usize = 64;

/// Nesting and length from which a span is parsed on its own large stack; below them
/// the caller's stack is ample, even for a debug build on a 1 MB thread.
const SHALLOW_NESTING: usize = 12;
const SHALLOW_TOKENS: usize = 1_500;

/// True when parsing `tokens` may need more stack than a small thread has.
fn is_deep(tokens: &[Token]) -> bool {
    if tokens.len() > SHALLOW_TOKENS {
        return true;
    }
    let mut depth = 0usize;
    let mut max = 0usize;
    for t in tokens {
        match t.kind {
            Tok::LBrace | Tok::Open { .. } | Tok::Begin(_) => {
                depth += 1;
                max = max.max(depth);
            }
            Tok::RBrace | Tok::Close { .. } | Tok::End(_) => depth = depth.saturating_sub(1),
            _ => {}
        }
    }
    max > SHALLOW_NESTING
}

/// Runs a parse (`f`, given its nesting limit) on a thread with [`DEEP_STACK`] bytes
/// of stack when the input is `deep`, so parsing never overflows the caller's stack.
/// When no thread can be started, the parse runs here with a nesting limit any stack
/// holds.
fn on_parse_stack<T: Send>(deep: bool, f: impl FnOnce(usize) -> T + Send) -> T {
    if !deep {
        return f(MAX_DEPTH);
    }
    let mut f = Some(f);
    let outcome = std::thread::scope(|scope| {
        let job = &mut f;
        let spawned = std::thread::Builder::new()
            .name("mpp-parse".into())
            .stack_size(DEEP_STACK)
            .spawn_scoped(scope, move || job.take().map(|g| g(MAX_DEPTH)));
        match spawned {
            Ok(handle) => match handle.join() {
                Ok(v) => v,
                Err(panic) => std::panic::resume_unwind(panic),
            },
            Err(_) => None,
        }
    });
    match (outcome, f) {
        (Some(v), _) => v,
        // No thread to be had: a nesting limit that fits any stack instead.
        (None, Some(g)) => g(SAFE_DEPTH),
        // The thread took the parse and ran it, so it returned a result.
        (None, None) => unreachable!("a parse thread that ran returns its result"),
    }
}

/// Parses a whole maths span (the text between `$ … $` or `$$ … $$`).
///
/// `base_offset` is where `src` starts in the file, so every span indexes the file.
pub fn parse_math(src: &str, base_offset: usize) -> (MathAst, Vec<Diagnostic>) {
    let (tokens, lex_diags) = lex(src, base_offset);
    on_parse_stack(is_deep(&tokens), move |limit| {
        let mut parser = Parser::new(src, base_offset, tokens).with_max_depth(limit);
        let ast = parser.parse_ast();
        (ast, parser.finish(lex_diags))
    })
}

/// Parses a single expression or relation chain, such as the `$…$` in a sentence form.
pub fn parse_expr(src: &str, base_offset: usize) -> (Node, Vec<Diagnostic>) {
    let (tokens, lex_diags) = lex(src, base_offset);
    on_parse_stack(is_deep(&tokens), move |limit| {
        let mut parser = Parser::new(src, base_offset, tokens).with_max_depth(limit);
        let node = parser.parse_relation();
        parser.expect_end("the end of the expression");
        (node, parser.finish(lex_diags))
    })
}

/// Parses `a, f, R(\theta)`: the names of a `Using … from` sentence.
pub fn parse_name_list(src: &str, base_offset: usize) -> (Vec<Node>, Vec<Diagnostic>) {
    let (tokens, lex_diags) = lex(src, base_offset);
    on_parse_stack(is_deep(&tokens), move |limit| {
        let mut parser = Parser::new(src, base_offset, tokens).with_max_depth(limit);
        let names = parser.parse_names();
        parser.expect_end("`,` or the end of the list");
        (names, parser.finish(lex_diags))
    })
}

/// Parses an interval such as `[0, 4]` or `(0, \infty)`; a round pair is an open interval.
pub fn parse_interval(src: &str, base_offset: usize) -> (Node, Vec<Diagnostic>) {
    let (tokens, lex_diags) = lex(src, base_offset);
    on_parse_stack(is_deep(&tokens), move |limit| {
        let mut parser = Parser::new(src, base_offset, tokens).with_max_depth(limit);
        let node = parser.parse_expr();
        let node = expr::as_interval(node);
        if !matches!(node.kind, NodeKind::Interval { .. } | NodeKind::Error) {
            parser.diags.push(
                Diagnostic::error(node.span, "expected an interval such as `[0, 1]`")
                    .expecting(["`[a, b]`", "`(a, b)`"]),
            );
        }
        parser.expect_end("the end of the interval");
        (node, parser.finish(lex_diags))
    })
}

/// Parses one clause: a where-binding (`r = 3`), a range (`x \in [0, 1]`) or a condition.
pub fn parse_clause(src: &str, base_offset: usize) -> (Clause, Vec<Diagnostic>) {
    let (tokens, lex_diags) = lex(src, base_offset);
    on_parse_stack(is_deep(&tokens), move |limit| {
        let mut parser = Parser::new(src, base_offset, tokens).with_max_depth(limit);
        let clause = parser.parse_clause();
        parser.expect_end("the end of the clause");
        (clause, parser.finish(lex_diags))
    })
}

/// Context flags that brackets reset.
#[derive(Debug, Clone, Copy, Default)]
struct Ctx {
    /// Inside an integral body: a differential `d x` ends the body.
    in_integral: bool,
    /// Inside a bracket-less function argument: stop at the next function name
    /// and at explicit operators.
    func_arg: bool,
    /// Inside `| … |`: a `|` where an operator is expected closes it.
    bars: bool,
    /// Inside `‖ … ‖`.
    norms: bool,
}

/// Saved parser state for backtracking.
#[derive(Clone, Copy)]
struct Checkpoint {
    pos: usize,
    diags: usize,
    ctx: Ctx,
    reported: (bool, bool),
}

/// Parser state over one token list.
pub(crate) struct Parser<'a> {
    src: &'a str,
    base: usize,
    toks: Vec<Token>,
    pos: usize,
    /// Tokens at or after this index read as the end (used for `x \to 0^+`).
    stop: usize,
    diags: Vec<Diagnostic>,
    depth: usize,
    /// Deepest nesting followed ([`MAX_DEPTH`] unless the stack is small).
    max_depth: usize,
    too_deep_reported: bool,
    too_long_reported: bool,
    ctx: Ctx,
    /// The line just read stopped where another statement starts on the same line.
    continues: bool,
    /// The domain of `… \text{ for all } t \ge 0`, read inside a statement; it becomes
    /// a condition of the line.
    quantified: Vec<crate::math::ast::Clause>,
}

impl<'a> Parser<'a> {
    fn new(src: &'a str, base: usize, toks: Vec<Token>) -> Self {
        let stop = toks.len();
        Parser {
            src,
            base,
            toks,
            pos: 0,
            stop,
            diags: Vec::new(),
            depth: 0,
            max_depth: MAX_DEPTH,
            too_deep_reported: false,
            continues: false,
            quantified: Vec::new(),
            too_long_reported: false,
            ctx: Ctx::default(),
        }
    }

    /// Merges lexer and parser diagnostics in source order.
    fn finish(self, mut lex_diags: Vec<Diagnostic>) -> Vec<Diagnostic> {
        lex_diags.extend(self.diags);
        lex_diags.sort_by_key(|d| (d.span.start, d.span.end));
        lex_diags
    }

    // ----- token access -----

    fn peek(&self) -> Option<&Tok> {
        self.peek_n(0)
    }

    fn peek_n(&self, n: usize) -> Option<&Tok> {
        let i = self.pos + n;
        if i < self.stop {
            self.toks.get(i).map(|t| &t.kind)
        } else {
            None
        }
    }

    fn at(&self, f: impl Fn(&Tok) -> bool) -> bool {
        self.peek().is_some_and(f)
    }

    fn at_end(&self) -> bool {
        self.peek().is_none()
    }

    fn bump(&mut self) -> Option<Token> {
        if self.pos < self.stop {
            let tok = self.toks.get(self.pos).cloned();
            self.pos += 1;
            tok
        } else {
            None
        }
    }

    /// Consumes the next token when it matches.
    fn eat(&mut self, f: impl Fn(&Tok) -> bool) -> Option<Token> {
        if self.at(f) {
            self.bump()
        } else {
            None
        }
    }

    /// Span of the next token, or an empty span just after the last one.
    fn cur_span(&self) -> Span {
        if self.pos < self.stop {
            if let Some(t) = self.toks.get(self.pos) {
                return t.span;
            }
        }
        Span::point(self.prev_end())
    }

    /// End of the previous token (or the start of the source).
    fn prev_end(&self) -> usize {
        self.pos
            .checked_sub(1)
            .and_then(|i| self.toks.get(i))
            .map_or(self.base, |t| t.span.end)
    }

    fn prev_span(&self) -> Span {
        self.pos
            .checked_sub(1)
            .and_then(|i| self.toks.get(i))
            .map_or(Span::point(self.base), |t| t.span)
    }

    /// The source text under a span.
    fn text(&self, span: Span) -> &'a str {
        let start = span.start.saturating_sub(self.base);
        let end = span.end.saturating_sub(self.base);
        self.src.get(start..end).unwrap_or("")
    }

    /// Short description of the previous token for messages (`+`, `\frac`).
    fn prev_text(&self) -> Option<String> {
        if self.pos == 0 {
            return None;
        }
        let t = self.text(self.prev_span());
        Some(t.chars().take(24).collect())
    }

    fn checkpoint(&self) -> Checkpoint {
        Checkpoint {
            pos: self.pos,
            diags: self.diags.len(),
            ctx: self.ctx,
            reported: (self.too_deep_reported, self.too_long_reported),
        }
    }

    fn restore(&mut self, cp: Checkpoint) {
        self.pos = cp.pos;
        self.diags.truncate(cp.diags);
        self.ctx = cp.ctx;
        (self.too_deep_reported, self.too_long_reported) = cp.reported;
    }

    // ----- diagnostics -----

    fn error(&mut self, span: Span, message: impl Into<String>, expected: &[&str]) {
        self.diags.push(
            Diagnostic::error(span, message).expecting(expected.iter().map(|s| s.to_string())),
        );
    }

    /// Reports `expected {what}` at the next token and returns an error node there.
    fn missing(&mut self, what: &str, expected: &[&str]) -> Node {
        let span = self.cur_span();
        let message = match self.prev_text() {
            Some(prev)
                if !prev.is_empty()
                    && !what.contains(" after ")
                    && !what.contains(&format!("`{prev}`")) =>
            {
                format!("expected {what} after `{prev}`")
            }
            _ => format!("expected {what}"),
        };
        self.error(span, message, expected);
        Node::error(Span::point(span.start))
    }

    /// Reports leftovers when a helper expects the input to be finished.
    fn expect_end(&mut self, what: &str) {
        if let Some(tok) = self.bump() {
            let last = self.stop.checked_sub(1).and_then(|i| self.toks.get(i));
            let span = tok
                .span
                .join(last.map_or(tok.span, |t| Span::point(t.span.end)));
            let found = self.text(tok.span).to_string();
            self.error(span, format!("unexpected `{found}`"), &[what]);
            self.pos = self.stop;
        }
    }

    // ----- nesting guard -----

    /// The same parser with nesting limited to `max_depth`.
    fn with_max_depth(mut self, max_depth: usize) -> Self {
        self.max_depth = max_depth.min(MAX_DEPTH);
        self
    }

    fn enter(&mut self) -> bool {
        if self.depth >= self.max_depth {
            return false;
        }
        self.depth += 1;
        true
    }

    fn leave(&mut self) {
        self.depth = self.depth.saturating_sub(1);
    }

    /// Error node used when nesting is too deep; skips the rest of the line.
    fn too_deep(&mut self) -> Node {
        let span = self.cur_span();
        if !self.too_deep_reported {
            self.too_deep_reported = true;
            self.error(span, "the expression is nested too deeply", &[]);
        }
        self.skip_line();
        Node::error(span)
    }

    /// False (after reporting and skipping the rest of the line) once a chain of
    /// `count` operators at one level is too long.
    fn chain_ok(&mut self, count: usize) -> bool {
        if count < MAX_CHAIN {
            return true;
        }
        if !self.too_long_reported {
            self.too_long_reported = true;
            let span = self.cur_span();
            self.error(
                span,
                "this expression is too long; split it over several lines",
                &[r"`\\`"],
            );
        }
        self.skip_line();
        false
    }

    /// Skips to the next `\\` (or the end).
    fn skip_line(&mut self) {
        while self.peek().is_some_and(|t| !matches!(t, Tok::NewLine)) {
            self.bump();
        }
    }

    /// Runs `f` with fresh context flags (inside brackets), restoring them afterwards.
    fn nested<T>(&mut self, f: impl FnOnce(&mut Self) -> T) -> T {
        let saved = self.ctx;
        self.ctx = Ctx::default();
        let out = f(self);
        self.ctx = saved;
        out
    }

    /// Runs `f` with the given flags, restoring the old ones afterwards.
    fn with_ctx<T>(&mut self, ctx: Ctx, f: impl FnOnce(&mut Self) -> T) -> T {
        let saved = self.ctx;
        self.ctx = ctx;
        let out = f(self);
        self.ctx = saved;
        out
    }

    /// Parses a Unicode super- or subscript run (`²`, `⁻¹`, `ₙ₊₁`) as an expression.
    /// Every node in the result gets the span of the whole run.
    fn fragment(&mut self, text: &str, span: Span) -> Node {
        let (tokens, lex_diags) = lex(text, 0);
        let mut sub = Parser::new(text, 0, tokens);
        let node = sub.parse_relation();
        sub.expect_end("the end of the script");
        for mut d in sub.finish(lex_diags) {
            d.span = span;
            self.diags.push(d);
        }
        respan(node, span)
    }

    /// Index of the token that closes the bracket opened at `open` (or the end).
    fn matching_close(&self, open: usize) -> usize {
        self.matching_close_within(open, usize::MAX)
            .unwrap_or(self.stop)
    }

    /// Like [`Parser::matching_close`], but gives up (`None`) after `limit` tokens, for
    /// look-aheads that only care about short groups.
    fn matching_close_within(&self, open: usize, limit: usize) -> Option<usize> {
        let mut depth = 0usize;
        let end = self.stop.min(open.saturating_add(limit));
        for (i, t) in self.toks.iter().enumerate().take(end).skip(open) {
            match t.kind {
                Tok::LBrace | Tok::Open { .. } | Tok::Begin(_) => depth += 1,
                Tok::RBrace | Tok::Close { .. } | Tok::End(_) => {
                    depth = depth.saturating_sub(1);
                    if depth == 0 {
                        return Some(i);
                    }
                }
                _ => {}
            }
        }
        (end == self.stop).then_some(self.stop)
    }
}

/// Tokens that end an expression and must be left for an outer rule.
fn is_terminator(t: &Tok) -> bool {
    matches!(
        t,
        Tok::Close { .. }
            | Tok::RBrace
            | Tok::Comma
            | Tok::Semicolon
            | Tok::NewLine
            | Tok::Amp
            | Tok::Rel(_)
            | Tok::End(_)
            | Tok::Assign
            | Tok::CheckEq
            | Tok::CheckEquiv
            | Tok::Period
    )
}

/// True for a closing bracket of any kind.
fn is_close(t: &Tok) -> bool {
    matches!(t, Tok::Close { .. })
}

/// True when `t` closes `delim` (any close token closes a sized or plain bracket).
fn closes(t: &Tok, delim: Delim) -> bool {
    matches!(t, Tok::Close { delim: d, .. } if *d == delim)
}

/// Gives every node in a tree the same span.
fn respan(mut node: Node, span: Span) -> Node {
    fn walk(node: &mut Node, span: Span) {
        node.span = span;
        crate::math::visit::children_mut(node, &mut |child| walk(child, span));
    }
    walk(&mut node, span);
    node
}
