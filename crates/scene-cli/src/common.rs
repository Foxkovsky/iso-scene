//! Helpers shared by the commands: argument parsing, reading files,
//! reporting diagnostics, the project root and model previews.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use scene_syntax::Diagnostic;

/// Result of a command step: `Err` carries the exit code of a failure whose
/// message has already been printed, so commands can use `?`.
pub(crate) type Parsed<T> = Result<T, ExitCode>;

/// Print a usage error and return exit code 2.
pub(crate) fn usage(message: impl std::fmt::Display) -> ExitCode {
    eprintln!("iso-scene: {message}");
    ExitCode::from(2)
}

/// Print an export diagnostic and return exit code 1.
pub(crate) fn failed(diagnostic: &Diagnostic) -> ExitCode {
    eprintln!(
        "iso-scene: error[{}]: {}",
        diagnostic.code, diagnostic.message
    );
    ExitCode::from(1)
}

/// The arguments of one command, consumed flag by flag.
pub(crate) struct Args {
    items: std::vec::IntoIter<String>,
    /// What a flag's value is, for "`--flag` needs ..." messages.
    hints: &'static [(&'static str, &'static str)],
}

impl Iterator for Args {
    type Item = String;

    fn next(&mut self) -> Option<String> {
        self.items.next()
    }
}

impl Args {
    pub(crate) fn new(args: Vec<String>) -> Self {
        Self {
            items: args.into_iter(),
            hints: &[],
        }
    }

    /// Describe flag values (e.g. `("--out", "a path")`) in usage errors.
    pub(crate) fn hints(mut self, hints: &'static [(&'static str, &'static str)]) -> Self {
        self.hints = hints;
        self
    }

    /// The value after `flag`.
    pub(crate) fn value(&mut self, flag: &str) -> Parsed<String> {
        let hint = self
            .hints
            .iter()
            .find(|(name, _)| *name == flag)
            .map_or("a value", |(_, hint)| hint);
        self.items
            .next()
            .ok_or_else(|| usage(format!("`{flag}` needs {hint}")))
    }

    /// The value after `flag`, parsed and accepted by `valid`; otherwise the
    /// `invalid` message for the raw text.
    pub(crate) fn parse<T: std::str::FromStr>(
        &mut self,
        flag: &str,
        valid: impl FnOnce(&T) -> bool,
        invalid: impl FnOnce(&str) -> String,
    ) -> Parsed<T> {
        let text = self.value(flag)?;
        match text.parse::<T>() {
            Ok(value) if valid(&value) => Ok(value),
            _ => Err(usage(invalid(&text))),
        }
    }

    /// The comma-separated values after `flag`, each parsed and accepted by
    /// `valid`; otherwise the `invalid` message for the offending item.
    pub(crate) fn list<T: std::str::FromStr>(
        &mut self,
        flag: &str,
        valid: impl Fn(&T) -> bool,
        invalid: impl Fn(&str) -> String,
    ) -> Parsed<Vec<T>> {
        let text = self.value(flag)?;
        text.split(',')
            .map(|part| match part.trim().parse::<T>() {
                Ok(value) if valid(&value) => Ok(value),
                _ => Err(usage(invalid(part))),
            })
            .collect()
    }
}

/// An input file compiled to a scene graph, with its source for diagnostics.
pub(crate) struct Loaded {
    pub(crate) path: String,
    pub(crate) source: String,
    pub(crate) scene: scene_core::SceneGraph,
    /// Directory `include` and `material` paths must stay inside.
    pub(crate) root: PathBuf,
}

impl Loaded {
    /// Report a diagnostic against this file.
    pub(crate) fn report(&self, diagnostic: &Diagnostic) -> ExitCode {
        report(&self.path, &self.source, diagnostic)
    }

    /// `out/<stem>.<extension>` next to the working directory.
    pub(crate) fn default_output(&self, extension: &str) -> PathBuf {
        let stem = Path::new(&self.path)
            .file_stem()
            .and_then(|value| value.to_str())
            .unwrap_or("scene");
        PathBuf::from("out").join(format!("{stem}.{extension}"))
    }
}

/// Read and compile `path` for `command`: a `.scene`, or (with `models`) a
/// `.model` shown as a one-row preview. Relative paths resolve from `base`
/// (by default the file's directory) inside [`project_root`].
pub(crate) fn load_scene(
    command: &str,
    path: &str,
    base: Option<&Path>,
    models: bool,
) -> Parsed<Loaded> {
    let file = Path::new(path);
    let extension = file.extension().and_then(|value| value.to_str());
    let is_model = extension == Some("model");
    if extension != Some("scene") && !(models && is_model) {
        return Err(usage(if models {
            format!("`{command}` expects a .scene or .model file")
        } else {
            format!("`{command}` expects a .scene file")
        }));
    }
    let source = read(path).ok_or(ExitCode::from(2))?;
    let program =
        scene_syntax::parse(&source).map_err(|diagnostic| report(path, &source, &diagnostic))?;
    let fallback = file.parent().unwrap_or_else(|| Path::new("."));
    let directory = base.unwrap_or(fallback);
    let root = project_root(base);
    let scene = if is_model {
        let model = scene_core::compile_model(&program, directory)
            .map_err(|diagnostic| report(path, &source, &diagnostic))?;
        model_preview(&model).map_err(usage)?
    } else {
        scene_core::compile_scene(&program, directory, &root)
            .map_err(|diagnostic| report(path, &source, &diagnostic))?
    };
    Ok(Loaded {
        path: path.to_string(),
        source,
        scene,
        root,
    })
}

/// Create the directory that will hold `output`.
pub(crate) fn create_parent(output: &Path) -> Parsed<()> {
    match output.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => fs::create_dir_all(parent)
            .map_err(|error| usage(format!("cannot create `{}`: {error}", parent.display()))),
        _ => Ok(()),
    }
}

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
