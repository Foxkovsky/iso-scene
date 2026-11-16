//! `import-tiled`: turn a Tiled map back into a `.scene`.
//!
//! Object layers from `iso-scene tiled` use the object's `type`/`name` and
//! `rotation`/`turn` properties. Tile-layer maps (the university game) use a
//! `model` property on each tile. In both cases the referenced `.model` files
//! and their materials are copied next to the generated scene, so it is
//! self-contained.

#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    clippy::cast_sign_loss,
    clippy::too_many_lines
)]

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use serde_json::Value;

use crate::common::{Args, Parsed, read, usage};

pub(crate) fn import_tiled(args: Vec<String>) -> ExitCode {
    match run(args) {
        Ok(code) | Err(code) => code,
    }
}

fn run(args: Vec<String>) -> Parsed<ExitCode> {
    let mut map_path: Option<String> = None;
    let mut models: Option<PathBuf> = None;
    let mut layers: Option<Vec<String>> = None;
    let mut scale: Option<u32> = None;
    let mut cell: Option<f64> = None;
    let mut output = PathBuf::from("out/scene-from-tiled");
    let mut name = "scene".to_string();
    let mut args = Args::new(args).hints(&[
        ("--models", "a model directory"),
        ("--layers", "layer names or `all`"),
        ("--scale", "pixels per block"),
        ("--cell", "scene units per Tiled cell"),
        ("--out", "an output directory"),
        ("--name", "a scene file stem"),
    ]);
    while let Some(argument) = args.next() {
        match argument.as_str() {
            "--models" => models = Some(PathBuf::from(args.value("--models")?)),
            "--layers" => {
                let value = args.value("--layers")?;
                layers = Some(
                    value
                        .split(',')
                        .map(|part| part.trim().to_string())
                        .filter(|part| !part.is_empty())
                        .collect(),
                );
            }
            "--scale" => {
                scale = Some(args.parse(
                    "--scale",
                    |n: &u32| (1..=scene_core::MAX_SCALE).contains(n),
                    |v| format!("invalid scale `{v}`"),
                )?);
            }
            "--cell" => {
                let value = args.value("--cell")?;
                match value.parse::<f64>() {
                    Ok(number) if number.is_finite() && number > 0.0 => cell = Some(number),
                    _ => return Err(usage(format!("invalid cell size `{value}`"))),
                }
            }
            "--out" | "-o" => output = PathBuf::from(args.value("--out")?),
            "--name" => name = args.value("--name")?,
            other if map_path.is_none() => map_path = Some(other.to_string()),
            other => return Err(usage(format!("unexpected argument `{other}`"))),
        }
    }
    let Some(map_path) = map_path else {
        return Err(usage("`import-tiled` needs a .tmj or Tiled JSON map"));
    };
    let source = read(&map_path).ok_or(ExitCode::from(2))?;
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
    let cell = cell.unwrap_or(if object_mode || blocks {
        1.0
    } else {
        tile_height
    });

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
    if placements.is_empty() {
        return Err(usage(format!(
            "no objects or tiles with a model on layers {selected:?}; pass --layers"
        )));
    }
    let Some(models) = models else {
        return Err(usage(
            "`--models <dir>` is required: the scene needs the .model files",
        ));
    };
    let scene = render_scene(&name, &map_path, scale, &placements, &used);
    fs::create_dir_all(&output)
        .map_err(|error| usage(format!("cannot create `{}`: {error}", output.display())))?;
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
    copy_assets(&models, &output)?;
    let scene_path = output.join(format!("{name}.scene"));
    fs::write(&scene_path, scene)
        .map_err(|error| usage(format!("cannot write `{}`: {error}", scene_path.display())))?;
    println!(
        "wrote {} ({} placements, {} models, scale {}, cell {})",
        scene_path.display(),
        placements.len(),
        used.len(),
        scale,
        cell
    );
    Ok(ExitCode::SUCCESS)
}

fn load_tilesets(map: &Value, map_dir: &Path) -> Parsed<Vec<(u32, Value)>> {
    let mut result = Vec::new();
    for reference in map["tilesets"].as_array().into_iter().flatten() {
        let firstgid = reference["firstgid"].as_u64().unwrap_or(1) as u32;
        let json = if let Some(source) = reference["source"].as_str() {
            let path = map_dir.join(source);
            let text = read(&path.to_string_lossy()).ok_or(ExitCode::from(2))?;
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

fn tile_info(tilesets: &[(u32, Value)], gid: u32) -> Option<(String, u16, f64, Option<[f64; 2]>)> {
    let (firstgid, tileset) = tilesets
        .iter()
        .rev()
        .find(|(firstgid, _)| *firstgid <= gid)?;
    let tile_id = gid - firstgid;
    let tile = tileset["tiles"]
        .as_array()?
        .iter()
        .find(|tile| tile["id"].as_u64() == Some(u64::from(tile_id)))?;
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

fn render_scene(
    name: &str,
    map_path: &str,
    scale: u32,
    placements: &[Placement],
    used: &BTreeSet<String>,
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
    let _ = writeln!(
        scene,
        "\nrender iso;\nscale {scale};\nbackground transparent;\n"
    );
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
