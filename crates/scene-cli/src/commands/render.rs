//! `render`: PNG or SVG of a scene or model.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use crate::common::{model_preview, project_root, read, report};

#[allow(clippy::too_many_lines)]
pub(crate) fn render_scene(args: Vec<String>) -> ExitCode {
    let mut path: Option<String> = None;
    let mut output: Option<PathBuf> = None;
    let mut view: Option<String> = None;
    let mut base_override: Option<PathBuf> = None;
    let mut args = args.into_iter();

    while let Some(argument) = args.next() {
        if argument == "--out" || argument == "-o" {
            let Some(value) = args.next() else {
                eprintln!("iso-scene: `--out` needs a path");
                return ExitCode::from(2);
            };
            output = Some(PathBuf::from(value));
        } else if argument == "--view" {
            let Some(value) = args.next() else {
                eprintln!("iso-scene: `--view` needs iso, top, front or side");
                return ExitCode::from(2);
            };
            view = Some(value);
        } else if argument == "--base" {
            let Some(value) = args.next() else {
                eprintln!("iso-scene: `--base` needs a directory");
                return ExitCode::from(2);
            };
            base_override = Some(PathBuf::from(value));
        } else if path.is_none() {
            path = Some(argument);
        } else {
            eprintln!("iso-scene: unexpected argument `{argument}`");
            return ExitCode::from(2);
        }
    }

    let Some(path) = path else {
        eprintln!("iso-scene: `render` needs a .scene file");
        return ExitCode::from(2);
    };
    let Some(source) = read(&path) else {
        return ExitCode::from(2);
    };
    let extension = Path::new(&path)
        .extension()
        .and_then(|value| value.to_str());
    if extension != Some("scene") && extension != Some("model") {
        eprintln!("iso-scene: `render` expects a .scene or .model file");
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
    let mut scene = if extension == Some("model") {
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
    if let Some(view) = view {
        scene.render = match view.as_str() {
            "iso" => scene_syntax::ast::RenderKind::Iso,
            "top" => scene_syntax::ast::RenderKind::Top,
            "front" => scene_syntax::ast::RenderKind::Front,
            "side" => scene_syntax::ast::RenderKind::Side,
            other => {
                eprintln!("iso-scene: unknown view `{other}`; expected iso, top, front or side");
                return ExitCode::from(2);
            }
        };
    }
    let output = output.unwrap_or_else(|| {
        let stem = file
            .file_stem()
            .and_then(|value| value.to_str())
            .unwrap_or("scene");
        PathBuf::from("out").join(format!("{stem}.png"))
    });
    if let Some(parent) = output.parent() {
        if !parent.as_os_str().is_empty() {
            if let Err(error) = fs::create_dir_all(parent) {
                eprintln!("iso-scene: cannot create `{}`: {error}", parent.display());
                return ExitCode::from(2);
            }
        }
    }
    let svg = output
        .extension()
        .and_then(std::ffi::OsStr::to_str)
        .is_some_and(|extension| extension.eq_ignore_ascii_case("svg"));
    if svg {
        let document = match scene_backend_iso::render_svg(&scene) {
            Ok(document) => document,
            Err(diagnostic) => return report(&path, &source, &diagnostic),
        };
        if let Err(error) = fs::write(&output, document) {
            eprintln!("iso-scene: cannot write `{}`: {error}", output.display());
            return ExitCode::from(2);
        }
        println!(
            "wrote {} (SVG, {} instances)",
            output.display(),
            scene.instances.len()
        );
        return ExitCode::SUCCESS;
    }
    let image = match scene.render {
        scene_syntax::ast::RenderKind::Iso => scene_backend_iso::render(&scene),
        scene_syntax::ast::RenderKind::Top
        | scene_syntax::ast::RenderKind::Front
        | scene_syntax::ast::RenderKind::Side => scene_backend_ortho::render(&scene),
    };
    let image = match image {
        Ok(image) => image,
        Err(diagnostic) => return report(&path, &source, &diagnostic),
    };
    if let Err(message) = scene_render::write_png(&image, &output) {
        eprintln!("iso-scene: {message}");
        return ExitCode::from(2);
    }
    println!(
        "wrote {} ({}x{}, {} instances)",
        output.display(),
        image.width,
        image.height,
        scene.instances.len()
    );
    ExitCode::SUCCESS
}
