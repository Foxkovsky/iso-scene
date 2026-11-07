//! Cells and pages: rendering, pivots, the uniform grid and page files.

use std::path::{Path, PathBuf};

use scene_backend_iso::{Sprite, render_object_sprite_with};
use scene_core::CompiledObject;
use scene_render::{Image, composite_over, write_png};
use scene_syntax::Diagnostic;

use crate::atlas::atlas_json;
use crate::debug::debug_grid;
use crate::tiled::{TiledLayer, tileset_json};
use crate::{Pivot, Projection, SheetObject, SheetOptions, error, write_json};

/// One sheet image with its own uniform grid.
pub(crate) struct Page {
    /// File stem: the sheet name, or `name-<n>` when there are several pages.
    pub(crate) stem: String,
    pub(crate) cells: Vec<Cell>,
    pub(crate) cell_size: [u32; 2],
    pub(crate) anchor: [f64; 2],
    pub(crate) columns: u32,
    pub(crate) rows: u32,
    pub(crate) width: u32,
    pub(crate) height: u32,
}

impl Page {
    pub(crate) fn image_name(&self) -> String {
        format!("{}.png", self.stem)
    }
}

pub(crate) struct Cell {
    /// Row within the cell's page.
    pub(crate) row: u32,
    pub(crate) column: u32,
    pub(crate) name: String,
    /// Declared footprint `(width, depth)` in blocks.
    pub(crate) footprint: [f64; 2],
    pub(crate) source: Option<String>,
    /// Quarter turn used for the view.
    pub(crate) rotation: u16,
    /// Extra yaw: the rest of a free view angle, or the animated angle.
    pub(crate) angle: f64,
    /// Index into `SheetOptions::animations` for an animation frame.
    pub(crate) animation: Option<usize>,
    pub(crate) duration: Option<u32>,
    pub(crate) sprite: Sprite,
    /// Pixel that must sit on the cell anchor: the footprint pivot for a
    /// static view, the object's local origin for an animation frame.
    pub(crate) reference: [f64; 2],
}

/// One cell to render: which object, how it is turned and where it goes.
pub(crate) struct Job<'a> {
    pub(crate) row: u32,
    pub(crate) column: u32,
    pub(crate) entry: &'a SheetObject,
    pub(crate) rotation: u16,
    pub(crate) angle: f64,
    /// Animation index and frame duration for an animation frame.
    pub(crate) animation: Option<(usize, u32)>,
}

impl Job<'_> {
    pub(crate) fn render(
        &self,
        options: &SheetOptions,
        materials: &std::collections::BTreeMap<String, PathBuf>,
    ) -> Result<Cell, Diagnostic> {
        let object = &self.entry.object;
        let sprite = render_cell(object, self.rotation, self.angle, options, materials)?;
        // A static view sits on its footprint pivot; a frame turns around the
        // object's local origin.
        let reference = match self.animation {
            None => footprint_pivot(
                object,
                self.rotation,
                self.angle,
                options.scale,
                &sprite,
                options.pivot,
                options.projection,
            ),
            Some(_) => sprite.anchor,
        };
        Ok(Cell {
            row: self.row,
            column: self.column,
            name: self.entry.name.clone(),
            footprint: [object.width, object.depth],
            source: self.entry.source.clone(),
            rotation: self.rotation,
            angle: self.angle,
            animation: self.animation.map(|(index, _)| index),
            duration: self.animation.map(|(_, duration)| duration),
            sprite,
            reference,
        })
    }
}

/// Files written for one page.
pub(crate) struct PageFiles {
    pub(crate) image: PathBuf,
    pub(crate) tileset: PathBuf,
    pub(crate) debug: Option<PathBuf>,
    pub(crate) atlas: Option<PathBuf>,
}

/// Composite a page and write its image, tilesets and optional extras.
pub(crate) fn write_page(
    options: &SheetOptions,
    page: &Page,
    out_dir: &Path,
) -> Result<PageFiles, Diagnostic> {
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
    let image = out_dir.join(page.image_name());
    write_png(&sheet, &image).map_err(|message| error("E0603", message))?;

    let debug = if options.debug_grid {
        let path = out_dir.join(format!("{}.debug.png", page.stem));
        write_png(&debug_grid(&sheet, page, options.padding), &path)
            .map_err(|message| error("E0603", message))?;
        Some(path)
    } else {
        None
    };

    let tileset = out_dir.join(format!("{}.tsj", page.stem));
    write_json(&tileset, &tileset_json(options, page, TiledLayer::Tiles))?;
    let objects = out_dir.join(format!("{}.objects.tsj", page.stem));
    write_json(&objects, &tileset_json(options, page, TiledLayer::Objects))?;

    let atlas = if options.atlas {
        let path = out_dir.join(format!("{}.atlas.json", page.stem));
        write_json(&path, &atlas_json(options, page))?;
        Some(path)
    } else {
        None
    };
    Ok(PageFiles {
        image,
        tileset,
        debug,
        atlas,
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
pub(crate) fn footprint_pivot(
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
pub(crate) fn render_cell(
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
pub(crate) fn split_view(view: f64) -> (u16, f64) {
    let view = view.rem_euclid(360.0);
    let quarter = (view / 90.0).floor().clamp(0.0, 3.0);
    let rest = view - quarter * 90.0;
    // Snap float noise such as 89.999999 to the next exact quarter turn.
    if (90.0 - rest).abs() < 1e-9 {
        return ((((quarter as u16) + 1) % 4) * 90, 0.0);
    }
    ((quarter as u16) * 90, rest)
}

/// Uniform cell size and the bottom-centre anchor inside a cell.
pub(crate) fn cell_metrics(cells: &[Cell]) -> ([u32; 2], [f64; 2]) {
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

/// Top-left pixel of a cell in the sheet.
pub(crate) fn cell_origin(cell: &Cell, cell_size: [u32; 2], padding: u32) -> [u32; 2] {
    [
        padding + cell.column * (cell_size[0] + padding),
        padding + cell.row * (cell_size[1] + padding),
    ]
}
