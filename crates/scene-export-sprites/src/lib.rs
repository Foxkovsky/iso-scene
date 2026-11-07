//! Pack object renders into a sprite sheet: 2:1 isometric by default, or a
//! top, front or side orthographic view.
//!
//! The sheet is a uniform grid of cells sharing one anchor at the horizontal
//! centre of the cell. A static view puts the footprint's front corner (where
//! the object meets the floor) on the anchor; an animation frame puts the
//! object's local origin (the hinge it turns around) there instead. A game
//! draws a cell with its anchor on the object's position and gets the same
//! placement Tiled uses. Two sidecars describe the grid:
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

use scene_backend_iso::{Sprite, render_object_sprite_with};
use scene_backend_ortho::View;
use scene_core::CompiledObject;
use scene_render::{Image, composite_over, write_png};
use scene_syntax::Diagnostic;
use serde_json::{Value, json};

/// `format` written into the manifest.
pub const MANIFEST_FORMAT: &str = "iso-scene-sprites";
/// Manifest `version`; bumped on incompatible changes.
pub const MANIFEST_VERSION: u32 = 1;

/// Maximum sheet width or height in pixels.
pub const MAX_SHEET_SIDE: u32 = 8_192;
/// Maximum sheet area in pixels.
pub const MAX_SHEET_PIXELS: u64 = 16_000_000;
/// Maximum number of animation frames.
pub const MAX_FRAMES: u32 = 1_024;
/// Maximum number of cells (static views plus frames), checked before rendering.
pub const MAX_CELLS: usize = 4_096;
/// Largest Tiled tile width accepted for the `tileoffset`.
pub const MAX_TILE_WIDTH: u32 = MAX_SHEET_SIDE;

/// One object that can appear in the sheet.
#[derive(Clone, Debug)]
pub struct SheetObject {
    pub name: String,
    pub object: Arc<CompiledObject>,
    /// File the object came from, reported in the manifest when known.
    pub source: Option<String>,
}

/// A rotation animation packed as one row of frames.
#[derive(Clone, Debug)]
pub struct AnimationSpec {
    /// Name of the animation in the manifest; unique within the sheet.
    pub name: String,
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
    /// Per-frame durations in milliseconds; empty means `duration` for every
    /// frame, otherwise exactly one entry per frame.
    pub durations: Vec<u32>,
    /// Whether the animation repeats; `false` plays once and holds the last
    /// frame.
    pub looped: bool,
    /// Quarter turn applied before the animated angle.
    pub rotation: u16,
}

/// Point of the footprint that a static view puts on the cell anchor.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Pivot {
    /// Front (bottom) corner, where the object meets the floor nearest the
    /// viewer; matches a Tiled tile placed on the floor.
    #[default]
    Front,
    /// Back (top) corner.
    Back,
    /// Left corner.
    Left,
    /// Right corner.
    Right,
    /// Centre of the footprint, e.g. for inventory icons.
    Center,
}

impl Pivot {
    /// Name used on the command line and in the manifest.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::Front => "front",
            Self::Back => "back",
            Self::Left => "left",
            Self::Right => "right",
            Self::Center => "center",
        }
    }

    /// Parse a command-line name.
    #[must_use]
    pub fn parse(name: &str) -> Option<Self> {
        [
            Self::Front,
            Self::Back,
            Self::Left,
            Self::Right,
            Self::Center,
        ]
        .into_iter()
        .find(|pivot| pivot.name() == name)
    }
}

/// Camera used for every cell.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Projection {
    /// 2:1 isometric.
    #[default]
    Iso,
    /// Orthographic, looking down.
    Top,
    /// Orthographic, looking along +y.
    Front,
    /// Orthographic, looking along -x.
    Side,
}

impl Projection {
    /// Name used on the command line and in the manifest.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::Iso => "iso",
            Self::Top => "top",
            Self::Front => "front",
            Self::Side => "side",
        }
    }

    /// Parse a command-line name.
    #[must_use]
    pub fn parse(name: &str) -> Option<Self> {
        [Self::Iso, Self::Top, Self::Front, Self::Side]
            .into_iter()
            .find(|projection| projection.name() == name)
    }

    fn ortho(self) -> Option<View> {
        match self {
            Self::Iso => None,
            Self::Top => Some(View::Top),
            Self::Front => Some(View::Front),
            Self::Side => Some(View::Side),
        }
    }

    fn project(self, point: [f64; 3], scale: f64) -> [f64; 2] {
        match self.ortho() {
            None => scene_backend_iso::project_point(point, scale),
            Some(view) => scene_backend_ortho::project_point(view, point, scale),
        }
    }
}

/// What to put on the sheet.
#[derive(Clone, Debug)]
pub struct SheetOptions {
    /// File stem for `name.png`, `name.json` and `name.tsj`.
    pub name: String,
    /// Pixels per block.
    pub scale: u32,
    /// Camera for every cell.
    pub projection: Projection,
    /// Yaw of each static view in degrees, in order. Quarter turns are exact;
    /// other angles (e.g. 45° for eight directions) turn the object freely.
    pub views: Vec<f64>,
    /// Footprint point that static views put on the anchor.
    pub pivot: Pivot,
    /// Bake a soft contact shadow under every sprite (isometric only).
    pub shadow: bool,
    /// Transparent pixels around and between cells, so linear filtering or
    /// mipmaps do not bleed one sprite into its neighbour.
    pub padding: u32,
    /// Also write `name.debug.png`: cell borders, anchors and labels.
    pub debug_grid: bool,
    /// Also write `name.atlas.json` in the `TexturePacker` "JSON Hash" format
    /// read by Phaser and `PixiJS`.
    pub atlas: bool,
    /// Split the sheet into pages of at most this many rows, each with its
    /// own cell size; `None` keeps one page.
    pub max_rows: Option<u32>,
    /// Animation rows, one per entry, below the static views.
    pub animations: Vec<AnimationSpec>,
    /// Tile width used for the Tiled `tileoffset`, when known.
    pub tile_width: Option<u32>,
}

/// Files written by [`export`]. With several pages, the per-page fields
/// describe the first page and `pages` counts them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SheetReport {
    pub image: PathBuf,
    pub manifest: PathBuf,
    pub tileset: PathBuf,
    /// `name.debug.png`, when requested.
    pub debug: Option<PathBuf>,
    /// `name.atlas.json`, when requested.
    pub atlas: Option<PathBuf>,
    pub cells: [u32; 2],
    pub columns: u32,
    pub rows: u32,
    /// Number of pages (sheet images).
    pub pages: usize,
    /// Number of static object/view cells.
    pub sprites: usize,
    /// Number of animation frames.
    pub frames: usize,
}

/// One sheet image with its own uniform grid.
struct Page {
    /// File stem: the sheet name, or `name-<n>` when there are several pages.
    stem: String,
    cells: Vec<Cell>,
    cell_size: [u32; 2],
    anchor: [f64; 2],
    columns: u32,
    rows: u32,
    width: u32,
    height: u32,
}

impl Page {
    fn image_name(&self) -> String {
        format!("{}.png", self.stem)
    }
}

struct Cell {
    /// Row within the cell's page.
    row: u32,
    column: u32,
    name: String,
    /// Declared footprint `(width, depth)` in blocks.
    footprint: [f64; 2],
    source: Option<String>,
    /// Quarter turn used for the view.
    rotation: u16,
    /// Extra yaw: the rest of a free view angle, or the animated angle.
    angle: f64,
    /// Index into `SheetOptions::animations` for an animation frame.
    animation: Option<usize>,
    duration: Option<u32>,
    sprite: Sprite,
    /// Pixel that must sit on the cell anchor: the footprint pivot for a
    /// static view, the object's local origin for an animation frame.
    reference: [f64; 2],
}

/// Render objects into `out_dir`.
///
/// Rows are objects (one row per object), columns are views; every animation
/// gets its own row at the bottom. Empty cells stay transparent. With
/// `max_rows` the rows are split into pages `name-1.png`, `name-2.png`, …,
/// each sized for its own cells, with one tileset per page and one manifest.
///
/// # Errors
///
/// Returns `E0606` for invalid options (no objects or views, an invalid
/// scale, sheet name or view angle, an animation with an unknown object, a
/// duplicate name or the wrong number of durations), `E0602` for too many
/// cells or frames or a sheet that exceeds the size limit, and `E0603` for a
/// file error.
#[allow(clippy::too_many_lines)]
pub fn export(
    objects: &[SheetObject],
    materials: &std::collections::BTreeMap<String, PathBuf>,
    options: &SheetOptions,
    out_dir: &Path,
) -> Result<SheetReport, Diagnostic> {
    if objects.is_empty() {
        return Err(error("E0606", "no objects to pack"));
    }
    if options.views.is_empty() {
        return Err(error("E0606", "at least one view or angle is required"));
    }
    if !(1..=scene_core::MAX_SCALE).contains(&options.scale) {
        return Err(error(
            "E0606",
            format!("scale must be in 1..={}", scene_core::MAX_SCALE),
        ));
    }
    if options.views.iter().any(|view| !view.is_finite()) {
        return Err(error("E0606", "view angles must be finite"));
    }
    validate_name(&options.name).map_err(|message| error("E0606", message))?;
    let mut animation_frames = 0_usize;
    for (index, animation) in options.animations.iter().enumerate() {
        if animation.frames == 0 {
            return Err(error("E0606", "an animation needs at least one frame"));
        }
        if !animation.durations.is_empty() && animation.durations.len() != animation.frames as usize
        {
            return Err(error(
                "E0606",
                format!(
                    "animation `{}` has {} frames but {} durations",
                    animation.name,
                    animation.frames,
                    animation.durations.len()
                ),
            ));
        }
        if !objects.iter().any(|entry| entry.name == animation.object) {
            return Err(error(
                "E0606",
                format!("scene has no object `{}`", animation.object),
            ));
        }
        if options.animations[..index]
            .iter()
            .any(|other| other.name == animation.name)
        {
            return Err(error(
                "E0606",
                format!("animation name `{}` is used twice", animation.name),
            ));
        }
        animation_frames = animation_frames.saturating_add(animation.frames as usize);
    }
    if animation_frames > MAX_FRAMES as usize {
        return Err(error(
            "E0602",
            format!("{animation_frames} animation frames exceed the limit of {MAX_FRAMES}"),
        ));
    }
    let cell_count = objects
        .len()
        .saturating_mul(options.views.len())
        .saturating_add(animation_frames);
    if cell_count > MAX_CELLS {
        return Err(error(
            "E0602",
            format!("{cell_count} sprite cells exceed the limit of {MAX_CELLS}"),
        ));
    }
    if options.max_rows == Some(0) {
        return Err(error("E0606", "a page needs at least one row"));
    }

    let mut cells = Vec::new();
    for (row, entry) in objects.iter().enumerate() {
        for (column, view) in options.views.iter().enumerate() {
            let (rotation, angle) = split_view(*view);
            let sprite = render_cell(&entry.object, rotation, angle, options, materials)?;
            let reference = footprint_pivot(
                &entry.object,
                rotation,
                angle,
                options.scale,
                &sprite,
                options.pivot,
                options.projection,
            );
            cells.push(Cell {
                row: row as u32,
                column: column as u32,
                name: entry.name.clone(),
                footprint: [entry.object.width, entry.object.depth],
                source: entry.source.clone(),
                rotation,
                angle,
                animation: None,
                duration: None,
                sprite,
                reference,
            });
        }
    }
    for (index, animation) in options.animations.iter().enumerate() {
        let entry = objects
            .iter()
            .find(|entry| entry.name == animation.object)
            .ok_or_else(|| error("E0602", "internal animation lookup failed"))?;
        let row = (objects.len() + index) as u32;
        let angles = frame_angles(animation.from, animation.to, animation.frames);
        for (column, angle) in angles.into_iter().enumerate() {
            let sprite = render_cell(&entry.object, animation.rotation, angle, options, materials)?;
            let duration = animation
                .durations
                .get(column)
                .copied()
                .unwrap_or(animation.duration);
            cells.push(Cell {
                row,
                column: column as u32,
                name: entry.name.clone(),
                footprint: [entry.object.width, entry.object.depth],
                source: entry.source.clone(),
                rotation: animation.rotation,
                angle,
                animation: Some(index),
                duration: Some(duration),
                reference: sprite.anchor,
                sprite,
            });
        }
    }

    let total_rows = objects.len() + options.animations.len();
    let rows_per_page = options
        .max_rows
        .map_or(total_rows, |rows| rows as usize)
        .max(1);
    let page_count = total_rows.div_ceil(rows_per_page).max(1);
    let mut buckets: Vec<Vec<Cell>> = (0..page_count).map(|_| Vec::new()).collect();
    for mut cell in cells {
        let row = cell.row as usize;
        cell.row = (row % rows_per_page) as u32;
        if let Some(bucket) = buckets.get_mut(row / rows_per_page) {
            bucket.push(cell);
        }
    }
    // Size every page before writing anything.
    let mut pages = Vec::with_capacity(page_count);
    for (index, cells) in buckets.into_iter().enumerate() {
        let (cell_size, anchor) = cell_metrics(&cells);
        let columns = cells.iter().map(|cell| cell.column + 1).max().unwrap_or(1);
        let rows = cells.iter().map(|cell| cell.row + 1).max().unwrap_or(1);
        let side = |count: u32, size: u32| {
            u64::from(count) * u64::from(size) + (u64::from(count) + 1) * u64::from(options.padding)
        };
        let (wide, tall) = (side(columns, cell_size[0]), side(rows, cell_size[1]));
        if wide > u64::from(MAX_SHEET_SIDE)
            || tall > u64::from(MAX_SHEET_SIDE)
            || wide * tall > MAX_SHEET_PIXELS
        {
            return Err(error(
                "E0602",
                format!("sprite sheet {wide}x{tall} exceeds the limit; try `--max-rows`"),
            ));
        }
        let stem = if page_count == 1 {
            options.name.clone()
        } else {
            format!("{}-{}", options.name, index + 1)
        };
        // Both sides are at most MAX_SHEET_SIDE, so every pixel offset fits u32.
        pages.push(Page {
            stem,
            cells,
            cell_size,
            anchor,
            columns,
            rows,
            width: wide as u32,
            height: tall as u32,
        });
    }

    fs::create_dir_all(out_dir).map_err(|cause| {
        error(
            "E0603",
            format!("cannot create `{}`: {cause}", out_dir.display()),
        )
    })?;

    let mut images = Vec::new();
    let mut tilesets = Vec::new();
    let mut debugs = Vec::new();
    let mut atlases = Vec::new();
    for page in &pages {
        let mut sheet = Image {
            width: page.width,
            height: page.height,
            rgba: vec![0; (page.width as usize) * (page.height as usize) * 4],
        };
        for cell in &page.cells {
            let [left, top] = cell_origin(cell, page.cell_size, options.padding);
            let x = f64::from(left) + page.anchor[0] - cell.reference[0];
            let y = f64::from(top) + page.anchor[1] - cell.reference[1];
            composite_over(
                &mut sheet,
                &cell.sprite.image,
                x.round() as i64,
                y.round() as i64,
                [left, top, page.cell_size[0], page.cell_size[1]],
            );
        }
        let image_path = out_dir.join(page.image_name());
        write_png(&sheet, &image_path).map_err(|message| error("E0603", message))?;
        images.push(image_path);

        if options.debug_grid {
            let path = out_dir.join(format!("{}.debug.png", page.stem));
            let image = debug_grid(&sheet, page, options.padding);
            write_png(&image, &path).map_err(|message| error("E0603", message))?;
            debugs.push(path);
        }

        let tileset_path = out_dir.join(format!("{}.tsj", page.stem));
        write_json(
            &tileset_path,
            &tileset_json(options, page, TiledLayer::Tiles),
        )?;
        tilesets.push(tileset_path);
        let objects_path = out_dir.join(format!("{}.objects.tsj", page.stem));
        write_json(
            &objects_path,
            &tileset_json(options, page, TiledLayer::Objects),
        )?;

        if options.atlas {
            let path = out_dir.join(format!("{}.atlas.json", page.stem));
            write_json(&path, &atlas_json(options, page))?;
            atlases.push(path);
        }
    }

    let manifest_path = out_dir.join(format!("{}.json", options.name));
    write_json(&manifest_path, &manifest_json(options, &pages))?;

    let first = pages
        .first()
        .ok_or_else(|| error("E0602", "internal: no sheet page"))?;
    Ok(SheetReport {
        image: images
            .into_iter()
            .next()
            .ok_or_else(|| error("E0602", "internal: no sheet image"))?,
        manifest: manifest_path,
        tileset: tilesets
            .into_iter()
            .next()
            .ok_or_else(|| error("E0602", "internal: no tileset"))?,
        debug: debugs.into_iter().next(),
        atlas: atlases.into_iter().next(),
        cells: first.cell_size,
        columns: first.columns,
        rows: first.rows,
        pages: pages.len(),
        sprites: objects.len() * options.views.len(),
        frames: animation_frames,
    })
}

/// Pixel of the footprint point selected by `pivot`.
///
/// In the 2:1 projection the local origin of a footprint is its far (top)
/// corner, so a sprite anchored there hangs above the cell. The default front
/// corner is the point a game places on the floor and Tiled aligns to the cell
/// bottom.
///
/// The point comes from the declared footprint `(width, depth)` at floor
/// level, not from the geometry's bounds: a table top or a cornice that
/// overhangs the footprint must not lift the anchor into the air. When two
/// corners are equally extreme the pivot is the middle of that edge.
fn footprint_pivot(
    object: &CompiledObject,
    rotation: u16,
    angle: f64,
    scale: u32,
    sprite: &Sprite,
    pivot: Pivot,
    projection: Projection,
) -> [f64; 2] {
    let scale = f64::from(scale);
    let instance = scene_core::Instance {
        object: std::sync::Arc::new(object.clone()),
        at: [0.0, 0.0, 0.0],
        rotation,
        angle,
    };
    let origin = projection.project(instance.to_world([0.0, 0.0, 0.0]), scale);
    let corners: Vec<[f64; 2]> = [
        [0.0, 0.0],
        [object.width, 0.0],
        [0.0, object.depth],
        [object.width, object.depth],
    ]
    .into_iter()
    .map(|[x, y]| projection.project(instance.to_world([x, y, 0.0]), scale))
    .collect();
    // Score to maximise for each pivot; the centre averages every corner.
    let score = |point: &[f64; 2]| match pivot {
        Pivot::Front => point[1],
        Pivot::Back => -point[1],
        Pivot::Left => -point[0],
        Pivot::Right => point[0],
        Pivot::Center => 0.0,
    };
    let best = corners.iter().map(score).fold(f64::NEG_INFINITY, f64::max);
    let chosen: Vec<&[f64; 2]> = corners
        .iter()
        .filter(|point| score(point) >= best - 1e-6)
        .collect();
    let count = chosen.len().max(1) as f64;
    let point = [
        chosen.iter().map(|point| point[0]).sum::<f64>() / count,
        chosen.iter().map(|point| point[1]).sum::<f64>() / count,
    ];
    [
        sprite.anchor[0] + point[0] - origin[0],
        sprite.anchor[1] + point[1] - origin[1],
    ]
}

/// Render one cell with the sheet's projection; `Sprite::anchor` is the pixel
/// of the object's local origin at floor level.
fn render_cell(
    object: &CompiledObject,
    rotation: u16,
    angle: f64,
    options: &SheetOptions,
    materials: &std::collections::BTreeMap<String, PathBuf>,
) -> Result<Sprite, Diagnostic> {
    match options.projection.ortho() {
        None => render_object_sprite_with(
            object,
            rotation,
            angle,
            options.scale,
            materials,
            options.shadow,
        ),
        Some(view) => {
            let (image, anchor) = scene_backend_ortho::render_object_sprite(
                object,
                view,
                rotation,
                angle,
                options.scale,
                materials,
            )?;
            Ok(Sprite { image, anchor })
        }
    }
}

/// Split a view yaw into an exact quarter turn and the remaining degrees.
fn split_view(view: f64) -> (u16, f64) {
    let view = view.rem_euclid(360.0);
    let quarter = (view / 90.0).floor().clamp(0.0, 3.0);
    let rest = view - quarter * 90.0;
    // Snap float noise such as 89.999999 to the next exact quarter turn.
    if (90.0 - rest).abs() < 1e-9 {
        return ((((quarter as u16) + 1) % 4) * 90, 0.0);
    }
    ((quarter as u16) * 90, rest)
}

/// Degrees as a JSON number: an integer when whole, so quarter turns keep
/// their `0/90/180/270` form.
fn degrees_json(degrees: f64) -> Value {
    if degrees.fract() == 0.0 && degrees.abs() < 1e15 {
        json!(degrees as i64)
    } else {
        json!(degrees)
    }
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

/// Evenly spaced angles from `from` to `to`, both included; one frame shows
/// `from`.
#[must_use]
pub fn frame_angles(from: f64, to: f64, frames: u32) -> Vec<f64> {
    if frames <= 1 {
        return vec![from];
    }
    (0..frames)
        .map(|frame| from + (to - from) * f64::from(frame) / f64::from(frames - 1))
        .collect()
}

fn manifest_json(options: &SheetOptions, pages: &[Page]) -> Value {
    // Every cell with the page it lives on, in page order.
    let located = || {
        pages
            .iter()
            .enumerate()
            .flat_map(|(index, page)| page.cells.iter().map(move |cell| (index, page, cell)))
    };
    let describe = |index: usize, page: &Page, cell: &Cell| {
        let mut value = cell_json(cell, page.cell_size, page.anchor, options.padding);
        if let Some(object) = value.as_object_mut() {
            object.insert("page".to_string(), json!(index));
        }
        value
    };
    let sprites: Vec<Value> = located()
        .filter(|(_, _, cell)| cell.animation.is_none())
        .map(|(index, page, cell)| {
            let mut sprite = describe(index, page, cell);
            if let Some(object) = sprite.as_object_mut() {
                object.insert("pivot".to_string(), json!(options.pivot.name()));
            }
            sprite
        })
        .collect();
    let animations: Vec<Value> = options
        .animations
        .iter()
        .enumerate()
        .map(|(number, animation)| {
            let frames: Vec<Value> = located()
                .filter(|(_, _, cell)| cell.animation == Some(number))
                .map(|(index, page, cell)| {
                    let mut frame = describe(index, page, cell);
                    if let Some(object) = frame.as_object_mut() {
                        object.remove("view");
                        object.insert("index".to_string(), json!(cell.column));
                        object.insert("angle".to_string(), json!(cell.angle));
                        object.insert("duration".to_string(), json!(cell.duration));
                        object.insert("pivot".to_string(), json!("origin"));
                    }
                    frame
                })
                .collect();
            json!({
                "name": animation.name,
                "object": animation.object,
                "from": animation.from,
                "to": animation.to,
                "rotation": animation.rotation,
                "duration": animation.duration,
                "loop": animation.looped,
                "frames": frames
            })
        })
        .collect();
    let page_list: Vec<Value> = pages
        .iter()
        .map(|page| {
            json!({
                "image": page.image_name(),
                "imageWidth": page.width,
                "imageHeight": page.height,
                "tileset": format!("{}.tsj", page.stem),
                "objectTileset": format!("{}.objects.tsj", page.stem),
                "cell": {"width": page.cell_size[0], "height": page.cell_size[1]},
                "anchor": {"x": page.anchor[0], "y": page.anchor[1]},
                "columns": page.columns,
                "rows": page.rows
            })
        })
        .collect();
    // The top-level grid fields describe the first page, which is the whole
    // sheet unless `--max-rows` split it.
    let first = page_list.first().cloned().unwrap_or(Value::Null);

    json!({
        "format": MANIFEST_FORMAT,
        "version": MANIFEST_VERSION,
        "image": first["image"],
        "imageWidth": first["imageWidth"],
        "imageHeight": first["imageHeight"],
        "name": options.name,
        "projection": options.projection.name(),
        "scale": options.scale,
        "tileWidth": options.tile_width,
        "views": options.views.iter().map(|view| degrees_json(*view)).collect::<Vec<_>>(),
        "pivot": options.pivot.name(),
        "cell": first["cell"],
        "padding": options.padding,
        "shadow": options.shadow && options.projection == Projection::Iso,
        "anchor": first["anchor"],
        "columns": first["columns"],
        "rows": first["rows"],
        "pages": page_list,
        "sprites": sprites,
        "animations": animations
    })
}

/// The sheet on a light background with cell borders, anchor crosses and a
/// `name/view` (or `name#frame`) label in each cell, for checking that nothing
/// is clipped and that anchors line up.
fn debug_grid(sheet: &Image, page: &Page, padding: u32) -> Image {
    const BACKGROUND: [u8; 4] = [236, 236, 236, 255];
    const BORDER: [u8; 4] = [220, 40, 160, 255];
    const CROSS: [u8; 4] = [0, 150, 220, 255];
    const TEXT: [u8; 4] = [20, 20, 20, 255];
    let (cells, cell_size, anchor) = (&page.cells, page.cell_size, page.anchor);
    let mut image = Image {
        width: sheet.width,
        height: sheet.height,
        rgba: BACKGROUND.repeat(sheet.width as usize * sheet.height as usize),
    };
    composite_over(&mut image, sheet, 0, 0, [0, 0, sheet.width, sheet.height]);
    for cell in cells {
        let [left, top] = cell_origin(cell, cell_size, padding);
        let right = left + cell_size[0] - 1;
        let bottom = top + cell_size[1] - 1;
        for x in left..=right {
            put(&mut image, x, top, BORDER);
            put(&mut image, x, bottom, BORDER);
        }
        for y in top..=bottom {
            put(&mut image, left, y, BORDER);
            put(&mut image, right, y, BORDER);
        }
        let center = [
            left + anchor[0].floor() as u32,
            top + anchor[1].floor() as u32,
        ];
        for delta in 0..=4_u32 {
            put(&mut image, center[0] + delta, center[1], CROSS);
            put(
                &mut image,
                center[0].saturating_sub(delta),
                center[1],
                CROSS,
            );
            put(&mut image, center[0], center[1] + delta, CROSS);
            put(
                &mut image,
                center[0],
                center[1].saturating_sub(delta),
                CROSS,
            );
        }
        let label = match cell.animation {
            Some(_) => format!("{}#{}", cell.name, cell.column),
            None => format!("{}/{}", cell.name, cell.column),
        };
        draw_text(&mut image, left + 2, top + 2, right, &label, TEXT);
    }
    image
}

fn put(image: &mut Image, x: u32, y: u32, colour: [u8; 4]) {
    if x < image.width && y < image.height {
        let index = (y as usize * image.width as usize + x as usize) * 4;
        if let Some(pixel) = image.rgba.get_mut(index..index + 4) {
            pixel.copy_from_slice(&colour);
        }
    }
}

/// Draw `text` with a 3x5 pixel font, clipped at `right`. Unknown characters
/// become a question mark.
fn draw_text(image: &mut Image, x: u32, y: u32, right: u32, text: &str, colour: [u8; 4]) {
    let mut pen = x;
    for character in text.chars() {
        if pen + 3 > right {
            break;
        }
        let rows = glyph(character.to_ascii_lowercase());
        for (row, bits) in rows.iter().enumerate() {
            for column in 0..3_u32 {
                if bits & (0b100 >> column) != 0 {
                    put(image, pen + column, y + row as u32, colour);
                }
            }
        }
        pen += 4;
    }
}

/// Rows of a 3x5 glyph, most significant of the three bits on the left.
fn glyph(character: char) -> [u8; 5] {
    match character {
        'a' => [0b010, 0b101, 0b111, 0b101, 0b101],
        'b' => [0b110, 0b101, 0b110, 0b101, 0b110],
        'c' => [0b011, 0b100, 0b100, 0b100, 0b011],
        'd' => [0b110, 0b101, 0b101, 0b101, 0b110],
        'e' => [0b111, 0b100, 0b110, 0b100, 0b111],
        'f' => [0b111, 0b100, 0b110, 0b100, 0b100],
        'g' => [0b011, 0b100, 0b101, 0b101, 0b011],
        'h' => [0b101, 0b101, 0b111, 0b101, 0b101],
        'i' => [0b111, 0b010, 0b010, 0b010, 0b111],
        'j' => [0b001, 0b001, 0b001, 0b101, 0b010],
        'k' => [0b101, 0b101, 0b110, 0b101, 0b101],
        'l' => [0b100, 0b100, 0b100, 0b100, 0b111],
        'm' => [0b101, 0b111, 0b111, 0b101, 0b101],
        'n' => [0b110, 0b101, 0b101, 0b101, 0b101],
        'o' => [0b010, 0b101, 0b101, 0b101, 0b010],
        'p' => [0b110, 0b101, 0b110, 0b100, 0b100],
        'q' => [0b010, 0b101, 0b101, 0b110, 0b011],
        'r' => [0b110, 0b101, 0b110, 0b101, 0b101],
        's' => [0b011, 0b100, 0b010, 0b001, 0b110],
        't' => [0b111, 0b010, 0b010, 0b010, 0b010],
        'u' => [0b101, 0b101, 0b101, 0b101, 0b111],
        'v' => [0b101, 0b101, 0b101, 0b101, 0b010],
        'w' => [0b101, 0b101, 0b111, 0b111, 0b101],
        'x' => [0b101, 0b101, 0b010, 0b101, 0b101],
        'y' => [0b101, 0b101, 0b010, 0b010, 0b010],
        'z' => [0b111, 0b001, 0b010, 0b100, 0b111],
        '0' => [0b111, 0b101, 0b101, 0b101, 0b111],
        '1' => [0b010, 0b110, 0b010, 0b010, 0b111],
        '2' => [0b110, 0b001, 0b010, 0b100, 0b111],
        '3' => [0b110, 0b001, 0b010, 0b001, 0b110],
        '4' => [0b101, 0b101, 0b111, 0b001, 0b001],
        '5' => [0b111, 0b100, 0b110, 0b001, 0b110],
        '6' => [0b011, 0b100, 0b111, 0b101, 0b111],
        '7' => [0b111, 0b001, 0b010, 0b010, 0b010],
        '8' => [0b111, 0b101, 0b111, 0b101, 0b111],
        '9' => [0b111, 0b101, 0b111, 0b001, 0b110],
        '_' => [0b000, 0b000, 0b000, 0b000, 0b111],
        '-' => [0b000, 0b000, 0b111, 0b000, 0b000],
        '/' => [0b001, 0b001, 0b010, 0b100, 0b100],
        '#' => [0b101, 0b111, 0b101, 0b111, 0b101],
        '.' => [0b000, 0b000, 0b000, 0b000, 0b010],
        ' ' => [0; 5],
        _ => [0b111, 0b001, 0b010, 0b000, 0b010],
    }
}

/// Key of a cell in the atlas: `object/view` for a static view and
/// `animation#frame` for an animation frame.
fn atlas_key(options: &SheetOptions, cell: &Cell) -> String {
    match cell
        .animation
        .and_then(|index| options.animations.get(index))
    {
        Some(animation) => format!("{}#{}", animation.name, cell.column),
        None => format!("{}/{}", cell.name, cell.column),
    }
}

/// `TexturePacker` "JSON Hash" atlas, as loaded by Phaser (`load.atlas`) and
/// `PixiJS` (`Spritesheet`). `pivot` is the anchor as a fraction of the frame,
/// `animations` lists frame keys in order (`PixiJS`), and each frame carries
/// its `duration` like an Aseprite export.
fn atlas_json(options: &SheetOptions, page: &Page) -> Value {
    let (cells, cell_size, anchor) = (&page.cells, page.cell_size, page.anchor);
    let mut frames = serde_json::Map::new();
    for cell in cells {
        let [x, y] = cell_origin(cell, cell_size, options.padding);
        let mut frame = json!({
            "frame": {"x": x, "y": y, "w": cell_size[0], "h": cell_size[1]},
            "rotated": false,
            "trimmed": false,
            "spriteSourceSize": {"x": 0, "y": 0, "w": cell_size[0], "h": cell_size[1]},
            "sourceSize": {"w": cell_size[0], "h": cell_size[1]},
            "pivot": {
                "x": anchor[0] / f64::from(cell_size[0]),
                "y": anchor[1] / f64::from(cell_size[1])
            }
        });
        if let (Some(duration), Some(object)) = (cell.duration, frame.as_object_mut()) {
            object.insert("duration".to_string(), json!(duration));
        }
        frames.insert(atlas_key(options, cell), frame);
    }
    let mut animations = serde_json::Map::new();
    for (index, animation) in options.animations.iter().enumerate() {
        let keys: Vec<String> = cells
            .iter()
            .filter(|cell| cell.animation == Some(index))
            .map(|cell| atlas_key(options, cell))
            .collect();
        animations.insert(animation.name.clone(), json!(keys));
    }
    json!({
        "frames": frames,
        "animations": animations,
        "meta": {
            "app": "iso-scene",
            "version": env!("CARGO_PKG_VERSION"),
            "image": page.image_name(),
            "format": "RGBA8888",
            "size": {"w": page.width, "h": page.height},
            "scale": "1"
        }
    })
}

/// Top-left pixel of a cell in the sheet.
fn cell_origin(cell: &Cell, cell_size: [u32; 2], padding: u32) -> [u32; 2] {
    [
        padding + cell.column * (cell_size[0] + padding),
        padding + cell.row * (cell_size[1] + padding),
    ]
}

/// Yaw of a static view in degrees; for an animation frame only its quarter
/// turn (the animated part is reported as `angle`).
fn view_degrees(cell: &Cell) -> f64 {
    let quarter = f64::from(cell.rotation);
    if cell.animation.is_some() {
        quarter
    } else {
        quarter + cell.angle
    }
}

fn cell_json(cell: &Cell, cell_size: [u32; 2], anchor: [f64; 2], padding: u32) -> Value {
    let [x, y] = cell_origin(cell, cell_size, padding);
    let mut value = json!({
        "name": cell.name,
        "view": cell.column,
        "rotation": degrees_json(view_degrees(cell)),
        "x": x,
        "y": y,
        "w": cell_size[0],
        "h": cell_size[1],
        "anchor": {"x": anchor[0], "y": anchor[1]},
        "footprint": {"width": cell.footprint[0], "depth": cell.footprint[1]}
    });
    if let (Some(source), Some(object)) = (&cell.source, value.as_object_mut()) {
        object.insert("source".to_string(), json!(source));
    }
    value
}

/// Tiled layer a tileset is tuned for.
#[derive(Clone, Copy, PartialEq, Eq)]
enum TiledLayer {
    /// Tile layers draw an image from the left corner of its cell.
    Tiles,
    /// Tile objects on an isometric map are already centred on their
    /// position, and Tiled adds `tileoffset` on top.
    Objects,
}

fn tileset_json(options: &SheetOptions, page: &Page, layer: TiledLayer) -> Value {
    let (cells, cell_size, anchor) = (&page.cells, page.cell_size, page.anchor);
    let (columns, rows) = (page.columns, page.rows);
    let mut tiles: Vec<Value> = cells
        .iter()
        .filter(|cell| cell.animation.is_none())
        .map(|cell| {
            json!({
                "id": cell.row * columns + cell.column,
                "type": cell.name,
                "properties": [
                    {"name": "name", "type": "string", "value": cell.name},
                    {"name": "view", "type": "int", "value": cell.column},
                    {
                        "name": "rotation",
                        "type": if view_degrees(cell).fract() == 0.0 { "int" } else { "float" },
                        "value": degrees_json(view_degrees(cell))
                    },
                    {"name": "pivot", "type": "string", "value": options.pivot.name()},
                    {"name": "anchor-x", "type": "float", "value": anchor[0]},
                    {"name": "anchor-y", "type": "float", "value": anchor[1]}
                ]
            })
        })
        .collect();

    for (index, animation) in options.animations.iter().enumerate() {
        let frames: Vec<Value> = cells
            .iter()
            .filter(|cell| cell.animation == Some(index))
            .map(|cell| {
                json!({
                    "tileid": cell.row * columns + cell.column,
                    "duration": cell.duration.unwrap_or(0)
                })
            })
            .collect();
        if let Some(first) = cells
            .iter()
            .find(|cell| cell.animation == Some(index) && cell.column == 0)
        {
            // Tiled always loops tile animations; `loop` tells the game.
            tiles.push(json!({
                "id": first.row * columns + first.column,
                "type": animation.object,
                "properties": [
                    {"name": "name", "type": "string", "value": animation.name},
                    {"name": "object", "type": "string", "value": animation.object},
                    {"name": "loop", "type": "bool", "value": animation.looped},
                    {"name": "pivot", "type": "string", "value": "origin"},
                    {"name": "anchor-x", "type": "float", "value": anchor[0]},
                    {"name": "anchor-y", "type": "float", "value": anchor[1]}
                ],
                "animation": frames
            }));
        }
    }
    tiles.sort_by_key(|tile| tile["id"].as_u64().unwrap_or(0));

    // Checked with `tmxrasterizer`: in a tile layer the image's left edge sits
    // on the cell's left corner, so the anchor needs this shift to reach the
    // cell's bottom vertex; a bottom-aligned tile object needs none.
    let offset_x = match layer {
        TiledLayer::Tiles => options.tile_width.map_or(0, |tile_width| {
            (i64::from(tile_width) - i64::from(cell_size[0])) / 2
        }),
        TiledLayer::Objects => 0,
    };
    // The anchor sits `cell_size[1] - anchor[1]` pixels above the cell bottom;
    // shift the image down by that much so the anchor lands on the tile cell.
    let offset_y = (f64::from(cell_size[1]) - anchor[1]).round() as i64;
    json!({
        "type": "tileset",
        "name": page.stem,
        "version": "1.10",
        "tiledversion": "1.12.2",
        "tilewidth": cell_size[0],
        "tileheight": cell_size[1],
        "tilecount": columns * rows,
        "columns": columns,
        "image": page.image_name(),
        "imagewidth": page.width,
        "imageheight": page.height,
        "margin": options.padding,
        "spacing": options.padding,
        "tileoffset": {"x": offset_x, "y": offset_y},
        "objectalignment": "bottom",
        "tiles": tiles
    })
}

/// Check that `name` is a bare file stem: `name.png` must land in the output
/// directory and nowhere else.
///
/// # Errors
///
/// Returns a message for an empty name, a path separator, `..`, or a name
/// that already ends in one of the written extensions.
pub fn validate_name(name: &str) -> Result<(), String> {
    if name.is_empty() || name == "." || name == ".." {
        return Err(format!("invalid sheet name `{name}`"));
    }
    if name.contains(['/', '\\', '\0']) || name.contains("..") {
        return Err(format!(
            "sheet name `{name}` must be a file stem without `/`, `\\` or `..`"
        ));
    }
    let lower = name.to_ascii_lowercase();
    if [".png", ".json", ".tsj"]
        .iter()
        .any(|extension| lower.ends_with(extension))
    {
        return Err(format!(
            "sheet name `{name}` must not include an extension; `.png`, `.json` and `.tsj` are added"
        ));
    }
    Ok(())
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
                source: None,
            },
            SheetObject {
                name: "desk".into(),
                object: box_object("desk", 2.0, 1.0),
                source: None,
            },
        ];
        let options = SheetOptions {
            name: "sheet".into(),
            scale: 8,
            projection: Projection::Iso,
            views: vec![0.0, 90.0, 180.0, 270.0],
            pivot: Pivot::Front,
            shadow: true,
            padding: 0,
            debug_grid: false,
            atlas: false,
            max_rows: None,
            animations: Vec::new(),
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
            source: None,
        }];
        let options = SheetOptions {
            name: "door".into(),
            scale: 8,
            projection: Projection::Iso,
            views: vec![0.0, 180.0],
            pivot: Pivot::Front,
            shadow: true,
            padding: 0,
            debug_grid: false,
            atlas: false,
            max_rows: None,
            animations: vec![AnimationSpec {
                name: "door".into(),
                object: "door".into(),
                from: 0.0,
                to: 90.0,
                frames: 5,
                duration: 100,
                durations: Vec::new(),
                looped: true,
                rotation: 0,
            }],
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
            source: None,
        }];
        let options = SheetOptions {
            name: "one".into(),
            scale: 8,
            projection: Projection::Iso,
            views: vec![0.0],
            pivot: Pivot::Front,
            shadow: true,
            padding: 0,
            debug_grid: false,
            atlas: false,
            max_rows: None,
            animations: Vec::new(),
            tile_width: None,
        };
        let dir = std::env::temp_dir().join("iso-scene-sprites-one-test");
        let report = export(&objects, &BTreeMap::new(), &options, &dir).expect("sheet exports");
        assert_eq!(report.rows, 1);
        assert_eq!(report.columns, 1);
        let _ = fs::remove_dir_all(dir);
    }

    fn chair_options(name: &str) -> SheetOptions {
        SheetOptions {
            name: name.into(),
            scale: 8,
            projection: Projection::Iso,
            views: vec![0.0, 90.0, 180.0, 270.0],
            pivot: Pivot::Front,
            shadow: true,
            padding: 0,
            debug_grid: false,
            atlas: false,
            max_rows: None,
            animations: Vec::new(),
            tile_width: None,
        }
    }

    #[test]
    fn soft_shadows_stay_translucent() {
        let objects = vec![SheetObject {
            name: "chair".into(),
            object: box_object("chair", 2.0, 1.0),
            source: None,
        }];
        let dir = std::env::temp_dir().join("iso-scene-sprites-alpha-test");
        let report = export(&objects, &BTreeMap::new(), &chair_options("alpha"), &dir)
            .expect("sheet exports");
        let decoder = png::Decoder::new(std::io::BufReader::new(
            fs::File::open(&report.image).unwrap(),
        ));
        let mut reader = decoder.read_info().unwrap();
        let mut buffer = vec![0; reader.output_buffer_size()];
        reader.next_frame(&mut buffer).unwrap();
        assert!(
            buffer
                .chunks_exact(4)
                .any(|pixel| pixel[3] > 0 && pixel[3] < 255),
            "the baked contact shadow must keep partial alpha"
        );
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn half_pixel_references_do_not_bleed_into_the_next_cell() {
        let sprite = |reference_x: f64| Cell {
            row: 0,
            column: 0,
            name: "x".into(),
            footprint: [1.0, 1.0],
            source: None,
            rotation: 0,
            angle: 0.0,
            animation: None,
            duration: None,
            sprite: Sprite {
                image: Image {
                    width: 4,
                    height: 2,
                    rgba: [255, 0, 0, 255].repeat(8),
                },
                anchor: [reference_x, 2.0],
            },
            reference: [reference_x, 2.0],
        };
        // A cell of width 4 whose anchor is 2.0, with a sprite referenced at
        // 1.5: the sprite lands at x = 0.5, which rounds to 1 and would spill
        // one column into the neighbour without a clip.
        let mut sheet = Image {
            width: 8,
            height: 2,
            rgba: vec![0; 8 * 2 * 4],
        };
        let cell = sprite(1.5);
        let x = 2.0 - cell.reference[0];
        composite_over(
            &mut sheet,
            &cell.sprite.image,
            x.round() as i64,
            0,
            [0, 0, 4, 2],
        );
        assert_eq!(sheet.pixel(4, 0)[3], 0, "column 4 belongs to the next cell");
        assert_eq!(sheet.pixel(3, 0)[3], 255);
    }

    #[test]
    fn frame_and_cell_limits_are_checked_before_rendering() {
        let objects = vec![SheetObject {
            name: "door".into(),
            object: box_object("door", 1.0, 1.0),
            source: None,
        }];
        let mut options = chair_options("limits");
        options.animations = vec![AnimationSpec {
            name: "door".into(),
            object: "door".into(),
            from: 0.0,
            to: 90.0,
            frames: MAX_FRAMES + 1,
            duration: 100,
            durations: Vec::new(),
            looped: true,
            rotation: 0,
        }];
        let dir = std::env::temp_dir().join("iso-scene-sprites-limit-test");
        let error = export(&objects, &BTreeMap::new(), &options, &dir).unwrap_err();
        assert_eq!(error.code, "E0602");
        assert!(error.message.contains("frames"), "{}", error.message);

        let many: Vec<SheetObject> = (0..=MAX_CELLS / 4)
            .map(|index| SheetObject {
                name: format!("o{index}"),
                object: objects[0].object.clone(),
                source: None,
            })
            .collect();
        let error = export(&many, &BTreeMap::new(), &chair_options("limits"), &dir).unwrap_err();
        assert_eq!(error.code, "E0602");
        assert!(error.message.contains("cells"), "{}", error.message);
        assert!(!dir.exists(), "nothing is written when a limit fails");
    }

    #[test]
    fn sheet_names_are_bare_stems() {
        for bad in [
            "",
            ".",
            "..",
            "../evil",
            "a/b",
            "a\\b",
            "sheet.png",
            "x.JSON",
        ] {
            assert!(validate_name(bad).is_err(), "`{bad}` should be rejected");
        }
        for good in ["sheet", "furniture-4", "door_anim", "v1.2"] {
            assert!(validate_name(good).is_ok(), "`{good}` should be accepted");
        }
        let objects = vec![SheetObject {
            name: "chair".into(),
            object: box_object("chair", 1.0, 1.0),
            source: None,
        }];
        let dir = std::env::temp_dir().join("iso-scene-sprites-name-test");
        let error =
            export(&objects, &BTreeMap::new(), &chair_options("../evil"), &dir).unwrap_err();
        assert_eq!(error.code, "E0606");
    }

    #[test]
    fn an_overhang_does_not_lift_the_static_anchor() {
        let compile = |source: &str| {
            let program = scene_syntax::parse(source).expect("test model parses");
            scene_core::compile_model(&program, Path::new("."))
                .expect("test model compiles")
                .objects["table"]
                .clone()
        };
        let plain = compile(
            "version 1;\nobject table (1, 1) {\n    box at (0, 0, 0) size (1, 1, 1) material wood;\n}\n",
        );
        let overhang = compile(
            "version 1;\nobject table (1, 1) {\n    box at (0, 0, 0) size (1, 1, 1) material wood;\n    box at (-0.5, -0.5, 1) size (2, 2, 0.25) material wood;\n}\n",
        );
        for rotation in [0, 90, 180, 270] {
            let reference = |object: &CompiledObject| {
                let sprite = scene_backend_iso::render_object_sprite_at(
                    object,
                    rotation,
                    0.0,
                    8,
                    &BTreeMap::new(),
                )
                .unwrap();
                let corner = footprint_pivot(
                    object,
                    rotation,
                    0.0,
                    8,
                    &sprite,
                    Pivot::Front,
                    Projection::Iso,
                );
                // The anchor relative to the object's origin pixel.
                [corner[0] - sprite.anchor[0], corner[1] - sprite.anchor[1]]
            };
            assert_eq!(reference(&plain), reference(&overhang), "rot {rotation}");
        }
    }

    #[test]
    fn pivots_pick_footprint_corners_and_centre() {
        let object = box_object("desk", 2.0, 1.0);
        let sprite =
            scene_backend_iso::render_object_sprite_at(&object, 0, 0.0, 8, &BTreeMap::new())
                .unwrap();
        let at = |pivot| {
            let point = footprint_pivot(&object, 0, 0.0, 8, &sprite, pivot, Projection::Iso);
            [point[0] - sprite.anchor[0], point[1] - sprite.anchor[1]]
        };
        // Screen offsets from the origin corner at scale 8: x = (x - y) * 8,
        // y = (x + y) / 2 * 8.
        assert_eq!(at(Pivot::Back), [0.0, 0.0]);
        assert_eq!(at(Pivot::Front), [8.0, 12.0]);
        assert_eq!(at(Pivot::Right), [16.0, 8.0]);
        assert_eq!(at(Pivot::Left), [-8.0, 4.0]);
        assert_eq!(at(Pivot::Center), [4.0, 6.0]);
        assert_eq!(Pivot::parse("center"), Some(Pivot::Center));
        assert_eq!(Pivot::parse("middle"), None);
    }

    #[test]
    fn view_angles_split_into_quarter_turns_and_a_rest() {
        assert_eq!(split_view(0.0), (0, 0.0));
        assert_eq!(split_view(90.0), (90, 0.0));
        assert_eq!(split_view(135.0), (90, 45.0));
        assert_eq!(split_view(-90.0), (270, 0.0));
        assert_eq!(split_view(360.0), (0, 0.0));
        assert_eq!(split_view(89.999_999_999_9), (90, 0.0));
        assert_eq!(degrees_json(90.0), json!(90));
        assert_eq!(degrees_json(22.5), json!(22.5));
    }

    #[test]
    fn eight_directions_report_their_angles() {
        let objects = vec![SheetObject {
            name: "desk".into(),
            object: box_object("desk", 2.0, 1.0),
            source: None,
        }];
        let mut options = chair_options("eight");
        options.views = (0..8).map(|step| f64::from(step) * 45.0).collect();
        let dir = std::env::temp_dir().join("iso-scene-sprites-eight-test");
        let report = export(&objects, &BTreeMap::new(), &options, &dir).expect("sheet exports");
        assert_eq!(report.columns, 8);
        let manifest: Value =
            serde_json::from_str(&fs::read_to_string(&report.manifest).unwrap()).unwrap();
        let rotations: Vec<Value> = manifest["sprites"]
            .as_array()
            .unwrap()
            .iter()
            .map(|sprite| sprite["rotation"].clone())
            .collect();
        assert_eq!(rotations[1], json!(45));
        assert_eq!(rotations[2], json!(90));
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn shadow_can_be_turned_off() {
        let objects = vec![SheetObject {
            name: "chair".into(),
            object: box_object("chair", 2.0, 1.0),
            source: None,
        }];
        let mut options = chair_options("bare");
        options.shadow = false;
        let dir = std::env::temp_dir().join("iso-scene-sprites-bare-test");
        let report = export(&objects, &BTreeMap::new(), &options, &dir).expect("sheet exports");
        let decoder = png::Decoder::new(std::io::BufReader::new(
            fs::File::open(&report.image).unwrap(),
        ));
        let mut reader = decoder.read_info().unwrap();
        let mut buffer = vec![0; reader.output_buffer_size()];
        reader.next_frame(&mut buffer).unwrap();
        // Without the shadow only the opaque object (and no soft halo) remains.
        let soft = buffer
            .chunks_exact(4)
            .filter(|pixel| pixel[3] > 0 && pixel[3] < 255)
            .count();
        assert_eq!(soft, 0);
        let _ = fs::remove_dir_all(dir);
    }

    fn door_animation(name: &str, frames: u32) -> AnimationSpec {
        AnimationSpec {
            name: name.into(),
            object: "door".into(),
            from: 0.0,
            to: 90.0,
            frames,
            duration: 100,
            durations: Vec::new(),
            looped: true,
            rotation: 0,
        }
    }

    #[test]
    fn several_animations_get_one_row_each_with_timing_and_loop() {
        let objects = vec![SheetObject {
            name: "door".into(),
            object: box_object("door", 1.0, 1.0),
            source: None,
        }];
        let mut options = chair_options("doors");
        let mut close = door_animation("close", 3);
        close.from = 90.0;
        close.to = 0.0;
        close.durations = vec![50, 60, 400];
        close.looped = false;
        options.animations = vec![door_animation("open", 5), close];
        let dir = std::env::temp_dir().join("iso-scene-sprites-many-anim-test");
        let report = export(&objects, &BTreeMap::new(), &options, &dir).expect("sheet exports");
        assert_eq!(report.rows, 3);
        assert_eq!(report.columns, 5);
        assert_eq!(report.frames, 8);

        let manifest: Value =
            serde_json::from_str(&fs::read_to_string(&report.manifest).unwrap()).unwrap();
        let close = &manifest["animations"][1];
        assert_eq!(close["name"], "close");
        assert_eq!(close["object"], "door");
        assert_eq!(close["loop"], false);
        assert_eq!(close["frames"][2]["duration"], 400);
        assert_eq!(close["frames"][0]["y"], 2 * report.cells[1]);

        let tileset: Value =
            serde_json::from_str(&fs::read_to_string(&report.tileset).unwrap()).unwrap();
        let animated: Vec<&Value> = tileset["tiles"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|tile| tile.get("animation").is_some())
            .collect();
        assert_eq!(animated.len(), 2);
        assert_eq!(animated[1]["animation"][2]["duration"], 400);
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn invalid_animations_are_rejected() {
        let objects = vec![SheetObject {
            name: "door".into(),
            object: box_object("door", 1.0, 1.0),
            source: None,
        }];
        let dir = std::env::temp_dir().join("iso-scene-sprites-bad-anim-test");
        let mut options = chair_options("bad");
        options.animations = vec![door_animation("a", 3), door_animation("a", 3)];
        let error = export(&objects, &BTreeMap::new(), &options, &dir).unwrap_err();
        assert_eq!(error.code, "E0606");
        assert!(error.message.contains("used twice"), "{}", error.message);
        let mut timed = door_animation("b", 3);
        timed.durations = vec![10, 20];
        options.animations = vec![timed];
        let error = export(&objects, &BTreeMap::new(), &options, &dir).unwrap_err();
        assert!(error.message.contains("durations"), "{}", error.message);
        assert!(!dir.exists());
    }

    #[test]
    fn padding_separates_cells_and_matches_the_tileset() {
        let objects = vec![
            SheetObject {
                name: "a".into(),
                object: box_object("a", 1.0, 1.0),
                source: None,
            },
            SheetObject {
                name: "b".into(),
                object: box_object("b", 1.0, 1.0),
                source: None,
            },
        ];
        let mut options = chair_options("padded");
        options.views = vec![0.0, 90.0];
        options.padding = 3;
        let dir = std::env::temp_dir().join("iso-scene-sprites-padding-test");
        let report = export(&objects, &BTreeMap::new(), &options, &dir).expect("sheet exports");
        let [w, h] = report.cells;
        let manifest: Value =
            serde_json::from_str(&fs::read_to_string(&report.manifest).unwrap()).unwrap();
        assert_eq!(manifest["sprites"][0]["x"], 3);
        assert_eq!(manifest["sprites"][1]["x"], 3 + w + 3);
        assert_eq!(manifest["sprites"][2]["y"], 3 + h + 3);
        let tileset: Value =
            serde_json::from_str(&fs::read_to_string(&report.tileset).unwrap()).unwrap();
        assert_eq!(tileset["margin"], 3);
        assert_eq!(tileset["spacing"], 3);
        assert_eq!(tileset["imagewidth"], 2 * w + 9);
        let decoder = png::Decoder::new(std::io::BufReader::new(
            fs::File::open(&report.image).unwrap(),
        ));
        let reader = decoder.read_info().unwrap();
        assert_eq!(reader.info().width, 2 * w + 9);
        assert_eq!(reader.info().height, 2 * h + 9);
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn manifest_describes_format_scale_and_footprints() {
        let objects = vec![SheetObject {
            name: "desk".into(),
            object: box_object("desk", 2.0, 1.0),
            source: Some("desk.model".into()),
        }];
        let mut options = chair_options("meta");
        options.tile_width = Some(16);
        let dir = std::env::temp_dir().join("iso-scene-sprites-meta-test");
        let report = export(&objects, &BTreeMap::new(), &options, &dir).expect("sheet exports");
        let manifest: Value =
            serde_json::from_str(&fs::read_to_string(&report.manifest).unwrap()).unwrap();
        assert_eq!(manifest["format"], MANIFEST_FORMAT);
        assert_eq!(manifest["version"], MANIFEST_VERSION);
        assert_eq!(manifest["scale"], 8);
        assert_eq!(manifest["tileWidth"], 16);
        assert_eq!(manifest["views"], json!([0, 90, 180, 270]));
        assert_eq!(manifest["imageWidth"], 4 * report.cells[0]);
        assert_eq!(manifest["imageHeight"], report.cells[1]);
        let sprite = &manifest["sprites"][0];
        assert_eq!(sprite["footprint"], json!({"width": 2.0, "depth": 1.0}));
        assert_eq!(sprite["source"], "desk.model");
        let _ = fs::remove_dir_all(dir);
    }

    /// Keys of `value` must be described by `schema` (directly or through
    /// `allOf`), and every required key must be present.
    fn check_against(schema: &Value, defs: &Value, value: &Value, path: &str) {
        let mut properties = serde_json::Map::new();
        let mut required = Vec::new();
        let mut parts = vec![schema.clone()];
        if let Some(all) = schema["allOf"].as_array() {
            for reference in all {
                let name = reference["$ref"]
                    .as_str()
                    .unwrap()
                    .trim_start_matches("#/$defs/");
                parts.push(defs[name].clone());
            }
        }
        for part in &parts {
            if let Some(map) = part["properties"].as_object() {
                properties.extend(map.clone());
            }
            if let Some(list) = part["required"].as_array() {
                required.extend(list.iter().map(|key| key.as_str().unwrap().to_string()));
            }
        }
        let object = value.as_object().unwrap();
        for key in object.keys() {
            assert!(
                properties.contains_key(key),
                "{path}.{key} is not in the schema"
            );
        }
        for key in required {
            assert!(
                object.contains_key(&key),
                "{path}.{key} is required by the schema"
            );
        }
    }

    #[test]
    fn manifest_matches_the_published_schema() {
        let schema: Value =
            serde_json::from_str(include_str!("../../../docs/sprite-sheet.schema.json")).unwrap();
        let defs = &schema["$defs"];
        let objects = vec![SheetObject {
            name: "door".into(),
            object: box_object("door", 1.0, 1.0),
            source: Some("door.model".into()),
        }];
        let mut options = chair_options("schema");
        options.animations = vec![door_animation("open", 2)];
        let dir = std::env::temp_dir().join("iso-scene-sprites-schema-test");
        let report = export(&objects, &BTreeMap::new(), &options, &dir).expect("sheet exports");
        let manifest: Value =
            serde_json::from_str(&fs::read_to_string(&report.manifest).unwrap()).unwrap();
        check_against(&schema, defs, &manifest, "sheet");
        assert_eq!(manifest["format"], schema["properties"]["format"]["const"]);
        assert_eq!(
            manifest["version"],
            schema["properties"]["version"]["const"]
        );
        for sprite in manifest["sprites"].as_array().unwrap() {
            check_against(&defs["sprite"], defs, sprite, "sprite");
        }
        for animation in manifest["animations"].as_array().unwrap() {
            check_against(&defs["animation"], defs, animation, "animation");
            for frame in animation["frames"].as_array().unwrap() {
                check_against(&defs["frame"], defs, frame, "frame");
            }
        }
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn debug_grid_draws_borders_on_an_opaque_copy() {
        let objects = vec![SheetObject {
            name: "chair".into(),
            object: box_object("chair", 1.0, 1.0),
            source: None,
        }];
        let mut options = chair_options("debug");
        options.debug_grid = true;
        let dir = std::env::temp_dir().join("iso-scene-sprites-debug-test");
        let report = export(&objects, &BTreeMap::new(), &options, &dir).expect("sheet exports");
        let path = report.debug.expect("debug image written");
        let decoder = png::Decoder::new(std::io::BufReader::new(fs::File::open(&path).unwrap()));
        let mut reader = decoder.read_info().unwrap();
        let mut buffer = vec![0; reader.output_buffer_size()];
        reader.next_frame(&mut buffer).unwrap();
        assert!(buffer.chunks_exact(4).all(|pixel| pixel[3] == 255));
        // The top-left pixel is a cell border.
        assert_eq!(&buffer[..4], &[220, 40, 160, 255]);
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn atlas_lists_frames_pivots_and_animations() {
        let objects = vec![SheetObject {
            name: "door".into(),
            object: box_object("door", 1.0, 1.0),
            source: None,
        }];
        let mut options = chair_options("atlas");
        options.views = vec![0.0, 90.0];
        options.padding = 2;
        options.atlas = true;
        options.animations = vec![door_animation("open", 3)];
        let dir = std::env::temp_dir().join("iso-scene-sprites-atlas-test");
        let report = export(&objects, &BTreeMap::new(), &options, &dir).expect("sheet exports");
        let atlas: Value =
            serde_json::from_str(&fs::read_to_string(report.atlas.unwrap()).unwrap()).unwrap();
        let [w, h] = report.cells;
        assert_eq!(atlas["frames"].as_object().unwrap().len(), 5);
        assert_eq!(atlas["frames"]["door/1"]["frame"]["x"], 2 + w + 2);
        assert_eq!(atlas["frames"]["open#2"]["frame"]["y"], 2 + h + 2);
        assert_eq!(atlas["frames"]["open#2"]["duration"], 100);
        let pivot = atlas["frames"]["door/0"]["pivot"]["x"].as_f64().unwrap();
        assert!((pivot - 0.5).abs() < 1e-9);
        assert_eq!(
            atlas["animations"]["open"],
            json!(["open#0", "open#1", "open#2"])
        );
        assert_eq!(atlas["meta"]["image"], "atlas.png");
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn max_rows_splits_pages_with_their_own_cell_size() {
        let objects = vec![
            SheetObject {
                name: "cup".into(),
                object: box_object("cup", 1.0, 1.0),
                source: None,
            },
            SheetObject {
                name: "bed".into(),
                object: box_object("bed", 4.0, 3.0),
                source: None,
            },
        ];
        let mut options = chair_options("paged");
        options.max_rows = Some(1);
        options.atlas = true;
        let dir = std::env::temp_dir().join("iso-scene-sprites-pages-test");
        let report = export(&objects, &BTreeMap::new(), &options, &dir).expect("sheet exports");
        assert_eq!(report.pages, 2);
        assert!(dir.join("paged-1.png").exists());
        assert!(dir.join("paged-2.png").exists());
        assert!(dir.join("paged-2.tsj").exists());
        assert!(dir.join("paged-2.atlas.json").exists());
        assert!(!dir.join("paged.png").exists());
        let manifest: Value =
            serde_json::from_str(&fs::read_to_string(&report.manifest).unwrap()).unwrap();
        let pages = manifest["pages"].as_array().unwrap();
        assert_eq!(pages.len(), 2);
        let small = pages[0]["cell"]["width"].as_u64().unwrap();
        let large = pages[1]["cell"]["width"].as_u64().unwrap();
        assert!(
            small < large,
            "the cup page should use smaller cells: {small} vs {large}"
        );
        let bed = &manifest["sprites"][4];
        assert_eq!(bed["name"], "bed");
        assert_eq!(bed["page"], 1);
        assert_eq!(bed["y"], 0);
        let _ = fs::remove_dir_all(dir);
    }

    fn read_png(path: &Path) -> (u32, u32, Vec<u8>) {
        let decoder = png::Decoder::new(std::io::BufReader::new(fs::File::open(path).unwrap()));
        let mut reader = decoder.read_info().unwrap();
        let mut buffer = vec![0; reader.output_buffer_size()];
        let info = reader.next_frame(&mut buffer).unwrap();
        (info.width, info.height, buffer)
    }

    fn opaque(rgba: &[u8]) -> usize {
        rgba.chunks_exact(4).filter(|pixel| pixel[3] > 0).count()
    }

    #[test]
    fn every_sprite_is_packed_whole_without_overlap() {
        let shapes = [("a", 1.0, 1.0), ("b", 3.0, 1.0), ("c", 1.0, 2.5)];
        let objects: Vec<SheetObject> = shapes
            .iter()
            .map(|(name, width, depth)| SheetObject {
                name: (*name).into(),
                object: box_object(name, *width, *depth),
                source: None,
            })
            .collect();
        let mut options = chair_options("whole");
        options.views = vec![0.0, 45.0, 90.0, 180.0, 270.0];
        options.padding = 1;
        options.animations = vec![AnimationSpec {
            object: "b".into(),
            ..door_animation("swing", 4)
        }];
        let dir = std::env::temp_dir().join("iso-scene-sprites-whole-test");
        let report = export(&objects, &BTreeMap::new(), &options, &dir).expect("sheet exports");
        let (_, _, sheet) = read_png(&report.image);

        // Composited pixels never merge (no overlap) and are never cut (no
        // clipping), so the sheet holds exactly the sum of all sprites.
        let mut expected = 0;
        for entry in &objects {
            for view in &options.views {
                let (rotation, angle) = split_view(*view);
                let sprite = render_object_sprite_with(
                    &entry.object,
                    rotation,
                    angle,
                    8,
                    &BTreeMap::new(),
                    true,
                )
                .unwrap();
                expected += opaque(&sprite.image.rgba);
            }
        }
        for angle in frame_angles(0.0, 90.0, 4) {
            let sprite =
                render_object_sprite_with(&objects[1].object, 0, angle, 8, &BTreeMap::new(), true)
                    .unwrap();
            expected += opaque(&sprite.image.rgba);
        }
        assert_eq!(opaque(&sheet), expected);
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn static_views_stand_on_the_anchor_line() {
        let objects = vec![
            SheetObject {
                name: "a".into(),
                object: box_object("a", 1.0, 1.0),
                source: None,
            },
            SheetObject {
                name: "b".into(),
                object: box_object("b", 3.0, 1.0),
                source: None,
            },
        ];
        let mut options = chair_options("floor");
        options.shadow = false;
        let dir = std::env::temp_dir().join("iso-scene-sprites-floor-test");
        let report = export(&objects, &BTreeMap::new(), &options, &dir).expect("sheet exports");
        let (width, _, sheet) = read_png(&report.image);
        let manifest: Value =
            serde_json::from_str(&fs::read_to_string(&report.manifest).unwrap()).unwrap();
        for sprite in manifest["sprites"].as_array().unwrap() {
            let [x, y, w, h] = ["x", "y", "w", "h"].map(|key| sprite[key].as_u64().unwrap());
            let anchor_y = sprite["anchor"]["y"].as_f64().unwrap();
            let lowest = (y..y + h)
                .filter(|row| {
                    (x..x + w).any(|column| {
                        sheet[((row * u64::from(width) + column) * 4 + 3) as usize] > 0
                    })
                })
                .max()
                .unwrap();
            // A box's front corner is its lowest point: it sits on the anchor
            // line (allowing for the 1 px outline and rounding).
            let offset = (lowest - y) as f64 - anchor_y;
            assert!(
                offset.abs() <= 2.0,
                "{}: lowest row {offset} px from the anchor",
                sprite["name"]
            );
        }
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn tileset_ids_and_sizes_are_consistent() {
        let objects = vec![SheetObject {
            name: "door".into(),
            object: box_object("door", 1.0, 1.0),
            source: None,
        }];
        let mut options = chair_options("tsj");
        options.padding = 2;
        options.animations = vec![door_animation("open", 7), door_animation("shut", 2)];
        options.max_rows = Some(2);
        let dir = std::env::temp_dir().join("iso-scene-sprites-tsj-test");
        let report = export(&objects, &BTreeMap::new(), &options, &dir).expect("sheet exports");
        for page in 1..=report.pages {
            let stem = format!("tsj-{page}");
            let tileset: Value =
                serde_json::from_str(&fs::read_to_string(dir.join(format!("{stem}.tsj"))).unwrap())
                    .unwrap();
            let (width, height, _) = read_png(&dir.join(format!("{stem}.png")));
            assert_eq!(tileset["imagewidth"], width);
            assert_eq!(tileset["imageheight"], height);
            let [count, columns, margin, spacing, tile_w, tile_h] = [
                "tilecount",
                "columns",
                "margin",
                "spacing",
                "tilewidth",
                "tileheight",
            ]
            .map(|key| tileset[key].as_u64().unwrap());
            // Tiled derives the grid from the image the same way.
            assert_eq!(
                columns,
                (u64::from(width) - 2 * margin + spacing) / (tile_w + spacing)
            );
            let rows = (u64::from(height) - 2 * margin + spacing) / (tile_h + spacing);
            assert_eq!(count, columns * rows);
            for tile in tileset["tiles"].as_array().unwrap() {
                assert!(tile["id"].as_u64().unwrap() < count);
                for frame in tile["animation"].as_array().into_iter().flatten() {
                    assert!(frame["tileid"].as_u64().unwrap() < count);
                }
            }
        }
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn orthographic_sheets_anchor_on_the_footprint() {
        let objects = vec![SheetObject {
            name: "desk".into(),
            object: box_object("desk", 2.0, 1.0),
            source: None,
        }];
        for projection in [Projection::Top, Projection::Front, Projection::Side] {
            let mut options = chair_options(projection.name());
            options.projection = projection;
            options.shadow = false;
            let dir = std::env::temp_dir().join(format!(
                "iso-scene-sprites-ortho-{}-test",
                projection.name()
            ));
            let report = export(&objects, &BTreeMap::new(), &options, &dir).expect("sheet exports");
            let manifest: Value =
                serde_json::from_str(&fs::read_to_string(&report.manifest).unwrap()).unwrap();
            assert_eq!(manifest["projection"], projection.name());
            assert_eq!(manifest["shadow"], false);
            let (width, _, sheet) = read_png(&report.image);
            let sprite = &manifest["sprites"][0];
            let [x, y, w, h] = ["x", "y", "w", "h"].map(|key| sprite[key].as_u64().unwrap());
            let anchor_y = sprite["anchor"]["y"].as_f64().unwrap();
            let lowest = (y..y + h)
                .filter(|row| {
                    (x..x + w).any(|column| {
                        sheet[((row * u64::from(width) + column) * 4 + 3) as usize] > 0
                    })
                })
                .max()
                .unwrap();
            // In every orthographic view the front pivot is the bottom edge.
            let offset = (lowest - y) as f64 - anchor_y;
            assert!(offset.abs() <= 2.0, "{projection:?}: {offset}");
            let _ = fs::remove_dir_all(dir);
        }
    }

    #[test]
    fn object_layers_get_a_tileset_without_the_horizontal_shift() {
        let objects = vec![SheetObject {
            name: "desk".into(),
            object: box_object("desk", 2.0, 1.0),
            source: None,
        }];
        let mut options = chair_options("layers");
        options.tile_width = Some(64);
        let dir = std::env::temp_dir().join("iso-scene-sprites-layers-test");
        let report = export(&objects, &BTreeMap::new(), &options, &dir).expect("sheet exports");
        let read = |name: &str| -> Value {
            serde_json::from_str(&fs::read_to_string(dir.join(name)).unwrap()).unwrap()
        };
        let tiles = read("layers.tsj");
        let objects = read("layers.objects.tsj");
        let cell_width = i64::from(report.cells[0]);
        // Verified in Tiled with tmxrasterizer on an isometric map: a tile
        // layer needs the shift, a bottom-aligned tile object does not.
        assert_eq!(tiles["tileoffset"]["x"], (64 - cell_width) / 2);
        assert_eq!(objects["tileoffset"]["x"], 0);
        assert_eq!(objects["tileoffset"]["y"], tiles["tileoffset"]["y"]);
        assert_eq!(objects["objectalignment"], "bottom");
        assert_eq!(
            read("layers.json")["pages"][0]["objectTileset"],
            "layers.objects.tsj"
        );
        let _ = fs::remove_dir_all(dir);
    }
}
