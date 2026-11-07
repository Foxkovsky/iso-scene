//! Validation, evaluation and the `SceneGraph` shared by all backends.

pub mod eval;
pub mod geometry;

pub use eval::{compile_model, compile_scene, compile_scene_with_fuel};
pub use geometry::{
    Bounds, CompiledModel, CompiledObject, Instance, Part, Primitive, SceneGraph, rotate_degrees,
    rotate_xy,
};

/// Axis-aligned extent measured in blocks.
pub type Blocks = f64;

/// Default evaluation budget.
pub const DEFAULT_FUEL: u64 = 1_000_000;
/// Maximum number of primitives across all objects.
pub const MAX_SHAPES: usize = 100_000;
/// Maximum number of placed instances in a scene.
pub const MAX_INSTANCES: usize = 100_000;
/// Maximum number of object declarations.
pub const MAX_OBJECTS: usize = 4_096;
/// Maximum include nesting depth.
pub const MAX_INCLUDE_DEPTH: usize = 16;
/// Maximum total size of included files.
pub const MAX_INCLUDED_BYTES: usize = 8 * 1024 * 1024;
/// Maximum absolute coordinate in blocks.
pub const MAX_COORD: f64 = 1_000_000.0;
/// Maximum absolute loop bound.
pub const MAX_LOOP_BOUND: f64 = 1_000_000_000.0;
/// Maximum canvas side in blocks.
pub const MAX_CANVAS_BLOCKS: f64 = 8_192.0;
/// Maximum pixels per block.
pub const MAX_SCALE: u32 = 256;
