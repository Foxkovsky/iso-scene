//! `animate`: a rotation animation as a strip, tileset and map.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use crate::common::{read, report};

#[allow(
    clippy::too_many_lines,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]
pub(crate) fn animate_scene(args: Vec<String>) -> ExitCode {
    let mut path: Option<String> = None;
    let mut object_name: Option<String> = None;
    let mut from = 0.0_f64;
    let mut to = 90.0_f64;
    let mut frames = 7_u32;
    let mut duration = 120_u32;
    let mut output: Option<PathBuf> = None;
    let mut args = args.into_iter();

    while let Some(argument) = args.next() {
        let mut value = |name: &str| -> Option<String> {
            args.next().or_else(|| {
                eprintln!("iso-scene: `{name}` needs a value");
                None
            })
        };
        match argument.as_str() {
            "--out" | "-o" => {
                let Some(v) = value("--out") else {
                    return ExitCode::from(2);
                };
                output = Some(PathBuf::from(v));
            }
            "--object" => {
                let Some(v) = value("--object") else {
                    return ExitCode::from(2);
                };
                object_name = Some(v);
            }
            "--from" => {
                let Some(v) = value("--from") else {
                    return ExitCode::from(2);
                };
                match v.parse::<f64>() {
                    Ok(n) if n.is_finite() => from = n,
                    _ => {
                        eprintln!("iso-scene: invalid angle `{v}`");
                        return ExitCode::from(2);
                    }
                }
            }
            "--to" => {
                let Some(v) = value("--to") else {
                    return ExitCode::from(2);
                };
                match v.parse::<f64>() {
                    Ok(n) if n.is_finite() => to = n,
                    _ => {
                        eprintln!("iso-scene: invalid angle `{v}`");
                        return ExitCode::from(2);
                    }
                }
            }
            "--frames" => {
                let Some(v) = value("--frames") else {
                    return ExitCode::from(2);
                };
                match v.parse::<u32>() {
                    Ok(n) if (1..=scene_export_sprites::MAX_FRAMES).contains(&n) => frames = n,
                    _ => {
                        eprintln!(
                            "iso-scene: invalid frame count `{v}`; use 1..={}",
                            scene_export_sprites::MAX_FRAMES
                        );
                        return ExitCode::from(2);
                    }
                }
            }
            "--duration" => {
                let Some(v) = value("--duration") else {
                    return ExitCode::from(2);
                };
                match v.parse::<u32>() {
                    Ok(n) if n >= 1 => duration = n,
                    _ => {
                        eprintln!("iso-scene: invalid duration `{v}`");
                        return ExitCode::from(2);
                    }
                }
            }
            other if path.is_none() => path = Some(other.to_string()),
            other => {
                eprintln!("iso-scene: unexpected argument `{other}`");
                return ExitCode::from(2);
            }
        }
    }

    let Some(path) = path else {
        eprintln!("iso-scene: `animate` needs a .scene file");
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
    let scene = match scene_core::compile_scene(&program, base, &root) {
        Ok(scene) => scene,
        Err(diagnostic) => return report(&path, &source, &diagnostic),
    };
    let Some(index) = scene
        .instances
        .iter()
        .position(|instance| Some(&instance.object.name) == object_name.as_ref())
    else {
        eprintln!(
            "iso-scene: scene has no instance of `{}`",
            object_name.as_deref().unwrap_or("")
        );
        return ExitCode::from(2);
    };

    let output = output.unwrap_or_else(|| {
        let stem = file
            .file_stem()
            .and_then(|value| value.to_str())
            .unwrap_or("animation");
        PathBuf::from("out").join(stem)
    });
    let options = scene_export_sprites::animate::AnimateOptions {
        name: output
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or("animation")
            .to_string(),
        from,
        to,
        frames,
        duration,
    };
    let report =
        match scene_export_sprites::animate::export_animation(&scene, index, &options, &output) {
            Ok(report) => report,
            Err(diagnostic) => {
                eprintln!(
                    "iso-scene: error[{}]: {}",
                    diagnostic.code, diagnostic.message
                );
                return ExitCode::from(1);
            }
        };
    println!(
        "wrote {} frames {}x{} ({} ms each); open `{}` in Tiled",
        report.frames,
        report.frame_size[0],
        report.frame_size[1],
        duration,
        report.map.display()
    );
    ExitCode::SUCCESS
}
