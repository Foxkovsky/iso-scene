//! Export an isometric scene as a Tiled map with floor and object tiles.

#![allow(
    clippy::many_single_char_names,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss
)]

mod collision;

use std::fs;
use std::path::{Path, PathBuf};

use collision::Footprint;
pub use collision::{Collision, MAX_FOOTPRINT_CELLS};

use rayon::prelude::*;
use scene_backend_iso::{Sprite, project_point, render_object_sprite};
use scene_core::SceneGraph;
use scene_render::{Image, write_png};
use scene_syntax::Diagnostic;
use scene_syntax::ast::RenderKind;
use scene_walls::{
    WallCell, WallLayout, layer_data_where, render_shadow_tiles, render_tiles, shadow_layer_data,
    wangid,
};
use serde_json::{Value, json};

const MAX_MAP_CELLS: u64 = 1_000_000;

/// Most objects in one cell with `--tile-layer`: each needs its own layer
/// (`docs/spec.md` §9).
pub const MAX_OBJECT_TILE_LAYERS: usize = 16;

/// Move an object anchor just inside its cell; see the export loop.
const ANCHOR_NUDGE: f64 = 0.001;

/// Where the wall layer goes relative to the objects.
///
/// Tiled draws whole layers in order, so a tile layer cannot interleave with
/// the depth-sorted objects.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum WallOrder {
    /// One `walls` layer below the objects: a door in a wall gap stays
    /// whole, but objects are drawn over the near walls too.
    Behind,
    /// One `walls` layer above the objects, for rooms without near walls.
    Front,
    /// `walls-back` below the objects and `walls-front` above them. A wall
    /// goes to the front when a floor cell lies right behind it (see
    /// [`WallCell::front`]), so near walls hide the furniture behind them.
    #[default]
    Split,
}

/// Files written by [`export`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExportReport {
    pub map: PathBuf,
    pub floor_tileset: PathBuf,
    pub objects_tileset: Option<PathBuf>,
    pub walls_tileset: Option<PathBuf>,
    pub preview: Option<PathBuf>,
    pub instances: usize,
    pub cells: [u32; 2],
    /// Overlapping objects and objects on wall cells, sorted.
    pub collisions: Vec<Collision>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct SpriteKey {
    name: String,
    rotation: u16,
}

struct SpriteEntry {
    key: SpriteKey,
    sprite: Sprite,
    bottom_world: [f64; 3],
    bottom_px: [f64; 2],
}

/// Export a scene to `out_dir`.
///
/// The result is an isometric Tiled map (`scene.tmj`), the floor tileset, a
/// floor tile and a normal iso preview for comparison. Scenes with objects
/// also get an object atlas and the `objects` layer; scenes without objects
/// export only the floor (and walls, when given).
///
/// # Errors
///
/// Returns `E0604` for a non-iso scene, `E0602` for a map that is too large
/// or footprints covering more than [`MAX_FOOTPRINT_CELLS`] cells, and
/// `E0603` for file errors. Collisions are not errors; they are listed in
/// [`ExportReport::collisions`].
#[allow(clippy::too_many_lines)]
pub fn export(scene: &SceneGraph, out_dir: &Path) -> Result<ExportReport, Diagnostic> {
    export_with_walls(scene, None, out_dir)
}

/// Export a scene with an optional generated wall layer.
///
/// # Errors
///
/// Returns the same diagnostics as [`export`], plus wall tileset failures.
#[allow(clippy::too_many_lines)]
pub fn export_with_walls(
    scene: &SceneGraph,
    walls: Option<&WallLayout>,
    out_dir: &Path,
) -> Result<ExportReport, Diagnostic> {
    export_with(scene, walls, false, WallOrder::default(), out_dir)
}

/// Export a scene with an optional wall layer and object layout choice.
///
/// With `objects_as_tiles` the sprites go into a tile layer named `objects`
/// (one tile per cell, Tiled tile alignment) instead of an object layer.
/// `wall_order` decides where the wall layer goes relative to the objects; see
/// [`WallOrder`].
///
/// # Errors
///
/// Returns the same diagnostics as [`export`].
#[allow(clippy::too_many_lines)]
pub fn export_with(
    scene: &SceneGraph,
    walls: Option<&WallLayout>,
    objects_as_tiles: bool,
    wall_order: WallOrder,
    out_dir: &Path,
) -> Result<ExportReport, Diagnostic> {
    if scene.render != RenderKind::Iso {
        return Err(error(
            "E0604",
            format!("Tiled export needs an iso scene, got {:?}", scene.render),
        ));
    }
    // Cheap and bounded: check it before rendering any sprite.
    let collisions = collision::find(&scene.instances, walls)?;
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

    let wall_tileset_path = if let Some(layout) = walls {
        let tiles = render_tiles(layout, scale);
        let image_path = out_dir.join("walls.png");
        write_png(&tiles.image, &image_path).map_err(|message| error("E0603", message))?;
        let tileset = json!({
            "type": "tileset",
            "name": "walls",
            "version": "1.10",
            "tiledversion": "1.12.2",
            "tilewidth": tiles.tile_width,
            "tileheight": tiles.tile_height,
            "tilecount": 16,
            "columns": 16,
            "image": "walls.png",
            "imagewidth": tiles.tile_width * 16,
            "imageheight": tiles.tile_height,
            "margin": 0,
            "spacing": 0,
            "tileoffset": {"x": 0, "y": 0},
            "tiles": (0..16_u8).map(|mask| json!({
                "id": mask,
                "type": "wall",
                "properties": [
                    {"name": "mask", "type": "int", "value": mask},
                    {"name": "connections", "type": "int", "value": 4 - mask.count_ones()},
                    {"name": "walkable", "type": "bool", "value": false},
                    {"name": "collides", "type": "bool", "value": true}
                ]
            })).collect::<Vec<_>>(),
            "wangsets": [{
                "name": "wall",
                "type": "edge",
                "tile": -1,
                "colors": [{"name": "wall", "color": "#7a8794", "tile": -1, "probability": 1}],
                "wangtiles": (0..16_u8).map(|mask| json!({
                    "tileid": mask,
                    "wangid": wangid(mask)
                })).collect::<Vec<_>>()
            }]
        });
        let path = out_dir.join("walls.tsj");
        write_json(&path, &tileset)?;
        write_shadow_tileset(scale, out_dir)?;
        Some(path)
    } else {
        None
    };

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

    // Sprites are independent: render them in parallel, keep key order, and
    // let the first failing key (in order) decide the error.
    let rendered: Vec<Result<SpriteEntry, Diagnostic>> = keys
        .into_par_iter()
        .map(|key| {
            let Some(instance) = scene.instances.iter().find(|instance| {
                instance.object.name == key.name && instance.rotation == key.rotation
            }) else {
                return Err(error("E0602", "internal sprite lookup failed"));
            };
            let sprite = render_object_sprite(
                instance.object.as_ref(),
                instance.rotation,
                scale,
                &scene.materials,
            )?;
            let zero = scene_core::Instance {
                object: instance.object.clone(),
                at: [0.0, 0.0, 0.0],
                rotation: instance.rotation,
                angle: 0.0,
            };
            let bounds = zero.world_bounds();
            let bottom_world = [bounds.max[0], bounds.max[1], bounds.min[2]];
            // The sprite anchor is the pixel of the object's local origin,
            // which a quarter turn moves to another corner of the footprint.
            let origin = zero.to_world([0.0, 0.0, 0.0]);
            let projected = project_point(
                [
                    bottom_world[0] - origin[0],
                    bottom_world[1] - origin[1],
                    bottom_world[2],
                ],
                f64::from(scale),
            );
            let bottom_px = [
                sprite.anchor[0] + projected[0],
                sprite.anchor[1] + projected[1],
            ];
            Ok(SpriteEntry {
                key,
                sprite,
                bottom_world,
                bottom_px,
            })
        })
        .collect();
    let sprites = rendered
        .into_iter()
        .collect::<Result<Vec<SpriteEntry>, Diagnostic>>()?;

    let max_left = sprites
        .iter()
        .map(|entry| entry.bottom_px[0])
        .fold(0.0_f64, f64::max);
    let max_right = sprites
        .iter()
        .map(|entry| f64::from(entry.sprite.image.width) - entry.bottom_px[0])
        .fold(0.0_f64, f64::max);
    let max_up = sprites
        .iter()
        .map(|entry| entry.bottom_px[1])
        .fold(0.0_f64, f64::max);
    let mut canvas_w = (2.0 * max_left.max(max_right)).ceil().max(2.0) as u32 + 2;
    if canvas_w % 2 == 1 {
        canvas_w += 1;
    }
    let canvas_h = max_up.ceil().max(2.0) as u32 + 1;
    let canvas_anchor = [f64::from(canvas_w) / 2.0, f64::from(canvas_h)];
    let mut offsets = Vec::with_capacity(sprites.len());
    for entry in &sprites {
        let offset_x = (canvas_anchor[0] - entry.bottom_px[0]).round().max(0.0) as u32;
        let offset_y = (canvas_anchor[1] - entry.bottom_px[1]).round().max(0.0) as u32;
        offsets.push((offset_x, offset_y));
    }

    let has_objects = !sprites.is_empty();

    let floor = floor_image(tile_w, tile_h);
    let floor_path = out_dir.join("floor_tile.png");
    write_png(&floor, &floor_path).map_err(|message| error("E0603", message))?;

    if has_objects {
        let mut atlas = Image {
            width: canvas_w
                .checked_mul(sprites.len() as u32)
                .ok_or_else(|| error("E0602", "object atlas is too large"))?,
            height: canvas_h,
            rgba: vec![0; (canvas_w as usize) * (canvas_h as usize) * sprites.len() * 4],
        };
        for (index, (entry, (offset_x, offset_y))) in sprites.iter().zip(offsets.iter()).enumerate()
        {
            blit(
                &mut atlas,
                &entry.sprite.image,
                *offset_x + index as u32 * canvas_w,
                *offset_y,
            );
        }
        let path = out_dir.join("objects.png");
        write_png(&atlas, &path).map_err(|message| error("E0603", message))?;
    }

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

    let object_ts_path = if has_objects {
        let object_tiles: Vec<Value> = sprites
            .iter()
            .enumerate()
            .map(|(index, key)| {
                json!({
                    "id": index,
                    "type": key.key.name,
                    "properties": [
                        {"name": "rotation", "type": "int", "value": key.key.rotation},
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
            "tileoffset": if objects_as_tiles {
                json!({"x": (i64::from(tile_w) - i64::from(canvas_w)) / 2, "y": 0})
            } else {
                json!({"x": 0, "y": 0})
            },
            "tiles": object_tiles
        });
        let path = out_dir.join("objects.tsj");
        write_json(&path, &object_ts)?;
        Some(path)
    } else {
        None
    };

    let wall_columns = walls.map_or(0.0, |layout| f64::from(layout.columns));
    let wall_rows = walls.map_or(0.0, |layout| f64::from(layout.rows));
    let min_x = scene.bounds.min[0].floor().min(0.0) as i64;
    let min_y = scene.bounds.min[1].floor().min(0.0) as i64;
    let mut max_x = scene.bounds.max[0].ceil().max(wall_columns);
    let mut max_y = scene.bounds.max[1].ceil().max(wall_rows);
    // An explicit `canvas` also fixes the map rectangle, so the cell grid stays
    // stable when objects move around inside it.
    if let Some(canvas) = scene.canvas {
        max_x = max_x.max(canvas[0].ceil());
        max_y = max_y.max(canvas[1].ceil());
    }
    let width = ((max_x as i64) - min_x).max(1) as u32;
    let height = ((max_y as i64) - min_y).max(1) as u32;
    if u64::from(width) * u64::from(height) > MAX_MAP_CELLS {
        return Err(error(
            "E0602",
            format!("Tiled map {width}x{height} is too large"),
        ));
    }

    let floor_data = vec![1_u32; (width * height) as usize];
    let wall_first_gid = 2 + sprites.len() as u32;
    let mut ordered: Vec<&scene_core::Instance> = scene.instances.iter().collect();
    ordered.sort_by(|a, b| {
        let left = a.at[0] + a.object.width + a.at[1] + a.object.depth + a.at[2];
        let right = b.at[0] + b.object.width + b.at[1] + b.object.depth + b.at[2];
        left.total_cmp(&right)
    });

    let mut objects = Vec::with_capacity(ordered.len());
    let mut footprints = Vec::with_capacity(ordered.len());
    let mut tile_cells = Vec::new();
    for (index, instance) in ordered.iter().enumerate() {
        let sprite_index = sprites
            .iter()
            .position(|entry| {
                entry.key.name == instance.object.name && entry.key.rotation == instance.rotation
            })
            .ok_or_else(|| error("E0602", "internal sprite index failed"))?;
        let entry = &sprites[sprite_index];
        let footprint = Footprint::of(instance);
        let [x0, y0, x1, y1] = footprint.area;
        let bottom_world = [
            instance.at[0] + entry.bottom_world[0],
            instance.at[1] + entry.bottom_world[1],
            instance.at[2] + entry.bottom_world[2],
        ];
        if objects_as_tiles {
            // The sprite's base is the footprint corner; the cell that owns that
            // corner is the one whose bottom vertex it is.
            let cell_x = (bottom_world[0] - 1.0).floor() as i64 - min_x;
            let cell_y = (bottom_world[1] - 1.0).floor() as i64 - min_y;
            if cell_x >= 0 && cell_y >= 0 && cell_x < i64::from(width) && cell_y < i64::from(height)
            {
                let at = (cell_y as usize) * (width as usize) + cell_x as usize;
                tile_cells.push((at, 2 + sprite_index as u32));
            }
        }
        // Tiled stores isometric object positions unprojected: both axes count
        // `tileheight` pixels per cell, and Tiled projects them itself. An
        // elevation of `z` blocks lifts the sprite by `z * tileheight` screen
        // pixels, which is `z` cells back along both axes.
        let cell_x = bottom_world[0] - min_x as f64 - bottom_world[2];
        let cell_y = bottom_world[1] - min_y as f64 - bottom_world[2];
        // Tiled derives an object's cell from its position. A footprint corner
        // lies exactly on a cell border, where Tiled rounds to the next cell
        // and highlights the wrong diamond. Nudging the anchor a thousandth of
        // a pixel back moves it inside the cell the object stands on.
        let object_x = cell_x * f64::from(tile_h) - ANCHOR_NUDGE;
        let object_y = cell_y * f64::from(tile_h) - ANCHOR_NUDGE;
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
                {"name": "z", "type": "float", "value": instance.at[2]},
                // The footprint in cells, relative to the stored position, so
                // it follows the object when it is moved in Tiled.
                {"name": "footprint_x", "type": "float",
                 "value": x0 - min_x as f64 - object_x / f64::from(tile_h)},
                {"name": "footprint_y", "type": "float",
                 "value": y0 - min_y as f64 - object_y / f64::from(tile_h)},
                {"name": "footprint_w", "type": "float", "value": x1 - x0},
                {"name": "footprint_h", "type": "float", "value": y1 - y0},
                {"name": "bottom", "type": "float", "value": footprint.height[0]},
                {"name": "top", "type": "float", "value": footprint.height[1]}
            ]
        }));
        // The footprint as an isometric rectangle: Tiled draws it as the
        // diamond the object stands on.
        let mut properties = vec![
            json!({"name": "bottom", "type": "float", "value": footprint.height[0]}),
            json!({"name": "top", "type": "float", "value": footprint.height[1]}),
        ];
        if !objects_as_tiles {
            properties.push(json!({"name": "object", "type": "object", "value": index + 1}));
        }
        footprints.push(json!({
            "id": ordered.len() + index + 1,
            "name": instance.object.name,
            "type": "footprint",
            "x": (x0 - min_x as f64) * f64::from(tile_h),
            "y": (y0 - min_y as f64) * f64::from(tile_h),
            "width": (x1 - x0) * f64::from(tile_h),
            "height": (y1 - y0) * f64::from(tile_h),
            "visible": true,
            "properties": properties
        }));
    }

    let object_tile_layers = if objects_as_tiles {
        stack_tiles(&tile_cells, (width as usize) * (height as usize))?
    } else {
        Vec::new()
    };

    let mut tilesets = vec![json!({"firstgid": 1, "source": "floor.tsj"})];
    if has_objects {
        tilesets.push(json!({"firstgid": 2, "source": "objects.tsj"}));
    }
    if walls.is_some() {
        tilesets.push(json!({"firstgid": wall_first_gid, "source": "walls.tsj"}));
        tilesets.push(json!({"firstgid": wall_first_gid + 16, "source": "wall_shadow.tsj"}));
    }
    let tile_layer = |name: &str, data: Vec<u32>| {
        json!({
            "type": "tilelayer",
            "name": name,
            "width": width,
            "height": height,
            "x": 0,
            "y": 0,
            "opacity": 1,
            "visible": true,
            "data": data
        })
    };
    let wall_layer = |name: &str, keep: fn(&WallCell) -> bool| {
        walls.map(|layout| {
            tile_layer(
                name,
                layer_data_where(layout, width, height, min_x, min_y, wall_first_gid, keep),
            )
        })
    };
    let (below, above) = match wall_order {
        WallOrder::Behind => (wall_layer("walls", |_| true), None),
        WallOrder::Front => (None, wall_layer("walls", |_| true)),
        WallOrder::Split => (
            wall_layer("walls-back", |cell| !cell.front),
            wall_layer("walls-front", |cell| cell.front),
        ),
    };
    let mut layers = vec![tile_layer("floor", floor_data)];
    // Contact shadows lie on the floor, under every wall layer and object.
    layers.extend(walls.map(|layout| {
        tile_layer(
            "wall-shadow",
            shadow_layer_data(layout, width, height, min_x, min_y, wall_first_gid + 16),
        )
    }));
    layers.extend(below);
    if has_objects && objects_as_tiles {
        for (index, data) in object_tile_layers.into_iter().enumerate() {
            let name = match index {
                0 => "objects".to_owned(),
                _ => format!("objects-{}", index + 1),
            };
            layers.push(tile_layer(&name, data));
        }
    } else if has_objects {
        layers.push(json!({
                "type": "objectgroup",
                "name": "objects",
                "x": 0,
                "y": 0,
                "opacity": 1,
                "visible": true,
                "draworder": "index",
                "objects": objects
        }));
    }
    layers.extend(above);
    if has_objects {
        // Hidden by default: switch it on in Tiled to see what each object
        // occupies; games read it as collision shapes.
        layers.push(json!({
            "type": "objectgroup",
            "name": "collision",
            "x": 0,
            "y": 0,
            "opacity": 0.5,
            "visible": false,
            "draworder": "index",
            "objects": footprints
        }));
    }
    for (id, layer) in (1_u32..).zip(&mut layers) {
        layer["id"] = json!(id);
    }
    let next_layer_id = layers.len() + 1;

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
        "nextlayerid": next_layer_id,
        "nextobjectid": 2 * ordered.len() + 1,
        "tilesets": tilesets,
        "layers": layers
    });
    let map_path = out_dir.join("scene.tmj");
    write_json(&map_path, &map)?;

    let preview_path = if has_objects {
        let preview = scene_backend_iso::render(scene)?;
        let path = out_dir.join("preview.png");
        write_png(&preview, &path).map_err(|message| error("E0603", message))?;
        Some(path)
    } else {
        None
    };

    Ok(ExportReport {
        map: map_path,
        floor_tileset: floor_ts_path,
        objects_tileset: object_ts_path,
        walls_tileset: wall_tileset_path,
        preview: preview_path,
        instances: scene.instances.len(),
        collisions,
        cells: [width, height],
    })
}

/// Spread object tiles over as many tile layers as the busiest cell needs.
///
/// `tiles` holds `(cell, gid)` in depth order; the n-th object in a cell goes
/// to the n-th layer, so nearer objects are drawn later. The layer count is
/// checked against [`MAX_OBJECT_TILE_LAYERS`] before any layer is allocated.
fn stack_tiles(tiles: &[(usize, u32)], cells: usize) -> Result<Vec<Vec<u32>>, Diagnostic> {
    let mut sorted: Vec<usize> = tiles.iter().map(|&(cell, _)| cell).collect();
    sorted.sort_unstable();
    let depth = sorted
        .chunk_by(|a, b| a == b)
        .map(<[usize]>::len)
        .max()
        .unwrap_or(0);
    if depth > MAX_OBJECT_TILE_LAYERS {
        return Err(error(
            "E0602",
            format!(
                "{depth} objects share one cell; a tile layer export allows at most \
                 {MAX_OBJECT_TILE_LAYERS} (use the object layer instead)"
            ),
        ));
    }
    let mut layers = vec![vec![0_u32; cells]; depth.max(1)];
    for &(cell, gid) in tiles {
        if let Some(layer) = layers.iter_mut().find(|layer| layer[cell] == 0) {
            layer[cell] = gid;
        }
    }
    Ok(layers)
}

/// Write `wall_shadow.png` and `wall_shadow.tsj`: floor-sized tiles that
/// darken the floor along the walls next to it.
fn write_shadow_tileset(scale: u32, out_dir: &Path) -> Result<(), Diagnostic> {
    let tiles = render_shadow_tiles(scale);
    write_png(&tiles.image, &out_dir.join("wall_shadow.png"))
        .map_err(|message| error("E0603", message))?;
    let tileset = json!({
        "type": "tileset",
        "name": "wall_shadow",
        "version": "1.10",
        "tiledversion": "1.12.2",
        "tilewidth": tiles.tile_width,
        "tileheight": tiles.tile_height,
        "tilecount": 16,
        "columns": 16,
        "image": "wall_shadow.png",
        "imagewidth": tiles.tile_width * 16,
        "imageheight": tiles.tile_height,
        "margin": 0,
        "spacing": 0,
        "tiles": (1..16_u8).map(|mask| json!({
            "id": mask,
            "type": "wall-shadow",
            "properties": [{"name": "walls", "type": "int", "value": mask}]
        })).collect::<Vec<_>>()
    });
    write_json(&out_dir.join("wall_shadow.tsj"), &tileset)
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
