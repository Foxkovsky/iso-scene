//! End-to-end checks for `iso-scene sprites`.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn binary() -> Command {
    Command::new(env!("CARGO_BIN_EXE_iso-scene"))
}

/// Fresh scratch directory for one test.
fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("iso-scene-cli-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn write(dir: &Path, name: &str, text: &str) {
    fs::write(dir.join(name), text).unwrap();
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

const THING_A: &str =
    "version 1;\nobject thing (1, 1) { box at (0, 0, 0) size (1, 1, 1) material wood; }\n";
const THING_B: &str =
    "version 1;\nobject thing (2, 2) { box at (0, 0, 0) size (2, 2, 2) material wood; }\n";

#[test]
fn different_objects_with_one_name_are_a_conflict() {
    let dir = scratch("collide");
    write(&dir, "a.model", THING_A);
    write(&dir, "b.model", THING_B);
    let output = binary()
        .current_dir(&dir)
        .args([
            "sprites", "a.model", "b.model", "--scale", "8", "--views", "1",
        ])
        .args(["-o", "out"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2), "{}", stderr(&output));
    assert!(
        stderr(&output).contains("object `thing`"),
        "{}",
        stderr(&output)
    );
    assert!(!dir.join("out").exists());
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn identical_objects_from_two_files_are_packed_once() {
    let dir = scratch("same");
    write(&dir, "a.model", THING_A);
    write(&dir, "b.model", THING_A);
    let output = binary()
        .current_dir(&dir)
        .args([
            "sprites", "a.model", "b.model", "--scale", "8", "--views", "1",
        ])
        .args(["-o", "out"])
        .output()
        .unwrap();
    assert!(output.status.success(), "{}", stderr(&output));
    let manifest = fs::read_to_string(dir.join("out/sheet.json")).unwrap();
    assert_eq!(manifest.matches("\"name\": \"thing\"").count(), 1);
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn sheet_name_must_be_a_stem() {
    let dir = scratch("name");
    write(&dir, "a.model", THING_A);
    for bad in ["../evil", "sheet.png", ""] {
        let output = binary()
            .current_dir(&dir)
            .args([
                "sprites", "a.model", "--scale", "8", "--name", bad, "-o", "out",
            ])
            .output()
            .unwrap();
        assert_eq!(
            output.status.code(),
            Some(2),
            "`{bad}`: {}",
            stderr(&output)
        );
    }
    assert!(!dir.join("evil.png").exists());
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn unknown_animation_object_and_huge_frame_counts_are_usage_errors() {
    let dir = scratch("anim");
    write(&dir, "a.model", THING_A);
    let output = binary()
        .current_dir(&dir)
        .args([
            "sprites",
            "a.model",
            "--scale",
            "8",
            "--animation",
            "nope",
            "-o",
            "out",
        ])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2), "{}", stderr(&output));
    let output = binary()
        .current_dir(&dir)
        .args(["sprites", "a.model", "--scale", "8", "--animation", "thing"])
        .args(["--frames", "100000", "-o", "out"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2), "{}", stderr(&output));
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn base_outside_the_working_directory_resolves_includes() {
    let dir = scratch("base");
    let project = dir.join("project");
    let elsewhere = dir.join("elsewhere");
    fs::create_dir_all(&project).unwrap();
    fs::create_dir_all(&elsewhere).unwrap();
    write(&project, "frame.model", THING_A);
    write(
        &project,
        "door.scene",
        "version 1;\ninclude \"frame.model\";\nrender iso;\nscale 8;\ncanvas (2, 2, 2);\nplace thing at (0, 0, 0) rot 0;\n",
    );
    let output = binary()
        .current_dir(&elsewhere)
        .arg("sprites")
        .arg(project.join("door.scene"))
        .arg("--base")
        .arg(&project)
        .args(["--views", "1", "-o", "out"])
        .output()
        .unwrap();
    assert!(output.status.success(), "{}", stderr(&output));
    assert!(elsewhere.join("out/sheet.png").exists());
    let _ = fs::remove_dir_all(dir);
}
