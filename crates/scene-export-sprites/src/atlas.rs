//! `<name>.atlas.json`: the `TexturePacker` "JSON Hash" atlas.

use serde_json::{Value, json};

use crate::SheetOptions;
use crate::layout::{Cell, Page, cell_origin};

/// Key of a cell in the atlas: `object/view` for a static view and
/// `animation#frame` for an animation frame.
pub(crate) fn atlas_key(options: &SheetOptions, cell: &Cell) -> String {
    match cell
        .animation
        .and_then(|index| options.animations.get(index))
    {
        Some(animation) => format!("{}#{}", animation.name, cell.column),
        None => format!("{}/{}", cell.name, cell.column),
    }
}

/// `TexturePacker` "JSON Hash" atlas, as loaded by Phaser (`load.atlas`) and
/// `PixiJS` (`Spritesheet`). `pivot` is the anchor as a fraction of the frame,
/// `animations` lists frame keys in order (`PixiJS`), and each frame carries
/// its `duration` like an Aseprite export.
pub(crate) fn atlas_json(options: &SheetOptions, page: &Page) -> Value {
    let (cells, cell_size, anchor) = (&page.cells, page.cell_size, page.anchor);
    let mut frames = serde_json::Map::new();
    for cell in cells {
        let [x, y] = cell_origin(cell, cell_size, options.padding);
        let mut frame = json!({
            "frame": {"x": x, "y": y, "w": cell_size[0], "h": cell_size[1]},
            "rotated": false,
            "trimmed": false,
            "spriteSourceSize": {"x": 0, "y": 0, "w": cell_size[0], "h": cell_size[1]},
            "sourceSize": {"w": cell_size[0], "h": cell_size[1]},
            "pivot": {
                "x": anchor[0] / f64::from(cell_size[0]),
                "y": anchor[1] / f64::from(cell_size[1])
            }
        });
        if let (Some(duration), Some(object)) = (cell.duration, frame.as_object_mut()) {
            object.insert("duration".to_string(), json!(duration));
        }
        frames.insert(atlas_key(options, cell), frame);
    }
    let mut animations = serde_json::Map::new();
    for (index, animation) in options.animations.iter().enumerate() {
        let keys: Vec<String> = cells
            .iter()
            .filter(|cell| cell.animation == Some(index))
            .map(|cell| atlas_key(options, cell))
            .collect();
        animations.insert(animation.name.clone(), json!(keys));
    }
    json!({
        "frames": frames,
        "animations": animations,
        "meta": {
            "app": "iso-scene",
            "version": env!("CARGO_PKG_VERSION"),
            "image": page.image_name(),
            "format": "RGBA8888",
            "size": {"w": page.width, "h": page.height},
            "scale": "1"
        }
    })
}
