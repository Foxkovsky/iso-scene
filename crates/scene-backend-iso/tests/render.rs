use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use scene_backend_iso::{IsoImage, render, write_png};
use scene_core::compile_scene;
use scene_syntax::parse;

fn render_source(source: &str) -> Result<IsoImage, scene_syntax::Diagnostic> {
    let program = parse(source).unwrap_or_else(|error| panic!("{}: {}", error.code, error.message));
    let here = Path::new(".");
    let scene = compile_scene(&program, here, here)
        .unwrap_or_else(|error| panic!("{}: {}", error.code, error.message));
    render(&scene)
}

fn examples() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples")
}

#[test]
fn single_box_produces_faces_and_outline() {
    let image = render_source(
        r"
        version 1;
        object box1 (1, 1) { box at (0, 0, 0) size (1, 1, 1) material wood; }
        render iso;
        scale 32;
        place box1 at (0, 0, 0);
        ",
    )
    .expect("render");

    assert!(image.width > 30 && image.height > 30);
    let colors: HashSet<[u8; 4]> = image
        .rgba
        .chunks_exact(4)
        .map(|pixel| [pixel[0], pixel[1], pixel[2], pixel[3]])
        .collect();
    for expected in [
        [0xB0, 0xB2, 0xB6, 255],
        [0x8A, 0x8D, 0x92, 255],
        [0x68, 0x6B, 0x70, 255],
        [0x26, 0x28, 0x2E, 255],
    ] {
        assert!(colors.contains(&expected), "missing colour {expected:?}");
    }
}

#[test]
fn all_primitives_render() {
    let image = render_source(
        r"
        version 1;
        object shapes (4, 4) {
            box at (0, 0, 0) size (1, 1, 1) material wood;
            cyl at (2.5, 0.5, 0) radius 0.5 height 1 material metal;
            sphere at (0.5, 2.5, 0.5) radius 0.5 material light;
            slantbox at (2, 2, 0) size (1, 1, 1) offset (0.2, 0.1) material red;
            slantcyl at (3.5, 3.5, 0) radius 0.4 height 1 offset (-0.2, 0) material glass;
            ramp at (0, 3.2, 0) size (1, 0.6) rise (0, 0.5) material dark;
        }
        render iso;
        scale 24;
        place shapes at (0, 0, 0);
        ",
    )
    .expect("render");

    assert!(image.rgba.chunks_exact(4).any(|pixel| pixel[3] == 255));
}

#[test]
fn classroom_scene_renders_deterministically() {
    let root = examples();
    let source = fs::read_to_string(root.join("classroom.scene")).expect("read classroom");
    let program = parse(&source).expect("parse classroom");
    let scene = compile_scene(&program, &root, &root).expect("compile classroom");
    let first = render(&scene).expect("render classroom");
    let second = render(&scene).expect("render classroom again");

    assert!(first.width > 1_000, "width {}", first.width);
    assert!(first.height > 500, "height {}", first.height);
    assert_eq!(first, second);
    assert!(first.rgba.chunks_exact(4).any(|pixel| pixel[3] == 255));
}

#[test]
fn background_colour_fills_empty_pixels() {
    let image = render_source(
        r"
        version 1;
        object box1 (1, 1) { box at (0, 0, 0) size (1, 1, 1) material wood; }
        render iso;
        scale 16;
        background #102030;
        place box1 at (0, 0, 0);
        ",
    )
    .expect("render");
    assert_eq!(image.pixel(0, 0), [0x10, 0x20, 0x30, 255]);
}

#[test]
fn missing_material_file_is_reported() {
    let error = render_source(
        r#"
        version 1;
        material oak = "oak.toml";
        object box1 (1, 1) { box at (0, 0, 0) size (1, 1, 1) material oak; }
        render iso;
        scale 16;
        place box1 at (0, 0, 0);
        "#,
    )
    .expect_err("material files are not implemented");
    assert_eq!(error.code, "E0601");
}

#[test]
fn non_iso_render_kind_is_rejected() {
    let error = render_source(
        "version 1; object box1 (1, 1) { box at (0, 0, 0) size (1, 1, 1); } render top; scale 8; place box1 at (0, 0, 0);",
    )
    .expect_err("top is a different backend");
    assert_eq!(error.code, "E0604");
}

#[test]
fn png_is_written_with_a_signature() {
    let image = render_source(
        "version 1; object box1 (1, 1) { box at (0, 0, 0) size (1, 1, 1); } render iso; scale 8; place box1 at (0, 0, 0);",
    )
    .expect("render");
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_nanos());
    let path = std::env::temp_dir().join(format!(
        "iso-scene-render-{}-{nanos}.png",
        std::process::id()
    ));
    write_png(&image, &path).expect("write png");
    let bytes = fs::read(&path).expect("read png");
    assert_eq!(&bytes[..8], b"\x89PNG\r\n\x1a\n");
    fs::remove_file(&path).expect("remove png");
}

#[test]
fn toml_material_colours_are_loaded() {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_nanos());
    let root = std::env::temp_dir().join(format!(
        "iso-scene-material-render-{}-{nanos}",
        std::process::id()
    ));
    fs::create_dir_all(&root).expect("create temp dir");
    fs::write(
        root.join("paint.toml"),
        "name = \"Paint\"\ncolor = \"#3366CC\"\noutline = \"#101820\"\n",
    )
    .expect("write material");
    let source = r#"
        version 1;
        material paint = "paint.toml";
        object box1 (1, 1) { box at (0, 0, 0) size (1, 1, 1) material paint; }
        render iso;
        scale 32;
        place box1 at (0, 0, 0);
    "#;
    let program = parse(source).expect("parse");
    let scene = compile_scene(&program, &root, &root).expect("compile");
    let image = render(&scene).expect("render");
    assert!(
        image
            .rgba
            .chunks_exact(4)
            .any(|pixel| pixel[..3] == [0x33, 0x66, 0xCC]),
        "the custom top colour must appear"
    );
    let _ = fs::remove_dir_all(root);
}
