//! `render`: PNG or SVG of a scene or model.

use std::fs;
use std::path::PathBuf;
use std::process::ExitCode;

use scene_syntax::ast::RenderKind;

use crate::common::{Args, Parsed, create_parent, load_scene, usage};

pub(crate) fn render_scene(args: Vec<String>) -> ExitCode {
    match run(args) {
        Ok(code) | Err(code) => code,
    }
}

fn run(args: Vec<String>) -> Parsed<ExitCode> {
    let mut path: Option<String> = None;
    let mut output: Option<PathBuf> = None;
    let mut view: Option<String> = None;
    let mut rotate: Option<u16> = None;
    let mut base: Option<PathBuf> = None;
    let mut args = Args::new(args).hints(&[
        ("--out", "a path"),
        ("--view", "iso, top, front or side"),
        ("--rotate", "0, 90, 180 or 270 degrees"),
        ("--base", "a directory"),
    ]);
    while let Some(argument) = args.next() {
        match argument.as_str() {
            "--out" | "-o" => output = Some(PathBuf::from(args.value("--out")?)),
            "--view" => view = Some(args.value("--view")?),
            "--rotate" => {
                let value = args.value("--rotate")?;
                match value.parse::<u16>() {
                    Ok(degrees) if matches!(degrees, 0 | 90 | 180 | 270) => rotate = Some(degrees),
                    _ => {
                        return Err(usage(format!(
                            "invalid rotation `{value}`; expected 0, 90, 180 or 270"
                        )));
                    }
                }
            }
            "--base" => base = Some(PathBuf::from(args.value("--base")?)),
            _ if path.is_none() => path = Some(argument),
            _ => return Err(usage(format!("unexpected argument `{argument}`"))),
        }
    }
    let path = path.ok_or_else(|| usage("`render` needs a .scene file"))?;
    let mut loaded = load_scene("render", &path, base.as_deref(), true)?;
    if let Some(view) = view {
        loaded.scene.render = match view.as_str() {
            "iso" => RenderKind::Iso,
            "top" => RenderKind::Top,
            "front" => RenderKind::Front,
            "side" => RenderKind::Side,
            other => {
                return Err(usage(format!(
                    "unknown view `{other}`; expected iso, top, front or side"
                )));
            }
        };
    }
    if let Some(rotate) = rotate {
        loaded.scene.rotate_z(rotate);
    }
    let output = output.unwrap_or_else(|| loaded.default_output("png"));
    create_parent(&output)?;
    let scene = &loaded.scene;
    let svg = output
        .extension()
        .and_then(std::ffi::OsStr::to_str)
        .is_some_and(|extension| extension.eq_ignore_ascii_case("svg"));
    if svg {
        let document = scene_backend_iso::render_svg(scene)
            .map_err(|diagnostic| loaded.report(&diagnostic))?;
        fs::write(&output, document)
            .map_err(|error| usage(format!("cannot write `{}`: {error}", output.display())))?;
        println!(
            "wrote {} (SVG, {} instances)",
            output.display(),
            scene.instances.len()
        );
        return Ok(ExitCode::SUCCESS);
    }
    let image = match scene.render {
        RenderKind::Iso => scene_backend_iso::render(scene),
        RenderKind::Top | RenderKind::Front | RenderKind::Side => {
            scene_backend_ortho::render(scene)
        }
    }
    .map_err(|diagnostic| loaded.report(&diagnostic))?;
    scene_render::write_png(&image, &output).map_err(usage)?;
    println!(
        "wrote {} ({}x{}, {} instances)",
        output.display(),
        image.width,
        image.height,
        scene.instances.len()
    );
    Ok(ExitCode::SUCCESS)
}
