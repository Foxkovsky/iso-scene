#![allow(clippy::cast_precision_loss)]

use std::f64::consts::TAU;

use scene_backend_iso::{project_point, view_depth};

fn close(actual: f64, expected: f64) {
    assert!(
        (actual - expected).abs() < 1e-9,
        "expected {expected}, got {actual}"
    );
}

#[test]
fn isometric_axes_have_the_documented_slopes() {
    let scale = 10.0;
    assert_eq!(project_point([0.0, 0.0, 0.0], scale), [0.0, 0.0]);
    assert_eq!(project_point([1.0, 0.0, 0.0], scale), [10.0, 5.0]);
    assert_eq!(project_point([0.0, 1.0, 0.0], scale), [-10.0, 5.0]);
    assert_eq!(project_point([0.0, 0.0, 1.0], scale), [0.0, -10.0]);

    // +X and +Y edges have slope ±1/2; +Z is vertical.
    close((5.0_f64 - 0.0) / (10.0 - 0.0), 0.5);
    close((5.0_f64 - 0.0) / (-10.0 - 0.0), -0.5);
    close((project_point([0.0, 0.0, 1.0], scale)[0] - 0.0) / 10.0, 0.0);
}

#[test]
fn unit_cube_covers_two_by_two_scale_pixels() {
    let scale = 10.0;
    let mut min_x = f64::INFINITY;
    let mut min_y = f64::INFINITY;
    let mut max_x = f64::NEG_INFINITY;
    let mut max_y = f64::NEG_INFINITY;
    for corner in [
        [0.0, 0.0, 0.0],
        [1.0, 0.0, 0.0],
        [0.0, 1.0, 0.0],
        [1.0, 1.0, 0.0],
        [0.0, 0.0, 1.0],
        [1.0, 0.0, 1.0],
        [0.0, 1.0, 1.0],
        [1.0, 1.0, 1.0],
    ] {
        let projected = project_point(corner, scale);
        min_x = min_x.min(projected[0]);
        min_y = min_y.min(projected[1]);
        max_x = max_x.max(projected[0]);
        max_y = max_y.max(projected[1]);
    }
    close(max_x - min_x, 20.0);
    close(max_y - min_y, 20.0);
}

#[test]
fn top_face_is_a_two_to_one_diamond() {
    let scale = 10.0;
    let top = [
        project_point([0.0, 0.0, 1.0], scale),
        project_point([1.0, 0.0, 1.0], scale),
        project_point([1.0, 1.0, 1.0], scale),
        project_point([0.0, 1.0, 1.0], scale),
    ];
    close(top[0][0], 0.0);
    close(top[0][1], -10.0);
    close(top[1][0], 10.0);
    close(top[1][1], -5.0);
    close(top[2][0], 0.0);
    close(top[2][1], 0.0);
    close(top[3][0], -10.0);
    close(top[3][1], -5.0);
}

#[test]
fn horizontal_circle_projects_to_a_two_to_one_ellipse() {
    let scale = 10.0;
    let radius = 2.0;
    let mut min_x = f64::INFINITY;
    let mut min_y = f64::INFINITY;
    let mut max_x = f64::NEG_INFINITY;
    let mut max_y = f64::NEG_INFINITY;
    for step in 0..360 {
        #[allow(clippy::cast_precision_loss)]
        let angle = TAU * f64::from(step) / 360.0;
        let point = [radius * angle.cos(), radius * angle.sin(), 0.0];
        let projected = project_point(point, scale);
        min_x = min_x.min(projected[0]);
        min_y = min_y.min(projected[1]);
        max_x = max_x.max(projected[0]);
        max_y = max_y.max(projected[1]);
    }
    let width = max_x - min_x;
    let height = max_y - min_y;
    close(width / height, 2.0);
    close(width, 2.0 * radius * 2.0_f64.sqrt() * scale);
}

#[test]
fn translation_and_depth_follow_the_formula() {
    let scale = 10.0;
    let base = [1.0, 2.0, 3.0];
    let shift = [2.0, -1.0, 0.5];
    let before = project_point(base, scale);
    let after = project_point(
        [base[0] + shift[0], base[1] + shift[1], base[2] + shift[2]],
        scale,
    );
    close(after[0] - before[0], (shift[0] - shift[1]) * scale);
    close(
        after[1] - before[1],
        shift[0].midpoint(shift[1]) * scale - shift[2] * scale,
    );
    close(
        view_depth([1.0, 2.0, 3.0]) - view_depth([0.0, 0.0, 0.0]),
        6.0,
    );
}

#[test]
fn box_bounds_match_the_formula() {
    let scale = 10.0;
    for (w, d, h) in [(0.5, 0.5, 0.5), (2.0, 1.0, 1.0), (4.3, 2.7, 3.1)] {
        let mut min_x = f64::INFINITY;
        let mut min_y = f64::INFINITY;
        let mut max_x = f64::NEG_INFINITY;
        let mut max_y = f64::NEG_INFINITY;
        for x in [0.0, w] {
            for y in [0.0, d] {
                for z in [0.0, h] {
                    let projected = project_point([x, y, z], scale);
                    min_x = min_x.min(projected[0]);
                    min_y = min_y.min(projected[1]);
                    max_x = max_x.max(projected[0]);
                    max_y = max_y.max(projected[1]);
                }
            }
        }
        close(max_x - min_x, (w + d) * scale);
        close(max_y - min_y, (w.midpoint(d) + h) * scale);
    }
}
