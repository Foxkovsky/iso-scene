//! Export an isometric scene as a Tiled map with floor and object tiles.

#![allow(
    clippy::many_single_char_names,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss
)]

use std::fs;
use std::path::{Path, PathBuf};

use scene_backend_iso::{Sprite, render_object_sprite};
use scene_core::SceneGraph;
use scene_render::{Image, write_png};
use scene_syntax::Diagnostic;
use scene_syntax::ast::RenderKind;
use serde_json::{Value, json};

const PAD: f64 = 2.0;
const MAX_MAP_CELLS: u64 = 1_000_000;

/// Files written by [`export`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExportReport {
    pub map: PathBuf,
    pub floor_tileset: PathBuf,
    pub objects_tileset: PathBuf,
    pub preview: PathBuf,
    pub instances: usize,
    pub cells: [u32; 2],
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct SpriteKey {
    name: String,
    rotation: u16,
}

/// Export a scene to `out_dir`.
///
/// The result is an isometric Tiled map (`scene.tmj`), two tilesets, the
/// object atlas, a floor tile and a normal iso preview for comparison.
///
/// # Errors
///
/// Returns `E0604` for a non-iso scene, `E0602` for a map that is too large,
/// objects above the ground, two objects in one cell, or object placement
/// outside the map, and `E0603` for file errors.
#[allow(clippy::too_many_lines)]
pub fn export(scene: &SceneGraph, out_dir: &Path) -> Result<ExportReport, Diagnostic> {
    if scene.render != RenderKind::Iso {
        return Err(error(
            "E0604",
            format!("Tiled export needs an iso scene, got {:?}", scene.render),
        ));
    }
    fs::create_dir_all(out_dir).map_err(|cause| {
        error(
            "E0603",
            format!("cannot create `{}`: {cause}", out_dir.display()),
        )
    })?;

    let scale = scene.scale;
    let tile_w = scale
        .checked_mul(2)
        .ok_or_else(|| error("E0602", "scale is too large for a Tiled tileset"))?;
    let tile_h = scale;

    let mut keys = Vec::new();
    for instance in &scene.instances {
        let key = SpriteKey {
            name: instance.object.name.clone(),
            rotation: instance.rotation,
        };
        if !keys.contains(&key) {
            keys.push(key);
        }
    }

    let mut sprites: Vec<(SpriteKey, Sprite)> = Vec::with_capacity(keys.len());
    for key in keys {
        let Some(instance) = scene
            .instances
            .iter()
            .find(|instance| instance.object.name == key.name && instance.rotation == key.rotation)
        else {
            return Err(error("E0602", "internal sprite lookup failed"));
        };
        let sprite = render_object_sprite(
            instance.object.as_ref(),
            instance.rotation,
            scale,
            &scene.materials,
        )?;
        sprites.push((key, sprite));
    }

    let anchor_x = sprites
        .iter()
        .map(|(_, sprite)| sprite.anchor[0])
        .fold(f64::NEG_INFINITY, f64::max)
        .ceil()
        + PAD;
    let anchor_y = sprites
        .iter()
        .map(|(_, sprite)| sprite.anchor[1])
        .fold(f64::NEG_INFINITY, f64::max)
        .ceil()
        + PAD;

    let mut offsets = Vec::with_capacity(sprites.len());
    let mut canvas_w = 1_u32;
    let mut canvas_h = 1_u32;
    for (_, sprite) in &sprites {
        let offset_x = (anchor_x - sprite.anchor[0]).round().max(0.0) as u32;
        let offset_y = (anchor_y - sprite.anchor[1]).round().max(0.0) as u32;
        canvas_w = canvas_w.max(offset_x + sprite.image.width + 1);
        canvas_h = canvas_h.max(offset_y + sprite.image.height + 1);
        offsets.push((offset_x, offset_y));
    }

    let mut atlas = Image {
        width: canvas_w
            .checked_mul(sprites.len() as u32)
            .ok_or_else(|| error("E0602", "object atlas is too large"))?,
        height: canvas_h,
        rgba: vec![0; (canvas_w as usize) * (canvas_h as usize) * sprites.len() * 4],
    };
    for (index, ((_, sprite), (offset_x, offset_y))) in
        sprites.iter().zip(offsets.iter()).enumerate()
    {
        blit(
            &mut atlas,
            &sprite.image,
            *offset_x + index as u32 * canvas_w,
            *offset_y,
        );
    }

    let floor = floor_image(tile_w, tile_h);
    let floor_path = out_dir.join("floor_tile.png");
    write_png(&floor, &floor_path).map_err(|message| error("E0603", message))?;
    let atlas_path = out_dir.join("objects.png");
    write_png(&atlas, &atlas_path).map_err(|message| error("E0603", message))?;

    let floor_ts = json!({
        "type": "tileset",
        "name": "floor",
        "version": "1.10",
        "tiledversion": "1.12.2",
        "tilewidth": tile_w,
        "tileheight": tile_h,
        "tilecount": 1,
        "columns": 1,
        "image": "floor_tile.png",
        "imagewidth": tile_w,
        "imageheight": tile_h,
        "margin": 0,
        "spacing": 0,
        "tiles": [{
            "id": 0,
            "type": "floor",
            "properties": [
                {"name": "walkable", "type": "bool", "value": true},
                {"name": "collides", "type": "bool", "value": false}
            ]
        }]
    });
    let floor_ts_path = out_dir.join("floor.tsj");
    write_json(&floor_ts_path, &floor_ts)?;

    let object_tiles: Vec<Value> = sprites
        .iter()
        .enumerate()
        .map(|(index, (key, _))| {
            json!({
                "id": index,
                "type": key.name,
                "properties": [
                    {"name": "rotation", "type": "int", "value": key.rotation},
                    {"name": "walkable", "type": "bool", "value": false},
                    {"name": "collides", "type": "bool", "value": true}
                ]
            })
        })
        .collect();
    let object_ts = json!({
        "type": "tileset",
        "name": "scene_objects",
        "version": "1.10",
        "tiledversion": "1.12.2",
        "tilewidth": canvas_w,
        "tileheight": canvas_h,
        "tilecount": sprites.len(),
        "columns": sprites.len(),
        "image": "objects.png",
        "imagewidth": canvas_w * sprites.len() as u32,
        "imageheight": canvas_h,
        "margin": 0,
        "spacing": 0,
        "tileoffset": {"x": 0, "y": 0},
        "tiles": object_tiles
    });
    let object_ts_path = out_dir.join("objects.tsj");
    write_json(&object_ts_path, &object_ts)?;

    let min_x = scene.bounds.min[0].floor() as i64;
    let min_y = scene.bounds.min[1].floor() as i64;
    let width = ((scene.bounds.max[0].ceil() as i64) - min_x).max(1) as u32;
    let height = ((scene.bounds.max[1].ceil() as i64) - min_y).max(1) as u32;
    if u64::from(width) * u64::from(height) > MAX_MAP_CELLS {
        return Err(error(
            "E0602",
            format!("Tiled map {width}x{height} is too large"),
        ));
    }

    let floor_data = vec![1_u32; (width * height) as usize];
    let mut ordered: Vec<&scene_core::Instance> = scene.instances.iter().collect();
    ordered.sort_by(|a, b| {
        let left = a.at[0] + a.object.width + a.at[1] + a.object.depth + a.at[2];
        let right = b.at[0] + b.object.width + b.at[1] + b.object.depth + b.at[2];
        left.total_cmp(&right)
    });

    let mut objects = Vec::with_capacity(ordered.len());
    for (index, instance) in ordered.iter().enumerate() {
        let sprite_index = sprites
            .iter()
            .position(|(key, _)| {
                key.name == instance.object.name && key.rotation == instance.rotation
            })
            .ok_or_else(|| error("E0602", "internal sprite index failed"))?;
        let local_x = instance.at[0] - min_x as f64;
        let local_y = instance.at[1] - min_y as f64;
        let anchor_screen = [
            (local_x - local_y) * f64::from(scale) + f64::from(height * scale),
            local_x.midpoint(local_y) * f64::from(scale) - instance.at[2] * f64::from(scale),
        ];
        let object_x = anchor_screen[0] - anchor_x;
        let object_y = anchor_screen[1] - anchor_y + f64::from(canvas_h);
        objects.push(json!({
            "id": index + 1,
            "gid": 2 + sprite_index as u32,
            "x": object_x,
            "y": object_y,
            "width": canvas_w,
            "height": canvas_h,
            "name": instance.object.name,
            "type": instance.object.name,
            "visible": true,
            "properties": [
                {"name": "rotation", "type": "int", "value": instance.rotation},
                {"name": "z", "type": "float", "value": instance.at[2]}
            ]
        }));
    }

    let map = json!({
        "type": "map",
        "version": "1.10",
        "tiledversion": "1.12.2",
        "orientation": "isometric",
        "renderorder": "right-down",
        "width": width,
        "height": height,
        "tilewidth": tile_w,
        "tileheight": tile_h,
        "infinite": false,
        "nextlayerid": 3,
        "nextobjectid": objects.len() + 1,
        "tilesets": [
            {"firstgid": 1, "source": "floor.tsj"},
            {"firstgid": 2, "source": "objects.tsj"}
        ],
        "layers": [
            {
                "type": "tilelayer",
                "name": "floor",
                "id": 1,
                "width": width,
                "height": height,
                "x": 0,
                "y": 0,
                "opacity": 1,
                "visible": true,
                "data": floor_data
            },
            {
                "type": "objectgroup",
                "name": "objects",
                "id": 2,
                "x": 0,
                "y": 0,
                "opacity": 1,
                "visible": true,
                "draworder": "index",
                "objects": objects
            }
        ]
    });
    let map_path = out_dir.join("scene.tmj");
    write_json(&map_path, &map)?;

    let preview = scene_backend_iso::render(scene)?;
    let preview_path = out_dir.join("preview.png");
    write_png(&preview, &preview_path).map_err(|message| error("E0603", message))?;

    Ok(ExportReport {
        map: map_path,
        floor_tileset: floor_ts_path,
        objects_tileset: object_ts_path,
        preview: preview_path,
        instances: scene.instances.len(),
        cells: [width, height],
    })
}

fn write_json(path: &Path, value: &Value) -> Result<(), Diagnostic> {
    fs::write(
        path,
        serde_json::to_string_pretty(value).unwrap_or_default() + "\n",
    )
    .map_err(|cause| {
        error(
            "E0603",
            format!("cannot write `{}`: {cause}", path.display()),
        )
    })
}

fn floor_image(width: u32, height: u32) -> Image {
    let mut image = Image {
        width,
        height,
        rgba: vec![0; (width * height * 4) as usize],
    };
    let half_w = f64::from(width) / 2.0;
    let half_h = f64::from(height) / 2.0;
    for y in 0..height {
        for x in 0..width {
            let dx = ((f64::from(x) + 0.5) - half_w).abs() / half_w;
            let dy = ((f64::from(y) + 0.5) - half_h).abs() / half_h;
            let distance = dx + dy;
            if distance > 1.0 {
                continue;
            }
            let color = if distance > 0.94 {
                [0x6E, 0x72, 0x78, 255]
            } else {
                [0xB9, 0xBC, 0xC1, 255]
            };
            let index = ((y * width + x) * 4) as usize;
            image.rgba[index..index + 4].copy_from_slice(&color);
        }
    }
    image
}

fn blit(dst: &mut Image, src: &Image, offset_x: u32, offset_y: u32) {
    for y in 0..src.height {
        for x in 0..src.width {
            let source = ((y * src.width + x) * 4) as usize;
            if src.rgba[source + 3] == 0 {
                continue;
            }
            let target_x = offset_x + x;
            let target_y = offset_y + y;
            if target_x >= dst.width || target_y >= dst.height {
                continue;
            }
            let target = ((target_y * dst.width + target_x) * 4) as usize;
            dst.rgba[target..target + 4].copy_from_slice(&src.rgba[source..source + 4]);
        }
    }
}

fn error(code: &'static str, message: impl Into<String>) -> Diagnostic {
    Diagnostic::new(code, message, scene_syntax::Span::new(0, 0))
}
