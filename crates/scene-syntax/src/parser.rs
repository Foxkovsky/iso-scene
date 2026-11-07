//! Recursive-descent parser for iso-scene v1.
//!
//! The parser consumes the token stream from [`crate::lex`] and produces the
//! AST in [`crate::ast`]. It reports the first error and never panics.

use crate::ast::{
    Ambient, Anchor, AssignOp, Axis, Background, BinaryOp, Block, Canvas, ElseBranch, Expr,
    ExprKind, Include, Item, ItemKind, Material, Object, Origin, Program, Render, RenderKind,
    Scale, Shape, Statement, StatementKind, Sun, UnaryOp,
};
use crate::{Diagnostic, Span, Token, TokenKind, lex};

/// Deepest nesting of blocks, parentheses and unary operators
/// (`docs/spec.md` §9).
pub const MAX_NESTING: usize = 128;
/// Deepest expression tree, operator chains included (`docs/spec.md` §9).
/// Bounding it here keeps the recursive evaluator off the end of the stack.
pub const MAX_EXPR_DEPTH: usize = 256;

/// Parse a complete source file.
///
/// # Errors
///
/// Returns the first lexical or syntactic [`Diagnostic`].
pub fn parse(source: &str) -> Result<Program, Diagnostic> {
    let tokens = lex(source)?;
    Parser::new(tokens).parse_program()
}

struct Parser {
    tokens: Vec<Token>,
    pos: usize,
    /// Blocks, parentheses and unary operators currently open.
    nesting: usize,
    /// Expression levels currently open, operator chains included.
    depth: usize,
}

impl Parser {
    fn new(tokens: Vec<Token>) -> Self {
        debug_assert!(!tokens.is_empty(), "the lexer always emits EOF");
        Self {
            tokens,
            pos: 0,
            nesting: 0,
            depth: 0,
        }
    }

    /// Open a block, parenthesis or unary operator.
    fn nest(&mut self) -> Result<(), Diagnostic> {
        if self.nesting >= MAX_NESTING {
            return Err(Diagnostic::new(
                "E0402",
                format!("nesting is deeper than {MAX_NESTING} levels"),
                self.current().span,
            ));
        }
        self.nesting += 1;
        Ok(())
    }

    /// Open one level of an expression tree.
    fn deepen(&mut self) -> Result<(), Diagnostic> {
        if self.depth >= MAX_EXPR_DEPTH {
            return Err(Diagnostic::new(
                "E0402",
                format!("expression is deeper than {MAX_EXPR_DEPTH} levels"),
                self.current().span,
            ));
        }
        self.depth += 1;
        Ok(())
    }

    fn current(&self) -> &Token {
        let index = self.pos.min(self.tokens.len().saturating_sub(1));
        &self.tokens[index]
    }

    fn current_kind(&self) -> &TokenKind {
        &self.current().kind
    }

    fn previous_span(&self) -> Span {
        let index = self.pos.saturating_sub(1).min(self.tokens.len() - 1);
        self.tokens[index].span
    }

    fn bump(&mut self) -> Token {
        let token = self.current().clone();
        if self.pos + 1 < self.tokens.len() {
            self.pos += 1;
        }
        token
    }

    fn at(&self, kind: &TokenKind) -> bool {
        std::mem::discriminant(self.current_kind()) == std::mem::discriminant(kind)
    }

    fn eat(&mut self, kind: &TokenKind) -> bool {
        if self.at(kind) {
            self.bump();
            true
        } else {
            false
        }
    }

    fn expect(&mut self, kind: &TokenKind, what: &str) -> Result<Span, Diagnostic> {
        if self.at(kind) {
            Ok(self.bump().span)
        } else {
            Err(self.expected(what))
        }
    }

    fn expected(&self, what: &str) -> Diagnostic {
        Diagnostic::new(
            "E0201",
            format!("expected {what}, found {}", describe(self.current_kind())),
            self.current().span,
        )
    }

    fn expect_ident(&mut self, what: &str) -> Result<(String, Span), Diagnostic> {
        let TokenKind::Ident(name) = self.current_kind() else {
            return Err(self.expected(what));
        };
        let name = name.clone();
        let span = self.bump().span;
        Ok((name, span))
    }

    fn expect_string(&mut self, what: &str) -> Result<(String, Span), Diagnostic> {
        let TokenKind::String(value) = self.current_kind() else {
            return Err(self.expected(what));
        };
        let value = value.clone();
        let span = self.bump().span;
        Ok((value, span))
    }

    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    fn expect_integer(&mut self, what: &str) -> Result<(u32, Span), Diagnostic> {
        let token = self.current().clone();
        if let TokenKind::Number(value) = token.kind {
            if value.is_finite()
                && value.fract() == 0.0
                && value >= 0.0
                && value <= f64::from(u32::MAX)
            {
                self.bump();
                return Ok((value as u32, token.span));
            }
        }
        Err(Diagnostic::new(
            "E0204",
            format!("expected {what} as a non-negative integer"),
            token.span,
        ))
    }

    fn parse_program(&mut self) -> Result<Program, Diagnostic> {
        let start = self.current().span;
        self.expect(&TokenKind::Version, "`version`")?;
        let (version, version_span) = self.expect_integer("language version")?;
        if version != 1 {
            return Err(Diagnostic::new(
                "E0203",
                format!("unsupported language version {version}; expected 1"),
                version_span,
            ));
        }
        self.expect(&TokenKind::Semicolon, "`;`")?;

        let mut items = Vec::new();
        while !self.at(&TokenKind::Eof) {
            items.push(self.parse_item()?);
        }
        let end = self.current().span;
        Ok(Program {
            version,
            items,
            span: Span::new(start.start, end.end),
        })
    }

    fn parse_item(&mut self) -> Result<Item, Diagnostic> {
        let start = self.current().span;
        let kind = match self.current_kind() {
            TokenKind::Include => ItemKind::Include(self.parse_include()?),
            TokenKind::Material => ItemKind::Material(self.parse_material()?),
            TokenKind::Object => ItemKind::Object(self.parse_object()?),
            TokenKind::Render => ItemKind::Render(self.parse_render()?),
            TokenKind::Scale => ItemKind::Scale(self.parse_scale()?),
            TokenKind::Canvas => ItemKind::Canvas(self.parse_canvas()?),
            TokenKind::Sun => ItemKind::Sun(self.parse_sun()?),
            TokenKind::Ambient => ItemKind::Ambient(self.parse_ambient()?),
            TokenKind::Background => ItemKind::Background(self.parse_background()?),
            _ => ItemKind::Statement(Box::new(self.parse_statement()?)),
        };
        let end = self.previous_span();
        Ok(Item {
            kind,
            span: Span::new(start.start, end.end),
        })
    }

    fn parse_include(&mut self) -> Result<Include, Diagnostic> {
        self.expect(&TokenKind::Include, "`include`")?;
        let (path, _) = self.expect_string("include path")?;
        self.expect(&TokenKind::Semicolon, "`;`")?;
        Ok(Include { path })
    }

    fn parse_material(&mut self) -> Result<Material, Diagnostic> {
        self.expect(&TokenKind::Material, "`material`")?;
        let (name, _) = self.expect_ident("material name")?;
        self.expect(&TokenKind::Assign, "`=`")?;
        let (path, _) = self.expect_string("material path")?;
        self.expect(&TokenKind::Semicolon, "`;`")?;
        Ok(Material { name, path })
    }

    fn parse_render(&mut self) -> Result<Render, Diagnostic> {
        self.expect(&TokenKind::Render, "`render`")?;
        let (name, span) = self.expect_ident("render kind")?;
        let kind = match name.as_str() {
            "iso" => RenderKind::Iso,
            "top" => RenderKind::Top,
            "front" => RenderKind::Front,
            "side" => RenderKind::Side,
            _ => {
                return Err(Diagnostic::new(
                    "E0202",
                    format!("unknown render kind `{name}`; expected iso, top, front or side"),
                    span,
                ));
            }
        };
        self.expect(&TokenKind::Semicolon, "`;`")?;
        Ok(Render { kind })
    }

    fn parse_scale(&mut self) -> Result<Scale, Diagnostic> {
        self.expect(&TokenKind::Scale, "`scale`")?;
        let (pixels_per_block, _) = self.expect_integer("scale")?;
        self.expect(&TokenKind::Semicolon, "`;`")?;
        Ok(Scale { pixels_per_block })
    }

    fn parse_canvas(&mut self) -> Result<Canvas, Diagnostic> {
        self.expect(&TokenKind::Canvas, "`canvas`")?;
        let (size, _) = self.parse_tuple3()?;
        self.expect(&TokenKind::Semicolon, "`;`")?;
        Ok(Canvas { size })
    }

    fn parse_sun(&mut self) -> Result<Sun, Diagnostic> {
        self.expect(&TokenKind::Sun, "`sun`")?;
        let (angles, _) = self.parse_pair()?;
        let [azimuth, altitude] = angles;
        self.expect(&TokenKind::Semicolon, "`;`")?;
        Ok(Sun { azimuth, altitude })
    }

    fn parse_ambient(&mut self) -> Result<Ambient, Diagnostic> {
        self.expect(&TokenKind::Ambient, "`ambient`")?;
        let value = self.parse_expr()?;
        self.expect(&TokenKind::Semicolon, "`;`")?;
        Ok(Ambient { value })
    }

    fn parse_background(&mut self) -> Result<Background, Diagnostic> {
        self.expect(&TokenKind::Background, "`background`")?;
        let background = if let TokenKind::Color(rgb) = *self.current_kind() {
            self.bump();
            Background::Color(rgb)
        } else {
            let (name, span) = self.expect_ident("`transparent` or a color")?;
            if name == "transparent" {
                Background::Transparent
            } else {
                return Err(Diagnostic::new(
                    "E0202",
                    format!("unknown background `{name}`; expected `transparent` or `#RRGGBB`"),
                    span,
                ));
            }
        };
        self.expect(&TokenKind::Semicolon, "`;`")?;
        Ok(background)
    }

    fn parse_object(&mut self) -> Result<Object, Diagnostic> {
        self.expect(&TokenKind::Object, "`object`")?;
        let (name, _) = self.expect_ident("object name")?;
        self.expect(&TokenKind::LParen, "`(`")?;
        let width = self.parse_expr()?;
        self.expect(&TokenKind::Comma, "`,`")?;
        let depth = self.parse_expr()?;
        self.expect(&TokenKind::RParen, "`)`")?;
        let origin = if self.eat(&TokenKind::Origin) {
            let (name, span) = self.expect_ident("`back`, `center` or `front`")?;
            match name.as_str() {
                "back" => Origin::Back,
                "center" => Origin::Center,
                "front" => Origin::Front,
                other => {
                    return Err(Diagnostic::new(
                        "E0202",
                        format!("unknown origin `{other}`; expected back, center or front"),
                        span,
                    ));
                }
            }
        } else {
            Origin::Back
        };
        let body = self.parse_block()?;
        Ok(Object {
            name,
            width,
            depth,
            origin,
            body,
        })
    }

    fn parse_block(&mut self) -> Result<Block, Diagnostic> {
        self.nest()?;
        let start = self.expect(&TokenKind::LBrace, "`{`")?;
        let mut statements = Vec::new();
        while !self.at(&TokenKind::RBrace) {
            if self.at(&TokenKind::Eof) {
                return Err(self.expected("`}`"));
            }
            statements.push(self.parse_statement()?);
        }
        let end = self.expect(&TokenKind::RBrace, "`}`")?;
        self.nesting -= 1;
        Ok(Block {
            statements,
            span: Span::new(start.start, end.end),
        })
    }

    fn parse_statement(&mut self) -> Result<Statement, Diagnostic> {
        let start = self.current().span;
        let kind = match self.current_kind() {
            TokenKind::Let | TokenKind::Var => self.parse_let()?,
            TokenKind::For => self.parse_for()?,
            TokenKind::If => self.parse_if()?,
            TokenKind::Place => self.parse_place()?,
            TokenKind::Use => self.parse_use()?,
            TokenKind::Merge => self.parse_merge()?,
            TokenKind::Mirror => self.parse_mirror()?,
            TokenKind::Box
            | TokenKind::Cyl
            | TokenKind::Sphere
            | TokenKind::SlantBox
            | TokenKind::SlantCyl
            | TokenKind::Ramp => StatementKind::Shape(self.parse_shape()?),
            TokenKind::Ident(_) => self.parse_assign()?,
            _ => return Err(self.expected("a statement")),
        };
        let end = self.previous_span();
        Ok(Statement {
            kind,
            span: Span::new(start.start, end.end),
        })
    }

    fn parse_let(&mut self) -> Result<StatementKind, Diagnostic> {
        let mutable = self.at(&TokenKind::Var);
        if mutable {
            self.expect(&TokenKind::Var, "`var`")?;
        } else {
            self.expect(&TokenKind::Let, "`let`")?;
        }
        let (name, _) = self.expect_ident("variable name")?;
        self.expect(&TokenKind::Assign, "`=`")?;
        let value = self.parse_expr()?;
        self.expect(&TokenKind::Semicolon, "`;`")?;
        Ok(StatementKind::Let {
            name,
            value,
            mutable,
        })
    }

    fn parse_assign(&mut self) -> Result<StatementKind, Diagnostic> {
        let (name, _) = self.expect_ident("variable name")?;
        let op = match self.current_kind() {
            TokenKind::Assign => AssignOp::Set,
            TokenKind::PlusAssign => AssignOp::Add,
            TokenKind::MinusAssign => AssignOp::Sub,
            TokenKind::StarAssign => AssignOp::Mul,
            TokenKind::SlashAssign => AssignOp::Div,
            _ => return Err(self.expected("`=`, `+=`, `-=`, `*=` or `/=`")),
        };
        self.bump();
        let value = self.parse_expr()?;
        self.expect(&TokenKind::Semicolon, "`;`")?;
        Ok(StatementKind::Assign { name, op, value })
    }

    fn parse_for(&mut self) -> Result<StatementKind, Diagnostic> {
        self.expect(&TokenKind::For, "`for`")?;
        let (variable, _) = self.expect_ident("loop variable")?;
        self.expect(&TokenKind::In, "`in`")?;
        let start = self.parse_expr()?;
        self.expect(&TokenKind::DotDot, "`..`")?;
        let end = self.parse_expr()?;
        let body = self.parse_block()?;
        Ok(StatementKind::For {
            variable,
            start,
            end,
            body,
        })
    }

    fn parse_if(&mut self) -> Result<StatementKind, Diagnostic> {
        self.expect(&TokenKind::If, "`if`")?;
        self.expect(&TokenKind::LParen, "`(`")?;
        let condition = self.parse_expr()?;
        self.expect(&TokenKind::RParen, "`)`")?;
        let then_block = self.parse_block()?;
        let else_branch = if self.eat(&TokenKind::Else) {
            if self.at(&TokenKind::If) {
                Some(ElseBranch::If(Box::new(self.parse_statement()?)))
            } else {
                Some(ElseBranch::Block(self.parse_block()?))
            }
        } else {
            None
        };
        Ok(StatementKind::If {
            condition,
            then_block,
            else_branch,
        })
    }

    fn parse_place(&mut self) -> Result<StatementKind, Diagnostic> {
        self.expect(&TokenKind::Place, "`place`")?;
        let (object, _) = self.expect_ident("object name")?;
        self.expect(&TokenKind::At, "`at`")?;
        let (at, _) = self.parse_tuple3()?;
        let rotation = if self.eat(&TokenKind::Rot) {
            Some(self.parse_expr()?)
        } else {
            None
        };
        let anchor = if self.eat(&TokenKind::Anchor) {
            let (name, span) =
                self.expect_ident("`back`, `center`, `front`, `left`, `right` or `cell`")?;
            Some(match name.as_str() {
                "back" => Anchor::Back,
                "center" => Anchor::Center,
                "front" => Anchor::Front,
                "left" => Anchor::Left,
                "right" => Anchor::Right,
                "cell" => Anchor::Cell,
                other => {
                    return Err(Diagnostic::new(
                        "E0202",
                        format!(
                            "unknown anchor `{other}`; expected back, center, front, left, right or cell"
                        ),
                        span,
                    ));
                }
            })
        } else {
            None
        };
        self.expect(&TokenKind::Semicolon, "`;`")?;
        Ok(StatementKind::Place {
            object,
            at,
            rotation,
            anchor,
        })
    }

    fn parse_merge(&mut self) -> Result<StatementKind, Diagnostic> {
        self.expect(&TokenKind::Merge, "`merge`")?;
        let block = self.parse_block()?;
        Ok(StatementKind::Merge(block))
    }

    fn parse_use(&mut self) -> Result<StatementKind, Diagnostic> {
        self.expect(&TokenKind::Use, "`use`")?;
        let (object, _) = self.expect_ident("object name")?;
        let at = if self.eat(&TokenKind::At) {
            let (at, _) = self.parse_tuple3()?;
            Some(at)
        } else {
            None
        };
        let rotation = if self.eat(&TokenKind::Rot) {
            Some(self.parse_expr()?)
        } else {
            None
        };
        self.expect(&TokenKind::Semicolon, "`;`")?;
        Ok(StatementKind::Use {
            object,
            at,
            rotation,
        })
    }

    fn parse_mirror(&mut self) -> Result<StatementKind, Diagnostic> {
        self.expect(&TokenKind::Mirror, "`mirror`")?;
        let (axis, span) = self.expect_ident("`x` or `y`")?;
        let axis = match axis.as_str() {
            "x" => Axis::X,
            "y" => Axis::Y,
            _ => {
                return Err(Diagnostic::new(
                    "E0202",
                    format!("unknown mirror axis `{axis}`; expected x or y"),
                    span,
                ));
            }
        };
        self.expect(&TokenKind::Semicolon, "`;`")?;
        Ok(StatementKind::Mirror(axis))
    }

    fn parse_shape(&mut self) -> Result<Shape, Diagnostic> {
        let shape = match self.current_kind() {
            TokenKind::Box => {
                self.bump();
                if self.eat(&TokenKind::From) {
                    let (from, _) = self.parse_tuple3()?;
                    self.expect(&TokenKind::To, "`to`")?;
                    let (to, _) = self.parse_tuple3()?;
                    Shape::BoxBetween {
                        from,
                        to,
                        material: self.parse_optional_material()?,
                    }
                } else {
                    let at = self.parse_shape_at()?;
                    self.expect(&TokenKind::Size, "`size`")?;
                    let (size, _) = self.parse_tuple3()?;
                    Shape::Box {
                        at,
                        size,
                        material: self.parse_optional_material()?,
                    }
                }
            }
            TokenKind::Cyl => {
                self.bump();
                let at = self.parse_shape_at()?;
                let radius = self.parse_radius()?;
                let height = self.parse_height()?;
                Shape::Cyl {
                    at,
                    radius,
                    height,
                    material: self.parse_optional_material()?,
                }
            }
            TokenKind::Sphere => {
                self.bump();
                let at = self.parse_shape_at()?;
                let radius = self.parse_radius()?;
                Shape::Sphere {
                    at,
                    radius,
                    material: self.parse_optional_material()?,
                }
            }
            TokenKind::SlantBox => {
                self.bump();
                let at = self.parse_shape_at()?;
                self.expect(&TokenKind::Size, "`size`")?;
                let (size, _) = self.parse_tuple3()?;
                let offset = self.parse_offset()?;
                Shape::SlantBox {
                    at,
                    size,
                    offset,
                    material: self.parse_optional_material()?,
                }
            }
            TokenKind::SlantCyl => {
                self.bump();
                let at = self.parse_shape_at()?;
                let radius = self.parse_radius()?;
                let height = self.parse_height()?;
                let offset = self.parse_offset()?;
                Shape::SlantCyl {
                    at,
                    radius,
                    height,
                    offset,
                    material: self.parse_optional_material()?,
                }
            }
            TokenKind::Ramp => {
                self.bump();
                let at = self.parse_shape_at()?;
                self.expect(&TokenKind::Size, "`size`")?;
                let (size, _) = self.parse_pair()?;
                self.expect(&TokenKind::Rise, "`rise`")?;
                let (rise, _) = self.parse_pair()?;
                Shape::Ramp {
                    at,
                    size,
                    rise,
                    material: self.parse_optional_material()?,
                }
            }
            _ => return Err(self.expected("a shape")),
        };
        self.expect(&TokenKind::Semicolon, "`;`")?;
        Ok(shape)
    }

    fn parse_shape_at(&mut self) -> Result<[Expr; 3], Diagnostic> {
        self.expect(&TokenKind::At, "`at`")?;
        let (at, _) = self.parse_tuple3()?;
        Ok(at)
    }

    fn parse_radius(&mut self) -> Result<Expr, Diagnostic> {
        self.expect(&TokenKind::Radius, "`radius`")?;
        self.parse_expr()
    }

    fn parse_height(&mut self) -> Result<Expr, Diagnostic> {
        self.expect(&TokenKind::Height, "`height`")?;
        self.parse_expr()
    }

    fn parse_offset(&mut self) -> Result<[Expr; 2], Diagnostic> {
        self.expect(&TokenKind::Offset, "`offset`")?;
        let (offset, _) = self.parse_pair()?;
        Ok(offset)
    }

    fn parse_optional_material(&mut self) -> Result<Option<String>, Diagnostic> {
        if !self.eat(&TokenKind::Material) {
            return Ok(None);
        }
        let (name, _) = self.expect_ident("material name")?;
        Ok(Some(name))
    }

    fn parse_tuple3(&mut self) -> Result<([Expr; 3], Span), Diagnostic> {
        let start = self.expect(&TokenKind::LParen, "`(`")?;
        let first = self.parse_expr()?;
        self.expect(&TokenKind::Comma, "`,`")?;
        let second = self.parse_expr()?;
        self.expect(&TokenKind::Comma, "`,`")?;
        let third = self.parse_expr()?;
        let end = self.expect(&TokenKind::RParen, "`)`")?;
        Ok(([first, second, third], Span::new(start.start, end.end)))
    }

    fn parse_pair(&mut self) -> Result<([Expr; 2], Span), Diagnostic> {
        let start = self.expect(&TokenKind::LParen, "`(`")?;
        let first = self.parse_expr()?;
        self.expect(&TokenKind::Comma, "`,`")?;
        let second = self.parse_expr()?;
        let end = self.expect(&TokenKind::RParen, "`)`")?;
        Ok(([first, second], Span::new(start.start, end.end)))
    }

    fn parse_expr(&mut self) -> Result<Expr, Diagnostic> {
        self.parse_or()
    }

    fn parse_or(&mut self) -> Result<Expr, Diagnostic> {
        let depth = self.depth;
        let mut left = self.parse_and()?;
        while self.at(&TokenKind::OrOr) {
            // Each operator in a chain nests the chain so far one level down.
            self.deepen()?;
            self.bump();
            let right = self.parse_and()?;
            left = binary(BinaryOp::Or, left, right);
        }
        self.depth = depth;
        Ok(left)
    }

    fn parse_and(&mut self) -> Result<Expr, Diagnostic> {
        let depth = self.depth;
        let mut left = self.parse_comparison()?;
        while self.at(&TokenKind::AndAnd) {
            self.deepen()?;
            self.bump();
            let right = self.parse_comparison()?;
            left = binary(BinaryOp::And, left, right);
        }
        self.depth = depth;
        Ok(left)
    }

    fn parse_comparison(&mut self) -> Result<Expr, Diagnostic> {
        let left = self.parse_additive()?;
        let op = match self.current_kind() {
            TokenKind::EqEq => BinaryOp::Eq,
            TokenKind::NotEq => BinaryOp::Ne,
            TokenKind::Lt => BinaryOp::Lt,
            TokenKind::Le => BinaryOp::Le,
            TokenKind::Gt => BinaryOp::Gt,
            TokenKind::Ge => BinaryOp::Ge,
            _ => return Ok(left),
        };
        self.deepen()?;
        self.bump();
        let right = self.parse_additive()?;
        self.depth -= 1;
        Ok(binary(op, left, right))
    }

    fn parse_additive(&mut self) -> Result<Expr, Diagnostic> {
        let depth = self.depth;
        let mut left = self.parse_multiplicative()?;
        loop {
            let op = match self.current_kind() {
                TokenKind::Plus => BinaryOp::Add,
                TokenKind::Minus => BinaryOp::Sub,
                _ => {
                    self.depth = depth;
                    return Ok(left);
                }
            };
            self.deepen()?;
            self.bump();
            let right = self.parse_multiplicative()?;
            left = binary(op, left, right);
        }
    }

    fn parse_multiplicative(&mut self) -> Result<Expr, Diagnostic> {
        let depth = self.depth;
        let mut left = self.parse_unary()?;
        loop {
            let op = match self.current_kind() {
                TokenKind::Star => BinaryOp::Mul,
                TokenKind::Slash => BinaryOp::Div,
                TokenKind::Percent => BinaryOp::Rem,
                _ => {
                    self.depth = depth;
                    return Ok(left);
                }
            };
            self.deepen()?;
            self.bump();
            let right = self.parse_unary()?;
            left = binary(op, left, right);
        }
    }

    fn parse_unary(&mut self) -> Result<Expr, Diagnostic> {
        let op = match self.current_kind() {
            TokenKind::Minus => Some(UnaryOp::Negate),
            TokenKind::Bang => Some(UnaryOp::Not),
            _ => None,
        };
        if let Some(op) = op {
            self.nest()?;
            self.deepen()?;
            let start = self.bump().span;
            let operand = self.parse_unary()?;
            self.nesting -= 1;
            self.depth -= 1;
            let end = operand.span;
            return Ok(Expr {
                kind: ExprKind::Unary {
                    op,
                    operand: Box::new(operand),
                },
                span: Span::new(start.start, end.end),
            });
        }
        self.parse_primary()
    }

    fn parse_primary(&mut self) -> Result<Expr, Diagnostic> {
        let token = self.current().clone();
        match token.kind {
            TokenKind::Number(value) => {
                self.bump();
                Ok(Expr {
                    kind: ExprKind::Number(value),
                    span: token.span,
                })
            }
            TokenKind::True => {
                self.bump();
                Ok(Expr {
                    kind: ExprKind::Bool(true),
                    span: token.span,
                })
            }
            TokenKind::False => {
                self.bump();
                Ok(Expr {
                    kind: ExprKind::Bool(false),
                    span: token.span,
                })
            }
            TokenKind::Ident(name) => {
                self.bump();
                Ok(Expr {
                    kind: ExprKind::Ident(name),
                    span: token.span,
                })
            }
            TokenKind::LParen => {
                self.nest()?;
                self.deepen()?;
                self.bump();
                let expr = self.parse_expr()?;
                let end = self.expect(&TokenKind::RParen, "`)`")?;
                self.nesting -= 1;
                self.depth -= 1;
                Ok(Expr {
                    kind: expr.kind,
                    span: Span::new(token.span.start, end.end),
                })
            }
            _ => Err(self.expected("an expression")),
        }
    }
}

fn binary(op: BinaryOp, left: Expr, right: Expr) -> Expr {
    let span = Span::new(left.span.start, right.span.end);
    Expr {
        kind: ExprKind::Binary {
            op,
            left: Box::new(left),
            right: Box::new(right),
        },
        span,
    }
}

fn describe(kind: &TokenKind) -> String {
    match kind {
        TokenKind::Ident(name) => format!("identifier `{name}`"),
        TokenKind::Number(value) => format!("number `{value}`"),
        TokenKind::String(_) => "string".to_owned(),
        TokenKind::Color(_) => "color".to_owned(),
        TokenKind::Eof => "end of file".to_owned(),
        TokenKind::Version => "`version`".to_owned(),
        TokenKind::Include => "`include`".to_owned(),
        TokenKind::Material => "`material`".to_owned(),
        TokenKind::Object => "`object`".to_owned(),
        TokenKind::Render => "`render`".to_owned(),
        TokenKind::Scale => "`scale`".to_owned(),
        TokenKind::Canvas => "`canvas`".to_owned(),
        TokenKind::Sun => "`sun`".to_owned(),
        TokenKind::Ambient => "`ambient`".to_owned(),
        TokenKind::Background => "`background`".to_owned(),
        TokenKind::Place => "`place`".to_owned(),
        TokenKind::Mirror => "`mirror`".to_owned(),
        TokenKind::For => "`for`".to_owned(),
        TokenKind::In => "`in`".to_owned(),
        TokenKind::If => "`if`".to_owned(),
        TokenKind::Else => "`else`".to_owned(),
        TokenKind::Let => "`let`".to_owned(),
        TokenKind::Var => "`var`".to_owned(),
        TokenKind::True => "`true`".to_owned(),
        TokenKind::False => "`false`".to_owned(),
        TokenKind::Box => "`box`".to_owned(),
        TokenKind::Cyl => "`cyl`".to_owned(),
        TokenKind::Sphere => "`sphere`".to_owned(),
        TokenKind::SlantBox => "`slantbox`".to_owned(),
        TokenKind::SlantCyl => "`slantcyl`".to_owned(),
        TokenKind::Ramp => "`ramp`".to_owned(),
        TokenKind::At => "`at`".to_owned(),
        TokenKind::Size => "`size`".to_owned(),
        TokenKind::Radius => "`radius`".to_owned(),
        TokenKind::Height => "`height`".to_owned(),
        TokenKind::Offset => "`offset`".to_owned(),
        TokenKind::Rise => "`rise`".to_owned(),
        TokenKind::Rot => "`rot`".to_owned(),
        TokenKind::Anchor => "`anchor`".to_owned(),
        TokenKind::From => "`from`".to_owned(),
        TokenKind::To => "`to`".to_owned(),
        TokenKind::Use => "`use`".to_owned(),
        TokenKind::Merge => "`merge`".to_owned(),
        TokenKind::Origin => "`origin`".to_owned(),
        TokenKind::LBrace => "`{`".to_owned(),
        TokenKind::RBrace => "`}`".to_owned(),
        TokenKind::LParen => "`(`".to_owned(),
        TokenKind::RParen => "`)`".to_owned(),
        TokenKind::Semicolon => "`;`".to_owned(),
        TokenKind::Comma => "`,`".to_owned(),
        TokenKind::Assign => "`=`".to_owned(),
        TokenKind::Plus => "`+`".to_owned(),
        TokenKind::Minus => "`-`".to_owned(),
        TokenKind::Star => "`*`".to_owned(),
        TokenKind::Slash => "`/`".to_owned(),
        TokenKind::Percent => "`%`".to_owned(),
        TokenKind::PlusAssign => "`+=`".to_owned(),
        TokenKind::MinusAssign => "`-=`".to_owned(),
        TokenKind::StarAssign => "`*=`".to_owned(),
        TokenKind::SlashAssign => "`/=`".to_owned(),
        TokenKind::EqEq => "`==`".to_owned(),
        TokenKind::NotEq => "`!=`".to_owned(),
        TokenKind::Lt => "`<`".to_owned(),
        TokenKind::Le => "`<=`".to_owned(),
        TokenKind::Gt => "`>`".to_owned(),
        TokenKind::Ge => "`>=`".to_owned(),
        TokenKind::AndAnd => "`&&`".to_owned(),
        TokenKind::OrOr => "`||`".to_owned(),
        TokenKind::Bang => "`!`".to_owned(),
        TokenKind::DotDot => "`..`".to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::{MAX_EXPR_DEPTH, MAX_NESTING, parse};
    use crate::Diagnostic;
    use crate::ast::{
        Anchor, AssignOp, Background, BinaryOp, ElseBranch, ExprKind, ItemKind, Origin, RenderKind,
        Shape, StatementKind,
    };

    fn parse_ok(source: &str) -> crate::ast::Program {
        parse(source).unwrap_or_else(|error| panic!("{}: {}", error.code, error.message))
    }

    fn parse_err(source: &str) -> Diagnostic {
        parse(source).expect_err("expected a parse error")
    }

    fn nested(open: &str, inner: &str, close: &str, depth: usize) -> String {
        format!("{}{inner}{}", open.repeat(depth), close.repeat(depth))
    }

    /// Run `test` on an 8 MiB stack, as a Linux main thread has: debug frames
    /// of deeply nested blocks outgrow the 2 MiB of a test thread (a release
    /// build needs under 512 KiB at the limits).
    fn on_main_stack(test: impl FnOnce() + Send + 'static) {
        std::thread::Builder::new()
            .stack_size(8 * 1024 * 1024)
            .spawn(test)
            .expect("spawn")
            .join()
            .unwrap_or_else(|panic| std::panic::resume_unwind(panic));
    }

    #[test]
    fn nesting_is_limited_before_the_stack_runs_out() {
        on_main_stack(nesting_limits);
    }

    fn nesting_limits() {
        let scene = |expr: &str| format!("version 1; render iso; scale 8; let x = {expr};");
        // Parentheses and unary operators: 128 levels parse, 129 do not, and
        // a hundred thousand give a diagnostic instead of a stack overflow.
        for (open, close) in [("(", ")"), ("-", ""), ("!", "")] {
            assert!(parse(&scene(&nested(open, "1", close, MAX_NESTING))).is_ok());
            for depth in [MAX_NESTING + 1, 100_000] {
                let error = parse_err(&scene(&nested(open, "1", close, depth)));
                assert_eq!(error.code, "E0402", "{open} x {depth}");
            }
        }
        // Blocks count too; the object body is the first one.
        let object = |depth: usize| {
            let body = nested("if (true) { ", "", "} ", depth);
            format!("version 1; object o (1, 1) {{ {body} }}")
        };
        parse_ok(&object(MAX_NESTING - 1));
        assert_eq!(parse_err(&object(MAX_NESTING)).code, "E0402");
    }

    #[test]
    fn operator_chains_count_towards_the_expression_depth() {
        let scene = |expr: &str| format!("version 1; render iso; scale 8; let x = {expr};");
        let chain = |op: &str, terms: usize| vec!["1"; terms].join(op);
        for op in [" + ", " * ", " && ", " || "] {
            parse_ok(&scene(&chain(op, MAX_EXPR_DEPTH + 1)));
            let error = parse_err(&scene(&chain(op, MAX_EXPR_DEPTH + 2)));
            assert_eq!(error.code, "E0402", "{op}");
        }
        let error = parse_err(&scene(&chain(" + ", 200_000)));
        assert_eq!(error.code, "E0402");
    }

    #[test]
    fn long_strings_are_rejected() {
        let include = |bytes: usize| format!("version 1; include \"{}\";", "a".repeat(bytes));
        parse_ok(&include(crate::lexer::MAX_STRING_BYTES));
        assert_eq!(
            parse_err(&include(crate::lexer::MAX_STRING_BYTES + 1)).code,
            "E0402"
        );
    }

    #[test]
    fn minimal_program() {
        let program = parse_ok("version 1;");
        assert_eq!(program.version, 1);
        assert_eq!(program.items.len(), 0);
    }

    #[test]
    fn object_with_shapes_and_mirror() {
        let source = r"
            version 1;
            object chair (1.8, 1.8) {
                let leg = 0.16;
                box at (0.08, 0.08, 0) size (leg, leg, 1.64) material metal;
                mirror y;
                box at (0, 0, 1.64) size (w, d, 0.16) material wood;
            }
        ";
        let program = parse_ok(source);
        let ItemKind::Object(object) = &program.items[0].kind else {
            panic!("expected an object");
        };
        assert_eq!(object.name, "chair");
        assert_eq!(object.body.statements.len(), 4);
        let StatementKind::Shape(Shape::Box { size, material, .. }) =
            &object.body.statements[1].kind
        else {
            panic!("expected a box");
        };
        assert_eq!(size.len(), 3);
        assert_eq!(material.as_deref(), Some("metal"));
    }

    #[test]
    fn arithmetic_precedence() {
        let program = parse_ok("version 1; let x = 1 + 2 * 3;");
        let ItemKind::Statement(statement) = &program.items[0].kind else {
            panic!("expected a statement");
        };
        let StatementKind::Let { value, .. } = &statement.kind else {
            panic!("expected a let");
        };
        let ExprKind::Binary {
            op: BinaryOp::Add,
            right,
            ..
        } = &value.kind
        else {
            panic!("expected addition");
        };
        assert!(matches!(
            &right.kind,
            ExprKind::Binary {
                op: BinaryOp::Mul,
                ..
            }
        ));
    }

    #[test]
    fn for_range_and_place() {
        let program = parse_ok("version 1; for i in 0..4 { place chair at (0, 0, 0) rot 90; }");
        let ItemKind::Statement(statement) = &program.items[0].kind else {
            panic!("expected a statement");
        };
        let StatementKind::For {
            variable,
            start,
            end,
            body,
        } = &statement.kind
        else {
            panic!("expected a for");
        };
        assert_eq!(variable, "i");
        assert_eq!(&start.kind, &ExprKind::Number(0.0));
        assert_eq!(&end.kind, &ExprKind::Number(4.0));
        let StatementKind::Place { rotation, .. } = &body.statements[0].kind else {
            panic!("expected a place");
        };
        assert_eq!(
            rotation.as_ref().map(|expr| &expr.kind),
            Some(&ExprKind::Number(90.0))
        );
    }

    fn place_anchor(source: &str) -> Option<Anchor> {
        let program = parse_ok(source);
        let ItemKind::Statement(statement) = &program.items[0].kind else {
            panic!("expected a place statement");
        };
        let StatementKind::Place { anchor, .. } = &statement.kind else {
            panic!("expected a place");
        };
        *anchor
    }

    #[test]
    fn box_can_be_given_by_two_corners() {
        let program = parse_ok("version 1; object a (1, 1) { box from (0, 0, 0) to (1, 1, 1); }");
        let ItemKind::Object(object) = &program.items[0].kind else {
            panic!("expected an object");
        };
        let StatementKind::Shape(Shape::BoxBetween { from, to, .. }) =
            &object.body.statements[0].kind
        else {
            panic!("expected a box between corners");
        };
        assert_eq!(from.len(), 3);
        assert_eq!(to.len(), 3);
    }

    #[test]
    fn use_is_parsed_inside_an_object() {
        let program = parse_ok("version 1; object a (1, 1) { use b at (0.1, 0.2, 0) rot 90; }");
        let ItemKind::Object(object) = &program.items[0].kind else {
            panic!("expected an object");
        };
        let StatementKind::Use {
            object: name,
            at,
            rotation,
        } = &object.body.statements[0].kind
        else {
            panic!("expected a use");
        };
        assert_eq!(name, "b");
        assert!(at.is_some());
        assert!(rotation.is_some());
    }

    #[test]
    fn object_origin_is_parsed() {
        let program = parse_ok("version 1; object a (2, 2) origin center { }");
        let ItemKind::Object(object) = &program.items[0].kind else {
            panic!("expected an object");
        };
        assert_eq!(object.origin, Origin::Center);
        let program = parse_ok("version 1; object a (2, 2) { }");
        let ItemKind::Object(object) = &program.items[0].kind else {
            panic!("expected an object");
        };
        assert_eq!(object.origin, Origin::Back);
    }

    #[test]
    fn an_unknown_origin_is_reported() {
        let error = parse_err("version 1; object a (2, 2) origin middle { }");
        assert_eq!(error.code, "E0202");
    }

    #[test]
    fn place_anchor_is_parsed() {
        assert_eq!(
            place_anchor("version 1; place a at (1, 2, 0) anchor front;"),
            Some(Anchor::Front)
        );
        assert_eq!(
            place_anchor("version 1; place a at (1, 2, 0) rot 90 anchor left;"),
            Some(Anchor::Left)
        );
        assert_eq!(
            place_anchor("version 1; place a at (1, 2, 0) anchor cell;"),
            Some(Anchor::Cell)
        );
    }

    #[test]
    fn place_anchor_defaults_to_none() {
        assert_eq!(place_anchor("version 1; place a at (0, 0, 0);"), None);
    }

    #[test]
    fn an_unknown_place_anchor_is_reported() {
        let error = parse_err("version 1; place a at (0, 0, 0) anchor corner;");
        assert_eq!(error.code, "E0202");
        assert!(
            error
                .message
                .contains("back, center, front, left, right or cell")
        );
    }

    #[test]
    fn else_if_chain() {
        let source = "version 1; if (true) { let a = 1; } else if (false) { let b = 2; } else { let c = 3; }";
        let program = parse_ok(source);
        let ItemKind::Statement(statement) = &program.items[0].kind else {
            panic!("expected a statement");
        };
        let StatementKind::If { else_branch, .. } = &statement.kind else {
            panic!("expected an if");
        };
        let Some(ElseBranch::If(nested)) = else_branch else {
            panic!("expected else if");
        };
        let StatementKind::If { else_branch, .. } = &nested.kind else {
            panic!("expected nested if");
        };
        assert!(matches!(else_branch, Some(ElseBranch::Block(_))));
    }

    #[test]
    fn all_shape_kinds() {
        let source = r"
            version 1;
            box at (0, 0, 0) size (1, 1, 1) material wood;
            cyl at (0, 0, 0) radius 1 height 2;
            sphere at (0, 0, 0) radius 1;
            slantbox at (0, 0, 0) size (1, 1, 1) offset (0.1, 0.2);
            slantcyl at (0, 0, 0) radius 1 height 2 offset (0.1, 0.2);
            ramp at (0, 0, 0) size (1, 1) rise (0, 0.5);
        ";
        let program = parse_ok(source);
        let shapes: Vec<&Shape> = program
            .items
            .iter()
            .filter_map(|item| match &item.kind {
                ItemKind::Statement(statement) => match &statement.kind {
                    StatementKind::Shape(shape) => Some(shape),
                    _ => None,
                },
                _ => None,
            })
            .collect();
        assert_eq!(shapes.len(), 6);
        assert!(matches!(shapes[0], &Shape::Box { .. }));
        assert!(matches!(shapes[1], &Shape::Cyl { .. }));
        assert!(matches!(shapes[2], &Shape::Sphere { .. }));
        assert!(matches!(shapes[3], &Shape::SlantBox { .. }));
        assert!(matches!(shapes[4], &Shape::SlantCyl { .. }));
        assert!(matches!(shapes[5], &Shape::Ramp { .. }));
    }

    #[test]
    fn directives() {
        let source = r#"
            version 1;
            include "chair.model";
            material oak = "oak.toml";
            render top;
            scale 16;
            canvas (10, 20, 30);
            sun (135, 45);
            ambient 0.5;
            background transparent;
            background #102030;
        "#;
        let program = parse_ok(source);
        assert_eq!(program.items.len(), 9);
        assert!(matches!(
            &program.items[2].kind,
            ItemKind::Render(render) if render.kind == RenderKind::Top
        ));
        assert!(matches!(
            &program.items[3].kind,
            ItemKind::Scale(scale) if scale.pixels_per_block == 16
        ));
        assert!(matches!(
            &program.items[7].kind,
            ItemKind::Background(Background::Transparent)
        ));
        assert!(matches!(
            &program.items[8].kind,
            ItemKind::Background(Background::Color([0x10, 0x20, 0x30]))
        ));
    }

    #[test]
    fn compound_assignment() {
        let program = parse_ok("version 1; var a = 1; a += 2; a -= 1; a *= 3; a /= 2;");
        let ops: Vec<AssignOp> = program
            .items
            .iter()
            .filter_map(|item| match &item.kind {
                ItemKind::Statement(statement) => match &statement.kind {
                    StatementKind::Assign { op, .. } => Some(*op),
                    _ => None,
                },
                _ => None,
            })
            .collect();
        assert_eq!(
            ops,
            vec![AssignOp::Add, AssignOp::Sub, AssignOp::Mul, AssignOp::Div]
        );
    }

    #[test]
    fn comments_are_ignored() {
        let program = parse_ok("version 1; /* block */ let a = 1; // line\nlet b = 2;");
        assert_eq!(program.items.len(), 2);
    }

    #[test]
    fn unsupported_version() {
        assert_eq!(parse_err("version 2;").code, "E0203");
    }

    #[test]
    fn missing_semicolon() {
        assert_eq!(parse_err("version 1").code, "E0201");
    }

    #[test]
    fn scale_must_be_an_integer() {
        assert_eq!(parse_err("version 1; scale 1.5;").code, "E0204");
    }

    #[test]
    fn render_kind_is_checked() {
        assert_eq!(parse_err("version 1; render diagonal;").code, "E0202");
    }

    #[test]
    fn shape_arguments_match_the_shape() {
        assert_eq!(
            parse_err("version 1; object a (1, 1) { box at (0, 0, 0) radius 1; }").code,
            "E0201"
        );
    }

    #[test]
    fn unclosed_block_is_reported() {
        assert_eq!(parse_err("version 1; for i in 0..4 {").code, "E0201");
    }

    #[test]
    fn lexer_errors_are_propagated() {
        assert_eq!(parse_err("version 1; @").code, "E0102");
    }
}
