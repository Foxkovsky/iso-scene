//! Voxel backend: occupancy sampling of the scene graph into a VOX model.

#![allow(
    clippy::many_single_char_names,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss
)]

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use scene_core::{Instance, Primitive, SceneGraph};
use scene_render::builtin;
use scene_syntax::Diagnostic;

const MAX_AXIS: u32 = 4096;
const MAX_GRID: u32 = 256 * 256 * 256;
const MAX_COLORS: usize = 255;

/// One occupied voxel and its palette index (1-based).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Voxel {
    pub x: u16,
    pub y: u16,
    pub z: u16,
    pub color: u8,
}

/// A voxel model ready for VOX encoding.
#[derive(Clone, Debug, PartialEq)]
pub struct VoxelModel {
    pub size: [u32; 3],
    pub origin: [f64; 3],
    pub step: f64,
    /// Palette entry `i` is colour index `i + 1`.
    pub palette: Vec<[u8; 3]>,
    pub voxels: Vec<Voxel>,
}

impl VoxelModel {
    /// Encode the model as a `MagicaVoxel` `.vox` file.
    ///
    /// Models larger than 256 voxels on an axis are split into several models
    /// placed through a scene graph (`nTRN`/`nGRP`/`nSHP`), because `XYZI`
    /// stores coordinates in a single byte.
    #[must_use]
    #[allow(clippy::too_many_lines)]
    pub fn encode(&self) -> Vec<u8> {
        const CHUNK: u32 = 256;

        let mut rgba = vec![0_u8; 256 * 4];
        for (index, color) in self.palette.iter().enumerate() {
            let base = index * 4;
            rgba[base] = color[0];
            rgba[base + 1] = color[1];
            rgba[base + 2] = color[2];
            rgba[base + 3] = 255;
        }

        let chunked = self.size.iter().any(|&side| side > CHUNK);
        if !chunked {
            let mut size_bytes = Vec::with_capacity(12);
            for value in self.size {
                size_bytes
                    .extend_from_slice(&i32::try_from(value).unwrap_or(i32::MAX).to_le_bytes());
            }
            let mut xyzi = Vec::with_capacity(4 + self.voxels.len() * 4);
            xyzi.extend_from_slice(&(self.voxels.len() as u32).to_le_bytes());
            for voxel in &self.voxels {
                xyzi.extend_from_slice(&[voxel.x as u8, voxel.y as u8, voxel.z as u8, voxel.color]);
            }
            let mut children = Vec::new();
            children.extend(chunk(*b"SIZE", &size_bytes));
            children.extend(chunk(*b"XYZI", &xyzi));
            children.extend(chunk(*b"RGBA", &rgba));
            return wrapping(children);
        }

        let counts = [
            self.size[0].div_ceil(CHUNK),
            self.size[1].div_ceil(CHUNK),
            self.size[2].div_ceil(CHUNK),
        ];
        let mut models: Vec<(u32, [u32; 3], Vec<Voxel>)> = Vec::new();
        for cz in 0..counts[2] {
            for cy in 0..counts[1] {
                for cx in 0..counts[0] {
                    let origin = [cx * CHUNK, cy * CHUNK, cz * CHUNK];
                    let dims = [
                        CHUNK.min(self.size[0] - origin[0]),
                        CHUNK.min(self.size[1] - origin[1]),
                        CHUNK.min(self.size[2] - origin[2]),
                    ];
                    let mut voxels = Vec::new();
                    for voxel in &self.voxels {
                        let (x, y, z) =
                            (u32::from(voxel.x), u32::from(voxel.y), u32::from(voxel.z));
                        if x >= origin[0]
                            && y >= origin[1]
                            && z >= origin[2]
                            && x < origin[0] + dims[0]
                            && y < origin[1] + dims[1]
                            && z < origin[2] + dims[2]
                        {
                            voxels.push(Voxel {
                                x: (x - origin[0]) as u16,
                                y: (y - origin[1]) as u16,
                                z: (z - origin[2]) as u16,
                                color: voxel.color,
                            });
                        }
                    }
                    if voxels.is_empty() {
                        continue;
                    }
                    models.push((models.len() as u32, origin, voxels));
                }
            }
        }

        let mut children = Vec::new();
        for (_, origin, voxels) in &models {
            let dims = chunk_dims(self.size, *origin);
            let mut size_bytes = Vec::with_capacity(12);
            for value in dims {
                size_bytes
                    .extend_from_slice(&i32::try_from(value).unwrap_or(i32::MAX).to_le_bytes());
            }
            let mut xyzi = Vec::with_capacity(4 + voxels.len() * 4);
            xyzi.extend_from_slice(&(voxels.len() as u32).to_le_bytes());
            for voxel in voxels {
                xyzi.extend_from_slice(&[voxel.x as u8, voxel.y as u8, voxel.z as u8, voxel.color]);
            }
            children.extend(chunk(*b"SIZE", &size_bytes));
            children.extend(chunk(*b"XYZI", &xyzi));
        }
        children.extend(chunk(*b"RGBA", &rgba));

        // Scene graph: root transform -> group -> per-model transform -> shape.
        let count = models.len() as u32;
        children.extend(chunk(*b"nTRN", &transform_node(0, 1, [0, 0, 0])));
        children.extend(chunk(
            *b"nGRP",
            &group_node(1, &(0..count).map(|i| 2 + i * 2).collect::<Vec<_>>()),
        ));
        for (index, (_, origin, _)) in models.iter().enumerate() {
            let trn_id = 2 + index as u32 * 2;
            let shp_id = trn_id + 1;
            children.extend(chunk(*b"nTRN", &transform_node(trn_id, shp_id, *origin)));
            children.extend(chunk(*b"nSHP", &shape_node(shp_id, index as u32)));
        }

        wrapping(children)
    }
}

fn chunk_dims(size: [u32; 3], origin: [u32; 3]) -> [u32; 3] {
    [
        256.min(size[0] - origin[0]),
        256.min(size[1] - origin[1]),
        256.min(size[2] - origin[2]),
    ]
}

fn dict(pairs: &[(&str, &str)]) -> Vec<u8> {
    let mut out = (pairs.len() as u32).to_le_bytes().to_vec();
    for (key, value) in pairs {
        out.extend_from_slice(&(key.len() as u32).to_le_bytes());
        out.extend_from_slice(key.as_bytes());
        out.extend_from_slice(&(value.len() as u32).to_le_bytes());
        out.extend_from_slice(value.as_bytes());
    }
    out
}

fn transform_node(id: u32, child: u32, translation: [u32; 3]) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(&id.to_le_bytes());
    out.extend(dict(&[]));
    out.extend_from_slice(&child.to_le_bytes());
    out.extend_from_slice(&(-1_i32).to_le_bytes());
    out.extend_from_slice(&0_i32.to_le_bytes());
    out.extend_from_slice(&1_u32.to_le_bytes());
    let text = format!("{} {} {}", translation[0], translation[1], translation[2]);
    out.extend(dict(&[("_t", &text)]));
    out
}

fn group_node(id: u32, children: &[u32]) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(&id.to_le_bytes());
    out.extend(dict(&[]));
    out.extend_from_slice(&(children.len() as u32).to_le_bytes());
    for child in children {
        out.extend_from_slice(&child.to_le_bytes());
    }
    out
}

fn shape_node(id: u32, model: u32) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(&id.to_le_bytes());
    out.extend(dict(&[]));
    out.extend_from_slice(&1_u32.to_le_bytes());
    out.extend_from_slice(&model.to_le_bytes());
    out.extend(dict(&[]));
    out
}

fn wrapping(children: Vec<u8>) -> Vec<u8> {
    let mut out = Vec::with_capacity(20 + children.len());
    out.extend_from_slice(b"VOX ");
    out.extend_from_slice(&150_u32.to_le_bytes());
    out.extend_from_slice(b"MAIN");
    out.extend_from_slice(&0_u32.to_le_bytes());
    out.extend_from_slice(&(children.len() as u32).to_le_bytes());
    out.extend(children);
    out
}

/// Voxelize a scene at `voxels_per_block` samples per block.
///
/// # Errors
///
/// Returns `E0601` for material problems, `E0602` for an empty scene or a
/// grid larger than 256³ in total or 4096 on one axis, and `E0604` for
/// invalid density.
#[allow(clippy::too_many_lines)]
pub fn voxelize(scene: &SceneGraph, voxels_per_block: f64) -> Result<VoxelModel, Diagnostic> {
    if !voxels_per_block.is_finite() || voxels_per_block <= 0.0 {
        return Err(error("E0604", "voxels per block must be positive"));
    }
    if scene.instances.is_empty() {
        return Err(error("E0602", "scene has no instances to voxelize"));
    }

    let step = 1.0 / voxels_per_block;
    let bounds = scene.bounds;
    let extent = [
        bounds.max[0] - bounds.min[0],
        bounds.max[1] - bounds.min[1],
        bounds.max[2] - bounds.min[2],
    ];
    let size = [
        ((extent[0] / step).ceil() as u32).max(1),
        ((extent[1] / step).ceil() as u32).max(1),
        ((extent[2] / step).ceil() as u32).max(1),
    ];
    let total = u64::from(size[0]) * u64::from(size[1]) * u64::from(size[2]);
    if size.iter().any(|&side| side > MAX_AXIS) || total > u64::from(MAX_GRID) {
        let hint = if size.iter().any(|&side| side > MAX_AXIS) {
            format!("an axis is longer than {MAX_AXIS} voxels; increase --voxel-size")
        } else {
            format!("the model has {total} voxels; increase --voxel-size")
        };
        return Err(error(
            "E0602",
            format!(
                "voxel grid {}x{}x{} is too large; {hint}",
                size[0], size[1], size[2]
            ),
        ));
    }

    let colors = load_colors(scene)?;
    let mut palette: Vec<[u8; 3]> = Vec::new();
    let mut indices: BTreeMap<[u8; 3], u8> = BTreeMap::new();
    let mut grid = vec![0_u8; (size[0] * size[1] * size[2]) as usize];

    for instance in &scene.instances {
        let object_bounds = instance.world_bounds();
        let lo = [
            (((object_bounds.min[0] - bounds.min[0]) / step).floor() as i64).max(0),
            (((object_bounds.min[1] - bounds.min[1]) / step).floor() as i64).max(0),
            (((object_bounds.min[2] - bounds.min[2]) / step).floor() as i64).max(0),
        ];
        let hi = [
            (((object_bounds.max[0] - bounds.min[0]) / step).ceil() as i64).min(i64::from(size[0])),
            (((object_bounds.max[1] - bounds.min[1]) / step).ceil() as i64).min(i64::from(size[1])),
            (((object_bounds.max[2] - bounds.min[2]) / step).ceil() as i64).min(i64::from(size[2])),
        ];

        for z in lo[2]..hi[2] {
            for y in lo[1]..hi[1] {
                for x in lo[0]..hi[0] {
                    let world = [
                        bounds.min[0] + (x as f64 + 0.5) * step,
                        bounds.min[1] + (y as f64 + 0.5) * step,
                        bounds.min[2] + (z as f64 + 0.5) * step,
                    ];
                    for part in &instance.object.parts {
                        let local = to_local(world, instance);
                        if contains(&part.primitive, local) {
                            let color = color_for(part.material.as_deref(), &colors)?;
                            let index = if let Some(index) = indices.get(&color) {
                                *index
                            } else {
                                if palette.len() >= MAX_COLORS {
                                    return Err(error(
                                        "E0602",
                                        format!("more than {MAX_COLORS} colours"),
                                    ));
                                }
                                palette.push(color);
                                let index = (palette.len()) as u8;
                                indices.insert(color, index);
                                index
                            };
                            let grid_index = (x as usize)
                                + (size[0] as usize) * (y as usize)
                                + (size[0] as usize) * (size[1] as usize) * (z as usize);
                            grid[grid_index] = index;
                        }
                    }
                }
            }
        }
    }

    let mut voxels = Vec::new();
    for z in 0..size[2] {
        for y in 0..size[1] {
            for x in 0..size[0] {
                let index = (x as usize)
                    + (size[0] as usize) * (y as usize)
                    + (size[0] as usize) * (size[1] as usize) * (z as usize);
                if grid[index] != 0 {
                    voxels.push(Voxel {
                        x: x as u16,
                        y: y as u16,
                        z: z as u16,
                        color: grid[index],
                    });
                }
            }
        }
    }

    Ok(VoxelModel {
        size,
        origin: bounds.min,
        step,
        palette,
        voxels,
    })
}

/// Write a `.vox` file.
///
/// # Errors
///
/// Returns `E0603` if the file cannot be written.
pub fn write_vox(model: &VoxelModel, path: &Path) -> Result<(), Diagnostic> {
    fs::write(path, model.encode()).map_err(|cause| {
        error(
            "E0603",
            format!("cannot write `{}`: {cause}", path.display()),
        )
    })
}

fn chunk(tag: [u8; 4], data: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(12 + data.len());
    out.extend_from_slice(&tag);
    out.extend_from_slice(&(data.len() as u32).to_le_bytes());
    out.extend_from_slice(&0_u32.to_le_bytes());
    out.extend(data);
    out
}

fn load_colors(scene: &SceneGraph) -> Result<BTreeMap<String, [u8; 3]>, Diagnostic> {
    let mut colors = BTreeMap::new();
    for (name, path) in &scene.materials {
        let palette = scene_render::material::load(path)
            .map_err(|message| error("E0601", format!("material `{name}`: {message}")))?;
        colors.insert(name.clone(), palette.top);
    }
    Ok(colors)
}

fn color_for(
    name: Option<&str>,
    colors: &BTreeMap<String, [u8; 3]>,
) -> Result<[u8; 3], Diagnostic> {
    let name = name.unwrap_or("wood");
    if let Some(color) = colors.get(name) {
        return Ok(*color);
    }
    if let Some(palette) = builtin(name) {
        return Ok(palette.top);
    }
    Err(error("E0605", format!("unknown material `{name}`")))
}

fn to_local(point: [f64; 3], instance: &Instance) -> [f64; 3] {
    instance.to_local(point)
}

fn contains(primitive: &Primitive, p: [f64; 3]) -> bool {
    const EPS: f64 = 1e-9;
    match primitive {
        Primitive::Box { at, size } => {
            p[0] >= at[0] - EPS
                && p[0] <= at[0] + size[0] + EPS
                && p[1] >= at[1] - EPS
                && p[1] <= at[1] + size[1] + EPS
                && p[2] >= at[2] - EPS
                && p[2] <= at[2] + size[2] + EPS
        }
        Primitive::Cyl { at, radius, height } => {
            let dx = p[0] - at[0];
            let dy = p[1] - at[1];
            dx * dx + dy * dy <= radius * radius + EPS
                && p[2] >= at[2] - EPS
                && p[2] <= at[2] + height + EPS
        }
        Primitive::Sphere { at, radius } => {
            let dx = p[0] - at[0];
            let dy = p[1] - at[1];
            let dz = p[2] - at[2];
            dx * dx + dy * dy + dz * dz <= radius * radius + EPS
        }
        Primitive::SlantBox { at, size, offset } => {
            if p[2] < at[2] - EPS || p[2] > at[2] + size[2] + EPS {
                return false;
            }
            let t = ((p[2] - at[2]) / size[2]).clamp(0.0, 1.0);
            let x0 = at[0] + offset[0] * t;
            let y0 = at[1] + offset[1] * t;
            p[0] >= x0 - EPS
                && p[0] <= x0 + size[0] + EPS
                && p[1] >= y0 - EPS
                && p[1] <= y0 + size[1] + EPS
        }
        Primitive::SlantCyl {
            at,
            radius,
            height,
            offset,
        } => {
            if p[2] < at[2] - EPS || p[2] > at[2] + height + EPS {
                return false;
            }
            let t = ((p[2] - at[2]) / height).clamp(0.0, 1.0);
            let dx = p[0] - (at[0] + offset[0] * t);
            let dy = p[1] - (at[1] + offset[1] * t);
            dx * dx + dy * dy <= radius * radius + EPS
        }
        Primitive::Ramp { at, size, rise } => {
            if p[0] < at[0] - EPS
                || p[0] > at[0] + size[0] + EPS
                || p[1] < at[1] - EPS
                || p[1] > at[1] + size[1] + EPS
            {
                return false;
            }
            let t = ((p[0] - at[0]) / size[0]).clamp(0.0, 1.0);
            let top = at[2] + rise[0] + (rise[1] - rise[0]) * t;
            let (low, high) = if top < at[2] {
                (top, at[2])
            } else {
                (at[2], top)
            };
            p[2] >= low - EPS && p[2] <= high + EPS
        }
    }
}

fn error(code: &'static str, message: impl Into<String>) -> Diagnostic {
    Diagnostic::new(code, message, scene_syntax::Span::new(0, 0))
}
