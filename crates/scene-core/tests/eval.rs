use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use scene_core::{compile_model, compile_scene, compile_scene_with_fuel};
use scene_syntax::parse;

fn examples() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples")
}

fn parse_ok(source: &str) -> scene_syntax::ast::Program {
    parse(source).unwrap_or_else(|error| panic!("{}: {}", error.code, error.message))
}

fn compile_scene_ok(source: &str) -> scene_core::SceneGraph {
    let program = parse_ok(source);
    let here = Path::new(".");
    compile_scene(&program, here, here)
        .unwrap_or_else(|error| panic!("{}: {}", error.code, error.message))
}

fn compile_error(source: &str) -> String {
    let program = parse_ok(source);
    let here = Path::new(".");
    let error = compile_scene(&program, here, here).expect_err("expected an evaluation error");
    error.code.to_owned()
}

#[test]
fn chair_model_compiles() {
    let source = fs::read_to_string(examples().join("chair.model")).expect("read chair");
    let program = parse_ok(&source);
    let model = compile_model(&program, &examples()).expect("compile chair");
    assert_eq!(model.materials.len(), 2, "oak and steel");
    let chair = model.objects.get("chair").expect("chair object");
    assert_eq!(chair.width, 1.8);
    assert_eq!(chair.parts.len(), 6, "four legs, seat and backrest");
    assert!(model.bounds.size()[2] > 3.0);
}

#[test]
fn classroom_scene_compiles_with_includes_and_loops() {
    let root = examples();
    let source = fs::read_to_string(root.join("classroom.scene")).expect("read classroom");
    let program = parse_ok(&source);
    let scene = compile_scene(&program, &root, &root).expect("compile classroom");

    assert_eq!(scene.instances.len(), 40, "4 rows x 5 columns x 2 objects");
    assert_eq!(scene.instances[0].object.name, "desk");
    assert_eq!(scene.instances[1].object.name, "chair");
    assert_eq!(scene.render, scene_syntax::ast::RenderKind::Iso);
    assert_eq!(scene.scale, 32);
    assert_eq!(scene.canvas, Some([34.0, 20.0, 4.0]));
    assert_eq!(
        scene.materials.len(),
        2,
        "oak and steel from included models"
    );
    assert!(scene.bounds.size()[0] > 30.0);
    assert!(scene.bounds.size()[2] > 3.0);
}

#[test]
fn mirror_and_loops() {
    let scene = compile_scene_ok(
        r"
        version 1;
        object post (1, 1) {
            box at (0, 0, 0) size (0.4, 0.4, 1) material wood;
            mirror x;
        }
        render iso;
        scale 8;
        for i in 0..2 { place post at (i * 2, 0, 0) rot 0; }
        ",
    );
    assert_eq!(scene.instances.len(), 2);
    assert_eq!(scene.instances[0].object.parts.len(), 2);
}

#[test]
fn rotation_swaps_plan_bounds() {
    let scene = compile_scene_ok(
        r"
        version 1;
        object slab (2, 1) { box at (0, 0, 0) size (2, 1, 0.5) material wood; }
        render iso;
        scale 8;
        place slab at (10, 0, 0) rot 90;
        ",
    );
    let bounds = scene.instances[0].world_bounds();
    assert!((bounds.min[0] - 10.0).abs() < 1e-9);
    assert!((bounds.max[0] - 11.0).abs() < 1e-9);
    assert!((bounds.min[1] - 0.0).abs() < 1e-9);
    assert!((bounds.max[1] - 2.0).abs() < 1e-9);
}

#[test]
fn undefined_name_is_reported() {
    assert_eq!(
        compile_error("version 1; render iso; scale 8; let a = missing;"),
        "E0301"
    );
}

#[test]
fn duplicate_name_is_reported() {
    assert_eq!(
        compile_error("version 1; render iso; scale 8; let a = 1; let a = 2;"),
        "E0303"
    );
}

#[test]
fn immutable_assignment_is_reported() {
    assert_eq!(
        compile_error("version 1; render iso; scale 8; let a = 1; a = 2;"),
        "E0304"
    );
}

#[test]
fn type_mismatch_is_reported() {
    assert_eq!(
        compile_error("version 1; render iso; scale 8; let a = 1; if (a) { }"),
        "E0302"
    );
}

#[test]
fn unknown_material_is_reported() {
    let program =
        parse_ok("version 1; object a (1, 1) { box at (0, 0, 0) size (1, 1, 1) material gold; }");
    assert_eq!(
        compile_model(&program, Path::new("."))
            .expect_err("unknown material")
            .code,
        "E0305"
    );
}

#[test]
fn default_placement_puts_the_back_corner_on_the_coordinates() {
    let scene = compile_scene_ok(
        r"
        version 1;
        object slab (2, 1) { box at (0, 0, 0) size (2, 1, 0.5) material wood; }
        render iso;
        scale 8;
        place slab at (10, 20, 0);
        ",
    );
    let bounds = scene.instances[0].world_bounds();
    assert!((bounds.min[0] - 10.0).abs() < 1e-9);
    assert!((bounds.min[1] - 20.0).abs() < 1e-9);
    assert!((bounds.max[0] - 12.0).abs() < 1e-9);
    assert!((bounds.max[1] - 21.0).abs() < 1e-9);
}

#[test]
fn the_front_anchor_puts_the_near_corner_on_the_coordinates() {
    let scene = compile_scene_ok(
        r"
        version 1;
        object slab (2, 1) { box at (0, 0, 0) size (2, 1, 0.5) material wood; }
        render iso;
        scale 8;
        place slab at (10, 20, 0) anchor front;
        ",
    );
    let bounds = scene.instances[0].world_bounds();
    assert!((bounds.max[0] - 10.0).abs() < 1e-9);
    assert!((bounds.max[1] - 20.0).abs() < 1e-9);
    assert!((bounds.min[0] - 8.0).abs() < 1e-9);
    assert!((bounds.min[1] - 19.0).abs() < 1e-9);
}

#[test]
fn the_center_anchor_puts_the_footprint_centre_on_the_coordinates() {
    let scene = compile_scene_ok(
        r"
        version 1;
        object slab (2, 1) { box at (0, 0, 0) size (2, 1, 0.5) material wood; }
        render iso;
        scale 8;
        place slab at (10, 20, 0) anchor center;
        ",
    );
    let bounds = scene.instances[0].world_bounds();
    assert!((bounds.min[0].midpoint(bounds.max[0]) - 10.0).abs() < 1e-9);
    assert!((bounds.min[1].midpoint(bounds.max[1]) - 20.0).abs() < 1e-9);
}

#[test]
fn the_anchor_is_applied_to_the_rotated_footprint() {
    let scene = compile_scene_ok(
        r"
        version 1;
        object slab (2, 1) { box at (0, 0, 0) size (2, 1, 0.5) material wood; }
        render iso;
        scale 8;
        place slab at (10, 20, 0) rot 90 anchor front;
        ",
    );
    let bounds = scene.instances[0].world_bounds();
    assert!((bounds.max[0] - 10.0).abs() < 1e-9);
    assert!((bounds.max[1] - 20.0).abs() < 1e-9);
    assert!((bounds.min[0] - 9.0).abs() < 1e-9);
    assert!((bounds.min[1] - 18.0).abs() < 1e-9);
}

#[test]
fn the_cell_anchor_puts_the_sprite_on_the_placement_cell() {
    let scene = compile_scene_ok(
        r"
        version 1;
        object cube (2, 2) { box at (0, 0, 0) size (2, 2, 2) material light; }
        render iso;
        scale 32;
        place cube at (3, 5, 0) anchor cell;
        ",
    );
    let bounds = scene.instances[0].world_bounds();
    assert!((bounds.max[0] - 4.0).abs() < 1e-9);
    assert!((bounds.max[1] - 6.0).abs() < 1e-9);
    assert!((bounds.min[0] - 2.0).abs() < 1e-9);
    assert!((bounds.min[1] - 4.0).abs() < 1e-9);
}

#[test]
fn box_from_to_normalises_the_corners() {
    let scene = compile_scene_ok(
        r"
        version 1;
        object b (1, 1) { box from (2, 2, 2) to (1, 1, 1) material wood; }
        render iso;
        scale 8;
        place b at (0, 0, 0);
        ",
    );
    let bounds = scene.instances[0].world_bounds();
    assert!((bounds.min[0] - 1.0).abs() < 1e-9);
    assert!((bounds.max[0] - 2.0).abs() < 1e-9);
    assert!((bounds.min[1] - 1.0).abs() < 1e-9);
    assert!((bounds.max[1] - 2.0).abs() < 1e-9);
    assert!((bounds.min[2] - 1.0).abs() < 1e-9);
    assert!((bounds.max[2] - 2.0).abs() < 1e-9);
}

#[test]
fn use_inserts_another_object_with_an_offset() {
    let scene = compile_scene_ok(
        r"
        version 1;
        object leg (0.16, 0.16) { box at (0, 0, 0) size (w, d, 1.0) material metal; }
        object chair (1.0, 1.0) {
            use leg at (0.1, 0.1, 0);
            use leg at (0.74, 0.1, 0) rot 90;
        }
        render iso;
        scale 8;
        place chair at (0, 0, 0);
        ",
    );
    let object = &scene.instances[0].object;
    assert_eq!(object.parts.len(), 2);
    let bounds = scene.instances[0].world_bounds();
    assert!((bounds.min[0] - 0.1).abs() < 1e-9);
    assert!((bounds.max[0] - 0.9).abs() < 1e-9);
}

#[test]
fn use_is_rejected_in_a_scene() {
    let source = r"
        version 1;
        object leg (0.1, 0.1) { box at (0, 0, 0) size (w, d, 1) material metal; }
        render iso;
        scale 8;
        use leg at (0, 0, 0);
    ";
    assert_eq!(compile_error(source), "E0302");
}

#[test]
fn merge_marks_its_parts_as_one_group() {
    let scene = compile_scene_ok(
        r"
        version 1;
        object bench (2, 1) {
            merge {
                box from (0, 0, 0) to (1, 1, 0.5) material wood;
                box from (1, 0, 0) to (2, 1, 0.5) material wood;
            }
            box from (0, 0, 0.5) to (2, 1, 0.6) material wood;
        }
        render iso;
        scale 8;
        place bench at (0, 0, 0);
        ",
    );
    let parts = &scene.instances[0].object.parts;
    assert_eq!(parts.len(), 3);
    assert!(parts[0].group.is_some());
    assert_eq!(parts[0].group, parts[1].group);
    assert!(parts[2].group.is_none());
}

#[test]
fn invalid_rotation_is_reported() {
    assert_eq!(
        compile_error(
            "version 1; object a (1, 1) { box at (0, 0, 0) size (1, 1, 1) material wood; } render iso; scale 8; place a at (0, 0, 0) rot 45;"
        ),
        "E0306"
    );
}

#[test]
fn zero_size_is_reported() {
    let program =
        parse_ok("version 1; object a (1, 1) { box at (0, 0, 0) size (0, 1, 1) material wood; }");
    assert_eq!(
        compile_model(&program, Path::new("."))
            .expect_err("zero size")
            .code,
        "E0307"
    );
}

#[test]
fn division_by_zero_is_reported() {
    assert_eq!(
        compile_error("version 1; render iso; scale 8; let a = 1 / 0;"),
        "E0308"
    );
}

#[test]
fn non_integer_loop_bound_is_reported() {
    assert_eq!(
        compile_error("version 1; render iso; scale 8; for i in 0..1.5 { }"),
        "E0309"
    );
}

#[test]
fn non_constant_canvas_is_reported() {
    assert_eq!(
        compile_error("version 1; render iso; scale 8; let x = 1; canvas (x, 1, 1);"),
        "E0310"
    );
}

#[test]
fn render_and_scale_are_required() {
    assert_eq!(compile_error("version 1;"), "E0311");
    assert_eq!(compile_error("version 1; render iso;"), "E0311");
}

#[test]
fn fuel_exhaustion_is_reported() {
    let program = parse_ok("version 1; render iso; scale 8; for i in 0..100 { }");
    let here = Path::new(".");
    let error =
        compile_scene_with_fuel(&program, here, here, 10).expect_err("fuel must be exhausted");
    assert_eq!(error.code, "E0401");
}

#[test]
fn include_resolves_relative_model() {
    let temp = TempDir::new();
    fs::write(
        temp.path().join("chair.model"),
        "version 1; object chair (1, 1) { box at (0, 0, 0) size (1, 1, 1) material wood; }",
    )
    .expect("write model");
    let scene_path = temp.path().join("room.scene");
    fs::write(
        &scene_path,
        "version 1; include \"chair.model\"; render iso; scale 8; place chair at (0, 0, 0);",
    )
    .expect("write scene");
    let source = fs::read_to_string(&scene_path).expect("read scene");
    let program = parse_ok(&source);
    let scene = compile_scene(&program, temp.path(), temp.path()).expect("compile scene");
    assert_eq!(scene.instances.len(), 1);
    assert_eq!(scene.instances[0].object.name, "chair");
}

#[test]
fn include_cannot_escape_the_root() {
    let temp = TempDir::new();
    let root = temp.path().join("root");
    fs::create_dir_all(&root).expect("create root");
    fs::write(
        temp.path().join("outside.model"),
        "version 1; object outside (1, 1) { box at (0, 0, 0) size (1, 1, 1) material wood; }",
    )
    .expect("write outside model");
    let source = "version 1; include \"../outside.model\"; render iso; scale 8;";
    let program = parse_ok(source);
    let error = compile_scene(&program, &root, &root).expect_err("path must escape");
    assert_eq!(error.code, "E0502");
}

#[test]
fn include_must_be_a_model() {
    let temp = TempDir::new();
    fs::write(temp.path().join("chair.scene"), "version 1;").expect("write scene");
    let source = "version 1; include \"chair.scene\"; render iso; scale 8;";
    let program = parse_ok(source);
    let error = compile_scene(&program, temp.path(), temp.path()).expect_err("wrong extension");
    assert_eq!(error.code, "E0503");
}

struct TempDir(PathBuf);

static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);

impl TempDir {
    fn new() -> Self {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |duration| duration.as_nanos());
        let sequence = NEXT_TEMP.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "iso-scene-test-{}-{nanos}-{sequence}",
            std::process::id()
        ));
        fs::create_dir_all(&path).expect("create temp dir");
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
