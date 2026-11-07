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
    /// Indoor floor lies right behind the wall, at `x - 1` or `y - 1`: the
    /// wall faces the viewer and hides what stands in the room. A wall right
    /// behind a door or window gap (at `x + 1` or `y + 1`) is not in front,
    /// so the door in that gap is not cut.
    pub front: bool,
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
    let indoor = indoor_floor(&layout_lines, columns as usize);
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
            let indoor_at = |x: u32, y: u32| indoor[(y * columns + x) as usize];
            let gap_at = |x: u32, y: u32| matches!(grid_char(&layout_lines, x, y), 'D' | 'W');
            // A wall right behind a gap stays at the back, so the door or
            // window standing in that gap is drawn whole over it.
            let front = ((x > 0 && indoor_at(x - 1, y)) || (y > 0 && indoor_at(x, y - 1)))
                && !gap_at(x + 1, y)
                && !gap_at(x, y + 1);
            cells.push(WallCell { x, y, mask, front });
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

/// How far the contact shadow at a wall's base reaches into the floor cell.
const SHADOW_REACH: f64 = 0.4;
/// Opacity of the contact shadow right at the wall.
const SHADOW_ALPHA: f64 = 0.42;

/// Build the 16-tile strip of floor shadows cast by neighbouring walls.
///
/// Tile `mask` darkens the floor diamond along the sides whose bit is set
/// (`top = 1`, `right = 2`, `bottom = 4`, `left = 8`, as for walls, but a set
/// bit means a wall **is** there). The shadow is black with an alpha that
/// fades from the wall into the cell, so it works on any floor.
#[must_use]
pub fn render_shadow_tiles(scale: u32) -> WallTiles {
    let s = f64::from(scale.max(1));
    let tile_width = 2 * scale.max(1);
    let tile_height = scale.max(1);
    let width = (tile_width * 16) as usize;
    let mut strip = Image {
        width: tile_width * 16,
        height: tile_height,
        rgba: vec![0; width * tile_height as usize * 4],
    };
    for y in 0..tile_height {
        for x in 0..tile_width {
            // Pixel centre in cell space: `u` runs along x, `v` along y, both
            // 0..1 inside the diamond whose top vertex is at (s, 0).
            let px = (f64::from(x) + 0.5 - s) / s;
            let py = 2.0 * (f64::from(y) + 0.5) / s;
            let u = f64::midpoint(py, px);
            let v = f64::midpoint(py, -px);
            if !(0.0..=1.0).contains(&u) || !(0.0..=1.0).contains(&v) {
                continue;
            }
            let sides = [
                (BIT_TOP, v),
                (BIT_RIGHT, 1.0 - u),
                (BIT_BOTTOM, 1.0 - v),
                (BIT_LEFT, u),
            ];
            for mask in 1..16_u8 {
                let shade = sides
                    .iter()
                    .filter(|(bit, _)| mask & bit != 0)
                    .map(|&(_, distance)| (1.0 - distance / SHADOW_REACH).max(0.0).powi(2))
                    .fold(0.0_f64, f64::max);
                if shade <= 0.0 {
                    continue;
                }
                let alpha = (shade * SHADOW_ALPHA * 255.0).round() as u8;
                let at = (y as usize * width + (u32::from(mask) * tile_width + x) as usize) * 4;
                strip.rgba[at..at + 4].copy_from_slice(&[0, 0, 0, alpha]);
            }
        }
    }
    WallTiles {
        image: strip,
        tile_width,
        tile_height,
    }
}

/// Build the Tiled layer of wall contact shadows for a map.
///
/// Every cell that is not a wall gets the shadow tile for the walls around
/// it (see [`render_shadow_tiles`]); cells without neighbouring walls stay
/// empty.
#[must_use]
pub fn shadow_layer_data(
    layout: &WallLayout,
    map_width: u32,
    map_height: u32,
    offset_x: i64,
    offset_y: i64,
    first_gid: u32,
) -> Vec<u32> {
    let columns = i64::from(layout.columns);
    let rows = i64::from(layout.rows);
    let mut wall = vec![false; (layout.columns as usize) * (layout.rows as usize)];
    for cell in &layout.cells {
        wall[(cell.y * layout.columns + cell.x) as usize] = true;
    }
    let wall_at = |x: i64, y: i64| {
        x >= 0 && y >= 0 && x < columns && y < rows && wall[(y * columns + x) as usize]
    };
    let mut data = vec![0_u32; (map_width as usize) * (map_height as usize)];
    for map_y in 0..i64::from(map_height) {
        for map_x in 0..i64::from(map_width) {
            let (x, y) = (map_x + offset_x, map_y + offset_y);
            if wall_at(x, y) {
                continue;
            }
            let mask = [
                (BIT_TOP, x, y - 1),
                (BIT_RIGHT, x + 1, y),
                (BIT_BOTTOM, x, y + 1),
                (BIT_LEFT, x - 1, y),
            ]
            .iter()
            .filter(|&&(_, x, y)| wall_at(x, y))
            .fold(0_u8, |mask, &(bit, _, _)| mask | bit);
            if mask != 0 {
                data[(map_y * i64::from(map_width) + map_x) as usize] = first_gid + u32::from(mask);
            }
        }
    }
    data
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
    layer_data_where(
        layout,
        map_width,
        map_height,
        offset_x,
        offset_y,
        first_gid,
        |_| true,
    )
}

/// Like [`layer_data`], keeping only the cells `keep` accepts (for example
/// the walls in [`WallCell::front`] of the floor).
#[must_use]
pub fn layer_data_where(
    layout: &WallLayout,
    map_width: u32,
    map_height: u32,
    offset_x: i64,
    offset_y: i64,
    first_gid: u32,
    keep: impl Fn(&WallCell) -> bool,
) -> Vec<u32> {
    let mut data = vec![0_u32; (map_width * map_height) as usize];
    for cell in layout.cells.iter().filter(|cell| keep(cell)) {
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

/// Floor cells enclosed by walls and gaps, row by row.
///
/// Floor reachable from the layout border without crossing a wall or a gap is
/// outdoors; the rest is indoors. Gaps count as closed (a door in its frame).
fn indoor_floor(lines: &[(String, Span)], columns: usize) -> Vec<bool> {
    let rows = lines.len();
    let floor: Vec<bool> = lines
        .iter()
        .flat_map(|(row, _)| row.chars().map(|c| c == '.'))
        .collect();
    let mut outdoor = vec![false; floor.len()];
    let mut stack: Vec<usize> = (0..floor.len())
        .filter(|&index| {
            let (x, y) = (index % columns, index / columns);
            x == 0 || y == 0 || x + 1 == columns || y + 1 == rows
        })
        .collect();
    while let Some(index) = stack.pop() {
        if !floor[index] || outdoor[index] {
            continue;
        }
        outdoor[index] = true;
        let (x, y) = (index % columns, index / columns);
        if x > 0 {
            stack.push(index - 1);
        }
        if x + 1 < columns {
            stack.push(index + 1);
        }
        if y > 0 {
            stack.push(index - columns);
        }
        if y + 1 < rows {
            stack.push(index + columns);
        }
    }
    floor
        .iter()
        .zip(&outdoor)
        .map(|(&floor, &outdoor)| floor && !outdoor)
        .collect()
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
