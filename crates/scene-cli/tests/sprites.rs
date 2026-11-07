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

#[test]
fn animation_flags_apply_to_the_latest_animation() {
    let dir = scratch("anims");
    write(&dir, "a.model", THING_A);
    let output = binary()
        .current_dir(&dir)
        .args(["sprites", "a.model", "--scale", "8", "--views", "1"])
        .args(["--animation", "thing", "--label", "open", "--frames", "4"])
        .args(["--animation", "thing", "--label", "close", "--from", "90"])
        .args([
            "--to",
            "0",
            "--frames",
            "2",
            "--durations",
            "50,500",
            "--once",
        ])
        .args(["-o", "out"])
        .output()
        .unwrap();
    assert!(output.status.success(), "{}", stderr(&output));
    let manifest = fs::read_to_string(dir.join("out/sheet.json")).unwrap();
    assert!(manifest.contains("\"name\": \"open\""), "{manifest}");
    assert!(manifest.contains("\"name\": \"close\""), "{manifest}");
    assert!(manifest.contains("\"loop\": false"), "{manifest}");
    assert!(manifest.contains("\"duration\": 500"), "{manifest}");

    // The same object twice without labels collides.
    let output = binary()
        .current_dir(&dir)
        .args(["sprites", "a.model", "--scale", "8"])
        .args(["--animation", "thing", "--animation", "thing", "-o", "out2"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2), "{}", stderr(&output));
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn out_of_range_scale_is_a_usage_error() {
    let dir = scratch("scale");
    write(&dir, "a.model", THING_A);
    for scale in ["0", "257", "4294967295"] {
        let output = binary()
            .current_dir(&dir)
            .args(["sprites", "a.model", "--scale", scale, "-o", "out"])
            .output()
            .unwrap();
        assert_eq!(
            output.status.code(),
            Some(2),
            "scale {scale}: {}",
            stderr(&output)
        );
    }
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn exclude_drops_helper_objects() {
    let dir = scratch("exclude");
    write(
        &dir,
        "chair.model",
        "version 1;\nobject leg (1, 1) { box at (0, 0, 0) size (1, 1, 1) material wood; }\n\
         object chair (1, 1) { box at (0, 0, 0) size (1, 1, 2) material wood; }\n",
    );
    let output = binary()
        .current_dir(&dir)
        .args(["sprites", "chair.model", "--scale", "8", "--views", "1"])
        .args(["--exclude", "leg", "-o", "out"])
        .output()
        .unwrap();
    assert!(output.status.success(), "{}", stderr(&output));
    let manifest = fs::read_to_string(dir.join("out/sheet.json")).unwrap();
    assert!(manifest.contains("\"name\": \"chair\""), "{manifest}");
    assert!(!manifest.contains("\"name\": \"leg\""), "{manifest}");

    let output = binary()
        .current_dir(&dir)
        .args([
            "sprites",
            "chair.model",
            "--scale",
            "8",
            "--exclude",
            "nope",
        ])
        .args(["-o", "out2"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2), "{}", stderr(&output));
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn animate_rejects_huge_frame_counts_and_non_finite_angles() {
    let dir = scratch("animate");
    write(&dir, "frame.model", THING_A);
    write(
        &dir,
        "door.scene",
        "version 1;\ninclude \"frame.model\";\nrender iso;\nscale 8;\ncanvas (2, 2, 2);\nplace thing at (0, 0, 0) rot 0;\n",
    );
    for args in [["--frames", "100000"], ["--from", "nan"], ["--to", "inf"]] {
        let output = binary()
            .current_dir(&dir)
            .args(["animate", "door.scene", "--object", "thing"])
            .args(args)
            .args(["-o", "out"])
            .output()
            .unwrap();
        assert_eq!(
            output.status.code(),
            Some(2),
            "{args:?}: {}",
            stderr(&output)
        );
    }
    let output = binary()
        .current_dir(&dir)
        .args([
            "animate",
            "door.scene",
            "--object",
            "thing",
            "--frames",
            "3",
        ])
        .args(["-o", "out"])
        .output()
        .unwrap();
    assert!(output.status.success(), "{}", stderr(&output));
    let _ = fs::remove_dir_all(dir);
}

const ROOM: &str = "version 1;\ninclude \"a.model\";\ninclude \"b.model\";\nrender iso;\nscale 8;\ncanvas (6, 6, 3);\nplace thing at (0, 0, 0) rot 0;\nplace other at (3, 3, 0) rot 90;\nplace thing at (2, 0, 0) rot 0;\n";

fn room(dir: &Path) {
    write(dir, "a.model", THING_A);
    write(
        dir,
        "b.model",
        "version 1;\nobject other (2, 1) { box at (0, 0, 0) size (2, 1, 1) material wood; }\n",
    );
    write(dir, "room.scene", ROOM);
}

fn manifest(dir: &Path, out: &str) -> serde_json::Value {
    serde_json::from_str(&fs::read_to_string(dir.join(out).join("sheet.json")).unwrap()).unwrap()
}

#[test]
fn a_scene_contributes_each_placed_object_once_in_order() {
    let dir = scratch("scene");
    room(&dir);
    let output = binary()
        .current_dir(&dir)
        .args(["sprites", "room.scene", "--views", "8", "-o", "out"])
        .output()
        .unwrap();
    assert!(output.status.success(), "{}", stderr(&output));
    let sheet = manifest(&dir, "out");
    assert_eq!(sheet["rows"], 2);
    assert_eq!(sheet["columns"], 8);
    assert_eq!(sheet["scale"], 8);
    assert_eq!(sheet["sprites"][0]["name"], "thing");
    assert_eq!(sheet["sprites"][8]["name"], "other");
    assert_eq!(sheet["sprites"][1]["rotation"], 45);
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn angles_and_object_select_columns_and_rows() {
    let dir = scratch("select");
    room(&dir);
    let output = binary()
        .current_dir(&dir)
        .args([
            "sprites",
            "room.scene",
            "--angles",
            "0,90",
            "--object",
            "other",
        ])
        .args(["-o", "out"])
        .output()
        .unwrap();
    assert!(output.status.success(), "{}", stderr(&output));
    let sheet = manifest(&dir, "out");
    assert_eq!(sheet["rows"], 1);
    assert_eq!(sheet["views"], serde_json::json!([0, 90]));
    assert_eq!(sheet["sprites"][0]["name"], "other");
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn bad_flag_values_are_usage_errors() {
    let dir = scratch("flags");
    room(&dir);
    for args in [
        &["--views", "3"][..],
        &["--angles", "0,x"],
        &["--pivot", "middle"],
        &["--shadow", "soft"],
        &["--padding", "-1"],
        &["--max-rows", "0"],
        &["--object", "missing"],
        &["--durations", "10"],
    ] {
        let output = binary()
            .current_dir(&dir)
            .args(["sprites", "room.scene"])
            .args(args)
            .args(["-o", "out"])
            .output()
            .unwrap();
        assert_eq!(
            output.status.code(),
            Some(2),
            "{args:?}: {}",
            stderr(&output)
        );
    }
    assert!(!dir.join("out").exists());
    let _ = fs::remove_dir_all(dir);
}
