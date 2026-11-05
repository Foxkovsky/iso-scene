#![allow(
    clippy::many_single_char_names,
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss
)]

//! 2:1 isometric raster backend.
//!
//! The backend is deliberately small: it turns the scene graph into flat
//! coloured polygons, rasterizes them with a z-buffer and writes RGBA PNG.
//! Material files are not loaded yet; only the built-in palettes are used.

pub mod raster;

use std::f64::consts::PI;
use std::fs::File;
use std::io::BufWriter;
use std::path::Path;

use scene_core::{Instance, Primitive, SceneGraph, rotate_xy};
use scene_syntax::Diagnostic;
use scene_syntax::ast::{Background, RenderKind};

use raster::Framebuffer;

const MAX_OUTPUT_SIDE: usize = 8_192;
const MAX_OUTPUT_PIXELS: usize = 16_000_000;
const MARGIN: f64 = 2.0;

/// Rendered image in RGBA order.
#[derive(Clone, Debug, PartialEq)]
pub struct IsoImage {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

impl IsoImage {
    #[must_use]
    pub fn pixel(&self, x: u32, y: u32) -> [u8; 4] {
        let index = (y as usize * self.width as usize + x as usize) * 4;
        [
            self.rgba[index],
            self.rgba[index + 1],
            self.rgba[index + 2],
            self.rgba[index + 3],
        ]
    }
}

#[derive(Clone, Copy)]
struct Palette {
    top: [u8; 3],
    right: [u8; 3],
    left: [u8; 3],
    line: [u8; 3],
}

#[derive(Clone, Copy)]
enum FaceKind {
    Top,
    Right,
    Left,
}

struct Face {
    points: Vec<[f64; 3]>,
    color: [u8; 3],
    line: [u8; 3],
    outline: bool,
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
            let projected = project(*point, scale);
            min_x = min_x.min(projected[0]);
            min_y = min_y.min(projected[1]);
            max_x = max_x.max(projected[0]);
            max_y = max_y.max(projected[1]);
        }
    }
    if let Some(canvas) = scene.canvas {
        for corner in box_corners([0.0, 0.0, 0.0], canvas) {
            let projected = project(corner, scale);
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
        let projected: Vec<[f64; 3]> = face
            .points
            .iter()
            .map(|point| project(*point, scale))
            .map(|point| [point[0] - origin[0], point[1] - origin[1], point[2]])
            .collect();
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
        let projected: Vec<[f64; 3]> = face
            .points
            .iter()
            .map(|point| project(*point, scale))
            .map(|point| [point[0] - origin[0], point[1] - origin[1], point[2]])
            .collect();
        for index in 0..projected.len() {
            framebuffer.line(
                projected[index],
                projected[(index + 1) % projected.len()],
                face.line,
                bias,
            );
        }
    }

    Ok(IsoImage {
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
    let file = File::create(path).map_err(|cause| {
        error(
            "E0603",
            format!("cannot create `{}`: {cause}", path.display()),
        )
    })?;
    let writer = BufWriter::new(file);
    let mut encoder = png::Encoder::new(writer, image.width, image.height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder
        .write_header()
        .map_err(|cause| error("E0603", format!("cannot write PNG header: {cause}")))?;
    writer
        .write_image_data(&image.rgba)
        .map_err(|cause| error("E0603", format!("cannot write PNG data: {cause}")))?;
    writer
        .finish()
        .map_err(|cause| error("E0603", format!("cannot finish PNG: {cause}")))?;
    Ok(())
}

fn project(point: [f64; 3], scale: f64) -> [f64; 3] {
    [
        (point[0] - point[1]) * scale,
        (point[0].midpoint(point[1]) - point[2]) * scale,
        point[0] + point[1] + point[2],
    ]
}

fn collect_faces(scene: &SceneGraph) -> Result<Vec<Face>, Diagnostic> {
    let mut faces = Vec::new();
    for instance in &scene.instances {
        for part in &instance.object.parts {
            let palette = palette_for(part.material.as_deref(), scene)?;
            let local = primitive_faces(&part.primitive);
            for face in local {
                let points = face
                    .points
                    .into_iter()
                    .map(|point| world_point(point, instance))
                    .collect();
                faces.push(Face {
                    points,
                    color: color_for(face.kind, palette),
                    line: palette.line,
                    outline: face.outline,
                });
            }
        }
    }
    Ok(faces)
}

struct PrimitiveFace {
    points: Vec<[f64; 3]>,
    kind: FaceKind,
    outline: bool,
}

fn primitive_faces(primitive: &Primitive) -> Vec<PrimitiveFace> {
    match primitive {
        Primitive::Box { at, size } => box_faces(*at, *size),
        Primitive::Cyl { at, radius, height } => cylinder_faces(*at, *radius, *height),
        Primitive::Sphere { at, radius } => sphere_faces(*at, *radius),
        Primitive::SlantBox { at, size, offset } => slant_box_faces(*at, *size, *offset),
        Primitive::SlantCyl {
            at,
            radius,
            height,
            offset,
        } => slant_cylinder_faces(*at, *radius, *height, *offset),
        Primitive::Ramp { at, size, rise } => ramp_faces(*at, *size, *rise),
    }
}

fn box_faces(at: [f64; 3], size: [f64; 3]) -> Vec<PrimitiveFace> {
    let [x, y, z] = at;
    let [w, d, h] = size;
    let (x1, y1, z1) = (x + w, y + d, z + h);
    let mut faces = Vec::new();
    add_face(
        &mut faces,
        vec![[x, y, z1], [x1, y, z1], [x1, y1, z1], [x, y1, z1]],
        true,
    );
    add_face(
        &mut faces,
        vec![[x1, y, z], [x1, y1, z], [x1, y1, z1], [x1, y, z1]],
        true,
    );
    add_face(
        &mut faces,
        vec![[x1, y1, z], [x, y1, z], [x, y1, z1], [x1, y1, z1]],
        true,
    );
    add_face(
        &mut faces,
        vec![[x, y, z], [x, y1, z], [x1, y1, z], [x1, y, z]],
        true,
    );
    add_face(
        &mut faces,
        vec![[x, y, z], [x, y, z1], [x, y1, z1], [x, y1, z]],
        true,
    );
    add_face(
        &mut faces,
        vec![[x, y, z], [x1, y, z], [x1, y, z1], [x, y, z1]],
        true,
    );
    faces
}

fn cylinder_faces(at: [f64; 3], radius: f64, height: f64) -> Vec<PrimitiveFace> {
    const SEGMENTS: usize = 48;
    let mut bottom = Vec::with_capacity(SEGMENTS);
    let mut top = Vec::with_capacity(SEGMENTS);
    for segment in 0..SEGMENTS {
        #[allow(clippy::cast_precision_loss)]
        let angle = 2.0 * PI * segment as f64 / SEGMENTS as f64;
        let x = at[0] + radius * angle.cos();
        let y = at[1] + radius * angle.sin();
        bottom.push([x, y, at[2]]);
        top.push([x, y, at[2] + height]);
    }

    let mut faces = Vec::new();
    add_face(&mut faces, top.clone(), true);
    for index in 0..SEGMENTS {
        let next = (index + 1) % SEGMENTS;
        add_face(
            &mut faces,
            vec![bottom[index], bottom[next], top[next], top[index]],
            false,
        );
    }
    faces
}

fn sphere_faces(at: [f64; 3], radius: f64) -> Vec<PrimitiveFace> {
    const RINGS: usize = 8;
    const SEGMENTS: usize = 16;
    let mut faces = Vec::new();
    for ring in 0..RINGS {
        #[allow(clippy::cast_precision_loss)]
        let lat0 = -PI / 2.0 + PI * ring as f64 / RINGS as f64;
        #[allow(clippy::cast_precision_loss)]
        let lat1 = -PI / 2.0 + PI * (ring + 1) as f64 / RINGS as f64;
        for segment in 0..SEGMENTS {
            #[allow(clippy::cast_precision_loss)]
            let lon0 = 2.0 * PI * segment as f64 / SEGMENTS as f64;
            #[allow(clippy::cast_precision_loss)]
            let lon1 = 2.0 * PI * (segment + 1) as f64 / SEGMENTS as f64;
            let points = vec![
                sphere_point(at, radius, lat0, lon0),
                sphere_point(at, radius, lat1, lon0),
                sphere_point(at, radius, lat1, lon1),
                sphere_point(at, radius, lat0, lon1),
            ];
            add_face(&mut faces, points, false);
        }
    }
    faces
}

fn sphere_point(at: [f64; 3], radius: f64, latitude: f64, longitude: f64) -> [f64; 3] {
    [
        at[0] + radius * latitude.cos() * longitude.cos(),
        at[1] + radius * latitude.cos() * longitude.sin(),
        at[2] + radius * latitude.sin(),
    ]
}

fn slant_box_faces(at: [f64; 3], size: [f64; 3], offset: [f64; 2]) -> Vec<PrimitiveFace> {
    let [x, y, z] = at;
    let [w, d, h] = size;
    let bottom = vec![[x, y, z], [x + w, y, z], [x + w, y + d, z], [x, y + d, z]];
    let top = vec![
        [x + offset[0], y + offset[1], z + h],
        [x + w + offset[0], y + offset[1], z + h],
        [x + w + offset[0], y + d + offset[1], z + h],
        [x + offset[0], y + d + offset[1], z + h],
    ];
    prism_faces(&bottom, &top, true)
}

fn slant_cylinder_faces(
    at: [f64; 3],
    radius: f64,
    height: f64,
    offset: [f64; 2],
) -> Vec<PrimitiveFace> {
    const SEGMENTS: usize = 48;
    let mut bottom = Vec::with_capacity(SEGMENTS);
    let mut top = Vec::with_capacity(SEGMENTS);
    for segment in 0..SEGMENTS {
        #[allow(clippy::cast_precision_loss)]
        let angle = 2.0 * PI * segment as f64 / SEGMENTS as f64;
        let x = at[0] + radius * angle.cos();
        let y = at[1] + radius * angle.sin();
        bottom.push([x, y, at[2]]);
        top.push([x + offset[0], y + offset[1], at[2] + height]);
    }

    let mut faces = Vec::new();
    add_face(&mut faces, top.clone(), true);
    for index in 0..SEGMENTS {
        let next = (index + 1) % SEGMENTS;
        add_face(
            &mut faces,
            vec![bottom[index], bottom[next], top[next], top[index]],
            false,
        );
    }
    faces
}

fn ramp_faces(at: [f64; 3], size: [f64; 2], rise: [f64; 2]) -> Vec<PrimitiveFace> {
    let [x, y, z] = at;
    let [w, d] = size;
    let bottom = vec![[x, y, z], [x + w, y, z], [x + w, y + d, z], [x, y + d, z]];
    let top = vec![
        [x, y, z + rise[0]],
        [x + w, y, z + rise[1]],
        [x + w, y + d, z + rise[1]],
        [x, y + d, z + rise[0]],
    ];
    prism_faces(&bottom, &top, true)
}

fn prism_faces(bottom: &[[f64; 3]], top: &[[f64; 3]], outline: bool) -> Vec<PrimitiveFace> {
    let mut faces = Vec::new();
    add_face(&mut faces, top.to_vec(), outline);
    add_face(&mut faces, bottom.to_vec(), outline);
    for index in 0..bottom.len() {
        let next = (index + 1) % bottom.len();
        add_face(
            &mut faces,
            vec![bottom[index], bottom[next], top[next], top[index]],
            outline,
        );
    }
    faces
}

fn add_face(faces: &mut Vec<PrimitiveFace>, points: Vec<[f64; 3]>, outline: bool) {
    let normal = normal(&points);
    if normal[0] * normal[0] + normal[1] * normal[1] + normal[2] * normal[2] < 1e-18 {
        return;
    }
    let kind = if normal[2] > normal[0].abs() + normal[1].abs() {
        FaceKind::Top
    } else if normal[0] > normal[1] {
        FaceKind::Right
    } else {
        FaceKind::Left
    };
    faces.push(PrimitiveFace {
        points,
        kind,
        outline,
    });
}

fn normal(points: &[[f64; 3]]) -> [f64; 3] {
    let mut normal = [0.0; 3];
    for index in 0..points.len() {
        let a = points[index];
        let b = points[(index + 1) % points.len()];
        normal[0] += (a[1] - b[1]) * (a[2] + b[2]);
        normal[1] += (a[2] - b[2]) * (a[0] + b[0]);
        normal[2] += (a[0] - b[0]) * (a[1] + b[1]);
    }
    normal
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

fn color_for(kind: FaceKind, palette: Palette) -> [u8; 3] {
    match kind {
        FaceKind::Top => palette.top,
        FaceKind::Right => palette.right,
        FaceKind::Left => palette.left,
    }
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

fn builtin(name: &str) -> Option<Palette> {
    let palette = match name {
        "wood" => Palette {
            top: [0xB0, 0xB2, 0xB6],
            right: [0x8A, 0x8D, 0x92],
            left: [0x68, 0x6B, 0x70],
            line: [0x26, 0x28, 0x2E],
        },
        "metal" => Palette {
            top: [0x96, 0x9A, 0xA0],
            right: [0x75, 0x79, 0x7F],
            left: [0x58, 0x5C, 0x62],
            line: [0x22, 0x24, 0x2A],
        },
        "light" => Palette {
            top: [0xCE, 0xD0, 0xD4],
            right: [0xA8, 0xAB, 0xB0],
            left: [0x84, 0x87, 0x8D],
            line: [0x2C, 0x2E, 0x34],
        },
        "glass" => Palette {
            top: [0x96, 0xBE, 0xC8],
            right: [0x78, 0x9E, 0xA8],
            left: [0x5E, 0x80, 0x8A],
            line: [0x28, 0x34, 0x3A],
        },
        "dark" => Palette {
            top: [0x6E, 0x70, 0x76],
            right: [0x56, 0x58, 0x5E],
            left: [0x40, 0x42, 0x48],
            line: [0x1E, 0x20, 0x26],
        },
        "red" => Palette {
            top: [0xA4, 0x18, 0x1A],
            right: [0x84, 0x12, 0x16],
            left: [0x6C, 0x10, 0x14],
            line: [0x28, 0x19, 0x1B],
        },
        "white" => Palette {
            top: [0xF4, 0xF4, 0xF0],
            right: [0xE6, 0xE7, 0xE4],
            left: [0xD7, 0xD9, 0xD7],
            line: [0x26, 0x28, 0x2E],
        },
        _ => return None,
    };
    Some(palette)
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
