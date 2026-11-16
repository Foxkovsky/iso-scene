//! End-to-end checks for `iso-scene import-tiled`.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn binary() -> Command {
    Command::new(env!("CARGO_BIN_EXE_iso-scene"))
}

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("iso-scene-import-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn write(path: &Path, text: &str) {
    fs::write(path, text).unwrap();
}

const MODEL: &str =
    "version 1;\nobject thing (1, 1) { box at (0, 0, 0) size (1, 1, 1) material wood; }\n";

#[test]
fn an_object_layer_becomes_a_scene() {
    let dir = scratch("objects");
    let models = dir.join("models");
    fs::create_dir_all(models.join("textures")).unwrap();
    write(&models.join("thing.model"), MODEL);
    write(&models.join("textures/screen.png"), "not decoded by import");
    write(
        &dir.join("map.tmj"),
        r#"{
  "type": "map", "orientation": "isometric",
  "width": 4, "height": 4, "tilewidth": 64, "tileheight": 32,
  "tilesets": [],
  "layers": [{
    "type": "objectgroup", "name": "objects",
    "objects": [{
      "id": 1, "name": "thing", "type": "thing",
      "x": 64, "y": 32, "width": 32, "height": 32,
      "properties": [
        {"name": "rotation", "type": "int", "value": 90},
        {"name": "turn", "type": "float", "value": 15}
      ]
    }]
  }]
}"#,
    );
    let output = binary()
        .current_dir(&dir)
        .args([
            "import-tiled",
            "map.tmj",
            "--models",
            "models",
            "--layers",
            "objects",
            "-o",
            "out",
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let scene = fs::read_to_string(dir.join("out/scene.scene")).unwrap();
    assert!(
        scene.contains("place thing at (2, 1, 0) rot 90 turn 15;"),
        "{scene}"
    );
    assert!(dir.join("out/thing.model").exists());
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn a_tile_layer_uses_the_model_property() {
    let dir = scratch("tiles");
    let models = dir.join("models");
    fs::create_dir_all(&models).unwrap();
    write(&models.join("thing.model"), MODEL);
    write(
        &dir.join("map.tmj"),
        r#"{
  "type": "map", "orientation": "isometric",
  "width": 2, "height": 2, "tilewidth": 64, "tileheight": 32,
  "tilesets": [{
    "firstgid": 1, "tilewidth": 32, "tileheight": 32,
    "tilecount": 1, "columns": 1,
    "tiles": [{"id": 0, "type": "thing", "properties": [
      {"name": "model", "type": "string", "value": "thing"}
    ]}]
  }],
  "layers": [{"type": "tilelayer", "name": "objects", "width": 2, "height": 2, "data": [1, 0, 0, 0]}]
}"#,
    );
    let output = binary()
        .current_dir(&dir)
        .args([
            "import-tiled",
            "map.tmj",
            "--models",
            "models",
            "--layers",
            "objects",
            "--scale",
            "1",
            "--cell",
            "32",
            "-o",
            "out",
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let scene = fs::read_to_string(dir.join("out/scene.scene")).unwrap();
    assert!(scene.contains("place thing at (0, 0, 0) rot 0;"), "{scene}");
    let _ = fs::remove_dir_all(dir);
}
