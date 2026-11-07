//! Geometry primitives, compiled objects and the scene graph.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;

use scene_syntax::ast::{Axis, Background, RenderKind};

/// A solid primitive in object-local coordinates, measured in blocks.
#[derive(Clone, Debug, PartialEq)]
pub enum Primitive {
    Box {
        at: [f64; 3],
        size: [f64; 3],
    },
    Cyl {
        at: [f64; 3],
        radius: f64,
        height: f64,
    },
    Sphere {
        at: [f64; 3],
        radius: f64,
    },
    SlantBox {
        at: [f64; 3],
        size: [f64; 3],
        offset: [f64; 2],
    },
    SlantCyl {
        at: [f64; 3],
        radius: f64,
        height: f64,
        offset: [f64; 2],
    },
    Ramp {
        at: [f64; 3],
        size: [f64; 2],
        rise: [f64; 2],
    },
}

impl Primitive {
    /// Local axis-aligned bounds of the primitive.
    #[must_use]
    pub fn local_bounds(&self) -> Bounds {
        match self {
            Self::Box { at, size } => Bounds::from_min_max(*at, add(*at, *size)),
            Self::Cyl { at, radius, height } => Bounds::from_min_max(
                [at[0] - radius, at[1] - radius, at[2]],
                [at[0] + radius, at[1] + radius, at[2] + height],
            ),
            Self::Sphere { at, radius } => Bounds::from_min_max(
                [at[0] - radius, at[1] - radius, at[2] - radius],
                [at[0] + radius, at[1] + radius, at[2] + radius],
            ),
            Self::SlantBox { at, size, offset } => {
                let bottom = Bounds::from_min_max(*at, add(*at, *size));
                let top_min = [at[0] + offset[0], at[1] + offset[1], at[2] + size[2]];
                let top = Bounds::from_min_max(
                    top_min,
                    [top_min[0] + size[0], top_min[1] + size[1], top_min[2]],
                );
                bottom.union(top)
            }
            Self::SlantCyl {
                at,
                radius,
                height,
                offset,
            } => {
                let bottom = Bounds::from_min_max(
                    [at[0] - radius, at[1] - radius, at[2]],
                    [at[0] + radius, at[1] + radius, at[2]],
                );
                let top = Bounds::from_min_max(
                    [
                        at[0] + offset[0] - radius,
                        at[1] + offset[1] - radius,
                        at[2] + height,
                    ],
                    [
                        at[0] + offset[0] + radius,
                        at[1] + offset[1] + radius,
                        at[2] + height,
                    ],
                );
                bottom.union(top)
            }
            Self::Ramp { at, size, rise } => {
                let z_min = at[2] + rise[0].min(rise[1]).min(0.0);
                let z_max = at[2] + rise[0].max(rise[1]).max(0.0);
                Bounds::from_min_max(
                    [at[0], at[1], z_min],
                    [at[0] + size[0], at[1] + size[1], z_max],
                )
            }
        }
    }

    #[must_use]
    fn mirrored(&self, axis: Axis, width: f64, depth: f64) -> Self {
        match self {
            Self::Box { at, size } => Self::Box {
                at: mirror_at(*at, *size, axis, width, depth),
                size: *size,
            },
            Self::Cyl { at, radius, height } => Self::Cyl {
                at: mirror_center(*at, axis, width, depth),
                radius: *radius,
                height: *height,
            },
            Self::Sphere { at, radius } => Self::Sphere {
                at: mirror_center(*at, axis, width, depth),
                radius: *radius,
            },
            Self::SlantBox { at, size, offset } => Self::SlantBox {
                at: mirror_at(*at, *size, axis, width, depth),
                size: *size,
                offset: mirror_offset(*offset, axis),
            },
            Self::SlantCyl {
                at,
                radius,
                height,
                offset,
            } => Self::SlantCyl {
                at: mirror_center(*at, axis, width, depth),
                radius: *radius,
                height: *height,
                offset: mirror_offset(*offset, axis),
            },
            Self::Ramp { at, size, rise } => Self::Ramp {
                at: mirror_at(*at, [size[0], size[1], 0.0], axis, width, depth),
                size: *size,
                rise: if axis == Axis::X {
                    [rise[1], rise[0]]
                } else {
                    *rise
                },
            },
        }
    }
}

impl Primitive {
    /// Rotate the primitive in 90 degree steps around the object footprint.
    #[must_use]
    pub(crate) fn rotated(&self, rotation: u16, width: f64, depth: f64) -> Self {
        if rotation == 0 {
            return self.clone();
        }
        let point = |x: f64, y: f64| rotate_xy(x, y, width, depth, rotation);
        let vector = |dx: f64, dy: f64| match rotation {
            90 => (-dy, dx),
            180 => (-dx, -dy),
            270 => (dy, -dx),
            _ => (dx, dy),
        };
        match self {
            Self::Box { at, size } => {
                let (x1, y1) = point(at[0], at[1]);
                let (x2, y2) = point(at[0] + size[0], at[1] + size[1]);
                Self::Box {
                    at: [x1.min(x2), y1.min(y2), at[2]],
                    size: [(x2 - x1).abs(), (y2 - y1).abs(), size[2]],
                }
            }
            Self::Cyl { at, radius, height } => {
                let (x, y) = point(at[0], at[1]);
                Self::Cyl {
                    at: [x, y, at[2]],
                    radius: *radius,
                    height: *height,
                }
            }
            Self::Sphere { at, radius } => {
                let (x, y) = point(at[0], at[1]);
                Self::Sphere {
                    at: [x, y, at[2]],
                    radius: *radius,
                }
            }
            Self::SlantBox { at, size, offset } => {
                let (x1, y1) = point(at[0], at[1]);
                let (x2, y2) = point(at[0] + size[0], at[1] + size[1]);
                let (dx, dy) = vector(offset[0], offset[1]);
                Self::SlantBox {
                    at: [x1.min(x2), y1.min(y2), at[2]],
                    size: [(x2 - x1).abs(), (y2 - y1).abs(), size[2]],
                    offset: [dx, dy],
                }
            }
            Self::SlantCyl {
                at,
                radius,
                height,
                offset,
            } => {
                let (x, y) = point(at[0], at[1]);
                let (dx, dy) = vector(offset[0], offset[1]);
                Self::SlantCyl {
                    at: [x, y, at[2]],
                    radius: *radius,
                    height: *height,
                    offset: [dx, dy],
                }
            }
            Self::Ramp { at, size, rise } => {
                let (x1, y1) = point(at[0], at[1]);
                let (x2, y2) = point(at[0] + size[0], at[1] + size[1]);
                let (dx, dy) = vector(rise[0], rise[1]);
                Self::Ramp {
                    at: [x1.min(x2), y1.min(y2), at[2]],
                    size: [(x2 - x1).abs(), (y2 - y1).abs()],
                    rise: [dx, dy],
                }
            }
        }
    }

    /// Translate the primitive by an object-local offset.
    #[must_use]
    pub(crate) fn translated(&self, offset: [f64; 3]) -> Self {
        let shift = |at: &[f64; 3]| [at[0] + offset[0], at[1] + offset[1], at[2] + offset[2]];
        match self {
            Self::Box { at, size } => Self::Box {
                at: shift(at),
                size: *size,
            },
            Self::Cyl { at, radius, height } => Self::Cyl {
                at: shift(at),
                radius: *radius,
                height: *height,
            },
            Self::Sphere { at, radius } => Self::Sphere {
                at: shift(at),
                radius: *radius,
            },
            Self::SlantBox {
                at,
                size,
                offset: slant,
            } => Self::SlantBox {
                at: shift(at),
                size: *size,
                offset: *slant,
            },
            Self::SlantCyl {
                at,
                radius,
                height,
                offset: slant,
            } => Self::SlantCyl {
                at: shift(at),
                radius: *radius,
                height: *height,
                offset: *slant,
            },
            Self::Ramp { at, size, rise } => Self::Ramp {
                at: shift(at),
                size: *size,
                rise: *rise,
            },
        }
    }
}

/// A primitive together with its material alias.
#[derive(Clone, Debug, PartialEq)]
pub struct Part {
    pub primitive: Primitive,
    pub material: Option<String>,
    /// Parts sharing a merge group are welded into one solid when rendered.
    pub group: Option<u32>,
}

impl Part {
    #[must_use]
    pub(crate) fn rotated(&self, rotation: u16, width: f64, depth: f64) -> Self {
        Self {
            primitive: self.primitive.rotated(rotation, width, depth),
            material: self.material.clone(),
            group: self.group,
        }
    }

    #[must_use]
    pub(crate) fn translated(&self, offset: [f64; 3]) -> Self {
        Self {
            primitive: self.primitive.translated(offset),
            material: self.material.clone(),
            group: self.group,
        }
    }

    #[must_use]
    pub(crate) fn mirrored(&self, axis: Axis, width: f64, depth: f64) -> Self {
        Self {
            primitive: self.primitive.mirrored(axis, width, depth),
            material: self.material.clone(),
            group: self.group,
        }
    }
}

/// A reusable object compiled from a `.model` file or an inline `object`.
#[derive(Clone, Debug, PartialEq)]
pub struct CompiledObject {
    pub name: String,
    pub width: f64,
    pub depth: f64,
    pub parts: Vec<Part>,
}

impl CompiledObject {
    #[must_use]
    pub fn local_bounds(&self) -> Bounds {
        self.parts
            .iter()
            .fold(Bounds::empty(), |bounds, part| {
                bounds.union(part.primitive.local_bounds())
            })
            .or_zero()
    }
}

/// A placed object in a scene.
#[derive(Clone, Debug)]
pub struct Instance {
    pub object: Arc<CompiledObject>,
    pub at: [f64; 3],
    pub rotation: u16,
    /// Extra yaw in degrees, applied around the object's local origin.
    pub angle: f64,
}

/// Rotate a point around the origin by an arbitrary angle in degrees.
#[must_use]
pub fn rotate_degrees(x: f64, y: f64, degrees: f64) -> (f64, f64) {
    if degrees == 0.0 {
        return (x, y);
    }
    let radians = degrees.to_radians();
    let (sin, cos) = radians.sin_cos();
    (x * cos - y * sin, x * sin + y * cos)
}

impl Instance {
    /// Object-local point to world space, including the extra yaw.
    #[must_use]
    pub fn to_world(&self, point: [f64; 3]) -> [f64; 3] {
        let (x, y) = rotate_xy(
            point[0],
            point[1],
            self.object.width,
            self.object.depth,
            self.rotation,
        );
        let (x, y) = rotate_degrees(x, y, self.angle);
        [x + self.at[0], y + self.at[1], point[2] + self.at[2]]
    }

    /// World point back to object-local space.
    #[must_use]
    pub fn to_local(&self, point: [f64; 3]) -> [f64; 3] {
        let x = point[0] - self.at[0];
        let y = point[1] - self.at[1];
        let (x, y) = rotate_degrees(x, y, -self.angle);
        let (x, y) = match self.rotation {
            90 => (y, self.object.depth - x),
            180 => (self.object.width - x, self.object.depth - y),
            270 => (self.object.width - y, x),
            _ => (x, y),
        };
        [x, y, point[2] - self.at[2]]
    }

    /// World-space bounds of the placed object.
    #[must_use]
    pub fn world_bounds(&self) -> Bounds {
        let mut bounds = Bounds::empty();
        for part in &self.object.parts {
            let local = part.primitive.local_bounds();
            for corner in corners(local) {
                bounds.include(self.to_world(corner));
            }
        }
        bounds.or_zero()
    }
}

/// Everything a render backend needs.
#[derive(Clone, Debug)]
pub struct SceneGraph {
    pub render: RenderKind,
    pub scale: u32,
    pub canvas: Option<[f64; 3]>,
    pub sun: [f64; 2],
    pub ambient: f64,
    pub background: Background,
    pub materials: BTreeMap<String, PathBuf>,
    pub instances: Vec<Instance>,
    pub bounds: Bounds,
}

/// A compiled model library, used for model previews.
#[derive(Clone, Debug)]
pub struct CompiledModel {
    pub materials: BTreeMap<String, PathBuf>,
    pub objects: BTreeMap<String, Arc<CompiledObject>>,
    pub bounds: Bounds,
}

/// Axis-aligned bounds in blocks.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Bounds {
    pub min: [f64; 3],
    pub max: [f64; 3],
}

impl Bounds {
    #[must_use]
    pub const fn empty() -> Self {
        Self {
            min: [f64::INFINITY; 3],
            max: [f64::NEG_INFINITY; 3],
        }
    }

    #[must_use]
    pub const fn from_min_max(min: [f64; 3], max: [f64; 3]) -> Self {
        Self { min, max }
    }

    #[must_use]
    pub fn is_empty(self) -> bool {
        self.min[0] > self.max[0]
    }

    pub fn include(&mut self, point: [f64; 3]) {
        for ((min, max), value) in self.min.iter_mut().zip(self.max.iter_mut()).zip(point) {
            *min = (*min).min(value);
            *max = (*max).max(value);
        }
    }

    #[must_use]
    pub fn union(self, other: Self) -> Self {
        if self.is_empty() {
            return other;
        }
        if other.is_empty() {
            return self;
        }
        let mut result = self;
        result.include(other.min);
        result.include(other.max);
        result
    }

    #[must_use]
    pub fn or_zero(self) -> Self {
        if self.is_empty() {
            Self::from_min_max([0.0; 3], [0.0; 3])
        } else {
            self
        }
    }

    #[must_use]
    pub fn size(self) -> [f64; 3] {
        [
            self.max[0] - self.min[0],
            self.max[1] - self.min[1],
            self.max[2] - self.min[2],
        ]
    }
}

fn add(at: [f64; 3], size: [f64; 3]) -> [f64; 3] {
    [at[0] + size[0], at[1] + size[1], at[2] + size[2]]
}

fn mirror_at(at: [f64; 3], size: [f64; 3], axis: Axis, width: f64, depth: f64) -> [f64; 3] {
    match axis {
        Axis::X => [width - (at[0] + size[0]), at[1], at[2]],
        Axis::Y => [at[0], depth - (at[1] + size[1]), at[2]],
    }
}

fn mirror_center(at: [f64; 3], axis: Axis, width: f64, depth: f64) -> [f64; 3] {
    match axis {
        Axis::X => [width - at[0], at[1], at[2]],
        Axis::Y => [at[0], depth - at[1], at[2]],
    }
}

fn mirror_offset(offset: [f64; 2], axis: Axis) -> [f64; 2] {
    match axis {
        Axis::X => [-offset[0], offset[1]],
        Axis::Y => [offset[0], -offset[1]],
    }
}

fn corners(bounds: Bounds) -> [[f64; 3]; 8] {
    [
        [bounds.min[0], bounds.min[1], bounds.min[2]],
        [bounds.max[0], bounds.min[1], bounds.min[2]],
        [bounds.min[0], bounds.max[1], bounds.min[2]],
        [bounds.max[0], bounds.max[1], bounds.min[2]],
        [bounds.min[0], bounds.min[1], bounds.max[2]],
        [bounds.max[0], bounds.min[1], bounds.max[2]],
        [bounds.min[0], bounds.max[1], bounds.max[2]],
        [bounds.max[0], bounds.max[1], bounds.max[2]],
    ]
}

#[must_use]
pub fn rotate_xy(x: f64, y: f64, width: f64, depth: f64, rotation: u16) -> (f64, f64) {
    match rotation {
        90 => (depth - y, x),
        180 => (width - x, depth - y),
        270 => (y, width - x),
        _ => (x, y),
    }
}
