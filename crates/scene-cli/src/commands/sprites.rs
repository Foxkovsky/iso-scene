//! `sprites`: pack objects into a sprite sheet.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use scene_export_sprites::{AnimationSpec, Pivot, Projection, SheetObject, SheetOptions};

use crate::common::{Args, Parsed, failed, project_root, read, report, usage};

pub(crate) fn sprites_scene(args: Vec<String>) -> ExitCode {
    match run(args) {
        Ok(code) | Err(code) => code,
    }
}

fn run(args: Vec<String>) -> Parsed<ExitCode> {
    let flags = parse(args)?;
    let mut inputs = load(&flags)?;
    let objects = select(&flags, std::mem::take(&mut inputs.objects))?;
    let scale = flags.scale.or(inputs.scale).unwrap_or(16);
    let options = SheetOptions {
        name: flags.name.clone(),
        scale,
        projection: flags.projection.or(inputs.projection).unwrap_or_default(),
        views: flags
            .views
            .clone()
            .unwrap_or_else(|| vec![0.0, 90.0, 180.0, 270.0]),
        pivot: flags.pivot,
        shadow: flags.shadow,
        padding: flags.padding,
        debug_grid: flags.debug_grid,
        atlas: flags.atlas,
        max_rows: flags.max_rows,
        animations: animation_specs(flags.animations)?,
        tile_width: flags.tile_width.or_else(|| scale.checked_mul(2)),
    };

    let report = scene_export_sprites::export(&objects, &inputs.materials, &options, &flags.output)
        .map_err(|diagnostic| failed(&diagnostic))?;
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
    Ok(ExitCode::SUCCESS)
}

/// Everything given on the command line.
struct Flags {
    paths: Vec<String>,
    output: PathBuf,
    name: String,
    views: Option<Vec<f64>>,
    scale: Option<u32>,
    tile_width: Option<u32>,
    keep: Vec<String>,
    exclude: Vec<String>,
    pivot: Pivot,
    shadow: bool,
    padding: u32,
    debug_grid: bool,
    atlas: bool,
    max_rows: Option<u32>,
    projection: Option<Projection>,
    base: Option<PathBuf>,
    animations: Vec<AnimationDraft>,
}

#[allow(clippy::too_many_lines)]
fn parse(args: Vec<String>) -> Parsed<Flags> {
    let mut flags = Flags {
        paths: Vec::new(),
        output: PathBuf::from("out/sprites"),
        name: "sheet".to_string(),
        views: None,
        scale: None,
        tile_width: None,
        keep: Vec::new(),
        exclude: Vec::new(),
        pivot: Pivot::default(),
        shadow: true,
        padding: 0,
        debug_grid: false,
        atlas: false,
        max_rows: None,
        projection: None,
        base: None,
        animations: Vec::new(),
    };
    // Animation flags apply to the latest `--animation`; flags given before
    // the first one apply to the first.
    let mut current = AnimationDraft::default();
    let mut args = Args::new(args);
    while let Some(argument) = args.next() {
        match argument.as_str() {
            "--out" | "-o" => flags.output = PathBuf::from(args.value("--out")?),
            "--base" => flags.base = Some(PathBuf::from(args.value("--base")?)),
            "--name" => flags.name = args.value("--name")?,
            "--scale" => {
                flags.scale = Some(args.parse(
                    "--scale",
                    |n: &u32| (1..=scene_core::MAX_SCALE).contains(n),
                    |v| format!("invalid scale `{v}`; use 1..={}", scene_core::MAX_SCALE),
                )?);
            }
            "--tile-width" => {
                flags.tile_width = Some(args.parse(
                    "--tile-width",
                    |n: &u32| (1..=scene_export_sprites::MAX_TILE_WIDTH).contains(n),
                    |v| format!("invalid tile width `{v}`"),
                )?);
            }
            "--views" => {
                let count: u16 = args.parse(
                    "--views",
                    |n| matches!(n, 1 | 2 | 4 | 8 | 16),
                    |_| "`--views` must be 1, 2, 4, 8 or 16".to_string(),
                )?;
                flags.views = Some(if count == 2 {
                    vec![0.0, 180.0]
                } else {
                    (0..count)
                        .map(|step| f64::from(step) * 360.0 / f64::from(count))
                        .collect()
                });
            }
            "--angles" => {
                flags.views = Some(args.list(
                    "--angles",
                    |angle: &f64| angle.is_finite(),
                    |part| format!("invalid angle `{part}`"),
                )?);
            }
            "--object" => flags.keep.push(args.value("--object")?),
            "--exclude" => flags.exclude.push(args.value("--exclude")?),
            "--debug-grid" => flags.debug_grid = true,
            "--atlas" => flags.atlas = true,
            "--projection" => {
                let value = args.value("--projection")?;
                flags.projection = Some(
                    Projection::parse(&value)
                        .ok_or_else(|| usage("`--projection` must be iso, top, front or side"))?,
                );
            }
            "--max-rows" => {
                flags.max_rows = Some(args.parse(
                    "--max-rows",
                    |n: &u32| *n >= 1,
                    |v| format!("invalid row count `{v}`"),
                )?);
            }
            "--padding" => {
                flags.padding = args.parse(
                    "--padding",
                    |n: &u32| *n <= 256,
                    |_| "`--padding` must be 0..=256 pixels".to_string(),
                )?;
            }
            "--shadow" => {
                flags.shadow = match args.value("--shadow")?.as_str() {
                    "contact" => true,
                    "none" => false,
                    _ => return Err(usage("`--shadow` must be contact or none")),
                };
            }
            "--pivot" => {
                let value = args.value("--pivot")?;
                flags.pivot = Pivot::parse(&value)
                    .ok_or_else(|| usage("`--pivot` must be front, back, left, right or center"))?;
            }
            "--animation" => {
                let object = args.value("--animation")?;
                if current.object.is_some() {
                    flags.animations.push(std::mem::take(&mut current));
                }
                current.object = Some(object);
            }
            "--label" => {
                current.label = Some(args.value("--label")?);
                current.touched = true;
            }
            "--from" | "--to" => {
                let angle = args.parse(
                    &argument,
                    |n: &f64| n.is_finite(),
                    |v| format!("invalid angle `{v}`"),
                )?;
                if argument == "--from" {
                    current.from = angle;
                } else {
                    current.to = angle;
                }
                current.touched = true;
            }
            "--frames" => {
                current.frames = args.parse(
                    "--frames",
                    |n: &u32| (1..=scene_export_sprites::MAX_FRAMES).contains(n),
                    |v| {
                        format!(
                            "invalid frame count `{v}`; use 1..={}",
                            scene_export_sprites::MAX_FRAMES
                        )
                    },
                )?;
                current.touched = true;
            }
            "--duration" => {
                current.duration = args.parse(
                    "--duration",
                    |n: &u32| *n >= 1,
                    |v| format!("invalid duration `{v}`"),
                )?;
                current.touched = true;
            }
            "--durations" => {
                current.durations = args.list(
                    "--durations",
                    |n: &u32| *n >= 1,
                    |part| format!("invalid duration `{part}`"),
                )?;
                current.touched = true;
            }
            "--once" => {
                current.looped = false;
                current.touched = true;
            }
            "--rotation" => {
                current.rotation = args.parse(
                    "--rotation",
                    |n: &u16| matches!(n, 0 | 90 | 180 | 270),
                    |_| "`--rotation` must be 0, 90, 180 or 270".to_string(),
                )?;
                current.touched = true;
            }
            _ => flags.paths.push(argument),
        }
    }

    if flags.paths.is_empty() {
        return Err(usage("`sprites` needs a .scene or .model file"));
    }
    if current.object.is_some() {
        flags.animations.push(current);
    } else if current.touched {
        return Err(usage(
            "`--from/--to/--frames/--duration/--durations/--once/--rotation/--label` need `--animation`",
        ));
    }
    for draft in &flags.animations {
        if !draft.durations.is_empty() && draft.durations.len() != draft.frames as usize {
            return Err(usage(format!(
                "`--durations` lists {} values for {} frames",
                draft.durations.len(),
                draft.frames
            )));
        }
    }
    scene_export_sprites::validate_name(&flags.name).map_err(usage)?;
    Ok(flags)
}

/// Objects, materials and scene defaults collected from the input files.
struct Inputs {
    objects: Vec<SheetObject>,
    materials: BTreeMap<String, PathBuf>,
    /// Scale and projection of the first scene.
    scale: Option<u32>,
    projection: Option<Projection>,
}

fn load(flags: &Flags) -> Parsed<Inputs> {
    let mut inputs = Inputs {
        objects: Vec::new(),
        materials: BTreeMap::new(),
        scale: None,
        projection: None,
    };
    // Object and material name -> file that first defined it, for conflicts.
    let mut object_files: BTreeMap<String, String> = BTreeMap::new();
    let mut material_files: BTreeMap<String, String> = BTreeMap::new();
    for path in &flags.paths {
        let source = read(path).ok_or(ExitCode::from(2))?;
        let program = scene_syntax::parse(&source)
            .map_err(|diagnostic| report(path, &source, &diagnostic))?;
        let file = Path::new(path);
        let fallback_base = file.parent().unwrap_or_else(|| Path::new("."));
        let base = flags.base.as_deref().unwrap_or(fallback_base);
        let root = project_root(flags.base.as_deref());
        match file.extension().and_then(|value| value.to_str()) {
            Some("scene") => {
                let scene = scene_core::compile_scene(&program, base, &root)
                    .map_err(|diagnostic| report(path, &source, &diagnostic))?;
                inputs.scale.get_or_insert(scene.scale);
                inputs.projection.get_or_insert(match scene.render {
                    scene_syntax::ast::RenderKind::Iso => Projection::Iso,
                    scene_syntax::ast::RenderKind::Top => Projection::Top,
                    scene_syntax::ast::RenderKind::Front => Projection::Front,
                    scene_syntax::ast::RenderKind::Side => Projection::Side,
                });
                if flags.scale.is_none() && inputs.scale.is_some_and(|known| known != scene.scale) {
                    return Err(usage(
                        "scenes use different scales; pass --scale to pick one",
                    ));
                }
                merge_materials(
                    &mut inputs.materials,
                    &mut material_files,
                    &scene.materials,
                    path,
                )
                .map_err(usage)?;
                for instance in &scene.instances {
                    push_sheet_object(
                        &mut inputs.objects,
                        &mut object_files,
                        &instance.object,
                        path,
                    )
                    .map_err(usage)?;
                }
            }
            Some("model") => {
                let model = scene_core::compile_model(&program, base)
                    .map_err(|diagnostic| report(path, &source, &diagnostic))?;
                merge_materials(
                    &mut inputs.materials,
                    &mut material_files,
                    &model.materials,
                    path,
                )
                .map_err(usage)?;
                for object in model.objects.values() {
                    push_sheet_object(&mut inputs.objects, &mut object_files, object, path)
                        .map_err(usage)?;
                }
            }
            _ => {
                return Err(usage(format!(
                    "`sprites` expects .scene or .model files, got `{path}`"
                )));
            }
        }
    }
    Ok(inputs)
}

/// Apply `--object` and `--exclude`, and check that animated objects remain.
fn select(flags: &Flags, mut objects: Vec<SheetObject>) -> Parsed<Vec<SheetObject>> {
    for wanted in flags.keep.iter().chain(&flags.exclude) {
        if !objects.iter().any(|entry| entry.name == *wanted) {
            return Err(usage(format!("no object named `{wanted}`")));
        }
    }
    if !flags.keep.is_empty() {
        objects.retain(|entry| flags.keep.contains(&entry.name));
    }
    objects.retain(|entry| !flags.exclude.contains(&entry.name));
    if objects.is_empty() {
        return Err(usage("every object was filtered out"));
    }
    for draft in &flags.animations {
        let wanted = draft.object.as_deref().unwrap_or_default();
        if !objects.iter().any(|entry| entry.name == wanted) {
            return Err(usage(format!("no object named `{wanted}` to animate")));
        }
    }
    Ok(objects)
}

fn animation_specs(drafts: Vec<AnimationDraft>) -> Parsed<Vec<AnimationSpec>> {
    let mut specs: Vec<AnimationSpec> = Vec::new();
    for draft in drafts {
        let object = draft.object.unwrap_or_default();
        let label = draft.label.unwrap_or_else(|| object.clone());
        if specs.iter().any(|spec| spec.name == label) {
            return Err(usage(format!(
                "animation `{label}` is defined twice; name them with `--label`"
            )));
        }
        specs.push(AnimationSpec {
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
    Ok(specs)
}

/// Animation flags collected for one `--animation`.
struct AnimationDraft {
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
fn push_sheet_object(
    objects: &mut Vec<SheetObject>,
    files: &mut BTreeMap<String, String>,
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
            objects.push(SheetObject {
                name: object.name.clone(),
                object: object.clone(),
                source: Some(file.to_string()),
            });
            Ok(())
        }
    }
}

/// Merge `incoming` materials; the same name bound to two files is a conflict.
fn merge_materials(
    materials: &mut BTreeMap<String, PathBuf>,
    files: &mut BTreeMap<String, String>,
    incoming: &BTreeMap<String, PathBuf>,
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
