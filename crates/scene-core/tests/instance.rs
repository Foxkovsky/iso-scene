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
