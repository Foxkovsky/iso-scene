//! Abstract syntax tree for iso-scene v1.
//!
//! The AST is deliberately close to the grammar in `docs/grammar.ebnf`.
//! Invalid primitive states (for example a `box` with a cylinder radius) are
//! unrepresentable: each shape is its own enum variant.

use crate::Span;

/// A parsed source file.
#[derive(Clone, Debug, PartialEq)]
pub struct Program {
    pub version: u32,
    pub items: Vec<Item>,
    pub span: Span,
}

/// A top-level item.
#[derive(Clone, Debug, PartialEq)]
pub struct Item {
    pub kind: ItemKind,
    pub span: Span,
}

/// Top-level item kinds.
#[derive(Clone, Debug, PartialEq)]
pub enum ItemKind {
    Include(Include),
    Material(Material),
    Object(Object),
    Render(Render),
    Scale(Scale),
    Canvas(Canvas),
    Sun(Sun),
    Ambient(Ambient),
    Background(Background),
    Statement(Box<Statement>),
}

/// `include "path";`
#[derive(Clone, Debug, PartialEq)]
pub struct Include {
    pub path: String,
}

/// `material alias = "path";`
#[derive(Clone, Debug, PartialEq)]
pub struct Material {
    pub name: String,
    pub path: String,
}

/// `render iso | top | front | side;`
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RenderKind {
    Iso,
    Top,
    Front,
    Side,
}

/// `render ...;`
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Render {
    pub kind: RenderKind,
}

/// `scale N;` — pixels per block.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Scale {
    pub pixels_per_block: u32,
}

/// `canvas (w, d, h);`
#[derive(Clone, Debug, PartialEq)]
pub struct Canvas {
    pub size: [Expr; 3],
}

/// `sun (azimuth, altitude);`
#[derive(Clone, Debug, PartialEq)]
pub struct Sun {
    pub azimuth: Expr,
    pub altitude: Expr,
}

/// `ambient value;`
#[derive(Clone, Debug, PartialEq)]
pub struct Ambient {
    pub value: Expr,
}

/// `background #RRGGBB;` or `background transparent;`
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Background {
    Color([u8; 3]),
    Transparent,
}

/// `object name (w, d) { ... }`
#[derive(Clone, Debug, PartialEq)]
pub struct Object {
    pub name: String,
    pub width: Expr,
    pub depth: Expr,
    pub body: Block,
}

/// A brace-delimited statement list.
#[derive(Clone, Debug, PartialEq)]
pub struct Block {
    pub statements: Vec<Statement>,
    pub span: Span,
}

/// A statement.
#[derive(Clone, Debug, PartialEq)]
pub struct Statement {
    pub kind: StatementKind,
    pub span: Span,
}

/// Statement kinds.
#[derive(Clone, Debug, PartialEq)]
pub enum StatementKind {
    Let {
        name: String,
        value: Expr,
        mutable: bool,
    },
    Assign {
        name: String,
        op: AssignOp,
        value: Expr,
    },
    For {
        variable: String,
        start: Expr,
        end: Expr,
        body: Block,
    },
    If {
        condition: Expr,
        then_block: Block,
        else_branch: Option<ElseBranch>,
    },
    Place {
        object: String,
        at: [Expr; 3],
        rotation: Option<Expr>,
        anchor: Option<Anchor>,
    },
    Shape(Shape),
    Mirror(Axis),
}

/// Which point of the rotated footprint sits at the `place` coordinates.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Anchor {
    /// (min x, min y): the far corner; this is the default.
    Back,
    Center,
    /// (max x, max y): the near corner, closest to the viewer.
    Front,
    /// (min x, max y): the left corner on screen.
    Left,
    /// (max x, min y): the right corner on screen.
    Right,
}

/// Assignment operator.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AssignOp {
    Set,
    Add,
    Sub,
    Mul,
    Div,
}

/// `else if ...` or `else { ... }`.
#[derive(Clone, Debug, PartialEq)]
pub enum ElseBranch {
    If(Box<Statement>),
    Block(Block),
}

/// Mirror axis.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Axis {
    X,
    Y,
}

/// A primitive shape.
#[derive(Clone, Debug, PartialEq)]
pub enum Shape {
    Box {
        at: [Expr; 3],
        size: [Expr; 3],
        material: Option<String>,
    },
    Cyl {
        at: [Expr; 3],
        radius: Expr,
        height: Expr,
        material: Option<String>,
    },
    Sphere {
        at: [Expr; 3],
        radius: Expr,
        material: Option<String>,
    },
    SlantBox {
        at: [Expr; 3],
        size: [Expr; 3],
        offset: [Expr; 2],
        material: Option<String>,
    },
    SlantCyl {
        at: [Expr; 3],
        radius: Expr,
        height: Expr,
        offset: [Expr; 2],
        material: Option<String>,
    },
    Ramp {
        at: [Expr; 3],
        size: [Expr; 2],
        rise: [Expr; 2],
        material: Option<String>,
    },
}

impl Shape {
    /// Base point of the shape.
    #[must_use]
    pub const fn at(&self) -> &[Expr; 3] {
        match self {
            Self::Box { at, .. }
            | Self::Cyl { at, .. }
            | Self::Sphere { at, .. }
            | Self::SlantBox { at, .. }
            | Self::SlantCyl { at, .. }
            | Self::Ramp { at, .. } => at,
        }
    }

    /// Optional material alias.
    #[must_use]
    pub fn material(&self) -> Option<&str> {
        match self {
            Self::Box { material, .. }
            | Self::Cyl { material, .. }
            | Self::Sphere { material, .. }
            | Self::SlantBox { material, .. }
            | Self::SlantCyl { material, .. }
            | Self::Ramp { material, .. } => material.as_deref(),
        }
    }
}

/// An expression.
#[derive(Clone, Debug, PartialEq)]
pub struct Expr {
    pub kind: ExprKind,
    pub span: Span,
}

/// Expression kinds.
#[derive(Clone, Debug, PartialEq)]
pub enum ExprKind {
    Number(f64),
    Bool(bool),
    Ident(String),
    Unary {
        op: UnaryOp,
        operand: Box<Expr>,
    },
    Binary {
        op: BinaryOp,
        left: Box<Expr>,
        right: Box<Expr>,
    },
}

/// Unary operator.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UnaryOp {
    Negate,
    Not,
}

/// Binary operator.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BinaryOp {
    Or,
    And,
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    Add,
    Sub,
    Mul,
    Div,
    Rem,
}
