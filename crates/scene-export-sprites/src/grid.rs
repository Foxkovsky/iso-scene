//! Shared page and cell model plus the pixel grid.

use scene_backend_iso::Sprite;

/// One sheet image with its own uniform grid.
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

/// Top-left pixel of a cell in the sheet.
pub(crate) fn cell_origin(cell: &Cell, cell_size: [u32; 2], padding: u32) -> [u32; 2] {
    [
        padding + cell.column * (cell_size[0] + padding),
        padding + cell.row * (cell_size[1] + padding),
    ]
}
