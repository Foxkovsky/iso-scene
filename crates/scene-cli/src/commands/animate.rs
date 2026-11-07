//! `animate`: a rotation animation as a strip, tileset and map.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use scene_export_sprites::animate::{AnimateOptions, export_animation};

use crate::common::{Args, Parsed, failed, load_scene, usage};

pub(crate) fn animate_scene(args: Vec<String>) -> ExitCode {
    match run(args) {
        Ok(code) | Err(code) => code,
    }
}

fn run(args: Vec<String>) -> Parsed<ExitCode> {
    let mut path: Option<String> = None;
    let mut object_name: Option<String> = None;
    let mut from = 0.0_f64;
    let mut to = 90.0_f64;
    let mut frames = 7_u32;
    let mut duration = 120_u32;
    let mut output: Option<PathBuf> = None;
    let mut base: Option<PathBuf> = None;
    let mut args = Args::new(args);
    let finite = |value: &f64| value.is_finite();
    while let Some(argument) = args.next() {
        match argument.as_str() {
            "--out" | "-o" => output = Some(PathBuf::from(args.value("--out")?)),
            "--base" => base = Some(PathBuf::from(args.value("--base")?)),
            "--object" => object_name = Some(args.value("--object")?),
            "--from" => from = args.parse("--from", finite, |v| format!("invalid angle `{v}`"))?,
            "--to" => to = args.parse("--to", finite, |v| format!("invalid angle `{v}`"))?,
            "--frames" => {
                frames = args.parse(
                    "--frames",
                    |n: &u32| (1..=scene_export_sprites::MAX_FRAMES).contains(n),
                    |v| {
                        format!(
                            "invalid frame count `{v}`; use 1..={}",
                            scene_export_sprites::MAX_FRAMES
                        )
                    },
                )?;
            }
            "--duration" => {
                duration = args.parse(
                    "--duration",
                    |n: &u32| *n >= 1,
                    |v| format!("invalid duration `{v}`"),
                )?;
            }
            _ if path.is_none() => path = Some(argument),
            _ => return Err(usage(format!("unexpected argument `{argument}`"))),
        }
    }
    let path = path.ok_or_else(|| usage("`animate` needs a .scene file"))?;
    let loaded = load_scene("animate", &path, base.as_deref(), false)?;
    let index = loaded
        .scene
        .instances
        .iter()
        .position(|instance| Some(&instance.object.name) == object_name.as_ref())
        .ok_or_else(|| {
            usage(format!(
                "scene has no instance of `{}`",
                object_name.as_deref().unwrap_or("")
            ))
        })?;
    let output = output.unwrap_or_else(|| {
        let stem = Path::new(&path)
            .file_stem()
            .and_then(|value| value.to_str())
            .unwrap_or("animation");
        PathBuf::from("out").join(stem)
    });
    let options = AnimateOptions {
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
    let report = export_animation(&loaded.scene, index, &options, &output)
        .map_err(|diagnostic| failed(&diagnostic))?;
    println!(
        "wrote {} frames {}x{} ({} ms each); open `{}` in Tiled",
        report.frames,
        report.frame_size[0],
        report.frame_size[1],
        duration,
        report.map.display()
    );
    Ok(ExitCode::SUCCESS)
}
