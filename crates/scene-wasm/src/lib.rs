//! A minimal WebAssembly entry point for the iso renderer.
//!
//! The browser demo calls [`render_demo`], reads the RGBA bytes at the returned
//! pointer from the module's linear memory, and draws them into a canvas.
//! This crate deliberately stays small: it renders one built-in box through
//! `scene-backend-iso`, proving the rasterizer and geometry pipeline compile to
//! `wasm32-unknown-unknown` without a filesystem.
//!
//! The full navigator will replace the demo scene with a serialized
//! `SceneGraph` and render only the visible tiles.

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::{Arc, Mutex};

use scene_core::{Instance, SceneGraph};
use scene_syntax::ast::{Background, RenderKind};
use wasm_bindgen::prelude::*;

/// The last rendered image; width and height are returned separately.
static IMAGE: Mutex<Option<scene_render::Image>> = Mutex::new(None);

/// Render a built-in isometric box and return its RGBA bytes.
///
/// The returned array is a copy; read the size with [`demo_width`] and
/// [`demo_height`]. Returns an empty array when the built-in scene cannot be
/// compiled or rendered.
#[wasm_bindgen]
pub fn render_demo() -> Vec<u8> {
    let Some(image) = build_demo() else {
        return Vec::new();
    };
    let Ok(mut slot) = IMAGE.lock() else {
        return Vec::new();
    };
    let bytes = image.rgba.clone();
    *slot = Some(image);
    bytes
}

/// Width of the last [`render_demo`] image in pixels.
#[wasm_bindgen]
pub fn demo_width() -> u32 {
    IMAGE
        .lock()
        .ok()
        .and_then(|slot| slot.as_ref().map(|image| image.width))
        .unwrap_or(0)
}

/// Height of the last [`render_demo`] image in pixels.
#[wasm_bindgen]
pub fn demo_height() -> u32 {
    IMAGE
        .lock()
        .ok()
        .and_then(|slot| slot.as_ref().map(|image| image.height))
        .unwrap_or(0)
}

fn build_demo() -> Option<scene_render::Image> {
    let source = "version 1;\n\
        object demo (1, 1) {\n\
            box at (0, 0, 0) size (1, 1, 1) material wood;\n\
        }\n";
    let program = scene_syntax::parse(source).ok()?;
    let model = scene_core::compile_model(&program, Path::new(".")).ok()?;
    let object = model.objects.get("demo")?.clone();
    let graph = SceneGraph {
        shadow: scene_core::Shadow::default(),
        halo: scene_core::Halo::default(),
        floor: None,
        render: RenderKind::Iso,
        scale: 32,
        canvas: None,
        sun: [135.0, 45.0],
        ambient: 0.35,
        background: Background::Transparent,
        materials: BTreeMap::new(),
        instances: vec![Instance {
            object: Arc::new((*object).clone()),
            at: [0.0, 0.0, 0.0],
            rotation: 0,
            angle: 0.0,
        }],
        bounds: object.local_bounds(),
        animations: Vec::new(),
        animation_frame: None,
    };
    scene_backend_iso::render(&graph).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_demo_renders_a_non_empty_box() {
        let bytes = render_demo();
        let width = demo_width();
        let height = demo_height();
        assert!(width > 0 && height > 0, "demo size is {width}x{height}");
        assert_eq!(bytes.len(), (width as usize) * (height as usize) * 4);
        assert!(bytes.chunks_exact(4).any(|pixel| pixel[3] == 255));
    }
}
