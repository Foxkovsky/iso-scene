//! Placement transforms of `Instance`.

use std::sync::Arc;

use scene_core::{CompiledObject, Instance};

fn instance(rotation: u16, angle: f64) -> Instance {
    Instance {
        object: Arc::new(CompiledObject {
            name: "leaf".into(),
            width: 2.0,
            depth: 1.0,
            parts: Vec::new(),
        }),
        at: [10.0, 20.0, 0.0],
        rotation,
        angle,
    }
}

fn close(a: [f64; 3], b: [f64; 3]) -> bool {
    a.iter().zip(b).all(|(a, b)| (a - b).abs() < 1e-9)
}

#[test]
fn the_extra_yaw_keeps_the_local_origin_fixed_for_every_quarter_turn() {
    for rotation in [0, 90, 180, 270] {
        let still = instance(rotation, 0.0).to_world([0.0, 0.0, 0.0]);
        for angle in [15.0, 45.0, 90.0, -60.0] {
            let turned = instance(rotation, angle).to_world([0.0, 0.0, 0.0]);
            assert!(
                close(still, turned),
                "rot {rotation} angle {angle}: hinge moved from {still:?} to {turned:?}"
            );
        }
    }
}

#[test]
fn to_local_inverts_to_world() {
    for rotation in [0, 90, 180, 270] {
        for angle in [0.0, 30.0, 90.0] {
            let placed = instance(rotation, angle);
            let point = [1.5, 0.25, 0.75];
            let back = placed.to_local(placed.to_world(point));
            assert!(close(point, back), "rot {rotation} angle {angle}: {back:?}");
        }
    }
}

#[test]
fn rotating_a_scene_is_a_rigid_quarter_turn_around_its_centre() {
    let source = r"
        version 1;
        object leaf (2, 1) { box at (0, 0, 0) size (2, 1, 1); }
        render iso;
        scale 1;
        place leaf at (0, 0, 0);
    ";
    let program = scene_syntax::parse(source).expect("parse");
    let here = std::path::Path::new(".");
    let mut scene = scene_core::compile_scene(&program, here, here).expect("compile");
    scene.rotate_z(90);
    let placed = &scene.instances[0];
    assert!((placed.at[0] - 0.5).abs() < 1e-9, "at {:?}", placed.at);
    assert!((placed.at[1] + 0.5).abs() < 1e-9, "at {:?}", placed.at);
    assert_eq!(placed.rotation, 90);
    assert!(
        (scene.bounds.min[0] - 0.5).abs() < 1e-9,
        "{:?}",
        scene.bounds
    );
    assert!(
        (scene.bounds.min[1] + 0.5).abs() < 1e-9,
        "{:?}",
        scene.bounds
    );
    assert!(
        (scene.bounds.max[1] - 1.5).abs() < 1e-9,
        "{:?}",
        scene.bounds
    );
}

#[test]
fn four_quarter_turns_restore_positions_and_rotations() {
    let source = r"
        version 1;
        object leaf (2, 1) { box at (0, 0, 0) size (2, 1, 1); }
        render iso;
        scale 1;
        place leaf at (0, 0, 0);
        place leaf at (10, 0, 0) rot 90;
        place leaf at (5, 8, 2) rot 180 turn 30;
    ";
    let program = scene_syntax::parse(source).expect("parse");
    let here = std::path::Path::new(".");
    let mut scene = scene_core::compile_scene(&program, here, here).expect("compile");
    let before: Vec<_> = scene
        .instances
        .iter()
        .map(|instance| (instance.at, instance.rotation, instance.angle))
        .collect();
    for _ in 0..4 {
        scene.rotate_z(90);
    }
    for (before, after) in before.iter().zip(&scene.instances) {
        assert!(
            close(before.0, after.at),
            "{:?} != {:?}",
            before.0,
            after.at
        );
        assert_eq!(before.1, after.rotation);
        assert!((before.2 - after.angle).abs() < 1e-9);
    }
}

#[test]
fn the_yaw_turns_the_same_way_after_a_quarter_turn() {
    // A point one block along local x swings by the same screen-independent
    // angle around the hinge whatever the quarter turn is.
    for rotation in [0, 90, 180, 270] {
        let hinge = instance(rotation, 0.0).to_world([0.0, 0.0, 0.0]);
        let before = instance(rotation, 0.0).to_world([1.0, 0.0, 0.0]);
        let after = instance(rotation, 90.0).to_world([1.0, 0.0, 0.0]);
        let a = [before[0] - hinge[0], before[1] - hinge[1]];
        let b = [after[0] - hinge[0], after[1] - hinge[1]];
        let cross = a[0] * b[1] - a[1] * b[0];
        assert!((cross - 1.0).abs() < 1e-9, "rot {rotation}: cross {cross}");
    }
}

#[test]
fn a_decal_rotates_with_the_object_that_uses_it() {
    let source = r#"
        version 1;
        object panel (2, 1) {
            decal "decal.png" at (0, 1, 0) size (2, 2) facing +y;
        }
        object rotated (1, 2) {
            use panel at (0, 0, 0) rot 90;
        }
        render iso;
        scale 1;
        place rotated at (0, 0, 0);
    "#;
    let program = scene_syntax::parse(source).expect("parse");
    let here = std::path::Path::new(".");
    let scene = scene_core::compile_scene(&program, here, here).expect("compile");
    let decal = scene.instances[0]
        .object
        .parts
        .iter()
        .find_map(|part| match &part.primitive {
            scene_core::Primitive::Decal { corners, .. } => Some(*corners),
            _ => None,
        })
        .expect("decal");
    assert!(
        decal.iter().all(|corner| corner[0].abs() < 1e-9),
        "{decal:?}"
    );
    assert!(
        decal
            .iter()
            .all(|corner| corner[2] >= 0.0 && corner[2] <= 2.0)
    );
    let y_min = decal
        .iter()
        .map(|corner| corner[1])
        .fold(f64::INFINITY, f64::min);
    let y_max = decal
        .iter()
        .map(|corner| corner[1])
        .fold(f64::NEG_INFINITY, f64::max);
    assert!(
        (y_min - 0.0).abs() < 1e-9 && (y_max - 2.0).abs() < 1e-9,
        "{decal:?}"
    );
}
