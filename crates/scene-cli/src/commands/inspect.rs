//! `check` and `eval`.

use std::path::Path;
use std::process::ExitCode;

use crate::common::{Args, format_bounds, read, report, usage};

pub(crate) fn check(path: &str) -> ExitCode {
    let Some(source) = read(path) else {
        return ExitCode::from(2);
    };
    let file = Path::new(path);
    let base = file.parent().unwrap_or_else(|| Path::new("."));
    let root = crate::common::project_root_for(file, None);
    let extension = file.extension().and_then(|value| value.to_str());
    if extension == Some("walls") {
        return match scene_walls::parse(&source, base, &root) {
            Ok(layout) => {
                println!(
                    "{path}: ok ({}x{} cells, {} walls)",
                    layout.columns,
                    layout.rows,
                    layout.cells.len()
                );
                ExitCode::SUCCESS
            }
            Err(diagnostic) => report(path, &source, &diagnostic),
        };
    }
    let program = match scene_syntax::parse(&source) {
        Ok(program) => program,
        Err(diagnostic) => return report(path, &source, &diagnostic),
    };
    // `check` runs the same static and semantic checks as the renderers, so an
    // unknown object or a missing `scale` is an error here too.
    let compiled = match extension {
        Some("model") => scene_core::compile_model(&program, base).map(drop),
        Some("scene") => scene_core::compile_scene(&program, base, &root).map(drop),
        _ => {
            return usage(format!(
                "`check` expects a .model, .scene or .walls file, got `{path}`"
            ));
        }
    };
    match compiled {
        Ok(()) => {
            println!(
                "{path}: ok (version {}, items: {})",
                program.version,
                program.items.len()
            );
            ExitCode::SUCCESS
        }
        Err(diagnostic) => report(path, &source, &diagnostic),
    }
}

pub(crate) fn eval(args: Vec<String>) -> ExitCode {
    let mut path: Option<String> = None;
    let mut fuel = scene_core::DEFAULT_FUEL;
    let mut args = Args::new(args).hints(&[("--fuel", "a number")]);
    while let Some(argument) = args.next() {
        if argument == "--fuel" {
            let value = match args.value("--fuel") {
                Ok(value) => value,
                Err(code) => return code,
            };
            match value.parse() {
                Ok(parsed) => fuel = parsed,
                Err(error) => return usage(format!("invalid fuel `{value}`: {error}")),
            }
        } else if path.is_none() {
            path = Some(argument);
        } else {
            return usage(format!("unexpected argument `{argument}`"));
        }
    }

    let Some(path) = path else {
        return usage("`eval` needs a file");
    };
    let Some(source) = read(&path) else {
        return ExitCode::from(2);
    };
    let program = match scene_syntax::parse(&source) {
        Ok(program) => program,
        Err(diagnostic) => return report(&path, &source, &diagnostic),
    };

    let file = Path::new(&path);
    let base = file.parent().unwrap_or_else(|| Path::new("."));
    let root = crate::common::project_root_for(file, None);

    match file.extension().and_then(|value| value.to_str()) {
        Some("model") => match scene_core::compile_model(&program, base) {
            Ok(model) => {
                let parts: usize = model
                    .objects
                    .values()
                    .map(|object| object.parts.len())
                    .sum();
                println!(
                    "{path}: model ok: {} objects, {} parts, bounds {}",
                    model.objects.len(),
                    parts,
                    format_bounds(model.bounds)
                );
                ExitCode::SUCCESS
            }
            Err(diagnostic) => report(&path, &source, &diagnostic),
        },
        Some("scene") => match scene_core::compile_scene_with_fuel(&program, base, &root, fuel) {
            Ok(scene) => {
                println!(
                    "{path}: scene ok: {} instances, materials: {}, render {:?}, scale {}, bounds {}",
                    scene.instances.len(),
                    scene.materials.len(),
                    scene.render,
                    scene.scale,
                    format_bounds(scene.bounds)
                );
                ExitCode::SUCCESS
            }
            Err(diagnostic) => report(&path, &source, &diagnostic),
        },
        _ => {
            eprintln!("iso-scene: `eval` expects a .model or .scene file");
            ExitCode::from(2)
        }
    }
}
