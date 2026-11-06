use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use scene_syntax::Diagnostic;

fn main() -> ExitCode {
    let mut args = std::env::args().skip(1);
    let Some(command) = args.next() else {
        print_help();
        return ExitCode::SUCCESS;
    };

    match command.as_str() {
        "-V" | "--version" => {
            println!("iso-scene {}", env!("CARGO_PKG_VERSION"));
            ExitCode::SUCCESS
        }
        "-h" | "--help" => {
            print_help();
            ExitCode::SUCCESS
        }
        "spec-version" => {
            println!("{}", scene_syntax::SPEC_VERSION);
            ExitCode::SUCCESS
        }
        "check" => {
            let Some(path) = args.next() else {
                eprintln!("iso-scene: `check` needs a file");
                return ExitCode::from(2);
            };
            check(&path)
        }
        "eval" => eval(args.collect()),
        "render" => render_scene(args.collect()),
        "voxel" => voxel_scene(args.collect()),
        other => {
            eprintln!("iso-scene: unknown command `{other}`");
            eprintln!("Try `iso-scene --help`.");
            ExitCode::from(2)
        }
    }
}

fn check(path: &str) -> ExitCode {
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

fn eval(args: Vec<String>) -> ExitCode {
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

fn render_scene(args: Vec<String>) -> ExitCode {
    let mut path: Option<String> = None;
    let mut output: Option<PathBuf> = None;
    let mut args = args.into_iter();

    while let Some(argument) = args.next() {
        if argument == "--out" || argument == "-o" {
            let Some(value) = args.next() else {
                eprintln!("iso-scene: `--out` needs a path");
                return ExitCode::from(2);
            };
            output = Some(PathBuf::from(value));
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
    if Path::new(&path)
        .extension()
        .and_then(|value| value.to_str())
        != Some("scene")
    {
        eprintln!("iso-scene: `render` expects a .scene file");
        return ExitCode::from(2);
    }
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

fn voxel_scene(args: Vec<String>) -> ExitCode {
    let mut path: Option<String> = None;
    let mut output: Option<PathBuf> = None;
    let mut density: u32 = 8;
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
            match value.parse::<u32>() {
                Ok(0) | Err(_) => {
                    eprintln!("iso-scene: invalid density `{value}`");
                    return ExitCode::from(2);
                }
                Ok(value) => density = value,
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
    if Path::new(&path)
        .extension()
        .and_then(|value| value.to_str())
        != Some("scene")
    {
        eprintln!("iso-scene: `voxel` expects a .scene file");
        return ExitCode::from(2);
    }
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

fn read(path: &str) -> Option<String> {
    match fs::read_to_string(path) {
        Ok(source) => Some(source),
        Err(error) => {
            eprintln!("iso-scene: cannot read `{path}`: {error}");
            None
        }
    }
}

fn report(path: &str, source: &str, diagnostic: &Diagnostic) -> ExitCode {
    let (line, column) = line_column(source, diagnostic.span.start);
    eprintln!(
        "{path}:{line}:{column}: error[{}]: {}",
        diagnostic.code, diagnostic.message
    );
    ExitCode::from(1)
}

fn format_bounds(bounds: scene_core::Bounds) -> String {
    format!(
        "({:.3}, {:.3}, {:.3})..({:.3}, {:.3}, {:.3})",
        bounds.min[0], bounds.min[1], bounds.min[2], bounds.max[0], bounds.max[1], bounds.max[2]
    )
}

/// Convert a byte offset to a 1-based line and column.
fn line_column(source: &str, offset: u32) -> (usize, usize) {
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

fn print_help() {
    println!("iso-scene {}", env!("CARGO_PKG_VERSION"));
    println!();
    println!("Usage: iso-scene <command> [options]");
    println!();
    println!("Commands:");
    println!("  check <file>    parse a .model or .scene file and report diagnostics");
    println!("  eval <file>     parse and evaluate to a scene graph");
    println!("  render <file>   render a .scene with the iso or ortho backend");
    println!("  voxel <file>    export a .scene as a MagicaVoxel .vox model");
    println!("  spec-version    print the implemented language version");
    println!();
    println!("Options:");
    println!("  -h, --help      show this help");
    println!("  -V, --version   show the program version");
    println!();
    println!("Evaluation options:");
    println!(
        "  --fuel <n>      evaluation budget for `eval` (default {})",
        scene_core::DEFAULT_FUEL
    );
    println!();
    println!("Render backends are not implemented yet.");
}

#[cfg(test)]
mod tests {
    use super::line_column;

    #[test]
    fn spec_version_is_exposed() {
        assert_eq!(scene_syntax::SPEC_VERSION, 1);
    }

    #[test]
    fn offsets_map_to_lines_and_columns() {
        assert_eq!(line_column("", 0), (1, 1));
        assert_eq!(line_column("a\nbc", 0), (1, 1));
        assert_eq!(line_column("a\nbc", 1), (1, 2));
        assert_eq!(line_column("a\nbc", 2), (2, 1));
        assert_eq!(line_column("a\nbc", 3), (2, 2));
        assert_eq!(line_column("a\nbc", 99), (2, 3));
    }
}
