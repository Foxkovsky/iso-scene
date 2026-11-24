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
/// Why the last scene render failed, or an empty string.
static ERROR: Mutex<String> = Mutex::new(String::new());

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

/// Add one file to the in-memory tree used by [`render_scene`].
///
/// # Errors
///
/// Returns an error for an empty path or a poisoned filesystem lock.
#[wasm_bindgen]
pub fn set_file(path: &str, bytes: Vec<u8>) -> Result<(), JsValue> {
    scene_fs::set_file(path, bytes).map_err(|message| JsValue::from_str(&message))
}

/// Drop the in-memory files before loading another scene folder.
#[wasm_bindgen]
pub fn clear_files() {
    scene_fs::clear_files();
}

/// Render a `.scene` against the files added with [`set_file`].
///
/// `base` is the scene file's directory; includes and material paths resolve
/// from it. Returns RGBA bytes; read the size with [`scene_width`] and
/// [`scene_height`], and a failure with [`scene_error`].
#[wasm_bindgen]
pub fn render_scene(source: &str, base: &str) -> Vec<u8> {
    let result = (|| -> Result<scene_render::Image, String> {
        let program = scene_syntax::parse(source)
            .map_err(|error| format!("{}: {}", error.code, error.message))?;
        let scene = scene_core::compile_scene(&program, Path::new(base), Path::new(""))
            .map_err(|error| format!("{}: {}", error.code, error.message))?;
        scene_backend_iso::render(&scene)
            .map_err(|error| format!("{}: {}", error.code, error.message))
    })();
    match result {
        Ok(image) => {
            if let Ok(mut error) = ERROR.lock() {
                error.clear();
            }
            let bytes = image.rgba.clone();
            if let Ok(mut slot) = IMAGE.lock() {
                *slot = Some(image);
            }
            bytes
        }
        Err(message) => {
            if let Ok(mut error) = ERROR.lock() {
                *error = message;
            }
            Vec::new()
        }
    }
}

/// Last [`render_scene`] error, or an empty string after success.
#[wasm_bindgen]
pub fn scene_error() -> String {
    ERROR.lock().map(|error| error.clone()).unwrap_or_default()
}

/// Width of the last [`render_scene`] image in pixels.
#[wasm_bindgen]
#[must_use]
pub fn scene_width() -> u32 {
    demo_width()
}

/// Height of the last [`render_scene`] image in pixels.
#[wasm_bindgen]
#[must_use]
pub fn scene_height() -> u32 {
    demo_height()
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
    use std::sync::Mutex as TestMutex;

    static TEST_LOCK: TestMutex<()> = TestMutex::new(());

    #[test]
    fn the_demo_renders_a_non_empty_box() {
        let _guard = TEST_LOCK.lock().expect("test lock");
        let bytes = render_demo();
        let width = demo_width();
        let height = demo_height();
        assert!(width > 0 && height > 0, "demo size is {width}x{height}");
        assert_eq!(bytes.len(), (width as usize) * (height as usize) * 4);
        assert!(bytes.chunks_exact(4).any(|pixel| pixel[3] == 255));
    }

    #[test]
    fn a_scene_with_includes_and_materials_renders_from_memory() {
        let _guard = TEST_LOCK.lock().expect("test lock");
        scene_fs::clear_files();
        scene_fs::set_file(
            "box.model",
            b"version 1; material paint = \"paint.toml\"; object box1 (1, 1) { box at (0, 0, 0) size (1, 1, 1) material paint; }".to_vec(),
        )
        .expect("model");
        scene_fs::set_file(
            "paint.toml",
            b"color = \"#C03030\"\nspecular = 0.0\n".to_vec(),
        )
        .expect("material");
        let source =
            "version 1; include \"box.model\"; render iso; scale 8; place box1 at (0, 0, 0);";
        let bytes = render_scene(source, "");
        assert!(scene_error().is_empty(), "{}", scene_error());
        assert!(scene_width() > 0 && scene_height() > 0);
        assert_eq!(
            bytes.len(),
            (scene_width() as usize) * (scene_height() as usize) * 4
        );
        assert!(
            bytes
                .chunks_exact(4)
                .any(|pixel| pixel[0] > pixel[1] && pixel[0] > pixel[2])
        );
        scene_fs::clear_files();
    }
}
