//! `sprites`: pack objects into a sprite sheet.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use crate::common::{project_root, read, report};

#[allow(
    clippy::too_many_lines,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]
pub(crate) fn sprites_scene(args: Vec<String>) -> ExitCode {
    let mut paths: Vec<String> = Vec::new();
    let mut output = PathBuf::from("out/sprites");
    let mut name: Option<String> = None;
    let mut views: Option<Vec<f64>> = None;
    let mut scale: Option<u32> = None;
    let mut tile_width: Option<u32> = None;
    let mut object_filter: Vec<String> = Vec::new();
    let mut object_exclude: Vec<String> = Vec::new();
    let mut pivot = scene_export_sprites::Pivot::default();
    let mut shadow = true;
    let mut padding = 0_u32;
    let mut debug_grid = false;
    let mut atlas = false;
    let mut max_rows: Option<u32> = None;
    let mut projection: Option<scene_export_sprites::Projection> = None;
    let mut base_override: Option<PathBuf> = None;
    // Animation flags apply to the latest `--animation`; flags given before
    // the first one apply to the first.
    let mut animations: Vec<AnimationDraft> = Vec::new();
    let mut current = AnimationDraft::default();
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
                    Ok(n) if (1..=scene_core::MAX_SCALE).contains(&n) => scale = Some(n),
                    _ => {
                        eprintln!(
                            "iso-scene: invalid scale `{v}`; use 1..={}",
                            scene_core::MAX_SCALE
                        );
                        return ExitCode::from(2);
                    }
                }
            }
            "--tile-width" => {
                let Some(v) = value("--tile-width") else {
                    return ExitCode::from(2);
                };
                match v.parse::<u32>() {
                    Ok(n) if (1..=scene_export_sprites::MAX_TILE_WIDTH).contains(&n) => {
                        tile_width = Some(n);
                    }
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
            "--debug-grid" => debug_grid = true,
            "--atlas" => atlas = true,
            "--projection" => {
                let Some(v) = value("--projection") else {
                    return ExitCode::from(2);
                };
                let Some(parsed) = scene_export_sprites::Projection::parse(&v) else {
                    eprintln!("iso-scene: `--projection` must be iso, top, front or side");
                    return ExitCode::from(2);
                };
                projection = Some(parsed);
            }
            "--max-rows" => {
                let Some(v) = value("--max-rows") else {
                    return ExitCode::from(2);
                };
                match v.parse::<u32>() {
                    Ok(n) if n >= 1 => max_rows = Some(n),
                    _ => {
                        eprintln!("iso-scene: invalid row count `{v}`");
                        return ExitCode::from(2);
                    }
                }
            }
            "--exclude" => {
                let Some(v) = value("--exclude") else {
                    return ExitCode::from(2);
                };
                object_exclude.push(v);
            }
            "--padding" => {
                let Some(v) = value("--padding") else {
                    return ExitCode::from(2);
                };
                match v.parse::<u32>() {
                    Ok(n) if n <= 256 => padding = n,
                    _ => {
                        eprintln!("iso-scene: `--padding` must be 0..=256 pixels");
                        return ExitCode::from(2);
                    }
                }
            }
            "--shadow" => {
                let Some(v) = value("--shadow") else {
                    return ExitCode::from(2);
                };
                shadow = match v.as_str() {
                    "contact" => true,
                    "none" => false,
                    _ => {
                        eprintln!("iso-scene: `--shadow` must be contact or none");
                        return ExitCode::from(2);
                    }
                };
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
                if current.object.is_some() {
                    animations.push(std::mem::take(&mut current));
                }
                current.object = Some(v);
            }
            "--label" => {
                let Some(v) = value("--label") else {
                    return ExitCode::from(2);
                };
                current.label = Some(v);
                current.touched = true;
            }
            "--from" | "--to" => {
                let flag = argument.clone();
                let Some(v) = value(&flag) else {
                    return ExitCode::from(2);
                };
                match v.parse::<f64>() {
                    Ok(n) if n.is_finite() => {
                        if flag == "--from" {
                            current.from = n;
                        } else {
                            current.to = n;
                        }
                        current.touched = true;
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
                    Ok(n) if (1..=scene_export_sprites::MAX_FRAMES).contains(&n) => {
                        current.frames = n;
                        current.touched = true;
                    }
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
                    Ok(n) if n >= 1 => {
                        current.duration = n;
                        current.touched = true;
                    }
                    _ => {
                        eprintln!("iso-scene: invalid duration `{v}`");
                        return ExitCode::from(2);
                    }
                }
            }
            "--durations" => {
                let Some(v) = value("--durations") else {
                    return ExitCode::from(2);
                };
                let mut durations = Vec::new();
                for part in v.split(',') {
                    match part.trim().parse::<u32>() {
                        Ok(n) if n >= 1 => durations.push(n),
                        _ => {
                            eprintln!("iso-scene: invalid duration `{part}`");
                            return ExitCode::from(2);
                        }
                    }
                }
                current.durations = durations;
                current.touched = true;
            }
            "--once" => {
                current.looped = false;
                current.touched = true;
            }
            "--rotation" => {
                let Some(v) = value("--rotation") else {
                    return ExitCode::from(2);
                };
                match v.parse::<u16>() {
                    Ok(n) if matches!(n, 0 | 90 | 180 | 270) => {
                        current.rotation = n;
                        current.touched = true;
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
    if current.object.is_some() {
        animations.push(current);
    } else if current.touched {
        eprintln!(
            "iso-scene: `--from/--to/--frames/--duration/--durations/--once/--rotation/--label` need `--animation`"
        );
        return ExitCode::from(2);
    }
    for draft in &animations {
        if !draft.durations.is_empty() && draft.durations.len() != draft.frames as usize {
            eprintln!(
                "iso-scene: `--durations` lists {} values for {} frames",
                draft.durations.len(),
                draft.frames
            );
            return ExitCode::from(2);
        }
    }
    let name = name.unwrap_or_else(|| "sheet".to_string());
    if let Err(message) = scene_export_sprites::validate_name(&name) {
        eprintln!("iso-scene: {message}");
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
    let mut scene_projection: Option<scene_export_sprites::Projection> = None;

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
                scene_projection.get_or_insert(match scene.render {
                    scene_syntax::ast::RenderKind::Iso => scene_export_sprites::Projection::Iso,
                    scene_syntax::ast::RenderKind::Top => scene_export_sprites::Projection::Top,
                    scene_syntax::ast::RenderKind::Front => scene_export_sprites::Projection::Front,
                    scene_syntax::ast::RenderKind::Side => scene_export_sprites::Projection::Side,
                });
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

    for wanted in object_filter.iter().chain(&object_exclude) {
        if !objects.iter().any(|entry| entry.name == *wanted) {
            eprintln!("iso-scene: no object named `{wanted}`");
            return ExitCode::from(2);
        }
    }
    if !object_filter.is_empty() {
        objects.retain(|entry| object_filter.contains(&entry.name));
    }
    objects.retain(|entry| !object_exclude.contains(&entry.name));
    if objects.is_empty() {
        eprintln!("iso-scene: every object was filtered out");
        return ExitCode::from(2);
    }
    for draft in &animations {
        let wanted = draft.object.as_deref().unwrap_or_default();
        if !objects.iter().any(|entry| entry.name == wanted) {
            eprintln!("iso-scene: no object named `{wanted}` to animate");
            return ExitCode::from(2);
        }
    }

    let scale = scale.or(scene_scale).unwrap_or(16);
    let mut specs: Vec<scene_export_sprites::AnimationSpec> = Vec::new();
    for draft in animations {
        let object = draft.object.unwrap_or_default();
        let label = draft.label.unwrap_or_else(|| object.clone());
        if specs.iter().any(|spec| spec.name == label) {
            eprintln!("iso-scene: animation `{label}` is defined twice; name them with `--label`");
            return ExitCode::from(2);
        }
        specs.push(scene_export_sprites::AnimationSpec {
            name: label,
            object,
            from: draft.from,
            to: draft.to,
            frames: draft.frames,
            duration: draft.duration,
            durations: draft.durations,
            looped: draft.looped,
            rotation: draft.rotation,
        });
    }
    let options = scene_export_sprites::SheetOptions {
        name,
        scale,
        projection: projection.or(scene_projection).unwrap_or_default(),
        views: views.unwrap_or_else(|| vec![0.0, 90.0, 180.0, 270.0]),
        pivot,
        shadow,
        padding,
        debug_grid,
        atlas,
        max_rows,
        animations: specs,
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
    for extra in report.debug.iter().chain(&report.atlas) {
        println!("wrote {}", extra.display());
    }
    if report.pages > 1 {
        println!(
            "{} pages; the figures above are for the first, see {} for all",
            report.pages,
            report.manifest.display()
        );
    }
    ExitCode::SUCCESS
}

/// Animation flags collected for one `--animation`.
pub(crate) struct AnimationDraft {
    object: Option<String>,
    label: Option<String>,
    from: f64,
    to: f64,
    frames: u32,
    duration: u32,
    durations: Vec<u32>,
    looped: bool,
    rotation: u16,
    /// Any animation flag was given (to reject flags without `--animation`).
    touched: bool,
}

impl Default for AnimationDraft {
    fn default() -> Self {
        Self {
            object: None,
            label: None,
            from: 0.0,
            to: 90.0,
            frames: 7,
            duration: 120,
            durations: Vec::new(),
            looped: true,
            rotation: 0,
            touched: false,
        }
    }
}

/// Add `object` unless an identical object of that name is already on the
/// sheet. A different object with the same name is a conflict: picking one
/// silently would drop the other from the sheet.
pub(crate) fn push_sheet_object(
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
                source: Some(file.to_string()),
            });
            Ok(())
        }
    }
}

/// Merge `incoming` materials; the same name bound to two files is a conflict.
pub(crate) fn merge_materials(
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

pub(crate) fn same_file(a: &Path, b: &Path) -> bool {
    a == b
        || matches!(
            (a.canonicalize(), b.canonicalize()),
            (Ok(a), Ok(b)) if a == b
        )
}
