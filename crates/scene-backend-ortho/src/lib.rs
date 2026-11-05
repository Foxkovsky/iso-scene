//! Orthographic backends: top, front and side views.

#![allow(
    clippy::many_single_char_names,
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss
)]

use std::collections::BTreeMap;
use std::path::Path;

use scene_core::{Instance, SceneGraph, rotate_xy};
use scene_render::{Framebuffer, MeshFace, Palette, builtin, primitive_faces};
use scene_syntax::Diagnostic;
use scene_syntax::ast::{Background, RenderKind};

/// Rendered image (shared with the other backends).
pub type OrthoImage = scene_render::Image;
use scene_render::Image;

const MAX_OUTPUT_SIDE: usize = 8_192;
const MAX_OUTPUT_PIXELS: usize = 16_000_000;
const MARGIN: f64 = 2.0;

/// One orthographic view.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum View {
    Top,
    Front,
    Side,
}

impl View {
    /// Map a scene render kind to an orthographic view.
    #[must_use]
    pub fn from_render(kind: RenderKind) -> Option<Self> {
        match kind {
            RenderKind::Top => Some(Self::Top),
            RenderKind::Front => Some(Self::Front),
            RenderKind::Side => Some(Self::Side),
            RenderKind::Iso => None,
        }
    }
}

/// Project a world point to screen coordinates.
///
/// `top`:   `(x, y)`
/// `front`: `(x, -z)`
/// `side`:  `(y, -z)`
#[must_use]
pub fn project_point(view: View, point: [f64; 3], scale: f64) -> [f64; 2] {
    match view {
        View::Top => [point[0] * scale, point[1] * scale],
        View::Front => [point[0] * scale, -point[2] * scale],
        View::Side => [point[1] * scale, -point[2] * scale],
    }
}

/// View depth: larger is closer to the camera.
#[must_use]
pub fn view_depth(view: View, point: [f64; 3]) -> f64 {
    match view {
        View::Top => point[2],
        View::Front => -point[1],
        View::Side => point[0],
    }
}

/// Render a scene with an orthographic backend.
///
/// # Errors
///
/// Returns `E0602` for an oversized image, `E0604` for the iso kind and
/// `E0601` for material files (not implemented yet).
pub fn render(scene: &SceneGraph) -> Result<OrthoImage, Diagnostic> {
    let Some(view) = View::from_render(scene.render) else {
        return Err(error(
            "E0604",
            format!("the ortho backend cannot render {:?}", scene.render),
        ));
    };

    let palettes = load_palettes(scene)?;
    let faces = collect_faces(scene, &palettes)?;
    let scale = f64::from(scene.scale);

    let mut min_x = f64::INFINITY;
    let mut min_y = f64::INFINITY;
    let mut max_x = f64::NEG_INFINITY;
    let mut max_y = f64::NEG_INFINITY;

    for face in &faces {
        for point in &face.points {
            let projected = project_point(view, *point, scale);
            min_x = min_x.min(projected[0]);
            min_y = min_y.min(projected[1]);
            max_x = max_x.max(projected[0]);
            max_y = max_y.max(projected[1]);
        }
    }
    if let Some(canvas) = scene.canvas {
        for corner in box_corners([0.0, 0.0, 0.0], canvas) {
            let projected = project_point(view, corner, scale);
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
        let projected = project_face(view, face, scale, origin);
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
        let projected = project_face(view, face, scale, origin);
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
pub fn write_png(image: &OrthoImage, path: &Path) -> Result<(), Diagnostic> {
    scene_render::write_png(image, path).map_err(|message| error("E0603", message))
}

fn project_face(view: View, face: &Face, scale: f64, origin: [f64; 2]) -> Vec<[f64; 3]> {
    face.points
        .iter()
        .map(|point| {
            let projected = project_point(view, *point, scale);
            [
                projected[0] - origin[0],
                projected[1] - origin[1],
                view_depth(view, *point),
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

fn collect_faces(
    scene: &SceneGraph,
    palettes: &BTreeMap<String, Palette>,
) -> Result<Vec<Face>, Diagnostic> {
    let view = View::from_render(scene.render)
        .ok_or_else(|| error("E0604", "the ortho backend needs top, front or side"))?;
    let mut faces = Vec::new();
    for instance in &scene.instances {
        for part in &instance.object.parts {
            let palette = palette_for(part.material.as_deref(), scene, palettes)?;
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
                    color: view_color(view, &normal, palette),
                    line: palette.line,
                    outline,
                });
            }
        }
    }
    Ok(faces)
}

fn view_color(view: View, normal: &[f64; 3], palette: Palette) -> [u8; 3] {
    match view {
        View::Top => {
            if normal[2] > 0.5 {
                palette.top
            } else if normal[0] > normal[1] {
                palette.right
            } else {
                palette.left
            }
        }
        View::Front => {
            if normal[1] < -0.5 {
                palette.left
            } else if normal[2] > 0.5 {
                palette.top
            } else {
                palette.right
            }
        }
        View::Side => {
            if normal[0] > 0.5 {
                palette.right
            } else if normal[2] > 0.5 {
                palette.top
            } else {
                palette.left
            }
        }
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

fn load_palettes(scene: &SceneGraph) -> Result<BTreeMap<String, Palette>, Diagnostic> {
    let mut palettes = BTreeMap::new();
    for (name, path) in &scene.materials {
        let palette = scene_render::material::load(path)
            .map_err(|message| error("E0601", format!("material `{name}`: {message}")))?;
        palettes.insert(name.clone(), palette);
    }
    Ok(palettes)
}

fn palette_for(
    name: Option<&str>,
    scene: &SceneGraph,
    palettes: &BTreeMap<String, Palette>,
) -> Result<Palette, Diagnostic> {
    let name = name.unwrap_or("wood");
    if let Some(palette) = builtin(name) {
        return Ok(palette);
    }
    if let Some(palette) = palettes.get(name) {
        return Ok(*palette);
    }
    if scene.materials.contains_key(name) {
        return Err(error("E0601", format!("material `{name}` was not loaded")));
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
