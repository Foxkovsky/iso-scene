//! End-to-end checks for `iso-scene render`.

use std::fs;
use std::path::PathBuf;
use std::process::Command;

fn binary() -> Command {
    Command::new(env!("CARGO_BIN_EXE_iso-scene"))
}

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("iso-scene-render-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn several_rotations_write_one_png_per_angle() {
    let dir = scratch("rotations");
    fs::write(
        dir.join("a.scene"),
        "version 1;\nobject a (1, 1) { box at (0, 0, 0) size (1, 1, 1) material wood; box at (0, 0, 1) size (0.3, 0.3, 0.5) material red; }\nrender iso;\nscale 8;\nplace a at (0, 0, 0);\n",
    )
    .unwrap();
    let output = binary()
        .current_dir(&dir)
        .args(["render", "a.scene", "--rotate", "0,90", "-o", "rots"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(dir.join("rots/a-0.png").is_file());
    assert!(dir.join("rots/a-90.png").is_file());
    let single = binary()
        .current_dir(&dir)
        .args(["render", "a.scene", "--rotate", "90", "-o", "single.png"])
        .output()
        .unwrap();
    assert!(single.status.success());
    assert_eq!(
        fs::read(dir.join("rots/a-90.png")).unwrap(),
        fs::read(dir.join("single.png")).unwrap()
    );
    // Several angles need a directory, not a PNG path.
    let output = binary()
        .current_dir(&dir)
        .args(["render", "a.scene", "--rotate", "0,90", "-o", "out.png"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn duplicate_or_invalid_rotations_are_usage_errors() {
    let dir = scratch("invalid");
    fs::write(
        dir.join("a.scene"),
        "version 1;\nobject a (1, 1) { box at (0, 0, 0) size (1, 1, 1) material wood; }\nrender iso;\nscale 8;\nplace a at (0, 0, 0);\n",
    )
    .unwrap();
    for angles in ["0,0", "45", "0,,90"] {
        let output = binary()
            .current_dir(&dir)
            .args(["render", "a.scene", "--rotate", angles, "-o", "out"])
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(2), "{angles}");
    }
    let _ = fs::remove_dir_all(dir);
}
