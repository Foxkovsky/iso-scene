//! `voxel`: export to the `MagicaVoxel` format.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use crate::common::{model_preview, project_root, read, report};

#[allow(clippy::too_many_lines)]
pub(crate) fn voxel_scene(args: Vec<String>) -> ExitCode {
    let mut path: Option<String> = None;
    let mut output: Option<PathBuf> = None;
    let mut density: f64 = 8.0;
    let mut explicit_density = false;
    let mut base_override: Option<PathBuf> = None;
    let mut args = args.into_iter();

    while let Some(argument) = args.next() {
        if argument == "--out" || argument == "-o" {
            let Some(value) = args.next() else {
                eprintln!("iso-scene: `--out` needs a path");
                return ExitCode::from(2);
            };
            output = Some(PathBuf::from(value));
        } else if argument == "--density" {
            let Some(value) = args.next() else {
                eprintln!("iso-scene: `--density` needs a number");
                return ExitCode::from(2);
            };
            match value.parse::<f64>() {
                Ok(value) if value.is_finite() && value > 0.0 => {
                    density = value;
                    explicit_density = true;
                }
                _ => {
                    eprintln!("iso-scene: invalid density `{value}`");
                    return ExitCode::from(2);
                }
            }
        } else if argument == "--base" {
            let Some(value) = args.next() else {
                eprintln!("iso-scene: `--base` needs a directory");
                return ExitCode::from(2);
            };
            base_override = Some(PathBuf::from(value));
        } else if argument == "--voxel-size" {
            let Some(value) = args.next() else {
                eprintln!("iso-scene: `--voxel-size` needs a number");
                return ExitCode::from(2);
            };
            match value.parse::<f64>() {
                Ok(value) if value.is_finite() && value > 0.0 => {
                    density = 1.0 / value;
                    explicit_density = true;
                }
                _ => {
                    eprintln!("iso-scene: invalid voxel size `{value}`");
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
        eprintln!("iso-scene: `voxel` needs a .scene file");
        return ExitCode::from(2);
    };
    let Some(source) = read(&path) else {
        return ExitCode::from(2);
    };
    let extension = Path::new(&path)
        .extension()
        .and_then(|value| value.to_str());
    if extension != Some("scene") && extension != Some("model") {
        eprintln!("iso-scene: `voxel` expects a .scene or .model file");
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
    let scene = if extension == Some("model") {
        let model = match scene_core::compile_model(&program, base) {
            Ok(model) => model,
            Err(diagnostic) => return report(&path, &source, &diagnostic),
        };
        match model_preview(&model) {
            Ok(scene) => scene,
            Err(message) => {
                eprintln!("iso-scene: {message}");
                return ExitCode::from(2);
            }
        }
    } else {
        match scene_core::compile_scene(&program, base, &root) {
            Ok(scene) => scene,
            Err(diagnostic) => return report(&path, &source, &diagnostic),
        }
    };
    if !explicit_density {
        // Fit the longest axis into roughly 128 voxels.
        let size = scene.bounds.size();
        let extent = size[0].max(size[1]).max(size[2]).max(1.0);
        density = (128.0 / extent).clamp(0.05, 8.0);
    }
    let model = match scene_backend_voxel::voxelize(&scene, density) {
        Ok(model) => model,
        Err(diagnostic) => return report(&path, &source, &diagnostic),
    };
    let output = output.unwrap_or_else(|| {
        let stem = file
            .file_stem()
            .and_then(|value| value.to_str())
            .unwrap_or("scene");
        PathBuf::from("out").join(format!("{stem}.vox"))
    });
    if let Some(parent) = output.parent() {
        if !parent.as_os_str().is_empty() {
            if let Err(error) = fs::create_dir_all(parent) {
                eprintln!("iso-scene: cannot create `{}`: {error}", parent.display());
                return ExitCode::from(2);
            }
        }
    }
    if let Err(diagnostic) = scene_backend_voxel::write_vox(&model, &output) {
        return report(&path, &source, &diagnostic);
    }
    println!(
        "wrote {} ({}x{}x{}, {} voxels, density {})",
        output.display(),
        model.size[0],
        model.size[1],
        model.size[2],
        model.voxels.len(),
        density
    );
    ExitCode::SUCCESS
}
