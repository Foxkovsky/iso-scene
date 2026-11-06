use std::fs;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use scene_core::compile_scene;
use scene_export_tiled::export;
use scene_syntax::parse;

fn compile(source: &str) -> scene_core::SceneGraph {
    let program = parse(source).unwrap_or_else(|error| panic!("{}: {}", error.code, error.message));
    let here = Path::new(".");
    compile_scene(&program, here, here)
        .unwrap_or_else(|error| panic!("{}: {}", error.code, error.message))
}

fn temp_dir(name: &str) -> std::path::PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_nanos());
    let path = std::env::temp_dir().join(format!(
        "iso-scene-tiled-{name}-{}-{nanos}",
        std::process::id()
    ));
    fs::create_dir_all(&path).expect("create temp dir");
    path
}

#[test]
fn simple_box_exports_to_a_tiled_map() {
    let scene = compile(
        "version 1; object b (1, 1) { box at (0, 0, 0) size (1, 1, 1) material wood; } render iso; scale 16; place b at (0, 0, 0);",
    );
    let dir = temp_dir("simple");
    let report = export(&scene, &dir).expect("export");
    assert_eq!(report.cells, [1, 1]);
    assert!(report.map.exists());
    assert!(report.preview.exists());

    let map: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&report.map).expect("read map"))
            .expect("parse map");
    assert_eq!(map["width"], 1);
    assert_eq!(map["height"], 1);
    assert_eq!(map["tilewidth"], 32);
    assert_eq!(map["tileheight"], 16);
    assert_eq!(map["layers"][0]["data"], serde_json::json!([1]));
    assert_eq!(map["layers"][1]["data"], serde_json::json!([2]));

    let ts: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&report.objects_tileset).expect("read tsj"))
            .expect("parse tsj");
    assert_eq!(ts["tilecount"], 1);
    assert!(ts["tileoffset"]["x"].as_i64().is_some());
    assert!(ts["tileoffset"]["y"].as_i64().is_some());

    let _ = fs::remove_dir_all(dir);
}

#[test]
fn two_rotations_produce_two_object_tiles() {
    let scene = compile(
        "version 1; object b (1, 1) { box at (0, 0, 0) size (1, 1, 1) material wood; } render iso; scale 16; place b at (0, 0, 0); place b at (2, 0, 0) rot 90;",
    );
    let dir = temp_dir("rotations");
    let report = export(&scene, &dir).expect("export");
    let ts: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&report.objects_tileset).expect("read tsj"))
            .expect("parse tsj");
    assert_eq!(ts["tilecount"], 2);
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn two_objects_in_one_cell_are_rejected() {
    let scene = compile(
        "version 1; object b (1, 1) { box at (0, 0, 0) size (1, 1, 1) material wood; } render iso; scale 16; place b at (0, 0, 0); place b at (0, 0, 0) rot 90;",
    );
    let dir = temp_dir("collision");
    let error = export(&scene, &dir).expect_err("collision");
    assert_eq!(error.code, "E0602");
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn elevated_objects_are_rejected() {
    let scene = compile(
        "version 1; object b (1, 1) { box at (0, 0, 0) size (1, 1, 1) material wood; } render iso; scale 16; place b at (0, 0, 1);",
    );
    let dir = temp_dir("elevated");
    let error = export(&scene, &dir).expect_err("elevated");
    assert_eq!(error.code, "E0602");
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn orthographic_scene_is_rejected() {
    let scene = compile(
        "version 1; object b (1, 1) { box at (0, 0, 0) size (1, 1, 1) material wood; } render top; scale 16; place b at (0, 0, 0);",
    );
    let dir = temp_dir("top");
    let error = export(&scene, &dir).expect_err("top");
    assert_eq!(error.code, "E0604");
    let _ = fs::remove_dir_all(dir);
}
