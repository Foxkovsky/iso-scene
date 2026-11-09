//! Object footprints on the cell grid and the collisions between them.

use std::collections::BTreeSet;

use scene_core::Instance;
use scene_syntax::Diagnostic;
use scene_walls::WallLayout;

/// Most footprint cells, summed over all objects, that a collision check
/// visits (`docs/spec.md` §9).
pub const MAX_FOOTPRINT_CELLS: u64 = 4_000_000;

/// Footprint edges closer than this to a cell border do not spill into the
/// next cell, and overlaps thinner than this are touching, not colliding.
const EPSILON: f64 = 1e-6;

/// The declared footprint of a placed object.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Footprint {
    /// World rectangle `[min_x, min_y, max_x, max_y]` in blocks.
    pub area: [f64; 4],
    /// Height range `[bottom, top]` in blocks.
    pub height: [f64; 2],
}

impl Footprint {
    pub(crate) fn of(instance: &Instance) -> Self {
        let (width, depth) = match instance.rotation {
            90 | 270 => (instance.object.depth, instance.object.width),
            _ => (instance.object.width, instance.object.depth),
        };
        let bounds = instance.world_bounds();
        Self {
            area: [
                instance.at[0],
                instance.at[1],
                instance.at[0] + width,
                instance.at[1] + depth,
            ],
            height: [bounds.min[2], bounds.max[2]],
        }
    }

    /// Cells the footprint covers, as `[x0, y0, x1, y1]` (half-open).
    #[allow(clippy::cast_possible_truncation)]
    pub(crate) fn cells(&self) -> [i64; 4] {
        let [x0, y0, x1, y1] = self.area;
        [
            (x0 + EPSILON).floor() as i64,
            (y0 + EPSILON).floor() as i64,
            ((x1 - EPSILON).ceil() as i64).max((x0 + EPSILON).floor() as i64 + 1),
            ((y1 - EPSILON).ceil() as i64).max((y0 + EPSILON).floor() as i64 + 1),
        ]
    }

    fn overlaps(&self, other: &Self) -> bool {
        let [ax0, ay0, ax1, ay1] = self.area;
        let [bx0, by0, bx1, by1] = other.area;
        ax0.max(bx0) + EPSILON < ax1.min(bx1)
            && ay0.max(by0) + EPSILON < ay1.min(by1)
            && self.height[0].max(other.height[0]) + EPSILON < self.height[1].min(other.height[1])
    }
}

/// Two things that occupy the same space.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Collision {
    /// Two objects, by index in the scene and name, overlap in plan and
    /// height.
    Objects {
        first: (usize, String),
        second: (usize, String),
    },
    /// An object stands on a wall cell.
    Wall {
        object: (usize, String),
        cell: [i64; 2],
    },
}

/// Find every pair of overlapping objects and every object on a wall.
///
/// Objects are bucketed by the cells they cover, so only objects that share a
/// cell are compared. The result is sorted, so it does not depend on the
/// order the buckets are visited in.
///
/// # Errors
///
/// Returns `E0602` when the footprints cover more than
/// [`MAX_FOOTPRINT_CELLS`] cells in total, before allocating for them.
pub(crate) fn find(
    instances: &[Instance],
    walls: Option<&WallLayout>,
) -> Result<Vec<Collision>, Diagnostic> {
    let footprints: Vec<Footprint> = instances.iter().map(Footprint::of).collect();
    let total = footprints.iter().try_fold(0_u64, |total, footprint| {
        let [x0, y0, x1, y1] = footprint.cells();
        let count = x1
            .saturating_sub(x0)
            .unsigned_abs()
            .saturating_mul(y1.saturating_sub(y0).unsigned_abs());
        total
            .checked_add(count)
            .filter(|&sum| sum <= MAX_FOOTPRINT_CELLS)
    });
    let Some(total) = total else {
        return Err(Diagnostic::new(
            "E0602",
            format!("object footprints cover more than {MAX_FOOTPRINT_CELLS} cells"),
            scene_syntax::Span::new(0, 0),
        ));
    };
    let name = |index: usize| (index, instances[index].object.name.clone());
    let mut found = BTreeSet::new();
    #[allow(clippy::cast_possible_truncation)]
    let mut cells: Vec<([i64; 2], usize)> = Vec::with_capacity(total as usize);
    for (index, footprint) in footprints.iter().enumerate() {
        let [x0, y0, x1, y1] = footprint.cells();
        for y in y0..y1 {
            for x in x0..x1 {
                cells.push(([x, y], index));
            }
        }
    }
    cells.sort_unstable();
    for bucket in cells.chunk_by(|a, b| a.0 == b.0) {
        for (offset, &(_, first)) in bucket.iter().enumerate() {
            for &(_, second) in &bucket[offset + 1..] {
                if footprints[first].overlaps(&footprints[second]) {
                    found.insert(Collision::Objects {
                        first: name(first),
                        second: name(second),
                    });
                }
            }
        }
    }
    if let Some(layout) = walls {
        let columns = i64::from(layout.columns);
        let rows = i64::from(layout.rows);
        let mut wall = vec![false; (layout.columns as usize) * (layout.rows as usize)];
        for cell in &layout.cells {
            wall[(cell.y * layout.columns + cell.x) as usize] = true;
        }
        for &([x, y], index) in &cells {
            #[allow(clippy::cast_sign_loss, clippy::cast_possible_truncation)]
            let on_wall =
                x >= 0 && y >= 0 && x < columns && y < rows && wall[(y * columns + x) as usize];
            if on_wall {
                found.insert(Collision::Wall {
                    object: name(index),
                    cell: [x, y],
                });
            }
        }
    }
    Ok(found.into_iter().collect())
}
