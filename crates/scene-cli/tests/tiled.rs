//! End-to-end checks for `iso-scene tiled`.

use std::fs;
use std::path::PathBuf;
use std::process::Command;

fn binary() -> Command {
    Command::new(env!("CARGO_BIN_EXE_iso-scene"))
}

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("iso-scene-tiled-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn a_huge_preview_is_skipped_and_the_map_is_written() {
    let dir = scratch("preview-limit");
    fs::write(
        dir.join("big.scene"),
        "version 1;\nobject a (1, 1) { box at (0, 0, 0) size (1, 1, 1) material wood; }\nrender iso;\nscale 32;\ncanvas (512, 512, 1);\nplace a at (0, 0, 0);\n",
    )
    .unwrap();
    let output = binary()
        .current_dir(&dir)
        .args(["tiled", "big.scene", "-o", "map"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(dir.join("map/scene.tmj").is_file());
    assert!(!dir.join("map/preview.png").exists());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("preview skipped"), "{stderr}");
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn a_small_scene_still_writes_the_preview() {
    let dir = scratch("preview-small");
    fs::write(
        dir.join("small.scene"),
        "version 1;\nobject a (1, 1) { box at (0, 0, 0) size (1, 1, 1) material wood; }\nrender iso;\nscale 8;\nplace a at (0, 0, 0);\n",
    )
    .unwrap();
    let output = binary()
        .current_dir(&dir)
        .args(["tiled", "small.scene", "-o", "map"])
        .output()
        .unwrap();
    assert!(output.status.success());
    assert!(dir.join("map/preview.png").is_file());
    assert!(!String::from_utf8_lossy(&output.stderr).contains("preview skipped"));
    let _ = fs::remove_dir_all(dir);
}
