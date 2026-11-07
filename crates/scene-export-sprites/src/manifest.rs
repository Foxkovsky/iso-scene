//! `sheet.json`: the engine-neutral manifest.

use serde_json::{Value, json};

use crate::layout::{Cell, Page, cell_origin};
use crate::{MANIFEST_FORMAT, MANIFEST_VERSION, Projection, SheetOptions};

/// Degrees as a JSON number: an integer when whole, so quarter turns keep
/// their `0/90/180/270` form.
pub(crate) fn degrees_json(degrees: f64) -> Value {
    if degrees.fract() == 0.0 && degrees.abs() < 1e15 {
        json!(degrees as i64)
    } else {
        json!(degrees)
    }
}

pub(crate) fn manifest_json(options: &SheetOptions, pages: &[Page]) -> Value {
    // Every cell with the page it lives on, in page order.
    let located = || {
        pages
            .iter()
            .enumerate()
            .flat_map(|(index, page)| page.cells.iter().map(move |cell| (index, page, cell)))
    };
    let describe = |index: usize, page: &Page, cell: &Cell| {
        let mut value = cell_json(cell, page.cell_size, page.anchor, options.padding);
        if let Some(object) = value.as_object_mut() {
            object.insert("page".to_string(), json!(index));
        }
        value
    };
    let sprites: Vec<Value> = located()
        .filter(|(_, _, cell)| cell.animation.is_none())
        .map(|(index, page, cell)| {
            let mut sprite = describe(index, page, cell);
            if let Some(object) = sprite.as_object_mut() {
                object.insert("pivot".to_string(), json!(options.pivot.name()));
            }
            sprite
        })
        .collect();
    let animations: Vec<Value> = options
        .animations
        .iter()
        .enumerate()
        .map(|(number, animation)| {
            let frames: Vec<Value> = located()
                .filter(|(_, _, cell)| cell.animation == Some(number))
                .map(|(index, page, cell)| {
                    let mut frame = describe(index, page, cell);
                    if let Some(object) = frame.as_object_mut() {
                        object.remove("view");
                        object.insert("index".to_string(), json!(cell.column));
                        object.insert("angle".to_string(), json!(cell.angle));
                        object.insert("duration".to_string(), json!(cell.duration));
                        object.insert("pivot".to_string(), json!("origin"));
                    }
                    frame
                })
                .collect();
            json!({
                "name": animation.name,
                "object": animation.object,
                "from": animation.from,
                "to": animation.to,
                "rotation": animation.rotation,
                "duration": animation.duration,
                "loop": animation.looped,
                "frames": frames
            })
        })
        .collect();
    let page_list: Vec<Value> = pages
        .iter()
        .map(|page| {
            json!({
                "image": page.image_name(),
                "imageWidth": page.width,
                "imageHeight": page.height,
                "tileset": format!("{}.tsj", page.stem),
                "objectTileset": format!("{}.objects.tsj", page.stem),
                "cell": {"width": page.cell_size[0], "height": page.cell_size[1]},
                "anchor": {"x": page.anchor[0], "y": page.anchor[1]},
                "columns": page.columns,
                "rows": page.rows
            })
        })
        .collect();
    // The top-level grid fields describe the first page, which is the whole
    // sheet unless `--max-rows` split it.
    let first = page_list.first().cloned().unwrap_or(Value::Null);

    json!({
        "format": MANIFEST_FORMAT,
        "version": MANIFEST_VERSION,
        "image": first["image"],
        "imageWidth": first["imageWidth"],
        "imageHeight": first["imageHeight"],
        "name": options.name,
        "projection": options.projection.name(),
        "scale": options.scale,
        "tileWidth": options.tile_width,
        "views": options.views.iter().map(|view| degrees_json(*view)).collect::<Vec<_>>(),
        "pivot": options.pivot.name(),
        "cell": first["cell"],
        "padding": options.padding,
        "shadow": options.shadow && options.projection == Projection::Iso,
        "anchor": first["anchor"],
        "columns": first["columns"],
        "rows": first["rows"],
        "pages": page_list,
        "sprites": sprites,
        "animations": animations
    })
}

/// Yaw of a static view in degrees; for an animation frame only its quarter
/// turn (the animated part is reported as `angle`).
pub(crate) fn view_degrees(cell: &Cell) -> f64 {
    let quarter = f64::from(cell.rotation);
    if cell.animation.is_some() {
        quarter
    } else {
        quarter + cell.angle
    }
}

pub(crate) fn cell_json(cell: &Cell, cell_size: [u32; 2], anchor: [f64; 2], padding: u32) -> Value {
    let [x, y] = cell_origin(cell, cell_size, padding);
    let mut value = json!({
        "name": cell.name,
        "view": cell.column,
        "rotation": degrees_json(view_degrees(cell)),
        "x": x,
        "y": y,
        "w": cell_size[0],
        "h": cell_size[1],
        "anchor": {"x": anchor[0], "y": anchor[1]},
        "footprint": {"width": cell.footprint[0], "depth": cell.footprint[1]}
    });
    if let (Some(source), Some(object)) = (&cell.source, value.as_object_mut()) {
        object.insert("source".to_string(), json!(source));
    }
    value
}
