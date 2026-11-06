use std::fs;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use scene_core::compile_scene;
use scene_export_tiled::{export, export_with_walls};
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
    assert!(report.preview.as_ref().expect("preview").exists());

    let map: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&report.map).expect("read map"))
            .expect("parse map");
    assert_eq!(map["width"], 1);
    assert_eq!(map["height"], 1);
    assert_eq!(map["tilewidth"], 32);
    assert_eq!(map["tileheight"], 16);
    assert_eq!(map["layers"][0]["data"], serde_json::json!([1]));
    assert_eq!(map["layers"][1]["type"], "objectgroup");
    let objects = map["layers"][1]["objects"].as_array().expect("objects");
    assert_eq!(objects.len(), 1);
    assert_eq!(objects[0]["gid"], 2);
    assert_eq!(objects[0]["x"], 16.0);
    assert_eq!(objects[0]["y"], 16.0);

    let ts: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(report.objects_tileset.as_ref().expect("objects tsj"))
            .expect("read tsj"),
    )
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
    let ts: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(report.objects_tileset.as_ref().expect("objects tsj"))
            .expect("read tsj"),
    )
    .expect("parse tsj");
    assert_eq!(ts["tilecount"], 2);
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn two_objects_in_one_cell_are_allowed_in_an_object_layer() {
    let scene = compile(
        "version 1; object b (1, 1) { box at (0, 0, 0) size (1, 1, 1) material wood; } render iso; scale 16; place b at (0, 0, 0); place b at (0, 0, 0) rot 90;",
    );
    let dir = temp_dir("collision");
    let report = export(&scene, &dir).expect("export");
    let map: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&report.map).expect("read map")).expect("map");
    assert_eq!(
        map["layers"][1]["objects"]
            .as_array()
            .expect("objects")
            .len(),
        2
    );
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn elevated_objects_use_a_z_property() {
    let scene = compile(
        "version 1; object b (1, 1) { box at (0, 0, 0) size (1, 1, 1) material wood; } render iso; scale 16; place b at (0, 0, 1);",
    );
    let dir = temp_dir("elevated");
    let report = export(&scene, &dir).expect("export");
    let map: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&report.map).expect("read map")).expect("map");
    let object = &map["layers"][1]["objects"][0];
    let properties = object["properties"].as_array().expect("properties");
    let z = properties
        .iter()
        .find(|property| property["name"] == "z")
        .expect("z property");
    assert_eq!(z["value"], 1.0);
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

#[test]
fn walls_produce_a_separate_tile_layer_and_wang_set() {
    let scene = compile(
        "version 1; object b (1, 1) { box at (0, 0, 0) size (1, 1, 1) material wood; } render iso; scale 16; place b at (0, 0, 0);",
    );
    let walls = scene_walls::parse(
        "version 1;\nheight 2;\nlayout {\n\"###\"\n\"#.#\"\n\"###\"\n}\n",
        Path::new("."),
        Path::new("."),
    )
    .expect("walls");
    let dir = temp_dir("walls");
    let report = export_with_walls(&scene, Some(&walls), &dir).expect("export walls");
    assert!(report.walls_tileset.is_some());

    let map: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&report.map).expect("read map")).expect("map");
    assert_eq!(map["layers"].as_array().expect("layers").len(), 3);
    assert_eq!(map["layers"][2]["name"], "walls");
    let walls_data = map["layers"][2]["data"].as_array().expect("wall data");
    assert!(
        walls_data
            .iter()
            .any(|value| value.as_u64().unwrap_or(0) > 0)
    );
    assert_eq!(map["tilesets"].as_array().expect("tilesets").len(), 3);

    let ts: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(report.walls_tileset.as_ref().expect("walls tsj")).expect("read tsj"),
    )
    .expect("tsj");
    assert_eq!(ts["tilecount"], 16);
    assert_eq!(
        ts["wangsets"][0]["wangtiles"]
            .as_array()
            .expect("wangtiles")
            .len(),
        16
    );
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn empty_scene_exports_floor_without_an_object_layer() {
    let scene = compile("version 1; render iso; scale 16;");
    let walls = scene_walls::parse(
        "version 1;\nheight 2;\nlayout {\n\"###\"\n\"#.#\"\n\"###\"\n}\n",
        Path::new("."),
        Path::new("."),
    )
    .expect("walls");
    let dir = temp_dir("empty");
    let report = export_with_walls(&scene, Some(&walls), &dir).expect("export empty");
    assert_eq!(report.cells, [3, 3]);
    assert!(report.objects_tileset.is_none());
    assert!(report.preview.is_none());
    assert!(!dir.join("objects.png").exists());
    assert!(!dir.join("objects.tsj").exists());

    let map: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&report.map).expect("read map")).expect("map");
    let layers = map["layers"].as_array().expect("layers");
    assert_eq!(layers.len(), 2);
    assert_eq!(layers[0]["name"], "floor");
    assert_eq!(layers[1]["name"], "walls");
    assert_eq!(map["tilesets"].as_array().expect("tilesets").len(), 2);
    assert_eq!(layers[1]["id"], 2);
    assert_eq!(map["nextlayerid"], 3);
    assert!(
        layers[1]["data"]
            .as_array()
            .expect("wall data")
            .iter()
            .any(|value| value.as_u64().unwrap_or(0) > 0)
    );
    let _ = fs::remove_dir_all(dir);
}
