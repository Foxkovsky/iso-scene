//! `<name>.tsj` and `<name>.objects.tsj`: Tiled tilesets.

use serde_json::{Value, json};

use crate::SheetOptions;
use crate::layout::Page;
use crate::manifest::{degrees_json, view_degrees};

/// Tiled layer a tileset is tuned for.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum TiledLayer {
    /// Tile layers draw an image from the left corner of its cell.
    Tiles,
    /// Tile objects on an isometric map are already centred on their
    /// position, and Tiled adds `tileoffset` on top.
    Objects,
}

pub(crate) fn tileset_json(options: &SheetOptions, page: &Page, layer: TiledLayer) -> Value {
    let (cells, cell_size, anchor) = (&page.cells, page.cell_size, page.anchor);
    let (columns, rows) = (page.columns, page.rows);
    let mut tiles: Vec<Value> = cells
        .iter()
        .filter(|cell| cell.animation.is_none())
        .map(|cell| {
            json!({
                "id": cell.row * columns + cell.column,
                "type": cell.name,
                "properties": [
                    {"name": "name", "type": "string", "value": cell.name},
                    {"name": "view", "type": "int", "value": cell.column},
                    {
                        "name": "rotation",
                        "type": if view_degrees(cell).fract() == 0.0 { "int" } else { "float" },
                        "value": degrees_json(view_degrees(cell))
                    },
                    {"name": "pivot", "type": "string", "value": options.pivot.name()},
                    {"name": "anchor-x", "type": "float", "value": anchor[0]},
                    {"name": "anchor-y", "type": "float", "value": anchor[1]}
                ]
            })
        })
        .collect();

    for (index, animation) in options.animations.iter().enumerate() {
        let frames: Vec<Value> = cells
            .iter()
            .filter(|cell| cell.animation == Some(index))
            .map(|cell| {
                json!({
                    "tileid": cell.row * columns + cell.column,
                    "duration": cell.duration.unwrap_or(0)
                })
            })
            .collect();
        if let Some(first) = cells
            .iter()
            .find(|cell| cell.animation == Some(index) && cell.column == 0)
        {
            // Tiled always loops tile animations; `loop` tells the game.
            tiles.push(json!({
                "id": first.row * columns + first.column,
                "type": animation.object,
                "properties": [
                    {"name": "name", "type": "string", "value": animation.name},
                    {"name": "object", "type": "string", "value": animation.object},
                    {"name": "loop", "type": "bool", "value": animation.looped},
                    {"name": "pivot", "type": "string", "value": "origin"},
                    {"name": "anchor-x", "type": "float", "value": anchor[0]},
                    {"name": "anchor-y", "type": "float", "value": anchor[1]}
                ],
                "animation": frames
            }));
        }
    }
    tiles.sort_by_key(|tile| tile["id"].as_u64().unwrap_or(0));

    // Checked with `tmxrasterizer`: in a tile layer the image's left edge sits
    // on the cell's left corner, so the anchor needs this shift to reach the
    // cell's bottom vertex; a bottom-aligned tile object needs none.
    let offset_x = match layer {
        TiledLayer::Tiles => options.tile_width.map_or(0, |tile_width| {
            (i64::from(tile_width) - i64::from(cell_size[0])) / 2
        }),
        TiledLayer::Objects => 0,
    };
    // The anchor sits `cell_size[1] - anchor[1]` pixels above the cell bottom;
    // shift the image down by that much so the anchor lands on the tile cell.
    let offset_y = (f64::from(cell_size[1]) - anchor[1]).round() as i64;
    json!({
        "type": "tileset",
        "name": page.stem,
        "version": "1.10",
        "tiledversion": "1.12.2",
        "tilewidth": cell_size[0],
        "tileheight": cell_size[1],
        "tilecount": columns * rows,
        "columns": columns,
        "image": page.image_name(),
        "imagewidth": page.width,
        "imageheight": page.height,
        "margin": options.padding,
        "spacing": options.padding,
        "tileoffset": {"x": offset_x, "y": offset_y},
        "objectalignment": "bottom",
        "tiles": tiles
    })
}
