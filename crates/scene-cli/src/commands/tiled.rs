//! `tiled`: isometric Tiled map export.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use crate::common::{project_root, read, report};

pub(crate) fn tiled_scene(args: Vec<String>) -> ExitCode {
    let mut path: Option<String> = None;
    let mut walls_path: Option<String> = None;
    let mut objects_as_tiles = false;
    let mut walls_in_front = false;
    let mut base_override: Option<PathBuf> = None;
    let mut output = PathBuf::from("out/tiled");
    let mut args = args.into_iter();

    while let Some(argument) = args.next() {
        if argument == "--out" || argument == "-o" {
            let Some(value) = args.next() else {
                eprintln!("iso-scene: `--out` needs a path");
                return ExitCode::from(2);
            };
            output = PathBuf::from(value);
        } else if argument == "--base" {
            let Some(value) = args.next() else {
                eprintln!("iso-scene: `--base` needs a directory");
                return ExitCode::from(2);
            };
            base_override = Some(PathBuf::from(value));
        } else if argument == "--tile-layer" {
            objects_as_tiles = true;
        } else if argument == "--walls-front" {
            walls_in_front = true;
        } else if argument == "--walls" {
            let Some(value) = args.next() else {
                eprintln!("iso-scene: `--walls` needs a .walls file");
                return ExitCode::from(2);
            };
            walls_path = Some(value);
        } else if path.is_none() {
            path = Some(argument);
        } else {
            eprintln!("iso-scene: unexpected argument `{argument}`");
            return ExitCode::from(2);
        }
    }

    let Some(path) = path else {
        eprintln!("iso-scene: `tiled` needs a .scene file");
        return ExitCode::from(2);
    };
    let Some(source) = read(&path) else {
        return ExitCode::from(2);
    };
    if Path::new(&path)
        .extension()
        .and_then(|value| value.to_str())
        != Some("scene")
    {
        eprintln!("iso-scene: `tiled` expects a .scene file");
        return ExitCode::from(2);
    }
    let program = match scene_syntax::parse(&source) {
        Ok(program) => program,
        Err(diagnostic) => return report(&path, &source, &diagnostic),
    };
    let file = Path::new(&path);
    let fallback_base = file.parent().unwrap_or_else(|| Path::new("."));
    let base = base_override.as_deref().unwrap_or(fallback_base);
    let root = project_root(base_override.as_deref());
    let scene = match scene_core::compile_scene(&program, base, &root) {
        Ok(scene) => scene,
        Err(diagnostic) => return report(&path, &source, &diagnostic),
    };
    let walls = if let Some(walls_path) = walls_path {
        let walls_file = Path::new(&walls_path);
        match scene_walls::load(walls_file, &root) {
            Ok(walls) => Some(walls),
            Err(diagnostic) => {
                let walls_source = fs::read_to_string(walls_file).unwrap_or_default();
                return report(&walls_path, &walls_source, &diagnostic);
            }
        }
    } else {
        None
    };
    let exported = match scene_export_tiled::export_with(
        &scene,
        walls.as_ref(),
        objects_as_tiles,
        walls_in_front,
        &output,
    ) {
        Ok(report) => report,
        Err(diagnostic) => return report(&path, &source, &diagnostic),
    };
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
    ExitCode::SUCCESS
}
