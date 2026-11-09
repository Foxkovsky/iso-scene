//! `tiled`: isometric Tiled map export.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use scene_export_tiled::{Collision, WallOrder};

use crate::common::{Args, Parsed, load_scene, report, usage};

pub(crate) fn tiled_scene(args: Vec<String>) -> ExitCode {
    match run(args) {
        Ok(code) | Err(code) => code,
    }
}

fn run(args: Vec<String>) -> Parsed<ExitCode> {
    let mut path: Option<String> = None;
    let mut walls_path: Option<String> = None;
    let mut objects_as_tiles = false;
    let mut wall_order = WallOrder::default();
    let mut base: Option<PathBuf> = None;
    let mut output = PathBuf::from("out/tiled");
    let mut args = Args::new(args).hints(&[
        ("--out", "a path"),
        ("--base", "a directory"),
        ("--walls", "a .walls file"),
    ]);
    while let Some(argument) = args.next() {
        match argument.as_str() {
            "--out" | "-o" => output = PathBuf::from(args.value("--out")?),
            "--base" => base = Some(PathBuf::from(args.value("--base")?)),
            "--tile-layer" => objects_as_tiles = true,
            "--walls-front" => wall_order = WallOrder::Front,
            "--walls-behind" => wall_order = WallOrder::Behind,
            "--walls" => walls_path = Some(args.value("--walls")?),
            _ if path.is_none() => path = Some(argument),
            _ => return Err(usage(format!("unexpected argument `{argument}`"))),
        }
    }
    let path = path.ok_or_else(|| usage("`tiled` needs a .scene file"))?;
    let loaded = load_scene("tiled", &path, base.as_deref(), false)?;
    let walls = match walls_path {
        Some(walls_path) => {
            let walls_file = Path::new(&walls_path);
            Some(
                scene_walls::load(walls_file, &loaded.root).map_err(|diagnostic| {
                    let walls_source = fs::read_to_string(walls_file).unwrap_or_default();
                    report(&walls_path, &walls_source, &diagnostic)
                })?,
            )
        }
        None => None,
    };
    let exported = scene_export_tiled::export_with(
        &loaded.scene,
        walls.as_ref(),
        objects_as_tiles,
        wall_order,
        &output,
    )
    .map_err(|diagnostic| loaded.report(&diagnostic))?;
    for collision in &exported.collisions {
        match collision {
            Collision::Objects { first, second } => eprintln!(
                "warning: `{}` (place #{}) overlaps `{}` (place #{})",
                first.1,
                first.0 + 1,
                second.1,
                second.0 + 1
            ),
            Collision::Wall { object, cell } => eprintln!(
                "warning: `{}` (place #{}) stands on the wall at cell ({}, {})",
                object.1,
                object.0 + 1,
                cell[0],
                cell[1]
            ),
        }
    }
    println!(
        "wrote {} ({}x{} cells, {} instances); open it in Tiled; preview: {}",
        exported.map.display(),
        exported.cells[0],
        exported.cells[1],
        exported.instances,
        exported.preview.as_ref().map_or_else(
            || "none (no objects)".to_string(),
            |p| p.display().to_string()
        )
    );
    Ok(ExitCode::SUCCESS)
}
