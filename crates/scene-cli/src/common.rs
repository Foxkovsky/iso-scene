//! Helpers shared by the commands: reading files, reporting diagnostics,
//! the project root and model previews.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use scene_syntax::Diagnostic;

/// Directory that `include` and `material` paths must stay inside.
///
/// Without `--base` it is the working directory. With `--base`, the working
/// directory is kept when it contains the base (so includes may still reach
/// siblings of the base), otherwise the base itself is the root, so a project
/// can be built from anywhere.
pub(crate) fn project_root(base_override: Option<&Path>) -> PathBuf {
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let Some(base) = base_override else {
        return cwd;
    };
    let base = base.canonicalize().unwrap_or_else(|_| base.to_path_buf());
    let cwd_canonical = cwd.canonicalize().unwrap_or(cwd);
    if base.starts_with(&cwd_canonical) {
        cwd_canonical
    } else {
        base
    }
}

pub(crate) fn read(path: &str) -> Option<String> {
    match fs::read_to_string(path) {
        Ok(source) => Some(source),
        Err(error) => {
            eprintln!("iso-scene: cannot read `{path}`: {error}");
            None
        }
    }
}

pub(crate) fn report(path: &str, source: &str, diagnostic: &Diagnostic) -> ExitCode {
    let (line, column) = line_column(source, diagnostic.span.start);
    eprintln!(
        "{path}:{line}:{column}: error[{}]: {}",
        diagnostic.code, diagnostic.message
    );
    ExitCode::from(1)
}

pub(crate) fn format_bounds(bounds: scene_core::Bounds) -> String {
    format!(
        "({:.3}, {:.3}, {:.3})..({:.3}, {:.3}, {:.3})",
        bounds.min[0], bounds.min[1], bounds.min[2], bounds.max[0], bounds.max[1], bounds.max[2]
    )
}

/// Convert a byte offset to a 1-based line and column.
pub(crate) fn line_column(source: &str, offset: u32) -> (usize, usize) {
    let offset = usize::try_from(offset)
        .unwrap_or(usize::MAX)
        .min(source.len());
    let prefix = source.get(..offset).unwrap_or(source);
    let mut line = 1;
    let mut column = 1;
    for character in prefix.chars() {
        if character == '\n' {
            line += 1;
            column = 1;
        } else {
            column += 1;
        }
    }
    (line, column)
}

/// Build a one-row scene from a `.model` file so it can be previewed.
#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
pub(crate) fn model_preview(
    model: &scene_core::CompiledModel,
) -> Result<scene_core::SceneGraph, String> {
    if model.objects.is_empty() {
        return Err("the model defines no objects".to_string());
    }
    let mut extent: f64 = 1.0;
    for object in model.objects.values() {
        let local = object.local_bounds();
        let size = local.size();
        extent = extent.max(size[0]).max(size[1]).max(size[2]);
    }
    let scale = (220.0 / extent).round().clamp(1.0, 64.0) as u32;

    let mut instances = Vec::new();
    let mut offset = 0.0_f64;
    for object in model.objects.values() {
        instances.push(scene_core::Instance {
            object: object.clone(),
            at: [offset, 0.0, 0.0],
            rotation: 0,
            angle: 0.0,
        });
        offset += object.width + object.width.mul_add(0.25, 1.0);
    }

    let mut bounds = scene_core::Bounds {
        min: [f64::INFINITY; 3],
        max: [f64::NEG_INFINITY; 3],
    };
    for instance in &instances {
        let local = instance.world_bounds();
        for axis in 0..3 {
            bounds.min[axis] = bounds.min[axis].min(local.min[axis]);
            bounds.max[axis] = bounds.max[axis].max(local.max[axis]);
        }
    }

    Ok(scene_core::SceneGraph {
        render: scene_syntax::ast::RenderKind::Iso,
        scale,
        canvas: None,
        sun: [135.0, 45.0],
        ambient: 0.35,
        background: scene_syntax::ast::Background::Transparent,
        materials: model.materials.clone(),
        instances,
        bounds,
    })
}
