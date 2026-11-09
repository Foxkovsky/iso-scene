//! One animated instance of a scene packed as a horizontal strip of frames,
//! with a Tiled tileset whose first tile plays them and a one-tile map.

use std::fs;
use std::path::{Path, PathBuf};

use rayon::prelude::*;
use scene_core::SceneGraph;
use scene_render::{Image, composite_over, write_png};
use scene_syntax::Diagnostic;
use serde_json::json;

use crate::{
    MAX_FRAMES, MAX_SHEET_PIXELS, MAX_SHEET_SIDE, error, frame_angles, validate_name, write_json,
};

/// What to animate and how.
#[derive(Clone, Debug)]
pub struct AnimateOptions {
    /// File stem for `<name>.png` and `<name>.tsj`.
    pub name: String,
    /// First angle in degrees.
    pub from: f64,
    /// Last angle in degrees.
    pub to: f64,
    /// Number of frames, `1..=MAX_FRAMES`.
    pub frames: u32,
    /// Frame duration in milliseconds.
    pub duration: u32,
}

/// Files written by [`export_animation`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AnimateReport {
    pub image: PathBuf,
    pub tileset: PathBuf,
    pub map: PathBuf,
    pub frames: usize,
    pub frame_size: [u32; 2],
}

/// Render the scene once per frame with instance `index` turned around its
/// local origin, and write `<name>.png`, `<name>.tsj` and `scene.tmj`.
///
/// Every frame shows the whole scene on a transparent background; frames are
/// aligned so the scene does not move between them.
///
/// # Errors
///
/// Returns `E0606` for invalid options, `E0602` for a strip beyond the size
/// limit (checked before allocating it), `E0603` for file errors and the
/// render diagnostics of the iso backend.
pub fn export_animation(
    scene: &SceneGraph,
    index: usize,
    options: &AnimateOptions,
    out_dir: &Path,
) -> Result<AnimateReport, Diagnostic> {
    validate_name(&options.name).map_err(|message| error("E0606", message))?;
    if !(1..=MAX_FRAMES).contains(&options.frames) {
        return Err(error(
            "E0606",
            format!("an animation needs 1..={MAX_FRAMES} frames"),
        ));
    }
    if !options.from.is_finite() || !options.to.is_finite() {
        return Err(error("E0606", "animation angles must be finite"));
    }
    let mut transparent = scene.clone();
    transparent.background = scene_syntax::ast::Background::Transparent;
    let angles = frame_angles(options.from, options.to, options.frames);
    let frames = scene_backend_iso::prepare_frames(&transparent, index, &angles)?;
    let layout = frames.layout();
    let [frame_w, frame_h] = frame_size(&layout)?;
    let rendered = frames.render();
    let sheet = pack(&rendered, &layout, [frame_w, frame_h]);

    fs::create_dir_all(out_dir).map_err(|cause| {
        error(
            "E0603",
            format!("cannot create `{}`: {cause}", out_dir.display()),
        )
    })?;
    let png_name = format!("{}.png", options.name);
    let image = out_dir.join(&png_name);
    write_png(&sheet, &image).map_err(|message| error("E0603", message))?;

    let (tileset, map) = write_tiled(
        &options.name,
        options.duration,
        true,
        out_dir,
        rendered.len(),
        [frame_w, frame_h],
    )?;
    Ok(AnimateReport {
        image,
        tileset,
        map,
        frames: rendered.len(),
        frame_size: [frame_w, frame_h],
    })
}

/// Render frames of a scene `animation` (one compiled scene per frame) and
/// write `<name>.png`, `<name>.tsj` and `scene.tmj`.
///
/// Frames are aligned on the scene's projected origin, so whatever does not
/// move stays put. They render in parallel batches; after each batch the
/// strip that holds every frame so far is checked against the limits, so an
/// oversized animation stops early instead of rendering every frame first.
///
/// # Errors
///
/// Returns `E0606` for an invalid name or frame count, `E0602` for a strip
/// beyond the size limit, `E0603` for file errors and the render
/// diagnostics of the scene's backend.
pub fn export_scene_animation(
    frames: &[SceneGraph],
    name: &str,
    duration: u32,
    looped: bool,
    out_dir: &Path,
) -> Result<AnimateReport, Diagnostic> {
    validate_name(name).map_err(|message| error("E0606", message))?;
    if frames.is_empty() || frames.len() > MAX_FRAMES as usize {
        return Err(error(
            "E0606",
            format!("an animation needs 1..={MAX_FRAMES} frames"),
        ));
    }
    let render = |frame: &SceneGraph| match frame.render {
        scene_syntax::ast::RenderKind::Iso => scene_backend_iso::render_with_origin(frame),
        _ => scene_backend_ortho::render_with_origin(frame),
    };
    let batch = rayon::current_num_threads().max(1);
    let mut rendered: Vec<(Image, [f64; 2])> = Vec::with_capacity(frames.len());
    let mut layout: Vec<([f64; 2], [u32; 2])> = Vec::with_capacity(frames.len());
    for chunk in frames.chunks(batch) {
        let images: Vec<Result<(Image, [f64; 2]), Diagnostic>> =
            chunk.par_iter().map(render).collect();
        for image in images {
            let (image, origin) = image?;
            layout.push((origin, [image.width, image.height]));
            rendered.push((image, origin));
        }
        // The union of the frames so far, times every frame of the strip.
        strip_size(&layout, frames.len())?;
    }
    let size = frame_size(&layout)?;
    let sheet = pack(&rendered, &layout, size);
    fs::create_dir_all(out_dir).map_err(|cause| {
        error(
            "E0603",
            format!("cannot create `{}`: {cause}", out_dir.display()),
        )
    })?;
    let image = out_dir.join(format!("{name}.png"));
    write_png(&sheet, &image).map_err(|message| error("E0603", message))?;
    let (tileset, map) = write_tiled(name, duration, looped, out_dir, rendered.len(), size)?;
    Ok(AnimateReport {
        image,
        tileset,
        map,
        frames: rendered.len(),
        frame_size: size,
    })
}

/// Size of a frame that holds every frame aligned on one origin, checked
/// against the strip limits before anything is packed.
fn frame_size(layout: &[([f64; 2], [u32; 2])]) -> Result<[u32; 2], Diagnostic> {
    let size = union_size(layout);
    check_strip(size, layout.len())?;
    Ok(size)
}

/// Check that `frames` frames as large as the union of `layout` fit a strip.
fn strip_size(layout: &[([f64; 2], [u32; 2])], frames: usize) -> Result<(), Diagnostic> {
    check_strip(union_size(layout), frames)
}

#[allow(clippy::cast_possible_truncation)]
fn union_size(layout: &[([f64; 2], [u32; 2])]) -> [u32; 2] {
    let mut min_x = f64::INFINITY;
    let mut min_y = f64::INFINITY;
    let mut max_x = f64::NEG_INFINITY;
    let mut max_y = f64::NEG_INFINITY;
    for (origin, size) in layout {
        min_x = min_x.min(origin[0]);
        min_y = min_y.min(origin[1]);
        max_x = max_x.max(origin[0] + f64::from(size[0]));
        max_y = max_y.max(origin[1] + f64::from(size[1]));
    }
    let side = |extent: f64| u32::try_from(extent.ceil() as isize).unwrap_or(1).max(1);
    [side(max_x - min_x), side(max_y - min_y)]
}

fn check_strip([frame_w, frame_h]: [u32; 2], frames: usize) -> Result<(), Diagnostic> {
    let sheet_w = u64::from(frame_w) * frames as u64;
    if sheet_w > u64::from(MAX_SHEET_SIDE)
        || frame_h > MAX_SHEET_SIDE
        || sheet_w * u64::from(frame_h) > MAX_SHEET_PIXELS
    {
        return Err(error(
            "E0602",
            format!("animation sheet {sheet_w}x{frame_h} exceeds the limit; use fewer frames"),
        ));
    }
    Ok(())
}

/// Lay the frames out left to right, aligned on their common origin.
#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
fn pack(
    rendered: &[(Image, [f64; 2])],
    layout: &[([f64; 2], [u32; 2])],
    [frame_w, frame_h]: [u32; 2],
) -> Image {
    let min_x = layout
        .iter()
        .map(|(origin, _)| origin[0])
        .fold(f64::INFINITY, f64::min);
    let min_y = layout
        .iter()
        .map(|(origin, _)| origin[1])
        .fold(f64::INFINITY, f64::min);
    // `frame_size` bounded the strip, so its width fits u32.
    let sheet_w = frame_w * rendered.len() as u32;
    let mut sheet = Image {
        width: sheet_w,
        height: frame_h,
        rgba: vec![0; (sheet_w as usize) * (frame_h as usize) * 4],
    };
    for (frame, (image, origin)) in rendered.iter().enumerate() {
        let left = frame as u32 * frame_w;
        let x = (origin[0] - min_x).round() as i64 + i64::from(left);
        let y = (origin[1] - min_y).round() as i64;
        composite_over(&mut sheet, image, x, y, [left, 0, frame_w, frame_h]);
    }
    sheet
}

/// Write the tileset (its first tile plays every frame) and a one-tile map.
#[allow(clippy::cast_possible_truncation)]
fn write_tiled(
    name: &str,
    duration: u32,
    looped: bool,
    out_dir: &Path,
    frames: usize,
    [frame_w, frame_h]: [u32; 2],
) -> Result<(PathBuf, PathBuf), Diagnostic> {
    let png_name = format!("{name}.png");
    let animation: Vec<_> = (0..frames)
        .map(|frame| json!({"tileid": frame, "duration": duration}))
        .collect();
    let tsj_name = format!("{name}.tsj");
    let tileset = out_dir.join(&tsj_name);
    write_json(
        &tileset,
        &json!({
            "type": "tileset",
            "name": name,
            "version": "1.10",
            "tiledversion": "1.12.2",
            "tilewidth": frame_w,
            "tileheight": frame_h,
            "tilecount": frames,
            "columns": frames,
            "image": png_name,
            "imagewidth": frame_w * frames as u32,
            "imageheight": frame_h,
            "margin": 0,
            "spacing": 0,
            // Tiled always loops; `loop` tells a game whether to stop.
            "tiles": [{
                "id": 0,
                "animation": animation,
                "properties": [{"name": "loop", "type": "bool", "value": looped}]
            }]
        }),
    )?;
    let map = out_dir.join("scene.tmj");
    write_json(
        &map,
        &json!({
            "type": "map",
            "version": "1.10",
            "tiledversion": "1.12.2",
            "orientation": "isometric",
            "renderorder": "right-down",
            "width": 1,
            "height": 1,
            "tilewidth": frame_w,
            "tileheight": frame_h,
            "infinite": false,
            "nextlayerid": 2,
            "nextobjectid": 1,
            "tilesets": [{"firstgid": 1, "source": tsj_name}],
            "layers": [{
                "type": "tilelayer",
                "name": "animation",
                "id": 1,
                "width": 1,
                "height": 1,
                "x": 0,
                "y": 0,
                "opacity": 1,
                "visible": true,
                "data": [1]
            }]
        }),
    )?;
    Ok((tileset, map))
}
