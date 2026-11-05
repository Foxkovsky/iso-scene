//! Triangulatable polygon faces for the primitives.
//!
//! Faces are generated in object-local coordinates and include back faces;
//! each backend rasterizes them with its own depth convention and z-buffer.

#![allow(clippy::many_single_char_names)]

use std::f64::consts::PI;

use scene_core::Primitive;

/// A flat polygon with a normal and an outline flag.
#[derive(Clone, Debug, PartialEq)]
pub struct MeshFace {
    pub points: Vec<[f64; 3]>,
    pub normal: [f64; 3],
    pub outline: bool,
}

/// Generate faces for one primitive in local coordinates.
#[must_use]
pub fn primitive_faces(primitive: &Primitive) -> Vec<MeshFace> {
    match primitive {
        Primitive::Box { at, size } => box_faces(*at, *size),
        Primitive::Cyl { at, radius, height } => cylinder_faces(*at, *radius, *height),
        Primitive::Sphere { at, radius } => sphere_faces(*at, *radius),
        Primitive::SlantBox { at, size, offset } => slant_box_faces(*at, *size, *offset),
        Primitive::SlantCyl {
            at,
            radius,
            height,
            offset,
        } => slant_cylinder_faces(*at, *radius, *height, *offset),
        Primitive::Ramp { at, size, rise } => ramp_faces(*at, *size, *rise),
    }
}

fn box_faces(at: [f64; 3], size: [f64; 3]) -> Vec<MeshFace> {
    let [x, y, z] = at;
    let [w, d, h] = size;
    let (x1, y1, z1) = (x + w, y + d, z + h);
    let mut faces = Vec::new();
    add_face(
        &mut faces,
        vec![[x, y, z1], [x1, y, z1], [x1, y1, z1], [x, y1, z1]],
        true,
    );
    add_face(
        &mut faces,
        vec![[x1, y, z], [x1, y1, z], [x1, y1, z1], [x1, y, z1]],
        true,
    );
    add_face(
        &mut faces,
        vec![[x1, y1, z], [x, y1, z], [x, y1, z1], [x1, y1, z1]],
        true,
    );
    add_face(
        &mut faces,
        vec![[x, y, z], [x, y1, z], [x1, y1, z], [x1, y, z]],
        true,
    );
    add_face(
        &mut faces,
        vec![[x, y, z], [x, y, z1], [x, y1, z1], [x, y1, z]],
        true,
    );
    add_face(
        &mut faces,
        vec![[x, y, z], [x1, y, z], [x1, y, z1], [x, y, z1]],
        true,
    );
    faces
}

fn cylinder_faces(at: [f64; 3], radius: f64, height: f64) -> Vec<MeshFace> {
    const SEGMENTS: usize = 48;
    let mut bottom = Vec::with_capacity(SEGMENTS);
    let mut top = Vec::with_capacity(SEGMENTS);
    for segment in 0..SEGMENTS {
        #[allow(clippy::cast_precision_loss)]
        let angle = 2.0 * PI * segment as f64 / SEGMENTS as f64;
        let x = at[0] + radius * angle.cos();
        let y = at[1] + radius * angle.sin();
        bottom.push([x, y, at[2]]);
        top.push([x, y, at[2] + height]);
    }

    let mut faces = Vec::new();
    add_face(&mut faces, top.clone(), true);
    for index in 0..SEGMENTS {
        let next = (index + 1) % SEGMENTS;
        add_face(
            &mut faces,
            vec![bottom[index], bottom[next], top[next], top[index]],
            false,
        );
    }
    faces
}

fn sphere_faces(at: [f64; 3], radius: f64) -> Vec<MeshFace> {
    const RINGS: usize = 8;
    const SEGMENTS: usize = 16;
    let mut faces = Vec::new();
    for ring in 0..RINGS {
        #[allow(clippy::cast_precision_loss)]
        let lat0 = -PI / 2.0 + PI * ring as f64 / RINGS as f64;
        #[allow(clippy::cast_precision_loss)]
        let lat1 = -PI / 2.0 + PI * (ring + 1) as f64 / RINGS as f64;
        for segment in 0..SEGMENTS {
            #[allow(clippy::cast_precision_loss)]
            let lon0 = 2.0 * PI * segment as f64 / SEGMENTS as f64;
            #[allow(clippy::cast_precision_loss)]
            let lon1 = 2.0 * PI * (segment + 1) as f64 / SEGMENTS as f64;
            let points = vec![
                sphere_point(at, radius, lat0, lon0),
                sphere_point(at, radius, lat1, lon0),
                sphere_point(at, radius, lat1, lon1),
                sphere_point(at, radius, lat0, lon1),
            ];
            add_face(&mut faces, points, false);
        }
    }
    faces
}

fn sphere_point(at: [f64; 3], radius: f64, latitude: f64, longitude: f64) -> [f64; 3] {
    [
        at[0] + radius * latitude.cos() * longitude.cos(),
        at[1] + radius * latitude.cos() * longitude.sin(),
        at[2] + radius * latitude.sin(),
    ]
}

fn slant_box_faces(at: [f64; 3], size: [f64; 3], offset: [f64; 2]) -> Vec<MeshFace> {
    let [x, y, z] = at;
    let [w, d, h] = size;
    let bottom = vec![[x, y, z], [x + w, y, z], [x + w, y + d, z], [x, y + d, z]];
    let top = vec![
        [x + offset[0], y + offset[1], z + h],
        [x + w + offset[0], y + offset[1], z + h],
        [x + w + offset[0], y + d + offset[1], z + h],
        [x + offset[0], y + d + offset[1], z + h],
    ];
    prism_faces(&bottom, &top, true)
}

fn slant_cylinder_faces(at: [f64; 3], radius: f64, height: f64, offset: [f64; 2]) -> Vec<MeshFace> {
    const SEGMENTS: usize = 48;
    let mut bottom = Vec::with_capacity(SEGMENTS);
    let mut top = Vec::with_capacity(SEGMENTS);
    for segment in 0..SEGMENTS {
        #[allow(clippy::cast_precision_loss)]
        let angle = 2.0 * PI * segment as f64 / SEGMENTS as f64;
        let x = at[0] + radius * angle.cos();
        let y = at[1] + radius * angle.sin();
        bottom.push([x, y, at[2]]);
        top.push([x + offset[0], y + offset[1], at[2] + height]);
    }

    let mut faces = Vec::new();
    add_face(&mut faces, top.clone(), true);
    for index in 0..SEGMENTS {
        let next = (index + 1) % SEGMENTS;
        add_face(
            &mut faces,
            vec![bottom[index], bottom[next], top[next], top[index]],
            false,
        );
    }
    faces
}

fn ramp_faces(at: [f64; 3], size: [f64; 2], rise: [f64; 2]) -> Vec<MeshFace> {
    let [x, y, z] = at;
    let [w, d] = size;
    let bottom = vec![[x, y, z], [x + w, y, z], [x + w, y + d, z], [x, y + d, z]];
    let top = vec![
        [x, y, z + rise[0]],
        [x + w, y, z + rise[1]],
        [x + w, y + d, z + rise[1]],
        [x, y + d, z + rise[0]],
    ];
    prism_faces(&bottom, &top, true)
}

fn prism_faces(bottom: &[[f64; 3]], top: &[[f64; 3]], outline: bool) -> Vec<MeshFace> {
    let mut faces = Vec::new();
    add_face(&mut faces, top.to_vec(), outline);
    add_face(&mut faces, bottom.to_vec(), outline);
    for index in 0..bottom.len() {
        let next = (index + 1) % bottom.len();
        add_face(
            &mut faces,
            vec![bottom[index], bottom[next], top[next], top[index]],
            outline,
        );
    }
    faces
}

fn add_face(faces: &mut Vec<MeshFace>, points: Vec<[f64; 3]>, outline: bool) {
    let normal = normal(&points);
    if normal[0] * normal[0] + normal[1] * normal[1] + normal[2] * normal[2] < 1e-18 {
        return;
    }
    faces.push(MeshFace {
        points,
        normal,
        outline,
    });
}

fn normal(points: &[[f64; 3]]) -> [f64; 3] {
    let mut normal = [0.0; 3];
    for index in 0..points.len() {
        let a = points[index];
        let b = points[(index + 1) % points.len()];
        normal[0] += (a[1] - b[1]) * (a[2] + b[2]);
        normal[1] += (a[2] - b[2]) * (a[0] + b[0]);
        normal[2] += (a[0] - b[0]) * (a[1] + b[1]);
    }
    normal
}
