//! 2:1 isometric raster backend.
//!
//! The backend turns the scene graph into flat coloured polygons, rasterizes
//! them with a z-buffer and returns an RGBA image. Materials are loaded from
//! built-in palettes or TOML files referenced by the scene graph.

#![allow(
    clippy::many_single_char_names,
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    clippy::cast_sign_loss
)]

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use scene_core::{CompiledObject, Instance, SceneGraph, rotate_xy};
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
    Ok(render_internal(scene, MARGIN)?.image)
}

/// A single rendered object with the pixel position of its local origin.
#[derive(Clone, Debug)]
pub struct Sprite {
    pub image: Image,
    pub anchor: [f64; 2],
}

struct Rendered {
    image: Image,
    origin: [f64; 2],
}

/// Render one object (already rotated) into a transparent sprite.
///
/// # Errors
///
/// Returns the same diagnostics as [`render`].
pub fn render_object_sprite(
    object: &CompiledObject,
    rotation: u16,
    scale: u32,
    materials: &BTreeMap<String, PathBuf>,
) -> Result<Sprite, Diagnostic> {
    let graph = SceneGraph {
        render: RenderKind::Iso,
        scale,
        canvas: None,
        sun: [135.0, 45.0],
        ambient: 0.35,
        background: Background::Transparent,
        materials: materials.clone(),
        instances: vec![Instance {
            object: Arc::new(object.clone()),
            at: [0.0, 0.0, 0.0],
            rotation,
        }],
        bounds: object.local_bounds(),
    };
    let rendered = render_internal(&graph, MARGIN)?;
    let (x, y) = rotate_xy(0.0, 0.0, object.width, object.depth, rotation);
    let screen = project_point([x, y, 0.0], f64::from(scale));
    Ok(Sprite {
        image: rendered.image,
        anchor: [
            screen[0] - rendered.origin[0],
            screen[1] - rendered.origin[1],
        ],
    })
}

fn render_internal(scene: &SceneGraph, margin: f64) -> Result<Rendered, Diagnostic> {
    if scene.render != RenderKind::Iso {
        return Err(error(
            "E0604",
            format!("the iso backend cannot render {:?}", scene.render),
        ));
    }

    let palettes = load_palettes(scene)?;
    let faces = collect_faces(scene, &palettes)?;
    let scale = f64::from(scene.scale);
    let (width, height, origin) = scene_size(scene, &faces, margin)?;

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

    Ok(Rendered {
        image: Image {
            width: u32::try_from(width).unwrap_or(0),
            height: u32::try_from(height).unwrap_or(0),
            rgba: framebuffer.into_rgba(),
        },
        origin,
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

/// Render a scene as an SVG document.
///
/// The faces are the same ones the raster backend draws. They are painted from
/// far to near; a face lying completely behind another face's plane is always
/// painted before it, so its edges cannot show through.
///
/// # Errors
///
/// Returns `E0604` for a non-iso scene, `E0601`/`E0605` for materials and
/// `E0602` for an output that is too large.
pub fn render_svg(scene: &SceneGraph) -> Result<String, Diagnostic> {
    use std::fmt::Write as _;

    if scene.render != RenderKind::Iso {
        return Err(error(
            "E0604",
            format!("the iso backend cannot render {:?}", scene.render),
        ));
    }

    let palettes = load_palettes(scene)?;
    let faces = collect_faces(scene, &palettes)?;
    let scale = f64::from(scene.scale);
    let (width, height, origin) = scene_size(scene, &faces, MARGIN)?;
    let order = order_faces(&faces, scale);

    let mut svg = String::new();
    let _ = writeln!(
        svg,
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{width}\" height=\"{height}\" viewBox=\"0 0 {width} {height}\">"
    );
    if let Background::Color([red, green, blue]) = scene.background {
        let _ = writeln!(
            svg,
            "<rect width=\"100%\" height=\"100%\" fill=\"#{red:02x}{green:02x}{blue:02x}\"/>"
        );
    }
    for index in order {
        let face = &faces[index];
        let mut points = String::new();
        for (position, point) in face.points.iter().enumerate() {
            let projected = project_point(*point, scale);
            if position > 0 {
                points.push(' ');
            }
            let _ = write!(
                points,
                "{:.3},{:.3}",
                projected[0] - origin[0],
                projected[1] - origin[1]
            );
        }
        let fill = format!(
            "#{:02x}{:02x}{:02x}",
            face.color[0], face.color[1], face.color[2]
        );
        let stroke = if face.outline {
            format!(
                "#{:02x}{:02x}{:02x}",
                face.line[0], face.line[1], face.line[2]
            )
        } else {
            "none".to_string()
        };
        let _ = writeln!(
            svg,
            "<polygon points=\"{points}\" fill=\"{fill}\" stroke=\"{stroke}\" stroke-width=\"1\" stroke-linejoin=\"round\"/>"
        );
    }
    let _ = writeln!(svg, "</svg>");
    Ok(svg)
}

/// Order faces by the visibility the raster backend would produce.
///
/// A depth buffer records which face wins every pixel. Each time two faces meet
/// at a pixel, the one behind gets an ordering constraint before the one in
/// front. A topological sort then turns the pixel-level constraints into a
/// painter order, so the vector output covers exactly what the PNG covers.
///
/// Faces that genuinely interleave (in front at one pixel, behind at another)
/// cannot be ordered this way; a cycle is broken by mean depth.
fn order_faces(faces: &[Face], scale: f64) -> Vec<usize> {
    let count = faces.len();
    if count == 0 {
        return Vec::new();
    }

    let mut min_x = f64::INFINITY;
    let mut min_y = f64::INFINITY;
    let mut max_x = f64::NEG_INFINITY;
    let mut max_y = f64::NEG_INFINITY;
    for face in faces {
        for point in &face.points {
            let projected = project_point(*point, scale);
            min_x = min_x.min(projected[0]);
            min_y = min_y.min(projected[1]);
            max_x = max_x.max(projected[0]);
            max_y = max_y.max(projected[1]);
        }
    }
    if !min_x.is_finite() || !min_y.is_finite() || !max_x.is_finite() || !max_y.is_finite() {
        return (0..count).collect();
    }
    let origin = [min_x.floor() - 1.0, min_y.floor() - 1.0];
    let width = usize::try_from((max_x.ceil() + 1.0 - origin[0]) as isize).unwrap_or(0);
    let height = usize::try_from((max_y.ceil() + 1.0 - origin[1]) as isize).unwrap_or(0);
    if width == 0 || height == 0 || width.saturating_mul(height) > MAX_OUTPUT_PIXELS {
        return (0..count).collect();
    }

    let mut depths = vec![f64::NEG_INFINITY; width * height];
    let mut owners = vec![usize::MAX; width * height];
    let mut mean_depth = Vec::with_capacity(count);
    let mut after: Vec<Vec<usize>> = vec![Vec::new(); count];

    for (index, face) in faces.iter().enumerate() {
        let mut depth_sum = 0.0;
        for point in &face.points {
            depth_sum += view_depth(*point);
        }
        mean_depth.push(if face.points.is_empty() {
            0.0
        } else {
            depth_sum / face.points.len() as f64
        });

        let projected: Vec<[f64; 3]> = face
            .points
            .iter()
            .map(|point| {
                let screen = project_point(*point, scale);
                [
                    screen[0] - origin[0],
                    screen[1] - origin[1],
                    view_depth(*point),
                ]
            })
            .collect();
        for triangle in 1..projected.len().saturating_sub(1) {
            rasterize_visibility(
                projected[0],
                projected[triangle],
                projected[triangle + 1],
                index,
                width,
                height,
                &mut depths,
                &mut owners,
                &mut after,
            );
        }
    }

    let mut placed = vec![false; count];
    let mut order = Vec::with_capacity(count);
    for _ in 0..count {
        let mut pick: Option<usize> = None;
        for index in 0..count {
            if placed[index] || after[index].iter().any(|&other| !placed[other]) {
                continue;
            }
            if pick.is_none_or(|best| mean_depth[index] < mean_depth[best]) {
                pick = Some(index);
            }
        }
        let index = pick.or_else(|| {
            (0..count)
                .filter(|&index| !placed[index])
                .min_by(|&left, &right| mean_depth[left].total_cmp(&mean_depth[right]))
        });
        let Some(index) = index else {
            break;
        };
        placed[index] = true;
        order.push(index);
    }
    order
}

#[allow(clippy::too_many_arguments)]
fn rasterize_visibility(
    a: [f64; 3],
    b: [f64; 3],
    c: [f64; 3],
    index: usize,
    width: usize,
    height: usize,
    depths: &mut [f64],
    owners: &mut [usize],
    after: &mut [Vec<usize>],
) {
    const EPSILON: f64 = 1e-9;
    let denominator = (b[1] - c[1]) * (a[0] - c[0]) + (c[0] - b[0]) * (a[1] - c[1]);
    if denominator.abs() < 1e-12 {
        return;
    }
    let min_x = a[0].min(b[0]).min(c[0]).floor().max(0.0) as usize;
    let max_x = a[0]
        .max(b[0])
        .max(c[0])
        .ceil()
        .min(width.saturating_sub(1) as f64) as usize;
    let min_y = a[1].min(b[1]).min(c[1]).floor().max(0.0) as usize;
    let max_y = a[1]
        .max(b[1])
        .max(c[1])
        .ceil()
        .min(height.saturating_sub(1) as f64) as usize;
    if min_x > max_x || min_y > max_y || width == 0 || height == 0 {
        return;
    }

    for y in min_y..=max_y {
        for x in min_x..=max_x {
            let px = x as f64 + 0.5;
            let py = y as f64 + 0.5;
            let u = ((b[1] - c[1]) * (px - c[0]) + (c[0] - b[0]) * (py - c[1])) / denominator;
            let v = ((c[1] - a[1]) * (px - c[0]) + (a[0] - c[0]) * (py - c[1])) / denominator;
            let w = 1.0 - u - v;
            if u < -1e-9 || v < -1e-9 || w < -1e-9 {
                continue;
            }
            let z = u * a[2] + v * b[2] + w * c[2];
            let at = y * width + x;
            let owner = owners[at];
            if owner == usize::MAX {
                depths[at] = z;
                owners[at] = index;
            } else if z > depths[at] + EPSILON {
                if owner != index && !after[index].contains(&owner) {
                    after[index].push(owner);
                }
                depths[at] = z;
                owners[at] = index;
            } else if z < depths[at] - EPSILON && owner != index && !after[owner].contains(&index) {
                after[owner].push(index);
            }
        }
    }
}

fn scene_size(
    scene: &SceneGraph,
    faces: &[Face],
    margin: f64,
) -> Result<(usize, usize, [f64; 2]), Diagnostic> {
    let scale = f64::from(scene.scale);
    let mut min_x = f64::INFINITY;
    let mut min_y = f64::INFINITY;
    let mut max_x = f64::NEG_INFINITY;
    let mut max_y = f64::NEG_INFINITY;

    for face in faces {
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

    let left = (min_x.floor() - margin) as isize;
    let top = (min_y.floor() - margin) as isize;
    let right = (max_x.ceil() + margin) as isize;
    let bottom = (max_y.ceil() + margin) as isize;
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
    Ok((width, height, [left as f64, top as f64]))
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

fn collect_faces(
    scene: &SceneGraph,
    palettes: &BTreeMap<String, Palette>,
) -> Result<Vec<Face>, Diagnostic> {
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

#[cfg(test)]
mod tests {
    use super::*;

    fn face(points: Vec<[f64; 3]>) -> Face {
        Face {
            points,
            color: [128, 128, 128],
            line: [30, 32, 38],
            outline: true,
        }
    }

    #[test]
    fn a_handle_behind_a_panel_is_painted_before_it() {
        let panel = face(vec![
            [-1.0, 0.2, 0.0],
            [1.0, 0.2, 0.0],
            [1.0, 0.2, 2.0],
            [-1.0, 0.2, 2.0],
        ]);
        let behind = face(vec![
            [-0.2, 0.0, 0.8],
            [0.2, 0.0, 0.8],
            [0.2, 0.0, 1.2],
            [-0.2, 0.0, 1.2],
        ]);
        let front = face(vec![
            [-0.2, 0.4, 0.8],
            [0.2, 0.4, 0.8],
            [0.2, 0.4, 1.2],
            [-0.2, 0.4, 1.2],
        ]);

        let order = order_faces(&[front, panel, behind], 32.0);
        let position = |index: usize| {
            order
                .iter()
                .position(|&value| value == index)
                .expect("every face is ordered")
        };
        assert!(
            position(2) < position(1),
            "the handle behind the panel must be painted first: {order:?}"
        );
        assert!(
            position(1) < position(0),
            "the handle in front must be painted last: {order:?}"
        );
    }
}
