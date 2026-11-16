//! End-to-end checks for `iso-scene check`.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn binary() -> Command {
    Command::new(env!("CARGO_BIN_EXE_iso-scene"))
}

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("iso-scene-check-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn check(dir: &Path, file: &str) -> std::process::Output {
    binary()
        .current_dir(dir)
        .args(["check", file])
        .output()
        .unwrap()
}

#[test]
fn a_scene_is_evaluated_not_only_parsed() {
    let dir = scratch("scene");
    fs::write(dir.join("missing-scale.scene"), "version 1;\nrender iso;\n").unwrap();
    fs::write(
        dir.join("unknown-object.scene"),
        "version 1;\nrender iso;\nscale 8;\nplace nothing at (0, 0, 0);\n",
    )
    .unwrap();
    fs::write(
        dir.join("ok.scene"),
        "version 1;\nrender iso;\nscale 8;\nobject a (1, 1) { box at (0, 0, 0) size (1, 1, 1); }\nplace a at (0, 0, 0);\n",
    )
    .unwrap();
    for (file, code) in [
        ("missing-scale.scene", "E0311"),
        ("unknown-object.scene", "E0301"),
    ] {
        let output = check(&dir, file);
        assert!(!output.status.success(), "{file} must fail");
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(stderr.contains(code), "{file}: {stderr}");
    }
    let output = check(&dir, "ok.scene");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn a_walls_file_is_parsed_by_the_walls_parser() {
    let dir = scratch("walls");
    fs::write(
        dir.join("room.walls"),
        "version 1;\nheight 2;\nlayout {\n\"#.#\"\n}\n",
    )
    .unwrap();
    fs::write(
        dir.join("broken.walls"),
        "version 1;\nlayout {\n\"#?\"\n}\n",
    )
    .unwrap();
    let output = check(&dir, "room.walls");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("2 walls"));
    let output = check(&dir, "broken.walls");
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("E0202"));
    let _ = fs::remove_dir_all(dir);
}

#[cfg(unix)]
#[test]
fn a_bare_file_name_still_keeps_materials_inside_the_working_directory() {
    use std::os::unix::fs::symlink;
    let dir = scratch("bare");
    fs::create_dir_all(dir.join("materials")).unwrap();
    let secret = dir.with_file_name(format!(
        "iso-scene-check-secret-{}.toml",
        std::process::id()
    ));
    fs::write(&secret, "name = \"Secret\"\ncolor = \"#112233\"\n").unwrap();
    symlink(&secret, dir.join("materials/oak.toml")).unwrap();
    fs::write(
        dir.join("linked.scene"),
        "version 1;\nmaterial oak = \"materials/oak.toml\";\nrender iso;\nscale 8;\nobject a (1, 1) { box at (0, 0, 0) size (1, 1, 1) material oak; }\nplace a at (0, 0, 0);\n",
    )
    .unwrap();
    let output = check(&dir, "linked.scene");
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("E0502"));
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn check_rejects_other_files() {
    let dir = scratch("other");
    fs::write(dir.join("notes.txt"), "version 1;").unwrap();
    let output = check(&dir, "notes.txt");
    assert_eq!(output.status.code(), Some(2));
    let _ = fs::remove_dir_all(dir);
}
