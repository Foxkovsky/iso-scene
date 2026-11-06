use std::path::Path;

use scene_backend_iso::render_svg;
use scene_core::compile_scene;
use scene_syntax::parse;

fn svg(source: &str) -> String {
    let program = parse(source).unwrap_or_else(|error| panic!("{}: {}", error.code, error.message));
    let here = Path::new(".");
    let scene = compile_scene(&program, here, here)
        .unwrap_or_else(|error| panic!("{}: {}", error.code, error.message));
    render_svg(&scene).unwrap_or_else(|error| panic!("{}: {}", error.code, error.message))
}

#[test]
fn a_box_is_written_as_one_polygon_per_face() {
    let document = svg(r"
        version 1;
        object box1 (1, 1) { box at (0, 0, 0) size (1, 1, 1) material wood; }
        render iso;
        scale 32;
        background #202226;
        place box1 at (0, 0, 0);
        ");

    assert!(document.starts_with("<svg"));
    assert!(document.contains("width=\""));
    assert!(document.contains("viewBox=\"0 0 "));
    assert_eq!(document.matches("<polygon ").count(), 6);
    assert!(document.contains("fill=\"#b0b2b6\""));
    assert!(document.contains("stroke=\"#26282e\""));
    assert!(document.ends_with("</svg>\n"));
}

#[test]
fn the_background_colour_becomes_a_rect() {
    let document = svg(r"
        version 1;
        object box1 (1, 1) { box at (0, 0, 0) size (1, 1, 1) material wood; }
        render iso;
        scale 32;
        background #202226;
        place box1 at (0, 0, 0);
        ");
    assert!(document.contains("<rect width=\"100%\" height=\"100%\" fill=\"#202226\"/>"));
}

#[test]
fn a_transparent_background_has_no_rect() {
    let document = svg(r"
        version 1;
        object box1 (1, 1) { box at (0, 0, 0) size (1, 1, 1) material wood; }
        render iso;
        scale 32;
        place box1 at (0, 0, 0);
        ");
    assert!(!document.contains("<rect"));
}

#[test]
fn the_same_scene_writes_the_same_document() {
    let source = r"
        version 1;
        object box1 (2, 2) {
            box at (0, 0, 0) size (1, 1, 1) material wood;
            box at (1, 1, 0) size (1, 1, 1) material metal;
        }
        render iso;
        scale 24;
        background #1a1c22;
        place box1 at (0, 0, 0);
    ";
    assert_eq!(svg(source), svg(source));
}

#[test]
fn a_handle_behind_a_panel_is_written_before_it() {
    // The door from the old compiler: a thin panel with one handle behind and
    // one in front. The back handle must not end up on top of the panel.
    let document = svg(r"
        version 1;
        object panel (2, 2) { box at (0, 0, 0) size (2, 0.2, 2) material wood; }
        object back (1, 1)  { box at (0, 0, 0) size (0.4, 0.2, 0.4) material red; }
        render iso;
        scale 32;
        place panel at (0, 0, 0);
        place back at (0.8, -0.4, 0.8);
        ");

    let polygons: Vec<&str> = document
        .split("<polygon ")
        .skip(1)
        .map(|polygon| polygon.split("/>").next().unwrap_or_default())
        .collect();
    let back_at = polygons
        .iter()
        .position(|polygon| polygon.contains("#a4181a"))
        .expect("the back handle is written");
    let panel_at = polygons
        .iter()
        .position(|polygon| polygon.contains("#b0b2b6"))
        .expect("the panel is written");
    assert!(
        back_at < panel_at,
        "the back handle must be painted before the panel: {back_at} < {panel_at}"
    );
}

#[test]
fn an_orthographic_scene_is_rejected() {
    let program = parse(
        r"
        version 1;
        object box1 (1, 1) { box at (0, 0, 0) size (1, 1, 1) material wood; }
        render top;
        scale 16;
        place box1 at (0, 0, 0);
        ",
    )
    .expect("parse");
    let here = Path::new(".");
    let scene = compile_scene(&program, here, here).expect("compile");
    let error = render_svg(&scene).expect_err("top view has no iso SVG");
    assert_eq!(error.code, "E0604");
}

#[test]
fn merge_hides_the_seam_between_same_colour_parts() {
    let merged = svg(r"
        version 1;
        object bench (2, 1) {
            merge {
                box from (0, 0, 0) to (1, 1, 0.5) material wood;
                box from (1, 0, 0) to (2, 1, 0.5) material wood;
            }
        }
        render iso;
        scale 32;
        place bench at (0, 0, 0);
        ");
    let loose = svg(r"
        version 1;
        object bench (2, 1) {
            box from (0, 0, 0) to (1, 1, 0.5) material wood;
            box from (1, 0, 0) to (2, 1, 0.5) material wood;
        }
        render iso;
        scale 32;
        place bench at (0, 0, 0);
        ");
    assert!(merged.contains("<line "), "merged faces are drawn as edges");
    assert!(!loose.contains("<line "), "loose parts keep their outlines");
}
