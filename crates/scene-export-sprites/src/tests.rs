//! Unit tests of the sprite sheet exporter.

use super::*;
use crate::layout::{Cell, footprint_pivot, split_view};
use crate::manifest::degrees_json;
use scene_backend_iso::{Sprite, render_object_sprite_with};
use scene_render::{Image, composite_over};
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use std::sync::Arc;

/// A temporary directory unique to this test process, so parallel runs of
/// the suite do not share files.
fn scratch(name: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!("{name}-{}", std::process::id()))
}

fn box_object(name: &str, width: f64, depth: f64) -> Arc<CompiledObject> {
    let program = scene_syntax::parse(&format!(
        "version 1;\nobject {name} ({width}, {depth}) {{\n    box at (0, 0, 0) size ({width}, {depth}, 1) material wood;\n}}\n"
    ))
    .expect("test model parses");
    let model = scene_core::compile_model(&program, Path::new(".")).expect("test model compiles");
    model.objects[name].clone()
}

#[test]
fn pack_two_objects_with_four_views() {
    let objects = vec![
        SheetObject {
            name: "chair".into(),
            object: box_object("chair", 1.0, 1.0),
            source: None,
        },
        SheetObject {
            name: "desk".into(),
            object: box_object("desk", 2.0, 1.0),
            source: None,
        },
    ];
    let options = SheetOptions {
        name: "sheet".into(),
        scale: 8,
        projection: Projection::Iso,
        views: vec![0.0, 90.0, 180.0, 270.0],
        pivot: Pivot::Front,
        shadow: true,
        padding: 0,
        debug_grid: false,
        atlas: false,
        max_rows: None,
        animations: Vec::new(),
        tile_width: Some(16),
    };
    let dir = scratch("iso-scene-sprites-test");
    let report = export(&objects, &BTreeMap::new(), &options, &dir).expect("sheet exports");
    assert_eq!(report.rows, 2);
    assert_eq!(report.columns, 4);
    assert_eq!(report.sprites, 8);
    assert_eq!(report.frames, 0);

    let manifest: Value =
        serde_json::from_str(&fs::read_to_string(&report.manifest).unwrap()).unwrap();
    assert_eq!(manifest["sprites"].as_array().unwrap().len(), 8);
    assert_eq!(manifest["cell"]["width"], report.cells[0]);
    assert_eq!(manifest["anchor"]["x"], f64::from(report.cells[0]) / 2.0);
    // A static view sits on the floor: the anchor is near the cell bottom.
    let anchor_y = manifest["anchor"]["y"].as_f64().unwrap();
    assert!(
        anchor_y > f64::from(report.cells[1]) - 20.0,
        "anchor {anchor_y} should be near the cell bottom {}",
        report.cells[1]
    );

    let tileset: Value =
        serde_json::from_str(&fs::read_to_string(&report.tileset).unwrap()).unwrap();
    assert_eq!(tileset["tilewidth"], report.cells[0]);
    assert_eq!(tileset["columns"], 4);
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn animation_gets_its_own_row_and_tiled_animation() {
    let objects = vec![SheetObject {
        name: "door".into(),
        object: box_object("door", 1.0, 1.0),
        source: None,
    }];
    let options = SheetOptions {
        name: "door".into(),
        scale: 8,
        projection: Projection::Iso,
        views: vec![0.0, 180.0],
        pivot: Pivot::Front,
        shadow: true,
        padding: 0,
        debug_grid: false,
        atlas: false,
        max_rows: None,
        animations: vec![AnimationSpec {
            name: "door".into(),
            object: "door".into(),
            from: 0.0,
            to: 90.0,
            frames: 5,
            duration: 100,
            durations: Vec::new(),
            looped: true,
            rotation: 0,
        }],
        tile_width: None,
    };
    let dir = scratch("iso-scene-sprites-anim-test");
    let report = export(&objects, &BTreeMap::new(), &options, &dir).expect("sheet exports");
    assert_eq!(report.rows, 2);
    assert_eq!(report.columns, 5);
    assert_eq!(report.frames, 5);

    let manifest: Value =
        serde_json::from_str(&fs::read_to_string(&report.manifest).unwrap()).unwrap();
    assert_eq!(
        manifest["animations"][0]["frames"]
            .as_array()
            .unwrap()
            .len(),
        5
    );
    assert_eq!(manifest["animations"][0]["frames"][4]["angle"], 90.0);

    let tileset: Value =
        serde_json::from_str(&fs::read_to_string(&report.tileset).unwrap()).unwrap();
    let animated = tileset["tiles"]
        .as_array()
        .unwrap()
        .iter()
        .find(|tile| tile.get("animation").is_some())
        .expect("animated tile");
    assert_eq!(animated["animation"].as_array().unwrap().len(), 5);
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn a_single_view_stays_on_one_row() {
    let objects = vec![SheetObject {
        name: "chair".into(),
        object: box_object("chair", 1.0, 1.0),
        source: None,
    }];
    let options = SheetOptions {
        name: "one".into(),
        scale: 8,
        projection: Projection::Iso,
        views: vec![0.0],
        pivot: Pivot::Front,
        shadow: true,
        padding: 0,
        debug_grid: false,
        atlas: false,
        max_rows: None,
        animations: Vec::new(),
        tile_width: None,
    };
    let dir = scratch("iso-scene-sprites-one-test");
    let report = export(&objects, &BTreeMap::new(), &options, &dir).expect("sheet exports");
    assert_eq!(report.rows, 1);
    assert_eq!(report.columns, 1);
    let _ = fs::remove_dir_all(dir);
}

fn chair_options(name: &str) -> SheetOptions {
    SheetOptions {
        name: name.into(),
        scale: 8,
        projection: Projection::Iso,
        views: vec![0.0, 90.0, 180.0, 270.0],
        pivot: Pivot::Front,
        shadow: true,
        padding: 0,
        debug_grid: false,
        atlas: false,
        max_rows: None,
        animations: Vec::new(),
        tile_width: None,
    }
}

#[test]
fn soft_shadows_stay_translucent() {
    let objects = vec![SheetObject {
        name: "chair".into(),
        object: box_object("chair", 2.0, 1.0),
        source: None,
    }];
    let dir = scratch("iso-scene-sprites-alpha-test");
    let report =
        export(&objects, &BTreeMap::new(), &chair_options("alpha"), &dir).expect("sheet exports");
    let decoder = png::Decoder::new(std::io::BufReader::new(
        fs::File::open(&report.image).unwrap(),
    ));
    let mut reader = decoder.read_info().unwrap();
    let mut buffer = vec![0; reader.output_buffer_size()];
    reader.next_frame(&mut buffer).unwrap();
    assert!(
        buffer
            .chunks_exact(4)
            .any(|pixel| pixel[3] > 0 && pixel[3] < 255),
        "the baked contact shadow must keep partial alpha"
    );
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn half_pixel_references_do_not_bleed_into_the_next_cell() {
    let sprite = |reference_x: f64| Cell {
        row: 0,
        column: 0,
        name: "x".into(),
        footprint: [1.0, 1.0],
        source: None,
        rotation: 0,
        angle: 0.0,
        animation: None,
        duration: None,
        sprite: Sprite {
            image: Image {
                width: 4,
                height: 2,
                rgba: [255, 0, 0, 255].repeat(8),
            },
            anchor: [reference_x, 2.0],
        },
        reference: [reference_x, 2.0],
    };
    // A cell of width 4 whose anchor is 2.0, with a sprite referenced at
    // 1.5: the sprite lands at x = 0.5, which rounds to 1 and would spill
    // one column into the neighbour without a clip.
    let mut sheet = Image {
        width: 8,
        height: 2,
        rgba: vec![0; 8 * 2 * 4],
    };
    let cell = sprite(1.5);
    let x = 2.0 - cell.reference[0];
    composite_over(
        &mut sheet,
        &cell.sprite.image,
        x.round() as i64,
        0,
        [0, 0, 4, 2],
    );
    assert_eq!(sheet.pixel(4, 0)[3], 0, "column 4 belongs to the next cell");
    assert_eq!(sheet.pixel(3, 0)[3], 255);
}

#[test]
fn frame_and_cell_limits_are_checked_before_rendering() {
    let objects = vec![SheetObject {
        name: "door".into(),
        object: box_object("door", 1.0, 1.0),
        source: None,
    }];
    let mut options = chair_options("limits");
    options.animations = vec![AnimationSpec {
        name: "door".into(),
        object: "door".into(),
        from: 0.0,
        to: 90.0,
        frames: MAX_FRAMES + 1,
        duration: 100,
        durations: Vec::new(),
        looped: true,
        rotation: 0,
    }];
    let dir = scratch("iso-scene-sprites-limit-test");
    let error = export(&objects, &BTreeMap::new(), &options, &dir).unwrap_err();
    assert_eq!(error.code, "E0602");
    assert!(error.message.contains("frames"), "{}", error.message);

    let many: Vec<SheetObject> = (0..=MAX_CELLS / 4)
        .map(|index| SheetObject {
            name: format!("o{index}"),
            object: objects[0].object.clone(),
            source: None,
        })
        .collect();
    let error = export(&many, &BTreeMap::new(), &chair_options("limits"), &dir).unwrap_err();
    assert_eq!(error.code, "E0602");
    assert!(error.message.contains("cells"), "{}", error.message);
    assert!(!dir.exists(), "nothing is written when a limit fails");
}

#[test]
fn sheet_names_are_bare_stems() {
    for bad in [
        "",
        ".",
        "..",
        "../evil",
        "a/b",
        "a\\b",
        "sheet.png",
        "x.JSON",
    ] {
        assert!(validate_name(bad).is_err(), "`{bad}` should be rejected");
    }
    for good in ["sheet", "furniture-4", "door_anim", "v1.2"] {
        assert!(validate_name(good).is_ok(), "`{good}` should be accepted");
    }
    let objects = vec![SheetObject {
        name: "chair".into(),
        object: box_object("chair", 1.0, 1.0),
        source: None,
    }];
    let dir = scratch("iso-scene-sprites-name-test");
    let error = export(&objects, &BTreeMap::new(), &chair_options("../evil"), &dir).unwrap_err();
    assert_eq!(error.code, "E0606");
}

#[test]
fn an_overhang_does_not_lift_the_static_anchor() {
    let compile = |source: &str| {
        let program = scene_syntax::parse(source).expect("test model parses");
        scene_core::compile_model(&program, Path::new("."))
            .expect("test model compiles")
            .objects["table"]
            .clone()
    };
    let plain = compile(
        "version 1;\nobject table (1, 1) {\n    box at (0, 0, 0) size (1, 1, 1) material wood;\n}\n",
    );
    let overhang = compile(
        "version 1;\nobject table (1, 1) {\n    box at (0, 0, 0) size (1, 1, 1) material wood;\n    box at (-0.5, -0.5, 1) size (2, 2, 0.25) material wood;\n}\n",
    );
    for rotation in [0, 90, 180, 270] {
        let reference = |object: &CompiledObject| {
            let sprite = scene_backend_iso::render_object_sprite_at(
                object,
                rotation,
                0.0,
                8,
                &BTreeMap::new(),
            )
            .unwrap();
            let corner = footprint_pivot(
                object,
                rotation,
                0.0,
                8,
                &sprite,
                Pivot::Front,
                Projection::Iso,
            );
            // The anchor relative to the object's origin pixel.
            [corner[0] - sprite.anchor[0], corner[1] - sprite.anchor[1]]
        };
        assert_eq!(reference(&plain), reference(&overhang), "rot {rotation}");
    }
}

#[test]
fn pivots_pick_footprint_corners_and_centre() {
    let object = box_object("desk", 2.0, 1.0);
    let sprite =
        scene_backend_iso::render_object_sprite_at(&object, 0, 0.0, 8, &BTreeMap::new()).unwrap();
    let at = |pivot| {
        let point = footprint_pivot(&object, 0, 0.0, 8, &sprite, pivot, Projection::Iso);
        [point[0] - sprite.anchor[0], point[1] - sprite.anchor[1]]
    };
    // Screen offsets from the origin corner at scale 8: x = (x - y) * 8,
    // y = (x + y) / 2 * 8.
    assert_eq!(at(Pivot::Back), [0.0, 0.0]);
    assert_eq!(at(Pivot::Front), [8.0, 12.0]);
    assert_eq!(at(Pivot::Right), [16.0, 8.0]);
    assert_eq!(at(Pivot::Left), [-8.0, 4.0]);
    assert_eq!(at(Pivot::Center), [4.0, 6.0]);
    assert_eq!(Pivot::parse("center"), Some(Pivot::Center));
    assert_eq!(Pivot::parse("middle"), None);
}

#[test]
fn view_angles_split_into_quarter_turns_and_a_rest() {
    assert_eq!(split_view(0.0), (0, 0.0));
    assert_eq!(split_view(90.0), (90, 0.0));
    assert_eq!(split_view(135.0), (90, 45.0));
    assert_eq!(split_view(-90.0), (270, 0.0));
    assert_eq!(split_view(360.0), (0, 0.0));
    assert_eq!(split_view(89.999_999_999_9), (90, 0.0));
    assert_eq!(degrees_json(90.0), json!(90));
    assert_eq!(degrees_json(22.5), json!(22.5));
}

#[test]
fn eight_directions_report_their_angles() {
    let objects = vec![SheetObject {
        name: "desk".into(),
        object: box_object("desk", 2.0, 1.0),
        source: None,
    }];
    let mut options = chair_options("eight");
    options.views = (0..8).map(|step| f64::from(step) * 45.0).collect();
    let dir = scratch("iso-scene-sprites-eight-test");
    let report = export(&objects, &BTreeMap::new(), &options, &dir).expect("sheet exports");
    assert_eq!(report.columns, 8);
    let manifest: Value =
        serde_json::from_str(&fs::read_to_string(&report.manifest).unwrap()).unwrap();
    let rotations: Vec<Value> = manifest["sprites"]
        .as_array()
        .unwrap()
        .iter()
        .map(|sprite| sprite["rotation"].clone())
        .collect();
    assert_eq!(rotations[1], json!(45));
    assert_eq!(rotations[2], json!(90));
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn shadow_can_be_turned_off() {
    let objects = vec![SheetObject {
        name: "chair".into(),
        object: box_object("chair", 2.0, 1.0),
        source: None,
    }];
    let mut options = chair_options("bare");
    options.shadow = false;
    let dir = scratch("iso-scene-sprites-bare-test");
    let report = export(&objects, &BTreeMap::new(), &options, &dir).expect("sheet exports");
    let decoder = png::Decoder::new(std::io::BufReader::new(
        fs::File::open(&report.image).unwrap(),
    ));
    let mut reader = decoder.read_info().unwrap();
    let mut buffer = vec![0; reader.output_buffer_size()];
    reader.next_frame(&mut buffer).unwrap();
    // Without the shadow only the opaque object (and no soft halo) remains.
    let soft = buffer
        .chunks_exact(4)
        .filter(|pixel| pixel[3] > 0 && pixel[3] < 255)
        .count();
    assert_eq!(soft, 0);
    let _ = fs::remove_dir_all(dir);
}

fn door_animation(name: &str, frames: u32) -> AnimationSpec {
    AnimationSpec {
        name: name.into(),
        object: "door".into(),
        from: 0.0,
        to: 90.0,
        frames,
        duration: 100,
        durations: Vec::new(),
        looped: true,
        rotation: 0,
    }
}

#[test]
fn several_animations_get_one_row_each_with_timing_and_loop() {
    let objects = vec![SheetObject {
        name: "door".into(),
        object: box_object("door", 1.0, 1.0),
        source: None,
    }];
    let mut options = chair_options("doors");
    let mut close = door_animation("close", 3);
    close.from = 90.0;
    close.to = 0.0;
    close.durations = vec![50, 60, 400];
    close.looped = false;
    options.animations = vec![door_animation("open", 5), close];
    let dir = scratch("iso-scene-sprites-many-anim-test");
    let report = export(&objects, &BTreeMap::new(), &options, &dir).expect("sheet exports");
    assert_eq!(report.rows, 3);
    assert_eq!(report.columns, 5);
    assert_eq!(report.frames, 8);

    let manifest: Value =
        serde_json::from_str(&fs::read_to_string(&report.manifest).unwrap()).unwrap();
    let close = &manifest["animations"][1];
    assert_eq!(close["name"], "close");
    assert_eq!(close["object"], "door");
    assert_eq!(close["loop"], false);
    assert_eq!(close["frames"][2]["duration"], 400);
    assert_eq!(close["frames"][0]["y"], 2 * report.cells[1]);

    let tileset: Value =
        serde_json::from_str(&fs::read_to_string(&report.tileset).unwrap()).unwrap();
    let animated: Vec<&Value> = tileset["tiles"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|tile| tile.get("animation").is_some())
        .collect();
    assert_eq!(animated.len(), 2);
    assert_eq!(animated[1]["animation"][2]["duration"], 400);
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn invalid_animations_are_rejected() {
    let objects = vec![SheetObject {
        name: "door".into(),
        object: box_object("door", 1.0, 1.0),
        source: None,
    }];
    let dir = scratch("iso-scene-sprites-bad-anim-test");
    let mut options = chair_options("bad");
    options.animations = vec![door_animation("a", 3), door_animation("a", 3)];
    let error = export(&objects, &BTreeMap::new(), &options, &dir).unwrap_err();
    assert_eq!(error.code, "E0606");
    assert!(error.message.contains("used twice"), "{}", error.message);
    let mut timed = door_animation("b", 3);
    timed.durations = vec![10, 20];
    options.animations = vec![timed];
    let error = export(&objects, &BTreeMap::new(), &options, &dir).unwrap_err();
    assert!(error.message.contains("durations"), "{}", error.message);
    assert!(!dir.exists());
}

#[test]
fn padding_separates_cells_and_matches_the_tileset() {
    let objects = vec![
        SheetObject {
            name: "a".into(),
            object: box_object("a", 1.0, 1.0),
            source: None,
        },
        SheetObject {
            name: "b".into(),
            object: box_object("b", 1.0, 1.0),
            source: None,
        },
    ];
    let mut options = chair_options("padded");
    options.views = vec![0.0, 90.0];
    options.padding = 3;
    let dir = scratch("iso-scene-sprites-padding-test");
    let report = export(&objects, &BTreeMap::new(), &options, &dir).expect("sheet exports");
    let [w, h] = report.cells;
    let manifest: Value =
        serde_json::from_str(&fs::read_to_string(&report.manifest).unwrap()).unwrap();
    assert_eq!(manifest["sprites"][0]["x"], 3);
    assert_eq!(manifest["sprites"][1]["x"], 3 + w + 3);
    assert_eq!(manifest["sprites"][2]["y"], 3 + h + 3);
    let tileset: Value =
        serde_json::from_str(&fs::read_to_string(&report.tileset).unwrap()).unwrap();
    assert_eq!(tileset["margin"], 3);
    assert_eq!(tileset["spacing"], 3);
    assert_eq!(tileset["imagewidth"], 2 * w + 9);
    let decoder = png::Decoder::new(std::io::BufReader::new(
        fs::File::open(&report.image).unwrap(),
    ));
    let reader = decoder.read_info().unwrap();
    assert_eq!(reader.info().width, 2 * w + 9);
    assert_eq!(reader.info().height, 2 * h + 9);
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn manifest_describes_format_scale_and_footprints() {
    let objects = vec![SheetObject {
        name: "desk".into(),
        object: box_object("desk", 2.0, 1.0),
        source: Some("desk.model".into()),
    }];
    let mut options = chair_options("meta");
    options.tile_width = Some(16);
    let dir = scratch("iso-scene-sprites-meta-test");
    let report = export(&objects, &BTreeMap::new(), &options, &dir).expect("sheet exports");
    let manifest: Value =
        serde_json::from_str(&fs::read_to_string(&report.manifest).unwrap()).unwrap();
    assert_eq!(manifest["format"], MANIFEST_FORMAT);
    assert_eq!(manifest["version"], MANIFEST_VERSION);
    assert_eq!(manifest["scale"], 8);
    assert_eq!(manifest["tileWidth"], 16);
    assert_eq!(manifest["views"], json!([0, 90, 180, 270]));
    assert_eq!(manifest["imageWidth"], 4 * report.cells[0]);
    assert_eq!(manifest["imageHeight"], report.cells[1]);
    let sprite = &manifest["sprites"][0];
    assert_eq!(sprite["footprint"], json!({"width": 2.0, "depth": 1.0}));
    assert_eq!(sprite["source"], "desk.model");
    let _ = fs::remove_dir_all(dir);
}

/// Keys of `value` must be described by `schema` (directly or through
/// `allOf`), and every required key must be present.
fn check_against(schema: &Value, defs: &Value, value: &Value, path: &str) {
    let mut properties = serde_json::Map::new();
    let mut required = Vec::new();
    let mut parts = vec![schema.clone()];
    if let Some(all) = schema["allOf"].as_array() {
        for reference in all {
            let name = reference["$ref"]
                .as_str()
                .unwrap()
                .trim_start_matches("#/$defs/");
            parts.push(defs[name].clone());
        }
    }
    for part in &parts {
        if let Some(map) = part["properties"].as_object() {
            properties.extend(map.clone());
        }
        if let Some(list) = part["required"].as_array() {
            required.extend(list.iter().map(|key| key.as_str().unwrap().to_string()));
        }
    }
    let object = value.as_object().unwrap();
    for key in object.keys() {
        assert!(
            properties.contains_key(key),
            "{path}.{key} is not in the schema"
        );
    }
    for key in required {
        assert!(
            object.contains_key(&key),
            "{path}.{key} is required by the schema"
        );
    }
}

#[test]
fn manifest_matches_the_published_schema() {
    let schema: Value =
        serde_json::from_str(include_str!("../../../docs/sprite-sheet.schema.json")).unwrap();
    let defs = &schema["$defs"];
    let objects = vec![SheetObject {
        name: "door".into(),
        object: box_object("door", 1.0, 1.0),
        source: Some("door.model".into()),
    }];
    let mut options = chair_options("schema");
    options.animations = vec![door_animation("open", 2)];
    let dir = scratch("iso-scene-sprites-schema-test");
    let report = export(&objects, &BTreeMap::new(), &options, &dir).expect("sheet exports");
    let manifest: Value =
        serde_json::from_str(&fs::read_to_string(&report.manifest).unwrap()).unwrap();
    check_against(&schema, defs, &manifest, "sheet");
    assert_eq!(manifest["format"], schema["properties"]["format"]["const"]);
    assert_eq!(
        manifest["version"],
        schema["properties"]["version"]["const"]
    );
    for sprite in manifest["sprites"].as_array().unwrap() {
        check_against(&defs["sprite"], defs, sprite, "sprite");
    }
    for animation in manifest["animations"].as_array().unwrap() {
        check_against(&defs["animation"], defs, animation, "animation");
        for frame in animation["frames"].as_array().unwrap() {
            check_against(&defs["frame"], defs, frame, "frame");
        }
    }
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn debug_grid_draws_borders_on_an_opaque_copy() {
    let objects = vec![SheetObject {
        name: "chair".into(),
        object: box_object("chair", 1.0, 1.0),
        source: None,
    }];
    let mut options = chair_options("debug");
    options.debug_grid = true;
    let dir = scratch("iso-scene-sprites-debug-test");
    let report = export(&objects, &BTreeMap::new(), &options, &dir).expect("sheet exports");
    let path = report.debug.expect("debug image written");
    let decoder = png::Decoder::new(std::io::BufReader::new(fs::File::open(&path).unwrap()));
    let mut reader = decoder.read_info().unwrap();
    let mut buffer = vec![0; reader.output_buffer_size()];
    reader.next_frame(&mut buffer).unwrap();
    assert!(buffer.chunks_exact(4).all(|pixel| pixel[3] == 255));
    // The top-left pixel is a cell border.
    assert_eq!(&buffer[..4], &[220, 40, 160, 255]);
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn atlas_lists_frames_pivots_and_animations() {
    let objects = vec![SheetObject {
        name: "door".into(),
        object: box_object("door", 1.0, 1.0),
        source: None,
    }];
    let mut options = chair_options("atlas");
    options.views = vec![0.0, 90.0];
    options.padding = 2;
    options.atlas = true;
    options.animations = vec![door_animation("open", 3)];
    let dir = scratch("iso-scene-sprites-atlas-test");
    let report = export(&objects, &BTreeMap::new(), &options, &dir).expect("sheet exports");
    let atlas: Value =
        serde_json::from_str(&fs::read_to_string(report.atlas.unwrap()).unwrap()).unwrap();
    let [w, h] = report.cells;
    assert_eq!(atlas["frames"].as_object().unwrap().len(), 5);
    assert_eq!(atlas["frames"]["door/1"]["frame"]["x"], 2 + w + 2);
    assert_eq!(atlas["frames"]["open#2"]["frame"]["y"], 2 + h + 2);
    assert_eq!(atlas["frames"]["open#2"]["duration"], 100);
    let pivot = atlas["frames"]["door/0"]["pivot"]["x"].as_f64().unwrap();
    assert!((pivot - 0.5).abs() < 1e-9);
    assert_eq!(
        atlas["animations"]["open"],
        json!(["open#0", "open#1", "open#2"])
    );
    assert_eq!(atlas["meta"]["image"], "atlas.png");
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn max_rows_splits_pages_with_their_own_cell_size() {
    let objects = vec![
        SheetObject {
            name: "cup".into(),
            object: box_object("cup", 1.0, 1.0),
            source: None,
        },
        SheetObject {
            name: "bed".into(),
            object: box_object("bed", 4.0, 3.0),
            source: None,
        },
    ];
    let mut options = chair_options("paged");
    options.max_rows = Some(1);
    options.atlas = true;
    let dir = scratch("iso-scene-sprites-pages-test");
    let report = export(&objects, &BTreeMap::new(), &options, &dir).expect("sheet exports");
    assert_eq!(report.pages, 2);
    assert!(dir.join("paged-1.png").exists());
    assert!(dir.join("paged-2.png").exists());
    assert!(dir.join("paged-2.tsj").exists());
    assert!(dir.join("paged-2.atlas.json").exists());
    assert!(!dir.join("paged.png").exists());
    let manifest: Value =
        serde_json::from_str(&fs::read_to_string(&report.manifest).unwrap()).unwrap();
    let pages = manifest["pages"].as_array().unwrap();
    assert_eq!(pages.len(), 2);
    let small = pages[0]["cell"]["width"].as_u64().unwrap();
    let large = pages[1]["cell"]["width"].as_u64().unwrap();
    assert!(
        small < large,
        "the cup page should use smaller cells: {small} vs {large}"
    );
    let bed = &manifest["sprites"][4];
    assert_eq!(bed["name"], "bed");
    assert_eq!(bed["page"], 1);
    assert_eq!(bed["y"], 0);
    let _ = fs::remove_dir_all(dir);
}

fn read_png(path: &Path) -> (u32, u32, Vec<u8>) {
    let decoder = png::Decoder::new(std::io::BufReader::new(fs::File::open(path).unwrap()));
    let mut reader = decoder.read_info().unwrap();
    let mut buffer = vec![0; reader.output_buffer_size()];
    let info = reader.next_frame(&mut buffer).unwrap();
    (info.width, info.height, buffer)
}

fn opaque(rgba: &[u8]) -> usize {
    rgba.chunks_exact(4).filter(|pixel| pixel[3] > 0).count()
}

#[test]
fn every_sprite_is_packed_whole_without_overlap() {
    let shapes = [("a", 1.0, 1.0), ("b", 3.0, 1.0), ("c", 1.0, 2.5)];
    let objects: Vec<SheetObject> = shapes
        .iter()
        .map(|(name, width, depth)| SheetObject {
            name: (*name).into(),
            object: box_object(name, *width, *depth),
            source: None,
        })
        .collect();
    let mut options = chair_options("whole");
    options.views = vec![0.0, 45.0, 90.0, 180.0, 270.0];
    options.padding = 1;
    options.animations = vec![AnimationSpec {
        object: "b".into(),
        ..door_animation("swing", 4)
    }];
    let dir = scratch("iso-scene-sprites-whole-test");
    let report = export(&objects, &BTreeMap::new(), &options, &dir).expect("sheet exports");
    let (_, _, sheet) = read_png(&report.image);

    // Composited pixels never merge (no overlap) and are never cut (no
    // clipping), so the sheet holds exactly the sum of all sprites.
    let mut expected = 0;
    for entry in &objects {
        for view in &options.views {
            let (rotation, angle) = split_view(*view);
            let sprite = render_object_sprite_with(
                &entry.object,
                rotation,
                angle,
                8,
                &BTreeMap::new(),
                true,
            )
            .unwrap();
            expected += opaque(&sprite.image.rgba);
        }
    }
    for angle in frame_angles(0.0, 90.0, 4) {
        let sprite =
            render_object_sprite_with(&objects[1].object, 0, angle, 8, &BTreeMap::new(), true)
                .unwrap();
        expected += opaque(&sprite.image.rgba);
    }
    assert_eq!(opaque(&sheet), expected);
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn static_views_stand_on_the_anchor_line() {
    let objects = vec![
        SheetObject {
            name: "a".into(),
            object: box_object("a", 1.0, 1.0),
            source: None,
        },
        SheetObject {
            name: "b".into(),
            object: box_object("b", 3.0, 1.0),
            source: None,
        },
    ];
    let mut options = chair_options("floor");
    options.shadow = false;
    let dir = scratch("iso-scene-sprites-floor-test");
    let report = export(&objects, &BTreeMap::new(), &options, &dir).expect("sheet exports");
    let (width, _, sheet) = read_png(&report.image);
    let manifest: Value =
        serde_json::from_str(&fs::read_to_string(&report.manifest).unwrap()).unwrap();
    for sprite in manifest["sprites"].as_array().unwrap() {
        let [x, y, w, h] = ["x", "y", "w", "h"].map(|key| sprite[key].as_u64().unwrap());
        let anchor_y = sprite["anchor"]["y"].as_f64().unwrap();
        let lowest = (y..y + h)
            .filter(|row| {
                (x..x + w)
                    .any(|column| sheet[((row * u64::from(width) + column) * 4 + 3) as usize] > 0)
            })
            .max()
            .unwrap();
        // A box's front corner is its lowest point: it sits on the anchor
        // line (allowing for the 1 px outline and rounding).
        let offset = (lowest - y) as f64 - anchor_y;
        assert!(
            offset.abs() <= 2.0,
            "{}: lowest row {offset} px from the anchor",
            sprite["name"]
        );
    }
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn tileset_ids_and_sizes_are_consistent() {
    let objects = vec![SheetObject {
        name: "door".into(),
        object: box_object("door", 1.0, 1.0),
        source: None,
    }];
    let mut options = chair_options("tsj");
    options.padding = 2;
    options.animations = vec![door_animation("open", 7), door_animation("shut", 2)];
    options.max_rows = Some(2);
    let dir = scratch("iso-scene-sprites-tsj-test");
    let report = export(&objects, &BTreeMap::new(), &options, &dir).expect("sheet exports");
    for page in 1..=report.pages {
        let stem = format!("tsj-{page}");
        let tileset: Value =
            serde_json::from_str(&fs::read_to_string(dir.join(format!("{stem}.tsj"))).unwrap())
                .unwrap();
        let (width, height, _) = read_png(&dir.join(format!("{stem}.png")));
        assert_eq!(tileset["imagewidth"], width);
        assert_eq!(tileset["imageheight"], height);
        let [count, columns, margin, spacing, tile_w, tile_h] = [
            "tilecount",
            "columns",
            "margin",
            "spacing",
            "tilewidth",
            "tileheight",
        ]
        .map(|key| tileset[key].as_u64().unwrap());
        // Tiled derives the grid from the image the same way.
        assert_eq!(
            columns,
            (u64::from(width) - 2 * margin + spacing) / (tile_w + spacing)
        );
        let rows = (u64::from(height) - 2 * margin + spacing) / (tile_h + spacing);
        assert_eq!(count, columns * rows);
        for tile in tileset["tiles"].as_array().unwrap() {
            assert!(tile["id"].as_u64().unwrap() < count);
            for frame in tile["animation"].as_array().into_iter().flatten() {
                assert!(frame["tileid"].as_u64().unwrap() < count);
            }
        }
    }
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn orthographic_sheets_anchor_on_the_footprint() {
    let objects = vec![SheetObject {
        name: "desk".into(),
        object: box_object("desk", 2.0, 1.0),
        source: None,
    }];
    for projection in [Projection::Top, Projection::Front, Projection::Side] {
        let mut options = chair_options(projection.name());
        options.projection = projection;
        options.shadow = false;
        let dir = scratch(&format!(
            "iso-scene-sprites-ortho-{}-test",
            projection.name()
        ));
        let report = export(&objects, &BTreeMap::new(), &options, &dir).expect("sheet exports");
        let manifest: Value =
            serde_json::from_str(&fs::read_to_string(&report.manifest).unwrap()).unwrap();
        assert_eq!(manifest["projection"], projection.name());
        assert_eq!(manifest["shadow"], false);
        let (width, _, sheet) = read_png(&report.image);
        let sprite = &manifest["sprites"][0];
        let [x, y, w, h] = ["x", "y", "w", "h"].map(|key| sprite[key].as_u64().unwrap());
        let anchor_y = sprite["anchor"]["y"].as_f64().unwrap();
        let lowest = (y..y + h)
            .filter(|row| {
                (x..x + w)
                    .any(|column| sheet[((row * u64::from(width) + column) * 4 + 3) as usize] > 0)
            })
            .max()
            .unwrap();
        // In every orthographic view the front pivot is the bottom edge.
        let offset = (lowest - y) as f64 - anchor_y;
        assert!(offset.abs() <= 2.0, "{projection:?}: {offset}");
        let _ = fs::remove_dir_all(dir);
    }
}

#[test]
fn object_layers_get_a_tileset_without_the_horizontal_shift() {
    let objects = vec![SheetObject {
        name: "desk".into(),
        object: box_object("desk", 2.0, 1.0),
        source: None,
    }];
    let mut options = chair_options("layers");
    options.tile_width = Some(64);
    let dir = scratch("iso-scene-sprites-layers-test");
    let report = export(&objects, &BTreeMap::new(), &options, &dir).expect("sheet exports");
    let read = |name: &str| -> Value {
        serde_json::from_str(&fs::read_to_string(dir.join(name)).unwrap()).unwrap()
    };
    let tiles = read("layers.tsj");
    let objects = read("layers.objects.tsj");
    let cell_width = i64::from(report.cells[0]);
    // Verified in Tiled with tmxrasterizer on an isometric map: a tile
    // layer needs the shift, a bottom-aligned tile object does not.
    assert_eq!(tiles["tileoffset"]["x"], (64 - cell_width) / 2);
    assert_eq!(objects["tileoffset"]["x"], 0);
    assert_eq!(objects["tileoffset"]["y"], tiles["tileoffset"]["y"]);
    assert_eq!(objects["objectalignment"], "bottom");
    assert_eq!(
        read("layers.json")["pages"][0]["objectTileset"],
        "layers.objects.tsj"
    );
    let _ = fs::remove_dir_all(dir);
}
