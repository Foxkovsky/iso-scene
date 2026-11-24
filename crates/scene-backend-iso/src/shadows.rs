//! Cast shadows on the optional scene floor.
//!
//! Every instance is treated as a box: its footprint at the bottom and top is
//! pushed away from the sun and the convex hull is clipped to the floor. The
//! result is drawn as a translucent black layer just above `z = 0`, so the
//! floor receives it and the objects cover their own shadow.

use scene_core::SceneGraph;

use crate::floor_extent;

/// Longest projected shadow in blocks, so a low sun cannot explode the polygon.
const MAX_SHADOW_LENGTH: f64 = 80.0;

/// Convex hull of footprint points, in world cells.
fn convex_hull(points: &[[f64; 2]]) -> Vec<[f64; 2]> {
    let mut sorted = points.to_vec();
    sorted.sort_by(|left, right| {
        left[0]
            .total_cmp(&right[0])
            .then(left[1].total_cmp(&right[1]))
    });
    sorted.dedup_by(|left, right| {
        left[0].to_bits() == right[0].to_bits() && left[1].to_bits() == right[1].to_bits()
    });
    if sorted.len() < 3 {
        return sorted;
    }
    let cross = |origin: [f64; 2], a: [f64; 2], b: [f64; 2]| {
        (a[0] - origin[0]) * (b[1] - origin[1]) - (a[1] - origin[1]) * (b[0] - origin[0])
    };
    let mut lower = Vec::new();
    for point in &sorted {
        while lower.len() >= 2
            && cross(lower[lower.len() - 2], lower[lower.len() - 1], *point) <= 0.0
        {
            lower.pop();
        }
        lower.push(*point);
    }
    let mut upper = Vec::new();
    for point in sorted.iter().rev() {
        while upper.len() >= 2
            && cross(upper[upper.len() - 2], upper[upper.len() - 1], *point) <= 0.0
        {
            upper.pop();
        }
        upper.push(*point);
    }
    lower.pop();
    upper.pop();
    lower.extend(upper);
    lower
}

/// Keep the part of a convex polygon inside an axis-aligned rectangle.
fn clip_rect(points: &[[f64; 2]], width: f64, depth: f64) -> Vec<[f64; 2]> {
    let mut polygon = points.to_vec();
    for (inside, intersect) in [
        (
            Box::new(|point: [f64; 2]| point[0] >= 0.0) as Box<dyn Fn([f64; 2]) -> bool>,
            Box::new(|a: [f64; 2], b: [f64; 2]| -> [f64; 2] {
                let t = -a[0] / (b[0] - a[0]);
                [0.0, a[1] + (b[1] - a[1]) * t]
            }) as Box<dyn Fn([f64; 2], [f64; 2]) -> [f64; 2]>,
        ),
        (
            Box::new(|point: [f64; 2]| point[0] <= width) as Box<dyn Fn([f64; 2]) -> bool>,
            Box::new(|a: [f64; 2], b: [f64; 2]| -> [f64; 2] {
                let t = (width - a[0]) / (b[0] - a[0]);
                [width, a[1] + (b[1] - a[1]) * t]
            }) as Box<dyn Fn([f64; 2], [f64; 2]) -> [f64; 2]>,
        ),
        (
            Box::new(|point: [f64; 2]| point[1] >= 0.0) as Box<dyn Fn([f64; 2]) -> bool>,
            Box::new(|a: [f64; 2], b: [f64; 2]| -> [f64; 2] {
                let t = -a[1] / (b[1] - a[1]);
                [a[0] + (b[0] - a[0]) * t, 0.0]
            }) as Box<dyn Fn([f64; 2], [f64; 2]) -> [f64; 2]>,
        ),
        (
            Box::new(|point: [f64; 2]| point[1] <= depth) as Box<dyn Fn([f64; 2]) -> bool>,
            Box::new(|a: [f64; 2], b: [f64; 2]| -> [f64; 2] {
                let t = (depth - a[1]) / (b[1] - a[1]);
                [a[0] + (b[0] - a[0]) * t, depth]
            }) as Box<dyn Fn([f64; 2], [f64; 2]) -> [f64; 2]>,
        ),
    ] {
        if polygon.len() < 3 {
            return Vec::new();
        }
        let mut clipped = Vec::new();
        for index in 0..polygon.len() {
            let current = polygon[index];
            let previous = polygon[(index + polygon.len() - 1) % polygon.len()];
            let current_in = inside(current);
            let previous_in = inside(previous);
            if current_in {
                if !previous_in {
                    clipped.push(intersect(previous, current));
                }
                clipped.push(current);
            } else if previous_in {
                clipped.push(intersect(previous, current));
            }
        }
        polygon = clipped;
    }
    if polygon.len() < 3 {
        Vec::new()
    } else {
        polygon
    }
}

/// Footprint polygons in world cells, with the instance index that owns each.
pub(crate) fn cast_shadows(scene: &SceneGraph) -> Vec<(usize, Vec<[f64; 2]>)> {
    let Some((width, depth)) = floor_extent(scene) else {
        return Vec::new();
    };
    let altitude = scene.sun[1].to_radians();
    if altitude <= 0.02 || scene.shadow.strength <= 0.0 {
        return Vec::new();
    }
    let tangent = altitude.tan().max(1e-6);
    let azimuth = scene.sun[0].to_radians();
    let direction = [-azimuth.cos(), -azimuth.sin()];
    let mut shadows = Vec::new();
    for (owner, instance) in scene.instances.iter().enumerate() {
        let bounds = instance.world_bounds();
        let bottom = bounds.min[2].max(0.0);
        let top = bounds.max[2];
        if top <= bottom {
            continue;
        }
        let mut points = Vec::with_capacity(8);
        for height in [bottom, top] {
            let length = (height / tangent).min(MAX_SHADOW_LENGTH);
            let offset = [direction[0] * length, direction[1] * length];
            for corner in [
                [bounds.min[0], bounds.min[1]],
                [bounds.max[0], bounds.min[1]],
                [bounds.max[0], bounds.max[1]],
                [bounds.min[0], bounds.max[1]],
            ] {
                points.push([corner[0] + offset[0], corner[1] + offset[1]]);
            }
        }
        let clipped = clip_rect(&convex_hull(&points), width, depth);
        if clipped.len() >= 3 {
            shadows.push((owner, clipped));
        }
    }
    shadows
}

#[cfg(test)]
mod tests {
    use super::{clip_rect, convex_hull};

    #[test]
    fn hull_drops_inner_points() {
        let hull = convex_hull(&[[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0], [0.5, 0.5]]);
        assert_eq!(hull.len(), 4);
    }

    #[test]
    fn clipping_keeps_the_rectangle_part() {
        let clipped = clip_rect(
            &[[-1.0, 0.5], [2.0, 0.5], [2.0, 1.5], [-1.0, 1.5]],
            1.0,
            1.0,
        );
        assert!(clipped.iter().all(|point| (0.0..=1.0).contains(&point[0])));
        assert!(clipped.iter().all(|point| (0.0..=1.0).contains(&point[1])));
        assert!(clipped.len() >= 3);
    }
}
