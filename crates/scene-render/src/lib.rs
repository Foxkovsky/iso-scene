//! Shared rendering primitives: rasterizer, image, materials and meshes.

pub mod image;
pub mod material;
pub mod mesh;
pub mod raster;

pub use image::{Image, Rect, composite_over, write_png};
pub use material::{Palette, Texture, builtin};
pub use mesh::{MeshFace, primitive_faces};
pub use raster::Framebuffer;
