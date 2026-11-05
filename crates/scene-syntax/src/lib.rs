//! Lexer, parser, AST and diagnostics for iso-scene.
//!
//! The normative grammar lives in `docs/grammar.ebnf`. This crate must never
//! panic on malformed input: every failure is a [`Diagnostic`].

pub mod ast;
pub mod lexer;
pub mod parser;

pub use lexer::{Token, TokenKind, lex};
pub use parser::parse;

/// Version of the language specification implemented by this crate.
pub const SPEC_VERSION: u32 = 1;

/// Byte range in a source file.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Span {
    pub start: u32,
    pub end: u32,
}

impl Span {
    #[must_use]
    pub const fn new(start: u32, end: u32) -> Self {
        Self { start, end }
    }

    #[must_use]
    pub const fn len(self) -> u32 {
        self.end.saturating_sub(self.start)
    }

    #[must_use]
    pub const fn is_empty(self) -> bool {
        self.start == self.end
    }
}

/// A user-facing error with a stable code and a source span.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Diagnostic {
    pub code: &'static str,
    pub message: String,
    pub span: Span,
}

impl Diagnostic {
    #[must_use]
    pub fn new(code: &'static str, message: impl Into<String>, span: Span) -> Self {
        Self {
            code,
            message: message.into(),
            span,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{SPEC_VERSION, Span};

    #[test]
    fn spec_version_is_one() {
        assert_eq!(SPEC_VERSION, 1);
    }

    #[test]
    fn span_length_is_saturating() {
        assert_eq!(Span::new(4, 10).len(), 6);
        assert_eq!(Span::new(10, 4).len(), 0);
        assert!(Span::new(3, 3).is_empty());
    }
}
