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
