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
        "tiled" => tiled_scene(args.collect()),
        "animate" => animate_scene(args.collect()),
        "sprites" => sprites_scene(args.collect()),
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

#[allow(clippy::too_many_lines)]
fn render_scene(args: Vec<String>) -> ExitCode {
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

/// Build a one-row scene from a `.model` file so it can be previewed.
#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
fn model_preview(model: &scene_core::CompiledModel) -> Result<scene_core::SceneGraph, String> {
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

#[allow(
    clippy::too_many_lines,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]
fn animate_scene(args: Vec<String>) -> ExitCode {
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
                if let Ok(n) = v.parse() {
                    from = n;
                } else {
                    eprintln!("iso-scene: invalid angle `{v}`");
                    return ExitCode::from(2);
                }
            }
            "--to" => {
                let Some(v) = value("--to") else {
                    return ExitCode::from(2);
                };
                if let Ok(n) = v.parse() {
                    to = n;
                } else {
                    eprintln!("iso-scene: invalid angle `{v}`");
                    return ExitCode::from(2);
                }
            }
            "--frames" => {
                let Some(v) = value("--frames") else {
                    return ExitCode::from(2);
                };
                match v.parse::<u32>() {
                    Ok(n) if n >= 1 => frames = n,
                    _ => {
                        eprintln!("iso-scene: invalid frame count `{v}`");
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

    let angles: Vec<f64> = if frames <= 1 {
        vec![from]
    } else {
        (0..frames)
            .map(|frame| from + (to - from) * f64::from(frame) / f64::from(frames - 1))
            .collect()
    };
    let mut rendered = Vec::with_capacity(angles.len());
    for angle in &angles {
        let mut frame = scene.clone();
        frame.background = scene_syntax::ast::Background::Transparent;
        frame.instances[index].angle = *angle;
        match scene_backend_iso::render_with_origin(&frame) {
            Ok((image, origin)) => rendered.push((image, origin)),
            Err(diagnostic) => return report(&path, &source, &diagnostic),
        }
    }

    let mut min_x = f64::INFINITY;
    let mut min_y = f64::INFINITY;
    let mut max_x = f64::NEG_INFINITY;
    let mut max_y = f64::NEG_INFINITY;
    for (image, origin) in &rendered {
        min_x = min_x.min(origin[0]);
        min_y = min_y.min(origin[1]);
        max_x = max_x.max(origin[0] + f64::from(image.width));
        max_y = max_y.max(origin[1] + f64::from(image.height));
    }
    let frame_w = u32::try_from((max_x - min_x).ceil() as isize)
        .unwrap_or(1)
        .max(1);
    let frame_h = u32::try_from((max_y - min_y).ceil() as isize)
        .unwrap_or(1)
        .max(1);
    let sheet_w = frame_w.saturating_mul(rendered.len() as u32);
    let mut sheet = scene_render::Image {
        width: sheet_w,
        height: frame_h,
        rgba: vec![0; (sheet_w as usize) * (frame_h as usize) * 4],
    };
    for (index, (image, origin)) in rendered.iter().enumerate() {
        let left = index as u32 * frame_w;
        let x = (origin[0] - min_x).round() as i64 + i64::from(left);
        let y = (origin[1] - min_y).round() as i64;
        scene_render::composite_over(&mut sheet, image, x, y, [left, 0, frame_w, frame_h]);
    }

    let output = output.unwrap_or_else(|| {
        let stem = file
            .file_stem()
            .and_then(|value| value.to_str())
            .unwrap_or("animation");
        PathBuf::from("out").join(stem)
    });
    if let Err(error) = fs::create_dir_all(&output) {
        eprintln!("iso-scene: cannot create `{}`: {error}", output.display());
        return ExitCode::from(2);
    }
    let stem = output
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("animation")
        .to_string();
    let png_name = format!("{stem}.png");
    let png_path = output.join(&png_name);
    if let Err(message) = scene_render::write_png(&sheet, &png_path) {
        eprintln!("iso-scene: {message}");
        return ExitCode::from(2);
    }
    let animation: Vec<serde_json::Value> = (0..rendered.len())
        .map(|frame| serde_json::json!({"tileid": frame, "duration": duration}))
        .collect();
    let tileset = serde_json::json!({
        "type": "tileset",
        "name": stem,
        "version": "1.10",
        "tiledversion": "1.12.2",
        "tilewidth": frame_w,
        "tileheight": frame_h,
        "tilecount": rendered.len(),
        "columns": rendered.len(),
        "image": png_name,
        "imagewidth": sheet_w,
        "imageheight": frame_h,
        "margin": 0,
        "spacing": 0,
        "tiles": [{"id": 0, "animation": animation}]
    });
    let tsj_name = format!("{stem}.tsj");
    if let Err(error) = fs::write(
        output.join(&tsj_name),
        serde_json::to_string_pretty(&tileset).unwrap_or_default() + "\n",
    ) {
        eprintln!("iso-scene: cannot write `{}`: {error}", output.display());
        return ExitCode::from(2);
    }
    let map = serde_json::json!({
        "type": "map",
        "version": "1.10",
        "tiledversion": "1.12.2",
        "orientation": "isometric",
        "renderorder": "right-down",
        "width": 1,
        "height": 1,
        "tilewidth": frame_w,
        "tileheight": frame_h,
        "infinite": false,
        "nextlayerid": 2,
        "nextobjectid": 1,
        "tilesets": [{"firstgid": 1, "source": tsj_name}],
        "layers": [{
            "type": "tilelayer",
            "name": "animation",
            "id": 1,
            "width": 1,
            "height": 1,
            "x": 0,
            "y": 0,
            "opacity": 1,
            "visible": true,
            "data": [1]
        }]
    });
    if let Err(error) = fs::write(
        output.join("scene.tmj"),
        serde_json::to_string_pretty(&map).unwrap_or_default() + "\n",
    ) {
        eprintln!("iso-scene: cannot write scene.tmj: {error}");
        return ExitCode::from(2);
    }
    println!(
        "wrote {} frames {}x{} ({} ms each); open `{}` in Tiled",
        rendered.len(),
        frame_w,
        frame_h,
        duration,
        output.join("scene.tmj").display()
    );
    ExitCode::SUCCESS
}

#[allow(
    clippy::too_many_lines,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]
fn sprites_scene(args: Vec<String>) -> ExitCode {
    let mut paths: Vec<String> = Vec::new();
    let mut output = PathBuf::from("out/sprites");
    let mut name: Option<String> = None;
    let mut views: Option<Vec<f64>> = None;
    let mut scale: Option<u32> = None;
    let mut tile_width: Option<u32> = None;
    let mut object_filter: Vec<String> = Vec::new();
    let mut pivot = scene_export_sprites::Pivot::default();
    let mut base_override: Option<PathBuf> = None;
    let mut animation_object: Option<String> = None;
    let mut from = 0.0_f64;
    let mut to = 90.0_f64;
    let mut frames = 7_u32;
    let mut duration = 120_u32;
    let mut rotation = 0_u16;
    let mut animation_flag = false;
    let mut args = args.into_iter();

    while let Some(argument) = args.next() {
        let mut value = |flag: &str| -> Option<String> {
            args.next().or_else(|| {
                eprintln!("iso-scene: `{flag}` needs a value");
                None
            })
        };
        match argument.as_str() {
            "--out" | "-o" => {
                let Some(v) = value("--out") else {
                    return ExitCode::from(2);
                };
                output = PathBuf::from(v);
            }
            "--base" => {
                let Some(v) = value("--base") else {
                    return ExitCode::from(2);
                };
                base_override = Some(PathBuf::from(v));
            }
            "--name" => {
                let Some(v) = value("--name") else {
                    return ExitCode::from(2);
                };
                name = Some(v);
            }
            "--scale" => {
                let Some(v) = value("--scale") else {
                    return ExitCode::from(2);
                };
                match v.parse::<u32>() {
                    Ok(n) if n >= 1 => scale = Some(n),
                    _ => {
                        eprintln!("iso-scene: invalid scale `{v}`");
                        return ExitCode::from(2);
                    }
                }
            }
            "--tile-width" => {
                let Some(v) = value("--tile-width") else {
                    return ExitCode::from(2);
                };
                match v.parse::<u32>() {
                    Ok(n) if n >= 1 => tile_width = Some(n),
                    _ => {
                        eprintln!("iso-scene: invalid tile width `{v}`");
                        return ExitCode::from(2);
                    }
                }
            }
            "--views" => {
                let Some(v) = value("--views") else {
                    return ExitCode::from(2);
                };
                views = match v.as_str() {
                    "1" => Some(vec![0.0]),
                    "2" => Some(vec![0.0, 180.0]),
                    "4" | "8" | "16" => {
                        let count: u16 = v.parse().unwrap_or(4);
                        Some(
                            (0..count)
                                .map(|step| f64::from(step) * 360.0 / f64::from(count))
                                .collect(),
                        )
                    }
                    _ => {
                        eprintln!("iso-scene: `--views` must be 1, 2, 4, 8 or 16");
                        return ExitCode::from(2);
                    }
                };
            }
            "--angles" => {
                let Some(v) = value("--angles") else {
                    return ExitCode::from(2);
                };
                let mut angles = Vec::new();
                for part in v.split(',') {
                    match part.trim().parse::<f64>() {
                        Ok(angle) if angle.is_finite() => angles.push(angle),
                        _ => {
                            eprintln!("iso-scene: invalid angle `{part}`");
                            return ExitCode::from(2);
                        }
                    }
                }
                if angles.is_empty() {
                    eprintln!("iso-scene: `--angles` needs at least one angle");
                    return ExitCode::from(2);
                }
                views = Some(angles);
            }
            "--object" => {
                let Some(v) = value("--object") else {
                    return ExitCode::from(2);
                };
                object_filter.push(v);
            }
            "--pivot" => {
                let Some(v) = value("--pivot") else {
                    return ExitCode::from(2);
                };
                let Some(parsed) = scene_export_sprites::Pivot::parse(&v) else {
                    eprintln!("iso-scene: `--pivot` must be front, back, left, right or center");
                    return ExitCode::from(2);
                };
                pivot = parsed;
            }
            "--animation" => {
                let Some(v) = value("--animation") else {
                    return ExitCode::from(2);
                };
                animation_object = Some(v);
            }
            "--from" => {
                let Some(v) = value("--from") else {
                    return ExitCode::from(2);
                };
                match v.parse::<f64>() {
                    Ok(n) if n.is_finite() => {
                        from = n;
                        animation_flag = true;
                    }
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
                    Ok(n) if n.is_finite() => {
                        to = n;
                        animation_flag = true;
                    }
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
                    Ok(n) if n >= 1 => {
                        frames = n;
                        animation_flag = true;
                    }
                    _ => {
                        eprintln!("iso-scene: invalid frame count `{v}`");
                        return ExitCode::from(2);
                    }
                }
            }
            "--duration" => {
                let Some(v) = value("--duration") else {
                    return ExitCode::from(2);
                };
                match v.parse::<u32>() {
                    Ok(n) if n >= 1 => {
                        duration = n;
                        animation_flag = true;
                    }
                    _ => {
                        eprintln!("iso-scene: invalid duration `{v}`");
                        return ExitCode::from(2);
                    }
                }
            }
            "--rotation" => {
                let Some(v) = value("--rotation") else {
                    return ExitCode::from(2);
                };
                match v.parse::<u16>() {
                    Ok(n) if matches!(n, 0 | 90 | 180 | 270) => {
                        rotation = n;
                        animation_flag = true;
                    }
                    _ => {
                        eprintln!("iso-scene: `--rotation` must be 0, 90, 180 or 270");
                        return ExitCode::from(2);
                    }
                }
            }
            other => paths.push(other.to_string()),
        }
    }

    if paths.is_empty() {
        eprintln!("iso-scene: `sprites` needs a .scene or .model file");
        return ExitCode::from(2);
    }
    if animation_object.is_none() && animation_flag {
        eprintln!("iso-scene: `--from/--to/--frames/--duration` need `--animation`");
        return ExitCode::from(2);
    }
    let name = name.unwrap_or_else(|| "sheet".to_string());
    if let Err(message) = scene_export_sprites::validate_name(&name) {
        eprintln!("iso-scene: {message}");
        return ExitCode::from(2);
    }
    if frames > scene_export_sprites::MAX_FRAMES {
        eprintln!(
            "iso-scene: `--frames` must be at most {}",
            scene_export_sprites::MAX_FRAMES
        );
        return ExitCode::from(2);
    }

    let mut objects: Vec<scene_export_sprites::SheetObject> = Vec::new();
    // Object and material name -> file that first defined it, for conflicts.
    let mut object_files: std::collections::BTreeMap<String, String> =
        std::collections::BTreeMap::new();
    let mut materials: std::collections::BTreeMap<String, PathBuf> =
        std::collections::BTreeMap::new();
    let mut material_files: std::collections::BTreeMap<String, String> =
        std::collections::BTreeMap::new();
    let mut scene_scale: Option<u32> = None;

    for path in &paths {
        let Some(source) = read(path) else {
            return ExitCode::from(2);
        };
        let program = match scene_syntax::parse(&source) {
            Ok(program) => program,
            Err(diagnostic) => return report(path, &source, &diagnostic),
        };
        let file = Path::new(path);
        let fallback_base = file.parent().unwrap_or_else(|| Path::new("."));
        let base = base_override.as_deref().unwrap_or(fallback_base);
        let root = project_root(base_override.as_deref());
        match file.extension().and_then(|value| value.to_str()) {
            Some("scene") => {
                let scene = match scene_core::compile_scene(&program, base, &root) {
                    Ok(scene) => scene,
                    Err(diagnostic) => return report(path, &source, &diagnostic),
                };
                scene_scale.get_or_insert(scene.scale);
                if scale.is_none() && scene_scale.is_some_and(|known| known != scene.scale) {
                    eprintln!("iso-scene: scenes use different scales; pass --scale to pick one");
                    return ExitCode::from(2);
                }
                if let Err(message) =
                    merge_materials(&mut materials, &mut material_files, &scene.materials, path)
                {
                    eprintln!("iso-scene: {message}");
                    return ExitCode::from(2);
                }
                for instance in &scene.instances {
                    if let Err(message) =
                        push_sheet_object(&mut objects, &mut object_files, &instance.object, path)
                    {
                        eprintln!("iso-scene: {message}");
                        return ExitCode::from(2);
                    }
                }
            }
            Some("model") => {
                let model = match scene_core::compile_model(&program, base) {
                    Ok(model) => model,
                    Err(diagnostic) => return report(path, &source, &diagnostic),
                };
                if let Err(message) =
                    merge_materials(&mut materials, &mut material_files, &model.materials, path)
                {
                    eprintln!("iso-scene: {message}");
                    return ExitCode::from(2);
                }
                for object in model.objects.values() {
                    if let Err(message) =
                        push_sheet_object(&mut objects, &mut object_files, object, path)
                    {
                        eprintln!("iso-scene: {message}");
                        return ExitCode::from(2);
                    }
                }
            }
            _ => {
                eprintln!("iso-scene: `sprites` expects .scene or .model files, got `{path}`");
                return ExitCode::from(2);
            }
        }
    }

    for wanted in &object_filter {
        if !objects.iter().any(|entry| entry.name == *wanted) {
            eprintln!("iso-scene: no object named `{wanted}`");
            return ExitCode::from(2);
        }
    }
    if !object_filter.is_empty() {
        objects.retain(|entry| object_filter.contains(&entry.name));
    }
    if let Some(wanted) = &animation_object
        && !objects.iter().any(|entry| entry.name == *wanted)
    {
        eprintln!("iso-scene: no object named `{wanted}` to animate");
        return ExitCode::from(2);
    }

    let scale = scale.or(scene_scale).unwrap_or(16);
    let animation = animation_object.map(|object| scene_export_sprites::AnimationSpec {
        object,
        from,
        to,
        frames,
        duration,
        rotation,
    });
    let options = scene_export_sprites::SheetOptions {
        name,
        scale,
        views: views.unwrap_or_else(|| vec![0.0, 90.0, 180.0, 270.0]),
        pivot,
        animation,
        tile_width: tile_width.or_else(|| scale.checked_mul(2)),
    };

    let report = match scene_export_sprites::export(&objects, &materials, &options, &output) {
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
        "wrote {} ({}x{} cells of {}x{}, {} sprites, {} frames); {} and {}",
        report.image.display(),
        report.columns,
        report.rows,
        report.cells[0],
        report.cells[1],
        report.sprites,
        report.frames,
        report.manifest.display(),
        report.tileset.display()
    );
    ExitCode::SUCCESS
}

/// Add `object` unless an identical object of that name is already on the
/// sheet. A different object with the same name is a conflict: picking one
/// silently would drop the other from the sheet.
fn push_sheet_object(
    objects: &mut Vec<scene_export_sprites::SheetObject>,
    files: &mut std::collections::BTreeMap<String, String>,
    object: &std::sync::Arc<scene_core::CompiledObject>,
    file: &str,
) -> Result<(), String> {
    match objects.iter().find(|entry| entry.name == object.name) {
        Some(entry) if *entry.object == **object => Ok(()),
        Some(_) => {
            let first = files.get(&object.name).map_or("", String::as_str);
            Err(format!(
                "object `{}` in `{file}` differs from the one in `{first}`; rename one or pick with `--object`",
                object.name
            ))
        }
        None => {
            files.insert(object.name.clone(), file.to_string());
            objects.push(scene_export_sprites::SheetObject {
                name: object.name.clone(),
                object: object.clone(),
            });
            Ok(())
        }
    }
}

/// Merge `incoming` materials; the same name bound to two files is a conflict.
fn merge_materials(
    materials: &mut std::collections::BTreeMap<String, PathBuf>,
    files: &mut std::collections::BTreeMap<String, String>,
    incoming: &std::collections::BTreeMap<String, PathBuf>,
    file: &str,
) -> Result<(), String> {
    for (key, value) in incoming {
        match materials.get(key) {
            Some(known) if same_file(known, value) => {}
            Some(known) => {
                let first = files.get(key).map_or("", String::as_str);
                return Err(format!(
                    "material `{key}` is `{}` in `{file}` but `{}` in `{first}`",
                    value.display(),
                    known.display()
                ));
            }
            None => {
                files.insert(key.clone(), file.to_string());
                materials.insert(key.clone(), value.clone());
            }
        }
    }
    Ok(())
}

fn same_file(a: &Path, b: &Path) -> bool {
    a == b
        || matches!(
            (a.canonicalize(), b.canonicalize()),
            (Ok(a), Ok(b)) if a == b
        )
}

/// Directory that `include` and `material` paths must stay inside.
///
/// Without `--base` it is the working directory. With `--base`, the working
/// directory is kept when it contains the base (so includes may still reach
/// siblings of the base), otherwise the base itself is the root, so a project
/// can be built from anywhere.
fn project_root(base_override: Option<&Path>) -> PathBuf {
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

#[allow(clippy::too_many_lines)]
fn voxel_scene(args: Vec<String>) -> ExitCode {
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

fn tiled_scene(args: Vec<String>) -> ExitCode {
    let mut path: Option<String> = None;
    let mut walls_path: Option<String> = None;
    let mut objects_as_tiles = false;
    let mut walls_in_front = false;
    let mut base_override: Option<PathBuf> = None;
    let mut output = PathBuf::from("out/tiled");
    let mut args = args.into_iter();

    while let Some(argument) = args.next() {
        if argument == "--out" || argument == "-o" {
            let Some(value) = args.next() else {
                eprintln!("iso-scene: `--out` needs a path");
                return ExitCode::from(2);
            };
            output = PathBuf::from(value);
        } else if argument == "--base" {
            let Some(value) = args.next() else {
                eprintln!("iso-scene: `--base` needs a directory");
                return ExitCode::from(2);
            };
            base_override = Some(PathBuf::from(value));
        } else if argument == "--tile-layer" {
            objects_as_tiles = true;
        } else if argument == "--walls-front" {
            walls_in_front = true;
        } else if argument == "--walls" {
            let Some(value) = args.next() else {
                eprintln!("iso-scene: `--walls` needs a .walls file");
                return ExitCode::from(2);
            };
            walls_path = Some(value);
        } else if path.is_none() {
            path = Some(argument);
        } else {
            eprintln!("iso-scene: unexpected argument `{argument}`");
            return ExitCode::from(2);
        }
    }

    let Some(path) = path else {
        eprintln!("iso-scene: `tiled` needs a .scene file");
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
        eprintln!("iso-scene: `tiled` expects a .scene file");
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
    let scene = match scene_core::compile_scene(&program, base, &root) {
        Ok(scene) => scene,
        Err(diagnostic) => return report(&path, &source, &diagnostic),
    };
    let walls = if let Some(walls_path) = walls_path {
        let walls_file = Path::new(&walls_path);
        match scene_walls::load(walls_file, &root) {
            Ok(walls) => Some(walls),
            Err(diagnostic) => {
                let walls_source = fs::read_to_string(walls_file).unwrap_or_default();
                return report(&walls_path, &walls_source, &diagnostic);
            }
        }
    } else {
        None
    };
    let exported = match scene_export_tiled::export_with(
        &scene,
        walls.as_ref(),
        objects_as_tiles,
        walls_in_front,
        &output,
    ) {
        Ok(report) => report,
        Err(diagnostic) => return report(&path, &source, &diagnostic),
    };
    println!(
        "wrote {} ({}x{} cells, {} instances); open it in Tiled; preview: {}",
        exported.map.display(),
        exported.cells[0],
        exported.cells[1],
        exported.instances,
        exported.preview.as_ref().map_or_else(
            || "none (no objects)".to_string(),
            |p| p.display().to_string()
        )
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
    println!("  render <file>   render a .scene to PNG (iso or ortho); -o out.svg writes iso SVG");
    println!("  voxel <file>    export a .scene as a MagicaVoxel .vox model");
    println!("  tiled <file>    export a .scene as an isometric Tiled map");
    println!("  animate <file>  render a rotation animation as a sprite sheet + animated tile");
    println!(
        "  sprites <file>  pack objects from .scene/.model files into a sprite sheet + JSON/TSJ"
    );
    println!("                  --views 1|2|4|8|16 or --angles 0,45,90,...; --object filters");
    println!("                  --pivot front|back|left|right|center anchors static views");
    println!(
        "                  --animation <name> --from --to --frames --duration adds a frame row"
    );
    println!("                  --walls <file> adds a 16-way wall tile layer with Wang set");
    println!(
        "                  --tile-layer puts objects in a tile layer instead of an object layer"
    );
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
