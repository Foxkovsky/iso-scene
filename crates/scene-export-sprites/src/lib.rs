//! Pack isometric object renders into a sprite sheet.
//!
//! The sheet is a uniform grid of cells. Every cell is anchored at its
//! bottom-centre, so a game can draw an object at the cell's bottom-centre and
//! get the same placement Tiled uses. Two sidecars describe the grid:
//!
//! - `sheet.json` — engine-neutral manifest (names, views, frames, timing);
//! - `sheet.tsj` — a Tiled tileset, including `animation` for frame rows.

#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss,
    clippy::too_many_arguments
)]

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use scene_backend_iso::{Sprite, project_point, render_object_sprite_at};
use scene_core::CompiledObject;
use scene_render::{Image, write_png};
use scene_syntax::Diagnostic;
use serde_json::{Value, json};

/// Maximum sheet width or height in pixels.
const MAX_SHEET_SIDE: u32 = 8_192;
/// Maximum sheet area in pixels.
const MAX_SHEET_PIXELS: u64 = 16_000_000;

/// One object that can appear in the sheet.
#[derive(Clone, Debug)]
pub struct SheetObject {
    pub name: String,
    pub object: Arc<CompiledObject>,
}

/// A rotation animation packed as one row of frames.
#[derive(Clone, Debug)]
pub struct AnimationSpec {
    /// Object to turn; must be one of the sheet objects.
    pub object: String,
    /// First angle in degrees.
    pub from: f64,
    /// Last angle in degrees.
    pub to: f64,
    /// Number of frames, at least one.
    pub frames: u32,
    /// Frame duration in milliseconds.
    pub duration: u32,
    /// Quarter turn applied before the animated angle.
    pub rotation: u16,
}

/// What to put on the sheet.
#[derive(Clone, Debug)]
pub struct SheetOptions {
    /// File stem for `name.png`, `name.json` and `name.tsj`.
    pub name: String,
    /// Pixels per block.
    pub scale: u32,
    /// Quarter turns for the static views, in order.
    pub views: Vec<u16>,
    /// Optional animation row.
    pub animation: Option<AnimationSpec>,
    /// Tile width used for the Tiled `tileoffset`, when known.
    pub tile_width: Option<u32>,
}

/// Files written by [`export`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SheetReport {
    pub image: PathBuf,
    pub manifest: PathBuf,
    pub tileset: PathBuf,
    pub cells: [u32; 2],
    pub columns: u32,
    pub rows: u32,
    /// Number of static object/view cells.
    pub sprites: usize,
    /// Number of animation frames.
    pub frames: usize,
}

struct Cell {
    row: u32,
    column: u32,
    name: String,
    /// Quarter turn used for the view.
    rotation: u16,
    /// Extra yaw used for animation frames.
    angle: f64,
    duration: Option<u32>,
    sprite: Sprite,
    /// Pixel that must sit on the cell anchor: the footprint's front corner for
    /// a static view, the object's local origin for an animation frame.
    reference: [f64; 2],
}

/// Render objects into `out_dir`.
///
/// Rows are objects (one row per object), columns are views; an animation, if
/// requested, gets its own row at the bottom. Empty cells stay transparent.
///
/// # Errors
///
/// Returns `E0602` for an unknown animation object, an empty view list, or a
/// sheet that exceeds the size limit, and `E0603` for file errors.
#[allow(clippy::too_many_lines)]
pub fn export(
    objects: &[SheetObject],
    materials: &std::collections::BTreeMap<String, PathBuf>,
    options: &SheetOptions,
    out_dir: &Path,
) -> Result<SheetReport, Diagnostic> {
    if objects.is_empty() {
        return Err(error("E0602", "no objects to pack"));
    }
    if options.views.is_empty() {
        return Err(error("E0602", "at least one view or angle is required"));
    }
    let animation_frames = match &options.animation {
        Some(animation) => {
            if animation.frames == 0 {
                return Err(error("E0602", "an animation needs at least one frame"));
            }
            if !objects.iter().any(|entry| entry.name == animation.object) {
                return Err(error(
                    "E0602",
                    format!("scene has no object `{}`", animation.object),
                ));
            }
            animation.frames as usize
        }
        None => 0,
    };
    let columns = options.views.len().max(animation_frames).max(1) as u32;
    let rows = objects.len() as u32 + u32::from(options.animation.is_some());

    let mut cells = Vec::new();
    for (row, entry) in objects.iter().enumerate() {
        for (column, rotation) in options.views.iter().enumerate() {
            let sprite =
                render_object_sprite_at(&entry.object, *rotation, 0.0, options.scale, materials)?;
            let reference = front_corner(&entry.object, *rotation, options.scale, &sprite);
            cells.push(Cell {
                row: row as u32,
                column: column as u32,
                name: entry.name.clone(),
                rotation: *rotation,
                angle: 0.0,
                duration: None,
                sprite,
                reference,
            });
        }
    }
    if let Some(animation) = &options.animation {
        let entry = objects
            .iter()
            .find(|entry| entry.name == animation.object)
            .ok_or_else(|| error("E0602", "internal animation lookup failed"))?;
        let row = objects.len() as u32;
        let angles = frame_angles(animation.from, animation.to, animation.frames);
        for (column, angle) in angles.into_iter().enumerate() {
            let sprite = render_object_sprite_at(
                &entry.object,
                animation.rotation,
                angle,
                options.scale,
                materials,
            )?;
            cells.push(Cell {
                row,
                column: column as u32,
                name: entry.name.clone(),
                rotation: animation.rotation,
                angle,
                duration: Some(animation.duration),
                reference: sprite.anchor,
                sprite,
            });
        }
    }

    let (cell_size, anchor) = cell_metrics(&cells);
    let width = columns
        .checked_mul(cell_size[0])
        .ok_or_else(|| error("E0602", "sprite sheet is too wide"))?;
    let height = rows
        .checked_mul(cell_size[1])
        .ok_or_else(|| error("E0602", "sprite sheet is too tall"))?;
    if width > MAX_SHEET_SIDE
        || height > MAX_SHEET_SIDE
        || u64::from(width) * u64::from(height) > MAX_SHEET_PIXELS
    {
        return Err(error(
            "E0602",
            format!("sprite sheet {width}x{height} exceeds the limit"),
        ));
    }

    fs::create_dir_all(out_dir).map_err(|cause| {
        error(
            "E0603",
            format!("cannot create `{}`: {cause}", out_dir.display()),
        )
    })?;

    let mut sheet = Image {
        width,
        height,
        rgba: vec![0; (width as usize) * (height as usize) * 4],
    };
    for cell in &cells {
        let x = f64::from(cell.column * cell_size[0]) + anchor[0] - cell.reference[0];
        let y = f64::from(cell.row * cell_size[1]) + anchor[1] - cell.reference[1];
        blit(&mut sheet, &cell.sprite.image, x, y);
    }

    let image_name = format!("{}.png", options.name);
    let image_path = out_dir.join(&image_name);
    write_png(&sheet, &image_path).map_err(|message| error("E0603", message))?;

    let manifest = manifest_json(options, cells.as_slice(), cell_size, anchor, columns, rows);
    let manifest_name = format!("{}.json", options.name);
    let manifest_path = out_dir.join(&manifest_name);
    write_json(&manifest_path, &manifest)?;

    let tileset = tileset_json(
        options,
        cells.as_slice(),
        cell_size,
        anchor,
        columns,
        rows,
        &image_name,
    );
    let tileset_name = format!("{}.tsj", options.name);
    let tileset_path = out_dir.join(&tileset_name);
    write_json(&tileset_path, &tileset)?;

    Ok(SheetReport {
        image: image_path,
        manifest: manifest_path,
        tileset: tileset_path,
        cells: cell_size,
        columns,
        rows,
        sprites: objects.len() * options.views.len(),
        frames: animation_frames,
    })
}

/// Pixel of the footprint's front corner, where the object meets the floor.
///
/// In the 2:1 projection the local origin of a footprint is its far (top)
/// corner, so a sprite anchored there hangs above the cell. The front corner is
/// the point a game places on the floor and Tiled aligns to the cell bottom.
fn front_corner(object: &CompiledObject, rotation: u16, scale: u32, sprite: &Sprite) -> [f64; 2] {
    let scale = f64::from(scale);
    let instance = scene_core::Instance {
        object: std::sync::Arc::new(object.clone()),
        at: [0.0, 0.0, 0.0],
        rotation,
        angle: 0.0,
    };
    let bounds = instance.world_bounds();
    let (origin_x, origin_y) =
        scene_core::rotate_xy(0.0, 0.0, object.width, object.depth, rotation);
    let origin = project_point([origin_x, origin_y, 0.0], scale);
    let front = project_point([bounds.max[0], bounds.max[1], bounds.min[2]], scale);
    [
        sprite.anchor[0] + front[0] - origin[0],
        sprite.anchor[1] + front[1] - origin[1],
    ]
}

/// Uniform cell size and the bottom-centre anchor inside a cell.
fn cell_metrics(cells: &[Cell]) -> ([u32; 2], [f64; 2]) {
    let mut left = 0.0_f64;
    let mut right = 0.0_f64;
    let mut top = 0.0_f64;
    let mut bottom = 0.0_f64;
    for cell in cells {
        let reference = cell.reference;
        left = left.max(reference[0]);
        right = right.max(f64::from(cell.sprite.image.width) - reference[0]);
        top = top.max(reference[1]);
        bottom = bottom.max(f64::from(cell.sprite.image.height) - reference[1]);
    }
    let half = left.max(right).ceil();
    let width = (half * 2.0).max(2.0) as u32;
    let height = (top.ceil() + bottom.ceil()).max(2.0) as u32;
    ([width, height], [f64::from(width) / 2.0, top.ceil()])
}

fn frame_angles(from: f64, to: f64, frames: u32) -> Vec<f64> {
    if frames <= 1 {
        return vec![from];
    }
    (0..frames)
        .map(|frame| from + (to - from) * f64::from(frame) / f64::from(frames - 1))
        .collect()
}

#[allow(clippy::too_many_arguments)]
fn manifest_json(
    options: &SheetOptions,
    cells: &[Cell],
    cell_size: [u32; 2],
    anchor: [f64; 2],
    columns: u32,
    rows: u32,
) -> Value {
    let sprites: Vec<Value> = cells
        .iter()
        .filter(|cell| cell.duration.is_none())
        .map(|cell| cell_json(cell, cell_size, anchor))
        .collect();
    let animations: Vec<Value> = options
        .animation
        .as_ref()
        .map(|animation| {
            let frames: Vec<Value> = cells
                .iter()
                .filter(|cell| cell.duration.is_some())
                .map(|cell| {
                    let mut frame = cell_json(cell, cell_size, anchor);
                    if let Some(object) = frame.as_object_mut() {
                        object.remove("view");
                        object.insert("index".to_string(), json!(cell.column));
                        object.insert("angle".to_string(), json!(cell.angle));
                        object.insert("duration".to_string(), json!(cell.duration));
                    }
                    frame
                })
                .collect();
            json!({
                "name": animation.object,
                "from": animation.from,
                "to": animation.to,
                "duration": animation.duration,
                "frames": frames
            })
        })
        .into_iter()
        .collect();

    json!({
        "image": format!("{}.png", options.name),
        "name": options.name,
        "cell": {"width": cell_size[0], "height": cell_size[1]},
        "anchor": {"x": anchor[0], "y": anchor[1]},
        "columns": columns,
        "rows": rows,
        "sprites": sprites,
        "animations": animations
    })
}

fn cell_json(cell: &Cell, cell_size: [u32; 2], anchor: [f64; 2]) -> Value {
    json!({
        "name": cell.name,
        "view": cell.column,
        "rotation": cell.rotation,
        "x": cell.column * cell_size[0],
        "y": cell.row * cell_size[1],
        "w": cell_size[0],
        "h": cell_size[1],
        "anchor": {"x": anchor[0], "y": anchor[1]}
    })
}

#[allow(clippy::too_many_arguments)]
fn tileset_json(
    options: &SheetOptions,
    cells: &[Cell],
    cell_size: [u32; 2],
    anchor: [f64; 2],
    columns: u32,
    rows: u32,
    image_name: &str,
) -> Value {
    let mut tiles: Vec<Value> = cells
        .iter()
        .filter(|cell| cell.duration.is_none())
        .map(|cell| {
            json!({
                "id": cell.row * columns + cell.column,
                "type": cell.name,
                "properties": [
                    {"name": "name", "type": "string", "value": cell.name},
                    {"name": "view", "type": "int", "value": cell.column},
                    {"name": "rotation", "type": "int", "value": cell.rotation},
                    {"name": "anchor", "type": "string", "value": "bottom-center"},
                    {"name": "anchor-x", "type": "float", "value": anchor[0]},
                    {"name": "anchor-y", "type": "float", "value": anchor[1]}
                ]
            })
        })
        .collect();

    if let Some(animation) = &options.animation {
        let frames: Vec<Value> = cells
            .iter()
            .filter(|cell| cell.duration.is_some())
            .map(|cell| {
                json!({
                    "tileid": cell.row * columns + cell.column,
                    "duration": cell.duration.unwrap_or(0)
                })
            })
            .collect();
        if let Some(first) = cells
            .iter()
            .find(|cell| cell.duration.is_some() && cell.column == 0)
        {
            tiles.push(json!({
                "id": first.row * columns + first.column,
                "type": animation.object,
                "properties": [
                    {"name": "name", "type": "string", "value": animation.object},
                    {"name": "anchor", "type": "string", "value": "bottom-center"},
                    {"name": "anchor-x", "type": "float", "value": anchor[0]},
                    {"name": "anchor-y", "type": "float", "value": anchor[1]}
                ],
                "animation": frames
            }));
        }
    }

    let offset_x = options.tile_width.map_or(0, |tile_width| {
        (i64::from(tile_width) - i64::from(cell_size[0])) / 2
    });
    // The anchor sits `cell_size[1] - anchor[1]` pixels above the cell bottom;
    // shift the image down by that much so the anchor lands on the tile cell.
    let offset_y = (f64::from(cell_size[1]) - anchor[1]).round() as i64;
    json!({
        "type": "tileset",
        "name": options.name,
        "version": "1.10",
        "tiledversion": "1.12.2",
        "tilewidth": cell_size[0],
        "tileheight": cell_size[1],
        "tilecount": columns * rows,
        "columns": columns,
        "image": image_name,
        "imagewidth": columns * cell_size[0],
        "imageheight": rows * cell_size[1],
        "margin": 0,
        "spacing": 0,
        "tileoffset": {"x": offset_x, "y": offset_y},
        "tiles": tiles
    })
}

fn blit(dst: &mut Image, src: &Image, x: f64, y: f64) {
    let x = x.round() as i64;
    let y = y.round() as i64;
    for sy in 0..src.height {
        let target_y = y + i64::from(sy);
        if target_y < 0 || target_y >= i64::from(dst.height) {
            continue;
        }
        for sx in 0..src.width {
            let target_x = x + i64::from(sx);
            if target_x < 0 || target_x >= i64::from(dst.width) {
                continue;
            }
            let source = ((sy * src.width + sx) * 4) as usize;
            let alpha = u32::from(src.rgba[source + 3]);
            if alpha == 0 {
                continue;
            }
            let target = ((target_y as u32 * dst.width + target_x as u32) * 4) as usize;
            if alpha == 255 {
                dst.rgba[target..target + 4].copy_from_slice(&src.rgba[source..source + 4]);
                continue;
            }
            let inverse = 255 - alpha;
            for channel in 0..3 {
                dst.rgba[target + channel] = ((u32::from(src.rgba[source + channel]) * alpha
                    + u32::from(dst.rgba[target + channel]) * inverse)
                    / 255) as u8;
            }
            dst.rgba[target + 3] = 255;
        }
    }
}

fn write_json(path: &Path, value: &Value) -> Result<(), Diagnostic> {
    let text = serde_json::to_string_pretty(value).map_err(|cause| {
        error(
            "E0603",
            format!("cannot encode `{}`: {cause}", path.display()),
        )
    })?;
    fs::write(path, format!("{text}\n")).map_err(|cause| {
        error(
            "E0603",
            format!("cannot write `{}`: {cause}", path.display()),
        )
    })
}

fn error(code: &'static str, message: impl Into<String>) -> Diagnostic {
    Diagnostic::new(code, message, scene_syntax::Span::new(0, 0))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    fn box_object(name: &str, width: f64, depth: f64) -> Arc<CompiledObject> {
        let program = scene_syntax::parse(&format!(
            "version 1;\nobject {name} ({width}, {depth}) {{\n    box at (0, 0, 0) size ({width}, {depth}, 1) material wood;\n}}\n"
        ))
        .expect("test model parses");
        let model =
            scene_core::compile_model(&program, Path::new(".")).expect("test model compiles");
        model.objects[name].clone()
    }

    #[test]
    fn pack_two_objects_with_four_views() {
        let objects = vec![
            SheetObject {
                name: "chair".into(),
                object: box_object("chair", 1.0, 1.0),
            },
            SheetObject {
                name: "desk".into(),
                object: box_object("desk", 2.0, 1.0),
            },
        ];
        let options = SheetOptions {
            name: "sheet".into(),
            scale: 8,
            views: vec![0, 90, 180, 270],
            animation: None,
            tile_width: Some(16),
        };
        let dir = std::env::temp_dir().join("iso-scene-sprites-test");
        let report = export(&objects, &BTreeMap::new(), &options, &dir).expect("sheet exports");
        assert_eq!(report.rows, 2);
        assert_eq!(report.columns, 4);
        assert_eq!(report.sprites, 8);
        assert_eq!(report.frames, 0);

        let manifest: Value =
            serde_json::from_str(&fs::read_to_string(&report.manifest).unwrap()).unwrap();
        assert_eq!(manifest["sprites"].as_array().unwrap().len(), 8);
        assert_eq!(manifest["cell"]["width"], report.cells[0]);
        assert_eq!(manifest["anchor"]["x"], f64::from(report.cells[0]) / 2.0);
        // A static view sits on the floor: the anchor is near the cell bottom.
        let anchor_y = manifest["anchor"]["y"].as_f64().unwrap();
        assert!(
            anchor_y > f64::from(report.cells[1]) - 20.0,
            "anchor {anchor_y} should be near the cell bottom {}",
            report.cells[1]
        );

        let tileset: Value =
            serde_json::from_str(&fs::read_to_string(&report.tileset).unwrap()).unwrap();
        assert_eq!(tileset["tilewidth"], report.cells[0]);
        assert_eq!(tileset["columns"], 4);
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn animation_gets_its_own_row_and_tiled_animation() {
        let objects = vec![SheetObject {
            name: "door".into(),
            object: box_object("door", 1.0, 1.0),
        }];
        let options = SheetOptions {
            name: "door".into(),
            scale: 8,
            views: vec![0, 180],
            animation: Some(AnimationSpec {
                object: "door".into(),
                from: 0.0,
                to: 90.0,
                frames: 5,
                duration: 100,
                rotation: 0,
            }),
            tile_width: None,
        };
        let dir = std::env::temp_dir().join("iso-scene-sprites-anim-test");
        let report = export(&objects, &BTreeMap::new(), &options, &dir).expect("sheet exports");
        assert_eq!(report.rows, 2);
        assert_eq!(report.columns, 5);
        assert_eq!(report.frames, 5);

        let manifest: Value =
            serde_json::from_str(&fs::read_to_string(&report.manifest).unwrap()).unwrap();
        assert_eq!(
            manifest["animations"][0]["frames"]
                .as_array()
                .unwrap()
                .len(),
            5
        );
        assert_eq!(manifest["animations"][0]["frames"][4]["angle"], 90.0);

        let tileset: Value =
            serde_json::from_str(&fs::read_to_string(&report.tileset).unwrap()).unwrap();
        let animated = tileset["tiles"]
            .as_array()
            .unwrap()
            .iter()
            .find(|tile| tile.get("animation").is_some())
            .expect("animated tile");
        assert_eq!(animated["animation"].as_array().unwrap().len(), 5);
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn a_single_view_stays_on_one_row() {
        let objects = vec![SheetObject {
            name: "chair".into(),
            object: box_object("chair", 1.0, 1.0),
        }];
        let options = SheetOptions {
            name: "one".into(),
            scale: 8,
            views: vec![0],
            animation: None,
            tile_width: None,
        };
        let dir = std::env::temp_dir().join("iso-scene-sprites-one-test");
        let report = export(&objects, &BTreeMap::new(), &options, &dir).expect("sheet exports");
        assert_eq!(report.rows, 1);
        assert_eq!(report.columns, 1);
        let _ = fs::remove_dir_all(dir);
    }
}
