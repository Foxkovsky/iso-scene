//! 2:1 isometric raster backend.
//!
//! The backend turns the scene graph into flat coloured polygons, rasterizes
//! them with a z-buffer and returns an RGBA image. Material files are not
//! loaded yet; only the built-in palettes are used.

#![allow(
    clippy::many_single_char_names,
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss
)]

use std::path::Path;

use scene_core::{Instance, SceneGraph, rotate_xy};
use scene_render::{Framebuffer, MeshFace, Palette, builtin, primitive_faces};
use scene_syntax::Diagnostic;
use scene_syntax::ast::{Background, RenderKind};

/// Rendered image (shared with the other backends).
pub type IsoImage = scene_render::Image;
use scene_render::Image;

const MAX_OUTPUT_SIDE: usize = 8_192;
const MAX_OUTPUT_PIXELS: usize = 16_000_000;
const MARGIN: f64 = 2.0;

/// Project a world point to isometric screen coordinates.
///
/// `screen_x = (x - y) * scale`, `screen_y = ((x + y) / 2 - z) * scale`.
#[must_use]
pub fn project_point(point: [f64; 3], scale: f64) -> [f64; 2] {
    [
        (point[0] - point[1]) * scale,
        (point[0].midpoint(point[1]) - point[2]) * scale,
    ]
}

/// View depth for the iso camera: larger is closer.
#[must_use]
pub fn view_depth(point: [f64; 3]) -> f64 {
    point[0] + point[1] + point[2]
}

/// Render a scene with the 2:1 isometric backend.
///
/// # Errors
///
/// Returns `E0601` for material files (not implemented yet), `E0602` for an
/// oversized image, and `E0604` for a non-iso render kind.
pub fn render(scene: &SceneGraph) -> Result<IsoImage, Diagnostic> {
    if scene.render != RenderKind::Iso {
        return Err(error(
            "E0604",
            format!("the iso backend cannot render {:?}", scene.render),
        ));
    }

    let faces = collect_faces(scene)?;
    let scale = f64::from(scene.scale);

    let mut min_x = f64::INFINITY;
    let mut min_y = f64::INFINITY;
    let mut max_x = f64::NEG_INFINITY;
    let mut max_y = f64::NEG_INFINITY;

    for face in &faces {
        for point in &face.points {
            let projected = project_point(*point, scale);
            min_x = min_x.min(projected[0]);
            min_y = min_y.min(projected[1]);
            max_x = max_x.max(projected[0]);
            max_y = max_y.max(projected[1]);
        }
    }
    if let Some(canvas) = scene.canvas {
        for corner in box_corners([0.0, 0.0, 0.0], canvas) {
            let projected = project_point(corner, scale);
            min_x = min_x.min(projected[0]);
            min_y = min_y.min(projected[1]);
            max_x = max_x.max(projected[0]);
            max_y = max_y.max(projected[1]);
        }
    }
    if !min_x.is_finite() || !min_y.is_finite() || !max_x.is_finite() || !max_y.is_finite() {
        min_x = 0.0;
        min_y = 0.0;
        max_x = 0.0;
        max_y = 0.0;
    }

    let left = (min_x.floor() - MARGIN) as isize;
    let top = (min_y.floor() - MARGIN) as isize;
    let right = (max_x.ceil() + MARGIN) as isize;
    let bottom = (max_y.ceil() + MARGIN) as isize;
    let width = usize::try_from(right - left + 1).unwrap_or(0);
    let height = usize::try_from(bottom - top + 1).unwrap_or(0);
    if width == 0
        || height == 0
        || width > MAX_OUTPUT_SIDE
        || height > MAX_OUTPUT_SIDE
        || width.saturating_mul(height) > MAX_OUTPUT_PIXELS
    {
        return Err(error(
            "E0602",
            format!("output image {width}x{height} exceeds the limit"),
        ));
    }

    let origin = [left as f64, top as f64];
    let background = match scene.background {
        Background::Transparent => [0, 0, 0, 0],
        Background::Color(rgb) => [rgb[0], rgb[1], rgb[2], 255],
    };
    let mut framebuffer = Framebuffer::new(width, height, background);
    let bias = 1.6 / scale;

    for face in &faces {
        let projected = project_face(face, scale, origin);
        for index in 1..projected.len().saturating_sub(1) {
            framebuffer.triangle(
                projected[0],
                projected[index],
                projected[index + 1],
                face.color,
            );
        }
    }

    for face in &faces {
        if !face.outline {
            continue;
        }
        let projected = project_face(face, scale, origin);
        for index in 0..projected.len() {
            framebuffer.line(
                projected[index],
                projected[(index + 1) % projected.len()],
                face.line,
                bias,
            );
        }
    }

    Ok(Image {
        width: u32::try_from(width).unwrap_or(0),
        height: u32::try_from(height).unwrap_or(0),
        rgba: framebuffer.into_rgba(),
    })
}

/// Encode an image as an RGBA PNG.
///
/// # Errors
///
/// Returns `E0603` if the file cannot be written.
pub fn write_png(image: &IsoImage, path: &Path) -> Result<(), Diagnostic> {
    scene_render::write_png(image, path).map_err(|message| error("E0603", message))
}

fn project_face(face: &Face, scale: f64, origin: [f64; 2]) -> Vec<[f64; 3]> {
    face.points
        .iter()
        .map(|point| {
            let projected = project_point(*point, scale);
            [
                projected[0] - origin[0],
                projected[1] - origin[1],
                view_depth(*point),
            ]
        })
        .collect()
}

struct Face {
    points: Vec<[f64; 3]>,
    color: [u8; 3],
    line: [u8; 3],
    outline: bool,
}

fn collect_faces(scene: &SceneGraph) -> Result<Vec<Face>, Diagnostic> {
    let mut faces = Vec::new();
    for instance in &scene.instances {
        for part in &instance.object.parts {
            let palette = palette_for(part.material.as_deref(), scene)?;
            for MeshFace {
                points,
                normal,
                outline,
            } in primitive_faces(&part.primitive)
            {
                let points = points
                    .into_iter()
                    .map(|point| world_point(point, instance))
                    .collect();
                faces.push(Face {
                    points,
                    color: iso_color(&normal, palette),
                    line: palette.line,
                    outline,
                });
            }
        }
    }
    Ok(faces)
}

fn iso_color(normal: &[f64; 3], palette: Palette) -> [u8; 3] {
    if normal[2] > normal[0].abs() + normal[1].abs() {
        palette.top
    } else if normal[0] > normal[1] {
        palette.right
    } else {
        palette.left
    }
}

fn world_point(point: [f64; 3], instance: &Instance) -> [f64; 3] {
    let (x, y) = rotate_xy(
        point[0],
        point[1],
        instance.object.width,
        instance.object.depth,
        instance.rotation,
    );
    [
        x + instance.at[0],
        y + instance.at[1],
        point[2] + instance.at[2],
    ]
}

fn palette_for(name: Option<&str>, scene: &SceneGraph) -> Result<Palette, Diagnostic> {
    let name = name.unwrap_or("wood");
    if let Some(palette) = builtin(name) {
        return Ok(palette);
    }
    if let Some(path) = scene.materials.get(name) {
        return Err(error(
            "E0601",
            format!("material files are not loaded yet: `{name}` ({path})"),
        ));
    }
    Err(error("E0605", format!("unknown material `{name}`")))
}

fn box_corners(at: [f64; 3], size: [f64; 3]) -> [[f64; 3]; 8] {
    [
        [at[0], at[1], at[2]],
        [at[0] + size[0], at[1], at[2]],
        [at[0], at[1] + size[1], at[2]],
        [at[0] + size[0], at[1] + size[1], at[2]],
        [at[0], at[1], at[2] + size[2]],
        [at[0] + size[0], at[1], at[2] + size[2]],
        [at[0], at[1] + size[1], at[2] + size[2]],
        [at[0] + size[0], at[1] + size[1], at[2] + size[2]],
    ]
}

fn error(code: &'static str, message: impl Into<String>) -> Diagnostic {
    Diagnostic::new(code, message, scene_syntax::Span::new(0, 0))
}
