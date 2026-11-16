//! `import-tiled`: turn a Tiled map back into a `.scene`.
//!
//! Object layers from `iso-scene tiled` use the object's `type`/`name` and
//! `rotation`/`turn` properties. Tile-layer maps (the university game) use a
//! `model` property on each tile. In both cases the referenced `.model` files,
//! their materials, the scene settings and the wall layout are restored next to
//! the generated scene, so it is self-contained.

use std::path::PathBuf;
use std::process::ExitCode;

use crate::common::{Args, Parsed, usage};

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
    let options = scene_export_tiled::import::ImportOptions {
        map: PathBuf::from(map_path),
        models,
        layers,
        scale,
        cell,
        output,
        name,
    };
    let report = scene_export_tiled::import::import(&options).map_err(usage)?;
    println!(
        "wrote {} ({} placements, {} models, scale {}, cell {})",
        report.scene.display(),
        report.placements,
        report.models,
        report.scale,
        report.cell
    );
    if let (Some(path), Some(height)) = (&report.walls, report.wall_height) {
        println!(
            "restored walls: {} (height {}; see its header for material and gaps)",
            path.display(),
            height
        );
    }
    for note in &report.notes {
        eprintln!("iso-scene: {note}");
    }
    Ok(ExitCode::SUCCESS)
}
