#![allow(clippy::cast_precision_loss)]

use std::path::Path;

use scene_backend_ortho::{OrthoImage, View, project_point, render, view_depth};
use scene_core::compile_scene;
use scene_syntax::parse;

fn compile(source: &str) -> scene_core::SceneGraph {
    let program = parse(source).unwrap_or_else(|error| panic!("{}: {}", error.code, error.message));
    let here = Path::new(".");
    compile_scene(&program, here, here)
        .unwrap_or_else(|error| panic!("{}: {}", error.code, error.message))
}

fn render_source(source: &str) -> OrthoResult {
    render(&compile(source))
}

type OrthoResult = Result<OrthoImage, scene_syntax::Diagnostic>;

fn alpha_bbox(image: &OrthoImage) -> (u32, u32, u32, u32) {
    let mut min_x = image.width;
    let mut min_y = image.height;
    let mut max_x = 0;
    let mut max_y = 0;
    let mut any = false;
    for y in 0..image.height {
        for x in 0..image.width {
            if image.pixel(x, y)[3] > 0 {
                any = true;
                min_x = min_x.min(x);
                min_y = min_y.min(y);
                max_x = max_x.max(x);
                max_y = max_y.max(y);
            }
        }
    }
    assert!(any, "image is fully transparent");
    (min_x, min_y, max_x - min_x + 1, max_y - min_y + 1)
}

#[test]
fn projection_axes_are_exact() {
    let scale = 10.0;
    assert_eq!(
        project_point(View::Top, [1.0, 0.0, 0.0], scale),
        [10.0, 0.0]
    );
    assert_eq!(
        project_point(View::Top, [0.0, 1.0, 0.0], scale),
        [0.0, 10.0]
    );
    assert_eq!(project_point(View::Top, [0.0, 0.0, 1.0], scale), [0.0, 0.0]);

    assert_eq!(
        project_point(View::Front, [1.0, 0.0, 0.0], scale),
        [10.0, 0.0]
    );
    assert_eq!(
        project_point(View::Front, [0.0, 0.0, 1.0], scale),
        [0.0, -10.0]
    );

    assert_eq!(
        project_point(View::Side, [0.0, 1.0, 0.0], scale),
        [10.0, 0.0]
    );
    assert_eq!(
        project_point(View::Side, [0.0, 0.0, 1.0], scale),
        [0.0, -10.0]
    );

    assert!(view_depth(View::Top, [0.0, 0.0, 2.0]) > view_depth(View::Top, [0.0, 0.0, 1.0]));
    assert!(view_depth(View::Front, [0.0, 0.0, 0.0]) > view_depth(View::Front, [0.0, 1.0, 0.0]));
    assert!(view_depth(View::Side, [1.0, 0.0, 0.0]) > view_depth(View::Side, [0.0, 0.0, 0.0]));
}

#[test]
fn top_view_of_a_box_is_a_rectangle() {
    let image = render_source(
        "version 1; object b (2, 1) { box at (0, 0, 0) size (2, 1, 1) material wood; } render top; scale 20; place b at (0, 0, 0);",
    )
    .expect("render top");
    let (_, _, width, height) = alpha_bbox(&image);
    assert!((40..=41).contains(&width), "width {width}");
    assert!((20..=21).contains(&height), "height {height}");
    assert_eq!(
        image.pixel(image.width / 2, image.height / 2),
        [0xB0, 0xB2, 0xB6, 255]
    );
}

#[test]
fn front_view_of_a_box_is_a_rectangle() {
    let image = render_source(
        "version 1; object b (2, 1) { box at (0, 0, 0) size (2, 1, 1) material wood; } render front; scale 20; place b at (0, 0, 0);",
    )
    .expect("render front");
    let (_, _, width, height) = alpha_bbox(&image);
    assert!((40..=41).contains(&width), "width {width}");
    assert!((20..=21).contains(&height), "height {height}");
    assert_eq!(
        image.pixel(image.width / 2, image.height / 2),
        [0x68, 0x6B, 0x70, 255]
    );
}

#[test]
fn side_view_of_a_box_is_a_rectangle() {
    let image = render_source(
        "version 1; object b (2, 1) { box at (0, 0, 0) size (2, 1, 1) material wood; } render side; scale 20; place b at (0, 0, 0);",
    )
    .expect("render side");
    let (_, _, width, height) = alpha_bbox(&image);
    assert!((20..=21).contains(&width), "width {width}");
    assert!((20..=21).contains(&height), "height {height}");
    assert_eq!(
        image.pixel(image.width / 2, image.height / 2),
        [0x8A, 0x8D, 0x92, 255]
    );
}

#[test]
fn iso_is_rejected() {
    let error = render_source(
        "version 1; object b (1, 1) { box at (0, 0, 0) size (1, 1, 1); } render iso; scale 8; place b at (0, 0, 0);",
    )
    .expect_err("iso belongs to another backend");
    assert_eq!(error.code, "E0604");
}

#[test]
fn box_bounds_match_the_formula_in_every_view() {
    let scale = 10.0;
    for (w, d, h) in [(0.5, 0.5, 0.5), (2.0, 1.0, 1.0), (4.3, 2.7, 3.1)] {
        for (view, width, height) in [
            (View::Top, w * scale, d * scale),
            (View::Front, w * scale, h * scale),
            (View::Side, d * scale, h * scale),
        ] {
            let mut min_x = f64::INFINITY;
            let mut min_y = f64::INFINITY;
            let mut max_x = f64::NEG_INFINITY;
            let mut max_y = f64::NEG_INFINITY;
            for x in [0.0, w] {
                for y in [0.0, d] {
                    for z in [0.0, h] {
                        let projected = project_point(view, [x, y, z], scale);
                        min_x = min_x.min(projected[0]);
                        min_y = min_y.min(projected[1]);
                        max_x = max_x.max(projected[0]);
                        max_y = max_y.max(projected[1]);
                    }
                }
            }
            assert!((max_x - min_x - width).abs() < 1e-9);
            assert!((max_y - min_y - height).abs() < 1e-9);
        }
    }
}
