//! Wall layouts, 16-way autotiling and the Tiled wall tileset.
//!
//! A `.walls` file is a grid of cells:
//!
//! ```text
//! version 1;
//! height 2;
//! material wall = "materials/wall.toml";
//! layout {
//!     "#########"
//!     "#.......#"
//!     "####.####"
//! }
//! ```
//!
//! `#` is a wall, `.` is empty, `D` is a door gap and `W` is a window gap.
//! Wall masks use the same bit order as the top-down wall set:
//! `top = 1`, `right = 2`, `bottom = 4`, `left = 8`; a set bit means the side
//! is open (no neighbouring wall).

#![allow(
    clippy::many_single_char_names,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss
)]

use std::fs;
use std::path::{Component, Path};

use scene_render::{Framebuffer, Image, Palette, builtin};
use scene_syntax::Diagnostic;
use scene_syntax::Span;

pub const BIT_TOP: u8 = 1;
pub const BIT_RIGHT: u8 = 2;
pub const BIT_BOTTOM: u8 = 4;
pub const BIT_LEFT: u8 = 8;

/// One wall cell and its connection mask.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WallCell {
    pub x: u32,
    pub y: u32,
    pub mask: u8,
}

/// Parsed wall layout.
#[derive(Clone, Debug)]
pub struct WallLayout {
    pub columns: u32,
    pub rows: u32,
    pub wall_height: u32,
    pub palette: Palette,
    pub cells: Vec<WallCell>,
}

/// A horizontal strip of 16 wall tiles.
#[derive(Clone, Debug)]
pub struct WallTiles {
    pub image: Image,
    pub tile_width: u32,
    pub tile_height: u32,
}

/// Load a `.walls` file.
///
/// # Errors
///
/// Returns a diagnostic for invalid syntax, unsupported characters, invalid
/// dimensions or an unreadable material file.
pub fn load(path: &Path, root: &Path) -> Result<WallLayout, Diagnostic> {
    let source = fs::read_to_string(path).map_err(|cause| {
        diag(
            "E0501",
            format!("cannot read `{}`: {cause}", path.display()),
            Span::new(0, 0),
        )
    })?;
    let base = path.parent().unwrap_or_else(|| Path::new("."));
    parse(&source, base, root)
}

/// Parse a wall layout from text.
///
/// # Errors
///
/// Returns a diagnostic with a byte span in `source`.
#[allow(clippy::too_many_lines)]
pub fn parse(source: &str, base_dir: &Path, root: &Path) -> Result<WallLayout, Diagnostic> {
    let mut version = false;
    let mut wall_height = 2_u32;
    let mut material: Option<String> = None;
    let mut layout_lines: Vec<(String, Span)> = Vec::new();
    let mut in_layout = false;
    let mut offset = 0_usize;

    for raw in source.split_inclusive('\n') {
        let start = offset;
        offset += raw.len();
        let line = raw.trim_end_matches(['\n', '\r']);
        let span = Span::new(
            u32::try_from(start).unwrap_or(u32::MAX),
            u32::try_from(start + line.len()).unwrap_or(u32::MAX),
        );
        let line = line.split("//").next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }

        if in_layout {
            if line == "}" {
                in_layout = false;
                continue;
            }
            let text = line.trim_end_matches([';', ',']).trim();
            let Some(inner) = text
                .strip_prefix('"')
                .and_then(|value| value.strip_suffix('"'))
            else {
                return Err(diag("E0201", "layout rows must be quoted strings", span));
            };
            layout_lines.push((inner.to_owned(), span));
            continue;
        }

        let lower = line.to_ascii_lowercase();
        if lower.starts_with("version") {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() != 2 || parts[1] != "1;" {
                return Err(diag("E0201", "expected `version 1;`", span));
            }
            version = true;
        } else if lower.starts_with("height") {
            wall_height = parse_u32_directive(line, "height", span)?;
            if !(1..=8).contains(&wall_height) {
                return Err(diag("E0304", "wall height must be in 1..=8", span));
            }
        } else if lower.starts_with("thickness") {
            let thickness = parse_u32_directive(line, "thickness", span)?;
            if thickness != 1 {
                return Err(diag(
                    "E0304",
                    "wall thickness must be 1 block for a full-cell wall",
                    span,
                ));
            }
        } else if lower.starts_with("material") {
            material = Some(parse_material(line, span)?);
        } else if lower == "layout {" {
            in_layout = true;
        } else {
            return Err(diag(
                "E0201",
                "expected `version`, `height`, `thickness`, `material` or `layout {`",
                span,
            ));
        }
    }

    if !version {
        return Err(diag("E0201", "missing `version 1;`", Span::new(0, 0)));
    }
    if in_layout {
        return Err(diag("E0201", "unclosed `layout {`", Span::new(0, 0)));
    }
    if layout_lines.is_empty() {
        return Err(diag("E0201", "layout is empty", Span::new(0, 0)));
    }

    let columns = layout_lines[0].0.chars().count();
    if columns == 0 {
        return Err(diag("E0201", "layout row is empty", layout_lines[0].1));
    }
    for (row, span) in &layout_lines {
        if row.chars().count() != columns {
            return Err(diag(
                "E0201",
                "all layout rows must have the same width",
                *span,
            ));
        }
        if let Some(character) = row.chars().find(|c| !matches!(c, '#' | '.' | 'D' | 'W')) {
            return Err(diag(
                "E0202",
                format!("unexpected layout character `{character}`; use #, ., D or W"),
                *span,
            ));
        }
    }

    let palette = match material {
        Some(path) => load_palette(&path, base_dir)?,
        None => builtin("light")
            .ok_or_else(|| diag("E0305", "missing default material", Span::new(0, 0)))?,
    };

    let rows = layout_lines.len() as u32;
    let columns = columns as u32;
    let mut cells = Vec::new();
    for y in 0..rows {
        for x in 0..columns {
            if grid_char(&layout_lines, x, y) != '#' {
                continue;
            }
            let mut mask = 0_u8;
            if grid_char(&layout_lines, x, y.saturating_sub(1)) != '#' || y == 0 {
                mask |= BIT_TOP;
            }
            if x + 1 >= columns || grid_char(&layout_lines, x + 1, y) != '#' {
                mask |= BIT_RIGHT;
            }
            if y + 1 >= rows || grid_char(&layout_lines, x, y + 1) != '#' {
                mask |= BIT_BOTTOM;
            }
            if x == 0 || grid_char(&layout_lines, x.saturating_sub(1), y) != '#' {
                mask |= BIT_LEFT;
            }
            cells.push(WallCell { x, y, mask });
        }
    }

    let _ = root;
    Ok(WallLayout {
        columns,
        rows,
        wall_height,
        palette,
        cells,
    })
}

/// Build the 16-tile horizontal strip for a layout at `scale` pixels/block.
#[must_use]
pub fn render_tiles(layout: &WallLayout, scale: u32) -> WallTiles {
    let s = f64::from(scale);
    let body = f64::from(layout.wall_height) * s;
    let tile_width = (2 * scale).max(1);
    let tile_height = ((layout.wall_height + 1) * scale).max(1);
    let mut strip = Image {
        width: tile_width * 16,
        height: tile_height,
        rgba: vec![0; (tile_width * 16 * tile_height * 4) as usize],
    };

    for mask in 0..16_u8 {
        let tile = wall_tile(mask, layout.palette, s, body, tile_width, tile_height);
        let offset = u32::from(mask) * tile_width;
        for y in 0..tile_height {
            for x in 0..tile_width {
                let source = ((y * tile_width + x) * 4) as usize;
                let target = ((y * strip.width + offset + x) * 4) as usize;
                strip.rgba[target..target + 4].copy_from_slice(&tile[source..source + 4]);
            }
        }
    }

    WallTiles {
        image: strip,
        tile_width,
        tile_height,
    }
}

/// Tiled edge `wangid` for a wall mask.
#[must_use]
pub fn wangid(mask: u8) -> [i32; 8] {
    let connected = |bit| i32::from(mask & bit == 0);
    [
        connected(BIT_TOP),
        0,
        connected(BIT_RIGHT),
        0,
        connected(BIT_BOTTOM),
        0,
        connected(BIT_LEFT),
        0,
    ]
}

/// Build a Tiled `walls` layer for a map.
#[must_use]
pub fn layer_data(
    layout: &WallLayout,
    map_width: u32,
    map_height: u32,
    offset_x: i64,
    offset_y: i64,
    first_gid: u32,
) -> Vec<u32> {
    let mut data = vec![0_u32; (map_width * map_height) as usize];
    for cell in &layout.cells {
        let x = i64::from(cell.x) - offset_x;
        let y = i64::from(cell.y) - offset_y;
        if x < 0 || y < 0 || x >= i64::from(map_width) || y >= i64::from(map_height) {
            continue;
        }
        data[(y as u32 * map_width + x as u32) as usize] = first_gid + u32::from(cell.mask);
    }
    data
}

fn wall_tile(mask: u8, palette: Palette, s: f64, body: f64, width: u32, height: u32) -> Vec<u8> {
    let mut fb = Framebuffer::new(width as usize, height as usize, [0, 0, 0, 0]);
    let top = [[s, 0.0], [2.0 * s, s / 2.0], [s, s], [0.0, s / 2.0]];
    let right = [
        [s, s],
        [2.0 * s, s / 2.0],
        [2.0 * s, s / 2.0 + body],
        [s, s + body],
    ];
    let left = [[0.0, s / 2.0], [s, s], [s, s + body], [0.0, s / 2.0 + body]];

    fill(&mut fb, &top, palette.top);
    fill(&mut fb, &right, palette.right);
    fill(&mut fb, &left, palette.left);

    let bias = 1.0;
    let line = palette.line;
    if mask & BIT_TOP != 0 {
        fb.line(
            [top[0][0], top[0][1], 0.0],
            [top[1][0], top[1][1], 0.0],
            line,
            bias,
        );
    }
    if mask & BIT_RIGHT != 0 {
        fb.line(
            [top[1][0], top[1][1], 0.0],
            [top[2][0], top[2][1], 0.0],
            line,
            bias,
        );
        fb.line(
            [2.0 * s, s / 2.0, 0.0],
            [2.0 * s, s / 2.0 + body, 0.0],
            line,
            bias,
        );
        fb.line(
            [s, s + body, 0.0],
            [2.0 * s, s / 2.0 + body, 0.0],
            line,
            bias,
        );
    }
    if mask & BIT_BOTTOM != 0 {
        fb.line(
            [top[2][0], top[2][1], 0.0],
            [top[3][0], top[3][1], 0.0],
            line,
            bias,
        );
        fb.line([0.0, s / 2.0 + body, 0.0], [s, s + body, 0.0], line, bias);
    }
    if mask & BIT_LEFT != 0 {
        fb.line(
            [top[3][0], top[3][1], 0.0],
            [top[0][0], top[0][1], 0.0],
            line,
            bias,
        );
        fb.line([0.0, s / 2.0, 0.0], [0.0, s / 2.0 + body, 0.0], line, bias);
    }
    fb.into_rgba()
}

fn fill(fb: &mut Framebuffer, points: &[[f64; 2]], color: [u8; 3]) {
    for index in 1..points.len().saturating_sub(1) {
        fb.triangle(
            [points[0][0], points[0][1], 0.0],
            [points[index][0], points[index][1], 0.0],
            [points[index + 1][0], points[index + 1][1], 0.0],
            color,
        );
    }
}

fn grid_char(lines: &[(String, Span)], x: u32, y: u32) -> char {
    lines
        .get(y as usize)
        .and_then(|(row, _)| row.chars().nth(x as usize))
        .unwrap_or('.')
}

fn load_palette(path: &str, base_dir: &Path) -> Result<Palette, Diagnostic> {
    let path = Path::new(path);
    if path.is_absolute()
        || path.components().any(|component| {
            matches!(
                component,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        })
    {
        return Err(diag(
            "E0502",
            "wall material path must be relative and stay inside the project",
            Span::new(0, 0),
        ));
    }
    scene_render::material::load(&base_dir.join(path)).map_err(|message| {
        diag(
            "E0601",
            format!("wall material: {message}"),
            Span::new(0, 0),
        )
    })
}

fn parse_u32_directive(line: &str, name: &str, span: Span) -> Result<u32, Diagnostic> {
    let value = line
        .strip_prefix(name)
        .map(str::trim)
        .and_then(|value| value.strip_suffix(';'))
        .map(str::trim)
        .ok_or_else(|| diag("E0201", format!("expected `{name} N;`"), span))?;
    value
        .parse()
        .map_err(|_| diag("E0201", format!("`{name}` expects a number"), span))
}

fn parse_material(line: &str, span: Span) -> Result<String, Diagnostic> {
    let Some(rest) = line.strip_prefix("material") else {
        return Err(diag("E0201", "expected `material`", span));
    };
    let Some((left, right)) = rest.split_once('=') else {
        return Err(diag("E0201", "expected `material name = \"path\";`", span));
    };
    if left.trim().is_empty() {
        return Err(diag("E0201", "expected a material alias", span));
    }
    let path = right.trim().trim_end_matches(';').trim();
    let Some(path) = path
        .strip_prefix('"')
        .and_then(|value| value.strip_suffix('"'))
    else {
        return Err(diag("E0201", "material path must be quoted", span));
    };
    Ok(path.to_owned())
}

fn diag(code: &'static str, message: impl Into<String>, span: Span) -> Diagnostic {
    Diagnostic::new(code, message, span)
}
