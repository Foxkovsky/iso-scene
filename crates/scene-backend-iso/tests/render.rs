use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use scene_backend_iso::{IsoImage, render, write_png};
use scene_core::{compile_scene, compile_scene_frame};
use scene_syntax::parse;

fn compile_source(source: &str) -> scene_core::SceneGraph {
    let program = parse(source).unwrap_or_else(|error| panic!("{}: {}", error.code, error.message));
    let here = Path::new(".");
    compile_scene(&program, here, here)
        .unwrap_or_else(|error| panic!("{}: {}", error.code, error.message))
}

fn render_source(source: &str) -> Result<IsoImage, scene_syntax::Diagnostic> {
    render(&compile_source(source))
}

fn examples() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples")
}

#[test]
fn rotating_the_scene_shows_another_side() {
    let mut scene = compile_source(
        r"
        version 1;
        object room (4, 4) { box at (0, 0, 0) size (4, 4, 1) material wood; }
        object mark (1, 1) { box at (0, 0, 1) size (1, 1, 1) material red; }
        render iso;
        scale 16;
        place room at (0, 0, 0);
        place mark at (0, 0, 0);
        ",
    );
    let front = render(&scene).expect("render front");
    scene.rotate_z(90);
    let side = render(&scene).expect("render side");
    assert!(
        (front.width, front.height, &front.rgba) != (side.width, side.height, &side.rgba),
        "90 degrees looks the same as 0"
    );
}

#[test]
fn four_quarter_turns_render_the_original_pixels_again() {
    let mut scene = compile_source(
        r"
        version 1;
        object room (4, 4) { box at (0, 0, 0) size (4, 4, 1) material wood; }
        object mark (1, 1) { box at (0, 0, 1) size (1, 1, 1) material red; }
        render iso;
        scale 16;
        place room at (0, 0, 0);
        place mark at (0, 0, 0);
        ",
    );
    let front = render(&scene).expect("render");
    for _ in 0..4 {
        scene.rotate_z(90);
    }
    let again = render(&scene).expect("render again");
    assert_eq!(front.rgba, again.rgba);
}

#[test]
fn an_animated_decal_reads_the_frame_from_the_scene_animation() {
    let dir = std::env::temp_dir().join(format!("iso-scene-decal-frames-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let mut rgba = vec![0_u8; 4 * 2 * 4];
    for y in 0..2 {
        for x in 0..2 {
            let left = (y * 4 + x) * 4;
            rgba[left..left + 4].copy_from_slice(&[255, 0, 255, 255]);
            let right = (y * 4 + x + 2) * 4;
            rgba[right..right + 4].copy_from_slice(&[0, 255, 255, 255]);
        }
    }
    scene_render::write_png(
        &scene_render::Image {
            width: 4,
            height: 2,
            rgba,
        },
        &dir.join("strip.png"),
    )
    .unwrap();
    let source = r#"
        version 1;
        object panel (1, 1) {
            box at (0, 0, 0) size (1, 1, 1) material dark;
            decal "strip.png" at (0, 1, 0) size (1, 1) facing +y animation matrix;
        }
        render iso;
        scale 32;
        background transparent;
        animation matrix (t) frames 2 duration 100;
        place panel at (0, 0, 0);
    "#;
    let program = parse(source).expect("parse");
    let first = compile_scene_frame(&program, &dir, &dir, "matrix", 0).expect("frame 0");
    let second = compile_scene_frame(&program, &dir, &dir, "matrix", 1).expect("frame 1");
    let has = |image: &scene_render::Image, color: [u8; 4]| {
        image.rgba.chunks_exact(4).any(|pixel| pixel == color)
    };
    let first_image = render(&first).expect("render 0");
    let second_image = render(&second).expect("render 1");
    assert!(has(&first_image, [255, 0, 255, 255]), "frame 0");
    assert!(has(&second_image, [0, 255, 255, 255]), "frame 1");
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn a_decal_is_drawn_as_an_unlit_image_with_transparency() {
    let dir = std::env::temp_dir().join(format!("iso-scene-decal-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let mut rgba = vec![0_u8; 4 * 4 * 4];
    for y in 0..4 {
        for x in 0..2 {
            let index = (y * 4 + x) * 4;
            rgba[index..index + 4].copy_from_slice(&[255, 0, 255, 255]);
        }
    }
    scene_render::write_png(
        &scene_render::Image {
            width: 4,
            height: 4,
            rgba,
        },
        &dir.join("decal.png"),
    )
    .unwrap();
    let source = r#"
        version 1;
        object panel (2, 1) {
            box at (0, 0, 0) size (2, 1, 2) material red;
            decal "decal.png" at (0, 1, 0) size (2, 2) facing +y;
        }
        render iso;
        scale 32;
        background transparent;
        place panel at (0, 0, 0);
    "#;
    let program = parse(source).expect("parse");
    let scene = compile_scene(&program, &dir, &dir).expect("compile");
    let image = render(&scene).expect("render");
    let has_magenta = image
        .rgba
        .chunks_exact(4)
        .any(|pixel| pixel == [255, 0, 255, 255]);
    let has_red_face = image
        .rgba
        .chunks_exact(4)
        .any(|pixel| pixel == [0x6C, 0x10, 0x14, 255]);
    assert!(has_magenta, "the decal colour was not drawn");
    assert!(has_red_face, "the transparent half did not reveal the box");
    let _ = fs::remove_dir_all(dir);
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

#[test]
fn the_order_of_equivalent_placements_does_not_change_pixels() {
    let scene = |first: &str, second: &str| {
        compile_source(&format!(
            r"
            version 1;
            object a (1, 1) {{ box at (0, 0, 0) size (1, 1, 1) material wood; }}
            object b (1, 1) {{ box at (0, 0, 0) size (1, 1, 1) material metal; }}
            render iso;
            scale 16;
            place {first} at (0, 0, 0);
            place {second} at (0, 0, 0);
            "
        ))
    };
    let first = render(&scene("a", "b")).expect("render a b");
    let second = render(&scene("b", "a")).expect("render b a");
    assert_eq!(first.width, second.width);
    assert_eq!(first.height, second.height);
    assert_eq!(first.rgba, second.rgba);
}

#[test]
fn scene_light_defaults_keep_the_pixels_and_other_values_change_them() {
    let source = |extra: &str| {
        format!(
            "version 1; {extra} object a (1, 1) {{ box at (0, 0, 0) size (1, 1, 1) material wood; }} render iso; scale 8; place a at (0, 0, 0);"
        )
    };
    let base = render(&compile_source(&source(""))).expect("base");
    let explicit =
        render(&compile_source(&source("sun (135, 45); ambient 0.35;"))).expect("explicit");
    assert_eq!(base.rgba, explicit.rgba);
    let bright = render(&compile_source(&source("sun (0, 90); ambient 0.8;"))).expect("bright");
    assert_ne!(base.rgba, bright.rgba);
}

fn compile_at(source: &str, base: &Path) -> scene_core::SceneGraph {
    let program = parse(source).unwrap_or_else(|error| panic!("{}: {}", error.code, error.message));
    compile_scene(&program, base, base)
        .unwrap_or_else(|error| panic!("{}: {}", error.code, error.message))
}

#[test]
fn halo_directive_changes_the_bloom() {
    let dir = std::env::temp_dir().join(format!("iso-scene-halo-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    fs::write(
        dir.join("glow.toml"),
        "color = \"#FFFFFF\"\nemission = \"#FFFFFF\"\nglow = 1.0\n",
    )
    .unwrap();
    let source = |halo: &str| {
        format!(
            r#"
            version 1;
            {halo}
            material glow = "glow.toml";
            object b (1, 1) {{ box at (0, 0, 0) size (1, 1, 1) material glow; }}
            render iso;
            scale 8;
            place b at (0, 0, 0);
            "#
        )
    };
    let weak = render(&compile_at(&source("halo (0, 0);"), &dir)).expect("render weak halo");
    let strong = render(&compile_at(&source("halo (6, 4.0);"), &dir)).expect("render strong halo");
    assert_ne!(weak.rgba, strong.rgba);
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn contact_shadow_style_changes_the_sprite() {
    let scene = compile_source(
        r"
        version 1;
        object b (1, 1) { box at (0, 0, 0) size (1, 1, 1) material wood; }
        render iso;
        scale 8;
        place b at (0, 0, 0);
        ",
    );
    let object = &scene.instances[0].object;
    let reference =
        scene_backend_iso::render_object_sprite_with(object, 0, 0.0, 8, &scene.materials, true)
            .expect("reference sprite");
    let custom = scene_backend_iso::render_object_sprite_with_light(
        object,
        0,
        0.0,
        8,
        &scene.materials,
        true,
        scene_render::material::Light::default(),
        scene_core::Shadow {
            reach: 0.8,
            strength: 2.0,
        },
        scene_core::Halo::default(),
    )
    .expect("custom sprite");
    assert_ne!(reference.image.rgba, custom.image.rgba);
}

#[test]
fn image_textures_and_outline_width_render() {
    let dir = std::env::temp_dir().join(format!("iso-scene-texture-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let mut rgba = vec![0_u8; 8 * 8 * 4];
    for y in 0..8 {
        for x in 0..8 {
            let index = (y * 8 + x) * 4;
            let color = if (x / 2 + y / 2) % 2 == 0 {
                [220, 60, 40, 255]
            } else {
                [40, 80, 200, 255]
            };
            rgba[index..index + 4].copy_from_slice(&color);
        }
    }
    scene_render::write_png(
        &scene_render::Image {
            width: 8,
            height: 8,
            rgba,
        },
        &dir.join("checker.png"),
    )
    .unwrap();
    for (name, width) in [("thin", 1.0), ("wide", 4.0)] {
        fs::write(
            dir.join(format!("{name}.toml")),
            format!(
                "name = \"{name}\"\ncolor = \"#FFFFFF\"\noutline = \"#101010\"\noutline_width = {width}\nspecular = 0.0\n[texture]\nimage = \"checker.png\"\nscale = 1.0\n"
            ),
        )
        .unwrap();
    }
    let source = |name: &str| {
        format!(
            r#"
            version 1;
            material {name} = "{name}.toml";
            object crate (2, 2) {{ box at (0, 0, 0) size (2, 2, 2) material {name}; }}
            render iso;
            scale 16;
            place crate at (0, 0, 0);
            "#
        )
    };
    let thin = render(&compile_at(&source("thin"), &dir)).expect("thin render");
    let wide = render(&compile_at(&source("wide"), &dir)).expect("wide render");
    let dark = |image: &scene_backend_iso::IsoImage| {
        image
            .rgba
            .chunks_exact(4)
            .filter(|pixel| pixel[0] < 40 && pixel[1] < 40 && pixel[2] < 40)
            .count()
    };
    assert!(
        dark(&wide) > dark(&thin),
        "outline_width did not thicken the line"
    );
    let has_red = thin
        .rgba
        .chunks_exact(4)
        .any(|pixel| pixel[0] == 220 && pixel[1] == 60 && pixel[2] == 40);
    let has_blue = thin
        .rgba
        .chunks_exact(4)
        .any(|pixel| pixel[0] == 40 && pixel[1] == 80 && pixel[2] == 200);
    assert!(has_red && has_blue, "the albedo image was not sampled");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn scene_floor_is_rendered_under_objects() {
    let plain = compile_source(
        "version 1; object b (1, 1) { box at (0, 0, 0) size (1, 1, 1) material white; } render iso; scale 8; place b at (0, 0, 0);",
    );
    let floored = compile_source(
        "version 1; floor red; canvas (4, 4, 4); object b (1, 1) { box at (0, 0, 0) size (1, 1, 1) material white; } render iso; scale 8; place b at (0, 0, 0);",
    );
    let plain_image = render(&plain).expect("plain");
    let floored_image = render(&floored).expect("floor");
    assert!(floored_image.width > plain_image.width);
    let has_floor = floored_image
        .rgba
        .chunks_exact(4)
        .any(|pixel| pixel[0] > 100 && pixel[0] > pixel[1] && pixel[0] > pixel[2]);
    assert!(has_floor, "the floor material did not paint the ground");
}

#[test]
fn floor_receives_cast_shadows() {
    let source = |shadow: &str, sun: &str| {
        format!(
            r"
            version 1;
            floor white;
            canvas (6, 6, 4);
            {sun}
            shadow (0.4, {shadow});
            object crate (1, 1) {{ box at (2, 2, 0) size (1, 1, 4) material red; }}
            render iso;
            scale 8;
            place crate at (0, 0, 0);
            "
        )
    };
    let lit = render(&compile_source(&source("1", "sun (135, 45);"))).expect("lit");
    let off = render(&compile_source(&source("0", "sun (135, 45);"))).expect("off");
    assert_ne!(
        lit.rgba, off.rgba,
        "shadow strength 0 did not remove the cast"
    );
    let noon = render(&compile_source(&source("1", "sun (135, 90);"))).expect("noon");
    assert_ne!(
        lit.rgba, noon.rgba,
        "sun altitude did not change the shadow"
    );
    let dark = |image: &scene_backend_iso::IsoImage| {
        image
            .rgba
            .chunks_exact(4)
            .filter(|pixel| pixel[0] < 240 && pixel[1] < 240 && pixel[2] < 240)
            .count()
    };
    assert!(
        dark(&lit) > dark(&off),
        "the floor did not receive a shadow"
    );
}
