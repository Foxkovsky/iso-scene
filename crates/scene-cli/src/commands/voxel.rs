//! `voxel`: export to the `MagicaVoxel` format.

use std::path::PathBuf;
use std::process::ExitCode;

use crate::common::{Args, Parsed, create_parent, load_scene, usage};

pub(crate) fn voxel_scene(args: Vec<String>) -> ExitCode {
    match run(args) {
        Ok(code) | Err(code) => code,
    }
}

fn run(args: Vec<String>) -> Parsed<ExitCode> {
    let mut path: Option<String> = None;
    let mut output: Option<PathBuf> = None;
    let mut density: Option<f64> = None;
    let mut base: Option<PathBuf> = None;
    let mut args = Args::new(args).hints(&[
        ("--out", "a path"),
        ("--base", "a directory"),
        ("--density", "a number"),
        ("--voxel-size", "a number"),
    ]);
    let positive = |value: &f64| value.is_finite() && *value > 0.0;
    while let Some(argument) = args.next() {
        match argument.as_str() {
            "--out" | "-o" => output = Some(PathBuf::from(args.value("--out")?)),
            "--base" => base = Some(PathBuf::from(args.value("--base")?)),
            "--density" => {
                density =
                    Some(args.parse("--density", positive, |v| format!("invalid density `{v}`"))?);
            }
            "--voxel-size" => {
                let size = args.parse("--voxel-size", positive, |v| {
                    format!("invalid voxel size `{v}`")
                })?;
                density = Some(1.0 / size);
            }
            _ if path.is_none() => path = Some(argument),
            _ => return Err(usage(format!("unexpected argument `{argument}`"))),
        }
    }
    let path = path.ok_or_else(|| usage("`voxel` needs a .scene file"))?;
    let loaded = load_scene("voxel", &path, base.as_deref(), true)?;
    let density = density.unwrap_or_else(|| {
        // Fit the longest axis into roughly 128 voxels.
        let size = loaded.scene.bounds.size();
        let extent = size[0].max(size[1]).max(size[2]).max(1.0);
        (128.0 / extent).clamp(0.05, 8.0)
    });
    let model = scene_backend_voxel::voxelize(&loaded.scene, density)
        .map_err(|diagnostic| loaded.report(&diagnostic))?;
    let output = output.unwrap_or_else(|| loaded.default_output("vox"));
    create_parent(&output)?;
    scene_backend_voxel::write_vox(&model, &output)
        .map_err(|diagnostic| loaded.report(&diagnostic))?;
    println!(
        "wrote {} ({}x{}x{}, {} voxels, density {})",
        output.display(),
        model.size[0],
        model.size[1],
        model.size[2],
        model.voxels.len(),
        density
    );
    Ok(ExitCode::SUCCESS)
}
