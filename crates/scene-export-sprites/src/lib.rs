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

pub mod animate;
mod atlas;
mod debug;
mod layout;
mod manifest;
#[cfg(test)]
mod tests;
mod tiled;

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::layout::{Cell, Job, Page, PageFiles, cell_metrics, split_view, write_page};
use crate::manifest::manifest_json;
use rayon::prelude::*;
use scene_backend_ortho::View;
use scene_core::CompiledObject;
use scene_syntax::Diagnostic;
use serde_json::Value;

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

    // Every cell is described first, in sheet order, then rendered in
    // parallel; results keep that order and the first failing cell (in order)
    // decides the error, so output and diagnostics do not depend on threads.
    let mut jobs = Vec::with_capacity(cell_count);
    for (row, entry) in objects.iter().enumerate() {
        for (column, view) in options.views.iter().enumerate() {
            let (rotation, angle) = split_view(*view);
            jobs.push(Job {
                row: row as u32,
                column: column as u32,
                entry,
                rotation,
                angle,
                animation: None,
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
            let duration = animation
                .durations
                .get(column)
                .copied()
                .unwrap_or(animation.duration);
            jobs.push(Job {
                row,
                column: column as u32,
                entry,
                rotation: animation.rotation,
                angle,
                animation: Some((index, duration)),
            });
        }
    }
    let rendered: Vec<Result<Cell, Diagnostic>> = jobs
        .par_iter()
        .map(|job| job.render(options, materials))
        .collect();
    let cells = rendered
        .into_iter()
        .collect::<Result<Vec<Cell>, Diagnostic>>()?;

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

    // Pages are independent files: write them in parallel, report in order.
    let written: Vec<Result<PageFiles, Diagnostic>> = pages
        .par_iter()
        .map(|page| write_page(options, page, out_dir))
        .collect();
    let written = written
        .into_iter()
        .collect::<Result<Vec<PageFiles>, Diagnostic>>()?;
    let mut images = written.iter().map(|files| files.image.clone());
    let mut tilesets = written.iter().map(|files| files.tileset.clone());
    let debug = written.iter().find_map(|files| files.debug.clone());
    let atlas = written.iter().find_map(|files| files.atlas.clone());

    let manifest_path = out_dir.join(format!("{}.json", options.name));
    write_json(&manifest_path, &manifest_json(options, &pages))?;

    let first = pages
        .first()
        .ok_or_else(|| error("E0602", "internal: no sheet page"))?;
    Ok(SheetReport {
        image: images
            .next()
            .ok_or_else(|| error("E0602", "internal: no sheet image"))?,
        manifest: manifest_path,
        tileset: tilesets
            .next()
            .ok_or_else(|| error("E0602", "internal: no tileset"))?,
        debug,
        atlas,
        cells: first.cell_size,
        columns: first.columns,
        rows: first.rows,
        pages: pages.len(),
        sprites: objects.len() * options.views.len(),
        frames: animation_frames,
    })
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
