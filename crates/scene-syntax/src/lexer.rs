//! Lexer for iso-scene.
//!
//! The lexer works on bytes. All syntax characters are ASCII; non-ASCII is
//! allowed only inside string literals and comments. Every failure is a
//! [`Diagnostic`] with a byte span, never a panic.

use crate::{Diagnostic, Span};

/// Maximum source size for a single file (1 MiB).
pub const MAX_SOURCE_BYTES: usize = 1024 * 1024;
/// Maximum number of tokens in a single file.
pub const MAX_TOKENS: usize = 1_000_000;

/// A lexical token.
#[derive(Clone, Debug, PartialEq)]
pub struct Token {
    pub kind: TokenKind,
    pub span: Span,
}

/// Token kinds produced by the lexer.
#[derive(Clone, Debug, PartialEq)]
pub enum TokenKind {
    Ident(String),
    Number(f64),
    String(String),
    Color([u8; 3]),

    // Directives and declarations
    Version,
    Include,
    Material,
    Object,
    Render,
    Scale,
    Canvas,
    Sun,
    Ambient,
    Background,
    Place,
    Mirror,
    For,
    In,
    If,
    Else,
    Let,
    Var,

    // Literals
    True,
    False,

    // Shapes and their arguments
    Box,
    Cyl,
    Sphere,
    SlantBox,
    SlantCyl,
    Ramp,
    At,
    Size,
    Radius,
    Height,
    Offset,
    Rise,
    Rot,

    // Punctuation
    LBrace,
    RBrace,
    LParen,
    RParen,
    Semicolon,
    Comma,
    Assign,
    Plus,
    Minus,
    Star,
    Slash,
    Percent,
    EqEq,
    NotEq,
    Lt,
    Le,
    Gt,
    Ge,
    AndAnd,
    OrOr,
    Bang,
    DotDot,

    Eof,
}

struct Lexer<'a> {
    bytes: &'a [u8],
    pos: usize,
    tokens: usize,
}

impl<'a> Lexer<'a> {
    fn new(source: &'a str) -> Self {
        let bytes = source.as_bytes();
        let pos = usize::from(bytes.starts_with(&[0xEF, 0xBB, 0xBF])) * 3;
        Self {
            bytes,
            pos,
            tokens: 0,
        }
    }

    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.pos).copied()
    }

    fn peek_at(&self, offset: usize) -> Option<u8> {
        self.bytes.get(self.pos + offset).copied()
    }

    fn bump(&mut self) -> Option<u8> {
        let byte = self.peek()?;
        self.pos += 1;
        Some(byte)
    }

    fn error(&self, code: &'static str, message: impl Into<String>, start: usize) -> Diagnostic {
        Diagnostic::new(code, message, Span::new(to_u32(start), to_u32(self.pos)))
    }

    fn skip_trivia(&mut self) -> Result<(), Diagnostic> {
        loop {
            match self.peek() {
                Some(b' ' | b'\t' | b'\r' | b'\n') => {
                    self.pos += 1;
                }
                Some(b'/') if self.peek_at(1) == Some(b'/') => {
                    self.pos += 2;
                    while let Some(byte) = self.peek() {
                        self.pos += 1;
                        if byte == b'\n' {
                            break;
                        }
                    }
                }
                Some(b'/') if self.peek_at(1) == Some(b'*') => {
                    let start = self.pos;
                    self.pos += 2;
                    loop {
                        match (self.peek(), self.peek_at(1)) {
                            (Some(b'*'), Some(b'/')) => {
                                self.pos += 2;
                                break;
                            }
                            (Some(_), _) => self.pos += 1,
                            (None, _) => {
                                self.pos = self.bytes.len();
                                return Err(self.error(
                                    "E0101",
                                    "unterminated block comment",
                                    start,
                                ));
                            }
                        }
                    }
                }
                _ => return Ok(()),
            }
        }
    }

    fn token(&mut self) -> Result<Token, Diagnostic> {
        self.skip_trivia()?;
        let start = self.pos;
        let Some(byte) = self.peek() else {
            return Ok(Token {
                kind: TokenKind::Eof,
                span: Span::new(to_u32(start), to_u32(start)),
            });
        };

        let kind = match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'_' => self.identifier(),
            b'0'..=b'9' => self.number()?,
            b'"' => self.string()?,
            b'#' => self.color()?,
            b'{' => self.single(TokenKind::LBrace),
            b'}' => self.single(TokenKind::RBrace),
            b'(' => self.single(TokenKind::LParen),
            b')' => self.single(TokenKind::RParen),
            b';' => self.single(TokenKind::Semicolon),
            b',' => self.single(TokenKind::Comma),
            b'+' => self.single(TokenKind::Plus),
            b'-' => self.single(TokenKind::Minus),
            b'*' => self.single(TokenKind::Star),
            b'%' => self.single(TokenKind::Percent),
            b'/' => self.single(TokenKind::Slash),
            b'=' if self.peek_at(1) == Some(b'=') => {
                self.pos += 2;
                TokenKind::EqEq
            }
            b'=' => self.single(TokenKind::Assign),
            b'!' if self.peek_at(1) == Some(b'=') => {
                self.pos += 2;
                TokenKind::NotEq
            }
            b'!' => self.single(TokenKind::Bang),
            b'<' if self.peek_at(1) == Some(b'=') => {
                self.pos += 2;
                TokenKind::Le
            }
            b'<' => self.single(TokenKind::Lt),
            b'>' if self.peek_at(1) == Some(b'=') => {
                self.pos += 2;
                TokenKind::Ge
            }
            b'>' => self.single(TokenKind::Gt),
            b'&' if self.peek_at(1) == Some(b'&') => {
                self.pos += 2;
                TokenKind::AndAnd
            }
            b'|' if self.peek_at(1) == Some(b'|') => {
                self.pos += 2;
                TokenKind::OrOr
            }
            b'.' if self.peek_at(1) == Some(b'.') => {
                self.pos += 2;
                TokenKind::DotDot
            }
            _ => {
                let len = Self::char_len(byte);
                self.pos += len;
                return Err(self.error(
                    "E0102",
                    format!("unexpected character `{}`", self.display(start, len)),
                    start,
                ));
            }
        };

        Ok(Token {
            kind,
            span: Span::new(to_u32(start), to_u32(self.pos)),
        })
    }

    fn single(&mut self, kind: TokenKind) -> TokenKind {
        self.pos += 1;
        kind
    }

    fn identifier(&mut self) -> TokenKind {
        let start = self.pos;
        while matches!(
            self.peek(),
            Some(b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'_')
        ) {
            self.pos += 1;
        }
        let text = std::str::from_utf8(&self.bytes[start..self.pos]).unwrap_or_default();
        keyword(text).unwrap_or_else(|| TokenKind::Ident(text.to_owned()))
    }

    fn number(&mut self) -> Result<TokenKind, Diagnostic> {
        let start = self.pos;
        while matches!(self.peek(), Some(b'0'..=b'9')) {
            self.pos += 1;
        }
        if self.peek() == Some(b'.') && self.peek_at(1) != Some(b'.') {
            if !matches!(self.peek_at(1), Some(b'0'..=b'9')) {
                self.pos += 1;
                return Err(self.error("E0103", "digit expected after `.`", start));
            }
            self.pos += 1;
            while matches!(self.peek(), Some(b'0'..=b'9')) {
                self.pos += 1;
            }
        }
        let text = std::str::from_utf8(&self.bytes[start..self.pos]).unwrap_or_default();
        let value: f64 = text
            .parse()
            .map_err(|_| self.error("E0103", format!("invalid number `{text}`"), start))?;
        if !value.is_finite() {
            return Err(self.error("E0103", "number must be finite", start));
        }
        Ok(TokenKind::Number(value))
    }

    fn string(&mut self) -> Result<TokenKind, Diagnostic> {
        let start = self.pos;
        self.pos += 1;
        let mut value = String::new();
        loop {
            match self.bump() {
                None => {
                    return Err(self.error("E0104", "unterminated string", start));
                }
                Some(b'"') => return Ok(TokenKind::String(value)),
                Some(b'\\') => match self.bump() {
                    Some(b'\\') => value.push('\\'),
                    Some(b'"') => value.push('"'),
                    Some(b'n') => value.push('\n'),
                    Some(b't') => value.push('\t'),
                    Some(_) => {
                        return Err(self.error("E0105", "invalid escape sequence", start));
                    }
                    None => {
                        return Err(self.error("E0104", "unterminated string", start));
                    }
                },
                Some(byte) => {
                    let len = Self::char_len(byte);
                    let end = self.pos + len - 1;
                    let text = std::str::from_utf8(&self.bytes[self.pos - 1..end])
                        .map_err(|_| self.error("E0102", "invalid UTF-8 in string", start))?;
                    value.push_str(text);
                    self.pos = end;
                }
            }
        }
    }

    fn color(&mut self) -> Result<TokenKind, Diagnostic> {
        let start = self.pos;
        self.pos += 1;
        let mut rgb = [0_u8; 3];
        for channel in &mut rgb {
            let Some(high) = self.bump().and_then(hex_value) else {
                return Err(self.error("E0106", "expected #RRGGBB", start));
            };
            let Some(low) = self.bump().and_then(hex_value) else {
                return Err(self.error("E0106", "expected #RRGGBB", start));
            };
            *channel = (high << 4) | low;
        }
        Ok(TokenKind::Color(rgb))
    }

    /// Length in bytes of the UTF-8 character starting with `first`.
    fn char_len(first: u8) -> usize {
        if first < 0x80 {
            1
        } else if first >> 5 == 0b110 {
            2
        } else if first >> 4 == 0b1110 {
            3
        } else if first >> 3 == 0b1_1110 {
            4
        } else {
            1
        }
    }

    fn display(&self, start: usize, len: usize) -> String {
        std::str::from_utf8(&self.bytes[start..self.pos.min(start + len)])
            .unwrap_or("�")
            .to_owned()
    }
}

fn to_u32(value: usize) -> u32 {
    u32::try_from(value).unwrap_or(u32::MAX)
}

fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

fn keyword(text: &str) -> Option<TokenKind> {
    Some(match text {
        "version" => TokenKind::Version,
        "include" => TokenKind::Include,
        "material" => TokenKind::Material,
        "object" => TokenKind::Object,
        "render" => TokenKind::Render,
        "scale" => TokenKind::Scale,
        "canvas" => TokenKind::Canvas,
        "sun" => TokenKind::Sun,
        "ambient" => TokenKind::Ambient,
        "background" => TokenKind::Background,
        "place" => TokenKind::Place,
        "mirror" => TokenKind::Mirror,
        "for" => TokenKind::For,
        "in" => TokenKind::In,
        "if" => TokenKind::If,
        "else" => TokenKind::Else,
        "let" => TokenKind::Let,
        "var" => TokenKind::Var,
        "true" => TokenKind::True,
        "false" => TokenKind::False,
        "box" => TokenKind::Box,
        "cyl" => TokenKind::Cyl,
        "sphere" => TokenKind::Sphere,
        "slantbox" => TokenKind::SlantBox,
        "slantcyl" => TokenKind::SlantCyl,
        "ramp" => TokenKind::Ramp,
        "at" => TokenKind::At,
        "size" => TokenKind::Size,
        "radius" => TokenKind::Radius,
        "height" => TokenKind::Height,
        "offset" => TokenKind::Offset,
        "rise" => TokenKind::Rise,
        "rot" => TokenKind::Rot,
        _ => return None,
    })
}

/// Tokenize a source file.
///
/// # Errors
///
/// Returns the first lexical [`Diagnostic`] if the input contains an invalid
/// character, number, string, color or an unterminated comment.
pub fn lex(source: &str) -> Result<Vec<Token>, Diagnostic> {
    if source.len() > MAX_SOURCE_BYTES {
        return Err(Diagnostic::new(
            "E0402",
            format!("source is larger than {MAX_SOURCE_BYTES} bytes"),
            Span::new(0, to_u32(source.len().min(MAX_SOURCE_BYTES + 1))),
        ));
    }

    let mut lexer = Lexer::new(source);
    let mut tokens = Vec::new();
    loop {
        if lexer.tokens >= MAX_TOKENS {
            return Err(Diagnostic::new(
                "E0402",
                format!("more than {MAX_TOKENS} tokens"),
                Span::new(to_u32(lexer.pos), to_u32(lexer.pos)),
            ));
        }
        let token = lexer.token()?;
        lexer.tokens += 1;
        let eof = token.kind == TokenKind::Eof;
        tokens.push(token);
        if eof {
            return Ok(tokens);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{TokenKind, lex};

    fn kinds(source: &str) -> Vec<TokenKind> {
        let mut tokens = lex(source).expect("lex");
        assert_eq!(tokens.pop().map(|t| t.kind), Some(TokenKind::Eof));
        tokens.into_iter().map(|t| t.kind).collect()
    }

    #[test]
    fn range_is_not_a_decimal() {
        assert_eq!(
            kinds("1..5"),
            vec![
                TokenKind::Number(1.0),
                TokenKind::DotDot,
                TokenKind::Number(5.0)
            ]
        );
        assert_eq!(kinds("1.5"), vec![TokenKind::Number(1.5)]);
    }

    #[test]
    fn comments_are_skipped() {
        assert_eq!(
            kinds("/* a /* b */ box // tail\nat"),
            vec![TokenKind::Box, TokenKind::At]
        );
    }

    #[test]
    fn unterminated_comment_is_an_error() {
        let error = lex("box /* nope").expect_err("must fail");
        assert_eq!(error.code, "E0101");
    }

    #[test]
    fn unterminated_string_is_an_error() {
        let error = lex(r#"include "nope"#).expect_err("must fail");
        assert_eq!(error.code, "E0104");
    }

    #[test]
    fn invalid_escape_is_an_error() {
        let error = lex(r#"include "a\q""#).expect_err("must fail");
        assert_eq!(error.code, "E0105");
    }

    #[test]
    fn colors_are_parsed() {
        assert_eq!(
            kinds("background #1a1C22;"),
            vec![
                TokenKind::Background,
                TokenKind::Color([0x1a, 0x1C, 0x22]),
                TokenKind::Semicolon
            ]
        );
    }

    #[test]
    fn keywords_and_identifiers_are_distinct() {
        assert_eq!(
            kinds("object chair"),
            vec![TokenKind::Object, TokenKind::Ident("chair".to_owned())]
        );
    }

    #[test]
    fn string_escapes_are_decoded() {
        assert_eq!(
            kinds(r#""a\\b\"c\nd""#),
            vec![TokenKind::String("a\\b\"c\nd".to_owned())]
        );
    }

    #[test]
    fn unicode_is_allowed_in_strings_and_comments() {
        assert_eq!(
            kinds("// привет\n\"файл\""),
            vec![TokenKind::String("файл".to_owned())]
        );
    }

    #[test]
    fn unicode_outside_strings_is_rejected() {
        let error = lex("box ×").expect_err("must fail");
        assert_eq!(error.code, "E0102");
    }

    #[test]
    fn leading_dot_number_is_rejected() {
        let error = lex(".5").expect_err("must fail");
        assert_eq!(error.code, "E0102");
    }

    #[test]
    fn trailing_dot_number_is_rejected() {
        let error = lex("1.").expect_err("must fail");
        assert_eq!(error.code, "E0103");
    }
}
