//! Turn a Tiled map back into a scene, its wall layout and the model files.
//!
//! Paths are taken as given: the CLI and the MCP server resolve them against
//! their project root before calling in.

use std::collections::BTreeSet;
use std::fmt::Display;
use std::fs;
use std::path::{Path, PathBuf};

use scene_walls::MAX_WALL_HEIGHT;
use serde_json::Value;

/// What to restore from a Tiled map.
pub struct ImportOptions {
    /// The `.tmj`/Tiled JSON map.
    pub map: PathBuf,
    /// Directory the referenced `.model` files are copied from.
    pub models: Option<PathBuf>,
    /// Object/tile layers to import; `None` means `objects`.
    pub layers: Option<Vec<String>>,
    /// Pixels per block; inferred from the map when absent.
    pub scale: Option<u32>,
    /// Scene units per Tiled cell; inferred from the map when absent.
    pub cell: Option<f64>,
    /// Directory for the scene, `.walls` and copied models.
    pub output: PathBuf,
    /// Scene file stem.
    pub name: String,
}

/// Files written by [`import`] and what they contain.
pub struct ImportReport {
    pub scene: PathBuf,
    pub walls: Option<PathBuf>,
    pub wall_height: Option<u32>,
    pub placements: usize,
    pub models: usize,
    pub scale: u32,
    pub cell: f64,
    /// Warnings the caller should show (e.g. walls that do not fit `.walls`).
    pub notes: Vec<String>,
}

type Parsed<T> = Result<T, String>;

fn usage(message: impl Display) -> String {
    message.to_string()
}

fn read(path: &str) -> Parsed<String> {
    fs::read_to_string(path).map_err(|error| format!("cannot read `{path}`: {error}"))
}

/// Restore a map as `<name>.scene`, `<name>.walls` (when it has walls) and the
/// referenced models next to them.
///
/// # Errors
///
/// Returns a readable message for an unreadable map, a missing model or a
/// write failure.
#[allow(clippy::too_many_lines)]
pub fn import(options: &ImportOptions) -> Parsed<ImportReport> {
    let map_path = options.map.to_string_lossy().into_owned();
    let models = options.models.clone();
    let layers = options.layers.clone();
    let scale = options.scale;
    let cell = options.cell;
    let output = options.output.clone();
    let name = options.name.clone();
    let mut notes = Vec::new();

    let source = read(&map_path)?;
    let map: Value = serde_json::from_str(&source)
        .map_err(|error| usage(format!("cannot parse `{map_path}`: {error}")))?;
    if map["type"].as_str() != Some("map") || map["orientation"].as_str() != Some("isometric") {
        return Err(usage("import-tiled expects an isometric Tiled JSON map"));
    }
    let map_dir = Path::new(&map_path)
        .parent()
        .unwrap_or_else(|| Path::new("."));
    let tile_height = map["tileheight"].as_f64().unwrap_or(32.0);
    if tile_height <= 0.0 {
        return Err(usage("map tileheight must be positive"));
    }
    let tilesets = load_tilesets(&map, map_dir)?;
    let selected = layers.unwrap_or_else(|| vec!["objects".to_string()]);
    let all = selected.iter().any(|layer| layer == "all");
    let has_tile_models = map["layers"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|layer| layer["type"] == "tilelayer")
        .filter(|layer| {
            all || selected
                .iter()
                .any(|name| Some(name.as_str()) == layer["name"].as_str())
        })
        .filter_map(|layer| layer["data"].as_array())
        .flatten()
        .filter_map(Value::as_u64)
        .filter_map(|gid| tile_info(&tilesets, gid as u32))
        .any(|(model, ..)| !model.is_empty());
    // A map written by `iso-scene tiled` says so: its tiles reference models
    // in blocks, while a tileset from the university game uses pixel units.
    let blocks = property(&map, "iso_units").and_then(Value::as_str) == Some("blocks");
    let map_scale = property(&map, "iso_scale")
        .and_then(Value::as_u64)
        .and_then(|value| u32::try_from(value).ok());
    let object_mode = !has_tile_models;
    let scale = scale.unwrap_or_else(|| {
        if object_mode {
            tile_height as u32
        } else {
            map_scale.unwrap_or(1)
        }
    });
    if !(1..=scene_core::MAX_SCALE).contains(&scale) {
        return Err(usage(format!(
            "cannot infer a scene scale from the map; pass --scale (1..={})",
            scene_core::MAX_SCALE
        )));
    }
    let cell = cell.unwrap_or(if object_mode || blocks {
        1.0
    } else {
        tile_height
    });
    let settings = scene_settings(&map);

    let mut placements = Vec::new();
    let mut used = BTreeSet::new();
    for layer in map["layers"].as_array().into_iter().flatten() {
        let kind = layer["type"].as_str().unwrap_or_default();
        let layer_name = layer["name"].as_str().unwrap_or_default();
        if !all && !selected.iter().any(|name| name == layer_name) {
            continue;
        }
        match kind {
            "objectgroup" => {
                for object in layer["objects"].as_array().into_iter().flatten() {
                    let Some(model) = object_model(object) else {
                        continue;
                    };
                    let Some((x, y, z)) = object_position(object, tile_height, cell) else {
                        continue;
                    };
                    let rotation = object_rotation(object);
                    let turn = object_turn(object);
                    used.insert(model.clone());
                    placements.push(Placement {
                        model,
                        x,
                        y,
                        z,
                        rotation,
                        turn,
                    });
                }
            }
            "tilelayer" => {
                let data = layer["data"].as_array().into_iter().flatten();
                for (index, gid) in data.enumerate() {
                    let Some(gid) = gid.as_u64() else {
                        continue;
                    };
                    if gid == 0 {
                        continue;
                    }
                    let Some((model, rotation, turn, origin)) = tile_info(&tilesets, gid as u32)
                    else {
                        continue;
                    };
                    let width = map["width"].as_u64().unwrap_or(1) as usize;
                    let (cell_x, cell_y) = ((index % width) as f64, (index / width) as f64);
                    let (x, y) = match origin {
                        // The exporter stores the offset from the tile's cell
                        // back to the placement's back corner.
                        Some([origin_x, origin_y]) => {
                            ((cell_x + origin_x) * cell, (cell_y + origin_y) * cell)
                        }
                        None => (cell_x * cell, cell_y * cell),
                    };
                    used.insert(model.clone());
                    placements.push(Placement {
                        model,
                        x,
                        y,
                        z: 0.0,
                        rotation,
                        turn,
                    });
                }
            }
            _ => {}
        }
    }
    let walls = collect_walls(&map, &tilesets);
    if walls.is_none() && has_wall_tiles(&map, &tilesets) {
        notes.push(
            "wall layers found, but the wall height does not fit `1..={MAX_WALL_HEIGHT}`; no .walls written"
                .to_owned(),
        );
    }
    if placements.is_empty() && walls.is_none() {
        return Err(usage(format!(
            "no objects or tiles with a model on layers {selected:?}; pass --layers"
        )));
    }
    let scene = render_scene(&name, &map_path, scale, &placements, &used, &settings);
    fs::create_dir_all(&output)
        .map_err(|error| usage(format!("cannot create `{}`: {error}", output.display())))?;
    if !used.is_empty() {
        let Some(models) = models.as_deref() else {
            return Err(usage(
                "`--models <dir>` is required: the scene needs the .model files",
            ));
        };
        for model in &used {
            let from = models.join(format!("{model}.model"));
            if !from.is_file() {
                return Err(usage(format!(
                    "model `{model}` not found in `{}`",
                    models.display()
                )));
            }
            fs::copy(&from, output.join(format!("{model}.model"))).map_err(|error| {
                usage(format!(
                    "cannot copy `{}` to `{}`: {error}",
                    from.display(),
                    output.display()
                ))
            })?;
        }
        copy_assets(models, &output)?;
    }
    let scene_path = output.join(format!("{name}.scene"));
    fs::write(&scene_path, scene)
        .map_err(|error| usage(format!("cannot write `{}`: {error}", scene_path.display())))?;
    let walls_path = match &walls {
        Some(grid) => {
            let path = output.join(format!("{name}.walls"));
            fs::write(&path, render_walls(&map_path, grid))
                .map_err(|error| usage(format!("cannot write `{}`: {error}", path.display())))?;
            Some(path)
        }
        None => None,
    };
    let wall_height = walls.as_ref().map(|grid| grid.height);
    Ok(ImportReport {
        scene: scene_path,
        walls: walls_path,
        wall_height,
        placements: placements.len(),
        models: used.len(),
        scale,
        cell,
        notes,
    })
}

fn load_tilesets(map: &Value, map_dir: &Path) -> Parsed<Vec<(u32, Value)>> {
    let mut result = Vec::new();
    for reference in map["tilesets"].as_array().into_iter().flatten() {
        let firstgid = reference["firstgid"].as_u64().unwrap_or(1) as u32;
        let json = if let Some(source) = reference["source"].as_str() {
            let path = map_dir.join(source);
            let text = read(&path.to_string_lossy())?;
            serde_json::from_str(&text)
                .map_err(|error| usage(format!("cannot parse tileset `{source}`: {error}")))?
        } else {
            reference.clone()
        };
        result.push((firstgid, json));
    }
    result.sort_by_key(|(firstgid, _)| *firstgid);
    Ok(result)
}

fn property<'a>(value: &'a Value, name: &str) -> Option<&'a Value> {
    value["properties"]
        .as_array()?
        .iter()
        .find(|property| property["name"] == name)
        .and_then(|property| property.get("value"))
}

fn tile_json(tilesets: &[(u32, Value)], gid: u32) -> Option<&Value> {
    let (firstgid, tileset) = tilesets
        .iter()
        .rev()
        .find(|(firstgid, _)| *firstgid <= gid)?;
    let tile_id = gid - firstgid;
    tileset["tiles"]
        .as_array()?
        .iter()
        .find(|tile| tile["id"].as_u64() == Some(u64::from(tile_id)))
}

fn tile_info(tilesets: &[(u32, Value)], gid: u32) -> Option<(String, u16, f64, Option<[f64; 2]>)> {
    let tile = tile_json(tilesets, gid)?;
    // An object tile names its model explicitly (the university game) or
    // carries the cell origin that `iso-scene tiled` writes. Floor, shadow and
    // wall tiles have neither and must not become placements.
    let model = if let Some(model) = property(tile, "model").and_then(Value::as_str) {
        model.to_string()
    } else if property(tile, "origin_x").is_some() && property(tile, "origin_y").is_some() {
        tile["type"].as_str()?.to_string()
    } else {
        return None;
    };
    let rotation = property(tile, "rotation")
        .and_then(Value::as_u64)
        .unwrap_or(0) as u16;
    let turn = property(tile, "turn")
        .and_then(Value::as_f64)
        .unwrap_or(0.0);
    let origin = match (
        property(tile, "origin_x").and_then(Value::as_f64),
        property(tile, "origin_y").and_then(Value::as_f64),
    ) {
        (Some(x), Some(y)) => Some([x, y]),
        _ => None,
    };
    Some((model, rotation, turn, origin))
}

/// Wall cells rebuilt from the wall layers of a Tiled map.
struct WallGrid {
    height: u32,
    /// Layout rows with `#`, `.`, `D` and `W`.
    layout: Vec<String>,
    /// Whether `layout` came from the map's `iso_walls_layout` (so gaps are
    /// exact) or was rebuilt from the wall layers as `#`/`.`.
    exact: bool,
}

/// Layer names `iso-scene tiled` uses for walls.
const WALL_LAYERS: [&str; 5] = [
    "walls",
    "walls-back",
    "walls-front",
    "walls_back",
    "walls_front",
];

/// A wall tile of `iso-scene tiled`: the 16-mask autotile, not a wall model
/// sprite (the university game puts `wall.model` tiles into its wall layers).
fn is_wall_tile(tile: &Value) -> bool {
    property(tile, "mask").is_some()
}

fn has_wall_tiles(map: &Value, tilesets: &[(u32, Value)]) -> bool {
    map["layers"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|layer| {
            layer["type"] == "tilelayer"
                && layer["name"]
                    .as_str()
                    .is_some_and(|name| WALL_LAYERS.contains(&name))
        })
        .filter_map(|layer| layer["data"].as_array())
        .flatten()
        .filter_map(Value::as_u64)
        .filter(|gid| *gid > 0)
        .any(|gid| tile_json(tilesets, gid as u32).is_some_and(is_wall_tile))
}

/// Rebuild the wall grid from `walls`, `walls-back` and `walls-front` layers.
///
/// The map stores the cells and the tile height but not the original material
/// or which gap was a door and which a window; those stay in the header of the
/// generated `.walls` file as a note.
fn collect_walls(map: &Value, tilesets: &[(u32, Value)]) -> Option<WallGrid> {
    if let Some(text) = property(map, "iso_walls_layout").and_then(Value::as_str) {
        if let Some(grid) = property(map, "iso_walls_height")
            .and_then(Value::as_u64)
            .and_then(|height| u32::try_from(height).ok())
            .filter(|height| (1..=MAX_WALL_HEIGHT).contains(height))
            .and_then(|height| layout_grid(text, height))
        {
            return Some(grid);
        }
    }
    let columns = u32::try_from(map["width"].as_u64()?).ok()?;
    let rows = u32::try_from(map["height"].as_u64()?).ok()?;
    if columns == 0 || rows == 0 {
        return None;
    }
    let map_height = map["tileheight"].as_f64()?;
    let mut cells = vec![false; (columns as usize).checked_mul(rows as usize)?];
    let mut height = None;
    for layer in map["layers"].as_array().into_iter().flatten() {
        let name = layer["name"].as_str().unwrap_or_default();
        if layer["type"] != "tilelayer" || !WALL_LAYERS.contains(&name) {
            continue;
        }
        let Some(data) = layer["data"].as_array() else {
            continue;
        };
        for (index, gid) in data.iter().enumerate() {
            let Some(gid) = gid.as_u64() else {
                continue;
            };
            if gid == 0 {
                continue;
            }
            let Some(tile) = tile_json(tilesets, gid as u32) else {
                continue;
            };
            if !is_wall_tile(tile) {
                continue;
            }
            let x = index % columns as usize;
            let y = index / columns as usize;
            if x >= columns as usize || y >= rows as usize {
                continue;
            }
            cells[y * columns as usize + x] = true;
            if height.is_none() {
                // The wall tileset is one height unit taller than the wall.
                height = Some(wall_height(tilesets, gid as u32, map_height));
            }
        }
    }
    if !cells.iter().any(|&wall| wall) {
        return None;
    }
    // A `.walls` height is `1..=MAX_WALL_HEIGHT`; a taller autotile cannot be
    // represented.
    let height = height.flatten()?;
    let layout: Vec<String> = (0..rows)
        .map(|y| {
            (0..columns)
                .map(|x| {
                    if cells[(y * columns + x) as usize] {
                        '#'
                    } else {
                        '.'
                    }
                })
                .collect()
        })
        .collect();
    Some(WallGrid {
        height,
        layout,
        exact: false,
    })
}

/// A layout stored by `iso-scene tiled`; rejects anything a `.walls` parser
/// would reject instead of writing a broken file.
fn layout_grid(text: &str, height: u32) -> Option<WallGrid> {
    let layout: Vec<String> = text
        .lines()
        .map(|line| line.trim_end_matches('\r').to_owned())
        .collect();
    let columns = layout.first()?.chars().count();
    if columns == 0
        || layout.iter().any(|row| {
            row.chars().count() != columns
                || !row
                    .chars()
                    .all(|character| matches!(character, '#' | '.' | 'D' | 'W'))
        })
        || !layout.iter().any(|row| row.contains('#'))
    {
        return None;
    }
    Some(WallGrid {
        height,
        layout,
        exact: true,
    })
}

fn wall_height(tilesets: &[(u32, Value)], gid: u32, map_height: f64) -> Option<u32> {
    let (firstgid, tileset) = tilesets
        .iter()
        .rev()
        .find(|(firstgid, _)| *firstgid <= gid)?;
    let _ = firstgid;
    let tile_height = tileset["tileheight"].as_f64()?;
    if !map_height.is_finite() || map_height <= 0.0 {
        return None;
    }
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let walls = (tile_height / map_height).round() as i64 - 1;
    u32::try_from(walls)
        .ok()
        .filter(|value| (1..=MAX_WALL_HEIGHT).contains(value))
}

fn render_walls(map_path: &str, grid: &WallGrid) -> String {
    use std::fmt::Write as _;
    let mut text = String::new();
    let _ = writeln!(
        text,
        "// rebuilt from {map_path} by `iso-scene import-tiled`"
    );
    if !grid.exact {
        text.push_str(
            "// Door and window gaps are plain floor: this map does not record which was which,\n",
        );
        text.push_str(
            "// so a re-export can move a wall between the back and the front layer near a gap.\n",
        );
    }
    text.push_str("// The wall palette is baked into the tileset; add `material` to restore it.\n");
    text.push_str("version 1;\n");
    let _ = writeln!(text, "height {};", grid.height);
    text.push_str("layout {\n");
    for row in &grid.layout {
        let _ = writeln!(text, "    \"{row}\"");
    }
    text.push_str("}\n");
    text
}

fn object_model(object: &Value) -> Option<String> {
    let kind = object["type"]
        .as_str()
        .or_else(|| object["name"].as_str())?;
    if matches!(kind, "" | "label" | "footprint") {
        return None;
    }
    Some(kind.to_string())
}

fn object_position(object: &Value, tile_height: f64, cell: f64) -> Option<(f64, f64, f64)> {
    let x = object["x"].as_f64()? / tile_height;
    let y = object["y"].as_f64()? / tile_height;
    let z = object["properties"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|property| property["name"] == "z")
        .and_then(|property| property["value"].as_f64())
        .unwrap_or(0.0);
    // `iso-scene tiled` stores the sprite's near corner. `footprint_x/y` are
    // relative to that corner, so their sum is the placement's back corner in
    // cells; a map without them keeps the old position as written.
    let (x, y) = match (
        property(object, "footprint_x").and_then(Value::as_f64),
        property(object, "footprint_y").and_then(Value::as_f64),
    ) {
        (Some(fx), Some(fy)) => (x + fx, y + fy),
        _ => (x + z, y + z),
    };
    Some((x * cell, y * cell, z))
}

fn object_rotation(object: &Value) -> u16 {
    property(object, "rotation")
        .and_then(Value::as_u64)
        .unwrap_or(0) as u16
}

fn object_turn(object: &Value) -> f64 {
    property(object, "turn")
        .and_then(Value::as_f64)
        .unwrap_or(0.0)
}

struct Placement {
    model: String,
    x: f64,
    y: f64,
    z: f64,
    rotation: u16,
    turn: f64,
}

/// Scene-level settings `iso-scene tiled` keeps in map properties.
#[derive(Default)]
struct SceneSettings {
    background: Option<String>,
    sun: Option<[f64; 2]>,
    ambient: Option<f64>,
    canvas: Option<[f64; 3]>,
}

fn scene_settings(map: &Value) -> SceneSettings {
    SceneSettings {
        background: property(map, "iso_background")
            .and_then(Value::as_str)
            .and_then(valid_background),
        sun: property(map, "iso_sun")
            .and_then(Value::as_str)
            .and_then(parse_sun),
        ambient: property(map, "iso_ambient")
            .and_then(Value::as_f64)
            .filter(|value| (0.0..=1.0).contains(value)),
        canvas: property(map, "iso_canvas")
            .and_then(Value::as_str)
            .and_then(parse_canvas),
    }
}

/// A `#RRGGBB` colour or `transparent`; anything else is dropped rather than
/// written into a scene the parser would reject.
fn valid_background(text: &str) -> Option<String> {
    if text == "transparent" {
        return Some(text.to_owned());
    }
    let digits = text.strip_prefix('#')?;
    (digits.len() == 6
        && digits
            .chars()
            .all(|character| character.is_ascii_hexdigit()))
    .then(|| text.to_owned())
}

fn parse_sun(text: &str) -> Option<[f64; 2]> {
    let mut parts = text.split_whitespace();
    let azimuth = parts.next()?.parse::<f64>().ok()?;
    let altitude = parts.next()?.parse::<f64>().ok()?;
    if parts.next().is_some() {
        return None;
    }
    ((0.0..=360.0).contains(&azimuth) && (0.0..=90.0).contains(&altitude))
        .then_some([azimuth, altitude])
}

fn parse_canvas(text: &str) -> Option<[f64; 3]> {
    let mut parts = text.split_whitespace();
    let mut size = [0.0; 3];
    for value in &mut size {
        *value = parts.next()?.parse::<f64>().ok()?;
    }
    if parts.next().is_some()
        || !size.iter().all(|value| {
            value.is_finite() && *value > 0.0 && *value <= scene_core::MAX_CANVAS_BLOCKS
        })
    {
        return None;
    }
    Some(size)
}

fn render_scene(
    name: &str,
    map_path: &str,
    scale: u32,
    placements: &[Placement],
    used: &BTreeSet<String>,
    settings: &SceneSettings,
) -> String {
    use std::fmt::Write as _;
    let mut scene = String::new();
    let _ = writeln!(
        scene,
        "// generated from {map_path} by `iso-scene import-tiled`"
    );
    let _ = writeln!(scene, "version 1;\n");
    for model in used {
        let _ = writeln!(scene, "include \"{model}.model\";");
    }
    let _ = writeln!(scene, "\nrender iso;\nscale {scale};");
    let background = settings.background.as_deref().unwrap_or("transparent");
    let _ = writeln!(scene, "background {background};");
    if let Some([azimuth, altitude]) = settings.sun {
        let _ = writeln!(scene, "sun ({}, {});", number(azimuth), number(altitude));
    }
    if let Some(ambient) = settings.ambient {
        let _ = writeln!(scene, "ambient {};", number(ambient));
    }
    if let Some([width, depth, height]) = settings.canvas {
        let _ = writeln!(
            scene,
            "canvas ({}, {}, {});",
            number(width),
            number(depth),
            number(height)
        );
    }
    let _ = writeln!(scene, "// scene {name}");
    for placement in placements {
        let _ = write!(
            scene,
            "place {} at ({}, {}, {}) rot {}",
            placement.model,
            number(placement.x),
            number(placement.y),
            number(placement.z),
            placement.rotation
        );
        if placement.turn != 0.0 {
            let _ = write!(scene, " turn {}", number(placement.turn));
        }
        scene.push_str(";\n");
    }
    scene
}

fn number(value: f64) -> String {
    // `object.x / tileheight` and the anchor nudge leave values like
    // `1.0000003`; the exported map is a grid, so print them as integers.
    if (value - value.round()).abs() < 1e-6 && value.abs() < 1e15 {
        format!("{}", value.round() as i64)
    } else {
        format!("{value:.4}")
    }
}

fn copy_assets(models: &Path, output: &Path) -> Parsed<()> {
    for folder in ["materials", "textures"] {
        let source = models.join(folder);
        if source.is_dir() {
            copy_tree(&source, &output.join(folder))?;
        }
    }
    Ok(())
}

fn copy_tree(source: &Path, target: &Path) -> Parsed<()> {
    fs::create_dir_all(target)
        .map_err(|error| usage(format!("cannot create `{}`: {error}", target.display())))?;
    let mut entries: Vec<_> = fs::read_dir(source)
        .map_err(|error| usage(format!("cannot read `{}`: {error}", source.display())))?
        .collect::<std::io::Result<Vec<_>>>()
        .map_err(|error| usage(error.to_string()))?;
    entries.sort_by_key(std::fs::DirEntry::file_name);
    for entry in entries {
        let path = entry.path();
        let destination = target.join(entry.file_name());
        if path.is_dir() {
            copy_tree(&path, &destination)?;
        } else if path.is_file() {
            fs::copy(&path, &destination).map_err(|error| {
                usage(format!(
                    "cannot copy `{}` to `{}`: {error}",
                    path.display(),
                    destination.display()
                ))
            })?;
        }
    }
    Ok(())
}
