//! `check` and `eval`.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use crate::common::{format_bounds, read, report};

pub(crate) fn check(path: &str) -> ExitCode {
    let Some(source) = read(path) else {
        return ExitCode::from(2);
    };
    match scene_syntax::parse(&source) {
        Ok(program) => {
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
    let mut args = args.into_iter();

    while let Some(argument) = args.next() {
        if argument == "--fuel" {
            let Some(value) = args.next() else {
                eprintln!("iso-scene: `--fuel` needs a number");
                return ExitCode::from(2);
            };
            match value.parse() {
                Ok(value) => fuel = value,
                Err(error) => {
                    eprintln!("iso-scene: invalid fuel `{value}`: {error}");
                    return ExitCode::from(2);
                }
            }
        } else if path.is_none() {
            path = Some(argument);
        } else {
            eprintln!("iso-scene: unexpected argument `{argument}`");
            return ExitCode::from(2);
        }
    }

    let Some(path) = path else {
        eprintln!("iso-scene: `eval` needs a file");
        return ExitCode::from(2);
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
    let root = std::env::current_dir().unwrap_or_else(|_| PathBuf::from(base));

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
