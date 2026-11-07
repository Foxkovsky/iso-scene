use std::fs;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use scene_core::compile_scene;
use scene_export_tiled::{WallOrder, export, export_with, export_with_walls};
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
    // A 1x1 object at (0, 0): footprint corner (1, 1), one 16-pixel tile
    // height per cell on both of Tiled's unprojected axes.
    assert!((objects[0]["x"].as_f64().expect("x") - 16.0).abs() < 0.01);
    assert!((objects[0]["y"].as_f64().expect("y") - 16.0).abs() < 0.01);

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
    assert_eq!(map["tilesets"].as_array().expect("tilesets").len(), 4);
    assert!(dir.join("wall_shadow.png").exists());
    assert!(dir.join("wall_shadow.tsj").exists());
    // The floor cell in the middle has walls on all four sides.
    let shadow_first_gid = map["tilesets"][3]["firstgid"].as_u64().expect("firstgid");
    assert_eq!(map["tilesets"][3]["source"], "wall_shadow.tsj");
    assert_eq!(map["layers"][1]["data"][4], shadow_first_gid + 15);
    // By default the near walls go above the objects and the rest below.
    assert_eq!(
        layer_names(&map),
        [
            "floor",
            "wall-shadow",
            "walls-back",
            "objects",
            "walls-front"
        ]
    );
    let walls_in = |name: &str| -> usize {
        map["layers"]
            .as_array()
            .expect("layers")
            .iter()
            .find(|layer| layer["name"] == name)
            .and_then(|layer| layer["data"].as_array())
            .expect("wall data")
            .iter()
            .filter(|value| value.as_u64().unwrap_or(0) > 0)
            .count()
    };
    // Of the eight walls around the single floor cell, the right one and the
    // bottom one have that floor right behind them.
    assert_eq!(walls_in("walls-back"), 6);
    assert_eq!(walls_in("walls-front"), 2);

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

fn layer_names(map: &serde_json::Value) -> Vec<String> {
    map["layers"]
        .as_array()
        .expect("layers")
        .iter()
        .map(|layer| layer["name"].as_str().expect("name").to_owned())
        .collect()
}

#[test]
fn wall_order_puts_the_wall_layer_below_above_or_around_the_objects() {
    let scene = compile(
        "version 1; object b (1, 1) { box at (0, 0, 0) size (1, 1, 1) material wood; } render iso; scale 16; place b at (1, 1, 0);",
    );
    let walls = scene_walls::parse(
        "version 1;\nlayout {\n\"###\"\n\"#.#\"\n\"###\"\n}\n",
        Path::new("."),
        Path::new("."),
    )
    .expect("walls");
    for (order, names) in [
        (
            WallOrder::Behind,
            &["floor", "wall-shadow", "walls", "objects"][..],
        ),
        (
            WallOrder::Front,
            &["floor", "wall-shadow", "objects", "walls"][..],
        ),
        (
            WallOrder::Split,
            &[
                "floor",
                "wall-shadow",
                "walls-back",
                "objects",
                "walls-front",
            ][..],
        ),
    ] {
        let dir = temp_dir("wall-order");
        let report = export_with(&scene, Some(&walls), false, order, &dir).expect("export");
        let map: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&report.map).expect("read map")).expect("map");
        assert_eq!(layer_names(&map), names, "{order:?}");
        // Layer ids are unique and `nextlayerid` is past the last one.
        let ids: Vec<u64> = map["layers"]
            .as_array()
            .expect("layers")
            .iter()
            .map(|layer| layer["id"].as_u64().expect("id"))
            .collect();
        let expected: Vec<u64> = (1..=names.len() as u64).collect();
        assert_eq!(ids, expected, "{order:?}");
        assert_eq!(map["nextlayerid"], names.len() + 1, "{order:?}");
        let _ = fs::remove_dir_all(dir);
    }
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
    assert_eq!(
        layer_names(&map),
        ["floor", "wall-shadow", "walls-back", "walls-front"]
    );
    assert_eq!(map["tilesets"].as_array().expect("tilesets").len(), 3);
    assert_eq!(layers[1]["id"], 2);
    assert_eq!(map["nextlayerid"], 5);
    assert!(
        layers[1]["data"]
            .as_array()
            .expect("wall data")
            .iter()
            .any(|value| value.as_u64().unwrap_or(0) > 0)
    );
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn an_object_sits_on_the_projected_footprint_corner() {
    let scene = compile(
        "version 1; object b (2, 1) { box at (0, 0, 0) size (2, 1, 1) material wood; } render iso; scale 16; place b at (2, 3, 0);",
    );
    let dir = temp_dir("footprint");
    let report = export(&scene, &dir).expect("export");
    let map: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&report.map).expect("read map")).expect("map");
    let object = &map["layers"][1]["objects"][0];
    // Footprint corner (4, 4) times the 16-pixel tile height.
    assert!((object["x"].as_f64().expect("x") - 64.0).abs() < 0.01);
    assert!((object["y"].as_f64().expect("y") - 64.0).abs() < 0.01);
    let _ = fs::remove_dir_all(dir);
}

/// Where Tiled draws an isometric object: its `IsometricRenderer` projects the
/// stored position, counting `tileheight` pixels per cell on both axes.
fn tiled_screen(object: &serde_json::Value, map: &serde_json::Value) -> [f64; 2] {
    let tile_w = map["tilewidth"].as_f64().expect("tilewidth");
    let tile_h = map["tileheight"].as_f64().expect("tileheight");
    let rows = map["height"].as_f64().expect("height");
    let x = object["x"].as_f64().expect("x") / tile_h;
    let y = object["y"].as_f64().expect("y") / tile_h;
    [
        (x - y) * tile_w / 2.0 + rows * tile_w / 2.0,
        (x + y) * tile_h / 2.0,
    ]
}

#[test]
fn tiled_draws_objects_on_their_footprint_corner_off_the_diagonal() {
    // Corner (5, 2) on a 6x4 map, and a raised object: the earlier exporter
    // wrote screen pixels, which only coincide with Tiled's coordinates when
    // the corner lies on the diagonal of a square map.
    let scene = compile(
        "version 1; object b (2, 1) { box at (0, 0, 0) size (2, 1, 1) material wood; } render iso; canvas (6, 4, 4); scale 16; place b at (3, 1, 0); place b at (0, 2, 1);",
    );
    let dir = temp_dir("off-diagonal");
    let report = export(&scene, &dir).expect("export");
    let map: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&report.map).expect("read map")).expect("map");
    let objects = map["layers"][1]["objects"].as_array().expect("objects");
    let origin_x = 4.0 * 32.0 / 2.0;
    // Screen pixel of the iso projection of world (x, y, z) on this map.
    let iso = |x: f64, y: f64, z: f64| [(x - y) * 16.0 + origin_x, (x + y) * 8.0 - z * 16.0];
    let mut expected = [iso(2.0, 3.0, 1.0), iso(5.0, 2.0, 0.0)];
    let mut actual: Vec<[f64; 2]> = objects.iter().map(|o| tiled_screen(o, &map)).collect();
    expected.sort_by(|a, b| a[0].total_cmp(&b[0]));
    actual.sort_by(|a, b| a[0].total_cmp(&b[0]));
    for (actual, expected) in actual.iter().zip(expected) {
        assert!(
            (actual[0] - expected[0]).abs() < 0.01,
            "{actual:?} != {expected:?}"
        );
        assert!(
            (actual[1] - expected[1]).abs() < 0.01,
            "{actual:?} != {expected:?}"
        );
    }
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn a_quarter_turn_does_not_shift_a_symmetric_sprite() {
    // A full cube looks the same at every quarter turn. The sprite anchor is
    // the pixel of the turned local origin, not of world (0, 0); confusing the
    // two shifted turned sprites by their footprint inside the atlas cell.
    let scene = compile(
        "version 1; object b (1, 1) { box at (0, 0, 0) size (1, 1, 1) material wood; } render iso; scale 16; place b at (0, 0, 0) rot 0; place b at (2, 0, 0) rot 90; place b at (0, 2, 0) rot 180; place b at (2, 2, 0) rot 270;",
    );
    let dir = temp_dir("quarter-turns");
    let report = export(&scene, &dir).expect("export");
    let ts: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(report.objects_tileset.as_ref().expect("objects tsj"))
            .expect("read tsj"),
    )
    .expect("parse tsj");
    let cell = usize::try_from(ts["tilewidth"].as_u64().expect("tilewidth")).expect("width");
    let decoder = png::Decoder::new(std::io::BufReader::new(
        fs::File::open(dir.join("objects.png")).expect("atlas"),
    ));
    let mut reader = decoder.read_info().expect("png header");
    let mut atlas = vec![0; reader.output_buffer_size()];
    let info = reader.next_frame(&mut atlas).expect("png data");
    let width = info.width as usize;
    assert_eq!(width, cell * 4);
    // Faces are shaded per turn, so compare where the cube is, not its colours.
    let column = |index: usize| -> Vec<bool> {
        atlas
            .chunks_exact(width * 4)
            .flat_map(|row| row[index * cell * 4..(index + 1) * cell * 4].chunks_exact(4))
            .map(|pixel| pixel[3] == 255)
            .collect()
    };
    for index in 1..4 {
        assert!(column(0) == column(index), "turn {index} moved the sprite");
    }
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn a_canvas_sets_the_map_rectangle() {
    let scene = compile(
        "version 1; object b (1, 1) { box at (0, 0, 0) size (1, 1, 1) material wood; } render iso; canvas (6, 5, 3); scale 16; place b at (0, 0, 0);",
    );
    let dir = temp_dir("canvas");
    let report = export(&scene, &dir).expect("export");
    assert_eq!(report.cells, [6, 5]);
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn objects_can_be_exported_as_a_tile_layer() {
    let scene = compile(
        "version 1; object b (1, 1) { box at (0, 0, 0) size (1, 1, 1) material wood; } render iso; scale 16; place b at (0, 0, 0);",
    );
    let dir = temp_dir("tilelayer");
    let report = export_with(&scene, None, true, WallOrder::default(), &dir).expect("export");
    let map: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&report.map).expect("read map")).expect("map");
    let layer = &map["layers"][1];
    assert_eq!(layer["name"], "objects");
    assert_eq!(layer["type"], "tilelayer");
    assert_eq!(layer["data"], serde_json::json!([2]));

    let ts: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(report.objects_tileset.as_ref().expect("objects tsj")).expect("read"),
    )
    .expect("tsj");
    let expected = (map["tilewidth"].as_i64().expect("map tile width")
        - ts["imagewidth"].as_i64().expect("image width"))
        / 2;
    assert_eq!(ts["tileoffset"]["x"], expected);
    let _ = fs::remove_dir_all(dir);
}
