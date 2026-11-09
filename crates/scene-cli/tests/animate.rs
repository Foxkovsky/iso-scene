//! End-to-end tests of `iso-scene animate` with `animation` directives.

use std::fs;
use std::path::PathBuf;
use std::process::Command;

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("iso-scene-cli-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("scratch dir");
    dir
}

const ROOM: &str = "version 1;
object leaf (1, 0.2) { box at (0, 0, 0) size (w, d, 2) material wood; }
object drawer (1, 1) { box at (0, 0, 0) size (w, d, 0.5) material red; }
render iso;
scale 8;
animation open (t) frames 4 duration 100 once;
animation slide (s) frames 3 duration 50;
place leaf at (0, 0, 0) turn 90 * t;
place drawer at (2, 1 + s, 0);
";

#[test]
fn every_declared_animation_gets_a_strip_tileset_and_map() {
    let dir = scratch("animate-scene");
    fs::write(dir.join("room.scene"), ROOM).expect("write scene");
    let out = dir.join("out");
    let output = Command::new(env!("CARGO_BIN_EXE_iso-scene"))
        .arg("animate")
        .arg(dir.join("room.scene"))
        .arg("-o")
        .arg(&out)
        .output()
        .expect("run");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    for (name, frames) in [("open", 4), ("slide", 3)] {
        for file in [
            format!("{name}.png"),
            format!("{name}.tsj"),
            "scene.tmj".to_owned(),
        ] {
            assert!(out.join(name).join(&file).exists(), "{name}/{file}");
        }
        let tileset: serde_json::Value = serde_json::from_str(
            &fs::read_to_string(out.join(name).join(format!("{name}.tsj"))).expect("tsj"),
        )
        .expect("json");
        assert_eq!(tileset["tilecount"], frames, "{name}");
    }
    // `--animation` picks one.
    let only = dir.join("only");
    let output = Command::new(env!("CARGO_BIN_EXE_iso-scene"))
        .args(["animate", "--animation", "slide", "-o"])
        .arg(&only)
        .arg(dir.join("room.scene"))
        .output()
        .expect("run");
    assert!(output.status.success());
    assert!(only.join("slide").exists() && !only.join("open").exists());
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn a_scene_without_animations_says_how_to_add_one() {
    let dir = scratch("animate-none");
    fs::write(
        dir.join("still.scene"),
        "version 1; object b (1, 1) { box at (0, 0, 0) size (1, 1, 1) material wood; } render iso; scale 8; place b at (0, 0, 0);",
    )
    .expect("write scene");
    let output = Command::new(env!("CARGO_BIN_EXE_iso-scene"))
        .arg("animate")
        .arg(dir.join("still.scene"))
        .output()
        .expect("run");
    assert_eq!(output.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&output.stderr).contains("declares no `animation`"));
    let unknown = Command::new(env!("CARGO_BIN_EXE_iso-scene"))
        .args(["animate", "--animation", "nope"])
        .arg(dir.join("still.scene"))
        .output()
        .expect("run");
    assert_eq!(unknown.status.code(), Some(2));
    let _ = fs::remove_dir_all(dir);
}
